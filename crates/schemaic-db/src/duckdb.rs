//! DuckDB backend (fifth engine), built on the [`duckdb`] crate.
//!
//! Dispatched to from [`crate::Db`]'s public methods when the connection's engine
//! is [`crate::Engine::DuckDb`]. Embedded like SQLite, so most of what
//! [`crate::sqlite`]'s doc says holds here too — **there is no server**, a
//! connection is a file ([`crate::Db::file`]) and nothing else, and the driver
//! is blocking, so every call runs on Tokio's blocking pool. Four things make it
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
//! **What phase 1 of the roadmap does not do yet**, refused rather than faked:
//! grid write-back, re-fetch, blob fetch and import ([`DbRefusal::DuckDbNotYet`]),
//! column provenance (so a result is read-only), and Manual tabs
//! (`tx::supports_manual_mode`). DDL plans and `.sql` scripts run.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, Weak};
use std::time::Instant;

use duckdb::core::{LogicalTypeHandle, LogicalTypeId};
use duckdb::types::{TimeUnit, Value as DValue};
use duckdb::{AccessMode, Config, Connection as DuckConn};
use schemaic_core::blob::{BlobRef, BlobValue};
use schemaic_core::intel::SqlDialect;
use schemaic_core::model::{
    Column, DbRefusal, GridWrite, RefetchRow, RefetchTemplate, ResultBuilder, ResultSet, Value,
    binary_display,
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

/// The refusal for an operation phase 1 has not written.
fn not_yet() -> DbError {
    DbError::Refused(DbRefusal::DuckDbNotYet)
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
        if read_only && let Err(e) = conn.execute_batch("BEGIN TRANSACTION READ ONLY") {
            return (Err(query_err(e)), moved);
        }
        let rs = run_query(&conn, &sql, &mut moved);
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
        if read_only && let Err(e) = conn.execute_batch("BEGIN TRANSACTION READ ONLY") {
            let _ = res_tx.send((0usize, Err(query_err(e))));
            return;
        }
        for (i, stmt) in owned.iter().enumerate() {
            if worker_cancel.is_cancelled() {
                break;
            }
            let mut dest = crate::RowDest::Capped(row_cap);
            let r = match run_query(&conn, stmt, &mut dest) {
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

/// The blocking half of [`fetch_query`].
fn run_query(conn: &DuckConn, sql: &str, dest: &mut crate::RowDest) -> Result<ResultSet, DbError> {
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
            // A `NULL`-typed column (`SELECT NULL`) is asked nothing: the driver
            // panics decoding one (duckdb 1.20000.1, `row.rs`).
            if *kind == LogicalTypeId::SqlNull {
                cells.push(Value::Null);
                continue;
            }
            // And any other value it cannot decode costs that cell, not the
            // worker and with it the whole result.
            let got =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| row.get::<_, DValue>(i)));
            let v: DValue = match got {
                Ok(r) => r.map_err(query_err)?,
                Err(_) => {
                    cells.push(Value::Str(format!("<{}>", columns_type(&builder, i))));
                    continue;
                }
            };
            if matches!(v, DValue::Blob(_) | DValue::Geometry(_)) {
                builder.mark_binary(i);
            }
            cells.push(cell_of(v, *kind));
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
        DValue::Text(s) | DValue::Enum(s) if nested => format!("'{}'", s.replace('\'', "''")),
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
                .map(|(k, x)| format!("'{k}': {}", text_of(x, LogicalTypeId::Invalid, true)))
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

    // Columns, in declared order.
    let mut stmt = conn
        .prepare(
            "SELECT schema_name, table_name, column_name, data_type, is_nullable, \
                    column_default, comment \
               FROM duckdb_columns() WHERE database_name = current_database() AND NOT internal \
              ORDER BY schema_name, table_name, column_index",
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
        tables[k].columns.push(ColumnInfo {
            name: r.get(2).map_err(query_err)?,
            type_name: r.get(3).map_err(query_err)?,
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
        // One entry per name: an overload is the same macro to `DROP MACRO`.
        if routines
            .iter()
            .any(|x| x.name == name && x.schema.as_deref() == Some(schema.as_str()))
        {
            continue;
        }
        let kind: String = r.get(2).map_err(query_err)?;
        let params = strings(r.get(3).map_err(query_err)?);
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

// ── Writing ─────────────────────────────────────────────────────────────────

/// Run a DDL plan in one transaction, all or nothing — DuckDB's DDL is
/// transactional, as SQLite's is, so a failure leaves nothing applied.
pub(crate) async fn run_ddl(
    db: &Db,
    stmts: &[String],
    cancel: CancellationToken,
) -> Result<(), crate::DdlError> {
    use crate::{DdlCause, DdlError};
    let fail = |at: usize, cause: DdlCause| DdlError {
        cause,
        at,
        applied: 0,
    };
    if cancel.is_cancelled() {
        return Err(fail(0, DdlCause::Db(DbError::Cancelled)));
    }
    let owned = stmts.to_vec();
    let db = db.clone();
    tokio::task::spawn_blocking(move || {
        let (_inst, conn) = connect(&db).map_err(|e| fail(0, DdlCause::Db(e)))?;
        conn.execute_batch("BEGIN TRANSACTION")
            .map_err(|e| fail(0, DdlCause::Server(e.to_string())))?;
        for (i, s) in owned.iter().enumerate() {
            if let Err(e) = conn.execute_batch(s) {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(fail(i, DdlCause::Server(e.to_string())));
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
    })?
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

/// Not yet (roadmap, DuckDB phase 2): a result carries no provenance, so the
/// grid offers no edit to commit.
pub(crate) async fn commit_writes(
    _db: &Db,
    _write: &GridWrite,
    _cancel: CancellationToken,
) -> Result<u64, DbError> {
    Err(not_yet())
}

/// Not yet — see [`commit_writes`].
pub(crate) async fn refetch_rows(
    _db: &Db,
    _template: &RefetchTemplate,
    _rows: &[RefetchRow],
    _cancel: CancellationToken,
) -> Result<Vec<(usize, Vec<Value>)>, DbError> {
    Err(not_yet())
}

/// Not yet — see [`commit_writes`].
pub(crate) async fn fetch_blob(
    _db: &Db,
    _r: &BlobRef,
    _cancel: CancellationToken,
) -> Result<Option<BlobValue>, DbError> {
    Err(not_yet())
}

/// Not yet (roadmap, DuckDB phase 2); `import::supports_import` keeps the
/// wizard off DuckDB connections until it is.
pub(crate) async fn import_rows(
    _db: &Db,
    _target: crate::ImportTarget<'_>,
    _rows: crate::RowSource<'_>,
    _cancel: CancellationToken,
) -> Result<u64, DbError> {
    Err(not_yet())
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
        assert_eq!(s.sequences[0].name, "s");
        assert_eq!(s.routines[0].name, "plus_one");
        assert_eq!(s.routines[0].arguments, "x");
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
}
