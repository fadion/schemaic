//! SQLite backend (third engine), built on [`rusqlite`].
//!
//! Dispatched to from [`crate::Db`]'s public methods when the connection's engine
//! is [`crate::Engine::Sqlite`]. Four things make it unlike the other two, and
//! each shapes the code below rather than being a detail of it.
//!
//! **There is no server.** A connection is a *file* ([`crate::Db::file`]), so
//! there is no host, no port, no user, no password and nothing for an SSH tunnel
//! to reach. `fetch_databases` therefore doesn't query anything: it reports the
//! one database SQLite calls `main`. That is not a placeholder — `main` is the
//! name SQLite itself uses for the file you opened, and the name any qualified
//! reference to it must use.
//!
//! **The driver is blocking**, so every call runs inside
//! [`tokio::task::spawn_blocking`] and opens its own [`rusqlite::Connection`]
//! there. That is not a compromise imposed by rusqlite: it is exactly the
//! one-connection-per-operation invariant the other two engines already follow,
//! and on a local file the open costs microseconds. Cancellation goes through
//! [`rusqlite::Connection::get_interrupt_handle`], whose handle is `Send + Sync`
//! and is the direct analogue of MySQL's `KILL QUERY` — with the difference that
//! it needs no second connection, because it interrupts the one already running.
//!
//! **Values are dynamically typed.** SQLite has five storage classes and a column
//! declares an *affinity*, not a type, so any cell may hold any class regardless
//! of what its column says. [`value_of`] therefore reads the class of the value in
//! front of it rather than trusting the column, which is the same instinct the
//! MySQL and Postgres paths follow in taking the wire's text form: the model
//! records what the database actually returned.
//!
//! **Column provenance is not available from the driver.** MySQL gives
//! `org_table`/`org_name` on the wire and Postgres has `table_oid`/`column_id` on
//! a prepared statement; SQLite's C API has the equivalent
//! (`sqlite3_column_table_name`) but only when compiled with
//! `SQLITE_ENABLE_COLUMN_METADATA`, and **rusqlite exposes neither the flag nor
//! the call** — its `Column` carries a name and a declared type and nothing else
//! (measured against 0.32.1 and confirmed against 0.40: there is no
//! `column_metadata` feature, and `libsqlite3-sys` generates no binding). So
//! provenance has to be derived from the *statement* instead, and that is
//! deliberately conservative: anything but a plainly single-table `SELECT` leaves
//! `origin: None`, which the editing system already reads as "not editable" for an
//! expression column. Guessing wider would make a wrong `UPDATE`, which is the one
//! failure this whole layer exists to prevent. **Until that derivation lands,
//! every column here carries `None`** — a SQLite result is readable and not yet
//! editable, and the grid degrades to exactly what it does for a computed column.

use std::time::Instant;

use rusqlite::types::ValueRef;
use rusqlite::{Connection as SqliteConn, OpenFlags};
#[cfg(test)]
use schemaic_core::model::CellTag;
use schemaic_core::model::{
    Column, ColumnFlags, ColumnOrigin, GridWrite, RefetchRow, RefetchTemplate, ResultBuilder,
    ResultSet, Value, WriteStep, one_row_verdict,
};
use schemaic_core::schema::{
    ColumnInfo, DbSchema, ForeignKeyInfo, IndexColumn, IndexInfo, TableInfo,
};
use tokio_util::sync::CancellationToken;

use crate::{Db, DbError, ident_sqlite};

/// The one database name SQLite gives the file you opened.
///
/// Exposed because it is not a label this layer invented: `main` is what
/// `PRAGMA database_list` reports, what a qualified `main.t` resolves through,
/// and what `ATTACH` distinguishes other files from. The app's "database" tree
/// level shows this single entry.
pub const MAIN: &str = "main";

/// Map a rusqlite error onto the app's error type.
///
/// Everything that isn't a connection failure is a query failure, matching how
/// the other two backends split them: the distinction the UI draws is "couldn't
/// reach it" versus "it said no".
fn query_err(e: rusqlite::Error) -> DbError {
    DbError::Query(e.to_string())
}

/// Open the file this handle points at.
///
/// **`SQLITE_OPEN_CREATE` is deliberately absent.** rusqlite's `open` creates a
/// missing file, which for a database *client* is the wrong default by some
/// distance: a mistyped path would silently produce an empty database and present
/// it as a connection that worked, and the user would go looking for their tables
/// in a file that never had any. A missing file is an error here, and says so.
fn open(db: &Db) -> Result<SqliteConn, DbError> {
    if db.file.trim().is_empty() {
        return Err(DbError::Connect("no database file is set".to_string()));
    }
    SqliteConn::open_with_flags(
        &db.file,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|e| DbError::Connect(format!("{}: {e}", db.file)))
}

/// Run `f` against a fresh connection on a blocking thread.
///
/// Every entry point in this module funnels through here, so "one connection per
/// operation" is a property of the module rather than a habit each function has
/// to remember. The closure gets `&mut` so it can open a transaction.
async fn with_conn<T, F>(db: &Db, f: F) -> Result<T, DbError>
where
    T: Send + 'static,
    F: FnOnce(&mut SqliteConn) -> Result<T, DbError> + Send + 'static,
{
    let db = db.clone();
    tokio::task::spawn_blocking(move || {
        let mut conn = open(&db)?;
        f(&mut conn)
    })
    .await
    .map_err(|e| DbError::Query(format!("worker failed: {e}")))?
}

/// One cell, read as the storage class SQLite actually returned.
///
/// A declared type is only an *affinity* in SQLite — a `TEXT` column can hold an
/// integer and often does — so this reads the value in front of it rather than
/// what the column claimed.
///
/// A **BLOB has no lossless text form**, and the model's `Value` has no bytes
/// variant, so one is rendered as its size rather than as mojibake or as a hex
/// string long enough to hang the grid. The editing system independently refuses
/// to write a binary column, so nothing round-trips this text back into the
/// database; it is a display, and it says what it is.
fn value_of(raw: ValueRef<'_>) -> Value {
    match raw {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => Value::Int(i),
        ValueRef::Real(f) => Value::Float(f),
        // Invalid UTF-8 in a TEXT cell is possible (SQLite doesn't validate), and
        // losing the row to it would be worse than showing the replacement chars.
        ValueRef::Text(b) => Value::Str(String::from_utf8_lossy(b).into_owned()),
        ValueRef::Blob(b) => Value::Str(format!("<{} bytes>", b.len())),
    }
}

/// The declared type of each result column, or an empty string where the
/// statement computed the value rather than reading it from a column.
///
/// The grid renders this under the column name and uses its leading token to
/// decide numeric right-alignment, so an expression's blank is the honest answer:
/// SQLite genuinely does not assign a type to `count(*)` until it has run.
fn columns_of(stmt: &rusqlite::Statement<'_>) -> Vec<Column> {
    stmt.columns()
        .iter()
        .map(|c| Column {
            name: c.name().to_string(),
            type_name: c.decl_type().unwrap_or_default().to_string(),
            origin: None,
        })
        .collect()
}

/// Fill in each result column's `origin`, so the editing system can decide what
/// is writable — the statement-derived stand-in for the provenance MySQL reads
/// off the wire and PostgreSQL off a prepared statement's `table_oid`.
///
/// Everything about it is deliberately conservative, because the failure it
/// guards against is an `UPDATE` aimed at the wrong row or the wrong column:
///
/// - the statement must be a plainly single-table `SELECT`
///   ([`schemaic_core::intel::projection_of`]) — a join, a CTE, a subquery or a
///   set operation leaves every column unattributed and the result read-only;
/// - a **view** is skipped. SQLite will not accept a write to one without an
///   `INSTEAD OF` trigger, and offering an edit that the server refuses at commit
///   time is worse than not offering it;
/// - a qualifier other than `main` is skipped, since an `ATTACH`ed database is a
///   different file and nothing here has introspected it;
/// - the projection is placed **positionally**, never matched by name — see
///   `projection_of` for the aliasing case that makes the difference.
///
/// Flags come from the table's own pragmas, so `analyze_edit` gets the same
/// material it gets from the other two engines and needs no SQLite-specific
/// branch.
fn attach_origins(conn: &SqliteConn, sql: &str, columns: &mut [Column]) {
    use schemaic_core::intel::{Projection, SqlDialect, projection_of};

    let Some((source, projection)) = projection_of(sql, SqlDialect::Sqlite) else {
        return;
    };
    if source
        .qualifier
        .as_deref()
        .is_some_and(|q| !q.eq_ignore_ascii_case(MAIN))
    {
        return;
    }
    if is_view(conn, &source.name).unwrap_or(true) {
        return;
    }
    let Ok(info) = table_columns(conn, &source.name) else {
        return;
    };
    let unique = single_column_unique_indexes(conn, &source.name).unwrap_or_default();

    // The base column each result column reads, by position.
    let bases: Vec<Option<String>> = match projection {
        Projection::Wildcard => info.iter().map(|c| Some(c.name.clone())).collect(),
        Projection::Items(items) => items,
        // The wildcard expands into whatever follows the placed leading items —
        // `SELECT rowid, * FROM t` is one of these. The width check below is what
        // keeps the expansion honest.
        Projection::LeadingThenWildcard(lead) => lead
            .into_iter()
            .chain(info.iter().map(|c| Some(c.name.clone())))
            .collect(),
    };
    // A wildcard's width has to agree with the table's, or the placement is off —
    // which happens for real when a generated column is present, since SQLite
    // omits a VIRTUAL column from `SELECT *` in some versions but `table_xinfo`
    // always lists it.
    if bases.len() != columns.len() {
        return;
    }

    for (col, base) in columns.iter_mut().zip(bases) {
        let Some(base) = base else { continue };
        let Some(ci) = info.iter().find(|c| c.name.eq_ignore_ascii_case(&base)) else {
            continue;
        };
        col.origin = Some(ColumnOrigin {
            database: MAIN.to_string(),
            // SQLite has no namespace level; `main` is the database, not a schema.
            schema: None,
            table: source.name.clone(),
            column: ci.name.clone(),
            flags: ColumnFlags {
                primary_key: ci.primary_key,
                unique_key: unique.iter().any(|u| u.eq_ignore_ascii_case(&ci.name)),
                not_null: !ci.nullable,
                auto_increment: ci.auto_increment,
                // A new row must supply this column or the INSERT fails. Nullable
                // columns have an implicit NULL default, and a rowid alias fills
                // itself in, so neither counts.
                no_default: !ci.nullable
                    && !ci.auto_increment
                    && ci.default.is_none()
                    && ci.generated.is_none(),
            },
            // A BLOB cell is rendered as its size and cannot round-trip, so the
            // editing system must treat such a column as read-only — the same call
            // the other two engines make for a binary charset. It is the
            // *declared* type that decides, since that is what the column is for.
            binary: ci.type_name.eq_ignore_ascii_case("BLOB"),
            implicit_key: false,
        });
    }
}

/// Is this name a view rather than a base table? `None` when it is neither.
fn is_view(conn: &SqliteConn, name: &str) -> Result<bool, DbError> {
    let kind: Option<String> = conn
        .query_row(
            "SELECT type FROM sqlite_master WHERE name = ?1 AND type IN ('table','view')",
            [name],
            |r| r.get(0),
        )
        .ok();
    match kind.as_deref() {
        Some("view") => Ok(true),
        Some(_) => Ok(false),
        // Not found — treated as "don't attribute", by the caller's `unwrap_or(true)`.
        None => Err(DbError::Query(format!("no such table: {name}"))),
    }
}

/// Columns covered by a single-column UNIQUE index — what the editing system can
/// use as a `WHERE` key when there is no primary key.
fn single_column_unique_indexes(conn: &SqliteConn, table: &str) -> Result<Vec<String>, DbError> {
    let mut out = Vec::new();
    for ix in table_indexes(conn, table)? {
        if ix.unique && ix.columns.len() == 1 {
            out.push(ix.columns[0].name.clone());
        }
    }
    Ok(out)
}

/// Run `sql` and collect up to `row_cap` rows.
///
/// Cancellation is armed *before* the statement runs and disarmed after: the
/// interrupt handle is cloned out to the async side, which calls it when the token
/// fires. SQLite then returns `SQLITE_INTERRUPT` from the step in flight, which
/// arrives here as an ordinary error and is reported as [`DbError::Cancelled`]
/// rather than as a failure — the user asked for this one.
pub(crate) async fn fetch_query(
    db: &Db,
    sql: &str,
    row_cap: usize,
    cancel: CancellationToken,
) -> Result<ResultSet, DbError> {
    let sql = sql.to_string();
    let db = db.clone();
    let (tx, rx) = tokio::sync::oneshot::channel();

    let work = tokio::task::spawn_blocking(move || {
        let conn = open(&db)?;
        // Hand the interrupt handle to the async side before doing any work.
        let _ = tx.send(conn.get_interrupt_handle());
        run_query(&conn, &sql, row_cap)
    });

    // The handle arrives as soon as the connection is open; if the blocking task
    // failed before sending, the `rx` error is not the interesting one — the task's
    // own result is, so it is simply awaited.
    let interrupt = rx.await.ok();
    tokio::select! {
        r = work => r.map_err(|e| DbError::Query(format!("worker failed: {e}")))?,
        _ = cancel.cancelled() => {
            if let Some(h) = interrupt {
                h.interrupt();
            }
            Err(DbError::Cancelled)
        }
    }
}

/// The blocking half of [`fetch_query`].
///
/// A statement that returns no rows still has to be told apart from one that
/// returns none *of a row-bearing shape*: `stmt.column_count() == 0` is SQLite's
/// answer for `INSERT`/`UPDATE`/`DELETE`/DDL, and those report `affected` instead,
/// exactly as the other two engines do.
fn run_query(conn: &SqliteConn, sql: &str, row_cap: usize) -> Result<ResultSet, DbError> {
    let start = Instant::now();
    let mut stmt = conn.prepare(sql).map_err(query_err)?;

    if stmt.column_count() == 0 {
        drop(stmt);
        let affected = conn.execute(sql, []).map_err(query_err)?;
        let mut rs = ResultSet::default();
        rs.affected = Some(affected as u64);
        rs.elapsed_ms = start.elapsed().as_millis();
        return Ok(rs);
    }

    let mut columns = columns_of(&stmt);
    attach_origins(conn, sql, &mut columns);
    let ncols = columns.len();
    let mut builder = ResultBuilder::new(columns);
    let mut rows = stmt.query([]).map_err(query_err)?;
    let mut cells: Vec<Value> = Vec::with_capacity(ncols);
    let mut truncated = false;

    while let Some(row) = rows.next().map_err(query_err)? {
        if builder.row_count() >= row_cap {
            truncated = true;
            break;
        }
        cells.clear();
        for i in 0..ncols {
            cells.push(value_of(row.get_ref(i).map_err(query_err)?));
        }
        builder.push_row(&cells);
    }

    let mut rs = builder.finish();
    rs.truncated = truncated;
    rs.elapsed_ms = start.elapsed().as_millis();
    Ok(rs)
}

// ── Write-back ───────────────────────────────────────────────────────────────

/// Bind a [`Value`] as a parameter.
///
/// `Value::Str` covers everything the text protocols hand back, and SQLite's
/// dynamic typing means binding it as text is not the lossy choice it would be
/// elsewhere: the comparison in a `WHERE` applies the column's affinity to the
/// bound value, so `WHERE id = '3'` finds the row whose `id` is the integer 3.
fn bind(v: &Value) -> rusqlite::types::Value {
    use rusqlite::types::Value as Sq;
    match v {
        Value::Null => Sq::Null,
        Value::Int(i) => Sq::Integer(*i),
        // The model's u64 exceeds i64 only above 2^63, which no SQLite integer
        // can hold anyway; the lossy case is unreachable from a SQLite result.
        Value::UInt(u) => i64::try_from(*u)
            .map(Sq::Integer)
            .unwrap_or(Sq::Real(*u as f64)),
        Value::Float(f) => Sq::Real(*f),
        Value::Str(s) => Sq::Text(s.clone()),
    }
}

/// `col = ?` … ` AND ` …, with a NULL key column compared as `IS NULL`.
///
/// `= NULL` is never true in SQL, so a key column holding NULL would silently
/// match no rows — which the 1-row guard would then report as a failed write
/// rather than as the wrong `WHERE`. It arises for real here: SQLite allows NULL
/// in a non-`INTEGER` `PRIMARY KEY`.
fn where_clause(key: &[(String, Value)], params: &mut Vec<rusqlite::types::Value>) -> String {
    key.iter()
        .map(|(col, v)| {
            if v.is_null() {
                format!("{} IS NULL", ident_sqlite(col))
            } else {
                params.push(bind(v));
                format!("{} = ?", ident_sqlite(col))
            }
        })
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// Commit a batch of grid edits in one transaction, each statement required to
/// affect exactly one row.
///
/// The order and the per-statement verdict both come from `core::model`
/// ([`GridWrite::plan`], [`one_row_verdict`]) rather than from here, so the
/// promise cannot drift between the three engines.
///
/// **The rollback is unconditional and complete**, which is the one way this
/// engine is simpler than MySQL: SQLite has no non-transactional table type, so
/// there is no `Rollback::note` case where the rollback succeeds without
/// achieving anything. A failure here really does leave the file untouched.
/// Returns the total number of rows affected, which the 1-row guard makes equal
/// to the number of statements — the caller reports it either way.
pub(crate) async fn commit_writes(
    db: &Db,
    write: &GridWrite,
    cancel: CancellationToken,
) -> Result<u64, DbError> {
    if write.is_empty() {
        return Ok(0);
    }
    let write = write.clone();
    let db = db.clone();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let work = tokio::task::spawn_blocking(move || {
        let mut conn = open(&db)?;
        let _ = tx.send(conn.get_interrupt_handle());
        // Foreign keys are **off by default** in SQLite, per connection. A grid
        // delete that orphans rows would otherwise succeed here and fail nowhere,
        // which is not what the table declares.
        let _ = conn.execute_batch("PRAGMA foreign_keys = ON");
        let txn = conn.transaction().map_err(query_err)?;
        let mut total = 0u64;
        for step in write.plan() {
            let (sql, params) = statement_for(step);
            let affected = txn
                .execute(&sql, rusqlite::params_from_iter(params))
                .map_err(query_err)? as u64;
            one_row_verdict(step, affected).map_err(DbError::Query)?;
            total += affected;
        }
        txn.commit().map_err(query_err)?;
        Ok(total)
    });

    let interrupt = rx.await.ok();
    tokio::select! {
        r = work => r.map_err(|e| DbError::Query(format!("worker failed: {e}")))?,
        _ = cancel.cancelled() => {
            if let Some(h) = interrupt { h.interrupt(); }
            Err(DbError::Cancelled)
        }
    }
}

/// The SQL and bound parameters for one step of a [`GridWrite`].
///
/// The table is named **bare**, not `main.t`: a connection is one file, so there
/// is nothing to disambiguate, and `main` would be wrong if this statement were
/// ever run somewhere the file is attached under another name.
fn statement_for(step: WriteStep<'_>) -> (String, Vec<rusqlite::types::Value>) {
    let mut params = Vec::new();
    let sql = match step {
        WriteStep::Delete(d) => {
            let w = where_clause(&d.key, &mut params);
            format!("DELETE FROM {} WHERE {w}", ident_sqlite(&d.table))
        }
        WriteStep::Update(u) => {
            // The SET parameters bind before the WHERE ones, matching the order
            // they appear in the statement.
            let sets = u
                .set
                .iter()
                .map(|(col, val)| {
                    params.push(match val {
                        Some(t) => rusqlite::types::Value::Text(t.clone()),
                        None => rusqlite::types::Value::Null,
                    });
                    format!("{} = ?", ident_sqlite(col))
                })
                .collect::<Vec<_>>()
                .join(", ");
            let w = where_clause(&u.key, &mut params);
            format!("UPDATE {} SET {sets} WHERE {w}", ident_sqlite(&u.table))
        }
        WriteStep::Insert(i) => {
            let cols = i
                .cols
                .iter()
                .map(|(col, val)| {
                    params.push(match val {
                        Some(t) => rusqlite::types::Value::Text(t.clone()),
                        None => rusqlite::types::Value::Null,
                    });
                    ident_sqlite(col)
                })
                .collect::<Vec<_>>();
            if cols.is_empty() {
                // Every column left to its default. SQLite spells that
                // `DEFAULT VALUES`; an empty `() VALUES ()` is a syntax error.
                format!("INSERT INTO {} DEFAULT VALUES", ident_sqlite(&i.table))
            } else {
                let holes = vec!["?"; cols.len()].join(", ");
                format!(
                    "INSERT INTO {} ({}) VALUES ({holes})",
                    ident_sqlite(&i.table),
                    cols.join(", ")
                )
            }
        }
    };
    (sql, params)
}

/// Re-read the rows a commit changed, so the grid can splice them in place
/// instead of re-running the whole query.
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
    let db = db.clone();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let work = tokio::task::spawn_blocking(move || {
        let conn = open(&db)?;
        let _ = tx.send(conn.get_interrupt_handle());
        let cols = template
            .columns
            .iter()
            .map(|c| ident_sqlite(c))
            .collect::<Vec<_>>()
            .join(", ");
        let mut out = Vec::with_capacity(rows.len());
        for row in &rows {
            let key: Vec<(String, Value)> = template
                .key_cols
                .iter()
                .zip(&row.key)
                .map(|(&ci, v)| (template.columns[ci].clone(), v.clone()))
                .collect();
            let mut params = Vec::new();
            let w = where_clause(&key, &mut params);
            let sql = format!(
                "SELECT {cols} FROM {} WHERE {w} LIMIT 1",
                ident_sqlite(&template.table)
            );
            let mut stmt = conn.prepare(&sql).map_err(query_err)?;
            let mut got = stmt
                .query(rusqlite::params_from_iter(params))
                .map_err(query_err)?;
            // A row that isn't there any more is skipped rather than reported: the
            // key was written by the commit that just ran, and a concurrent delete
            // is a real possibility whose right answer is "nothing to splice".
            if let Some(r) = got.next().map_err(query_err)? {
                let cells = (0..template.columns.len())
                    .map(|i| r.get_ref(i).map(value_of).map_err(query_err))
                    .collect::<Result<Vec<_>, _>>()?;
                out.push((row.data_row, cells));
            }
        }
        Ok(out)
    });

    let interrupt = rx.await.ok();
    tokio::select! {
        r = work => r.map_err(|e| DbError::Query(format!("worker failed: {e}")))?,
        _ = cancel.cancelled() => {
            if let Some(h) = interrupt { h.interrupt(); }
            Err(DbError::Cancelled)
        }
    }
}

/// Bulk-load rows into one table in a single transaction, as batched multi-row
/// `INSERT`s — the same shape as the other two engines, and the same guarantee:
/// each batch must affect exactly as many rows as it carried, or the whole thing
/// rolls back.
///
/// The statement text comes from [`schemaic_core::import::build_insert`], so the
/// quoting and literal escaping are the ones the SQL *export* is tested for
/// rather than a second set written here.
///
/// **It uses `block_in_place`, not `spawn_blocking`**, because `RowSource` is a
/// borrowed `&mut dyn Iterator` that cannot be moved into a `'static` task. The
/// alternative — pulling rows on the async side and shipping batches over a
/// channel — would need the connection to outlive each batch, which is exactly
/// what the transaction requires and what `spawn_blocking` per batch cannot give.
///
/// **The rollback here is unconditional**, unlike MySQL's: SQLite has no
/// non-transactional table type, so there is no `Rollback::note` case where the
/// rollback succeeds having achieved nothing. A cancelled or failed import really
/// does leave the file as it was.
pub(crate) async fn import_rows(
    db: &Db,
    target: crate::ImportTarget<'_>,
    rows: crate::RowSource<'_>,
    cancel: CancellationToken,
) -> Result<u64, DbError> {
    use schemaic_core::import::{INSERT_BATCH_ROWS, build_insert};
    use schemaic_core::intel::SqlDialect;

    let conn = tokio::task::block_in_place(|| open(db))?;
    let cols: Vec<&str> = target.columns.iter().map(String::as_str).collect();

    tokio::task::block_in_place(|| {
        let _ = conn.execute_batch("PRAGMA foreign_keys = ON");
        conn.execute_batch("BEGIN").map_err(query_err)?;

        // Any early return past this point must undo the transaction, so the body
        // runs to a result first and the rollback is applied to it once.
        let result = (|| -> Result<u64, DbError> {
            let mut total = 0u64;
            let mut batch: Vec<Vec<Value>> = Vec::with_capacity(INSERT_BATCH_ROWS);
            let flush = |batch: &mut Vec<Vec<Value>>| -> Result<u64, DbError> {
                let Some(sql) = build_insert(
                    target.database,
                    target.schema,
                    target.table,
                    &cols,
                    batch,
                    SqlDialect::Sqlite,
                ) else {
                    return Ok(0);
                };
                let affected = conn.execute(&sql, []).map_err(query_err)? as u64;
                if affected != batch.len() as u64 {
                    return Err(DbError::Query(format!(
                        "a batch of {} rows affected {affected} — the import was rolled back",
                        batch.len()
                    )));
                }
                batch.clear();
                Ok(affected)
            };

            for row in rows {
                if cancel.is_cancelled() {
                    return Err(DbError::Cancelled);
                }
                batch.push(row.map_err(DbError::Query)?);
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
                // Reported only if the rollback itself fails — the original error
                // is the one worth surfacing.
                let undo = conn.execute_batch("ROLLBACK");
                match undo {
                    Ok(()) => Err(e),
                    Err(u) => Err(DbError::Query(format!(
                        "{e} — and the rollback failed too: {u}"
                    ))),
                }
            }
        }
    })
}

/// The databases this connection offers: the one file, under the name SQLite
/// gives it.
///
/// It answers without opening anything, and that is a decision rather than an
/// optimisation. The schema sidebar lists a connection's databases on selection,
/// long before the user has asked for anything; a SQLite connection pointed at a
/// path that is missing, locked, or on a disconnected network share would
/// otherwise fail there, in a place with nowhere good to put the error. Failing
/// when the user actually reads something is both later and clearer.
pub(crate) async fn fetch_databases(_db: &Db) -> Result<Vec<String>, DbError> {
    Ok(vec![MAIN.to_string()])
}

/// Is the file readable, and is it a database?
///
/// `PRAGMA schema_version` is the cheapest statement that requires SQLite to have
/// actually parsed the file header — `SELECT 1` would succeed against any file at
/// all, since the header is not read until something needs it, and a "connected"
/// status for a JPEG is worse than no status.
pub(crate) async fn ping(db: &Db) -> Result<(), DbError> {
    with_conn(db, |conn| {
        conn.query_row("PRAGMA schema_version", [], |_| Ok(()))
            .map_err(query_err)
    })
    .await
}

/// Validate `sql` without running it.
///
/// `prepare` compiles the statement and reports a syntax error or an unknown
/// table/column, which is the same tier the other two engines' non-executing
/// `PREPARE` provides. Dropping the prepared statement runs nothing.
pub(crate) async fn prepare_check(db: &Db, sql: &str) -> Result<(), DbError> {
    let sql = sql.to_string();
    with_conn(db, move |conn| {
        conn.prepare(&sql).map(|_| ()).map_err(query_err)
    })
    .await
}

/// Introspect the whole database.
///
/// Everything comes from `sqlite_master` plus the `PRAGMA` family, which is
/// SQLite's catalogue. The shape is the same one `assemble_schema` produces for
/// the other engines; what differs is what SQLite has no concept of, and those
/// are left empty rather than invented: no namespaces (`schema: None`), no
/// storage engine, no collation at table level, no comments anywhere — SQLite
/// stores none, so a comment field would be a promise the round-trip can't keep.
pub(crate) async fn fetch_schema(db: &Db) -> Result<DbSchema, DbError> {
    with_conn(db, |conn| {
        let mut tables = Vec::new();
        for (name, kind, sql) in master_entries(conn)? {
            let is_view = kind == "view";
            let columns = table_columns(conn, &name)?;
            let (indexes, foreign_keys) = if is_view {
                (Vec::new(), Vec::new())
            } else {
                (
                    table_indexes(conn, &name)?,
                    table_foreign_keys(conn, &name)?,
                )
            };
            // The table's own `CREATE` text, plus the `CREATE INDEX` statements
            // SQLite stores separately — a table's DDL is incomplete without
            // them, and Copy DDL is the one place that matters. Views keep using
            // the shared emitter (see `TableInfo::create_sql`).
            let create_sql = (!is_view).then(|| {
                let mut out = sql.trim().trim_end_matches(';').to_string();
                out.push(';');
                for ix in index_statements(conn, &name)? {
                    out.push('\n');
                    out.push_str(ix.trim().trim_end_matches(';'));
                    out.push(';');
                }
                Ok::<_, DbError>(out)
            });
            let create_sql = match create_sql {
                Some(r) => Some(r?),
                None => None,
            };
            tables.push(TableInfo {
                name,
                schema: None,
                columns,
                indexes,
                foreign_keys,
                is_view,
                create_sql,
                // The **body**, not the statement — see `view_body_of`.
                view_definition: is_view.then(|| view_body_of(&sql)).flatten(),
                view_options: None,
                engine: None,
                collation: None,
                comment: None,
                check_constraints: Vec::new(),
                triggers: Vec::new(),
            });
        }
        Ok(DbSchema {
            tables,
            ..Default::default()
        })
    })
    .await
}

/// The table **list** — names, view flags, and nothing else.
///
/// One `sqlite_master` scan and none of the per-table pragmas, which is where the
/// cost of a full introspection actually is; see [`crate::Db::fetch_table_list`]
/// for why the distinction earns a method.
pub(crate) async fn fetch_table_list(db: &Db) -> Result<DbSchema, DbError> {
    with_conn(db, |conn| {
        let tables = master_entries(conn)?
            .into_iter()
            .map(|(name, kind, _)| TableInfo {
                name,
                is_view: kind == "view",
                ..Default::default()
            })
            .collect();
        Ok(DbSchema {
            tables,
            ..Default::default()
        })
    })
    .await
}

/// Names and kinds from `sqlite_master`, tables before views, each alphabetically.
///
/// `sqlite_` -prefixed names are SQLite's own bookkeeping (`sqlite_sequence`,
/// `sqlite_stat1`) and are filtered out: they are not the user's tables, and
/// showing them in the tree invites editing something whose corruption breaks the
/// file.
fn master_entries(conn: &SqliteConn) -> Result<Vec<(String, String, String)>, DbError> {
    let mut stmt = conn
        .prepare(
            "SELECT name, type, COALESCE(sql, '') FROM sqlite_master \
             WHERE type IN ('table','view') AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\' \
             ORDER BY type = 'view', name",
        )
        .map_err(query_err)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(query_err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(query_err)
}

/// One table's columns, from `PRAGMA table_xinfo`.
///
/// `table_xinfo` rather than `table_info` because only the former reports
/// **generated** columns (`hidden` 2 or 3). They are otherwise invisible, and a
/// write path that can't see one would offer to insert into it — which SQLite
/// refuses, failing the whole transaction. `hidden = 1` is a virtual table's
/// hidden column and is skipped: it isn't part of the table as declared.
fn table_columns(conn: &SqliteConn, table: &str) -> Result<Vec<ColumnInfo>, DbError> {
    let mut stmt = conn
        .prepare(
            "SELECT name, type, \"notnull\", dflt_value, pk, hidden FROM pragma_table_xinfo(?1)",
        )
        .map_err(query_err)?;
    let rows = stmt
        .query_map([table], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)? != 0,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, i64>(4)? != 0,
                r.get::<_, i64>(5)?,
            ))
        })
        .map_err(query_err)?;

    let mut out = Vec::new();
    for row in rows {
        let (name, type_name, notnull, default, pk, hidden) = row.map_err(query_err)?;
        if hidden == 1 {
            continue; // a virtual table's hidden column — not part of the declaration
        }
        let generated = (hidden == 2 || hidden == 3)
            .then(|| generated_expr(conn, table, &name))
            .flatten();
        // `AUTOINCREMENT` is a separate keyword, but an `INTEGER PRIMARY KEY` is
        // the rowid and is server-assigned whether or not it is present — which is
        // what this flag is asked about, so both count.
        let auto_increment = pk && type_name.eq_ignore_ascii_case("INTEGER");
        out.push(ColumnInfo {
            name,
            type_name,
            // A PK column is NOT NULL in every SQL engine — except SQLite, where
            // an `INTEGER PRIMARY KEY` is the rowid and everything else declared
            // `PRIMARY KEY` may hold NULLs, a documented quirk kept for
            // compatibility. So `notnull` is reported as the pragma gives it,
            // never inferred from `pk`.
            nullable: !notnull,
            primary_key: pk,
            default,
            auto_increment,
            identity_always: false,
            generated,
            on_update: None,
            comment: None,
            collation: None,
        });
    }
    Ok(out)
}

/// The `CREATE INDEX` statements SQLite stores for `table`, in catalogue order.
///
/// Only the ones the user wrote: an index SQLite created itself to back a
/// `UNIQUE` or `PRIMARY KEY` constraint has a **NULL** `sql`, because it is part
/// of the table's own declaration and re-issuing it would be an error.
fn index_statements(conn: &SqliteConn, table: &str) -> Result<Vec<String>, DbError> {
    let mut stmt = conn
        .prepare(
            "SELECT sql FROM sqlite_master \
             WHERE type = 'index' AND tbl_name = ?1 AND sql IS NOT NULL \
             ORDER BY name",
        )
        .map_err(query_err)?;
    let rows = stmt
        .query_map([table], |r| r.get::<_, String>(0))
        .map_err(query_err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(query_err)
}

/// A view's **body** — the `SELECT` — read out of the `CREATE VIEW` statement
/// `sqlite_master` stores.
///
/// **Normalised on the way in**, like `mysql_column`'s defaults and
/// `mysql_check_clause`, because `TableInfo::view_definition` is contracted to
/// hold the stored `SELECT` and nothing else: MySQL's `VIEW_DEFINITION` and
/// PostgreSQL's `pg_get_viewdef` both hand back a body, while SQLite is the one
/// engine that keeps the *whole statement*. Storing that verbatim made Copy DDL
/// emit `CREATE VIEW "v" AS` wrapped around a second, complete `CREATE VIEW` —
/// reported from a real database, and the reason this is a function with tests
/// rather than a `strip_prefix`.
///
/// It is a **positional reader** over the shared lexer, beside
/// `view_algorithm_of` and `trigger_body_of` in `lib.rs`: the header is
/// `CREATE [TEMP|TEMPORARY] VIEW [IF NOT EXISTS] [schema.]name [(columns)] AS`,
/// so the body starts after the first `AS` at a **code** position and at paren
/// depth zero. Both qualifications carry weight — a view may be *named* `as` in
/// any of SQLite's three quotings, and a column list may hold one — and after
/// that point every remaining `AS` belongs to the user's own SQL.
///
/// `None` for anything it can't read, which `view_definition` documents and
/// `create_ddl` already degrades on. That is the safe direction: a body it
/// guessed wrong would be *emitted*.
fn view_body_of(create_sql: &str) -> Option<String> {
    use schemaic_core::intel::SqlDialect;
    use schemaic_core::sql::{is_word_byte, is_word_start, skip_noncode};

    let b = create_sql.as_bytes();
    let mut i = 0usize;
    let mut depth = 0i32;
    let mut seen_create = false;
    while i < b.len() {
        if let Some(j) = skip_noncode(b, i, SqlDialect::Sqlite) {
            i = j.max(i + 1);
            continue;
        }
        match b[i] {
            b'(' => {
                depth += 1;
                i += 1;
                continue;
            }
            b')' => {
                depth -= 1;
                i += 1;
                continue;
            }
            _ => {}
        }
        if !is_word_start(b[i]) {
            i += 1;
            continue;
        }
        let start = i;
        let mut end = i + 1;
        while end < b.len() && is_word_byte(b[end]) {
            end += 1;
        }
        let word = &create_sql[start..end];
        if !seen_create {
            // Whatever this is, it isn't a `CREATE VIEW`; refuse rather than
            // hand back a body read out of something else.
            if !word.eq_ignore_ascii_case("CREATE") {
                return None;
            }
            seen_create = true;
        } else if depth == 0 && word.eq_ignore_ascii_case("AS") {
            // The tail — trailing whitespace and a terminating `;` — is
            // `ddl::view_body`'s rule, shared rather than repeated here.
            let body = schemaic_core::ddl::view_body(&create_sql[end..]);
            return (!body.is_empty()).then_some(body);
        }
        i = end;
    }
    None
}

/// A generated column's expression, dug out of the table's own `CREATE` text.
///
/// SQLite has no pragma for it — `table_xinfo` says a column *is* generated and
/// nothing more — so the declaration is the only source. Returning `None` when it
/// can't be read is safe: the column is still marked generated by its caller, and
/// a missing expression costs a designer field, where a wrong one would emit a
/// different column.
fn generated_expr(conn: &SqliteConn, table: &str, column: &str) -> Option<String> {
    let sql: String = conn
        .query_row(
            "SELECT COALESCE(sql, '') FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |r| r.get(0),
        )
        .ok()?;
    generated_expr_of(&sql, column)
}

/// The pure reader behind [`generated_expr`]: find `<column> … AS ( … )` in a
/// `CREATE TABLE` body and return what is inside the parens.
///
/// It walks the text through the shared boundary lexer rather than searching it,
/// so a column named inside a string or a comment can't match, and the paren scan
/// is [`schemaic_core::sql::balanced_paren_span`] — the same one `ddl`'s
/// `peel_parens` uses, because an expression may perfectly well contain `')'`
/// inside a literal.
fn generated_expr_of(create_sql: &str, column: &str) -> Option<String> {
    use schemaic_core::intel::SqlDialect;
    use schemaic_core::sql::{balanced_paren_span, is_word_byte, is_word_start, skip_noncode};

    let b = create_sql.as_bytes();
    let mut i = 0usize;
    // Walk code positions, looking for the column's name as a whole word.
    while i < b.len() {
        if let Some(j) = skip_noncode(b, i, SqlDialect::Sqlite) {
            i = j.max(i + 1);
            continue;
        }
        if !is_word_start(b[i]) {
            i += 1;
            continue;
        }
        let start = i;
        let mut end = i + 1;
        while end < b.len() && is_word_byte(b[end]) {
            end += 1;
        }
        if create_sql[start..end].eq_ignore_ascii_case(column) {
            // From here to the next comma at this paren depth is the column's
            // declaration; an `AS (` inside it opens the expression.
            if let Some(expr) = as_expression(create_sql, end) {
                return Some(expr);
            }
        }
        i = end;
    }
    // Not found, or the name matched something that wasn't a column declaration.
    let _ = balanced_paren_span;
    None
}

/// From `at`, scan forward for `AS (` at a code position and return the text
/// inside its parens. Stops at the comma or close-paren that ends this column's
/// declaration, so a *later* column's expression can't be attributed to this one.
fn as_expression(sql: &str, at: usize) -> Option<String> {
    use schemaic_core::intel::SqlDialect;
    use schemaic_core::sql::{balanced_paren_span, is_word_byte, is_word_start, skip_noncode};

    let b = sql.as_bytes();
    let mut i = at;
    let mut depth = 0i32;
    while i < b.len() {
        if let Some(j) = skip_noncode(b, i, SqlDialect::Sqlite) {
            i = j.max(i + 1);
            continue;
        }
        match b[i] {
            b'(' if depth > 0 => depth += 1,
            b',' if depth == 0 => return None, // this column's declaration ended
            b')' if depth == 0 => return None, // the table's declaration ended
            _ => {}
        }
        if is_word_start(b[i]) {
            let start = i;
            let mut end = i + 1;
            while end < b.len() && is_word_byte(b[end]) {
                end += 1;
            }
            if sql[start..end].eq_ignore_ascii_case("AS") {
                // The next code byte should be `(`.
                let mut k = end;
                while k < b.len() && b[k].is_ascii_whitespace() {
                    k += 1;
                }
                if b.get(k) == Some(&b'(') {
                    // `balanced_paren_span` returns the index *of* the closing
                    // paren, not one past it.
                    let close = balanced_paren_span(b, k, SqlDialect::Sqlite)?;
                    return Some(sql[k + 1..close].trim().to_string());
                }
            }
            i = end;
            continue;
        }
        i += 1;
    }
    None
}

/// One table's indexes, from `PRAGMA index_list` + `PRAGMA index_xinfo`.
///
/// `index_xinfo` rather than `index_info` because it reports the sort order and
/// includes the rowid columns an index carries implicitly; the latter are dropped
/// here (`key = 0`), since they are not part of what was declared and recreating
/// the index with them would be wrong.
///
/// An index SQLite created for a `UNIQUE` or `PRIMARY KEY` constraint has
/// `origin` `u`/`pk` and **cannot be dropped by name**, exactly as a
/// constraint-backed index can't on PostgreSQL — so the same field carries it,
/// `IndexInfo::constraint`, and the primary one is renamed `PRIMARY` so
/// `IndexInfo::is_primary` answers the MySQL way, as PG's does.
fn table_indexes(conn: &SqliteConn, table: &str) -> Result<Vec<IndexInfo>, DbError> {
    let mut stmt = conn
        .prepare("SELECT name, \"unique\", origin, partial FROM pragma_index_list(?1)")
        .map_err(query_err)?;
    let rows = stmt
        .query_map([table], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)? != 0,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)? != 0,
            ))
        })
        .map_err(query_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(query_err)?;

    let mut out = Vec::new();
    for (name, unique, origin, partial) in rows {
        let (columns, dropped_expression) = index_columns(conn, &name)?;
        let is_pk = origin == "pk";
        out.push(IndexInfo {
            name: if is_pk {
                "PRIMARY".to_string()
            } else {
                name.clone()
            },
            columns,
            unique,
            foreign: false,
            method: None,
            // A partial index's predicate isn't in any pragma — only in the
            // index's own `CREATE` text — so it is left unread rather than
            // guessed, and `lossy` below is what stops a recreate silently
            // widening the index to every row.
            predicate: None,
            constraint: (origin != "c").then(|| name.clone()),
            // Two things SQLite's pragmas don't give back, and each would be
            // destroyed by the drop-and-create an index edit is: the predicate of
            // a partial index, and an expression key column, which `index_xinfo`
            // reports with a NULL name and nothing else.
            lossy: partial || dropped_expression,
        });
    }
    Ok(out)
}

/// One index's key columns, in key order, plus whether an **expression** key was
/// dropped — which the caller records as `lossy`, since recreating the index
/// without it would silently change what it indexes.
fn index_columns(conn: &SqliteConn, index: &str) -> Result<(Vec<IndexColumn>, bool), DbError> {
    let mut stmt = conn
        .prepare("SELECT name, desc, key FROM pragma_index_xinfo(?1) ORDER BY seqno")
        .map_err(query_err)?;
    let rows = stmt
        .query_map([index], |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, i64>(1)? != 0,
                r.get::<_, i64>(2)? != 0,
            ))
        })
        .map_err(query_err)?;

    let mut out = Vec::new();
    let mut dropped_expression = false;
    for row in rows {
        let (name, descending, key) = row.map_err(query_err)?;
        if !key {
            continue; // an implicitly carried rowid, not a declared key column
        }
        // A NULL name is an expression key (`CREATE INDEX … ON t (lower(a))`),
        // which no pragma spells out — so it is dropped and the index is marked
        // lossy, rather than recreated as an index over different columns.
        let Some(name) = name else {
            dropped_expression = true;
            continue;
        };
        out.push(IndexColumn {
            name,
            prefix: None,
            descending,
            expression: false,
        });
    }
    Ok((out, dropped_expression))
}

/// One table's foreign keys, from `PRAGMA foreign_key_list`.
///
/// The pragma reports one row per *column* of each key, grouped by an `id` that
/// counts down from the last-declared key, so rows are gathered by that id and the
/// per-key fields taken from the first row of each group.
///
/// A key's `to` column is **NULL when the reference is implicit** (`REFERENCES
/// artist` with no column list, meaning the target's primary key). SQLite resolves
/// that at write time; here it is resolved by asking the target for its PK, so the
/// model carries the columns a `JOIN` would actually use — an empty `ref_columns`
/// would silently disable FK navigation and the ERD's edges.
fn table_foreign_keys(conn: &SqliteConn, table: &str) -> Result<Vec<ForeignKeyInfo>, DbError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, seq, \"table\", \"from\", \"to\", on_update, on_delete \
             FROM pragma_foreign_key_list(?1) ORDER BY id, seq",
        )
        .map_err(query_err)?;
    let rows = stmt
        .query_map([table], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
            ))
        })
        .map_err(query_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(query_err)?;

    let mut out: Vec<ForeignKeyInfo> = Vec::new();
    let mut current: Option<i64> = None;
    for (id, ref_table, from, to, on_update, on_delete) in rows {
        if current != Some(id) {
            current = Some(id);
            out.push(ForeignKeyInfo {
                // SQLite's foreign keys have no names of their own unless the
                // declaration gave one, and no pragma reports it. A synthetic
                // name keeps the model addressable; it is never emitted, because
                // SQLite cannot drop a foreign key at all — the whole table has
                // to be rebuilt.
                name: format!("fk_{table}_{id}"),
                columns: Vec::new(),
                ref_schema: None,
                ref_table: ref_table.clone(),
                ref_columns: Vec::new(),
                on_delete: action_of(&on_delete),
                on_update: action_of(&on_update),
            });
        }
        let fk = out.last_mut().expect("just pushed");
        fk.columns.push(from);
        match to {
            Some(col) => fk.ref_columns.push(col),
            None => {
                // Implicit reference to the target's primary key.
                if let Ok(pk) = primary_key_of(conn, &ref_table) {
                    fk.ref_columns = pk;
                }
            }
        }
    }
    Ok(out)
}

/// A referential action as the model wants it: `None` for the standard default,
/// which both other engines also leave unwritten so that emitting nothing
/// round-trips exactly.
fn action_of(action: &str) -> Option<String> {
    (!action.eq_ignore_ascii_case("NO ACTION")).then(|| action.to_string())
}

/// A table's primary-key columns, in key order.
fn primary_key_of(conn: &SqliteConn, table: &str) -> Result<Vec<String>, DbError> {
    let mut stmt = conn
        .prepare("SELECT name FROM pragma_table_info(?1) WHERE pk > 0 ORDER BY pk")
        .map_err(query_err)?;
    let rows = stmt
        .query_map([table], |r| r.get::<_, String>(0))
        .map_err(query_err)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(query_err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemaic_core::model::{RowDelete, RowEdit, RowInsert};

    /// An in-memory database, seeded. This is the one backend whose DB layer can
    /// be tested hermetically — SQLite needs no server — so it is, and the tests
    /// below assert against real SQLite behaviour rather than against a model of
    /// it.
    fn seeded() -> SqliteConn {
        let conn = SqliteConn::open_in_memory().expect("open");
        conn.execute_batch(
            "CREATE TABLE artist (
                 id   INTEGER PRIMARY KEY,
                 name TEXT NOT NULL,
                 note TEXT
             );
             CREATE TABLE album (
                 id        INTEGER PRIMARY KEY AUTOINCREMENT,
                 title     TEXT NOT NULL,
                 artist_id INTEGER REFERENCES artist ON DELETE CASCADE,
                 slug      TEXT GENERATED ALWAYS AS (lower(title)) VIRTUAL
             );
             CREATE UNIQUE INDEX album_title ON album(title DESC);
             CREATE VIEW big AS SELECT id, title FROM album;
             INSERT INTO artist (id, name, note) VALUES (1, 'Ada', NULL), (2, 'Grace', 'x');
             INSERT INTO album (title, artist_id) VALUES ('One', 1);",
        )
        .expect("seed");
        conn
    }

    /// One column, four storage classes — which is what "a declared type is an
    /// affinity, not a type" actually costs a reader.
    ///
    /// The column is `INTEGER` on purpose. A `TEXT` one would *not* show this:
    /// TEXT affinity converts an inserted `42` to the string `'42'`, so every cell
    /// really would be text and the test would pass against code that read the
    /// declared type and ignored the value. INTEGER affinity converts only what it
    /// can — `'x'` has no integer form and `1.5` no lossless one — so the same
    /// column comes back Int, Str, Null and Float, and nothing but reading each
    /// value's own class gets that right.
    #[test]
    fn a_cell_is_read_as_the_class_sqlite_returned_not_the_declared_type() {
        let conn = SqliteConn::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE t (a INTEGER); INSERT INTO t VALUES (42), ('x'), (NULL), (1.5);",
        )
        .unwrap();
        let rs = run_query(&conn, "SELECT a FROM t", 100).unwrap();
        let tag = |r: usize| rs.cell(r, 0).expect("cell").tag;
        let text = |r: usize| rs.cell(r, 0).expect("cell").display().to_string();
        // The tags are the assertion: `display()` would read "42" either way, so
        // asserting on text alone would pass against reading everything as TEXT.
        assert_eq!(tag(0), CellTag::Int);
        assert_eq!(text(0), "42");
        assert_eq!(tag(1), CellTag::Str);
        assert_eq!(text(1), "x");
        assert_eq!(tag(2), CellTag::Null);
        assert_eq!(tag(3), CellTag::Float);
        assert_eq!(text(3), "1.5");
        // And the column still reports the type it was declared with, which is
        // the half that makes the other half worth asserting.
        let stmt = conn.prepare("SELECT a FROM t").unwrap();
        assert_eq!(columns_of(&stmt)[0].type_name, "INTEGER");
    }

    #[test]
    fn a_blob_is_shown_as_its_size_rather_than_as_mojibake() {
        let conn = SqliteConn::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE t (b BLOB); INSERT INTO t VALUES (x'00ff10');")
            .unwrap();
        let rs = run_query(&conn, "SELECT b FROM t", 10).unwrap();
        assert_eq!(rs.cell(0, 0).expect("cell").display(), "<3 bytes>");
    }

    #[test]
    fn a_statement_returning_no_rows_reports_affected_instead() {
        let conn = seeded();
        let rs = run_query(&conn, "UPDATE artist SET note = 'y' WHERE id = 1", 10).unwrap();
        assert_eq!(rs.affected, Some(1));
        assert_eq!(rs.columns.len(), 0);
        // …and a SELECT reports rows, with `affected` left None so the UI can
        // tell the two apart.
        let rs = run_query(&conn, "SELECT * FROM artist", 10).unwrap();
        assert_eq!(rs.affected, None);
        assert_eq!(rs.row_count(), 2);
    }

    #[test]
    fn the_row_cap_truncates_and_says_so() {
        let conn = seeded();
        let rs = run_query(&conn, "SELECT * FROM artist", 1).unwrap();
        assert_eq!(rs.row_count(), 1);
        assert!(rs.truncated);
        let rs = run_query(&conn, "SELECT * FROM artist", 50).unwrap();
        assert_eq!(rs.row_count(), 2);
        assert!(!rs.truncated);
    }

    #[test]
    fn a_column_carries_its_declared_type_and_an_expression_carries_none() {
        let conn = seeded();
        let mut stmt = conn
            .prepare("SELECT id, name, count(*) FROM artist")
            .unwrap();
        let cols = columns_of(&stmt);
        assert_eq!(cols[0].type_name, "INTEGER");
        assert_eq!(cols[1].type_name, "TEXT");
        // SQLite assigns no type to a computed column, and saying so beats
        // inventing one.
        assert_eq!(cols[2].type_name, "");
        let _ = stmt.query([]).unwrap();
    }

    #[test]
    fn introspection_reads_columns_keys_and_generated_expressions() {
        let conn = seeded();
        let cols = table_columns(&conn, "album").unwrap();
        let names: Vec<&str> = cols.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["id", "title", "artist_id", "slug"]);

        let id = &cols[0];
        assert!(id.primary_key);
        // An INTEGER PRIMARY KEY is the rowid, so it is server-assigned.
        assert!(id.auto_increment);

        let title = &cols[1];
        assert!(!title.nullable);
        assert!(!title.primary_key);

        // A generated column is visible at all only via `table_xinfo`, and its
        // expression only via the table's own CREATE text.
        let slug = &cols[3];
        assert_eq!(slug.generated.as_deref(), Some("lower(title)"));
    }

    /// SQLite's documented quirk: a `PRIMARY KEY` that isn't `INTEGER` does not
    /// imply NOT NULL. Reporting nullability from `pk` rather than from the
    /// pragma would make the write path build a key on a column that can be NULL.
    #[test]
    fn a_non_integer_primary_key_is_nullable_because_sqlite_says_so() {
        let conn = SqliteConn::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE t (k TEXT PRIMARY KEY, v TEXT);")
            .unwrap();
        let cols = table_columns(&conn, "t").unwrap();
        assert!(cols[0].primary_key);
        assert!(cols[0].nullable, "SQLite allows NULL in a non-INTEGER PK");
        assert!(!cols[0].auto_increment);
    }

    #[test]
    fn an_index_reports_its_order_and_whether_a_constraint_backs_it() {
        let conn = seeded();
        let ix = table_indexes(&conn, "album").unwrap();
        let by_name = |n: &str| ix.iter().find(|i| i.name == n).cloned();

        let explicit = by_name("album_title").expect("the declared index");
        assert!(explicit.unique);
        assert_eq!(explicit.columns[0].name, "title");
        assert!(explicit.columns[0].descending, "DESC is part of the key");
        // Declared with CREATE INDEX, so nothing but its own name drops it.
        assert_eq!(explicit.constraint, None);
    }

    #[test]
    fn a_foreign_key_resolves_an_implicit_reference_to_the_targets_key() {
        let conn = seeded();
        let fks = table_foreign_keys(&conn, "album").unwrap();
        assert_eq!(fks.len(), 1);
        let fk = &fks[0];
        assert_eq!(fk.columns, ["artist_id"]);
        assert_eq!(fk.ref_table, "artist");
        // `REFERENCES artist` names no column; without resolving it to the
        // target's PK the ERD would draw an edge to nothing.
        assert_eq!(fk.ref_columns, ["id"]);
        assert_eq!(fk.on_delete.as_deref(), Some("CASCADE"));
        // NO ACTION is the standard default and is left unwritten, so that
        // emitting nothing round-trips.
        assert_eq!(fk.on_update, None);
    }

    #[test]
    fn the_catalogue_lists_tables_then_views_and_hides_sqlites_own() {
        let conn = seeded();
        let entries = master_entries(&conn).unwrap();
        let names: Vec<&str> = entries.iter().map(|(n, _, _)| n.as_str()).collect();
        // `sqlite_sequence` exists (album is AUTOINCREMENT) and must not be listed.
        assert!(
            !names.contains(&"sqlite_sequence"),
            "SQLite's own bookkeeping is not the user's schema: {names:?}"
        );
        assert_eq!(names, ["album", "artist", "big"]);
        assert_eq!(entries[2].1, "view");
    }

    // ── Provenance and write-back ────────────────────────────────────────────

    fn origins_for(conn: &SqliteConn, sql: &str) -> Vec<Option<ColumnOrigin>> {
        let stmt = conn.prepare(sql).expect("prepare");
        let mut cols = columns_of(&stmt);
        drop(stmt);
        attach_origins(conn, sql, &mut cols);
        cols.into_iter().map(|c| c.origin).collect()
    }

    #[test]
    fn a_plain_select_attributes_every_column_to_its_base_table() {
        let conn = seeded();
        let o = origins_for(&conn, "SELECT id, name, note FROM artist");
        assert_eq!(o.len(), 3);
        let id = o[0].as_ref().expect("id has an origin");
        assert_eq!(id.table, "artist");
        assert_eq!(id.column, "id");
        assert_eq!(id.database, "main");
        assert_eq!(id.schema, None, "SQLite has no namespace level");
        assert!(id.flags.primary_key);
        assert!(
            id.flags.auto_increment,
            "an INTEGER PRIMARY KEY is the rowid"
        );
        let name = o[1].as_ref().expect("name has an origin");
        assert!(name.flags.not_null);
        assert!(!name.flags.primary_key);
    }

    /// The case that makes the derivation positional rather than name-matched: the
    /// first result column is *named* `name` but holds `note`. Matching by name
    /// would map it to column `name`, and an edit to it would silently `UPDATE`
    /// the wrong column.
    #[test]
    fn an_alias_is_attributed_to_the_column_behind_it() {
        let conn = seeded();
        let o = origins_for(&conn, "SELECT note AS name, name AS note FROM artist");
        assert_eq!(o[0].as_ref().unwrap().column, "note");
        assert_eq!(o[1].as_ref().unwrap().column, "name");
    }

    #[test]
    fn a_computed_column_and_a_joined_statement_are_not_editable() {
        let conn = seeded();
        // An expression belongs to no column.
        let o = origins_for(&conn, "SELECT id, id * 2 FROM artist");
        assert!(o[0].is_some());
        assert!(o[1].is_none(), "an expression has no base column");
        // A join leaves the whole result unattributed.
        let o = origins_for(
            &conn,
            "SELECT album.id, artist.name FROM album JOIN artist ON album.artist_id = artist.id",
        );
        assert!(o.iter().all(|x| x.is_none()), "a join is not editable");
    }

    /// A view has no rows of its own, and SQLite refuses a write to one without an
    /// `INSTEAD OF` trigger — so offering the edit and failing at commit time
    /// would be worse than not offering it.
    #[test]
    fn a_view_is_not_editable() {
        let conn = seeded();
        let o = origins_for(&conn, "SELECT id, title FROM big");
        assert!(o.iter().all(|x| x.is_none()));
    }

    /// A BLOB cell renders as its size and cannot round-trip, so its column is
    /// marked binary — the call the other two engines make for a binary charset.
    #[test]
    fn a_blob_column_is_marked_binary_and_so_read_only() {
        let conn = SqliteConn::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE t (id INTEGER PRIMARY KEY, b BLOB);")
            .unwrap();
        let o = origins_for(&conn, "SELECT id, b FROM t");
        assert!(!o[0].as_ref().unwrap().binary);
        assert!(o[1].as_ref().unwrap().binary);
    }

    /// A **shared** in-memory database plus a `Db` pointing at it.
    ///
    /// The write paths each open their own connection, so a plain `:memory:` —
    /// which is private to one connection — wouldn't do; SQLite's shared-cache
    /// memory URI gives several connections one database, with **no file
    /// anywhere**, so the suite stays as pure as the rest of the workspace.
    ///
    /// The returned connection is the keeper: a shared-memory database exists
    /// only while at least one connection to it is open, so tests must hold it for
    /// their duration. `name` must be unique per test, since the suite runs
    /// threaded and the name is the whole identity.
    fn shared_memory(name: &str) -> (SqliteConn, Db) {
        let uri = format!("file:{name}?mode=memory&cache=shared");
        let keeper = SqliteConn::open_with_flags(
            &uri,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_URI,
        )
        .expect("open a shared in-memory database");
        let db = Db::from_parts(
            crate::Engine::Sqlite,
            String::new(),
            0,
            String::new(),
            String::new(),
            uri,
        );
        (keeper, db)
    }

    fn edit(table: &str, set: &[(&str, Option<&str>)], key: &[(&str, Value)]) -> RowEdit {
        RowEdit {
            database: MAIN.to_string(),
            schema: None,
            table: table.to_string(),
            set: set
                .iter()
                .map(|(c, v)| (c.to_string(), v.map(str::to_string)))
                .collect(),
            key: key
                .iter()
                .map(|(c, v)| (c.to_string(), v.clone()))
                .collect(),
        }
    }

    #[tokio::test]
    async fn a_commit_applies_deletes_updates_and_inserts_in_that_order() {
        let (keeper, db) = shared_memory("commit_order");
        keeper
            .execute_batch(
                "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT);
                 INSERT INTO t VALUES (1, 'a'), (2, 'b');",
            )
            .unwrap();
        let write = GridWrite {
            updates: vec![edit("t", &[("v", Some("B"))], &[("id", Value::Int(2))])],
            inserts: vec![RowInsert {
                database: MAIN.to_string(),
                schema: None,
                table: "t".to_string(),
                cols: vec![
                    ("id".into(), Some("1".into())),
                    ("v".into(), Some("z".into())),
                ],
            }],
            deletes: vec![RowDelete {
                database: MAIN.to_string(),
                schema: None,
                table: "t".to_string(),
                key: vec![("id".to_string(), Value::Int(1))],
            }],
        };
        // The insert reuses id 1, which only works because the delete runs first —
        // which is `GridWrite::plan`'s order, shared with the other two engines.
        let n = commit_writes(&db, &write, CancellationToken::new())
            .await
            .expect("commit");
        assert_eq!(n, 3);

        let rows: Vec<(i64, String)> = keeper
            .prepare("SELECT id, v FROM t ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(rows, [(1, "z".to_string()), (2, "B".to_string())]);
    }

    /// The 1-row safety net, which is the whole promise of this path: a key that
    /// matches nothing must roll the *entire* batch back, not just fail its own
    /// statement.
    #[tokio::test]
    async fn a_statement_matching_no_row_rolls_the_whole_batch_back() {
        let (keeper, db) = shared_memory("one_row_guard");
        keeper
            .execute_batch(
                "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT);
                 INSERT INTO t VALUES (1, 'a');",
            )
            .unwrap();
        let write = GridWrite {
            updates: vec![
                edit("t", &[("v", Some("ok"))], &[("id", Value::Int(1))]),
                // No such row.
                edit("t", &[("v", Some("no"))], &[("id", Value::Int(99))]),
            ],
            ..Default::default()
        };
        let err = commit_writes(&db, &write, CancellationToken::new())
            .await
            .expect_err("the guard must fire");
        assert!(
            format!("{err}").contains("affected 0 rows"),
            "the message names what the guard saw: {err}"
        );

        // The *first* update must be gone too — SQLite has no non-transactional
        // table type, so unlike MySQL this rollback is unconditionally complete.
        let v: String = keeper
            .query_row("SELECT v FROM t WHERE id = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, "a", "the batch rolled back whole");
    }

    /// `= NULL` is never true, so a NULL key column would match no rows and the
    /// guard would report a failed write rather than the wrong `WHERE`. SQLite
    /// makes this reachable by allowing NULL in a non-INTEGER primary key.
    #[tokio::test]
    async fn a_null_key_column_is_compared_with_is_null() {
        let (keeper, db) = shared_memory("null_key");
        keeper
            .execute_batch(
                "CREATE TABLE t (k TEXT PRIMARY KEY, v TEXT);
                 INSERT INTO t VALUES (NULL, 'a');",
            )
            .unwrap();
        let write = GridWrite {
            updates: vec![edit("t", &[("v", Some("B"))], &[("k", Value::Null)])],
            ..Default::default()
        };
        commit_writes(&db, &write, CancellationToken::new())
            .await
            .expect("a NULL key must match its row");
        let v: String = keeper
            .query_row("SELECT v FROM t WHERE k IS NULL", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, "B");
    }

    /// An insert that names no columns at all — every one left to its default.
    /// `INSERT INTO t () VALUES ()` is a syntax error in SQLite; the spelling is
    /// `DEFAULT VALUES`.
    #[test]
    fn an_insert_with_no_columns_uses_default_values() {
        let ins = RowInsert {
            database: MAIN.to_string(),
            schema: None,
            table: "t".to_string(),
            cols: Vec::new(),
        };
        let (sql, params) = statement_for(WriteStep::Insert(&ins));
        assert_eq!(sql, r#"INSERT INTO "t" DEFAULT VALUES"#);
        assert!(params.is_empty());
    }

    #[tokio::test]
    async fn a_refetch_reads_the_row_the_commit_just_wrote() {
        let (_keeper, db) = shared_memory("refetch");
        _keeper
            .execute_batch(
                "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT);
                 INSERT INTO t VALUES (1, 'a'), (2, 'b');",
            )
            .unwrap();
        let template = RefetchTemplate {
            database: MAIN.to_string(),
            schema: None,
            table: "t".to_string(),
            columns: vec!["id".into(), "v".into()],
            key_cols: vec![0],
        };
        let rows = vec![
            RefetchRow {
                data_row: 7,
                key: vec![Value::Int(2)],
            },
            // A row that no longer exists is skipped, not reported: a concurrent
            // delete is a real possibility and "nothing to splice" is the answer.
            RefetchRow {
                data_row: 8,
                key: vec![Value::Int(404)],
            },
        ];
        let got = refetch_rows(&db, &template, &rows, CancellationToken::new())
            .await
            .expect("refetch");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, 7, "the grid row index is carried through");
        assert_eq!(got[0].1[1].display(), "b");
    }

    /// `block_in_place` needs the multi-threaded runtime, which the app uses.
    #[tokio::test(flavor = "multi_thread")]
    async fn an_import_loads_every_batch_in_one_transaction() {
        let (keeper, db) = shared_memory("import_ok");
        keeper
            .execute_batch("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT);")
            .unwrap();
        // More than one batch, so the batching itself is exercised.
        let n = schemaic_core::import::INSERT_BATCH_ROWS + 7;
        let mut rows = (1..=n).map(|i| Ok(vec![Value::Int(i as i64), Value::Str(format!("v{i}"))]));
        let cols = vec!["id".to_string(), "v".to_string()];
        let target = crate::ImportTarget {
            database: MAIN,
            schema: None,
            table: "t",
            columns: &cols,
        };
        let loaded = import_rows(&db, target, &mut rows, CancellationToken::new())
            .await
            .expect("import");
        assert_eq!(loaded, n as u64);
        let count: i64 = keeper
            .query_row("SELECT count(*) FROM t", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, n as i64);
    }

    /// All-or-nothing: a reader error partway through leaves nothing behind, and
    /// unlike MySQL there is no engine here that ignores the rollback.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_failed_import_leaves_no_rows() {
        let (keeper, db) = shared_memory("import_fail");
        keeper
            .execute_batch("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT);")
            .unwrap();
        // Fails after enough rows to have flushed a full batch already.
        let n = schemaic_core::import::INSERT_BATCH_ROWS + 3;
        let mut rows = (1..=n).map(|i| {
            if i == n {
                Err("row 503 is malformed".to_string())
            } else {
                Ok(vec![Value::Int(i as i64), Value::Str(format!("v{i}"))])
            }
        });
        let cols = vec!["id".to_string(), "v".to_string()];
        let target = crate::ImportTarget {
            database: MAIN,
            schema: None,
            table: "t",
            columns: &cols,
        };
        let err = import_rows(&db, target, &mut rows, CancellationToken::new())
            .await
            .expect_err("the reader error must abort the import");
        assert!(format!("{err}").contains("malformed"), "{err}");
        let count: i64 = keeper
            .query_row("SELECT count(*) FROM t", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "the committed batch rolled back with the rest");
    }

    /// `sqlite_master.sql` stores the **whole** `CREATE VIEW …` statement, but
    /// `TableInfo::view_definition` is contracted to hold only the body — so
    /// storing the statement made Copy DDL emit a `CREATE VIEW "v" AS` header
    /// wrapped around a second, complete `CREATE VIEW`. Reported from a real
    /// database; this is the regression test.
    #[test]
    fn a_views_body_is_read_out_of_its_create_statement() {
        assert_eq!(
            view_body_of("CREATE VIEW v AS SELECT 1").as_deref(),
            Some("SELECT 1")
        );
        // The reported shape, newlines and all.
        let real = "CREATE VIEW album_titles AS\n    SELECT album.id, album.title\n    \
                    FROM album JOIN artist ON album.artist_id = artist.id";
        assert_eq!(
            view_body_of(real).as_deref(),
            Some(
                "SELECT album.id, album.title\n    \
                 FROM album JOIN artist ON album.artist_id = artist.id"
            )
        );
    }

    /// The header has several optional parts, and the body's own `AS` must not be
    /// mistaken for the one that ends the header — only the first at paren depth
    /// zero counts.
    #[test]
    fn the_view_header_is_read_past_every_optional_part() {
        for (sql, body) in [
            ("CREATE TEMP VIEW v AS SELECT 1", "SELECT 1"),
            (
                "CREATE TEMPORARY VIEW IF NOT EXISTS v AS SELECT 1",
                "SELECT 1",
            ),
            ("CREATE VIEW main.v AS SELECT 1", "SELECT 1"),
            // A column list is parenthesised, so nothing in it is at depth 0.
            ("CREATE VIEW v (a, b) AS SELECT 1, 2", "SELECT 1, 2"),
            // The body's own alias must survive — the *first* AS ends the header.
            (
                "CREATE VIEW v AS SELECT a AS b FROM t",
                "SELECT a AS b FROM t",
            ),
            // A trailing semicolon isn't part of the body.
            ("CREATE VIEW v AS SELECT 1;", "SELECT 1"),
            ("CREATE VIEW v /* c */ AS SELECT 1", "SELECT 1"),
        ] {
            assert_eq!(view_body_of(sql).as_deref(), Some(body), "{sql}");
        }
    }

    /// A view *named* `as`, in each of SQLite's three quotings. The lexer skips a
    /// quoted identifier whole, so none of them ends the header early — which a
    /// byte search for "as" would get wrong three different ways.
    #[test]
    fn a_view_named_as_does_not_end_its_own_header() {
        for sql in [
            r#"CREATE VIEW "as" AS SELECT 1"#,
            "CREATE VIEW `as` AS SELECT 1",
            "CREATE VIEW [as] AS SELECT 1",
            r#"CREATE VIEW "my as view" AS SELECT 1"#,
        ] {
            assert_eq!(view_body_of(sql).as_deref(), Some("SELECT 1"), "{sql}");
        }
    }

    /// Anything it can't read is `None`, which `TableInfo::view_definition`
    /// documents ("views whose definition couldn't be read") and `create_ddl`
    /// already degrades on — better than handing back a statement that would be
    /// wrapped in a second header.
    #[test]
    fn an_unreadable_header_yields_no_body_rather_than_the_whole_statement() {
        for sql in [
            "SELECT 1",            // not a CREATE at all
            "CREATE VIEW v",       // no AS
            "CREATE VIEW v AS",    // nothing after it
            "CREATE VIEW v AS   ", // …still nothing
            "",
        ] {
            assert_eq!(view_body_of(sql), None, "{sql:?}");
        }
    }

    /// Copy DDL on a **table** hands back SQLite's own text, plus the
    /// `CREATE INDEX` statements it stores separately.
    ///
    /// The assertion is that SQLite *accepts the output*, because the bug this
    /// replaces was not a formatting difference: reconstructing from the model
    /// emitted `AUTO_INCREMENT` (which SQLite silently swallows into the type
    /// name), MySQL's inline `KEY name (cols)` (a syntax error), and an empty
    /// column list for an expression index — while dropping the foreign key,
    /// `WITHOUT ROWID`, and the partial index's predicate outright.
    #[tokio::test]
    async fn copy_ddl_on_a_table_is_sqlites_own_text_and_replays() {
        let (keeper, db) = shared_memory("table_ddl");
        keeper
            .execute_batch(
                "CREATE TABLE artist (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
                 CREATE TABLE album (
                     id        INTEGER PRIMARY KEY AUTOINCREMENT,
                     title     TEXT NOT NULL COLLATE NOCASE,
                     artist_id INTEGER REFERENCES artist ON DELETE CASCADE,
                     slug      TEXT GENERATED ALWAYS AS (lower(title)) VIRTUAL,
                     CHECK (length(title) > 0)
                 );
                 CREATE INDEX album_lower ON album(lower(title));
                 CREATE INDEX album_partial ON album(title) WHERE artist_id IS NOT NULL;
                 CREATE TABLE pair (a TEXT, b TEXT, PRIMARY KEY (a, b)) WITHOUT ROWID;",
            )
            .unwrap();
        let schema = fetch_schema(&db).await.expect("schema");
        let ddl_of = |name: &str| {
            schema
                .tables
                .iter()
                .find(|t| t.name == name)
                .unwrap_or_else(|| panic!("{name}"))
                .create_ddl(schemaic_core::intel::SqlDialect::Sqlite)
        };

        let album = ddl_of("album");
        // Everything the model cannot carry, and therefore could not restate.
        assert!(album.contains("AUTOINCREMENT"), "{album}");
        assert!(
            !album.contains("AUTO_INCREMENT"),
            "MySQL's spelling: {album}"
        );
        assert!(album.contains("REFERENCES artist"), "the FK: {album}");
        assert!(album.contains("ON DELETE CASCADE"), "its action: {album}");
        assert!(album.contains("COLLATE NOCASE"), "the collation: {album}");
        assert!(
            album.contains("CHECK (length(title) > 0)"),
            "the check: {album}"
        );
        // The separately-stored index statements, including the ones whose keys
        // the pragmas report as `lossy`.
        assert!(album.contains("CREATE INDEX album_lower"), "{album}");
        assert!(
            album.contains("lower(title)"),
            "the expression key: {album}"
        );
        assert!(
            album.contains("WHERE artist_id IS NOT NULL"),
            "the partial predicate: {album}"
        );
        // MySQL's inline index syntax must not appear at all.
        assert!(!album.contains("KEY \""), "MySQL inline index: {album}");

        assert!(ddl_of("pair").contains("WITHOUT ROWID"));

        // And the whole thing replays: the real test of DDL is that the engine
        // takes it back.
        let replay = SqliteConn::open_in_memory().unwrap();
        for name in ["artist", "album", "pair"] {
            let sql = ddl_of(name);
            replay
                .execute_batch(&sql)
                .unwrap_or_else(|e| panic!("SQLite rejected its own DDL for {name}: {e}\n{sql}"));
        }
    }

    /// The other engines are untouched: with no `create_sql`, `create_ddl` still
    /// builds from the model exactly as before.
    #[test]
    fn a_table_without_its_own_text_still_goes_through_the_shared_emitter() {
        use schemaic_core::intel::SqlDialect;
        use schemaic_core::schema::{ColumnInfo, TableInfo};
        let t = TableInfo {
            name: "t".into(),
            columns: vec![ColumnInfo {
                name: "id".into(),
                type_name: "int".into(),
                primary_key: true,
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(t.create_sql, None);
        let ddl = t.create_ddl(SqlDialect::MySql);
        assert!(ddl.starts_with("CREATE TABLE `t`"), "{ddl}");
        assert!(ddl.contains("PRIMARY KEY (`id`)"), "{ddl}");
    }

    /// End to end through the real `fetch_schema` and the real DDL emitter,
    /// because the bug was in the **wiring** — the reader can be perfect and the
    /// field still be filled from the wrong string. It asserts the artefact the
    /// user actually saw: one `CREATE VIEW`, not two.
    #[tokio::test]
    async fn copy_ddl_on_a_view_emits_one_create_statement() {
        let (keeper, db) = shared_memory("view_ddl");
        keeper
            .execute_batch(
                "CREATE TABLE album (id INTEGER PRIMARY KEY, title TEXT);
                 CREATE VIEW album_titles AS
                     SELECT album.id, album.title FROM album;",
            )
            .unwrap();
        let schema = fetch_schema(&db).await.expect("schema");
        let view = schema
            .tables
            .iter()
            .find(|t| t.name == "album_titles")
            .expect("the view");

        let body = view.view_definition.as_deref().expect("a definition");
        assert!(
            body.to_ascii_uppercase().starts_with("SELECT"),
            "the field holds the SELECT, not the statement: {body:?}"
        );

        let ddl = view.create_ddl(schemaic_core::intel::SqlDialect::Sqlite);
        assert_eq!(
            ddl.to_ascii_uppercase().matches("CREATE VIEW").count(),
            1,
            "exactly one CREATE VIEW, not a header wrapped around a statement:\n{ddl}"
        );
        assert!(ddl.contains("album_titles"));
    }

    #[test]
    fn a_generated_expression_is_read_through_the_lexer_not_by_searching() {
        // The column's name appears inside a string and inside another column's
        // expression before its own declaration; neither may match.
        let sql = "CREATE TABLE t (\n  a TEXT DEFAULT 'slug is here',\n  \
                   b TEXT GENERATED ALWAYS AS (a || 'slug') VIRTUAL,\n  \
                   slug TEXT GENERATED ALWAYS AS (lower(a) || ')') VIRTUAL\n)";
        assert_eq!(
            generated_expr_of(sql, "slug").as_deref(),
            Some("lower(a) || ')'"),
            "the close paren inside the literal must not end the expression"
        );
        // A plain column has no expression.
        assert_eq!(generated_expr_of(sql, "a"), None);
    }
}
