//! SQL Server backend (fourth engine), built on [`tiberius`] — vendored and
//! patched, see `vendor/tiberius/PATCHES.md`.
//!
//! Dispatched to from [`crate::Db`]'s public methods when the connection's
//! engine is [`crate::Engine::MsSql`]. **A preview, not parity.** What is here:
//! connect, list databases, run queries, batches and `.sql` scripts,
//! non-executing validation (`prepare_check`), schema introspection, table
//! statistics, server activity, the grid's write-back (`commit_writes`,
//! `refetch_rows`, `fetch_blob`) and the schema changes `ddl::supports_change`
//! allows it (`run_ddl`). The other entry points that write (`import_rows`,
//! `run_server_ddl`) and `explain` answer with a refusal naming SQL Server until
//! they are written, so a caller that skips the capability gates is told so
//! rather than handed another engine's SQL.
//!
//! **Values come over TDS typed**, not as text: an `int` arrives as an `i32`,
//! a `decimal` as a scaled integer, a `datetime2` as a day count and a count of
//! ticks. [`cell_value`] renders each to the text SQL Server itself would show,
//! and every decision in it is a pure function with a test — the equivalent of
//! PostgreSQL's text protocol, done on this side of the wire.
//!
//! **Column provenance** — which base table and column a result column came
//! from, which is what makes the grid editable — is not in TDS's column
//! metadata. It comes from `sys.dm_exec_describe_first_result_set` in browse
//! mode, which compiles the statement without running it and names each
//! column's source, the analogue of PostgreSQL's `PREPARE` here.
//!
//! **Model note:** a SQL Server *database* maps onto the app's database tree
//! level, as on PostgreSQL, and every table carries its schema — SQL Server
//! qualifies with `schema.table`, and `dbo` is only the usual default.

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use schemaic_core::blob::{BlobRef, BlobValue, FETCH_CAP};
use schemaic_core::export::language_safe_datetime;
use schemaic_core::intel::SqlDialect;
use schemaic_core::model::{
    CellEdit, Column, ColumnFlags, ColumnOrigin, GridWrite, RefetchRow, RefetchTemplate,
    ResultBuilder, ResultSet, Rollback, RowInsert, Value, WriteStep, binary_display,
    one_row_verdict, type_is_binary,
};
use schemaic_core::schema::{DbSchema, ListedDatabase, TableInfo};
use tiberius::{ColumnData, ColumnType, QueryItem};
use tokio::net::TcpStream;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};
use tokio_util::sync::CancellationToken;

use crate::{Db, DbError, RowDest};

/// This module's dialect, once — the same convention `pg.rs` keeps.
const MS: SqlDialect = SqlDialect::MsSql;

/// A connected client. The socket is tokio's, and tiberius speaks `futures`
/// I/O, hence the compat wrapper.
pub(crate) type MsClient = tiberius::Client<Compat<TcpStream>>;

/// What an unfinished entry point answers.
fn not_yet(what: &str) -> DbError {
    DbError::Refused(format!("{what} is not available for SQL Server yet."))
}

// ── Connecting ───────────────────────────────────────────────────────────────

/// The driver's configuration for this endpoint, scoped to `database` or, with
/// none, to the connection's own — and with no database at all, to the login's
/// default one, which is SQL Server's own answer and usually `master`.
///
/// Async for one mode: an Entra sign-in needs a token first, from the Azure
/// CLI or the cache in front of it ([`crate::entra`]).
///
/// **A sign-in whose TLS floor the plan does not reach is refused first**,
/// before a token is fetched — an Entra token over a session that accepts any
/// certificate (`AuthMode::transport_refusal`). A `Db` resolved from a
/// connection already carries the raised plan (`Connection::tls_plan`); this
/// is for one assembled from parts.
async fn config(db: &Db, database: Option<&str>) -> Result<tiberius::Config, DbError> {
    if let Some(why) = db.auth.transport_refusal(db.tls_plan()) {
        return Err(DbError::Refused(why.to_string()));
    }
    let mut cfg = tiberius::Config::new();
    cfg.host(&db.host);
    cfg.port(db.port);
    if let Some(d) = database.or(db.database()) {
        cfg.database(d);
    }
    cfg.authentication(auth_method(db).await?);
    cfg.application_name("Schemaic");
    // **No per-response deadline.** The driver's default is thirty seconds,
    // ADO.NET's `CommandTimeout`, which would end any query whose first row
    // takes longer — an aggregate over a large table, an index build. Every
    // caller here has a cancellation token, and a timeout is the caller's
    // decision (`PING_TIMEOUT` wraps the ones that need it).
    cfg.command_timeout(None);
    cfg.encryption(crate::tls::mssql_encryption(db.tls_plan()));
    cfg.rustls_client_config(crate::tls::mssql_client_config(db.tls_plan())?);
    if is_azure_sql(&db.host) {
        cfg.handshake_timeout(Some(AZURE_HANDSHAKE));
    }
    Ok(cfg)
}

/// How long an Azure SQL login may take — a minute, where the driver's own
/// bound is fifteen seconds. **A serverless database that has paused holds
/// the login at the gateway while it resumes**, and measured on a free-offer
/// database in Sweden Central that outlasted fifteen seconds: the first
/// connection after an idle hour failed with a bare handshake timeout, and a
/// second a minute later went straight through.
const AZURE_HANDSHAKE: std::time::Duration = std::time::Duration::from_secs(60);

/// Is `host` an Azure SQL Database server — `<name>.database.windows.net`,
/// or a sovereign cloud's spelling of it?
fn is_azure_sql(host: &str) -> bool {
    let h = host.trim().trim_end_matches('.').to_ascii_lowercase();
    [
        ".database.windows.net",
        ".database.chinacloudapi.cn",
        ".database.usgovcloudapi.net",
    ]
    .iter()
    .any(|s| h.ends_with(s))
}

/// The sentence for a login that timed out on an Azure SQL server: the
/// likely reason, and what to do — the attempt itself is what wakes a paused
/// database.
fn azure_timeout_text() -> String {
    format!(
        "Azure SQL did not finish signing in within {} seconds. A serverless database that has \
         paused takes about a minute to resume, and this attempt has woken it — connect again \
         shortly.",
        AZURE_HANDSHAKE.as_secs()
    )
}

/// Azure SQL's *database is not currently available* — what a paused
/// serverless database answers when the gateway does not hold the login
/// instead (both were seen, minutes apart, on the same free-offer database).
const AZURE_UNAVAILABLE: u32 = 40613;

/// [`azure_timeout_text`]'s sibling for Msg 40613: the server's own words,
/// then what they usually mean and what to do.
fn azure_unavailable_text(server: &str) -> String {
    format!(
        "{server} A serverless database that has paused takes about a minute to resume, and this \
         attempt has woken it — connect again shortly."
    )
}

/// How the login packet signs in, per [`Db::auth`]:
///
/// - a password — a SQL Server login;
/// - Windows — the identity this process runs as, through SSPI, so the form's
///   user and password are not sent. Compiled on Windows alone (tiberius's
///   `winauth`), and never reached elsewhere, since
///   [`schemaic_core::connection::AuthMode::offered`] does not offer it there;
/// - Entra — an Azure CLI access token, in the login's FedAuth extension.
async fn auth_method(db: &Db) -> Result<tiberius::AuthMethod, DbError> {
    use schemaic_core::connection::AuthMode;
    match db.auth {
        AuthMode::Password => Ok(tiberius::AuthMethod::sql_server(&db.user, &db.pass)),
        #[cfg(windows)]
        AuthMode::Windows => Ok(tiberius::AuthMethod::Integrated),
        #[cfg(not(windows))]
        AuthMode::Windows => Err(DbError::Refused(
            "Windows sign-in is available only in the Windows build of Schemaic.".to_string(),
        )),
        AuthMode::AzureCli => Ok(tiberius::AuthMethod::aad_token(
            crate::entra::sql_token().await?,
        )),
    }
}

/// Open a fresh connection. Follows one routing redirect, which is how Azure
/// SQL's gateway hands a client to the node that serves its database.
///
/// **A refused Entra login forgets its token**, so the next attempt asks the
/// Azure CLI again: a token the server will not take — the user signed in to
/// the CLI as someone else since, or was removed — would otherwise be handed
/// over until it expired.
pub(crate) async fn connect(db: &Db, database: Option<&str>) -> Result<MsClient, DbError> {
    let cfg = config(db, database).await?;
    let result = match connect_with(cfg.clone()).await {
        Err(tiberius::error::Error::Routing { host, port }) => {
            let mut routed = cfg;
            routed.host(&host);
            routed.port(port);
            connect_with(routed).await
        }
        other => other,
    };
    if db.auth == schemaic_core::connection::AuthMode::AzureCli
        && matches!(&result, Err(tiberius::error::Error::Server(t)) if t.code() == 18456)
    {
        crate::entra::forget();
    }
    if is_azure_sql(&db.host) {
        match &result {
            Err(tiberius::error::Error::Io { kind, .. })
                if *kind == std::io::ErrorKind::TimedOut =>
            {
                return Err(DbError::Connect(azure_timeout_text()));
            }
            // The other way a waking database answers: at once, with Msg 40613
            // "not currently available", rather than by holding the login.
            Err(e @ tiberius::error::Error::Server(t)) if t.code() == AZURE_UNAVAILABLE => {
                return Err(DbError::Connect(azure_unavailable_text(&ms_text(e))));
            }
            _ => {}
        }
    }
    result.map_err(|e| connect_err(&e))
}

async fn connect_with(cfg: tiberius::Config) -> tiberius::Result<MsClient> {
    let tcp = TcpStream::connect(cfg.get_addr()).await?;
    tcp.set_nodelay(true)?;
    tiberius::Client::connect(cfg, tcp.compat_write()).await
}

/// A failed connect, in the server's words when it gave any — a wrong
/// password is `Login failed for user 'x'.` (18456), a missing database is
/// `Cannot open database "x" requested by the login.` (4060).
fn connect_err(e: &tiberius::error::Error) -> DbError {
    DbError::Connect(ms_text(e))
}

/// A failed statement.
fn db_err(e: &tiberius::error::Error) -> DbError {
    DbError::Query(ms_text(e))
}

/// What SQL Server said, on one line: its message, then the error number and
/// line in the form its own tools print them, which is how anybody searches
/// for one. Anything that is not the server's is the driver's own sentence.
fn ms_text(e: &tiberius::error::Error) -> String {
    match e {
        tiberius::error::Error::Server(t) => server_message(t.message(), t.code(), t.line()),
        other => other.to_string(),
    }
}

/// [`ms_text`]'s server arm without the driver's type, so it can be asserted.
fn server_message(message: &str, code: u32, line: u32) -> String {
    let message = message.trim();
    if line > 0 {
        format!("{message} (Msg {code}, line {line})")
    } else {
        format!("{message} (Msg {code})")
    }
}

/// Lightweight reachability check bounded by `timeout`.
pub(crate) async fn ping(db: &Db, timeout: Duration) -> Result<(), DbError> {
    let check = async {
        let mut client = connect(db, None).await?;
        drain(&mut client, "SELECT 1").await
    };
    tokio::time::timeout(timeout, check)
        .await
        .map_err(|_| DbError::Connect("timed out".to_string()))?
}

/// The databases worth putting in the schema tree: online, open to this
/// login, and not one of the four system databases (`master`, `tempdb`,
/// `model`, `msdb` are ids 1–4, and on Azure SQL `master` is the only one).
///
/// `HAS_DBACCESS` is the catalogue saying whether *this* login may enter,
/// which is what expanding the node will need — the same reason PostgreSQL's
/// listing asks `has_database_privilege`.
///
/// **Never asked of a single-user database, and inside a `CASE`.** On one that
/// another session holds `SINGLE_USER`, `HAS_DBACCESS` takes about two
/// seconds to answer (2,174 ms against 150 ms, SQL Server 2022 CU27; 1,997 ms
/// re-measured for a plain login) — what an administrator's maintenance window
/// would cost every tree refresh — and a plain `AND user_access <> 1` beside
/// it does not stop it being evaluated, since T-SQL promises no order for
/// `AND`. `CASE` does. Whether another session holds it is not something a
/// plain login can see without that stall, so for such a login a single-user
/// database is left out, held or not. A `RESTRICTED_USER` one is asked like any
/// other: `HAS_DBACCESS` answers it at once (4 ms) and correctly — 0 for a
/// login that is not `db_owner`, which could not open it (measured on 2022).
///
/// **Nothing is asked of a login that may enter any database** —
/// `CONNECT ANY DATABASE`, which `sysadmin` holds — which therefore sees every
/// online database, restricted and single-user ones included: the listing used
/// to leave both out for everyone, so the database an administrator had just
/// put in maintenance vanished from their tree while opening it by name
/// worked; a single-user one is listed unread, and its *Read* says it is held
/// when another session holds it. **The
/// catalogue is read `WITH (READPAST)`**, and the short-circuit comes first,
/// both because a database another session is creating or
/// dropping holds its catalogue row under a lock that reading `sys.databases`
/// and `HAS_DBACCESS` both wait on (`LCK_M_S`, and one `SET LOCK_TIMEOUT` does
/// not govern): under six parallel create/drop loops the plain listing took up
/// to 5.1 s against the 5 s bound, the `READPAST` scan 0 ms, and with the
/// short-circuit `sa`'s listing 0–4 ms (SQL Server 2022 CU27). `READPAST` skips
/// only the rows under that lock — a database mid-create or mid-drop, which the
/// next refresh shows as it lands. A plain login still asks `HAS_DBACCESS`,
/// which still waits (to 7.8 s measured); [`listing_within`] is what bounds it.
///
/// **`user_access` comes back with the name** — `1` is `SINGLE_USER` — because
/// a listed single-user database must not be *read* unasked: the schema load
/// connects into every database it is given, and a connection into a
/// single-user one takes the slot the administrator just reserved
/// (`schema::unasked_reads`).
const DATABASE_LISTING: &str = "SELECT name, user_access FROM sys.databases WITH (READPAST) \
     WHERE database_id > 4 AND state = 0 \
       AND CASE WHEN HAS_PERMS_BY_NAME(NULL, NULL, 'CONNECT ANY DATABASE') = 1 THEN 1 \
                WHEN user_access = 1 THEN 0 \
                ELSE HAS_DBACCESS(name) END = 1 \
     ORDER BY name";

/// [`DATABASE_LISTING`] without the access check — what the tree shows when
/// that check stalls. A database this login cannot enter, a restricted or a
/// held single-user one included, then says so when it is expanded, which is
/// the answer it would have had before the check existed.
const DATABASE_LISTING_UNFILTERED: &str = "SELECT name, user_access FROM sys.databases WITH (READPAST) \
     WHERE database_id > 4 AND state = 0 \
     ORDER BY name";

/// The most of the listing's budget the access-checked **query** gets before
/// [`listing_within`] falls back; the rest is the unfiltered query's, which
/// needs a connection of its own (the stalled one is dropped mid-query). The
/// sign-in is not in it: it bounds `HAS_DBACCESS`, which is all it is for.
const ACCESS_CHECK_BUDGET: Duration = Duration::from_secs(3);

/// What the fallback's unfiltered query is allowed beyond its sign-in — a
/// `READPAST` scan of `sys.databases`, 0 ms locally, so this is round trips.
const FALLBACK_QUERY_ALLOWANCE: Duration = Duration::from_millis(500);

/// How long the access-checked query may run, given that the sign-in took
/// `connected` and `left` remains of the budget.
///
/// **The fallback's own sign-in is reserved first**, estimated as the one just
/// measured, with [`FALLBACK_QUERY_ALLOWANCE`] for its query. The check used
/// to get `min(3 s, left)` and the fallback whatever was over, which behind a
/// 1.5 s connect was half a second for a second sign-in and a query — so a
/// stalled check answered "timed out" where the listing before it had listed,
/// the arithmetic needing `2·connect + query < 2 s`. When even the reserve
/// does not fit, no fallback could finish, and the check is given everything:
/// a 3–5 s Entra sign-in (`a_slow_connect_is_not_charged_to_the_access_check`)
/// is answered by the filtered query or not at all.
fn access_check_share(connected: Duration, left: Duration) -> Duration {
    let reserve = connected + FALLBACK_QUERY_ALLOWANCE;
    if left > reserve {
        ACCESS_CHECK_BUDGET.min(left - reserve)
    } else {
        left
    }
}

/// List the user databases, sorted by name. Bounded by
/// [`crate::PING_TIMEOUT`], as on every engine.
pub(crate) async fn fetch_databases(db: &Db) -> Result<Vec<String>, DbError> {
    Ok(list_databases(db)
        .await?
        .into_iter()
        .map(|d| d.name)
        .collect())
}

/// [`fetch_databases`], each with whether it is in single-user mode.
pub(crate) async fn list_databases(db: &Db) -> Result<Vec<ListedDatabase>, DbError> {
    let rows_of = |mut client: MsClient, sql: &'static str| async move {
        let rows = query_rows(&mut client, sql).await?;
        Ok::<_, DbError>(rows.iter().map(|r| listed_database(r)).collect())
    };
    listing_within(
        crate::PING_TIMEOUT,
        connect(db, None),
        |client| rows_of(client, DATABASE_LISTING),
        || async {
            let client = connect(db, None).await?;
            rows_of(client, DATABASE_LISTING_UNFILTERED).await
        },
    )
    .await
}

/// One row of [`DATABASE_LISTING`] (or its unfiltered twin): the name, and
/// `user_access` — `1` is `SINGLE_USER`.
fn listed_database(row: &[Option<String>]) -> ListedDatabase {
    ListedDatabase {
        name: cell(row, 0),
        single_user: cell(row, 1).trim() == "1",
    }
}

/// Sign in with `connect`, then run `filtered` on that connection for
/// [`access_check_share`] — at most [`ACCESS_CHECK_BUDGET`], less what the
/// fallback's own sign-in will need; if it has not answered by then, run
/// `unfiltered` in what is left of `budget`. The whole is bounded by
/// `budget`, and **the share times the query alone** — a sign-in that takes
/// 3–5 s (Microsoft Entra's, through the Azure CLI) was charged to it, so the
/// filtered listing
/// was abandoned mid-connect and the fallback connected from scratch in what
/// was left, failing a listing the one 5 s bound before it answered. An
/// **error** from either the connect or `filtered` is the answer and is
/// returned as one — only a stall falls back, since a refused login would be
/// refused again.
async fn listing_within<C, R, F, FF, U, UF>(
    budget: Duration,
    connect: impl std::future::Future<Output = Result<C, DbError>>,
    filtered: F,
    unfiltered: U,
) -> Result<R, DbError>
where
    F: FnOnce(C) -> FF,
    FF: std::future::Future<Output = Result<R, DbError>>,
    U: FnOnce() -> UF,
    UF: std::future::Future<Output = Result<R, DbError>>,
{
    let start = tokio::time::Instant::now();
    let timed_out = || DbError::Connect("timed out".to_string());
    let left = |start: tokio::time::Instant| budget.saturating_sub(start.elapsed());
    let client = tokio::time::timeout(budget, connect)
        .await
        .map_err(|_| timed_out())??;
    match tokio::time::timeout(
        access_check_share(start.elapsed(), left(start)),
        filtered(client),
    )
    .await
    {
        Ok(answer) => answer,
        Err(_) => tokio::time::timeout(left(start), unfiltered())
            .await
            .map_err(|_| timed_out())?,
    }
}

// ── Running statements ───────────────────────────────────────────────────────

/// Connect and run one statement.
///
/// **`Enforce::ReadOnly` is a transaction that is always rolled back.** SQL
/// Server has no read-only session or transaction to ask for — nothing like
/// PostgreSQL's `default_transaction_read_only` or MySQL's `START TRANSACTION
/// READ ONLY` — so the statement runs inside `BEGIN TRANSACTION` and the
/// connection is closed without a commit, which the server rolls back. Its DDL
/// is transactional, so a table a `SELECT … INTO` made goes too.
///
/// What that does **not** undo, and why the text gate
/// (`sql::read_only_reason`) stays in front and refuses them by name: a
/// procedure or extended procedure's effect outside the database (`EXEC`,
/// `xp_cmdshell`), a sequence advanced by `NEXT VALUE FOR`, and anything sent
/// to another server (`OPENQUERY`, `OPENROWSET`). A write inside the
/// transaction is also briefly visible to a session reading uncommitted data.
/// A login granted only `SELECT` is the one guard that sees everything; the
/// connection form says so.
///
/// `AsJudged` needs nothing: the gate reads `"…"` as a name and T-SQL, under
/// either `QUOTED_IDENTIFIER` setting, ends the span where the gate does.
pub(crate) async fn fetch_query(
    db: &Db,
    database: Option<&str>,
    sql: &str,
    dest: &mut RowDest,
    cancel: CancellationToken,
    enforce: Option<crate::Enforce>,
) -> Result<ResultSet, DbError> {
    let mut client = connect(db, database).await?;
    if enforce == Some(crate::Enforce::ReadOnly) {
        drain(&mut client, "BEGIN TRANSACTION")
            .await
            .map_err(read_only_setup_failed)?;
    }
    // Dropped, not committed: the server rolls an open transaction back when
    // its connection closes.
    run_statement(&mut client, sql, dest, &cancel).await
}

/// The server refusing the statement that guards the session — about
/// Schemaic's statement, not the user's, so `Connect` rather than `Query`, as
/// `pg::read_only_setup_failed` explains.
fn read_only_setup_failed(e: DbError) -> DbError {
    DbError::Connect(format!("could not guard the session: {e}"))
}

/// Run several statements in order on ONE connection, so session state
/// (`USE`, `SET`, temporary tables, a transaction) carries across them. Stops
/// at the first failing statement; every one after it reports
/// [`DbError::Cancelled`]. A `USE` moves `scope`, as MySQL's does.
///
/// Under `Enforce::ReadOnly` the whole batch is one transaction that is never
/// committed — see [`fetch_query`].
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_batch(
    db: &Db,
    database: Option<&str>,
    stmts: &[String],
    row_cap: usize,
    cancel: CancellationToken,
    mut on_result: impl FnMut(usize, Result<ResultSet, DbError>),
    scope: Arc<std::sync::Mutex<Option<String>>>,
    enforce: Option<crate::Enforce>,
) {
    let opened = match connect(db, database).await {
        Ok(mut c) if enforce == Some(crate::Enforce::ReadOnly) => {
            match drain(&mut c, "BEGIN TRANSACTION").await {
                Ok(()) => Ok(c),
                Err(e) => Err(read_only_setup_failed(e)),
            }
        }
        other => other,
    };
    let mut client = match opened {
        Ok(c) => c,
        Err(e) => {
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
        let outcome = run_statement(&mut client, sql, &mut RowDest::Capped(row_cap), &cancel).await;
        if outcome.is_err() {
            stopped = true;
        }
        if outcome.is_ok()
            && schemaic_core::sql::leading_keyword(sql, MS).as_deref() == Some("USE")
            && let Ok(mut scope) = scope.lock()
        {
            *scope = schemaic_core::sql::use_target(sql, MS);
        }
        on_result(i, outcome);
    }
}

/// Validate `sql` **without executing it**, through
/// `sys.dm_exec_describe_first_result_set`, which compiles the batch —
/// syntax, and every table and column it names — and runs none of it.
///
/// **Not `SET NOEXEC ON`**, which was the first version of this: it compiles
/// without resolving names, so `SELECT * FROM nope` came back clean on SQL
/// Server 2022, and a missing table is the error this exists to show. The
/// describe answers it as error 208 in a row rather than raising it.
///
/// The statement goes in as a parameter. What cannot be described — a
/// temporary table the batch creates, dynamic SQL — is answered with a 115xx
/// error of the describe's own, which is not a fault in the statement and
/// passes ([`describe_error`]).
pub(crate) async fn prepare_check(
    db: &Db,
    database: Option<&str>,
    sql: &str,
) -> Result<(), DbError> {
    let stmt = sql.trim().trim_end_matches(';').trim_end();
    if stmt.is_empty() {
        return Ok(());
    }
    let mut client = connect(db, database).await?;
    const CHECK: &str = "SELECT error_number, error_message \
         FROM sys.dm_exec_describe_first_result_set(@P1, NULL, 0) \
         WHERE error_number IS NOT NULL";
    let rows = client
        .query(CHECK, &[&stmt])
        .await
        .map_err(|e| db_err(&e))?
        .into_first_result()
        .await
        .map_err(|e| db_err(&e))?;
    let errors: Vec<(u32, String)> = rows
        .iter()
        .map(|r| {
            (
                cell_text(r, 0).and_then(|n| n.parse().ok()).unwrap_or(0),
                cell_text(r, 1).unwrap_or_default(),
            )
        })
        .collect();
    match describe_error(&errors) {
        Some(e) => Err(DbError::Query(e)),
        None => Ok(()),
    }
}

/// The error a describe's rows report about the statement, if any: the first
/// that is not one of the describe's own 115xx answers ("uses a temp table",
/// "every code path results in an error", "could not be analyzed"), which
/// follow a real error or stand for "cannot say".
fn describe_error(rows: &[(u32, String)]) -> Option<String> {
    rows.iter()
        .find(|(n, _)| !(11500..11600).contains(n))
        .map(|(n, m)| server_message(m, *n, 0))
}

/// What the server says of this connection's transaction: `@@TRANCOUNT` —
/// how many `BEGIN TRAN`s are open, 0 for none — and `XACT_STATE()`, which is
/// -1 for a transaction that is open but can only be rolled back. `None` when
/// the connection could not answer.
pub(crate) async fn tx_state(client: &mut MsClient) -> Option<(i64, i64)> {
    let stream = client
        .simple_query("SELECT CAST(@@TRANCOUNT AS int), CAST(XACT_STATE() AS int)")
        .await
        .ok()?;
    let row = stream.into_row().await.ok()??;
    // `try_get`, never `get`: `get` unwraps, so a reply of the wrong shape — a
    // connection out of step answering an earlier request — panicked the
    // run task instead of answering `None`.
    let count: i32 = row.try_get(0).ok()??;
    let state: i32 = row.try_get(1).ok()??;
    Some((i64::from(count), i64::from(state)))
}

/// Does this connection answer its own requests? Asked of a pinned connection
/// after a Stop, since what an unacknowledged attention leaves on the wire
/// makes every later reply the answer to the request before it.
///
/// A query only this call could have sent — its own number — whose reply must
/// be that number, bounded by [`crate::CANCEL_TIMEOUT`]. Anything else, an
/// error or no answer in time, is `false`.
pub(crate) async fn answers_in_step(client: &mut MsClient) -> bool {
    // Far from any number a user's query plausibly returned last.
    static NEXT: std::sync::atomic::AtomicU64 =
        std::sync::atomic::AtomicU64::new(0x5C4E_1C00_0000_0000);
    let nonce = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let probe = async {
        let stream = client
            .simple_query(format!("SELECT CAST({nonce} AS bigint)"))
            .await
            .ok()?;
        let row = stream.into_row().await.ok()??;
        row.try_get::<i64, _>(0).ok()?
    };
    matches!(
        tokio::time::timeout(crate::CANCEL_TIMEOUT, probe).await,
        Ok(Some(n)) if n == nonce as i64
    )
}

/// This connection's `@@SPID` — the id Server Activity lists it under. `None`
/// if the server did not answer, which costs only the recognition.
pub(crate) async fn spid(client: &mut MsClient) -> Option<i64> {
    let stream = client
        .simple_query("SELECT CAST(@@SPID AS int)")
        .await
        .ok()?;
    let row = stream.into_row().await.ok()??;
    row.try_get::<i32, _>(0).ok()?.map(i64::from)
}

/// Run a statement for its side effect alone, reading its whole answer.
pub(crate) async fn drain(client: &mut MsClient, sql: &str) -> Result<(), DbError> {
    let mut stream = client.simple_query(sql).await.map_err(|e| db_err(&e))?;
    while let Some(item) = stream.next().await {
        item.map_err(|e| db_err(&e))?;
    }
    Ok(())
}

/// One statement on an open client, its rows into `dest`.
///
/// The columns come from [`describe`] when it can name them — types in full
/// (`nvarchar(50)`, `decimal(10,2)`) and each column's source for the grid's
/// editing — and otherwise from the wire's metadata, which has a base type and
/// no source.
///
/// **One result set per read**, as the other engines report: a second
/// announces itself with its own metadata, and the read goes on past it to
/// the stream's end, its rows dropped — the driver raises a batch's error only
/// there, and dropping the stream decided by timing whether the statements
/// after it ran. Past the row cap the same, unless the piece is one read
/// ([`schemaic_core::sql::drains_past_row_cap`]): a lone read's remaining rows
/// are left unread, while a write's `OUTPUT` rows are the write still running
/// — dropped, the connection went and the server rolled the write back — and a
/// procedure's are its later statements.
///
/// A cancel is TDS's own **attention**, on this connection: the driver's
/// `cancel_query` aborts the running batch and waits for the server to
/// acknowledge it, so nothing keeps running once Stop has answered.
///
/// **Every step is raced against Stop, and every Stop sends the attention.**
/// The describe compiles the statement, and a compile waits behind another
/// session's schema lock: awaited on its own, it left Stop doing nothing until
/// that lock was released, however long that was. `simple_query` does not
/// return until the statement's first result set — for an `UPDATE`, until it
/// has finished — and a Stop there used to leave the stopping to the
/// connection's close.
pub(crate) async fn run_statement(
    client: &mut MsClient,
    sql: &str,
    dest: &mut RowDest,
    cancel: &CancellationToken,
) -> Result<ResultSet, DbError> {
    let row_cap = dest.cap();
    let start = Instant::now();
    let described = {
        let step = describe(client, sql);
        tokio::select! {
            d = step => Some(d),
            _ = cancel.cancelled() => None,
        }
    };
    let Some(described) = described else {
        return Err(cancel_now(client).await);
    };
    let chunk_capacity = dest.chunk_capacity();
    // Statements after the first result's end are statements the server still
    // runs, and whose error is the batch's.
    let several = schemaic_core::sql::holds_several_statements(sql, MS);
    // Past the cap, read on unless the rest is surely a read's own rows: a
    // write's `OUTPUT` rows are the write still running, and a procedure's are
    // its later statements — drop the stream there and the server aborts them.
    let drain_at_cap = schemaic_core::sql::drains_past_row_cap(sql, MS);

    let mut grid: Option<ResultBuilder> = None;
    let mut truncated = false;
    let mut cancelled = false;
    let mut sets = 0usize;
    // Past what the grid shows: rows are read and dropped, to the stream's end.
    let mut draining = false;
    // A block rather than an early return on a Stop: the stream borrows the
    // client until the block ends, and the attention needs the client.
    let affected = 'read: {
        let opened = {
            let step = client.simple_query(sql);
            tokio::select! {
                r = step => Some(r),
                _ = cancel.cancelled() => None,
            }
        };
        let Some(opened) = opened else {
            cancelled = true;
            break 'read Ok(0);
        };
        let mut stream = match opened {
            Ok(stream) => stream,
            Err(e) => break 'read Err(batch_error(db_err(&e), several)),
        };
        loop {
            let next = tokio::select! {
                n = stream.next() => n,
                _ = cancel.cancelled() => {
                    cancelled = true;
                    break;
                }
            };
            let Some(item) = next else { break };
            let item = match item {
                Ok(item) => item,
                Err(e) => break 'read Err(batch_error(db_err(&e), several)),
            };
            match item {
                QueryItem::Metadata(meta) => {
                    sets += 1;
                    if sets > 1 {
                        // **Read on to the end, never stop here.** The driver
                        // raises a batch's error only when its stream ends, so
                        // a read that dropped the stream at the second result
                        // reported a failed batch as a success — and dropping
                        // it aborted the statements after, or not, depending
                        // on how much the first result returned.
                        if !draining {
                            tracing::warn!(
                                "batch returned more than one result set; reporting only the first"
                            );
                        }
                        draining = true;
                        continue;
                    }
                    let columns = result_columns(meta.columns(), described.as_deref());
                    grid = Some(ResultBuilder::with_capacity(columns, chunk_capacity));
                }
                QueryItem::Row(_) if draining => {}
                QueryItem::Row(row) => {
                    let Some(builder) = grid.as_mut() else {
                        continue;
                    };
                    if builder.row_count() >= row_cap {
                        truncated = true;
                        // A lone read's remaining rows are its own, and left
                        // unread; a write's, a module's and a batch's later
                        // statements are not.
                        if drain_at_cap {
                            draining = true;
                            continue;
                        }
                        break;
                    }
                    let cells: Vec<Value> = row.cells().map(|(_, d)| cell_value(d)).collect();
                    builder.push_row(&cells);
                    if dest.chunk_full(builder.row_count(), builder.text_bytes()) {
                        let next = dest.chunk_capacity();
                        dest.flush(builder, next).await?;
                    }
                }
            }
        }
        // The statement's own count is the last one: a trigger's statements
        // report theirs first, in the same batch.
        Ok(stream.rows_affected().last().copied().unwrap_or(0))
    };
    if cancelled {
        return Err(cancel_now(client).await);
    }
    let affected = affected?;
    let Some(mut builder) = grid else {
        return Ok(ResultSet::affected_rows(Vec::new(), affected)
            .with_elapsed(start.elapsed().as_millis()));
    };
    dest.flush(&mut builder, 0).await?;
    builder.set_truncated(truncated);
    builder.set_elapsed(start.elapsed().as_millis());
    Ok(builder.finish())
}

/// A batch's error, with what SQL Server did after it said when the batch held
/// more than one statement: a statement's error ends that statement, not the
/// batch, unless the error is one that aborts the batch — so a duplicate key
/// followed by an `UPDATE` reports the duplicate while the `UPDATE` commits
/// (measured on 2022). Only the server's own errors are annotated.
fn batch_error(e: DbError, several: bool) -> DbError {
    match e {
        DbError::Query(m) if several => DbError::Query(format!(
            "{m}\nSQL Server runs a batch on past a statement's error unless the error ends \
             the batch, so the statements after the failing one may have run."
        )),
        other => other,
    }
}

/// Stop what this connection is running — the attention, bounded by
/// [`crate::CANCEL_TIMEOUT`] — and answer [`DbError::Cancelled`].
///
/// A connection a dropped future left mid-write refuses the attention; the
/// server has only part of that request, runs none of it, and the caller
/// drops the connection either way.
async fn cancel_now(client: &mut MsClient) -> DbError {
    let _ = attention(client).await;
    DbError::Cancelled
}

/// Send the attention and answer whether the server **acknowledged** it.
///
/// Acknowledged, the driver has read the stream through the server's
/// `DONE_ATTN` and discarded what the aborted request left, so the next
/// request's replies are its own. Not acknowledged — refused, or past
/// [`crate::CANCEL_TIMEOUT`] — nothing read off this connection afterwards can
/// be trusted to answer the request that asked it.
async fn attention(client: &mut MsClient) -> bool {
    matches!(
        tokio::time::timeout(crate::CANCEL_TIMEOUT, client.cancel_query()).await,
        Ok(Ok(()))
    )
}

// ── Describing a result ──────────────────────────────────────────────────────

/// One column as `sys.dm_exec_describe_first_result_set` names it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Described {
    name: String,
    type_name: String,
    /// `(database, schema, table, column)` of the base column, when the
    /// column is one.
    source: Option<(String, String, String, String)>,
    /// Part of the key browse mode found for its table.
    in_key: bool,
    nullable: bool,
    identity: bool,
}

/// The first result set of `sql`, described without running it — or `None`
/// when the server cannot (a temporary table the batch makes itself, dynamic
/// SQL), in which case the wire's metadata is all there is.
///
/// **Browse mode** (the third argument, `1`) is what names each column's base
/// table and column. It also adds the key columns the select list left out,
/// flagged `is_hidden`; those are not in the result and are dropped here.
///
/// The statement goes in as a parameter, never spliced into text.
async fn describe(client: &mut MsClient, sql: &str) -> Option<Vec<Described>> {
    const DESCRIBE: &str = "SELECT name, system_type_name, source_database, source_schema, \
            source_table, source_column, is_part_of_unique_key, is_nullable, \
            is_identity_column \
         FROM sys.dm_exec_describe_first_result_set(@P1, NULL, 1) \
         WHERE is_hidden = 0 \
         ORDER BY column_ordinal";
    let stream = client.query(DESCRIBE, &[&sql]).await.ok()?;
    let rows = stream.into_first_result().await.ok()?;
    let out: Vec<Described> = rows
        .iter()
        .map(|r| {
            let text = |i: usize| -> Option<String> { cell_text(r, i) };
            let flag = |i: usize| text(i).as_deref() == Some("1");
            let source = match (text(2), text(3), text(4), text(5)) {
                (Some(d), Some(s), Some(t), Some(c)) => Some((d, s, t, c)),
                _ => None,
            };
            Described {
                name: text(0).unwrap_or_default(),
                type_name: text(1).unwrap_or_default(),
                source,
                in_key: flag(6),
                nullable: flag(7),
                identity: flag(8),
            }
        })
        .collect();
    (!out.is_empty()).then_some(out)
}

/// The columns a result set is read under: the description's when it agrees
/// with the wire about how many there are and what they are called, and the
/// wire's otherwise.
///
/// **Agreement is checked, not assumed.** The description is of the batch's
/// *first* result set, compiled a moment before it ran; a batch whose first
/// set comes from a branch the compiler could not see (`IF … SELECT … ELSE
/// SELECT …`) can describe one shape and return another, and every cell would
/// then sit under another column's name and type.
fn result_columns(wire: &[tiberius::Column], described: Option<&[Described]>) -> Vec<Column> {
    let agrees = described.is_some_and(|d| {
        d.len() == wire.len()
            && d.iter()
                .zip(wire)
                .all(|(d, w)| d.name.eq_ignore_ascii_case(w.name()))
    });
    match described {
        Some(d) if agrees => d.iter().map(described_column).collect(),
        _ => wire
            .iter()
            .map(|w| Column {
                name: w.name().to_string(),
                type_name: wire_type_name(w.column_type()).to_string(),
                origin: None,
            })
            .collect(),
    }
}

/// A described column as the grid's [`Column`]. The origin's key flags are
/// what browse mode knows: that the column is in *a* key of its table, which
/// is the question `analyze_edit` asks of it.
fn described_column(d: &Described) -> Column {
    let origin = d
        .source
        .as_ref()
        .map(|(database, schema, table, column)| ColumnOrigin {
            database: database.clone(),
            schema: Some(schema.clone()),
            table: table.clone(),
            column: column.clone(),
            flags: ColumnFlags {
                primary_key: false,
                unique_key: d.in_key,
                not_null: !d.nullable,
                auto_increment: d.identity,
                no_default: false,
            },
            binary: type_is_binary(&d.type_name),
            implicit_key: false,
        });
    Column {
        name: d.name.clone(),
        type_name: d.type_name.clone(),
        origin,
    }
}

/// A type name for a column the description could not name, from the wire's
/// base type — no length, since the wire's is in bytes and not the declared
/// one.
fn wire_type_name(t: ColumnType) -> &'static str {
    match t {
        ColumnType::Null => "",
        ColumnType::Bit | ColumnType::Bitn => "bit",
        ColumnType::Int1 => "tinyint",
        ColumnType::Int2 => "smallint",
        ColumnType::Int4 => "int",
        ColumnType::Int8 => "bigint",
        ColumnType::Intn => "int",
        ColumnType::Float4 => "real",
        ColumnType::Float8 | ColumnType::Floatn => "float",
        ColumnType::Money | ColumnType::Money4 => "money",
        ColumnType::Datetime4 => "smalldatetime",
        ColumnType::Datetime | ColumnType::Datetimen => "datetime",
        ColumnType::Daten => "date",
        ColumnType::Timen => "time",
        ColumnType::Datetime2 => "datetime2",
        ColumnType::DatetimeOffsetn => "datetimeoffset",
        ColumnType::Guid => "uniqueidentifier",
        ColumnType::Decimaln => "decimal",
        ColumnType::Numericn => "numeric",
        ColumnType::BigVarBin => "varbinary",
        ColumnType::BigBinary => "binary",
        ColumnType::Image => "image",
        ColumnType::BigVarChar => "varchar",
        ColumnType::BigChar => "char",
        ColumnType::NVarchar => "nvarchar",
        ColumnType::NChar => "nchar",
        ColumnType::Text => "text",
        ColumnType::NText => "ntext",
        ColumnType::Xml => "xml",
        ColumnType::Udt => "udt",
        ColumnType::SSVariant => "sql_variant",
    }
}

// ── Values ───────────────────────────────────────────────────────────────────

/// One TDS value as the grid stores it — the text SQL Server's own tools show.
///
/// Integers and floats keep their numeric variants; everything else is exact
/// text. **Nothing here is lossy**: a `decimal` is rendered from its scaled
/// integer, never through a float; a `real` from its own shortest
/// representation, not widened to `f64` first (which would show `0.1` as
/// `0.10000000149011612`); bytes as [`binary_display`], as on every engine.
fn cell_value(d: &ColumnData<'_>) -> Value {
    match d {
        ColumnData::U8(v) => v.map_or(Value::Null, |v| Value::Int(v.into())),
        ColumnData::I16(v) => v.map_or(Value::Null, |v| Value::Int(v.into())),
        ColumnData::I32(v) => v.map_or(Value::Null, |v| Value::Int(v.into())),
        ColumnData::I64(v) => v.map_or(Value::Null, Value::Int),
        ColumnData::F32(v) => v.map_or(Value::Null, real_value),
        ColumnData::F64(v) => v.map_or(Value::Null, Value::Float),
        ColumnData::Bit(v) => v.map_or(Value::Null, |b| Value::Int(b.into())),
        ColumnData::String(v) => v
            .as_ref()
            .map_or(Value::Null, |s| Value::Str(s.to_string())),
        ColumnData::Guid(v) => v.map_or(Value::Null, |g| Value::Str(guid_text(&g))),
        ColumnData::Binary(v) => v
            .as_ref()
            .map_or(Value::Null, |b| Value::Str(binary_display(b.len()))),
        ColumnData::Numeric(v) => v.map_or(Value::Null, |n| {
            Value::Str(decimal_text(n.value(), n.scale()))
        }),
        ColumnData::Xml(v) => v
            .as_ref()
            .map_or(Value::Null, |x| Value::Str(x.to_string())),
        ColumnData::DateTime(v) => v.map_or(Value::Null, |t| {
            Value::Str(datetime_text(t.days(), t.seconds_fragments()))
        }),
        ColumnData::SmallDateTime(v) => v.map_or(Value::Null, |t| {
            Value::Str(smalldatetime_text(t.days(), t.seconds_fragments()))
        }),
        ColumnData::Time(v) => v.map_or(Value::Null, |t| {
            Value::Str(time_text(t.increments(), t.scale()))
        }),
        ColumnData::Date(v) => v.map_or(Value::Null, |d| Value::Str(date_text(d.days()))),
        ColumnData::DateTime2(v) => v.map_or(Value::Null, |t| {
            Value::Str(datetime2_text(
                t.date().days(),
                t.time().increments(),
                t.time().scale(),
            ))
        }),
        ColumnData::DateTimeOffset(v) => v.map_or(Value::Null, |t| {
            let dt = t.datetime2();
            Value::Str(datetimeoffset_text(
                dt.date().days(),
                dt.time().increments(),
                dt.time().scale(),
                t.offset(),
            ))
        }),
    }
}

/// A `real` as the number it was written as: its own shortest decimal form,
/// read back as the `f64` of that text.
fn real_value(v: f32) -> Value {
    format!("{v}")
        .parse::<f64>()
        .map_or(Value::Float(v.into()), Value::Float)
}

/// A `uniqueidentifier` in the upper case SQL Server prints it in.
fn guid_text(g: &tiberius::Uuid) -> String {
    g.hyphenated().to_string().to_ascii_uppercase()
}

/// A `decimal`/`numeric`/`money` from its scaled integer: `12345` at scale 2
/// is `123.45`. A scale of 0 has no point — the driver's own formatting
/// writes `123.` — and a negative value keeps its sign on a zero integer part
/// (`-0.50`).
fn decimal_text(value: i128, scale: u8) -> String {
    let digits = value.unsigned_abs().to_string();
    let sign = if value < 0 { "-" } else { "" };
    let scale = usize::from(scale);
    if scale == 0 {
        return format!("{sign}{digits}");
    }
    let padded = format!("{digits:0>width$}", width = scale + 1);
    let (int, frac) = padded.split_at(padded.len() - scale);
    format!("{sign}{int}.{frac}")
}

/// Days since 1970-01-01 of SQL Server's two epochs.
const EPOCH_1900: i64 = -25_567;
const EPOCH_0001: i64 = -719_162;

/// `YYYY-MM-DD` for a count of days since 1970-01-01.
fn civil(days_since_1970: i64) -> String {
    let (y, m, d) = schemaic_core::date::civil_from_days(days_since_1970);
    format!("{y:04}-{m:02}-{d:02}")
}

/// `HH:MM:SS` for a count of seconds since midnight.
fn clock(seconds: u64) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

/// A `date`: days since 0001-01-01.
fn date_text(days: u32) -> String {
    civil(EPOCH_0001 + i64::from(days))
}

/// A `time(scale)`: increments of 10^-scale seconds since midnight, with as
/// many fraction digits as its scale — `time(7)` prints seven, `time(0)` none.
fn time_text(increments: u64, scale: u8) -> String {
    let per_second = 10u64.pow(u32::from(scale));
    let whole = clock(increments / per_second);
    if scale == 0 {
        whole
    } else {
        let frac = increments % per_second;
        format!("{whole}.{frac:0width$}", width = usize::from(scale))
    }
}

/// A `datetime2(scale)`.
fn datetime2_text(days: u32, increments: u64, scale: u8) -> String {
    format!("{} {}", date_text(days), time_text(increments, scale))
}

/// A `datetimeoffset(scale)`, in the offset it was stored with.
///
/// **The wire carries the instant in UTC**, and the offset beside it (MS-TDS
/// 2.2.5.5.1.8), so the local time is the UTC time moved by the offset — the
/// value `2026-01-01 10:00 +02:00` arrives as 08:00 and +120.
fn datetimeoffset_text(days: u32, increments: u64, scale: u8, offset_minutes: i16) -> String {
    let per_second = 10u64.pow(u32::from(scale));
    let per_day = 86_400 * per_second;
    let shift = i128::from(offset_minutes) * 60 * i128::from(per_second);
    let utc = i128::from(days) * i128::from(per_day) + i128::from(increments);
    let local = utc + shift;
    let (day, ticks) = (
        local.div_euclid(i128::from(per_day)),
        local.rem_euclid(i128::from(per_day)),
    );
    let sign = if offset_minutes < 0 { '-' } else { '+' };
    let off = offset_minutes.unsigned_abs();
    format!(
        "{} {} {sign}{:02}:{:02}",
        civil(EPOCH_0001 + day as i64),
        time_text(ticks as u64, scale),
        off / 60,
        off % 60
    )
}

/// A `datetime`: days since 1900-01-01 and three-hundredths of a second since
/// midnight, printed to the millisecond as SQL Server prints it
/// (`2026-09-27 12:50:53.730`) — so a value ending in `.997` still does.
fn datetime_text(days: i32, fragments: u32) -> String {
    let millis = (u64::from(fragments) * 1000 + 150) / 300;
    format!(
        "{} {}.{:03}",
        civil(EPOCH_1900 + i64::from(days)),
        clock(millis / 1000),
        millis % 1000
    )
}

/// A `smalldatetime`: days since 1900-01-01 and minutes since midnight.
fn smalldatetime_text(days: u16, minutes: u16) -> String {
    format!(
        "{} {}",
        civil(EPOCH_1900 + i64::from(days)),
        clock(u64::from(minutes) * 60)
    )
}

// ── Catalogue helpers ────────────────────────────────────────────────────────

/// Every row of a catalogue query, each cell as its text. For the bounded
/// reads — the database list, the schema — whose size is the catalogue's.
async fn query_rows(client: &mut MsClient, sql: &str) -> Result<Vec<Vec<Option<String>>>, DbError> {
    let rows = client
        .simple_query(sql)
        .await
        .map_err(|e| db_err(&e))?
        .into_first_result()
        .await
        .map_err(|e| db_err(&e))?;
    Ok(rows
        .iter()
        .map(|r| (0..r.len()).map(|i| cell_text(r, i)).collect())
        .collect())
}

/// A catalogue cell as text, or `None` for `NULL`.
fn cell_text(row: &tiberius::Row, i: usize) -> Option<String> {
    let (_, data) = row.cells().nth(i)?;
    match cell_value(data) {
        Value::Null => None,
        Value::Int(v) => Some(v.to_string()),
        Value::UInt(v) => Some(v.to_string()),
        Value::Float(v) => Some(v.to_string()),
        Value::Str(s) => Some(s),
    }
}

/// A cell of a catalogue row, empty for `NULL`.
fn cell(row: &[Option<String>], i: usize) -> String {
    row.get(i).cloned().flatten().unwrap_or_default()
}

// ── The schema ───────────────────────────────────────────────────────────────

/// The tables and views of every user schema, for the tree's first paint.
const TABLE_LISTING: &str = "SELECT s.name, o.name, \
            CASE o.type WHEN 'V' THEN 'VIEW' ELSE 'BASE TABLE' END \
     FROM sys.objects o JOIN sys.schemas s ON s.schema_id = o.schema_id \
     WHERE o.type IN ('U', 'V') AND o.is_ms_shipped = 0 \
     ORDER BY s.name, o.name";

/// The table list alone, without the catalogue reads a full fetch makes.
pub(crate) async fn fetch_table_list(db: &Db, database: &str) -> Result<DbSchema, DbError> {
    let mut client = connect(db, Some(database)).await?;
    let tables = query_rows(&mut client, TABLE_LISTING)
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

/// Every column of every user table and view: `(schema, table, column, type,
/// max_length, precision, scale, user-defined type, nullable, identity,
/// computed definition, persisted, default definition, collation (when not the
/// database's), description, is rowversion, the type's schema, identity seed,
/// identity increment, a typed `xml` column's schema collection's schema and
/// name, and whether it is `DOCUMENT`)`.
const COLUMN_LISTING: &str = "SELECT s.name, o.name, c.name, ty.name, \
            c.max_length, c.precision, c.scale, CAST(ty.is_user_defined AS int), \
            CAST(c.is_nullable AS int), CAST(c.is_identity AS int), \
            cc.definition, CAST(COALESCE(cc.is_persisted, 0) AS int), \
            dc.definition, \
            CASE WHEN c.collation_name <> CAST(DATABASEPROPERTYEX(DB_NAME(), 'Collation') \
                 AS sysname) THEN c.collation_name END, \
            CAST(ep.value AS nvarchar(4000)), \
            CAST(CASE WHEN ty.name = 'timestamp' THEN 1 ELSE 0 END AS int), \
            SCHEMA_NAME(ty.schema_id), \
            CAST(idc.seed_value AS nvarchar(40)), CAST(idc.increment_value AS nvarchar(40)), \
            SCHEMA_NAME(xc.schema_id), xc.name, CAST(c.is_xml_document AS int) \
     FROM sys.columns c \
     LEFT JOIN sys.xml_schema_collections xc \
            ON xc.xml_collection_id = c.xml_collection_id AND c.xml_collection_id > 0 \
     JOIN sys.objects o ON o.object_id = c.object_id \
     JOIN sys.schemas s ON s.schema_id = o.schema_id \
     JOIN sys.types ty ON ty.user_type_id = c.user_type_id \
     LEFT JOIN sys.identity_columns idc \
            ON idc.object_id = c.object_id AND idc.column_id = c.column_id \
     LEFT JOIN sys.computed_columns cc \
            ON cc.object_id = c.object_id AND cc.column_id = c.column_id \
     LEFT JOIN sys.default_constraints dc ON dc.object_id = c.default_object_id \
     LEFT JOIN sys.extended_properties ep \
            ON ep.class = 1 AND ep.major_id = c.object_id AND ep.minor_id = c.column_id \
           AND ep.name = 'MS_Description' \
     WHERE o.type IN ('U', 'V') AND o.is_ms_shipped = 0 AND c.graph_type IS NULL \
     ORDER BY s.name, o.name, c.column_id";

/// Every sequence, for a dump to create before the defaults that draw on it:
/// `(schema, name, base type, precision, alias type's schema, alias type's
/// name, start, increment, minimum, maximum, cycling, cached, cache size, last
/// used)` — the bounds as text, a sequence being possibly `decimal(38,0)`.
const SEQUENCE_LISTING: &str = "SELECT SCHEMA_NAME(s.schema_id), s.name, \
            TYPE_NAME(s.system_type_id), CAST(s.precision AS int), \
            CASE WHEN s.user_type_id <> s.system_type_id THEN SCHEMA_NAME(t.schema_id) END, \
            CASE WHEN s.user_type_id <> s.system_type_id THEN t.name END, \
            CAST(s.start_value AS nvarchar(40)), CAST(s.increment AS nvarchar(40)), \
            CAST(s.minimum_value AS nvarchar(40)), CAST(s.maximum_value AS nvarchar(40)), \
            CAST(s.is_cycling AS int), CAST(s.is_cached AS int), CAST(s.cache_size AS int), \
            CAST(s.last_used_value AS nvarchar(40)) \
     FROM sys.sequences s JOIN sys.types t ON t.user_type_id = s.user_type_id \
     ORDER BY 1, 2";

/// Every alias type (`CREATE TYPE … FROM`): `(schema, name, base type,
/// max_length, precision, scale, nullable)`. A table type and a CLR type are
/// other kinds of object, and not these.
const ALIAS_TYPE_LISTING: &str = "SELECT SCHEMA_NAME(t.schema_id), t.name, \
            TYPE_NAME(t.system_type_id), t.max_length, t.precision, t.scale, \
            CAST(t.is_nullable AS int) \
     FROM sys.types t \
     WHERE t.is_user_defined = 1 AND t.is_table_type = 0 AND t.is_assembly_type = 0 \
     ORDER BY 1, 2";

/// Every user XML schema collection, its schemas as the server prints them:
/// `(schema, name, definition)`. The `sys` one is in every database.
const XML_COLLECTION_LISTING: &str = "SELECT SCHEMA_NAME(x.schema_id), x.name, \
            CAST(XML_SCHEMA_NAMESPACE(SCHEMA_NAME(x.schema_id), x.name) AS nvarchar(max)) \
     FROM sys.xml_schema_collections x WHERE x.schema_id <> SCHEMA_ID('sys') \
     ORDER BY 1, 2";

/// Every synonym and its target, cut into its parts by the server: `(schema,
/// name, server, database, schema, object)`.
const SYNONYM_LISTING: &str = "SELECT SCHEMA_NAME(sn.schema_id), sn.name, \
            PARSENAME(sn.base_object_name, 4), PARSENAME(sn.base_object_name, 3), \
            PARSENAME(sn.base_object_name, 2), PARSENAME(sn.base_object_name, 1) \
     FROM sys.synonyms sn ORDER BY 1, 2";

/// The tables that are more than their columns — `TsqlTableKind`: `(schema,
/// table, node, edge, temporal type, memory-optimised, has edge constraints)`.
///
/// A graph table's internal columns (`graph_id_…`, `$node_id_…`, an edge's
/// `$from_id_…`/`$to_id_…`) are left out of [`COLUMN_LISTING`] by their
/// `graph_type`: they are the server's to add, `AS NODE`/`AS EDGE` restates
/// them, and as ordinary columns they made a plain table that refused the
/// original's rows (Msg 515). An edge constraint is an `EC` object, which a
/// server before 2019 simply has none of.
const TABLE_KIND_LISTING: &str = "SELECT s.name, t.name, CAST(t.is_node AS int), \
            CAST(t.is_edge AS int), CAST(t.temporal_type AS int), \
            CAST(t.is_memory_optimized AS int), \
            CAST(CASE WHEN EXISTS (SELECT 1 FROM sys.objects ec \
                                    WHERE ec.parent_object_id = t.object_id AND ec.type = 'EC') \
                 THEN 1 ELSE 0 END AS int) \
     FROM sys.tables t JOIN sys.schemas s ON s.schema_id = t.schema_id \
     WHERE t.is_ms_shipped = 0 \
       AND (t.is_node = 1 OR t.is_edge = 1 OR t.temporal_type <> 0 OR t.is_memory_optimized = 1)";

/// Every index's key columns, in key order: `(schema, table, index, unique,
/// primary key, column, descending, filter, type, has included columns,
/// constraint, the column's graph type)`. The primary key's index is renamed `PRIMARY`, as PostgreSQL's
/// is, so `IndexInfo::is_primary` and the DDL treat it the one way; its real
/// name is kept as the constraint's.
///
/// **A columnstore index has no key columns** — its columns are listed with
/// `key_ordinal` 0 and flagged as included — so it is read by its columns, in
/// their order, or a key-only join drops it and the table's DDL left it out
/// without the note every other index it cannot restate gets.
const INDEX_LISTING: &str = "SELECT s.name, t.name, \
            CASE WHEN i.is_primary_key = 1 THEN 'PRIMARY' ELSE i.name END, \
            CAST(i.is_unique AS int), CAST(i.is_primary_key AS int), \
            c.name, CAST(ic.is_descending_key AS int), i.filter_definition, i.type, \
            CAST(CASE WHEN EXISTS (SELECT 1 FROM sys.index_columns x \
                                    WHERE x.object_id = i.object_id \
                                      AND x.index_id = i.index_id \
                                      AND x.is_included_column = 1) \
                 THEN 1 ELSE 0 END AS int), \
            CASE WHEN i.is_primary_key = 1 OR i.is_unique_constraint = 1 THEN i.name END, \
            c.graph_type \
     FROM sys.indexes i \
     JOIN sys.tables t ON t.object_id = i.object_id \
     JOIN sys.schemas s ON s.schema_id = t.schema_id \
     JOIN sys.index_columns ic \
            ON ic.object_id = i.object_id AND ic.index_id = i.index_id \
           AND ((ic.is_included_column = 0 AND ic.key_ordinal > 0) OR i.type IN (5, 6)) \
     JOIN sys.columns c ON c.object_id = ic.object_id AND c.column_id = ic.column_id \
     WHERE t.is_ms_shipped = 0 AND i.index_id > 0 AND i.is_hypothetical = 0 \
     ORDER BY s.name, t.name, i.name, ic.key_ordinal, ic.index_column_id";

/// Every **indexed view's** indexes, in [`INDEX_LISTING`]'s shape — the
/// unique clustered index that materialises the view and any nonclustered
/// ones on it. Their own listing, over `sys.views`, so the table listing is
/// untouched: they land in `TsqlView::indexes`, which the view's edit plan
/// and script restate, since `ALTER VIEW` and `DROP VIEW` both take them.
const VIEW_INDEX_LISTING: &str = "SELECT s.name, v.name, i.name, \
            CAST(i.is_unique AS int), CAST(i.is_primary_key AS int), \
            c.name, CAST(ic.is_descending_key AS int), i.filter_definition, i.type, \
            CAST(CASE WHEN EXISTS (SELECT 1 FROM sys.index_columns x \
                                    WHERE x.object_id = i.object_id \
                                      AND x.index_id = i.index_id \
                                      AND x.is_included_column = 1) \
                 THEN 1 ELSE 0 END AS int), \
            NULL \
     FROM sys.indexes i \
     JOIN sys.views v ON v.object_id = i.object_id \
     JOIN sys.schemas s ON s.schema_id = v.schema_id \
     JOIN sys.index_columns ic \
            ON ic.object_id = i.object_id AND ic.index_id = i.index_id \
           AND ((ic.is_included_column = 0 AND ic.key_ordinal > 0) OR i.type IN (5, 6)) \
     JOIN sys.columns c ON c.object_id = ic.object_id AND c.column_id = ic.column_id \
     WHERE v.is_ms_shipped = 0 AND i.index_id > 0 AND i.is_hypothetical = 0 \
     ORDER BY s.name, v.name, i.name, ic.key_ordinal, ic.index_column_id";

/// Every foreign key's column pairs, in key order, with its actions:
/// `(schema, table, constraint, column, ref schema, ref table, ref column,
/// delete action, update action, disabled, not trusted)`.
const FK_LISTING: &str = "SELECT s.name, t.name, fk.name, pc.name, rs.name, rt.name, rc.name, \
            fk.delete_referential_action_desc, fk.update_referential_action_desc, \
            CAST(fk.is_disabled AS int), CAST(fk.is_not_trusted AS int) \
     FROM sys.foreign_keys fk \
     JOIN sys.tables t ON t.object_id = fk.parent_object_id \
     JOIN sys.schemas s ON s.schema_id = t.schema_id \
     JOIN sys.tables rt ON rt.object_id = fk.referenced_object_id \
     JOIN sys.schemas rs ON rs.schema_id = rt.schema_id \
     JOIN sys.foreign_key_columns k ON k.constraint_object_id = fk.object_id \
     JOIN sys.columns pc ON pc.object_id = k.parent_object_id \
                        AND pc.column_id = k.parent_column_id \
     JOIN sys.columns rc ON rc.object_id = k.referenced_object_id \
                        AND rc.column_id = k.referenced_column_id \
     WHERE t.is_ms_shipped = 0 \
     ORDER BY s.name, t.name, fk.name, k.constraint_column_id";

/// Every view's stored `CREATE VIEW`: `(schema, view, definition, ANSI_NULLS,
/// QUOTED_IDENTIFIER)`. `NULL` for one created `WITH ENCRYPTION`, which the
/// server will not show anyone; the last two are the settings it was created
/// under ([`module_settings`]).
const VIEW_LISTING: &str = "SELECT s.name, v.name, m.definition, \
            CAST(m.uses_ansi_nulls AS int), CAST(m.uses_quoted_identifier AS int) \
     FROM sys.views v \
     JOIN sys.schemas s ON s.schema_id = v.schema_id \
     LEFT JOIN sys.sql_modules m ON m.object_id = v.object_id \
     WHERE v.is_ms_shipped = 0";

/// Every table's `CHECK` constraints: `(schema, table, name, definition,
/// column-level, disabled, not trusted)`.
const CHECK_LISTING: &str = "SELECT s.name, t.name, ck.name, ck.definition, \
            CAST(CASE WHEN ck.parent_column_id > 0 THEN 1 ELSE 0 END AS int), \
            CAST(ck.is_disabled AS int), CAST(ck.is_not_trusted AS int) \
     FROM sys.check_constraints ck \
     JOIN sys.tables t ON t.object_id = ck.parent_object_id \
     JOIN sys.schemas s ON s.schema_id = t.schema_id \
     WHERE t.is_ms_shipped = 0 \
     ORDER BY s.name, t.name, ck.name";

/// Every DML trigger on a table or view, one row per event: `(schema,
/// table, trigger, instead of, disabled, event, definition, first, last,
/// ANSI_NULLS, QUOTED_IDENTIFIER, signed)` — first and last the event's
/// `sp_settriggerorder` rank, the next two the settings it was created under
/// ([`module_settings`]), the last whether it is signed.
const TRIGGER_LISTING: &str = "SELECT s.name, o.name, tr.name, \
            CAST(tr.is_instead_of_trigger AS int), CAST(tr.is_disabled AS int), \
            te.type_desc, m.definition, CAST(te.is_first AS int), CAST(te.is_last AS int), \
            CAST(m.uses_ansi_nulls AS int), CAST(m.uses_quoted_identifier AS int), \
            CAST(CASE WHEN EXISTS (SELECT 1 FROM sys.crypt_properties cp \
                                    WHERE cp.class = 1 AND cp.major_id = tr.object_id) \
                 THEN 1 ELSE 0 END AS int) \
     FROM sys.triggers tr \
     JOIN sys.objects o ON o.object_id = tr.parent_id \
     JOIN sys.schemas s ON s.schema_id = o.schema_id \
     JOIN sys.trigger_events te ON te.object_id = tr.object_id \
     LEFT JOIN sys.sql_modules m ON m.object_id = tr.object_id \
     WHERE tr.parent_class = 1 AND tr.is_ms_shipped = 0 \
     ORDER BY s.name, o.name, tr.name, te.type";

/// A trigger's stored text (`sys.sql_modules.definition`, `None` when the
/// server shows none) read into its action and SQL Server parts.
///
/// Through `ddl::tsql_trigger_parts`: the body after the header's `AS`, and
/// the options a `CREATE OR ALTER` must restate. A header the parts cannot
/// hold keeps the whole text as `verbatim` (and as the body, for whatever
/// displays it); no text at all — `WITH ENCRYPTION`, or no `VIEW DEFINITION`
/// — is `hidden`. Neither is rebuilt (`TriggerInfo::is_editable`).
fn tsql_trigger_reading(
    definition: Option<&str>,
) -> (
    schemaic_core::schema::TriggerAction,
    schemaic_core::schema::TsqlTrigger,
) {
    use schemaic_core::schema::{TriggerAction, TsqlModule, TsqlTrigger};
    let Some(def) = definition else {
        return (
            TriggerAction::Body(String::new()),
            TsqlTrigger {
                hidden: true,
                ..TsqlTrigger::default()
            },
        );
    };
    match schemaic_core::ddl::tsql_trigger_parts(def) {
        Some(p) => (
            TriggerAction::Body(p.body),
            TsqlTrigger {
                execute_as: p.execute_as,
                schemabinding: p.schemabinding,
                native_compilation: p.native_compilation,
                not_for_replication: p.not_for_replication,
                module: TsqlModule::with_header_comments(p.header_comments),
                ..TsqlTrigger::default()
            },
        ),
        None => (
            TriggerAction::Body(def.to_string()),
            TsqlTrigger {
                verbatim: Some(def.to_string()),
                ..TsqlTrigger::default()
            },
        ),
    }
}

/// The two creation-time settings `sys.sql_modules` keeps for a module —
/// `uses_ansi_nulls` and `uses_quoted_identifier`, the cells at `at` and
/// `at + 1` — onto `m`: off only where the server says `0`. A `NULL` (no
/// module row visible) leaves the ANSI default, which is what a rebuild would
/// have used anyway. See `schemaic_core::schema::TsqlModule::ansi_nulls_off`.
fn module_settings(r: &[Option<String>], at: usize, m: &mut schemaic_core::schema::TsqlModule) {
    let off = |i: usize| r.get(i).and_then(|c| c.as_deref()) == Some("0");
    m.ansi_nulls_off = off(at);
    m.quoted_identifier_off = off(at + 1);
}

/// What [`tsql_routine_reading`] fills in of a routine.
struct TsqlRoutineReading {
    arguments: String,
    returns: String,
    body: String,
    tsql: schemaic_core::schema::TsqlRoutine,
}

/// A routine's stored text (`None` when the server shows none) read into its
/// parameter list, return clause, body and SQL Server parts, through
/// `ddl::tsql_routine_parts` — with the catalogue's `arguments` and `returns`
/// (from `sys.parameters`) as the fallback for a routine that cannot be
/// rebuilt, where they are only for display.
///
/// **The text's parameter list wins when it reads**, because it is the only
/// place a T-SQL default lives: `sys.parameters.has_default_value` is 0 for
/// `@a int = 5` (measured on SQL Server 2022), so the catalogue's list, fed
/// back to `CREATE OR ALTER`, would silently drop every default.
fn tsql_routine_reading(
    definition: Option<&str>,
    arguments: String,
    returns: String,
) -> TsqlRoutineReading {
    use schemaic_core::schema::{TsqlModule, TsqlRoutine};
    let Some(def) = definition else {
        return TsqlRoutineReading {
            arguments,
            returns,
            body: String::new(),
            tsql: TsqlRoutine {
                hidden: true,
                ..TsqlRoutine::default()
            },
        };
    };
    match schemaic_core::ddl::tsql_routine_parts(def) {
        Some(p) => TsqlRoutineReading {
            arguments: p.arguments,
            returns: p.returns,
            body: p.body,
            tsql: TsqlRoutine {
                options: p.options,
                for_replication: p.for_replication,
                module: TsqlModule::with_header_comments(p.header_comments),
                ..TsqlRoutine::default()
            },
        },
        None => TsqlRoutineReading {
            arguments,
            returns,
            body: def.to_string(),
            tsql: TsqlRoutine {
                verbatim: Some(def.to_string()),
                ..TsqlRoutine::default()
            },
        },
    }
}

/// Every procedure and function written in T-SQL: `(schema, name, type,
/// definition, deterministic, description, ANSI_NULLS, QUOTED_IDENTIFIER,
/// signed)` — the two settings it was created under ([`module_settings`]),
/// and whether `ADD SIGNATURE` signed it. CLR routines have no module text
/// and are left out.
const ROUTINE_LISTING: &str = "SELECT s.name, o.name, o.type, m.definition, \
            CAST(COALESCE(OBJECTPROPERTY(o.object_id, 'IsDeterministic'), 0) AS int), \
            CAST(ep.value AS nvarchar(4000)), \
            CAST(m.uses_ansi_nulls AS int), CAST(m.uses_quoted_identifier AS int), \
            CAST(CASE WHEN EXISTS (SELECT 1 FROM sys.crypt_properties cp \
                                    WHERE cp.class = 1 AND cp.major_id = o.object_id) \
                 THEN 1 ELSE 0 END AS int) \
     FROM sys.objects o \
     JOIN sys.schemas s ON s.schema_id = o.schema_id \
     JOIN sys.sql_modules m ON m.object_id = o.object_id \
     LEFT JOIN sys.extended_properties ep \
            ON ep.class = 1 AND ep.major_id = o.object_id AND ep.minor_id = 0 \
           AND ep.name = 'MS_Description' \
     WHERE o.type IN ('P', 'FN', 'IF', 'TF') AND o.is_ms_shipped = 0 \
     ORDER BY s.name, o.name";

/// Every member of a numbered procedure group past the first: `(schema,
/// procedure, number, definition)`. The first is the procedure itself, in
/// `sys.sql_modules`; the rest live only here, and `DROP PROCEDURE` takes
/// them all (`schemaic_core::schema::TsqlRoutine::numbered`).
const NUMBERED_LISTING: &str = "SELECT s.name, o.name, np.procedure_number, np.definition \
     FROM sys.numbered_procedures np \
     JOIN sys.objects o ON o.object_id = np.object_id \
     JOIN sys.schemas s ON s.schema_id = o.schema_id \
     WHERE o.is_ms_shipped = 0 \
     ORDER BY s.name, o.name, np.procedure_number";

/// Every routine parameter, in order: `(schema, routine, parameter, type,
/// max_length, precision, scale, user-defined, output)`. Parameter 0 is a
/// scalar function's return type.
const PARAMETER_LISTING: &str = "SELECT s.name, o.name, p.name, ty.name, \
            p.max_length, p.precision, p.scale, CAST(ty.is_user_defined AS int), \
            CAST(p.is_output AS int), p.parameter_id, SCHEMA_NAME(ty.schema_id) \
     FROM sys.parameters p \
     JOIN sys.objects o ON o.object_id = p.object_id \
     JOIN sys.schemas s ON s.schema_id = o.schema_id \
     JOIN sys.types ty ON ty.user_type_id = p.user_type_id \
     WHERE o.type IN ('P', 'FN', 'IF', 'TF') AND o.is_ms_shipped = 0 \
     ORDER BY s.name, o.name, p.parameter_id";

/// Table and view descriptions: `(schema, object, description)`.
const TABLE_COMMENTS: &str = "SELECT s.name, o.name, CAST(ep.value AS nvarchar(4000)) \
     FROM sys.extended_properties ep \
     JOIN sys.objects o ON o.object_id = ep.major_id \
     JOIN sys.schemas s ON s.schema_id = o.schema_id \
     WHERE ep.class = 1 AND ep.minor_id = 0 AND ep.name = 'MS_Description' \
       AND o.type IN ('U', 'V')";

/// A type as it was declared, from `sys.types` and the column's own sizes —
/// `nvarchar(50)`, `varbinary(max)`, `decimal(10,2)`, `datetime2(7)`.
///
/// `max_length` is in **bytes**, so an `nchar`/`nvarchar` length is half of
/// it, and `-1` is `max`.
///
/// An alias type (`CREATE TYPE dbo.Name FROM nvarchar(50)`) is its own name,
/// **qualified with its schema**: its sizes are its base type's, and the alias
/// is what was written. `alias` carries that schema. Unqualified, `Name` in a
/// `SalesLT` table's DDL resolves through the login's default schema — which
/// is how AdventureWorks' `dbo.Name` came out as a type the script could not
/// find.
fn mssql_type_name(
    name: &str,
    max_length: i64,
    precision: i64,
    scale: i64,
    alias: Option<&str>,
) -> String {
    if let Some(schema) = alias {
        let q = |s: &str| schemaic_core::export::ident_sql(s, MS);
        return format!("{}.{}", q(schema), q(name));
    }
    let n = name.to_ascii_lowercase();
    let length = |bytes_per_char: i64| {
        if max_length == -1 {
            "max".to_string()
        } else {
            (max_length / bytes_per_char).to_string()
        }
    };
    match n.as_str() {
        "varchar" | "char" | "varbinary" | "binary" => format!("{n}({})", length(1)),
        "nvarchar" | "nchar" => format!("{n}({})", length(2)),
        "decimal" | "numeric" => format!("{n}({precision},{scale})"),
        "datetime2" | "datetimeoffset" | "time" => format!("{n}({scale})"),
        // `float` is `float(53)` unless it says otherwise; `float(24)` is a
        // `real`, which `sys.types` names as such.
        "float" if precision != 53 => format!("float({precision})"),
        _ => n,
    }
}

/// An identity's `(seed, increment)` from `sys.identity_columns`, kept only
/// when both are integer text — they are spliced into `IDENTITY(…)`, so a
/// value of any other shape is dropped, and the DDL falls back to `(1,1)` with
/// the note that says so, rather than carry it.
fn identity_spec(seed: &str, increment: &str) -> Option<(String, String)> {
    let integer = |s: &str| {
        let s = s.trim();
        let digits = s.strip_prefix('-').unwrap_or(s);
        (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())).then(|| s.to_string())
    };
    Some((integer(seed)?, integer(increment)?))
}

/// A stored expression without the parentheses SQL Server wraps it in:
/// `((0))` is `0`, `(getdate())` is `getdate()`, `([a]>(0))` is `[a]>(0)`.
///
/// Only pairs that enclose the **whole** text are removed — `(a)+(b)` keeps
/// both, since its first `(` closes before the end — and a parenthesis inside
/// a string or a quoted name is not one.
fn strip_outer_parens(expr: &str) -> String {
    let mut s = expr.trim();
    while s.starts_with('(') && encloses_whole(s) {
        s = s[1..s.len() - 1].trim();
    }
    s.to_string()
}

/// Does the `(` at the start of `s` close at its very end?
fn encloses_whole(s: &str) -> bool {
    let b = s.as_bytes();
    let mut depth = 0usize;
    let mut i = 0;
    while i < b.len() {
        if let Some(j) = schemaic_core::sql::skip_noncode(b, i, MS) {
            i = j;
            continue;
        }
        match b[i] {
            b'(' => depth += 1,
            b')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return i == b.len() - 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    false
}

/// What [`tsql_view_reading`] fills in of a view.
struct TsqlViewReading {
    /// The `SELECT` — `TableInfo::view_definition`.
    body: String,
    /// The stored statement — `TableInfo::create_sql`.
    create_sql: Option<String>,
    options: schemaic_core::schema::ViewOptions,
}

/// A view's stored text (`sys.sql_modules.definition`, `None` when the server
/// shows none) read into its `SELECT` and the header `ALTER VIEW` resets
/// unless it is restated — the column list and the attributes
/// ([`schemaic_core::schema::ViewOptions::attributes`]).
///
/// Through `ddl::tsql_view_parts`, the header walk the trigger and routine
/// readers share. A header it cannot read keeps the whole text, as the body
/// and as `create_sql`, and is `TsqlView::verbatim`; no text at all is
/// `hidden`. Neither is edited (`ddl::view_is_editable`).
fn tsql_view_reading(definition: Option<&str>) -> TsqlViewReading {
    use schemaic_core::schema::{TsqlModule, TsqlView, ViewOptions};
    let Some(def) = definition else {
        return TsqlViewReading {
            body: String::new(),
            create_sql: None,
            options: ViewOptions {
                tsql: TsqlView {
                    hidden: true,
                    ..TsqlView::default()
                },
                ..ViewOptions::default()
            },
        };
    };
    match schemaic_core::ddl::tsql_view_parts(def) {
        Some(p) => TsqlViewReading {
            body: p.body,
            create_sql: Some(def.to_string()),
            options: ViewOptions {
                column_list: p.column_list,
                attributes: p.attributes,
                tsql: TsqlView {
                    module: TsqlModule::with_header_comments(p.header_comments),
                    ..TsqlView::default()
                },
                ..ViewOptions::default()
            },
        },
        None => TsqlViewReading {
            body: def.trim().to_string(),
            create_sql: Some(def.to_string()),
            options: ViewOptions {
                tsql: TsqlView {
                    verbatim: true,
                    ..TsqlView::default()
                },
                ..ViewOptions::default()
            },
        },
    }
}

/// What a routine is, from its `sys.objects.type`, and what it returns, given
/// a scalar function's declared return type (its parameter 0).
///
/// **The column is `char(2)`**, so a procedure's one-letter `P` arrives
/// padded — `P ` — and is trimmed before it is compared.
fn routine_shape(
    object_type: &str,
    scalar_returns: Option<String>,
) -> (schemaic_core::schema::RoutineKind, String) {
    use schemaic_core::schema::RoutineKind;
    match object_type.trim() {
        "P" => (RoutineKind::Procedure, String::new()),
        "FN" => (RoutineKind::Function, scalar_returns.unwrap_or_default()),
        "IF" | "TF" => (RoutineKind::Function, "TABLE".to_string()),
        _ => (RoutineKind::Function, String::new()),
    }
}

/// A referential action as the model spells it: `NO_ACTION` is the default
/// and is left unwritten, the rest lose their underscore.
fn fk_action(desc: &str) -> Option<String> {
    match desc.trim() {
        "" | "NO_ACTION" => None,
        other => Some(other.replace('_', " ")),
    }
}

/// The whole schema of `database`.
///
/// A dozen catalogue reads, raced against `cancel` as a whole — the shape
/// `pg::fetch_schema` has, and for its reason: this is every column, index,
/// key, view, check, trigger and routine, and a large database takes a while.
pub(crate) async fn fetch_schema(
    db: &Db,
    database: &str,
    cancel: CancellationToken,
) -> Result<DbSchema, DbError> {
    let mut client = connect(db, Some(database)).await?;
    let outcome = {
        let collect = collect_schema(&mut client);
        tokio::select! {
            r = collect => Some(r),
            _ = cancel.cancelled() => None,
        }
    };
    match outcome {
        Some(r) => r,
        None => Err(cancel_now(&mut client).await),
    }
}

async fn collect_schema(client: &mut MsClient) -> Result<DbSchema, DbError> {
    use crate::{ColRow, IdxRow, assemble_schema, group_by};
    use schemaic_core::schema::{
        CheckInfo, ColumnInfo, IndexColumn, RoutineInfo, TriggerEnabled, TriggerEvent, TriggerInfo,
        TriggerLevel, TriggerTiming,
    };
    use std::collections::{HashMap, HashSet};

    let int = |r: &[Option<String>], i: usize| -> i64 { cell(r, i).parse().unwrap_or(0) };
    let flag = |r: &[Option<String>], i: usize| cell(r, i) == "1";

    let table_rows = query_rows(client, TABLE_LISTING).await?;
    let mut idx_all = query_rows(client, INDEX_LISTING).await?;
    // **A graph table's internal index is the server's, like its internal
    // columns**: one keyed on nothing but graph ids (`graph_type` 1, the
    // `GRAPH_UNIQUE_INDEX_…` that `AS NODE`/`AS EDGE` makes) goes; one that
    // names another graph column among its keys — `$from_id`/`$to_id`, say —
    // is kept but withheld as `lossy`, since its key names columns the model
    // leaves out.
    {
        type IdxKey = (String, String, String);
        let mut graph: HashMap<IdxKey, (bool, bool)> = HashMap::new();
        for r in &idx_all {
            let g = r.get(11).cloned().flatten();
            let e = graph
                .entry((cell(r, 0), cell(r, 1), cell(r, 2)))
                .or_insert((true, false));
            e.0 &= g.as_deref() == Some("1");
            e.1 |= g.is_some();
        }
        idx_all.retain(|r| {
            let (all_ids, _) = graph[&(cell(r, 0), cell(r, 1), cell(r, 2))];
            !all_ids
        });
        for r in &mut idx_all {
            let (_, any) = graph[&(cell(r, 0), cell(r, 1), cell(r, 2))];
            if any && r.len() > 11 {
                r[11] = Some("lossy".to_string());
            }
        }
    }
    let pk_set: HashSet<(String, String, String)> = idx_all
        .iter()
        .filter(|r| flag(r, 4))
        .map(|r| (cell(r, 0), cell(r, 1), cell(r, 5)))
        .collect();

    let col_rows: Vec<(String, ColRow)> = query_rows(client, COLUMN_LISTING)
        .await?
        .into_iter()
        .map(|r| {
            let (ns, t, c) = (cell(&r, 0), cell(&r, 1), cell(&r, 2));
            let generated = r.get(10).cloned().flatten().map(|d| strip_outer_parens(&d));
            // A typed `xml` column names its schema collection — dropped, the
            // copy took any XML at all, and the collection was never created.
            let type_name = match r.get(20).cloned().flatten() {
                Some(coll) => format!(
                    "xml({}{}.{})",
                    if flag(&r, 21) { "DOCUMENT " } else { "" },
                    schemaic_core::export::ident_sql(&cell(&r, 19), MS),
                    schemaic_core::export::ident_sql(&coll, MS)
                ),
                None => mssql_type_name(
                    &cell(&r, 3),
                    int(&r, 4),
                    int(&r, 5),
                    int(&r, 6),
                    flag(&r, 7).then(|| cell(&r, 16)).as_deref(),
                ),
            };
            let column = ColumnInfo {
                primary_key: pk_set.contains(&(ns.clone(), t.clone(), c.clone())),
                name: c,
                type_name,
                nullable: flag(&r, 8),
                // A computed column's value is its definition; a `rowversion`
                // is the server's too, and like an identity no `INSERT`
                // supplies it.
                auto_increment: flag(&r, 9),
                identity_always: flag(&r, 9) || flag(&r, 15),
                identity_spec: flag(&r, 9)
                    .then(|| identity_spec(r.get(17)?.as_deref()?, r.get(18)?.as_deref()?))
                    .flatten(),
                generated_stored: flag(&r, 11),
                generated,
                default: r.get(12).cloned().flatten().map(|d| strip_outer_parens(&d)),
                collation: r.get(13).cloned().flatten(),
                comment: r.get(14).cloned().flatten(),
                on_update: None,
                sqlite_autoincrement: false,
                invisible: false,
            };
            (ns, ColRow { table: t, column })
        })
        .collect();

    // An indexed view's indexes, in the same shape — folded onto the view
    // and moved to `TsqlView::indexes` below.
    let view_idx_all = query_rows(client, VIEW_INDEX_LISTING).await?;
    let idx_rows: Vec<(String, IdxRow)> = idx_all
        .iter()
        .chain(&view_idx_all)
        .map(|r| {
            let mut column = IndexColumn::plain(cell(r, 5));
            column.descending = flag(r, 6);
            // Rowstore indexes — clustered and nonclustered, types 1 and 2 —
            // are what the model can say; a columnstore, XML or spatial index
            // is not, and neither are included columns, which an edit would
            // drop. Clustering is `IndexInfo::clustered`: before it was
            // modelled, a clustered index other than the key's had to be
            // withheld, since recreating it plainly left the table a heap.
            let kind = int(r, 8);
            // `lossy` in the graph column: an index over a graph table's
            // internal columns, marked above.
            let lossy = kind > 2 || flag(r, 9) || cell(r, 11) == "lossy";
            (
                cell(r, 0),
                IdxRow {
                    table: cell(r, 1),
                    index: cell(r, 2),
                    unique: flag(r, 3),
                    column,
                    method: None,
                    predicate: r.get(7).cloned().flatten().map(|p| strip_outer_parens(&p)),
                    lossy,
                    create_sql: None,
                    clustered: Some(kind == 1),
                },
            )
        })
        .collect();
    let idx_constraints: HashMap<(String, String, String), String> = idx_all
        .iter()
        .filter_map(|r| {
            let name = r.get(10).cloned().flatten()?;
            Some(((cell(r, 0), cell(r, 1), cell(r, 2)), name))
        })
        .collect();

    let fk_all = query_rows(client, FK_LISTING).await?;
    let fk_rows: Vec<(String, crate::FkColRow)> = fk_all
        .iter()
        .map(|r| {
            (
                cell(r, 0),
                (
                    cell(r, 1),
                    cell(r, 2),
                    cell(r, 3),
                    Some(cell(r, 4)),
                    Some(cell(r, 5)),
                    Some(cell(r, 6)),
                ),
            )
        })
        .collect();
    // `(schema, table, constraint)` to `(on delete, on update, disabled, not
    // trusted)`.
    type FkRules = HashMap<(String, String, String), (Option<String>, Option<String>, bool, bool)>;
    let fk_rules: FkRules = fk_all
        .iter()
        .map(|r| {
            (
                (cell(r, 0), cell(r, 1), cell(r, 2)),
                (
                    fk_action(&cell(r, 7)),
                    fk_action(&cell(r, 8)),
                    flag(r, 9),
                    flag(r, 10),
                ),
            )
        })
        .collect();

    // Each view's stored text, read into its parts — see `tsql_view_reading`.
    let mut view_readings: HashMap<(String, String), TsqlViewReading> =
        query_rows(client, VIEW_LISTING)
            .await?
            .iter()
            .map(|r| {
                let mut v = tsql_view_reading(r.get(2).and_then(|d| d.as_deref()));
                module_settings(r, 3, &mut v.options.tsql.module);
                ((cell(r, 0), cell(r, 1)), v)
            })
            .collect();
    let view_rows: Vec<(String, (String, String))> = view_readings
        .iter()
        .map(|((ns, name), v)| (ns.clone(), (name.clone(), v.body.clone())))
        .collect();

    // Partition per schema before folding — `assemble_schema` keys on the
    // table name alone, so one call would merge `dbo.orders` and
    // `sales.orders`. `dbo` first, as PostgreSQL puts `public` first.
    let mut namespaces: Vec<String> = table_rows.iter().map(|r| cell(r, 0)).collect();
    namespaces.sort_by_key(|n| (n != "dbo", n.clone()));
    namespaces.dedup();
    let mut ns_tables = group_by(
        table_rows
            .iter()
            .map(|r| (cell(r, 0), (cell(r, 1), cell(r, 2)))),
    );
    let mut ns_cols = group_by(col_rows);
    let mut ns_fks = group_by(fk_rows);
    let mut ns_idx = group_by(idx_rows);
    let mut ns_views = group_by(view_rows);
    let mut tables = Vec::new();
    for ns in &namespaces {
        let schema = assemble_schema(
            Some(ns),
            &ns_tables.remove(ns).unwrap_or_default(),
            &ns_cols.remove(ns).unwrap_or_default(),
            &ns_fks.remove(ns).unwrap_or_default(),
            &ns_idx.remove(ns).unwrap_or_default(),
            &ns_views.remove(ns).unwrap_or_default(),
        );
        tables.extend(schema.tables);
    }

    let checks = query_rows(client, CHECK_LISTING).await?;
    let mut checks_by: HashMap<(String, String), Vec<CheckInfo>> = HashMap::new();
    for r in &checks {
        checks_by
            .entry((cell(r, 0), cell(r, 1)))
            .or_default()
            .push(CheckInfo {
                name: cell(r, 2),
                expression: strip_outer_parens(&cell(r, 3)),
                enforced: !flag(r, 5),
                validated: !flag(r, 6),
                inherited: false,
                column_level: flag(r, 4),
            });
    }

    // One row per event, folded into one trigger each.
    let mut triggers_by: HashMap<(String, String), Vec<TriggerInfo>> = HashMap::new();
    for r in query_rows(client, TRIGGER_LISTING).await? {
        let (ns, table, name) = (cell(&r, 0), cell(&r, 1), cell(&r, 2));
        let event = match cell(&r, 5).as_str() {
            "INSERT" => TriggerEvent::Insert,
            "UPDATE" => TriggerEvent::Update,
            "DELETE" => TriggerEvent::Delete,
            _ => continue,
        };
        // The event's `sp_settriggerorder` rank, which any `ALTER TRIGGER`
        // drops and `TriggerInfo::tsql_follow_ups` restates.
        let rank = if flag(&r, 7) {
            Some(schemaic_core::schema::FiringRank::First)
        } else if flag(&r, 8) {
            Some(schemaic_core::schema::FiringRank::Last)
        } else {
            None
        };
        let list = triggers_by.entry((ns.clone(), table.clone())).or_default();
        if let Some(t) = list.iter_mut().find(|t| t.name == name) {
            if !t.events.contains(&event) {
                t.events.push(event);
                // Declaration order, which is what the editor's toggles keep —
                // a catalogue order they re-sorted would be a phantom change.
                t.events.sort();
            }
            t.tsql.rank.extend(rank.map(|k| (event, k)));
            // Event order, which `TsqlTrigger::set_rank` keeps for the same
            // reason.
            t.tsql.rank.sort_by_key(|(e, _)| *e);
            continue;
        }
        let (action, mut tsql) = tsql_trigger_reading(r.get(6).and_then(|d| d.as_deref()));
        module_settings(&r, 9, &mut tsql.module);
        tsql.module.signed = flag(&r, 11);
        tsql.rank.extend(rank.map(|k| (event, k)));
        list.push(TriggerInfo {
            name,
            schema: Some(ns),
            table,
            timing: if flag(&r, 3) {
                TriggerTiming::InsteadOf
            } else {
                TriggerTiming::After
            },
            events: vec![event],
            update_columns: Vec::new(),
            // A T-SQL trigger fires once per statement, with the rows in
            // `inserted` and `deleted`.
            level: TriggerLevel::Statement,
            condition: None,
            // The body after the header's `AS` — see `tsql_trigger_reading`.
            action,
            definer: None,
            order: None,
            sql_mode: None,
            charset_client: None,
            collation_connection: None,
            old_table: None,
            new_table: None,
            enabled: if flag(&r, 4) {
                TriggerEnabled::Disabled
            } else {
                TriggerEnabled::Origin
            },
            constraint: false,
            tsql,
        });
    }

    let comments: HashMap<(String, String), String> = query_rows(client, TABLE_COMMENTS)
        .await?
        .into_iter()
        .filter_map(|r| Some(((cell(&r, 0), cell(&r, 1)), r.get(2).cloned().flatten()?)))
        .collect();
    let kinds: HashMap<(String, String), schemaic_core::schema::TsqlTableKind> =
        query_rows(client, TABLE_KIND_LISTING)
            .await?
            .into_iter()
            .map(|r| {
                (
                    (cell(&r, 0), cell(&r, 1)),
                    schemaic_core::schema::TsqlTableKind {
                        node: flag(&r, 2),
                        edge: flag(&r, 3),
                        temporal_type: u8::try_from(int(&r, 4)).unwrap_or(0),
                        memory_optimized: flag(&r, 5),
                        edge_constraints: flag(&r, 6),
                    },
                )
            })
            .collect();

    for t in &mut tables {
        let ns = t.schema.clone().unwrap_or_default();
        let key = (ns.clone(), t.name.clone());
        t.comment = comments.get(&key).cloned();
        t.tsql_kind = kinds.get(&key).cloned().unwrap_or_default();
        t.check_constraints = checks_by.remove(&key).unwrap_or_default();
        t.triggers = triggers_by.remove(&key).unwrap_or_default();
        if t.is_view
            && let Some(v) = view_readings.remove(&key)
        {
            t.create_sql = v.create_sql;
            let mut options = v.options;
            // An indexed view's indexes are the view's to restate, not a
            // table's for the grid and the designer to read.
            options.tsql.indexes = std::mem::take(&mut t.indexes);
            t.view_options = Some(options);
            // `dependent_ddl` stays empty on purpose: a view's only re-create
            // here is a rename, and an `INSTEAD OF` trigger's stored text
            // names the old view, so replaying it would address a view the
            // plan has just dropped. The risk line says the triggers go.
        }
        for ix in &mut t.indexes {
            ix.constraint = idx_constraints
                .get(&(ns.clone(), t.name.clone(), ix.name.clone()))
                .cloned();
        }
        for fk in &mut t.foreign_keys {
            if let Some((on_delete, on_update, disabled, untrusted)) =
                fk_rules.get(&(ns.clone(), t.name.clone(), fk.name.clone()))
            {
                fk.on_delete = on_delete.clone();
                fk.on_update = on_update.clone();
                // Restated as they are — see `ForeignKeyInfo::not_enforced`.
                fk.not_enforced = *disabled;
                fk.not_validated = *untrusted;
            }
        }
    }
    // Once every key has its actions: the rebuild puts another table's key
    // back as it was, `ON DELETE` and all.
    schemaic_core::schema::link_inbound_foreign_keys(&mut tables);

    // Routines, with their parameters spelled as their `CREATE` has them.
    let mut params: HashMap<(String, String), (Vec<String>, Option<String>)> = HashMap::new();
    for r in query_rows(client, PARAMETER_LISTING).await? {
        let ty = mssql_type_name(
            &cell(&r, 3),
            int(&r, 4),
            int(&r, 5),
            int(&r, 6),
            flag(&r, 7).then(|| cell(&r, 10)).as_deref(),
        );
        let entry = params.entry((cell(&r, 0), cell(&r, 1))).or_default();
        if cell(&r, 9) == "0" {
            entry.1 = Some(ty);
        } else {
            let out = if flag(&r, 8) { " OUTPUT" } else { "" };
            entry.0.push(format!("{} {ty}{out}", cell(&r, 2)));
        }
    }
    // A numbered group's members past the first, which the head's own text
    // gives no sign of.
    let mut numbered: HashMap<(String, String), Vec<(i32, String)>> = HashMap::new();
    for r in query_rows(client, NUMBERED_LISTING).await? {
        numbered
            .entry((cell(&r, 0), cell(&r, 1)))
            .or_default()
            .push((int(&r, 2) as i32, cell(&r, 3)));
    }
    let routines = query_rows(client, ROUTINE_LISTING)
        .await?
        .into_iter()
        .map(|r| {
            let (ns, name) = (cell(&r, 0), cell(&r, 1));
            let (args, returns) = params
                .remove(&(ns.clone(), name.clone()))
                .unwrap_or_default();
            let (kind, returns) = routine_shape(&cell(&r, 2), returns);
            // The parts of the stored text — see `tsql_routine_reading`.
            let mut read = tsql_routine_reading(
                r.get(3).and_then(|d| d.as_deref()),
                args.join(", "),
                returns,
            );
            read.tsql.numbered = numbered
                .remove(&(ns.clone(), name.clone()))
                .unwrap_or_default();
            module_settings(&r, 6, &mut read.tsql.module);
            read.tsql.module.signed = flag(&r, 8);
            Arc::new(RoutineInfo {
                name,
                schema: Some(ns),
                kind,
                arguments: read.arguments,
                returns: read.returns,
                language: "SQL".to_string(),
                body: read.body,
                deterministic: flag(&r, 4),
                comment: r.get(5).cloned().flatten(),
                tsql: read.tsql,
                ..Default::default()
            })
        })
        .collect();

    // The standalone objects a dump creates before the tables that name them.
    let mut tsql_objects = Vec::new();
    {
        use schemaic_core::schema::{TsqlObject, TsqlObjectKind};
        let obj = |r: &[Option<String>], kind| TsqlObject {
            schema: Some(cell(r, 0)),
            name: cell(r, 1),
            kind,
        };
        for r in query_rows(client, XML_COLLECTION_LISTING).await? {
            tsql_objects.push(obj(
                &r,
                TsqlObjectKind::XmlSchemaCollection {
                    definition: cell(&r, 2),
                },
            ));
        }
        for r in query_rows(client, ALIAS_TYPE_LISTING).await? {
            tsql_objects.push(obj(
                &r,
                TsqlObjectKind::AliasType {
                    base: mssql_type_name(&cell(&r, 2), int(&r, 3), int(&r, 4), int(&r, 5), None),
                    nullable: flag(&r, 6),
                },
            ));
        }
        for r in query_rows(client, SEQUENCE_LISTING).await? {
            let base = cell(&r, 2);
            let data_type = match r.get(5).cloned().flatten() {
                Some(alias) => format!(
                    "{}.{}",
                    schemaic_core::export::ident_sql(&cell(&r, 4), MS),
                    schemaic_core::export::ident_sql(&alias, MS)
                ),
                None if base == "decimal" || base == "numeric" => {
                    format!("{base}({},0)", int(&r, 3))
                }
                None => base,
            };
            tsql_objects.push(obj(
                &r,
                TsqlObjectKind::Sequence {
                    data_type,
                    start: cell(&r, 6),
                    increment: cell(&r, 7),
                    min: cell(&r, 8),
                    max: cell(&r, 9),
                    cycle: flag(&r, 10),
                    cache: flag(&r, 11).then(|| r.get(12).cloned().flatten()),
                    last_used: r.get(13).cloned().flatten(),
                },
            ));
        }
        for r in query_rows(client, SYNONYM_LISTING).await? {
            tsql_objects.push(obj(
                &r,
                TsqlObjectKind::Synonym {
                    target: (2..6).filter_map(|i| r.get(i).cloned().flatten()).collect(),
                },
            ));
        }
    }

    Ok(DbSchema {
        tables,
        routines,
        tsql_objects,
        flavour: schemaic_core::schema::ServerFlavour::Unknown,
        // Not needed, for PostgreSQL's reason: a foreign key's schema and a
        // view's names are part of the object, not the database's address.
        database: None,
        ..Default::default()
    })
}

// ── The entry points around a query ──────────────────────────────────────────

/// Up to `limit` rows of one table for the Live Monitor, schema-qualified
/// always, as the write paths name a table.
pub(crate) async fn fetch_table(
    db: &Db,
    database: &str,
    schema: Option<&str>,
    table: &str,
    order_by: Option<&[String]>,
    limit: usize,
    cancel: CancellationToken,
) -> Result<ResultSet, DbError> {
    let q = |n: &str| schemaic_core::export::ident_sql(n, MS);
    let name = match schema {
        Some(s) => format!("{}.{}", q(s), q(table)),
        None => q(table),
    };
    let rest = format!(
        "{name}{}",
        crate::order_by_clause(order_by, |c| schemaic_core::export::ident_sql(c, MS))
    );
    let sql = schemaic_core::sql::limited_select(MS, "*", &rest, limit);
    db.fetch_query(Some(database), &sql, limit, cancel).await
}

/// The statement's plan: one XML document per planned statement, as a result
/// under [`schemaic_core::plan::SHOWPLAN_COLUMN`], which
/// `QueryPlan::from_result` reads into the plan table.
///
/// `SET SHOWPLAN_XML ON` compiles the batch and **executes none of it** — the
/// estimated plan. `SET STATISTICS XML ON` runs it and appends the measured
/// plan after the statement's own results, which are read past; that runs
/// inside a transaction that is never committed, as `Enforce::ReadOnly` does
/// on [`fetch_query`], with the same limits (see [`explain_setup`]). Each `SET`
/// goes in a batch of its own, which `SHOWPLAN_XML` requires.
pub(crate) async fn explain(
    db: &Db,
    database: Option<&str>,
    sql: &str,
    analyze: bool,
    read_only: bool,
    cancel: CancellationToken,
) -> Result<ResultSet, DbError> {
    let setup = explain_setup(sql, analyze, read_only)?;
    let mut client = connect(db, database).await?;
    for step in setup {
        drain(&mut client, step)
            .await
            .map_err(|e| DbError::Connect(format!("could not set up the plan: {e}")))?;
    }
    let docs = plan_documents(&mut client, sql, &cancel).await?;
    // Dropped, not committed: a measured statement's work is rolled back.
    let column = Column {
        name: schemaic_core::plan::SHOWPLAN_COLUMN.to_string(),
        type_name: "xml".to_string(),
        origin: None,
    };
    Ok(ResultSet::from_rows(
        vec![column],
        docs.into_iter().map(|d| vec![Value::Str(d)]).collect(),
    ))
}

/// The batches [`explain`] sends before the statement.
///
/// The measured form opens a transaction first, so what the statement does is
/// rolled back with the connection — which does not undo a procedure's effect
/// outside the database, a sequence's advance or a remote write. The editor's
/// Analyze toggle is gated on `sql::contains_write`; **on a read-only
/// connection** the statement must also pass `sql::read_only_reason`, the gate
/// that refuses exactly those by name, since SQL Server has no read-only
/// transaction to refuse them for it. The estimated form runs nothing and
/// needs neither.
fn explain_setup(
    sql: &str,
    analyze: bool,
    read_only: bool,
) -> Result<&'static [&'static str], DbError> {
    if !analyze {
        return Ok(&["SET SHOWPLAN_XML ON"]);
    }
    if read_only {
        schemaic_core::sql::read_only_reason(sql, MS).map_err(DbError::Refused)?;
    }
    Ok(&["BEGIN TRANSACTION", "SET STATISTICS XML ON"])
}

/// Is a result set with these column names a plan document's?
fn is_showplan_set(columns: &[&str]) -> bool {
    matches!(columns, [only] if *only == schemaic_core::plan::SHOWPLAN_COLUMN)
}

/// Run `sql` and keep only the plan documents it answers with, raced against
/// Stop as [`run_statement`] is.
async fn plan_documents(
    client: &mut MsClient,
    sql: &str,
    cancel: &CancellationToken,
) -> Result<Vec<String>, DbError> {
    let mut docs = Vec::new();
    let mut cancelled = false;
    'read: {
        let opened = tokio::select! {
            r = client.simple_query(sql) => Some(r),
            _ = cancel.cancelled() => None,
        };
        let Some(opened) = opened else {
            cancelled = true;
            break 'read;
        };
        let mut stream = opened.map_err(|e| db_err(&e))?;
        let mut in_plan = false;
        loop {
            let next = tokio::select! {
                n = stream.next() => n,
                _ = cancel.cancelled() => {
                    cancelled = true;
                    break;
                }
            };
            let Some(item) = next else { break };
            match item.map_err(|e| db_err(&e))? {
                QueryItem::Metadata(meta) => {
                    let names: Vec<&str> = meta.columns().iter().map(|c| c.name()).collect();
                    in_plan = is_showplan_set(&names);
                }
                QueryItem::Row(row) if in_plan => {
                    if let Some(Value::Str(doc)) = row.cells().next().map(|(_, d)| cell_value(d)) {
                        docs.push(doc);
                    }
                }
                QueryItem::Row(_) => {}
            }
        }
    }
    if cancelled {
        return Err(cancel_now(client).await);
    }
    Ok(docs)
}

/// The exact row count, from `stats::count_rows_sql`'s `COUNT_BIG(*)`.
/// Cancelled on the server by an attention, like a query.
pub(crate) async fn count_rows(
    db: &Db,
    database: &str,
    sql: &str,
    cancel: CancellationToken,
) -> Result<u64, DbError> {
    let mut client = connect(db, Some(database)).await?;
    let rs = run_statement(&mut client, sql, &mut RowDest::Capped(1), &cancel).await?;
    let text = rs
        .cell(0, 0)
        .map(|c| c.text().to_string())
        .unwrap_or_default();
    text.parse()
        .map_err(|_| DbError::Query(format!("the count came back as {text:?}, not a number")))
}

/// Rows and sizes per table, from `sys.dm_db_partition_stats` — what SSMS's
/// *Disk usage by table* reads. The row count is the heap or clustered
/// index's (index 0 or 1), summed over partitions; pages are 8 KiB.
///
/// The counts are the storage engine's own and kept current by it, not a
/// sampled estimate like PostgreSQL's `reltuples` — but its documentation
/// still calls them approximate, and they are reported as estimates.
const TABLE_STATS: &str = "SELECT s.name, t.name, \
            SUM(CASE WHEN p.index_id IN (0, 1) THEN p.row_count ELSE 0 END), \
            SUM(CASE WHEN p.index_id IN (0, 1) THEN p.used_page_count ELSE 0 END) * 8192, \
            SUM(CASE WHEN p.index_id > 1 THEN p.used_page_count ELSE 0 END) * 8192, \
            SUM(p.reserved_page_count - p.used_page_count) * 8192, \
            CONVERT(varchar(23), t.create_date, 121), \
            CONVERT(varchar(23), t.modify_date, 121) \
     FROM sys.tables t \
     JOIN sys.schemas s ON s.schema_id = t.schema_id \
     JOIN sys.dm_db_partition_stats p ON p.object_id = t.object_id \
     WHERE t.is_ms_shipped = 0 \
     GROUP BY s.name, t.name, t.object_id, t.create_date, t.modify_date";

/// Each index's size and use, for the same properties surface.
const INDEX_STATS: &str = "SELECT s.name, t.name, i.name, \
            SUM(p.used_page_count) * 8192, \
            MAX(COALESCE(u.user_seeks, 0) + COALESCE(u.user_scans, 0) \
                + COALESCE(u.user_lookups, 0)), \
            CAST(i.is_primary_key AS int), CAST(i.is_unique AS int) \
     FROM sys.indexes i \
     JOIN sys.tables t ON t.object_id = i.object_id \
     JOIN sys.schemas s ON s.schema_id = t.schema_id \
     JOIN sys.dm_db_partition_stats p ON p.object_id = i.object_id AND p.index_id = i.index_id \
     LEFT JOIN sys.dm_db_index_usage_stats u \
            ON u.object_id = i.object_id AND u.index_id = i.index_id \
           AND u.database_id = DB_ID() \
     WHERE t.is_ms_shipped = 0 AND i.index_id > 0 AND i.name IS NOT NULL \
     GROUP BY s.name, t.name, i.name, i.is_primary_key, i.is_unique";

pub(crate) async fn fetch_table_stats(
    db: &Db,
    database: &str,
) -> Result<schemaic_core::stats::SchemaStats, DbError> {
    use schemaic_core::stats::{Freshness, IndexStats, SchemaStats, TableStats};
    let mut client = connect(db, Some(database)).await?;
    let num = |r: &[Option<String>], i: usize| -> Option<u64> {
        r.get(i)?.as_deref()?.split('.').next()?.parse().ok()
    };
    let mut by_table: std::collections::HashMap<(String, String), Vec<IndexStats>> =
        std::collections::HashMap::new();
    for r in query_rows(&mut client, INDEX_STATS).await? {
        by_table
            .entry((cell(&r, 0), cell(&r, 1)))
            .or_default()
            .push(IndexStats {
                name: cell(&r, 2),
                bytes: num(&r, 3),
                cardinality: None,
                // Reset by a restart, as PostgreSQL's `idx_scan` is by a stats
                // reset: a count since then, not since the index was made.
                scans: num(&r, 4),
                is_primary: cell(&r, 5) == "1",
                is_unique: cell(&r, 6) == "1",
            });
    }
    let tables = query_rows(&mut client, TABLE_STATS)
        .await?
        .into_iter()
        .map(|r| {
            let (ns, name) = (cell(&r, 0), cell(&r, 1));
            TableStats {
                indexes: by_table
                    .remove(&(ns.clone(), name.clone()))
                    .unwrap_or_default(),
                table: name,
                schema: Some(ns),
                rows: num(&r, 2),
                exact_rows: None,
                data_bytes: num(&r, 3),
                index_bytes: num(&r, 4),
                free_bytes: num(&r, 5),
                dead_rows: None,
                // `IDENT_CURRENT` is the last value handed out, not the next;
                // the next is one increment on, which this cannot know without
                // the increment, so the figure is left out rather than guessed.
                auto_increment: None,
                row_format: None,
                engine: None,
                created: r.get(6).cloned().flatten(),
                updated: r.get(7).cloned().flatten(),
                freshness: Freshness::Unknown,
            }
        })
        .collect();
    Ok(SchemaStats::new(tables))
}

/// Every user session, with the request it is running if any, projected in
/// [`schemaic_core::activity::MsSessionRow`]'s order.
///
/// `VIEW SERVER STATE` (on Azure SQL Database, `VIEW DATABASE STATE`) is what
/// shows other logins' sessions; without it the server returns only the
/// caller's own, which is a shorter list rather than an error.
///
/// `session_id <> @@SPID` drops the poll itself, as PostgreSQL's
/// `pg_backend_pid()` and MySQL's `CONNECTION_ID()` do: otherwise every refresh
/// listed this query as a running session, with a *Kill session* under it.
fn activity_sql(limit: usize) -> String {
    format!(
        "SELECT TOP ({limit}) s.session_id, s.login_name, s.host_name, \
                DB_NAME(s.database_id), r.status, s.open_transaction_count, \
                COALESCE(r.blocking_session_id, 0), LEFT(t.text, 1024), \
                DATEDIFF_BIG(millisecond, \
                    COALESCE(r.start_time, s.last_request_end_time, s.login_time), \
                    SYSDATETIME()) / 1000.0 \
         FROM sys.dm_exec_sessions s \
         LEFT JOIN sys.dm_exec_requests r ON r.session_id = s.session_id \
         OUTER APPLY sys.dm_exec_sql_text(r.sql_handle) t \
         WHERE s.is_user_process = 1 AND s.session_id <> @@SPID \
         ORDER BY s.session_id"
    )
}

pub(crate) async fn fetch_sessions(
    db: &Db,
) -> Result<Vec<schemaic_core::activity::SessionInfo>, DbError> {
    use schemaic_core::activity::{self, MsSessionRow};
    let mut client = connect(db, None).await?;
    let rows: Vec<MsSessionRow> =
        query_rows(&mut client, &activity_sql(activity::MAX_SESSIONS + 1))
            .await?
            .into_iter()
            .filter_map(|r| {
                Some(MsSessionRow {
                    session_id: cell(&r, 0).parse().ok()?,
                    login: cell(&r, 1),
                    host: r.get(2).cloned().flatten(),
                    database: r.get(3).cloned().flatten(),
                    request_status: r.get(4).cloned().flatten(),
                    open_transactions: cell(&r, 5).parse().unwrap_or(0),
                    blocking_session: cell(&r, 6).parse().unwrap_or(0),
                    sql: r.get(7).cloned().flatten(),
                    seconds: r.get(8).cloned().flatten().and_then(|s| s.parse().ok()),
                })
            })
            .collect();
    Ok(activity::from_mssql_rows(&rows))
}

/// `KILL <session>`, from a fresh connection. There is no statement that
/// cancels another session's request and leaves the session standing, so a
/// *Cancel query* is refused rather than performed as a `KILL` — see
/// `activity::supports_kill_kind`.
pub(crate) async fn kill_session(
    db: &Db,
    id: i64,
    kind: schemaic_core::activity::KillKind,
) -> Result<(), DbError> {
    if !schemaic_core::activity::supports_kill_kind(MS, kind) {
        return Err(DbError::Refused(
            "SQL Server cannot cancel another session's statement without ending the \
             session — use Kill session."
                .to_string(),
        ));
    }
    let mut client = connect(db, None).await?;
    // An integer the panel read from `session_id`, formatted as one: there is
    // no text of anybody's in this statement.
    drain(&mut client, &format!("KILL {id}")).await
}

/// Run a `.sql` file's statements, one connection for the whole file — the
/// second exception to one-connection-per-operation, as on every engine.
///
/// The splitter has already cut the file into batches at its `GO` lines and
/// kept each routine's body whole (`sql::scan_bounds`), so a statement here
/// is what SQL Server's own tools would send. A Stop is an attention on this
/// connection: the server aborts the running statement and rolls it back, so
/// a stopped one is not counted as run.
pub(crate) async fn run_script(
    db: &Db,
    database: &str,
    mut rx: tokio::sync::mpsc::Receiver<schemaic_core::script::Statement>,
    cancel: CancellationToken,
) -> (schemaic_core::script::ExecEnd, usize) {
    use schemaic_core::script::ExecEnd;
    let mut client = match connect(db, Some(database)).await {
        Ok(c) => c,
        Err(e) => return (ExecEnd::Connect(e.to_string()), 0),
    };
    let mut ran = 0usize;
    let end = loop {
        let next = tokio::select! {
            s = rx.recv() => s,
            _ = cancel.cancelled() => break ExecEnd::Cancelled,
        };
        let Some(st) = next else { break ExecEnd::Done };
        let outcome = {
            let step = drain(&mut client, &st.sql);
            tokio::select! {
                r = step => Some(r),
                _ = cancel.cancelled() => None,
            }
        };
        match outcome {
            Some(Ok(())) => ran += 1,
            Some(Err(e)) => {
                break ExecEnd::Failed {
                    message: e.to_string(),
                    sql: st.sql,
                    line: st.line,
                };
            }
            None => {
                cancel_now(&mut client).await;
                break ExecEnd::Cancelled;
            }
        }
    };
    (end, ran)
}

// ── Not written yet ──────────────────────────────────────────────────────────
//
// Each answers the whole interface's name (`ENGINE_ENTRY_POINTS`), and each
// refuses: no path in the app reaches them for SQL Server — the capability
// gates above them answer no — and one that does is told so in a sentence.

pub(crate) async fn run_server_ddl(
    _db: &Db,
    _avoid: Option<&str>,
    _stmts: &[String],
    _cancel: CancellationToken,
) -> Result<(), crate::DdlError> {
    Err(crate::DdlError {
        message: not_yet("Creating or dropping a database").to_string(),
        at: 0,
        applied: 0,
    })
}

/// The SQL Server half of [`Db::import_rows`]: every row in one transaction,
/// in `INSERT … VALUES` batches of up to [`schemaic_core::import::INSERT_BATCH_ROWS`]
/// rows, each required to insert exactly its own rows.
///
/// **Literal statements, from the builder every engine shares**
/// (`import::build_insert`), not bound ones as [`commit_writes`] uses: a
/// bound batch would spend a parameter per cell, and SQL Server takes at most
/// 2,100 per request — 500 rows of five columns is past it. The literal form
/// has no such ceiling, and its 500 rows are under T-SQL's 1,000-row limit on
/// a `VALUES` list. Quoting is `export::sql_literal`'s, `N'…'` for text.
///
/// **An empty field bound for a number or a date is refused before its batch
/// runs** ([`import_blank_refusal`]), and the whole import rolls back: SQL
/// Server would store `0` or `1900-01-01` and report success. An identity
/// column in the list runs the import under `IDENTITY_INSERT`.
///
/// **Stop is `commit_writes`' rule**: checked between batches, and raced
/// against the one in flight, with the rollback sent only after the server
/// acknowledged the attention — otherwise the connection's close is what
/// rolls it back, and the error says it is not known to have.
pub(crate) async fn import_rows(
    db: &Db,
    target: crate::ImportTarget<'_>,
    rows: crate::RowSource<'_>,
    cancel: CancellationToken,
) -> Result<u64, DbError> {
    if cancel.is_cancelled() {
        return Err(DbError::Cancelled);
    }
    let mut client = connect(db, Some(target.database)).await?;
    let facts = column_facts(&mut client, target.database, target.schema, target.table).await?;
    let cols: Vec<&str> = target.columns.iter().map(String::as_str).collect();
    let identity =
        import_sets_identity(target.columns, &facts).then(|| qname(target.schema, target.table));
    drain(&mut client, "BEGIN TRANSACTION").await?;
    if let Some(t) = &identity
        && let Err(e) = drain(&mut client, &format!("SET IDENTITY_INSERT {t} ON")).await
    {
        return Err(failed(&mut client, err_text(e)).await);
    }
    let mut total = 0u64;
    // The row the byte ceiling held back from the previous batch — see
    // `crate::next_batch`.
    let mut held: Option<Vec<Value>> = None;
    loop {
        if cancel.is_cancelled() {
            return Err(cancelled_import(rollback(&mut client).await));
        }
        let batch = match crate::next_batch_off_executor(rows, &mut held) {
            Ok(Some(b)) => b,
            Ok(None) => break,
            Err(e) => return Err(failed(&mut client, err_text(e)).await),
        };
        // **Asked again once the batch is in hand**: reading it is where an
        // import spends its time, so it is where Stop lands. Left to the race
        // below, the attention went to a request barely sent, the server's
        // acknowledgement was not seen, and the rollback — which nothing
        // prevented — was reported as not known to have happened.
        if cancel.is_cancelled() {
            return Err(cancelled_import(rollback(&mut client).await));
        }
        if let Some(msg) = import_blank_refusal(target.columns, &facts, &batch, total) {
            let undone = rollback(&mut client).await;
            return Err(DbError::Refused(format!("{msg}{}", undone.note())));
        }
        let check = import_code_page_check(target.columns, &facts, &batch, total);
        let refusal = match check.refusal {
            Some(msg) => Some(msg),
            None => match server_code_page_refusal(&mut client, &check.ask_server).await {
                Ok(r) => r,
                Err(e) => return Err(failed(&mut client, err_text(e)).await),
            },
        };
        if let Some(msg) = refusal {
            let undone = rollback(&mut client).await;
            return Err(DbError::Refused(format!("{msg}{}", undone.note())));
        }
        let batch = language_safe_batch(target.columns, &facts, batch);
        let Some(sql) = schemaic_core::import::build_insert(
            target.database,
            target.schema,
            target.table,
            &cols,
            &batch,
            MS,
        ) else {
            continue;
        };
        let bound = Bound {
            sql,
            params: Vec::new(),
        };
        let affected = match execute_counted(&mut client, &bound, &cancel).await {
            Ok(Ran::Counted(n)) => n,
            Ok(Ran::Stopped { in_step }) => {
                let undone = if in_step {
                    rollback(&mut client).await
                } else {
                    Rollback::Unknown
                };
                return Err(cancelled_import(undone));
            }
            Err(e) => return Err(failed(&mut client, err_text(e)).await),
        };
        if affected != batch.len() as u64 {
            let msg = format!("a batch of {} rows inserted {affected}", batch.len());
            return Err(failed(&mut client, msg).await);
        }
        total += affected;
    }
    if let Some(t) = &identity
        && let Err(e) = drain(&mut client, &format!("SET IDENTITY_INSERT {t} OFF")).await
    {
        return Err(failed(&mut client, err_text(e)).await);
    }
    if let Err(e) = drain(&mut client, "COMMIT TRANSACTION").await {
        return Err(failed(&mut client, err_text(e)).await);
    }
    Ok(total)
}

// ── Schema changes ───────────────────────────────────────────────────────────

/// The SQL Server half of [`Db::run_ddl`]: one transaction around the whole
/// plan. T-SQL's `CREATE TABLE`, `CREATE INDEX` and `DROP` are transactional,
/// as PostgreSQL's are, so a failure anywhere leaves the database as it was —
/// which is why [`crate::DdlError::applied`] is always 0 on this path.
///
/// Stop sends the attention, and the rollback after it is sent only when the
/// server acknowledged — `commit_writes`' rule, for its reason: on a stream at
/// an unknown point a `ROLLBACK` can read another request's answer as its own.
/// Unacknowledged, the connection's close is what rolls the plan back.
pub(crate) async fn run_ddl(
    db: &Db,
    database: &str,
    stmts: &[String],
    cancel: CancellationToken,
) -> Result<(), crate::DdlError> {
    let fail = |at: usize, message: String| crate::DdlError {
        message,
        at,
        applied: 0,
    };
    let mut client = connect(db, Some(database))
        .await
        .map_err(|e| fail(0, err_text(e)))?;
    drain(&mut client, "BEGIN TRANSACTION")
        .await
        .map_err(|e| fail(0, err_text(e)))?;
    // Best-effort, as on the other engines: a plan that waits behind another
    // session's lock gives up rather than hanging the modal.
    let _ = drain(&mut client, &crate::lock_wait_sql(crate::Engine::MsSql)).await;
    for (i, sql) in stmts.iter().enumerate() {
        let step = {
            let run = drain(&mut client, sql);
            tokio::select! {
                r = run => Some(r),
                _ = cancel.cancelled() => None,
            }
        };
        match step {
            Some(Ok(())) => {}
            Some(Err(e)) => {
                let _ = rollback(&mut client).await;
                return Err(fail(i, err_text(e)));
            }
            None => {
                if attention(&mut client).await {
                    let _ = rollback(&mut client).await;
                }
                return Err(fail(i, "cancelled".to_string()));
            }
        }
    }
    if let Err(e) = drain(&mut client, "COMMIT TRANSACTION").await {
        let _ = rollback(&mut client).await;
        return Err(fail(stmts.len().saturating_sub(1), err_text(e)));
    }
    Ok(())
}

// ── Write-back ───────────────────────────────────────────────────────────────
//
// The grid's edits, re-reads and binary cells. The statements are built here,
// as each engine builds its own; the order they run in and the verdict on each
// are `core::model`'s (`GridWrite::plan`, `one_row_verdict`), shared with the
// other three engines.

/// `[name]`, through the one quoter.
fn ident(name: &str) -> String {
    schemaic_core::export::ident_sql(name, MS)
}

/// `[schema].[table]`, qualified whenever the schema is known — for
/// `pg_qname`'s reason: the statement is never shown, so it must not depend on
/// the login's default schema.
fn qname(schema: Option<&str>, table: &str) -> String {
    match schema {
        Some(s) => format!("{}.{}", ident(s), ident(table)),
        None => ident(table),
    }
}

/// `[database].[schema].[table]` — [`qname`] with the database in front
/// whenever it is known, for every statement the grid's write, re-read and
/// blob read build.
///
/// **A pinned session's connection is in its own database, not the edit's.**
/// A Manual tab pinned to A that read `B.dbo.t` and edited a row of it wrote
/// the same-named `dbo.t` in A, and the re-read — the same two-part name on
/// the same connection — read A's row back and showed the typed value over
/// the table the user never touched. The 1-row net checks a count, not an
/// identity, so nothing else could catch it. T-SQL takes a three-part name in
/// every statement here (`SET IDENTITY_INSERT` included), and inside the
/// pinned transaction.
fn qname3(database: &str, schema: Option<&str>, table: &str) -> String {
    if database.is_empty() {
        return qname(schema, table);
    }
    // A three-part name needs its schema part; `db..t` would take the
    // default schema, which is what a missing one means anyway.
    format!(
        "{}.{}.{}",
        ident(database),
        schema.map(ident).unwrap_or_default(),
        ident(table)
    )
}

/// A statement and the values its `@P1`, `@P2`… placeholders stand for, in
/// placeholder order.
#[derive(Debug, Default, PartialEq)]
struct Bound {
    sql: String,
    params: Vec<ColumnData<'static>>,
}

/// Add `v` as the next parameter and answer its placeholder.
fn hole(params: &mut Vec<ColumnData<'static>>, v: ColumnData<'static>) -> String {
    params.push(v);
    format!("@P{}", params.len())
}

/// Text as `col` should receive it: a `datetime` or `smalldatetime` in the
/// form every language reads alike ([`language_safe_datetime`], core's, which
/// the export and the grid's filter write too), anything else as it is.
fn column_text(facts: &[ColumnFacts], col: &str, text: &str) -> String {
    match fact(facts, col) {
        Some(f) if schemaic_core::export::reads_dates_by_language(&f.base_type, MS) => {
            language_safe_datetime(text).unwrap_or_else(|| text.to_string())
        }
        _ => text.to_string(),
    }
}

/// A key value as a parameter.
///
/// **A float goes as its text**, the digits the grid shows. A `real` column
/// compared with an `f64` is widened to it, and `0.1` stored as a `real` is not
/// `0.1` as an `f64` — the key would match nothing. Text is converted to the
/// *column's* type instead (a string has the lowest precedence), which is the
/// value the grid read. Everything else textual is already text: a `decimal`, a
/// date, a `uniqueidentifier` all arrive as the server's own rendering of them
/// and convert back exactly — **except a `datetime` or `smalldatetime`**,
/// whose rendering converts back under the login's date order, and so goes
/// through [`column_text`].
fn key_param(col: &str, v: &Value, facts: &[ColumnFacts]) -> ColumnData<'static> {
    let text = |s: String| ColumnData::String(Some(s.into()));
    match v {
        Value::Int(i) => ColumnData::I64(Some(*i)),
        Value::UInt(u) => {
            i64::try_from(*u).map_or_else(|_| text(u.to_string()), |i| ColumnData::I64(Some(i)))
        }
        Value::Float(f) => text(f.to_string()),
        Value::Str(s) => text(column_text(facts, col, s)),
        // Never bound: `where_key` writes `IS NULL` for one.
        Value::Null => ColumnData::String(None),
    }
}

/// A staged cell as a parameter, or `None` for NULL — which is written as the
/// literal, since a TDS parameter is typed and a NULL of one type is not a
/// NULL of every other. Text goes through [`column_text`], as a key does.
fn cell_param(col: &str, v: &CellEdit, facts: &[ColumnFacts]) -> Option<ColumnData<'static>> {
    match v {
        CellEdit::Null => None,
        CellEdit::Text(t) => Some(ColumnData::String(Some(column_text(facts, col, t).into()))),
        CellEdit::Bytes(b) => Some(ColumnData::Binary(Some(b.to_vec().into()))),
    }
}

/// `[c] = @Pn` … ` AND ` …, with a NULL key value compared as `IS NULL`: T-SQL
/// has no null-safe equality before 2022's `IS NOT DISTINCT FROM`, and `= NULL`
/// is never true.
fn where_key(
    key: &[(String, Value)],
    facts: &[ColumnFacts],
    params: &mut Vec<ColumnData<'static>>,
) -> String {
    key.iter()
        .map(|(col, v)| {
            if v.is_null() {
                format!("{} IS NULL", ident(col))
            } else {
                format!(
                    "{} = {}",
                    ident(col),
                    hole(params, key_param(col, v, facts))
                )
            }
        })
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// The value side of one staged cell: its placeholder, or `NULL`.
fn cell_sql(
    col: &str,
    v: &CellEdit,
    facts: &[ColumnFacts],
    params: &mut Vec<ColumnData<'static>>,
) -> String {
    match cell_param(col, v, facts) {
        Some(p) => hole(params, p),
        None => "NULL".to_string(),
    }
}

/// The statement for one step of a [`GridWrite`]. The `SET` values bind before
/// the `WHERE`'s, the order they appear in the text. `facts` are the table's
/// columns, for the types whose text is rewritten ([`column_text`]).
fn statement_for(step: WriteStep<'_>, facts: &[ColumnFacts]) -> Bound {
    let mut params = Vec::new();
    let sql = match step {
        WriteStep::Delete(d) => {
            let w = where_key(&d.key, facts, &mut params);
            format!(
                "DELETE FROM {} WHERE {w}",
                qname3(&d.database, d.schema.as_deref(), &d.table)
            )
        }
        WriteStep::Update(u) => {
            let sets = u
                .set
                .iter()
                .map(|(col, v)| {
                    format!("{} = {}", ident(col), cell_sql(col, v, facts, &mut params))
                })
                .collect::<Vec<_>>()
                .join(", ");
            let w = where_key(&u.key, facts, &mut params);
            format!(
                "UPDATE {} SET {sets} WHERE {w}",
                qname3(&u.database, u.schema.as_deref(), &u.table)
            )
        }
        WriteStep::Insert(i) => {
            let table = qname3(&i.database, i.schema.as_deref(), &i.table);
            if i.cols.is_empty() {
                // Every column left to its default; `() VALUES ()` is not T-SQL.
                format!("INSERT INTO {table} DEFAULT VALUES")
            } else {
                let cols = i.cols.iter().map(|(c, _)| ident(c)).collect::<Vec<_>>();
                let vals = i
                    .cols
                    .iter()
                    .map(|(c, v)| cell_sql(c, v, facts, &mut params))
                    .collect::<Vec<_>>();
                format!(
                    "INSERT INTO {table} ({}) VALUES ({})",
                    cols.join(", "),
                    vals.join(", ")
                )
            }
        }
    };
    Bound { sql, params }
}

/// One re-read row's `SELECT`: key columns, then the confirming ones, the order
/// `edit::refetch_key` builds the values in.
fn refetch_statement(template: &RefetchTemplate, row: &RefetchRow, facts: &[ColumnFacts]) -> Bound {
    let mut params = Vec::new();
    let cols = template
        .columns
        .iter()
        .map(|c| ident(c))
        .collect::<Vec<_>>()
        .join(", ");
    let key: Vec<(String, Value)> = template
        .key_cols
        .iter()
        .chain(template.confirm_cols.iter())
        .zip(&row.key)
        .map(|(&ci, v)| (template.columns[ci].clone(), v.clone()))
        .collect();
    let w = where_key(&key, facts, &mut params);
    Bound {
        sql: format!(
            "SELECT TOP (1) {cols} FROM {} WHERE {w}",
            qname3(
                &template.database,
                template.schema.as_deref(),
                &template.table
            )
        ),
        params,
    }
}

/// One binary cell's length and its first [`FETCH_CAP`] bytes. `DATALENGTH`,
/// not `LEN`: the second counts characters and trims trailing blanks.
fn blob_statement(r: &BlobRef, facts: &[ColumnFacts]) -> Bound {
    let mut params = Vec::new();
    let col = ident(&r.column);
    let w = where_key(&r.key, facts, &mut params);
    Bound {
        sql: format!(
            "SELECT TOP (1) DATALENGTH({col}), SUBSTRING({col}, 1, {FETCH_CAP}) FROM {} WHERE {w}",
            qname3(&r.database, r.schema.as_deref(), &r.table)
        ),
        params,
    }
}

/// What the write needs to know about a column that the grid does not carry.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ColumnFacts {
    name: String,
    /// The base type — an alias type's underlying one — lower case.
    base_type: String,
    identity: bool,
    /// The code page a `char`/`varchar`/`text` column stores its text in —
    /// `65001` under a UTF-8 collation — and `0` for every other column,
    /// whose text (if any) is Unicode.
    code_page: u32,
    /// The column's collation, for asking the server about a code page
    /// [`code_page_fit`] cannot answer.
    collation: Option<String>,
}

/// The catalogue read behind [`ColumnFacts`], in `database`'s `sys.columns`
/// — the table's own, which on a pinned session is not the connection's
/// (see [`qname3`]); `@P1` is the table's three-part name.
fn column_facts_sql(database: &str) -> String {
    let catalogue = if database.is_empty() {
        "sys.columns".to_string()
    } else {
        format!("{}.sys.columns", ident(database))
    };
    format!(
        "SELECT c.name, TYPE_NAME(c.system_type_id), c.is_identity, \
                c.collation_name, CAST(COLLATIONPROPERTY(c.collation_name, 'CodePage') AS int) \
         FROM {catalogue} c WHERE c.object_id = OBJECT_ID(@P1)"
    )
}

/// Does `base_type` store its text in a code page rather than as Unicode?
fn holds_code_page_text(base_type: &str) -> bool {
    matches!(base_type, "char" | "varchar" | "text")
}

/// Whether a value survives the conversion into a column's code page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fit {
    Fits,
    /// The first character the code page has no byte for — which SQL Server
    /// writes as `?`, or as a best-fit look-alike (`Ω` as `O`), without a word.
    Loses(char),
    /// A code page this cannot read locally; the server has to be asked.
    Unknown,
}

/// Does `text` survive SQL Server's conversion into `code_page`?
///
/// `0` is a Unicode column and `65001` a UTF-8 collation, both of which hold
/// everything; ASCII fits every code page SQL Server stores a `varchar` in.
/// The rest are the Windows code pages `encoding_rs` implements, asked
/// strictly: a character with no byte there is a loss, never the best-fit
/// substitute the server would pick. The two IBM code pages behind the
/// `SQL_Latin1_General_CP437`/`CP850` collations are not among them, and
/// answer [`Fit::Unknown`].
fn code_page_fit(code_page: u32, text: &str) -> Fit {
    use encoding_rs::{
        BIG5, EUC_KR, GBK, SHIFT_JIS, WINDOWS_874, WINDOWS_1250, WINDOWS_1251, WINDOWS_1252,
        WINDOWS_1253, WINDOWS_1254, WINDOWS_1255, WINDOWS_1256, WINDOWS_1257, WINDOWS_1258,
    };
    if code_page == 0 || code_page == 65001 || text.is_ascii() {
        return Fit::Fits;
    }
    let enc = match code_page {
        874 => WINDOWS_874,
        932 => SHIFT_JIS,
        936 => GBK,
        949 => EUC_KR,
        950 => BIG5,
        1250 => WINDOWS_1250,
        1251 => WINDOWS_1251,
        1252 => WINDOWS_1252,
        1253 => WINDOWS_1253,
        1254 => WINDOWS_1254,
        1255 => WINDOWS_1255,
        1256 => WINDOWS_1256,
        1257 => WINDOWS_1257,
        1258 => WINDOWS_1258,
        _ => return Fit::Unknown,
    };
    if !enc.encode(text).2 {
        return Fit::Fits;
    }
    let mut buf = [0u8; 4];
    text.chars()
        .find(|c| !c.is_ascii() && enc.encode(c.encode_utf8(&mut buf)).2)
        .map_or(Fit::Unknown, Fit::Loses)
}

/// What [`code_page_check`] found: the refusal for the first value its column
/// cannot hold, and the values only the server can judge.
#[derive(Debug, Default)]
struct CodePageCheck<'a> {
    refusal: Option<String>,
    ask_server: Vec<(&'a ColumnFacts, &'a str)>,
}

/// The sentence for a value `col` cannot hold. `row` names an import's row.
fn code_page_refusal(row: Option<u64>, f: &ColumnFacts, lost: Option<char>) -> String {
    let what = match lost {
        Some(c) => format!(
            "`{c}`, which code page {} has no character for",
            f.code_page
        ),
        None => "a character its code page has no character for".to_string(),
    };
    let head = match row {
        Some(r) => format!("Row {r} has a value for {} ({})", f.name, f.base_type),
        None => format!("A value for {} ({})", f.name, f.base_type),
    };
    format!(
        "{head} holds {what}: SQL Server would store it as `?` rather than refuse it. \
         Store the column as nvarchar, or remove the character."
    )
}

/// [`blank_refusal`]'s twin for the other silent conversion: text written to
/// a `char`/`varchar`/`text` column that its code page cannot hold, which
/// SQL Server stores as `?` and reports as success.
fn code_page_check<'a>(write: &'a GridWrite, facts: &'a [ColumnFacts]) -> CodePageCheck<'a> {
    let staged = write
        .updates
        .iter()
        .flat_map(|u| u.set.iter())
        .chain(write.inserts.iter().flat_map(|i| i.cols.iter()));
    let mut out = CodePageCheck::default();
    for (col, v) in staged {
        let CellEdit::Text(t) = v else { continue };
        let Some(f) = fact(facts, col) else { continue };
        match code_page_fit(f.code_page, t) {
            Fit::Fits => {}
            Fit::Loses(c) => {
                out.refusal = Some(code_page_refusal(None, f, Some(c)));
                return out;
            }
            Fit::Unknown => out.ask_server.push((f, t)),
        }
    }
    out
}

/// [`code_page_check`] for an import batch, naming the file's row as
/// [`import_blank_refusal`] does.
fn import_code_page_check<'a>(
    columns: &[String],
    facts: &'a [ColumnFacts],
    batch: &'a [Vec<Value>],
    first_row: u64,
) -> CodePageCheck<'a> {
    let mut out = CodePageCheck::default();
    for (i, row) in batch.iter().enumerate() {
        for (col, v) in columns.iter().zip(row) {
            let Value::Str(s) = v else { continue };
            let Some(f) = fact(facts, col) else { continue };
            match code_page_fit(f.code_page, s) {
                Fit::Fits => {}
                Fit::Loses(c) => {
                    let row = first_row + i as u64 + 1;
                    out.refusal = Some(code_page_refusal(Some(row), f, Some(c)));
                    return out;
                }
                Fit::Unknown => out.ask_server.push((f, s)),
            }
        }
    }
    out
}

/// Ask the server whether each of `values` survives its column's collation —
/// for the code pages [`code_page_fit`] cannot read — and answer the refusal
/// for the first that does not. A collation name is spliced into the text, so
/// one that is not a plain word is not asked about, and the server decides.
async fn server_code_page_refusal(
    client: &mut MsClient,
    values: &[(&ColumnFacts, &str)],
) -> Result<Option<String>, DbError> {
    for (f, text) in values {
        let Some(coll) = f
            .collation
            .as_deref()
            .filter(|c| !c.is_empty() && c.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
        else {
            continue;
        };
        let sql = format!(
            "SELECT CASE WHEN CAST(CAST(@P1 COLLATE {coll} AS varchar(max)) AS nvarchar(max)) \
             COLLATE Latin1_General_BIN2 = @P1 COLLATE Latin1_General_BIN2 THEN 1 ELSE 0 END"
        );
        let stream = client
            .query(sql.as_str(), &[text])
            .await
            .map_err(|e| db_err(&e))?;
        let row = stream.into_row().await.map_err(|e| db_err(&e))?;
        if row.as_ref().and_then(|r| cell_text(r, 0)).as_deref() == Some("0") {
            return Ok(Some(code_page_refusal(None, f, None)));
        }
    }
    Ok(None)
}

/// Does `base_type` hold text, so that an empty string is a value of it?
fn holds_text(base_type: &str) -> bool {
    matches!(
        base_type,
        "char" | "varchar" | "nchar" | "nvarchar" | "text" | "ntext" | "xml" | "sql_variant"
    )
}

/// Would SQL Server convert this text to a zero or `1900-01-01` rather than
/// refuse it, in a column whose type is not text? An empty string, and one
/// made only of spaces: `' '` converts exactly as `''` does, while a tab, a
/// line break or any other blank is refused by the server itself (Msg 245 for
/// a number, 241 for a date — measured on 2022).
fn converts_blank(t: &str) -> bool {
    t.bytes().all(|b| b == b' ')
}

fn fact<'a>(facts: &'a [ColumnFacts], col: &str) -> Option<&'a ColumnFacts> {
    facts.iter().find(|f| f.name.eq_ignore_ascii_case(col))
}

/// The refusal for a batch that would write an empty string where SQL Server
/// converts one silently, or `None`.
///
/// **SQL Server does not refuse `''` for a number or a date — it converts it.**
/// An `int` becomes `0`, a `datetime` `1900-01-01`, a `bit` `0`; PostgreSQL
/// refuses the same statement. A cleared cell in the grid is `''`, so on this
/// engine alone clearing a quantity would store zero and report success. Asked
/// before anything runs, so nothing is written; a column the catalogue does not
/// name is left to the server.
fn blank_refusal(write: &GridWrite, facts: &[ColumnFacts]) -> Option<String> {
    let staged = write
        .updates
        .iter()
        .flat_map(|u| u.set.iter())
        .chain(write.inserts.iter().flat_map(|i| i.cols.iter()));
    for (col, v) in staged {
        let CellEdit::Text(t) = v else { continue };
        if !converts_blank(t) {
            continue;
        }
        if let Some(f) = fact(facts, col).filter(|f| !holds_text(&f.base_type)) {
            return Some(format!(
                "An empty or blank value can't be written to {} ({}): SQL Server would store it as \
                 0 or 1900-01-01 rather than refuse it. Set the cell to NULL, or type a value.",
                col, f.base_type
            ));
        }
    }
    None
}

/// Does this insert give an identity column a value? SQL Server refuses one
/// unless `IDENTITY_INSERT` is on for the table — and, once it is on, refuses
/// an insert that *doesn't*, so it is switched per statement.
fn sets_identity(ins: &RowInsert, facts: &[ColumnFacts]) -> bool {
    ins.cols
        .iter()
        .any(|(c, _)| fact(facts, c).is_some_and(|f| f.identity))
}

/// [`blank_refusal`] for an import batch: the refusal for the first empty
/// string bound for a column SQL Server would convert it in, or `None`.
/// `first_row` is how many rows earlier batches held, so the row named is the
/// file's (1-based, header not counted).
///
/// **It matters more here than in the grid.** A CSV's empty field is the
/// ordinary spelling of "no value", and unless the import's NULL rule caught
/// it, it arrives as `''` — which this engine alone stores as `0` or
/// `1900-01-01` and reports as success, for every row of a column at once.
fn import_blank_refusal(
    columns: &[String],
    facts: &[ColumnFacts],
    batch: &[Vec<Value>],
    first_row: u64,
) -> Option<String> {
    for (i, row) in batch.iter().enumerate() {
        for (col, v) in columns.iter().zip(row) {
            let Value::Str(s) = v else { continue };
            if !converts_blank(s) {
                continue;
            }
            if let Some(f) = fact(facts, col).filter(|f| !holds_text(&f.base_type)) {
                return Some(format!(
                    "Row {} has an empty or blank value for {} ({}): SQL Server would store it as 0 or \
                     1900-01-01 rather than refuse it. Import empty fields as NULL, or fill them in.",
                    first_row + i as u64 + 1,
                    col,
                    f.base_type
                ));
            }
        }
    }
    None
}

/// An import batch with every `datetime`/`smalldatetime` text rewritten by
/// [`column_text`] — a file's `2026-01-02 10:30` is 2 January under every
/// login language, as the grid's key is.
fn language_safe_batch(
    columns: &[String],
    facts: &[ColumnFacts],
    mut batch: Vec<Vec<Value>>,
) -> Vec<Vec<Value>> {
    for row in &mut batch {
        for (col, v) in columns.iter().zip(row.iter_mut()) {
            if let Value::Str(s) = v {
                *s = column_text(facts, col, s);
            }
        }
    }
    batch
}

/// Does an import into `columns` give an identity column a value? Then the
/// whole import runs under `IDENTITY_INSERT`, which is one table's at a time
/// and one import's for its length.
fn import_sets_identity(columns: &[String], facts: &[ColumnFacts]) -> bool {
    columns
        .iter()
        .any(|c| fact(facts, c).is_some_and(|f| f.identity))
}

/// The columns of the table a batch writes to.
async fn column_facts(
    client: &mut MsClient,
    database: &str,
    schema: Option<&str>,
    table: &str,
) -> Result<Vec<ColumnFacts>, DbError> {
    let name = qname3(database, schema, table);
    let stream = client
        .query(column_facts_sql(database).as_str(), &[&name.as_str()])
        .await
        .map_err(|e| db_err(&e))?;
    let rows = stream.into_first_result().await.map_err(|e| db_err(&e))?;
    Ok(rows
        .iter()
        .map(|r| {
            let base_type = cell_text(r, 1).unwrap_or_default().to_ascii_lowercase();
            let code_page = if holds_code_page_text(&base_type) {
                cell_text(r, 4).and_then(|c| c.parse().ok()).unwrap_or(0)
            } else {
                0
            };
            ColumnFacts {
                name: cell_text(r, 0).unwrap_or_default(),
                identity: cell_text(r, 2).as_deref() == Some("1"),
                code_page,
                collation: cell_text(r, 3),
                base_type,
            }
        })
        .collect())
}

// ── Accounts ─────────────────────────────────────────────────────────────────

/// Every login — SQL (`S`), Windows (`U`) and Windows group (`G`) — with its
/// server roles: `(name, type, disabled, default database, server roles)`.
/// The `##…##` certificate logins are the server's plumbing and left out.
const LOGIN_LISTING: &str = "SELECT sp.name, sp.type, CAST(sp.is_disabled AS int), \
            sp.default_database_name, \
            (SELECT STRING_AGG(r.name, ', ') WITHIN GROUP (ORDER BY r.name) \
               FROM sys.server_role_members m \
               JOIN sys.server_principals r ON r.principal_id = m.role_principal_id \
              WHERE m.member_principal_id = sp.principal_id) \
     FROM sys.server_principals sp \
     WHERE sp.type IN ('S', 'U', 'G') AND sp.name NOT LIKE N'##%' \
     ORDER BY sp.name";

/// The current database's users and roles, with each user's login and every
/// principal's role memberships: `(name, type, login, authentication, default
/// schema, fixed role, member of)`.
const USER_LISTING: &str = "SELECT dp.name, dp.type, SUSER_SNAME(dp.sid), \
            dp.authentication_type_desc, dp.default_schema_name, CAST(dp.is_fixed_role AS int), \
            (SELECT STRING_AGG(r.name, ', ') WITHIN GROUP (ORDER BY r.name) \
               FROM sys.database_role_members m \
               JOIN sys.database_principals r ON r.principal_id = m.role_principal_id \
              WHERE m.member_principal_id = dp.principal_id) \
     FROM sys.database_principals dp \
     WHERE dp.type IN ('S', 'U', 'G', 'E', 'X', 'R') \
     ORDER BY dp.name";

/// [`USER_LISTING`] for Azure SQL Database, which refuses `SUSER_SNAME` a
/// parameter (Msg 40507): the login is found by its SID instead. Not the one
/// query for both, as on a server `SUSER_SNAME` also names a Windows user who
/// has no login of their own, which the join cannot.
const AZURE_USER_LISTING: &str = "SELECT dp.name, dp.type, \
            (SELECT sp.name FROM sys.server_principals sp WHERE sp.sid = dp.sid), \
            dp.authentication_type_desc, dp.default_schema_name, CAST(dp.is_fixed_role AS int), \
            (SELECT STRING_AGG(r.name, ', ') WITHIN GROUP (ORDER BY r.name) \
               FROM sys.database_role_members m \
               JOIN sys.database_principals r ON r.principal_id = m.role_principal_id \
              WHERE m.member_principal_id = dp.principal_id) \
     FROM sys.database_principals dp \
     WHERE dp.type IN ('S', 'U', 'G', 'E', 'X', 'R') \
     ORDER BY dp.name";

/// `SERVERPROPERTY('EngineEdition')` of Azure SQL Database.
const AZURE_SQL_DATABASE: &str = "5";

/// A database principal's permissions in the current database, on **every**
/// securable class: `(state, permission, class, schema, name, column,
/// principal type)` — the schema of a schema-scoped securable (an object, a
/// type, an XML schema collection) or of a schema's own permission, the
/// securable's name, a column's name when the permission is on one, and a
/// principal's type where the securable is a user or a role.
///
/// It read four classes (`sys.objects` joined, so not even a system object
/// such as master's `sys.xp_cmdshell`), and `users::mssql_grant_statements`
/// dropped the rest. `OBJECT_SCHEMA_NAME`/`OBJECT_NAME` name system objects
/// too. Each name is `COLLATE DATABASE_DEFAULT`, since the catalogue views
/// disagree (`Latin1_General_BIN` beside the database's own) and a `CASE`
/// over them is Msg 451. A class with no branch here comes back unnamed, and
/// `users::mssql_unshown_note` counts it.
const DATABASE_PERMISSIONS: &str = "SELECT p.state, p.permission_name, p.class_desc, \
            CASE p.class WHEN 1 THEN OBJECT_SCHEMA_NAME(p.major_id) WHEN 3 THEN SCHEMA_NAME(p.major_id) \
                 WHEN 6 THEN SCHEMA_NAME(ty.schema_id) WHEN 10 THEN SCHEMA_NAME(x.schema_id) END, \
            CASE p.class WHEN 1 THEN OBJECT_NAME(p.major_id) \
                 WHEN 4 THEN dp.name COLLATE DATABASE_DEFAULT WHEN 5 THEN a.name COLLATE DATABASE_DEFAULT \
                 WHEN 6 THEN ty.name COLLATE DATABASE_DEFAULT WHEN 10 THEN x.name COLLATE DATABASE_DEFAULT \
                 WHEN 15 THEN mt.name COLLATE DATABASE_DEFAULT WHEN 16 THEN sc.name COLLATE DATABASE_DEFAULT \
                 WHEN 17 THEN sv.name COLLATE DATABASE_DEFAULT WHEN 18 THEN rsb.name COLLATE DATABASE_DEFAULT \
                 WHEN 19 THEN rt.name COLLATE DATABASE_DEFAULT WHEN 23 THEN ftc.name COLLATE DATABASE_DEFAULT \
                 WHEN 24 THEN sk.name COLLATE DATABASE_DEFAULT WHEN 25 THEN ce.name COLLATE DATABASE_DEFAULT \
                 WHEN 26 THEN ak.name COLLATE DATABASE_DEFAULT WHEN 29 THEN fsl.name COLLATE DATABASE_DEFAULT \
                 WHEN 31 THEN spl.name COLLATE DATABASE_DEFAULT WHEN 32 THEN dsc.name COLLATE DATABASE_DEFAULT END, \
            COL_NAME(CASE WHEN p.class = 1 AND p.minor_id <> 0 THEN p.major_id END, p.minor_id), \
            CASE p.class WHEN 4 THEN dp.type END \
     FROM sys.database_permissions p \
     LEFT JOIN sys.database_principals dp ON p.class = 4 AND dp.principal_id = p.major_id \
     LEFT JOIN sys.assemblies a ON p.class = 5 AND a.assembly_id = p.major_id \
     LEFT JOIN sys.types ty ON p.class = 6 AND ty.user_type_id = p.major_id \
     LEFT JOIN sys.xml_schema_collections x ON p.class = 10 AND x.xml_collection_id = p.major_id \
     LEFT JOIN sys.service_message_types mt ON p.class = 15 AND mt.message_type_id = p.major_id \
     LEFT JOIN sys.service_contracts sc ON p.class = 16 AND sc.service_contract_id = p.major_id \
     LEFT JOIN sys.services sv ON p.class = 17 AND sv.service_id = p.major_id \
     LEFT JOIN sys.remote_service_bindings rsb ON p.class = 18 AND rsb.remote_service_binding_id = p.major_id \
     LEFT JOIN sys.routes rt ON p.class = 19 AND rt.route_id = p.major_id \
     LEFT JOIN sys.fulltext_catalogs ftc ON p.class = 23 AND ftc.fulltext_catalog_id = p.major_id \
     LEFT JOIN sys.symmetric_keys sk ON p.class = 24 AND sk.symmetric_key_id = p.major_id \
     LEFT JOIN sys.certificates ce ON p.class = 25 AND ce.certificate_id = p.major_id \
     LEFT JOIN sys.asymmetric_keys ak ON p.class = 26 AND ak.asymmetric_key_id = p.major_id \
     LEFT JOIN sys.fulltext_stoplists fsl ON p.class = 29 AND fsl.stoplist_id = p.major_id \
     LEFT JOIN sys.registered_search_property_lists spl ON p.class = 31 AND spl.property_list_id = p.major_id \
     LEFT JOIN sys.database_scoped_credentials dsc ON p.class = 32 AND dsc.credential_id = p.major_id \
     WHERE p.grantee_principal_id = DATABASE_PRINCIPAL_ID(@P1) \
     ORDER BY p.class, 4, 5, 6, p.permission_name";

/// The database roles a database principal is a member of.
const DATABASE_ROLES_OF: &str = "SELECT r.name FROM sys.database_role_members m \
     JOIN sys.database_principals r ON r.principal_id = m.role_principal_id \
     WHERE m.member_principal_id = DATABASE_PRINCIPAL_ID(@P1) ORDER BY r.name";

/// A login's server-level permissions, on every class, in
/// [`DATABASE_PERMISSIONS`]' shape: `(state, permission, class, schema, name,
/// column, principal type)` — the server itself (no name), a login or a
/// server role (`SERVER_PRINCIPAL`, told apart by type) or an endpoint.
///
/// It read class `SERVER` alone, so `IMPERSONATE ON LOGIN::sa` — which makes
/// the login sysadmin one `EXECUTE AS` away — was never on the list, and the
/// login read as holding `CONNECT SQL`. An availability group comes back
/// unnamed, and `users::mssql_unshown_note` counts it.
const SERVER_PERMISSIONS: &str = "SELECT p.state, p.permission_name, p.class_desc, NULL, \
            CASE p.class WHEN 101 THEN sp.name WHEN 105 THEN e.name END, NULL, \
            CASE p.class WHEN 101 THEN sp.type END \
     FROM sys.server_permissions p \
     LEFT JOIN sys.server_principals sp ON p.class = 101 AND sp.principal_id = p.major_id \
     LEFT JOIN sys.endpoints e ON p.class = 105 AND e.endpoint_id = p.major_id \
     WHERE p.grantee_principal_id = SUSER_ID(@P1) \
     ORDER BY p.class, 5, p.permission_name";

/// The server roles a login is a member of.
const SERVER_ROLES_OF: &str = "SELECT r.name FROM sys.server_role_members m \
     JOIN sys.server_principals r ON r.principal_id = m.role_principal_id \
     WHERE m.member_principal_id = SUSER_ID(@P1) ORDER BY r.name";

/// `sql` with the one name parameter `@P1`, every row's cells as text.
async fn named_rows(
    client: &mut MsClient,
    sql: &str,
    name: &str,
) -> Result<Vec<Vec<Option<String>>>, DbError> {
    let stream = client.query(sql, &[&name]).await.map_err(|e| db_err(&e))?;
    let rows = stream.into_first_result().await.map_err(|e| db_err(&e))?;
    Ok(rows
        .iter()
        .map(|r| (0..r.len()).map(|i| cell_text(r, i)).collect())
        .collect())
}

/// Every login, and — with a database — that database's users and roles,
/// folded into the browser's one list by `users::from_mssql_rows`, each user
/// linked to its login. Without a database the note says where the users
/// are; a login that may not view every principal is told why the list is
/// short, since `sys.server_principals` shows such a login itself and little
/// else, with no error to say so.
pub(crate) async fn fetch_principals(
    db: &Db,
    database: Option<&str>,
) -> Result<schemaic_core::users::Principals, DbError> {
    use schemaic_core::users::{MsLoginRow, MsUserRow, Principals, from_mssql_rows};
    let flag = |r: &[Option<String>], i: usize| cell(r, i) == "1";
    let database = database.filter(|d| !d.is_empty());
    let mut client = connect(db, database).await?;
    let edition = query_rows(
        &mut client,
        "SELECT CAST(SERVERPROPERTY('EngineEdition') AS int)",
    )
    .await?
    .first()
    .map(|r| cell(r, 0))
    .unwrap_or_default();
    // Azure SQL Database's logins are `master`'s, and nothing sent from a
    // user database reaches them (`users::AccountScope::logins_elsewhere`) —
    // nor from `master` their server permissions, which have no catalogue
    // there. So none is listed, rather than rows no form could act on.
    let logins_elsewhere = edition == AZURE_SQL_DATABASE;
    let logins: Vec<MsLoginRow> = if logins_elsewhere {
        Vec::new()
    } else {
        query_rows(&mut client, LOGIN_LISTING)
            .await?
            .iter()
            .map(|r| MsLoginRow {
                name: cell(r, 0),
                kind: cell(r, 1),
                disabled: flag(r, 2),
                default_database: r.get(3).cloned().flatten(),
                server_roles: r.get(4).cloned().flatten(),
            })
            .collect()
    };
    let user_listing = if edition == AZURE_SQL_DATABASE {
        AZURE_USER_LISTING
    } else {
        USER_LISTING
    };
    let users: Vec<MsUserRow> = match database {
        Some(_) => query_rows(&mut client, user_listing)
            .await?
            .iter()
            .map(|r| MsUserRow {
                name: cell(r, 0),
                kind: cell(r, 1),
                login: r.get(2).cloned().flatten(),
                authentication: r.get(3).cloned().flatten(),
                default_schema: r.get(4).cloned().flatten(),
                fixed_role: flag(r, 5),
                member_of: r.get(6).cloned().flatten(),
            })
            .collect(),
        None => Vec::new(),
    };
    let sees_all = query_rows(
        &mut client,
        "SELECT CAST(HAS_PERMS_BY_NAME(NULL, NULL, 'VIEW ANY DEFINITION') AS int)",
    )
    .await?
    .first()
    .is_some_and(|r| flag(r, 0));
    // Whether a new user here may hold a password of its own — only a
    // contained database takes one (Msg 33233 elsewhere), and every Azure SQL
    // database does, though its `containment` reads 0.
    let contained = match database {
        Some(_) if edition == AZURE_SQL_DATABASE => true,
        Some(_) => query_rows(
            &mut client,
            "SELECT CAST(containment AS int) FROM sys.databases WHERE database_id = DB_ID()",
        )
        .await?
        .first()
        .is_some_and(|r| flag(r, 0)),
        None => false,
    };
    let mut notes: Vec<String> = Vec::new();
    if logins_elsewhere {
        notes.push(
            "Azure SQL Database keeps its logins in master, and they are not managed here: \
             listed are the database's users and roles."
                .to_string(),
        );
    } else if database.is_none() {
        notes.push(
            "Only the server's logins are listed: pick a database to see its users and roles."
                .to_string(),
        );
    }
    if !sees_all {
        notes.push(
            "This login lacks VIEW ANY DEFINITION, so the server shows it only the \
             principals it may see — the list may be short."
                .to_string(),
        );
    }
    Ok(Principals {
        list: from_mssql_rows(&logins, &users),
        note: (!notes.is_empty()).then(|| notes.join(" ")),
        password_policy: None,
        scope: schemaic_core::users::AccountScope {
            contained,
            logins_elsewhere,
            entra_users: db.auth == schemaic_core::connection::AuthMode::AzureCli,
        },
    })
}

/// A principal's permissions as `GRANT`/`DENY` sentences: a login's at the
/// server, with its server roles; a database user's or role's in `database`,
/// with its database roles — through `users::mssql_grant_statements`.
pub(crate) async fn fetch_grants(
    db: &Db,
    database: Option<&str>,
    principal: &schemaic_core::users::Principal,
) -> Result<schemaic_core::users::Grants, DbError> {
    use schemaic_core::users::{
        Grants, MsPermRow, PrincipalKind, mssql_grant_statements, mssql_server_role_statements,
        mssql_unshown_note,
    };
    let text = |r: &Vec<Option<String>>, i: usize| r.get(i).cloned().flatten();
    let first = |rows: Vec<Vec<Option<String>>>| -> Vec<String> {
        rows.into_iter()
            .filter_map(|r| r.into_iter().next().flatten())
            .collect()
    };
    // `SERVER_PERMISSIONS` and `DATABASE_PERMISSIONS` share one row shape.
    let perm_rows = |rows: Vec<Vec<Option<String>>>| -> Vec<MsPermRow> {
        rows.iter()
            .map(|r| MsPermRow {
                state: text(r, 0).unwrap_or_default(),
                permission: text(r, 1).unwrap_or_default(),
                class: text(r, 2).unwrap_or_default(),
                schema: text(r, 3),
                object: text(r, 4),
                column: text(r, 5),
                kind: text(r, 6),
            })
            .collect()
    };
    if principal.kind == PrincipalKind::Login {
        let mut client = connect(db, None).await?;
        let perms = perm_rows(named_rows(&mut client, SERVER_PERMISSIONS, &principal.name).await?);
        let roles = first(named_rows(&mut client, SERVER_ROLES_OF, &principal.name).await?);
        let mut statements = mssql_grant_statements(&principal.name, "", &perms, &[]);
        statements.extend(mssql_server_role_statements(&principal.name, &roles));
        return Ok(Grants {
            statements,
            note: mssql_unshown_note(&perms),
        });
    }
    let Some(database) = database.filter(|d| !d.is_empty()) else {
        return Ok(Grants {
            statements: Vec::new(),
            note: Some(
                "A database user's permissions are that database's — pick one to see them."
                    .to_string(),
            ),
        });
    };
    let mut client = connect(db, Some(database)).await?;
    let perms = perm_rows(named_rows(&mut client, DATABASE_PERMISSIONS, &principal.name).await?);
    let roles = first(named_rows(&mut client, DATABASE_ROLES_OF, &principal.name).await?);
    Ok(Grants {
        statements: mssql_grant_statements(&principal.name, database, &perms, &roles),
        note: mssql_unshown_note(&perms),
    })
}

/// What running one write statement came to.
#[derive(Debug)]
enum Ran {
    /// It finished, and this is its own row count.
    Counted(u64),
    /// Stop won and the attention was sent; `in_step` is whether the server
    /// acknowledged it (see [`attention`]), and so whether a rollback sent
    /// next on this connection reads its own answer.
    Stopped { in_step: bool },
}

/// Run one statement for its row count, raced against Stop.
///
/// The statement's own count is the **last** one: a trigger's statements
/// report theirs first, in the same batch.
async fn execute_counted(
    client: &mut MsClient,
    b: &Bound,
    cancel: &CancellationToken,
) -> Result<Ran, DbError> {
    let refs: Vec<&dyn tiberius::ToSql> =
        b.params.iter().map(|p| p as &dyn tiberius::ToSql).collect();
    let outcome = {
        let step = client.execute(b.sql.as_str(), &refs);
        tokio::select! {
            r = step => Some(r),
            _ = cancel.cancelled() => None,
        }
    };
    match outcome {
        None => Ok(Ran::Stopped {
            in_step: attention(client).await,
        }),
        Some(r) => r
            .map(|done| Ran::Counted(done.rows_affected().last().copied().unwrap_or(0)))
            .map_err(|e| db_err(&e)),
    }
}

/// Roll back, and say whether it is known to have happened.
///
/// **Complete whenever the server answered**, and there is no MySQL-style
/// half-answer to read: every SQL Server table is transactional. `IF
/// @@TRANCOUNT > 0` because some errors end the transaction themselves (a
/// deadlock victim, a severe error), and a bare `ROLLBACK` after one fails.
/// Unanswered is `Unknown` — the server rolls back a dropped connection's
/// transaction, but this side did not see it do so.
async fn rollback(client: &mut MsClient) -> Rollback {
    match drain(client, "IF @@TRANCOUNT > 0 ROLLBACK TRANSACTION").await {
        Ok(()) => Rollback::Complete,
        Err(_) => Rollback::Unknown,
    }
}

/// A cancelled commit: [`DbError::Cancelled`] when the rollback is known,
/// otherwise the sentence saying it is not — MySQL's `cancelled_write`.
fn cancelled_write(undone: Rollback) -> DbError {
    match undone {
        Rollback::Complete => DbError::Cancelled,
        undone => DbError::Refused(format!("Commit cancelled{}", undone.note())),
    }
}

/// [`cancelled_write`] for an import, whose sentence names the import: the
/// user stopped a file load, not a commit.
fn cancelled_import(undone: Rollback) -> DbError {
    match undone {
        Rollback::Complete => DbError::Cancelled,
        undone => DbError::Refused(format!("Import cancelled{}", undone.note())),
    }
}

/// An error's own sentence, without the variant's `query failed:` prefix — so
/// wrapping it again with a rollback note does not say that twice.
fn err_text(e: DbError) -> String {
    match e {
        DbError::Query(m) | DbError::Connect(m) | DbError::Refused(m) => m,
        other => other.to_string(),
    }
}

/// A failed step's error with what the rollback achieved appended.
async fn failed(client: &mut MsClient, msg: String) -> DbError {
    let undone = rollback(client).await;
    DbError::Query(format!("{msg}{}", undone.note()))
}

/// The savepoint a pinned session's write batches run under (its reads are not
/// fenced on this engine — see `Session::run_scope_sql`) —
/// T-SQL's spelling of the name `TxScope::Savepoint` uses on the other engines.
pub(crate) const SAVEPOINT: &str = "schemaic_w";

/// How [`write_on`] brackets a batch of grid edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WriteScope {
    /// Its own transaction, committed at the end — Auto mode, on a connection
    /// opened for it.
    Own,
    /// A savepoint inside the transaction the caller holds — a Manual tab's
    /// pinned session. T-SQL's is `SAVE TRANSACTION`, rolled back to by name
    /// and never released; the transaction is the caller's to end.
    Savepoint,
}

impl WriteScope {
    async fn begin(self, client: &mut MsClient) -> Result<(), DbError> {
        match self {
            WriteScope::Own => drain(client, "BEGIN TRANSACTION").await,
            WriteScope::Savepoint => drain(client, &format!("SAVE TRANSACTION {SAVEPOINT}")).await,
        }
    }

    /// Undo the batch so far, and say whether it is known to be undone.
    ///
    /// **A savepoint's rollback leaves the session's settings alone**, so an
    /// `IDENTITY_INSERT` the batch switched on is switched off again here: on a
    /// pinned connection it would outlive the batch and refuse the tab's next
    /// ordinary insert into that table (`Msg 545`).
    async fn undo(self, client: &mut MsClient, identity_on: Option<&str>) -> Rollback {
        match self {
            WriteScope::Own => rollback(client).await,
            WriteScope::Savepoint => {
                let undone = drain(client, &format!("ROLLBACK TRANSACTION {SAVEPOINT}")).await;
                if let Some(t) = identity_on {
                    let _ = drain(client, &format!("SET IDENTITY_INSERT {t} OFF")).await;
                }
                match undone {
                    Ok(()) => Rollback::Complete,
                    Err(_) => Rollback::Unknown,
                }
            }
        }
    }

    /// [`Self::undo`], with its answer recorded for the caller of `write_on`.
    async fn undo_into(
        self,
        client: &mut MsClient,
        identity_on: Option<&str>,
        record: &mut Option<Rollback>,
    ) -> Rollback {
        let undone = self.undo(client, identity_on).await;
        *record = Some(undone);
        undone
    }

    /// A failed step's error. Under a savepoint the rollback note is left off:
    /// the session answers for what survived, from `write_on`'s `undone`,
    /// and "rolled back all changes" would claim the user's whole transaction.
    fn failure(self, msg: String, undone: Rollback) -> DbError {
        match self {
            WriteScope::Own => DbError::Query(format!("{msg}{}", undone.note())),
            WriteScope::Savepoint => DbError::Query(msg),
        }
    }

    fn cancelled(self, undone: Rollback) -> DbError {
        match self {
            WriteScope::Own => cancelled_write(undone),
            WriteScope::Savepoint => DbError::Cancelled,
        }
    }
}

/// Commit a batch of grid edits in one transaction, each statement required to
/// affect exactly one row. Returns the rows written — equal to the statements
/// run, by the 1-row guard.
///
/// A `GridWrite` is one table's by construction, so its database is read off
/// the first step and the connection opened on it. Closing the connection is
/// the backstop for every early return: the server rolls back what a closed
/// connection left open.
pub(crate) async fn commit_writes(
    db: &Db,
    write: &GridWrite,
    cancel: CancellationToken,
) -> Result<u64, DbError> {
    let Some(first) = write.plan().first().copied() else {
        return Ok(0);
    };
    if cancel.is_cancelled() {
        return Err(DbError::Cancelled);
    }
    let database = match first {
        WriteStep::Delete(d) => &d.database,
        WriteStep::Update(u) => &u.database,
        WriteStep::Insert(i) => &i.database,
    };
    let mut client = connect(db, Some(database)).await?;
    write_on(&mut client, write, &cancel, WriteScope::Own, &mut None).await
}

/// [`commit_writes`]' body, on a connection the caller holds, bracketed by
/// `scope` — its own transaction, or a savepoint in the pinned session's.
///
/// `undone` is set to what the undo achieved whenever one ran, and left `None`
/// when the batch failed before anything was written. **The pinned session
/// reads it rather than rolling back to the savepoint again**, as the other
/// engines' sessions confirm theirs: T-SQL cannot release a savepoint, so its
/// name stays live for the rest of the transaction, and a second
/// `ROLLBACK TRANSACTION schemaic_w` after a batch that failed before its own
/// `SAVE` would land on an *earlier* batch's — silently undoing the user's
/// statements since.
pub(crate) async fn write_on(
    client: &mut MsClient,
    write: &GridWrite,
    cancel: &CancellationToken,
    scope: WriteScope,
    undone: &mut Option<Rollback>,
) -> Result<u64, DbError> {
    let plan = write.plan();
    let Some(first) = plan.first() else {
        return Ok(0);
    };
    if cancel.is_cancelled() {
        return Err(DbError::Cancelled);
    }
    // The database is part of the name here — see `qname3`: on a pinned
    // session the connection is in the session's database, not the edit's.
    let (database, schema, table) = match first {
        WriteStep::Delete(d) => (&d.database, d.schema.as_deref(), &d.table),
        WriteStep::Update(u) => (&u.database, u.schema.as_deref(), &u.table),
        WriteStep::Insert(i) => (&i.database, i.schema.as_deref(), &i.table),
    };
    let facts = column_facts(client, database, schema, table).await?;
    if let Some(msg) = blank_refusal(write, &facts) {
        return Err(DbError::Refused(msg));
    }
    let check = code_page_check(write, &facts);
    if let Some(msg) = check.refusal {
        return Err(DbError::Refused(msg));
    }
    if let Some(msg) = server_code_page_refusal(client, &check.ask_server).await? {
        return Err(DbError::Refused(msg));
    }
    scope.begin(client).await?;
    let mut total = 0u64;
    for step in plan {
        if cancel.is_cancelled() {
            return Err(scope.cancelled(scope.undo_into(client, None, undone).await));
        }
        let identity = match step {
            WriteStep::Insert(i) if sets_identity(i, &facts) => {
                Some(qname3(&i.database, i.schema.as_deref(), &i.table))
            }
            _ => None,
        };
        if let Some(t) = &identity
            && let Err(e) = drain(client, &format!("SET IDENTITY_INSERT {t} ON")).await
        {
            let u = scope.undo_into(client, None, undone).await;
            return Err(scope.failure(err_text(e), u));
        }
        let affected = match execute_counted(client, &statement_for(step, &facts), cancel).await {
            Ok(Ran::Counted(n)) => n,
            // **Only an acknowledged attention lets the rollback be believed.**
            // The statement's future was dropped mid-reply; without the
            // server's `DONE_ATTN` the stream is at an unknown point, and a
            // `ROLLBACK` sent now could read the aborted request's leftovers as
            // its own success — MySQL's desynchronised-stream defect, on this
            // engine. So nothing is sent: the server rolls back when the
            // connection closes, but this side did not see it.
            Ok(Ran::Stopped { in_step }) => {
                let u = if in_step {
                    scope.undo_into(client, identity.as_deref(), undone).await
                } else {
                    *undone = Some(Rollback::Unknown);
                    Rollback::Unknown
                };
                return Err(scope.cancelled(u));
            }
            Err(e) => {
                let u = scope.undo_into(client, identity.as_deref(), undone).await;
                return Err(scope.failure(err_text(e), u));
            }
        };
        if let Some(t) = &identity
            && let Err(e) = drain(client, &format!("SET IDENTITY_INSERT {t} OFF")).await
        {
            let u = scope.undo_into(client, identity.as_deref(), undone).await;
            return Err(scope.failure(err_text(e), u));
        }
        if let Err(msg) = one_row_verdict(step, affected) {
            let u = scope.undo_into(client, None, undone).await;
            return Err(scope.failure(msg, u));
        }
        total += affected;
    }
    if scope == WriteScope::Own
        && let Err(e) = drain(client, "COMMIT TRANSACTION").await
    {
        let u = scope.undo_into(client, None, undone).await;
        return Err(scope.failure(err_text(e), u));
    }
    Ok(total)
}

/// Read one bound `SELECT`'s first row as the grid's values, raced against Stop.
async fn first_row(
    client: &mut MsClient,
    b: &Bound,
    cancel: &CancellationToken,
) -> Result<Option<Vec<Value>>, DbError> {
    let refs: Vec<&dyn tiberius::ToSql> =
        b.params.iter().map(|p| p as &dyn tiberius::ToSql).collect();
    let read = async {
        let stream = client
            .query(b.sql.as_str(), &refs)
            .await
            .map_err(|e| db_err(&e))?;
        let row = stream.into_row().await.map_err(|e| db_err(&e))?;
        Ok::<_, DbError>(row.map(|r| r.cells().map(|(_, d)| cell_value(d)).collect()))
    };
    let outcome = tokio::select! {
        r = read => Some(r),
        _ = cancel.cancelled() => None,
    };
    match outcome {
        Some(r) => r,
        None => Err(cancel_now(client).await),
    }
}

/// Re-read the rows a commit changed, so the grid can splice them in place.
/// Each cell is rendered by [`cell_value`], as the read that produced the grid
/// rendered it. A row no longer there is skipped: a concurrent delete's right
/// answer is "nothing to splice".
pub(crate) async fn refetch_rows(
    db: &Db,
    template: &RefetchTemplate,
    rows: &[RefetchRow],
    cancel: CancellationToken,
) -> Result<Vec<(usize, Vec<Value>)>, DbError> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    if cancel.is_cancelled() {
        return Err(DbError::Cancelled);
    }
    let mut client = connect(db, Some(&template.database)).await?;
    refetch_on(&mut client, template, rows, &cancel).await
}

/// [`refetch_rows`]' body, on a connection the caller holds — the pinned
/// session's, the only one that sees rows its own transaction wrote.
pub(crate) async fn refetch_on(
    client: &mut MsClient,
    template: &RefetchTemplate,
    rows: &[RefetchRow],
    cancel: &CancellationToken,
) -> Result<Vec<(usize, Vec<Value>)>, DbError> {
    // The key's types — a `datetime` key is rewritten as the write's was.
    let facts = column_facts(
        client,
        &template.database,
        template.schema.as_deref(),
        &template.table,
    )
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let b = refetch_statement(template, row, &facts);
        if let Some(cells) = first_row(client, &b, cancel).await? {
            out.push((row.data_row, cells));
        }
    }
    Ok(out)
}

/// Read one binary cell — its whole length and its first [`FETCH_CAP`] bytes.
/// A NULL cell and a vanished row are the same answer, as on every engine.
pub(crate) async fn fetch_blob(
    db: &Db,
    r: &BlobRef,
    cancel: CancellationToken,
) -> Result<Option<BlobValue>, DbError> {
    if cancel.is_cancelled() {
        return Err(DbError::Cancelled);
    }
    let mut client = connect(db, Some(&r.database)).await?;
    blob_on(&mut client, r, &cancel).await
}

/// [`fetch_blob`]'s body, on a connection the caller holds — the pinned
/// session's, which sees bytes its own transaction wrote.
pub(crate) async fn blob_on(
    client: &mut MsClient,
    r: &BlobRef,
    cancel: &CancellationToken,
) -> Result<Option<BlobValue>, DbError> {
    let facts = column_facts(client, &r.database, r.schema.as_deref(), &r.table).await?;
    let b = blob_statement(r, &facts);
    let refs: Vec<&dyn tiberius::ToSql> =
        b.params.iter().map(|p| p as &dyn tiberius::ToSql).collect();
    let read = async {
        let stream = client
            .query(b.sql.as_str(), &refs)
            .await
            .map_err(|e| db_err(&e))?;
        let Some(row) = stream.into_row().await.map_err(|e| db_err(&e))? else {
            return Ok(None);
        };
        let mut cells = row.into_iter();
        // `DATALENGTH` is an `int`, or a `bigint` for a `(max)` column.
        let len = match cells.next() {
            Some(ColumnData::I32(Some(n))) => i64::from(n),
            Some(ColumnData::I64(Some(n))) => n,
            _ => return Ok(None),
        };
        let bytes = match cells.next() {
            Some(ColumnData::Binary(Some(b))) => b.into_owned(),
            _ => Vec::new(),
        };
        Ok::<_, DbError>(Some(BlobValue {
            bytes,
            len: len.max(0) as u64,
        }))
    };
    let outcome = tokio::select! {
        v = read => Some(v),
        _ = cancel.cancelled() => None,
    };
    match outcome {
        Some(v) => v,
        None => Err(cancel_now(client).await),
    }
}

#[cfg(test)]
mod write_tests {
    use super::*;
    use schemaic_core::model::{RowDelete, RowEdit};

    fn text(s: &str) -> ColumnData<'static> {
        ColumnData::String(Some(s.to_string().into()))
    }

    fn edit(set: Vec<(&str, CellEdit)>, key: Vec<(&str, Value)>) -> RowEdit {
        RowEdit {
            database: "shop".into(),
            schema: Some("dbo".into()),
            table: "orders".into(),
            set: set.into_iter().map(|(c, v)| (c.to_string(), v)).collect(),
            key: key.into_iter().map(|(c, v)| (c.to_string(), v)).collect(),
        }
    }

    fn insert(cols: Vec<(&str, CellEdit)>) -> RowInsert {
        RowInsert {
            database: "shop".into(),
            schema: Some("dbo".into()),
            table: "orders".into(),
            cols: cols.into_iter().map(|(c, v)| (c.to_string(), v)).collect(),
        }
    }

    fn facts(cols: &[(&str, &str, bool)]) -> Vec<ColumnFacts> {
        cols.iter()
            .map(|(n, t, id)| ColumnFacts {
                name: n.to_string(),
                base_type: t.to_string(),
                identity: *id,
                code_page: 0,
                collation: None,
            })
            .collect()
    }

    /// The `SET` values bind before the `WHERE`'s, a NULL is the literal and
    /// never a typed parameter, bytes bind as bytes, and a NULL key value is
    /// `IS NULL` — `= NULL` would match no row, and the 1-row guard would then
    /// report the wrong `WHERE` as a failed write.
    #[test]
    fn an_update_binds_in_text_order_and_writes_null_as_the_literal() {
        let e = edit(
            vec![
                ("note", CellEdit::Text("hi".into())),
                ("gone", CellEdit::Null),
                ("photo", CellEdit::Bytes(Arc::from(&[1u8, 2][..]))),
            ],
            vec![("id", Value::Int(7)), ("region", Value::Null)],
        );
        let b = statement_for(WriteStep::Update(&e), &[]);
        assert_eq!(
            b.sql,
            "UPDATE [shop].[dbo].[orders] SET [note] = @P1, [gone] = NULL, [photo] = @P2 \
             WHERE [id] = @P3 AND [region] IS NULL"
        );
        assert_eq!(
            b.params,
            vec![
                text("hi"),
                ColumnData::Binary(Some(vec![1u8, 2].into())),
                ColumnData::I64(Some(7)),
            ]
        );
    }

    #[test]
    fn an_insert_with_nothing_set_takes_every_default() {
        let i = insert(vec![]);
        let b = statement_for(WriteStep::Insert(&i), &[]);
        assert_eq!(b.sql, "INSERT INTO [shop].[dbo].[orders] DEFAULT VALUES");
        assert!(b.params.is_empty());

        let i = insert(vec![
            ("qty", CellEdit::Text("3".into())),
            ("note", CellEdit::Null),
        ]);
        let b = statement_for(WriteStep::Insert(&i), &[]);
        assert_eq!(
            b.sql,
            "INSERT INTO [shop].[dbo].[orders] ([qty], [note]) VALUES (@P1, NULL)"
        );
        assert_eq!(b.params, vec![text("3")]);
    }

    #[test]
    fn a_delete_names_its_whole_key() {
        let d = RowDelete {
            database: "shop".into(),
            schema: Some("sales".into()),
            table: "order lines".into(),
            key: vec![
                ("order_id".into(), Value::Int(1)),
                ("line".into(), Value::Str("A".into())),
            ],
        };
        let b = statement_for(WriteStep::Delete(&d), &[]);
        assert_eq!(
            b.sql,
            "DELETE FROM [shop].[sales].[order lines] WHERE [order_id] = @P1 AND [line] = @P2"
        );
        assert_eq!(b.params, vec![ColumnData::I64(Some(1)), text("A")]);
    }

    /// Through the one quoter: a `]` in a name is doubled, and nothing else is
    /// special inside brackets.
    #[test]
    fn a_bracket_in_a_name_is_doubled() {
        let mut e = edit(
            vec![("a]b", CellEdit::Text("x".into()))],
            vec![("id", Value::Int(1))],
        );
        e.table = "t]1".into();
        e.schema = None;
        let b = statement_for(WriteStep::Update(&e), &[]);
        // No schema: `db..t`, T-SQL's spelling of the default one.
        assert_eq!(
            b.sql,
            "UPDATE [shop]..[t]]1] SET [a]]b] = @P1 WHERE [id] = @P2"
        );
        // And no database either: two-part, as before.
        e.database = String::new();
        let b = statement_for(WriteStep::Update(&e), &[]);
        assert_eq!(b.sql, "UPDATE [t]]1] SET [a]]b] = @P1 WHERE [id] = @P2");
    }

    /// **A `datetime`'s text goes in the ISO `T` form**, which SQL Server
    /// reads the same under every login language: `yyyy-mm-dd hh:mm:ss` is
    /// year-*day*-month under `british` (measured on 2022). The grid's own
    /// spelling, a typed shorter one and a bare date are all rewritten;
    /// anything else is the server's to read.
    #[test]
    fn a_datetime_is_written_in_the_form_every_language_reads_alike() {
        let safe = language_safe_datetime;
        assert_eq!(
            safe("2026-01-02 00:00:00.000").as_deref(),
            Some("2026-01-02T00:00:00.000")
        );
        assert_eq!(
            safe("2026-03-04 10:30").as_deref(),
            Some("2026-03-04T10:30:00")
        );
        assert_eq!(
            safe("2026-03-04 9:05:07").as_deref(),
            Some("2026-03-04T09:05:07")
        );
        assert_eq!(safe("2026-03-04").as_deref(), Some("2026-03-04T00:00:00"));
        assert_eq!(
            safe("2026-03-04T10:30:00.5").as_deref(),
            Some("2026-03-04T10:30:00.5")
        );
        for other in [
            "20260304",
            "04/03/2026",
            "2026-3-4",
            "2026-03-04 10",
            "now",
            "",
        ] {
            assert_eq!(safe(other), None, "{other}");
        }

        // An import batch, the same way.
        let cols = ["at".to_string(), "note".to_string()];
        let batch = language_safe_batch(
            &cols,
            &facts(&[("at", "datetime", false)]),
            vec![vec![
                Value::Str("2026-01-02 10:30".into()),
                Value::Str("2026-01-02 10:30".into()),
            ]],
        );
        assert_eq!(
            batch[0],
            vec![
                Value::Str("2026-01-02T10:30:00".into()),
                Value::Str("2026-01-02 10:30".into())
            ]
        );

        // Applied to exactly the two language-sensitive types, keys and values.
        let f = facts(&[
            ("at", "datetime", false),
            ("sm", "smalldatetime", false),
            ("d2", "datetime2", false),
            ("note", "nvarchar", false),
        ]);
        let mut e = edit(
            vec![
                ("sm", CellEdit::Text("2026-03-04 10:30".into())),
                ("d2", CellEdit::Text("2026-03-04 10:30".into())),
                ("note", CellEdit::Text("2026-03-04 10:30".into())),
            ],
            vec![("at", Value::Str("2026-01-02 00:00:00.000".into()))],
        );
        e.schema = Some("dbo".into());
        let b = statement_for(WriteStep::Update(&e), &f);
        assert_eq!(
            b.params,
            vec![
                text("2026-03-04T10:30:00"),
                text("2026-03-04 10:30"),
                text("2026-03-04 10:30"),
                text("2026-01-02T00:00:00.000"),
            ]
        );
    }

    /// A float key goes as its text, so the server converts it to the
    /// column's own type — a `real` compared with an `f64` is widened, and
    /// `0.1` stored as a `real` is not `0.1` as an `f64`. A `u64` past `i64`
    /// goes as text too, rather than wrapping.
    #[test]
    fn a_float_key_is_compared_as_the_text_the_grid_shows() {
        let key = |v: Value| key_param("k", &v, &[]);
        assert_eq!(key(Value::Float(0.1)), text("0.1"));
        assert_eq!(key(Value::UInt(u64::MAX)), text(&u64::MAX.to_string()));
        assert_eq!(key(Value::UInt(5)), ColumnData::I64(Some(5)));
        // A column the facts do not name is not rewritten.
        assert_eq!(key(Value::Str("2008-06-01".into())), text("2008-06-01"));
    }

    #[test]
    fn a_reread_selects_one_row_by_key_then_confirming_columns() {
        let t = RefetchTemplate {
            database: "shop".into(),
            schema: Some("dbo".into()),
            table: "orders".into(),
            columns: vec!["id".into(), "qty".into(), "note".into()],
            key_cols: vec![0],
            confirm_cols: vec![1],
        };
        let row = RefetchRow {
            data_row: 4,
            key: vec![Value::Int(9), Value::Null],
        };
        let b = refetch_statement(&t, &row, &[]);
        assert_eq!(
            b.sql,
            "SELECT TOP (1) [id], [qty], [note] FROM [shop].[dbo].[orders] WHERE [id] = @P1 AND [qty] IS NULL"
        );
        assert_eq!(b.params, vec![ColumnData::I64(Some(9))]);
    }

    /// `DATALENGTH`, never `LEN` — the second counts characters and trims
    /// trailing blanks — and the read is capped at `FETCH_CAP`.
    #[test]
    fn a_binary_cell_is_read_by_its_byte_length_and_capped() {
        let r = BlobRef {
            database: "shop".into(),
            schema: Some("SalesLT".into()),
            table: "Product".into(),
            column: "ThumbNailPhoto".into(),
            key: vec![("ProductID".into(), Value::Int(680))],
        };
        let b = blob_statement(&r, &[]);
        assert_eq!(
            b.sql,
            format!(
                "SELECT TOP (1) DATALENGTH([ThumbNailPhoto]), SUBSTRING([ThumbNailPhoto], 1, {FETCH_CAP}) \
                 FROM [shop].[SalesLT].[Product] WHERE [ProductID] = @P1"
            )
        );
    }

    /// **SQL Server converts `''` to a number or a date rather than refusing
    /// it**, so a cleared cell would store 0 and report success. The batch is
    /// refused before anything runs — for an update and for an insert — and
    /// only where the column is not text.
    #[test]
    fn an_empty_value_is_refused_where_sql_server_would_convert_it() {
        let f = facts(&[
            ("qty", "int", false),
            ("shipped", "datetime", false),
            ("note", "nvarchar", false),
        ]);
        let blank = |col: &str| GridWrite {
            updates: vec![edit(
                vec![(col, CellEdit::Text(String::new()))],
                vec![("id", Value::Int(1))],
            )],
            ..Default::default()
        };
        let msg = blank_refusal(&blank("qty"), &f).expect("an int refuses ''");
        assert!(msg.contains("qty") && msg.contains("int"), "{msg}");
        assert!(blank_refusal(&blank("shipped"), &f).is_some());
        // Case-insensitive, as SQL Server's own names usually are.
        assert!(blank_refusal(&blank("QTY"), &f).is_some());
        // Text holds an empty string; the server decides for a column the
        // catalogue did not name.
        assert_eq!(blank_refusal(&blank("note"), &f), None);
        assert_eq!(blank_refusal(&blank("mystery"), &f), None);
        // An insert is refused the same way.
        let ins = GridWrite {
            inserts: vec![insert(vec![("qty", CellEdit::Text(String::new()))])],
            ..Default::default()
        };
        assert!(blank_refusal(&ins, &f).is_some());
        // A value, and a NULL, are not blanks.
        let fine = GridWrite {
            updates: vec![edit(
                vec![
                    ("qty", CellEdit::Text("0".into())),
                    ("shipped", CellEdit::Null),
                ],
                vec![("id", Value::Int(1))],
            )],
            ..Default::default()
        };
        assert_eq!(blank_refusal(&fine, &f), None);
    }

    /// **A value of spaces converts like an empty one.** SQL Server stores
    /// `' '` in an `int` as `0`, in a `datetime` or `date` as `1900-01-01`, in
    /// a `money` as `0.0000` and in a `float` or `bit` as `0`, exactly as it
    /// does `''` (measured on 2022), so a stray space — a spreadsheet paste of
    /// an empty-looking cell — went round the refusal. A tab, a line break or
    /// an ideographic space it refuses itself (Msg 245/241), so those are left
    /// to it. A text column keeps its spaces.
    #[test]
    fn a_whitespace_only_value_is_refused_like_an_empty_one() {
        let f = facts(&[
            ("qty", "int", false),
            ("shipped", "datetime", false),
            ("note", "nvarchar", false),
        ]);
        let set = |col: &str, v: &str| GridWrite {
            updates: vec![edit(
                vec![(col, CellEdit::Text(v.into()))],
                vec![("id", Value::Int(1))],
            )],
            ..Default::default()
        };
        for blank in [" ", "   "] {
            assert!(blank_refusal(&set("qty", blank), &f).is_some(), "{blank:?}");
            assert!(
                blank_refusal(&set("shipped", blank), &f).is_some(),
                "{blank:?}"
            );
            assert_eq!(blank_refusal(&set("note", blank), &f), None, "{blank:?}");
        }
        assert_eq!(blank_refusal(&set("qty", " 3 "), &f), None);
        assert_eq!(blank_refusal(&set("qty", "\t"), &f), None);
        let cols = ["qty".to_string()];
        let batch = [vec![Value::Str(" ".into())]];
        assert!(import_blank_refusal(&cols, &f, &batch, 0).is_some());
    }

    /// **Whether text survives a column's code page is told before anything
    /// runs.** Every code page SQL Server stores a `varchar` in holds ASCII,
    /// a UTF-8 collation and a Unicode column hold everything, and the rest
    /// are asked of the code page itself — `Ω` is not in 1252 and is in 1253.
    /// The two IBM code pages `encoding_rs` lacks answer `Unknown`, which the
    /// write settles by asking the server.
    #[test]
    fn a_code_page_says_which_characters_it_cannot_hold() {
        assert_eq!(code_page_fit(1252, "café"), Fit::Fits);
        assert_eq!(code_page_fit(1252, "Ωμέγα"), Fit::Loses('Ω'));
        assert_eq!(code_page_fit(1252, "ab日本"), Fit::Loses('日'));
        assert_eq!(code_page_fit(1253, "Ωμέγα"), Fit::Fits);
        assert_eq!(code_page_fit(932, "日本"), Fit::Fits);
        assert_eq!(code_page_fit(65001, "日本😀"), Fit::Fits);
        assert_eq!(code_page_fit(0, "日本"), Fit::Fits);
        assert_eq!(code_page_fit(850, "plain"), Fit::Fits);
        assert_eq!(code_page_fit(850, "é"), Fit::Unknown);
    }

    /// **Text a `varchar` column's code page cannot hold is refused**, naming
    /// the column and the character: SQL Server writes it as `?` and reports
    /// success (measured on 2022, `Ωμέγα` into a Latin-1 `varchar` stored as
    /// `Oµ??a`). An `nvarchar` takes it, and so does the same `varchar` given
    /// text its code page has. A value whose fit only the server can tell is
    /// handed back to be asked.
    #[test]
    fn text_a_varchar_cannot_hold_is_refused_before_it_becomes_a_question_mark() {
        let mut f = facts(&[("v", "varchar", false), ("n", "nvarchar", false)]);
        f[0].code_page = 1252;
        f[0].collation = Some("SQL_Latin1_General_CP1_CI_AS".into());
        let set = |col: &str, v: &str| GridWrite {
            updates: vec![edit(
                vec![(col, CellEdit::Text(v.into()))],
                vec![("id", Value::Int(1))],
            )],
            ..Default::default()
        };
        let greek = set("v", "Ωμέγα");
        let msg = code_page_check(&greek, &f).refusal.expect("refused");
        assert!(
            msg.contains('v') && msg.contains('Ω') && msg.contains("1252"),
            "{msg}"
        );
        assert!(code_page_check(&set("v", "café"), &f).refusal.is_none());
        assert!(code_page_check(&set("n", "Ωμέγα"), &f).refusal.is_none());
        f[0].code_page = 850;
        let accented = set("v", "é");
        let check = code_page_check(&accented, &f);
        assert!(check.refusal.is_none());
        assert_eq!(check.ask_server.len(), 1);

        f[0].code_page = 1252;
        let cols = ["n".to_string(), "v".to_string()];
        let batch = [
            vec![Value::Str("日本".into()), Value::Str("ok".into())],
            vec![Value::Str("x".into()), Value::Str("日本".into())],
        ];
        let msg = import_code_page_check(&cols, &f, &batch, 10)
            .refusal
            .expect("refused");
        assert!(msg.starts_with("Row 12 ") && msg.contains('日'), "{msg}");
    }

    /// `IDENTITY_INSERT` is wanted exactly when an insert gives the identity
    /// column a value: on, SQL Server refuses an insert that *doesn't*.
    #[test]
    fn identity_insert_is_wanted_only_for_an_insert_that_sets_the_identity() {
        let f = facts(&[("id", "int", true), ("name", "nvarchar", false)]);
        assert!(sets_identity(
            &insert(vec![("ID", CellEdit::Text("5".into()))]),
            &f
        ));
        assert!(!sets_identity(
            &insert(vec![("name", CellEdit::Text("x".into()))]),
            &f
        ));
        assert!(!sets_identity(&insert(vec![]), &f));
    }

    /// **An imported empty field is refused where SQL Server would convert
    /// it** — a CSV's blank quantity would otherwise land as `0` and a blank
    /// date as `1900-01-01` — naming the column and the row, counted from the
    /// first imported row. NULL, text columns and columns the catalogue does
    /// not name pass.
    #[test]
    fn an_imported_blank_is_refused_where_sql_server_would_convert_it() {
        let f = facts(&[("qty", "int", false), ("note", "nvarchar", false)]);
        let cols = ["note".to_string(), "qty".to_string()];
        let row = |note: &str, qty: Value| vec![Value::Str(note.into()), qty];
        let ok = [row("", Value::Null), row("x", Value::Str("3".into()))];
        assert_eq!(import_blank_refusal(&cols, &f, &ok, 0), None);
        let bad = [row("a", Value::Int(1)), row("b", Value::Str(String::new()))];
        let msg = import_blank_refusal(&cols, &f, &bad, 500).expect("refused");
        assert!(msg.contains("qty") && msg.starts_with("Row 502 "), "{msg}");
        // A column the catalogue does not know is the server's to judge.
        let unknown = ["other".to_string()];
        assert_eq!(
            import_blank_refusal(&unknown, &f, &[vec![Value::Str(String::new())]], 0),
            None
        );
    }

    /// A stopped import whose rollback is not confirmed says it was the
    /// *import* that was stopped — `cancelled_write`'s sentence says commit.
    #[test]
    fn a_stopped_import_is_called_an_import() {
        assert!(matches!(
            cancelled_import(Rollback::Complete),
            DbError::Cancelled
        ));
        let msg = cancelled_import(Rollback::Unknown).to_string();
        assert!(
            msg.contains("Import cancelled") && !msg.contains("Commit"),
            "{msg}"
        );
    }

    /// `IDENTITY_INSERT` is wanted for an import exactly when its columns give
    /// the identity a value.
    #[test]
    fn an_import_wants_identity_insert_only_when_it_writes_the_identity() {
        let f = facts(&[("id", "int", true), ("name", "nvarchar", false)]);
        assert!(import_sets_identity(&["ID".into(), "name".into()], &f));
        assert!(!import_sets_identity(&["name".into()], &f));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **An Entra handle with a plan that verifies nothing is refused before a
    /// token is fetched** — the composition of `AuthMode::transport_refusal`
    /// with the driver config every connect goes through. A handle from parts
    /// (the MCP handoff) carries only the plan it was handed; with none, the
    /// Azure CLI must never be asked, and nothing is dialled.
    #[tokio::test]
    async fn an_entra_handle_without_a_verifying_plan_is_refused_before_the_token() {
        use schemaic_core::connection::{AuthMode, SslMode, Tls};
        let entra = |mode: SslMode| {
            Db::from_parts(
                crate::Engine::MsSql,
                "srv.database.windows.net".into(),
                1433,
                String::new(),
                String::new(),
                String::new(),
            )
            .with_auth(AuthMode::AzureCli)
            .with_tls(
                Tls {
                    mode,
                    ..Tls::default()
                }
                .plan(),
            )
        };
        for mode in [
            SslMode::Disable,
            SslMode::Prefer,
            SslMode::Require,
            SslMode::VerifyCa,
        ] {
            match config(&entra(mode), None).await {
                Err(DbError::Refused(why)) => assert!(why.contains("Verify full"), "{why}"),
                other => panic!("{mode:?}: expected a refusal, got {:?}", other.map(|_| ())),
            }
        }
        // A password handle over the same plans is configured as before.
        let pw = Db::from_parts(
            crate::Engine::MsSql,
            "h".into(),
            1433,
            "sa".into(),
            "x".into(),
            String::new(),
        );
        assert!(config(&pw, None).await.is_ok());
    }

    /// **An Azure SQL server is told by its name**, in each cloud's spelling
    /// and whatever the case or a trailing dot — and only it gets the longer
    /// login bound a resuming serverless database needs.
    #[test]
    fn an_azure_sql_host_is_told_by_its_name() {
        for h in [
            "srv.database.windows.net",
            "SRV.Database.Windows.Net.",
            " srv.database.chinacloudapi.cn",
            "srv.database.usgovcloudapi.net",
        ] {
            assert!(is_azure_sql(h), "{h}");
        }
        for h in [
            "localhost",
            "127.0.0.1",
            "database.windows.net.evil.com",
            "db.windows.net",
        ] {
            assert!(!is_azure_sql(h), "{h}");
        }
        assert!(azure_timeout_text().contains("60 seconds"));
        let t = azure_unavailable_text("Database 'd' is not currently available. (Msg 40613)");
        assert!(
            t.starts_with("Database 'd'") && t.contains("connect again shortly"),
            "{t}"
        );
    }

    /// **The estimated plan executes nothing**, so it needs no guard and a
    /// write is as welcome as a read; **the measured one runs the statement**,
    /// inside a transaction opened first and never committed — and on a
    /// read-only connection only after the headless read gate has passed it,
    /// since that rollback cannot undo what `read_only_reason` refuses by name.
    #[test]
    fn a_plan_is_set_up_by_whether_it_runs_the_statement() {
        assert_eq!(
            explain_setup("DELETE FROM t", false, true).unwrap(),
            ["SET SHOWPLAN_XML ON"]
        );
        assert_eq!(
            explain_setup("SELECT 1", true, false).unwrap(),
            ["BEGIN TRANSACTION", "SET STATISTICS XML ON"]
        );
        assert_eq!(
            explain_setup("SELECT * FROM t", true, true).unwrap(),
            ["BEGIN TRANSACTION", "SET STATISTICS XML ON"]
        );
        for write in ["DELETE FROM t", "EXEC dbo.p", "SELECT NEXT VALUE FOR dbo.s"] {
            assert!(
                matches!(explain_setup(write, true, true), Err(DbError::Refused(_))),
                "{write}"
            );
        }
    }

    /// Only the plan's own result sets are kept: under `STATISTICS XML` the
    /// statement's rows come first, in sets of their own.
    #[test]
    fn only_a_showplan_result_set_is_a_plan() {
        assert!(is_showplan_set(&[schemaic_core::plan::SHOWPLAN_COLUMN]));
        assert!(!is_showplan_set(&["id"]));
        assert!(!is_showplan_set(&[
            schemaic_core::plan::SHOWPLAN_COLUMN,
            "x"
        ]));
        assert!(!is_showplan_set(&[]));
    }

    /// A stored trigger is read into its body and the header parts; one the
    /// parts cannot hold keeps its whole text as `verbatim`, and one whose text
    /// the server does not show (`WITH ENCRYPTION`) is `hidden` — both still
    /// listed, neither rebuilt.
    #[test]
    fn a_stored_trigger_reads_into_its_parts_or_is_kept_whole() {
        use schemaic_core::schema::{ExecuteAs, TriggerAction};
        let (action, tsql) = tsql_trigger_reading(Some(
            "create trigger dbo.tr on dbo.t with execute as owner after insert \
             not for replication as\nset nocount on",
        ));
        assert_eq!(action, TriggerAction::Body("set nocount on".into()));
        assert_eq!(tsql.execute_as, Some(ExecuteAs::Owner));
        assert!(tsql.not_for_replication && tsql.verbatim.is_none() && !tsql.hidden);
        let odd = "CREATE TRIGGER dbo.tr ON dbo.t FOR INSERT WITH APPEND AS SELECT 1";
        let (action, tsql) = tsql_trigger_reading(Some(odd));
        assert_eq!(tsql.verbatim.as_deref(), Some(odd));
        assert_eq!(action, TriggerAction::Body(odd.into()));
        let (action, tsql) = tsql_trigger_reading(None);
        assert!(tsql.hidden);
        assert_eq!(action, TriggerAction::Body(String::new()));
    }

    /// A stored routine reads into the parts its `CREATE` has — the
    /// parameter list **with its defaults**, which the catalogue does not keep
    /// — and one the parts cannot hold keeps the catalogue's list, the whole
    /// text as `verbatim`; no text at all is `hidden`.
    #[test]
    fn a_stored_routine_reads_into_its_parts_or_is_kept_whole() {
        use schemaic_core::schema::TsqlRoutineOption;
        let r = tsql_routine_reading(
            Some("create procedure dbo.p @a int = 5 with recompile as select @a"),
            "@a int".into(),
            String::new(),
        );
        assert_eq!(
            (r.arguments.as_str(), r.body.as_str()),
            ("@a int = 5", "select @a")
        );
        assert_eq!(r.tsql.options, [TsqlRoutineOption::Recompile]);
        assert!(r.tsql.verbatim.is_none() && !r.tsql.hidden);
        let odd = "CREATE PROCEDURE p;2 AS SELECT 1";
        let r = tsql_routine_reading(Some(odd), "@a int".into(), String::new());
        assert_eq!((r.arguments.as_str(), r.body.as_str()), ("@a int", odd));
        assert_eq!(r.tsql.verbatim.as_deref(), Some(odd));
        let r = tsql_routine_reading(None, "@x int".into(), "int".into());
        assert!(r.tsql.hidden);
        assert_eq!(
            (r.arguments.as_str(), r.returns.as_str(), r.body.as_str()),
            ("@x int", "int", "")
        );
    }

    fn names(n: &[&str]) -> Vec<String> {
        n.iter().map(|s| s.to_string()).collect()
    }

    async fn after(secs: u64, r: Result<Vec<String>, DbError>) -> Result<Vec<String>, DbError> {
        tokio::time::sleep(Duration::from_secs(secs)).await;
        r
    }

    /// A sign-in that takes `ms`, as [`listing_within`]'s `connect`.
    async fn signed_in_after(ms: u64) -> Result<(), DbError> {
        tokio::time::sleep(Duration::from_millis(ms)).await;
        Ok(())
    }

    /// The access check answering in time is the listing — the unfiltered
    /// one is never asked.
    #[tokio::test(start_paused = true)]
    async fn a_listing_that_answers_in_time_is_the_filtered_one() {
        let got = listing_within(
            crate::PING_TIMEOUT,
            signed_in_after(0),
            |()| after(1, Ok(names(&["a"]))),
            || async { panic!("the fallback ran") },
        )
        .await;
        assert_eq!(got.unwrap(), names(&["a"]));
    }

    /// **A plain login's `HAS_DBACCESS` waits on a database being created or
    /// dropped** — measured to 7.8 s — so a check that outruns its share of
    /// the budget gives way to the list without it, in what is left.
    #[tokio::test(start_paused = true)]
    async fn a_stalled_access_check_falls_back_to_the_unfiltered_listing() {
        let start = tokio::time::Instant::now();
        let got = listing_within(
            crate::PING_TIMEOUT,
            signed_in_after(100),
            |()| after(60, Ok(names(&["filtered"]))),
            || after(1, Ok(names(&["a", "b"]))),
        )
        .await;
        assert_eq!(got.unwrap(), names(&["a", "b"]));
        assert!(
            start.elapsed() < crate::PING_TIMEOUT,
            "{:?}",
            start.elapsed()
        );
    }

    /// The fallback shares the one budget rather than starting its own, and
    /// an error is an answer, not a stall — it is not papered over.
    #[tokio::test(start_paused = true)]
    async fn the_fallback_is_bounded_and_an_error_is_not_retried() {
        let start = tokio::time::Instant::now();
        let got = listing_within(
            crate::PING_TIMEOUT,
            signed_in_after(0),
            |()| after(60, Ok(Vec::new())),
            || after(60, Ok(Vec::new())),
        )
        .await;
        assert!(got.is_err());
        assert!(
            start.elapsed() <= crate::PING_TIMEOUT,
            "{:?}",
            start.elapsed()
        );
        let got = listing_within(
            crate::PING_TIMEOUT,
            signed_in_after(0),
            |()| after(0, Err(DbError::Query("denied".into()))),
            || async { panic!("the fallback ran") },
        )
        .await;
        assert!(matches!(got, Err(DbError::Query(m)) if m == "denied"));
        // A sign-in that never finishes is bounded by the whole budget, and
        // one that is refused is the answer.
        let start = tokio::time::Instant::now();
        let got: Result<Vec<String>, _> = listing_within(
            crate::PING_TIMEOUT,
            signed_in_after(60_000),
            |()| async { panic!("the query ran") },
            || async { panic!("the fallback ran") },
        )
        .await;
        assert!(matches!(got, Err(DbError::Connect(_))), "{got:?}");
        assert!(start.elapsed() <= crate::PING_TIMEOUT);
        let got: Result<Vec<String>, _> = listing_within(
            crate::PING_TIMEOUT,
            async { Err::<(), _>(DbError::Connect("refused".into())) },
            |()| async { panic!("the query ran") },
            || async { panic!("the fallback ran") },
        )
        .await;
        assert!(matches!(got, Err(DbError::Connect(m)) if m == "refused"));
    }

    /// **A slow sign-in is not a stalled access check.** A Microsoft Entra
    /// connect can take 3–5 s (the Azure CLI's token, then the login), and the
    /// access check's 3 s share used to include it: the filtered listing was
    /// abandoned mid-connect and the fallback connected from scratch in the
    /// 2 s left, so the tree said "timed out" where the one 5 s bound before
    /// it answered. The share times the query alone.
    #[tokio::test(start_paused = true)]
    async fn a_slow_connect_is_not_charged_to_the_access_check() {
        let got = listing_within(
            crate::PING_TIMEOUT,
            signed_in_after(3500),
            |()| after(0, Ok(names(&["a"]))),
            || async {
                signed_in_after(3500).await?;
                Ok(names(&["unfiltered"]))
            },
        )
        .await;
        assert_eq!(got.unwrap(), names(&["a"]));
    }

    /// **The fallback signs in again, so its sign-in is reserved before the
    /// access check is timed.** The check had `min(3 s, what is left)` and the
    /// fallback the rest, which behind a 1.5 s connect — a remote server, a
    /// VPN, Azure SQL's redirect — is half a second for a second sign-in and
    /// a query: a stalled `HAS_DBACCESS` answered "timed out" where the
    /// listing before it had listed. Modelled here as the real fallback is,
    /// a connect as long as the first and then the query.
    #[tokio::test(start_paused = true)]
    async fn a_stalled_access_check_leaves_the_fallback_its_own_sign_in() {
        for connect_ms in [100, 1000, 1500, 2000] {
            let start = tokio::time::Instant::now();
            let got = listing_within(
                crate::PING_TIMEOUT,
                signed_in_after(connect_ms),
                |()| after(60, Ok(names(&["filtered"]))),
                || async move {
                    signed_in_after(connect_ms).await?;
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    Ok(names(&["a", "b"]))
                },
            )
            .await;
            assert_eq!(
                got.unwrap(),
                names(&["a", "b"]),
                "a {connect_ms} ms sign-in"
            );
            assert!(
                start.elapsed() <= crate::PING_TIMEOUT,
                "{:?}",
                start.elapsed()
            );
        }
    }

    /// Both listings skip a database whose catalogue row is locked (`READPAST`)
    /// — reading `sys.databases` otherwise waits on one being created — and the
    /// filtered one asks nothing per database of a login that may enter any.
    #[test]
    fn the_listings_read_past_a_database_mid_create() {
        for q in [DATABASE_LISTING, DATABASE_LISTING_UNFILTERED] {
            assert!(q.contains("sys.databases WITH (READPAST)"), "{q}");
        }
        let short_circuit = DATABASE_LISTING
            .find("CONNECT ANY DATABASE")
            .expect("the short-circuit");
        assert!(short_circuit < DATABASE_LISTING.find("HAS_DBACCESS").unwrap());
        assert!(!DATABASE_LISTING_UNFILTERED.contains("HAS_DBACCESS"));
        // A single-user database never reaches `HAS_DBACCESS`, which stalls
        // two seconds on a held one — but only after the short-circuit, so a
        // login that may enter any database still sees it; and nothing
        // filters on `user_access` outside the `CASE`, which is how a
        // restricted database vanished from every tree.
        let single = DATABASE_LISTING
            .find("WHEN user_access = 1 THEN 0")
            .expect("the single-user guard");
        assert!(short_circuit < single);
        assert!(single < DATABASE_LISTING.find("HAS_DBACCESS").unwrap());
        let filter_of = |q: &'static str| &q[q.find("WHERE").expect("a filter")..];
        assert_eq!(
            filter_of(DATABASE_LISTING).matches("user_access").count(),
            1
        );
        assert!(!filter_of(DATABASE_LISTING_UNFILTERED).contains("user_access"));
        // Both return it, so a load can leave a single-user database unread.
        for q in [DATABASE_LISTING, DATABASE_LISTING_UNFILTERED] {
            assert!(q.starts_with("SELECT name, user_access FROM"), "{q}");
        }
    }

    /// A listing row's `user_access` of `1` is `SINGLE_USER`; `0`
    /// (`MULTI_USER`) and `2` (`RESTRICTED_USER`) are read like any other.
    #[test]
    fn a_listing_row_says_whether_its_database_is_single_user() {
        let row = |access: &str| vec![Some("d".to_string()), Some(access.to_string())];
        assert!(listed_database(&row("1")).single_user);
        assert!(!listed_database(&row("0")).single_user);
        assert!(!listed_database(&row("2")).single_user);
        assert!(!listed_database(&[Some("d".to_string()), None]).single_user);
        assert_eq!(listed_database(&row("1")).name, "d");
    }

    /// A view's stored text reads into its body and the header `ALTER VIEW`
    /// would reset — through `ddl::tsql_view_parts`, whose own tests hold the
    /// walk. What is here is what the reader does with each answer: parts
    /// into the options, an unreadable header kept whole and `verbatim`, no
    /// text at all `hidden`.
    #[test]
    fn a_stored_view_reads_into_its_parts_or_is_kept_whole() {
        let def = "CREATE VIEW dbo.v$as (a) WITH SCHEMABINDING AS SELECT id FROM dbo.t;";
        let v = tsql_view_reading(Some(def));
        assert_eq!(v.body, "SELECT id FROM dbo.t");
        assert_eq!(v.create_sql.as_deref(), Some(def));
        assert_eq!(v.options.column_list.as_deref(), Some("a"));
        assert_eq!(v.options.attributes, ["SCHEMABINDING"]);
        assert!(!v.options.tsql.verbatim && !v.options.tsql.hidden);

        let odd = "CREATE VIEW v WITH SCHEMABINDING, SOMETHING_NEW AS SELECT 1 AS x";
        let v = tsql_view_reading(Some(odd));
        assert_eq!((v.body.as_str(), v.create_sql.as_deref()), (odd, Some(odd)));
        assert!(v.options.tsql.verbatim);
        assert!(v.options.attributes.is_empty(), "nothing guessed at");

        let v = tsql_view_reading(None);
        assert!(v.options.tsql.hidden);
        assert_eq!((v.body.as_str(), v.create_sql), ("", None));
    }

    /// Seed and increment are spliced into `IDENTITY(…)`, so only integer
    /// text is kept — a `decimal(38,0)` identity's seed is wider than `i64`,
    /// which is why this is text at all.
    #[test]
    fn an_identity_spec_is_kept_only_as_integer_text() {
        assert_eq!(
            identity_spec("1000", "-5"),
            Some(("1000".into(), "-5".into()))
        );
        assert_eq!(
            identity_spec("99999999999999999999", "1"),
            Some(("99999999999999999999".into(), "1".into()))
        );
        assert_eq!(identity_spec("1.5", "1"), None);
        assert_eq!(identity_spec("1", "1) x"), None);
        assert_eq!(identity_spec("-", "1"), None);
        assert_eq!(identity_spec("", "1"), None);
    }

    #[test]
    fn a_decimal_is_rendered_from_its_scaled_integer() {
        assert_eq!(decimal_text(12345, 2), "123.45");
        assert_eq!(decimal_text(-5, 2), "-0.05");
        assert_eq!(decimal_text(-50, 2), "-0.50");
        assert_eq!(decimal_text(0, 3), "0.000");
        // No trailing point at scale 0 — the driver's own `Display` writes one.
        assert_eq!(decimal_text(123, 0), "123");
        assert_eq!(decimal_text(-123, 0), "-123");
        // `decimal(38, 0)`'s widest, which no float can hold.
        let max = 10i128.pow(38) - 1;
        assert_eq!(decimal_text(max, 0), "9".repeat(38));
        assert_eq!(decimal_text(i128::MIN, 4).len(), 41);
    }

    #[test]
    fn a_type_is_named_as_it_was_declared() {
        let t = |n: &str, len: i64, p: i64, s: i64| mssql_type_name(n, len, p, s, None);
        assert_eq!(t("nvarchar", 100, 0, 0), "nvarchar(50)");
        assert_eq!(t("nvarchar", -1, 0, 0), "nvarchar(max)");
        assert_eq!(t("varbinary", -1, 0, 0), "varbinary(max)");
        assert_eq!(t("char", 10, 0, 0), "char(10)");
        assert_eq!(t("decimal", 9, 10, 2), "decimal(10,2)");
        assert_eq!(t("datetime2", 8, 27, 7), "datetime2(7)");
        assert_eq!(t("time", 3, 8, 0), "time(0)");
        assert_eq!(t("float", 8, 53, 0), "float");
        assert_eq!(t("float", 4, 24, 0), "float(24)");
        assert_eq!(t("int", 4, 10, 0), "int");
        // An alias type is qualified with its own schema, which need not be
        // the table's — AdventureWorks' `SalesLT` tables use `dbo.Name`.
        assert_eq!(
            mssql_type_name("Name", 100, 0, 0, Some("dbo")),
            "[dbo].[Name]"
        );
    }

    #[test]
    fn a_stored_expression_loses_only_the_parentheses_around_all_of_it() {
        assert_eq!(strip_outer_parens("((0))"), "0");
        assert_eq!(strip_outer_parens("(getdate())"), "getdate()");
        assert_eq!(strip_outer_parens("([a]>(0))"), "[a]>(0)");
        assert_eq!(strip_outer_parens("(N'(x')"), "N'(x'");
        assert_eq!(strip_outer_parens("(a)+(b)"), "(a)+(b)");
        assert_eq!(strip_outer_parens("([x)]>(1))"), "[x)]>(1)");
        assert_eq!(strip_outer_parens("0"), "0");
    }

    /// **`sys.objects.type` is `char(2)`**, so a procedure's one-letter `P`
    /// arrives padded, `P `, and compared bare every procedure read as a
    /// function. The two-letter types fill the column and arrive as they are.
    #[test]
    fn a_routine_is_shaped_by_its_padded_object_type() {
        use schemaic_core::schema::RoutineKind;
        assert_eq!(
            routine_shape("P ", None),
            (RoutineKind::Procedure, String::new())
        );
        assert_eq!(
            routine_shape("FN", Some("int".into())),
            (RoutineKind::Function, "int".to_string())
        );
        assert_eq!(
            routine_shape("IF", None),
            (RoutineKind::Function, "TABLE".to_string())
        );
        assert_eq!(routine_shape("TF", None).1, "TABLE");
    }

    #[test]
    fn a_referential_action_is_spelled_as_sql_and_the_default_is_left_out() {
        assert_eq!(fk_action("NO_ACTION"), None);
        assert_eq!(fk_action("CASCADE").as_deref(), Some("CASCADE"));
        assert_eq!(fk_action("SET_NULL").as_deref(), Some("SET NULL"));
        assert_eq!(fk_action("SET_DEFAULT").as_deref(), Some("SET DEFAULT"));
    }

    #[test]
    fn a_date_counts_days_from_the_year_one() {
        assert_eq!(date_text(0), "0001-01-01");
        assert_eq!(date_text(3_652_058), "9999-12-31");
        // 2026-09-27 is 739,885 days after 0001-01-01.
        assert_eq!(date_text(739_885), "2026-09-27");
    }

    #[test]
    fn a_time_prints_the_fraction_its_scale_declares() {
        assert_eq!(time_text(0, 0), "00:00:00");
        assert_eq!(time_text(86_399, 0), "23:59:59");
        assert_eq!(time_text(462_537_336_813, 7), "12:50:53.7336813");
        assert_eq!(time_text(46_253_700, 3), "12:50:53.700");
        assert_eq!(time_text(5, 7), "00:00:00.0000005");
    }

    #[test]
    fn a_datetime_counts_three_hundredths_from_1900() {
        assert_eq!(datetime_text(0, 0), "1900-01-01 00:00:00.000");
        // 23:59:59.997, the largest `datetime` fraction.
        assert_eq!(datetime_text(0, 25_919_999), "1900-01-01 23:59:59.997");
        // Before 1900 the day count is negative.
        assert_eq!(datetime_text(-53_690, 0), "1753-01-01 00:00:00.000");
        // 12:50:53.730, as the server printed it on 2026-09-27.
        assert_eq!(datetime_text(46_290, 13_876_119), "2026-09-27 12:50:53.730");
    }

    #[test]
    fn a_smalldatetime_counts_minutes() {
        assert_eq!(smalldatetime_text(0, 0), "1900-01-01 00:00:00");
        assert_eq!(smalldatetime_text(1, 1439), "1900-01-02 23:59:00");
    }

    /// The wire holds UTC; the value shown is the stored local time, with
    /// its offset — including across midnight in either direction.
    #[test]
    fn a_datetimeoffset_is_shown_in_its_own_offset() {
        let day = 739_616; // 2026-01-01
        // Ticks at scale 7 are 10^-7 s; at scale 0, seconds.
        let ticks7 = |h: u64| h * 3600 * 10_000_000;
        let secs = |h: u64| h * 3600;
        assert_eq!(
            datetimeoffset_text(day, ticks7(8), 7, 120),
            "2026-01-01 10:00:00.0000000 +02:00"
        );
        assert_eq!(
            datetimeoffset_text(day, secs(23), 0, 90),
            "2026-01-02 00:30:00 +01:30"
        );
        assert_eq!(
            datetimeoffset_text(day, 0, 0, -300),
            "2025-12-31 19:00:00 -05:00"
        );
        assert_eq!(
            datetimeoffset_text(day, secs(12), 0, 0),
            "2026-01-01 12:00:00 +00:00"
        );
    }

    #[test]
    fn a_real_is_the_number_it_was_written_as() {
        assert_eq!(real_value(0.1), Value::Float(0.1));
        assert_eq!(real_value(-2.5), Value::Float(-2.5));
    }

    /// The rows measured on SQL Server 2022 for four statements: a missing
    /// table is reported with the describe's own follow-up after it, a temp
    /// table the batch makes is the describe saying it cannot tell, and a
    /// clean statement has no rows.
    #[test]
    fn a_describe_reports_the_statements_error_and_not_its_own() {
        let missing = [
            (208, "Invalid object name 'nope'.".to_string()),
            (11529, "The metadata could not be determined …".to_string()),
        ];
        assert_eq!(
            describe_error(&missing).as_deref(),
            Some("Invalid object name 'nope'. (Msg 208)")
        );
        let column = [
            (207, "Invalid column name 'nocol'.".to_string()),
            (11501, "The batch could not be analyzed …".to_string()),
        ];
        assert!(describe_error(&column).is_some_and(|e| e.contains("207")));
        let temp = [(11525, "… uses a temp table …".to_string())];
        assert_eq!(describe_error(&temp), None);
        assert_eq!(describe_error(&[]), None);
    }

    #[test]
    fn a_server_error_names_its_number_and_line() {
        assert_eq!(
            server_message("Invalid object name 'x'. ", 208, 1),
            "Invalid object name 'x'. (Msg 208, line 1)"
        );
        assert_eq!(
            server_message("Login failed for user 'sa'.", 18456, 0),
            "Login failed for user 'sa'. (Msg 18456)"
        );
    }

    fn described(name: &str) -> Described {
        Described {
            name: name.to_string(),
            type_name: "nvarchar(50)".to_string(),
            source: Some(("app".into(), "dbo".into(), "t".into(), name.into())),
            in_key: name == "id",
            nullable: name != "id",
            identity: false,
        }
    }

    fn wire(names: &[&str]) -> Vec<tiberius::Column> {
        names
            .iter()
            .map(|n| tiberius::Column::new(n.to_string(), ColumnType::NVarchar))
            .collect()
    }

    /// The description is used only when it names the columns the wire
    /// sent — a branch the compiler described differently from the one that
    /// ran must not put every cell under another column's type.
    #[test]
    fn a_description_that_disagrees_with_the_wire_is_not_used() {
        let d = vec![described("id"), described("name")];
        let used = result_columns(&wire(&["id", "name"]), Some(&d));
        assert_eq!(used[0].type_name, "nvarchar(50)");
        assert!(used[0].origin.as_ref().is_some_and(|o| o.flags.unique_key));
        assert!(used[1].origin.as_ref().is_some_and(|o| !o.flags.not_null));
        for other in [wire(&["id"]), wire(&["id", "other"])] {
            let used = result_columns(&other, Some(&d));
            assert!(used.iter().all(|c| c.origin.is_none()), "{used:?}");
            assert_eq!(used[0].type_name, "nvarchar");
        }
        assert!(result_columns(&wire(&["x"]), None)[0].origin.is_none());
    }
}
