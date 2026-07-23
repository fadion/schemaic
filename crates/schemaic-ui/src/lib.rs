//! Schemaic UI (Floem).
//!
//! M2: the three-pane shell plus a **virtualized** Results grid — a frozen
//! header over a `scroll(virtual_stack(...))` that renders only the visible
//! rows, so millions of rows stay smooth. Rows are keyed by index and the view
//! fn indexes into a shared `Arc<ResultSet>` (no per-row cloning). Layout
//! follows FEATURES §1.

mod ai_panel;
mod completion;
mod connection_form;
mod consts;
mod diff_view;
mod editor_pane;
pub mod fonts;
mod grid;
mod history_panel;
pub mod icons;
mod markdown;
mod overlays;
mod plan_view;
mod schema_tree;
mod settings;
pub mod sql_highlight;
mod tabs;
pub mod theme;
pub mod themes;
mod widgets;

use ai_panel::ai_panel;
use connection_form::manage_modal;
use consts::*;
use editor_pane::{QueryPaneParams, editor_placeholder, query_pane};
use grid::{GridCtx, grid_error_bar, grid_find_bar, loaded_view, results_view, running_view};
use history_panel::history_panel;
use overlays::{
    active_db_menu_overlay, conn_menu_overlay, context_menu_overlay, db_visibility_overlay,
    error_modal_overlay, find_overlay, popup_menu_overlay, schema_settings_overlay,
};
use plan_view::plan_overlay;
use schema_tree::schema_panel;
use settings::{ai_settings_overlay, help_overlay, term_settings_overlay, theme_settings_overlay};
use tabs::tab_bar;
use widgets::*;

use std::collections::HashSet;
use std::rc::Rc;

use floem::AnyView;
use floem::event::{Event, EventListener, EventPropagation};
use floem::keyboard::{Key, NamedKey};
use floem::kurbo::Point;
use floem::prelude::*;
use floem::reactive::{Memo, Scope, create_effect, create_memo, untrack};
use floem::style::{CursorStyle, Transition, Width};
use floem::text::FamilyOwned;
use floem::unit::Px;
use floem::views::editor::command::CommandExecuted;
use floem::views::editor::core::editor::EditType;
use floem::views::editor::core::selection::Selection;
use floem::views::editor::keypress::default_key_handler;
use floem::views::editor::keypress::key::KeyInput;
use floem::views::editor::text::{SimpleStyling, WrapMethod, default_dark_color};
use floem::views::scroll::{Handle, Rounded, Thickness, Track};
use floem::views::{Decorators, Delay, TextInputClass, TooltipClass, TooltipContainerClass};
use schemaic_core::connection::{ConnStatus, Connection, SshAuth};
use schemaic_core::db_color::DbColorRule;
use schemaic_core::format::ColumnFormatRule;
use schemaic_core::history::HistoryEntry;
use schemaic_core::model::{CommitDone, GridWrite, QueryState, RefetchRequest};
use schemaic_core::resource::ResourceSample;

/// The grid-commit completion callback, invoked on the UI thread with the outcome.
pub type CommitDoneFn = Rc<dyn Fn(CommitDone)>;
/// Commit staged grid changes transactionally: apply the `GridWrite`, optionally
/// re-fetch (`Some` ⇒ splice the edited rows in place), then report via
/// [`CommitDoneFn`]. Aliased to keep the field/signal types below readable.
pub type CommitFn = Rc<dyn Fn(GridWrite, Option<RefetchRequest>, CommitDoneFn)>;
use schemaic_core::schema::SchemaState;
use schemaic_core::transcript::{Seg, TurnStats};
use schemaic_term::Screen;

// Layout & dimension constants live in `consts.rs` (glob-imported above).

/// One query tab: its own editor buffer, result, and target connection.
/// Signals are created in the app's root scope so they persist for the tab's
/// lifetime.
///
/// A tab's identity is `(conn_id, database)` — the saved connection it runs
/// against plus which database is `USE`d — not a credential URL (review §3.1).
/// The app resolves `conn_id` to a `schemaic_db::Db` at run time, so a tab keeps
/// running against the connection it was opened under even after the active
/// connection is switched (review H13).
#[derive(Clone, Copy)]
pub struct Tab {
    /// This tab's own child `Scope`: every signal below is created in it, so
    /// closing the tab can `dispose()` it and reclaim them (review C14 — else a
    /// closed tab's `results` signal, up to `QUERY_ROW_CAP` rows, leaks until exit).
    pub cx: Scope,
    pub id: usize,
    /// Display number, e.g. "Query 3".
    pub label: usize,
    pub query: RwSignal<String>,
    pub results: RwSignal<QueryState>,
    /// Multi-statement run (Run Everything) results — one panel per statement,
    /// each with its own state. Empty for single runs (Run / Run Current /
    /// Ctrl+Enter), which use `results` and the legacy single-grid view.
    pub result_tabs: RwSignal<Vec<ResultPanel>>,
    /// Which `result_tabs` entry is shown.
    pub active_result: RwSignal<usize>,
    /// The saved connection id this tab's query runs against.
    pub conn_id: RwSignal<u64>,
    /// The database `USE`d for this tab's queries (`None` before the connection's
    /// database list has loaded — queries then run at the server level).
    pub database: RwSignal<Option<String>>,
    /// `(database, table)` this tab was opened from, if any — used to highlight
    /// the source table in the schema sidebar.
    pub source: RwSignal<Option<(String, String)>>,
    /// User-assigned tab name (double-click to rename). `None` = the default
    /// "Query N" label. Persisted with the tab and shown in query history.
    pub name: RwSignal<Option<String>>,
    /// Pinned tabs sort to the left of the strip (in pin order), drop their close
    /// ×, and can't be closed (×/middle-click/Ctrl+W all no-op) until unpinned.
    pub pinned: RwSignal<bool>,
    /// True while the tab title is being edited inline (renders a text field in
    /// place of the label).
    pub editing: RwSignal<bool>,
    /// Backing buffer for the inline rename field (committed to `name` on Enter /
    /// blur).
    pub edit_buf: RwSignal<String>,
    /// Caret byte offset in `query`, mirrored out of the editor by an effect in
    /// `query_pane` so the status bar can show Ln/Col for the active tab.
    pub cursor_offset: RwSignal<usize>,
    /// Opens this tab's Go-to-line popup. Set by Ctrl+G in the editor or by
    /// clicking the Ln/Col segment in the status bar; the editor pane owns the view.
    pub goto_open: RwSignal<bool>,
}

impl Tab {
    /// `parent` is the app root scope; the tab creates its own child scope under
    /// it so `dispose()` on close frees exactly this tab's signals (C14).
    pub fn new(
        parent: Scope,
        id: usize,
        initial: &str,
        conn_id: u64,
        database: Option<String>,
    ) -> Tab {
        let cx = parent.create_child();
        Tab {
            cx,
            id,
            label: id,
            query: cx.create_rw_signal(initial.to_string()),
            results: cx.create_rw_signal(QueryState::Idle),
            result_tabs: cx.create_rw_signal(Vec::new()),
            active_result: cx.create_rw_signal(0),
            conn_id: cx.create_rw_signal(conn_id),
            database: cx.create_rw_signal(database),
            source: cx.create_rw_signal(None),
            name: cx.create_rw_signal(None),
            pinned: cx.create_rw_signal(false),
            editing: cx.create_rw_signal(false),
            edit_buf: cx.create_rw_signal(String::new()),
            cursor_offset: cx.create_rw_signal(0),
            goto_open: cx.create_rw_signal(false),
        }
    }

    /// The tab's display title: its user-assigned name, or the default "Query N".
    /// Reads the `name` signal reactively, so callers in a reactive scope re-run
    /// on rename.
    pub fn title(&self) -> String {
        self.name
            .get()
            .unwrap_or_else(|| format!("Query {}", self.label))
    }
}

/// One statement's result within a multi-statement (Run Everything) run. The
/// label names its tab; `state` is that statement's own lifecycle.
#[derive(Clone)]
pub struct ResultPanel {
    pub label: String,
    pub state: QueryState,
}

/// Who authored a chat message in the AI panel.
#[derive(Clone, Copy, PartialEq)]
pub enum Role {
    User,
    Assistant,
    Error,
}

/// One message in the AI panel conversation.
#[derive(Clone)]
pub struct ChatMessage {
    pub role: Role,
    /// The user's text (user messages only).
    pub text: String,
    /// The assistant turn's rendered segments (assistant/error messages).
    pub segs: Vec<Seg>,
    /// Cost/usage footer, once the turn completes.
    pub stats: Option<TurnStats>,
    /// True while awaiting the assistant's reply (renders as "Thinking…").
    pub pending: bool,
}

impl ChatMessage {
    pub fn user(text: String) -> ChatMessage {
        ChatMessage {
            role: Role::User,
            text,
            segs: Vec::new(),
            stats: None,
            pending: false,
        }
    }
    /// Placeholder assistant message shown while the CLI runs.
    pub fn pending() -> ChatMessage {
        ChatMessage {
            role: Role::Assistant,
            text: String::new(),
            segs: Vec::new(),
            stats: None,
            pending: true,
        }
    }
}

/// One connection shown in the schema sidebar: a named database plus its
/// lazily-introspected schema (updated through the `schema` signal when the
/// background loader finishes).
#[derive(Clone)]
pub struct ConnNode {
    pub id: usize,
    pub name: String,
    pub database: String,
    pub schema: RwSignal<SchemaState>,
}

impl ConnNode {
    pub fn new(cx: Scope, id: usize, name: &str, database: &str) -> ConnNode {
        ConnNode {
            id,
            name: name.to_string(),
            database: database.to_string(),
            schema: cx.create_rw_signal(SchemaState::Loading),
        }
    }
}

/// Text-field signals backing the "Manage Connections" form. `id == None`
/// means a new (not-yet-saved) connection. Ports are edited as text and parsed
/// on save.
#[derive(Clone, Copy)]
pub struct DraftSignals {
    pub id: RwSignal<Option<u64>>,
    pub name: RwSignal<String>,
    pub db_type: RwSignal<String>,
    pub host: RwSignal<String>,
    pub port: RwSignal<String>,
    pub user: RwSignal<String>,
    pub password: RwSignal<String>,
    pub ssh_enabled: RwSignal<bool>,
    pub ssh_host: RwSignal<String>,
    pub ssh_port: RwSignal<String>,
    pub ssh_user: RwSignal<String>,
    pub ssh_password: RwSignal<String>,
    /// SSH auth method + the key-pair fields (used when `ssh_auth == KeyPair`).
    pub ssh_auth: RwSignal<SshAuth>,
    pub ssh_key_path: RwSignal<String>,
    pub ssh_key_passphrase: RwSignal<String>,
    /// Chosen identity colour (a `#rrggbb` hex), or `None` for no colour.
    pub color: RwSignal<Option<String>>,
    /// Draw the identity colour as a prominent editor frame (off by default).
    pub prominent_color: RwSignal<bool>,
    /// Read-only guard-rail (off by default): disables cell edits + blocks writes.
    pub read_only: RwSignal<bool>,
}

impl DraftSignals {
    pub fn new(cx: Scope) -> DraftSignals {
        DraftSignals {
            id: cx.create_rw_signal(None),
            name: cx.create_rw_signal(String::new()),
            db_type: cx.create_rw_signal("MySQL".to_string()),
            host: cx.create_rw_signal("127.0.0.1".to_string()),
            port: cx.create_rw_signal("3306".to_string()),
            user: cx.create_rw_signal(String::new()),
            password: cx.create_rw_signal(String::new()),
            ssh_enabled: cx.create_rw_signal(false),
            ssh_host: cx.create_rw_signal(String::new()),
            ssh_port: cx.create_rw_signal("22".to_string()),
            ssh_user: cx.create_rw_signal(String::new()),
            ssh_password: cx.create_rw_signal(String::new()),
            ssh_auth: cx.create_rw_signal(SshAuth::Password),
            ssh_key_path: cx.create_rw_signal(String::new()),
            ssh_key_passphrase: cx.create_rw_signal(String::new()),
            color: cx.create_rw_signal(None),
            prominent_color: cx.create_rw_signal(false),
            read_only: cx.create_rw_signal(false),
        }
    }

    /// Populate the form from an existing connection.
    pub fn load(&self, c: &Connection) {
        self.id.set(Some(c.id));
        self.name.set(c.name.clone());
        self.db_type.set(c.db_type.clone());
        self.host.set(c.host.clone());
        self.port.set(c.port.to_string());
        self.user.set(c.user.clone());
        self.password.set(c.password.clone());
        self.ssh_enabled.set(c.ssh.enabled);
        self.ssh_host.set(c.ssh.host.clone());
        self.ssh_port.set(c.ssh.port.to_string());
        self.ssh_user.set(c.ssh.user.clone());
        self.ssh_password.set(c.ssh.password.clone());
        self.ssh_auth.set(c.ssh.auth);
        self.ssh_key_path.set(c.ssh.key_path.clone());
        self.ssh_key_passphrase.set(c.ssh.key_passphrase.clone());
        self.color.set(c.color.clone());
        self.prominent_color.set(c.prominent_color);
        self.read_only.set(c.read_only);
    }

    /// Reset the form for a brand-new connection.
    pub fn blank(&self) {
        self.id.set(None);
        self.name.set("New connection".to_string());
        self.db_type.set("MySQL".to_string());
        self.host.set("127.0.0.1".to_string());
        self.port.set("3306".to_string());
        self.user.set(String::new());
        self.password.set(String::new());
        self.ssh_enabled.set(false);
        self.ssh_host.set(String::new());
        self.ssh_port.set("22".to_string());
        self.ssh_user.set(String::new());
        self.ssh_password.set(String::new());
        self.ssh_auth.set(SshAuth::Password);
        self.ssh_key_path.set(String::new());
        self.ssh_key_passphrase.set(String::new());
        self.color.set(None);
        self.prominent_color.set(false);
        self.read_only.set(false);
    }

    /// Build a `Connection` from the current form values (with the given id).
    pub fn to_connection(&self, id: u64) -> Connection {
        Connection {
            id,
            name: self.name.get_untracked(),
            db_type: self.db_type.get_untracked(),
            host: self.host.get_untracked(),
            port: self.port.get_untracked().trim().parse().unwrap_or(3306),
            user: self.user.get_untracked(),
            password: self.password.get_untracked(),
            ssh: schemaic_core::connection::SshTunnel {
                enabled: self.ssh_enabled.get_untracked(),
                host: self.ssh_host.get_untracked(),
                port: self.ssh_port.get_untracked().trim().parse().unwrap_or(22),
                user: self.ssh_user.get_untracked(),
                password: self.ssh_password.get_untracked(),
                auth: self.ssh_auth.get_untracked(),
                key_path: self.ssh_key_path.get_untracked(),
                key_passphrase: self.ssh_key_passphrase.get_untracked(),
            },
            color: self.color.get_untracked(),
            prominent_color: self.prominent_color.get_untracked(),
            read_only: self.read_only.get_untracked(),
        }
    }
}

/// What a schema-tree right-click landed on. Action data (DDL, AI prompt) is
/// precomputed when the menu opens, since the row has the context then.
#[derive(Clone)]
pub enum CtxKind {
    Database,
    Table {
        database: String,
        table: String,
        ddl: String,
    },
    Field,
}

/// An open schema context menu: what was clicked, its display name (for "Copy
/// name"), and a ready-to-send AI prompt. It's anchored at the last mouse
/// position (tracked in window coords at the root).
#[derive(Clone)]
pub struct CtxMenu {
    pub kind: CtxKind,
    pub name: String,
    pub ai_prompt: String,
}

/// Result channel for the editor's inline (Ctrl+K) AI prompt: the app writes the
/// generated SQL here and the popup previews it before the user Accepts.
#[derive(Clone, Debug, PartialEq)]
pub enum InlineAiState {
    Idle,
    Busy,
    Ready(String),
    Failed(String),
}

/// One inline-AI request from the editor's Ctrl+K prompt.
pub struct InlineAiRequest {
    /// The user's natural-language intent.
    pub intent: String,
    /// The whole editor buffer (context for generation).
    pub current_sql: String,
    /// The selected SQL when transforming a selection; `None` = generate at caret.
    pub selection: Option<String>,
}

/// Terminal-panel signals (Copy bundle). Grouped out of the flat `Ui` god-struct
/// (review §3.3) so the terminal views take a focused handle.
#[derive(Clone, Copy)]
pub struct TermUi {
    /// Latest render snapshot of the terminal grid.
    pub screen: RwSignal<schemaic_term::Screen>,
    /// Whether the terminal panel has keyboard focus (drives the cursor).
    pub focused: RwSignal<bool>,
    /// Whether the terminal settings modal is open.
    pub settings_open: RwSignal<bool>,
    /// Available shells + selected default (for the settings modal).
    pub shells: RwSignal<Vec<schemaic_term::ShellProfile>>,
    pub shell_selected: RwSignal<usize>,
    /// Terminal appearance/behaviour settings (persisted to `terminal.json`).
    pub font_size: RwSignal<u16>,
    pub copy_on_select: RwSignal<bool>,
    pub cursor_style: RwSignal<TermCursor>,
    pub cursor_blink: RwSignal<bool>,
}

/// Terminal-panel callbacks (owned by the app). Bundled behind one `Rc` so
/// cloning `Ui` bumps one refcount instead of a dozen.
pub struct TermActions {
    /// Send already-encoded bytes to the shell.
    pub input: Rc<dyn Fn(Vec<u8>)>,
    /// Resize the terminal to cols×rows.
    pub resize: Rc<dyn Fn(u16, u16)>,
    /// Scroll the terminal viewport by N lines (positive = into history).
    pub scroll: Rc<dyn Fn(i32)>,
    /// Snap the terminal viewport back to the live bottom.
    pub scroll_bottom: Rc<dyn Fn()>,
    /// Respawn the current shell (fresh session).
    pub restart: Rc<dyn Fn()>,
    /// Apply the selected shell (respawns the terminal).
    pub apply_shell: Rc<dyn Fn(usize)>,
    /// Begin / extend / clear a mouse selection (viewport row, col).
    pub sel_start: Rc<dyn Fn(usize, usize)>,
    pub sel_update: Rc<dyn Fn(usize, usize)>,
    pub sel_clear: Rc<dyn Fn()>,
    /// Selected text (for copy), and paste-from-clipboard.
    pub copy: Rc<dyn Fn() -> Option<String>>,
    pub paste: Rc<dyn Fn(String)>,
    /// Open a clicked terminal URL in the OS browser.
    pub open_link: Rc<dyn Fn(String)>,
}

/// AI-panel signals (Copy bundle) — chat state, settings, and the inline (Ctrl+K)
/// generation result.
#[derive(Clone, Copy)]
pub struct AiUi {
    pub messages: RwSignal<Vec<ChatMessage>>,
    pub input: RwSignal<String>,
    pub busy: RwSignal<bool>,
    /// Whether the AI settings modal is open.
    pub settings_open: RwSignal<bool>,
    /// Override path to the `claude` CLI (empty = auto-detect `detected_path`).
    pub cli_path: RwSignal<String>,
    /// Selected model / reasoning effort.
    pub model: RwSignal<AiModel>,
    pub effort: RwSignal<AiEffort>,
    /// Extra instructions appended to the assistant's system prompt.
    pub instructions: RwSignal<String>,
    /// How much schema context to inject into the system prompt.
    pub schema_scope: RwSignal<SchemaScope>,
    /// Whether the assistant may run read-only queries (the `run_query` tool).
    pub run_queries: RwSignal<bool>,
    /// Latest inline (Ctrl+K) generation result, previewed by the popup.
    pub inline: RwSignal<InlineAiState>,
}

/// AI-panel callbacks (owned by the app), plus the auto-detected CLI path.
pub struct AiActions {
    /// Send the given message to the assistant.
    pub send: Rc<dyn Fn(String)>,
    /// Kill the in-flight assistant request (the message-field stop button).
    pub cancel: Rc<dyn Fn()>,
    /// Start a fresh conversation (clear bubbles, drop the session).
    pub new_chat: Rc<dyn Fn()>,
    /// Regenerate the last assistant turn.
    pub regenerate: Rc<dyn Fn()>,
    /// Commit AI settings (restart the session so new model/effort/path apply).
    pub apply: Rc<dyn Fn()>,
    /// Whether Claude is reachable for a given CLI-path value.
    pub cli_ok: Rc<dyn Fn(String) -> bool>,
    /// Kick off an inline (Ctrl+K) generation; the result lands in `AiUi::inline`.
    pub inline_run: Rc<dyn Fn(InlineAiRequest)>,
    /// Cancel an in-flight inline generation (no-op when idle).
    pub inline_cancel: Rc<dyn Fn()>,
    /// Auto-detected `claude` path (`None` = detection failed), the green hint.
    pub detected_path: Option<String>,
}

/// Tabs / query signals (Copy bundle).
#[derive(Clone, Copy)]
pub struct TabsUi {
    pub tabs: RwSignal<Vec<Tab>>,
    pub active: RwSignal<usize>,
    pub flashing: RwSignal<Option<usize>>,
    /// The active tab's database name (`None` = no db yet).
    pub active_db: Memo<Option<String>>,
    /// Whether the active-database menu (in the QUERY toolbar) is open.
    pub active_db_menu_open: RwSignal<bool>,
    /// Bottom-right corner of the DB-selector trigger, in window coords.
    pub active_db_anchor: RwSignal<Point>,
}

/// Tabs / query callbacks (owned by the app).
pub struct TabsActions {
    pub run: Rc<dyn Fn(String)>,
    /// Run several statements in order (Run Everything): one result tab each.
    pub run_all: Rc<dyn Fn(Vec<String>)>,
    pub cancel: Rc<dyn Fn()>,
    /// Commit staged grid changes (cell edits + new-row inserts) transactionally.
    /// Arg 2 is an optional re-fetch request (present ⇒ splice the edited rows
    /// instead of full-re-running; `None` for inserts, which full-re-run); arg 3 is
    /// the completion callback, invoked on the UI thread with the outcome.
    pub commit_edits: CommitFn,
    pub add_tab: Rc<dyn Fn()>,
    pub close_tab: Rc<dyn Fn(usize)>,
    /// Toggle a tab's pinned state (by id) and re-order the strip so pinned tabs
    /// stay contiguous at the left, in pin order.
    pub toggle_pin: Rc<dyn Fn(usize)>,
    /// Duplicate a tab (by id): a new tab with the same connection/database and
    /// query, opened right after the source and made active.
    pub duplicate_tab: Rc<dyn Fn(usize)>,
    /// (database, table) → show it in a tab: focus the tab already showing it, or
    /// open a fresh one ("Open").
    pub open_table: Rc<dyn Fn(String, String)>,
    /// (database, table) → always open the table in a brand-new tab, even if it's
    /// already open ("Open in new tab").
    pub open_table_new: Rc<dyn Fn(String, String)>,
    /// Open a new query tab containing `sql` (does NOT run it).
    pub open_query: Rc<dyn Fn(String)>,
    /// Switch the active tab to a database (remembers it as the new-tab default).
    pub set_active_db: Rc<dyn Fn(String)>,
    /// Open the DB CLI for the active connection in the terminal, optionally
    /// scoped to a database.
    pub open_db_cli: Rc<dyn Fn(Option<String>)>,
    /// Run `EXPLAIN` (or `EXPLAIN ANALYZE` when arg 2 is true) for a statement,
    /// filling the plan modal's state. Targets the active tab's connection/db.
    pub run_plan: Rc<dyn Fn(String, bool)>,
}

/// The global navigation keys — handled at BOTH the workspace root and inside the
/// editor (which `on_event_stop`s every KeyDown, so it can't rely on bubbling).
/// Ctrl+P Find Anywhere · Ctrl+T new tab · Ctrl+W close tab · Ctrl+Tab cycle
/// (Shift = reverse) · Ctrl+1..9 jump to the Nth tab.
#[derive(Clone)]
pub(crate) struct NavKeys {
    pub tabs: RwSignal<Vec<Tab>>,
    pub active: RwSignal<usize>,
    pub find_open: RwSignal<bool>,
    pub add_tab: Rc<dyn Fn()>,
    pub close_tab: Rc<dyn Fn(usize)>,
}

impl NavKeys {
    /// Try to handle a Ctrl-modified key. Callers pass `shift`, the lowercased
    /// character (`ch`, if the key was a Character), and whether it was the Tab
    /// key. Returns true iff it consumed the key. (Ctrl is assumed already checked
    /// by the caller.)
    pub(crate) fn handle(&self, shift: bool, ch: Option<&str>, is_tab: bool) -> bool {
        if is_tab {
            self.cycle(shift);
            return true;
        }
        // Ctrl+Shift+<letter/digit> belong to the panel toggles, not us.
        if shift {
            return false;
        }
        match ch {
            Some("p") => {
                // Find Anywhere; a redundant set(true) would rebuild the overlay.
                if !self.find_open.get_untracked() {
                    self.find_open.set(true);
                }
                true
            }
            Some("t") => {
                (self.add_tab)();
                true
            }
            Some("w") => {
                (self.close_tab)(self.active.get_untracked());
                true
            }
            // Ctrl+1..9 → jump to the 1st..9th tab (by position, not id).
            Some(d) if d.len() == 1 && matches!(d.as_bytes()[0], b'1'..=b'9') => {
                self.jump((d.as_bytes()[0] - b'1') as usize);
                true
            }
            _ => false,
        }
    }

    fn cycle(&self, back: bool) {
        self.tabs.with_untracked(|v| {
            let n = v.len();
            if n == 0 {
                return;
            }
            let pos = v
                .iter()
                .position(|t| t.id == self.active.get_untracked())
                .unwrap_or(0);
            let next = if back {
                (pos + n - 1) % n
            } else {
                (pos + 1) % n
            };
            self.active.set(v[next].id);
        });
    }

    fn jump(&self, idx: usize) {
        self.tabs.with_untracked(|v| {
            if let Some(t) = v.get(idx) {
                self.active.set(t.id);
            }
        });
    }
}

/// Schema-tree signals (Copy bundle).
#[derive(Clone, Copy)]
pub struct SchemaUi {
    pub db_nodes: RwSignal<Vec<ConnNode>>,
    pub expanded: RwSignal<HashSet<String>>,
    /// (database, table) of the active tab's source, highlighted in the tree.
    pub active_table: RwSignal<Option<(String, String)>>,
    /// Names of databases hidden from the schema panel and search.
    pub hidden_dbs: RwSignal<HashSet<String>>,
    /// Whether the database-visibility menu is open.
    pub db_menu_open: RwSignal<bool>,
    /// Whether the SCHEMA settings menu (Refresh) is open.
    pub schema_menu_open: RwSignal<bool>,
}

/// Schema-tree callbacks (owned by the app).
pub struct SchemaActions {
    pub on_toggle: Rc<dyn Fn(String)>,
    /// Toggle a database's hidden state (persists).
    pub toggle_db_hidden: Rc<dyn Fn(String)>,
    /// Collapse every node in the schema tree (databases + tables).
    pub collapse_all: Rc<dyn Fn()>,
    /// Collapse all tables of one database (keeps the database node open).
    pub collapse_db: Rc<dyn Fn(String)>,
    /// Re-introspect the active connection's full schema.
    pub refresh_schema: Rc<dyn Fn()>,
    /// Re-introspect a single database's schema by name (context-menu Refresh).
    pub refresh_db: Rc<dyn Fn(String)>,
}

/// Result of a "Test" of the Manage-Connections draft (host + credentials),
/// shown as an icon on the Test button. Transient — never persisted.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum TestState {
    /// No test run yet (or the draft was edited since the last one).
    #[default]
    Idle,
    /// A test is in flight.
    Testing,
    /// The connection succeeded.
    Ok,
    /// The connection failed (unreachable / auth / tunnel).
    Fail,
}

/// UI-facing lifecycle of the query-plan modal's EXPLAIN run.
#[derive(Clone, Debug)]
pub enum PlanState {
    /// The modal is closed / nothing run yet.
    Idle,
    /// An EXPLAIN is in flight.
    Running,
    /// The plan loaded successfully.
    Loaded(schemaic_core::plan::QueryPlan),
    /// The EXPLAIN failed (message shown in the modal body).
    Failed(String),
}

/// Connection-management signals (Copy bundle).
#[derive(Clone, Copy)]
pub struct ConnUi {
    pub connections: RwSignal<Vec<Connection>>,
    pub active_conn: RwSignal<u64>,
    pub conn_menu_open: RwSignal<bool>,
    /// Live reachability of the active connection (health-checked periodically).
    pub conn_status: RwSignal<ConnStatus>,
    /// Whether the Manage Connections modal is open.
    pub manage_open: RwSignal<bool>,
    /// The Manage-Connections edit form's field signals.
    pub draft: DraftSignals,
    /// Result of the Manage-Connections "Test" button (draft connectivity).
    pub conn_test: RwSignal<TestState>,
}

/// Connection-management callbacks (owned by the app).
pub struct ConnActions {
    pub switch_conn: Rc<dyn Fn(u64)>,
    pub select_conn: Rc<dyn Fn(u64)>,
    pub new_conn: Rc<dyn Fn()>,
    pub save_conn: Rc<dyn Fn()>,
    pub delete_conn: Rc<dyn Fn(u64)>,
    /// Test the draft's host + credentials (opens a throwaway connection/tunnel
    /// and pings); the result lands in [`ConnUi::conn_test`].
    pub test_conn: Rc<dyn Fn()>,
}

/// Query-history signals (Copy bundle). The full list across all connections;
/// the panel filters it to the active connection.
#[derive(Clone, Copy)]
pub struct HistoryUi {
    pub entries: RwSignal<Vec<HistoryEntry>>,
}

/// Query-history callbacks (owned by the app).
pub struct HistoryActions {
    /// Clear the history for the currently-active connection (persists).
    pub clear: Rc<dyn Fn()>,
    /// Reopen a history entry in a new tab: seeds the SQL, the database it ran
    /// against, and the originating tab's custom name (does NOT run it).
    pub open: Rc<dyn Fn(HistoryEntry)>,
}

/// Panel-layout + appearance signals (Copy bundle), persisted across sessions.
/// The single `persist_layout` callback stays flat on [`Ui`].
#[derive(Clone, Copy)]
pub struct LayoutUi {
    /// Whether the schema sidebar is shown.
    pub schema_visible: RwSignal<bool>,
    /// Which panel occupies the right column (AI / Terminal / None).
    pub right_panel: RwSignal<RightPanel>,
    /// Draggable-divider sizes (logical px): schema width, right-column width, and
    /// the query-editor height (the results grid takes the remaining height).
    pub schema_w: RwSignal<f64>,
    pub right_w: RwSignal<f64>,
    pub editor_h: RwSignal<f64>,
    /// Whether the theme settings modal is open.
    pub theme_settings_open: RwSignal<bool>,
    /// Whether the keyboard-shortcuts modal is open.
    pub help_open: RwSignal<bool>,
    /// Active interface (chrome) theme; drives `theme::set_ui`.
    pub ui_theme: RwSignal<theme::UiThemeKind>,
    /// Active SQL-editor theme; drives `theme::set_editor`.
    pub editor_theme: RwSignal<theme::EditorThemeKind>,
    /// Editor font size (px); drives `theme::set_editor_font`.
    pub editor_font: RwSignal<f32>,
    /// Editor tab width (columns); drives `theme::set_editor_tab_width`.
    pub tab_width: RwSignal<usize>,
    /// Editor soft tabs (spaces) vs literal `\t`; drives `theme::set_editor_soft_tabs`.
    pub soft_tabs: RwSignal<bool>,
    /// Editor word wrap; drives `theme::set_editor_word_wrap`.
    pub word_wrap: RwSignal<bool>,
    /// Max rows fetched per query (results-grid cap).
    pub row_limit: RwSignal<usize>,
    /// Confirm before running any write/DDL statement.
    pub confirm_writes: RwSignal<bool>,
    /// Reopen the previous session's query tabs on startup.
    pub restore_tabs: RwSignal<bool>,
}

/// Overlay signals (Copy bundle): the two menu channels, the cursor anchor, and
/// the Find / error modals. No callbacks.
#[derive(Clone, Copy)]
pub struct OverlayUi {
    /// Schema-tree right-click menu target; `None` when closed.
    pub context_menu: RwSignal<Option<CtxMenu>>,
    /// Generic popup menu (built `MenuEntry` list). Opens at `last_mouse` unless
    /// `popup_anchor` is set (a toolbar dropdown anchored under its icon).
    pub popup_menu: RwSignal<Option<Vec<MenuEntry>>>,
    /// When set, `popup_menu` anchors under an icon instead of the cursor:
    /// `(icon_left, icon_right, icon_bottom, panel_width)` in window coords. The
    /// panel drops a few px below the icon and opens left-aligned under it
    /// (overlapping it); if that would spill past the window's right edge it flips
    /// to right-aligned under the icon (right edge flush on `icon_right`, using the
    /// real width — unlike the cursor path's conservative estimate). Set by the
    /// grid's Copy toolbar dropdown; cleared (`None`) by the cursor right-click menus.
    pub popup_anchor: RwSignal<Option<(f64, f64, f64, f64)>>,
    /// Last pointer position in window coords (anchors the context menu).
    pub last_mouse: RwSignal<(f64, f64)>,
    pub find_open: RwSignal<bool>,
    pub find_query: RwSignal<String>,
    /// "View" modal for an error bar. When `error_modal_text` is `Some`, the modal
    /// shows that text (the grid's commit error); otherwise it falls back to the
    /// active tab's full query error (the editor error bar).
    pub error_modal_open: RwSignal<bool>,
    pub error_modal_text: RwSignal<Option<String>>,
    /// Query-plan (EXPLAIN) modal: open flag, the running/loaded state, the
    /// statement being explained (re-run when the Analyze toggle flips), and the
    /// Analyze toggle itself.
    pub plan_open: RwSignal<bool>,
    pub plan_state: RwSignal<PlanState>,
    pub plan_sql: RwSignal<String>,
    pub plan_analyze: RwSignal<bool>,
}

/// All app state + callbacks the UI needs, bundled so views take one argument.
/// The app (schemaic-app) owns the signals and provides the `Rc<dyn Fn>`
/// callbacks; the UI only reads/renders and invokes callbacks.
///
/// Split per-domain into `…Ui` (Copy signals) + `Rc<…Actions>` (callbacks)
/// bundles — review §3.3. Cloning `Ui` bumps a handful of `Rc`s (the `…Actions`
/// bundles + `persist_layout`) instead of ~36.
#[derive(Clone)]
pub struct Ui {
    // Tabs / query — grouped (review §3.3).
    pub tabs_ui: TabsUi,
    pub tab_actions: Rc<TabsActions>,
    // Overlays (menus, Find, error modal) — grouped (review §3.3).
    pub overlay: OverlayUi,
    // Schema tree — grouped (review §3.3).
    pub schema: SchemaUi,
    pub schema_actions: Rc<SchemaActions>,
    // Connections — grouped (review §3.3).
    pub conn: ConnUi,
    pub conn_actions: Rc<ConnActions>,
    // AI panel (Claude Code) — grouped (review §3.3).
    pub ai: AiUi,
    pub ai_actions: Rc<AiActions>,
    // Query history — grouped.
    pub history: HistoryUi,
    pub history_actions: Rc<HistoryActions>,
    // Terminal panel — grouped (review §3.3).
    pub term: TermUi,
    pub term_actions: Rc<TermActions>,
    // Panel layout + appearance — grouped (review §3.3).
    pub layout: LayoutUi,
    /// Persist the current panel layout (divider sizes + visibility) to disk.
    /// Called when a resize drag ends or a divider is double-clicked to reset.
    pub persist_layout: Rc<dyn Fn()>,
    /// App-wide per-column display-formatter rules (persisted to `format.json`),
    /// read + upserted by the results grid's "Format as" menu.
    pub formats: RwSignal<Vec<ColumnFormatRule>>,
    /// Persist the formatter rules to disk (after the grid upserts one).
    pub save_formats: Rc<dyn Fn()>,
    /// Per-database identity colours (persisted to `db_colors.json`), keyed by
    /// `(conn_id, database)`; set from the schema tree, shown as a dot on the DB
    /// node, the active-DB selector, and the database's query tabs.
    pub db_colors: RwSignal<Vec<DbColorRule>>,
    /// Persist the database-colour rules to disk (after a menu upsert).
    pub save_db_colors: Rc<dyn Fn()>,
    /// The app process's own CPU/RAM usage, sampled on a timer at the app
    /// boundary and shown in the status bar. Transient (never persisted).
    pub resources: RwSignal<ResourceSample>,
}

/// Which panel occupies the right column. AI and Terminal are mutually
/// exclusive (they replace each other); `None` frees the space for the editor.
/// Hiding a panel only stops rendering it — its state (chat, live shell) lives
/// in signals/backends that persist, so re-showing restores it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RightPanel {
    None,
    Ai,
    Terminal,
    History,
}

// Convert to/from the serializable core type so the chosen panel persists.
impl From<schemaic_core::persist::RightPanelState> for RightPanel {
    fn from(s: schemaic_core::persist::RightPanelState) -> Self {
        use schemaic_core::persist::RightPanelState as S;
        match s {
            S::None => RightPanel::None,
            S::Ai => RightPanel::Ai,
            S::Terminal => RightPanel::Terminal,
            S::History => RightPanel::History,
        }
    }
}
impl From<RightPanel> for schemaic_core::persist::RightPanelState {
    fn from(p: RightPanel) -> Self {
        use schemaic_core::persist::RightPanelState as S;
        match p {
            RightPanel::None => S::None,
            RightPanel::Ai => S::Ai,
            RightPanel::Terminal => S::Terminal,
            RightPanel::History => S::History,
        }
    }
}

/// AI model choice → Claude CLI `--model` alias.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AiModel {
    Haiku,
    Sonnet,
    Opus,
}
impl AiModel {
    pub const ALL: [AiModel; 3] = [AiModel::Haiku, AiModel::Sonnet, AiModel::Opus];
    /// CLI alias passed to `--model`.
    pub fn cli(self) -> &'static str {
        match self {
            AiModel::Haiku => "haiku",
            AiModel::Sonnet => "sonnet",
            AiModel::Opus => "opus",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            AiModel::Haiku => "Haiku",
            AiModel::Sonnet => "Sonnet",
            AiModel::Opus => "Opus",
        }
    }
    /// Parse a persisted alias; anything unknown falls back to the default (Haiku).
    pub fn from_cli(s: &str) -> AiModel {
        match s {
            "sonnet" => AiModel::Sonnet,
            "opus" => AiModel::Opus,
            _ => AiModel::Haiku,
        }
    }
}

/// AI reasoning effort → Claude CLI `--effort` level (Extra = `xhigh`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AiEffort {
    Low,
    Medium,
    High,
    Extra,
}
impl AiEffort {
    pub const ALL: [AiEffort; 4] = [
        AiEffort::Low,
        AiEffort::Medium,
        AiEffort::High,
        AiEffort::Extra,
    ];
    pub fn cli(self) -> &'static str {
        match self {
            AiEffort::Low => "low",
            AiEffort::Medium => "medium",
            AiEffort::High => "high",
            AiEffort::Extra => "xhigh",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            AiEffort::Low => "Low",
            AiEffort::Medium => "Medium",
            AiEffort::High => "High",
            AiEffort::Extra => "Extra",
        }
    }
    pub fn from_cli(s: &str) -> AiEffort {
        match s {
            "low" => AiEffort::Low,
            "high" => AiEffort::High,
            "xhigh" | "max" => AiEffort::Extra,
            _ => AiEffort::Medium,
        }
    }
}

/// How much schema context to inject into the AI system prompt.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SchemaScope {
    Active,
    All,
    None,
}
impl SchemaScope {
    pub const ALL: [SchemaScope; 3] = [SchemaScope::Active, SchemaScope::All, SchemaScope::None];
    pub fn label(self) -> &'static str {
        match self {
            SchemaScope::Active => "Active database only",
            SchemaScope::All => "All databases",
            SchemaScope::None => "None",
        }
    }
    /// Persisted key.
    pub fn key(self) -> &'static str {
        match self {
            SchemaScope::Active => "active",
            SchemaScope::All => "all",
            SchemaScope::None => "none",
        }
    }
    pub fn from_key(s: &str) -> SchemaScope {
        match s {
            "all" => SchemaScope::All,
            "none" => SchemaScope::None,
            _ => SchemaScope::Active,
        }
    }
}

/// Terminal cursor shape.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TermCursor {
    Block,
    Bar,
    Underline,
}
impl TermCursor {
    pub const ALL: [TermCursor; 3] = [TermCursor::Block, TermCursor::Bar, TermCursor::Underline];
    pub fn label(self) -> &'static str {
        match self {
            TermCursor::Block => "Block",
            TermCursor::Bar => "Bar",
            TermCursor::Underline => "Underline",
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            TermCursor::Block => "block",
            TermCursor::Bar => "bar",
            TermCursor::Underline => "underline",
        }
    }
    pub fn from_key(s: &str) -> TermCursor {
        match s {
            "bar" => TermCursor::Bar,
            "underline" => TermCursor::Underline,
            _ => TermCursor::Block,
        }
    }
}

/// IBM Plex Mono cell metrics (width, height) for a given font size. The width
/// ratio (0.6023) is confirmed by `DIFF_CELL_W` (8.43 at 14px); height keeps the
/// original 18px-at-13px leading. Only these drive how many cols/rows we request.
fn term_cell_wh(font: u16) -> (f64, f64) {
    let f = font as f64;
    (f * 0.6023, (f * 1.3846).round())
}

/// Root view: the app shell (header / body / footer) with any open overlays
/// (connection menu, Find Anywhere, Manage Connections) stacked on top.
pub fn workspace(ui: Ui) -> impl IntoView {
    let last_mouse = ui.overlay.last_mouse;
    let context_menu = ui.overlay.context_menu;
    let popup_menu = ui.overlay.popup_menu;
    let db_menu_open = ui.schema.db_menu_open;
    let schema_menu_open = ui.schema.schema_menu_open;
    // Panel visibility is owned by the app (loaded from / saved to disk), so the
    // layout is restored on the next launch.
    let schema_visible = ui.layout.schema_visible;
    let right_panel = ui.layout.right_panel;
    // Global tab/find navigation keys (shared with the editor's own handler).
    let navkeys = NavKeys {
        tabs: ui.tabs_ui.tabs,
        active: ui.tabs_ui.active,
        find_open: ui.overlay.find_open,
        add_tab: ui.tab_actions.add_tab.clone(),
        close_tab: ui.tab_actions.close_tab.clone(),
    };
    let shell = v_stack((
        header(ui.clone()),
        body(ui.clone(), schema_visible, right_panel),
        footer(ui.clone()),
    ))
    .style(|s| {
        s.size_full()
            .flex_col()
            .background(theme::bg_editor())
            .color(theme::text())
            .font_size(theme::FONT_TITLE)
    });

    stack((
        shell,
        conn_menu_overlay(ui.clone()),
        active_db_menu_overlay(ui.clone()),
        db_visibility_overlay(ui.clone()),
        schema_settings_overlay(ui.clone()),
        context_menu_overlay(ui.clone()),
        popup_menu_overlay(ui.clone()),
        find_overlay(ui.clone()),
        error_modal_overlay(ui.clone()),
        plan_overlay(ui.clone()),
        term_settings_overlay(ui.clone()),
        ai_settings_overlay(ui.clone()),
        theme_settings_overlay(ui.clone()),
        help_overlay(ui.clone()),
        manage_modal(ui),
    ))
    // Track the pointer in window coordinates (root-local == window) so the
    // schema context menu can anchor at the cursor.
    .on_event(EventListener::PointerMove, move |e| {
        if let Some(p) = e.point() {
            last_mouse.set((p.x, p.y));
        }
        EventPropagation::Continue
    })
    // Publish the window size (for menu edge-flipping).
    .on_resize(|r| window_size().set((r.width(), r.height())))
    // Panel toggles when focus is OUTSIDE the editor (the editor handles these
    // in its own key handler and stops propagation; anything else that doesn't
    // consume the key bubbles up here). Ctrl+Shift+E / Ctrl+Shift+A / Ctrl+`.
    .on_event(EventListener::KeyDown, move |e| {
        if let Event::KeyDown(ke) = e {
            let m = ke.modifiers;
            if m.control() {
                // Global nav (Ctrl+P/T/W/Tab/1-9) — also wired inside the editor,
                // which stops KeyDown; here it catches every other focus (grid,
                // schema, nothing) since those bubble unhandled keys up.
                let is_tab = matches!(ke.key.logical_key, Key::Named(NamedKey::Tab));
                let ch = match &ke.key.logical_key {
                    Key::Character(c) => Some(c.as_str().to_ascii_lowercase()),
                    _ => None,
                };
                if navkeys.handle(m.shift(), ch.as_deref(), is_tab) {
                    return EventPropagation::Stop;
                }
                if let Key::Character(c) = &ke.key.logical_key {
                    let c = c.as_str();
                    if m.shift() && c.eq_ignore_ascii_case("e") {
                        schema_visible.update(|v| *v = !*v);
                        return EventPropagation::Stop;
                    }
                    if m.shift() && c.eq_ignore_ascii_case("a") {
                        right_panel.update(|p| {
                            *p = if matches!(*p, RightPanel::Ai) {
                                RightPanel::None
                            } else {
                                RightPanel::Ai
                            };
                        });
                        return EventPropagation::Stop;
                    }
                    if c == "`" {
                        right_panel.update(|p| {
                            *p = if matches!(*p, RightPanel::Terminal) {
                                RightPanel::None
                            } else {
                                RightPanel::Terminal
                            };
                        });
                        return EventPropagation::Stop;
                    }
                }
            }
        }
        EventPropagation::Continue
    })
    // Any pointer-down anywhere closes an open schema context menu (OS-like:
    // a fresh right-click collapses the previous menu). The menu panel itself
    // stops pointer-downs, so this doesn't fire when interacting with it; and a
    // right-click on another row closes the old menu here (on down) while that
    // row's own handler opens the new one (on up) — one gesture.
    .on_event(EventListener::PointerDown, move |_| {
        if context_menu.get_untracked().is_some() {
            context_menu.set(None);
        }
        if popup_menu.get_untracked().is_some() {
            popup_menu.set(None);
        }
        if db_menu_open.get_untracked() {
            db_menu_open.set(false);
        }
        if schema_menu_open.get_untracked() {
            schema_menu_open.set(false);
        }
        EventPropagation::Continue
    })
    .style(|s| {
        s.size_full()
            // Floem's default theme paints text inputs white — and also sets
            // light backgrounds for the hover/active/focus states. Override the
            // class for every state so inputs stay dark throughout (app + modals).
            .class(TextInputClass, |s| {
                s.background(theme::bg_deepest())
                    .color(theme::text())
                    .font_size(theme::FONT_BODY)
                    .cursor(CursorStyle::Text)
                    .cursor_color(floem::peniko::Brush::Solid(theme::accent()))
                    .border(1.0)
                    .border_color(theme::field_border())
                    .border_radius(6.0)
                    .padding_horiz(6.0)
                    .hover(|s| {
                        s.background(theme::bg_deepest())
                            .border_color(theme::field_border())
                    })
                    .active(|s| s.background(theme::bg_deepest()))
                    .focus(|s| {
                        s.background(theme::bg_deepest())
                            .border_color(theme::field_border_active())
                            .hover(|s| s.background(theme::bg_deepest()))
                    })
            })
            // Global scrollbar handle style — cascades to every scroll (panels,
            // editor, inputs, lists). Resting #232431, hover #2F3243, 6px rounded.
            // (The 3px edge inset is per-scroll — see `thin_scroll` — since the
            // inset prop doesn't cascade.)
            .class(Handle, |s| {
                s.background(theme::scrollbar())
                    .set(Thickness, Px(6.0))
                    .set(Rounded, true)
                    .hover(|s| s.background(theme::scrollbar_hover()))
            })
            // Transparent track (resting + hover) — the container shows through, so
            // there's no visible track behind the handle, just the floating thumb.
            .class(Track, |s| {
                let clear = floem::peniko::Color::TRANSPARENT;
                s.background(clear).hover(|s| s.background(clear))
            })
            // Custom tooltip chrome (replaces Floem's bare default) — a compact
            // bordered panel with a soft drop shadow, applied to every `.tooltip(…)`.
            .class(TooltipClass, tooltip_style)
            // Shorten the hover delay from Floem's 600ms default (felt sluggish).
            .class(TooltipContainerClass, |s| {
                s.set(Delay, std::time::Duration::from_millis(300))
            })
    })
}

// ── Header ────────────────────────────────────────────────────────────────
fn header(ui: Ui) -> impl IntoView {
    let connections = ui.conn.connections;
    let active_conn = ui.conn.active_conn;
    let conn_menu_open = ui.conn.conn_menu_open;
    let conn_status = ui.conn.conn_status;
    let find_open = ui.overlay.find_open;
    let theme_settings_open = ui.layout.theme_settings_open;
    let help_open = ui.layout.help_open;

    // Connection switcher: shows the active connection's name; click toggles the
    // dropdown (rendered as an overlay so it floats above the app).
    let conn_label = move || {
        connections.with(|cs| {
            cs.iter()
                .find(|c| c.id == active_conn.get())
                .map(|c| c.name.clone())
                .unwrap_or_else(|| "No connection".to_string())
        })
    };
    let switcher = container(
        h_stack((
            label(conn_label).style(|s| s.color(theme::text())),
            icons::icon(icons::CHEVRON_DOWN, 16.0)
                .style(move |s| s.color(active_conn_color(connections, active_conn))),
        ))
        .style(|s| s.flex_row().items_center().gap(6.0)),
    )
    .on_click_stop(move |_| conn_menu_open.update(|o| *o = !*o))
    .style(move |s| {
        s.padding_left(11.0)
            .padding_right(7.0)
            .padding_vert(3.0)
            .margin_top(7.0)
            .margin_bottom(7.0)
            .items_center()
            // Opaque fill (same color as the header) so the 1px border has a
            // solid backing and renders crisply — an outline over a transparent
            // interior anti-aliases on both edges and looks blurry.
            .background(theme::bg_chrome())
            .border(1.0)
            .border_color(active_conn_color(connections, active_conn))
            .border_radius(5.0)
            .hover(|s| s.background(theme::bg_panel()))
    });

    // Find-anywhere trigger: a plain Lucide search glyph, 24px, 20px from the
    // header's right edge (brightens on hover like the schema-panel icons).
    let search = icons::icon(icons::SEARCH, 20.0)
        .on_click_stop(move |_| find_open.set(true))
        .style(|s| {
            s.flex_shrink(0.0_f32)
                .margin_right(16.0)
                .color(theme::text_muted())
                .hover(|s| s.color(theme::text()))
        });

    // Keyboard-shortcuts help, 20px, just left of the settings gear — same look
    // and spacing as the other header glyphs.
    let help = icons::icon(icons::CIRCLE_QUESTION, 20.0)
        .on_click_stop(move |_| help_open.set(true))
        .style(|s| {
            s.flex_shrink(0.0_f32)
                .margin_right(16.0)
                .color(theme::text_muted())
                .hover(|s| s.color(theme::text()))
        });

    // App settings (theme picker), 20px, sitting just right of the search glyph.
    let settings = icons::icon(icons::SETTINGS, 20.0)
        .on_click_stop(move |_| theme_settings_open.set(true))
        .style(|s| {
            s.flex_shrink(0.0_f32)
                .margin_right(20.0)
                .color(theme::text_muted())
                .hover(|s| s.color(theme::text()))
        });
    let right = h_stack((search, help, settings)).style(|s| s.items_center());

    // Left cluster (dot + switcher) and the right glyph cluster, pinned to
    // opposite edges via `justify_between` (a lone flex-grow spacer under-fills —
    // see the schema title-row note). The dot's own `margin_left(15)` sets the
    // left inset.
    let left =
        h_stack((connection_dot(conn_status), switcher)).style(|s| s.flex_row().items_center());
    h_stack((left, right)).style(|s| {
        s.width_full()
            .height(theme::HEADER_H)
            .min_height(theme::HEADER_H)
            .flex_shrink(0.0_f32)
            .flex_row()
            .items_center()
            .justify_between()
            .background(theme::bg_chrome())
            .border_bottom(1.0)
            .border_color(theme::border())
    })
}

fn connection_dot(conn_status: RwSignal<ConnStatus>) -> impl IntoView {
    // 15px from the header's left edge, 15px from the switcher button — carries
    // the live health status (the identity colour is on the switcher outline).
    icons::icon(icons::DOT, 6.0).style(move |s| {
        s.color(status_color(conn_status.get()))
            .margin_left(15.0)
            .margin_right(15.0)
    })
}

/// Live connection-status accent: green when reachable, red when not, neutral
/// until the first health check lands. Drives the status dots (the connection
/// identity colour lives on the switcher outline instead).
pub(crate) fn status_color(status: ConnStatus) -> floem::peniko::Color {
    match status {
        ConnStatus::Connected => theme::conn_ok(),
        ConnStatus::Disconnected => theme::reject_bg(),
        ConnStatus::Unknown => theme::text_dim(),
    }
}

/// The active connection's identity colour (its `#rrggbb` parsed to a `Color`),
/// with a neutral fallback for a legacy/absent colour. Reactive — call inside a
/// `.style(…)`/effect closure so a colour or connection change re-runs it.
pub(crate) fn active_conn_color(
    connections: RwSignal<Vec<Connection>>,
    active_conn: RwSignal<u64>,
) -> floem::peniko::Color {
    let id = active_conn.get();
    connections
        .with(|cs| {
            cs.iter()
                .find(|c| c.id == id)
                .and_then(|c| c.color.as_deref())
                .and_then(theme::parse_hex)
        })
        .unwrap_or_else(theme::text_dim)
}

/// The active connection's editor-frame colour: its identity colour when that
/// connection has the "prominent colour" toggle on, else `None` (no frame).
/// Reactive — call inside a `.style(…)`/effect closure.
pub(crate) fn active_conn_frame_color(
    connections: RwSignal<Vec<Connection>>,
    active_conn: RwSignal<u64>,
) -> Option<floem::peniko::Color> {
    let id = active_conn.get();
    connections.with(|cs| {
        cs.iter().find(|c| c.id == id).and_then(|c| {
            if c.prominent_color {
                c.color.as_deref().and_then(theme::parse_hex)
            } else {
                None
            }
        })
    })
}

/// A 2px identity-colour rule pinned to the top (`top`) or bottom edge of its
/// parent container, in the active connection's colour when the "prominent
/// colour" toggle is on (transparent otherwise). An absolute, pointer-events-off
/// overlay drawn *over* the parent's existing 1px border: it takes no layout
/// space, so toggling the setting never shifts the panels by a pixel. Wrap a
/// fixed-size element (tab bar, grid, footer) in a `stack` with this as the last
/// child so it paints on top and hugs the chosen edge.
pub(crate) fn conn_edge_border(
    connections: RwSignal<Vec<Connection>>,
    active_conn: RwSignal<u64>,
    top: bool,
) -> impl IntoView {
    empty()
        .style(move |s| {
            let color = active_conn_frame_color(connections, active_conn)
                .unwrap_or(floem::peniko::Color::TRANSPARENT);
            let s = s.absolute().inset(0.0).border_color(color);
            if top {
                s.border_top(2.0)
            } else {
                s.border_bottom(2.0)
            }
        })
        .pointer_events(|| false)
}

/// A small identity dot (6px — matching the connection status dot) for a database
/// that has an identity colour, or a zero-footprint `empty()` when it doesn't, so
/// uncoloured databases render exactly as before. `key` yields the `(conn_id,
/// database)` to look up reactively; `ml`/`mr`/`mt` are the dot's margins (left /
/// right / top), applied only when a dot is drawn — `mt` fine-tunes its vertical
/// alignment against the neighbouring text. Rebuilds when the colour or key
/// changes. The colour is a fixed identity hex (not themable), so capturing it by
/// value here is correct.
pub(crate) fn db_color_dot(
    db_colors: RwSignal<Vec<DbColorRule>>,
    key: impl Fn() -> Option<(u64, String)> + 'static,
    ml: f64,
    mr: f64,
    mt: f64,
) -> impl IntoView {
    dyn_container(
        move || {
            key().and_then(|(cid, db)| {
                db_colors.with(|rules| schemaic_core::db_color::lookup(rules, cid, &db))
            })
        },
        move |hex| match hex.as_deref().and_then(theme::parse_hex) {
            Some(color) => icons::icon(icons::DOT, 6.0)
                .style(move |s| {
                    s.color(color)
                        .flex_shrink(0.0_f32)
                        .margin_left(ml)
                        .margin_right(mr)
                        .margin_top(mt)
                })
                .into_any(),
            None => empty().into_any(),
        },
    )
}

/// One identity-colour preset: `(display name, #rrggbb hex, parsed-colour
/// accessor)`. The accessor is a `fn` pointer because menu icon/label colours are
/// `fn`s (so they can follow theme switches), so each preset needs a concrete one.
pub(crate) type ColorPreset = (&'static str, &'static str, fn() -> floem::peniko::Color);

/// Preset identity colours — saturated mid-tones that read on both themes. Single
/// source for the connection-form swatches, the auto-assign pool, and the database
/// colour picker.
pub(crate) const CONN_COLOR_PRESETS: &[ColorPreset] = &[
    ("Red", "#E05252", || parse_preset("#E05252")),
    ("Orange", "#E08A4B", || parse_preset("#E08A4B")),
    ("Amber", "#E0C24B", || parse_preset("#E0C24B")),
    ("Green", "#52C77A", || parse_preset("#52C77A")),
    ("Teal", "#43C6C6", || parse_preset("#43C6C6")),
    ("Blue", "#5B8DEF", || parse_preset("#5B8DEF")),
    ("Purple", "#9B6DE0", || parse_preset("#9B6DE0")),
    ("Pink", "#E06D9B", || parse_preset("#E06D9B")),
];

fn parse_preset(hex: &str) -> floem::peniko::Color {
    theme::parse_hex(hex).unwrap_or(floem::peniko::Color::TRANSPARENT)
}

/// Pick an identity colour for a new connection: a preset not already used by an
/// existing connection (so colours stay distinct), or — once every preset is
/// taken — one at random from the full palette. `used` is the existing colours.
pub fn pick_connection_color(used: &[String]) -> String {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as usize)
        .unwrap_or(0);
    let is_used = |c: &str| used.iter().any(|u| u.eq_ignore_ascii_case(c));
    let all: Vec<&str> = CONN_COLOR_PRESETS.iter().map(|(_, hex, _)| *hex).collect();
    let unused: Vec<&str> = all.iter().copied().filter(|c| !is_used(c)).collect();
    let pool = if unused.is_empty() { &all } else { &unused };
    pool[seed % pool.len()].to_string()
}

// A vertical divider between two side-by-side panels: absolute, full-height,
// centered on the boundary. `from_right` anchors it by `inset_right` (the right
// column) rather than `inset_left` (schema); `dim` is the width signal it drags,
// with the boundary snapping to the pointer. Collapses to a 0-width no-op when the
// panel is hidden.
fn h_resize_handle(
    from_right: bool,
    dim: RwSignal<f64>,
    visible: impl Fn() -> bool + Copy + 'static,
    // Owned by the caller so the panel wrapper can drop its width transition while
    // dragging (the transition is for the collapse slide; during a resize it just
    // makes the width lag the pointer).
    dragging: RwSignal<bool>,
    // Double-click resets `dim` to this default (animated, since not dragging).
    default: f64,
    // Persist the layout after a drag ends or a reset (debounced to gesture-end so
    // we don't write on every pixel).
    on_commit: Rc<dyn Fn()>,
) -> impl IntoView {
    let hovered = RwSignal::new(false);
    let bar = empty().style(move |s| {
        let s = s.width(RESIZE_BAR).height_full();
        if hovered.get() || dragging.get() {
            s.background(theme::resize_handle())
        } else {
            s
        }
    });
    let handle = container(bar);
    let id = handle.id();
    handle
        .style(move |s| {
            let s = s
                .absolute()
                .height_full()
                .items_center()
                .justify_center()
                .cursor(CursorStyle::ColResize)
                .width(if visible() { RESIZE_HIT } else { 0.0 });
            let inset = dim.get() - RESIZE_HIT / 2.0;
            if from_right {
                s.inset_right(inset)
            } else {
                s.inset_left(inset)
            }
        })
        .on_event(EventListener::PointerEnter, move |_| {
            hovered.set(true);
            EventPropagation::Continue
        })
        .on_event(EventListener::PointerLeave, move |_| {
            hovered.set(false);
            EventPropagation::Continue
        })
        .on_event_stop(EventListener::PointerDown, move |_| {
            dragging.set(true);
            id.request_active();
        })
        .on_event(EventListener::PointerMove, move |e| {
            if dragging.get_untracked() {
                if let Event::PointerMove(pe) = e {
                    // pe.pos is relative to the handle, whose center rides the
                    // boundary; (pos - center) is the pointer's offset from it, so
                    // adding it snaps the boundary to the pointer (negated for the
                    // right column, which grows leftward).
                    let d = pe.pos.x - RESIZE_HIT / 2.0;
                    let d = if from_right { -d } else { d };
                    // Floor at RESIZE_MIN; ceiling so a panel can't be dragged past
                    // the window edge and swallow the center (leave ≥300px for it).
                    // `window_size` is (0,0) until the first resize — skip the
                    // ceiling then (§7.4).
                    let ww = window_size().get_untracked().0;
                    let hi = if ww > 1.0 {
                        (ww - 300.0).max(RESIZE_MIN)
                    } else {
                        f64::INFINITY
                    };
                    dim.update(|w| *w = (*w + d).clamp(RESIZE_MIN, hi));
                }
                EventPropagation::Stop
            } else {
                EventPropagation::Continue
            }
        })
        .on_event_stop(EventListener::PointerUp, {
            let on_commit = on_commit.clone();
            move |_| {
                dragging.set(false);
                id.clear_active();
                on_commit();
            }
        })
        .on_double_click_stop(move |_| {
            // The double-click's second PointerUp is consumed here (not by the
            // PointerUp handler), so clear the drag state ourselves — otherwise the
            // handle stays captured/active and keeps resizing on mouse-move.
            dragging.set(false);
            id.clear_active();
            dim.set(default);
            on_commit();
        })
}

// A horizontal divider between the query editor and the results grid (drags
// up/down). `base_top` offsets past the tab bar to the editor's bottom edge; `dim`
// is the editor height. Always shown (both areas are always present).
fn v_resize_handle(
    base_top: f64,
    dim: RwSignal<f64>,
    default: f64,
    on_commit: Rc<dyn Fn()>,
) -> impl IntoView {
    let hovered = RwSignal::new(false);
    let dragging = RwSignal::new(false);
    let bar = empty().style(move |s| {
        let s = s.height(RESIZE_BAR).width_full();
        if hovered.get() || dragging.get() {
            s.background(theme::resize_handle())
        } else {
            s
        }
    });
    let handle = container(bar);
    let id = handle.id();
    handle
        .style(move |s| {
            s.absolute()
                .width_full()
                .items_center()
                .justify_center()
                .cursor(CursorStyle::RowResize)
                .height(RESIZE_HIT)
                .inset_top(base_top + dim.get() - RESIZE_HIT / 2.0)
        })
        .on_event(EventListener::PointerEnter, move |_| {
            hovered.set(true);
            EventPropagation::Continue
        })
        .on_event(EventListener::PointerLeave, move |_| {
            hovered.set(false);
            EventPropagation::Continue
        })
        .on_event_stop(EventListener::PointerDown, move |_| {
            dragging.set(true);
            id.request_active();
        })
        .on_event(EventListener::PointerMove, move |e| {
            if dragging.get_untracked() {
                if let Event::PointerMove(pe) = e {
                    let d = pe.pos.y - RESIZE_HIT / 2.0;
                    dim.update(|h| *h = (*h + d).max(RESIZE_MIN));
                }
                EventPropagation::Stop
            } else {
                EventPropagation::Continue
            }
        })
        .on_event_stop(EventListener::PointerUp, {
            let on_commit = on_commit.clone();
            move |_| {
                dragging.set(false);
                id.clear_active();
                on_commit();
            }
        })
        .on_double_click_stop(move |_| {
            // See h_resize_handle: clear drag state the eaten PointerUp would have.
            dragging.set(false);
            id.clear_active();
            dim.set(default);
            on_commit();
        })
}

// ── Body: schema | center | ai ──────────────────────────────────────────────
fn body(
    ui: Ui,
    schema_visible: RwSignal<bool>,
    right_panel: RwSignal<RightPanel>,
) -> impl IntoView {
    let schema_w = ui.layout.schema_w;
    let right_w = ui.layout.right_w;
    let ui_persist = ui.persist_layout.clone();
    let ui_schema = ui.clone();
    let ui_center = ui.clone();
    let ui_right = ui;

    // Collapsing a side panel animates its wrapper width 0↔full over 150ms; the
    // center (query/results) flex-grows, so it widens/narrows in step. The panel
    // content stays mounted at its natural width and the wrapper `clip`s it — so
    // there's something to reveal/hide during the slide instead of a blank box.
    // `min_width(0)` lets the wrapper actually reach 0 despite the fixed-width
    // child; `flex_shrink(0)` keeps it at exactly the animated width.
    let anim = || Transition::ease_in_out(std::time::Duration::from_millis(150));

    // While a divider is being dragged, its panel drops the width transition so the
    // edge tracks the pointer 1:1 instead of easing toward each step (which reads as
    // lag). The transition is only wanted for the collapse/expand slide.
    let schema_dragging = RwSignal::new(false);
    let right_dragging = RwSignal::new(false);

    // Left: the schema tree. Always mounted (it only reads signals; nothing is
    // spawned on build), so hiding is purely the width animation. `clip()` hides
    // the fixed-width content as the wrapper narrows.
    let schema = schema_panel(ui_schema).clip().style(move |s| {
        let s = s.flex_shrink(0.0_f32).min_width(0.0);
        let s = if schema_dragging.get() {
            s
        } else {
            s.transition(Width, anim())
        };
        if schema_visible.get() {
            s.width(schema_w.get())
        } else {
            s.width(0.0)
        }
    });

    // Right: AI or Terminal. `right_content` sticks to the last non-None panel so
    // its content lingers (clipped) through the collapse rather than popping out
    // the instant the panel closes; only the wrapper width animates.
    let right_content = RwSignal::new(match right_panel.get_untracked() {
        RightPanel::None => RightPanel::Ai,
        p => p,
    });
    create_effect(move |_| {
        let p = right_panel.get();
        if p != RightPanel::None {
            right_content.set(p);
        }
    });
    let right_inner = dyn_container(
        move || right_content.get(),
        move |panel| match panel {
            RightPanel::Terminal => terminal_panel(ui_right.clone()).into_any(),
            RightPanel::History => history_panel(ui_right.clone()).into_any(),
            _ => ai_panel(ui_right.clone()).into_any(),
        },
    );
    let right = right_inner.clip().style(move |s| {
        let s = s.flex_shrink(0.0_f32).min_width(0.0);
        let s = if right_dragging.get() {
            s
        } else {
            s.transition(Width, anim())
        };
        if right_panel.get() == RightPanel::None {
            s.width(0.0)
        } else {
            s.width(right_w.get())
        }
    });

    // Drag handles overlay the panel boundaries (absolute → no layout impact).
    // Order them last so they paint over the panels' 1px separator lines.
    // Double-click resets to the hardcoded defaults; drag-end/reset persists.
    let commit = ui_persist.clone();
    let schema_handle = h_resize_handle(
        false,
        schema_w,
        move || schema_visible.get(),
        schema_dragging,
        theme::SCHEMA_W,
        commit,
    );
    let commit = ui_persist.clone();
    let right_handle = h_resize_handle(
        true,
        right_w,
        move || right_panel.get() != RightPanel::None,
        right_dragging,
        theme::AI_W,
        commit,
    );

    h_stack((
        schema,
        center(ui_center),
        right,
        schema_handle,
        right_handle,
    ))
    .style(|s| s.width_full().flex_grow(1.0_f32).flex_row().min_height(0.0))
}

// DDL generation from the introspected schema lives in
// `schemaic_core::schema::TableInfo::create_ddl` (pure, unit-tested there).

// The center column: tab bar, then the active tab's query editor over its
// Results grid. The content is keyed on the active tab id, so switching tabs
// rebuilds the editor from that tab's buffer.
fn center(ui: Ui) -> impl IntoView {
    let tabs = ui.tabs_ui.tabs;
    let active = ui.tabs_ui.active;
    let flashing = ui.tabs_ui.flashing;
    let connections = ui.conn.connections;
    let active_conn = ui.conn.active_conn;
    // Is the active tab's connection read-only? (Reactive — follows the tab's
    // `conn_id` and a live toggle of the connection.) Gates cell edits + write runs.
    let read_only = create_memo(move |_| {
        let id = active.get();
        let cid = tabs.with(|v| v.iter().find(|t| t.id == id).map(|t| t.conn_id.get()));
        match cid {
            Some(cid) => connections
                .with(|cs| cs.iter().find(|c| c.id == cid).map(|c| c.read_only))
                .unwrap_or(false),
            None => false,
        }
    });
    let confirm_writes = ui.layout.confirm_writes;
    let run = ui.tab_actions.run.clone();
    let run_all = ui.tab_actions.run_all.clone();
    let cancel = ui.tab_actions.cancel.clone();
    let db_nodes = ui.schema.db_nodes;
    let inline_ai = ui.ai.inline;
    let inline_ai_run = ui.ai_actions.inline_run.clone();
    let inline_ai_cancel = ui.ai_actions.inline_cancel.clone();
    let error_modal_open = ui.overlay.error_modal_open;
    let error_modal_text = ui.overlay.error_modal_text;
    let schema_visible = ui.layout.schema_visible;
    let right_panel = ui.layout.right_panel;
    let ai_send = ui.ai_actions.send.clone();
    let context_menu = ui.overlay.context_menu;
    let popup = ui.overlay.popup_menu;
    let popup_anchor = ui.overlay.popup_anchor;
    let editor_h = ui.layout.editor_h;
    // Reveal the AI panel + send a message (the grid cell "AI Summary" builds a
    // context-rich prompt itself, so this just reveals + forwards).
    let summarize: Rc<dyn Fn(String)> = {
        let ai = ai_send.clone();
        Rc::new(move |msg: String| {
            if !matches!(right_panel.get_untracked(), RightPanel::Ai) {
                right_panel.set(RightPanel::Ai);
            }
            (ai)(msg);
        })
    };
    // Close any other open menu — grid cells consume the pointer-down, so the root
    // dismissal handler never fires for clicks inside the grid, and the toolbar Copy
    // dropdown calls this before opening so it's mutually exclusive with the schema
    // eye/settings (and other) dropdowns.
    let db_menu_open_d = ui.schema.db_menu_open;
    let schema_menu_open_d = ui.schema.schema_menu_open;
    let conn_menu_open_d = ui.conn.conn_menu_open;
    let active_db_menu_open_d = ui.tabs_ui.active_db_menu_open;
    let dismiss_menus: Rc<dyn Fn()> = Rc::new(move || {
        if popup.get_untracked().is_some() {
            popup.set(None);
        }
        if context_menu.get_untracked().is_some() {
            context_menu.set(None);
        }
        if db_menu_open_d.get_untracked() {
            db_menu_open_d.set(false);
        }
        if schema_menu_open_d.get_untracked() {
            schema_menu_open_d.set(false);
        }
        if conn_menu_open_d.get_untracked() {
            conn_menu_open_d.set(false);
        }
        if active_db_menu_open_d.get_untracked() {
            active_db_menu_open_d.set(false);
        }
    });
    let commit_edits = ui.tab_actions.commit_edits.clone();
    let active_db = ui.tabs_ui.active_db;
    let active_db_menu_open = ui.tabs_ui.active_db_menu_open;
    let active_db_anchor = ui.tabs_ui.active_db_anchor;
    let formats = ui.formats;
    let save_formats = ui.save_formats.clone();
    // Global nav keys, so the editor can handle Ctrl+P/T/W/Tab/1-9 (it stops
    // KeyDown propagation, so the workspace-root handler can't see them).
    let navkeys = NavKeys {
        tabs: ui.tabs_ui.tabs,
        active: ui.tabs_ui.active,
        find_open: ui.overlay.find_open,
        add_tab: ui.tab_actions.add_tab.clone(),
        close_tab: ui.tab_actions.close_tab.clone(),
    };

    // Open the query-plan modal for a statement (from the editor's "Plan" menu):
    // seed the statement, reset the Analyze toggle, and open. The actual EXPLAIN is
    // kicked off by the modal's own effect (which also re-runs when Analyze flips),
    // so we just pre-set Running here to avoid a flash of the previous plan.
    let open_plan: Rc<dyn Fn(String)> = {
        let plan_open = ui.overlay.plan_open;
        let plan_sql = ui.overlay.plan_sql;
        let plan_analyze = ui.overlay.plan_analyze;
        let plan_state = ui.overlay.plan_state;
        Rc::new(move |stmt: String| {
            plan_sql.set(stmt);
            plan_analyze.set(false);
            plan_state.set(PlanState::Running);
            plan_open.set(true);
        })
    };

    // Editor area: the active tab's query editor — or, while that tab is
    // "flashing" closed, a solid placeholder of identical size, so nothing
    // below it shifts.
    let editor_area = dyn_container(
        move || (active.get(), flashing.get() == Some(active.get())),
        move |(id, is_flashing)| {
            if is_flashing {
                return editor_placeholder(editor_h).into_any();
            }
            match tabs.with_untracked(|v| v.iter().find(|t| t.id == id).copied()) {
                Some(tab) => query_pane(QueryPaneParams {
                    query: tab.query,
                    cursor_offset: tab.cursor_offset,
                    goto_open: tab.goto_open,
                    results: tab.results,
                    run: run.clone(),
                    run_all: run_all.clone(),
                    db_nodes,
                    inline_ai,
                    inline_ai_run: inline_ai_run.clone(),
                    inline_ai_cancel: inline_ai_cancel.clone(),
                    error_modal_open,
                    schema_visible,
                    right_panel,
                    ai_send: ai_send.clone(),
                    context_menu,
                    editor_h,
                    active_db,
                    active_db_menu_open,
                    active_db_anchor,
                    read_only,
                    confirm_writes,
                    popup_menu: popup,
                    popup_anchor,
                    open_plan: open_plan.clone(),
                    nav: navkeys.clone(),
                })
                .into_any(),
                None => editor_placeholder(editor_h).into_any(),
            }
        },
    )
    .style(|s| {
        s.width_full()
            .flex_shrink(0.0_f32)
            .flex_col()
            .min_width(0.0)
    });

    // Results area: the active tab's grid. Deliberately NOT tied to `flashing`,
    // so closing the last tab leaves the table exactly in place.
    let results_area = dyn_container(
        move || active.get(),
        move |id| match tabs.with_untracked(|v| v.iter().find(|t| t.id == id).copied()) {
            Some(tab) => results_section(
                tab.results,
                tab.result_tabs,
                tab.active_result,
                cancel.clone(),
                GridCtx {
                    source: tab.source,
                    db_nodes,
                    connections,
                    active_conn,
                    popup,
                    popup_anchor,
                    summarize: summarize.clone(),
                    dismiss: dismiss_menus.clone(),
                    commit: commit_edits.clone(),
                    // `results_view` fills this in for the single-result path; the
                    // multi-result path leaves it `None` (full-re-run on commit).
                    sync_canonical: None,
                    read_only,
                    conn_id: tab.conn_id,
                    formats,
                    save_formats: save_formats.clone(),
                    // Find state (Ctrl+F), created per active-tab render.
                    find_open: RwSignal::new(false),
                    find_query: RwSignal::new(String::new()),
                    find_step: RwSignal::new((0u64, true)),
                    find_total: RwSignal::new(0),
                    find_pos: RwSignal::new(0),
                    find_more: RwSignal::new(false),
                    // Commit-error bar (bottom) — its own per-tab-render signal;
                    // "View" opens the shared workspace error modal with its text.
                    commit_err: RwSignal::new(None),
                    error_open: error_modal_open,
                    error_text: error_modal_text,
                },
            )
            .into_any(),
            None => empty().into_any(),
        },
    )
    .style(|s| {
        s.flex_grow(1.0_f32)
            .width_full()
            .flex_col()
            .min_height(0.0)
            .min_width(0.0)
    });

    // Divider between editor and results, offset past the tab bar. Double-click
    // resets to the default editor height; drag-end/reset persists the layout.
    let split_handle = v_resize_handle(TAB_BAR_H, editor_h, EDITOR_H, ui.persist_layout.clone());

    // Identity-colour rule under the tab strip (drawn on the "prominent colour"
    // setting). Wrapping the tab bar in a `stack` pins the 2px line to the bar's
    // bottom edge as a no-layout overlay, so enabling it never nudges the editor.
    let tabs_row = stack((
        tab_bar(ui.clone()),
        conn_edge_border(connections, active_conn, false),
    ))
    .style(|s| s.width_full().flex_shrink(0.0_f32));

    v_stack((tabs_row, editor_area, results_area, split_handle)).style(|s| {
        s.flex_grow(1.0_f32)
            .height_full()
            .flex_col()
            .min_height(0.0)
            .min_width(0.0)
    })
}

// The RESULTS pane for one tab. Single runs use the legacy single-grid view
// (`results`); a Run Everything batch shows a result-tab strip over the active
// statement's grid (`result_tabs` non-empty).
fn results_section(
    results: RwSignal<QueryState>,
    result_tabs: RwSignal<Vec<ResultPanel>>,
    active_result: RwSignal<usize>,
    cancel: Rc<dyn Fn()>,
    gctx: GridCtx,
) -> impl IntoView {
    // Find-bar + error-bar signals (Copy) — captured before `gctx` moves into `body`.
    let (find_open, find_query, find_step) = (gctx.find_open, gctx.find_query, gctx.find_step);
    let (find_total, find_pos, find_more) = (gctx.find_total, gctx.find_pos, gctx.find_more);
    let (commit_err, error_open, error_text) = (gctx.commit_err, gctx.error_open, gctx.error_text);
    let body = dyn_container(
        move || !result_tabs.with(|v| v.is_empty()),
        move |multi| {
            if multi {
                results_multi(result_tabs, active_result, cancel.clone(), gctx.clone()).into_any()
            } else {
                results_view(results, cancel.clone(), gctx.clone()).into_any()
            }
        },
    )
    .style(|s| {
        s.flex_grow(1.0_f32)
            .width_full()
            .flex_col()
            .min_height(0.0)
            .min_width(0.0)
    });

    let panel = v_stack((section_title("RESULTS"), body)).style(|s| {
        s.width_full()
            .flex_grow(1.0_f32)
            .flex_col()
            .min_height(0.0)
            .min_width(0.0)
            .background(theme::bg_results())
    });
    // Overlay the find bar at the panel's top edge + the commit-error bar at the
    // bottom (a `stack` anchors the absolute bars to the panel).
    stack((
        panel,
        grid_find_bar(
            find_open, find_query, find_step, find_total, find_pos, find_more,
        ),
        grid_error_bar(commit_err, error_open, error_text),
    ))
    .style(|s| {
        s.width_full()
            .flex_grow(1.0_f32)
            .flex_col()
            .min_height(0.0)
            .min_width(0.0)
    })
}

// Run Everything results: a result-tab strip over the active statement's grid.
fn results_multi(
    result_tabs: RwSignal<Vec<ResultPanel>>,
    active_result: RwSignal<usize>,
    cancel: Rc<dyn Fn()>,
    gctx: GridCtx,
) -> impl IntoView {
    let body = dyn_container(
        move || {
            let ai = active_result.get();
            result_tabs.with(|v| v.get(ai).or_else(|| v.first()).map(|p| p.state.clone()))
        },
        move |state| match state {
            None => empty().into_any(),
            Some(QueryState::Idle) => empty().into_any(),
            Some(QueryState::Running) => running_view(cancel.clone()).into_any(),
            // Unlike single runs (whose error shows in the editor bar), a batch
            // statement's error has nowhere else to go, so show it here.
            Some(QueryState::Failed(m)) => centered_msg(m, theme::reject_text()).into_any(),
            Some(QueryState::Cancelled) => {
                centered_msg("Query cancelled.", theme::text_dim()).into_any()
            }
            Some(QueryState::Loaded(rs)) => loaded_view(rs, gctx.clone()),
        },
    )
    .style(|s| {
        s.flex_grow(1.0_f32)
            .width_full()
            .flex_col()
            .min_height(0.0)
            .min_width(0.0)
    });

    v_stack((result_tab_strip(result_tabs, active_result), body)).style(|s| {
        s.flex_grow(1.0_f32)
            .width_full()
            .flex_col()
            .min_height(0.0)
            .min_width(0.0)
    })
}

// Result-tab strip (Run Everything): one chip per statement, click to switch.
// Borrows the query tab bar's look (top-rounded, oversize-and-clip); no close /
// "+" since the tabs are regenerated on each run.
fn result_tab_strip(
    result_tabs: RwSignal<Vec<ResultPanel>>,
    active_result: RwSignal<usize>,
) -> impl IntoView {
    let chips = dyn_stack(
        move || {
            result_tabs
                .get()
                .into_iter()
                .enumerate()
                .collect::<Vec<_>>()
        },
        |(i, p): &(usize, ResultPanel)| (*i, p.label.clone()),
        move |(i, panel)| result_tab_chip(i, panel.label, result_tabs, active_result),
    )
    .style(|s| s.flex_row().height_full());

    // Chips pan horizontally on the plain wheel (no visible bars) so overflowed
    // result tabs stay reachable — same treatment as the query strip.
    let scroller =
        wheel_hscroll(chips).style(|s| s.flex_shrink(1.0_f32).min_width(0.0).height_full());

    // Flat, full-height result tabs. Unlike the query strip, this one adds a
    // full-width **top** separator too (the query strip sits below the header,
    // which already provides one).
    h_stack((scroller,)).style(|s| {
        s.width_full()
            .flex_row()
            .height(TAB_BAR_H)
            .min_height(TAB_BAR_H)
            .flex_shrink(0.0_f32)
            .background(theme::bg_chrome())
            .border_top(1.0)
            .border_bottom(1.0)
            .border_color(theme::border())
    })
}

fn result_tab_chip(
    idx: usize,
    label: String,
    result_tabs: RwSignal<Vec<ResultPanel>>,
    active_result: RwSignal<usize>,
) -> impl IntoView {
    // State is read reactively (the chip isn't rebuilt when only state changes,
    // since it's keyed by label): a failed statement's tab tints red.
    let is_err = move || {
        result_tabs.with(|v| matches!(v.get(idx).map(|p| &p.state), Some(QueryState::Failed(_))))
    };
    // Colour is set on the tab container and cascades to the label.
    container(text(label).style(|s| s.margin_horiz(10.0).font_size(theme::FONT_BODY)))
        .on_click_stop(move |_| active_result.set(idx))
        .style(move |s| {
            let s = s
                .flex_row()
                .items_center()
                .border_right(1.0)
                .border_color(theme::tab_separator());
            let s = if active_result.get() == idx {
                s.background(theme::tab_active())
            } else {
                s.background(theme::bg_chrome())
            };
            if is_err() {
                s.color(theme::reject_text())
            } else if active_result.get() == idx {
                s.color(theme::text())
            } else {
                s.color(theme::tab_text()).hover(|s| s.color(theme::text()))
            }
        })
}

// ── Terminal panel ───────────────────────────────────────────────────────────
//
// Renders the [`Screen`] snapshot as rows of coalesced, colored monospace runs.
// Columns align because the font is monospace; the per-size cell metrics
// (`term_cell_wh`) only drive how many cols/rows we ask the PTY for.

fn term_color(c: (u8, u8, u8)) -> floem::peniko::Color {
    floem::peniko::Color::rgb8(c.0, c.1, c.2)
}

// Encode a key event into the bytes a PTY expects. `None` = not forwarded.
fn encode_key(ke: &floem::keyboard::KeyEvent) -> Option<Vec<u8>> {
    let mods = ke.modifiers;
    let ctrl = mods.control() || mods.meta();
    match &ke.key.logical_key {
        Key::Character(s) => {
            if ctrl
                && !mods.alt()
                && let Some(c) = s.chars().next()
            {
                let up = c.to_ascii_uppercase();
                if up.is_ascii_alphabetic() {
                    return Some(vec![(up as u8) & 0x1f]);
                }
                match c {
                    ' ' => return Some(vec![0]),
                    '[' => return Some(vec![0x1b]),
                    ']' => return Some(vec![0x1d]),
                    '\\' => return Some(vec![0x1c]),
                    _ => {}
                }
            }
            let mut bytes = s.as_bytes().to_vec();
            // Alt prefixes ESC (Meta) — but NOT AltGr (Ctrl+Alt), which produces a
            // real character (e.g. `@` on a German layout); prefixing ESC there
            // would send `ESC @` (§7.3).
            if mods.alt() && !mods.control() {
                let mut v = vec![0x1b];
                v.append(&mut bytes);
                bytes = v;
            }
            Some(bytes)
        }
        Key::Named(named) => {
            // xterm modifier code: 1 + shift + 2·alt + 4·ctrl. When any modifier is
            // held, cursor/edit keys take the `ESC [ 1 ; <mod> <final>` form so the
            // shell sees Ctrl+←/→ (word-jump), Shift+arrows, etc.
            let modcode = 1 + (mods.shift() as u8) + 2 * (mods.alt() as u8) + 4 * (ctrl as u8);
            let csi_final = |fin: char| -> Vec<u8> {
                if modcode != 1 {
                    format!("\x1b[1;{modcode}{fin}").into_bytes()
                } else {
                    format!("\x1b[{fin}").into_bytes()
                }
            };
            let csi_tilde = |n: u8| -> Vec<u8> {
                if modcode != 1 {
                    format!("\x1b[{n};{modcode}~").into_bytes()
                } else {
                    format!("\x1b[{n}~").into_bytes()
                }
            };
            // F1-F4 use SS3; F5-F12 use CSI numbers (no modifier form — rarely used).
            let fkey = |n: u8| -> Vec<u8> {
                match n {
                    1 => b"\x1bOP".to_vec(),
                    2 => b"\x1bOQ".to_vec(),
                    3 => b"\x1bOR".to_vec(),
                    4 => b"\x1bOS".to_vec(),
                    5 => b"\x1b[15~".to_vec(),
                    6 => b"\x1b[17~".to_vec(),
                    7 => b"\x1b[18~".to_vec(),
                    8 => b"\x1b[19~".to_vec(),
                    9 => b"\x1b[20~".to_vec(),
                    10 => b"\x1b[21~".to_vec(),
                    11 => b"\x1b[23~".to_vec(),
                    12 => b"\x1b[24~".to_vec(),
                    _ => Vec::new(),
                }
            };
            let seq: Vec<u8> = match named {
                NamedKey::Enter => b"\r".to_vec(),
                NamedKey::Backspace => b"\x7f".to_vec(),
                NamedKey::Tab if mods.shift() => b"\x1b[Z".to_vec(),
                NamedKey::Tab => b"\t".to_vec(),
                NamedKey::Escape => b"\x1b".to_vec(),
                NamedKey::ArrowUp => csi_final('A'),
                NamedKey::ArrowDown => csi_final('B'),
                NamedKey::ArrowRight => csi_final('C'),
                NamedKey::ArrowLeft => csi_final('D'),
                NamedKey::Home => csi_final('H'),
                NamedKey::End => csi_final('F'),
                NamedKey::PageUp => csi_tilde(5),
                NamedKey::PageDown => csi_tilde(6),
                NamedKey::Delete => csi_tilde(3),
                NamedKey::Insert => csi_tilde(2),
                NamedKey::Space if ctrl && !mods.alt() => vec![0],
                NamedKey::Space => b" ".to_vec(),
                NamedKey::F1 => fkey(1),
                NamedKey::F2 => fkey(2),
                NamedKey::F3 => fkey(3),
                NamedKey::F4 => fkey(4),
                NamedKey::F5 => fkey(5),
                NamedKey::F6 => fkey(6),
                NamedKey::F7 => fkey(7),
                NamedKey::F8 => fkey(8),
                NamedKey::F9 => fkey(9),
                NamedKey::F10 => fkey(10),
                NamedKey::F11 => fkey(11),
                NamedKey::F12 => fkey(12),
                _ => return None,
            };
            Some(seq)
        }
        _ => None,
    }
}

// Build the grid views from a snapshot. `font` is the current terminal font size
// (drives glyph size + row height); `open_link` opens clicked URLs.
fn terminal_grid(scr: Screen, font: u16, open_link: Rc<dyn Fn(String)>) -> impl IntoView {
    let fsz = font as f32;
    let (_, cell_h) = term_cell_wh(font);
    let rows = scr.rows.into_iter().map(move |row| {
        let open_link = open_link.clone();
        let runs = row.runs.into_iter().map(move |run| {
            let fg = term_color(run.fg);
            let bg = run.bg.map(term_color);
            let bold = run.bold;
            let link = run.link.clone();
            let is_link = link.is_some();
            let styled = text(run.text).style(move |s| {
                let color = if is_link { theme::accent() } else { fg };
                let s = s
                    .font_family("monospace".to_string())
                    .font_size(fsz)
                    .line_height(1.0)
                    .color(color);
                let s = if bold { s.font_bold() } else { s };
                let s = if is_link {
                    s.cursor(CursorStyle::Pointer)
                } else {
                    s
                };
                match bg {
                    Some(c) => s.background(c),
                    None => s,
                }
            });
            match link {
                Some(url) => {
                    let open_link = open_link.clone();
                    styled
                        .on_click_stop(move |_| (open_link)(url.clone()))
                        .into_any()
                }
                None => styled.into_any(),
            }
        });
        h_stack_from_iter(runs).style(move |s| {
            s.flex_row()
                .height(cell_h)
                .min_height(cell_h)
                .items_center()
        })
    });
    v_stack_from_iter(rows).style(|s| s.flex_col().min_width(0.0))
}

fn terminal_panel(ui: Ui) -> impl IntoView {
    let screen = ui.term.screen;
    let focused = ui.term.focused;
    let input = ui.term_actions.input.clone();
    let resize = ui.term_actions.resize.clone();
    let scroll = ui.term_actions.scroll.clone();
    let settings_open = ui.term.settings_open;
    let sel_start = ui.term_actions.sel_start.clone();
    let sel_update = ui.term_actions.sel_update.clone();
    let sel_clear = ui.term_actions.sel_clear.clone();
    let copy = ui.term_actions.copy.clone();
    let paste = ui.term_actions.paste.clone();
    let open_link = ui.term_actions.open_link.clone();
    let restart = ui.term_actions.restart.clone();
    let scroll_bottom = ui.term_actions.scroll_bottom.clone();
    let open_cli = ui.tab_actions.open_db_cli.clone();
    let font_size = ui.term.font_size;
    let copy_on_select = ui.term.copy_on_select;
    let cursor_style = ui.term.cursor_style;

    // Custom scrollback scrollbar state (the terminal isn't a Floem scroll): a
    // `shown` flag toggled by scroll activity, hidden 3s after it stops.
    let (bar_shown, bar_poke) = autohide_state();
    let bar_poke_wheel = bar_poke.clone();

    // Title row: "TERMINAL" left; open-DB-CLI + restart + settings gear right,
    // each 10px apart (gear 12px from the edge), matching the AI panel's spacing.
    let db_cli_btn = toolbar_icon(icons::DATABASE, 5.0, 2.0, || true, move || (open_cli)(None));
    let restart_btn = toolbar_icon(icons::REFRESH_CW, 5.0, 2.0, || true, move || (restart)());
    let gear = toolbar_icon(
        icons::SLIDERS_VERTICAL,
        5.0,
        7.0,
        || true,
        move || settings_open.set(true),
    );
    let icons_group = h_stack((db_cli_btn, restart_btn, gear))
        .style(|s| s.flex_row().items_start().flex_shrink(0.0_f32));
    let title_row = h_stack((section_title("TERMINAL"), icons_group))
        .style(|s| s.width_full().flex_row().items_start().justify_between());

    // Rebuilds on a snapshot change or a font-size change (the latter re-lays
    // every row at the new glyph size).
    let grid = dyn_container(
        move || (screen.get(), font_size.get()),
        move |(scr, font)| terminal_grid(scr, font, open_link.clone()).into_any(),
    )
    .style(|s| s.flex_col().min_width(0.0).min_height(0.0));

    // Tracks the last (cols,rows) we reported so the fit effect doesn't spam, the
    // last surface rect (so a font-size change can re-fit without a resize event),
    // plus mouse-drag selection state.
    let last_dims: RwSignal<(u16, u16)> = RwSignal::new((0, 0));
    let view_rect: RwSignal<(f64, f64)> = RwSignal::new((0.0, 0.0));
    let dragging = RwSignal::new(false);
    let moved = RwSignal::new(false);

    // Map a surface-local pixel point to a viewport (row, col), clamped.
    let cell_at = move |x: f64, y: f64| -> (usize, usize) {
        let (cols, rows) = last_dims.get_untracked();
        let (cw, ch) = term_cell_wh(font_size.get_untracked());
        let cx = ((x - 6.0).max(0.0) / cw) as usize;
        let cy = ((y - 6.0).max(0.0) / ch) as usize;
        (
            cy.min(rows.max(1) as usize - 1),
            cx.min(cols.max(1) as usize - 1),
        )
    };

    // Fit the PTY to the surface: recompute cols/rows whenever the surface
    // resizes OR the font size changes, then resize the grid if they moved.
    create_effect(move |_| {
        let (w, h) = view_rect.get();
        let (cw, ch) = term_cell_wh(font_size.get());
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let cols = ((w - 12.0) / cw).floor().max(1.0) as u16;
        let rows = ((h - 12.0) / ch).floor().max(1.0) as u16;
        if last_dims.get_untracked() != (cols, rows) {
            last_dims.set((cols, rows));
            (resize)(cols, rows);
        }
    });

    // Clones for the various handlers.
    let (copy_key, copy_ctx, copy_sel) = (copy.clone(), copy.clone(), copy);
    let (paste_key, paste_ctx) = (paste.clone(), paste);
    let sel_clear_up = sel_clear.clone();

    let surface = shift_hscroll(grid)
        .style(|s| {
            s.flex_grow(1.0_f32)
                .width_full()
                .min_height(0.0)
                .min_width(0.0)
                .padding(6.0)
                .background(term_color(schemaic_term::DEFAULT_BG))
                .cursor(CursorStyle::Text)
        })
        .keyboard_navigable()
        .on_event(EventListener::FocusGained, move |_| {
            focused.set(true);
            EventPropagation::Continue
        })
        .on_event(EventListener::FocusLost, move |_| {
            focused.set(false);
            EventPropagation::Continue
        })
        .on_event(EventListener::KeyDown, move |e| {
            if let Event::KeyDown(ke) = e {
                let m = ke.modifiers;
                // Ctrl+Shift+C / Ctrl+Shift+V for copy / paste.
                if (m.control() || m.meta())
                    && m.shift()
                    && let Key::Character(s) = &ke.key.logical_key
                {
                    match s.as_str() {
                        "c" | "C" => {
                            if let Some(t) = (copy_key)() {
                                let _ = floem::Clipboard::set_contents(t);
                            }
                            return EventPropagation::Stop;
                        }
                        "v" | "V" => {
                            if let Ok(t) = floem::Clipboard::get_contents() {
                                (paste_key)(t);
                            }
                            return EventPropagation::Stop;
                        }
                        _ => {}
                    }
                }
                if let Some(bytes) = encode_key(ke) {
                    (input)(bytes);
                    return EventPropagation::Stop;
                }
            }
            EventPropagation::Continue
        })
        .on_event(EventListener::PointerWheel, move |e| {
            if let Event::PointerWheel(pe) = e {
                let dy = pe.delta.y;
                if dy.abs() > 0.0 {
                    let lines = if dy < 0.0 { 3 } else { -3 };
                    (scroll)(lines);
                    (bar_poke_wheel)();
                    return EventPropagation::Stop;
                }
            }
            EventPropagation::Continue
        })
        .on_event(EventListener::PointerDown, move |e| {
            if let Event::PointerDown(pe) = e
                && pe.button.is_primary()
            {
                let (r, c) = cell_at(pe.pos.x, pe.pos.y);
                (sel_start)(r, c);
                dragging.set(true);
                moved.set(false);
            }
            EventPropagation::Continue
        })
        .on_event(EventListener::PointerMove, move |e| {
            if dragging.get_untracked()
                && let Event::PointerMove(pe) = e
            {
                let (r, c) = cell_at(pe.pos.x, pe.pos.y);
                moved.set(true);
                (sel_update)(r, c);
            }
            EventPropagation::Continue
        })
        .on_event(EventListener::PointerUp, move |_| {
            if dragging.get_untracked() {
                dragging.set(false);
                if moved.get_untracked() {
                    // Copy-on-select: mirror the finished selection to the
                    // clipboard (keep it highlighted so the user sees what stuck).
                    if copy_on_select.get_untracked()
                        && let Some(t) = (copy_sel)()
                    {
                        let _ = floem::Clipboard::set_contents(t);
                    }
                } else {
                    (sel_clear_up)();
                }
            }
            EventPropagation::Continue
        })
        // Right-click: copy the selection if any, else paste.
        .on_secondary_click_stop(move |_| {
            if let Some(t) = (copy_ctx)() {
                let _ = floem::Clipboard::set_contents(t);
                (sel_clear)();
            } else if let Ok(t) = floem::Clipboard::get_contents() {
                (paste_ctx)(t);
            }
        })
        .on_resize(move |rect| {
            // Just record the size; the fit effect above turns it (+ the font
            // size) into cols/rows so a font change re-fits without a resize.
            let wh = (rect.width(), rect.height());
            if view_rect.get_untracked() != wh {
                view_rect.set(wh);
            }
        });

    // Jump-to-bottom: shown while scrolled up into history (display_offset > 0).
    let jump = jump_to_bottom_button(
        move || screen.get().display_offset > 0,
        move || (scroll_bottom)(),
    );

    // Custom scrollback scrollbar (the terminal isn't a Floem scroll): a thumb on
    // the right whose size/position reflect the viewport within the total
    // scrollback. Read-only indicator (no drag); auto-hides via `bar_shown`.
    let scrollbar = empty()
        .style(move |s| {
            let sc = screen.get();
            let vr = sc.rows.len().max(1);
            let total = sc.total_lines.max(vr);
            if total <= vr || !bar_shown.get() {
                return s.hide();
            }
            let (_, cell_h) = term_cell_wh(font_size.get());
            let track_h = vr as f64 * cell_h;
            let thumb_h = thumb_len((vr as f64 / total as f64) * track_h, track_h);
            // ratio: 1.0 at the live bottom (offset 0), 0.0 at the top of history.
            let ratio = (total - vr - sc.display_offset) as f64 / (total - vr) as f64;
            let top = 6.0 + ratio * (track_h - thumb_h);
            s.absolute()
                .inset_right(3.0)
                .inset_top(top)
                .width(6.0)
                .height(thumb_h)
                .border_radius(3.0)
                .background(theme::scrollbar())
        })
        .pointer_events(|| false);

    // Bar / underline cursor: a thin overlay at the reported cursor cell. (Block
    // is baked into the snapshot in `schemaic-term`.) `screen.cursor` is already
    // `None` when the cursor is hidden or blinked off, so this follows blink for
    // free. Positions match the grid: the surface pads 6px, cells are cw×ch.
    let cursor_overlay = empty()
        .style(move |s| {
            let sc = screen.get();
            let (cw, ch) = term_cell_wh(font_size.get());
            let color = term_color(schemaic_term::CURSOR);
            match (cursor_style.get(), sc.cursor) {
                (TermCursor::Bar, Some((r, c))) => s
                    .absolute()
                    .inset_left(6.0 + c as f64 * cw)
                    .inset_top(6.0 + r as f64 * ch)
                    .width(2.0)
                    .height(ch)
                    .background(color),
                (TermCursor::Underline, Some((r, c))) => s
                    .absolute()
                    .inset_left(6.0 + c as f64 * cw)
                    .inset_top(6.0 + r as f64 * ch + ch - 2.0)
                    .width(cw)
                    .height(2.0)
                    .background(color),
                _ => s.hide(),
            }
        })
        .pointer_events(|| false);

    let body = stack((surface, scrollbar, jump, cursor_overlay)).style(|s| {
        s.flex_col()
            .flex_grow(1.0_f32)
            .width_full()
            .min_height(0.0)
            .min_width(0.0)
    });

    let right_w = ui.layout.right_w;
    v_stack((title_row, body)).style(move |s| {
        s.width(right_w.get())
            .flex_shrink(0.0_f32)
            .height_full()
            .min_height(0.0)
            .flex_col()
            .background(theme::bg_panel())
            .border_left(1.0)
            .border_color(theme::border())
    })
}

/// A `fn`-pointer transparent background for [`FieldCfg::background`] (the
/// Ctrl+K field, whose surface is owned by an animated outer container).
pub(crate) fn bg_transparent() -> floem::peniko::Color {
    floem::peniko::Color::TRANSPARENT
}

/// Config for [`edit_field`], the app's shared editor-backed input.
pub(crate) struct FieldCfg {
    pub placeholder: &'static str,
    /// Box background. A fn (not a `Color`) so it's re-read inside the field's
    /// reactive style — the surface then follows a live theme switch instead of
    /// freezing the colour captured when the field was first built.
    pub background: fn() -> floem::peniko::Color,
    /// Wrap + auto-grow to `CHAT_MAX_ROWS` then scroll (the AI chat box).
    /// Otherwise a single line: no wrap, Enter submits, horizontal scroll with
    /// no visible bar (the caret stays in view like a normal OS field).
    pub multiline: bool,
    /// Show a trailing × that empties the value (single-line filters).
    pub clearable: bool,
    /// Grab focus on mount (e.g. the Find palette).
    pub autofocus: bool,
    pub font_size: f32,
    pub border_radius: f32,
    /// Read-only: no text edits (still handles Enter/Escape). Suppresses autofocus.
    pub read_only: bool,
    /// Fixed box height. `None` = derive from content (auto-grow for multiline).
    pub height: Option<f64>,
    /// Reactive override for the multiline auto-grow cap (rows). `None` =
    /// `CHAT_MAX_ROWS`. A signal so the cap can follow a resizing container (the
    /// value viewer caps at the results-panel height).
    pub max_rows: Option<RwSignal<usize>>,
    /// Override the text colour (`None` = `theme::text`). A `fn` (not a `Color`)
    /// so it's re-read inside the reactive style — follows a live theme switch
    /// instead of freezing the colour captured at build (§7.4, matches `background`).
    pub text_color: Option<fn() -> floem::peniko::Color>,
    /// Override the placeholder colour (`None` = `theme::placeholder`). `fn` for
    /// live theme switching, as `text_color`.
    pub placeholder_color: Option<fn() -> floem::peniko::Color>,
    /// Fixed border colour for both focus states (`None` = the focus-driven
    /// `field_border` / `field_border_active`). `fn` for live theme switching.
    pub border_color: Option<fn() -> floem::peniko::Color>,
    /// Enter (single-line) / plain Enter (multiline).
    pub on_submit: Option<Rc<dyn Fn()>>,
    /// Escape key (e.g. close an overlay).
    pub on_escape: Option<Rc<dyn Fn()>>,
    /// Focus lost — the field was blurred (clicking elsewhere, Tab-ing away). Not
    /// fired on the initial build. Used by the inline tab-rename to commit on
    /// click-away.
    pub on_blur: Option<Rc<dyn Fn()>>,
    /// Arrow Up / Down (e.g. move the selection in a command-palette list). When
    /// set, the key is consumed here instead of moving the editor caret.
    pub on_arrow_up: Option<Rc<dyn Fn()>>,
    pub on_arrow_down: Option<Rc<dyn Fn()>>,
    /// A trailing action rendered INSIDE the field, right-aligned (same spot as
    /// the clearable ×) — e.g. the AI-panel send/stop icon. A factory so the view
    /// is built inside the field.
    pub trailing: Option<Rc<dyn Fn() -> AnyView>>,
}

impl Default for FieldCfg {
    fn default() -> Self {
        FieldCfg {
            placeholder: "",
            background: theme::bg_deepest,
            multiline: false,
            clearable: false,
            autofocus: false,
            font_size: 13.0,
            border_radius: 6.0,
            read_only: false,
            height: None,
            max_rows: None,
            text_color: None,
            placeholder_color: None,
            border_color: None,
            on_submit: None,
            on_escape: None,
            on_blur: None,
            on_arrow_up: None,
            on_arrow_down: None,
            trailing: None,
        }
    }
}

/// Length (bytes, rounded down to a char boundary) of the common prefix of two
/// strings — for mapping the caret across a signal→doc reconcile.
fn common_prefix_len(a: &str, b: &str) -> usize {
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    let max = ab.len().min(bb.len());
    let mut i = 0;
    while i < max && ab[i] == bb[i] {
        i += 1;
    }
    while i > 0 && !a.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Length (bytes, char-boundary-aligned) of the common suffix of two strings,
/// not overlapping the shared prefix `floor`.
fn common_suffix_len(a: &str, b: &str, floor: usize) -> usize {
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    let max = a
        .len()
        .saturating_sub(floor)
        .min(b.len().saturating_sub(floor));
    let mut i = 0;
    while i < max && ab[a.len() - 1 - i] == bb[b.len() - 1 - i] {
        i += 1;
    }
    while i > 0 && !a.is_char_boundary(a.len() - i) {
        i -= 1;
    }
    i
}

/// The one text-input component used across the app (except the specialised
/// Ctrl+K overlay and the `*`-masked password fields): Floem's editor engine —
/// the same one that powers the SQL editor — configured as a plain field inside
/// a bordered box that owns the surface. Every field gets real text editing
/// (mouse, drag-select, clipboard), a placeholder shown only while empty AND
/// unfocused, a focus border, and IBM Plex styling. `multiline` fields wrap and
/// auto-grow; single-line fields don't wrap and scroll to the caret with no
/// visible bar. Callers add their own outer layout via `.style`.
pub(crate) fn edit_field(text_sig: RwSignal<String>, cfg: FieldCfg) -> impl IntoView {
    let FieldCfg {
        placeholder,
        background,
        multiline,
        clearable,
        autofocus,
        font_size,
        border_radius,
        read_only,
        height,
        max_rows,
        text_color,
        placeholder_color,
        border_color,
        on_submit,
        on_escape,
        on_blur,
        on_arrow_up,
        on_arrow_down,
        trailing,
    } = cfg;
    // An in-flow trailing action (like the clearable ×) shrinks the editor.
    let has_side = clearable || trailing.is_some();
    // Line height derived from the font so the box height matches the rendered
    // text (≈1.46× the app's body rhythm: 13→19, 16→23).
    let line_h = (font_size as f64 * 1.46).round();
    // Keep as `fn`s (not resolved Colors) so the style closures below can call
    // them and follow a live theme switch (§7.4).
    let text_color: fn() -> floem::peniko::Color = text_color.unwrap_or(theme::text);
    let placeholder_color: fn() -> floem::peniko::Color =
        placeholder_color.unwrap_or(theme::placeholder);
    // With a fixed height, centre the single line vertically; otherwise use the
    // standard vertical padding and let the height follow the content.
    let pad_v = match height {
        Some(hh) => ((hh - line_h) / 2.0 - 2.0).max(2.0),
        None => CHAT_PAD_V,
    };
    let cap = if multiline { CHAT_MAX_ROWS } else { 1 };
    let wrap = if multiline {
        WrapMethod::EditorWidth
    } else {
        WrapMethod::None
    };

    let focused = RwSignal::new(false);
    // Visual (wrapped) line count → drives the box height (clamped to `cap`).
    let rows = RwSignal::new(1usize);

    let submit = on_submit.clone();
    let escape = on_escape.clone();
    let arrow_up = on_arrow_up.clone();
    let arrow_down = on_arrow_down.clone();
    let editor = text_editor_keys(text_sig.get_untracked(), move |editor_sig, kp, mods| {
        if let Some(esc) = &escape
            && matches!(kp.key, KeyInput::Keyboard(Key::Named(NamedKey::Escape), _))
        {
            (esc)();
            return CommandExecuted::Yes;
        }
        // Arrow Up/Down drive an external list (command-palette nav) instead of
        // the caret, when the caller opted in.
        if let Some(cb) = &arrow_up
            && matches!(kp.key, KeyInput::Keyboard(Key::Named(NamedKey::ArrowUp), _))
        {
            (cb)();
            return CommandExecuted::Yes;
        }
        if let Some(cb) = &arrow_down
            && matches!(
                kp.key,
                KeyInput::Keyboard(Key::Named(NamedKey::ArrowDown), _)
            )
        {
            (cb)();
            return CommandExecuted::Yes;
        }
        if matches!(kp.key, KeyInput::Keyboard(Key::Named(NamedKey::Enter), _)) {
            if multiline {
                // Plain Enter submits; Shift/Ctrl+Enter fall through → newline.
                if !mods.shift() && !mods.control() {
                    if let Some(cb) = &submit {
                        (cb)();
                    }
                    return CommandExecuted::Yes;
                }
            } else {
                // Single line never inserts a newline; Enter just submits.
                if let Some(cb) = &submit {
                    (cb)();
                }
                return CommandExecuted::Yes;
            }
        }
        default_key_handler(editor_sig)(kp, mods)
    });
    let ed = editor.editor().clone();
    let editor = if read_only {
        editor.read_only()
    } else {
        editor
    };

    // Plain styling in the app's body font, with an explicit line height so the
    // box-height math below matches the rendered lines.
    let styling = {
        // NB: `SimpleStyling`'s wrap is dropped in `build()` — wrapping is
        // controlled by `wrap_method` on `editor_style` below, not here.
        let mut b = SimpleStyling::builder();
        b.font_size(font_size as usize)
            .line_height(line_h as f32)
            .font_family(vec![FamilyOwned::Name("IBM Plex Sans".to_string())]);
        b.build()
    };

    // doc → signal: mirror the editor text into `text_sig` and recompute the
    // grown height. Single-line fields strip any pasted newlines.
    let ed_upd = ed.clone();
    let editor = editor
        .styling(styling)
        // NB: not the editor's built-in `.placeholder()` — it stays visible while
        // focused-but-empty. A custom overlay (below) shows it only when empty
        // AND unfocused.
        .editor_style(move |s| {
            default_dark_color(s)
                .hide_gutter(true)
                .cursor_color(theme::accent())
                .selection_color(theme::accent().multiply_alpha(0.30))
                .current_line_color(floem::peniko::Color::TRANSPARENT)
                // Single-line: no wrap → text runs on and scrolls horizontally
                // (like a normal input). Multiline: wrap to the box width.
                .wrap_method(wrap)
                // Don't reserve a trailing blank screenful — otherwise a
                // scrollbar shows even when the text fits.
                .scroll_beyond_last_line(false)
        })
        .update(move |_| {
            let mut t = ed_upd.doc().text().to_string();
            if !multiline && (t.contains('\n') || t.contains('\r')) {
                t = t.replace(['\n', '\r'], "");
            }
            if text_sig.get_untracked() != t {
                text_sig.set(t);
            }
            // Store the natural (unclamped) line count; the height clamps it to the
            // effective cap so a resizing cap (the viewer) re-clamps reactively.
            rows.set(ed_upd.last_vline().get() + 1);
        })
        .style(move |s| {
            // The box (below) owns the border/background; the editor is
            // transparent and fills it.
            let s = s
                .height_full()
                .min_width(0.0)
                .color(text_color())
                .background(floem::peniko::Color::TRANSPARENT)
                .class(Handle, move |s| {
                    if multiline {
                        // The chat box shows a thin bar past the row cap.
                        s.set(Thickness, Px(6.0))
                            .set(Rounded, true)
                            .background(theme::scrollbar())
                    } else {
                        // Single-line scrolls to the caret with NO scrollbar at
                        // all (0 thickness → nothing to show, even on hover).
                        s.set(Thickness, Px(0.0))
                            .background(floem::peniko::Color::TRANSPARENT)
                    }
                });
            // A side control (clearable × or a trailing action) sits in-flow, so
            // the editor flex-grows beside it; otherwise it fills the box.
            if has_side {
                s.flex_grow(1.0_f32)
            } else {
                s.width_full()
            }
        });
    // Autofocus: focus the editor's own view id, deferred a frame so it exists
    // (a `request_focus` on the outer view doesn't reach the editor).
    // `try_get_untracked` — the field may be disposed before this timer fires
    // (e.g. an overlay opened then closed in the same tick), and a plain
    // `get_untracked` on a disposed signal panics.
    // `read_only` suppresses autofocus (per the `FieldCfg` doc): a read-only
    // field — e.g. the value viewer — should never steal focus on mount.
    if autofocus && !read_only {
        let ed_af = ed.clone();
        floem::action::exec_after(std::time::Duration::from_millis(0), move |_| {
            if let Some(Some(vid)) = ed_af.editor_view_id.try_get_untracked() {
                vid.request_focus();
                // Land the caret at the end of any seeded text — a programmatic
                // focus on a prefilled field (e.g. the inline tab rename) should
                // sit after the text, not before it. (Empty fields: end == 0.)
                let len = ed_af.doc().text().to_string().len();
                ed_af.cursor.update(|c| c.set_offset(len, false, false));
            }
        });
    }

    // signal → doc: reconcile the doc when the signal differs (external clears,
    // the × button, or loading a saved value). UNTRACKED doc read so this fires
    // only on signal changes, never per-keystroke (which would fight the caret).
    {
        let ed_ext = ed.clone();
        create_effect(move |_| {
            let want = text_sig.get();
            let have = untrack(|| ed_ext.doc().text().to_string());
            if want != have {
                let len = have.len();
                // Preserve the caret across the reconcile instead of pinning it to
                // the end — a masked password field re-masks on every keystroke,
                // which fires this effect, so end-pinning made mid-string edits
                // jump the caret to the end (§7.4). Map the old offset through the
                // prefix/suffix common to `have` and `want`.
                let cur = untrack(|| ed_ext.cursor.get_untracked().offset()).min(len);
                let cp = common_prefix_len(&have, &want);
                let cs = common_suffix_len(&have, &want, cp);
                let new_off = if cur <= cp {
                    cur // before the change — unaffected
                } else if cur >= len.saturating_sub(cs) {
                    // in/after the changed tail — shift by the length delta
                    (cur + want.len()).saturating_sub(len).max(cp)
                } else {
                    // inside the replaced region — land at its new end
                    want.len().saturating_sub(cs)
                }
                .min(want.len());
                ed_ext
                    .doc()
                    .edit_single(Selection::region(0, len), &want, EditType::Delete);
                // Only move the caret when the field actually has focus. An
                // unfocused field reconciling an external value change (loading a
                // connection, New/clear) must NOT touch `cursor`: floem resets the
                // caret's blink on any cursor change (editor/mod.rs), which makes an
                // unfocused field's caret appear — looking exactly like focus. The
                // stale offset is harmless (floem's offset→line math clamps it), and
                // a real click sets a fresh caret position anyway.
                if focused.get_untracked() {
                    ed_ext
                        .cursor
                        .update(|c| c.set_offset(new_off, false, false));
                }
            }
        });
    }

    // Recompute the wrapped line count when the editor lays out / resizes. The
    // `.update` above only fires on edits, so multiline text set *programmatically*
    // (the value viewer) would otherwise be measured at zero width → one line, and
    // never learn its true wrapped height. Tracking `viewport` catches first layout
    // and width changes.
    if multiline {
        let ed_rows = ed.clone();
        create_effect(move |_| {
            ed_rows.viewport.track();
            let n = ed_rows.last_vline().get() + 1;
            if rows.get_untracked() != n {
                rows.set(n);
            }
        });
    }

    // Caret focus-gating + border focus tracking. The focus-lost effect is
    // created second so it wins the initial run → the field starts unfocused
    // (unless `autofocus` re-focuses it right after).
    {
        let ed_focus = ed.clone();
        create_effect(move |_| {
            ed_focus.editor_view_focused.track();
            focused.set(true);
            // A read-only field can still be focused (to receive Enter/Escape),
            // but shows no blinking caret.
            if read_only {
                ed_focus.cursor_info.hidden.set(true);
                ed_focus
                    .cursor_info
                    .blink_timer
                    .set(floem::action::TimerToken::INVALID);
            } else {
                ed_focus.cursor_info.reset();
            }
        });
        let ed_blur = ed.clone();
        let blur_cb = on_blur.clone();
        create_effect(move |prev: Option<()>| {
            ed_blur.editor_view_focus_lost.track();
            focused.set(false);
            ed_blur.cursor_info.hidden.set(true);
            ed_blur
                .cursor_info
                .blink_timer
                .set(floem::action::TimerToken::INVALID);
            // Skip the initial effect run (`prev` is `None` only then) — it's
            // establishing tracking, not a real blur — so callers don't get a
            // spurious focus-lost on mount.
            if prev.is_some()
                && let Some(cb) = &blur_cb
            {
                (cb)();
            }
        });
    }

    // Placeholder overlay: shown only when EMPTY *and* unfocused, positioned over
    // where the first line of text renders.
    let ph_top = pad_v + (line_h - font_size as f64) / 2.0;
    let placeholder = dyn_container(
        move || text_sig.with(|t| t.is_empty()) && !focused.get(),
        move |show| {
            if show {
                text(placeholder)
                    .style(move |s| {
                        s.font_size(font_size)
                            .font_family("IBM Plex Sans".to_string())
                            .color(placeholder_color())
                    })
                    .into_any()
            } else {
                empty().into_any()
            }
        },
    )
    .style(move |s| s.absolute().inset_left(CHAT_PAD_H).inset_top(ph_top))
    // Let clicks fall through to the editor beneath — otherwise clicking on the
    // placeholder text (which sits on top) fails to focus the field.
    .pointer_events(|| false);

    // Trailing × that empties the value. In-flow beside the editor (which
    // flex-grows) — NOT an absolute overlay — so the editor's width is bounded
    // and its text can never scroll underneath the ×.
    let inner: AnyView = if let Some(trailing) = trailing {
        // Trailing action (e.g. the AI send/stop icon) in-flow beside the editor,
        // right-aligned and vertically centred — same spot as the clearable ×. The
        // negative right margin pulls it 4px closer to the box edge (14px gap).
        let side = container(trailing()).style(|s| {
            s.flex_shrink(0.0_f32)
                .items_center()
                .margin_left(6.0)
                .margin_right(-4.0)
        });
        h_stack((editor, side))
            .style(|s| s.width_full().height_full().min_width(0.0).items_center())
            .into_any()
    } else if clearable {
        let clear = dyn_container(
            move || !text_sig.with(|t| t.is_empty()),
            move |show| {
                if show {
                    container(icons::icon(icons::X, 16.0).style(|s| s.color(theme::text())))
                        .on_click_stop(move |_| text_sig.set(String::new()))
                        .style(|s| {
                            s.flex_shrink(0.0_f32)
                                .items_center()
                                .margin_left(6.0)
                                .color(theme::text())
                                .hover(|s| s.color(theme::text_dim()))
                        })
                        .into_any()
                } else {
                    empty().into_any()
                }
            },
        );
        h_stack((editor, clear))
            .style(|s| s.width_full().height_full().min_width(0.0).items_center())
            .into_any()
    } else {
        editor.into_any()
    };

    stack((inner, placeholder)).style(move |s| {
        // Fixed height when given; else derive from content. +3 (auto case): the
        // 1px top/bottom borders (border-box) plus a hair of slack so the editor's
        // viewport fully contains its content and no phantom scrollbar shows.
        let h = match height {
            Some(hh) => hh,
            None => {
                // Effective cap: a reactive `max_rows` (viewer) else the default.
                let cap_n = max_rows.map(|m| m.get()).unwrap_or(cap).max(1);
                rows.get().clamp(1, cap_n) as f64 * line_h + pad_v * 2.0 + 3.0
            }
        };
        // No flex_grow baked in: in a vertical stack that would stretch the box's
        // HEIGHT and blow past `h`. Callers that need to fill a row (the chat box)
        // add flex_grow themselves.
        let s = s
            .min_width(0.0)
            .height(h)
            .padding_horiz(CHAT_PAD_H)
            .padding_vert(pad_v)
            .background(background())
            .border(1.0)
            .border_radius(border_radius)
            .cursor(CursorStyle::Text);
        match border_color {
            Some(c) => s.border_color(c()),
            None if focused.get() => s.border_color(theme::field_border_active()),
            None => s.border_color(theme::field_border()),
        }
    })
}

// ── Results pane: reactive on QueryState ────────────────────────────────────
pub(crate) fn thumb_len(desired: f64, track: f64) -> f64 {
    let track = track.max(0.0);
    let min = 24.0_f64.min(track);
    desired.clamp(min, track.max(min))
}

// ── Overlays: connection menu · Find Anywhere · Manage Connections ──────────
//
// Each overlay is a `dyn_container` that is a *direct* child of the workspace
// root `stack`. When open, we style the container itself `absolute().inset(0)`
// so it fills the window (the root is its positioned ancestor); when closed it
// falls back to default layout with an `empty()` child → zero-size and
// click-through. Absolute children nested any deeper resolve against a
// zero-sized parent, so this is the one placement that actually fills the view.

// The connection switcher dropdown: saved connections + "Manage Connections".
// ── Footer (status bar) ──────────────────────────────────────────────────

/// A status-bar text segment: 12px, muted grey (`status_text`).
fn footer_text(s: String) -> AnyView {
    text(s)
        .style(|st| st.color(theme::status_text()).font_size(theme::FONT_STATUS))
        .into_any()
}

fn footer(ui: Ui) -> impl IntoView {
    let schema_visible = ui.layout.schema_visible;
    let right_panel = ui.layout.right_panel;
    let connections = ui.conn.connections;
    let active_conn = ui.conn.active_conn;
    let tabs = ui.tabs_ui.tabs;
    let active = ui.tabs_ui.active;
    let db_nodes = ui.schema.db_nodes;
    let soft_tabs = ui.layout.soft_tabs;
    let tab_width = ui.layout.tab_width;
    let word_wrap = ui.layout.word_wrap;
    let popup_menu = ui.overlay.popup_menu;
    let popup_anchor = ui.overlay.popup_anchor;
    let resources = ui.resources;
    let ai_model = ui.ai.model;
    let ai_effort = ui.ai.effort;

    // ── Reactive state for the left status cluster ──
    // Caret Ln/Col of the active tab (1-based). Reads the tab's `query` +
    // `cursor_offset` (mirrored out of the editor); safe to read per-tab signals
    // here — the same pattern as the `read_only`/`active_db` memos.
    let cursor_lc = create_memo(move |_| {
        let id = active.get();
        tabs.with(|v| {
            v.iter().find(|t| t.id == id).map(|t| {
                schemaic_core::text_ops::line_col_of_offset(&t.query.get(), t.cursor_offset.get())
            })
        })
        .unwrap_or((1, 1))
    });
    // Live count of probable-typo warnings in the active tab's SQL (same analysis
    // as the editor squiggles). Tracks the query text and the schema list.
    let warn_count = create_memo(move |_| {
        let id = active.get();
        db_nodes.track();
        let q = tabs.with(|v| v.iter().find(|t| t.id == id).map(|t| t.query.get()));
        match q {
            Some(q) => editor_pane::syntax_errors(&q, db_nodes).len(),
            None => 0,
        }
    });
    // Is the active tab's connection read-only? (Same derivation as `center`.)
    let read_only = create_memo(move |_| {
        let id = active.get();
        let cid = tabs.with(|v| v.iter().find(|t| t.id == id).map(|t| t.conn_id.get()));
        match cid {
            Some(cid) => connections
                .with(|cs| cs.iter().find(|c| c.id == cid).map(|c| c.read_only))
                .unwrap_or(false),
            None => false,
        }
    });

    // AI/Terminal toggles are mutually exclusive: turning one on replaces the
    // other; clicking the active one hides it (right column freed).
    let set_right = move |target: RightPanel| {
        right_panel.update(|r| {
            *r = if *r == target {
                RightPanel::None
            } else {
                target
            }
        });
    };
    // Left edge: the Schema (folder-tree) toggle — kept on the left so it reads as
    // opening the panel that lives on the left. Right edge: AI / History / Terminal
    // toggles, likewise under their right-column panels.
    let schema_icon = toggle_icon(
        icons::FOLDER_TREE,
        move || schema_visible.get(),
        move || schema_visible.update(|v| *v = !*v),
    )
    .style(|s| s.margin_left(5.0));
    let right_group = h_stack((
        toggle_icon_view(
            icons::icon_wh(icons::AI_LOGO, 16.0, 10.0).style(|s| s.flex_shrink(0.0_f32)),
            move || right_panel.get() == RightPanel::Ai,
            move || set_right(RightPanel::Ai),
        ),
        toggle_icon(
            icons::TIMELINE,
            move || right_panel.get() == RightPanel::History,
            move || set_right(RightPanel::History),
        ),
        toggle_icon(
            icons::TERMINAL,
            move || right_panel.get() == RightPanel::Terminal,
            move || set_right(RightPanel::Terminal),
        )
        .style(|s| s.margin_right(5.0)),
    ))
    .style(|s| s.flex_row().items_center().gap(10.0));

    // ── Left status cluster (after the schema icon) ──
    // Gaps: 40px between the four groups (position | editor | status | AI), 15px
    // within a group; the icon→its-label gap is 5px. All text 12px muted grey.
    // Ln/Col — click (or Ctrl+G in the editor) opens the active tab's Go-to-line
    // popup. Colour is set on this container so the child text inherits it and the
    // hover (schema-icon accent) reaches the text.
    let cursor_seg = dyn_container(
        move || cursor_lc.get(),
        move |(l, c)| {
            text(format!("Ln {l}, Col {c}"))
                .style(|s| s.font_size(theme::FONT_STATUS))
                .into_any()
        },
    )
    .on_click_stop(move |_| {
        let id = active.get_untracked();
        tabs.with_untracked(|v| {
            if let Some(t) = v.iter().find(|t| t.id == id) {
                t.goto_open.set(true);
            }
        });
    })
    .style(|s| {
        s.margin_left(40.0)
            .items_center()
            .color(theme::status_text())
            .hover(|s| s.color(theme::chip_active()))
    });
    // Tabs vs Spaces + width. Click opens a menu (centred above the segment): the
    // two indent styles, a separator, then sizes 1–6; the active style + size are
    // tinted (chip accent). Clicking again while open toggles it shut.
    // `tab_origin`/`tab_size` track the segment's window rect so the popup can
    // centre on it (its x shifts as the Ln/Col text to its left grows/shrinks).
    let tab_origin: RwSignal<(f64, f64)> = RwSignal::new((0.0, 0.0));
    let tab_size: RwSignal<(f64, f64)> = RwSignal::new((0.0, 0.0));
    let tabs_seg = dyn_container(
        move || (soft_tabs.get(), tab_width.get()),
        move |(soft, w)| {
            text(format!("{}: {}", if soft { "Spaces" } else { "Tabs" }, w))
                .style(|s| s.font_size(theme::FONT_STATUS))
                .into_any()
        },
    )
    .on_move(move |p| tab_origin.set((p.x, p.y)))
    .on_resize(move |r| tab_size.set((r.width(), r.height())))
    // Stop the pointer-down so the workspace-root "close popup on down" handler
    // doesn't fire for our own clicks — otherwise the down would close the menu
    // and the up would immediately reopen it (never toggling shut).
    .on_event_stop(EventListener::PointerDown, |_| {})
    .on_click_stop(move |_| {
        // Toggle: a second click on the segment closes the open menu.
        if popup_menu.get_untracked().is_some() {
            popup_menu.set(None);
            return;
        }
        let soft = soft_tabs.get_untracked();
        let w = tab_width.get_untracked();
        let mut entries = vec![
            if soft {
                MenuEntry::action_colored("Spaces", theme::chip_active, move || soft_tabs.set(true))
            } else {
                MenuEntry::action("Spaces", move || soft_tabs.set(true))
            },
            if !soft {
                MenuEntry::action_colored("Tabs", theme::chip_active, move || soft_tabs.set(false))
            } else {
                MenuEntry::action("Tabs", move || soft_tabs.set(false))
            },
            MenuEntry::Separator,
        ];
        for n in 1..=6usize {
            entries.push(if n == w {
                MenuEntry::action_colored(n.to_string(), theme::chip_active, move || {
                    tab_width.set(n)
                })
            } else {
                MenuEntry::action(n.to_string(), move || tab_width.set(n))
            });
        }
        let (ox, oy) = tab_origin.get_untracked();
        let (sw, _sh) = tab_size.get_untracked();
        popup_anchor.set(Some((ox, ox + sw, oy, sw)));
        popup_menu.set(Some(entries));
    })
    .style(|s| {
        s.margin_left(15.0)
            .items_center()
            .color(theme::status_text())
            .hover(|s| s.color(theme::chip_active()))
    });
    let wrap_seg = dyn_container(
        move || word_wrap.get(),
        move |w| footer_text((if w { "Wrap" } else { "No wrap" }).to_string()),
    )
    .style(|s| s.margin_left(15.0));
    // Warnings: amber triangle + amber count, or a green check (no text) when clean.
    let warn_seg = dyn_container(
        move || warn_count.get(),
        move |n| {
            if n == 0 {
                icons::icon(icons::CIRCLE_CHECK, 15.0)
                    .style(|s| s.color(theme::status_ok()))
                    .into_any()
            } else {
                h_stack((
                    icons::icon(icons::TRIANGLE_ALERT, 16.0)
                        .style(|s| s.color(theme::status_warn())),
                    text(n.to_string()).style(|s| {
                        s.margin_left(5.0)
                            .color(theme::status_warn())
                            .font_size(theme::FONT_STATUS)
                    }),
                ))
                .style(|s| s.flex_row().items_center())
                .into_any()
            }
        },
    )
    .style(|s| s.margin_left(40.0));
    // Read-only vs write mode (from the active connection's setting). Text only:
    // green for read-only (safe), amber for write mode (a heads-up).
    let ro_seg = dyn_container(
        move || read_only.get(),
        move |ro| {
            let (label, color) = if ro {
                ("Read only", theme::status_ok as fn() -> Color)
            } else {
                ("Write mode", theme::status_warn as fn() -> Color)
            };
            text(label)
                .style(move |s| s.color(color()).font_size(theme::FONT_STATUS))
                .into_any()
        },
    )
    .style(|s| s.margin_left(15.0));
    let cpu_seg = dyn_container(
        move || resources.get().cpu_label(),
        move |c| footer_text(format!("CPU: {c}")),
    )
    .style(|s| s.margin_left(15.0));
    let ram_seg = dyn_container(
        move || resources.get().ram_label(),
        move |r| footer_text(format!("RAM: {r}")),
    )
    .style(|s| s.margin_left(15.0));
    let model_seg = dyn_container(
        move || ai_model.get().label(),
        move |m| footer_text(m.to_string()),
    )
    .style(|s| s.margin_left(40.0));
    let effort_seg = dyn_container(
        move || ai_effort.get().label(),
        move |e| footer_text(e.to_string()),
    )
    .style(|s| s.margin_left(15.0));

    let left_group = h_stack((
        schema_icon,
        cursor_seg,
        tabs_seg,
        wrap_seg,
        warn_seg,
        ro_seg,
        cpu_seg,
        ram_seg,
        model_seg,
        effort_seg,
    ))
    .style(|s| s.flex_row().items_center().min_width(0.0));

    let bar = h_stack((left_group, right_group)).style(|s| {
        s.width_full()
            .height(theme::FOOTER_H)
            .min_height(theme::FOOTER_H)
            .flex_shrink(0.0_f32)
            .flex_row()
            .items_center()
            .justify_between()
            .background(theme::bg_deepest())
            .border_top(1.0)
            .border_color(theme::border())
    });
    // Identity-colour rule on the footer's top edge (on the "prominent colour"
    // setting): a 2px no-layout overlay over the existing 1px border.
    stack((bar, conn_edge_border(connections, active_conn, true)))
        .style(|s| s.width_full().flex_shrink(0.0_f32))
}

// The Find-palette search box: the shared field, autofocused on open, with a
// larger font. Escape closes the palette; Up/Down move the result selection and
// Enter opens the selected result (command-palette style).
pub(crate) fn search_box(
    query: RwSignal<String>,
    on_escape: Rc<dyn Fn()>,
    on_arrow_up: Rc<dyn Fn()>,
    on_arrow_down: Rc<dyn Fn()>,
    on_submit: Rc<dyn Fn()>,
) -> impl IntoView {
    edit_field(
        query,
        FieldCfg {
            placeholder: "Search everywhere",
            autofocus: true,
            font_size: 16.0,
            border_radius: 8.0,
            on_escape: Some(on_escape),
            on_arrow_up: Some(on_arrow_up),
            on_arrow_down: Some(on_arrow_down),
            on_submit: Some(on_submit),
            ..Default::default()
        },
    )
    .style(|s| s.width_full())
}
