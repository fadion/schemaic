//! DuckDB backend (fifth engine), built on the [`duckdb`] crate.
//!
//! Dispatched to from [`crate::Db`]'s public methods when the connection's engine
//! is [`crate::Engine::DuckDb`]. Embedded like SQLite, so most of what
//! [`crate::sqlite`]'s doc says holds here too — **there is no server**, a
//! connection is a file ([`crate::Db::file`]) and nothing else, and the driver
//! is blocking, so every call runs on Tokio's blocking pool. Five things make it
//! unlike SQLite, and each shapes the code below.
//!
//! **One database instance per file, per process.** DuckDB locks a file per
//! *instance*, and a second instance on the same file in the same process is
//! refused outright (measured on 2.0: *The process cannot access the file because
//! it is being used by another process*, naming this process). So the
//! one-connection-per-operation rule cannot be SQLite's "open the file per
//! operation": two operations that overlap — a running query and the health
//! check, a stream and a cell's blob — would refuse each other. Instead every
//! operation takes **its own connection** ([`duckdb::Connection::try_clone`]) on
//! a **shared instance** ([`instance`]), and the instance lives exactly as long
//! as some operation holds it: the registry keeps a [`Weak`], so the file is
//! closed — and its lock released for the DuckDB CLI or another program — the
//! moment the last operation on it ends. That is the stated exception to the
//! invariant (`docs/architecture/invariants.md`), and it is narrower than it
//! sounds: connections are still per operation; only the database underneath
//! them is shared while they overlap.
//!
//! **Sealed instances, for the paths nobody watches.** A DuckDB query can read
//! any file on this machine (`read_csv`, `read_parquet`, `FROM 'x.csv'`), so the
//! headless read paths — the MCP server's tools, `schemaic query`/`export`/
//! `explain` — open the file *sealed* ([`crate::Db::sealed`]): read-only, with
//! external access switched off and the configuration locked, so neither a
//! statement nor its functions can reach a file the database does not hold.
//! The text gate (`sql::read_only_reason`) refuses the readers by name first;
//! this is what holds when a spelling slips past it. An instance is sealed or
//! not for its whole life, so an operation that needs the other kind while one
//! is open is refused ([`DbRefusal::DuckDbSealedElsewhere`]) rather than given
//! the wrong one.
//!
//! **Nothing is fetched from the network.** Every instance opens with extension
//! auto-install and auto-load off: the extensions this build links (`parquet`,
//! `json`) are there, and any other is unreachable rather than downloaded behind
//! a query.
//!
//! **Values are typed by column, and richly.** A cell is read as DuckDB's own
//! value and rendered as DuckDB writes it — `true`/`false`, a `DECIMAL` exact,
//! a timestamp in ISO form (`TIMESTAMP WITH TIME ZONE` in UTC, this build linking
//! no ICU), a list `[1, 2]`, a struct `{'a': 1}` — see [`cell_of`].
//!
//! **Grid write-back rests on provenance read from the statement**, since
//! the driver reports none: [`origins_of`] places each result column on one
//! base table and proves it against the catalogue, or attributes nothing.
//! `rowid` is never a key — a checkpoint renumbers it.
//!
//! **What the roadmap's phase 2 has not reached yet**, refused rather than
//! faked: the view and macro editors (`ddl::duckdb_supports`). The table
//! designer runs — in place where DuckDB's `ALTER TABLE` can, by rebuilding
//! the table where it cannot (`ddl::duckdb_rebuild_sql`) — as do DDL plans,
//! `.sql` scripts, imports and the dump, and a Manual tab holds a connection
//! of its own ([`Pinned`]) — with the instance, and a file's lock, for as long
//! as it does.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, Weak};
use std::time::Instant;

use duckdb::core::{LogicalTypeHandle, LogicalTypeId};
use duckdb::types::{TimeUnit, Value as DValue};
use duckdb::{AccessMode, Config, Connection as DuckConn};
use schemaic_core::blob::{BlobRef, BlobValue};
use schemaic_core::intel::SqlDialect;
use schemaic_core::model::{
    CellEdit, Column, ColumnOrigin, DbRefusal, GridWrite, RefetchRow, RefetchTemplate,
    ResultBuilder, ResultSet, Value, WriteStep, binary_display,
};
use schemaic_core::schema::{
    CheckInfo, ColumnInfo, DbSchema, ForeignKeyInfo, IndexColumn, IndexInfo, RoutineInfo,
    RoutineKind, SequenceInfo, TableInfo,
};
use schemaic_core::server_vars::ServerVar;
use tokio_util::sync::CancellationToken;

use crate::{Db, DbError};

const D: SqlDialect = SqlDialect::DuckDb;

/// Map a driver error onto the app's error type: everything past the open is
/// the statement's, as on the other engines.
fn query_err(e: duckdb::Error) -> DbError {
    DbError::Query(e.to_string())
}

// ── The instance registry ───────────────────────────────────────────────────

/// One open DuckDB database: the connection every operation's own is cloned
/// from, and whether it was opened sealed.
struct Instance {
    root: Mutex<DuckConn>,
    sealed: bool,
}

/// The instances open in this process, by file. [`Weak`], so an instance
/// closes — and releases the file's lock — when its last operation ends.
static INSTANCES: LazyLock<Mutex<HashMap<String, Weak<Instance>>>> =
    LazyLock::new(Default::default);

/// The test suites' in-memory databases, held for the process: an in-memory
/// instance *is* its data, so letting it close between two operations of one
/// test would empty it. Only `cfg(test)` targets reach here ([`open_target`]).
#[cfg(test)]
static KEEP: LazyLock<Mutex<Vec<Arc<Instance>>>> = LazyLock::new(Default::default);

/// What [`open_target`] resolved a connection's file to.
#[derive(Debug, PartialEq, Eq)]
enum Target {
    /// A database file that exists.
    File(std::path::PathBuf),
    /// A named in-memory database, for the suites alone.
    #[cfg(test)]
    Memory,
}

/// The registry key and target for `file`, or why it is not one.
///
/// **A missing file is refused, not created.** DuckDB's open creates one, which
/// for a database *client* is the wrong default by some distance — a mistyped
/// path would open an empty database that looked like it worked. `:memory:` and
/// a MotherDuck `md:` name are refused too: the first is a database that dies
/// with each operation's instance, the second needs an extension this build
/// does not fetch. `allow_memory` is `cfg!(test)` at the one call site, where a
/// `memory:<name>` target is a shared in-memory database the suites seed, and
/// `is_file` is the filesystem's answer — a parameter so the refusals are
/// reachable from a test that touches no file.
fn open_target(
    file: &str,
    allow_memory: bool,
    is_file: impl Fn(&std::path::Path) -> bool,
) -> Result<(String, Target), DbError> {
    let f = file.trim();
    if f.is_empty() {
        return Err(DbError::Refused(DbRefusal::DuckDbNoFile));
    }
    if allow_memory && let Some(name) = f.strip_prefix("memory:") {
        #[cfg(test)]
        return Ok((format!("memory:{name}"), Target::Memory));
        #[cfg(not(test))]
        let _ = name;
    }
    if f.eq_ignore_ascii_case(":memory:")
        || f.to_ascii_lowercase().starts_with("md:")
        || f.to_ascii_lowercase().starts_with("motherduck:")
    {
        return Err(DbError::Refused(DbRefusal::DuckDbNotAFile(f.to_string())));
    }
    let path = std::path::Path::new(f);
    if !is_file(path) {
        return Err(DbError::Refused(DbRefusal::DuckDbMissing(f.to_string())));
    }
    // Keyed by the canonical path, so two spellings of one file share the
    // instance rather than the second being refused the lock.
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    Ok((
        canonical.to_string_lossy().into_owned(),
        Target::File(canonical),
    ))
}

/// The configuration every instance opens with.
fn config(sealed: bool, read_only_file: bool) -> duckdb::Result<Config> {
    let c = Config::default()
        .enable_autoload_extension(false)?
        .with("autoinstall_known_extensions", "false")?;
    if !sealed {
        return Ok(c);
    }
    let c = if read_only_file {
        c.access_mode(AccessMode::ReadOnly)?
    } else {
        c
    };
    c.enable_external_access(false)?
        .with("lock_configuration", "true")
}

/// The instance for `db`'s file — the one already open, or a new one.
fn instance(db: &Db) -> Result<Arc<Instance>, DbError> {
    let (key, target) = open_target(&db.file, cfg!(test), std::path::Path::is_file)?;
    let mut map = INSTANCES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(inst) = map.get(&key).and_then(Weak::upgrade) {
        if inst.sealed != db.sealed {
            return Err(DbError::Refused(DbRefusal::DuckDbSealedElsewhere));
        }
        return Ok(inst);
    }
    let connect_err = |e: duckdb::Error| DbError::Connect(format!("{}: {e}", db.file));
    let conn = match target {
        Target::File(path) => {
            DuckConn::open_with_flags(&path, config(db.sealed, true).map_err(connect_err)?)
                .map_err(connect_err)?
        }
        #[cfg(test)]
        Target::Memory => {
            DuckConn::open_in_memory_with_flags(config(db.sealed, false).map_err(connect_err)?)
                .map_err(connect_err)?
        }
    };
    let inst = Arc::new(Instance {
        root: Mutex::new(conn),
        sealed: db.sealed,
    });
    map.retain(|_, w| w.strong_count() > 0);
    map.insert(key.clone(), Arc::downgrade(&inst));
    #[cfg(test)]
    if key.starts_with("memory:") {
        KEEP.lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(inst.clone());
    }
    Ok(inst)
}

/// An operation's own connection, with the instance it must keep alive, and
/// the connection's init SQL already run on it.
fn connect(db: &Db) -> Result<(Arc<Instance>, DuckConn), DbError> {
    let inst = instance(db)?;
    let conn = inst
        .root
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .try_clone()
        .map_err(|e| DbError::Connect(format!("{}: {e}", db.file)))?;
    for stmt in db.init_statements()? {
        conn.execute_batch(&stmt)
            .map_err(|e| Db::init_failed(&stmt, e))?;
    }
    Ok((inst, conn))
}

/// Run `f` on an operation's own connection on a blocking thread.
async fn with_conn<T, F>(db: &Db, f: F) -> Result<T, DbError>
where
    T: Send + 'static,
    F: FnOnce(&DuckConn) -> Result<T, DbError> + Send + 'static,
{
    let db = db.clone();
    tokio::task::spawn_blocking(move || {
        let (_inst, conn) = connect(&db)?;
        f(&conn)
    })
    .await
    .map_err(|e| DbError::Refused(DbRefusal::DuckDbWorker(e.to_string())))?
}

fn refuse_if_cancelled(cancel: &CancellationToken) -> Result<(), DbError> {
    if cancel.is_cancelled() {
        return Err(DbError::Cancelled);
    }
    Ok(())
}

// ── Reading ─────────────────────────────────────────────────────────────────

/// Run `sql` and collect its rows into `dest`.
///
/// `ReadOnly` is a **read-only transaction** — `BEGIN TRANSACTION READ ONLY`,
/// which DuckDB holds per connection and in which every write is refused by its
/// effect (measured: *Cannot write to database … transaction is launched in
/// read-only mode*) — rolled back after. A sealed instance refuses writes
/// besides, being opened read-only.
pub(crate) async fn fetch_query(
    db: &Db,
    sql: &str,
    dest: &mut crate::RowDest,
    cancel: CancellationToken,
    enforce: Option<crate::Enforce>,
) -> Result<ResultSet, DbError> {
    refuse_if_cancelled(&cancel)?;
    let sql = sql.to_string();
    let db = db.clone();
    let read_only = enforce.is_some_and(crate::Enforce::refuses_writes);
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut moved = std::mem::replace(dest, crate::RowDest::Capped(0));
    let work = tokio::task::spawn_blocking(move || {
        let (_inst, conn) = match connect(&db) {
            Ok(c) => c,
            Err(e) => return (Err(e), moved),
        };
        let _ = tx.send(conn.interrupt_handle());
        let home = catalog(&conn).ok();
        if read_only && let Err(e) = conn.execute_batch("BEGIN TRANSACTION READ ONLY") {
            return (Err(query_err(e)), moved);
        }
        let rs = run_query(&conn, home.as_deref(), &sql, &mut moved);
        if read_only {
            let _ = conn.execute_batch("ROLLBACK");
        }
        (rs, moved)
    });
    let interrupt = rx.await.ok();
    let outcome = tokio::select! {
        r = work => Some(r.map_err(|e| DbError::Refused(DbRefusal::DuckDbWorker(e.to_string())))?),
        _ = cancel.cancelled() => {
            if let Some(h) = interrupt {
                h.interrupt();
            }
            None
        }
    };
    match outcome {
        Some((rs, back)) => {
            *dest = back;
            rs
        }
        None => Err(DbError::Cancelled),
    }
}

/// Run a statement list in order on **one connection** — so a `SET`, a
/// `USE` or a temporary table one statement makes is there for the next —
/// delivering each statement's outcome as it completes and stopping at the
/// first failure, as every engine's `run_batch` does.
pub(crate) async fn run_batch(
    db: &Db,
    stmts: &[String],
    row_cap: usize,
    cancel: CancellationToken,
    mut on_result: impl FnMut(usize, Result<ResultSet, DbError>),
    enforce: Option<crate::Enforce>,
) {
    let read_only = enforce.is_some_and(crate::Enforce::refuses_writes);
    let n = stmts.len();
    let owned: Vec<String> = stmts.to_vec();
    let db = db.clone();
    let (handle_tx, handle_rx) = tokio::sync::oneshot::channel();
    let (res_tx, mut res_rx) = tokio::sync::mpsc::unbounded_channel();
    let worker_cancel = cancel.clone();
    let work = tokio::task::spawn_blocking(move || {
        let (_inst, conn) = match connect(&db) {
            Ok(c) => c,
            Err(e) => {
                let _ = res_tx.send((0usize, Err(e)));
                return;
            }
        };
        let _ = handle_tx.send(conn.interrupt_handle());
        let home = catalog(&conn).ok();
        if read_only && let Err(e) = conn.execute_batch("BEGIN TRANSACTION READ ONLY") {
            let _ = res_tx.send((0usize, Err(query_err(e))));
            return;
        }
        for (i, stmt) in owned.iter().enumerate() {
            if worker_cancel.is_cancelled() {
                break;
            }
            let mut dest = crate::RowDest::Capped(row_cap);
            let r = match run_query(&conn, home.as_deref(), stmt, &mut dest) {
                Err(_) if worker_cancel.is_cancelled() => Err(DbError::Cancelled),
                other => other,
            };
            let failed = r.is_err();
            if res_tx.send((i, r)).is_err() || failed {
                break;
            }
        }
        if read_only {
            let _ = conn.execute_batch("ROLLBACK");
        }
    });
    let watcher = tokio::spawn(async move {
        let Ok(h) = handle_rx.await else { return };
        cancel.cancelled().await;
        h.interrupt();
    });
    let mut delivered = 0usize;
    while let Some((i, r)) = res_rx.recv().await {
        delivered = i + 1;
        on_result(i, r);
    }
    let _ = work.await;
    watcher.abort();
    for i in delivered..n {
        on_result(i, Err(DbError::Cancelled));
    }
}

/// Does `sql` begin a statement DuckDB answers with a row count — an
/// `INSERT`, `UPDATE` or `DELETE` with no `RETURNING`, or a
/// `CREATE TABLE … AS`? DuckDB returns that count as a one-row, one-column
/// result named `Count`, which the grid would otherwise show as data.
fn counts_rows(sql: &str) -> bool {
    let words = schemaic_core::sql::leading_words(sql, usize::MAX, D);
    let head = words.first().map(String::as_str);
    let returning = words.iter().any(|w| w == "RETURNING");
    match head {
        Some("INSERT" | "UPDATE" | "DELETE") => !returning,
        Some("CREATE") => words.iter().any(|w| w == "AS") && words.iter().any(|w| w == "TABLE"),
        _ => false,
    }
}

/// The blocking half of [`fetch_query`]. `home` is the file's own catalog,
/// read when the connection opened ([`origins_of`]).
fn run_query(
    conn: &DuckConn,
    home: Option<&str>,
    sql: &str,
    dest: &mut crate::RowDest,
) -> Result<ResultSet, DbError> {
    let row_cap = dest.cap();
    let start = Instant::now();
    let mut stmt = conn.prepare(sql).map_err(query_err)?;
    let mut rows = stmt.query([]).map_err(query_err)?;
    let (columns, kinds): (Vec<Column>, Vec<LogicalTypeId>) = {
        let st = rows
            .as_ref()
            .ok_or_else(|| DbError::Query("the statement returned no result".into()))?;
        (0..st.column_count())
            .map(|i| {
                let lt = st.column_logical_type(i);
                (
                    Column {
                        name: st.column_name(i).cloned().unwrap_or_default(),
                        type_name: type_name(&lt),
                        origin: None,
                    },
                    lt.id(),
                )
            })
            .unzip()
    };
    if counts_rows(sql) && columns.len() == 1 && columns[0].name == "Count" {
        let affected = match rows.next().map_err(query_err)? {
            Some(r) => match r.get::<_, DValue>(0).map_err(query_err)? {
                DValue::BigInt(n) => u64::try_from(n).unwrap_or(0),
                DValue::HugeInt(n) => u64::try_from(n).unwrap_or(0),
                DValue::UBigInt(n) => n,
                _ => 0,
            },
            None => 0,
        };
        let mut rs = ResultSet::default();
        rs.affected = Some(affected);
        rs.elapsed_ms = start.elapsed().as_millis();
        return Ok(rs);
    }
    if columns.is_empty() {
        let mut rs = ResultSet::default();
        rs.elapsed_ms = start.elapsed().as_millis();
        return Ok(rs);
    }
    // Before the rows are read, on the same connection — the result is
    // already materialised, and the session state it was read under (a
    // temporary table, `search_path`, `USE`) is the state that decides it.
    let mut columns = columns;
    let origins = origins_of(conn, home, sql, &columns);
    for (c, o) in columns.iter_mut().zip(origins) {
        c.origin = o;
    }
    let ncols = columns.len();
    let chunk_capacity = dest.chunk_capacity();
    let mut builder = ResultBuilder::with_capacity(columns, chunk_capacity);
    let mut cells: Vec<Value> = Vec::with_capacity(ncols);
    let mut truncated = false;
    while let Some(row) = rows.next().map_err(query_err)? {
        if builder.row_count() >= row_cap {
            truncated = true;
            break;
        }
        cells.clear();
        for (i, kind) in kinds.iter().enumerate() {
            let (v, bytes) = decode_cell(row, i, *kind, || columns_type(&builder, i))?;
            if bytes {
                builder.mark_binary(i);
            }
            cells.push(v);
        }
        builder.push_row(&cells);
        if dest.chunk_full(builder.row_count(), builder.text_bytes()) {
            dest.flush_blocking(&mut builder, chunk_capacity)?;
        }
    }
    dest.flush_blocking(&mut builder, 0)?;
    let mut rs = builder.finish();
    rs.truncated = truncated;
    rs.elapsed_ms = start.elapsed().as_millis();
    Ok(rs)
}

// ── Provenance ──────────────────────────────────────────────────────────────

/// One column of a table, as provenance needs it.
struct ColFacts {
    name: String,
    type_name: String,
    not_null: bool,
    default: Option<String>,
    generated: bool,
    primary_key: bool,
    unique: bool,
}

/// Where each of `columns` came from — the per-column provenance MySQL and
/// PostgreSQL read off the wire and DuckDB's driver does not report at all.
///
/// **Read from the statement, then checked against the catalogue, on the
/// connection that ran it.** [`schemaic_core::intel::projection_of`] places each result
/// column positionally on one base table (an alias renames the output, not the
/// column), and refuses every shape where a position could move — a join, a
/// set operation, a CTE, a derived table, `* EXCLUDE`/`* REPLACE`, a qualifier
/// that may be a struct column. What it places is then proven here, and any
/// doubt attributes the *whole* result to nothing, because a wrong origin is
/// an `UPDATE` of another row or another table while a missing one is only a
/// read-only grid:
///
/// - the connection is still on the file's own catalog (`home`) — a `USE` of
///   an attached database would have the bare name mean another file;
/// - a bare name is read in `current_schema()`, and only with `search_path`
///   unset and no temporary relation of that name, either of which would have
///   DuckDB resolve it elsewhere;
/// - the relation is a **base table** of that schema (a view is not: its rows
///   belong to whatever it reads);
/// - a wildcard's columns are the table's, in order and by name.
///
/// One thing costs only its own column rather than the result: a placed name
/// that is not a column of the table — `rowid`, or an earlier alias — is
/// attributed to nothing while the columns around it keep theirs. Names match
/// case-insensitively, as DuckDB binds, and come back in the catalogue's
/// spelling. A table column wins over an earlier alias of the same name, which
/// is what makes reading a bare name as the column sound (measured:
/// `SELECT x + 10 AS a, a` returns the column `a`).
///
/// **`rowid` is never an origin**, and so never the implicit key it is on
/// SQLite: a checkpoint renumbers rowids after a delete
/// (`a_checkpoint_renumbers_rowids_so_none_is_a_key`), so a keyless table is
/// read-only here.
fn origins_of(
    conn: &DuckConn,
    home: Option<&str>,
    sql: &str,
    columns: &[Column],
) -> Vec<Option<ColumnOrigin>> {
    attribute(conn, home, sql, columns).unwrap_or_else(|| vec![None; columns.len()])
}

/// [`origins_of`], `None` for "attribute nothing".
fn attribute(
    conn: &DuckConn,
    home: Option<&str>,
    sql: &str,
    columns: &[Column],
) -> Option<Vec<Option<ColumnOrigin>>> {
    use schemaic_core::intel::Projection;
    let (source, projection) = schemaic_core::intel::projection_of(sql, D)?;
    let home = home?;
    let one = |q: &str, p: &[&str]| -> Option<String> {
        conn.query_row(q, duckdb::params_from_iter(p), |r| r.get(0))
            .ok()
    };
    if one("SELECT current_database()", &[])?.as_str() != home {
        return None;
    }
    let schema = match &source.qualifier {
        // A qualifier that is also a catalog's name is DuckDB's to read either
        // way; refuse rather than guess which.
        Some(q) => {
            let catalogs: i64 = conn
                .query_row(
                    "SELECT count(*) FROM duckdb_databases() WHERE lower(database_name) = lower(?)",
                    [q],
                    |r| r.get(0),
                )
                .ok()?;
            if catalogs > 0 {
                return None;
            }
            one(
                "SELECT schema_name FROM duckdb_schemas() \
                 WHERE database_name = current_database() AND lower(schema_name) = lower(?)",
                &[q],
            )?
        }
        None => {
            if !one("SELECT current_setting('search_path')", &[])?.is_empty() {
                return None;
            }
            let shadows: i64 = conn
                .query_row(
                    "SELECT (SELECT count(*) FROM duckdb_tables() \
                              WHERE database_name = 'temp' AND lower(table_name) = lower(?1)) \
                          + (SELECT count(*) FROM duckdb_views() \
                              WHERE database_name = 'temp' AND lower(view_name) = lower(?1))",
                    [&source.name],
                    |r| r.get(0),
                )
                .ok()?;
            if shadows > 0 {
                return None;
            }
            one("SELECT current_schema()", &[])?
        }
    };
    let table = one(
        "SELECT table_name FROM duckdb_tables() \
         WHERE database_name = current_database() AND schema_name = ? \
           AND lower(table_name) = lower(?)",
        &[&schema, &source.name],
    )?;
    let facts = table_facts(conn, &schema, &table).ok()?;

    let find = |name: &str| facts.iter().find(|c| c.name.eq_ignore_ascii_case(name));
    let tail_matches = |from: usize| {
        columns.len() == from + facts.len()
            && columns[from..]
                .iter()
                .zip(&facts)
                .all(|(c, f)| c.name == f.name)
    };
    let placed: Vec<Option<&ColFacts>> = match &projection {
        Projection::Wildcard => {
            if !tail_matches(0) {
                return None;
            }
            facts.iter().map(Some).collect()
        }
        Projection::Items(items) => {
            if items.len() != columns.len() {
                return None;
            }
            items.iter().map(|i| i.as_deref().and_then(find)).collect()
        }
        Projection::LeadingThenWildcard(lead) => {
            if !tail_matches(lead.len()) {
                return None;
            }
            lead.iter()
                .map(|i| i.as_deref().and_then(find))
                .chain(facts.iter().map(Some))
                .collect()
        }
    };
    Some(
        placed
            .into_iter()
            .map(|f| {
                f.map(|f| {
                    let auto_increment = f.default.as_deref().is_some_and(|d| {
                        d.trim_start().to_ascii_lowercase().starts_with("nextval(")
                    });
                    ColumnOrigin {
                        database: home.to_string(),
                        schema: Some(schema.clone()),
                        table: table.clone(),
                        column: f.name.clone(),
                        flags: schemaic_core::model::ColumnFlags {
                            primary_key: f.primary_key,
                            unique_key: f.unique,
                            not_null: f.not_null,
                            auto_increment,
                            no_default: f.not_null
                                && f.default.is_none()
                                && !auto_increment
                                && !f.generated,
                        },
                        binary: schemaic_core::model::type_is_binary(&f.type_name),
                        implicit_key: false,
                    }
                })
            })
            .collect(),
    )
}

/// `schema.table`'s columns in declared order, with the keys each is part of
/// — a single-column `UNIQUE` marks its column unique, as a key would be.
fn table_facts(conn: &DuckConn, schema: &str, table: &str) -> duckdb::Result<Vec<ColFacts>> {
    let mut facts: Vec<ColFacts> = conn
        .prepare(
            "SELECT c.column_name, c.data_type, NOT c.is_nullable, c.column_default, \
                    coalesce(ic.is_generated = 'ALWAYS', false) \
               FROM duckdb_columns() c \
               LEFT JOIN information_schema.columns ic \
                 ON ic.table_catalog = c.database_name AND ic.table_schema = c.schema_name \
                AND ic.table_name = c.table_name AND ic.column_name = c.column_name \
              WHERE c.database_name = current_database() AND c.schema_name = ? \
                AND c.table_name = ? \
              ORDER BY c.column_index",
        )?
        .query_map([schema, table], |r| {
            let generated: bool = r.get(4)?;
            Ok(ColFacts {
                name: r.get(0)?,
                type_name: r.get(1)?,
                not_null: r.get(2)?,
                default: if generated { None } else { r.get(3)? },
                generated,
                primary_key: false,
                unique: false,
            })
        })?
        .collect::<duckdb::Result<_>>()?;
    let mut stmt = conn.prepare(
        "SELECT constraint_type, constraint_column_names FROM duckdb_constraints() \
          WHERE database_name = current_database() AND schema_name = ? AND table_name = ? \
            AND constraint_type IN ('PRIMARY KEY', 'UNIQUE')",
    )?;
    let mut rows = stmt.query([schema, table])?;
    while let Some(r) = rows.next()? {
        let kind: String = r.get(0)?;
        let cols = strings(r.get(1)?);
        for f in &mut facts {
            if !cols.iter().any(|c| c == &f.name) {
                continue;
            }
            if kind == "PRIMARY KEY" {
                f.primary_key = true;
                f.not_null = true;
            } else if cols.len() == 1 {
                f.unique = true;
            }
        }
    }
    Ok(facts)
}

/// Cell `i` of `row` as [`cell_of`] renders it, and whether it held bytes —
/// the one decoder, so a re-fetched row reads as the result it replaces.
///
/// A `NULL`-typed column (`SELECT NULL`) is asked nothing, since the driver
/// panics decoding one (duckdb 1.20000.1, `row.rs`), and any other value it
/// cannot decode costs that cell — a `<TYPE>` placeholder from `type_name` —
/// rather than the worker and with it the whole result.
fn decode_cell(
    row: &duckdb::Row<'_>,
    i: usize,
    kind: LogicalTypeId,
    type_name: impl FnOnce() -> String,
) -> Result<(Value, bool), DbError> {
    if kind == LogicalTypeId::SqlNull {
        return Ok((Value::Null, false));
    }
    let got = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| row.get::<_, DValue>(i)));
    let v: DValue = match got {
        Ok(r) => r.map_err(query_err)?,
        Err(_) => return Ok((Value::Str(format!("<{}>", type_name())), false)),
    };
    let bytes = matches!(v, DValue::Blob(_) | DValue::Geometry(_));
    Ok((cell_of(v, kind), bytes))
}

/// Column `i`'s type name, for a cell the driver could not decode.
fn columns_type(builder: &ResultBuilder, i: usize) -> String {
    builder
        .columns()
        .get(i)
        .map_or_else(|| "value".to_string(), |c| c.type_name.clone())
}

/// A result column's type, as DuckDB names it — `INTEGER`, `DECIMAL(10,3)`,
/// `VARCHAR[]`, `JSON` (an alias of `VARCHAR`, kept because it is what the
/// column was declared).
fn type_name(lt: &LogicalTypeHandle) -> String {
    if let Some(alias) = lt.get_alias().filter(|a| !a.is_empty()) {
        return alias.to_ascii_uppercase();
    }
    use LogicalTypeId as T;
    match lt.id() {
        T::Boolean => "BOOLEAN".into(),
        T::Tinyint => "TINYINT".into(),
        T::Smallint => "SMALLINT".into(),
        T::Integer => "INTEGER".into(),
        T::Bigint => "BIGINT".into(),
        T::Hugeint => "HUGEINT".into(),
        T::UTinyint => "UTINYINT".into(),
        T::USmallint => "USMALLINT".into(),
        T::UInteger => "UINTEGER".into(),
        T::UBigint => "UBIGINT".into(),
        T::UHugeint => "UHUGEINT".into(),
        T::Float => "FLOAT".into(),
        T::Double => "DOUBLE".into(),
        T::Decimal => format!("DECIMAL({},{})", lt.decimal_width(), lt.decimal_scale()),
        T::Varchar => "VARCHAR".into(),
        T::Blob => "BLOB".into(),
        T::Bit => "BIT".into(),
        T::Uuid => "UUID".into(),
        T::Date => "DATE".into(),
        T::Time => "TIME".into(),
        T::TimeNs => "TIME_NS".into(),
        T::TimeTZ => "TIME WITH TIME ZONE".into(),
        T::Timestamp => "TIMESTAMP".into(),
        T::TimestampS => "TIMESTAMP_S".into(),
        T::TimestampMs => "TIMESTAMP_MS".into(),
        T::TimestampNs => "TIMESTAMP_NS".into(),
        T::TimestampTZ => "TIMESTAMP WITH TIME ZONE".into(),
        T::Interval => "INTERVAL".into(),
        T::Enum => "ENUM".into(),
        T::Bignum => "BIGNUM".into(),
        T::SqlNull => "NULL".into(),
        T::List => format!("{}[]", type_name(&lt.child(0))),
        T::Array => format!("{}[]", type_name(&lt.child(0))),
        T::Map => format!(
            "MAP({}, {})",
            type_name(&lt.child(0)),
            type_name(&lt.child(1))
        ),
        T::Struct | T::Union => {
            let fields: Vec<String> = (0..lt.num_children())
                .map(|i| format!("{} {}", lt.child_name(i), type_name(&lt.child(i))))
                .collect();
            let kw = if lt.id() == T::Struct {
                "STRUCT"
            } else {
                "UNION"
            };
            format!("{kw}({})", fields.join(", "))
        }
        other => format!("{other:?}").to_ascii_uppercase(),
    }
}

/// One cell, as DuckDB writes it.
///
/// Integers stay numbers (a `HUGEINT` past `i64` becomes its digits); a `FLOAT`
/// is widened through its own shortest text, so `0.1::FLOAT` reads `0.1` rather
/// than `0.10000000149011612`; a `DECIMAL` keeps every digit as text. A
/// **blob has no lossless text form** and is rendered as its size, as on every
/// engine (`model::binary_display`); the caller marks the column binary.
fn cell_of(v: DValue, kind: LogicalTypeId) -> Value {
    match v {
        DValue::Null => Value::Null,
        DValue::Boolean(b) => Value::Str(if b { "true" } else { "false" }.to_string()),
        DValue::TinyInt(i) => Value::Int(i.into()),
        DValue::SmallInt(i) => Value::Int(i.into()),
        DValue::Int(i) => Value::Int(i.into()),
        DValue::BigInt(i) => Value::Int(i),
        DValue::UTinyInt(u) => Value::UInt(u.into()),
        DValue::USmallInt(u) => Value::UInt(u.into()),
        DValue::UInt(u) => Value::UInt(u.into()),
        DValue::UBigInt(u) => Value::UInt(u),
        DValue::HugeInt(i) => {
            i64::try_from(i).map_or_else(|_| Value::Str(i.to_string()), Value::Int)
        }
        DValue::UHugeInt(u) => {
            u64::try_from(u).map_or_else(|_| Value::Str(u.to_string()), Value::UInt)
        }
        DValue::Float(f) => Value::Float(f.to_string().parse().unwrap_or(f64::from(f))),
        DValue::Double(f) => Value::Float(f),
        DValue::Blob(b) | DValue::Geometry(b) => Value::Str(binary_display(b.len())),
        other => Value::Str(text_of(&other, kind, false)),
    }
}

/// `v` as DuckDB's text, `nested` inside a list, struct or map — where a
/// string is quoted, as DuckDB writes `['a', 'b']`.
fn text_of(v: &DValue, kind: LogicalTypeId, nested: bool) -> String {
    match v {
        DValue::Null => "NULL".to_string(),
        DValue::Boolean(b) => b.to_string(),
        DValue::TinyInt(i) => i.to_string(),
        DValue::SmallInt(i) => i.to_string(),
        DValue::Int(i) => i.to_string(),
        DValue::BigInt(i) => i.to_string(),
        DValue::HugeInt(i) => i.to_string(),
        DValue::UHugeInt(u) => u.to_string(),
        DValue::UTinyInt(u) => u.to_string(),
        DValue::USmallInt(u) => u.to_string(),
        DValue::UInt(u) => u.to_string(),
        DValue::UBigInt(u) => u.to_string(),
        DValue::Float(f) => f.to_string(),
        DValue::Double(f) => f.to_string(),
        DValue::Decimal(d) => d.to_string(),
        DValue::Timestamp(unit, t) => timestamp_text(*unit, *t, kind == LogicalTypeId::TimestampTZ),
        DValue::Text(s) | DValue::Enum(s) if nested => nested_string(s),
        DValue::Text(s) | DValue::Enum(s) => s.clone(),
        DValue::Blob(b) | DValue::Geometry(b) => binary_display(b.len()),
        DValue::Date32(days) => date_text(*days),
        DValue::Time64(unit, t) => time_text(micros(*unit, *t)),
        DValue::Interval {
            months,
            days,
            nanos,
        } => interval_text(*months, *days, *nanos),
        DValue::List(items) | DValue::Array(items) => {
            let inner: Vec<String> = items
                .iter()
                .map(|x| text_of(x, LogicalTypeId::Invalid, true))
                .collect();
            format!("[{}]", inner.join(", "))
        }
        DValue::Struct(fields) => {
            let inner: Vec<String> = fields
                .iter()
                .map(|(k, x)| {
                    format!(
                        "{}: {}",
                        nested_string(k),
                        text_of(x, LogicalTypeId::Invalid, true)
                    )
                })
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
        DValue::Map(pairs) => {
            let inner: Vec<String> = pairs
                .iter()
                .map(|(k, x)| {
                    format!(
                        "{}={}",
                        text_of(k, LogicalTypeId::Invalid, true),
                        text_of(x, LogicalTypeId::Invalid, true)
                    )
                })
                .collect();
            format!("{{{}}}", inner.join(", "))
        }
        DValue::Union(inner) => text_of(inner, LogicalTypeId::Invalid, nested),
        // A kind a newer driver adds: its debug form says what it is rather
        // than the row failing.
        other => format!("{other:?}"),
    }
}

/// A string inside a list, struct or map — or a struct's field name — as
/// DuckDB writes it and, more to the point, **reads it back**: quoted, with a
/// backslash before a quote or a backslash. Doubling the quote, SQL's own
/// escape, is not this grammar's — `['it''s']` casts back as `its` (measured
/// on 2.0) — and this text is what a grid edit of the cell sends.
fn nested_string(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// `t` in `unit`, in microseconds — the finest the text below writes.
fn micros(unit: TimeUnit, t: i64) -> i64 {
    match unit {
        TimeUnit::Second => t.saturating_mul(1_000_000),
        TimeUnit::Millisecond => t.saturating_mul(1_000),
        TimeUnit::Microsecond => t,
        TimeUnit::Nanosecond => t / 1_000,
    }
}

/// `.ffffff` with its trailing zeros dropped, or nothing for a whole second.
fn fraction(us: u32) -> String {
    if us == 0 {
        return String::new();
    }
    let s = format!(".{us:06}");
    s.trim_end_matches('0').to_string()
}

fn date_text(days: i32) -> String {
    chrono::DateTime::from_timestamp(i64::from(days) * 86_400, 0)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| days.to_string())
}

fn time_text(us: i64) -> String {
    let us = us.rem_euclid(86_400_000_000);
    let secs = us / 1_000_000;
    format!(
        "{:02}:{:02}:{:02}{}",
        secs / 3600,
        (secs / 60) % 60,
        secs % 60,
        fraction((us % 1_000_000) as u32)
    )
}

/// A timestamp as DuckDB writes one: `2020-01-02 03:04:05.123456`, with `+00`
/// after it for a `TIMESTAMP WITH TIME ZONE`, which this build (no ICU) holds
/// and shows in UTC.
fn timestamp_text(unit: TimeUnit, t: i64, zoned: bool) -> String {
    let us = micros(unit, t);
    let Some(dt) = chrono::DateTime::from_timestamp_micros(us) else {
        return t.to_string();
    };
    let frac = fraction(us.rem_euclid(1_000_000) as u32);
    let zone = if zoned { "+00" } else { "" };
    format!("{}{frac}{zone}", dt.format("%Y-%m-%d %H:%M:%S"))
}

/// An interval as DuckDB writes one: `1 year 2 months 3 days 04:05:06`, the
/// parts that are zero left out, `00:00:00` for none.
fn interval_text(months: i32, days: i32, nanos: i64) -> String {
    let mut parts = Vec::new();
    let plural = |n: i64, one: &str| {
        if n.abs() == 1 {
            format!("{n} {one}")
        } else {
            format!("{n} {one}s")
        }
    };
    let (years, months) = (months / 12, months % 12);
    if years != 0 {
        parts.push(plural(years.into(), "year"));
    }
    if months != 0 {
        parts.push(plural(months.into(), "month"));
    }
    if days != 0 {
        parts.push(plural(days.into(), "day"));
    }
    if nanos != 0 || parts.is_empty() {
        let us = nanos / 1_000;
        let sign = if us < 0 { "-" } else { "" };
        let us = us.abs();
        let secs = us / 1_000_000;
        parts.push(format!(
            "{sign}{:02}:{:02}:{:02}{}",
            secs / 3600,
            (secs / 60) % 60,
            secs % 60,
            fraction((us % 1_000_000) as u32)
        ));
    }
    parts.join(" ")
}

// ── Catalogue ───────────────────────────────────────────────────────────────

/// The file's own catalog name — what DuckDB calls the database the operation
/// opened on (`current_database()`), named after the file.
fn catalog(conn: &DuckConn) -> Result<String, DbError> {
    conn.query_row("SELECT current_database()", [], |r| r.get(0))
        .map_err(query_err)
}

/// Is the file there, and is it a DuckDB database? Opening the instance reads
/// the header, so a file that is not one fails here rather than at the first
/// query.
pub(crate) async fn ping(db: &Db) -> Result<(), DbError> {
    with_conn(db, |conn| catalog(conn).map(|_| ())).await
}

/// The one database a DuckDB connection holds: its file's catalog. An
/// `ATTACH`ed one lives only as long as the connection that ran it, which no
/// tree outlives.
pub(crate) async fn fetch_databases(db: &Db) -> Result<Vec<String>, DbError> {
    match tokio::time::timeout(crate::PING_TIMEOUT, with_conn(db, catalog)).await {
        Ok(r) => Ok(vec![r?]),
        Err(_) => Err(DbError::ConnectFailed(crate::ConnectFailure::TimedOut)),
    }
}

/// Validate `sql` without running it: DuckDB's `prepare` parses and binds.
///
/// A statement that acts while it is prepared is not prepared at all — the
/// heads below change the session or the instance (`SET`, `PRAGMA`, `ATTACH`,
/// `INSTALL`/`LOAD`, `USE`, `CHECKPOINT`), and this runs as the user types.
/// Those go unchecked; the run reports their errors.
pub(crate) async fn prepare_check(db: &Db, sql: &str) -> Result<(), DbError> {
    let words = schemaic_core::sql::leading_words(sql, 1, D);
    if matches!(
        words.first().map(String::as_str),
        Some(
            "SET"
                | "RESET"
                | "PRAGMA"
                | "ATTACH"
                | "DETACH"
                | "INSTALL"
                | "LOAD"
                | "USE"
                | "CHECKPOINT"
                | "FORCE"
                | "CALL"
                | "EXPORT"
                | "IMPORT"
                | "COPY"
                | "VACUUM"
                | "BEGIN"
                | "COMMIT"
                | "ROLLBACK"
                | "ABORT"
        )
    ) {
        return Ok(());
    }
    let sql = sql.to_string();
    with_conn(db, move |conn| {
        conn.prepare(&sql).map(|_| ()).map_err(query_err)
    })
    .await
}

/// The Server variables viewer's list: the engine's version, then every
/// `duckdb_settings()` row, its description as the detail.
pub(crate) async fn fetch_server_vars(db: &Db) -> Result<Vec<ServerVar>, DbError> {
    with_conn(db, |conn| {
        let version: String = conn
            .query_row("SELECT version()", [], |r| r.get(0))
            .map_err(query_err)?;
        let mut out = vec![ServerVar {
            name: "version".to_string(),
            value: version,
            detail: None,
        }];
        let mut stmt = conn
            .prepare(
                "SELECT name, coalesce(value, ''), description FROM duckdb_settings() \
                 ORDER BY name",
            )
            .map_err(query_err)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(ServerVar {
                    name: r.get(0)?,
                    value: r.get(1)?,
                    detail: r.get::<_, Option<String>>(2)?,
                })
            })
            .map_err(query_err)?;
        for r in rows {
            out.push(r.map_err(query_err)?);
        }
        Ok(out)
    })
    .await
}

/// Every table and view in the file's catalog, names only.
pub(crate) async fn fetch_table_list(db: &Db) -> Result<DbSchema, DbError> {
    with_conn(db, |conn| {
        let mut stmt = conn
            .prepare(
                "SELECT schema_name, table_name, false FROM duckdb_tables() \
                   WHERE database_name = current_database() AND NOT internal \
                 UNION ALL \
                 SELECT schema_name, view_name, true FROM duckdb_views() \
                   WHERE database_name = current_database() AND NOT internal \
                 ORDER BY 1, 2",
            )
            .map_err(query_err)?;
        let tables = stmt
            .query_map([], |r| {
                Ok(TableInfo {
                    schema: Some(r.get(0)?),
                    name: r.get(1)?,
                    is_view: r.get(2)?,
                    ..Default::default()
                })
            })
            .map_err(query_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(query_err)?;
        Ok(DbSchema {
            tables,
            database: Some(catalog(conn)?),
            names_ignore_case: true,
            ..Default::default()
        })
    })
    .await
}

/// Sizes per table: DuckDB keeps a row estimate per table and nothing per
/// table on disk, so the rows are all there is.
pub(crate) async fn fetch_table_stats(
    db: &Db,
) -> Result<schemaic_core::stats::SchemaStats, DbError> {
    with_conn(db, |conn| {
        let mut stmt = conn
            .prepare(
                "SELECT schema_name, table_name, estimated_size FROM duckdb_tables() \
                 WHERE database_name = current_database() AND NOT internal",
            )
            .map_err(query_err)?;
        let tables = stmt
            .query_map([], |r| {
                Ok(schemaic_core::stats::TableStats {
                    schema: Some(r.get(0)?),
                    table: r.get(1)?,
                    rows: r
                        .get::<_, Option<i64>>(2)?
                        .and_then(|n| u64::try_from(n).ok()),
                    ..Default::default()
                })
            })
            .map_err(query_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(query_err)?;
        Ok(schemaic_core::stats::SchemaStats::new(tables))
    })
    .await
}

/// A `VARCHAR[]` read as its strings.
fn strings(v: DValue) -> Vec<String> {
    match v {
        DValue::List(items) | DValue::Array(items) => items
            .into_iter()
            .filter_map(|x| match x {
                DValue::Text(s) => Some(s),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The `SELECT` of a stored `CREATE VIEW` — what follows its first top-level
/// `AS` — or the whole text where it has none. DuckDB keeps the view's
/// statement as written (`duckdb_views().sql`); the editor and the differ want
/// the definition alone, as PostgreSQL's `pg_get_viewdef` gives it.
fn view_body(sql: &str) -> String {
    let b = sql.as_bytes();
    let mut i = 0;
    let mut depth = 0i32;
    while i < b.len() {
        if let Some(next) = schemaic_core::sql::skip_noncode(b, i, D) {
            i = next;
            continue;
        }
        match b[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ => {}
        }
        if depth == 0
            && b[i].eq_ignore_ascii_case(&b'a')
            && b.get(i + 1).is_some_and(|c| c.eq_ignore_ascii_case(&b's'))
            && (i == 0 || !schemaic_core::sql::is_word_byte(b[i - 1]))
            && !b
                .get(i + 2)
                .is_some_and(|c| schemaic_core::sql::is_word_byte(*c))
        {
            return sql[i + 2..].trim().trim_end_matches(';').trim().to_string();
        }
        i += 1;
    }
    sql.trim().trim_end_matches(';').trim().to_string()
}

/// Introspect the file's catalog: its schemas, tables and views with their
/// columns, keys, foreign keys, checks and indexes, its sequences and its
/// macros — all from DuckDB's own `duckdb_*()` functions, which hold each
/// object's `CREATE` statement as written. That text is kept
/// ([`TableInfo::create_sql`]), so *Copy DDL* and the definition shown are
/// the engine's own words rather than a reconstruction.
pub(crate) async fn fetch_schema(db: &Db, cancel: CancellationToken) -> Result<DbSchema, DbError> {
    refuse_if_cancelled(&cancel)?;
    with_conn(db, read_schema).await
}

/// The blocking half of [`fetch_schema`].
fn read_schema(conn: &DuckConn) -> Result<DbSchema, DbError> {
    let database = catalog(conn)?;
    let mut namespaces: Vec<String> = conn
        .prepare(
            "SELECT schema_name FROM duckdb_schemas() \
             WHERE database_name = current_database() AND NOT internal ORDER BY 1",
        )
        .and_then(|mut s| s.query_map([], |r| r.get(0))?.collect())
        .map_err(query_err)?;
    if !namespaces
        .iter()
        .any(|s| s == schemaic_core::schema::DUCKDB_DEFAULT_SCHEMA)
    {
        namespaces.insert(0, schemaic_core::schema::DUCKDB_DEFAULT_SCHEMA.to_string());
    }

    // Tables and views, each with its stored statement and comment.
    let mut tables: Vec<TableInfo> = conn
        .prepare(
            "SELECT schema_name, table_name, sql, comment, false FROM duckdb_tables() \
               WHERE database_name = current_database() AND NOT internal \
             UNION ALL \
             SELECT schema_name, view_name, sql, comment, true FROM duckdb_views() \
               WHERE database_name = current_database() AND NOT internal \
             ORDER BY 1, 2",
        )
        .and_then(|mut s| {
            s.query_map([], |r| {
                let sql: Option<String> = r.get(2)?;
                let is_view: bool = r.get(4)?;
                Ok(TableInfo {
                    schema: Some(r.get(0)?),
                    name: r.get(1)?,
                    comment: r.get::<_, Option<String>>(3)?.filter(|c| !c.is_empty()),
                    is_view,
                    view_definition: if is_view {
                        sql.as_deref().map(view_body)
                    } else {
                        None
                    },
                    create_sql: sql,
                    ..Default::default()
                })
            })?
            .collect()
        })
        .map_err(query_err)?;
    let slot = |tables: &[TableInfo], schema: &str, name: &str| {
        tables
            .iter()
            .position(|t| t.schema.as_deref() == Some(schema) && t.name == name)
    };

    // Columns, in declared order. A **generated** column is reported by
    // `duckdb_columns()` with its expression as the default (measured:
    // `CAST((a * 2) AS INTEGER)`); only `information_schema` says which it is,
    // and it is read as generated — no default — so the grid keeps it
    // read-only, as it does every engine's.
    let mut stmt = conn
        .prepare(
            "SELECT c.schema_name, c.table_name, c.column_name, c.data_type, c.is_nullable, \
                    c.column_default, c.comment, coalesce(ic.is_generated = 'ALWAYS', false) \
               FROM duckdb_columns() c \
               LEFT JOIN information_schema.columns ic \
                 ON ic.table_catalog = c.database_name AND ic.table_schema = c.schema_name \
                AND ic.table_name = c.table_name AND ic.column_name = c.column_name \
              WHERE c.database_name = current_database() AND NOT c.internal \
              ORDER BY c.schema_name, c.table_name, c.column_index",
        )
        .map_err(query_err)?;
    let mut rows = stmt.query([]).map_err(query_err)?;
    while let Some(r) = rows.next().map_err(query_err)? {
        let schema: String = r.get(0).map_err(query_err)?;
        let table: String = r.get(1).map_err(query_err)?;
        let Some(k) = slot(&tables, &schema, &table) else {
            continue;
        };
        let default: Option<String> = r.get(5).map_err(query_err)?;
        let generated: bool = r.get(7).map_err(query_err)?;
        let type_name: String = r.get(3).map_err(query_err)?;
        let (default, generated) = if generated {
            (None, default.map(|g| generated_expression(&g, &type_name)))
        } else {
            (default, None)
        };
        tables[k].columns.push(ColumnInfo {
            generated,
            name: r.get(2).map_err(query_err)?,
            type_name,
            nullable: r.get(4).map_err(query_err)?,
            // A key counted from a sequence is DuckDB's auto-number.
            auto_increment: default
                .as_deref()
                .is_some_and(|d| d.trim_start().to_ascii_lowercase().starts_with("nextval(")),
            default,
            comment: r
                .get::<_, Option<String>>(6)
                .map_err(query_err)?
                .filter(|c| !c.is_empty()),
            ..Default::default()
        });
    }
    drop(rows);
    drop(stmt);

    // Constraints: the primary key marks its columns and is the `PRIMARY`
    // index the model reads keys from; a unique constraint is a unique index
    // named after it; a check and a foreign key are themselves.
    let mut stmt = conn
        .prepare(
            "SELECT schema_name, table_name, constraint_type, constraint_name, \
                    constraint_column_names, constraint_text, referenced_table, \
                    referenced_column_names \
               FROM duckdb_constraints() WHERE database_name = current_database() \
              ORDER BY schema_name, table_name, constraint_index",
        )
        .map_err(query_err)?;
    let mut rows = stmt.query([]).map_err(query_err)?;
    while let Some(r) = rows.next().map_err(query_err)? {
        let schema: String = r.get(0).map_err(query_err)?;
        let table: String = r.get(1).map_err(query_err)?;
        let Some(k) = slot(&tables, &schema, &table) else {
            continue;
        };
        let kind: String = r.get(2).map_err(query_err)?;
        let name: String = r
            .get::<_, Option<String>>(3)
            .map_err(query_err)?
            .unwrap_or_default();
        let cols = strings(r.get(4).map_err(query_err)?);
        let text: String = r
            .get::<_, Option<String>>(5)
            .map_err(query_err)?
            .unwrap_or_default();
        let t = &mut tables[k];
        match kind.as_str() {
            "PRIMARY KEY" => {
                for c in &mut t.columns {
                    if cols.iter().any(|n| n == &c.name) {
                        c.primary_key = true;
                    }
                }
                t.indexes.push(IndexInfo {
                    name: "PRIMARY".to_string(),
                    columns: cols.iter().map(|n| plain_column(n)).collect(),
                    unique: true,
                    constraint: Some(name),
                    ..Default::default()
                });
            }
            "UNIQUE" => t.indexes.push(IndexInfo {
                name: name.clone(),
                columns: cols.iter().map(|n| plain_column(n)).collect(),
                unique: true,
                constraint: Some(name),
                ..Default::default()
            }),
            "CHECK" => t.check_constraints.push(CheckInfo {
                name,
                expression: check_expression(&text),
                enforced: true,
                validated: true,
                ..Default::default()
            }),
            "FOREIGN KEY" => {
                let ref_table: Option<String> = r.get(6).map_err(query_err)?;
                t.foreign_keys.push(ForeignKeyInfo {
                    name,
                    columns: cols,
                    ref_schema: Some(schema.clone()),
                    ref_table: ref_table.unwrap_or_default(),
                    ref_columns: strings(r.get(7).map_err(query_err)?),
                    ..Default::default()
                });
            }
            // `NOT NULL` is the column's own nullability, already read.
            _ => {}
        }
    }
    drop(rows);
    drop(stmt);

    // Which tables other tables' keys point at: the designer's rebuild cannot
    // drop such a table (`ddl::duckdb_rebuild_sql`), and says so.
    schemaic_core::schema::link_inbound_foreign_keys(&mut tables);

    // Indexes made by `CREATE INDEX`, each with its statement.
    let mut stmt = conn
        .prepare(
            "SELECT schema_name, table_name, index_name, is_unique, expressions, sql \
               FROM duckdb_indexes() WHERE database_name = current_database() \
              ORDER BY schema_name, table_name, index_name",
        )
        .map_err(query_err)?;
    let mut rows = stmt.query([]).map_err(query_err)?;
    while let Some(r) = rows.next().map_err(query_err)? {
        let schema: String = r.get(0).map_err(query_err)?;
        let table: String = r.get(1).map_err(query_err)?;
        let Some(k) = slot(&tables, &schema, &table) else {
            continue;
        };
        let exprs = match r.get::<_, DValue>(4).map_err(query_err)? {
            DValue::Text(s) => parse_expression_list(&s),
            other => strings(other),
        };
        tables[k].indexes.push(IndexInfo {
            name: r.get(2).map_err(query_err)?,
            columns: exprs.iter().map(|e| index_column(e)).collect(),
            unique: r.get(3).map_err(query_err)?,
            create_sql: r.get(5).map_err(query_err)?,
            ..Default::default()
        });
    }
    drop(rows);
    drop(stmt);

    // Sequences.
    let sequences: Vec<SequenceInfo> = conn
        .prepare(
            "SELECT schema_name, sequence_name, start_value, increment_by, min_value, \
                    max_value, cycle, last_value, comment \
               FROM duckdb_sequences() WHERE database_name = current_database() \
              ORDER BY 1, 2",
        )
        .and_then(|mut s| {
            s.query_map([], |r| {
                Ok(SequenceInfo {
                    schema: Some(r.get(0)?),
                    name: r.get(1)?,
                    data_type: "BIGINT".to_string(),
                    start: r.get(2)?,
                    increment: r.get(3)?,
                    min_value: r.get(4)?,
                    max_value: r.get(5)?,
                    cycle: r.get(6)?,
                    last_value: r.get(7)?,
                    comment: r.get::<_, Option<String>>(8)?.filter(|c| !c.is_empty()),
                    ..Default::default()
                })
            })?
            .collect()
        })
        .map_err(query_err)?;

    // Macros — DuckDB's user functions: an expression or a table query.
    let mut routines: Vec<Arc<RoutineInfo>> = Vec::new();
    let mut stmt = conn
        .prepare(
            "SELECT schema_name, function_name, function_type, parameters, macro_definition, \
                    comment \
               FROM duckdb_functions() \
              WHERE database_name = current_database() AND NOT internal \
                AND function_type IN ('macro', 'table_macro') \
              ORDER BY 1, 2",
        )
        .map_err(query_err)?;
    let mut rows = stmt.query([]).map_err(query_err)?;
    while let Some(r) = rows.next().map_err(query_err)? {
        let schema: String = r.get(0).map_err(query_err)?;
        let name: String = r.get(1).map_err(query_err)?;
        let kind: String = r.get(2).map_err(query_err)?;
        let params = strings(r.get(3).map_err(query_err)?);
        // **One entry per name**: an overload is the same macro to `DROP
        // MACRO`, and one `CREATE MACRO` declares them all —
        // `m(a) AS a, (a, b) AS a + b`. So a later overload is folded into the
        // first entry's body in exactly that form, and `create_sql` writes the
        // whole statement back (`a_macros_ddl_replays_as_the_same_macro`).
        if let Some(first) = routines
            .iter_mut()
            .find(|x| x.name == name && x.schema.as_deref() == Some(schema.as_str()))
        {
            let def: String = r
                .get::<_, Option<String>>(4)
                .map_err(query_err)?
                .unwrap_or_default();
            let table = if kind == "table_macro" { "TABLE " } else { "" };
            let first = Arc::make_mut(first);
            first.body = format!("{}, ({}) AS {table}{}", first.body, params.join(", "), def);
            continue;
        }
        routines.push(Arc::new(RoutineInfo {
            name,
            schema: Some(schema),
            kind: RoutineKind::Function,
            arguments: params.join(", "),
            identity_arguments: params.join(", "),
            returns: if kind == "table_macro" {
                "TABLE".into()
            } else {
                String::new()
            },
            language: "SQL".to_string(),
            body: r
                .get::<_, Option<String>>(4)
                .map_err(query_err)?
                .unwrap_or_default(),
            comment: r
                .get::<_, Option<String>>(5)
                .map_err(query_err)?
                .filter(|c| !c.is_empty()),
            ..Default::default()
        }));
    }
    drop(rows);
    drop(stmt);

    Ok(DbSchema {
        tables,
        sequences,
        routines,
        database: Some(database),
        namespaces,
        names_ignore_case: true,
        ..Default::default()
    })
}

/// A key or constraint column, as the catalogue names it.
fn plain_column(name: &str) -> IndexColumn {
    IndexColumn {
        name: name.to_string(),
        ..Default::default()
    }
}

/// An index's key part: a column when it is a bare (or quoted) name, an
/// expression otherwise.
fn index_column(expr: &str) -> IndexColumn {
    let e = expr.trim();
    let unquoted = e
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .map(|s| s.replace("\"\"", "\""));
    match unquoted {
        Some(name) => plain_column(&name),
        None if !e.is_empty() && e.bytes().all(schemaic_core::sql::is_word_byte) => plain_column(e),
        None => IndexColumn {
            name: e.to_string(),
            expression: true,
            ..Default::default()
        },
    }
}

/// `duckdb_indexes().expressions` where it comes back as text — `[a, "b c"]` —
/// split at its top-level commas.
fn parse_expression_list(s: &str) -> Vec<String> {
    let inner = s
        .trim()
        .strip_prefix('[')
        .and_then(|x| x.strip_suffix(']'))
        .unwrap_or(s);
    let b = inner.as_bytes();
    let (mut out, mut start, mut depth, mut i) = (Vec::new(), 0, 0i32, 0);
    while i < b.len() {
        if let Some(next) = schemaic_core::sql::skip_noncode(b, i, D) {
            i = next;
            continue;
        }
        match b[i] {
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth -= 1,
            b',' if depth == 0 => {
                out.push(inner[start..i].trim().to_string());
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    let last = inner[start..].trim();
    if !last.is_empty() {
        out.push(last.to_string());
    }
    out
}

/// A check constraint's expression from its text, `CHECK((n > 0))` → `(n > 0)`.
fn check_expression(text: &str) -> String {
    let t = text.trim();
    let body = if t.len() >= 5 && t[..5].eq_ignore_ascii_case("CHECK") {
        t[5..].trim()
    } else {
        t
    };
    body.strip_prefix('(')
        .and_then(|b| b.strip_suffix(')'))
        .unwrap_or(body)
        .trim()
        .to_string()
}

/// A generated column's expression as written, from the text DuckDB keeps for
/// it: `CAST((a * 2) AS INTEGER)` → `(a * 2)` on an `INTEGER` column.
///
/// **DuckDB wraps the expression in a cast to the column's type each time it
/// stores one** (measured: `a * 2` reads back as `CAST((a * 2) AS INTEGER)`),
/// so restating the text it hands back wraps it again — a rebuild turned it
/// into `CAST(CAST((a * 2) AS INTEGER) AS INTEGER)`, which the designer then
/// read as an edit nobody made. The outer cast is dropped only when it spans
/// the whole text and names exactly the column's type; anything else is kept
/// as read.
fn generated_expression(text: &str, column_type: &str) -> String {
    let t = text.trim();
    let suffix = format!(" AS {column_type})");
    let Some(inner) = t
        .get(..5)
        .filter(|h| h.eq_ignore_ascii_case("CAST("))
        .and_then(|_| t.strip_suffix(suffix.as_str()))
        .map(|x| &x[5..])
    else {
        return t.to_string();
    };
    // The `(` after `CAST` must close at the very end, not earlier: in
    // `CAST(a AS INTEGER) + CAST(b AS INTEGER)` the suffix matches and the
    // first cast closes half-way.
    let b = inner.as_bytes();
    let (mut depth, mut i) = (0i32, 0);
    while i < b.len() {
        if let Some(next) = schemaic_core::sql::skip_noncode(b, i, D) {
            i = next;
            continue;
        }
        match b[i] {
            b'(' | b'[' => depth += 1,
            b')' | b']' => {
                depth -= 1;
                if depth < 0 {
                    return t.to_string();
                }
            }
            _ => {}
        }
        i += 1;
    }
    if depth == 0 {
        inner.trim().to_string()
    } else {
        t.to_string()
    }
}

// ── Writing ─────────────────────────────────────────────────────────────────

/// Run a DDL plan in one transaction, all or nothing — DuckDB's DDL is
/// transactional, as SQLite's is, so a failure leaves nothing applied.
///
/// **Stop reaches a statement already running**, through the connection's
/// interrupt handle, as `sqlite::run_ddl` and [`run_script`] do: a plan's one
/// long statement — a rebuild's copy over a large table — would otherwise
/// hold the preview modal open for its whole length with the button reading
/// *Stop* (`stop_interrupts_a_running_ddl_statement`). An interrupt surfaces
/// as an ordinary error, so the token is what tells the two apart.
pub(crate) async fn run_ddl(
    db: &Db,
    stmts: &[String],
    cancel: CancellationToken,
) -> Result<(), crate::DdlError> {
    use crate::{DdlCause, DdlError};
    // **The rebuild's working table is ours, not the user's**: an engine
    // message naming `t_schemaic_rebuild` names an object that exists only
    // between two statements of the plan, so the name is taken back out
    // (`ddl::unshadow`), as `sqlite::run_ddl` does.
    let fail = |at: usize, cause: DdlCause| DdlError {
        cause: match cause {
            DdlCause::Server(m) => DdlCause::Server(schemaic_core::ddl::unshadow(&m)),
            other => other,
        },
        at,
        applied: 0,
    };
    if cancel.is_cancelled() {
        return Err(fail(0, DdlCause::Cancelled));
    }
    let owned = stmts.to_vec();
    let db = db.clone();
    let (handle_tx, handle_rx) = tokio::sync::oneshot::channel::<Arc<duckdb::InterruptHandle>>();
    let watcher = {
        let cancel = cancel.clone();
        tokio::spawn(async move {
            let Ok(h) = handle_rx.await else { return };
            cancel.cancelled().await;
            h.interrupt();
        })
    };
    let worker_cancel = cancel.clone();
    let out = tokio::task::spawn_blocking(move || {
        let cancel = worker_cancel;
        let (_inst, conn) = connect(&db).map_err(|e| fail(0, DdlCause::Db(e)))?;
        let _ = handle_tx.send(conn.interrupt_handle());
        conn.execute_batch("BEGIN TRANSACTION")
            .map_err(|e| fail(0, DdlCause::Server(e.to_string())))?;
        for (i, s) in owned.iter().enumerate() {
            let failed = if cancel.is_cancelled() {
                Some(DdlCause::Cancelled)
            } else {
                conn.execute_batch(s).err().map(|e| {
                    if cancel.is_cancelled() {
                        DdlCause::Cancelled
                    } else {
                        DdlCause::Server(e.to_string())
                    }
                })
            };
            if let Some(cause) = failed {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(fail(i, cause));
            }
        }
        conn.execute_batch("COMMIT").map_err(|e| {
            fail(
                owned.len().saturating_sub(1),
                DdlCause::Server(e.to_string()),
            )
        })
    })
    .await
    .map_err(|e| {
        fail(
            0,
            DdlCause::Db(DbError::Refused(DbRefusal::DuckDbWorker(e.to_string()))),
        )
    });
    watcher.abort();
    out?
}

/// Run a `.sql` file's statements in order on **one connection**, as they
/// arrive — the user's own file, so nothing is wrapped around it: a `BEGIN` in
/// it is the file's to make.
pub(crate) async fn run_script(
    db: &Db,
    mut rx: tokio::sync::mpsc::Receiver<schemaic_core::script::Statement>,
    cancel: CancellationToken,
) -> (schemaic_core::script::ExecEnd, usize) {
    use schemaic_core::script::ExecEnd;
    let (tx, handle) = tokio::sync::oneshot::channel::<Arc<duckdb::InterruptHandle>>();
    let watcher = {
        let cancel = cancel.clone();
        tokio::spawn(async move {
            let Ok(h) = handle.await else { return };
            cancel.cancelled().await;
            h.interrupt();
        })
    };
    let db = db.clone();
    let loop_cancel = cancel.clone();
    let work = move || {
        let cancel = loop_cancel;
        let (_inst, conn) = match connect(&db) {
            Ok(c) => c,
            Err(e) => return (e.script_connect_end(), 0),
        };
        let _ = tx.send(conn.interrupt_handle());
        let mut ran = 0usize;
        let end = loop {
            if cancel.is_cancelled() {
                break ExecEnd::Cancelled;
            }
            let Some(st) = rx.blocking_recv() else {
                break ExecEnd::Done;
            };
            match conn.execute_batch(&st.sql) {
                Ok(()) => ran += 1,
                Err(_) if cancel.is_cancelled() => break ExecEnd::Cancelled,
                Err(e) => {
                    break ExecEnd::Failed {
                        message: e.to_string(),
                        sql: st.sql,
                        line: st.line,
                    };
                }
            }
        };
        (end, ran)
    };
    use tokio::runtime::{Handle, RuntimeFlavor};
    let out = match Handle::try_current().map(|h| h.runtime_flavor()) {
        Ok(RuntimeFlavor::MultiThread) => tokio::task::block_in_place(work),
        _ => tokio::task::spawn_blocking(work).await.unwrap_or_else(|e| {
            (
                ExecEnd::Failed {
                    message: format!("the script runner stopped unexpectedly: {e}"),
                    sql: String::new(),
                    line: 0,
                },
                0,
            )
        }),
    };
    watcher.abort();
    out
}

/// Copy the database whole into a **new** file at `target`: `ATTACH` it, `COPY
/// FROM DATABASE` the file's catalog into it — every schema, table, view,
/// sequence and macro, with the rows — and `DETACH` ([`copy_into`]). It changes
/// nothing in the source, so a read-only connection may take one. A target that
/// already exists is refused rather than overwritten, as SQLite's `VACUUM INTO`
/// refuses one: `ATTACH` would open it and copy *into* whatever it holds.
pub(crate) async fn backup_to(db: &Db, target: String) -> Result<(), DbError> {
    if std::path::Path::new(&target).exists() {
        return Err(DbError::Query(format!("{target} already exists")));
    }
    with_conn(db, move |conn| copy_into(conn, &target)).await
}

/// The file's catalog copied whole into the attached database `alias`.
fn copy_database(conn: &DuckConn, alias: &str) -> Result<(), DbError> {
    let src = schemaic_core::export::ident_sql(&catalog(conn)?, D);
    let to = schemaic_core::export::ident_sql(alias, D);
    conn.execute_batch(&format!("COPY FROM DATABASE {src} TO {to}"))
        .map_err(query_err)
}

/// [`backup_to`]'s copy, into whatever `target` names to `ATTACH` — a file, or
/// `:memory:` for the test that cannot write one.
fn copy_into(conn: &DuckConn, target: &str) -> Result<(), DbError> {
    let file = schemaic_core::export::sql_literal(&Value::Str(target.to_string()), D);
    let alias = "schemaic_backup";
    conn.execute_batch(&format!("ATTACH {file} AS {alias}"))
        .map_err(query_err)?;
    let copied = copy_database(conn, alias);
    let detached = conn
        .execute_batch(&format!("DETACH {alias}"))
        .map_err(query_err);
    copied.and(detached)
}

/// Run `f` on an operation's own connection on a blocking thread, with Stop
/// reaching a running statement through the connection's interrupt handle —
/// the grid's commit, re-fetch and blob read, as SQLite's are.
async fn with_conn_stoppable<T, F>(db: &Db, cancel: CancellationToken, f: F) -> Result<T, DbError>
where
    T: Send + 'static,
    F: FnOnce(&DuckConn) -> Result<T, DbError> + Send + 'static,
{
    refuse_if_cancelled(&cancel)?;
    let db = db.clone();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let work = tokio::task::spawn_blocking(move || {
        let (_inst, conn) = connect(&db)?;
        let _ = tx.send(conn.interrupt_handle());
        f(&conn)
    });
    let interrupt = rx.await.ok();
    tokio::select! {
        r = work => r.map_err(|e| DbError::Refused(DbRefusal::DuckDbWorker(e.to_string())))?,
        _ = cancel.cancelled() => {
            if let Some(h) = interrupt {
                h.interrupt();
            }
            Err(DbError::Cancelled)
        }
    }
}

/// A table a grid write names, as `"schema"."table"`.
///
/// **Not catalog-qualified**, and that is safe for the reason SQLite's bare
/// name is: the statement runs on a fresh connection whose catalog is the
/// file's, and [`origins_of`] attributes nothing outside it — a result read
/// after a `USE` of an attached database has no origin at all, so no write can
/// name one of its tables.
fn table_sql(schema: Option<&str>, table: &str) -> String {
    match schema {
        Some(s) => format!("{}.{}", crate::ident_duckdb(s), crate::ident_duckdb(table)),
        None => crate::ident_duckdb(table),
    }
}

/// A key value as a bound parameter. Text keys are cast by DuckDB to the
/// column's type at bind time (measured on 2.0 for integers, decimals,
/// timestamps with their zone, intervals, booleans, UUIDs and `HUGEINT`s past
/// `i64` — the forms [`cell_of`] writes them in).
fn key_param(v: &Value) -> DValue {
    match v {
        Value::Null => DValue::Null,
        Value::Int(i) => DValue::BigInt(*i),
        Value::UInt(u) => DValue::UBigInt(*u),
        Value::Float(f) => DValue::Double(*f),
        Value::Str(s) => DValue::Text(s.clone()),
    }
}

/// `col = ?` … ` AND ` …, a NULL key value compared as `IS NULL` — `= NULL`
/// matches nothing, which the one-row guard would report as a failed write
/// rather than as the wrong `WHERE`.
fn where_clause(key: &[(String, Value)], params: &mut Vec<DValue>) -> String {
    key.iter()
        .map(|(col, v)| {
            if v.is_null() {
                format!("{} IS NULL", crate::ident_duckdb(col))
            } else {
                params.push(key_param(v));
                format!("{} = ?", crate::ident_duckdb(col))
            }
        })
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// One staged cell as a bound parameter, or `None` for
/// [`CellEdit::Default`], which is the keyword `DEFAULT` in the statement —
/// DuckDB takes it in `SET` and in `VALUES` alike (measured). Text is cast to
/// the column's type by DuckDB, nested types included, which is why
/// [`nested_string`] writes a list, struct or map in the form that cast reads.
fn cell_param(v: &CellEdit) -> Option<DValue> {
    match v {
        CellEdit::Text(t) => Some(DValue::Text(t.clone())),
        CellEdit::Bytes(b) => Some(DValue::Blob(b.to_vec())),
        CellEdit::Null => Some(DValue::Null),
        CellEdit::Default => None,
    }
}

/// The SQL and bound parameters for one step of a [`GridWrite`].
fn statement_for(step: WriteStep<'_>) -> (String, Vec<DValue>) {
    let mut params = Vec::new();
    let sql = match step {
        WriteStep::Delete(d) => {
            let w = where_clause(&d.key, &mut params);
            format!(
                "DELETE FROM {} WHERE {w}",
                table_sql(d.schema.as_deref(), &d.table)
            )
        }
        WriteStep::Update(u) => {
            // The SET parameters bind before the WHERE ones, in statement order.
            let sets = u
                .set
                .iter()
                .map(|(col, val)| match cell_param(val) {
                    Some(p) => {
                        params.push(p);
                        format!("{} = ?", crate::ident_duckdb(col))
                    }
                    None => format!("{} = DEFAULT", crate::ident_duckdb(col)),
                })
                .collect::<Vec<_>>()
                .join(", ");
            let w = where_clause(&u.key, &mut params);
            format!(
                "UPDATE {} SET {sets} WHERE {w}",
                table_sql(u.schema.as_deref(), &u.table)
            )
        }
        WriteStep::Insert(i) => {
            let table = table_sql(i.schema.as_deref(), &i.table);
            if i.cols.is_empty() {
                format!("INSERT INTO {table} DEFAULT VALUES")
            } else {
                let cols: Vec<String> =
                    i.cols.iter().map(|(c, _)| crate::ident_duckdb(c)).collect();
                let values: Vec<&str> = i
                    .cols
                    .iter()
                    .map(|(_, v)| match cell_param(v) {
                        Some(p) => {
                            params.push(p);
                            "?"
                        }
                        None => "DEFAULT",
                    })
                    .collect();
                format!(
                    "INSERT INTO {table} ({}) VALUES ({})",
                    cols.join(", "),
                    values.join(", ")
                )
            }
        }
    };
    (sql, params)
}

/// Commit a batch of grid edits in **one transaction**, each statement held to
/// exactly one row — the order and the verdict are `core::model`'s
/// ([`GridWrite::plan`], [`schemaic_core::model::one_row_verdict`]), as on
/// every engine. DuckDB has no non-transactional table, so a failure rolls
/// back completely ([`schemaic_core::model::Rollback::Complete`]). Returns the rows affected, which the guard makes
/// the number of statements.
pub(crate) async fn commit_writes(
    db: &Db,
    write: &GridWrite,
    cancel: CancellationToken,
) -> Result<u64, DbError> {
    use schemaic_core::model::{Rollback, WriteCause, one_row_verdict};
    if write.is_empty() {
        return Ok(0);
    }
    let write = write.clone();
    with_conn_stoppable(db, cancel, move |conn| {
        conn.execute_batch("BEGIN TRANSACTION").map_err(query_err)?;
        let applied = (|| {
            let mut total = 0u64;
            for step in write.plan() {
                let (sql, params) = statement_for(step);
                let affected = conn
                    .execute(&sql, duckdb::params_from_iter(params))
                    .map_err(query_err)? as u64;
                one_row_verdict(step, affected).map_err(|miss| {
                    DbError::write(WriteCause::OneRow(miss), Some(Rollback::Complete))
                })?;
                total += affected;
            }
            Ok(total)
        })();
        match applied {
            Ok(total) => {
                conn.execute_batch("COMMIT").map_err(query_err)?;
                Ok(total)
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    })
    .await
}

/// Re-read the rows a commit changed, so the grid splices them in place
/// rather than re-running the query. A row no longer there is skipped — the
/// right answer to a concurrent delete is nothing to splice.
pub(crate) async fn refetch_rows(
    db: &Db,
    template: &RefetchTemplate,
    rows: &[RefetchRow],
    cancel: CancellationToken,
) -> Result<Vec<(usize, Vec<Value>)>, DbError> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let template = template.clone();
    let rows = rows.to_vec();
    with_conn_stoppable(db, cancel, move |conn| refetch_on(conn, &template, &rows)).await
}

/// The body of [`refetch_rows`], on whichever connection — a fresh one, or
/// a Manual tab's ([`pinned_refetch`]).
fn refetch_on(
    conn: &DuckConn,
    template: &RefetchTemplate,
    rows: &[RefetchRow],
) -> Result<Vec<(usize, Vec<Value>)>, DbError> {
    let cols = template
        .columns
        .iter()
        .map(|c| crate::ident_duckdb(c))
        .collect::<Vec<_>>()
        .join(", ");
    let table = table_sql(template.schema.as_deref(), &template.table);
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        // Key then confirming columns, the order `edit::refetch_key`
        // builds the values in.
        let key: Vec<(String, Value)> = template
            .key_cols
            .iter()
            .chain(template.confirm_cols.iter())
            .zip(&row.key)
            .map(|(&ci, v)| (template.columns[ci].clone(), v.clone()))
            .collect();
        let mut params = Vec::new();
        let w = where_clause(&key, &mut params);
        let mut stmt = conn
            .prepare(&format!("SELECT {cols} FROM {table} WHERE {w} LIMIT 1"))
            .map_err(query_err)?;
        let mut got = stmt
            .query(duckdb::params_from_iter(params))
            .map_err(query_err)?;
        let kinds: Vec<(LogicalTypeId, String)> = match got.as_ref() {
            Some(st) => (0..st.column_count())
                .map(|i| {
                    let lt = st.column_logical_type(i);
                    (lt.id(), type_name(&lt))
                })
                .collect(),
            None => continue,
        };
        if let Some(r) = got.next().map_err(query_err)? {
            let cells = kinds
                .iter()
                .enumerate()
                .map(|(i, (kind, name))| decode_cell(r, i, *kind, || name.clone()).map(|(v, _)| v))
                .collect::<Result<Vec<_>, _>>()?;
            out.push((row.data_row, cells));
        }
    }
    Ok(out)
}

/// Read one `BLOB` cell: its octet length and its first
/// [`schemaic_core::blob::FETCH_CAP`] bytes. `None` for a vanished row or a
/// NULL, as on every engine. DuckDB has no byte-slicing of a `BLOB` in SQL, so
/// the value comes over whole and is cut here.
pub(crate) async fn fetch_blob(
    db: &Db,
    r: &BlobRef,
    cancel: CancellationToken,
) -> Result<Option<BlobValue>, DbError> {
    let r = r.clone();
    with_conn_stoppable(db, cancel, move |conn| blob_on(conn, &r)).await
}

/// The body of [`fetch_blob`], on whichever connection.
fn blob_on(conn: &DuckConn, r: &BlobRef) -> Result<Option<BlobValue>, DbError> {
    let mut params = Vec::new();
    let w = where_clause(&r.key, &mut params);
    let col = crate::ident_duckdb(&r.column);
    let sql = format!(
        "SELECT {col} FROM {} WHERE {w} LIMIT 1",
        table_sql(r.schema.as_deref(), &r.table)
    );
    let mut stmt = conn.prepare(&sql).map_err(query_err)?;
    let mut got = stmt
        .query(duckdb::params_from_iter(params))
        .map_err(query_err)?;
    let Some(row) = got.next().map_err(query_err)? else {
        return Ok(None);
    };
    match row.get::<_, DValue>(0).map_err(query_err)? {
        DValue::Blob(mut bytes) | DValue::Geometry(mut bytes) => {
            let len = bytes.len() as u64;
            bytes.truncate(schemaic_core::blob::FETCH_CAP);
            Ok(Some(BlobValue { bytes, len }))
        }
        _ => Ok(None),
    }
}

// ── A Manual tab's pinned connection ────────────────────────────────────────

/// The one connection a Manual tab's `Session` holds, **with the instance it
/// keeps open** — and so, for a file, the file's lock, for as long as the tab
/// stays in Manual mode: no other process can open the file meanwhile.
///
/// The connection is blocking, so every call runs on the blocking pool with
/// it behind a lock of its own ([`on_pinned`]); the session's `tokio` mutex
/// above that is what orders the tab's operations. Stop is the interrupt
/// handle, which reaches the running statement from outside that lock.
pub(crate) struct Pinned {
    conn: Arc<Mutex<DuckConn>>,
    interrupt: Arc<duckdb::InterruptHandle>,
    /// The file's own catalog, read at open — what provenance compares a
    /// later `USE` against ([`origins_of`]).
    home: Option<String>,
    _instance: Arc<Instance>,
}

/// Open a Manual tab's connection ([`Pinned`]).
pub(crate) async fn pin(db: &Db) -> Result<Pinned, DbError> {
    let db = db.clone();
    tokio::task::spawn_blocking(move || {
        let (instance, conn) = connect(&db)?;
        let home = catalog(&conn).ok();
        Ok(Pinned {
            interrupt: conn.interrupt_handle(),
            conn: Arc::new(Mutex::new(conn)),
            home,
            _instance: instance,
        })
    })
    .await
    .map_err(|e| DbError::Refused(DbRefusal::DuckDbWorker(e.to_string())))?
}

/// Run `f` on the pinned connection on the blocking pool, Stop interrupting
/// it. **The worker is awaited even after a Stop**, so the next operation
/// never starts while this one still holds the connection; an interrupted
/// statement answers an error, which reads as [`DbError::Cancelled`].
async fn on_pinned<T, F>(p: &Pinned, cancel: &CancellationToken, f: F) -> Result<T, DbError>
where
    T: Send + 'static,
    F: FnOnce(&DuckConn, Option<&str>) -> Result<T, DbError> + Send + 'static,
{
    refuse_if_cancelled(cancel)?;
    let conn = p.conn.clone();
    let home = p.home.clone();
    let work = tokio::task::spawn_blocking(move || {
        let conn = conn.lock().unwrap_or_else(|e| e.into_inner());
        f(&conn, home.as_deref())
    });
    let interrupt = p.interrupt.clone();
    let watcher = {
        let cancel = cancel.clone();
        tokio::spawn(async move {
            cancel.cancelled().await;
            interrupt.interrupt();
        })
    };
    let r = work
        .await
        .map_err(|e| DbError::Refused(DbRefusal::DuckDbWorker(e.to_string())));
    watcher.abort();
    match r? {
        Err(_) if cancel.is_cancelled() => Err(DbError::Cancelled),
        other => other,
    }
}

/// Run one statement on the pinned connection, as [`fetch_query`] runs one
/// on its own.
pub(crate) async fn pinned_query(
    p: &Pinned,
    sql: &str,
    row_cap: usize,
    cancel: &CancellationToken,
) -> Result<ResultSet, DbError> {
    let sql = sql.to_string();
    on_pinned(p, cancel, move |conn, home| {
        run_query(conn, home, &sql, &mut crate::RowDest::Capped(row_cap))
    })
    .await
}

/// Run a statement of the session's own — `BEGIN`, `COMMIT`, `ROLLBACK`, or
/// the probe whose failure says the transaction is aborted.
pub(crate) async fn pinned_exec(p: &Pinned, sql: &'static str) -> Result<(), DbError> {
    on_pinned(p, &CancellationToken::new(), move |conn, _| {
        conn.execute_batch(sql).map_err(query_err)
    })
    .await
}

/// Write a batch of grid edits **inside the tab's open transaction**, each
/// statement held to exactly one row.
///
/// **DuckDB has no `SAVEPOINT`** (2.0: a parser error), so a batch that
/// fails cannot be undone alone: a statement the server refuses aborts the
/// transaction, and a statement that touched the wrong number of rows has
/// written into it. Either way the whole transaction is rolled back here —
/// the roadmap's decision — and the failure says so
/// ([`Rollback::Transaction`](schemaic_core::model::Rollback::Transaction)),
/// never that only these edits were undone: the tab's earlier statements
/// went with them. A Stop is the same loss.
pub(crate) async fn pinned_write(
    p: &Pinned,
    write: &GridWrite,
    cancel: &CancellationToken,
) -> Result<u64, DbError> {
    use schemaic_core::model::{Rollback, WriteCause, one_row_verdict};
    // **A Stop before anything is sent costs nothing**: a plain `Cancelled`,
    // the transaction untouched — the session reports it as not sent. Only a
    // Stop that lands once the batch is on its way takes the transaction.
    refuse_if_cancelled(cancel)?;
    let write = write.clone();
    let r = on_pinned(p, cancel, move |conn, _| {
        let mut total = 0u64;
        for step in write.plan() {
            let (sql, params) = statement_for(step);
            let affected = conn
                .execute(&sql, duckdb::params_from_iter(params))
                .map_err(|e| DbError::write(WriteCause::Server(e.to_string()), None))?
                as u64;
            one_row_verdict(step, affected)
                .map_err(|miss| DbError::write(WriteCause::OneRow(miss), None))?;
            total += affected;
        }
        Ok(total)
    })
    .await;
    let Err(e) = r else { return r };
    // Claimed only where the server took the `ROLLBACK`: one it refused
    // leaves nothing known, and the note says that instead.
    let undone = if pinned_exec(p, "ROLLBACK").await.is_ok() {
        Rollback::Transaction
    } else {
        Rollback::Unknown
    };
    Err(match e {
        DbError::Cancelled => DbError::write(WriteCause::CommitCancelled, Some(undone)),
        DbError::Write(mut f) => {
            f.undone = Some(undone);
            DbError::Write(f)
        }
        other => other,
    })
}

/// [`refetch_rows`] on the pinned connection, which alone sees the rows the
/// open transaction wrote.
pub(crate) async fn pinned_refetch(
    p: &Pinned,
    template: &RefetchTemplate,
    rows: &[RefetchRow],
    cancel: &CancellationToken,
) -> Result<Vec<(usize, Vec<Value>)>, DbError> {
    let (template, rows) = (template.clone(), rows.to_vec());
    on_pinned(p, cancel, move |conn, _| refetch_on(conn, &template, &rows)).await
}

/// [`fetch_blob`] on the pinned connection.
pub(crate) async fn pinned_blob(
    p: &Pinned,
    r: &BlobRef,
    cancel: &CancellationToken,
) -> Result<Option<BlobValue>, DbError> {
    let r = r.clone();
    on_pinned(p, cancel, move |conn, _| blob_on(conn, &r)).await
}

/// Run `f` off the async executor: `block_in_place` on a multi-threaded
/// runtime (the app's), inline on a current-thread one (the headless front
/// ends'), where `block_in_place` panics — `sqlite::off_executor`'s shape.
fn off_executor<T>(f: impl FnOnce() -> T) -> T {
    use tokio::runtime::{Handle, RuntimeFlavor};
    match Handle::try_current().map(|h| h.runtime_flavor()) {
        Ok(RuntimeFlavor::MultiThread) => tokio::task::block_in_place(f),
        _ => f(),
    }
}

/// Bulk-load rows into one table in **one transaction**, as batched
/// multi-row `INSERT`s from [`schemaic_core::import::build_insert_as`] — the
/// statement text the other engines run, quoting and literals included, and
/// the same guarantee: each batch must affect exactly the rows it carried, or
/// the whole import rolls back ([`Rollback::Complete`](schemaic_core::model::Rollback::Complete),
/// DuckDB having no non-transactional table).
///
/// A file's cells arrive as text, and DuckDB casts a string literal to the
/// column's type on insert — a boolean, a timestamp, a list or a struct
/// (`an_import_loads_every_batch_in_one_transaction`).
///
/// **Off the executor rather than on the blocking pool**, as SQLite's is:
/// `RowSource` is a borrowed iterator that cannot move into a `'static` task,
/// and the transaction needs one connection for every batch.
pub(crate) async fn import_rows(
    db: &Db,
    target: crate::ImportTarget<'_>,
    rows: crate::RowSource<'_>,
    cancel: CancellationToken,
) -> Result<u64, DbError> {
    use schemaic_core::import::INSERT_BATCH_ROWS;
    use schemaic_core::model::{Rollback, WriteCause};
    refuse_if_cancelled(&cancel)?;
    let (_inst, conn) = off_executor(|| connect(db))?;
    off_executor(|| {
        conn.execute_batch("BEGIN TRANSACTION").map_err(query_err)?;
        let result = (|| -> Result<u64, DbError> {
            let mut total = 0u64;
            let mut batch: Vec<Vec<Value>> = Vec::with_capacity(INSERT_BATCH_ROWS);
            let flush = |batch: &mut Vec<Vec<Value>>| -> Result<u64, DbError> {
                let Some(sql) = target.insert_sql(batch, D)? else {
                    return Ok(0);
                };
                let affected = conn.execute(&sql, []).map_err(query_err)? as u64;
                if affected != batch.len() as u64 {
                    return Err(DbError::write(
                        WriteCause::Batch {
                            rows: batch.len(),
                            wrote: affected,
                            affected: true,
                        },
                        Some(Rollback::Complete),
                    ));
                }
                batch.clear();
                Ok(affected)
            };
            for row in rows {
                if cancel.is_cancelled() {
                    return Err(DbError::Cancelled);
                }
                batch.push(row.map_err(crate::row_failure)?);
                if batch.len() >= INSERT_BATCH_ROWS {
                    total += flush(&mut batch)?;
                }
            }
            total += flush(&mut batch)?;
            Ok(total)
        })();
        match result {
            Ok(total) => {
                conn.execute_batch("COMMIT").map_err(query_err)?;
                Ok(total)
            }
            Err(e) => {
                // A failed statement has already aborted DuckDB's transaction;
                // the `ROLLBACK` ends it, and either way nothing was written.
                let _ = conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A handle on a fresh in-memory database named for the test.
    fn mem(name: &str) -> Db {
        Db::from_parts(
            crate::Engine::DuckDb,
            String::new(),
            0,
            String::new(),
            String::new(),
            format!("memory:{name}"),
        )
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap()
    }

    fn seed(db: &Db, sql: &str) {
        let (_i, c) = connect(db).unwrap();
        c.execute_batch(sql).unwrap();
    }

    async fn q(db: &Db, sql: &str) -> Result<ResultSet, DbError> {
        db.fetch_query(None, sql, 1000, CancellationToken::new())
            .await
    }

    /// A path with no file behind it is refused before DuckDB is asked —
    /// DuckDB's open would create one.
    #[test]
    fn a_missing_file_is_refused_not_created() {
        let r = open_target("C:/data/nope.duckdb", false, |_| false);
        assert!(matches!(
            r,
            Err(DbError::Refused(DbRefusal::DuckDbMissing(p))) if p == "C:/data/nope.duckdb"
        ));
        let (_, t) = open_target("C:/data/yes.duckdb", false, |_| true).unwrap();
        assert!(matches!(t, Target::File(_)));
    }

    #[test]
    fn names_that_are_not_a_file_are_refused() {
        for f in [":memory:", "md:my_db", "MotherDuck:x"] {
            assert!(
                matches!(
                    open_target(f, false, |_| true),
                    Err(DbError::Refused(DbRefusal::DuckDbNotAFile(_)))
                ),
                "{f}"
            );
        }
        assert!(matches!(
            open_target("  ", false, |_| true),
            Err(DbError::Refused(DbRefusal::DuckDbNoFile))
        ));
        // A suite's memory target is a release build's ordinary missing file.
        assert!(matches!(
            open_target("memory:x", false, |_| false),
            Err(DbError::Refused(DbRefusal::DuckDbMissing(_)))
        ));
    }

    /// Two operations open at once share the instance — a second instance on
    /// the same file is what DuckDB refuses — and each gets its own connection.
    #[test]
    fn overlapping_operations_share_one_instance() {
        let db = mem("share");
        let (a, ca) = connect(&db).unwrap();
        let (b, cb) = connect(&db).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        ca.execute_batch("CREATE TABLE t(x INT); INSERT INTO t VALUES (1)")
            .unwrap();
        let n: i64 = cb
            .query_row("SELECT count(*) FROM t", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn a_sealed_instance_reads_no_file_and_refuses_the_other_kind() {
        let open = mem("seal-open");
        let _held = connect(&open).unwrap();
        // The same file asked for sealed while it is open unsealed.
        assert!(matches!(
            connect(&open.clone().sealed()),
            Err(DbError::Refused(DbRefusal::DuckDbSealedElsewhere))
        ));
        let sealed = mem("seal-only").sealed();
        let (_i, c) = connect(&sealed).unwrap();
        let Err(e) = c.prepare("SELECT * FROM read_csv('C:/Windows/win.ini')") else {
            panic!("a sealed instance reads no file");
        };
        assert!(e.to_string().contains("disabled by configuration"), "{e}");
        assert!(
            c.execute_batch("SET enable_external_access = true")
                .is_err()
        );
    }

    #[test]
    fn no_extension_is_fetched_behind_a_query() {
        let db = mem("ext");
        let (_i, c) = connect(&db).unwrap();
        let (install, load): (bool, bool) = c
            .query_row(
                "SELECT current_setting('autoinstall_known_extensions'), \
                        current_setting('autoload_known_extensions')",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!(!install && !load);
    }

    #[test]
    fn a_read_only_run_refuses_a_write_by_its_effect() {
        let db = mem("ro");
        seed(&db, "CREATE TABLE t(x INT)");
        let rt = rt();
        let r = rt.block_on(db.fetch_query_enforced(
            None,
            "INSERT INTO t VALUES (1)",
            10,
            CancellationToken::new(),
            crate::Enforce::ReadOnly,
        ));
        assert!(r.is_err(), "{r:?}");
        let n = rt.block_on(q(&db, "SELECT count(*) AS n FROM t")).unwrap();
        assert_eq!(n.cell(0, 0).unwrap().to_value(), Value::Int(0));
        // And the next operation is not left inside that transaction.
        assert!(rt.block_on(q(&db, "INSERT INTO t VALUES (2)")).is_ok());
    }

    #[test]
    fn a_write_reports_its_count_not_a_count_column() {
        let db = mem("count");
        seed(&db, "CREATE TABLE t(x INT)");
        let rt = rt();
        let rs = rt
            .block_on(q(&db, "INSERT INTO t VALUES (1), (2), (3)"))
            .unwrap();
        assert_eq!(rs.affected, Some(3));
        assert!(rs.columns.is_empty());
        let rs = rt
            .block_on(q(&db, "INSERT INTO t VALUES (4) RETURNING x"))
            .unwrap();
        assert_eq!(rs.columns[0].name, "x");
        assert_eq!(rs.cell(0, 0).unwrap().to_value(), Value::Int(4));
    }

    #[test]
    fn values_read_as_duckdb_writes_them() {
        let db = mem("values");
        let rt = rt();
        let rs = rt
            .block_on(q(
                &db,
                "SELECT true AS b, 0.1::FLOAT AS f, 1.250::DECIMAL(10,3) AS d, \
                        12345678901234567890::HUGEINT AS h, \
                        TIMESTAMP '2020-01-02 03:04:05.5' AS ts, \
                        TIMESTAMPTZ '2020-01-02 03:04:05+02' AS tz, \
                        DATE '2020-01-01' AS dt, TIME '01:02:03' AS tm, \
                        INTERVAL 3 DAY + INTERVAL 90 MINUTE AS iv, \
                        [1, 2] AS l, ['a', 'b'] AS ls, {'a': 1, 'b': 'x'} AS s, \
                        'x'::BLOB AS bl, NULL AS n",
            ))
            .unwrap();
        let row: Vec<String> = (0..rs.columns.len())
            .map(|c| rs.cell(0, c).unwrap().display().to_string())
            .collect();
        assert_eq!(
            row,
            [
                "true",
                "0.1",
                "1.250",
                "12345678901234567890",
                "2020-01-02 03:04:05.5",
                "2020-01-02 01:04:05+00",
                "2020-01-01",
                "01:02:03",
                "3 days 01:30:00",
                "[1, 2]",
                "['a', 'b']",
                "{'a': 1, 'b': 'x'}",
                &binary_display(1),
                "NULL",
            ]
        );
        let types: Vec<&str> = rs.columns.iter().map(|c| c.type_name.as_str()).collect();
        assert_eq!(
            types,
            [
                "BOOLEAN",
                "FLOAT",
                "DECIMAL(10,3)",
                "HUGEINT",
                "TIMESTAMP",
                "TIMESTAMP WITH TIME ZONE",
                "DATE",
                "TIME",
                "INTERVAL",
                "INTEGER[]",
                "VARCHAR[]",
                "STRUCT(a INTEGER, b VARCHAR)",
                "BLOB",
                "NULL",
            ]
        );
        assert!(rs.binary_columns.contains(&12));
    }

    /// **A string inside a list, struct or map is written as DuckDB writes it
    /// and reads it back**: quoted, with a backslash before a quote or a
    /// backslash (measured on 2.0 — `'it''s'` casts back as `its`). The text
    /// is what a grid edit sends, so a form the cast misreads would write
    /// another value than the one shown.
    #[test]
    fn a_nested_string_is_escaped_the_way_duckdb_reads_it_back() {
        let db = mem("nested-escape");
        let rt = rt();
        let rs = rt
            .block_on(q(
                &db,
                r"SELECT ['it''s', 'a\b', NULL] AS l, {'it''s': 'x''y'} AS s,
                         MAP {'k''': 'v\'} AS m",
            ))
            .unwrap();
        let row: Vec<String> = (0..3)
            .map(|c| rs.cell(0, c).unwrap().display().to_string())
            .collect();
        assert_eq!(
            row,
            [
                r"['it\'s', 'a\\b', NULL]",
                r"{'it\'s': 'x\'y'}",
                r"{'k\''='v\\'}"
            ]
        );
        // And each reads back as the value it came from.
        let (_i, c) = connect(&db).unwrap();
        let same: bool = c
            .query_row(
                r#"SELECT ?::VARCHAR[] IS NOT DISTINCT FROM ['it''s', 'a\b', NULL]
                     AND ?::STRUCT("it's" VARCHAR) = {'it''s': 'x''y'}
                     AND ?::MAP(VARCHAR, VARCHAR) = MAP {'k''': 'v\'}"#,
                [&row[0], &row[1], &row[2]],
                |r| r.get(0),
            )
            .unwrap();
        assert!(same);
    }

    #[test]
    fn the_schema_reads_tables_views_keys_and_macros() {
        let db = mem("schema");
        seed(
            &db,
            "CREATE SEQUENCE s;
             CREATE TABLE p(id INTEGER PRIMARY KEY DEFAULT nextval('s'), name VARCHAR UNIQUE,
                            n INT CHECK (n > 0));
             CREATE TABLE c(id INT, p_id INT REFERENCES p(id));
             CREATE INDEX c_p ON c(p_id);
             CREATE VIEW v AS SELECT id, name FROM p;
             CREATE SCHEMA other;
             CREATE TABLE other.w(k INT);
             CREATE MACRO plus_one(x) AS x + 1;
             CREATE TABLE g(a INT, b INT GENERATED ALWAYS AS (a * 2));
             COMMENT ON TABLE p IS 'parents';",
        );
        let rt = rt();
        let s = rt
            .block_on(fetch_schema(&db, CancellationToken::new()))
            .unwrap();
        assert_eq!(s.database.as_deref(), Some("memory"));
        assert!(s.namespaces.contains(&"main".to_string()));
        assert!(s.namespaces.contains(&"other".to_string()));
        let t = |n: &str| s.tables.iter().find(|t| t.name == n).unwrap();
        let p = t("p");
        assert_eq!(p.schema.as_deref(), Some("main"));
        assert_eq!(p.comment.as_deref(), Some("parents"));
        assert!(
            p.create_sql
                .as_deref()
                .is_some_and(|q| q.starts_with("CREATE TABLE p"))
        );
        let id = &p.columns[0];
        assert!(id.primary_key && id.auto_increment && !id.nullable);
        assert!(p.indexes.iter().any(|i| i.is_primary()));
        assert!(
            p.indexes
                .iter()
                .any(|i| i.unique && i.columns[0].name == "name" && !i.is_primary())
        );
        assert_eq!(p.check_constraints.len(), 1);
        assert!(p.check_constraints[0].expression.contains("n > 0"));
        let c = t("c");
        assert_eq!(c.foreign_keys[0].ref_table, "p");
        assert_eq!(c.foreign_keys[0].columns, ["p_id"]);
        assert_eq!(c.foreign_keys[0].ref_columns, ["id"]);
        assert!(
            c.indexes
                .iter()
                .any(|i| i.name == "c_p" && i.columns[0].name == "p_id")
        );
        let v = t("v");
        assert!(v.is_view);
        assert_eq!(
            v.view_definition.as_deref(),
            Some("SELECT id, \"name\" FROM p")
        );
        assert_eq!(v.columns.len(), 2);
        assert_eq!(t("w").schema.as_deref(), Some("other"));
        // A generated column is its expression, not a default — which is how
        // `duckdb_columns()` reports it — so the grid keeps it read-only.
        let b = &t("g").columns[1];
        assert!(
            b.generated.as_deref().is_some_and(|g| g.contains("a * 2")),
            "{b:?}"
        );
        assert_eq!(b.default, None);
        assert_eq!(t("g").columns[0].generated, None);
        assert_eq!(s.sequences[0].name, "s");
        assert_eq!(s.routines[0].name, "plus_one");
        assert_eq!(s.routines[0].arguments, "x");
    }

    /// A provenance fixture: a keyed table with every kind of column, a
    /// same-named table in another schema, and a view over the first.
    fn provenance_db(name: &str) -> Db {
        let db = mem(name);
        seed(
            &db,
            "CREATE TABLE p(id INTEGER PRIMARY KEY, name VARCHAR NOT NULL, note VARCHAR DEFAULT 'x',
                            g INT GENERATED ALWAYS AS (id * 2), b BLOB, code VARCHAR UNIQUE);
             INSERT INTO p (id, name, b, code) VALUES (1, 'one', 'ab'::BLOB, 'c1');
             CREATE SCHEMA other; CREATE TABLE other.p(id INT);
             CREATE VIEW v AS SELECT * FROM p;",
        );
        db
    }

    /// Each result column's origin as `schema.table.column`, `-` for none.
    fn origins(rs: &ResultSet) -> Vec<String> {
        rs.columns
            .iter()
            .map(|c| match &c.origin {
                Some(o) => format!(
                    "{}.{}.{}",
                    o.schema.as_deref().unwrap_or("?"),
                    o.table,
                    o.column
                ),
                None => "-".to_string(),
            })
            .collect()
    }

    /// A plain read of one table is attributed, column by column and by
    /// position — an alias renames the output, not the column behind it —
    /// with the flags the grid's key choice and inserts read.
    #[test]
    fn a_single_table_read_is_attributed_by_position() {
        let db = provenance_db("prov-plain");
        let rt = rt();
        let rs = rt.block_on(q(&db, "SELECT * FROM p")).unwrap();
        assert_eq!(
            origins(&rs),
            [
                "main.p.id",
                "main.p.name",
                "main.p.note",
                "main.p.g",
                "main.p.b",
                "main.p.code"
            ]
        );
        let o = |i: usize| rs.columns[i].origin.clone().unwrap();
        assert_eq!(o(0).database, "memory");
        assert!(o(0).flags.primary_key && o(0).flags.not_null && !o(0).implicit_key);
        assert!(o(1).flags.not_null && o(1).flags.no_default);
        assert!(!o(2).flags.no_default && !o(2).flags.not_null);
        assert!(o(4).binary);
        assert!(o(5).flags.unique_key);

        let rs = rt
            .block_on(q(&db, "SELECT name AS id, id, id + 1, upper(name) FROM p"))
            .unwrap();
        assert_eq!(origins(&rs), ["main.p.name", "main.p.id", "-", "-"]);
        // Case-folded names come back in the catalogue's spelling.
        let rs = rt.block_on(q(&db, "SELECT X.ID FROM P AS x")).unwrap();
        assert_eq!(origins(&rs), ["main.p.id"]);
        let rs = rt.block_on(q(&db, "SELECT * FROM other.p")).unwrap();
        assert_eq!(origins(&rs), ["other.p.id"]);
    }

    /// Whatever the statement cannot vouch for is attributed to nothing: a
    /// view, a join, a wildcard with options, `rowid` (which a checkpoint
    /// renumbers — `a_checkpoint_renumbers_rowids_so_none_is_a_key`), and a
    /// name a temporary table or a `search_path` points elsewhere. A wrong
    /// origin is a write to another row or another table; a missing one is a
    /// read-only grid.
    #[test]
    fn what_the_statement_cannot_vouch_for_is_attributed_to_nothing() {
        let db = provenance_db("prov-refused");
        let rt = rt();
        for sql in [
            "SELECT * FROM v",
            "SELECT p.id FROM p JOIN other.p o ON o.id = p.id",
            "SELECT * EXCLUDE (note) FROM p",
            "SELECT * REPLACE (id + 1 AS id) FROM p",
            "SELECT id FROM p UNION ALL SELECT id FROM p",
            "WITH w AS (SELECT * FROM p) SELECT * FROM w",
            "SELECT * FROM (SELECT * FROM p) s",
        ] {
            let rs = rt.block_on(q(&db, sql)).unwrap();
            assert!(
                rs.columns.iter().all(|c| c.origin.is_none()),
                "{sql}: {:?}",
                origins(&rs)
            );
        }
        let rs = rt.block_on(q(&db, "SELECT rowid, id FROM p")).unwrap();
        assert_eq!(origins(&rs), ["-", "main.p.id"]);

        // Session state the statement's own connection carries.
        let batch = |stmts: &[&str]| -> ResultSet {
            let stmts: Vec<String> = stmts.iter().map(|s| s.to_string()).collect();
            let mut last = None;
            rt.block_on(
                db.run_batch(None, &stmts, 100, CancellationToken::new(), |_, r| {
                    last = Some(r)
                }),
            );
            last.unwrap().unwrap()
        };
        let rs = batch(&["CREATE TEMP TABLE p(z INT)", "SELECT * FROM p"]);
        assert_eq!(origins(&rs), ["-"]);
        let rs = batch(&["SET search_path = 'other'", "SELECT * FROM p"]);
        assert_eq!(origins(&rs), ["-"]);
        let rs = batch(&["USE memory.other", "SELECT * FROM p"]);
        assert!(
            origins(&rs).iter().all(|o| o == "-" || o == "other.p.id"),
            "{:?}",
            origins(&rs)
        );
    }

    fn edit(set: Vec<(&str, CellEdit)>, id: i64) -> schemaic_core::model::RowEdit {
        schemaic_core::model::RowEdit {
            database: "memory".into(),
            schema: Some("main".into()),
            table: "p".into(),
            set: set.into_iter().map(|(c, v)| (c.to_string(), v)).collect(),
            key: vec![("id".into(), Value::Int(id))],
        }
    }

    fn names(db: &Db, rt: &tokio::runtime::Runtime) -> Vec<String> {
        let rs = rt
            .block_on(q(
                db,
                "SELECT id || ':' || name || ':' || coalesce(note, '-') FROM p ORDER BY id",
            ))
            .unwrap();
        (0..rs.row_count())
            .map(|r| rs.cell(r, 0).unwrap().display().to_string())
            .collect()
    }

    /// A grid commit is one transaction: an update — with a staged `DEFAULT`
    /// written as the keyword — an insert and a delete, each held to one row.
    #[test]
    fn a_grid_commit_updates_inserts_and_deletes_in_one_transaction() {
        let db = provenance_db("commit");
        let rt = rt();
        let write = GridWrite {
            updates: vec![edit(
                vec![
                    ("name", CellEdit::Text("uno".into())),
                    ("note", CellEdit::Null),
                ],
                1,
            )],
            inserts: vec![schemaic_core::model::RowInsert {
                database: "memory".into(),
                schema: Some("main".into()),
                table: "p".into(),
                cols: vec![
                    ("id".into(), CellEdit::Text("2".into())),
                    ("name".into(), CellEdit::Text("it's".into())),
                    ("note".into(), CellEdit::Default),
                ],
            }],
            deletes: vec![],
        };
        let n = rt
            .block_on(commit_writes(&db, &write, CancellationToken::new()))
            .unwrap();
        assert_eq!(n, 2);
        assert_eq!(names(&db, &rt), ["1:uno:-", "2:it's:x"]);

        let write = GridWrite {
            updates: vec![edit(vec![("note", CellEdit::Default)], 1)],
            deletes: vec![schemaic_core::model::RowDelete {
                database: "memory".into(),
                schema: Some("main".into()),
                table: "p".into(),
                key: vec![("id".into(), Value::Str("2".into()))],
            }],
            ..Default::default()
        };
        rt.block_on(commit_writes(&db, &write, CancellationToken::new()))
            .unwrap();
        assert_eq!(names(&db, &rt), ["1:uno:x"]);
    }

    /// **The seam, not the halves**: this engine's provenance and its schema
    /// read, put through the grid's own `edit::analyze_edit`. A keyed table is
    /// editable by its key with its generated column read-only; a keyless one
    /// is not editable at all, its `rowid` included.
    #[test]
    fn the_grid_edits_a_keyed_table_and_not_a_keyless_one() {
        let db = provenance_db("grid-seam");
        seed(
            &db,
            "CREATE TABLE k(x INT, y VARCHAR); INSERT INTO k VALUES (1, 'a')",
        );
        let rt = rt();
        let schema = rt
            .block_on(fetch_schema(&db, CancellationToken::new()))
            .unwrap();
        let schema_for = |d: &str, s: Option<&str>, t: &str| {
            assert_eq!(d, "memory");
            schema
                .tables
                .iter()
                .find(|x| x.schema.as_deref() == s && x.name == t)
                .cloned()
        };
        let rs = rt.block_on(q(&db, "SELECT * FROM main.p")).unwrap();
        let m = schemaic_core::edit::analyze_edit(&rs, D, schema_for);
        let t = m.insert_target().expect("one keyed table");
        assert_eq!((t.table.as_str(), t.key_cols.as_slice()), ("p", &[0][..]));
        assert!(m.editable(1) && m.editable(2) && m.editable(5));
        assert!(!m.editable(3), "the generated column");

        let rs = rt.block_on(q(&db, "SELECT rowid, * FROM k")).unwrap();
        let m = schemaic_core::edit::analyze_edit(&rs, D, schema_for);
        assert!(!m.any_editable());
        assert!(m.insert_target().is_none());
    }

    fn import_into<'a>(
        schema: Option<&'a str>,
        table: &'a str,
        cols: &'a [String],
    ) -> crate::ImportTarget<'a> {
        crate::ImportTarget {
            database: "memory",
            schema,
            table,
            columns: cols,
            ..Default::default()
        }
    }

    /// An import is one transaction across all its batches, into the schema
    /// it names, with a file's text cast by DuckDB to each column's type —
    /// a boolean, a list, a timestamp arrive from a CSV as strings.
    #[test]
    fn an_import_loads_every_batch_in_one_transaction() {
        let db = mem("import");
        seed(
            &db,
            "CREATE SCHEMA other;
             CREATE TABLE other.t(id INTEGER PRIMARY KEY, v VARCHAR, ok BOOLEAN, tags INT[],
                                  seen TIMESTAMP);",
        );
        let rt = rt();
        let n = schemaic_core::import::INSERT_BATCH_ROWS + 7;
        let mut rows = (1..=n).map(|i| {
            Ok(vec![
                Value::Int(i as i64),
                Value::Str(format!("it's {i}")),
                Value::Str("true".into()),
                Value::Str("[1, 2]".into()),
                Value::Str("2020-01-02 03:04:05".into()),
            ])
        });
        let cols: Vec<String> = ["id", "v", "ok", "tags", "seen"].map(String::from).to_vec();
        let loaded = rt
            .block_on(import_rows(
                &db,
                import_into(Some("other"), "t", &cols),
                &mut rows,
                CancellationToken::new(),
            ))
            .unwrap();
        assert_eq!(loaded, n as u64);
        let rs = rt
            .block_on(q(
                &db,
                "SELECT count(*), min(v), bool_and(ok), max(len(tags)), max(seen)::VARCHAR \
                 FROM other.t",
            ))
            .unwrap();
        let row: Vec<String> = (0..5)
            .map(|c| rs.cell(0, c).unwrap().display().to_string())
            .collect();
        assert_eq!(
            row,
            [
                n.to_string().as_str(),
                "it's 1",
                "true",
                "2",
                "2020-01-02 03:04:05"
            ]
        );
    }

    /// A failure anywhere — here a duplicate key in the second batch — leaves
    /// nothing of the first behind.
    #[test]
    fn a_failed_import_leaves_the_table_as_it_was() {
        let db = mem("import-fail");
        seed(&db, "CREATE TABLE t(id INTEGER PRIMARY KEY)");
        let rt = rt();
        let n = schemaic_core::import::INSERT_BATCH_ROWS + 3;
        let mut rows = (1..=n).map(|i| Ok(vec![Value::Int(if i == n { 1 } else { i as i64 })]));
        let cols = vec!["id".to_string()];
        let e = rt.block_on(import_rows(
            &db,
            import_into(None, "t", &cols),
            &mut rows,
            CancellationToken::new(),
        ));
        assert!(e.is_err());
        let rs = rt.block_on(q(&db, "SELECT count(*) FROM t")).unwrap();
        assert_eq!(rs.cell(0, 0).unwrap().to_value(), Value::Int(0));
    }

    /// The headless front ends run on a current-thread runtime, where
    /// `block_in_place` panics.
    #[test]
    fn an_import_runs_on_a_current_thread_runtime() {
        let db = mem("import-ct");
        seed(&db, "CREATE TABLE t(id INT)");
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let mut rows = (1..=3).map(|i| Ok(vec![Value::Int(i)]));
        let cols = vec!["id".to_string()];
        let loaded = rt
            .block_on(import_rows(
                &db,
                import_into(None, "t", &cols),
                &mut rows,
                CancellationToken::new(),
            ))
            .unwrap();
        assert_eq!(loaded, 3);
    }

    /// The one-row safety net: a statement that matches no row fails the
    /// commit, and the update before it in the same batch is rolled back.
    #[test]
    fn a_write_that_misses_its_row_rolls_the_whole_commit_back() {
        let db = provenance_db("commit-miss");
        let rt = rt();
        let write = GridWrite {
            updates: vec![
                edit(vec![("name", CellEdit::Text("changed".into()))], 1),
                edit(vec![("name", CellEdit::Text("ghost".into()))], 99),
            ],
            ..Default::default()
        };
        let e = rt
            .block_on(commit_writes(&db, &write, CancellationToken::new()))
            .unwrap_err();
        assert!(matches!(e, DbError::Write(_)), "{e:?}");
        assert_eq!(names(&db, &rt), ["1:one:x"]);
    }

    #[test]
    fn rows_are_refetched_and_blobs_read_by_key() {
        let db = provenance_db("refetch");
        let rt = rt();
        let template = RefetchTemplate {
            database: "memory".into(),
            schema: Some("main".into()),
            table: "p".into(),
            columns: vec!["id".into(), "name".into(), "note".into(), "b".into()],
            key_cols: vec![0],
            confirm_cols: vec![],
        };
        let rows = vec![
            RefetchRow {
                data_row: 4,
                key: vec![Value::Int(1)],
            },
            RefetchRow {
                data_row: 5,
                key: vec![Value::Int(99)],
            },
        ];
        let got = rt
            .block_on(refetch_rows(
                &db,
                &template,
                &rows,
                CancellationToken::new(),
            ))
            .unwrap();
        assert_eq!(got.len(), 1, "the vanished row is skipped");
        assert_eq!(got[0].0, 4);
        assert_eq!(
            got[0].1,
            [
                Value::Int(1),
                Value::Str("one".into()),
                Value::Str("x".into()),
                Value::Str(binary_display(2)),
            ]
        );
        let blob = |id: i64| BlobRef {
            database: "memory".into(),
            schema: Some("main".into()),
            table: "p".into(),
            column: "b".into(),
            key: vec![("id".into(), Value::Int(id))],
        };
        let b = rt
            .block_on(fetch_blob(&db, &blob(1), CancellationToken::new()))
            .unwrap()
            .unwrap();
        assert_eq!((b.bytes.as_slice(), b.len), (&b"ab"[..], 2));
        assert!(
            rt.block_on(fetch_blob(&db, &blob(99), CancellationToken::new()))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn a_view_body_is_what_follows_its_top_level_as() {
        assert_eq!(view_body("CREATE VIEW v AS SELECT 1;"), "SELECT 1");
        assert_eq!(
            view_body("CREATE VIEW v(a, \"as\") AS SELECT 'AS' AS a, 2"),
            "SELECT 'AS' AS a, 2"
        );
        assert_eq!(view_body("CREATE VIEW alias AS SELECT 1"), "SELECT 1");
    }

    #[test]
    fn an_index_key_part_is_a_column_or_an_expression() {
        assert_eq!(
            parse_expression_list("[a, \"b c\", lower(d)]"),
            ["a", "\"b c\"", "lower(d)"]
        );
        assert!(!index_column("a").expression);
        assert_eq!(index_column("\"b c\"").name, "b c");
        assert!(index_column("lower(d)").expression);
        assert_eq!(check_expression("CHECK((n > 0))"), "(n > 0)");
    }

    #[test]
    fn a_ddl_plan_is_all_or_nothing() {
        let db = mem("ddl");
        let rt = rt();
        let e = rt
            .block_on(run_ddl(
                &db,
                &[
                    "CREATE TABLE a(x INT)".into(),
                    "CREATE TABLE a(x INT)".into(),
                ],
                CancellationToken::new(),
            ))
            .unwrap_err();
        assert_eq!(e.at, 1);
        assert!(
            rt.block_on(q(&db, "SELECT * FROM a")).is_err(),
            "rolled back"
        );
    }

    /// Stop reaches a statement already running, not only the gap before the
    /// next one — a plan's one long statement would otherwise hold the preview
    /// modal open for its whole length. The statement counts 10^11 rows, which
    /// is minutes; the interrupt makes it a fraction of a second.
    #[test]
    fn stop_interrupts_a_running_ddl_statement() {
        let db = mem("ddl-stop");
        let rt = rt();
        let cancel = CancellationToken::new();
        let later = cancel.clone();
        rt.spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            later.cancel();
        });
        let r = rt.block_on(async {
            tokio::time::timeout(
                std::time::Duration::from_secs(20),
                run_ddl(
                    &db,
                    &["CREATE TABLE big AS SELECT count(*) AS n FROM range(100000000000)".into()],
                    cancel,
                ),
            )
            .await
        });
        let outcome = r.expect("the statement was interrupted, not run to its end");
        assert!(
            matches!(
                outcome,
                Err(crate::DdlError {
                    cause: crate::DdlCause::Cancelled,
                    ..
                })
            ),
            "{outcome:?}"
        );
        assert!(rt.block_on(q(&db, "SELECT * FROM big")).is_err());
        rt.shutdown_background();
    }

    /// The copy carries the tables and their rows. Into an in-memory
    /// `ATTACH`, so no file is written; the refusal of an existing target is
    /// a path check this suite does not make.
    #[test]
    fn a_backup_copies_every_table_with_its_rows() {
        let db = mem("backup");
        seed(&db, "CREATE TABLE t(x INT); INSERT INTO t VALUES (1), (2)");
        let (_i, c) = connect(&db).unwrap();
        // The whole round trip, attach to detach…
        copy_into(&c, ":memory:").unwrap();
        // …and the copy itself, into a target kept attached to look inside.
        c.execute_batch("ATTACH ':memory:' AS probe").unwrap();
        copy_database(&c, "probe").unwrap();
        let n: i64 = c
            .query_row("SELECT count(*) FROM probe.t", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 2);
    }

    /// **Why `rowid` is never an edit key here** — the roadmap's measurement,
    /// kept. A checkpoint after a delete that empties most of a row group
    /// compacts it and renumbers what is left: the row holding 63 had rowid 63
    /// and comes back as 3, a rowid another row held before. A grid that read
    /// rowid 3 and then committed would write that other row. (Measured the
    /// same on a file, across a reopen; an `UPDATE` alone keeps the rowid.)
    #[test]
    fn a_checkpoint_renumbers_rowids_so_none_is_a_key() {
        let db = mem("rowid-moves");
        seed(
            &db,
            "CREATE TABLE t(x BIGINT); INSERT INTO t SELECT range FROM range(300000);
             CHECKPOINT; DELETE FROM t WHERE x % 20 <> 3 AND x < 250000; CHECKPOINT;",
        );
        let (_i, c) = connect(&db).unwrap();
        let r: i64 = c
            .query_row("SELECT rowid FROM t WHERE x = 63", [], |r| r.get(0))
            .unwrap();
        assert_ne!(
            r, 63,
            "rowids survived the compaction — re-measure the key decision"
        );
    }

    /// **A macro's DDL replays as the same macro**, overloads included — the
    /// text *Copy DDL* gives. Each read macro's `create_sql` is run into a
    /// fresh database and every overload is called there. (A parameter's
    /// default is not in the catalogue and is not carried; measured on 2.0,
    /// `duckdb_functions()` lists `b := 2` as `b`.)
    #[test]
    fn a_macros_ddl_replays_as_the_same_macro() {
        let db = mem("macro-ddl");
        seed(
            &db,
            "CREATE SCHEMA other;
             CREATE MACRO other.add(a, b) AS a + b;
             CREATE MACRO other.nums(n) AS TABLE SELECT * FROM range(n);
             CREATE MACRO ov(a) AS a * 10, (a, b) AS a * 10 + b;",
        );
        let rt = rt();
        let s = rt
            .block_on(fetch_schema(&db, CancellationToken::new()))
            .unwrap();
        assert_eq!(s.routines.len(), 3, "one entry per name");
        let copy = mem("macro-ddl-copy");
        seed(&copy, "CREATE SCHEMA other");
        for r in &s.routines {
            seed(&copy, &r.create_sql(D, false));
        }
        let (_i, c) = connect(&copy).unwrap();
        let got: (i64, i64, i64, i64) = c
            .query_row(
                "SELECT other.add(2, 3), (SELECT count(*) FROM other.nums(4)), ov(1), ov(1, 2)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(got, (5, 4, 10, 12));
    }

    #[test]
    fn settings_and_databases_are_listed() {
        let db = mem("vars");
        let rt = rt();
        let vars = rt.block_on(fetch_server_vars(&db)).unwrap();
        assert_eq!(vars[0].name, "version");
        assert!(vars.iter().any(|v| v.name == "threads"));
        assert_eq!(rt.block_on(fetch_databases(&db)).unwrap(), ["memory"]);
    }

    /// Every shipped DuckDB snippet runs against the linked engine — the
    /// SQLite pack's test (`every_sqlite_builtin_snippet_runs`), for the same
    /// reason. The Parquet one reads a file, which no test here makes: it has
    /// to get as far as looking for it.
    #[test]
    fn every_builtin_snippet_runs() {
        use schemaic_core::params::{Binding, ParamValue};
        use schemaic_core::{params, snippet};
        let db = mem("snippets");
        seed(
            &db,
            "CREATE TABLE album(id INTEGER PRIMARY KEY, title VARCHAR)",
        );
        let (_i, c) = connect(&db).unwrap();
        for snip in snippet::builtins(D) {
            let bindings: Vec<Binding> = params::names(&snip.body, D)
                .into_iter()
                .map(|name| Binding {
                    name,
                    value: Some(ParamValue::Text("album".to_string())),
                })
                .collect();
            let sql = params::substitute(&snip.body, &bindings, D)
                .unwrap_or_else(|e| panic!("{:?} could not be bound: {e}", snip.name));
            let ran = c.prepare(&sql).and_then(|mut s| s.query([]).map(|_| ()));
            if snip.body.contains("read_parquet") {
                let e = ran.expect_err("there is no such file");
                assert!(
                    e.to_string().contains("No files found"),
                    "{:?}: {e}",
                    snip.name
                );
            } else {
                ran.unwrap_or_else(|e| panic!("{:?} did not run: {e}\n{sql}", snip.name));
            }
        }
    }

    /// The generated catalogue in `core::duckdb_builtins` is the linked
    /// engine's — rebuilt here from `duckdb_functions()` and `duckdb_keywords()`
    /// by the module's recipe and compared, so a version bump that moves either
    /// fails here.
    #[test]
    fn the_builtin_catalogue_is_the_linked_engines() {
        let db = mem("catalogue");
        let (_i, c) = connect(&db).unwrap();
        let mut st = c
            .prepare(
                "SELECT function_name,
                        any_value(function_name || '(' || array_to_string(parameters, ', ') || ')'
                                  ORDER BY len(parameters), parameters),
                        coalesce(any_value(description ORDER BY description IS NULL,
                                           len(parameters)), '')
                   FROM duckdb_functions()
                  WHERE internal AND schema_name IN ('main', 'pg_catalog')
                    AND function_type IN ('scalar', 'aggregate', 'table', 'macro', 'table_macro')
                    AND regexp_full_match(function_name, '[a-z][a-z0-9_]*')
                  GROUP BY function_name ORDER BY function_name",
            )
            .unwrap();
        let mut want: Vec<(String, String, String)> = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .map(|r| {
                let (n, sig, d): (String, String, String) = r.unwrap();
                let sig = if sig.len() > 72 {
                    format!("{n}(…)")
                } else {
                    sig
                };
                let d = d.trim().replace(['\n', '\t'], " ");
                let d = match d.chars().next() {
                    None => "No description in `duckdb_functions()`".to_string(),
                    Some(f) => f.to_uppercase().collect::<String>() + &d[f.len_utf8()..],
                };
                (n, sig, d)
            })
            .collect();
        for n in [
            "current_time",
            "current_timestamp",
            "localtime",
            "localtimestamp",
        ] {
            if let Some(f) = schemaic_core::duckdb_builtins::DUCKDB_FUNCTIONS
                .iter()
                .find(|f| f.name == n)
                .filter(|_| !want.iter().any(|w| w.0 == n))
            {
                want.push((n.into(), f.signature.into(), f.summary.into()));
            }
        }
        want.sort();
        let have: Vec<(String, String, String)> = schemaic_core::duckdb_builtins::DUCKDB_FUNCTIONS
            .iter()
            .map(|f| (f.name.into(), f.signature.into(), f.summary.into()))
            .collect();
        assert_eq!(have.len(), want.len());
        for (h, w) in have.iter().zip(&want) {
            assert_eq!(h, w);
        }
        let words: Vec<String> = c
            .prepare(
                "SELECT upper(keyword_name) FROM duckdb_keywords() \
                 WHERE keyword_category = 'reserved' ORDER BY 1",
            )
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(schemaic_core::duckdb_builtins::DUCKDB_RESERVED, words);
    }

    /// A dump of every table in `db`, as the app's writer writes it.
    fn dump_file(db: &Db, opts: schemaic_core::dump::DumpOptions) -> String {
        use schemaic_core::dump::{DumpStep, plan, render_rows};
        let rt = rt();
        let schema = rt
            .block_on(fetch_schema(db, CancellationToken::new()))
            .unwrap();
        let chosen: Vec<String> = schema
            .tables
            .iter()
            .map(|t| schemaic_core::schema::display_name(t.schema.as_deref(), &t.name))
            .collect();
        let database = schema.database.clone().unwrap_or_default();
        let dump = plan(&schema, &database, &chosen, opts, D);
        let mut file = String::new();
        for step in dump.steps {
            match step {
                DumpStep::Text(sql) => {
                    file.push_str(&sql);
                    file.push_str("\n\n");
                }
                DumpStep::Rows {
                    database,
                    insert_database,
                    schema,
                    table,
                    select,
                    server,
                } => {
                    let rs = rt
                        .block_on(db.fetch_query(
                            Some(&database),
                            &select,
                            10_000,
                            CancellationToken::new(),
                        ))
                        .unwrap();
                    let order: Vec<usize> = (0..rs.row_count()).collect();
                    let mut out = Vec::new();
                    render_rows(
                        &mut out,
                        &mut schemaic_core::export::OneChunk::new(&rs, &order),
                        (&insert_database, schema.as_deref(), &table),
                        &server,
                        D,
                    )
                    .unwrap();
                    file.push_str(&String::from_utf8(out).unwrap());
                    file.push('\n');
                }
            }
        }
        file
    }

    /// Run `file` into `db` the way Run file does: the splitter, then
    /// `run_script` on one connection.
    fn restore_file(db: &Db, file: &str) -> schemaic_core::script::ExecEnd {
        let mut splitter = schemaic_core::script::Splitter::new(D);
        let mut stmts = splitter.push_str(file);
        stmts.extend(splitter.finish());
        let rt = rt();
        rt.block_on(async {
            let (tx, rx) = tokio::sync::mpsc::channel(stmts.len().max(1));
            for s in stmts {
                tx.send(s).await.unwrap();
            }
            drop(tx);
            run_script(db, rx, CancellationToken::new()).await.0
        })
    }

    const DUMP_FIXTURE: &str = "
        CREATE SCHEMA other;
        CREATE SEQUENCE seq_id START 5 INCREMENT BY 2;
        CREATE SEQUENCE other.oseq START 100;
        CREATE TABLE parent(id INTEGER PRIMARY KEY DEFAULT nextval('seq_id'), name VARCHAR NOT NULL UNIQUE, CHECK (length(name) > 0));
        CREATE TABLE child(id INTEGER PRIMARY KEY, parent_id INTEGER REFERENCES parent(id), note VARCHAR);
        CREATE TABLE other.o(id BIGINT DEFAULT nextval('other.oseq'), v DOUBLE, d DATE, j JSON, l INTEGER[], s STRUCT(a INT, b VARCHAR));
        CREATE INDEX child_note ON child(note);
        CREATE UNIQUE INDEX o_v ON other.o(v);
        CREATE VIEW pc AS SELECT p.name, c.note FROM parent p JOIN child c ON c.parent_id = p.id;
        CREATE MACRO other.twice(x) AS x * 2;
        COMMENT ON TABLE parent IS 'the parent''s table';
        COMMENT ON COLUMN child.note IS 'free text';
        INSERT INTO parent(name) VALUES ('a'), ('b');
        INSERT INTO child VALUES (1, 5, 'x'), (2, 7, NULL);
        INSERT INTO other.o(v, d, j, l, s) VALUES (1.5, DATE '2026-01-02', '{\"k\": 1}', [1, 2], {'a': 1, 'b': 'it''s'});
    ";

    /// What a restore has to reproduce: every table's statement, comment and
    /// rows, each column's comment, every index, and what each view reads.
    fn fingerprint(db: &Db) -> Vec<String> {
        let (_i, c) = connect(db).unwrap();
        let strings = |sql: &str| -> Vec<String> {
            let mut st = c.prepare(sql).unwrap();
            st.query_map([], |r| r.get::<_, String>(0))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        let mut out = strings(
            "SELECT schema_name || '.' || table_name || ' ' || sql || ' -- ' || \
                    coalesce(comment, '') FROM duckdb_tables() ORDER BY ALL",
        );
        out.extend(strings(
            "SELECT table_name || '.' || column_name || ' -- ' || comment FROM duckdb_columns() \
              WHERE comment IS NOT NULL AND comment <> '' ORDER BY ALL",
        ));
        out.extend(strings("SELECT sql FROM duckdb_indexes() ORDER BY ALL"));
        let names = strings(
            "SELECT schema_name || '.' || table_name FROM duckdb_tables() \
             UNION ALL SELECT schema_name || '.' || view_name FROM duckdb_views() \
               WHERE NOT internal ORDER BY ALL",
        );
        for n in names {
            out.push(format!("{n}:"));
            out.extend(strings(&format!(
                "SELECT CAST(t AS VARCHAR) FROM {n} t ORDER BY ALL"
            )));
        }
        out
    }

    fn next_key(db: &Db) -> (i64, i64) {
        let (_i, c) = connect(db).unwrap();
        c.query_row("SELECT nextval('seq_id'), nextval('other.oseq')", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap()
    }

    /// **A default dump restores into an empty database as the source was** —
    /// keys, checks and indexes, comments, every row of every type, views
    /// and macros — and each sequence goes on from where the source's was,
    /// so the next key a default draws is not one the rows already hold.
    #[test]
    fn a_dump_restores_into_an_empty_database_as_it_was() {
        use schemaic_core::script::ExecEnd;
        let src = mem("dump-src");
        seed(&src, DUMP_FIXTURE);
        let file = dump_file(&src, schemaic_core::dump::DumpOptions::default());
        let seen = fingerprint(&src).join("\n");
        for part in ["CREATE INDEX child_note", "free text", "it\\'s", "[1, 2]"] {
            assert!(seen.contains(part), "the fingerprint misses {part}: {seen}");
        }
        let dst = mem("dump-dst");
        assert_eq!(restore_file(&dst, &file), ExecEnd::Done, "{file}");
        assert_eq!(fingerprint(&dst), fingerprint(&src), "{file}");
        assert_eq!(next_key(&dst), (9, 101), "{file}");
        let (_i, c) = connect(&dst).unwrap();
        let twice: i64 = c
            .query_row("SELECT other.twice(21)", [], |r| r.get(0))
            .unwrap();
        assert_eq!(twice, 42);
        c.execute_batch("INSERT INTO parent(name) VALUES ('c')")
            .expect("a default key clears the restored rows");
    }

    /// **A replay onto the database the dump came from** puts back exactly
    /// what was there — no row doubled, no counter rewound. DuckDB cannot
    /// drop a foreign key, so the file drops each referencing table before
    /// the one it references.
    #[test]
    fn a_dump_replays_onto_its_source() {
        use schemaic_core::script::ExecEnd;
        let src = mem("dump-replay");
        seed(&src, DUMP_FIXTURE);
        let before = fingerprint(&src);
        let file = dump_file(&src, schemaic_core::dump::DumpOptions::default());
        assert_eq!(restore_file(&src, &file), ExecEnd::Done, "{file}");
        assert_eq!(fingerprint(&src), before, "{file}");
        assert_eq!(next_key(&src), (9, 101), "{file}");
    }

    /// **Without One transaction a replay stops at the first table a key
    /// points at, before dropping it** — the drop beside its `CREATE` meets
    /// the referencing table still standing — and leaves that table as it
    /// was, which is what the toggle's hint says.
    #[test]
    fn a_replay_without_a_transaction_stops_at_a_referenced_table() {
        use schemaic_core::script::ExecEnd;
        let src = mem("dump-replay-no-tx");
        seed(&src, DUMP_FIXTURE);
        let before = fingerprint(&src);
        let opts = schemaic_core::dump::DumpOptions {
            wrap_transaction: false,
            ..Default::default()
        };
        let file = dump_file(&src, opts);
        match restore_file(&src, &file) {
            ExecEnd::Failed { sql, .. } => {
                assert!(
                    sql.contains("DROP TABLE IF EXISTS \"main\".\"parent\""),
                    "{sql}"
                );
            }
            other => panic!("{other:?}\n{file}"),
        }
        assert_eq!(fingerprint(&src), before);
    }

    // ── Manual tabs ─────────────────────────────────────────────────────────

    use schemaic_core::tx::StmtOutcome;

    /// Run `sql` on `s` as a Manual tab does: `ensure_tx`, then the statement.
    async fn tab_run(s: &crate::Session, sql: &str) -> crate::Outcome<ResultSet> {
        s.ensure_tx().await.expect("BEGIN");
        s.fetch_query(sql, 100, CancellationToken::new()).await
    }

    /// **A Manual tab's statements are one transaction** no other connection
    /// sees until Commit, and Rollback undoes them.
    #[test]
    fn a_manual_tab_holds_its_writes_until_commit() {
        let db = provenance_db("manual-commit");
        let rt = rt();
        rt.block_on(async {
            let s = crate::Session::open(&db, None).await.expect("a session");
            let out = tab_run(&s, "UPDATE p SET name = 'uno' WHERE id = 1").await;
            assert_eq!(out.stmt, StmtOutcome::Ok, "{:?}", out.result);
            let seen = tab_run(&s, "SELECT name FROM p").await.result.unwrap();
            assert_eq!(seen.cell(0, 0).unwrap().display().to_string(), "uno");
        });
        assert_eq!(names(&db, &rt), ["1:one:x"], "not committed yet");
        rt.block_on(async {
            let s = crate::Session::open(&db, None).await.unwrap();
            tab_run(&s, "UPDATE p SET name = 'gone' WHERE id = 1").await;
            s.rollback().await.unwrap();
            tab_run(&s, "UPDATE p SET name = 'uno' WHERE id = 1").await;
            s.commit().await.unwrap();
        });
        assert_eq!(names(&db, &rt), ["1:uno:x"]);
    }

    /// **A failed grid commit inside a Manual transaction takes the whole
    /// transaction with it**, and says so: DuckDB has no savepoint to undo
    /// the batch alone. The tab's earlier statement is gone too, the outcome
    /// folds the pill to no transaction, and the next statement starts a new
    /// one.
    #[test]
    fn a_failed_grid_commit_rolls_the_manual_transaction_back_and_says_so() {
        use schemaic_core::model::{Rollback, WriteFailure};
        let db = provenance_db("manual-grid-miss");
        let rt = rt();
        rt.block_on(async {
            let s = crate::Session::open(&db, None).await.unwrap();
            tab_run(&s, "INSERT INTO p (id, name) VALUES (5, 'five')").await;
            let write = GridWrite {
                updates: vec![
                    edit(vec![("name", CellEdit::Text("changed".into()))], 1),
                    edit(vec![("name", CellEdit::Text("ghost".into()))], 99),
                ],
                ..Default::default()
            };
            s.ensure_tx().await.unwrap();
            let out = s.commit_writes(&write, CancellationToken::new()).await;
            assert_eq!(out.stmt, StmtOutcome::FailedAndRolledBack);
            match out.result {
                Err(DbError::Write(WriteFailure { undone, .. })) => {
                    assert_eq!(undone, Some(Rollback::Transaction));
                }
                other => panic!("{other:?}"),
            }
            // A new transaction, which commits on its own.
            let out = tab_run(&s, "INSERT INTO p (id, name) VALUES (6, 'six')").await;
            assert_eq!(out.stmt, StmtOutcome::Ok, "{:?}", out.result);
            s.commit().await.unwrap();
        });
        assert_eq!(names(&db, &rt), ["1:one:x", "6:six:x"]);
    }

    /// **A grid commit stopped before it is sent costs the transaction
    /// nothing**: reported as not sent, and the tab's earlier work commits.
    #[test]
    fn a_grid_commit_stopped_before_it_is_sent_keeps_the_transaction() {
        let db = provenance_db("manual-grid-not-sent");
        let rt = rt();
        rt.block_on(async {
            let s = crate::Session::open(&db, None).await.unwrap();
            tab_run(&s, "INSERT INTO p (id, name) VALUES (5, 'five')").await;
            let write = GridWrite {
                updates: vec![edit(vec![("name", CellEdit::Text("uno".into()))], 1)],
                ..Default::default()
            };
            let stopped = CancellationToken::new();
            stopped.cancel();
            let out = s.commit_writes(&write, stopped).await;
            assert_eq!(out.stmt, StmtOutcome::NotSent);
            assert!(
                matches!(out.result, Err(DbError::Cancelled)),
                "{:?}",
                out.result
            );
            s.commit().await.unwrap();
        });
        assert_eq!(names(&db, &rt), ["1:one:x", "5:five:x"]);
    }

    /// The same with a statement the server refuses mid-batch: a duplicate
    /// key, after an edit that succeeded.
    #[test]
    fn a_refused_grid_statement_rolls_the_manual_transaction_back_too() {
        use schemaic_core::model::{Rollback, WriteFailure};
        let db = provenance_db("manual-grid-refused");
        let rt = rt();
        rt.block_on(async {
            let s = crate::Session::open(&db, None).await.unwrap();
            tab_run(
                &s,
                "INSERT INTO p (id, name, code) VALUES (5, 'five', 'c5')",
            )
            .await;
            let write = GridWrite {
                updates: vec![
                    edit(vec![("name", CellEdit::Text("changed".into()))], 1),
                    edit(vec![("code", CellEdit::Text("c1".into()))], 5),
                ],
                ..Default::default()
            };
            s.ensure_tx().await.unwrap();
            let out = s.commit_writes(&write, CancellationToken::new()).await;
            assert_eq!(out.stmt, StmtOutcome::FailedAndRolledBack);
            assert!(
                matches!(
                    out.result,
                    Err(DbError::Write(WriteFailure {
                        undone: Some(Rollback::Transaction),
                        ..
                    }))
                ),
                "{:?}",
                out.result
            );
        });
        assert_eq!(names(&db, &rt), ["1:one:x"]);
    }

    /// A grid commit that succeeds stays inside the transaction, and the
    /// re-fetch on the pinned connection sees what it wrote.
    #[test]
    fn a_manual_grid_commit_is_refetched_on_the_pinned_connection() {
        let db = provenance_db("manual-grid-ok");
        let rt = rt();
        rt.block_on(async {
            let s = crate::Session::open(&db, None).await.unwrap();
            let write = GridWrite {
                updates: vec![edit(vec![("name", CellEdit::Text("uno".into()))], 1)],
                ..Default::default()
            };
            s.ensure_tx().await.unwrap();
            let out = s.commit_writes(&write, CancellationToken::new()).await;
            assert_eq!(out.result.unwrap(), 1);
            let template = RefetchTemplate {
                database: "memory".into(),
                schema: Some("main".into()),
                table: "p".into(),
                columns: vec!["id".into(), "name".into()],
                key_cols: vec![0],
                confirm_cols: vec![],
            };
            let rows = [RefetchRow {
                data_row: 0,
                key: vec![Value::Int(1)],
            }];
            let got = s
                .refetch_rows(&template, &rows, CancellationToken::new())
                .await
                .result
                .unwrap();
            assert_eq!(got[0].1[1], Value::Str("uno".into()));
        });
        assert_eq!(names(&db, &rt), ["1:one:x"], "never committed");
    }

    /// **A failed statement aborts the transaction, and Commit says so**
    /// rather than committing nothing under a success.
    #[test]
    fn commit_after_a_failed_statement_reports_the_abort() {
        let db = provenance_db("manual-abort");
        let rt = rt();
        rt.block_on(async {
            let s = crate::Session::open(&db, None).await.unwrap();
            tab_run(&s, "UPDATE p SET name = 'uno' WHERE id = 1").await;
            let out = tab_run(&s, "SELECT * FROM nosuch").await;
            assert_eq!(out.stmt, StmtOutcome::Failed);
            let e = s.commit().await.unwrap_err();
            assert!(
                matches!(e, DbError::Refused(DbRefusal::CommitAborted)),
                "{e:?}"
            );
        });
        assert_eq!(names(&db, &rt), ["1:one:x"]);
    }

    /// A read-only connection's Manual tab refuses a write by its effect.
    #[test]
    fn a_read_only_manual_tab_refuses_a_write() {
        let db = provenance_db("manual-read-only");
        let rt = rt();
        rt.block_on(async {
            let s = crate::Session::open_enforced(&db, None, Some(crate::Enforce::ReadOnly))
                .await
                .unwrap();
            let out = tab_run(&s, "UPDATE p SET name = 'uno' WHERE id = 1").await;
            assert!(out.result.is_err(), "{:?}", out.result);
            s.rollback().await.unwrap();
        });
        assert_eq!(names(&db, &rt), ["1:one:x"]);
    }

    /// **Stop interrupts a statement on the pinned connection — and aborts
    /// the transaction, as PostgreSQL's cancel does** (measured: the next
    /// statement answers *Current transaction is aborted*), so `Cancelled`
    /// folding to an aborted pill is the truth, and Commit says so. After
    /// Rollback the connection runs the next statement.
    #[test]
    fn stop_interrupts_a_manual_tabs_statement() {
        let db = mem("manual-stop");
        let rt = rt();
        rt.block_on(async {
            let s = crate::Session::open(&db, None).await.unwrap();
            s.ensure_tx().await.unwrap();
            let cancel = CancellationToken::new();
            let stop = cancel.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                stop.cancel();
            });
            let out = s
                .fetch_query("SELECT count(*) FROM range(100000000000) a", 10, cancel)
                .await;
            assert!(
                matches!(out.result, Err(DbError::Cancelled)),
                "{:?}",
                out.result
            );
            assert_eq!(out.stmt, StmtOutcome::Cancelled);
            assert!(
                matches!(
                    s.commit().await,
                    Err(DbError::Refused(DbRefusal::CommitAborted))
                ),
                "an interrupt aborts the transaction"
            );
            let out = tab_run(&s, "SELECT 42").await;
            assert_eq!(
                out.result
                    .unwrap()
                    .cell(0, 0)
                    .unwrap()
                    .display()
                    .to_string(),
                "42"
            );
        });
    }

    // ── The table designer ───────────────────────────────────────────────

    /// The cast DuckDB wraps round a generated expression comes off — and
    /// only that cast, spanning the whole text and naming the column's type.
    #[test]
    fn a_generated_expression_loses_only_the_cast_duckdb_added() {
        let g = generated_expression;
        assert_eq!(g("CAST((a * 2) AS INTEGER)", "INTEGER"), "(a * 2)");
        assert_eq!(
            g("CAST(CAST(x AS BIGINT) AS INTEGER)", "INTEGER"),
            "CAST(x AS BIGINT)"
        );
        // Another type is not the one DuckDB added.
        assert_eq!(
            g("CAST((a * 2) AS BIGINT)", "INTEGER"),
            "CAST((a * 2) AS BIGINT)"
        );
        // Two casts side by side: the first closes half-way.
        assert_eq!(
            g("CAST(a AS INTEGER) + CAST(b AS INTEGER)", "INTEGER"),
            "CAST(a AS INTEGER) + CAST(b AS INTEGER)"
        );
        // A bracket inside a string is not a bracket.
        assert_eq!(
            g("CAST(concat(a, ')') AS VARCHAR)", "VARCHAR"),
            "concat(a, ')')"
        );
        assert_eq!(g("a + 1", "INTEGER"), "a + 1");
    }

    /// Every cell of `sql`'s result, as displayed.
    fn cells(db: &Db, sql: &str) -> Vec<Vec<String>> {
        let rs = rt().block_on(q(db, sql)).unwrap();
        (0..rs.row_count())
            .map(|r| {
                (0..rs.columns.len())
                    .map(|c| rs.cell(r, c).unwrap().to_value().display())
                    .collect()
            })
            .collect()
    }

    /// `name` as the reader hands it to the designer.
    fn table_of(db: &Db, name: &str) -> TableInfo {
        rt().block_on(fetch_schema(db, CancellationToken::new()))
            .unwrap()
            .tables
            .into_iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| panic!("{name} is gone"))
    }

    /// The draft `d` describes, as it will read back once applied: every
    /// object named as it now is.
    fn rebased(d: &schemaic_core::ddl::TableDraft) -> schemaic_core::ddl::TableDraft {
        let mut d = d.clone();
        d.original = Some(d.name.clone());
        for c in &mut d.columns {
            c.original = Some(c.info.name.clone());
        }
        for ix in &mut d.indexes {
            ix.original = Some(ix.info.name.clone());
        }
        for fk in &mut d.foreign_keys {
            fk.original = Some(fk.info.name.clone());
        }
        for ck in &mut d.check_constraints {
            ck.original = Some(ck.info.name.clone());
        }
        d
    }

    /// Plan `edit` of `table` through `diff` → `emit`, as the designer does,
    /// run it through `run_ddl`, and check the table read back is the one the
    /// draft described — no change left between the two. Hands back the plan.
    fn designed(
        db: &Db,
        table: &str,
        edit: impl FnOnce(&mut schemaic_core::ddl::TableDraft),
    ) -> Vec<String> {
        use schemaic_core::ddl::{TableDraft, diff};
        let before = table_of(db, table);
        let mut draft = TableDraft::from_table(&before);
        edit(&mut draft);
        let cs = diff(&before, &draft, SqlDialect::DuckDb);
        assert!(
            cs.unsupported().is_empty(),
            "withheld: {:?}",
            cs.unsupported()
        );
        let stmts = cs.emit();
        assert!(!stmts.is_empty(), "an edit with nothing to run");
        rt().block_on(db.run_ddl("", &stmts, CancellationToken::new()))
            .unwrap_or_else(|e| panic!("{e} — plan {stmts:#?}"));
        let after = table_of(db, &draft.name);
        let left = diff(&after, &rebased(&draft), SqlDialect::DuckDb);
        assert!(
            left.changes.is_empty(),
            "the table read back differs from the draft: {:#?}\nplan {stmts:#?}",
            left.changes
        );
        stmts
    }

    /// A table the reader hands the designer is one the designer leaves
    /// alone: diffing it against its own draft is empty, for every kind of
    /// column and constraint the reader models.
    #[test]
    fn a_read_table_round_trips_to_no_change() {
        let db = mem("designer_round_trip");
        seed(
            &db,
            "CREATE SEQUENCE s;
             CREATE TABLE p(id INTEGER PRIMARY KEY, code VARCHAR UNIQUE);
             CREATE TABLE t(
                 id INTEGER DEFAULT nextval('s') PRIMARY KEY,
                 a INTEGER NOT NULL DEFAULT 7,
                 b VARCHAR DEFAULT 'x',
                 c DECIMAL(10, 2) CHECK (c > 0),
                 d INTEGER GENERATED ALWAYS AS (a * 2),
                 pid INTEGER REFERENCES p(id),
                 UNIQUE (a, b));
             CREATE INDEX t_b ON t(b);
             COMMENT ON TABLE t IS 'things';
             COMMENT ON COLUMN t.a IS 'the a';",
        );
        for name in ["p", "t"] {
            let t = table_of(&db, name);
            let cs = schemaic_core::ddl::diff(
                &t,
                &schemaic_core::ddl::TableDraft::from_table(&t),
                SqlDialect::DuckDb,
            );
            assert!(cs.changes.is_empty(), "{name}: {:#?}", cs.changes);
        }
    }

    /// A table nothing hangs off alters in place, one statement per change —
    /// DuckDB's `ALTER TABLE` takes one action — with no copy of the rows.
    #[test]
    fn a_plain_table_alters_in_place() {
        let db = mem("designer_in_place");
        seed(
            &db,
            "CREATE TABLE t(a INTEGER, b VARCHAR, gone INTEGER);
             INSERT INTO t VALUES (1, '10', 0), (2, '20', 0);",
        );
        let plan = designed(&db, "t", |d| {
            d.name = "t2".into();
            d.comment = Some("renamed".into());
            d.rename_column(0, "a2");
            d.columns[0].info.nullable = false;
            d.columns[0].info.comment = Some("the a".into());
            d.columns[1].info.type_name = "INTEGER".into();
            d.columns[1].info.default = Some("5".into());
            d.columns.retain(|c| c.info.name != "gone");
            d.columns
                .push(schemaic_core::ddl::ColumnDraft::new(ColumnInfo {
                    name: "n".into(),
                    type_name: "BIGINT".into(),
                    nullable: true,
                    default: Some("0".into()),
                    ..Default::default()
                }));
        });
        assert!(
            !plan.iter().any(|s| s.starts_with("CREATE TABLE")),
            "altered in place, not rebuilt: {plan:#?}"
        );
        assert!(
            plan.iter()
                .filter(|s| s.starts_with("ALTER TABLE"))
                .all(|s| !s.contains(",\n")),
            "one action per ALTER TABLE: {plan:#?}"
        );
        assert_eq!(
            cells(&db, "SELECT a2, b + 1, n FROM t2 ORDER BY a2"),
            vec![vec!["1", "11", "0"], vec!["2", "21", "0"]]
        );
    }

    /// A table with an index, a key, a check and a foreign key refuses almost
    /// every `ALTER` (*entries that depend on it*), so the plan rebuilds it:
    /// the rows, the index, the constraints and both comments come back.
    #[test]
    fn a_keyed_table_is_rebuilt_with_its_rows_indexes_and_comments() {
        let db = mem("designer_rebuild");
        seed(
            &db,
            "CREATE TABLE p(id INTEGER PRIMARY KEY);
             INSERT INTO p VALUES (1), (2);
             CREATE TABLE t(
                 id INTEGER PRIMARY KEY,
                 a INTEGER,
                 b VARCHAR UNIQUE,
                 c INTEGER CHECK (c > 0),
                 pid INTEGER REFERENCES p(id));
             CREATE INDEX t_a ON t(a);
             COMMENT ON TABLE t IS 'things';
             COMMENT ON COLUMN t.c IS 'positive';
             INSERT INTO t VALUES (1, 10, '100', 1, 1), (2, 20, '200', 2, 2);",
        );
        let plan = designed(&db, "t", |d| {
            d.rename_column(1, "a2");
            d.columns[2].info.type_name = "INTEGER".into();
        });
        assert!(
            plan.iter().any(|s| s.starts_with("CREATE TABLE")),
            "rebuilt: {plan:#?}"
        );
        let t = table_of(&db, "t");
        assert!(t.indexes.iter().any(|ix| ix.name == "t_a"), "{t:#?}");
        assert_eq!(t.comment.as_deref(), Some("things"));
        assert_eq!(t.columns[3].comment.as_deref(), Some("positive"));
        assert_eq!(t.foreign_keys.len(), 1);
        assert_eq!(t.check_constraints.len(), 1);
        assert_eq!(
            cells(&db, "SELECT sum(a2), sum(b) FROM t"),
            vec![vec!["30", "300"]]
        );
        // And the check still bites.
        assert!(
            rt().block_on(q(&db, "INSERT INTO t VALUES (3, 1, '3', 0, 1)"))
                .is_err()
        );
    }

    /// What `ALTER TABLE` has no form of at all — adding a check or a foreign
    /// key, dropping a unique constraint — is a rebuild too.
    #[test]
    fn constraints_are_added_and_dropped_by_a_rebuild() {
        let db = mem("designer_constraints");
        seed(
            &db,
            "CREATE TABLE p(id INTEGER PRIMARY KEY);
             INSERT INTO p VALUES (1);
             CREATE TABLE t(id INTEGER, u INTEGER UNIQUE, pid INTEGER);
             INSERT INTO t VALUES (1, 1, 1);",
        );
        designed(&db, "t", |d| {
            d.primary_key = vec!["id".into()];
            d.columns[0].info.primary_key = true;
            d.columns[0].info.nullable = false;
            d.indexes.clear();
            d.check_constraints.push(schemaic_core::ddl::CheckDraft {
                original: None,
                info: CheckInfo {
                    name: "t_u_check".into(),
                    expression: "(u > 0)".into(),
                    enforced: true,
                    validated: true,
                    ..Default::default()
                },
            });
            d.foreign_keys.push(schemaic_core::ddl::ForeignKeyDraft {
                original: None,
                info: ForeignKeyInfo {
                    name: "t_pid_id_fkey".into(),
                    columns: vec!["pid".into()],
                    ref_schema: Some("main".into()),
                    ref_table: "p".into(),
                    ref_columns: vec!["id".into()],
                    ..Default::default()
                },
            });
        });
        let t = table_of(&db, "t");
        assert!(t.indexes.iter().all(|ix| ix.is_primary()), "{t:#?}");
        assert!(
            rt().block_on(q(&db, "INSERT INTO t VALUES (2, 1, 9)"))
                .is_err()
        );
        assert!(
            rt().block_on(q(&db, "INSERT INTO t VALUES (2, 0, 1)"))
                .is_err()
        );
    }

    /// A table another table's foreign key references cannot be dropped, so
    /// it cannot be rebuilt either, and DuckDB has no `ADD FOREIGN KEY` to put
    /// the reference back: the plan says so instead of failing half-way.
    #[test]
    fn a_referenced_table_is_not_rebuilt() {
        let db = mem("designer_referenced");
        seed(
            &db,
            "CREATE TABLE p(id INTEGER PRIMARY KEY, v INTEGER);
             CREATE TABLE t(pid INTEGER REFERENCES p(id));",
        );
        let before = table_of(&db, "p");
        assert_eq!(before.referenced_by.len(), 1, "{before:#?}");
        let mut draft = schemaic_core::ddl::TableDraft::from_table(&before);
        draft.rename_column(1, "v2");
        let cs = schemaic_core::ddl::diff(&before, &draft, SqlDialect::DuckDb);
        assert!(!cs.unsupported().is_empty(), "{:#?}", cs.emit());
        // Adding a column is still its own statement, and still works.
        designed(&db, "p", |d| {
            let mut c = d.columns[1].info.clone();
            c.name = "w".into();
            d.columns.push(schemaic_core::ddl::ColumnDraft::new(c));
        });
    }

    /// A rebuild the engine refuses half-way names the user's table, never
    /// the working copy that only exists between two statements of the plan,
    /// and leaves the table as it was.
    #[test]
    fn a_failed_rebuild_names_the_table_not_its_working_copy() {
        use schemaic_core::ddl::{TableDraft, diff};
        let db = mem("designer_failed_rebuild");
        seed(
            &db,
            "CREATE TABLE t(id INTEGER PRIMARY KEY, v VARCHAR);
             INSERT INTO t VALUES (1, 'not a number');",
        );
        let before = table_of(&db, "t");
        let mut draft = TableDraft::from_table(&before);
        draft.columns[1].info.type_name = "INTEGER".into();
        let cs = diff(&before, &draft, SqlDialect::DuckDb);
        let err = rt()
            .block_on(db.run_ddl("", &cs.emit(), CancellationToken::new()))
            .expect_err("the copy cannot cast the text");
        let text = err.to_string();
        assert!(!text.contains("schemaic_rebuild"), "{text}");
        assert_eq!(cells(&db, "SELECT v FROM t"), vec![vec!["not a number"]]);
    }

    /// A generated column is not copied — it is computed — and comes back
    /// generated from the rebuilt table. Renaming the column it reads would
    /// leave its expression naming nothing, which the plan says up front
    /// rather than meeting as a rollback.
    #[test]
    fn a_rebuild_keeps_a_generated_column() {
        let db = mem("designer_generated");
        seed(
            &db,
            "CREATE TABLE g(id INTEGER PRIMARY KEY, a INTEGER, b INTEGER GENERATED ALWAYS AS (a * 2), n VARCHAR);
             INSERT INTO g(id, a, n) VALUES (1, 4, 'x');",
        );
        let before = table_of(&db, "g");
        let mut draft = schemaic_core::ddl::TableDraft::from_table(&before);
        draft.rename_column(1, "a2");
        let cs = schemaic_core::ddl::diff(&before, &draft, SqlDialect::DuckDb);
        assert!(
            cs.withheld_lines().iter().any(|w| matches!(
                w,
                schemaic_core::ddl::Withheld::StrandsGeneratedColumn { column, .. } if column == "b"
            )),
            "{:?}",
            cs.unsupported()
        );
        designed(&db, "g", |d| d.rename_column(3, "n2"));
        assert_eq!(cells(&db, "SELECT b FROM g"), vec![vec!["8"]]);
    }

    /// A key counted from a sequence keeps counting from it: its default is
    /// the counter, restated rather than swapped for another engine's word.
    #[test]
    fn a_sequence_default_survives_a_rebuild() {
        let db = mem("designer_sequence");
        seed(
            &db,
            "CREATE SEQUENCE s START 10;
             CREATE TABLE t(id INTEGER DEFAULT nextval('s') PRIMARY KEY, v VARCHAR);
             CREATE INDEX t_v ON t(v);
             INSERT INTO t(v) VALUES ('a');",
        );
        designed(&db, "t", |d| d.rename_column(1, "v2"));
        rt().block_on(q(&db, "INSERT INTO t(v2) VALUES ('b')"))
            .unwrap();
        assert_eq!(cells(&db, "SELECT max(id) FROM t"), vec![vec!["11"]]);
    }

    /// A new table from the designer, with every kind of constraint and an
    /// index, is created as drawn.
    #[test]
    fn a_new_table_is_created_as_drawn() {
        use schemaic_core::ddl::{ColumnDraft, IndexDraft, TableDraft};
        let db = mem("designer_new");
        seed(&db, "CREATE TABLE p(id INTEGER PRIMARY KEY);");
        let col = |name: &str, ty: &str| {
            ColumnDraft::new(ColumnInfo {
                name: name.into(),
                type_name: ty.into(),
                nullable: true,
                ..Default::default()
            })
        };
        let mut draft = TableDraft {
            name: "n".into(),
            schema: Some("main".into()),
            columns: vec![
                col("id", "INTEGER"),
                col("v", "VARCHAR"),
                col("pid", "INTEGER"),
            ],
            primary_key: vec!["id".into()],
            comment: Some("new".into()),
            ..Default::default()
        };
        draft.columns[0].info.primary_key = true;
        draft.columns[0].info.nullable = false;
        draft.columns[1].info.comment = Some("value".into());
        draft.indexes.push(IndexDraft {
            original: None,
            info: IndexInfo {
                name: "n_v".into(),
                columns: vec![schemaic_core::schema::IndexColumn::plain("v")],
                ..Default::default()
            },
        });
        draft
            .foreign_keys
            .push(schemaic_core::ddl::ForeignKeyDraft {
                original: None,
                info: ForeignKeyInfo {
                    name: "n_pid_id_fkey".into(),
                    columns: vec!["pid".into()],
                    ref_schema: Some("main".into()),
                    ref_table: "p".into(),
                    ref_columns: vec!["id".into()],
                    ..Default::default()
                },
            });
        let cs = schemaic_core::ddl::create(&draft, SqlDialect::DuckDb);
        assert!(cs.unsupported().is_empty(), "{:?}", cs.unsupported());
        rt().block_on(db.run_ddl("", &cs.emit(), CancellationToken::new()))
            .unwrap_or_else(|e| panic!("{e} — {:#?}", cs.emit()));
        let t = table_of(&db, "n");
        let left = schemaic_core::ddl::diff(&t, &rebased(&draft), SqlDialect::DuckDb);
        assert!(left.changes.is_empty(), "{:#?}", left.changes);
    }
}
