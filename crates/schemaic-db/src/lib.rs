//! Database access for Schemaic: the [`Db`] façade and the engine dispatch
//! behind it.
//!
//! **Three engines, and they are not equal.** Every public method here connects,
//! runs, disconnects (the one-connection-per-operation invariant, with its two
//! stated exceptions in [`session`] and [`Db::run_script`]), and dispatches on
//! [`Engine`]: PostgreSQL to [`pg`], SQLite to [`sqlite`], MySQL to [`mysql`].
//! **That last one is being moved here a piece at a time** — the bodies not yet
//! in `mysql.rs` are still inline below, so `pg.rs` and `sqlite.rs` are peers of
//! `mysql.rs` for the entry points it has taken over and peers of this file for
//! the rest. What makes it bearable is that [`Engine`] is an
//! enum: a fourth variant is a compiler error at every dispatch site. What it
//! does not catch is a fourth engine *module* that omits a function, since the
//! engine interface here is a **naming convention rather than a trait** —
//! deliberately, because the signatures genuinely differ (`sqlite::fetch_query`
//! takes no database, since there is one). `ENGINE_ENTRY_POINTS` is that
//! convention written down, and `every_engine_module_answers_the_whole_interface`
//! is what holds a module to it.
//!
//! A query runs on a **dedicated connection** whose id is captured up front, so
//! it can be cancelled server-side from a second connection — `KILL QUERY` on
//! MySQL, `pg_cancel_backend` on PostgreSQL, the interrupt handle on SQLite —
//! and stops at a row cap.
//!
//! # The MySQL backend
//!
//! Statements go over the **text protocol**. Built on [`mysql_async`] (not
//! sqlx): we need the per-column wire metadata — `org_table` / `org_name` / key
//! flags — that the MySQL protocol sends in every column-definition packet,
//! which is the foundation of the editing system. sqlx's MySQL driver parses
//! that packet but keeps only the alias name + type, so it can't tell which real
//! table/column a result cell came from.

pub mod mysql;
pub mod pg;
pub mod session;
pub mod sqlite;
pub mod ssh;
mod tls;

pub use session::{Outcome, Session};

use std::collections::HashMap;

use mysql_async::consts::ColumnFlags;
use mysql_async::prelude::Queryable;
use mysql_async::{Column as MyColumn, Conn, Row, Value as MyValue};
use mysql_async::{OptsBuilder, Params};
use schemaic_core::activity::{self, KillKind, SessionInfo};
use schemaic_core::blob::{BlobRef, BlobValue, FETCH_CAP};
use schemaic_core::model::{
    CellEdit, Column, GridWrite, RefetchRow, RefetchTemplate, ResultBuilder, ResultSet, Rollback,
    RowDelete, RowEdit, RowInsert, Value, WriteStep, one_row_verdict,
};
use schemaic_core::schema::{
    ColumnInfo, DbSchema, EventSource, ForeignKeyInfo, IndexInfo, TableInfo, TriggerSource,
};
use schemaic_core::stats::{SchemaStats, count_rows_sql};
use schemaic_core::users::{self, Grants, Principal};
use tokio_util::sync::CancellationToken;

// **Reached back into the engine module, which is the wrong direction** — and
// temporary. The write-back paths below (`refetch_on` and its neighbours) are
// the last MySQL bodies still in this file; they move in the next step and take
// these three with them. Listed here rather than spelled `mysql::` at each call
// site so that the day they leave, this import is what fails.
use crate::mysql::{convert_row, fractional_scales, map_column};

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

/// What an engine module has to answer, by name.
///
/// **The engine interface is a convention, not a trait**, and this is the
/// convention. A trait is not obviously right here — `sqlite::fetch_query` takes
/// no `database` because there is only one, and `pg::run_script` and
/// `sqlite::run_script` differ in what a statement boundary is — so the shapes
/// are per engine on purpose. What that costs is the check a trait gives for
/// free: adding a fourth [`Engine`] variant is a compiler error at all ~25
/// dispatch sites, while adding a fourth engine *module* that simply omits
/// `fetch_blob` is no error at all until somebody writes that arm.
///
/// So the list is written down, and
/// `every_engine_module_answers_the_whole_interface` holds each module to it.
/// MySQL is not in that check because it has no module of its own — its bodies
/// are inline in this file, which is the asymmetry the crate doc opens with.
///
/// Test-only: it is a statement *about* the code rather than something the code
/// reads, which is the same reason `source_gate`'s machinery next door is.
#[cfg(test)]
pub(crate) const ENGINE_ENTRY_POINTS: &[&str] = &[
    "fetch_query",
    "fetch_schema",
    "fetch_databases",
    "fetch_table_list",
    "fetch_blob",
    "count_rows",
    "refetch_rows",
    "commit_writes",
    "import_rows",
    "run_ddl",
    "run_script",
    "prepare_check",
    "ping",
];

/// Which database engine a [`Db`] speaks. Selected from the saved connection's
/// `db_type` at [`Db::connect`] time; each public method dispatches to the
/// engine-specific backend (MySQL bodies inline here, Postgres in [`pg`],
/// SQLite in [`sqlite`]).
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
#[derive(Clone)]
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

/// **Hand-written, because the derived one printed the password.**
///
/// No site formats a `Db` today — the whole workspace was checked — which is
/// what made the derive latent rather than live. But this type is threaded
/// through nearly everything (cloned into `McpEndpoint`, `StartAiParams`, the
/// dump and script runners), and the moment a struct that owns one gains a
/// `#[derive(Debug)]` and is logged, or anyone writes
/// `.expect(&format!("{db:?}"))`, the credential lands in
/// `%APPDATA%\Roaming\schemaic`'s log — the folder the Settings pane invites
/// the user to share for support.
///
/// The struct's own doc already claims the property: "no plaintext URL is
/// embedded anywhere as identity or leaked on a command line". A derived
/// `Debug` was an unguarded second spelling of the same leak, and the invariant
/// it belongs to is *no credential in a URL, argv or log* — all three.
///
/// Everything else prints, because the point of a `Debug` here is to say which
/// endpoint this is.
impl std::fmt::Debug for Db {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Db")
            .field("engine", &self.engine)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.user)
            .field("pass", &"<redacted>")
            .field("file", &self.file)
            .field("database", &self.database)
            .field("tls", &self.tls)
            .finish()
    }
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
            Engine::MySql => mysql::fetch_query(self, database, sql, dest, cancel).await?,
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
        // **Bounded, because the Live Monitor calls this on a timer, forever.**
        // `fetch_sessions`' deadline was added under the claim that it was the
        // app's only such caller; `monitor_tick` re-arms this one every two
        // seconds for as long as its modal is open. The `CancellationToken` in
        // the signature is what admitted it to the "already bounded" set, and it
        // is bounded only if a caller *keeps* the token — the monitor built one
        // inline, stored it nowhere and cancelled it never. A dark host does not
        // refuse the connect, it swallows it, so every tick cost the OS TCP
        // timeout (21.0 s on MySQL, 63 s on PostgreSQL — see [`PING_TIMEOUT`])
        // under a modal still showing the last snapshot with no error.
        //
        // The deadline is over the whole dispatch, the shape the two activity
        // methods took, so it covers the engine arms as well as the connect.
        let fetch = async {
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
                Engine::MySql => {
                    mysql::fetch_table(self, database, schema, table, order_by, limit, cancel).await
                }
            }
        };
        tokio::time::timeout(PING_TIMEOUT, fetch)
            .await
            .map_err(|_| DbError::Connect("timed out".to_string()))?
    }
}

/// A plan's row count is tiny (classic EXPLAIN) or one big row (tree-format
/// `EXPLAIN ANALYZE`); this cap is only a backstop.
pub(crate) const EXPLAIN_ROW_CAP: usize = 10_000;

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
            Engine::MySql => mysql::explain(self, database, sql, analyze, cancel).await,
        }
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
        mysql::routine_source(self, database, kind, name).await
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
        mysql::event_source(self, database, name).await
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
        mysql::trigger_source(self, database, trigger).await
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
        mysql::view_algorithm(self, database, view).await
    }

    pub async fn prepare_check(&self, database: Option<&str>, sql: &str) -> Result<(), DbError> {
        let stmt = sql.trim().trim_end_matches(';').trim_end();
        if stmt.is_empty() {
            return Ok(());
        }
        match self.engine {
            Engine::Postgres => pg::prepare_check(self, database, sql).await,
            Engine::Sqlite => sqlite::prepare_check(self, stmt).await,
            Engine::MySql => mysql::prepare_check(self, database, stmt).await,
        }
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
        let on_result = {
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
            }
            Engine::Sqlite => {
                // **One connection, like the other two arms and like this
                // method's own doc.** This was a loop over `fetch_query` — a
                // fresh connection per statement — on the reasoning that there
                // is no `USE` to carry and no session state to keep. A `PRAGMA`
                // is session state, and `sqlite_rebuild_sql` puts two of them in
                // the plan deliberately because that plan is also what Copy and
                // "Open in editor" hand the user. Both were inert here: the
                // rebuild's `PRAGMA foreign_keys = OFF` was gone by the
                // `DROP TABLE`, which then cascade-emptied child tables on a
                // plan that reported success, and `legacy_alter_table` was gone
                // by the shadow table's `RENAME TO`, which left the user's table
                // dropped. See `sqlite::run_batch`.
                //
                // The `scope` stamping above is still a no-op for an engine with
                // one database.
                sqlite::run_batch(self, stmts, row_cap, cancel, on_result).await;
            }
            // `scope` and the dialect go across because `USE` is MySQL's alone —
            // see `mysql::run_batch` for why one engine's arm takes two
            // arguments its peers do not.
            Engine::MySql => {
                mysql::run_batch(
                    self, database, stmts, row_cap, cancel, on_result, scope, dialect,
                )
                .await;
            }
        }
    }
}

/// `DbError` isn't `Clone`; this reproduces one for the "connect failed" fan-out.
pub(crate) fn err_clone(e: &DbError) -> DbError {
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
            // Bounded inside, like its two neighbours — see `mysql::ping`.
            Engine::MySql => mysql::ping(self, timeout).await,
        }
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
            Engine::Postgres => pg::fetch_databases(self).await,
            Engine::Sqlite => sqlite::fetch_databases(self).await,
            Engine::MySql => mysql::fetch_databases(self).await,
        }
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
            Engine::Postgres => pg::fetch_schema(self, database, cancel).await,
            Engine::Sqlite => sqlite::fetch_schema(self, cancel).await,
            Engine::MySql => mysql::fetch_schema(self, database, cancel).await,
        }
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
            Engine::Postgres => pg::fetch_table_list(self, database).await,
            // SQLite's names come from one `sqlite_master` scan, which is already
            // what the full introspection starts from; the saving this method
            // exists for is the *per-table* pragmas, so the list path skips those.
            Engine::Sqlite => sqlite::fetch_table_list(self).await,
            Engine::MySql => mysql::fetch_table_list(self, database).await,
        }
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
            Engine::Postgres => pg::fetch_table_stats(self, database).await,
            Engine::Sqlite => Ok(SchemaStats::default()),
            Engine::MySql => mysql::fetch_table_stats(self, database).await,
        }
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
            Engine::Postgres => pg::count_rows(self, database, &sql, cancel).await,
            Engine::Sqlite => sqlite::count_rows(self, &sql, cancel).await,
            Engine::MySql => mysql::count_rows(self, database, &sql, cancel).await,
        }
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
    /// **Bounded by [`PING_TIMEOUT`], around the whole thing** — and it is the
    /// one method here that most needed it, because it is the only one that runs
    /// **on a timer, forever**.
    ///
    /// Every sibling in this file already bounds itself for a host that stops
    /// answering at the packet level: [`Db::ping`] and [`Db::fetch_databases`]
    /// take the same five seconds, [`Db::kill_query`] takes [`CANCEL_TIMEOUT`],
    /// and the unbounded reads all take a `CancellationToken`. This took
    /// neither, so a poll against a black-holed host blocked for the OS TCP
    /// connect timeout — 21.0 s on MySQL, 63 s on PostgreSQL, this file's own
    /// measurement at [`PING_TIMEOUT`] — showing the previous snapshot with no
    /// error, beside a health check that had already said *Disconnected* at five
    /// seconds.
    ///
    /// **Hung polls stacked.** Regaining window focus re-runs the panel effect,
    /// which bumps the activity generation and then refreshes because it woke;
    /// the in-flight guard is keyed on that generation, so by construction it
    /// cannot suppress a refresh that follows a bump. Three alt-tabs inside one
    /// 21-second connect left three connects hanging at once, each holding a
    /// socket and a spawned task. A deadline is the fix that does not depend on
    /// the guard: a poll that has not answered inside the app's own "the server
    /// is not responding" window has nothing to add to a panel that will ask
    /// again in two seconds.
    ///
    /// Around the dispatch rather than inside each arm, for
    /// [`Db::fetch_databases`]' reason: PostgreSQL's `connect_maintenance` tries
    /// three candidate databases in turn, so a per-attempt bound is three times
    /// the deadline it claims.
    pub async fn fetch_sessions(&self) -> Result<Vec<SessionInfo>, DbError> {
        if !activity::supports_activity(self.engine.dialect()) {
            return Err(DbError::Query(NO_SESSIONS_MSG.to_string()));
        }
        let poll = async {
            match self.engine {
                Engine::Postgres => pg::fetch_sessions(self).await,
                Engine::MySql => mysql::fetch_sessions(self).await,
                // Unreachable — `supports_activity` above is the gate.
                Engine::Sqlite => Err(DbError::Query(NO_SESSIONS_MSG.to_string())),
            }
        };
        tokio::time::timeout(PING_TIMEOUT, poll)
            .await
            .map_err(|_| DbError::Connect("timed out".to_string()))?
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
    ///
    /// **Bounded by [`CANCEL_TIMEOUT`]**, for the reason that constant exists and
    /// [`Db::kill_query`] already cites: the whole premise of reaching this is
    /// that something on that server is not behaving, and the answer is to open
    /// a *fresh* connection to it — full TCP, a TLS handshake, possibly a second
    /// connect on `prefer`. Unbounded, a Kill against a host that has stopped
    /// answering hangs the modal button for the OS connect timeout with no way
    /// to say so.
    ///
    /// The timeout is reported as an error, which is the honest reading and not
    /// a claim the kill failed: the statement may well have landed. The panel
    /// polls, so the list settles the question within the interval — and
    /// `activity_kill_error` exists precisely so a refused or unanswered kill
    /// leaves the snapshot on screen rather than replacing it.
    pub async fn kill_session(&self, id: i64, kind: KillKind) -> Result<(), DbError> {
        if !activity::supports_kill(self.engine.dialect()) {
            return Err(DbError::Query(NO_SESSIONS_MSG.to_string()));
        }
        let kill = async {
            match self.engine {
                Engine::Postgres => pg::kill_session(self, id, kind).await,
                Engine::MySql => mysql::kill_session(self, id, kind).await,
                // Unreachable — `supports_kill` above is the gate.
                Engine::Sqlite => Err(DbError::Query(NO_SESSIONS_MSG.to_string())),
            }
        };
        tokio::time::timeout(CANCEL_TIMEOUT, kill)
            .await
            .map_err(|_| DbError::Connect("timed out".to_string()))?
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
            Engine::Postgres => pg::fetch_principals(self).await,
            Engine::MySql => mysql::fetch_principals(self).await,
            // Unreachable — `supports_users` above is the gate.
            Engine::Sqlite => Err(DbError::Query(NO_USERS_MSG.to_string())),
        }
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
            Engine::Postgres => pg::fetch_grants(self, database, principal).await,
            // `database` is not passed on: MySQL's grant tables are server-wide
            // and answer for every database at once, which is what this method's
            // own doc says and what `mysql::fetch_grants` declines to take a
            // parameter for.
            Engine::MySql => mysql::fetch_grants(self, principal).await,
            // Unreachable — `supports_users` above is the gate.
            Engine::Sqlite => Err(DbError::Query(NO_USERS_MSG.to_string())),
        }
    }
}

/// Why a connection has no accounts to browse. One sentence, one place, so the
/// two methods that raise it can't drift apart — and, like [`NO_SESSIONS_MSG`],
/// it names the capability rather than the engine because that is what the
/// caller asked. The browser checks `supports_users` itself and shows its own
/// explanation; this is the backstop for a caller that didn't.
const NO_USERS_MSG: &str = "this connection's engine has no user accounts";

/// Why a connection has no Server Activity to report. One sentence, one place,
/// so the two methods that raise it can't drift apart.
///
/// It names the *capability* rather than the engine, because that is what the
/// caller asked and because nothing renders this in the ordinary course: the app
/// checks `supports_activity` itself and shows the panel's own explanation
/// (`ActivityState::Unsupported`). This is the backstop for a caller that didn't.
const NO_SESSIONS_MSG: &str = "this connection's engine has no server sessions";

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
    /// [`schemaic_core::schema::IndexInfo::lossy`]. On MySQL that is a
    /// **functional** key part or an index the DBA has **switched off**
    /// (`INVISIBLE` on MySQL 8, `IGNORED` on MariaDB 10.6+ — see
    /// [`schemaic_core::schema::index_disabled_sql`]): the index type is read
    /// and can be re-emitted, and a prefix and a direction always could be, but
    /// no emitter here can spell either of those two.
    pub lossy: bool,
    /// The server's own whole `CREATE INDEX`, where the engine publishes one —
    /// `pg_get_indexdef` on PostgreSQL, and `None` on MySQL, which has no such
    /// accessor. See [`schemaic_core::schema::IndexInfo::create_sql`] for the
    /// one job it does.
    pub create_sql: Option<String>,
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
            // **`TABLE_TYPE` has a third answer on MariaDB.** Only `VIEW` was
            // read, so a sequence — stored as a one-row table of internal
            // counters — arrived as an editable base table. See
            // `TableInfo::is_sequence` for what that cost.
            is_sequence: ty.eq_ignore_ascii_case("SEQUENCE"),
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
                // MySQL keeps no statement per index and leaves this `None`;
                // PostgreSQL's `pg_get_indexdef` is a real one, and is what
                // lets an index the model only partly read be emitted whole.
                create_sql: r.create_sql.clone(),
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
        // Best-effort too, and for the same reason the dump writes it into the
        // file: every literal in `stmts` was written by `export::sql_literal`,
        // which doubles a backslash because that is what MySQL does with one by
        // default — and on a session carrying `NO_BACKSLASH_ESCAPES` the doubled
        // literal stores two. On a `CREATE USER … IDENTIFIED BY` that is an
        // account nobody can log in to — repairable now that the browser offers
        // a password reset, but through the same `ddl_string` this would have
        // got wrong, so the reset would store the same mangled value and the
        // escape has to be right here rather than fixable afterwards.
        //
        // **Scoped to the plan, never to the connection.** A user who sets that
        // mode means it for the SQL they *type*, and pinning it at connect time
        // would quietly change what their own statements mean. This connection
        // runs one reviewed plan and is disconnected on the way out, per the
        // one-connection-per-operation rule, so nothing here outlives the call.
        if let Some(sql) = schemaic_core::export::literal_mode_sql(self.engine.dialect()) {
            let _ = conn.query_drop(sql).await;
        }
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
        // The same literal-mode pin `run_ddl` sets, for the same reason: a
        // container plan carries literals too (a `CREATE DATABASE`'s comment,
        // a collation name), and this connection is as short-lived as that one.
        if let Some(sql) = schemaic_core::export::literal_mode_sql(self.engine.dialect()) {
            let _ = conn.query_drop(sql).await;
        }
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
        // **The cancel is inside the loop, not a race around it.** This was
        // `tokio::select!` over the whole of `import_on`, and the cancel arm
        // then *dropped* that future mid-statement — which leaves a
        // `mysql_async` connection's result stream desynchronised. Measured on
        // MariaDB 10.11.14 and MySQL 8.4.11: after the drop, `SELECT
        // CONNECTION_ID()` came back `Ok(None)`, the following `ROLLBACK`
        // "succeeded" and `SHOW WARNINGS` was empty — so the rollback below
        // classified `Complete` off a reply that was not its own, and a
        // cancelled import into a `MyISAM` table reported `DbError::Cancelled`
        // (the variant the modal renders as *"nothing was written"*) over 1,000
        // rows that were permanently there. That is the exact failure this
        // path's `Rollback::note()` was added to prevent, still live on the one
        // exit that dropped the connection out from under it.
        //
        // `import_on` now owns the token and stops at a point where the
        // protocol is intact: between batches, or after awaiting a statement it
        // killed — the same "the killed statement is awaited, not dropped"
        // rule `run_script_mysql` states.
        let outcome = import_on(
            self,
            &mut conn,
            conn_id,
            self.engine.dialect(),
            &target,
            rows,
            &cancel,
        )
        .await;
        let _ = conn.disconnect().await;
        outcome
    }
}

/// What a cancelled import reports, given what its `ROLLBACK` achieved.
///
/// **Cancelling is a write-path exit like any other, so it reports what the
/// rollback achieved.** It used to `kill_query` and disconnect, and the modal
/// then said, unconditionally, "the transaction rolled back, so nothing was
/// written" — which on `MyISAM`/`MEMORY`/`ARCHIVE`/`CSV` is false: every batch
/// already executed is durable there, so the user re-ran the import and doubled
/// the rows it had already loaded.
///
/// [`DbError::Cancelled`] is what the modal renders as "nothing was written",
/// so it is only for [`Rollback::Complete`]; anything else carries
/// [`Rollback::note`], which says the rows may still be there.
fn cancelled_import(undone: Rollback) -> DbError {
    match undone {
        Rollback::Complete => DbError::Cancelled,
        undone => DbError::Query(format!("Import cancelled{}", undone.note())),
    }
}

/// [`cancelled_import`]'s twin for a cancelled **grid write-back**, and the
/// same rule for the same reason.
///
/// Separate only because the sentence names a different act: "Import cancelled"
/// is wrong over a Commit, and the whole point of the non-`Complete` arm is
/// that the user reads it and knows what may still be in the table.
fn cancelled_write(undone: Rollback) -> DbError {
    match undone {
        Rollback::Complete => DbError::Cancelled,
        undone => DbError::Query(format!("Commit cancelled{}", undone.note())),
    }
}

/// ` ORDER BY a, b` for the Live Monitor's window, or `""` when there is no key
/// to order by. `quote` is the engine's identifier quoter, so the two callers
/// can't drift on quoting.
pub(crate) fn order_by_clause(cols: Option<&[String]>, quote: fn(&str) -> String) -> String {
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
fn next_batch_off_executor(
    rows: RowSource<'_>,
    held: &mut Option<Vec<Value>>,
) -> Result<Option<Vec<Vec<Value>>>, DbError> {
    use tokio::runtime::{Handle, RuntimeFlavor};
    match Handle::try_current().map(|h| h.runtime_flavor()) {
        Ok(RuntimeFlavor::MultiThread) => tokio::task::block_in_place(|| next_batch(rows, held)),
        _ => next_batch(rows, held),
    }
}

/// Pull the next batch of rows from the source. `Ok(None)` at the end.
///
/// **Bounded in bytes as well as rows** ([`schemaic_core::import::batch_is_full`]).
/// A row count alone let 500 rows of a 40 KiB text column build a 20 MB
/// statement, and MariaDB at its default `max_allowed_packet` answers that by
/// closing the connection — taking with it the `ROLLBACK` the caller's error arm
/// needs. The row that would cross the ceiling is **held back**, never split.
///
/// `held` is where it waits. The source is a bare `dyn Iterator`, so a row once
/// pulled cannot be put back on it; the caller owns the slot and passes it to
/// every call, which is also what makes a held row impossible to drop between
/// batches.
fn next_batch(
    rows: RowSource<'_>,
    held: &mut Option<Vec<Value>>,
) -> Result<Option<Vec<Vec<Value>>>, DbError> {
    let mut batch = Vec::with_capacity(schemaic_core::import::INSERT_BATCH_ROWS);
    let mut bytes = 0usize;
    // `next()` in a loop rather than `by_ref().take(..)`: `by_ref` isn't callable
    // on a `dyn Iterator`, and the source has to stay borrowed for the next batch.
    loop {
        let row = match held.take() {
            Some(row) => row,
            None => match rows.next() {
                Some(row) => row.map_err(DbError::Query)?,
                None => break,
            },
        };
        let size = schemaic_core::import::row_bytes(&row);
        if schemaic_core::import::batch_is_full(batch.len(), bytes, size) {
            *held = Some(row);
            break;
        }
        bytes += size;
        batch.push(row);
    }
    Ok(if batch.is_empty() { None } else { Some(batch) })
}

/// The MySQL half of [`Db::import_rows`], on an already-open connection.
async fn import_on(
    db: &Db,
    conn: &mut Conn,
    conn_id: u32,
    dialect: schemaic_core::intel::SqlDialect,
    target: &ImportTarget<'_>,
    rows: RowSource<'_>,
    cancel: &CancellationToken,
) -> Result<u64, DbError> {
    let qerr = |e: mysql_async::Error| DbError::Query(e.to_string());
    let cols: Vec<&str> = target.columns.iter().map(String::as_str).collect();
    conn.query_drop("BEGIN").await.map_err(qerr)?;

    let mut total: u64 = 0;
    // The row the byte ceiling held back from the previous batch — see
    // `next_batch`. It lives here so it cannot be lost between two of them.
    let mut held: Option<Vec<Value>> = None;
    loop {
        // **Between batches**, where the connection is idle and a `ROLLBACK`
        // is the connection's own next statement. A Stop pressed while the
        // reader is pulling rows lands here — `next_batch_off_executor` blocks
        // the task, so nothing could have observed the token earlier anyway.
        if cancel.is_cancelled() {
            return Err(cancelled_import(rollback(conn, "ROLLBACK").await));
        }
        // A reader error (a bad record, a value that wouldn't coerce) has to undo
        // the transaction too — returning straight out would leave it open until
        // the connection drops, which is a lock held for no reason.
        let batch = match next_batch_off_executor(rows, &mut held) {
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
        // **The killed statement is awaited, not dropped** — the same rule
        // `run_script_mysql` states, and for a sharper reason here: dropping it
        // desynchronises the result stream, and everything after that
        // (`ROLLBACK`, the `SHOW WARNINGS` behind `Rollback`) then reads
        // somebody else's reply. Scoped so the borrow ends before the rollback.
        let step = {
            let mut fut = std::pin::pin!(conn.query_drop(&sql));
            let raced = tokio::select! {
                r = fut.as_mut() => Some(r),
                _ = cancel.cancelled() => None,
            };
            match raced {
                Some(r) => Some(r),
                None => {
                    db.kill_query(conn_id).await;
                    let _ = fut.await;
                    None
                }
            }
        };
        match step {
            Some(Ok(())) => {}
            Some(Err(e)) => {
                let msg = e.to_string();
                let undone = rollback(conn, "ROLLBACK").await;
                return Err(DbError::Query(format!("{msg}{}", undone.note())));
            }
            None => return Err(cancelled_import(rollback(conn, "ROLLBACK").await)),
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

    // A Stop pressed after the last batch and before the commit is still a
    // Stop: committing here would land the whole import the user just stopped.
    if cancel.is_cancelled() {
        return Err(cancelled_import(rollback(conn, "ROLLBACK").await));
    }
    // **The fifth exit, and the one that had no `Rollback::note`.** Every other
    // failure here says what the rollback achieved; this one returned the
    // driver's bare sentence. On a MySQL `MyISAM`/`MEMORY`/`ARCHIVE`/`CSV`
    // target — the tables this whole apparatus exists for — a connection that
    // dies during the `COMMIT` left every row of the file durable in the table
    // while the modal showed "Server has gone away" and nothing else. Asking
    // for the rollback is what produces the answer: on a dead socket it fails
    // too, and `Rollback::note` is what turns that into a sentence about the
    // data rather than about the socket.
    if let Err(e) = conn.query_drop("COMMIT").await {
        let msg = qerr(e).to_string();
        let undone = rollback(conn, "ROLLBACK").await;
        return Err(DbError::Query(format!("{msg}{}", undone.note())));
    }
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
/// the column type.
///
/// **A cancel is an exit like any other, and leaves through an explicit
/// `ROLLBACK`.** The MySQL arm used to `kill_query`, disconnect, and return
/// `DbError::Cancelled` — relying on the drop to undo the transaction, and
/// reporting the one verdict the modal renders as "nothing was written" without
/// ever asking whether that was true. On a `MyISAM`/`MEMORY`/`ARCHIVE`/`CSV`
/// table every statement already executed is durable, so a cancelled commit of
/// three staged `INSERT`s that got two in reported "nothing was written", left
/// all three staged, and a second Commit landed the two again. That is
/// [`cancelled_import`]'s documented contract read backwards, and `import_on`
/// three hundred lines up and `pg::commit_writes` both already did it the other
/// way.
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

        // **The cancel is inside `write_on`, not a race around it** — the same
        // rule `import_rows` states three hundred lines up, for the same
        // measured reason. A `tokio::select!` over the whole write dropped that
        // future *mid-statement* (the `&mut conn` re-borrow in the cancel arm
        // only compiles because the branch futures are dropped first), which
        // leaves a `mysql_async` connection's result stream desynchronised:
        // measured there on MariaDB 10.11.14 and MySQL 8.4.11, after the drop
        // the following `ROLLBACK` "succeeded" and `SHOW WARNINGS` was empty.
        // So the `ROLLBACK` this method's doc is about classified `Complete`
        // off a reply that was not its own, and a cancelled Commit of three
        // staged `INSERT`s into a `MyISAM` table reported "nothing was written"
        // over two rows that were permanently there — the exact failure the
        // `ROLLBACK` was added to prevent, one exit further along.
        let outcome = write_on(
            &mut conn,
            write,
            TxScope::Own,
            Some((self, conn_id, &cancel)),
        )
        .await;

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
///
/// **`cancel` is handled *here*, not raced around this future.** That is the
/// rule [`Db::import_rows`] states with its measurement: a `tokio::select!`
/// around the whole write drops it mid-statement, and a dropped `mysql_async`
/// statement leaves the connection's result stream desynchronised — after which
/// the `ROLLBACK` and the `SHOW WARNINGS` behind [`Rollback`] read replies that
/// are not their own and `Complete` is claimed off garbage. Inside, the batch
/// stops between statements, or after *awaiting* a statement it killed, so the
/// rollback goes out on an intact protocol and [`cancelled_write`] can be
/// believed.
///
/// `None` for a caller that does not cancel — or that owns the cancel itself,
/// as [`crate::session::Session::commit_writes`] does, where the batch is a
/// savepoint inside the user's transaction and `classify_isolated` is what
/// answers for it.
pub(crate) async fn write_on(
    conn: &mut Conn,
    write: &GridWrite,
    scope: TxScope,
    cancel: Option<(&Db, u32, &CancellationToken)>,
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
        cancel: Option<(&Db, u32, &CancellationToken)>,
    ) -> Result<u64, DbError> {
        // **The killed statement is awaited, not dropped** — see this
        // function's doc, and `import_on`, which states the rule and the
        // measurement behind it. Scoped so the borrow ends before the rollback.
        let sent = match cancel {
            None => Some(conn.exec_drop(sql, params).await),
            Some((db, conn_id, token)) => {
                let mut fut = std::pin::pin!(conn.exec_drop(sql, params));
                let raced = tokio::select! {
                    r = fut.as_mut() => Some(r),
                    _ = token.cancelled() => None,
                };
                match raced {
                    Some(r) => Some(r),
                    None => {
                        db.kill_query(conn_id).await;
                        let _ = fut.await;
                        None
                    }
                }
            }
        };
        let Some(sent) = sent else {
            return Err(cancelled_write(rollback(conn, scope.rollback_sql()).await));
        };
        if let Err(e) = sent {
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
        // Between statements, where the protocol is whole.
        if cancel.is_some_and(|(_, _, t)| t.is_cancelled()) {
            return Err(cancelled_write(rollback(conn, scope.rollback_sql()).await));
        }
        let (sql, params) = match step {
            WriteStep::Delete(del) => build_delete(del),
            WriteStep::Update(edit) => build_update(edit),
            WriteStep::Insert(ins) => build_insert(ins),
        };
        total += one(conn, scope, sql, params, step, cancel).await?;
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
/// it. Anything unreadable resolves to [`Rollback::Unknown`] — the write path
/// must not claim more than it knows.
///
/// **`Unknown`, not `Incomplete`, and the difference is a sentence the user
/// acts on.** `Incomplete` says *this table's storage engine is not
/// transactional, so the rows already written remain* — a claim about the
/// engine, which only warning 1196 establishes. The other two routes here
/// establish nothing: a `ROLLBACK` sent down a socket that is already gone never
/// reached a server, and the server had almost certainly rolled the transaction
/// back itself when the connection dropped. Told `Incomplete`, the user audits
/// an InnoDB table that is exactly as they left it.
///
/// The unreadable-warnings route was worse than misdescribed — it was silently
/// `Complete`. `unwrap_or_default()` turns a failed `SHOW WARNINGS` into an
/// empty vector, no 1196 is found, and the write path then promised a clean
/// rollback on the strength of a query that did not run. The paragraph above
/// has claimed otherwise since it was written.
async fn rollback(conn: &mut Conn, sql: &str) -> Rollback {
    /// `ER_WARNING_NOT_COMPLETE_ROLLBACK`.
    const INCOMPLETE_ROLLBACK: u32 = 1196;
    if conn.query_drop(sql).await.is_err() {
        return Rollback::Unknown;
    }
    let Ok(warnings) = conn
        .query::<(String, u32, String), _>("SHOW WARNINGS")
        .await
    else {
        return Rollback::Unknown;
    };
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
    /// The bytes of a `BLOB` never reach a `ResultSet` on any engine (see
    /// [`schemaic_core::blob`]), so looking at one is a second, *targeted* query
    /// rather than a lookup in the loaded result. It is aimed by the same row
    /// identity a write of that row would carry, which is why a result whose
    /// binary column has no keyed base table never gets here at all —
    /// `blob_source` answers `None` and the panel is not offered.
    ///
    /// **"Never reach a `ResultSet`" is not "dropped at the wire", and the
    /// difference is a MySQL limit worth knowing.** PostgreSQL and SQLite really
    /// do leave the bytes on the server — the `SELECT` behind a grid asks for a
    /// placeholder. MySQL's does not: `convert_row` receives the whole value and
    /// *then* substitutes [`binary_display`], so the row still has to cross the
    /// wire whole. A row whose blob exceeds the server's `max_allowed_packet`
    /// (16 MiB by default on MariaDB — a **quarter** of
    /// [`schemaic_core::blob::FETCH_CAP`]) therefore fails the ordinary grid
    /// read, and this panel's 64 MiB promise is still unreachable on that engine
    /// however large the cap here is: the bound is the server's setting, not
    /// ours, and nothing here can raise it. What it is no longer is a *failure*.
    /// `PACKET_ROOM` caps this panel's own `SUBSTRING` at what one packet
    /// holds, so a value over that arrives truncated beside its true
    /// `OCTET_LENGTH` — which [`BlobValue::truncated`] already knows how to
    /// describe — rather than dropping the connection mid-row.
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

/// The most this connection can be asked for, in the server's own words.
///
/// **A row has to fit in one `max_allowed_packet`, and on MySQL the blob really
/// does cross the wire** — the row is not spared the way PostgreSQL's and
/// SQLite's are. MariaDB ships that setting at 16 MiB, a **quarter** of
/// [`FETCH_CAP`], so asking for the full 64 MiB of a large value did not fail
/// politely: measured against MariaDB 10.11 with a 20 MiB `LONGBLOB`, the
/// server dropped the connection mid-row (`ERROR 2013`), taking a
/// manual-transaction tab's pinned session and its uncommitted work with it.
///
/// Read from `@@max_allowed_packet` **inside the statement** rather than in a
/// round trip of its own, so there is no window for the setting to change
/// between the asking and the reading, and no second query on a path that is
/// one click.
///
/// The 1 MiB of headroom is for everything else in the packet — the
/// `OCTET_LENGTH` column, the row and column framing, the protocol's own
/// bookkeeping — and is deliberately generous, because being 100 bytes over
/// costs the whole connection while being 1 MiB under costs a truncation the
/// panel already knows how to describe. `GREATEST` keeps the arithmetic
/// positive on a server configured below the headroom (MariaDB's floor is
/// 1 KiB), and the `CAST` keeps the subtraction from wrapping in unsigned.
const PACKET_ROOM: &str = "GREATEST(1024, CAST(@@max_allowed_packet AS SIGNED) - 1048576)";

/// Build the `SELECT OCTET_LENGTH(c), SUBSTRING(c, 1, LEAST(?, …)) … WHERE
/// <key> <=> ? … LIMIT 1` behind [`blob_on`].
///
/// **The length and the bytes come from one row of one statement**, not two
/// queries: asked separately they can straddle another session's `UPDATE`, and
/// the pair is what [`BlobValue::truncated`] reads to decide whether saving the
/// buffer would write a file that is not the data.
///
/// `SUBSTRING` on a binary string is byte-indexed in MySQL (it is
/// character-indexed only for a character string), so the cap really is octets.
/// **Two caps, and the smaller wins**: ours ([`FETCH_CAP`], bound) and the
/// server's ([`PACKET_ROOM`], read live). Neither subsumes the other — a small
/// `max_allowed_packet` bounds a large blob, and a large one leaves `FETCH_CAP`
/// the operative limit — and a value cut by either arrives with its true
/// `OCTET_LENGTH` beside it, so [`BlobValue::truncated`] answers `true` and the
/// panel shows the prefix and refuses to save it. That is the whole point of
/// capping rather than erroring: 16 MiB of a 20 MiB value, honestly labelled,
/// beats a dropped connection.
///
/// The WHERE is `build_update`'s, NULL-safe `<=>` and all — the identity of a
/// row is one thing on this path, whether it is being written or read.
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
        "SELECT OCTET_LENGTH({col}), SUBSTRING({col}, 1, LEAST(?, {PACKET_ROOM})) \
         FROM {}.{} WHERE {where_sql} LIMIT 1",
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
        // **Before the collect**, which consumes the result and empties
        // `columns_ref`. The declared fractional precision is only on the wire.
        let scale = fractional_scales(result.columns_ref());
        // **This is the path that needs it.** A prepared statement sends a
        // `ZEROFILL` column as a bare integer, so the splice would otherwise
        // paint `7` over the `0007` the load read — see `zerofill_value`.
        let zerofill = zerofill_widths(result.columns_ref());
        let fetched: Vec<Row> = result.collect::<Row>().await.map_err(qerr)?;
        if let Some(r) = fetched.first() {
            let binary: Vec<bool> = columns.iter().map(Column::is_binary).collect();
            let bit: Vec<bool> = columns
                .iter()
                .map(|c| schemaic_core::model::type_is_bit(&c.type_name))
                .collect();
            let kinds: Vec<NumKind> = columns.iter().map(|c| num_kind(&c.type_name)).collect();
            out.push((
                row.data_row,
                convert_row(r, &columns, &binary, &bit, &scale, &kinds, &zerofill),
            ));
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
    // Key first, then the confirming columns — the order `edit::refetch_key`
    // builds the values in, and the reason they are here at all is
    // `RefetchTemplate::confirm_cols`: the write's own `WHERE` carries them and
    // this copy dropped them.
    let where_sql = template
        .key_cols
        .iter()
        .chain(template.confirm_cols.iter())
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
pub(crate) fn ident(name: &str) -> String {
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
    // **A `ZEROFILL` column's text *is* its value.** The server pads it to the
    // declared display width — `INT(4) UNSIGNED ZEROFILL` holding 7 arrives as
    // `0007` — and parsing that as a number drops the padding in the grid and
    // in every export. `Text` keeps the server's own rendering, the way
    // `DECIMAL` already does; the column still right-aligns, because
    // `Column::is_numeric` reads the leading type token and ignores the
    // suffixes.
    if t.contains("ZEROFILL") {
        return NumKind::Text;
    }
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

/// Re-render a **binary-protocol** number the way the text protocol would have
/// sent it, for a `ZEROFILL` column — `width` is the column's display width, or
/// `None` for every other column, which passes through untouched. `scale` is the
/// column's declared decimals, which a float needs and an integer ignores.
///
/// **The two protocols disagree about these cells and only one of them is the
/// user's answer.** A prepared statement — which is what the post-commit
/// re-fetch runs — sends `Int(7)` for the cell `SELECT` sent as `0007`
/// (measured on MariaDB 10.11.14 and MySQL 8.4). Without this the splice would
/// paint `7` over the padded value the load put in the grid, and the column
/// would disagree with itself about a cell the user never edited.
///
/// **Floats are `ZEROFILL` columns too**, and covering the integer arms alone
/// left exactly that disagreement on them. MySQL's numeric-type syntax admits
/// the attribute on `FLOAT[(M,D)]` and `DOUBLE[(M,D)]`, and `zerofill_widths`
/// answers `Some` for every numeric column carrying the flag — so the width was
/// computed, carried into `convert_row`, and consumed by two of the arms that
/// could use it. Measured on MariaDB 10.11.14 **and** MySQL 8.4.11, where the
/// form is deprecated but still accepted: `DOUBLE(10,2) UNSIGNED ZEROFILL`
/// holding `123.45` reads back `0000123.45` over the text protocol, and
/// `FLOAT(8,2) UNSIGNED ZEROFILL` holding `12.5` reads `00012.50`. The splice
/// painted `123.45` over the first of those, in a cell the user never touched,
/// and an export taken before the next full re-run wrote the unpadded text out.
///
/// The scale is part of it: the server pads to `width` *including* the decimal
/// point and the fraction, so `{n:0w$.p$}` is the same rendering.
///
/// Padding only, never truncation: a value wider than the declared width is the
/// server's to render, and `format!` leaves it alone.
pub(crate) fn zerofill_value(v: Value, width: Option<usize>, scale: u32) -> Value {
    let Some(w) = width else {
        return v;
    };
    let p = scale as usize;
    match v {
        Value::Int(n) => Value::Str(format!("{n:0w$}")),
        Value::UInt(n) => Value::Str(format!("{n:0w$}")),
        // **`NOT_FIXED_DEC` is not a scale.** A `FLOAT`/`DOUBLE` declared
        // without `(M,D)` reports `decimals = 31`, and formatting to
        // thirty-one places would invent digits the server never sent. There
        // the shortest round-tripping form is what the text protocol prints,
        // padded — the same reading `binary_as_text` takes for an `f32`.
        Value::Float(f) if p >= 31 => Value::Str(format!("{:0>w$}", f.to_string())),
        Value::Float(f) => Value::Str(format!("{f:0w$.p$}")),
        other => other,
    }
}

/// Per-column display width for the `ZEROFILL` columns of a result, `None` for
/// every other column — computed **once for the result**, like
/// [`fractional_scales`] beside it, because it is a fact about the column.
pub(crate) fn zerofill_widths(columns: &[MyColumn]) -> Vec<Option<usize>> {
    columns
        .iter()
        .map(|c| {
            c.flags()
                .contains(ColumnFlags::ZEROFILL_FLAG)
                .then(|| c.column_length() as usize)
        })
        .collect()
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
    // **Tests here reaching into the engine module, which is the wrong
    // direction** — and temporary, like the production import at the top of this
    // file. The decoder tests that need these sit interleaved with tests for
    // `value_to_param`, `build_refetch_sql` and `build_blob_select`, write-back
    // builders that have not moved yet; the whole block goes across with them
    // rather than being split by hand now and again later. When it does, this
    // import is what fails.
    use crate::mysql::{binary_as_text, resolve_type_name};
    use mysql_async::consts::ColumnType;

    /// **A fourth engine variant is a compiler error; a fourth engine module is
    /// not.** [`ENGINE_ENTRY_POINTS`] is the interface the dispatcher expects by
    /// name, and this is the only thing that notices a module answering thirteen
    /// of it — the case the crate doc names is a `sqlite.rs` peer with no
    /// `fetch_blob`, which compiles until somebody writes that dispatch arm.
    ///
    /// Reading the source is the subject because the thing under test is a
    /// *file*, exactly as `core/tests/doc_coverage.rs` reads `src/*.rs`. MySQL
    /// is absent on purpose: it has no module, which is the asymmetry rather
    /// than an omission.
    #[test]
    fn every_engine_module_answers_the_whole_interface() {
        for (name, src) in [
            ("pg.rs", include_str!("pg.rs")),
            ("sqlite.rs", include_str!("sqlite.rs")),
        ] {
            for f in ENGINE_ENTRY_POINTS {
                let sync = format!("\npub(crate) fn {f}(");
                let asyn = format!("\npub(crate) async fn {f}(");
                assert!(
                    src.contains(&sync) || src.contains(&asyn),
                    "{name} does not answer `{f}` — the engine interface is a \
                     naming convention (see `ENGINE_ENTRY_POINTS`), so a module \
                     that omits one compiles cleanly until the dispatch arm for \
                     it is written"
                );
            }
        }
    }

    /// And the dispatcher reaches all three engines for each of them, so the
    /// convention is not a list of names nothing calls.
    #[test]
    fn the_dispatcher_calls_both_engine_modules_for_every_entry_point() {
        let me = include_str!("lib.rs");
        for f in ENGINE_ENTRY_POINTS {
            for module in ["pg", "sqlite"] {
                assert!(
                    me.contains(&format!("{module}::{f}(")),
                    "nothing in the dispatcher calls `{module}::{f}` — either the \
                     arm is missing or `ENGINE_ENTRY_POINTS` has a name the \
                     interface no longer has"
                );
            }
        }
    }

    /// **A `Db` must not print its password**, in any formatting, ever.
    ///
    /// The derive did, and no site formats one today — which is what made it
    /// latent rather than live. This type is cloned into `McpEndpoint`,
    /// `StartAiParams`, the dump and script runners, so the day a struct owning
    /// one gains a `#[derive(Debug)]` and is logged, the credential lands in
    /// `%APPDATA%\Roaming\schemaic`'s log — the folder the Settings pane
    /// invites the user to share for support. The struct's own doc already
    /// claimed "no plaintext URL is embedded anywhere as identity or leaked on
    /// a command line"; the invariant behind it says *URL, argv or log*.
    ///
    /// It is also the test a future field keeps honest: a hand-written `Debug`
    /// has to be extended for a new secret, and this says what happens if it is
    /// not.
    #[test]
    fn a_db_never_prints_its_password() {
        let db = Db::from_parts(
            Engine::MySql,
            "db.internal".into(),
            3306,
            "app".into(),
            "hunter2".into(),
            String::new(),
        );
        let shown = format!("{db:?}");
        assert!(!shown.contains("hunter2"), "{shown}");
        assert!(shown.contains("<redacted>"), "{shown}");
        // Everything a reader needs to say *which* endpoint this is still
        // prints — a `Debug` that redacted the host would be no use at all.
        assert!(shown.contains("db.internal"), "{shown}");
        assert!(shown.contains("3306"), "{shown}");
        assert!(shown.contains("app"), "{shown}");
    }

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

    // ── the two protocols, and the one row read through both ────────────

    /// **The re-fetch reads the same row over a different protocol**, and its
    /// answer is spliced straight over the cells on screen. `collect_rows` uses
    /// `query_iter` (text), where every value is `Bytes`; `refetch_on` uses
    /// `exec_iter` (binary, the statement being prepared with the key bound),
    /// where MySQL sends `DATETIME` as `Date`, `TIME` as `Time` and `FLOAT` as an
    /// `f32`. Those fell to `convert_row`'s catch-all, `MyValue::as_sql` — a
    /// **SQL literal**, not the text form — which prints a `Date` with a zero
    /// time as `'YYYY-MM-DD'` and a `Time` as `'{:03}:{:02}:{:02}'`.
    ///
    /// Measured on MariaDB 10.11.14 and MySQL 8.4.11: editing one column of
    /// `(1, 'a', '2024-01-15 00:00:00', '10:30:00', 3.14)` spliced the row back
    /// with `2024-01-15`, `010:30:00` and `3.140000104904175` in three cells
    /// nobody touched. Re-running the query restored them, so the grid disagreed
    /// with itself about one row, and any export taken in between wrote the wrong
    /// text.
    #[test]
    fn a_binary_temporal_reads_back_as_the_text_protocol_wrote_it() {
        // The zero time a `DATETIME` carries and `as_sql` drops.
        assert_eq!(
            binary_as_text(&MyValue::Date(2024, 1, 15, 0, 0, 0, 0), "DATETIME", 0).as_deref(),
            Some("2024-01-15 00:00:00")
        );
        // A bare `DATE` has no time to print, and must not grow one.
        assert_eq!(
            binary_as_text(&MyValue::Date(2024, 1, 15, 0, 0, 0, 0), "DATE", 0).as_deref(),
            Some("2024-01-15")
        );
        // `TIME` is a duration: `as_sql`'s `{:03}` made this `010:30:00`.
        assert_eq!(
            binary_as_text(&MyValue::Time(false, 0, 10, 30, 0, 0), "TIME", 0).as_deref(),
            Some("10:30:00")
        );
        // …which runs past a day, and backwards.
        assert_eq!(
            binary_as_text(&MyValue::Time(false, 3, 2, 0, 0, 0), "TIME", 0).as_deref(),
            Some("74:00:00")
        );
        assert_eq!(
            binary_as_text(&MyValue::Time(true, 0, 1, 2, 3, 0), "TIME", 0).as_deref(),
            Some("-01:02:03")
        );
        // An `f32` widened to `f64` is what produced `3.140000104904175`; the
        // value here is one whose widening is visible without being a constant
        // clippy recognises — `0.1f32 as f64` is `0.10000000149011612`.
        assert_eq!(
            binary_as_text(&MyValue::Float(0.1), "FLOAT", 0).as_deref(),
            Some("0.1")
        );
        assert_ne!(
            (0.1f32 as f64).to_string(),
            "0.1",
            "if this ever holds, the widening was never the bug"
        );
        // A `DOUBLE` was already right and stays out of this.
        assert_eq!(binary_as_text(&MyValue::Double(1.25), "DOUBLE", 0), None);
        assert_eq!(binary_as_text(&MyValue::Int(7), "INT", 0), None);
    }

    /// **The declared precision, and only that.** A `DATETIME(3)` reads
    /// `…:00.120` and a bare `DATETIME` reads `…:00`, so the fraction cannot come
    /// from the value — the binary protocol always sends microseconds, and
    /// `.120` trimmed of trailing zeros would be `.12`. It comes off the wire's
    /// column definition, which is also the only place it is: `type_name_of`
    /// builds `DATETIME` from the type code with no precision in it.
    #[test]
    fn the_fraction_follows_the_columns_declared_precision() {
        let at = |scale| {
            binary_as_text(
                &MyValue::Date(2024, 1, 15, 8, 9, 10, 120_000),
                "DATETIME",
                scale,
            )
            .expect("a temporal")
        };
        assert_eq!(at(0), "2024-01-15 08:09:10");
        assert_eq!(at(3), "2024-01-15 08:09:10.120");
        assert_eq!(at(6), "2024-01-15 08:09:10.120000");
        // MySQL's own maximum, so a server answering more does not widen it.
        assert_eq!(at(9), "2024-01-15 08:09:10.120000");
        assert_eq!(
            binary_as_text(&MyValue::Time(false, 0, 10, 30, 0, 500_000), "TIME", 1).as_deref(),
            Some("10:30:00.5")
        );
    }

    /// **A hoisted kind has to answer what the per-cell call answered.**
    ///
    /// `convert_row` called `parse_typed` per cell, which is
    /// `parse_as(num_kind(type_name), s)` — and `num_kind` opens by
    /// uppercasing the type name, then walks up to eight `starts_with` scans
    /// and a `contains`, for a property of the *column*. Both docs already
    /// said so ("Called once per column"; "What a row loop should call is
    /// `parse_as` with a kind it computed once"), and the two sibling
    /// classifications beside it in the row loop were hoisted on exactly that
    /// reasoning.
    ///
    /// The answer must not move, so this is the equivalence: over every type
    /// spelling the mapping distinguishes, `parse_as(num_kind(t), s)` is
    /// `parse_typed(s, t)`.
    #[test]
    fn a_kind_computed_once_parses_a_cell_the_way_the_per_cell_call_did() {
        let types = [
            "TINYINT",
            "SMALLINT",
            "MEDIUMINT",
            "INT",
            "BIGINT",
            "YEAR",
            "INT UNSIGNED",
            "BIGINT UNSIGNED",
            "tinyint unsigned",
            "FLOAT",
            "DOUBLE",
            "DECIMAL(10,2)",
            "VARCHAR(255)",
            "TEXT",
            "DATETIME",
            "",
        ];
        let cells = ["42", "-1", "0", "3.5", "18446744073709551615", "abc", ""];
        for t in types {
            let kind = num_kind(t);
            for c in cells {
                assert_eq!(
                    parse_as(kind, c.to_string()),
                    parse_typed(c.to_string(), t),
                    "{t:?} / {c:?}"
                );
            }
        }
    }

    /// A column index past the end falls back to `Text`, which is what a
    /// value with no column to describe it has to be — the row loop indexes
    /// `kinds` the same way it indexes `binary` and `bit`, both of which take
    /// the same defensive default.
    #[test]
    fn a_missing_kind_is_text() {
        let kinds: Vec<NumKind> = vec![NumKind::Int];
        assert_eq!(
            kinds.get(9).copied().unwrap_or(NumKind::Text),
            NumKind::Text
        );
        assert!(matches!(
            parse_as(NumKind::Text, "42".to_string()),
            Value::Str(_)
        ));
    }

    /// **A `ZEROFILL` column's padding is the server's own rendering, and it is
    /// the value the user sees in `mysql` and in DataGrip.** `INT(4) UNSIGNED
    /// ZEROFILL` holding 7 arrives over the text protocol as `0007`
    /// (**measured** on MariaDB 10.11.14 and MySQL 8.4 — both send the padded
    /// bytes and set `ZEROFILL_FLAG` with the display width in
    /// `column_length`), and parsing it as a number threw the padding away in
    /// the grid and in every export, on the path whose doc promises the cell
    /// keeps its exact text.
    ///
    /// `Text` costs nothing here that matters: `Column::is_numeric` reads the
    /// **leading** type token, so the column still right-aligns, and a
    /// fixed-width zero-padded unsigned sorts identically as text.
    #[test]
    fn a_zerofill_column_keeps_the_padding_the_server_sent() {
        assert_eq!(num_kind("INT(4) UNSIGNED ZEROFILL"), NumKind::Text);
        assert_eq!(num_kind("BIGINT(8) UNSIGNED ZEROFILL"), NumKind::Text);
        assert!(matches!(
            parse_typed("0007".into(), "INT(4) UNSIGNED ZEROFILL"),
            Value::Str(s) if s == "0007"
        ));
        // Without the attribute nothing changes: an ordinary unsigned integer is
        // still a number, which is what the hoisted kind array is mostly for.
        assert_eq!(num_kind("INT UNSIGNED"), NumKind::UInt);
        assert_eq!(num_kind("INT"), NumKind::Int);
    }

    /// The wire flag has to reach the type name, or `num_kind` above cannot see
    /// it — the type name is the only thing it is given.
    #[test]
    fn resolve_type_name_carries_the_zerofill_attribute() {
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_LONG, true, false, true),
            "INT UNSIGNED ZEROFILL"
        );
        // ZEROFILL implies UNSIGNED on the server, but the flags are
        // independent on the wire; the name is built from what arrived.
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_LONGLONG, false, false, true),
            "BIGINT ZEROFILL"
        );
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_LONG, true, false, false),
            "INT UNSIGNED"
        );
        // Non-numeric types never carry it, the way they never carry UNSIGNED.
        assert_eq!(
            resolve_type_name(ColumnType::MYSQL_TYPE_VAR_STRING, true, false, true),
            "VARCHAR"
        );
    }

    /// **The other protocol, which sends no padding at all.** The post-commit
    /// re-fetch runs a *prepared* statement, and the binary protocol carries an
    /// integer: `Int(7)` for the same cell the text protocol sent as `0007`
    /// (measured on both engines). Left alone, the splice would paint `7` over
    /// the `0007` the load put there and the grid would disagree with itself
    /// about a cell nobody edited — so the re-fetch re-renders it the way the
    /// server would have, from the display width the wire also carries.
    #[test]
    fn a_zerofill_cell_from_the_binary_protocol_is_padded_back() {
        assert!(matches!(
            zerofill_value(Value::Int(7), Some(4), 0),
            Value::Str(s) if s == "0007"
        ));
        assert!(matches!(
            zerofill_value(Value::UInt(42), Some(8), 0),
            Value::Str(s) if s == "00000042"
        ));
        // A value already at or over the width is untouched by the padding.
        assert!(matches!(
            zerofill_value(Value::UInt(12345), Some(4), 0),
            Value::Str(s) if s == "12345"
        ));
        // No width means no ZEROFILL: every other column passes through whole.
        assert!(matches!(
            zerofill_value(Value::Int(7), None, 0),
            Value::Int(7)
        ));
        assert!(matches!(
            zerofill_value(Value::Str("0007".into()), Some(4), 0),
            Value::Str(s) if s == "0007"
        ));
        assert!(matches!(
            zerofill_value(Value::Null, Some(4), 0),
            Value::Null
        ));
    }

    /// **A float is a `ZEROFILL` column too**, and the first spelling of this
    /// covered the integer arms alone — so `zerofill_widths`, which answers
    /// `Some` for every numeric column carrying the flag, computed a width that
    /// two of the three arms that could use it threw away. The splice then
    /// painted `123.45` over the `0000123.45` the text load had put in the grid,
    /// in a cell the user never touched, and an export taken before the next
    /// full re-run wrote the unpadded text out.
    ///
    /// Measured on MariaDB 10.11.14 **and** MySQL 8.4.11, where the `(M,D)` form
    /// is deprecated and still accepted: `DOUBLE(10,2) UNSIGNED ZEROFILL`
    /// holding `123.45` reads back `0000123.45`, and `FLOAT(8,2) UNSIGNED
    /// ZEROFILL` holding `12.5` reads `00012.50`.
    #[test]
    fn a_zerofill_float_is_padded_to_its_declared_scale() {
        assert!(matches!(
            zerofill_value(Value::Float(123.45), Some(10), 2),
            Value::Str(s) if s == "0000123.45"
        ));
        // The scale is part of the rendering: a trailing zero the server prints
        // is not noise, it is the column's declared precision.
        assert!(matches!(
            zerofill_value(Value::Float(12.5), Some(8), 2),
            Value::Str(s) if s == "00012.50"
        ));
        // **`NOT_FIXED_DEC` is not a scale.** A float declared without `(M,D)`
        // reports `decimals = 31`; formatting to thirty-one places would invent
        // digits the server never sent, so the shortest round-tripping form is
        // padded instead.
        assert!(matches!(
            zerofill_value(Value::Float(1.5), Some(6), 31),
            Value::Str(s) if s == "0001.5"
        ));
        // And no width is still no ZEROFILL.
        assert!(matches!(
            zerofill_value(Value::Float(1.5), None, 2),
            Value::Float(f) if f == 1.5
        ));
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
            confirm_cols: Vec::new(),
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
            confirm_cols: Vec::new(),
        };
        assert_eq!(
            build_refetch_sql(&t),
            "SELECT `a`, `b`, `c` FROM `db`.`t` WHERE `a` <=> ? AND `c` <=> ? LIMIT 1"
        );
    }

    /// **The confirming half of the `WHERE`, which no test reached.**
    /// `confirm_cols` is populated only by SQLite's implicit-rowid key
    /// (`sqlite.rs`'s `implicit_key`), so on this builder it is empty in every
    /// round trip the suite runs and the `.chain(confirm_cols)` above could be
    /// deleted with the whole suite green. The chain is still this builder's
    /// contract — the placeholders it writes are bound in
    /// `edit::refetch_key`'s order, key first — so it is asserted here
    /// directly, with a template that has both halves.
    #[test]
    fn build_refetch_sql_confirms_with_the_columns_after_the_key() {
        let t = RefetchTemplate {
            database: "db".to_string(),
            schema: None,
            table: "t".to_string(),
            columns: vec!["rowid".to_string(), "a".to_string(), "b".to_string()],
            key_cols: vec![0],
            confirm_cols: vec![1, 2],
        };
        assert_eq!(
            build_refetch_sql(&t),
            "SELECT `rowid`, `a`, `b` FROM `db`.`t` \
             WHERE `rowid` <=> ? AND `a` <=> ? AND `b` <=> ? LIMIT 1"
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
            confirm_cols: Vec::new(),
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
            "SELECT OCTET_LENGTH(`picture`), SUBSTRING(`picture`, 1, \
             LEAST(?, GREATEST(1024, CAST(@@max_allowed_packet AS SIGNED) - 1048576))) \
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
            "SELECT OCTET_LENGTH(`c``c`), SUBSTRING(`c``c`, 1, \
             LEAST(?, GREATEST(1024, CAST(@@max_allowed_packet AS SIGNED) - 1048576))) \
             FROM `d``b`.`t``t` WHERE `k``k` <=> ? LIMIT 1"
        );
    }

    /// **The server's limit is the other cap, and the smaller of the two wins.**
    /// `FETCH_CAP` stays bound as a parameter — the statement must not carry a
    /// second literal — while `max_allowed_packet` is read live inside the
    /// statement, because it is the server's answer and it can change under a
    /// long-lived connection.
    ///
    /// Asking for the full 64 MiB unconditionally is not a polite failure on
    /// MySQL: measured against MariaDB 10.11 (`max_allowed_packet` = 16 MiB) a
    /// 20 MiB `LONGBLOB` dropped the connection mid-row. The cap turns that into
    /// a truncated read the panel already describes.
    #[test]
    fn build_blob_select_bounds_the_read_by_the_servers_packet_limit() {
        let (sql, params) = build_blob_select(&blob_ref(&[("id", Value::Int(1))]));
        assert!(
            sql.contains(
                "LEAST(?, GREATEST(1024, CAST(@@max_allowed_packet AS SIGNED) - 1048576))"
            ),
            "the two caps must both be in the length, smaller winning: {sql}"
        );
        assert!(
            !sql.contains(&FETCH_CAP.to_string()),
            "our cap is bound, not written into the statement: {sql}"
        );
        let Params::Positional(p) = params else {
            panic!("positional params expected");
        };
        assert_eq!(
            p[0],
            MyValue::UInt(FETCH_CAP as u64),
            "and it is still the first parameter, ahead of the key"
        );
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

    /// One key column of an index, as a fetch produces it (`non_unique` is the
    /// catalogue's sense: 1 means not unique).
    ///
    /// **`cr` and `ir` stay here because `assemble_schema` does**, and it does
    /// because `pg.rs` calls it: a test for a function two engines share belongs
    /// beside the function, not in one engine's module, where the next reader
    /// would take it for MySQL's. They travelled to `mysql.rs` with the
    /// introspection and came back; the compiler is what noticed, by reporting
    /// them unused at both ends. Only `s` is genuinely wanted in both places,
    /// and a four-line `to_string` wrapper is cheaper twice than as API.
    fn ir(table: &str, index: &str, non_unique: i64, col: &str) -> IdxRow {
        IdxRow {
            table: s(table),
            index: s(index),
            unique: non_unique == 0,
            column: schemaic_core::schema::IndexColumn::plain(col),
            method: None,
            predicate: None,
            lossy: false,
            create_sql: None,
        }
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

    /// This crate's three identifier quoters answer to `core`'s, so the SQL a
    /// write path builds can't drift from the SQL the export and DDL paths
    /// **A cancelled write reports `Cancelled` only when the rollback really
    /// undid it**, which is the same rule `cancelled_import` states and the
    /// grid's commit did not follow.
    #[test]
    fn a_cancelled_write_says_so_only_when_the_rollback_was_complete() {
        assert!(matches!(
            cancelled_write(Rollback::Complete),
            DbError::Cancelled
        ));
        match cancelled_write(Rollback::Incomplete) {
            DbError::Query(msg) => {
                assert!(msg.starts_with("Commit cancelled"), "{msg}");
                assert!(
                    msg.contains("remain"),
                    "the note that says the rows may still be there is missing: {msg}"
                );
            }
            other => panic!("an incomplete undo reported as {other:?}"),
        }
        // And it names the right act: "Import cancelled" over a Commit is the
        // sentence `cancelled_import` would have given.
        assert!(
            !format!("{:?}", cancelled_write(Rollback::Incomplete)).contains("Import"),
            "a cancelled commit reported as a cancelled import"
        );
    }

    /// **And the composition, which the test above cannot reach.**
    ///
    /// `DbError::Cancelled` is what the modal renders as "nothing was written",
    /// so the whole question is whether the cancel arm *asks*. It did not: it
    /// killed the query, disconnected, and returned `Cancelled` on the strength
    /// of the connection drop undoing the transaction — true on InnoDB and
    /// false on `MyISAM`/`MEMORY`/`ARCHIVE`/`CSV`, where a cancelled commit of
    /// three staged `INSERT`s that got two in reported "nothing was written",
    /// left all three staged, and a second Commit landed the two again.
    ///
    /// A source gate for `import_on`'s reason one function up: every arm of
    /// `commit_writes` needs a live MySQL connection to reach, and a unit test
    /// of `cancelled_write` alone is green against the defect — the seam is
    /// between the predicate and its caller, which is the shape CLAUDE.md's
    /// testing section names.
    ///
    /// **And the half that gate could not see: *where* the cancel happens.**
    /// Asking `rollback` is worth nothing if the connection cannot hear the
    /// answer, and a `tokio::select!` around the whole write is exactly that —
    /// the cancel arm runs only after the branch futures are dropped, which is
    /// why its `&mut conn` re-borrow compiles at all, so the killed statement is
    /// dropped rather than awaited and the stream is torn. `import_rows` removed
    /// that construct three hundred lines up for this reason and recorded the
    /// measurement; `commit_writes` kept it and then put a `ROLLBACK` on the
    /// torn connection, which is how `Complete` came to be claimed off a reply
    /// that was not its own.
    #[test]
    fn the_grid_commits_cancel_arm_goes_through_a_rollback() {
        let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"))
            .expect("this module's own source");
        let at = src
            .find("    pub async fn commit_writes(")
            .expect("`commit_writes` is gone or was renamed");
        let body = &src[at..at + 2400];
        // The write itself is not raced. Assembled, so this paragraph is not the
        // hit.
        let raced = format!("{}! {{", "tokio::select");
        assert!(
            !body.contains(&raced),
            "`commit_writes` races the write future again: the cancel arm drops \
             it mid-statement, which desynchronises the connection's result \
             stream, and the `ROLLBACK` below then reads somebody else's reply. \
             Hand the token into `write_on`, the way `import_rows` does."
        );
        // …and the cancel still leaves through a rollback whose outcome is asked
        // for, which is now `write_on`'s job.
        let wo = src
            .find("pub(crate) async fn write_on(")
            .expect("`write_on` is gone or was renamed");
        let wob = &src[wo..wo + 3000];
        assert!(
            wob.contains("rollback(") && wob.contains("cancelled_write("),
            "the cancel path returns without asking what the rollback achieved, \
             so it claims 'nothing was written' about a MySQL table that may \
             hold half the batch"
        );
        assert!(
            wob.contains("db.kill_query(conn_id).await") && wob.contains("let _ = fut.await;"),
            "the killed statement is not awaited, so the next statement reads \
             its reply — see `import_on`, which states the rule"
        );
    }

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
