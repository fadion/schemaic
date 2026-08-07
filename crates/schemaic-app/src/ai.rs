//! The AI-panel machinery: the live `claude` streaming session (`AiSession` +
//! `start_ai_session`, which spawns the CLI child and streams transcript snapshots
//! over a channel), the per-session MCP config plumbing (the DB endpoint written
//! to a temp file so credentials stay off the command line — review C6), the
//! system-prompt context builder (`ai_context`), the per-turn context refresh
//! (`TurnContext` / `apply_turn_delta` — the system prompt is written once at
//! spawn, so what moves afterwards rides along with each user turn), and the
//! inline-AI (Ctrl+K) helpers (`inline_system_prompt` / `extract_sql`). These are
//! free functions and plain types — the reactive wiring that drives them lives in
//! `app_view`.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};

use floem::reactive::{RwSignal, SignalGet, SignalWith};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use schemaic_core::connection::Connection;
use schemaic_core::schema::{DbSchema, SchemaState};
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
    /// The live context (active database / schema outline / editor contents) as
    /// the assistant last saw it — seeded from the system prompt at spawn, then
    /// advanced on every turn. The system prompt is written once, so without
    /// this the assistant answers later turns against the state from the first
    /// question.
    pub(crate) last_context: TurnContext,
    /// The database the MCP subprocess was spawned against. Fixed for the life
    /// of the session (it rides in the config file `claude` was launched with),
    /// so the turn delta has to warn when the user switches away from it.
    pub(crate) mcp_database: Option<String>,
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
    endpoint_from_value(&v)
}

/// Parse a DB endpoint from the MCP-config JSON value: host defaults to
/// `127.0.0.1`, port to `3306`, user/pass to empty, database optional. Pure so
/// the defaulting is unit-tested without touching the environment.
fn endpoint_from_value(v: &serde_json::Value) -> (Db, Option<String>) {
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
    // Engine tag (default MySQL for back-compat with older endpoint blobs) so the
    // MCP subprocess talks the right driver to the DB.
    let engine = schemaic_db::Engine::from_db_type(
        v.get("engine").and_then(|x| x.as_str()).unwrap_or("mysql"),
    );
    (Db::from_parts(engine, host, port, user, pass), database)
}

/// Serialize a DB endpoint (host/port/user/pass + default database) as the JSON
/// blob handed to the MCP subprocess via its environment.
fn endpoint_json(db: &Db, database: Option<&str>) -> String {
    let (host, port, user, pass) = db.parts();
    serde_json::json!({
        "host": host, "port": port, "user": user, "pass": pass,
        "database": database, "engine": db.engine().as_str()
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
    let cfg = mcp_config_json(&exe, endpoint);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("schemaic-mcp-{}-{n}.json", std::process::id()));
    write_private(&path, cfg.as_bytes()).ok()?;
    Some(path)
}

/// The `claude` MCP config JSON launching `exe --mcp-serve` with the DB endpoint
/// in its `env` (so credentials stay off the command line — review C6). Pure so
/// the config shape is unit-tested.
fn mcp_config_json(exe: &str, endpoint: &str) -> String {
    serde_json::json!({
        "mcpServers": {
            "schemaic": {
                "command": exe,
                "args": ["--mcp-serve"],
                "env": { "SCHEMAIC_MCP_ENDPOINT": endpoint }
            }
        }
    })
    .to_string()
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
            // Capture stderr (was discarded): a failing `claude` — e.g. an expired
            // OAuth session — writes its reason here or to stdout, and we need it to
            // surface a real error instead of an empty response.
            .stderr(Stdio::piped())
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

        // Drain stderr concurrently into a shared buffer so it's available if the
        // session dies (reading it only on exit could deadlock a full pipe).
        let stderr_buf = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        if let Some(se) = child.stderr.take() {
            let buf = stderr_buf.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(se).lines();
                while let Ok(Some(l)) = lines.next_line().await {
                    if let Ok(mut b) = buf.lock() {
                        b.push_str(&l);
                        b.push('\n');
                    }
                }
            });
        }
        // Plain-text stdout lines that aren't stream-json (e.g. a fatal error the
        // CLI prints before exiting) — kept as a fallback diagnostic.
        let mut raw_output: Vec<String> = Vec::new();

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
                        let events = schemaic_ai::parse_stream_line(&l);
                        // A non-blank line that yields no events AND isn't valid JSON
                        // is a plain-text diagnostic (e.g. the auth error) — keep it.
                        if events.is_empty()
                            && !l.trim().is_empty()
                            && serde_json::from_str::<serde_json::Value>(l.trim()).is_err()
                        {
                            raw_output.push(l.trim().to_string());
                        }
                        let mut changed = false;
                        let mut done: Option<(bool, schemaic_core::transcript::TurnStats)> = None;
                        for ev in events {
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
                            raw_output.clear(); // a clean turn boundary — drop stale diagnostics
                        } else if changed {
                            let _ = ai_tx.send(AiStreamMsg {
                                segs: turn.segments(),
                                done: false,
                                is_error: false,
                                stats: None,
                            });
                        }
                    }
                    // stdout closed → `claude` exited on its own (crash / auth failure
                    // / etc.), not a normal turn end. Surface WHY instead of returning
                    // an empty response: prefer stderr, then the plain-text stdout it
                    // printed, then the exit status.
                    _ => {
                        let code = child.wait().await.ok().and_then(|s| s.code());
                        let stderr_text = stderr_buf.lock().map(|b| b.clone()).unwrap_or_default();
                        let raw = raw_output.join("\n");
                        if code != Some(0) || !stderr_text.trim().is_empty() || !raw.trim().is_empty()
                        {
                            let why = schemaic_ai::cli_failure_message(code, &raw, &stderr_text);
                            let _ = ai_tx.send(AiStreamMsg {
                                segs: vec![schemaic_core::transcript::Seg::Text(format!(
                                    "The AI session ended unexpectedly: {why}"
                                ))],
                                done: true,
                                is_error: true,
                                stats: None,
                            });
                        }
                        break;
                    }
                },
            }
        }
        let _ = child.kill().await;
    });

    (tx, mcp_cfg)
}

/// The parts of the AI's context that change *while a session is alive* — the
/// active database, the schema outline (a database's tables land here when
/// introspection finishes), and the query editor's contents.
///
/// The system prompt is written once, when the `claude` child is spawned, so
/// without this the assistant answers every later turn against the state from
/// the first question. [`render_turn_delta`] diffs two snapshots into a small
/// block prepended to the user's turn.
#[derive(Clone, Default, PartialEq)]
pub(crate) struct TurnContext {
    pub(crate) active_db: Option<String>,
    pub(crate) outline: String,
    pub(crate) query: String,
}

/// Render the context block prepended to a user turn: only the parts that
/// changed since `prev`, or `None` when nothing did (the common case — no
/// tokens spent re-stating what the model already knows).
///
/// An outline that is empty in both snapshots is never reported: that's
/// `SchemaScope::None`, where the system prompt promised no schema section at
/// all.
///
/// `mcp_database` is the database the MCP subprocess was spawned against. It's
/// fixed for the life of the session, so once the user switches away the block
/// says so — otherwise `run_query` would silently resolve the assistant's
/// unqualified table names against the old database.
fn render_turn_delta(
    prev: &TurnContext,
    cur: &TurnContext,
    mcp_database: Option<&str>,
) -> Option<String> {
    if prev == cur {
        return None;
    }
    let mut out = String::from(
        "[Schemaic context update — this supersedes the matching section of your \
         system prompt.]\n",
    );
    if prev.active_db != cur.active_db {
        out.push_str(&format!(
            "Active database: {}\n",
            cur.active_db.as_deref().unwrap_or("(none)")
        ));
        if cur.active_db.is_some() && cur.active_db.as_deref() != mcp_database {
            let pinned = mcp_database.unwrap_or("the connection default");
            out.push_str(&format!(
                "Note: the run_query tool still runs against {pinned} — qualify table \
                 names (db.table) to reach another database.\n"
            ));
        }
    }
    if prev.outline != cur.outline && !cur.outline.is_empty() {
        out.push_str(&format!("Databases and tables:\n{}", cur.outline));
    }
    if prev.query != cur.query {
        out.push_str(&format!(
            "Current query editor:\n```sql\n{}\n```\n",
            cur.query
        ));
    }
    Some(out)
}

/// Prepend the context delta (if any) to a user turn, so the model reads the
/// refreshed context before the question it applies to.
pub(crate) fn apply_turn_delta(
    prev: &TurnContext,
    cur: &TurnContext,
    mcp_database: Option<&str>,
    msg: &str,
) -> String {
    match render_turn_delta(prev, cur, mcp_database) {
        Some(block) => format!("{block}\n{msg}"),
        None => msg.to_string(),
    }
}

/// Bundled inputs for [`ai_context`] (keeps the argument count in check).
#[derive(Clone, Copy)]
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
    let conn_name = p
        .connections
        .with_untracked(|cs| {
            cs.iter()
                .find(|c| c.id == p.active_conn.get_untracked())
                .map(|c| c.name.clone())
        })
        .unwrap_or_else(|| "(none)".to_string());
    render_ai_context(
        &conn_name,
        &turn_context(p),
        p.scope,
        p.run_queries,
        instructions,
    )
}

/// Snapshot the live parts of the AI's context (active database, schema outline,
/// editor contents) for the active tab. Taken before every user turn so
/// [`apply_turn_delta`] can report what moved since the session started.
pub(crate) fn turn_context(p: AiContextParams) -> TurnContext {
    let AiContextParams {
        db_nodes,
        tabs,
        active,
        scope,
        ..
    } = p;
    let tab = |f: &dyn Fn(&Tab) -> Option<String>| {
        tabs.with_untracked(|v| {
            v.iter()
                .find(|t| t.id == active.get_untracked())
                .and_then(f)
        })
    };
    let active_db = tab(&|t| t.database.get_untracked());
    let query = tab(&|t| Some(t.query.get_untracked())).unwrap_or_default();
    let databases = snapshot_databases(db_nodes);
    TurnContext {
        outline: render_schema_outline(&databases, active_db.as_deref(), scope),
        active_db,
        query,
    }
}

/// The `- database: table, table` outline, filtered per the scope setting.
/// Shared by the system prompt and the per-turn delta so the two can never
/// disagree about what the assistant has been told.
fn render_schema_outline(
    databases: &[(String, Option<DbSchema>)],
    active_db: Option<&str>,
    scope: SchemaScope,
) -> String {
    let mut outline = String::new();
    if scope == SchemaScope::None {
        return outline;
    }
    for (database, schema) in databases {
        if scope == SchemaScope::Active && Some(database.as_str()) != active_db {
            continue;
        }
        match schema {
            Some(s) => {
                // Qualified outside PostgreSQL's `public` — the assistant has to
                // be able to name the table it's told about.
                let tables: Vec<String> = s
                    .tables
                    .iter()
                    .map(|t| schemaic_core::schema::display_name(t.schema.as_deref(), &t.name))
                    .collect();
                outline.push_str(&format!("- {}: {}\n", database, tables.join(", ")));
            }
            None => outline.push_str(&format!("- {database}\n")),
        }
    }
    outline
}

/// Snapshot each schema-tree node into plain data: `(database, Some(schema))`
/// when introspection has loaded, `(database, None)` while it's still pending.
/// Reads the signals once so the prompt builders below can stay pure.
fn snapshot_databases(db_nodes: RwSignal<Vec<ConnNode>>) -> Vec<(String, Option<DbSchema>)> {
    db_nodes.with_untracked(|v| {
        v.iter()
            .map(|n| {
                let schema = match n.schema.get_untracked() {
                    SchemaState::Loaded(s) => Some(s),
                    _ => None,
                };
                (n.database.clone(), schema)
            })
            .collect()
    })
}

/// Pure core of [`ai_context`]: assemble the AI-panel system prompt from an
/// already-snapshotted connection name, [`TurnContext`], scope, and run-queries
/// flag. No signals — so the prompt shape (tools line, schema section) is
/// unit-tested. Every live section it writes is one [`render_turn_delta`] can
/// supersede later in the session.
fn render_ai_context(
    conn_name: &str,
    cx: &TurnContext,
    scope: SchemaScope,
    run_queries: bool,
    instructions: &str,
) -> String {
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
        format!("Databases and tables:\n{}\n", cx.outline)
    };
    let current = &cx.query;

    let mut out = format!(
        "You are a SQL assistant embedded in Schemaic, a native MySQL/MariaDB editor. \
         Help the user write, fix, and understand SQL. Be concise and return runnable \
         SQL in fenced code blocks. {tools_line}\n\n\
         Active connection: {conn_name}\n\
         Active database: {active_db}\n\
         {schema_section}\
         Current query editor:\n```sql\n{current}\n```",
        active_db = cx.active_db.as_deref().unwrap_or("(none)"),
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
    let databases = snapshot_databases(db_nodes);
    render_inline_prompt(&databases, active_db, req)
}

/// Pure core of [`inline_system_prompt`]: build the Ctrl+K generator prompt from
/// snapshotted per-database schema. Columns are spelled out only for tables the
/// request plausibly touches (in `active_db`, or named in the buffer/intent);
/// every table is still listed by name. No signals — so the column-inclusion
/// heuristic and the selection-vs-insert task line are unit-tested.
fn render_inline_prompt(
    databases: &[(String, Option<DbSchema>)],
    active_db: Option<&str>,
    req: &InlineAiRequest,
) -> String {
    let haystack = format!("{} {}", req.current_sql, req.intent).to_lowercase();
    let mut outline = String::new();
    for (database, schema) in databases {
        match schema {
            Some(s) => {
                outline.push_str(&format!("{database}:\n"));
                let full_db = active_db == Some(database.as_str());
                for t in &s.tables {
                    let name = schemaic_core::schema::display_name(t.schema.as_deref(), &t.name);
                    // Match on the bare name: a buffer saying `orders` should pull in
                    // `sales.orders`'s columns too.
                    if full_db || haystack.contains(&t.name.to_lowercase()) {
                        let cols: Vec<&str> = t.columns.iter().map(|c| c.name.as_str()).collect();
                        outline.push_str(&format!("  {name}({})\n", cols.join(", ")));
                    } else {
                        outline.push_str(&format!("  {name}\n"));
                    }
                }
            }
            None => outline.push_str(&format!("{database}\n")),
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_sql_returns_bare_text_unchanged() {
        assert_eq!(extract_sql("SELECT 1"), "SELECT 1");
        assert_eq!(extract_sql("  SELECT 1  "), "SELECT 1");
    }

    #[test]
    fn extract_sql_strips_fenced_block_with_sql_tag() {
        assert_eq!(extract_sql("```sql\nSELECT 1\n```"), "SELECT 1");
        // No language tag.
        assert_eq!(extract_sql("```\nSELECT 2\n```"), "SELECT 2");
        // Leading/trailing prose whitespace around the fence.
        assert_eq!(extract_sql("  ```sql\nSELECT 3\n```  "), "SELECT 3");
    }

    #[test]
    fn extract_sql_handles_unclosed_fence() {
        // No closing fence → take everything after the opening fence + tag.
        assert_eq!(extract_sql("```sql\nSELECT 4"), "SELECT 4");
    }

    #[test]
    fn endpoint_json_serializes_parts_and_database() {
        let db = Db::from_parts(
            schemaic_db::Engine::Postgres,
            "h".into(),
            3307,
            "u".into(),
            "p".into(),
        );
        let out = endpoint_json(&db, Some("shop"));
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["host"], "h");
        assert_eq!(v["port"], 3307);
        assert_eq!(v["user"], "u");
        assert_eq!(v["pass"], "p");
        assert_eq!(v["database"], "shop");
        assert_eq!(v["engine"], "postgres"); // engine tag serialized
        // No default database → JSON null.
        let out = endpoint_json(&db, None);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(v["database"].is_null());
    }

    #[test]
    fn endpoint_json_roundtrips_through_value_parser() {
        // endpoint_json → endpoint_from_value reconstructs the same endpoint
        // (incl. engine), with no environment access.
        let db = Db::from_parts(
            schemaic_db::Engine::Postgres,
            "host".into(),
            3306,
            "user".into(),
            "pw".into(),
        );
        let json = endpoint_json(&db, Some("db1"));
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let (parsed, database) = endpoint_from_value(&v);
        assert_eq!(parsed.parts(), ("host", 3306, "user", "pw"));
        assert_eq!(parsed.engine(), schemaic_db::Engine::Postgres);
        assert_eq!(database.as_deref(), Some("db1"));
    }

    #[test]
    fn endpoint_from_value_fills_defaults() {
        // Empty/Null object → local defaults, no database, MySQL engine.
        let (db, database) = endpoint_from_value(&serde_json::Value::Null);
        assert_eq!(db.parts(), ("127.0.0.1", 3306, "", ""));
        assert_eq!(db.engine(), schemaic_db::Engine::MySql);
        assert!(database.is_none());
        // Partial object → only the missing keys default.
        let v = serde_json::json!({ "host": "h", "user": "u" });
        let (db, database) = endpoint_from_value(&v);
        assert_eq!(db.parts(), ("h", 3306, "u", ""));
        assert!(database.is_none());
    }

    #[test]
    fn mcp_config_json_shape() {
        let v: serde_json::Value =
            serde_json::from_str(&mcp_config_json("/path/schemaic", "ENDPOINT_BLOB")).unwrap();
        let server = &v["mcpServers"]["schemaic"];
        assert_eq!(server["command"], "/path/schemaic");
        assert_eq!(server["args"][0], "--mcp-serve");
        assert_eq!(server["env"]["SCHEMAIC_MCP_ENDPOINT"], "ENDPOINT_BLOB");
    }

    use schemaic_core::schema::{ColumnInfo, IndexInfo, TableInfo};

    fn table(name: &str, cols: &[&str]) -> TableInfo {
        TableInfo {
            schema: None,
            name: name.to_string(),
            columns: cols
                .iter()
                .map(|c| ColumnInfo {
                    name: c.to_string(),
                    type_name: "int".to_string(),
                    nullable: true,
                    primary_key: false,
                })
                .collect(),
            indexes: Vec::<IndexInfo>::new(),
            foreign_keys: Vec::new(),
            is_view: false,
            view_definition: None,
        }
    }

    fn schema(tables: Vec<TableInfo>) -> DbSchema {
        DbSchema { tables }
    }

    /// Build the system-prompt context the way `turn_context` would, but from
    /// plain snapshotted data (no signals).
    fn ctx_of(
        dbs: &[(String, Option<DbSchema>)],
        active_db: Option<&str>,
        query: &str,
        scope: SchemaScope,
    ) -> TurnContext {
        TurnContext {
            outline: render_schema_outline(dbs, active_db, scope),
            active_db: active_db.map(str::to_string),
            query: query.to_string(),
        }
    }

    #[test]
    fn render_ai_context_active_scope_lists_only_active_db() {
        let dbs = vec![
            (
                "shop".to_string(),
                Some(schema(vec![table("orders", &["id"])])),
            ),
            (
                "blog".to_string(),
                Some(schema(vec![table("posts", &["id"])])),
            ),
        ];
        let cx = ctx_of(&dbs, Some("shop"), "SELECT 1", SchemaScope::Active);
        let out = render_ai_context("Local", &cx, SchemaScope::Active, true, "");
        assert!(out.contains("Active connection: Local"));
        assert!(out.contains("Active database: shop"));
        assert!(out.contains("- shop: orders"));
        assert!(!out.contains("blog")); // Active scope drops non-active dbs
        // run_queries = true → mentions run_query.
        assert!(out.contains("run_query"));
        assert!(out.contains("```sql\nSELECT 1\n```"));
    }

    #[test]
    fn render_ai_context_all_scope_lists_every_db_and_unloaded_shows_bare() {
        let dbs = vec![
            (
                "shop".to_string(),
                Some(schema(vec![table("orders", &["id"])])),
            ),
            ("blog".to_string(), None), // schema not loaded yet
        ];
        let cx = ctx_of(&dbs, Some("shop"), "", SchemaScope::All);
        let out = render_ai_context("Local", &cx, SchemaScope::All, false, "");
        assert!(out.contains("- shop: orders"));
        assert!(out.contains("- blog\n")); // unloaded → name only, no ": tables"
        // run_queries = false → the no-queries tools line.
        assert!(out.contains("cannot run"));
        assert!(!out.contains("with the run_query"));
    }

    #[test]
    fn render_ai_context_none_scope_omits_schema_and_appends_instructions() {
        let dbs = vec![(
            "shop".to_string(),
            Some(schema(vec![table("orders", &["id"])])),
        )];
        let cx = ctx_of(&dbs, Some("shop"), "", SchemaScope::None);
        let out = render_ai_context("Local", &cx, SchemaScope::None, true, "  Prefer CTEs.  ");
        assert!(!out.contains("Databases and tables:"));
        assert!(!out.contains("orders"));
        // Instructions are trimmed and appended.
        assert!(out.contains("Additional instructions from the user:\nPrefer CTEs."));
    }

    fn req(intent: &str, current: &str, selection: Option<&str>) -> InlineAiRequest {
        InlineAiRequest {
            intent: intent.to_string(),
            current_sql: current.to_string(),
            selection: selection.map(str::to_string),
        }
    }

    #[test]
    fn render_inline_prompt_expands_active_db_columns_others_by_mention() {
        let dbs = vec![(
            "shop".to_string(),
            Some(schema(vec![
                table("orders", &["id", "total"]),
                table("audit", &["id"]),
            ])),
        )];
        // active_db = shop → every table in shop gets columns.
        let out = render_inline_prompt(&dbs, Some("shop"), &req("count orders", "SELECT 1", None));
        assert!(out.contains("orders(id, total)"));
        assert!(out.contains("audit(id)"));
        assert!(out.contains("to be inserted at the cursor"));
    }

    #[test]
    fn render_inline_prompt_lists_bare_table_unless_mentioned() {
        let dbs = vec![(
            "blog".to_string(),
            Some(schema(vec![
                table("posts", &["id", "body"]),
                table("tags", &["id"]),
            ])),
        )];
        // active_db = shop (not blog) → only tables named in the request get columns.
        let out = render_inline_prompt(
            &dbs,
            Some("shop"),
            &req("update posts", "SELECT * FROM posts", None),
        );
        assert!(out.contains("posts(id, body)")); // mentioned → columns
        assert!(out.contains("  tags\n")); // not mentioned → bare name
        assert!(!out.contains("tags(")); // no columns for the unmentioned table
    }

    fn cx(active_db: Option<&str>, outline: &str, query: &str) -> TurnContext {
        TurnContext {
            active_db: active_db.map(str::to_string),
            outline: outline.to_string(),
            query: query.to_string(),
        }
    }

    #[test]
    fn turn_delta_is_none_when_nothing_changed() {
        let c = cx(Some("shop"), "- shop: orders\n", "SELECT 1");
        assert_eq!(render_turn_delta(&c, &c, Some("shop")), None);
    }

    #[test]
    fn turn_delta_reports_only_the_changed_query() {
        let prev = cx(Some("shop"), "- shop: orders\n", "SELECT 1");
        let cur = cx(Some("shop"), "- shop: orders\n", "SELECT 2");
        let out = render_turn_delta(&prev, &cur, Some("shop")).expect("query changed");
        assert!(out.contains("```sql\nSELECT 2\n```"));
        // Unchanged parts are not re-sent.
        assert!(!out.contains("Active database:"));
        assert!(!out.contains("Databases and tables:"));
    }

    #[test]
    fn turn_delta_reports_active_database_change() {
        let prev = cx(Some("shop"), "- shop: orders\n", "SELECT 1");
        let cur = cx(Some("blog"), "- shop: orders\n", "SELECT 1");
        let out = render_turn_delta(&prev, &cur, Some("shop")).expect("database changed");
        assert!(out.contains("Active database: blog"));
        assert!(!out.contains("Current query editor:"));
    }

    #[test]
    fn turn_delta_reports_schema_outline_change() {
        // A schema finishing introspection changes the outline even though the
        // active database and editor are untouched.
        let prev = cx(Some("shop"), "- shop\n", "SELECT 1");
        let cur = cx(Some("shop"), "- shop: orders, customers\n", "SELECT 1");
        let out = render_turn_delta(&prev, &cur, Some("shop")).expect("outline changed");
        assert!(out.contains("Databases and tables:\n- shop: orders, customers"));
        assert!(!out.contains("Active database:"));
    }

    #[test]
    fn turn_delta_reports_a_cleared_editor_and_dropped_database() {
        let prev = cx(Some("shop"), "- shop: orders\n", "SELECT 1");
        let cur = cx(None, "- shop: orders\n", "");
        let out = render_turn_delta(&prev, &cur, Some("shop")).expect("db and query changed");
        assert!(out.contains("Active database: (none)"));
        assert!(out.contains("```sql\n\n```"));
    }

    #[test]
    fn turn_delta_warns_when_the_active_db_drifts_from_the_mcp_default() {
        // `run_query` is pinned to the database the session was spawned with, so
        // once the user switches the assistant must qualify its table names.
        let prev = cx(Some("shop"), "", "SELECT 1");
        let cur = cx(Some("blog"), "", "SELECT 1");
        let out = render_turn_delta(&prev, &cur, Some("shop")).expect("database changed");
        assert!(out.contains("Active database: blog"));
        assert!(out.contains("run_query"));
        assert!(out.contains("shop"));
    }

    #[test]
    fn turn_delta_has_no_tool_warning_while_the_active_db_matches() {
        let prev = cx(Some("shop"), "", "SELECT 1");
        let cur = cx(Some("shop"), "", "SELECT 2");
        let out = render_turn_delta(&prev, &cur, Some("shop")).expect("query changed");
        assert!(!out.contains("run_query"));
    }

    #[test]
    fn apply_turn_delta_prepends_the_block_and_passes_a_clean_turn_through() {
        let prev = cx(Some("shop"), "", "SELECT 1");
        let cur = cx(Some("shop"), "", "SELECT 2");
        let out = apply_turn_delta(&prev, &cur, Some("shop"), "why is this slow?");
        assert!(out.starts_with("[Schemaic context update"));
        assert!(out.ends_with("why is this slow?"));
        // Nothing moved → the user's message is sent verbatim.
        assert_eq!(apply_turn_delta(&cur, &cur, Some("shop"), "hello"), "hello");
    }

    #[test]
    fn turn_delta_omits_the_outline_when_scope_is_none() {
        // SchemaScope::None yields an empty outline in both snapshots — nothing to
        // report, so an unchanged-empty outline never emits a header.
        let prev = cx(Some("shop"), "", "SELECT 1");
        let cur = cx(Some("shop"), "", "SELECT 2");
        let out = render_turn_delta(&prev, &cur, Some("shop")).expect("query changed");
        assert!(!out.contains("Databases and tables:"));
    }

    #[test]
    fn schema_outline_matches_the_scope() {
        let dbs = vec![
            (
                "shop".to_string(),
                Some(schema(vec![table("orders", &["id"])])),
            ),
            ("blog".to_string(), None),
        ];
        // Active → only the active database.
        let out = render_schema_outline(&dbs, Some("shop"), SchemaScope::Active);
        assert_eq!(out, "- shop: orders\n");
        // All → every database; an unloaded one is listed bare.
        let out = render_schema_outline(&dbs, Some("shop"), SchemaScope::All);
        assert_eq!(out, "- shop: orders\n- blog\n");
        // None → nothing at all.
        assert_eq!(
            render_schema_outline(&dbs, Some("shop"), SchemaScope::None),
            ""
        );
    }

    #[test]
    fn render_inline_prompt_selection_asks_for_rewrite() {
        let dbs: Vec<(String, Option<DbSchema>)> = vec![];
        let out = render_inline_prompt(
            &dbs,
            None,
            &req("uppercase", "SELECT a FROM t", Some("SELECT a FROM t")),
        );
        assert!(out.contains("The user selected this SQL to transform:"));
        assert!(out.contains("Rewrite ONLY that"));
    }
}
