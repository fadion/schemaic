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
mod update;

/// Process-wide heap accounting (live/peak bytes), for leak-vs-retention
/// diagnosis. Delegates to the system allocator; only adds two atomics per
/// alloc. Logging is opt-in via `SCHEMAIC_HEAP_LOG` (see `heap::spawn_logger`).
#[global_allocator]
static GLOBAL: heap::Tracking = heap::Tracking;

use ai::{
    AiContextParams, AiSession, AiSettings, AiStreamMsg, RECAP_QUESTIONS, StartAiParams,
    active_tab_database, ai_context, apply_turn_delta, extract_sql, inline_system_prompt,
    mcp_endpoint_from_env, render_recap, scoped_database, start_ai_session, turn_context,
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
    RwSignal, Scope, SignalGet, SignalTrack, SignalUpdate, SignalWith, create_effect, create_memo,
};
use floem::window::{Icon, WindowConfig};
use schemaic_core::connection::{ConnStatus, Connection};
use schemaic_core::edit::analyze_edit;
use schemaic_core::health;
use schemaic_core::model::{CommitDone, GridWrite, QueryState, RefetchRequest, ResultSet};
use schemaic_core::monitor::{Snapshot, TickAction, diff_snapshots};

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
/// An action declared before the closure that performs it exists, filled in once
/// it does. `app_view` builds its actions in dependency order, and the few places
/// where that order can't hold both ways — an early action needing a later one —
/// read through one of these rather than being duplicated.
type LateAction<T> = Rc<RefCell<Option<Rc<dyn Fn(T)>>>>;

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
    /// The `.sql` file the tab was bound to, and the file state that goes with it
    /// — so Ctrl+Shift+T brings back a *file* tab, not an untitled copy of its text.
    path: Option<std::path::PathBuf>,
    disk_sql: Option<String>,
    file_format: schemaic_core::sqlfile::SqlFormat,
}
/// Record one executed query into history: `(conn_id, database, sql, tab_name)`,
/// returning the **run id** it was recorded under — what
/// [`FinishHistoryFn`] later reports that run's outcome against.
/// Record a run's statements and hand back one run id each, in order.
///
/// A **slice**, and one file write for the lot. It took a single statement and
/// wrote the whole of `history.json` each time — clone the cross-connection
/// vector, serialize it, temp file, read-back, `.bak`, rename — so Run Everything
/// on a hundred-statement migration did that a hundred times in one UI-thread
/// handler, ~500 fs operations and O(N × min(N, MAX_PER_CONN)) entry
/// serializations, before the batch was even spawned. `finish_history` was given
/// this shape by an earlier fix; only the launch half was left.
type RecordHistoryFn = Rc<dyn Fn(u64, Option<String>, &[String], Option<String>) -> Vec<u64>>;

/// Resolve the pinned session a tab's statements must run on: `Ok(None)` in
/// Auto-commit (fresh connection per op, as everywhere else), `Err` when the tab
/// is Manual but its connection isn't up.
type SessionForFn = Rc<dyn Fn(&Tab) -> Result<Option<Arc<Session>>, String>>;
/// End a tab's transaction — `(tab id, commit?, what to run once it's settled)`.
type EndTxFn = Rc<dyn Fn(usize, bool, Option<Rc<dyn Fn()>>)>;
/// Settle an open transaction before an action that would strand it (or wait
/// behind it) — `(tab id, the action to resume once it's settled, what to do if
/// the user backs out)`.
///
/// The cancel arm is `None` for every caller whose action simply doesn't happen
/// (a tab isn't closed, a mode isn't switched). It is `Some` for the one caller
/// that has already told a modal work is under way and has to take that back.
type GuardTxFn = Rc<dyn Fn(usize, Rc<dyn Fn()>, Option<Rc<dyn Fn()>>)>;
/// The same shape, for the guard that asks about a tab's **unsaved `.sql` file**
/// before closing it — so `guard_close` can compose the two and drop into every
/// place that already takes a [`GuardTxFn`].
type GuardCloseFn = GuardTxFn;
/// Start one database's introspection against a `Db` — the single path the
/// initial load, the connection-wide Refresh and the per-database Refresh all
/// take, so what the tree shows while a fetch is out is decided once.
type FetchSchemaFn = Rc<dyn Fn(&ConnNode, Db)>;
/// Record how one or more runs turned out — `(run id, outcome)` per run — onto
/// the history entries their launch already wrote.
///
/// A **slice**, not one run, because Run Everything lands a whole batch at once
/// and each recorded run costs a full rewrite of `history.json`.
/// Fill in how runs went, and delete the entries of runs that never happened.
///
/// One call for the whole slice, and one file write for both halves.
type FinishHistoryFn = Rc<dyn Fn(&[(u64, schemaic_core::history::RunResult)], &[u64])>;
use schemaic_core::filter::{BrowseKey, Order, table_query};
use schemaic_core::intel::SqlDialect;
use schemaic_core::persist::{self, ConnectionsFile, UiState};
use schemaic_core::schema::{SchemaState, TableSource};
use schemaic_core::sql::{GuardPolicy, RunVerdict, run_verdict};
use schemaic_core::tx::{
    StmtOutcome, TabTx, TxEngine, TxMode, TxState, ddl_blocking_tabs, session_still_wanted,
};
use schemaic_db::{Db, DbError, Session};
use schemaic_ui::theme::{EditorThemeKind, UiThemeKind};
use schemaic_ui::{
    AiActions, AiEffort, AiModel, AiUi, ChatMessage, Confirm, ConnActions, ConnNode, ConnUi,
    CtxMenu, DdlOutcome, DraftSignals, HistoryActions, HistoryUi, InlineAiRequest, InlineAiState,
    LayoutUi, MonitorEntry, OverlayUi, PendingRun, PlanState, ResultPanel, RightPanel, Role,
    RunGuard, SchemaActions, SchemaScope, SchemaUi, Tab, TabsActions, TabsUi, TermActions,
    TermCursor, TermUi, TestState, TxChoice, TxPrompt, Ui, pick_connection_color,
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

    // Velopack's startup hook, and it has to run before *everything* below —
    // before tracing, the font registration, the tokio runtime and any Floem
    // signal or `Scope`. The installer and the updater re-invoke this exe with
    // `--veloapp-install` / `--veloapp-updated` / `--veloapp-obsolete` /
    // `--veloapp-uninstall` / `--veloapp-firstrun` to run lifecycle work, and
    // `run()` services those and then **terminates the process**. Anything set up
    // ahead of it is built only to be thrown away — or worse, half-initialised
    // when the process dies mid-hook.
    //
    // The one thing it deliberately sits *after* is the `--mcp-serve` early exit
    // above, which is a different program: a stdio JSON-RPC server whose stdout is
    // the protocol stream, so nothing may write to stdout ahead of it. The two
    // never collide — the `--veloapp-*` args come from the installer, `--mcp-serve`
    // from the `claude` CLI, and neither invocation passes the other's flag — so
    // ordering between them is free, and this way the protocol stream stays clean.
    //
    // With no hook args present (the normal user launch) `run()` returns
    // immediately, so this costs nothing on a cold start.
    velopack::VelopackApp::build().run();

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

    // **No system title bar** — the app draws its own (`ui::window_chrome`), so
    // the header carries the connection switcher on the left and the caption
    // buttons on the right, in one strip.
    //
    // `show_titlebar(false)` rather than `undecorated(true)`, and the difference
    // is per-platform: floem turns the former into a genuinely undecorated
    // window on Windows/Linux, but on macOS into a *transparent* title bar over
    // a full-size content view, which keeps the traffic lights and the native
    // resize behaviour. `undecorated` would throw those away too, on the one
    // platform where they still work.
    let chrome = schemaic_core::window_chrome::Chrome::current();
    let mut config = WindowConfig::default()
        .size(Size::new(1280.0, 820.0))
        .show_titlebar(false)
        // Windows only, and a no-op elsewhere: keeps the DWM drop shadow (and
        // with it the window's visual edge) behind a frameless window.
        .undecorated_shadow(chrome.wants_drop_shadow())
        .title(schemaic_core::APP_NAME);
    if let Some(icon) = app_icon() {
        config = config.window_icon(icon);
    }

    Application::new()
        .window(move |id| app_view(handle.clone(), id), Some(config))
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

/// Which client binary a DB-CLI session resolved to, and how it is reached.
enum CliLauncher<'a> {
    /// A native client on `PATH`.
    Native(&'a str),
    /// The client inside WSL, via `wsl.exe -e <prog>` — the fallback on a
    /// Windows box with the server (and its client) installed under WSL.
    Wsl(&'a str),
}

/// Find a client: the first of `progs` on `PATH`, else the first one inside WSL.
/// `None` when neither exists, which is the caller's cue to say so in the
/// terminal rather than spawn something that dies immediately.
fn resolve_cli<'a>(progs: &[&'a str]) -> Option<CliLauncher<'a>> {
    use schemaic_term::shell::which;
    for prog in progs {
        if which(prog).is_some() {
            return Some(CliLauncher::Native(prog));
        }
    }
    which("wsl.exe")
        .is_some()
        .then(|| CliLauncher::Wsl(progs[0]))
}

/// Find a client on `PATH` only, with **no WSL fallback** — see [`sqlite_shell`]
/// for the one client that must not have one.
fn resolve_native_cli(prog: &str) -> Option<CliLauncher<'_>> {
    schemaic_term::shell::which(prog)
        .is_some()
        .then_some(CliLauncher::Native(prog))
}

/// Build the terminal shell that launches the MySQL/MariaDB CLI for `conn`,
/// optionally scoped to `db`. The password rides `MYSQL_PWD` (via `WSLENV` for
/// the WSL case) so it never appears on the command line or in shell history.
fn mysql_shell(
    conn: &schemaic_core::connection::Connection,
    db: Option<&str>,
) -> Option<schemaic_term::ShellConfig> {
    resolve_cli(&["mysql", "mariadb"]).map(|l| mysql_shell_config(l, conn, db))
}

/// The PostgreSQL half of [`mysql_shell`]. `db` is required — see
/// [`psql_database`] for how the caller arrives at one.
fn psql_shell(
    conn: &schemaic_core::connection::Connection,
    db: &str,
) -> Option<schemaic_term::ShellConfig> {
    resolve_cli(&["psql"]).map(|l| psql_shell_config(l, conn, db))
}

/// The SQLite third: `sqlite3 <file>`.
///
/// **Native only, deliberately** ([`resolve_native_cli`]). The other two clients
/// take a host and a port, which mean the same thing inside WSL as outside it; this
/// one takes a *path*, and `sqlite3 'C:\data\app.db'` under WSL does not fail — it
/// **creates an empty database** under that literal name and hands the user a
/// session on something that looks like theirs. Offering it would mean translating
/// the path to `/mnt/c/…`, which nothing here does yet.
fn sqlite_shell(
    conn: &schemaic_core::connection::Connection,
) -> Option<schemaic_term::ShellConfig> {
    resolve_native_cli("sqlite3").map(|l| sqlite_shell_config(l, conn))
}

/// Which database `psql` should open.
///
/// Unlike the MySQL client, psql cannot start a session with no database: given
/// none it connects to one named after the user, which on most servers doesn't
/// exist. So take the caller's explicit choice (the schema tree's "Open in CLI"),
/// else whatever database the user is looking at, else `postgres` — the
/// maintenance database every server is created with.
fn psql_database(explicit: Option<&str>, active: Option<&str>) -> String {
    [explicit, active]
        .into_iter()
        .flatten()
        .find(|d| !d.trim().is_empty())
        .unwrap_or("postgres")
        .to_string()
}

/// Build the launch config for the resolved MySQL client — pure argv +
/// credential env construction, split from the `PATH` probing in [`mysql_shell`]
/// so it's unit-tested. The password always rides `MYSQL_PWD` (forwarded across
/// `WSLENV` in the WSL case) and never lands on the argv.
fn mysql_shell_config(
    launcher: CliLauncher,
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
    wrap_launcher(launcher, cli_args, Some(("MYSQL_PWD", &conn.password)))
}

/// The SQLite twin of [`mysql_shell_config`] — pure argv construction, split from
/// the `PATH` probing in [`sqlite_shell`] so it's unit-tested.
///
/// One argument (the file) and **no credential env at all**: there is no server, so
/// `host`/`port`/`user`/`password` are inert on such a connection and an env var
/// here would be a secret invented for an engine that has none.
///
/// `cwd` is the file's own directory, which the server clients have no equivalent
/// of: `sqlite3`'s dot-commands take relative paths (`.output rows.csv`,
/// `.read seed.sql`), and resolving those against wherever the app was launched
/// from — on a desktop launch, not a directory the user can name — would write
/// files nobody can find. `None` when the path has no directory part, since
/// spawning into `""` fails outright.
fn sqlite_shell_config(
    launcher: CliLauncher,
    conn: &schemaic_core::connection::Connection,
) -> schemaic_term::ShellConfig {
    let mut cfg = wrap_launcher(launcher, vec![conn.file.clone()], None);
    cfg.cwd = std::path::Path::new(&conn.file)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .and_then(|p| p.to_str())
        .map(str::to_string);
    cfg
}

/// [`mysql_shell_config`]'s PostgreSQL twin. Every parameter takes a different
/// flag here (`-p`/`-U`/`-d`, against MySQL's `-P`/`-u`/positional) and the
/// password variable differs too, so the two builders stay separate rather than
/// growing an engine conditional per argument.
fn psql_shell_config(
    launcher: CliLauncher,
    conn: &schemaic_core::connection::Connection,
    db: &str,
) -> schemaic_term::ShellConfig {
    let cli_args: Vec<String> = vec![
        "-h".into(),
        conn.host.clone(),
        "-p".into(),
        conn.port.to_string(),
        "-U".into(),
        conn.user.clone(),
        "-d".into(),
        db.to_string(),
    ];
    wrap_launcher(launcher, cli_args, Some(("PGPASSWORD", &conn.password)))
}

/// Turn a client's argv into a spawnable config, native or through WSL, with any
/// password in `secret`'s variable — the half every engine shares, so none of them
/// can lose the rule that the password never reaches the command line.
///
/// `secret` is `None` for a client that has no credential to pass (SQLite's), which
/// is not the same as an empty password: it means no variable is set at all, and no
/// `WSLENV` entry naming one.
fn wrap_launcher(
    launcher: CliLauncher,
    cli_args: Vec<String>,
    secret: Option<(&str, &str)>,
) -> schemaic_term::ShellConfig {
    let env = |prefix: Vec<(String, String)>| match secret {
        Some((var, password)) => {
            let mut env = prefix;
            env.push((var.into(), password.to_string()));
            env
        }
        None => Vec::new(),
    };
    match launcher {
        CliLauncher::Native(prog) => schemaic_term::ShellConfig {
            program: prog.into(),
            args: cli_args,
            cwd: None,
            env: env(Vec::new()),
        },
        CliLauncher::Wsl(prog) => {
            let mut args: Vec<String> = vec!["-e".into(), prog.into()];
            args.extend(cli_args);
            schemaic_term::ShellConfig {
                program: "wsl.exe".into(),
                args,
                cwd: None,
                // WSLENV is what carries the variable across the boundary; without
                // it the password simply doesn't arrive and psql/mysql prompts.
                env: env(match secret {
                    Some((var, _)) => vec![("WSLENV".into(), format!("{var}/u"))],
                    None => Vec::new(),
                }),
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
/// An action with no arguments, held by the connection gate across an async
/// re-check.
type Action = Rc<dyn Fn()>;
/// Runs an [`Action`], but only against a connection that answers.
type ConnGate = Rc<dyn Fn(Action)>;
/// Reports a health check's outcome.
type CheckDoneFn = Rc<dyn Fn(bool)>;

/// Wrap a one-argument action behind the live-connection gate.
///
/// The argument is cloned per invocation because the gate may hold the action
/// across an async re-check and call it later.
fn gate1<A: Clone + 'static>(gate: &ConnGate, action: &Rc<dyn Fn(A)>) -> Rc<dyn Fn(A)> {
    let gate = gate.clone();
    let action = action.clone();
    Rc::new(move |arg: A| {
        let action = action.clone();
        (gate)(Rc::new(move || action(arg.clone())));
    })
}

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
///
/// The same `Engine::dialect()` [`dialect_for`] uses, which is exhaustive. This
/// was a two-engine `if Postgres { … } else { MySql }` that sorted SQLite onto
/// the MySQL side — the shape a third engine makes silently wrong.
fn dialect_of(db: &Db) -> SqlDialect {
    dialect_for(db.engine())
}

/// One loaded table's DDL, the columns that identify one of its rows, and the
/// implicit row key it has if it has none of its own
/// ([`schemaic_core::schema::TableInfo::implicit_key`] — SQLite's rowid, `None`
/// on the other two engines). Everything empty when the schema isn't loaded yet.
///
/// The middle value is `schema::browse_key_columns`, **not** the primary key:
/// the same precedence `edit::resolve_key` uses, so the statement the grid runs
/// and the key the write path resolves cannot disagree about whether the table
/// has a key of its own.
fn table_ddl_and_pk(
    db_nodes: RwSignal<Vec<ConnNode>>,
    source: &TableSource,
    dialect: SqlDialect,
) -> (String, Vec<String>, Option<String>) {
    db_nodes
        .with_untracked(|nodes| {
            nodes
                .iter()
                .find(|n| n.database == source.database)
                .and_then(|n| match n.schema.get_untracked() {
                    schemaic_core::schema::SchemaState::Loaded(s) => s
                        .find_table(source.schema.as_deref(), &source.table)
                        .map(|t| {
                            (
                                t.create_ddl(dialect),
                                schemaic_core::schema::browse_key_columns(t),
                                t.implicit_key.clone(),
                            )
                        }),
                    _ => None,
                })
        })
        .unwrap_or_default()
}

/// The bottom-sample query for AI seed data: most-recent rows by primary key
/// (`ORDER BY <pk> DESC`) so enums/sequences/FK values are representative.
///
/// No implicit key is passed: this sample is read to *describe* the table's data
/// to the model, never written back, and a rowid column would be a column of
/// noise in the prompt rather than a row identity anything here needs.
fn sample_sql(engine: schemaic_db::Engine, source: &TableSource, pk_cols: &[String]) -> String {
    table_query(
        dialect_for(engine),
        &source.database,
        source.schema.as_deref(),
        &source.table,
        BrowseKey::pick(pk_cols, None),
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
    engine.dialect()
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

/// What a schema load that has just landed is still allowed to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LoadLanding {
    /// Nothing superseded it: install the nodes, bind the tabs, fan the
    /// per-database fetches out.
    Install,
    /// Something newer owns the tree now. The tunnel this load opened is still
    /// worth caching — it belongs to its *connection*, which the user may return
    /// to, and dropping it would only force a reconnect — but nothing else it
    /// carries may touch shared state.
    KeepTunnelOnly,
}

/// Is a landed schema load still the one the UI is waiting for?
///
/// `started` is the `(connection id, generation)` the load stamped itself with;
/// `current` is what those are now. A load is a `fetch_databases` and, over an
/// SSH tunnel, a connect before it — seconds, during which the user can switch
/// connection or press Refresh again.
///
/// Both halves earn their place. The connection id is the case the user sees: a
/// slow remote load landing after a fast local one repoints the tree, the
/// active-database menu, the completion index and the grid's key icons at a
/// connection every query has stopped using. The generation is the case an id
/// check alone misses — two loads of the *same* connection, where the first to
/// land installs the older node list and disposes the newer one's scope.
fn load_landing(started: (u64, u64), current: (u64, u64)) -> LoadLanding {
    if started == current {
        LoadLanding::Install
    } else {
        LoadLanding::KeepTunnelOnly
    }
}

/// What one database of a landed connection load becomes: an existing node kept,
/// or a fresh node at this id. Both carry the node id, because the schema tree's
/// `dyn_stack` is keyed on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NodePlan {
    /// The database was already on screen: reuse its node, and with it its
    /// `schema` signal (so the rows stay up while the re-introspection runs) and
    /// its id (so the `dyn_stack` doesn't rebuild it at all).
    Keep(usize),
    Create(usize),
}

/// Which node each database name gets on a landed connection load.
///
/// Extracted because it silently decides three things nobody could assert while
/// it lived inside a closure inside a closure: that a database dropped and
/// re-created gets a **fresh** id rather than colliding with a live one, that
/// reordering the server's list renumbers nothing (the tree keys on id, so it
/// would rebuild every row), and that a reload against an empty node list still
/// produces a usable set.
///
/// `reload` is "this is the connection already on screen". A **switch** reuses
/// nothing: the rows would be another server's.
fn plan_nodes(existing: &[(usize, String)], names: &[String], reload: bool) -> Vec<NodePlan> {
    let existing = if reload { existing } else { &[][..] };
    // Past every id in use, so a name that has come back doesn't take the id of
    // one that is still there.
    let mut next_id = existing.iter().map(|(id, _)| *id).max().unwrap_or(0) + 1;
    names
        .iter()
        .map(|name| match existing.iter().find(|(_, db)| db == name) {
            Some((id, _)) => NodePlan::Keep(*id),
            None => {
                let id = next_id;
                next_id += 1;
                NodePlan::Create(id)
            }
        })
        .collect()
}

/// May a landed **per-database** introspection write its result?
///
/// [`load_landing`]'s counterpart, one level down, and the level that had no
/// guard at all. `load_landing` covers the `fetch_databases` leg; the
/// per-database legs it fans out were written with `sig.try_update`, which
/// guards a *disposed* scope — a connection switch — and says nothing about a
/// **superseded** fetch of the same node.
///
/// The interleaving is ordinary: press the SCHEMA header's Refresh (one fetch
/// out per database, slow over an SSH tunnel), then apply an `ALTER TABLE`,
/// whose own `refresh_db` starts a second fetch of that database and lands
/// first with the post-`ALTER` schema. The connection-wide one then lands with
/// its **pre-`ALTER`** snapshot and overwrites it. Nothing detects that and
/// nothing schedules another refresh, so the tree, the completion index,
/// `intel`'s catalog and `table_designer::loaded_table` hold the pre-apply model
/// indefinitely — and reopening the designer emits a `MODIFY COLUMN` restating
/// the old definition, which destroys what the `ALTER` added. MySQL's `MODIFY`
/// replaces the whole column, so nothing warns and `risks()` discloses nothing:
/// from the plan's view the type did not change.
///
/// Last writer *asked* wins, not last to land.
fn fetch_landing(started: u64, current: u64) -> bool {
    started == current
}

/// Where the session's run-id counter starts: past **every** id on disk, across
/// all connections. Each `record_history` then hands out `seed + 1`, `+ 2`, …
///
/// Three properties, and the whole of the argument that a landing run reports
/// against the entry it launched:
///
/// - **Global, not per-connection.** Ids are matched by `finish` without a
///   connection filter, so a per-connection seed would let two connections issue
///   the same id and let one run's outcome land on the other's entry.
/// - **Only ever counting up.** Re-deriving `max + 1` per push would reuse an id
///   the moment the per-connection cap evicted the entry holding the maximum —
///   while the run holding it was still in flight.
/// - **Never zero.** Entries written before run ids exist carry `0`, so the
///   first id handed out must not be one, or a landing run would claim a legacy
///   entry. `max().unwrap_or(0)` on an empty history seeds 0 and the first
///   allocation is 1.
fn run_id_seed(entries: &[schemaic_core::history::HistoryEntry]) -> u64 {
    entries.iter().map(|e| e.run_id).max().unwrap_or(0)
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
        file: String::new(),
        ssh: Default::default(),
        color: None,
        prominent_color: false,
        read_only: false,
        environment: Default::default(),
    }
}

/// What a finished run should record in history, or `None` for one that reached
/// no verdict — still running, or cancelled, where the honest answer is the
/// nothing the entry already says.
///
/// Shared by both run paths so a single run and a statement inside Run
/// Everything can't come to answer this differently. A thin match over
/// `QueryState`; what each arm *records* is `RunResult::loaded`/`failed`, in
/// core and under test.
fn run_result(state: &QueryState, duration_ms: u64) -> Option<schemaic_core::history::RunResult> {
    use schemaic_core::history::RunResult;
    match state {
        QueryState::Loaded(rs) => Some(RunResult::loaded(
            duration_ms,
            rs.affected,
            rs.row_count() as u64,
            rs.truncated,
        )),
        QueryState::Failed(_) => Some(RunResult::failed(duration_ms)),
        QueryState::Idle | QueryState::Running | QueryState::Cancelled => None,
    }
}

/// Which engine's transaction semantics apply — the divergence that
/// [`schemaic_core::tx`] encodes (Postgres poisons a transaction on any error;
/// MySQL implicitly commits on DDL).
///
/// SQLite takes MySQL's arm and never reaches it: manual-transaction mode isn't
/// offered on a SQLite connection and `Session::open` refuses one. Of the two,
/// MySQL's is the safe default to answer with — Postgres' would report a
/// transaction as poisoned when there is no transaction at all.
fn tx_engine(db: &Db) -> TxEngine {
    match db.engine() {
        schemaic_db::Engine::Postgres => TxEngine::Postgres,
        schemaic_db::Engine::MySql | schemaic_db::Engine::Sqlite => TxEngine::MySql,
    }
}

fn app_view(handle: tokio::runtime::Handle, window: floem::window::WindowId) -> impl IntoView {
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

    // Identity colours, both stores out of one file (persisted; set from the schema
    // tree's right-click menu). Per-database — keyed by connection+database, shown
    // as a dot on the DB node, the active-DB selector and the database's query tabs
    // — and per-table, keyed by connection+database+display name, shown as a dot on
    // the table row and as a tint on the table's ER-diagram card header.
    let colors = persist::load_json::<schemaic_core::db_color::DbColorsFile>("db_colors.json");
    let db_colors = RwSignal::new(colors.rules);
    let table_colors = RwSignal::new(colors.tables);
    // One save for the pair — they share `db_colors.json`, so writing either half
    // has to write both or the other is lost.
    let save_db_colors: Rc<dyn Fn()> = Rc::new(move || {
        persist::save_json(
            "db_colors.json",
            &schemaic_core::db_color::DbColorsFile {
                rules: db_colors.get_untracked(),
                tables: table_colors.get_untracked(),
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
    let saved_tabs = {
        let saved = persist::load_json::<schemaic_core::persist::SavedTabsFile>("tabs.json");
        if ui_state.restore_tabs {
            saved
        } else {
            // **The setting off is not a licence to drop unrecoverable text.** A
            // file-backed tab with unsaved edits is the one tab whose text is
            // neither on disk nor retypeable, and a window quit is the one way of
            // losing it that cannot ask first (floem 0.2 can't veto a close). The
            // same subset is what the flush writes while the setting is off — read
            // here as well so a full session left over from when it was *on* isn't
            // silently restored either.
            saved.unsaved_files_only()
        }
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
                        // What a restored tab knows about its file is one
                        // decision with four inputs, and it lives in
                        // `sqlfile::restored_binding` where it is tested — the
                        // combination that goes wrong (dirty, restored as clean)
                        // silently drops the modified marker and makes Ctrl+S a
                        // no-op over the user's unsaved work.
                        let binding = schemaic_core::sqlfile::restored_binding(
                            s.path.clone(),
                            s.file_dirty,
                            &s.query,
                            schemaic_core::sqlfile::SqlFormat {
                                crlf: s.file_crlf,
                                bom: s.file_bom,
                                lossy: s.file_lossy,
                            },
                        );
                        t.path.set(binding.path);
                        t.file_format.set(binding.format);
                        t.disk_sql.set(binding.disk_sql);
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
    // built in. A `load_schema` that switches connection swaps in a fresh scope
    // and disposes the old one, so a session's connection switches don't accrete
    // orphaned schema signals (review C14). A *reload of the same connection*
    // keeps the scope, because it keeps the nodes — see `nodes_conn`.
    let nodes_scope: Rc<RefCell<Option<Scope>>> = Rc::new(RefCell::new(None));
    // The connection the installed `db_nodes` belong to. `active_conn` can't
    // answer this: it is set when the user picks a connection, which is *before*
    // that connection's databases have been listed, so during a switch it names
    // the connection whose nodes are not on screen yet. The whole `Connection`,
    // not its id, so `targets_same_server` can see an edit in place — a saved
    // connection repointed at another host keeps its id.
    let nodes_conn: Rc<RefCell<Option<Connection>>> = Rc::new(RefCell::new(None));

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
            let tunnel = if conn.uses_tunnel() {
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
    // Consecutive failed health checks of the active connection, folded by every
    // check (polled or manual). Drives the health poll's backoff so a server
    // that's been down for a while isn't probed every 10s; reset on switch.
    let health_failures = RwSignal::new(0u32);
    // OS window focus, set from the workspace root. Starts `true`: the window is
    // focused on launch and winit only reports the *changes*.
    let window_focused = RwSignal::new(true);
    // Pending "you have an open transaction" question (see `TxPrompt`).
    let tx_prompt: RwSignal<Option<TxPrompt>> = RwSignal::new(None);
    // The shared "are you sure?" channel (see `Confirm`) — one modal for every
    // destructive action, rather than one modal each.
    let confirm: RwSignal<Option<Confirm>> = RwSignal::new(None);

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
    let monitor_partial: RwSignal<bool> = RwSignal::new(false);
    let monitor_interval: RwSignal<u64> = RwSignal::new(MONITOR_INTERVAL_SECS);
    let monitor_paused: RwSignal<bool> = RwSignal::new(false);
    let monitor_export_err: RwSignal<Option<String>> = RwSignal::new(None);
    let monitor_exported: RwSignal<bool> = RwSignal::new(false);
    let monitor_dropped: RwSignal<usize> = RwSignal::new(0);
    // Table-properties modal. `properties` is both the open flag and the object
    // being described, so a stale fetch can check it before writing.
    let properties: RwSignal<Option<schemaic_ui::PropertiesTarget>> = RwSignal::new(None);
    let properties_state: RwSignal<schemaic_ui::PropertiesState> =
        RwSignal::new(schemaic_ui::PropertiesState::Loading);
    let properties_counting: RwSignal<bool> = RwSignal::new(false);
    let properties_count_err: RwSignal<Option<String>> = RwSignal::new(None);
    // The schema tree's size column (persisted; see `UiState::show_table_sizes`).
    let table_sizes = RwSignal::new(ui_state.show_table_sizes);
    // Bumped whenever a refresh puts some node's statistics back to `Idle`, and
    // read by the size-column effect below — the *only* thing that tells it to
    // look again.
    //
    // `ConnNode::stats` can't do that job itself. The effect reads each node's
    // slot with `get_untracked`, deliberately: it writes `Loading` into those
    // same slots, and tracking them would re-enter the effect mid-loop and
    // double-fetch the databases it hadn't reached yet. So the reset at
    // `start_fetch` is invisible to it, and both refresh paths used to leave the
    // column blank until something unrelated (the toggle, an expand, a
    // connection switch) happened to re-run it. `db_nodes` is not that
    // something: the connection-wide refresh does `set` it, but *before*
    // `start_fetch` resets the slots, so that run still sees them `Loaded` and
    // finds nothing to do.
    let stats_gen: RwSignal<u64> = RwSignal::new(0);

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
        // `ChatFile::of` is what drops the tool results — query output, i.e. the
        // user's own rows — on the way to disk.
        persist::save_json(
            "chats.json",
            &schemaic_core::chat::ChatFile::of(&saved_chats.get_untracked()),
        );
    });
    let (ai_tx, ai_rx) = crossbeam_channel::unbounded::<AiStreamMsg>();
    let ai_stream = create_signal_from_channel(ai_rx);

    // Run ids, handed out by `record_history` and quoted back by
    // `finish_history` — see `HistoryEntry::run_id`. Seeded past every id on
    // disk, and only ever counting up, so an id can't be reused while the run
    // holding it is still in flight (which re-deriving `max + 1` per push would
    // allow, once the per-connection cap evicted the entry holding the maximum).
    let run_ids: Rc<Cell<u64>> = Rc::new(Cell::new(
        history_entries.with_untracked(|v| run_id_seed(v)),
    ));

    // Record an executed query into the history (newest-first, capped) and persist
    // it. Called from every run path (single Run, Run Current, Run Everything).
    let record_history: RecordHistoryFn = {
        let run_ids = run_ids.clone();
        Rc::new(
            move |conn_id: u64,
                  database: Option<String>,
                  stmts: &[String],
                  tab_name: Option<String>| {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                // The connection's own dialect: `push` skips credential-bearing
                // statements, and where a string or comment ends differs per
                // engine.
                let dialect = connections
                    .with_untracked(|cs| {
                        cs.iter()
                            .find(|c| c.id == conn_id)
                            .map(|c| SqlDialect::from_db_type(&c.db_type))
                    })
                    .unwrap_or_default();
                let mut ids = Vec::with_capacity(stmts.len());
                let mut wrote = false;
                history_entries.update(|v| {
                    for sql in stmts {
                        let run_id = run_ids.get() + 1;
                        run_ids.set(run_id);
                        ids.push(run_id);
                        wrote |= schemaic_core::history::push(
                            v,
                            schemaic_core::history::HistoryEntry {
                                conn_id,
                                database: database.clone(),
                                sql: sql.clone(),
                                ts,
                                run_id,
                                tab_name: tab_name.clone(),
                                // Filled in by `finish_history` when the run lands.
                                duration_ms: None,
                                rows: None,
                                rows_capped: false,
                                outcome: schemaic_core::history::Outcome::Unknown,
                            },
                            dialect,
                        );
                    }
                });
                // Skipped when nothing was recorded — the same skip
                // `finish_history` documents. A credential-bearing statement
                // records nothing and used to cost a whole atomic rewrite for it.
                if wrote {
                    persist::save_json(
                        "history.json",
                        &schemaic_core::history::HistoryFile {
                            entries: history_entries.get_untracked(),
                        },
                    );
                }
                ids
            },
        )
    };

    // Fill in how runs went, on the entries `record_history` wrote when they
    // launched (see `history::finish` for why it is two passes and not one).
    //
    // Persists once for the whole slice, not once per statement: a save clones
    // the entire history, serializes it, and does an atomic write (temp file,
    // read-back, `.bak`, rename). Run Everything on a migration script lands a
    // hundred statements in a single UI-thread callback, and one write each
    // froze the window for as long as that took. The write is also skipped
    // entirely when nothing was updated — a credential-bearing statement is
    // never recorded, and would otherwise cost a file write for nothing.
    // `dropped` is the runs that never happened — the tail of a script that
    // stopped, which was pushed at launch and would otherwise evict the
    // connection's real history under `MAX_PER_CONN`. See `history::drop_runs`.
    let finish_history: FinishHistoryFn = Rc::new(
        move |runs: &[(u64, schemaic_core::history::RunResult)], dropped: &[u64]| {
            let updated = history_entries.try_update(|v| {
                let mut any = schemaic_core::history::drop_runs(v, dropped);
                for (run_id, result) in runs {
                    any |= schemaic_core::history::finish(v, *run_id, *result);
                }
                any
            });
            if updated != Some(true) {
                return;
            }
            persist::save_json(
                "history.json",
                &schemaic_core::history::HistoryFile {
                    entries: history_entries.get_untracked(),
                },
            );
        },
    );

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
        let finish_history = finish_history.clone();
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
            // The run id this launch was recorded under, quoted back when it
            // lands. `None` for a view re-run, which records no history at all.
            let run_id = (!is_view)
                .then(|| {
                    (record_history)(
                        tab.conn_id.get_untracked(),
                        database.clone(),
                        std::slice::from_ref(&sql),
                        tab.name.get_untracked(),
                    )
                })
                .and_then(|ids| ids.first().copied());

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
            let finish_history = finish_history.clone();
            let send = create_ext_action(
                cx,
                move |(state, stmt, took): (QueryState, Option<StmtOutcome>, u64)| {
                    // Fold the transaction state first, and unconditionally: it
                    // tracks the *connection*, so it stays true even when a newer run
                    // has superseded this one for display purposes.
                    if let Some(stmt) = stmt {
                        tab.tx
                            .update(|t| *t = t.on_statement(engine, &tx_sql, stmt));
                    }
                    // History is about the *run*, not this tab's display, so it
                    // too is recorded before the supersede check — and only for a
                    // run that reached a verdict (a cancelled one leaves the entry
                    // saying it ran, which is all anyone knows). Quoting the id
                    // back is what keeps a superseded run from overwriting the one
                    // that replaced it.
                    if let Some(run_id) = run_id
                        && let Some(result) = run_result(&state, took)
                    {
                        // Nothing dropped: a *single* run the user cancels was
                        // dispatched, may have written something, and is the
                        // entry they are most likely to want back.
                        (finish_history)(&[(run_id, result)], &[]);
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
            handle.spawn(async move {
                // Wall-clock, and around everything: connecting, the statement,
                // and pulling the rows back are all time the user waited.
                let started = std::time::Instant::now();
                let (res, stmt) = match &session {
                    Some(s) => {
                        // `BEGIN` is issued lazily, on the first statement of a
                        // transaction, so flipping to Manual and changing your
                        // mind costs nothing. The session decides whether one is
                        // needed under its own lock — asking `TxState` here would
                        // read a signal that isn't folded until an in-flight
                        // operation finishes, so two runs could both `BEGIN`.
                        if let Err(e) = s.ensure_tx().await {
                            (Err(e), None)
                        } else {
                            let out = s.fetch_query(&sql, cap, token).await;
                            (out.result, Some(out.stmt))
                        }
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
                send((state, stmt, started.elapsed().as_millis() as u64));
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
            monitor_partial.set(false);
            monitor_paused.set(false);
            monitor_export_err.set(None);
            monitor_exported.set(false);
            monitor_dropped.set(0);
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
                partial: monitor_partial,
                prev: monitor_prev.clone(),
                key_cols: monitor_key_cols.clone(),
                generation: monitor_gen.clone(),
                started: Instant::now(),
                target: (conn_id, source),
                interval: monitor_interval,
                paused: monitor_paused,
                exported: monitor_exported,
                dropped: monitor_dropped,
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
        let finish_history = finish_history.clone();
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
            // One run id per statement, in the same order, quoted back when the
            // batch lands. A statement repeated in one script de-duplicates down
            // to a single entry, so only the later of the two ids matches — which
            // is the run whose result the entry should be reporting.
            let stmt_run_ids: Vec<u64> =
                (record_history)(conn_id, database.clone(), &stmts, tab_name.clone());

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
            let finish_history = finish_history.clone();
            let send = create_ext_action(
                cx,
                move |(states, outcomes, took): (
                    Vec<QueryState>,
                    Vec<Option<StmtOutcome>>,
                    Vec<u64>,
                )| {
                    for (sql, stmt) in tx_stmts.iter().zip(&outcomes) {
                        if let Some(stmt) = stmt {
                            tab.tx.update(|t| *t = t.on_statement(engine, sql, *stmt));
                        }
                    }
                    // Before the supersede check, for the same reasons as a single
                    // run's — and handed over as one batch, so the whole script
                    // costs one history save rather than one per statement.
                    let mut runs: Vec<(u64, schemaic_core::history::RunResult)> = Vec::new();
                    // A statement that reached no verdict in a *batch* never
                    // ran: the batch stops at its first failure (or at the
                    // user's cancel) and reports every statement after it
                    // `Cancelled` without dispatching it. Recorded at launch,
                    // because an entry has to exist while a query is in flight —
                    // but a 60-statement script failing at statement 2 then
                    // evicted the connection's 50 real entries in favour of 48
                    // that never ran, indistinguishable from cancelled ones.
                    let mut undispatched: Vec<u64> = Vec::new();
                    for ((run_id, state), ms) in stmt_run_ids.iter().zip(&states).zip(&took) {
                        match run_result(state, *ms) {
                            Some(r) => runs.push((*run_id, r)),
                            None => undispatched.push(*run_id),
                        }
                    }
                    if !runs.is_empty() || !undispatched.is_empty() {
                        (finish_history)(&runs, &undispatched);
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
            handle.spawn(async move {
                let mut states: Vec<QueryState> = vec![QueryState::Cancelled; n];
                let mut outcomes: Vec<Option<StmtOutcome>> = vec![None; n];
                // Wall-clock per statement, for history. A statement that never
                // ran keeps its 0 — `run_result` reads nothing off a cancelled
                // one, so the number is never shown.
                let mut took: Vec<u64> = vec![0; n];
                let mut clock = std::time::Instant::now();
                match &session {
                    Some(s) => {
                        // See `run_query_core`: the session owns the decision, and
                        // a failed BEGIN aborts rather than running the batch
                        // outside the transaction the user asked for.
                        if let Err(e) = s.ensure_tx().await {
                            states[0] = QueryState::Failed(e.to_string());
                            took[0] = clock.elapsed().as_millis() as u64;
                            send((states, outcomes, took));
                            return;
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
                            took[i] = clock.elapsed().as_millis() as u64;
                            clock = std::time::Instant::now();
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
                        // The callback fires as each statement lands, so the gap
                        // since the previous one *is* that statement's wall-clock
                        // — the batch runs them back to back on one connection.
                        db.run_batch(database.as_deref(), &stmts, cap, token, |i, res| {
                            took[i] = clock.elapsed().as_millis() as u64;
                            clock = std::time::Instant::now();
                            states[i] = match res {
                                Ok(rs) => QueryState::Loaded(Arc::new(rs)),
                                Err(DbError::Cancelled) => QueryState::Cancelled,
                                Err(e) => QueryState::Failed(e.to_string()),
                            };
                        })
                        .await;
                    }
                }
                send((states, outcomes, took));
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

    // ── File import ─────────────────────────────────────────────────────────
    // Read a file's opening records so the modal can show what it found. On a
    // worker thread: the path comes from a file dialog and could be anything —
    // a huge file, a slow network share — and the window must stay live.
    //
    // Only the first bytes are read for the sniff, and only `SAMPLE_ROWS`
    // records for the preview, so opening a 2GB CSV costs the same as a small
    // one. A JSON *array* is the exception — its structure isn't known until the
    // closing bracket, so `read_sample` has to parse the whole thing (see
    // `import::json_records`); JSON Lines samples as cheaply as CSV.
    let import_probe: schemaic_ui::ImportProbeFn = {
        let handle = handle.clone();
        Rc::new(
            move |req: schemaic_ui::ImportProbeRequest, done: schemaic_ui::ImportProbeDoneFn| {
                const SNIFF_BYTES: usize = 64 * 1024;
                const SAMPLE_ROWS: usize = 200;
                let report = create_ext_action(
                    cx,
                    move |res: Result<schemaic_ui::ImportProbeResult, String>| (done)(res),
                );
                handle.spawn_blocking(move || {
                    let probe = || -> Result<schemaic_ui::ImportProbeResult, String> {
                        use std::io::Read as _;
                        // Settings first: either the caller's, or sniffed from the
                        // head of the file.
                        let cfg = match req.cfg {
                            Some(c) => c,
                            None => {
                                let mut head = vec![0u8; SNIFF_BYTES];
                                let mut f =
                                    std::fs::File::open(&req.path).map_err(|e| e.to_string())?;
                                let n = f.read(&mut head).map_err(|e| e.to_string())?;
                                head.truncate(n);
                                schemaic_core::import::ReadConfig {
                                    dialect: schemaic_core::import::sniff(
                                        &String::from_utf8_lossy(&head),
                                    ),
                                    ..Default::default()
                                }
                            }
                        };
                        let f = std::fs::File::open(&req.path).map_err(|e| e.to_string())?;
                        let sample = schemaic_core::import::read_sample(
                            std::io::BufReader::new(f),
                            req.format,
                            &cfg,
                            SAMPLE_ROWS,
                        )
                        .map_err(|e| e.to_string())?;
                        // Best-effort: a size we can't read just means no
                        // large-file warning, never a failed probe.
                        let file_bytes = std::fs::metadata(&req.path).map(|m| m.len()).unwrap_or(0);
                        Ok(schemaic_ui::ImportProbeResult {
                            cfg,
                            sample,
                            file_bytes,
                        })
                    };
                    report(probe());
                });
            },
        )
    };

    // Check the whole file, then — only if it's clean — load it in one
    // transaction.
    //
    // The check is a separate pass over the file, and it's the point of the
    // design: the transaction would roll back on the first bad row anyway, one
    // error per attempt. Reading it through first turns that into a single list
    // of everything wrong, with nothing written either way.
    // The running import's cancellation token, so the modal's Cancel can reach it.
    //
    // One at a time, enforced by `widgets::accept_launch` in the caller — *not*
    // by the disabled Import button, which is what this comment used to claim.
    // The button is disabled on a later update pass, so a single key dispatch
    // that fired twice reached here twice, and the second launch overwrote this
    // slot: the first load became uncancellable, and both committed.
    //
    // Cleared when a run reports, so `import_cancel` can no longer cancel a token
    // belonging to a load that has already finished.
    let import_token: Rc<RefCell<Option<CancellationToken>>> = Rc::new(RefCell::new(None));

    let import_run: schemaic_ui::ImportFn = {
        let handle = handle.clone();
        let db_for = db_for.clone();
        let import_token = import_token.clone();
        Rc::new(
            move |req: schemaic_ui::ImportRunRequest, done: schemaic_ui::ImportDoneFn| {
                const MAX_ISSUES: usize = 200;
                let db = match db_for(req.target.conn_id) {
                    Ok(db) => db,
                    Err(e) => {
                        (done)(schemaic_ui::ImportOutcome::Failed(e));
                        return;
                    }
                };
                let dialect = db.engine().dialect();
                let token = CancellationToken::new();
                *import_token.borrow_mut() = Some(token.clone());
                let report = {
                    let import_token = import_token.clone();
                    create_ext_action(cx, move |o: schemaic_ui::ImportOutcome| {
                        // This run is over, so the slot must not still name its
                        // token: a later Cancel would otherwise "cancel" a load
                        // that already committed and report nothing at all.
                        *import_token.borrow_mut() = None;
                        (done)(o)
                    })
                };
                handle.spawn(async move {
                    let open = |path: &std::path::PathBuf| {
                        std::fs::File::open(path)
                            .map(std::io::BufReader::new)
                            .map_err(|e| e.to_string())
                    };
                    // Pass 1 — validate. Blocking file work, so off the runtime.
                    let checked = {
                        let (path, format, cfg, table, mapping) = (
                            req.path.clone(),
                            req.format,
                            req.cfg.clone(),
                            req.target.table.clone(),
                            req.mapping.clone(),
                        );
                        tokio::task::spawn_blocking(move || {
                            let f = open(&path)?;
                            schemaic_core::import::validate(
                                f, format, &cfg, &table, &mapping, dialect, MAX_ISSUES,
                            )
                            .map_err(|e| e.to_string())
                        })
                        .await
                    };
                    let validation = match checked {
                        Ok(Ok(v)) => v,
                        Ok(Err(e)) => return report(schemaic_ui::ImportOutcome::Failed(e)),
                        Err(e) => {
                            return report(schemaic_ui::ImportOutcome::Failed(e.to_string()));
                        }
                    };
                    if !validation.issues.is_empty() {
                        return report(schemaic_ui::ImportOutcome::Invalid(validation));
                    }
                    // Cancelling during the check can't interrupt the read itself
                    // (it's one blocking pass over the file), but it must still
                    // stop the load that would follow — which is the part that
                    // writes and the part that takes minutes.
                    if token.is_cancelled() {
                        return report(schemaic_ui::ImportOutcome::Cancelled);
                    }

                    // Pass 2 — load. The row iterator parses between statements;
                    // 500 records is microseconds against a round-trip.
                    let f = match open(&req.path) {
                        Ok(f) => f,
                        Err(e) => return report(schemaic_ui::ImportOutcome::Failed(e)),
                    };
                    let mut rows = match schemaic_core::import::row_iter(
                        f,
                        req.format,
                        &req.cfg,
                        &req.target.table,
                        &req.mapping,
                        dialect,
                    ) {
                        Ok(it) => it,
                        Err(e) => return report(schemaic_ui::ImportOutcome::Failed(e.to_string())),
                    };
                    let columns: Vec<String> =
                        schemaic_core::import::insert_columns(&req.mapping, &req.target.table)
                            .iter()
                            .map(|&i| req.target.table.columns[i].name.clone())
                            .collect();
                    let outcome = db
                        .import_rows(
                            schemaic_db::ImportTarget {
                                database: &req.target.database,
                                schema: req.target.schema.as_deref(),
                                table: &req.target.table.name,
                                columns: &columns,
                            },
                            &mut rows,
                            token,
                        )
                        .await;
                    report(match outcome {
                        Ok(n) => schemaic_ui::ImportOutcome::Done(n),
                        Err(DbError::Cancelled) => schemaic_ui::ImportOutcome::Cancelled,
                        Err(e) => schemaic_ui::ImportOutcome::Failed(e.to_string()),
                    });
                });
            },
        )
    };

    // Render + write an export on a worker thread. The grid owns the save dialog
    // and snapshots the rows (cheap `Arc` clones) before it opens; this does the
    // part that scales with the result — a 200k-row export took long enough to
    // freeze the window when it ran inline on the UI thread.
    //
    // `spawn_blocking`, not `spawn`: this is synchronous file IO, and running it
    // on a runtime worker would stall every other task sharing that thread.
    let export_file: schemaic_ui::ExportFn = {
        let handle = handle.clone();
        Rc::new(
            move |req: schemaic_ui::ExportRequest, done: schemaic_ui::ExportDoneFn| {
                let report = create_ext_action(cx, move |res: Result<(), String>| (done)(res));
                handle.spawn_blocking(move || {
                    let write = || -> std::io::Result<()> {
                        use std::io::Write as _;
                        let file = std::fs::File::create(&req.path)?;
                        let mut w = std::io::BufWriter::new(file);
                        req.format.render_to(
                            &mut w,
                            req.rs.as_ref(),
                            req.order.as_slice(),
                            req.source.as_ref().map(|s| {
                                (s.database.as_str(), s.schema.as_deref(), s.table.as_str())
                            }),
                            req.dialect,
                        )?;
                        // Explicit: `BufWriter` swallows a flush failure on drop,
                        // which is exactly the case where the last block never
                        // reached the disk — silently truncating the file.
                        w.flush()
                    };
                    report(write().map_err(|e| format!("Export failed: {e}")));
                });
            },
        )
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
                handle.spawn(async move {
                    let token = CancellationToken::new();
                    // Both branches write the rows and then, on success, re-read
                    // them. The session branch keeps both on the pinned
                    // connection — a fresh one couldn't see rows the transaction
                    // hasn't committed.
                    let written = match &session {
                        Some(s) => {
                            // See `run_query_core`: the session owns the decision.
                            // A failed BEGIN aborts — writing outside the
                            // transaction is what Manual mode exists to prevent.
                            match s.ensure_tx().await {
                                Err(e) => Err(e),
                                Ok(()) => {
                                    let out = s.commit_writes(&write, token.clone()).await;
                                    fold(out.stmt);
                                    out.result
                                }
                            }
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
        Rc::new(
            move |tab_id: usize, proceed: Rc<dyn Fn()>, on_cancel: Option<Rc<dyn Fn()>>| {
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
                            TxChoice::Cancel => {
                                if let Some(cancel) = on_cancel.clone() {
                                    cancel();
                                }
                            }
                        }
                    }),
                }));
            },
        )
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
            let closer = handle.clone();
            let opened = create_ext_action(cx, move |res: Result<Arc<Session>, String>| {
                // Re-resolve the tab instead of reading the captured copy. An
                // open is a full connect — seconds through a tunnel — and a tab
                // closed meanwhile has had its scope disposed one tick later, so
                // `tab.tx_mode.get_untracked()` would be a read of a freed
                // signal, which panics. Absent from `tabs` is the answer, and it
                // is also the answer to "who owns this session now".
                let mode = tabs.with_untracked(|v| {
                    v.iter()
                        .find(|t| t.id == tab_id)
                        .map(|t| t.tx_mode.get_untracked())
                });
                match res {
                    Ok(s) => {
                        // A flip back to Auto (or a tab close) may have raced us;
                        // don't resurrect a session nobody wants — and don't file
                        // it under a dead tab id either, where nothing would ever
                        // remove it and the connection would be held for the life
                        // of the process.
                        if session_still_wanted(mode) {
                            sessions.borrow_mut().insert(tab_id, s);
                        } else {
                            closer.spawn(async move { s.close().await });
                        }
                    }
                    // Nothing to flip back, and a modal about a tab the user has
                    // already closed is noise.
                    Err(_) if mode.is_none() => {}
                    Err(e) => {
                        tab.tx_mode.set(TxMode::Auto);
                        error_modal_text
                            .set(Some(format!("couldn't open a transaction connection: {e}")));
                        error_modal_open.set(true);
                    }
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
                        None,
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
        let tokens = tokens.clone();
        Rc::new(move |name: String| {
            // The DB selector lists the active connection's databases, so picking
            // one binds the active tab to the active connection + that database.
            let exists = db_nodes.with_untracked(|ns| ns.iter().any(|n| n.database == name));
            if !exists {
                return;
            }
            let id = active.get_untracked();
            let open_session = open_session.clone();
            let tokens = tokens.clone();
            // A pinned session belongs to one database — PostgreSQL can't switch
            // and MySQL's transaction context wouldn't survive the move — so an
            // open transaction has to be settled before the tab moves.
            (guard_tx)(
                id,
                Rc::new(move || {
                    // Rebinding the tab is the same kind of event as closing it:
                    // a run started against the old `(conn_id, database)` is
                    // still outstanding, and its generation check can't see the
                    // difference because no *new* run was started. Left alone it
                    // lands in a tab that now says another database, so the rows
                    // are right and everything around them — footer, schema
                    // context, completion, key icons — describes somewhere else.
                    // Cancelled is the honest outcome; the user asked to move.
                    if let Some((_, tok)) = tokens.borrow_mut().remove(&id) {
                        tok.cancel();
                    }
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
                None,
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
    // The same list with the pinned flag, for the closing rules — a pinned tab is
    // visible and selectable but not closable.
    let closable_refs = move || {
        tabs.with_untracked(|v| {
            v.iter()
                .map(|t| (t.id, t.conn_id.get_untracked(), t.pinned.get_untracked()))
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
                let path = tab.path.get_untracked();
                // A file-backed tab is worth restoring even when the file is
                // empty: the binding to the path is the thing being lost.
                if query.trim().is_empty() && source.is_none() && name.is_none() && path.is_none() {
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
                    path,
                    disk_sql: tab.disk_sql.get_untracked(),
                    file_format: tab.file_format.get_untracked(),
                });
            };
            // Pinned tabs aren't closable, and this is the last thing every close
            // path (× click, middle-click, Ctrl+W, the Close-all/others chains)
            // passes through, so gating here covers them all. Unpin first to close.
            //
            // It is the **backstop**, not the only gate: refusing this late is too
            // late to stop the questions a close asks on the way here, one of which
            // settles a transaction. `guard_close` answers the same question first,
            // through `tabsel::can_close`.
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
                // Shed the `.sql` binding too, or the "blank slate" left behind
                // still points at a file — and the next Ctrl+S would overwrite
                // that file with the empty document. The path went into the
                // reopen ring with the text (`record` above).
                //
                // Taken as one value (`FileBinding::none`), because the failure
                // here is always a line left out: a kept path overwrites a file,
                // a kept format writes a BOM and CRLF the new document never had.
                let shed = schemaic_core::sqlfile::FileBinding::none();
                tab.path.set(shed.path);
                tab.disk_sql.set(shed.disk_sql);
                tab.file_format.set(shed.format);
                // Also reset the results pane so the reopened tab is fully fresh
                // (single-grid Idle state, no leftover Run-Everything tabs).
                tab.results.set(QueryState::Idle);
                tab.result_tabs.set(Vec::new());
                tab.active_result.set(0);
                // Drop any temporary font zoom so the respawned tab starts at the
                // user's configured size (the post-flash rebuild reads this).
                tab.font_zoom.set(None);
                // This tab survives only because the strip must keep one — but
                // what comes back is a blank slate, so give it a blank slate's
                // identity too: no custom name, and the lowest free number for
                // the connection. Its old number went with its contents (already
                // snapshotted into the reopen ring above). Without this, closing
                // "Query 3" as the last tab leaves a ghost still calling itself
                // Query 3 while the next new tab opens as Query 1 beside it —
                // most visible after Close all tabs, which always ends here.
                tab.name.set(None);
                let conn = tab.conn_id.get_untracked();
                let free = smallest_free_label(&tabs.with_untracked(|v| {
                    v.iter()
                        .filter(|t| t.id != id && t.conn_id.get_untracked() == conn)
                        .map(|t| t.label)
                        .collect::<Vec<_>>()
                }));
                if free != tab.label {
                    // `label` is a plain field and the strip keys its chips on
                    // `(id, label)`, so writing it through `tabs` is what makes
                    // the new number render.
                    tabs.update(|v| {
                        if let Some(t) = v.iter_mut().find(|t| t.id == id) {
                            t.label = free;
                        }
                    });
                }
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

    // Everything a close has to ask about, in one guard: is this closable at all,
    // then unsaved `.sql` edits, then an open transaction. Same signature as
    // `guard_tx`, so it drops straight into the close paths that already took one.
    //
    // **Closability is settled before anything is asked**
    // (`tabsel::can_close`), because one of the questions is not a question:
    // answering the transaction prompt *commits or rolls back*. The pinned test
    // used to live only at the far end, in `close_tab_now`, so Ctrl+W on a pinned
    // tab holding a transaction prompted, took the commit, and then declined to
    // close — a transaction settled for a close that could never have happened.
    // `close_tab_now` still refuses; that gate is the backstop for every close
    // path, and this one exists so nothing is *asked* about an impossible close.
    //
    // **Then the file question, because it has no side effect either.** If the
    // transaction ran first and the user then said No to discarding their file
    // edits, they'd again be left with a settled transaction and no close. A No
    // here has changed nothing.
    //
    // The file question is only ever raised on a file-backed tab: `Tab::modified`
    // is false for an ordinary one, whose text is in the session and in the reopen
    // ring anyway.
    let guard_close: GuardCloseFn = {
        let guard_tx = guard_tx.clone();
        Rc::new(
            move |id: usize, proceed: Rc<dyn Fn()>, on_cancel: Option<Rc<dyn Fn()>>| {
                // Unknown ids answer `false` too, which is the same "nothing to
                // close, so nothing to ask" — see `tabsel::can_close`.
                if !schemaic_core::tabsel::can_close(&closable_refs(), id) {
                    if let Some(cancel) = on_cancel {
                        (cancel)();
                    }
                    return;
                }
                let guard_tx = guard_tx.clone();
                let tx_then = {
                    let proceed = proceed.clone();
                    let on_cancel = on_cancel.clone();
                    Rc::new(move || (guard_tx)(id, proceed.clone(), on_cancel.clone()))
                };
                let Some(tab) = tabs.with_untracked(|v| v.iter().find(|t| t.id == id).copied())
                else {
                    return; // already gone
                };
                if !tab.modified() {
                    (tx_then)();
                    return;
                }
                let name = tab
                    .path
                    .get_untracked()
                    .map(|p| schemaic_core::sqlfile::tab_title(&p))
                    .unwrap_or_else(|| tab.title());
                confirm.set(Some(Confirm {
                    title: format!("Close “{name}”"),
                    message: format!(
                        "“{name}” has unsaved changes. Closing discards them; the file on \
                         disk is left as it is. Close anyway?"
                    ),
                    resolve: Rc::new(move |yes| {
                        if yes {
                            (tx_then)();
                        } else if let Some(cancel) = on_cancel.clone() {
                            (cancel)();
                        }
                    }),
                }));
            },
        )
    };

    // Closing a tab asks about unsaved file changes and about an open transaction
    // — the pinned connection dies with the tab, so an unanswered transaction
    // would just vanish. Every close path (× click, middle-click, Ctrl+W, and the
    // Close-all/Close-others sequences) goes through `guard_close`.
    let close_tab: Rc<dyn Fn(usize)> = {
        let close_tab_now = close_tab_now.clone();
        let guard_close = guard_close.clone();
        Rc::new(move |id: usize| {
            let close_tab_now = close_tab_now.clone();
            (guard_close)(id, Rc::new(move || (close_tab_now)(id)), None);
        })
    };

    // Close `ids` one at a time, each tab waiting on the one before it. Recursion
    // rather than a loop because the wait is a *continuation*: `guard` may return
    // having only opened a prompt, and the close happens whenever the user answers
    // it. `guard` is `guard_close`, so each tab's unsaved-file question and its
    // transaction prompt take their turn in the same chain — the blanket "close
    // all tabs?" confirm is about closing tabs, not about discarding file edits.
    fn close_tabs_seq(ids: Vec<usize>, guard: GuardCloseFn, close_now: Rc<dyn Fn(usize)>) {
        let Some((&id, rest)) = ids.split_first() else {
            return;
        };
        let rest = rest.to_vec();
        let g = guard.clone();
        let c = close_now.clone();
        (guard)(
            id,
            Rc::new(move || {
                (c)(id);
                close_tabs_seq(rest.clone(), g.clone(), c.clone());
            }),
            None,
        );
    }

    // Close every tab of the active connection — the ones the strip actually
    // shows. Pinned tabs stay (they're unclosable through every other path too),
    // and the connection's last remaining tab clears in place instead of
    // vanishing, per `close_tab_now`'s "keep ≥1 tab" rule.
    //
    // Sequential rather than a loop over `close_tab`: `tx_prompt` holds one
    // question at a time, so asking about several open transactions at once would
    // clobber every prompt but the last and strand exactly the transactions the
    // prompt exists to protect. Chaining also gives Cancel the sensible meaning —
    // it stops the whole run, rather than skipping one tab and closing the rest.
    //
    // Asks first: this is the one action that can clear the whole strip in a
    // click, and undoing it means pressing Ctrl+Shift+T once per tab.
    let close_all_tabs: Rc<dyn Fn()> = {
        let close_tab_now = close_tab_now.clone();
        let guard_close = guard_close.clone();
        Rc::new(move || {
            let conn = active_conn.get_untracked();
            let ids = schemaic_core::tabsel::all_to_close(&closable_refs(), conn);
            // Nothing closable (every tab pinned) — no action, so nothing to ask.
            if ids.is_empty() {
                return;
            }
            let guard_close = guard_close.clone();
            let close_tab_now = close_tab_now.clone();
            confirm.set(Some(Confirm {
                title: "Close all tabs".to_string(),
                message: "Are you sure you want to close all the tabs?".to_string(),
                resolve: Rc::new(move |yes| {
                    if yes {
                        close_tabs_seq(ids.clone(), guard_close.clone(), close_tab_now.clone());
                    }
                }),
            }));
        })
    };

    // Close every tab of the active connection except the one the menu was
    // opened on — `close_all_tabs`' set, less that tab — with the same rules:
    // pinned tabs stay, open transactions are asked about one at a time, and
    // Cancel stops the run.
    //
    // The kept tab is made active *before* the closes, and only once the user
    // has said yes. Before, because the right-click may have landed on a tab
    // that wasn't active and this is the one tab certain to survive, so nothing
    // downstream has to pick a survivor. The keep-≥1 rule in `close_tab_now`
    // therefore never fires here: the connection always still has this tab.
    let close_other_tabs: Rc<dyn Fn(usize)> = {
        let close_tab_now = close_tab_now.clone();
        let guard_close = guard_close.clone();
        Rc::new(move |keep: usize| {
            let conn = active_conn.get_untracked();
            // The same call the menu entry dims on (`can_close_other_tabs`), so
            // the row and the action can't disagree about whether there is
            // anything to do.
            let ids = schemaic_core::tabsel::others_to_close(&closable_refs(), conn, keep);
            // Nothing else closable (alone, or every other tab pinned) — no
            // action, so nothing to ask and nothing to activate.
            if ids.is_empty() {
                return;
            }
            let guard_close = guard_close.clone();
            let close_tab_now = close_tab_now.clone();
            confirm.set(Some(Confirm {
                title: "Close other tabs".to_string(),
                message: "Are you sure you want to close all the other tabs?".to_string(),
                resolve: Rc::new(move |yes| {
                    if yes {
                        active.set(keep);
                        close_tabs_seq(ids.clone(), guard_close.clone(), close_tab_now.clone());
                    }
                }),
            }));
        })
    };

    // Place a freshly-built tab: reuse the active tab *in place* if it's a blank
    // slate (empty editor, no results / no Run-Everything panels, no `.sql` file)
    // — the common "app opened on an empty Query 1" case — else open it as a new tab.
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
                    // A tab bound to a `.sql` file is not a blank slate even when
                    // the file is empty: reusing it would silently drop the
                    // binding, and the next Ctrl+S would go somewhere else.
                    && t.path.with_untracked(|p| p.is_none())
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
            // A table with no key of its own is opened with its implicit row key
            // projected, which is the only thing that makes it editable — see
            // `table_query`. `None` unless the engine has one to offer.
            let (_, pk_cols, implicit_key) = table_ddl_and_pk(db_nodes, &source, dialect);
            let sql = table_query(
                dialect,
                &source.database,
                source.schema.as_deref(),
                &source.table,
                BrowseKey::pick(&pk_cols, implicit_key.as_deref()),
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
                // Deliberately *not* running the tab's query, even though a restored
                // tab is `Idle` and so shows an empty grid. A table tab keeps its
                // `source` however the user edits its text, so "open the table" would
                // execute whatever that tab is now holding — `DELETE FROM orders;`
                // included. Executing SQL is the user's call; the empty grid is one
                // Ctrl+Enter away from filled.
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
                // Same rule as `open_table`: a restored tab is not run for the user
                // (its text is no longer necessarily the table's `SELECT`). The
                // highlight stays pending — the effect consumes it whenever the
                // results reach `Loaded`, whether that's now or after the user runs.
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

    // ── `.sql` files ────────────────────────────────────────────────────────
    //
    // Two halves, split the way the results export is: the *dialog* and the tab
    // bookkeeping run here on the UI thread, and the actual read/write goes to a
    // worker (`spawn_blocking` — synchronous file IO, and a large script would
    // otherwise freeze the window) with `create_ext_action` bringing the outcome
    // back. Every decision about bytes and names is `core::sqlfile`.

    /// Why a read didn't produce text.
    ///
    /// `TooBig` is separate because it is not an error to report but a *question
    /// to ask*: the file is readable and the user may well want it anyway. It
    /// carries the size so the question can name it.
    enum FileReadError {
        Message(String),
        TooBig(u64),
    }

    /// Report a file operation's outcome — invoked on the UI thread. A read's
    /// error is already a sentence: the "too large" question is asked and
    /// resolved inside `read_sql_file`, so nothing downstream has to know it
    /// exists.
    type FileReadDone = Rc<dyn Fn(Result<schemaic_core::sqlfile::SqlText, String>)>;
    type SizedReadDone = Rc<dyn Fn(Result<schemaic_core::sqlfile::SqlText, FileReadError>)>;
    type FileWriteDone = Rc<dyn Fn(Result<(), String>)>;

    // `allow_big` is the user's answer to the confirmation below, carried back in
    // on the second attempt — a file over the warn threshold is read only once
    // they have said so.
    let read_sql_file_sized: Rc<dyn Fn(std::path::PathBuf, bool, SizedReadDone)> = {
        let handle = handle.clone();
        Rc::new(
            move |path: std::path::PathBuf, allow_big: bool, done: SizedReadDone| {
                let report = create_ext_action(cx, move |res| (done)(res));
                handle.spawn_blocking(move || {
                    use schemaic_core::sqlfile::{OpenVerdict, open_verdict};
                    // **The size is asked before the bytes are.** The read itself
                    // is cheap; what is not is the editor's own analysis, which
                    // runs over the whole document on the UI thread 120 ms after
                    // every pause in typing — so a 16 MB script is an
                    // eleven-second freeze per burst, for as long as the tab is
                    // open. The import path already asks this question the same
                    // way (`fs::metadata().len()`).
                    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    match open_verdict(size) {
                        OpenVerdict::Open => {}
                        OpenVerdict::Confirm(n) if !allow_big => {
                            report(Err(FileReadError::TooBig(n)));
                            return;
                        }
                        OpenVerdict::Confirm(_) => {}
                        OpenVerdict::Refuse(n) => {
                            report(Err(FileReadError::Message(format!(
                                "{} is {} — too large to open in an editor tab. \
                                 Schemaic would spend most of its time re-analysing \
                                 it. Run it from a query tab, or use Import for a \
                                 data file.",
                                path.display(),
                                schemaic_core::stats::format_bytes(n)
                            ))));
                            return;
                        }
                    }
                    let res = std::fs::read(&path)
                        .map(|bytes| schemaic_core::sqlfile::decode(&bytes))
                        .map_err(|e| {
                            FileReadError::Message(format!("Couldn't read {}: {e}", path.display()))
                        });
                    report(res);
                });
            },
        )
    };
    // The same read, with the "this file is large" question asked and resolved
    // here rather than by each caller — Open and reload both want it worded the
    // same way, and neither wants to know the band exists otherwise.
    let read_sql_file: Rc<dyn Fn(std::path::PathBuf, FileReadDone)> = {
        let sized = read_sql_file_sized.clone();
        Rc::new(move |path: std::path::PathBuf, done: FileReadDone| {
            let retry = sized.clone();
            let again = path.clone();
            (sized)(
                path,
                false,
                Rc::new(move |res| match res {
                    Ok(f) => (done)(Ok(f)),
                    Err(FileReadError::Message(m)) => (done)(Err(m)),
                    Err(FileReadError::TooBig(n)) => {
                        let (retry, again, done) = (retry.clone(), again.clone(), done.clone());
                        confirm.set(Some(Confirm {
                            title: "Open a large file?".to_string(),
                            message: format!(
                                "“{}” is {}. Schemaic re-analyses the whole document \
                                 shortly after every pause in typing, so a file this \
                                 size makes the editor slow to respond for as long as \
                                 the tab is open. Open it anyway?",
                                schemaic_core::sqlfile::tab_title(&again),
                                schemaic_core::stats::format_bytes(n),
                            ),
                            resolve: Rc::new(move |yes| {
                                if !yes {
                                    return;
                                }
                                let done = done.clone();
                                (retry)(
                                    again.clone(),
                                    true,
                                    Rc::new(move |res| {
                                        (done)(res.map_err(|e| match e {
                                            FileReadError::Message(m) => m,
                                            // Unreachable: the retry allows it.
                                            FileReadError::TooBig(_) => {
                                                "The file is too large to open.".to_string()
                                            }
                                        }))
                                    }),
                                );
                            }),
                        }));
                    }
                }),
            )
        })
    };

    // `expect_disk` is what the file's bytes must still be for the write to go
    // ahead — `Some` only for a Save over a file this tab read, where somebody
    // else's edit would otherwise be discarded without a word. `None` means
    // "write it whatever is there", which is what a Save As the user has already
    // confirmed the overwrite for means.
    type FileWriteReq = (std::path::PathBuf, String, Option<Vec<u8>>);
    let write_sql_file: Rc<dyn Fn(FileWriteReq, FileWriteDone)> = {
        let handle = handle.clone();
        Rc::new(
            move |(path, contents, expect_disk): FileWriteReq, done: FileWriteDone| {
                let report = create_ext_action(cx, move |res| (done)(res));
                handle.spawn_blocking(move || {
                    // Read-then-write, on the worker, immediately before the
                    // rename. It is not a lock — nothing here can take one — but
                    // it closes the window that matters in practice: a file
                    // edited in another program since this tab last read it.
                    // Silently discarding that edit is the failure; a missing
                    // file is not one, since Save is how it comes back.
                    if let Some(expected) = expect_disk
                        && let Ok(now) = std::fs::read(&path)
                        && now != expected
                    {
                        report(Err(format!(
                            "{} has changed on disk since it was opened. \
                             Saving now would discard those changes — reload the \
                             file (or Save As to a different name) instead.",
                            path.display()
                        )));
                        return;
                    }
                    // Atomic: `fs::write` truncates first, and this file is the
                    // one thing Schemaic can't regenerate.
                    let res = schemaic_core::persist::write_file_atomic(&path, contents.as_bytes())
                        .map_err(|e| format!("Couldn't save {}: {e}", path.display()));
                    report(res);
                });
            },
        )
    };

    // Surface a file error where the app already puts the ones it can't attach to
    // a result: the shared error modal. A failed Open or Save has no grid and no
    // error bar of its own to land in, and silence is the one thing it must not be.
    let file_error: Rc<dyn Fn(String)> = Rc::new(move |msg: String| {
        error_modal_text.set(Some(msg));
        error_modal_open.set(true);
    });

    // Write a tab's current text to `path` and, on success, bind the tab to it and
    // record what's now on disk. The snapshot is taken *before* the write, so
    // typing during it correctly leaves the tab modified afterwards.
    //
    // **A save that cannot be undone asks first.** `sqlfile::decode` reads bytes
    // it can't make sense of as U+FFFD so a mis-encoded byte costs a character
    // rather than the whole file — but writing that text back replaces every one
    // of those bytes on disk permanently, including in lines the user never
    // touched, and a Latin-1 `mysqldump` is the ordinary shape of it. So a lossy
    // tab's save is confirmed, in the same modal every other irreversible action
    // in the app uses.
    let write_tab_to: Rc<dyn Fn(Tab, std::path::PathBuf)> = {
        let write_sql_file = write_sql_file.clone();
        let file_error = file_error.clone();
        Rc::new(move |tab: Tab, path: std::path::PathBuf| {
            let format = tab.file_format.get_untracked();
            let text = tab.query.get_untracked();
            let contents = schemaic_core::sqlfile::encode(&text, format);
            // What the file must still hold. Reconstructed from the text this tab
            // read rather than kept as a second copy of the bytes — `encode` is
            // `decode`'s inverse for exactly the files this can apply to. A lossy
            // read has no inverse and a restored-dirty tab never read one, so
            // both skip the check; the lossy case has its own, louder question.
            let expect_disk = (!format.lossy)
                .then(|| tab.disk_sql.get_untracked())
                .flatten()
                .map(|disk| schemaic_core::sqlfile::encode(&disk, format).into_bytes());
            let write: Rc<dyn Fn()> = {
                let write_sql_file = write_sql_file.clone();
                let file_error = file_error.clone();
                let landed = path.clone();
                Rc::new(move || {
                    let file_error = file_error.clone();
                    let landed2 = landed.clone();
                    let text = text.clone();
                    (write_sql_file)(
                        (landed.clone(), contents.clone(), expect_disk.clone()),
                        Rc::new(move |res| match res {
                            // The dialog and the write take a moment; a tab closed
                            // in the meantime has had its scope disposed, and
                            // reading a freed signal panics. Absent is the answer —
                            // the bytes are on disk either way, there is just no tab
                            // left to mark saved.
                            Ok(()) => {
                                if tab.path.try_get_untracked().is_none() {
                                    return;
                                }
                                tab.path.set(Some(landed2.clone()));
                                tab.disk_sql.set(Some(text.clone()));
                                // Saved as UTF-8, so what was unreadable is gone
                                // and the tab and the file now agree. Asking again
                                // would be asking about a file that no longer
                                // exists.
                                tab.file_format.update(
                                    |f: &mut schemaic_core::sqlfile::SqlFormat| f.lossy = false,
                                );
                            }
                            Err(e) => (file_error)(e),
                        }),
                    );
                })
            };
            if !format.lossy {
                (write)();
                return;
            }
            confirm.set(Some(Confirm {
                title: "Save as UTF-8?".to_string(),
                message: format!(
                    "Schemaic couldn't read every byte of “{}” as text and showed \
                     those bytes as “�”. Saving writes what you see, so each of \
                     them is replaced permanently — in the whole file, not just \
                     the lines you edited. Save anyway?",
                    schemaic_core::sqlfile::tab_title(&path)
                ),
                resolve: Rc::new(move |yes| {
                    if yes {
                        (write)();
                    }
                }),
            }));
        })
    };

    let tab_by_id =
        move |id: usize| tabs.with_untracked(|v| v.iter().find(|t| t.id == id).copied());

    // Ctrl+Shift+S — pick a path and write the tab there. The suggestion is the
    // file's own name when it has one, else the tab's title scrubbed into
    // something a file system will accept.
    let save_sql_file_as: Rc<dyn Fn(usize)> = {
        let write_tab_to = write_tab_to.clone();
        Rc::new(move |id: usize| {
            let Some(tab) = tab_by_id(id) else {
                return;
            };
            let default_name = match tab.path.get_untracked() {
                Some(p) => schemaic_core::sqlfile::tab_title(&p),
                None => schemaic_core::sqlfile::suggested_name(&tab.title()),
            };
            let opts = floem::file::FileDialogOptions::new()
                .title("Save SQL file")
                .default_name(default_name)
                .allowed_types(vec![floem::file::FileSpec {
                    name: schemaic_core::sqlfile::SQL_FILTER_NAME,
                    extensions: schemaic_core::sqlfile::SQL_EXTENSIONS,
                }]);
            let write_tab_to = write_tab_to.clone();
            // `save_as` takes an `Fn`, so everything it needs is cloned per call.
            floem::action::save_as(opts, move |file| {
                let Some(picked) = file.and_then(|f| f.path.first().cloned()) else {
                    return; // cancelled
                };
                // The native dialogs mostly append the filter's extension, but
                // not on every platform — see `sqlfile::ensure_extension`.
                let path = schemaic_core::sqlfile::ensure_extension(picked.clone());
                let write_tab_to = write_tab_to.clone();
                // **The dialog checked the name the user typed, not this one.**
                // Typing `orders` when `orders.sql` already exists gets no
                // "replace?" from the native dialog, because `orders` doesn't
                // exist — and then the extension is added and the existing file
                // is overwritten with no prompt at all. So the extra path this
                // step invented is confirmed here, where the dialog can't.
                if path != picked && path.exists() {
                    confirm.set(Some(Confirm {
                        title: "Replace file?".to_string(),
                        message: format!(
                            "“{}” already exists — “{}” was saved with the .sql \
                             extension added. Replace it?",
                            schemaic_core::sqlfile::tab_title(&path),
                            schemaic_core::sqlfile::tab_title(&picked),
                        ),
                        resolve: Rc::new(move |yes| {
                            if yes {
                                (write_tab_to)(tab, path.clone());
                            }
                        }),
                    }));
                    return;
                }
                (write_tab_to)(tab, path);
            });
        })
    };

    // Ctrl+S — write the tab back to its file, or fall through to Save As when it
    // hasn't got one. Always the answer to "save this".
    let save_sql_file: Rc<dyn Fn(usize)> = {
        let write_tab_to = write_tab_to.clone();
        let save_sql_file_as = save_sql_file_as.clone();
        Rc::new(move |id: usize| {
            let Some(tab) = tab_by_id(id) else {
                return;
            };
            match tab.path.get_untracked() {
                Some(path) => (write_tab_to)(tab, path),
                None => (save_sql_file_as)(id),
            }
        })
    };

    // **A file already open on *another* connection still has to be reachable**,
    // and the only correct way to reach it is the connection switch itself —
    // which reloads the schema, restores that connection's remembered tab and
    // resets the status. `switch_conn` is defined much further down (it needs
    // `load_schema`), so the reference is filled in there and read from here.
    // Activating a tab the strip doesn't show would leave the window contradicting
    // itself.
    let switch_conn_late: LateAction<u64> = Rc::new(RefCell::new(None));

    // Ctrl+O — pick a `.sql` file and open it in a tab, reusing a blank one the
    // way every other "open something in a tab" path does (`place_tab`).
    let open_sql_file: Rc<dyn Fn()> = {
        let next_id = next_id.clone();
        let default_tab_target = default_tab_target.clone();
        let place_tab = place_tab.clone();
        let switch_conn_late = switch_conn_late.clone();
        let read_sql_file = read_sql_file.clone();
        let file_error = file_error.clone();
        Rc::new(move || {
            let opts = floem::file::FileDialogOptions::new()
                .title("Open SQL file")
                .allowed_types(vec![floem::file::FileSpec {
                    name: schemaic_core::sqlfile::SQL_FILTER_NAME,
                    extensions: schemaic_core::sqlfile::SQL_EXTENSIONS,
                }]);
            let next_id = next_id.clone();
            let default_tab_target = default_tab_target.clone();
            let place_tab = place_tab.clone();
            let read_sql_file = read_sql_file.clone();
            let file_error = file_error.clone();
            let switch_conn_late = switch_conn_late.clone();
            floem::file_action::open_file(opts, move |file| {
                let Some(path) = file.and_then(|f| f.path.first().cloned()) else {
                    return; // cancelled
                };
                // **Already open anywhere?** Activate that tab instead of opening
                // a second view of one file: each tab keeps its own copy of the
                // bytes on disk, so saving the second discards the first — and the
                // first goes on showing itself clean, because its own copy still
                // matches what *it* wrote.
                //
                // Asked of every tab, not just this connection's. The strip being
                // per-connection is a fact about visibility and no answer at all to
                // the lost edit; scoping the search to the active connection made
                // opening the same file under a second connection produce a second
                // tab *always*.
                //
                // Canonicalised first, then compared by `sqlfile::same_file`: the
                // resolved form settles case, 8.3 short names, junctions and a
                // substituted drive when the file exists, and the path comparison
                // is what is left when it doesn't (a path that was typed into Save
                // As cannot be canonicalised at all).
                let resolve = |p: &std::path::Path| {
                    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
                };
                let wanted = resolve(&path);
                let already = tabs.with_untracked(|v| {
                    v.iter()
                        .find(|t| {
                            t.path.with_untracked(|p| {
                                p.as_deref().is_some_and(|q| {
                                    schemaic_core::sqlfile::same_file(&resolve(q), &wanted)
                                })
                            })
                        })
                        .map(|t| (t.id, t.conn_id.get_untracked()))
                });
                if let Some((id, on_conn)) = already {
                    if on_conn != active_conn.get_untracked() {
                        // Cloned out of the cell before the call: the switch runs
                        // arbitrary app code, and holding the borrow across it
                        // would panic if any of it came back here.
                        let switch = switch_conn_late.borrow().clone();
                        if let Some(switch) = switch {
                            switch(on_conn);
                        }
                    }
                    active.set(id);
                    return;
                }
                let next_id = next_id.clone();
                let default_tab_target = default_tab_target.clone();
                let place_tab = place_tab.clone();
                let file_error = file_error.clone();
                let opened = path.clone();
                (read_sql_file)(
                    path,
                    Rc::new(move |res| match res {
                        Ok(f) => {
                            let id = next_id.get();
                            next_id.set(id + 1);
                            let (conn_id, database) = default_tab_target();
                            let tab = Tab::new(cx, id, &f.text, conn_id, database);
                            tab.path.set(Some(opened.clone()));
                            tab.disk_sql.set(Some(f.text.clone()));
                            tab.file_format.set(f.format);
                            (place_tab)(tab);
                        }
                        Err(e) => (file_error)(e),
                    }),
                );
            });
        })
    };

    // Re-read the tab's file, discarding unsaved edits — confirmed first when
    // there are any, since nothing else in the app can put them back.
    let reload_sql_file: Rc<dyn Fn(usize)> = {
        let read_sql_file = read_sql_file.clone();
        let file_error = file_error.clone();
        Rc::new(move |id: usize| {
            let Some(tab) = tab_by_id(id) else {
                return;
            };
            let Some(path) = tab.path.get_untracked() else {
                return; // no file to reload from
            };
            let reload: Rc<dyn Fn()> = {
                let read_sql_file = read_sql_file.clone();
                let file_error = file_error.clone();
                let path = path.clone();
                Rc::new(move || {
                    let file_error = file_error.clone();
                    (read_sql_file)(
                        path.clone(),
                        Rc::new(move |res| match res {
                            Ok(f) => {
                                // Closed while the read was in flight (see
                                // `write_tab_to`) — nothing left to reload into.
                                if tab.query.try_get_untracked().is_none() {
                                    return;
                                }
                                tab.query.set(f.text.clone());
                                tab.disk_sql.set(Some(f.text.clone()));
                                tab.file_format.set(f.format);
                                // The mounted editor owns its document, so the
                                // new text only shows once the pane remounts.
                                tab.reload_gen.update(|g| *g = g.wrapping_add(1));
                            }
                            Err(e) => (file_error)(e),
                        }),
                    );
                })
            };
            if !tab.modified() {
                (reload)();
                return;
            }
            confirm.set(Some(schemaic_ui::Confirm {
                title: "Reload from disk".to_string(),
                message: format!(
                    "“{}” has unsaved changes. Reloading discards them. Reload anyway?",
                    schemaic_core::sqlfile::tab_title(&path)
                ),
                resolve: Rc::new(move |yes| {
                    if yes {
                        (reload)();
                    }
                }),
            }));
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
            tab.path.set(snap.path);
            tab.disk_sql.set(snap.disk_sql);
            tab.file_format.set(snap.file_format);
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

    // Does the ring hold anything for the active connection? Same per-connection
    // scoping `reopen_closed_tab` itself applies, so the tab menu can dim the
    // entry instead of offering a click that does nothing.
    let can_reopen_closed_tab: Rc<dyn Fn() -> bool> = {
        let recently_closed = recently_closed.clone();
        Rc::new(move || {
            let conn = active_conn.get_untracked();
            recently_closed.borrow().iter().any(|s| s.conn_id == conn)
        })
    };

    // Whether "Close other tabs" has anything to close, so the entry can be
    // dimmed rather than silently doing nothing — the same `tabsel` call the
    // action makes.
    let can_close_other_tabs: Rc<dyn Fn(usize) -> bool> = Rc::new(move |keep: usize| {
        !schemaic_core::tabsel::others_to_close(&closable_refs(), active_conn.get_untracked(), keep)
            .is_empty()
    });

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
            show_table_sizes: table_sizes.get_untracked(),
        });
    });

    // Persist the layout whenever a panel is toggled (the footer chips mutate
    // these signals directly, so we react rather than route through a callback).
    {
        let save_ui = save_ui.clone();
        create_effect(move |_| {
            schema_visible.get();
            right_panel.get();
            table_sizes.get();
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

    // The session as it stands, hoisted out of the debounced effect below so the
    // flush on window close writes exactly the same thing — a second builder
    // would be a second answer to "what was open", and the one that runs at quit
    // is the one nobody watches.
    let session_snapshot: Rc<dyn Fn() -> schemaic_core::persist::SavedTabsFile> =
        Rc::new(move || {
            tabs.with_untracked(|v| {
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
                                source: src.as_ref().map(|s| (s.database.clone(), s.table.clone())),
                                source_schema: src.and_then(|s| s.schema),
                                name: t.name.get_untracked(),
                                pinned: t.pinned.get_untracked(),
                                path: t.path.get_untracked(),
                                file_crlf: t.file_format.get_untracked().crlf,
                                file_bom: t.file_format.get_untracked().bom,
                                // The warning has to survive a relaunch: the
                                // restored tab holds the *decoded* text, so
                                // nothing in it would show that a save destroys
                                // the original bytes.
                                file_lossy: t.file_format.get_untracked().lossy,
                                // One bit instead of a second copy of the file's
                                // text — see `SavedTab::file_dirty`. It is the
                                // input `sqlfile::restored_binding` reads on the
                                // way back, and the whole reason the restore can
                                // tell "this text is what's on disk" from "this
                                // text is unsaved work".
                                file_dirty: t.modified(),
                            }
                        })
                        .collect(),
                }
            })
        });

    // **Write the session now, because the window is going.** Quitting is the one
    // way of losing a tab that never reaches `guard_close`, and on floem 0.2 it
    // cannot be vetoed (`app_handle.rs` calls `close_window` on `CloseRequested`
    // unconditionally) — so the answer is not a prompt but a flush. Without it a
    // quit inside the 600 ms debounce left `tabs.json` holding the *previous*
    // save, whose `file_dirty` was `false` because the tab was clean then: the tab
    // came back with the pre-edit text, no italic and no dot, reporting itself as
    // matching disk. Confidently wrong is worse than stale.
    //
    // With the setting off, only the tabs whose text is nowhere else are written
    // (`unsaved_files_only`), and nothing at all when there are none — a quit must
    // not silently discard unsaved file edits, and it must not store a session the
    // user asked not to keep either.
    let flush_session: Rc<dyn Fn()> = {
        let session_snapshot = session_snapshot.clone();
        Rc::new(move || {
            let file = session_snapshot();
            if restore_tabs.get_untracked() {
                persist::save_json("tabs.json", &file);
                return;
            }
            let unsaved = file.unsaved_files_only();
            if !unsaved.tabs.is_empty() {
                persist::save_json("tabs.json", &unsaved);
            }
        })
    };

    // Persist the open tabs (query text + connection + source) so the next launch
    // can restore the session, when the setting is on. Query edits fire on every
    // keystroke, so the write is debounced with a short trailing delay: each change
    // bumps a generation and schedules a save; a later change (or toggling the
    // setting off) supersedes the pending one, so only the last edit of a burst
    // touches disk. `tabs.json` holds ids/text only — no credentials.
    {
        let session_snapshot = session_snapshot.clone();
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
                    t.path.get();
                    t.disk_sql.get();
                }
            });
            active.get();
            let g = tabs_save_gen.get() + 1;
            tabs_save_gen.set(g);
            if !on {
                return; // bumping `g` above also cancels any pending save
            }
            let gen_at = tabs_save_gen.clone();
            let session_snapshot = session_snapshot.clone();
            exec_after(Duration::from_millis(600), move |_| {
                if gen_at.get() != g {
                    return; // superseded by a newer change
                }
                persist::save_json("tabs.json", &session_snapshot());
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
            let prefix = schemaic_ui::table_key_prefix(&db);
            expanded.update(|set| set.retain(|k| !k.starts_with(&prefix)));
            save_ui();
        })
    };

    // ── Connection health ────────────────────────────────────────────────────
    // One health check of the *active* connection: ping it (through the SSH
    // tunnel if one is established) and set `conn_status`. Runs off the UI
    // thread; the result is marshalled back via `create_ext_action`.
    // Health-check the active connection, optionally running `on_ok` if it
    // answers. The continuation is what makes the "connection is down" block
    // recoverable: a blocked action re-checks and proceeds if the server is
    // back, so a stale `Disconnected` can't strand the user.
    let check_conn_then: Rc<dyn Fn(Option<CheckDoneFn>)> = {
        let handle = handle.clone();
        let tunnels = tunnels.clone();
        Rc::new(move |done: Option<CheckDoneFn>| {
            let id = active_conn.get_untracked();
            let Some(conn) =
                connections.with_untracked(|cs| cs.iter().find(|c| c.id == id).cloned())
            else {
                conn_status.set(ConnStatus::Unknown);
                return;
            };
            // Effective endpoint — through the tunnel for SSH connections. If the
            // tunnel isn't up yet, stay Unknown; a later tick will catch it.
            let tunnel = if conn.uses_tunnel() {
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
                // Every check counts toward the backoff, not just the polled
                // ones — a user hammering Retry against a dead host shouldn't
                // reset the timer's patience either.
                health_failures.set(health::record(health_failures.get_untracked(), ok));
                if let Some(f) = &done {
                    f(ok);
                }
            });
            handle.spawn(async move {
                let ok = db.ping(std::time::Duration::from_secs(5)).await.is_ok();
                send(ok);
            });
        })
    };
    let check_conn: Rc<dyn Fn()> = {
        let check_conn_then = check_conn_then.clone();
        Rc::new(move || (check_conn_then)(None))
    };

    // Gate for anything that needs a working connection: run it now when the
    // connection isn't known-dead, otherwise re-check and run it only if the
    // server answers.
    //
    // The block has to be recoverable even so. The health poll keeps the flag
    // reasonably fresh, but it deliberately backs off a dead host and pauses
    // while the window is unfocused, so `Disconnected` can still be a minute or
    // two stale — gating on the cached flag alone would lock a user out of a
    // server that came back. A blocked attempt therefore pings first; if it
    // still fails, the reason is surfaced rather than the action silently doing
    // nothing.
    let with_conn: ConnGate = {
        let check_conn_then = check_conn_then.clone();
        Rc::new(move |action: Rc<dyn Fn()>| {
            if !conn_status.get_untracked().is_down() {
                action();
                return;
            }
            let name = connections
                .with_untracked(|cs| {
                    cs.iter()
                        .find(|c| c.id == active_conn.get_untracked())
                        .map(|c| c.name.clone())
                })
                .unwrap_or_else(|| "this connection".to_string());
            (check_conn_then)(Some(Rc::new(move |ok: bool| {
                if ok {
                    action();
                } else {
                    // Still unreachable — say so, rather than letting the action
                    // silently do nothing.
                    error_modal_text.set(Some(format!(
                        "Not connected to {name}. The server didn't answer — check \
                         that it's running and that this connection's settings are \
                         right."
                    )));
                    error_modal_open.set(true);
                }
            })));
        })
    };

    // ── The write guard ─────────────────────────────────────────────────────
    //
    // The read-only block, the missing-`WHERE` net and `confirm_writes` used to
    // live as two closures inside the editor pane's *view body*, which meant
    // they protected exactly one caller. The command palette's `>run` and the AI
    // chat's Insert & Run both reached the raw run action and executed writes
    // past all three — including the read-only block, which by design has no
    // "Run anyway". So the guard lives here now, wrapping the run actions
    // themselves: `tab_actions.run`/`run_all` *are* the guarded pair, the raw
    // ones never leave this crate, and a new caller can't opt out by omission.
    //
    // The decision itself is `schemaic_core::sql::run_verdict` — pure and
    // tested. This closure only supplies the policy and parks what was held
    // back.
    let run_guard: RwSignal<Option<RunGuard>> = RwSignal::new(None);
    let guard_policy = move || {
        let id = active.get_untracked();
        let cid = tabs.with_untracked(|v| {
            v.iter()
                .find(|t| t.id == id)
                .map(|t| t.conn_id.get_untracked())
        });
        // No database bound to this tab. On PostgreSQL the connection still lands
        // *somewhere* — the hidden maintenance database — so the guard has to say
        // so; `needs_database` decides which statements that actually stops.
        let no_database = tabs.with_untracked(|v| {
            v.iter()
                .find(|t| t.id == id)
                .is_none_or(|t| t.database.get_untracked().is_none())
        });
        let conn = cid.and_then(|cid| {
            connections.with_untracked(|cs| cs.iter().find(|c| c.id == cid).cloned())
        });
        GuardPolicy {
            read_only: conn.as_ref().is_some_and(|c| c.read_only),
            confirm_writes: confirm_writes.get_untracked(),
            dialect: conn
                .as_ref()
                .map(|c| SqlDialect::from_db_type(&c.db_type))
                .unwrap_or_default(),
            no_database,
        }
    };
    // The connection-gated but *unguarded* pair. Only the two wrappers below and
    // "Run anyway" reach them; nothing outside this crate can.
    let gated_run = gate1(&with_conn, &run);
    let gated_run_all = gate1(&with_conn, &run_all);

    let guarded_run: Rc<dyn Fn(String)> = {
        let gated_run = gated_run.clone();
        Rc::new(
            move |sql: String| match run_verdict(std::slice::from_ref(&sql), guard_policy()) {
                RunVerdict::Allow => (gated_run)(sql),
                RunVerdict::Block(message) => run_guard.set(Some(RunGuard {
                    message,
                    pending: None,
                })),
                RunVerdict::Confirm(message) => run_guard.set(Some(RunGuard {
                    message,
                    pending: Some(PendingRun::Single(sql)),
                })),
            },
        )
    };
    let guarded_run_all: Rc<dyn Fn(Vec<String>)> = {
        let gated_run_all = gated_run_all.clone();
        Rc::new(
            move |stmts: Vec<String>| match run_verdict(&stmts, guard_policy()) {
                RunVerdict::Allow => (gated_run_all)(stmts),
                RunVerdict::Block(message) => run_guard.set(Some(RunGuard {
                    message,
                    pending: None,
                })),
                RunVerdict::Confirm(message) => run_guard.set(Some(RunGuard {
                    message,
                    pending: Some(PendingRun::Batch(stmts)),
                })),
            },
        )
    };
    // A held-back run belongs to the tab it was raised in. The guard bar used to
    // be per-pane and vanished when the pane was rebuilt on a tab switch; now
    // that the guard is one signal, dropping it here keeps that behaviour — and
    // stops "Run anyway" replaying a statement into a different tab, which may
    // be a different connection and database. Guarded against a redundant `set`,
    // which would rebuild the bar's container on every switch.
    create_effect(move |_| {
        active.get();
        if run_guard.get_untracked().is_some() {
            run_guard.set(None);
        }
    });
    // "Run anyway": replay what the guard parked. A hard block parked nothing.
    let run_anyway: Rc<dyn Fn()> = {
        let gated_run = gated_run.clone();
        let gated_run_all = gated_run_all.clone();
        Rc::new(move || {
            let Some(g) = run_guard.get_untracked() else {
                return;
            };
            run_guard.set(None);
            match g.pending {
                Some(PendingRun::Single(sql)) => (gated_run)(sql),
                Some(PendingRun::Batch(stmts)) => (gated_run_all)(stmts),
                None => {}
            }
        })
    };

    // ── Schema loading ──────────────────────────────────────────────────────
    // For an SSH connection, open (or reuse) a tunnel first, then list the
    // databases through it; the resolved tunnel port is cached and every
    // downstream `Db` (schema, table-open, editor) is built pointing through it.
    //
    // Every call stamps itself `(conn id, generation)`; the completion checks the
    // stamp against the live one before touching anything shared. See
    // `load_landing`.
    let schema_gen: Rc<Cell<u64>> = Rc::new(Cell::new(0));

    // Start one database's introspection, keeping whatever that database already
    // shows while it runs (`SchemaState::begin_refresh`).
    //
    // The initial load, the connection-wide Refresh and the per-database Refresh
    // all come through here on purpose: they differ only in *which* `Db` and how
    // the node was obtained, and when they each decided for themselves what the
    // tree shows meanwhile, two of the three blanked it.
    // The newest fetch asked for per node, so a slower older one can't land on
    // top of it. Keyed on the node id, which survives the connection-wide
    // refresh's node reuse — the case the two fetches actually race in. A switch
    // disposes the scope and `try_update` below still covers that.
    let fetch_seq: Rc<RefCell<HashMap<usize, u64>>> = Rc::new(RefCell::new(HashMap::new()));
    let next_fetch_seq: Rc<Cell<u64>> = Rc::new(Cell::new(0));

    let start_fetch: FetchSchemaFn = {
        let handle = handle.clone();
        let fetch_seq = fetch_seq.clone();
        let next_fetch_seq = next_fetch_seq.clone();
        Rc::new(move |node: &ConnNode, db: Db| {
            let sig = node.schema;
            let database = node.database.clone();
            if let Some(st) = sig.get_untracked().begin_refresh() {
                sig.set(st);
            }
            // What the schema editors ask before seeding a draft: a `Loaded`
            // database is not necessarily a *current* one while this is out.
            let refreshing = node.refreshing;
            refreshing.set(true);
            // Sizes go back to unasked, so the tree's size column refetches
            // alongside the schema. Refresh is the one gesture that means "these
            // figures are out of date", and it is also the only thing that ever
            // retries a database whose statistics fetch failed.
            //
            // The bump is what makes that true. The reset alone is a write the
            // size-column effect does not watch (see `stats_gen`), so on its own
            // it only *clears* the column.
            node.stats.set(schemaic_ui::DbStatsState::Idle);
            stats_gen.update(|g| *g = g.wrapping_add(1));
            let seq = next_fetch_seq.get() + 1;
            next_fetch_seq.set(seq);
            fetch_seq.borrow_mut().insert(node.id, seq);
            let (id, landed_seq) = (node.id, fetch_seq.clone());
            // `try_update`, not `set`: switching connections disposes the node
            // scope this signal lives in, and a fetch already in flight then
            // lands on a freed one. The stamp is the *other* half — see
            // `fetch_landing`, whose absence let a pre-`ALTER` snapshot overwrite
            // a post-`ALTER` one with nothing to detect it.
            let send_schema = create_ext_action(cx, move |st: SchemaState| {
                let current = landed_seq.borrow().get(&id).copied().unwrap_or(seq);
                if !fetch_landing(seq, current) {
                    // A newer fetch of the same node is still out, so the model
                    // stays flagged stale until *it* lands.
                    return;
                }
                let _ = sig.try_update(|v| *v = st);
                let _ = refreshing.try_update(|v| *v = false);
            });
            handle.spawn(async move {
                let st = match db.fetch_schema(&database).await {
                    Ok(s) => SchemaState::Loaded(Arc::new(s)),
                    Err(e) => SchemaState::Failed(e.to_string()),
                };
                send_schema(st);
            });
        })
    };

    let load_schema: Rc<dyn Fn(Connection)> = {
        let handle = handle.clone();
        let tunnels = tunnels.clone();
        let nodes_scope = nodes_scope.clone();
        let nodes_conn = nodes_conn.clone();
        let schema_gen = schema_gen.clone();
        let start_fetch = start_fetch.clone();
        Rc::new(move |conn: Connection| {
            // Reloading the connection already on screen (the SCHEMA header's
            // Refresh) keeps its databases visible while the list is re-fetched;
            // only a *switch* clears, where the rows would otherwise be another
            // server's for as long as the connect takes.
            let reload = nodes_conn
                .borrow()
                .as_ref()
                .is_some_and(|c| c.targets_same_server(&conn));
            if !reload {
                db_nodes.set(Vec::new());
            }
            let stamp = (conn.id, schema_gen.get() + 1);
            schema_gen.set(stamp.1);
            let gen_cb = schema_gen.clone();
            let nodes_scope_cb = nodes_scope.clone();
            let nodes_conn_cb = nodes_conn.clone();
            let start_fetch_cb = start_fetch.clone();
            let cached_port = tunnels.borrow().get(&conn.id).map(|h| h.port());
            let tunnels_cache = tunnels.clone();
            // `conn` (original) → the send callback; `conn_task` → the async task.
            let conn_send = conn.clone();
            // Result payload: the effective tunnel port (if SSH), a *newly opened*
            // tunnel handle to cache (None when reusing a cached one), and the db
            // names.
            let send = create_ext_action(cx, move |res: ConnectResult| {
                let landing = load_landing(stamp, (active_conn.get_untracked(), gen_cb.get()));
                match res {
                    Ok((tunnel_port, new_handle, names)) => {
                        if let Some(handle) = new_handle {
                            // Dropping any prior handle here tears its listener down.
                            tunnels_cache.borrow_mut().insert(conn_send.id, handle);
                        }
                        // Everything past this point writes state the tree, the
                        // database menu, the completion index and every open tab
                        // read — so a load the user has moved on from stops here,
                        // its tunnel kept.
                        if landing != LoadLanding::Install {
                            return;
                        }
                        // A reload of the connection already on screen reuses the
                        // node of every database that is still there — its
                        // `schema` signal comes through untouched, so the rows
                        // stay up while the re-introspection runs, and its id
                        // comes through with it, so the tree (keyed on node id)
                        // doesn't rebuild a surviving database at all. Only a
                        // database that has *appeared* gets a new node.
                        //
                        // Which means the scope has to survive too. On a
                        // connection switch nothing is reused, so that path
                        // still builds in a fresh child scope and disposes the
                        // old one (deferred, so the tree rebuilds off the new
                        // nodes before the old signals are freed) — schema
                        // signals must not accrete across switches (C14).
                        let existing = db_nodes.get_untracked();
                        let kept_scope = nodes_scope_cb.borrow().filter(|_| reload);
                        let node_cx = kept_scope.unwrap_or_else(|| cx.create_child());
                        let by_id: Vec<(usize, String)> = existing
                            .iter()
                            .map(|n| (n.id, n.database.clone()))
                            .collect();
                        let nodes: Vec<ConnNode> = plan_nodes(&by_id, &names, reload)
                            .into_iter()
                            .zip(names.iter())
                            .map(|(plan, name)| match plan {
                                NodePlan::Keep(id) => existing
                                    .iter()
                                    .find(|n| n.id == id)
                                    .cloned()
                                    .unwrap_or_else(|| ConnNode::new(node_cx, id, name, name)),
                                NodePlan::Create(id) => ConnNode::new(node_cx, id, name, name),
                            })
                            .collect();
                        db_nodes.set(nodes.clone());
                        if kept_scope.is_none()
                            && let Some(old) = nodes_scope_cb.borrow_mut().replace(node_cx)
                        {
                            exec_after(Duration::ZERO, move |_| old.dispose());
                        }
                        *nodes_conn_cb.borrow_mut() = Some(conn_send.clone());
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
                        for node in &nodes {
                            (start_fetch_cb)(node, db.clone());
                        }
                    }
                    Err(e) => {
                        tracing::error!("schema load failed: {e}");
                        // Same rule on this side: clearing the tree here would empty
                        // it for a connection that loaded perfectly well.
                        if landing == LoadLanding::Install {
                            db_nodes.set(Vec::new());
                            // **And forget which connection the tree was for.**
                            // Nothing references the node scope once the tree is
                            // empty, and leaving it in place made the *next* load
                            // of this connection take the reuse path against an
                            // empty node list: `kept_scope` was `Some`, so the
                            // deferred `dispose()` was skipped, and every node was
                            // rebuilt inside a scope that still owned the previous
                            // set's `RwSignal<SchemaState>` — each holding an
                            // `Arc<DbSchema>`, unreachable and never freed. One
                            // set per failed connect, indefinitely.
                            //
                            // A database *dropped* between two successful reloads
                            // still leaves its signals in the surviving scope; that
                            // one needs a scope per node and is not this fix.
                            *nodes_conn_cb.borrow_mut() = None;
                            if let Some(old) = nodes_scope_cb.borrow_mut().take() {
                                exec_after(Duration::ZERO, move |_| old.dispose());
                            }
                        }
                    }
                }
            });
            let conn_task = conn.clone();
            handle.spawn(async move {
                // Establish (or reuse) the SSH tunnel, then build the `Db`. A
                // freshly opened tunnel's handle is returned so the UI thread can
                // cache it (and thereby own its lifetime).
                let (tunnel_port, new_handle) = if conn_task.uses_tunnel() {
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
        let db_for = db_for.clone();
        Rc::new(move |database: String| {
            let node = db_nodes
                .with_untracked(|nodes| nodes.iter().find(|n| n.database == database).cloned());
            let Some(node) = node else { return };
            // The tree shows the active connection's databases, so refresh runs
            // against the active connection's `Db`.
            match db_for(active_conn.get_untracked()) {
                // Keeps the rows on screen while the fetch is out; see
                // `start_fetch`.
                Ok(db) => (start_fetch)(&node, db),
                Err(e) => node.schema.set(SchemaState::Failed(e)),
            }
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

    // ── Schema editing (DDL) ────────────────────────────────────────────────
    // Apply an approved plan, then re-introspect the database it changed.
    //
    // The re-introspection isn't optional bookkeeping: `db_nodes` is what the
    // schema tree, the grid's key icons, the completion index and `intel`'s
    // catalog all read, so leaving it stale after an `ALTER` would have the
    // editor flagging columns that now exist as unknown.
    //
    // Which is why the gate is `ddl_changed_schema`, not success: MySQL has no
    // transactional DDL, so a plan that fails halfway has genuinely half-applied
    // and the stale model is the *worse* half — it describes a column that was
    // just dropped.
    // One `SHOW CREATE VIEW`, for the view the user just opened for editing.
    // A fresh connection like every other read-only side channel, so it can't
    // queue behind a tab's open transaction.
    let view_algorithm: schemaic_ui::ViewAlgoFn = {
        let handle = handle.clone();
        let db_for = db_for.clone();
        Rc::new(
            move |req: schemaic_ui::ViewAlgoRequest, done: schemaic_ui::ViewAlgoDoneFn| {
                let Ok(db) = db_for(req.conn_id) else {
                    // Nothing to report: the editor keeps the algorithm it has.
                    return;
                };
                let report = create_ext_action(cx, move |algo: Option<String>| (done)(algo));
                handle.spawn(async move {
                    // A failure here is not worth interrupting an edit for — it
                    // leaves the emitter writing exactly what it writes today.
                    let algo = db
                        .view_algorithm(Some(&req.database), &req.view)
                        .await
                        .unwrap_or(None);
                    report(algo);
                });
            },
        )
    };

    let trigger_source: schemaic_ui::TriggerSrcFn = {
        let handle = handle.clone();
        let db_for = db_for.clone();
        Rc::new(
            move |req: schemaic_ui::TriggerSrcRequest, done: schemaic_ui::TriggerSrcDoneFn| {
                let Ok(db) = db_for(req.conn_id) else {
                    // Nothing to report: the editor keeps the body it has.
                    return;
                };
                let name = req.trigger.clone();
                let report = create_ext_action(cx, move |src| (done)(name.clone(), src));
                handle.spawn(async move {
                    // A failed read leaves the editor on `information_schema`'s
                    // body — which is the state every build before this shipped,
                    // and better than refusing to open the editor at all.
                    let src = db
                        .trigger_source(Some(&req.database), &req.trigger)
                        .await
                        .unwrap_or(None);
                    report(src);
                });
            },
        )
    };

    // ── Table properties ────────────────────────────────────────────────────
    // The statistics behind the properties modal. Fetched for the whole database
    // (one round trip either way) and then narrowed to the object asked about,
    // so the set is there for a future size column in the tree without a second
    // query shape.
    //
    // Every landing checks the target is still the one on screen before writing:
    // this is a fresh connection and a slow-ish catalogue read, so the user can
    // close the panel or open another table while it is in flight, and a late
    // reply must not overwrite the newer one.
    let table_stats: Rc<dyn Fn(schemaic_ui::PropertiesTarget)> = {
        let handle = handle.clone();
        let db_for = db_for.clone();
        Rc::new(move |target: schemaic_ui::PropertiesTarget| {
            let db = match db_for(target.conn_id) {
                Ok(db) => db,
                Err(e) => {
                    properties_state.set(schemaic_ui::PropertiesState::Failed(e));
                    return;
                }
            };
            // Asked before the round trip: an engine that publishes nothing has
            // a different thing to say than one whose fetch failed, and finding
            // that out by running a query would be the same query returning
            // empty either way.
            if !schemaic_core::stats::supports_table_stats(db.engine().dialect()) {
                properties_state.set(schemaic_ui::PropertiesState::Unsupported);
                return;
            }
            // **The tree and the toolbar already have a slot for this, so ask it
            // first.** One fetch covers a whole database — the query's cost is in
            // making the server materialize per-table statistics for all of them —
            // and this modal used to issue a fresh one, on a fresh connection, on
            // every open: ten tables inspected in a row was ten full catalogue
            // fetches for data that was in memory the whole time. Worse on a server
            // with `information_schema_stats_expiry = 0`, which re-reads from the
            // storage engine each time, and the one the panel prints a note for.
            //
            // Only for the **active** connection: `db_nodes` is its tree, and a
            // query tab's properties may name another server (which is why the
            // target carries `conn_id` at all). Anything else takes the fetch below.
            let slot = (target.conn_id == active_conn.get_untracked())
                .then(|| {
                    db_nodes.with_untracked(|nodes| {
                        nodes
                            .iter()
                            .find(|n| n.database == target.database)
                            .map(|n| n.stats)
                    })
                })
                .flatten();
            if let Some(slot) = slot
                && let schemaic_ui::DbStatsState::Loaded(set) = slot.get_untracked()
            {
                properties_state.set(schemaic_ui::PropertiesState::Loaded(Box::new(
                    set.get(target.schema.as_deref(), &target.table)
                        .cloned()
                        .unwrap_or_default(),
                )));
                return;
            }
            let want = target.clone();
            let report = create_ext_action(
                cx,
                move |res: Result<schemaic_core::stats::SchemaStats, String>| {
                    if properties.with_untracked(|t| t.as_ref() != Some(&want)) {
                        return;
                    }
                    properties_state.set(match &res {
                        Ok(set) => schemaic_ui::PropertiesState::Loaded(Box::new(
                            set.get(want.schema.as_deref(), &want.table)
                                .cloned()
                                // A table the catalogue didn't list — a view on
                                // MySQL, a partitioned parent — is "nothing to
                                // report", not a failure.
                                .unwrap_or_default(),
                        )),
                        Err(e) => schemaic_ui::PropertiesState::Failed(e.clone()),
                    });
                    // **And warm the shared slot with it**, so the size column and
                    // a capped result's total are spared the same round trip — the
                    // two paths used to be unable to see each other in either
                    // direction. Only into a slot that hasn't got figures already:
                    // `Loading` means a fetch of its own is in flight and will land,
                    // and overwriting a `Loaded` set would substitute one reading
                    // for another with nothing to say which is newer.
                    if let (Some(slot), Ok(set)) = (slot, res)
                        && matches!(
                            slot.get_untracked(),
                            schemaic_ui::DbStatsState::Idle
                                | schemaic_ui::DbStatsState::Unavailable
                        )
                    {
                        slot.set(schemaic_ui::DbStatsState::Loaded(set));
                    }
                },
            );
            handle.spawn(async move {
                let res = db
                    .fetch_table_stats(&target.database)
                    .await
                    .map_err(|e| e.to_string());
                report(res);
            });
        })
    };

    // The in-flight `COUNT(*)`, if any. It is a **full scan** and the only way to
    // stop one is to hold its token: closing the modal used to abandon the answer
    // and leave the scan running on the server for minutes, holding a connection,
    // while the reopened panel offered the button again — N opens, N concurrent
    // scans.
    let counting_token: Rc<RefCell<Option<CancellationToken>>> = Rc::new(RefCell::new(None));

    // Whatever the modal is pointing at changed — closed, or reopened on another
    // table — so a scan asked for by the *previous* target is no longer wanted.
    // This is the close path: the modal owns its own dismissal (Escape, the ✕, the
    // backdrop) and all three arrive here as one write.
    {
        let counting_token = counting_token.clone();
        create_effect(move |_| {
            properties.track();
            if let Some(tok) = counting_token.borrow_mut().take() {
                tok.cancel();
                properties_counting.set(false);
            }
        });
    }

    let count_cancel: Rc<dyn Fn()> = {
        let counting_token = counting_token.clone();
        Rc::new(move || {
            if let Some(tok) = counting_token.borrow_mut().take() {
                tok.cancel();
                properties_counting.set(false);
            }
        })
    };

    // The exact `COUNT(*)`. Its result is folded into the loaded statistics
    // rather than kept beside them, so everything that prints a row figure —
    // the headline, the Markdown copy — reads one place and cannot disagree.
    let count_rows: Rc<dyn Fn(schemaic_ui::PropertiesTarget)> = {
        let handle = handle.clone();
        let db_for = db_for.clone();
        let counting_token = counting_token.clone();
        Rc::new(move |target: schemaic_ui::PropertiesTarget| {
            let db = match db_for(target.conn_id) {
                Ok(db) => db,
                Err(e) => {
                    properties_count_err.set(Some(e));
                    return;
                }
            };
            properties_counting.set(true);
            properties_count_err.set(None);
            // One scan at a time, and it can be stopped. Any older token is
            // cancelled rather than dropped: dropping one abandons the *answer*
            // while the server keeps scanning.
            let token = CancellationToken::new();
            if let Some(old) = counting_token.borrow_mut().replace(token.clone()) {
                old.cancel();
            }
            let want = target.clone();
            let report = create_ext_action(cx, move |res: Result<u64, String>| {
                if properties.with_untracked(|t| t.as_ref() != Some(&want)) {
                    return;
                }
                properties_counting.set(false);
                match res {
                    Ok(n) => {
                        properties_state.update(|st| {
                            // An engine with no statistics still gets its count:
                            // the state becomes a `Loaded` holding nothing but
                            // the one figure it could answer.
                            let mut stats = match st {
                                schemaic_ui::PropertiesState::Loaded(s) => s.clone(),
                                _ => Box::new(schemaic_core::stats::TableStats {
                                    table: want.table.clone(),
                                    schema: want.schema.clone(),
                                    ..Default::default()
                                }),
                            };
                            stats.exact_rows = Some(n);
                            *st = schemaic_ui::PropertiesState::Loaded(stats);
                        });
                    }
                    // A cancelled count is not a failure to report: the user asked
                    // for it to stop, and the estimate they already have is what
                    // the panel goes back to showing.
                    Err(e) if e == schemaic_db::DbError::Cancelled.to_string() => {}
                    Err(e) => properties_count_err.set(Some(e)),
                }
            });
            handle.spawn(async move {
                let res = db
                    .count_rows(
                        &target.database,
                        target.schema.as_deref(),
                        &target.table,
                        token,
                    )
                    .await
                    .map_err(|e| e.to_string());
                report(res);
            });
        })
    };

    let toggle_table_sizes: Rc<dyn Fn()> = Rc::new(move || table_sizes.update(|on| *on = !*on));

    // Fetch one database's table statistics into its node's slot, once.
    //
    // **`ConnNode::stats` is both the trigger and the guard.** Only a node at
    // `Idle` is fetched, and moving it to `Loading` before the spawn is what stops
    // a second ask — from the size-column effect below, from a capped result's
    // toolbar, or from the two at once — becoming a second query. Nothing here
    // retries a failure either: a column that re-queried a failing server on every
    // expand would cost more than it is worth, so a refusal is remembered until a
    // refresh puts the slot back to `Idle`.
    let fetch_db_stats: Rc<dyn Fn(u64, String, RwSignal<schemaic_ui::DbStatsState>)> = {
        let db_for = db_for.clone();
        let handle = handle.clone();
        Rc::new(
            move |conn_id: u64, database: String, slot: RwSignal<schemaic_ui::DbStatsState>| {
                if slot.get_untracked() != schemaic_ui::DbStatsState::Idle {
                    return;
                }
                let Ok(db) = db_for(conn_id) else {
                    return;
                };
                // An engine with nothing to publish is settled here rather than
                // by a round trip that would come back empty and be retried on
                // the next expand.
                if !schemaic_core::stats::supports_table_stats(db.engine().dialect()) {
                    slot.set(schemaic_ui::DbStatsState::Unavailable);
                    return;
                }
                slot.set(schemaic_ui::DbStatsState::Loading);
                let report =
                    create_ext_action(cx, move |res: Option<schemaic_core::stats::SchemaStats>| {
                        slot.set(match res {
                            Some(set) => schemaic_ui::DbStatsState::Loaded(set),
                            None => schemaic_ui::DbStatsState::Unavailable,
                        });
                    });
                handle.spawn(async move {
                    report(db.fetch_table_stats(&database).await.ok());
                });
            },
        )
    };

    // The same fetch, asked for by name — the results toolbar's route to a row
    // estimate for a capped result (`SchemaActions::db_stats`). It is deliberately
    // free to ask on every capped result: the slot above answers all but the first.
    //
    // **The connection has to match.** `db_nodes` holds the *active* connection's
    // databases, so filling a slot from a query tab bound to some other server
    // would write one server's figures into another's tree. A tab of another
    // connection is not on screen anyway (the strip shows the active connection's
    // tabs), which makes this a guard rather than a case.
    let db_stats: Rc<dyn Fn(u64, String)> = {
        let fetch = fetch_db_stats.clone();
        Rc::new(move |conn_id: u64, database: String| {
            if conn_id != active_conn.get_untracked() {
                return;
            }
            let slot = db_nodes.with_untracked(|nodes| {
                nodes
                    .iter()
                    .find(|n| n.database == database)
                    .map(|n| n.stats)
            });
            if let Some(slot) = slot {
                (fetch)(conn_id, database, slot);
            }
        })
    };

    // Fill the schema tree's size column, one database at a time and only for
    // the ones the user can actually see: sizes on, and the database expanded.
    //
    // Which is also why the slots are read untracked, and why a refresh has to
    // announce itself through `stats_gen` instead: tracking them would make this
    // effect its own dependency, re-entering on the first `Loading` write and
    // re-fetching every database it had not reached yet.
    {
        let fetch = fetch_db_stats.clone();
        create_effect(move |_| {
            if !table_sizes.get() {
                return;
            }
            let conn_id = active_conn.get();
            // A refresh reset some node to `Idle`; nothing else here would see it.
            stats_gen.track();
            // `with`, not `get`: the expanded set holds one key per open database,
            // table and folder — thousands in a working session — and `get` would
            // clone the whole `HashSet` to answer one membership test per database.
            // A connection-wide refresh bumps `stats_gen` once per database, so it
            // was one full clone per database per refresh. Tracking is identical
            // either way.
            let pending: Vec<(String, RwSignal<schemaic_ui::DbStatsState>)> =
                expanded.with(|open| {
                    db_nodes.with(|nodes| {
                        nodes
                            .iter()
                            .filter(|n| open.contains(&schemaic_ui::db_key(&n.database)))
                            .filter(|n| n.stats.get_untracked() == schemaic_ui::DbStatsState::Idle)
                            .map(|n| (n.database.clone(), n.stats))
                            .collect()
                    })
                });
            for (database, slot) in pending {
                (fetch)(conn_id, database, slot);
            }
        });
    }

    let trigger_functions: schemaic_ui::TriggerFnFn = {
        let handle = handle.clone();
        let db_for = db_for.clone();
        Rc::new(
            move |req: schemaic_ui::TriggerFnRequest, done: schemaic_ui::TriggerFnDoneFn| {
                let Ok(db) = db_for(req.conn_id) else {
                    // Nothing to report: the dropdown stays on whatever the
                    // draft already names, which is the honest empty state.
                    return;
                };
                let report = create_ext_action(cx, move |fns| (done)(fns));
                handle.spawn(async move {
                    // A failure here isn't worth interrupting an edit for — the
                    // user can still type a function name by hand.
                    let fns = db
                        .trigger_functions(&req.database)
                        .await
                        .unwrap_or_default();
                    report(fns);
                });
            },
        )
    };

    let run_ddl: schemaic_ui::DdlFn = {
        let handle = handle.clone();
        let db_for = db_for.clone();
        let refresh_db = refresh_db.clone();
        let guard_tx = guard_tx.clone();
        Rc::new(
            move |req: schemaic_ui::DdlRunRequest, done: schemaic_ui::DdlDoneFn| {
                let db = match db_for(req.conn_id) {
                    Ok(db) => db,
                    Err(e) => {
                        (done)(DdlOutcome::Failed(e));
                        return;
                    }
                };
                let conn_id = req.conn_id;
                let req = Rc::new(req);

                // The apply itself, run once nothing is in its way.
                let start: Rc<dyn Fn()> = {
                    let handle = handle.clone();
                    let refresh_db = refresh_db.clone();
                    let done = done.clone();
                    Rc::new(move || {
                        let db = db.clone();
                        let req = req.clone();
                        let done = done.clone();
                        let refresh_db = refresh_db.clone();
                        let database = req.database.clone();
                        let report = create_ext_action(
                            cx,
                            move |(changed, res): (bool, Result<(), String>)| {
                                // Refresh before reporting, so the modal's success
                                // state and the tree can't be seen disagreeing for
                                // a frame. `changed`, not `is_ok()`: a MySQL plan
                                // that failed halfway still moved the schema out
                                // from under us.
                                if changed {
                                    (refresh_db)(database.clone());
                                }
                                (done)(match res {
                                    Ok(()) => DdlOutcome::Applied,
                                    Err(e) => DdlOutcome::Failed(e),
                                });
                            },
                        );
                        // Owned copies: the plan crosses onto a runtime worker,
                        // and the `Rc` holding it can't.
                        let (database, statements) = (req.database.clone(), req.statements.clone());
                        handle.spawn(async move {
                            let out = db
                                .run_ddl(&database, &statements, CancellationToken::new())
                                .await;
                            let changed = schemaic_db::ddl_changed_schema(&out);
                            report((changed, out.map_err(|e| e.to_string())));
                        });
                    })
                };

                // A schema change is the tab's own work *and* a write, so it
                // takes neither branch of the one-connection-per-operation rule
                // cleanly: it runs on a fresh connection, and then waits there
                // for the lock the user's own uncommitted transaction is holding
                // — with no timeout on either engine and every modal exit
                // refusing while an apply is in flight. So ask first, one prompt
                // per open transaction on this connection, chained the way
                // `close_tabs_seq` chains its closes (`tx_prompt` holds one
                // question at a time).
                let snapshot = tabs.with_untracked(|v| {
                    v.iter()
                        .map(|t| TabTx {
                            tab_id: t.id,
                            conn_id: t.conn_id.get_untracked(),
                            state: t.tx.get_untracked(),
                        })
                        .collect::<Vec<_>>()
                });
                let declined: Rc<dyn Fn()> = Rc::new(move || (done)(DdlOutcome::Declined));
                let mut proceed = start;
                for tab_id in ddl_blocking_tabs(&snapshot, conn_id).into_iter().rev() {
                    let guard_tx = guard_tx.clone();
                    let next = proceed.clone();
                    let declined = declined.clone();
                    proceed =
                        Rc::new(move || (guard_tx)(tab_id, next.clone(), Some(declined.clone())));
                }
                proceed();
            },
        )
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
            // Clear stale status until this connection's own check lands. The
            // failure count goes with it — the previous connection's backoff
            // says nothing about this one.
            conn_status.set(ConnStatus::Unknown);
            health_failures.set(0);
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
    // The forward reference declared beside `open_sql_file`, which needs this to
    // reach a file already open under another connection.
    *switch_conn_late.borrow_mut() = Some(switch_conn.clone());

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

    // Duplicate a saved connection: same server, new identity, selected in the
    // form so the one thing the copy is missing — what makes it different — is
    // where the cursor already is.
    //
    // Persisted at once, unlike New, which leaves an unsaved draft. The copy's
    // whole value is the credentials it carries, and those only reach the
    // keyring through a save; a duplicate that evaporates when the user clicks
    // another row would have saved them nothing.
    let duplicate_conn: Rc<dyn Fn(u64)> = {
        let select_conn = select_conn.clone();
        Rc::new(move |id: u64| {
            let Some(src) =
                connections.with_untracked(|cs| cs.iter().find(|c| c.id == id).cloned())
            else {
                return;
            };
            let (names, used_colors, next_id) = connections.with_untracked(|cs| {
                (
                    cs.iter().map(|c| c.name.clone()).collect::<Vec<_>>(),
                    cs.iter()
                        .filter_map(|c| c.color.clone())
                        .collect::<Vec<_>>(),
                    Connection::next_id(cs),
                )
            });
            // A fresh colour, not the original's: the dot is what tells two
            // connections apart in the switcher and on their tabs, and a copy
            // of a connection is precisely the case where the *names* are
            // nearly identical too.
            let copy = src.duplicate(
                next_id,
                unique_name(&format!("{} (copy)", src.name), &names),
                Some(pick_connection_color(&used_colors)),
            );
            connections.update(|cs| cs.push(copy));
            persist_conns(Some(active_conn.get_untracked()));
            (select_conn)(next_id);
        })
    };

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
                let tunnel = if conn.uses_tunnel() {
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
            let id = draft
                .id
                .get_untracked()
                .unwrap_or_else(|| connections.with_untracked(|cs| Connection::next_id(cs)));
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
    let delete_conn_now: Rc<dyn Fn(u64)> = {
        let load_schema = load_schema.clone();
        let tunnels = tunnels.clone();
        let drop_session = drop_session.clone();
        let tokens = tokens.clone();
        let recently_closed = recently_closed.clone();
        let last_tab = last_tab.clone();
        let open_tab_on = open_tab_on.clone();
        let save_db_colors = save_db_colors.clone();
        let save_db_favorites = save_db_favorites.clone();
        let save_formats = save_formats.clone();
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
                &schemaic_core::chat::ChatFile::of(&saved_chats.get_untracked()),
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
            // The connection's tabs go with it. Folding them into another
            // connection's strip would be a contradiction — tabs scoped to a
            // connection that no longer exists — and deleting a connection
            // (often a temporary or production one) shouldn't leave its queries
            // behind.
            let doomed: Vec<(usize, floem::reactive::Scope)> = tabs.with_untracked(|v| {
                v.iter()
                    .filter(|t| t.conn_id.get_untracked() == id)
                    .map(|t| (t.id, t.cx))
                    .collect()
            });
            for (tab_id, _) in &doomed {
                // H5: cancel in-flight work so it can't complete onto freed
                // signals (its session was already released above).
                if let Some((_, tok)) = tokens.borrow_mut().remove(tab_id) {
                    tok.cancel();
                }
            }
            tabs.update(|v| v.retain(|t| t.conn_id.get_untracked() != id));
            recently_closed.borrow_mut().retain(|s| s.conn_id != id);
            last_tab.borrow_mut().remove(&id);
            // Whatever is active now may have just been removed; make sure it's
            // a tab the strip will show, opening one if this connection has none.
            let adopting = active_conn.get_untracked();
            match schemaic_core::tabsel::pick_active(
                &tab_refs(),
                adopting,
                Some(active.get_untracked()),
            ) {
                Some(tab) => active.set(tab),
                None => (open_tab_on)(adopting, None),
            }
            // Scopes are disposed a tick later, once the center view has rebuilt
            // for the new active tab — freeing them now drops signals a mounted
            // view still reads this frame (C14).
            if !doomed.is_empty() {
                exec_after(Duration::ZERO, move |_| {
                    for (_, scope) in doomed {
                        scope.dispose();
                    }
                });
            }

            // Everything else keyed to this connection goes too. A deleted
            // connection shouldn't be reconstructable from what's left on disk —
            // its queries, the databases it had, the tables looked at.
            history_entries.update(|v| schemaic_core::history::clear_conn(v, id));
            persist::save_json(
                "history.json",
                &schemaic_core::history::HistoryFile {
                    entries: history_entries.get_untracked(),
                },
            );
            // Persisted by an effect on change.
            search_history.update(|v| schemaic_core::search_history::clear_conn(v, id));
            db_colors.update(|v| schemaic_core::db_color::clear_conn(v, id));
            table_colors.update(|v| schemaic_core::db_color::table_clear_conn(v, id));
            // One save, both stores — see where `save_db_colors` is built.
            (save_db_colors)();
            db_favorites.update(|v| schemaic_core::favorite::clear_conn(v, id));
            (save_db_favorites)();
            formats.update(|v| schemaic_core::format::clear_conn(v, id));
            (save_formats)();
            // Diagram layouts live only on disk (no signal) — load, prune, save.
            let mut layouts: schemaic_core::erd::DiagramLayoutsFile =
                persist::load_json("diagrams.json");
            schemaic_core::erd::clear_conn_layouts(&mut layouts, id);
            persist::save_json("diagrams.json", &layouts);
        })
    };

    // Ask first. **Every one of those is unrecoverable**: the three keyring
    // entries (`conn.{id}.password` / `.ssh_password` / `.ssh_passphrase`), the
    // saved AI conversation, the query history, the tabs *and* their editor
    // contents — and `recently_closed` is filtered too, so Ctrl+Shift+T cannot
    // bring them back.
    //
    // It ran on the click, from a two-entry menu whose other entry is
    // `Duplicate`, a few pixels above. Meanwhile the app raises a confirm for
    // "close other tabs", whose tabs *are* recoverable — so this was an internal
    // inconsistency rather than a house style, and the inconsistency ran the
    // wrong way round.
    //
    // The message names the connection, because the menu is opened at the
    // cursor over a list of rows that look alike, and says the stored password
    // goes with it, because that is the part no re-typing of a host and port
    // gets back.
    let delete_conn: Rc<dyn Fn(u64)> = {
        let delete_conn_now = delete_conn_now.clone();
        Rc::new(move |id: u64| {
            let name = connections.with_untracked(|cs| {
                cs.iter()
                    .find(|c| c.id == id)
                    .map(|c| c.name.clone())
                    .unwrap_or_default()
            });
            let delete = delete_conn_now.clone();
            confirm.set(Some(Confirm {
                title: format!("Delete “{name}”"),
                message: "This deletes the connection, its stored password, its saved AI \
                          conversation, its query history and its tabs. It can't be undone."
                    .to_string(),
                resolve: Rc::new(move |yes| {
                    if yes {
                        (delete)(id);
                    }
                }),
            }));
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
            // Ctrl+K generates SQL that lands straight in the editor, so it has
            // to be the active connection's dialect — a Postgres tab was being
            // handed MySQL syntax.
            let conn_id = active_conn.get_untracked();
            let dialect = connections
                .with_untracked(|cs| {
                    cs.iter()
                        .find(|c| c.id == conn_id)
                        .map(|c| SqlDialect::from_db_type(&c.db_type))
                })
                .unwrap_or_default();
            let system = inline_system_prompt(db_nodes, active_db.as_deref(), &req, dialect);
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

    // Leaving the tab a Ctrl+K generation belongs to cancels it.
    //
    // The editor pane is keyed on the active tab, so a switch disposes the pane
    // and its `CmdK` — and nothing was left holding the generation. The `claude`
    // child ran to completion (never aborted, so `kill_on_drop` never fired: the
    // request was billed and answered in full), and the reply set this *global*
    // signal to `Ready` while no pane had the popup open, so it rendered nowhere
    // and was unreachable on return. The user saw the prompt simply vanish.
    //
    // Watching `active` rather than the pane's teardown, because floem has no
    // scope-cleanup hook to hang this on — and it is the more honest signal
    // anyway: the generation is bound to the tab it was started from, and
    // `inline_ai` being one global signal means there is only ever one to cancel.
    {
        let inline_ai_cancel = inline_ai_cancel.clone();
        create_effect(move |prev: Option<usize>| {
            let id = active.get();
            if let Some(prev) = prev
                && prev != id
                && matches!(inline_ai.get_untracked(), InlineAiState::Busy)
            {
                (inline_ai_cancel)();
            }
            id
        });
    }

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
                // The implicit row key is dropped: `sample_sql` doesn't project one.
                let (ddl, pk_cols, _) = table_ddl_and_pk(db_nodes, &req.source, dialect_of(&db));
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
                        dialect_for(db.engine()),
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
                // The implicit row key is dropped: `sample_sql` doesn't project one.
                let (ddl, pk_cols, _) = table_ddl_and_pk(db_nodes, &req.source, dialect_of(&db));
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
                        dialect_for(db.engine()),
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

    // ── Connection health poll ───────────────────────────────────────────────
    // Health-check the active connection now, then keep re-checking so
    // `ConnStatus` stays worth trusting: a server that dies mid-session goes red
    // on its own, and one that comes back goes green without the user clicking
    // Retry. Every gate that reads `is_down()` gets a fresher answer for it.
    //
    // What to do on each tick is `core::health`'s call (pure + tested) — ping or
    // skip, and how long until the next one. This closure only gathers the
    // snapshot it decides from, and re-arms.
    //
    // Perpetual, so every signal read goes through `try_*_untracked`: at
    // shutdown the scope disposes and a pending timer would otherwise panic on a
    // freed signal. `None` from any read means "the app is going away" — stop
    // rescheduling.
    let health_poll: Rc<dyn Fn() -> Option<std::time::Duration>> = {
        let check_conn = check_conn.clone();
        let tunnels = tunnels.clone();
        let tokens = tokens.clone();
        Rc::new(move || {
            let status = conn_status.try_get_untracked()?;
            let failures = health_failures.try_get_untracked()?;
            let focused = window_focused.try_get_untracked()?;
            let id = active_conn.try_get_untracked()?;
            // An id with no saved connection behind it (none configured yet)
            // reads as "not tunnelled" — `check_conn` handles the nothing-to-ping
            // case itself.
            let ssh = connections
                .try_with_untracked(|cs| {
                    cs.map(|cs| cs.iter().find(|c| c.id == id).map(|c| c.uses_tunnel()))
                })?
                .unwrap_or(false);
            // A run already in flight against *this* connection probes it far
            // better than `SELECT 1` does. Runs on another connection's tabs
            // don't count — they say nothing about this server.
            let any_running = !tokens.borrow().is_empty();
            let busy = any_running
                && tabs.try_with_untracked(|ts| {
                    ts.map(|ts| {
                        ts.iter().any(|t| {
                            t.conn_id.get_untracked() == id && tokens.borrow().contains_key(&t.id)
                        })
                    })
                })?;
            let ctx = health::TickCtx {
                status,
                failures,
                busy,
                focused,
                tunnelled: ssh,
                tunnel_pending: ssh && !tunnels.borrow().contains_key(&id),
            };
            let tick = health::tick(health::HealthCfg::default(), ctx);
            if tick.ping() {
                check_conn();
            }
            Some(tick.next)
        })
    };
    fn arm_health_poll(
        delay: std::time::Duration,
        poll: Rc<dyn Fn() -> Option<std::time::Duration>>,
    ) {
        floem::action::exec_after(delay, move |_| {
            if let Some(next) = poll() {
                arm_health_poll(next, poll.clone());
            }
        });
    }
    check_conn();
    arm_health_poll(
        health::interval(health::HealthCfg::default(), false),
        health_poll,
    );

    // Regaining focus re-checks immediately: the poll pauses while the window is
    // in the background, so this is what makes coming back to Schemaic show a
    // current status instead of however things stood when the user left.
    {
        let check_conn = check_conn.clone();
        create_effect(move |prev: Option<bool>| {
            let focused = window_focused.get();
            // Only a real false → true transition. `prev` is `None` on the
            // effect's own first run, which is a mount, not a focus change (and
            // the startup check above already covers it).
            if prev == Some(false) && focused {
                check_conn();
            }
            focused
        });
    }

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
    // Which engine the terminal is a CLI for, or `None` for an ordinary shell —
    // the panel title's badge. Every respawn sets it (this one starts a shell),
    // since a session is only ever replaced, never layered.
    let term_db_label: RwSignal<Option<String>> = RwSignal::new(None);

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
                // The DB-CLI badge outlives the session it names unless something
                // notices the client quit (`\q`, `exit`). The reader thread's
                // EOF-notify is that something, and it lands on this same tick.
                if t.has_exited() && term_db_label.get_untracked().is_some() {
                    term_db_label.set(None);
                }
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
                    term_db_label.set(None); // back to a plain shell
                    (term_notify)();
                }
                Err(e) => tracing::error!("terminal restart failed: {e}"),
            }
        })
    };
    // Open the DB CLI for the active connection in the terminal — `mysql`/
    // `mariadb` or `psql`, per the connection's engine — optionally scoped to a
    // database. Reveals the terminal panel and respawns it as a dedicated client
    // session.
    let open_db_cli: Rc<dyn Fn(Option<String>)> = {
        let terminal = terminal.clone();
        let term_dims = term_dims.clone();
        let term_notify = term_notify.clone();
        let tunnels = tunnels.clone();
        Rc::new(move |db: Option<String>| {
            // Guard the panel reveal: a redundant `set` rebuilds the panel
            // `dyn_container` (docs/architecture.md gotcha / review H11).
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
            let conn = if conn.uses_tunnel() {
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
                            // A message, not a session — nothing to badge.
                            term_db_label.set(None);
                            (term_notify)();
                        }
                        return;
                    }
                }
            } else {
                conn
            };
            // **A match on the engine, not `if Postgres { … } else { mysql }`** —
            // which is what this was, and it sent a SQLite connection to the MySQL
            // client with the inert `127.0.0.1:3306` of a *file* connection: either
            // "no client found" or, on a machine that has one, a session against
            // some unrelated local server presented as this connection's. Exhaustive
            // here, so a fourth engine is a compile error rather than a wrong guess
            // (the same reason `dialect_of` was rewritten this way).
            let built = match schemaic_db::Engine::from_db_type(&conn.db_type) {
                schemaic_db::Engine::Postgres => {
                    // The button on the terminal's toolbar passes no database, and
                    // psql needs one. Fall back to the focused tab's — but only when
                    // that tab is on this connection, or we'd name a database from
                    // another server (`scoped_database`'s whole reason for being).
                    let tab = tabs.with_untracked(|v| {
                        v.iter()
                            .find(|t| t.id == active.get_untracked())
                            .map(|t| (t.conn_id.get_untracked(), t.database.get_untracked()))
                    });
                    let scoped = scoped_database(tab, active_conn.get_untracked(), None);
                    let target = psql_database(db.as_deref(), scoped.as_deref());
                    psql_shell(&conn, &target).ok_or("No psql client found (PATH or WSL).")
                }
                // `db` is ignored: a SQLite connection's one database is the file
                // itself, which the config already names.
                schemaic_db::Engine::Sqlite => {
                    sqlite_shell(&conn).ok_or("No sqlite3 client found on PATH.")
                }
                schemaic_db::Engine::MySql => mysql_shell(&conn, db.as_deref())
                    .ok_or("No mysql/mariadb client found (PATH or WSL)."),
            };
            // Badge the panel only for a session that really is a client. The
            // no-client arm spawns a message instead, which is nobody's engine.
            let label = built
                .is_ok()
                .then(|| schemaic_core::connection::engine_label(&conn.db_type));
            let cfg = built.unwrap_or_else(message_shell);
            let (cols, rows) = term_dims.get();
            match schemaic_term::Terminal::spawn(&cfg, cols, rows, term_notify.clone()) {
                Ok(t) => {
                    *terminal.borrow_mut() = Some(t);
                    term_db_label.set(label);
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
                    term_db_label.set(None); // a shell profile, not a client
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

    // One background update check per launch. Returns the "Restart to update"
    // action; both are inert (and the check never reaches the network) unless this
    // is a Velopack-installed build — see `update::start`.
    let update_state = RwSignal::new(schemaic_core::update::UpdateState::default());
    let apply_update = update::start(cx, &handle, window, update_state);

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
            // Already connection-gated *and* write-guarded — see `guarded_run`.
            run: guarded_run.clone(),
            apply_view,
            run_all: guarded_run_all.clone(),
            run_anyway: run_anyway.clone(),
            cancel,
            commit_edits: {
                // Writes need the same gate, but a `CommitFn` reports back
                // through its own callback — so a blocked commit answers the
                // grid with an error instead of leaving it spinning.
                let g = with_conn.clone();
                let f = commit_edits.clone();
                Rc::new(move |w, r, done: schemaic_ui::CommitDoneFn| {
                    if conn_status.get_untracked().is_down() {
                        (g)(Rc::new(|| {}));
                        done(schemaic_core::model::CommitDone::Failed(
                            "Not connected — the commit was not attempted. Staged \
                             edits are kept."
                                .into(),
                        ));
                        return;
                    }
                    f(w, r, done);
                })
            },
            // Deliberately un-gated: an export writes rows that are already
            // fetched and sitting in memory, so it works fine on a connection
            // that has since gone away.
            export_file,
            set_tx_mode,
            commit_tx,
            rollback_tx,
            add_tab: {
                let g = with_conn.clone();
                let f = add_tab.clone();
                Rc::new(move || (g)(f.clone()))
            },
            close_tab,
            close_all_tabs,
            close_other_tabs,
            toggle_pin,
            duplicate_tab,
            open_table,
            open_table_new,
            open_table_col,
            open_query,
            // Deliberately un-gated on the connection, like the export: reading
            // and writing a `.sql` file is between the editor and the disk, and
            // works perfectly well against a server that has gone away.
            open_sql_file,
            save_sql_file,
            save_sql_file_as,
            reload_sql_file,
            reopen_closed_tab,
            can_reopen_closed_tab,
            can_close_other_tabs,
            open_table_filtered,
            set_active_db,
            open_db_cli,
            run_plan: {
                let g = with_conn.clone();
                let f = run_plan.clone();
                Rc::new(move |sql: String, analyze: bool| {
                    let f = f.clone();
                    (g)(Rc::new(move || f(sql.clone(), analyze)))
                })
            },
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
            confirm,
            plan_open,
            plan_state,
            plan_sql,
            plan_analyze,
            monitor_open,
            monitor_title,
            monitor_cols,
            monitor_log,
            monitor_error,
            monitor_partial,
            monitor_interval,
            monitor_paused,
            monitor_export_err,
            monitor_exported,
            monitor_dropped,
            erd: RwSignal::new(None),
            properties,
            properties_state,
            properties_counting,
            properties_count_err,
            run_guard,
        },
        schema: SchemaUi {
            db_nodes,
            stats_gen,
            expanded,
            active_table,
            hidden_dbs,
            table_sizes,
            db_menu_open,
            schema_menu_open,
            db_menu_anchor: RwSignal::new(floem::kurbo::Point::ZERO),
            schema_menu_anchor: RwSignal::new(floem::kurbo::Point::ZERO),
        },
        schema_actions: Rc::new(SchemaActions {
            on_toggle,
            toggle_db_hidden,
            collapse_all,
            collapse_db,
            refresh_schema,
            refresh_db,
            import_probe,
            import_run,
            import_cancel: {
                let import_token = import_token.clone();
                Rc::new(move || {
                    if let Some(t) = import_token.borrow().as_ref() {
                        t.cancel();
                    }
                })
            },
            run_ddl,
            view_algorithm,
            trigger_functions,
            trigger_source,
            table_stats,
            count_rows,
            count_cancel,
            toggle_table_sizes,
            db_stats,
        }),
        // Reset on every open (`import_view::open_import`), so one bundle serves
        // every table rather than a per-open scope that would need disposing.
        import: schemaic_ui::ImportUi {
            target: RwSignal::new(None),
            step: RwSignal::new(schemaic_ui::ImportStep::Source),
            path: RwSignal::new(None),
            format: RwSignal::new(schemaic_core::import::ImportFormat::Csv),
            delimiter: RwSignal::new(",".to_string()),
            has_header: RwSignal::new(true),
            empty_is_null: RwSignal::new(true),
            null_tokens: RwSignal::new(String::new()),
            trim: RwSignal::new(false),
            file_bytes: RwSignal::new(0),
            sample: RwSignal::new(None),
            mapping: RwSignal::new(schemaic_core::import::Mapping {
                targets: Vec::new(),
            }),
            issues: RwSignal::new(Vec::new()),
            more_issues: RwSignal::new(false),
            error: RwSignal::new(None),
            imported: RwSignal::new(0),
            reading: RwSignal::new(false),
            loading: RwSignal::new(false),
            applying: RwSignal::new(false),
            generation: RwSignal::new(0),
            probe_seq: RwSignal::new(0),
        },
        // Same rule as `import` above: reset on open, so one bundle serves every
        // table rather than a per-open scope that would need disposing.
        ddl: schemaic_ui::DdlUi {
            designer: RwSignal::new(None),
            draft: RwSignal::new(schemaic_core::ddl::TableDraft::default()),
            tab: RwSignal::new(schemaic_ui::DesignerTab::Table),
            selected: RwSignal::new(0),
            rev: RwSignal::new(0),
            view: RwSignal::new(None),
            view_draft: RwSignal::new(schemaic_core::ddl::ViewDraft::default()),
            view_rows: RwSignal::new(14),
            trigger: RwSignal::new(None),
            trigger_draft: RwSignal::new(schemaic_core::ddl::TriggerSetDraft::default()),
            function: RwSignal::new(None),
            function_draft: RwSignal::new(schemaic_core::ddl::FunctionDraft::default()),
            functions: RwSignal::new(Vec::new()),
            object: RwSignal::new(None),
            object_draft: RwSignal::new(schemaic_core::ddl::ObjectDraft::default()),
            object_errors: RwSignal::new(Vec::new()),
            object_rev: RwSignal::new(0),
            preview: RwSignal::new(None),
            sql: RwSignal::new(String::new()),
            sql_rows: RwSignal::new(16),
            applying: RwSignal::new(false),
            error: RwSignal::new(None),
            applied: RwSignal::new(false),
            generation: RwSignal::new(0),
            session: RwSignal::new(0),
        },
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
            duplicate_conn,
            save_conn,
            toggle_read_only,
            delete_conn,
            test_conn,
            recheck_conn: check_conn.clone(),
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
            // The assistant's DB tools can't reach a dead connection and its
            // schema never loaded, so a turn would answer from nothing.
            send: gate1(&with_conn, &ai_send),
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
            db_label: term_db_label,
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
            window_focused,
        },
        persist_layout: save_ui.clone(),
        formats,
        save_formats,
        db_colors,
        table_colors,
        save_db_colors,
        db_favorites,
        save_db_favorites,
        resources,
        update_state,
        apply_update,
    };
    // Every config file has been loaded by now. If any of them was unreadable it
    // was preserved as `.corrupt` and recovered from the backup or defaults —
    // which from the user's side looks like their connections or preferences just
    // vanished, so say so instead of only logging it.
    let recoveries = persist::take_recoveries();
    if !recoveries.is_empty() {
        error_modal_text.set(Some(recoveries.join("\n\n")));
        error_modal_open.set(true);
    }
    // **The session write that has to happen even though nobody asked for it.**
    // Quitting the window is the one way of losing a tab that never reaches
    // `guard_close`, and floem 0.2 handles `CloseRequested` by closing
    // unconditionally, so there is no veto to hang a prompt off. The close *is*
    // observable, though — `WindowHandle::destroy` fires `WindowClosed` before it
    // disposes the scope — which is enough to make the debounced save's 600 ms
    // window stop mattering. See `flush_session`.
    {
        use floem::views::Decorators;
        schemaic_ui::workspace(ui, window)
            .on_event_cont(floem::event::EventListener::WindowClosed, move |_| {
                flush_session()
            })
    }
}

/// Live Monitor tuning: the poll interval. The two caps — per-poll rows and
/// change-log length — live in `core::monitor`, because the modal's status line
/// has to name both: past `ROW_CAP` it is watching a page rather than a table,
/// and past `LOG_CAP` the log it can export is missing its oldest entries.
const MONITOR_INTERVAL_SECS: u64 = 2;
use schemaic_core::monitor::ROW_CAP as MONITOR_LIMIT;

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
    /// The last poll filled the row cap — the modal says the window is a page.
    partial: RwSignal<bool>,
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
    /// The modal's Pause toggle. Read fresh on each tick, like `interval`.
    paused: RwSignal<bool>,
    /// Whether the log as it stands is already on disk — cleared here the moment
    /// a poll appends anything, so the Clear confirmation asks about a log that
    /// really has no second copy.
    exported: RwSignal<bool>,
    /// How many entries the cap has dropped, accumulated from
    /// [`schemaic_core::monitor::trim_log`] — the status line's caveat reads it
    /// rather than guessing from the log's length.
    dropped: RwSignal<usize>,
}

/// One Live Monitor poll: fetch the watched table (bounded), then hand the result
/// to [`monitor_apply`] on the UI thread. Stops silently if the modal was closed
/// (`open` false) or a newer session superseded this one (`generation` bumped).
///
/// **Pause skips the fetch, not the loop.** Re-arming while paused costs one
/// signal read per interval and keeps resuming free — a pause that unwound the
/// loop would need `open_monitor` to restart it, which resets the baseline and
/// the log, which is the opposite of what Pause is for. The cost is that the
/// baseline ages: the first poll after a resume diffs against the pre-pause
/// table and logs the *net* change at the resume timestamp. That is the log's
/// standing rule (an entry is stamped when a poll observed it), just coarser.
fn monitor_tick(ctx: MonitorCtx, my_gen: u64) {
    // The decision itself is `monitor::tick_action`, where it can be tested —
    // this reads the signals and does what it says. A disposed `open` reads as
    // closed, which it is.
    match schemaic_core::monitor::tick_action(
        ctx.open.try_get_untracked() == Some(true),
        ctx.generation.get() != my_gen,
        ctx.paused.try_get_untracked() == Some(true),
    ) {
        TickAction::Stop => return,
        TickAction::Reschedule => {
            monitor_reschedule(ctx, my_gen);
            return;
        }
        TickAction::Fetch => {}
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
    // The window has to be the *same* window each poll, or the diff reports the
    // window sliding as data changing. Ordering needs the key before the first
    // fetch, and `analyze_edit` can only answer after one — so take it from the
    // already-introspected schema, which is where `analyze_edit` would get it too.
    let order_by = monitor_order_key(ctx.db_nodes, &source);
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
                order_by.as_deref(),
                MONITOR_LIMIT,
                CancellationToken::new(),
            )
            .await
            .map_err(|e| e.to_string());
        send(out);
    });
}

/// The monitored table's primary-key column names, for the poll's `ORDER BY`.
///
/// `None` when the schema isn't loaded or the table has no primary key — the
/// monitor then polls unordered, exactly as before, and (because the snapshot
/// isn't flagged as an ordered window) claims nothing about its tail.
fn monitor_order_key(
    db_nodes: RwSignal<Vec<ConnNode>>,
    source: &TableSource,
) -> Option<Vec<String>> {
    let table = db_nodes.with_untracked(|nodes| {
        nodes
            .iter()
            .find(|n| n.database == source.database)
            .and_then(|n| match n.schema.get_untracked() {
                SchemaState::Loaded(s) => s
                    .find_table(source.schema.as_deref(), &source.table)
                    .cloned(),
                _ => None,
            })
    })?;
    let key: Vec<String> = table
        .columns
        .iter()
        .filter(|c| c.primary_key)
        .map(|c| c.name.clone())
        .collect();
    (!key.is_empty()).then_some(key)
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
            // Flagged as an ordered window only when the poll really was ordered
            // (a resolvable primary key) *and* came back full — that pair is what
            // licenses the diff to treat the tail as the window sliding.
            let full = rs.row_count() >= MONITOR_LIMIT;
            let ordered_full = full && monitor_order_key(ctx.db_nodes, &ctx.target.1).is_some();
            ctx.partial.set(full);
            let snap = Snapshot::from_result(&rs, &key_cols).ordered_window(ordered_full);
            if let Some(prev) = ctx.prev.borrow().as_ref() {
                let changes = diff_snapshots(prev, &snap);
                if !changes.is_empty() {
                    let at = fmt_elapsed(ctx.started.elapsed().as_secs());
                    let mut dropped = 0usize;
                    ctx.log.update(|log| {
                        // Stamp, append and trim in one core call: the sequence
                        // number the rendered list is keyed on is assigned there,
                        // and the cap is applied there too, so the modal's caveat
                        // can't disagree with what the log holds.
                        dropped = schemaic_core::monitor::append_changes(log, &at, changes);
                    });
                    if dropped > 0 {
                        ctx.dropped.update(|n| *n += dropped);
                    }
                    // The file on disk no longer holds what the log holds, so
                    // Clear goes back to asking (`monitor::discard_needs_asking`).
                    ctx.exported.set(false);
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
    use super::{
        CliLauncher, inline_outcome, mysql_shell_config, psql_database, psql_shell_config,
        resolve_native_cli, sqlite_shell_config, unique_name,
    };
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
    fn a_schema_load_nothing_superseded_installs() {
        use super::{LoadLanding, load_landing};
        assert_eq!(load_landing((7, 3), (7, 3)), LoadLanding::Install);
    }

    #[test]
    fn a_schema_load_for_a_connection_the_user_left_installs_nothing() {
        use super::{LoadLanding, load_landing};
        // The slow one lands after the fast one: its nodes, its first-database
        // binding and its per-database fetches all describe a connection the
        // user is no longer looking at.
        assert_eq!(
            load_landing((7, 3), (8, 4)),
            LoadLanding::KeepTunnelOnly,
            "another connection is active now"
        );
    }

    #[test]
    fn an_older_load_of_the_same_connection_installs_nothing_either() {
        use super::{LoadLanding, load_landing};
        // The case an `active_conn` check alone misses: Refresh pressed twice,
        // or switching away and back. Both loads are for the active connection
        // and the first to land is not the one the tree should show — it would
        // also dispose the *newer* node scope.
        assert_eq!(load_landing((7, 3), (7, 4)), LoadLanding::KeepTunnelOnly);
    }

    /// The level `load_landing` doesn't reach. `try_update` guards a *disposed*
    /// scope — a connection switch — and says nothing about a **superseded**
    /// fetch of the same node, which is the interleaving that leaves the tree,
    /// the completion index and the schema editors holding a pre-`ALTER` model
    /// indefinitely.
    /// The whole of the run-id allocator's correctness argument, which was
    /// untested: deleting the `+ 1` at the call site or narrowing the seed to the
    /// active connection left the suite green.
    #[test]
    fn a_run_id_is_seeded_past_every_id_on_disk() {
        use super::run_id_seed;
        use schemaic_core::history::{HistoryEntry, Outcome};
        let e = |conn_id: u64, run_id: u64| HistoryEntry {
            conn_id,
            database: None,
            sql: "SELECT 1".into(),
            ts: 0,
            run_id,
            tab_name: None,
            duration_ms: None,
            rows: None,
            rows_capped: false,
            outcome: Outcome::Unknown,
        };
        // Across **all** connections: `finish` matches by id with no connection
        // filter, so a per-connection seed would let one run's outcome land on
        // another connection's entry.
        assert_eq!(run_id_seed(&[e(1, 3), e(2, 9), e(1, 5)]), 9);
        // Empty history seeds 0, so the first id handed out is 1 — never the 0
        // that entries written before run ids carry.
        assert_eq!(run_id_seed(&[]), 0);
        assert_eq!(run_id_seed(&[e(1, 0), e(1, 0)]), 0);
    }

    #[test]
    fn an_older_introspection_of_the_same_database_writes_nothing() {
        use super::fetch_landing;
        assert!(fetch_landing(4, 4), "nothing newer was asked for");
        assert!(
            !fetch_landing(3, 4),
            "a newer fetch of this node is out; last asked wins, not last to land"
        );
        // The newer one still writes when it lands, whichever order they arrive.
        assert!(fetch_landing(4, 4));
    }

    fn dbs(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    fn nodes(pairs: &[(usize, &str)]) -> Vec<(usize, String)> {
        pairs.iter().map(|(i, n)| (*i, n.to_string())).collect()
    }

    /// A reload of the connection already on screen keeps every database that is
    /// still there — same node, so the same `schema` signal keeps its rows up,
    /// and the same id, so the tree doesn't rebuild the row at all.
    #[test]
    fn a_reload_keeps_the_node_of_every_surviving_database() {
        use super::{NodePlan, plan_nodes};
        let existing = nodes(&[(1, "world"), (2, "sakila")]);
        assert_eq!(
            plan_nodes(&existing, &dbs(&["world", "sakila"]), true),
            vec![NodePlan::Keep(1), NodePlan::Keep(2)]
        );
    }

    /// Reordering the server's list must not renumber anything: the `dyn_stack`
    /// keys on the id, so a renumber rebuilds every database's subtree and drops
    /// its expansion state.
    #[test]
    fn reordering_the_server_list_renumbers_nothing() {
        use super::{NodePlan, plan_nodes};
        let existing = nodes(&[(1, "world"), (2, "sakila")]);
        assert_eq!(
            plan_nodes(&existing, &dbs(&["sakila", "world"]), true),
            vec![NodePlan::Keep(2), NodePlan::Keep(1)]
        );
    }

    /// A database that has *appeared* gets a fresh id, past every one in use —
    /// including the case that made an id counter necessary: a database dropped
    /// and created again must not collide with a node that is still live.
    #[test]
    fn a_reappearing_database_takes_a_fresh_id() {
        use super::{NodePlan, plan_nodes};
        // `sakila` (id 2) is gone; `chinook` is new. The next id is past 2, not
        // reusing it.
        let existing = nodes(&[(1, "world"), (2, "sakila")]);
        assert_eq!(
            plan_nodes(&existing, &dbs(&["world", "chinook"]), true),
            vec![NodePlan::Keep(1), NodePlan::Create(3)]
        );
        // And when it comes back it is a different node again.
        let existing = nodes(&[(1, "world"), (3, "chinook")]);
        assert_eq!(
            plan_nodes(&existing, &dbs(&["world", "chinook", "sakila"]), true),
            vec![NodePlan::Keep(1), NodePlan::Keep(3), NodePlan::Create(4)]
        );
    }

    /// A **switch** reuses nothing, whatever is on screen — the rows belong to
    /// another server.
    #[test]
    fn a_connection_switch_builds_every_node_fresh() {
        use super::{NodePlan, plan_nodes};
        let existing = nodes(&[(1, "world"), (2, "sakila")]);
        assert_eq!(
            plan_nodes(&existing, &dbs(&["world", "sakila"]), false),
            vec![NodePlan::Create(1), NodePlan::Create(2)]
        );
    }

    /// The case a failed connect leaves behind: `reload` is true and there is
    /// nothing to reuse. It still has to produce a usable set.
    #[test]
    fn a_reload_against_an_empty_tree_still_builds_every_node() {
        use super::{NodePlan, plan_nodes};
        assert_eq!(
            plan_nodes(&[], &dbs(&["world"]), true),
            vec![NodePlan::Create(1)]
        );
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
            file: String::new(),
            ssh: Default::default(),
            color: None,
            prominent_color: false,
            read_only: false,
            environment: Default::default(),
        }
    }

    #[test]
    fn native_shell_puts_password_in_env_not_argv() {
        let cfg = mysql_shell_config(CliLauncher::Native("mysql"), &conn(), Some("shop"));
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
        let cfg = mysql_shell_config(CliLauncher::Native("mariadb"), &conn(), None);
        assert_eq!(cfg.args, vec!["-h", "10.0.0.5", "-P", "3307", "-u", "root"]);
    }

    #[test]
    fn wsl_shell_prepends_client_and_forwards_password_via_wslenv() {
        let cfg = mysql_shell_config(CliLauncher::Wsl("mysql"), &conn(), Some("shop"));
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

    // ── The PostgreSQL client ─────────────────────────────────────────────
    // psql takes a different flag for every one of the four parameters (`-p`
    // not `-P`, `-U` not `-u`, `-d` not a bare argument) and a different
    // password variable, which is why it can't share the MySQL builder.

    #[test]
    fn psql_shell_puts_password_in_env_not_argv() {
        let cfg = psql_shell_config(CliLauncher::Native("psql"), &conn(), "chinook");
        assert_eq!(cfg.program, "psql");
        assert_eq!(
            cfg.args,
            vec![
                "-h", "10.0.0.5", "-p", "3307", "-U", "root", "-d", "chinook"
            ]
        );
        assert_eq!(
            cfg.env,
            vec![("PGPASSWORD".to_string(), "s3cr3t".to_string())]
        );
        assert!(!cfg.args.iter().any(|a| a.contains("s3cr3t")));
    }

    #[test]
    fn psql_wsl_shell_prepends_client_and_forwards_password_via_wslenv() {
        let cfg = psql_shell_config(CliLauncher::Wsl("psql"), &conn(), "world");
        assert_eq!(cfg.program, "wsl.exe");
        assert_eq!(
            cfg.args,
            vec![
                "-e", "psql", "-h", "10.0.0.5", "-p", "3307", "-U", "root", "-d", "world"
            ]
        );
        assert_eq!(
            cfg.env,
            vec![
                ("WSLENV".to_string(), "PGPASSWORD/u".to_string()),
                ("PGPASSWORD".to_string(), "s3cr3t".to_string()),
            ]
        );
        assert!(!cfg.args.iter().any(|a| a.contains("s3cr3t")));
    }

    // ── The SQLite client ─────────────────────────────────────────────────
    // A file, not a server: no host, port, user or password to pass, and the one
    // argument is the database file itself.

    /// Forward slashes, which **both** platforms parse as separators — a
    /// backslashed path has no directory part at all on Linux, and this suite runs
    /// on both.
    fn file_conn() -> Connection {
        Connection {
            db_type: "SQLite".to_string(),
            file: "/data/chinook.db".to_string(),
            ..conn()
        }
    }

    #[test]
    fn sqlite_shell_opens_the_file_and_carries_no_secret() {
        let cfg = sqlite_shell_config(CliLauncher::Native("sqlite3"), &file_conn());
        assert_eq!(cfg.program, "sqlite3");
        assert_eq!(cfg.args, vec!["/data/chinook.db"]);
        // Nothing to pass: the server side of a file connection is inert, and the
        // password field of one is empty by construction (`Connection::sanitized`).
        // An env var here would be a credential invented for an engine that has
        // none.
        assert!(cfg.env.is_empty(), "a file has no secret to carry");
    }

    /// **The file's own directory**, so `.output rows.csv` and `.read seed.sql`
    /// land beside the database rather than in whatever directory the app was
    /// started from — which on a desktop launch is not a place the user can find.
    #[test]
    fn sqlite_shell_starts_in_the_databases_directory() {
        let cfg = sqlite_shell_config(CliLauncher::Native("sqlite3"), &file_conn());
        assert_eq!(cfg.cwd.as_deref(), Some("/data"));
    }

    /// The form a Windows connection actually holds, which is where these files
    /// live for this project's own author.
    #[cfg(windows)]
    #[test]
    fn sqlite_shell_handles_a_backslashed_windows_path() {
        let c = Connection {
            file: r"C:\Users\me\dbs\chinook.db".to_string(),
            ..file_conn()
        };
        let cfg = sqlite_shell_config(CliLauncher::Native("sqlite3"), &c);
        assert_eq!(cfg.args, vec![r"C:\Users\me\dbs\chinook.db"]);
        assert_eq!(cfg.cwd.as_deref(), Some(r"C:\Users\me\dbs"));
    }

    /// A file with no parent (a bare name, or a root) must not produce an empty
    /// `cwd` — spawning into `""` fails outright on both platforms.
    #[test]
    fn sqlite_shell_has_no_cwd_when_the_path_has_no_directory() {
        let c = Connection {
            file: "scratch.db".to_string(),
            ..file_conn()
        };
        let cfg = sqlite_shell_config(CliLauncher::Native("sqlite3"), &c);
        assert_eq!(cfg.args, vec!["scratch.db"]);
        assert_eq!(cfg.cwd, None);
    }

    /// **No WSL fallback for SQLite**, unlike the two server clients.
    ///
    /// Their target is a host and a port, which mean the same thing on both sides
    /// of the boundary. A *path* does not: `sqlite3 'C:\data\chinook.db'` inside
    /// WSL doesn't fail, it **creates an empty database** under that literal name
    /// in the current directory, and the user gets a session on a database that
    /// looks like theirs and is empty. Translating to `/mnt/c/...` is the only
    /// honest way to offer it, and nothing here does that yet.
    #[test]
    fn sqlite_client_is_resolved_natively_only() {
        assert!(matches!(
            resolve_native_cli("sqlite3"),
            None | Some(CliLauncher::Native("sqlite3"))
        ));
    }

    #[test]
    fn psql_database_prefers_the_explicit_choice_then_the_active_one() {
        assert_eq!(psql_database(Some("chinook"), Some("world")), "chinook");
        assert_eq!(psql_database(None, Some("world")), "world");
    }

    #[test]
    fn psql_database_falls_back_to_the_maintenance_database() {
        // The terminal toolbar's button passes no database and there may be no
        // active one. psql with no `-d` tries a database named after the user,
        // which is what made the button do nothing at all.
        assert_eq!(psql_database(None, None), "postgres");
        // A blank is not a choice.
        assert_eq!(psql_database(Some("  "), None), "postgres");
        assert_eq!(psql_database(Some(""), Some("world")), "world");
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
