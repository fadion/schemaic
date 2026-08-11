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
use schemaic_core::model::{
    Column, ColumnFlags, ColumnOrigin, GridWrite, RefetchRow, RefetchTemplate, ResultBuilder,
    ResultSet, Rollback, RowDelete, RowEdit, RowInsert, Value, WriteStep, one_row_verdict,
};
use schemaic_core::schema::{ColumnInfo, DbSchema, IndexColumn, ViewOptions};
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
    fetch_query(db, Some(database), &sql, limit, cancel).await
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
        return fetch_query(db, database, &format!("EXPLAIN {stmt}"), 10_000, cancel).await;
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
        &client,
        database.unwrap_or(""),
        &format!("EXPLAIN ANALYZE {stmt}"),
        10_000,
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
/// [`DdlError::applied`] is always 0 on this path.
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
                            schema: Some(m.schema.clone()),
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
            let _ = token.cancel_query(NoTls).await;
            return Err(DbError::Cancelled);
        }
    };
    let mut stream = std::pin::pin!(stream);

    // Start the builder from PREPARE's columns when it gave any, so a zero-row
    // SELECT still reports its columns. Otherwise the first row names them (names
    // only) — an unpreparable statement that still returns rows. Still `None`
    // after the loop ⇒ DML/DDL/utility, which reports affected rows, not a grid.
    let mut grid: Option<(ResultBuilder, Vec<String>)> = prepared_cols
        .filter(|c| !c.is_empty())
        .map(|c| (ResultBuilder::new(c.clone()), type_names_of(&c)));
    let mut affected: u64 = 0;
    let mut truncated = false;

    loop {
        // Cancellation is checked per message now rather than only around the
        // whole call, so stopping a long fetch takes effect at the next row
        // instead of after the last one.
        let next = tokio::select! {
            n = stream.next() => n,
            _ = cancel.cancelled() => {
                let _ = token.cancel_query(NoTls).await;
                return Err(DbError::Cancelled);
            }
        };
        let Some(msg) = next else { break };
        match msg.map_err(|e| db_err(&e))? {
            SimpleQueryMessage::Row(r) => {
                let (builder, type_names) = grid.get_or_insert_with(|| {
                    let cols: Vec<Column> = r
                        .columns()
                        .iter()
                        .map(|c| Column {
                            name: c.name().to_string(),
                            type_name: String::new(),
                            origin: None,
                        })
                        .collect();
                    let names = type_names_of(&cols);
                    (ResultBuilder::new(cols), names)
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
                let cells: Vec<Value> = (0..type_names.len())
                    .map(|i| match r.get(i) {
                        None => Value::Null,
                        Some(s) => parse_typed(s.to_string(), &type_names[i]),
                    })
                    .collect();
                builder.push_row(&cells);
            }
            SimpleQueryMessage::CommandComplete(n) => affected = n,
            _ => {}
        }
    }

    // No result columns → DML/DDL/utility: report affected rows, not a grid.
    let Some((mut builder, _)) = grid else {
        return Ok(ResultSet::affected_rows(Vec::new(), affected)
            .with_elapsed(start.elapsed().as_millis()));
    };
    builder.set_truncated(truncated);
    builder.set_elapsed(start.elapsed().as_millis());
    Ok(builder.finish())
}

/// The per-column type names the text-cell parser keys on, in column order.
fn type_names_of(columns: &[Column]) -> Vec<String> {
    columns.iter().map(|c| c.type_name.clone()).collect()
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
/// tables. Each resulting [`TableInfo`](schemaic_core::schema::TableInfo) carries
/// its namespace, and the schemas are concatenated `public`-first.
pub(crate) async fn fetch_schema(db: &Db, database: &str) -> Result<DbSchema, DbError> {
    let client = connect_to(db, database).await?;

    // Every browsable object (BASE TABLE / VIEW / materialized view), across
    // every user schema. See `table_list_sql` for why this can't come from
    // `information_schema`.
    let table_rows: Vec<(String, String, String)> = query_all(&client, &table_list_sql())
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
    let idx_all = query_all(&client, &index_list_sql()).await?;
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
        &client,
        &format!(
            "SELECT n.nspname, c.relname, a.attname, \
                    format_type(a.atttypid, a.atttypmod), \
                    a.attnotnull, \
                    pg_get_expr(ad.adbin, ad.adrelid), \
                    a.attidentity, a.attgenerated, \
                    col_description(c.oid, a.attnum), \
                    co.collname \
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
        let identity = !cell(&r, 6).is_empty();
        let generated = !cell(&r, 7).is_empty();
        let default = r.get(5).cloned().flatten();
        let column = ColumnInfo {
            primary_key: pk_set.contains(&(ns.clone(), t.clone(), c.clone())),
            name: c,
            type_name: cell(&r, 3),
            // `attnotnull` arrives over the text protocol as `t`/`f`.
            nullable: cell(&r, 4) != "t",
            // A generated column's `pg_get_expr` *is* its expression, and it
            // must not also be emitted as a DEFAULT.
            generated: generated.then(|| default.clone().unwrap_or_default()),
            default: default.clone().filter(|_| !generated && !identity),
            // Both an identity column and a `serial` (a default of
            // `nextval(...)`) are server-assigned; the designer treats them the
            // same way, so both land here.
            auto_increment: identity
                || default
                    .as_deref()
                    .is_some_and(|d| d.starts_with("nextval(")),
            // …but they are *not* the same to a writer: `attidentity = 'a'`
            // (`GENERATED ALWAYS`) rejects an explicit value, where `'d'`
            // (`BY DEFAULT`) and `serial` accept one.
            identity_always: cell(&r, 6) == "a",
            comment: r.get(8).cloned().flatten(),
            collation: r.get(9).cloned().flatten(),
            on_update: None,
        };
        (ns, ColRow { table: t, column })
    })
    .collect();

    // Foreign keys via `pg_catalog`: pair `conkey` (referencing) with `confkey`
    // (referenced) by ordinal so composite FKs map their columns correctly (the
    // `constraint_column_usage` join can mis-pair them).
    let fk_all = query_all(
        &client,
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
        &client,
        &format!(
            "SELECT n.nspname, c.relname, pg_get_viewdef(c.oid, true), \
                    c.relkind, array_to_string(c.reloptions, ',') \
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

    // Post-fold enrichment: the two things `assemble_schema` can't carry because
    // MySQL has no equivalent — an index's backing constraint, and each foreign
    // key's referential actions.
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
                pg_view_options(&cell(r, 3), &cell(r, 4)),
            )
        })
        .collect();
    for t in &mut tables {
        let ns = t.schema.clone().unwrap_or_default();
        if t.is_view {
            t.view_options = view_options.get(&(ns.clone(), t.name.clone())).cloned();
        }
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
    Ok(DbSchema { tables })
}

/// A view's options from its `pg_class` row: the relation kind (`v` a view, `m`
/// a materialized one) and its storage parameters, already flattened to
/// `a=1,b=2` by `array_to_string`.
///
/// `check_option` is lifted out because both engines spell that one the same way
/// in DDL (`WITH CASCADED CHECK OPTION`); everything else stays verbatim, since
/// it's restated inside the `WITH (…)` it came from. Pure + tested.
pub(crate) fn pg_view_options(relkind: &str, reloptions: &str) -> ViewOptions {
    let mut out = ViewOptions {
        materialized: relkind == "m",
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
    format!("\"{}\"", name.replace('"', "\"\""))
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
        .map(|(_, v)| pg_opt_lit(v))
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
            let _ = token.cancel_query(NoTls).await;
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

    /// `check_option` is lifted out (it's a DDL clause on both engines); the
    /// rest is restated verbatim inside the `WITH (…)` it came from.
    #[test]
    fn pg_view_options_splits_check_option_from_the_storage_parameters() {
        let o = pg_view_options("v", "security_barrier=true,check_option=cascaded");
        assert_eq!(o.check_option.as_deref(), Some("CASCADED"));
        assert_eq!(o.storage, vec!["security_barrier=true".to_string()]);
        assert!(!o.materialized);

        // A plain view with no options at all.
        assert_eq!(pg_view_options("v", ""), ViewOptions::default());

        // `relkind = 'm'` is the one thing that makes a view uneditable here.
        assert!(pg_view_options("m", "").materialized);
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
            schema: None,
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
            schema: None,
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
            set: vec![("total".into(), Some("9".into()))],
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
            cols: vec![("total".into(), Some("9".into()))],
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
}
