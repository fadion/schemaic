//! Schemaic — native SQL editor. Binary entry point.
//!
//! The app owns all the mutable state (tabs, saved connections, the loaded
//! schema) as signals in the root scope, plus the `Rc<dyn Fn>` callbacks the UI
//! invokes. A connection is a *server*; the schema sidebar lists all of the
//! active connection's databases. DB IO runs on the tokio runtime and results
//! are marshalled back through Floem's async→UI seam (ARCHITECTURE §5).

mod ai;
mod claude_cli;
mod mcp;

use ai::{
    AiContextParams, AiSession, AiSettings, AiStreamMsg, StartAiParams, ai_context, extract_sql,
    inline_system_prompt, mcp_endpoint_from_env, start_ai_session,
};
use claude_cli::{claude_bin, claude_reachable, detect_claude_bin};

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use floem::Application;
use floem::IntoView;
use floem::action::exec_after;
use floem::ext_event::create_ext_action;
use floem::ext_event::create_signal_from_channel;
use floem::kurbo::Size;
use floem::reactive::{
    RwSignal, Scope, SignalGet, SignalUpdate, SignalWith, create_effect, create_memo,
};
use floem::window::WindowConfig;
use schemaic_core::connection::{ConnStatus, Connection};
use schemaic_core::model::{CommitDone, GridWrite, QueryState, RefetchRequest};

/// Outcome of a background connect + schema-load task: `(tunnel port, tunnel
/// handle, database names)` on success, or an error message.
type ConnectResult =
    Result<(Option<u16>, Option<schemaic_db::ssh::TunnelHandle>, Vec<String>), String>;
/// Self-rescheduling cursor-blink tick — holds an `Rc` to itself so it can re-arm.
type BlinkTick = Rc<RefCell<Option<Rc<dyn Fn()>>>>;
use schemaic_core::persist::{self, ConnectionsFile, UiState};
use schemaic_core::schema::SchemaState;
use schemaic_db::{Db, DbError};
use schemaic_ui::theme::{EditorThemeKind, UiThemeKind};
use schemaic_ui::{
    AiActions, AiEffort, AiModel, AiUi, ChatMessage, ConnActions, ConnNode, ConnUi, CtxMenu,
    DraftSignals, HistoryActions, HistoryUi, InlineAiRequest, InlineAiState, LayoutUi, OverlayUi,
    PlanState, ResultPanel, RightPanel, Role, SchemaActions, SchemaScope, SchemaUi, Tab,
    TabsActions, TabsUi, TermActions, TermCursor, TermUi, TestState, Ui, pick_connection_color,
};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

fn main() {
    // MCP stdio server mode (launched by the `claude` CLI for the AI panel).
    // Runs the JSON-RPC loop and exits — no GUI. The (already-tunnelled) endpoint
    // arrives as a JSON blob in `$SCHEMAIC_MCP_ENDPOINT` (set via the MCP config
    // file, never a command-line arg — review C6). No credential URL is involved.
    if std::env::args().any(|a| a == "--mcp-serve") {
        let (db, database) = mcp_endpoint_from_env();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");
        rt.block_on(mcp::serve(db, database));
        return;
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("schemaic=info")),
        )
        .init();

    tracing::info!(
        "{} v{} starting",
        schemaic_core::APP_NAME,
        schemaic_core::APP_VERSION
    );

    // Register the bundled IBM Plex faces before any text is laid out.
    schemaic_ui::fonts::load_fonts();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime");
    let handle = rt.handle().clone();

    let config = WindowConfig::default()
        .size(Size::new(1280.0, 820.0))
        .title(schemaic_core::APP_NAME);

    Application::new()
        .window(move |_id| app_view(handle.clone()), Some(config))
        .run();

    drop(rt);
}

/// Build the terminal shell that launches the DB CLI for `conn`, optionally
/// scoped to `db`. Prefers a native `mysql` then `mariadb` on `PATH`, else falls
/// back to the WSL-side client (`wsl.exe -e mysql …`). Returns `None` when no
/// client is found anywhere. The password rides `MYSQL_PWD` (via `WSLENV` for the
/// WSL case) so it never appears on the command line or in shell history.
fn mysql_shell(
    conn: &schemaic_core::connection::Connection,
    db: Option<&str>,
) -> Option<schemaic_term::ShellConfig> {
    use schemaic_term::shell::which;
    let mut cli_args: Vec<String> = vec![
        "-h".into(),
        conn.host.clone(),
        "-P".into(),
        conn.port.to_string(),
        "-u".into(),
        conn.user.clone(),
    ];
    if let Some(d) = db {
        cli_args.push(d.to_string());
    }
    // Native client on PATH.
    for prog in ["mysql", "mariadb"] {
        if which(prog).is_some() {
            return Some(schemaic_term::ShellConfig {
                program: prog.into(),
                args: cli_args,
                cwd: None,
                env: vec![("MYSQL_PWD".into(), conn.password.clone())],
            });
        }
    }
    // WSL fallback: run the WSL client, forwarding MYSQL_PWD across via WSLENV.
    if which("wsl.exe").is_some() {
        let mut args: Vec<String> = vec!["-e".into(), "mysql".into()];
        args.extend(cli_args);
        return Some(schemaic_term::ShellConfig {
            program: "wsl.exe".into(),
            args,
            cwd: None,
            env: vec![
                ("WSLENV".into(), "MYSQL_PWD/u".into()),
                ("MYSQL_PWD".into(), conn.password.clone()),
            ],
        });
    }
    None
}

/// A throwaway shell that just prints `msg` and stays open — used to surface "no
/// client found" in the terminal rather than spawning a broken session.
fn message_shell(msg: &str) -> schemaic_term::ShellConfig {
    #[cfg(windows)]
    {
        schemaic_term::ShellConfig {
            program: "cmd.exe".into(),
            args: vec!["/k".into(), format!("echo {msg}")],
            cwd: None,
            env: Vec::new(),
        }
    }
    #[cfg(not(windows))]
    {
        schemaic_term::ShellConfig {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), format!("echo '{msg}'; exec /bin/sh")],
            cwd: None,
            env: Vec::new(),
        }
    }
}

/// Build the system-prompt context for the assistant: the active connection,
/// a db→tables outline, and the current query buffer.
/// First name of the form `base`, `base 1`, `base 2`, … not already present.
fn unique_name(base: &str, existing: &[String]) -> String {
    if !existing.iter().any(|e| e == base) {
        return base.to_string();
    }
    let mut n = 1;
    loop {
        let candidate = format!("{base} {n}");
        if !existing.iter().any(|e| e == &candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// The default connection created on first launch (matches the local WSL
/// MariaDB used in development).
fn seed_connection() -> Connection {
    Connection {
        id: 1,
        name: "Local MariaDB".to_string(),
        db_type: "MySQL".to_string(),
        host: "127.0.0.1".to_string(),
        port: 3306,
        user: "schemaic".to_string(),
        password: "schemaic".to_string(),
        ssh: Default::default(),
        color: None,
        prominent_color: false,
        read_only: false,
    }
}

fn app_view(handle: tokio::runtime::Handle) -> impl IntoView {
    let cx = Scope::current();

    // Load (or seed) saved connections.
    let mut cf = persist::load_connections();
    if cf.connections.is_empty() {
        let seed = seed_connection();
        cf.active = Some(seed.id);
        cf.connections.push(seed);
        persist::save_connections(&cf);
    }
    // Backfill an identity colour for any connection saved before colours existed
    // (and the freshly-seeded one), so every connection always has one. Colours
    // stay distinct while presets last; persist only if we changed something.
    {
        let mut used: Vec<String> = cf
            .connections
            .iter()
            .filter_map(|c| c.color.clone())
            .collect();
        let mut changed = false;
        for c in cf.connections.iter_mut() {
            if c.color.is_none() {
                let col = pick_connection_color(&used);
                used.push(col.clone());
                c.color = Some(col);
                changed = true;
            }
        }
        if changed {
            persist::save_connections(&cf);
        }
    }
    let active_id = cf
        .active
        .filter(|id| cf.connections.iter().any(|c| c.id == *id))
        .or_else(|| cf.connections.first().map(|c| c.id))
        .unwrap_or(1);
    let connections = RwSignal::new(cf.connections.clone());
    let active_conn = RwSignal::new(active_id);

    // Query history (persisted, newest-first across all connections; the panel
    // filters to the active connection).
    let history_entries = RwSignal::new(
        persist::load_json::<schemaic_core::history::HistoryFile>("history.json").entries,
    );

    // Per-column display formatters (persisted, keyed by connection+table+column;
    // read + upserted by the results grid's "Format as" menu).
    let formats = RwSignal::new(
        persist::load_json::<schemaic_core::format::FormatsFile>("format.json").rules,
    );
    let save_formats: Rc<dyn Fn()> = Rc::new(move || {
        persist::save_json(
            "format.json",
            &schemaic_core::format::FormatsFile {
                rules: formats.get_untracked(),
            },
        );
    });

    // Persisted UI state (loaded here so tab restore below can read `restore_tabs`).
    let ui_state = persist::load_ui_state();

    // Tab state. When "restore tabs on startup" is on and the last session saved
    // any tabs, rebuild them (query text + connection + source); otherwise start
    // with one blank tab bound to the active connection. A tab whose saved
    // connection no longer exists falls back to the active one (its query text is
    // still worth keeping). Each tab's database is filled in once its connection's
    // database list loads — but only while still `None`, so a restored database
    // survives (see the schema-load rebind).
    let saved_tabs = if ui_state.restore_tabs {
        persist::load_json::<schemaic_core::persist::SavedTabsFile>("tabs.json")
    } else {
        schemaic_core::persist::SavedTabsFile::default()
    };
    let (initial_tabs, initial_active, first_free_id): (Vec<Tab>, usize, usize) =
        if saved_tabs.tabs.is_empty() {
            (vec![Tab::new(cx, 1, "", active_id, None)], 1, 2)
        } else {
            let built: Vec<Tab> = saved_tabs
                .tabs
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let conn = if cf.connections.iter().any(|c| c.id == s.conn_id) {
                        s.conn_id
                    } else {
                        active_id
                    };
                    let t = Tab::new(cx, i + 1, &s.query, conn, s.database.clone());
                    t.source.set(s.source.clone());
                    t
                })
                .collect();
            let n = built.len();
            let active_id = built[saved_tabs.active.min(n - 1)].id;
            (built, active_id, n + 1)
        };
    let tabs = RwSignal::new(initial_tabs);
    let active = RwSignal::new(initial_active);
    let next_id = Rc::new(Cell::new(first_free_id));
    let flashing: RwSignal<Option<usize>> = RwSignal::new(None);
    // Per-tab in-flight query token, tagged with a monotonic run generation so a
    // completing run can tell whether it still owns the tab's slot (a newer run
    // or a tab close supersedes it) before touching `tokens`/`results`.
    let tokens: Rc<RefCell<HashMap<usize, (u64, CancellationToken)>>> =
        Rc::new(RefCell::new(HashMap::new()));
    let run_gen = Rc::new(Cell::new(0u64));

    // Cache of established SSH tunnels: connection id → live tunnel handle.
    // Keeps us from re-opening a tunnel on every schema reload; dropping a handle
    // (evict/replace) tears down its listener + local port (review H9).
    let tunnels: Rc<RefCell<HashMap<u64, schemaic_db::ssh::TunnelHandle>>> =
        Rc::new(RefCell::new(HashMap::new()));
    // The child scope the current `db_nodes` (and their `schema` signals) were
    // built in. Each `load_schema` swaps in a fresh scope and disposes the old
    // one, so a session's connection switches / refreshes don't accrete orphaned
    // schema signals (review C14).
    let nodes_scope: Rc<RefCell<Option<Scope>>> = Rc::new(RefCell::new(None));

    // Resolve a saved connection id to a `Db` handle (the app's connection
    // identity — no credential URL). For an SSH connection this needs the tunnel
    // to be established; returns `None` until it is (the caller reports "not
    // ready"). Because a tab carries its own `conn_id`, this keeps running each
    // tab against the connection it was opened under, even after the active
    // connection is switched (review H13).
    let db_for: Rc<dyn Fn(u64) -> Result<Db, String>> = {
        let tunnels = tunnels.clone();
        Rc::new(move |conn_id: u64| {
            let conn = connections
                .with_untracked(|cs| cs.iter().find(|c| c.id == conn_id).cloned())
                .ok_or_else(|| "connection no longer exists".to_string())?;
            let tunnel = if conn.ssh.enabled {
                match tunnels.borrow().get(&conn_id).map(|h| h.port()) {
                    Some(p) => Some(p),
                    None => return Err("SSH tunnel is not established yet".to_string()),
                }
            } else {
                None
            };
            Ok(Db::connect(&conn, tunnel))
        })
    };

    // Schema tree (one ConnNode per database of the active connection).
    let db_nodes: RwSignal<Vec<ConnNode>> = RwSignal::new(Vec::new());
    let expanded: RwSignal<HashSet<String>> =
        RwSignal::new(ui_state.expanded.into_iter().collect());
    let hidden_dbs: RwSignal<HashSet<String>> =
        RwSignal::new(ui_state.hidden_dbs.into_iter().collect());
    // Persisted panel layout: whether the schema sidebar is shown, and which
    // panel (AI / Terminal / None) fills the right column.
    let schema_visible: RwSignal<bool> = RwSignal::new(ui_state.schema_visible);
    let right_panel: RwSignal<RightPanel> = RwSignal::new(ui_state.right_panel.into());
    // Draggable-divider sizes, restored from the persisted layout (defaults live in
    // `UiState`). The resize handles mutate these live; a drag-end / double-click
    // reset commits them back to disk via `persist_layout`.
    let schema_w: RwSignal<f64> = RwSignal::new(ui_state.schema_w);
    let right_w: RwSignal<f64> = RwSignal::new(ui_state.right_w);
    let editor_h: RwSignal<f64> = RwSignal::new(ui_state.editor_h);
    // AI Assistant settings (gear → modal), restored from disk.
    let ai_settings_open = RwSignal::new(false);
    let ai_cli_path = RwSignal::new(ui_state.ai_cli_path.clone());
    let ai_model = RwSignal::new(AiModel::from_cli(&ui_state.ai_model));
    let ai_effort = RwSignal::new(AiEffort::from_cli(&ui_state.ai_effort));
    let ai_instructions = RwSignal::new(ui_state.ai_instructions.clone());
    let ai_schema_scope = RwSignal::new(SchemaScope::from_key(&ui_state.ai_schema_scope));
    let ai_run_queries = RwSignal::new(ui_state.ai_run_queries);
    // Appearance (Settings → theme picker), restored from disk. Seed the live
    // theme registry from the persisted choice *before* any view builds, then
    // mirror the signals into it whenever the picker mutates them (live switch).
    let theme_settings_open = RwSignal::new(false);
    let help_open = RwSignal::new(false);
    let ui_theme = RwSignal::new(UiThemeKind::from_key(&ui_state.ui_theme));
    let editor_theme = RwSignal::new(EditorThemeKind::from_key(&ui_state.editor_theme));
    schemaic_ui::theme::init(ui_theme.get_untracked(), editor_theme.get_untracked());
    create_effect(move |_| schemaic_ui::theme::set_ui(ui_theme.get()));
    create_effect(move |_| schemaic_ui::theme::set_editor(editor_theme.get()));
    // Editor content settings (font / indentation) + query/behaviour settings.
    // Seed the global editor-config registry before the view builds, then mirror the
    // signals into it live (a change re-lays out the editor / re-applies indent).
    let editor_font = RwSignal::new(ui_state.editor_font_size);
    let tab_width = RwSignal::new(ui_state.tab_width);
    let soft_tabs = RwSignal::new(ui_state.soft_tabs);
    let word_wrap = RwSignal::new(ui_state.word_wrap);
    let row_limit = RwSignal::new(ui_state.row_limit);
    let confirm_writes = RwSignal::new(ui_state.confirm_writes);
    let restore_tabs = RwSignal::new(ui_state.restore_tabs);
    schemaic_ui::theme::set_editor_font(editor_font.get_untracked());
    schemaic_ui::theme::set_editor_tab_width(tab_width.get_untracked());
    schemaic_ui::theme::set_editor_soft_tabs(soft_tabs.get_untracked());
    schemaic_ui::theme::set_editor_word_wrap(word_wrap.get_untracked());
    create_effect(move |_| schemaic_ui::theme::set_editor_font(editor_font.get()));
    create_effect(move |_| schemaic_ui::theme::set_editor_tab_width(tab_width.get()));
    create_effect(move |_| schemaic_ui::theme::set_editor_soft_tabs(soft_tabs.get()));
    create_effect(move |_| schemaic_ui::theme::set_editor_word_wrap(word_wrap.get()));
    let ai_detected_path = detect_claude_bin();
    let db_menu_open = RwSignal::new(false);
    let schema_menu_open = RwSignal::new(false);
    let context_menu: RwSignal<Option<CtxMenu>> = RwSignal::new(None);
    let last_mouse: RwSignal<(f64, f64)> = RwSignal::new((0.0, 0.0));
    let active_table: RwSignal<Option<(String, String)>> = RwSignal::new(None);

    // Manage-connections form + overlay signals.
    let draft = DraftSignals::new(cx);
    let conn_menu_open = RwSignal::new(false);
    let manage_open = RwSignal::new(false);
    let conn_test = RwSignal::new(TestState::Idle);
    let find_open = RwSignal::new(false);
    let find_query = RwSignal::new(String::new());
    let error_modal_open = RwSignal::new(false);
    let conn_status = RwSignal::new(ConnStatus::Unknown);

    // Query-plan (EXPLAIN) modal signals.
    let plan_open = RwSignal::new(false);
    let plan_state = RwSignal::new(PlanState::Idle);
    let plan_sql = RwSignal::new(String::new());
    let plan_analyze = RwSignal::new(false);

    // AI panel state. `ai_session` holds the live CLI conversation (bound to a
    // connection); the reader task streams transcript snapshots over a channel
    // into `ai_stream`, which an effect applies to `ai_messages`.
    let ai_messages: RwSignal<Vec<ChatMessage>> = RwSignal::new(Vec::new());
    let ai_input = RwSignal::new(String::new());
    let ai_busy = RwSignal::new(false);
    let ai_session: Rc<RefCell<Option<AiSession>>> = Rc::new(RefCell::new(None));
    let (ai_tx, ai_rx) = crossbeam_channel::unbounded::<AiStreamMsg>();
    let ai_stream = create_signal_from_channel(ai_rx);

    // Record an executed query into the history (newest-first, capped) and persist
    // it. Called from every run path (single Run, Run Current, Run Everything).
    let record_history: Rc<dyn Fn(u64, Option<String>, String)> = {
        Rc::new(move |conn_id: u64, database: Option<String>, sql: String| {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            history_entries.update(|v| {
                schemaic_core::history::push(
                    v,
                    schemaic_core::history::HistoryEntry {
                        conn_id,
                        database,
                        sql,
                        ts,
                    },
                );
            });
            persist::save_json(
                "history.json",
                &schemaic_core::history::HistoryFile {
                    entries: history_entries.get_untracked(),
                },
            );
        })
    };

    // Clear the active connection's history (the panel's trash button), persisting.
    let clear_history: Rc<dyn Fn()> = {
        Rc::new(move || {
            let conn = active_conn.get_untracked();
            history_entries.update(|v| schemaic_core::history::clear_conn(v, conn));
            persist::save_json(
                "history.json",
                &schemaic_core::history::HistoryFile {
                    entries: history_entries.get_untracked(),
                },
            );
        })
    };

    // ── Run a query into the active tab (targets that tab's connection URL) ──
    let run: Rc<dyn Fn(String)> = {
        let handle = handle.clone();
        let tokens = tokens.clone();
        let run_gen = run_gen.clone();
        let db_for = db_for.clone();
        let record_history = record_history.clone();
        Rc::new(move |sql: String| {
            if sql.trim().is_empty() {
                return;
            }
            let id = active.get_untracked();
            let Some(tab) = tabs.with_untracked(|v| v.iter().find(|t| t.id == id).copied()) else {
                return;
            };
            let results = tab.results;
            // Resolve this tab's own connection (not necessarily the active one).
            let db = match db_for(tab.conn_id.get_untracked()) {
                Ok(db) => db,
                Err(e) => {
                    tab.result_tabs.set(Vec::new());
                    results.set(QueryState::Failed(e));
                    return;
                }
            };
            let database = tab.database.get_untracked();
            (record_history)(tab.conn_id.get_untracked(), database.clone(), sql.clone());

            if let Some((_, old)) = tokens.borrow_mut().remove(&id) {
                old.cancel();
            }
            let token = CancellationToken::new();
            let generation = run_gen.get() + 1;
            run_gen.set(generation);
            tokens.borrow_mut().insert(id, (generation, token.clone()));
            // A single run reverts the results pane to the one-grid view (any
            // prior Run Everything tabs are cleared).
            tab.result_tabs.set(Vec::new());
            results.set(QueryState::Running);

            let tokens_done = tokens.clone();
            let send = create_ext_action(cx, move |state: QueryState| {
                // Only apply if this run still owns the tab (else a newer run or
                // a close superseded it — don't clobber their state/token).
                if tokens_done.borrow().get(&id).map(|(g, _)| *g) != Some(generation) {
                    return;
                }
                tokens_done.borrow_mut().remove(&id);
                results.set(state);
            });
            // Read the row cap on the UI thread (signals are single-threaded).
            let cap = row_limit.get_untracked();
            handle.spawn(async move {
                let state = match db.fetch_query(database.as_deref(), &sql, cap, token).await {
                    Ok(rs) => {
                        tracing::info!(
                            "query ok: {} rows (truncated={}), {} cols in {} ms",
                            rs.row_count(),
                            rs.truncated,
                            rs.col_count(),
                            rs.elapsed_ms
                        );
                        QueryState::Loaded(Arc::new(rs))
                    }
                    Err(DbError::Cancelled) => {
                        tracing::info!("query cancelled");
                        QueryState::Cancelled
                    }
                    Err(e) => {
                        tracing::error!("query failed: {e}");
                        QueryState::Failed(e.to_string())
                    }
                };
                send(state);
            });
        })
    };

    // ── Run EXPLAIN for the query-plan modal (targets the active tab's db) ──
    let plan_token: Rc<RefCell<Option<CancellationToken>>> = Rc::new(RefCell::new(None));
    let run_plan: Rc<dyn Fn(String, bool)> = {
        let handle = handle.clone();
        let db_for = db_for.clone();
        let plan_token = plan_token.clone();
        Rc::new(move |sql: String, analyze: bool| {
            if sql.trim().is_empty() {
                return;
            }
            let id = active.get_untracked();
            let Some(tab) = tabs.with_untracked(|v| v.iter().find(|t| t.id == id).copied()) else {
                return;
            };
            let db = match db_for(tab.conn_id.get_untracked()) {
                Ok(db) => db,
                Err(e) => {
                    plan_state.set(PlanState::Failed(e));
                    return;
                }
            };
            let database = tab.database.get_untracked();

            // Cancel any in-flight EXPLAIN (e.g. the Analyze toggle re-firing).
            if let Some(old) = plan_token.borrow_mut().take() {
                old.cancel();
            }
            let token = CancellationToken::new();
            *plan_token.borrow_mut() = Some(token.clone());
            plan_state.set(PlanState::Running);

            let send = create_ext_action(cx, move |st: PlanState| plan_state.set(st));
            handle.spawn(async move {
                let st = match db.explain(database.as_deref(), &sql, analyze, token).await {
                    Ok(rs) => PlanState::Loaded(schemaic_core::plan::QueryPlan::from_result(&rs)),
                    Err(DbError::Cancelled) => return, // superseded — leave state alone
                    Err(e) => PlanState::Failed(e.to_string()),
                };
                send(st);
            });
        })
    };

    // Run Everything: execute all statements in order on one connection (session
    // state carries across them), one result tab each. Seeds N "Running" panels
    // immediately, then fills every panel's final state in one update when the
    // batch completes.
    let run_all: Rc<dyn Fn(Vec<String>)> = {
        let handle = handle.clone();
        let tokens = tokens.clone();
        let run_gen = run_gen.clone();
        let db_for = db_for.clone();
        let record_history = record_history.clone();
        Rc::new(move |stmts: Vec<String>| {
            let stmts: Vec<String> = stmts.into_iter().filter(|s| !s.trim().is_empty()).collect();
            if stmts.is_empty() {
                return;
            }
            let id = active.get_untracked();
            let Some(tab) = tabs.with_untracked(|v| v.iter().find(|t| t.id == id).copied()) else {
                return;
            };
            let db = match db_for(tab.conn_id.get_untracked()) {
                Ok(db) => db,
                Err(e) => {
                    tab.results.set(QueryState::Failed(e));
                    return;
                }
            };
            let database = tab.database.get_untracked();
            // Record each statement (oldest first, so the batch lands newest-last).
            let conn_id = tab.conn_id.get_untracked();
            for s in &stmts {
                (record_history)(conn_id, database.clone(), s.clone());
            }

            if let Some((_, old)) = tokens.borrow_mut().remove(&id) {
                old.cancel();
            }
            let token = CancellationToken::new();
            let generation = run_gen.get() + 1;
            run_gen.set(generation);
            tokens.borrow_mut().insert(id, (generation, token.clone()));

            // Dismiss any single-run error bar; seed one Running panel per
            // statement (labelled "Result N") and select the first.
            tab.results.set(QueryState::Idle);
            let n = stmts.len();
            tab.result_tabs.set(
                (0..n)
                    .map(|i| ResultPanel {
                        label: format!("Result {}", i + 1),
                        state: QueryState::Running,
                    })
                    .collect(),
            );
            tab.active_result.set(0);

            let result_tabs = tab.result_tabs;
            let tokens_done = tokens.clone();
            let send = create_ext_action(cx, move |states: Vec<QueryState>| {
                // Only apply if this batch still owns the tab (see `run`).
                if tokens_done.borrow().get(&id).map(|(g, _)| *g) != Some(generation) {
                    return;
                }
                tokens_done.borrow_mut().remove(&id);
                result_tabs.update(|panels| {
                    for (p, st) in panels.iter_mut().zip(states) {
                        p.state = st;
                    }
                });
            });
            let cap = row_limit.get_untracked();
            handle.spawn(async move {
                let mut states: Vec<QueryState> = vec![QueryState::Cancelled; n];
                db.run_batch(database.as_deref(), &stmts, cap, token, |i, res| {
                    states[i] = match res {
                        Ok(rs) => QueryState::Loaded(Arc::new(rs)),
                        Err(DbError::Cancelled) => QueryState::Cancelled,
                        Err(e) => QueryState::Failed(e.to_string()),
                    };
                })
                .await;
                send(states);
            });
        })
    };

    let cancel: Rc<dyn Fn()> = {
        let tokens = tokens.clone();
        Rc::new(move || {
            let id = active.get_untracked();
            if let Some((_, tok)) = tokens.borrow().get(&id) {
                tok.cancel();
            }
        })
    };

    // Commit staged grid changes (cell edits + new-row inserts): run them in one
    // transaction off-thread, then reflect the database's truth (triggers /
    // defaults / computed columns). If the grid supplied a re-fetch request (a
    // spliceable single-table UPDATE-only result), we re-`SELECT` just the edited
    // rows and hand them back so the grid splices them in place — no re-run,
    // scroll/selection preserved. Otherwise (inserts, or not spliceable) we re-run
    // the whole query. On failure the message goes back and the grid keeps its edits.
    let commit_edits: schemaic_ui::CommitFn = {
        let handle = handle.clone();
        let run = run.clone();
        let db_for = db_for.clone();
        Rc::new(
            move |write: GridWrite,
                  refetch: Option<RefetchRequest>,
                  done: Rc<dyn Fn(CommitDone)>| {
                if write.is_empty() {
                    return;
                }
                let id = active.get_untracked();
                let Some(tab) = tabs.with_untracked(|v| v.iter().find(|t| t.id == id).copied())
                else {
                    return;
                };
                let db = match db_for(tab.conn_id.get_untracked()) {
                    Ok(db) => db,
                    Err(e) => {
                        (done)(CommitDone::Failed(e));
                        return;
                    }
                };
                let query = tab.query.get_untracked();
                let run = run.clone();
                let finish = create_ext_action(cx, move |outcome: CommitDone| {
                    // A full re-run must happen on the UI thread and only if the
                    // committed tab is still active — `run` targets the active tab,
                    // so refreshing after the user switched away would run this
                    // tab's SQL against a different tab (H4). If they switched, skip
                    // it; the commit already succeeded (the tab's cached result is
                    // then stale until a manual re-run, matching prior behaviour).
                    // A splice with the tab no longer active is downgraded to a
                    // no-op (the grid it targeted is gone).
                    let still_active = active.get_untracked() == id;
                    let outcome = match outcome {
                        CommitDone::FullReran => {
                            if still_active {
                                (run)(query.clone());
                            }
                            CommitDone::FullReran
                        }
                        CommitDone::Spliced(rows) if still_active => CommitDone::Spliced(rows),
                        CommitDone::Spliced(_) => CommitDone::FullReran,
                        other => other,
                    };
                    (done)(outcome);
                });
                handle.spawn(async move {
                    let token = CancellationToken::new();
                    match db.commit_writes(&write, token.clone()).await {
                        Err(e) => {
                            tracing::error!("commit failed: {e}");
                            finish(CommitDone::Failed(e.to_string()));
                        }
                        Ok(_) => match refetch {
                            // Splice path: re-fetch just the edited rows. If that
                            // fails, fall back to a full re-run (data is committed).
                            Some(req) => {
                                match db.refetch_rows(&req.template, &req.rows, token).await {
                                    Ok(rows) => finish(CommitDone::Spliced(rows)),
                                    Err(e) => {
                                        tracing::warn!("re-fetch after commit failed: {e}");
                                        finish(CommitDone::FullReran);
                                    }
                                }
                            }
                            None => finish(CommitDone::FullReran),
                        },
                    }
                });
            },
        )
    };

    // Active-database context. A tab carries its `(conn_id, database)`, so
    // switching the active db just rewrites the active tab's `database` (and binds
    // it to the active connection) — no server-side `USE` / session state to track.
    let active_db_menu_open = RwSignal::new(false);
    let active_db_anchor = RwSignal::new(floem::kurbo::Point::ZERO);
    // The last database the user explicitly switched to; new tabs default to it.
    let last_db: RwSignal<Option<String>> = RwSignal::new(None);
    let active_db: floem::reactive::Memo<Option<String>> = create_memo(move |_| {
        let id = active.get();
        tabs.with(|v| v.iter().find(|t| t.id == id).and_then(|t| t.database.get()))
    });
    let set_active_db: Rc<dyn Fn(String)> = Rc::new(move |name: String| {
        // The DB selector lists the active connection's databases, so picking one
        // binds the active tab to the active connection + that database.
        let exists = db_nodes.with_untracked(|ns| ns.iter().any(|n| n.database == name));
        if exists {
            let id = active.get_untracked();
            tabs.with_untracked(|v| {
                if let Some(t) = v.iter().find(|t| t.id == id) {
                    t.conn_id.set(active_conn.get_untracked());
                    t.database.set(Some(name.clone()));
                }
            });
            last_db.set(Some(name));
        }
    });

    // A new tab's target `(conn_id, database)`: the active connection, scoped to
    // the last database the user switched to, else its first database (so an
    // unqualified `SELECT … FROM t` has a context), else `None` before the list
    // has loaded.
    let default_tab_target: Rc<dyn Fn() -> (u64, Option<String>)> = Rc::new(move || {
        let conn_id = active_conn.get_untracked();
        let database = db_nodes.with_untracked(|v| {
            last_db
                .get_untracked()
                .filter(|name| v.iter().any(|n| &n.database == name))
                .or_else(|| v.first().map(|n| n.database.clone()))
        });
        (conn_id, database)
    });

    let add_tab: Rc<dyn Fn()> = {
        let next_id = next_id.clone();
        let default_tab_target = default_tab_target.clone();
        Rc::new(move || {
            let id = next_id.get();
            next_id.set(id + 1);
            let (conn_id, database) = default_tab_target();
            tabs.update(|v| v.push(Tab::new(cx, id, "", conn_id, database)));
            active.set(id);
        })
    };

    // Close a tab. Closing the last one clears it and briefly flashes it away
    // (design keeps ≥1 tab); other tabs activate a neighbor.
    let close_tab: Rc<dyn Fn(usize)> = {
        let tokens = tokens.clone();
        Rc::new(move |id: usize| {
            // H5: cancel this tab's in-flight query so it can't complete onto
            // cleared/freed signals (and stops the server-side work).
            if let Some((_, tok)) = tokens.borrow_mut().remove(&id) {
                tok.cancel();
            }
            let is_last = tabs.with_untracked(|v| v.len()) <= 1;
            if is_last {
                let Some(tab) = tabs.with_untracked(|v| v.iter().find(|t| t.id == id).copied())
                else {
                    return;
                };
                tab.query.set(String::new());
                tab.source.set(None);
                // Also reset the results pane so the reopened tab is fully fresh
                // (single-grid Idle state, no leftover Run-Everything tabs).
                tab.results.set(QueryState::Idle);
                tab.result_tabs.set(Vec::new());
                tab.active_result.set(0);
                flashing.set(Some(id));
                exec_after(Duration::from_millis(150), move |_| flashing.set(None));
                return;
            }
            let was_active = active.get_untracked() == id;
            let neighbor = tabs.with_untracked(|v| {
                v.iter().position(|t| t.id == id).map(|i| {
                    let ni = if i + 1 < v.len() { i + 1 } else { i - 1 };
                    v[ni].id
                })
            });
            // Grab this tab's scope before dropping it from the list, so we can
            // free its signals (C14).
            let closed_cx = tabs.with_untracked(|v| v.iter().find(|t| t.id == id).map(|t| t.cx));
            tabs.update(|v| v.retain(|t| t.id != id));
            if was_active
                && let Some(n) = neighbor {
                    active.set(n);
                }
            // Dispose deferred: the center view is keyed on the active tab, so it
            // rebuilds (unmounting this tab's editor/grid) after the `active.set`
            // above. Freeing the scope now would drop signals its still-mounted
            // view reads this frame → disposed-signal panic. One tick later the
            // old view is gone.
            if let Some(scope) = closed_cx {
                exec_after(Duration::ZERO, move |_| scope.dispose());
            }
        })
    };

    // Place a freshly-built tab: reuse the active tab *in place* if it's a blank
    // slate (empty editor, no results / no Run-Everything panels) — the common
    // "app opened on an empty Query 1" case — otherwise open it as a new tab.
    // Keeps the reused tab's visible number so it reads as the same tab.
    let place_tab: Rc<dyn Fn(Tab)> = Rc::new(move |new_tab: Tab| {
        let active_id = active.get_untracked();
        let reuse_at = tabs.with_untracked(|v| {
            v.iter().position(|t| t.id == active_id).filter(|&i| {
                let t = &v[i];
                t.query.get_untracked().trim().is_empty()
                    && matches!(t.results.get_untracked(), QueryState::Idle)
                    && t.result_tabs.get_untracked().is_empty()
            })
        });
        // When reusing a blank tab in place, its (empty) signals are replaced by
        // the new tab's — free the old scope so it doesn't leak (C14).
        let replaced_cx = reuse_at.map(|pos| tabs.with_untracked(|v| v[pos].cx));
        tabs.update(move |v| match reuse_at {
            Some(pos) => {
                let mut nt = new_tab;
                nt.label = v[pos].label;
                v[pos] = nt;
            }
            None => v.push(new_tab),
        });
        active.set(new_tab.id);
        // Deferred for the same reason as `close_tab`: let the center view rebuild
        // for the new tab id before the old tab's scope is dropped.
        if let Some(scope) = replaced_cx {
            exec_after(Duration::ZERO, move |_| scope.dispose());
        }
    });

    // Build + place a fresh tab showing a table: `SELECT * … LIMIT 100` bound to
    // the active connection + that db, remembering its source for tree
    // highlighting. Reuses a blank active tab (via `place_tab`), but never dedupes
    // to an already-open table tab — that's the caller's job.
    let spawn_table_tab: Rc<dyn Fn(String, String)> = {
        let run = run.clone();
        let next_id = next_id.clone();
        let place_tab = place_tab.clone();
        Rc::new(move |database: String, table: String| {
            let id = next_id.get();
            next_id.set(id + 1);
            let sql = format!("SELECT * FROM {table} LIMIT 100");
            let tab = Tab::new(
                cx,
                id,
                &sql,
                active_conn.get_untracked(),
                Some(database.clone()),
            );
            tab.source.set(Some((database, table)));
            (place_tab)(tab);
            run(sql);
        })
    };

    // Open a table from the sidebar / Find ("Open"): if a tab is already showing
    // it (same connection + source), just switch to that tab; otherwise open a
    // fresh one. Matching on `conn_id` too (not source alone) so the same-named
    // table under a different connection doesn't wrongly steal focus (H13).
    let open_table: Rc<dyn Fn(String, String)> = {
        let spawn = spawn_table_tab.clone();
        Rc::new(move |database: String, table: String| {
            let existing = tabs.with_untracked(|v| {
                v.iter()
                    .find(|t| {
                        t.source.get_untracked() == Some((database.clone(), table.clone()))
                            && t.conn_id.get_untracked() == active_conn.get_untracked()
                    })
                    .map(|t| t.id)
            });
            if let Some(id) = existing {
                active.set(id);
                return;
            }
            (spawn)(database, table);
        })
    };

    // Always open the table in a brand-new tab, even if it's already open
    // ("Open in new tab" — only offered by the menu when a tab for it exists).
    let open_table_new: Rc<dyn Fn(String, String)> = {
        let spawn = spawn_table_tab.clone();
        Rc::new(move |database: String, table: String| (spawn)(database, table))
    };

    // Open a new tab with `sql` in the editor but do NOT run it (used by
    // "Generate DDL" in the schema context menu, and the AI code-block bar).
    let open_query: Rc<dyn Fn(String)> = {
        let next_id = next_id.clone();
        let default_tab_target = default_tab_target.clone();
        let place_tab = place_tab.clone();
        Rc::new(move |sql: String| {
            let id = next_id.get();
            next_id.set(id + 1);
            let (conn_id, database) = default_tab_target();
            (place_tab)(Tab::new(cx, id, &sql, conn_id, database));
        })
    };

    // ── Persisted expand/collapse + database-visibility state ───────────────
    // Snapshot both sets to disk (best effort).
    let save_ui: Rc<dyn Fn()> = Rc::new(move || {
        persist::save_ui_state(&UiState {
            expanded: expanded.with_untracked(|s| s.iter().cloned().collect()),
            hidden_dbs: hidden_dbs.with_untracked(|s| s.iter().cloned().collect()),
            schema_visible: schema_visible.get_untracked(),
            right_panel: right_panel.get_untracked().into(),
            schema_w: schema_w.get_untracked(),
            right_w: right_w.get_untracked(),
            editor_h: editor_h.get_untracked(),
            ai_cli_path: ai_cli_path.get_untracked(),
            ai_model: ai_model.get_untracked().cli().to_string(),
            ai_effort: ai_effort.get_untracked().cli().to_string(),
            ai_instructions: ai_instructions.get_untracked(),
            ai_schema_scope: ai_schema_scope.get_untracked().key().to_string(),
            ai_run_queries: ai_run_queries.get_untracked(),
            ui_theme: ui_theme.get_untracked().key().to_string(),
            editor_theme: editor_theme.get_untracked().key().to_string(),
            editor_font_size: editor_font.get_untracked(),
            row_limit: row_limit.get_untracked(),
            confirm_writes: confirm_writes.get_untracked(),
            tab_width: tab_width.get_untracked(),
            soft_tabs: soft_tabs.get_untracked(),
            word_wrap: word_wrap.get_untracked(),
            restore_tabs: restore_tabs.get_untracked(),
        });
    });

    // Persist the layout whenever a panel is toggled (the footer chips mutate
    // these signals directly, so we react rather than route through a callback).
    {
        let save_ui = save_ui.clone();
        create_effect(move |_| {
            schema_visible.get();
            right_panel.get();
            save_ui();
        });
    }

    // Persist the theme choice whenever the picker changes it. (First run writes
    // the current values back — harmless; the file already holds them.)
    {
        let save_ui = save_ui.clone();
        create_effect(move |_| {
            ui_theme.get();
            editor_theme.get();
            save_ui();
        });
    }

    // Persist the editor / query settings whenever they change.
    {
        let save_ui = save_ui.clone();
        create_effect(move |_| {
            editor_font.get();
            tab_width.get();
            soft_tabs.get();
            word_wrap.get();
            row_limit.get();
            confirm_writes.get();
            restore_tabs.get();
            save_ui();
        });
    }

    // Persist the open tabs (query text + connection + source) so the next launch
    // can restore the session, when the setting is on. Query edits fire on every
    // keystroke, so the write is debounced with a short trailing delay: each change
    // bumps a generation and schedules a save; a later change (or toggling the
    // setting off) supersedes the pending one, so only the last edit of a burst
    // touches disk. `tabs.json` holds ids/text only — no credentials.
    {
        let tabs_save_gen = Rc::new(Cell::new(0u64));
        create_effect(move |_| {
            let on = restore_tabs.get();
            // Read structure + each tab's persisted fields so an edit re-runs us.
            tabs.with(|v| {
                for t in v {
                    t.query.get();
                    t.conn_id.get();
                    t.database.get();
                    t.source.get();
                }
            });
            active.get();
            let g = tabs_save_gen.get() + 1;
            tabs_save_gen.set(g);
            if !on {
                return; // bumping `g` above also cancels any pending save
            }
            let gen_at = tabs_save_gen.clone();
            exec_after(Duration::from_millis(600), move |_| {
                if gen_at.get() != g {
                    return; // superseded by a newer change
                }
                let file = tabs.with_untracked(|v| {
                    let active_id = active.get_untracked();
                    schemaic_core::persist::SavedTabsFile {
                        active: v.iter().position(|t| t.id == active_id).unwrap_or(0),
                        tabs: v
                            .iter()
                            .map(|t| schemaic_core::persist::SavedTab {
                                query: t.query.get_untracked(),
                                conn_id: t.conn_id.get_untracked(),
                                database: t.database.get_untracked(),
                                source: t.source.get_untracked(),
                            })
                            .collect(),
                    }
                });
                persist::save_json("tabs.json", &file);
            });
        });
    }

    let on_toggle: Rc<dyn Fn(String)> = {
        let save_ui = save_ui.clone();
        Rc::new(move |key: String| {
            expanded.update(move |set| {
                if !set.remove(&key) {
                    set.insert(key);
                }
            });
            save_ui();
        })
    };

    let toggle_db_hidden: Rc<dyn Fn(String)> = {
        let save_ui = save_ui.clone();
        Rc::new(move |db: String| {
            hidden_dbs.update(move |set| {
                if !set.remove(&db) {
                    set.insert(db);
                }
            });
            save_ui();
        })
    };

    // Collapse every node (databases + tables): clear the whole expanded set.
    let collapse_all: Rc<dyn Fn()> = {
        let save_ui = save_ui.clone();
        Rc::new(move || {
            expanded.update(|set| set.clear());
            save_ui();
        })
    };

    // Collapse just one database's tables (keep the DB node itself open):
    // drop every `tbl:<database>:*` key.
    let collapse_db: Rc<dyn Fn(String)> = {
        let save_ui = save_ui.clone();
        Rc::new(move |db: String| {
            let prefix = format!("tbl:{db}:");
            expanded.update(|set| set.retain(|k| !k.starts_with(&prefix)));
            save_ui();
        })
    };

    // ── Connection health ────────────────────────────────────────────────────
    // One health check of the *active* connection: ping it (through the SSH
    // tunnel if one is established) and set `conn_status`. Runs off the UI
    // thread; the result is marshalled back via `create_ext_action`.
    let check_conn: Rc<dyn Fn()> = {
        let handle = handle.clone();
        let tunnels = tunnels.clone();
        Rc::new(move || {
            let id = active_conn.get_untracked();
            let Some(conn) =
                connections.with_untracked(|cs| cs.iter().find(|c| c.id == id).cloned())
            else {
                conn_status.set(ConnStatus::Unknown);
                return;
            };
            // Effective endpoint — through the tunnel for SSH connections. If the
            // tunnel isn't up yet, stay Unknown; a later tick will catch it.
            let tunnel = if conn.ssh.enabled {
                match tunnels.borrow().get(&conn.id).map(|h| h.port()) {
                    Some(port) => Some(port),
                    None => {
                        conn_status.set(ConnStatus::Unknown);
                        return;
                    }
                }
            } else {
                None
            };
            let db = Db::connect(&conn, tunnel);
            let send = create_ext_action(cx, move |ok: bool| {
                conn_status.set(if ok {
                    ConnStatus::Connected
                } else {
                    ConnStatus::Disconnected
                });
            });
            handle.spawn(async move {
                let ok = db.ping(std::time::Duration::from_secs(5)).await.is_ok();
                send(ok);
            });
        })
    };

    // ── Schema loading ──────────────────────────────────────────────────────
    // For an SSH connection, open (or reuse) a tunnel first, then list the
    // databases through it; the resolved tunnel port is cached and every
    // downstream `Db` (schema, table-open, editor) is built pointing through it.
    let load_schema: Rc<dyn Fn(Connection)> = {
        let handle = handle.clone();
        let tunnels = tunnels.clone();
        let nodes_scope = nodes_scope.clone();
        Rc::new(move |conn: Connection| {
            db_nodes.set(Vec::new());
            let nodes_scope_cb = nodes_scope.clone();
            let cached_port = tunnels.borrow().get(&conn.id).map(|h| h.port());
            let handle_inner = handle.clone();
            let tunnels_cache = tunnels.clone();
            // `conn` (original) → the send callback; `conn_task` → the async task.
            let conn_send = conn.clone();
            // Result payload: the effective tunnel port (if SSH), a *newly opened*
            // tunnel handle to cache (None when reusing a cached one), and the db
            // names.
            let send = create_ext_action(
                cx,
                move |res: ConnectResult| match res {
                    Ok((tunnel_port, new_handle, names)) => {
                        if let Some(handle) = new_handle {
                            // Dropping any prior handle here tears its listener down.
                            tunnels_cache.borrow_mut().insert(conn_send.id, handle);
                        }
                        // Build these nodes in a fresh child scope; dispose the
                        // previous set's scope (deferred, so the schema tree —
                        // keyed on node id — rebuilds off the new nodes before the
                        // old signals are freed) so schema signals don't leak
                        // across connection switches / refreshes (C14).
                        let node_cx = cx.create_child();
                        let mut nodes = Vec::with_capacity(names.len());
                        for (i, name) in names.iter().enumerate() {
                            nodes.push(ConnNode::new(node_cx, i + 1, name, name));
                        }
                        db_nodes.set(nodes.clone());
                        if let Some(old) = nodes_scope_cb.borrow_mut().replace(node_cx) {
                            exec_after(Duration::ZERO, move |_| old.dispose());
                        }
                        // Bind any tab of THIS connection that doesn't yet have a
                        // database (e.g. the initial tab) to the first database.
                        if let Some(first) = names.first() {
                            tabs.with_untracked(|v| {
                                for t in v {
                                    if t.conn_id.get_untracked() == conn_send.id
                                        && t.database.get_untracked().is_none()
                                    {
                                        t.database.set(Some(first.clone()));
                                    }
                                }
                            });
                        }
                        // One `Db` for this connection, cloned per-database fetch.
                        let db = Db::connect(&conn_send, tunnel_port);
                        for node in nodes {
                            let sig = node.schema;
                            let dbname = node.database.clone();
                            let db = db.clone();
                            let send_schema =
                                create_ext_action(cx, move |st: SchemaState| sig.set(st));
                            handle_inner.spawn(async move {
                                let st = match db.fetch_schema(&dbname).await {
                                    Ok(s) => SchemaState::Loaded(s),
                                    Err(e) => SchemaState::Failed(e.to_string()),
                                };
                                send_schema(st);
                            });
                        }
                    }
                    Err(e) => {
                        tracing::error!("schema load failed: {e}");
                        db_nodes.set(Vec::new());
                    }
                },
            );
            let conn_task = conn.clone();
            handle.spawn(async move {
                // Establish (or reuse) the SSH tunnel, then build the `Db`. A
                // freshly opened tunnel's handle is returned so the UI thread can
                // cache it (and thereby own its lifetime).
                let (tunnel_port, new_handle) = if conn_task.ssh.enabled {
                    match cached_port {
                        Some(p) => (Some(p), None),
                        None => match schemaic_db::ssh::open_tunnel(
                            &conn_task.ssh,
                            &conn_task.host,
                            conn_task.port,
                        )
                        .await
                        {
                            Ok(h) => (Some(h.port()), Some(h)),
                            Err(e) => {
                                send(Err(e.to_string()));
                                return;
                            }
                        },
                    }
                } else {
                    (None, None)
                };
                let db = Db::connect(&conn_task, tunnel_port);
                match db.fetch_databases().await {
                    Ok(names) => send(Ok((tunnel_port, new_handle, names))),
                    Err(e) => send(Err(e.to_string())),
                }
            });
        })
    };

    // Re-introspect a single database's schema in place (context-menu Refresh).
    // Finds the matching node and re-fetches just its tables — no full tree
    // rebuild, so the rest of the panel and its expansion state stay put.
    let refresh_db: Rc<dyn Fn(String)> = {
        let handle = handle.clone();
        let db_for = db_for.clone();
        Rc::new(move |database: String| {
            let sig = db_nodes.with_untracked(|nodes| {
                nodes
                    .iter()
                    .find(|n| n.database == database)
                    .map(|n| n.schema)
            });
            let Some(sig) = sig else { return };
            // The tree shows the active connection's databases, so refresh runs
            // against the active connection's `Db`.
            let db = match db_for(active_conn.get_untracked()) {
                Ok(db) => db,
                Err(e) => {
                    sig.set(SchemaState::Failed(e));
                    return;
                }
            };
            sig.set(SchemaState::Loading);
            let send_schema = create_ext_action(cx, move |st: SchemaState| sig.set(st));
            handle.spawn(async move {
                let st = match db.fetch_schema(&database).await {
                    Ok(s) => SchemaState::Loaded(s),
                    Err(e) => SchemaState::Failed(e.to_string()),
                };
                send_schema(st);
            });
        })
    };

    // Full refresh of the active connection (SCHEMA settings → Refresh): re-lists
    // databases and reloads every schema, and re-checks reachability.
    let refresh_schema: Rc<dyn Fn()> = {
        let load_schema = load_schema.clone();
        let check_conn = check_conn.clone();
        Rc::new(move || {
            if let Some(conn) = connections.with_untracked(|cs| {
                cs.iter()
                    .find(|c| c.id == active_conn.get_untracked())
                    .cloned()
            }) {
                load_schema(conn);
            }
            check_conn();
        })
    };

    // Persist the current connections list with a given active id.
    let persist_conns = move |active: Option<u64>| {
        let file = ConnectionsFile {
            connections: connections.get_untracked(),
            active,
        };
        persist::save_connections(&file);
    };

    // Switch the active connection and reload its schema.
    let switch_conn: Rc<dyn Fn(u64)> = {
        let load_schema = load_schema.clone();
        let ai_session = ai_session.clone();
        let check_conn = check_conn.clone();
        Rc::new(move |id: u64| {
            active_conn.set(id);
            persist_conns(Some(id));
            // Clear stale status until this connection's own check lands.
            conn_status.set(ConnStatus::Unknown);
            // The AI conversation is bound to a connection — reset it on switch.
            *ai_session.borrow_mut() = None;
            ai_messages.set(Vec::new());
            ai_busy.set(false);
            if let Some(conn) =
                connections.with_untracked(|cs| cs.iter().find(|c| c.id == id).cloned())
            {
                load_schema(conn);
            }
            check_conn();
        })
    };

    // Load an existing connection into the edit form.
    let select_conn: Rc<dyn Fn(u64)> = Rc::new(move |id: u64| {
        if let Some(conn) = connections.with_untracked(|cs| cs.iter().find(|c| c.id == id).cloned())
        {
            draft.load(&conn);
        }
    });

    // Start editing a brand-new connection: a blank form with a unique default
    // name. NOT persisted until the user clicks Save.
    let new_conn: Rc<dyn Fn()> = Rc::new(move || {
        let existing: Vec<String> =
            connections.with_untracked(|cs| cs.iter().map(|c| c.name.clone()).collect());
        let used_colors: Vec<String> =
            connections.with_untracked(|cs| cs.iter().filter_map(|c| c.color.clone()).collect());
        draft.blank();
        draft.name.set(unique_name("New connection", &existing));
        // Auto-assign a distinct identity colour (the user can change it below).
        draft.color.set(Some(pick_connection_color(&used_colors)));
    });

    // Test the draft's host + credentials without saving: open a throwaway
    // connection (and, for SSH, a throwaway tunnel that drops at task end — never
    // cached, since the draft may differ from any saved connection) and ping it.
    // The result lands in `conn_test` as an icon on the Test button.
    let test_conn: Rc<dyn Fn()> = {
        let handle = handle.clone();
        Rc::new(move || {
            conn_test.set(TestState::Testing);
            let conn = draft.to_connection(0);
            let send = create_ext_action(cx, move |ok: bool| {
                conn_test.set(if ok { TestState::Ok } else { TestState::Fail });
            });
            handle.spawn(async move {
                // Keep the tunnel handle alive for the duration of the ping; it
                // drops (freeing the listener/port) when this task ends.
                let tunnel = if conn.ssh.enabled {
                    match schemaic_db::ssh::open_tunnel(&conn.ssh, &conn.host, conn.port).await {
                        Ok(h) => Some(h),
                        Err(_) => {
                            send(false);
                            return;
                        }
                    }
                } else {
                    None
                };
                let db = Db::connect(&conn, tunnel.as_ref().map(|h| h.port()));
                let ok = db.ping(std::time::Duration::from_secs(5)).await.is_ok();
                drop(tunnel);
                send(ok);
            });
        })
    };

    // Save the form (create or update); reload schema if the active conn changed.
    let save_conn: Rc<dyn Fn()> = {
        let load_schema = load_schema.clone();
        let tunnels = tunnels.clone();
        Rc::new(move || {
            let id = draft.id.get_untracked().unwrap_or_else(|| {
                connections.with_untracked(|cs| cs.iter().map(|c| c.id).max().unwrap_or(0)) + 1
            });
            let conn = draft.to_connection(id);
            connections.update(|cs| {
                if let Some(existing) = cs.iter_mut().find(|c| c.id == id) {
                    *existing = conn.clone();
                } else {
                    cs.push(conn.clone());
                }
            });
            draft.id.set(Some(id));
            persist_conns(Some(active_conn.get_untracked()));
            // The edit may have changed the host / SSH settings, so any cached
            // tunnel for this connection is stale — drop it (its listener is torn
            // down) so `load_schema` re-establishes a fresh one (review H9).
            tunnels.borrow_mut().remove(&id);
            if active_conn.get_untracked() == id {
                load_schema(conn);
            }
        })
    };

    // Delete a connection; if it was active, fall back to the first remaining.
    let delete_conn: Rc<dyn Fn(u64)> = {
        let load_schema = load_schema.clone();
        let tunnels = tunnels.clone();
        Rc::new(move |id: u64| {
            let was_active = active_conn.get_untracked() == id;
            // Drop any tunnel for the deleted connection (frees its listener/port).
            tunnels.borrow_mut().remove(&id);
            connections.update(|cs| cs.retain(|c| c.id != id));
            let fallback = connections.with_untracked(|cs| cs.first().map(|c| c.id));
            let new_active = if was_active {
                fallback
            } else {
                Some(active_conn.get_untracked())
            };
            persist_conns(new_active);
            match connections.with_untracked(|cs| cs.first().cloned()) {
                Some(c) => draft.load(&c),
                None => {
                    draft.blank();
                    // A fresh blank form still gets an identity colour so a
                    // connection saved from it is never colourless.
                    draft.color.set(Some(pick_connection_color(&[])));
                }
            }
            if was_active {
                match connections.with_untracked(|cs| cs.first().cloned()) {
                    Some(conn) => {
                        active_conn.set(conn.id);
                        load_schema(conn);
                    }
                    None => db_nodes.set(Vec::new()),
                }
            }
        })
    };

    // ── AI panel (Claude Code) ──────────────────────────────────────────────
    // Apply streamed transcript snapshots to the pending assistant bubble.
    create_effect(move |_| {
        if let Some(msg) = ai_stream.get() {
            ai_messages.update(|v| {
                if let Some(last) = v.last_mut() {
                    last.segs = msg.segs;
                    last.stats = msg.stats;
                    last.pending = !msg.done;
                    if msg.done && msg.is_error {
                        last.role = Role::Error;
                    }
                }
            });
            if msg.done {
                ai_busy.set(false);
            }
        }
    });

    // Send a user turn: (re)start the per-connection session, then write it to
    // the CLI's stdin. Replies stream back via `ai_stream` (above).
    // Snapshot the session-affecting AI settings (Copy signals → this closure is
    // Copy, usable from both `ai_send` and `ai_apply`).
    let ai_settings_now = move || AiSettings {
        model: ai_model.get_untracked(),
        effort: ai_effort.get_untracked(),
        run_queries: ai_run_queries.get_untracked(),
        cli_path: ai_cli_path.get_untracked(),
        instructions: ai_instructions.get_untracked(),
        schema_scope: ai_schema_scope.get_untracked(),
    };

    let ai_send: Rc<dyn Fn(String)> = {
        let handle = handle.clone();
        let ai_session = ai_session.clone();
        let default_tab_target = default_tab_target.clone();
        let db_for = db_for.clone();
        Rc::new(move |msg: String| {
            let msg = msg.trim().to_string();
            if msg.is_empty() || ai_busy.get_untracked() {
                return;
            }
            let active_id = active_conn.get_untracked();
            let need_new = ai_session
                .borrow()
                .as_ref()
                .map(|s| s.conn_id != active_id)
                .unwrap_or(true);
            if need_new {
                let context = ai_context(
                    AiContextParams {
                        connections,
                        active_conn,
                        db_nodes,
                        tabs,
                        active,
                        scope: ai_schema_scope.get_untracked(),
                        run_queries: ai_run_queries.get_untracked(),
                    },
                    &ai_instructions.get_untracked(),
                );
                // The MCP endpoint is the active connection, scoped to the active
                // tab's database (else the new-tab default). If the connection's
                // `Db` can't be built yet (SSH tunnel pending), skip the MCP tools
                // rather than blocking the chat.
                let database = tabs
                    .with_untracked(|v| {
                        v.iter()
                            .find(|t| t.id == active.get_untracked())
                            .map(|t| t.database.get_untracked())
                    })
                    .flatten()
                    .or_else(|| default_tab_target().1);
                if let Ok(db) = db_for(active_id) {
                    let (stdin_tx, mcp_cfg) = start_ai_session(
                        &handle,
                        StartAiParams {
                            system_context: context,
                            db,
                            database,
                            ai_tx: ai_tx.clone(),
                            model: ai_model.get_untracked().cli().to_string(),
                            effort: ai_effort.get_untracked().cli().to_string(),
                            run_queries: ai_run_queries.get_untracked(),
                            cli_path: ai_cli_path.get_untracked(),
                        },
                    );
                    *ai_session.borrow_mut() = Some(AiSession {
                        conn_id: active_id,
                        stdin_tx,
                        mcp_cfg,
                        settings: ai_settings_now(),
                    });
                }
            }

            ai_messages.update(|v| {
                v.push(ChatMessage::user(msg.clone()));
                v.push(ChatMessage::pending());
            });
            ai_input.set(String::new());
            ai_busy.set(true);

            if let Some(s) = ai_session.borrow().as_ref() {
                let _ = s.stdin_tx.send(schemaic_ai::user_message_line(&msg));
            }
        })
    };

    // Kill the in-flight assistant turn (the message-field stop button). Dropping
    // the session's stdin sender closes the reader task's channel, which drops the
    // `claude` child (kill_on_drop) → the turn ends. A fresh session starts on the
    // next message (need_new). Trade-off: this ends the whole session, so the
    // conversation context resets after a cancel.
    let ai_cancel: Rc<dyn Fn()> = {
        let ai_session = ai_session.clone();
        Rc::new(move || {
            if !ai_busy.get_untracked() {
                return;
            }
            ai_session.borrow_mut().take();
            ai_messages.update(|v| {
                if let Some(last) = v.last_mut() {
                    last.pending = false;
                    if last.segs.is_empty() {
                        last.segs = vec![schemaic_core::transcript::Seg::Text("(stopped)".into())];
                    }
                }
            });
            ai_busy.set(false);
        })
    };

    // New chat: drop the session (fresh context next message) and clear bubbles.
    let ai_new_chat: Rc<dyn Fn()> = {
        let ai_session = ai_session.clone();
        Rc::new(move || {
            ai_session.borrow_mut().take();
            ai_messages.set(Vec::new());
            ai_busy.set(false);
        })
    };

    // Regenerate the last assistant turn: drop the trailing assistant bubble(s),
    // re-show "Thinking…", and re-send the last user message to the LIVE session
    // (which still holds full context). Last-turn-only, so there's nothing after it
    // to discard. No-op while busy or with no session / no prior user message.
    let ai_regenerate: Rc<dyn Fn()> = {
        let ai_session = ai_session.clone();
        let ai_send = ai_send.clone();
        Rc::new(move || {
            if ai_busy.get_untracked() {
                return;
            }
            let last_user = ai_messages.with_untracked(|v| {
                v.iter()
                    .rev()
                    .find(|m| m.role == Role::User)
                    .map(|m| m.text.clone())
            });
            let Some(text) = last_user else {
                return;
            };
            // Remove the last turn from the transcript: the trailing assistant/
            // error message(s) AND the user prompt itself (`ai_send` re-adds it).
            ai_messages.update(|v| {
                while v.last().is_some_and(|m| m.role != Role::User) {
                    v.pop();
                }
                v.pop(); // the user prompt being regenerated
            });
            // Drop the live session so the re-ask runs in a FRESH `claude` process
            // — a true regenerate. Re-sending into the existing session left the
            // discarded answer in the model's context, so it just rephrased it
            // (review §7.4). `ai_send` respawns the session (need_new). Trade-off:
            // like `ai_cancel`, this resets multi-turn context — acceptable since
            // regenerate targets the latest answer.
            ai_session.borrow_mut().take();
            (ai_send)(text);
        })
    };

    // Commit AI settings (called when the settings modal closes): drop the live
    // session so the next message respawns `claude` with the new model / effort /
    // CLI path, and persist the choices.
    let ai_apply: Rc<dyn Fn()> = {
        let ai_session = ai_session.clone();
        let save_ui = save_ui.clone();
        Rc::new(move || {
            // Only respawn if a session-affecting setting actually changed —
            // closing the modal with no change used to needlessly reset the live
            // conversation (review §7.4).
            let current = ai_settings_now();
            let changed = ai_session
                .borrow()
                .as_ref()
                .is_some_and(|s| s.settings != current);
            if changed {
                ai_session.borrow_mut().take();
            }
            save_ui();
        })
    };

    // Inline (Ctrl+K) editor AI: a one-shot `claude -p` generation, schema-aware,
    // returning bare SQL that the editor popup previews before Accept.
    let inline_ai: RwSignal<InlineAiState> = RwSignal::new(InlineAiState::Idle);
    // Holds the in-flight generation task so Cancel can abort it. The `claude`
    // child is spawned with `kill_on_drop`, so aborting the task drops the
    // `output()` future → the child is killed (no orphaned request).
    let inline_ai_task: Rc<RefCell<Option<tokio::task::JoinHandle<()>>>> =
        Rc::new(RefCell::new(None));
    let inline_ai_run: Rc<dyn Fn(InlineAiRequest)> = {
        let handle = handle.clone();
        let task_slot = inline_ai_task.clone();
        Rc::new(move |req: InlineAiRequest| {
            inline_ai.set(InlineAiState::Busy);
            // The active tab's database gets full column detail; others only when
            // a table is named in the buffer/intent.
            let active_db = tabs.with_untracked(|v| {
                v.iter()
                    .find(|t| t.id == active.get_untracked())
                    .and_then(|t| t.database.get_untracked())
            });
            let system = inline_system_prompt(db_nodes, active_db.as_deref(), &req);
            let intent = req.intent.clone();
            let bin = claude_bin(&ai_cli_path.get_untracked());
            // Follow the AI panel's model choice (one place to change it).
            let model = ai_model.get_untracked().cli().to_string();
            let send = create_ext_action(cx, move |state: InlineAiState| inline_ai.set(state));
            let jh = handle.spawn(async move {
                let out = Command::new(bin)
                    .arg("-p")
                    .arg(&intent)
                    .arg("--append-system-prompt")
                    .arg(&system)
                    .arg("--model")
                    .arg(&model)
                    .kill_on_drop(true)
                    .output()
                    .await;
                let state = match out {
                    Ok(o) if o.status.success() => {
                        let sql = extract_sql(&String::from_utf8_lossy(&o.stdout));
                        if sql.trim().is_empty() {
                            InlineAiState::Failed("No SQL returned".to_string())
                        } else {
                            InlineAiState::Ready(sql)
                        }
                    }
                    Ok(o) => InlineAiState::Failed(
                        String::from_utf8_lossy(&o.stderr)
                            .lines()
                            .next()
                            .unwrap_or("generation failed")
                            .to_string(),
                    ),
                    Err(e) => InlineAiState::Failed(e.to_string()),
                };
                send(state);
            });
            *task_slot.borrow_mut() = Some(jh);
        })
    };
    let inline_ai_cancel: Rc<dyn Fn()> = {
        let task_slot = inline_ai_task.clone();
        Rc::new(move || {
            if let Some(jh) = task_slot.borrow_mut().take() {
                jh.abort();
            }
            inline_ai.set(InlineAiState::Idle);
        })
    };

    // Keep `active_table` in sync with the active tab's source (for highlight).
    create_effect(move |_| {
        let id = active.get();
        let src = tabs.with(|v| v.iter().find(|t| t.id == id).and_then(|t| t.source.get()));
        active_table.set(src);
    });

    // Kick off schema load for the active connection.
    if let Some(conn) = cf.connections.iter().find(|c| c.id == active_id).cloned() {
        load_schema(conn);
    }

    // Health-check the active connection now, then re-check every 10s. The
    // timer re-arms itself; each tick reads the current active connection.
    fn arm_health_poll(check: Rc<dyn Fn()>) {
        floem::action::exec_after(std::time::Duration::from_secs(10), move |_| {
            check();
            arm_health_poll(check.clone());
        });
    }
    check_conn();
    arm_health_poll(check_conn.clone());

    // ── Terminal panel ──────────────────────────────────────────────────────
    // A shell on a PTY (schemaic-term). The reader thread notifies via a
    // crossbeam channel bridged into a Floem signal (`term_tick`); an effect
    // re-snapshots the grid into `term_screen`. The terminal lives in a RefCell
    // so the settings screen can respawn it with a different shell.
    let term_screen: RwSignal<schemaic_term::Screen> =
        RwSignal::new(schemaic_term::Screen::default());
    let term_focused = RwSignal::new(false);
    let term_settings_open = RwSignal::new(false);
    let detected_shells = schemaic_term::shell::detect_shells();
    let term_shells = RwSignal::new(detected_shells.clone());
    let term_dims = Rc::new(Cell::new((80u16, 24u16)));

    // Persisted shell preference → initial shell + which list row is selected.
    let term_prefs = persist::load_json::<schemaic_term::TerminalSettings>("terminal.json");
    let init_shell = term_prefs
        .shell
        .as_ref()
        .map(|p| p.config())
        .unwrap_or_else(schemaic_term::shell::default_shell);
    let init_selected = term_prefs
        .shell
        .as_ref()
        .and_then(|p| {
            detected_shells
                .iter()
                .position(|d| d.program == p.program && d.args == p.args)
        })
        .unwrap_or(0);
    let term_shell_selected = RwSignal::new(init_selected);
    // Terminal appearance/behaviour, restored from `terminal.json`.
    let term_font_size = RwSignal::new(term_prefs.font_size);
    let term_copy_on_select = RwSignal::new(term_prefs.copy_on_select);
    let term_cursor_style = RwSignal::new(TermCursor::from_key(&term_prefs.cursor_style));
    let term_cursor_blink = RwSignal::new(term_prefs.cursor_blink);
    // Blink phase; the cursor is shown when `!blink || blink_on` (and focused).
    let term_blink_on = RwSignal::new(true);

    let (term_tx, term_rx) = crossbeam_channel::unbounded::<()>();
    let term_tick = create_signal_from_channel(term_rx);
    let term_notify: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        let _ = term_tx.send(());
    });

    let terminal: Rc<RefCell<Option<schemaic_term::Terminal>>> = Rc::new(RefCell::new(
        schemaic_term::Terminal::spawn(&init_shell, 80, 24, term_notify.clone())
            .map_err(|e| tracing::error!("terminal spawn failed: {e}"))
            .ok(),
    ));

    // Re-snapshot on a notify tick, focus change, cursor-style change, or blink
    // phase. The cursor shows only while focused (and, if blinking, on-phase); a
    // block cursor is baked into the snapshot, bar/underline are drawn by the UI.
    {
        let terminal = terminal.clone();
        create_effect(move |_| {
            term_tick.get();
            let focused = term_focused.get();
            let blink = term_cursor_blink.get();
            let blink_on = term_blink_on.get();
            let style = term_cursor_style.get();
            let cursor_on = focused && (!blink || blink_on);
            let bake_block = matches!(style, TermCursor::Block);
            if let Some(t) = terminal.borrow().as_ref() {
                term_screen.set(t.snapshot(cursor_on, bake_block));
            }
        });
    }

    // Persist all terminal prefs (shell + appearance) as one file. Reading the
    // selected shell here keeps `terminal.json` whole when any field changes.
    let save_term_prefs: Rc<dyn Fn()> = Rc::new(move || {
        let shell = term_shells
            .get_untracked()
            .get(term_shell_selected.get_untracked())
            .cloned();
        persist::save_json(
            "terminal.json",
            &schemaic_term::TerminalSettings {
                shell,
                font_size: term_font_size.get_untracked(),
                copy_on_select: term_copy_on_select.get_untracked(),
                cursor_style: term_cursor_style.get_untracked().key().to_string(),
                cursor_blink: term_cursor_blink.get_untracked(),
            },
        );
    });
    // Save whenever an appearance/behaviour pref changes (the shell saves via
    // `term_apply_shell`, which respawns the terminal too).
    {
        let save = save_term_prefs.clone();
        create_effect(move |_| {
            term_font_size.get();
            term_copy_on_select.get();
            term_cursor_style.get();
            term_cursor_blink.get();
            save();
        });
    }

    // Cursor blink: a perpetual 530ms tick that flips the phase while focused and
    // blinking, and otherwise parks the cursor visible. Kept off the render path
    // when idle (it only notifies `term_blink_on` when the value actually flips).
    {
        let tick: BlinkTick = Rc::new(RefCell::new(None));
        let tick2 = tick.clone();
        *tick.borrow_mut() = Some(Rc::new(move || {
            // App shutting down disposes these signals; a still-pending tick would
            // then panic reading a freed signal. Bail (and stop rescheduling) once
            // any is gone.
            let (Some(is_focused), Some(blink)) = (
                term_focused.try_get_untracked(),
                term_cursor_blink.try_get_untracked(),
            ) else {
                return;
            };
            if is_focused && blink {
                term_blink_on.update(|b| *b = !*b);
            } else if term_blink_on.try_get_untracked() == Some(false) {
                term_blink_on.set(true);
            }
            let t = tick2.clone();
            floem::action::exec_after(std::time::Duration::from_millis(530), move |_| {
                if let Some(f) = t.borrow().as_ref() {
                    f();
                }
            });
        }));
        let t = tick.clone();
        floem::action::exec_after(std::time::Duration::from_millis(530), move |_| {
            if let Some(f) = t.borrow().as_ref() {
                f();
            }
        });
    }

    let term_input: Rc<dyn Fn(Vec<u8>)> = {
        let terminal = terminal.clone();
        Rc::new(move |bytes: Vec<u8>| {
            if let Some(t) = terminal.borrow().as_ref() {
                t.scroll_to_bottom();
                t.send_input(&bytes);
            }
        })
    };
    let term_resize: Rc<dyn Fn(u16, u16)> = {
        let terminal = terminal.clone();
        let term_dims = term_dims.clone();
        Rc::new(move |cols: u16, rows: u16| {
            term_dims.set((cols, rows));
            if let Some(t) = terminal.borrow().as_ref() {
                t.resize(cols, rows);
            }
        })
    };
    let term_scroll: Rc<dyn Fn(i32)> = {
        let terminal = terminal.clone();
        Rc::new(move |delta: i32| {
            if let Some(t) = terminal.borrow().as_ref() {
                t.scroll(delta);
            }
        })
    };
    let term_scroll_bottom: Rc<dyn Fn()> = {
        let terminal = terminal.clone();
        let term_notify = term_notify.clone();
        Rc::new(move || {
            if let Some(t) = terminal.borrow().as_ref() {
                t.scroll_to_bottom();
                (term_notify)();
            }
        })
    };
    // Restart: respawn the current shell (fresh session). The old terminal drops,
    // killing its PTY/child.
    let term_restart: Rc<dyn Fn()> = {
        let terminal = terminal.clone();
        let term_dims = term_dims.clone();
        let term_notify = term_notify.clone();
        Rc::new(move || {
            let cfg = term_shells
                .get_untracked()
                .get(term_shell_selected.get_untracked())
                .map(|p| p.config())
                .unwrap_or_else(schemaic_term::shell::default_shell);
            let (cols, rows) = term_dims.get();
            match schemaic_term::Terminal::spawn(&cfg, cols, rows, term_notify.clone()) {
                Ok(t) => {
                    *terminal.borrow_mut() = Some(t);
                    (term_notify)();
                }
                Err(e) => tracing::error!("terminal restart failed: {e}"),
            }
        })
    };
    // Open the DB CLI (mysql/mariadb) for the active connection in the terminal,
    // optionally scoped to a database. Reveals the terminal panel and respawns it
    // as a dedicated client session.
    let open_db_cli: Rc<dyn Fn(Option<String>)> = {
        let terminal = terminal.clone();
        let term_dims = term_dims.clone();
        let term_notify = term_notify.clone();
        let tunnels = tunnels.clone();
        Rc::new(move |db: Option<String>| {
            // Guard the panel reveal: a redundant `set` rebuilds the panel
            // `dyn_container` (CLAUDE.md gotcha / review H11).
            if !matches!(right_panel.get_untracked(), RightPanel::Terminal) {
                right_panel.set(RightPanel::Terminal);
            }
            let conn = connections.with_untracked(|cs| {
                cs.iter()
                    .find(|c| c.id == active_conn.get_untracked())
                    .cloned()
            });
            let Some(conn) = conn else {
                return;
            };
            // For an SSH connection, point the client at the local tunnel
            // (127.0.0.1:<port>), not the firewalled remote host (review H11). If
            // the tunnel isn't up yet, say so rather than silently failing.
            let conn = if conn.ssh.enabled {
                match tunnels.borrow().get(&conn.id).map(|h| h.port()) {
                    Some(port) => Connection {
                        host: "127.0.0.1".to_string(),
                        port,
                        ..conn
                    },
                    None => {
                        let cfg = message_shell(
                            "SSH tunnel is not established yet; try again in a moment.",
                        );
                        let (cols, rows) = term_dims.get();
                        if let Ok(t) =
                            schemaic_term::Terminal::spawn(&cfg, cols, rows, term_notify.clone())
                        {
                            *terminal.borrow_mut() = Some(t);
                            (term_notify)();
                        }
                        return;
                    }
                }
            } else {
                conn
            };
            let cfg = mysql_shell(&conn, db.as_deref())
                .unwrap_or_else(|| message_shell("No mysql/mariadb client found (PATH or WSL)."));
            let (cols, rows) = term_dims.get();
            match schemaic_term::Terminal::spawn(&cfg, cols, rows, term_notify.clone()) {
                Ok(t) => {
                    *terminal.borrow_mut() = Some(t);
                    (term_notify)();
                }
                Err(e) => tracing::error!("db cli spawn failed: {e}"),
            }
        })
    };
    let term_apply_shell: Rc<dyn Fn(usize)> = {
        let terminal = terminal.clone();
        let term_dims = term_dims.clone();
        let term_notify = term_notify.clone();
        let save_term_prefs = save_term_prefs.clone();
        Rc::new(move |idx: usize| {
            let Some(profile) = term_shells.get_untracked().get(idx).cloned() else {
                return;
            };
            let (cols, rows) = term_dims.get();
            match schemaic_term::Terminal::spawn(&profile.config(), cols, rows, term_notify.clone())
            {
                Ok(t) => {
                    *terminal.borrow_mut() = Some(t);
                    term_shell_selected.set(idx);
                    // Persist the whole prefs file (shell + appearance).
                    (save_term_prefs)();
                }
                Err(e) => tracing::error!("terminal respawn failed: {e}"),
            }
        })
    };
    let term_sel_start: Rc<dyn Fn(usize, usize)> = {
        let terminal = terminal.clone();
        Rc::new(move |row, col| {
            if let Some(t) = terminal.borrow().as_ref() {
                t.selection_start(row, col);
            }
        })
    };
    let term_sel_update: Rc<dyn Fn(usize, usize)> = {
        let terminal = terminal.clone();
        Rc::new(move |row, col| {
            if let Some(t) = terminal.borrow().as_ref() {
                t.selection_update(row, col);
            }
        })
    };
    let term_sel_clear: Rc<dyn Fn()> = {
        let terminal = terminal.clone();
        Rc::new(move || {
            if let Some(t) = terminal.borrow().as_ref() {
                t.selection_clear();
            }
        })
    };
    let term_copy: Rc<dyn Fn() -> Option<String>> = {
        let terminal = terminal.clone();
        Rc::new(move || terminal.borrow().as_ref().and_then(|t| t.selection_text()))
    };
    let term_paste: Rc<dyn Fn(String)> = {
        let terminal = terminal.clone();
        Rc::new(move |text: String| {
            if let Some(t) = terminal.borrow().as_ref() {
                t.paste(&text);
            }
        })
    };
    let term_open_link: Rc<dyn Fn(String)> = Rc::new(|url: String| open_url(&url));

    let ui = Ui {
        tabs_ui: TabsUi {
            tabs,
            active,
            flashing,
            active_db,
            active_db_menu_open,
            active_db_anchor,
        },
        tab_actions: Rc::new(TabsActions {
            run,
            run_all,
            cancel,
            commit_edits,
            add_tab,
            close_tab,
            open_table,
            open_table_new,
            open_query,
            set_active_db,
            open_db_cli,
            run_plan,
        }),
        overlay: OverlayUi {
            context_menu,
            popup_menu: RwSignal::new(None),
            popup_anchor: RwSignal::new(None),
            last_mouse,
            find_open,
            find_query,
            error_modal_open,
            error_modal_text: RwSignal::new(None),
            plan_open,
            plan_state,
            plan_sql,
            plan_analyze,
        },
        schema: SchemaUi {
            db_nodes,
            expanded,
            active_table,
            hidden_dbs,
            db_menu_open,
            schema_menu_open,
        },
        schema_actions: Rc::new(SchemaActions {
            on_toggle,
            toggle_db_hidden,
            collapse_all,
            collapse_db,
            refresh_schema,
            refresh_db,
        }),
        conn: ConnUi {
            connections,
            active_conn,
            conn_menu_open,
            conn_status,
            manage_open,
            draft,
            conn_test,
        },
        conn_actions: Rc::new(ConnActions {
            switch_conn,
            select_conn,
            new_conn,
            save_conn,
            delete_conn,
            test_conn,
        }),
        ai: AiUi {
            messages: ai_messages,
            input: ai_input,
            busy: ai_busy,
            settings_open: ai_settings_open,
            cli_path: ai_cli_path,
            model: ai_model,
            effort: ai_effort,
            instructions: ai_instructions,
            schema_scope: ai_schema_scope,
            run_queries: ai_run_queries,
            inline: inline_ai,
        },
        ai_actions: Rc::new(AiActions {
            send: ai_send,
            cancel: ai_cancel,
            new_chat: ai_new_chat,
            regenerate: ai_regenerate,
            apply: ai_apply,
            cli_ok: Rc::new(|p: String| claude_reachable(&p)),
            inline_run: inline_ai_run,
            inline_cancel: inline_ai_cancel,
            detected_path: ai_detected_path,
        }),
        history: HistoryUi {
            entries: history_entries,
        },
        history_actions: Rc::new(HistoryActions {
            clear: clear_history,
        }),
        term: TermUi {
            screen: term_screen,
            focused: term_focused,
            settings_open: term_settings_open,
            shells: term_shells,
            shell_selected: term_shell_selected,
            font_size: term_font_size,
            copy_on_select: term_copy_on_select,
            cursor_style: term_cursor_style,
            cursor_blink: term_cursor_blink,
        },
        term_actions: Rc::new(TermActions {
            input: term_input,
            resize: term_resize,
            scroll: term_scroll,
            scroll_bottom: term_scroll_bottom,
            restart: term_restart,
            apply_shell: term_apply_shell,
            sel_start: term_sel_start,
            sel_update: term_sel_update,
            sel_clear: term_sel_clear,
            copy: term_copy,
            paste: term_paste,
            open_link: term_open_link,
        }),
        layout: LayoutUi {
            schema_visible,
            right_panel,
            schema_w,
            right_w,
            editor_h,
            theme_settings_open,
            help_open,
            ui_theme,
            editor_theme,
            editor_font,
            tab_width,
            soft_tabs,
            word_wrap,
            row_limit,
            confirm_writes,
            restore_tabs,
        },
        persist_layout: save_ui.clone(),
        formats,
        save_formats,
    };
    schemaic_ui::workspace(ui)
}

/// Open an http(s) URL in the OS default browser (clicked terminal link).
fn open_url(url: &str) {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return;
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}
