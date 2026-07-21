//! The AI-panel machinery: the live `claude` streaming session (`AiSession` +
//! `start_ai_session`, which spawns the CLI child and streams transcript snapshots
//! over a channel), the per-session MCP config plumbing (the DB endpoint written
//! to a temp file so credentials stay off the command line — review C6), the
//! system-prompt context builder (`ai_context`), and the inline-AI (Ctrl+K)
//! helpers (`inline_system_prompt` / `extract_sql`). These are free functions and
//! plain types — the reactive wiring that drives them lives in `app_view`.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};

use floem::reactive::{RwSignal, SignalGet, SignalWith};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use schemaic_core::connection::Connection;
use schemaic_core::schema::SchemaState;
use schemaic_db::Db;
use schemaic_ui::{AiEffort, AiModel, ConnNode, InlineAiRequest, SchemaScope, Tab};

use crate::claude_cli::claude_bin;

// ===== moved from main.rs (AI session + context) =====
const AI_TOOLS_WITH_QUERY: &[&str] = &["mcp__schemaic__run_query", "mcp__schemaic__list_schema"];
const AI_TOOLS_READ_ONLY: &[&str] = &["mcp__schemaic__list_schema"];

/// A live AI conversation: the CLI child's stdin channel plus which connection
/// it's bound to. Dropping this (its `stdin_tx`) ends the session task, which
/// kills the child; the temp MCP-config file (if any) is removed on drop too.
pub(crate) struct AiSession {
    pub(crate) conn_id: u64,
    pub(crate) stdin_tx: tokio::sync::mpsc::UnboundedSender<String>,
    /// The per-session MCP config file (holds the DB endpoint out of the command
    /// line — review C6). Removed when the session ends.
    pub(crate) mcp_cfg: Option<PathBuf>,
    /// The AI settings this session was spawned with, so closing the settings
    /// modal only respawns `claude` when one actually changed (review §7.4).
    pub(crate) settings: AiSettings,
}

/// Snapshot of the AI settings that require respawning the `claude` session
/// (process args + the system context / MCP config sent at session start).
#[derive(Clone, PartialEq)]
pub(crate) struct AiSettings {
    pub(crate) model: AiModel,
    pub(crate) effort: AiEffort,
    pub(crate) run_queries: bool,
    pub(crate) cli_path: String,
    pub(crate) instructions: String,
    pub(crate) schema_scope: SchemaScope,
}

impl Drop for AiSession {
    fn drop(&mut self) {
        if let Some(p) = &self.mcp_cfg {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Parse the MCP DB endpoint from `$SCHEMAIC_MCP_ENDPOINT` (the JSON the app
/// writes into the MCP config file). Falls back to an empty local endpoint.
pub(crate) fn mcp_endpoint_from_env() -> (Db, Option<String>) {
    let v = std::env::var("SCHEMAIC_MCP_ENDPOINT")
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .unwrap_or(serde_json::Value::Null);
    let host = v
        .get("host")
        .and_then(|x| x.as_str())
        .unwrap_or("127.0.0.1")
        .to_string();
    let port = v.get("port").and_then(|x| x.as_u64()).unwrap_or(3306) as u16;
    let user = v
        .get("user")
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_string();
    let pass = v
        .get("pass")
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_string();
    let database = v
        .get("database")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    (Db::from_parts(host, port, user, pass), database)
}

/// Serialize a DB endpoint (host/port/user/pass + default database) as the JSON
/// blob handed to the MCP subprocess via its environment.
fn endpoint_json(db: &Db, database: Option<&str>) -> String {
    let (host, port, user, pass) = db.parts();
    serde_json::json!({
        "host": host, "port": port, "user": user, "pass": pass, "database": database
    })
    .to_string()
}

/// Write the `claude` MCP config to a per-session temp file and return its path.
/// The DB endpoint (with credentials) rides in the config's `env`, so it never
/// appears on a command line where another same-user process could read it
/// (review C6). Best-effort owner-only permissions; removed when the session
/// drops. Returns `None` if the file couldn't be written (caller then skips MCP).
fn write_mcp_config(endpoint: &str) -> Option<PathBuf> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "schemaic".to_string());
    let cfg = serde_json::json!({
        "mcpServers": {
            "schemaic": {
                "command": exe,
                "args": ["--mcp-serve"],
                "env": { "SCHEMAIC_MCP_ENDPOINT": endpoint }
            }
        }
    })
    .to_string();
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("schemaic-mcp-{}-{n}.json", std::process::id()));
    write_private(&path, cfg.as_bytes()).ok()?;
    Some(path)
}

/// Write `bytes` to `path`, owner-only where the platform supports it.
fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        f.write_all(bytes)
    }
    #[cfg(not(unix))]
    {
        // On Windows the user's temp dir is already ACL-scoped to the user; a
        // same-user process can read it, but that's no worse than the env var,
        // and strictly better than a command-line argument (review C6).
        let mut f = std::fs::File::create(path)?;
        f.write_all(bytes)
    }
}

/// A streamed transcript snapshot pushed from the reader task to the UI.
#[derive(Clone)]
pub(crate) struct AiStreamMsg {
    pub(crate) segs: Vec<schemaic_core::transcript::Seg>,
    pub(crate) done: bool,
    pub(crate) is_error: bool,
    /// Cost/usage summary; only populated on the final (done) snapshot.
    pub(crate) stats: Option<schemaic_core::transcript::TurnStats>,
}

/// Spawn a persistent streaming `claude` session for a connection. Returns the
/// stdin sender and the temp MCP-config path (removed when the session drops);
/// the reader task streams transcript snapshots over `ai_tx`.
/// Bundled inputs for [`start_ai_session`] (the runtime `handle` stays a separate
/// borrowed argument; everything else is owned and travels in here).
pub(crate) struct StartAiParams {
    pub system_context: String,
    pub db: Db,
    pub database: Option<String>,
    pub ai_tx: crossbeam_channel::Sender<AiStreamMsg>,
    pub model: String,
    pub effort: String,
    pub run_queries: bool,
    pub cli_path: String,
}

pub(crate) fn start_ai_session(
    handle: &tokio::runtime::Handle,
    p: StartAiParams,
) -> (tokio::sync::mpsc::UnboundedSender<String>, Option<PathBuf>) {
    let StartAiParams {
        system_context,
        db,
        database,
        ai_tx,
        model,
        effort,
        run_queries,
        cli_path,
    } = p;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    // MCP config: launch THIS binary in `--mcp-serve` mode, handing it the
    // (already-tunnelled) DB endpoint via env — written to a temp file so the
    // credentials never appear on a command line (review C6).
    let mcp_cfg = write_mcp_config(&endpoint_json(&db, database.as_deref()));
    let tools = if run_queries {
        AI_TOOLS_WITH_QUERY
    } else {
        AI_TOOLS_READ_ONLY
    };
    let mcp_cfg_arg = mcp_cfg.as_ref().map(|p| p.to_string_lossy().into_owned());
    let args = schemaic_ai::build_session_args(
        &system_context,
        Some(&model),
        Some(&effort),
        mcp_cfg_arg.as_deref(),
        tools,
    );

    handle.spawn(async move {
        let mut child = match Command::new(claude_bin(&cli_path))
            .args(&args)
            .current_dir(std::env::temp_dir())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                let _ = ai_tx.send(AiStreamMsg {
                    segs: vec![schemaic_core::transcript::Seg::Text(format!(
                        "Couldn't launch the `claude` CLI ({e}). Ensure Claude Code is \
                         installed (or set SCHEMAIC_CLAUDE_BIN)."
                    ))],
                    done: true,
                    is_error: true,
                    stats: None,
                });
                return;
            }
        };
        let mut stdin = child.stdin.take().expect("stdin piped");
        let mut reader = BufReader::new(child.stdout.take().expect("stdout piped")).lines();
        let mut turn = schemaic_ai::TurnState::default();

        loop {
            tokio::select! {
                maybe = rx.recv() => match maybe {
                    Some(line) => {
                        if stdin.write_all(line.as_bytes()).await.is_err() {
                            break;
                        }
                        let _ = stdin.flush().await;
                    }
                    None => break, // session dropped
                },
                line = reader.next_line() => match line {
                    Ok(Some(l)) => {
                        let mut changed = false;
                        let mut done: Option<(bool, schemaic_core::transcript::TurnStats)> = None;
                        for ev in schemaic_ai::parse_stream_line(&l) {
                            match ev {
                                schemaic_ai::StreamEvent::TurnDone { is_error, stats } => {
                                    done = Some((is_error, stats))
                                }
                                other => {
                                    turn.apply(&other);
                                    changed = true;
                                }
                            }
                        }
                        if let Some((is_error, stats)) = done {
                            let _ = ai_tx.send(AiStreamMsg {
                                segs: turn.segments(),
                                done: true,
                                is_error,
                                stats: (!stats.is_empty()).then_some(stats),
                            });
                            turn = schemaic_ai::TurnState::default();
                        } else if changed {
                            let _ = ai_tx.send(AiStreamMsg {
                                segs: turn.segments(),
                                done: false,
                                is_error: false,
                                stats: None,
                            });
                        }
                    }
                    _ => break,
                },
            }
        }
        let _ = child.kill().await;
    });

    (tx, mcp_cfg)
}

/// Bundled inputs for [`ai_context`] (keeps the argument count in check).
pub(crate) struct AiContextParams {
    pub connections: RwSignal<Vec<Connection>>,
    pub active_conn: RwSignal<u64>,
    pub db_nodes: RwSignal<Vec<ConnNode>>,
    pub tabs: RwSignal<Vec<Tab>>,
    pub active: RwSignal<usize>,
    pub scope: SchemaScope,
    pub run_queries: bool,
}

pub(crate) fn ai_context(p: AiContextParams, instructions: &str) -> String {
    let AiContextParams {
        connections,
        active_conn,
        db_nodes,
        tabs,
        active,
        scope,
        run_queries,
    } = p;
    let conn_name = connections
        .with_untracked(|cs| {
            cs.iter()
                .find(|c| c.id == active_conn.get_untracked())
                .map(|c| c.name.clone())
        })
        .unwrap_or_else(|| "(none)".to_string());

    // The active tab's database.
    let active_db = tabs.with_untracked(|v| {
        v.iter()
            .find(|t| t.id == active.get_untracked())
            .and_then(|t| t.database.get_untracked())
    });

    // Schema outline, scoped per the setting.
    let mut outline = String::new();
    if scope != SchemaScope::None {
        db_nodes.with_untracked(|v| {
            for n in v {
                if scope == SchemaScope::Active && Some(&n.database) != active_db.as_ref() {
                    continue;
                }
                match n.schema.get_untracked() {
                    SchemaState::Loaded(s) => {
                        let tables: Vec<&str> = s.tables.iter().map(|t| t.name.as_str()).collect();
                        outline.push_str(&format!("- {}: {}\n", n.database, tables.join(", ")));
                    }
                    _ => outline.push_str(&format!("- {}\n", n.database)),
                }
            }
        });
    }

    let current = tabs
        .with_untracked(|v| {
            v.iter()
                .find(|t| t.id == active.get_untracked())
                .map(|t| t.query.get_untracked())
        })
        .unwrap_or_default();

    // Tools line — kept truthful: the assistant always has `list_schema`, and
    // `run_query` only when the setting allows it.
    let tools_line = if run_queries {
        "You can inspect the live schema with the list_schema tool and run read-only \
         queries (a single SELECT/SHOW/DESCRIBE/EXPLAIN/WITH statement) with the run_query \
         tool. Use them when they help you answer."
    } else {
        "You can inspect the live schema with the list_schema tool, but you cannot run \
         queries — answer from the schema context and your knowledge."
    };
    let schema_section = if scope == SchemaScope::None {
        String::new()
    } else {
        format!("Databases and tables:\n{outline}\n")
    };

    let mut out = format!(
        "You are a SQL assistant embedded in Schemaic, a native MySQL/MariaDB editor. \
         Help the user write, fix, and understand SQL. Be concise and return runnable \
         SQL in fenced code blocks. {tools_line}\n\n\
         Active connection: {conn_name}\n\
         {schema_section}\
         Current query editor:\n```sql\n{current}\n```"
    );
    let instructions = instructions.trim();
    if !instructions.is_empty() {
        out.push_str(&format!(
            "\n\nAdditional instructions from the user:\n{instructions}"
        ));
    }
    out
}

/// Pull a bare SQL statement out of the assistant's reply, stripping a markdown
/// code fence if the model wrapped it despite instructions.
pub(crate) fn extract_sql(text: &str) -> String {
    let t = text.trim();
    if t.starts_with("```") {
        let after = t.trim_start_matches('`');
        let after = after.strip_prefix("sql").unwrap_or(after);
        let after = after.trim_start();
        let body = match after.rfind("```") {
            Some(idx) => &after[..idx],
            None => after,
        };
        return body.trim().to_string();
    }
    t.to_string()
}

/// System prompt for the inline (Ctrl+K) generator: a db→table(columns) outline
/// plus the current buffer, and (for a selection edit) the snippet to rewrite.
/// Demands bare SQL so the result can drop straight into the editor.
///
/// To keep the prompt small, columns are spelled out only for tables the request
/// plausibly touches — those in `active_db`, or whose name appears in the buffer
/// or intent. Every table is still listed by name so the model knows what exists.
pub(crate) fn inline_system_prompt(
    db_nodes: RwSignal<Vec<ConnNode>>,
    active_db: Option<&str>,
    req: &InlineAiRequest,
) -> String {
    let haystack = format!("{} {}", req.current_sql, req.intent).to_lowercase();
    let mut outline = String::new();
    db_nodes.with_untracked(|v| {
        for n in v {
            if let SchemaState::Loaded(s) = n.schema.get_untracked() {
                outline.push_str(&format!("{}:\n", n.database));
                let full_db = active_db == Some(n.database.as_str());
                for t in &s.tables {
                    if full_db || haystack.contains(&t.name.to_lowercase()) {
                        let cols: Vec<&str> = t.columns.iter().map(|c| c.name.as_str()).collect();
                        outline.push_str(&format!("  {}({})\n", t.name, cols.join(", ")));
                    } else {
                        outline.push_str(&format!("  {}\n", t.name));
                    }
                }
            } else {
                outline.push_str(&format!("{}\n", n.database));
            }
        }
    });
    let task = match &req.selection {
        Some(sel) => format!(
            "The user selected this SQL to transform:\n{sel}\n\nRewrite ONLY that \
             snippet per the request; output just the replacement SQL."
        ),
        None => "Write a SQL statement for the request, to be inserted at the cursor.".to_string(),
    };
    format!(
        "You are a SQL generator for MySQL/MariaDB inside the Schemaic editor. Output \
         ONLY SQL — no prose, no explanation, no markdown fences. Use only tables and \
         columns from the schema below.\n\n\
         Schema (database: table(columns)):\n{outline}\n\
         Current editor contents (for context):\n{current}\n\n{task}",
        current = req.current_sql,
    )
}
