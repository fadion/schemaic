// Release builds are GUI-subsystem on Windows, so launching the .exe doesn't pop
// a console window. Debug builds keep the console so `tracing` logs stay visible
// during development.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Schemaic — native SQL editor. Binary entry point.
//!
//! The app owns all the mutable state (tabs, saved connections, the loaded
//! schema) as signals in the root scope, plus the `Rc<dyn Fn>` callbacks the UI
//! invokes. A connection is a *server*; the schema sidebar lists all of the
//! active connection's databases. DB IO runs on the tokio runtime and results
//! are marshalled back through Floem's async→UI seam.

mod ai;
mod claude_cli;
mod heap;
mod mcp;
mod secrets;

/// Process-wide heap accounting (live/peak bytes), for leak-vs-retention
/// diagnosis. Delegates to the system allocator; only adds two atomics per
/// alloc. Logging is opt-in via `SCHEMAIC_HEAP_LOG` (see `heap::spawn_logger`).
#[global_allocator]
static GLOBAL: heap::Tracking = heap::Tracking;

use ai::{
    AiContextParams, AiSession, AiSettings, AiStreamMsg, RECAP_QUESTIONS, StartAiParams,
    active_tab_database, ai_context, apply_turn_delta, extract_sql, inline_system_prompt,
    mcp_endpoint_from_env, render_recap, start_ai_session, turn_context,
};
use claude_cli::{claude_bin, claude_reachable, detect_claude_bin};

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use floem::Application;
use floem::IntoView;
use floem::action::exec_after;
use floem::ext_event::create_ext_action;
use floem::ext_event::create_signal_from_channel;
use floem::kurbo::Size;
use floem::reactive::{
    RwSignal, Scope, SignalGet, SignalUpdate, SignalWith, create_effect, create_memo,
};
use floem::window::{Icon, WindowConfig};
use schemaic_core::connection::{ConnStatus, Connection};
use schemaic_core::edit::analyze_edit;
use schemaic_core::model::{CommitDone, GridWrite, QueryState, RefetchRequest, ResultSet};
use schemaic_core::monitor::{Snapshot, diff_snapshots};

/// Outcome of a background connect + schema-load task: `(tunnel port, tunnel
/// handle, database names)` on success, or an error message.
type ConnectResult = Result<
    (
        Option<u16>,
        Option<schemaic_db::ssh::TunnelHandle>,
        Vec<String>,
    ),
    String,
>;
/// Self-rescheduling cursor-blink tick — holds an `Rc` to itself so it can re-arm.
type BlinkTick = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

/// A closed tab's restorable state — plain data (no signals), so it outlives the
/// tab's disposed scope and can rebuild the tab on Ctrl+Shift+T.
#[derive(Clone)]
struct ClosedTab {
    query: String,
    conn_id: u64,
    database: Option<String>,
    source: Option<TableSource>,
    name: Option<String>,
    /// The original "Query N" number, restored on reopen when no live tab claims it.
    label: usize,
}
/// Record one executed query into history: `(conn_id, database, sql, tab_name)`.
type RecordHistoryFn = Rc<dyn Fn(u64, Option<String>, String, Option<String>)>;

/// Resolve the pinned session a tab's statements must run on: `Ok(None)` in
/// Auto-commit (fresh connection per op, as everywhere else), `Err` when the tab
/// is Manual but its connection isn't up.
type SessionForFn = Rc<dyn Fn(&Tab) -> Result<Option<Arc<Session>>, String>>;
/// End a tab's transaction — `(tab id, commit?, what to run once it's settled)`.
type EndTxFn = Rc<dyn Fn(usize, bool, Option<Rc<dyn Fn()>>)>;
/// Settle an open transaction before an action that would strand it —
/// `(tab id, the action to resume once it's settled)`.
type GuardTxFn = Rc<dyn Fn(usize, Rc<dyn Fn()>)>;
use schemaic_core::filter::{Order, table_query};
use schemaic_core::intel::SqlDialect;
use schemaic_core::persist::{self, ConnectionsFile, UiState};
use schemaic_core::schema::{SchemaState, TableSource};
use schemaic_core::tx::{StmtOutcome, TxEngine, TxMode, TxState};
use schemaic_db::{Db, DbError, Session};
use schemaic_ui::theme::{EditorThemeKind, UiThemeKind};
use schemaic_ui::{
    AiActions, AiEffort, AiModel, AiUi, ChatMessage, ConnActions, ConnNode, ConnUi, CtxMenu,
    DraftSignals, HistoryActions, HistoryUi, InlineAiRequest, InlineAiState, LayoutUi,
    MonitorEntry, OverlayUi, PlanState, ResultPanel, RightPanel, Role, SchemaActions, SchemaScope,
    SchemaUi, Tab, TabsActions, TabsUi, TermActions, TermCursor, TermUi, TestState, TxChoice,
    TxPrompt, Ui, pick_connection_color,
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
        let endpoint = mcp_endpoint_from_env();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio runtime");
        rt.block_on(mcp::serve(endpoint));
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

    // Opt-in heap logging (SCHEMAIC_HEAP_LOG=1) for memory diagnosis.
    heap::spawn_logger();

    // Register the bundled IBM Plex faces before any text is laid out.
    schemaic_ui::fonts::load_fonts();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime");
    let handle = rt.handle().clone();

    let mut config = WindowConfig::default()
        .size(Size::new(1280.0, 820.0))
        .title(schemaic_core::APP_NAME);
    if let Some(icon) = app_icon() {
        config = config.window_icon(icon);
    }

    Application::new()
        .window(move |_id| app_view(handle.clone()), Some(config))
        .run();

    drop(rt);
}

/// Decode the bundled PNG into a window icon (title bar / taskbar, both OSes).
/// Returns `None` if the image can't be decoded, in which case the window just
/// uses the platform default.
fn app_icon() -> Option<Icon> {
    let bytes = include_bytes!("../../../assets/icon.png");
    let img = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    Icon::from_rgba(img.into_raw(), w, h).ok()
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
    // Native client on PATH, else the WSL-side client.
    for prog in ["mysql", "mariadb"] {
        if which(prog).is_some() {
            return Some(mysql_shell_config(MysqlLauncher::Native(prog), conn, db));
        }
    }
    if which("wsl.exe").is_some() {
        return Some(mysql_shell_config(MysqlLauncher::Wsl, conn, db));
    }
    None
}

/// Which DB client `mysql_shell` resolved to run.
enum MysqlLauncher<'a> {
    /// A native `mysql`/`mariadb` on PATH.
    Native(&'a str),
    /// The WSL-side client via `wsl.exe -e mysql`.
    Wsl,
}

/// Build the launch config for the resolved DB client — pure argv + credential
/// env construction, split from the PATH probing in [`mysql_shell`] so it's
/// unit-tested. The password always rides `MYSQL_PWD` (forwarded across `WSLENV`
/// in the WSL case) and never lands on the argv.
fn mysql_shell_config(
    launcher: MysqlLauncher,
    conn: &schemaic_core::connection::Connection,
    db: Option<&str>,
) -> schemaic_term::ShellConfig {
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
    match launcher {
        MysqlLauncher::Native(prog) => schemaic_term::ShellConfig {
            program: prog.into(),
            args: cli_args,
            cwd: None,
            env: vec![("MYSQL_PWD".into(), conn.password.clone())],
        },
        MysqlLauncher::Wsl => {
            let mut args: Vec<String> = vec!["-e".into(), "mysql".into()];
            args.extend(cli_args);
            schemaic_term::ShellConfig {
                program: "wsl.exe".into(),
                args,
                cwd: None,
                env: vec![
                    ("WSLENV".into(), "MYSQL_PWD/u".into()),
                    ("MYSQL_PWD".into(), conn.password.clone()),
                ],
            }
        }
    }
}

/// Map a finished inline-AI (`Ctrl+K`) generation's output to a UI state: on
/// success, the fence-stripped SQL (or "No SQL returned" if blank); on failure,
/// the first stderr line. Pure so the parsing of untrusted subprocess output is
/// unit-tested (the closure keeps only the spawn + the spawn-error arm).
/// Settle the in-flight assistant bubble after the user stops a turn.
///
/// Keeps whatever partial answer had streamed in and adds a `(stopped)` marker.
/// The CLI reports an interrupted turn as an error `result`, so this also undoes
/// the error styling that would otherwise make a deliberate stop look like a
/// failure. Usage stats are left alone — the tokens were really spent.
fn mark_stopped(messages: RwSignal<Vec<ChatMessage>>) {
    messages.update(|v| {
        if let Some(last) = v.last_mut() {
            last.pending = false;
            last.role = Role::Assistant;
            last.segs
                .push(schemaic_core::transcript::Seg::Text("(stopped)".into()));
        }
    });
}

fn inline_outcome(success: bool, stdout: &[u8], stderr: &[u8]) -> InlineAiState {
    if success {
        let sql = extract_sql(&String::from_utf8_lossy(stdout));
        if sql.trim().is_empty() {
            InlineAiState::Failed("No SQL returned".to_string())
        } else {
            InlineAiState::Ready(sql)
        }
    } else {
        InlineAiState::Failed(
            String::from_utf8_lossy(stderr)
                .lines()
                .next()
                .unwrap_or("generation failed")
                .to_string(),
        )
    }
}

/// Convert a fetched sample `ResultSet` into seed `Row`s (col name → value; SQL
/// NULL → `None`) for the AI seed-data prompts. Shared by the fill + seed callbacks.
fn sample_rows(rs: &schemaic_core::model::ResultSet) -> Vec<schemaic_core::seed::Row> {
    let names: Vec<String> = rs.columns.iter().map(|c| c.name.clone()).collect();
    (0..rs.row_count())
        .map(|r| {
            (0..rs.col_count())
                .map(|c| {
                    let v = rs
                        .cell(r, c)
                        .and_then(|cell| (!cell.is_null()).then(|| cell.display().to_string()));
                    (names[c].clone(), v)
                })
                .collect()
        })
        .collect()
}

/// Look up a base table in the loaded schema, returning its `CREATE TABLE`
/// skeleton (prompt structure) + its primary-key column names (to order the
/// bottom-sample). Empty/`([], "")` when the schema hasn't been introspected yet.
/// Map a resolved `Db`'s engine to the SQL dialect (for dialect-aware DDL).
fn dialect_of(db: &Db) -> SqlDialect {
    if db.engine() == schemaic_db::Engine::Postgres {
        SqlDialect::Postgres
    } else {
        SqlDialect::MySql
    }
}

fn table_ddl_and_pk(
    db_nodes: RwSignal<Vec<ConnNode>>,
    source: &TableSource,
    dialect: SqlDialect,
) -> (String, Vec<String>) {
    db_nodes
        .with_untracked(|nodes| {
            nodes
                .iter()
                .find(|n| n.database == source.database)
                .and_then(|n| match n.schema.get_untracked() {
                    schemaic_core::schema::SchemaState::Loaded(s) => s
                        .find_table(source.schema.as_deref(), &source.table)
                        .map(|t| {
                            let pk = t
                                .columns
                                .iter()
                                .filter(|c| c.primary_key)
                                .map(|c| c.name.clone())
                                .collect();
                            (t.create_ddl(dialect), pk)
                        }),
                    _ => None,
                })
        })
        .unwrap_or_default()
}

/// The bottom-sample query for AI seed data: most-recent rows by primary key
/// (`ORDER BY <pk> DESC`) so enums/sequences/FK values are representative.
fn sample_sql(engine: schemaic_db::Engine, source: &TableSource, pk_cols: &[String]) -> String {
    table_query(
        dialect_for(engine),
        &source.database,
        source.schema.as_deref(),
        &source.table,
        pk_cols,
        Order::Desc,
        AI_SAMPLE_ROWS,
    )
}

/// Rows the AI seed sample reads from the bottom of a table.
const AI_SAMPLE_ROWS: usize = 20;
/// Rows a freshly-opened table tab shows.
const TABLE_TAB_ROWS: usize = 100;

/// The engine's SQL dialect — the two enums are parallel (one is the driver, the
/// other the parser/quoting rules).
fn dialect_for(engine: schemaic_db::Engine) -> SqlDialect {
    match engine {
        schemaic_db::Engine::Postgres => SqlDialect::Postgres,
        schemaic_db::Engine::MySql => SqlDialect::MySql,
    }
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

/// Smallest positive "Query N" number not present in `used` (a tab's display
/// number, its `label`). New tabs pick the lowest free number so closing and
/// opening keeps numbering compact instead of climbing forever — the display
/// number is decoupled from the ever-incrementing tab `id`.
/// The "Query N" numbers already taken **on one connection**.
///
/// Numbering is per connection because everything else about a tab is: the
/// strip, history, the AI conversation. A brand-new connection opening on
/// "Query 10" — or skipping 5 because another connection holds it — reads as a
/// bug, since those tabs are never on screen together.
fn used_labels(tabs: &[Tab], conn: u64) -> Vec<usize> {
    tabs.iter()
        .filter(|t| t.conn_id.get_untracked() == conn)
        .map(|t| t.label)
        .collect()
}

fn smallest_free_label(used: &[usize]) -> usize {
    let mut n = 1;
    while used.contains(&n) {
        n += 1;
    }
    n
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
        environment: Default::default(),
    }
}

/// Which engine's transaction semantics apply — the divergence that
/// [`schemaic_core::tx`] encodes (Postgres poisons a transaction on any error;
/// MySQL implicitly commits on DDL).
fn tx_engine(db: &Db) -> TxEngine {
    match db.engine() {
        schemaic_db::Engine::Postgres => TxEngine::Postgres,
        schemaic_db::Engine::MySql => TxEngine::MySql,
    }
}

fn app_view(handle: tokio::runtime::Handle) -> impl IntoView {
    let cx = Scope::current();

    // Load (or seed) saved connections. Secrets are hydrated from the OS keyring
    // (and any legacy plaintext migrated into it) by `secrets::load_connections`.
    let mut cf = secrets::load_connections();
    if cf.connections.is_empty() {
        let seed = seed_connection();
        cf.active = Some(seed.id);
        cf.connections.push(seed);
        secrets::save_connections(&cf);
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
            secrets::save_connections(&cf);
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

    // Find-Anywhere per-connection search history. The overlay records activations
    // and reads recents directly on the signal; this effect persists on change.
    let search_history = RwSignal::new(
        persist::load_json::<schemaic_core::search_history::SearchHistoryFile>(
            "search_history.json",
        )
        .entries,
    );
    create_effect(move |_| {
        let entries = search_history.get();
        persist::save_json(
            "search_history.json",
            &schemaic_core::search_history::SearchHistoryFile { entries },
        );
    });

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

    // Per-database identity colours (persisted, keyed by connection+database;
    // set from the schema tree's right-click menu, shown as a dot on the DB node,
    // the active-DB selector, and the database's query tabs).
    let db_colors = RwSignal::new(
        persist::load_json::<schemaic_core::db_color::DbColorsFile>("db_colors.json").rules,
    );
    let save_db_colors: Rc<dyn Fn()> = Rc::new(move || {
        persist::save_json(
            "db_colors.json",
            &schemaic_core::db_color::DbColorsFile {
                rules: db_colors.get_untracked(),
            },
        );
    });
    // Favorited (bookmarked) databases — same standalone-file pattern as colours.
    let db_favorites = RwSignal::new(
        persist::load_json::<schemaic_core::favorite::FavoritesFile>("favorites.json").rules,
    );
    let save_db_favorites: Rc<dyn Fn()> = Rc::new(move || {
        persist::save_json(
            "favorites.json",
            &schemaic_core::favorite::FavoritesFile {
                rules: db_favorites.get_untracked(),
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
            // Running "Query N" counter per connection, for the labels below.
            let mut per_conn: HashMap<u64, usize> = HashMap::new();
            let mut built: Vec<Tab> =
                saved_tabs
                    .tabs
                    .iter()
                    .enumerate()
                    .map(|(i, s)| {
                        let conn = if cf.connections.iter().any(|c| c.id == s.conn_id) {
                            s.conn_id
                        } else {
                            active_id
                        };
                        let mut t = Tab::new(cx, i + 1, &s.query, conn, s.database.clone());
                        // Numbering restarts per connection (labels aren't
                        // persisted — they're always derived on restore), so a
                        // connection's tabs come back as Query 1..N rather than
                        // carrying the whole file's running count.
                        t.label = *per_conn.entry(conn).and_modify(|n| *n += 1).or_insert(1);
                        t.source.set(s.source.clone().map(|(db, table)| {
                            TableSource::new(db, s.source_schema.clone(), table)
                        }));
                        t.name.set(s.name.clone());
                        t.pinned.set(s.pinned);
                        t
                    })
                    .collect();
            let n = built.len();
            let active_id = built[saved_tabs.active.min(n - 1)].id;
            // Enforce the pinned-first invariant (stable, so pin order + relative
            // unpinned order both survive) in case the file was hand-edited.
            built.sort_by_key(|t| !t.pinned.get_untracked());
            (built, active_id, n + 1)
        };
    // `tabs.json` and `connections.json` restore independently, so the saved
    // active tab can belong to a connection other than the saved active one —
    // and the strip only shows the active connection's tabs. Land on one of
    // them, opening a tab when this connection has none (there's no
    // empty-editor state).
    let (mut initial_tabs, initial_active, first_free_id) =
        (initial_tabs, initial_active, first_free_id);
    let (initial_active, first_free_id) = {
        let refs: Vec<(usize, u64)> = initial_tabs
            .iter()
            .map(|t| (t.id, t.conn_id.get_untracked()))
            .collect();
        match schemaic_core::tabsel::pick_active(&refs, active_id, Some(initial_active)) {
            Some(id) => (id, first_free_id),
            None => {
                let used = used_labels(&initial_tabs, active_id);
                let mut t = Tab::new(cx, first_free_id, "", active_id, None);
                t.label = smallest_free_label(&used);
                let id = t.id;
                initial_tabs.push(t);
                (id, first_free_id + 1)
            }
        }
    };
    let tabs = RwSignal::new(initial_tabs);
    let active = RwSignal::new(initial_active);
    let next_id = Rc::new(Cell::new(first_free_id));
    // Ring of recently-closed tabs (most-recent first, capped) for Ctrl+Shift+T.
    // Plain `ClosedTab` data so entries survive the closed tab's scope disposal.
    let recently_closed: Rc<RefCell<VecDeque<ClosedTab>>> = Rc::new(RefCell::new(VecDeque::new()));
    let flashing: RwSignal<Option<usize>> = RwSignal::new(None);
    // Where the user last was on each connection (connection id → tab id), so
    // switching away and back returns to that tab instead of the first one.
    // Runtime only — on launch every connection falls back to its first tab.
    let last_tab: Rc<RefCell<HashMap<u64, usize>>> = Rc::new(RefCell::new(HashMap::new()));
    // Per-tab in-flight query token, tagged with a monotonic run generation so a
    // completing run can tell whether it still owns the tab's slot (a newer run
    // or a tab close supersedes it) before touching `tokens`/`results`.
    let tokens: Rc<RefCell<HashMap<usize, (u64, CancellationToken)>>> =
        Rc::new(RefCell::new(HashMap::new()));
    let run_gen = Rc::new(Cell::new(0u64));

    // Pinned connections for tabs in manual-transaction mode: tab id → session.
    // Absent for every tab in Auto-commit (the default), which keeps using a
    // fresh connection per operation — the session map is the *only* place this
    // app holds a connection open across UI actions. Entries are created lazily
    // on a Manual tab's first statement and removed (rolled back + closed) when
    // the tab leaves Manual, closes, or its connection goes away.
    let sessions: Rc<RefCell<HashMap<usize, Arc<Session>>>> = Rc::new(RefCell::new(HashMap::new()));

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

    // The pinned session a tab's statements must run on, or `None` when the tab
    // is in Auto-commit and every op gets its own fresh connection.
    //
    // `Err` means the tab *is* in Manual but its connection isn't up yet — the
    // session opens asynchronously when the mode is switched, so there's a brief
    // window (and a permanent one if that open failed). Running the statement on
    // a fresh connection instead would silently escape the transaction the user
    // asked for, so this refuses rather than guesses.
    let session_for: SessionForFn = {
        let sessions = sessions.clone();
        Rc::new(move |tab: &Tab| match tab.tx_mode.get_untracked() {
            TxMode::Auto => Ok(None),
            TxMode::Manual => match sessions.borrow().get(&tab.id).cloned() {
                Some(s) => Ok(Some(s)),
                None => Err(
                    "the transaction connection isn't ready — switch to Auto-commit and back"
                        .to_string(),
                ),
            },
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
    // Editor-collapse toggle (RESULTS "expand" icon). Session-only — always starts
    // expanded. `editor_h` is the restore height; collapsing sets the editor height
    // to 0 (instant).
    let editor_collapsed: RwSignal<bool> = RwSignal::new(false);
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
    let live_validate = RwSignal::new(ui_state.live_validate);
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
    let active_table: RwSignal<Option<TableSource>> = RwSignal::new(None);

    // Manage-connections form + overlay signals.
    let draft = DraftSignals::new(cx);
    let conn_menu_open = RwSignal::new(false);
    let manage_open = RwSignal::new(false);
    let conn_test = RwSignal::new(TestState::Idle);
    let find_open = RwSignal::new(false);
    let find_query = RwSignal::new(String::new());
    let error_modal_open = RwSignal::new(false);
    let error_modal_text: RwSignal<Option<String>> = RwSignal::new(None);
    let conn_status = RwSignal::new(ConnStatus::Unknown);
    // Pending "you have an open transaction" question (see `TxPrompt`).
    let tx_prompt: RwSignal<Option<TxPrompt>> = RwSignal::new(None);

    // Query-plan (EXPLAIN) modal signals.
    let plan_open = RwSignal::new(false);
    let plan_state = RwSignal::new(PlanState::Idle);
    let plan_sql = RwSignal::new(String::new());
    let plan_analyze = RwSignal::new(false);
    // Live Monitor modal state (rendered by `schemaic_ui::monitor_view`; polled by
    // the `open_monitor` action + `monitor_tick` loop below).
    let monitor_open = RwSignal::new(false);
    let monitor_title: RwSignal<Option<String>> = RwSignal::new(None);
    let monitor_cols: RwSignal<Vec<String>> = RwSignal::new(Vec::new());
    let monitor_log: RwSignal<Vec<MonitorEntry>> = RwSignal::new(Vec::new());
    let monitor_error: RwSignal<Option<String>> = RwSignal::new(None);
    let monitor_interval: RwSignal<u64> = RwSignal::new(MONITOR_INTERVAL_SECS);

    // AI panel state. `ai_session` holds the live CLI conversation (bound to a
    // connection); the reader task streams transcript snapshots over a channel
    // into `ai_stream`, which an effect applies to `ai_messages`.
    let ai_messages: RwSignal<Vec<ChatMessage>> = RwSignal::new(Vec::new());
    let ai_input = RwSignal::new(String::new());
    let ai_busy = RwSignal::new(false);
    let ai_session: Rc<RefCell<Option<AiSession>>> = Rc::new(RefCell::new(None));
    // True between pressing Stop and the interrupted turn's `result` landing.
    // The CLI reports that result as an error; this says it was us.
    let ai_stopping = RwSignal::new(false);
    // Saved conversations (`chats.json`), keyed by connection like the panel
    // itself. Seeded into `ai_messages` for the active connection below, once
    // the restored connection id is known.
    let saved_chats: RwSignal<Vec<schemaic_core::chat::SavedChat>> =
        RwSignal::new(persist::load_json::<schemaic_core::chat::ChatFile>("chats.json").chats);
    // Restore the active connection's conversation at launch (the switch path
    // does the same on every later change). Marked seen first so the whole
    // conversation doesn't play the new-message entrance animation.
    let restored =
        schemaic_core::chat::for_conn(&saved_chats.get_untracked(), active_conn.get_untracked());
    schemaic_ui::mark_messages_seen(restored.len());
    ai_messages.set(restored);
    // Store the panel's current conversation under `conn_id` and write the file.
    // Called when a turn finishes and when a conversation is cleared — never
    // mid-stream, so a half-written turn can't reach disk.
    let persist_chat: Rc<dyn Fn(u64)> = Rc::new(move |conn_id: u64| {
        saved_chats.update(|chats| {
            schemaic_core::chat::save(chats, conn_id, &ai_messages.get_untracked());
        });
        persist::save_json(
            "chats.json",
            &schemaic_core::chat::ChatFile {
                chats: saved_chats.get_untracked(),
            },
        );
    });
    let (ai_tx, ai_rx) = crossbeam_channel::unbounded::<AiStreamMsg>();
    let ai_stream = create_signal_from_channel(ai_rx);

    // Record an executed query into the history (newest-first, capped) and persist
    // it. Called from every run path (single Run, Run Current, Run Everything).
    let record_history: RecordHistoryFn = {
        Rc::new(
            move |conn_id: u64, database: Option<String>, sql: String, tab_name: Option<String>| {
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
                            tab_name,
                        },
                    );
                });
                persist::save_json(
                    "history.json",
                    &schemaic_core::history::HistoryFile {
                        entries: history_entries.get_untracked(),
                    },
                );
            },
        )
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
    // Shared execution engine for both a manual run and a filter/sort re-run
    // (`apply_view`). `is_view` distinguishes them: a manual run records history and
    // drives the whole results pane (Running → Loaded/Failed), whereas a view re-run
    // keeps the current table visible and, on error, surfaces the message in the
    // grid's bottom bar (`tab.view_err`) instead of replacing the grid.
    let run_query_core: Rc<dyn Fn(String, bool)> = {
        let handle = handle.clone();
        let tokens = tokens.clone();
        let run_gen = run_gen.clone();
        let db_for = db_for.clone();
        let session_for = session_for.clone();
        let record_history = record_history.clone();
        Rc::new(move |sql: String, is_view: bool| {
            if sql.trim().is_empty() {
                return;
            }
            let id = active.get_untracked();
            let Some(tab) = tabs.with_untracked(|v| v.iter().find(|t| t.id == id).copied()) else {
                return;
            };
            let results = tab.results;
            let view_err = tab.view_err;
            let load_gen = tab.load_gen;
            // Resolve this tab's own connection (not necessarily the active one).
            let db = match db_for(tab.conn_id.get_untracked()) {
                Ok(db) => db,
                Err(e) => {
                    if is_view {
                        view_err.set(Some(e));
                    } else {
                        tab.result_tabs.set(Vec::new());
                        results.set(QueryState::Failed(e));
                    }
                    return;
                }
            };
            // In Manual mode the statement runs on the tab's pinned connection,
            // inside the transaction, instead of on a fresh one.
            let session = match session_for(&tab) {
                Ok(s) => s,
                Err(e) => {
                    if is_view {
                        view_err.set(Some(e));
                    } else {
                        tab.result_tabs.set(Vec::new());
                        results.set(QueryState::Failed(e));
                    }
                    return;
                }
            };
            let database = tab.database.get_untracked();
            if !is_view {
                (record_history)(
                    tab.conn_id.get_untracked(),
                    database.clone(),
                    sql.clone(),
                    tab.name.get_untracked(),
                );
            }

            if let Some((_, old)) = tokens.borrow_mut().remove(&id) {
                old.cancel();
            }
            let token = CancellationToken::new();
            let generation = run_gen.get() + 1;
            run_gen.set(generation);
            tokens.borrow_mut().insert(id, (generation, token.clone()));
            if is_view {
                // Keep the current table on screen during the re-run; a fresh attempt
                // clears any prior filter error.
                view_err.set(None);
            } else {
                // A single run reverts the results pane to the one-grid view (any
                // prior Run Everything tabs are cleared).
                tab.result_tabs.set(Vec::new());
                results.set(QueryState::Running);
            }

            let tokens_done = tokens.clone();
            let tx_sql = sql.clone();
            let engine = tx_engine(&db);
            let send = create_ext_action(
                cx,
                move |(state, stmt): (QueryState, Option<StmtOutcome>)| {
                    // Fold the transaction state first, and unconditionally: it
                    // tracks the *connection*, so it stays true even when a newer run
                    // has superseded this one for display purposes.
                    if let Some(stmt) = stmt {
                        tab.tx
                            .update(|t| *t = t.on_statement(engine, &tx_sql, stmt));
                    }
                    // Only apply if this run still owns the tab (else a newer run or
                    // a close superseded it — don't clobber their state/token).
                    if tokens_done.borrow().get(&id).map(|(g, _)| *g) != Some(generation) {
                        return;
                    }
                    tokens_done.borrow_mut().remove(&id);
                    if is_view {
                        match state {
                            // Success → swap in the filtered result, then bump the load
                            // nonce so the grid rebuilds despite Loaded→Loaded. Order
                            // matters: the rebuild reads `results` untracked, so the new
                            // Arc must already be in place before the nonce changes.
                            QueryState::Loaded(_) => {
                                results.set(state);
                                load_gen.update(|g| *g = g.wrapping_add(1));
                                view_err.set(None);
                            }
                            // Error → keep the current table, show the message in the bar.
                            QueryState::Failed(m) => view_err.set(Some(m)),
                            // Cancelled/superseded → leave the table + error untouched.
                            _ => {}
                        }
                    } else {
                        results.set(state);
                    }
                },
            );
            // Read the row cap on the UI thread (signals are single-threaded).
            let cap = row_limit.get_untracked();
            // `BEGIN` is issued lazily, on the first statement of a transaction,
            // so flipping to Manual and changing your mind costs nothing.
            let needs_begin = session.is_some() && !tab.tx.get_untracked().is_open();
            handle.spawn(async move {
                let (res, stmt) = match &session {
                    Some(s) => {
                        if needs_begin && let Err(e) = s.begin().await {
                            tracing::error!("BEGIN failed: {e}");
                        }
                        let out = s.fetch_query(&sql, cap, token).await;
                        (out.result, Some(out.stmt))
                    }
                    None => (
                        db.fetch_query(database.as_deref(), &sql, cap, token).await,
                        None,
                    ),
                };
                let state = match res {
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
                send((state, stmt));
            });
        })
    };

    // A manual run (Ctrl+Enter / Run): records history, captures the SQL as the
    // grid filter/sort base, and clears any active filter/sort so the fresh result
    // starts unfiltered.
    let run: Rc<dyn Fn(String)> = {
        let core = run_query_core.clone();
        Rc::new(move |sql: String| {
            if sql.trim().is_empty() {
                return;
            }
            let id = active.get_untracked();
            if let Some(tab) = tabs.with_untracked(|v| v.iter().find(|t| t.id == id).copied()) {
                tab.base_sql.set(Some(sql.clone()));
                tab.grid_query
                    .set(schemaic_core::filter::GridQuery::default());
                tab.view_err.set(None);
            }
            core(sql, false);
        })
    };

    // A filter/sort re-run: keeps the current table until the filtered result lands
    // (or an error shows in the grid's bottom bar), without recording history or
    // disturbing `base_sql`/`grid_query` (the grid owns those).
    let apply_view: Rc<dyn Fn(String)> = {
        let core = run_query_core.clone();
        Rc::new(move |sql: String| core(sql, true))
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

    // Tier-2 live validation: PREPARE the statement under the cursor against the
    // real DB (no execution) and hand back the diagnostics. Staleness is handled
    // caller-side (the editor's debounce generation), so no cancellation token is
    // needed here — a superseded result is simply ignored by the caller.
    let validate_stmt: schemaic_ui::ValidateFn = {
        let handle = handle.clone();
        let db_for = db_for.clone();
        Rc::new(
            move |sql: String, lo: usize, hi: usize, on_done: schemaic_ui::ValidateDoneFn| {
                let id = active.get_untracked();
                let Some(tab) = tabs.with_untracked(|v| v.iter().find(|t| t.id == id).copied())
                else {
                    return;
                };
                let db = match db_for(tab.conn_id.get_untracked()) {
                    Ok(db) => db,
                    Err(_) => return, // can't resolve a connection → skip silently
                };
                let database = tab.database.get_untracked();
                let send =
                    create_ext_action(cx, move |diags: Vec<schemaic_core::intel::Diagnostic>| {
                        on_done(diags)
                    });
                handle.spawn(async move {
                    let stmt = sql.get(lo..hi).unwrap_or("").to_string();
                    let diags = match db.prepare_check(database.as_deref(), &stmt).await {
                        Ok(()) => Vec::new(),
                        Err(e) => vec![schemaic_core::intel::db_error_diagnostic(
                            &sql,
                            lo,
                            hi,
                            &e.to_string(),
                        )],
                    };
                    send(diags);
                });
            },
        )
    };

    // Live Monitor: poll a table on an interval, diffing each snapshot against the
    // previous to log inserts/updates/deletes (poll-only-while-open — closing the
    // modal sets `monitor_open` false, which stops the loop). This state persists
    // across ticks and reopens: `monitor_gen` supersedes a stale in-flight fetch on
    // reopen, `monitor_prev` is the last snapshot, `monitor_key_cols` the identity.
    let monitor_gen: Rc<Cell<u64>> = Rc::new(Cell::new(0));
    let monitor_prev: Rc<RefCell<Option<Snapshot>>> = Rc::new(RefCell::new(None));
    let monitor_key_cols: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));
    let open_monitor: schemaic_ui::MonitorFn = {
        let handle = handle.clone();
        let db_for = db_for.clone();
        let monitor_gen = monitor_gen.clone();
        let monitor_prev = monitor_prev.clone();
        let monitor_key_cols = monitor_key_cols.clone();
        Rc::new(move |conn_id: u64, source: TableSource| {
            // Fresh session: bump the generation (kills any stale tick), reset state,
            // reveal the modal.
            let g = monitor_gen.get().wrapping_add(1);
            monitor_gen.set(g);
            *monitor_prev.borrow_mut() = None;
            *monitor_key_cols.borrow_mut() = Vec::new();
            monitor_log.set(Vec::new());
            monitor_cols.set(Vec::new());
            monitor_error.set(None);
            monitor_title.set(Some(format!("{}.{}", source.database, source.display())));
            monitor_open.set(true);
            let ctx = MonitorCtx {
                handle: handle.clone(),
                db_for: db_for.clone(),
                db_nodes,
                cx,
                open: monitor_open,
                cols: monitor_cols,
                log: monitor_log,
                error: monitor_error,
                prev: monitor_prev.clone(),
                key_cols: monitor_key_cols.clone(),
                generation: monitor_gen.clone(),
                started: Instant::now(),
                target: (conn_id, source),
                interval: monitor_interval,
            };
            monitor_tick(ctx, g);
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
        let session_for = session_for.clone();
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
            let session = match session_for(&tab) {
                Ok(s) => s,
                Err(e) => {
                    tab.results.set(QueryState::Failed(e));
                    return;
                }
            };
            let database = tab.database.get_untracked();
            // Record each statement (oldest first, so the batch lands newest-last).
            let conn_id = tab.conn_id.get_untracked();
            let tab_name = tab.name.get_untracked();
            for s in &stmts {
                (record_history)(conn_id, database.clone(), s.clone(), tab_name.clone());
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
            let engine = tx_engine(&db);
            // The batch's effect on the transaction is folded per statement, in
            // order — a MySQL DDL halfway through implicitly commits, and the
            // statements after it belong to a *new* transaction.
            let tx_stmts = stmts.clone();
            let send = create_ext_action(
                cx,
                move |(states, outcomes): (Vec<QueryState>, Vec<Option<StmtOutcome>>)| {
                    for (sql, stmt) in tx_stmts.iter().zip(&outcomes) {
                        if let Some(stmt) = stmt {
                            tab.tx.update(|t| *t = t.on_statement(engine, sql, *stmt));
                        }
                    }
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
                },
            );
            let cap = row_limit.get_untracked();
            let needs_begin = session.is_some() && !tab.tx.get_untracked().is_open();
            handle.spawn(async move {
                let mut states: Vec<QueryState> = vec![QueryState::Cancelled; n];
                let mut outcomes: Vec<Option<StmtOutcome>> = vec![None; n];
                match &session {
                    Some(s) => {
                        if needs_begin && let Err(e) = s.begin().await {
                            tracing::error!("BEGIN failed: {e}");
                        }
                        // The session runs statements one at a time on the pinned
                        // connection, so each outcome is collected as it lands.
                        let mut stopped = false;
                        for (i, sql) in stmts.iter().enumerate() {
                            if stopped || token.is_cancelled() {
                                states[i] = QueryState::Cancelled;
                                continue;
                            }
                            let out = s.fetch_query(sql, cap, token.clone()).await;
                            outcomes[i] = Some(out.stmt);
                            states[i] = match out.result {
                                Ok(rs) => QueryState::Loaded(Arc::new(rs)),
                                Err(DbError::Cancelled) => {
                                    stopped = true;
                                    QueryState::Cancelled
                                }
                                Err(e) => {
                                    stopped = true;
                                    QueryState::Failed(e.to_string())
                                }
                            };
                        }
                    }
                    None => {
                        db.run_batch(database.as_deref(), &stmts, cap, token, |i, res| {
                            states[i] = match res {
                                Ok(rs) => QueryState::Loaded(Arc::new(rs)),
                                Err(DbError::Cancelled) => QueryState::Cancelled,
                                Err(e) => QueryState::Failed(e.to_string()),
                            };
                        })
                        .await;
                    }
                }
                send((states, outcomes));
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
                // In Manual mode the edits join the tab's transaction (nested
                // under a savepoint) instead of committing on their own.
                let session = match session_for(&tab) {
                    Ok(s) => s,
                    Err(e) => {
                        (done)(CommitDone::Failed(e));
                        return;
                    }
                };
                let query = tab.query.get_untracked();
                let run = run.clone();
                let engine = tx_engine(&db);
                let fold = create_ext_action(cx, move |stmt: StmtOutcome| {
                    // A write batch is one unit as far as the transaction is
                    // concerned, so it folds as a single statement.
                    tab.tx
                        .update(|t| *t = t.on_statement(engine, "UPDATE", stmt));
                });
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
                let needs_begin = session.is_some() && !tab.tx.get_untracked().is_open();
                handle.spawn(async move {
                    let token = CancellationToken::new();
                    // Both branches write the rows and then, on success, re-read
                    // them. The session branch keeps both on the pinned
                    // connection — a fresh one couldn't see rows the transaction
                    // hasn't committed.
                    let written = match &session {
                        Some(s) => {
                            if needs_begin && let Err(e) = s.begin().await {
                                tracing::error!("BEGIN failed: {e}");
                            }
                            let out = s.commit_writes(&write, token.clone()).await;
                            fold(out.stmt);
                            out.result
                        }
                        None => db.commit_writes(&write, token.clone()).await,
                    };
                    if let Err(e) = written {
                        tracing::error!("commit failed: {e}");
                        finish(CommitDone::Failed(e.to_string()));
                        return;
                    }
                    match refetch {
                        // Splice path: re-fetch just the edited rows. If that
                        // fails, fall back to a full re-run (data is committed).
                        Some(req) => {
                            let rows = match &session {
                                Some(s) => {
                                    s.refetch_rows(&req.template, &req.rows, token).await.result
                                }
                                None => db.refetch_rows(&req.template, &req.rows, token).await,
                            };
                            match rows {
                                Ok(rows) => finish(CommitDone::Spliced(rows)),
                                Err(e) => {
                                    tracing::warn!("re-fetch after commit failed: {e}");
                                    finish(CommitDone::FullReran);
                                }
                            }
                        }
                        None => finish(CommitDone::FullReran),
                    }
                });
            },
        )
    };

    // ── Manual-transaction controls ──────────────────────────────────────────
    // Throw away a tab's pinned session, rolling back anything still open. Used
    // whenever the tab stops being a Manual tab: mode switch, close, disconnect.
    // The rollback is spawned, never awaited — the UI thread must not block on
    // the network, and the server rolls back on disconnect regardless.
    let drop_session: Rc<dyn Fn(usize)> = {
        let sessions = sessions.clone();
        let handle = handle.clone();
        Rc::new(move |tab_id: usize| {
            if let Some(s) = sessions.borrow_mut().remove(&tab_id) {
                handle.spawn(async move {
                    let _ = s.rollback().await;
                    s.close().await;
                });
            }
        })
    };

    // COMMIT or ROLLBACK a tab's transaction, then optionally resume whatever
    // was waiting on the answer (the `TxPrompt` continuation). The tab stays in
    // Manual and its session stays open, ready for the next transaction.
    let end_tx: EndTxFn = {
        let sessions = sessions.clone();
        let handle = handle.clone();
        Rc::new(
            move |tab_id: usize, commit: bool, then: Option<Rc<dyn Fn()>>| {
                let Some(tab) = tabs.with_untracked(|v| v.iter().find(|t| t.id == tab_id).copied())
                else {
                    return;
                };
                let Some(session) = sessions.borrow().get(&tab_id).cloned() else {
                    // No session: nothing to end, but the state machine may still
                    // be showing a lost transaction — clear it and carry on.
                    tab.tx.set(TxState::closed());
                    if let Some(then) = then {
                        then();
                    }
                    return;
                };
                let done = create_ext_action(cx, move |err: Option<String>| {
                    match err {
                        // Even a failed COMMIT/ROLLBACK leaves no usable
                        // transaction — the server has ended it or the connection
                        // is gone — so the state resets either way; the message is
                        // what the user acts on.
                        Some(msg) => {
                            tab.tx.set(TxState::closed());
                            error_modal_text.set(Some(msg));
                            error_modal_open.set(true);
                        }
                        None => tab.tx.set(TxState::closed()),
                    }
                    if let Some(then) = then.clone() {
                        then();
                    }
                });
                handle.spawn(async move {
                    let r = if commit {
                        session.commit().await
                    } else {
                        session.rollback().await
                    };
                    done(r.err().map(|e| e.to_string()));
                });
            },
        )
    };

    // Ask about an open transaction before doing something that would strand it.
    // `proceed` runs once the transaction is settled (or immediately when there
    // is none); Cancel drops it entirely. Every path that can orphan a
    // transaction — mode switch, tab close, database switch — goes through here,
    // so the UI never has to remember to ask.
    let guard_tx: GuardTxFn = {
        let end_tx = end_tx.clone();
        Rc::new(move |tab_id: usize, proceed: Rc<dyn Fn()>| {
            let found = tabs.with_untracked(|v| {
                v.iter()
                    .find(|t| t.id == tab_id)
                    .map(|t| (t.tx.get_untracked(), t.title()))
            });
            let (state, tab_title) = found.unwrap_or_default();
            if !state.is_open() {
                proceed();
                return;
            }
            let end_tx = end_tx.clone();
            tx_prompt.set(Some(TxPrompt {
                tab_id,
                tab: tab_title,
                stmts: state.stmts(),
                can_commit: state.can_commit(),
                resolve: Rc::new(move |choice| {
                    tx_prompt.set(None);
                    match choice {
                        TxChoice::Commit => (end_tx)(tab_id, true, Some(proceed.clone())),
                        TxChoice::Rollback => (end_tx)(tab_id, false, Some(proceed.clone())),
                        TxChoice::Cancel => {}
                    }
                }),
            }));
        })
    };

    // Pin a fresh connection for a Manual tab, replacing any it already had.
    // Opened eagerly (so a bad connection is reported when the user asks for
    // Manual, not at their first statement) but *not* begun — `BEGIN` is lazy.
    // Also used to re-pin when the tab's database changes, since a PostgreSQL
    // session is bound to one database for its whole life.
    let open_session: Rc<dyn Fn(usize)> = {
        let sessions = sessions.clone();
        let handle = handle.clone();
        let db_for = db_for.clone();
        let drop_session = drop_session.clone();
        Rc::new(move |tab_id: usize| {
            let Some(tab) = tabs.with_untracked(|v| v.iter().find(|t| t.id == tab_id).copied())
            else {
                return;
            };
            (drop_session)(tab_id);
            let db = match db_for(tab.conn_id.get_untracked()) {
                Ok(db) => db,
                Err(e) => {
                    tab.tx_mode.set(TxMode::Auto);
                    error_modal_text.set(Some(e));
                    error_modal_open.set(true);
                    return;
                }
            };
            let database = tab.database.get_untracked();
            let sessions = sessions.clone();
            let opened =
                create_ext_action(cx, move |res: Result<Arc<Session>, String>| match res {
                    Ok(s) => {
                        // A flip back to Auto (or a tab close) may have raced us;
                        // don't resurrect a session nobody wants.
                        if tab.tx_mode.get_untracked().is_manual() {
                            sessions.borrow_mut().insert(tab_id, s);
                        }
                    }
                    Err(e) => {
                        tab.tx_mode.set(TxMode::Auto);
                        error_modal_text
                            .set(Some(format!("couldn't open a transaction connection: {e}")));
                        error_modal_open.set(true);
                    }
                });
            handle.spawn(async move {
                opened(
                    Session::open(&db, database.as_deref())
                        .await
                        .map_err(|e| e.to_string()),
                );
            });
        })
    };

    // Flip a tab between Auto-commit and Manual. Auto is only reachable with no
    // transaction open; the footer raises a `TxPrompt` first if there is one.
    let set_tx_mode: Rc<dyn Fn(usize, TxMode)> = {
        let drop_session = drop_session.clone();
        let open_session = open_session.clone();
        let guard_tx = guard_tx.clone();
        Rc::new(move |tab_id: usize, mode: TxMode| {
            let Some(tab) = tabs.with_untracked(|v| v.iter().find(|t| t.id == tab_id).copied())
            else {
                return;
            };
            if tab.tx_mode.get_untracked() == mode {
                return;
            }
            match mode {
                TxMode::Manual => {
                    tab.tx.set(TxState::closed());
                    tab.tx_mode.set(TxMode::Manual);
                    (open_session)(tab_id);
                }
                // Leaving Manual with a transaction open would silently discard
                // it, so ask; `guard_tx` runs this straight through when there's
                // nothing open.
                TxMode::Auto => {
                    let drop_session = drop_session.clone();
                    (guard_tx)(
                        tab_id,
                        Rc::new(move || {
                            tab.tx.set(TxState::closed());
                            tab.tx_mode.set(TxMode::Auto);
                            (drop_session)(tab_id);
                        }),
                    );
                }
            }
        })
    };

    let commit_tx: Rc<dyn Fn(usize)> = {
        let end_tx = end_tx.clone();
        Rc::new(move |id: usize| (end_tx)(id, true, None))
    };
    let rollback_tx: Rc<dyn Fn(usize)> = {
        let end_tx = end_tx.clone();
        Rc::new(move |id: usize| (end_tx)(id, false, None))
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
    let set_active_db: Rc<dyn Fn(String)> = {
        let guard_tx = guard_tx.clone();
        let open_session = open_session.clone();
        Rc::new(move |name: String| {
            // The DB selector lists the active connection's databases, so picking
            // one binds the active tab to the active connection + that database.
            let exists = db_nodes.with_untracked(|ns| ns.iter().any(|n| n.database == name));
            if !exists {
                return;
            }
            let id = active.get_untracked();
            let open_session = open_session.clone();
            // A pinned session belongs to one database — PostgreSQL can't switch
            // and MySQL's transaction context wouldn't survive the move — so an
            // open transaction has to be settled before the tab moves.
            (guard_tx)(
                id,
                Rc::new(move || {
                    let manual = tabs.with_untracked(|v| {
                        if let Some(t) = v.iter().find(|t| t.id == id) {
                            t.conn_id.set(active_conn.get_untracked());
                            t.database.set(Some(name.clone()));
                            t.tx_mode.get_untracked().is_manual()
                        } else {
                            false
                        }
                    });
                    last_db.set(Some(name.clone()));
                    if manual {
                        (open_session)(id);
                    }
                }),
            );
        })
    };

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

    // Open a tab against an explicit connection + database and activate it.
    // Split out from `add_tab` so a connection switch can open one on the
    // connection being switched *to* — `default_tab_target` reads `db_nodes`,
    // which still holds the previous connection's databases until its schema
    // finishes loading.
    let open_tab_on: Rc<dyn Fn(u64, Option<String>)> = {
        let next_id = next_id.clone();
        Rc::new(move |conn_id: u64, database: Option<String>| {
            let id = next_id.get();
            next_id.set(id + 1);
            tabs.update(|v| {
                let mut t = Tab::new(cx, id, "", conn_id, database);
                t.label = smallest_free_label(&used_labels(v, conn_id));
                v.push(t);
            });
            active.set(id);
        })
    };

    let add_tab: Rc<dyn Fn()> = {
        let default_tab_target = default_tab_target.clone();
        let open_tab_on = open_tab_on.clone();
        Rc::new(move || {
            let (conn_id, database) = default_tab_target();
            (open_tab_on)(conn_id, database);
        })
    };

    // `(tab id, connection id)` in display order — the shape `core::tabsel`'s
    // selection rules work on.
    let tab_refs = move || {
        tabs.with_untracked(|v| {
            v.iter()
                .map(|t| (t.id, t.conn_id.get_untracked()))
                .collect::<Vec<_>>()
        })
    };

    // Close a tab. Closing the last one clears it and briefly flashes it away
    // (design keeps ≥1 tab); other tabs activate a neighbor.
    let close_tab_now: Rc<dyn Fn(usize)> = {
        let tokens = tokens.clone();
        let recently_closed = recently_closed.clone();
        let drop_session = drop_session.clone();
        Rc::new(move |id: usize| {
            // A Manual tab's pinned connection goes with it. By the time we get
            // here any open transaction has been settled by `close_tab`'s prompt,
            // so this is just releasing the connection.
            (drop_session)(id);
            // Snapshot a closing tab into the reopen ring (most-recent first,
            // capped at 10) — but only if it holds something worth restoring.
            let record = |tab: &Tab| {
                let query = tab.query.get_untracked();
                let source = tab.source.get_untracked();
                let name = tab.name.get_untracked();
                if query.trim().is_empty() && source.is_none() && name.is_none() {
                    return;
                }
                let mut ring = recently_closed.borrow_mut();
                if ring.len() >= 10 {
                    ring.pop_back();
                }
                ring.push_front(ClosedTab {
                    query,
                    conn_id: tab.conn_id.get_untracked(),
                    database: tab.database.get_untracked(),
                    source,
                    name,
                    label: tab.label,
                });
            };
            // Pinned tabs aren't closable — this is the single choke point for
            // every close path (× click, middle-click, Ctrl+W), so gating here
            // covers them all. Unpin first to close.
            if tabs
                .with_untracked(|v| {
                    v.iter()
                        .find(|t| t.id == id)
                        .map(|t| t.pinned.get_untracked())
                })
                .unwrap_or(false)
            {
                return;
            }
            // H5: cancel this tab's in-flight query so it can't complete onto
            // cleared/freed signals (and stops the server-side work).
            if let Some((_, tok)) = tokens.borrow_mut().remove(&id) {
                tok.cancel();
            }
            // "Keep ≥1 tab" is per *connection* now: the strip shows one
            // connection's tabs, so closing the last of those must clear-and-
            // flash rather than remove — however many tabs other connections
            // hold. Removing it would leave `active` pointing at a tab that no
            // longer exists (its scoped neighbour is `None`), and the deferred
            // scope disposal then frees signals the mounted view still reads.
            let is_last = schemaic_core::tabsel::closing_would_empty(&tab_refs(), id);
            if is_last {
                let Some(tab) = tabs.with_untracked(|v| v.iter().find(|t| t.id == id).copied())
                else {
                    return;
                };
                record(&tab);
                tab.query.set(String::new());
                tab.source.set(None);
                // Also reset the results pane so the reopened tab is fully fresh
                // (single-grid Idle state, no leftover Run-Everything tabs).
                tab.results.set(QueryState::Idle);
                tab.result_tabs.set(Vec::new());
                tab.active_result.set(0);
                // Drop any temporary font zoom so the respawned tab starts at the
                // user's configured size (the post-flash rebuild reads this).
                tab.font_zoom.set(None);
                flashing.set(Some(id));
                exec_after(Duration::from_millis(150), move |_| flashing.set(None));
                return;
            }
            let was_active = active.get_untracked() == id;
            // Scoped to the closing tab's own connection: the neighbour in the
            // flat list can belong to another one, which would silently switch
            // what the user is looking at.
            let neighbor = schemaic_core::tabsel::neighbor(&tab_refs(), id);
            // Grab this tab before dropping it from the list: snapshot it for the
            // reopen ring and keep its scope so we can free its signals (C14).
            let closed = tabs.with_untracked(|v| v.iter().find(|t| t.id == id).copied());
            if let Some(tab) = &closed {
                record(tab);
            }
            let closed_cx = closed.map(|t| t.cx);
            tabs.update(|v| v.retain(|t| t.id != id));
            if was_active && let Some(n) = neighbor {
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

    // Closing a tab with an open transaction asks first — the pinned connection
    // dies with the tab, so an unanswered transaction would just vanish.
    let close_tab: Rc<dyn Fn(usize)> = {
        let close_tab_now = close_tab_now.clone();
        let guard_tx = guard_tx.clone();
        Rc::new(move |id: usize| {
            let close_tab_now = close_tab_now.clone();
            (guard_tx)(id, Rc::new(move || (close_tab_now)(id)));
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
                !t.pinned.get_untracked()
                    && t.query.get_untracked().trim().is_empty()
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
            None => {
                let mut nt = new_tab;
                let used = used_labels(v, nt.conn_id.get_untracked());
                nt.label = smallest_free_label(&used);
                v.push(nt);
            }
        });
        active.set(new_tab.id);
        // Deferred for the same reason as `close_tab`: let the center view rebuild
        // for the new tab id before the old tab's scope is dropped.
        if let Some(scope) = replaced_cx {
            exec_after(Duration::ZERO, move |_| scope.dispose());
        }
    });

    // Toggle a tab's pinned state, then re-order the strip so pinned tabs stay
    // contiguous at the left in pin order. The tab is pulled out and reinserted at
    // the pinned/unpinned boundary (the count of leading pinned tabs) — which is
    // correct both ways: a newly pinned tab lands just after the existing pinned
    // ones; a newly unpinned tab lands at the first unpinned slot.
    let toggle_pin: Rc<dyn Fn(usize)> = Rc::new(move |id: usize| {
        let Some(t) = tabs.with_untracked(|v| v.iter().find(|x| x.id == id).copied()) else {
            return;
        };
        t.pinned.set(!t.pinned.get_untracked());
        tabs.update(|v| {
            if let Some(pos) = v.iter().position(|x| x.id == id) {
                let tab = v.remove(pos);
                let boundary = v.iter().take_while(|x| x.pinned.get_untracked()).count();
                v.insert(boundary, tab);
            }
        });
    });

    // Duplicate a tab: a fresh (unpinned) tab with the same connection/database and
    // query, opened right after the source and made active. If the source is
    // pinned, the duplicate can't sit inside the pinned block — it clamps to the
    // first unpinned slot so the pinned-contiguous invariant holds.
    let duplicate_tab: Rc<dyn Fn(usize)> = {
        let next_id = next_id.clone();
        Rc::new(move |id: usize| {
            let Some(src) = tabs.with_untracked(|v| v.iter().find(|t| t.id == id).copied()) else {
                return;
            };
            let new_id = next_id.get();
            next_id.set(new_id + 1);
            let nt = Tab::new(
                cx,
                new_id,
                &src.query.get_untracked(),
                src.conn_id.get_untracked(),
                src.database.get_untracked(),
            );
            tabs.update(|v| {
                let mut nt = nt;
                let used = used_labels(v, nt.conn_id.get_untracked());
                nt.label = smallest_free_label(&used);
                let boundary = v.iter().take_while(|t| t.pinned.get_untracked()).count();
                let at = v
                    .iter()
                    .position(|t| t.id == id)
                    .map(|i| i + 1)
                    .unwrap_or(v.len())
                    .max(boundary);
                v.insert(at, nt);
            });
            active.set(new_id);
        })
    };

    // Build + place a fresh tab showing a table: `SELECT * … ORDER BY <pk> LIMIT
    // 100` bound to the active connection + that db, remembering its source for
    // tree highlighting. Reuses a blank active tab (via `place_tab`), but never
    // dedupes to an already-open table tab — that's the caller's job.
    let spawn_table_tab: Rc<dyn Fn(TableSource, Option<String>)> = {
        let run = run.clone();
        let next_id = next_id.clone();
        let place_tab = place_tab.clone();
        Rc::new(move |source: TableSource, highlight: Option<String>| {
            let id = next_id.get();
            next_id.set(id + 1);
            // Order by the primary key so the capped page is a defined set
            // (see `table_query`). The key comes from the loaded schema —
            // which is how the user got here, via the tree — and is empty
            // only if introspection hasn't finished, in which case the
            // statement is unordered exactly as before.
            // From the saved connection's `db_type`, not `db_for` — that
            // needs an established SSH tunnel, and falling back to the
            // default dialect would quote a Postgres table MySQL-style.
            let conn_id = active_conn.get_untracked();
            let dialect = connections
                .with_untracked(|cs| {
                    cs.iter()
                        .find(|c| c.id == conn_id)
                        .map(|c| SqlDialect::from_db_type(&c.db_type))
                })
                .unwrap_or_default();
            let (_, pk_cols) = table_ddl_and_pk(db_nodes, &source, dialect);
            let sql = table_query(
                dialect,
                &source.database,
                source.schema.as_deref(),
                &source.table,
                &pk_cols,
                Order::Asc,
                TABLE_TAB_ROWS,
            );
            let tab = Tab::new(
                cx,
                id,
                &sql,
                active_conn.get_untracked(),
                Some(source.database.clone()),
            );
            tab.source.set(Some(source));
            // A column to select once the results load (schema-tree column
            // double-click). Consumed + cleared by the grid.
            tab.highlight_col.set(highlight);
            (place_tab)(tab);
            run(sql);
        })
    };

    // Open a table from the sidebar / Find ("Open"): if a tab is already showing
    // it (same connection + source), just switch to that tab; otherwise open a
    // fresh one. Matching on `conn_id` too (not source alone) so the same-named
    // table under a different connection doesn't wrongly steal focus (H13).
    let open_table: Rc<dyn Fn(TableSource)> = {
        let spawn = spawn_table_tab.clone();
        let run = run.clone();
        Rc::new(move |source: TableSource| {
            let existing = tabs.with_untracked(|v| {
                v.iter()
                    .find(|t| {
                        t.source.get_untracked().as_ref() == Some(&source)
                            && t.conn_id.get_untracked() == active_conn.get_untracked()
                    })
                    .copied()
            });
            if let Some(tab) = existing {
                active.set(tab.id);
                // A restored tab keeps its query text but hasn't run (restore never
                // auto-runs), so switching to it would show an empty grid. Run it now
                // — a freshly-opened table tab always runs on open, so reusing one
                // should match. (`run` targets the now-active tab.)
                if matches!(tab.results.get_untracked(), QueryState::Idle) {
                    (run)(tab.query.get_untracked());
                }
                return;
            }
            (spawn)(source, None);
        })
    };

    // Open a table and highlight one of its columns in the grid (schema-tree column
    // double-click). Same tab-reuse rules as `open_table`, but records the column to
    // select once the grid loads. For an already-open tab, set the highlight *then*
    // switch to it — switching rebuilds that tab's grid, whose effect consumes it.
    let open_table_col: Rc<dyn Fn(TableSource, String)> = {
        let spawn = spawn_table_tab.clone();
        let run = run.clone();
        Rc::new(move |source: TableSource, column: String| {
            let existing = tabs.with_untracked(|v| {
                v.iter()
                    .find(|t| {
                        t.source.get_untracked().as_ref() == Some(&source)
                            && t.conn_id.get_untracked() == active_conn.get_untracked()
                    })
                    .copied()
            });
            if let Some(tab) = existing {
                tab.highlight_col.set(Some(column));
                // Only switch tabs when we're not already on it: `active.set` never
                // dedups, so re-setting the current id would rebuild (and dispose)
                // the live grid out from under the highlight effect. When the tab is
                // already active, setting `highlight_col` alone re-fires its mounted
                // grid's effect, which re-selects on the live grid — no rebuild.
                if active.get_untracked() != tab.id {
                    active.set(tab.id);
                }
                // A restored tab has its query but hasn't run — run it now so the
                // grid loads and the highlight effect can fire (it consumes
                // `highlight_col` once the results are `Loaded`).
                if matches!(tab.results.get_untracked(), QueryState::Idle) {
                    (run)(tab.query.get_untracked());
                }
                return;
            }
            (spawn)(source, Some(column));
        })
    };

    // Always open the table in a brand-new tab, even if it's already open
    // ("Open in new tab" — only offered by the menu when a tab for it exists).
    let open_table_new: Rc<dyn Fn(TableSource)> = {
        let spawn = spawn_table_tab.clone();
        Rc::new(move |source: TableSource| (spawn)(source, None))
    };

    // Follow a foreign key from the grid: open the referenced table in a fresh tab
    // running the supplied filter `SELECT`, and auto-run it. Sourced from
    // `(database, table)` so the new grid is editable and shows key icons — like a
    // normal table tab, only with a WHERE. The referenced table lives on the same
    // connection (FKs can't cross servers), possibly in another database.
    let open_table_filtered: Rc<dyn Fn(TableSource, String)> = {
        let next_id = next_id.clone();
        let place_tab = place_tab.clone();
        let run = run.clone();
        Rc::new(move |source: TableSource, sql: String| {
            let id = next_id.get();
            next_id.set(id + 1);
            let tab = Tab::new(
                cx,
                id,
                &sql,
                active_conn.get_untracked(),
                Some(source.database.clone()),
            );
            tab.source.set(Some(source));
            (place_tab)(tab);
            run(sql);
        })
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

    // Reopen a query-history entry in a new tab. Unlike `open_query` (which
    // targets the *active* connection/db), this restores the entry's own
    // `conn_id`/`database` and its originating tab name — all recorded on the
    // entry. The history panel is per-connection, so `conn_id` is the active
    // (valid) connection. Does NOT run the query.
    let open_history: Rc<dyn Fn(schemaic_core::history::HistoryEntry)> = {
        let next_id = next_id.clone();
        let place_tab = place_tab.clone();
        Rc::new(move |entry: schemaic_core::history::HistoryEntry| {
            let id = next_id.get();
            next_id.set(id + 1);
            let tab = Tab::new(cx, id, &entry.sql, entry.conn_id, entry.database);
            tab.name.set(entry.tab_name);
            (place_tab)(tab);
        })
    };

    // Reopen the most-recently-closed tab (Ctrl+Shift+T): pop the ring and rebuild
    // the tab from the snapshot — its own connection/database, query, source (so it
    // stays editable if it was a table view), and name. No-op when the ring's empty.
    let reopen_closed_tab: Rc<dyn Fn()> = {
        let next_id = next_id.clone();
        let place_tab = place_tab.clone();
        let recently_closed = recently_closed.clone();
        Rc::new(move || {
            // Reopen the most recent close *on this connection*. The strip is
            // scoped, so reopening another connection's tab would restore it out
            // of sight; its own connection reopens it when the user goes back.
            let Some(snap) = ({
                let mut ring = recently_closed.borrow_mut();
                let conn = active_conn.get_untracked();
                ring.iter()
                    .position(|s| s.conn_id == conn)
                    .and_then(|at| ring.remove(at))
            }) else {
                return;
            };
            let id = next_id.get();
            next_id.set(id + 1);
            // Named tabs already restore their name; for unnamed ones, restore the
            // original "Query N" number too (unless a live tab now claims it).
            let orig_label = snap.label;
            let restore_label = snap.name.is_none();
            let tab = Tab::new(cx, id, &snap.query, snap.conn_id, snap.database);
            tab.source.set(snap.source);
            tab.name.set(snap.name);
            (place_tab)(tab);
            if restore_label {
                // A clash only matters within the connection — that's the scope
                // the number is unique in, and the only place both would show.
                let clash = tabs.with_untracked(|v| {
                    v.iter().any(|t| {
                        t.id != id
                            && t.label == orig_label
                            && t.conn_id.get_untracked() == snap.conn_id
                    })
                });
                if !clash {
                    tabs.update(|v| {
                        if let Some(t) = v.iter_mut().find(|t| t.id == id) {
                            t.label = orig_label;
                        }
                    });
                }
            }
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
            live_validate: live_validate.get_untracked(),
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
            live_validate.get();
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
                    t.name.get();
                    t.pinned.get();
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
                            .map(|t| {
                                let src = t.source.get_untracked();
                                schemaic_core::persist::SavedTab {
                                    query: t.query.get_untracked(),
                                    conn_id: t.conn_id.get_untracked(),
                                    database: t.database.get_untracked(),
                                    // The namespace rides alongside the pair rather
                                    // than widening it, so an older build's session
                                    // file still restores (see `SavedTab`).
                                    source: src
                                        .as_ref()
                                        .map(|s| (s.database.clone(), s.table.clone())),
                                    source_schema: src.and_then(|s| s.schema),
                                    name: t.name.get_untracked(),
                                    pinned: t.pinned.get_untracked(),
                                }
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
            let send = create_ext_action(cx, move |res: ConnectResult| match res {
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
                        let send_schema = create_ext_action(cx, move |st: SchemaState| sig.set(st));
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
            });
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
        secrets::save_connections(&file);
    };

    // Flip a connection's read-only flag and persist (the status-bar shortcut).
    // The `read_only` memo reads this reactively, so write-gating updates at once.
    let toggle_read_only: Rc<dyn Fn(u64)> = Rc::new(move |id: u64| {
        connections.update(|cs| {
            if let Some(c) = cs.iter_mut().find(|c| c.id == id) {
                c.read_only = !c.read_only;
            }
        });
        persist_conns(Some(active_conn.get_untracked()));
    });

    // Switch the active connection and reload its schema.
    let switch_conn: Rc<dyn Fn(u64)> = {
        let load_schema = load_schema.clone();
        let ai_session = ai_session.clone();
        let check_conn = check_conn.clone();
        let last_tab = last_tab.clone();
        let open_tab_on = open_tab_on.clone();
        Rc::new(move |id: u64| {
            // Remember where the user was here before leaving, so coming back
            // returns to that tab rather than the connection's first.
            last_tab
                .borrow_mut()
                .insert(active_conn.get_untracked(), active.get_untracked());
            active_conn.set(id);
            persist_conns(Some(id));
            // The strip shows only this connection's tabs, so the active tab has
            // to become one of them. A connection with none gets a fresh tab —
            // with no database, since `db_nodes` still holds the previous
            // connection's until `load_schema` below finishes.
            let remembered = last_tab.borrow().get(&id).copied();
            match schemaic_core::tabsel::pick_active(&tab_refs(), id, remembered) {
                Some(tab) => active.set(tab),
                None => (open_tab_on)(id, None),
            }
            // Clear stale status until this connection's own check lands.
            conn_status.set(ConnStatus::Unknown);
            // The AI conversation is bound to a connection — swap in the one
            // saved for this connection (empty when there isn't one). The live
            // session can't be reused, so the restored turns are transcript;
            // the next message spawns a session that gets them replayed.
            *ai_session.borrow_mut() = None;
            let restored = schemaic_core::chat::for_conn(&saved_chats.get_untracked(), id);
            // Reappearing, not arriving — mount them without the entrance pop.
            schemaic_ui::mark_messages_seen(restored.len());
            ai_messages.set(restored);
            ai_busy.set(false);
            // Any in-flight Stop belonged to the conversation just replaced.
            ai_stopping.set(false);
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
        let drop_session = drop_session.clone();
        Rc::new(move |id: u64| {
            let was_active = active_conn.get_untracked() == id;
            // Release any pinned transaction connection on the connection being
            // deleted — its tunnel is about to go, and a Manual tab pointed at a
            // connection that no longer exists can't do anything with it. No
            // prompt: the connection is already gone as far as the user is
            // concerned, and the server rolls back on disconnect. The tabs drop
            // back to Auto-commit so their footer stops claiming a transaction.
            let orphaned: Vec<usize> = tabs.with_untracked(|v| {
                v.iter()
                    .filter(|t| t.conn_id.get_untracked() == id)
                    .map(|t| {
                        t.tx_mode.set(TxMode::Auto);
                        t.tx.set(TxState::closed());
                        t.id
                    })
                    .collect()
            });
            for tab_id in orphaned {
                (drop_session)(tab_id);
            }
            // Drop any tunnel for the deleted connection (frees its listener/port).
            tunnels.borrow_mut().remove(&id);
            // Forget its keyring secrets so nothing is left behind.
            secrets::forget_connection(id);
            // …and its saved AI conversation, which would otherwise linger and
            // resurface under whatever connection reuses the id.
            saved_chats.update(|chats| schemaic_core::chat::clear_conn(chats, id));
            persist::save_json(
                "chats.json",
                &schemaic_core::chat::ChatFile {
                    chats: saved_chats.get_untracked(),
                },
            );
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
            // The deleted connection's tabs would now be invisible in every
            // connection's strip. Adopt them into the active one rather than
            // closing them — an unsaved query shouldn't vanish with a connection
            // — clearing the database, which named one that just went away.
            // (Restore does the same for a tab whose connection is gone.)
            let adopting = active_conn.get_untracked();
            if adopting != id {
                tabs.with_untracked(|v| {
                    for t in v.iter().filter(|t| t.conn_id.get_untracked() == id) {
                        t.conn_id.set(adopting);
                        t.database.set(None);
                    }
                });
            }
            last_tab.borrow_mut().remove(&id);
            // The active tab may have been one of those (or on the deleted
            // connection); make sure it's one the strip will show.
            if let Some(tab) = schemaic_core::tabsel::pick_active(
                &tab_refs(),
                adopting,
                Some(active.get_untracked()),
            ) {
                active.set(tab);
            }
        })
    };

    // ── AI panel (Claude Code) ──────────────────────────────────────────────
    // (see `mark_stopped` below for how a stopped turn is settled)
    // Apply streamed transcript snapshots to the pending assistant bubble.
    {
        let ai_session = ai_session.clone();
        let persist_chat = persist_chat.clone();
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
                    // A turn we stopped ends as an error by the CLI's reckoning;
                    // present it as a stop instead, keeping whatever partial
                    // answer had streamed in.
                    if ai_stopping.get_untracked() {
                        ai_stopping.set(false);
                        mark_stopped(ai_messages);
                    }
                    ai_busy.set(false);
                    // Save under the *session's* connection, not the active one:
                    // a switch mid-turn drops the session and clears the panel,
                    // and a late snapshot must not overwrite the conversation
                    // just restored for the connection switched to.
                    if let Some(id) = ai_session.borrow().as_ref().map(|s| s.conn_id) {
                        (persist_chat)(id);
                    }
                }
            }
        });
    }

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
            // The live context as it stands *now* — the system prompt is written
            // once at spawn, so every later turn carries the delta (see
            // `apply_turn_delta`).
            let cx_params = AiContextParams {
                connections,
                active_conn,
                db_nodes,
                tabs,
                active,
                scope: ai_schema_scope.get_untracked(),
                run_queries: ai_run_queries.get_untracked(),
            };
            // The active tab's database counts only when that tab is on the
            // active connection (a tab keeps its own); otherwise the new-tab
            // default for this connection stands in. Resolved once, so the
            // system prompt, the turn deltas, and the MCP endpoint all name the
            // same database.
            let fallback_db = default_tab_target().1;
            let context_now = turn_context(cx_params, fallback_db.as_deref());
            // The conversation as it stands *before* this question is appended.
            let prior = ai_messages.get_untracked();
            if need_new {
                // Whatever is on screen predates this session — a restored
                // conversation, or turns from one that was cancelled/respawned.
                // Replay it into the prompt so a follow-up still resolves.
                let context = ai_context(
                    cx_params,
                    fallback_db.as_deref(),
                    &prior,
                    &ai_instructions.get_untracked(),
                );
                // If the connection's `Db` can't be built yet (SSH tunnel
                // pending), skip the MCP tools rather than blocking the chat.
                let database = context_now.active_db.clone();
                if let Ok(db) = db_for(active_id) {
                    let mcp_database = database.clone();
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
                        // The system prompt just stated this context, so the
                        // first turn has no delta to report.
                        last_context: context_now.clone(),
                        mcp_database,
                    });
                }
            }

            ai_messages.update(|v| {
                v.push(ChatMessage::user(msg.clone()));
                v.push(ChatMessage::pending());
            });
            ai_input.set(String::new());
            ai_busy.set(true);

            // Prepend whatever moved since the assistant last looked (edited SQL,
            // a switched database, a schema that finished introspecting), then
            // advance the session's snapshot so the next turn diffs against this
            // one.
            // A recap of recent questions rides along, because the CLI's own
            // cross-turn memory isn't dependable (measured: ~2 in 3, unaffected
            // by --session-id or --resume). Skipped when the session was just
            // spawned above — its system prompt already replayed the thread.
            let recap = if need_new {
                String::new()
            } else {
                render_recap(&prior, RECAP_QUESTIONS)
            };
            if let Some(s) = ai_session.borrow_mut().as_mut() {
                let turn = apply_turn_delta(
                    &s.last_context,
                    &context_now,
                    s.mcp_database.as_deref(),
                    &recap,
                    &msg,
                );
                s.last_context = context_now;
                let _ = s.stdin_tx.send(schemaic_ai::user_message_line(&turn));
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
            // Ask the CLI to end the *turn*. It answers with a `control_response`
            // and a `result`, then stays available for the next message — so
            // stopping one runaway answer no longer costs a process respawn.
            // `ai_stopping` tells the stream effect that the `result` about to
            // arrive (flagged `is_error`) is this stop, not a failure.
            let sent = ai_session
                .borrow()
                .as_ref()
                .map(|s| s.stdin_tx.send(schemaic_ai::interrupt_line("stop")).is_ok())
                .unwrap_or(false);
            if !sent {
                // No live session (or its channel is gone) — fall back to the
                // old behaviour so Stop always stops.
                ai_session.borrow_mut().take();
                mark_stopped(ai_messages);
                ai_busy.set(false);
                return;
            }
            ai_stopping.set(true);
            // Safety net: if the interrupt is ignored, don't leave the panel
            // spinning — drop the session (killing the child) and settle the UI.
            floem::action::exec_after(std::time::Duration::from_secs(5), {
                let ai_session = ai_session.clone();
                move |_| {
                    if ai_stopping.try_get_untracked() == Some(true) {
                        ai_session.borrow_mut().take();
                        mark_stopped(ai_messages);
                        ai_stopping.set(false);
                        ai_busy.set(false);
                    }
                }
            });
        })
    };

    // New chat: drop the session (fresh context next message) and clear bubbles.
    // Also forgets the saved conversation — "New Chat" should not leave the old
    // one waiting to reappear on the next connection switch.
    let ai_new_chat: Rc<dyn Fn()> = {
        let ai_session = ai_session.clone();
        let persist_chat = persist_chat.clone();
        Rc::new(move || {
            ai_session.borrow_mut().take();
            ai_messages.set(Vec::new());
            ai_busy.set(false);
            ai_stopping.set(false);
            (persist_chat)(active_conn.get_untracked());
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
            // a table is named in the buffer/intent. Scoped to the active
            // connection — the outline comes from that connection's `db_nodes`,
            // so a database from another connection would match nothing.
            let active_db = active_tab_database(
                AiContextParams {
                    connections,
                    active_conn,
                    db_nodes,
                    tabs,
                    active,
                    scope: ai_schema_scope.get_untracked(),
                    run_queries: ai_run_queries.get_untracked(),
                },
                default_tab_target().1.as_deref(),
            );
            let system = inline_system_prompt(db_nodes, active_db.as_deref(), &req);
            let intent = req.intent.clone();
            let bin = claude_bin(&ai_cli_path.get_untracked());
            // Follow the AI panel's model choice (one place to change it).
            let model = ai_model.get_untracked().cli().to_string();
            let send = create_ext_action(cx, move |state: InlineAiState| inline_ai.set(state));
            let jh = handle.spawn(async move {
                let out = Command::new(bin)
                    .args(schemaic_ai::inline_args(&intent, &system, &model))
                    .kill_on_drop(true)
                    .output()
                    .await;
                let state = match out {
                    Ok(o) => inline_outcome(o.status.success(), &o.stdout, &o.stderr),
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

    // AI-fill a single grid cell: bottom-sample the base table, build a prompt from
    // its DDL + sample + the row's other cells, run a one-shot `claude -p` call, and
    // report the parsed value back for the grid to stage (never auto-committed).
    let ai_fill: schemaic_ui::AiFillFn = {
        let handle = handle.clone();
        let db_for = db_for.clone();
        Rc::new(
            move |req: schemaic_ui::AiFillRequest, done: schemaic_ui::AiFillDoneFn| {
                use schemaic_ui::AiFillResult;
                let db = match db_for(req.conn_id) {
                    Ok(db) => db,
                    Err(e) => {
                        (done)(AiFillResult::Failed(e));
                        return;
                    }
                };
                // DDL skeleton + PK columns from the loaded schema (empty if
                // introspection hasn't run — the sample still carries conventions).
                let (ddl, pk_cols) = table_ddl_and_pk(db_nodes, &req.source, dialect_of(&db));
                let bin = claude_bin(&ai_cli_path.get_untracked());
                let model = ai_model.get_untracked().cli().to_string();
                let finish = create_ext_action(cx, move |res: AiFillResult| (done)(res));
                let schemaic_ui::AiFillRequest {
                    source,
                    column,
                    row_context,
                    ..
                } = req;
                handle.spawn(async move {
                    let token = CancellationToken::new();
                    // Bottom-sample the base table for enum/format/FK inference.
                    let sql = sample_sql(db.engine(), &source, &pk_cols);
                    let database = source.database.clone();
                    let sample = match db.fetch_query(Some(&database), &sql, 20, token).await {
                        Ok(rs) => sample_rows(&rs),
                        Err(_) => Vec::new(), // empty/unsampleable → DDL-only prompt
                    };
                    let prompt = schemaic_core::seed::build_fill_prompt(
                        &format!("{database}.{}", source.display()),
                        &column,
                        &ddl,
                        &sample,
                        &row_context,
                    );
                    let system = "You output only the requested raw value — no quotes, \
                                  no markdown, no prose.";
                    let out = Command::new(bin)
                        .args(schemaic_ai::inline_args(&prompt, system, &model))
                        // Close stdin so `claude -p` doesn't stall ~3s waiting for
                        // piped input ("no stdin data received") before responding.
                        .stdin(std::process::Stdio::null())
                        .kill_on_drop(true)
                        .output()
                        .await;
                    let res = match out {
                        Ok(o) if o.status.success() => {
                            let stdout = String::from_utf8_lossy(&o.stdout);
                            match schemaic_core::seed::parse_fill_response(&stdout) {
                                schemaic_core::seed::FillOutcome::Value(v) => {
                                    AiFillResult::Value(v)
                                }
                                schemaic_core::seed::FillOutcome::Null => AiFillResult::Null,
                                schemaic_core::seed::FillOutcome::Empty => {
                                    AiFillResult::Failed("The AI returned no value.".into())
                                }
                            }
                        }
                        Ok(o) => AiFillResult::Failed(schemaic_ai::cli_failure_message(
                            o.status.code(),
                            &String::from_utf8_lossy(&o.stdout),
                            &String::from_utf8_lossy(&o.stderr),
                        )),
                        Err(e) => AiFillResult::Failed(e.to_string()),
                    };
                    finish(res);
                });
            },
        )
    };

    // AI-generate seed rows (Insert Row = 1, Seed Table = N): bottom-sample the base
    // table, prompt for a JSON array of rows over the given columns, parse, and hand
    // the rows back for the grid to stage as pending rows (never auto-committed).
    let ai_seed: schemaic_ui::AiSeedFn = {
        let handle = handle.clone();
        let db_for = db_for.clone();
        Rc::new(
            move |req: schemaic_ui::AiSeedRequest, done: schemaic_ui::AiSeedDoneFn| {
                use schemaic_ui::AiSeedResult;
                let db = match db_for(req.conn_id) {
                    Ok(db) => db,
                    Err(e) => {
                        (done)(AiSeedResult::Failed(e));
                        return;
                    }
                };
                let (ddl, pk_cols) = table_ddl_and_pk(db_nodes, &req.source, dialect_of(&db));
                let bin = claude_bin(&ai_cli_path.get_untracked());
                let model = ai_model.get_untracked().cli().to_string();
                let finish = create_ext_action(cx, move |res: AiSeedResult| (done)(res));
                let schemaic_ui::AiSeedRequest {
                    source,
                    fill_columns,
                    count,
                    ..
                } = req;
                handle.spawn(async move {
                    let token = CancellationToken::new();
                    let sql = sample_sql(db.engine(), &source, &pk_cols);
                    let database = source.database.clone();
                    let sample = match db.fetch_query(Some(&database), &sql, 20, token).await {
                        Ok(rs) => sample_rows(&rs),
                        Err(_) => Vec::new(), // empty/unsampleable → DDL-only prompt
                    };
                    let prompt = schemaic_core::seed::build_seed_prompt(
                        &format!("{database}.{}", source.display()),
                        &ddl,
                        &fill_columns,
                        &sample,
                        count,
                    );
                    let system = "You output only a JSON array of row objects — no \
                                  markdown, no prose.";
                    let out = Command::new(bin)
                        .args(schemaic_ai::inline_args(&prompt, system, &model))
                        // Close stdin so `claude -p` doesn't stall ~3s waiting for
                        // piped input ("no stdin data received") before responding.
                        .stdin(std::process::Stdio::null())
                        .kill_on_drop(true)
                        .output()
                        .await;
                    let res = match out {
                        Ok(o) if o.status.success() => {
                            let stdout = String::from_utf8_lossy(&o.stdout);
                            match schemaic_core::seed::parse_seed_response(&stdout) {
                                Ok(rows) => AiSeedResult::Rows(rows),
                                Err(e) => AiSeedResult::Failed(e.to_string()),
                            }
                        }
                        Ok(o) => AiSeedResult::Failed(schemaic_ai::cli_failure_message(
                            o.status.code(),
                            &String::from_utf8_lossy(&o.stdout),
                            &String::from_utf8_lossy(&o.stderr),
                        )),
                        Err(e) => AiSeedResult::Failed(e.to_string()),
                    };
                    finish(res);
                });
            },
        )
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

    // App-process resource usage for the status bar, sampled on a 1s timer.
    let resources = RwSignal::new(schemaic_core::resource::ResourceSample::default());
    start_resource_monitor(resources);

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
            apply_view,
            run_all,
            cancel,
            commit_edits,
            set_tx_mode,
            commit_tx,
            rollback_tx,
            add_tab,
            close_tab,
            toggle_pin,
            duplicate_tab,
            open_table,
            open_table_new,
            open_table_col,
            open_query,
            reopen_closed_tab,
            open_table_filtered,
            set_active_db,
            open_db_cli,
            run_plan,
            validate_stmt,
            open_monitor,
            ai_fill,
            ai_seed,
        }),
        overlay: OverlayUi {
            context_menu,
            popup_menu: RwSignal::new(None),
            popup_anchor: RwSignal::new(None),
            popup_width: RwSignal::new(170.0),
            last_mouse,
            find_open,
            find_query,
            search_history,
            error_modal_open,
            error_modal_text,
            tx_prompt,
            plan_open,
            plan_state,
            plan_sql,
            plan_analyze,
            monitor_open,
            monitor_title,
            monitor_cols,
            monitor_log,
            monitor_error,
            monitor_interval,
            erd: RwSignal::new(None),
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
            toggle_read_only,
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
            open: open_history,
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
            editor_collapsed,
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
            live_validate,
        },
        persist_layout: save_ui.clone(),
        formats,
        save_formats,
        db_colors,
        save_db_colors,
        db_favorites,
        save_db_favorites,
        resources,
    };
    schemaic_ui::workspace(ui)
}

/// Live Monitor tuning: poll interval, per-poll row cap (the monitor is bounded by
/// construction — it never polls an unbounded table), and the max change-log
/// length kept in memory (older entries drop off the top).
const MONITOR_INTERVAL_SECS: u64 = 2;
const MONITOR_LIMIT: usize = 1000;
const MONITOR_LOG_MAX: usize = 1000;

/// Everything a Live Monitor poll tick needs, so it can re-arm itself across ticks
/// (all fields cheap to clone: `Copy` signals, `Rc`s, a `Handle`). See the
/// `open_monitor` action that builds it.
#[derive(Clone)]
struct MonitorCtx {
    handle: tokio::runtime::Handle,
    db_for: Rc<dyn Fn(u64) -> Result<Db, String>>,
    db_nodes: RwSignal<Vec<ConnNode>>,
    cx: Scope,
    open: RwSignal<bool>,
    cols: RwSignal<Vec<String>>,
    log: RwSignal<Vec<MonitorEntry>>,
    error: RwSignal<Option<String>>,
    prev: Rc<RefCell<Option<Snapshot>>>,
    key_cols: Rc<RefCell<Vec<usize>>>,
    generation: Rc<Cell<u64>>,
    started: Instant,
    /// The connection + the table being watched (namespace included, so a
    /// PostgreSQL table outside `public` is actually the one polled).
    target: (u64, TableSource),
    /// Poll interval (seconds), read fresh on each re-arm so the popup's dropdown
    /// takes effect on the next tick.
    interval: RwSignal<u64>,
}

/// One Live Monitor poll: fetch the watched table (bounded), then hand the result
/// to [`monitor_apply`] on the UI thread. Stops silently if the modal was closed
/// (`open` false) or a newer session superseded this one (`generation` bumped).
fn monitor_tick(ctx: MonitorCtx, my_gen: u64) {
    if ctx.open.try_get_untracked() != Some(true) || ctx.generation.get() != my_gen {
        return;
    }
    let (conn_id, source) = ctx.target.clone();
    let db = match (ctx.db_for)(conn_id) {
        Ok(db) => db,
        Err(e) => {
            ctx.error.set(Some(e));
            monitor_reschedule(ctx, my_gen);
            return;
        }
    };
    let ctx2 = ctx.clone();
    let send = create_ext_action(ctx.cx, move |out: Result<ResultSet, String>| {
        monitor_apply(ctx2.clone(), my_gen, out);
    });
    ctx.handle.spawn(async move {
        let out = db
            .fetch_table(
                &source.database,
                source.schema.as_deref(),
                &source.table,
                MONITOR_LIMIT,
                CancellationToken::new(),
            )
            .await
            .map_err(|e| e.to_string());
        send(out);
    });
}

/// UI-thread half of a poll: on the first result, record the columns + resolve the
/// row-identity key (via `analyze_edit`); thereafter diff each snapshot against the
/// previous one and append any changes to the log. Then re-arm the next tick.
fn monitor_apply(ctx: MonitorCtx, my_gen: u64, out: Result<ResultSet, String>) {
    if ctx.open.try_get_untracked() != Some(true) || ctx.generation.get() != my_gen {
        return;
    }
    match out {
        Err(e) => ctx.error.set(Some(e)),
        Ok(rs) => {
            if ctx.prev.borrow().is_none() {
                // Baseline poll: capture columns + resolve the identity key once.
                ctx.cols
                    .set(rs.columns.iter().map(|c| c.name.clone()).collect());
                let db_nodes = ctx.db_nodes;
                let model = analyze_edit(&rs, |db, ns, table| {
                    db_nodes.with_untracked(|nodes| {
                        nodes.iter().find(|n| n.database == db).and_then(|n| {
                            match n.schema.get_untracked() {
                                SchemaState::Loaded(s) => s.find_table(ns, table).cloned(),
                                _ => None,
                            }
                        })
                    })
                });
                match model.insert_target() {
                    Some(t) if !t.key_cols.is_empty() => {
                        *ctx.key_cols.borrow_mut() = t.key_cols.clone();
                    }
                    _ => {
                        ctx.error.set(Some(
                            "No row key for this table — changes can't be tracked.".to_string(),
                        ));
                        return; // nothing meaningful to poll; leave the modal open on the message
                    }
                }
            }
            ctx.error.set(None);
            let key_cols = ctx.key_cols.borrow().clone();
            let snap = Snapshot::from_result(&rs, &key_cols);
            if let Some(prev) = ctx.prev.borrow().as_ref() {
                let changes = diff_snapshots(prev, &snap);
                if !changes.is_empty() {
                    let at = fmt_elapsed(ctx.started.elapsed().as_secs());
                    ctx.log.update(|log| {
                        for change in changes {
                            log.push(MonitorEntry {
                                at: at.clone(),
                                change,
                            });
                        }
                        if log.len() > MONITOR_LOG_MAX {
                            let drop = log.len() - MONITOR_LOG_MAX;
                            log.drain(0..drop);
                        }
                    });
                }
            }
            *ctx.prev.borrow_mut() = Some(snap);
        }
    }
    monitor_reschedule(ctx, my_gen);
}

/// Re-arm the next poll in `MONITOR_INTERVAL_SECS`, unless the monitor was closed
/// or superseded (checked again inside `monitor_tick`).
fn monitor_reschedule(ctx: MonitorCtx, my_gen: u64) {
    if ctx.open.try_get_untracked() != Some(true) || ctx.generation.get() != my_gen {
        return;
    }
    // Read the interval fresh each re-arm so the popup's dropdown takes effect on
    // the next tick. Clamp to a sane floor in case of a stray value.
    let secs = ctx.interval.get_untracked().max(1);
    floem::action::exec_after(Duration::from_secs(secs), move |_| {
        monitor_tick(ctx, my_gen);
    });
}

/// Format elapsed seconds since monitoring started as `MM:SS` (or `H:MM:SS`).
fn fmt_elapsed(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

/// Sample the app process's own CPU/RAM on a self-rescheduling ~1s timer and
/// publish it to `sample` for the status bar. Cross-platform via `sysinfo`
/// (Windows/macOS/Linux). CPU is normalized across logical cores. The first
/// reading after a refresh is 0 (CPU% is a delta between two refreshes), so the
/// CPU figure only becomes meaningful on the second tick — fine for a readout.
fn start_resource_monitor(sample: RwSignal<schemaic_core::resource::ResourceSample>) {
    use schemaic_core::resource::ResourceSample;
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

    let pid = Pid::from_u32(std::process::id());
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let sys = Rc::new(RefCell::new(System::new()));

    // Perpetual tick; reads `sample` with `try_get_untracked` and stops
    // rescheduling once it's disposed at shutdown (else the last timer panics on
    // a freed signal — the same rule as the terminal cursor-blink tick).
    let tick: BlinkTick = Rc::new(RefCell::new(None));
    let tick2 = tick.clone();
    *tick.borrow_mut() = Some(Rc::new(move || {
        if sample.try_get_untracked().is_none() {
            return;
        }
        {
            let mut s = sys.borrow_mut();
            s.refresh_processes_specifics(
                ProcessesToUpdate::Some(&[pid]),
                true,
                ProcessRefreshKind::nothing().with_cpu().with_memory(),
            );
            if let Some(p) = s.process(pid) {
                sample.set(ResourceSample::new(p.memory(), p.cpu_usage(), cores));
            }
        }
        let t = tick2.clone();
        exec_after(Duration::from_secs(1), move |_| {
            if let Some(f) = t.borrow().as_ref() {
                f();
            }
        });
    }));
    let t = tick.clone();
    exec_after(Duration::from_secs(1), move |_| {
        if let Some(f) = t.borrow().as_ref() {
            f();
        }
    });
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

#[cfg(test)]
mod app_tests {
    use super::{MysqlLauncher, inline_outcome, mysql_shell_config, unique_name};
    use schemaic_core::connection::Connection;
    use schemaic_ui::InlineAiState;

    #[test]
    fn returns_base_when_unused() {
        assert_eq!(unique_name("Query", &[]), "Query");
        assert_eq!(unique_name("Query", &["Other".to_string()]), "Query");
    }

    #[test]
    fn appends_first_free_numeric_suffix() {
        let existing = vec!["Query".to_string()];
        assert_eq!(unique_name("Query", &existing), "Query 1");
        let existing = vec!["Query".to_string(), "Query 1".to_string()];
        assert_eq!(unique_name("Query", &existing), "Query 2");
        // Gaps are filled: "Query 1" free even though "Query"/"Query 2" taken.
        let existing = vec!["Query".to_string(), "Query 2".to_string()];
        assert_eq!(unique_name("Query", &existing), "Query 1");
    }

    #[test]
    fn smallest_free_label_reuses_gaps() {
        use super::smallest_free_label;
        assert_eq!(smallest_free_label(&[]), 1);
        assert_eq!(smallest_free_label(&[1, 2]), 3);
        // A freed middle number is reused, not skipped.
        assert_eq!(smallest_free_label(&[1, 3]), 2);
        // Order-independent.
        assert_eq!(smallest_free_label(&[3, 1]), 2);
        assert_eq!(smallest_free_label(&[2, 3]), 1);
    }

    fn conn() -> Connection {
        Connection {
            id: 1,
            name: "c".to_string(),
            db_type: "MySQL".to_string(),
            host: "10.0.0.5".to_string(),
            port: 3307,
            user: "root".to_string(),
            password: "s3cr3t".to_string(),
            ssh: Default::default(),
            color: None,
            prominent_color: false,
            read_only: false,
            environment: Default::default(),
        }
    }

    #[test]
    fn native_shell_puts_password_in_env_not_argv() {
        let cfg = mysql_shell_config(MysqlLauncher::Native("mysql"), &conn(), Some("shop"));
        assert_eq!(cfg.program, "mysql");
        assert_eq!(
            cfg.args,
            vec!["-h", "10.0.0.5", "-P", "3307", "-u", "root", "shop"]
        );
        // Password rides MYSQL_PWD, never the command line.
        assert_eq!(
            cfg.env,
            vec![("MYSQL_PWD".to_string(), "s3cr3t".to_string())]
        );
        assert!(!cfg.args.iter().any(|a| a.contains("s3cr3t")));
    }

    #[test]
    fn native_shell_omits_db_when_none() {
        let cfg = mysql_shell_config(MysqlLauncher::Native("mariadb"), &conn(), None);
        assert_eq!(cfg.args, vec!["-h", "10.0.0.5", "-P", "3307", "-u", "root"]);
    }

    #[test]
    fn wsl_shell_prepends_client_and_forwards_password_via_wslenv() {
        let cfg = mysql_shell_config(MysqlLauncher::Wsl, &conn(), Some("shop"));
        assert_eq!(cfg.program, "wsl.exe");
        assert_eq!(
            cfg.args,
            vec![
                "-e", "mysql", "-h", "10.0.0.5", "-P", "3307", "-u", "root", "shop"
            ]
        );
        assert_eq!(
            cfg.env,
            vec![
                ("WSLENV".to_string(), "MYSQL_PWD/u".to_string()),
                ("MYSQL_PWD".to_string(), "s3cr3t".to_string()),
            ]
        );
        assert!(!cfg.args.iter().any(|a| a.contains("s3cr3t")));
    }

    #[test]
    fn inline_outcome_success_returns_stripped_sql() {
        let out = inline_outcome(true, b"```sql\nSELECT 1\n```", b"");
        assert!(matches!(out, InlineAiState::Ready(sql) if sql == "SELECT 1"));
    }

    #[test]
    fn inline_outcome_blank_success_is_no_sql_returned() {
        let out = inline_outcome(true, b"   \n", b"");
        assert!(matches!(out, InlineAiState::Failed(m) if m == "No SQL returned"));
    }

    #[test]
    fn inline_outcome_failure_surfaces_first_stderr_line() {
        let out = inline_outcome(false, b"", b"boom: bad model\nsecond line");
        assert!(matches!(out, InlineAiState::Failed(m) if m == "boom: bad model"));
        // Empty stderr → a generic fallback message.
        let out = inline_outcome(false, b"", b"");
        assert!(matches!(out, InlineAiState::Failed(m) if m == "generation failed"));
    }
}
