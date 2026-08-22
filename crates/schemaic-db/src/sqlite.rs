//! SQLite backend (third engine), built on [`rusqlite`].
//!
//! Dispatched to from [`crate::Db`]'s public methods when the connection's engine
//! is [`crate::Engine::Sqlite`]. Four things make it unlike the other two, and
//! each shapes the code below rather than being a detail of it.
//!
//! **There is no server.** A connection is a *file* ([`crate::Db::file`]), so
//! there is no host, no port, no user, no password and nothing for an SSH tunnel
//! to reach. `fetch_databases` therefore has nothing to enumerate: it reports the
//! one database SQLite calls `main`. That is not a placeholder — `main` is the
//! name SQLite itself uses for the file you opened, and the name any qualified
//! reference to it must use. It still *opens* the file to say so, because a list
//! the app can produce for a connection that is down is a list the schema tree
//! will draw for one.
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

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use rusqlite::types::ValueRef;
use rusqlite::{Connection as SqliteConn, OpenFlags};
#[cfg(test)]
use schemaic_core::model::CellTag;
use schemaic_core::model::{
    Column, ColumnFlags, ColumnOrigin, GridWrite, RefetchRow, RefetchTemplate, ResultBuilder,
    ResultSet, Rollback, Value, WriteStep, one_row_verdict,
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
///
/// **`SQLITE_OPEN_URI` is absent in a release build**, and present only under
/// `cfg(test)`, where the shared in-memory databases the suites use are reached
/// by a `file:name?mode=memory&cache=shared` URI. Setting it in production put a
/// second, invisible meaning on the one field the user types: a saved path
/// beginning `file:` would be parsed as a URI, so `file:reports?x` opens
/// `reports` and quietly discards the rest — and `?mode=memory` on a real path
/// opens a scratch database that accepts every write and keeps none.
fn open(db: &Db) -> Result<SqliteConn, DbError> {
    if db.file.trim().is_empty() {
        return Err(DbError::Connect("no database file is set".to_string()));
    }
    #[allow(unused_mut)]
    let mut flags = OpenFlags::SQLITE_OPEN_READ_WRITE;
    #[cfg(test)]
    {
        flags |= OpenFlags::SQLITE_OPEN_URI;
    }
    SqliteConn::open_with_flags(&db.file, flags)
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
            binary: declares_bytes(&ci.type_name),
            implicit_key: false,
        });
    }
}

/// Is a column declared `declared` one the grid must never let anyone type
/// over — a column meant to hold raw bytes?
///
/// **Not an equality test against `"BLOB"`.** SQLite's declared type is
/// arbitrary text, so the same intent is written `BLOB`, `MEDIUMBLOB`,
/// `VARBINARY(16)`, or — idiomatically — as nothing at all. Every one of those
/// stores raw bytes, [`value_of`] renders them all as `<N bytes>`, and a column
/// this answers `false` for is *editable*: committing that placeholder writes
/// the literal text over the data, and Duplicate row does it with no cell edit
/// at all.
///
/// So it asks two questions, in the order that makes each one cheap to justify:
/// SQLite's own [`schemaic_core::schema::sqlite_affinity`] rule, which covers
/// every `…BLOB…` spelling
/// and the untyped column; then the `BINARY` family, which SQLite gives NUMERIC
/// affinity but which nobody writes meaning anything but bytes.
///
/// What it still cannot see is a blob stored in a column declared `TEXT` —
/// SQLite permits that, and only a `Value` variant for bytes would catch it.
/// Widening here is safe in the one direction that matters: a column wrongly
/// called binary is read-only, never wrongly writable.
fn declares_bytes(declared: &str) -> bool {
    use schemaic_core::schema::{SqliteAffinity, sqlite_affinity};
    sqlite_affinity(declared) == SqliteAffinity::Blob
        || declared.to_ascii_uppercase().contains("BINARY")
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

/// `(without_rowid, strict)` for `table`, from the same `pragma_table_list` row
/// [`has_rowid`] reads.
///
/// Both default to `false` when the row can't be read — the shape of an ordinary
/// table, which is the conservative answer: emitting a clause a table doesn't
/// have would change it, while omitting one it does have is the failure the
/// caller's own emitter is now guarded against by the round-trip gate.
///
/// `strict` arrived in SQLite 3.37; on an older library the column is absent and
/// the query fails, which lands on the same `false`.
fn table_list_flags(conn: &SqliteConn, table: &str) -> (bool, bool) {
    conn.query_row(
        "SELECT wr, strict FROM pragma_table_list(?1) WHERE schema = ?2 AND type = 'table'",
        rusqlite::params![table, MAIN],
        |r| Ok((r.get::<_, i64>(0)? != 0, r.get::<_, i64>(1)? != 0)),
    )
    .unwrap_or((false, false))
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
///
/// **Matched case-insensitively**, because SQLite resolves an object name that
/// way and every other lookup on this path already does. A case-sensitive `=`
/// here made `SELECT * FROM ARTIST` fall to the caller's "don't attribute"
/// answer and open read-only, while `SELECT * FROM artist` was editable — the
/// same table, decided by how the user typed it.
fn is_view(conn: &SqliteConn, name: &str) -> Result<bool, DbError> {
    let kind: Option<String> = conn
        .query_row(
            "SELECT type FROM sqlite_master \
             WHERE name = ?1 COLLATE NOCASE AND type IN ('table','view')",
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
/// Refuse work whose token was already cancelled when the call was made.
///
/// Every cancellable path below ends in a `tokio::select!` between the blocking
/// task and `cancel.cancelled()`, and **`select!` polls its ready branches in
/// random order**. On a small table the blocking task finishes almost at once,
/// so with an already-cancelled token *both* branches are ready and the answer
/// came back roughly half the time — which is exactly how
/// `a_cancelled_count_stops_instead_of_answering` failed on CI while passing
/// locally for weeks.
///
/// Checking first makes that case deterministic rather than a coin flip, and it
/// is the honest answer besides: nothing has run yet, so there is nothing to
/// report but the cancellation. It says nothing about a token cancelled *during*
/// a scan, which is a real race and is still resolved by the `select!`.
fn refuse_if_cancelled(cancel: &CancellationToken) -> Result<(), DbError> {
    if cancel.is_cancelled() {
        return Err(DbError::Cancelled);
    }
    Ok(())
}

pub(crate) async fn fetch_query(
    db: &Db,
    sql: &str,
    row_cap: usize,
    cancel: CancellationToken,
) -> Result<ResultSet, DbError> {
    refuse_if_cancelled(&cancel)?;
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
        // **`execute` with an empty parameter list is load-bearing, not
        // incidental.** SQLite is the one engine that *accepts* `:name` — it is
        // a documented bind-parameter form there — so a `skeleton` draft run by
        // reflex prepares cleanly, and what refuses it is this call's
        // parameter-count check (`Wrong number of parameters passed to query`).
        // A raw bind-nothing execution would bind them all as NULL and write the
        // row `core::skeleton`'s doc promises cannot happen by accident.
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
    refuse_if_cancelled(&cancel)?;
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
            // **The verdict says what the guard saw; the clause says what the
            // rollback achieved.** `one_row_verdict` is reached before the
            // rollback runs and so cannot know, which is why the caller appends
            // the note — and why all three executors share one wording. Always
            // `Complete` here: SQLite has no non-transactional table type.
            one_row_verdict(step, affected)
                .map_err(|m| DbError::Query(format!("{m}{}", Rollback::Complete.note())))?;
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
    refuse_if_cancelled(&cancel)?;
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
                    // The clause comes from the shared `Rollback` rather than
                    // being written here: it used to be inline in each executor,
                    // with divergent wordings and no test, which is what
                    // `Rollback::note` exists to stop happening again. Always
                    // `Complete` — every SQLite table is transactional.
                    return Err(DbError::Query(format!(
                        "a batch of {} rows affected {affected}{}",
                        batch.len(),
                        Rollback::Complete.note()
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
    // **The shadow table is ours, not the user's.** A rebuild's copy step fails
    // with `NOT NULL constraint failed: t_schemaic_rebuild.c` — naming an object
    // that exists between two statements of a script they may not have read. The
    // engine's message is otherwise the most useful thing there is, so only the
    // name is changed (`ddl::unshadow`).
    let fail = |at: usize, message: String| crate::DdlError {
        message: schemaic_core::ddl::unshadow(&message),
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
        //
        // A rebuild plan carries the same pragma as its first and last statement
        // (`ddl::FK_OFF`/`FK_ON`), because that list is also what the preview's
        // Copy and "Open in editor" hand to a query tab. Both are silently
        // ignored here — SQLite ignores the pragma inside a transaction — which
        // is exactly why this one has to stay.
        let _ = conn.execute_batch("PRAGMA foreign_keys = OFF");

        // **What was already broken is not the plan's fault.** A `.db` written by
        // the sqlite3 CLI — where foreign keys are off by default — very commonly
        // carries a child row whose parent is gone, and
        // `pragma_foreign_key_check` scans the *whole database*: without this,
        // adding a column to an unrelated third table was refused with "the plan
        // leaves a foreign key pointing at nothing", and every DDL operation on
        // that file failed the same way for ever. Taken before `BEGIN` so it
        // describes the state the plan inherited, and compared below so only a
        // violation the plan *added* refuses it.
        let inherited = fk_violations(&conn).unwrap_or_default();

        // **Step 9 of SQLite's twelve-step procedure, which the plan has no way
        // to perform.** A view naming a column the rebuild drops or renames is
        // broken by it — the engine refuses the *native* `DROP COLUMN` for
        // exactly this, and `legacy_alter_table = ON` (right for the rename, see
        // `ddl::sqlite_rebuild_sql`) is what stops it noticing here. So the plan
        // used to report success and the user found out the next time they opened
        // the view.
        //
        // The engine is the authority on what a view resolves to, so this asks
        // it: preparing `SELECT * FROM <view>` re-parses the definition against
        // the schema the plan left behind. Read before `BEGIN` as well, and
        // compared, for the same reason the foreign-key rows are — a `.db` can
        // arrive with a view over a table that is already gone, and refusing for
        // that would take DDL away from every other table in the file.
        let broken_before = broken_views(&conn);

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
            let now = fk_violations(&conn).map_err(|e| fail(last, format!("{e}")))?;
            // Only what the plan *added*. A row that was already dangling before
            // `BEGIN` came with the file, and refusing for it would take the whole
            // DDL feature away from a database whose bad row has nothing to do
            // with the edit.
            if let Some(row) = now.added_since(&inherited) {
                return Err(fail(
                    last,
                    format!("the plan leaves a foreign key pointing at nothing: {row}"),
                ));
            }
            if let Some((view, why)) = first_newly_broken_view(&conn, &broken_before) {
                return Err(fail(
                    last,
                    format!(
                        "the plan would break the view {view}, which reads a column it \
                         renames or drops ({why}). Nothing was changed. Edit or drop the \
                         view first."
                    ),
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

/// Every view in the database that does not currently resolve, by name.
///
/// Asked by *preparing* `SELECT * FROM <view>`: SQLite re-parses the stored
/// definition against the live schema at prepare time, which is the only complete
/// answer to "does this view still work" — the definition text alone would need
/// the whole resolver to interpret, and `PRAGMA integrity_check` does not look at
/// views at all. Nothing is executed, so the cost is a parse per view and no rows
/// are read.
///
/// Errors are the answer rather than a failure: a view that cannot be prepared is
/// precisely what this is looking for. A catalogue read that fails altogether
/// gives an empty set, which makes the comparison in [`run_ddl`] permissive rather
/// than refusing a plan on the strength of a question that couldn't be asked.
fn broken_views(conn: &SqliteConn) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Ok(mut stmt) = conn.prepare("SELECT name FROM sqlite_master WHERE type = 'view'") else {
        return out;
    };
    let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0)) else {
        return out;
    };
    for name in rows.flatten() {
        if let Err(e) = conn.prepare(&format!("SELECT * FROM {}", ident_sqlite(&name))) {
            out.insert(name, e.to_string());
        }
    }
    out
}

/// The first view the plan broke — one that resolves no longer and did resolve
/// before — with the engine's own reason.
///
/// `None` when every view that fails now was already failing, which is the
/// inherited case: see [`run_ddl`].
fn first_newly_broken_view(
    conn: &SqliteConn,
    before: &HashMap<String, String>,
) -> Option<(String, String)> {
    let mut now: Vec<(String, String)> = broken_views(conn)
        .into_iter()
        .filter(|(name, _)| !before.contains_key(name))
        .collect();
    // Sorted so a plan that breaks several names the same one every time — a
    // `HashMap`'s order would make the message drift between identical runs.
    now.sort();
    now.into_iter().next()
}

/// Every dangling foreign-key row in the database, as identities that can be
/// compared between two readings.
///
/// `PRAGMA foreign_key_check` returns one row per violation — the child table,
/// the child's rowid, the parent it names, and which of that table's foreign keys
/// it was. Those four *are* the identity, which is what lets `run_ddl` ask the
/// only question worth asking: is this one the plan's doing, or was it here
/// already?
#[derive(Debug, Default)]
struct FkViolations {
    /// `(table, rowid, parent, fkid)` per violation.
    rows: HashSet<(String, Option<i64>, Option<String>, i64)>,
    /// The reading was truncated at [`FK_SCAN_CAP`] — see [`Self::added_since`].
    truncated: bool,
}

/// How many violations are read back before the scan gives up.
///
/// A database with more than this many dangling rows is already broken in a way
/// no DDL plan is going to make meaningfully worse, and reading all of them into
/// memory to prove it would be the expensive half of every apply.
const FK_SCAN_CAP: usize = 10_000;

impl FkViolations {
    /// One violation present here and not in `before`, described for the user —
    /// or `None` when this reading adds nothing.
    ///
    /// When either reading was truncated the set comparison can't be trusted, so
    /// it falls back to the count: more violations than before is still an
    /// answer, and equal-or-fewer on an already-broken database is not something
    /// to refuse a plan over.
    fn added_since(&self, before: &FkViolations) -> Option<String> {
        if self.truncated || before.truncated {
            return (self.rows.len() > before.rows.len())
                .then(|| self.rows.iter().next().map(describe_fk_row))
                .flatten();
        }
        self.rows
            .difference(&before.rows)
            .next()
            .map(describe_fk_row)
    }
}

/// One violation row, in the words the refusal uses — naming the child table is
/// what makes it actionable rather than a bare "constraint failed".
fn describe_fk_row(row: &(String, Option<i64>, Option<String>, i64)) -> String {
    match &row.2 {
        Some(p) => format!("a row in {} refers to {p}", row.0),
        None => format!("a row in {}", row.0),
    }
}

/// Read the whole database's dangling foreign-key rows.
fn fk_violations(conn: &SqliteConn) -> Result<FkViolations, DbError> {
    let mut stmt = conn
        .prepare(
            "SELECT \"table\", \"rowid\", \"parent\", \"fkid\" \
             FROM pragma_foreign_key_check() LIMIT ?1",
        )
        .map_err(query_err)?;
    let rows = stmt
        .query_map([FK_SCAN_CAP as i64 + 1], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<i64>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })
        .map_err(query_err)?;
    let mut out = FkViolations::default();
    for row in rows {
        out.rows.insert(row.map_err(query_err)?);
        if out.rows.len() > FK_SCAN_CAP {
            out.truncated = true;
            break;
        }
    }
    Ok(out)
}

/// The databases this connection offers: the one file, under the name SQLite
/// gives it.
///
/// There is nothing to enumerate — `main` is the name SQLite itself gives the
/// file you opened — so the work here is the *opening*: this is the one engine
/// whose database list could be answered without touching the connection at all,
/// and answering it that way is what made a dead connection look alive.
///
/// The schema sidebar lists a connection's databases on selection, long before
/// the user has asked to read anything, and it treats a failed listing as "this
/// connection has nothing to show" — the tree is emptied. MySQL and PostgreSQL
/// get that for free, since their listing is a query and a query needs a server.
/// A SQLite connection pointed at a path that is missing, locked, or on a
/// disconnected share used to answer `main` anyway, so the tree grew a node for
/// a database that isn't there, and the connect error surfaced one level down as
/// red text *inside* the tree — beneath a header already saying "Disconnected",
/// which is the affordance that belongs to this.
///
/// [`ping`] is the check, so "can this connection be listed" and "is this
/// connection up" cannot drift apart: both are `PRAGMA schema_version` against a
/// freshly opened file. It costs one local `open` per selection.
pub(crate) async fn fetch_databases(db: &Db) -> Result<Vec<String>, DbError> {
    ping(db).await?;
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
            // The two clauses that change what the table *is*. Both are in the
            // `pragma_table_list` row `has_rowid` already reads, and both are
            // modelled because the rebuild writes the table back from the model:
            // one the model doesn't carry is one the edit silently drops.
            let (without_rowid, strict) = if is_view {
                (false, false)
            } else {
                table_list_flags(conn, &name)
            };
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
            // What a re-create has to put back. A **table**'s rebuild drops the
            // table and its triggers go with it; a **view**'s edit is a
            // `DROP VIEW` + `CREATE VIEW` on this engine, because SQLite has no
            // `CREATE OR REPLACE VIEW`, and SQLite drops a view's `INSTEAD OF`
            // triggers with the view too. That is the only way a SQLite view is
            // written to at all, and the text is unrecoverable once the drop has
            // run — so it is collected here for both, and `ChangeSet` replays it
            // in both places.
            let dependent_ddl = trigger_statements(&trigger_sql);
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
                without_rowid,
                strict,
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

/// The SQLite half of [`Db::count_rows`].
///
/// The whole of SQLite's contribution to the properties surface. It publishes no
/// per-table statistics to fetch — see
/// [`schemaic_core::stats::supports_table_stats`] — so the exact count is not a
/// fallback here, it is the only figure there is.
pub(crate) async fn count_rows(
    db: &Db,
    sql: &str,
    cancel: CancellationToken,
) -> Result<u64, DbError> {
    refuse_if_cancelled(&cancel)?;
    let sql = sql.to_string();
    let db = db.clone();
    let (tx, rx) = tokio::sync::oneshot::channel();
    // Not `with_conn`: a full scan has to be interruptible, and the handle has to
    // reach the async side before the scan starts — the same shape `fetch_query`
    // uses, and the reason `count_rows` takes a token at all.
    let work = tokio::task::spawn_blocking(move || {
        let conn = open(&db)?;
        let _ = tx.send(conn.get_interrupt_handle());
        conn.query_row(&sql, [], |r| r.get::<_, i64>(0))
            .map(|n| n.max(0) as u64)
            .map_err(query_err)
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
    // `coll` is what makes a column's `COLLATE NOCASE` survive a rebuild — the
    // rebuild writes the table from this model, so a collation the model doesn't
    // carry is one the edit silently drops, and `'A' = 'a'` stops being true.
    // `pragma_table_xinfo` has no `coll`; `pragma_table_info` has none either.
    // It comes from the table's own `CREATE` text (see `collations_of`).
    let collations = collations_of(conn, table)?;
    let has_rowid = has_rowid(conn, table);
    let declared_autoincrement = table_declares_autoincrement(conn, table);
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

    let mut rows_out = Vec::new();
    for row in rows {
        rows_out.push(row.map_err(query_err)?);
    }
    // How many columns the primary key has. An `INTEGER PRIMARY KEY` is the
    // rowid only when it is the *whole* key: `PRIMARY KEY (a, b)` over two
    // INTEGER columns assigns neither, and neither does a `WITHOUT ROWID`
    // table's key. Reading each column on its own said "server-assigned" for
    // both, and the grid then dropped those columns from its `INSERT` — so
    // duplicating a junction row wrote `(NULL, NULL)`.
    let key_width = rows_out.iter().filter(|r| r.4).count();

    let mut out = Vec::new();
    for (name, type_name, notnull, default, pk, hidden) in rows_out {
        if hidden == 1 {
            continue; // a virtual table's hidden column — not part of the declaration
        }
        // 2 = VIRTUAL, 3 = STORED. The two values *are* the distinction, and
        // collapsing them turned a materialised column back into a computed one.
        let generated = (hidden == 2 || hidden == 3)
            .then(|| generated_expr(conn, table, &name))
            .flatten();
        // `AUTOINCREMENT` is a separate keyword, but an `INTEGER PRIMARY KEY` is
        // the rowid and is server-assigned whether or not it is present — which is
        // what this flag is asked about, so both count. It has to be the sole key
        // column of a rowid table, though: that is what makes it the rowid.
        let rowid_alias =
            pk && key_width == 1 && has_rowid && type_name.eq_ignore_ascii_case("INTEGER");
        out.push(ColumnInfo {
            name: name.clone(),
            type_name,
            // A PK column is NOT NULL in every SQL engine — except SQLite, where
            // an `INTEGER PRIMARY KEY` is the rowid and everything else declared
            // `PRIMARY KEY` may hold NULLs, a documented quirk kept for
            // compatibility. So `notnull` is reported as the pragma gives it,
            // never inferred from `pk`.
            nullable: !notnull,
            primary_key: pk,
            default,
            auto_increment: rowid_alias,
            sqlite_autoincrement: rowid_alias && declared_autoincrement,
            identity_always: false,
            generated,
            generated_stored: hidden == 3,
            on_update: None,
            comment: None,
            collation: collations.get(&name.to_ascii_lowercase()).cloned(),
        });
    }
    Ok(out)
}

/// [`declares_autoincrement`] against the table's stored declaration.
fn table_declares_autoincrement(conn: &SqliteConn, table: &str) -> bool {
    conn.query_row(
        "SELECT COALESCE(sql, '') FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [table],
        |r| r.get::<_, String>(0),
    )
    .map(|sql| declares_autoincrement(&sql))
    .unwrap_or(false)
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
        // here is stringing statements together, so it goes back on once —
        // through `sql::terminated`, because an index's stored text keeps the
        // author's own trailing `-- comment` and a `;` trimmed onto the end of
        // that is commented out, taking the *next* statement with it.
        .map(|v| {
            v.into_iter()
                .map(|(n, s)| {
                    (
                        n,
                        schemaic_core::sql::terminated(
                            &s,
                            schemaic_core::intel::SqlDialect::Sqlite,
                        ),
                    )
                })
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
        // or the replay runs into whatever follows. Through `sql::terminated`,
        // the same guard `index_sql` needs: a trigger's stored text is truncated
        // at its last token today, but that is the engine's choice and not a
        // property of this code, and getting it wrong here fails a *rebuild*.
        .map(|s| schemaic_core::sql::terminated(s, schemaic_core::intel::SqlDialect::Sqlite))
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

/// Each column's explicit `COLLATE`, keyed by lower-cased column name.
///
/// **No pragma reports it.** `table_xinfo` has no `coll` column and
/// `index_xinfo`'s is the index's, not the column's — so, exactly as with
/// [`checks_of`] and [`generated_expr_of`], the table's own `CREATE` text is the
/// only source. The cost of not reading it is that the rebuild writes the table
/// back without the clause: a `COLLATE NOCASE` column silently becomes
/// case-sensitive, and every comparison the user relies on changes meaning.
///
/// Read at the item's own paren depth only, so a `COLLATE` inside a `CHECK`
/// predicate or an index expression can't be mistaken for a column's.
fn collations_of(conn: &SqliteConn, table: &str) -> Result<HashMap<String, String>, DbError> {
    let sql: String = conn
        .query_row(
            "SELECT COALESCE(sql, '') FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |r| r.get(0),
        )
        .unwrap_or_default();
    Ok(collations_of_text(&sql))
}

/// The pure reader behind [`collations_of`].
fn collations_of_text(create_sql: &str) -> HashMap<String, String> {
    use schemaic_core::intel::SqlDialect;
    use schemaic_core::sql::{is_word_byte, is_word_start, skip_noncode};

    let b = create_sql.as_bytes();
    let mut out = HashMap::new();
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
    // The first identifier of the current item, which for a column declaration
    // is its name. Reset at every top-level comma, so a table constraint's
    // keyword can't be filed as a column.
    let mut column: Option<String> = None;
    let mut first = true;
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
                column = None;
                first = true;
                i += 1;
                continue;
            }
            _ => {}
        }
        if depth == 1 && first {
            let (name, next) = ident_at(create_sql, i);
            if let Some(n) = name {
                column = Some(n);
                first = false;
                i = next;
                continue;
            }
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
        if depth == 1
            && create_sql[start..end].eq_ignore_ascii_case("COLLATE")
            && let Some(col) = column.as_deref()
        {
            let (name, next) = ident_at(create_sql, end);
            if let Some(n) = name {
                out.insert(col.to_ascii_lowercase(), n);
                i = next;
                continue;
            }
        }
        i = end;
    }
    out
}

/// Does this table's declaration carry the `AUTOINCREMENT` keyword?
///
/// SQLite publishes no pragma for it, and `sqlite_sequence` is not the answer
/// either: the engine writes a row there only once a row has been inserted, so
/// an empty `AUTOINCREMENT` table would read as an ordinary one and lose the
/// keyword on the first rebuild. The declaration is the authority, read through
/// the shared boundary lexer as a whole word so a column named
/// `autoincrement_note` — or the word inside a string or a comment — can't match.
fn declares_autoincrement(create_sql: &str) -> bool {
    use schemaic_core::intel::SqlDialect;
    use schemaic_core::sql::{is_word_byte, is_word_start, skip_noncode};

    let b = create_sql.as_bytes();
    let mut i = 0usize;
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
        if create_sql[start..end].eq_ignore_ascii_case("AUTOINCREMENT") {
            return true;
        }
        i = end;
    }
    false
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
        // **A name belongs to the constraint it introduces, and to no other.**
        // `a TEXT CONSTRAINT nn_a NOT NULL CHECK (a <> '')` names the *NOT NULL*;
        // clearing `pending` only at the comma let the bare `CHECK` beside it
        // inherit `nn_a`, so a rebuild wrote `CONSTRAINT "nn_a" CHECK (…)` and
        // gave the table a name it never gave itself — silently and permanently,
        // since SQLite accepts it. Any constraint keyword that isn't `CHECK`
        // consumes the pending name.
        if depth == 1
            && matches!(
                word.to_ascii_uppercase().as_str(),
                "NOT"
                    | "NULL"
                    | "UNIQUE"
                    | "PRIMARY"
                    | "REFERENCES"
                    | "DEFAULT"
                    | "COLLATE"
                    | "GENERATED"
            )
        {
            pending = None;
            i = end;
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
/// it.
///
/// A thin alias for [`schemaic_core::sql::ident_at`], which owns the rule. It
/// used to be a second copy — its own quote table, its own doubling rule, its own
/// `]`-has-no-escape exception — and the copy had already drifted: its bare arm
/// looped on `is_word_byte` alone, so `CONSTRAINT 3way` read back a constraint
/// named `3way`, a name no engine would accept.
fn ident_at(sql: &str, at: usize) -> (Option<String>, usize) {
    schemaic_core::sql::ident_at(sql, at, schemaic_core::intel::SqlDialect::Sqlite)
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
    let column_collations = collations_of(conn, table)?;
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
        let (columns, dropped_expression) = index_columns(conn, &name, &column_collations)?;
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
/// `column_collations` is the table's own per-column `COLLATE` (see
/// [`collations_of`]), which is what makes the index's `coll` readable: the
/// pragma reports the *effective* collation of each key, so it says `NOCASE` both
/// for `CREATE INDEX … (email COLLATE NOCASE)` and for a plain index over a
/// column already declared `COLLATE NOCASE`. Only the first is the index's own,
/// and only the first has to be restated.
fn index_columns(
    conn: &SqliteConn,
    index: &str,
    column_collations: &HashMap<String, String>,
) -> Result<(Vec<IndexColumn>, bool), DbError> {
    let mut stmt = conn
        .prepare("SELECT name, desc, key, coll FROM pragma_index_xinfo(?1) ORDER BY seqno")
        .map_err(query_err)?;
    let rows = stmt
        .query_map([index], |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, i64>(1)? != 0,
                r.get::<_, i64>(2)? != 0,
                r.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(query_err)?;

    let mut out = Vec::new();
    let mut dropped_expression = false;
    for row in rows {
        let (name, descending, key, coll) = row.map_err(query_err)?;
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
        // What the column would collate as on its own — `BINARY` unless the
        // table declares otherwise. Anything else is the index's own clause, and
        // dropping it is what turns a case-insensitive UNIQUE index into a
        // case-sensitive one without a word.
        let column_default = column_collations
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
            .unwrap_or("BINARY");
        let collation = coll.filter(|c| !c.eq_ignore_ascii_case(column_default));
        out.push(IndexColumn {
            name,
            prefix: None,
            descending,
            expression: false,
            collation,
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

    /// **`auto_increment` means "the engine fills this in", and only a rowid
    /// alias does.** A single `INTEGER PRIMARY KEY` in a rowid table is one;
    /// each column of a composite `INTEGER` key is not, and neither is a
    /// `WITHOUT ROWID` table's key. The grid leaves a server-assigned column out
    /// of its `INSERT`, so calling both columns of a junction table
    /// auto-increment made Duplicate row write `(NULL, NULL)`.
    #[test]
    fn only_a_rowid_alias_is_server_assigned() {
        let conn = SqliteConn::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE solo (id INTEGER PRIMARY KEY, n TEXT);
             CREATE TABLE pair (a INTEGER, b INTEGER, PRIMARY KEY (a, b));
             CREATE TABLE wr (k INTEGER PRIMARY KEY, v TEXT) WITHOUT ROWID;
             CREATE TABLE texty (k TEXT PRIMARY KEY);",
        )
        .unwrap();
        let flags = |t: &str| -> Vec<(String, bool)> {
            table_columns(&conn, t)
                .unwrap()
                .into_iter()
                .map(|c| (c.name, c.auto_increment))
                .collect()
        };
        assert_eq!(
            flags("solo"),
            vec![("id".to_string(), true), ("n".to_string(), false)]
        );
        assert_eq!(
            flags("pair"),
            vec![("a".to_string(), false), ("b".to_string(), false)],
            "neither column of a composite key is assigned"
        );
        assert_eq!(
            flags("wr"),
            vec![("k".to_string(), false), ("v".to_string(), false)],
            "a WITHOUT ROWID key is the user's to supply"
        );
        assert_eq!(flags("texty"), vec![("k".to_string(), false)]);
    }

    /// The keyword is a narrower claim than "server-assigned", and it is read
    /// from the declaration — `sqlite_sequence` has no row until something has
    /// been inserted, so an empty table would have lost it.
    #[test]
    fn autoincrement_is_read_from_the_declaration() {
        let conn = SqliteConn::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE plain (id INTEGER PRIMARY KEY);
             CREATE TABLE keyed (id INTEGER PRIMARY KEY AUTOINCREMENT);
             CREATE TABLE decoy (id INTEGER PRIMARY KEY, autoincrement_note TEXT);",
        )
        .unwrap();
        let keyword = |t: &str| table_columns(&conn, t).unwrap()[0].sqlite_autoincrement;
        assert!(!keyword("plain"));
        assert!(keyword("keyed"), "and no row exists in sqlite_sequence yet");
        assert!(!keyword("decoy"), "a column name is not the keyword");
    }

    /// The two `pragma_table_xinfo.hidden` values *are* the VIRTUAL/STORED
    /// distinction, and collapsing them un-materialises a stored column.
    #[test]
    fn a_generated_column_reports_which_kind_it_is() {
        let conn = SqliteConn::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE g (a INTEGER,
                             s INTEGER GENERATED ALWAYS AS (a*2) STORED,
                             v INTEGER GENERATED ALWAYS AS (a+1) VIRTUAL);",
        )
        .unwrap();
        let cols = table_columns(&conn, "g").unwrap();
        let of = |n: &str| cols.iter().find(|c| c.name == n).unwrap().generated_stored;
        assert!(!of("a"), "not generated at all");
        assert!(of("s"));
        assert!(!of("v"));
    }

    /// No pragma reports a column's `COLLATE`, so it comes out of the table's own
    /// declaration — and a rebuild that didn't carry it made every comparison on
    /// the column case-sensitive again.
    #[test]
    fn a_columns_collation_is_read_from_the_declaration() {
        let conn = SqliteConn::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE c (email TEXT COLLATE NOCASE,
                             plain TEXT,
                             note  TEXT DEFAULT 'collate rtrim',
                             ck    TEXT CHECK (ck COLLATE NOCASE <> 'x'));",
        )
        .unwrap();
        let cols = table_columns(&conn, "c").unwrap();
        let of = |n: &str| cols.iter().find(|c| c.name == n).unwrap().collation.clone();
        assert_eq!(of("email").as_deref(), Some("NOCASE"));
        assert_eq!(of("plain"), None);
        assert_eq!(of("note"), None, "the word inside a string literal");
        assert_eq!(
            of("ck"),
            None,
            "a COLLATE inside a CHECK is not the column's"
        );
    }

    /// The two clauses that change what the table *is*, from the pragma row the
    /// implicit key already reads.
    #[test]
    fn table_list_flags_reports_without_rowid_and_strict() {
        let conn = SqliteConn::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE plain (a INTEGER);
             CREATE TABLE wr (a TEXT PRIMARY KEY) WITHOUT ROWID;
             CREATE TABLE st (a INTEGER) STRICT;
             CREATE TABLE both (a TEXT PRIMARY KEY) WITHOUT ROWID, STRICT;",
        )
        .unwrap();
        assert_eq!(table_list_flags(&conn, "plain"), (false, false));
        assert_eq!(table_list_flags(&conn, "wr"), (true, false));
        assert_eq!(table_list_flags(&conn, "st"), (false, true));
        assert_eq!(table_list_flags(&conn, "both"), (true, true));
        assert_eq!(table_list_flags(&conn, "gone"), (false, false));
    }

    /// An index's `COLLATE` is its own only when it differs from the column's —
    /// the pragma reports the *effective* collation either way.
    #[test]
    fn an_index_reports_only_the_collation_it_states_itself() {
        let conn = SqliteConn::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE m (email TEXT, ci TEXT COLLATE NOCASE);
             CREATE INDEX ix_own    ON m (email COLLATE NOCASE);
             CREATE INDEX ix_plain  ON m (email);
             CREATE INDEX ix_column ON m (ci);",
        )
        .unwrap();
        let ixs = table_indexes(&conn, "m").unwrap();
        let coll = |n: &str| {
            ixs.iter()
                .find(|i| i.name == n)
                .unwrap_or_else(|| panic!("{n}"))
                .columns[0]
                .collation
                .clone()
        };
        assert_eq!(coll("ix_own").as_deref(), Some("NOCASE"));
        assert_eq!(coll("ix_plain"), None);
        assert_eq!(
            coll("ix_column"),
            None,
            "the column already collates that way — restating it would be noise"
        );
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

    /// **How the user typed the name is not part of what the table is.** SQLite
    /// resolves an object name case-insensitively and every other lookup on this
    /// path already does — a case-sensitive `is_view` made `SELECT * FROM ARTIST`
    /// fall to "don't attribute" and open read-only, while `SELECT * FROM artist`
    /// was editable.
    #[test]
    fn a_tables_provenance_does_not_depend_on_how_its_name_was_typed() {
        let conn = seeded();
        for sql in [
            "SELECT id, name FROM artist",
            "SELECT id, name FROM ARTIST",
            "SELECT id, name FROM Artist",
            r#"SELECT id, name FROM "ArTiSt""#,
        ] {
            let o = origins_for(&conn, sql);
            assert!(
                o.iter().all(|x| x.is_some()),
                "{sql} should be editable: {o:?}"
            );
        }
        // And a view stays read-only however it is typed.
        for sql in ["SELECT id, title FROM big", "SELECT id, title FROM BIG"] {
            assert!(origins_for(&conn, sql).iter().all(|x| x.is_none()), "{sql}");
        }
    }

    /// A BLOB cell renders as its size and cannot round-trip, so its column is
    /// marked binary — the call the other two engines make for a binary charset.
    ///
    /// **Every spelling of it.** SQLite's declared type is arbitrary text, so
    /// asking `== "BLOB"` leaves `MEDIUMBLOB`, `VARBINARY(16)` and the untyped
    /// column — all of which hold raw bytes — *editable*, and committing the
    /// rendered `<N bytes>` writes that text over the data. Duplicate row does
    /// it with no cell edit at all.
    #[test]
    fn a_blob_column_is_marked_binary_and_so_read_only() {
        let conn = SqliteConn::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE t (
                 id    INTEGER PRIMARY KEY,
                 b     BLOB,
                 med   MEDIUMBLOB,
                 vb    VARBINARY(16),
                 bin   BINARY(8),
                 bare,
                 label TEXT,
                 n     REAL
             );",
        )
        .unwrap();
        let o = origins_for(&conn, "SELECT id, b, med, vb, bin, bare, label, n FROM t");
        let binary: Vec<bool> = o.iter().map(|x| x.as_ref().unwrap().binary).collect();
        assert_eq!(
            binary,
            vec![false, true, true, true, true, true, false, false]
        );
    }

    /// The value really is a blob in each of those columns — the premise the
    /// test above rests on, asked of SQLite rather than assumed.
    #[test]
    fn every_one_of_those_spellings_really_stores_bytes() {
        let conn = SqliteConn::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE t (b BLOB, med MEDIUMBLOB, vb VARBINARY(16), bin BINARY(8), bare);
             INSERT INTO t VALUES (x'00ff00ff', x'00ff00ff', x'00ff00ff', x'00ff00ff', x'00ff00ff');",
        )
        .unwrap();
        let kinds: Vec<String> = conn
            .prepare("SELECT typeof(b), typeof(med), typeof(vb), typeof(bin), typeof(bare) FROM t")
            .unwrap()
            .query_row([], |r| {
                Ok((0..5)
                    .map(|i| r.get::<_, String>(i).unwrap())
                    .collect::<Vec<_>>())
            })
            .unwrap();
        assert_eq!(kinds, vec!["blob"; 5]);
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
             CREATE TABLE keyed (id INTEGER PRIMARY KEY, a TEXT);
             CREATE VIEW v AS SELECT a FROM plain;
             INSERT INTO plain VALUES ('no', 'key');
             INSERT INTO keyed VALUES (1, 'x');",
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

    /// **What keeps the widened projection safe.** `ed7e60c` made
    /// `SELECT a, * FROM t` resolve where it used to return `None`, and a `None`
    /// projection was read-only *by construction* — no origin, no edit. Now every
    /// column gets one, and two of them claim the base column `a`. The only thing
    /// left refusing the table is `resolve_key`'s duplicate check (C1), in
    /// another crate: this walks the new shape through it end to end, so a
    /// relaxation there can't quietly make a self-ambiguous result writable.
    #[test]
    fn a_column_exposed_twice_by_the_widened_projection_stays_read_only() {
        let conn = shadowing();
        // The projection really does attribute all three now — that is the half
        // this test exists to hold the other end of.
        let o = origins_for(&conn, "SELECT a, * FROM keyed");
        assert_eq!(o.len(), 3);
        assert!(o.iter().all(|x| x.is_some()), "every column is attributed");
        assert_eq!(o[0].as_ref().unwrap().column, "a");
        assert_eq!(o[2].as_ref().unwrap().column, "a", "`a` twice");

        let rs = run_query(&conn, "SELECT a, * FROM keyed", 100).unwrap();
        let m = schemaic_core::edit::analyze_edit(&rs, |_, _, t| Some(table_info_of(&conn, t)));
        assert!(m.insert_target().is_none(), "no row can be identified");
        for ci in 0..3 {
            assert!(!m.editable(ci), "column {ci} must stay read-only");
        }
    }

    /// The other half of the same widening: a **computed** leading item is not a
    /// duplicate of anything, so `SELECT 1, * FROM t` — which was unattributable
    /// before `ed7e60c` and is editable after it — must key on the table's real
    /// primary key, with the computed column read-only rather than shifting the
    /// wildcard's expansion by one.
    #[test]
    fn a_computed_leading_item_keys_on_the_tables_own_primary_key() {
        let conn = shadowing();
        let rs = run_query(&conn, "SELECT 1, * FROM keyed", 100).unwrap();
        let m = schemaic_core::edit::analyze_edit(&rs, |_, _, t| Some(table_info_of(&conn, t)));
        assert_eq!(
            m.table(0).map(|t| t.key_cols.clone()),
            Some(vec![1]),
            "the key is `id`, at result position 1"
        );
        assert!(!m.editable(0), "a literal is no column of the table");
        // `id` is a *declared* column, so unlike the implicit rowid it is the
        // table's own data and stays writable through the key it also forms.
        assert!(m.editable(1) && m.editable(2));
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

    /// **SQLite accepts `:name`, so the driver's parameter count is what stops a
    /// skeleton run by reflex.**
    ///
    /// `core::skeleton`'s whole safety argument is that a generated draft is not
    /// a statement a server will run. That holds on MySQL and PostgreSQL because
    /// the parser refuses `:price`; on SQLite it is a documented bind-parameter
    /// form and the statement *prepares*. What refuses it is `run_query`'s
    /// `conn.execute(sql, [])` — bind nothing instead, and SQLite would bind
    /// them all as NULL and write the row.
    #[tokio::test]
    async fn a_generated_skeleton_is_refused_rather_than_run() {
        let (keeper, db) = shared_memory("skeleton_is_refused");
        keeper
            .execute_batch("CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT);")
            .unwrap();

        let table = crate::TableInfo {
            name: "notes".to_string(),
            columns: vec![
                crate::ColumnInfo {
                    name: "id".to_string(),
                    type_name: "INTEGER".to_string(),
                    primary_key: true,
                    ..Default::default()
                },
                crate::ColumnInfo {
                    name: "body".to_string(),
                    type_name: "TEXT".to_string(),
                    nullable: true,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let sql = schemaic_core::skeleton::insert_skeleton(
            schemaic_core::intel::SqlDialect::Sqlite,
            MAIN,
            &table,
        );
        assert!(sql.contains(":body"), "{sql}");

        assert!(
            db.fetch_query(None, &sql, 100, CancellationToken::new())
                .await
                .is_err(),
            "{sql}"
        );
        let after = keeper
            .query_row("SELECT count(*) FROM notes", [], |r| r.get::<_, i64>(0))
            .unwrap();
        assert_eq!(after, 0, "the draft wrote a row");
    }

    #[tokio::test]
    async fn an_openable_file_lists_the_one_database_sqlite_calls_main() {
        let (_keeper, db) = shared_memory("fetch_databases_ok");
        assert_eq!(db.fetch_databases().await.unwrap(), vec![MAIN.to_string()]);
    }

    /// A file that cannot be opened has **no** databases, rather than one called
    /// `main` that nothing can be read from.
    ///
    /// This is what the schema tree shows a dead connection: the other two
    /// engines fail their `fetch_databases` and the app empties the tree, so a
    /// SQLite connection reporting `main` regardless left a phantom node whose
    /// every child fetch failed — the connect error printed *inside* the tree,
    /// under a database that isn't there, next to a header already saying
    /// "Disconnected".
    ///
    /// Opening creates nothing (there is no `SQLITE_OPEN_CREATE`), so the path
    /// below is never written and the suite stays as pure as the rest.
    #[tokio::test]
    async fn a_file_that_cannot_be_opened_lists_no_databases_at_all() {
        let db = Db::from_parts(
            crate::Engine::Sqlite,
            String::new(),
            0,
            String::new(),
            String::new(),
            "no-such-file-9c1f2a7b.sqlite".to_string(),
        );
        let err = db.fetch_databases().await.expect_err("the file is missing");
        assert!(
            matches!(err, DbError::Connect(_)),
            "a missing file is a connect failure, not a query one: {err:?}"
        );
    }

    /// The exact count is the properties surface's whole answer on SQLite, so it
    /// is the one part of that feature with a live database behind its test.
    #[tokio::test]
    async fn counting_rows_returns_the_real_number() {
        let (keeper, db) = shared_memory("count_rows");
        keeper
            .execute_batch(
                "CREATE TABLE t (id INTEGER PRIMARY KEY);
                 INSERT INTO t VALUES (1), (2), (3);
                 CREATE TABLE empty_one (id INTEGER PRIMARY KEY);",
            )
            .unwrap();

        assert_eq!(
            db.count_rows(MAIN, None, "t", CancellationToken::new())
                .await
                .unwrap(),
            3
        );
        assert_eq!(
            db.count_rows(MAIN, None, "empty_one", CancellationToken::new())
                .await
                .unwrap(),
            0
        );
    }

    /// **A count that was cancelled reports as cancelled**, rather than the caller
    /// abandoning an answer while the scan runs on. A token cancelled before the
    /// call is the deterministic half of that: the interrupt handle reaches the
    /// async side before the query starts, so this is the same path a mid-scan
    /// Cancel takes.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_cancelled_count_stops_instead_of_answering() {
        let (keeper, db) = shared_memory("count_rows_cancel");
        keeper
            .execute_batch(
                "CREATE TABLE t (id INTEGER PRIMARY KEY);
                 INSERT INTO t VALUES (1), (2), (3);",
            )
            .unwrap();
        let token = CancellationToken::new();
        token.cancel();
        let err = db
            .count_rows(MAIN, None, "t", token)
            .await
            .expect_err("a cancelled count must not report a figure");
        assert!(matches!(err, DbError::Cancelled), "{err}");
        // And the connection is not left behind: the next count answers normally.
        assert_eq!(
            db.count_rows(MAIN, None, "t", CancellationToken::new())
                .await
                .unwrap(),
            3
        );
    }

    /// The name goes through the one quoter, so a table named after a keyword —
    /// or holding a quote character — counts rather than producing a syntax
    /// error.
    #[tokio::test]
    async fn counting_rows_quotes_an_awkward_table_name() {
        let (keeper, db) = shared_memory("count_rows_quoting");
        keeper
            .execute_batch(
                "CREATE TABLE \"order\" (id INTEGER PRIMARY KEY);
                 INSERT INTO \"order\" VALUES (1), (2);
                 CREATE TABLE \"we\"\"ird\" (id INTEGER PRIMARY KEY);
                 INSERT INTO \"we\"\"ird\" VALUES (7);",
            )
            .unwrap();

        assert_eq!(
            db.count_rows(MAIN, None, "order", CancellationToken::new())
                .await
                .unwrap(),
            2
        );
        assert_eq!(
            db.count_rows(MAIN, None, "we\"ird", CancellationToken::new())
                .await
                .unwrap(),
            1
        );
    }

    /// SQLite publishes no per-table statistics, and the fetch says so by
    /// returning nothing — not by failing, and not by inventing zeroes.
    #[tokio::test]
    async fn sqlite_reports_no_table_statistics() {
        let (keeper, db) = shared_memory("no_stats");
        keeper
            .execute_batch("CREATE TABLE t (id INTEGER PRIMARY KEY); INSERT INTO t VALUES (1);")
            .unwrap();

        let stats = db.fetch_table_stats(MAIN).await.expect("no error");
        assert!(stats.is_empty());
        assert!(stats.get(None, "t").is_none());
    }

    /// **A write cancelled before it starts writes nothing**, and says so.
    ///
    /// The count path had this same bug and only a coin flip revealed it (see
    /// `refuse_if_cancelled`): with the token already cancelled, `select!` found
    /// both the finished work and the cancellation ready and picked at random. On
    /// a *write* that coin flip is worse than a wrong number — the losing side
    /// tells the user "cancelled" over a transaction that committed, or commits
    /// one they cancelled. This pins the deterministic half: nothing ran, so
    /// nothing changed, and the error says exactly that.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_write_cancelled_before_it_starts_changes_nothing() {
        let (keeper, db) = shared_memory("commit_cancelled");
        keeper
            .execute_batch(
                "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT);
                 INSERT INTO t VALUES (1, 'a');",
            )
            .unwrap();
        let write = GridWrite {
            updates: vec![edit("t", &[("v", Some("B"))], &[("id", Value::Int(1))])],
            inserts: Vec::new(),
            deletes: Vec::new(),
        };
        let token = CancellationToken::new();
        token.cancel();

        let err = commit_writes(&db, &write, token)
            .await
            .expect_err("a cancelled write must not report a row count");
        assert!(matches!(err, DbError::Cancelled), "{err}");

        // The row is untouched — the whole point, and the half a "cancelled"
        // report would be lying about.
        let v: String = keeper
            .query_row("SELECT v FROM t WHERE id = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, "a");
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

        // The key is resolved and built the way the app does it, not hand-written.
        let rs = run_query(&keeper, "SELECT rowid, * FROM t", 100).unwrap();
        let m =
            schemaic_core::edit::analyze_edit(&rs, |_, _, name| Some(table_info_of(&keeper, name)));
        let tbl = m.insert_target().expect("a single writable table");
        assert_eq!(tbl.key_cols, vec![0]);
        let key_of = |row: usize| schemaic_core::edit::row_key(&rs, tbl, row);

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

    /// **A rowid is not a row identity, and the 1-row net cannot see that on its
    /// own.** The designer's rebuild renumbers a keyless table; nothing re-runs
    /// an open result tab, so the grid still holds the old numbers. Keyed on the
    /// number alone the `UPDATE` lands on a *neighbour* and affects exactly 1 —
    /// the number [`one_row_verdict`] is looking for — so it commits and the
    /// report says it worked. The read values ride in the `WHERE` so a
    /// renumbered rowid matches zero and the net fires.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_renumbered_rowid_fails_the_write_instead_of_hitting_a_neighbour() {
        use schemaic_core::ddl::{TableDraft, sqlite_rebuild_sql};
        let (keeper, db) = shared_memory("rowid_stale_rebuild");
        keeper
            .execute_batch(
                "CREATE TABLE t (a TEXT, b TEXT);
                 INSERT INTO t VALUES ('alice','x'), ('bob','x'), ('carol','x'), ('dave','x');
                 DELETE FROM t WHERE a = 'alice';",
            )
            .unwrap();

        // What the grid read: rowids 2, 3, 4.
        let rs = run_query(&keeper, "SELECT rowid, * FROM t", 100).unwrap();
        let m =
            schemaic_core::edit::analyze_edit(&rs, |_, _, name| Some(table_info_of(&keeper, name)));
        let tbl = m.insert_target().expect("writable");

        // A designer edit on the same table, through the real path.
        let before = fetch_schema(&db)
            .await
            .expect("introspect")
            .tables
            .into_iter()
            .find(|t| t.name == "t")
            .expect("t");
        let mut draft = TableDraft::from_table(&before);
        draft.columns.push(schemaic_core::ddl::ColumnDraft::new(
            schemaic_core::schema::ColumnInfo {
                name: "note".into(),
                type_name: "TEXT".into(),
                nullable: true,
                ..Default::default()
            },
        ));
        db.run_ddl(
            MAIN,
            &sqlite_rebuild_sql(&before, &draft),
            CancellationToken::new(),
        )
        .await
        .expect("rebuild");

        // The grid now holds a key for row `bob` (rowid 2 as read). Whether the
        // rebuild kept the numbering or not, the write must land on `bob` or on
        // nothing — never on `carol`.
        let write = GridWrite {
            updates: vec![RowEdit {
                database: MAIN.to_string(),
                schema: None,
                table: "t".to_string(),
                set: vec![("b".to_string(), Some("BOBS-EDIT".to_string()))],
                key: schemaic_core::edit::row_key(&rs, tbl, 0),
            }],
            ..Default::default()
        };
        let outcome = commit_writes(&db, &write, CancellationToken::new()).await;

        let rows: Vec<(String, String)> = keeper
            .prepare("SELECT a, b FROM t ORDER BY rowid")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        let edited: Vec<&str> = rows
            .iter()
            .filter(|(_, b)| b == "BOBS-EDIT")
            .map(|(a, _)| a.as_str())
            .collect();
        assert!(
            edited.is_empty() || edited == ["bob"],
            "the edit landed on {edited:?}, outcome {outcome:?}, rows {rows:?}"
        );
    }

    /// **Rowid reuse.** Nothing renumbers here — anything else on the connection
    /// deletes the highest row and inserts a new one, which takes the freed
    /// number. The stale tab's update and its delete both hit the new row, both
    /// affect exactly 1, and both commit.
    #[tokio::test]
    async fn a_reused_rowid_fails_the_write_instead_of_hitting_the_new_row() {
        let (keeper, db) = shared_memory("rowid_reuse");
        keeper
            .execute_batch(
                "CREATE TABLE t (a TEXT, b TEXT);
                 INSERT INTO t VALUES ('r1','one'), ('r2','two'), ('r3','three');",
            )
            .unwrap();

        let rs = run_query(&keeper, "SELECT rowid, * FROM t", 100).unwrap();
        let m =
            schemaic_core::edit::analyze_edit(&rs, |_, _, name| Some(table_info_of(&keeper, name)));
        let tbl = m.insert_target().expect("writable");
        let stale = schemaic_core::edit::row_key(&rs, tbl, 2); // rowid 3 = 'r3'

        keeper
            .execute_batch(
                "DELETE FROM t WHERE a = 'r3';
                 INSERT INTO t VALUES ('BRAND-NEW','payroll');",
            )
            .unwrap();
        let took: i64 = keeper
            .query_row("SELECT rowid FROM t WHERE a = 'BRAND-NEW'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(took, 3, "the premise: SQLite reused the freed rowid");

        for write in [
            GridWrite {
                updates: vec![RowEdit {
                    database: MAIN.to_string(),
                    schema: None,
                    table: "t".to_string(),
                    set: vec![("b".to_string(), Some("edited-by-user".to_string()))],
                    key: stale.clone(),
                }],
                ..Default::default()
            },
            GridWrite {
                deletes: vec![RowDelete {
                    database: MAIN.to_string(),
                    schema: None,
                    table: "t".to_string(),
                    key: stale.clone(),
                }],
                ..Default::default()
            },
        ] {
            commit_writes(&db, &write, CancellationToken::new())
                .await
                .expect_err("a stale rowid must match nothing, not the new row");
        }

        let rows: Vec<(String, String)> = keeper
            .prepare("SELECT a, b FROM t ORDER BY rowid")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(
            rows,
            [
                ("r1".to_string(), "one".to_string()),
                ("r2".to_string(), "two".to_string()),
                ("BRAND-NEW".to_string(), "payroll".to_string()),
            ],
            "the new row is untouched"
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
        // **And that the other edits went with it.** The verdict is reached
        // before the rollback runs and so can't know what it achieved; the
        // caller appends the clause once it does, in the wording all three
        // executors share. Without it a user reads "one statement failed" and
        // has the wrong model of what is in their table.
        assert!(
            format!("{err}").contains(schemaic_core::model::Rollback::Complete.note()),
            "the message must say the batch was rolled back: {err}"
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

    /// **What the import actually *wrote*, not how many rows it wrote.** Nothing
    /// read a value back before this, so `coerce`'s per-dialect literal could be
    /// wrong for a whole engine and every test still passed.
    ///
    /// The boolean is the case that was wrong: SQLite fell into the arm written
    /// for PostgreSQL and got `'true'`, which a NUMERIC-affinity column stores as
    /// **TEXT** — and a TEXT value in a boolean context converts to 0, so every
    /// row imported as true was invisible to `WHERE flag` and came back from
    /// `WHERE NOT flag`.
    #[tokio::test(flavor = "multi_thread")]
    async fn an_import_writes_values_sqlite_reads_back_as_it_meant_them() {
        use schemaic_core::import::{ColKind, NullRule, coerce};
        use schemaic_core::intel::SqlDialect;

        let (keeper, db) = shared_memory("import_values");
        keeper
            .execute_batch(
                "CREATE TABLE t (id INTEGER PRIMARY KEY, flag BOOLEAN, n INTEGER, s TEXT);",
            )
            .unwrap();
        let null = NullRule::default();
        let cell = |text: &str, kind: ColKind| {
            coerce(text, kind, true, &null, SqlDialect::Sqlite).expect(text)
        };
        let mut rows = [
            Ok(vec![
                Value::Int(1),
                cell("true", ColKind::Bool),
                cell("42", ColKind::Int),
                cell("hi", ColKind::Other),
            ]),
            Ok(vec![
                Value::Int(2),
                cell("false", ColKind::Bool),
                cell("-7", ColKind::Int),
                cell("", ColKind::Other),
            ]),
        ]
        .into_iter();
        let cols = ["id", "flag", "n", "s"].map(String::from).to_vec();
        let target = crate::ImportTarget {
            database: MAIN,
            schema: None,
            table: "t",
            columns: &cols,
        };
        import_rows(&db, target, &mut rows, CancellationToken::new())
            .await
            .expect("import");

        // Stored as the engine's own booleans, not as text.
        let kinds: Vec<String> = keeper
            .prepare("SELECT typeof(flag) FROM t ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(kinds, vec!["integer", "integer"], "not TEXT");

        // And the queries a user would write agree with what they imported.
        let ids = |sql: &str| -> Vec<i64> {
            keeper
                .prepare(sql)
                .unwrap()
                .query_map([], |r| r.get::<_, i64>(0))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        };
        assert_eq!(ids("SELECT id FROM t WHERE flag ORDER BY id"), vec![1]);
        assert_eq!(ids("SELECT id FROM t WHERE flag = 1 ORDER BY id"), vec![1]);
        assert_eq!(ids("SELECT id FROM t WHERE NOT flag ORDER BY id"), vec![2]);
        assert_eq!(ids("SELECT id FROM t WHERE n = 42 ORDER BY id"), vec![1]);
        assert_eq!(ids("SELECT id FROM t WHERE n = -7 ORDER BY id"), vec![2]);
        assert_eq!(ids("SELECT id FROM t WHERE s = 'hi' ORDER BY id"), vec![1]);
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

    /// **The other half of the same gate: what must *not* take the fast path.**
    /// SQLite's `DEFAULT` in an `ADD COLUMN` admits a literal, a signed number
    /// and nothing else — every operator expression without parentheses is
    /// refused. Calling those constants sent them down the one path with no
    /// transaction around it: through Copy / "Open in editor" a two-column add
    /// then half-applies, which is the exact failure `sqlite_native_add` exists
    /// to prevent.
    #[tokio::test(flavor = "multi_thread")]
    async fn an_expression_default_is_not_a_constant_and_takes_the_rebuild() {
        use schemaic_core::ddl::{self, ColumnDraft, TableDraft};
        use schemaic_core::intel::SqlDialect::Sqlite;
        use schemaic_core::schema::ColumnInfo;

        for (i, default) in ["1+2", "'a'||'b'", "-1*2", "datetime('now')"]
            .into_iter()
            .enumerate()
        {
            let (keeper, db) = shared_memory(&format!("add_col_expr_{i}"));
            keeper
                .execute_batch("CREATE TABLE t (a INTEGER); INSERT INTO t VALUES (7);")
                .unwrap();
            let schema = fetch_schema(&db).await.expect("schema");
            let table = schema.tables.iter().find(|t| t.name == "t").expect("t");

            let mut draft = TableDraft::from_table(table);
            draft.columns.push(ColumnDraft::new(ColumnInfo {
                name: "c".into(),
                type_name: "TEXT".into(),
                nullable: true,
                default: Some(default.into()),
                ..Default::default()
            }));
            let sql = ddl::diff(table, &draft, Sqlite).emit();
            assert!(
                !sql.iter().any(|s| s.contains("ADD COLUMN")),
                "DEFAULT {default} must not take the fast path: {sql:?}"
            );
            // And the rebuild it takes instead really does apply.
            db.run_ddl(MAIN, &sql, CancellationToken::new())
                .await
                .unwrap_or_else(|e| panic!("DEFAULT {default}: {e}\n{sql:?}"));
            let a: i64 = keeper
                .query_row("SELECT a FROM t", [], |r| r.get(0))
                .unwrap();
            assert_eq!(a, 7, "DEFAULT {default} lost the existing row");
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

    /// **A name belongs to the constraint it introduces.** `CONSTRAINT nn_a NOT
    /// NULL` names the *NOT NULL*; the bare `CHECK` beside it in the same column
    /// used to inherit it, so a rebuild wrote `CONSTRAINT "nn_a" CHECK (…)` and
    /// gave the table a name the user deliberately left off — accepted by SQLite,
    /// so silent and permanent.
    #[test]
    fn a_name_does_not_leak_from_one_constraint_to_the_next() {
        assert_eq!(
            one(r#"CREATE TABLE c (a TEXT CONSTRAINT nn_a NOT NULL CHECK (a <> ''))"#),
            (String::new(), "a <> ''".to_string())
        );
        // Each keyword that can carry a name of its own consumes it.
        for keyword in [
            "NOT NULL",
            "UNIQUE",
            "PRIMARY KEY",
            "REFERENCES other(id)",
            "DEFAULT 'x'",
            "COLLATE NOCASE",
        ] {
            let sql = format!("CREATE TABLE c (a TEXT CONSTRAINT n1 {keyword} CHECK (a <> ''))");
            assert_eq!(one(&sql).0, "", "{keyword}");
        }
        // And a name that really does introduce the check still lands on it.
        assert_eq!(
            one(r#"CREATE TABLE c (a TEXT NOT NULL CONSTRAINT ck_a CHECK (a <> ''))"#),
            ("ck_a".to_string(), "a <> ''".to_string())
        );
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

/// **Does the rebuilt table declare what the original declared?**
///
/// The suites above assert *consequences* — the rows came across, the CHECK
/// bites, the trigger still fires — and every one of them passes for a table
/// that came back missing `WITHOUT ROWID`, `STRICT`, a column's `COLLATE`, a
/// `STORED` generated column's storage, an expression `DEFAULT`, or with an
/// `AUTOINCREMENT` nobody asked for. This one reads `sqlite_master.sql` back and
/// compares it to what was there, which is the only question that catches those.
#[cfg(test)]
mod rebuild_fidelity_tests {
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

    /// Create `ddl`, make the smallest edit that forces a rebuild (retype the
    /// last column to itself is not an edit, so a comment-free retype is used),
    /// apply it through `diff` → `emit` → `run_ddl`, and hand back the table's
    /// declaration afterwards.
    async fn rebuilt(name: &str, table: &str, ddl: &str, edit: fn(&mut TableDraft)) -> String {
        let (keeper, db) = shared_memory(name);
        keeper.execute_batch(ddl).unwrap();
        let before = table_of(&db, table).await;
        let mut draft = TableDraft::from_table(&before);
        edit(&mut draft);
        let cs = diff(&before, &draft, SqlDialect::Sqlite);
        assert!(
            cs.unsupported().is_empty(),
            "withheld: {:?}",
            cs.unsupported()
        );
        db.run_ddl(MAIN, &cs.emit(), CancellationToken::new())
            .await
            .unwrap_or_else(|e| panic!("{name}: {e} — plan {:#?}", cs.emit()));
        keeper
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |r| r.get::<_, String>(0),
            )
            .unwrap()
    }

    /// Retype the *last* column — an edit no `ALTER TABLE` can do, so every case
    /// below really does go through the twelve steps.
    fn retype_last(d: &mut TableDraft) {
        let i = d.columns.len() - 1;
        d.columns[i].info.type_name = "TEXT".into();
    }

    /// **Create table, on SQLite, end to end.** `ddl::create` is the designer's
    /// whole New-table path (`table_designer.rs`), and its change set has to
    /// reach the engine as a statement: a plan holding one `CreateTable` is not
    /// empty, so the preview opens and Run is offered whatever the emitter
    /// produced. An emitter with no arm for it therefore reports success and
    /// creates nothing.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_new_table_is_created_from_the_designers_draft() {
        let (keeper, db) = shared_memory("fid_create");
        // The shape the designer produces: a fresh draft, no original.
        let mut draft = TableDraft::from_table(&TableInfo {
            name: "made".into(),
            columns: vec![
                schemaic_core::schema::ColumnInfo {
                    name: "id".into(),
                    type_name: "INTEGER".into(),
                    nullable: false,
                    primary_key: true,
                    auto_increment: true,
                    ..Default::default()
                },
                schemaic_core::schema::ColumnInfo {
                    name: "label".into(),
                    type_name: "TEXT".into(),
                    nullable: true,
                    ..Default::default()
                },
            ],
            ..Default::default()
        });
        draft.original = None;

        let cs = schemaic_core::ddl::create(&draft, SqlDialect::Sqlite);
        assert!(!cs.is_empty(), "the preview opens on this");
        assert!(
            !cs.emit().is_empty(),
            "…so it must emit something: {:#?}",
            cs.changes
        );
        db.run_ddl(MAIN, &cs.emit(), CancellationToken::new())
            .await
            .unwrap_or_else(|e| panic!("{e} — plan {:#?}", cs.emit()));

        keeper
            .execute_batch("INSERT INTO made (label) VALUES ('x')")
            .expect("the table is really there");
    }

    /// **The round-trip gate, on SQLite.** `docs/architecture.md` states the rule
    /// — a table drafted and diffed against *itself* must produce no changes,
    /// "since any model-fidelity gap surfaces to the user as a phantom change" —
    /// and `ddl::roundtrip` enforces it from hand-built fixtures. This is the
    /// same assertion made against **real introspection**, which is the half a
    /// fixture cannot cover: a fixture states what the reader is believed to
    /// produce, and every gap this range's findings named was a disagreement
    /// between that belief and the pragmas.
    ///
    /// Its user-visible failure is worse here than on the other two engines,
    /// because a phantom change on SQLite is not a stray line in a preview: the
    /// diff being non-empty is what routes an edit through the twelve-step
    /// rebuild, so an invented change means the table is dropped and recreated
    /// for an edit nobody made.
    #[tokio::test(flavor = "multi_thread")]
    async fn an_introspected_table_diffs_to_nothing_against_its_own_draft() {
        // Every fidelity property the range added, as declarations rather than as
        // a model — the corpus of the suite above, in one database.
        let ddl = "
            CREATE TABLE plain   (id INTEGER PRIMARY KEY, a TEXT NOT NULL, b TEXT);
            CREATE TABLE autoinc (id INTEGER PRIMARY KEY AUTOINCREMENT, n INTEGER);
            CREATE TABLE wr      (a TEXT NOT NULL, b TEXT, PRIMARY KEY (a)) WITHOUT ROWID;
            CREATE TABLE strictt (a INTEGER, b TEXT) STRICT;
            CREATE TABLE coll    (email TEXT COLLATE NOCASE, n INTEGER);
            CREATE TABLE gen     (a INTEGER, v INTEGER GENERATED ALWAYS AS (a*2) VIRTUAL,
                                  s INTEGER GENERATED ALWAYS AS (a*3) STORED);
            CREATE TABLE defs    (a TEXT DEFAULT 'hi', n INTEGER DEFAULT 3,
                                  t TEXT DEFAULT CURRENT_TIMESTAMP,
                                  e TEXT DEFAULT (datetime('now')));
            CREATE TABLE checks  (q INTEGER, n INTEGER,
                                  CONSTRAINT ck_q CHECK (q > 0), CHECK (n <> q));
            CREATE TABLE checks2 (a INTEGER, b INTEGER, CHECK (a > 0), CHECK (b > 0));
            CREATE TABLE uniq    (id INTEGER PRIMARY KEY, email TEXT UNIQUE);
            CREATE TABLE parent  (id INTEGER PRIMARY KEY);
            CREATE TABLE child   (id INTEGER PRIMARY KEY, pid INTEGER,
                                  FOREIGN KEY (pid) REFERENCES parent(id) ON DELETE CASCADE);
            CREATE TABLE idx     (id INTEGER PRIMARY KEY, email TEXT, low TEXT, n INTEGER);
            CREATE UNIQUE INDEX ix_mail ON idx (email COLLATE NOCASE);
            CREATE INDEX ix_partial ON idx (n) WHERE n > 0;
            CREATE INDEX ix_expr ON idx (lower(low));
            CREATE INDEX ix_desc ON idx (n DESC);
            CREATE TABLE trg     (id INTEGER PRIMARY KEY, n INTEGER);
            CREATE TRIGGER trg_ai AFTER INSERT ON trg BEGIN UPDATE trg SET n = 1; END;
        ";
        let (keeper, db) = shared_memory("fid_phantom");
        keeper.execute_batch(ddl).unwrap();

        let schema = fetch_schema(&db).await.expect("introspect");
        assert!(
            schema.tables.len() >= 13,
            "the premise: {}",
            schema.tables.len()
        );
        for t in &schema.tables {
            let cs = diff(t, &TableDraft::from_table(t), SqlDialect::Sqlite);
            assert!(
                cs.is_empty(),
                "{} shows a phantom change: {:#?}",
                t.name,
                cs.changes
            );
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn without_rowid_survives() {
        let sql = rebuilt(
            "fid_wr",
            "w",
            "CREATE TABLE w (a TEXT NOT NULL, b TEXT, PRIMARY KEY (a)) WITHOUT ROWID;",
            retype_last,
        )
        .await;
        assert!(sql.to_uppercase().contains("WITHOUT ROWID"), "{sql}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn strict_survives() {
        let sql = rebuilt(
            "fid_strict",
            "s",
            "CREATE TABLE s (a INTEGER, b TEXT) STRICT;",
            retype_last,
        )
        .await;
        assert!(sql.to_uppercase().contains("STRICT"), "{sql}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_columns_collation_survives() {
        let sql = rebuilt(
            "fid_coll",
            "c",
            "CREATE TABLE c (email TEXT COLLATE NOCASE, n INTEGER);",
            retype_last,
        )
        .await;
        assert!(sql.to_uppercase().contains("COLLATE NOCASE"), "{sql}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_stored_generated_column_stays_stored() {
        let sql = rebuilt(
            "fid_stored",
            "g",
            "CREATE TABLE g (a INTEGER, b INTEGER GENERATED ALWAYS AS (a*2) STORED, c INTEGER);",
            retype_last,
        )
        .await;
        assert!(sql.to_uppercase().contains("STORED"), "{sql}");
    }

    /// The one that used to kill the plan outright: `pragma_table_xinfo` strips
    /// the parentheses SQLite's grammar requires, so the re-emitted default was
    /// `near "(": syntax error` and the table could never be edited again.
    #[tokio::test(flavor = "multi_thread")]
    async fn an_expression_default_survives() {
        let sql = rebuilt(
            "fid_default",
            "e",
            "CREATE TABLE e (id INTEGER PRIMARY KEY, made TEXT DEFAULT (datetime('now')), n INTEGER);",
            retype_last,
        )
        .await;
        assert!(sql.contains("datetime('now')"), "{sql}");
        assert!(sql.contains("DEFAULT (datetime"), "parenthesised: {sql}");
    }

    /// A literal default must *not* grow a pair of parentheses it never had.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_literal_default_is_left_bare() {
        let sql = rebuilt(
            "fid_default_lit",
            "l",
            "CREATE TABLE l (a TEXT DEFAULT 'hi', n INTEGER DEFAULT 3, t TEXT DEFAULT CURRENT_TIMESTAMP, z INTEGER);",
            retype_last,
        )
        .await;
        assert!(sql.contains("DEFAULT 'hi'"), "{sql}");
        assert!(sql.contains("DEFAULT 3"), "{sql}");
        assert!(sql.contains("DEFAULT CURRENT_TIMESTAMP"), "{sql}");
    }

    /// A plain `INTEGER PRIMARY KEY` is server-assigned but is **not**
    /// `AUTOINCREMENT`; adding the keyword changes what the key promises and
    /// creates a `sqlite_sequence` row for a table that had none.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_plain_key_does_not_grow_autoincrement() {
        let sql = rebuilt(
            "fid_plainkey",
            "p",
            "CREATE TABLE p (id INTEGER PRIMARY KEY, n INTEGER);",
            retype_last,
        )
        .await;
        assert!(!sql.to_uppercase().contains("AUTOINCREMENT"), "{sql}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_declared_autoincrement_key_keeps_it() {
        let sql = rebuilt(
            "fid_autoinc",
            "a",
            "CREATE TABLE a (id INTEGER PRIMARY KEY AUTOINCREMENT, n INTEGER);",
            retype_last,
        )
        .await;
        assert!(sql.to_uppercase().contains("AUTOINCREMENT"), "{sql}");
    }

    /// **The one that refused every edit.** SQLite backs a `UNIQUE` constraint
    /// with `sqlite_autoindex_*`, a name it will not accept in a `CREATE INDEX` —
    /// so replaying it as an index failed the whole plan, and skipping it would
    /// have dropped the constraint instead. It belongs in the table body.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_unique_constraint_comes_back_as_a_constraint() {
        let (keeper, db) = shared_memory("fid_unique");
        keeper
            .execute_batch(
                "CREATE TABLE u (id INTEGER PRIMARY KEY, email TEXT UNIQUE, n INTEGER);
                 INSERT INTO u VALUES (1, 'a@b', 1);",
            )
            .unwrap();
        let before = table_of(&db, "u").await;
        let mut draft = TableDraft::from_table(&before);
        retype_last(&mut draft);
        let cs = diff(&before, &draft, SqlDialect::Sqlite);
        assert!(cs.unsupported().is_empty(), "{:?}", cs.unsupported());
        db.run_ddl(MAIN, &cs.emit(), CancellationToken::new())
            .await
            .unwrap_or_else(|e| panic!("{e} — plan {:#?}", cs.emit()));

        // The constraint is still enforced, which is the assertion that matters.
        assert!(
            keeper
                .execute_batch("INSERT INTO u VALUES (2, 'a@b', 2)")
                .is_err(),
            "the UNIQUE constraint must survive"
        );
        // And there is no `CREATE UNIQUE INDEX` claiming a reserved name.
        let sql: String = keeper
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'u'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(sql.to_uppercase().contains("UNIQUE"), "{sql}");
    }

    /// The same constraint across a **rename**: the body is rebuilt from the
    /// draft's names, so it follows the column. Replaying the engine's index
    /// would have named the old one.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_unique_constraint_follows_a_renamed_column() {
        let (keeper, db) = shared_memory("fid_unique_rename");
        keeper
            .execute_batch("CREATE TABLE o (id INTEGER PRIMARY KEY, email TEXT UNIQUE);")
            .unwrap();
        let before = table_of(&db, "o").await;
        let mut draft = TableDraft::from_table(&before);
        draft.rename_column(1, "mail");
        let cs = diff(&before, &draft, SqlDialect::Sqlite);
        assert!(cs.unsupported().is_empty(), "{:?}", cs.unsupported());
        db.run_ddl(MAIN, &cs.emit(), CancellationToken::new())
            .await
            .unwrap_or_else(|e| panic!("{e} — plan {:#?}", cs.emit()));

        keeper
            .execute_batch("INSERT INTO o VALUES (1, 'a@b')")
            .unwrap();
        assert!(
            keeper
                .execute_batch("INSERT INTO o VALUES (2, 'a@b')")
                .is_err(),
            "the constraint moved with the column"
        );
    }

    /// **An index's own `COLLATE` is what its uniqueness is measured in.** Read
    /// back without it, a case-insensitive UNIQUE index comes back
    /// case-sensitive and accepts the pair it used to refuse.
    #[tokio::test(flavor = "multi_thread")]
    async fn an_index_collation_survives() {
        let (keeper, db) = shared_memory("fid_index_coll");
        keeper
            .execute_batch(
                "CREATE TABLE m (id INTEGER PRIMARY KEY, email TEXT, n INTEGER);
                 CREATE UNIQUE INDEX ix_mail ON m (email COLLATE NOCASE);
                 INSERT INTO m VALUES (1, 'a@X', 1);",
            )
            .unwrap();
        let before = table_of(&db, "m").await;
        let mut draft = TableDraft::from_table(&before);
        retype_last(&mut draft);
        let cs = diff(&before, &draft, SqlDialect::Sqlite);
        assert!(cs.unsupported().is_empty(), "{:?}", cs.unsupported());
        db.run_ddl(MAIN, &cs.emit(), CancellationToken::new())
            .await
            .unwrap_or_else(|e| panic!("{e} — plan {:#?}", cs.emit()));

        assert!(
            keeper
                .execute_batch("INSERT INTO m VALUES (2, 'A@x', 2)")
                .is_err(),
            "the index must still compare case-insensitively"
        );
    }

    /// **A rebuild that renames a column must re-point the checks standing on
    /// it.** SQLite resolves a `CHECK`'s column references at `CREATE TABLE`
    /// time and refuses a predicate naming a column the shadow table doesn't
    /// have — named or unnamed, in a transaction or not — so the very first
    /// statement of the twelve steps failed and there was no route to the edit at
    /// all on that engine.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_rename_re_points_the_checks_the_rebuild_restates() {
        let (keeper, db) = shared_memory("fid_check_rename");
        keeper
            .execute_batch(
                "CREATE TABLE t (q INTEGER, n INTEGER,
                                 CONSTRAINT ck_q CHECK (q > 0),
                                 CHECK (n <> q));",
            )
            .unwrap();
        let before = table_of(&db, "t").await;
        assert_eq!(before.check_constraints.len(), 2, "the premise");
        let mut draft = TableDraft::from_table(&before);
        draft.rename_column(0, "qty");
        // A retype beside it, so the plan really is the rebuild rather than
        // SQLite's own `RENAME COLUMN`.
        draft.columns[1].info.type_name = "TEXT".into();

        let cs = diff(&before, &draft, SqlDialect::Sqlite);
        assert!(cs.unsupported().is_empty(), "{:?}", cs.unsupported());
        db.run_ddl(MAIN, &cs.emit(), CancellationToken::new())
            .await
            .unwrap_or_else(|e| panic!("{e} — plan {:#?}", cs.emit()));

        let sql: String = keeper
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 't'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(sql.contains("qty"), "{sql}");
        // And the constraints are real, not just spelled: the named one refuses a
        // non-positive value, the unnamed one refuses an equal pair.
        assert!(
            keeper
                .execute_batch("INSERT INTO t VALUES (0, '1')")
                .is_err(),
            "the named check still bites"
        );
        assert!(
            keeper
                .execute_batch("INSERT INTO t VALUES (5, '5')")
                .is_err(),
            "the unnamed check still bites"
        );
        keeper
            .execute_batch("INSERT INTO t VALUES (5, '6')")
            .unwrap();
    }

    /// **A trigger's replayed text is a snapshot, so a rebuild must not carry a
    /// rename.** The trigger would go back naming a column that no longer
    /// exists — SQLite accepts such a trigger and then refuses every write to
    /// the table, after a plan that reported success.
    ///
    /// Two halves, and both matter. A rename **on its own** takes SQLite's own
    /// `ALTER TABLE … RENAME COLUMN`, which re-points the trigger for us; a
    /// rename arriving *with* something only a rebuild can do is withheld in the
    /// preview, because there the verbatim replay is the only route and it
    /// cannot work.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_rename_under_a_trigger_takes_the_native_route_or_is_withheld() {
        let (keeper, db) = shared_memory("fid_trigger_rename");
        keeper
            .execute_batch(
                "CREATE TABLE t (id INTEGER PRIMARY KEY, n INTEGER, scratch INTEGER);
                 CREATE TABLE log (n INTEGER);
                 CREATE TRIGGER t_ai AFTER INSERT ON t
                   BEGIN INSERT INTO log VALUES (NEW.n); END;",
            )
            .unwrap();
        let before = table_of(&db, "t").await;

        // A rename beside a retype — only the rebuild can do both.
        let mut draft = TableDraft::from_table(&before);
        draft.rename_column(1, "amount");
        retype_last(&mut draft);
        let withheld = diff(&before, &draft, SqlDialect::Sqlite).unsupported();
        assert_eq!(withheld.len(), 1, "{withheld:?}");
        assert!(withheld[0].contains("trigger"), "{withheld:?}");

        // The rename alone: native, and the trigger comes with it.
        let mut draft = TableDraft::from_table(&before);
        draft.rename_column(1, "amount");
        let cs = diff(&before, &draft, SqlDialect::Sqlite);
        assert!(cs.unsupported().is_empty(), "{:?}", cs.unsupported());
        assert!(
            cs.emit().iter().any(|s| s.contains("RENAME COLUMN")),
            "the engine's own statement: {:#?}",
            cs.emit()
        );
        db.run_ddl(MAIN, &cs.emit(), CancellationToken::new())
            .await
            .expect("a bare rename must apply");
        keeper
            .execute_batch("INSERT INTO t (amount) VALUES (5);")
            .unwrap();
        assert_eq!(
            keeper
                .query_row("SELECT n FROM log", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            5,
            "SQLite re-pointed the trigger at the new name"
        );

        // And a retype under the same trigger is still fine — nothing moved.
        let before = table_of(&db, "t").await;
        let mut draft = TableDraft::from_table(&before);
        retype_last(&mut draft);
        let cs = diff(&before, &draft, SqlDialect::Sqlite);
        assert!(cs.unsupported().is_empty(), "{:?}", cs.unsupported());
        db.run_ddl(MAIN, &cs.emit(), CancellationToken::new())
            .await
            .expect("a retype is still allowed");
        keeper
            .execute_batch("INSERT INTO t (amount) VALUES (6);")
            .unwrap();
        assert_eq!(
            keeper
                .query_row("SELECT count(*) FROM log", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
    }

    /// **`AUTOINCREMENT`'s promise is that an id is never reused**, and the only
    /// thing keeping it is a `sqlite_sequence` row that `DROP TABLE` takes with
    /// it. The copy re-seeds the counter from the rows that survived, so a table
    /// whose highest row was deleted hands that id out a second time — and
    /// anything outside the database still holding a reference to it now resolves
    /// to a different row.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_rebuild_keeps_the_autoincrement_counter() {
        let (keeper, db) = shared_memory("fid_seq");
        keeper
            .execute_batch(
                "CREATE TABLE s (id INTEGER PRIMARY KEY AUTOINCREMENT, v TEXT, n INTEGER);
                 INSERT INTO s (v, n) VALUES ('a', 1), ('b', 2), ('c', 3);
                 DELETE FROM s WHERE id = 3;",
            )
            .unwrap();
        let seq = |k: &rusqlite::Connection| {
            k.query_row(
                "SELECT seq FROM sqlite_sequence WHERE name = 's'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap()
        };
        assert_eq!(seq(&keeper), 3, "the premise: id 3 has been issued");

        let before = table_of(&db, "s").await;
        let mut draft = TableDraft::from_table(&before);
        retype_last(&mut draft);
        let cs = diff(&before, &draft, SqlDialect::Sqlite);
        assert!(cs.unsupported().is_empty(), "{:?}", cs.unsupported());
        db.run_ddl(MAIN, &cs.emit(), CancellationToken::new())
            .await
            .unwrap_or_else(|e| panic!("{e} — plan {:#?}", cs.emit()));

        assert_eq!(seq(&keeper), 3, "the counter came across the rename");
        keeper
            .execute_batch("INSERT INTO s (v, n) VALUES ('d', 4)")
            .unwrap();
        assert_eq!(
            keeper
                .query_row("SELECT id FROM s WHERE v = 'd'", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            4,
            "id 3 must never be issued twice"
        );
    }

    /// **Three clauses the model has no field for, all measured deleted by a
    /// plan that reported success.** The round-trip gate cannot see any of them —
    /// the draft is built from the same incomplete model, so `diff` reads the
    /// untouched draft as a no-op — which is why this asks the preview instead of
    /// the declaration.
    #[tokio::test(flavor = "multi_thread")]
    async fn clauses_the_model_cannot_restate_are_withheld() {
        let (keeper, db) = shared_memory("fid_unrestatable");
        keeper
            .execute_batch(
                "CREATE TABLE p (id INTEGER PRIMARY KEY, n INTEGER);
                 CREATE TABLE c (id INTEGER PRIMARY KEY,
                                 pid INTEGER REFERENCES p(id) DEFERRABLE INITIALLY DEFERRED,
                                 n INTEGER);
                 CREATE TABLE q (a TEXT NOT NULL ON CONFLICT REPLACE DEFAULT 'z', n INTEGER);
                 CREATE TABLE k (a TEXT, n INTEGER, PRIMARY KEY (a DESC));",
            )
            .unwrap();
        for (table, clause) in [("c", "DEFERRABLE"), ("q", "ON CONFLICT"), ("k", "DESC")] {
            let before = table_of(&db, table).await;
            let mut draft = TableDraft::from_table(&before);
            retype_last(&mut draft);
            let cs = diff(&before, &draft, SqlDialect::Sqlite);
            assert!(!cs.is_empty(), "{table}: the premise is a real plan");
            let w = cs.unsupported();
            assert_eq!(w.len(), 1, "{table}: {w:?}");
            assert!(w[0].contains(clause), "{table}: {w:?}");
            // And the omission travels with the copied script, which is not
            // disabled the way Apply is.
            assert!(cs.script().contains("INCOMPLETE"), "{table}");
        }
        // The gate is narrow: an ordinary table beside them is still editable.
        let before = table_of(&db, "p").await;
        let mut draft = TableDraft::from_table(&before);
        retype_last(&mut draft);
        let cs = diff(&before, &draft, SqlDialect::Sqlite);
        assert!(cs.unsupported().is_empty(), "{:?}", cs.unsupported());
        db.run_ddl(MAIN, &cs.emit(), CancellationToken::new())
            .await
            .unwrap_or_else(|e| panic!("{e} — plan {:#?}", cs.emit()));
    }

    /// The other side of it: a table with no counter must not come back with a
    /// `sqlite_sequence` row, which would be a fact about the table that isn't
    /// true.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_rebuild_invents_no_counter() {
        let (keeper, db) = shared_memory("fid_seq_none");
        keeper
            .execute_batch(
                "CREATE TABLE p (id INTEGER PRIMARY KEY, v TEXT, n INTEGER);
                 INSERT INTO p (v, n) VALUES ('a', 1);",
            )
            .unwrap();
        let before = table_of(&db, "p").await;
        let mut draft = TableDraft::from_table(&before);
        retype_last(&mut draft);
        let cs = diff(&before, &draft, SqlDialect::Sqlite);
        db.run_ddl(MAIN, &cs.emit(), CancellationToken::new())
            .await
            .unwrap_or_else(|e| panic!("{e} — plan {:#?}", cs.emit()));
        assert_eq!(
            keeper
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE name = 'sqlite_sequence'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            0,
            "no AUTOINCREMENT anywhere, so the table should not exist at all"
        );
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

    /// **Step 9 of SQLite's own procedure, and the engine's answer beside it.** A
    /// native `ALTER TABLE … DROP COLUMN` is *refused* while a view names the
    /// column; the rebuild runs under `legacy_alter_table = ON` — right for the
    /// rename, and what removes the last check — so the same edit reported
    /// success and left `SELECT * FROM v` failing `no such column: doomed`, with
    /// nothing said until the user next opened the view.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_rebuild_that_breaks_a_view_is_refused_and_rolled_back() {
        let (keeper, db) = shared_memory("rebuild_view_drop");
        keeper
            .execute_batch(
                "CREATE TABLE t (id INTEGER PRIMARY KEY, keepme TEXT, doomed TEXT);
                 CREATE VIEW v AS SELECT id, doomed FROM t;
                 INSERT INTO t VALUES (1, 'a', 'b');",
            )
            .unwrap();
        let before = table_of(&db, "t").await;
        let mut draft = TableDraft::from_table(&before);
        // A drop *and* a retype, so the plan is the rebuild rather than SQLite's
        // own `DROP COLUMN` — which the engine would refuse for us.
        draft.remove_column(2);
        draft.columns[1].info.type_name = "BLOB".into();

        let err = db
            .run_ddl(
                MAIN,
                &sqlite_rebuild_sql(&before, &draft),
                CancellationToken::new(),
            )
            .await
            .expect_err("a plan that breaks a view must not commit");
        assert!(format!("{err}").contains("v"), "names the view: {err}");
        assert_eq!(err.applied, 0, "{err}");

        // Rolled back whole: the column is still there and the view still reads.
        assert_eq!(
            keeper
                .query_row("SELECT count(*) FROM v", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1,
            "the view must still resolve"
        );
    }

    /// The gate is about the columns a view actually names: a view over the same
    /// table that doesn't touch the dropped column is not affected, and the edit
    /// goes through.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_view_that_names_no_dropped_column_is_no_obstacle() {
        let (keeper, db) = shared_memory("rebuild_view_ok");
        keeper
            .execute_batch(
                "CREATE TABLE t (id INTEGER PRIMARY KEY, keepme TEXT, doomed TEXT);
                 CREATE VIEW v AS SELECT id, keepme FROM t;
                 INSERT INTO t VALUES (1, 'a', 'b');",
            )
            .unwrap();
        let before = table_of(&db, "t").await;
        let mut draft = TableDraft::from_table(&before);
        draft.remove_column(2);
        draft.columns[1].info.type_name = "BLOB".into();
        db.run_ddl(
            MAIN,
            &sqlite_rebuild_sql(&before, &draft),
            CancellationToken::new(),
        )
        .await
        .expect("nothing the view names has moved");
        assert_eq!(
            keeper
                .query_row("SELECT count(*) FROM v", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    /// **What was already broken is not the plan's fault** — the same rule the
    /// inherited-foreign-key reading follows. A `.db` can carry a view over a
    /// table that no longer exists, and refusing for it would take the DDL
    /// feature away from every other table in the file.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_view_that_was_already_broken_does_not_refuse_the_plan() {
        let (keeper, db) = shared_memory("rebuild_view_inherited");
        keeper
            .execute_batch(
                "CREATE TABLE t (id INTEGER PRIMARY KEY, n INTEGER);
                 INSERT INTO t VALUES (1, 1);",
            )
            .unwrap();
        let before = table_of(&db, "t").await;
        // Created after the read, because a view over a missing table is enough
        // to fail `fetch_schema` itself — a separate matter from what the plan
        // is allowed to commit.
        keeper
            .execute_batch("CREATE VIEW gone AS SELECT * FROM no_such_table;")
            .unwrap();
        assert!(
            keeper
                .query_row("SELECT 1 FROM gone", [], |r| r.get::<_, i64>(0))
                .is_err(),
            "the premise: the view is broken before the plan runs"
        );
        let mut draft = TableDraft::from_table(&before);
        draft.columns[1].info.type_name = "TEXT".into();
        db.run_ddl(
            MAIN,
            &sqlite_rebuild_sql(&before, &draft),
            CancellationToken::new(),
        )
        .await
        .expect("an unrelated broken view must not refuse the edit");
    }
}

/// Foreign keys around a rebuild — the part of the twelve steps that is about
/// the *other* tables — and, beside them, the three things a plan owes the user
/// whatever it does: a script that replays, a message that names their table,
/// and a Truncate that truncates.
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

    /// **Truncate on SQLite is an unqualified `DELETE`, and it has to work.**
    /// The menu offered it, asked "Delete all ~N rows?" and then opened an empty
    /// preview — a destructive question for something the emitter had no arm
    /// for. This is the arm, run against the engine.
    #[tokio::test(flavor = "multi_thread")]
    async fn truncate_empties_the_table_and_leaves_it_there() {
        use schemaic_core::ddl::{Change, ChangeSet};
        use schemaic_core::intel::SqlDialect;
        use schemaic_core::schema::ServerFlavour;

        let (keeper, db) = shared_memory("truncate_sqlite");
        keeper
            .execute_batch(
                "CREATE TABLE orders (id INTEGER PRIMARY KEY, n TEXT);
                 CREATE INDEX ix_n ON orders (n);
                 INSERT INTO orders VALUES (1, 'a'), (2, 'b');",
            )
            .unwrap();
        let cs = ChangeSet {
            table: "orders".into(),
            schema: None,
            dialect: SqlDialect::Sqlite,
            flavour: ServerFlavour::Unknown,
            changes: vec![Change::TruncateTable],
        };
        assert!(cs.unsupported().is_empty(), "{:?}", cs.unsupported());
        db.run_ddl(MAIN, &cs.emit(), CancellationToken::new())
            .await
            .unwrap_or_else(|e| panic!("{e} — plan {:#?}", cs.emit()));

        let rows: i64 = keeper
            .query_row("SELECT count(*) FROM orders", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 0, "every row went");
        // The *table* is still there, with its index — that is the whole
        // difference between Truncate and Drop.
        let objects: i64 = keeper
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name IN ('orders', 'ix_n')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(objects, 2);
    }

    /// **A failed rebuild must name the user's table, not ours.** Adding a
    /// `NOT NULL` column with no default to a table that *has rows* fails the
    /// copy step — the new column is in neither list, so every copied row
    /// supplies NULL — and the engine names the shadow table it failed on. An
    /// empty table succeeds, which is why a fixture without rows misses it.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_failed_rebuild_names_the_real_table() {
        use schemaic_core::ddl::{self, ColumnDraft, TableDraft};
        use schemaic_core::intel::SqlDialect;
        use schemaic_core::schema::ColumnInfo;

        let (_keeper, db) = shared_memory("rebuild_shadow_name");
        _keeper
            .execute_batch("CREATE TABLE t (a INTEGER); INSERT INTO t VALUES (7);")
            .unwrap();
        let before = fetch_schema(&db)
            .await
            .expect("introspect")
            .tables
            .into_iter()
            .find(|t| t.name == "t")
            .expect("t");
        let mut draft = TableDraft::from_table(&before);
        draft.columns.push(ColumnDraft::new(ColumnInfo {
            name: "c".into(),
            type_name: "TEXT".into(),
            nullable: false,
            ..Default::default()
        }));
        let sql = ddl::diff(&before, &draft, SqlDialect::Sqlite).emit();
        let err = db
            .run_ddl(MAIN, &sql, CancellationToken::new())
            .await
            .expect_err("a NOT NULL column with no default cannot be copied in");
        let text = format!("{err}");
        assert!(
            !text.contains(ddl::REBUILD_SUFFIX),
            "the shadow table is ours, not theirs: {text}"
        );
        assert!(
            text.contains("t.c"),
            "and it still names what failed: {text}"
        );
    }

    /// **Copy DDL has to hand back a script the engine will run.** SQLite keeps
    /// the author's own trailing comment in `sqlite_master.sql`, so trimming and
    /// appending a `;` put the terminator *inside* the comment — and the
    /// statement that followed joined it.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_tables_ddl_replays_even_with_a_comment_on_an_index() {
        let (keeper, db) = shared_memory("ddl_index_comment");
        keeper
            .execute_batch(
                "CREATE TABLE t (a INT, b INT);
                 CREATE INDEX ia ON t(a) -- why this index exists
                 ;
                 CREATE INDEX ib ON t(b);",
            )
            .unwrap();
        let ddl = fetch_schema(&db)
            .await
            .expect("introspect")
            .tables
            .into_iter()
            .find(|t| t.name == "t")
            .expect("t")
            .create_sql
            .expect("SQLite keeps its own text");
        assert!(ddl.contains("why this index exists"), "{ddl}");

        // The whole point: it replays into a fresh database.
        let fresh = SqliteConn::open_in_memory().unwrap();
        fresh
            .execute_batch(&ddl)
            .unwrap_or_else(|e| panic!("{e}\n{ddl}"));
        let indexes: i64 = fresh
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'index' AND sql IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(indexes, 2, "both indexes came across:\n{ddl}");
    }

    /// **What was already broken is not the plan's fault.** A `.db` written by
    /// the sqlite3 CLI — foreign keys off by default — very commonly carries a
    /// child row whose parent is gone, and the check scans the whole database.
    /// Without a before-reading, adding a column to an unrelated third table was
    /// refused with "the plan leaves a foreign key pointing at nothing", and
    /// *every* DDL operation on that file failed the same way for ever.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_pre_existing_dangling_row_does_not_refuse_an_unrelated_plan() {
        let (keeper, db) = shared_memory("rebuild_fk_inherited");
        keeper
            .execute_batch(
                "PRAGMA foreign_keys = OFF;
                 CREATE TABLE artist (id INTEGER PRIMARY KEY);
                 CREATE TABLE album (
                     id        INTEGER PRIMARY KEY,
                     artist_id INTEGER REFERENCES artist (id)
                 );
                 CREATE TABLE unrelated (a INTEGER);
                 INSERT INTO artist VALUES (1);
                 INSERT INTO album  VALUES (1, 999);  -- dangling before we arrive
                 INSERT INTO album  VALUES (2, 1);    -- perfectly fine",
            )
            .unwrap();
        // The premise: the file arrives inconsistent.
        let conn = open(&db).expect("open");
        assert!(
            !fk_violations(&conn).unwrap().rows.is_empty(),
            "the fixture must be broken to begin with"
        );

        db.run_ddl(
            MAIN,
            &[r#"ALTER TABLE "unrelated" ADD COLUMN "b" TEXT;"#.to_string()],
            CancellationToken::new(),
        )
        .await
        .expect("a plan that touches nothing related must still apply");

        // And a plan that dangles a reference **of its own** is still refused,
        // on the same already-inconsistent database — the row it strands is a
        // different row from the one that was there.
        let err = db
            .run_ddl(
                MAIN,
                &[r#"DELETE FROM "artist" WHERE "id" = 1;"#.to_string()],
                CancellationToken::new(),
            )
            .await
            .expect_err("the plan's own violation still refuses");
        assert!(
            format!("{err}").to_lowercase().contains("foreign key"),
            "{err}"
        );
        let artists: i64 = keeper
            .query_row("SELECT count(*) FROM artist", [], |r| r.get(0))
            .unwrap();
        assert_eq!(artists, 1, "and it rolled back");
    }

    /// **The same cascade, on the path that has no `run_ddl` around it.** The
    /// preview's Copy and "Open in editor" hand the user this exact statement
    /// list to run in a query tab, whose connection enforces foreign keys and
    /// opens no transaction. So the guard has to be *in* the list; a plan that
    /// relies on the backend setting it out of band empties the child table the
    /// moment it leaves the modal.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_script_guards_itself_when_run_outside_run_ddl() {
        let (keeper, db) = shared_memory("rebuild_cascade_script");
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
        let stmts = sqlite_rebuild_sql(&before, &draft);

        // The query tab's connection, opened the way production opens one.
        let conn = open(&db).expect("open");
        let enforcing: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(enforcing, 1, "the premise: an editor connection enforces");
        for sql in &stmts {
            conn.execute_batch(sql)
                .unwrap_or_else(|e| panic!("{sql}: {e}"));
        }

        let albums: i64 = keeper
            .query_row("SELECT count(*) FROM album", [], |r| r.get(0))
            .unwrap();
        assert_eq!(albums, 2, "the children must not have been cascaded away");
        // And the script left enforcement as it found it.
        let after: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(after, 1, "the guard closes behind itself");
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

    /// **Editing a view must not cost it its `INSTEAD OF` triggers.** SQLite has
    /// no `CREATE OR REPLACE VIEW`, so every edit is a `DROP` + `CREATE`, and
    /// the engine drops the view's triggers with it. They are the only way a
    /// SQLite view is written to, and their text lives nowhere else once the
    /// drop has run — so the plan replays them, or the edit silently turns a
    /// writable view into a read-only one.
    #[tokio::test(flavor = "multi_thread")]
    async fn editing_a_view_keeps_its_instead_of_triggers() {
        use schemaic_core::ddl::{ViewDraft, diff_view};
        let (keeper, db) = shared_memory("view_trigger_replay");
        keeper
            .execute_batch(
                "CREATE TABLE t (a INTEGER);
                 CREATE VIEW v AS SELECT a FROM t;
                 CREATE TRIGGER vi INSTEAD OF INSERT ON v
                   BEGIN INSERT INTO t(a) VALUES (NEW.a); END;
                 INSERT INTO v(a) VALUES (1);",
            )
            .unwrap();
        assert_eq!(
            keeper
                .query_row("SELECT count(*) FROM t", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1,
            "the premise: the view is writable through its trigger"
        );

        let before = table_of(&db, "v").await;
        assert!(before.is_view);
        assert_eq!(
            before.dependent_ddl.len(),
            1,
            "introspection has to collect it: {:?}",
            before.dependent_ddl
        );
        let mut draft = ViewDraft::from_table(&before).expect("a view drafts");
        draft.select = "SELECT a FROM t WHERE a > 0".into();

        let cs = diff_view(&before, &draft, SqlDialect::Sqlite);
        db.run_ddl(MAIN, &cs.emit(), CancellationToken::new())
            .await
            .expect("the view edit must apply");

        let triggers: i64 = keeper
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'trigger' AND tbl_name = 'v'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(triggers, 1, "the trigger is still there");
        // And it still fires — the assertion the catalogue count can't make.
        keeper
            .execute_batch("INSERT INTO v(a) VALUES (2);")
            .unwrap();
        assert_eq!(
            keeper
                .query_row("SELECT count(*) FROM t", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            2,
            "the view is still writable"
        );
    }

    /// A **rename** takes the same route, and its replay cannot work: the
    /// trigger's own SQL names the old view. Rather than fail after the drop has
    /// run — or quietly rewrite text the user wrote — the plan refuses in the
    /// preview. A view with nothing hanging off it renames as it always did.
    #[tokio::test(flavor = "multi_thread")]
    async fn renaming_a_view_with_triggers_is_refused_rather_than_stranded() {
        use schemaic_core::ddl::{ViewDraft, diff_view};
        let (keeper, db) = shared_memory("view_trigger_rename");
        keeper
            .execute_batch(
                "CREATE TABLE t (a INTEGER);
                 CREATE VIEW v AS SELECT a FROM t;
                 CREATE TRIGGER vi INSTEAD OF INSERT ON v
                   BEGIN INSERT INTO t(a) VALUES (NEW.a); END;
                 CREATE VIEW plain AS SELECT a FROM t;",
            )
            .unwrap();

        let before = table_of(&db, "v").await;
        let mut draft = ViewDraft::from_table(&before).expect("a view drafts");
        draft.name = "w".into();
        let withheld = diff_view(&before, &draft, SqlDialect::Sqlite).unsupported();
        assert_eq!(withheld.len(), 1, "{withheld:?}");
        assert!(withheld[0].contains("trigger"), "{withheld:?}");
        // Nothing applied, so the view and its trigger are both still there.
        assert_eq!(
            keeper
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE name IN ('v', 'vi')",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            2
        );

        // The same rename on a view with no dependents still works.
        let plain = table_of(&db, "plain").await;
        let mut draft = ViewDraft::from_table(&plain).expect("a view drafts");
        draft.name = "renamed".into();
        let cs = diff_view(&plain, &draft, SqlDialect::Sqlite);
        assert!(cs.unsupported().is_empty(), "{:?}", cs.unsupported());
        db.run_ddl(MAIN, &cs.emit(), CancellationToken::new())
            .await
            .expect("the rename must apply");
        assert_eq!(
            keeper
                .query_row("SELECT count(*) FROM renamed", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}
