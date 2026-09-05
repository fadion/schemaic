//! PostgreSQL backend (second engine), built on [`tokio_postgres`].
//!
//! Dispatched to from [`crate::Db`]'s public methods when the connection's engine
//! is [`crate::Engine::Postgres`]. Full parity with the MySQL path: connect, list
//! databases, run queries/batches, introspect schema, non-executing validation
//! (`prepare_check`), EXPLAIN, the Live Monitor (`fetch_table`), and transactional
//! write-back (`commit_writes`/`refetch_rows`) with the same 1-row safety net.
//!
//! **Values come back over the simple-query (text) protocol**, *streamed*
//! (`simple_query_raw`): every cell arrives as its textual form, so `NUMERIC`,
//! `UUID`, arrays, and any exotic type round-trip losslessly without a per-type
//! decoder — mirroring the MySQL text-protocol path (`crate::parse_typed`).
//! Column *types* are obtained from a non-executing `PREPARE`
//! (`Client::prepare`) so the grid still gets type names (and zero-row `SELECT`s
//! still report their columns).
//!
//! Use `simple_query_raw` rather than `simple_query` for anything row-bearing:
//! the latter is the former plus `try_collect()`, so it materializes the entire
//! result before the row cap can apply. Rows stream into the columnar
//! `ResultBuilder` and the stream is dropped at the cap — safe on a reused
//! connection because tokio-postgres' connection task keeps paging through a
//! hung-up receiver's remaining messages, so the protocol stays in sync. (The
//! bounded helpers — schema introspection, `refetch_rows` — keep using
//! `simple_query`; their size is the schema's, not the user's.)
//!
//! **Column provenance** (`ColumnOrigin`, driving grid editability) is resolved
//! from each prepared column's `table_oid`/`column_id` via a `pg_catalog` lookup
//! (`fetch_col_meta`) — the Postgres analog of MySQL's `org_table`/`org_name` +
//! key flags. Expression columns carry `origin: None`.
//!
//! **Model note:** a PostgreSQL *database* maps onto the app's "database" tree
//! level (mirroring a MySQL schema). Within a database, **every user namespace**
//! is introspected (see `user_schema_filter`) and each table carries its
//! `TableInfo::schema`; the UI adds a schema tree level only when a database has
//! more than one, so a `public`-only database looks exactly as it always did.
//! Cross-*database* references remain out of scope — PostgreSQL itself doesn't
//! support them.
//!
//! **Two qualification rules, on purpose.** User-facing SQL (the editor's
//! open-table statement, FK-follow, DDL) uses `schemaic_core::schema::
//! sql_qualifier`, which drops `public` so single-schema statements stay clean.
//! The write path (`commit_writes`/`refetch_rows`/`fetch_table`) uses `pg_qname`,
//! which qualifies **always** — that SQL is never shown and must not resolve
//! through `search_path`.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use schemaic_core::activity::{self, KillKind, SessionInfo};
use schemaic_core::blob::{BlobRef, BlobValue, FETCH_CAP};
use schemaic_core::intel::SqlDialect;
use schemaic_core::model::{
    CellEdit, Column, ColumnFlags, ColumnOrigin, GridWrite, RefetchRow, RefetchTemplate,
    ResultBuilder, ResultSet, Rollback, RowDelete, RowEdit, RowInsert, Value, WriteStep,
    binary_display, one_row_verdict,
};
use schemaic_core::schema::{
    CheckInfo, ColumnInfo, DbSchema, DomainInfo, EnumInfo, IndexColumn, RoutineInfo, RoutineKind,
    SequenceInfo, SequenceOwner, TableInfo, TriggerAction, TriggerEnabled, TriggerEvent,
    TriggerInfo, TriggerLevel, TriggerTiming, ViewOptions, Volatility,
};
use schemaic_core::sql;
use schemaic_core::stats::{Freshness, IndexStats, SchemaStats, TableStats};
use schemaic_core::users::{self, Grants, PgAclRow, PgObjectKind, PgRoleRow, Principal};
use tokio_postgres::error::SqlState;
use tokio_postgres::types::Type;
use tokio_postgres::{Client, Config, NoTls, SimpleQueryMessage};
use tokio_util::sync::CancellationToken;

use crate::{Db, DbError, FkColRow, TxScope, assemble_schema};

/// This module's dialect, once. Every boundary scan here is PostgreSQL's — the
/// engine the file exists for — and spelling it out at each call site is what
/// makes a scan easy to write without one.
const PG: SqlDialect = SqlDialect::Postgres;

/// Open a fresh connection to a specific PostgreSQL database. Unlike MySQL,
/// PostgreSQL requires connecting to a concrete database to run any statement.
/// The background connection driver is spawned onto the tokio runtime and lives
/// until the returned [`Client`] is dropped.
pub(crate) async fn connect_to(db: &Db, database: &str) -> Result<Client, DbError> {
    connect_probe(db, database).await.map_err(|(e, _)| e)
}

/// [`connect_to`], with the extra bit [`connect_maintenance`] needs: whether
/// trying a **different database** could plausibly succeed.
///
/// Without it the probe repeats a doomed connect once per candidate. Against
/// Aiven that meant four TLS handshakes across Europe to learn one thing the
/// first already knew, inside a budget bounded for the whole sequence — so a
/// certificate the user had misconfigured reported "timed out" instead of
/// naming the certificate.
async fn connect_probe(db: &Db, database: &str) -> Result<Client, (DbError, bool)> {
    let mut cfg = Config::new();
    cfg.host(&db.host)
        .port(db.port)
        .user(&db.user)
        .password(&db.pass)
        .dbname(database)
        // **The one session setting this module's SQL depends on, pinned rather
        // than assumed.** `pg_str_lit` doubles quotes and nothing else, and
        // `pg_cell_lit` sends bytes as `decode('…','hex')` — both are safe
        // exactly while `standard_conforming_strings` is on, which is the
        // default and is settable per database and per role. With it off a
        // backslash in a row's own text data ends the literal, and the rest of
        // that value is parsed as SQL: on `blob_on`'s `simple_query`, which
        // accepts several statements, that is a second statement running from a
        // single click on a connection marked read-only.
        //
        // Pinning it is the belt, not the fix — the values still reach the
        // server inside statement text — but it removes the dependency the same
        // module already refuses to take twice, in `roles`' prefix comparison
        // and in `pg_cell_lit`'s own seventeen-line argument for `decode`.
        // Sent on the startup packet, so it is in force before the first
        // statement and cannot be undone by anything the app runs.
        .options("-c standard_conforming_strings=on")
        // `Prefer` here is what implements the mode of the same name: unlike
        // MySQL, the Postgres driver negotiates the downgrade itself, so no
        // retry of ours is involved and `require` upward cannot fall back.
        .ssl_mode(crate::tls::pg_ssl_mode(db.tls_plan()));

    // The two arms differ only in the connector's *type*, which is why they
    // cannot be one expression.
    let connected = match db.tls_plan() {
        None => cfg.connect(NoTls).await.map(spawn_connection),
        Some(plan) => {
            // A TLS setup failure is ours, not the server's — no database
            // changes it.
            let connector = crate::tls::pg_connector(plan).map_err(|e| (e, false))?;
            cfg.connect(connector).await.map(spawn_connection)
        }
    };

    connected.map_err(|e| {
        let retry = another_database_might_work(e.code());
        (DbError::Connect(error_chain(&e)), retry)
    })
}

/// Is this failure about the *database* we asked for, rather than the server,
/// the transport or the user?
///
/// Five codes qualify and no others. `3D000` is the database not existing.
/// `28000` is `pg_hba.conf` refusing the connection — which reads like an
/// authentication failure but is matched **per database**, so it is exactly the
/// case another candidate can fix, and exactly what a hosted provider answers
/// for a database it does not publish.
///
/// **The three that were missing are strictly per-database refusals too**, and
/// classifying them as server-level failed the whole connection where the
/// pre-`82a83cd` loop recovered — the opposite of `maintenance_candidates`' own
/// "degrade rather than lock the user out" promise:
///
/// - `55000` — `datallowconn = false`. `template0` ships this way, so a server
///   whose first candidate is a template was unbrowsable.
/// - `53300` — too many connections *for that database*, and **transient**: a
///   momentarily-full `postgres` made the whole server fail rather than the
///   next candidate be tried a moment later.
/// - `42501` — `CONNECT` revoked on that database, which is how a hosted
///   provider fences its own catalogues off.
///
/// All five measured on PG 16.15.
///
/// A wrong password (`28P01`) deliberately does not qualify: it is the same
/// wrong password for every database, and retrying it three more times is three
/// more failed logins against whatever is counting them.
fn another_database_might_work(code: Option<&SqlState>) -> bool {
    matches!(
        code,
        Some(&SqlState::INVALID_CATALOG_NAME)
            | Some(&SqlState::INVALID_AUTHORIZATION_SPECIFICATION)
            | Some(&SqlState::OBJECT_NOT_IN_PREREQUISITE_STATE)
            | Some(&SqlState::TOO_MANY_CONNECTIONS)
            | Some(&SqlState::INSUFFICIENT_PRIVILEGE)
    )
}

/// Drive a freshly-opened connection in the background and hand back its client.
///
/// Generic over the stream type so the plaintext and rustls arms of
/// [`connect_to`] share one body; the connection future completes when the
/// client drops.
fn spawn_connection<S, T>(
    (client, connection): (Client, tokio_postgres::Connection<S, T>),
) -> Client
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            tracing::debug!("postgres connection closed: {e}");
        }
    });
    client
}

/// An error together with everything it was caused by, joined into one line.
///
/// `tokio_postgres::Error`'s own `Display` is a *category* — a failed TLS
/// handshake renders as exactly "error performing TLS handshake", with the
/// reason (unknown issuer, expired, wrong name) only in `source()`. That is the
/// whole content of the message for the failures this driver's TLS support
/// exists to produce, so the chain is walked rather than the head printed.
fn error_chain(e: &dyn std::error::Error) -> String {
    let mut msg = e.to_string();
    let mut source = e.source();
    while let Some(cause) = source {
        let text = cause.to_string();
        // Some wrappers restate their cause verbatim; repeating it reads like
        // two separate problems.
        if !msg.ends_with(&text) {
            msg.push_str(": ");
            msg.push_str(&text);
        }
        source = cause.source();
    }
    msg
}

/// Best-effort query cancellation, over the same transport the query itself
/// used.
///
/// Postgres cancellation opens a *second* connection, so a server configured to
/// refuse plaintext refuses this one too — a `NoTls` cancel would simply stop
/// working for every TLS user, silently, since the result is discarded.
/// **Bounded by [`crate::CANCEL_TIMEOUT`].** Cancellation opens a second
/// connection, so against a host that has gone away it hangs — inside a modal
/// whose every exit maps to this same cancel.
pub(crate) async fn cancel_query(db: &Db, token: &tokio_postgres::CancelToken) {
    let ask = async {
        match db.tls_plan() {
            None => {
                let _ = token.cancel_query(NoTls).await;
            }
            Some(plan) => match crate::tls::pg_connector(plan) {
                Ok(connector) => {
                    let _ = token.cancel_query(connector).await;
                }
                Err(e) => tracing::debug!("cancel skipped, TLS setup failed: {e:?}"),
            },
        }
    };
    if tokio::time::timeout(crate::CANCEL_TIMEOUT, ask)
        .await
        .is_err()
    {
        tracing::debug!("cancel timed out after {:?}", crate::CANCEL_TIMEOUT);
    }
}

/// Connect to a *maintenance* database for server-level work (listing databases,
/// health checks) — PostgreSQL has no server-level connection. Tries the usual
/// always-present candidates in turn so it works whether or not `postgres` exists.
pub(crate) async fn connect_maintenance(db: &Db) -> Result<Client, DbError> {
    connect_maintenance_avoiding(db, None).await
}

/// [`connect_maintenance`], but never landing on `avoid`.
///
/// **`DROP DATABASE` cannot run on a connection to the database it drops** —
/// *"cannot drop the currently open database"* — and the configured database is
/// the *first* maintenance candidate, so dropping the one a connection names
/// would otherwise pick precisely the connection that cannot perform it. The
/// exclusion is passed to [`maintenance_candidates`] rather than filtered here,
/// so the decision stays in the pure function that already has tests on it.
pub(crate) async fn connect_maintenance_avoiding(
    db: &Db,
    avoid: Option<&str>,
) -> Result<Client, DbError> {
    let mut last: Option<DbError> = None;
    for cand in maintenance_candidates(db.database(), &db.user, avoid) {
        match connect_probe(db, cand).await {
            Ok(c) => return Ok(c),
            // The server answered about *this database*; another may work.
            Err((e, true)) => last = Some(e),
            // Anything else — the transport, the host, the password — is the
            // same answer for every candidate, and the caller is better served
            // by that error now than by the last one three round trips later.
            Err((e, false)) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| DbError::Connect("no maintenance database reachable".to_string())))
}

/// Which databases to try, in order, for server-level work.
///
/// **A configured database is not one guess among four — it is the answer, and
/// the rest are what to do without one.** Postgres cannot connect without naming
/// a database, which stayed invisible while almost every server had a `postgres`
/// one anybody could reach; a hosted provider need not. On Aiven only
/// `defaultdb` is permitted, so all three guesses are refused by `pg_hba` and
/// the server is unreachable — the whole reason
/// [`Connection::database`](schemaic_core::connection::Connection::database)
/// exists.
///
/// It is still only *first* rather than only: a database that has since been
/// dropped should degrade to the old behaviour rather than lock the user out of
/// the server, and the guesses cost nothing when the first attempt succeeds.
///
/// **What a *stale* configured name costs, stated rather than fixed.** One that
/// is retryable (`3D000` — dropped or renamed) is tried first on **every**
/// server-level operation, for ever: one failed connect each time, inside the
/// `PING_TIMEOUT` that bounds the whole sequence, so on the hosted providers
/// this field exists for that is a wasted WAN TLS handshake per listing.
/// Invisible on loopback (~2 ms) and never measured on a real link.
///
/// Not cached, deliberately: remembering the failure means per-`Db` state on a
/// handle whose whole design is that it has none — every method connects, runs
/// and disconnects — and a cache that outlives the database being recreated is a
/// connection that stays broken after the user has fixed it. The honest fix is
/// for the *form* to notice a name the server rejects and say so, which is a
/// different feature.
///
/// Split out from the connect loop so the order is a decision with a test on it
/// rather than a literal in the middle of an I/O function.
///
/// `avoid` drops one name from the list wherever it appears — the database a
/// `DROP DATABASE` is about, which cannot be the one the statement runs on. It
/// is dropped rather than moved to the back: a connection to it is not a worse
/// candidate for that plan, it is an impossible one.
fn maintenance_candidates<'a>(
    configured: Option<&'a str>,
    user: &'a str,
    avoid: Option<&str>,
) -> Vec<&'a str> {
    let mut out = Vec::with_capacity(4);
    out.extend(configured);
    for fallback in ["postgres", user, "template1"] {
        if !fallback.is_empty() && !out.contains(&fallback) {
            out.push(fallback);
        }
    }
    if let Some(avoid) = avoid {
        out.retain(|c| *c != avoid);
    }
    out
}

/// Lightweight reachability check bounded by `timeout`.
pub(crate) async fn ping(db: &Db, timeout: Duration) -> Result<(), DbError> {
    let check = async {
        let client = connect_maintenance(db).await?;
        client
            .simple_query("SELECT 1")
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;
        Ok::<(), DbError>(())
    };
    tokio::time::timeout(timeout, check)
        .await
        .map_err(|_| DbError::Connect("timed out".to_string()))?
}

/// The databases worth putting in the schema tree.
///
/// Templates and `postgres` were always excluded as *plumbing*. The other two
/// terms exclude what cannot be **opened**, which is the same idea and was
/// missing: a managed provider keeps its own bookkeeping database on the server
/// — Aiven's is `_aiven` — and the tree listed it, tried to expand it, and
/// rendered the refusal as a red line under a database the user did not create,
/// cannot use, and has no way to dismiss.
///
/// Asked of PostgreSQL rather than answered with a list of names to skip, so a
/// second provider's bookkeeping database needs no second patch: `datallowconn`
/// is the catalog saying connections are barred outright, and
/// `has_database_privilege(…, 'CONNECT')` is it saying they are barred for *us*.
///
/// **Neither sees `pg_hba.conf`**, which is matched at connection time and can
/// refuse a database the catalog says we may enter. Nothing queryable predicts
/// that, so the expansion failure still has to read well.
const DATABASE_LISTING: &str = "SELECT datname FROM pg_database \
     WHERE datistemplate = false AND datname <> 'postgres' \
     AND datallowconn AND has_database_privilege(current_user, oid, 'CONNECT') \
     ORDER BY datname";

/// List the user databases on the server (excludes templates, the built-in
/// `postgres` maintenance database, and anything this role cannot connect to —
/// see [`DATABASE_LISTING`]), sorted by name.
///
/// **Bounded by [`crate::PING_TIMEOUT`], around the whole sequence.** This is
/// the same connect [`ping`] makes, and it was bounded when it was called a ping
/// and unbounded when it was called a listing — the asymmetry visible inside one
/// module. Worse here than on MySQL, because [`connect_maintenance`] tries
/// `postgres`, the user's own name and `template1` **in turn**: against an
/// unroutable address that measured 3 × 21.1 s ≈ 63 s for one click on a
/// connection, while the health check beside it said "Disconnected" at five.
///
/// Around the sequence and not per attempt, deliberately. A per-attempt deadline
/// still costs three of them; and the candidates that are merely *absent* fail
/// fast with a "database does not exist" rather than by timing out, so nothing
/// legitimate is cut short by bounding the whole thing at one.
pub(crate) async fn fetch_databases(db: &Db) -> Result<Vec<String>, DbError> {
    let listing = async {
        let client = connect_maintenance(db).await?;
        let msgs = client
            .simple_query(DATABASE_LISTING)
            .await
            .map_err(|e| DbError::Query(e.to_string()))?;
        let mut out = Vec::new();
        for m in msgs {
            if let SimpleQueryMessage::Row(r) = m
                && let Some(name) = r.get(0)
            {
                out.push(name.to_string());
            }
        }
        Ok::<Vec<String>, DbError>(out)
    };
    tokio::time::timeout(crate::PING_TIMEOUT, listing)
        .await
        .map_err(|_| DbError::Connect("timed out".to_string()))?
}

/// Connect (to `database`, or the maintenance db if unscoped) and run one
/// statement.
pub(crate) async fn fetch_query(
    db: &Db,
    database: Option<&str>,
    sql: &str,
    dest: &mut crate::RowDest,
    cancel: CancellationToken,
) -> Result<ResultSet, DbError> {
    let client = match database {
        Some(d) => connect_to(db, d).await?,
        None => connect_maintenance(db).await?,
    };
    run_statement(db, &client, database.unwrap_or(""), sql, dest, &cancel).await
}

/// Run several statements in order on ONE connection, so session state carries
/// across them (matching the MySQL `run_batch` contract). Stops at the first
/// failing statement; every statement after it reports [`DbError::Cancelled`].
pub(crate) async fn run_batch(
    db: &Db,
    database: Option<&str>,
    stmts: &[String],
    row_cap: usize,
    cancel: CancellationToken,
    mut on_result: impl FnMut(usize, Result<ResultSet, DbError>),
) {
    let client = match database {
        Some(d) => connect_to(db, d).await,
        None => connect_maintenance(db).await,
    };
    let client = match client {
        Ok(c) => c,
        Err(e) => {
            // Couldn't even connect: fail the first statement, cancel the rest.
            let msg = e.to_string();
            for i in 0..stmts.len() {
                on_result(
                    i,
                    if i == 0 {
                        Err(DbError::Connect(msg.clone()))
                    } else {
                        Err(DbError::Cancelled)
                    },
                );
            }
            return;
        }
    };

    let mut stopped = false;
    for (i, sql) in stmts.iter().enumerate() {
        if stopped || cancel.is_cancelled() {
            on_result(i, Err(DbError::Cancelled));
            continue;
        }
        let outcome = run_statement(
            db,
            &client,
            database.unwrap_or(""),
            sql,
            &mut crate::RowDest::Capped(row_cap),
            &cancel,
        )
        .await;
        if outcome.is_err() {
            stopped = true;
        }
        on_result(i, outcome);
    }
}

/// A clean error message from a `tokio_postgres::Error`: prefer the server's own
/// `ERROR: …` text (via `as_db_error`) over the driver's wrapped `Display`, so the
/// editor squiggle / toolbar shows what Postgres actually said.
fn db_err(e: &tokio_postgres::Error) -> DbError {
    match e.as_db_error() {
        Some(d) => DbError::Query(d.message().to_string()),
        None => DbError::Query(e.to_string()),
    }
}

/// Validate `sql` **without executing it**: `PREPARE` it (Parse/Describe only) and
/// let the deallocation happen when the statement drops. Postgres checks syntax,
/// object names, and types but runs nothing — safe even for `UPDATE`/`DELETE`.
/// Returns the server's error text on failure, `Ok(())` on a clean prepare.
pub(crate) async fn prepare_check(
    db: &Db,
    database: Option<&str>,
    sql: &str,
) -> Result<(), DbError> {
    let stmt = sql.trim().trim_end_matches(';').trim_end();
    if stmt.is_empty() {
        return Ok(());
    }
    let client = match database {
        Some(d) => connect_to(db, d).await?,
        None => connect_maintenance(db).await?,
    };
    match client.prepare(stmt).await {
        Ok(_) => Ok(()),
        Err(e) => Err(db_err(&e)),
    }
}

/// Fetch up to `limit` rows of a single table for the Live Monitor. The
/// double-quoted table name is namespace-qualified whenever one is known (the
/// connection is already scoped to `database`), matching the write path — a
/// monitor must watch exactly the table it was opened on.
pub(crate) async fn fetch_table(
    db: &Db,
    database: &str,
    schema: Option<&str>,
    table: &str,
    order_by: Option<&[String]>,
    limit: usize,
    cancel: CancellationToken,
) -> Result<ResultSet, DbError> {
    let sql = format!(
        "SELECT * FROM {}{} LIMIT {}",
        pg_qname(schema, table),
        crate::order_by_clause(order_by, pg_ident),
        limit
    );
    fetch_query(
        db,
        Some(database),
        &sql,
        &mut crate::RowDest::Capped(limit),
        cancel,
    )
    .await
}

/// Run `EXPLAIN sql` (or `EXPLAIN ANALYZE sql`) and return the plan as a result
/// set (the caller parses it with `schemaic_core::plan`). Plain `EXPLAIN` only
/// plans (safe for any statement); `ANALYZE` **executes** it. Postgres spells the
/// analyzing form `EXPLAIN ANALYZE` natively — no MariaDB-style `ANALYZE <stmt>`
/// fallback is needed.
///
/// **The analyzing form runs inside a transaction that is always rolled back.**
/// The UI gates the Analyze toggle on `sql::contains_write`, but that gate reads
/// the statement and any reading of a statement can be wrong — a data-modifying
/// CTE fooled it once already. Measuring must not be the thing that changes the
/// data, so the rollback holds whether or not the gate above it was right.
/// PostgreSQL is fully transactional here, so the rollback is real.
pub(crate) async fn explain(
    db: &Db,
    database: Option<&str>,
    sql: &str,
    analyze: bool,
    cancel: CancellationToken,
) -> Result<ResultSet, DbError> {
    let stmt = sql.trim().trim_end_matches(';').trim_end();
    if !analyze {
        return fetch_query(
            db,
            database,
            &format!("EXPLAIN {stmt}"),
            &mut crate::RowDest::Capped(10_000),
            cancel,
        )
        .await;
    }

    let client = match database {
        Some(d) => connect_to(db, d).await?,
        None => connect_maintenance(db).await?,
    };
    // `BEGIN` isn't a preparable statement, so it goes through `batch_execute`.
    client
        .batch_execute("BEGIN")
        .await
        .map_err(|e| db_err(&e))?;
    let out = run_statement(
        db,
        &client,
        database.unwrap_or(""),
        &format!("EXPLAIN ANALYZE {stmt}"),
        &mut crate::RowDest::Capped(10_000),
        &cancel,
    )
    .await;
    // Unconditional, and its own failure can't mask the plan: dropping the
    // client without a COMMIT would roll back anyway.
    let _ = client.batch_execute("ROLLBACK").await;
    out
}

/// Execute one statement over the text protocol and materialize a [`ResultSet`].
/// Column names + types come from a non-executing `PREPARE`; when the statement
/// isn't preparable (some utility statements) the columns fall back to those on
/// the first returned row (names only). A statement with no result columns
/// (DML/DDL) reports its affected-row count instead of a grid.
/// The PostgreSQL half of [`Db::run_ddl`]: one transaction around the whole
/// plan. `ALTER TABLE`, `CREATE INDEX` and `COMMENT ON` are all transactional
/// here, so a failure anywhere leaves the table exactly as it was — which is why
/// [`crate::DdlError::applied`] is always 0 on this path.
pub(crate) async fn run_ddl(
    db: &Db,
    database: &str,
    stmts: &[String],
    cancel: CancellationToken,
) -> Result<(), crate::DdlError> {
    let fail = |at: usize, e: String| crate::DdlError {
        message: e,
        at,
        applied: 0,
    };
    let client = connect_to(db, database)
        .await
        .map_err(|e| fail(0, e.to_string()))?;
    // Control statements go through `batch_execute` — `BEGIN` isn't a preparable
    // statement, and neither are several DDL forms.
    client
        .batch_execute("BEGIN")
        .await
        .map_err(|e| fail(0, e.to_string()))?;
    // Inside the transaction, so it reverts with it — and the connection is
    // this plan's alone either way. Best-effort, as on MySQL.
    let _ = client
        .batch_execute(&crate::lock_wait_sql(crate::Engine::Postgres))
        .await;
    for (i, sql) in stmts.iter().enumerate() {
        let step = tokio::select! {
            r = client.batch_execute(sql) => r.map_err(|e| e.to_string()),
            _ = cancel.cancelled() => Err("cancelled".to_string()),
        };
        if let Err(e) = step {
            let _ = client.batch_execute("ROLLBACK").await;
            return Err(fail(i, e));
        }
    }
    if let Err(e) = client.batch_execute("COMMIT").await {
        let _ = client.batch_execute("ROLLBACK").await;
        return Err(fail(stmts.len().saturating_sub(1), e.to_string()));
    }
    Ok(())
}

/// PostgreSQL's arm of [`crate::Db::run_server_ddl`] — `CREATE DATABASE` and
/// `DROP DATABASE`.
///
/// Two things separate it from [`run_ddl`] above, and neither is a matter of
/// taste:
///
/// - **No transaction.** PostgreSQL refuses both statements inside one —
///   *"CREATE DATABASE cannot run inside a transaction block"* — so the `BEGIN`
///   that makes every other plan here all-or-nothing is exactly what would make
///   this one fail. There is nothing to give up by leaving it out: a
///   server-level plan is a single statement, so atomicity across statements is
///   not a property it could have had.
/// - **A connection that is not to the target.** `run_ddl` connects to the
///   database it is changing; here that database is the thing being created (so
///   it cannot be connected to) or dropped (so it must not be). The maintenance
///   connection is picked with the target excluded — see
///   [`connect_maintenance_avoiding`].
///
/// `applied` is therefore the count of statements that really did land, with no
/// rollback to qualify it.
pub(crate) async fn run_server_ddl(
    db: &Db,
    avoid: Option<&str>,
    stmts: &[String],
    cancel: CancellationToken,
) -> Result<(), crate::DdlError> {
    let client = connect_maintenance_avoiding(db, avoid)
        .await
        .map_err(|e| crate::DdlError {
            message: e.to_string(),
            at: 0,
            applied: 0,
        })?;
    for (i, sql) in stmts.iter().enumerate() {
        let step = tokio::select! {
            r = client.batch_execute(sql) => r.map_err(|e| e.to_string()),
            _ = cancel.cancelled() => Err("cancelled".to_string()),
        };
        if let Err(e) = step {
            return Err(crate::DdlError {
                message: e,
                at: i,
                applied: i,
            });
        }
    }
    Ok(())
}

/// PostgreSQL's arm of [`crate::Db::run_script`]. See there for why nothing is
/// wrapped in a transaction and why the connection is pinned.
///
/// `batch_execute` rather than a prepared statement for the reason `run_ddl`
/// gives above: `BEGIN` is not preparable and neither are several DDL forms, and
/// a script is mostly made of exactly those.
/// How one statement of a script ended, so the borrow of its SQL can be
/// released before the failure arm moves it.
///
/// `Cancelled { ran }` is the point: a cancel is a request, and the statement it
/// interrupted may have landed anyway. Both engines' script loops answer in
/// these three shapes.
pub(crate) enum ScriptStep {
    Ran,
    /// The user stopped it; `ran` says whether the in-flight statement
    /// nevertheless completed.
    Cancelled {
        ran: bool,
    },
    Failed(String),
}

pub(crate) async fn run_script(
    db: &Db,
    database: &str,
    mut rx: tokio::sync::mpsc::Receiver<schemaic_core::script::Statement>,
    cancel: CancellationToken,
) -> (schemaic_core::script::ExecEnd, usize) {
    use schemaic_core::script::ExecEnd;
    let client = match connect_to(db, database).await {
        Ok(c) => c,
        Err(e) => return (ExecEnd::Connect(e.to_string()), 0),
    };
    // **No lock bound is set here, and that is the change.** This used to apply
    // `DDL_LOCK_WAIT_SECS`, a constant documented for the Apply modal — a short
    // reviewed plan whose refusal costs nothing. A user's `.sql` file is not
    // that: a restore dying at statement N with N−1 applied and no transaction
    // of ours to roll back is far worse than waiting. Measured, the one
    // constant meant three different things anyway — 10 s on PostgreSQL, 50 s
    // on MariaDB (metadata 10, rows 50), 15 s on SQLite's busy timeout — so it
    // was not even one bound.
    //
    // `psql -f` and `mysql <` set nothing, which is what this replaces, and the
    // escape hatch here is better than theirs: Stop cancels the running
    // statement on the server.
    let mut ran = 0usize;
    let end = loop {
        let next = tokio::select! {
            s = rx.recv() => s,
            _ = cancel.cancelled() => break ExecEnd::Cancelled,
        };
        let Some(st) = next else { break ExecEnd::Done };
        // **The running statement is cancelled on the server, not just dropped
        // here.** PostgreSQL does not abort a backend when the client stops
        // reading (`client_connection_check_interval` is 0 by default), so
        // without this a Stop during a long `CREATE INDEX` left the index being
        // built and autocommitted while the report said the run stopped one
        // statement earlier — short by exactly the statement the user would go
        // looking for. Every other long operation in this file already does
        // this; the script arm was the one that did not.
        let pg_cancel = client.cancel_token();
        // **The cancelled statement is awaited, not dropped.** A cancel is a
        // request: the server may already have committed the statement by the
        // time it arrives, and dropping the future left `ran` a *floor* while
        // `RunOutcome`'s own doc calls it "the answer" and the panel calls it
        // "the message, not a footnote". The plain `select!` race is the other
        // way in — this crate already has that written down at
        // `sqlite.rs`'s read loop.
        //
        // Scoped so the borrow of `st.sql` ends before the `Failed` arm needs
        // to move it.
        let step = {
            let mut fut = std::pin::pin!(client.batch_execute(&st.sql));
            let raced = tokio::select! {
                r = fut.as_mut() => Some(r),
                _ = cancel.cancelled() => None,
            };
            match raced {
                Some(Ok(())) => ScriptStep::Ran,
                Some(Err(e)) => ScriptStep::Failed(e.to_string()),
                None => {
                    cancel_query(db, &pg_cancel).await;
                    ScriptStep::Cancelled {
                        ran: fut.await.is_ok(),
                    }
                }
            }
        };
        match step {
            ScriptStep::Ran => ran += 1,
            ScriptStep::Cancelled { ran: landed } => {
                if landed {
                    ran += 1;
                }
                break ExecEnd::Cancelled;
            }
            ScriptStep::Failed(message) => {
                break ExecEnd::Failed {
                    message,
                    sql: st.sql,
                    line: st.line,
                };
            }
        }
    };
    (end, ran)
}

pub(crate) async fn run_statement(
    db: &Db,
    client: &Client,
    database: &str,
    sql: &str,
    dest: &mut crate::RowDest,
    cancel: &CancellationToken,
) -> Result<ResultSet, DbError> {
    let row_cap = dest.cap();
    let start = Instant::now();

    // Column metadata from a non-executing PREPARE. The prepared columns carry
    // names + types AND per-column provenance (`table_oid` / `column_id`) — the
    // Postgres analog of MySQL's `org_table`/`org_name`, which the editing system
    // (`analyze_edit`) needs. Columns from expressions carry no provenance.
    //
    // **The first statement, not the whole string.** `Parse` takes one command, so
    // a multi-statement string cannot be prepared at all — and the loop below
    // reports only the first result set, so the first statement is exactly the one
    // whose columns describe what comes back. Preparing the whole string instead
    // left `type_name` empty for every column, and an empty type name is an
    // unknown [`CellKind`]: a `bytea` was then read as text and its entire hex
    // encoding went into the cell. For a single statement this is the same string
    // and the same one round trip.
    let describe = schemaic_core::sql::first_statement(sql, PG);
    let prepared_cols: Option<Vec<Column>> = match client.prepare(describe).await {
        Ok(stmt) => {
            let cols = stmt.columns();
            let mut columns: Vec<Column> = cols
                .iter()
                .map(|c| Column {
                    name: c.name().to_string(),
                    type_name: pg_type_name(c.type_()),
                    origin: None,
                })
                .collect();
            // (table_oid, attnum) per column, when it comes straight from a table.
            let ids: Vec<Option<(u32, i16)>> = cols
                .iter()
                .map(|c| match (c.table_oid(), c.column_id()) {
                    (Some(oid), Some(attnum)) if attnum > 0 => Some((oid, attnum)),
                    _ => None,
                })
                .collect();
            // Resolve provenance (real table/column names + key flags) from the
            // catalog, then attach `ColumnOrigin` to each table-backed column.
            // `database` is the connected DB name — matches the app's schema
            // lookup (`ConnNode.database`) so editability uses the full schema.
            if !database.is_empty()
                && ids.iter().any(|p| p.is_some())
                && let Ok(meta) = fetch_col_meta(client, &ids).await
            {
                for (col, id) in columns.iter_mut().zip(ids.iter()) {
                    if let Some(key) = id
                        && let Some(m) = meta.get(key)
                    {
                        let binary = col.type_name == "BYTEA";
                        col.origin = Some(ColumnOrigin {
                            database: database.to_string(),
                            schema: Some(m.schema.clone()),
                            table: m.table.clone(),
                            column: m.column.clone(),
                            flags: m.flags,
                            binary,
                            // PostgreSQL's `ctid` is a physical location, not a
                            // row identity: `VACUUM` moves it. There is nothing
                            // here to key a write on that isn't a column.
                            implicit_key: false,
                        });
                    }
                }
            }
            Some(columns)
        }
        Err(_) => None,
    };

    // Execute over the text protocol, honoring cancellation via the cancel token.
    //
    // `simple_query_raw`, not `simple_query`: the latter is just the former plus
    // `try_collect()`, so it materializes every row of the result as a
    // `Vec<SimpleQueryMessage>` before the row cap gets a say. On a table larger
    // than the cap that's the whole table in memory to keep 200k rows of it —
    // the MySQL path has streamed row-by-row all along. Rows go straight into the
    // columnar `ResultBuilder` as they arrive, and the stream is dropped once the
    // cap is reached.
    let token = client.cancel_token();
    let stream = tokio::select! {
        r = client.simple_query_raw(sql) => r.map_err(|e| db_err(&e))?,
        _ = cancel.cancelled() => {
            cancel_query(db, &token).await;
            return Err(DbError::Cancelled);
        }
    };
    let mut stream = std::pin::pin!(stream);

    // Start the builder from PREPARE's columns when it gave any, so a zero-row
    // SELECT still reports its columns. Otherwise the first row names them (names
    // only) — an unpreparable statement that still returns rows. Still `None`
    // after the loop ⇒ DML/DDL/utility, which reports affected rows, not a grid.
    // The binary flags ride along with the type names, and for the same reason:
    // both are per-column answers, and re-deriving either per cell would put a
    // string split in the path of every one of up to 200k x N reads.
    let chunk_capacity = dest.chunk_capacity();
    let mut grid: Option<(ResultBuilder, Vec<CellKind>)> =
        prepared_cols.filter(|c| !c.is_empty()).map(|c| {
            let kinds = cell_kinds(&type_names_of(&c));
            (
                ResultBuilder::with_capacity(c.clone(), chunk_capacity),
                kinds,
            )
        });
    let mut affected: u64 = 0;
    let mut truncated = false;
    // How many row-returning result sets the stream has announced. A simple query
    // is one string, and PostgreSQL will happily run several statements from it —
    // the MCP server hands one straight through, and nothing upstream splits it.
    // Each row-returning statement announces itself with its own
    // `RowDescription`, which is the only boundary marker on the stream, and it
    // used to be discarded by the wildcard below: the second set's rows were then
    // pushed through the *first* set's columns and cell kinds, so a `bytea` in set
    // two was read under set one's `INT4` and dumped its whole hex encoding into a
    // cell headed by the first set's column name — the case `pg_cell` exists to
    // prevent ("a hex dump of a 40 MB blob is a cell no grid survives").
    let mut sets = 0usize;

    loop {
        // Cancellation is checked per message now rather than only around the
        // whole call, so stopping a long fetch takes effect at the next row
        // instead of after the last one.
        let next = tokio::select! {
            n = stream.next() => n,
            _ = cancel.cancelled() => {
                cancel_query(db, &token).await;
                return Err(DbError::Cancelled);
            }
        };
        let Some(msg) = next else { break };
        match msg.map_err(|e| db_err(&e))? {
            SimpleQueryMessage::Row(r) => {
                let (builder, kinds) = grid.get_or_insert_with(|| {
                    let cols: Vec<Column> = r
                        .columns()
                        .iter()
                        .map(|c| Column {
                            name: c.name().to_string(),
                            type_name: String::new(),
                            origin: None,
                        })
                        .collect();
                    let kinds = cell_kinds(&type_names_of(&cols));
                    (ResultBuilder::with_capacity(cols, chunk_capacity), kinds)
                });
                if builder.row_count() >= row_cap {
                    // A row beyond the cap exists → the result is truncated. Drop
                    // the stream rather than draining it: tokio-postgres' own
                    // connection task keeps paging through the remaining messages
                    // for a hung-up receiver, so the protocol stays in sync and
                    // the connection is still reusable — which a Manual-mode tab,
                    // holding one pinned connection, depends on.
                    truncated = true;
                    break;
                }
                // Parse each text cell by its column type (integers/floats become
                // compact numeric variants; everything else stays an exact string
                // — never lossy).
                let cells: Vec<Value> = kinds
                    .iter()
                    .enumerate()
                    .map(|(i, &kind)| match r.get(i) {
                        None => Value::Null,
                        Some(s) => pg_cell(s, kind),
                    })
                    .collect();
                builder.push_row(&cells);
                // Hand a full block to the export and keep reading into an empty
                // builder. A capped read never fills one.
                if dest.chunk_full(builder.row_count(), builder.text_bytes()) {
                    let next = dest.chunk_capacity();
                    dest.flush(builder, next).await?;
                }
            }
            SimpleQueryMessage::CommandComplete(n) => affected = n,
            // A second one means a second result set, and this stops rather than
            // merging: one result set per read is what the MySQL path already
            // reports (`collect_rows` reads `columns_ref()` once and its stream
            // yields only the first set), so the two engines answer a
            // multi-statement string the same way. Dropping the stream here is
            // safe for the same reason the row cap's `break` is — the driver's
            // connection task keeps paging, so the connection stays reusable.
            SimpleQueryMessage::RowDescription(_) => {
                sets += 1;
                if sets > 1 {
                    tracing::warn!(
                        "multi-statement query returned more than one result set; \
                         reporting only the first"
                    );
                    break;
                }
            }
            // `SimpleQueryMessage` is `#[non_exhaustive]`, so this arm cannot be
            // removed — which is exactly how `RowDescription` came to be swallowed.
            // Anything new arriving here must be *considered* before it is
            // ignored: the question to ask is whether it carries a boundary or a
            // column list, because those are the two things this loop's state
            // depends on.
            _ => {}
        }
    }

    // No result columns → DML/DDL/utility: report affected rows, not a grid.
    let Some((mut builder, _)) = grid else {
        return Ok(ResultSet::affected_rows(Vec::new(), affected)
            .with_elapsed(start.elapsed().as_millis()));
    };
    // The tail — see the same flush in MySQL's `collect_rows`: the export's
    // header comes from the first chunk, so a table with no rows must still hand
    // over the empty one that carries its columns. A statement with no result
    // columns returned just above and never reaches here; `Db::stream_query`
    // refuses those outright.
    dest.flush(&mut builder, 0).await?;
    builder.set_truncated(truncated);
    builder.set_elapsed(start.elapsed().as_millis());
    Ok(builder.finish())
}

/// The per-column type names the text-cell parser keys on, in column order.
fn type_names_of(columns: &[Column]) -> Vec<String> {
    columns.iter().map(|c| c.type_name.clone()).collect()
}

/// One text-protocol cell → the `Value` the grid stores.
///
/// The simple-query protocol hands every value over as text, and for `bytea`
/// that text is the server's `hex` output — `\x48656c6c6f`. That is lossless,
/// but it is a *third* rendering of a raw-bytes cell (SQLite showed the size,
/// MySQL showed mojibake), and a hex dump of a 40 MB blob is a cell no grid
/// survives. So it renders as `binary_display` like the other two, and the
/// length is read off the hex rather than guessed.
/// Both signals are required: the column's type says bytes *and* the text
/// parses as hex. A `text` column can legitimately hold the characters
/// `\x4869`, and turning that into `<2 bytes>` would be the same lie in the
/// other direction. The type name comes from the prepared statement rather than
/// the catalog, so this reaches a `bytea` expression with no table provenance —
/// which the wire `ColumnOrigin::binary` flag never did.
/// `kind` is the column's answer, computed once per result by [`cell_kinds`]:
/// `type_is_binary` splits a type name and walks a keyword list, and the numeric
/// kind allocates a `String` to do it — neither is something to re-derive for
/// every cell of a read that has no row cap.
fn pg_cell(text: &str, kind: CellKind) -> Value {
    if kind.binary
        && let Some(len) = bytea_hex_len(text)
    {
        return Value::Str(binary_display(len));
    }
    crate::parse_as(kind.num, text.to_string())
}

/// Everything a text cell's conversion needs to know about its column, computed
/// **once per column**.
///
/// Both halves are per-column answers derived from a type name by splitting it
/// and walking a keyword list, which is not something to re-derive for every cell
/// of a read that no longer has a row cap. `binary` was hoisted for that reason
/// already; `num` is the other half of the same `match`, and it was still paying
/// a `String` allocation per cell — 100M of them on a 5M-row × 20-column export.
#[derive(Clone, Copy)]
struct CellKind {
    num: crate::NumKind,
    binary: bool,
}

/// [`CellKind`] per column, in column order.
fn cell_kinds(type_names: &[String]) -> Vec<CellKind> {
    type_names
        .iter()
        .map(|t| CellKind {
            num: crate::num_kind(t),
            binary: schemaic_core::model::type_is_binary(t),
        })
        .collect()
}

/// Byte length of a `bytea` in PostgreSQL's default `hex` output, or `None`
/// when the text is not one.
///
/// `None` is the important half and covers two real cases rather than being a
/// defensive shrug: a server set to the legacy `bytea_output = escape` sends a
/// different encoding entirely, and every non-`bytea` column is text that
/// merely starts with a backslash. Both must fall through to the ordinary
/// parser with their text intact — replacing a value we *can* still read with
/// a placeholder would be the data loss this rendering exists to prevent.
fn bytea_hex_len(text: &str) -> Option<usize> {
    let hex = text.strip_prefix("\\x")?;
    (hex.len() % 2 == 0 && hex.bytes().all(|b| b.is_ascii_hexdigit())).then_some(hex.len() / 2)
}

/// SQL predicate selecting the *user* schemas of a database — everything except
/// PostgreSQL's own catalogs and the per-session temp namespaces. Extension-owned
/// schemas (PostGIS's `topology`, `pg_cron`, …) are deliberately kept: the user
/// installed them and their tables are legitimately browsable.
///
/// `{ns}` is the alias of the `pg_namespace` (or `information_schema` column)
/// holding the schema name, so the same rule can be spliced into either catalogue
/// style of query and the five introspection queries can't drift apart.
fn user_schema_filter(ns: &str) -> String {
    format!(
        "{ns} NOT IN ('pg_catalog', 'information_schema') \
         AND {ns} NOT LIKE 'pg\\_toast%' AND {ns} NOT LIKE 'pg\\_temp%' \
         AND {ns} NOT LIKE 'pg\\_toast\\_temp%'"
    )
}

/// Every browsable object in the database, as `(namespace, name, "BASE TABLE" |
/// "VIEW")` — the list that **decides what exists**, since `assemble_schema`
/// builds its tables from it alone and drops every other row set whose table
/// isn't in it.
///
/// From `pg_catalog`, not `information_schema.tables`, and that is the point:
/// PostgreSQL 16's own catalogue definition filters that view to
/// `c.relkind IN ('r','v','f','p')`, so **it cannot return a materialized
/// view** — they aren't in the SQL standard. Every matview was therefore
/// invisible in the tree, completion, the ERD, Find-Anywhere and `Catalog`, with
/// no error and no partial entry, while the four other queries in
/// [`fetch_schema`] all already reached `'m'` and had their rows discarded. The
/// view-body query had switched to `pg_get_viewdef` over `pg_class` for exactly
/// this reason; this one hadn't.
///
/// Checked against the live fixtures: identical output to the old query on
/// `world`, `chinook` and the multi-schema `warehouse`, differing only by the
/// matview it now returns.
fn table_list_sql() -> String {
    format!(
        "SELECT n.nspname, c.relname, \
                CASE WHEN c.relkind IN ('v','m') THEN 'VIEW' ELSE 'BASE TABLE' END \
         FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE c.relkind IN ('r','v','m','p','f') AND {} \
         ORDER BY n.nspname, c.relname",
        user_schema_filter("n.nspname")
    )
}

use crate::{ColRow, IdxRow};
/// A catalogue row tagged with the namespace it belongs to, so the whole-database
/// fetch can be partitioned per schema before folding.
type InSchema<T> = (String, T);

/// Order schemas for display: `public` first (it's the default namespace and
/// where most work happens), everything else alphabetically.
/// Every index of every browsable schema, one row per **key position** in
/// `indkey` order (`unnest(…) WITH ORDINALITY` preserves it). See the comment at
/// its call site for what each of the trailing columns is for.
fn index_list_sql() -> String {
    format!(
        "SELECT n.nspname, c.relname, \
                CASE WHEN ix.indisprimary THEN 'PRIMARY' ELSE ic.relname END AS iname, \
                CASE WHEN ix.indisunique THEN 0 ELSE 1 END AS non_unique, \
                a.attname, ix.indisprimary, \
                am.amname, pg_get_expr(ix.indpred, ix.indrelid), \
                pgc.conname, \
                (EXISTS (SELECT 1 FROM unnest(ix.indclass::oid[]) AS q(oid) \
                           JOIN pg_opclass o ON o.oid = q.oid \
                          WHERE NOT o.opcdefault) \
                 OR EXISTS (SELECT 1 FROM unnest(ix.indoption::int2[]) AS p(opt) \
                             WHERE opt NOT IN (0, 3))) AS lossy, \
                pg_get_indexdef(ix.indexrelid, k.ord::int, true) AS keydef, \
                (o.opt & 1) <> 0 AS descending \
         FROM pg_index ix \
         JOIN pg_class c ON c.oid = ix.indrelid \
         JOIN pg_class ic ON ic.oid = ix.indexrelid \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         JOIN pg_am am ON am.oid = ic.relam \
         LEFT JOIN pg_constraint pgc \
                ON pgc.conindid = ic.oid AND pgc.contype IN ('p', 'u') \
         JOIN unnest(ix.indkey) WITH ORDINALITY AS k(attnum, ord) ON true \
         JOIN unnest(ix.indoption) WITH ORDINALITY AS o(opt, oord) ON o.oord = k.ord \
         LEFT JOIN pg_attribute a \
                ON a.attrelid = ix.indrelid AND a.attnum = k.attnum AND a.attnum > 0 \
         WHERE {} \
         ORDER BY n.nspname, c.relname, iname, k.ord",
        user_schema_filter("n.nspname")
    )
}

fn schema_sort_key(name: &str) -> (u8, String) {
    if name == schemaic_core::schema::PG_DEFAULT_SCHEMA {
        (0, String::new())
    } else {
        (1, name.to_string())
    }
}

/// Introspect every **user** schema of one database (tables → columns +
/// PK/unique/FK + all indexes) via `information_schema` + `pg_catalog`.
///
/// The catalogue rows are fetched for all namespaces in one round trip each, then
/// **partitioned by namespace** and handed to the shared, engine-agnostic
/// [`assemble_schema`] one schema at a time — that function keys its rows by table
/// name alone, so feeding it two schemas at once would silently merge same-named
/// tables. Each resulting [`TableInfo`] carries
/// its namespace, and the schemas are concatenated `public`-first.
/// The PostgreSQL half of [`Db::fetch_table_list`]: the same table list the full
/// fetch starts from, and none of the four catalogue queries after it.
pub(crate) async fn fetch_table_list(db: &Db, database: &str) -> Result<DbSchema, DbError> {
    let client = connect_to(db, database).await?;
    let tables = query_all(&client, &table_list_sql())
        .await?
        .into_iter()
        .map(|r| TableInfo {
            schema: Some(cell(&r, 0)),
            name: cell(&r, 1),
            is_view: cell(&r, 2) == "VIEW",
            ..Default::default()
        })
        .collect();
    Ok(DbSchema {
        tables,
        ..Default::default()
    })
}

/// Sizes and row estimates for every relation that **has** storage, per
/// namespace.
///
/// `reltuples` is `-1` on a relation that has never been analyzed, and `-1` is
/// not a row count — it is the catalogue saying it doesn't know, so it becomes
/// `NULL` here and `None` in the model rather than a negative number nobody
/// checks for. **PostgreSQL 13 and earlier wrote `0` for the same thing**, which
/// is indistinguishable from an empty table; on those servers an unanalyzed
/// table reports zero rows, and the estimate label plus **Count rows** are the
/// only remedy. 13 went end-of-life in November 2025.
///
/// Only `r` (ordinary) and `m` (materialized view) are asked for a size. A
/// **partitioned** parent (`p`) has no storage of its own — `pg_table_size`
/// would return a truthful `0` that reads as "this 40 GB table is empty" — so it
/// is listed with null sizes; its partitions are ordinary relations and appear
/// in their own right. Foreign tables (`f`) are excluded for the same reason.
/// Plain views (`v`) are not here at all, having neither rows nor bytes.
fn table_stats_sql() -> String {
    format!(
        "SELECT n.nspname, c.relname, \
                CASE WHEN c.reltuples < 0 THEN NULL ELSE c.reltuples::bigint END, \
                CASE WHEN c.relkind = 'p' THEN NULL ELSE pg_table_size(c.oid) END, \
                CASE WHEN c.relkind = 'p' THEN NULL ELSE pg_indexes_size(c.oid) END, \
                s.n_dead_tup, \
                GREATEST(s.last_analyze, s.last_autoanalyze) \
         FROM pg_class c \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         LEFT JOIN pg_stat_all_tables s ON s.relid = c.oid \
         WHERE c.relkind IN ('r','m','p') AND {} \
         ORDER BY n.nspname, c.relname",
        user_schema_filter("n.nspname")
    )
}

/// Per-index size and scan count. The primary key's index is renamed `PRIMARY`
/// exactly as [`index_list_sql`] does it, so the properties surface and the
/// designer name the same index the same way.
///
/// **The size carries [`table_stats_sql`]'s partitioned guard, in the index's own
/// spelling.** A partitioned index (`relkind = 'I'`) is a parent with no storage,
/// exactly as a partitioned table (`'p'`) is: `pg_relation_size` on one returns a
/// truthful `0` that the panel renders as `0 B` — and *Copy* exports — for an
/// index spread over 40 GB of partitions. Null instead, which prints as `—`.
///
/// The scan half needs no guard and must not grow one: `pg_stat_all_indexes` is
/// defined over `relkind IN ('r','t','m')`, so a partitioned index has no row
/// there, `idx_scan` is NULL, and `scans: None` is the truth — "nobody counted",
/// which is what stops [`IndexStats::is_unused`] flagging it.
fn index_stats_sql() -> String {
    format!(
        "SELECT n.nspname, c.relname, \
                CASE WHEN ix.indisprimary THEN 'PRIMARY' ELSE ic.relname END, \
                CASE WHEN ic.relkind = 'I' THEN NULL ELSE pg_relation_size(ic.oid) END, \
                si.idx_scan, \
                ix.indisprimary, ix.indisunique \
         FROM pg_index ix \
         JOIN pg_class c ON c.oid = ix.indrelid \
         JOIN pg_class ic ON ic.oid = ix.indexrelid \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         LEFT JOIN pg_stat_all_indexes si ON si.indexrelid = ix.indexrelid \
         WHERE {} \
         ORDER BY n.nspname, c.relname, ic.relname",
        user_schema_filter("n.nspname")
    )
}

/// The PostgreSQL half of [`Db::fetch_table_stats`].
///
/// Both queries go through [`query_all_optional`]: the `pg_stat_*` views can be
/// restricted or absent, and a properties panel is not worth failing a whole
/// fetch over.
pub(crate) async fn fetch_table_stats(db: &Db, database: &str) -> Result<SchemaStats, DbError> {
    let client = connect_to(db, database).await?;

    let mut by_table: HashMap<(String, String), Vec<IndexStats>> = HashMap::new();
    for r in query_all_optional(&client, &index_stats_sql()).await? {
        by_table
            .entry((cell(&r, 0), cell(&r, 1)))
            .or_default()
            .push(IndexStats {
                name: cell(&r, 2),
                bytes: num(&r, 3),
                cardinality: None, // PostgreSQL keeps per-column n_distinct, not this.
                scans: num(&r, 4),
                is_primary: flag(&r, 5),
                is_unique: flag(&r, 6),
            });
    }

    let tables = query_all_optional(&client, &table_stats_sql())
        .await?
        .into_iter()
        .map(|r| {
            let (ns, name) = (cell(&r, 0), cell(&r, 1));
            let rows = num(&r, 2);
            let analyzed = opt(&r, 6);
            // "Never analyzed" is a claim about the table, and it explains a
            // *missing* estimate. With an estimate in hand and no timestamp —
            // a stats reset, or a figure VACUUM set — saying it would
            // contradict the number right next to it, so that case says
            // nothing instead.
            let freshness = match (&analyzed, rows) {
                (Some(_), _) => Freshness::Analyzed(analyzed.clone()),
                (None, None) => Freshness::Analyzed(None),
                (None, Some(_)) => Freshness::Unknown,
            };
            TableStats {
                indexes: by_table
                    .remove(&(ns.clone(), name.clone()))
                    .unwrap_or_default(),
                table: name,
                schema: Some(ns),
                rows,
                exact_rows: None,
                data_bytes: num(&r, 3),
                index_bytes: num(&r, 4),
                // PostgreSQL has no "allocated but unused" figure to report;
                // dead tuples are how it expresses the same idea.
                free_bytes: None,
                dead_rows: num(&r, 5),
                auto_increment: None,
                row_format: None,
                engine: None,
                created: None,
                updated: None,
                freshness,
            }
        })
        .collect();
    Ok(SchemaStats::new(tables))
}

/// The PostgreSQL half of [`Db::count_rows`].
pub(crate) async fn count_rows(
    db: &Db,
    database: &str,
    sql: &str,
    cancel: CancellationToken,
) -> Result<u64, DbError> {
    let client = connect_to(db, database).await?;
    // A full scan on a large table, so the token is what stops it *on the server*
    // rather than only abandoning the answer — see `Db::count_rows`.
    let token = client.cancel_token();
    let rows = tokio::select! {
        r = query_all(&client, sql) => r?,
        _ = cancel.cancelled() => {
            cancel_query(db, &token).await;
            return Err(DbError::Cancelled);
        }
    };
    rows.first()
        .and_then(|r| opt(r, 0))
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| DbError::Query("COUNT(*) returned no row".into()))
}

/// `pg_stat_activity`, narrowed to the sessions a person opened.
///
/// `backend_type = 'client backend'` drops the checkpointer, the WAL writer, the
/// autovacuum launcher and the rest of the cluster's own processes: they are
/// permanent, unkillable from here, and every one of them would sort to the top
/// with an age measured in days. `pid <> pg_backend_pid()` drops the poll itself.
///
/// The age is *time in the current state*, matching what MySQL's
/// `PROCESSLIST.TIME` means, so one column means one thing across the two
/// engines. `pg_blocking_pids` is the blocking graph in one call — PostgreSQL
/// resolves it through `pg_locks` internally, including the transitive case a
/// hand-written join gets wrong.
///
/// **Blocked backends first, then longest-standing.** Ordering by age alone
/// sounds like "keep the interesting end of the list" and is the opposite of it:
/// the panel ranks lock waits above everything
/// ([`schemaic_core::activity::rank`]), and a backend that
/// started waiting four seconds ago has the *smallest* age on the cluster — so
/// past the cap, a pile of hour-old idle pool connections displaced every row of
/// the wait the panel exists to show.
///
/// The sort is exact here rather than a proxy (MySQL has to settle for
/// `COMMAND <> 'Sleep'`): the blocking graph is already a column. It reads out of
/// a subquery so `pg_blocking_pids` is evaluated **once** per backend instead of
/// once for the projection and again for the `ORDER BY` — it takes locks
/// internally, and this runs against every backend on the cluster before the
/// `LIMIT` can apply. The outer projection restates the columns in order because
/// `simple_query` hands them back positionally.
///
/// **The masked-row branch keeps the rows the role may not inspect — and only
/// those.** PostgreSQL masks `pg_stat_activity` for backends the caller has no
/// `HAS_PGSTAT_PERMISSIONS` over, and `backend_type` is one of the masked
/// columns — so `backend_type = 'client backend'` is `NULL` for them and the
/// rows were filtered out entirely rather than merely showing blanks. Connected
/// as an ordinary application role, the panel listed only that role's own
/// sessions and stated the count as the server's total; a blocked row could name
/// a holder that was nowhere in the list, so the banner never appeared and there
/// was nothing to kill. `pid` and `pg_blocking_pids(pid)` are *not* masked, so a
/// kept row is still killable and still a usable node in the graph.
///
/// The masking is why the branch cannot be a bare `backend_type IS NULL`: the
/// equality test was also the only thing excluding PostgreSQL's **auxiliary
/// processes** — checkpointer, background writer, walwriter, both launchers —
/// and NULL is exactly what an ordinary role reads for their `backend_type` too.
/// Live on PostgreSQL 16.15 under a non-privileged role, the bare form admitted
/// five rows and **all five were auxiliary processes**, each drawn with a live
/// *Kill session* under it that answers `f` and a WARNING.
///
/// **`usename` and `datname` are the columns that are not masked, and it takes
/// both.** Probed live on 16.15, plain role against a superuser's `psql`: a
/// masked client backend keeps `usename` *and* `datname` and loses only `state`,
/// `query`, `backend_type` and the timestamps — while every auxiliary process
/// has `datname` NULL, the logical replication launcher included, and that one
/// runs as `postgres` so a `usename` test alone still admits it. Requiring both
/// also keeps out autovacuum workers (no `usename`) and walsenders (no
/// `datname`), which is where the pre-range equality test had them.
/// `activity::is_pg_session` is the second guard, at the level a test can reach.
///
/// **The state term in the sort** is the half MySQL's query has and this one
/// didn't. Blocked-ness is only rank 0 of `activity::rank`'s four; an idle pool
/// connection's age grows without bound while a fresh lock holder's is near
/// zero, so past the `LIMIT` a wall of hour-old idle backends displaced the
/// running statement and the `idle in transaction` holder the panel exists to
/// show — while keeping the waiter, whose note then named a holder that had been
/// cut.
///
/// **Both sort terms have to survive the mask, and neither did.** A masked row
/// reads `state` NULL, so `state IS DISTINCT FROM 'idle'` is `true` for every
/// one of them and separates nothing; its `age` is NULL too, and PostgreSQL
/// sorts `DESC` as `NULLS FIRST`. Live under a plain role the effect was exact:
/// past the `LIMIT` the rows kept were the ones the role **cannot read** and the
/// ones cut were its own running statements. `NULLS LAST` and an explicit `state
/// IS NOT NULL` demote an unreadable row below a readable working one, which is
/// where `pg_state` and `activity::rank` put it once it arrives.
///
/// **The outer projection is not written here.** It is built from
/// [`activity::PG_ACTIVITY_COLUMNS`], which is the same list
/// [`activity::PgActivityRow::from_slots`] destructures a row in — so a column
/// added, dropped or moved changes both ends at once. Written out twice, the
/// fold indexed the row with bare literals against this string in another
/// crate, and `array::from_fn` padded a short row with `None`s rather than
/// refusing it.
const PG_ACTIVITY_SQL: &str = "FROM ( \
     SELECT pid, usename, host(client_addr) AS client_host, datname, state, query, \
     EXTRACT(EPOCH FROM (now() - COALESCE(state_change, query_start, backend_start)))::float8 \
     AS age, \
     pg_blocking_pids(pid)::text AS blockers \
     FROM pg_stat_activity \
     WHERE (backend_type = 'client backend' \
     OR (backend_type IS NULL AND usename IS NOT NULL AND datname IS NOT NULL)) \
     AND pid <> pg_backend_pid() \
     ) s ORDER BY (s.blockers <> '{}') DESC, \
     (s.state IS DISTINCT FROM 'idle' AND s.state IS NOT NULL) DESC, \
     s.age DESC NULLS LAST LIMIT ";

/// The PostgreSQL half of [`Db::fetch_sessions`].
///
/// `pg_stat_activity` is cluster-wide, so this runs on the maintenance
/// connection: which database it lands in changes nothing about the answer, and
/// the panel has no active database to speak of.
pub(crate) async fn fetch_sessions(db: &Db) -> Result<Vec<SessionInfo>, DbError> {
    let client = connect_maintenance(db).await?;
    let rows = query_all(&client, &pg_activity_sql(activity::MAX_SESSIONS + 1)).await?;
    // The fold is `activity::from_pg_rows` — where every decision about what a
    // `SessionInfo` says lives, reachable from a test with a literal row vector.
    // A row that isn't the projection's arity is refused there, not padded.
    let rows: Vec<activity::PgActivityRow> = rows
        .iter()
        .filter_map(|r| {
            let slots: Vec<Option<String>> = (0..activity::PG_ACTIVITY_COLUMNS.len())
                .map(|i| opt(r, i))
                .collect();
            activity::PgActivityRow::from_slots(&slots)
        })
        .collect();
    Ok(activity::from_pg_rows(&rows))
}

/// The activity query, projecting [`activity::PG_ACTIVITY_COLUMNS`] in order.
fn pg_activity_sql(limit: usize) -> String {
    let projection: Vec<String> = activity::PG_ACTIVITY_COLUMNS
        .iter()
        .map(|c| format!("s.{c}"))
        .collect();
    format!("SELECT {} {PG_ACTIVITY_SQL}{limit}", projection.join(", "))
}

/// This connection's backend pid, or `None` if the server didn't answer.
///
/// The one thing a PostgreSQL client can't learn from the handshake the way a
/// MySQL one can (`Conn::id()`), and the pinned session needs it to recognise
/// itself in `pg_stat_activity` — see [`Session::server_id`](crate::Session::server_id).
pub(crate) async fn backend_pid(client: &Client) -> Option<i64> {
    let rows = query_all(client, "SELECT pg_backend_pid()").await.ok()?;
    opt(rows.first()?, 0)?.parse().ok()
}

/// The PostgreSQL half of [`Db::kill_session`].
///
/// Both functions **answer with a boolean rather than raising**, so the reply is
/// the only place a refusal appears: `false` comes back with a `WARNING`, which
/// is a notice and not an error, and `simple_query` succeeds either way. The
/// answer used to be discarded, and the caller then treated every such kill as
/// completed — killed-session repair, generation bump, refresh, and the row
/// straight back with nothing said anywhere. `activity::kill_verdict` is where
/// the reading of it lives.
///
/// A missing *privilege* does raise, and that error surfaces as it always did.
pub(crate) async fn kill_session(db: &Db, id: i64, kind: KillKind) -> Result<(), DbError> {
    let client = connect_maintenance(db).await?;
    let func = match kind {
        KillKind::Query => "pg_cancel_backend",
        KillKind::Session => "pg_terminate_backend",
    };
    // `id` came from the server as an integer and goes back as a decimal literal.
    let rows = query_all(&client, &format!("SELECT {func}({id})")).await?;
    let cell = rows.first().and_then(|r| opt(r, 0));
    activity::kill_verdict(cell.as_deref(), kind, id).map_err(DbError::Query)
}

pub(crate) async fn fetch_schema(
    db: &Db,
    database: &str,
    cancel: CancellationToken,
) -> Result<DbSchema, DbError> {
    let client = connect_to(db, database).await?;
    // A dozen catalogue queries over every table of the database, so the token
    // is what stops it *on the server* rather than only abandoning the answer —
    // the same shape `count_rows` uses, and for the same reason.
    let token = client.cancel_token();
    tokio::select! {
        r = collect_schema(&client) => r,
        _ = cancel.cancelled() => {
            cancel_query(db, &token).await;
            Err(DbError::Cancelled)
        }
    }
}

/// The catalogue reads themselves, on a client the caller owns — split out so
/// [`fetch_schema`] can race the whole sequence against a cancellation rather
/// than checking a token between each of the dozen queries.
async fn collect_schema(client: &Client) -> Result<DbSchema, DbError> {
    // Every browsable object (BASE TABLE / VIEW / materialized view), across
    // every user schema. See `table_list_sql` for why this can't come from
    // `information_schema`.
    let table_rows: Vec<(String, String, String)> = query_all(client, &table_list_sql())
        .await?
        .into_iter()
        .map(|r| (cell(&r, 0), cell(&r, 1), cell(&r, 2)))
        .collect();

    // Indexes via `pg_catalog` (every index, not just constraint-backed ones),
    // columns in `indkey` order. `unnest(... ) WITH ORDINALITY` preserves column
    // order; the primary-key index is renamed "PRIMARY" so `IndexInfo::is_primary()`
    // (and `create_ddl`) treat it the MySQL way. `pk_set` is derived from it.
    //
    // Two per-key-position columns do the widening the model gained for them:
    //
    // - `pg_get_indexdef(indexrelid, ord, true)` is the key's SQL — the column's
    //   name for an ordinary key, the **expression** for a computed one. The
    //   `pg_attribute` join is LEFT so an expression position survives it at
    //   all: PostgreSQL stores `0` in `indkey` there and has no `pg_attribute`
    //   row to join to.
    // - `indoption`'s bit 0 is **DESC**, joined by ordinality rather than
    //   subscripted so the pairing can't drift.
    //
    // The trailing `lossy` flag is what stops an index edit destroying the parts
    // of an index this query *still* can't see (see `IndexInfo::lossy`). What
    // remains, measured against PostgreSQL 16:
    //
    // - a non-default **operator class** (`… text_pattern_ops`). Nothing
    //   per-column returns it, not even `pg_get_indexdef(oid, colno, …)`.
    // - a **NULLS** ordering that isn't the default for its direction. The
    //   defaults are `ASC NULLS LAST` (`indoption` 0) and `DESC NULLS FIRST`
    //   (3); 1 and 2 are the two spellings that would be lost, since the model
    //   has a field for the direction but none for the nulls ordering.
    //
    // Verified to flag all five shapes on a fixture and to fire **zero** times
    // across the 108 indexes of the `world` and `chinook` sample databases.
    let idx_all = query_all(client, &index_list_sql()).await?;
    let idx_rows: Vec<InSchema<IdxRow>> = idx_all
        .iter()
        .map(|r| {
            let method = cell(r, 6);
            (
                cell(r, 0),
                IdxRow {
                    table: cell(r, 1),
                    index: cell(r, 2),
                    unique: cell(r, 3) == "0",
                    // A key is either a column (`attname` from the LEFT JOIN) or
                    // an expression, in which case there is no `pg_attribute`
                    // row and `pg_get_indexdef`'s per-position text is the key
                    // itself. Prefixes don't exist on PG at all.
                    column: {
                        let name = cell(r, 4);
                        let mut col = if name.is_empty() {
                            IndexColumn::expr(expr_key(&cell(r, 10)))
                        } else {
                            IndexColumn::plain(name)
                        };
                        col.descending = cell(r, 11) == "t";
                        col
                    },
                    // Only worth restating when it isn't the default, or every
                    // generated statement carries a redundant `USING btree`.
                    method: (method != "btree" && !method.is_empty()).then_some(method),
                    predicate: r.get(7).cloned().flatten(),
                    lossy: cell(r, 9) == "t",
                },
            )
        })
        .collect();
    // Primary-key columns = the columns of the primary index (`indisprimary`),
    // keyed by (schema, table, column) so two namespaces don't share a PK set.
    let pk_set: HashSet<(String, String, String)> = idx_all
        .iter()
        .filter(|r| cell(r, 5) == "t" && !cell(r, 4).is_empty())
        .map(|r| (cell(r, 0), cell(r, 1), cell(r, 4)))
        .collect();

    // Columns for the whole schema, in ordinal order — from `pg_catalog`, not
    // `information_schema.columns`.
    //
    // `format_type(atttypid, atttypmod)` is the reason. `udt_name` names the
    // *base* type only, so `varchar(45)` came back as `varchar` and
    // `numeric(10,2)` as `numeric` — the length and precision were being dropped
    // before anything could see them, which made generated DDL wrong and a
    // faithful column edit impossible. `format_type` is what `pg_dump` uses and
    // returns the declared type in full.
    //
    // The rest of the row is equally unavailable from `information_schema`
    // without more joins than it's worth: `pg_get_expr(adbin, adrelid)` renders a
    // default back to SQL text, `attidentity`/`attgenerated` distinguish an
    // identity column from a generated one, and `col_description` carries the
    // comment.
    let col_rows: Vec<InSchema<ColRow>> = query_all(
        client,
        &format!(
            "SELECT n.nspname, c.relname, a.attname, \
                    format_type(a.atttypid, a.atttypmod), \
                    a.attnotnull, \
                    pg_get_expr(ad.adbin, ad.adrelid), \
                    a.attidentity, a.attgenerated, \
                    col_description(c.oid, a.attnum), \
                    co.collname, \
                    EXISTS (SELECT 1 FROM pg_depend d \
                            JOIN pg_class s ON s.oid = d.objid AND s.relkind = 'S' \
                            WHERE d.classid = 'pg_class'::regclass \
                              AND d.refclassid = 'pg_class'::regclass \
                              AND d.refobjid = a.attrelid \
                              AND d.refobjsubid = a.attnum \
                              AND d.deptype IN ('a', 'i')) AS owns_sequence \
             FROM pg_attribute a \
             JOIN pg_class c ON c.oid = a.attrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             LEFT JOIN pg_attrdef ad ON ad.adrelid = a.attrelid AND ad.adnum = a.attnum \
             LEFT JOIN pg_collation co ON co.oid = a.attcollation \
                   AND co.collname <> 'default' \
             WHERE a.attnum > 0 AND NOT a.attisdropped \
               AND c.relkind IN ('r', 'v', 'm', 'p', 'f') \
               AND {} \
             ORDER BY n.nspname, c.relname, a.attnum",
            user_schema_filter("n.nspname")
        ),
    )
    .await?
    .into_iter()
    .map(|r| {
        let (ns, t, c) = (cell(&r, 0), cell(&r, 1), cell(&r, 2));
        // The three mutually-exclusive ways a column gets a value, resolved in
        // one tested place — the catalogue reports them in overlapping fields.
        let a = pg_assignment(
            r.get(5).cloned().flatten(),
            &cell(&r, 6),
            &cell(&r, 7),
            cell(&r, 10) == "t",
        );
        let column = ColumnInfo {
            primary_key: pk_set.contains(&(ns.clone(), t.clone(), c.clone())),
            name: c,
            type_name: cell(&r, 3),
            // `attnotnull` arrives over the text protocol as `t`/`f`.
            nullable: cell(&r, 4) != "t",
            generated: a.generated,
            default: a.default,
            auto_increment: a.auto_increment,
            identity_always: a.identity_always,
            comment: r.get(8).cloned().flatten(),
            collation: r.get(9).cloned().flatten(),
            on_update: None,
            // PostgreSQL has only the stored form of a generated column, and no
            // `AUTOINCREMENT` keyword at all — its counter is an identity
            // column, which `auto_increment`/`identity_always` above carry.
            generated_stored: true,
            sqlite_autoincrement: false,
        };
        (ns, ColRow { table: t, column })
    })
    .collect();

    // Foreign keys via `pg_catalog`: pair `conkey` (referencing) with `confkey`
    // (referenced) by ordinal so composite FKs map their columns correctly (the
    // `constraint_column_usage` join can mis-pair them).
    let fk_all = query_all(
        client,
        &format!(
            "SELECT n.nspname, c.relname, con.conname, a.attname, \
                    rn.nspname, rc.relname, ra.attname, \
                    con.confdeltype, con.confupdtype \
             FROM pg_constraint con \
             JOIN pg_class c ON c.oid = con.conrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             JOIN pg_class rc ON rc.oid = con.confrelid \
             JOIN pg_namespace rn ON rn.oid = rc.relnamespace \
             JOIN unnest(con.conkey) WITH ORDINALITY AS k(attnum, ord) ON true \
             JOIN pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = k.attnum \
             JOIN unnest(con.confkey) WITH ORDINALITY AS fk(attnum, ord) ON fk.ord = k.ord \
             JOIN pg_attribute ra ON ra.attrelid = con.confrelid AND ra.attnum = fk.attnum \
             WHERE con.contype = 'f' AND {} \
             ORDER BY n.nspname, c.relname, con.conname, k.ord",
            user_schema_filter("n.nspname")
        ),
    )
    .await?;
    let fk_col_rows: Vec<InSchema<FkColRow>> = fk_all
        .iter()
        .map(|r| {
            (
                cell(r, 0),
                (
                    cell(r, 1),
                    cell(r, 2),
                    cell(r, 3),
                    // The *referenced* namespace: kept as-is (it may point into
                    // another schema) and consumed by `follow_target`.
                    Some(cell(r, 4)),
                    Some(cell(r, 5)),
                    Some(cell(r, 6)),
                ),
            )
        })
        .collect();
    // Referential actions, per constraint. A key that isn't restated with its
    // `ON DELETE CASCADE` comes back as `NO ACTION`, so a schema editor that
    // drops and recreates one has to know it.
    // `(namespace, table, constraint, on_delete, on_update)`.
    type FkRule = (String, String, String, Option<String>, Option<String>);
    let fk_rules: Vec<FkRule> = fk_all
        .iter()
        .map(|r| {
            (
                cell(r, 0),
                cell(r, 1),
                cell(r, 2),
                fk_action(&cell(r, 7)),
                fk_action(&cell(r, 8)),
            )
        })
        .collect();

    // View bodies and options, from `pg_catalog` rather than
    // `information_schema.views`. Two reasons, both the kind that only show up
    // in someone else's database: that view hands back an **empty** definition
    // to anyone who doesn't own the view, and it doesn't list materialized views
    // at all (they aren't in the SQL standard), so a matview's body read as
    // `None`. `pg_get_viewdef` answers for both, and `reloptions` carries the
    // storage parameters a `CREATE OR REPLACE` would otherwise reset —
    // `security_barrier` among them, whose loss makes a view leak the rows it
    // was written to hide.
    let view_all = query_all(
        client,
        &format!(
            "SELECT n.nspname, c.relname, pg_get_viewdef(c.oid, true), \
                    c.relkind, array_to_string(c.reloptions, ','), \
                    c.relispopulated \
             FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE c.relkind IN ('v', 'm') AND {}",
            user_schema_filter("n.nspname")
        ),
    )
    .await?;
    let view_rows: Vec<InSchema<(String, String)>> = view_all
        .iter()
        .map(|r| (cell(r, 0), (cell(r, 1), cell(r, 2))))
        .collect();

    // Partition every row set by namespace, then fold one namespace at a time —
    // `assemble_schema` keys on table name alone, so a shared call would merge
    // `public.orders` with `sales.orders`.
    let mut namespaces: Vec<String> = table_rows.iter().map(|(ns, ..)| ns.clone()).collect();
    namespaces.sort_by_key(|n| schema_sort_key(n));
    namespaces.dedup();

    /// Keep only the rows tagged with `ns`, dropping the tag.
    fn of<T: Clone>(rows: &[InSchema<T>], ns: &str) -> Vec<T> {
        rows.iter()
            .filter(|(s, _)| s == ns)
            .map(|(_, r)| r.clone())
            .collect()
    }
    let mut tables = Vec::new();
    for ns in &namespaces {
        let t: Vec<(String, String)> = table_rows
            .iter()
            .filter(|(s, ..)| s == ns)
            .map(|(_, name, ty)| (name.clone(), ty.clone()))
            .collect();
        let schema = assemble_schema(
            Some(ns),
            &t,
            &of(&col_rows, ns),
            &of(&fk_col_rows, ns),
            &of(&idx_rows, ns),
            &of(&view_rows, ns),
        );
        tables.extend(schema.tables);
    }

    // CHECK constraints. `pg_get_constraintdef` re-prints the predicate from its
    // parse tree and returns the whole clause (`CHECK ((total >= 0))`), which
    // `ddl::check_predicate` reduces to the bare predicate the model stores.
    //
    // `contype = 'c'` is only user checks: PostgreSQL 17 records a NOT NULL
    // constraint here too, under `'n'`, and `conislocal` keeps an inherited
    // constraint from being restated on the child that merely inherits it.
    let check_all = query_all(
        client,
        &format!(
            "SELECT n.nspname, c.relname, con.conname, pg_get_constraintdef(con.oid) \
             FROM pg_constraint con \
             JOIN pg_class c ON c.oid = con.conrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE con.contype = 'c' AND con.conislocal AND {} \
             ORDER BY n.nspname, c.relname, con.conname",
            user_schema_filter("n.nspname")
        ),
    )
    .await?;

    // Triggers. `NOT tgisinternal` is doing real work: every foreign key is
    // implemented as a pair of hidden triggers, so without it each FK would show
    // up as two triggers the user never wrote and a designer would offer to drop
    // — taking the constraint with them. It keeps user-written `CREATE
    // CONSTRAINT TRIGGER`s, which are visible objects and `tgconstraint <> 0`.
    //
    // `tgparentid <> 0` is a trigger *cloned onto a partition* by a trigger on
    // the parent. It is `tgisinternal = false`, so the filter above keeps it:
    // a 100-partition table listed the same trigger 101 times, and every drop
    // or edit of a clone failed ("trigger pt on table p1 ... requires it"),
    // because a clone can only be dropped through its parent.
    let trigger_all = query_all(
        client,
        &format!(
            "SELECT n.nspname, c.relname, t.tgname, t.tgtype::int, t.tgenabled, \
                    (t.tgconstraint <> 0)::int, t.tgfoid::regproc::text, \
                    pg_get_triggerdef(t.oid), \
                    COALESCE(t.tgoldtable, ''), COALESCE(t.tgnewtable, '') \
             FROM pg_trigger t \
             JOIN pg_class c ON c.oid = t.tgrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             WHERE NOT t.tgisinternal AND t.tgparentid = 0 AND {} \
             ORDER BY n.nspname, c.relname, t.tgname",
            user_schema_filter("n.nspname")
        ),
    )
    .await?;

    // `UPDATE OF a, b` — **one row per column**, not a `string_agg`.
    //
    // The aggregate that was here joined on `','` and was split back on `','`,
    // so an `UPDATE OF "a,b"` trigger read back as two columns that don't
    // exist. Same rule the enum labels beside it already follow: every
    // separator is a value some database already stores, so the fold belongs in
    // Rust.
    //
    // `tgattr` is an `int2vector` whose text form is space-separated attnums;
    // going through `string_to_array` avoids depending on an int2vector→array
    // cast, and `NULLIF` makes the empty case yield no rows rather than one
    // empty element.
    let trigger_cols_all = query_all(
        client,
        &format!(
            "SELECT n.nspname, c.relname, t.tgname, a.attname \
             FROM pg_trigger t \
             JOIN pg_class c ON c.oid = t.tgrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             CROSS JOIN LATERAL unnest(string_to_array(NULLIF(t.tgattr::text, ''), ' ')) \
                  WITH ORDINALITY AS x(attnum, ord) \
             JOIN pg_attribute a ON a.attrelid = t.tgrelid \
                                AND a.attnum = x.attnum::smallint \
             WHERE NOT t.tgisinternal AND t.tgparentid = 0 AND {} \
             ORDER BY n.nspname, c.relname, t.tgname, x.ord",
            user_schema_filter("n.nspname")
        ),
    )
    .await?;
    let mut trigger_cols: HashMap<(String, String, String), Vec<String>> = HashMap::new();
    for r in &trigger_cols_all {
        trigger_cols
            .entry((cell(r, 0), cell(r, 1), cell(r, 2)))
            .or_default()
            .push(cell(r, 3));
    }

    // Post-fold enrichment: the things `assemble_schema` can't carry because
    // MySQL has no equivalent — an index's backing constraint, and each foreign
    // key's referential actions — plus the checks, which it has no field for on
    // either engine's row shape.
    let idx_constraints: HashMap<(String, String, String), String> = idx_all
        .iter()
        .filter_map(|r| {
            let name = cell(r, 8);
            (!name.is_empty()).then(|| ((cell(r, 0), cell(r, 1), cell(r, 2)), name))
        })
        .collect();
    let view_options: HashMap<(String, String), ViewOptions> = view_all
        .iter()
        .map(|r| {
            (
                (cell(r, 0), cell(r, 1)),
                pg_view_options(&cell(r, 3), &cell(r, 4), &cell(r, 5)),
            )
        })
        .collect();
    for t in &mut tables {
        let ns = t.schema.clone().unwrap_or_default();
        if t.is_view {
            t.view_options = view_options.get(&(ns.clone(), t.name.clone())).cloned();
        }
        t.check_constraints = check_all
            .iter()
            .filter(|r| cell(r, 0) == ns && cell(r, 1) == t.name)
            .map(|r| CheckInfo {
                name: cell(r, 2),
                expression: schemaic_core::ddl::check_predicate(
                    &cell(r, 3),
                    schemaic_core::intel::SqlDialect::Postgres,
                ),
                // PostgreSQL has no `NOT ENFORCED`; its `NOT VALID` exempts only
                // the rows already there, which can't change what a write does.
                enforced: true,
                // …but it is still part of the clause, and restating it wrong
                // changes what the table promises. Read from the same text the
                // predicate came out of.
                validated: schemaic_core::ddl::check_clause_flags(&cell(r, 3)).0,
                inherited: schemaic_core::ddl::check_clause_flags(&cell(r, 3)).1,
                // MariaDB's alone: PostgreSQL folds a column-level `CHECK` into
                // a table constraint at `CREATE` time, and `pg_constraint` has
                // no other kind.
                column_level: false,
            })
            .collect();
        t.triggers = trigger_all
            .iter()
            .filter(|r| cell(r, 0) == ns && cell(r, 1) == t.name)
            .map(|r| {
                let (timing, events, level) =
                    pg_trigger_type(cell(r, 3).parse::<i32>().unwrap_or_default());
                TriggerInfo {
                    name: cell(r, 2),
                    schema: t.schema.clone(),
                    table: t.name.clone(),
                    timing,
                    events,
                    update_columns: trigger_cols
                        .get(&(ns.clone(), t.name.clone(), cell(r, 2)))
                        .cloned()
                        .unwrap_or_default(),
                    level,
                    // Held bare; `create_sql` is the only thing that wraps it.
                    condition: pg_trigger_when(&cell(r, 7))
                        .map(|w| {
                            schemaic_core::ddl::trigger_condition(
                                &w,
                                schemaic_core::intel::SqlDialect::Postgres,
                            )
                        })
                        .filter(|w| !w.is_empty()),
                    action: TriggerAction::Function {
                        name: cell(r, 6),
                        args: pg_trigger_args(&cell(r, 7)),
                    },
                    // MySQL's alone — a PostgreSQL trigger has no body of its
                    // own, so no session state was captured with one.
                    definer: None,
                    order: None,
                    sql_mode: None,
                    charset_client: None,
                    collation_connection: None,
                    old_table: Some(cell(r, 8)).filter(|s| !s.is_empty()),
                    new_table: Some(cell(r, 9)).filter(|s| !s.is_empty()),
                    // All four of `tgenabled`'s states: `A`/`R` used to fold
                    // into "enabled" and be recreated as `O`, which changes
                    // what fires during replication apply.
                    enabled: TriggerEnabled::parse(&cell(r, 4)),
                    constraint: cell(r, 5) == "1",
                }
            })
            .collect();
        for ix in &mut t.indexes {
            ix.constraint = idx_constraints
                .get(&(ns.clone(), t.name.clone(), ix.name.clone()))
                .cloned();
        }
        for (rns, rtable, rname, on_delete, on_update) in &fk_rules {
            if *rns == ns
                && *rtable == t.name
                && let Some(fk) = t.foreign_keys.iter_mut().find(|f| f.name == *rname)
            {
                fk.on_delete = on_delete.clone();
                fk.on_update = on_update.clone();
            }
        }
    }

    // The standalone objects. They hang off the database, not off any table, so
    // they're gathered after the per-table fold rather than inside it.
    let (enums, domains) = pg_types(client).await?;
    Ok(DbSchema {
        tables,
        enums,
        domains,
        sequences: pg_sequences(client).await?,
        routines: routines_on(client)
            .await?
            .into_iter()
            .map(std::sync::Arc::new)
            .collect(),
        // PostgreSQL has no scheduled events — `pg_cron` is an extension with
        // its own catalogue and no `CREATE EVENT` grammar — so this stays empty
        // and `ddl::supports_event_editing` is false here.
        events: Vec::new(),
        // A MySQL-family flavour is meaningless here, and `Unknown` is what
        // makes the emitter withhold MariaDB-specific behaviour rather than
        // assume it.
        flavour: schemaic_core::schema::ServerFlavour::Unknown,
    })
}

/// The enum types and domains of every user namespace, plus each one's labels
/// and constraints.
///
/// Enums and domains come out of **one** `pg_type` scan because that is where
/// both live (`typtype` `e` and `d`); splitting them would be two round trips
/// over the same rows. The labels and the domain constraints then need one query
/// each, and the labels can't be string-aggregated into the first: an enum label
/// is arbitrary text up to 63 bytes and may contain a comma, a newline, or
/// nothing at all, so **any** separator is a value some database already uses.
/// One row per label is the only rendering that can't be misread.
///
/// A domain's collation is reported only when it differs from its base type's.
/// PostgreSQL fills `typcollation` in for every collatable domain whether or not
/// the statement said `COLLATE`, so restating it unconditionally would put a
/// `COLLATE "en_US.utf8"` on every `text` domain and make each one open with a
/// phantom change against the round-trip gate.
async fn pg_types(client: &Client) -> Result<(Vec<EnumInfo>, Vec<DomainInfo>), DbError> {
    let filter = user_schema_filter("n.nspname");
    let type_rows = query_all_optional(
        client,
        &format!(
            "SELECT n.nspname, t.typname, t.typtype::text, \
                    CASE WHEN t.typtype = 'd' \
                         THEN format_type(t.typbasetype, t.typtypmod) ELSE '' END, \
                    COALESCE(pg_get_expr(t.typdefaultbin, 0), t.typdefault, ''), \
                    t.typnotnull::int, COALESCE(co.collname, ''), \
                    COALESCE(obj_description(t.oid, 'pg_type'), ''), \
                    (t.typdefaultbin IS NOT NULL OR t.typdefault IS NOT NULL)::int, \
                    COALESCE(cn.nspname, '') \
             FROM pg_type t \
             JOIN pg_namespace n ON n.oid = t.typnamespace \
             LEFT JOIN pg_type bt ON bt.oid = t.typbasetype \
             LEFT JOIN pg_collation co ON co.oid = t.typcollation \
                                      AND t.typcollation <> bt.typcollation \
             LEFT JOIN pg_namespace cn ON cn.oid = co.collnamespace \
             WHERE t.typtype IN ('e', 'd') AND {filter} \
             ORDER BY n.nspname, t.typname"
        ),
    )
    .await?;
    if type_rows.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }

    let label_rows = query_all_optional(
        client,
        &format!(
            "SELECT n.nspname, t.typname, e.enumlabel \
             FROM pg_enum e \
             JOIN pg_type t ON t.oid = e.enumtypid \
             JOIN pg_namespace n ON n.oid = t.typnamespace \
             WHERE {filter} \
             ORDER BY n.nspname, t.typname, e.enumsortorder"
        ),
    )
    .await?;
    let constraint_rows = query_all_optional(
        client,
        &format!(
            "SELECT n.nspname, t.typname, c.conname, pg_get_constraintdef(c.oid) \
             FROM pg_constraint c \
             JOIN pg_type t ON t.oid = c.contypid \
             JOIN pg_namespace n ON n.oid = t.typnamespace \
             WHERE c.contype = 'c' AND {filter} \
             ORDER BY n.nspname, t.typname, c.conname"
        ),
    )
    .await?;

    Ok(pg_fold_types(&type_rows, &label_rows, &constraint_rows))
}

/// Fold the three `pg_type` row sets into the model — the pure half of
/// [`pg_types`], separated so the decisions in it can be tested without a server.
///
/// Two of those decisions are only visible in an edge case a live database
/// happily produces. A domain's default may be the **empty string**, so "has a
/// default" is a flag of its own rather than a non-empty test on the text — read
/// the other way, `DEFAULT ''` would come back as no default and every replay
/// would silently drop it. And an enum's labels are matched by `(namespace,
/// name)` rather than positionally, because the label query is ordered by sort
/// order within a type and a type with **no** labels contributes no rows at all.
fn pg_fold_types(
    type_rows: &[Vec<Option<String>>],
    label_rows: &[Vec<Option<String>>],
    constraint_rows: &[Vec<Option<String>>],
) -> (Vec<EnumInfo>, Vec<DomainInfo>) {
    let (mut enums, mut domains) = (Vec::new(), Vec::new());
    for r in type_rows {
        let (ns, name) = (cell(r, 0), cell(r, 1));
        let of = |rows: &[Vec<Option<String>>]| -> Vec<Vec<Option<String>>> {
            rows.iter()
                .filter(|x| cell(x, 0) == ns && cell(x, 1) == name)
                .cloned()
                .collect()
        };
        let comment = Some(cell(r, 7)).filter(|c| !c.is_empty());
        if cell(r, 2) == "e" {
            enums.push(EnumInfo {
                schema: Some(ns.clone()),
                name: name.clone(),
                values: of(label_rows).iter().map(|l| cell(l, 2)).collect(),
                comment,
            });
        } else {
            domains.push(DomainInfo {
                schema: Some(ns.clone()),
                name: name.clone(),
                base_type: cell(r, 3),
                collation: Some(cell(r, 6)).filter(|c| !c.is_empty()),
                // The namespace the collation is in. `pg_catalog` is dropped:
                // it is searched first and can't be shadowed, so a built-in
                // needs no qualifier and qualifying it would rewrite the DDL of
                // every domain that already round-trips.
                collation_schema: Some(cell(r, 9)).filter(|s| !s.is_empty() && s != "pg_catalog"),
                default_value: (cell(r, 8) == "1").then(|| cell(r, 4)),
                not_null: cell(r, 5) == "1",
                checks: of(constraint_rows)
                    .iter()
                    .map(|c| CheckInfo {
                        name: cell(c, 2),
                        // The same normalization a table's checks get: the server
                        // hands back the whole `CHECK (…)` clause and the model
                        // holds the predicate bare.
                        expression: schemaic_core::ddl::check_predicate(
                            &cell(c, 3),
                            schemaic_core::intel::SqlDialect::Postgres,
                        ),
                        // PostgreSQL has no `NOT ENFORCED`.
                        enforced: true,
                        // A domain's checks share the table path's parser, and
                        // so shared its bug: a `NOT VALID` one made every
                        // statement the domain emitted a syntax error.
                        validated: schemaic_core::ddl::check_clause_flags(&cell(c, 3)).0,
                        inherited: schemaic_core::ddl::check_clause_flags(&cell(c, 3)).1,
                        // A domain's constraints are never column-level; that is
                        // MariaDB's, and a domain has no columns.
                        column_level: false,
                    })
                    .collect(),
                comment,
            });
        }
    }
    (enums, domains)
}

/// Every sequence in every user namespace, with what owns it.
///
/// The definition comes from the `pg_sequence` **catalogue** rather than the
/// `pg_sequences` view, and `last_value` from the view: the catalogue is
/// readable whatever the role's privileges on the sequence itself, while the
/// counter's position is exactly the part that needs `SELECT`/`USAGE`. Read
/// together, a role that may see the schema but not the data gets a complete
/// definition and a blank position, instead of the sequence vanishing.
///
/// `deptype` is what separates the two kinds: `a` is a `serial` column's
/// sequence — an object in its own right that merely gets dropped with its
/// column — and `i` is an identity column's counter, which *is* part of the
/// column and which PostgreSQL refuses to drop separately.
async fn pg_sequences(client: &Client) -> Result<Vec<SequenceInfo>, DbError> {
    let rows = query_all_optional(
        client,
        &format!(
            "SELECT n.nspname, c.relname, format_type(s.seqtypid, NULL), \
                    s.seqstart::text, s.seqincrement::text, s.seqmin::text, \
                    s.seqmax::text, s.seqcache::text, s.seqcycle::int, \
                    COALESCE(ps.last_value::text, ''), \
                    COALESCE(dt.relname, ''), COALESCE(da.attname, ''), \
                    COALESCE(d.deptype, ''), \
                    COALESCE(obj_description(c.oid, 'pg_class'), '') \
             FROM pg_sequence s \
             JOIN pg_class c ON c.oid = s.seqrelid \
             JOIN pg_namespace n ON n.oid = c.relnamespace \
             LEFT JOIN pg_sequences ps \
                    ON ps.schemaname = n.nspname AND ps.sequencename = c.relname \
             LEFT JOIN pg_depend d \
                    ON d.classid = 'pg_class'::regclass AND d.objid = c.oid \
                   AND d.refclassid = 'pg_class'::regclass \
                   AND d.deptype IN ('a', 'i') \
             LEFT JOIN pg_class dt ON dt.oid = d.refobjid \
             LEFT JOIN pg_attribute da \
                    ON da.attrelid = d.refobjid AND da.attnum = d.refobjsubid \
             WHERE {} \
             ORDER BY n.nspname, c.relname",
            user_schema_filter("n.nspname")
        ),
    )
    .await?;
    Ok(rows.iter().map(|r| pg_sequence_row(r)).collect())
}

/// One `pg_sequence` row as a [`SequenceInfo`] — the pure half of
/// [`pg_sequences`].
///
/// The bounds come back as **text** and are parsed here rather than read as
/// integers, because `seqmax` on a `bigint` sequence is `i64::MAX` and the text
/// protocol is the only path that carries it without a decoder. Each falls back
/// to PostgreSQL's own default rather than to zero: a sequence claiming
/// `INCREMENT BY 0` is a statement the server rejects, so a parse failure that
/// degraded to `Default` would turn a display glitch into un-runnable DDL.
fn pg_sequence_row(r: &[Option<String>]) -> SequenceInfo {
    let num = |i: usize, fallback: i64| cell(r, i).parse::<i64>().unwrap_or(fallback);
    let owner_table = cell(r, 10);
    SequenceInfo {
        schema: Some(cell(r, 0)),
        name: cell(r, 1),
        data_type: cell(r, 2),
        start: num(3, 1),
        increment: num(4, 1),
        min_value: num(5, 1),
        max_value: num(6, i64::MAX),
        cache: num(7, 1),
        cycle: cell(r, 8) == "1",
        // Blank means "never used, or this role may not look" — both of which are
        // "no position to show", and neither of which is a position of 0.
        last_value: cell(r, 9).parse::<i64>().ok(),
        owned_by: (!owner_table.is_empty()).then(|| SequenceOwner {
            table: owner_table,
            column: cell(r, 11),
            // `i` is an identity column's counter — part of the column, and
            // undroppable on its own. `a` is a `serial`'s, which is a real object.
            internal: cell(r, 12) == "i",
        }),
        comment: Some(cell(r, 13)).filter(|c| !c.is_empty()),
    }
}

/// Which `pg_proc` rows are routines **at all** — the floor both readers below
/// stand on.
///
/// Two narrowings:
///
/// * **`prokind IN ('f', 'p')`** — functions and procedures. Aggregates (`a`)
///   and window functions (`w`) live in the same catalogue and are neither: they
///   have no body to show (`prosrc` names a transition function or a C symbol)
///   and no `CREATE FUNCTION` that would recreate them, so listing them would
///   be a row whose editor cannot open. The column is **PostgreSQL 11 and
///   later**; 10 went end-of-life in November 2022, and there is no pre-11
///   spelling that separates a procedure from an aggregate — `proisagg` says
///   only which one an aggregate is, and procedures did not exist to be told
///   apart.
/// * **A user namespace** — the same filter every other query here uses.
fn routine_scope() -> String {
    format!(
        "p.prokind IN ('f', 'p') AND {}",
        user_schema_filter("n.nspname")
    )
}

/// …and of those, the ones the user **owns** — what the schema tree browses.
///
/// Adds: **not owned by an extension** (`pg_depend.deptype = 'e'`). This is
/// *not* applied to the types and sequences beside it, and the difference is one
/// of degree rather than principle: an extension installs a handful of types and
/// hundreds of functions. PostGIS alone puts ~1000 into whichever namespace it
/// was created in, which is `public` by default — so without this the Functions
/// folder on a PostGIS database is a wall of `st_*` with the user's own routines
/// lost inside it, and every schema refresh carries their bodies. They remain
/// callable, completable and documented by the extension that owns them; what
/// they aren't is the user's to edit.
///
/// **Which is why [`trigger_functions`] does not use this one.** A trigger binds
/// to whatever returns `trigger`, extension-owned or not — `moddatetime` is the
/// standard "touch the modified column" function and arrives exactly this way —
/// and the picker that reads it is a dropdown with no free-text entry, so
/// filtering here would put those triggers permanently out of reach.
fn routine_filter() -> String {
    format!(
        "{} AND NOT EXISTS (SELECT 1 FROM pg_depend d \
                            WHERE d.objid = p.oid \
                              AND d.classid = 'pg_proc'::regclass \
                              AND d.deptype = 'e')",
        routine_scope()
    )
}

/// …and of those, the ones a **trigger can bind to**.
///
/// Narrowed on the server rather than in Rust: the alternative is shipping every
/// routine body in the database over the wire to keep the handful that return
/// `trigger`, on a call the trigger editor makes every time the routine editor
/// closes back to it.
///
/// Both return types, because [`RoutineInfo::is_trigger_function`] accepts both
/// and the two lists must agree — an `event_trigger` function the model calls
/// bindable but the query never returns is a row that can't be selected.
fn trigger_function_filter() -> String {
    format!(
        "{} AND p.prorettype IN ('trigger'::regtype, 'event_trigger'::regtype)",
        routine_scope()
    )
}

/// Every function a trigger can bind to, on its own connection — see
/// [`trigger_function_filter`] for why this is its own query rather than a
/// filter over [`routines_on`].
pub(crate) async fn trigger_functions(
    db: &Db,
    database: &str,
) -> Result<Vec<RoutineInfo>, DbError> {
    let client = connect_to(db, database).await?;
    routines_where(&client, &trigger_function_filter()).await
}

/// The roles a database or schema could be **owned by**, in name order.
///
/// A *shortcut* for the Owner field, which stays free text — so this list only
/// has to be useful, not exhaustive, and a role created since the modal opened
/// can still be typed. The `pg_` prefixed roles are excluded because they are
/// the server's own bookkeeping (`pg_read_all_data`, `pg_monitor` and their
/// kin): they are not who anything is owned by, and they would be most of a
/// short list on a fresh cluster.
///
/// **Both login roles and groups**, deliberately — `OWNER` takes either, and
/// filtering on `rolcanlogin` would hide precisely the group roles a database is
/// most often deliberately owned by.
///
/// Server-level, on the maintenance connection: `pg_roles` is a cluster-wide
/// catalogue, and the caller may be asking in order to create a database that
/// does not exist yet.
pub(crate) async fn roles(db: &Db) -> Result<Vec<String>, DbError> {
    let client = connect_maintenance(db).await?;
    // **`left(…, 3)`, not `NOT LIKE 'pg\_%'`.** That pattern needs the backslash
    // to survive the string literal and reach `LIKE` as its escape, which is
    // true only while `standard_conforming_strings` is on. With it off — legacy,
    // and still settable per database or per role — the literal consumes the
    // backslash, `LIKE` sees `pg_%` with `_` as a single-character wildcard, and
    // roles named `pgadmin` or `pgbouncer` vanish from the menu on the very
    // server most likely to have them. A prefix comparison has no escaping to
    // depend on.
    let rows = query_all(
        &client,
        "SELECT rolname FROM pg_roles WHERE left(rolname, 3) <> 'pg_' ORDER BY rolname",
    )
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| r.into_iter().next().flatten())
        .collect())
}

/// The PostgreSQL half of [`Db::fetch_principals`](crate::Db::fetch_principals).
///
/// Server-level, on the maintenance connection: `pg_roles` is a cluster-wide
/// catalogue and the browser may be open on a connection with no database
/// selected. Unlike [`roles`] this keeps the `pg_` predefined roles — they hold
/// real privileges and "why can this account read that" is the question the
/// browser exists to answer — and `users::from_pg_rows` sorts them last.
pub(crate) async fn fetch_principals(db: &Db) -> Result<users::Principals, DbError> {
    let client = connect_maintenance(db).await?;
    let rows = query_all(&client, PG_ROLES_SQL).await?;
    let rows: Vec<PgRoleRow> = rows
        .iter()
        .filter(|r| r.len() >= 10)
        .map(|r| PgRoleRow {
            name: cell(r, 0),
            superuser: pg_bool(r, 1),
            inherit: pg_bool(r, 2),
            createrole: pg_bool(r, 3),
            createdb: pg_bool(r, 4),
            canlogin: pg_bool(r, 5),
            replication: pg_bool(r, 6),
            bypassrls: pg_bool(r, 7),
            // Not `num`, which refuses a negative — and `-1` is precisely the
            // value that means "no limit" and has to survive as one.
            connlimit: opt(r, 8).and_then(|s| s.parse().ok()).unwrap_or(-1),
            valid_until: opt(r, 9),
        })
        .collect();
    // Complete: `pg_roles` is world-readable, so unlike MySQL's `mysql.user`
    // there is no rung below this one and nothing to disclose.
    Ok(users::Principals::complete(users::from_pg_rows(&rows)))
}

/// `pg_roles` in the order [`PgRoleRow`] reads it.
const PG_ROLES_SQL: &str = "SELECT rolname, rolsuper, rolinherit, rolcreaterole, rolcreatedb, \
            rolcanlogin, rolreplication, rolbypassrls, rolconnlimit, \
            CAST(rolvaliduntil AS text) \
     FROM pg_catalog.pg_roles ORDER BY rolname";

/// A boolean column from `simple_query`'s text protocol, which spells it `t`/`f`.
fn pg_bool(row: &[Option<String>], i: usize) -> bool {
    opt(row, i).as_deref() == Some("t")
}

/// The PostgreSQL half of [`Db::fetch_grants`](crate::Db::fetch_grants).
///
/// **Four queries, because PostgreSQL has no `SHOW GRANTS`.** Privileges live as
/// `aclitem[]` columns on the objects themselves, one catalogue per kind, and
/// `aclexplode` is what turns each array into rows. `users::pg_grant_statements`
/// puts them back together — the reassembly is there, with the tests, and not
/// here, because it is a decision about what a privilege is *called* and this is
/// only the fetch.
///
/// **Three of the four see one database only.** `pg_database` is cluster-wide,
/// but `pg_namespace` and `pg_class` are per-database catalogues, so a
/// connection answers for the database it is attached to and knows nothing about
/// the others. That is what [`users::pg_scope_note`] says on the screen, rather
/// than a list that looks complete and isn't.
pub(crate) async fn fetch_grants(
    db: &Db,
    database: Option<&str>,
    principal: &Principal,
) -> Result<Grants, DbError> {
    // No database selected: the cluster-wide half is still answerable, and a
    // maintenance connection is where `fetch_principals` already stands.
    let (client, scope) = match database {
        Some(d) => (connect_to(db, d).await?, Some(d.to_string())),
        None => (connect_maintenance(db).await?, None),
    };
    let who = pg_str_lit(&principal.name);
    // Every ACL query compares against the role's own oid. A role that has since
    // been dropped yields NULL, which matches nothing — an empty list, not an
    // error, which is the right answer to "what may a role that isn't there do".
    let oid = format!("(SELECT oid FROM pg_catalog.pg_roles WHERE rolname = {who})");

    let members = query_all(
        &client,
        &format!(
            "SELECT r.rolname, m.admin_option FROM pg_catalog.pg_auth_members m \
             JOIN pg_catalog.pg_roles r ON r.oid = m.roleid \
             WHERE m.member = {oid} ORDER BY r.rolname"
        ),
    )
    .await?;
    let member_of: Vec<(String, bool)> = members
        .iter()
        .map(|r| (cell(r, 0), pg_bool(r, 1)))
        .collect();

    let mut acl: Vec<PgAclRow> = Vec::new();
    let databases = query_all(
        &client,
        &format!(
            "SELECT d.datname, a.privilege_type, a.is_grantable \
             FROM pg_catalog.pg_database d, aclexplode(d.datacl) a \
             WHERE a.grantee = {oid} \
             ORDER BY d.datname, a.is_grantable, a.privilege_type"
        ),
    )
    .await?;
    acl.extend(databases.iter().map(|r| PgAclRow {
        kind: PgObjectKind::Database,
        schema: None,
        name: cell(r, 0),
        privilege: cell(r, 1),
        grantable: pg_bool(r, 2),
    }));

    // **The per-database half runs only when a database was named.** With none
    // selected the client above stands on PostgreSQL's *maintenance* database,
    // and these two queries would answer for it — a database the user did not
    // choose and is not looking at — while the note said nothing at all. See
    // `users::pg_no_database_note`, which is what is said instead.
    if scope.is_some() {
        let schemas = query_all(
            &client,
            &format!(
                "SELECT n.nspname, a.privilege_type, a.is_grantable \
             FROM pg_catalog.pg_namespace n, aclexplode(n.nspacl) a \
             WHERE a.grantee = {oid} \
             ORDER BY n.nspname, a.is_grantable, a.privilege_type"
            ),
        )
        .await?;
        acl.extend(schemas.iter().map(|r| PgAclRow {
            kind: PgObjectKind::Schema,
            schema: None,
            name: cell(r, 0),
            privilege: cell(r, 1),
            grantable: pg_bool(r, 2),
        }));

        // Views and materialised views are granted `ON TABLE` like the tables they
        // stand in for; only a sequence takes its own keyword. `relkind` is filtered
        // rather than left open so an index or a composite type — which have no
        // `GRANT` spelling at all — can't reach the statement builder.
        let rels = query_all(
            &client,
            &format!(
                "SELECT n.nspname, c.relname, c.relkind, a.privilege_type, a.is_grantable \
             FROM pg_catalog.pg_class c \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             CROSS JOIN LATERAL aclexplode(c.relacl) a \
             WHERE a.grantee = {oid} AND c.relkind IN ('r', 'p', 'v', 'm', 'f', 'S') \
             ORDER BY n.nspname, c.relname, a.is_grantable, a.privilege_type"
            ),
        )
        .await?;
        acl.extend(rels.iter().map(|r| PgAclRow {
            kind: if cell(r, 2) == "S" {
                PgObjectKind::Sequence
            } else {
                PgObjectKind::Table
            },
            schema: Some(cell(r, 0)),
            name: cell(r, 1),
            privilege: cell(r, 3),
            grantable: pg_bool(r, 4),
        }));
    }

    // **What no ACL entry records.** `aclexplode` reads explicit entries only,
    // and the three commonest ways a role holds power leave none: superuser,
    // ownership, and a `pg_*` predefined role's wired-in rights. Without this,
    // clicking `pg_read_all_data` printed "This account holds no privileges."
    // See `users::pg_implicit_note`.
    let implicit = query_all(
        &client,
        &format!(
            "SELECT (SELECT rolsuper FROM pg_catalog.pg_roles WHERE rolname = {who}), \
                    (SELECT count(*) FROM pg_catalog.pg_database WHERE datdba = {oid}) \
                  + (SELECT count(*) FROM pg_catalog.pg_namespace WHERE nspowner = {oid}) \
                  + (SELECT count(*) FROM pg_catalog.pg_class \
                     WHERE relowner = {oid} AND relkind IN ('r', 'p', 'v', 'm', 'f', 'S'))"
        ),
    )
    .await?;
    let extra = implicit.first().and_then(|r| {
        users::pg_implicit_note(
            &principal.name,
            pg_bool(r, 0),
            opt(r, 1).and_then(|s| s.parse().ok()).unwrap_or(0),
            scope.as_deref().unwrap_or("this cluster"),
        )
    });

    let mut statements = users::pg_membership_statements(&principal.name, &member_of);
    statements.extend(users::pg_grant_statements(&principal.name, &acl));
    Ok(Grants {
        statements,
        note: Some(match scope.as_deref() {
            Some(d) => users::pg_scope_note(d, extra.as_deref()),
            None => users::pg_no_database_note(extra.as_deref()),
        }),
    })
}

/// The browse list against a client the caller already has — what `fetch_schema`
/// uses, so a schema refresh doesn't open a second connection for it.
pub(crate) async fn routines_on(client: &Client) -> Result<Vec<RoutineInfo>, DbError> {
    routines_where(client, &routine_filter()).await
}

/// The read itself, over whichever `WHERE` its caller stands on.
///
/// `proconfig` comes back **one row per setting**, keyed on the function's oid.
///
/// It was `array_to_string(proconfig, '|')` split back on `'|'`, chosen because
/// a value can contain a comma — but a value can contain a `|` just as easily,
/// and `SET search_path = "my|schema"` then emitted two broken `SET` clauses on
/// a `SECURITY DEFINER` function, which is the privilege-escalation shape
/// [`RoutineInfo::settings`] exists to preserve. Every separator is a value some
/// database already stores; the fold belongs in Rust, as the enum labels beside
/// it already do.
///
/// Both queries take the **same** filter: the settings fold is keyed on oid, so
/// a filter that disagreed with the row query would silently attach one
/// routine's `SET` clauses to nothing at all.
async fn routines_where(client: &Client, filter: &str) -> Result<Vec<RoutineInfo>, DbError> {
    let settings_all = query_all(
        client,
        &format!(
            "SELECT p.oid::text, s.setting \
             FROM pg_proc p \
             JOIN pg_namespace n ON n.oid = p.pronamespace \
             CROSS JOIN LATERAL unnest(p.proconfig) WITH ORDINALITY AS s(setting, ord) \
             WHERE {filter} \
             ORDER BY p.oid, s.ord"
        ),
    )
    .await?;
    let mut settings: HashMap<String, Vec<String>> = HashMap::new();
    for r in &settings_all {
        settings.entry(cell(r, 0)).or_default().push(cell(r, 1));
    }
    Ok(query_all(
        client,
        &format!(
            // Both argument renderings, because they are different strings and
            // each is a syntax error where the other belongs:
            // `pg_get_function_arguments` prints `DEFAULT` expressions the
            // `CREATE` needs, and `…_identity_arguments` omits them because
            // `DROP`/`ALTER … RENAME` have no grammar for them.
            "SELECT n.nspname, p.proname, pg_get_function_arguments(p.oid), \
                    pg_get_function_result(p.oid), l.lanname, p.prosrc, \
                    p.provolatile, p.proisstrict::int, p.prosecdef::int, \
                    p.oid::text, p.prokind, \
                    pg_get_function_identity_arguments(p.oid) \
             FROM pg_proc p \
             JOIN pg_namespace n ON n.oid = p.pronamespace \
             JOIN pg_language l ON l.oid = p.prolang \
             WHERE {filter} \
             ORDER BY n.nspname, p.proname"
        ),
    )
    .await?
    .iter()
    .map(|r| {
        let kind = RoutineKind::parse(&cell(r, 10));
        RoutineInfo {
            schema: Some(cell(r, 0)),
            name: cell(r, 1),
            kind,
            arguments: cell(r, 2),
            identity_arguments: cell(r, 11),
            // `pg_get_function_result` returns NULL for a procedure, which
            // `cell` renders as the empty string — which is exactly what the
            // model wants there, and what `RoutineDraft::validate` demands.
            returns: cell(r, 3),
            language: cell(r, 4),
            body: cell(r, 5),
            volatility: Volatility::parse_code(&cell(r, 6)),
            strict: cell(r, 7) == "1",
            security_definer: cell(r, 8) == "1",
            settings: settings.get(&cell(r, 9)).cloned().unwrap_or_default(),
            // MySQL's alone; PostgreSQL has no clause for any of them and the
            // emitter writes nothing for a `None`.
            ..Default::default()
        }
    })
    .collect())
}

/// Decode `pg_trigger.tgtype` into the three things it packs: when the trigger
/// fires, what it fires on, and how often.
///
/// The bits are PostgreSQL's own (`ROW = 1`, `BEFORE = 2`, `INSERT = 4`,
/// `DELETE = 8`, `UPDATE = 16`, `TRUNCATE = 32`, `INSTEAD = 64`) and there is no
/// catalogue view that spells them out, so this is the only place they're read.
/// Timing is a three-way choice off two bits: `BEFORE` set means before,
/// `INSTEAD` set means instead-of, and neither means after — checked in that
/// order because `INSTEAD` is never set together with `BEFORE`.
///
/// Events come out in PostgreSQL's own print order (INSERT, DELETE, UPDATE,
/// TRUNCATE) rather than declaration order, which nothing records. That matters
/// for the round-trip gate: a re-emitted `INSERT OR UPDATE` has to come back
/// byte-identical, and only a fixed order can promise that.
fn pg_trigger_type(tgtype: i32) -> (TriggerTiming, Vec<TriggerEvent>, TriggerLevel) {
    let level = if tgtype & 1 != 0 {
        TriggerLevel::Row
    } else {
        TriggerLevel::Statement
    };
    let timing = if tgtype & 2 != 0 {
        TriggerTiming::Before
    } else if tgtype & 64 != 0 {
        TriggerTiming::InsteadOf
    } else {
        TriggerTiming::After
    };
    let mut events = Vec::new();
    for (bit, ev) in [
        (4, TriggerEvent::Insert),
        (8, TriggerEvent::Delete),
        (16, TriggerEvent::Update),
        (32, TriggerEvent::Truncate),
    ] {
        if tgtype & bit != 0 {
            events.push(ev);
        }
    }
    (timing, events, level)
}

/// The `WHEN` guard of a trigger, read out of `pg_get_triggerdef`.
///
/// **Not `pg_get_expr(tgqual, tgrelid)`**, which is the obvious call and does
/// not work: a trigger's guard may reference both `OLD` and `NEW`, and
/// `pg_get_expr` renders expressions over a *single* relation — so it fails the
/// whole query with `expression contains variables of more than one relation`,
/// taking the entire schema fetch down with it. `pg_get_triggerdef` re-prints
/// the same guard correctly, and this module already fetches it for the
/// arguments.
///
/// Returned with the server's parens still on; [`schemaic_core::ddl::trigger_condition`]
/// is what reduces it to the bare predicate the model stores.
fn pg_trigger_when(def: &str) -> Option<String> {
    // Search only the part before the call, so a literal argument containing
    // the word can't be mistaken for the clause — and take the keyword itself at
    // a *code* position, or an argument holding `EXECUTE FUNCTION ` cuts the
    // head in the wrong place.
    let exec = ["EXECUTE FUNCTION ", "EXECUTE PROCEDURE "]
        .iter()
        .find_map(|kw| sql::find_code(def, kw, PG))
        .unwrap_or(def.len());
    let head = &def[..exec];
    // The **first** `WHEN` at a code position, not the last: a guard may carry
    // its own `CASE WHEN`, and anchoring at the end recorded that inner
    // condition as the whole clause.
    let at = sql::find_code(head, " WHEN ", PG)? + " WHEN ".len();
    let b = head.as_bytes();
    let open = at + b[at..].iter().position(|c| !c.is_ascii_whitespace())?;
    let end = sql::balanced_paren_span(b, open, PG)?;
    Some(head[open..=end].to_string())
}

/// The literal arguments a trigger passes to its function, read out of
/// `pg_get_triggerdef`.
///
/// They live in `pg_trigger.tgargs` as a NUL-separated `bytea`, which SQL has no
/// clean way to split and this driver surfaces as escaped text — so the server's
/// own rendering is the thing to read, exactly as it is for a check predicate.
/// Trigger arguments are always string literals, so the only escape that can
/// appear is a doubled quote.
///
/// Returns the values **unquoted**: the model holds raw strings and
/// [`schemaic_core::schema::TriggerInfo::create_sql`] re-quotes them, so a value
/// containing a quote survives a round trip instead of gaining a backslash per
/// edit.
fn pg_trigger_args(def: &str) -> Vec<String> {
    // `EXECUTE PROCEDURE` is the pre-11 spelling; servers still accept it and
    // older ones still print it. Located at a code position: `rfind` landed
    // *inside* an argument that contains the same text and read the list from
    // the wrong offset.
    let Some(tail) = ["EXECUTE FUNCTION ", "EXECUTE PROCEDURE "]
        .iter()
        .find_map(|kw| sql::find_code(def, kw, PG).map(|i| &def[i + kw.len()..]))
    else {
        return Vec::new();
    };
    let b = tail.as_bytes();
    // The `(` that opens the call — a quoted function name may carry one.
    let mut i = 0usize;
    let open = loop {
        if i >= b.len() {
            return Vec::new();
        }
        if let Some(j) = sql::skip_noncode(b, i, PG) {
            i = j;
            continue;
        }
        if b[i] == b'(' {
            break i;
        }
        i += 1;
    };
    // Each literal is one token to the shared lexer, so a `)` or a keyword
    // inside one is data and never structure.
    let mut out: Vec<String> = Vec::new();
    let mut i = open + 1;
    while i < b.len() {
        if b[i] == b'\'' {
            let end = sql::skip_noncode(b, i, PG).unwrap_or(b.len());
            // `end` is one past the closing quote — unless the literal was never
            // terminated, in which case the scan ran to the end of the input.
            let close = if b.get(end - 1) == Some(&b'\'') {
                end - 1
            } else {
                end
            };
            out.push(tail[i + 1..close].replace("''", "'"));
            i = end;
            continue;
        }
        if let Some(j) = sql::skip_noncode(b, i, PG) {
            i = j;
            continue;
        }
        if b[i] == b')' {
            break;
        }
        i += 1;
    }
    out
}

/// A view's options from its `pg_class` row: the relation kind (`v` a view, `m`
/// a materialized one) and its storage parameters, already flattened to
/// `a=1,b=2` by `array_to_string`.
///
/// `check_option` is lifted out because both engines spell that one the same way
/// in DDL (`WITH CASCADED CHECK OPTION`); everything else stays verbatim, since
/// it's restated inside the `WITH (…)` it came from. Pure + tested.
pub(crate) fn pg_view_options(
    relkind: &str,
    reloptions: &str,
    relispopulated: &str,
) -> ViewOptions {
    let mut out = ViewOptions {
        materialized: relkind == "m",
        // A bool column, so `cell` renders it `t` / `f`. Anything else — an
        // older server, a column that stopped being selected — reads as
        // populated, which is the answer that leaves the concurrent refresh on
        // offer rather than silently withdrawing it from every view.
        populated: !relispopulated.eq_ignore_ascii_case("f"),
        ..Default::default()
    };
    for opt in reloptions
        .split(',')
        .map(str::trim)
        .filter(|o| !o.is_empty())
    {
        match opt.split_once('=') {
            Some((k, v)) if k.trim().eq_ignore_ascii_case("check_option") => {
                out.check_option = Some(v.trim().to_ascii_uppercase());
            }
            _ => out.storage.push(opt.to_string()),
        }
    }
    out
}

/// How a column is assigned a value when the writer doesn't give one: the three
/// model fields PostgreSQL spreads across four catalogue signals.
pub(crate) struct Assignment {
    pub default: Option<String>,
    pub generated: Option<String>,
    pub auto_increment: bool,
    pub identity_always: bool,
}

/// Fold `pg_get_expr(adbin)` / `attidentity` / `attgenerated` / sequence
/// ownership into [`Assignment`].
///
/// Pure because the three ways a column gets a value are mutually exclusive in
/// the SQL that restates them, while the catalogue reports them in overlapping
/// fields — and naming two at once doesn't fail here, it fails later, in DDL the
/// user is about to run.
///
/// * A **generated** column's default expression *is* its expression.
/// * A **`serial`** is a sequence binding rendered as a `nextval` default. It is
///   auto-increment, and carrying the default as well emits both halves of a
///   pairing PostgreSQL rejects.
/// * An **identity** column has no default at all; `'a'` (`ALWAYS`) additionally
///   refuses an explicit value, where `'d'` and `serial` accept one.
///
/// `owns_sequence` — not the `nextval(` text — is what makes the second case a
/// `serial`. A hand-written `DEFAULT nextval('shared')` over a sequence the
/// column doesn't own is an ordinary default, and rewriting it as an identity
/// would quietly swap a shared counter for a private one.
pub(crate) fn pg_assignment(
    default: Option<String>,
    identity: &str,
    generated: &str,
    owns_sequence: bool,
) -> Assignment {
    let is_generated = !generated.is_empty();
    let is_identity = !identity.is_empty();
    // A sequence the column owns, bound through a default, is a `serial`.
    let is_serial = !is_identity
        && !is_generated
        && owns_sequence
        && default
            .as_deref()
            .is_some_and(|d| d.starts_with("nextval("));
    Assignment {
        generated: is_generated.then(|| default.clone().unwrap_or_default()),
        default: default.filter(|_| !is_generated && !is_identity && !is_serial),
        auto_increment: is_identity || is_serial,
        identity_always: identity == "a",
    }
}

/// One `pg_constraint.confdeltype`/`confupdtype` code as the SQL it stands for.
/// `a` is `NO ACTION` — the standard default, which both engines leave unwritten,
/// so it maps to `None` and an untouched key round-trips to no change.
fn fk_action(code: &str) -> Option<String> {
    match code {
        "r" => Some("RESTRICT".to_string()),
        "c" => Some("CASCADE".to_string()),
        "n" => Some("SET NULL".to_string()),
        "d" => Some("SET DEFAULT".to_string()),
        _ => None,
    }
}

/// Run a read-only SELECT and return every row as a `Vec<Option<String>>` (one
/// entry per column, `None` = SQL NULL) over the text protocol.
/// [`query_all`], but a server that has never heard of the catalogue being asked
/// about answers "nothing" instead of failing the whole schema load.
///
/// The same judgement `mysql_checks` makes, and the same two-sided rule: only
/// `undefined_table`/`undefined_column`/`undefined_function` degrade — the
/// SQLSTATEs that mean *this server predates the feature* — and every other error
/// still surfaces. A blanket `unwrap_or_default` would turn a typo, a permission
/// problem or a dropped connection into a database that silently appears to have
/// no types, which is indistinguishable from the truth and therefore worse than
/// an error.
///
/// It is worth the care here specifically: these queries run inside
/// [`fetch_schema`], so a failure takes **the whole database's** schema with it —
/// the exact shape of the `pg_get_expr` trigger bug.
async fn query_all_optional(
    client: &Client,
    sql: &str,
) -> Result<Vec<Vec<Option<String>>>, DbError> {
    use tokio_postgres::error::SqlState;
    match client.simple_query(sql).await {
        Ok(msgs) => Ok(msgs
            .into_iter()
            .filter_map(|m| match m {
                SimpleQueryMessage::Row(r) => {
                    let n = r.columns().len();
                    Some((0..n).map(|i| r.get(i).map(|s| s.to_string())).collect())
                }
                _ => None,
            })
            .collect()),
        Err(e)
            if matches!(
                e.code(),
                Some(&SqlState::UNDEFINED_TABLE)
                    | Some(&SqlState::UNDEFINED_COLUMN)
                    | Some(&SqlState::UNDEFINED_FUNCTION)
            ) =>
        {
            Ok(Vec::new())
        }
        Err(e) => Err(DbError::Query(e.to_string())),
    }
}

async fn query_all(client: &Client, sql: &str) -> Result<Vec<Vec<Option<String>>>, DbError> {
    let msgs = client
        .simple_query(sql)
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;
    let mut out = Vec::new();
    for m in msgs {
        if let SimpleQueryMessage::Row(r) = m {
            let n = r.columns().len();
            out.push((0..n).map(|i| r.get(i).map(|s| s.to_string())).collect());
        }
    }
    Ok(out)
}

/// Column `i` of a text row as an owned `String` (empty when NULL/missing).
/// The expression text for one index key position, as `pg_get_indexdef(oid, n,
/// true)` renders it, without the parentheses it wraps a compound expression in.
///
/// [`IndexInfo::key_sql`](schemaic_core::schema::IndexInfo::key_sql) puts exactly
/// one pair back, which is what PostgreSQL requires when the index is recreated —
/// so the stored form is the bare expression and the emitted form is
/// parenthesised, rather than each site guessing.
fn expr_key(def: &str) -> String {
    let s = def.trim();
    schemaic_core::ddl::unwrap_parens(s)
        .unwrap_or(s)
        .to_string()
}

fn cell(row: &[Option<String>], i: usize) -> String {
    row.get(i).and_then(|c| c.clone()).unwrap_or_default()
}

/// Column `i` as an owned `String`, keeping the NULL — the distinction
/// [`cell`] flattens away, and the one the statistics model is built on: a
/// missing figure and an empty one mean different things there.
fn opt(row: &[Option<String>], i: usize) -> Option<String> {
    row.get(i).and_then(|c| c.clone()).filter(|s| !s.is_empty())
}

/// Column `i` as a count. `None` for NULL, for anything unparseable, and for a
/// negative — the catalogue writes `-1` for "unknown", which must not survive as
/// a number.
fn num(row: &[Option<String>], i: usize) -> Option<u64> {
    opt(row, i)?.parse().ok()
}

/// Column `i` as a boolean. `simple_query` returns everything as text, so a
/// PostgreSQL `bool` arrives as `t` or `f`.
fn flag(row: &[Option<String>], i: usize) -> bool {
    opt(row, i).as_deref() == Some("t")
}

/// Map a PostgreSQL wire type to a human SQL type name. See [`pg_type_name_str`].
fn pg_type_name(t: &Type) -> String {
    pg_type_name_str(t.name())
}

/// Map a PostgreSQL *internal* type name (`pg_type.typname` / `information_schema`'s
/// `udt_name` — e.g. `varchar`, `timestamptz`, `int4`) to a short, human SQL type
/// name. This is the **single** type-name mapping: the grid feeds it the wire
/// `Type` (via [`pg_type_name`]) and the schema panel feeds it `udt_name`, so the
/// two always agree (rather than the grid showing `VARCHAR` while the schema panel
/// shows the verbose `information_schema.data_type` "character varying"). Integer/
/// float names are chosen so [`crate::parse_typed`] recognizes them (its `starts_with`
/// checks key off `INT`/`SMALLINT`/`BIGINT` and `FLOAT`/`DOUBLE`); `NUMERIC` stays a
/// string so it's never coerced to a lossy float. `timestamp`/`timestamptz` keep
/// Postgres's own short aliases (the no-tz form is plain `TIMESTAMP`, the tz form
/// `TIMESTAMPTZ`) instead of the standard "timestamp without/with time zone".
/// Unknown types pass through uppercased.
fn pg_type_name_str(name: &str) -> String {
    match name {
        "int2" => "SMALLINT",
        "int4" => "INTEGER",
        "int8" => "BIGINT",
        "float4" => "FLOAT",
        "float8" => "DOUBLE PRECISION",
        "numeric" => "NUMERIC",
        "bool" => "BOOLEAN",
        "varchar" => "VARCHAR",
        "bpchar" => "CHAR",
        "text" | "name" => "TEXT",
        "date" => "DATE",
        "time" => "TIME",
        "timetz" => "TIMETZ",
        "timestamp" => "TIMESTAMP",
        "timestamptz" => "TIMESTAMPTZ",
        "uuid" => "UUID",
        "json" => "JSON",
        "jsonb" => "JSONB",
        "bytea" => "BYTEA",
        other => return other.to_ascii_uppercase(),
    }
    .to_string()
}

// ── Provenance resolver ──────────────────────────────────────────────────────

/// Per-column metadata resolved from `pg_catalog` for a table-backed result
/// column: its real table/column name + key flags (drives editability + the
/// new-row placeholder previews).
struct ColMeta {
    /// The table's namespace (`pg_namespace.nspname`) — part of its identity, so
    /// the write path can name exactly the table the row was read from.
    schema: String,
    table: String,
    column: String,
    flags: ColumnFlags,
}

/// Resolve `(table_oid, attnum)` → real names + key flags via `pg_catalog`, in
/// one query over the set of referenced table OIDs. Booleans come back as 't'/'f'
/// over the text protocol. `auto_increment` covers both identity columns and
/// `serial` (a `nextval(...)` default); `no_default` is a NOT-NULL-ish column
/// with neither a default nor identity (→ `<required>` in the new-row preview).
async fn fetch_col_meta(
    client: &Client,
    ids: &[Option<(u32, i16)>],
) -> Result<HashMap<(u32, i16), ColMeta>, DbError> {
    let oids: HashSet<u32> = ids.iter().filter_map(|p| p.map(|(o, _)| o)).collect();
    if oids.is_empty() {
        return Ok(HashMap::new());
    }
    let in_list = oids
        .iter()
        .map(|o| o.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT a.attrelid, a.attnum, a.attname, c.relname, n.nspname, \
                a.attnotnull, \
                (a.attidentity <> '' OR (a.atthasdef AND \
                    COALESCE(pg_get_expr(ad.adbin, ad.adrelid), '') LIKE 'nextval(%')) AS auto_inc, \
                a.atthasdef, a.attidentity, \
                (pk.attnum IS NOT NULL) AS is_pk \
         FROM pg_attribute a \
         JOIN pg_class c ON c.oid = a.attrelid \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         LEFT JOIN pg_attrdef ad ON ad.adrelid = a.attrelid AND ad.adnum = a.attnum \
         LEFT JOIN (SELECT i.indrelid AS relid, k AS attnum \
                    FROM pg_index i, unnest(i.indkey) AS k WHERE i.indisprimary) pk \
           ON pk.relid = a.attrelid AND pk.attnum = a.attnum \
         WHERE a.attrelid IN ({in_list}) AND a.attnum > 0 AND NOT a.attisdropped"
    );
    let rows = query_all(client, &sql).await?;
    let mut out = HashMap::new();
    for r in rows {
        let oid: u32 = cell(&r, 0).parse().unwrap_or(0);
        let attnum: i16 = cell(&r, 1).parse().unwrap_or(0);
        if oid == 0 || attnum <= 0 {
            continue;
        }
        // Column order: attrelid, attnum, attname, relname, nspname, attnotnull,
        // auto_inc, atthasdef, attidentity, is_pk.
        let not_null = cell(&r, 5) == "t";
        let flags = ColumnFlags {
            primary_key: cell(&r, 9) == "t",
            unique_key: false, // not surfaced yet (key-icon nicety); PK covers editing
            not_null,
            auto_increment: cell(&r, 6) == "t",
            // Decided in Rust rather than in the `SELECT`, so the rule the flag's
            // contract states has a test standing on it.
            no_default: pg_no_default(not_null, cell(&r, 7) == "t", &cell(&r, 8)),
        };
        out.insert(
            (oid, attnum),
            ColMeta {
                schema: cell(&r, 4),
                table: cell(&r, 3),
                column: cell(&r, 2),
                flags,
            },
        );
    }
    Ok(out)
}

/// Must the user supply a value for this column, or will the server?
///
/// This is [`schemaic_core::model::ColumnFlags::no_default`]'s contract, and it
/// is **not** "has no `DEFAULT` clause": a nullable column has an implicit `NULL`
/// default, so the flag is only set for a NOT-NULL, non-identity column with no
/// `DEFAULT`. PostgreSQL computed it without the NOT-NULL term, so the grid's
/// new-row preview marked most nullable columns a red `<required>` — telling the
/// user a correct action would fail — while MySQL showed `<null>` for the same
/// column. Pure and tested rather than a term inside the `SELECT`, so the rule
/// the contract states is the rule a test can check.
fn pg_no_default(not_null: bool, has_default: bool, identity: &str) -> bool {
    not_null && !has_default && identity.is_empty()
}

// ── Write-back (commit + refetch) ────────────────────────────────────────────

/// Double-quote a Postgres identifier, doubling any embedded quote.
fn pg_ident(name: &str) -> String {
    // The one identifier-quoting rule, pinned to this module's only engine.
    schemaic_core::export::ident_sql(name, schemaic_core::intel::SqlDialect::Postgres)
}

/// Reaches [`pg_ident`] from the crate-level test that binds every quoter to
/// `core`'s.
#[cfg(test)]
pub(crate) fn pg_ident_for_test(name: &str) -> String {
    pg_ident(name)
}

/// A table name for the **write path**, qualified with its namespace whenever one
/// is known — including `public`.
///
/// Deliberately unlike the user-facing
/// [`sql_qualifier`](schemaic_core::schema::sql_qualifier), which drops `public`
/// to keep the editor's SQL clean: nothing here is ever shown, and an `UPDATE`
/// that resolves through `search_path` could hit a different table than the one
/// the row was read from. `None` (MySQL-shaped origins, or a Postgres result
/// whose namespace couldn't be resolved) falls back to the bare name.
fn pg_qname(schema: Option<&str>, table: &str) -> String {
    match schema {
        Some(s) => format!("{}.{}", pg_ident(s), pg_ident(table)),
        None => pg_ident(table),
    }
}

/// A SQL string literal (single-quoted, quotes doubled). Safe under Postgres's
/// default `standard_conforming_strings = on` (backslashes are literal). Such an
/// "unknown"-typed literal coerces to the target column type — the text-path
/// analog of a bound parameter.
fn pg_str_lit(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// A settable cell value as a literal — this module's `SET`/`VALUES` payload.
///
/// **`decode('…', 'hex')`, not `'\x…'::bytea`.** The `\x` form needs its
/// backslash to survive the *string* literal and reach `bytea`'s input function
/// as that type's hex marker — true only while `standard_conforming_strings` is
/// on. That is the same dependency [`roles`] refuses to take, and it is settable
/// per database and per role.
///
/// With it off the string literal claims the backslash first, as its own hex
/// escape: `'\x41424344'` becomes the seven characters `A424344` (`\x41` → `A`,
/// the rest left alone), `bytea` reads *those* in its escape format, and the
/// column takes seven bytes of ASCII where four bytes of value belonged — no
/// error, no warning the app would see. Measured, not reasoned: with
/// `standard_conforming_strings = off`, `encode('\x41424344'::bytea, 'hex')`
/// answers `41343234333434`. It is not even reliably silent — a payload whose
/// escaped form is not valid UTF-8 fails the *statement* instead, which is how
/// most real blobs would present.
///
/// `decode` has no escaping to depend on: its argument is hex digits and nothing
/// else, so there is nothing in it to escape and nothing in it to quote. The
/// live tier's `staged_bytes_reach_the_column_as_bytes` proves the bytes arrive
/// but **does not** pin this choice — a server with the default setting takes
/// both forms, which is precisely why the argument is written down here.
fn pg_cell_lit(v: &CellEdit) -> String {
    match v {
        CellEdit::Text(s) => pg_str_lit(s),
        CellEdit::Bytes(b) => {
            use std::fmt::Write as _;
            let mut hex = String::with_capacity(b.len() * 2);
            for byte in b.iter() {
                let _ = write!(hex, "{byte:02x}");
            }
            format!("decode('{hex}', 'hex')")
        }
        CellEdit::Null => "NULL".to_string(),
    }
}

/// A typed key `Value` as a SQL literal for a WHERE comparison. Numbers are
/// emitted bare (safe); text via `pg_str_lit`. (Float/binary keys are excluded
/// upstream by `analyze_edit`.)
fn pg_value_lit(v: &Value) -> String {
    match v {
        Value::Null => "NULL".to_string(),
        Value::Int(i) => i.to_string(),
        Value::UInt(u) => u.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Str(s) => pg_str_lit(s),
    }
}

/// NULL-safe WHERE from key columns: `"c" IS NOT DISTINCT FROM <lit> AND …`
/// (Postgres's equivalent of MySQL's `<=>`).
fn where_key(key: &[(String, Value)]) -> String {
    key.iter()
        .map(|(c, v)| format!("{} IS NOT DISTINCT FROM {}", pg_ident(c), pg_value_lit(v)))
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// `UPDATE "t" SET … WHERE <key>` — unqualified table (resolved via search_path;
/// the connection is already scoped to the tab's database, so `edit.database` is
/// not used as a qualifier).
fn build_update(edit: &RowEdit) -> String {
    let set_sql = edit
        .set
        .iter()
        .map(|(c, v)| format!("{} = {}", pg_ident(c), pg_cell_lit(v)))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "UPDATE {} SET {set_sql} WHERE {}",
        pg_qname(edit.schema.as_deref(), &edit.table),
        where_key(&edit.key)
    )
}

/// `INSERT INTO "t" (cols) VALUES (…)`, or `INSERT INTO "t" DEFAULT VALUES` when
/// no columns are set (Postgres's all-defaults form — MySQL's `() VALUES ()` is
/// invalid here).
fn build_insert(ins: &RowInsert) -> String {
    let name = pg_qname(ins.schema.as_deref(), &ins.table);
    if ins.cols.is_empty() {
        return format!("INSERT INTO {name} DEFAULT VALUES");
    }
    let cols = ins
        .cols
        .iter()
        .map(|(c, _)| pg_ident(c))
        .collect::<Vec<_>>()
        .join(", ");
    let vals = ins
        .cols
        .iter()
        .map(|(_, v)| pg_cell_lit(v))
        .collect::<Vec<_>>()
        .join(", ");
    format!("INSERT INTO {name} ({cols}) VALUES ({vals})")
}

/// `DELETE FROM "t" WHERE <key>`.
fn build_delete(del: &RowDelete) -> String {
    format!(
        "DELETE FROM {} WHERE {}",
        pg_qname(del.schema.as_deref(), &del.table),
        where_key(&del.key)
    )
}

/// The PostgreSQL half of [`crate::Db::import_rows`] — same contract: one
/// transaction, batched multi-row `INSERT`s, each batch's affected count checked
/// against its size, everything undone on any failure.
pub(crate) async fn import_rows(
    db: &Db,
    target: crate::ImportTarget<'_>,
    rows: crate::RowSource<'_>,
    cancel: CancellationToken,
) -> Result<u64, DbError> {
    let client = connect_to(db, target.database).await?;
    let token = client.cancel_token();
    tokio::select! {
        r = import_on(&client, &target, rows) => r,
        _ = cancel.cancelled() => {
            cancel_query(db, &token).await;
            // Explicit, like every other exit in `import_on` — the drop would
            // abort the transaction too, but it leaves it open until the
            // connection actually goes away, holding its locks meanwhile.
            //
            // No `Rollback::note()` here and none is owed: PostgreSQL has no
            // non-transactional table, so `Cancelled` really does mean nothing
            // was written. Its MySQL counterpart cannot say that (see
            // `Db::import_rows`), and the divergence is the engines', not a
            // difference in care between the two paths.
            let _ = client.batch_execute("ROLLBACK").await;
            Err(DbError::Cancelled)
        }
    }
}

async fn import_on(
    client: &Client,
    target: &crate::ImportTarget<'_>,
    rows: crate::RowSource<'_>,
) -> Result<u64, DbError> {
    // `db_err`, not `to_string()`: the driver's own Display for a server error is
    // the literal text "db error" — a duplicate-key import has to say so.
    let cols: Vec<&str> = target.columns.iter().map(String::as_str).collect();
    client
        .batch_execute("BEGIN")
        .await
        .map_err(|e| db_err(&e))?;

    let mut total: u64 = 0;
    loop {
        // Postgres leaves the transaction aborted after any error, so every exit
        // path below rolls back explicitly rather than relying on the drop.
        let batch = match crate::next_batch_off_executor(rows) {
            Ok(Some(b)) => b,
            Ok(None) => break,
            Err(e) => {
                let _ = client.batch_execute("ROLLBACK").await;
                return Err(e);
            }
        };
        let Some(sql) = schemaic_core::import::build_insert(
            target.database,
            target.schema,
            target.table,
            &cols,
            &batch,
            schemaic_core::intel::SqlDialect::Postgres,
        ) else {
            continue;
        };
        let affected = match client.execute(sql.as_str(), &[]).await {
            Ok(n) => n,
            Err(e) => {
                let _ = client.batch_execute("ROLLBACK").await;
                return Err(db_err(&e));
            }
        };
        if affected != batch.len() as u64 {
            let _ = client.batch_execute("ROLLBACK").await;
            // Always `Complete` here — every PostgreSQL table is transactional.
            return Err(DbError::Query(format!(
                "a batch of {} rows inserted {affected}{}",
                batch.len(),
                Rollback::Complete.note()
            )));
        }
        total += affected;
    }

    client
        .batch_execute("COMMIT")
        .await
        .map_err(|e| db_err(&e))?;
    Ok(total)
}

/// Apply a batch of staged grid mutations in a single transaction — deletes →
/// updates → inserts, each required to affect exactly one row (else roll back
/// all). Mirrors the MySQL `commit_writes` contract + 1-row safety net; the
/// connection is scoped to the base table's database (`origin.database`).
pub(crate) async fn commit_writes(
    db: &Db,
    write: &GridWrite,
    cancel: CancellationToken,
) -> Result<u64, DbError> {
    if write.is_empty() {
        return Ok(0);
    }
    // Every item in one GridWrite shares the base table's database (single result
    // → single tab → single DB); connect to it.
    let database = write
        .updates
        .first()
        .map(|e| e.database.as_str())
        .or_else(|| write.inserts.first().map(|i| i.database.as_str()))
        .or_else(|| write.deletes.first().map(|d| d.database.as_str()))
        .unwrap_or("");
    if database.is_empty() {
        return Ok(0);
    }
    let client = connect_to(db, database).await?;
    let token = client.cancel_token();

    tokio::select! {
        r = write_on(&client, write, TxScope::Own) => r,
        _ = cancel.cancelled() => {
            cancel_query(db, &token).await;
            Err(DbError::Cancelled)
        }
    }
}

/// Apply a staged batch of grid mutations on an already-open client: deletes →
/// updates → inserts, each required to affect exactly one row, the whole batch
/// undone if any doesn't. `scope` decides whether that atomicity is a
/// transaction of its own or a savepoint nested in the caller's transaction.
///
/// Control statements go through `batch_execute` — `BEGIN`/`SAVEPOINT` aren't
/// preparable, so `execute` can't run them.
pub(crate) async fn write_on(
    client: &Client,
    write: &GridWrite,
    scope: TxScope,
) -> Result<u64, DbError> {
    let qerr = |e: tokio_postgres::Error| db_err(&e);
    client
        .batch_execute(scope.begin_sql())
        .await
        .map_err(qerr)?;

    // One statement + its 1-row check; on a miss the batch is undone. Postgres
    // needs the undo even for a plain error — the transaction is aborted until
    // something rolls it back, and with a savepoint that something is us.
    async fn one(
        client: &Client,
        scope: TxScope,
        sql: String,
        step: WriteStep<'_>,
    ) -> Result<u64, DbError> {
        let n = match client.execute(sql.as_str(), &[]).await {
            Ok(n) => n,
            Err(e) => {
                let _ = client.batch_execute(scope.rollback_sql()).await;
                return Err(db_err(&e));
            }
        };
        if let Err(msg) = one_row_verdict(step, n) {
            let _ = client.batch_execute(scope.rollback_sql()).await;
            // Every PostgreSQL table is transactional, so the undo is real and
            // the note is always `Complete` — unlike MySQL, where the engine
            // decides. The clause still comes from the shared `Rollback` so the
            // two executors keep one wording.
            return Err(DbError::Query(format!(
                "{msg}{}",
                Rollback::Complete.note()
            )));
        }
        Ok(n)
    }

    let mut total: u64 = 0;
    // Deletes → updates → inserts, ordered by the same `GridWrite::plan` the
    // MySQL executor runs.
    for step in write.plan() {
        let sql = match step {
            WriteStep::Delete(del) => build_delete(del),
            WriteStep::Update(edit) => build_update(edit),
            WriteStep::Insert(ins) => build_insert(ins),
        };
        total += one(client, scope, sql, step).await?;
    }

    if let Err(e) = client.batch_execute(scope.commit_sql()).await {
        let _ = client.batch_execute(scope.rollback_sql()).await;
        return Err(qerr(e));
    }
    Ok(total)
}

/// [`Db::fetch_blob`]'s PostgreSQL body — read one `bytea` cell.
pub(crate) async fn fetch_blob(
    db: &Db,
    r: &BlobRef,
    cancel: CancellationToken,
) -> Result<Option<BlobValue>, DbError> {
    let client = connect_to(db, &r.database).await?;
    let token = client.cancel_token();
    tokio::select! {
        res = blob_on(&client, r) => res,
        _ = cancel.cancelled() => {
            cancel_query(db, &token).await;
            Err(DbError::Cancelled)
        }
    }
}

/// [`fetch_blob`]'s body on an already-open client, so a manual-transaction
/// tab's pinned connection can read bytes it has written but not committed.
///
/// **The bytes arrive as text and are decoded here**, because this path speaks
/// the simple-query protocol like every other read in this module: a `bytea`
/// comes over as `\x89504e47…`, which is what [`bytea_hex_bytes`] turns back
/// into the value. That is also why the length is taken from `octet_length` and
/// not from the hex string — they agree for a whole value and deliberately do
/// not for a capped one.
pub(crate) async fn blob_on(client: &Client, r: &BlobRef) -> Result<Option<BlobValue>, DbError> {
    let col = pg_ident(&r.column);
    let sql = format!(
        "SELECT octet_length({col}), substring({col} from 1 for {FETCH_CAP}) FROM {} WHERE {} LIMIT 1",
        pg_qname(r.schema.as_deref(), &r.table),
        where_key(&r.key),
    );
    let msgs = client
        .simple_query(&sql)
        .await
        .map_err(|e| DbError::Query(e.to_string()))?;
    for m in msgs {
        let SimpleQueryMessage::Row(row) = m else {
            continue;
        };
        // A NULL cell makes `octet_length` NULL too, which `get` reports as
        // `None` — the same "no bytes to show" as a row that is gone.
        let Some(len) = row.get(0).and_then(|s| s.parse::<u64>().ok()) else {
            return Ok(None);
        };
        // **A value this cannot decode is an error, not an empty one.** The
        // decoder refuses anything that is not the `\x…` hex form — a server
        // with `bytea_output = escape` sends the other one — and substituting
        // an empty buffer here would pair *no bytes* with the real
        // `octet_length`: the panel would report 35.5 KB over an empty dump and
        // call the value truncated, which is the "looks like data" failure
        // `bytea_hex_bytes`' own doc refuses to commit. Naming the setting is
        // the only part of this the user can act on.
        let bytes = match row.get(1) {
            // A zero-length `bytea` is `\x`, which decodes to an empty vec —
            // distinct from a column that decoded to nothing.
            Some(text) => bytea_hex_bytes(text).ok_or_else(|| {
                DbError::Query(
                    "this server sends bytea in a format Schemaic cannot decode \
                     (expected the hex form `\\x…`); set `bytea_output = hex` on \
                     the server or the session to read binary values"
                        .to_string(),
                )
            })?,
            // The length was not NULL but the value was: impossible from one
            // row, and not something to invent bytes for.
            None => return Ok(None),
        };
        return Ok(Some(BlobValue { bytes, len }));
    }
    Ok(None)
}

/// Decode the text protocol's `\x…` `bytea` form into the bytes it stands for.
///
/// The byte-counting half of this is [`bytea_hex_len`], and both are deliberate
/// about rejecting rather than salvaging: a `bytea` whose text is not the hex
/// form (a server with `bytea_output = escape`) must not be *half* decoded into
/// something that looks like data, for the same reason `binary_display`
/// replaced MySQL's `from_utf8_lossy`.
fn bytea_hex_bytes(text: &str) -> Option<Vec<u8>> {
    let hex = text.strip_prefix("\\x")?;
    if hex.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(hex.len() / 2);
    for pair in hex.as_bytes().chunks(2) {
        let hi = (pair[0] as char).to_digit(16)?;
        let lo = (pair[1] as char).to_digit(16)?;
        out.push((hi * 16 + lo) as u8);
    }
    Some(out)
}

/// Re-`SELECT` the given just-edited rows by their (post-edit) key, so the grid
/// can splice DB truth back in without re-running the whole query. One
/// `SELECT … LIMIT 1` per row; rows that no longer match are skipped. Read-only.
pub(crate) async fn refetch_rows(
    db: &Db,
    template: &RefetchTemplate,
    rows: &[RefetchRow],
    cancel: CancellationToken,
) -> Result<Vec<(usize, Vec<Value>)>, DbError> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let client = connect_to(db, &template.database).await?;
    let token = client.cancel_token();

    tokio::select! {
        r = refetch_on(&client, template, rows) => r,
        _ = cancel.cancelled() => {
            cancel_query(db, &token).await;
            Err(DbError::Cancelled)
        }
    }
}

/// Re-`SELECT` just-edited rows on an already-open client. Read-only, so it is
/// safe on a fresh connection and *necessary* inside an open transaction — only
/// that connection can see the rows it just wrote but hasn't committed.
pub(crate) async fn refetch_on(
    client: &Client,
    template: &RefetchTemplate,
    rows: &[RefetchRow],
) -> Result<Vec<(usize, Vec<Value>)>, DbError> {
    let proj = template
        .columns
        .iter()
        .map(|c| pg_ident(c))
        .collect::<Vec<_>>()
        .join(", ");
    // Column types once (via a LIMIT 0 prepare) so every row's text cells parse
    // to the same typed `Value` the grid already holds.
    let qname = pg_qname(template.schema.as_deref(), &template.table);
    let type_names: Vec<String> = {
        let probe = format!("SELECT {proj} FROM {qname} LIMIT 0");
        match client.prepare(&probe).await {
            Ok(stmt) => stmt
                .columns()
                .iter()
                .map(|c| pg_type_name(c.type_()))
                .collect(),
            Err(_) => vec![String::new(); template.columns.len()],
        }
    };
    let kinds = cell_kinds(&type_names);

    {
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let where_sql = template
                .key_cols
                .iter()
                .enumerate()
                .map(|(i, &kci)| {
                    format!(
                        "{} IS NOT DISTINCT FROM {}",
                        pg_ident(&template.columns[kci]),
                        pg_value_lit(&row.key[i])
                    )
                })
                .collect::<Vec<_>>()
                .join(" AND ");
            let sql = format!("SELECT {proj} FROM {qname} WHERE {where_sql} LIMIT 1");
            let msgs = client
                .simple_query(&sql)
                .await
                .map_err(|e| DbError::Query(e.to_string()))?;
            for m in msgs {
                if let SimpleQueryMessage::Row(r) = m {
                    let cells: Vec<Value> = kinds
                        .iter()
                        .enumerate()
                        .map(|(i, &kind)| match r.get(i) {
                            None => Value::Null,
                            Some(s) => pg_cell(s, kind),
                        })
                        .collect();
                    out.push((row.data_row, cells));
                    break;
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic chain, because a `tokio_postgres::Error` cannot be built by
    /// hand — and the property under test is the walk, not the driver.
    #[derive(Debug)]
    struct Layer(&'static str, Option<Box<Layer>>);

    impl std::fmt::Display for Layer {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(self.0)
        }
    }

    impl std::error::Error for Layer {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            self.1.as_deref().map(|l| l as &dyn std::error::Error)
        }
    }

    /// The decision this pins lives in the SQL string, so the string is the
    /// subject.
    ///
    /// A database the role cannot connect to is not a database the tree can
    /// show: expanding it can only produce an error, under a row the user did
    /// not create and cannot remove. Both terms are needed and they are not the
    /// same question — `datallowconn` is "nobody may connect", the privilege
    /// check is "this role may not" — and asking PostgreSQL beats a list of
    /// provider-specific names to skip, which would need extending for every
    /// managed service that keeps bookkeeping beside the user's data.
    #[test]
    fn the_listing_hides_databases_that_cannot_be_opened() {
        assert!(DATABASE_LISTING.contains("datallowconn"));
        assert!(DATABASE_LISTING.contains("has_database_privilege(current_user, oid, 'CONNECT')"));
        // Still plumbing, still hidden.
        assert!(DATABASE_LISTING.contains("datistemplate = false"));
        assert!(DATABASE_LISTING.contains("datname <> 'postgres'"));
        // By `oid`, not by name: `has_database_privilege` takes either, and a
        // name would have to be quoted to survive one containing a capital or a
        // space.
        assert!(!DATABASE_LISTING.contains("privilege(current_user, datname"));
    }

    /// The probe exists to find a database, so it may only repeat itself for a
    /// failure that is *about* the database. `pg_hba` refusal is the one that
    /// looks like it should not qualify and must: it reads as an authentication
    /// error and is matched per database, which is precisely what a hosted
    /// provider answers for one it does not publish.
    #[test]
    fn only_a_database_level_failure_is_worth_another_candidate() {
        assert!(another_database_might_work(Some(
            &SqlState::INVALID_CATALOG_NAME
        )));
        assert!(another_database_might_work(Some(
            &SqlState::INVALID_AUTHORIZATION_SPECIFICATION
        )));
        // **The three this used to classify as server-level.** Each refuses one
        // database and says nothing about the server, so failing the whole
        // connection on them locked the user out where the pre-`82a83cd` loop
        // recovered. `template0` ships with `datallowconn = false`; `53300` is
        // transient, so a momentarily-full `postgres` made the server
        // unbrowsable; `42501` is how a hosted provider fences its catalogues.
        assert!(another_database_might_work(Some(
            &SqlState::OBJECT_NOT_IN_PREREQUISITE_STATE
        )));
        assert!(another_database_might_work(Some(
            &SqlState::TOO_MANY_CONNECTIONS
        )));
        assert!(another_database_might_work(Some(
            &SqlState::INSUFFICIENT_PRIVILEGE
        )));

        // The same wrong password for every database, and three more failed
        // logins against whatever is counting them.
        assert!(!another_database_might_work(Some(
            &SqlState::INVALID_PASSWORD
        )));
        // No SQLSTATE at all: the server never answered. A TLS rejection, a
        // refused socket, a wrong host — repeating it four times is how a
        // misconfigured certificate came to report "timed out".
        assert!(!another_database_might_work(None));
    }

    /// The configured database goes first, because on a provider that permits
    /// only its own it is the sole one that can work — and it is not the only
    /// candidate, because one that has since been dropped must degrade to the
    /// old guesses rather than lock the user out of the server entirely.
    #[test]
    fn a_configured_database_is_tried_before_the_guesses() {
        assert_eq!(
            maintenance_candidates(Some("defaultdb"), "avnadmin", None),
            ["defaultdb", "postgres", "avnadmin", "template1"]
        );
    }

    /// Without one, exactly the order that shipped before the field existed.
    #[test]
    fn no_configured_database_leaves_the_old_order_alone() {
        assert_eq!(
            maintenance_candidates(None, "app", None),
            ["postgres", "app", "template1"]
        );
    }

    /// A wasted round trip against a server that is already refusing us is a
    /// real cost: the probe is bounded as a whole, so a duplicate can be the
    /// attempt that pushes the last real candidate past the deadline.
    #[test]
    fn a_candidate_is_never_tried_twice() {
        assert_eq!(
            maintenance_candidates(Some("postgres"), "postgres", None),
            ["postgres", "template1"]
        );
    }

    /// An empty username is not a database to try. It reached the loop as `""`
    /// before, which spends a round trip to be told so.
    #[test]
    fn an_empty_username_is_not_a_candidate() {
        assert_eq!(
            maintenance_candidates(None, "", None),
            ["postgres", "template1"]
        );
    }

    /// **`DROP DATABASE` cannot run on a connection to its own target**, and the
    /// configured database is tried *first* — so dropping the database a
    /// connection names would pick, before any other candidate, the one
    /// connection that cannot perform the statement. The exclusion covers every
    /// slot rather than the first: `app` reaches this list as the username
    /// guess too, so filtering only the configured entry would still land on it
    /// for a user whose role and database share a name, which is the default
    /// PostgreSQL setup.
    #[test]
    fn the_database_being_dropped_is_never_the_one_it_runs_on() {
        assert_eq!(
            maintenance_candidates(Some("shop"), "app", Some("shop")),
            ["postgres", "app", "template1"]
        );
        assert_eq!(
            maintenance_candidates(Some("app"), "app", Some("app")),
            ["postgres", "template1"]
        );
        // And nothing is filtered when the plan is a create, which has no target
        // to avoid — the same list the old signature produced.
        assert_eq!(
            maintenance_candidates(Some("shop"), "app", None),
            ["shop", "postgres", "app", "template1"]
        );
    }

    /// The reason a TLS handshake failed lives only in `source()`, so a message
    /// built from the head alone says "error performing TLS handshake" and
    /// nothing else — for every one of the failures this feature exists to
    /// produce.
    #[test]
    fn a_connect_error_carries_what_caused_it() {
        let e = Layer(
            "error performing TLS handshake",
            Some(Box::new(Layer(
                "invalid peer certificate: UnknownIssuer",
                None,
            ))),
        );
        assert_eq!(
            error_chain(&e),
            "error performing TLS handshake: invalid peer certificate: UnknownIssuer"
        );
    }

    /// A wrapper that restates its cause would otherwise read as two problems.
    #[test]
    fn a_cause_already_spelled_out_is_not_repeated() {
        let e = Layer(
            "connection closed: broken pipe",
            Some(Box::new(Layer("broken pipe", None))),
        );
        assert_eq!(error_chain(&e), "connection closed: broken pipe");
    }

    #[test]
    fn an_error_with_no_cause_is_left_alone() {
        assert_eq!(error_chain(&Layer("plain", None)), "plain");
    }

    /// The decision this pins lives in the SQL string, so the string is the
    /// subject — a `from_pg_rows` case is a level too low to see it.
    ///
    /// `backend_type` is masked to NULL for every row an ordinary role has no
    /// `HAS_PGSTAT_PERMISSIONS` over, **including PostgreSQL's own auxiliary
    /// processes**, so a bare `backend_type IS NULL` admits the checkpointer,
    /// the background writer, the walwriter and both launchers as killable
    /// sessions. Live on PostgreSQL 16.15 as a plain role, the bare form
    /// returned five rows and all five were auxiliary processes.
    ///
    /// It takes **both** surviving columns: the logical replication launcher
    /// runs as `postgres`, so `usename IS NOT NULL` alone still admits it, and
    /// `datname` is the one every auxiliary process leaves NULL.
    #[test]
    fn the_activity_query_admits_masked_backends_without_admitting_aux_processes() {
        assert!(
            PG_ACTIVITY_SQL
                .contains("backend_type IS NULL AND usename IS NOT NULL AND datname IS NOT NULL"),
            "the masked-row branch must require both unmasked identity columns"
        );
        assert!(
            !PG_ACTIVITY_SQL.contains("OR backend_type IS NULL)"),
            "a bare `backend_type IS NULL` is the aux-process hole"
        );
    }

    /// Both sort terms have to survive the mask, because the `LIMIT` is applied
    /// on the server and `activity::prepare` only ever re-sorts what survived
    /// it. A masked row reads `state` NULL — so `IS DISTINCT FROM 'idle'` is
    /// true for all of them — and `age` NULL, which `DESC` sorts **first** by
    /// default. Live under a plain role the `LIMIT` kept the unreadable rows and
    /// cut that role's own running statements.
    #[test]
    fn the_activity_sort_puts_unknown_ages_and_unreadable_states_last() {
        assert!(
            PG_ACTIVITY_SQL.contains("s.age DESC NULLS LAST"),
            "PostgreSQL's default for DESC is NULLS FIRST"
        );
        assert!(
            PG_ACTIVITY_SQL.contains("s.state IS DISTINCT FROM 'idle' AND s.state IS NOT NULL"),
            "a NULL state is not evidence of work"
        );
    }

    /// The projection is the one thing that has to agree with
    /// `activity::PgActivityRow`'s field order, and it is now generated from
    /// the same list — so this pins the *whole* statement the server sees,
    /// including that the inner subquery still supplies every projected name.
    /// Run verbatim against PostgreSQL 16.15.
    #[test]
    fn the_activity_query_projects_the_columns_the_fold_reads() {
        assert_eq!(
            pg_activity_sql(51),
            "SELECT s.pid, s.usename, s.client_host, s.datname, s.state, s.query, s.age, \
             s.blockers FROM ( SELECT pid, usename, host(client_addr) AS client_host, datname, \
             state, query, EXTRACT(EPOCH FROM (now() - COALESCE(state_change, query_start, \
             backend_start)))::float8 AS age, pg_blocking_pids(pid)::text AS blockers FROM \
             pg_stat_activity WHERE (backend_type = 'client backend' OR (backend_type IS NULL \
             AND usename IS NOT NULL AND datname IS NOT NULL)) AND pid <> pg_backend_pid() ) s \
             ORDER BY (s.blockers <> '{}') DESC, (s.state IS DISTINCT FROM 'idle' AND s.state \
             IS NOT NULL) DESC, s.age DESC NULLS LAST LIMIT 51"
        );
    }

    /// The four catalogue signals fold into three model fields, and only one
    /// combination per row is legal — so each is pinned rather than trusted.
    #[test]
    fn a_plain_default_stays_a_default() {
        let a = pg_assignment(Some("0".into()), "", "", false);
        assert_eq!(a.default.as_deref(), Some("0"));
        assert_eq!(a.generated, None);
        assert!(!a.auto_increment);
        assert!(!a.identity_always);
    }

    /// A `serial` is one fact reported twice: the catalogue renders its sequence
    /// binding as a `nextval` default *and* the sequence is owned by the column.
    /// Carrying both wrote `DEFAULT nextval(…) GENERATED BY DEFAULT AS IDENTITY`
    /// into a generated `CREATE TABLE`, which PostgreSQL rejects outright.
    #[test]
    fn a_serial_is_auto_increment_and_not_also_a_default() {
        let a = pg_assignment(Some("nextval('t_id_seq'::regclass)".into()), "", "", true);
        assert_eq!(a.default, None);
        assert!(a.auto_increment);
        // `serial` accepts an explicit value — only `GENERATED ALWAYS` refuses.
        assert!(!a.identity_always);
    }

    /// The case that makes ownership the test rather than the `nextval` text: a
    /// hand-written `DEFAULT nextval('shared')` over a sequence the column does
    /// *not* own is an ordinary default. Reading it as auto-increment would drop
    /// the shared sequence and silently give the column a private one.
    #[test]
    fn a_nextval_over_an_unowned_sequence_is_an_ordinary_default() {
        let a = pg_assignment(
            Some("nextval('shared_seq'::regclass)".into()),
            "",
            "",
            false,
        );
        assert_eq!(
            a.default.as_deref(),
            Some("nextval('shared_seq'::regclass)")
        );
        assert!(!a.auto_increment);
    }

    /// `GENERATED ALWAYS AS IDENTITY` is the one form that *rejects* an explicit
    /// value, which is what `is_server_assigned` turns on.
    #[test]
    fn the_two_identity_forms_are_told_apart() {
        let always = pg_assignment(None, "a", "", true);
        assert!(always.auto_increment && always.identity_always);
        let by_default = pg_assignment(None, "d", "", true);
        assert!(by_default.auto_increment && !by_default.identity_always);
    }

    /// A generated column's `pg_get_expr` *is* its expression, not a default —
    /// emitting both is a syntax error.
    #[test]
    fn a_generated_expression_is_not_a_default() {
        let a = pg_assignment(Some("(qty * price)".into()), "", "s", false);
        assert_eq!(a.generated.as_deref(), Some("(qty * price)"));
        assert_eq!(a.default, None);
        assert!(!a.auto_increment);
    }

    /// `check_option` is lifted out (it's a DDL clause on both engines); the
    /// rest is restated verbatim inside the `WITH (…)` it came from.
    /// The bits are PostgreSQL's own and nothing else reads them, so every
    /// combination that changes behaviour is pinned here.
    #[test]
    fn pg_trigger_type_decodes_timing_events_and_level() {
        // ROW|BEFORE|INSERT = 1|2|4
        let (timing, events, level) = pg_trigger_type(7);
        assert_eq!(timing, TriggerTiming::Before);
        assert_eq!(events, vec![TriggerEvent::Insert]);
        assert_eq!(level, TriggerLevel::Row);

        // ROW|INSERT|UPDATE with neither BEFORE nor INSTEAD ⇒ AFTER.
        let (timing, events, _) = pg_trigger_type(1 | 4 | 16);
        assert_eq!(timing, TriggerTiming::After);
        assert_eq!(events, vec![TriggerEvent::Insert, TriggerEvent::Update]);

        // INSTEAD OF on a view: ROW|INSTEAD|DELETE.
        let (timing, events, _) = pg_trigger_type(1 | 64 | 8);
        assert_eq!(timing, TriggerTiming::InsteadOf);
        assert_eq!(events, vec![TriggerEvent::Delete]);

        // TRUNCATE is statement-level: no ROW bit.
        let (_, events, level) = pg_trigger_type(32);
        assert_eq!(events, vec![TriggerEvent::Truncate]);
        assert_eq!(level, TriggerLevel::Statement);
    }

    /// Events must come out in a fixed order or the round-trip gate can't hold:
    /// nothing records the order they were declared in.
    #[test]
    fn pg_trigger_type_orders_events_the_way_postgresql_prints_them() {
        let (_, events, _) = pg_trigger_type(1 | 4 | 8 | 16 | 32);
        assert_eq!(
            events,
            vec![
                TriggerEvent::Insert,
                TriggerEvent::Delete,
                TriggerEvent::Update,
                TriggerEvent::Truncate
            ]
        );
    }

    /// The two canonical orderings must be **the same** ordering.
    ///
    /// This module builds its vec in `tgtype` bit order; `TriggerEvent`'s derived
    /// `Ord` follows its declaration order in `schemaic-core`. When those
    /// disagreed, `ui::trigger_editor`'s `events.sort()` renormalised an
    /// introspected `[Delete, Update]` to `[Update, Delete]`, `diff_triggers`'
    /// element-wise compare saw a change, and one tick of any checkbox — on and
    /// straight back off — made the designer offer to drop and recreate a
    /// trigger nothing had touched.
    #[test]
    fn the_introspected_event_order_is_already_sorted() {
        // `tgtype = 25` is `AFTER DELETE OR UPDATE FOR EACH ROW`, confirmed live.
        for tgtype in [25, 1 | 4 | 8 | 16 | 32, 1 | 4 | 16, 1 | 8, 32] {
            let (_, events, _) = pg_trigger_type(tgtype);
            let mut sorted = events.clone();
            sorted.sort();
            assert_eq!(events, sorted, "tgtype = {tgtype}");
        }
    }

    /// Verbatim `pg_get_triggerdef` output from PostgreSQL 16.14 — the guard has
    /// the server's own double parens and a cast, and the call carries a literal
    /// with an escaped quote.
    const REAL_DEF: &str = "CREATE TRIGGER t_upd_cols BEFORE UPDATE OF name, total \
         ON public.trig_demo FOR EACH ROW WHEN ((new.total > (0)::numeric)) \
         EXECUTE FUNCTION schemaic_audit('audit', 'it''s')";

    /// Regression: this used to be `pg_get_expr(tgqual, tgrelid)`, which fails
    /// with "expression contains variables of more than one relation" the moment
    /// a guard mentions both OLD and NEW — and took the whole schema fetch with
    /// it, so every database showed "query failed".
    #[test]
    fn pg_trigger_when_reads_the_guard_out_of_the_definition() {
        assert_eq!(
            pg_trigger_when(REAL_DEF).as_deref(),
            Some("((new.total > (0)::numeric))")
        );
        // Normalizing is `ddl::trigger_condition`'s job, and it peels to bare.
        assert_eq!(
            schemaic_core::ddl::trigger_condition(
                &pg_trigger_when(REAL_DEF).unwrap(),
                schemaic_core::intel::SqlDialect::Postgres,
            ),
            "new.total > (0)::numeric"
        );
    }

    #[test]
    fn pg_trigger_when_is_none_without_a_guard() {
        let def = "CREATE TRIGGER t AFTER INSERT ON public.x FOR EACH ROW \
                   EXECUTE FUNCTION f()";
        assert!(pg_trigger_when(def).is_none());
    }

    /// The word may appear inside a literal argument; only the clause before the
    /// call counts, and a paren inside a string mustn't close the group.
    #[test]
    fn pg_trigger_when_ignores_the_word_inside_an_argument() {
        let def = "CREATE TRIGGER t AFTER INSERT ON public.x FOR EACH ROW \
                   EXECUTE FUNCTION f(' WHEN (nope')";
        assert!(pg_trigger_when(def).is_none());

        let def = "CREATE TRIGGER t AFTER INSERT ON public.x FOR EACH ROW \
                   WHEN ((new.note = 'a)b')) EXECUTE FUNCTION f()";
        assert_eq!(
            pg_trigger_when(def).as_deref(),
            Some("((new.note = 'a)b'))")
        );
    }

    /// Regression: the anchor was `rfind(" WHEN (")`, so a guard containing its
    /// own `CASE WHEN` recorded the **inner** condition — the recreated trigger
    /// then fires on a different set of rows, silently.
    #[test]
    fn pg_trigger_when_takes_the_outer_clause_not_an_inner_case_when() {
        // Copied verbatim from PG 16.14's `pg_get_triggerdef` — it pretty-prints
        // a `CASE` across lines under a *single* outer paren, so the anchor has
        // to survive both the newlines and the inner `WHEN (`.
        let def = "CREATE TRIGGER t_case AFTER UPDATE ON wp2.t FOR EACH ROW WHEN (\n\
                   \x20       CASE\n\
                   \x20           WHEN (new.a > 0) THEN true\n\
                   \x20           ELSE false\n\
                   \x20       END) EXECUTE FUNCTION audit_fn()";
        let got = pg_trigger_when(def).expect("guard");
        assert!(got.starts_with("(\n"), "{got}");
        assert!(got.ends_with("END)"), "{got}");
        assert!(got.contains("WHEN (new.a > 0) THEN true"), "{got}");
        // The whole expression, not the inner condition the old `rfind` picked.
        assert_ne!(got, "(new.a > 0)");
    }

    /// Regression: the hand-rolled scan knew only `'`, so a quoted *identifier*
    /// holding an apostrophe latched it into a string that never ended and the
    /// guard came back as absent — the recreated trigger then fires
    /// unconditionally.
    #[test]
    fn pg_trigger_when_survives_an_apostrophe_in_a_quoted_identifier() {
        let def = "CREATE TRIGGER t AFTER UPDATE ON public.x FOR EACH ROW \
                   WHEN ((new.\"it's\" > 0)) EXECUTE FUNCTION f()";
        assert_eq!(
            pg_trigger_when(def).as_deref(),
            Some("((new.\"it's\" > 0))")
        );
    }

    /// Regression: `rfind("EXECUTE FUNCTION ")` landed **inside** the literal
    /// argument that contains the same text, so the argument list was read from
    /// the wrong offset and came back as a single `", "`.
    #[test]
    fn pg_trigger_args_are_not_located_by_a_keyword_inside_an_argument() {
        let def = "CREATE TRIGGER t AFTER INSERT ON public.x FOR EACH ROW \
                   EXECUTE FUNCTION audit_fn('EXECUTE FUNCTION x(', 'b')";
        assert_eq!(pg_trigger_args(def), vec!["EXECUTE FUNCTION x(", "b"]);
    }

    #[test]
    fn pg_trigger_args_survives_the_real_definition() {
        assert_eq!(pg_trigger_args(REAL_DEF), vec!["audit", "it's"]);
    }

    #[test]
    fn pg_trigger_args_reads_the_literals_unquoted() {
        let def = "CREATE TRIGGER a AFTER INSERT ON public.t FOR EACH ROW \
                   EXECUTE FUNCTION public.audit('orders', 'ins')";
        assert_eq!(pg_trigger_args(def), vec!["orders", "ins"]);
    }

    #[test]
    fn pg_trigger_args_unescapes_a_doubled_quote() {
        // The model holds raw values and `create_sql` re-quotes them, so this has
        // to come back as one quote or a round trip grows an escape per edit.
        let def = "… EXECUTE FUNCTION f('it''s', 'b')";
        assert_eq!(pg_trigger_args(def), vec!["it's", "b"]);
    }

    #[test]
    fn pg_trigger_args_handles_no_args_and_the_pre_11_spelling() {
        assert!(pg_trigger_args("… EXECUTE FUNCTION f()").is_empty());
        assert_eq!(pg_trigger_args("… EXECUTE PROCEDURE f('x')"), vec!["x"]);
        // Nothing to read at all rather than a panic.
        assert!(pg_trigger_args("CREATE TRIGGER a AFTER INSERT ON t").is_empty());
    }

    /// A close-paren inside a literal must not end the argument list — the same
    /// hazard `peel_parens` goes through `skip_noncode` for.
    #[test]
    fn pg_trigger_args_ignores_a_paren_inside_a_literal() {
        assert_eq!(
            pg_trigger_args("… EXECUTE FUNCTION f('a)b', 'c')"),
            vec!["a)b", "c"]
        );
    }

    #[test]
    fn pg_view_options_splits_check_option_from_the_storage_parameters() {
        let o = pg_view_options("v", "security_barrier=true,check_option=cascaded", "t");
        assert_eq!(o.check_option.as_deref(), Some("CASCADED"));
        assert_eq!(o.storage, vec!["security_barrier=true".to_string()]);
        assert!(!o.materialized);

        // A plain view with no options at all.
        assert_eq!(pg_view_options("v", "", "t"), ViewOptions::default());

        // `relkind = 'm'` is the one thing that makes a view uneditable here.
        assert!(pg_view_options("m", "", "t").materialized);
    }

    /// `relispopulated` is what tells a `WITH NO DATA` materialized view apart,
    /// and PostgreSQL refuses a concurrent refresh on one — so reading it wrong
    /// makes *Refresh view* inoperable for that view.
    ///
    /// Only a literal `f` means unpopulated. An older server, or a column that
    /// stopped being selected, reads as populated: that is the answer which
    /// leaves the non-blocking form on offer rather than silently withdrawing
    /// it from every materialized view there is.
    #[test]
    fn only_a_false_relispopulated_marks_a_view_unpopulated() {
        assert!(!pg_view_options("m", "", "f").populated);
        assert!(!pg_view_options("m", "", "F").populated);
        assert!(pg_view_options("m", "", "t").populated);
        assert!(pg_view_options("m", "", "").populated);
        // And a plain view, which has no such notion, is never "unpopulated".
        assert!(pg_view_options("v", "", "t").populated);
    }

    #[test]
    fn pg_type_name_maps_numeric_and_text_types() {
        // Integer names start with INT/SMALLINT/BIGINT so parse_typed treats them
        // as integers.
        assert_eq!(pg_type_name(&Type::INT4), "INTEGER");
        assert_eq!(pg_type_name(&Type::INT2), "SMALLINT");
        assert_eq!(pg_type_name(&Type::INT8), "BIGINT");
        // Float names start with FLOAT/DOUBLE so parse_typed floats them.
        assert_eq!(pg_type_name(&Type::FLOAT4), "FLOAT");
        assert_eq!(pg_type_name(&Type::FLOAT8), "DOUBLE PRECISION");
        // NUMERIC stays a string (exact, never a lossy float).
        assert_eq!(pg_type_name(&Type::NUMERIC), "NUMERIC");
        assert_eq!(pg_type_name(&Type::VARCHAR), "VARCHAR");
        assert_eq!(pg_type_name(&Type::TEXT), "TEXT");
        // Unknown → uppercased passthrough.
        assert_eq!(pg_type_name(&Type::INET), "INET");
    }

    #[test]
    fn pg_type_name_str_matches_grid_for_udt_names() {
        // The schema panel feeds `udt_name` (internal pg type names) through the
        // SAME mapper the grid uses for wire types, so both agree on the short form
        // instead of the verbose `information_schema.data_type`.
        assert_eq!(pg_type_name_str("varchar"), "VARCHAR"); // not "character varying"
        assert_eq!(pg_type_name_str("bpchar"), "CHAR");
        assert_eq!(pg_type_name_str("timestamp"), "TIMESTAMP"); // not "… without time zone"
        assert_eq!(pg_type_name_str("timestamptz"), "TIMESTAMPTZ"); // not "… with time zone"
        assert_eq!(pg_type_name_str("int4"), "INTEGER");
        assert_eq!(pg_type_name_str("numeric"), "NUMERIC");
        // The wire-type path routes through the string mapper → identical output.
        assert_eq!(pg_type_name(&Type::VARCHAR), pg_type_name_str("varchar"));
        assert_eq!(
            pg_type_name(&Type::TIMESTAMP),
            pg_type_name_str("timestamp")
        );
    }

    /// `pg_type_name(&Type::BYTEA)` is what the cell renderer gates on, so if
    /// it ever stopped being a name `core::model::type_is_binary` recognises,
    /// every bytea would silently go back to dumping hex into the grid.
    #[test]
    fn the_bytea_type_name_is_one_core_reads_as_binary() {
        assert!(schemaic_core::model::type_is_binary(&pg_type_name(
            &Type::BYTEA
        )));
    }

    /// One column's cell, through the same pair the read loop uses:
    /// `cell_kinds` decides, `pg_cell` renders. Split so the answer is computed
    /// once per result rather than per cell — and asserted together, because a
    /// test that passed the flag by hand would still pass if `cell_kinds` stopped
    /// recognising `BYTEA`.
    fn one_cell(text: &str, type_name: &str) -> Value {
        pg_cell(text, cell_kinds(&[type_name.to_string()])[0])
    }

    #[test]
    fn a_hex_bytea_renders_as_its_size() {
        let t = pg_type_name(&Type::BYTEA);
        assert_eq!(
            one_cell("\\x48656c6c6f", &t),
            Value::Str("<5 bytes>".into())
        );
        // An empty bytea is `\x` and has a size like any other.
        assert_eq!(one_cell("\\x", &t), Value::Str("<0 bytes>".into()));
    }

    /// The `escape` output format (`bytea_output = escape`) is a different
    /// encoding this cannot measure. Its text is still the value, so it must
    /// survive — replacing it with a placeholder would lose data the user can
    /// currently read.
    #[test]
    fn an_escape_format_bytea_keeps_its_text() {
        let t = pg_type_name(&Type::BYTEA);
        assert_eq!(one_cell("Hello\\000", &t), Value::Str("Hello\\000".into()));
        // Odd-length and non-hex payloads are not hex output either.
        assert_eq!(bytea_hex_len("\\x4"), None);
        assert_eq!(bytea_hex_len("\\xzz"), None);
        assert_eq!(bytea_hex_len("48656c"), None);
    }

    /// **The decoder and the counter agree on what is hex output.** They are the
    /// two halves of one `bytea` text form — one runs on every loaded cell, the
    /// other only when the panel fetches — and a value either has this encoding
    /// or does not. A decoder that salvaged what the counter rejects would put
    /// bytes on screen that are not the value.
    #[test]
    fn bytea_hex_decodes_to_the_bytes_it_counts() {
        assert_eq!(bytea_hex_bytes("\\x48656c6c6f"), Some(b"Hello".to_vec()));
        assert_eq!(bytea_hex_bytes("\\x"), Some(Vec::new()));
        // Case-insensitive, and the high bytes a text path would have mangled.
        assert_eq!(
            bytea_hex_bytes("\\x89504E470D0a1A0a"),
            Some(b"\x89PNG\r\n\x1a\n".to_vec())
        );
        for bad in ["\\x4", "\\xzz", "48656c", "Hello\\000", ""] {
            assert_eq!(bytea_hex_bytes(bad), None, "{bad:?} should not decode");
            assert_eq!(bytea_hex_len(bad), None, "{bad:?} should not count");
        }
        // And where it does decode, the two answers are the same length.
        for good in ["\\x", "\\x00", "\\xdeadbeef"] {
            assert_eq!(
                bytea_hex_bytes(good).map(|b| b.len()),
                bytea_hex_len(good),
                "{good:?}"
            );
        }
    }

    /// A `text` column may legitimately hold the characters `\x4869`, and the
    /// type gate is the only thing standing between that and a cell replaced by
    /// a byte count.
    #[test]
    fn a_text_cell_that_looks_like_hex_output_is_left_alone() {
        let t = pg_type_name(&Type::TEXT);
        assert_eq!(one_cell("\\x4869", &t), Value::Str("\\x4869".into()));
    }

    /// The pg type names feed the shared parser correctly — and through the
    /// hoisted path the row loop actually takes (`cell_kinds` once per column,
    /// then `pg_cell` per cell), not through the one-cell `parse_typed`
    /// convenience. The two cannot disagree, because `parse_typed` *is* their
    /// composition, but the loop is what runs 100M times.
    #[test]
    fn parse_typed_uses_pg_type_names() {
        assert!(matches!(
            one_cell("42", &pg_type_name(&Type::INT4)),
            Value::Int(42)
        ));
        assert!(matches!(
            one_cell("1.5", &pg_type_name(&Type::FLOAT8)),
            Value::Float(f) if f == 1.5
        ));
        // NUMERIC preserved as an exact string.
        assert!(matches!(
            one_cell("1.10", &pg_type_name(&Type::NUMERIC)),
            Value::Str(s) if s == "1.10"
        ));
        // An unsigned MySQL spelling still reaches the `u64` arm — `num_kind` is
        // shared, and this is the one branch a per-column split could drop.
        assert!(matches!(
            crate::parse_typed("18446744073709551615".to_string(), "BIGINT UNSIGNED"),
            Value::UInt(u) if u == u64::MAX
        ));
    }

    #[test]
    fn cell_handles_null_and_missing() {
        let row = vec![Some("a".to_string()), None];
        assert_eq!(cell(&row, 0), "a");
        assert_eq!(cell(&row, 1), ""); // SQL NULL → empty
        assert_eq!(cell(&row, 5), ""); // out of range → empty
    }

    #[test]
    fn pg_ident_double_quotes_and_escapes() {
        assert_eq!(pg_ident("plain"), "\"plain\"");
        assert_eq!(pg_ident("Weird Name"), "\"Weird Name\"");
        // Embedded double-quote is doubled.
        assert_eq!(pg_ident("a\"b"), "\"a\"\"b\"");
    }

    #[test]
    fn pg_str_lit_escapes_single_quotes() {
        assert_eq!(pg_str_lit("abc"), "'abc'");
        assert_eq!(pg_str_lit("O'Brien"), "'O''Brien'");
        // A classic injection attempt is neutralised by doubling the quote.
        assert_eq!(
            pg_str_lit("x'; DROP TABLE t; --"),
            "'x''; DROP TABLE t; --'"
        );
    }

    /// **What the doubling does not do, stated so the dependency is not
    /// forgotten again.** `pg_str_lit` leaves a backslash alone, which is
    /// correct under `standard_conforming_strings = on` and only there: with it
    /// off, PostgreSQL reads the literal as an escape string, `\'` is an escaped
    /// quote rather than a backslash and a quote, and a row's own text data ends
    /// the literal — after which the rest of that value is parsed as SQL. On
    /// `blob_on`, which speaks the multi-statement simple protocol, that is a
    /// second statement running from a single click on a connection marked
    /// read-only.
    ///
    /// The setting is `USERSET` and settable per database and per role, so the
    /// default is not a guarantee. `connect_probe` pins it on the startup packet
    /// — in force before the first statement and not undoable by anything the
    /// app runs — which is the same dependency this module already refuses to
    /// take in `roles`' prefix comparison and in `pg_cell_lit`'s argument for
    /// `decode(…,'hex')`.
    #[test]
    fn pg_str_lit_leaves_a_backslash_alone_which_is_why_the_setting_is_pinned() {
        assert_eq!(pg_str_lit("a\\b"), "'a\\b'");
        // The shape that escapes when the setting is off: the backslash claims
        // the quote after it, so the literal runs on into the `;`.
        assert_eq!(
            pg_str_lit("x\\'; DROP TABLE t; --"),
            "'x\\''; DROP TABLE t; --'"
        );
    }

    #[test]
    fn pg_value_lit_numbers_bare_text_quoted() {
        assert_eq!(pg_value_lit(&Value::Int(-7)), "-7");
        assert_eq!(pg_value_lit(&Value::UInt(42)), "42");
        assert_eq!(pg_value_lit(&Value::Str("ab".into())), "'ab'");
        assert_eq!(pg_value_lit(&Value::Null), "NULL");
    }

    #[test]
    fn build_update_shape_null_safe_key() {
        let edit = RowEdit {
            database: "world".into(),
            schema: None,
            table: "city".into(),
            set: vec![
                ("name".into(), CellEdit::Text("Kabul".into())),
                ("district".into(), CellEdit::Null), // set to NULL
            ],
            key: vec![("id".into(), Value::Int(1))],
        };
        assert_eq!(
            build_update(&edit),
            "UPDATE \"city\" SET \"name\" = 'Kabul', \"district\" = NULL \
             WHERE \"id\" IS NOT DISTINCT FROM 1"
        );
    }

    /// **The bytea literal, and the form it deliberately is not.**
    /// `standard_conforming_strings` is settable per database and per role, and
    /// this module already refuses to depend on it once (`roles`). `decode` is
    /// the form that has nothing to escape: hex digits only, so there is no
    /// backslash to be eaten and no quote to double.
    #[test]
    fn build_update_emits_a_bytea_literal_with_nothing_to_escape() {
        let png = vec![0x89u8, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        let edit = RowEdit {
            database: "d".into(),
            schema: None,
            table: "docs".into(),
            set: vec![
                ("title".into(), CellEdit::Text("x".into())),
                ("payload".into(), CellEdit::bytes(png)),
            ],
            key: vec![("id".into(), Value::Int(1))],
        };
        let sql = build_update(&edit);
        assert_eq!(
            sql,
            "UPDATE \"docs\" SET \"title\" = 'x', \"payload\" = decode('89504e470d0a1a0a', 'hex') \
             WHERE \"id\" IS NOT DISTINCT FROM 1"
        );
        // The whole point of the form: no backslash to survive a literal.
        assert!(!sql.contains('\\'), "no escape for the server to interpret");
    }

    /// Hex is lower-case and **two digits per byte, always** — a `0x0A` written
    /// as one digit shifts every byte after it and `decode` rejects an odd-length
    /// string, so the failure would be a runtime error on some blobs only.
    #[test]
    fn bytea_hex_is_zero_padded_two_digits_per_byte() {
        let lit = pg_cell_lit(&CellEdit::bytes(vec![0x00, 0x01, 0x0F, 0xA0, 0xFF]));
        assert_eq!(lit, "decode('00010fa0ff', 'hex')");
        // An empty file is an empty bytea, not NULL — `decode('', 'hex')` is a
        // zero-length bytea, which is the value the user loaded.
        assert_eq!(
            pg_cell_lit(&CellEdit::bytes(Vec::new())),
            "decode('', 'hex')"
        );
        assert_eq!(pg_cell_lit(&CellEdit::Null), "NULL");
        assert_eq!(pg_cell_lit(&CellEdit::Text("O'B".into())), "'O''B'");
    }

    #[test]
    fn build_insert_shapes_including_default_values() {
        let ins = RowInsert {
            database: "world".into(),
            schema: None,
            table: "country".into(),
            cols: vec![
                ("code".into(), CellEdit::Text("AAA".into())),
                ("name".into(), CellEdit::Null),
            ],
        };
        assert_eq!(
            build_insert(&ins),
            "INSERT INTO \"country\" (\"code\", \"name\") VALUES ('AAA', NULL)"
        );
        // No columns set → Postgres all-defaults form (NOT `() VALUES ()`).
        let empty = RowInsert {
            database: "world".into(),
            schema: None,
            table: "t".into(),
            cols: vec![],
        };
        assert_eq!(build_insert(&empty), "INSERT INTO \"t\" DEFAULT VALUES");
    }

    #[test]
    fn build_delete_shape_composite_key() {
        let del = RowDelete {
            database: "world".into(),
            schema: None,
            table: "countrylanguage".into(),
            key: vec![
                ("countrycode".into(), Value::Str("NLD".into())),
                ("language".into(), Value::Str("Dutch".into())),
            ],
        };
        assert_eq!(
            build_delete(&del),
            "DELETE FROM \"countrylanguage\" \
             WHERE \"countrycode\" IS NOT DISTINCT FROM 'NLD' \
             AND \"language\" IS NOT DISTINCT FROM 'Dutch'"
        );
    }

    // ── multi-schema write path ───────────────────────────────────────────

    #[test]
    fn pg_qname_qualifies_whenever_a_namespace_is_known() {
        // Unlike the user-facing SQL, the write path qualifies `public` too — an
        // invisible statement must not depend on `search_path`.
        assert_eq!(pg_qname(Some("public"), "city"), "\"public\".\"city\"");
        assert_eq!(pg_qname(Some("sales"), "orders"), "\"sales\".\"orders\"");
        // No namespace (MySQL-shaped origin, or unresolved) → bare name.
        assert_eq!(pg_qname(None, "city"), "\"city\"");
        // Both halves are escaped.
        assert_eq!(
            pg_qname(Some("we\"ird"), "t\"x"),
            "\"we\"\"ird\".\"t\"\"x\""
        );
    }

    #[test]
    fn writes_target_the_rows_own_namespace() {
        // The three write shapes must all name the schema the row came from —
        // otherwise a same-named table earlier on the search_path gets the write.
        let edit = RowEdit {
            database: "warehouse".into(),
            schema: Some("sales".into()),
            table: "orders".into(),
            set: vec![("total".into(), CellEdit::Text("9".into()))],
            key: vec![("id".into(), Value::Int(1))],
        };
        assert_eq!(
            build_update(&edit),
            "UPDATE \"sales\".\"orders\" SET \"total\" = '9' \
             WHERE \"id\" IS NOT DISTINCT FROM 1"
        );

        let ins = RowInsert {
            database: "warehouse".into(),
            schema: Some("sales".into()),
            table: "orders".into(),
            cols: vec![("total".into(), CellEdit::Text("9".into()))],
        };
        assert_eq!(
            build_insert(&ins),
            "INSERT INTO \"sales\".\"orders\" (\"total\") VALUES ('9')"
        );
        let empty = RowInsert {
            cols: vec![],
            ..ins
        };
        assert_eq!(
            build_insert(&empty),
            "INSERT INTO \"sales\".\"orders\" DEFAULT VALUES"
        );

        let del = RowDelete {
            database: "warehouse".into(),
            schema: Some("sales".into()),
            table: "orders".into(),
            key: vec![("id".into(), Value::Int(1))],
        };
        assert_eq!(
            build_delete(&del),
            "DELETE FROM \"sales\".\"orders\" WHERE \"id\" IS NOT DISTINCT FROM 1"
        );
    }

    /// **The trigger picker and the browse list are different questions**, and
    /// the extension exclusion belongs to only one of them. A dropdown with no
    /// free-text entry means a function missing from this list is a function no
    /// trigger can be pointed at — and `moddatetime`, the standard "touch the
    /// modified column" function, arrives owned by its extension.
    #[test]
    fn the_trigger_function_filter_keeps_what_an_extension_owns() {
        let f = trigger_function_filter();
        assert!(!f.contains("deptype"), "{f}");
        // Narrowed on the *server*, not by pulling every body over the wire.
        assert!(
            f.contains("p.prorettype IN ('trigger'::regtype, 'event_trigger'::regtype)"),
            "{f}"
        );
        // Both return types, because `is_trigger_function` accepts both — a
        // model that calls one bindable over a query that never returns it is a
        // row that cannot be selected.
        assert!(f.contains("event_trigger"), "{f}");
        // Still routines in a user namespace, on the shared floor.
        assert!(f.contains("p.prokind IN ('f', 'p')"), "{f}");
        assert!(f.contains("'pg_catalog', 'information_schema'"), "{f}");
    }

    /// The three narrowings, asserted on the string because that is all a unit
    /// test can reach — and each has a wrong answer that produces a plausible
    /// list rather than an error.
    #[test]
    fn the_routine_filter_takes_only_editable_user_routines() {
        let f = routine_filter();
        // Functions and procedures, never aggregates or window functions.
        assert!(f.contains("p.prokind IN ('f', 'p')"), "{f}");
        // The namespace filter is spliced, not re-spelled, so the five queries
        // that use it cannot diverge.
        assert!(f.contains("'pg_catalog', 'information_schema'"), "{f}");
        // Extension-owned routines are excluded — PostGIS alone would otherwise
        // put ~1000 into the Functions folder of whichever namespace it was
        // created in.
        assert!(f.contains("d.deptype = 'e'"), "{f}");
        assert!(f.contains("NOT EXISTS"), "{f}");
        // Scoped to `pg_proc`: an oid is only unique within its catalogue, so a
        // dependency on some other object with the same numeric oid would
        // otherwise hide a routine at random.
        assert!(f.contains("d.classid = 'pg_proc'::regclass"), "{f}");
    }

    #[test]
    fn user_schema_filter_excludes_only_postgres_internals() {
        let f = user_schema_filter("n.nspname");
        assert!(f.contains("'pg_catalog', 'information_schema'"));
        assert!(f.contains("pg\\_toast%"));
        assert!(f.contains("pg\\_temp%"));
        // Extension-owned namespaces stay browsable — they aren't named here.
        assert!(!f.contains("topology"));
        // The alias is spliced everywhere, so the five queries can't diverge.
        assert_eq!(f.matches("n.nspname").count(), 4);
    }

    // ── The statistics queries ────────────────────────────────────────────
    //
    // The per-engine half of the properties panel is these two strings: which
    // relkinds get a size, what a negative `reltuples` means, what the primary
    // key's index is called. Each has a wrong answer that produces a
    // plausible-looking panel rather than an error, and the divergence this
    // convention exists to catch is the one that shipped — a guard present in one
    // builder and absent from its sibling.

    /// A partitioned parent has no storage of its own, so it is listed with null
    /// sizes rather than a truthful `0` that reads as "this 40 GB table is empty".
    #[test]
    fn the_table_stats_query_nulls_a_partitioned_parents_sizes() {
        let q = table_stats_sql();
        assert!(
            q.contains("CASE WHEN c.relkind = 'p' THEN NULL ELSE pg_table_size(c.oid) END"),
            "{q}"
        );
        assert!(
            q.contains("CASE WHEN c.relkind = 'p' THEN NULL ELSE pg_indexes_size(c.oid) END"),
            "both sizes, or the split disagrees with the total: {q}"
        );
        // `reltuples` is -1 on a relation that has never been analyzed. Passed
        // through it would say "0 rows" about a full table.
        assert!(q.contains("CASE WHEN c.reltuples < 0 THEN NULL"), "{q}");
        // Sized relkinds only, and the partitioned parent listed among them so it
        // still gets its row estimate.
        assert!(q.contains("c.relkind IN ('r','m','p')"), "{q}");
    }

    /// **The sibling guard, in the index's own spelling.** A partitioned index
    /// (`relkind = 'I'`) is the same parent-with-no-storage as a partitioned
    /// table, and `pg_relation_size` returns `0` for it — which the panel printed
    /// as `0 B` and Copy exported, for an index over 40 GB of partitions.
    #[test]
    fn the_index_stats_query_nulls_a_partitioned_indexs_size() {
        let q = index_stats_sql();
        assert!(
            q.contains("CASE WHEN ic.relkind = 'I' THEN NULL ELSE pg_relation_size(ic.oid) END"),
            "{q}"
        );
        // And the scan count is left unguarded on purpose: `pg_stat_all_indexes`
        // has no row for a partitioned index, so NULL there means "nobody
        // counted", which is what keeps `is_unused` from flagging it.
        assert!(q.contains("si.idx_scan"), "{q}");
        assert!(!q.contains("THEN NULL ELSE si.idx_scan"), "{q}");
    }

    /// The primary key is named the same thing on every surface — the designer's
    /// index list calls it `PRIMARY`, so the properties panel must too, or the two
    /// describe the same index under two names.
    #[test]
    fn the_index_stats_query_names_the_primary_key_as_the_designer_does() {
        let q = index_stats_sql();
        assert!(
            q.contains("CASE WHEN ix.indisprimary THEN 'PRIMARY' ELSE ic.relname END"),
            "{q}"
        );
        assert!(
            index_list_sql().contains("'PRIMARY'"),
            "the claim is about agreeing with this one"
        );
    }

    /// Both statistics queries filter internal schemas through the shared
    /// builder, so a new namespace rule reaches them with the rest.
    #[test]
    fn both_statistics_queries_filter_internal_schemas() {
        for q in [table_stats_sql(), index_stats_sql()] {
            assert!(q.contains("n.nspname NOT IN"), "{q}");
            assert!(q.contains("pg\\_toast%"), "{q}");
        }
    }

    #[test]
    fn schema_sort_key_puts_public_first() {
        let mut v = vec!["sales", "public", "analytics"];
        v.sort_by_key(|s| schema_sort_key(s));
        assert_eq!(v, vec!["public", "analytics", "sales"]);
    }

    /// `no_default` means "the user must supply this", not "there is no DEFAULT
    /// clause" — the difference is the NOT-NULL term, and PostgreSQL was missing
    /// it, so the grid's new-row preview called most nullable columns
    /// `<required>` in red.
    #[test]
    fn pg_no_default_means_the_user_must_supply_it() {
        // NOT NULL, no default, not identity → the user must fill it in.
        assert!(pg_no_default(true, false, ""));
        // Nullable is the case that regressed: NULL is its implicit default.
        assert!(!pg_no_default(false, false, ""));
        // A DEFAULT clause supplies it.
        assert!(!pg_no_default(true, true, ""));
        // So does an identity column, either form.
        assert!(!pg_no_default(true, false, "a"));
        assert!(!pg_no_default(true, false, "d"));
    }
}

#[cfg(test)]
mod table_list_tests {
    use super::*;

    /// `information_schema.tables` **cannot** return a materialized view:
    /// PostgreSQL 16's own catalogue definition filters
    /// `c.relkind IN ('r','v','f','p')` and `'m'` is absent. Reading the object
    /// list from it made every matview invisible everywhere — tree, completion,
    /// ERD, Find-Anywhere, catalog — while the four *other* queries in
    /// `fetch_schema` all already reached `'m'`, so its columns, body and options
    /// were fetched over the wire and then silently discarded.
    #[test]
    fn the_object_list_does_not_come_from_information_schema() {
        let sql = table_list_sql();
        assert!(
            !sql.contains("information_schema.tables"),
            "the one view that structurally cannot answer this question"
        );
        assert!(sql.contains("pg_class"));
        assert!(sql.contains("pg_namespace"));
    }

    #[test]
    fn the_object_list_reaches_materialized_views() {
        let sql = table_list_sql();
        // Ordinary tables, views, matviews, partitioned tables, foreign tables.
        for kind in ["'r'", "'v'", "'m'", "'p'", "'f'"] {
            assert!(sql.contains(kind), "relkind {kind} missing from {sql}");
        }
    }

    /// A matview has to arrive typed `VIEW`, or `assemble_schema` builds a
    /// `TableInfo` with `is_view` false and the whole view path — the editor's
    /// drop-only gate, the context menu — stays unreachable for it.
    #[test]
    fn views_and_matviews_are_both_typed_view() {
        let sql = table_list_sql();
        assert!(sql.contains("IN ('v','m') THEN 'VIEW'"));
    }

    #[test]
    fn the_object_list_uses_the_shared_namespace_filter() {
        // Same filter as the other four queries, so the five can't diverge on
        // which schemas are browsable.
        assert!(table_list_sql().contains(&user_schema_filter("n.nspname")));
    }
}

#[cfg(test)]
mod index_key_tests {
    use super::*;

    /// `pg_get_indexdef(oid, n, true)` gives an ordinary key as a bare name and a
    /// computed one as its expression — wrapped when it is a compound. The model
    /// stores the bare expression because `IndexInfo::key_sql` adds exactly one
    /// pair back; leaving PostgreSQL's pair on would double them.
    #[test]
    fn an_expression_key_is_stored_without_its_wrapping_parens() {
        assert_eq!(expr_key("lower(email)"), "lower(email)");
        assert_eq!(
            expr_key("((first_name || last_name))"),
            "(first_name || last_name)"
        );
        assert_eq!(expr_key("  (created_at::date)  "), "created_at::date");
    }

    /// Only the pair that wraps the *whole* expression comes off. Stripping the
    /// first `(` and the last `)` of `(a) + (b)` would leave `a) + (b`.
    #[test]
    fn only_a_pair_enclosing_the_whole_expression_is_stripped() {
        assert_eq!(expr_key("(a) + (b)"), "(a) + (b)");
        assert_eq!(expr_key("coalesce(a, b) || c"), "coalesce(a, b) || c");
        assert_eq!(expr_key("(unbalanced"), "(unbalanced");
    }

    /// The three things the index query has to read per key position, and the one
    /// join that has to stay LEFT. An expression key has no `pg_attribute` row at
    /// all (PostgreSQL stores `0` in `indkey`), so an inner join drops the whole
    /// position — which is how those indexes came back missing a key and had to
    /// be refused as lossy.
    #[test]
    fn the_index_query_reads_each_key_position_in_full() {
        let sql = index_list_sql();
        assert!(
            sql.contains("pg_get_indexdef(ix.indexrelid, k.ord::int, true)"),
            "the per-position key text, which is the expression for a computed key"
        );
        assert!(
            sql.contains("LEFT JOIN pg_attribute a"),
            "an expression position has no pg_attribute row to join to"
        );
        assert!(
            sql.contains("unnest(ix.indoption) WITH ORDINALITY"),
            "DESC comes from indoption, paired by ordinality rather than subscript"
        );
    }

    /// What `lossy` still has to cover, and what it must no longer claim.
    /// `indoption` 0 is `ASC NULLS LAST` and 3 is `DESC NULLS FIRST` — the two
    /// defaults, both of which the model can now express. 1 and 2 are the
    /// spellings whose NULLS ordering would be lost.
    #[test]
    fn lossy_no_longer_covers_what_the_model_can_hold() {
        let sql = index_list_sql();
        assert!(
            !sql.contains("0 = ANY(ix.indkey"),
            "an expression key is read now, not refused"
        );
        assert!(
            sql.contains("opt NOT IN (0, 3)"),
            "only a non-default NULLS ordering is lossy, not every non-zero option"
        );
        assert!(
            sql.contains("NOT o.opcdefault"),
            "a non-default operator class is still unreadable per column"
        );
    }

    // ── Standalone objects ──────────────────────────────────────────────────

    /// A catalogue row from a list of column texts, `None` for a NULL.
    fn row(cells: &[&str]) -> Vec<Option<String>> {
        cells.iter().map(|c| Some((*c).to_string())).collect()
    }

    #[test]
    fn an_enum_takes_its_labels_in_query_order() {
        let types = vec![row(&[
            "public",
            "mood",
            "e",
            "",
            "",
            "0",
            "",
            "how it went",
            "0",
        ])];
        let labels = vec![
            row(&["public", "mood", "sad"]),
            row(&["public", "mood", "ok"]),
            // Another type's labels must not leak in.
            row(&["public", "other", "nope"]),
            row(&["sales", "mood", "great"]),
        ];
        let (enums, domains) = pg_fold_types(&types, &labels, &[]);
        assert!(domains.is_empty());
        assert_eq!(enums.len(), 1);
        assert_eq!(enums[0].values, vec!["sad", "ok"]);
        assert_eq!(enums[0].comment.as_deref(), Some("how it went"));
    }

    /// A label may be the empty string, a comma, or a newline — which is why the
    /// labels arrive one per row instead of string-aggregated. Any separator at
    /// all would be a value some database already stores.
    #[test]
    fn enum_labels_survive_being_commas_newlines_and_empty() {
        let types = vec![row(&["public", "weird", "e", "", "", "0", "", "", "0"])];
        let labels = vec![
            row(&["public", "weird", "a,b"]),
            row(&["public", "weird", "line1\nline2"]),
            row(&["public", "weird", ""]),
        ];
        let (enums, _) = pg_fold_types(&types, &labels, &[]);
        assert_eq!(enums[0].values, vec!["a,b", "line1\nline2", ""]);
    }

    #[test]
    fn an_enum_with_no_labels_is_still_an_enum() {
        // `CREATE TYPE t AS ENUM ()` is legal and contributes no label rows.
        let types = vec![row(&["public", "empty", "e", "", "", "0", "", "", "0"])];
        let (enums, _) = pg_fold_types(&types, &[], &[]);
        assert_eq!(enums.len(), 1);
        assert!(enums[0].values.is_empty());
    }

    /// The whole reason `has default` is its own column: `DEFAULT ''` is a real
    /// default, and reading it off the text would drop it on every replay.
    #[test]
    fn a_domain_defaulting_to_the_empty_string_still_has_a_default() {
        let blank = vec![row(&[
            "public", "blankdef", "d", "text", "", "0", "", "", "1",
        ])];
        let (_, domains) = pg_fold_types(&blank, &[], &[]);
        assert_eq!(domains[0].default_value.as_deref(), Some(""));

        let none = vec![row(&["public", "plain", "d", "text", "", "0", "", "", "0"])];
        let (_, domains) = pg_fold_types(&none, &[], &[]);
        assert_eq!(domains[0].default_value, None);
    }

    #[test]
    fn a_domains_constraints_are_normalized_like_a_tables() {
        let types = vec![row(&[
            "public", "positive", "d", "integer", "", "1", "en_US", "", "0",
        ])];
        let checks = vec![
            row(&[
                "public",
                "positive",
                "positive_check",
                "CHECK ((VALUE > 0))",
            ]),
            row(&["public", "elsewhere", "other", "CHECK (false)"]),
        ];
        let (_, domains) = pg_fold_types(&types, &[], &checks);
        assert_eq!(domains.len(), 1);
        assert!(domains[0].not_null);
        assert_eq!(domains[0].collation.as_deref(), Some("en_US"));
        assert_eq!(domains[0].checks.len(), 1);
        // Held bare: `check_predicate` peels the server's wrapping `CHECK (…)`.
        assert_eq!(domains[0].checks[0].expression, "VALUE > 0");
        assert!(domains[0].checks[0].enforced);
    }

    fn seq_row(extra: &[&str]) -> Vec<Option<String>> {
        let mut base = vec![
            "public",
            "counter",
            "bigint",
            "1",
            "1",
            "1",
            "9223372036854775807",
            "1",
            "0",
        ];
        base.extend_from_slice(extra);
        row(&base)
    }

    #[test]
    fn a_sequence_reads_its_bounds_out_of_text() {
        // `seqmax` on a bigint sequence *is* `i64::MAX`; going through text is
        // what carries it intact.
        let s = pg_sequence_row(&seq_row(&["", "", "", "", ""]));
        assert_eq!(s.max_value, i64::MAX);
        assert_eq!(s.min_value, 1);
        assert_eq!(s.increment, 1);
        assert!(!s.cycle);
        assert_eq!(s.last_value, None);
        assert_eq!(s.owned_by, None);
        assert_eq!(s.comment, None);
    }

    #[test]
    fn an_unparseable_bound_falls_back_to_postgres_own_default() {
        // Never to zero: `INCREMENT BY 0` is a statement the server rejects, so a
        // display glitch would become un-runnable DDL.
        let mut r = seq_row(&["", "", "", "", ""]);
        r[4] = Some("not-a-number".into());
        assert_eq!(pg_sequence_row(&r).increment, 1);
    }

    #[test]
    fn a_sequences_owner_separates_identity_from_serial() {
        // `a`: a serial's sequence — a real object of its own.
        let serial = pg_sequence_row(&seq_row(&["", "orders", "id", "a", ""]));
        let o = serial.owned_by.expect("owned");
        assert_eq!((o.table.as_str(), o.column.as_str()), ("orders", "id"));
        assert!(!o.internal);

        // `i`: an identity column's counter — part of the column, undroppable.
        let identity = pg_sequence_row(&seq_row(&["", "orders", "id", "i", ""]));
        assert!(identity.owned_by.expect("owned").internal);

        // No dependency row at all → a free-standing sequence.
        assert!(
            pg_sequence_row(&seq_row(&["", "", "", "", ""]))
                .owned_by
                .is_none()
        );
    }

    #[test]
    fn a_blank_last_value_is_no_position_rather_than_zero() {
        // Blank means never used, or this role may not look — neither is 0.
        assert_eq!(
            pg_sequence_row(&seq_row(&["", "", "", "", ""])).last_value,
            None
        );
        assert_eq!(
            pg_sequence_row(&seq_row(&["41", "", "", "", ""])).last_value,
            Some(41)
        );
    }

    #[test]
    fn the_object_queries_use_the_shared_namespace_filter() {
        // Same rule as every other introspection query, so a namespace can't be
        // browsable for tables and invisible for its types.
        let f = user_schema_filter("n.nspname");
        assert!(f.contains("'pg_catalog', 'information_schema'"));
    }
}
