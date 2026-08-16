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
use schemaic_core::model::{Column, ResultBuilder, ResultSet, Value};
use schemaic_core::schema::{
    ColumnInfo, DbSchema, ForeignKeyInfo, IndexColumn, IndexInfo, TableInfo,
};
use tokio_util::sync::CancellationToken;

use crate::{Db, DbError};

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

    let columns = columns_of(&stmt);
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
            tables.push(TableInfo {
                name,
                schema: None,
                columns,
                indexes,
                foreign_keys,
                is_view,
                view_definition: is_view.then(|| sql.clone()),
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
