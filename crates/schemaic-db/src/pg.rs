//! PostgreSQL backend (second engine), built on [`tokio_postgres`].
//!
//! Dispatched to from [`crate::Db`]'s public methods when the connection's engine
//! is [`crate::Engine::Postgres`]. Full parity with the MySQL path: connect, list
//! databases, run queries/batches, introspect schema, non-executing validation
//! (`prepare_check`), EXPLAIN, the Live Monitor (`fetch_table`), and transactional
//! write-back (`commit_writes`/`refetch_rows`) with the same 1-row safety net.
//!
//! **Values come back over the simple-query (text) protocol** (`simple_query`):
//! every cell arrives as its textual form, so `NUMERIC`, `UUID`, arrays, and any
//! exotic type round-trip losslessly without a per-type decoder — mirroring the
//! MySQL text-protocol path (`crate::parse_typed`). Column *types* are obtained
//! from a non-executing `PREPARE` (`Client::prepare`) so the grid still gets type
//! names (and zero-row `SELECT`s still report their columns).
//!
//! **Column provenance** (`ColumnOrigin`, driving grid editability) is resolved
//! from each prepared column's `table_oid`/`column_id` via a `pg_catalog` lookup
//! (`fetch_col_meta`) — the Postgres analog of MySQL's `org_table`/`org_name` +
//! key flags. Expression columns carry `origin: None`.
//!
//! **Model note:** a PostgreSQL *database* maps onto the app's "database" tree
//! level (mirroring a MySQL schema). Within a database we introspect the `public`
//! schema — where the standard sample databases live. Multi-schema browsing and
//! cross-database references (which PostgreSQL itself doesn't support) remain
//! future work.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use schemaic_core::model::{
    Column, ColumnFlags, ColumnOrigin, GridWrite, RefetchRow, RefetchTemplate, ResultBuilder,
    ResultSet, RowDelete, RowEdit, RowInsert, Value,
};
use schemaic_core::schema::DbSchema;
use tokio_postgres::types::Type;
use tokio_postgres::{Client, Config, NoTls, SimpleQueryMessage};
use tokio_util::sync::CancellationToken;

use crate::{Db, DbError, FkColRow, TxScope, assemble_schema, parse_typed};

/// Open a fresh connection to a specific PostgreSQL database. Unlike MySQL,
/// PostgreSQL requires connecting to a concrete database to run any statement.
/// The background connection driver is spawned onto the tokio runtime and lives
/// until the returned [`Client`] is dropped.
pub(crate) async fn connect_to(db: &Db, database: &str) -> Result<Client, DbError> {
    let mut cfg = Config::new();
    cfg.host(&db.host)
        .port(db.port)
        .user(&db.user)
        .password(&db.pass)
        .dbname(database);
    let (client, connection) = cfg
        .connect(NoTls)
        .await
        .map_err(|e| DbError::Connect(e.to_string()))?;
    // Drive the connection in the background; it completes when `client` drops.
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            tracing::debug!("postgres connection closed: {e}");
        }
    });
    Ok(client)
}

/// Connect to a *maintenance* database for server-level work (listing databases,
/// health checks) — PostgreSQL has no server-level connection. Tries the usual
/// always-present candidates in turn so it works whether or not `postgres` exists.
pub(crate) async fn connect_maintenance(db: &Db) -> Result<Client, DbError> {
    let mut last: Option<DbError> = None;
    for cand in ["postgres", db.user.as_str(), "template1"] {
        match connect_to(db, cand).await {
            Ok(c) => return Ok(c),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or_else(|| DbError::Connect("no maintenance database reachable".to_string())))
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

/// List the user databases on the server (excludes templates and the built-in
/// `postgres` maintenance database), sorted by name.
pub(crate) async fn fetch_databases(db: &Db) -> Result<Vec<String>, DbError> {
    let client = connect_maintenance(db).await?;
    let msgs = client
        .simple_query(
            "SELECT datname FROM pg_database \
             WHERE datistemplate = false AND datname <> 'postgres' \
             ORDER BY datname",
        )
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
    Ok(out)
}

/// Connect (to `database`, or the maintenance db if unscoped) and run one
/// statement.
pub(crate) async fn fetch_query(
    db: &Db,
    database: Option<&str>,
    sql: &str,
    row_cap: usize,
    cancel: CancellationToken,
) -> Result<ResultSet, DbError> {
    let client = match database {
        Some(d) => connect_to(db, d).await?,
        None => connect_maintenance(db).await?,
    };
    run_statement(&client, database.unwrap_or(""), sql, row_cap, &cancel).await
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
        let outcome = run_statement(&client, database.unwrap_or(""), sql, row_cap, &cancel).await;
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

/// Fetch up to `limit` rows of a single table for the Live Monitor. Unqualified,
/// double-quoted table name (resolved via search_path — the connection is scoped
/// to `database`), matching the write path.
pub(crate) async fn fetch_table(
    db: &Db,
    database: &str,
    table: &str,
    limit: usize,
    cancel: CancellationToken,
) -> Result<ResultSet, DbError> {
    let sql = format!("SELECT * FROM {} LIMIT {}", pg_ident(table), limit);
    fetch_query(db, Some(database), &sql, limit, cancel).await
}

/// Run `EXPLAIN sql` (or `EXPLAIN ANALYZE sql`) and return the plan as a result
/// set (the caller parses it with `schemaic_core::plan`). Plain `EXPLAIN` only
/// plans (safe for any statement); `ANALYZE` **executes** it, so callers gate it
/// to read-only. Postgres spells the analyzing form `EXPLAIN ANALYZE` natively —
/// no MariaDB-style `ANALYZE <stmt>` fallback is needed.
pub(crate) async fn explain(
    db: &Db,
    database: Option<&str>,
    sql: &str,
    analyze: bool,
    cancel: CancellationToken,
) -> Result<ResultSet, DbError> {
    let stmt = sql.trim().trim_end_matches(';').trim_end();
    let cmd = if analyze {
        format!("EXPLAIN ANALYZE {stmt}")
    } else {
        format!("EXPLAIN {stmt}")
    };
    fetch_query(db, database, &cmd, 10_000, cancel).await
}

/// Execute one statement over the text protocol and materialize a [`ResultSet`].
/// Column names + types come from a non-executing `PREPARE`; when the statement
/// isn't preparable (some utility statements) the columns fall back to those on
/// the first returned row (names only). A statement with no result columns
/// (DML/DDL) reports its affected-row count instead of a grid.
pub(crate) async fn run_statement(
    client: &Client,
    database: &str,
    sql: &str,
    row_cap: usize,
    cancel: &CancellationToken,
) -> Result<ResultSet, DbError> {
    let start = Instant::now();

    // Column metadata from a non-executing PREPARE. The prepared columns carry
    // names + types AND per-column provenance (`table_oid` / `column_id`) — the
    // Postgres analog of MySQL's `org_table`/`org_name`, which the editing system
    // (`analyze_edit`) needs. Columns from expressions carry no provenance.
    let prepared_cols: Option<Vec<Column>> = match client.prepare(sql).await {
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
                            table: m.table.clone(),
                            column: m.column.clone(),
                            flags: m.flags,
                            binary,
                        });
                    }
                }
            }
            Some(columns)
        }
        Err(_) => None,
    };

    // Execute over the text protocol, honoring cancellation via the cancel token.
    let token = client.cancel_token();
    let messages = tokio::select! {
        r = client.simple_query(sql) => r.map_err(|e| DbError::Query(e.to_string()))?,
        _ = cancel.cancelled() => {
            let _ = token.cancel_query(NoTls).await;
            return Err(DbError::Cancelled);
        }
    };

    // Split into rows + affected-row count; derive columns from the first row if
    // PREPARE didn't yield any (e.g. an unpreparable statement that still returns
    // rows).
    let mut rows: Vec<tokio_postgres::SimpleQueryRow> = Vec::new();
    let mut affected: u64 = 0;
    let mut columns = prepared_cols;
    for msg in messages {
        match msg {
            SimpleQueryMessage::Row(r) => {
                if columns.is_none() {
                    columns = Some(
                        r.columns()
                            .iter()
                            .map(|c| Column {
                                name: c.name().to_string(),
                                type_name: String::new(),
                                origin: None,
                            })
                            .collect(),
                    );
                }
                rows.push(r);
            }
            SimpleQueryMessage::CommandComplete(n) => affected = n,
            _ => {}
        }
    }

    // No result columns → DML/DDL/utility: report affected rows, not a grid.
    let columns = match columns {
        Some(c) if !c.is_empty() => c,
        _ => {
            return Ok(ResultSet::affected_rows(Vec::new(), affected)
                .with_elapsed(start.elapsed().as_millis()));
        }
    };

    // Parse each text cell by its column type (integers/floats become compact
    // numeric variants; everything else stays an exact string — never lossy).
    let type_names: Vec<String> = columns.iter().map(|c| c.type_name.clone()).collect();
    let ncols = columns.len();
    let mut builder = ResultBuilder::new(columns);
    let mut truncated = false;
    for r in &rows {
        if builder.row_count() >= row_cap {
            truncated = true; // a row beyond the cap exists → result is truncated
            break;
        }
        let cells: Vec<Value> = (0..ncols)
            .map(|i| match r.get(i) {
                None => Value::Null,
                Some(s) => parse_typed(s.to_string(), &type_names[i]),
            })
            .collect();
        builder.push_row(&cells);
    }
    builder.set_truncated(truncated);
    builder.set_elapsed(start.elapsed().as_millis());
    Ok(builder.finish())
}

/// Introspect one database's `public` schema (tables → columns + PK/unique/FK +
/// all indexes) via `information_schema` + `pg_catalog`, then hand the rows to the
/// shared, engine-agnostic [`assemble_schema`] (same folding as MySQL).
pub(crate) async fn fetch_schema(db: &Db, database: &str) -> Result<DbSchema, DbError> {
    let client = connect_to(db, database).await?;

    // Tables (BASE TABLE / VIEW).
    let table_rows: Vec<(String, String)> = query_all(
        &client,
        "SELECT table_name, table_type FROM information_schema.tables \
         WHERE table_schema = 'public' ORDER BY table_name",
    )
    .await?
    .into_iter()
    .map(|r| (cell(&r, 0), cell(&r, 1)))
    .collect();

    // Indexes via `pg_catalog` (every index, not just constraint-backed ones),
    // columns in `indkey` order. `unnest(... ) WITH ORDINALITY` preserves column
    // order; the primary-key index is renamed "PRIMARY" so `IndexInfo::is_primary()`
    // (and `create_ddl`) treat it the MySQL way. `pk_set` is derived from it.
    let idx_all = query_all(
        &client,
        "SELECT c.relname, \
                CASE WHEN ix.indisprimary THEN 'PRIMARY' ELSE ic.relname END AS iname, \
                CASE WHEN ix.indisunique THEN 0 ELSE 1 END AS non_unique, \
                a.attname, ix.indisprimary \
         FROM pg_index ix \
         JOIN pg_class c ON c.oid = ix.indrelid \
         JOIN pg_class ic ON ic.oid = ix.indexrelid \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         JOIN unnest(ix.indkey) WITH ORDINALITY AS k(attnum, ord) ON true \
         JOIN pg_attribute a ON a.attrelid = ix.indrelid AND a.attnum = k.attnum \
         WHERE n.nspname = 'public' AND a.attnum > 0 \
         ORDER BY c.relname, iname, k.ord",
    )
    .await?;
    let idx_rows: Vec<(String, String, i64, String)> = idx_all
        .iter()
        .map(|r| {
            (
                cell(r, 0),
                cell(r, 1),
                cell(r, 2).parse::<i64>().unwrap_or(1),
                cell(r, 3),
            )
        })
        .collect();
    // Primary-key columns = the columns of the primary index (`indisprimary`).
    let pk_set: HashSet<(String, String)> = idx_all
        .iter()
        .filter(|r| cell(r, 4) == "t")
        .map(|r| (cell(r, 0), cell(r, 3)))
        .collect();

    // Columns for the whole schema, in ordinal order. Type names come from
    // `udt_name` (the underlying pg type — `varchar`, `timestamp`, `int4`) mapped
    // through the shared `pg_type_name_str`, so the schema panel shows the SAME
    // short names as the grid (which maps the wire type) — not the verbose
    // `data_type` ("character varying", "timestamp without time zone").
    let col_rows: Vec<(String, String, String, String, String)> = query_all(
        &client,
        "SELECT table_name, column_name, udt_name, is_nullable \
         FROM information_schema.columns \
         WHERE table_schema = 'public' \
         ORDER BY table_name, ordinal_position",
    )
    .await?
    .into_iter()
    .map(|r| {
        let (t, c) = (cell(&r, 0), cell(&r, 1));
        let key = if pk_set.contains(&(t.clone(), c.clone())) {
            "PRI".to_string()
        } else {
            String::new()
        };
        (t, c, pg_type_name_str(&cell(&r, 2)), cell(&r, 3), key)
    })
    .collect();

    // Foreign keys via `pg_catalog`: pair `conkey` (referencing) with `confkey`
    // (referenced) by ordinal so composite FKs map their columns correctly (the
    // `constraint_column_usage` join can mis-pair them).
    let fk_col_rows: Vec<FkColRow> = query_all(
        &client,
        "SELECT c.relname, con.conname, a.attname, rn.nspname, rc.relname, ra.attname \
         FROM pg_constraint con \
         JOIN pg_class c ON c.oid = con.conrelid \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         JOIN pg_class rc ON rc.oid = con.confrelid \
         JOIN pg_namespace rn ON rn.oid = rc.relnamespace \
         JOIN unnest(con.conkey) WITH ORDINALITY AS k(attnum, ord) ON true \
         JOIN pg_attribute a ON a.attrelid = con.conrelid AND a.attnum = k.attnum \
         JOIN unnest(con.confkey) WITH ORDINALITY AS fk(attnum, ord) ON fk.ord = k.ord \
         JOIN pg_attribute ra ON ra.attrelid = con.confrelid AND ra.attnum = fk.attnum \
         WHERE con.contype = 'f' AND n.nspname = 'public' \
         ORDER BY c.relname, con.conname, k.ord",
    )
    .await?
    .into_iter()
    .map(|r| {
        (
            cell(&r, 0),
            cell(&r, 1),
            cell(&r, 2),
            Some(cell(&r, 3)),
            Some(cell(&r, 4)),
            Some(cell(&r, 5)),
        )
    })
    .collect();

    // View definitions.
    let view_rows: Vec<(String, String)> = query_all(
        &client,
        "SELECT table_name, view_definition FROM information_schema.views \
         WHERE table_schema = 'public'",
    )
    .await?
    .into_iter()
    .map(|r| (cell(&r, 0), cell(&r, 1)))
    .collect();

    Ok(assemble_schema(
        &table_rows,
        &col_rows,
        &fk_col_rows,
        &idx_rows,
        &view_rows,
    ))
}

/// Run a read-only SELECT and return every row as a `Vec<Option<String>>` (one
/// entry per column, `None` = SQL NULL) over the text protocol.
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
fn cell(row: &[Option<String>], i: usize) -> String {
    row.get(i).and_then(|c| c.clone()).unwrap_or_default()
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
/// float names are chosen so [`parse_typed`] recognizes them (its `starts_with`
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
        "SELECT a.attrelid, a.attnum, a.attname, c.relname, \
                a.attnotnull, \
                (a.attidentity <> '' OR (a.atthasdef AND \
                    COALESCE(pg_get_expr(ad.adbin, ad.adrelid), '') LIKE 'nextval(%')) AS auto_inc, \
                (NOT a.atthasdef AND a.attidentity = '') AS no_default, \
                (pk.attnum IS NOT NULL) AS is_pk \
         FROM pg_attribute a \
         JOIN pg_class c ON c.oid = a.attrelid \
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
        let flags = ColumnFlags {
            primary_key: cell(&r, 7) == "t",
            unique_key: false, // not surfaced yet (key-icon nicety); PK covers editing
            not_null: cell(&r, 4) == "t",
            auto_increment: cell(&r, 5) == "t",
            no_default: cell(&r, 6) == "t",
        };
        out.insert(
            (oid, attnum),
            ColMeta {
                table: cell(&r, 3),
                column: cell(&r, 2),
                flags,
            },
        );
    }
    Ok(out)
}

// ── Write-back (commit + refetch) ────────────────────────────────────────────

/// Double-quote a Postgres identifier, doubling any embedded quote.
fn pg_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// A SQL string literal (single-quoted, quotes doubled). Safe under Postgres's
/// default `standard_conforming_strings = on` (backslashes are literal). Such an
/// "unknown"-typed literal coerces to the target column type — the text-path
/// analog of a bound parameter.
fn pg_str_lit(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// A settable cell value (`Some(text)` or explicit SQL `NULL`) as a literal.
fn pg_opt_lit(v: &Option<String>) -> String {
    match v {
        Some(s) => pg_str_lit(s),
        None => "NULL".to_string(),
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
        .map(|(c, v)| format!("{} = {}", pg_ident(c), pg_opt_lit(v)))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "UPDATE {} SET {set_sql} WHERE {}",
        pg_ident(&edit.table),
        where_key(&edit.key)
    )
}

/// `INSERT INTO "t" (cols) VALUES (…)`, or `INSERT INTO "t" DEFAULT VALUES` when
/// no columns are set (Postgres's all-defaults form — MySQL's `() VALUES ()` is
/// invalid here).
fn build_insert(ins: &RowInsert) -> String {
    if ins.cols.is_empty() {
        return format!("INSERT INTO {} DEFAULT VALUES", pg_ident(&ins.table));
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
        .map(|(_, v)| pg_opt_lit(v))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "INSERT INTO {} ({cols}) VALUES ({vals})",
        pg_ident(&ins.table)
    )
}

/// `DELETE FROM "t" WHERE <key>`.
fn build_delete(del: &RowDelete) -> String {
    format!(
        "DELETE FROM {} WHERE {}",
        pg_ident(&del.table),
        where_key(&del.key)
    )
}

fn one_row_err(action: &str, database: &str, table: &str, n: u64) -> DbError {
    DbError::Query(format!(
        "{action} {database}.{table} affected {n} rows (expected exactly 1) — \
         rolled back all changes"
    ))
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
            let _ = token.cancel_query(NoTls).await;
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
    let qerr = |e: tokio_postgres::Error| DbError::Query(e.to_string());
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
        action: &str,
        database: &str,
        table: &str,
    ) -> Result<u64, DbError> {
        let n = match client.execute(sql.as_str(), &[]).await {
            Ok(n) => n,
            Err(e) => {
                let _ = client.batch_execute(scope.rollback_sql()).await;
                return Err(DbError::Query(e.to_string()));
            }
        };
        if n != 1 {
            let _ = client.batch_execute(scope.rollback_sql()).await;
            return Err(one_row_err(action, database, table, n));
        }
        Ok(n)
    }

    let mut total: u64 = 0;
    for del in &write.deletes {
        total += one(
            client,
            scope,
            build_delete(del),
            "delete on",
            &del.database,
            &del.table,
        )
        .await?;
    }
    for edit in &write.updates {
        total += one(
            client,
            scope,
            build_update(edit),
            "update on",
            &edit.database,
            &edit.table,
        )
        .await?;
    }
    for ins in &write.inserts {
        total += one(
            client,
            scope,
            build_insert(ins),
            "insert into",
            &ins.database,
            &ins.table,
        )
        .await?;
    }

    if let Err(e) = client.batch_execute(scope.commit_sql()).await {
        let _ = client.batch_execute(scope.rollback_sql()).await;
        return Err(qerr(e));
    }
    Ok(total)
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
            let _ = token.cancel_query(NoTls).await;
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
    let type_names: Vec<String> = {
        let probe = format!("SELECT {proj} FROM {} LIMIT 0", pg_ident(&template.table));
        match client.prepare(&probe).await {
            Ok(stmt) => stmt
                .columns()
                .iter()
                .map(|c| pg_type_name(c.type_()))
                .collect(),
            Err(_) => vec![String::new(); template.columns.len()],
        }
    };

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
            let sql = format!(
                "SELECT {proj} FROM {} WHERE {where_sql} LIMIT 1",
                pg_ident(&template.table)
            );
            let msgs = client
                .simple_query(&sql)
                .await
                .map_err(|e| DbError::Query(e.to_string()))?;
            for m in msgs {
                if let SimpleQueryMessage::Row(r) = m {
                    let cells: Vec<Value> = (0..template.columns.len())
                        .map(|i| match r.get(i) {
                            None => Value::Null,
                            Some(s) => parse_typed(s.to_string(), &type_names[i]),
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

    #[test]
    fn parse_typed_uses_pg_type_names() {
        // The pg type names feed the shared parse_typed correctly.
        assert!(matches!(
            parse_typed("42".to_string(), &pg_type_name(&Type::INT4)),
            Value::Int(42)
        ));
        assert!(matches!(
            parse_typed("1.5".to_string(), &pg_type_name(&Type::FLOAT8)),
            Value::Float(f) if f == 1.5
        ));
        // NUMERIC preserved as an exact string.
        assert!(matches!(
            parse_typed("1.10".to_string(), &pg_type_name(&Type::NUMERIC)),
            Value::Str(s) if s == "1.10"
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
            table: "city".into(),
            set: vec![
                ("name".into(), Some("Kabul".into())),
                ("district".into(), None), // set to NULL
            ],
            key: vec![("id".into(), Value::Int(1))],
        };
        assert_eq!(
            build_update(&edit),
            "UPDATE \"city\" SET \"name\" = 'Kabul', \"district\" = NULL \
             WHERE \"id\" IS NOT DISTINCT FROM 1"
        );
    }

    #[test]
    fn build_insert_shapes_including_default_values() {
        let ins = RowInsert {
            database: "world".into(),
            table: "country".into(),
            cols: vec![("code".into(), Some("AAA".into())), ("name".into(), None)],
        };
        assert_eq!(
            build_insert(&ins),
            "INSERT INTO \"country\" (\"code\", \"name\") VALUES ('AAA', NULL)"
        );
        // No columns set → Postgres all-defaults form (NOT `() VALUES ()`).
        let empty = RowInsert {
            database: "world".into(),
            table: "t".into(),
            cols: vec![],
        };
        assert_eq!(build_insert(&empty), "INSERT INTO \"t\" DEFAULT VALUES");
    }

    #[test]
    fn build_delete_shape_composite_key() {
        let del = RowDelete {
            database: "world".into(),
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
}
