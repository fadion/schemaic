//! Built-in MCP (stdio) server for the AI panel.
//!
//! Launched as `schemaic --mcp-serve` by the `claude` CLI (configured via a
//! temp-file `--mcp-config`, so no credentials ride a command line — review C6).
//! Speaks newline-delimited JSON-RPC over stdin/stdout and exposes three
//! read-only tools against the DB endpoint passed in `$SCHEMAIC_MCP_ENDPOINT`:
//!   - `run_query` — run a single read-only statement, return the rows.
//!   - `list_schema` — the server as an overview (databases → table names), or
//!     one database in full (columns, PK/NOT NULL markers, foreign-key edges).
//!   - `describe_table` — one table's DDL, foreign keys, and sample rows.
//!
//! The split is deliberate: enriching every table with keys makes each entry
//! bigger, so the broad listing stays cheap and the detail lives behind a
//! drill-down rather than blowing the model's context on a 50-database server.
//!
//! The endpoint points at the app's active connection (already tunnelled for
//! SSH, since the tunnel is just a local listener any process can use), and is a
//! structured `schemaic_db::Db` handle — no credential URL is involved.

use schemaic_core::intel::SqlDialect;
use schemaic_core::model::ResultSet;
use schemaic_core::schema::{DbSchema, TableInfo};
use schemaic_db::Db;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio_util::sync::CancellationToken;

// Max rows returned to the AI/MCP client per query (a small preview — the
// model reasons over a sample, not the full result). Distinct from QUERY_ROW_CAP.
const MCP_ROW_CAP: usize = 200;

/// Run the stdio JSON-RPC loop until stdin closes, against the resolved
/// endpoint (its `database` is the default schema `USE`d for `run_query`).
pub async fn serve(endpoint: crate::ai::McpEndpoint) {
    let crate::ai::McpEndpoint {
        db,
        database,
        samples,
    } = endpoint;
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();
    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(req) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let id = req.get("id").cloned();
        let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");

        let result: Option<Value> = match method {
            "initialize" => {
                let ver = req
                    .pointer("/params/protocolVersion")
                    .and_then(|v| v.as_str())
                    .unwrap_or("2024-11-05")
                    .to_string();
                Some(json!({
                    "protocolVersion": ver,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "schemaic", "version": env!("CARGO_PKG_VERSION") }
                }))
            }
            "tools/list" => Some(json!({ "tools": tools_list() })),
            "tools/call" => {
                let name = req
                    .pointer("/params/name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let args = req
                    .pointer("/params/arguments")
                    .cloned()
                    .unwrap_or(json!({}));
                Some(call_tool(&db, database.as_deref(), samples, &name, &args).await)
            }
            "ping" => Some(json!({})),
            // Unknown request → empty result; notifications (no id) → nothing.
            _ => id.as_ref().map(|_| json!({})),
        };

        if let (Some(id), Some(result)) = (id, result) {
            let msg = json!({ "jsonrpc": "2.0", "id": id, "result": result });
            let _ = stdout.write_all(format!("{msg}\n").as_bytes()).await;
            let _ = stdout.flush().await;
        }
    }
}

fn tools_list() -> Value {
    json!([
        {
            "name": "run_query",
            "description": "Execute ONE read-only SQL statement (SELECT/SHOW/DESCRIBE/EXPLAIN/WITH \
                            only) against the user's active database connection and return the rows.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "sql": { "type": "string", "description": "A single read-only SQL statement." }
                },
                "required": ["sql"]
            }
        },
        {
            "name": "list_schema",
            "description": "List what's on the active connection. With no arguments: every database \
                            and its table names. With `database`: that database's tables in full — \
                            columns with types, PK and NOT NULL markers, and foreign-key edges.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "database": {
                        "type": "string",
                        "description": "Limit to one database and include its columns and keys."
                    }
                }
            }
        },
        {
            "name": "describe_table",
            "description": "One table in depth: its CREATE TABLE (or CREATE VIEW) definition \
                            including indexes, its foreign keys, and a few sample rows.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "table": {
                        "type": "string",
                        "description": "Table name, exactly as list_schema printed it — including \
                                        the `schema.` prefix when it showed one."
                    },
                    "database": {
                        "type": "string",
                        "description": "Defaults to the connection's active database."
                    }
                },
                "required": ["table"]
            }
        }
    ])
}

async fn call_tool(
    db: &Db,
    database: Option<&str>,
    samples: bool,
    name: &str,
    args: &Value,
) -> Value {
    let arg = |key: &str| {
        args.get(key)
            .and_then(|s| s.as_str())
            .filter(|s| !s.is_empty())
    };
    let (text, is_error) = match name {
        "run_query" => {
            let sql = args.get("sql").and_then(|s| s.as_str()).unwrap_or("");
            run_query(db, database, sql).await
        }
        "list_schema" => list_schema(db, arg("database")).await,
        "describe_table" => match arg("table") {
            Some(table) => describe_table(db, database, arg("database"), table, samples).await,
            None => ("describe_table needs a `table`.".to_string(), true),
        },
        other => (format!("Unknown tool: {other}"), true),
    };
    json!({ "content": [ { "type": "text", "text": text } ], "isError": is_error })
}

/// The connection's SQL dialect — shared by the read-only gate, the DDL
/// skeleton, and the sample-row query builder so all three agree on quoting.
fn dialect_of(db: &Db) -> SqlDialect {
    if db.engine() == schemaic_db::Engine::Postgres {
        SqlDialect::Postgres
    } else {
        SqlDialect::MySql
    }
}

/// Statement timeout for AI-issued queries — a backstop against `SLEEP()` /
/// heavy scans holding the connection open.
const QUERY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

async fn run_query(db: &Db, database: Option<&str>, sql: &str) -> (String, bool) {
    let Some(stmt) = normalize_stmt(sql) else {
        return ("Empty query.".to_string(), true);
    };
    // Read-only gate (comment/string/identifier-aware) lives in schemaic-core,
    // where it's unit-tested alongside the other SQL analysis. Gate in the
    // connection's dialect so PG `#`-operators aren't mistaken for comments.
    if let Err(reason) = schemaic_core::sql::read_only_reason(stmt, dialect_of(db)) {
        return (format!("Rejected: {reason}."), true);
    }
    // Run with a deadline; on timeout, cancel so the query is killed server-side
    // (keep the future alive across the cancel so its KILL branch runs).
    let token = CancellationToken::new();
    let fut = db.fetch_query(database, stmt, MCP_ROW_CAP, token.clone());
    tokio::pin!(fut);
    tokio::select! {
        r = &mut fut => match r {
            Ok(rs) => (format_table(&rs), false),
            Err(e) => (format!("Query error: {e}"), true),
        },
        _ = tokio::time::sleep(QUERY_TIMEOUT) => {
            token.cancel();
            let _ = fut.await; // let fetch_query KILL the query server-side
            ("Query timed out (30s) and was cancelled.".to_string(), true)
        }
    }
}

/// Trim surrounding whitespace and a single trailing `;` from an AI-issued
/// statement, returning `None` if nothing is left. Pure so the empty/`;`-only
/// cases are unit-tested.
fn normalize_stmt(sql: &str) -> Option<&str> {
    let stmt = sql.trim().trim_end_matches(';').trim();
    (!stmt.is_empty()).then_some(stmt)
}

/// Render a result set as a pipe table (capped), for the assistant to read.
fn format_table(rs: &ResultSet) -> String {
    if rs.columns.is_empty() {
        return "(no columns)".to_string();
    }
    let mut out = String::new();
    let header: Vec<&str> = rs.columns.iter().map(|c| c.name.as_str()).collect();
    out.push_str(&format!("| {} |\n", header.join(" | ")));
    out.push_str(&format!("| {} |\n", vec!["---"; header.len()].join(" | ")));
    let ncols = rs.col_count();
    for r in 0..rs.row_count() {
        let cells: Vec<String> = (0..ncols)
            .map(|c| {
                let raw = rs.cell(r, c).map(|cell| cell.display()).unwrap_or_default();
                let s = raw.replace('|', "\\|").replace('\n', " ");
                if s.chars().count() > 60 {
                    format!("{}…", s.chars().take(60).collect::<String>())
                } else {
                    s
                }
            })
            .collect();
        out.push_str(&format!("| {} |\n", cells.join(" | ")));
    }
    let more = if rs.truncated {
        format!("\n({}+ rows, capped)", rs.row_count())
    } else {
        format!("\n({} rows)", rs.row_count())
    };
    out.push_str(&more);
    out
}

/// `list_schema` — the whole server as an overview (databases + table names), or
/// one database in full (columns, keys, foreign keys) when `database` is given.
/// Splitting it this way keeps a 50-database server from burning the model's
/// context on a listing it didn't ask for.
async fn list_schema(db: &Db, database: Option<&str>) -> (String, bool) {
    if let Some(name) = database {
        return match db.fetch_schema(name).await {
            Ok(schema) => (format_database_schema(name, &schema), false),
            Err(e) => (format!("Error reading schema for {name}: {e}"), true),
        };
    }
    let names = match db.fetch_databases().await {
        Ok(d) => d,
        Err(e) => return (format!("Error listing databases: {e}"), true),
    };
    let mut dbs = Vec::with_capacity(names.len());
    for name in names {
        let schema = db.fetch_schema(&name).await.map_err(|e| e.to_string());
        dbs.push((name, schema));
    }
    (format_database_list(&dbs), false)
}

/// The server overview: one heading per database, its tables listed by name.
/// Columns deliberately stay out — the trailing hint points at the two calls
/// that carry them. Pure so the shape is unit-tested.
fn format_database_list(dbs: &[(String, Result<DbSchema, String>)]) -> String {
    let mut out = String::new();
    for (name, schema) in dbs {
        match schema {
            Ok(s) => {
                let n = s.tables.len();
                let plural = if n == 1 { "table" } else { "tables" };
                let qualify = needs_qualifier(s);
                out.push_str(&format!("## {name} ({n} {plural})\n"));
                for t in &s.tables {
                    let view = if t.is_view { " (view)" } else { "" };
                    // Namespace-qualified so the assistant writes a name that
                    // actually resolves.
                    out.push_str(&format!("- {}{view}\n", table_label(t, qualify)));
                }
            }
            Err(e) => out.push_str(&format!("## {name} (error: {e})\n")),
        }
    }
    out.push_str(
        "\nCall list_schema with {\"database\": \"<name>\"} for that database's columns and \
         keys, or describe_table for one table's DDL, foreign keys, and sample rows.\n",
    );
    out
}

/// One database in full: every table with its columns (marked `PK` / `NOT NULL`)
/// and its foreign-key edges. The FK lines are the point — without them the
/// assistant invents join conditions we already introspected. Pure.
fn format_database_schema(database: &str, schema: &DbSchema) -> String {
    let mut out = format!("## {database}\n");
    if schema.tables.is_empty() {
        out.push_str("(no tables)\n");
        return out;
    }
    let qualify = needs_qualifier(schema);
    for t in &schema.tables {
        let view = if t.is_view { " (view)" } else { "" };
        out.push_str(&format!("### {}{view}\n", table_label(t, qualify)));
        for c in &t.columns {
            let pk = if c.primary_key { " PK" } else { "" };
            // Nullable is the default; only the constraint is worth the tokens.
            let not_null = if c.nullable { "" } else { " NOT NULL" };
            out.push_str(&format!("  {} {}{pk}{not_null}\n", c.name, c.type_name));
        }
        for line in fk_lines(t, qualify) {
            out.push_str(&format!("  {line}\n"));
        }
    }
    out
}

/// One table in depth: the `CREATE TABLE` skeleton (columns, PK, indexes) plus a
/// foreign-key section — `create_ddl` documents that it emits no FK references,
/// so they'd otherwise be missing entirely. Sample rows are appended by the
/// caller, which needs the DB. Pure.
fn format_table_detail(
    t: &TableInfo,
    dialect: schemaic_core::intel::SqlDialect,
    qualify: bool,
) -> String {
    let mut out = format!("{}\n", t.create_ddl(dialect));
    let fks = fk_lines(t, qualify);
    if !fks.is_empty() {
        out.push_str("\nForeign keys:\n");
        for line in fks {
            out.push_str(&format!("- {}\n", line.trim_start_matches("FK: ")));
        }
    }
    out
}

/// The heading above a table's detail: a name the connection's dialect can
/// actually resolve.
///
/// On MySQL the database *is* the namespace, so `database.table` is valid SQL.
/// On PostgreSQL it isn't — `warehouse.sales.orders` names nothing — so the
/// heading carries the schema-qualified name and the database goes on its own
/// line. (An earlier `{database}.{table}` heading is exactly what led a model to
/// write `warehouse.orders` for a `public` table.)
fn format_table_heading(
    database: &str,
    t: &TableInfo,
    dialect: schemaic_core::intel::SqlDialect,
    qualify: bool,
) -> String {
    match dialect {
        schemaic_core::intel::SqlDialect::MySql => {
            format!("### {database}.{}\n", t.name)
        }
        _ => format!("### {}\nDatabase: {database}\n", table_label(t, qualify)),
    }
}

/// A table's FK edges as `FK: col -> other.col` (or, for a composite key,
/// `FK: (a, b) -> other(x, y)`). ASCII arrows — the result is read by a model,
/// not rendered in the UI.
fn fk_lines(t: &TableInfo, qualify: bool) -> Vec<String> {
    t.foreign_keys
        .iter()
        .map(|fk| {
            let target = namespaced(fk.ref_schema.as_deref(), &fk.ref_table, qualify);
            if fk.columns.len() == 1 && fk.ref_columns.len() == 1 {
                format!("FK: {} -> {target}.{}", fk.columns[0], fk.ref_columns[0])
            } else {
                format!(
                    "FK: ({}) -> {target}({})",
                    fk.columns.join(", "),
                    fk.ref_columns.join(", ")
                )
            }
        })
        .collect()
}

/// Resolve the name `describe_table` was given against the database's tables.
///
/// Every form the listings can print has to round-trip: `display_name`'s output
/// (`sales.orders`, or a bare name in `public`/MySQL), and — since a
/// multi-schema listing spells out `public.` — an explicitly qualified name that
/// `display_name` would never produce.
fn find_described<'a>(schema: &'a DbSchema, table: &str) -> Option<&'a TableInfo> {
    schema
        .find_by_display(table)
        .or_else(|| {
            let (ns, name) = table.split_once('.')?;
            schema.find_table(Some(ns), name)
        })
        .or_else(|| schema.find_table(None, table))
}

/// Does this database need its namespaces spelled out? True once more than one
/// is present — the same rule the schema tree uses to grow a namespace level.
/// `schemas()` is empty on MySQL, so those always render bare.
fn needs_qualifier(schema: &DbSchema) -> bool {
    schema.schemas().len() > 1
}

/// A table's name as the assistant should write it.
fn table_label(t: &TableInfo, qualify: bool) -> String {
    namespaced(t.schema.as_deref(), &t.name, qualify)
}

/// `schema.table` when the database has several namespaces, else
/// [`display_name`]'s rule (bare in `public`, qualified elsewhere). Spelling out
/// `public.` matters only when something else could be meant.
fn namespaced(schema: Option<&str>, table: &str, qualify: bool) -> String {
    match schema {
        Some(s) if qualify => format!("{s}.{table}"),
        _ => schemaic_core::schema::display_name(schema, table),
    }
}

/// Number of sample rows `describe_table` returns — enough to show the shape of
/// the data (formats, typical magnitudes, whether a column is really populated)
/// without turning the tool result into a data dump.
const SAMPLE_ROWS: usize = 5;

/// `describe_table` — one table's DDL, foreign keys, and a few sample rows.
async fn describe_table(
    db: &Db,
    default_db: Option<&str>,
    database: Option<&str>,
    table: &str,
    samples: bool,
) -> (String, bool) {
    let Some(database) = database.or(default_db) else {
        return (
            "No database given and the connection has no default — pass a `database`.".to_string(),
            true,
        );
    };
    let schema = match db.fetch_schema(database).await {
        Ok(s) => s,
        Err(e) => return (format!("Error reading schema for {database}: {e}"), true),
    };
    let Some(info) = find_described(&schema, table) else {
        return (
            format!("Table {table} not found in {database}. Call list_schema to see what exists."),
            true,
        );
    };

    let dialect = dialect_of(db);
    let qualify = needs_qualifier(&schema);
    let mut out = format_table_heading(database, info, dialect, qualify);
    out.push_str(&format_table_detail(info, dialect, qualify));

    // Sample rows read real data, so they follow the panel's "run queries"
    // setting — with it off the assistant still gets the DDL and the keys.
    if !samples {
        return (out, false);
    }
    // Reuse the same dialect-aware, properly quoted builder the grid's "open
    // table" uses, so a reserved-word or spaced name is safe.
    let sql = schemaic_core::filter::table_query(
        dialect,
        database,
        info.schema.as_deref(),
        &info.name,
        &[],
        schemaic_core::filter::Order::Asc,
        SAMPLE_ROWS,
    );
    out.push_str(&format!("\nSample rows (up to {SAMPLE_ROWS}):\n"));
    let token = CancellationToken::new();
    match db
        .fetch_query(Some(database), &sql, SAMPLE_ROWS, token)
        .await
    {
        Ok(rs) => out.push_str(&format_table(&rs)),
        // A sample is a bonus, not the point — a view we can't select from still
        // returns its DDL and keys.
        Err(e) => out.push_str(&format!("(unavailable: {e})")),
    }
    (out, false)
}

#[cfg(test)]
mod tests {
    use super::{
        find_described, format_database_list, format_database_schema, format_table,
        format_table_detail, format_table_heading, normalize_stmt,
    };
    use schemaic_core::intel::SqlDialect;
    use schemaic_core::model::{Column, ResultSet, Value};
    use schemaic_core::schema::{ColumnInfo, DbSchema, ForeignKeyInfo, IndexInfo, TableInfo};

    fn tbl(name: &str, columns: Vec<ColumnInfo>) -> TableInfo {
        TableInfo {
            schema: None,
            name: name.to_string(),
            columns,
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            is_view: false,
            view_definition: None,
        }
    }

    fn c(name: &str, type_name: &str, nullable: bool, primary_key: bool) -> ColumnInfo {
        ColumnInfo {
            name: name.to_string(),
            type_name: type_name.to_string(),
            nullable,
            primary_key,
        }
    }

    fn fk(columns: &[&str], ref_table: &str, ref_columns: &[&str]) -> ForeignKeyInfo {
        ForeignKeyInfo {
            columns: columns.iter().map(|s| s.to_string()).collect(),
            ref_schema: None,
            ref_table: ref_table.to_string(),
            ref_columns: ref_columns.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn database_list_names_tables_and_marks_views() {
        let mut view = tbl("v_active", vec![c("id", "int", true, false)]);
        view.is_view = true;
        let dbs = vec![(
            "shop".to_string(),
            Ok(DbSchema {
                tables: vec![tbl("orders", vec![c("id", "int", false, true)]), view],
            }),
        )];
        let out = format_database_list(&dbs);
        assert!(out.contains("## shop (2 tables)"));
        assert!(out.contains("- orders\n"));
        assert!(out.contains("- v_active (view)"));
        // No columns at this level — that's what the drill-down calls are for.
        assert!(!out.contains("int"));
        assert!(out.contains("describe_table"));
    }

    #[test]
    fn database_list_reports_a_failed_introspection() {
        let dbs = vec![("locked".to_string(), Err("access denied".to_string()))];
        let out = format_database_list(&dbs);
        assert!(out.contains("## locked (error: access denied)"));
    }

    #[test]
    fn database_schema_marks_keys_not_null_and_foreign_keys() {
        let mut orders = tbl(
            "orders",
            vec![
                c("id", "int", false, true),
                c("customer_id", "int", false, false),
                c("total", "decimal(10,2)", true, false),
            ],
        );
        orders.foreign_keys = vec![fk(&["customer_id"], "customers", &["id"])];
        let schema = DbSchema {
            tables: vec![orders],
        };
        let out = format_database_schema("shop", &schema);
        assert!(out.contains("## shop"));
        assert!(out.contains("### orders"));
        assert!(out.contains("  id int PK NOT NULL"));
        assert!(out.contains("  customer_id int NOT NULL"));
        // Nullable is the default — no marker, no noise.
        assert!(out.contains("  total decimal(10,2)\n"));
        assert!(out.contains("  FK: customer_id -> customers.id"));
    }

    #[test]
    fn database_schema_renders_a_composite_foreign_key() {
        let mut t = tbl(
            "line_items",
            vec![
                c("order_id", "int", false, true),
                c("line_no", "int", false, true),
            ],
        );
        t.foreign_keys = vec![fk(&["order_id", "line_no"], "shipments", &["ord", "line"])];
        let out = format_database_schema("shop", &DbSchema { tables: vec![t] });
        assert!(out.contains("  FK: (order_id, line_no) -> shipments(ord, line)"));
    }

    #[test]
    fn database_schema_marks_a_view() {
        let mut view = tbl("v_active", vec![c("id", "int", true, false)]);
        view.is_view = true;
        let out = format_database_schema("shop", &DbSchema { tables: vec![view] });
        assert!(out.contains("### v_active (view)"));
    }

    #[test]
    fn database_schema_reports_an_empty_database() {
        let out = format_database_schema("shop", &DbSchema { tables: vec![] });
        assert!(out.contains("(no tables)"));
    }

    #[test]
    fn table_detail_carries_ddl_and_foreign_keys() {
        let mut orders = tbl(
            "orders",
            vec![
                c("id", "int", false, true),
                c("customer_id", "int", false, false),
            ],
        );
        orders.indexes = vec![IndexInfo {
            name: "ix_customer".to_string(),
            columns: vec!["customer_id".to_string()],
            unique: false,
            foreign: true,
        }];
        orders.foreign_keys = vec![fk(&["customer_id"], "customers", &["id"])];
        let out = format_table_detail(&orders, SqlDialect::MySql, false);
        // The DDL skeleton carries columns + PK + indexes…
        assert!(out.contains("CREATE TABLE `orders`"));
        assert!(out.contains("PRIMARY KEY (`id`)"));
        assert!(out.contains("KEY `ix_customer`"));
        // …but never FK references, so they get their own section.
        assert!(out.contains("Foreign keys:\n- customer_id -> customers.id"));
    }

    fn in_schema(mut t: TableInfo, schema: &str) -> TableInfo {
        t.schema = Some(schema.to_string());
        t
    }

    #[test]
    fn multi_schema_database_qualifies_every_table_including_public() {
        // `display_name` leaves a `public` table bare, which is unambiguous on
        // its own but not beside `sales.orders` — a model reading the bare name
        // invented `warehouse.orders` (database.table, invalid in PostgreSQL).
        let schema = DbSchema {
            tables: vec![
                in_schema(tbl("orders", vec![c("id", "int", false, true)]), "public"),
                in_schema(tbl("orders", vec![c("id", "int", false, true)]), "sales"),
            ],
        };
        let out = format_database_schema("warehouse", &schema);
        assert!(out.contains("### public.orders"));
        assert!(out.contains("### sales.orders"));
        // The overview qualifies the same way.
        let list = format_database_list(&[("warehouse".to_string(), Ok(schema))]);
        assert!(list.contains("- public.orders"));
        assert!(list.contains("- sales.orders"));
    }

    #[test]
    fn single_schema_database_leaves_public_tables_bare() {
        // One namespace → no ambiguity to resolve, and `public.` prefixes would
        // just be noise. MySQL tables (no namespace at all) take this path too.
        let pg = DbSchema {
            tables: vec![in_schema(
                tbl("orders", vec![c("id", "int", false, true)]),
                "public",
            )],
        };
        let out = format_database_schema("shop", &pg);
        assert!(out.contains("### orders"));
        assert!(!out.contains("public."));

        let mysql = DbSchema {
            tables: vec![tbl("orders", vec![c("id", "int", false, true)])],
        };
        assert!(format_database_schema("shop", &mysql).contains("### orders"));
    }

    #[test]
    fn multi_schema_database_qualifies_foreign_key_targets() {
        let mut child = in_schema(
            tbl("daily_revenue", vec![c("order_id", "int", false, false)]),
            "analytics",
        );
        child.foreign_keys = vec![ForeignKeyInfo {
            columns: vec!["order_id".to_string()],
            ref_schema: Some("public".to_string()),
            ref_table: "orders".to_string(),
            ref_columns: vec!["id".to_string()],
        }];
        let schema = DbSchema {
            tables: vec![
                child,
                in_schema(tbl("orders", vec![c("id", "int", false, true)]), "public"),
            ],
        };
        let out = format_database_schema("warehouse", &schema);
        // A `public` FK target is spelled out too, or the edge reads as though it
        // pointed at whatever `orders` means here.
        assert!(out.contains("FK: order_id -> public.orders.id"));
    }

    #[test]
    fn table_detail_heading_is_a_name_the_dialect_can_actually_resolve() {
        // MySQL: the database *is* the namespace, so `db.table` is valid SQL.
        let t = tbl("payments", vec![c("id", "int", false, true)]);
        let out = format_table_heading("classicmodels", &t, SqlDialect::MySql, false);
        assert_eq!(out, "### classicmodels.payments\n");

        // PostgreSQL: `warehouse.sales.orders` is not a thing, so the heading
        // carries the schema-qualified name and states the database separately.
        let t = in_schema(tbl("orders", vec![c("id", "int", false, true)]), "sales");
        let out = format_table_heading("warehouse", &t, SqlDialect::Postgres, true);
        assert!(out.starts_with("### sales.orders\n"));
        assert!(out.contains("Database: warehouse"));
        assert!(!out.contains("warehouse.sales"));
    }

    #[test]
    fn lookup_resolves_every_name_the_listing_advertises() {
        let schema = DbSchema {
            tables: vec![
                in_schema(tbl("orders", vec![c("id", "int", false, true)]), "public"),
                in_schema(tbl("orders", vec![c("id", "int", false, true)]), "sales"),
                tbl("payments", vec![c("id", "int", false, true)]),
            ],
        };
        let found = |n: &str| find_described(&schema, n).map(|t| t.schema.clone());
        // A multi-schema listing prints `public.orders`, so that name has to
        // resolve — `display_name` renders a `public` table bare, so the plain
        // display lookup never matches it.
        assert_eq!(found("public.orders"), Some(Some("public".to_string())));
        assert_eq!(found("sales.orders"), Some(Some("sales".to_string())));
        // A bare name still lands on `public` (the display-name rule).
        assert_eq!(found("orders"), Some(Some("public".to_string())));
        // MySQL tables carry no namespace.
        assert_eq!(found("payments"), Some(None));
        assert!(find_described(&schema, "nope").is_none());
    }

    #[test]
    fn table_detail_omits_the_fk_section_when_there_are_none() {
        let t = tbl("logs", vec![c("id", "int", false, true)]);
        let out = format_table_detail(&t, SqlDialect::MySql, false);
        assert!(!out.contains("Foreign keys:"));
    }

    #[test]
    fn normalize_stmt_trims_and_rejects_empty() {
        assert_eq!(normalize_stmt("  SELECT 1 ;  "), Some("SELECT 1"));
        assert_eq!(normalize_stmt("SELECT 1"), Some("SELECT 1"));
        assert_eq!(normalize_stmt("   "), None);
        assert_eq!(normalize_stmt(";"), None);
        assert_eq!(normalize_stmt(""), None);
    }

    fn col(name: &str) -> Column {
        Column {
            name: name.to_string(),
            type_name: "VARCHAR".to_string(),
            origin: None,
        }
    }

    #[test]
    fn empty_columns_reported() {
        assert_eq!(format_table(&ResultSet::default()), "(no columns)");
    }

    #[test]
    fn renders_pipe_table_with_header_and_row_count() {
        let rs = ResultSet::from_rows(
            vec![col("id"), col("name")],
            vec![
                vec![Value::Int(1), Value::Str("Ada".into())],
                vec![Value::Null, Value::Str("Bo".into())],
            ],
        );
        let out = format_table(&rs);
        assert!(out.contains("| id | name |"));
        assert!(out.contains("| --- | --- |"));
        assert!(out.contains("| 1 | Ada |"));
        assert!(out.contains("| NULL | Bo |"));
        assert!(out.ends_with("(2 rows)"));
    }

    #[test]
    fn escapes_pipes_and_newlines_and_truncates_long_cells() {
        let rs = ResultSet::from_rows(
            vec![col("v")],
            vec![
                vec![Value::Str("a|b\nc".into())],
                vec![Value::Str("x".repeat(80))],
            ],
        );
        let out = format_table(&rs);
        // Pipe escaped, newline flattened to a space.
        assert!(out.contains(r"a\|b c"));
        // Long value truncated to 60 chars + ellipsis.
        assert!(out.contains(&format!("{}…", "x".repeat(60))));
        assert!(!out.contains(&"x".repeat(61)));
    }

    #[test]
    fn truncated_result_notes_capped_rows() {
        let rs =
            ResultSet::from_rows(vec![col("id")], vec![vec![Value::Int(1)]]).with_truncated(true);
        assert!(format_table(&rs).ends_with("(1+ rows, capped)"));
    }
}
