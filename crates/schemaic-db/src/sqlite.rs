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
//! failure this whole layer exists to prevent. See [`attach_origins`] for what the
//! derivation does accept.
//!
//! **Every rowid table has a key, and it isn't a column.** A table with no primary
//! key and no usable unique index is read-only on the other two engines because
//! there is genuinely no way to name one of its rows. On SQLite there always is,
//! unless the table was declared `WITHOUT ROWID` — so such a table is made
//! editable by projecting its `rowid` explicitly (`SELECT rowid, * FROM t`, which
//! [`schemaic_core::filter::table_query`] generates) and marking that result
//! column [`ColumnOrigin::implicit_key`]. The column is shown rather than hidden:
//! the grid has no notion of a column that isn't one, and inventing one would put
//! the burden of skipping it on export, copy and every aggregate. See
//! [`implicit_row_key`] for why the *spelling* is chosen rather than fixed.

use std::collections::HashMap;
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
    CheckInfo, ColumnInfo, DbSchema, ForeignKeyInfo, IndexColumn, IndexInfo, TableInfo,
    TriggerInfo, ViewOptions,
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
            // Not a declared column — but it may still be the table's rowid,
            // named explicitly because `SELECT *` does not return it. A declared
            // column of the same name is looked for *first* and wins, which is
            // what SQLite itself does with the name, so a table with its own
            // `rowid` column is unaffected by any of this.
            //
            // The name is recorded **as written**: `_rowid_` is the spelling that
            // reaches the true rowid of a table that has taken `rowid`, and the
            // `WHERE` the write-back builds from this must resolve to the same
            // value the `SELECT` read.
            if ROWID_ALIASES.iter().any(|a| a.eq_ignore_ascii_case(&base))
                && has_rowid(conn, &source.name)
            {
                col.origin = Some(ColumnOrigin {
                    database: MAIN.to_string(),
                    schema: None,
                    table: source.name.clone(),
                    column: base,
                    flags: ColumnFlags {
                        // Not a primary key: it is one only in the sense that it
                        // identifies a row, and `implicit_key` is the field that
                        // says so. A rowid is never NULL and is always assigned
                        // by the engine.
                        not_null: true,
                        auto_increment: true,
                        ..Default::default()
                    },
                    binary: false,
                    implicit_key: true,
                });
            }
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

/// The names SQLite accepts for a rowid table's implicit key, in the order a
/// generated query should prefer them. All three mean the same thing; they exist
/// as three because any of them may have been taken by a declared column.
const ROWID_ALIASES: [&str; 3] = ["rowid", "_rowid_", "oid"];

/// Does `table` have a `rowid` — the implicit 64-bit key SQLite gives every table
/// that wasn't declared `WITHOUT ROWID`?
///
/// `PRAGMA table_list`'s `wr` column is the authority, and it is the reason the
/// stored `CREATE` text isn't searched for the clause instead: `WITHOUT ROWID`
/// can be separated by a comment or a newline, may be followed by `, STRICT`, and
/// a column named `without_rowid` reads identically to a substring match. A view,
/// or a name that isn't there, answers `false` — neither has a rowid either.
fn has_rowid(conn: &SqliteConn, table: &str) -> bool {
    let wr: Option<i64> = conn
        .query_row(
            "SELECT wr FROM pragma_table_list(?1) WHERE schema = ?2 AND type = 'table'",
            rusqlite::params![table, MAIN],
            |r| r.get(0),
        )
        .ok();
    wr == Some(0)
}

/// The spelling of `table`'s implicit row key to project, or `None` for a table
/// that has none to reach.
///
/// Which spelling matters, and that is the whole reason this is a choice rather
/// than the constant `"rowid"`. All three of [`ROWID_ALIASES`] name the rowid —
/// but only while no *declared* column has taken the name, because SQLite lets a
/// table define a column called `rowid` and then that column is what the word
/// means. Projecting the first unshadowed spelling keeps the generated `SELECT`
/// and the `WHERE` the write-back builds from it referring to the same value; a
/// table that has taken all three has no way left to name its rowid, so it has no
/// implicit key and stays read-only, which is the conservative answer.
fn implicit_row_key(columns: &[ColumnInfo], has_rowid: bool) -> Option<String> {
    if !has_rowid {
        return None;
    }
    ROWID_ALIASES
        .iter()
        .find(|alias| !columns.iter().any(|c| c.name.eq_ignore_ascii_case(alias)))
        .map(|alias| (*alias).to_string())
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

/// Run a DDL plan, all or nothing.
///
/// **SQLite's DDL is transactional**, which MySQL's is not, so this backend can
/// do what the MySQL path cannot: wrap the whole plan in one transaction and
/// roll it back whole. That is why every [`crate::DdlError`] from here carries
/// `applied: 0` — a half-applied plan is a state this engine never leaves behind,
/// so there is no partial progress for the report to have to admit to.
///
/// What may reach here is decided in `core::ddl::supports_change`, not here: the
/// menus hide what SQLite can't express and the emitter writes nothing for it, so
/// a plan that arrives is one made of statements this engine has. Anything that
/// slipped through still fails at the engine rather than half-applying.
pub(crate) async fn run_ddl(
    db: &Db,
    stmts: &[String],
    cancel: CancellationToken,
) -> Result<(), crate::DdlError> {
    let fail = |at: usize, message: String| crate::DdlError {
        message,
        at,
        applied: 0,
    };
    if stmts.is_empty() {
        return Ok(());
    }
    let conn = tokio::task::block_in_place(|| open(db)).map_err(|e| fail(0, format!("{e}")))?;

    tokio::task::block_in_place(|| {
        if cancel.is_cancelled() {
            return Err(fail(0, "the plan was cancelled".into()));
        }
        // **Enforcement off for the duration, and verified before the commit.**
        // Outside the transaction, because SQLite ignores this pragma inside
        // one.
        //
        // Enforcing it *during* a plan is not the safe reading it looks like.
        // With foreign keys on, the rebuild's `DROP TABLE` on a parent is an
        // implicit `DELETE FROM parent`, which fires `ON DELETE CASCADE` and
        // empties the child tables — the table comes back exactly as the user
        // drew it and another table has quietly lost every row. That is the
        // reason step 1 of SQLite's own twelve-step procedure turns them off,
        // and a test here does the cascade to prove it.
        //
        // Nothing is given up by doing so: `PRAGMA foreign_key_check` below runs
        // against the finished state and refuses the commit if the plan left a
        // reference dangling, which is a stricter question than the per-statement
        // one — a plan is allowed to pass through states no single statement
        // could.
        let _ = conn.execute_batch("PRAGMA foreign_keys = OFF");
        conn.execute_batch("BEGIN")
            .map_err(|e| fail(0, format!("{e}")))?;

        // Any early return past this point must undo the transaction, so the
        // body runs to a result first and the rollback is applied to it once.
        let result = (|| -> Result<(), crate::DdlError> {
            for (i, sql) in stmts.iter().enumerate() {
                if cancel.is_cancelled() {
                    return Err(fail(i, "the plan was cancelled".into()));
                }
                conn.execute_batch(sql)
                    .map_err(|e| fail(i, format!("{e}")))?;
            }
            // The last statement's index, so a violation is reported against the
            // plan rather than against nothing.
            let last = stmts.len() - 1;
            if let Some(row) = first_fk_violation(&conn).map_err(|e| fail(last, format!("{e}")))? {
                return Err(fail(
                    last,
                    format!("the plan leaves a foreign key pointing at nothing: {row}"),
                ));
            }
            Ok(())
        })();

        match result {
            Ok(()) => conn
                .execute_batch("COMMIT")
                .map_err(|e| fail(stmts.len() - 1, format!("{e}"))),
            Err(e) => {
                // Reported only if the rollback itself fails — the original
                // error is the one worth surfacing.
                match conn.execute_batch("ROLLBACK") {
                    Ok(()) => Err(e),
                    Err(u) => Err(crate::DdlError {
                        message: format!("{} — and the rollback failed too: {u}", e.message),
                        at: e.at,
                        applied: 0,
                    }),
                }
            }
        }
    })
}

/// The first foreign-key violation in the database, described, or `None` when
/// there is none.
///
/// `PRAGMA foreign_key_check` returns one row per violation — the child table,
/// the rowid, the parent it names, and which of that table's foreign keys it
/// was. One is enough to refuse the plan, and naming it is what makes the
/// refusal actionable rather than a bare "constraint failed".
fn first_fk_violation(conn: &SqliteConn) -> Result<Option<String>, DbError> {
    let mut stmt = conn
        .prepare("SELECT \"table\", \"parent\" FROM pragma_foreign_key_check() LIMIT 1")
        .map_err(query_err)?;
    let mut rows = stmt
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })
        .map_err(query_err)?;
    match rows.next() {
        None => Ok(None),
        Some(row) => {
            let (child, parent) = row.map_err(query_err)?;
            Ok(Some(match parent {
                Some(p) => format!("a row in {child} refers to {p}"),
                None => format!("a row in {child}"),
            }))
        }
    }
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
                for (_, ix) in index_sql(conn, &name)? {
                    out.push('\n');
                    out.push_str(&ix);
                }
                Ok::<_, DbError>(out)
            });
            let create_sql = match create_sql {
                Some(r) => Some(r?),
                None => None,
            };
            // A rowid table always has a key even when it declares none, so a
            // keyless SQLite table is editable where the same table on the other
            // two engines could not be. Views and `WITHOUT ROWID` tables get
            // `None` and go on behaving as they did.
            let implicit_key = (!is_view)
                .then(|| implicit_row_key(&columns, has_rowid(conn, &name)))
                .flatten();
            // One read of the catalogue, two things made from it — the model the
            // *editor* diffs, and the statements a *rebuild* replays. They are
            // not redundant: see `TableInfo::dependent_ddl` for why a rebuild
            // keeps the server's own text rather than re-emitting from a parse.
            let trigger_sql = trigger_sql(conn, &name)?;
            // Parsed out of each trigger's own `CREATE` text — SQLite has no
            // catalogue of the parts. Read for a **view** as well as a table: an
            // `INSTEAD OF` trigger is the only way a SQLite view is written to,
            // and it hangs off the view.
            let triggers = triggers_of(&trigger_sql);
            // What a rebuild has to put back, which is a table's business only.
            let dependent_ddl = if is_view {
                Vec::new()
            } else {
                trigger_statements(&trigger_sql)
            };
            tables.push(TableInfo {
                name,
                schema: None,
                columns,
                indexes,
                foreign_keys,
                is_view,
                implicit_key,
                create_sql,
                // The **body**, not the statement — see `view_body_of`.
                view_definition: is_view.then(|| view_body_of(&sql)).flatten(),
                // SQLite has none of the options the other two engines carry —
                // no definer, no security type, no algorithm, no storage
                // parameters and no check option. It has exactly one, and it is
                // the one the re-create behind every view edit would otherwise
                // drop: the explicit column list.
                view_options: is_view.then(|| ViewOptions {
                    column_list: view_columns_of(&sql),
                    ..Default::default()
                }),
                engine: None,
                collation: None,
                comment: None,
                // Read out of the table's own `CREATE` text, there being no
                // pragma for them. They have to be *modelled* rather than left
                // to `create_sql`, because a rebuild writes the new table from
                // the draft: a check missing from the draft is a check the
                // rebuild silently drops.
                check_constraints: if is_view { Vec::new() } else { checks_of(&sql) },
                triggers,
                dependent_ddl,
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

/// The `CREATE INDEX` statements SQLite stores for `table`, each terminated and
/// paired with the index's name, in catalogue order.
///
/// Only the ones the user wrote: an index SQLite created itself to back a
/// `UNIQUE` or `PRIMARY KEY` constraint has a **NULL** `sql`, because it is part
/// of the table's own declaration and re-issuing it would be an error.
///
/// One query behind two consumers, the same pairing [`trigger_sql`] has: Copy
/// DDL appends the statements to the table's own text ([`fetch_schema`]), and
/// [`table_indexes`] hands each one to its index as [`IndexInfo::create_sql`],
/// where a rebuild can replay it instead of re-emitting an index it only partly
/// read.
fn index_sql(conn: &SqliteConn, table: &str) -> Result<Vec<(String, String)>, DbError> {
    let mut stmt = conn
        .prepare(
            "SELECT name, sql FROM sqlite_master \
             WHERE type = 'index' AND tbl_name = ?1 AND sql IS NOT NULL \
             ORDER BY name",
        )
        .map_err(query_err)?;
    let rows = stmt
        .query_map([table], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(query_err)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(query_err)
        // SQLite stores the statement without its terminator; every consumer
        // here is stringing statements together, so it goes back on once.
        .map(|v| {
            v.into_iter()
                .map(|(n, s)| (n, format!("{};", s.trim().trim_end_matches(';'))))
                .collect()
        })
}

/// The `CREATE TRIGGER` statements SQLite stores for `table`, each terminated,
/// in catalogue order.
///
/// **A trigger is owned by its table and dropped with it**, so these are what
/// the twelve-step rebuild has to put back (`TableInfo::dependent_ddl`). Views
/// are not here and need none of this: `DROP TABLE` leaves a view that selects
/// from the table in place — SQLite resolves a view's references when it runs,
/// not when it is declared — and the table comes back under the same name, so
/// the view is whole again by the end of the transaction.
fn trigger_statements(raw: &[String]) -> Vec<String> {
    raw.iter()
        // SQLite stores the statement without its terminator, and a trigger body
        // is full of internal `;` — so the one that ends it has to be added back
        // or the replay runs into whatever follows.
        .map(|s| format!("{};", s.trim().trim_end_matches(';')))
        .collect()
}

/// The raw `CREATE TRIGGER` text SQLite stores for `table`, exactly as the
/// catalogue holds it — unterminated, and carrying the user's own spacing and
/// comments. The one query behind both consumers: [`trigger_statements`], which
/// terminates it for replay, and [`triggers_of`], which parses it into the
/// model.
fn trigger_sql(conn: &SqliteConn, table: &str) -> Result<Vec<String>, DbError> {
    let mut stmt = conn
        .prepare(
            "SELECT sql FROM sqlite_master \
             WHERE type = 'trigger' AND tbl_name = ?1 AND sql IS NOT NULL \
             ORDER BY name",
        )
        .map_err(query_err)?;
    let rows = stmt
        .query_map([table], |r| r.get::<_, String>(0))
        .map_err(query_err)?;
    rows.collect::<Result<Vec<String>, _>>().map_err(query_err)
}

/// The triggers on `table`, as the model holds them.
///
/// SQLite publishes no catalogue of a trigger's parts, so each one is *parsed*
/// out of its own `CREATE TRIGGER` text (`ddl::sqlite_trigger_info`) — this is
/// the one engine where introspecting a trigger means reading SQL.
///
/// **A statement it can't read is left out rather than guessed at**, the same
/// direction `view_body_of` refuses in. That is safe in the one way that
/// matters: `ddl::diff_triggers` only drops what the *server copy* lists, so a
/// trigger missing from this list is never touched by an edit to its
/// neighbours — where a trigger read *wrong* would be dropped and recreated
/// wrong. It also stays out of the rebuild's way, which replays
/// `TableInfo::dependent_ddl` verbatim and never consults this list.
fn triggers_of(raw: &[String]) -> Vec<TriggerInfo> {
    raw.iter()
        .filter_map(|s| schemaic_core::ddl::sqlite_trigger_info(s))
        .collect()
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

/// A view's explicit **column list** — the `(x, y)` of `CREATE VIEW v (x, y) AS
/// …` — read out of the statement `sqlite_master` stores, without its
/// parentheses. `None` for the usual view, which takes its column names from its
/// body.
///
/// The third positional reader over the shared lexer, beside [`view_body_of`]
/// and `checks_of`, and it splits the same header: the list is the one
/// parenthesised group that can appear *before* the `AS` at code position and
/// paren depth zero. Everything after that `AS` is the user's own SQL, where a
/// `(` is arithmetic or a subquery rather than a column list.
///
/// Kept **verbatim**, quoting and spacing included. SQLite hands back whatever
/// the list was written with, and it goes straight back out again on the
/// re-create that every view edit there performs
/// ([`schemaic_core::ddl::supports_or_replace_view`]) — parsing the names apart
/// and re-quoting them is a way to change them, and this list is what stops the
/// view's columns silently taking the body's names.
fn view_columns_of(create_sql: &str) -> Option<String> {
    use schemaic_core::intel::SqlDialect;
    use schemaic_core::sql::{is_word_byte, is_word_start, skip_noncode};

    let b = create_sql.as_bytes();
    let mut i = 0usize;
    let mut depth = 0i32;
    let mut seen_create = false;
    // The most recent depth-0 parenthesised group, as byte bounds of its
    // contents. Only the one still in hand when the header's `AS` arrives is a
    // column list.
    let mut group: Option<(usize, usize)> = None;
    let mut open: Option<usize> = None;
    while i < b.len() {
        if let Some(j) = skip_noncode(b, i, SqlDialect::Sqlite) {
            i = j.max(i + 1);
            continue;
        }
        match b[i] {
            b'(' => {
                if depth == 0 {
                    open = Some(i + 1);
                }
                depth += 1;
                i += 1;
                continue;
            }
            b')' => {
                depth -= 1;
                if depth == 0
                    && let Some(start) = open.take()
                {
                    group = Some((start, i));
                }
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
            // hand back a list read out of something else.
            if !word.eq_ignore_ascii_case("CREATE") {
                return None;
            }
            seen_create = true;
        } else if depth == 0 && word.eq_ignore_ascii_case("AS") {
            let (s, e) = group?;
            let cols = create_sql[s..e].trim();
            return (!cols.is_empty()).then(|| cols.to_string());
        }
        i = end;
    }
    // No header-ending `AS`: the statement is unreadable, and a list without the
    // body it names is nothing to hand back.
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

/// Every `CHECK` constraint a table declares, in declaration order.
///
/// **No pragma exposes these**, so the table's own `CREATE TABLE` text is the
/// only source — the same position `generated_expr_of` is in, and the same tools
/// answer it: the shared boundary lexer, so a column called `check_sum` or the
/// word inside a string or a comment can't match, and
/// [`schemaic_core::sql::balanced_paren_span`] for the predicate, which may
/// perfectly well contain a comma or a `')'` inside a literal.
///
/// It reads a column-level `CHECK` and a table constraint alike, because SQLite
/// makes no distinction between them once the table exists — both constrain the
/// table, and a rebuild restates both as table constraints. A constraint written
/// without `CONSTRAINT <name>` comes back with an empty name, which is the
/// honest answer: most SQLite checks have none, and inventing one would make a
/// rebuild look like it renamed something.
fn checks_of(create_sql: &str) -> Vec<CheckInfo> {
    use schemaic_core::intel::SqlDialect;
    use schemaic_core::sql::{balanced_paren_span, is_word_byte, is_word_start, skip_noncode};

    let b = create_sql.as_bytes();
    let mut out: Vec<CheckInfo> = Vec::new();
    // Step into the table body. Everything before its `(` is the header, where a
    // quoted table name could otherwise be mistaken for content.
    let mut i = 0usize;
    let body = loop {
        if i >= b.len() {
            return out;
        }
        if let Some(j) = skip_noncode(b, i, SqlDialect::Sqlite) {
            i = j.max(i + 1);
            continue;
        }
        if b[i] == b'(' {
            break i + 1;
        }
        i += 1;
    };

    let mut i = body;
    let mut depth = 1i32;
    // The name from a `CONSTRAINT <name>` seen since the last comma, and whether
    // anything has been read in this item at all — both reset per item, so a
    // name can't carry across into the constraint that follows it.
    let mut pending: Option<String> = None;
    while i < b.len() && depth > 0 {
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
            b',' if depth == 1 => {
                pending = None;
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
        // Only ever at the item's own level: a `CHECK` nested inside another
        // constraint's parens is part of that predicate, not a new constraint.
        if depth == 1 && word.eq_ignore_ascii_case("CONSTRAINT") {
            let (name, next) = ident_at(create_sql, end);
            pending = name;
            i = next;
            continue;
        }
        if depth == 1 && word.eq_ignore_ascii_case("CHECK") {
            let mut k = end;
            while k < b.len() && b[k].is_ascii_whitespace() {
                k += 1;
            }
            if b.get(k) == Some(&b'(')
                && let Some(close) = balanced_paren_span(b, k, SqlDialect::Sqlite)
            {
                out.push(CheckInfo {
                    name: pending.take().unwrap_or_default(),
                    expression: create_sql[k + 1..close].trim().to_string(),
                    enforced: true,
                    validated: true,
                    inherited: true,
                    ..Default::default()
                });
                i = close + 1;
                continue;
            }
        }
        i = end;
    }
    out
}

/// The identifier starting at or after `at`, unquoted, and the offset just past
/// it. SQLite accepts four spellings of a quoted name (`"x"`, `` `x` ``, `[x]`
/// and `'x'`), and a doubled quote inside one is a literal quote.
fn ident_at(sql: &str, at: usize) -> (Option<String>, usize) {
    let b = sql.as_bytes();
    let mut i = at;
    while i < b.len() && b[i].is_ascii_whitespace() {
        i += 1;
    }
    if i >= b.len() {
        return (None, i);
    }
    let close = match b[i] {
        b'"' => b'"',
        b'`' => b'`',
        b'\'' => b'\'',
        b'[' => b']',
        _ => {
            let start = i;
            while i < b.len() && schemaic_core::sql::is_word_byte(b[i]) {
                i += 1;
            }
            return if i > start {
                (Some(sql[start..i].to_string()), i)
            } else {
                (None, i)
            };
        }
    };
    let mut name = String::new();
    i += 1;
    while i < b.len() {
        if b[i] == close {
            // A doubled closing quote is one literal character — `[x]` has no
            // such rule, its content simply runs to the first `]`.
            if close != b']' && b.get(i + 1) == Some(&close) {
                name.push(close as char);
                i += 2;
                continue;
            }
            return (Some(name), i + 1);
        }
        let ch_len = sql[i..].chars().next().map_or(1, char::len_utf8);
        name.push_str(&sql[i..i + ch_len]);
        i += ch_len;
    }
    (Some(name), i)
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
///
/// Each index also carries the statement that declared it ([`index_sql`]), which
/// is the only complete record of the ones the pragmas read `lossy`.
fn table_indexes(conn: &SqliteConn, table: &str) -> Result<Vec<IndexInfo>, DbError> {
    let declared: HashMap<String, String> = index_sql(conn, table)?.into_iter().collect();
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
            // Keyed on the catalogue name, not the `PRIMARY` this may have been
            // renamed to — and absent for exactly the indexes SQLite declares
            // itself, which have no statement of their own to keep.
            create_sql: declared.get(&name).cloned(),
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
        // And the statement that declared it, terminated — what a rebuild
        // replays when the pragmas above could only partly read the index.
        assert_eq!(
            explicit.create_sql.as_deref(),
            Some("CREATE UNIQUE INDEX album_title ON album(title DESC);")
        );
    }

    /// An index SQLite wrote itself has **no statement of its own** — its `sql`
    /// is NULL, because it is part of the table's declaration. Carrying the
    /// table's text here would be the wrong string in the right field, and
    /// replaying it a second `CREATE TABLE`.
    #[test]
    fn an_index_the_engine_declared_carries_no_text() {
        let conn = SqliteConn::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE t (id INTEGER PRIMARY KEY, email TEXT UNIQUE);")
            .unwrap();
        let ix = table_indexes(&conn, "t").unwrap();
        let backing = ix
            .iter()
            .find(|i| i.constraint.is_some())
            .expect("the UNIQUE constraint's index");
        assert_eq!(backing.create_sql, None);
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

    // ── the reserved-word lists, checked against SQLite itself ────────────

    /// Every keyword this build of SQLite knows, from **SQLite's own table**
    /// (`sqlite3_keyword_name`) rather than a list copied out of the docs. That
    /// is what makes the test below a standing guard instead of a snapshot: a
    /// keyword added by a future release arrives here on its own and fails the
    /// assertion until somebody looks at it.
    fn sqlite_keywords() -> Vec<String> {
        let mut out = Vec::new();
        // SAFETY: the two calls are SQLite's documented keyword-table accessors.
        // They touch no connection and no shared state, and `sqlite3_keyword_name`
        // hands back a pointer to a static string plus its length, which is read
        // and copied before anything else runs.
        unsafe {
            for i in 0..rusqlite::ffi::sqlite3_keyword_count() {
                let mut p: *const std::os::raw::c_char = std::ptr::null();
                let mut len: std::os::raw::c_int = 0;
                if rusqlite::ffi::sqlite3_keyword_name(i, &mut p, &mut len)
                    != rusqlite::ffi::SQLITE_OK
                    || p.is_null()
                {
                    continue;
                }
                let bytes = std::slice::from_raw_parts(p as *const u8, len as usize);
                if let Ok(s) = std::str::from_utf8(bytes) {
                    out.push(s.to_string());
                }
            }
        }
        assert!(out.len() > 100, "SQLite reported {} keywords", out.len());
        out
    }

    /// Run `setup`, then compile `probe`, **on one connection** — a fresh
    /// connection per statement would fail every `SELECT … FROM <just-created>`
    /// with "no such table" and make every keyword look reserved.
    fn compiles(setup: &str, probe: &str) -> bool {
        let c = SqliteConn::open_in_memory().unwrap();
        c.execute_batch(setup).is_ok() && c.prepare(probe).is_ok()
    }

    /// The reserved-word lists say what SQLite will refuse, so SQLite is what
    /// they are checked against — not a reading of its documentation.
    ///
    /// Two lists because there are two questions with **opposite** costs. The
    /// quoter must cover every word that can't be a bare identifier, or it emits
    /// SQL that doesn't parse; the alias diagnostic must cover only words that
    /// can't be an alias, or it squiggles working SQL. They are not the same set:
    /// `CAST`, `IF` and `RAISE` need quoting as a column or table name yet are
    /// perfectly good `AS` aliases.
    #[test]
    fn the_reserved_lists_match_what_sqlite_itself_refuses() {
        use schemaic_core::intel::{SqlDialect, is_reserved_word, must_quote_ident};
        let mut quote_wrong = Vec::new();
        let mut alias_wrong = Vec::new();
        for kw in sqlite_keywords() {
            // Usable as a bare column name, and as a bare table name?
            let as_ident = compiles(
                &format!("CREATE TABLE zz ({kw} INTEGER);"),
                &format!("SELECT {kw} FROM zz"),
            ) && compiles(
                &format!("CREATE TABLE {kw} (x INTEGER);"),
                &format!("SELECT * FROM {kw}"),
            );
            // Usable as a bare `AS` alias?
            let as_alias = compiles("CREATE TABLE t (x INTEGER);", &format!("SELECT 1 AS {kw}"));

            if must_quote_ident(&kw, SqlDialect::Sqlite) != !as_ident {
                quote_wrong.push(format!(
                    "{kw}: SQLite {} it bare as an identifier, we say must_quote={}",
                    if as_ident { "accepts" } else { "refuses" },
                    must_quote_ident(&kw, SqlDialect::Sqlite)
                ));
            }
            if is_reserved_word(&kw, SqlDialect::Sqlite) != !as_alias {
                alias_wrong.push(format!(
                    "{kw}: SQLite {} it as an alias, we say reserved={}",
                    if as_alias { "accepts" } else { "refuses" },
                    is_reserved_word(&kw, SqlDialect::Sqlite)
                ));
            }
        }
        assert!(
            quote_wrong.is_empty(),
            "must_quote_ident disagrees with SQLite:\n  {}",
            quote_wrong.join("\n  ")
        );
        assert!(
            alias_wrong.is_empty(),
            "is_reserved_word disagrees with SQLite:\n  {}",
            alias_wrong.join("\n  ")
        );
    }

    /// The end of the chain: tables and columns named with the words that were
    /// missing, opened through the statement the tree generates, against real
    /// SQLite.
    ///
    /// **Each word is used in the position where it actually breaks**, which is
    /// not the same position for all of them: `CAST` and `RAISE` are refused as a
    /// bare *column* name but accepted as a table's, and `IF` is the reverse. A
    /// first draft of this test put each one on the side it tolerates and passed
    /// with the fix reverted.
    #[tokio::test]
    async fn a_table_named_for_a_keyword_still_opens() {
        let (keeper, db) = shared_memory("kw_named_table");
        keeper
            .execute_batch(
                r#"CREATE TABLE "if" ("cast" INTEGER PRIMARY KEY, "raise" TEXT);
                   INSERT INTO "if" VALUES (1, 'x');
                   CREATE TABLE "nothing" ("nothing" TEXT PRIMARY KEY);
                   INSERT INTO "nothing" VALUES ('y');"#,
            )
            .unwrap();
        let schema = fetch_schema(&db).await.expect("introspect");
        for name in ["if", "nothing"] {
            let t = schema
                .tables
                .iter()
                .find(|t| t.name == name)
                .unwrap_or_else(|| panic!("{name} missing from the catalogue"));
            let pk: Vec<String> = t
                .columns
                .iter()
                .filter(|c| c.primary_key)
                .map(|c| c.name.clone())
                .collect();
            let sql = schemaic_core::filter::table_query(
                schemaic_core::intel::SqlDialect::Sqlite,
                MAIN,
                None,
                &t.name,
                schemaic_core::filter::BrowseKey::pick(&pk, t.implicit_key.as_deref()),
                schemaic_core::filter::Order::Asc,
                10,
            );
            let rs = run_query(&keeper, &sql, 10).unwrap_or_else(|e| {
                panic!("generated statement does not run: {sql}\n  {e}");
            });
            assert_eq!(rs.row_count(), 1, "{sql}");
        }
    }

    /// The three words that separate the two lists, named so the distinction
    /// can't be collapsed back into one by someone tidying up.
    #[test]
    fn a_word_can_need_quoting_as_a_name_yet_be_a_fine_alias() {
        use schemaic_core::intel::{SqlDialect, is_reserved_word, must_quote_ident};
        for w in ["CAST", "IF", "RAISE"] {
            assert!(must_quote_ident(w, SqlDialect::Sqlite), "{w}");
            assert!(!is_reserved_word(w, SqlDialect::Sqlite), "{w}");
        }
        // `NOTHING` (from `ON CONFLICT DO NOTHING`) is refused in both positions,
        // so it belongs to both lists.
        assert!(must_quote_ident("NOTHING", SqlDialect::Sqlite));
        assert!(is_reserved_word("NOTHING", SqlDialect::Sqlite));
    }

    // ── the implicit key: a keyless table's rowid ─────────────────────────

    /// A keyless table with a `rowid` column of its own, one with the first two
    /// spellings taken, and one with nothing taken.
    fn shadowing() -> SqliteConn {
        let conn = SqliteConn::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE plain    (a TEXT, b TEXT);
             CREATE TABLE owns_it  (rowid TEXT, b TEXT);
             CREATE TABLE owns_two (RowId TEXT, _ROWID_ TEXT);
             CREATE TABLE owns_all (rowid TEXT, _rowid_ TEXT, oid TEXT);
             CREATE TABLE wr (a TEXT, b TEXT, PRIMARY KEY (a)) WITHOUT ROWID;
             CREATE VIEW v AS SELECT a FROM plain;
             INSERT INTO plain VALUES ('no', 'key');",
        )
        .unwrap();
        conn
    }

    /// One table's schema as `fetch_schema` would assemble it, without the async
    /// `Db` round trip — enough for `analyze_edit`, which reads the columns, the
    /// indexes and nothing else.
    fn table_info_of(conn: &SqliteConn, name: &str) -> TableInfo {
        let columns = table_columns(conn, name).unwrap();
        let implicit_key = implicit_row_key(&columns, has_rowid(conn, name));
        TableInfo {
            name: name.to_string(),
            indexes: table_indexes(conn, name).unwrap(),
            columns,
            implicit_key,
            ..Default::default()
        }
    }

    /// A rowid table always has a rowid; a `WITHOUT ROWID` table never does, and
    /// nor does a view or a name that isn't there. `PRAGMA table_list` is asked
    /// rather than the `CREATE` text searched.
    #[test]
    fn has_rowid_answers_for_tables_views_and_without_rowid() {
        let conn = shadowing();
        assert!(has_rowid(&conn, "plain"));
        assert!(has_rowid(&conn, "owns_it"));
        assert!(!has_rowid(&conn, "wr"));
        assert!(!has_rowid(&conn, "v"));
        assert!(!has_rowid(&conn, "nonexistent"));
    }

    /// The spelling is chosen, not fixed: a declared column takes the name away,
    /// and a table that has taken all three has no way left to name its rowid.
    #[test]
    fn implicit_row_key_picks_the_first_spelling_no_column_has_taken() {
        let conn = shadowing();
        let cols = |t: &str| table_columns(&conn, t).unwrap();
        let key = |t: &str| implicit_row_key(&cols(t), has_rowid(&conn, t));
        assert_eq!(key("plain").as_deref(), Some("rowid"));
        assert_eq!(key("owns_it").as_deref(), Some("_rowid_"));
        // Case-insensitively taken — `RowId` and `_ROWID_` are the same names.
        assert_eq!(key("owns_two").as_deref(), Some("oid"));
        assert_eq!(key("owns_all"), None);
        // No rowid to reach, whatever the columns are called.
        assert_eq!(key("wr"), None);
    }

    /// The point of the whole change: a table with no primary key and no unique
    /// index becomes editable, keyed on a rowid the projection asks for by name.
    #[test]
    fn a_keyless_table_is_editable_through_its_projected_rowid() {
        let conn = shadowing();
        let o = origins_for(&conn, "SELECT rowid, * FROM plain");
        assert_eq!(o.len(), 3, "the wildcard expands after the leading item");
        let key = o[0].as_ref().expect("the rowid is attributed");
        assert!(key.implicit_key);
        assert_eq!(key.column, "rowid");
        assert_eq!(key.table, "plain");
        // The data columns are ordinary origins — the leading item didn't shift
        // the wildcard's expansion by one.
        assert_eq!(o[1].as_ref().unwrap().column, "a");
        assert_eq!(o[2].as_ref().unwrap().column, "b");
        assert!(!o[1].as_ref().unwrap().implicit_key);

        // And the editing system reaches the same conclusion end to end.
        let rs = run_query(&conn, "SELECT rowid, * FROM plain", 100).unwrap();
        let m = schemaic_core::edit::analyze_edit(&rs, |_, _, t| Some(table_info_of(&conn, t)));
        assert_eq!(m.table(0).map(|t| t.key_cols.clone()), Some(vec![0]));
        assert!(m.editable(1) && m.editable(2));
        assert!(!m.editable(0), "the key is a handle, not the table's data");
    }

    /// Without the rowid projected, the same table is exactly as read-only as it
    /// was — nothing here makes a bare `SELECT *` editable by guessing.
    #[test]
    fn the_same_keyless_table_stays_read_only_without_the_rowid() {
        let conn = shadowing();
        let rs = run_query(&conn, "SELECT * FROM plain", 100).unwrap();
        let m = schemaic_core::edit::analyze_edit(&rs, |_, _, t| Some(table_info_of(&conn, t)));
        assert!(!m.editable(0));
        assert!(m.insert_target().is_none());
    }

    /// A table that declares its own `rowid` column means *that* column by the
    /// name, and SQLite agrees — so nothing is synthesised, and the duplicate the
    /// projection now contains is caught by the edit model's own self-join guard.
    #[test]
    fn a_declared_rowid_column_wins_over_the_implicit_one() {
        let conn = shadowing();
        let o = origins_for(&conn, "SELECT rowid, * FROM owns_it");
        let first = o[0].as_ref().expect("the declared column is attributed");
        assert!(!first.implicit_key);
        assert_eq!(first.column, "rowid");
        // `rowid` is now exposed twice — once named, once through the wildcard.
        assert_eq!(o[1].as_ref().unwrap().column, "rowid");
    }

    /// A `WITHOUT ROWID` table has no rowid to name, and SQLite will not even
    /// prepare the statement that asks for one. Nothing about it changes.
    #[test]
    fn a_without_rowid_table_cannot_be_asked_for_a_rowid() {
        let conn = shadowing();
        assert!(conn.prepare("SELECT rowid, * FROM wr").is_err());
        let cols = table_columns(&conn, "wr").unwrap();
        assert_eq!(implicit_row_key(&cols, has_rowid(&conn, "wr")), None);
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
    pub(super) fn shared_memory(name: &str) -> (SqliteConn, Db) {
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

    /// The implicit key end to end: read a keyless table, resolve its key, and
    /// commit an `UPDATE` and a `DELETE` through it.
    ///
    /// The two rows are **identical** on purpose. That is legal in a table with no
    /// key, it is the case a "match the row by all its values" scheme gets wrong,
    /// and it is the reason the 1-row net would fire if the key weren't really
    /// unique — the `WHERE` here matches exactly one of them.
    #[tokio::test]
    async fn a_keyless_table_writes_back_through_its_rowid() {
        let (keeper, db) = shared_memory("rowid_writeback");
        keeper
            .execute_batch(
                "CREATE TABLE t (a TEXT, b TEXT);
                 INSERT INTO t VALUES ('same', 'same'), ('same', 'same'), ('third', 'row');",
            )
            .unwrap();

        // The key is resolved the way the app resolves it, not hand-written.
        let rs = run_query(&keeper, "SELECT rowid, * FROM t", 100).unwrap();
        let m =
            schemaic_core::edit::analyze_edit(&rs, |_, _, name| Some(table_info_of(&keeper, name)));
        let tbl = m.insert_target().expect("a single writable table");
        assert_eq!(tbl.key_cols, vec![0]);
        let key_of = |row: usize| {
            let ci = tbl.key_cols[0];
            vec![(
                rs.columns[ci].origin.as_ref().unwrap().column.clone(),
                rs.cell(row, ci).unwrap().to_value(),
            )]
        };

        let write = GridWrite {
            updates: vec![RowEdit {
                database: MAIN.to_string(),
                schema: None,
                table: "t".to_string(),
                set: vec![("b".to_string(), Some("edited".to_string()))],
                key: key_of(1),
            }],
            deletes: vec![RowDelete {
                database: MAIN.to_string(),
                schema: None,
                table: "t".to_string(),
                key: key_of(2),
            }],
            ..Default::default()
        };
        let n = commit_writes(&db, &write, CancellationToken::new())
            .await
            .expect("commit");
        assert_eq!(n, 2);

        let rows: Vec<(i64, String, String)> = keeper
            .prepare("SELECT rowid, a, b FROM t ORDER BY rowid")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        // Only the second of the two identical rows changed, and the third is gone.
        assert_eq!(
            rows,
            [
                (1, "same".to_string(), "same".to_string()),
                (2, "same".to_string(), "edited".to_string()),
            ]
        );
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

    /// The splice path keyed on a rowid. An UPDATE-only commit re-fetches its
    /// edited rows in place rather than re-running the query, and the key it
    /// re-fetches by is whatever `analyze_edit` resolved — so a keyless table's
    /// `WHERE` here names a column the table doesn't have. SQLite resolves
    /// `rowid` in a `SELECT` list and a `WHERE` alike, which is what makes the
    /// re-fetch work with no special case; this is the test that says so.
    #[tokio::test]
    async fn a_refetch_reads_a_keyless_row_back_by_its_rowid() {
        let (keeper, db) = shared_memory("refetch_rowid");
        keeper
            .execute_batch(
                "CREATE TABLE t (a TEXT, b TEXT);
                 INSERT INTO t VALUES ('one', 'x'), ('two', 'y');",
            )
            .unwrap();
        let rs = run_query(&keeper, "SELECT rowid, * FROM t", 100).unwrap();
        let m =
            schemaic_core::edit::analyze_edit(&rs, |_, _, name| Some(table_info_of(&keeper, name)));
        let template =
            schemaic_core::edit::refetch_template(&rs, &m).expect("a keyless table is spliceable");
        assert_eq!(template.columns, ["rowid", "a", "b"]);
        assert_eq!(template.key_cols, vec![0]);

        keeper
            .execute("UPDATE t SET b = 'written' WHERE rowid = 2", [])
            .unwrap();
        let got = refetch_rows(
            &db,
            &template,
            &[RefetchRow {
                data_row: 1,
                key: vec![Value::Int(2)],
            }],
            CancellationToken::new(),
        )
        .await
        .expect("refetch");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, 1);
        assert_eq!(got[0].1[0].display(), "2", "the rowid comes back too");
        assert_eq!(got[0].1[2].display(), "written");
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

    /// `CREATE VIEW v (x, y) AS …` names the view's columns independently of its
    /// body, and SQLite is the only engine that reports the two separately — the
    /// other two bake the names into the definition they hand back. Since every
    /// edit to a SQLite view is a drop and a re-create, a list read wrong is a
    /// list *emitted* wrong, and the view's columns quietly take the body's
    /// names instead.
    #[test]
    fn a_views_column_list_is_read_out_of_its_create_statement() {
        for (sql, cols) in [
            ("CREATE VIEW v (x, y) AS SELECT a, b FROM t", "x, y"),
            // No space before the paren, and none inside it.
            ("CREATE VIEW v(x,y) AS SELECT 1,2", "x,y"),
            // Each of SQLite's three quotings, kept verbatim: re-quoting a
            // parsed list is a way to change it.
            (
                r#"CREATE VIEW v ("odd name", [b], `c`) AS SELECT 1, 2, 3"#,
                r#""odd name", [b], `c`"#,
            ),
            // A column *named* `as` doesn't end the header — it's quoted, and
            // the lexer skips it whole.
            (r#"CREATE VIEW v ("as") AS SELECT 1"#, r#""as""#),
            // …and neither does a view named `as`.
            (r#"CREATE VIEW "as" (a) AS SELECT 1"#, "a"),
            ("CREATE TEMP VIEW IF NOT EXISTS main.v (a) AS SELECT 1", "a"),
        ] {
            assert_eq!(view_columns_of(sql).as_deref(), Some(cols), "{sql}");
        }
    }

    /// The usual view has no column list, and parentheses in the *body* are not
    /// one — the list can only appear before the `AS` that ends the header.
    #[test]
    fn a_view_without_a_column_list_reports_none() {
        for sql in [
            "CREATE VIEW v AS SELECT 1",
            "CREATE VIEW v AS SELECT (1 + 2)",
            "CREATE VIEW v AS SELECT a FROM (SELECT 1 AS a)",
            // An unreadable header yields nothing, the same direction
            // `view_body_of` refuses in: a list guessed wrong would be emitted.
            "CREATE VIEW v (a, b)",
            "SELECT 1",
            "",
        ] {
            assert_eq!(view_columns_of(sql), None, "{sql:?}");
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

/// `run_ddl` on SQLite — see [`crate::Db::run_ddl`] for why this exists at all.
#[cfg(test)]
mod ddl_tests {
    use super::tests::shared_memory;
    use super::*;
    use tokio_util::sync::CancellationToken;

    fn objects(keeper: &SqliteConn, name: &str) -> i64 {
        keeper
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name = ?1",
                [name],
                |r| r.get(0),
            )
            .unwrap()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_plan_drops_a_table() {
        let (keeper, db) = shared_memory("ddl_drop_table");
        keeper
            .execute_batch("CREATE TABLE t (id INTEGER PRIMARY KEY);")
            .unwrap();
        db.run_ddl(
            MAIN,
            &["DROP TABLE \"t\";".to_string()],
            CancellationToken::new(),
        )
        .await
        .expect("SQLite can drop a table");
        assert_eq!(objects(&keeper, "t"), 0);
    }

    /// **Every column shape the fast path calls native, run at real SQLite.**
    ///
    /// This is the test the `ADD COLUMN` fast path actually rests on. Its
    /// failure mode is not a wrong answer but a *half-applied plan*: the rebuild
    /// is skipped, the engine refuses the statement, and the edit the preview
    /// promised is gone. A predicate that drifts away from the engine's
    /// restrictions can't be caught by reasoning about it — only by asking
    /// SQLite, which is what this does, one shape per restriction.
    #[tokio::test(flavor = "multi_thread")]
    async fn every_natively_added_column_is_one_sqlite_accepts() {
        use schemaic_core::ddl::{self, ColumnDraft, TableDraft};
        use schemaic_core::intel::SqlDialect::Sqlite;
        use schemaic_core::schema::ColumnInfo;

        // `ColumnInfo::default()` is NOT NULL with no default, which is a shape
        // SQLite genuinely refuses — so the shapes are built off an addable base
        // rather than off `Default`, or the test proves the opposite of what it
        // says.
        let base = || ColumnInfo {
            name: "c".into(),
            type_name: "TEXT".into(),
            nullable: true,
            ..Default::default()
        };
        let shapes: Vec<(&str, ColumnInfo)> = vec![
            ("plain", base()),
            (
                "not null with a constant default",
                ColumnInfo {
                    nullable: false,
                    default: Some("'x'".into()),
                    ..base()
                },
            ),
            (
                "collated",
                ColumnInfo {
                    collation: Some("NOCASE".into()),
                    ..base()
                },
            ),
            (
                "negative number default",
                ColumnInfo {
                    type_name: "INTEGER".into(),
                    default: Some("-1".into()),
                    ..base()
                },
            ),
            (
                "a default whose parens are inside a literal",
                ColumnInfo {
                    default: Some("'a (b)'".into()),
                    ..base()
                },
            ),
            (
                "generated",
                ColumnInfo {
                    type_name: "INTEGER".into(),
                    generated: Some("a * 2".into()),
                    ..base()
                },
            ),
            (
                "generated and not null",
                ColumnInfo {
                    generated: Some("'x'".into()),
                    nullable: false,
                    ..base()
                },
            ),
        ];

        for (i, (label, shape)) in shapes.into_iter().enumerate() {
            let (keeper, db) = shared_memory(&format!("add_col_{i}"));
            keeper
                .execute_batch("CREATE TABLE t (a INTEGER); INSERT INTO t VALUES (7);")
                .unwrap();
            let schema = fetch_schema(&db).await.expect("schema");
            let table = schema.tables.iter().find(|t| t.name == "t").expect("t");

            let mut draft = TableDraft::from_table(table);
            draft.columns.push(ColumnDraft::new(shape));
            let plan = ddl::diff(table, &draft, Sqlite);
            let sql = plan.emit();
            assert!(
                sql.iter().any(|s| s.contains("ADD COLUMN")),
                "{label} should take the fast path: {sql:?}"
            );
            assert!(
                !sql.iter().any(|s| s.contains("INSERT INTO")),
                "{label} rebuilt instead: {sql:?}"
            );
            db.run_ddl(MAIN, &sql, CancellationToken::new())
                .await
                .unwrap_or_else(|e| panic!("SQLite refused the {label} column: {e}\n{sql:?}"));

            // The column is there, and the row that was already in the table
            // survived — the half-applied plan this test exists to catch would
            // have left one or the other wrong.
            //
            // `table_xinfo`, not `table_info`: the latter omits generated
            // columns entirely, so it reports a successful add as a missing one.
            let cols: Vec<String> = keeper
                .prepare("SELECT name FROM pragma_table_xinfo('t')")
                .unwrap()
                .query_map([], |r| r.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect();
            assert_eq!(cols, ["a", "c"], "{label}");
            let a: i64 = keeper
                .query_row("SELECT a FROM t", [], |r| r.get(0))
                .unwrap();
            assert_eq!(a, 7, "{label} lost the existing row");
        }
    }

    /// Editing a view, end to end through the real `fetch_schema`, the real
    /// differ, the real emitter and real SQLite — because every part of this is
    /// a *different shape* from the other two engines, and each one is only
    /// right if the engine accepts the result.
    ///
    /// The view carries an explicit column list, which is the whole hazard: the
    /// edit is a `DROP` plus a `CREATE` (SQLite has no `CREATE OR REPLACE
    /// VIEW`), so a list left behind doesn't fail — it silently renames the
    /// view's columns to whatever the body calls them.
    #[tokio::test(flavor = "multi_thread")]
    async fn editing_a_view_keeps_its_column_list() {
        use schemaic_core::ddl::{self, ViewDraft};
        use schemaic_core::intel::SqlDialect::Sqlite;

        let (keeper, db) = shared_memory("ddl_view_edit");
        keeper
            .execute_batch(
                "CREATE TABLE t (a INTEGER, b TEXT);
                 CREATE VIEW v (x, y) AS SELECT a, b FROM t;
                 INSERT INTO t VALUES (1, 'one'), (2, 'two');",
            )
            .unwrap();

        let schema = fetch_schema(&db).await.expect("schema");
        let view = schema.tables.iter().find(|t| t.name == "v").expect("view");
        let mut draft = ViewDraft::from_table(view).expect("a view drafts");
        draft.select = "SELECT a, b FROM t WHERE a > 1".into();

        let plan = ddl::diff_view(view, &draft, Sqlite);
        db.run_ddl(MAIN, &plan.emit(), CancellationToken::new())
            .await
            .expect("SQLite accepts the plan");

        // The columns are still the view's own names, not the body's.
        let cols: Vec<String> = keeper
            .prepare("SELECT name FROM pragma_table_info('v')")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(cols, ["x", "y"], "the column list was dropped by the edit");
        // …and the edit itself landed.
        let rows: i64 = keeper
            .query_row("SELECT count(*) FROM v", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 1, "the new WHERE didn't take effect");
    }

    /// A rename, which on SQLite is the same drop-and-create: `ALTER VIEW` isn't
    /// a statement there and `ALTER TABLE … RENAME TO` refuses a view outright.
    #[tokio::test(flavor = "multi_thread")]
    async fn renaming_a_view_lands_under_the_new_name() {
        use schemaic_core::ddl::{self, ViewDraft};
        use schemaic_core::intel::SqlDialect::Sqlite;

        let (keeper, db) = shared_memory("ddl_view_rename");
        keeper
            .execute_batch(
                "CREATE TABLE t (a INTEGER);
                 CREATE VIEW v AS SELECT a FROM t;",
            )
            .unwrap();

        let schema = fetch_schema(&db).await.expect("schema");
        let view = schema.tables.iter().find(|t| t.name == "v").expect("view");
        let mut draft = ViewDraft::from_table(view).expect("a view drafts");
        draft.name = "v2".into();

        let plan = ddl::diff_view(view, &draft, Sqlite);
        db.run_ddl(MAIN, &plan.emit(), CancellationToken::new())
            .await
            .expect("SQLite accepts the rename plan");

        assert_eq!(objects(&keeper, "v"), 0, "the old view is gone");
        assert_eq!(objects(&keeper, "v2"), 1, "the new one is there");
    }

    /// Editing a trigger, end to end: real `fetch_schema` (which for SQLite
    /// means a real *parse* of `sqlite_master`), real differ, real emitter, real
    /// engine. The trigger carries the parts MySQL has no form of — `UPDATE OF`
    /// and a `WHEN` guard — because those are what a reader written to MySQL's
    /// shape would silently drop, leaving a trigger that fires on every column
    /// and never checks its guard.
    #[tokio::test(flavor = "multi_thread")]
    async fn editing_a_trigger_keeps_its_when_guard_and_update_columns() {
        use schemaic_core::ddl::{self, TriggerDraft, TriggerSetDraft};
        use schemaic_core::intel::SqlDialect::Sqlite;
        use schemaic_core::schema::TriggerAction;

        let (keeper, db) = shared_memory("ddl_trigger_edit");
        keeper
            .execute_batch(
                "CREATE TABLE emp (a INTEGER, b TEXT);
                 CREATE TABLE log (n INTEGER);
                 INSERT INTO log VALUES (0);
                 CREATE TRIGGER bump BEFORE UPDATE OF a, b ON emp
                   FOR EACH ROW WHEN NEW.a > OLD.a
                   BEGIN UPDATE log SET n = n + 1; END;",
            )
            .unwrap();

        let schema = fetch_schema(&db).await.expect("schema");
        let table = schema.tables.iter().find(|t| t.name == "emp").expect("emp");
        assert_eq!(table.triggers.len(), 1, "the trigger was read back");
        let cur = &table.triggers[0];
        assert_eq!(cur.update_columns, ["a", "b"]);
        assert_eq!(cur.condition.as_deref(), Some("NEW.a > OLD.a"));

        // Edit only the body; everything else has to survive untouched.
        let mut d = TriggerDraft::from_info(cur);
        d.info.action = TriggerAction::Body("BEGIN UPDATE log SET n = n + 10; END".into());
        let set = TriggerSetDraft {
            schema: None,
            table: "emp".into(),
            triggers: vec![d],
        };
        let plan = ddl::diff_triggers(&table.triggers, &set, Sqlite);
        db.run_ddl(MAIN, &plan.emit(), CancellationToken::new())
            .await
            .expect("SQLite accepts the plan");

        // The guard still holds: an update that doesn't raise `a` must not fire.
        keeper
            .execute_batch("INSERT INTO emp VALUES (5, 'x'); UPDATE emp SET a = 1;")
            .unwrap();
        let n: i64 = keeper
            .query_row("SELECT n FROM log", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "the WHEN guard was dropped by the edit");
        // …and raising it fires the *new* body.
        keeper.execute_batch("UPDATE emp SET a = 9;").unwrap();
        let n: i64 = keeper
            .query_row("SELECT n FROM log", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 10, "the new body didn't take effect");
    }

    /// An `INSTEAD OF` trigger on a **view** — the only way a SQLite view is
    /// written to, and the case that needs triggers read for views and not just
    /// for tables.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_views_instead_of_trigger_is_read_and_rebuilt() {
        use schemaic_core::ddl::{self, TriggerDraft, TriggerSetDraft};
        use schemaic_core::intel::SqlDialect::Sqlite;
        use schemaic_core::schema::{TriggerAction, TriggerTiming};

        let (keeper, db) = shared_memory("ddl_trigger_view");
        keeper
            .execute_batch(
                "CREATE TABLE emp (a INTEGER);
                 CREATE VIEW v AS SELECT a FROM emp;
                 CREATE TRIGGER v_ins INSTEAD OF INSERT ON v
                   BEGIN INSERT INTO emp VALUES (NEW.a); END;",
            )
            .unwrap();

        let schema = fetch_schema(&db).await.expect("schema");
        let view = schema.tables.iter().find(|t| t.name == "v").expect("view");
        assert_eq!(view.triggers.len(), 1, "a view's trigger is read too");
        assert_eq!(view.triggers[0].timing, TriggerTiming::InsteadOf);

        let mut d = TriggerDraft::from_info(&view.triggers[0]);
        d.info.action = TriggerAction::Body("BEGIN INSERT INTO emp VALUES (NEW.a * 2); END".into());
        let set = TriggerSetDraft {
            schema: None,
            table: "v".into(),
            triggers: vec![d],
        };
        let plan = ddl::diff_triggers(&view.triggers, &set, Sqlite);
        db.run_ddl(MAIN, &plan.emit(), CancellationToken::new())
            .await
            .expect("SQLite accepts the plan");

        keeper.execute_batch("INSERT INTO v VALUES (21);").unwrap();
        let a: i64 = keeper
            .query_row("SELECT a FROM emp", [], |r| r.get(0))
            .unwrap();
        assert_eq!(a, 42, "the replaced INSTEAD OF trigger didn't run");
    }

    /// A trigger removed from the draft is dropped, and the drops all run before
    /// the creates — the ordering `trigger_statements` documents.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_trigger_left_out_of_the_draft_is_dropped() {
        use schemaic_core::ddl::{self, TriggerSetDraft};
        use schemaic_core::intel::SqlDialect::Sqlite;

        let (keeper, db) = shared_memory("ddl_trigger_drop");
        keeper
            .execute_batch(
                "CREATE TABLE emp (a INTEGER);
                 CREATE TRIGGER gone AFTER INSERT ON emp BEGIN SELECT 1; END;",
            )
            .unwrap();
        let schema = fetch_schema(&db).await.expect("schema");
        let table = schema.tables.iter().find(|t| t.name == "emp").expect("emp");

        let set = TriggerSetDraft {
            schema: None,
            table: "emp".into(),
            triggers: vec![],
        };
        let plan = ddl::diff_triggers(&table.triggers, &set, Sqlite);
        db.run_ddl(MAIN, &plan.emit(), CancellationToken::new())
            .await
            .expect("SQLite accepts the drop");
        assert_eq!(objects(&keeper, "gone"), 0);
    }

    /// The order the emitter puts them in is the order that works: the index has
    /// to go before the column it names.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_plan_runs_its_statements_in_order() {
        let (keeper, db) = shared_memory("ddl_order");
        keeper
            .execute_batch(
                "CREATE TABLE t (id INTEGER PRIMARY KEY, email TEXT);\
                 CREATE INDEX ix_email ON t (email);",
            )
            .unwrap();
        db.run_ddl(
            MAIN,
            &[
                "DROP INDEX \"ix_email\";".to_string(),
                "ALTER TABLE \"t\" DROP COLUMN \"email\";".to_string(),
            ],
            CancellationToken::new(),
        )
        .await
        .expect("index then column");
        assert_eq!(objects(&keeper, "ix_email"), 0);
        let cols: i64 = keeper
            .query_row("SELECT count(*) FROM pragma_table_info('t')", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(cols, 1, "only id is left");
    }

    /// **All or nothing.** SQLite's DDL is transactional, unlike MySQL's, so a
    /// half-applied plan is a state this backend never has to report.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_failed_plan_leaves_nothing_behind() {
        let (keeper, db) = shared_memory("ddl_rollback");
        keeper
            .execute_batch("CREATE TABLE t (id INTEGER PRIMARY KEY, a TEXT, b TEXT);")
            .unwrap();
        let err = db
            .run_ddl(
                MAIN,
                &[
                    "ALTER TABLE \"t\" DROP COLUMN \"a\";".to_string(),
                    "ALTER TABLE \"t\" DROP COLUMN \"nope\";".to_string(),
                ],
                CancellationToken::new(),
            )
            .await
            .expect_err("the second statement names no column");
        assert_eq!(err.at, 1, "the failure is the second statement");
        assert_eq!(err.applied, 0, "and the first one went back");
        let cols: i64 = keeper
            .query_row("SELECT count(*) FROM pragma_table_info('t')", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(cols, 3, "the dropped column is still there");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_cancelled_plan_applies_nothing() {
        let (keeper, db) = shared_memory("ddl_cancel");
        keeper
            .execute_batch("CREATE TABLE t (id INTEGER PRIMARY KEY);")
            .unwrap();
        let cancel = CancellationToken::new();
        cancel.cancel();
        let err = db
            .run_ddl(MAIN, &["DROP TABLE \"t\";".to_string()], cancel)
            .await
            .expect_err("cancelled before it ran");
        assert!(format!("{err}").to_lowercase().contains("cancel"), "{err}");
        assert_eq!(objects(&keeper, "t"), 1, "the table is untouched");
    }
}

#[cfg(test)]
mod check_text_tests {
    use super::checks_of;

    fn one(sql: &str) -> (String, String) {
        let c = checks_of(sql);
        assert_eq!(c.len(), 1, "{c:?}");
        (c[0].name.clone(), c[0].expression.clone())
    }

    #[test]
    fn a_named_table_constraint_keeps_its_name() {
        let (name, expr) = one(r#"CREATE TABLE "people" (
                 "age" INTEGER,
                 CONSTRAINT "ck_age" CHECK ("age" >= 0)
               )"#);
        assert_eq!(name, "ck_age");
        assert_eq!(expr, r#""age" >= 0"#);
    }

    /// SQLite doesn't require a name, and most checks in the wild don't have
    /// one. An empty name is the honest answer, not a generated one.
    #[test]
    fn an_unnamed_constraint_comes_back_nameless() {
        let (name, expr) = one(r#"CREATE TABLE t ("age" INTEGER, CHECK ("age" >= 0))"#);
        assert_eq!(name, "");
        assert_eq!(expr, r#""age" >= 0"#);
    }

    /// Written inside the column definition rather than after it — the same
    /// constraint as far as the table is concerned.
    #[test]
    fn a_column_level_check_is_found_too() {
        let (name, expr) = one(r#"CREATE TABLE t ("age" INTEGER CHECK ("age" >= 0), b TEXT)"#);
        assert_eq!(name, "");
        assert_eq!(expr, r#""age" >= 0"#);
    }

    /// The predicate is not scanned for a comma or a closing paren — both are
    /// ordinary content inside one, and a naive split truncates the constraint
    /// into something that means something else.
    #[test]
    fn a_predicate_may_contain_commas_parens_and_a_literal() {
        let (_, expr) = one(r#"CREATE TABLE t (
                 s TEXT,
                 CHECK (substr(s, 1, 2) IN ('a)', 'b,c'))
               )"#);
        assert_eq!(expr, "substr(s, 1, 2) IN ('a)', 'b,c')");
    }

    /// The word has to be a keyword at the right place, not text that happens to
    /// read `CHECK` — the boundary lexer is what makes that distinction.
    #[test]
    fn the_word_check_elsewhere_is_not_a_constraint() {
        assert!(
            checks_of(r#"CREATE TABLE t ("check_sum" INTEGER, note TEXT DEFAULT 'CHECK (x)')"#)
                .is_empty()
        );
        assert!(checks_of("CREATE TABLE t (a INT) -- CHECK (a > 0)").is_empty());
    }

    #[test]
    fn several_checks_all_come_back_in_order() {
        let cs = checks_of(
            r#"CREATE TABLE t (
                 a INT CHECK (a > 0),
                 b INT,
                 CONSTRAINT ck_b CHECK (b < 10),
                 CHECK (a <> b)
               )"#,
        );
        let got: Vec<(&str, &str)> = cs
            .iter()
            .map(|c| (c.name.as_str(), c.expression.as_str()))
            .collect();
        assert_eq!(got, vec![("", "a > 0"), ("ck_b", "b < 10"), ("", "a <> b")]);
    }

    #[test]
    fn a_table_without_checks_reports_none() {
        assert!(checks_of(r#"CREATE TABLE t (a INT PRIMARY KEY, b TEXT NOT NULL)"#).is_empty());
        assert!(checks_of("").is_empty());
    }
}

/// Introspection carrying the checks it reads — the wiring behind
/// [`check_text_tests`], over a real database.
#[cfg(test)]
mod check_schema_tests {
    use super::tests::shared_memory;
    use super::*;

    #[tokio::test]
    async fn a_tables_checks_reach_the_model() {
        let (keeper, db) = shared_memory("checks_modelled");
        keeper
            .execute_batch(
                r#"CREATE TABLE account (
                     id      INTEGER PRIMARY KEY,
                     balance INTEGER NOT NULL CHECK (balance >= 0),
                     kind    TEXT,
                     CONSTRAINT ck_kind CHECK (kind IN ('a', 'b'))
                   );
                   CREATE VIEW rich AS SELECT * FROM account WHERE balance > 100;"#,
            )
            .unwrap();
        let schema = fetch_schema(&db).await.expect("introspect");
        let t = schema
            .tables
            .iter()
            .find(|t| t.name == "account")
            .expect("account");
        let got: Vec<(&str, &str)> = t
            .check_constraints
            .iter()
            .map(|c| (c.name.as_str(), c.expression.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![("", "balance >= 0"), ("ck_kind", "kind IN ('a', 'b')")]
        );

        // A view has no constraints of its own, and its body is full of words
        // that would match if this were a text search.
        let v = schema
            .tables
            .iter()
            .find(|t| t.name == "rich")
            .expect("rich");
        assert!(v.check_constraints.is_empty());
    }
}

/// The rebuild emitted by `core::ddl` is only correct if SQLite accepts it, and
/// the only way to know that is to run it.
#[cfg(test)]
mod rebuild_roundtrip_tests {
    use super::tests::shared_memory;
    use super::*;
    use schemaic_core::ddl::{TableDraft, sqlite_rebuild_sql};
    use tokio_util::sync::CancellationToken;

    async fn table_of(db: &Db, name: &str) -> TableInfo {
        fetch_schema(db)
            .await
            .expect("introspect")
            .tables
            .into_iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| panic!("{name} is gone"))
    }

    /// Retype a column and drop another — neither is something SQLite's
    /// `ALTER TABLE` can do — and check the rows came across.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_rebuild_retypes_a_column_and_keeps_the_rows() {
        let (keeper, db) = shared_memory("rebuild_retype");
        keeper
            .execute_batch(
                "CREATE TABLE t (id INTEGER PRIMARY KEY, n INTEGER, scratch TEXT);
                 CREATE INDEX ix_n ON t (n);
                 INSERT INTO t VALUES (1, 42, 'x'), (2, 7, 'y');",
            )
            .unwrap();

        let before = table_of(&db, "t").await;
        let mut draft = TableDraft::from_table(&before);
        draft.columns[1].info.type_name = "TEXT".into();
        draft.columns.retain(|c| c.info.name != "scratch");

        let stmts = sqlite_rebuild_sql(&before, &draft);
        db.run_ddl(MAIN, &stmts, CancellationToken::new())
            .await
            .expect("the rebuild must be valid SQLite");

        let after = table_of(&db, "t").await;
        let cols: Vec<(&str, &str)> = after
            .columns
            .iter()
            .map(|c| (c.name.as_str(), c.type_name.as_str()))
            .collect();
        assert_eq!(cols, vec![("id", "INTEGER"), ("n", "TEXT")]);
        assert!(
            after.indexes.iter().any(|i| i.name == "ix_n"),
            "the index came back: {:?}",
            after.indexes
        );
        let rows: i64 = keeper
            .query_row("SELECT count(*) FROM t WHERE n IN ('42', '7')", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(rows, 2, "both rows survived the copy");
    }

    /// The case the plain `ALTER TABLE` path can't reach at all: adding a CHECK
    /// constraint, which only exists as part of a table declaration.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_rebuild_adds_a_check_that_then_bites() {
        let (keeper, db) = shared_memory("rebuild_check");
        keeper
            .execute_batch(
                "CREATE TABLE account (id INTEGER PRIMARY KEY, balance INTEGER);
                 INSERT INTO account VALUES (1, 10);",
            )
            .unwrap();

        let before = table_of(&db, "account").await;
        let mut draft = TableDraft::from_table(&before);
        draft
            .check_constraints
            .push(schemaic_core::ddl::CheckDraft::new(CheckInfo {
                name: "ck_positive".into(),
                expression: "balance >= 0".into(),
                enforced: true,
                validated: true,
                inherited: true,
                ..Default::default()
            }));
        let stmts = sqlite_rebuild_sql(&before, &draft);
        db.run_ddl(MAIN, &stmts, CancellationToken::new())
            .await
            .expect("rebuild");

        // The constraint is real, not just recorded.
        let refused = keeper.execute_batch("INSERT INTO account VALUES (2, -5)");
        assert!(refused.is_err(), "the CHECK must be enforced");
        assert_eq!(
            table_of(&db, "account").await.check_constraints.len(),
            1,
            "and it reads back"
        );
    }

    /// A trigger goes down with the table it hangs off. Replaying it is what
    /// stops the rebuild quietly disarming it.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_replayed_trigger_still_fires_afterwards() {
        let (keeper, db) = shared_memory("rebuild_trigger");
        keeper
            .execute_batch(
                "CREATE TABLE t (id INTEGER PRIMARY KEY, n INTEGER);
                 CREATE TABLE log (n INTEGER);
                 CREATE TRIGGER t_ai AFTER INSERT ON t
                   BEGIN INSERT INTO log VALUES (NEW.n); END;",
            )
            .unwrap();
        let before = table_of(&db, "t").await;
        // Introspection is what supplies it — the rebuild reads
        // `TableInfo::dependent_ddl`, so this covers the wiring as well.
        assert_eq!(before.dependent_ddl.len(), 1, "{:?}", before.dependent_ddl);
        let mut draft = TableDraft::from_table(&before);
        draft.columns[1].info.type_name = "TEXT".into();
        let stmts = sqlite_rebuild_sql(&before, &draft);
        db.run_ddl(MAIN, &stmts, CancellationToken::new())
            .await
            .expect("rebuild");

        keeper
            .execute_batch("INSERT INTO t VALUES (1, 5);")
            .unwrap();
        let logged: i64 = keeper
            .query_row("SELECT count(*) FROM log", [], |r| r.get(0))
            .unwrap();
        assert_eq!(logged, 1, "the trigger survived the rebuild");
    }

    /// **The limitation this lifted.** A partial index and an expression index
    /// are both `lossy` — the pragmas report neither the predicate nor the
    /// expression — and a rebuild that re-emitted them from the model would put
    /// back an index over every row, and one over no key at all. So a table
    /// carrying either used to be uneditable outright.
    ///
    /// Replaying each index's own `CREATE` text is what makes the edit possible,
    /// and the assertion is against the **engine's** copy of that text
    /// afterwards, plus `partial` from the pragma the model can't read — an
    /// index that came back narrower or wider fails here, where a comparison of
    /// what Schemaic itself emitted would not.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_lossy_index_comes_back_exactly_as_it_was() {
        let (keeper, db) = shared_memory("rebuild_lossy_index");
        keeper
            .execute_batch(
                "CREATE TABLE t (id INTEGER PRIMARY KEY, n INTEGER, title TEXT);
                 CREATE INDEX ix_lower ON t (lower(title));
                 CREATE INDEX ix_live  ON t (title) WHERE n IS NOT NULL;
                 INSERT INTO t VALUES (1, 5, 'A'), (2, NULL, 'B');",
            )
            .unwrap();

        let before = table_of(&db, "t").await;
        // Introspection is what supplies the text, so this covers the wiring too.
        for name in ["ix_lower", "ix_live"] {
            let ix = before
                .indexes
                .iter()
                .find(|i| i.name == name)
                .unwrap_or_else(|| panic!("{name}"));
            assert!(ix.lossy, "{name} must read lossy: {ix:?}");
            assert!(ix.create_sql.is_some(), "{name} keeps its own text");
        }

        let mut draft = TableDraft::from_table(&before);
        draft.columns[1].info.type_name = "TEXT".into();
        let cs =
            schemaic_core::ddl::diff(&before, &draft, schemaic_core::intel::SqlDialect::Sqlite);
        assert!(
            cs.unsupported().is_empty(),
            "the plan must no longer be refused: {:?}",
            cs.unsupported()
        );
        db.run_ddl(MAIN, &cs.emit(), CancellationToken::new())
            .await
            .expect("rebuild");

        let after = table_of(&db, "t").await;
        for name in ["ix_lower", "ix_live"] {
            let was = before.indexes.iter().find(|i| i.name == name).unwrap();
            let now = after
                .indexes
                .iter()
                .find(|i| i.name == name)
                .unwrap_or_else(|| panic!("{name} did not come back"));
            assert_eq!(now.create_sql, was.create_sql, "{name} came back different");
        }
        // Still partial, as the engine sees it — the failure this prevents is an
        // index that reads back under the same name covering twice the rows.
        let partial: i64 = keeper
            .query_row(
                "SELECT partial FROM pragma_index_list('t') WHERE name = 'ix_live'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(partial, 1, "the predicate survived");
    }

    /// The other half of the same rule: the text is a snapshot, so an edit to a
    /// column it may name puts the plan back on the refusal. Nothing is applied
    /// and the message says which column.
    #[tokio::test(flavor = "multi_thread")]
    async fn renaming_a_column_under_a_lossy_index_is_still_refused() {
        let (keeper, db) = shared_memory("rebuild_lossy_rename");
        keeper
            .execute_batch(
                "CREATE TABLE t (id INTEGER PRIMARY KEY, n INTEGER, title TEXT);
                 CREATE INDEX ix_live ON t (title) WHERE n IS NOT NULL;",
            )
            .unwrap();
        let before = table_of(&db, "t").await;
        let mut draft = TableDraft::from_table(&before);
        draft.columns[2].info.type_name = "BLOB".into();
        // `n` appears only in the predicate, which no pragma reports — so the
        // model cannot see that this rename breaks the index at all.
        draft.rename_column(1, "count");
        let withheld =
            schemaic_core::ddl::diff(&before, &draft, schemaic_core::intel::SqlDialect::Sqlite)
                .unsupported();
        assert_eq!(withheld.len(), 1, "{withheld:?}");
        assert!(withheld[0].contains("ix_live"), "{withheld:?}");
        assert!(withheld[0].contains('n'), "{withheld:?}");
    }
}

/// A rebuild happens under objects that reference the table. These are the ones
/// that bite.
#[cfg(test)]
mod rebuild_bystander_tests {
    use super::tests::shared_memory;
    use super::*;
    use schemaic_core::ddl::{TableDraft, sqlite_rebuild_sql};
    use tokio_util::sync::CancellationToken;

    async fn table_of(db: &Db, name: &str) -> TableInfo {
        fetch_schema(db)
            .await
            .expect("introspect")
            .tables
            .into_iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| panic!("{name} is gone"))
    }

    /// **A view over the table is the trap.** From 3.25 SQLite re-parses every
    /// view and trigger during `ALTER TABLE … RENAME`, and at that moment the
    /// original table has already been dropped — so a view selecting from it
    /// refers to nothing and the rename fails, taking the whole rebuild with it.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_view_over_the_table_does_not_break_the_rename() {
        let (keeper, db) = shared_memory("rebuild_view");
        keeper
            .execute_batch(
                "CREATE TABLE t (id INTEGER PRIMARY KEY, n INTEGER);
                 CREATE VIEW big AS SELECT id FROM t WHERE n > 10;
                 INSERT INTO t VALUES (1, 50);",
            )
            .unwrap();
        let before = table_of(&db, "t").await;
        let mut draft = TableDraft::from_table(&before);
        draft.columns[1].info.type_name = "TEXT".into();
        db.run_ddl(
            MAIN,
            &sqlite_rebuild_sql(&before, &draft),
            CancellationToken::new(),
        )
        .await
        .expect("the rename must survive a view that references the table");

        let seen: i64 = keeper
            .query_row("SELECT count(*) FROM big", [], |r| r.get(0))
            .unwrap();
        assert_eq!(seen, 1, "and the view still resolves afterwards");
    }
}

/// Foreign keys around a rebuild — the part of the twelve steps that is about
/// the *other* tables.
#[cfg(test)]
mod rebuild_fk_tests {
    use super::tests::shared_memory;
    use super::*;
    use schemaic_core::ddl::{TableDraft, sqlite_rebuild_sql};
    use tokio_util::sync::CancellationToken;

    async fn table_of(db: &Db, name: &str) -> TableInfo {
        fetch_schema(db)
            .await
            .expect("introspect")
            .tables
            .into_iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| panic!("{name} is gone"))
    }

    /// **The one that eats data.** With foreign keys enforced, the rebuild's
    /// `DROP TABLE` on a parent is an implicit `DELETE FROM parent`, which fires
    /// `ON DELETE CASCADE` and takes every child row with it. The table comes
    /// back looking exactly as asked for, and another table has been emptied.
    #[tokio::test(flavor = "multi_thread")]
    async fn rebuilding_a_parent_does_not_cascade_into_its_children() {
        let (keeper, db) = shared_memory("rebuild_cascade");
        keeper
            .execute_batch(
                "CREATE TABLE artist (id INTEGER PRIMARY KEY, name TEXT);
                 CREATE TABLE album (
                     id        INTEGER PRIMARY KEY,
                     artist_id INTEGER REFERENCES artist (id) ON DELETE CASCADE
                 );
                 INSERT INTO artist VALUES (1, 'a');
                 INSERT INTO album  VALUES (1, 1), (2, 1);",
            )
            .unwrap();

        let before = table_of(&db, "artist").await;
        let mut draft = TableDraft::from_table(&before);
        draft.columns[1].info.type_name = "BLOB".into();
        db.run_ddl(
            MAIN,
            &sqlite_rebuild_sql(&before, &draft),
            CancellationToken::new(),
        )
        .await
        .expect("rebuild");

        let albums: i64 = keeper
            .query_row("SELECT count(*) FROM album", [], |r| r.get(0))
            .unwrap();
        assert_eq!(albums, 2, "the children must not have been cascaded away");
        let artists: i64 = keeper
            .query_row("SELECT count(*) FROM artist", [], |r| r.get(0))
            .unwrap();
        assert_eq!(artists, 1);
    }

    /// Enforcement is suspended for the plan, not abandoned: what the plan
    /// leaves behind is checked before it is allowed to commit.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_plan_that_leaves_a_dangling_reference_is_refused() {
        let (keeper, db) = shared_memory("rebuild_fk_check");
        keeper
            .execute_batch(
                "CREATE TABLE artist (id INTEGER PRIMARY KEY, name TEXT);
                 CREATE TABLE album (
                     id        INTEGER PRIMARY KEY,
                     artist_id INTEGER REFERENCES artist (id)
                 );
                 INSERT INTO artist VALUES (1, 'a');
                 INSERT INTO album  VALUES (1, 1);",
            )
            .unwrap();

        // Dropping the parent outright leaves album.artist_id pointing at
        // nothing — with enforcement suspended, only the final check catches it.
        let err = db
            .run_ddl(
                MAIN,
                &[r#"DROP TABLE "artist";"#.to_string()],
                CancellationToken::new(),
            )
            .await
            .expect_err("a dangling reference must not commit");
        assert!(
            format!("{err}").to_lowercase().contains("foreign key"),
            "{err}"
        );
        let artists: i64 = keeper
            .query_row("SELECT count(*) FROM artist", [], |r| r.get(0))
            .unwrap();
        assert_eq!(artists, 1, "and the drop rolled back");
    }
}

/// The designer's own path, end to end: `diff` → `emit` → `run_ddl`. The
/// rebuild tests above call the builder directly; this one goes the way the
/// application does.
#[cfg(test)]
mod designer_path_tests {
    use super::tests::shared_memory;
    use super::*;
    use schemaic_core::ddl::{TableDraft, diff};
    use schemaic_core::intel::SqlDialect;
    use tokio_util::sync::CancellationToken;

    async fn table_of(db: &Db, name: &str) -> TableInfo {
        fetch_schema(db)
            .await
            .expect("introspect")
            .tables
            .into_iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| panic!("{name} is gone"))
    }

    /// Retype a column, add one, drop one, rename the table and add a key — a
    /// designer session's worth of edits, none of which SQLite can do in place.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_designer_session_applies_as_one_plan() {
        let (keeper, db) = shared_memory("designer_path");
        keeper
            .execute_batch(
                "CREATE TABLE person (id INTEGER, name TEXT, scratch TEXT);
                 INSERT INTO person VALUES (1, 'ada', 'x');",
            )
            .unwrap();

        let before = table_of(&db, "person").await;
        let mut draft = TableDraft::from_table(&before);
        draft.columns[0].info.type_name = "TEXT".into();
        draft.columns.retain(|c| c.info.name != "scratch");
        draft.primary_key = vec!["id".into()];
        draft.name = "people".into();

        let cs = diff(&before, &draft, SqlDialect::Sqlite);
        assert!(cs.unsupported().is_empty(), "{:?}", cs.unsupported());
        db.run_ddl(MAIN, &cs.emit(), CancellationToken::new())
            .await
            .expect("the designer's plan must be valid SQLite");

        let after = table_of(&db, "people").await;
        let cols: Vec<&str> = after.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(cols, vec!["id", "name"]);
        assert!(
            after.indexes.iter().any(|i| i.is_primary()),
            "the key was added: {:?}",
            after.indexes
        );
        let name: String = keeper
            .query_row("SELECT name FROM people WHERE id = '1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(name, "ada", "the row came across and the id retyped");
    }
}
