//! Database access for Schemaic.
//!
//! Connect, run a statement over the MySQL **text protocol**, and stop at a row
//! cap (ARCHITECTURE §8). The query runs on a **dedicated connection** whose id
//! we capture up front, so it can be cancelled server-side with `KILL QUERY`
//! from a second connection (ARCHITECTURE §7).
//!
//! Built on [`mysql_async`] (not sqlx): we need the per-column wire metadata —
//! `org_table` / `org_name` / key flags — that the MySQL protocol sends in every
//! column-definition packet, which is the foundation of the editing system.
//! sqlx's MySQL driver parses that packet but keeps only the alias name + type,
//! so it can't tell which real table/column a result cell came from.

pub mod pg;
pub mod session;
pub mod sqlite;
pub mod ssh;
mod tls;

pub use session::{Outcome, Session};

use std::collections::HashMap;

use futures_util::StreamExt;
use mysql_async::consts::{ColumnFlags, ColumnType};
use mysql_async::prelude::Queryable;
use mysql_async::{Column as MyColumn, Conn, Row, Value as MyValue};
use mysql_async::{OptsBuilder, Params};
use schemaic_core::activity::{self, KillKind, SessionInfo};
use schemaic_core::blob::{BlobRef, BlobValue, FETCH_CAP};
use schemaic_core::export;
use schemaic_core::intel::SqlDialect;
use schemaic_core::model::{
    CellEdit, Column, ColumnFlags as CoreColFlags, ColumnOrigin, GridWrite, RefetchRow,
    RefetchTemplate, ResultBuilder, ResultSet, Rollback, RowDelete, RowEdit, RowInsert, Value,
    WriteStep, binary_display, one_row_verdict,
};
use schemaic_core::schema::{
    CheckInfo, ColumnInfo, DbSchema, EventInfo, EventSchedule, EventSource, EventStatus,
    ForeignKeyInfo, IndexInfo, RoutineInfo, TableInfo, TriggerAction, TriggerEvent, TriggerInfo,
    TriggerOrder, TriggerSource, TriggerTiming, ViewOptions, event_interval_expr, event_time_expr,
};
use schemaic_core::sql;
use schemaic_core::stats::{Freshness, IndexStats, SchemaStats, TableStats, count_rows_sql};
use schemaic_core::users::{self, Grants, MyUserRow, Principal};
use tokio_util::sync::CancellationToken;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("connection failed: {0}")]
    Connect(String),
    #[error("query failed: {0}")]
    Query(String),
    #[error("query cancelled")]
    Cancelled,
}

/// One block of an export on its way from the server to the file — or the reason
/// there will be no more.
///
/// The error rides the **channel** rather than only being returned from
/// [`Db::stream_query`], because the writer is on the other end of it and would
/// otherwise see a closed channel and call the file finished. A partial export
/// that reports success is the failure mode this whole path exists to avoid.
pub type ExportChunk = Result<ResultSet, String>;

/// Where a row loop puts the rows it reads.
///
/// The three engines each have one row loop, and each is the product of a long
/// argument with its driver — PostgreSQL's is `simple_query_raw` precisely so the
/// cap can apply before the result materialises, MySQL's chooses per caller
/// whether to abandon or drain the stream, SQLite's is the blocking half of a
/// `spawn_blocking`. **Streaming a whole table is a second destination for those
/// rows, not a second way to read them**, so it is a parameter here rather than
/// three more loops that would have to be kept in step with the originals.
pub(crate) enum RowDest {
    /// Accumulate into one [`ResultSet`], stopping at this many rows and
    /// reporting `truncated` — every ordinary query.
    Capped(usize),
    /// No cap: hand over every `chunk` rows as their own [`ResultSet`], so an
    /// export reaches the disk as it comes off the wire and memory stays bounded
    /// by one chunk. `sent` counts what went out.
    Chunked {
        chunk: usize,
        tx: tokio::sync::mpsc::Sender<ExportChunk>,
        sent: u64,
    },
}

/// How much **text** one streamed chunk may hold before it is handed over,
/// whatever its row count.
///
/// 32 MiB, and the figure is chosen against the pipeline rather than against a
/// row: up to four chunks are in flight at once (one filling, two queued on the
/// bounded channel, one rendering), so this is a ~128 MiB ceiling on the
/// export's own footprint — the "megabytes rather than gigabytes" the row budget
/// claimed and could not deliver. Well above any per-row cost, so an ordinary
/// narrow table still flushes on its row count and pays nothing for this.
///
/// It bounds the *arena* — the cell text — and not the whole `ResultSet`, which
/// also carries one offset word per cell. That part is proportional to
/// `rows × columns` and is already bounded by the row count.
pub(crate) const CHUNK_BYTE_BUDGET: usize = 32 * 1024 * 1024;

impl RowDest {
    /// The row cap to stop at. A stream has none — the point of it — and
    /// `usize::MAX` says so without every loop growing a second branch around
    /// the comparison it already makes.
    pub(crate) fn cap(&self) -> usize {
        match self {
            RowDest::Capped(cap) => *cap,
            RowDest::Chunked { .. } => usize::MAX,
        }
    }

    /// Has the builder filled a chunk — **by rows or by bytes**? Always false for
    /// [`RowDest::Capped`], which flushes nothing.
    ///
    /// The row count alone was a budget in the wrong unit. A chunk is
    /// `chunk × the row width` and nothing bounds a row's width: the channel
    /// holds two, the loop is filling a third and the writer is rendering a
    /// fourth, so a table of 1 MB documents put ~40 GB in flight against a
    /// constant whose own doc promised "megabytes rather than gigabytes". The
    /// only thing that stopped it was the per-column 512 MiB arena ceiling, and
    /// hitting *that* is the data loss `ExportTally::blanked` now reports.
    ///
    /// So a chunk also ends when its text passes [`CHUNK_BYTE_BUDGET`], which
    /// makes the promise true for any row width: the block goes out smaller and
    /// more often instead of larger.
    pub(crate) fn chunk_full(&self, rows: usize, bytes: usize) -> bool {
        matches!(
            self,
            RowDest::Chunked { chunk, .. } if rows >= *chunk || bytes >= CHUNK_BYTE_BUDGET
        )
    }

    /// How much room to give the next chunk's per-column buffers — the chunk
    /// size for a stream, nothing for a capped read, which never starts a second
    /// builder.
    pub(crate) fn chunk_capacity(&self) -> usize {
        match self {
            RowDest::Capped(_) => 0,
            RowDest::Chunked { chunk, .. } => *chunk,
        }
    }

    /// Rows handed to the channel so far.
    pub(crate) fn sent(&self) -> u64 {
        match self {
            RowDest::Capped(_) => 0,
            RowDest::Chunked { sent, .. } => *sent,
        }
    }

    /// Take the rows built so far and send them, from an **async** loop
    /// (MySQL, PostgreSQL). A no-op for [`RowDest::Capped`].
    ///
    /// The channel is bounded, so this is also the backpressure: a server faster
    /// than the disk waits here instead of queueing the table in memory.
    pub(crate) async fn flush(
        &mut self,
        builder: &mut ResultBuilder,
        next_capacity: usize,
    ) -> Result<(), DbError> {
        let RowDest::Chunked { tx, sent, .. } = self else {
            return Ok(());
        };
        let rs = builder.take_chunk(next_capacity);
        *sent += rs.row_count() as u64;
        tx.send(Ok(rs)).await.map_err(|_| writer_gone())
    }

    /// [`Self::flush`] from a **blocking** loop (SQLite, which runs inside
    /// `spawn_blocking`). `blocking_send` panics on a runtime thread, so the two
    /// cannot be one method.
    pub(crate) fn flush_blocking(
        &mut self,
        builder: &mut ResultBuilder,
        next_capacity: usize,
    ) -> Result<(), DbError> {
        let RowDest::Chunked { tx, sent, .. } = self else {
            return Ok(());
        };
        let rs = builder.take_chunk(next_capacity);
        *sent += rs.row_count() as u64;
        tx.blocking_send(Ok(rs)).map_err(|_| writer_gone())
    }
}

/// The receiver hung up: the file writer failed or the export was abandoned.
/// Reported as a query error so the row loop stops rather than reading a table
/// nobody is writing down.
fn writer_gone() -> DbError {
    DbError::Query("the export stopped reading".to_string())
}

/// The binary collation id (`binary`) — a column with this charset holds raw
/// bytes (BLOB/BINARY/VARBINARY) rather than text.
const BINARY_CHARSET: u16 = 63;

/// Which database engine a [`Db`] speaks. Selected from the saved connection's
/// `db_type` at [`Db::connect`] time; each public method dispatches to the
/// engine-specific backend (MySQL bodies inline here, Postgres in [`pg`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Engine {
    #[default]
    MySql,
    Postgres,
    Sqlite,
}

impl Engine {
    /// The SQL dialect this engine speaks — the quoting and escaping rules any
    /// generated statement has to follow.
    pub fn dialect(self) -> schemaic_core::intel::SqlDialect {
        match self {
            Engine::MySql => schemaic_core::intel::SqlDialect::MySql,
            Engine::Postgres => schemaic_core::intel::SqlDialect::Postgres,
            Engine::Sqlite => schemaic_core::intel::SqlDialect::Sqlite,
        }
    }

    /// A stable lowercase tag for this engine — used to serialize the engine into
    /// the MCP endpoint JSON (round-trips through [`Engine::from_db_type`]).
    pub fn as_str(self) -> &'static str {
        match self {
            Engine::MySql => "mysql",
            Engine::Postgres => "postgres",
            Engine::Sqlite => "sqlite",
        }
    }

    /// Is this engine reached over the network — i.e. does a host, a port, a user,
    /// a password or an SSH tunnel mean anything for it?
    ///
    /// SQLite is the one that answers `false`, and it is worth a predicate rather
    /// than an `== Engine::Sqlite` at each site because the *question* is what the
    /// callers actually have: whether to open a tunnel, whether to show a port
    /// field, whether a credential is worth keyring space.
    ///
    /// Delegated to [`schemaic_core::connection::is_networked`], which the
    /// connection form can also reach — the two used to answer separately.
    pub fn is_networked(self) -> bool {
        schemaic_core::connection::is_networked(self.as_str())
    }

    /// Map a saved connection's `db_type` label to an engine. Anything that isn't
    /// recognizably Postgres or SQLite falls back to MySQL (the historical
    /// default), so old saved connections and the "MySQL"/"MariaDB" labels keep
    /// working.
    ///
    /// Delegates to [`schemaic_core::connection`]'s predicates — they own the
    /// aliases, and a label that meant SQLite to the connection list and MySQL to
    /// the driver would open a TCP socket for a file path.
    pub fn from_db_type(db_type: &str) -> Engine {
        if schemaic_core::connection::is_postgres(db_type) {
            Engine::Postgres
        } else if schemaic_core::connection::is_sqlite(db_type) {
            Engine::Sqlite
        } else {
            Engine::MySql
        }
    }
}

/// A resolved connection target — server coordinates + credentials, already
/// pointed through any established SSH tunnel. Built once from a saved
/// [`schemaic_core::connection::Connection`]; every operation derives a fresh
/// `mysql_async` connection from it.
///
/// This is the app's single connection *identity* (review §3.1): the app threads
/// a `Db` (or a connection id resolving to one), never a `mysql://user:pass@…`
/// URL string. Credentials go to the driver through `OptsBuilder`, not a URL, so
/// a password containing `@ / # ? % :` needs no percent-encoding and can't break
/// parsing (review B7), and no plaintext URL is embedded anywhere as identity or
/// leaked on a command line (review C6).
#[derive(Clone, Debug)]
pub struct Db {
    pub(crate) engine: Engine,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) user: String,
    pub(crate) pass: String,
    /// The database file, for [`Engine::Sqlite`] — the whole target, since that
    /// engine has no server. Empty for every other engine, where the coordinates
    /// above are the target instead. See [`sqlite`].
    pub(crate) file: String,
    /// The database this endpoint opens in when no other is named, empty for
    /// none — already resolved through
    /// [`schemaic_core::connection::Connection::default_database`], so no driver
    /// here re-asks the engine whether one applies.
    pub(crate) database: String,
    /// How this endpoint's transport is secured, or `None` for plaintext —
    /// already resolved from the saved connection by
    /// [`schemaic_core::connection::Connection::tls_plan`], so no driver here
    /// re-reads a mode. See [`tls`].
    pub(crate) tls: Option<schemaic_core::connection::TlsPlan>,
}

/// What database a MySQL/MariaDB connection opens in.
///
/// **The two readings of `open(None)`, given separate spellings.** Since a
/// connection gained a configured database, `None` has meant *"the caller named
/// none, so use the connection's"* — and eleven call sites were written when it
/// meant *"this operation needs no database scope"*. Most are harmless because
/// their SQL is fully qualified anyway; `run_server_ddl` is not, and it is the
/// one that emits `DROP DATABASE`.
#[derive(Clone, Copy, Debug)]
enum Scope<'a> {
    /// The named database, or the connection's own when none is named.
    Database(Option<&'a str>),
    /// **No database at all**, and the connection's own must not fill it in.
    Server,
}

impl Db {
    /// Resolve a saved connection into a `Db`. For an SSH connection, pass the
    /// established tunnel's local port and the target is rewritten to
    /// `127.0.0.1:<port>`. Infallible — no URL is parsed. The engine is derived
    /// from the connection's `db_type` (MySQL/MariaDB vs PostgreSQL).
    pub fn connect(conn: &schemaic_core::connection::Connection, tunnel_port: Option<u16>) -> Db {
        let engine = Engine::from_db_type(&conn.db_type);
        // A SQLite target is a local file, so a tunnel port is meaningless — and
        // rewriting the endpoint to `127.0.0.1:<port>` for one would be actively
        // wrong. The caller shouldn't open a tunnel for such a connection at all
        // (`Engine::is_networked`), but the rewrite is ignored here as well, so a
        // caller that does can't repoint the file.
        let file = conn.file.clone();
        // Asked once, through the connection rather than the mode: a SQLite file
        // plans no handshake however the picker left the TLS block.
        let tls = conn.tls_plan();
        // Asked through the connection, so a name left behind by an engine
        // switch never reaches a driver that has no databases to open.
        let database = conn.default_database().unwrap_or_default().to_string();
        match tunnel_port.filter(|_| engine.is_networked()) {
            // **The certificate is still the far end's.** Rewriting the endpoint
            // to `127.0.0.1` would have `verify-full` compare a perfectly good
            // certificate against the loopback address and reject it, so the
            // name to check is carried over in the same step that moves the
            // address — the two must not be able to drift apart.
            Some(port) => Db {
                engine,
                host: "127.0.0.1".to_string(),
                port,
                user: conn.user.clone(),
                pass: conn.password.clone(),
                file,
                database,
                tls: tls.map(|p| schemaic_core::connection::TlsPlan {
                    hostname_override: Some(conn.host.clone()),
                    ..p
                }),
            },
            None => Db {
                engine,
                host: conn.host.clone(),
                port: conn.port,
                user: conn.user.clone(),
                pass: conn.password.clone(),
                file,
                database,
                tls,
            },
        }
    }

    /// Reconstruct from raw parts + engine — used by the MCP subprocess, which
    /// receives the (already-tunnelled) endpoint (incl. engine) over its
    /// environment, so AI queries run against the right driver.
    ///
    /// `file` carries a SQLite connection's target and is empty for the other
    /// engines. It is part of the endpoint for the same reason `host` is: without
    /// it the subprocess has an engine it can't reach anything with.
    pub fn from_parts(
        engine: Engine,
        host: String,
        port: u16,
        user: String,
        pass: String,
        file: String,
    ) -> Db {
        Db {
            engine,
            host,
            port,
            user,
            pass,
            file,
            database: String::new(),
            tls: None,
        }
    }

    /// The database this handle opens in when none is named, or `None`.
    pub fn database(&self) -> Option<&str> {
        (!self.database.is_empty()).then_some(self.database.as_str())
    }

    /// Attach a default database to a handle built by [`Self::from_parts`] —
    /// the endpoint handoff's half of [`Self::database`], for the same reason
    /// [`Self::with_tls`] exists.
    ///
    /// Without it the MCP subprocess falls back to guessing, which on a provider
    /// that permits only its own database means the assistant cannot reach a
    /// server the app itself is connected to.
    pub fn with_database(mut self, database: Option<&str>) -> Db {
        self.database = database.unwrap_or_default().to_string();
        self
    }

    /// Attach a resolved TLS plan to a handle built by [`Self::from_parts`].
    ///
    /// Separate from `from_parts` because the endpoint handoff is a *string*
    /// channel (environment variables, for the MCP subprocess) while a plan is a
    /// structure: the caller rebuilds it on the far side and hangs it on here.
    /// Without it the subprocess would connect in plaintext to a server the user
    /// configured for TLS — the same query, quietly less protected, which is the
    /// one outcome this setting must never produce silently.
    pub fn with_tls(mut self, plan: Option<schemaic_core::connection::TlsPlan>) -> Db {
        self.tls = plan;
        self
    }

    /// The TLS plan this handle connects with, if any — for the endpoint handoff.
    pub fn tls_plan(&self) -> Option<&schemaic_core::connection::TlsPlan> {
        self.tls.as_ref()
    }

    /// Borrow the endpoint parts `(host, port, user, pass, file)` — used to
    /// serialize the endpoint for the MCP subprocess handoff.
    pub fn parts(&self) -> (&str, u16, &str, &str, &str) {
        (&self.host, self.port, &self.user, &self.pass, &self.file)
    }

    /// The database file this handle points at — empty unless it is SQLite.
    pub fn file(&self) -> &str {
        &self.file
    }

    /// The engine this handle speaks.
    pub fn engine(&self) -> Engine {
        self.engine
    }

    /// Build connection options for a fresh connection, optionally with a default
    /// database (`USE`d on connect so unqualified names resolve) and
    /// `CLIENT_FOUND_ROWS` (so `affected_rows()` counts *matched* rows, not
    /// *changed* ones — the commit path's exactly-one-row guard relies on it).
    fn opts(&self, scope: Scope<'_>, found_rows: bool) -> OptsBuilder {
        self.opts_with_tls(scope, found_rows, self.tls.as_ref())
    }

    /// [`Self::opts`] with the TLS plan named explicitly, so the `prefer`
    /// fallback can build the *same* options minus the handshake rather than a
    /// second, subtly different set.
    fn opts_with_tls(
        &self,
        scope: Scope<'_>,
        found_rows: bool,
        tls: Option<&schemaic_core::connection::TlsPlan>,
    ) -> OptsBuilder {
        let mut b = OptsBuilder::default()
            .ip_or_hostname(self.host.clone())
            .tcp_port(self.port)
            .user(Some(self.user.clone()))
            .pass(Some(self.pass.clone()))
            .client_found_rows(found_rows)
            .ssl_opts(tls.map(tls::mysql_ssl_opts));
        // The connection's own database is a *fallback*, never an override: an
        // operation that named one is working in it, and quietly redirecting
        // that to the connection default would run a statement somewhere the
        // caller did not ask for.
        if let Scope::Database(named) = scope
            && let Some(db) = named.or_else(|| self.database())
        {
            b = b.db_name(Some(db));
        }
        b
    }

    /// Open one connection to this endpoint (optionally scoped to a database).
    ///
    /// **`prefer` retries in plaintext, and nothing else does.** A server with no
    /// TLS fails the handshake rather than declining it, so "encrypt if you can"
    /// can only be implemented as a second attempt — and offering that second
    /// attempt to `require` would turn the strongest half of this setting into
    /// the weakest while still reporting success.
    pub(crate) async fn open(
        &self,
        database: Option<&str>,
        found_rows: bool,
    ) -> Result<Conn, DbError> {
        self.open_scoped(Scope::Database(database), found_rows)
            .await
    }

    /// Open a connection that names **no database at all**.
    ///
    /// **`open(None)` is not this**, and the two readings were spelled
    /// identically until now. Since the connection gained a configured
    /// database, `open(None)` means *"no database was named, so use the
    /// connection's"* — which is right for a `SHOW DATABASES` and wrong for the
    /// one operation that must not be attached to a database: `DROP DATABASE`
    /// ran on a session pointed at its own target, so the statement failed or
    /// left the connection answering `ERROR 1049` to everything afterwards.
    ///
    /// `KILL QUERY` takes this door too. It names no object, and a connection
    /// whose configured database will not open should still be able to cancel
    /// its own query.
    pub(crate) async fn open_serverless(&self, found_rows: bool) -> Result<Conn, DbError> {
        self.open_scoped(Scope::Server, found_rows).await
    }

    async fn open_scoped(&self, scope: Scope<'_>, found_rows: bool) -> Result<Conn, DbError> {
        let out = self.dial(scope, found_rows).await;
        // **The configured database must not be able to break the listing that
        // would let the user fix it.**
        //
        // On MySQL/MariaDB the database is part of the *handshake*, so a
        // **Database** field naming something the server will not open fails
        // every `Db` method with `ERROR 1049` — `ping`, `fetch_databases`,
        // `fetch_schema`, `commit_writes`, `run_server_ddl`, all of them. One
        // typo and the tree cannot list the databases, so there is nothing on
        // screen to correct it from. PostgreSQL degrades deliberately here
        // (`maintenance_candidates`); MySQL had no retry anywhere.
        //
        // Narrow on purpose: only where the caller named **no** database and
        // the fallback supplied one. An operation that asked for `shop` by name
        // still fails, because silently running it somewhere else is worse than
        // failing.
        if let Scope::Database(None) = scope
            && self.database().is_some()
            && out.as_ref().err().is_some_and(unknown_database)
        {
            return self.dial(Scope::Server, found_rows).await;
        }
        out
    }

    async fn dial(&self, scope: Scope<'_>, found_rows: bool) -> Result<Conn, DbError> {
        // Before the driver gets a chance to report a mistyped path as an
        // anonymous I/O error.
        //
        // **It runs for every mode, `prefer` included, and that is the
        // decision.** The comment here used to say it was skipped when the plan
        // may fall back; it never was. Leaving it running means a `prefer`
        // connection whose client-certificate path is stale fails outright
        // rather than quietly connecting in plaintext — a refusal naming the
        // file, which the user can fix, instead of a silent downgrade of
        // something they deliberately configured. `prefer`'s promise is about
        // what the *server* offers, not about tolerating a broken local setup.
        if let Some(plan) = self.tls.as_ref() {
            tls::preflight(plan)?;
        }
        let first = Conn::new(self.opts(scope, found_rows)).await;
        match first {
            Ok(conn) => Ok(conn),
            Err(e) => {
                if !self
                    .tls
                    .as_ref()
                    .is_some_and(|p| should_retry_plaintext(p, &e))
                {
                    return Err(DbError::Connect(e.to_string()));
                }
                // The plaintext error is the one worth reporting: having chosen
                // to fall back, the user's problem is whatever plaintext hit.
                Conn::new(self.opts_with_tls(scope, found_rows, None))
                    .await
                    .map_err(|e| DbError::Connect(e.to_string()))
            }
        }
    }

    /// Best-effort server-side cancel: connect afresh and `KILL QUERY <id>`.
    ///
    /// **Bounded by [`CANCEL_TIMEOUT`].** The whole reason this is reached is
    /// that something is not responding, and it answers that by opening a
    /// *fresh* connection — full TCP, a TLS handshake, and on `prefer` possibly
    /// a second connect — to a host that may be gone. Unbounded, a Stop against
    /// a dead server hangs inside a modal whose every exit maps to that same
    /// Stop, and the only way out is killing the process.
    pub(crate) async fn kill_query(&self, conn_id: u32) {
        let kill = async {
            if let Ok(mut killer) = self.open_serverless(false).await {
                let _ = killer.query_drop(format!("KILL QUERY {conn_id}")).await;
                let _ = killer.disconnect().await;
            }
        };
        if tokio::time::timeout(CANCEL_TIMEOUT, kill).await.is_err() {
            tracing::debug!("kill query timed out after {CANCEL_TIMEOUT:?}");
        }
    }
}

/// Did this connect fail because the *database* could not be opened, rather
/// than because the server, the network or the credentials were wrong?
///
/// Read off the message because that is all a `DbError::Connect` carries. Both
/// spellings are checked: MySQL and MariaDB print the code, and the driver's
/// own `Display` for a server error puts the text beside it.
///
/// Free and pure so the classification can be asserted without a live server —
/// the sole thing that decides whether an unopenable configured database is a
/// recoverable mistake or a dead connection.
pub(crate) fn unknown_database(e: &DbError) -> bool {
    let DbError::Connect(msg) = e else {
        return false;
    };
    msg.contains("1049") || msg.to_ascii_lowercase().contains("unknown database")
}

/// Should a failed MySQL connect be retried in plaintext?
///
/// **Only when the server said it has no TLS.** The retry's condition used to be
/// `plan.fallback_to_plaintext` alone — the error was never looked at — so a
/// `prefer` connection retried after *any* failure: a wrong password produced
/// twelve connect attempts for ten pings, which doubles failed logins against
/// `max_connect_errors` and fail2ban on a path that runs once per operation.
/// The downgrade half is worse: one injected RST or malformed TLS record during
/// the handshake and the whole operation continues in cleartext, which is an
/// attacker-forceable downgrade rather than a server capability.
///
/// `DriverError::NoClientSslFlagFromServer` is the exact variant `prefer` exists
/// for: the server did not advertise `CLIENT_SSL`. PostgreSQL's side already
/// falls back only on the server's `N`, so this is also what stops the two
/// engines meaning different things by one word in the picker.
///
/// Free rather than a method so it can be asserted without a live server — the
/// whole decision is `(plan, error) -> bool`.
pub(crate) fn should_retry_plaintext(
    plan: &schemaic_core::connection::TlsPlan,
    e: &mysql_async::Error,
) -> bool {
    plan.fallback_to_plaintext
        && matches!(
            e,
            mysql_async::Error::Driver(mysql_async::DriverError::NoClientSslFlagFromServer)
        )
}

impl Db {
    /// Connect (scoped to `database`), run `sql` (up to `row_cap` rows), and
    /// return the result. If `cancel` fires first, the running query is killed
    /// server-side and `DbError::Cancelled` is returned.
    pub async fn fetch_query(
        &self,
        database: Option<&str>,
        sql: &str,
        row_cap: usize,
        cancel: CancellationToken,
    ) -> Result<ResultSet, DbError> {
        self.run_to(database, sql, &mut RowDest::Capped(row_cap), cancel)
            .await
    }

    /// Run `sql` with **no row cap**, handing the rows to `tx` in blocks of
    /// `chunk_rows` as they arrive. Returns how many rows went out.
    ///
    /// This is the whole-table export, and the row cap is the thing it exists to
    /// escape. A capped fetch answers "what is in this table" and a cap is the
    /// right answer for a grid nobody can scroll two million rows of; an export
    /// answers "give me the table", where a cap is not a kindness but a silently
    /// short file.
    ///
    /// **It is still one connection for one operation** — this connects, runs,
    /// and disconnects like every other `Db` method. What is new is only how long
    /// that takes, and the receiving end is what bounds it: the channel is
    /// bounded, so a server faster than the disk waits rather than queueing the
    /// table in memory. Nothing is cached and no second connection path is
    /// added — the rule that `Session` is the one exception still holds.
    ///
    /// **A failure goes down the channel as well as back to the caller.** The
    /// writer is on the other end and would otherwise read a closed channel as
    /// "the table ended", and call a half-written file finished.
    pub async fn stream_query(
        &self,
        database: Option<&str>,
        sql: &str,
        chunk_rows: usize,
        cancel: CancellationToken,
        tx: tokio::sync::mpsc::Sender<ExportChunk>,
    ) -> Result<u64, DbError> {
        let mut dest = RowDest::Chunked {
            chunk: chunk_rows.max(1),
            tx: tx.clone(),
            sent: 0,
        };
        let outcome = self.run_to(database, sql, &mut dest, cancel).await;
        match outcome {
            // **A statement with no result set is not an empty export.** All
            // three engines return before their tail flush when the statement
            // returns no columns (a DML/DDL/utility outcome reports `affected`
            // instead), so nothing at all reaches the channel — and a writer
            // that saw no chunk would produce an empty file and call it done.
            // The export path never offers such a statement, but this is public
            // API and the next caller may not be gated the same way, so the
            // refusal lives here rather than in the caller that happens to be
            // careful.
            Ok(rs) if rs.columns.is_empty() && dest.sent() == 0 => {
                let e = DbError::Query("that statement returns no rows to export".to_string());
                let _ = tx.send(Err(e.to_string())).await;
                Err(e)
            }
            Ok(_) => Ok(dest.sent()),
            Err(e) => {
                // Best effort: if the writer has already gone the send fails, and
                // then it is the writer's own error that reaches the user.
                let _ = tx.send(Err(e.to_string())).await;
                Err(e)
            }
        }
    }

    /// The engine dispatch both [`Self::fetch_query`] and [`Self::stream_query`]
    /// go through — one connection, one statement, one destination for its rows.
    async fn run_to(
        &self,
        database: Option<&str>,
        sql: &str,
        dest: &mut RowDest,
        cancel: CancellationToken,
    ) -> Result<ResultSet, DbError> {
        // Stamped here, in the one place that knows what the connection was
        // actually scoped to, rather than by the caller from the tab it will land
        // in — see `ResultSet::database`.
        let mut rs = match self.engine {
            Engine::Postgres => pg::fetch_query(self, database, sql, dest, cancel).await?,
            Engine::Sqlite => sqlite::fetch_query(self, sql, dest, cancel).await?,
            Engine::MySql => {
                let mut conn = self.open(database, false).await?;
                // The connection id, so a second connection can KILL its in-flight query.
                let conn_id = conn.id();

                let outcome = tokio::select! {
                    // `early_stop`: this connection is torn down right after, so we can bail
                    // out of the row stream at the cap without draining the rest.
                    r = collect_rows(&mut conn, sql, dest, true) => r,
                    _ = cancel.cancelled() => {
                        self.kill_query(conn_id).await;
                        Err(DbError::Cancelled)
                    }
                };

                let _ = conn.disconnect().await;
                outcome?
            }
        };
        // A SQLite connection has exactly one database and the caller passes none,
        // so the label comes from the engine rather than from a scope nobody set.
        rs.database = match self.engine {
            Engine::Sqlite => Some(sqlite::MAIN.to_string()),
            _ => database.map(str::to_string),
        };
        Ok(rs)
    }

    /// Fetch up to `limit` rows of a single table for the Live Monitor:
    /// `SELECT * FROM `db`.`table` [ORDER BY …] LIMIT n`. Bounded by construction
    /// — the monitor never polls an unbounded table. Column provenance is
    /// populated as for any query, so the caller derives the row-identity key via
    /// `analyze_edit`.
    ///
    /// **`order_by` is what makes the window comparable between polls.** Without
    /// it the engine may return any `limit` rows in any order, so a table over the
    /// limit produced insert/delete pairs that never happened — and on PostgreSQL
    /// an `UPDATE` moves its tuple to the end of the heap, so the next scan
    /// reorders and the updated row is logged as *deleted* while an untouched one
    /// is logged as *inserted*. Pass the row-identity key; `None` only when the
    /// table has none, where the monitor can't track changes anyway.
    pub async fn fetch_table(
        &self,
        database: &str,
        schema: Option<&str>,
        table: &str,
        order_by: Option<&[String]>,
        limit: usize,
        cancel: CancellationToken,
    ) -> Result<ResultSet, DbError> {
        match self.engine {
            Engine::Postgres => {
                return pg::fetch_table(self, database, schema, table, order_by, limit, cancel)
                    .await;
            }
            Engine::Sqlite => {
                // One file, one namespace: the table stands alone, and the
                // `main.` qualifier would only be noise.
                let sql = format!(
                    "SELECT * FROM {}{} LIMIT {}",
                    ident_sqlite(table),
                    order_by_clause(order_by, ident_sqlite),
                    limit
                );
                return self.fetch_query(None, &sql, limit, cancel).await;
            }
            Engine::MySql => {}
        }
        // MySQL has no namespace level — the database already is one.
        debug_assert!(schema.is_none(), "MySQL tables carry no namespace");
        let sql = format!(
            "SELECT * FROM {}.{}{} LIMIT {}",
            ident(database),
            ident(table),
            order_by_clause(order_by, ident),
            limit
        );
        self.fetch_query(Some(database), &sql, limit, cancel).await
    }
}

/// A plan's row count is tiny (classic EXPLAIN) or one big row (tree-format
/// `EXPLAIN ANALYZE`); this cap is only a backstop.
const EXPLAIN_ROW_CAP: usize = 10_000;

impl Db {
    /// Run `EXPLAIN sql` (or `EXPLAIN ANALYZE sql`) and return the plan as a
    /// result set (the caller parses it with `schemaic_core::plan`).
    ///
    /// Plain `EXPLAIN` only *plans* the statement — it never executes it, so it's
    /// safe even for `UPDATE`/`DELETE`. `analyze` is different: it **executes** the
    /// statement to measure it, so callers must gate it to read-only statements.
    ///
    /// MariaDB spells the analyzing form `ANALYZE <stmt>`, not `EXPLAIN ANALYZE`
    /// (which it rejects as a syntax error *before* running anything), so when the
    /// `EXPLAIN ANALYZE` attempt fails we retry with `ANALYZE`. On MySQL the reverse
    /// (`ANALYZE <select>`) is itself a syntax error, so the two servers never both
    /// match — the fallback can't double-execute.
    ///
    /// **The analyzing form runs inside a transaction that is always rolled
    /// back.** The UI gates the Analyze toggle on `sql::contains_write`, but that
    /// gate reads the statement and any reading of a statement can be wrong — a
    /// data-modifying CTE fooled it once already. Measuring must not be the thing
    /// that changes the data, so the rollback holds whether or not the gate above
    /// it was right. Note the limit this shares with every MySQL write path: on a
    /// non-transactional table (MyISAM) the rollback does nothing, and on a DDL
    /// statement the server commits implicitly.
    pub async fn explain(
        &self,
        database: Option<&str>,
        sql: &str,
        analyze: bool,
        cancel: CancellationToken,
    ) -> Result<ResultSet, DbError> {
        match self.engine {
            Engine::Postgres => return pg::explain(self, database, sql, analyze, cancel).await,
            Engine::Sqlite => {
                // SQLite's `EXPLAIN` disassembles the statement into VDBE opcodes,
                // which is a different artefact from the other two engines' plans
                // and useless to `core::plan`'s heuristics. `EXPLAIN QUERY PLAN` is
                // the one that answers the question the panel asks — which index,
                // which scan — so that is what runs.
                //
                // There is no analyzing form at all: SQLite will not execute a
                // statement to time it. `analyze` is therefore ignored rather than
                // refused, since the plan it falls back to is still the right
                // answer to "how will this run", and the caller has already gated
                // the toggle on the statement being read-only.
                let stmt = sql.trim().trim_end_matches(';').trim_end();
                let plan = format!("EXPLAIN QUERY PLAN {stmt}");
                return self
                    .fetch_query(database, &plan, EXPLAIN_ROW_CAP, cancel)
                    .await;
            }
            Engine::MySql => {}
        }
        let (primary, fallback) = explain_commands(sql, analyze);
        if !analyze {
            return self
                .fetch_query(database, &primary, EXPLAIN_ROW_CAP, cancel)
                .await;
        }
        match self
            .explain_in_rolled_back_tx(database, &primary, cancel.clone())
            .await
        {
            // MariaDB: `EXPLAIN ANALYZE` is invalid — retry with `ANALYZE <stmt>`.
            Err(DbError::Query(_)) if fallback.is_some() => {
                self.explain_in_rolled_back_tx(database, &fallback.unwrap(), cancel)
                    .await
            }
            other => other,
        }
    }

    /// Run one analyzing-EXPLAIN command on a single connection, wrapped in a
    /// transaction that is always rolled back. Separate from [`Db::fetch_query`]
    /// because that opens a fresh connection per call, which would put the
    /// `BEGIN`, the measurement and the `ROLLBACK` on three different sessions.
    async fn explain_in_rolled_back_tx(
        &self,
        database: Option<&str>,
        cmd: &str,
        cancel: CancellationToken,
    ) -> Result<ResultSet, DbError> {
        let mut conn = self.open(database, false).await?;
        let conn_id = conn.id();
        if let Err(e) = conn.query_drop("BEGIN").await {
            let _ = conn.disconnect().await;
            return Err(DbError::Query(e.to_string()));
        }

        let mut dest = RowDest::Capped(EXPLAIN_ROW_CAP);
        let outcome = tokio::select! {
            r = collect_rows(&mut conn, cmd, &mut dest, true) => r,
            _ = cancel.cancelled() => {
                self.kill_query(conn_id).await;
                Err(DbError::Cancelled)
            }
        };

        // Unconditional. Dropping the connection would roll back too, but saying
        // so explicitly is what makes the guarantee readable at the call site.
        let _ = conn.query_drop("ROLLBACK").await;
        let _ = conn.disconnect().await;
        outcome
    }

    /// Validate `sql` against the server **without executing it**: prepare it via
    /// the binary protocol (`PREPARE`), then deallocate. The server checks syntax,
    /// object names, and types but runs nothing — safe even for `UPDATE`/`DELETE`.
    /// Returns the server's error text on failure, `Ok(())` on a clean prepare.
    ///
    /// Statements the prepared-statement protocol doesn't support (server error
    /// 1295 — e.g. some `SHOW`/admin forms) can't be validated this way, so they're
    /// treated as `Ok` rather than surfacing a spurious error. A trailing `;` is
    /// trimmed (the protocol prepares a single statement).
    /// A MySQL view's `ALGORITHM`, which lives nowhere a bulk query can reach it.
    ///
    /// MariaDB reports it in `information_schema.VIEWS` and the schema fetch
    /// already carries it. **MySQL 8 has the column nowhere but `SHOW CREATE
    /// VIEW`**, one statement per view — too many round-trips to fold into a
    /// schema fetch, so this is called lazily, for the single view about to be
    /// edited.
    ///
    /// It matters because `CREATE OR REPLACE VIEW` replaces the whole view: a
    /// `MERGE` view redefined without the clause comes back `UNDEFINED`, letting
    /// the server pick a materialization the author had ruled out. The same class
    /// of silent loss as the `SQL SECURITY` bug, which is why it isn't left to
    /// the default.
    ///
    /// `Ok(None)` means the server didn't state one (`UNDEFINED`), which is also
    /// what PostgreSQL — with no such concept — returns without asking.
    /// Every trigger function in `database` — PostgreSQL only, and re-read on
    /// its own when the trigger or routine editor asks.
    ///
    /// The schema fetch carries most of the same list, so this is largely a
    /// **refresh**: the trigger editor calls it after the routine editor closes,
    /// because a function just created has to appear in the dropdown and nothing
    /// else would put it there before the next schema reload.
    ///
    /// **It is not the same query as the browse list, and the difference is
    /// deliberate.** `pg::routines` hides extension-owned routines, which is
    /// right for a Functions folder the user edits and wrong here: `moddatetime`
    /// and its kin are exactly what a trigger binds to, and the picker is a
    /// dropdown with no free-text entry, so a function missing from this list is
    /// a function no trigger can be pointed at. The narrowing this one does
    /// instead — to what actually returns `trigger` — happens on the server, so
    /// a database with hundreds of routines doesn't ship every body over the
    /// wire to have them filtered here.
    ///
    /// Empty on MySQL, whose triggers hold their own body and need no function
    /// at all, and on SQLite, which has no stored routines.
    pub async fn trigger_functions(
        &self,
        database: &str,
    ) -> Result<Vec<schemaic_core::schema::RoutineInfo>, DbError> {
        // PostgreSQL alone has trigger functions as objects of their own; a MySQL
        // trigger carries its body, and SQLite's carries a statement list.
        match self.engine {
            Engine::Postgres => pg::trigger_functions(self, database).await,
            Engine::MySql | Engine::Sqlite => Ok(Vec::new()),
        }
    }

    /// The roles a database or namespace could be owned by — read lazily, when
    /// the database editor opens on PostgreSQL.
    ///
    /// The same shape [`Db::trigger_functions`] uses, and with the same
    /// standing: it feeds a *shortcut* beside a free-text field, so an empty
    /// list costs the user a menu and never a value. Empty on the two engines
    /// with no such concept — a MySQL database belongs to nobody (it is reached
    /// through grants) and SQLite has neither roles nor databases.
    pub async fn roles(&self) -> Result<Vec<String>, DbError> {
        match self.engine {
            Engine::Postgres => pg::roles(self).await,
            Engine::MySql | Engine::Sqlite => Ok(Vec::new()),
        }
    }

    /// A MySQL routine's body **as written**, plus the session state it was
    /// written under — read lazily, when the routine editor opens.
    ///
    /// The same shape [`Db::trigger_source`] uses, and **not an optimisation**:
    /// `information_schema.ROUTINES.ROUTINE_DEFINITION` resolves the body's
    /// escapes on MySQL 8, and every edit on this engine begins with a `DROP`
    /// that commits on its own — so a restate built from the resolved text can
    /// fail after the only copy is gone. See
    /// [`schemaic_core::schema::RoutineSource`].
    ///
    /// `Ok(None)` on PostgreSQL (whose `prosrc` is faithful) and on SQLite
    /// (which has no routines), and for a routine the connected role may not
    /// read the definition of — `SHOW CREATE` returns a NULL body without
    /// `SHOW_ROUTINE` or ownership, and a `None` leaves the editor on what the
    /// schema already carried rather than blanking the body.
    pub async fn routine_source(
        &self,
        database: Option<&str>,
        kind: schemaic_core::schema::RoutineKind,
        name: &str,
    ) -> Result<Option<schemaic_core::schema::RoutineSource>, DbError> {
        if self.engine != Engine::MySql {
            return Ok(None);
        }
        let mut conn = self.open(database, false).await?;
        // (Procedure|Function, sql_mode, Create …, character_set_client,
        //  collation_connection, Database Collation)
        let sql = format!("SHOW CREATE {} {}", kind.sql_keyword(), ident(name.trim()));
        let row: Option<MyShowCreateRoutineRow> = conn
            .query_first(sql.as_str())
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;
        let _ = conn.disconnect().await;
        let some = |s: String| Some(s).filter(|s| !s.is_empty());
        // **The session state survives a body this can't read.** All four values
        // come from the same row and only the body needs parsing, so folding
        // them into its success meant a routine with an unfamiliar header was
        // later recreated under whatever `sql_mode` the applying session had.
        Ok(row.map(
            |(_, mode, create, cs, coll, ..)| schemaic_core::schema::RoutineSource {
                body: create.as_deref().and_then(routine_body_of),
                sql_mode: some(mode),
                charset_client: some(cs),
                collation_connection: some(coll),
                // Read off the *header*, so it survives a body this can't parse
                // for the same reason the session state does — and the header
                // is the only place it exists at all.
                aggregate: create.as_deref().is_some_and(routine_is_aggregate),
            },
        ))
    }

    /// A MySQL event's body **as written**, plus the session state and the time
    /// zone it was written under — read lazily, when the event editor opens.
    ///
    /// The same shape [`Db::routine_source`] uses and for the same reason:
    /// `information_schema.EVENTS.EVENT_DEFINITION` resolves the body's escapes,
    /// so an edit restated from it can be refused over a quote the user never
    /// typed. Milder than the routine case — `ALTER EVENT` edits in place, so a
    /// refusal leaves the event standing rather than gone — and still the only
    /// faithful source.
    ///
    /// `Ok(None)` on the two engines that have no events, and for an event the
    /// connected account may not read the definition of.
    pub async fn event_source(
        &self,
        database: Option<&str>,
        name: &str,
    ) -> Result<Option<EventSource>, DbError> {
        if self.engine != Engine::MySql {
            return Ok(None);
        }
        let mut conn = self.open(database, false).await?;
        // (Event, sql_mode, time_zone, Create Event, character_set_client,
        //  collation_connection, Database Collation)
        let sql = format!("SHOW CREATE EVENT {}", ident(name.trim()));
        let row: Option<MyShowCreateEventRow> = conn
            .query_first(sql.as_str())
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;
        let _ = conn.disconnect().await;
        let some = |s: String| Some(s).filter(|s| !s.is_empty());
        // **The session state survives a body this can't read**, exactly as it
        // does for a routine: all five values come from one row and only the
        // body needs parsing.
        Ok(row.map(|(_, mode, tz, create, cs, coll, ..)| EventSource {
            body: create.as_deref().and_then(event_body_of),
            time_zone: some(tz),
            sql_mode: some(mode),
            charset_client: some(cs),
            collation_connection: some(coll),
        }))
    }

    /// A MySQL trigger's body **as written**, plus the session state it was
    /// written under — read lazily, when the trigger editor opens.
    ///
    /// The same shape [`Db::view_algorithm`] uses, and not part of
    /// `fetch_schema` for the same reason: one `SHOW CREATE TRIGGER` per trigger
    /// is far too many round trips for a schema refresh, and the answer is only
    /// needed for the trigger actually being edited.
    ///
    /// **This is not an optimisation — it is the only correct source.** See
    /// [`TriggerSource`] for what `information_schema` does to the body instead.
    /// `Ok(None)` on PostgreSQL (whose triggers have no body) and on MariaDB,
    /// which returns a faithful `ACTION_STATEMENT` already and needs no second
    /// round trip.
    pub async fn trigger_source(
        &self,
        database: Option<&str>,
        trigger: &str,
    ) -> Result<Option<TriggerSource>, DbError> {
        // The lazy second round-trip exists for MySQL's escape-mangling alone —
        // PostgreSQL reports a faithful body already, and SQLite stores the
        // trigger's original `CREATE` text verbatim in `sqlite_master`, so neither
        // needs it.
        if self.engine != Engine::MySql {
            return Ok(None);
        }
        let mut conn = self.open(database, false).await?;
        // (Trigger, sql_mode, SQL Original Statement, character_set_client,
        //  collation_connection, Database Collation, Created)
        let sql = format!("SHOW CREATE TRIGGER {}", ident(trigger));
        let row: Option<MyShowCreateTriggerRow> = conn
            .query_first(sql.as_str())
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;
        let _ = conn.disconnect().await;
        let some = |s: String| Some(s).filter(|s| !s.is_empty());
        Ok(row.and_then(|(_, mode, create, cs, coll, ..)| {
            trigger_body_of(&create).map(|body| TriggerSource {
                body,
                sql_mode: some(mode),
                charset_client: some(cs),
                collation_connection: some(coll),
            })
        }))
    }

    pub async fn view_algorithm(
        &self,
        database: Option<&str>,
        view: &str,
    ) -> Result<Option<String>, DbError> {
        // `ALGORITHM` is MySQL's alone — neither other engine has the clause, so
        // there is nothing to fetch and nothing a replace would reset.
        if self.engine != Engine::MySql {
            return Ok(None);
        }
        let mut conn = self.open(database, false).await?;
        // `SHOW CREATE VIEW` returns (View, Create View, charset, collation).
        let sql = format!("SHOW CREATE VIEW {}", ident(view));
        let row: Option<(String, String, String, String)> = conn
            .query_first(sql.as_str())
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;
        let _ = conn.disconnect().await;
        Ok(row.and_then(|(_, create, ..)| view_algorithm_of(&create)))
    }

    pub async fn prepare_check(&self, database: Option<&str>, sql: &str) -> Result<(), DbError> {
        let stmt = sql.trim().trim_end_matches(';').trim_end();
        if stmt.is_empty() {
            return Ok(());
        }
        match self.engine {
            Engine::Postgres => return pg::prepare_check(self, database, sql).await,
            Engine::Sqlite => return sqlite::prepare_check(self, stmt).await,
            Engine::MySql => {}
        }
        let mut conn = self.open(database, false).await?;
        let result = match conn.prep(stmt).await {
            Ok(prepared) => {
                let _ = conn.close(prepared).await;
                Ok(())
            }
            Err(e) => {
                let msg = e.to_string();
                // 1295 = "not supported in the prepared statement protocol": we
                // can't validate it, so don't flag a false error.
                if msg.contains("1295")
                    || msg
                        .to_ascii_lowercase()
                        .contains("prepared statement protocol")
                {
                    Ok(())
                } else {
                    Err(DbError::Query(msg))
                }
            }
        };
        let _ = conn.disconnect().await;
        result
    }
}

/// The `EXPLAIN`/`ANALYZE` command(s) for `sql`: the statement is trimmed of a
/// trailing `;`, then wrapped. Returns `(primary, fallback)` — for `analyze` the
/// fallback is MariaDB's `ANALYZE <stmt>` (MySQL uses `EXPLAIN ANALYZE`); plain
/// `EXPLAIN` has no fallback. Pure so the wrapping/fallback logic is unit-tested.
fn explain_commands(sql: &str, analyze: bool) -> (String, Option<String>) {
    let stmt = sql.trim().trim_end_matches(';').trim_end();
    if analyze {
        (
            format!("EXPLAIN ANALYZE {stmt}"),
            Some(format!("ANALYZE {stmt}")),
        )
    } else {
        (format!("EXPLAIN {stmt}"), None)
    }
}

/// Run several statements in order on ONE connection, so session state (`USE`,
/// `SET`, temp tables, transactions) carries across them exactly as a SQL script
/// would — unlike calling [`Db::fetch_query`] per statement, which reconnects each
/// time. Each statement's outcome is delivered through `on_result(index, …)` as
/// soon as it completes, so the UI can fill result tabs progressively.
///
/// Execution stops at the first failing statement (its index reports the error);
/// every statement after it reports [`DbError::Cancelled`], matching DataGrip's
/// default "stop on error". `cancel` is honored both between and during
/// statements (a mid-flight statement is killed server-side, as in `fetch_query`).
impl Db {
    pub async fn run_batch(
        &self,
        database: Option<&str>,
        stmts: &[String],
        row_cap: usize,
        cancel: CancellationToken,
        on_result: impl FnMut(usize, Result<ResultSet, DbError>),
    ) {
        // Wrap the sink once so the scope is stamped on every statement's result
        // whichever engine produced it — a per-engine stamp is one a new path
        // forgets. See `ResultSet::database`.
        //
        // **The scope follows a `USE`.** It was computed once before the loop, on
        // a method whose own doc advertises that a `USE` carries across
        // statements — so `USE sakila; SELECT * FROM actor;` from a tab scoped to
        // `world` really ran statement 2 in `sakila` and labelled its result
        // `world`, the stats line lying in exactly the case the label exists to
        // catch. `sql::use_target` is deliberately conservative: a `USE` it can't
        // read plainly drops the label to `None`, which prints nothing, rather
        // than carrying a name that is now certainly wrong.
        // `Arc<Mutex>` rather than `Rc<RefCell>`: this future is spawned onto the
        // multi-threaded runtime and must be `Send`.
        let dialect = self.engine.dialect();
        let scope = std::sync::Arc::new(std::sync::Mutex::new(database.map(str::to_string)));
        let stamp = scope.clone();
        let mut on_result = {
            let mut inner = on_result;
            move |i: usize, r: Result<ResultSet, DbError>| {
                inner(
                    i,
                    r.map(|mut rs| {
                        rs.database = stamp.lock().ok().and_then(|s| s.clone());
                        rs
                    }),
                )
            }
        };
        match self.engine {
            Engine::Postgres => {
                pg::run_batch(self, database, stmts, row_cap, cancel, on_result).await;
                return;
            }
            Engine::Sqlite => {
                // Statement by statement on a fresh connection each, which is what
                // `fetch_query` already does. There is no `USE` to carry and no
                // session state to keep, so a batch here needs nothing a loop
                // doesn't give it — the `scope` stamping above is a no-op for an
                // engine with one database.
                for (i, stmt) in stmts.iter().enumerate() {
                    if cancel.is_cancelled() {
                        on_result(i, Err(DbError::Cancelled));
                        continue;
                    }
                    let r = sqlite::fetch_query(
                        self,
                        stmt,
                        &mut RowDest::Capped(row_cap),
                        cancel.clone(),
                    )
                    .await;
                    let failed = r.is_err();
                    on_result(i, r);
                    // A batch stops at its first failure, as the other two do:
                    // the statements after it were written against a state that
                    // never happened.
                    if failed {
                        for (j, _) in stmts.iter().enumerate().skip(i + 1) {
                            on_result(j, Err(DbError::Cancelled));
                        }
                        return;
                    }
                }
                return;
            }
            Engine::MySql => {}
        }
        let mut conn = match self.open(database, false).await {
            Ok(c) => c,
            Err(e) => {
                // Couldn't even connect: fail the first statement, cancel the rest.
                for (i, _) in stmts.iter().enumerate() {
                    on_result(
                        i,
                        if i == 0 {
                            Err(err_clone(&e))
                        } else {
                            Err(DbError::Cancelled)
                        },
                    );
                }
                return;
            }
        };
        let conn_id = conn.id();

        let mut stopped = false;
        for (i, sql) in stmts.iter().enumerate() {
            if stopped || cancel.is_cancelled() {
                on_result(i, Err(DbError::Cancelled));
                continue;
            }
            let mut dest = RowDest::Capped(row_cap);
            let outcome = tokio::select! {
                // `early_stop = false`: the connection is reused for the next
                // statement, so a truncated result must be drained fully to leave
                // the connection clean.
                r = collect_rows(&mut conn, sql, &mut dest, false) => r,
                _ = cancel.cancelled() => {
                    self.kill_query(conn_id).await;
                    Err(DbError::Cancelled)
                }
            };
            if outcome.is_err() {
                stopped = true;
            }
            // Before the sink, so a `USE` labels its own (empty) result with the
            // database it moved to — which is what the statement did.
            if outcome.is_ok()
                && schemaic_core::sql::leading_keyword(sql, dialect).as_deref() == Some("USE")
                && let Ok(mut scope) = scope.lock()
            {
                *scope = schemaic_core::sql::use_target(sql, dialect);
            }
            on_result(i, outcome);
        }

        let _ = conn.disconnect().await;
    }
}

/// `DbError` isn't `Clone`; this reproduces one for the "connect failed" fan-out.
fn err_clone(e: &DbError) -> DbError {
    match e {
        DbError::Connect(s) => DbError::Connect(s.clone()),
        DbError::Query(s) => DbError::Query(s.clone()),
        DbError::Cancelled => DbError::Cancelled,
    }
}

/// How long any "is this connection up" check may take before it is a failure.
///
/// Named because **four** paths ask it — the health check through [`Db::ping`],
/// and all three database listings, each of which is a ping with a `SELECT`
/// after it. They used to disagree, and this used to say "two paths": the
/// listings were unbounded, so a host that stops answering at the packet level
/// (a dropped VPN, a laptop off the office network, a firewall `DROP`) left the
/// schema tree empty for the OS TCP connect timeout while the health check
/// beside it gave up at five seconds and painted "Disconnected".
///
/// Measured on this machine against an unroutable address: **21.0 s** on MySQL,
/// and **63 s** on PostgreSQL, where `pg::connect_maintenance` tries `postgres`,
/// the user's own name and `template1` in sequence. SQLite's is the same story
/// with a share that has gone away rather than a host.
///
/// **The deadline is on the listing, not on every `open`.** The obvious
/// alternative — a driver-level connect timeout, which would bound
/// `fetch_schema` and `fetch_query` too — is not available on both engines:
/// `tokio_postgres::Config` has `connect_timeout`, and the pinned
/// `mysql_async` 0.34 `OptsBuilder` has no TCP connect option at all. Doing it
/// on one engine only would put a five-second cap on PostgreSQL query connects
/// and nothing on MySQL's, which is a worse asymmetry than the one it fixes.
pub const PING_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// How long a **cancel** may take before it is given up on.
///
/// A cancel is best-effort by construction — its result is discarded on every
/// path — but "best-effort" and "unbounded" are not the same word. Both engines
/// cancel by opening a *second* connection (MySQL to send `KILL QUERY`,
/// PostgreSQL because that is what the cancellation protocol is), which is a
/// full TCP connect plus a TLS handshake to a host that, by the time anyone is
/// pressing Stop, may be gone.
///
/// Unbounded that hangs *inside a modal whose every exit maps to the same
/// Stop*, with the global shortcuts gated off behind `modal_up()`: there is no
/// way out but killing the process. Five seconds matches [`PING_TIMEOUT`],
/// which is the app's existing answer to "the server is not responding".
pub const CANCEL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

impl Db {
    /// Lightweight reachability check: connect and run `SELECT 1`, all bounded by
    /// `timeout` so a dead host/tunnel can't hang the caller. `Ok(())` means the
    /// server answered. [`PING_TIMEOUT`] is the app's answer for `timeout`.
    pub async fn ping(&self, timeout: std::time::Duration) -> Result<(), DbError> {
        match self.engine {
            Engine::Postgres => return pg::ping(self, timeout).await,
            Engine::Sqlite => {
                // The timeout still applies: a file on a disconnected network
                // share can block in `open` for as long as the OS lets it.
                return match tokio::time::timeout(timeout, sqlite::ping(self)).await {
                    Ok(r) => r,
                    Err(_) => Err(DbError::Connect("timed out".to_string())),
                };
            }
            Engine::MySql => {}
        }
        let check = async {
            let mut conn = self.open(None, false).await?;
            let r = conn
                .query_drop("SELECT 1")
                .await
                .map_err(|e| DbError::Query(e.to_string()));
            let _ = conn.disconnect().await;
            r
        };
        tokio::time::timeout(timeout, check)
            .await
            .map_err(|_| DbError::Connect("timed out".to_string()))?
    }

    /// List the user databases on a server (excludes the built-in system schemas),
    /// sorted by name. Connects at the server level (no specific database needed).
    ///
    /// **Bounded by [`PING_TIMEOUT`], on every engine.** The schema sidebar lists
    /// a connection's databases the moment it is selected, so this is the first
    /// thing a dead host hangs — and the health check on the same connection
    /// gives up at the same five seconds and says "Disconnected". They have to
    /// agree, or the tree sits empty under a header that has already explained
    /// why and the user has nothing to do but wait out the OS.
    pub async fn fetch_databases(&self) -> Result<Vec<String>, DbError> {
        match self.engine {
            // Both already bounded inside, and PostgreSQL's needs to be bounded
            // *around* the sequence rather than per attempt: `connect_maintenance`
            // tries three candidate databases in turn.
            Engine::Postgres => return pg::fetch_databases(self).await,
            Engine::Sqlite => return sqlite::fetch_databases(self).await,
            Engine::MySql => {}
        }
        let listing = async {
            let mut conn = self.open(None, false).await?;
            let out = conn
                .query_map(
                    "SELECT CAST(SCHEMA_NAME AS CHAR) AS n FROM information_schema.SCHEMATA \
                 WHERE SCHEMA_NAME NOT IN \
                   ('information_schema','mysql','performance_schema','sys') \
                 ORDER BY SCHEMA_NAME",
                    |n: String| n,
                )
                .await
                .map_err(|e| DbError::Query(e.to_string()));
            let _ = conn.disconnect().await;
            out
        };
        tokio::time::timeout(PING_TIMEOUT, listing)
            .await
            .map_err(|_| DbError::Connect("timed out".to_string()))?
    }

    /// Introspect one database's schema (tables → columns + indexes) via
    /// `information_schema` (ARCHITECTURE §11). Everything is `CAST` to a known type
    /// so the protocol never surprises us with a width mismatch.
    ///
    /// **Takes a `CancellationToken` like every other unbounded operation.** This
    /// is every column, index, key, view, check and trigger of a whole database,
    /// so on a few hundred tables it runs for a long time — and the Export
    /// modal's `Reading the schema` phase is mounted over it behind a full
    /// backdrop whose only exit is a cancel. Without the token the press did
    /// nothing: the read ran to completion, and nothing else in the app was
    /// clickable meanwhile. [`Db::count_rows`]' own doc records the same failure
    /// once already.
    ///
    /// A caller with no cancel of its own passes `CancellationToken::new()`,
    /// which is never cancelled.
    pub async fn fetch_schema(
        &self,
        database: &str,
        cancel: CancellationToken,
    ) -> Result<DbSchema, DbError> {
        // At the door, before any engine opens anything: a token cancelled
        // before the call — Stop pressed while a queued dump was still waiting —
        // would otherwise pay for a full connection handshake and then a *second*
        // connection to KILL a query that was never issued.
        if cancel.is_cancelled() {
            return Err(DbError::Cancelled);
        }
        match self.engine {
            Engine::Postgres => return pg::fetch_schema(self, database, cancel).await,
            Engine::Sqlite => return sqlite::fetch_schema(self, cancel).await,
            Engine::MySql => {}
        }
        let mut conn = self.open(None, false).await?;
        // The connection id, so a second connection can KILL the read that is
        // already running on the server — the same shape `count_rows` uses, and
        // the only thing that actually stops work in flight.
        let conn_id = conn.id();
        let out = tokio::select! {
            r = collect_schema(&mut conn, database) => r,
            _ = cancel.cancelled() => {
                self.kill_query(conn_id).await;
                Err(DbError::Cancelled)
            }
        };
        let _ = conn.disconnect().await;
        out
    }

    /// `database`'s table **list** — name, namespace and view flag, and nothing
    /// else. Every returned [`TableInfo`]'s columns, indexes and foreign keys are
    /// **empty**, so this is not a substitute for [`Db::fetch_schema`]; it shares
    /// the return type only so a name-listing caller needs no second formatter.
    ///
    /// It exists because `fetch_schema` was being used as a name list: the MCP
    /// server's no-argument `list_schema` introspected **every** database on the
    /// server in full — five catalogue queries each, every column of every table —
    /// and then printed the names. That is the assistant's usual first tool call,
    /// and the cost was unrelated to the answer.
    pub async fn fetch_table_list(&self, database: &str) -> Result<DbSchema, DbError> {
        match self.engine {
            Engine::Postgres => return pg::fetch_table_list(self, database).await,
            // SQLite's names come from one `sqlite_master` scan, which is already
            // what the full introspection starts from; the saving this method
            // exists for is the *per-table* pragmas, so the list path skips those.
            Engine::Sqlite => return sqlite::fetch_table_list(self).await,
            Engine::MySql => {}
        }
        let mut conn = self.open(None, false).await?;
        let out = conn
            .exec_map(
                "SELECT CAST(TABLE_NAME AS CHAR) AS t, CAST(TABLE_TYPE AS CHAR) AS ty \
                 FROM information_schema.TABLES \
                 WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME",
                (database,),
                |(name, ty): (String, String)| TableInfo {
                    name,
                    is_view: ty.eq_ignore_ascii_case("VIEW"),
                    ..Default::default()
                },
            )
            .await
            .map(|tables| DbSchema {
                tables,
                ..Default::default()
            })
            .map_err(|e| DbError::Query(e.to_string()));
        let _ = conn.disconnect().await;
        out
    }

    /// Size, row estimate and index usage for **every** table in `database`.
    ///
    /// Whole-database rather than per-table because it costs the same round trip
    /// either way, and having the set is what lets the schema tree put a size
    /// beside every table at once.
    ///
    /// **Deliberately not part of [`Db::fetch_schema`].** On MySQL, selecting
    /// `DATA_LENGTH` and friends from `information_schema.TABLES` makes the
    /// server materialize per-table statistics, and on a schema with thousands
    /// of tables with a cold stats cache that is slow enough to notice. The
    /// schema fetch runs on every connect; this one runs when someone asks to
    /// see the numbers.
    ///
    /// SQLite returns an empty set — see
    /// [`schemaic_core::stats::supports_table_stats`] for why that is a fact
    /// about SQLite and not a gap here.
    pub async fn fetch_table_stats(&self, database: &str) -> Result<SchemaStats, DbError> {
        match self.engine {
            Engine::Postgres => return pg::fetch_table_stats(self, database).await,
            Engine::Sqlite => return Ok(SchemaStats::default()),
            Engine::MySql => {}
        }
        let mut conn = self.open(None, false).await?;
        let out = collect_table_stats(&mut conn, database).await;
        let _ = conn.disconnect().await;
        out
    }

    /// `SELECT COUNT(*)` — the exact row count, on demand.
    ///
    /// The one figure every engine can answer without qualification, and the
    /// answer to an estimate the user doesn't believe. Unbounded by nature: on a
    /// large table this is a full scan, which is why nothing calls it
    /// automatically.
    ///
    /// **And why it takes a token like every other unbounded operation here.** It
    /// was the one that didn't: closing the properties modal abandoned the *result*
    /// and left the scan running on the server for minutes, holding its connection,
    /// with nothing anywhere able to stop it — and reopening offered the button
    /// again, so N opens stacked N concurrent full scans on a production server.
    pub async fn count_rows(
        &self,
        database: &str,
        schema: Option<&str>,
        table: &str,
        cancel: CancellationToken,
    ) -> Result<u64, DbError> {
        let sql = count_rows_sql(schema, table, self.engine.dialect());
        match self.engine {
            Engine::Postgres => return pg::count_rows(self, database, &sql, cancel).await,
            Engine::Sqlite => return sqlite::count_rows(self, &sql, cancel).await,
            Engine::MySql => {}
        }
        let mut conn = self.open(Some(database), false).await?;
        // The connection id, so a second connection can KILL the scan — the same
        // shape `fetch_query` uses, and the only thing that actually stops work
        // already running on the server.
        let conn_id = conn.id();
        let out = tokio::select! {
            r = conn.query_first::<u64, _>(sql) => r
                .map_err(|e| DbError::Query(e.to_string()))
                .and_then(|n| n.ok_or_else(|| DbError::Query("COUNT(*) returned no row".into()))),
            _ = cancel.cancelled() => {
                self.kill_query(conn_id).await;
                Err(DbError::Cancelled)
            }
        };
        let _ = conn.disconnect().await;
        out
    }

    /// Every session currently connected to this server, with the lock waits
    /// between them — the Server Activity panel's whole input.
    ///
    /// Unsorted and uncapped here: [`schemaic_core::activity::prepare`] owns the
    /// ordering and the cut, so the panel and its tests see one answer. The
    /// queries do ask for [`MAX_SESSIONS`](schemaic_core::activity::MAX_SESSIONS)
    /// `+ 1` rows so that cut has something to notice.
    ///
    /// **Never the caller's own connection.** Every operation here opens a fresh
    /// one (ARCHITECTURE §7), so the poller would otherwise report itself running
    /// `SELECT … FROM information_schema.PROCESSLIST` at the top of every refresh
    /// — a row that exists only because someone looked.
    ///
    /// An engine with no sessions errors rather than returning nothing, and the
    /// app is expected not to ask — the gate is
    /// [`supports_activity`](schemaic_core::activity::supports_activity), asked
    /// as a **capability** rather than spelled out as another `== Sqlite`. The
    /// `match` below dispatches to a query set, which is a different question:
    /// there is one catalogue per engine and no capability can paper over that.
    /// What the predicate buys is the arm that *doesn't* exist — a fourth engine
    /// added to [`Engine`] stops here with one honest error instead of falling
    /// through to `information_schema.PROCESSLIST` and failing three catalogue
    /// lookups deep.
    pub async fn fetch_sessions(&self) -> Result<Vec<SessionInfo>, DbError> {
        if !activity::supports_activity(self.engine.dialect()) {
            return Err(DbError::Query(NO_SESSIONS_MSG.to_string()));
        }
        match self.engine {
            Engine::Postgres => return pg::fetch_sessions(self).await,
            Engine::MySql => {}
            // Unreachable — `supports_activity` above is the gate.
            Engine::Sqlite => return Err(DbError::Query(NO_SESSIONS_MSG.to_string())),
        }
        let mut conn = self.open(None, false).await?;
        let out = collect_sessions(&mut conn).await;
        let _ = conn.disconnect().await;
        out
    }

    /// Cancel a statement, or terminate a session outright, by server id.
    ///
    /// **A fresh connection, always.** The session being killed may be the one
    /// holding up everything else, and on MySQL a `KILL` issued from a connection
    /// that is itself waiting on that lock never gets sent — the same reason
    /// [`Db::kill_query`] opens its own.
    ///
    /// Gated on [`supports_kill`](schemaic_core::activity::supports_kill), the
    /// capability the panel's own menu asks — see [`Db::fetch_sessions`] for why
    /// that is not the same thing as the engine `match` below it.
    pub async fn kill_session(&self, id: i64, kind: KillKind) -> Result<(), DbError> {
        if !activity::supports_kill(self.engine.dialect()) {
            return Err(DbError::Query(NO_SESSIONS_MSG.to_string()));
        }
        match self.engine {
            Engine::Postgres => return pg::kill_session(self, id, kind).await,
            Engine::MySql => {}
            // Unreachable — `supports_kill` above is the gate.
            Engine::Sqlite => return Err(DbError::Query(NO_SESSIONS_MSG.to_string())),
        }
        // `id` is an `i64` the server itself reported and is formatted back as a
        // decimal, so there is nothing here a quoter would have to escape.
        let sql = match kind {
            KillKind::Query => format!("KILL QUERY {id}"),
            KillKind::Session => format!("KILL CONNECTION {id}"),
        };
        let mut conn = self.open(None, false).await?;
        let out = conn
            .query_drop(sql)
            .await
            .map_err(|e| DbError::Query(e.to_string()));
        let _ = conn.disconnect().await;
        out
    }

    /// Every account the server will tell us about.
    ///
    /// Gated on [`supports_users`](schemaic_core::users::supports_users) for the
    /// same reason [`Db::fetch_sessions`] is gated on `supports_activity`: the
    /// `match` below picks a *catalogue*, and a fourth engine added to [`Engine`]
    /// should stop here with one honest sentence rather than fall through to
    /// `mysql.user` and fail a lookup deep inside a driver.
    pub async fn fetch_principals(&self) -> Result<users::Principals, DbError> {
        if !users::supports_users(self.engine.dialect()) {
            return Err(DbError::Query(NO_USERS_MSG.to_string()));
        }
        match self.engine {
            Engine::Postgres => return pg::fetch_principals(self).await,
            Engine::MySql => {}
            // Unreachable — `supports_users` above is the gate.
            Engine::Sqlite => return Err(DbError::Query(NO_USERS_MSG.to_string())),
        }
        let mut conn = self.open(None, false).await?;
        let out = collect_my_users(&mut conn).await;
        let _ = conn.disconnect().await;
        out
    }

    /// What one account is allowed to do, as `GRANT` statements.
    ///
    /// `database` is the database PostgreSQL's per-database privileges are read
    /// from — see [`users::pg_scope_note`] for why one connection can only ever
    /// answer for one — and is ignored on MySQL, whose grant tables are
    /// server-wide and answer for every database at once.
    pub async fn fetch_grants(
        &self,
        database: Option<&str>,
        principal: &Principal,
    ) -> Result<Grants, DbError> {
        if !users::supports_users(self.engine.dialect()) {
            return Err(DbError::Query(NO_USERS_MSG.to_string()));
        }
        match self.engine {
            Engine::Postgres => return pg::fetch_grants(self, database, principal).await,
            Engine::MySql => {}
            // Unreachable — `supports_users` above is the gate.
            Engine::Sqlite => return Err(DbError::Query(NO_USERS_MSG.to_string())),
        }
        // `SHOW GRANTS FOR` takes an account, not a placeholder, so the pair goes
        // in as SQL — through `users::account_sql`, which is the one literal
        // quoting and the reason a host of `it's` cannot end the statement early.
        let sql = format!(
            "SHOW GRANTS FOR {}",
            users::account_sql(principal, SqlDialect::MySql)
        );
        let mut conn = self.open(None, false).await?;
        let out = conn
            .query_map(sql, |g: String| {
                // **Every statement out of here is redacted**, at the boundary
                // rather than in the view: MariaDB answers with the account's
                // password hash inline, and a second reader of this method — the
                // grant/revoke step, a copy-all button — would otherwise have to
                // remember to do it too.
                users::redact_secrets(&g, SqlDialect::MySql)
            })
            .await
            .map_err(|e| DbError::Query(e.to_string()))
            // **A note, not `None`.** `SHOW GRANTS` is direct-only on both
            // servers, so everything the account holds through a granted role is
            // absent from this list — and on a role-provisioned server that is
            // most of it. See `users::my_scope_note`.
            .map(|statements| Grants {
                statements,
                note: Some(users::my_scope_note()),
            });
        let _ = conn.disconnect().await;
        out
    }
}

/// Why a connection has no accounts to browse. One sentence, one place, so the
/// two methods that raise it can't drift apart — and, like [`NO_SESSIONS_MSG`],
/// it names the capability rather than the engine because that is what the
/// caller asked. The browser checks `supports_users` itself and shows its own
/// explanation; this is the backstop for a caller that didn't.
const NO_USERS_MSG: &str = "this connection's engine has no user accounts";

/// `mysql.user`, MariaDB's spelling. `is_role` is the flag that makes a role a
/// role, and only MariaDB has it.
const MY_USERS_MARIADB_SQL: &str = "SELECT CAST(User AS CHAR), CAST(Host AS CHAR), \
            CAST(plugin AS CHAR), CAST(password_expired AS CHAR), CAST(is_role AS CHAR) \
     FROM mysql.user ORDER BY User, Host";

/// The same, MySQL 8's spelling — `account_locked` in place of `is_role`, which
/// does not exist there.
const MY_USERS_MYSQL_SQL: &str = "SELECT CAST(User AS CHAR), CAST(Host AS CHAR), \
            CAST(plugin AS CHAR), CAST(password_expired AS CHAR), CAST(account_locked AS CHAR) \
     FROM mysql.user ORDER BY User, Host";

/// **`is_role` on its own**, for a MariaDB that has roles but not password
/// expiry — every 10.1, 10.2 and 10.3, which is a window still in service.
///
/// Without this rung such a server fell through to the pair below, where
/// `is_role` is `None` on every row, so `from_mysql_rows` folded each role into
/// a `User` and kept the host MariaDB stores as `''`. A role `readers` then
/// listed as `readers@` and three of the four actions built from it are errors
/// the model already documents: `SHOW GRANTS FOR 'readers'@''` is 1141,
/// `GRANT … TO 'readers'@''` is 1133, and `DROP ROLE` has no `@host` grammar at
/// all. Role-ness is the one column whose absence changes what a statement
/// *is*, so it gets a rung of its own rather than sharing the pair's.
const MY_USERS_ROLE_SQL: &str = "SELECT CAST(User AS CHAR), CAST(Host AS CHAR), CAST(is_role AS CHAR) \
     FROM mysql.user ORDER BY User, Host";

/// The pair alone, for a server that has none of the extra columns — and for the
/// version of `mysql.user` a future release trims again.
const MY_USERS_PLAIN_SQL: &str =
    "SELECT CAST(User AS CHAR), CAST(Host AS CHAR) FROM mysql.user ORDER BY User, Host";

/// The last resort, and the one an *application* account can actually read.
///
/// `mysql.user` needs `SELECT` on the `mysql` database. A connection that hasn't
/// got it — which is every properly-provisioned application account — can still
/// read `information_schema.USER_PRIVILEGES`, where it sees its own row. One
/// account is a poor list, but it is a true one, and it is the account whose
/// grants the person opening this is most likely asking about.
const MY_USERS_GRANTEE_SQL: &str =
    "SELECT DISTINCT GRANTEE FROM information_schema.USER_PRIVILEGES ORDER BY GRANTEE";

/// One `mysql.user` row as the two wide queries project it.
type MyUserTuple = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// The MySQL/MariaDB half of [`Db::fetch_principals`]: four queries, of which
/// exactly one runs to completion.
///
/// **The fallbacks fire on an *error*, not on an empty result** — the same rule
/// the lock-wait pair in [`collect_sessions`] had to learn. `mysql.user` is never
/// legitimately empty (the server cannot run without accounts), so an empty
/// answer would mean the query was denied, and an error is what actually says
/// so. Trying MariaDB's column first and MySQL's second costs one failed
/// round-trip on MySQL and none on MariaDB, and there is no version probe that
/// would be cheaper: `SELECT @@version` is itself a round-trip, and it answers
/// the wrong question — what matters is which columns this build of `mysql.user`
/// has, not what it calls itself.
async fn collect_my_users(conn: &mut Conn) -> Result<users::Principals, DbError> {
    if let Ok(rows) = conn
        .query_map(MY_USERS_MARIADB_SQL, |r: MyUserTuple| r)
        .await
    {
        return Ok(users::Principals::complete(users::from_mysql_rows(
            &my_user_rows(rows, true),
        )));
    }
    if let Ok(rows) = conn.query_map(MY_USERS_MYSQL_SQL, |r: MyUserTuple| r).await {
        return Ok(users::Principals::complete(users::from_mysql_rows(
            &my_user_rows(rows, false),
        )));
    }
    if let Ok(rows) = conn
        .query_map(MY_USERS_ROLE_SQL, |r: (String, String, Option<String>)| r)
        .await
    {
        return Ok(users::Principals::complete(users::from_mysql_rows(
            &my_role_rows(rows),
        )));
    }
    if let Ok(rows) = conn
        .query_map(MY_USERS_PLAIN_SQL, |(u, h): (String, String)| MyUserRow {
            user: u,
            host: h,
            ..Default::default()
        })
        .await
    {
        return Ok(users::Principals::complete(users::from_mysql_rows(&rows)));
    }
    // The one whose failure the caller sees, because by here every wider read
    // has already been refused and this error is the reason why.
    let grantees: Vec<String> = conn
        .query_map(MY_USERS_GRANTEE_SQL, |g: String| g)
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;
    let rows: Vec<MyUserRow> = grantees
        .iter()
        // A cell that isn't an account pair is dropped rather than guessed at —
        // see `users::parse_grantee`.
        .filter_map(|g| users::parse_grantee(g))
        .map(|(user, host)| MyUserRow {
            user,
            host,
            ..Default::default()
        })
        .collect();
    // **Reaching here is news, and the list has to say so.** Every wider read
    // was refused, so what follows is this connection's own row and nothing
    // else — which renders identically to a server that genuinely has one
    // account. The three `if let Ok` above discard their errors deliberately
    // (a denied read is the expected case, not a failure), and that is exactly
    // why the *absence* has to be carried out rather than inferred from a count.
    Ok(users::Principals {
        list: users::from_mysql_rows(&rows),
        note: Some(users::my_own_account_only_note()),
    })
}

/// Slot the fifth column into whichever field this server's spelling meant it
/// for. The fold that reads it — `users::from_mysql_rows` — is where every
/// decision about what a [`Principal`] *says* lives, and it needs the two
/// columns kept apart rather than merged into a "flag" it would have to
/// re-interpret.
fn my_user_rows(rows: Vec<MyUserTuple>, mariadb: bool) -> Vec<MyUserRow> {
    rows.into_iter()
        .map(|(user, host, plugin, expired, fifth)| MyUserRow {
            user,
            host,
            plugin,
            password_expired: expired,
            is_role: if mariadb { fifth.clone() } else { None },
            account_locked: if mariadb { None } else { fifth },
        })
        .collect()
}

/// [`MY_USERS_ROLE_SQL`]'s three columns, as rows.
///
/// A named function rather than a closure inside the ladder, for the reason
/// [`my_user_rows`] is one: the mapping is the whole content of a rung, and a
/// rung whose mapping is inline is a rung no test can reach.
fn my_role_rows(rows: Vec<(String, String, Option<String>)>) -> Vec<MyUserRow> {
    rows.into_iter()
        .map(|(user, host, is_role)| MyUserRow {
            user,
            host,
            is_role,
            ..Default::default()
        })
        .collect()
}

#[cfg(test)]
mod my_user_tests {
    use super::{MyUserRow, my_role_rows, my_user_rows};

    /// **Which column the fifth one is.** Two bare boolean literals twelve lines
    /// apart decide it, and nothing in any tier asserted the result: transposing
    /// them makes every locked MySQL account a `Role`, drops its host, and
    /// `DROP USER "app"` then resolves to a *different* account. The live role
    /// test finds its role by name and never asks what kind it is.
    #[test]
    fn the_fifth_column_lands_in_the_field_this_servers_spelling_meant() {
        let row = |fifth: &str| {
            vec![(
                "app".to_string(),
                "%".to_string(),
                Some("plugin".to_string()),
                Some("N".to_string()),
                Some(fifth.to_string()),
            )]
        };
        // MariaDB's fifth column is `is_role`…
        let maria = my_user_rows(row("Y"), true);
        assert_eq!(maria[0].is_role.as_deref(), Some("Y"));
        assert_eq!(maria[0].account_locked, None);
        // …and MySQL 8's is `account_locked`, which does not make a role.
        let mysql = my_user_rows(row("Y"), false);
        assert_eq!(mysql[0].is_role, None);
        assert_eq!(mysql[0].account_locked.as_deref(), Some("Y"));
        // The other four columns are the same either way.
        assert_eq!(maria[0].user, mysql[0].user);
        assert_eq!(maria[0].host, mysql[0].host);
        assert_eq!(maria[0].plugin, mysql[0].plugin);
        assert_eq!(maria[0].password_expired, mysql[0].password_expired);
    }

    /// The rung that exists so a MariaDB with roles but no password expiry still
    /// knows a role when it sees one. Fed through `from_mysql_rows`, because
    /// what the missing column costs is a *principal*, not a field: a role kept
    /// as a `User` carries MariaDB's empty host into `'readers'@''`, which three
    /// of the four statements reject.
    #[test]
    fn the_role_rung_still_tells_a_role_from_a_user() {
        let rows = my_role_rows(vec![
            ("app".into(), "%".into(), Some("N".into())),
            ("readers".into(), String::new(), Some("Y".into())),
        ]);
        let out = schemaic_core::users::from_mysql_rows(&rows);
        let role = out
            .iter()
            .find(|p| p.name == "readers")
            .expect("the role is listed");
        assert_eq!(role.kind, schemaic_core::users::PrincipalKind::Role);
        // A role has no host, so nothing writes `'readers'@''`.
        assert_eq!(role.host, None);
        assert_eq!(role.display(), "readers");

        let user = out.iter().find(|p| p.name == "app").expect("the user");
        assert_eq!(user.kind, schemaic_core::users::PrincipalKind::User);
        assert_eq!(user.host.as_deref(), Some("%"));

        // And the rung below it — the bare pair — is what this one exists to
        // stop being reached on such a server: with no `is_role` the same role
        // folds into a user with an empty host.
        let bare = schemaic_core::users::from_mysql_rows(&[MyUserRow {
            user: "readers".into(),
            host: String::new(),
            ..Default::default()
        }]);
        assert_eq!(bare[0].kind, schemaic_core::users::PrincipalKind::User);
    }
}

/// Why a connection has no Server Activity to report. One sentence, one place,
/// so the two methods that raise it can't drift apart.
///
/// It names the *capability* rather than the engine, because that is what the
/// caller asked and because nothing renders this in the ordinary course: the app
/// checks `supports_activity` itself and shows the panel's own explanation
/// (`ActivityState::Unsupported`). This is the backstop for a caller that didn't.
const NO_SESSIONS_MSG: &str = "this connection's engine has no server sessions";

/// `information_schema.PROCESSLIST`, minus this connection and minus the
/// server's own internal threads.
///
/// `COMMAND <> 'Daemon'` drops the event scheduler and friends: they are threads,
/// not sessions — nobody connected them, nothing can kill them, and they would
/// sit at the top of the list forever with an uptime-length duration. A
/// replication `Binlog Dump` *is* a real client and stays.
///
/// **Working threads first, then longest-standing** — and the first half of that
/// is load-bearing. Ordering by `TIME` alone reads as "keep the interesting end
/// of the list", but the panel's own attention order
/// ([`schemaic_core::activity::rank`]) puts *blocked* sessions at
/// the top, and a session that started waiting four seconds ago has the smallest
/// `TIME` on the server. On a box holding three thousand pool connections idle
/// for hours, `ORDER BY TIME DESC LIMIT 501` returned five hundred sleepers and
/// cut every row of the lock pile-up the panel was opened for — a quiet-looking
/// list during an incident.
///
/// `COMMAND <> 'Sleep'` is the proxy for "doing something", and it is a proxy on
/// purpose: a blocked thread on MySQL sits in `Query` while it waits, but
/// `PROCESSLIST` itself carries no lock information, and the view that does
/// (`INNODB_TRX`) needs `PROCESS` privileges this statement deliberately does not
/// require — see [`collect_sessions`]. Sorting by what every account can see
/// keeps the required query required.
///
/// **`USER <> 'system user'` drops the server's own threads.** A replica's
/// applier and receiver are threads, not sessions — nobody connected them, they
/// have no host, their `TIME` is the replica's uptime, and terminating one stops
/// replication — but they are not `COMMAND = 'Daemon'` (MariaDB reports
/// `Slave_IO`/`Slave_SQL`, MySQL 8 `Connect`/`Query`) and not `Sleep` either, so
/// they sat at the very *top* of the list forever with a live "Kill session"
/// under them. The account is what both engines have in common for them, and it
/// is what `SHOW PROCESSLIST` readers filter on. `Binlog Dump` still stays: that
/// is the primary side, and it really is a client.
const MY_PROCESSLIST_SQL: &str = "SELECT ID, USER, HOST, DB, COMMAND, TIME, INFO \
     FROM information_schema.PROCESSLIST \
     WHERE ID <> CONNECTION_ID() AND COMMAND <> 'Daemon' AND USER <> 'system user' \
     ORDER BY (COMMAND <> 'Sleep') DESC, TIME DESC LIMIT ";

/// Open InnoDB transactions, keyed by the thread holding them. This is what
/// separates an idle pool connection from a client that went away mid-transaction
/// — see [`schemaic_core::activity::mysql_state`].
const MY_INNODB_TRX_SQL: &str =
    "SELECT trx_mysql_thread_id, trx_state FROM information_schema.INNODB_TRX";

/// Who is waiting on whom, MySQL 8 spelling. `performance_schema.data_lock_waits`
/// names transactions, so `INNODB_TRX` maps them back to the thread ids the rest
/// of the panel is keyed by.
const MY_LOCK_WAITS_PS_SQL: &str = "SELECT rt.trx_mysql_thread_id, bt.trx_mysql_thread_id \
     FROM performance_schema.data_lock_waits w \
     JOIN information_schema.INNODB_TRX rt ON rt.trx_id = w.REQUESTING_ENGINE_TRANSACTION_ID \
     JOIN information_schema.INNODB_TRX bt ON bt.trx_id = w.BLOCKING_ENGINE_TRANSACTION_ID";

/// The same graph, MariaDB spelling. MariaDB has no `data_lock_waits` and MySQL 8
/// removed `INNODB_LOCK_WAITS`, so neither statement works on both servers and
/// the pair is tried in turn.
///
/// If *both* fail — an account without `PROCESS`, or a build with InnoDB's lock
/// views compiled out — the panel still knows **who** is blocked (that comes from
/// `trx_state`, above) and simply cannot say by whom. That is the honest
/// degradation: a `Blocked` row with no "waiting on…" note, rather than a list
/// that quietly claims nothing is wrong.
const MY_LOCK_WAITS_IS_SQL: &str = "SELECT rt.trx_mysql_thread_id, bt.trx_mysql_thread_id \
     FROM information_schema.INNODB_LOCK_WAITS w \
     JOIN information_schema.INNODB_TRX rt ON rt.trx_id = w.requesting_trx_id \
     JOIN information_schema.INNODB_TRX bt ON bt.trx_id = w.blocking_trx_id";

/// One `PROCESSLIST` row as `mysql_async` hands it back:
/// `(id, user, host, db, command, time, info)`. Reshaped into
/// [`activity::MyProcessRow`] for the fold.
type MyProcessRow = (
    i64,
    String,
    String,
    Option<String>,
    String,
    i64,
    Option<String>,
);

/// Run the three activity queries on one connection and fold them into
/// [`SessionInfo`]s.
///
/// The process list is required — without it there is no panel — while the
/// transaction and lock-wait views are best effort, because they are the two that
/// need `PROCESS` privileges and differ by server. A user who can see their own
/// sessions and nothing else still gets a working panel.
async fn collect_sessions(conn: &mut Conn) -> Result<Vec<SessionInfo>, DbError> {
    let list_sql = format!("{MY_PROCESSLIST_SQL}{}", activity::MAX_SESSIONS + 1);
    let rows: Vec<MyProcessRow> = conn
        .query_map(list_sql, |r: MyProcessRow| r)
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;

    let trx: HashMap<i64, String> = conn
        .query_map(MY_INNODB_TRX_SQL, |(id, state): (i64, String)| (id, state))
        .await
        .map(|v| v.into_iter().collect())
        .unwrap_or_default();

    // MySQL 8 first, MariaDB second — only one of them exists on any given
    // server.
    //
    // **The fallback fires on an *error*, not on an empty result**, which is the
    // condition it actually means. `waits.is_empty()` could not tell "the
    // performance_schema view found no waits" — the ordinary case, since most
    // polls find none — from "the view does not exist", so on MySQL 8 every
    // quiet poll went on to run `information_schema.INNODB_LOCK_WAITS`, which
    // 8.0 removed, and paid a guaranteed round-trip failure forever.
    let waits: Vec<(i64, i64)> = match conn
        .query_map(MY_LOCK_WAITS_PS_SQL, |r: (i64, i64)| r)
        .await
    {
        Ok(v) => v,
        Err(_) => conn
            .query_map(MY_LOCK_WAITS_IS_SQL, |r: (i64, i64)| r)
            .await
            .unwrap_or_default(),
    };
    // The fold is `activity::from_mysql_rows` — it is where every decision about
    // what a `SessionInfo` *says* lives, and it needs to be reachable from a
    // test with a literal row vector.
    let rows: Vec<activity::MyProcessRow> = rows
        .into_iter()
        .map(
            |(id, user, host, database, command, seconds, info)| activity::MyProcessRow {
                id,
                user,
                host,
                database,
                command,
                seconds,
                info,
            },
        )
        .collect();
    Ok(activity::from_mysql_rows(&rows, &trx, &waits))
}

/// One `information_schema.TABLES` statistics row. Every figure is nullable —
/// a view has none of them, and `AUTO_INCREMENT` is null on a table without one.
type MyStatRow = (
    String,
    Option<u64>,
    Option<u64>,
    Option<u64>,
    Option<u64>,
    Option<u64>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// Sizes and estimates. `CAST(… AS CHAR)` on the timestamps because these are
/// shown, not computed with, and the server's own rendering is the one the user
/// would see in a client.
const MY_TABLE_STATS_SQL: &str = "SELECT CAST(TABLE_NAME AS CHAR), TABLE_ROWS, DATA_LENGTH, \
            INDEX_LENGTH, DATA_FREE, AUTO_INCREMENT, CAST(ROW_FORMAT AS CHAR), \
            CAST(ENGINE AS CHAR), CAST(CREATE_TIME AS CHAR), CAST(UPDATE_TIME AS CHAR) \
     FROM information_schema.TABLES \
     WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME";

/// Cardinality per index. `information_schema.STATISTICS` has one row per key
/// *position*, each carrying the cardinality of the prefix ending there, so the
/// index's own figure is the last one — `MAX` over the group. Reading a row
/// instead would report the **first** column's distinct count as the whole
/// index's, which on a `(status, created_at)` index is a handful against
/// millions. `NON_UNIQUE` is constant within a group; `MIN` just picks it out.
const MY_INDEX_CARDINALITY_SQL: &str = "SELECT CAST(TABLE_NAME AS CHAR), CAST(INDEX_NAME AS CHAR), \
            MAX(CARDINALITY), MIN(NON_UNIQUE) \
     FROM information_schema.STATISTICS \
     WHERE TABLE_SCHEMA = ? \
     GROUP BY TABLE_NAME, INDEX_NAME \
     ORDER BY TABLE_NAME, INDEX_NAME";

/// How often each index has actually been used. Performance Schema is the only
/// place MySQL keeps this, and it is routinely off, not instrumented, or not
/// granted — all of which must leave the scan count **absent** rather than zero,
/// because zero is what marks an index unused. So a failure here drops the whole
/// map and every index reports "we don't know".
///
/// `INDEX_NAME IS NOT NULL` because the same view carries a row for the table
/// itself, which is not an index and would otherwise be counted as one.
const MY_INDEX_USAGE_SQL: &str = "SELECT CAST(OBJECT_NAME AS CHAR), CAST(INDEX_NAME AS CHAR), COUNT_STAR \
     FROM performance_schema.table_io_waits_summary_by_index_usage \
     WHERE OBJECT_SCHEMA = ? AND INDEX_NAME IS NOT NULL";

/// The MySQL/MariaDB half of [`Db::fetch_table_stats`]: three queries, only the
/// first of which is required. The rows become a [`SchemaStats`] in
/// [`map_mysql_stats`], which is where the decisions are and is therefore where
/// the tests are.
async fn collect_table_stats(conn: &mut Conn, database: &str) -> Result<SchemaStats, DbError> {
    let qerr = |e: mysql_async::Error| DbError::Query(e.to_string());

    let rows: Vec<MyStatRow> = conn
        .exec_map(MY_TABLE_STATS_SQL, (database,), |r: MyStatRow| r)
        .await
        .map_err(qerr)?;

    let idx_rows: Vec<(String, String, Option<u64>, Option<i64>)> = conn
        .exec_map(
            MY_INDEX_CARDINALITY_SQL,
            (database,),
            |r: (String, String, Option<u64>, Option<i64>)| r,
        )
        .await
        .map_err(qerr)?;

    let usage: HashMap<(String, String), u64> = conn
        .exec_map(
            MY_INDEX_USAGE_SQL,
            (database,),
            |(t, i, n): (String, String, u64)| ((t, i), n),
        )
        .await
        .map(|v| v.into_iter().collect())
        .unwrap_or_default();

    // How stale the figures above may be. MySQL 8 serves them from a cache whose
    // maximum age is this variable — 86400 (a day) out of the box, which is long
    // enough that a size can be badly wrong and the user has to be told. MariaDB
    // has no such variable and the statement errors there, which is the honest
    // `Unknown`: its statistics are refreshed on a different rule entirely.
    let freshness = match conn
        .query_first::<u64, _>("SELECT @@information_schema_stats_expiry")
        .await
    {
        Ok(Some(secs)) => Freshness::CachedUpTo(secs),
        _ => Freshness::Unknown,
    };

    Ok(map_mysql_stats(rows, &idx_rows, &usage, freshness))
}

/// The three MySQL statistics queries' rows, as the model the panel reads.
///
/// Pure, and separate from [`collect_table_stats`] because every decision in this
/// feature's MySQL half is here rather than in the round trip: which indexes
/// belong to which table, what makes one unique, and — the one that decides
/// whether an index gets flagged for deletion — that a missing `usage` entry
/// leaves `scans` **absent** rather than zero.
fn map_mysql_stats(
    rows: Vec<MyStatRow>,
    idx_rows: &[(String, String, Option<u64>, Option<i64>)],
    usage: &HashMap<(String, String), u64>,
    freshness: Freshness,
) -> SchemaStats {
    let mut by_table: HashMap<&str, Vec<IndexStats>> = HashMap::new();
    for (table, index, cardinality, non_unique) in idx_rows {
        let is_primary = index == "PRIMARY";
        by_table.entry(table).or_default().push(IndexStats {
            name: index.clone(),
            // MySQL reports one `INDEX_LENGTH` for the whole table and never
            // breaks it down, so no index here has a size of its own.
            bytes: None,
            cardinality: *cardinality,
            // Absent, not zero: Performance Schema is routinely off or ungranted,
            // and zero is what `IndexStats::is_unused` reads as "drop me".
            scans: usage.get(&(table.clone(), index.clone())).copied(),
            is_primary,
            // The primary key is unique without saying so — `NON_UNIQUE` is 0 for
            // it too, but the flag is what the panel labels the row with, and a
            // key that failed this test would be labelled an ordinary index.
            is_unique: is_primary || *non_unique == Some(0),
        });
    }

    let tables = rows
        .into_iter()
        .map(
            |(name, rows, data, index, free, auto, row_format, engine, created, updated)| {
                let indexes = by_table.remove(name.as_str()).unwrap_or_default();
                TableStats {
                    indexes,
                    table: name,
                    schema: None,
                    rows,
                    exact_rows: None,
                    data_bytes: data,
                    index_bytes: index,
                    free_bytes: free,
                    dead_rows: None,
                    auto_increment: auto,
                    row_format,
                    engine,
                    created,
                    updated,
                    freshness: freshness.clone(),
                }
            },
        )
        .collect();
    SchemaStats::new(tables)
}

async fn collect_schema(conn: &mut Conn, database: &str) -> Result<DbSchema, DbError> {
    let qerr = |e: mysql_async::Error| DbError::Query(e.to_string());

    // Tables, ordered. `TABLE_TYPE` flags views ('VIEW') vs base tables so the
    // tree can render them distinctly; the engine/collation/comment behind it are
    // the table-level options the schema designer edits (and `ALTER TABLE`
    // replaces wholesale, so they have to be readable before they can be shown).
    let table_opt_rows: Vec<MyTableRow> = conn
        .exec_map(
            "SELECT CAST(TABLE_NAME AS CHAR) AS t, CAST(TABLE_TYPE AS CHAR) AS ty, \
                    CAST(ENGINE AS CHAR) AS eng, CAST(TABLE_COLLATION AS CHAR) AS coll, \
                    CAST(TABLE_COMMENT AS CHAR) AS cmt \
             FROM information_schema.TABLES \
             WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME",
            (database,),
            |r: MyTableRow| r,
        )
        .await
        .map_err(qerr)?;
    let table_rows: Vec<(String, String)> = table_opt_rows
        .iter()
        .map(|(t, ty, ..)| (t.clone(), ty.clone()))
        .collect();

    // Which server this is, for `mysql_column`'s default normalization — MariaDB
    // hands back SQL text where MySQL hands back a raw value, and nothing in the
    // catalogue itself says which. One extra row per schema fetch.
    let mariadb: bool = conn
        .query_first::<String, _>("SELECT VERSION()")
        .await
        .map_err(qerr)?
        .is_some_and(|v| v.to_ascii_lowercase().contains("mariadb"));

    // Columns for the whole schema in one pass, grouped back onto their tables.
    let col_rows: Vec<ColRow> = conn
        .exec_map(
            "SELECT CAST(TABLE_NAME AS CHAR) AS t, \
                    CAST(COLUMN_NAME AS CHAR) AS c, \
                    CAST(COLUMN_TYPE AS CHAR) AS ty, \
                    CAST(IS_NULLABLE AS CHAR) AS nullable, \
                    CAST(COLUMN_KEY AS CHAR) AS ck, \
                    CAST(COLUMN_DEFAULT AS CHAR) AS def, \
                    CAST(EXTRA AS CHAR) AS extra, \
                    CAST(COLLATION_NAME AS CHAR) AS coll, \
                    CAST(COLUMN_COMMENT AS CHAR) AS cmt, \
                    CAST(GENERATION_EXPRESSION AS CHAR) AS genexpr \
             FROM information_schema.COLUMNS \
             WHERE TABLE_SCHEMA = ? \
             ORDER BY TABLE_NAME, ORDINAL_POSITION",
            (database,),
            |r: MyColRow| r,
        )
        .await
        .map_err(qerr)?
        .into_iter()
        .map(|r| mysql_column(r, mariadb))
        .collect();

    // Foreign keys, one row per referencing key-column with its referenced
    // target, ordered so a composite key's columns fold in order. Drives both the
    // FOREIGN index tag (below) and the grid's "Follow FK" navigation. The
    // `REFERENCED_TABLE_NAME IS NOT NULL` filter keeps only FK usages (the same
    // view lists plain PK/unique key usages with NULL references).
    let fk_col_rows: Vec<FkColRow> = conn
        .exec_map(
            "SELECT CAST(TABLE_NAME AS CHAR) AS t, \
                    CAST(CONSTRAINT_NAME AS CHAR) AS cn, \
                    CAST(COLUMN_NAME AS CHAR) AS col, \
                    CAST(REFERENCED_TABLE_SCHEMA AS CHAR) AS rs, \
                    CAST(REFERENCED_TABLE_NAME AS CHAR) AS rt, \
                    CAST(REFERENCED_COLUMN_NAME AS CHAR) AS rc \
             FROM information_schema.KEY_COLUMN_USAGE \
             WHERE TABLE_SCHEMA = ? AND REFERENCED_TABLE_NAME IS NOT NULL \
             ORDER BY TABLE_NAME, CONSTRAINT_NAME, ORDINAL_POSITION",
            (database,),
            |(t, cn, col, rs, rt, rc): FkColRow| (t, cn, col, rs, rt, rc),
        )
        .await
        .map_err(qerr)?;

    // Each FK's referential actions, keyed by constraint. Separate from the
    // key-column rows above because they're per *constraint*, not per column —
    // and they can't be skipped: a schema editor that drops and recreates a
    // `ON DELETE CASCADE` key without restating the action silently turns it into
    // `NO ACTION`.
    let fk_rule_rows: Vec<(String, String, String, String)> = conn
        .exec_map(
            "SELECT CAST(TABLE_NAME AS CHAR) AS t, CAST(CONSTRAINT_NAME AS CHAR) AS cn, \
                    CAST(DELETE_RULE AS CHAR) AS dr, CAST(UPDATE_RULE AS CHAR) AS ur \
             FROM information_schema.REFERENTIAL_CONSTRAINTS \
             WHERE CONSTRAINT_SCHEMA = ?",
            (database,),
            |r: (String, String, String, String)| r,
        )
        .await
        .map_err(qerr)?;

    // Indexes: one row per (index, key-column); fold consecutive columns into
    // the same index, preserving `SEQ_IN_INDEX` order.
    let idx_rows: Vec<IdxRow> = conn
        .exec_map(
            "SELECT CAST(TABLE_NAME AS CHAR) AS t, \
                    CAST(INDEX_NAME AS CHAR) AS i, \
                    CAST(NON_UNIQUE AS SIGNED) AS nu, \
                    CAST(COLUMN_NAME AS CHAR) AS c, \
                    CAST(SUB_PART AS SIGNED) AS sub, \
                    CAST(COLLATION AS CHAR) AS coll \
             FROM information_schema.STATISTICS \
             WHERE TABLE_SCHEMA = ? \
             ORDER BY TABLE_NAME, INDEX_NAME, SEQ_IN_INDEX",
            (database,),
            |r: (String, String, i64, String, Option<i64>, Option<String>)| r,
        )
        .await
        .map_err(qerr)?
        .into_iter()
        .map(|(t, i, nu, c, sub, coll)| IdxRow {
            table: t,
            index: i,
            unique: nu == 0,
            column: schemaic_core::schema::IndexColumn {
                name: c,
                // A prefix index (`KEY (bio(20))`) — recreating it without the
                // length fails outright on a TEXT column.
                prefix: sub.and_then(|n| u32::try_from(n).ok()),
                // `COLLATION` is 'A' ascending, 'D' descending, NULL unsorted.
                descending: coll.as_deref() == Some("D"),
                // MySQL 8 does have functional key parts, but `STATISTICS` names
                // the hidden generated column they create rather than the
                // expression, so nothing here can read one back — it stays a
                // column, as it was before this field existed.
                expression: false,
                // MySQL collates per column, not per index key.
                collation: None,
            },
            // MySQL's index type is only worth restating when it isn't the
            // default; BTREE is, so emitting `USING BTREE` everywhere would be
            // noise in every generated statement.
            method: None,
            predicate: None,
            // MySQL's `STATISTICS` gives the whole key — prefix and direction
            // included — so nothing is being read past here. (Functional indexes
            // exist on MySQL 8 / MariaDB 10.5 too, but they appear as hidden
            // generated columns and so arrive as ordinary column names.)
            lossy: false,
        })
        .collect();

    // Views (only if the schema has any): the stored SELECT body, plus the
    // options a `CREATE OR REPLACE VIEW` **resets** when it doesn't restate them
    // — the check option, the definer, and the security type. The last of those
    // is a privilege: a view redefined without `SQL SECURITY DEFINER` starts
    // running as whoever calls it. Reading them here is what lets the schema
    // editor carry them through an edit (see `core::schema::ViewOptions`).
    let has_views = table_rows
        .iter()
        .any(|(_, ty)| ty.eq_ignore_ascii_case("VIEW"));
    let view_sql = format!(
        "SELECT CAST(TABLE_NAME AS CHAR) AS t, CAST(VIEW_DEFINITION AS CHAR) AS def, \
                CAST(CHECK_OPTION AS CHAR) AS chk, CAST(DEFINER AS CHAR) AS definer, \
                CAST(SECURITY_TYPE AS CHAR) AS sec, {} AS algo \
             FROM information_schema.VIEWS \
             WHERE TABLE_SCHEMA = ?",
        // MariaDB reports the view's ALGORITHM; MySQL 8 doesn't have the column
        // at all (only `SHOW CREATE VIEW` knows), and naming a column that
        // doesn't exist fails the whole query — so the row *shape* is held
        // steady with a NULL instead of branching the parsing.
        if mariadb {
            "CAST(ALGORITHM AS CHAR)"
        } else {
            "CAST(NULL AS CHAR)"
        }
    );
    let view_opt_rows: Vec<MyViewRow> = if has_views {
        conn.exec_map(view_sql.as_str(), (database,), |r: MyViewRow| r)
            .await
            .map_err(qerr)?
    } else {
        Vec::new()
    };
    let view_rows: Vec<(String, String)> = view_opt_rows
        .iter()
        .map(|(t, def, ..)| (t.clone(), def.clone()))
        .collect();

    // CHECK constraints. The two servers put them in different places:
    //
    // * **MySQL 8.0.16+** — `CHECK_CONSTRAINTS` carries only the clause, with no
    //   `TABLE_NAME`, so the table comes from a join onto `TABLE_CONSTRAINTS`,
    //   which is also the only place `ENFORCED` lives.
    // * **MariaDB 10.2+** — `CHECK_CONSTRAINTS` has `TABLE_NAME` itself, and
    //   there is no `NOT ENFORCED` to report.
    //
    // Anything older has no check constraints *and* no `CHECK_CONSTRAINTS` table
    // — MySQL 5.7 parsed the clause and threw it away. Naming a missing table
    // fails the query, so *that* error degrades to "no checks" rather than
    // taking the whole schema fetch down with it.
    //
    // Only that error. A blanket `unwrap_or_default` would turn a typo in the
    // query above into every table quietly reporting no constraints, which is
    // this feature's own bug wearing a disguise: the designer would then build a
    // `CREATE TABLE` that drops checks the server really has.
    //
    // `LEVEL` is MariaDB's alone and is not cosmetic: a `Column` check is part
    // of the column definition `MODIFY COLUMN` replaces, so the emitter has to
    // restate it or the server deletes it (see `CheckInfo::column_level`). MySQL
    // has no such thing — it rewrites the same syntax into a table constraint at
    // `CREATE` time — so that branch reports `Table` for every row.
    let check_sql = if mariadb {
        "SELECT CAST(TABLE_NAME AS CHAR) AS t, CAST(CONSTRAINT_NAME AS CHAR) AS cn, \
                CAST(CHECK_CLAUSE AS CHAR) AS cc, 'YES' AS enforced, \
                CAST(LEVEL AS CHAR) AS lvl \
         FROM information_schema.CHECK_CONSTRAINTS \
         WHERE CONSTRAINT_SCHEMA = ?"
    } else {
        "SELECT CAST(tc.TABLE_NAME AS CHAR) AS t, CAST(cc.CONSTRAINT_NAME AS CHAR) AS cn, \
                CAST(cc.CHECK_CLAUSE AS CHAR) AS cc, CAST(tc.ENFORCED AS CHAR) AS enforced, \
                'Table' AS lvl \
         FROM information_schema.CHECK_CONSTRAINTS cc \
         JOIN information_schema.TABLE_CONSTRAINTS tc \
           ON tc.CONSTRAINT_SCHEMA = cc.CONSTRAINT_SCHEMA \
          AND tc.CONSTRAINT_NAME = cc.CONSTRAINT_NAME \
          AND tc.CONSTRAINT_TYPE = 'CHECK' \
         WHERE cc.CONSTRAINT_SCHEMA = ?"
    };
    // MariaDB grew `LEVEL` in 10.5; 10.2-10.4 have the table without it. Losing
    // that column must not cost the whole database its check constraints, so a
    // missing-column error retries without it — those servers then behave as
    // Schemaic did before the column was read at all.
    let check_fallback = "SELECT CAST(TABLE_NAME AS CHAR) AS t, CAST(CONSTRAINT_NAME AS CHAR) AS cn, \
                CAST(CHECK_CLAUSE AS CHAR) AS cc, 'YES' AS enforced, 'Table' AS lvl \
         FROM information_schema.CHECK_CONSTRAINTS \
         WHERE CONSTRAINT_SCHEMA = ?";
    let check_rows: Vec<MyCheckRow> = match conn
        .exec_map(check_sql, (database,), |r: MyCheckRow| r)
        .await
    {
        Ok(rows) => rows,
        // 1109 `ER_UNKNOWN_TABLE` / 1146 `ER_NO_SUCH_TABLE`: the server predates
        // check constraints, so there are none to report.
        Err(mysql_async::Error::Server(e)) if e.code == 1109 || e.code == 1146 => Vec::new(),
        // 1054 `ER_BAD_FIELD_ERROR`: a MariaDB too old for `LEVEL`.
        Err(mysql_async::Error::Server(e)) if e.code == 1054 && mariadb => conn
            .exec_map(check_fallback, (database,), |r: MyCheckRow| r)
            .await
            .map_err(qerr)?,
        Err(e) => return Err(qerr(e)),
    };

    // Triggers. `information_schema.TRIGGERS` has been there since MySQL 5.0 and
    // `ACTION_ORDER` since 5.7.2 / MariaDB 10.2.3, both well below anything this
    // app connects to — so unlike CHECK_CONSTRAINTS there is no missing-table
    // case to degrade for, and a failure here is a real failure.
    let trigger_rows: Vec<MyTriggerRow> = conn
        .exec_map(
            "SELECT CAST(EVENT_OBJECT_TABLE AS CHAR) AS t, CAST(TRIGGER_NAME AS CHAR) AS n, \
                    CAST(ACTION_TIMING AS CHAR) AS ti, CAST(EVENT_MANIPULATION AS CHAR) AS ev, \
                    CAST(ACTION_STATEMENT AS CHAR) AS st, CAST(DEFINER AS CHAR) AS df, \
                    COALESCE(ACTION_ORDER, 0) AS ord \
             FROM information_schema.TRIGGERS \
             WHERE TRIGGER_SCHEMA = ?",
            (database,),
            |r: MyTriggerRow| r,
        )
        .await
        .map_err(qerr)?;

    // Stored routines. `information_schema.ROUTINES` and `PARAMETERS` have both
    // been there since 5.0, so — as with `TRIGGERS` — there is no
    // missing-table case to degrade for and a failure here is a real failure.
    //
    // `ORDINAL_POSITION = 0` is a *function's return value*, not a parameter;
    // left in, every function's rendered signature would open with its return
    // type. `PARAMETER_MODE` is reported as `IN` for a **function's** parameters
    // too, where `CREATE FUNCTION` has no grammar for it — which is
    // [`mysql_parameters`]' first job, and why the mode is not simply joined in
    // here.
    //
    // `SQL_MODE`/`CHARACTER_SET_CLIENT`/`COLLATION_CONNECTION` are the session
    // state the recreate has to restore, and the catalogue carries the same
    // values `SHOW CREATE` prints. Read here so a draft is never without them:
    // the editor's lazy `SHOW CREATE` corrects the *body*, and a keystroke that
    // lands first must not be able to strip the wrapper off a `CREATE` whose
    // `DROP` has already committed.
    let routine_rows: Vec<MyRoutineRow> = conn
        .exec_map(MY_ROUTINES_SQL, (database,), |r: Row| my_routine_row(&r))
        .await
        .map_err(qerr)?;
    let param_rows: Vec<MyParamRow> = conn
        .exec_map(
            "SELECT CAST(SPECIFIC_NAME AS CHAR) AS n, CAST(ROUTINE_TYPE AS CHAR) AS ty, \
                    CAST(COALESCE(PARAMETER_MODE, '') AS CHAR) AS mode, \
                    CAST(COALESCE(PARAMETER_NAME, '') AS CHAR) AS pname, \
                    CAST(DTD_IDENTIFIER AS CHAR) AS dtd, \
                    CAST(CHARACTER_SET_NAME AS CHAR) AS cs, \
                    CAST(COLLATION_NAME AS CHAR) AS coll \
             FROM information_schema.PARAMETERS \
             WHERE SPECIFIC_SCHEMA = ? AND ORDINAL_POSITION > 0 \
             ORDER BY SPECIFIC_NAME, ORDINAL_POSITION",
            (database,),
            |r: MyParamRow| r,
        )
        .await
        .map_err(qerr)?;
    let params = mysql_parameters(&param_rows);

    // Scheduled events. `information_schema.EVENTS` has been there since MySQL
    // 5.1 and MariaDB 5.1, but unlike `TRIGGERS` this **degrades** rather than
    // failing the whole read: the MySQL-protocol servers that aren't MySQL
    // (TiDB, Vitess and friends) are exactly the ones that may not implement the
    // scheduler, and a database whose tables can't be browsed because it has no
    // events table is a far worse outcome than one whose Events folder is empty.
    // The same call `CHECK_CONSTRAINTS` above makes, and the same two codes.
    let event_rows: Vec<MyEventRow> = match conn
        .exec_map(MY_EVENTS_SQL, (database,), |r: Row| my_event_row(&r))
        .await
    {
        Ok(rows) => rows,
        // 1109 `ER_UNKNOWN_TABLE` / 1146 `ER_NO_SUCH_TABLE`: no such catalogue,
        // so there are no events to report.
        Err(mysql_async::Error::Server(e)) if e.code == 1109 || e.code == 1146 => Vec::new(),
        Err(e) => return Err(qerr(e)),
    };

    let mut schema = assemble_schema(
        // MySQL: the database is the namespace, so tables carry none.
        None,
        &table_rows,
        &col_rows,
        &fk_col_rows,
        &idx_rows,
        &view_rows,
    );
    schema.routines = mysql_routines(&routine_rows, &params)
        .into_iter()
        .map(std::sync::Arc::new)
        .collect();
    schema.events = mysql_events(&event_rows)
        .into_iter()
        .map(std::sync::Arc::new)
        .collect();
    apply_table_options(&mut schema, &table_opt_rows);
    apply_view_options(&mut schema, &view_opt_rows);
    apply_fk_rules(&mut schema, &fk_rule_rows);
    apply_check_constraints(&mut schema, &check_rows, mariadb);
    apply_triggers(&mut schema, mysql_triggers(&trigger_rows));
    // The flavour was computed at the top of this function and then thrown
    // away, so the emitter — which is where MySQL and MariaDB actually diverge
    // — had no way to ask. It rides on the schema now.
    schema.flavour = if mariadb {
        schemaic_core::schema::ServerFlavour::MariaDb
    } else {
        schemaic_core::schema::ServerFlavour::MySql
    };
    Ok(schema)
}

/// The `ALGORITHM` of a `SHOW CREATE VIEW` body, or `None` when it doesn't name
/// one (which is what `UNDEFINED` means, and what the emitter leaves unwritten).
///
/// Pure, because the shape it reads is narrow and positional: the clause is
/// always `CREATE ALGORITHM=… DEFINER=…`, before the definer and before any
/// user-controlled text, so scanning to the first `ALGORITHM=` can't be led
/// astray by a view *body* that happens to contain the word. Anchored to the
/// leading `CREATE` for the same reason.
fn view_algorithm_of(create_sql: &str) -> Option<String> {
    let head = create_sql.trim_start();
    let rest = head
        .strip_prefix("CREATE")
        .or_else(|| head.strip_prefix("create"))?;
    // Only the clause immediately after `CREATE` — `DEFINER` follows it, and
    // everything past that is the user's own SQL.
    let rest = rest.trim_start();
    let rest = rest
        .get(..9)
        .filter(|p| p.eq_ignore_ascii_case("ALGORITHM"))
        .map(|_| &rest[9..])?;
    let value = rest.trim_start().strip_prefix('=')?.trim_start();
    let end = value
        .find(|c: char| c.is_whitespace())
        .unwrap_or(value.len());
    let algo = value[..end].trim().to_ascii_uppercase();
    // `UNDEFINED` is the default the emitter deliberately doesn't restate.
    (!algo.is_empty() && algo != "UNDEFINED").then_some(algo)
}

/// One `SHOW CREATE TRIGGER` row: `(Trigger, sql_mode, SQL Original Statement,
/// character_set_client, collation_connection, Database Collation, Created)`.
///
/// `Created` is nullable — MySQL only started recording it in 5.7.2, and a
/// trigger made before an upgrade still has none.
type MyShowCreateTriggerRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
);

/// The **body** of a `SHOW CREATE TRIGGER` statement — everything after
/// `FOR EACH ROW` and any `FOLLOWS`/`PRECEDES` clause.
///
/// Positional, like [`view_algorithm_of`], but it cannot be a plain `find`: the
/// text before the body is server-generated, yet it contains *identifiers*, and
/// a table or trigger named `` `x FOR EACH ROW y` `` is legal. So the scan goes
/// through [`sql::skip_noncode`] on the MySQL dialect, which steps over a
/// backtick-quoted name whole. Everything after the anchor is the user's own
/// SQL and is returned untouched — including any `FOR EACH ROW` inside it,
/// which is why the **first** anchor is the right one.
///
/// The ordering clause is dropped rather than kept: `TriggerInfo::order` is
/// reconstructed from `information_schema.ACTION_ORDER` and the emitter writes
/// it back, so carrying it in the body too would emit it twice.
fn trigger_body_of(create_sql: &str) -> Option<String> {
    const ANCHOR: &str = "FOR EACH ROW";
    let b = create_sql.as_bytes();
    let mut i = 0usize;
    let after = loop {
        if i >= b.len() {
            return None;
        }
        if let Some(j) = sql::skip_noncode(b, i, SqlDialect::MySql) {
            i = j;
            continue;
        }
        if b[i..].len() >= ANCHOR.len()
            && b[i..i + ANCHOR.len()].eq_ignore_ascii_case(ANCHOR.as_bytes())
        {
            break i + ANCHOR.len();
        }
        i += 1;
    };
    let rest = create_sql.get(after..)?.trim_start();
    // An ordering clause, if the server printed one: the keyword, then one
    // identifier (which may be backtick-quoted and hold anything).
    for kw in ["FOLLOWS", "PRECEDES"] {
        if rest.len() >= kw.len() && rest.as_bytes()[..kw.len()].eq_ignore_ascii_case(kw.as_bytes())
        {
            let after_kw = rest[kw.len()..].trim_start();
            let nb = after_kw.as_bytes();
            let end = match sql::skip_noncode(nb, 0, SqlDialect::MySql) {
                Some(j) => j,
                None => nb
                    .iter()
                    .position(|&c| !sql::is_word_byte(c))
                    .unwrap_or(nb.len()),
            };
            return Some(after_kw[end..].trim().to_string());
        }
    }
    Some(rest.trim().to_string())
}

/// One `SHOW CREATE {PROCEDURE|FUNCTION}` row: `(name, sql_mode, Create …,
/// character_set_client, collation_connection, Database Collation)`.
///
/// The `Create` column is **nullable**, and that is not a corner case: MySQL
/// returns NULL there for a routine the connected account may not see the
/// definition of (it needs `SHOW_ROUTINE`, or to be the definer). A `None`
/// leaves the editor on what the schema fetch already carried.
type MyShowCreateRoutineRow = (String, String, Option<String>, String, String, String);

/// One `SHOW CREATE EVENT` row: `(name, sql_mode, time_zone, Create Event,
/// character_set_client, collation_connection, Database Collation)`.
///
/// Seven columns rather than a routine's six, and the extra one is `time_zone` —
/// which is why an event needs its own row type rather than reusing that alias.
/// The `Create Event` column is **nullable** for the same reason a routine's is.
type MyShowCreateEventRow = (
    String,
    String,
    String,
    Option<String>,
    String,
    String,
    String,
);

/// The **body** of a `SHOW CREATE {PROCEDURE|FUNCTION}` statement — everything
/// after the parameter list and the characteristics that follow it.
///
/// The same shape as [`trigger_body_of`] and for the same reason, but it cannot
/// anchor on a keyword: a routine has no `FOR EACH ROW`, and what separates the
/// header from the body is *running out of characteristics*. So the parameter
/// list is skipped as a balanced group (through [`sql::balanced_paren_span`], so
/// a default or a type inside it can hold a paren in a string), and then the
/// clauses MySQL prints between it and the body are consumed by keyword.
///
/// **Greedy consumption is safe because the two vocabularies are disjoint.** The
/// characteristic words are `COMMENT`, `LANGUAGE`, `NOT`, `DETERMINISTIC`,
/// `CONTAINS`, `NO`, `READS`, `MODIFIES`, `SQL`, `DATA`, `SECURITY`, `DEFINER`,
/// `INVOKER` and `RETURNS`; no MySQL statement — and therefore no routine body —
/// begins with any of them. The first word that isn't one of them starts the
/// body, which is returned untouched.
///
/// `RETURNS` is the one clause with an argument that isn't a single token: the
/// type may carry a length (`VARCHAR(10)`) and trailing modifiers, so the word
/// after it takes an optional balanced group and then any of the type-modifier
/// words with it.
///
/// `None` when there is no parameter list to anchor on, or nothing after the
/// characteristics — both of which mean this didn't understand the text, and a
/// caller that gets `None` keeps the body it already had rather than blanking it.
/// Does this `SHOW CREATE` text declare a MariaDB **aggregate** function?
///
/// The one fact about a routine that `information_schema.ROUTINES` does not
/// publish — verified live on MariaDB 10.11.14, no column in that table names
/// it — while `SHOW CREATE FUNCTION` prints
/// ``CREATE DEFINER=`a`@`b` AGGREGATE FUNCTION `f`(…)``. Losing it destroyed the
/// function: the recreate's `CREATE` came back `ERROR 4105 (Aggregate specific
/// instruction (FETCH GROUP NEXT ROW) used in a wrong context)` after the
/// `DROP` had committed, and the catalogue was then empty for the name.
///
/// Only the **header** is read — everything before the parameter list — so a
/// body that mentions aggregates says nothing, and the scan goes through
/// [`sql::skip_noncode`] so a routine *named* `` `aggregate` `` is a quoted
/// identifier the scan steps over rather than the keyword. This is the same
/// span [`routine_body_of`] walks to find the parameter list and discards.
fn routine_is_aggregate(create_sql: &str) -> bool {
    let b = create_sql.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(j) = sql::skip_noncode(b, i, SqlDialect::MySql) {
            i = j.max(i + 1);
            continue;
        }
        // The parameter list: past here is the routine, not its header.
        if b[i] == b'(' {
            return false;
        }
        if sql::is_word_start(b[i]) {
            let mut j = i + 1;
            while j < b.len() && sql::is_word_byte(b[j]) {
                j += 1;
            }
            if create_sql[i..j].eq_ignore_ascii_case("AGGREGATE") {
                return true;
            }
            i = j;
            continue;
        }
        i += 1;
    }
    false
}

fn routine_body_of(create_sql: &str) -> Option<String> {
    const CHARACTERISTIC: &[&str] = &[
        "NOT",
        "DETERMINISTIC",
        "CONTAINS",
        "NO",
        "READS",
        "MODIFIES",
        "SQL",
        "DATA",
        "SECURITY",
        "DEFINER",
        "INVOKER",
    ];
    // Words that may trail a return type on their own:
    // `RETURNS DECIMAL(10,2) UNSIGNED`. Each takes no argument.
    const TYPE_FLAG: &[&str] = &[
        "UNSIGNED", "SIGNED", "ZEROFILL", "BINARY", "ASCII", "UNICODE",
    ];
    // …and the two that take a **name** with them: `CHARSET utf8mb4`,
    // `COLLATE utf8mb4_bin`. `CHARACTER SET utf8mb4` is the third and is spelled
    // in two words, which is why it is matched as a pair below rather than by
    // putting a bare `SET` on either list — a bare `SET` there also swallowed
    // the first word of a body that legitimately begins `SET @x = 1`.
    const TYPE_NAMED: &[&str] = &["CHARSET", "COLLATE"];

    let b = create_sql.as_bytes();
    // The parameter list: the first parenthesis that isn't inside a quoted
    // identifier, a string or a comment. `CREATE DEFINER=`a`@`b` PROCEDURE
    // `db`.`p`(…)` has none before it, and a routine named `` `p(x)` `` would.
    let mut i = 0usize;
    let after_params = loop {
        if i >= b.len() {
            return None;
        }
        if let Some(j) = sql::skip_noncode(b, i, SqlDialect::MySql) {
            i = j;
            continue;
        }
        if b[i] == b'(' {
            break sql::balanced_paren_span(b, i, SqlDialect::MySql)? + 1;
        }
        i += 1;
    };

    let mut rest = create_sql.get(after_params..)?.trim_start();
    loop {
        let word = leading_word(rest);
        let upper = word.to_ascii_uppercase();
        if word.is_empty() {
            break;
        }
        if upper == "COMMENT" {
            // The literal that follows, skipped as a quoted run so an escaped
            // or doubled quote inside it can't end it early.
            let after_kw = rest[word.len()..].trim_start();
            let nb = after_kw.as_bytes();
            let end = sql::skip_noncode(nb, 0, SqlDialect::MySql)?;
            rest = after_kw[end..].trim_start();
            continue;
        }
        if upper == "LANGUAGE" {
            let after_kw = rest[word.len()..].trim_start();
            let lang = leading_word(after_kw);
            rest = after_kw[lang.len()..].trim_start();
            continue;
        }
        if upper == "RETURNS" {
            rest = rest[word.len()..].trim_start();
            // The type name, then its optional length/precision group.
            let name = leading_word(rest);
            rest = rest[name.len()..].trim_start();
            if rest.as_bytes().first() == Some(&b'(') {
                let end = sql::balanced_paren_span(rest.as_bytes(), 0, SqlDialect::MySql)? + 1;
                rest = rest[end..].trim_start();
            }
            // The type's trailing modifiers. **Each form takes its argument with
            // it or takes none — a keyword consumed without its value leaves
            // that value at the head of what is returned as the body**, which is
            // a `CREATE` that fails 1064 *after* the `DROP` has committed.
            loop {
                let w = leading_word(rest);
                if w.is_empty() {
                    break;
                }
                let after = || rest[w.len()..].trim_start();
                if TYPE_FLAG.iter().any(|t| w.eq_ignore_ascii_case(t)) {
                    rest = after();
                } else if TYPE_NAMED.iter().any(|t| w.eq_ignore_ascii_case(t)) {
                    let tail = after();
                    let v = leading_word(tail);
                    rest = tail[v.len()..].trim_start();
                } else if w.eq_ignore_ascii_case("CHARACTER") {
                    // `CHARACTER SET <name>` — three words, and only as a pair:
                    // a lone `CHARACTER` isn't a modifier, so an unmatched one
                    // ends the type rather than eating what follows.
                    let tail = after();
                    let set = leading_word(tail);
                    if !set.eq_ignore_ascii_case("SET") {
                        break;
                    }
                    let tail = tail[set.len()..].trim_start();
                    let v = leading_word(tail);
                    rest = tail[v.len()..].trim_start();
                } else {
                    break;
                }
            }
            continue;
        }
        if CHARACTERISTIC.iter().any(|c| *c == upper) {
            rest = rest[word.len()..].trim_start();
            continue;
        }
        break;
    }
    let body = rest.trim();
    (!body.is_empty()).then(|| body.to_string())
}

/// The identifier-shaped word `s` starts with, or `""` when it doesn't start
/// with one. On [`sql::is_word_start`]/[`sql::is_word_byte`], which is the one
/// definition of what a word is here.
fn leading_word(s: &str) -> &str {
    let b = s.as_bytes();
    if b.first().is_none_or(|c| !sql::is_word_start(*c)) {
        return "";
    }
    let end = b
        .iter()
        .position(|c| !sql::is_word_byte(*c))
        .unwrap_or(b.len());
    &s[..end]
}

/// One `information_schema.CHECK_CONSTRAINTS` row, already joined to its table:
/// `(table, constraint name, check clause, enforced, level)`.
///
/// `level` is MariaDB's `Column`/`Table`; the MySQL query reports `Table` for
/// every row, which is what that server actually stores.
type MyCheckRow = (String, String, String, String, String);

/// One `CHECK_CLAUSE` as SQL that can actually be run.
///
/// The two servers disagree, and only one of them says so. **MySQL 8 returns the
/// clause with an extra level of backslash escaping** — a predicate that reads
/// `_latin1'new'` comes back as `_latin1\'new\'`, and `'C:\\temp'` as
/// `'C:\\\\temp'` — so restating it verbatim in a `CREATE TABLE` is a syntax
/// error, not a subtly different constraint. **MariaDB returns it already
/// runnable**, byte for byte what `SHOW CREATE TABLE` prints, so unescaping there
/// would eat the backslash out of `'it\'s'` and change what the predicate means.
///
/// The rule is one level of unescaping: a backslash escapes the character after
/// it, which is emitted alone. Measured against `SHOW CREATE TABLE` on MySQL
/// 8.4 — the same authority [`mysql_column`] uses for defaults, and the same
/// class of bug, except this one fails loudly instead of writing something else.
fn mysql_check_clause(clause: &str, mariadb: bool) -> String {
    if mariadb || !clause.contains('\\') {
        return clause.to_string();
    }
    let b = clause.as_bytes();
    let mut out = String::with_capacity(clause.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() {
            // **Decode the escape, don't just drop the backslash.** MySQL's
            // escapes are not all "the next byte literally": `\n` is a newline,
            // not the letter `n`. Dropping the backslash turned a column named
            // with an embedded newline into a different, non-existent
            // identifier — measured live on MySQL 8.4.11, `` `nl\ncol` ``
            // came back as `` `nlncol` ``.
            let decoded = match b[i + 1] {
                b'n' => Some('\n'),
                b'r' => Some('\r'),
                b't' => Some('\t'),
                b'0' => Some('\0'),
                b'b' => Some('\u{8}'),
                b'Z' => Some('\u{1a}'),
                // Everything else — `\'`, `\"`, `\\`, `\%`, `\_` — really is
                // the next character standing for itself.
                _ => None,
            };
            if let Some(c) = decoded {
                out.push(c);
                i += 2;
                continue;
            }
            i += 1;
        }
        // Copy one whole UTF-8 char: the escaped byte may begin a multi-byte one.
        let start = i;
        i += 1;
        while i < b.len() && (b[i] & 0xC0) == 0x80 {
            i += 1;
        }
        out.push_str(&clause[start..i]);
    }
    out
}

/// Fold MySQL/MariaDB's check constraints onto the assembled tables.
///
/// Kept out of [`assemble_schema`] for the reason [`apply_fk_rules`] is: it's a
/// second query's worth of rows keyed by table, not part of the row shape the
/// two engines share.
fn apply_check_constraints(schema: &mut DbSchema, rows: &[MyCheckRow], mariadb: bool) {
    for t in schema.tables.iter_mut() {
        t.check_constraints = rows
            .iter()
            .filter(|(table, ..)| *table == t.name)
            .map(|(_, name, clause, enforced, level)| CheckInfo {
                name: name.clone(),
                // `CHECK_CLAUSE` is the server's re-print of the predicate,
                // parenthesised and — on MySQL 8 — escaped a second time; the
                // model stores it bare and runnable. Unescaping has to come
                // first: `check_predicate`'s paren scan reads string boundaries,
                // and `\'new\'` isn't one until the escaping is gone.
                expression: schemaic_core::ddl::check_predicate(
                    &mysql_check_clause(clause, mariadb),
                    schemaic_core::intel::SqlDialect::MySql,
                ),
                // MariaDB has no `NOT ENFORCED` and the query hardcodes `YES`
                // there, so this reads as enforced on both.
                enforced: !enforced.eq_ignore_ascii_case("NO"),
                // MariaDB's `LEVEL`. Only `Column` matters — it says the
                // constraint lives inside the column definition, so a `MODIFY`
                // that doesn't restate it deletes it.
                column_level: level.eq_ignore_ascii_case("Column"),
                // `NOT VALID` / `NO INHERIT` are PostgreSQL's; neither engine
                // here can report one, and the emitter writes neither.
                ..Default::default()
            })
            .collect();
    }
}

/// One `information_schema.TRIGGERS` row: `(table, name, timing, event,
/// statement, definer, action order)`.
type MyTriggerRow = (String, String, String, String, String, String, u64);

/// Fold MySQL's trigger rows into [`TriggerInfo`]s, per table.
///
/// **The ordering has to be reconstructed, not read.** MySQL has no
/// `FOLLOWS`/`PRECEDES` column: `ACTION_ORDER` reports a trigger's *position*
/// within its `(table, timing, event)` group, and that is all. A recreate that
/// ignored it would silently reorder triggers that write the same row, which is
/// the whole reason anyone sets an order in the first place — so position 2 and
/// up become `FOLLOWS <the row before them>`, the same chain MySQL was given.
///
/// A server too old to report the column sends `0` for every row. `0` means "no
/// ordering information", not "first", so those get no clause at all rather than
/// a fabricated chain.
///
/// **The group's leader gets a `PRECEDES`, not nothing.** `FOLLOWS <previous>`
/// covers positions 2 and up, but position 1 has no predecessor to name — and a
/// `CREATE TRIGGER` with no ordering clause makes MySQL append the trigger
/// *last*, so replacing the leader reversed the whole group. Measured on MySQL
/// 8.4.11. A positive anchor is the only clause that can express "first", so the
/// leader of a group of two or more names its successor instead. A group of one
/// still gets nothing: there is no order to preserve.
fn mysql_triggers(rows: &[MyTriggerRow]) -> Vec<TriggerInfo> {
    // Group key then position, so the previous row in iteration order *is* the
    // trigger this one follows. Name last, to keep it deterministic when a stale
    // server reports ties.
    let mut sorted: Vec<&MyTriggerRow> = rows.iter().collect();
    sorted.sort_by(|a, b| (&a.0, &a.2, &a.3, a.6, &a.1).cmp(&(&b.0, &b.2, &b.3, b.6, &b.1)));
    let mut out: Vec<TriggerInfo> = Vec::with_capacity(sorted.len());
    // (table, timing, event, the name of the last trigger emitted in that group)
    let mut prev: Option<(String, String, String, String)> = None;
    for (i, (table, name, timing, event, stmt, definer, order)) in sorted.iter().enumerate() {
        let same_group = prev
            .as_ref()
            .is_some_and(|(t, ti, e, _)| t == table && ti == timing && e == event);
        let order_clause = match (&prev, same_group, *order > 1) {
            (Some((.., last)), true, true) => Some(TriggerOrder::Follows(last.clone())),
            // The leader of a group of two or more. `*order == 1` excludes the
            // `0` "no information" case, and the successor's name is already in
            // hand — it is the next row, which is in the same group by the sort.
            _ if *order == 1 => sorted
                .get(i + 1)
                .filter(|(t, _, ti, e, ..)| t == table && ti == timing && e == event)
                .map(|(_, next, ..)| TriggerOrder::Precedes(next.clone())),
            _ => None,
        };
        out.push(TriggerInfo {
            name: name.clone(),
            // MySQL has no namespace level between database and table.
            schema: None,
            table: table.clone(),
            // An unreadable timing/event would be a server that grew a new one;
            // fall back to the model's default rather than drop the trigger, so
            // it still shows up and can still be dropped.
            timing: TriggerTiming::parse(timing).unwrap_or_default(),
            events: TriggerEvent::parse(event).into_iter().collect(),
            update_columns: Vec::new(),
            // `ACTION_ORIENTATION` is always ROW on MySQL; there is no other.
            level: schemaic_core::schema::TriggerLevel::Row,
            condition: None,
            action: TriggerAction::Body(stmt.clone()),
            definer: Some(definer.clone()).filter(|d| !d.is_empty()),
            order: order_clause,
            // `information_schema` reports none of the three, and on MySQL 8 the
            // body it *does* report is already unescaped. Both come from
            // `Db::trigger_source`, lazily, when the editor opens.
            sql_mode: None,
            charset_client: None,
            collation_connection: None,
            // All three are PostgreSQL's alone: MySQL has no transition tables
            // and no per-trigger firing mode.
            old_table: None,
            new_table: None,
            enabled: schemaic_core::schema::TriggerEnabled::Origin,
            constraint: false,
        });
        prev = Some((table.clone(), timing.clone(), event.clone(), name.clone()));
    }
    out
}

/// Hang each trigger off the table it fires on, dropping any whose table wasn't
/// in this fetch — the same rule [`assemble_schema`] applies to column rows.
fn apply_triggers(schema: &mut DbSchema, triggers: Vec<TriggerInfo>) {
    for t in schema.tables.iter_mut() {
        t.triggers = triggers
            .iter()
            .filter(|g| g.table == t.name)
            .cloned()
            .collect();
    }
}

/// One [`MY_ROUTINES_SQL`] row.
///
/// A struct rather than the tuple its siblings here are, for two reasons: the
/// query selects fourteen columns, past `mysql_common`'s twelve-element
/// `FromRow` ceiling, and past the point where a positional `.6` in a test says
/// anything about which column it means.
///
/// The body is **nullable** — `ROUTINE_DEFINITION` is NULL for a routine the
/// connected account can't see the definition of — and is the one column here
/// that must not be trusted for an edit; see [`Db::routine_source`].
#[derive(Clone, Debug, Default)]
struct MyRoutineRow {
    name: String,
    /// `ROUTINE_TYPE` — `FUNCTION` or `PROCEDURE`, as the server spells it.
    kind: String,
    /// `DTD_IDENTIFIER`: a function's return type, empty for a procedure.
    returns: String,
    /// The return type's declared character set and collation, which
    /// `DTD_IDENTIFIER` does **not** carry. NULL for anything but a string type.
    returns_charset: Option<String>,
    returns_collation: Option<String>,
    body: Option<String>,
    deterministic: String,
    data_access: String,
    security: String,
    definer: String,
    comment: String,
    /// The session state the routine was created under. See
    /// [`schemaic_core::schema::RoutineSource`] for why a recreate has to
    /// restore it.
    sql_mode: Option<String>,
    charset_client: Option<String>,
    collation_connection: Option<String>,
}

/// The aliases [`MY_ROUTINES_SQL`] gives its columns, in the order it selects
/// them.
///
/// **A third statement of the list, deliberately.** The two that matter are the
/// query and [`my_routine_row_from`]'s reads, and a test that checked one
/// against the other would only be checking whether they were written by the
/// same hand. This is the oracle both are compared against, which is why it is
/// test-only and why it is written out rather than derived from either.
#[cfg(test)]
const MY_ROUTINE_COLUMNS: [&str; 14] = [
    "n", "ty", "rt", "rtcs", "rtcoll", "body", "det", "acc", "sec", "df", "cmt", "sqlmode", "cscl",
    "collconn",
];

/// The routine catalogue read, aliased column by column.
///
/// A `const` rather than a literal at the call so a test can read it: the
/// aliases are what [`my_routine_row`] binds to, and nothing else holds the
/// two in step.
const MY_ROUTINES_SQL: &str = "SELECT CAST(ROUTINE_NAME AS CHAR) AS n, \
     CAST(ROUTINE_TYPE AS CHAR) AS ty, \
     CAST(COALESCE(DTD_IDENTIFIER, '') AS CHAR) AS rt, \
     CAST(CHARACTER_SET_NAME AS CHAR) AS rtcs, \
     CAST(COLLATION_NAME AS CHAR) AS rtcoll, \
     CAST(ROUTINE_DEFINITION AS CHAR) AS body, \
     CAST(IS_DETERMINISTIC AS CHAR) AS det, \
     CAST(SQL_DATA_ACCESS AS CHAR) AS acc, \
     CAST(SECURITY_TYPE AS CHAR) AS sec, \
     CAST(DEFINER AS CHAR) AS df, \
     CAST(COALESCE(ROUTINE_COMMENT, '') AS CHAR) AS cmt, \
     CAST(SQL_MODE AS CHAR) AS sqlmode, \
     CAST(CHARACTER_SET_CLIENT AS CHAR) AS cscl, \
     CAST(COLLATION_CONNECTION AS CHAR) AS collconn \
     FROM information_schema.ROUTINES \
     WHERE ROUTINE_SCHEMA = ? ORDER BY ROUTINE_TYPE, ROUTINE_NAME";

/// Read one [`MY_ROUTINES_SQL`] row, by the aliases it gives its columns.
fn my_routine_row(r: &Row) -> MyRoutineRow {
    my_routine_row_from(|c| r.get::<Option<String>, &str>(c).flatten())
}

/// The name→field half of [`my_routine_row`], over any reader.
///
/// **By alias, not by position.** The struct replaced a tuple precisely because
/// fourteen columns is past `mysql_common`'s twelve-element `FromRow` ceiling —
/// which is the same thing as saying the compiler stopped checking the arity.
/// The reader that replaced it indexed the row `0..=13` against a `SELECT`
/// fifteen hundred lines away, with nothing but a doc comment holding the two
/// in step: insert a column at position 3 and `body` starts reading
/// `CHARACTER_SET_NAME`, `sql_mode` starts reading `ROUTINE_COMMENT`, the suite
/// stays green, and what ships is a routine whose Body field shows `utf8mb3`
/// and a recreate that `DROP`s the routine and re-`CREATE`s it from that — on
/// the engine whose `DROP` commits on its own.
///
/// Split from the `Row` so a test can supply the reader; `mysql_common`'s row
/// constructor isn't re-exported by `mysql_async`, and the decision here is the
/// mapping, not the driver.
///
/// Every column is `CAST(… AS CHAR)`, so a value that fails to convert is a
/// server this app can't read at all; it degrades to the empty string (or
/// `None`) here for the same reason the neighbouring queries `COALESCE` — a
/// missing characteristic must not cost the whole schema.
fn my_routine_row_from(mut opt: impl FnMut(&str) -> Option<String>) -> MyRoutineRow {
    MyRoutineRow {
        name: opt("n").unwrap_or_default(),
        kind: opt("ty").unwrap_or_default(),
        returns: opt("rt").unwrap_or_default(),
        returns_charset: opt("rtcs"),
        returns_collation: opt("rtcoll"),
        body: opt("body"),
        deterministic: opt("det").unwrap_or_default(),
        data_access: opt("acc").unwrap_or_default(),
        security: opt("sec").unwrap_or_default(),
        definer: opt("df").unwrap_or_default(),
        comment: opt("cmt").unwrap_or_default(),
        sql_mode: opt("sqlmode"),
        charset_client: opt("cscl"),
        collation_connection: opt("collconn"),
    }
}

/// One `information_schema.PARAMETERS` row: `(specific name, routine type, mode,
/// parameter name, DTD_IDENTIFIER, character set, collation)`.
type MyParamRow = (
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
);

/// Restate a declared character set and collation onto a type the catalogue
/// publishes without them.
///
/// `DTD_IDENTIFIER` renders `longtext`, never `longtext CHARACTER SET utf8mb3`,
/// and the two clauses live in their own columns. A recreate that emits only the
/// type re-declares the parameter under the *database's* default character set —
/// a silent change to what the routine accepts, with nothing on screen to say
/// so. MySQL reports both columns only for string types, so a non-NULL value is
/// exactly the case that needs restating.
fn mysql_type_with_charset(dtd: &str, charset: Option<&str>, collation: Option<&str>) -> String {
    let mut out = dtd.trim().to_string();
    if out.is_empty() {
        return out;
    }
    if let Some(cs) = charset.map(str::trim).filter(|s| !s.is_empty()) {
        out.push_str(" CHARACTER SET ");
        out.push_str(cs);
    }
    if let Some(coll) = collation.map(str::trim).filter(|s| !s.is_empty()) {
        out.push_str(" COLLATE ");
        out.push_str(coll);
    }
    out
}

/// Fold `information_schema.PARAMETERS` into the rendered parameter list each
/// routine's emitter wants, keyed by name **and kind** because a function and a
/// procedure may share a name.
///
/// **The mode is rendered only for a procedure.** The catalogue reports
/// `PARAMETER_MODE = 'IN'` for a function's parameters too, but `CREATE
/// FUNCTION`'s grammar is `param_name type` — the mode keywords belong to
/// `proc_parameter` alone, and both vendors' manuals say specifying one "is
/// valid only for a PROCEDURE". Joining the mode in for a function emitted a
/// `CREATE` the server answers 1064 to, *after* the recreate's `DROP` had
/// committed on its own: the function was destroyed and nothing replaced it.
///
/// **And the name is quoted, which is the same failure by the other half of the
/// same line.** `PARAMETER_NAME` is the *bare* name — a procedure declared
/// ``p(`order` INT)`` reports `order` — so joining it raw emitted
/// `CREATE PROCEDURE p(IN order INT)` and cost the routine in exactly the way
/// the paragraph above describes. Reproduced live on MariaDB 10.11.14 and MySQL
/// 8.4.11. Through [`export::ident_if_needed`], the project's one quoter for
/// SQL a user also reads — this string is what the editor's Parameters field
/// shows — so an ordinary lower-case name stays bare and no rendered list a
/// user is already looking at changes.
fn mysql_parameters(rows: &[MyParamRow]) -> HashMap<(String, String), Vec<String>> {
    let mut params: HashMap<(String, String), Vec<String>> = HashMap::new();
    for (name, ty, mode, pname, dtd, charset, collation) in rows {
        let kind = ty.to_ascii_uppercase();
        let mode = if kind == "PROCEDURE" { mode.trim() } else { "" };
        let dtd = mysql_type_with_charset(dtd, charset.as_deref(), collation.as_deref());
        let pname = export::ident_if_needed(pname.trim(), SqlDialect::MySql);
        let rendered = [mode, pname.as_str(), dtd.as_str()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        params
            .entry((name.clone(), kind))
            .or_default()
            .push(rendered);
    }
    params
}

/// Fold MySQL's `information_schema.ROUTINES` rows into [`RoutineInfo`]s.
///
/// `arguments` arrives separately, from `information_schema.PARAMETERS`: that
/// table has one row per parameter and MySQL has no rendered-signature column at
/// all, so the `IN a INT, OUT b TEXT` form the emitter and the tree both want is
/// rebuilt by [`mysql_parameters`].
///
/// A function's own return value is `PARAMETERS` ordinal **0**, which is why the
/// caller's query excludes it: folded in, every function's parameter list would
/// open with its return type.
fn mysql_routines(
    rows: &[MyRoutineRow],
    params: &HashMap<(String, String), Vec<String>>,
) -> Vec<RoutineInfo> {
    rows.iter()
        .map(|r| {
            let kind = schemaic_core::schema::RoutineKind::parse(&r.kind);
            let some = |s: &String| Some(s.clone()).filter(|s| !s.is_empty());
            RoutineInfo {
                name: r.name.clone(),
                // MySQL has no namespace level: the database *is* the
                // namespace, exactly as it is for a table.
                schema: None,
                kind,
                arguments: params
                    .get(&(r.name.clone(), r.kind.to_ascii_uppercase()))
                    .map(|p| p.join(", "))
                    .unwrap_or_default(),
                // A procedure's `DTD_IDENTIFIER` is NULL and arrives as the
                // empty string, which is what the model wants there.
                returns: mysql_type_with_charset(
                    &r.returns,
                    r.returns_charset.as_deref(),
                    r.returns_collation.as_deref(),
                ),
                // Everything MySQL stores is `SQL`; the column reports it and
                // the emitter never writes a `LANGUAGE` clause for it.
                language: "SQL".to_string(),
                body: r.body.clone().unwrap_or_default(),
                deterministic: r.deterministic.eq_ignore_ascii_case("YES"),
                data_access: schemaic_core::schema::SqlDataAccess::parse(&r.data_access),
                // **DEFINER is this engine's default**, the opposite of
                // PostgreSQL's — so an unreadable value must not fall to
                // `false` and quietly re-declare the routine as INVOKER.
                security_definer: !r.security.eq_ignore_ascii_case("INVOKER"),
                definer: some(&r.definer),
                comment: some(&r.comment),
                // The catalogue carries the same session state `SHOW CREATE`
                // prints, so a draft has it from the first frame and the lazy
                // read only ever corrects the *body*.
                sql_mode: r.sql_mode.clone(),
                charset_client: r.charset_client.clone(),
                collation_connection: r.collation_connection.clone(),
                // PostgreSQL's.
                ..Default::default()
            }
        })
        .collect()
}

/// One [`MY_EVENTS_SQL`] row.
///
/// A struct rather than a tuple, on the same two grounds [`MyRoutineRow`] is
/// one: sixteen columns is past `mysql_common`'s twelve-element `FromRow`
/// ceiling, and a positional `.9` would say nothing about which column it means.
///
/// The body is **nullable** for the same reason a routine's is, and carries the
/// same warning: `EVENT_DEFINITION` has had its escapes resolved, so it is what
/// the tree *reads* and never what an edit is emitted from. See
/// [`Db::event_source`].
#[derive(Clone, Debug, Default)]
struct MyEventRow {
    name: String,
    definer: String,
    /// `EVENT_TYPE` — `ONE TIME` or `RECURRING`, as the server spells it. The
    /// tag that decides which [`EventSchedule`] arm the other five columns are
    /// read into; the alternative, "is `EXECUTE_AT` NULL", is the same question
    /// asked of a column that is also NULL for a recurring event whose
    /// `INTERVAL_VALUE` failed to convert.
    kind: String,
    execute_at: Option<String>,
    interval_value: Option<String>,
    interval_field: Option<String>,
    starts: Option<String>,
    ends: Option<String>,
    status: String,
    on_completion: String,
    comment: String,
    body: Option<String>,
    /// The session state the event was created under, plus the time zone its
    /// schedule is read in. See [`schemaic_core::schema::EventInfo`] for why the
    /// fourth one is not optional decoration.
    time_zone: Option<String>,
    sql_mode: Option<String>,
    charset_client: Option<String>,
    collation_connection: Option<String>,
}

/// The aliases [`MY_EVENTS_SQL`] gives its columns, in the order it selects
/// them. A third statement of the list, for the reason
/// [`MY_ROUTINE_COLUMNS`] is one.
#[cfg(test)]
const MY_EVENT_COLUMNS: [&str; 16] = [
    "n", "df", "ty", "at", "iv", "if_", "st", "en", "stat", "oc", "cmt", "body", "tz", "sqlmode",
    "cscl", "collconn",
];

/// The scheduled-event catalogue read, aliased column by column.
const MY_EVENTS_SQL: &str = "SELECT CAST(EVENT_NAME AS CHAR) AS n, \
     CAST(DEFINER AS CHAR) AS df, \
     CAST(EVENT_TYPE AS CHAR) AS ty, \
     CAST(EXECUTE_AT AS CHAR) AS at, \
     CAST(INTERVAL_VALUE AS CHAR) AS iv, \
     CAST(INTERVAL_FIELD AS CHAR) AS if_, \
     CAST(STARTS AS CHAR) AS st, \
     CAST(ENDS AS CHAR) AS en, \
     CAST(STATUS AS CHAR) AS stat, \
     CAST(ON_COMPLETION AS CHAR) AS oc, \
     CAST(COALESCE(EVENT_COMMENT, '') AS CHAR) AS cmt, \
     CAST(EVENT_DEFINITION AS CHAR) AS body, \
     CAST(TIME_ZONE AS CHAR) AS tz, \
     CAST(SQL_MODE AS CHAR) AS sqlmode, \
     CAST(CHARACTER_SET_CLIENT AS CHAR) AS cscl, \
     CAST(COLLATION_CONNECTION AS CHAR) AS collconn \
     FROM information_schema.EVENTS \
     WHERE EVENT_SCHEMA = ? ORDER BY EVENT_NAME";

/// Read one [`MY_EVENTS_SQL`] row, by the aliases it gives its columns.
fn my_event_row(r: &Row) -> MyEventRow {
    my_event_row_from(|c| r.get::<Option<String>, &str>(c).flatten())
}

/// The name→field half of [`my_event_row`], over any reader. **By alias, not by
/// position**, for the reason [`my_routine_row_from`] spells out at length.
fn my_event_row_from(mut opt: impl FnMut(&str) -> Option<String>) -> MyEventRow {
    MyEventRow {
        name: opt("n").unwrap_or_default(),
        definer: opt("df").unwrap_or_default(),
        kind: opt("ty").unwrap_or_default(),
        execute_at: opt("at"),
        interval_value: opt("iv"),
        interval_field: opt("if_"),
        starts: opt("st"),
        ends: opt("en"),
        status: opt("stat").unwrap_or_default(),
        on_completion: opt("oc").unwrap_or_default(),
        comment: opt("cmt").unwrap_or_default(),
        body: opt("body"),
        time_zone: opt("tz"),
        sql_mode: opt("sqlmode"),
        charset_client: opt("cscl"),
        collation_connection: opt("collconn"),
    }
}

/// Fold `information_schema.EVENTS` rows into [`EventInfo`]s.
///
/// The one decision here is the schedule. `EVENT_TYPE` says which of the two
/// shapes the row is carrying, and the timestamps and the interval quantity are
/// quoted into SQL expressions on the way in — see [`event_time_expr`] for why
/// the model holds expressions rather than values.
///
/// A `RECURRING` row with no readable interval is read as `EVERY 1 DAY` rather
/// than dropped: an event Schemaic can't fully describe is still one the user
/// must be able to see, rename, disable and drop, and the schedule is the field
/// the editor shows them before anything is applied.
fn mysql_events(rows: &[MyEventRow]) -> Vec<EventInfo> {
    let d = SqlDialect::MySql;
    let some = |s: &str| Some(s.trim().to_string()).filter(|s| !s.is_empty());
    rows.iter()
        .map(|r| {
            let one_shot = r.kind.trim().eq_ignore_ascii_case("ONE TIME");
            let schedule = if one_shot {
                EventSchedule::At(
                    r.execute_at
                        .as_deref()
                        .and_then(|s| event_time_expr(s, d))
                        // **Both arms fall back, and to something legal.** An
                        // empty `AT` is what `EventDraft::validate` refuses, so
                        // it wouldn't have been an event with an unknown time —
                        // it would have been an event that cannot be renamed,
                        // disabled or commented, because Preview stays disabled
                        // while a draft is invalid.
                        //
                        // The fabricated value cannot reach the server on its
                        // own: `event_alter_clauses` restates `ON SCHEDULE` only
                        // when it *changed*, and it hasn't until the user edits
                        // it — at which point they are looking at the field.
                        // That is the same property that makes `EVERY 1 DAY`
                        // below safe.
                        .unwrap_or_else(|| "CURRENT_TIMESTAMP".to_string()),
                )
            } else {
                EventSchedule::Every {
                    value: r
                        .interval_value
                        .as_deref()
                        .map(|s| event_interval_expr(s, d))
                        .unwrap_or_else(|| "1".to_string()),
                    unit: r
                        .interval_field
                        .as_deref()
                        .map(|s| s.trim().to_ascii_uppercase())
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| "DAY".to_string()),
                    starts: r.starts.as_deref().and_then(|s| event_time_expr(s, d)),
                    ends: r.ends.as_deref().and_then(|s| event_time_expr(s, d)),
                }
            };
            EventInfo {
                name: r.name.clone(),
                // MySQL is the only engine with events, and a database is the
                // namespace there.
                schema: None,
                definer: some(&r.definer),
                schedule,
                // `ON_COMPLETION` reads `PRESERVE` or `NOT PRESERVE`; anything
                // else is read as the server's default, which is not to keep it.
                preserve: r.on_completion.trim().eq_ignore_ascii_case("PRESERVE"),
                status: EventStatus::parse(&r.status),
                comment: some(&r.comment),
                // `information_schema`'s copy — good enough to read and to copy
                // as DDL, and corrected by `Db::event_source` before an edit.
                body: r.body.clone().unwrap_or_default(),
                time_zone: r.time_zone.clone(),
                sql_mode: r.sql_mode.clone(),
                charset_client: r.charset_client.clone(),
                collation_connection: r.collation_connection.clone(),
            }
        })
        .collect()
}

/// The body of a `SHOW CREATE EVENT` statement — everything after its top-level
/// `DO`.
///
/// Simpler than [`routine_body_of`] because the keyword that opens the body is a
/// keyword: there is no parameter list to walk past and no characteristic list
/// to step over. What it still needs is [`sql::skip_noncode`], for the two ways
/// a bare byte scan gets this wrong — an event named `` `do` `` (a quoted
/// identifier) and a `COMMENT 'run this, do not touch'` (a string literal), both
/// of which sit before the real `DO`.
///
/// `None` when there is no top-level `DO` at all, which is a statement this
/// build doesn't understand; the caller keeps the body it already had rather
/// than blanking it.
fn event_body_of(create_sql: &str) -> Option<String> {
    let b = create_sql.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(j) = sql::skip_noncode(b, i, SqlDialect::MySql) {
            i = j.max(i + 1);
            continue;
        }
        if sql::is_word_start(b[i]) {
            let mut j = i + 1;
            while j < b.len() && sql::is_word_byte(b[j]) {
                j += 1;
            }
            if create_sql[i..j].eq_ignore_ascii_case("DO") {
                return Some(create_sql.get(j..)?.trim().to_string());
            }
            i = j;
            continue;
        }
        i += 1;
    }
    None
}

/// One `information_schema.VIEWS` row: `(name, definition, check option, definer,
/// security type, algorithm)`. The algorithm is `None` on MySQL, which doesn't
/// report it.
type MyViewRow = (String, String, String, String, String, Option<String>);

/// Fold MySQL's view options onto the assembled views. Kept out of
/// [`assemble_schema`] for the same reason [`apply_table_options`] is: half of
/// these have no PostgreSQL equivalent.
fn apply_view_options(schema: &mut DbSchema, rows: &[MyViewRow]) {
    let by_name: HashMap<&str, &MyViewRow> = rows.iter().map(|r| (r.0.as_str(), r)).collect();
    for t in schema.tables.iter_mut().filter(|t| t.is_view) {
        if let Some((_, _, check, definer, security, algorithm)) = by_name.get(t.name.as_str()) {
            t.view_options = Some(mysql_view_options(
                check,
                definer,
                security,
                algorithm.as_deref(),
            ));
        }
    }
}

/// A view's options as the catalogue reports them, in the form the emitter
/// wants: the values that *mean* "unset" (`NONE`, `UNDEFINED`, empty) become
/// `None`, so an untouched view round-trips to no change and nothing needless is
/// restated. Pure + tested — like [`mysql_column`], getting this wrong writes a
/// *different* view rather than failing.
pub(crate) fn mysql_view_options(
    check: &str,
    definer: &str,
    security: &str,
    algorithm: Option<&str>,
) -> ViewOptions {
    let set = |s: &str, unset: &str| {
        let s = s.trim();
        (!s.is_empty() && !s.eq_ignore_ascii_case(unset)).then(|| s.to_ascii_uppercase())
    };
    ViewOptions {
        check_option: set(check, "NONE"),
        // Not upper-cased: an account name is data, not a keyword.
        definer: Some(definer.trim().to_string()).filter(|d| !d.is_empty()),
        // Both values matter. `INVOKER` has to be restated or it reverts to the
        // default, and `DEFINER` is what that default *is* — restating it costs
        // nothing and keeps the emitted statement explicit.
        security: set(security, ""),
        algorithm: algorithm.and_then(|a| set(a, "UNDEFINED")),
        ..Default::default()
    }
}

/// One `information_schema.TABLES` row: `(name, type, engine, collation,
/// comment)`.
type MyTableRow = (String, String, Option<String>, Option<String>, String);

/// Fold MySQL's table-level options onto the assembled tables. Kept out of
/// [`assemble_schema`] (which both engines share) because PostgreSQL has no
/// equivalent of either the engine or the table collation.
fn apply_table_options(schema: &mut DbSchema, rows: &[MyTableRow]) {
    let by_name: HashMap<&str, &MyTableRow> = rows.iter().map(|r| (r.0.as_str(), r)).collect();
    for t in &mut schema.tables {
        let Some((.., engine, collation, comment)) = by_name.get(t.name.as_str()) else {
            continue;
        };
        t.engine = engine.clone().filter(|e| !e.is_empty());
        t.collation = collation.clone().filter(|c| !c.is_empty());
        t.comment = Some(comment.clone()).filter(|c| !c.is_empty());
    }
}

/// Attach each foreign key's referential actions. `NO ACTION` is the standard
/// default and both engines leave it unwritten, so it stays `None` — which is
/// exactly what makes an untouched key round-trip to no change at all.
fn apply_fk_rules(schema: &mut DbSchema, rows: &[(String, String, String, String)]) {
    let keep = |rule: &str| {
        let r = rule.trim();
        (!r.is_empty() && !r.eq_ignore_ascii_case("NO ACTION")).then(|| r.to_uppercase())
    };
    for (table, name, on_delete, on_update) in rows {
        let Some(t) = schema.tables.iter_mut().find(|t| t.name == *table) else {
            continue;
        };
        let Some(fk) = t.foreign_keys.iter_mut().find(|f| f.name == *name) else {
            continue;
        };
        fk.on_delete = keep(on_delete);
        fk.on_update = keep(on_update);
    }
}

/// A raw `information_schema.COLUMNS` row as the MySQL fetch selects it.
pub(crate) type MyColRow = (
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// Turn one MySQL/MariaDB catalogue row into a [`ColumnInfo`].
///
/// The whole reason this isn't a field-for-field copy is `COLUMN_DEFAULT`, where
/// the two servers genuinely disagree:
///
/// * **MariaDB** returns SQL text — a string default comes back *already quoted*
///   (`'draft'`), and an explicit `DEFAULT NULL` comes back as the four
///   characters `NULL`. It can be emitted verbatim.
/// * **MySQL** returns the *raw value* — `draft`, unquoted and indistinguishable
///   from the expression `draft`. Emitting it verbatim produces
///   `DEFAULT draft`, which is a syntax error at best and a column reference at
///   worst, so a non-expression default on a non-numeric column has to be quoted
///   here.
///
/// Expressions are told apart by `EXTRA`, which carries `DEFAULT_GENERATED` for
/// them on MySQL 8, plus the `CURRENT_TIMESTAMP` family which predates that flag.
/// Getting this wrong doesn't fail loudly — it writes a *different default* — so
/// it's normalized once, here, and the model downstream is plain SQL text.
pub(crate) fn mysql_column(r: MyColRow, mariadb: bool) -> ColRow {
    let (table, name, type_name, nullable, key, default, extra, collation, comment, generated) = r;
    let extra_lc = extra.to_ascii_lowercase();
    let numeric_or_bool = {
        let t = type_name.to_ascii_lowercase();
        [
            "int", "dec", "num", "float", "double", "real", "bit", "bool",
        ]
        .iter()
        .any(|p| t.starts_with(p))
    };
    let default = default.and_then(|d| {
        if mariadb {
            // Already SQL text. MariaDB writes a *missing* default as SQL NULL
            // and an explicit `DEFAULT NULL` as the literal text — both mean "no
            // default worth emitting" on a nullable column.
            (d != "NULL").then_some(d)
        } else if extra_lc.contains("default_generated")
            || numeric_or_bool
            || d.to_ascii_uppercase().starts_with("CURRENT_TIMESTAMP")
        {
            Some(d)
        } else {
            // This is the MySQL/MariaDB introspection path by construction, and
            // the quoting has to match what the emitter would write — otherwise
            // a backslash-bearing default is corrupted on the way *in* and
            // `TableDraft::from_table` produces a draft that differs from the
            // server without the designer showing any change.
            Some(schemaic_core::schema::ddl_string(
                &d,
                schemaic_core::intel::SqlDialect::MySql,
            ))
        }
    });
    ColRow {
        table,
        column: ColumnInfo {
            name,
            type_name,
            nullable: nullable.eq_ignore_ascii_case("YES"),
            primary_key: key == "PRI",
            default,
            auto_increment: extra_lc.contains("auto_increment"),
            // MySQL has no `GENERATED ALWAYS AS IDENTITY`: `AUTO_INCREMENT`
            // always accepts an explicit value.
            identity_always: false,
            // `GENERATION_EXPRESSION` is the empty string, not NULL, for an
            // ordinary column.
            generated: generated.filter(|g| !g.is_empty()),
            on_update: extra_lc
                .contains("on update current_timestamp")
                .then(|| "CURRENT_TIMESTAMP".to_string()),
            comment: comment.filter(|c| !c.is_empty()),
            collation,
            // MySQL reports `VIRTUAL GENERATED` / `STORED GENERATED` in `EXTRA`,
            // and its emitter restates neither — the flag is SQLite's, where the
            // rebuild has to write the word back or the column stops being
            // materialised.
            generated_stored: extra_lc.contains("stored generated"),
            // No such keyword on MySQL: `AUTO_INCREMENT` above is the whole
            // answer, and it already promises not to reuse a value.
            sqlite_autoincrement: false,
        },
    }
}

/// One introspected column, already turned into the model. A struct rather than
/// a widening tuple because a column now carries nine fields, and
/// `(String, String, String, String, String, Option<String>, bool, …)` at the
/// call site is unreadable and trivially mis-ordered.
///
/// Each engine's fetch does its own normalization (see
/// [`ColumnInfo::default`](schemaic_core::schema::ColumnInfo)) and hands the
/// finished column here, so [`assemble_schema`] stays pure grouping.
#[derive(Clone)]
pub(crate) struct ColRow {
    pub table: String,
    pub column: ColumnInfo,
}

/// One key-column of one index, plus the index-level attributes carried on every
/// row of it (they repeat per key column; the first row wins).
#[derive(Clone)]
pub(crate) struct IdxRow {
    pub table: String,
    pub index: String,
    pub unique: bool,
    pub column: schemaic_core::schema::IndexColumn,
    /// Access method, when the engine names one worth emitting.
    pub method: Option<String>,
    /// Partial-index predicate (PostgreSQL).
    pub predicate: Option<String>,
    /// This index holds something the model can't represent — see
    /// [`schemaic_core::schema::IndexInfo::lossy`]. Always false on MySQL.
    pub lossy: bool,
}

/// One `KEY_COLUMN_USAGE` row for a foreign key: `(table, constraint, column,
/// ref_schema, ref_table, ref_column)`. The referenced fields are `Option` since
/// the column is nullable in the catalogue (though non-null for the FK rows we
/// select). Aliased to keep [`assemble_schema`]'s signature readable.
pub(crate) type FkColRow = (
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// Assemble the fetched `information_schema` rows into a [`DbSchema`]: group
/// columns onto their tables, fold each index's key columns (in `SEQ_IN_INDEX`
/// order) into one [`IndexInfo`], flag an index FOREIGN when its name matches a
/// FK constraint, mark views, and attach view definitions. Pure — the async
/// `collect_schema` just runs the queries and hands the rows here — so the
/// key/uniqueness/foreign detection that drives editing + DDL is unit-tested.
///
/// Rows referencing a table not in `table_rows` are dropped. `idx_rows` and
/// `col_rows` are consumed in order, so callers must sort by
/// `TABLE_NAME, SEQ_IN_INDEX` / `ORDINAL_POSITION` as the queries do.
///
/// All rows must belong to **one** namespace, which is stamped onto every
/// produced table as [`TableInfo::schema`]: MySQL passes `None` (a database *is*
/// its namespace), and PostgreSQL calls this once per schema and concatenates,
/// since every row here is keyed by table name alone and two schemas may hold
/// same-named tables.
pub(crate) fn assemble_schema(
    schema: Option<&str>,
    table_rows: &[(String, String)],
    col_rows: &[ColRow],
    fk_col_rows: &[FkColRow],
    idx_rows: &[IdxRow],
    view_rows: &[(String, String)],
) -> DbSchema {
    let mut tables: Vec<TableInfo> = Vec::with_capacity(table_rows.len());
    let mut index: HashMap<String, usize> = HashMap::with_capacity(table_rows.len());
    for (name, ty) in table_rows {
        index.insert(name.clone(), tables.len());
        tables.push(TableInfo {
            schema: schema.map(str::to_string),
            name: name.clone(),
            is_view: ty.eq_ignore_ascii_case("VIEW"),
            ..Default::default()
        });
    }

    for c in col_rows {
        let Some(&ti) = index.get(&c.table) else {
            continue;
        };
        tables[ti].columns.push(c.column.clone());
    }

    // Fold the FK key-column rows into one `ForeignKeyInfo` per (table,
    // constraint), preserving column order. Rows missing a referenced table/
    // column are skipped (can't form a usable target).
    let mut fk_slot: HashMap<(usize, String), usize> = HashMap::new();
    for (t, cn, col, rs, rt, rc) in fk_col_rows {
        let Some(&ti) = index.get(t) else { continue };
        let (Some(rt), Some(rc)) = (rt.as_ref(), rc.as_ref()) else {
            continue;
        };
        match fk_slot.get(&(ti, cn.clone())) {
            Some(&fi) => {
                let fk = &mut tables[ti].foreign_keys[fi];
                fk.columns.push(col.clone());
                fk.ref_columns.push(rc.clone());
            }
            None => {
                let fi = tables[ti].foreign_keys.len();
                tables[ti].foreign_keys.push(ForeignKeyInfo {
                    name: cn.clone(),
                    columns: vec![col.clone()],
                    ref_schema: rs.clone(),
                    ref_table: rt.clone(),
                    ref_columns: vec![rc.clone()],
                    ..Default::default()
                });
                fk_slot.insert((ti, cn.clone()), fi);
            }
        }
    }

    for r in idx_rows {
        let Some(&ti) = index.get(&r.table) else {
            continue;
        };
        let table = &mut tables[ti];
        if let Some(existing) = table.indexes.iter_mut().find(|x| x.name == r.index) {
            existing.columns.push(r.column.clone());
        } else {
            table.indexes.push(IndexInfo {
                name: r.index.clone(),
                columns: vec![r.column.clone()],
                unique: r.unique,
                foreign: false, // set by the column-match pass below
                method: r.method.clone(),
                predicate: r.predicate.clone(),
                lossy: r.lossy,
                // Constraint-backed indexes are tagged by the engine's own fetch
                // afterwards (PostgreSQL only); the catalogue rows folded here
                // don't carry it.
                constraint: None,
                // Neither engine assembled here keeps a statement per index —
                // that is SQLite's `sqlite_master`, and SQLite doesn't come
                // through this fold.
                create_sql: None,
            });
        }
    }

    // Tag each index FOREIGN when its columns are exactly a FK's referencing
    // columns — matched by *columns*, not name. A FK's backing index is often
    // named after the column (e.g. classicmodels `customerNumber`), not the
    // constraint (`orders_ibfk_1`), so a name match misses it. Done after folding
    // so an index's full column list is known.
    for table in tables.iter_mut() {
        let fk_cols: Vec<&[String]> = table
            .foreign_keys
            .iter()
            .map(|fk| fk.columns.as_slice())
            .filter(|cols| !cols.is_empty())
            .collect();
        for ix in table.indexes.iter_mut() {
            let names: Vec<&str> = ix.column_names().collect();
            ix.foreign = fk_cols
                .iter()
                .any(|&cols| cols.len() == names.len() && cols.iter().eq(names.iter()));
        }
    }

    for (t, def) in view_rows {
        let Some(&ti) = index.get(t) else { continue };
        if !def.is_empty() {
            tables[ti].view_definition = Some(def.clone());
        }
    }

    DbSchema {
        tables,
        ..Default::default()
    }
}

/// Run the (unprepared, text-protocol) statement, stopping at the row cap, and
/// materialize it into a [`ResultSet`]. When `early_stop` is true, the row
/// stream is abandoned as soon as the cap is hit (the caller tears the
/// connection down); when false, the rest is drained so the connection stays
/// reusable for the next statement in a batch.
pub(crate) async fn collect_rows(
    conn: &mut Conn,
    sql: &str,
    dest: &mut RowDest,
    early_stop: bool,
) -> Result<ResultSet, DbError> {
    let row_cap = dest.cap();
    let qerr = |e: mysql_async::Error| DbError::Query(e.to_string());
    let start = std::time::Instant::now();

    let mut result = conn.query_iter(sql).await.map_err(qerr)?;

    // Column metadata arrives before any rows, and is present even for a
    // zero-row SELECT. A statement that returns no result set (DML/DDL) has no
    // columns — that's how we tell a grid apart from an affected-rows outcome.
    let columns: Vec<Column> = result.columns_ref().iter().map(map_column).collect();

    if columns.is_empty() {
        let affected = result.affected_rows();
        // Drain the (empty) result so the connection is clean.
        let _ = result.collect::<Row>().await;
        return Ok(
            ResultSet::affected_rows(columns, affected).with_elapsed(start.elapsed().as_millis())
        );
    }

    // Which columns hold raw bytes, answered **once for the result** rather than
    // once per cell. `Column::is_binary` splits a type name and walks a keyword
    // list; at the 200k-row cap on a wide result that is tens of millions of
    // calls in the row loop, for an answer that cannot change between rows.
    let binary: Vec<bool> = columns.iter().map(Column::is_binary).collect();
    // Hoisted for the same reason, and asked of the type name only: a bit-field's
    // bytes are a number, and nothing but the column says so.
    let bit: Vec<bool> = columns
        .iter()
        .map(|c| schemaic_core::model::type_is_bit(&c.type_name))
        .collect();
    // Assemble the result columnar, one row at a time, so we never hold a
    // row-major `Vec<Vec<Value>>` copy alongside the final storage.
    let chunk_capacity = dest.chunk_capacity();
    let mut builder = ResultBuilder::with_capacity(columns, chunk_capacity);
    let mut truncated = false;
    if let Some(mut stream) = result.stream::<Row>().await.map_err(qerr)? {
        while let Some(row) = stream.next().await {
            let row = row.map_err(qerr)?;
            if builder.row_count() < row_cap {
                let cells = convert_row(&row, builder.columns(), &binary, &bit);
                builder.push_row(&cells);
                // A stream hands the block over here and keeps reading into an
                // empty builder; a capped read never fills a chunk, so this is
                // dead weight for it and nothing more.
                if dest.chunk_full(builder.row_count(), builder.text_bytes()) {
                    dest.flush(&mut builder, chunk_capacity).await?;
                }
            } else {
                // A row beyond the cap exists → the result is truncated.
                truncated = true;
                if early_stop {
                    break;
                }
                // else: keep draining (discarding) to leave the conn clean.
            }
        }
    }

    // The tail: a stream's last block is usually short and may be empty, and the
    // export needs that last block even when it is — the columns for its header
    // come from the first chunk, and a table with no rows has only this one. Not
    // reached by a statement that returns no columns at all, which returned
    // above; `Db::stream_query` refuses those rather than letting the writer see
    // an empty stream and call the file finished.
    dest.flush(&mut builder, 0).await?;
    builder.set_truncated(truncated);
    builder.set_elapsed(start.elapsed().as_millis());
    Ok(builder.finish())
}

/// Map a wire column definition to our [`Column`], capturing its origin
/// (real database/table/column + key flags) when the server reports one.
/// Expression/aggregate/literal columns carry an empty `org_table`, which we
/// surface as `origin: None` — the signal that such a column is not editable.
fn map_column(c: &MyColumn) -> Column {
    let type_name = type_name_of(c);
    let binary = is_binary_data_type(&type_name);
    let f = c.flags();
    let flags = CoreColFlags {
        primary_key: f.contains(ColumnFlags::PRI_KEY_FLAG),
        unique_key: f.contains(ColumnFlags::UNIQUE_KEY_FLAG),
        not_null: f.contains(ColumnFlags::NOT_NULL_FLAG),
        auto_increment: f.contains(ColumnFlags::AUTO_INCREMENT_FLAG),
        no_default: f.contains(ColumnFlags::NO_DEFAULT_VALUE_FLAG),
    };
    let origin = column_origin(
        &c.schema_str(),
        &c.org_table_str(),
        &c.org_name_str(),
        flags,
        binary,
    );
    Column {
        name: c.name_str().to_string(),
        type_name,
        origin,
    }
}

/// Is the resolved SQL type a *binary-data* column (raw bytes), not merely
/// "binary charset"? Numeric / temporal columns also report charset 63, so this
/// keys off the resolved type name. Such values can't round-trip through the
/// text protocol losslessly, so the editing system treats them as read-only.
///
/// The list itself lives in `core::model::type_is_binary`, which is the same
/// question the export path asks of a column with no wire provenance to consult
/// — a second copy here is how the two would come to disagree.
fn is_binary_data_type(type_name: &str) -> bool {
    schemaic_core::model::type_is_binary(type_name)
}

/// Build a column's [`ColumnOrigin`] from its wire provenance, or `None` when
/// `org_table` is empty — an expression/aggregate/literal with no single base
/// column, the signal that such a column is not editable.
fn column_origin(
    schema: &str,
    org_table: &str,
    org_name: &str,
    flags: CoreColFlags,
    binary: bool,
) -> Option<ColumnOrigin> {
    if org_table.is_empty() {
        return None;
    }
    Some(ColumnOrigin {
        database: schema.to_string(),
        // MySQL has no namespace between database and table — `schema` here is
        // the wire protocol's `org_schema`, i.e. the database.
        schema: None,
        table: org_table.to_string(),
        column: org_name.to_string(),
        flags,
        binary,
        // MySQL's own row identity is always a column of the table; it has no
        // analogue of SQLite's `rowid`.
        implicit_key: false,
    })
}

/// Reconstruct a human SQL type name (`VARCHAR`, `INT UNSIGNED`, `DATETIME`, …)
/// from the wire column type + flags + charset — matching what the old sqlx
/// `type_info().name()` produced, so `parse_typed` and the UI keep behaving.
fn type_name_of(c: &MyColumn) -> String {
    resolve_type_name(
        c.column_type(),
        c.flags().contains(ColumnFlags::UNSIGNED_FLAG),
        c.character_set() == BINARY_CHARSET,
    )
}

/// Pure core of [`type_name_of`]: map a wire column type + UNSIGNED flag + binary
/// charset to a human SQL type name. Split out so the mapping (which drives
/// `parse_typed` and editability) is unit-tested without a wire column object.
fn resolve_type_name(ct: ColumnType, unsigned: bool, binary: bool) -> String {
    let base = match ct {
        ColumnType::MYSQL_TYPE_TINY => "TINYINT",
        ColumnType::MYSQL_TYPE_SHORT => "SMALLINT",
        ColumnType::MYSQL_TYPE_INT24 => "MEDIUMINT",
        ColumnType::MYSQL_TYPE_LONG => "INT",
        ColumnType::MYSQL_TYPE_LONGLONG => "BIGINT",
        ColumnType::MYSQL_TYPE_FLOAT => "FLOAT",
        ColumnType::MYSQL_TYPE_DOUBLE => "DOUBLE",
        ColumnType::MYSQL_TYPE_DECIMAL | ColumnType::MYSQL_TYPE_NEWDECIMAL => "DECIMAL",
        ColumnType::MYSQL_TYPE_YEAR => "YEAR",
        ColumnType::MYSQL_TYPE_BIT => "BIT",
        ColumnType::MYSQL_TYPE_TIMESTAMP | ColumnType::MYSQL_TYPE_TIMESTAMP2 => "TIMESTAMP",
        ColumnType::MYSQL_TYPE_DATE | ColumnType::MYSQL_TYPE_NEWDATE => "DATE",
        ColumnType::MYSQL_TYPE_TIME | ColumnType::MYSQL_TYPE_TIME2 => "TIME",
        ColumnType::MYSQL_TYPE_DATETIME | ColumnType::MYSQL_TYPE_DATETIME2 => "DATETIME",
        ColumnType::MYSQL_TYPE_JSON => "JSON",
        ColumnType::MYSQL_TYPE_ENUM => "ENUM",
        ColumnType::MYSQL_TYPE_SET => "SET",
        ColumnType::MYSQL_TYPE_GEOMETRY => "GEOMETRY",
        ColumnType::MYSQL_TYPE_VAR_STRING | ColumnType::MYSQL_TYPE_VARCHAR => {
            if binary {
                "VARBINARY"
            } else {
                "VARCHAR"
            }
        }
        ColumnType::MYSQL_TYPE_STRING => {
            if binary {
                "BINARY"
            } else {
                "CHAR"
            }
        }
        ColumnType::MYSQL_TYPE_TINY_BLOB => {
            if binary {
                "TINYBLOB"
            } else {
                "TINYTEXT"
            }
        }
        ColumnType::MYSQL_TYPE_MEDIUM_BLOB => {
            if binary {
                "MEDIUMBLOB"
            } else {
                "MEDIUMTEXT"
            }
        }
        ColumnType::MYSQL_TYPE_LONG_BLOB => {
            if binary {
                "LONGBLOB"
            } else {
                "LONGTEXT"
            }
        }
        ColumnType::MYSQL_TYPE_BLOB => {
            if binary {
                "BLOB"
            } else {
                "TEXT"
            }
        }
        ColumnType::MYSQL_TYPE_NULL => "NULL",
        _ => "UNKNOWN",
    };
    // MySQL reports UNSIGNED only for the numeric types.
    let numeric = matches!(
        ct,
        ColumnType::MYSQL_TYPE_TINY
            | ColumnType::MYSQL_TYPE_SHORT
            | ColumnType::MYSQL_TYPE_INT24
            | ColumnType::MYSQL_TYPE_LONG
            | ColumnType::MYSQL_TYPE_LONGLONG
            | ColumnType::MYSQL_TYPE_FLOAT
            | ColumnType::MYSQL_TYPE_DOUBLE
            | ColumnType::MYSQL_TYPE_DECIMAL
            | ColumnType::MYSQL_TYPE_NEWDECIMAL
    );
    if numeric && unsigned {
        format!("{base} UNSIGNED")
    } else {
        base.to_string()
    }
}

/// Convert one wire row into our typed cells. Over the text protocol every
/// non-NULL value arrives as `Bytes` (its textual form), so we parse it with the
/// column's type exactly as the old code did; the typed arms cover the binary
/// protocol defensively.
///
/// `binary[i]` is whether column `i` holds raw bytes, computed **once for the
/// result** by the caller: `Column::is_binary` splits a type name and walks a
/// keyword list, which is not an answer to re-derive per cell in a loop that
/// runs up to the row cap times the column count.
///
/// **A raw-bytes column is the exception, and it used to be a data bug.** A
/// BLOB/BINARY/BIT value arrives as its literal bytes, and
/// `from_utf8_lossy`-ing those produced mojibake that *looks like data* — so a
/// CSV or `INSERT` export wrote the replacement characters as the value and
/// re-imported as the wrong bytes. It renders as `binary_display` now, the same
/// `<n bytes>` SQLite and PostgreSQL show, which says what it is and cannot be
/// mistaken for the value.
fn convert_row(row: &Row, columns: &[Column], binary: &[bool], bit: &[bool]) -> Vec<Value> {
    (0..columns.len())
        .map(|i| match row.as_ref(i) {
            None | Some(MyValue::NULL) => Value::Null,
            Some(MyValue::Bytes(b)) if binary.get(i).copied().unwrap_or(false) => {
                Value::Str(binary_display(b.len()))
            }
            // **A bit-field arrives as bytes and is a number.** Nothing in the
            // value says so — only the column's type does — and lossy-decoding
            // those bytes as text is how a `BIT(8)` holding 10 became a newline
            // character. `bit_value` reads them the way MySQL wrote them and the
            // way it takes them back.
            //
            // `UInt`, not `Str`: the number is the value, and a `Value::Str`
            // carries a *quoted* literal into every export. `'10'` assigned to a
            // `BIT` column is the raw bits of its two bytes — 12594 on a
            // `BIT(16)`, "Data too long" on a `BIT(8)` — so the round trip that
            // taking `BIT` off the binary list was meant to enable was writing
            // wrong data instead of withholding it. The grid shows the same
            // digits either way.
            Some(MyValue::Bytes(b)) if bit.get(i).copied().unwrap_or(false) => {
                schemaic_core::model::bit_cell(b)
            }
            Some(MyValue::Bytes(b)) => parse_typed(
                String::from_utf8_lossy(b).into_owned(),
                &columns[i].type_name,
            ),
            Some(MyValue::Int(n)) => Value::Int(*n),
            Some(MyValue::UInt(n)) => Value::UInt(*n),
            Some(MyValue::Float(f)) => Value::Float(*f as f64),
            Some(MyValue::Double(f)) => Value::Float(*f),
            Some(other) => Value::Str(other.as_sql(false).trim_matches('\'').to_string()),
        })
        .collect()
}

/// Why a DDL run stopped, and — the part that matters — **how much of it
/// already happened**.
///
/// The two engines differ in a way no amount of wrapping can hide. PostgreSQL
/// has transactional DDL, so a failure rolls the whole plan back and `applied`
/// is 0. MySQL commits implicitly around every DDL statement, so a plan that
/// fails halfway has genuinely half-applied — and the honest thing is to say
/// which statement failed and how many are already in effect, not to pretend the
/// table is untouched.
#[derive(Debug, Clone)]
pub struct DdlError {
    pub message: String,
    /// 0-based index of the statement that failed, **counted over the whole
    /// emitted plan** — including its session scaffolding, because the script the
    /// user is reading in the preview panel includes it too, and an ordinal that
    /// disagreed with what is on screen would be a second wrong number rather
    /// than a fix for the first.
    pub at: usize,
    /// Statements that are in effect on the server despite the failure — the ones
    /// that changed something outliving the connection, which is not the same as
    /// the ones that succeeded (`ddl::alters_the_database`). Always 0 on
    /// PostgreSQL.
    pub applied: usize,
}

impl std::fmt::Display for DdlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "statement {} failed: {}", self.at + 1, self.message)?;
        if self.applied > 0 {
            write!(
                f,
                " — {} earlier statement{} already applied and cannot be rolled back",
                self.applied,
                if self.applied == 1 { "" } else { "s" }
            )?;
        }
        Ok(())
    }
}

/// How long a DDL statement may wait for a lock before giving up.
///
/// Short on purpose: this bounds *acquiring* the lock, not holding it, so a
/// legitimately long `ALTER` on a large table is unaffected — only one that never
/// starts because something else holds the table. Ten seconds is the point past
/// which a modal that refuses every exit while it works has stopped being a
/// progress indicator.
const DDL_LOCK_WAIT_SECS: u32 = 10;

// Zero doesn't mean "fail immediately" on either engine: PostgreSQL reads
// `lock_timeout = 0` as *disabled*, which would silently restore the unbounded
// wait this exists to prevent, and MySQL rejects it outright. Retuning the
// constant to 0 is a compile error rather than a quiet regression.
const _: () = assert!(DDL_LOCK_WAIT_SECS >= 1);

/// The statement that applies [`DDL_LOCK_WAIT_SECS`] to a DDL connection.
///
/// Without it, Apply can hang forever with no diagnosis and no way out: MySQL's
/// `lock_wait_timeout` defaults to a year (a day on MariaDB) and PostgreSQL's
/// `lock_timeout` defaults to *disabled*, so a plan queued behind a lock — the
/// user's own uncommitted transaction, another session's long read — simply never
/// returns. Bounded, it comes back as a server error the preview can show.
///
/// MySQL's variable is `lock_wait_timeout`, not `innodb_lock_wait_timeout`: what
/// an `ALTER TABLE` waits on is the **metadata** lock, and the InnoDB one covers
/// row locks (and is already bounded at 50s by default).
/// SQLite's answer is the empty string, and the caller skips an empty statement.
/// It has no lock-timeout *setting* — waiting is configured per connection as a
/// busy timeout, which `sqlite::open` sets — and it takes a single write lock over
/// the whole file, so the failure mode this bounds (a plan queued behind someone
/// else's metadata lock) has no analogue: the write either starts, or waits out
/// that busy timeout and returns `SQLITE_BUSY`.
fn lock_wait_sql(engine: Engine) -> String {
    match engine {
        Engine::MySql => format!("SET SESSION lock_wait_timeout = {DDL_LOCK_WAIT_SECS}"),
        Engine::Postgres => format!("SET lock_timeout = '{DDL_LOCK_WAIT_SECS}s'"),
        Engine::Sqlite => String::new(),
    }
}

/// Did this DDL run leave the database different from how the caller last read
/// it — and so must the schema be re-introspected?
///
/// The only outcome that changed nothing is a plan that stopped before its first
/// statement took effect. Every other outcome did: a success obviously, and a
/// half-applied MySQL plan because [`DdlError::applied`] statements are in force
/// on the server and cannot be rolled back.
///
/// This exists as a function rather than an `is_ok()` at the call site because
/// the caller sees the error as a display string by then, where "nothing was
/// applied" and "half the plan was applied" look identical — and `db_nodes` is
/// what the schema tree, the grid's key icons, the completion index and
/// `intel`'s catalog all read.
pub fn ddl_changed_schema(res: &Result<(), DdlError>) -> bool {
    match res {
        Ok(()) => true,
        Err(e) => e.applied > 0,
    }
}

impl Db {
    /// Run a generated DDL plan against `database`.
    ///
    /// The statements come from [`ChangeSet::emit`](schemaic_core::ddl::ChangeSet::emit),
    /// which has already put them in an order that works; this only decides how
    /// much atomicity the engine can actually give:
    ///
    /// * **PostgreSQL** — one transaction. `ALTER TABLE`/`CREATE INDEX` are
    ///   transactional there, so a failure anywhere leaves nothing behind.
    /// * **MySQL** — sequential, stopping at the first failure. Every DDL
    ///   statement commits implicitly, so a transaction here would be theatre
    ///   (`tx::implicit_commit` models the same truth for the manual-transaction
    ///   path). The caller is told which statement failed and how many stuck.
    ///
    /// Runs on a fresh connection, like every other operation — a designer's
    /// Apply must not ride inside a tab's transaction. It can still *queue*
    /// behind one, which is what [`lock_wait_sql`] bounds: the app asks about
    /// open transactions before applying, but nothing can ask about the locks
    /// another client holds.
    pub async fn run_ddl(
        &self,
        database: &str,
        stmts: &[String],
        cancel: CancellationToken,
    ) -> Result<(), DdlError> {
        let fail = |at: usize, applied: usize, e: DbError| DdlError {
            message: e.to_string(),
            at,
            applied,
        };
        if stmts.is_empty() {
            return Ok(());
        }
        match self.engine {
            Engine::Postgres => return pg::run_ddl(self, database, stmts, cancel).await,
            // **SQLite runs the plan**, whether that plan is a drop it performs
            // directly or the twelve-step rebuild a designer edit compiles to
            // (`ddl::sqlite_rebuild_sql`). What may reach here is decided
            // upstream, by `ddl::supports_change` and by `diff` — both can see
            // the `Change`, where this function has only strings.
            //
            // Unlike the MySQL path below, that arm also suspends foreign-key
            // enforcement for the transaction and checks it before committing;
            // the reason is in `sqlite::run_ddl`, and it is not an optimisation.
            Engine::Sqlite => return sqlite::run_ddl(self, stmts, cancel).await,
            Engine::MySql => {}
        }
        let mut conn = self
            .open(Some(database), false)
            .await
            .map_err(|e| fail(0, 0, e))?;
        let conn_id = conn.id();
        // Best-effort: a server old enough not to have the variable keeps its own
        // default rather than failing the plan over the bound.
        let _ = conn.query_drop(lock_wait_sql(self.engine)).await;
        let dialect = self.engine.dialect();
        let mut out = Ok(());
        for (i, sql) in stmts.iter().enumerate() {
            let step = tokio::select! {
                r = conn.query_drop(sql) => r.map_err(|e| DbError::Query(e.to_string())),
                _ = cancel.cancelled() => {
                    self.kill_query(conn_id).await;
                    Err(DbError::Cancelled)
                }
            };
            if let Err(e) = step {
                // **What applied, not what succeeded.** A routine, trigger or
                // event edit is emitted wrapped in a session guard, and those
                // `SET`s succeed against session variables on a connection this
                // function disconnects four lines down — nothing about them
                // outlives the call. Counting them made a rejected `ALTER EVENT`
                // report "2 earlier statements already applied and cannot be
                // rolled back" over a plan that had changed nothing, on the app's
                // only disclosure of a genuinely half-applied migration.
                //
                // The decision is `ddl::applied_count`'s, and it is there rather
                // than a counter here because it is a decision about emitted SQL
                // and this loop has only strings — which is how the scaffolding
                // came to be counted in the first place.
                out = Err(fail(
                    i,
                    schemaic_core::ddl::applied_count(stmts, i, dialect),
                    e,
                ));
                break;
            }
        }
        let _ = conn.disconnect().await;
        out
    }

    /// Run a **server-level** DDL plan — `CREATE DATABASE` / `DROP DATABASE`,
    /// the two changes `ddl::is_server_level` marks.
    ///
    /// Separate from [`Db::run_ddl`] because neither statement can take that
    /// function's two commitments:
    ///
    /// - It connects **without** naming the target. A database being created
    ///   cannot be connected to, and one being dropped must not be — PostgreSQL
    ///   refuses outright, and MySQL leaves the session pointed at a database
    ///   that no longer exists. `avoid` is the target, so the PostgreSQL arm can
    ///   keep it out of the maintenance candidates.
    /// - There is **no transaction**. PostgreSQL refuses both statements inside
    ///   one, which is precisely what `run_ddl` wraps every plan in. Nothing is
    ///   lost: a server-level plan is one statement, so there is no second one
    ///   for a rollback to protect.
    ///
    /// SQLite has no such statement at all — a database there is a file — and
    /// `ddl::supports_database_editing` refuses the change long before this, so
    /// its arm reports that rather than inventing a filesystem action.
    pub async fn run_server_ddl(
        &self,
        avoid: Option<&str>,
        stmts: &[String],
        cancel: CancellationToken,
    ) -> Result<(), DdlError> {
        let fail = |at: usize, applied: usize, e: DbError| DdlError {
            message: e.to_string(),
            at,
            applied,
        };
        if stmts.is_empty() {
            return Ok(());
        }
        match self.engine {
            Engine::Postgres => return pg::run_server_ddl(self, avoid, stmts, cancel).await,
            Engine::Sqlite => {
                return Err(fail(
                    0,
                    0,
                    DbError::Query(
                        "SQLite has no databases to create or drop — a database there is a \
                         file, which Schemaic does not create or delete for you."
                            .to_string(),
                    ),
                ));
            }
            Engine::MySql => {}
        }
        // **Serverless, not `open(None)`.** `avoid` names the database this
        // plan is about to drop or create, and `open(None)` fills an unnamed
        // database in from the connection's own — so `DROP DATABASE shop` on a
        // connection configured for `shop` ran on a session pointed at its
        // target. The comment here used to claim the opposite, and was true
        // until the connection gained a configured database. (PostgreSQL still
        // reads `avoid` in its own arm above: it must connect to *some*
        // database, so it picks one that is not the target. MySQL needs none.)
        let mut conn = self
            .open_serverless(false)
            .await
            .map_err(|e| fail(0, 0, e))?;
        let conn_id = conn.id();
        let _ = conn.query_drop(lock_wait_sql(self.engine)).await;
        let mut out = Ok(());
        for (i, sql) in stmts.iter().enumerate() {
            let step = tokio::select! {
                r = conn.query_drop(sql) => r.map_err(|e| DbError::Query(e.to_string())),
                _ = cancel.cancelled() => {
                    self.kill_query(conn_id).await;
                    Err(DbError::Cancelled)
                }
            };
            if let Err(e) = step {
                // No session-guard scaffolding on this path — every statement
                // here is one the user reviewed — so what applied is simply how
                // many ran, and `ddl::applied_count` has nothing to discount.
                out = Err(fail(i, i, e));
                break;
            }
        }
        let _ = conn.disconnect().await;
        out
    }
}

/// How many statements the driver may run ahead of the server.
///
/// The bound is the whole progress design: with the reader unable to get more
/// than this far in front, `script::Splitter::consumed` is within a few
/// statements of what the server has actually applied, so the driver can report
/// progress from the file position alone and no second channel is needed. It is
/// also the backpressure — reading a 2 GB file as fast as the disk allows, into
/// a queue the server drains one statement at a time, is how a load comes to
/// hold the whole file in memory after all.
///
/// **It bounds statements, not bytes, and the real ceiling is the product.**
/// Sixty-four `mysqldump` extended `INSERT`s is a few tens of megabytes, which
/// is the case this was sized for; sixty-four statements from a dump written at
/// a 16 MB `max_allowed_packet` is a gigabyte, and the reader's own
/// `MAX_PENDING_BYTES` (256 MB) bounds one *unfinished* statement rather than
/// the queue behind it. So "cannot pile up in memory" is true of the files this
/// meets and not a guarantee. Bounding the queue in bytes instead is the honest
/// fix and needs a real large-packet dump to size; until then this says what it
/// actually promises.
pub const SCRIPT_QUEUE: usize = 64;

impl Db {
    /// Run a `.sql` script: execute every statement the reader hands over, in
    /// order, on **one connection**, stopping at the first the server refuses.
    ///
    /// Returns how the executing half ended and how many statements ran; the
    /// driver folds that together with how the *reading* half ended through
    /// [`schemaic_core::script::run_outcome`], which is where the precedence
    /// between the two lives.
    ///
    /// **One pinned connection, and this is the second exception to
    /// one-connection-per-operation** (the first being a Manual-mode tab's
    /// `Session`). A script's statements are not independent: a dump opens with
    /// `SET FOREIGN_KEY_CHECKS = 0`, may carry its own `BEGIN` … `COMMIT`, and
    /// on MySQL switches the terminator around a routine — every one of those is
    /// *session* state, so a fresh connection per statement would apply the
    /// guard to a connection that is already gone and then fail the load on the
    /// first child row.
    ///
    /// **Nothing is wrapped in a transaction here, deliberately.** `run_ddl`
    /// wraps on all three engines, which is why it cannot be reused: the file
    /// decides. `dump.rs`'s *Replaying → One transaction* already writes
    /// `BEGIN`/`COMMIT` into the file when the user asked for it, and a second
    /// `BEGIN` around that is not what any of the three engines does with a
    /// nested one. `script::Probe::own_transaction` is what lets the UI say
    /// which kind of file this is before the run starts.
    pub async fn run_script(
        &self,
        database: &str,
        rx: tokio::sync::mpsc::Receiver<schemaic_core::script::Statement>,
        cancel: CancellationToken,
    ) -> (schemaic_core::script::ExecEnd, usize) {
        match self.engine {
            Engine::Postgres => pg::run_script(self, database, rx, cancel).await,
            Engine::Sqlite => sqlite::run_script(self, rx, cancel).await,
            Engine::MySql => self.run_script_mysql(database, rx, cancel).await,
        }
    }

    async fn run_script_mysql(
        &self,
        database: &str,
        mut rx: tokio::sync::mpsc::Receiver<schemaic_core::script::Statement>,
        cancel: CancellationToken,
    ) -> (schemaic_core::script::ExecEnd, usize) {
        use schemaic_core::script::ExecEnd;
        let mut conn = match self.open(Some(database), false).await {
            Ok(c) => c,
            Err(e) => return (ExecEnd::Connect(e.to_string()), 0),
        };
        let conn_id = conn.id();
        // **No lock bound is set here.** See `pg::run_script` for the whole
        // reasoning: `DDL_LOCK_WAIT_SECS` is documented for the Apply modal's
        // short reviewed plan, and a restore that dies at statement N with N−1
        // applied and no transaction of ours to roll back is worse than
        // waiting. `mysql <` sets nothing either, and Stop here kills the
        // running statement server-side.
        let mut ran = 0usize;
        let end = loop {
            // Cancel has to be reachable **while waiting for the next
            // statement**, not only while one is running. A load stalled on a
            // slow disk spends most of its life here, and a Stop that only
            // landed between statements would look ignored.
            let next = tokio::select! {
                s = rx.recv() => s,
                _ = cancel.cancelled() => break ExecEnd::Cancelled,
            };
            let Some(st) = next else { break ExecEnd::Done };
            // **The killed statement is awaited, not dropped.** `KILL QUERY` is
            // a request — MySQL documents that it may be ignored during an
            // online `ALTER`'s commit phase — so throwing the future away left
            // `ran` a floor while the panel presents it as the count. Scoped so
            // the borrow of `st.sql` ends before the `Failed` arm moves it.
            let step = {
                let mut fut = std::pin::pin!(conn.query_drop(&st.sql));
                let raced = tokio::select! {
                    r = fut.as_mut() => Some(r),
                    _ = cancel.cancelled() => None,
                };
                match raced {
                    Some(Ok(())) => pg::ScriptStep::Ran,
                    Some(Err(e)) => pg::ScriptStep::Failed(e.to_string()),
                    None => {
                        self.kill_query(conn_id).await;
                        pg::ScriptStep::Cancelled {
                            ran: fut.await.is_ok(),
                        }
                    }
                }
            };
            match step {
                pg::ScriptStep::Ran => ran += 1,
                pg::ScriptStep::Cancelled { ran: landed } => {
                    if landed {
                        ran += 1;
                    }
                    break ExecEnd::Cancelled;
                }
                pg::ScriptStep::Failed(message) => {
                    break ExecEnd::Failed {
                        message,
                        sql: st.sql,
                        line: st.line,
                    };
                }
            }
        };
        let _ = conn.disconnect().await;
        (end, ran)
    }
}

/// Where an import is writing, and what its `INSERT`s name.
///
/// `columns` are bare names (unquoted); the quoting is the export path's, applied
/// when the statement is built, so it can't drift from the SQL export that reads
/// the same table back out.
pub struct ImportTarget<'a> {
    pub database: &'a str,
    /// The PostgreSQL namespace, when the table has one. On MySQL a database *is*
    /// the namespace, so this is `None`.
    pub schema: Option<&'a str>,
    pub table: &'a str,
    pub columns: &'a [String],
}

/// A source of rows to import. Errors are the reader's — a malformed record or a
/// value that wouldn't coerce — and abort the transaction.
///
/// `Send` because the import runs on the tokio runtime: the reader is pulled
/// between `await`s, so it crosses whatever thread the task resumes on.
pub type RowSource<'a> = &'a mut (dyn Iterator<Item = Result<Vec<Value>, String>> + Send);

impl Db {
    /// Bulk-load rows into one table in a single transaction, as batched
    /// multi-row `INSERT`s.
    ///
    /// Deliberately **not** [`Db::commit_writes`], though it borrows its
    /// discipline. That path runs one statement per row with an exactly-one-row
    /// check, which is right for a handful of grid edits and ruinous for a file:
    /// 100k rows would be 100k round-trips inside one transaction. Here each
    /// statement carries up to `import::INSERT_BATCH_ROWS` rows and the check
    /// becomes "this batch affected exactly as many rows as it had" — same
    /// guarantee that nothing landed half-applied, at a thousandth of the
    /// round-trips.
    ///
    /// All-or-nothing: any reader error, any batch whose count doesn't match, or
    /// a cancellation rolls the whole thing back — **as far as the engine allows**.
    /// A MySQL table on `MyISAM`/`MEMORY`/`ARCHIVE`/`CSV` ignores `BEGIN` and
    /// `ROLLBACK`, so the batches already inserted stay; the error then says so
    /// (`rollback` reads the server's warning 1196) rather than reporting an undo
    /// that didn't happen, and
    /// the import modal warns before the load starts.
    ///
    /// Rows are pulled from `rows` in
    /// batches between statements, so the file is never held in memory — and
    /// since a reader that parses a batch does so between two awaits, the work it
    /// does there should stay small.
    pub async fn import_rows(
        &self,
        target: ImportTarget<'_>,
        rows: RowSource<'_>,
        cancel: CancellationToken,
    ) -> Result<u64, DbError> {
        if target.columns.is_empty() {
            return Err(DbError::Query("No columns to import into".to_string()));
        }
        match self.engine {
            Engine::Postgres => return pg::import_rows(self, target, rows, cancel).await,
            Engine::Sqlite => return sqlite::import_rows(self, target, rows, cancel).await,
            Engine::MySql => {}
        }
        let mut conn = self.open(Some(target.database), false).await?;
        let conn_id = conn.id();
        // `None` = the cancel arm won. The rollback can't happen inside the arm:
        // `select!` keeps every future alive across its handler, so `import_on`'s
        // `&mut conn` is still outstanding there — which is why the disconnect
        // has always been after the block too.
        let done: Option<Result<u64, DbError>> = tokio::select! {
            r = import_on(&mut conn, self.engine.dialect(), &target, rows) => Some(r),
            _ = cancel.cancelled() => {
                self.kill_query(conn_id).await;
                None
            }
        };
        // **Cancelling is a write-path exit like any other, so it rolls back
        // through `rollback()` and reports what that achieved.** It used to
        // `kill_query` and disconnect, and the modal then said, unconditionally,
        // "the transaction rolled back, so nothing was written" — which on
        // `MyISAM`/`MEMORY`/`ARCHIVE`/`CSV` is false: every batch already
        // executed is durable there, so the user re-ran the import and doubled
        // the rows it had already loaded. It was the one exit in this path that
        // skipped `Rollback::note()`, and the only one whose sentence was
        // composed in the UI, so there was nowhere for the note to attach.
        //
        // The connection is reused deliberately: the transaction belongs to it,
        // so a `ROLLBACK` on a fresh one would undo nothing. A `ROLLBACK` that
        // cannot be sent at all (the killed statement left the protocol mid-
        // exchange) is `Rollback::Incomplete`, which is the safe reading — it
        // says the rows may still be there rather than promising they aren't.
        let outcome = match done {
            Some(r) => r,
            None => match rollback(&mut conn, "ROLLBACK").await {
                Rollback::Complete => Err(DbError::Cancelled),
                // Not `Cancelled`: that variant is what the modal renders as
                // "nothing was written", and here something was.
                undone => Err(DbError::Query(format!("Import cancelled{}", undone.note()))),
            },
        };
        let _ = conn.disconnect().await;
        outcome
    }
}

/// ` ORDER BY a, b` for the Live Monitor's window, or `""` when there is no key
/// to order by. `quote` is the engine's identifier quoter, so the two callers
/// can't drift on quoting.
fn order_by_clause(cols: Option<&[String]>, quote: fn(&str) -> String) -> String {
    match cols.filter(|c| !c.is_empty()) {
        Some(cols) => format!(
            " ORDER BY {}",
            cols.iter().map(|c| quote(c)).collect::<Vec<_>>().join(", ")
        ),
        None => String::new(),
    }
}

/// Pull the next batch **off the executor**. `Ok(None)` at the end.
///
/// The source is a `std::io::BufReader` over the import file, so every pull is a
/// blocking `read` — and it happens between awaited DB round-trips, inside an
/// async task. Without this the read stalls a runtime worker for its duration,
/// and every unrelated task scheduled on that worker (the health ping, a schema
/// fetch, another tab's query) waits behind file IO on a slow disk or a network
/// share. That is exactly what `export_file` and `import_probe` use
/// `spawn_blocking` to avoid; this path is the one that reads the most and runs
/// the longest.
///
/// `block_in_place` rather than a reader thread feeding a channel: it is the
/// interleaved shape here (blocking pull, awaited write, repeat), and it doesn't
/// restructure the bulk-write loop. It **panics** on a current-thread runtime,
/// and the `--mcp-serve` mode builds one, so the flavour is checked rather than
/// assumed — the MCP server has no import path today, and this stays correct if
/// it ever gets one.
fn next_batch_off_executor(rows: RowSource<'_>) -> Result<Option<Vec<Vec<Value>>>, DbError> {
    use tokio::runtime::{Handle, RuntimeFlavor};
    match Handle::try_current().map(|h| h.runtime_flavor()) {
        Ok(RuntimeFlavor::MultiThread) => tokio::task::block_in_place(|| next_batch(rows)),
        _ => next_batch(rows),
    }
}

/// Pull the next batch of rows from the source. `Ok(None)` at the end.
fn next_batch(rows: RowSource<'_>) -> Result<Option<Vec<Vec<Value>>>, DbError> {
    let mut batch = Vec::with_capacity(schemaic_core::import::INSERT_BATCH_ROWS);
    // `next()` in a loop rather than `by_ref().take(..)`: `by_ref` isn't callable
    // on a `dyn Iterator`, and the source has to stay borrowed for the next batch.
    while batch.len() < schemaic_core::import::INSERT_BATCH_ROWS {
        match rows.next() {
            Some(row) => batch.push(row.map_err(DbError::Query)?),
            None => break,
        }
    }
    Ok(if batch.is_empty() { None } else { Some(batch) })
}

/// The MySQL half of [`Db::import_rows`], on an already-open connection.
async fn import_on(
    conn: &mut Conn,
    dialect: schemaic_core::intel::SqlDialect,
    target: &ImportTarget<'_>,
    rows: RowSource<'_>,
) -> Result<u64, DbError> {
    let qerr = |e: mysql_async::Error| DbError::Query(e.to_string());
    let cols: Vec<&str> = target.columns.iter().map(String::as_str).collect();
    conn.query_drop("BEGIN").await.map_err(qerr)?;

    let mut total: u64 = 0;
    loop {
        // A reader error (a bad record, a value that wouldn't coerce) has to undo
        // the transaction too — returning straight out would leave it open until
        // the connection drops, which is a lock held for no reason.
        let batch = match next_batch_off_executor(rows) {
            Ok(Some(b)) => b,
            Ok(None) => break,
            Err(e) => {
                let undone = rollback(conn, "ROLLBACK").await;
                return Err(match (e, undone) {
                    // Only worth saying when it isn't what the message implies.
                    (e, Rollback::Complete) => e,
                    (DbError::Query(msg), undone) => {
                        DbError::Query(format!("{msg}{}", undone.note()))
                    }
                    (e, _) => e,
                });
            }
        };
        let Some(sql) = schemaic_core::import::build_insert(
            target.database,
            target.schema,
            target.table,
            &cols,
            &batch,
            dialect,
        ) else {
            continue;
        };
        if let Err(e) = conn.query_drop(&sql).await {
            let msg = e.to_string();
            let undone = rollback(conn, "ROLLBACK").await;
            return Err(DbError::Query(format!("{msg}{}", undone.note())));
        }
        let affected = conn.affected_rows();
        if affected != batch.len() as u64 {
            let n = batch.len();
            let undone = rollback(conn, "ROLLBACK").await;
            return Err(DbError::Query(format!(
                "a batch of {n} rows inserted {affected}{}",
                undone.note()
            )));
        }
        total += affected;
    }

    conn.query_drop("COMMIT").await.map_err(qerr)?;
    Ok(total)
}

/// Apply a batch of staged grid mutations — `UPDATE`s then `INSERT`s — in a
/// single transaction. Every statement must affect **exactly one row**; if any
/// affects zero or more than one (a stale/ambiguous UPDATE identity, or an
/// INSERT that didn't add exactly one row), the whole transaction is rolled back
/// and an error returned, so nothing is half-applied. On success the transaction
/// commits and the total number of affected rows is returned.
///
/// UPDATE identity comes from each edit's `key` (typically the primary key);
/// INSERT columns not listed take their server default (auto-increment /
/// `DEFAULT` / `NULL`). All values are bound parameters, coerced by the server to
/// the column type. Cancellation kills the in-flight statement server-side; the
/// open transaction is then rolled back when the connection drops.
impl Db {
    pub async fn commit_writes(
        &self,
        write: &GridWrite,
        cancel: CancellationToken,
    ) -> Result<u64, DbError> {
        if write.is_empty() {
            return Ok(0);
        }
        match self.engine {
            Engine::Postgres => return pg::commit_writes(self, write, cancel).await,
            Engine::Sqlite => return sqlite::commit_writes(self, write, cancel).await,
            Engine::MySql => {}
        }
        // `client_found_rows` so the 1-row guard counts matches, not changes.
        let mut conn = self.open(None, true).await?;
        let conn_id = conn.id();

        let outcome = tokio::select! {
            r = write_on(&mut conn, write, TxScope::Own) => r,
            _ = cancel.cancelled() => {
                self.kill_query(conn_id).await;
                Err(DbError::Cancelled)
            }
        };

        let _ = conn.disconnect().await;
        outcome
    }
}

/// How a batch of writes gets its atomicity.
///
/// A fresh connection owns the whole transaction ([`TxScope::Own`]). Inside a
/// user's **manual** transaction the batch must be atomic *without* ending that
/// transaction, so it nests under a savepoint ([`TxScope::Savepoint`]) — the
/// 1-row guard then rolls back only its own batch and leaves the surrounding
/// transaction intact and usable. (On PostgreSQL the savepoint is what makes a
/// failed batch recoverable at all: a bare error aborts the whole transaction.)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TxScope {
    Own,
    Savepoint,
}

/// The savepoint is named `schemaic_w` throughout. A fixed name is safe because
/// batches never overlap on one connection — the session serialises them behind
/// its mutex — and it keeps these strings `&'static`.
impl TxScope {
    pub(crate) fn begin_sql(self) -> &'static str {
        match self {
            TxScope::Own => "BEGIN",
            TxScope::Savepoint => "SAVEPOINT schemaic_w",
        }
    }

    /// Make the batch permanent — for a savepoint that means releasing it, which
    /// merges it into the enclosing transaction rather than committing anything.
    pub(crate) fn commit_sql(self) -> &'static str {
        match self {
            TxScope::Own => "COMMIT",
            TxScope::Savepoint => "RELEASE SAVEPOINT schemaic_w",
        }
    }

    /// Undo the batch, and nothing beyond it.
    pub(crate) fn rollback_sql(self) -> &'static str {
        match self {
            TxScope::Own => "ROLLBACK",
            TxScope::Savepoint => "ROLLBACK TO SAVEPOINT schemaic_w",
        }
    }
}

/// Apply a staged batch of grid mutations on an already-open connection:
/// deletes → updates → inserts, each required to affect exactly one row, the
/// whole batch rolled back if any doesn't. `scope` decides whether that
/// atomicity comes from a transaction of its own or a nested savepoint.
///
/// Deletes run first so "delete a row, then insert one with the same unique key"
/// works. The caller is responsible for `client_found_rows` being on — the guard
/// counts *matched* rows, not *changed* ones.
pub(crate) async fn write_on(
    conn: &mut Conn,
    write: &GridWrite,
    scope: TxScope,
) -> Result<u64, DbError> {
    let qerr = |e: mysql_async::Error| DbError::Query(e.to_string());
    conn.query_drop(scope.begin_sql()).await.map_err(qerr)?;

    // One statement + its 1-row check. On a miss the batch is undone and the
    // error describes what happened, in the caller's terms — the verdict and its
    // wording are `one_row_verdict`, shared with the PostgreSQL executor, and
    // what the rollback *achieved* is asked of the server rather than assumed.
    async fn one(
        conn: &mut Conn,
        scope: TxScope,
        sql: String,
        params: Params,
        step: WriteStep<'_>,
    ) -> Result<u64, DbError> {
        if let Err(e) = conn.exec_drop(sql, params).await {
            let msg = e.to_string();
            let undone = rollback(conn, scope.rollback_sql()).await;
            return Err(DbError::Query(format!("{msg}{}", undone.note())));
        }
        let affected = conn.affected_rows();
        if let Err(msg) = one_row_verdict(step, affected) {
            let undone = rollback(conn, scope.rollback_sql()).await;
            return Err(DbError::Query(format!("{msg}{}", undone.note())));
        }
        Ok(affected)
    }

    let mut total: u64 = 0;
    // Deletes → updates → inserts, ordered by `GridWrite::plan` rather than by
    // three loops each engine has to keep in step.
    for step in write.plan() {
        let (sql, params) = match step {
            WriteStep::Delete(del) => build_delete(del),
            WriteStep::Update(edit) => build_update(edit),
            WriteStep::Insert(ins) => build_insert(ins),
        };
        total += one(conn, scope, sql, params, step).await?;
    }

    if let Err(e) = conn.query_drop(scope.commit_sql()).await {
        let msg = e.to_string();
        let undone = rollback(conn, scope.rollback_sql()).await;
        return Err(DbError::Query(format!("{msg}{}", undone.note())));
    }
    Ok(total)
}

/// Roll back, and find out from the server whether it worked.
///
/// MySQL's `ROLLBACK` **succeeds** when the transaction touched a
/// non-transactional table (`MyISAM`, `MEMORY`, `ARCHIVE`, `CSV`) and raises
/// warning **1196** — *"Some non-transactional changed tables couldn't be rolled
/// back"* — instead. Every rollback on this path used to be `let _ =
/// conn.query_drop(…)`, discarding the result *and* the server's own statement
/// that the undo was partial, so the write path promised an atomicity the engine
/// had just said it couldn't provide.
///
/// `SHOW WARNINGS` is read immediately after, since the next statement clears
/// it. Anything unreadable resolves to [`Rollback::Incomplete`] — the write
/// path must not claim more than it knows.
async fn rollback(conn: &mut Conn, sql: &str) -> Rollback {
    /// `ER_WARNING_NOT_COMPLETE_ROLLBACK`.
    const INCOMPLETE_ROLLBACK: u32 = 1196;
    if conn.query_drop(sql).await.is_err() {
        return Rollback::Incomplete;
    }
    let warnings: Vec<(String, u32, String)> =
        conn.query("SHOW WARNINGS").await.unwrap_or_default();
    if warnings
        .iter()
        .any(|(_, code, _)| *code == INCOMPLETE_ROLLBACK)
    {
        Rollback::Incomplete
    } else {
        Rollback::Complete
    }
}

impl Db {
    /// Re-`SELECT` the given just-edited rows by their (post-edit) key, so the
    /// grid can splice DB truth back in without re-running the whole query. Runs
    /// one `SELECT … LIMIT 1` per row on a fresh connection — the commit already
    /// committed, so a new connection sees the new data. Rows that no longer match
    /// (e.g. concurrently deleted) are silently skipped. Returns `(data_row,
    /// cells)` pairs, the cells aligned to `template.columns` (i.e. the result
    /// columns). Never mutates data — read-only, so it's safe outside the
    /// transactional write path.
    pub async fn refetch_rows(
        &self,
        template: &RefetchTemplate,
        rows: &[RefetchRow],
        cancel: CancellationToken,
    ) -> Result<Vec<(usize, Vec<Value>)>, DbError> {
        if rows.is_empty() {
            return Ok(Vec::new());
        }
        match self.engine {
            Engine::Postgres => return pg::refetch_rows(self, template, rows, cancel).await,
            Engine::Sqlite => return sqlite::refetch_rows(self, template, rows, cancel).await,
            Engine::MySql => {}
        }
        let mut conn = self.open(None, false).await?;
        let conn_id = conn.id();
        let outcome = tokio::select! {
            r = refetch_on(&mut conn, template, rows) => r,
            _ = cancel.cancelled() => {
                self.kill_query(conn_id).await;
                Err(DbError::Cancelled)
            }
        };
        let _ = conn.disconnect().await;
        outcome
    }
}

impl Db {
    /// Read one binary cell's bytes — the query behind the grid's binary-cell
    /// panel.
    ///
    /// The bytes of a `BLOB` are dropped at the wire on every engine (see
    /// [`schemaic_core::blob`]), so looking at one is a second, *targeted* query
    /// rather than a lookup in the loaded result. It is aimed by the same row
    /// identity a write of that row would carry, which is why a result whose
    /// binary column has no keyed base table never gets here at all —
    /// `blob_source` answers `None` and the panel is not offered.
    ///
    /// `Ok(None)` means the cell is SQL `NULL` **or** the row is gone (someone
    /// else deleted it since the result loaded); both are "there are no bytes to
    /// show", and the caller says so rather than inventing an error.
    pub async fn fetch_blob(
        &self,
        r: &BlobRef,
        cancel: CancellationToken,
    ) -> Result<Option<BlobValue>, DbError> {
        match self.engine {
            Engine::Postgres => return pg::fetch_blob(self, r, cancel).await,
            Engine::Sqlite => return sqlite::fetch_blob(self, r, cancel).await,
            Engine::MySql => {}
        }
        // `None`, not the target database: `build_blob_select` qualifies the
        // table as `db`.`table` itself, so the session default is never
        // consulted and a `USE` would be a round trip that decides nothing —
        // the same shape `refetch_rows` below already has.
        let mut conn = self.open(None, false).await?;
        let conn_id = conn.id();
        let outcome = tokio::select! {
            res = blob_on(&mut conn, r) => res,
            _ = cancel.cancelled() => {
                self.kill_query(conn_id).await;
                Err(DbError::Cancelled)
            }
        };
        let _ = conn.disconnect().await;
        outcome
    }
}

/// [`Db::fetch_blob`]'s MySQL body, on an already-open connection — so the
/// pinned connection of a manual-transaction tab can run the same read and see
/// its own uncommitted bytes.
pub(crate) async fn blob_on(conn: &mut Conn, r: &BlobRef) -> Result<Option<BlobValue>, DbError> {
    let (sql, params) = build_blob_select(r);
    let row: Option<mysql_async::Row> = conn
        .exec_first(sql, params)
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;
    let Some(row) = row else { return Ok(None) };
    // `OCTET_LENGTH(NULL)` is NULL, which is how a NULL cell arrives here.
    let Some(len) = row.get::<Option<u64>, _>(0).flatten() else {
        return Ok(None);
    };
    let bytes = row
        .get::<Option<Vec<u8>>, _>(1)
        .flatten()
        .unwrap_or_default();
    Ok(Some(BlobValue { bytes, len }))
}

/// Build the `SELECT OCTET_LENGTH(c), SUBSTRING(c, 1, ?) … WHERE <key> <=> ? …
/// LIMIT 1` behind [`blob_on`].
///
/// **The length and the bytes come from one row of one statement**, not two
/// queries: asked separately they can straddle another session's `UPDATE`, and
/// the pair is what [`BlobValue::truncated`] reads to decide whether saving the
/// buffer would write a file that is not the data.
///
/// `SUBSTRING` on a binary string is byte-indexed in MySQL (it is
/// character-indexed only for a character string), so the cap really is
/// [`FETCH_CAP`] octets. The WHERE is `build_update`'s, NULL-safe `<=>` and all
/// — the identity of a row is one thing on this path, whether it is being
/// written or read.
fn build_blob_select(r: &BlobRef) -> (String, Params) {
    let mut params: Vec<MyValue> = Vec::with_capacity(r.key.len() + 1);
    params.push(MyValue::UInt(FETCH_CAP as u64));
    let where_sql = r
        .key
        .iter()
        .map(|(col, val)| {
            params.push(value_to_param(val));
            format!("{} <=> ?", ident(col))
        })
        .collect::<Vec<_>>()
        .join(" AND ");
    let col = ident(&r.column);
    let sql = format!(
        "SELECT OCTET_LENGTH({col}), SUBSTRING({col}, 1, ?) FROM {}.{} WHERE {where_sql} LIMIT 1",
        ident(&r.database),
        ident(&r.table),
    );
    (sql, Params::Positional(params))
}

/// Re-`SELECT` just-edited rows on an already-open connection. Read-only, so it
/// is safe both on a fresh connection and inside an open transaction — and
/// inside one it is *required*, since only that connection can see the
/// uncommitted rows it just wrote.
pub(crate) async fn refetch_on(
    conn: &mut Conn,
    template: &RefetchTemplate,
    rows: &[RefetchRow],
) -> Result<Vec<(usize, Vec<Value>)>, DbError> {
    let sql = build_refetch_sql(template);
    let qerr = |e: mysql_async::Error| DbError::Query(e.to_string());
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let params: Vec<MyValue> = row.key.iter().map(value_to_param).collect();
        let mut result = conn
            .exec_iter(sql.as_str(), Params::Positional(params))
            .await
            .map_err(qerr)?;
        // Column metadata (owned) before consuming the result stream.
        let columns: Vec<Column> = result.columns_ref().iter().map(map_column).collect();
        let fetched: Vec<Row> = result.collect::<Row>().await.map_err(qerr)?;
        if let Some(r) = fetched.first() {
            let binary: Vec<bool> = columns.iter().map(Column::is_binary).collect();
            let bit: Vec<bool> = columns
                .iter()
                .map(|c| schemaic_core::model::type_is_bit(&c.type_name))
                .collect();
            out.push((row.data_row, convert_row(r, &columns, &binary, &bit)));
        }
    }
    Ok(out)
}

/// One staged cell value as a MySQL bound parameter.
///
/// `Text` and `Bytes` both become `MyValue::Bytes` — the wire has one
/// length-prefixed octet-string and the server coerces it to the column type —
/// but they arrive there by different routes and only one of them is reversible:
/// `Text` is the user's characters encoded as UTF-8, `Bytes` is the octets
/// themselves, unencoded. Collapsing the two at the *call site* is what would
/// hurt, because `String::into_bytes` on a lossily-decoded blob is not the blob.
fn cell_param(v: &CellEdit) -> MyValue {
    match v {
        CellEdit::Text(t) => MyValue::Bytes(t.clone().into_bytes()),
        CellEdit::Bytes(b) => MyValue::Bytes(b.to_vec()),
        CellEdit::Null => MyValue::NULL,
    }
}

/// Build a parameterized `UPDATE db.table SET … WHERE …` for one row edit.
/// Identifiers are backtick-escaped; every value is a bound parameter.
fn build_update(edit: &RowEdit) -> (String, Params) {
    let mut params: Vec<MyValue> = Vec::with_capacity(edit.set.len() + edit.key.len());
    let set_sql = edit
        .set
        .iter()
        .map(|(col, val)| {
            params.push(cell_param(val));
            format!("{} = ?", ident(col))
        })
        .collect::<Vec<_>>()
        .join(", ");
    let where_sql = edit
        .key
        .iter()
        .map(|(col, val)| {
            params.push(value_to_param(val));
            // NULL-safe equality so a NULL key value matches (plain `= NULL`
            // never does). Float/binary keys are excluded upstream in
            // `resolve_key`, where they can't be matched reliably at all.
            format!("{} <=> ?", ident(col))
        })
        .collect::<Vec<_>>()
        .join(" AND ");
    let sql = format!(
        "UPDATE {}.{} SET {set_sql} WHERE {where_sql}",
        ident(&edit.database),
        ident(&edit.table),
    );
    (sql, Params::Positional(params))
}

/// Build a parameterized `INSERT INTO db.table (cols) VALUES (?, …)` for one new
/// row. Identifiers are backtick-escaped; every value is a bound parameter — see
/// [`cell_param`]. Columns not listed take their server default — with none
/// listed, `() VALUES ()` inserts an all-defaults row.
fn build_insert(ins: &RowInsert) -> (String, Params) {
    let mut params: Vec<MyValue> = Vec::with_capacity(ins.cols.len());
    let cols_sql = ins
        .cols
        .iter()
        .map(|(col, val)| {
            params.push(cell_param(val));
            ident(col)
        })
        .collect::<Vec<_>>()
        .join(", ");
    let placeholders = vec!["?"; ins.cols.len()].join(", ");
    let sql = format!(
        "INSERT INTO {}.{} ({cols_sql}) VALUES ({placeholders})",
        ident(&ins.database),
        ident(&ins.table),
    );
    (sql, Params::Positional(params))
}

/// Build a parameterized `DELETE FROM db.table WHERE …` for one row, keyed by its
/// identity (NULL-safe `<=>` per key column, like `build_update`'s WHERE). Every
/// value is a bound parameter.
fn build_delete(del: &RowDelete) -> (String, Params) {
    let mut params: Vec<MyValue> = Vec::with_capacity(del.key.len());
    let where_sql = del
        .key
        .iter()
        .map(|(col, val)| {
            params.push(value_to_param(val));
            format!("{} <=> ?", ident(col))
        })
        .collect::<Vec<_>>()
        .join(" AND ");
    let sql = format!(
        "DELETE FROM {}.{} WHERE {where_sql}",
        ident(&del.database),
        ident(&del.table),
    );
    (sql, Params::Positional(params))
}

/// Build the `SELECT … WHERE <key> <=> ? … LIMIT 1` used to re-fetch one edited
/// row after a commit. Identifiers are backtick-escaped; the key columns become
/// positional NULL-safe placeholders (bound by the caller from each row's key,
/// in `template.key_cols` order). Pure so the SQL shape is unit-tested.
fn build_refetch_sql(template: &RefetchTemplate) -> String {
    let cols_sql = template
        .columns
        .iter()
        .map(|c| ident(c))
        .collect::<Vec<_>>()
        .join(", ");
    let where_sql = template
        .key_cols
        .iter()
        .map(|&kci| format!("{} <=> ?", ident(&template.columns[kci])))
        .collect::<Vec<_>>()
        .join(" AND ");
    format!(
        "SELECT {cols_sql} FROM {}.{} WHERE {where_sql} LIMIT 1",
        ident(&template.database),
        ident(&template.table),
    )
}

/// Backtick-quote an identifier, doubling any embedded backtick.
///
/// The one identifier-quoting rule, pinned to this path's only engine — these
/// statements are built for MySQL by construction (the PostgreSQL write path is
/// `pg.rs`'s).
fn ident(name: &str) -> String {
    schemaic_core::export::ident_sql(name, schemaic_core::intel::SqlDialect::MySql)
}

/// Double-quote an identifier for SQLite, doubling any embedded double-quote.
///
/// The same thin delegation as [`ident`], pinned to the other engine this file
/// builds statements for. SQLite would also accept backticks or brackets, but
/// what it *emits* is the standard form for the reason
/// [`schemaic_core::export::ident_sql`] gives: `"` is the only one of the three
/// with a defined escape.
pub(crate) fn ident_sqlite(name: &str) -> String {
    schemaic_core::export::ident_sql(name, schemaic_core::intel::SqlDialect::Sqlite)
}

/// Convert a typed cell value into a bound parameter for a `WHERE` comparison.
fn value_to_param(v: &Value) -> MyValue {
    match v {
        Value::Null => MyValue::NULL,
        Value::Int(i) => MyValue::Int(*i),
        Value::UInt(u) => MyValue::UInt(*u),
        Value::Float(f) => MyValue::Double(*f),
        Value::Str(s) => MyValue::Bytes(s.clone().into_bytes()),
    }
}

/// Which numeric variant a column's text cells parse into — the whole of
/// [`parse_typed`]'s decision, and a **per-column** fact.
///
/// Split out so a row loop can ask the type name once per column instead of once
/// per cell. The question is a `to_ascii_uppercase()` (a heap allocation), six
/// `starts_with` probes and, for integers, a `contains("UNSIGNED")` scan — which
/// is nothing at all per column and 100M allocations on a 5M × 20 export. The
/// neighbouring per-column answer was hoisted for exactly this reason
/// (`pg::cell_kinds`' doc, and `type_is_binary` before it); this is the other
/// half of the same `match`, and `f115e51` removed the row cap that used to bound
/// it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NumKind {
    Int,
    UInt,
    Float,
    /// Not numeric — the cell keeps its exact text, which is most columns.
    Text,
}

/// [`NumKind`] for a column's declared type. Called once per column.
pub(crate) fn num_kind(type_name: &str) -> NumKind {
    let t = type_name.to_ascii_uppercase();
    let is_integer = ["TINYINT", "SMALLINT", "MEDIUMINT", "INT", "BIGINT", "YEAR"]
        .iter()
        .any(|k| t.starts_with(k));
    if is_integer {
        return if t.contains("UNSIGNED") {
            NumKind::UInt
        } else {
            NumKind::Int
        };
    }
    if t.starts_with("FLOAT") || t.starts_with("DOUBLE") {
        return NumKind::Float;
    }
    NumKind::Text
}

/// Parse a text-protocol cell into a typed [`Value`], given its column's
/// [`NumKind`]. Any parse failure falls back to the string — never lossy.
pub(crate) fn parse_as(kind: NumKind, s: String) -> Value {
    match kind {
        NumKind::UInt => s.parse::<u64>().map(Value::UInt).unwrap_or(Value::Str(s)),
        NumKind::Int => s.parse::<i64>().map(Value::Int).unwrap_or(Value::Str(s)),
        NumKind::Float => s.parse::<f64>().map(Value::Float).unwrap_or(Value::Str(s)),
        NumKind::Text => Value::Str(s),
    }
}

/// Parse a text-protocol cell into a typed [`Value`] using the column's SQL
/// type. Integers/floats become compact numeric variants; anything else stays
/// an exact string. Any parse failure falls back to the string — never lossy.
///
/// The composition of [`num_kind`] and [`parse_as`], and *only* that: the two
/// cannot drift apart from the answer this function gives, because this function
/// is them. What a row loop should call is `parse_as` with a kind it computed
/// once; this spelling is for the callers with one cell to convert.
pub(crate) fn parse_typed(s: String, type_name: &str) -> Value {
    parse_as(num_kind(type_name), s)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── The server-level DDL runner ───────────────────────────────────────

    /// The runner for the two most destructive statements this app emits had
    /// **no test at all**. Two of its decisions need no server and are three
    /// lines each under the house rule, so they are here.
    ///
    /// The SQLite arm returns before any I/O: a database there is a file, and
    /// the refusal has to be a message rather than an invented filesystem
    /// action. `supports_database_editing` refuses the change long before this,
    /// so reaching here at all means something upstream let it through — which
    /// is exactly when a backstop earns its place.
    #[tokio::test]
    async fn sqlite_refuses_server_level_ddl_without_touching_anything() {
        let db = Db::from_parts(
            Engine::Sqlite,
            String::new(),
            0,
            String::new(),
            String::new(),
            "file:server_ddl_test?mode=memory&cache=shared".to_string(),
        );
        let err = db
            .run_server_ddl(
                None,
                &["CREATE DATABASE shop;".to_string()],
                CancellationToken::new(),
            )
            .await
            .expect_err("SQLite has no databases to create");
        assert!(err.message.contains("file"), "{}", err.message);
        assert_eq!(err.applied, 0, "nothing ran");
    }

    /// And an empty plan is a no-op on every engine — it must not open a
    /// connection to find that out. Asserted on SQLite, where a connect would
    /// otherwise be the one thing that *could* succeed and so would hide the
    /// early return.
    #[tokio::test]
    async fn an_empty_server_level_plan_runs_nothing() {
        let db = Db::from_parts(
            Engine::Sqlite,
            String::new(),
            0,
            String::new(),
            String::new(),
            "/nonexistent/path/that/cannot/be/opened.db".to_string(),
        );
        assert!(
            db.run_server_ddl(None, &[], CancellationToken::new())
                .await
                .is_ok()
        );
    }

    // ── The connection's own database ─────────────────────────────────────

    /// **`open(None)` and "no database at all" must not be the same
    /// spelling.** A `DROP DATABASE shop` on a connection configured for `shop`
    /// went out on a session pointed at its own target; every later operation
    /// then answered `ERROR 1049`.
    ///
    /// Asserted through the options builder, which is where the two readings
    /// diverge — the connect itself needs a server.
    #[test]
    fn a_server_level_connection_names_no_database_even_when_one_is_configured() {
        let db = Db::from_parts(
            Engine::MySql,
            "h".into(),
            3306,
            "u".into(),
            "p".into(),
            String::new(),
        )
        .with_database(Some("shop"));

        let named = mysql_async::Opts::from(db.opts(Scope::Database(None), false));
        assert_eq!(
            named.db_name(),
            Some("shop"),
            "an unnamed database still falls back to the connection's"
        );

        let server = mysql_async::Opts::from(db.opts(Scope::Server, false));
        assert_eq!(
            server.db_name(),
            None,
            "a server-level connection must not be filled in from the connection"
        );

        // And a caller that named one is never redirected.
        let explicit = mysql_async::Opts::from(db.opts(Scope::Database(Some("other")), false));
        assert_eq!(explicit.db_name(), Some("other"));
    }

    /// The other end of the same conflation: an unopenable configured database
    /// must not take out the listing that would let the user fix it. This is
    /// the classification the retry hangs on — narrow enough that a real
    /// credential or network failure is still reported.
    #[test]
    fn only_an_unopenable_database_is_worth_a_second_connect() {
        assert!(unknown_database(&DbError::Connect(
            "Server error: `ERROR 1049 (42000): Unknown database 'wolrd''".into()
        )));
        assert!(unknown_database(&DbError::Connect(
            "Unknown database 'wolrd'".into()
        )));

        assert!(!unknown_database(&DbError::Connect(
            "Access denied for user 'app'@'localhost' (using password: YES)".into()
        )));
        assert!(!unknown_database(&DbError::Connect(
            "Connection refused (os error 111)".into()
        )));
        // Only a *connect* failure; a query that mentions the words is not one.
        assert!(!unknown_database(&DbError::Query(
            "Unknown database 'x'".into()
        )));
    }

    // ── The plaintext retry ───────────────────────────────────────────────

    fn plan(mode: schemaic_core::connection::SslMode) -> schemaic_core::connection::TlsPlan {
        schemaic_core::connection::Tls {
            mode,
            ..Default::default()
        }
        .plan()
        .expect("every mode above Disable handshakes")
    }

    /// **The retry is for a server with no TLS, not for any failure at all.**
    /// Its only condition was `plan.fallback_to_plaintext`, so `prefer` retried
    /// after a wrong password (twelve connect attempts for ten pings, measured)
    /// and after anything an attacker can provoke mid-handshake — one injected
    /// RST and the whole operation continues in cleartext.
    #[test]
    fn prefer_falls_back_only_when_the_server_says_it_has_no_tls() {
        use mysql_async::{DriverError, Error};
        let prefer = plan(schemaic_core::connection::SslMode::Prefer);

        assert!(should_retry_plaintext(
            &prefer,
            &Error::Driver(DriverError::NoClientSslFlagFromServer)
        ));

        // Everything else is a real failure to report, not a reason to
        // downgrade. `ConnectionClosed` stands in for the whole class an
        // attacker can force by cutting the handshake.
        assert!(!should_retry_plaintext(
            &prefer,
            &Error::Driver(DriverError::ConnectionClosed)
        ));
        assert!(!should_retry_plaintext(
            &prefer,
            &Error::Io(mysql_async::IoError::Io(std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                "reset"
            )))
        ));
    }

    /// And no mode above `prefer` retries at all, whatever the error — offering
    /// the second attempt to `require` would turn the strongest half of the
    /// setting into the weakest while still reporting success.
    #[test]
    fn no_verifying_mode_ever_retries_in_plaintext() {
        use mysql_async::{DriverError, Error};
        use schemaic_core::connection::SslMode;
        for mode in [SslMode::Require, SslMode::VerifyCa, SslMode::VerifyFull] {
            assert!(
                !should_retry_plaintext(
                    &plan(mode),
                    &Error::Driver(DriverError::NoClientSslFlagFromServer)
                ),
                "{mode:?}"
            );
        }
    }

    // ── The MySQL statistics half ─────────────────────────────────────────
    //
    // Three decisions with wrong answers that produce a plausible-looking panel
    // rather than an error: reading a cardinality per key *position* instead of
    // per index, calling the primary key an ordinary index, and turning "nobody
    // counted the scans" into "zero scans" — which is what marks an index for
    // deletion.

    /// `information_schema.STATISTICS` has one row per key position, each with the
    /// cardinality of the prefix ending there. The index's own figure is the last
    /// one, so the query has to group and take `MAX`: reading a row instead would
    /// report `(status, created_at)`'s handful of statuses as the whole index's
    /// distinct count.
    #[test]
    fn the_cardinality_query_takes_the_index_and_not_one_key_position() {
        assert!(MY_INDEX_CARDINALITY_SQL.contains("MAX(CARDINALITY)"));
        assert!(MY_INDEX_CARDINALITY_SQL.contains("GROUP BY TABLE_NAME, INDEX_NAME"));
        // `NON_UNIQUE` is constant within the group; `MIN` is how it survives the
        // grouping rather than an aggregate that means anything.
        assert!(MY_INDEX_CARDINALITY_SQL.contains("MIN(NON_UNIQUE)"));
    }

    /// The usage view carries a row for the **table** as well as its indexes, with
    /// a NULL index name. Counted, it would appear as an index nobody can find.
    #[test]
    fn the_usage_query_skips_the_tables_own_row() {
        assert!(MY_INDEX_USAGE_SQL.contains("INDEX_NAME IS NOT NULL"));
        assert!(MY_INDEX_USAGE_SQL.contains("OBJECT_SCHEMA = ?"));
    }

    fn stat_row(name: &str) -> MyStatRow {
        (
            name.to_string(),
            Some(4_213_551),
            Some(1024),
            Some(512),
            None,
            None,
            None,
            Some("InnoDB".to_string()),
            None,
            None,
        )
    }

    /// The mapping's three rules at once: indexes land on their own table, a key
    /// is unique because it is the key, and an index Performance Schema said
    /// nothing about reports **no** scan count rather than zero.
    #[test]
    fn the_mapping_keeps_a_missing_scan_count_absent() {
        let idx = vec![
            ("orders".into(), "PRIMARY".into(), Some(4_000_000), Some(0)),
            (
                "orders".into(),
                "idx_email".into(),
                Some(3_996_120),
                Some(0),
            ),
            ("orders".into(), "idx_status".into(), Some(7), Some(1)),
            ("other".into(), "PRIMARY".into(), Some(1), Some(0)),
        ];
        let usage: HashMap<(String, String), u64> =
            [(("orders".to_string(), "idx_email".to_string()), 12)].into();
        let stats = map_mysql_stats(
            vec![stat_row("orders"), stat_row("other")],
            &idx,
            &usage,
            Freshness::Unknown,
        );

        let orders = stats.find(None, "orders").expect("orders");
        assert_eq!(orders.indexes.len(), 3, "the other table's key is not here");
        let by = |n: &str| {
            orders
                .indexes
                .iter()
                .find(|i| i.name == n)
                .unwrap_or_else(|| panic!("{n}"))
        };
        assert_eq!(by("idx_email").scans, Some(12));
        // The two nobody reported: absent, so `is_unused` cannot flag them.
        assert_eq!(by("idx_status").scans, None);
        assert!(!by("idx_status").is_unused(), "not counted is not unused");
        assert!(by("PRIMARY").is_primary && by("PRIMARY").is_unique);
        assert!(by("idx_email").is_unique, "NON_UNIQUE = 0");
        assert!(!by("idx_status").is_unique, "NON_UNIQUE = 1");
        // MySQL reports one `INDEX_LENGTH` for the whole table, so no index here
        // may claim a size of its own.
        assert!(orders.indexes.iter().all(|i| i.bytes.is_none()));
        // And the cardinality is carried, marked as the estimate it is.
        assert_eq!(
            by("idx_email").cardinality_label().as_deref(),
            Some("~4m"),
            "printed as an estimate, not as 3,996,120"
        );
    }

    /// A table with no rows in `STATISTICS` — a view, or a table whose grants hide
    /// it — still gets its entry, with no indexes rather than none of it.
    #[test]
    fn the_mapping_keeps_a_table_with_no_indexes() {
        let stats = map_mysql_stats(
            vec![stat_row("v")],
            &[],
            &HashMap::new(),
            Freshness::Unknown,
        );
        let v = stats.find(None, "v").expect("v");
        assert!(v.indexes.is_empty());
        assert_eq!(v.rows, Some(4_213_551));
    }

    #[test]
    fn build_insert_sql_shapes() {
        // Normal insert: listed columns → backtick-quoted names + placeholders.
        let ins = RowInsert {
            database: "db".to_string(),
            schema: None,
            table: "users".to_string(),
            cols: vec![
                ("name".to_string(), CellEdit::Text("Ada".to_string())),
                ("email".to_string(), CellEdit::Null), // explicit NULL
            ],
        };
        let (sql, _) = build_insert(&ins);
        assert_eq!(
            sql,
            "INSERT INTO `db`.`users` (`name`, `email`) VALUES (?, ?)"
        );

        // All-defaults insert (no columns set) → `() VALUES ()`.
        let empty = RowInsert {
            database: "db".to_string(),
            schema: None,
            table: "t".to_string(),
            cols: vec![],
        };
        let (sql, _) = build_insert(&empty);
        assert_eq!(sql, "INSERT INTO `db`.`t` () VALUES ()");

        // Identifiers with backticks are doubled.
        let weird = RowInsert {
            database: "d`b".to_string(),
            schema: None,
            table: "t".to_string(),
            cols: vec![("a`b".to_string(), CellEdit::Text("x".to_string()))],
        };
        let (sql, _) = build_insert(&weird);
        assert_eq!(sql, "INSERT INTO `d``b`.`t` (`a``b`) VALUES (?)");
    }

    #[test]
    fn build_delete_sql_shape() {
        // NULL-safe equality per key column (composite key joins with AND).
        let del = RowDelete {
            database: "db".to_string(),
            schema: None,
            table: "users".to_string(),
            key: vec![
                ("id".to_string(), Value::Int(7)),
                ("tenant".to_string(), Value::Str("acme".to_string())),
            ],
        };
        let (sql, _) = build_delete(&del);
        assert_eq!(
            sql,
            "DELETE FROM `db`.`users` WHERE `id` <=> ? AND `tenant` <=> ?"
        );
    }

    fn positional(p: &Params) -> &[MyValue] {
        match p {
            Params::Positional(v) => v.as_slice(),
            _ => panic!("expected positional params"),
        }
    }

    #[test]
    fn build_update_sql_and_param_order() {
        // SET params come first (in column order), then WHERE key params.
        let edit = RowEdit {
            database: "db".to_string(),
            schema: None,
            table: "users".to_string(),
            set: vec![
                ("name".to_string(), CellEdit::Text("Ada".to_string())),
                ("nickname".to_string(), CellEdit::Null), // set to NULL
            ],
            key: vec![("id".to_string(), Value::Int(7))],
        };
        let (sql, params) = build_update(&edit);
        assert_eq!(
            sql,
            "UPDATE `db`.`users` SET `name` = ?, `nickname` = ? WHERE `id` <=> ?"
        );
        let p = positional(&params);
        assert_eq!(p.len(), 3);
        assert!(matches!(&p[0], MyValue::Bytes(b) if b == b"Ada"));
        assert!(matches!(p[1], MyValue::NULL));
        assert!(matches!(p[2], MyValue::Int(7)));
    }

    /// **Bytes bind as bytes, and the two shapes are not the same param.**
    /// `MyValue::Bytes` is the wire shape both take, which is exactly why this
    /// is worth pinning: `Text` reaches it through `String::into_bytes` (UTF-8
    /// encoding the user's characters) and `Bytes` reaches it unencoded, so the
    /// two agree on every ASCII fixture and diverge on the first byte a blob
    /// actually contains. The fixture is a PNG header for that reason — `0x89`
    /// is not valid UTF-8 on its own, so a `Bytes` value that had gone through
    /// the text arm could not have arrived intact.
    #[test]
    fn build_update_binds_bytes_unencoded_next_to_a_text_column() {
        let png = vec![0x89u8, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        let edit = RowEdit {
            database: "sakila".to_string(),
            schema: None,
            table: "staff".to_string(),
            set: vec![
                ("first_name".to_string(), CellEdit::Text("Ada".to_string())),
                ("picture".to_string(), CellEdit::bytes(png.clone())),
                ("last_name".to_string(), CellEdit::Null),
            ],
            key: vec![("staff_id".to_string(), Value::Int(1))],
        };
        let (sql, params) = build_update(&edit);
        assert_eq!(
            sql,
            "UPDATE `sakila`.`staff` SET `first_name` = ?, `picture` = ?, `last_name` = ? \
             WHERE `staff_id` <=> ?"
        );
        let p = positional(&params);
        assert_eq!(p.len(), 4);
        assert!(matches!(&p[0], MyValue::Bytes(b) if b == b"Ada"));
        assert!(
            matches!(&p[1], MyValue::Bytes(b) if *b == png),
            "the blob's own octets, not a re-encoding of them"
        );
        assert!(matches!(p[2], MyValue::NULL));
        assert!(matches!(p[3], MyValue::Int(1)));
    }

    /// The same for an `INSERT` — a new row can carry a file too, and the
    /// `VALUES` list binds in column order.
    #[test]
    fn build_insert_binds_bytes_in_column_order() {
        let ins = RowInsert {
            database: "db".to_string(),
            schema: None,
            table: "docs".to_string(),
            cols: vec![
                (
                    "payload".to_string(),
                    CellEdit::bytes(vec![0xFF, 0x00, 0xFE]),
                ),
                ("title".to_string(), CellEdit::Text("x".to_string())),
            ],
        };
        let (sql, params) = build_insert(&ins);
        assert_eq!(
            sql,
            "INSERT INTO `db`.`docs` (`payload`, `title`) VALUES (?, ?)"
        );
        let p = positional(&params);
        assert!(matches!(&p[0], MyValue::Bytes(b) if *b == vec![0xFFu8, 0x00, 0xFE]));
        assert!(matches!(&p[1], MyValue::Bytes(b) if b == b"x"));
    }

    /// An empty file is a value, not an absence: zero bytes bind as a zero-length
    /// param, which MySQL stores as an empty blob. `NULL` is the other thing, and
    /// the two must not collapse — a `NOT NULL BLOB` column accepts the first and
    /// rejects the second.
    #[test]
    fn zero_bytes_is_an_empty_blob_and_not_null() {
        let ins = RowInsert {
            database: "db".to_string(),
            schema: None,
            table: "docs".to_string(),
            cols: vec![("payload".to_string(), CellEdit::bytes(Vec::new()))],
        };
        let (_, params) = build_insert(&ins);
        let p = positional(&params);
        assert!(matches!(&p[0], MyValue::Bytes(b) if b.is_empty()));
        assert!(!matches!(p[0], MyValue::NULL));
    }

    #[test]
    fn build_update_escapes_backtick_identifiers() {
        let edit = RowEdit {
            database: "d`b".to_string(),
            schema: None,
            table: "t`t".to_string(),
            set: vec![("a`b".to_string(), CellEdit::Text("x".to_string()))],
            key: vec![("k`k".to_string(), Value::Int(1))],
        };
        let (sql, _) = build_update(&edit);
        assert_eq!(
            sql,
            "UPDATE `d``b`.`t``t` SET `a``b` = ? WHERE `k``k` <=> ?"
        );
    }

    #[test]
    fn ident_doubles_embedded_backticks() {
        assert_eq!(ident("plain"), "`plain`");
        assert_eq!(ident("a`b"), "`a``b`");
        // Two backticks → each doubled (four), wrapped → six backticks.
        assert_eq!(ident("``"), "`".repeat(6));
    }

    #[test]
    fn value_to_param_maps_each_variant() {
        assert!(matches!(value_to_param(&Value::Null), MyValue::NULL));
        assert!(matches!(value_to_param(&Value::Int(-3)), MyValue::Int(-3)));
        assert!(matches!(value_to_param(&Value::UInt(3)), MyValue::UInt(3)));
        assert!(matches!(value_to_param(&Value::Float(1.5)), MyValue::Double(f) if f == 1.5));
        assert!(matches!(value_to_param(&Value::Str("s".into())), MyValue::Bytes(b) if b == b"s"));
    }

    #[test]
    fn parse_typed_integers_unsigned_floats_and_fallback() {
        // Signed integer types.
        assert!(matches!(parse_typed("42".into(), "INT"), Value::Int(42)));
        assert!(matches!(parse_typed("-1".into(), "BIGINT"), Value::Int(-1)));
        assert!(matches!(
            parse_typed("2024".into(), "YEAR"),
            Value::Int(2024)
        ));
        // Unsigned.
        assert!(matches!(
            parse_typed("42".into(), "INT UNSIGNED"),
            Value::UInt(42)
        ));
        // A negative into an UNSIGNED column can't parse → lossless string fallback.
        assert!(matches!(
            parse_typed("-1".into(), "INT UNSIGNED"),
            Value::Str(s) if s == "-1"
        ));
        // Floats.
        assert!(matches!(parse_typed("1.5".into(), "DOUBLE"), Value::Float(f) if f == 1.5));
        assert!(matches!(parse_typed("3.0".into(), "FLOAT"), Value::Float(f) if f == 3.0));
        // DECIMAL stays an exact string (never a lossy float).
        assert!(matches!(
            parse_typed("1.10".into(), "DECIMAL(10,2)"),
            Value::Str(s) if s == "1.10"
        ));
        // Non-numeric type → string.
        assert!(matches!(
            parse_typed("hi".into(), "VARCHAR(20)"),
            Value::Str(s) if s == "hi"
        ));
        // Unparseable integer → string fallback, never a panic.
        assert!(matches!(
            parse_typed("NaN".into(), "INT"),
            Value::Str(s) if s == "NaN"
        ));
    }

    #[test]
    fn db_connect_rewrites_endpoint_for_tunnel() {
        let conn = schemaic_core::connection::Connection {
            id: 1,
            name: "c".to_string(),
            db_type: "MySQL".to_string(),
            host: "remote.example".to_string(),
            port: 3306,
            user: "u".to_string(),
            password: "p".to_string(),
            file: String::new(),
            database: String::new(),
            ssh: Default::default(),
            tls: Default::default(),
            color: None,
            prominent_color: false,
            read_only: false,
            environment: Default::default(),
            ai_data: None,
        };
        // No tunnel → direct host/port passthrough.
        let direct = Db::connect(&conn, None);
        assert_eq!(direct.parts(), ("remote.example", 3306, "u", "p", ""));
        // Tunnel → rewritten to 127.0.0.1:<local port>, credentials preserved.
        let tunneled = Db::connect(&conn, Some(55001));
        assert_eq!(tunneled.parts(), ("127.0.0.1", 55001, "u", "p", ""));
    }

    /// **The name a tunnelled connection verifies against is still the far
    /// end's**, and this test exists because the one above could not see it:
    /// it builds `tls: Default::default()`, which is `Disable`, so `tls_plan()`
    /// is `None` and the mapping branch is never entered. Deleting
    /// `hostname_override` left the whole suite green while `verify-full`
    /// through a tunnel compared a perfectly good certificate against
    /// `127.0.0.1` and rejected it — the mode that most wants to work through a
    /// bastion being the one that cannot.
    ///
    /// Asserted for **every mode that handshakes**, because the override rides
    /// on the plan and a mode-specific answer here would be a mode-specific
    /// failure at a customer's bastion.
    #[test]
    fn a_tunnel_moves_the_address_and_keeps_the_name_to_verify() {
        use schemaic_core::connection::{SslMode, Tls};
        for mode in SslMode::ALL {
            let conn = schemaic_core::connection::Connection {
                id: 1,
                name: "c".to_string(),
                db_type: "PostgreSQL".to_string(),
                host: "remote.example".to_string(),
                port: 5432,
                user: "u".to_string(),
                password: "p".to_string(),
                file: String::new(),
                database: String::new(),
                ssh: Default::default(),
                tls: Tls {
                    mode,
                    ..Tls::default()
                },
                color: None,
                prominent_color: false,
                read_only: false,
                environment: Default::default(),
                ai_data: None,
            };

            let direct = Db::connect(&conn, None);
            assert!(
                direct
                    .tls_plan()
                    .is_none_or(|p| p.hostname_override.is_none()),
                "{mode:?}: an untunnelled connection dials the name it verifies"
            );

            let tunneled = Db::connect(&conn, Some(55001));
            assert_eq!(tunneled.parts().0, "127.0.0.1", "{mode:?}");
            match tunneled.tls_plan() {
                // `Disable` never handshakes, so there is nothing to verify.
                None => assert_eq!(mode, SslMode::Disable, "{mode:?} should have a plan"),
                Some(plan) => assert_eq!(
                    plan.hostname_override.as_deref(),
                    Some("remote.example"),
                    "{mode:?}: the address moved and the name did not come with it"
                ),
            }
        }
    }

    /// A SQLite connection's target is its file, and **a tunnel port must not
    /// repoint it**. Nothing should open a tunnel for one in the first place
    /// (`Engine::is_networked`), but a rewrite to `127.0.0.1:<port>` there would
    /// silently swap which database the app is talking to, so the rewrite is
    /// skipped by engine rather than by trusting every caller.
    #[test]
    fn a_tunnel_port_cannot_repoint_a_sqlite_file() {
        let conn = schemaic_core::connection::Connection {
            id: 1,
            name: "c".to_string(),
            db_type: "SQLite".to_string(),
            host: "ignored".to_string(),
            port: 0,
            user: String::new(),
            password: String::new(),
            file: "/data/app.db".to_string(),
            database: String::new(),
            ssh: Default::default(),
            tls: Default::default(),
            color: None,
            prominent_color: false,
            read_only: false,
            environment: Default::default(),
            ai_data: None,
        };
        let db = Db::connect(&conn, Some(55001));
        assert_eq!(db.engine(), Engine::Sqlite);
        assert_eq!(db.file(), "/data/app.db");
        assert!(!db.engine().is_networked());
        // The coordinates are carried untouched rather than rewritten.
        assert_eq!(db.parts().0, "ignored");
    }

    #[test]
    fn db_from_parts_roundtrips() {
        let db = Db::from_parts(
            Engine::Postgres,
            "h".into(),
            3307,
            "user".into(),
            "pass".into(),
            String::new(),
        );
        assert_eq!(db.parts(), ("h", 3307, "user", "pass", ""));
        assert_eq!(db.engine(), Engine::Postgres);
        // The file rides the endpoint too, or the MCP subprocess gets an engine
        // it can't reach anything with.
        let lite = Db::from_parts(
            Engine::Sqlite,
            String::new(),
            0,
            String::new(),
            String::new(),
            "/data/app.db".into(),
        );
        assert_eq!(lite.parts().4, "/data/app.db");
    }

    #[test]
    fn build_refetch_sql_single_key() {
        let t = RefetchTemplate {
            database: "db".to_string(),
            schema: None,
            table: "users".to_string(),
            columns: vec!["id".to_string(), "name".to_string()],
            key_cols: vec![0],
        };
        assert_eq!(
            build_refetch_sql(&t),
            "SELECT `id`, `name` FROM `db`.`users` WHERE `id` <=> ? LIMIT 1"
        );
    }

    #[test]
    fn build_refetch_sql_composite_key_joins_with_and() {
        let t = RefetchTemplate {
            database: "db".to_string(),
            schema: None,
            table: "t".to_string(),
            columns: vec!["a".to_string(), "b".to_string(), "c".to_string()],
            key_cols: vec![0, 2],
        };
        assert_eq!(
            build_refetch_sql(&t),
            "SELECT `a`, `b`, `c` FROM `db`.`t` WHERE `a` <=> ? AND `c` <=> ? LIMIT 1"
        );
    }

    #[test]
    fn build_refetch_sql_escapes_identifiers() {
        let t = RefetchTemplate {
            database: "d`b".to_string(),
            schema: None,
            table: "t`t".to_string(),
            columns: vec!["a`b".to_string()],
            key_cols: vec![0],
        };
        assert_eq!(
            build_refetch_sql(&t),
            "SELECT `a``b` FROM `d``b`.`t``t` WHERE `a``b` <=> ? LIMIT 1"
        );
    }

    fn blob_ref(key: &[(&str, Value)]) -> BlobRef {
        BlobRef {
            database: "db".to_string(),
            schema: None,
            table: "staff".to_string(),
            column: "picture".to_string(),
            key: key
                .iter()
                .map(|(c, v)| (c.to_string(), v.clone()))
                .collect(),
        }
    }

    /// **The cap binds before the key.** The `SUBSTRING` placeholder sits in the
    /// select list and every key placeholder in the `WHERE` after it, so the
    /// parameter vector has to be built in that order — reversed, MySQL reads
    /// the row's id as a byte count and the key as a length, and the statement
    /// still runs.
    #[test]
    fn build_blob_select_binds_the_cap_first_then_the_key() {
        let (sql, params) = build_blob_select(&blob_ref(&[("staff_id", Value::UInt(1))]));
        assert_eq!(
            sql,
            "SELECT OCTET_LENGTH(`picture`), SUBSTRING(`picture`, 1, ?) \
             FROM `db`.`staff` WHERE `staff_id` <=> ? LIMIT 1"
        );
        let Params::Positional(p) = params else {
            panic!("positional params expected");
        };
        assert_eq!(p.len(), 2);
        assert_eq!(p[0], MyValue::UInt(FETCH_CAP as u64));
        assert_eq!(p[1], MyValue::UInt(1));
    }

    /// A composite key joins with `AND`, in `row_key` order — the same WHERE
    /// `build_update` builds, because it is the same row identity.
    #[test]
    fn build_blob_select_joins_a_composite_key_with_and() {
        let (sql, params) = build_blob_select(&blob_ref(&[
            ("a", Value::Int(1)),
            ("b", Value::Str("x".to_string())),
        ]));
        assert!(
            sql.ends_with("WHERE `a` <=> ? AND `b` <=> ? LIMIT 1"),
            "{sql}"
        );
        let Params::Positional(p) = params else {
            panic!("positional params expected");
        };
        assert_eq!(p.len(), 3, "cap + two key values");
    }

    /// A NULL key value still compares, because the WHERE is NULL-safe — a
    /// plain `= NULL` would silently match no row and report the cell empty.
    #[test]
    fn build_blob_select_keeps_the_null_safe_comparison() {
        let (sql, _) = build_blob_select(&blob_ref(&[("k", Value::Null)]));
        assert!(sql.contains("`k` <=> ?"), "{sql}");
    }

    #[test]
    fn build_blob_select_escapes_every_identifier() {
        let r = BlobRef {
            database: "d`b".to_string(),
            schema: None,
            table: "t`t".to_string(),
            column: "c`c".to_string(),
            key: vec![("k`k".to_string(), Value::Int(1))],
        };
        let (sql, _) = build_blob_select(&r);
        assert_eq!(
            sql,
            "SELECT OCTET_LENGTH(`c``c`), SUBSTRING(`c``c`, 1, ?) \
             FROM `d``b`.`t``t` WHERE `k``k` <=> ? LIMIT 1"
        );
    }

    #[test]
    fn resolve_type_name_maps_common_types() {
        let non_binary = false;
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_LONG, false, non_binary),
            "INT"
        );
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_LONGLONG, false, non_binary),
            "BIGINT"
        );
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_NEWDECIMAL, false, non_binary),
            "DECIMAL"
        );
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_DATETIME, false, non_binary),
            "DATETIME"
        );
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_JSON, false, non_binary),
            "JSON"
        );
    }

    #[test]
    fn resolve_type_name_binary_charset_flips_string_and_blob_types() {
        // charset 63 (binary) turns text types into their binary counterparts.
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_VAR_STRING, false, true),
            "VARBINARY"
        );
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_VAR_STRING, false, false),
            "VARCHAR"
        );
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_STRING, false, true),
            "BINARY"
        );
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_STRING, false, false),
            "CHAR"
        );
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_BLOB, false, true),
            "BLOB"
        );
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_BLOB, false, false),
            "TEXT"
        );
    }

    #[test]
    fn resolve_type_name_unsigned_only_on_numeric_types() {
        // UNSIGNED suffix appended for numerics…
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_LONG, true, false),
            "INT UNSIGNED"
        );
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_NEWDECIMAL, true, false),
            "DECIMAL UNSIGNED"
        );
        // …but never for non-numeric types, even if the flag is set.
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_DATETIME, true, false),
            "DATETIME"
        );
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_VAR_STRING, true, false),
            "VARCHAR"
        );
    }

    #[test]
    fn is_binary_data_type_flags_only_raw_byte_types() {
        for t in [
            "VARBINARY",
            "BINARY",
            "BLOB",
            "TINYBLOB",
            "LONGBLOB",
            "GEOMETRY",
        ] {
            assert!(is_binary_data_type(t), "{t} should be binary data");
        }
        // Temporal/numeric report charset 63 too, but aren't binary DATA — and
        // `BIT` is in that company rather than with the blobs: it arrives as
        // bytes and is a *number*, which `convert_row` reads with `bit_display`
        // (see `core::model::type_is_bit`). Being on the list above made a
        // `BIT(8)` read `<1 bytes>`, kept it out of the CSV and JSON exports, and
        // made the column read-only.
        for t in [
            "DATETIME", "INT", "VARCHAR", "TEXT", "JSON", "DECIMAL", "BIT",
        ] {
            assert!(!is_binary_data_type(t), "{t} should not be binary data");
        }
    }

    #[test]
    fn column_origin_none_for_empty_org_table() {
        let flags = CoreColFlags::default();
        // Expression/aggregate/literal → empty org_table → not editable.
        assert!(column_origin("db", "", "expr", flags, false).is_none());
    }

    #[test]
    fn column_origin_some_carries_provenance_and_flags() {
        let flags = CoreColFlags {
            primary_key: true,
            not_null: true,
            ..Default::default()
        };
        let o = column_origin("shop", "users", "id", flags, false).expect("has base table");
        assert_eq!(o.database, "shop");
        assert_eq!(o.table, "users");
        assert_eq!(o.column, "id");
        assert!(o.flags.primary_key);
        assert!(o.flags.not_null);
        assert!(!o.binary);
    }

    fn s(x: &str) -> String {
        x.to_string()
    }

    /// A plain introspected column, for the grouping tests.
    fn cr(table: &str, name: &str, ty: &str, nullable: bool, pk: bool) -> ColRow {
        ColRow {
            table: s(table),
            column: ColumnInfo {
                name: s(name),
                type_name: s(ty),
                nullable,
                primary_key: pk,
                ..Default::default()
            },
        }
    }

    /// A raw catalogue row, for the default-normalization tests.
    fn my_row(ty: &str, default: Option<&str>, extra: &str) -> MyColRow {
        (
            s("t"),
            s("c"),
            s(ty),
            s("YES"),
            s(""),
            default.map(s),
            s(extra),
            None,
            None,
            None,
        )
    }

    /// MySQL hands back a string default *unquoted*, so emitting it verbatim
    /// would produce `DEFAULT draft` — a column reference, not a string.
    #[test]
    fn mysql_quotes_a_raw_string_default() {
        let c = mysql_column(my_row("varchar(20)", Some("draft"), ""), false).column;
        assert_eq!(c.default.as_deref(), Some("'draft'"));
    }

    /// MySQL 8 returns `CHECK_CLAUSE` with one *extra* level of backslash
    /// escaping, so restating it verbatim is a syntax error — measured against
    /// `SHOW CREATE TABLE`, which is the runnable form.
    #[test]
    fn mysql8_check_clauses_are_unescaped_to_the_runnable_form() {
        // Each pair is (what CHECK_CONSTRAINTS returns, what SHOW CREATE TABLE
        // says) for the same constraint on MySQL 8.4.
        let cases = [
            (
                r#"(`s` <> _latin1\'C:\\\\temp\')"#,
                r#"(`s` <> _latin1'C:\\temp')"#,
            ),
            (
                r#"(`s` <> _latin1\'it\\\'s\')"#,
                r#"(`s` <> _latin1'it\'s')"#,
            ),
            (r#"(`s` <> _latin1\'a\\nb\')"#, r#"(`s` <> _latin1'a\nb')"#),
            (
                r#"(not((`s` like _latin1\'%a%\')))"#,
                r#"(not((`s` like _latin1'%a%')))"#,
            ),
            (r#"(`qty` > 0)"#, r#"(`qty` > 0)"#),
            // A **control character in an identifier**, which is where dropping
            // the backslash instead of decoding the escape went wrong: this
            // names a column whose name contains a newline, and the old code
            // produced `nlncol` — a different, non-existent column. Measured on
            // MySQL 8.4.11 (`CHECK_CLAUSE` hex `…606E6C5C6E636F6C60…`, i.e. the
            // two bytes `\` `n`, against a real 0x0A in `SHOW CREATE TABLE`).
            ("(`nl\\ncol` > 0)", "(`nl\ncol` > 0)"),
        ];
        for (raw, want) in cases {
            assert_eq!(mysql_check_clause(raw, false), want, "for {raw}");
        }
    }

    /// MariaDB reports the clause already runnable — its `CHECK_CLAUSE` and its
    /// `SHOW CREATE TABLE` agree byte for byte. Unescaping it would eat the
    /// backslash out of `'it\'s'` and change the predicate.
    #[test]
    fn mariadb_check_clauses_are_left_alone() {
        let raw = r#"`s` <> 'it\'s'"#;
        assert_eq!(mysql_check_clause(raw, true), raw);
    }

    /// A trailing lone backslash has nothing to escape; dropping it would lose a
    /// character rather than an escape.
    #[test]
    fn a_dangling_backslash_survives() {
        assert_eq!(mysql_check_clause(r"a\", false), r"a\");
    }

    /// The clause MySQL 8 puts nowhere else. It sits between `CREATE` and
    /// `DEFINER`, so it's read positionally rather than searched for.
    #[test]
    fn a_views_algorithm_is_read_out_of_show_create_view() {
        assert_eq!(
            view_algorithm_of(
                "CREATE ALGORITHM=MERGE DEFINER=`root`@`localhost` SQL SECURITY DEFINER \
                 VIEW `v` AS select 1"
            )
            .as_deref(),
            Some("MERGE")
        );
        assert_eq!(
            view_algorithm_of("CREATE ALGORITHM = TEMPTABLE DEFINER=`r`@`h` VIEW `v` AS select 1")
                .as_deref(),
            Some("TEMPTABLE")
        );
    }

    /// `UNDEFINED` is the server's default and the emitter leaves it unwritten,
    /// so reading it back as a value would make every view look edited.
    #[test]
    fn an_undefined_or_absent_algorithm_is_none() {
        assert_eq!(
            view_algorithm_of("CREATE ALGORITHM=UNDEFINED DEFINER=`r`@`h` VIEW `v` AS select 1"),
            None
        );
        assert_eq!(
            view_algorithm_of("CREATE DEFINER=`r`@`h` SQL SECURITY DEFINER VIEW `v` AS select 1"),
            None
        );
        assert_eq!(view_algorithm_of(""), None);
    }

    /// The body is the user's own SQL and may say anything. Only the clause in
    /// its fixed position counts — a column called `algorithm=` in a `SELECT`
    /// must not be mistaken for one.
    #[test]
    fn the_view_body_cannot_impersonate_the_clause() {
        assert_eq!(
            view_algorithm_of(
                "CREATE DEFINER=`r`@`h` VIEW `v` AS select 'ALGORITHM=MERGE' as algorithm"
            ),
            None
        );
    }

    /// MariaDB already hands back SQL text, so quoting again would store the
    /// quotes as part of the value.
    #[test]
    fn mariadb_keeps_its_already_quoted_default() {
        let c = mysql_column(my_row("varchar(20)", Some("'draft'"), ""), true).column;
        assert_eq!(c.default.as_deref(), Some("'draft'"));
    }

    /// MariaDB writes an explicit `DEFAULT NULL` as the four characters `NULL`;
    /// that's the same as having no default worth restating.
    #[test]
    fn mariadb_null_default_is_no_default() {
        let c = mysql_column(my_row("varchar(20)", Some("NULL"), ""), true).column;
        assert_eq!(c.default, None);
    }

    /// A numeric default is a literal in both servers — quoting it would change
    /// the type of the stored default.
    #[test]
    fn a_numeric_default_is_never_quoted() {
        let c = mysql_column(my_row("int(11)", Some("0"), ""), false).column;
        assert_eq!(c.default.as_deref(), Some("0"));
    }

    /// The two ways MySQL says "this default is an expression": the 8.0
    /// `DEFAULT_GENERATED` flag, and the `CURRENT_TIMESTAMP` family that predates
    /// it. Quoting either would turn a live expression into a constant string.
    #[test]
    fn an_expression_default_is_left_alone() {
        let c = mysql_column(my_row("timestamp", Some("CURRENT_TIMESTAMP"), ""), false).column;
        assert_eq!(c.default.as_deref(), Some("CURRENT_TIMESTAMP"));
        let c = mysql_column(
            my_row("varchar(36)", Some("(uuid())"), "DEFAULT_GENERATED"),
            false,
        )
        .column;
        assert_eq!(c.default.as_deref(), Some("(uuid())"));
    }

    /// `EXTRA` is where MySQL keeps the two attributes `MODIFY COLUMN` would
    /// otherwise drop.
    #[test]
    fn extra_carries_auto_increment_and_on_update() {
        let c = mysql_column(my_row("int", None, "auto_increment"), false).column;
        assert!(c.auto_increment);
        let c = mysql_column(
            my_row(
                "timestamp",
                Some("CURRENT_TIMESTAMP"),
                "DEFAULT_GENERATED on update CURRENT_TIMESTAMP",
            ),
            false,
        )
        .column;
        assert_eq!(c.on_update.as_deref(), Some("CURRENT_TIMESTAMP"));
    }

    /// One key column of an index, as the MySQL fetch produces it (`non_unique`
    /// is the catalogue's sense: 1 means not unique).
    fn ir(table: &str, index: &str, non_unique: i64, col: &str) -> IdxRow {
        IdxRow {
            table: s(table),
            index: s(index),
            unique: non_unique == 0,
            column: schemaic_core::schema::IndexColumn::plain(col),
            method: None,
            predicate: None,
            lossy: false,
        }
    }

    fn tr(table: &str, name: &str, timing: &str, event: &str, order: u64) -> MyTriggerRow {
        (
            s(table),
            s(name),
            s(timing),
            s(event),
            s("SET NEW.x = 1"),
            s("root@localhost"),
            order,
        )
    }

    /// Verbatim `SHOW CREATE TRIGGER` output from MySQL 8.4.11 — the two bodies
    /// `information_schema.ACTION_STATEMENT` corrupts.
    ///
    /// Through that column they come back as `SET NEW.a = 'C:<TAB>emp'` (the
    /// `\t` already resolved, hex `…27433A09656D7027`) and `SET NEW.b = 'it's'`
    /// (a 1064 on restate). Here they are exactly as written.
    #[test]
    fn trigger_body_survives_the_escapes_information_schema_resolves() {
        let bs = "CREATE DEFINER=`schemaic`@`%` TRIGGER `wp5_bs` BEFORE INSERT ON `wp5` \
                  FOR EACH ROW SET NEW.a = 'C:\\temp'";
        assert_eq!(
            trigger_body_of(bs).as_deref(),
            Some("SET NEW.a = 'C:\\temp'")
        );
        let q = "CREATE DEFINER=`schemaic`@`%` TRIGGER `wp5_q` BEFORE UPDATE ON `wp5` \
                 FOR EACH ROW SET NEW.b = 'it''s'";
        assert_eq!(trigger_body_of(q).as_deref(), Some("SET NEW.b = 'it''s'"));
    }

    /// The anchor is found at a *code* position, so an identifier holding the
    /// words can't be mistaken for it — a table really can be named this.
    #[test]
    fn trigger_body_anchor_ignores_the_words_inside_an_identifier() {
        let sql = "CREATE DEFINER=`root`@`%` TRIGGER `t` BEFORE INSERT ON \
                   `a FOR EACH ROW b` FOR EACH ROW SET NEW.x = 1";
        assert_eq!(trigger_body_of(sql).as_deref(), Some("SET NEW.x = 1"));
        // …and a body that mentions them is returned whole.
        let sql = "CREATE TRIGGER `t` BEFORE INSERT ON `t2` FOR EACH ROW \
                   SET NEW.note = 'FOR EACH ROW'";
        assert_eq!(
            trigger_body_of(sql).as_deref(),
            Some("SET NEW.note = 'FOR EACH ROW'")
        );
    }

    /// The ordering clause belongs to `TriggerInfo::order`, which the emitter
    /// writes back — carrying it in the body too would emit it twice.
    #[test]
    fn trigger_body_drops_the_ordering_clause() {
        for sql in [
            "CREATE TRIGGER `b` BEFORE INSERT ON `t` FOR EACH ROW FOLLOWS `a` SET NEW.x = 1",
            "CREATE TRIGGER `b` BEFORE INSERT ON `t` FOR EACH ROW PRECEDES `a` SET NEW.x = 1",
            // A quoted name holding the next keyword must be stepped over whole.
            "CREATE TRIGGER `b` BEFORE INSERT ON `t` FOR EACH ROW FOLLOWS `a SET x` SET NEW.x = 1",
        ] {
            assert_eq!(
                trigger_body_of(sql).as_deref(),
                Some("SET NEW.x = 1"),
                "{sql}"
            );
        }
    }

    #[test]
    fn trigger_body_is_none_without_an_anchor() {
        assert!(trigger_body_of("CREATE TRIGGER `t` BEFORE INSERT ON `t2`").is_none());
        assert!(trigger_body_of("").is_none());
    }

    /// The ordering is the point: MySQL reports positions, and only a
    /// reconstructed FOLLOWS chain recreates the order it was given.
    #[test]
    fn mysql_triggers_rebuild_the_follows_chain_from_action_order() {
        let rows = [
            tr("orders", "third", "BEFORE", "INSERT", 3),
            tr("orders", "first", "BEFORE", "INSERT", 1),
            tr("orders", "second", "BEFORE", "INSERT", 2),
        ];
        let out = mysql_triggers(&rows);
        let by = |n: &str| out.iter().find(|t| t.name == n).unwrap().order.clone();
        // Position 1 anchors the chain from the front. This used to assert
        // `None` — which is the defect, not the contract: a `CREATE TRIGGER`
        // with no ordering clause is appended *last* by MySQL, so recreating
        // the leader reversed the group.
        assert_eq!(by("first"), Some(TriggerOrder::Precedes(s("second"))));
        assert_eq!(by("second"), Some(TriggerOrder::Follows(s("first"))));
        assert_eq!(by("third"), Some(TriggerOrder::Follows(s("second"))));
    }

    #[test]
    fn mysql_triggers_do_not_chain_across_groups() {
        // Same table, different event — and same event, different table. Neither
        // is a group, so neither may produce a FOLLOWS.
        let rows = [
            tr("orders", "a", "BEFORE", "INSERT", 1),
            tr("orders", "b", "BEFORE", "UPDATE", 1),
            tr("orders", "c", "AFTER", "INSERT", 1),
            tr("lines", "d", "BEFORE", "INSERT", 1),
        ];
        assert!(mysql_triggers(&rows).iter().all(|t| t.order.is_none()));
    }

    /// The **leading** trigger of a group needs an anchor too.
    ///
    /// It recorded none, because `ACTION_ORDER > 1` was the whole condition. So
    /// replacing it emitted a `CREATE TRIGGER` with no ordering clause, MySQL
    /// appended it **last**, and the group's firing order silently reversed —
    /// every later write computing from a chain that now runs backwards, with
    /// no error and nothing in the preview. Measured on MySQL 8.4.11: a group
    /// `[a, b]` came back `[b, a]` after replacing `a`, and `PRECEDES b`
    /// restored it.
    ///
    /// A group of one needs nothing: there is no order to lose.
    #[test]
    fn mysql_triggers_anchor_the_leading_trigger_of_a_group() {
        let rows = [
            tr("orders", "a", "BEFORE", "INSERT", 1),
            tr("orders", "b", "BEFORE", "INSERT", 2),
            tr("orders", "c", "BEFORE", "INSERT", 3),
        ];
        let out = mysql_triggers(&rows);
        assert_eq!(
            out[0].order,
            Some(TriggerOrder::Precedes(s("b"))),
            "the leader must name its successor"
        );
        assert_eq!(out[1].order, Some(TriggerOrder::Follows(s("a"))));
        assert_eq!(out[2].order, Some(TriggerOrder::Follows(s("b"))));

        // A lone trigger in its group has no order to preserve.
        let out = mysql_triggers(&[tr("orders", "only", "BEFORE", "INSERT", 1)]);
        assert!(out[0].order.is_none());
    }

    #[test]
    fn mysql_triggers_treat_zero_action_order_as_no_information() {
        // A server too old to report the column sends 0 for every row. Inventing
        // a chain there would order triggers the server never ordered.
        let rows = [
            tr("orders", "a", "BEFORE", "INSERT", 0),
            tr("orders", "b", "BEFORE", "INSERT", 0),
        ];
        assert!(mysql_triggers(&rows).iter().all(|t| t.order.is_none()));
    }

    #[test]
    fn mysql_triggers_carry_definer_timing_event_and_body() {
        let out = mysql_triggers(&[tr("orders", "a", "AFTER", "DELETE", 1)]);
        let t = &out[0];
        assert_eq!(t.timing, TriggerTiming::After);
        assert_eq!(t.events, vec![TriggerEvent::Delete]);
        assert_eq!(t.action, TriggerAction::Body(s("SET NEW.x = 1")));
        assert_eq!(t.definer.as_deref(), Some("root@localhost"));
        assert!(t.schema.is_none()); // MySQL has no namespace level
        assert_eq!(t.enabled, schemaic_core::schema::TriggerEnabled::Origin);
    }

    #[test]
    fn apply_triggers_drops_rows_for_tables_not_in_this_fetch() {
        let tables = [(s("orders"), s("BASE TABLE"))];
        let mut schema = assemble_schema(None, &tables, &[], &[], &[], &[]);
        let triggers = mysql_triggers(&[
            tr("orders", "keep", "BEFORE", "INSERT", 1),
            tr("ghost", "drop", "BEFORE", "INSERT", 1),
        ]);
        apply_triggers(&mut schema, triggers);
        assert_eq!(schema.tables[0].triggers.len(), 1);
        assert_eq!(schema.tables[0].triggers[0].name, "keep");
    }

    // ── Stored routines ──────────────────────────────────────────────────
    //
    // `routine_body_of` is the half `information_schema` cannot answer: every
    // MySQL routine edit is a `DROP` plus a `CREATE`, so a body that came back
    // with its escapes resolved fails *after* the only copy is gone. Every
    // input below is a real `SHOW CREATE` shape.

    /// The plain case: no characteristics at all between the parameter list and
    /// the body, which is what a bare `CREATE PROCEDURE` produces.
    #[test]
    fn routine_body_starts_after_the_parameter_list() {
        let sql = "CREATE DEFINER=`root`@`localhost` PROCEDURE `restock`(IN sku VARCHAR(20))\n\
                   BEGIN\n  UPDATE stock SET n = n + 1;\nEND";
        assert_eq!(
            routine_body_of(sql).as_deref(),
            Some("BEGIN\n  UPDATE stock SET n = n + 1;\nEND")
        );
    }

    /// Every characteristic MySQL prints, in the order it prints them — and the
    /// body still starts where the last one ends.
    #[test]
    fn routine_body_skips_every_characteristic_clause() {
        let sql = "CREATE DEFINER=`root`@`localhost` FUNCTION `label`(n INT) \
                   RETURNS varchar(20) CHARSET utf8mb4 COLLATE utf8mb4_general_ci\n\
                       DETERMINISTIC\n    NO SQL\n    SQL SECURITY INVOKER\n\
                       COMMENT 'names a number'\n\
                   BEGIN\n  RETURN 'x';\nEND";
        assert_eq!(
            routine_body_of(sql).as_deref(),
            Some("BEGIN\n  RETURN 'x';\nEND")
        );
    }

    /// The characteristic vocabulary and the statement vocabulary are disjoint,
    /// which is what makes consuming greedily safe — a body that begins with a
    /// bare statement survives, and so does one that *mentions* the words.
    #[test]
    fn routine_body_may_be_a_bare_statement() {
        let sql = "CREATE DEFINER=`a`@`b` PROCEDURE `p`() \
                   MODIFIES SQL DATA SELECT 'contains sql' AS note";
        assert_eq!(
            routine_body_of(sql).as_deref(),
            Some("SELECT 'contains sql' AS note")
        );
    }

    /// The escapes `information_schema.ROUTINE_DEFINITION` resolves — the whole
    /// reason this path exists. Through that column the second comes back as
    /// `'it's'`, a 1064 on restate.
    #[test]
    fn routine_body_survives_the_escapes_information_schema_resolves() {
        let bs = "CREATE DEFINER=`a`@`b` PROCEDURE `p`() SET @x = 'C:\\temp'";
        assert_eq!(routine_body_of(bs).as_deref(), Some("SET @x = 'C:\\temp'"));
        let q = "CREATE DEFINER=`a`@`b` PROCEDURE `p`() SET @x = 'it''s'";
        assert_eq!(routine_body_of(q).as_deref(), Some("SET @x = 'it''s'"));
    }

    /// The parameter list is found at a *code* position and skipped as a
    /// balanced group, so neither a routine named with a paren nor a default
    /// holding one can end it early.
    #[test]
    fn routine_body_is_not_confused_by_a_paren_in_a_name_or_a_literal() {
        let sql = "CREATE PROCEDURE `p(x)`(IN a VARCHAR(9) ) BEGIN SELECT 1; END";
        assert_eq!(routine_body_of(sql).as_deref(), Some("BEGIN SELECT 1; END"));
        // A `COMMENT` holding the word the loop would otherwise stop on.
        let sql = "CREATE PROCEDURE `p`() COMMENT 'BEGIN here' BEGIN SELECT 1; END";
        assert_eq!(routine_body_of(sql).as_deref(), Some("BEGIN SELECT 1; END"));
    }

    /// **A return type's modifiers each take their argument with them.** A
    /// keyword consumed without its value leaves that value at the head of what
    /// is returned as the body — and since every MySQL edit is a `DROP` plus a
    /// `CREATE`, that is a 1064 *after* the only copy is gone.
    #[test]
    fn routine_body_survives_every_return_type_modifier() {
        for ty in [
            "varchar(20) CHARSET utf8mb4",
            "varchar(20) CHARACTER SET utf8mb4",
            "varchar(20) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin",
            "varchar(20) CHARSET utf8mb4 COLLATE utf8mb4_bin",
            "decimal(10,2) UNSIGNED",
            "int UNSIGNED ZEROFILL",
            "char(1) BINARY",
            "int",
        ] {
            let sql = format!(
                "CREATE DEFINER=`a`@`b` FUNCTION `f`(n INT) RETURNS {ty}\n\
                 DETERMINISTIC\nBEGIN\n  RETURN 'x';\nEND"
            );
            assert_eq!(
                routine_body_of(&sql).as_deref(),
                Some("BEGIN\n  RETURN 'x';\nEND"),
                "RETURNS {ty}"
            );
        }
    }

    /// A bare `SET` is **not** a type modifier — it is only ever the second word
    /// of `CHARACTER SET`. On the trailer list it swallowed the first word of a
    /// body that legitimately begins `SET @x = 1`, which is a valid routine body
    /// on its own.
    #[test]
    fn routine_body_beginning_with_set_is_not_eaten_as_a_type_modifier() {
        let sql = "CREATE DEFINER=`a`@`b` FUNCTION `f`() RETURNS INT SET @x = 1";
        assert_eq!(routine_body_of(sql).as_deref(), Some("SET @x = 1"));
    }

    /// No parameter list, or nothing after the characteristics: this didn't
    /// understand the text, and says so rather than handing back a fragment the
    /// caller would restate.
    #[test]
    fn routine_body_is_none_when_the_text_is_not_understood() {
        assert_eq!(routine_body_of("not a create statement"), None);
        assert_eq!(routine_body_of("CREATE PROCEDURE `p`() NO SQL"), None);
        // Nothing at all after the keyword: the `COMMENT` arm used to index
        // byte 0 of an empty slice and take the process down rather than
        // degrade to the body it already had.
        assert_eq!(routine_body_of("CREATE PROCEDURE `p`() COMMENT"), None);
    }

    /// **Every field reads the column the query aliases, and it reads all of
    /// them.** The reader used to index the row `0..=13` against a `SELECT`
    /// fifteen hundred lines away — the struct replaced a tuple exactly because
    /// fourteen columns is past `mysql_common`'s `FromRow` ceiling, which is
    /// the same thing as saying the compiler stopped checking. Insert a column
    /// at position 3 and `body` read `CHARACTER_SET_NAME` and `sql_mode` read
    /// `ROUTINE_COMMENT`, with nothing failing; the three tests that exercise
    /// `mysql_routines` build a `MyRoutineRow` literal and never come through
    /// here.
    ///
    /// The reader now asks by name, so this is the pin on the two remaining
    /// ways it can drift: a field bound to the wrong alias, and an alias the
    /// query does not actually declare (which reads as NULL — silently empty,
    /// not loud).
    #[test]
    fn every_routine_field_reads_the_column_the_query_aliases() {
        // Hand each read back its own column name, and record what was asked.
        let mut asked: Vec<String> = Vec::new();
        let r = my_routine_row_from(|c| {
            asked.push(c.to_string());
            Some(c.to_string())
        });
        assert_eq!(r.name, "n");
        assert_eq!(r.kind, "ty");
        assert_eq!(r.returns, "rt");
        assert_eq!(r.returns_charset.as_deref(), Some("rtcs"));
        assert_eq!(r.returns_collation.as_deref(), Some("rtcoll"));
        assert_eq!(r.body.as_deref(), Some("body"));
        assert_eq!(r.deterministic, "det");
        assert_eq!(r.data_access, "acc");
        assert_eq!(r.security, "sec");
        assert_eq!(r.definer, "df");
        assert_eq!(r.comment, "cmt");
        assert_eq!(r.sql_mode.as_deref(), Some("sqlmode"));
        assert_eq!(r.charset_client.as_deref(), Some("cscl"));
        assert_eq!(r.collation_connection.as_deref(), Some("collconn"));
        assert_eq!(asked, MY_ROUTINE_COLUMNS, "one read per declared column");

        // …and the query really declares each of them, as a whole token, in
        // this order.
        let mut rest = MY_ROUTINES_SQL;
        for c in MY_ROUTINE_COLUMNS {
            let needle = format!(" AS {c}");
            let at = rest
                .find(&needle)
                .unwrap_or_else(|| panic!("the routine query aliases no column `{c}`"));
            rest = &rest[at + needle.len()..];
            assert!(
                matches!(rest.chars().next(), Some(',') | Some(' ')),
                "`{c}` is only the prefix of the alias the query declares"
            );
        }
    }

    /// The same pin, for the event read: a field bound to the wrong alias, and
    /// an alias the query does not actually declare (which reads as NULL —
    /// silently empty, not loud). Sixteen columns is well past the point where
    /// the compiler checks anything about this mapping.
    #[test]
    fn every_event_field_reads_the_column_the_query_aliases() {
        let mut asked: Vec<String> = Vec::new();
        let r = my_event_row_from(|c| {
            asked.push(c.to_string());
            Some(c.to_string())
        });
        assert_eq!(r.name, "n");
        assert_eq!(r.definer, "df");
        assert_eq!(r.kind, "ty");
        assert_eq!(r.execute_at.as_deref(), Some("at"));
        assert_eq!(r.interval_value.as_deref(), Some("iv"));
        assert_eq!(r.interval_field.as_deref(), Some("if_"));
        assert_eq!(r.starts.as_deref(), Some("st"));
        assert_eq!(r.ends.as_deref(), Some("en"));
        assert_eq!(r.status, "stat");
        assert_eq!(r.on_completion, "oc");
        assert_eq!(r.comment, "cmt");
        assert_eq!(r.body.as_deref(), Some("body"));
        assert_eq!(r.time_zone.as_deref(), Some("tz"));
        assert_eq!(r.sql_mode.as_deref(), Some("sqlmode"));
        assert_eq!(r.charset_client.as_deref(), Some("cscl"));
        assert_eq!(r.collation_connection.as_deref(), Some("collconn"));
        assert_eq!(asked, MY_EVENT_COLUMNS, "one read per declared column");

        let mut rest = MY_EVENTS_SQL;
        for c in MY_EVENT_COLUMNS {
            let needle = format!(" AS {c}");
            let at = rest
                .find(&needle)
                .unwrap_or_else(|| panic!("the event query aliases no column `{c}`"));
            rest = &rest[at + needle.len()..];
            assert!(
                matches!(rest.chars().next(), Some(',') | Some(' ')),
                "`{c}` is only the prefix of the alias the query declares"
            );
        }
    }

    fn er(name: &str, kind: &str) -> MyEventRow {
        MyEventRow {
            name: s(name),
            definer: s("root@localhost"),
            kind: s(kind),
            status: s("ENABLED"),
            on_completion: s("NOT PRESERVE"),
            body: Some(s("DELETE FROM t")),
            time_zone: Some(s("SYSTEM")),
            ..Default::default()
        }
    }

    /// A recurring row becomes an `EVERY` schedule, and the two catalogue
    /// columns that make it are quoted into SQL on the way in — the quantity
    /// bare because it is digits, the bounds as literals because they are
    /// datetimes.
    #[test]
    fn mysql_events_read_a_recurring_schedule() {
        let mut row = er("nightly", "RECURRING");
        row.interval_value = Some(s("1"));
        row.interval_field = Some(s("day"));
        row.starts = Some(s("2026-01-01 03:00:00"));
        let e = mysql_events(&[row]).remove(0);
        assert_eq!(e.name, "nightly");
        assert_eq!(e.schema, None);
        assert_eq!(e.definer.as_deref(), Some("root@localhost"));
        assert_eq!(
            e.schedule,
            EventSchedule::Every {
                value: s("1"),
                // Uppercased, so the emitted clause reads the way `SHOW CREATE`
                // prints it whatever case the catalogue used.
                unit: s("DAY"),
                starts: Some(s("'2026-01-01 03:00:00'")),
                ends: None,
            }
        );
        assert!(!e.preserve);
        assert_eq!(e.status, EventStatus::Enabled);
        assert_eq!(e.time_zone.as_deref(), Some("SYSTEM"));
    }

    /// A one-time row becomes an `AT` schedule, read off `EXECUTE_AT` — and
    /// `EVENT_TYPE` is what decides, not "is the interval NULL".
    #[test]
    fn mysql_events_read_a_one_time_schedule() {
        let mut row = er("once", "ONE TIME");
        row.execute_at = Some(s("2026-06-01 00:00:00"));
        row.on_completion = s("PRESERVE");
        row.status = s("SLAVESIDE_DISABLED");
        let e = mysql_events(&[row]).remove(0);
        assert_eq!(e.schedule, EventSchedule::At(s("'2026-06-01 00:00:00'")));
        assert!(e.preserve);
        assert_eq!(e.status, EventStatus::SlavesideDisabled);
    }

    /// A compound interval — `EVERY '1:30' HOUR_MINUTE` — keeps its quotes, and
    /// a `RECURRING` row the server reported nothing readable for still becomes
    /// a browsable event rather than being dropped from the list.
    #[test]
    fn mysql_events_survive_an_interval_they_cannot_read() {
        let mut row = er("compound", "RECURRING");
        row.interval_value = Some(s("1:30"));
        row.interval_field = Some(s("HOUR_MINUTE"));
        let e = mysql_events(&[row]).remove(0);
        assert_eq!(
            e.schedule,
            EventSchedule::Every {
                value: s("'1:30'"),
                unit: s("HOUR_MINUTE"),
                starts: None,
                ends: None,
            }
        );

        let e = mysql_events(&[er("mystery", "RECURRING")]).remove(0);
        assert_eq!(e.name, "mystery");
        assert_eq!(
            e.schedule,
            EventSchedule::Every {
                value: s("1"),
                unit: s("DAY"),
                starts: None,
                ends: None,
            }
        );
    }

    /// **The one-shot arm falls back too, and the fallback has to be legal.**
    /// An empty `AT` is exactly what `EventDraft::validate` refuses, so a
    /// `ONE TIME` row whose `EXECUTE_AT` came back empty would have opened an
    /// editor with Preview permanently disabled — an event that cannot be
    /// renamed, disabled or commented, which is the opposite of what the
    /// recurring arm's fallback is for.
    #[test]
    fn a_one_time_event_with_no_readable_time_is_still_editable() {
        let e = mysql_events(&[er("mystery_once", "ONE TIME")]).remove(0);
        assert_eq!(e.schedule, EventSchedule::At(s("CURRENT_TIMESTAMP")));
        assert!(
            schemaic_core::ddl::EventDraft::from_info(&e)
                .validate()
                .is_empty(),
            "the draft has to be one Preview will act on"
        );
        // And the fabricated value stays put: the schedule clause is restated
        // only on a change, so an edit that touches something else emits no
        // `ON SCHEDULE` at all.
        let mut d = schemaic_core::ddl::EventDraft::from_info(&e);
        d.info.comment = Some(s("paused"));
        let sql = schemaic_core::ddl::diff_event(&e, &d, SqlDialect::MySql).emit();
        assert!(!sql.iter().any(|s| s.contains("ON SCHEDULE")), "{sql:?}");
    }

    /// **The reader's output, put through the emitter.** Both halves of this seam
    /// were tested only against themselves: the tests above assert the *model* a
    /// catalogue row folds into and stop, and every `EventInfo` fixture in
    /// `schemaic-core` hand-writes `starts: Some("'2026-01-01 03:00:00'")` — i.e.
    /// hand-writes the answer `event_time_expr` is supposed to produce. So the
    /// quoting is asserted twice against one assumption and never end to end,
    /// which is what would let a second `ddl_string` downstream
    /// (`'''2026-01-01 03:00:00'''`) or a reader that stopped quoting
    /// (`STARTS 2026-01-01 03:00:00`) through.
    #[test]
    fn a_catalogue_row_emits_a_statement_with_its_datetimes_quoted_once() {
        use schemaic_core::ddl::{EventDraft, create_event, diff_event};

        // The columns exactly as `information_schema.EVENTS` reports them: bare,
        // unquoted.
        let mut row = er("nightly", "RECURRING");
        row.interval_value = Some(s("1"));
        row.interval_field = Some(s("day"));
        row.starts = Some(s("2026-01-01 03:00:00"));
        row.ends = Some(s("2027-01-01 03:00:00"));
        row.comment = s("nightly purge");
        let e = mysql_events(&[row]).remove(0);

        let sql = create_event(&EventDraft::from_info(&e), SqlDialect::MySql).emit();
        let create = sql
            .iter()
            .find(|s| s.starts_with("CREATE DEFINER"))
            .expect("one CREATE EVENT");
        assert!(
            create.contains("STARTS '2026-01-01 03:00:00'"),
            "one pair of quotes, not three and not none: {create}"
        );
        assert!(create.contains("ENDS '2027-01-01 03:00:00'"), "{create}");
        assert!(create.contains("COMMENT 'nightly purge'"), "{create}");

        // …and the round-trip gate holds across the seam: what the reader
        // produced diffs to nothing against its own draft, so opening the editor
        // on an untouched event says "No changes".
        assert!(diff_event(&e, &EventDraft::from_info(&e), SqlDialect::MySql).is_empty());

        // The one-time shape too — a different column (`EXECUTE_AT`) through a
        // different arm.
        let mut row = er("once", "ONE TIME");
        row.execute_at = Some(s("2026-06-01 00:00:00"));
        let e = mysql_events(&[row]).remove(0);
        let sql = create_event(&EventDraft::from_info(&e), SqlDialect::MySql).emit();
        assert!(
            sql.iter()
                .any(|s| s.contains("ON SCHEDULE AT '2026-06-01 00:00:00'")),
            "{sql:?}"
        );
        assert!(diff_event(&e, &EventDraft::from_info(&e), SqlDialect::MySql).is_empty());

        // **And the fabricated fallback reaching `create_sql`**, which is the
        // half `mysql_events`' own comment does not cover: the copy path restates
        // every clause unconditionally, so a row the server reported nothing
        // readable for emits the fallback rather than an empty clause. It has to
        // be legal SQL, which is why the fallback is a keyword and not "".
        let e = mysql_events(&[er("mystery", "RECURRING")]).remove(0);
        let sql = create_event(&EventDraft::from_info(&e), SqlDialect::MySql).emit();
        assert!(
            sql.iter().any(|s| s.contains("ON SCHEDULE EVERY 1 DAY")),
            "{sql:?}"
        );
        let e = mysql_events(&[er("mystery_once", "ONE TIME")]).remove(0);
        let sql = create_event(&EventDraft::from_info(&e), SqlDialect::MySql).emit();
        assert!(
            sql.iter()
                .any(|s| s.contains("ON SCHEDULE AT CURRENT_TIMESTAMP")),
            "an unquoted keyword, not a literal: {sql:?}"
        );
    }

    /// The body is everything after the **top-level** `DO`, and the two things
    /// that sit before it and spell it are stepped over rather than matched: a
    /// quoted identifier, and a string literal.
    #[test]
    fn the_show_create_event_body_starts_after_its_do() {
        assert_eq!(
            event_body_of(
                "CREATE DEFINER=`root`@`localhost` EVENT `nightly` ON SCHEDULE EVERY 1 DAY \
                 ON COMPLETION NOT PRESERVE ENABLE DO DELETE FROM sessions"
            )
            .as_deref(),
            Some("DELETE FROM sessions")
        );
        // A compound body, kept whole — the `;` inside it are the body's own.
        assert_eq!(
            event_body_of("CREATE EVENT `e` ON SCHEDULE EVERY 1 DAY DO BEGIN SELECT 1; END")
                .as_deref(),
            Some("BEGIN SELECT 1; END")
        );
        // An event *named* `do`: a quoted identifier, which `skip_noncode`
        // steps over rather than reading as the keyword.
        assert_eq!(
            event_body_of("CREATE EVENT `do` ON SCHEDULE EVERY 1 DAY DO SELECT 1").as_deref(),
            Some("SELECT 1")
        );
        // …and a `COMMENT` whose text says `do`, which is a string literal.
        assert_eq!(
            event_body_of(
                "CREATE EVENT `e` ON SCHEDULE EVERY 1 DAY COMMENT 'do not touch' DO SELECT 1"
            )
            .as_deref(),
            Some("SELECT 1")
        );
        // Not a statement this build understands: the caller keeps the body it
        // already had rather than blanking it.
        assert_eq!(
            event_body_of("CREATE EVENT `e` ON SCHEDULE EVERY 1 DAY"),
            None
        );
    }

    fn rr(name: &str, ty: &str, returns: &str) -> MyRoutineRow {
        MyRoutineRow {
            name: s(name),
            kind: s(ty),
            returns: s(returns),
            body: Some(s("BEGIN SELECT 1; END")),
            deterministic: s("NO"),
            data_access: s("READS_SQL_DATA"),
            security: s("DEFINER"),
            definer: s("root@localhost"),
            comment: s("hello"),
            ..Default::default()
        }
    }

    /// A `PARAMETERS` row as the server sends one. The mode is what the server
    /// states, **including for a function** — see
    /// [`mysql_parameters_drop_the_mode_from_a_functions_parameters`].
    fn pr(name: &str, ty: &str, mode: &str, pname: &str, dtd: &str) -> MyParamRow {
        (s(name), s(ty), s(mode), s(pname), s(dtd), None, None)
    }

    /// The rendered parameter list is rebuilt from `PARAMETERS`, because MySQL
    /// publishes no signature column — and it is keyed by name **and kind**, so
    /// a procedure and a function of the same name don't take each other's.
    #[test]
    fn mysql_routines_render_their_parameter_lists_per_kind() {
        let params = mysql_parameters(&[
            pr("go", "PROCEDURE", "IN", "sku", "VARCHAR(20)"),
            pr("go", "PROCEDURE", "OUT", "n", "INT"),
            pr("go", "FUNCTION", "IN", "n", "INT"),
        ]);
        let out = mysql_routines(
            &[rr("go", "PROCEDURE", ""), rr("go", "FUNCTION", "int")],
            &params,
        );
        assert_eq!(out[0].kind, schemaic_core::schema::RoutineKind::Procedure);
        assert_eq!(out[0].arguments, "IN sku VARCHAR(20), OUT n INT");
        assert!(out[0].returns.is_empty());
        assert_eq!(out[1].kind, schemaic_core::schema::RoutineKind::Function);
        assert_eq!(out[1].arguments, "n INT");
        assert_eq!(out[1].returns, "int");
    }

    /// The catalogue reports `PARAMETER_MODE = 'IN'` for a **function's**
    /// parameters — measured on MariaDB 10.11 — and `CREATE FUNCTION` has no
    /// grammar for it. Emitting it cost the routine: the recreate's `DROP` had
    /// already committed when the `CREATE` came back 1064.
    #[test]
    fn mysql_parameters_drop_the_mode_from_a_functions_parameters() {
        let params = mysql_parameters(&[
            pr("f", "FUNCTION", "IN", "n", "INT"),
            pr("p", "PROCEDURE", "IN", "n", "INT"),
            pr("p", "PROCEDURE", "INOUT", "acc", "DECIMAL(9,2)"),
        ]);
        assert_eq!(params[&(s("f"), s("FUNCTION"))], vec![s("n INT")]);
        assert_eq!(
            params[&(s("p"), s("PROCEDURE"))],
            vec![s("IN n INT"), s("INOUT acc DECIMAL(9,2)")]
        );
    }

    /// **`PARAMETER_NAME` is the bare name, and the rebuilt list has to quote
    /// it.**
    ///
    /// A procedure declared ``p(`order` INT)`` comes back from
    /// `information_schema.PARAMETERS` as `order`, and the list was joined raw
    /// — so the recreate emitted `CREATE PROCEDURE p(IN order INT)` and the
    /// server answered 1064 **after** the `DROP` had committed on its own.
    /// Reproduced live on MariaDB 10.11.14 and MySQL 8.4.11: the procedure was
    /// gone and nothing replaced it, and the backticked form restores it.
    ///
    /// This is the same statement, the same commit and the same stated failure
    /// as the parameter-mode fix beside it, which fixed the mode half and left
    /// the quoting half.
    #[test]
    fn mysql_parameters_quote_a_name_that_needs_quoting() {
        let params = mysql_parameters(&[
            pr("p", "PROCEDURE", "IN", "order", "INT"),
            pr("p", "PROCEDURE", "IN", "first name", "INT"),
            pr("p", "PROCEDURE", "IN", "amount", "DECIMAL(9,2)"),
            pr("f", "FUNCTION", "IN", "rank", "INT"),
        ]);
        assert_eq!(
            params[&(s("p"), s("PROCEDURE"))],
            vec![
                s("IN `order` INT"),
                s("IN `first name` INT"),
                // An ordinary name stays bare, so no rendered list a user is
                // already reading changes.
                s("IN amount DECIMAL(9,2)"),
            ]
        );
        assert_eq!(params[&(s("f"), s("FUNCTION"))], vec![s("`rank` INT")]);
    }

    /// **`AGGREGATE` is printed by `SHOW CREATE` and by nothing else.**
    ///
    /// MariaDB's `information_schema.ROUTINES` has no column that distinguishes
    /// an aggregate function — verified live, zero columns matching `%AGG%` —
    /// so the header of the `SHOW CREATE` text is the only place it can be
    /// learned. Dropping it cost the function: the recreate came back
    /// `ERROR 4105` after the `DROP` committed.
    #[test]
    fn the_show_create_header_reports_an_aggregate_function() {
        assert!(routine_is_aggregate(
            "CREATE DEFINER=`schemaic`@`%` AGGREGATE FUNCTION `f_agg`(x int(11)) RETURNS int(11) \
             BEGIN LOOP FETCH GROUP NEXT ROW; END LOOP; END"
        ));
        assert!(routine_is_aggregate(
            "create aggregate function f(x int) returns int RETURN 1"
        ));
        assert!(!routine_is_aggregate(
            "CREATE DEFINER=`schemaic`@`%` FUNCTION `f`(x int(11)) RETURNS int(11) RETURN 1"
        ));
        assert!(!routine_is_aggregate(
            "CREATE DEFINER=`a`@`b` PROCEDURE `p`(IN `n` INT) BEGIN SELECT 1; END"
        ));
        // The scan stops at the parameter list, so a body that talks about
        // aggregates says nothing about the header…
        assert!(!routine_is_aggregate(
            "CREATE FUNCTION `f`() RETURNS int BEGIN /* aggregate */ RETURN 1; END"
        ));
        // …and a routine *named* `aggregate` is a quoted identifier, which
        // `skip_noncode` steps over rather than reading as the keyword.
        assert!(!routine_is_aggregate(
            "CREATE DEFINER=`a`@`b` FUNCTION `aggregate`(x int) RETURNS int RETURN 1"
        ));
    }

    /// `DTD_IDENTIFIER` renders `longtext`, never the character set the
    /// parameter was declared with — that lives in its own column. A recreate
    /// that dropped it re-declared the parameter under the database default.
    #[test]
    fn mysql_routines_keep_a_parameters_character_set() {
        let params = mysql_parameters(&[(
            s("execute_prepared_stmt"),
            s("PROCEDURE"),
            s("IN"),
            s("in_query"),
            s("longtext"),
            Some(s("utf8mb3")),
            Some(s("utf8mb3_general_ci")),
        )]);
        assert_eq!(
            params[&(s("execute_prepared_stmt"), s("PROCEDURE"))],
            vec![s(
                "IN in_query longtext CHARACTER SET utf8mb3 COLLATE utf8mb3_general_ci"
            )]
        );

        // The same column pair on `ROUTINES` is a function's *return* type.
        let mut row = rr("extract_schema", "FUNCTION", "varchar(64)");
        row.returns_charset = Some(s("utf8mb3"));
        row.returns_collation = Some(s("utf8mb3_general_ci"));
        let out = mysql_routines(&[row], &HashMap::new());
        assert_eq!(
            out[0].returns,
            "varchar(64) CHARACTER SET utf8mb3 COLLATE utf8mb3_general_ci"
        );

        // NULL for every non-string type, and a procedure has no return type at
        // all — neither may grow a dangling clause.
        assert_eq!(mysql_type_with_charset("int", None, None), "int");
        assert_eq!(mysql_type_with_charset("", Some("utf8mb4"), None), "");
    }

    /// The session state a recreate has to restore comes from the catalogue, so
    /// a draft carries it from the first frame — the editor's lazy
    /// `SHOW CREATE` only ever corrects the body.
    #[test]
    fn mysql_routines_carry_the_session_state_a_recreate_restores() {
        let mut row = rr("rewards_report", "PROCEDURE", "");
        row.sql_mode = Some(s("STRICT_TRANS_TABLES,TRADITIONAL"));
        row.charset_client = Some(s("utf8mb3"));
        row.collation_connection = Some(s("utf8mb3_general_ci"));
        let out = mysql_routines(&[row], &HashMap::new());
        assert_eq!(
            out[0].sql_mode.as_deref(),
            Some("STRICT_TRANS_TABLES,TRADITIONAL")
        );
        assert_eq!(out[0].charset_client.as_deref(), Some("utf8mb3"));
        assert_eq!(
            out[0].collation_connection.as_deref(),
            Some("utf8mb3_general_ci")
        );
    }

    /// Every characteristic the emitter has to restate is read, and the
    /// security type falls to **DEFINER** — this engine's default, and the
    /// opposite of PostgreSQL's, so an unreadable value must not read as
    /// INVOKER and quietly widen what the routine may do.
    #[test]
    fn mysql_routines_carry_the_characteristics_a_recreate_would_reset() {
        let out = mysql_routines(&[rr("p", "PROCEDURE", "")], &HashMap::new());
        let r = &out[0];
        assert!(!r.deterministic);
        assert_eq!(
            r.data_access,
            schemaic_core::schema::SqlDataAccess::ReadsSqlData
        );
        assert!(r.security_definer);
        assert_eq!(r.definer.as_deref(), Some("root@localhost"));
        assert_eq!(r.comment.as_deref(), Some("hello"));
        // MySQL has no namespace level, and reports `SQL` for everything.
        assert!(r.schema.is_none());
        assert_eq!(r.language, "SQL");

        let mut invoker = rr("p", "PROCEDURE", "");
        invoker.security = s("INVOKER");
        assert!(!mysql_routines(&[invoker], &HashMap::new())[0].security_definer);
    }

    /// A routine whose definition the account may not read arrives with a NULL
    /// body, which is an empty one here rather than a panic.
    #[test]
    fn mysql_routines_tolerate_an_unreadable_body() {
        let mut row = rr("p", "PROCEDURE", "");
        row.body = None;
        assert!(mysql_routines(&[row], &HashMap::new())[0].body.is_empty());
    }

    #[test]
    fn assemble_schema_groups_columns_and_flags_pk() {
        let tables = [(s("users"), s("BASE TABLE"))];
        let cols = [
            cr("users", "id", "int", false, true),
            cr("users", "email", "varchar(255)", true, false),
        ];
        let schema = assemble_schema(None, &tables, &cols, &[], &[], &[]);
        assert_eq!(schema.tables.len(), 1);
        let t = &schema.tables[0];
        assert!(!t.is_view);
        assert_eq!(t.columns.len(), 2);
        assert!(t.columns[0].primary_key);
        assert!(!t.columns[0].nullable); // IS_NULLABLE = "NO"
        assert!(!t.columns[1].primary_key);
        assert!(t.columns[1].nullable); // "YES"
    }

    #[test]
    fn assemble_schema_stamps_the_namespace_on_every_table() {
        let tables = [(s("orders"), s("BASE TABLE")), (s("v"), s("VIEW"))];
        // MySQL: no namespace level at all.
        let mysql = assemble_schema(None, &tables, &[], &[], &[], &[]);
        assert!(mysql.tables.iter().all(|t| t.schema.is_none()));
        // Postgres: every table in the batch belongs to the one namespace it was
        // fetched for — views included.
        let pg = assemble_schema(Some("sales"), &tables, &[], &[], &[], &[]);
        assert!(
            pg.tables
                .iter()
                .all(|t| t.schema.as_deref() == Some("sales"))
        );
    }

    #[test]
    fn assemble_schema_folds_composite_index_in_order() {
        let tables = [(s("t"), s("BASE TABLE"))];
        // Two rows for the same index name → one IndexInfo, columns in row order.
        let idx = [
            ir("t", "idx_ab", 1, "a"),
            ir("t", "idx_ab", 1, "b"),
            ir("t", "PRIMARY", 0, "id"),
        ];
        let schema = assemble_schema(None, &tables, &[], &[], &idx, &[]);
        let t = &schema.tables[0];
        assert_eq!(t.indexes.len(), 2);
        let ab = t.indexes.iter().find(|i| i.name == "idx_ab").unwrap();
        assert_eq!(ab.column_names().collect::<Vec<_>>(), vec!["a", "b"]);
        assert!(!ab.unique); // NON_UNIQUE = 1
        let pk = t.indexes.iter().find(|i| i.name == "PRIMARY").unwrap();
        assert!(pk.unique); // NON_UNIQUE = 0
        assert!(pk.is_primary());
    }

    #[test]
    fn assemble_schema_flags_foreign_index_by_columns_not_name() {
        let tables = [(s("orders"), s("BASE TABLE"))];
        // The FK's backing index is named after the column (`customerNumber`), not
        // the constraint (`orders_ibfk_1`) — the classicmodels case. Matching by
        // name misses it; matching by columns flags it.
        let idx = [
            ir("orders", "customerNumber", 1, "customerNumber"),
            ir("orders", "idx_plain", 1, "total"),
        ];
        let fks = [(
            s("orders"),
            s("orders_ibfk_1"),
            s("customerNumber"),
            Some(s("shop")),
            Some(s("customers")),
            Some(s("customerNumber")),
        )];
        let schema = assemble_schema(None, &tables, &[], &fks, &idx, &[]);
        let t = &schema.tables[0];
        assert!(
            t.indexes
                .iter()
                .find(|i| i.name == "customerNumber")
                .unwrap()
                .foreign,
            "FK-backing index flagged FOREIGN by columns despite name != constraint"
        );
        assert!(
            !t.indexes
                .iter()
                .find(|i| i.name == "idx_plain")
                .unwrap()
                .foreign
        );
    }

    #[test]
    fn assemble_schema_builds_foreign_keys_with_targets() {
        let tables = [(s("orders"), s("BASE TABLE"))];
        // One single-column FK and one composite FK (two ordered rows).
        let fks = [
            (
                s("orders"),
                s("fk_customer"),
                s("customer_id"),
                Some(s("shop")),
                Some(s("customers")),
                Some(s("id")),
            ),
            (
                s("orders"),
                s("fk_line"),
                s("order_id"),
                Some(s("shop")),
                Some(s("lines")),
                Some(s("order_id")),
            ),
            (
                s("orders"),
                s("fk_line"),
                s("line_no"),
                Some(s("shop")),
                Some(s("lines")),
                Some(s("no")),
            ),
        ];
        let schema = assemble_schema(None, &tables, &[], &fks, &[], &[]);
        let t = &schema.tables[0];
        assert_eq!(t.foreign_keys.len(), 2);

        let single = t.fk_for_column("customer_id").unwrap();
        assert_eq!(single.ref_table, "customers");
        assert_eq!(single.ref_schema.as_deref(), Some("shop"));
        assert_eq!(single.ref_columns, vec!["id".to_string()]);

        // Composite FK: both columns fold into one FK, in ORDINAL_POSITION order.
        let composite = t.fk_for_column("line_no").unwrap();
        assert_eq!(composite.ref_table, "lines");
        assert_eq!(
            composite.columns,
            vec!["order_id".to_string(), "line_no".to_string()]
        );
        assert_eq!(
            composite.ref_columns,
            vec!["order_id".to_string(), "no".to_string()]
        );
    }

    #[test]
    fn assemble_schema_marks_views_and_attaches_definition() {
        let tables = [(s("v"), s("VIEW")), (s("base"), s("BASE TABLE"))];
        let views = [(s("v"), s("SELECT 1"))];
        let schema = assemble_schema(None, &tables, &[], &[], &[], &views);
        let v = schema.tables.iter().find(|t| t.name == "v").unwrap();
        assert!(v.is_view);
        assert_eq!(v.view_definition.as_deref(), Some("SELECT 1"));
        let base = schema.tables.iter().find(|t| t.name == "base").unwrap();
        assert!(!base.is_view);
        assert!(base.view_definition.is_none());
    }

    #[test]
    fn assemble_schema_drops_rows_for_unknown_tables() {
        let tables = [(s("known"), s("BASE TABLE"))];
        // Column/index rows referencing a table absent from `tables` are ignored.
        let cols = [cr("ghost", "x", "int", false, true)];
        let idx = [ir("ghost", "idx", 1, "x")];
        let schema = assemble_schema(None, &tables, &cols, &[], &idx, &[]);
        assert_eq!(schema.tables.len(), 1);
        assert!(schema.tables[0].columns.is_empty());
        assert!(schema.tables[0].indexes.is_empty());
    }

    #[test]
    fn assemble_schema_empty_view_definition_stays_none() {
        // A view whose VIEW_DEFINITION came back empty (e.g. privileges) → None,
        // so create_ddl falls back to its placeholder.
        let tables = [(s("v"), s("VIEW"))];
        let views = [(s("v"), s(""))];
        let schema = assemble_schema(None, &tables, &[], &[], &[], &views);
        assert!(schema.tables[0].view_definition.is_none());
    }

    /// The values that mean "nothing to restate" have to fold to `None`, or the
    /// schema editor opens on a phantom change; the ones that mean something
    /// have to survive, or a replace quietly resets them.
    #[test]
    fn mysql_view_options_keeps_only_what_is_set() {
        let o = mysql_view_options("NONE", "root@localhost", "DEFINER", Some("UNDEFINED"));
        assert_eq!(o.check_option, None);
        assert_eq!(o.definer.as_deref(), Some("root@localhost"));
        assert_eq!(o.security.as_deref(), Some("DEFINER"));
        assert_eq!(o.algorithm, None);

        let o = mysql_view_options("cascaded", "app@10.0.0.1", "INVOKER", Some("merge"));
        assert_eq!(o.check_option.as_deref(), Some("CASCADED"));
        assert_eq!(o.security.as_deref(), Some("INVOKER"));
        assert_eq!(o.algorithm.as_deref(), Some("MERGE"));
        // PostgreSQL's half of the struct stays empty on MySQL.
        assert!(o.storage.is_empty() && !o.materialized);

        // MySQL 8 reports no algorithm at all.
        assert_eq!(
            mysql_view_options("NONE", "", "", None),
            ViewOptions::default()
        );
    }

    #[test]
    fn explain_commands_plain_has_no_fallback() {
        let (primary, fallback) = explain_commands("SELECT * FROM t", false);
        assert_eq!(primary, "EXPLAIN SELECT * FROM t");
        assert!(fallback.is_none());
    }

    #[test]
    fn explain_commands_analyze_offers_mariadb_fallback() {
        let (primary, fallback) = explain_commands("SELECT 1", true);
        assert_eq!(primary, "EXPLAIN ANALYZE SELECT 1");
        assert_eq!(fallback.as_deref(), Some("ANALYZE SELECT 1"));
    }

    #[test]
    fn explain_commands_strips_trailing_semicolon_and_space() {
        let (primary, _) = explain_commands("  SELECT 1 ;  ", false);
        assert_eq!(primary, "EXPLAIN SELECT 1");
    }

    /// This crate's three identifier quoters answer to `core`'s, so the SQL a
    /// write path builds can't drift from the SQL the export and DDL paths
    /// build. Each is engine-fixed by construction — `pg.rs` only ever emits
    /// PostgreSQL, `sqlite.rs`'s statements only ever SQLite, this module's
    /// remaining builders only ever MySQL — which is why they take no dialect and
    /// why the binding has to be asserted rather than typed.
    #[test]
    fn the_write_paths_quote_identifiers_the_way_core_does() {
        use schemaic_core::export::ident_sql;
        use schemaic_core::intel::SqlDialect;
        for name in [
            "plain",
            "MixedCase",
            "with space",
            "a`b",
            "a\"b",
            "both`and\"",
            "sélect",
            "",
        ] {
            assert_eq!(ident(name), ident_sql(name, SqlDialect::MySql), "{name:?}");
            assert_eq!(
                crate::pg::pg_ident_for_test(name),
                ident_sql(name, SqlDialect::Postgres),
                "{name:?}"
            );
            assert_eq!(
                ident_sqlite(name),
                ident_sql(name, SqlDialect::Sqlite),
                "{name:?}"
            );
        }
    }

    #[test]
    fn lock_wait_sql_bounds_the_wait_on_both_engines() {
        assert_eq!(
            lock_wait_sql(Engine::MySql),
            "SET SESSION lock_wait_timeout = 10"
        );
        assert_eq!(lock_wait_sql(Engine::Postgres), "SET lock_timeout = '10s'");
    }

    #[test]
    fn a_successful_plan_changed_the_schema() {
        assert!(ddl_changed_schema(&Ok(())));
    }

    #[test]
    fn a_half_applied_mysql_plan_changed_the_schema() {
        // The whole reason `applied` exists: statement 2 failed, statement 1 is
        // in effect and cannot be rolled back, so the introspected model is now
        // wrong and the caller must re-read it.
        let err = DdlError {
            message: "Duplicate key name 'ix'".to_string(),
            at: 1,
            applied: 1,
        };
        assert!(ddl_changed_schema(&Err(err)));
    }

    #[test]
    fn a_plan_that_failed_on_its_first_statement_changed_nothing() {
        // Also PostgreSQL's every failure — the transaction rolled the plan back,
        // so `applied` is 0 whichever statement failed.
        let err = DdlError {
            message: "syntax error".to_string(),
            at: 3,
            applied: 0,
        };
        assert!(!ddl_changed_schema(&Err(err)));
    }
}
