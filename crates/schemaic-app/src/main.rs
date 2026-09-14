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

mod agent_cli;
mod ai;
mod antigravity;
mod conn_sources;
mod dump;
mod heap;
mod liveness;
mod logging;
mod mcp;
mod opencode;
mod script;
mod secrets;
mod update;

/// Process-wide heap accounting (live/peak bytes), for leak-vs-retention
/// diagnosis. Delegates to the system allocator; only adds two atomics per
/// alloc. Logging is opt-in via `SCHEMAIC_HEAP_LOG` (see `heap::spawn_logger`).
#[global_allocator]
static GLOBAL: heap::Tracking = heap::Tracking;

use agent_cli::{detect_bin, harness_bin, harness_reachable};
use ai::{
    AiContextParams, AiSession, AiSettings, AiStreamMsg, RECAP_QUESTIONS, StartAiParams,
    active_tab_database, ai_context, apply_turn_delta, extract_sql, inline_system_prompt,
    mcp_endpoint_from_env, needs_respawn, render_recap, start_ai_session, turn_context,
};
use schemaic_core::tabsel::scoped_database;

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
use schemaic_core::conn_import;
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
/// Record a run's statements and hand back one run id each, in order —
/// `(conn_id, database, statements, tab_name)`. The ids are what
/// [`FinishHistoryFn`] later reports those runs' outcomes against.
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
/// Fill in how runs went — `(run id, outcome)` per run, onto the history
/// entries their launch already wrote — and delete the entries of runs that
/// never happened.
///
/// A **slice**, not one run, because Run Everything lands a whole batch at once
/// and each recorded run would otherwise cost a full rewrite of `history.json`.
/// One call for the whole slice, and one file write for both halves.
type FinishHistoryFn = Rc<dyn Fn(&[(u64, schemaic_core::history::RunResult)], &[u64])>;
use schemaic_ai::harness::Harness;
use schemaic_core::filter::{BrowseKey, Order, table_query};
use schemaic_core::intel::SqlDialect;
use schemaic_core::launch;
use schemaic_core::params;
use schemaic_core::persist::{self, ConnectionsFile, UiState};
use schemaic_core::schema::{SchemaState, TableSource};
use schemaic_core::sql::{GuardPolicy, RunVerdict};
use schemaic_core::tx::{
    self, StmtOutcome, TabTx, TxEngine, TxMode, TxState, ddl_blocking_tabs, session_still_wanted,
};
use schemaic_db::{Db, DbError, Session};
use schemaic_ui::theme::{EditorThemeKind, UiScale, UiThemeKind};
use schemaic_ui::{
    ActivityActions, ActivityState, ActivityUi, AiActions, AiEffort, AiUi, ChatMessage, Confirm,
    ConnActions, ConnImportUi, ConnNode, ConnUi, CtxMenu, DdlOutcome, DraftSignals, HistoryActions,
    HistoryUi, InlineAiRequest, InlineAiState, LayoutUi, MonitorEntry, OverlayUi, PendingRun,
    PlanState, RightPanel, Role, RunGuard, SchemaActions, SchemaScope, SchemaUi, SnippetActions,
    SnippetsUi, Tab, TabsActions, TabsUi, TermActions, TermCursor, TermUi, TestState, TxChoice,
    TxPrompt, Ui, pick_connection_color,
};
use tokio_util::sync::CancellationToken;

fn main() {
    // MCP stdio server mode (launched by whichever agent CLI drives the AI
    // panel). Runs the JSON-RPC loop and exits — no GUI.
    //
    // The (already-tunnelled) endpoint reaches us two ways, and **neither is a
    // command-line argument** (review C6): as a JSON blob in
    // `$SCHEMAIC_MCP_ENDPOINT`, which is what a harness configured by a file we
    // write sets for us (Claude); or read from the path given as
    // `--endpoint-file`, for Codex, whose only configuration lever is `-c`
    // overrides and those *are* argv, and for Antigravity, whose `agy mcp add`
    // takes the child's argv the same way. The path is not a credential; what it
    // points at is, which is the whole reason it is not inlined. No credential
    // URL is involved either way.
    if std::env::args().any(|a| a == "--mcp-serve") {
        // **Refused, not defaulted.** An endpoint that cannot be read used to
        // fall back to `127.0.0.1:3306` with sample rows and catalogue listing
        // switched back *on*, so a session the user had pinned to schema-only
        // started answering from whatever local server was listening. Reported
        // on stderr — stdout is the JSON-RPC stream and nothing may write to it
        // — and the CLI that launched us surfaces the server as failed, which is
        // the honest end state.
        let endpoint = match mcp_endpoint_from_env() {
            Ok(e) => e,
            Err(why) => {
                eprintln!("schemaic --mcp-serve: {why}");
                std::process::exit(2);
            }
        };
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
    //
    // It does one more thing worth knowing, and we rely on the default: Velopack's
    // `auto_apply_on_startup` is **on**, so if a previous session downloaded an
    // update and the user never took the restart, `run()` finds the staged package
    // here and applies it — exiting and relaunching before anything below has run.
    // That is safe *because* of where this sits: no session state has been read or
    // written yet, so there is nothing to lose. Placed after the tab restore it
    // would be a data-loss bug. Turn it off with `.set_auto_apply_on_startup(false)`
    // if the surprise restart ever proves more annoying than the updates are worth.
    velopack::VelopackApp::build().run();

    logging::init();
    // Strictly after `init()`: the hook writes through the subscriber, and a
    // panic before there is one would be formatted and thrown away.
    logging::install_panic_hook();

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
///
/// `Err` carries the line the panel shows instead of a session — "no client
/// found", or whichever refusal [`mysql_shell_config`] reached. The caller has
/// exactly one arm for all of them, which is why the whole refusal set is
/// expressed as this one error type rather than an `Option` plus a second gate.
fn mysql_shell(
    conn: &schemaic_core::connection::Connection,
    db: Option<&str>,
) -> Result<schemaic_term::ShellConfig, &'static str> {
    let launcher =
        resolve_cli(&["mysql", "mariadb"]).ok_or("No mysql/mariadb client found (PATH or WSL).")?;
    mysql_shell_config(launcher, conn, db)
}

/// The PostgreSQL half of [`mysql_shell`]. `db` is required — see
/// [`psql_database`] for how the caller arrives at one.
fn psql_shell(
    conn: &schemaic_core::connection::Connection,
    db: &str,
) -> Result<schemaic_term::ShellConfig, &'static str> {
    let launcher = resolve_cli(&["psql"]).ok_or("No psql client found (PATH or WSL).")?;
    psql_shell_config(launcher, conn, db)
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
///
/// **Two things here are not decoration.** The `--` before the database name is
/// what stops a *server-supplied* name being read as an option: a database
/// called `--pager=touch /tmp/PWN` otherwise runs that command on the user's
/// first query. And the `--ssl-*` flags are mandatory rather than conditional —
/// omitting them leaves the client on its own `PREFERRED` default, which
/// accepts a plaintext socket and verifies nothing, so a connection the user
/// configured for `verify-full` was silently downgraded while the app's header
/// still said TLS. Both decisions live in [`launch`], with their tests.
fn mysql_shell_config(
    launcher: CliLauncher,
    conn: &schemaic_core::connection::Connection,
    db: Option<&str>,
) -> Result<schemaic_term::ShellConfig, &'static str> {
    if matches!(launcher, CliLauncher::Wsl(_))
        && let Some(why) = launch::wsl_tls_blocker(&conn.tls)
    {
        return Err(why);
    }
    let mut cli_args: Vec<String> = vec![
        "-h".into(),
        conn.host.clone(),
        "-P".into(),
        conn.port.to_string(),
        "-u".into(),
        conn.user.clone(),
    ];
    cli_args.extend(launch::mysql_cli_tls_args(&conn.tls));
    if let Some(d) = db {
        cli_args.push("--".into());
        cli_args.push(d.to_string());
    }
    Ok(wrap_launcher(
        launcher,
        cli_args,
        vec![("MYSQL_PWD".to_string(), conn.password.clone())],
    ))
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
    let mut cfg = wrap_launcher(launcher, vec![conn.file.clone()], Vec::new());
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
///
/// Returns an error rather than a config for the two cases a session cannot be
/// built honestly: a database name libpq would re-read as a connection string
/// (see [`launch::psql_target`]) and a TLS file the WSL client cannot open. The
/// caller already renders an `Err` in the panel — it is the same arm the
/// "no client found" message takes.
fn psql_shell_config(
    launcher: CliLauncher,
    conn: &schemaic_core::connection::Connection,
    db: &str,
) -> Result<schemaic_term::ShellConfig, &'static str> {
    let db = launch::psql_target(db)?;
    if matches!(launcher, CliLauncher::Wsl(_))
        && let Some(why) = launch::wsl_tls_blocker(&conn.tls)
    {
        return Err(why);
    }
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
    let mut env = vec![("PGPASSWORD".to_string(), conn.password.clone())];
    env.extend(launch::psql_cli_tls_env(&conn.tls));
    Ok(wrap_launcher(launcher, cli_args, env))
}

/// Turn a client's argv into a spawnable config, native or through WSL, with
/// every variable in `env` carried across the WSL boundary — the half every
/// engine shares, so none of them can lose the rule that the password never
/// reaches the command line, nor the rule that a transport setting reaches the
/// client at all.
///
/// `env` is empty for a client that has nothing to pass (SQLite's), which is not
/// the same as an empty password: it means no variable is set at all, and no
/// `WSLENV` entry naming one.
///
/// **Every entry is forwarded with `/u` and none with `/p`.** `/p` would be the
/// flag for a path needing Win→WSL translation, and there is deliberately no
/// such value here: [`launch::wsl_tls_blocker`] refuses a Windows-shaped
/// certificate path before a WSL config is built, so everything that survives
/// to this point is already a path the Linux side can open.
fn wrap_launcher(
    launcher: CliLauncher,
    cli_args: Vec<String>,
    env: Vec<(String, String)>,
) -> schemaic_term::ShellConfig {
    match launcher {
        CliLauncher::Native(prog) => schemaic_term::ShellConfig {
            program: prog.into(),
            args: cli_args,
            cwd: None,
            env,
        },
        CliLauncher::Wsl(prog) => {
            let mut args: Vec<String> = vec!["-e".into(), prog.into()];
            args.extend(cli_args);
            // WSLENV is what carries the variables across the boundary; without
            // it the password simply doesn't arrive and psql/mysql prompts.
            let mut wsl_env = Vec::with_capacity(env.len() + 1);
            if !env.is_empty() {
                let names: Vec<String> = env.iter().map(|(n, _)| format!("{n}/u")).collect();
                wsl_env.push(("WSLENV".to_string(), names.join(":")));
            }
            wsl_env.extend(env);
            schemaic_term::ShellConfig {
                program: "wsl.exe".into(),
                args,
                cwd: None,
                env: wsl_env,
            }
        }
    }
}

/// An action with no arguments, held by the connection gate across an async
/// re-check.
type Action = Rc<dyn Fn()>;
/// Runs an [`Action`], but only against a connection that answers.
type ConnGate = Rc<dyn Fn(Action)>;
/// [`ConnGate`], with the refusal handed back to the caller.
///
/// The second action runs *instead of* the first, and only when the gate has
/// decided the connection is unreachable. [`ConnGate`] is this with a no-op
/// refusal, which is why there is one gate and not two.
type ConnGateElse = Rc<dyn Fn(Action, Action)>;
/// Reports a health check's outcome.
type CheckDoneFn = Rc<dyn Fn(bool)>;

/// Replaces the terminal session: spawn, install, badge, notify.
///
/// `(config, badge, what)` — `badge` is the engine the session is a client for
/// (`None` for a plain shell or a message), and `what` names the caller in the
/// log. `true` when the new session is running.
type InstallTerminal = Rc<dyn Fn(&schemaic_term::ShellConfig, Option<String>, &str) -> bool>;

/// Why a gated launch never happened.
///
/// A [`ConnGate`] refusal answers the *error modal* and nothing else, which is
/// enough for an action that leaves nothing behind — a refused Run has simply
/// not run, and the modal says so. It is not enough for an action whose caller
/// has already moved a state machine into "in flight" **before** the gate sees
/// it: `open_plan` sets `PlanState::Running` and *then* calls the gated action,
/// so a refusal that speaks only to the error modal leaves the query-plan modal
/// rendering `loading_dots("Explaining")` for ever behind it. Dismissing the
/// error left a modal claiming work was in flight over a connection that was
/// down, and only Escape got out of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Refusal {
    /// The connection is down, and the health re-check did not revive it.
    NotConnected,
    /// The user left the tab the action was started on while the gate held it.
    TabMovedOn,
}

/// What a finished connection test leaves on the Test button.
///
/// The failure arm carries the engine's or the tunnel's own words. It used to be
/// a `bool`, and `open_tunnel`'s `Err(_) => { send(false); return; }` was where
/// `ssh::refusal_message` — several sentences naming the host, both
/// fingerprints, that the key *"has CHANGED since Schemaic first trusted it"*,
/// and the out-of-band check to perform — stopped existing. `ssh::authenticate`'s
/// doc names this button as the surface for exactly those errors.
///
/// The real connect path never had the gap (`Err(e) => send(Err(e.to_string()))`),
/// so only the *diagnostic* control lost the diagnosis.
fn test_outcome(res: Result<(), String>) -> TestState {
    match res {
        Ok(()) => TestState::Ok,
        Err(why) => TestState::Fail(why),
    }
}

/// The query-plan modal's answer to a refused launch.
///
/// A free function rather than a closure inside `app_view` so that the
/// *composition* has a test subject: `plan_refusal_text` on its own says only
/// what the words are, and the defect was never in the words — it was in nobody
/// calling them. Fed to [`gate1_on_tab_answered`], this is what turns a refusal
/// into something the modal can render.
fn plan_refused(plan_state: RwSignal<PlanState>, moved_on: &Rc<dyn Fn()>) -> Rc<dyn Fn(Refusal)> {
    let moved_on = moved_on.clone();
    Rc::new(move |why: Refusal| {
        // The tab case owes the same modal every other pinned action raises; the
        // connection case has already had one from the gate. Both owe the plan
        // modal an answer, which is the half that was missing.
        if why == Refusal::TabMovedOn {
            (moved_on)();
        }
        plan_state.set(PlanState::Failed(plan_refusal_text(why).to_string()));
    })
}

/// What the query-plan modal is left saying when its launch was refused.
///
/// Pure, and separate from the wiring, because it is the *answer* the modal's
/// state machine was missing: every other exit from `run_plan` reports into
/// `plan_state` (its `db_for` failure, its timeout — whose own comment says
/// "nothing is coming to replace this state"), and these two are the exits that
/// happen before `run_plan` is reached at all.
fn plan_refusal_text(why: Refusal) -> &'static str {
    match why {
        Refusal::NotConnected => {
            "Not connected — the plan was not run. The server didn't answer the \
             health check, so nothing was sent to it."
        }
        Refusal::TabMovedOn => {
            "You switched tabs while the connection was being re-checked, so the \
             plan was not run. It describes the tab it was asked from — go back \
             to that tab and ask again."
        }
    }
}

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

/// [`gate1`], **pinned to the tab it was started from**.
///
/// The gate can hold an action for up to five seconds — `PING_TIMEOUT`, spent
/// on a health re-check when the connection is `Disconnected` — and every run
/// action re-resolves its target when it lands: `run_query_core`'s first line is
/// `let id = active.get_untracked();`, and `run_all` and `run_plan` do the same.
/// Nothing is on screen during those seconds: the guard bar has just been taken
/// down and no panel has been opened, so clicking another tab is the natural
/// response to a Run that appears to have done nothing.
///
/// **What was judged is then not what runs.** The write guard's verdict is built
/// from the active tab at press time — `guard_policy` reads its connection, its
/// `read_only`, its dialect and whether it has a database — so a `DELETE`
/// confirmed against a tab bound to `staging` executed against a tab bound to
/// `production`, reported into its panel and was recorded in history under its
/// name. And the `no_database` term is per-tab, so a `CREATE TABLE` judged
/// `Allow` for a database-bound tab could run on a database-less one, past the
/// `Block("No database selected.")` arm — on PostgreSQL, into the hidden
/// maintenance database `needs_database`'s own doc says *"nothing in Schemaic
/// can reach again"*.
///
/// This is the *refusal* half of the fix, not the retarget half: the run is
/// dropped with a message rather than aimed at whatever tab is now in front. The
/// stronger shape is a `run_on(tab_id, …)` the whole pipeline takes instead of
/// re-reading `active` — the tab id is already what `tokens`, `begin_run` and
/// `session_for` are keyed on — and it is deliberately not what this does: it
/// threads through four entry points in a file no test in this workspace can
/// drive, and a refusal is *strictly stronger* than a wrong target, which is the
/// direction the write-guard invariant requires.
///
/// The same hazard is already guarded this way 2,400 lines away, in
/// `commit_edits`' completion: *"`run` targets the active tab, so refreshing
/// after the user switched away would run this tab's SQL against a different
/// tab"*.
///
/// **Only the tab-bound actions take this.** `add_tab` makes a tab rather than
/// using one and `ai_send` is bound to the connection, so pinning either would
/// refuse a gesture that is still correct.
fn gate1_on_tab<A: Clone + 'static>(
    gate: &ConnGate,
    action: &Rc<dyn Fn(A)>,
    active: RwSignal<usize>,
    moved_on: &Rc<dyn Fn()>,
) -> Rc<dyn Fn(A)> {
    let gate = gate.clone();
    let action = action.clone();
    let moved_on = moved_on.clone();
    Rc::new(move |arg: A| {
        let started_on = active.get_untracked();
        let action = action.clone();
        let moved_on = moved_on.clone();
        (gate)(Rc::new(move || {
            if active.get_untracked() != started_on {
                (moved_on)();
                return;
            }
            action(arg.clone())
        }));
    })
}

/// [`gate1_on_tab`], for an action whose caller has **already started a state
/// machine** and so needs an answer when the launch is refused.
///
/// Same pinning, same reasons; the difference is that both refusals — the gate's
/// own "still unreachable" and the tab having moved on — are reported back as a
/// [`Refusal`] rather than only to the error modal. The caller decides what that
/// means: the query-plan modal turns it into `PlanState::Failed`, and also
/// raises `run_moved_on` for the tab case, so the two channels stay one apiece.
///
/// **The refusal is not optional.** Making it a separate `moved_on` callback
/// plus a silent gate is exactly the shape that shipped the spinning modal: the
/// gate wrapped *around* the action answered nobody, while every early exit
/// inside it answered the caller.
fn gate1_on_tab_answered<A: Clone + 'static>(
    gate: &ConnGateElse,
    action: &Rc<dyn Fn(A)>,
    active: RwSignal<usize>,
    refused: &Rc<dyn Fn(Refusal)>,
) -> Rc<dyn Fn(A)> {
    let gate = gate.clone();
    let action = action.clone();
    let refused = refused.clone();
    Rc::new(move |arg: A| {
        let started_on = active.get_untracked();
        let action = action.clone();
        let moved = refused.clone();
        let unreachable = refused.clone();
        (gate)(
            Rc::new(move || {
                if active.get_untracked() != started_on {
                    (moved)(Refusal::TabMovedOn);
                    return;
                }
                action(arg.clone())
            }),
            Rc::new(move || (unreachable)(Refusal::NotConnected)),
        );
    })
}

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

/// Turn one inline generation's result into the editor's state.
///
/// **Takes the runner's `Result`, not the raw process output**, because reading
/// the reply is no longer one thing: three harnesses answer on stdout and Codex
/// answers in a file, and deciding which belongs with the argv that chose it
/// (`ai::run_inline`) rather than here. What arrives is either the reply or a
/// message that already names the CLI that failed.
fn inline_outcome(reply: Result<String, String>, dialect: SqlDialect) -> InlineAiState {
    match reply {
        Ok(text) => {
            let sql = extract_sql(&text);
            if sql.trim().is_empty() {
                return InlineAiState::Failed("No SQL returned".to_string());
            }
            // Fences off, now prove it is SQL. The tool the model runs in can put
            // a line of its own on stdout, and Ctrl+K's output goes straight into
            // the editor — so anything that will not parse is refused rather than
            // offered. See `intel::sql_reply` for what it will and won't shave off.
            match schemaic_core::intel::sql_reply(&sql, dialect) {
                Some(sql) => InlineAiState::Ready(sql),
                None => InlineAiState::Failed("The model did not return SQL".to_string()),
            }
        }
        // One line: the bar this lands in is a single row, and a CLI's stderr can
        // run to a stack trace.
        Err(why) => InlineAiState::Failed(
            why.lines()
                .next()
                .filter(|l| !l.trim().is_empty())
                .unwrap_or("generation failed")
                .to_string(),
        ),
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

/// Map a resolved `Db`'s engine to the SQL dialect (for dialect-aware DDL).
///
/// The same `Engine::dialect()` [`dialect_for`] uses, which is exhaustive. This
/// was a two-engine `if Postgres { … } else { MySql }` that sorted SQLite onto
/// the MySQL side — the shape a third engine makes silently wrong.
fn dialect_of(db: &Db) -> SqlDialect {
    dialect_for(db.engine())
}

/// The DDL to put in an AI prompt, or `None` where the user's *Schema context*
/// says to send none.
///
/// One place, because it is the same question at every AI surface and three of
/// them already answer it: the chat panel, the MCP tools and Ctrl+K. Fill and
/// Seed were written after `render_inline_prompt`'s fix and never received it,
/// which made them the fourth surface — *Schema context: None* withheld the
/// schema tools and emptied the system prompt's outline while a right-click ▸
/// AI Fill Value shipped the whole `CREATE TABLE`, comments included.
fn ai_ddl_for(ddl: String, scope: schemaic_ui::SchemaScope) -> Option<String> {
    match scope {
        schemaic_ui::SchemaScope::None => None,
        schemaic_ui::SchemaScope::Active | schemaic_ui::SchemaScope::All => Some(ddl),
    }
}

/// A connection's AI data level, or the default when the connection is gone.
///
/// The same read `grid::ai_data_of` makes, on the app side, because the two
/// prompt callbacks need it and neither has a `GridState`.
fn ai_data_of_conn(
    connections: RwSignal<Vec<schemaic_core::connection::Connection>>,
    conn_id: u64,
) -> schemaic_core::connection::AiData {
    connections
        .with_untracked(|cs| cs.iter().find(|c| c.id == conn_id).and_then(|c| c.ai_data))
        .unwrap_or_default()
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

/// Fold one source's result into the import review list — the rows and the
/// entries it could not offer.
///
/// **The one way anything gets onto that list**: the paste field, the file
/// picker and the client scan all come through here, so the three cannot
/// disagree about ticking, about duplicate rows, or about a skipped entry
/// reported twice. Both rules themselves live in `conn_import`
/// (`merge_rows`/`merge_skipped`), where they are unit-tested; this is the
/// signal plumbing around them.
fn add_import_result(ui: ConnImportUi, scan: conn_import::ImportScan) {
    let mut merged = conn_import::Merged::default();
    ui.rows
        .update(|rows| merged = conn_import::merge_rows(rows, scan.found));
    // **`added` only.** `chosen` is additive here, so extending it with the
    // repeats re-ticked rows the user had deliberately cleared — and Import
    // then created the connections they had just refused. A repeat's tick
    // belongs to the user; see `conn_import::Merged`.
    ui.chosen.update(|c| c.extend(merged.added));
    // The entries the cap kept out are still counted — see
    // `conn_import::SKIPPED_CAP`. Both halves of the total move together, so the
    // sentence stays a claim about the file rather than about the list.
    let mut over_cap = 0usize;
    ui.skipped
        .update(|s| over_cap = conn_import::merge_skipped(s, scan.skipped));
    ui.skipped_hidden
        .update(|n| *n += scan.skipped_hidden + over_cap);
}

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

/// Smallest positive "Query N" number not present in `used` (a tab's display
/// number, its `label`). New tabs pick the lowest free number so closing and
/// opening keeps numbering compact instead of climbing forever — the display
/// number is decoupled from the ever-incrementing tab `id`.
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

/// May a landed **health check** write the connection status it found?
///
/// The health leg of the same question [`load_landing`] asks of a schema load,
/// and for the same reason: a ping is up to five seconds of waiting, and the
/// user can switch connection or press Retry again inside it. `started` is the
/// `(connection id, generation)` the check stamped itself with; `current` is
/// what those are now.
///
/// The connection id is the reported bug — the header's "Disconnected · Retry"
/// still on screen after switching *to* a healthy connection. Leaving a dead
/// server means the ping fired at it is still outstanding; it lands after the
/// switch has reset the status to `Unknown` and after the new connection's own
/// check has answered `Connected`, and repaints the banner over a server that
/// is answering. Nothing clears it but the next poll, which the same stale
/// result has just told to back off (`health::record` counts every check).
///
/// The generation is the case an id check alone misses: two checks of the *same*
/// connection, out of order. Retry against a host that comes back in between
/// lands `Connected` from the second and then the first's five-second-old
/// failure on top.
///
/// A dropped check writes nothing: not the status, not the failure count.
///
/// Its `with_conn` continuation is a separate question — see
/// [`check_continues`], which is deliberately *not* this.
fn check_landing(started: (u64, u64), current: (u64, u64)) -> bool {
    started == current
}

/// May a landed health check run the `with_conn` continuation it was carrying?
///
/// **Connection identity only, and not the generation** — which is the whole
/// distinction. [`check_landing`] governs a *write*, where a stale answer is
/// worse than none: an old failure repainting "Disconnected" over a server that
/// is answering is the bug it exists for. A continuation is not a write, it is
/// the user's action waiting on an answer, and dropping it doesn't leave the
/// screen stale — it makes the button they clicked do nothing at all, silently,
/// which is precisely what `with_conn` promises can't happen.
///
/// Gating both on the generation made that promise false on a timer. A blocked
/// action pings, and the ping takes up to five seconds against a dead host; the
/// health poll ticks (or the window regains focus and re-checks) inside that
/// window, stamps a newer generation, and the user's own check lands into a guard
/// that throws it away. No action, no error, no explanation.
///
/// A superseded result of the *same* connection is still an answer about the same
/// server, and a few seconds old — good enough to decide whether to proceed, and
/// far better than a control that does nothing. A **different** connection is not,
/// which is the case this still refuses: running an action gated on a server the
/// user has left, or reporting the old one unreachable in a modal over the new
/// one.
fn check_continues(started: (u64, u64), current: (u64, u64)) -> bool {
    started.0 == current.0
}

/// What a landed health check is allowed to do.
///
/// The two predicates above answer half a question each, and the *composition*
/// is the whole decision — which is why it lives here rather than inline at the
/// call site. A review found a `return` inserted between the two calls that
/// reproduced the bug [`check_continues`] exists to prevent, with both
/// predicates still tested and still green: the seam had no subject.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CheckOutcome {
    /// Repaint the header (and count the result toward the backoff)?
    write_status: bool,
    /// Answer the `with_conn` continuation waiting on this check, and with what.
    ///
    /// `None` means there is nobody to answer *about this server* — the user has
    /// switched connections. It never means "the answer wasn't good enough":
    /// dropping the reply is what makes a clicked button do nothing at all.
    answer: Option<bool>,
}

/// Decide what a health check that has just landed may do.
///
/// The two rules are independent, and the asymmetry between them is the point:
/// a superseded result may not *write* (an old failure repainting "Disconnected"
/// over a server that is answering), but it must still *answer*, including when
/// the answer is `false`. Refusing to answer a superseded failure is the same
/// dropped continuation under a different name — and since `with_conn` only
/// pings when the connection is already down, `!ok` is the common case there,
/// not the corner one.
///
/// The spurious-modal case that motivates suppressing a stale `false` is
/// answered by the *caller* instead: `with_conn` re-reads `conn_status` before
/// it refuses, so a newer check that landed `Connected` in the meantime runs the
/// action rather than raising "Not connected" over it.
fn check_outcome(started: (u64, u64), current: (u64, u64), ok: bool) -> CheckOutcome {
    CheckOutcome {
        write_status: check_landing(started, current),
        answer: check_continues(started, current).then_some(ok),
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

/// The nodes a landed load leaves behind: in `existing`, kept by no plan.
///
/// **The half [`plan_nodes`] does not answer, and the half that holds memory.**
/// A `ConnNode` owns a `RwSignal<SchemaState>` whose `Loaded` arm is an
/// `Arc<DbSchema>` — every table, column, index, key, view, check and trigger of
/// one database, the largest single value the app caches. On a *reload* the node
/// scope is deliberately reused, so that the surviving rows keep their schema up
/// while the re-introspection runs; a database dropped from another client
/// between two reloads therefore vanished from `db_nodes` with its signals still
/// installed in that surviving scope, unreachable and retained until the app
/// exited or a connection switch happened to replace the scope. A scratch
/// database per refresh retained one full model per refresh.
///
/// `load_schema`'s own comment named this leak and named the fix — a scope per
/// node — and the ids are the half of it that can be tested.
fn departed_nodes(existing: &[(usize, String)], plans: &[NodePlan]) -> Vec<usize> {
    existing
        .iter()
        .map(|(id, _)| *id)
        .filter(|id| !plans.contains(&NodePlan::Keep(*id)))
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
/// **`schemaic_db`'s mapping, not a second copy of it.** This decides the footer
/// pill and what Commit and Rollback may do; `Session`'s decides whether the next
/// statement issues a `BEGIN`. They are the two halves of one tab's transaction
/// model, and they were the same three-arm match written out twice — one pinned
/// by `session.rs`'s tests and one structurally unreachable from them, so a
/// fourth engine was one clean-compiling edit away from a pill that disagreed
/// with the session it describes.
///
/// (SQLite takes MySQL's arm and never reaches it: manual-transaction mode isn't
/// offered on a SQLite connection and `Session::open` refuses one. Of the two,
/// MySQL's is the safe default to answer with — Postgres' would report a
/// transaction as poisoned when there is no transaction at all. The reasoning
/// lives with the mapping now.)
fn tx_engine(db: &Db) -> TxEngine {
    schemaic_db::session::tx_engine_of(db.engine())
}

/// Which of our own query tabs holds the server session `server_id` on the
/// connection `killed_conn` — the decision behind both halves of *Kill session*.
///
/// `sessions` is `(tab id, that tab's pinned server id)`, `tabs` is
/// `(tab id, its connection id)`, and the answer is a tab id.
///
/// **A free function because the composition is where the bug was, and the
/// composition could not be tested.** `Connection::targets_same_server` is
/// exhaustively covered — a dozen assertions, including the "an edit in place is
/// still the same server" cases — and it was never wrong. What was wrong was the
/// lookup around it: it compared `conn_id`, and two Schemaic connections
/// routinely point at one server (`local (app)` and `local (root)`, or two
/// entries differing only in default database), so a true match was rejected and
/// the tab was left holding a dead socket with Commit and Rollback still
/// offered. That is the seam CLAUDE.md names — "a pure function's composition
/// with its caller" — and while this lived as an `Rc<dyn Fn>` bound inside
/// `app_view` it had no name anything could call, so the same substitution could
/// be made again with the suite green.
///
/// **A server id is only unique on its own server**, which is why `killed_conn`
/// is half the key and not a formality: MySQL thread ids and PostgreSQL backend
/// pids are small integers each server hands out from its own counter, so two
/// Manual tabs on two connections routinely hold the same one. Matching on the
/// id alone closed a transaction on a server nobody had touched.
///
/// Both callers consequential: `repair_killed_session` puts a tab's `TxState`
/// back together and reopens its session, and the kill confirm names the tab so
/// the modal does not describe the user's own uncommitted work as somebody
/// else's client.
///
/// Ties are resolved by the order `sessions` arrives in, which is a `HashMap`'s
/// — arbitrary, and the same as before this was extracted. Two tabs pinned to
/// one session on one server is not a state the app can reach.
fn owning_tab_of(
    sessions: &[(usize, Option<i64>)],
    tabs: &[(usize, u64)],
    connections: &[schemaic_core::connection::Connection],
    killed_conn: u64,
    server_id: i64,
) -> Option<usize> {
    let conn_of = |id: u64| connections.iter().find(|c| c.id == id);
    let killed_on = conn_of(killed_conn);
    sessions
        .iter()
        .filter(|(_, sid)| *sid == Some(server_id))
        .find_map(|(tab_id, _)| {
            let tab_conn = tabs.iter().find(|(id, _)| id == tab_id).map(|(_, c)| *c)?;
            if tab_conn == killed_conn {
                return Some(*tab_id);
            }
            // Not the same connection — but possibly the same *server*, which is
            // the whole point of this function.
            conn_of(tab_conn)?
                .targets_same_server(killed_on?)
                .then_some(*tab_id)
        })
}

fn app_view(handle: tokio::runtime::Handle, window: floem::window::WindowId) -> impl IntoView {
    let cx = Scope::current();

    // Persisted UI state. Loaded up here because the connection migration below
    // reads the *old* global AI flag out of it; tab restore further down needs it
    // too (`restore_tabs`).
    let ui_state = persist::load_ui_state();

    // Load saved connections. Secrets are hydrated from the OS keyring (and any
    // legacy plaintext migrated into it) by `secrets::load_connections`.
    //
    // **An empty list stays empty.** A first launch used to seed a "Local
    // MariaDB" pointing at 127.0.0.1 with this repo's development credentials and
    // save it immediately, so a fresh install opened onto a connection its user
    // never made and almost certainly couldn't reach — and no connections.json
    // could be deleted, since the next launch wrote it straight back. The header
    // offers a New connection button in that state now, which is the answer a
    // seed was standing in for.
    let mut cf = secrets::load_connections();
    // Backfill an identity colour for any connection saved before colours
    // existed, so every connection always has one. Colours stay distinct while
    // presets last; persist only if we changed something.
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
            // Before the window exists, so it goes to the same startup channel
            // the config-recovery modal drains rather than to a surface that is
            // not there yet.
            if let Some(notice) = secrets::save_connections(&cf) {
                persist::queue_notice(notice);
            }
        }
    }
    // Settle the AI data-access level for every connection saved before it
    // existed, from the global flag it replaces. Done once, at load, so nothing
    // downstream has to keep asking "and what did the old setting say?" — and so
    // an upgrade neither hands the assistant access the user never granted nor
    // silently withdraws the access they had.
    {
        // **Read from the file, not from the loaded struct.** `load_ui_state` is
        // best effort and defaults the old flag to `true`, so an *absent*
        // `ui_state.json` — a restored `connections.json`, a moved config
        // directory, deleted preferences — would read as "the assistant was
        // running queries" and widen every saved connection to `Full`, once and
        // for good. With nothing to migrate *from*, the ordinary default is the
        // honest answer: the user can still raise it, and is shown the level.
        let settled = match persist::legacy_ai_run_queries() {
            Some(flag) => schemaic_core::connection::migrated_ai_data(flag),
            None => schemaic_core::connection::AiData::default(),
        };
        let mut changed = false;
        for c in cf.connections.iter_mut() {
            if c.ai_data.is_none() {
                c.ai_data = Some(settled);
                changed = true;
            }
        }
        if changed {
            // Before the window exists, so it goes to the same startup channel
            // the config-recovery modal drains rather than to a surface that is
            // not there yet.
            if let Some(notice) = secrets::save_connections(&cf) {
                persist::queue_notice(notice);
            }
        }
    }
    let active_id = Connection::startup_active_id(cf.active, &cf.connections);
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
    // **Not on the first run.** floem's `create_effect` runs its body once
    // immediately, so this saved the value it had *just loaded* before the window
    // was drawn — and this is the only store in this opening that persists
    // through an effect rather than an explicit saver, so it was the only one
    // whose `.bak` was always exactly one launch old even in normal operation.
    // On a launch that loaded defaults, the write put an empty primary on disk
    // and the launch after that rotated the empty file over the last real copy.
    create_effect(move |prev: Option<()>| {
        let entries = search_history.get();
        if prev.is_some() {
            persist::save_json(
                "search_history.json",
                &schemaic_core::search_history::SearchHistoryFile { entries },
            );
        }
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
                        let conn = Connection::rebind_tab(s.conn_id, &cf.connections, active_id);
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
    // **Emptying the tree, in the one place that also lets go of the generation
    // behind it.** `nodes_scope` owns every node's `RwSignal<SchemaState>`, and
    // each of those holds an `Arc<DbSchema>` — every column, index, key, view,
    // check and trigger of every database on the connection. Clearing
    // `db_nodes` alone leaves that scope installed and `nodes_conn` still naming
    // the connection, so the *next* load of it takes the reuse path against an
    // empty node list: `kept_scope` is `Some`, the deferred `dispose()` is
    // skipped, and the whole set is rebuilt inside a scope that still owns the
    // previous one — unreachable, and never freed.
    //
    // The failed-connect arm in `load_schema` diagnosed exactly that and fixed
    // it for its own path; the other two clears reached the same state and did
    // not. A connection switch the user reverses before the first load lands
    // orphans one generation per A→B→A round trip (seconds wide over a tunnel),
    // and deleting the last connection orphans one outright. Three steps that
    // must not come apart is one closure, not a comment asking three callers to
    // remember — the rule `rearm_activity` follows for the same reason.
    //
    // **Deferred, like every other dispose of this scope**, so the tree rebuilds
    // off the now-empty node list before the signals it was reading are freed.
    let clear_schema_tree: Rc<dyn Fn()> = {
        let nodes_scope = nodes_scope.clone();
        let nodes_conn = nodes_conn.clone();
        Rc::new(move || {
            db_nodes.set(Vec::new());
            *nodes_conn.borrow_mut() = None;
            if let Some(old) = nodes_scope.borrow_mut().take() {
                exec_after(Duration::ZERO, move |_| old.dispose());
            }
        })
    };
    // Expanded tree nodes, keyed by connection — see `core::expanded` for why
    // the key has to carry one. **The third store to need this fix**, after
    // `db_hidden` and `schema::tab_target`'s remembered database, and the same
    // shape: every key here is name-only (`db:sys`), so expanding `sys` on one
    // MySQL-family connection left it expanded on all of them, built the second
    // one's whole table list, and with the size column on fired a stats query
    // against a database nobody had opened there.
    //
    // `expanded` is the set for the connection currently being *looked at* —
    // the question the tree asks — and `expanded_rules` is the persisted truth.
    // The effect below writes one back into the other, and `switch_conn` reloads
    // it; see both for the ordering that keeps them honest.
    //
    // The legacy list is cleared only once it has actually been read, for
    // `hidden_dbs`' reason immediately below.
    let mut pending_legacy_expanded = ui_state.expanded;
    let expanded_rules: RwSignal<Vec<schemaic_core::expanded::ExpandedRule>> = RwSignal::new({
        let mut rules = ui_state.expanded_rules;
        if rules.is_empty()
            && !pending_legacy_expanded.is_empty()
            && let Some(migrated) = schemaic_core::expanded::migrate_flat(
                &pending_legacy_expanded,
                &cf.connections.iter().map(|c| c.id).collect::<Vec<_>>(),
            )
        {
            rules = migrated;
            pending_legacy_expanded = Vec::new();
        }
        rules
    });
    let pending_legacy_expanded = Rc::new(pending_legacy_expanded);
    let expanded: RwSignal<HashSet<String>> = RwSignal::new(
        expanded_rules.with_untracked(|r| schemaic_core::expanded::keys_for(r, active_id)),
    );
    // **Writes through on every change, reading the active connection
    // untracked.** Untracked is the load-bearing half: tracking it would fire
    // this on a connection *switch* and store the outgoing connection's set
    // under the incoming one, before `switch_conn` had a chance to replace it.
    // Tracking only `expanded` means the order is always "the set changed, file
    // it under whoever is active" — and `switch_conn` sets `active_conn` first,
    // then `expanded`, so its own write lands in the right place.
    create_effect(move |_| {
        let keys = expanded.get();
        let conn = active_conn.get_untracked();
        expanded_rules.update(|r| schemaic_core::expanded::set_keys(r, conn, &keys));
    });
    // Hidden databases, keyed by connection — see `core::db_hidden` for why the
    // key has to carry one, and `migrate_flat` for what a file written before
    // this means. The rules are the persisted truth; `hidden_dbs` is the set for
    // the connection currently being *looked at*, which is the question every
    // consumer is asking, so it is derived rather than kept in step by hand.
    //
    // **The legacy list is only cleared once it has actually been read.** The
    // migration runs at most once and `save_ui` writes the flat field empty from
    // then on, so a launch whose `connections.json` did not load — or one where
    // the user has deleted their last connection — used to turn every previously
    // hidden database permanently visible: no connection ids, no rules, and the
    // list gone by the first save. `migrate_flat` answers `None` for "not yet"
    // now, and what it did not consume is carried back out to disk.
    let mut pending_legacy_hidden = ui_state.hidden_dbs;
    let hidden_db_rules: RwSignal<Vec<schemaic_core::db_hidden::DbHiddenRule>> = RwSignal::new({
        let mut rules = ui_state.hidden_db_rules;
        if rules.is_empty()
            && !pending_legacy_hidden.is_empty()
            && let Some(migrated) = schemaic_core::db_hidden::migrate_flat(
                &pending_legacy_hidden,
                &cf.connections.iter().map(|c| c.id).collect::<Vec<_>>(),
            )
        {
            rules = migrated;
            pending_legacy_hidden = Vec::new();
        }
        rules
    });
    let pending_legacy_hidden = Rc::new(pending_legacy_hidden);
    let hidden_dbs: floem::reactive::Memo<HashSet<String>> = create_memo(move |_| {
        hidden_db_rules.with(|rules| schemaic_core::db_hidden::names_for(rules, active_conn.get()))
    });
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
    // Which agent CLI drives the panel. An unrecognised persisted key falls back
    // to Claude *and says so* — `Harness::from_key` refuses to guess precisely so
    // this decision is made where there is a UI to report it.
    //
    // **The key the file names, kept.** The field's doc promises the
    // unrecognised value is not silently replaced with the default — and it was,
    // one save later: the persist site writes `ai_harness.get().key()`, which is
    // the fallback, so the name the user's file carried was overwritten by the
    // next thing that touched the settings. A build that later grows that
    // harness would then have nothing to restore. Held here and written back
    // until the user picks something themselves.
    let ai_harness_unknown: RwSignal<Option<String>> =
        RwSignal::new(match Harness::from_key(&ui_state.ai_harness) {
            Some(_) => None,
            None => {
                // Said out loud rather than swallowed. The substitution is
                // otherwise invisible: the panel would drive Claude while
                // `ui_state.json` names something else, and the only symptom is
                // an assistant behaving unlike the CLI the user believes they
                // picked.
                tracing::warn!(
                    harness = %ui_state.ai_harness,
                    "unknown AI harness in ui_state.json; falling back to Claude Code"
                );
                Some(ui_state.ai_harness.clone())
            }
        });
    let ai_harness =
        RwSignal::new(Harness::from_key(&ui_state.ai_harness).unwrap_or(Harness::Claude));
    let ai_cli_path = RwSignal::new(match ai_harness_unknown.get_untracked() {
        Some(_) => String::new(),
        None => ui_state.ai_cli_path.clone(),
    });
    // Verbatim from disk. No `from_cli` narrowing any more — the value the file
    // names is the value that runs, including one this build has never heard of.
    //
    // **Except when the harness beside it is one this build has never heard
    // of.** The harness-switch effect clears the model on a *switch*, and a
    // restore is not one — so an unknown harness paired with, say,
    // `opencode/claude-sonnet-5` started a Claude session with that model id and
    // died on an unknown model under "check your installation". The path goes
    // with it, for the reason `harness_switch` gives.
    let ai_model = RwSignal::new(match ai_harness_unknown.get_untracked() {
        Some(_) => String::new(),
        None => ui_state.ai_model.clone(),
    });
    let ai_effort = RwSignal::new(AiEffort::from_cli(&ui_state.ai_effort));
    let ai_instructions = RwSignal::new(ui_state.ai_instructions.clone());
    let ai_schema_scope = RwSignal::new(SchemaScope::from_key(&ui_state.ai_schema_scope));
    let ai_gutter = RwSignal::new(ui_state.ai_gutter);
    // The legacy global flag, carried verbatim from load to save. It is read once
    // (the connection migration above) and never again — see
    // `UiState::ai_run_queries` for why it is still written back at all.
    let legacy_ai_run_queries = ui_state.ai_run_queries;
    // Rows the grid has staged for the next question. Session-only by design:
    // an attachment is consent for one turn, not a standing setting.
    let ai_attachment = RwSignal::new(None::<schemaic_core::transcript::Attachment>);
    // Appearance (Settings → theme picker), restored from disk. Seed the live
    // theme registry from the persisted choice *before* any view builds, then
    // mirror the signals into it whenever the picker mutates them (live switch).
    let theme_settings_open = RwSignal::new(false);
    let help_open = RwSignal::new(false);
    let ui_theme = RwSignal::new(UiThemeKind::from_key(&ui_state.ui_theme));
    let editor_theme = RwSignal::new(EditorThemeKind::from_key(&ui_state.editor_theme));
    let ui_scale = RwSignal::new(UiScale::from_key(&ui_state.ui_scale));
    schemaic_ui::theme::init(
        ui_theme.get_untracked(),
        editor_theme.get_untracked(),
        ui_scale.get_untracked(),
    );
    create_effect(move |_| schemaic_ui::theme::set_ui(ui_theme.get()));
    create_effect(move |_| schemaic_ui::theme::set_editor(editor_theme.get()));
    create_effect(move |_| schemaic_ui::theme::set_ui_scale(ui_scale.get()));
    // Editor content settings (font / indentation) + query/behaviour settings.
    // Seed the global editor-config registry before the view builds, then mirror the
    // signals into it live (a change re-lays out the editor / re-applies indent).
    // **Healed here, not only where they are used.** `ui_state.json` carries
    // both as raw numbers under `#[serde(default)]`, which fills in an absent
    // field and validates nothing — so a hand-edited config could put a `0` font
    // size or a tab width of 99 straight into the editor. The setters clamped
    // what they *stored*, which left the signal, the Settings picker and the
    // file all still saying 99 while the editor indented by 8; `save_ui` writes
    // these signals back, so the clamp has to reach them or it never sticks.
    let editor_font = RwSignal::new(schemaic_ui::theme::clamped_editor_font(
        ui_state.editor_font_size,
    ));
    let tab_width = RwSignal::new(schemaic_ui::theme::clamped_tab_width(ui_state.tab_width));
    let soft_tabs = RwSignal::new(ui_state.soft_tabs);
    let word_wrap = RwSignal::new(ui_state.word_wrap);
    let row_limit = RwSignal::new(ui_state.row_limit);
    let statement_timeout = RwSignal::new(ui_state.statement_timeout_secs);
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
    // Bumped when a background probe fills the cache, so the settings modal's
    // constraint notice — which reads the cache and must never fill it, being on
    // the UI thread — knows to look again.
    let ai_probe_gen = RwSignal::new(0u64);
    // Probe what this CLI takes now, off-thread, so the first AI action doesn't
    // pay for it — see `agent_cli::probe`.
    //
    // **Warmed on the path the spawn will actually use**, which is
    // `harness_bin(h, &ai_cli_path)` — the same expression every AI action
    // resolves. Warming the *auto-detected* path instead keyed the cache
    // differently from every reader whenever the user set an AI CLI override, so
    // each of the four AI entry points paid a blocking, timeout-free `--help` on
    // the UI thread. The effect re-warms when the setting changes, for the same
    // reason — and now on the **harness** too, which is a second key into the
    // same cache and a second way to warm an entry nobody reads.
    create_effect(move |_| {
        let h = ai_harness.get();
        let path = ai_cli_path.get();
        // **The settings notice reads the cache and never fills it**, so this
        // has to tell it when an answer lands, or a harness switch leaves the
        // grade blank until something else re-renders the modal. A cold key is
        // exactly what switching harness produces — it *clears* `cli_path` —
        // which is why the memo and this thread used to start in the same
        // update pass and both miss.
        let bump = create_ext_action(Scope::current(), move |()| {
            ai_probe_gen.update(|n| *n += 1);
        });
        agent_cli::warm_probe_cache(h, harness_bin(h, &path), move || bump(()));
        // **One key now covers every AI entry point.** The one-shot generators
        // used to run Claude whatever was selected, so they read a second key —
        // `(Claude, <claude bin>)` — that this effect did not fill, and each
        // Ctrl+K paid a blocking, timeout-free `claude --help` on the Floem UI
        // thread. They build their own harness's argv now (`ai::inline_plan`),
        // which is the same key as the line above.
    });
    // Antigravity is the one harness Schemaic configures by writing into the
    // *user's* files, so a session that never ran its cleanup leaves an MCP
    // registration and a set of tool permissions behind. Neither expires, and a
    // standing grant nobody remembers making is exactly what must not outlive
    // the process that needed it — so any are removed at startup, off-thread
    // because it shells out. See `antigravity::sweep` for the one case this
    // cannot distinguish (a second running Schemaic).
    {
        let agy = detect_bin(Harness::Antigravity);
        std::thread::spawn(move || {
            antigravity::sweep(agy.as_deref());
            // OpenCode's roots are Schemaic's own rather than the user's, so
            // this is tidiness rather than a permission being withdrawn — but a
            // per-instance directory nothing collects is a directory per launch,
            // forever. Same thread, because both shell out or walk the disk and
            // neither is wanted on the UI thread.
            opencode::sweep();
        });
    }
    // **A path override belongs to the harness it was typed for.** `ai_cli_path`
    // is one field shared by every harness, so leaving it behind across a switch
    // points the new CLI's spawn at the old CLI's binary — pick Codex after
    // setting a Claude path and `harness_bin` resolves it, spawning *Claude*
    // with Codex's argv. It dies on the first unknown flag and reports it as an
    // installation problem, which is the one thing that is not wrong with it.
    //
    // **And so does the model id**, for the same reason and with a quieter
    // failure: `ai_model` is one field too, and no CLI's `--model` accepts
    // another CLI's ids — pick Codex while the field says `haiku` and the
    // session dies on an unknown model, having been configured by a value the
    // user last chose for a different program. Empty already means "the
    // harness's default" and omits the flag entirely, so clearing lands on the
    // one id every harness is guaranteed to take.
    //
    // **Effort is clamped rather than cleared**, because unlike the other two it
    // has no "the harness's default" value — the setting is a closed enum and
    // every level in it means something. `Extra` is Claude's `xhigh` and
    // Antigravity's flag stops at `high`, so the switch moves the selection to
    // the *nearest* level the new harness advertises — nearest, not highest;
    // taking the highest is what sent a default `Medium` to OpenCode's `Max`.
    // The argv is already
    // clamped by `Harness::effort_arg`, so this is about what the modal *shows*:
    // the closed dropdown renders the selected level unconditionally, and a box
    // reading "Extra" over a harness that neither offers nor sends it is the
    // stale caption this effect exists to prevent.
    //
    // Clearing falls back to auto-detect for the new harness, which is both
    // right and what the empty value already means. The effect's return value is
    // the previous harness: on the first run there is none, and clearing then
    // would discard the override restored from `ui_state.json` before the user
    // has touched anything.
    // What each harness held when it was last selected, so browsing the list and
    // coming back does not cost the user their path and model. Session-scoped on
    // purpose: it is a "you were just here" memory, not a fifth thing to
    // persist.
    let harness_fields: RwSignal<HashMap<Harness, schemaic_ui::HarnessFields>> =
        RwSignal::new(HashMap::new());
    create_effect(move |prev: Option<Harness>| {
        let now = ai_harness.get();
        // Filed under the harness being *left*, before anything is cleared.
        if let Some(p) = prev
            && p != now
        {
            // The user has chosen, so the file should start naming what runs.
            ai_harness_unknown.set(None);
            // **All three**, including the effort — the field that was left
            // out is the one a round trip through the dropdown lost.
            let held = schemaic_ui::HarnessFields {
                cli_path: ai_cli_path.get_untracked(),
                model: ai_model.get_untracked(),
                effort: ai_effort.get_untracked(),
            };
            harness_fields.update(|m| {
                m.insert(p, held);
            });
        }
        // The rule itself is `schemaic_ui::harness_switch`, pure and tested —
        // see its doc for what each field does and why the clamp is not gated on
        // a switch. Every bug in this decision's history sat here, in the
        // composition, rather than in any of the three answers.
        let plan = schemaic_ui::harness_switch(
            prev,
            now,
            ai_cli_path.get_untracked(),
            ai_model.get_untracked(),
            ai_effort.get_untracked(),
            harness_fields.with_untracked(|m| m.get(&now).cloned()),
        );
        ai_cli_path.set(plan.cli_path.unwrap_or_default());
        ai_model.set(plan.model.unwrap_or_default());
        if plan.effort != ai_effort.get_untracked() {
            ai_effort.set(plan.effort);
        }
        now
    });
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
    // The Import Connections modal, raised from the same list.
    let import_ui = ConnImportUi {
        open: RwSignal::new(false),
        scanning: RwSignal::new(false),
        scanned: RwSignal::new(false),
        rows: RwSignal::new(Vec::new()),
        chosen: RwSignal::new(std::collections::HashSet::new()),
        skipped: RwSignal::new(Vec::new()),
        skipped_hidden: RwSignal::new(0),
        paste: RwSignal::new(String::new()),
        paste_error: RwSignal::new(None),
        file_error: RwSignal::new(None),
        done: RwSignal::new(None),
    };
    let find_open = RwSignal::new(false);
    let find_query = RwSignal::new(String::new());
    let error_modal_open = RwSignal::new(false);
    let error_modal_text: RwSignal<Option<String>> = RwSignal::new(None);
    // Whether that override is a *statement* failure — see `error_modal_fixable`.
    let error_modal_fixable = RwSignal::new(false);
    let conn_status = RwSignal::new(ConnStatus::Unknown);
    // Consecutive failed health checks of the active connection, folded by every
    // check (polled or manual). Drives the health poll's backoff so a server
    // that's been down for a while isn't probed every 10s; reset on switch.
    let health_failures = RwSignal::new(0u32);
    // Bumped by every health check that actually pings, so a check that lands
    // after a newer one (or after a connection switch) can tell and drop its
    // result — see `check_landing`.
    let health_gen = RwSignal::new(0u64);
    // OS window focus, set from the workspace root. Starts `true`: the window is
    // focused on launch and winit only reports the *changes*.
    let window_focused = RwSignal::new(true);
    // Pending "you have an open transaction" question (see `TxPrompt`).
    let tx_prompt: RwSignal<Option<TxPrompt>> = RwSignal::new(None);
    // The shared "are you sure?" channel (see `Confirm`) — one modal for every
    // destructive action, rather than one modal each.
    let confirm: RwSignal<Option<Confirm>> = RwSignal::new(None);
    // Which snippet the snippet editor is open on — the modal itself lives in
    // `schemaic-ui`; this is only the signal that raises it.
    let snippet_edit_open: RwSignal<Option<u64>> = RwSignal::new(None);

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
    // Users and privileges browser. `users` is both the open flag and the server
    // being described, so a stale fetch can check it before writing — the same
    // shape `properties` has, and for the same reason.
    let users: RwSignal<Option<schemaic_ui::UsersTarget>> = RwSignal::new(None);
    let users_state: RwSignal<schemaic_ui::UsersState> =
        RwSignal::new(schemaic_ui::UsersState::Loading);
    let users_filter: RwSignal<String> = RwSignal::new(String::new());
    let users_selected: RwSignal<Option<schemaic_core::users::Principal>> = RwSignal::new(None);
    let users_grants: RwSignal<schemaic_ui::GrantsState> =
        RwSignal::new(schemaic_ui::GrantsState::Idle);
    let users_generation: RwSignal<u64> = RwSignal::new(0);
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
    //
    // **The caller says which of those two it is.** A finished turn is an
    // ordinary save and wants the `.bak` a crash would be recovered from; "New
    // chat" replaces a transcript with nothing, and the ordinary save left the
    // whole previous conversation — which keeps row values in the assistant's
    // own prose — in `chats.json.bak`.
    let persist_chat: Rc<dyn Fn(u64, persist::Saving)> =
        Rc::new(move |conn_id: u64, saving: persist::Saving| {
            saved_chats.update(|chats| {
                schemaic_core::chat::save(chats, conn_id, &ai_messages.get_untracked());
            });
            // `ChatFile::of` is what drops the tool results — query output, i.e.
            // the user's own rows — on the way to disk.
            let file = schemaic_core::chat::ChatFile::of(&saved_chats.get_untracked());
            match saving {
                persist::Saving::Erasing => persist::save_json_erasing("chats.json", &file),
                persist::Saving::Replacing => persist::save_json("chats.json", &file),
            }
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
                // **One batch, not N pushes.** The per-connection cap applied on
                // each push let a script longer than the cap evict the whole of
                // the connection's real history — and its own dispatched
                // statements with it — before anything ran; `finish_history`
                // then dropped the tail and left nothing at all. `push_batch`
                // defers the cap to `history::trim`, which runs once the
                // outcomes are known. See `history::push_batch`.
                let batch: Vec<_> = stmts
                    .iter()
                    .map(|sql| {
                        let run_id = run_ids.get() + 1;
                        run_ids.set(run_id);
                        ids.push(run_id);
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
                        }
                    })
                    .collect();
                let mut wrote = false;
                history_entries.update(|v| {
                    wrote = schemaic_core::history::push_batch(v, batch, dialect) > 0;
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
                // **The cap, now that the outcomes are known.** `push_batch`
                // deliberately leaves the connection over it at launch so a
                // script cannot evict history before anything has run; this is
                // the only moment anything can tell a statement that ran from
                // one that was never sent. Idempotent, so a batch that fitted
                // reports no change and costs no write.
                any |= schemaic_core::history::trim(v);
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
            // **Erasing.** The confirm behind this button reads "This can't be
            // undone", and the ordinary save left every statement it named in
            // `history.json.bak` until the next query run.
            persist::save_json_erasing(
                "history.json",
                &schemaic_core::history::HistoryFile {
                    entries: history_entries.get_untracked(),
                },
            );
        })
    };

    // Delete one history entry (the row's menu), persisting. The write is
    // skipped when nothing matched — a row already gone shouldn't cost a full
    // rewrite of the file, which is what `remove`'s bool is for.
    let remove_history: Rc<dyn Fn(schemaic_core::history::HistoryEntry)> = {
        Rc::new(move |entry: schemaic_core::history::HistoryEntry| {
            let mut hit = false;
            history_entries.update(|v| {
                hit = schemaic_core::history::remove(v, entry.conn_id, &entry.sql);
            });
            if hit {
                // Erasing: the point of this save is that the row is gone, and
                // the ordinary one would have left it in `history.json.bak`.
                persist::save_json_erasing(
                    "history.json",
                    &schemaic_core::history::HistoryFile {
                        entries: history_entries.get_untracked(),
                    },
                );
            }
        })
    };

    // ── The snippet library ─────────────────────────────────────────────────
    //
    // One persisted list across every connection; which of them a connection may
    // see is `snippet::grouped`'s decision, in the core with tests. Saving,
    // renaming, duplicating and deleting all write the whole file, as every
    // other small store here does.
    let snippets = RwSignal::new(
        persist::load_json::<schemaic_core::snippet::SnippetsFile>("snippets.json").snippets,
    );
    let save_snippets: Rc<dyn Fn()> = Rc::new(move || {
        persist::save_json(
            "snippets.json",
            &schemaic_core::snippet::SnippetsFile {
                snippets: snippets.get_untracked(),
            },
        );
    });
    // The active tab, for the actions that read or write one.
    let active_tab = move || {
        let id = active.get_untracked();
        tabs.with_untracked(|v| v.iter().find(|t| t.id == id).copied())
    };
    // The dialect of the **active connection** — what a newly saved snippet is
    // scoped to, and what the snippet library, the library panel, the history
    // panel and the palette all group and colour by.
    //
    // **A memo, so it can be read either way.** The library below has to *track*
    // it: its built-in pack is `snippet::builtins(dialect)`, so a memo that read
    // the dialect untracked recomputed only when `snippets` changed — and every
    // writer of that signal is a user action. Switching to a connection of
    // another engine therefore left the whole shipped pack of the *previous*
    // engine in place, which `snippet::applies` then filtered out entirely: the
    // panel, abbrev expansion and Find-Anywhere all lost the built-ins until the
    // next time a user snippet happened to be written.
    let conn_dialect_memo = create_memo(move |_| {
        let cid = active_conn.get();
        connections
            .with(|cs| {
                cs.iter()
                    .find(|c| c.id == cid)
                    .map(|c| SqlDialect::from_db_type(&c.db_type))
            })
            .unwrap_or_default()
    });
    // What the editor would contribute to a snippet: the selection if there is
    // one, else the whole buffer. The rule — including the whitespace-only
    // selection that used to disagree with `can_save_snippet` — is
    // `snippet::snippet_text`, with the test that composes it with the button's
    // own predicate. It was three rules in a closure inside `app_view`, where
    // nothing could call it.
    let editor_snippet_text = move || {
        let tab = active_tab()?;
        let sql = tab.query.get_untracked();
        schemaic_core::snippet::snippet_text(&sql, tab.selection.get_untracked())
    };
    // Wall-clock millis, for "last used". The same reading `record_history`
    // takes, and the same reason: it is when the user did the thing.
    let snippet_now = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    };
    // "This was used just now", for the row's `3d ago` and the recently-used
    // sort. The gate is `snippet::is_builtin` — a built-in has nothing to
    // record, and without it every insert of a shipped snippet rewrote
    // `snippets.json` for no change.
    let record_snippet_use: Rc<dyn Fn(u64)> = {
        let save_snippets = save_snippets.clone();
        Rc::new(move |id: u64| {
            if schemaic_core::snippet::is_builtin(id) {
                return;
            }
            snippets.update(|v| schemaic_core::snippet::touch(v, id, snippet_now()));
            (save_snippets)();
        })
    };
    let insert_snippet: Rc<dyn Fn(schemaic_core::snippet::Snippet)> = {
        let record_snippet_use = record_snippet_use.clone();
        Rc::new(move |snip: schemaic_core::snippet::Snippet| {
            let Some(tab) = active_tab() else {
                return;
            };
            // Through the tab's request signal, not `query`: the mounted editor
            // owns the document, and writing the signal behind it loses the
            // insertion's undo step. See `Tab::insert_req`.
            tab.insert_req.set(Some(snip.body.clone()));
            (record_snippet_use)(snip.id);
        })
    };
    let save_snippet_current: Rc<dyn Fn() -> Option<u64>> = {
        let save_snippets = save_snippets.clone();
        Rc::new(move || {
            let body = editor_snippet_text()?;
            let id = snippets.with_untracked(|v| schemaic_core::snippet::next_id(v));
            let name = active_tab().map_or_else(|| "Snippet".to_string(), |t| t.title());
            // The scope and the used-now stamp are `new_saved`'s, in core with
            // the test that composes them with the panel's grouping and sort —
            // both were a struct literal here, and both shipped wrong once.
            snippets.update(|v| {
                v.push(schemaic_core::snippet::new_saved(
                    id,
                    name,
                    body,
                    active_conn.get_untracked(),
                    snippet_now(),
                ))
            });
            (save_snippets)();
            Some(id)
        })
    };
    // What every snippet surface reads: the user's list plus the built-in pack
    // for the active connection's engine.
    //
    // **The merge happens exactly once, here.** The panel, the completion popup
    // and the palette all read this memo, so none of them can be looking at a
    // library the others aren't — and the *persisted* signal stays the user's
    // alone, which is what keeps the pack in code where a later release can fix
    // one, instead of copied into everybody's `snippets.json`.
    let snippet_library = create_memo(move |_| {
        // **Tracked.** The pack is per engine, so this memo's dependency set has
        // to include the engine — see `conn_dialect_memo`.
        let dialect = conn_dialect_memo.get();
        snippets.with(|user| schemaic_core::snippet::library(user, dialect))
    });
    // Tracked, so the `+` un-dims as soon as the tab has text. It asks the same
    // question `editor_snippet_text` answers — is there anything to save — one
    // reactively and one at the moment of the click.
    let can_save_snippet = create_memo(move |_| {
        let id = active.get();
        tabs.with(|v| {
            v.iter()
                .find(|t| t.id == id)
                .is_some_and(|t| schemaic_core::snippet::can_save(&t.query.get()))
        })
    });
    let rename_snippet: Rc<dyn Fn(u64, String)> = {
        let save_snippets = save_snippets.clone();
        Rc::new(move |id: u64, name: String| {
            snippets.update(|v| {
                if let Some(s) = v.iter_mut().find(|s| s.id == id) {
                    s.name = name.clone();
                }
            });
            (save_snippets)();
        })
    };
    let set_snippet_abbrev: Rc<dyn Fn(u64, Option<String>)> = {
        let save_snippets = save_snippets.clone();
        Rc::new(move |id: u64, abbrev: Option<String>| {
            snippets.update(|v| {
                if let Some(s) = v.iter_mut().find(|s| s.id == id) {
                    s.abbrev = abbrev.clone().filter(|a| !a.trim().is_empty());
                }
            });
            (save_snippets)();
        })
    };
    let set_snippet_body: Rc<dyn Fn(u64, String)> = {
        let save_snippets = save_snippets.clone();
        Rc::new(move |id: u64, body: String| {
            snippets.update(|v| {
                if let Some(s) = v.iter_mut().find(|s| s.id == id) {
                    s.body = body.clone();
                }
            });
            (save_snippets)();
        })
    };
    let set_snippet_scope: Rc<dyn Fn(u64, schemaic_core::snippet::Scope)> = {
        let save_snippets = save_snippets.clone();
        Rc::new(move |id: u64, scope: schemaic_core::snippet::Scope| {
            snippets.update(|v| {
                if let Some(s) = v.iter_mut().find(|s| s.id == id) {
                    s.scope = scope.clone();
                }
            });
            (save_snippets)();
        })
    };
    let duplicate_snippet: Rc<dyn Fn(u64)> = {
        let save_snippets = save_snippets.clone();
        Rc::new(move |id: u64| {
            // Looked up in the **merged** library: Duplicate exists mainly to
            // get an editable copy of a built-in, and a built-in is not in the
            // user's list to be found there.
            let Some(src) =
                snippet_library.with_untracked(|v| v.iter().find(|s| s.id == id).cloned())
            else {
                return;
            };
            let new_id = snippets.with_untracked(|v| schemaic_core::snippet::next_id(v));
            // The five things the copy does and does not inherit are
            // `snippet::duplicate`'s, with the tests: they were a struct literal
            // here, where nothing could call them.
            snippets.update(|v| v.push(schemaic_core::snippet::duplicate(&src, new_id)));
            (save_snippets)();
        })
    };
    let remove_snippet: Rc<dyn Fn(u64)> = {
        let save_snippets = save_snippets.clone();
        Rc::new(move |id: u64| {
            let Some(snip) = snippets.with_untracked(|v| v.iter().find(|s| s.id == id).cloned())
            else {
                return;
            };
            let save_snippets = save_snippets.clone();
            confirm.set(Some(Confirm {
                title: "Delete snippet".to_string(),
                message: format!("Delete “{}”? This can't be undone.", snip.name),
                resolve: Rc::new(move |yes| {
                    if yes {
                        snippets.update(|v| schemaic_core::snippet::remove(v, id));
                        (save_snippets)();
                    }
                }),
            }));
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
            let view_err = tab.view_err;
            // Which panel this run reports into. A view re-run keeps the panel it
            // is re-reading (its table stays on screen throughout); a manual run
            // opens a fresh one, which replaces the unpinned panels and leaves
            // every pinned result where it is.
            let panel = if is_view {
                // **A kept result refuses a re-read.** A view re-run replaces the
                // panel it re-reads, which on a pinned one is the snapshot the pin
                // was for. The grid already withholds both offers that get here
                // (the filter row and the capped notice's "read all rows"); this
                // is the same rule at the funnel, where a stale affordance or a
                // later caller cannot get round it.
                if tab.shown_frozen() {
                    return;
                }
                tab.shown_panel_id()
            } else {
                tab.begin_run(std::slice::from_ref(&sql)).first().copied()
            };
            // Report a failure into the panel this run owns — the fresh one for a
            // manual run, and for a view re-run the bottom bar instead, which is
            // where a filter's error goes rather than over the table it failed to
            // replace.
            let fail = move |msg: String| match (is_view, panel) {
                (true, _) => view_err.set(Some(msg)),
                (false, Some(id)) => tab.set_panel_state(id, QueryState::Failed(msg)),
                (false, None) => {}
            };
            // Resolve this tab's own connection (not necessarily the active one).
            let db = match db_for(tab.conn_id.get_untracked()) {
                Ok(db) => db,
                Err(e) => {
                    fail(e);
                    return;
                }
            };
            // In Manual mode the statement runs on the tab's pinned connection,
            // inside the transaction, instead of on a fresh one.
            let session = match session_for(&tab) {
                Ok(s) => s,
                Err(e) => {
                    fail(e);
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
            // **Whatever supersedes a view re-run clears its flag**, because the
            // superseded run returns at the generation check below and never
            // reaches its own clear. Written beside the cancel that does the
            // superseding rather than in the `else` branch further down, so a run
            // that is *not* a view re-run cannot be added later without it: the
            // flag would then stay set for ever and every later result's
            // read-more offer would open permanently disabled. A view re-run sets
            // it straight back a few lines below.
            tab.view_busy.set(false);
            let token = CancellationToken::new();
            let generation = run_gen.get() + 1;
            run_gen.set(generation);
            tokens.borrow_mut().insert(id, (generation, token.clone()));
            if is_view {
                // Keep the current table on screen during the re-run; a fresh attempt
                // clears any prior filter error.
                view_err.set(None);
                // **Nothing else on the panel says this is happening.** The table
                // the re-run replaces stays on screen and the grid goes on looking
                // idle, so the affordance that started it — the capped notice's
                // "read all rows" — is still sitting there inviting a second full
                // read of a large table. Set here, on the last line before the
                // spawn, so every early return above it leaves the flag alone.
                tab.view_busy.set(true);
            }
            // A manual run's panel is already `Running` — `begin_run` opened it
            // above, before any of the early returns, so the strip shows the
            // question being asked whatever happens to it.

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
                        // Cleared for every arm, and **after** the supersede check
                        // above rather than before it: a run that has been replaced
                        // is not the one the flag is about any more, and clearing it
                        // here would say "idle" over the top of the run that
                        // replaced it. Cancelled counts as landing — nothing else
                        // is coming, and a link left disabled for ever is worse
                        // than one that can be clicked twice.
                        tab.view_busy.set(false);
                        match state {
                            // Success → swap in the filtered result, then bump the load
                            // nonce so the grid rebuilds despite Loaded→Loaded. Order
                            // matters: the rebuild reads `results` untracked, so the new
                            // Arc must already be in place before the nonce changes.
                            QueryState::Loaded(_) => {
                                if let Some(id) = panel {
                                    tab.set_panel_state(id, state);
                                    // The panel's rows are this statement's now,
                                    // not the one it was opened with — see
                                    // `Tab::set_panel_sql`.
                                    tab.set_panel_sql(id, &tx_sql);
                                    // **This panel's nonce, not the tab's.** One
                                    // nonce on the tab rebuilt whichever panel
                                    // was *shown* — losing that result's scroll
                                    // and selection for a re-run that landed
                                    // somewhere else. See `ResultPanel::load_gen`.
                                    tab.bump_panel_load(id);
                                }
                                view_err.set(None);
                            }
                            // Error → keep the current table, show the message in the bar.
                            QueryState::Failed(m) => view_err.set(Some(m)),
                            // Cancelled/superseded → leave the table + error untouched.
                            _ => {}
                        }
                    } else if let Some(id) = panel {
                        // The panel may have been closed while the statement ran,
                        // in which case `set_panel_state` finds nothing and the
                        // result lands nowhere — deliberately the same answer the
                        // generation check above gives a superseded run.
                        tab.set_panel_state(id, state);
                    }
                },
            );
            // Read the row cap on the UI thread (signals are single-threaded).
            //
            // The tab's own override wins where the capped notice set one. It is
            // read here, per run, for the same reason the global cap is: there is
            // no fetch mode to switch, only a bigger ceiling on the next read.
            let cap = tab
                .row_cap_override
                .get_untracked()
                .unwrap_or_else(|| row_limit.get_untracked());
            let timeout_secs = statement_timeout.get_untracked();
            handle.spawn(async move {
                // Wall-clock, and around everything: connecting, the statement,
                // and pulling the rows back are all time the user waited — which
                // is also what the timeout below bounds, deliberately: a
                // connection that never completes is as much a hang as a query
                // that never returns.
                let started = std::time::Instant::now();
                let watchdog = RunTimeout::arm(&token, timeout_secs);
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
                let timed_out = watchdog.fired();
                drop(watchdog);
                // What the error text cannot say: on MySQL a DDL statement
                // commits the open transaction before it runs, so a rejected or
                // killed one has spent it — and nothing else on screen says so.
                // `None` (no session) can't have had a transaction to spend.
                let note = |m: String| match stmt {
                    Some(o) => tx::failed_message(&m, o),
                    None => m,
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
                    // A statement that never left the client is neither a
                    // timed-out query nor a killed one, and saying either put a
                    // cause on screen that the run cannot support. This arm goes
                    // first for that reason.
                    Err(DbError::Cancelled) if stmt == Some(StmtOutcome::NotSent) => {
                        tracing::info!("run cancelled before the statement was sent");
                        QueryState::Failed(tx::not_sent_message(timed_out))
                    }
                    // A timeout and the Cancel button arrive as the same error,
                    // so the watchdog's flag is the only thing that can tell the
                    // user which of the two stopped their query.
                    Err(DbError::Cancelled) if tx::timeout_reached(stmt, timed_out) => {
                        QueryState::Failed(note(timeout_message(timeout_secs)))
                    }
                    // **A Stop is not a smaller timeout.** MySQL commits the
                    // open transaction before it runs a DDL statement, so
                    // cancelling a slow `ALTER` inside a Manual one makes
                    // everything already in it permanent — and the arm above,
                    // which discloses exactly that, is reached only when the
                    // *clock* stopped the run. See `tx::cancelled_message`.
                    Err(DbError::Cancelled) => match tx::cancelled_message(stmt) {
                        Some(m) => {
                            tracing::info!("query cancelled after an implicit commit");
                            QueryState::Failed(m)
                        }
                        None => {
                            tracing::info!("query cancelled");
                            QueryState::Cancelled
                        }
                    },
                    Err(e) => {
                        tracing::error!("query failed: {e}");
                        QueryState::Failed(note(e.to_string()))
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
                // One statement, so the grid's filter row and sort rebuild
                // from it — see `Tab::start_manual_run`, which `run_all` calls
                // with `None` for the same reason.
                tab.start_manual_run(Some(&sql));
            }
            core(sql, false);
        })
    };

    // A filter/sort re-run: keeps the current table until the filtered result lands
    // (or an error shows in the grid's bottom bar), without recording history or
    // disturbing `base_sql`/`grid_query` (the grid owns those).
    // **The refusal arrives with the argument.** `RerunRequest` can only be
    // minted through `sql::rerunnable_for_export`, so this reaches
    // `run_query_core` past `run_verdict` by design rather than by omission —
    // through a gate strictly stronger than it, with no `Confirm` arm. It used
    // to take a bare `String` and rest on whatever each caller happened to
    // check.
    let apply_view: Rc<dyn Fn(schemaic_ui::RerunRequest)> = {
        let core = run_query_core.clone();
        Rc::new(move |req: schemaic_ui::RerunRequest| core(req.into_sql(), true))
    };

    // ── Run EXPLAIN for the query-plan modal (targets the active tab's db) ──
    let plan_token: Rc<RefCell<Option<CancellationToken>>> = Rc::new(RefCell::new(None));
    // **Which run the modal is showing.** Cancelling the old token is not enough
    // to keep a superseded run off the screen: a timed-out EXPLAIN ANALYZE
    // returns *late* — its cancel path opens a second connection to `KILL` the
    // query — and the arm that reports the timeout is not the `Cancelled` arm
    // that returns, so it painted its message over a run the user had already
    // started. Bumped per launch and checked where the result lands, which is on
    // the UI thread and therefore the one place that can read it.
    let plan_seq: Rc<Cell<u64>> = Rc::new(Cell::new(0));
    let run_plan: Rc<dyn Fn(String, bool)> = {
        let handle = handle.clone();
        let db_for = db_for.clone();
        let plan_token = plan_token.clone();
        let plan_seq = plan_seq.clone();
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

            let seq = plan_seq.get().wrapping_add(1);
            plan_seq.set(seq);
            // Every arm goes through this, not just the timeout one: a late
            // failure of any kind belongs to the run that produced it.
            let send = {
                let plan_seq = plan_seq.clone();
                create_ext_action(cx, move |(n, st): (u64, PlanState)| {
                    if n == plan_seq.get() {
                        plan_state.set(st);
                    }
                })
            };
            // `analyze` *executes* the statement to measure it, so an EXPLAIN
            // ANALYZE is exactly as runaway-capable as the query itself. Plain
            // EXPLAIN only plans and is bounded too — it costs nothing, and an
            // introspection query that hangs is still a hang.
            let timeout_secs = statement_timeout.get_untracked();
            handle.spawn(async move {
                let watchdog = RunTimeout::arm(&token, timeout_secs);
                let res = db.explain(database.as_deref(), &sql, analyze, token).await;
                let timed_out = watchdog.fired();
                drop(watchdog);
                let st = match res {
                    Ok(rs) => PlanState::Loaded(schemaic_core::plan::QueryPlan::from_result(&rs)),
                    // A timeout is not a supersede: nothing is coming to replace
                    // this state, so returning would leave the modal spinning on
                    // `Running` for ever.
                    Err(DbError::Cancelled) if timed_out => {
                        PlanState::Failed(timeout_message(timeout_secs))
                    }
                    Err(DbError::Cancelled) => return, // superseded — leave state alone
                    Err(e) => PlanState::Failed(e.to_string()),
                };
                send((seq, st));
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
                // **Once, here, before the first fetch** — see
                // `MonitorCtx::order_by`. `source` is moved into `target` below,
                // so this reads it while it is still in hand.
                order_by: monitor_order_key(db_nodes, &source),
                target: (conn_id, source),
                dialect: connections
                    .with_untracked(|cs| {
                        cs.iter()
                            .find(|c| c.id == conn_id)
                            .map(|c| SqlDialect::from_db_type(&c.db_type))
                    })
                    .unwrap_or_default(),
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
            // One Running panel per statement, replacing the unpinned ones, and
            // the first of them selected — `begin_run` is the same call a single
            // run makes, which is what keeps a batch and a single run one thing.
            // Before the two resolutions below, so a batch that never dispatches
            // still reports into its own panels.
            // **A batch has no single base**, and this is a fresh manual run
            // like any other: without it, the previous single run's statement
            // stayed as `base_sql` and a batch panel's filter row rebuilt
            // *that* statement into *this* panel. See `Tab::start_manual_run`.
            tab.start_manual_run(None);
            let n = stmts.len();
            let panels = tab.begin_run(&stmts);
            // A batch that can't reach its connection **stops at its first
            // failure** like any other, so the first statement carries the
            // message and the rest are cancelled — the same shape the run's own
            // early stop produces, rather than sixty chips repeating one error.
            let fail_batch = {
                let panels = panels.clone();
                move |msg: String| {
                    // One `update`, as above: sixty chips must not cost sixty
                    // rebuilds of the strip.
                    tab.set_panel_states(panels.iter().enumerate().map(|(i, id)| {
                        (
                            *id,
                            if i == 0 {
                                QueryState::Failed(msg.clone())
                            } else {
                                QueryState::Cancelled
                            },
                        )
                    }));
                }
            };
            let db = match db_for(tab.conn_id.get_untracked()) {
                Ok(db) => db,
                Err(e) => {
                    fail_batch(e);
                    return;
                }
            };
            let session = match session_for(&tab) {
                Ok(s) => s,
                Err(e) => {
                    fail_batch(e);
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
            // A batch supersedes a view re-run too — see the same line in
            // `run_query_core`.
            tab.view_busy.set(false);
            let token = CancellationToken::new();
            let generation = run_gen.get() + 1;
            run_gen.set(generation);
            tokens.borrow_mut().insert(id, (generation, token.clone()));

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
                    // By id, statement for statement: the strip may have been
                    // pinned, reordered or partly closed while the batch ran, and
                    // a positional write would then land a statement's result on
                    // somebody else's panel.
                    //
                    // **In one `update`.** Writing them one at a time notified
                    // the strip, the body's key memo, every chip and the error
                    // bar once per statement — 0.67 ms to 263 ms at 400
                    // statements, measured. See `Tab::set_panel_states`, and the
                    // note there about `reactive::batch` making it worse.
                    tab.set_panel_states(panels.iter().copied().zip(states));
                },
            );
            let cap = row_limit.get_untracked();
            let timeout_secs = statement_timeout.get_untracked();
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
                            // **Per statement, not once above the loop.** It is
                            // a no-op while the session's flag says a
                            // transaction is open, so the ordinary script pays
                            // nothing — but a `COMMIT` in the middle of the
                            // script, or a MySQL implicit-commit DDL, clears
                            // that flag, and every statement after it then ran
                            // **auto-committed** while the pill still counted
                            // and Rollback was a successful no-op over data
                            // already permanent. `6d95b86` fixed the predicate
                            // half of this (`tx_after`) and left the caller
                            // half here.
                            if let Err(e) = s.ensure_tx().await {
                                states[i] = QueryState::Failed(e.to_string());
                                took[i] = clock.elapsed().as_millis() as u64;
                                stopped = true;
                                continue;
                            }
                            // Per statement, so a long script isn't bounded as
                            // one long statement. Dropped before the next arms.
                            let watchdog = RunTimeout::arm(&token, timeout_secs);
                            let out = s.fetch_query(sql, cap, token.clone()).await;
                            let timed_out = watchdog.fired();
                            drop(watchdog);
                            took[i] = clock.elapsed().as_millis() as u64;
                            clock = std::time::Instant::now();
                            outcomes[i] = Some(out.stmt);
                            // Same disclosure as the single-run path, and it
                            // matters more here: the statements after this one
                            // are skipped, so the pill is the only place the
                            // spent transaction would otherwise show.
                            states[i] = match out.result {
                                Ok(rs) => QueryState::Loaded(Arc::new(rs)),
                                Err(DbError::Cancelled) if out.stmt == StmtOutcome::NotSent => {
                                    stopped = true;
                                    QueryState::Failed(tx::not_sent_message(timed_out))
                                }
                                Err(DbError::Cancelled)
                                    if tx::timeout_reached(Some(out.stmt), timed_out) =>
                                {
                                    stopped = true;
                                    QueryState::Failed(tx::failed_message(
                                        &timeout_message(timeout_secs),
                                        out.stmt,
                                    ))
                                }
                                // The same pair as `run_query_core`'s, and the
                                // same hole: see `tx::cancelled_message`.
                                Err(DbError::Cancelled) => {
                                    stopped = true;
                                    match tx::cancelled_message(Some(out.stmt)) {
                                        Some(m) => QueryState::Failed(m),
                                        None => QueryState::Cancelled,
                                    }
                                }
                                Err(e) => {
                                    stopped = true;
                                    QueryState::Failed(tx::failed_message(&e.to_string(), out.stmt))
                                }
                            };
                        }
                    }
                    None => {
                        // The callback fires as each statement lands, so the gap
                        // since the previous one *is* that statement's wall-clock
                        // — the batch runs them back to back on one connection.
                        // One watchdog at a time, re-armed as each statement
                        // lands — the callback is the only per-statement seam
                        // this path has. It is dropped (and so disarmed) when
                        // the closure is, which is when `run_batch` returns.
                        let mut watchdog = RunTimeout::arm(&token, timeout_secs);
                        db.run_batch(database.as_deref(), &stmts, cap, token.clone(), |i, res| {
                            let timed_out = watchdog.fired();
                            watchdog = RunTimeout::arm(&token, timeout_secs);
                            took[i] = clock.elapsed().as_millis() as u64;
                            clock = std::time::Instant::now();
                            states[i] = match res {
                                Ok(rs) => QueryState::Loaded(Arc::new(rs)),
                                Err(DbError::Cancelled) if timed_out => {
                                    QueryState::Failed(timeout_message(timeout_secs))
                                }
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
                            // A workbook has nothing to sniff — no delimiter and
                            // no quote — and its head is deflated ZIP bytes, so
                            // running the sniffer over it would let a compressed
                            // stream's byte frequencies decide `has_header`.
                            // The defaults (header on, first sheet) are the
                            // right opening answer, and the user can change both.
                            None if req.format == schemaic_core::import::ImportFormat::Xlsx => {
                                schemaic_core::import::ReadConfig::default()
                            }
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
                        // Best-effort: a size we can't read just means no
                        // large-file warning, never a failed probe.
                        let file_bytes = std::fs::metadata(&req.path).map(|m| m.len()).unwrap_or(0);
                        // **Before the read, not after it.** A workbook has to
                        // be read whole before its first row can be shown (a
                        // ZIP's directory is at the end), so unlike CSV and
                        // JSON — both bounded at `SAMPLE_MAX_BYTES` — there is
                        // no cheap look at a big one. Asking the file's *size*
                        // costs nothing and is the only way the refusal can
                        // come before the thing it refuses.
                        if let Some(too_big) =
                            schemaic_core::import::xlsx_size_refusal(req.format, file_bytes)
                        {
                            return Err(too_big);
                        }
                        let f = std::fs::File::open(&req.path).map_err(|e| e.to_string())?;
                        // One parse of the workbook for both the preview and the
                        // sheet list: they come off the same open, so a probe
                        // does not pay for the file twice.
                        let (sample, sheets) =
                            if req.format == schemaic_core::import::ImportFormat::Xlsx {
                                schemaic_core::import::read_workbook_sample(
                                    std::io::BufReader::new(f),
                                    &cfg,
                                    SAMPLE_ROWS,
                                )
                                .map_err(|e| e.to_string())?
                            } else {
                                let sample = schemaic_core::import::read_sample(
                                    std::io::BufReader::new(f),
                                    req.format,
                                    &cfg,
                                    SAMPLE_ROWS,
                                )
                                .map_err(|e| e.to_string())?;
                                (sample, Vec::new())
                            };
                        Ok(schemaic_ui::ImportProbeResult {
                            cfg,
                            sample,
                            file_bytes,
                            sheets,
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
                    //
                    // **Building it is off the runtime, like pass 1 above.**
                    // Constructing the iterator opens the file, and for an
                    // `.xlsx` that means reading the whole archive before the
                    // first row exists — blocking work that was running on a
                    // tokio worker while the validate pass thirty lines up took
                    // `spawn_blocking` for exactly the same read.
                    let built = {
                        let (path, format, cfg, table, mapping) = (
                            req.path.clone(),
                            req.format,
                            req.cfg.clone(),
                            req.target.table.clone(),
                            req.mapping.clone(),
                        );
                        tokio::task::spawn_blocking(move || {
                            let f = open(&path)?;
                            schemaic_core::import::row_iter(
                                f, format, &cfg, &table, &mapping, dialect,
                            )
                            .map_err(|e| e.to_string())
                        })
                        .await
                    };
                    let mut rows = match built {
                        Ok(Ok(it)) => it,
                        Ok(Err(e)) => return report(schemaic_ui::ImportOutcome::Failed(e)),
                        Err(e) => {
                            return report(schemaic_ui::ImportOutcome::Failed(e.to_string()));
                        }
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
    /// Rows per block when streaming a table to a file — the **upper** bound of
    /// two, the other being bytes.
    ///
    /// Ten thousand is a compromise the two ends pull on: large enough that the
    /// per-block cost — a `ResultSet`, one allocation per column, a channel hop —
    /// is paid once per ten thousand rows instead of per row, and small enough
    /// that an ordinary result's block is a modest allocation.
    ///
    /// **It is not what keeps a wide table's block small**, and it used to claim
    /// it was: a block is `rows × the row width`, nothing bounds a row's width,
    /// and four blocks are in flight at once — so a table of 1 MB documents put
    /// tens of gigabytes through a figure whose doc promised megabytes. The row
    /// loop also flushes on `schemaic_db`'s byte budget, which is what makes the
    /// promise true for any row width; this figure decides only how often a
    /// *narrow* table flushes.
    ///
    /// Nothing downstream depends on either figure: the renderers take whatever
    /// blocks they are given, which is what `a_chunked_export_matches_the_same_
    /// rows_in_one_go` pins.
    const EXPORT_CHUNK_ROWS: usize = 10_000;

    // The running streamed export's token, in the shape `import_token` above
    // uses and for the same reasons — including the one about clearing the slot
    // when a run reports, so a later Cancel can't "cancel" an export that already
    // finished.
    let export_token: Rc<RefCell<Option<CancellationToken>>> = Rc::new(RefCell::new(None));

    // The streamed export's progress, reported from the **writer**, one message
    // per block. A channel rather than a callback for the reason the dump's and
    // the AI stream's are: `create_ext_action` is one-shot and has to be built on
    // the UI thread, and this fires many times from a `spawn_blocking` worker.
    //
    // Counted where the rows are *written* rather than where they are read: the
    // channel between the two is bounded, so a reader-side count would run two
    // blocks ahead of the file and report rows that are not in it yet — and on a
    // cancel it would be two blocks wrong about what survived.
    // Created here and handed to `Ui` below, the way `dump_progress` is: the
    // signal has to exist before the closure that feeds it, and the bundle it
    // belongs to is built at the end.
    let (export_tx, export_rx) = crossbeam_channel::unbounded::<u64>();
    let export_stream = create_signal_from_channel(export_rx);
    let export_progress: RwSignal<Option<u64>> = RwSignal::new(None);
    create_effect(move |_| {
        if let Some(rows) = export_stream.get() {
            export_progress.set(Some(rows));
        }
    });

    let export_file: schemaic_ui::ExportFn = {
        let handle = handle.clone();
        let db_for = db_for.clone();
        let export_token = export_token.clone();
        let export_tx = export_tx.clone();
        Rc::new(
            move |req: schemaic_ui::ExportRequest, done: schemaic_ui::ExportDoneFn| {
                use schemaic_ui::{ExportOutcome, ExportScope};

                let (path, format, dialect) = (req.path.clone(), req.format, req.dialect);
                // Owned, because it crosses to a worker thread; borrowed back
                // as the `(&str, Option<&str>, &str)` the renderer wants at the
                // point of use.
                let source = req
                    .source
                    .as_ref()
                    .map(|s| (s.database.clone(), s.schema.clone(), s.table.clone()));
                // **The destination is not opened until the export has
                // succeeded.** `File::create` truncates, so opening it first meant
                // a stream that died ten minutes in had already destroyed whatever
                // the user was overwriting — and `f115e51` turned that window from
                // milliseconds into minutes. The rows go to a `.part` sibling and
                // the sibling is renamed over the target at the end, which is the
                // dance `persist` already does for every config file, and which is
                // atomic *because* it is a sibling: a rename inside one directory
                // never crosses a filesystem.
                //
                // On every failure path the target is untouched and the fragment is
                // left in the sibling rather than swept away — see
                // `export::export_failure_note`, which names it.
                // `dump::part_of`, not a second spelling of `.part`: the
                // suffix belongs to `export::part_path`, which is also what
                // builds the sentences below that tell the user where the
                // fragment went. Two spellings is two things to keep in step,
                // on the one path where the fragment is the only copy of the
                // rows.
                let part_of = crate::dump::part_of;
                let create = |path: &std::path::Path| {
                    std::fs::File::create(path).map(std::io::BufWriter::new)
                };
                // Rename the finished sibling over the destination. Failing *here*
                // is the one case where the export wrote everything and the user
                // still has no file at the path they chose, so it is reported as a
                // failure naming both halves rather than as a success.
                let publish =
                    |part: &std::path::Path, path: &std::path::Path| std::fs::rename(part, path);

                match req.scope {
                    // Everything is already in memory, so this is one blocking
                    // task and no channel — but it is **rendered in blocks all
                    // the same**, through `SliceChunks`.
                    //
                    // Not for memory: the rows are already here, and a chunk is
                    // the same `&ResultSet` with a *slice* of the same order
                    // vector, so nothing is copied either way. It is for the two
                    // things a single block cannot offer. **Progress**: rendered
                    // whole there is no moment between "started" and "finished"
                    // at which anything can be reported, and a 200k-row Excel
                    // export looks identical to a hung one for as long as it
                    // takes. **A Stop that works**: the modal offers one, and
                    // between blocks is the only place a synchronous render can
                    // notice it.
                    //
                    // `chunking_a_fetched_result_cannot_change_the_bytes` is what
                    // says this is invisible in the file — five renderers write a
                    // header on the first chunk and rows on every one, so a
                    // source that suddenly yields three chunks where it yielded
                    // one is exactly how a CSV grows two extra header lines.
                    ExportScope::Fetched => {
                        // **The same single slot the streamed scope claims — but
                        // only for a run someone can stop.** The slot exists so
                        // one Stop always reaches the run it points at, and it
                        // refuses a second export to keep that true; a run with
                        // no Stop on screen has nothing to keep true and must
                        // stay out of it. The grid raises the modal and asks for
                        // the slot; the Live Monitor's log export does not, and
                        // putting it in would have it refused — *"An export is
                        // already running"* — over a modal the user cannot see
                        // and a save that has nothing to do with theirs. See
                        // `ExportRequest::stoppable`.
                        //
                        // Among the runs that *do* claim it there is no save the
                        // refusal can cost, because the modal covers the window
                        // while one is going: there is no second grid export to
                        // start.
                        let token = if req.stoppable {
                            if export_token.borrow().is_some() {
                                (done)(ExportOutcome::Failed {
                                    message: "An export is already running. Cancel it or wait for \
                                              it to finish."
                                        .to_string(),
                                    // Nothing was opened.
                                    partial: false,
                                });
                                return;
                            }
                            let token = CancellationToken::new();
                            *export_token.borrow_mut() = Some(token.clone());
                            token
                        } else {
                            // Its own token, never in the slot: it is never
                            // cancelled, so the checks below are constant-false
                            // and this path behaves exactly as it did before the
                            // `Fetched` render was chunked.
                            CancellationToken::new()
                        };
                        // Two ext actions over one `done`, because the worker
                        // owns the first and the task that observes the worker
                        // needs the second — see the `job.await` below. Both
                        // release the cancel slot, and `ExportTarget::run` drops
                        // whichever arrives second, so a worker that reported
                        // *and then* failed to join cannot report twice.
                        let ext = {
                            let export_token = export_token.clone();
                            let stoppable = req.stoppable;
                            let done = done.clone();
                            move || {
                                let export_token = export_token.clone();
                                let done = done.clone();
                                create_ext_action(cx, move |o: ExportOutcome| {
                                    // **Only a run that took the slot may clear
                                    // it**, or an unstoppable log export
                                    // finishing would release the slot out from
                                    // under the grid export that is actually
                                    // holding it — and the next Stop would reach
                                    // nothing.
                                    if stoppable {
                                        *export_token.borrow_mut() = None;
                                    }
                                    (done)(o)
                                })
                            }
                        };
                        let (report, died) = (ext(), ext());
                        let (rs, order) = (req.rs.clone(), req.order.clone());
                        let progress = export_tx.clone();
                        // Named, not a bare field read — see `reports_progress`.
                        let reports = req.reports_progress();
                        let sweep_part = part_of(&path);
                        let incremental = format.writes_incrementally();
                        // **The handle is awaited, not dropped.** This arm's
                        // only exit from the modal is an outcome, so a worker
                        // that ended without reporting — a panic in a renderer,
                        // a `spawn_blocking` pool shutdown — left an
                        // undismissable modal *and* a permanently occupied cancel
                        // slot. The `AllRows` sibling has awaited its writer and
                        // said "worker died" since it was written.
                        let job = handle.spawn_blocking(move || {
                            let part = part_of(&path);
                            let w_token = token.clone();
                            let write =
                                || -> std::io::Result<schemaic_core::export::ExportTally> {
                                    use std::io::Write as _;
                                    let mut w = create(&part)?;
                                    // One hook, both jobs, asked at one instant —
                                    // see `SliceChunks::watching`. `false` ends
                                    // the render as an *error*, which is the only
                                    // way to stop without the file looking
                                    // finished.
                                    let mut src = schemaic_core::export::SliceChunks::new(
                                        rs.as_ref(),
                                        order.as_slice(),
                                        EXPORT_CHUNK_ROWS,
                                    )
                                    .watching(move |n| {
                                        // **Only a run with the modal reports.**
                                        // `export_progress` is one signal for the
                                        // window and the messages carry no run
                                        // id, so an unstoppable run — the Live
                                        // Monitor's log export, which by design
                                        // stays out of the cancel slot and can
                                        // therefore overlap a grid export — would
                                        // drive the grid modal's counter with its
                                        // own row count. `ExportTarget::run`
                                        // guards the outcome against exactly this
                                        // and nothing guarded the count.
                                        if reports {
                                            let _ = progress.send(n);
                                        }
                                        !w_token.is_cancelled()
                                    });
                                    let tally = format.stream_to(
                                        &mut w,
                                        &mut src,
                                        source.as_ref().map(|(d, ns, t)| {
                                            (d.as_str(), ns.as_deref(), t.as_str())
                                        }),
                                        dialect,
                                    )?;
                                    // Explicit: `BufWriter` swallows a flush failure
                                    // on drop, which is exactly the case where the
                                    // last block never reached the disk — silently
                                    // truncating the file.
                                    w.flush()?;
                                    drop(w);
                                    // **Only now** does the destination change, and
                                    // not at all if this was stopped — the streamed
                                    // scope's rule, and it has to hold here for the
                                    // same reason: a truncated render must never be
                                    // renamed over the user's file.
                                    if token.is_cancelled() {
                                        return Err(std::io::Error::other("export cancelled"));
                                    }
                                    publish(&part, &path)?;
                                    Ok(tally)
                                };
                            report(match write() {
                                Ok(tally) => ExportOutcome::Done(tally),
                                // The user's own Stop is neither a success nor a
                                // failure, and the token is the witness — the
                                // error above is indistinguishable from a write
                                // error by its text alone.
                                // The `.part` is swept here for the same
                                // reason the failure arm below sweeps it: a
                                // buffered format has written *nothing* into the
                                // sibling, so a stop leaves a 0-byte
                                // `foo.xlsx.part` in the user's folder under a
                                // message reading "foo.xlsx was not changed."
                                // that does not mention it. An incremental
                                // format's sibling holds the rows that arrived
                                // and is the one thing worth keeping.
                                Err(_) if token.is_cancelled() => {
                                    if !incremental {
                                        let _ = std::fs::remove_file(&sweep_part);
                                    }
                                    ExportOutcome::Cancelled
                                }
                                // `partial`: the write had begun, so the `.part`
                                // sibling holds whatever arrived — unless the
                                // format buffers, in which case the sibling is
                                // empty and is swept rather than left as litter
                                // for a message to point at. The destination
                                // itself is untouched either way.
                                Err(e) => {
                                    if !incremental {
                                        let _ = std::fs::remove_file(&sweep_part);
                                    }
                                    ExportOutcome::Failed {
                                        message: format!("Export failed: {e}"),
                                        partial: incremental,
                                    }
                                }
                            });
                        });
                        handle.spawn(async move {
                            if let Err(e) = job.await {
                                died(ExportOutcome::Failed {
                                    message: format!("Export failed: worker died: {e}"),
                                    partial: false,
                                });
                            }
                        });
                    }
                    // **Two tasks and a bounded channel.** The reader is async
                    // (two of the three drivers are) and the writer is
                    // synchronous file IO, which must not run on a runtime
                    // worker; the channel's bound is what keeps a server faster
                    // than the disk from queueing the table in memory, which is
                    // the entire point of streaming it.
                    ExportScope::AllRows {
                        conn_id,
                        database,
                        sql,
                    } => {
                        let db = match db_for(conn_id) {
                            Ok(db) => db,
                            Err(e) => {
                                // Refused before the writer task runs, so the
                                // destination was never opened: `partial: false`,
                                // and the message must not claim a file it never
                                // touched is incomplete.
                                (done)(ExportOutcome::Failed {
                                    message: e,
                                    partial: false,
                                });
                                return;
                            }
                        };
                        // **One streamed export at a time.** The token is a
                        // single slot, and it can be, because this refuses the
                        // second rather than overwriting it: two exports sharing
                        // the slot meant Cancel reached only the later one, and
                        // whichever finished first cleared the slot and left the
                        // other uncancellable. `import_token` gets away with the
                        // same shape only because its modal admits one run;
                        // the Download menu is on every result tab.
                        if export_token.borrow().is_some() {
                            (done)(ExportOutcome::Failed {
                                message:
                                    "An export is already running. Cancel it or wait for it to \
                                     finish."
                                        .to_string(),
                                // Nothing was opened — see the arm above.
                                partial: false,
                            });
                            return;
                        }
                        let token = CancellationToken::new();
                        *export_token.borrow_mut() = Some(token.clone());
                        let report = {
                            let export_token = export_token.clone();
                            create_ext_action(cx, move |o: ExportOutcome| {
                                *export_token.borrow_mut() = None;
                                (done)(o)
                            })
                        };
                        // Small on purpose: each block is `EXPORT_CHUNK_ROWS`
                        // rows, so a deep queue would be exactly the memory this
                        // avoids. Two lets the server read the next block while
                        // the disk takes the last.
                        let (tx, mut rx) =
                            tokio::sync::mpsc::channel::<schemaic_db::ExportChunk>(2);
                        let part = part_of(&path);
                        // A copy for the failure arms, which run in the other
                        // task: a buffered format leaves an empty sibling and
                        // nothing else would remove it.
                        let sweep_part = part.clone();
                        // The writer needs the token as well as the reader: a
                        // cancelled read closes the channel, which the writer sees
                        // as an ordinary end of stream — so without this it would
                        // rename a truncated file over the destination and only
                        // *then* have the reader declare the cancel.
                        let w_token = token.clone();
                        let progress = export_tx.clone();
                        let writer = handle.spawn_blocking(move || {
                            use std::io::Write as _;
                            // One for the pull closure below, one for the check
                            // after the write — both ask the same question at
                            // two different moments.
                            let src_token = w_token.clone();
                            let mut w = create(&part).map_err(|e| e.to_string())?;
                            // Rows handed to the renderer so far. Counted here
                            // rather than at the reader because the channel
                            // between them is bounded and would run two blocks
                            // ahead of the file — reporting rows that are not in
                            // it yet, and being two blocks wrong about what
                            // survived a cancel.
                            let mut rows_done = 0u64;
                            let mut src = schemaic_core::export::PullChunks::new(move || match rx
                                .blocking_recv()
                            {
                                // **A cancelled read is an error, not an end of
                                // stream.** It closes the channel, which reads
                                // as "the table ended" — and for a buffered
                                // format that means the whole discarded workbook
                                // is still assembled and compressed before
                                // anyone notices: measured 12.3 s at 800k x 50,
                                // with the bar still saying "Exporting…" and a
                                // Cancel that has already been pressed. Raising
                                // it here returns `stream_to` at the next chunk
                                // instead, which is where the five incremental
                                // formats already stop.
                                None if src_token.is_cancelled() => {
                                    Err(std::io::Error::other("export cancelled"))
                                }
                                None => Ok(None),
                                Some(Ok(rs)) => {
                                    rows_done += rs.row_count() as u64;
                                    // Best-effort, the rule every progress
                                    // channel here follows: a full channel or a
                                    // closed receiver must never hold up a write.
                                    let _ = progress.send(rows_done);
                                    Ok(Some(rs))
                                }
                                // The reason the reader put on the channel.
                                // Without this the writer would read the
                                // close as "the table ended" and call a
                                // half-written file finished.
                                Some(Err(e)) => Err(std::io::Error::other(e)),
                            });
                            let tally = format
                                .stream_to(
                                    &mut w,
                                    &mut src,
                                    source
                                        .as_ref()
                                        .map(|(d, ns, t)| (d.as_str(), ns.as_deref(), t.as_str())),
                                    dialect,
                                )
                                .map_err(|e| e.to_string())?;
                            w.flush().map_err(|e| e.to_string())?;
                            // **Only now** does the destination change — and not
                            // at all if the export was cancelled. A cancel reaches
                            // the writer as an ordinary end of stream, so the check
                            // has to be here: publishing first and letting the
                            // reader declare the cancel afterwards would rename a
                            // truncated file over the user's file, which is the
                            // whole thing the sibling exists to prevent. The
                            // caller's cancel arm reports it; this error is the
                            // belt to that brace.
                            drop(w);
                            if w_token.is_cancelled() {
                                return Err("export cancelled".to_string());
                            }
                            publish(&part, &path).map_err(|e| e.to_string())?;
                            Ok::<schemaic_core::export::ExportTally, String>(tally)
                        });
                        handle.spawn(async move {
                            let read = db
                                .stream_query(
                                    database.as_deref(),
                                    &sql,
                                    EXPORT_CHUNK_ROWS,
                                    token,
                                    tx,
                                )
                                .await;
                            let written = writer.await;
                            // **Cancel is the reader's to declare; every other
                            // failure is the writer's to describe.**
                            //
                            // The reader has to win on cancel, because a
                            // cancelled read closes the channel and the writer
                            // sees an ordinary end of stream — on its own it
                            // would call a truncated file a finished export.
                            //
                            // On anything else the writer is the more proximate
                            // witness, and asking the reader first got it
                            // backwards: a full disk fails the *writer*, whose
                            // exit then fails the reader's next `send` with "the
                            // export stopped reading" — so the user was told the
                            // symptom instead of "No space left on device". A
                            // reader-side failure still arrives intact, because
                            // `stream_query` puts its reason on the channel and
                            // the writer returns that very message.
                            // Every failure arm here is *after* the writer task
                            // opened the `.part` sibling, and the destination is
                            // untouched because the rename only runs when the
                            // write completed. `partial` is what says the sibling
                            // holds something; `export_failure_note` states both
                            // halves.
                            //
                            // **It is the format's answer, not `true`.** Excel is
                            // a ZIP and nothing reaches the sink until
                            // `save_to_writer`, so every pre-save failure left a
                            // *zero-byte* sibling the message pointed the user
                            // at. `writes_incrementally` is the capability; the
                            // empty file is swept rather than left as litter.
                            let incremental = format.writes_incrementally();
                            // **A buffered format's sibling is empty whatever
                            // ended the export**, so both the failure and the
                            // cancel arm sweep it — a 0-byte `foo.xlsx.part` in
                            // the user's folder under a message that does not
                            // mention it is litter either way. An incremental
                            // format's sibling holds the rows that arrived and is
                            // the one thing worth keeping.
                            let sweep_empty = {
                                let sweep_part = sweep_part.clone();
                                move || {
                                    if !incremental {
                                        let _ = std::fs::remove_file(&sweep_part);
                                    }
                                }
                            };
                            let sweep_on_cancel = sweep_empty.clone();
                            let failed = move |message: String| {
                                sweep_empty();
                                ExportOutcome::Failed {
                                    message,
                                    partial: incremental,
                                }
                            };
                            report(match (read, written) {
                                (Err(schemaic_db::DbError::Cancelled), _) => {
                                    sweep_on_cancel();
                                    ExportOutcome::Cancelled
                                }
                                (_, Ok(Err(e))) => failed(format!("Export failed: {e}")),
                                (_, Err(e)) => failed(format!("Export failed: worker died: {e}")),
                                (Err(e), _) => failed(format!("Export failed: {e}")),
                                (Ok(_), Ok(Ok(tally))) => ExportOutcome::Done(tally),
                            });
                        });
                    }
                }
            },
        )
    };

    let export_cancel: Rc<dyn Fn()> = {
        let export_token = export_token.clone();
        Rc::new(move || {
            if let Some(t) = export_token.borrow().as_ref() {
                t.cancel();
            }
        })
    };

    // ── Schema + data dump ───────────────────────────────────────────────────
    //
    // The work is in `dump.rs`; these three are the wiring. A dump gets its own
    // cancel slot rather than sharing the export's: the export token exists to
    // stop two *exports* competing for one disk, while a dump can only be
    // launched from a modal that already refuses a second one, and folding them
    // together would let a running export's Cancel stop a dump the user cannot
    // even see.
    //
    // Progress is a channel rather than a callback, for the reason the AI stream
    // is: `create_ext_action` is one-shot and has to be built on the UI thread,
    // so a worker reporting *repeatedly* needs a signal fed from a channel.
    let (dump_tx, dump_rx) = crossbeam_channel::unbounded::<schemaic_ui::DumpProgress>();
    let dump_stream = create_signal_from_channel(dump_rx);
    // The modal's own signal, made here rather than in the `Ui` literal below so
    // the effect that feeds it can be written next to the channel it feeds from.
    let dump_progress: RwSignal<Option<schemaic_ui::DumpProgress>> = RwSignal::new(None);
    create_effect(move |_| {
        if let Some(p) = dump_stream.get() {
            dump_progress.set(Some(p));
        }
    });
    let dump_token: Rc<RefCell<Option<CancellationToken>>> = Rc::new(RefCell::new(None));

    let dump_tables: schemaic_ui::DumpTablesFn = {
        let handle = handle.clone();
        let db_for = db_for.clone();
        Rc::new(
            move |conn_id: u64, database: String, done: Rc<dyn Fn(Result<Vec<String>, String>)>| {
                let report = create_ext_action(cx, move |r: Result<Vec<String>, String>| (done)(r));
                let db = match (db_for)(conn_id) {
                    Ok(db) => db,
                    Err(e) => return report(Err(e)),
                };
                handle.spawn(async move {
                    // The *list*, not the full schema: names are all a picker
                    // needs, and `fetch_schema` would read every column of every
                    // table to print them.
                    let out = db
                        .fetch_table_list(&database)
                        .await
                        .map(|s| {
                            s.tables
                                .iter()
                                .map(|t| {
                                    schemaic_core::schema::display_name(
                                        t.schema.as_deref(),
                                        &t.name,
                                    )
                                })
                                .collect::<Vec<_>>()
                        })
                        .map_err(|e| e.to_string());
                    report(out);
                });
            },
        )
    };

    let dump_run: schemaic_ui::DumpFn = {
        let handle = handle.clone();
        let db_for = db_for.clone();
        let dump_token = dump_token.clone();
        let dump_tx = dump_tx.clone();
        Rc::new(
            move |req: schemaic_ui::DumpRequest, done: schemaic_ui::DumpDoneFn| {
                use schemaic_ui::DumpOutcome;
                let db = match (db_for)(req.conn_id) {
                    Ok(db) => db,
                    Err(e) => {
                        // Refused before the write, so it must not claim a file
                        // it never touched.
                        return (done)(DumpOutcome::Failed {
                            message: format!("Dump failed: {e}"),
                            partial: false,
                        });
                    }
                };
                let token = CancellationToken::new();
                *dump_token.borrow_mut() = Some(token.clone());
                let report = {
                    let dump_token = dump_token.clone();
                    create_ext_action(cx, move |o: DumpOutcome| {
                        *dump_token.borrow_mut() = None;
                        (done)(o);
                    })
                };
                let (handle2, tx) = (handle.clone(), dump_tx.clone());
                handle.spawn(async move {
                    let outcome = dump::run(db, req, handle2, token, tx, EXPORT_CHUNK_ROWS).await;
                    report(outcome);
                });
            },
        )
    };

    // The folder export, wired exactly as the dump above and **sharing its
    // cancel slot**: both are launched from the one modal, which refuses a second
    // run while either is going, so the two can never be in flight together —
    // and separate slots would only create the case where the footer's Stop
    // reaches neither. Progress rides the dump's channel for the same reason: it
    // is the same footer line, counting the same "3 of 12".
    let files_run: schemaic_ui::FilesFn = {
        let handle = handle.clone();
        let db_for = db_for.clone();
        let dump_token = dump_token.clone();
        let dump_tx = dump_tx.clone();
        Rc::new(
            move |req: schemaic_ui::FilesRequest, done: schemaic_ui::FilesDoneFn| {
                use schemaic_ui::FilesOutcome;
                let db = match (db_for)(req.conn_id) {
                    Ok(db) => db,
                    Err(e) => {
                        // Refused before the first file, so `files: 0` — the
                        // message must not send the user to a folder nothing was
                        // written to.
                        return (done)(FilesOutcome::Failed {
                            message: format!("Export failed: {e}"),
                            files: 0,
                            // Refused before a plan existed, so nothing is known
                            // to be missing — `missing` is the plan's answer —
                            // and nothing in the folder was touched.
                            missing: Vec::new(),
                            replaced: Vec::new(),
                        });
                    }
                };
                let token = CancellationToken::new();
                *dump_token.borrow_mut() = Some(token.clone());
                let report = {
                    let dump_token = dump_token.clone();
                    create_ext_action(cx, move |o: FilesOutcome| {
                        *dump_token.borrow_mut() = None;
                        (done)(o);
                    })
                };
                let (handle2, tx) = (handle.clone(), dump_tx.clone());
                handle.spawn(async move {
                    let outcome =
                        dump::run_files(db, req, handle2, token, tx, EXPORT_CHUNK_ROWS).await;
                    report(outcome);
                });
            },
        )
    };

    let dump_cancel: Rc<dyn Fn()> = {
        let dump_token = dump_token.clone();
        Rc::new(move || {
            if let Some(t) = dump_token.borrow().as_ref() {
                t.cancel();
            }
        })
    };

    // ── Running a `.sql` script ──────────────────────────────────────────────
    //
    // The dump's mirror image, and wired the same way for the same reasons: the
    // work is in `script.rs`, progress is a channel because `create_ext_action`
    // is one-shot, and the cancel slot is its own rather than shared — a running
    // export's Stop must not reach a load the user cannot see.
    let (script_tx, script_rx) = crossbeam_channel::unbounded::<schemaic_ui::ScriptProgress>();
    let script_stream = create_signal_from_channel(script_rx);
    let script_progress: RwSignal<Option<schemaic_ui::ScriptProgress>> = RwSignal::new(None);
    create_effect(move |_| {
        if let Some(p) = script_stream.get() {
            script_progress.set(Some(p));
        }
    });
    let script_token: Rc<RefCell<Option<CancellationToken>>> = Rc::new(RefCell::new(None));

    let script_probe: schemaic_ui::ScriptProbeFn = {
        let handle = handle.clone();
        Rc::new(
            move |path: std::path::PathBuf,
                  dialect: schemaic_core::intel::SqlDialect,
                  done: schemaic_ui::ScriptProbeDoneFn| {
                let report = create_ext_action(
                    cx,
                    move |r: Result<schemaic_core::script::Probe, String>| (done)(r),
                );
                // Off the UI thread and blocking: the probe reads up to
                // `PROBE_MAX_BYTES` off a disk, which is exactly the pause the
                // modal must not take on the thread drawing it.
                handle.spawn_blocking(move || {
                    let out = std::fs::File::open(&path)
                        .map_err(|e| e.to_string())
                        .and_then(|f| {
                            schemaic_core::script::probe(f, dialect).map_err(|e| e.to_string())
                        });
                    report(out);
                });
            },
        )
    };

    let script_run: schemaic_ui::ScriptFn = {
        let handle = handle.clone();
        let db_for = db_for.clone();
        let script_token = script_token.clone();
        let script_tx = script_tx.clone();
        Rc::new(
            move |req: schemaic_ui::ScriptRequest, done: schemaic_ui::ScriptDoneFn| {
                use schemaic_core::script::RunOutcome;
                let db = match (db_for)(req.conn_id()) {
                    Ok(db) => db,
                    // Nothing ran, and the report must say so rather than name a
                    // statement it never reached.
                    Err(e) => {
                        return (done)(RunOutcome::Failed {
                            message: e,
                            ran: 0,
                            at: None,
                        });
                    }
                };
                let token = CancellationToken::new();
                *script_token.borrow_mut() = Some(token.clone());
                let report = {
                    let script_token = script_token.clone();
                    create_ext_action(cx, move |o: RunOutcome| {
                        *script_token.borrow_mut() = None;
                        (done)(o);
                    })
                };
                let tx = script_tx.clone();
                handle.spawn(async move {
                    let outcome = script::run(db, req, token, tx).await;
                    report(outcome);
                });
            },
        )
    };

    let script_cancel: Rc<dyn Fn()> = {
        let script_token = script_token.clone();
        Rc::new(move || {
            if let Some(t) = script_token.borrow().as_ref() {
                t.cancel();
            }
        })
    };

    // Write an exported ER diagram. The modal captures what the user was looking
    // at before it opens the save dialog (see `ErdDoc`) — for a picture that is
    // the measured scene, because measuring goes through the font system and the
    // font system is the UI thread's. Everything after it is here: building the
    // document, rasterising it when the target is a PNG, and the write.
    //
    // Same `spawn_blocking` reasoning as `export_file` above: synchronous file IO,
    // and a render that is pure CPU for as long as the diagram is large.
    let export_erd: schemaic_ui::ErdExportFn = {
        let handle = handle.clone();
        Rc::new(
            move |req: schemaic_ui::ErdExportRequest, done: schemaic_ui::ExportDoneFn| {
                let report = create_ext_action(cx, move |o: schemaic_ui::ExportOutcome| (done)(o));
                handle.spawn_blocking(move || {
                    // A diagram is one document written in one call: there is no
                    // row count, nothing to withhold and nothing to cancel, so an
                    // empty tally is the whole of its success.
                    //
                    // **Through `write_file_atomic`, because `fs::write` does
                    // not** — it is `File::create`, which truncates *and then*
                    // writes, so a full disk or a dropped share between the two
                    // leaves the destination empty: last week's exported diagram
                    // replaced by nothing. That function's own doc is this
                    // paragraph, and every other user-facing write here already
                    // uses the staged pattern (the grid export's `.part`
                    // sibling, the single-file dump's, `.sql` saves). `partial:
                    // false` is honest now rather than by assertion — a failed
                    // staged write leaves the previous file exactly as it was.
                    report(
                        match req.doc.into_bytes().and_then(|b| {
                            schemaic_core::persist::write_file_atomic(&req.path, &b)
                                .map_err(|e| format!("Export failed: {e}"))
                        }) {
                            Ok(()) => schemaic_ui::ExportOutcome::Done(Default::default()),
                            Err(e) => schemaic_ui::ExportOutcome::Failed {
                                message: e,
                                partial: false,
                            },
                        },
                    );
                });
            },
        )
    };

    // The binary-cell panel's own state, built here so both halves below can
    // report into it: the fetch that fills it and the save that writes from it.
    let blob = schemaic_ui::BlobUi::new();

    // Read one binary cell's bytes for the panel. The grid holds none — every
    // engine drops them at the wire — so opening a `<n bytes>` cell is a second,
    // targeted `SELECT` keyed by the row's own identity.
    //
    // **The connection is the result's, and the session is the tab's.** A blob
    // is re-read over `conn_at_load`, because that is where the row with this
    // key lives; but when the *active* tab holds a manual transaction on that
    // same connection, the read has to go over its pinned session or it cannot
    // see bytes the transaction has written and not committed. The two
    // conditions are separate and both are required — a session belonging to
    // some other connection would answer with a different database's row.
    // The in-flight read's token, so the panel's exit can stop it and a second
    // opening cannot leave the first one streaming. A plain `Rc<RefCell<..>>`
    // rather than a signal: nothing renders from it, and every touch is on the
    // UI thread.
    let blob_cancel: Rc<std::cell::RefCell<Option<CancellationToken>>> =
        Rc::new(Default::default());

    let view_blob: schemaic_ui::ViewBlobFn = {
        let handle = handle.clone();
        let db_for = db_for.clone();
        let session_for = session_for.clone();
        let blob_cancel = blob_cancel.clone();
        Rc::new(
            move |conn_id: u64,
                  r: Option<schemaic_core::blob::BlobRef>,
                  target: schemaic_ui::BlobTarget,
                  stage: Option<schemaic_ui::BlobStage>| {
                // **Supersede the previous read first, whatever this opening
                // turns out to be.** The panel shows one cell, so an earlier
                // fetch has no reader the moment this one opens — and the
                // epoch guard only stops its *answer* landing, not the transfer
                // itself, which runs to `FETCH_CAP` holding a connection (or the
                // tab's pinned session) busy. This sat below the `BlobRef`
                // branch and so was skipped entirely by a cell with nothing to
                // fetch: opening a NULL cell left the previous blob streaming.
                let token = CancellationToken::new();
                if let Some(prev) = blob_cancel.borrow_mut().replace(token.clone()) {
                    prev.cancel();
                }
                // Nothing committed to read — a pending new row, or a NULL cell.
                // The panel opens *in* the same `Empty` the server would have
                // answered with, and is a loader rather than a viewer. Opened in
                // that state rather than told about it a line later: the two are
                // one turn apart, which is close enough for the panel's rebuild
                // to carry the state it no longer has.
                let Some(r) = r else {
                    blob.open(target, stage, schemaic_ui::BlobState::Empty);
                    return;
                };
                let epoch = blob.open(target, stage, schemaic_ui::BlobState::Loading);
                let db = match db_for(conn_id) {
                    Ok(db) => db,
                    Err(e) => {
                        blob.loaded(epoch, schemaic_ui::BlobState::Failed(e));
                        return;
                    }
                };
                // Only the active tab can hold a transaction, and only its own
                // connection's. A tab whose session cannot be resolved is not an
                // error here: a read over a fresh connection is still the right
                // answer, just without the uncommitted rows.
                let id = active.get_untracked();
                let tab = tabs
                    .with_untracked(|v| v.iter().find(|t| t.id == id).copied())
                    .filter(|t| t.conn_id.get_untracked() == conn_id);
                let session = tab.and_then(|t| session_for(&t).ok().flatten());
                let report =
                    create_ext_action(cx, move |st: schemaic_ui::BlobState| blob.loaded(epoch, st));
                // **A read on the pinned connection still tells the transaction
                // what happened to it.** `Session` reports an `Outcome`, not a
                // `Result`, because a statement that lost the connection has to
                // reach `TxState::on_statement` — otherwise the footer pill goes
                // on claiming a live transaction over a dead socket and leaves
                // Commit enabled for one that no longer exists. Every other
                // session call site folds this; a blob read is not special.
                let engine = tx_engine(&db);
                let fold = tab.map(|tab| {
                    create_ext_action(cx, move |stmt: StmtOutcome| {
                        tab.tx
                            .update(|t| *t = t.on_statement(engine, "SELECT", stmt));
                    })
                });
                handle.spawn(async move {
                    let out = match &session {
                        Some(s) => {
                            let out = s.fetch_blob(&r, token).await;
                            if let Some(fold) = fold {
                                fold(out.stmt);
                            }
                            out.result
                        }
                        None => db.fetch_blob(&r, token).await,
                    };
                    report(match out {
                        Ok(Some(v)) => {
                            let kind = schemaic_core::blob::sniff(&v.bytes);
                            schemaic_ui::BlobState::Ready {
                                value: std::sync::Arc::new(v),
                                kind,
                            }
                        }
                        Ok(None) => schemaic_ui::BlobState::Empty,
                        Err(e) => schemaic_ui::BlobState::Failed(e.to_string()),
                    });
                });
            },
        )
    };

    // Write the panel's bytes to the file its dialog chose. Off the UI thread
    // for the reason the ERD export is: the buffer runs to `blob::FETCH_CAP`,
    // and a 64 MiB `fs::write` on the UI thread is a frozen window.
    // Stop whatever the panel is reading. Idempotent: a finished read's token
    // is still here and cancelling it does nothing.
    let cancel_blob: Rc<dyn Fn()> = {
        let blob_cancel = blob_cancel.clone();
        Rc::new(move || {
            if let Some(token) = blob_cancel.borrow_mut().take() {
                token.cancel();
            }
        })
    };

    // Read a file into the panel, off the UI thread for `save_blob`'s reason in
    // reverse: the file can be as large as the value it replaces.
    let load_blob: schemaic_ui::BlobLoadFn = {
        let handle = handle.clone();
        Rc::new(move |req: schemaic_ui::BlobLoadRequest| {
            let epoch = req.epoch;
            let report = create_ext_action(cx, move |r: Result<Vec<u8>, String>| {
                blob.loaded_file(epoch, r)
            });
            handle.spawn_blocking(move || {
                // **The size is checked before the read, not after.** Refusing a
                // 4 GB file by looking at the `Vec` it produced means allocating
                // it first, which is the failure the cap is for. `metadata` is
                // one stat call and the read still bounds itself below, since a
                // file can grow between the two.
                let cap = schemaic_core::blob::LOAD_CAP as u64;
                // The pair is contrasted, not formatted twice: `human_bytes`
                // keeps one decimal, so every size in a ~51 KB window above the
                // cap read "That file is 64.0 MB — the most that can be loaded
                // is 64.0 MB."
                let too_big = |n: u64| {
                    let (got, most) = schemaic_core::format::contrasting_bytes(n, cap);
                    format!("That file is {got} — the most that can be loaded is {most}.")
                };
                match std::fs::metadata(&req.path) {
                    Ok(m) if schemaic_core::blob::load_too_large(m.len()) => {
                        return report(Err(too_big(m.len())));
                    }
                    Ok(_) => {}
                    // No metadata is not a refusal — the read below reports the
                    // real error, which says more than a guess about size would.
                    Err(_) => {}
                }
                // **The read bounds itself**, which is what the paragraph above
                // claimed and `std::fs::read` did not do: it sizes its buffer
                // from the same `metadata` hint and then reads to EOF, and on
                // Linux `st_size` is 0 for a character or block device — so
                // `/dev/zero` passed the check above and grew a `Vec` until the
                // process died. `read_capped` asks for one byte more than the
                // cap, so "exactly the cap" and "at least the cap" stay apart.
                report(
                    match std::fs::File::open(&req.path)
                        .and_then(|f| schemaic_core::blob::read_capped(f, cap))
                    {
                        // Over the cap, and the size is not knowable without
                        // reading the rest of it — which is the thing refused.
                        // The `metadata` hint is the honest number when it has
                        // one, and `cap + 1` says "more than this" when it does
                        // not.
                        Ok(None) => Err(too_big(
                            std::fs::metadata(&req.path)
                                .map(|m| m.len())
                                .unwrap_or(cap + 1)
                                .max(cap + 1),
                        )),
                        Ok(Some(bytes)) => Ok(bytes),
                        Err(e) => Err(format!("Load failed: {e}")),
                    },
                );
            });
        })
    };

    let save_blob: schemaic_ui::BlobSaveFn = {
        let handle = handle.clone();
        Rc::new(move |req: schemaic_ui::BlobSaveRequest| {
            let epoch = req.epoch;
            let report =
                create_ext_action(cx, move |r: Result<String, String>| blob.saved_at(epoch, r));
            handle.spawn_blocking(move || {
                let shown = req.path.display().to_string();
                // Staged and renamed, for the reason the ERD export above is:
                // `fs::write` truncates before it writes, and the file the user
                // picked is very often one they already had.
                report(
                    schemaic_core::persist::write_file_atomic(&req.path, &req.bytes.bytes)
                        .map(|()| shown)
                        .map_err(|e| format!("Save failed: {e}")),
                );
            });
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
                // **`base_sql`, not `query`, and only if it reads.**
                //
                // `query` is the live editor buffer, and it drifts: after a
                // parameterised run it holds the *template*, so the post-commit
                // re-run replayed `… WHERE id = :id` and replaced a successful
                // commit's grid with `ERROR 1064 … near ':id'` — with `commit_err`
                // empty, so it read as a failed commit and invited the insert to
                // be made twice. `base_sql` is what actually ran: substituted, and
                // already judged by `run_verdict` when it did.
                //
                // The read check is the other half. This site takes the *raw*
                // run — `guarded_run` is built much later in this function and
                // cannot be reached from here — so whatever it replays executes
                // with the missing-`WHERE` net and `confirm_writes` both bypassed.
                // A grid is only editable when it came from a read, so refusing to
                // replay anything else costs nothing real and closes the hole
                // rather than narrowing it.
                let refetch_sql = tab
                    .base_sql
                    .get_untracked()
                    .filter(|s| schemaic_core::sql::read_only_reason(s, dialect_of(&db)).is_ok());
                let run = run.clone();
                let engine = tx_engine(&db);
                let fold = create_ext_action(cx, move |stmt: StmtOutcome| {
                    // A write batch is one unit as far as the transaction is
                    // concerned, so it folds as a single statement.
                    tab.tx
                        .update(|t| *t = t.on_statement(engine, "UPDATE", stmt));
                });
                // **Its own action, because `create_ext_action` is one-shot** —
                // and the post-commit re-fetch is a second statement on the same
                // pinned connection, so it has its own outcome to fold. A
                // `SELECT`, like the blob read's.
                let fold_refetch = create_ext_action(cx, move |stmt: StmtOutcome| {
                    tab.tx
                        .update(|t| *t = t.on_statement(engine, "SELECT", stmt));
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
                            if still_active && let Some(sql) = refetch_sql.clone() {
                                (run)(sql);
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
                                    // **Folded, like every other session call
                                    // site.** This one took `.result` and threw
                                    // the `StmtOutcome` away, so a connection
                                    // that died between the commit and the
                                    // re-read left the tab's `tx` reading
                                    // `Open`: the pill went on claiming a live
                                    // transaction over a dead socket,
                                    // `can_commit()` stayed true, and Commit
                                    // issued `COMMIT` for a transaction the
                                    // server had already discarded — while the
                                    // failure fell to `FullReran` and showed
                                    // the pre-commit rows. The blob read 230
                                    // lines above states the rule: "a read on
                                    // the pinned connection still tells the
                                    // transaction what happened to it".
                                    let out = s.refetch_rows(&req.template, &req.rows, token).await;
                                    fold_refetch(out.stmt);
                                    out.result
                                }
                                None => db.refetch_rows(&req.template, &req.rows, token).await,
                            };
                            match rows {
                                // **A short answer is a re-fetch that could not
                                // answer, not a row that is gone.** The `WHERE`
                                // carries the confirming columns valued from
                                // what the grid read *before* the statement ran,
                                // so a trigger or a `STORED` column touching one
                                // of them makes the confirmed read miss a row
                                // that is there and holds exactly what the user
                                // asked for. Splicing the empty vector and
                                // clearing the staging anyway painted the
                                // pre-edit value back under a green "1 row
                                // updated". See `RefetchRequest::covered_by`.
                                Ok(rows) if req.covered_by(&rows) => {
                                    finish(CommitDone::Spliced(rows))
                                }
                                Ok(_) => finish(CommitDone::FullReran),
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

    // ── Server Activity panel: the sessions on the active connection's server ──
    //
    // Everything here is transient except the poll interval. The snapshot belongs
    // to one connection and is thrown away when the active connection changes —
    // showing another server's session ids for even one frame is an invitation to
    // kill the wrong thing.
    let activity_state: RwSignal<ActivityState> = RwSignal::new(ActivityState::Idle);
    // The interval is **per connection** — how hard you are willing to lean on a
    // particular server, not a taste. One number would carry a laptop's two
    // seconds straight onto a production replica on the next switch.
    let activity_intervals: RwSignal<Vec<schemaic_core::activity::IntervalRule>> =
        RwSignal::new(ui_state.activity_intervals);
    // The active connection's interval, derived. Everything downstream — the
    // clock's tint, the menu's marked row, the poll timer's re-arm — reads this
    // one value, so switching connections repoints all three at once.
    let activity_interval = create_memo(move |_| {
        let cid = active_conn.get();
        activity_intervals.with(|r| schemaic_core::activity::interval_for(r, cid))
    });
    let set_activity_interval: Rc<dyn Fn(u64)> = Rc::new(move |secs: u64| {
        let cid = active_conn.get_untracked();
        activity_intervals.update(|r| schemaic_core::activity::set_interval(r, cid, secs));
    });
    let activity_busy = RwSignal::new(false);
    // The last refused kill. Deliberately *not* `ActivityState::Failed`: that one
    // means "there is no snapshot", and a kill the server declined leaves the
    // snapshot perfectly good.
    let activity_kill_error: RwSignal<Option<String>> = RwSignal::new(None);
    // The clock's interval dropdown. Its flag and anchor live out here because the
    // menu is a root-level overlay — the right column is clipped, so a dropdown
    // drawn inside the panel would be cut off at the panel's edge.
    let activity_menu_open = RwSignal::new(false);
    let activity_menu_anchor = RwSignal::new(floem::kurbo::Point::ZERO);
    // Which generation the in-flight fetch was launched under, or `None`. This is
    // the de-dup guard, and it is a generation rather than the plain `busy` flag
    // for one case: a fetch still running when the user switches connections must
    // not block the *new* connection's first fetch. With auto-refresh off there is
    // no later tick to recover, and the panel would sit on "Loading…" until
    // someone pressed refresh.
    let activity_inflight: RwSignal<Option<u64>> = RwSignal::new(None);
    // Bumped whenever the panel is opened, closed, or repointed at another
    // connection. A poll timer carries the generation it was armed under and stops
    // when it no longer matches — the same guard the live monitor uses, and the
    // reason switching connections twice doesn't leave two loops polling.
    let activity_gen = RwSignal::new(0_u64);

    // **Defined here, above every `rearm_activity` caller, because all of them
    // have to ask it.** It used to sit further down, past the kill handler — so
    // the one arming site that could not reach it passed a literal `true`
    // instead, and a successful kill restarted auto-refresh on a panel the user
    // had switched away from or a window that had lost focus. That is precisely
    // the "connect every two seconds for nobody" load `should_poll` was written
    // to remove, reinstated by the one action on this panel that is guaranteed
    // to be followed by the user looking somewhere else.
    //
    // Every read is **tracked**, which is what the poll effect needs — crossing
    // the responsive breakpoint or losing focus must re-run it. The callers
    // outside any effect (`reset_activity`, the kill report) simply find the
    // tracking inert, and the answer is the same.
    let activity_polling = move || {
        schemaic_core::activity::should_poll(
            right_panel.get() == RightPanel::Activity,
            schemaic_ui::right_panel_visible(),
            window_focused.get(),
        )
    };

    let refresh_activity: Rc<dyn Fn()> = {
        let db_for = db_for.clone();
        let handle = handle.clone();
        Rc::new(move || {
            // The generation this fetch belongs to; also what the reply is checked
            // against on arrival.
            let generation = activity_gen.get_untracked();
            if activity_inflight.get_untracked() == Some(generation) {
                return;
            }
            let conn_id = active_conn.get_untracked();
            let db = match db_for(conn_id) {
                Ok(db) => db,
                // Usually "the SSH tunnel isn't established yet", which the next
                // tick resolves. Said out loud rather than left as a permanent
                // "Loading…", since with auto-refresh off there is no next tick.
                Err(e) => {
                    activity_state.set(ActivityState::Failed(e));
                    return;
                }
            };
            // Settled here rather than by a round trip that would come back with
            // an error the panel would have to translate back into this.
            if !schemaic_core::activity::supports_activity(db.engine().dialect()) {
                activity_state.set(ActivityState::Unsupported);
                return;
            }
            // `Loading` only while there is nothing to show; a refresh over a live
            // snapshot leaves it on screen (see `ActivityState`).
            if !matches!(activity_state.get_untracked(), ActivityState::Loaded { .. }) {
                activity_state.set(ActivityState::Loading);
            }
            activity_inflight.set(Some(generation));
            activity_busy.set(true);
            // A refused kill is news about one click, not a standing condition —
            // the next look at the server retires it.
            activity_kill_error.set(None);
            let report = create_ext_action(
                cx,
                move |res: Result<Vec<schemaic_core::activity::SessionInfo>, String>| {
                    // Release the guard only if this is still the fetch it is
                    // holding — a superseded reply landing after a newer fetch
                    // started must not unlock that one.
                    if activity_inflight.get_untracked() == Some(generation) {
                        activity_inflight.set(None);
                        activity_busy.set(false);
                    }
                    // A reply that outlived its generation describes the wrong
                    // server, or a panel that is no longer open.
                    if activity_gen.try_get_untracked() != Some(generation) {
                        return;
                    }
                    match res {
                        Ok(mut sessions) => {
                            let truncated = schemaic_core::activity::prepare(&mut sessions);
                            activity_state.set(ActivityState::Loaded {
                                sessions: Rc::new(sessions),
                                truncated,
                            });
                        }
                        Err(e) => activity_state.set(ActivityState::Failed(e)),
                    }
                },
            );
            handle.spawn(async move {
                report(db.fetch_sessions().await.map_err(|e| e.to_string()));
            });
        })
    };

    // **A killed session may be one of ours.** A Manual tab pins a connection
    // (`Session`), that connection is an ordinary row in the panel, and the
    // idle-in-transaction holder blocking your other tab is very often exactly
    // it. Terminating it left the tab holding a dead socket: the footer went on
    // offering Commit and Rollback, the next statement failed with "An
    // established connection was aborted", and the only way back was closing the
    // tab and reopening it.
    //
    // `Session::server_id` is what makes the connection recognisable, so the tab
    // is put back the way a lost transaction leaves it — no transaction open — on
    // a **fresh** pinned connection, and stays in Manual. The uncommitted work is
    // gone either way; the confirm said so before the kill.
    //
    // A *cancel* is not a kill: it stops the statement and leaves the session and
    // its transaction alive, so there is nothing to repair.
    //
    // **A server id is only unique on its own server**, so `conn_id` is half the
    // key and not a formality. MySQL thread ids and PostgreSQL backend pids are
    // small integers each server hands out from its own counter, so two Manual
    // tabs on two different connections routinely hold the same one — a laptop
    // MariaDB and a Docker MySQL both sitting at thread 42 is an ordinary
    // afternoon. Matching on the id alone reached into whichever tab the map
    // happened to yield first and, if that was the wrong one, closed a
    // transaction that was still open on a server nobody had touched and
    // re-pinned its connection underneath it — losing the uncommitted work of a
    // tab the user never acted on, while the tab that actually lost its socket
    // stayed broken.
    // Which of our own tabs holds this server session, if any.
    //
    // **Two callers, and the earlier one is the point.** `repair_killed_session`
    // has always asked this *after* the kill, to put the tab back together. The
    // confirm has to ask it *before*: a Manual tab's pinned session is an
    // ordinary row in the panel and wears the same `user@host` as every other row
    // from that connection, so without this the modal described the user's own
    // uncommitted work as somebody else's client. One lookup rather than two
    // spellings of it.
    // **Reads the three registries and hands them to [`owning_tab_of`]**, which
    // is where the decision lives and where it can be tested. This closure's only
    // job is the gathering; nothing here chooses anything.
    let owning_tab: Rc<dyn Fn(u64, i64) -> Option<schemaic_ui::Tab>> = {
        let sessions = sessions.clone();
        Rc::new(move |conn_id: u64, id: i64| {
            let pinned: Vec<(usize, Option<i64>)> = sessions
                .borrow()
                .iter()
                .map(|(tab_id, s)| (*tab_id, s.server_id()))
                .collect();
            let tab_conns: Vec<(usize, u64)> = tabs.with_untracked(|v| {
                v.iter()
                    .map(|t| (t.id, t.conn_id.get_untracked()))
                    .collect()
            });
            let owner = connections
                .with_untracked(|cs| owning_tab_of(&pinned, &tab_conns, cs, conn_id, id))?;
            tabs.with_untracked(|v| v.iter().find(|t| t.id == owner).copied())
        })
    };

    let repair_killed_session: Rc<dyn Fn(u64, i64, schemaic_core::activity::KillKind)> = {
        let owning_tab = owning_tab.clone();
        let open_session = open_session.clone();
        Rc::new(
            move |conn_id: u64, id: i64, kind: schemaic_core::activity::KillKind| {
                if kind != schemaic_core::activity::KillKind::Session {
                    return;
                }
                let Some(tab) = (owning_tab)(conn_id, id) else {
                    return;
                };
                tab.tx.set(TxState::closed());
                // Drops the dead session (its rollback is best effort and will
                // fail — the server already did it) and opens a replacement.
                (open_session)(tab.id);
            },
        )
    };

    // Cancel a statement / terminate a session, behind the shared confirm. The
    // confirm is raised here rather than in the panel so no route to a kill can
    // skip it, and the refresh afterwards is what makes the list agree with the
    // server again without waiting for the next tick.
    let kill_session: Rc<dyn Fn(i64, schemaic_core::activity::KillKind)> = {
        let db_for = db_for.clone();
        let handle = handle.clone();
        let refresh = refresh_activity.clone();
        Rc::new(move |id: i64, kind: schemaic_core::activity::KillKind| {
            // The confirm has to name the session, so it is built from the row in
            // the current snapshot rather than from the id alone.
            let session = activity_state.with_untracked(|st| match st {
                ActivityState::Loaded { sessions, .. } => {
                    sessions.iter().find(|s| s.id == id).cloned()
                }
                _ => None,
            });
            // Gone between the right-click and the click — a poll landed, or the
            // session ended on its own. Nothing to ask about and nothing left to
            // kill, which is the outcome the click wanted.
            let Some(session) = session else {
                return;
            };
            // **The target is resolved now, not when the button is clicked.**
            // A session id means nothing without the server that issued it, and a
            // modal is open across an unbounded stretch of time. Reading
            // `active_conn` inside `resolve` meant a connection change while the
            // confirm was up sent `KILL CONNECTION 1148` to a *different* server —
            // terminating whichever unrelated session there happened to hold that
            // id, under a modal whose title named the first one. Everything else
            // in this panel is generation-guarded against exactly this drift;
            // capturing the handle is that guard for the one destructive path.
            let conn_id = active_conn.get_untracked();
            let Ok(db) = db_for(conn_id) else {
                return;
            };
            // **The shared destructive guard, at the launch.** A connection
            // marked read-only is the protection with no "Run anyway", and
            // terminating a live client session — rolling back its transaction
            // under it — is the most destructive thing this app can do to a
            // server it has been told not to write to. Every other destructive
            // modal action asks this function; this one asked nothing at all.
            let read_only = connections
                .with_untracked(|cs| schemaic_core::connection::read_only_of(cs, conn_id));
            // The `false` is not a placeholder: a kill is fire-and-forget and
            // this action has no in-flight state of its own to read, which is
            // the one thing that would make a literal here the failure
            // CLAUDE.md warns about ("a constant in place of a capability").
            // `read_only`, the term that does vary, is read live above.
            if !schemaic_ui::may_launch_destructive(false, read_only) {
                // Per kind: this refusal answered *Cancel query* with a sentence
                // about terminating sessions, which is a different action.
                activity_kill_error.set(Some(schemaic_core::activity::read_only_refusal(kind)));
                return;
            }
            let dialect = db.engine().dialect();
            // **Asked before the confirm, not after the kill.** A Manual tab's
            // pinned session is an ordinary row here and wears the same
            // `user@host` as every other row from that connection, so the modal
            // was describing the user's own uncommitted work as a stranger's.
            let ours = (owning_tab)(conn_id, id).map(|t| t.title());
            let (title, message) =
                schemaic_core::activity::kill_confirm(kind, &session, dialect, ours.as_deref());
            let (handle, refresh) = (handle.clone(), refresh.clone());
            let repair_killed_session = repair_killed_session.clone();
            confirm.set(Some(Confirm {
                title,
                message,
                resolve: Rc::new(move |yes| {
                    if !yes {
                        return;
                    }
                    let db = db.clone();
                    let refresh = refresh.clone();
                    let repair = repair_killed_session.clone();
                    activity_kill_error.set(None);

                    let report = create_ext_action(cx, move |res: Result<(), String>| {
                        match res {
                            // A refused kill (no `CONNECTION_ADMIN`, no
                            // `pg_signal_backend`) says nothing about the snapshot
                            // on screen, so it no longer replaces it: routing this
                            // through `ActivityState::Failed` threw away the list
                            // someone was reading mid-incident — banner included —
                            // and with auto-refresh Off nothing brought it back.
                            Err(e) => activity_kill_error.set(Some(e)),
                            Ok(()) => {
                                (repair)(conn_id, id, kind);
                                // **Bump the generation first.** The refresh is
                                // guarded against a fetch already in flight, and
                                // a poll started while the kill was travelling
                                // swallowed it silently — so with auto-refresh
                                // Off the killed session stayed on the list with
                                // a live *Kill session* under it, and nothing
                                // said the kill had worked. Bumping strands the
                                // in-flight reply (it would be describing a
                                // server state that no longer holds) and frees
                                // the guard for this one.
                                //
                                // **And re-arms.** The bump strands the poll
                                // loop as surely as it strands the fetch, and
                                // for a while nothing here started a new one:
                                // auto-refresh died permanently at the first
                                // successful kill, on the panel whose subject is
                                // a live server, immediately after the one
                                // action that changes it.
                                //
                                // **Re-arms to `activity_polling()`, not to
                                // `true`.** A literal here says "there is
                                // someone watching" on the strength of a reply
                                // that has just come back from the network —
                                // by which time the panel may be closed, the
                                // window unfocused or the column collapsed to
                                // zero width, all three of which `should_poll`
                                // exists to answer No to. The two sibling
                                // callers below already pass it.
                                rearm_activity(
                                    activity_gen,
                                    activity_interval,
                                    refresh.clone(),
                                    activity_polling(),
                                );
                                (refresh)();
                            }
                        }
                    });
                    handle.spawn(async move {
                        report(db.kill_session(id, kind).await.map_err(|e| e.to_string()));
                    });
                }),
            }));
        })
    };

    // Open / close / repoint the panel. One effect rather than three, because the
    // generation bump, the snapshot reset and the refresh have to happen in that
    // order and a second effect firing between them is what leaves a stale list on
    // screen under a new connection's heading. (The re-arm rides with the bump in
    // `rearm_activity` and only schedules a timer, so where it falls among the
    // three is not observable — that it happens at all is.)
    //
    // The panel is polled only while it is *open* **and the window has focus**: a
    // two-second query against `information_schema` that nobody can see is pure
    // load, and the whole point of the panel is to notice load. Every tick is also
    // a full connect + authenticate against the server being watched (one
    // connection per operation, ARCHITECTURE §7), so a panel left open behind
    // another window was opening and tearing down a connection every couple of
    // seconds, indefinitely, for nobody. This is the rule the health poll already
    // follows, for the same reason; regaining focus re-runs this effect and the
    // first thing it does is refresh, so coming back shows current data rather
    // than whatever was on screen when you left.
    // **One spelling of "is the panel actually polling"**, because there are two
    // askers — the effect below and `reset_activity` — and this gate has grown
    // before: `right_panel_visible()` was added as the conjunct that was missing,
    // after a 0px panel went on polling because it could be neither watched nor
    // stopped from inside the app. A second copy is how the next conjunct reaches
    // one asker and not the other — and a *third* asker that could not reach the
    // closure at all is how the kill handler came to arm the loop with a bare
    // `true`. `activity_polling` is now defined above all three.
    {
        let refresh = refresh_activity.clone();
        create_effect(move |prev: Option<(u64, bool, u64)>| {
            let open = activity_polling();
            let conn = active_conn.get();
            let secs = activity_interval.get();
            let conn_changed = prev.is_none_or(|(c, _, _)| c != conn);
            // An interval change re-arms the timer and nothing else. Refreshing on
            // it too meant moving a struggling server from 2s to 30s — done
            // precisely to lean on it less — fired an immediate extra fetch, and
            // because the generation had just been bumped the in-flight guard
            // couldn't suppress it: two `PROCESSLIST` queries at once, on the
            // server that prompted the change.
            let woke = prev.is_none_or(|(_, was_open, _)| !was_open);
            // Bumping first is what strands every timer and every in-flight fetch
            // armed under the old state — and the re-arm rides with it, so the
            // panel cannot be left bumped-but-unarmed.
            rearm_activity(activity_gen, activity_interval, refresh.clone(), open);
            if conn_changed {
                activity_state.set(ActivityState::Idle);
                activity_kill_error.set(None);
            }
            if open && (conn_changed || woke) {
                (refresh)();
            }
            (conn, open, secs)
        });
    }

    // Throw the Server Activity snapshot away and start again — what the effect
    // above does on a *switch*, as a thing a caller can ask for.
    //
    // **It exists because a switch is not the only way the server under the panel
    // changes.** The effect keys on `active_conn`, which is an id, and editing a
    // connection in place does not move it: repoint the active connection from
    // host X to host Y with the panel open and a snapshot of X's sessions stays on
    // screen, live-looking, against a connection now pointing elsewhere. A kill
    // from that list sends X's thread ids to Y — where they name whatever Y
    // happens to have.
    //
    // `save_conn` calls this for **any** save of the active connection, without
    // asking whether the target actually moved. Comparing would mean carrying the
    // old connection's host/port/socket alongside the snapshot — a second copy of
    // connection identity, kept in step by hand, to save one `PROCESSLIST` query
    // when someone renames a connection. The cheap answer cannot be wrong; the
    // clever one has a way to be.
    //
    // The generation bump is the load-bearing half and comes first: a fetch
    // already in flight against the *old* host would otherwise land afterwards
    // and refill the panel with exactly the rows this is throwing away.
    //
    // **The refresh is the optional half, and it asks `db_for` first.** Clearing
    // is always right; *refetching now* is only right when the connection can be
    // reached this instant, and after an edit it routinely cannot: `save_conn`
    // drops the cached SSH tunnel, `load_schema` re-establishes it
    // asynchronously, and in between `db_for` answers "SSH tunnel is not
    // established yet". Refreshing anyway painted that over the panel as
    // `Failed` — with the interval off there is no tick to retire it, so a saved
    // edit left a stuck error where a stale snapshot used to be. `Failed` is the
    // right answer for a refresh the *user* asked for, which is why
    // `refresh_activity` says it out loud; it is the wrong answer for a reset
    // nobody asked for, where "no snapshot yet" is the truth. Deleting the last
    // connection reaches the same guard by the other door — `db_for` then answers
    // "connection no longer exists", and an empty panel is what that means.
    let reset_activity: Rc<dyn Fn()> = {
        let refresh = refresh_activity.clone();
        let db_for = db_for.clone();
        Rc::new(move || {
            let open = activity_polling();
            rearm_activity(activity_gen, activity_interval, refresh.clone(), open);
            // Guarded, like every other write to these: `RwSignal::set` never
            // dedups, and a rename with the panel closed would otherwise notify
            // every view reading them for a value that did not move.
            if !matches!(activity_state.get_untracked(), ActivityState::Idle) {
                activity_state.set(ActivityState::Idle);
            }
            if activity_kill_error.get_untracked().is_some() {
                activity_kill_error.set(None);
            }
            if open && db_for(active_conn.get_untracked()).is_ok() {
                (refresh)();
            }
        })
    };

    // Active-database context. A tab carries its `(conn_id, database)`, so
    // switching the active db just rewrites the active tab's `database` (and binds
    // it to the active connection) — no server-side `USE` / session state to track.
    let active_db_menu_open = RwSignal::new(false);
    let active_db_anchor = RwSignal::new(floem::kurbo::Point::ZERO);
    // The last database the user explicitly switched to **on each connection**;
    // new tabs default to it. Keyed by `conn_id` like `last_tab`, and for the
    // same reason: it was one global name, so picking `world` on MariaDB (where
    // it is visible) and switching to a PostgreSQL connection that also has a
    // `world` (hidden there) bound the new tab to the hidden one — the
    // per-connection guarantee `core::db_hidden` exists to give, defeated a
    // layer above it.
    let last_db: RwSignal<HashMap<u64, String>> = RwSignal::new(HashMap::new());
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
            //
            // **And picking the one you are on is not a pick.** The menu offers
            // it — the current row is accented rather than disabled — and the
            // rebind below is not free: it cancels the tab's running query,
            // settles its transaction through `guard_tx`, and re-pins its
            // session. `tabsel::rebind_needed` is that decision, with the
            // existence check folded in so there is one answer and not two;
            // `set_tx_mode` opens with the same refusal for the same reason.
            let id = active.get_untracked();
            let known: Vec<String> =
                db_nodes.with_untracked(|ns| ns.iter().map(|n| n.database.clone()).collect());
            let tab_binding = tabs.with_untracked(|v| {
                v.iter()
                    .find(|t| t.id == id)
                    .map(|t| (t.conn_id.get_untracked(), t.database.get_untracked()))
            });
            if !schemaic_core::tabsel::rebind_needed(
                tab_binding.as_ref().map(|(c, d)| (*c, d.as_deref())),
                active_conn.get_untracked(),
                &name,
                &known,
            ) {
                return;
            }
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
                    // Remembered against the connection it was picked on — the
                    // selector lists that connection's databases and nothing
                    // else, so the name means nothing anywhere else.
                    last_db.update(|m| {
                        m.insert(active_conn.get_untracked(), name.clone());
                    });
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
        // **The whole decision is `schema::tab_target`.** It used to be spelled
        // here as a `.filter(exists).or_else(first_bindable)`, and only the
        // fallback asked about visibility — so hiding the database you were in
        // and pressing Ctrl+T bound the new tab straight back into it, past
        // every list that had stopped showing it.
        let remembered = last_db.with_untracked(|m| m.get(&conn_id).cloned());
        // The connection's own **Database** field, which the form says is where
        // this connection opens — second only to what the user last switched to
        // here. See `schema::first_bindable`.
        let configured = connections.with_untracked(|cs| {
            cs.iter()
                .find(|c| c.id == conn_id)
                .map(|c| c.database.clone())
        });
        let names: Vec<String> =
            db_nodes.with_untracked(|v| v.iter().map(|n| n.database.clone()).collect());
        let database = hidden_dbs.with_untracked(|h| {
            schemaic_core::schema::tab_target(
                remembered.as_deref(),
                configured.as_deref(),
                &names,
                h,
            )
            .map(str::to_string)
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
            schemaic_ui::activate(active, id);
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
            // A Manual tab's pinned connection goes with it. By the time we get
            // here any open transaction has been settled by `close_tab`'s prompt,
            // so this is just releasing the connection.
            //
            // **Below the pinned backstop, not above it.** Releasing first meant
            // the `return` above could hand back a tab whose session had already
            // gone — unreachable today, because `guard_close` refuses a pinned
            // close through `tabsel::can_close` before anything is asked, but the
            // ordering is not something the next caller should have to know.
            (drop_session)(id);
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
                // Also reset the results pane so the reopened tab is fully fresh:
                // one empty result, and the pins go with the rest — they belong
                // to the tab that was closed, not to the one respawning here.
                tab.reset_results();
                // **And out of Manual**, because the release above took its
                // pinned session with it. `session_for` matches `TxMode::Manual`,
                // looks the tab up in `sessions`, finds nothing and refuses the
                // run before dispatch — "the transaction connection isn't ready
                // — switch to Auto-commit and back" — and nothing on this path
                // re-opens one, so the blank slate could not run a statement
                // until the user noticed the footer pill and toggled it twice.
                // The same two lines the connection-repointed path and
                // `delete_conn_now` fold into their own release, for the same
                // reason: the tab dropping to Auto-commit is what stops its
                // footer claiming a transaction that no longer exists.
                tab.tx_mode.set(TxMode::Auto);
                tab.tx.set(TxState::closed());
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
                schemaic_ui::activate(active, n);
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
                        schemaic_ui::activate(active, keep);
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
                    && t.results_untouched()
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
        schemaic_ui::activate(active, new_tab.id);
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
            schemaic_ui::activate(active, new_id);
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
            // Through the mint like every other use of the raw `run`, though
            // this statement is `filter::table_query`'s own and cannot be a
            // write: a caller that happens to generate only reads is a property
            // of the caller, and the whole point of `RerunRequest` is that the
            // action does not depend on one. With this, the two `run(sql)` sites
            // outside `guarded_run` both hold a request nothing but
            // `sql::rerunnable_for_export` can mint.
            if let Some(req) = schemaic_ui::RerunRequest::approved(sql, dialect) {
                run(req.into_sql());
            }
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
                schemaic_ui::activate(active, tab.id);
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
                schemaic_ui::activate(active, tab.id);
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
    // Same rule as `apply_view`: the caller supplies the SQL and this ends in
    // the raw `run`, so the guard is the argument's own — see
    // `schemaic_ui::RerunRequest`.
    let open_table_filtered: Rc<dyn Fn(TableSource, schemaic_ui::RerunRequest)> = {
        let next_id = next_id.clone();
        let place_tab = place_tab.clone();
        let run = run.clone();
        Rc::new(move |source: TableSource, req: schemaic_ui::RerunRequest| {
            let sql = req.into_sql();
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

    // Open a new tab with `sql` in the editor but do NOT run it (used by the
    // schema menus' Generate entries, the DDL preview and the AI code-block bar).
    //
    // **`database` is the database the statement is *for*.** Without it the tab
    // fell to `default_tab_target`, which answers "wherever a *new* tab should
    // start" — the last database the user picked, else the connection's first by
    // name. That is the right answer for Ctrl+T and the wrong one for a
    // statement that already names its subject: generating `employees.employees`
    // opened it bound to `bigschema`, so the toolbar contradicted the SQL and a
    // run would have gone to the wrong database (or failed, on a name that only
    // resolves in the other one). `None` still means "no particular database",
    // which is what the AI bar's free-standing snippets are.
    let open_query: Rc<dyn Fn(String, Option<String>)> = {
        let next_id = next_id.clone();
        let default_tab_target = default_tab_target.clone();
        let place_tab = place_tab.clone();
        Rc::new(move |sql: String, database: Option<String>| {
            let id = next_id.get();
            next_id.set(id + 1);
            let (conn_id, default_db) = default_tab_target();
            (place_tab)(Tab::new(cx, id, &sql, conn_id, database.or(default_db)));
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

    // `expect_disk` is what the file must still **say** for the write to go
    // ahead — `Some` only for a Save over a file this tab read, where somebody
    // else's edit would otherwise be discarded without a word. `None` means
    // "write it whatever is there", which is what a Save As the user has already
    // confirmed the overwrite for means. `sqlfile::expected_disk_text` decides
    // which of the two this is, and carries why it is the text rather than the
    // bytes.
    type FileWriteReq = (std::path::PathBuf, String, Option<String>);
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
                        && schemaic_core::sqlfile::changed_on_disk(&now, &expected)
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
            // **Every read of the tab is fallible from here down.** The Save As
            // dialog is not window-modal, so the app goes on taking input while
            // it stands open: Ctrl+W closes a clean tab with no prompt, the
            // scope is disposed a tick later, and naming a file in the dialog
            // then ran this against it. floem defines `get_untracked` as
            // `try_get_untracked().unwrap()`, so the first line panicked and took
            // every *other* tab's unsaved work with it. The guard existed
            // already, on the half of this function that is not behind a dialog.
            let (Some(format), Some(text), Some(disk), Some(tab_path)) = (
                tab.file_format.try_get_untracked(),
                tab.query.try_get_untracked(),
                tab.disk_sql.try_get_untracked(),
                tab.path.try_get_untracked(),
            ) else {
                return;
            };
            let contents = schemaic_core::sqlfile::encode(&text, format);
            // What the file must still say — `Some` only when this *is* the file
            // the tab read. `expected_disk_text` carries both halves of why:
            // that a Save As names somebody else's file, and that the comparison
            // is the text rather than the bytes.
            let expect_disk = schemaic_core::sqlfile::expected_disk_text(
                tab_path.as_deref(),
                &path,
                disk.as_deref(),
            );
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
                    schemaic_ui::activate(active, id);
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
    // The snippet library's "Open in new tab" — down here rather than with the
    // rest of the snippet actions because it needs `open_query`, which is
    // defined above this line and below those.
    let open_snippet_in_tab: Rc<dyn Fn(schemaic_core::snippet::Snippet)> = {
        let open_query = open_query.clone();
        let record_snippet_use = record_snippet_use.clone();
        Rc::new(move |snip: schemaic_core::snippet::Snippet| {
            // The tab the user is looking at decides the database, exactly as
            // an AI code block does: a brand-new tab would otherwise land on
            // whichever database a new tab starts on.
            let db = active_tab().and_then(|t| t.database.get_untracked());
            (open_query)(snip.body.clone(), db);
            (record_snippet_use)(snip.id);
        })
    };

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
    let save_ui: Rc<dyn Fn()> = Rc::new({
        let pending_legacy_hidden = pending_legacy_hidden.clone();
        move || {
            persist::save_ui_state(&UiState {
                // Same bargain as `hidden_dbs` below: the legacy flat field is
                // written empty only once the migration has read it, and carried
                // back out unchanged until then, so a launch that could not
                // migrate leaves the upgrade to a later one rather than
                // collapsing every tree permanently.
                expanded: (*pending_legacy_expanded).clone(),
                expanded_rules: expanded_rules.get_untracked(),
                // The legacy flat field is written empty **once the migration has
                // actually read it** — `hidden_db_rules` is the truth after that.
                // Until then it is carried back out unchanged, so a launch that
                // could not migrate (no connections loaded) leaves the upgrade to a
                // later one instead of erasing it.
                hidden_dbs: (*pending_legacy_hidden).clone(),
                hidden_db_rules: hidden_db_rules.get_untracked(),
                schema_visible: schema_visible.get_untracked(),
                right_panel: right_panel.get_untracked().into(),
                activity_intervals: activity_intervals.get_untracked(),
                schema_w: schema_w.get_untracked(),
                right_w: right_w.get_untracked(),
                editor_h: editor_h.get_untracked(),
                // **The unknown key survives the save**, or the field's own promise
                // — "an unrecognised value is *not* silently replaced with the
                // default" — is false one save later. Cleared the moment the user
                // picks a harness themselves, which is the point at which the file
                // should start naming what is actually running.
                ai_harness: persist::ai_harness_to_persist(
                    ai_harness_unknown.get_untracked().as_deref(),
                    ai_harness.get_untracked().key(),
                ),
                ai_cli_path: ai_cli_path.get_untracked(),
                ai_model: ai_model.get_untracked(),
                ai_effort: ai_effort.get_untracked().cli().to_string(),
                ai_instructions: ai_instructions.get_untracked(),
                ai_schema_scope: ai_schema_scope.get_untracked().key().to_string(),
                ai_gutter: ai_gutter.get_untracked(),
                ai_run_queries: legacy_ai_run_queries,
                ui_theme: ui_theme.get_untracked().key().to_string(),
                editor_theme: editor_theme.get_untracked().key().to_string(),
                ui_scale: ui_scale.get_untracked().key().to_string(),
                editor_font_size: editor_font.get_untracked(),
                row_limit: row_limit.get_untracked(),
                statement_timeout_secs: statement_timeout.get_untracked(),
                confirm_writes: confirm_writes.get_untracked(),
                tab_width: tab_width.get_untracked(),
                soft_tabs: soft_tabs.get_untracked(),
                word_wrap: word_wrap.get_untracked(),
                restore_tabs: restore_tabs.get_untracked(),
                live_validate: live_validate.get_untracked(),
                show_table_sizes: table_sizes.get_untracked(),
            });
        }
    });

    // Persist the layout whenever a panel is toggled (the footer chips mutate
    // these signals directly, so we react rather than route through a callback).
    {
        let save_ui = save_ui.clone();
        create_effect(move |_| {
            schema_visible.get();
            right_panel.get();
            table_sizes.get();
            activity_intervals.get();
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
            ui_scale.get();
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
            statement_timeout.get();
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
            let conn_id = active_conn.get_untracked();
            hidden_db_rules.update(move |rules| {
                schemaic_core::db_hidden::toggle(rules, conn_id, &db);
            });
            save_ui();
        })
    };

    // Collapse every node (databases + tables): clear the whole expanded set.
    let collapse_all: Rc<dyn Fn()> = {
        let save_ui = save_ui.clone();
        Rc::new(move || {
            // **Guarded**, like every clear in `schemaic-ui`. `update` notifies
            // unconditionally, so collapsing an already-collapsed tree rebuilt
            // every mounted subtree *and* wrote `ui.json` to disk for a click
            // that changed nothing. The guard was module-private to `grid.rs`
            // and unreachable from here until it moved to `widgets`.
            let before = expanded.with_untracked(|set| set.is_empty());
            schemaic_ui::clear_if_any(expanded);
            if !before {
                save_ui();
            }
        })
    };

    // Collapse everything under one database, keeping the DB node itself open.
    // `key_under` owns which key families that is — this used to drop
    // `tbl:<database>:*` alone, leaving object folders and PostgreSQL
    // namespace groups open with their rows on screen.
    let collapse_db: Rc<dyn Fn(String)> = {
        let save_ui = save_ui.clone();
        Rc::new(move |db: String| {
            // Same rule as `collapse_all` above, and the reason `retain_if_any`
            // exists: a `retain` reads as conditional but is not — one that
            // keeps everything still calls `update`, and `update` still
            // notifies. Collapsing a database with nothing expanded under it
            // rebuilt the whole tree and saved to disk.
            let had_any =
                expanded.with_untracked(|set| set.iter().any(|k| schemaic_ui::key_under(&db, k)));
            schemaic_ui::retain_if_any(expanded, move |k| !schemaic_ui::key_under(&db, k));
            if had_any {
                save_ui();
            }
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
            // **Both early returns answer the continuation before leaving.**
            // Dropping an `Rc<dyn Fn(bool)>` does nothing, so a `with_conn`-gated
            // control that landed here simply did *nothing at all* — no query, no
            // modal, no log — which is precisely what `with_conn` exists to
            // prevent. `false` is the honest answer: the endpoint genuinely
            // cannot be reached at this instant, and `with_conn` then says so.
            let answer_none = |done: &Option<CheckDoneFn>| {
                conn_status.set(ConnStatus::Unknown);
                if let Some(f) = done {
                    f(false);
                }
            };
            let Some(conn) =
                connections.with_untracked(|cs| cs.iter().find(|c| c.id == id).cloned())
            else {
                answer_none(&done);
                return;
            };
            // Effective endpoint — through the tunnel for SSH connections. If the
            // tunnel isn't up yet, stay Unknown; a later tick will catch it.
            let tunnel = if conn.uses_tunnel() {
                match tunnels.borrow().get(&conn.id).map(|h| h.port()) {
                    Some(port) => Some(port),
                    None => {
                        answer_none(&done);
                        return;
                    }
                }
            } else {
                None
            };
            let db = Db::connect(&conn, tunnel);
            // Stamped before the ping, checked when it lands: up to five seconds
            // pass in between, and the connection the user is on can change
            // inside them.
            let stamp = (id, health_gen.get_untracked() + 1);
            health_gen.set(stamp.1);
            let send = create_ext_action(cx, move |ok: bool| {
                let now = (active_conn.get_untracked(), health_gen.get_untracked());
                // The whole decision is `check_outcome`; this closure is a
                // `match` on it and holds no rule of its own. Anything inserted
                // between the two halves below belongs in that function, where a
                // test can see it.
                let outcome = check_outcome(stamp, now, ok);
                if outcome.write_status {
                    conn_status.set(if ok {
                        ConnStatus::Connected
                    } else {
                        ConnStatus::Disconnected
                    });
                    // Every check counts toward the backoff, not just the polled
                    // ones — a user hammering Retry against a dead host shouldn't
                    // reset the timer's patience either.
                    health_failures.set(health::record(health_failures.get_untracked(), ok));
                }
                if let Some(answer) = outcome.answer
                    && let Some(f) = &done
                {
                    f(answer);
                }
            });
            handle.spawn(async move {
                let ok = db.ping(schemaic_db::PING_TIMEOUT).await.is_ok();
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
    //
    // The refusal-carrying form is the real one; `with_conn` is it with nobody
    // listening. Any caller that has already told the user something is in
    // flight takes this instead — see [`Refusal`].
    let with_conn_else: ConnGateElse = {
        let check_conn_then = check_conn_then.clone();
        Rc::new(move |action: Rc<dyn Fn()>, refused: Rc<dyn Fn()>| {
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
                // A *superseded* failure still lands here — dropping it upstream
                // is what makes Run do nothing at all. What it must not do is
                // raise "Not connected" over a header a newer check has since
                // set to Connected, so the refusal re-reads the status: if
                // something more recent than our own ping says the server is up,
                // that is the better answer and the action proceeds.
                if ok || !conn_status.get_untracked().is_down() {
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
                    refused();
                }
            })));
        })
    };
    let with_conn: ConnGate = {
        let g = with_conn_else.clone();
        Rc::new(move |action: Rc<dyn Fn()>| (g)(action, Rc::new(|| {})))
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
            connections.with_untracked(|cs| schemaic_core::connection::by_id(cs, cid).cloned())
        });
        // **This closure gathers signals; it does not decide.** The decision is
        // `GuardPolicy::of`, in core with its tests — it was here, inside a
        // 9,600-line function where nothing is nameable, callable or testable,
        // which is why the write guard's *verdict* was pure and tested while its
        // three *inputs* were neither.
        GuardPolicy::of(conn.as_ref(), no_database, confirm_writes.get_untracked())
    };
    // Said when a deferred run lands on a tab the user has since left. It is a
    // refusal, so it has to be visible: the alternative — running anyway — is
    // the defect, and running *nothing* silently is how the defect went
    // unnoticed for as long as it did.
    let run_moved_on: Rc<dyn Fn()> = Rc::new(move || {
        error_modal_text.set(Some(
            "You switched tabs while the connection was being re-checked, so the \
             statement was not run. It was checked against the tab it was typed \
             in, and running it here would run it somewhere else. Go back to that \
             tab and run it again."
                .to_string(),
        ));
        error_modal_open.set(true);
    });
    // The connection-gated but *unguarded* pair. Only the two wrappers below and
    // "Run anyway" reach them; nothing outside this crate can.
    //
    // **Pinned to the tab**, because the gate can hold them for five seconds and
    // they re-resolve `active` when they land — see `gate1_on_tab`.
    let gated_run = gate1_on_tab(&with_conn, &run, active, &run_moved_on);
    let gated_run_all = gate1_on_tab(&with_conn, &run_all, active, &run_moved_on);

    // The active tab's parameter values, for the substitution that precedes the
    // guard. Untracked: this reads at the moment of a run, not reactively.
    let tab_bindings = move || {
        let id = active.get_untracked();
        tabs.with_untracked(|v| {
            v.iter()
                .find(|t| t.id == id)
                .map(|t| t.params.get_untracked())
                .unwrap_or_default()
        })
    };
    let guarded_run: Rc<dyn Fn(String)> = {
        let gated_run = gated_run.clone();
        Rc::new(move |sql: String| {
            // Substitute, *then* guard — `params::prepare_run` is the pair, and
            // what comes back is what runs. A `Raw` value expands to arbitrary
            // SQL, so a guard shown the template would be judging a statement
            // the engine never receives.
            let prepared =
                params::prepare_run(std::slice::from_ref(&sql), &tab_bindings(), guard_policy());
            let (mut stmts, verdict) = match prepared {
                Ok(pair) => pair,
                // A parameter with no value is a hard hold: there is nothing to
                // run yet, so the bar offers no "Run anyway".
                Err(e) => {
                    run_guard.set(Some(RunGuard {
                        message: e.to_string(),
                        pending: None,
                    }));
                    return;
                }
            };
            let sql = stmts.pop().unwrap_or(sql);
            match verdict {
                RunVerdict::Allow => {
                    // **A run that goes through clears the bar it got past.**
                    // Nothing else does: the only two writers of `None` are the
                    // tab-switch effect and "Run anyway", and neither is reachable
                    // from filling a parameter in. So "No value for :id" — the
                    // hold this very function raises — stayed on screen after the
                    // value was typed and the query ran, describing a state that
                    // no longer existed.
                    if run_guard.get_untracked().is_some() {
                        run_guard.set(None);
                    }
                    (gated_run)(sql)
                }
                RunVerdict::Block(message) => run_guard.set(Some(RunGuard {
                    message,
                    pending: None,
                })),
                RunVerdict::Confirm(message) => run_guard.set(Some(RunGuard {
                    message,
                    // The *substituted* statement is parked, so "Run anyway"
                    // replays what was judged rather than re-deriving it.
                    pending: Some(PendingRun::Single(sql)),
                })),
            }
        })
    };
    let guarded_run_all: Rc<dyn Fn(Vec<String>)> = {
        let gated_run_all = gated_run_all.clone();
        Rc::new(move |stmts: Vec<String>| {
            let (stmts, verdict) =
                match params::prepare_run(&stmts, &tab_bindings(), guard_policy()) {
                    Ok(pair) => pair,
                    Err(e) => {
                        run_guard.set(Some(RunGuard {
                            message: e.to_string(),
                            pending: None,
                        }));
                        return;
                    }
                };
            match verdict {
                RunVerdict::Allow => {
                    // Same as `guarded_run`: the batch that gets past the bar is
                    // what takes it down.
                    if run_guard.get_untracked().is_some() {
                        run_guard.set(None);
                    }
                    (gated_run_all)(stmts)
                }
                RunVerdict::Block(message) => run_guard.set(Some(RunGuard {
                    message,
                    pending: None,
                })),
                RunVerdict::Confirm(message) => run_guard.set(Some(RunGuard {
                    message,
                    pending: Some(PendingRun::Batch(stmts)),
                })),
            }
        })
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

    /// How many whole-database catalogue reads may be in flight at once.
    ///
    /// Small on purpose. Each one is a connection of its own plus five
    /// catalogue queries over every table in a database, and the thing being
    /// protected is a *shared* server: four is enough to keep the tree filling
    /// in visibly while leaving a hosting account's connection allowance to
    /// the queries the user is actually waiting on.
    const INTROSPECT_PERMITS: usize = 4;

    /// The permits, shared by every schema fetch in the process.
    ///
    /// Process-wide rather than per connection, because the limit is about the
    /// client's own burst — switching connections while a load is out would
    /// otherwise double it, and that is exactly the moment two full loads
    /// overlap.
    fn introspect_permits() -> Arc<tokio::sync::Semaphore> {
        static PERMITS: std::sync::LazyLock<Arc<tokio::sync::Semaphore>> =
            std::sync::LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(INTROSPECT_PERMITS)));
        PERMITS.clone()
    }

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
            let permits = introspect_permits();
            handle.spawn(async move {
                // **At most `INTROSPECT_PERMITS` catalogue reads at once.**
                // `fetch_schema` opens a connection of its own and reads every
                // column, index, key, view, check and trigger of a whole
                // database, and the connection load starts one per database
                // with nothing between them. On a shared host with 200 user
                // databases that is 200 simultaneous handshakes against a
                // server whose `max_connections` is 151 out of the box: the
                // 152nd onward failed ERROR 1040 and rendered as `Failed` rows
                // whose only retry is a manual Refresh, which repeats the
                // storm. `db::sqlite`'s `probe_permit` serialises the same
                // class of work for the same reason.
                //
                // Acquired inside the task, so the fan-out loop still returns
                // at once and the tree shows every database as loading.
                let _permit = permits.acquire_owned().await;
                // A fresh token: the tree's own refresh has no Stop to offer, so
                // there is nothing to cancel it with. `fetch_schema` takes one
                // for the Export modal, which does.
                let st = match db.fetch_schema(&database, CancellationToken::new()).await {
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
        let clear_schema_tree = clear_schema_tree.clone();
        Rc::new(move |conn: Connection| {
            // Reloading the connection already on screen (the SCHEMA header's
            // Refresh) keeps its databases visible while the list is re-fetched;
            // only a *switch* clears, where the rows would otherwise be another
            // server's for as long as the connect takes.
            // `is_reload_of`, not `targets_same_server`: the id term is *this
            // caller's* policy — a switch to a different connection that
            // happens to reach the same server must still clear, because those
            // rows belong to the other entry. It used to live inside the shared
            // predicate, where it made the killed-session repair's call dead.
            let reload = nodes_conn
                .borrow()
                .as_ref()
                .is_some_and(|c| c.is_reload_of(&conn));
            if !reload {
                // **The whole clear, not just the rows.** Leaving the scope and
                // `nodes_conn` behind is what let a switch away and back inside
                // one `fetch_databases` orphan a generation: the return trip
                // reads `is_reload_of` as true against an already-empty node
                // list, so the load that lands rebuilds every node inside the
                // scope that still owns the first set's signals.
                (clear_schema_tree)();
            }
            let stamp = (conn.id, schema_gen.get() + 1);
            schema_gen.set(stamp.1);
            let gen_cb = schema_gen.clone();
            let nodes_scope_cb = nodes_scope.clone();
            let nodes_conn_cb = nodes_conn.clone();
            let clear_schema_tree_cb = clear_schema_tree.clone();
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
                        let plans = plan_nodes(&by_id, &names, reload);
                        let nodes: Vec<ConnNode> = plans
                            .iter()
                            .copied()
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
                        // **The nodes the plan left behind, when the scope above
                        // them is staying.** A database dropped from another
                        // client disappears from the tree while its
                        // `Arc<DbSchema>` sits in the kept scope, unreachable and
                        // retained for the session — the leak this arm's own
                        // comment below named and said needed a scope per node.
                        // On a *switch* nothing is asked: `node_cx` is fresh and
                        // the deferred `old.dispose()` a few lines down takes the
                        // whole previous generation, children included.
                        let departing: Vec<floem::reactive::Scope> = kept_scope
                            .map(|_| departed_nodes(&by_id, &plans))
                            .unwrap_or_default()
                            .into_iter()
                            .filter_map(|id| existing.iter().find(|n| n.id == id).map(|n| n.cx))
                            .collect();
                        db_nodes.set(nodes.clone());
                        if !departing.is_empty() {
                            // Deferred, like every other dispose of these: the
                            // tree rebuilds off the new list first, so nothing
                            // reads a freed signal on the way past.
                            exec_after(Duration::ZERO, move |_| {
                                for cx in departing {
                                    cx.dispose();
                                }
                            });
                        }
                        if kept_scope.is_none()
                            && let Some(old) = nodes_scope_cb.borrow_mut().replace(node_cx)
                        {
                            exec_after(Duration::ZERO, move |_| old.dispose());
                        }
                        *nodes_conn_cb.borrow_mut() = Some(conn_send.clone());
                        // Bind any tab of THIS connection that doesn't yet have a
                        // database (e.g. the initial tab) to the first database
                        // **the SCHEMA panel would show** — see
                        // `schema::first_bindable` for why the filter belongs
                        // here of all places.
                        if let Some(first) = hidden_dbs
                            .with_untracked(|h| {
                                schemaic_core::schema::first_bindable(
                                    Some(conn_send.database.as_str()),
                                    &names,
                                    h,
                                )
                                .map(str::to_string)
                            })
                            .as_ref()
                        {
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
                        // **Most-wanted first.** `start_fetch` bounds how many
                        // of these run at once (`INTROSPECT_PERMITS`), so the
                        // order the queue is filled in decides which
                        // database's tables appear first. Unordered, a shared
                        // host's two-hundredth database could be read before
                        // the one the user opened the connection to look at.
                        let order = {
                            let open: HashSet<String> = expanded.with_untracked(|e| {
                                e.iter()
                                    .filter_map(|k| {
                                        schemaic_ui::db_name_of_key(k).map(str::to_string)
                                    })
                                    .collect()
                            });
                            hidden_dbs.with_untracked(|h| {
                                schemaic_core::schema::introspection_order(
                                    &names,
                                    Some(conn_send.database.as_str()),
                                    &open,
                                    h,
                                )
                            })
                        };
                        for i in order {
                            (start_fetch_cb)(&nodes[i], db.clone());
                        }
                    }
                    Err(e) => {
                        tracing::error!("schema load failed: {e}");
                        // Same rule on this side: clearing the tree here would empty
                        // it for a connection that loaded perfectly well.
                        if landing == LoadLanding::Install {
                            // **And forget which connection the tree was for** —
                            // this arm is where that was first diagnosed, and
                            // `clear_schema_tree` is where the three steps now
                            // live so the other two clears cannot omit them.
                            //
                            // A database *dropped* between two successful reloads
                            // still leaves its signals in the surviving scope; that
                            // one needs a scope per node and is not this fix.
                            (clear_schema_tree_cb)();
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

    // One `SHOW CREATE {PROCEDURE|FUNCTION}`, for the routine the user just
    // opened. The counterpart of `trigger_source` above and, like it, not an
    // optimisation — `information_schema` resolves the body's escapes, and every
    // MySQL routine edit begins with a `DROP` that commits on its own.
    let routine_source: schemaic_ui::RoutineSrcFn = {
        let handle = handle.clone();
        let db_for = db_for.clone();
        Rc::new(
            move |req: schemaic_ui::RoutineSrcRequest, done: schemaic_ui::RoutineSrcDoneFn| {
                let Ok(db) = db_for(req.conn_id) else {
                    // Nothing to report — but the editor is *waiting* on this
                    // callback, so it has to arrive either way; dropping it
                    // leaves Preview disabled for the life of the modal.
                    (done)(req.name.clone(), None);
                    return;
                };
                let name = req.name.clone();
                let report = create_ext_action(cx, move |src| (done)(name.clone(), src));
                handle.spawn(async move {
                    // A failed read leaves the editor on `information_schema`'s
                    // body, which is better than refusing to open at all — and
                    // is what a role without `SHOW_ROUTINE` will always get.
                    let src = db
                        .routine_source(Some(&req.database), req.kind, &req.name)
                        .await
                        .unwrap_or(None);
                    report(src);
                });
            },
        )
    };

    // One `SHOW CREATE EVENT`, for the event the user just opened. The
    // counterpart of `routine_source` above and, like it, not an optimisation:
    // `information_schema` resolves the body's escapes, and it is also the only
    // place the event's `time_zone` and session state are printed together.
    let event_source: schemaic_ui::EventSrcFn = {
        let handle = handle.clone();
        let db_for = db_for.clone();
        Rc::new(
            move |req: schemaic_ui::EventSrcRequest, done: schemaic_ui::EventSrcDoneFn| {
                let Ok(db) = db_for(req.conn_id) else {
                    // Nothing to report — but the editor is *waiting* on this
                    // callback, so it has to arrive either way; dropping it
                    // leaves Preview disabled for the life of the modal.
                    (done)(req.name.clone(), None);
                    return;
                };
                let name = req.name.clone();
                let report = create_ext_action(cx, move |src| (done)(name.clone(), src));
                handle.spawn(async move {
                    // A failed read leaves the editor on `information_schema`'s
                    // body, which is better than refusing to open at all.
                    let src = db
                        .event_source(Some(&req.database), &req.name)
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
    // Schema compare's signals. Declared here, not inline in the `OverlayUi`
    // literal the way `erd` is, because the two actions below write them and
    // the staleness check reads them back.
    let compare: RwSignal<Option<schemaic_ui::CompareTarget>> = RwSignal::new(None);
    let compare_state: RwSignal<schemaic_ui::CompareState> =
        RwSignal::new(schemaic_ui::CompareState::Idle);
    // The reading state the fetch seeds when a comparison lands, and the
    // overlay owns from then on.
    let compare_selected = RwSignal::new(std::collections::HashSet::<String>::new());
    let compare_expanded = RwSignal::new(std::collections::HashSet::<String>::new());
    let compare_dbs: RwSignal<Option<(u64, Vec<String>)>> = RwSignal::new(None);
    let compare_dbs_err: RwSignal<Option<String>> = RwSignal::new(None);

    // One connection's databases, for the compare picker's second step. Asked
    // of the server rather than read off `db_nodes`, which only ever holds the
    // *active* connection's list — see `OverlayUi::compare_dbs`.
    //
    // **Which listing is the current one.** Bumped on every ask, checked in
    // both arms of the landing — see there.
    let compare_dbs_gen: Rc<Cell<u64>> = Rc::new(Cell::new(0));
    let compare_list_dbs: Rc<dyn Fn(u64)> = {
        let handle = handle.clone();
        let db_for = db_for.clone();
        let compare_dbs_gen = compare_dbs_gen.clone();
        Rc::new(move |conn_id: u64| {
            // Every ask supersedes the one before it. `compare_fetch` has this
            // and this call had it in **neither** arm: a listing the user
            // abandoned — they clicked a second server while the first was
            // still out — landed late and replaced whatever was on screen. Its
            // *success* arm replaced the picker under the pointer, so the click
            // already on its way retargeted to a different server's database;
            // its *failure* arm replaced a fully rendered comparison with an
            // error about a server the user had moved on from.
            let my_gen = compare_dbs_gen.get().wrapping_add(1);
            compare_dbs_gen.set(my_gen);
            let mine = compare_dbs_gen.clone();
            // **Guarded, because `set` never dedups.** The modal's body is a
            // `dyn_container` keyed partly on this signal, so writing `None`
            // over `None` disposed and rebuilt the filter bar, the whole row
            // tree and the diff pane — losing the tree's scroll position for a
            // listing that had not even been sent yet.
            if compare_dbs_err.get_untracked().is_some() {
                compare_dbs_err.set(None);
            }
            let db = match db_for(conn_id) {
                Ok(db) => db,
                Err(e) => {
                    compare_dbs_err.set(Some(e));
                    return;
                }
            };
            // **This connection's hide rules, not the active connection's.**
            // `hidden_dbs` is a memo over `active_conn`, so filtering another
            // server's list through it hid a database that server really has
            // and offered one the user had hidden on it. On SQLite, where the
            // list is the single `main`, hiding `main` on the active connection
            // emptied every other SQLite connection's list outright.
            let hidden = hidden_db_rules
                .with_untracked(|rules| schemaic_core::db_hidden::names_for(rules, conn_id));
            let report = create_ext_action(cx, move |res: Result<Vec<String>, String>| {
                // Superseded: the user asked for another server's databases, or
                // closed the picker. Both arms, because both write to signals a
                // later ask has already filled.
                if mine.get() != my_gen {
                    return;
                }
                match res {
                    // Hidden databases are left out for the reason every other
                    // database list leaves them out: the user said they didn't
                    // want to see that one, and a picker is a list.
                    Ok(names) => compare_dbs.set(Some((
                        conn_id,
                        names
                            .into_iter()
                            .filter(|n| schemaic_core::schema::db_visible(&hidden, n))
                            .collect(),
                    ))),
                    Err(e) => compare_dbs_err.set(Some(e)),
                }
            });
            handle.spawn(async move {
                report(db.fetch_databases().await.map_err(|e| e.to_string()));
            });
        })
    };

    // Introspect both sides and compare them.
    //
    // **Its own fetch, not the tree's.** The ER diagram reads whatever the tree
    // has already loaded and says "not loaded yet" otherwise; that would rule
    // out this feature's whole point, since the right-hand side is routinely a
    // database on another connection the tree has never touched.
    //
    // **A token, though there is no Stop button.** Two schemas landing out of
    // order is already handled the way the properties fetch handles it — the
    // answer is dropped unless the target it was asked for is still the one on
    // screen — but dropping the *answer* still pays for the *read*, and this
    // read is two full `fetch_schema` sweeps across two servers. Picking a
    // different right-hand side three times left three of them running to
    // completion for nobody, on the same reasoning this range's own blob wiring
    // records as a fixed bug.
    let compare_token: Rc<RefCell<CancellationToken>> =
        Rc::new(RefCell::new(CancellationToken::new()));
    // **The token's other canceller.** `compare_fetch` below cancels its own
    // predecessor on the way in, which made every canceller a *new* fetch — so
    // closing the modal, the one path that starts nothing, left two full
    // `fetch_schema` sweeps running for nobody. Wired to the modal's close.
    let compare_cancel: Rc<dyn Fn()> = {
        let compare_token = compare_token.clone();
        Rc::new(move || compare_token.borrow().cancel())
    };
    let compare_fetch: Rc<dyn Fn(schemaic_ui::CompareTarget)> = {
        let handle = handle.clone();
        let db_for = db_for.clone();
        let compare_token = compare_token.clone();
        Rc::new(move |target: schemaic_ui::CompareTarget| {
            // Whatever was in flight is for a pair nobody is looking at.
            let token = {
                let mut slot = compare_token.borrow_mut();
                slot.cancel();
                *slot = CancellationToken::new();
                slot.clone()
            };
            let Some(right) = target.right.clone() else {
                compare_state.set(schemaic_ui::CompareState::Idle);
                return;
            };
            let (left_db, right_db) = match (db_for(target.left.conn_id), db_for(right.conn_id)) {
                (Ok(a), Ok(b)) => (a, b),
                (Err(e), _) | (_, Err(e)) => {
                    compare_state.set(schemaic_ui::CompareState::Failed(e));
                    return;
                }
            };
            // Refused before either round trip, and by **dialect** rather than
            // engine: MySQL and MariaDB compare perfectly well, three different
            // dialects do not map onto each other at all. Two full schema reads
            // to then say "these can't be compared" would be the same answer,
            // slower.
            let (ld, rd) = (left_db.engine().dialect(), right_db.engine().dialect());
            if let Err(why) = schemaic_core::compare::comparable(ld, rd) {
                compare_state.set(schemaic_ui::CompareState::Failed(why));
                return;
            }

            compare_state.set(schemaic_ui::CompareState::Loading);
            let want = target.clone();
            let report = create_ext_action(
                cx,
                move |res: Result<
                    (
                        Box<schemaic_core::schema::DbSchema>,
                        Box<schemaic_core::schema::DbSchema>,
                    ),
                    String,
                >| {
                    // The pair on screen changed while these were in flight —
                    // someone picked a different right-hand side, or closed the
                    // modal. The landing is for a question nobody is asking.
                    if compare.with_untracked(|t| t.as_ref() != Some(&want)) {
                        return;
                    }
                    match res {
                        Ok((l, r)) => {
                            let c = schemaic_core::compare::SchemaComparison::of(&l, &r, ld);
                            // Seed the reading: every kind that differs opens,
                            // and everything that *can* be applied arrives
                            // ticked — a comparison is opened to migrate the
                            // difference, so making the common case the default
                            // beats an empty tree with a dead button under it.
                            //
                            // **Through `selectable_keys`, not a filter written
                            // here.** That is the function for "every difference
                            // a plan could include" — the same question the
                            // footer's count and the Apply button ask through
                            // `is_planned` — and this seed used to re-spell half
                            // of it (`!needs_source()`, without `unplannable()`),
                            // in a third crate and outside every test. No filter
                            // is on screen yet, so `RowFilter::default()`.
                            compare_expanded.set(c.default_expanded());
                            compare_selected.set(
                                c.selectable_keys(schemaic_core::compare::RowFilter::default())
                                    .into_iter()
                                    .collect(),
                            );
                            compare_state
                                .set(schemaic_ui::CompareState::Ready(std::rc::Rc::new(c)));
                        }
                        Err(e) => compare_state.set(schemaic_ui::CompareState::Failed(e)),
                    }
                },
            );
            let (ldb, rdb) = (target.left.database.clone(), right.database.clone());
            handle.spawn(async move {
                // **Concurrently.** The two reads are independent — one `Db`
                // handle each, one connection per operation, routinely on two
                // different servers — so nothing orders them, and awaiting in
                // sequence made the user wait left *plus* right for a catalogue
                // read that is seconds on a large database. Each error keeps
                // its own "Reading {db}" prefix, so a failure still says which
                // side could not be read.
                let left = async {
                    left_db
                        .fetch_schema(&ldb, token.clone())
                        .await
                        .map_err(|e| format!("Reading {ldb}: {e}"))
                };
                let right = async {
                    right_db
                        .fetch_schema(&rdb, token.clone())
                        .await
                        .map_err(|e| format!("Reading {rdb}: {e}"))
                };
                match tokio::try_join!(left, right) {
                    Ok((l, r)) => report(Ok((Box::new(l), Box::new(r)))),
                    Err(e) => report(Err(e)),
                }
            });
        })
    };

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
                    // direction.
                    //
                    // Through `warm_stats_slot`, which is where the "only a
                    // vacant slot" rule and the disposal guard live: this
                    // closure keeps `slot` across an await, so a connection
                    // switch in between frees the signal it points at — and the
                    // `matches!(slot.get_untracked(), …)` this replaced was
                    // `try_get_untracked().unwrap()` on a `None`, a panic that
                    // took the window and every tab's uncommitted edits.
                    if let (Some(slot), Ok(set)) = (slot, res) {
                        schemaic_ui::warm_stats_slot(slot, set);
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
            // **`Option<String>` for the error, so the cancel is a variant and
            // not a sentence.** This matched `e == DbError::Cancelled.to_string()`
            // — the only place in the workspace that recognised a typed error by
            // its rendered `Display` — so rewording `#[error("query cancelled")]`,
            // a copy edit with nothing to fail, would have turned a cancelled
            // count into a visible error in the panel. The cause was an ordering
            // slip rather than a missing abstraction: `map_err(|e| e.to_string())`
            // ran in the spawned task, *before* the closure that has to tell the
            // variants apart, so the closure had nothing left but text. Two other
            // sites in this file get the order right; this one now does too.
            let report = create_ext_action(cx, move |res: Result<u64, Option<String>>| {
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
                    // A cancelled count is not a failure to report: the user
                    // asked for it to stop, and the estimate they already have
                    // is what the panel goes back to showing.
                    Err(None) => {}
                    Err(Some(e)) => properties_count_err.set(Some(e)),
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
                    // Discriminated while still typed, erased after — the whole
                    // point of the `Option` above.
                    .map_err(|e| match e {
                        schemaic_db::DbError::Cancelled => None,
                        other => Some(other.to_string()),
                    });
                report(res);
            });
        })
    };

    // The Users and privileges browser's list.
    let principals: Rc<dyn Fn(schemaic_ui::UsersTarget)> = {
        let handle = handle.clone();
        let db_for = db_for.clone();
        Rc::new(move |target: schemaic_ui::UsersTarget| {
            let db = match db_for(target.conn_id) {
                Ok(db) => db,
                Err(e) => {
                    users_state.set(schemaic_ui::UsersState::Failed(e));
                    return;
                }
            };
            // Asked before the round trip, for the reason `table_stats` asks
            // `supports_table_stats` before its own: an engine that has no
            // accounts has a different thing to say than one whose fetch failed,
            // and no query can tell the two apart.
            if !schemaic_core::users::supports_users(db.engine().dialect()) {
                users_state.set(schemaic_ui::UsersState::Unsupported);
                return;
            }
            let want = target.clone();
            // **Claimed before the round trip, compared when it lands.** Target
            // identity alone was not enough: two fetches on an *identical*
            // target are routine — creating an account is one, and closing the
            // preview afterwards is another — so the later request was not
            // guaranteed to be the last writer, and the list could settle on the
            // pre-mutation snapshot. `DdlUi::generation` is the same guard for
            // the same failure one modal up.
            let generation = users_generation.get_untracked() + 1;
            users_generation.set(generation);
            let report = create_ext_action(
                cx,
                move |res: Result<schemaic_core::users::Principals, String>| {
                    // The browser has since closed, or reopened on another
                    // server: this answer is about neither.
                    if users.with_untracked(|t| t.as_ref() != Some(&want)) {
                        return;
                    }
                    // …or a later fetch has since been issued, and this one's
                    // answer is older than the screen.
                    if users_generation.get_untracked() != generation {
                        return;
                    }
                    users_state.set(match res {
                        Ok(list) => schemaic_ui::UsersState::Loaded(list),
                        Err(e) => schemaic_ui::UsersState::Failed(e),
                    });
                },
            );
            handle.spawn(async move {
                let res = db.fetch_principals().await.map_err(|e| e.to_string());
                report(res);
            });
        })
    };

    // One account's privileges, per selection. Separate from `principals` for the
    // reason `count_rows` is separate from `table_stats`: it is a second round
    // trip the user asked for by picking a row, and a failure to read one
    // account's grants must not replace the list of accounts with an error.
    let grants: Rc<dyn Fn(schemaic_ui::UsersTarget, schemaic_core::users::Principal)> = {
        let handle = handle.clone();
        let db_for = db_for.clone();
        Rc::new(
            move |target: schemaic_ui::UsersTarget, principal: schemaic_core::users::Principal| {
                let db = match db_for(target.conn_id) {
                    Ok(db) => db,
                    Err(e) => {
                        users_grants.set(schemaic_ui::GrantsState::Failed(e));
                        return;
                    }
                };
                let want = principal.clone();
                let want_target = target.clone();
                let report = create_ext_action(
                    cx,
                    move |res: Result<schemaic_core::users::Grants, String>| {
                        // Both halves have to still be true: the browser may have
                        // closed, and the user may have clicked a second account
                        // while this one was in flight — in which case this
                        // answer belongs to a row that is no longer selected.
                        if users.with_untracked(|t| t.as_ref() != Some(&want_target))
                            || users_selected.with_untracked(|p| p.as_ref() != Some(&want))
                        {
                            return;
                        }
                        users_grants.set(match res {
                            Ok(g) => schemaic_ui::GrantsState::Loaded(g),
                            Err(e) => schemaic_ui::GrantsState::Failed(e),
                        });
                    },
                );
                handle.spawn(async move {
                    let res = db
                        .fetch_grants(target.database.as_deref(), &principal)
                        .await
                        .map_err(|e| e.to_string());
                    report(res);
                });
            },
        )
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
            //
            // `wants_db_stats`, not `open.contains(db_key(..))`: hiding a
            // database with the SCHEMA eye leaves its `db:` key in the set and
            // its node in `db_nodes` (the tree filters visibility at render),
            // so this kept paying for an `information_schema.tables` aggregate
            // nothing renders. Tracking `hidden_dbs` is what makes
            // hiding/unhiding re-decide.
            let pending: Vec<(String, RwSignal<schemaic_ui::DbStatsState>)> =
                hidden_dbs.with(|hidden| {
                    expanded.with(|open| {
                        db_nodes.with(|nodes| {
                            nodes
                                .iter()
                                .filter(|n| schemaic_ui::wants_db_stats(open, hidden, &n.database))
                                .filter(|n| {
                                    n.stats.get_untracked() == schemaic_ui::DbStatsState::Idle
                                })
                                .map(|n| (n.database.clone(), n.stats))
                                .collect()
                        })
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

    // The server's roles, for the database editor's Owner shortcut. The same
    // shape `trigger_functions` has, minus the database: `pg_roles` is a
    // cluster-wide catalogue, and the caller may be about to create a database
    // that does not exist yet.
    let roles: schemaic_ui::RolesFn = {
        let handle = handle.clone();
        let db_for = db_for.clone();
        Rc::new(move |conn_id: u64, done: schemaic_ui::RolesDoneFn| {
            let Ok(db) = db_for(conn_id) else {
                // Nothing to report: the Owner field is free text and keeps
                // whatever is typed in it.
                return;
            };
            let report = create_ext_action(cx, move |roles| (done)(roles));
            handle.spawn(async move {
                // A failure here costs a menu, never a value — see `RolesFn`.
                report(db.roles().await.unwrap_or_default());
            });
        })
    };

    // The token the running apply observes, so the preview modal's exit can
    // stop it where stopping means something — see `ddl::ddl_rolls_back_as_a_whole`.
    // The same shape as `script_token` above, and for the same reason: the token
    // belongs to the run, and the UI holds only a way to fire it.
    let ddl_token: Rc<RefCell<Option<CancellationToken>>> = Rc::new(RefCell::new(None));

    // Fire the apply's token. The *decision* whether an exit may reach this is
    // `ddl::ddl_rolls_back_as_a_whole`, asked in the modal; this only does it.
    let ddl_cancel: Rc<dyn Fn()> = {
        let ddl_token = ddl_token.clone();
        Rc::new(move || {
            if let Some(t) = ddl_token.borrow().as_ref() {
                t.cancel();
            }
        })
    };

    let run_ddl: schemaic_ui::DdlFn = {
        let handle = handle.clone();
        let db_for = db_for.clone();
        let refresh_db = refresh_db.clone();
        let refresh_schema = refresh_schema.clone();
        let guard_tx = guard_tx.clone();
        let ddl_token = ddl_token.clone();
        // The account list has its own refresh — see the `users` arm inside.
        let principals_for_ddl = principals.clone();
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
                    let refresh_schema = refresh_schema.clone();
                    let done = done.clone();
                    let ddl_token = ddl_token.clone();
                    let principals_refresh = principals_for_ddl.clone();
                    Rc::new(move || {
                        let db = db.clone();
                        let req = req.clone();
                        let done = done.clone();
                        let refresh_db = refresh_db.clone();
                        let refresh_schema = refresh_schema.clone();
                        let principals_refresh = principals_refresh.clone();
                        let database = req.database.clone();
                        let scope = req.scope;
                        let token = CancellationToken::new();
                        *ddl_token.borrow_mut() = Some(token.clone());
                        let report = create_ext_action(
                            cx,
                            move |(changed, res): (bool, Result<(), String>)| {
                                // Refresh before reporting, so the modal's success
                                // state and the tree can't be seen disagreeing for
                                // a frame. `changed`, not `is_ok()`: a MySQL plan
                                // that failed halfway still moved the schema out
                                // from under us.
                                if changed {
                                    match scope {
                                        // **A different refresh, not a different
                                        // argument to the same one.** What
                                        // changed here is the connection's
                                        // *list* of databases, and
                                        // `refresh_db` re-introspects one by
                                        // name — which after a drop is a name
                                        // that no longer exists, and after a
                                        // create is one the tree has never
                                        // heard of. Either way it finds no node
                                        // and returns, leaving the tree showing
                                        // a database that is gone.
                                        schemaic_ui::DdlScope::Server => (refresh_schema)(),
                                        schemaic_ui::DdlScope::Database => {
                                            (refresh_db)(database.clone())
                                        }
                                    }
                                    // **And the accounts, if the browser is
                                    // looking at them.** An account plan takes
                                    // the `Database` route above, whose refresh
                                    // re-introspects a *schema* — it knows
                                    // nothing about `mysql.user`, so a created
                                    // account never appeared and a dropped one
                                    // stayed on the list until the modal was
                                    // closed and reopened. The selection goes
                                    // back to nothing rather than being kept: the
                                    // account it named may be the one that was
                                    // just dropped.
                                    if let Some(t) = users.get_untracked() {
                                        users_selected.set(None);
                                        users_grants.set(schemaic_ui::GrantsState::Idle);
                                        users_state.set(schemaic_ui::UsersState::Loading);
                                        (principals_refresh)(t);
                                    }
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
                            let out = match scope {
                                // The database named here is what the run must
                                // **avoid** connecting to, not what it runs on —
                                // see `Db::run_server_ddl`. Empty for a create,
                                // which has nothing to avoid yet.
                                schemaic_ui::DdlScope::Server => {
                                    let avoid = Some(database.as_str()).filter(|d| !d.is_empty());
                                    db.run_server_ddl(avoid, &statements, token).await
                                }
                                schemaic_ui::DdlScope::Database => {
                                    db.run_ddl(&database, &statements, token).await
                                }
                            };
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
        // **Every mid-session save funnels through here**, which is why the
        // keyring's bad news is surfaced here: a save that had to leave a
        // password in plain text, or could not delete one the user cleared, used
        // to say nothing at all. `save_connections` returns each distinct notice
        // once per session, so a keyring that stays down does not raise a modal
        // on every read-only toggle.
        if let Some(notice) = secrets::save_connections(&file) {
            error_modal_text.set(Some(notice));
            error_modal_open.set(true);
        }
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
    // **Point the AI panel at `id`, and leave nothing of the previous
    // connection's behind.**
    //
    // Extracted from `switch_conn` because there are two places `active_conn`
    // moves and only one of them did this. `delete_conn_now` reimplemented two
    // of the neighbouring resets (`conn_status`, `health_failures`) and omitted
    // these, so deleting the active connection left its transcript on screen
    // under the fallback's header — and the next turn spawned a session on the
    // fallback, replayed the deleted conversation into its system prompt, and
    // `persist_chat` then wrote the whole thread back to `chats.json` **under
    // the fallback's id**. The one line that erased it ran two hundred lines
    // before the write that re-created it, which is what made the omission
    // invisible; the delete modal meanwhile says in as many words that the saved
    // AI conversation is unrecoverable.
    let reset_ai_panel: Rc<dyn Fn(u64)> = {
        let ai_session = ai_session.clone();
        Rc::new(move |id: u64| {
            // The AI conversation is bound to a connection — swap in the one
            // saved for this connection (empty when there isn't one). The live
            // session can't be reused, so the restored turns are transcript;
            // the next message spawns a session that gets them replayed.
            *ai_session.borrow_mut() = None;
            // Staged rows belong to the connection they were taken from. Leaving
            // the chip up would carry one connection's data into a question
            // asked on another — past that connection's own data-access level.
            ai_attachment.set(None);
            let restored = schemaic_core::chat::for_conn(&saved_chats.get_untracked(), id);
            // Reappearing, not arriving — mount them without the entrance pop.
            schemaic_ui::mark_messages_seen(restored.len());
            ai_messages.set(restored);
            ai_busy.set(false);
            // Any in-flight Stop belonged to the conversation just replaced.
            ai_stopping.set(false);
        })
    };

    // **The one place `active_conn` moves, because it never moves alone.**
    //
    // `expanded` is per-connection and is a plain `RwSignal` (the tree writes
    // it), so unlike `hidden_dbs` — a memo over `active_conn` — it does not
    // re-derive itself. Pairing the two was a caller obligation, and two of the
    // three sites that moved the id did not honour it: **deleting** the active
    // connection left the survivor's tree rendered against the *deleted*
    // connection's key set, so its `sys` node opened by itself, built its whole
    // table list and — with the size column on — issued a `fetch_table_stats`
    // against a database nobody had opened there, which is verbatim the failure
    // `8a75103` was written to remove. Worse, the first expand or collapse then
    // filed that whole set under the survivor's id, writing the deleted
    // connection's `db:`/`tbl:`/`col:` keys back into `ui_state.json` *after*
    // the delete had erased them — against the promise, eight lines above the
    // erase, that a deleted connection is not reconstructable from what is left
    // on disk.
    //
    // **Order matters and is the whole of it.** The write-through effect reads
    // the active connection *untracked*, so it files whatever `expanded`
    // becomes under whoever is active now. The outgoing connection's set is
    // already stored: the effect ran on every change that made it.
    let use_conn: Rc<dyn Fn(u64)> = Rc::new(move |id: u64| {
        active_conn.set(id);
        expanded.set(expanded_rules.with_untracked(|r| schemaic_core::expanded::keys_for(r, id)));
    });

    let switch_conn: Rc<dyn Fn(u64)> = {
        let load_schema = load_schema.clone();
        let reset_ai_panel = reset_ai_panel.clone();
        let use_conn = use_conn.clone();
        let check_conn = check_conn.clone();
        let last_tab = last_tab.clone();
        let open_tab_on = open_tab_on.clone();
        Rc::new(move |id: u64| {
            // Remember where the user was here before leaving, so coming back
            // returns to that tab rather than the connection's first.
            last_tab
                .borrow_mut()
                .insert(active_conn.get_untracked(), active.get_untracked());
            // Both halves, in order — see `use_conn`. Without the second the
            // tree opened the new connection with the old one's nodes expanded,
            // built their table lists, and with the size column on queried them.
            (use_conn)(id);
            persist_conns(Some(id));
            // The strip shows only this connection's tabs, so the active tab has
            // to become one of them. A connection with none gets a fresh tab —
            // with no database, since `db_nodes` still holds the previous
            // connection's until `load_schema` below finishes.
            let remembered = last_tab.borrow().get(&id).copied();
            match schemaic_core::tabsel::pick_active(&tab_refs(), id, remembered) {
                Some(tab) => schemaic_ui::activate(active, tab),
                None => (open_tab_on)(id, None),
            }
            // Clear stale status until this connection's own check lands. The
            // failure count goes with it — the previous connection's backoff
            // says nothing about this one.
            conn_status.set(ConnStatus::Unknown);
            health_failures.set(0);
            (reset_ai_panel)(id);
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
            // **The reason, not a bool.** The tunnel's failure is sometimes a
            // security control firing — `ssh::refusal_message`'s several
            // sentences about a host key that has *changed*, composed out of
            // band from russh for exactly this reason — and discarding it with
            // `Err(_)` made a machine-in-the-middle refusal, an unreadable trust
            // store, a wrong password and an unreachable host one red X.
            // `ssh::authenticate`'s doc names this button as their surface.
            let send = create_ext_action(cx, move |res: Result<(), String>| {
                conn_test.set(test_outcome(res));
            });
            handle.spawn(async move {
                // Keep the tunnel handle alive for the duration of the ping; it
                // drops (freeing the listener/port) when this task ends.
                let tunnel = if conn.uses_tunnel() {
                    match schemaic_db::ssh::open_tunnel(&conn.ssh, &conn.host, conn.port).await {
                        Ok(h) => Some(h),
                        Err(e) => {
                            send(Err(e.to_string()));
                            return;
                        }
                    }
                } else {
                    None
                };
                let db = Db::connect(&conn, tunnel.as_ref().map(|h| h.port()));
                let res = db
                    .ping(schemaic_db::PING_TIMEOUT)
                    .await
                    .map_err(|e| e.to_string());
                drop(tunnel);
                send(res);
            });
        })
    };

    // ---- Import connections from other clients ----------------------------
    //
    // The parsing is `schemaic_core::conn_import` and the file-finding is
    // `conn_sources`; what lives here is the two things only the app can do —
    // running the search off the UI thread, and turning an accepted proposal
    // into a saved connection with an id, a unique name and a keyring entry.

    // Search this machine, off the UI thread, and load the result into the
    // modal. `spawn_blocking` rather than `spawn`: this is filesystem work on a
    // home directory whose size we don't control, and the async runtime's worker
    // threads are the ones every query is waiting on.
    let scan_installed_clients: Rc<dyn Fn()> = {
        let handle = handle.clone();
        Rc::new(move || {
            if import_ui.scanning.get_untracked() {
                return;
            }
            import_ui.scanning.set(true);
            import_ui.done.set(None);
            let existing = connections.get_untracked();
            let send = create_ext_action(cx, move |scan: conn_import::ImportScan| {
                import_ui.scanning.set(false);
                import_ui.scanned.set(true);
                // Appended, like the other two sources: a scan is one of three
                // ways into this list, not the list itself, so it must not
                // discard a URL the user pasted before pressing it. Everything
                // not already saved arrives ticked — the common case is "take
                // the lot".
                add_import_result(import_ui, scan);
            });
            handle.spawn_blocking(move || {
                let files = conn_sources::discover();
                send(conn_import::scan(&files, &existing));
            });
        })
    };
    // **`scanning` had exactly one clearing site**, inside the callback above —
    // which never runs if the blocking closure panics, because tokio absorbs
    // the unwind. *Scan installed clients* then read "Scanning…" and stayed
    // disabled for the rest of the session, with no way back but a restart, and
    // its reachable trigger was a parser panic on non-ASCII text. Reopening the
    // modal resets six signals and did not reset this one; it does now, which
    // is the second site.

    // Open **empty**. The modal offers three ways in and only one of them reads
    // the filesystem, so that one is asked for rather than performed because a
    // dialog opened.
    let open_import: Rc<dyn Fn()> = Rc::new(move || {
        import_ui.paste.set(String::new());
        import_ui.paste_error.set(None);
        import_ui.file_error.set(None);
        // See `scan_installed_clients`: this is the second clearing site, and
        // the only one a user can reach when the first never ran.
        import_ui.scanning.set(false);
        import_ui.rows.set(Vec::new());
        import_ui.chosen.set(std::collections::HashSet::new());
        import_ui.skipped.set(Vec::new());
        import_ui.skipped_hidden.set(0);
        import_ui.scanned.set(false);
        import_ui.done.set(None);
        import_ui.open.set(true);
    });

    // Add whatever is in the paste field to the review list — one URL per line.
    //
    // Appended to the scan's results rather than replacing them, through the
    // same `add_import_result` as the other two sources, so the ticking rule is
    // theirs too: a pasted URL naming a connection the user already has arrives
    // unticked like any other.
    let add_pasted_url: Rc<dyn Fn()> = Rc::new(move || {
        let text = import_ui.paste.get_untracked();
        if text.trim().is_empty() {
            import_ui
                .paste_error
                .set(Some(conn_import::UrlError::Empty.message()));
            return;
        }
        let existing = connections.get_untracked();
        let file = conn_import::SourceFile {
            source: conn_import::ImportSource::Url,
            path: String::new(),
            text,
        };
        let scan = conn_import::scan(&[file], &existing);
        // A paste that produced nothing at all is answered under the field,
        // where the text that failed still is — and the field keeps it, so it
        // can be corrected rather than retyped.
        if scan.found.is_empty() {
            import_ui.paste_error.set(Some(
                scan.skipped
                    .first()
                    .map(|s| s.reason.message())
                    .unwrap_or_else(|| conn_import::UrlError::Empty.message()),
            ));
            return;
        }
        // A *partly* good paste clears the field, so the lines that failed have
        // to be reported somewhere else: they go to the skipped list with every
        // other entry a source held and this app could not offer. Dropping them
        // silently is how three good lines out of five reads as five.
        import_ui.paste_error.set(None);
        import_ui.paste.set(String::new());
        import_ui.done.set(None);
        add_import_result(import_ui, scan);
    });

    // Read a source file the user pointed at, for a layout `conn_sources`
    // doesn't search — a project's own `.idea/dataSources.xml`, an export, a
    // `.env`. Which parser to use is decided from the file's name.
    let choose_import_file: Rc<dyn Fn()> = {
        let handle = handle.clone();
        Rc::new(move || {
            use floem::file::FileDialogOptions;
            use floem::file_action::open_file;
            let handle = handle.clone();
            open_file(
                FileDialogOptions::new().title("Choose a connections file"),
                move |picked| {
                    let Some(path) = picked.and_then(|i| i.path.first().cloned()) else {
                        return;
                    };
                    // **Off the UI thread, for the reason `scan_installed_clients`
                    // gives one line up** — and more so: this is the one input
                    // whose size and shape the app controls least. The picker has
                    // no type filter and an unrecognised name is read as a list
                    // of URLs by design, so a shell history or a log produces one
                    // skipped entry per line. Every step below used to run inside
                    // this callback: the read, the parse, the `~/.pgpass` read
                    // and the merge, with a fully frozen window and no cancel.
                    let existing = connections.get_untracked();
                    let send = create_ext_action(
                        cx,
                        move |res: Result<conn_import::ImportScan, String>| match res {
                            Ok(scan) => {
                                import_ui.file_error.set(None);
                                import_ui.paste_error.set(None);
                                import_ui.done.set(None);
                                add_import_result(import_ui, scan);
                            }
                            // The one read the user asked for by name, so its
                            // failure is theirs to see — unlike the search, which
                            // opens files nobody named. **Its own message and its
                            // own slot:** one sentence for three causes, written
                            // into the *paste* field's error line, where it then
                            // outlived the URL typed after it.
                            Err(msg) => import_ui.file_error.set(Some(msg)),
                        },
                    );
                    handle.spawn_blocking(move || {
                        let source = conn_sources::source_for_path(&path);
                        let file = match conn_sources::open_source(&path, source) {
                            Ok(f) => f,
                            Err(e) => {
                                send(Err(e.message(&path)));
                                return;
                            }
                        };
                        let mut scan = conn_import::scan(&[file], &existing);
                        // **Then complete it from the password files.** Read on
                        // its own, a hand-picked DataGrip export arrives with
                        // every password blank while `~/.pgpass` on the same
                        // machine holds them — and whether a row can be completed
                        // must not depend on how its file was found. Applied
                        // *after* `scan` rather than by handing it both files:
                        // `scan` would offer `.pgpass`'s own servers as rows too,
                        // and the user asked to import one file.
                        //
                        // **`PickedFile`, because these two halves have different
                        // authors.** The rows came out of a file the user was
                        // handed; the passwords are the user's own. A wildcard
                        // `.pgpass` line may therefore only complete a row for a
                        // server that file already names outright, and a row it
                        // does complete says so and arrives unticked.
                        for pw in conn_sources::password_sources() {
                            conn_import::fill_missing_passwords(
                                &mut scan.found,
                                &conn_import::pgpass_entries(&pw.text),
                                conn_import::PgpassScope::PickedFile,
                            );
                        }
                        send(Ok(scan));
                    });
                },
            )
        })
    };

    // **The only step that writes.** Everything above builds proposals.
    //
    // Each accepted row takes a fresh id (`Connection::next_id` over the list as
    // it grows, so two rows in one press can't share one — the ids are the
    // keyring account strings), a name that doesn't collide with a saved one,
    // and an identity colour, exactly as New connection and Duplicate do.
    let import_chosen: Rc<dyn Fn()> = {
        let load_schema = load_schema.clone();
        let reset_activity = reset_activity.clone();
        Rc::new(move || {
            let picked: Vec<Connection> = import_ui.rows.with_untracked(|rows| {
                import_ui.chosen.with_untracked(|chosen| {
                    let mut idx: Vec<usize> = chosen.iter().copied().collect();
                    // The set has no order; import in the order the list showed.
                    idx.sort_unstable();
                    idx.iter()
                        .filter_map(|i| rows.get(*i))
                        .map(|r| r.connection.clone())
                        .collect()
                })
            });
            if picked.is_empty() {
                return;
            }
            let added = picked.len();
            // **Was the active id pointing at nothing?** On a fresh install
            // `Connection::startup_active_id` answers `next_id(&[])` — the id the
            // first connection saved this session is about to take — so the active
            // connection is about to come into existence below and nothing has
            // loaded it. `save_conn` handles the same case for a hand-created
            // connection; this is the second creator, and without the check the
            // headline path of this whole feature (a new user importing their
            // servers) ends with an empty schema tree and "Not connected".
            let active = active_conn.get_untracked();
            let was_dangling = connections.with_untracked(|cs| !cs.iter().any(|c| c.id == active));
            connections.update(|cs| {
                for conn in picked {
                    let names: Vec<String> = cs.iter().map(|c| c.name.clone()).collect();
                    let used_colors: Vec<String> =
                        cs.iter().filter_map(|c| c.color.clone()).collect();
                    let mut conn = conn;
                    conn.id = Connection::next_id(cs);
                    conn.name = unique_name(&conn.name, &names);
                    conn.color = Some(pick_connection_color(&used_colors));
                    // The same sanitising a saved form goes through: a SQLite row
                    // carries no server, whatever the source file had in it.
                    cs.push(conn.sanitized());
                }
            });
            persist_conns(Some(active));
            // The active id now names one of the connections just added, and nothing
            // has connected to it — see `was_dangling` above.
            if was_dangling
                && let Some(conn) =
                    connections.with_untracked(|cs| cs.iter().find(|c| c.id == active).cloned())
            {
                load_schema(conn);
                (reset_activity)();
            }
            // The list behind this modal has just grown, so the imported rows must
            // stop offering themselves: re-marking is cheaper and more honest than
            // removing them, since the row stays visible with "Already saved" on it.
            let existing = connections.get_untracked();
            import_ui.rows.update(|rows| {
                for r in rows.iter_mut() {
                    r.notes
                        .retain(|n| *n != conn_import::ImportNote::AlreadySaved);
                }
                conn_import::mark_existing(rows, &existing);
            });
            import_ui.chosen.set(std::collections::HashSet::new());
            // **And say what did not come across.** Every imported connection
            // lands `read_only: false` and `Environment::None`, whatever the
            // source tool had marked it — those are Schemaic's own two ways of
            // making a production server visibly dangerous, and a server
            // DBeaver marks read-only and types `prod` arrived writable and
            // unbadged with nothing on screen about it. The blank is a
            // deliberate choice (`conn_import::blank`) and it is still the
            // right one — guessing another tool's guard rails is worse — but a
            // deliberate choice the user is not told about is indistinguishable
            // from an oversight.
            //
            // Said once, here, rather than as a note on every row: it is true
            // of all of them, and a badge repeated twelve times is read zero
            // times.
            import_ui.done.set(Some(format!(
                "{} Read-only and the environment badge are not carried over — set them \
                 in Manage connections for anything that matters.",
                match added {
                    1 => "Added 1 connection.".to_string(),
                    n => format!("Added {n} connections."),
                }
            )));
        })
    };

    // Save the form (create or update); reload schema if the active conn changed.
    let save_conn: Rc<dyn Fn()> = {
        let load_schema = load_schema.clone();
        let tunnels = tunnels.clone();
        let reset_activity = reset_activity.clone();
        let drop_session = drop_session.clone();
        let ai_session = ai_session.clone();
        Rc::new(move || {
            let id = draft
                .id
                .get_untracked()
                .unwrap_or_else(|| connections.with_untracked(|cs| Connection::next_id(cs)));
            // The entry as it stood *before* this save, so the edit can be asked
            // what it moved. `None` for a connection being created, where there is
            // nothing yet to invalidate.
            let previous = connections.with_untracked(|cs| cs.iter().find(|c| c.id == id).cloned());
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
            // **Did this edit move the connection to a different server?** Asked
            // through `targets_same_server`, the predicate that already answers it
            // for the schema tree, rather than a second reading of the same
            // fields. Everything below is invalid exactly when the answer is yes.
            //
            // It used to be asked of nothing at all: the tunnel was dropped on
            // *every* save (review H9, "the edit may have changed the host / SSH
            // settings"). True of an edit that moved something, and quietly
            // destructive of one that didn't — tearing the listener down takes the
            // forwarded connections with it, so changing a connection's **colour**
            // killed the socket under a pinned Manual transaction and rolled back
            // uncommitted work, while the tab went on offering Commit and
            // Rollback. Unconditional was the cheap answer in the wrong place:
            // cheap is right for throwing a *snapshot* away, and wrong for
            // throwing a *transaction* away.
            let repointed = previous
                .as_ref()
                .is_some_and(|p| !p.targets_same_server(&conn));
            // **A wider question than "which server", asked separately.** The
            // transport is not part of `targets_same_server` — nor should it be,
            // since the schema tree's question really is only about which server
            // — but it *is* part of "is anything I have already opened still
            // valid". Change a connection's TLS mode and every one of those nine
            // fields is unchanged, so the block below was skipped and a pinned
            // Manual session went on running, and committing, over the plaintext
            // socket it was opened on, while the form and the status bar reported
            // TLS. See `Connection::invalidates_open_connections`.
            let invalidated = previous
                .as_ref()
                .is_some_and(|p| p.invalidates_open_connections(&conn));
            if repointed {
                // The cached tunnel reaches the old server — drop it (its listener
                // is torn down) so `load_schema` establishes a fresh one. **On the
                // server question, not the wider one:** a TLS change needs no new
                // listener, and tearing one down takes the forwarded connections
                // with it.
                tunnels.borrow_mut().remove(&id);
            }
            if invalidated {
                // Every pinned Manual session on this connection is now either on
                // a dead socket, on a server the user has left, or on a
                // transport the user has just changed their mind about — and its
                // transaction is gone either way. This is `delete_conn_now`'s
                // treatment, for the same reason and with the same absence of a
                // prompt: the server rolls back on disconnect, and the tab
                // dropping to Auto-commit is what stops its footer claiming a
                // transaction that no longer exists. **Not**
                // `repair_killed_session`'s reopen — that one puts the tab back on
                // a server that is still there, whereas here the tunnel has just
                // gone and `open_session` would fail on the spot, flip the tab to
                // Auto anyway and raise an error modal on top.
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
                // And the AI session, whose MCP subprocess holds a `Db` built at
                // spawn from the old target and the old tunnel port. `needs_respawn`
                // cannot see this: it compares the conn id, which did not move, and
                // the `AiSettings`, none of which name a host. Left alone, the
                // assistant's tools went on reading the previous server while the
                // whole UI said otherwise — or, tunnelled, reached a local port
                // that no longer answers. Dropping it costs nothing: `ai_send`
                // replays the conversation into the prompt of the session it
                // spawns next.
                let ours = ai_session
                    .borrow()
                    .as_ref()
                    .is_some_and(|s| s.conn_id == id);
                if ours {
                    ai_session.borrow_mut().take();
                }
            }
            if active_conn.get_untracked() == id {
                // Same reasoning as the tunnel above, for the other thing that
                // was taken against wherever this connection pointed *before* the
                // edit: the Server Activity snapshot. See `reset_activity` — the
                // effect that clears it cannot see an in-place edit, because the
                // id it keys on doesn't move.
                //
                // **After `load_schema`, not before.** When the edit repointed the
                // connection, `load_schema` is what re-opens the tunnel dropped
                // above, and the reset wants to refetch. Ordered the other way the
                // refetch is not merely at risk of finding no tunnel — it is
                // *guaranteed* to, the removal being a few lines up with the
                // re-open not yet asked for. Unconditional, unlike the block
                // above: a rename moves no server but the snapshot is cheap, and
                // `reset_activity` skips its own refetch when nothing is
                // reachable.
                load_schema(conn);
                (reset_activity)();
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
        let save_ui = save_ui.clone();
        let reset_activity = reset_activity.clone();
        let ai_session = ai_session.clone();
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
            //
            // **And say so when they are not.** The return value used to be
            // dropped on the floor and the only report was a `tracing::warn!`,
            // which a released GUI build discards — while the modal the user
            // just confirmed told them the keyring entries were unrecoverable.
            // They are not: `next_id` is `max + 1`, so deleting the
            // highest-numbered connection frees its id, and the next connection
            // created takes it and hydrates the dead one's password, SSH
            // password and key passphrase on the launch after that. The form
            // shows a filled mask and the connection sends server A's
            // credential to server B.
            if !secrets::forget_connection(id) {
                persist::queue_notice(
                    "Schemaic could not reach the OS keyring, so the deleted connection's \
                     stored password and SSH secrets are **still in it**.\n\
                     They will be removed the next time a connection is deleted or saved \
                     while the keyring is reachable. Until then, avoid creating a new \
                     connection: ids are reused, and a new one taking this id would be \
                     given those secrets."
                        .to_string(),
                );
            }
            // …and its saved AI conversation, which would otherwise linger and
            // resurface under whatever connection reuses the id.
            saved_chats.update(|chats| schemaic_core::chat::clear_conn(chats, id));
            persist::save_json_erasing(
                "chats.json",
                &schemaic_core::chat::ChatFile::of(&saved_chats.get_untracked()),
            );
            // The **live** session too, which the line above does not touch: it
            // clears the transcript on disk while a `claude` child goes on holding
            // an MCP endpoint aimed at the server just deleted. `needs_respawn`
            // usually covers this by the side door, since deleting the active
            // connection moves `active_conn` and a different id always respawns —
            // but not when the id is *recycled*. `next_id` is `max + 1`, so
            // deleting the highest-numbered connection frees its id for the next
            // one created; that connection becomes active under the same id, and
            // with the settings unchanged nothing asks for a respawn. The
            // assistant would answer about the deleted connection's server from a
            // new connection's panel.
            let ours = ai_session
                .borrow()
                .as_ref()
                .is_some_and(|s| s.conn_id == id);
            if ours {
                ai_session.borrow_mut().take();
            }
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
                // Whatever was on screen is about a connection the user has just
                // left — the same case `switch_conn` resets both of these for.
                // Without it the empty state could show the accent "New
                // connection" button and "Disconnected · Retry" side by side,
                // the notice being about a connection that no longer exists.
                conn_status.set(ConnStatus::Unknown);
                health_failures.set(0);
                match connections.with_untracked(|cs| cs.first().cloned()) {
                    Some(conn) => {
                        // Both halves — see `use_conn`. The survivor's tree must
                        // not be rendered against the deleted connection's keys.
                        (use_conn)(conn.id);
                        // **And the AI panel, which this used to leave alone.**
                        // Deleting the active connection is a connection switch
                        // by another name; without it the deleted transcript
                        // stayed on screen under the fallback's header, was
                        // replayed into the next session's system prompt, and
                        // was written back to `chats.json` under the fallback's
                        // id by `persist_chat` — undeleting the one thing the
                        // confirm modal promised was unrecoverable.
                        (reset_ai_panel)(conn.id);
                        load_schema(conn);
                    }
                    None => {
                        // The whole clear: with no connection left there is
                        // nothing the node scope could be reused for, and
                        // leaving it installed orphaned it outright.
                        (clear_schema_tree)();
                        // No connection left to restore a conversation from, so
                        // the panel empties — the same reset, with nothing to
                        // put back.
                        (reset_ai_panel)(0);
                        // **"The list is empty" has one meaning wherever it is
                        // reached.** Leaving `active_conn` on the deleted id was
                        // invisible until the empty state grew a New-connection
                        // button: `save_conn` loads the schema for a connection
                        // it saves only when it is the active one, and the new
                        // one takes `next_id(&[])` — so the first connection
                        // created after deleting the last was saved, took the
                        // switcher slot, said "No connection", and never
                        // connected. This is `startup_active_id`'s answer for
                        // the same state, which is where the coupling is
                        // documented and tested.
                        // Through `use_conn`, which also empties `expanded`:
                        // this id is `next_id(&[])` = 1, the id the *next*
                        // connection created will take, so leaving the deleted
                        // connection's keys in place opened that new connection
                        // with the dead one's tree expanded and persisted it
                        // under the new id.
                        (use_conn)(Connection::startup_active_id(None, &[]));
                    }
                }
                // **The id does not always move here either**, which is the same
                // defect `save_conn` calls this for. Delete the *last* connection
                // and the line above sets `active_conn` to
                // `startup_active_id(None, &[])` — which is `next_id(&[])`, which
                // is `1`, which is the id the first connection ever created has.
                // Deleting that one leaves the signal on the value it already
                // held, so the effect keyed on it sees no change and never clears:
                // Server Activity went on listing a deleted connection's sessions,
                // offering to kill them. Called unconditionally rather than only
                // on that branch — where a fallback *did* change the id the effect
                // will clear anyway, and this is guarded, so the cost is nothing
                // and there is no second condition to keep true.
                (reset_activity)();
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
            // **Guarded on `doomed`, which is already the answer.** Deleting a
            // connection with no open tabs is ordinary, and an unconditional
            // `update` would rebuild the whole tab strip for it — `update`
            // notifies whether or not the value changed. No second scan: the
            // list above *is* "which tabs would this remove".
            if !doomed.is_empty() {
                tabs.update(|v| v.retain(|t| t.conn_id.get_untracked() != id));
            }
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
                Some(tab) => schemaic_ui::activate(active, tab),
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
            // its queries, the databases it had, the tables looked at. Which is
            // also why these two saves are **erasing**: the ordinary one keeps
            // the pre-deletion generation as `.bak`, and "not reconstructable
            // from what's left on disk" is exactly the claim that breaks.
            history_entries.update(|v| schemaic_core::history::clear_conn(v, id));
            persist::save_json_erasing(
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
            // Its poll interval too — connection ids are reused, and the next
            // connection to take this one would inherit a choice nobody made for
            // it. `save_ui` follows from the effect watching this store.
            activity_intervals.update(|v| schemaic_core::activity::clear_conn(v, id));
            // Same rule for what it had put away with the eye. The flat set this
            // replaced could not do it at all, so a deleted connection's hidden
            // names went on hiding same-named databases forever.
            hidden_db_rules.update(|v| schemaic_core::db_hidden::clear_conn(v, id));
            // And what it had open in the tree, for the same reason — the flat
            // set could not do this either, so a deleted connection's expanded
            // nodes went on auto-opening same-named databases on connections
            // created later.
            expanded_rules.update(|v| schemaic_core::expanded::clear_conn(v, id));
            (save_ui)();
            formats.update(|v| schemaic_core::format::clear_conn(v, id));
            (save_formats)();
            // The twelfth store, and it was the one missed. A connection-scoped
            // snippet is a query the user wrote against *this* connection — so
            // both reasons above apply to it, and the id-recycling one with
            // force: the next connection to take this freed id would have found
            // them waiting under "THIS CONNECTION".
            snippets.update(|v| schemaic_core::snippet::clear_conn(v, id));
            (save_snippets)();
            // Diagram layouts live only on disk (no signal) — load, prune, save.
            // The third lazy load, and the third that owes the user the recovery
            // notice the startup drain cannot carry: the prune-and-save here is
            // unconditional, so a `.corrupt` rename would otherwise be followed
            // immediately by writing the defaulted file back.
            let mut layouts: schemaic_core::erd::DiagramLayoutsFile =
                persist::load_json("diagrams.json");
            schemaic_ui::report_recoveries(error_modal_text, error_modal_open);
            schemaic_core::erd::clear_conn_layouts(&mut layouts, id);
            persist::save_json("diagrams.json", &layouts);
        })
    };

    // Ask first. **Every one of those is unrecoverable**: the keyring entries
    // (`conn.{id}.password` / `.ssh_password` / `.ssh_passphrase` /
    // `.tls_key_passphrase`), the
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
                        // `Role::settled` is the middle link of the chain the
                        // decoder's `is_error` starts and
                        // `Role::carries_an_answer` ends. It only ever *raises*
                        // to Error — a turn already marked one is not un-marked
                        // by a later snapshot.
                        if msg.done {
                            last.role = last.role.settled(msg.is_error);
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
                        (persist_chat)(id, persist::Saving::Replacing);
                    }
                }
            }
        });
    }

    // Send a user turn: (re)start the per-connection session, then write it to
    // the CLI's stdin. Replies stream back via `ai_stream` (above).
    // Snapshot the session-affecting AI settings (Copy signals → this closure is
    // Copy, usable from both `ai_send` and `ai_apply`).
    // The active connection's AI data-access level. One lookup, used by the
    // session settings and by the spawn, so the tools list and the prompt that
    // describes it can never disagree.
    let conn_ai_data = move || -> schemaic_core::connection::AiData {
        let id = active_conn.get_untracked();
        connections
            .with_untracked(|cs| cs.iter().find(|c| c.id == id).and_then(|c| c.ai_data))
            .unwrap_or_default()
    };
    let ai_settings_now = move || AiSettings {
        harness: ai_harness.get_untracked(),
        model: ai_model.get_untracked(),
        effort: ai_effort.get_untracked(),
        data: conn_ai_data(),
        cli_path: ai_cli_path.get_untracked(),
        instructions: ai_instructions.get_untracked(),
        schema_scope: ai_schema_scope.get_untracked(),
        hidden: hidden_dbs.get_untracked(),
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
            let data_now = conn_ai_data();
            // A live session's argv, tools list and MCP blob were all fixed at
            // spawn, so a change to any setting carried in them has to respawn —
            // otherwise the setting the user just changed keeps not applying,
            // which is the worst possible failure for the controls that withhold
            // and a silent lie for the ones that don't. The rule is
            // `ai::needs_respawn`, where a test can reach it; the one thing it
            // cannot answer for itself is whether the CLI path names a binary
            // that exists, so that is resolved here.
            let settings_now = ai_settings_now();
            let scope_now = settings_now.schema_scope;
            let need_new = needs_respawn(
                ai_session
                    .borrow()
                    .as_ref()
                    .map(|s| (s.conn_id, &s.settings)),
                active_id,
                &settings_now,
                harness_reachable(settings_now.harness, &settings_now.cli_path),
            );
            // The live context as it stands *now* — the system prompt is written
            // once at spawn, so every later turn carries the delta (see
            // `apply_turn_delta`).
            let cx_params = AiContextParams {
                connections,
                active_conn,
                db_nodes,
                hidden_dbs,
                tabs,
                active,
                scope: ai_schema_scope.get_untracked(),
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
                // The harness the *new* session will run under, which is what
                // decides whether an earlier turn's prose may be replayed into
                // it — see `render_history`.
                let context = ai_context(
                    cx_params,
                    fallback_db.as_deref(),
                    &prior,
                    &ai_instructions.get_untracked(),
                    ai_harness.get_untracked().key(),
                );
                // If the connection's `Db` can't be built yet (SSH tunnel
                // pending), skip the MCP tools rather than blocking the chat.
                let database = context_now.active_db.clone();
                if let Ok(db) = db_for(active_id) {
                    let mcp_database = database.clone();
                    // **The old session goes first.** Assigning over
                    // `ai_session` below drops it, which is *after* the new
                    // one's `install` has been handed to a blocking thread — so
                    // an Antigravity teardown regularly ran after the new
                    // session's registration and took it back out again. The
                    // `Claim` nonce is what makes that harmless rather than a
                    // race won by whoever finishes last; taking the old session
                    // here is the ordering half, and the two together are the
                    // fix.
                    ai_session.borrow_mut().take();
                    let (stdin_tx, private) = start_ai_session(
                        &handle,
                        StartAiParams {
                            system_context: context,
                            db,
                            database,
                            ai_tx: ai_tx.clone(),
                            harness: ai_harness.get_untracked(),
                            model: ai_model.get_untracked(),
                            effort: ai_effort.get_untracked().cli().to_string(),
                            data: data_now,
                            cli_path: ai_cli_path.get_untracked(),
                            // Read at spawn, like the system prompt beside it:
                            // the MCP subprocess is handed a blob, not a signal,
                            // so hiding a database mid-session takes effect on
                            // the next one — the same as the schema outline.
                            hidden: hidden_dbs.get_untracked(),
                            schema_scope: scope_now,
                        },
                    );
                    *ai_session.borrow_mut() = Some(AiSession {
                        conn_id: active_id,
                        stdin_tx,
                        private,
                        settings: ai_settings_now(),
                        // The system prompt just stated this context, so the
                        // first turn has no delta to report.
                        last_context: context_now.clone(),
                        mcp_database,
                    });
                } else {
                    // No `Db` (tunnel still coming up, credentials refused) — so
                    // no session was spawned. **Drop the old one rather than
                    // sending this turn to it.** It was built for the previous
                    // connection, or the previous data-access level: answering
                    // through it would run `run_query` against a connection the
                    // user has just locked down, which is this control failing
                    // open. Say so and leave the question in the box.
                    //
                    // **Put it back if it isn't there.** Regenerate hands its
                    // question in as `msg` *after* deleting it from the
                    // transcript, so returning here without it lost the
                    // question and its answer durably — the one path into this
                    // arm where the box is not already holding the text.
                    ai_session.borrow_mut().take();
                    if ai_input.with_untracked(|s| s.trim().is_empty()) {
                        ai_input.set(msg.clone());
                    }
                    ai_messages.update(|v| {
                        v.push(ChatMessage {
                            role: Role::Error,
                            text: String::new(),
                            segs: vec![schemaic_core::transcript::Seg::Text(
                                "Can't reach the database, so the assistant can't be started \
                                 for this connection. Check the connection and try again."
                                    .to_string(),
                            )],
                            stats: None,
                            pending: false,
                            attachment: None,
                            // Schemaic's own refusal, not an agent's answer — no
                            // CLI was reached, so none is named over it.
                            harness: None,
                        });
                    });
                    return;
                }
            }

            // The staged rows travel with *this* turn and no other: taken here,
            // so a second question doesn't quietly re-send the same data.
            //
            // Gated one last time on the level of the connection the turn is
            // actually going to. Rows are staged against the connection they
            // came from, and the user can switch connections before sending —
            // so this is what stops a production grid being sent to a
            // schema-only connection's session by a path the grid already
            // refused.
            let attachment = ai_attachment
                .get_untracked()
                .filter(|_| data_now.may_attach());
            ai_attachment.set(None);
            // Read before the update borrows nothing of it, and *after* the
            // `need_new` spawn above — so on the spawn path this is already the
            // new session's own value and the two arms agree.
            let live_harness = ai_session.borrow().as_ref().map(|s| s.settings.harness);
            ai_messages.update(|v| {
                v.push(ChatMessage::user_with(msg.clone(), attachment.clone()));
                // The harness *this* turn runs on, stamped now rather than read
                // back at render time: the setting can change before the next
                // draw, and the transcript has to keep saying who actually
                // answered. **The live session's harness, not the selected
                // one** — `needs_respawn` keeps a working conversation when the
                // newly chosen harness is unreachable, so the two disagree on
                // exactly that path. See `ai::turn_harness`.
                v.push(ChatMessage::pending(Some(ai::turn_harness(
                    live_harness,
                    ai_harness.get_untracked(),
                    need_new,
                ))));
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
            // Attached rows go in with the question, ahead of it: they are state
            // the question refers to, like the context delta above them.
            let asked = match attachment.as_ref().map(|a| a.prompt_block()) {
                Some(block) if !block.is_empty() => format!("{block}\n\n{msg}"),
                _ => msg.clone(),
            };
            if let Some(s) = ai_session.borrow_mut().as_mut() {
                let turn = apply_turn_delta(
                    &s.last_context,
                    &context_now,
                    s.mcp_database.as_deref(),
                    &recap,
                    &asked,
                );
                s.last_context = context_now;
                let _ = s.stdin_tx.send(ai::SessionMsg::Turn(turn));
            } else {
                // Unreachable by construction (`need_new` either spawned one or
                // returned above), but the cost of being wrong is a spinner that
                // never stops and staged rows the user has to reselect — so put
                // both back rather than trust the reasoning.
                ai_attachment.set(attachment);
                ai_busy.set(false);
                mark_stopped(ai_messages);
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
                .map(|s| s.stdin_tx.send(ai::SessionMsg::Interrupt).is_ok())
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
            // "New chat" clears everything else about the conversation; a chip
            // left hanging over the fresh box would attach to a question the
            // user never staged it for.
            ai_attachment.set(None);
            // Erasing: the transcript this replaces with nothing would otherwise
            // sit in `chats.json.bak` until the next finished turn.
            (persist_chat)(active_conn.get_untracked(), persist::Saving::Erasing);
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
            // The attachment travels with the question, not in its text, so it
            // has to be carried over too — regenerating "what stands out about
            // these rows?" without them asks the model about data it can't see,
            // and the rebuilt bubble would lose the record that any went.
            let last_user = ai_messages.with_untracked(|v| {
                v.iter()
                    .rev()
                    .find(|m| m.role == Role::User)
                    .map(|m| (m.text.clone(), m.attachment.clone()))
            });
            let Some((text, attachment)) = last_user else {
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
            // Re-stage the rows the question came with, so `ai_send` picks them
            // up exactly as it did the first time. A restored conversation has
            // the summary without them (`retained` false) — nothing to re-send
            // there, and the filter says so rather than attaching an empty block.
            ai_attachment.set(attachment.filter(|a| a.retained()));
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
            //
            // **`ai::needs_respawn`, not `!=`.** A whole-struct comparison is a
            // second rule for the one question `needs_respawn` exists to answer,
            // and the two had already drifted apart: `!=` counts `cli_path`
            // unconditionally, so typing a path that resolves to nothing — the
            // state the field's own red hint is for — threw away a working
            // conversation for a binary that cannot be spawned. The modal is the
            // only place that path is typed, so this call site was the only one
            // where that could happen.
            let current = ai_settings_now();
            let usable = harness_reachable(current.harness, &current.cli_path);
            let conn_now = active_conn.get_untracked();
            let changed = ai_session.borrow().as_ref().is_some_and(|s| {
                needs_respawn(Some((s.conn_id, &s.settings)), conn_now, &current, usable)
            });
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
                    hidden_dbs,
                    tabs,
                    active,
                    scope: ai_schema_scope.get_untracked(),
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
            let system = inline_system_prompt(
                db_nodes,
                hidden_dbs,
                active_db.as_deref(),
                &req,
                dialect,
                ai_schema_scope.get_untracked(),
            );
            let intent = req.intent.clone();
            let send = create_ext_action(cx, move |state: InlineAiState| inline_ai.set(state));
            // Whichever CLI the user picked, on its own flags. This used to
            // resolve Claude regardless — see `ai::inline_plan`, which also
            // carries the oversize check and the constraint gate.
            let plan = match ai::inline_plan(
                ai_harness.get_untracked(),
                &ai_cli_path.get_untracked(),
                &ai_model.get_untracked(),
                ai_effort.get_untracked().cli(),
                &intent,
                &system,
            ) {
                Ok(p) => p,
                Err(why) => {
                    inline_ai.set(InlineAiState::Failed(why));
                    return;
                }
            };
            let jh = handle.spawn(async move {
                send(inline_outcome(ai::run_inline(plan).await, dialect));
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
            // **`Ready` too, not just `Busy`.** A settled suggestion belongs to
            // the tab it was asked for by exactly the argument above, and the
            // pane that could answer it is gone: the new pane's `CmdK` is fresh
            // (`open: false`, `start == end == 0`), so the publish effect drew
            // tab A's SQL as an insertion at line 0 of tab B and set tab B's
            // editor `read_only` — with no footer, because that is gated on
            // `open`, and no Escape route for the same reason. Every tab visited
            // afterwards came up frozen; only another Ctrl+K cleared it.
            if let Some(prev) = prev
                && prev != id
                && matches!(
                    inline_ai.get_untracked(),
                    InlineAiState::Busy | InlineAiState::Ready(_)
                )
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
                // **The two consent settings this path used to walk around.**
                // `Schema context` decides whether the table's structure goes at
                // all, and the connection's AI data level decides whether rows
                // the user never attached are fetched to go with it. Read here,
                // on the UI thread, like every other setting below.
                let ddl = ai_ddl_for(ddl, ai_schema_scope.get_untracked());
                let ai_data = ai_data_of_conn(connections, req.conn_id);
                // Read off the signals here — the spawn below is not on the UI
                // thread, and `ai::inline_plan` takes what they say.
                let harness = ai_harness.get_untracked();
                let cli_path = ai_cli_path.get_untracked();
                let model = ai_model.get_untracked();
                let effort = ai_effort.get_untracked().cli().to_string();
                let finish = create_ext_action(cx, move |res: AiFillResult| (done)(res));
                let schemaic_ui::AiFillRequest {
                    source,
                    column,
                    row_context,
                    ..
                } = req;
                handle.spawn(async move {
                    let database = source.database.clone();
                    // **Not fetched at all below `may_query`**, rather than
                    // fetched and dropped: a `SELECT * … LIMIT 20` the user never
                    // ran is a read of their data whether or not it is sent.
                    // `build_fill_prompt` drops the rows too, so the decision
                    // cannot be lost between here and there.
                    let sample = if ai_data.may_query() {
                        let token = CancellationToken::new();
                        let sql = sample_sql(db.engine(), &source, &pk_cols);
                        match db.fetch_query(Some(&database), &sql, 20, token).await {
                            Ok(rs) => sample_rows(&rs),
                            Err(_) => Vec::new(), // empty/unsampleable → DDL-only prompt
                        }
                    } else {
                        Vec::new()
                    };
                    let prompt = schemaic_core::seed::build_fill_prompt(
                        &format!("{database}.{}", source.display()),
                        &column,
                        ddl.as_deref(),
                        &sample,
                        &row_context,
                        ai_data,
                        dialect_for(db.engine()),
                    );
                    let system = "You output only the requested raw value — no quotes, \
                                  no markdown, no prose.";
                    let res =
                        match ai::inline_plan(harness, &cli_path, &model, &effort, &prompt, system)
                        {
                            Ok(plan) => match ai::run_inline(plan).await {
                                Ok(text) => match schemaic_core::seed::parse_fill_response(&text) {
                                    schemaic_core::seed::FillOutcome::Value(v) => {
                                        AiFillResult::Value(v)
                                    }
                                    schemaic_core::seed::FillOutcome::Null => AiFillResult::Null,
                                    schemaic_core::seed::FillOutcome::Empty => {
                                        AiFillResult::Failed("The AI returned no value.".into())
                                    }
                                },
                                Err(why) => AiFillResult::Failed(why),
                            },
                            Err(why) => AiFillResult::Failed(why),
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
                // The same two consent settings the fill callback above reads.
                let ddl = ai_ddl_for(ddl, ai_schema_scope.get_untracked());
                let ai_data = ai_data_of_conn(connections, req.conn_id);
                // Read off the signals here — see the fill callback above.
                let harness = ai_harness.get_untracked();
                let cli_path = ai_cli_path.get_untracked();
                let model = ai_model.get_untracked();
                let effort = ai_effort.get_untracked().cli().to_string();
                let finish = create_ext_action(cx, move |res: AiSeedResult| (done)(res));
                let schemaic_ui::AiSeedRequest {
                    source,
                    fill_columns,
                    count,
                    ..
                } = req;
                handle.spawn(async move {
                    let database = source.database.clone();
                    // Not fetched below `may_query` — see the fill callback.
                    let sample = if ai_data.may_query() {
                        let token = CancellationToken::new();
                        let sql = sample_sql(db.engine(), &source, &pk_cols);
                        match db.fetch_query(Some(&database), &sql, 20, token).await {
                            Ok(rs) => sample_rows(&rs),
                            Err(_) => Vec::new(), // empty/unsampleable → DDL-only prompt
                        }
                    } else {
                        Vec::new()
                    };
                    let prompt = schemaic_core::seed::build_seed_prompt(
                        &format!("{database}.{}", source.display()),
                        ddl.as_deref(),
                        &fill_columns,
                        &sample,
                        count,
                        ai_data,
                        dialect_for(db.engine()),
                    );
                    let system = "You output only a JSON array of row objects — no \
                                  markdown, no prose.";
                    let res =
                        match ai::inline_plan(harness, &cli_path, &model, &effort, &prompt, system)
                        {
                            Ok(plan) => match ai::run_inline(plan).await {
                                Ok(text) => match schemaic_core::seed::parse_seed_response(&text) {
                                    Ok(rows) => AiSeedResult::Rows(rows),
                                    Err(e) => AiSeedResult::Failed(e.to_string()),
                                },
                                Err(why) => AiSeedResult::Failed(why),
                            },
                            Err(why) => AiSeedResult::Failed(why),
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
    // **The shell this session is actually running**, which is not the same as
    // the picker's row: `term_shell_selected` is an index into
    // `detect_shells()`, and a saved profile this launch could not detect is
    // not in that list at all. Both the save and the Restart read this, so
    // neither can silently swap the user's shell for `detected[0]` — see
    // `shell::shell_to_persist`. `term_apply_shell` is the only writer.
    let current_shell = RwSignal::new(term_prefs.shell.clone());
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

    let terminal: Rc<RefCell<Option<schemaic_term::Terminal>>> = Rc::new(RefCell::new(None));
    // Which engine the terminal is a CLI for, or `None` for an ordinary shell —
    // the panel title's badge. Every install sets it, since a session is only
    // ever replaced, never layered.
    let term_db_label: RwSignal<Option<String>> = RwSignal::new(None);

    // **Replacing the terminal session is one function, and this is it.**
    //
    // It was written out at five sites, and three of the five things each has to
    // get right had already drifted between them: `term_apply_shell` was the one
    // that did *not* notify after installing, so the panel went on drawing the
    // previous session's last snapshot until the new shell wrote its first byte;
    // the initial spawn was the one that did not set the badge, relying on the
    // signal's initial `None` while the comment above it stated the rule as
    // "every respawn sets it"; and the tunnel-message path swallowed a spawn
    // failure with `if let Ok(t)` where the other three logged it.
    //
    // The old session drops as it is replaced, which kills its PTY and child.
    // `what` names the caller in the log and nothing else. Returns whether the
    // new session is running, for the caller that has more to do after
    // (`term_apply_shell` records the profile only if it really started).
    let install_terminal: InstallTerminal = {
        let terminal = terminal.clone();
        let term_dims = term_dims.clone();
        let term_notify = term_notify.clone();
        Rc::new(
            move |cfg: &schemaic_term::ShellConfig, label: Option<String>, what: &str| {
                let (cols, rows) = term_dims.get();
                match schemaic_term::Terminal::spawn(cfg, cols, rows, term_notify.clone()) {
                    Ok(t) => {
                        *terminal.borrow_mut() = Some(t);
                        term_db_label.set(label);
                        // The panel redraws off a notify tick, so without this it
                        // keeps the dead session's last frame on screen.
                        (term_notify)();
                        true
                    }
                    Err(e) => {
                        tracing::error!("{what} failed: {e}");
                        false
                    }
                }
            },
        )
    };
    // The session the window opens with. A plain shell, so no badge.
    (install_terminal)(&init_shell, None, "terminal spawn");

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
        // **Through `shell_to_persist`, which keeps a saved profile this launch
        // could not detect.** The picker index is an index into
        // `detect_shells()`, and that list is short whenever the shell is not
        // reachable at startup — a `wsl.exe -l -q` that exits non-zero returns
        // no WSL rows at all. The index then resolved to `0`, and the effect
        // below (which floem runs *immediately on creation*) rewrote
        // `terminal.json` with `detected[0]` before the window was drawn, with
        // no user action. The choice was gone for good, and the Restart icon
        // dropped the same session into a different shell.
        let shell = schemaic_term::shell::shell_to_persist(
            current_shell.get_untracked().as_ref(),
            &term_shells.get_untracked(),
            term_shell_selected.get_untracked(),
        );
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
        let install_terminal = install_terminal.clone();
        Rc::new(move || {
            // The shell that is running, not the picker's row: on a launch
            // that could not detect the saved profile the index is `0`, and
            // Restart replaced the user's session with `detected[0]`
            // unexplained.
            let cfg = schemaic_term::shell::shell_to_persist(
                current_shell.get_untracked().as_ref(),
                &term_shells.get_untracked(),
                term_shell_selected.get_untracked(),
            )
            .map(|p| p.config())
            .unwrap_or_else(schemaic_term::shell::default_shell);
            // Back to a plain shell, so no badge.
            (install_terminal)(&cfg, None, "terminal restart");
        })
    };
    // Open the DB CLI for the active connection in the terminal — `mysql`/
    // `mariadb` or `psql`, per the connection's engine — optionally scoped to a
    // database. Reveals the terminal panel and respawns it as a dedicated client
    // session.
    let open_db_cli: Rc<dyn Fn(Option<String>)> = {
        let install_terminal = install_terminal.clone();
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
                        // A message, not a session — nothing to badge. And a
                        // failure to spawn even *that* is logged now; this was
                        // the site that swallowed it.
                        (install_terminal)(&cfg, None, "tunnel message shell");
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
                    psql_shell(&conn, &target)
                }
                // `db` is ignored: a SQLite connection's one database is the file
                // itself, which the config already names.
                schemaic_db::Engine::Sqlite => {
                    sqlite_shell(&conn).ok_or("No sqlite3 client found on PATH.")
                }
                schemaic_db::Engine::MySql => mysql_shell(&conn, db.as_deref()),
            };
            // Badge the panel only for a session that really is a client. The
            // no-client arm spawns a message instead, which is nobody's engine.
            let label = built
                .is_ok()
                .then(|| schemaic_core::connection::engine_label(&conn.db_type));
            let cfg = built.unwrap_or_else(message_shell);
            (install_terminal)(&cfg, label, "db cli spawn");
        })
    };
    let term_apply_shell: Rc<dyn Fn(usize)> = {
        let install_terminal = install_terminal.clone();
        let save_term_prefs = save_term_prefs.clone();
        Rc::new(move |idx: usize| {
            let Some(profile) = term_shells.get_untracked().get(idx).cloned() else {
                return;
            };
            // A shell profile, not a client, so no badge.
            if !(install_terminal)(&profile.config(), None, "terminal respawn") {
                return;
            }
            term_shell_selected.set(idx);
            // The picked profile is what runs now, so it is also what a save and
            // a Restart must name — this is the one writer.
            current_shell.set(Some(profile.clone()));
            // Persist the whole prefs file (shell + appearance).
            (save_term_prefs)();
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
            export_cancel,
            export_erd,
            save_blob,
            load_blob,
            view_blob,
            cancel_blob,
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
                // Pinned to the tab for the reason `gate1_on_tab` gives:
                // `run_plan` re-resolves `active` when it lands, and the gate can
                // hold it for five seconds. **And answered on refusal**, which
                // the plain pinning cannot do: `open_plan` has already set
                // `PlanState::Running` by the time this is called, so a gate
                // that refuses silently leaves the modal on "Explaining…" for
                // ever. Two arguments, carried as a pair so the generic wrapper
                // still applies.
                let g = with_conn_else.clone();
                let f = run_plan.clone();
                let pair: Rc<dyn Fn((String, bool))> =
                    Rc::new(move |(sql, analyze)| f(sql, analyze));
                let refused = plan_refused(plan_state, &run_moved_on);
                let gated = gate1_on_tab_answered(&g, &pair, active, &refused);
                Rc::new(move |sql: String, analyze: bool| gated((sql, analyze)))
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
            date_pick: RwSignal::new(None),
            find_open,
            find_query,
            search_history,
            error_modal_open,
            error_modal_text,
            error_modal_fixable,
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
            compare,
            compare_state,
            compare_selected,
            compare_expanded,
            compare_focus: RwSignal::new(None),
            compare_dbs,
            compare_dbs_err,
            compare_query: RwSignal::new(String::new()),
            compare_show_same: RwSignal::new(false),
            properties,
            properties_state,
            properties_counting,
            properties_count_err,
            users,
            users_state,
            users_filter,
            users_selected,
            users_grants,
            users_generation,
            run_guard,
            snippet_edit: snippet_edit_open,
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
            dump_tables,
            dump_run,
            files_run,
            dump_cancel,
            script_probe,
            script_run,
            script_cancel,
            ddl_cancel,
            run_ddl,
            view_algorithm,
            trigger_functions,
            roles,
            trigger_source,
            routine_source,
            event_source,
            table_stats,
            compare_fetch,
            compare_cancel,
            compare_list_dbs,
            count_rows,
            count_cancel,
            principals,
            grants,
            toggle_table_sizes,
            db_stats,
        }),
        // Reset on every open (`import_view::open_import`), so one bundle serves
        // every table rather than a per-open scope that would need disposing.
        import: schemaic_ui::ImportUi::new(),
        // Same rule as `import` above: one bundle, reset on open.
        dump: schemaic_ui::DumpUi {
            target: RwSignal::new(None),
            tables: RwSignal::new(Vec::new()),
            chosen: RwSignal::new(Vec::new()),
            listing: RwSignal::new(schemaic_core::dump::Listing::Done),
            structure: RwSignal::new(true),
            data: RwSignal::new(true),
            other_objects: RwSignal::new(true),
            drop_if_exists: RwSignal::new(true),
            wrap_transaction: RwSignal::new(true),
            disable_fk_checks: RwSignal::new(true),
            running: RwSignal::new(false),
            progress: dump_progress,
            error: RwSignal::new(None),
            done: RwSignal::new(None),
            generation: RwSignal::new(0),
        },
        // Two signals, no reset-on-open: the modal is raised by an export rather
        // than by a menu, and `save_export` sets both at the launch.
        export: schemaic_ui::ExportUi {
            target: RwSignal::new(None),
            progress: export_progress,
            done: RwSignal::new(None),
            error: RwSignal::new(None),
        },
        // Its own reset lives on the bundle (`BlobUi::open`), because a panel
        // reopened on a second cell must not inherit the first one's "Saved
        // to …" — see that method.
        blob,
        // The load half of `dump` above, and the same bundle rule.
        script: schemaic_ui::ScriptUi {
            target: RwSignal::new(None),
            path: RwSignal::new(None),
            probing: RwSignal::new(false),
            probe: RwSignal::new(None),
            running: RwSignal::new(false),
            progress: script_progress,
            error: RwSignal::new(None),
            done: RwSignal::new(None),
            generation: RwSignal::new(0),
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
            routine: RwSignal::new(None),
            routine_draft: RwSignal::new(schemaic_core::ddl::RoutineDraft::default()),
            routine_body: RwSignal::new(String::new()),
            routine_source_pending: RwSignal::new(false),
            routine_body_stale: RwSignal::new(false),
            event: RwSignal::new(None),
            event_draft: RwSignal::new(schemaic_core::ddl::EventDraft::default()),
            event_body: RwSignal::new(String::new()),
            event_source_pending: RwSignal::new(false),
            event_body_stale: RwSignal::new(false),
            functions: RwSignal::new(Vec::new()),
            database: RwSignal::new(None),
            database_draft: RwSignal::new(schemaic_core::ddl::DatabaseDraft::default()),
            account: RwSignal::new(None),
            account_draft: RwSignal::new(schemaic_core::users::AccountDraft::default()),
            grant: RwSignal::new(None),
            grant_draft: RwSignal::new(schemaic_core::users::GrantDraft::default()),
            roles: RwSignal::new(Vec::new()),
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
            dialect: conn_dialect_memo,
            conn_menu_open,
            conn_status,
            manage_open,
            draft,
            conn_test,
            import: import_ui,
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
            open_import,
            scan_installed_clients,
            add_pasted_url,
            choose_import_file,
            import_chosen,
        }),
        ai: AiUi {
            messages: ai_messages,
            input: ai_input,
            busy: ai_busy,
            settings_open: ai_settings_open,
            harness: ai_harness,
            cli_path: ai_cli_path,
            model: ai_model,
            effort: ai_effort,
            instructions: ai_instructions,
            schema_scope: ai_schema_scope,
            gutter: ai_gutter,
            inline: inline_ai,
            attachment: ai_attachment,
        },
        ai_actions: Rc::new(AiActions {
            // The assistant's DB tools can't reach a dead connection and its
            // schema never loaded, so a turn would answer from nothing.
            send: gate1(&with_conn, &ai_send),
            cancel: ai_cancel,
            new_chat: ai_new_chat,
            regenerate: ai_regenerate,
            apply: ai_apply,
            // Reads the harness signal rather than closing over a value: the
            // modal's red hint must follow the dropdown the user is changing in
            // the same modal, and a captured harness would validate the path
            // against whichever CLI was selected when the closure was built.
            cli_ok: Rc::new(move |p: String| harness_reachable(ai_harness.get_untracked(), &p)),
            inline_run: inline_ai_run,
            inline_cancel: inline_ai_cancel,
            // Both read the harness signal at call time rather than closing over
            // a value: the modal asks them again on every harness change, and a
            // captured answer would describe whichever CLI was selected when the
            // closure was built.
            detect_path: Rc::new(move || detect_bin(ai_harness.get_untracked())),
            constraint_notice: Rc::new(move || {
                let h = ai_harness.get_untracked();
                let bin = harness_bin(h, &ai_cli_path.get_untracked());
                // **The cache, never the probe.** This closure runs inside a
                // `create_memo` on the Floem UI thread, and `probe` spawns a
                // blocking, timeout-free `--help` on a cold key. The harness
                // dropdown produces a cold key by construction — switching
                // harness clears `cli_path`, so the memo and the thread that
                // warms it were started in the same update pass and both
                // missed, leaving the UI thread to pay for it. A miss now shows
                // nothing; `agent_cli::probe_generation` moves when the warm
                // lands and the memo runs again.
                //
                // Read *tracked*, which is what subscribes the memo: the warming
                // thread bumps this when the answer lands, and without the
                // subscription a cold key would show nothing until some
                // unrelated change re-ran the memo.
                let _ = ai_probe_gen.get();
                crate::agent_cli::probe_cached(h, &bin)?
                    .constraint
                    .notice(h)
            }),
        }),
        history: HistoryUi {
            entries: history_entries,
        },
        history_actions: Rc::new(HistoryActions {
            clear: clear_history,
            open: open_history,
            remove: remove_history,
        }),
        snippets: SnippetsUi {
            items: snippet_library,
            can_save: can_save_snippet,
        },
        snippet_actions: Rc::new(SnippetActions {
            insert: insert_snippet,
            open_in_tab: open_snippet_in_tab,
            save_current: save_snippet_current,
            rename: rename_snippet,
            set_abbrev: set_snippet_abbrev,
            set_body: set_snippet_body,
            set_scope: set_snippet_scope,
            edit: {
                // The editor is a UI concern (the modal lives in `schemaic-ui`),
                // so the app only flips the signal that opens it.
                let overlay_snippet_edit = snippet_edit_open;
                Rc::new(move |id: u64| overlay_snippet_edit.set(Some(id)))
            },
            duplicate: duplicate_snippet,
            remove: remove_snippet,
        }),
        activity: ActivityUi {
            state: activity_state,
            interval: activity_interval,
            busy: activity_busy,
            kill_error: activity_kill_error,
            menu_open: activity_menu_open,
            menu_anchor: activity_menu_anchor,
        },
        activity_actions: Rc::new(ActivityActions {
            refresh: refresh_activity,
            kill: kill_session,
            set_interval: set_activity_interval,
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
            ui_scale,
            editor_font,
            tab_width,
            soft_tabs,
            word_wrap,
            row_limit,
            statement_timeout,
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
        open_config_dir: Rc::new(open_config_dir),
    };
    // Every config file loaded *during this build* has been loaded by now. If any
    // of them was unreadable it was preserved as `.corrupt` and recovered from
    // the backup or defaults — which from the user's side looks like their
    // connections or preferences just vanished, so say so instead of only
    // logging it.
    //
    // **Not the only drain any more, and that was the bug.** Three loads are
    // lazy and run long after this line — the ER diagram's layout read and its
    // save-side re-read, and the layout prune in `delete_conn_now` — so a
    // corrupt `diagrams.json` was renamed aside and reported to nobody. They
    // call `report_recoveries` too; this is the same function, not a fourth
    // spelling of it.
    schemaic_ui::report_recoveries(error_modal_text, error_modal_open);
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
    /// The watched connection's SQL dialect, for the baseline poll's
    /// `analyze_edit`. Carried rather than re-resolved: `monitor_apply` runs on
    /// the UI thread with no `Db` in hand, and the identity key it asks for must
    /// not depend on connecting again.
    dialect: SqlDialect,
    /// The poll's `ORDER BY`, resolved **once** at open — `None` when the
    /// schema could not answer, in which case the whole session polls unordered.
    ///
    /// **Pinned for the same reason `key_cols` is, and it was the one identity
    /// input that wasn't.** It used to be recomputed from the live `db_nodes` on
    /// every tick, *and again* on every reply to decide
    /// [`Snapshot::ordered`] — two evaluations of a signal that moves under
    /// them. On a fresh connect the schema routinely lands between the two: poll
    /// 1 goes out with no `ORDER BY`, the schema arrives while it is in flight,
    /// and its reply is stamped ordered. `diff_snapshots` then reads an
    /// arbitrary sample as an ordered prefix and reports every row of it the
    /// real first page does not hold as a DELETE, cells and all, into an
    /// exportable log — see
    /// `monitor::tests::an_arbitrary_window_called_ordered_reports_deletes_of_rows_that_are_still_there`.
    /// The second consequence needs no race: recomputed per tick, the *window*
    /// itself changes mid-session, which is the same diff by another door.
    ///
    /// Resolving once also settles a latent gap. `monitor_order_key` reads
    /// `db_nodes`, which every other consumer in this file treats as the
    /// **active** connection's tree and guards accordingly (`db_stats`), while
    /// the monitor's target is the captured `target.0`. Asked once, at open,
    /// the two are the same connection by construction; the modal's backdrop is
    /// what made that unreachable rather than anything here.
    order_by: Option<Vec<String>>,
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
    // window sliding as data changing — so the key is `MonitorCtx`'s, resolved
    // once at open, not re-derived here from a signal that moves between ticks.
    let order_by = ctx.order_by.clone();
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

/// The monitored table's **row-identity** column names, for the poll's
/// `ORDER BY`.
///
/// **The key the snapshot is diffed by, not the primary key.** This collected
/// `primary_key` columns and answered `None` when there were none — while the
/// key `Snapshot::from_result` is actually keyed by comes from `analyze_edit`
/// → `resolve_key`, which falls back to a unique NOT NULL index. So the whole
/// band "no PK, but a usable key exists" polled unordered and diffed anyway,
/// and the exportable log filled with inserts and deletes that never happened.
/// `core::edit::order_key_columns` is that same ladder, over the table alone.
///
/// `None` when the schema isn't loaded or the table has no key at all — the
/// monitor then polls unordered, and because the snapshot records that
/// separately from whether the window was full, the diff attributes nothing at
/// its edges rather than reporting the whole window.
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
    schemaic_core::edit::order_key_columns(&table)
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
                let model = analyze_edit(&rs, ctx.dialect, |db, ns, table| {
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
            // **The two facts the snapshot needs, recorded separately.**
            // Whether the poll carried an `ORDER BY` over the row key is what
            // licenses treating the tail as the window sliding; whether the
            // fetch came back at its limit is what says there *is* a tail. A
            // window that is full but unordered is neither, and reporting it
            // whole is what put inserts and deletes that never happened into
            // an exportable log — see `Snapshot::window_full`.
            let full = rs.row_count() >= MONITOR_LIMIT;
            // **The field the fetch used**, so the flag cannot describe a query
            // it did not shape. This asked `monitor_order_key` a second time,
            // here, on the reply — and answered `Some` for a window that had
            // gone out with no `ORDER BY` at all whenever the schema landed in
            // between. See `MonitorCtx::order_by`.
            let ordered = ctx.order_by.is_some();
            ctx.partial.set(full);
            let snap = Snapshot::from_result(&rs, &key_cols).window(ordered, full);
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

/// Bump the Server Activity generation and arm the poll loop under the new one.
///
/// **The two halves are one operation.** A bump strands every timer and every
/// in-flight fetch armed under the old generation — that is what it is for — but
/// [`activity_poll`]'s loop is one of the things it strands, so a bump that
/// nothing re-arms doesn't pause the auto-refresh, it ends it until something
/// else re-runs the panel effect. The kill handler bumped without arming and
/// froze the panel permanently after any successful kill.
///
/// `poll` is the caller's answer to "should this panel be polling at all" —
/// false bumps only, which is how closing the panel or losing focus stops the
/// loop without starting another. `activity_poll` still refuses an interval of
/// `0` (auto-refresh off) on its own.
///
/// Returns the new generation, which the caller needs for anything else it
/// stamps against this one.
fn rearm_activity(
    gen_sig: RwSignal<u64>,
    interval: floem::reactive::Memo<u64>,
    refresh: Rc<dyn Fn()>,
    poll: bool,
) -> u64 {
    let generation = gen_sig.get_untracked().wrapping_add(1);
    gen_sig.set(generation);
    if poll {
        activity_poll(generation, gen_sig, interval, refresh);
    }
    generation
}

/// Arm the Server Activity panel's next poll, and keep arming after each one.
///
/// `generation` is the value `activity_gen` held when this loop was started. Every
/// tick re-checks it and stops the moment it differs, which is how closing the
/// panel, switching connections or changing the interval ends the old loop —
/// without it, each of those would start a second one and leave the first running
/// forever.
///
/// The interval is read *fresh* on each arm rather than captured, and `0` (off)
/// ends the loop; turning it back on re-arms through the effect that owns the
/// generation.
fn activity_poll(
    generation: u64,
    gen_sig: RwSignal<u64>,
    interval: floem::reactive::Memo<u64>,
    refresh: Rc<dyn Fn()>,
) {
    // `try_get_untracked` throughout: at shutdown these signals are disposed while
    // a timer may still be pending, and a plain read of a freed signal panics —
    // the same rule the resource sampler's tick follows.
    let secs = interval.try_get_untracked().unwrap_or(0);
    if secs == 0 || gen_sig.try_get_untracked() != Some(generation) {
        return;
    }
    exec_after(Duration::from_secs(secs), move |_| {
        if gen_sig.try_get_untracked() != Some(generation) {
            return;
        }
        (refresh)();
        activity_poll(generation, gen_sig, interval, refresh);
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

/// A statement timeout armed over one run's own cancellation token.
///
/// **It fires the token the Cancel button fires**, which is the whole design:
/// cancelling a running statement already works end to end — MySQL gets a
/// `KILL QUERY` on a second connection, PostgreSQL a `cancel_query`, SQLite an
/// interrupt handle — so a timeout is a clock wired to that, not a second way
/// to stop a query. Nothing new can go wrong at the database.
///
/// The sleeper races `done` rather than running free, so a statement that
/// finishes in a second does not leave an hour-long task holding a token; and
/// `fired` is what lets the settle path tell a timeout from the user pressing
/// Cancel, which arrive as the identical [`DbError::Cancelled`].
///
/// **One of these per statement, not per run.** "Statement timeout" is what the
/// setting says, so a ten-statement script with a one-minute timeout gets a
/// minute *each* — a batch bounded as a whole would kill honest work whose only
/// fault was being long. A statement that does expire still stops the batch,
/// because the token it fires is the batch's.
struct RunTimeout {
    /// Cancelled when the statement settles, ending the sleeper. Cancelled by
    /// `Drop`, so a path that returns early can't leak the task.
    done: CancellationToken,
    fired: Arc<std::sync::atomic::AtomicBool>,
}

impl RunTimeout {
    /// Arm `secs` over `token`. `0` — the default — arms nothing at all, so an
    /// unconfigured app spawns no task per statement.
    ///
    /// Must be called from inside the runtime (it `tokio::spawn`s), i.e. within
    /// the `handle.spawn` block that runs the statement, not on the UI thread.
    fn arm(token: &CancellationToken, secs: u64) -> RunTimeout {
        let done = CancellationToken::new();
        let fired = Arc::new(std::sync::atomic::AtomicBool::new(false));
        if let Some(after) = persist::statement_timeout(secs) {
            let token = token.clone();
            let settled = done.clone();
            let flag = fired.clone();
            tokio::spawn(async move {
                tokio::select! {
                    _ = tokio::time::sleep(after) => {
                        // Set before cancelling, so the settle path can never
                        // read the cancellation without the reason for it.
                        flag.store(true, std::sync::atomic::Ordering::SeqCst);
                        tracing::warn!("statement timeout after {secs}s — cancelling the run");
                        token.cancel();
                    }
                    _ = settled.cancelled() => {}
                }
            });
        }
        RunTimeout { done, fired }
    }

    /// Whether the watchdog fired. Disarming is [`Drop`]'s job, so reading this
    /// twice — or not at all — is safe.
    ///
    /// A `true` here alongside an `Ok` result is possible: the timeout landed in
    /// the gap between the rows arriving and this call. That is why callers
    /// consult it only on [`DbError::Cancelled`].
    fn fired(&self) -> bool {
        self.fired.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl Drop for RunTimeout {
    fn drop(&mut self) {
        // Dropping a `CancellationToken` does not cancel it, so this has to be
        // explicit — otherwise every finished statement would leave an
        // hour-long `sleep` behind, one per run, for the life of the process.
        self.done.cancel();
    }
}

/// What the results pane says when the timeout cancelled a statement.
///
/// A bare "Cancelled" is what the user's own Cancel button produces, and
/// reusing it here would leave someone staring at a query they never stopped.
/// So: what happened, the setting that caused it *in the same words the
/// dropdown uses*, and where to change it.
fn timeout_message(secs: u64) -> String {
    format!(
        "Cancelled: the statement ran longer than the {} statement timeout. \
         Change or turn it off in Settings → Query.",
        persist::statement_timeout_label(secs).to_lowercase()
    )
}

/// Reveal the app's config directory — `schemaic.log` and the rest of the state
/// — in the OS file manager.
///
/// Best-effort and silent on failure, the same contract [`open_url`] has: the
/// worst case is a button that appears to do nothing, and the Settings row still
/// shows the path in text for the user to copy. `create_dir_all` first because
/// the directory is created lazily by the first save, and a fresh install that
/// has written nothing yet would otherwise open a file manager on a path that
/// does not exist.
fn open_config_dir() {
    let Some(dir) = schemaic_core::persist::config_dir() else {
        tracing::warn!("no config directory to open");
        return;
    };
    // Owner-only if we are the one creating it — the same rule the logger and
    // every config write follow, and this button is one of the three places that
    // can bring the directory into existence.
    let _ = schemaic_core::persist::ensure_private_dir(&dir);
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer").arg(&dir).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(&dir).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(&dir).spawn();
    }
}

/// Open an http(s) URL in the OS default browser (clicked terminal link).
///
/// The whole decision — may this be opened, and by which program — belongs to
/// [`launch::url_open_argv`], which is where its tests are. This function does
/// nothing but spawn what it is given, and that is the point: the previous
/// spelling built `cmd /C start "" <url>` itself, and `&` in a URL the terminal
/// had merely *printed* became a second command.
fn open_url(url: &str) {
    let Some(argv) = launch::url_open_argv(url) else {
        return;
    };
    let mut cmd = std::process::Command::new(&argv[0]);
    cmd.args(&argv[1..]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = cmd.spawn();
}

#[cfg(test)]
mod app_tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::{
        Action, CliLauncher, ConnGate, ConnGateElse, Refusal, RunTimeout, gate1, gate1_on_tab,
        gate1_on_tab_answered, inline_outcome, mysql_shell_config, owning_tab_of,
        plan_refusal_text, plan_refused, psql_database, psql_shell_config, resolve_native_cli,
        sqlite_shell_config, test_outcome, timeout_message, tx_engine, unique_name,
    };
    use floem::prelude::{SignalGet, SignalUpdate};
    use floem::reactive::RwSignal;
    use schemaic_core::connection::Connection;
    use schemaic_ui::{InlineAiState, TestState};
    use tokio_util::sync::CancellationToken;

    /// **The raw `run` binding has exactly two callers, and both hold an
    /// approved statement.**
    ///
    /// `run` at `:2401` is `run_query_core` with no `sql::run_verdict`, no
    /// `params::prepare_run` and no `read_only` term — `guarded_run` is what
    /// `TabsActions::run` is wired to, and CLAUDE.md's invariant is that the
    /// guard lives on the action rather than in a caller. Two other closures
    /// reach the raw one (`spawn_table_tab` and `open_table_filtered`), and both
    /// now take their SQL out of a [`schemaic_ui::RerunRequest`], which only
    /// `sql::rerunnable_for_export` can mint.
    ///
    /// The type carries that for the two `TabsActions` fields; what it cannot
    /// carry is a *third* closure calling `run` with a bare `String`, which is
    /// exactly how `open_table_filtered` came to be unguarded. So the count is
    /// the gate: a new call site fails this test and has to say, here, what
    /// refuses it.
    /// Both manual-run paths have to call it, which is the half a unit test on
    /// `Tab` cannot see: the bug was `run_all` not asking, not
    /// `start_manual_run` answering wrongly.
    #[test]
    fn both_run_and_run_all_start_a_manual_run() {
        let src = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join("main.rs"),
        )
        .expect("main.rs");
        let body = src
            .split("#[cfg(test)]")
            .next()
            .expect("production code")
            .to_string();
        assert!(
            body.contains("tab.start_manual_run(Some(&sql));"),
            "the single-statement run no longer records its base"
        );
        assert!(
            body.contains("tab.start_manual_run(None);"),
            "Run Everything no longer clears the previous run's base — a batch \
             panel's filter row will rebuild the last single statement"
        );
    }

    #[test]
    fn the_unguarded_run_has_only_its_two_stated_callers() {
        let src = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join("main.rs"),
        )
        .expect("this file's own source");
        // Comment lines out, so the prose above (which names `run(sql)`) is not
        // itself a call site. A call is `run(` at the head of a statement —
        // `guarded_run(`, `run_query_core(` and the rest end in other characters
        // before the paren and do not match.
        let calls: Vec<&str> = src
            .lines()
            .map(str::trim)
            .filter(|l| !l.starts_with("//"))
            .filter(|l| l.starts_with("run(") && l.ends_with(");"))
            .collect();
        assert_eq!(
            calls,
            ["run(req.into_sql());", "run(sql);"],
            "the raw `run` gained or lost a caller: every one must take its SQL \
             from a `RerunRequest`, which only `sql::rerunnable_for_export` mints"
        );
    }

    /// **The regression the fix that introduced `targets_same_server` could not
    /// have.**
    ///
    /// Two Schemaic connections pointing at one server is ordinary —
    /// `local (app)` and `local (root)`, or two entries differing only in
    /// default database. A killed session on one of them belongs to a Manual tab
    /// bound to the *other*, and the lookup used to compare `conn_id`: it
    /// rejected that true match, so the tab kept a dead socket with Commit and
    /// Rollback still offered, while the confirm modal described the user's own
    /// uncommitted work as somebody else's client.
    ///
    /// `targets_same_server` was never wrong and is exhaustively tested. The
    /// composition around it was, and while it lived as an `Rc<dyn Fn>` inside
    /// `app_view` nothing could call it — which is the whole reason
    /// `owning_tab_of` is a free function now.
    #[test]
    fn a_tab_on_a_second_connection_to_the_same_server_still_owns_its_session() {
        let base = conn();
        // Same host, port, user, engine — a different Schemaic entry for one
        // server, which is what `targets_same_server` exists to recognise.
        let other = Connection {
            id: 2,
            name: "the same box, again".to_string(),
            database: "reporting".to_string(),
            ..base.clone()
        };
        let conns = vec![base.clone(), other.clone()];
        // Tab 7 is pinned to a session whose server id is 42, and the tab is
        // bound to connection 2. The kill arrives naming connection 1.
        let sessions = [(7usize, Some(42i64))];
        let tabs = [(7usize, 2u64)];

        assert_eq!(
            owning_tab_of(&sessions, &tabs, &conns, 1, 42),
            Some(7),
            "the tab is on the same server, so it owns the killed session"
        );
        // And the same connection is of course still a match.
        assert_eq!(owning_tab_of(&sessions, &[(7, 2)], &conns, 2, 42), Some(7));
    }

    /// **A server id is only unique on its own server.** Thread ids and backend
    /// pids are small integers from each server's own counter, so two tabs on
    /// two *different* servers routinely hold the same one — matching on the id
    /// alone closed a transaction on a server nobody had touched.
    #[test]
    fn a_matching_id_on_a_different_server_is_not_a_match() {
        let here = conn();
        let elsewhere = Connection {
            id: 2,
            name: "a different box".to_string(),
            host: "10.9.9.9".to_string(),
            ..here.clone()
        };
        let conns = vec![here, elsewhere];
        assert_eq!(
            owning_tab_of(&[(7, Some(42))], &[(7, 2)], &conns, 1, 42),
            None,
            "thread 42 on one server is not thread 42 on another"
        );
    }

    /// The ordinary negatives: no tab pinned to that session, and a tab pinned
    /// to a different one.
    #[test]
    fn a_session_no_tab_holds_owns_nothing() {
        let conns = vec![conn()];
        assert_eq!(owning_tab_of(&[], &[], &conns, 1, 42), None);
        assert_eq!(
            owning_tab_of(&[(7, Some(43))], &[(7, 1)], &conns, 1, 42),
            None,
            "another session's tab is not this one's"
        );
        assert_eq!(
            owning_tab_of(&[(7, None)], &[(7, 1)], &conns, 1, 42),
            None,
            "an auto-commit tab pins no session at all"
        );
        // A session whose tab has since closed: the id is in the map, the tab is
        // gone. Reaching for it must not panic or invent an owner.
        assert_eq!(owning_tab_of(&[(7, Some(42))], &[], &conns, 1, 42), None);
        // And a kill naming a connection that has been deleted, where the tab is
        // on a *different* one — there is nothing left to compare against.
        assert_eq!(
            owning_tab_of(&[(7, Some(42))], &[(7, 2)], &conns, 99, 42),
            None
        );
    }

    /// **The monitor's order key is resolved once, and the resolver has one
    /// caller.**
    ///
    /// `MonitorCtx` pins every identity input for the life of a session —
    /// `key_cols` is written exactly once on the baseline poll, `dialect` is
    /// carried with a doc saying the key "must not depend on connecting again".
    /// `order_by` was the one that wasn't: `monitor_order_key` was called from
    /// `monitor_tick` to shape the fetch **and again** from `monitor_apply` to
    /// stamp `Snapshot::ordered` on the reply, both reading a `db_nodes` that
    /// moves under them. On a fresh connect the schema lands between the two,
    /// and the flag then describes a query it did not shape — which
    /// `diff_snapshots` reads as licence to compare an arbitrary sample as an
    /// ordered prefix. What that costs is pinned in
    /// `monitor::an_arbitrary_window_called_ordered_reports_deletes_of_rows_that_are_still_there`.
    ///
    /// A source gate because the defect is *a second call*, not a wrong value:
    /// both calls were individually correct. The one that remains is in
    /// `open_monitor`, before the first tick.
    #[test]
    fn the_monitors_order_key_is_resolved_at_open_and_nowhere_else() {
        let src = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join("main.rs"),
        )
        .expect("this file's own source");
        let body = src
            .split("#[cfg(test)]")
            .next()
            .expect("production code")
            .to_string();
        let calls: Vec<&str> = body
            .lines()
            .map(str::trim)
            .filter(|l| !l.starts_with("//") && !l.starts_with("///"))
            .filter(|l| l.contains("monitor_order_key(") && !l.starts_with("fn "))
            .collect();
        assert_eq!(
            calls,
            ["order_by: monitor_order_key(db_nodes, &source),"],
            "the order key must be resolved once, at open, and carried on \
             `MonitorCtx` — a second call can answer differently from the one \
             that shaped the fetch"
        );
    }

    /// **No arming site decides for itself that someone is watching.**
    ///
    /// `rearm_activity`'s last argument is "should the loop run", and
    /// `activity::should_poll` is the function that answers it — three
    /// conjuncts, the third (`right_panel_visible`) added after a zero-width
    /// panel went on connecting every two seconds for nobody. The kill
    /// handler passed a literal `true`, because `activity_polling` was defined
    /// eighty lines *below* it: a successful kill restarted auto-refresh on a
    /// panel switched away from or a window that had lost focus, reinstating
    /// exactly the load the third conjunct removed — and doing it right after
    /// the one action on this panel that is reliably followed by looking
    /// somewhere else.
    ///
    /// A source gate rather than a unit test because the defect was a literal
    /// at one call site out of three. Nothing about the value is wrong; the
    /// wrong thing is asking the question by hand.
    #[test]
    fn every_poll_arming_asks_whether_anyone_is_watching() {
        let src = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join("main.rs"),
        )
        .expect("this file's own source");
        let body = src
            .split("#[cfg(test)]")
            .next()
            .expect("production code")
            .to_string();
        // `rearm_activity(…)` spans several lines at two of the three sites and
        // its arguments carry parens of their own (`refresh.clone()`), so the
        // call is read by balancing rather than by matching a line or the first
        // `)`. The `fn` definition is skipped — it is not an arming site.
        let mut args: Vec<String> = Vec::new();
        for (i, _) in body.match_indices("rearm_activity(") {
            if body[..i].trim_end().ends_with("fn") {
                continue;
            }
            let rest = &body[i + "rearm_activity(".len()..];
            let mut depth = 0usize;
            let end = rest
                .char_indices()
                .find(|&(_, c)| {
                    match c {
                        '(' => depth += 1,
                        ')' if depth == 0 => return true,
                        ')' => depth -= 1,
                        _ => {}
                    }
                    false
                })
                .map(|(j, _)| j)
                .expect("a call, closed");
            let last = rest[..end]
                .rsplit(',')
                .map(str::trim)
                .find(|a| !a.is_empty())
                .expect("four arguments");
            args.push(last.to_string());
        }
        assert_eq!(
            args.len(),
            3,
            "an arming site appeared or vanished: {args:?}"
        );
        assert!(
            args.iter()
                .all(|a| a == "open" || a == "activity_polling()"),
            "a poll was armed on something other than `should_poll`'s answer: \
             {args:?}"
        );
    }

    /// **Nothing here re-spells "which differences can a plan carry".**
    ///
    /// `compare::is_planned`'s doc argues the case at length — the footer's
    /// count, the button's enabled state and the statements actually built have
    /// to ask one question, and the cheapest guarantee is that there be only one
    /// spelling of it. It says specifically that the predicate lives in
    /// `schemaic-core` *"rather than in the view that calls it"*, because the
    /// decision was untestable where it sat.
    ///
    /// The compare seed then re-spelled it in a **third** crate:
    /// `c.differences().filter(|e| !e.needs_source()).map(|e| e.key())`, which
    /// drops `unplannable()` — so the tree would open with objects ticked that
    /// the footer refuses to count. Nothing diverges today only because
    /// `unplannable` has no reachable producer; the seed now calls
    /// `selectable_keys(RowFilter::default())`, the function written to answer
    /// exactly this.
    ///
    /// A source gate rather than a unit test because the defect is a *site*,
    /// not a value: a fifth spelling compiles and passes every assertion in the
    /// workspace. `compare_view.rs`' own `needs_source()` is not one — it draws
    /// the tick-box's absence, which is the reason the exclusion exists.
    #[test]
    fn the_compare_seed_does_not_respell_the_plannable_predicate() {
        let src = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join("main.rs"),
        )
        .expect("this file's own source");
        let body = src
            .split("#[cfg(test)]")
            .next()
            .expect("production code")
            .to_string();
        let hits: Vec<&str> = body
            .lines()
            .map(str::trim)
            .filter(|l| !l.starts_with("//"))
            .filter(|l| l.contains("needs_source") || l.contains("unplannable"))
            .collect();
        assert!(
            hits.is_empty(),
            "this file must ask `SchemaComparison` which entries a plan can \
             carry, not re-derive it: {hits:?}"
        );
    }

    /// A gate that behaves like `with_conn` on a **down** connection: it holds
    /// the action instead of running it, and the caller decides when it lands.
    /// That five-second hold is the whole window this is about.
    fn deferring_gate() -> (ConnGate, Rc<RefCell<Option<Action>>>) {
        let held: Rc<RefCell<Option<Action>>> = Rc::new(RefCell::new(None));
        let slot = held.clone();
        let gate: ConnGate = Rc::new(move |a: Action| *slot.borrow_mut() = Some(a));
        (gate, held)
    }

    /// **The write guard judges the active tab at press time; the run resolves
    /// it again when it lands, and up to five seconds pass in between.**
    ///
    /// `with_conn` spends `PING_TIMEOUT` re-checking a `Disconnected`
    /// connection, and nothing is on screen while it does: the guard bar has
    /// just been taken down and no panel has been opened, so clicking another
    /// tab is the natural response to a Run that appears to have done nothing.
    /// `run_query_core`'s first line is `let id = active.get_untracked();`, so a
    /// `DELETE` confirmed against a tab bound to `staging` ran against a tab
    /// bound to `production` — reported into its panel, recorded in its history.
    ///
    /// The refusal is the fix, not a re-target: what was judged is no longer
    /// what would run, and a refusal is *strictly stronger*, which is the
    /// direction the write-guard invariant requires.
    #[test]
    fn a_deferred_run_does_not_land_on_a_tab_the_user_switched_to() {
        let active = RwSignal::new(7usize);
        let ran: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let refused = RwSignal::new(0usize);

        let sink = ran.clone();
        let action: Rc<dyn Fn(String)> = Rc::new(move |sql: String| sink.borrow_mut().push(sql));
        let moved_on: Rc<dyn Fn()> = Rc::new(move || refused.update(|n| *n += 1));

        let (gate, held) = deferring_gate();
        let gated = gate1_on_tab(&gate, &action, active, &moved_on);

        // Pressed on tab 7, held by the gate…
        gated("DELETE FROM sessions".to_string());
        assert!(ran.borrow().is_empty(), "the gate is holding it");
        // …the user switches to tab 9, and *then* the health check answers.
        active.set(9);
        (held.borrow_mut().take().expect("the gate held it"))();

        assert!(
            ran.borrow().is_empty(),
            "the statement ran on a tab it was never judged against: {:?}",
            ran.borrow()
        );
        assert_eq!(refused.get_untracked(), 1, "and the user was told why");
    }

    /// And the ordinary case is untouched: the tab the run was started on is
    /// still the active one when the check answers, so it runs. Without this the
    /// gate could pass by refusing everything.
    #[test]
    fn a_deferred_run_still_lands_on_the_tab_it_was_started_from() {
        let active = RwSignal::new(7usize);
        let ran: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let refused = RwSignal::new(0usize);

        let sink = ran.clone();
        let action: Rc<dyn Fn(String)> = Rc::new(move |sql: String| sink.borrow_mut().push(sql));
        let moved_on: Rc<dyn Fn()> = Rc::new(move || refused.update(|n| *n += 1));

        let (gate, held) = deferring_gate();
        let gated = gate1_on_tab(&gate, &action, active, &moved_on);
        gated("DELETE FROM sessions".to_string());
        // The user goes away and comes back, which is not a change.
        active.set(9);
        active.set(7);
        (held.borrow_mut().take().expect("the gate held it"))();

        assert_eq!(ran.borrow().as_slice(), ["DELETE FROM sessions"]);
        assert_eq!(refused.get_untracked(), 0);
    }

    /// **The seam, and the reason the pinned wrapper is a different function.**
    /// `gate1` is what the tab-bound runs used, and it carries the *argument*
    /// into the deferred closure and nothing else — no tab, no connection, no
    /// generation. Fed the same sequence, it runs the statement on tab 9. That
    /// is the defect, asserted rather than described, and it is also why
    /// `add_tab` and `ai_send` keep `gate1`: neither is bound to a tab, and
    /// pinning them would refuse a gesture that is still correct.
    #[test]
    fn the_unpinned_gate_is_the_one_that_retargets() {
        let active = RwSignal::new(7usize);
        let ran: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = ran.clone();
        let action: Rc<dyn Fn(String)> = Rc::new(move |sql: String| sink.borrow_mut().push(sql));

        let (gate, held) = deferring_gate();
        let gated = gate1(&gate, &action);
        gated("DELETE FROM sessions".to_string());
        active.set(9);
        (held.borrow_mut().take().expect("the gate held it"))();

        assert_eq!(
            ran.borrow().len(),
            1,
            "gate1 runs whatever it was handed, wherever the user now is"
        );
    }

    /// A gate that behaves like `with_conn_else` when the ping comes back
    /// **failed**: the action is dropped and the refusal is taken. That is the
    /// branch the query-plan modal was never told about.
    fn refusing_gate() -> ConnGateElse {
        Rc::new(move |_action: Action, refused: Action| refused())
    }

    /// The same, holding both halves so the caller decides which one lands —
    /// `with_conn_else` on a `Disconnected` connection, before the ping answers.
    #[allow(clippy::type_complexity)]
    fn deferring_gate_else() -> (ConnGateElse, Rc<RefCell<Option<(Action, Action)>>>) {
        let held: Rc<RefCell<Option<(Action, Action)>>> = Rc::new(RefCell::new(None));
        let slot = held.clone();
        let gate: ConnGateElse =
            Rc::new(move |a: Action, r: Action| *slot.borrow_mut() = Some((a, r)));
        (gate, held)
    }

    /// **The query-plan modal spun on "Explaining…" for ever when the connection
    /// was down.**
    ///
    /// `open_plan` sets `PlanState::Running` and *then* calls the gated action.
    /// `with_conn` saw `is_down()`, pinged, the ping failed, and it took the
    /// refusal branch: error modal, and the action never called. The inner
    /// `run_plan` is the only writer of `PlanState::Failed` on that route, so
    /// `plan_state` stayed `Running` and the body rendered `loading_dots`
    /// indefinitely behind the error. Dismissing the error left a modal claiming
    /// work was in flight over a server that was down; only Escape got out.
    ///
    /// This asserts the **composition** — the gate, the pinned wrapper and the
    /// modal's answer together — because each piece was individually fine: every
    /// early exit *inside* `run_plan` already reported into `plan_state`, and it
    /// was the gate wrapped *around* it that answered nobody.
    #[test]
    fn a_refused_plan_launch_leaves_the_modal_saying_so() {
        let active = RwSignal::new(7usize);
        let plan_state = RwSignal::new(schemaic_ui::PlanState::Running);
        let ran: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let moved_on_said = RwSignal::new(0usize);

        let sink = ran.clone();
        let action: Rc<dyn Fn((String, bool))> =
            Rc::new(move |(sql, _analyze)| sink.borrow_mut().push(sql));
        let moved_on: Rc<dyn Fn()> = Rc::new(move || moved_on_said.update(|n| *n += 1));

        let gated = gate1_on_tab_answered(
            &refusing_gate(),
            &action,
            active,
            &plan_refused(plan_state, &moved_on),
        );
        gated(("SELECT 1".to_string(), false));

        assert!(ran.borrow().is_empty(), "the gate refused, so nothing ran");
        match plan_state.get_untracked() {
            schemaic_ui::PlanState::Failed(msg) => assert_eq!(
                msg,
                plan_refusal_text(Refusal::NotConnected),
                "and the modal says which refusal it was"
            ),
            other => panic!("the modal is still spinning: {other:?}"),
        }
        assert_eq!(
            moved_on_said.get_untracked(),
            0,
            "the tab didn't move — that modal belongs to the other refusal"
        );
    }

    /// The second refusal, and the one the pinned wrapper owns rather than the
    /// gate: the check takes up to five seconds, the user clicks another tab
    /// while it does, and the deferred launch is dropped. It owes the modal the
    /// same answer — and *additionally* the "you switched tabs" error every
    /// other pinned action raises, which is why both channels are asserted.
    #[test]
    fn a_plan_launch_that_outlived_its_tab_also_answers_the_modal() {
        let active = RwSignal::new(7usize);
        let plan_state = RwSignal::new(schemaic_ui::PlanState::Running);
        let ran: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let moved_on_said = RwSignal::new(0usize);

        let sink = ran.clone();
        let action: Rc<dyn Fn((String, bool))> =
            Rc::new(move |(sql, _analyze)| sink.borrow_mut().push(sql));
        let moved_on: Rc<dyn Fn()> = Rc::new(move || moved_on_said.update(|n| *n += 1));

        let (gate, held) = deferring_gate_else();
        let gated =
            gate1_on_tab_answered(&gate, &action, active, &plan_refused(plan_state, &moved_on));
        gated(("SELECT 1".to_string(), false));
        active.set(9);
        // The ping came back *good*, so the gate runs the action half — and the
        // pinning is what refuses.
        (held.borrow_mut().take().expect("the gate held it").0)();

        assert!(ran.borrow().is_empty(), "it was judged against tab 7");
        assert_eq!(moved_on_said.get_untracked(), 1, "and said why, once");
        match plan_state.get_untracked() {
            schemaic_ui::PlanState::Failed(msg) => {
                assert_eq!(msg, plan_refusal_text(Refusal::TabMovedOn))
            }
            other => panic!("the modal is still spinning: {other:?}"),
        }
    }

    /// And the ordinary case: the gate lets it through on the tab it was started
    /// from, so the action runs and the modal is left alone to be answered by
    /// `run_plan` itself. Without this the wrapper could pass by refusing
    /// everything.
    #[test]
    fn an_allowed_plan_launch_runs_and_writes_no_refusal() {
        let active = RwSignal::new(7usize);
        let plan_state = RwSignal::new(schemaic_ui::PlanState::Running);
        let ran: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));

        let sink = ran.clone();
        let action: Rc<dyn Fn((String, bool))> =
            Rc::new(move |(sql, analyze)| sink.borrow_mut().push(format!("{sql}|{analyze}")));
        let moved_on: Rc<dyn Fn()> = Rc::new(|| {});

        let (gate, held) = deferring_gate_else();
        let gated =
            gate1_on_tab_answered(&gate, &action, active, &plan_refused(plan_state, &moved_on));
        gated(("SELECT 1".to_string(), true));
        (held.borrow_mut().take().expect("the gate held it").0)();

        assert_eq!(ran.borrow().as_slice(), ["SELECT 1|true"]);
        assert!(
            matches!(plan_state.get_untracked(), schemaic_ui::PlanState::Running),
            "nothing refused it, so the state machine is still `run_plan`'s"
        );
    }

    /// **Replacing the terminal session is five things, and it was written out
    /// five times.**
    ///
    /// Three of the five had already drifted: `term_apply_shell` was the one
    /// that did not notify, so the panel kept drawing the dead session's last
    /// frame until the new shell wrote a byte; the initial spawn was the one
    /// that did not set the badge, relying on the signal's initial `None` while
    /// the comment above it said "every respawn sets it"; and the tunnel-message
    /// path swallowed a spawn failure the other three logged.
    ///
    /// Nothing runtime-testable is left — the sequence is four statements inside
    /// a closure in `app_view` — so the subject is the source, and what it says
    /// is that there is one `Terminal::spawn` in the file. That is the property:
    /// a second one is a second copy of the sequence, which is where the drift
    /// came from. B26-L5-01's fix (validating a target before it becomes a
    /// `ShellConfig`) also needs a single choke point to be applied at.
    #[test]
    fn the_terminal_session_is_replaced_in_exactly_one_place() {
        let src = include_str!("main.rs");
        // Assembled rather than written, so this module's own mention of the
        // call is not one of the hits — the trap every source gate in this
        // workspace has to dodge.
        let call = format!("schemaic_term::{}::spawn(", "Terminal");
        let spawns = src.matches(&call).count();
        assert_eq!(
            spawns, 1,
            "found {spawns} `Terminal::spawn` calls — the spawn/install/badge/\
             notify sequence belongs in `install_terminal` alone, because the \
             five hand-written copies it replaced had already disagreed about \
             three of those four steps"
        );
        // And every caller goes through it, rather than one of them reaching
        // past it to the terminal cell.
        let installs = src.matches("(install_terminal)(").count();
        assert!(
            installs >= 5,
            "only {installs} callers of `install_terminal` — did one go back to \
             writing `*terminal.borrow_mut()` itself?"
        );
        let write = format!("*terminal.{}() = Some(", "borrow_mut");
        assert_eq!(
            src.matches(&write).count(),
            1,
            "the terminal cell is written in `install_terminal` and nowhere else"
        );
    }

    /// **The active connection never moves alone**, and the pairing was a caller
    /// obligation two of its three sites did not honour.
    ///
    /// `expanded` is per-connection and is a plain `RwSignal` — the tree writes
    /// it — so unlike `hidden_dbs`, a memo over `active_conn`, it does not
    /// re-derive itself. `switch_conn` reloaded it; the two arms that move the
    /// id when the **active connection is deleted** did not. The survivor's tree
    /// then rendered against the deleted connection's key set, opening databases
    /// nobody had opened there and firing the `fetch_table_stats` `8a75103`
    /// exists to stop — and the first expand or collapse filed that whole set
    /// under the survivor's id, writing the deleted connection's keys back into
    /// `ui_state.json` after the delete had erased them.
    ///
    /// The sequence is two statements inside a closure in `app_view`, so the
    /// subject is the source: there is exactly one such write in the file, and
    /// it is `use_conn`'s. A second one is a site that can forget the reload —
    /// which is what this was. The needle is assembled rather than written, so
    /// this module's own prose is not one of the hits.
    #[test]
    fn the_active_connection_is_moved_in_exactly_one_place() {
        let src = include_str!("main.rs");
        // Assembled so this module's own mention is not a hit.
        let call = format!("active_conn.{}(", "set");
        let sets = src.matches(&call).count();
        assert_eq!(
            sets, 1,
            "found {sets} direct writes to the active-connection signal — \
             moving it also has to reload `expanded` for it, and that pairing \
             belongs in `use_conn` alone"
        );
        // And the one that is there really does both halves.
        let keys = format!("expanded::{}(r, id)", "keys_for");
        assert!(src.contains(&keys), "`use_conn` reloads the expansion set");
        // Every site that moves it goes through the pair.
        let uses = src.matches("(use_conn)(").count();
        assert!(
            uses >= 3,
            "only {uses} callers of `use_conn` — did a site go back to setting \
             `active_conn` directly?"
        );
    }

    /// **The SSH host-key refusal was thrown away by the one control whose job
    /// is to report it.**
    ///
    /// `open_tunnel`'s failure arm was `Err(_) => { send(false); return; }`, and
    /// the form's only rendering of a failure was a red glyph — so
    /// `ssh::refusal_message`'s several sentences about a key that has *changed*
    /// looked exactly like a wrong password. `ssh::authenticate`'s own doc says
    /// these errors are *"surfaced by the Manage-Connections Test button"*.
    ///
    /// The structural half of the fix is that `TestState::Fail` now takes a
    /// `String`, so `send(false)` no longer type-checks; this pins the mapping.
    #[test]
    fn a_failed_test_always_carries_its_reason() {
        let refusal = "The host key for bastion.example.com has CHANGED since \
                       Schemaic first trusted it.";
        let failed = test_outcome(Err(refusal.to_string()));
        assert_eq!(failed.failure(), Some(refusal));
        assert!(
            failed.landed(),
            "a failure is a result, and flashes like one"
        );

        let ok = test_outcome(Ok(()));
        assert_eq!(
            ok.failure(),
            None,
            "nothing to say about a test that passed"
        );
        assert!(ok.landed());
    }

    /// The states that are not a finished test say nothing and flash nothing —
    /// which is what the "editing a field withdraws the last result" effect
    /// depends on.
    #[test]
    fn an_unfinished_test_reports_neither_way() {
        for st in [TestState::Idle, TestState::Testing] {
            assert!(!st.landed(), "{:?}", st.failure());
            assert_eq!(st.failure(), None);
        }
        // An empty reason is not a sentence: the icon still says it failed, and
        // the line below the footer stays away rather than opening blank.
        assert_eq!(TestState::Fail(String::new()).failure(), None);
        assert!(TestState::Fail(String::new()).landed());
    }

    /// The two refusals do not share wording. They are different facts — one is
    /// about the server, one about the tab — and the modal is the only place
    /// either is said.
    #[test]
    fn the_two_plan_refusals_say_different_things() {
        let a = plan_refusal_text(Refusal::NotConnected);
        let b = plan_refusal_text(Refusal::TabMovedOn);
        assert_ne!(a, b);
        assert!(!a.trim().is_empty() && !b.trim().is_empty());
        // Each names what did *not* happen; a modal that only says "error" is
        // the thing being replaced.
        assert!(a.contains("was not run") && b.contains("was not run"));
    }

    /// **The pill and the session read one mapping.** These are the two halves of
    /// one tab's transaction decision — this crate's answer drives the footer pill
    /// and what Commit and Rollback may do, `Session`'s decides whether the next
    /// statement issues a `BEGIN` — and they used to be the same three-arm match
    /// written out twice, one pinned by `session.rs`'s tests and one unreachable
    /// from them. This test could not be *written* before the fix (the mapping was
    /// private to `schemaic-db`), which is the finding; it fails now the moment
    /// anybody re-inlines one of the two.
    #[test]
    fn the_pill_and_the_session_agree_on_every_engine() {
        for engine in [
            schemaic_db::Engine::Postgres,
            schemaic_db::Engine::MySql,
            schemaic_db::Engine::Sqlite,
        ] {
            let db = schemaic_db::Db::from_parts(
                engine,
                String::new(),
                0,
                String::new(),
                String::new(),
                String::new(),
            );
            assert_eq!(
                tx_engine(&db),
                schemaic_db::session::tx_engine_of(engine),
                "{engine:?}"
            );
        }
    }

    /// **A timeout and the Cancel button arrive as the same `DbError`.** If the
    /// message were the plain "Cancelled" the user's own button produces, they
    /// would be left staring at a query nobody stopped — so it has to name the
    /// cause, and name the setting in the words the dropdown uses so the two
    /// can be matched up.
    #[test]
    fn a_timed_out_statement_says_what_stopped_it_and_where_to_change_it() {
        let msg = timeout_message(900);
        assert!(msg.contains("15 minutes"), "{msg}");
        assert!(msg.contains("Settings"), "{msg}");
        assert_ne!(msg, "Cancelled");
    }

    /// The label is shared with the settings dropdown for exactly this reason:
    /// a message quoting "900 seconds" against a dropdown reading "15 minutes"
    /// leaves the user unable to tell which setting fired.
    #[test]
    fn the_message_quotes_the_same_words_the_setting_shows() {
        for secs in [60u64, 300, 900, 1_800, 3_600] {
            let label = schemaic_core::persist::statement_timeout_label(secs);
            assert!(
                timeout_message(secs).contains(&label.to_lowercase()),
                "{secs}s: {label}"
            );
        }
    }

    /// Off is the default, and an unconfigured app must not pay a spawned task
    /// per statement for a feature nobody turned on. `fired()` staying false is
    /// the observable half of that.
    #[tokio::test]
    async fn a_zero_timeout_arms_no_watchdog() {
        let token = CancellationToken::new();
        let watchdog = RunTimeout::arm(&token, 0);
        tokio::task::yield_now().await;
        assert!(!watchdog.fired());
        assert!(!token.is_cancelled(), "nothing should cancel a run at 0s");
    }

    /// The seam the whole feature is: the clock expires, and **the run's own
    /// cancellation token** — the one the Cancel button fires, the one the
    /// backends already know how to honour — is what gets cancelled. Driven on
    /// a paused clock so the test does not actually wait a minute.
    #[tokio::test(start_paused = true)]
    async fn an_expired_timeout_cancels_the_runs_own_token() {
        let token = CancellationToken::new();
        let watchdog = RunTimeout::arm(&token, 60);
        assert!(!watchdog.fired(), "not yet");
        tokio::time::sleep(std::time::Duration::from_secs(61)).await;
        assert!(token.is_cancelled(), "the run was not cancelled");
        assert!(watchdog.fired(), "the reason was not recorded");
    }

    /// A statement that finishes must not leave an hour-long `sleep` behind,
    /// and it must not cancel a token the app has since reused. `Drop` is what
    /// disarms — dropping a `CancellationToken` does not cancel it, so the
    /// watchdog has to say so itself.
    #[tokio::test(start_paused = true)]
    async fn a_dropped_watchdog_never_fires() {
        let token = CancellationToken::new();
        drop(RunTimeout::arm(&token, 60));
        tokio::time::sleep(std::time::Duration::from_secs(600)).await;
        assert!(
            !token.is_cancelled(),
            "a finished statement's watchdog fired anyway"
        );
    }

    /// Every bump of `activity_gen` must arm the poll loop in the same breath.
    ///
    /// `activity_poll` carries the generation it was armed under and returns the
    /// moment the signal differs — so a bump that nothing re-arms doesn't *pause*
    /// Server Activity's auto-refresh, it **ends** it. That is what the kill
    /// handler did: bump, `(refresh)()`, and the panel froze permanently after
    /// any successful kill while the clock's tooltip still read "every 5s".
    ///
    /// The pairing has no runtime subject — `exec_after` and `main.rs`'s signal
    /// graph are not reachable from a test — so the subject is the source text,
    /// the way `core/tests/doc_coverage.rs` takes a file as its subject: the
    /// signal has no direct writer left, because `rearm_activity` — which cannot
    /// be spelled without the arm — is the only thing that writes it.
    /// **A config load that is not covered by the startup drain owes its own
    /// report.**
    ///
    /// `persist` renames an unreadable file to `.corrupt`, falls back to the
    /// `.bak` or to defaults, and queues a notice — because a released GUI build
    /// discards stderr, so without the modal the user just sees their settings
    /// gone. The app drained that queue once, after the `Ui` literal, under a
    /// comment saying every config file had been loaded by then.
    ///
    /// Three were not: the ER diagram's layout read and its save-side re-read,
    /// both in drag handlers, and the layout prune inside `delete_conn_now`'s
    /// click handler. So a truncated `diagrams.json` was renamed away and
    /// reported to nobody — and on the save side, if the `.bak` was unreadable
    /// too, the very next drag wrote the defaulted empty file over the recovered
    /// nothing.
    ///
    /// The rule is positional and cannot be: "is this load inside `app_view`'s
    /// build?" is not a question a scan can answer. So the gate is the
    /// **count** — every `load_json` outside the build sequence pairs with a
    /// `report_recoveries`, and the two crates hold as many reporters as they
    /// have lazy loads.
    #[test]
    fn every_lazy_config_load_reports_what_it_recovered() {
        let mut loads = 0usize;
        let mut reports = 0usize;
        for (name, code) in [
            ("main.rs", production_main()),
            (
                "erd_view.rs",
                // `CARGO_MANIFEST_DIR` is `<root>/crates/schemaic-app`, so one
                // parent is the crates dir.
                std::fs::read_to_string(
                    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                        .parent()
                        .expect("the crates dir")
                        .join("schemaic-ui")
                        .join("src")
                        .join("erd_view.rs"),
                )
                .expect("erd_view.rs"),
            ),
        ] {
            let body = code.split("#[cfg(test)]").next().unwrap_or(&code);
            let code: String = body
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n");
            // The lazy ones are exactly the `diagrams.json` loads: every other
            // config file is read once, during `app_view`'s build, before the
            // drain.
            loads += code.matches("load_json(\"diagrams.json\")").count()
                + code
                    .matches("load_json::<schemaic_core::erd::DiagramLayoutsFile>")
                    .count();
            reports += code.matches("report_recoveries(").count();
            let _ = name;
        }
        assert!(
            loads >= 3,
            "only {loads} lazy `diagrams.json` loads found — this gate has \
             stopped seeing the sites it is written about"
        );
        assert!(
            reports > loads,
            "{loads} lazy config loads and only {reports} `report_recoveries` \
             calls (one of which is the startup drain) — a load that recovers a \
             `.corrupt` file and says nothing leaves the user's settings gone \
             with no explanation"
        );
    }

    /// This file's production text — every source gate below reads it.
    fn production_main() -> String {
        let src = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join("main.rs"),
        )
        .expect("this file's own source");
        src.split("#[cfg(test)]")
            .next()
            .expect("production code")
            .to_string()
    }

    /// The closure binding a line sits in — `let <name>: Rc<dyn Fn…> = {` at
    /// `app_view`'s own indent. Deep inside a nested closure that is the name a
    /// reader would call the site by, and unlike a line number it does not move
    /// when something above it does.
    fn owning_closure(src: &str, upto: usize) -> (String, usize) {
        let (mut name, mut at) = (String::from("<app_view>"), 0usize);
        let mut off = 0usize;
        for line in src[..upto].split('\n') {
            if let Some(rest) = line.strip_prefix("    let ")
                && let Some(id) = rest.split([':', ' ', '=']).next()
                && !id.is_empty()
                && id.chars().all(|c| c.is_alphanumeric() || c == '_')
                && rest[id.len()..].starts_with(':')
            {
                name = id.to_string();
                at = off;
            }
            off += line.len() + 1;
        }
        (name, at)
    }

    /// That closure's whole body — to the `};` back at `app_view`'s own indent.
    ///
    /// A proximity window answers about the wrong thing here: the reset that
    /// pairs with a release legitimately sits far below it, in the branch that
    /// is *keeping* the tab, while the branch that removes the tab outright
    /// needs no reset at all.
    fn closure_body(src: &str, at: usize) -> &str {
        let end = src[at..].find("\n    };").map_or(src.len(), |i| at + i);
        &src[at..end]
    }

    /// **Releasing a tab's pinned session without dropping it out of Manual
    /// leaves a tab that refuses every statement.**
    ///
    /// `session_for` matches `TxMode::Manual` and then looks the tab up in
    /// `sessions`; with the entry gone it returns `None` and the run fails before
    /// dispatch with *"the transaction connection isn't ready — switch to
    /// Auto-commit and back"*. Nothing re-opens it — `open_session`'s callers are
    /// `set_tx_mode`, `set_active_db` and `repair_killed_session`, and a closed
    /// tab is on none of those paths.
    ///
    /// `close_tab_now`'s keep-≥1 branch is an explicit blank-slate rebuild — its
    /// own comment says so, and it resets nine pieces of tab state. `tx_mode` and
    /// `tx` were the two it did not touch, and they are the two the release
    /// invalidates. Both siblings that drop a session while keeping the tab
    /// (`save_conn`'s repoint, `delete_conn_now`) set `TxMode::Auto` in the same
    /// breath and say why.
    ///
    /// The behavioural half is GUI-only — `app_view`'s closures are not reachable
    /// from a test — so the subject is the source text, as
    /// `every_activity_generation_bump_arms_the_poll_loop` below takes it.
    #[test]
    fn releasing_a_session_drops_its_tab_out_of_manual() {
        /// `open_session` releases in order to *re-pin*: the tab is staying in
        /// Manual on purpose, and its own error arm drops to Auto if the
        /// re-open cannot happen.
        const EXEMPT: &[(&str, &str)] = &[(
            "open_session",
            "drops only to re-open; the Err arm is what falls back to Auto",
        )];
        let src = production_main();
        let mut offenders: Vec<String> = Vec::new();
        let mut from = 0;
        while let Some(at) = src[from..].find("(drop_session)(") {
            let at = from + at;
            from = at + "(drop_session)(".len();
            let (who, opens_at) = owning_closure(&src, at);
            if EXEMPT.iter().any(|(name, _)| *name == who) {
                continue;
            }
            if !closure_body(&src, opens_at).contains("TxMode::Auto") {
                offenders.push(who);
            }
        }
        assert!(
            offenders.is_empty(),
            "these release a tab's pinned session and leave it in Manual, so the \
             tab refuses every statement until the user toggles the mode twice: \
             {offenders:?}"
        );
    }

    /// **Picking the database you are already on is not a change**, and
    /// `set_active_db` treated it as one: it cancelled the tab's in-flight
    /// query, raised the Commit/Rollback prompt on a Manual tab, and dropped and
    /// re-opened its pinned connection. The menu offers it — the current row is
    /// *accented*, not disabled — so the click is one the UI invites.
    ///
    /// `set_tx_mode`, the sibling action that also calls `guard_tx` and
    /// `open_session`, opens with exactly this refusal. The decision now lives in
    /// `tabsel::rebind_needed` where it has tests; this is the half those tests
    /// cannot see, which is whether the caller asks.
    #[test]
    fn set_active_db_refuses_a_rebind_that_is_not_one() {
        let src = production_main();
        let at = src
            .find("let set_active_db:")
            .expect("set_active_db is gone — this gate is stale");
        let body = &src[at..];
        let end = body.find("\n    };").expect("the end of set_active_db");
        assert!(
            body[..end].contains("rebind_needed("),
            "set_active_db does not ask whether the pick is a change, so \
             re-picking the accented row cancels the running query, prompts to \
             settle a transaction that is not moving, and re-pins the session"
        );
    }

    /// **Emptying the schema tree has to let go of the generation behind it**,
    /// and two of the three clears did not.
    ///
    /// `nodes_scope` owns every node's `RwSignal<SchemaState>`, each holding an
    /// `Arc<DbSchema>` — every column, index, key, view, check and trigger of
    /// every database on the connection. Clear `db_nodes` and leave the scope
    /// and `nodes_conn` behind, and the *next* load of that connection takes the
    /// reuse path against an empty node list: `kept_scope` is `Some`, so the
    /// deferred `dispose()` is skipped, and the whole set is rebuilt inside a
    /// scope that still owns the previous one. Unreachable, and never freed.
    ///
    /// The failed-connect arm already carried that diagnosis in full and fixed
    /// it **for itself**. The same state is reached by a connection switch that
    /// the user reverses before the first load lands — one orphaned generation
    /// per A→B→A round trip, which is seconds wide over a tunnel — and by
    /// deleting the last connection.
    ///
    /// So the signal has no direct writer left: `clear_schema_tree`, which
    /// cannot be spelled without all three steps, is the only thing that empties
    /// it. The same shape as `every_activity_generation_bump_arms_the_poll_loop`
    /// below, and for the same reason — the pairing has no runtime subject.
    #[test]
    fn emptying_the_schema_tree_lets_go_of_its_scope() {
        let src = production_main();
        let (name, at) = ("let clear_schema_tree", src.find("let clear_schema_tree"));
        let at = at.unwrap_or_else(|| panic!("{name} is gone — this gate is stale"));
        let body = closure_body(&src, src[..at].rfind('\n').map_or(0, |i| i + 1));
        for step in [
            "db_nodes.set(Vec::new())",
            "*nodes_conn.borrow_mut() = None",
            "nodes_scope.borrow_mut().take()",
        ] {
            assert!(
                body.contains(step),
                "`clear_schema_tree` no longer does `{step}`, so a caller of it \
                 leaves a generation of schema signals orphaned"
            );
        }
        let writes: Vec<&str> = src
            .lines()
            .map(str::trim)
            .filter(|l| l.starts_with("db_nodes.set(Vec::new())"))
            .collect();
        assert_eq!(
            writes.len(),
            1,
            "db_nodes is emptied outside `clear_schema_tree`, so that caller \
             orphans the node scope and `nodes_conn` goes on naming a tree that \
             is not there"
        );
    }

    #[test]
    fn every_activity_generation_bump_arms_the_poll_loop() {
        let writes: Vec<&str> = include_str!("main.rs")
            .lines()
            .map(str::trim)
            .filter(|l| l.starts_with("activity_gen.set(") || l.starts_with("activity_gen.update("))
            .collect();
        assert_eq!(
            writes,
            [] as [&str; 0],
            "activity_gen is bumped outside `rearm_activity`, so this caller can \
             strand the poll loop without arming a new one"
        );
    }

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

    #[test]
    fn a_health_check_of_the_connection_still_active_lands() {
        use super::check_landing;
        assert!(check_landing((7, 3), (7, 3)));
    }

    #[test]
    fn a_health_check_of_a_connection_the_user_left_is_dropped() {
        use super::check_landing;
        // The reported bug: a dead connection's ping is still in flight when the
        // user switches to a live one. It lands seconds later and repaints the
        // header's "Disconnected · Retry" over a connection that is answering.
        assert!(
            !check_landing((7, 3), (8, 4)),
            "another connection is active now"
        );
    }

    #[test]
    fn an_older_check_of_the_same_connection_is_dropped_too() {
        use super::check_landing;
        // Retry pressed twice against a host that came back in between: the
        // second lands `Connected` first, then the first lands its five-second-
        // old failure on top, and the banner is back until a poll that has just
        // been told to back off.
        assert!(!check_landing((7, 3), (7, 4)));
    }

    /// A superseded check must still **answer** the action that asked for it,
    /// even though it may no longer write the status.
    ///
    /// The reported failure mode: a blocked action pings, the ping takes five
    /// seconds against a dead host, the health poll ticks inside that window and
    /// bumps the generation — and the user's action vanished with no error, which
    /// is exactly what `with_conn` exists to prevent.
    ///
    /// **Asserted on `check_outcome`, not on the two predicates.** The earlier
    /// spelling of this test called `check_landing` and `check_continues`
    /// separately and stayed green while a `return` sat between them at the one
    /// call site, reproducing the exact sequence this docstring describes. The
    /// subject has to be the composition.
    #[test]
    fn a_superseded_check_still_answers_the_action_that_asked() {
        use super::check_outcome;
        // The failing ping the user is waiting on, landing after a poll has
        // bumped the generation. It may not repaint the header — but it is the
        // only reply the Run button is ever going to get.
        let superseded_failure = check_outcome((7, 3), (7, 4), false);
        assert!(
            !superseded_failure.write_status,
            "no longer writes the status"
        );
        assert_eq!(
            superseded_failure.answer,
            Some(false),
            "but it still answers the action about connection 7"
        );
        // The same interleaving with a server that answered.
        assert_eq!(check_outcome((7, 3), (7, 4), true).answer, Some(true));
        // The ordinary case: nothing superseded it, so it does both.
        let ordinary = check_outcome((7, 3), (7, 3), false);
        assert!(ordinary.write_status);
        assert_eq!(ordinary.answer, Some(false));
    }

    #[test]
    fn a_check_of_a_connection_the_user_left_answers_nothing() {
        use super::check_outcome;
        // The line the looser rule still holds: running an action gated on a
        // server the user has walked away from, or reporting the old connection
        // unreachable in a modal sitting over the new one.
        for ok in [true, false] {
            for now in [(8, 4), (8, 3)] {
                let outcome = check_outcome((7, 3), now, ok);
                assert_eq!(outcome.answer, None, "{now:?} is a different connection");
                assert!(!outcome.write_status);
            }
        }
    }

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

    /// The level `load_landing` doesn't reach. `try_update` guards a *disposed*
    /// scope — a connection switch — and says nothing about a **superseded**
    /// fetch of the same node, which is the interleaving that leaves the tree,
    /// the completion index and the schema editors holding a pre-`ALTER` model
    /// indefinitely.
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

    /// **And the nodes the plan leaves behind own the memory.** A database
    /// dropped from another client between two reloads vanishes from the tree
    /// while its `RwSignal<SchemaState>` — holding an `Arc<DbSchema>`, the whole
    /// model of that database — stays installed in the node scope the reload
    /// deliberately keeps alive. `plan_nodes` never names it, because it maps
    /// the *server's* list; the departing set is the complement, and it is what
    /// the caller disposes.
    #[test]
    fn a_database_that_is_gone_leaves_its_node_behind_to_be_disposed() {
        use super::{departed_nodes, plan_nodes};
        let existing = nodes(&[(1, "world"), (2, "sakila")]);
        let plans = plan_nodes(&existing, &dbs(&["world", "chinook"]), true);
        assert_eq!(departed_nodes(&existing, &plans), vec![2]);
    }

    /// A reload where nothing was dropped disposes nothing — the surviving
    /// nodes keep their schema up while the re-introspection runs, which is the
    /// whole reason the scope is reused.
    #[test]
    fn a_reload_that_drops_nothing_disposes_nothing() {
        use super::{departed_nodes, plan_nodes};
        let existing = nodes(&[(1, "world"), (2, "sakila")]);
        let plans = plan_nodes(&existing, &dbs(&["sakila", "world"]), true);
        assert!(departed_nodes(&existing, &plans).is_empty());
        // Nor does a reload that only *gains* a database.
        let plans = plan_nodes(&existing, &dbs(&["world", "sakila", "chinook"]), true);
        assert!(departed_nodes(&existing, &plans).is_empty());
    }

    /// A database dropped and re-created between two reloads takes a fresh id,
    /// so the **old** node departs even though the name came back — and its
    /// signals are a stale model of a different database.
    #[test]
    fn a_recreated_database_leaves_its_old_node_behind() {
        use super::{departed_nodes, plan_nodes};
        let existing = nodes(&[(1, "world"), (2, "scratch")]);
        // `scratch` was dropped and made again: not found by name in one pass,
        // so it is a `Create` at a fresh id and node 2 is departing.
        let plans = plan_nodes(&existing, &dbs(&["world"]), true);
        assert_eq!(departed_nodes(&existing, &plans), vec![2]);
    }

    /// On a **switch** nothing is kept, so every node departs. The caller does
    /// not ask on that path — the parent scope is replaced whole and disposing
    /// it takes the children with it — but the answer has to be the honest one
    /// rather than depend on who asks.
    #[test]
    fn a_switch_leaves_every_node_behind() {
        use super::{departed_nodes, plan_nodes};
        let existing = nodes(&[(1, "world"), (2, "sakila")]);
        let plans = plan_nodes(&existing, &dbs(&["world", "sakila"]), false);
        assert_eq!(departed_nodes(&existing, &plans), vec![1, 2]);
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
            database: String::new(),
            ssh: Default::default(),
            tls: Default::default(),
            color: None,
            prominent_color: false,
            read_only: false,
            environment: Default::default(),
            ai_data: None,
        }
    }

    #[test]
    fn native_shell_puts_password_in_env_not_argv() {
        let cfg = mysql_shell_config(CliLauncher::Native("mysql"), &conn(), Some("shop")).unwrap();
        assert_eq!(cfg.program, "mysql");
        assert_eq!(
            cfg.args,
            vec![
                "-h",
                "10.0.0.5",
                "-P",
                "3307",
                "-u",
                "root",
                "--ssl-mode=DISABLED",
                "--",
                "shop"
            ]
        );
        // Password rides MYSQL_PWD, never the command line.
        assert_eq!(
            cfg.env,
            vec![("MYSQL_PWD".to_string(), "s3cr3t".to_string())]
        );
        assert!(!cfg.args.iter().any(|a| a.contains("s3cr3t")));
    }

    /// A **server-supplied** name reaching the client's argv as a bare
    /// positional is parsed as an *option*: a database literally named
    /// `--pager=touch /tmp/PWN` runs that command on the user's first query
    /// (measured against MariaDB 10.11.14). The name is not the user's to
    /// vouch for — on a shared server anyone who may `CREATE DATABASE` writes
    /// it — so the fix is positional, not a filter: `--` before the name.
    #[test]
    fn the_database_name_can_never_be_read_as_an_option() {
        for db in ["shop", "--pager=touch /tmp/PWN", "-e", "--help"] {
            let cfg = mysql_shell_config(CliLauncher::Native("mysql"), &conn(), Some(db)).unwrap();
            let at = cfg.args.iter().position(|a| a == db).unwrap();
            assert_eq!(
                cfg.args[at - 1],
                "--",
                "{db} is not behind an option terminator: {:?}",
                cfg.args
            );
            assert_eq!(at, cfg.args.len() - 1, "the name must be last");
        }
    }

    /// The composition, not the flag map: a connection the user configured for
    /// `verify-full` must reach the client *verifying*. Omitting the flag is
    /// not neutral — the client's own default is `PREFERRED`, which accepts a
    /// plaintext socket and checks no certificate, while the app's header goes
    /// on reporting TLS because the app's own socket really is encrypted.
    #[test]
    fn a_verifying_connection_reaches_the_mysql_client_verifying() {
        let c = Connection {
            tls: schemaic_core::connection::Tls {
                mode: schemaic_core::connection::SslMode::VerifyFull,
                ca_path: "/etc/ca.crt".into(),
                ..Default::default()
            },
            ..conn()
        };
        let cfg = mysql_shell_config(CliLauncher::Native("mysql"), &c, Some("shop")).unwrap();
        assert!(
            cfg.args.contains(&"--ssl-mode=VERIFY_IDENTITY".to_string()),
            "{:?}",
            cfg.args
        );
        assert!(cfg.args.contains(&"--ssl-ca=/etc/ca.crt".to_string()));
        // And before the terminator, or the client reads them as the database.
        let term = cfg.args.iter().position(|a| a == "--").unwrap();
        let flag = cfg
            .args
            .iter()
            .position(|a| a == "--ssl-mode=VERIFY_IDENTITY")
            .unwrap();
        assert!(flag < term);
    }

    #[test]
    fn a_verifying_connection_reaches_psql_verifying() {
        let c = Connection {
            db_type: "PostgreSQL".into(),
            tls: schemaic_core::connection::Tls {
                mode: schemaic_core::connection::SslMode::VerifyFull,
                ca_path: "/etc/ca.crt".into(),
                ..Default::default()
            },
            ..conn()
        };
        let cfg = psql_shell_config(CliLauncher::Native("psql"), &c, "chinook").unwrap();
        assert!(
            cfg.env
                .contains(&("PGSSLMODE".to_string(), "verify-full".to_string())),
            "{:?}",
            cfg.env
        );
        assert!(
            cfg.env
                .contains(&("PGSSLROOTCERT".to_string(), "/etc/ca.crt".to_string()))
        );
    }

    /// A plaintext connection must say so too: the same omission in the other
    /// direction leaves the client negotiating TLS the user turned off, which
    /// is at best a confusing failure against a server that has none.
    #[test]
    fn a_plaintext_connection_says_disabled_rather_than_nothing() {
        let cfg = mysql_shell_config(CliLauncher::Native("mysql"), &conn(), None).unwrap();
        assert!(cfg.args.contains(&"--ssl-mode=DISABLED".to_string()));
        let cfg = psql_shell_config(CliLauncher::Native("psql"), &conn(), "chinook").unwrap();
        assert!(
            cfg.env
                .contains(&("PGSSLMODE".to_string(), "disable".to_string()))
        );
    }

    /// psql has no `--` to hide behind: libpq re-reads a `-d` value containing
    /// `=` as a conninfo string and follows its `host=` elsewhere, with
    /// `PGPASSWORD` in hand. The refusal lives in the builder, not in
    /// `open_db_cli`, so no future launcher can reach the argv around it.
    #[test]
    fn a_conninfo_shaped_name_never_reaches_psqls_d_flag() {
        for db in ["dbname=postgres host=192.0.2.1", "postgresql://evil/x"] {
            assert!(
                psql_shell_config(CliLauncher::Native("psql"), &conn(), db).is_err(),
                "{db} was built into an argv"
            );
        }
    }

    /// A cert path only the Windows side can open, handed to a Linux client
    /// inside WSL, is a mode that cannot be expressed — so refuse rather than
    /// launch a session that will fail obscurely or, worse, fall back.
    #[test]
    fn a_windows_cert_path_refuses_a_wsl_client() {
        let c = Connection {
            tls: schemaic_core::connection::Tls {
                mode: schemaic_core::connection::SslMode::VerifyFull,
                ca_path: r"C:\certs\ca.crt".into(),
                ..Default::default()
            },
            ..conn()
        };
        assert!(mysql_shell_config(CliLauncher::Wsl("mysql"), &c, None).is_err());
        assert!(psql_shell_config(CliLauncher::Wsl("psql"), &c, "x").is_err());
        // A path the WSL side can open is fine.
        let c = Connection {
            tls: schemaic_core::connection::Tls {
                ca_path: "/etc/ca.crt".into(),
                ..c.tls
            },
            ..c
        };
        assert!(mysql_shell_config(CliLauncher::Wsl("mysql"), &c, None).is_ok());
    }

    #[test]
    fn native_shell_omits_db_when_none() {
        let cfg = mysql_shell_config(CliLauncher::Native("mariadb"), &conn(), None).unwrap();
        assert_eq!(
            cfg.args,
            vec![
                "-h",
                "10.0.0.5",
                "-P",
                "3307",
                "-u",
                "root",
                "--ssl-mode=DISABLED"
            ]
        );
        assert!(
            !cfg.args.contains(&"--".to_string()),
            "no positional, so no terminator to add"
        );
    }

    #[test]
    fn wsl_shell_prepends_client_and_forwards_password_via_wslenv() {
        let cfg = mysql_shell_config(CliLauncher::Wsl("mysql"), &conn(), Some("shop")).unwrap();
        assert_eq!(cfg.program, "wsl.exe");
        assert_eq!(
            cfg.args,
            vec![
                "-e",
                "mysql",
                "-h",
                "10.0.0.5",
                "-P",
                "3307",
                "-u",
                "root",
                "--ssl-mode=DISABLED",
                "--",
                "shop"
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
        let cfg = psql_shell_config(CliLauncher::Native("psql"), &conn(), "chinook").unwrap();
        assert_eq!(cfg.program, "psql");
        assert_eq!(
            cfg.args,
            vec![
                "-h", "10.0.0.5", "-p", "3307", "-U", "root", "-d", "chinook"
            ]
        );
        assert_eq!(
            cfg.env,
            vec![
                ("PGPASSWORD".to_string(), "s3cr3t".to_string()),
                ("PGSSLMODE".to_string(), "disable".to_string()),
            ]
        );
        assert!(!cfg.args.iter().any(|a| a.contains("s3cr3t")));
    }

    #[test]
    fn psql_wsl_shell_prepends_client_and_forwards_password_via_wslenv() {
        let cfg = psql_shell_config(CliLauncher::Wsl("psql"), &conn(), "world").unwrap();
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
                ("WSLENV".to_string(), "PGPASSWORD/u:PGSSLMODE/u".to_string()),
                ("PGPASSWORD".to_string(), "s3cr3t".to_string()),
                ("PGSSLMODE".to_string(), "disable".to_string()),
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

    const MY: schemaic_core::intel::SqlDialect = schemaic_core::intel::SqlDialect::MySql;

    #[test]
    fn inline_outcome_success_returns_stripped_sql() {
        let out = inline_outcome(Ok("```sql\nSELECT 1\n```".into()), MY);
        assert!(matches!(out, InlineAiState::Ready(sql) if sql == "SELECT 1"));
    }

    #[test]
    fn inline_outcome_blank_success_is_no_sql_returned() {
        let out = inline_outcome(Ok("   \n".into()), MY);
        assert!(matches!(out, InlineAiState::Failed(m) if m == "No SQL returned"));
    }

    #[test]
    fn inline_outcome_failure_surfaces_first_stderr_line() {
        let out = inline_outcome(Err("boom: bad model\nsecond line".into()), MY);
        assert!(matches!(out, InlineAiState::Failed(m) if m == "boom: bad model"));
        // Nothing to say → a generic fallback message, never a blank bar.
        let out = inline_outcome(Err(String::new()), MY);
        assert!(matches!(out, InlineAiState::Failed(m) if m == "generation failed"));
    }

    /// The tool's own chatter never reaches the editor: the composition of
    /// `extract_sql` (fences) with `intel::sql_reply` (the parse gate) is what
    /// the caller relies on, so it is pinned here rather than only in `intel`.
    #[test]
    fn inline_outcome_drops_a_tool_diagnostic_riding_on_the_sql() {
        let out = inline_outcome(
            Ok(
                "```sql\nSELECT * FROM t;\nClient.listTools() called but server does not advertise\n```"
                    .into(),
            ),
            MY,
        );
        assert!(matches!(out, InlineAiState::Ready(sql) if sql == "SELECT * FROM t;"));
    }

    /// And a reply with no SQL in it at all is a failure, not an empty edit.
    #[test]
    fn inline_outcome_refuses_a_reply_that_is_only_prose() {
        let out = inline_outcome(Ok("I'm sorry, I can't do that.".into()), MY);
        assert!(matches!(out, InlineAiState::Failed(m) if m == "The model did not return SQL"));
    }
}
