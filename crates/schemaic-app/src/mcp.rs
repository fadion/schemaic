//! Built-in MCP (stdio) server for the AI panel.
//!
//! Launched as `schemaic --mcp-serve` by the `claude` CLI (configured via a
//! temp-file `--mcp-config`, so no credentials ride a command line — review C6).
//! Speaks newline-delimited JSON-RPC over stdin/stdout and exposes two read-only
//! tools against the DB endpoint passed in `$SCHEMAIC_MCP_ENDPOINT`:
//!   - `run_query`  — run a single read-only statement, return the rows.
//!   - `list_schema` — databases → tables → columns.
//!
//! The endpoint points at the app's active connection (already tunnelled for
//! SSH, since the tunnel is just a local listener any process can use), and is a
//! structured `schemaic_db::Db` handle — no credential URL is involved.

use schemaic_core::model::ResultSet;
use schemaic_db::Db;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio_util::sync::CancellationToken;

// Max rows returned to the AI/MCP client per query (a small preview — the
// model reasons over a sample, not the full result). Distinct from QUERY_ROW_CAP.
const MCP_ROW_CAP: usize = 200;

/// Run the stdio JSON-RPC loop until stdin closes. `db` is the resolved endpoint
/// and `database` the default schema (`USE`d for `run_query`).
pub async fn serve(db: Db, database: Option<String>) {
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
                Some(call_tool(&db, database.as_deref(), &name, &args).await)
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
            "description": "List the databases, tables, and columns available on the active connection.",
            "inputSchema": { "type": "object", "properties": {} }
        }
    ])
}

async fn call_tool(db: &Db, database: Option<&str>, name: &str, args: &Value) -> Value {
    let (text, is_error) = match name {
        "run_query" => {
            let sql = args.get("sql").and_then(|s| s.as_str()).unwrap_or("");
            run_query(db, database, sql).await
        }
        "list_schema" => list_schema(db).await,
        other => (format!("Unknown tool: {other}"), true),
    };
    json!({ "content": [ { "type": "text", "text": text } ], "isError": is_error })
}

/// Statement timeout for AI-issued queries — a backstop against `SLEEP()` /
/// heavy scans holding the connection open.
const QUERY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

async fn run_query(db: &Db, database: Option<&str>, sql: &str) -> (String, bool) {
    let Some(stmt) = normalize_stmt(sql) else {
        return ("Empty query.".to_string(), true);
    };
    // Read-only gate (comment/string/identifier-aware) lives in schemaic-core,
    // where it's unit-tested alongside the other SQL analysis.
    if let Err(reason) = schemaic_core::sql::read_only_reason(stmt) {
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
    for row in &rs.rows {
        let cells: Vec<String> = row
            .iter()
            .map(|v| {
                let s = v.display().replace('|', "\\|").replace('\n', " ");
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

async fn list_schema(db: &Db) -> (String, bool) {
    let dbs = match db.fetch_databases().await {
        Ok(d) => d,
        Err(e) => return (format!("Error listing databases: {e}"), true),
    };
    let mut out = String::new();
    for name in &dbs {
        out.push_str(&format!("## {name}\n"));
        match db.fetch_schema(name).await {
            Ok(schema) => {
                for t in &schema.tables {
                    let cols: Vec<String> = t
                        .columns
                        .iter()
                        .map(|c| format!("{} {}", c.name, c.type_name))
                        .collect();
                    out.push_str(&format!("- {}({})\n", t.name, cols.join(", ")));
                }
            }
            Err(e) => out.push_str(&format!("  (error: {e})\n")),
        }
    }
    (out, false)
}

#[cfg(test)]
mod tests {
    use super::{format_table, normalize_stmt};
    use schemaic_core::model::{Column, ResultSet, Value};

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
        let rs = ResultSet {
            columns: vec![col("id"), col("name")],
            rows: vec![
                vec![Value::Int(1), Value::Str("Ada".into())],
                vec![Value::Null, Value::Str("Bo".into())],
            ],
            ..Default::default()
        };
        let out = format_table(&rs);
        assert!(out.contains("| id | name |"));
        assert!(out.contains("| --- | --- |"));
        assert!(out.contains("| 1 | Ada |"));
        assert!(out.contains("| NULL | Bo |"));
        assert!(out.ends_with("(2 rows)"));
    }

    #[test]
    fn escapes_pipes_and_newlines_and_truncates_long_cells() {
        let rs = ResultSet {
            columns: vec![col("v")],
            rows: vec![
                vec![Value::Str("a|b\nc".into())],
                vec![Value::Str("x".repeat(80))],
            ],
            ..Default::default()
        };
        let out = format_table(&rs);
        // Pipe escaped, newline flattened to a space.
        assert!(out.contains(r"a\|b c"));
        // Long value truncated to 60 chars + ellipsis.
        assert!(out.contains(&format!("{}…", "x".repeat(60))));
        assert!(!out.contains(&"x".repeat(61)));
    }

    #[test]
    fn truncated_result_notes_capped_rows() {
        let rs = ResultSet {
            columns: vec![col("id")],
            rows: vec![vec![Value::Int(1)]],
            truncated: true,
            ..Default::default()
        };
        assert!(format_table(&rs).ends_with("(1+ rows, capped)"));
    }
}
