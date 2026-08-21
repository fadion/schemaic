//! A **pinned connection** for manual-transaction mode.
//!
//! Everywhere else in this crate a `Db` operation opens a fresh connection, runs,
//! and disconnects — the app stays stateless and a dropped connection is never a
//! problem. That model can't express a user-driven transaction: `BEGIN` and
//! `COMMIT` are separate UI actions, minutes apart, with statements in between.
//!
//! A [`Session`] is the deliberate exception. It owns exactly one open connection
//! behind a `tokio::Mutex` — the mutex is what makes it usable as `Arc<Session>`
//! from spawned tasks, and it also serialises statements onto the connection,
//! which the driver requires anyway. One session belongs to one query tab, for as
//! long as that tab is in Manual mode.
//!
//! What runs here is the tab's own work: queries, batches, grid writes, and the
//! re-fetch after a write (which *must* be on this connection — it's the only one
//! that can see uncommitted rows). Read-only side channels — schema
//! introspection, live-validate, EXPLAIN, the Live Monitor, AI/MCP — deliberately
//! keep using fresh connections, so a long-running transaction never blocks them.
//!
//! The session reports a [`StmtOutcome`] per operation and the app folds that
//! into [`schemaic_core::tx::TxState`], which is the *display* model — what the
//! pill says and which buttons are live.
//!
//! The one piece of transaction state the session owns itself is whether a
//! transaction is open ([`Session::ensure_tx`]). That can't live in `TxState`,
//! because `TxState` is only folded when an operation *completes*: two runs
//! started close together would both read "no transaction" and both issue a
//! `BEGIN`, and on MySQL the second one implicitly commits the first's writes.

use std::sync::Arc;

use mysql_async::Conn;
use mysql_async::prelude::Queryable;
use schemaic_core::model::{GridWrite, RefetchRow, RefetchTemplate, ResultSet, Value};
use schemaic_core::tx::{self, StmtOutcome};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Mutex;
use tokio_postgres::{Client, NoTls};
use tokio_util::sync::CancellationToken;

use crate::{Db, DbError, Engine, TxScope, collect_rows, pg, refetch_on, write_on};

/// An operation's result plus what it means for the enclosing transaction.
///
/// The two are separate on purpose: `Err(Query(..))` on MySQL leaves the
/// transaction usable, the same error on PostgreSQL aborts it, and either engine
/// can hand back an error that actually means "the connection is gone". The
/// session is the only place that can tell those apart (it can probe the
/// connection), so it does, and the caller just folds the verdict in.
pub struct Outcome<T> {
    pub result: Result<T, DbError>,
    pub stmt: StmtOutcome,
}

impl<T> Outcome<T> {
    fn ok(value: T) -> Self {
        Outcome {
            result: Ok(value),
            stmt: StmtOutcome::Ok,
        }
    }
}

enum Backend {
    MySql { conn: Conn, conn_id: u32 },
    Postgres { client: Client },
}

/// One pinned connection, owned by one query tab in Manual mode.
pub struct Session {
    /// Kept so cancellation can open a *second* connection to `KILL QUERY` the
    /// pinned one (MySQL); the pinned connection is busy running the statement
    /// we're trying to kill.
    db: Db,
    database: Option<String>,
    /// The database statements on this connection are *currently* scoped to —
    /// the pinned one until a `USE` moves it.
    ///
    /// Separate from [`database`](Self::database), which is what the connection
    /// was opened with and what PostgreSQL's path still addresses. Only the
    /// **label** follows a `USE`: on MySQL the server really does change scope
    /// mid-session, and stamping the immutable pinned name meant a result that
    /// ran in `sakila` reported `world`. Written only while `inner` is held, so
    /// it can't race a statement.
    scope: std::sync::Mutex<Option<String>>,
    /// The server's own id for this connection — MySQL's thread id, PostgreSQL's
    /// backend pid. Captured once at open, because it is what lets the app
    /// recognise **its own** pinned session in the Server Activity panel's list
    /// and repair the tab when that session is killed from there. Without it a
    /// kill leaves the tab holding a dead connection it has no way to notice.
    ///
    /// `None` only if the server declined to say, which neither engine does.
    server_id: Option<i64>,
    inner: Mutex<Backend>,
    /// Whether a transaction is currently open on this connection.
    ///
    /// **Only ever read or written while `inner` is held** — that is what makes
    /// "is one open?" and the `BEGIN` a single atomic step; the atomic type is
    /// only so it can be mutated through `&self`. Owning this here rather than
    /// consulting the UI's `TxState` is the whole point: two operations started
    /// close together both read a state that isn't folded until each completes,
    /// so both decided they had to `BEGIN`, and on MySQL a second `BEGIN`
    /// implicitly commits the first one's writes.
    in_tx: AtomicBool,
}

impl Session {
    /// Open the pinned connection. No transaction is started yet —
    /// [`Session::ensure_tx`] opens one lazily on the tab's first statement, so
    /// flipping to Manual and then changing your mind costs nothing on the
    /// server.
    pub async fn open(db: &Db, database: Option<&str>) -> Result<Arc<Session>, DbError> {
        // Paired, because the id is a by-product of opening and each engine
        // learns it a different way — MySQL from the handshake, PostgreSQL from a
        // question.
        let (backend, server_id) = match db.engine() {
            Engine::MySql => {
                // `client_found_rows` matters: the write path's exactly-one-row
                // guard counts *matched* rows, not *changed* ones. A pinned
                // connection serves reads and writes both, so it has to be on
                // here or the safety net quietly changes meaning.
                let conn = db.open(database, true).await?;
                let conn_id = conn.id();
                (Backend::MySql { conn, conn_id }, Some(i64::from(conn_id)))
            }
            Engine::Postgres => {
                // Postgres has no server-level connection; a session is bound to
                // one database for its whole life. That's also why changing a
                // tab's database with a transaction open has to prompt.
                let client = match database {
                    Some(d) => pg::connect_to(db, d).await?,
                    None => pg::connect_maintenance(db).await?,
                };
                // Best effort — a session whose pid we failed to learn still
                // works, it just can't be recognised as ours later.
                let pid = pg::backend_pid(&client).await;
                (Backend::Postgres { client }, pid)
            }
            // **SQLite has no manual-transaction mode here, deliberately.**
            //
            // Not because the engine lacks transactions — it has them — but
            // because a pinned SQLite connection is a different concurrency
            // structure than this type is: `rusqlite::Connection` is blocking and
            // `!Sync`, so holding one across the awaits this `Mutex<Backend>` is
            // built around means owning a dedicated thread and talking to it over
            // a channel. That is worth building deliberately, not as a side
            // effect of adding an engine.
            //
            // Refusing here rather than silently running the tab's statements on
            // fresh connections is the whole point: Manual mode's promise is that
            // nothing commits until the user says so, and a mode that quietly
            // auto-committed would break exactly that promise. The UI doesn't
            // offer the mode for a SQLite connection, and this is the backstop for
            // any path that reaches it anyway.
            Engine::Sqlite => {
                return Err(DbError::Connect(
                    "SQLite connections don't support manual transaction mode yet — \
                     statements run and commit as they are sent"
                        .to_string(),
                ));
            }
        };
        Ok(Arc::new(Session {
            db: db.clone(),
            database: database.map(|s| s.to_string()),
            scope: std::sync::Mutex::new(database.map(|s| s.to_string())),
            server_id,
            inner: Mutex::new(backend),
            in_tx: AtomicBool::new(false),
        }))
    }

    /// The server's id for this pinned connection — see
    /// [`server_id`](Self::server_id). Matches the `id` on a
    /// [`SessionInfo`](schemaic_core::activity::SessionInfo) row, which is how
    /// the app tells its own session apart from everyone else's.
    pub fn server_id(&self) -> Option<i64> {
        self.server_id
    }

    pub fn engine(&self) -> Engine {
        self.db.engine()
    }

    /// The database this session is pinned to.
    pub fn database(&self) -> Option<&str> {
        self.database.as_deref()
    }

    /// Run a bare control statement (`BEGIN` / `COMMIT` / `ROLLBACK`) on the
    /// pinned connection.
    async fn control(&self, sql: &str) -> Result<(), DbError> {
        let mut guard = self.inner.lock().await;
        Session::control_on(&mut guard, sql).await
    }

    /// The body of [`Session::control`], for callers already holding the lock.
    async fn control_on(guard: &mut Backend, sql: &str) -> Result<(), DbError> {
        match &mut *guard {
            Backend::MySql { conn, .. } => conn
                .query_drop(sql)
                .await
                .map_err(|e| DbError::Query(e.to_string())),
            Backend::Postgres { client } => client
                .batch_execute(sql)
                .await
                .map_err(|e| DbError::Query(e.to_string())),
        }
    }

    /// Open a transaction unless this connection already has one.
    ///
    /// **The decision is taken under the same lock the `BEGIN` goes out on**, so
    /// two operations racing to start the tab's transaction cannot both issue
    /// one. Callers must use this rather than deciding for themselves from
    /// `TxState` — that signal is not folded until an operation *completes*, so
    /// while one is in flight every other caller reads it as "no transaction".
    ///
    /// The error is returned, not logged: running the statement outside the
    /// transaction is exactly the outcome Manual mode exists to prevent, so the
    /// operation must abort instead.
    pub async fn ensure_tx(&self) -> Result<(), DbError> {
        let mut guard = self.inner.lock().await;
        if self.in_tx.load(Ordering::SeqCst) {
            return Ok(());
        }
        Session::control_on(&mut guard, "BEGIN").await?;
        self.in_tx.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Both this and [`Session::rollback`] clear the flag even when the server
    /// rejects the statement. A redundant `BEGIN` on a connection with no
    /// transaction is harmless; skipping a needed one is not, so on doubt the
    /// flag errs toward "there is no transaction".
    pub async fn commit(&self) -> Result<(), DbError> {
        let r = self.control("COMMIT").await;
        self.in_tx.store(false, Ordering::SeqCst);
        r
    }

    /// Roll the transaction back. Also the way out of a PostgreSQL transaction
    /// that a failed statement aborted (`25P02`).
    pub async fn rollback(&self) -> Result<(), DbError> {
        let r = self.control("ROLLBACK").await;
        self.in_tx.store(false, Ordering::SeqCst);
        r
    }

    /// Close the pinned connection. Any open transaction is rolled back by the
    /// server when the connection drops, but callers should `rollback()` first so
    /// the intent is explicit rather than implied by a disconnect.
    pub async fn close(&self) {
        let mut guard = self.inner.lock().await;
        self.in_tx.store(false, Ordering::SeqCst);
        match &mut *guard {
            Backend::MySql { conn, .. } => {
                // `Conn::disconnect` consumes the connection, which we can't do
                // through a borrow — a quiet reset leaves the server tidy and the
                // real teardown happens when the `Session` drops.
                let _ = conn.query_drop("ROLLBACK").await;
            }
            Backend::Postgres { .. } => {
                // Dropping the client ends the connection task.
            }
        }
    }

    /// Is the pinned connection still alive? Used to tell "the statement failed"
    /// apart from "the connection died under us" — an idle-in-transaction timeout
    /// or a server restart looks like an ordinary query error otherwise, and
    /// silently reconnecting would hand the user a fresh, empty transaction while
    /// their work was already discarded.
    async fn alive(guard: &mut Backend) -> bool {
        match guard {
            Backend::MySql { conn, .. } => conn.ping().await.is_ok(),
            Backend::Postgres { client } => !client.is_closed(),
        }
    }

    /// Classify a finished operation for the transaction state machine.
    async fn classify<T>(guard: &mut Backend, result: Result<T, DbError>) -> Outcome<T> {
        let stmt = match &result {
            Ok(_) => StmtOutcome::Ok,
            Err(DbError::Cancelled) => StmtOutcome::Cancelled,
            Err(_) => {
                if Session::alive(guard).await {
                    StmtOutcome::Failed
                } else {
                    StmtOutcome::ConnectionLost
                }
            }
        };
        Outcome { result, stmt }
    }

    /// Classify a finished operation that ran inside its own `SAVEPOINT`.
    ///
    /// A plain failure here is *isolated*: the write path rolls back to its own
    /// savepoint, which on PostgreSQL clears the aborted state, so the user's
    /// transaction is untouched and still committable. Folding it as a bare
    /// [`StmtOutcome::Failed`] is what made the pill read "Tx aborted" and left
    /// Rollback — which discards everything — as the only enabled action.
    ///
    /// The recoverability is **confirmed, not assumed**. `write_on`'s own
    /// rollback is best-effort (`let _ =`), so this re-issues it and claims
    /// isolation only if it succeeds; `ROLLBACK TO SAVEPOINT` does not release
    /// the savepoint, so doing it twice is harmless. A cancellation is left
    /// alone: the query was killed mid-flight, so nothing guarantees the
    /// savepoint rollback ran.
    async fn classify_isolated<T>(guard: &mut Backend, result: Result<T, DbError>) -> Outcome<T> {
        let mut out = Session::classify(guard, result).await;
        if out.stmt == StmtOutcome::Failed && Session::undo_savepoint(guard).await {
            out.stmt = StmtOutcome::FailedIsolated;
        }
        out
    }

    /// Roll back to the write savepoint, reporting whether the server accepted
    /// it — which is the direct evidence that the transaction is alive.
    async fn undo_savepoint(guard: &mut Backend) -> bool {
        let sql = TxScope::Savepoint.rollback_sql();
        match guard {
            Backend::MySql { conn, .. } => conn.query_drop(sql).await.is_ok(),
            Backend::Postgres { client } => client.batch_execute(sql).await.is_ok(),
        }
    }

    /// Run one statement on the pinned connection, inside the open transaction.
    pub async fn fetch_query(
        &self,
        sql: &str,
        row_cap: usize,
        cancel: CancellationToken,
    ) -> Outcome<ResultSet> {
        let mut guard = self.inner.lock().await;
        let result = match &mut *guard {
            Backend::MySql { conn, conn_id } => {
                let conn_id = *conn_id;
                tokio::select! {
                    // `early_stop = false`: the connection outlives this
                    // statement, so a truncated result must be drained fully to
                    // leave it clean for the next one.
                    r = collect_rows(conn, sql, row_cap, false) => r,
                    _ = cancel.cancelled() => {
                        self.db.kill_query(conn_id).await;
                        Err(DbError::Cancelled)
                    }
                }
            }
            Backend::Postgres { client } => {
                let token = client.cancel_token();
                let db = self.database.as_deref().unwrap_or("");
                tokio::select! {
                    r = pg::run_statement(client, db, sql, row_cap, &cancel) => r,
                    _ = cancel.cancelled() => {
                        let _ = token.cancel_query(NoTls).await;
                        Err(DbError::Cancelled)
                    }
                }
            }
        };
        // Same stamp `Db::fetch_query` applies, from this session's own scope —
        // which is the one the statement ran under, and need not be the tab's
        // current selection. See `ResultSet::database`.
        //
        // The scope **follows a `USE`**, and the stamp is applied after it, so a
        // `USE` labels its own result with the database it moved to. Stamping
        // the immutable pinned name meant every statement after one reported the
        // database it was no longer running in. `sql::use_target` is
        // conservative: a `USE` it can't read plainly drops the label rather than
        // carrying a name that is now certainly wrong.
        if result.is_ok() {
            let dialect = self.db.engine().dialect();
            if schemaic_core::sql::leading_keyword(sql, dialect).as_deref() == Some("USE")
                && let Ok(mut scope) = self.scope.lock()
            {
                *scope = schemaic_core::sql::use_target(sql, dialect);
            }
        }
        let scope = self.scope.lock().ok().and_then(|s| s.clone());
        let result = result.map(|mut rs| {
            rs.database = scope;
            rs
        });
        // MySQL DDL commits the open transaction out from under us, so the flag
        // has to follow — otherwise the next statement would skip its `BEGIN`
        // and run auto-committed. `tx::tx_open_after` is that decision, pure and
        // tested, and it is built on the same `implicit_commit` the pill folds
        // on, so the session's view and the pill's can't drift. `None` = the
        // statement changed nothing about whether a transaction is open.
        if result.is_ok()
            && let Some(open) = tx::tx_open_after(self.tx_engine(), sql)
        {
            self.in_tx.store(open, Ordering::SeqCst);
        }
        Session::classify(&mut guard, result).await
    }

    /// This session's engine, in the vocabulary `schemaic_core::tx` speaks.
    ///
    /// SQLite can't reach here — [`Session::open`] refuses it — but the arm has to
    /// answer something, and MySQL's is the safe reading of the two: it assumes a
    /// statement may have implicitly committed, where Postgres' assumes the
    /// transaction is poisoned and only a rollback escapes. Claiming the stricter
    /// one for an engine we never opened would tell a user their transaction had
    /// died when there was no transaction at all.
    fn tx_engine(&self) -> tx::TxEngine {
        match self.db.engine() {
            Engine::Postgres => tx::TxEngine::Postgres,
            Engine::MySql | Engine::Sqlite => tx::TxEngine::MySql,
        }
    }

    // There's deliberately no `run_batch` here. The fresh-connection one collapses
    // a batch into a single call, but inside a transaction each statement's
    // outcome has to be folded *individually and in order* — a MySQL DDL halfway
    // through implicitly commits, and the statements after it belong to a new
    // transaction. The caller drives `fetch_query` in a loop instead, which keeps
    // that ordering visible where the state machine lives.

    /// Apply staged grid edits inside the user's transaction.
    ///
    /// Nested under a savepoint, so the exactly-one-row guard rolls back its own
    /// batch without ending the transaction the user is building. On PostgreSQL
    /// the savepoint is also what keeps a failed batch recoverable.
    pub async fn commit_writes(
        &self,
        write: &GridWrite,
        cancel: CancellationToken,
    ) -> Outcome<u64> {
        if write.is_empty() {
            return Outcome::ok(0);
        }
        let mut guard = self.inner.lock().await;
        let result = match &mut *guard {
            Backend::MySql { conn, conn_id } => {
                let conn_id = *conn_id;
                tokio::select! {
                    r = write_on(conn, write, TxScope::Savepoint) => r,
                    _ = cancel.cancelled() => {
                        self.db.kill_query(conn_id).await;
                        Err(DbError::Cancelled)
                    }
                }
            }
            Backend::Postgres { client } => {
                let token = client.cancel_token();
                tokio::select! {
                    r = pg::write_on(client, write, TxScope::Savepoint) => r,
                    _ = cancel.cancelled() => {
                        let _ = token.cancel_query(NoTls).await;
                        Err(DbError::Cancelled)
                    }
                }
            }
        };
        Session::classify_isolated(&mut guard, result).await
    }

    /// Re-read just-edited rows **on this connection** — the only one that can
    /// see rows written inside the open transaction.
    pub async fn refetch_rows(
        &self,
        template: &RefetchTemplate,
        rows: &[RefetchRow],
        cancel: CancellationToken,
    ) -> Outcome<Vec<(usize, Vec<Value>)>> {
        if rows.is_empty() {
            return Outcome::ok(Vec::new());
        }
        let mut guard = self.inner.lock().await;
        let result = match &mut *guard {
            Backend::MySql { conn, conn_id } => {
                let conn_id = *conn_id;
                tokio::select! {
                    r = refetch_on(conn, template, rows) => r,
                    _ = cancel.cancelled() => {
                        self.db.kill_query(conn_id).await;
                        Err(DbError::Cancelled)
                    }
                }
            }
            Backend::Postgres { client } => {
                let token = client.cancel_token();
                tokio::select! {
                    r = pg::refetch_on(client, template, rows) => r,
                    _ = cancel.cancelled() => {
                        let _ = token.cancel_query(NoTls).await;
                        Err(DbError::Cancelled)
                    }
                }
            }
        };
        Session::classify(&mut guard, result).await
    }
}
