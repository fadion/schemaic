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

use std::collections::HashSet;

use schemaic_core::ddl::Target;
use schemaic_core::intel::SqlDialect;
use schemaic_core::model::ResultSet;
use schemaic_core::propose::{self, Proposal};
use schemaic_core::schema::{DbSchema, TableInfo};
use schemaic_db::Db;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio_util::sync::CancellationToken;

// Max rows returned to the AI/MCP client per query (a small preview — the
// model reasons over a sample, not the full result). Distinct from QUERY_ROW_CAP.
const MCP_ROW_CAP: usize = 200;

// Characters of one cell a tool result carries. Deliberately narrower than a
// chat attachment's: this rides along on every turn that calls a tool, where the
// attachment was asked for once.
const MCP_CELL_CHARS: usize = 60;

/// The MCP protocol revisions this server actually implements, newest first.
const SUPPORTED_PROTOCOLS: &[&str] = &["2024-11-05"];

/// Answer the `initialize` handshake with a version this server really speaks.
///
/// It used to echo whatever the client asked for, which makes the handshake a
/// mirror: a future `claude` negotiating a revised protocol — different
/// tool-result shape, different error convention — would be told "yes, I speak
/// that" and then get `2024-11-05` behaviour. The failure mode isn't a crash but
/// a silent skew: results that parse as empty, or tools reported unavailable,
/// with nothing pointing at the version.
///
/// Per the MCP spec the server replies with a version it supports; if that isn't
/// the one requested, the client decides whether to proceed. So an unknown
/// request gets our newest rather than an error — the client can still walk away,
/// and it is told the truth either way.
fn negotiate_protocol(requested: Option<&str>) -> &'static str {
    requested
        .and_then(|r| SUPPORTED_PROTOCOLS.iter().find(|s| **s == r).copied())
        .unwrap_or(SUPPORTED_PROTOCOLS[0])
}

/// Run the stdio JSON-RPC loop until stdin closes, against the resolved
/// endpoint (its `database` is the default schema `USE`d for `run_query`).
pub async fn serve(endpoint: crate::ai::McpEndpoint) {
    let crate::ai::McpEndpoint {
        db,
        database,
        samples: reads_data,
        hidden,
        schema,
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
                let ver = negotiate_protocol(
                    req.pointer("/params/protocolVersion")
                        .and_then(|v| v.as_str()),
                );
                Some(json!({
                    "protocolVersion": ver,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "schemaic", "version": env!("CARGO_PKG_VERSION") }
                }))
            }
            "tools/list" => Some(json!({ "tools": tools_list(db.engine(), reads_data, schema) })),
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
                Some(
                    call_tool(
                        &db,
                        database.as_deref(),
                        reads_data,
                        schema,
                        &hidden,
                        &name,
                        &args,
                    )
                    .await,
                )
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

/// Which tools read row data, and so exist only on a connection whose
/// [`AiData`](schemaic_core::connection::AiData) level lets the assistant fetch
/// rows for itself.
///
/// One predicate, consulted twice — [`tools_list`] leaves the tool out and
/// [`refusal_for`] turns a call to it away — so a tool can never be advertised
/// by one rule and denied by another.
fn reads_row_data(tool: &str) -> bool {
    tool == "run_query"
}

/// Does this tool read the **catalogue** — the thing *Schema context* governs?
///
/// The same one-predicate-consulted-twice shape [`reads_row_data`] has, and for
/// the same reason. `propose_table_change` is deliberately not on the list: it
/// carries the table it is about in the call and reads nothing the model did not
/// already have.
fn reads_schema(tool: &str) -> bool {
    matches!(tool, "list_schema" | "describe_table")
}

/// What the server says when a schema tool is called at *Schema context: None*.
///
/// Names the setting, because the model's next move should be to ask the user
/// for the table and column names (or to raise the setting) rather than retry.
const NO_SCHEMA_ACCESS: &str = "Refused: the user's Schema context setting is None, so the \
     assistant is given no database structure and list_schema and describe_table are \
     unavailable. Ask the user for the table and column names you need — or tell them to set \
     Schema context to a database in AI settings.";

/// What the server says when a withheld tool is called anyway. It names the
/// setting, because the model's next move should be to ask the user for the
/// values (or to change the level) rather than to retry the same call.
const NO_DATA_ACCESS: &str = "Refused: this connection's AI data access is set so the assistant \
     reads no rows on its own, so run_query is unavailable. Ask the user for the values you need \
     — they can attach rows from a result grid, or raise the connection's AI data access \
     setting.";

/// The refusal a tool call earns from the connection's data-access level, if
/// any. Pure, so the gate is unit-tested without a live endpoint.
fn refusal_for(tool: &str, reads_data: bool, schema: bool) -> Option<&'static str> {
    if !reads_data && reads_row_data(tool) {
        return Some(NO_DATA_ACCESS);
    }
    (!schema && reads_schema(tool)).then_some(NO_SCHEMA_ACCESS)
}

/// The tools this server offers, described for **this** connection's engine.
///
/// `run_query`'s advertised statement heads come from
/// [`schemaic_core::sql::read_only_heads`], the same list the gate enforces — a
/// hard-coded `SELECT/SHOW/DESCRIBE/EXPLAIN/WITH` told every model that a SQLite
/// connection accepted `SHOW TABLES` and `DESCRIBE t`, which SQLite has no syntax
/// for, so a model reasoning from the tool's own text would spend turns on
/// statements that could only ever come back as parser errors.
///
/// `reads_data` is the connection's level: with it off, the tools that read rows
/// are not advertised at all. Listing a tool and then denying the call is worse
/// than not listing it — the model plans a turn around the tool it can see,
/// offers the user an analysis it cannot deliver, and only discovers the denial
/// once the user has said yes.
///
/// `schema` is the app's *Schema context* setting, and it withholds the
/// catalogue tools the same way. With the setting at `None` the system prompt
/// carries no databases and no tables — and the model held `list_schema`, whose
/// first call hands back every database and every table name. Whether that
/// setting is read as a token budget or as consent, a budget of zero that one
/// tool call walks around is neither.
pub(crate) fn tools_list(engine: schemaic_db::Engine, reads_data: bool, schema: bool) -> Value {
    let dialect = engine.dialect();
    let heads = schemaic_core::sql::read_only_heads(dialect).join("/");
    let mut tools = json!([
        {
            "name": "run_query",
            "description": format!(
                "Execute ONE read-only SQL statement ({heads} only) against the user's active \
                 database connection and return the rows. The connection is {}, so write it in \
                 that dialect.",
                dialect.engine_label(),
            ),
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
        },
        {
            "name": "propose_table_change",
            "description": format!(
                "Check a proposed change to a table and see the {} SQL it would produce. \
                 **This runs nothing and changes nothing** — it reads the table, applies your ops \
                 to it, and hands back the DDL plus anything destructive in it. Call it before \
                 offering a schema change so you find out here, not in front of the user, that a \
                 column is spelled differently than you thought. When it comes back valid, put the \
                 *same* JSON in a ```{} fenced block in your reply: Schemaic turns that into a \
                 change preview the user reviews and applies themselves. Never write the DDL out \
                 for them to run.\n\n\
                 Ops are a patch — what you don't name, you don't touch, so send only what should \
                 change. One op per object: \
                 {{\"add_column\": {{\"name\", \"type\", \"nullable\"?, \"default\"?, \"comment\"?, \
                 \"auto_increment\"?, \"generated\"?}}}}, \
                 {{\"alter_column\": {{\"name\", \"type\"?, \"nullable\"?, \"default\"?, \
                 \"drop_default\"?, \"comment\"?}}}}, \
                 {{\"drop_column\": {{\"name\"}}}}, {{\"rename_column\": {{\"from\", \"to\"}}}}, \
                 {{\"set_primary_key\": {{\"columns\": []}}}}, \
                 {{\"add_index\": {{\"columns\": [], \"name\"?, \"unique\"?}}}}, \
                 {{\"drop_index\": {{\"name\"}}}}, \
                 {{\"add_foreign_key\": {{\"columns\": [], \"ref_table\", \"ref_columns\": [], \
                 \"name\"?, \"ref_schema\"?, \"on_delete\"?, \"on_update\"?}}}}, \
                 {{\"drop_foreign_key\": {{\"name\"}}}}, \
                 {{\"add_check\": {{\"expression\", \"name\"?}}}}, {{\"drop_check\": {{\"name\"}}}}, \
                 {{\"rename_table\": {{\"to\"}}}}. \
                 Types are written the way this server writes them ({}). Unknown keys are refused \
                 rather than ignored.",
                dialect.engine_label(),
                schemaic_core::propose::FENCE_TAG,
                dialect.engine_label(),
            ),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "table": {
                        "type": "string",
                        "description": "The table to change, as list_schema printed it."
                    },
                    "database": {
                        "type": "string",
                        "description": "Defaults to the connection's active database."
                    },
                    "schema": {
                        "type": "string",
                        "description": "PostgreSQL namespace, when the table is outside the default."
                    },
                    "summary": {
                        "type": "string",
                        "description": "One line on what this change is for, shown above the preview."
                    },
                    "ops": {
                        "type": "array",
                        "description": "The changes to make, applied in order.",
                        "items": { "type": "object" }
                    }
                },
                "required": ["table", "ops"]
            }
        }
    ]);
    if let Some(list) = tools.as_array_mut() {
        list.retain(|t| {
            let name = t["name"].as_str().unwrap_or("");
            (reads_data || !reads_row_data(name)) && (schema || !reads_schema(name))
        });
    }
    tools
}

async fn call_tool(
    db: &Db,
    database: Option<&str>,
    reads_data: bool,
    schema: bool,
    hidden: &HashSet<String>,
    name: &str,
    args: &Value,
) -> Value {
    // The level gates the call as well as the listing: a client working from a
    // stale `tools/list` must not reach the DB through a tool this connection
    // withheld.
    if let Some(why) = refusal_for(name, reads_data, schema) {
        return json!({ "content": [ { "type": "text", "text": why } ], "isError": true });
    }
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
        "list_schema" => list_schema(db, arg("database"), database, hidden).await,
        "describe_table" => match arg("table") {
            Some(table) => describe_table(db, database, arg("database"), table, reads_data).await,
            None => ("describe_table needs a `table`.".to_string(), true),
        },
        "propose_table_change" => propose_change(db, database, arg("database"), args).await,
        other => (format!("Unknown tool: {other}"), true),
    };
    json!({ "content": [ { "type": "text", "text": text } ], "isError": is_error })
}

/// The connection's SQL dialect — shared by the read-only gate, the DDL
/// skeleton, and the sample-row query builder so all three agree on quoting.
///
/// It delegates to [`schemaic_db::Engine::dialect`], whose `match` is
/// exhaustive, and that is the point. This read
/// `if engine == Postgres { Postgres } else { MySql }` — which compiled cleanly
/// when SQLite arrived and quietly sorted it onto the MySQL side, so the
/// read-only gate lexed AI-issued SQL by MySQL's backslash and `#` rules (SQLite
/// has neither) and `describe_table`'s sample query quoted against MySQL's
/// reserved words instead of SQLite's, leaving a table named `virtual` or
/// `indexed` unquoted.
fn dialect_of(db: &Db) -> SqlDialect {
    db.engine().dialect()
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
    let header: Vec<String> = rs.columns.iter().map(|c| c.name.clone()).collect();
    let ncols = rs.col_count();
    let rows: Vec<Vec<String>> = (0..rs.row_count())
        .map(|r| {
            (0..ncols)
                .map(|c| {
                    rs.cell(r, c)
                        .map(|cell| cell.display().to_string())
                        .unwrap_or_default()
                })
                .collect()
        })
        .collect();
    // One table renderer for everything a model reads (`core::prompt`), so a
    // cell carrying `|`, a newline or a novel is handled the same here as in a
    // chat attachment. The width is this call site's: a tool result rides along
    // on every turn, so it stays narrow.
    let mut out = schemaic_core::prompt::pipe_table(&header, &rows, MCP_CELL_CHARS);
    let more = if rs.truncated {
        format!("\n({}+ rows, capped)", rs.row_count())
    } else {
        format!("\n({} rows)", rs.row_count())
    };
    out.push_str(&more);
    out
}

/// Which databases the server **overview** reports.
///
/// The SCHEMA eye's rule, reaching the assistant's tools as well as its prompt:
/// a database the user has put away is not one the assistant should be
/// discovering and proposing work in. `schema::db_contributes` carries the same
/// exception it does everywhere else — the session's own default database is
/// listed even when hidden, since that is the database the user is *in* and its
/// queries run there regardless.
///
/// **Only the overview.** `list_schema {"database": "archive"}` still answers in
/// full, and so do `describe_table` and `run_query`, which is the same line the
/// app draws for itself: hiding governs what is *offered* — a listing is an
/// offer — and never what is *true*, which a named lookup asks for. A tool that
/// refused to describe a table the user had explicitly named would be lying
/// about the server rather than tidying a view, and `run_query` reaching any
/// database means a half-filtered fence would only be theatre.
fn listed_databases(
    names: Vec<String>,
    hidden: &HashSet<String>,
    default_db: Option<&str>,
) -> Vec<String> {
    names
        .into_iter()
        .filter(|n| schemaic_core::schema::db_contributes(hidden, n, default_db))
        .collect()
}

/// `list_schema` — the whole server as an overview (databases + table names), or
/// one database in full (columns, keys, foreign keys) when `database` is given.
/// Splitting it this way keeps a 50-database server from burning the model's
/// context on a listing it didn't ask for.
async fn list_schema(
    db: &Db,
    database: Option<&str>,
    default_db: Option<&str>,
    hidden: &HashSet<String>,
) -> (String, bool) {
    if let Some(name) = database {
        return match db.fetch_schema(name).await {
            Ok(schema) => (format_database_schema(name, &schema), false),
            Err(e) => (format!("Error reading schema for {name}: {e}"), true),
        };
    }
    let names = match db.fetch_databases().await {
        Ok(d) => listed_databases(d, hidden, default_db),
        Err(e) => return (format!("Error listing databases: {e}"), true),
    };
    let mut dbs = Vec::with_capacity(names.len());
    for name in names {
        // The *list*, not the schema: this prints table names, and introspecting
        // every column of every table of every database to do that is the whole
        // server's catalogue for an answer that doesn't use it.
        let schema = db.fetch_table_list(&name).await.map_err(|e| e.to_string());
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
/// [`schemaic_core::schema::display_name`]'s rule (bare in `public`, qualified
/// elsewhere). Spelling out
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
    // table" uses, so a reserved-word or spaced name is safe. No key is passed
    // for either purpose: the sample is unordered on purpose, and an implicit row
    // key would add a column the assistant has no use for — nothing writes back
    // through this path.
    let sql = schemaic_core::filter::table_query(
        dialect,
        database,
        info.schema.as_deref(),
        &info.name,
        schemaic_core::filter::BrowseKey::None,
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

/// The proposal inside a `propose_table_change` call's arguments.
///
/// `database` is the *tool's* argument, not the proposal's, and
/// [`Proposal`] refuses unknown keys on purpose — a model that invents a field
/// has to be told rather than have it quietly dropped — so it comes off here
/// instead of being tolerated there. Pure, so both halves of that are tested.
fn proposal_from_args(args: &Value) -> Result<Proposal, String> {
    let mut fields = args.clone();
    if let Some(obj) = fields.as_object_mut() {
        obj.remove("database");
    }
    serde_json::from_value(fields).map_err(|e| format!("That isn't a valid proposal: {e}"))
}

/// `propose_table_change` — check a proposed change and show the DDL it would
/// produce. **Reads only.** Nothing here executes anything.
///
/// The value is the loop it closes: the model finds out *here* that it misread a
/// column name or asked for something the designer's own validation refuses,
/// instead of the user being handed a preview that can't be applied. What comes
/// back is the same SQL the preview modal will show, so the model is looking at
/// what the user will look at.
///
/// It deliberately stops there. The result goes to the *model*, and this process
/// has no route into the app's overlays — the proposal reaches the user only
/// when the model echoes it into its reply, where the app picks it up. So a
/// model that skips this tool has lost a check, not a safety rail: the preview
/// and the Apply click are downstream of both paths.
async fn propose_change(
    db: &Db,
    default_db: Option<&str>,
    database: Option<&str>,
    args: &Value,
) -> (String, bool) {
    let Some(database) = database.or(default_db) else {
        return (
            "No database given and the connection has no default — pass a `database`.".to_string(),
            true,
        );
    };
    let proposal = match proposal_from_args(args) {
        Ok(p) => p,
        Err(e) => return (e, true),
    };

    let schema = match db.fetch_schema(database).await {
        Ok(s) => s,
        Err(e) => return (format!("Error reading schema for {database}: {e}"), true),
    };
    let dialect = dialect_of(db);
    // **The same resolver the proposal card uses**, so the table this validates
    // is the table the user is later offered. The two used to disagree about
    // `proposal.schema` and about a qualified `sales.orders`, which is how a
    // change reported valid for `public.orders` reached a modal targeting
    // `sales.orders`.
    let info = match propose::resolve_target(&schema, &proposal) {
        Ok(t) => t,
        Err(e) => return (format!("{e} (in {database})"), true),
    };
    let draft = match propose::apply(info, &proposal, dialect) {
        Ok(d) => d,
        Err(e) => return (e.to_string(), true),
    };
    // The one differ, and the flavour the schema was read with — the MySQL
    // emitter's `ALTER TABLE` path reads it, so dropping it here would produce a
    // preview that differs from the designer's for the same change.
    let changes = schemaic_core::ddl::diff(info, &draft, Target::new(dialect, schema.flavour));
    if changes.is_empty() {
        return (
            format!(
                "Those ops leave {} exactly as it is — there is nothing to propose.",
                proposal.table
            ),
            true,
        );
    }

    // The change list the preview shows, in the preview's own words, so the
    // model and the user are reading the same account of the change.
    let mut out = String::from("Valid. Nothing has run.\n\nChanges:\n");
    for c in &changes.changes {
        out.push_str(&format!("- {}\n", c.summary()));
    }
    out.push_str(&format!("\nSQL:\n{}\n", changes.emit().join("\n")));
    let risks = changes.destructive();
    if !risks.is_empty() {
        out.push_str("\nDestructive — the preview will say so too:\n");
        for r in &risks {
            out.push_str(&format!("- {r}\n"));
        }
    }
    out.push_str(&format!(
        "\nTo offer this to the user, put the same JSON in a ```{} fenced block in your reply. \
         They get a change preview and apply it themselves — don't hand them the SQL to run.",
        propose::FENCE_TAG,
    ));
    (out, false)
}

#[cfg(test)]
mod tests {
    use super::{
        HashSet, NO_DATA_ACCESS, SUPPORTED_PROTOCOLS, dialect_of, find_described,
        format_database_list, format_database_schema, format_table, format_table_detail,
        format_table_heading, listed_databases, negotiate_protocol, normalize_stmt,
        proposal_from_args, reads_row_data, refusal_for,
    };

    /// Tool names the server advertises, in order.
    fn offered(engine: schemaic_db::Engine, reads_data: bool) -> Vec<String> {
        offered_with(engine, reads_data, true)
    }

    fn offered_with(
        engine: schemaic_db::Engine,
        reads_data: bool,
        reads_schema: bool,
    ) -> Vec<String> {
        super::tools_list(engine, reads_data, reads_schema)
            .as_array()
            .expect("a list")
            .iter()
            .map(|t| t["name"].as_str().expect("a name").to_string())
            .collect()
    }

    /// A connection that lets the assistant read rows is offered every tool.
    #[test]
    fn the_query_tool_is_offered_when_the_connection_sends_rows() {
        for engine in [
            schemaic_db::Engine::MySql,
            schemaic_db::Engine::Postgres,
            schemaic_db::Engine::Sqlite,
        ] {
            assert!(
                offered(engine, true).contains(&"run_query".to_string()),
                "{engine:?} withholds run_query from a connection that allows it"
            );
        }
    }

    /// The tool is *withheld*, not merely refused, when the connection sends no
    /// rows on the assistant's own initiative. A model that can see the tool
    /// plans around it — it offers to "run a query and analyse the results for
    /// you", the user says yes, and only then does the call come back denied.
    /// Nothing in a system prompt reliably outranks a tool the model can see, so
    /// the listing is where the setting has to bite.
    #[test]
    fn the_query_tool_is_withheld_when_the_connection_sends_no_rows() {
        for engine in [
            schemaic_db::Engine::MySql,
            schemaic_db::Engine::Postgres,
            schemaic_db::Engine::Sqlite,
        ] {
            let names = offered(engine, false);
            assert!(
                !names.contains(&"run_query".to_string()),
                "{engine:?} still advertises run_query: {names:?}"
            );
            // The schema tools are untouched — the level withholds *data*, not
            // the ability to look at the shape of the database.
            for kept in ["list_schema", "describe_table", "propose_table_change"] {
                assert!(
                    names.contains(&kept.to_string()),
                    "{engine:?} dropped {kept}: {names:?}"
                );
            }
        }
    }

    /// One predicate decides both halves, so a tool can never be listed by one
    /// rule and refused by another.
    #[test]
    fn only_the_query_tool_counts_as_reading_rows() {
        assert!(reads_row_data("run_query"));
        for schema_tool in ["list_schema", "describe_table", "propose_table_change"] {
            assert!(!reads_row_data(schema_tool));
        }
    }

    /// A client that calls the withheld tool anyway — an older `claude` that
    /// cached the listing, or one that never asked — is refused by the server
    /// itself, and told which setting to point the user at.
    #[test]
    fn a_call_to_the_withheld_tool_is_refused_with_the_reason() {
        assert_eq!(refusal_for("run_query", false, true), Some(NO_DATA_ACCESS));
        assert_eq!(refusal_for("run_query", true, true), None);
        for schema_tool in ["list_schema", "describe_table", "propose_table_change"] {
            assert_eq!(refusal_for(schema_tool, false, true), None);
        }
    }

    /// **"Schema context: None" has to reach the tools, or it means nothing.**
    ///
    /// With the setting at `None` the system prompt carries no databases and no
    /// tables — and the model still held `list_schema`, whose first call returns
    /// every database and every table name, and `list_schema {"database": …}`,
    /// which returns every column, key and foreign key. The setting the user
    /// just chose was defeated in one tool call, with only the tool's collapsed
    /// result line on screen. `c7bc9d7` closed exactly this on the inline path
    /// and left the tool path open.
    ///
    /// The same shape `reads_data` already has: withheld from the listing *and*
    /// refused on call, so a stale `tools/list` cannot reach it either.
    #[test]
    fn the_schema_tools_are_withheld_when_the_user_asked_for_no_schema() {
        for engine in [
            schemaic_db::Engine::MySql,
            schemaic_db::Engine::Postgres,
            schemaic_db::Engine::Sqlite,
        ] {
            let names = offered_with(engine, true, false);
            for gone in ["list_schema", "describe_table"] {
                assert!(
                    !names.contains(&gone.to_string()),
                    "{engine:?} still advertises {gone}: {names:?}"
                );
            }
            // `run_query` is the connection's decision, not this one, and a
            // proposal carries its own schema in the call.
            for kept in ["run_query", "propose_table_change"] {
                assert!(
                    names.contains(&kept.to_string()),
                    "{engine:?} dropped {kept}: {names:?}"
                );
            }
        }
        // …and a call to one anyway is refused with the setting named.
        for gone in ["list_schema", "describe_table"] {
            let refusal = refusal_for(gone, true, false).expect("refused");
            assert!(refusal.contains("Schema context"), "{refusal}");
        }
        assert_eq!(refusal_for("run_query", true, false), None);
        assert_eq!(refusal_for("propose_table_change", true, false), None);
    }

    /// The tool's arguments carry `database` alongside the proposal's own
    /// fields; the proposal type refuses unknown keys, so it has to come off
    /// before parsing or every call fails.
    #[test]
    fn a_proposal_parses_out_of_the_tools_arguments() {
        let args = serde_json::json!({
            "database": "shop",
            "table": "orders",
            "ops": [{"add_column": {"name": "deleted_at", "type": "datetime"}}]
        });
        let p = proposal_from_args(&args).expect("parses");
        assert_eq!(p.table, "orders");
        assert_eq!(p.ops.len(), 1);
    }

    /// And nothing *else* unknown gets the same pass — a model inventing a field
    /// must be told, because a silently-dropped key is a change the user was
    /// promised and wouldn't get.
    #[test]
    fn an_invented_argument_is_still_refused() {
        let args = serde_json::json!({
            "database": "shop",
            "table": "orders",
            "cascade": true,
            "ops": []
        });
        assert!(proposal_from_args(&args).is_err());
    }

    /// The proposal tool has to advertise the fence tag the app actually reads,
    /// or the model writes a block nothing picks up and the user is told a
    /// change is waiting that never arrives.
    #[test]
    fn the_proposal_tool_advertises_the_tag_the_app_looks_for() {
        for engine in [
            schemaic_db::Engine::MySql,
            schemaic_db::Engine::Postgres,
            schemaic_db::Engine::Sqlite,
        ] {
            let tools = super::tools_list(engine, true, true);
            let tool = tools
                .as_array()
                .expect("a list")
                .iter()
                .find(|t| t["name"] == "propose_table_change")
                .expect("the tool is offered");
            let description = tool["description"].as_str().expect("a description");
            assert!(
                description.contains(schemaic_core::propose::FENCE_TAG),
                "{engine:?} doesn't name the tag the app extracts on"
            );
        }
    }

    /// What `run_query` advertises is what the gate enforces, per engine. The
    /// description used to be a fixed `SELECT/SHOW/DESCRIBE/EXPLAIN/WITH`, so a
    /// SQLite connection told the model it could `SHOW TABLES` — a statement
    /// SQLite has no syntax for, and the model had only a parser error to learn
    /// from. This walks the advertised heads back through the gate.
    #[test]
    fn run_query_advertises_exactly_the_heads_its_gate_allows() {
        for engine in [
            schemaic_db::Engine::MySql,
            schemaic_db::Engine::Postgres,
            schemaic_db::Engine::Sqlite,
        ] {
            let dialect = engine.dialect();
            let tools = super::tools_list(engine, true, true);
            let desc = tools[0]["description"].as_str().expect("a description");
            assert_eq!(tools[0]["name"], "run_query");
            assert!(
                desc.contains(dialect.engine_label()),
                "{engine:?} is not named: {desc}"
            );
            // Every head it names is one the gate accepts…
            for head in schemaic_core::sql::read_only_heads(dialect) {
                assert!(desc.contains(head), "{engine:?} omits {head}: {desc}");
                let sql = if *head == "WITH" {
                    "WITH c AS (SELECT 1) SELECT * FROM c".to_string()
                } else {
                    format!("{head} t")
                };
                assert!(
                    schemaic_core::sql::read_only_reason(&sql, dialect).is_ok(),
                    "{engine:?} advertises {head} but the gate refuses it"
                );
            }
            // …and it names no head the gate would refuse.
            for head in ["SELECT", "SHOW", "DESCRIBE", "EXPLAIN", "WITH"] {
                if desc.contains(head) {
                    assert!(
                        schemaic_core::sql::read_only_heads(dialect).contains(&head),
                        "{engine:?} advertises {head}, which its gate rejects: {desc}"
                    );
                }
            }
        }
    }

    /// Every engine reaches its **own** dialect. This was an
    /// `if Postgres { … } else { MySql }`, which compiles cleanly while sorting a
    /// third engine onto whichever side it happens to fall — so a SQLite
    /// connection had the read-only gate lexed by MySQL's rules and its sample
    /// query quoted against MySQL's reserved words.
    #[test]
    fn the_dialect_is_the_engines_own_for_every_engine() {
        let db = |engine| {
            schemaic_db::Db::from_parts(
                engine,
                String::new(),
                0,
                String::new(),
                String::new(),
                String::new(),
            )
        };
        for (engine, want) in [
            (schemaic_db::Engine::MySql, SqlDialect::MySql),
            (schemaic_db::Engine::Postgres, SqlDialect::Postgres),
            (schemaic_db::Engine::Sqlite, SqlDialect::Sqlite),
        ] {
            assert_eq!(dialect_of(&db(engine)), want, "{engine:?}");
        }
    }
    use schemaic_core::intel::SqlDialect;
    use schemaic_core::model::{Column, ResultSet, Value};
    use schemaic_core::schema::{ColumnInfo, DbSchema, ForeignKeyInfo, IndexInfo, TableInfo};

    fn tbl(name: &str, columns: Vec<ColumnInfo>) -> TableInfo {
        TableInfo {
            name: name.to_string(),
            columns,
            ..Default::default()
        }
    }

    fn c(name: &str, type_name: &str, nullable: bool, primary_key: bool) -> ColumnInfo {
        ColumnInfo {
            name: name.to_string(),
            type_name: type_name.to_string(),
            nullable,
            primary_key,
            ..Default::default()
        }
    }

    fn fk(columns: &[&str], ref_table: &str, ref_columns: &[&str]) -> ForeignKeyInfo {
        ForeignKeyInfo {
            name: format!("fk_{ref_table}"),
            columns: columns.iter().map(|s| s.to_string()).collect(),
            ref_schema: None,
            ref_table: ref_table.to_string(),
            ref_columns: ref_columns.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
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
                ..Default::default()
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
    fn the_overview_leaves_out_a_database_the_user_hid() {
        let names = || {
            vec![
                "sakila".to_string(),
                "archive".to_string(),
                "shop".to_string(),
            ]
        };
        let hidden: HashSet<String> = ["archive".to_string()].into_iter().collect();
        assert_eq!(
            listed_databases(names(), &hidden, Some("sakila")),
            vec!["sakila".to_string(), "shop".to_string()]
        );
        // Nothing hidden → the server's own list, in the server's own order.
        assert_eq!(listed_databases(names(), &HashSet::new(), None), names());
    }

    #[test]
    fn the_session_still_sees_the_database_it_is_pointed_at() {
        // The exception every other surface makes: hiding the database the user
        // is working in doesn't move the tab, and an assistant that couldn't
        // name the database its own queries default to would be useless there.
        let hidden: HashSet<String> = ["archive".to_string()].into_iter().collect();
        assert_eq!(
            listed_databases(vec!["archive".to_string()], &hidden, Some("archive")),
            vec!["archive".to_string()]
        );
        assert!(listed_databases(vec!["archive".to_string()], &hidden, Some("shop")).is_empty());
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
            ..Default::default()
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
        let out = format_database_schema(
            "shop",
            &DbSchema {
                tables: vec![t],
                ..Default::default()
            },
        );
        assert!(out.contains("  FK: (order_id, line_no) -> shipments(ord, line)"));
    }

    #[test]
    fn database_schema_marks_a_view() {
        let mut view = tbl("v_active", vec![c("id", "int", true, false)]);
        view.is_view = true;
        let out = format_database_schema(
            "shop",
            &DbSchema {
                tables: vec![view],
                ..Default::default()
            },
        );
        assert!(out.contains("### v_active (view)"));
    }

    #[test]
    fn database_schema_reports_an_empty_database() {
        let out = format_database_schema("shop", &DbSchema::default());
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
            foreign: true,
            ..IndexInfo::plain("ix_customer", vec!["customer_id"], false)
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
            ..Default::default()
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
            ..Default::default()
        };
        let out = format_database_schema("shop", &pg);
        assert!(out.contains("### orders"));
        assert!(!out.contains("public."));

        let mysql = DbSchema {
            tables: vec![tbl("orders", vec![c("id", "int", false, true)])],
            ..Default::default()
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
            ..Default::default()
        }];
        let schema = DbSchema {
            tables: vec![
                child,
                in_schema(tbl("orders", vec![c("id", "int", false, true)]), "public"),
            ],
            ..Default::default()
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
            ..Default::default()
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

    #[test]
    fn a_supported_protocol_is_agreed_to() {
        assert_eq!(negotiate_protocol(Some("2024-11-05")), "2024-11-05");
    }

    #[test]
    fn an_unknown_protocol_gets_one_we_really_speak() {
        // The handshake exists to catch a skew. Echoing the request back claims
        // fluency in a protocol whose tool-result shape and error convention we
        // don't implement, and the failure that follows looks like empty results
        // rather than a version mismatch.
        assert_eq!(
            negotiate_protocol(Some("2099-01-01")),
            SUPPORTED_PROTOCOLS[0]
        );
        assert_eq!(negotiate_protocol(Some("")), SUPPORTED_PROTOCOLS[0]);
        // A client that names no version gets the same answer.
        assert_eq!(negotiate_protocol(None), SUPPORTED_PROTOCOLS[0]);
    }

    #[test]
    fn the_supported_list_is_newest_first_and_never_empty() {
        // `SUPPORTED_PROTOCOLS[0]` is what an unknown request is answered with,
        // so the ordering is load-bearing rather than cosmetic.
        assert!(!SUPPORTED_PROTOCOLS.is_empty());
        let mut sorted = SUPPORTED_PROTOCOLS.to_vec();
        sorted.sort_unstable_by(|a, b| b.cmp(a));
        assert_eq!(sorted, SUPPORTED_PROTOCOLS);
    }
}
