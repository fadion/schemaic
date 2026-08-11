//! Schemaic UI (Floem).
//!
//! M2: the three-pane shell plus a **virtualized** Results grid — a frozen
//! header over a `scroll(virtual_stack(...))` that renders only the visible
//! rows, so millions of rows stay smooth. Rows are keyed by index and the view
//! fn indexes into a shared `Arc<ResultSet>` (no per-row cloning). Layout
//! follows FEATURES §1.

mod ai_panel;
pub use ai_panel::mark_messages_seen;
mod completion;
mod connection_form;
mod consts;
mod ddl_preview;
mod diff_view;
mod editor_pane;
mod erd_view;
pub mod fonts;
mod grid;
mod history_panel;
pub mod icons;
mod import_view;
mod markdown;
mod monitor_view;
mod overlays;
mod plan_view;
mod schema_tree;
mod settings;
pub mod sql_highlight;
mod table_designer;
mod tabs;
pub mod theme;
pub mod themes;
mod view_editor;
mod widgets;

use ai_panel::ai_panel;
use connection_form::manage_modal;
use consts::*;
use editor_pane::{QueryPaneParams, editor_placeholder, query_pane};
use erd_view::erd_overlay;
use grid::{GridCtx, grid_error_bar, grid_find_bar, loaded_view, results_view, running_view};
use history_panel::history_panel;
use monitor_view::monitor_overlay;
use overlays::{
    active_db_menu_overlay, confirm_overlay, conn_menu_overlay, context_menu_overlay,
    db_visibility_overlay, error_modal_overlay, find_overlay, popup_menu_overlay,
    schema_settings_overlay, tx_prompt_overlay,
};
use plan_view::plan_overlay;
use schema_tree::schema_panel;
use settings::{ai_settings_overlay, help_overlay, term_settings_overlay, theme_settings_overlay};
use tabs::tab_bar;
use widgets::*;

use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

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
use floem::views::{
    Decorators, Delay, LabelClass, TextInputClass, TooltipClass, TooltipContainerClass,
};
use schemaic_core::connection::{ConnStatus, Connection, Environment, SshAuth};
use schemaic_core::db_color::DbColorRule;
use schemaic_core::favorite::FavoriteRule;
use schemaic_core::format::ColumnFormatRule;
use schemaic_core::history::HistoryEntry;
use schemaic_core::intel::SqlDialect;
use schemaic_core::model::{CommitDone, GridWrite, QueryState, RefetchRequest};
use schemaic_core::resource::ResourceSample;
use schemaic_core::tx::{TxMode, TxState};

/// The grid-commit completion callback, invoked on the UI thread with the outcome.
pub type CommitDoneFn = Rc<dyn Fn(CommitDone)>;
/// Commit staged grid changes transactionally: apply the `GridWrite`, optionally
/// re-fetch (`Some` ⇒ splice the edited rows in place), then report via
/// [`CommitDoneFn`]. Aliased to keep the field/signal types below readable.
pub type CommitFn = Rc<dyn Fn(GridWrite, Option<RefetchRequest>, CommitDoneFn)>;

/// What to write, and where. Everything here is owned or refcounted so the
/// request can cross to a worker thread — the `Arc`s mean a 200k-row snapshot
/// costs a refcount, not a copy.
pub struct ExportRequest {
    pub path: std::path::PathBuf,
    pub format: schemaic_core::export::ExportFormat,
    pub rs: Arc<schemaic_core::model::ResultSet>,
    pub order: Arc<Vec<usize>>,
    /// The result's base table, when it has one — only the SQL format uses it, to
    /// name the `INSERT` target.
    pub source: Option<TableSource>,
    /// The tab's connection dialect, so an exported `INSERT` loads back into the
    /// engine the rows came from.
    pub dialect: schemaic_core::intel::SqlDialect,
}

/// The table an import is loading into, captured when the modal opens so a
/// schema refresh underneath it can't retarget the import.
#[derive(Clone, Debug)]
pub struct ImportTargetInfo {
    pub conn_id: u64,
    pub database: String,
    /// The PostgreSQL namespace, when the table has one.
    pub schema: Option<String>,
    pub table: schemaic_core::schema::TableInfo,
}

impl ImportTargetInfo {
    /// `schema.table` on PostgreSQL outside `public`, else just the table name.
    pub fn display(&self) -> String {
        match &self.schema {
            Some(s) if s != "public" => format!("{s}.{}", self.table.name),
            _ => self.table.name.clone(),
        }
    }
}

/// Read a file's opening records so the modal can show what it found. `cfg` is
/// `None` on the first look, which asks for the dialect to be sniffed.
pub struct ImportProbeRequest {
    pub path: std::path::PathBuf,
    pub format: schemaic_core::import::ImportFormat,
    pub cfg: Option<schemaic_core::import::ReadConfig>,
}

/// What a probe found: the settings in effect (sniffed or as supplied) and the
/// first rows under them.
pub struct ImportProbeResult {
    pub cfg: schemaic_core::import::ReadConfig,
    pub sample: schemaic_core::import::Sample,
    /// The file's size on disk. Stat'd here rather than in the view because the
    /// probe is already the thread that touches the filesystem; the modal only
    /// needs it to warn about a large JSON load
    /// ([`schemaic_core::import::json_memory_warning`]).
    pub file_bytes: u64,
}

pub type ImportProbeDoneFn = Rc<dyn Fn(Result<ImportProbeResult, String>)>;
/// Read a sample off the UI thread — a file dialog can hand back anything, and
/// opening it must not stall the window.
pub type ImportProbeFn = Rc<dyn Fn(ImportProbeRequest, ImportProbeDoneFn)>;

/// Everything needed to check and load a file into a table.
pub struct ImportRunRequest {
    pub target: ImportTargetInfo,
    pub path: std::path::PathBuf,
    pub format: schemaic_core::import::ImportFormat,
    pub cfg: schemaic_core::import::ReadConfig,
    pub mapping: schemaic_core::import::Mapping,
}

/// How an import ended.
pub enum ImportOutcome {
    /// The file was checked and found wanting — **nothing was written**. This is
    /// the point of validating first: the all-or-nothing transaction would have
    /// rolled back on the first bad row, one error at a time.
    Invalid(schemaic_core::import::Validation),
    /// Rows committed.
    Done(u64),
    /// Stopped on request. Like every other exit that isn't `Done`, the
    /// transaction rolled back — a cancelled import leaves no partial load.
    Cancelled,
    /// The read or the transaction failed.
    Failed(String),
}

pub type ImportDoneFn = Rc<dyn Fn(ImportOutcome)>;
/// Validate the whole file, then load it in one transaction — off the UI thread.
pub type ImportFn = Rc<dyn Fn(ImportRunRequest, ImportDoneFn)>;

/// What the table designer is editing.
///
/// Captured when the modal opens, like [`ImportTargetInfo`]: a schema refresh
/// underneath must not retarget a draft the user is halfway through.
#[derive(Clone)]
pub struct DesignerTarget {
    pub conn_id: u64,
    pub database: String,
    /// The namespace a *new* table goes into (`None` on MySQL). For an existing
    /// table the draft carries it.
    pub schema: Option<String>,
    pub dialect: SqlDialect,
    /// The introspected table the draft started from — the left-hand side of
    /// every diff. `None` means this is a new table, which emits `CREATE`.
    pub current: Option<schemaic_core::schema::TableInfo>,
    /// Table names in the database, for the foreign-key target picker.
    pub tables: Vec<String>,
    /// The connection's read-only guard rail: the designer still opens (looking
    /// is fine), but Apply is refused.
    pub read_only: bool,
}

impl DesignerTarget {
    /// The modal's title subject: the table being designed, or the database a
    /// new one is being created in.
    pub fn display(&self) -> String {
        match &self.current {
            Some(t) => schemaic_core::schema::display_name(t.schema.as_deref(), &t.name),
            None => self.database.clone(),
        }
    }
}

/// What the view editor is editing.
///
/// Captured when the modal opens, exactly like [`DesignerTarget`] — but a much
/// smaller thing to hold, because a view *is* a name and a `SELECT`. That's why
/// this doesn't reuse the designer: a list-plus-form has nothing to list.
#[derive(Clone)]
pub struct ViewTarget {
    pub conn_id: u64,
    pub database: String,
    /// The namespace a *new* view goes into (`None` on MySQL). For an existing
    /// view the draft carries it.
    pub schema: Option<String>,
    pub dialect: SqlDialect,
    /// The introspected view the draft started from — the left-hand side of the
    /// diff. `None` means this is a new view, which emits `CREATE VIEW`.
    pub current: Option<schemaic_core::schema::TableInfo>,
    pub read_only: bool,
}

impl ViewTarget {
    /// The modal's title subject: the view being edited, or the database a new
    /// one is being created in.
    pub fn display(&self) -> String {
        match &self.current {
            Some(t) => schemaic_core::schema::display_name(t.schema.as_deref(), &t.name),
            None => self.database.clone(),
        }
    }
}

/// Which section of the designer is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesignerTab {
    Table,
    Columns,
    Indexes,
    ForeignKeys,
}

impl DesignerTab {
    pub const ALL: [DesignerTab; 4] = [
        DesignerTab::Table,
        DesignerTab::Columns,
        DesignerTab::Indexes,
        DesignerTab::ForeignKeys,
    ];
    pub fn label(self) -> &'static str {
        match self {
            DesignerTab::Table => "Table",
            DesignerTab::Columns => "Columns",
            DesignerTab::Indexes => "Indexes",
            DesignerTab::ForeignKeys => "Foreign keys",
        }
    }
}

/// A generated DDL plan waiting for approval. **No DDL is ever run without one**
/// — that's the decision this type exists to enforce, so every path into schema
/// editing (designer, create-table, context-menu shortcut) funnels through the
/// same review.
#[derive(Clone)]
pub struct DdlPreview {
    pub conn_id: u64,
    pub database: String,
    /// What the plan is about, for the title ("orders", or a new table's name).
    pub subject: String,
    /// One plain-language line per change.
    pub changes: Vec<String>,
    /// What the plan destroys, in plain language. Non-empty ⇒ the modal says so
    /// before the Apply button, in the error colour.
    pub destructive: Vec<String>,
    pub statements: Vec<String>,
    pub read_only: bool,
}

/// Run a generated DDL plan against a database.
pub struct DdlRunRequest {
    pub conn_id: u64,
    pub database: String,
    pub statements: Vec<String>,
}

/// Reports a DDL run's outcome on the UI thread. `Err` carries a message that
/// already says which statement failed and how much of the plan stuck (see
/// `schemaic_db::DdlError`).
pub type DdlDoneFn = Rc<dyn Fn(Result<(), String>)>;
/// Apply a DDL plan off the UI thread, then re-introspect the database.
pub type DdlFn = Rc<dyn Fn(DdlRunRequest, DdlDoneFn)>;

/// The schema-editing modals' state (Copy bundle, reset on open — as with
/// [`ImportUi`], these outlive no view so they need no scope to dispose).
///
/// `designer` and `preview` each double as their modal's open flag.
#[derive(Clone, Copy)]
pub struct DdlUi {
    pub designer: RwSignal<Option<DesignerTarget>>,
    /// The table being designed. One signal rather than a field per control: the
    /// draft is what [`schemaic_core::ddl::diff`] reads, so every edit has to
    /// land in the same value or the change count lies.
    pub draft: RwSignal<schemaic_core::ddl::TableDraft>,
    pub tab: RwSignal<DesignerTab>,
    /// Selected row in the active section's list.
    pub selected: RwSignal<usize>,
    /// Bumped on every *structural* edit (add / remove / move). The detail form
    /// is keyed on it as well as on `selected`, because removing the selected
    /// row leaves `selected` unchanged while the item at that index is now a
    /// different one — nothing else would tell the form to rebuild.
    pub rev: RwSignal<u64>,
    /// The view editor's target; doubles as its open flag. A view gets its own
    /// modal rather than a designer tab — see [`ViewTarget`].
    pub view: RwSignal<Option<ViewTarget>>,
    /// The view being edited. Same rule as `draft`: one value, because the
    /// footer's change count is [`schemaic_core::ddl::diff_view`] of exactly it.
    pub view_draft: RwSignal<schemaic_core::ddl::ViewDraft>,
    /// The body editor's auto-grow cap, in rows. A signal because that's what
    /// [`FieldCfg::max_rows`] takes; nothing changes it.
    pub view_rows: RwSignal<usize>,
    pub preview: RwSignal<Option<DdlPreview>>,
    /// The plan's SQL, bound to the preview's read-only field.
    pub sql: RwSignal<String>,
    /// The preview's SQL box auto-grow cap, in rows. A signal because that's
    /// what [`FieldCfg::max_rows`] takes; nothing changes it.
    pub sql_rows: RwSignal<usize>,
    pub applying: RwSignal<bool>,
    pub error: RwSignal<Option<String>>,
    /// Set once the plan has been applied — the modal then only offers Close.
    pub applied: RwSignal<bool>,
    /// Bumped on every open. An apply is off-thread and can outlive the modal
    /// that asked for it, so its callback checks this before writing.
    pub generation: RwSignal<u64>,
}

/// Reports an export's outcome back on the UI thread (`Err` carries a
/// user-facing message).
pub type ExportDoneFn = Rc<dyn Fn(Result<(), String>)>;
/// Stream a result set to a file **off the UI thread**, reporting via
/// [`ExportDoneFn`]. Writing a large export inline froze the window for as long
/// as it took; the grid owns the save dialog, the app owns the worker.
pub type ExportFn = Rc<dyn Fn(ExportRequest, ExportDoneFn)>;

/// Delivers the Tier-2 (DB-validated) diagnostics for the statement under the
/// cursor back onto the UI thread.
pub type ValidateDoneFn = Rc<dyn Fn(Vec<schemaic_core::intel::Diagnostic>)>;
/// A query-pane → app request to validate the statement `sql[lo..hi]` against the
/// live database (non-executing PREPARE) and report back via [`ValidateDoneFn`].
/// Targets the active tab's connection/database.
pub type ValidateFn = Rc<dyn Fn(String, usize, usize, ValidateDoneFn)>;

/// A grid → app request to AI-fill a single cell. The app bottom-samples the base
/// table, builds a prompt (DDL + sample + this row's context), runs a one-shot
/// `claude -p` call, parses the reply, and reports back via [`AiFillDoneFn`].
pub struct AiFillRequest {
    pub conn_id: u64,
    /// The base table being sampled — namespace included, so a PostgreSQL table
    /// outside `public` is sampled from the right schema.
    pub source: TableSource,
    /// The real column name being filled.
    pub column: String,
    /// The row being filled, as `(column_name, value)` for the same base table —
    /// so the model keeps the generated value coherent with the rest of the row.
    pub row_context: Vec<(String, Option<String>)>,
}
/// The outcome the grid stages: a value, an explicit SQL `NULL`, or a failure
/// (DB/CLI error or an empty reply) to surface in the error bar.
pub enum AiFillResult {
    Value(String),
    Null,
    Failed(String),
}
/// Report an [`AiFillRequest`]'s outcome — invoked on the UI thread.
pub type AiFillDoneFn = Rc<dyn Fn(AiFillResult)>;
/// AI-fill a single cell (grid → app), reporting via [`AiFillDoneFn`].
pub type AiFillFn = Rc<dyn Fn(AiFillRequest, AiFillDoneFn)>;

/// A grid → app request to AI-generate `count` seed rows (Insert Row = 1, Seed
/// Table = N). The app samples the base table, builds a prompt, runs the one-shot
/// call, parses a JSON array of rows, and reports back via [`AiSeedDoneFn`].
pub struct AiSeedRequest {
    pub conn_id: u64,
    /// The base table being seeded — namespace included (see [`AiFillRequest`]).
    pub source: TableSource,
    /// The columns the model should fill (editable, non-auto-increment). The grid
    /// stages only these back, so a stray column in the reply is ignored.
    pub fill_columns: Vec<String>,
    pub count: usize,
}
/// The outcome: the generated rows (`(column_name, value)` per row) or a failure
/// (DB/CLI error, empty/invalid reply) to surface in the error bar.
pub enum AiSeedResult {
    Rows(Vec<Vec<(String, Option<String>)>>),
    Failed(String),
}
/// Report an [`AiSeedRequest`]'s outcome — invoked on the UI thread.
pub type AiSeedDoneFn = Rc<dyn Fn(AiSeedResult)>;
/// AI-generate seed rows (grid → app), reporting via [`AiSeedDoneFn`].
pub type AiSeedFn = Rc<dyn Fn(AiSeedRequest, AiSeedDoneFn)>;
use schemaic_core::schema::{SchemaState, TableSource};
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
    /// The table this tab was opened from, if any — used to highlight the source
    /// table in the schema sidebar and to make the grid editable.
    pub source: RwSignal<Option<TableSource>>,
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
    /// A byte offset the editor should jump the caret to (move + centre + focus),
    /// then clear. Set by the status-bar warning count to reach the first warning.
    pub jump_offset: RwSignal<Option<usize>>,
    /// This tab's offline diagnostics — the squiggles — published by the editor
    /// pane's **debounced** analysis (`editor_pane`, 120 ms with a generation
    /// guard).
    ///
    /// It lives on the tab so the status bar can read the result instead of
    /// re-deriving it: the footer's warning count used to call
    /// `compute_diagnostics` itself, on every keystroke, undebounced — a full
    /// catalog rebuild plus a full parse of the whole document, measured at
    /// 20 ms per keypress on a 500-table schema with a 47 KB script, walking
    /// straight around the debounce the editor implements for exactly this.
    pub diagnostics: RwSignal<Vec<schemaic_core::intel::Diagnostic>>,
    /// A column name to select + scroll into view once this tab's grid is loaded,
    /// then clear. Set by double-clicking a column row in the schema tree (open the
    /// table, highlight the column); the grid consumes it reactively (`grid_view`).
    pub highlight_col: RwSignal<Option<String>>,
    /// Whether the RESULTS panel is maximized (editor collapsed to height 0) for
    /// *this* tab. Per-tab so maximizing in one tab doesn't affect others; the live
    /// render flag (`LayoutUi::editor_collapsed`) mirrors the active tab's value,
    /// loaded on tab switch and written back by the expand/shrink toggle. Session-only
    /// (starts un-maximized), matching the pre-per-tab behaviour.
    pub results_maximized: RwSignal<bool>,
    /// Commit mode for this tab. [`TxMode::Manual`] pins one connection open (a
    /// `Session` in the app) and holds a transaction across statements until the
    /// user commits or rolls back. Session-only, and always starts
    /// [`TxMode::Auto`] — a tab that reopened in Manual would be an easy way to
    /// leave writes uncommitted without realising.
    pub tx_mode: RwSignal<TxMode>,
    /// Where this tab's transaction stands, folded from each statement's outcome
    /// by the app (see `schemaic_core::tx`). Always [`TxState::Idle`] in
    /// [`TxMode::Auto`]; drives the footer pill and the Commit/Rollback controls.
    pub tx: RwSignal<TxState>,
    /// Temporary per-tab editor font-size override (px) for Ctrl+scroll zoom —
    /// `None` follows the user's configured font size. Session-only, per-tab, not
    /// persisted; Ctrl+middle-click resets it to `None`. Useful for screen-sharing.
    pub font_zoom: RwSignal<Option<f32>>,
    /// The exact SQL last run **manually** (Ctrl+Enter / Run) — the base the grid's
    /// server-side filter/sort splice into and re-run. `None` until the first run.
    /// Decoupled from `query` (the live editor buffer), which drifts after edits.
    pub base_sql: RwSignal<Option<String>>,
    /// The active server-side filter/sort for this tab's result (persists across
    /// result reloads; reset on a fresh manual run). Session-only.
    pub grid_query: RwSignal<schemaic_core::filter::GridQuery>,
    /// A filter/sort re-run's DB error, shown as a dismissible bar at the bottom of
    /// the *table* (the previous results stay put — unlike a manual run, which
    /// replaces the grid with the error). Cleared on a table click / new run.
    pub view_err: RwSignal<Option<String>>,
    /// Bumped on every fresh full result load (including a filter/sort re-run) so
    /// the results grid rebuilds even on a `Loaded`→`Loaded` transition — an
    /// in-place commit splice deliberately does NOT bump it, so it still avoids a
    /// rebuild. Part of the results-view container key.
    pub load_gen: RwSignal<u64>,
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
            jump_offset: cx.create_rw_signal(None),
            diagnostics: cx.create_rw_signal(Vec::new()),
            highlight_col: cx.create_rw_signal(None),
            results_maximized: cx.create_rw_signal(false),
            tx_mode: cx.create_rw_signal(TxMode::default()),
            tx: cx.create_rw_signal(TxState::default()),
            font_zoom: cx.create_rw_signal(None),
            base_sql: cx.create_rw_signal(None),
            grid_query: cx.create_rw_signal(schemaic_core::filter::GridQuery::default()),
            view_err: cx.create_rw_signal(None),
            load_gen: cx.create_rw_signal(0),
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

/// What the user chose when asked about an open transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TxChoice {
    Commit,
    Rollback,
    /// Back out — the action that raised the prompt doesn't happen either.
    Cancel,
}

/// A pending "this would strand your open transaction" question.
///
/// Raised by the four actions that can't just proceed: switching a tab back to
/// Auto-commit, closing it, disconnecting, and changing its database (a
/// PostgreSQL session is bound to one database for life). The modal only
/// renders it and calls `resolve`; the app does the `COMMIT`/`ROLLBACK` and then
/// resumes — or abandons — whatever raised it.
#[derive(Clone)]
pub struct TxPrompt {
    pub tab_id: usize,
    /// The tab's title — the subject of the question ("Query 3 has an open
    /// transaction…"). What the user was *doing* isn't repeated: the prompt
    /// appears the instant they do it.
    pub tab: String,
    /// Statements in the transaction at risk, for the body text.
    pub stmts: u32,
    /// `false` when the transaction is aborted (PostgreSQL) — Commit isn't
    /// offered, because committing an aborted transaction just rolls it back.
    pub can_commit: bool,
    pub resolve: Rc<dyn Fn(TxChoice)>,
}

/// Which step of the import modal is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportStep {
    /// Pick the file and confirm how to read it.
    Source,
    /// Map the file's columns onto the table's, with a preview.
    Mapping,
    /// Rows landed.
    Done,
}

/// The import modal's state (Copy bundle, created once and reset on open —
/// per-open signals would need a scope to dispose, and this modal outlives no
/// view).
///
/// `target` doubles as the open flag: `Some` ⇒ the modal is showing.
#[derive(Clone, Copy)]
pub struct ImportUi {
    pub target: RwSignal<Option<ImportTargetInfo>>,
    pub step: RwSignal<ImportStep>,
    /// The chosen file. `None` until one is picked.
    pub path: RwSignal<Option<std::path::PathBuf>>,
    pub format: RwSignal<schemaic_core::import::ImportFormat>,
    /// Read settings — sniffed on pick, then editable. Held as separate signals
    /// so each control binds to one.
    pub delimiter: RwSignal<String>,
    pub has_header: RwSignal<bool>,
    /// Whether an empty CSV field means NULL. Its own control rather than a
    /// token in the list below, because "the empty string" can't be written in a
    /// comma-separated list — an empty box would be indistinguishable from "no
    /// tokens at all", which silently flips the meaning of every blank field.
    pub empty_is_null: RwSignal<bool>,
    /// Additional comma-separated texts that mean NULL (`NULL`, `\N`, `NA`…).
    pub null_tokens: RwSignal<String>,
    /// Strip whitespace around every field. Off by default — see
    /// [`schemaic_core::import::ReadConfig::trim`].
    pub trim: RwSignal<bool>,
    /// The chosen file's size on disk, from the probe. Only used to warn that a
    /// large JSON load is held in memory — 0 when unknown.
    pub file_bytes: RwSignal<u64>,
    /// The file's columns and first rows, under the current settings.
    pub sample: RwSignal<Option<schemaic_core::import::Sample>>,
    pub mapping: RwSignal<schemaic_core::import::Mapping>,
    /// Problems found by the full check — populated only when an import was
    /// refused, so a non-empty list always means nothing was written.
    pub issues: RwSignal<Vec<schemaic_core::import::Issue>>,
    pub more_issues: RwSignal<bool>,
    /// A read or transaction failure (as opposed to per-row issues).
    pub error: RwSignal<Option<String>>,
    /// Rows committed, once the import succeeds.
    pub imported: RwSignal<u64>,
    /// True while a probe, check or load is running.
    pub busy: RwSignal<bool>,
    /// Set while the modal writes settings into its own controls, so the effect
    /// that re-reads the file on a settings change doesn't treat the app's own
    /// answer as a new question and loop.
    pub applying: RwSignal<bool>,
    /// Bumped on every open. A probe or an import is off-thread and can outlive
    /// the modal that asked for it, so its callback checks this before writing —
    /// otherwise closing a running import and opening the modal on another table
    /// lets the first one report its result into the second one's state.
    pub generation: RwSignal<u64>,
}

/// A yes/no question asked before something destructive runs.
///
/// Deliberately generic: this is *the* confirm modal, so the next action that
/// needs asking sets [`OverlayUi::confirm`] rather than growing a fourth bespoke
/// overlay. [`TxPrompt`] stays separate — Commit/Rollback/Cancel isn't a yes/no,
/// and there no safe default answer between keeping and discarding writes.
///
/// The overlay only renders the question; `resolve` gets the answer (`true` =
/// Yes) and the caller does the work. Escape and clicking the backdrop both
/// answer No, since declining is always the safe side of a confirm.
#[derive(Clone)]
pub struct Confirm {
    /// Bold heading, naming the action ("Close all tabs").
    pub title: String,
    /// The question itself.
    pub message: String,
    pub resolve: Rc<dyn Fn(bool)>,
}

/// One statement's result within a multi-statement (Run Everything) run. The
/// label names its tab; `state` is that statement's own lifecycle.
#[derive(Clone)]
pub struct ResultPanel {
    pub label: String,
    pub state: QueryState,
}

// The chat message types live in `schemaic_core::transcript` alongside the
// segments they carry — they're persisted (`core::chat`), and core can't depend
// on the UI. Re-exported so `schemaic_ui::ChatMessage` keeps working.
pub use schemaic_core::transcript::{ChatMessage, Role};

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
    /// Environment this connection points at, shown as a top-bar badge. Defaults
    /// to `Environment::None` (no badge).
    pub environment: RwSignal<Environment>,
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
            environment: cx.create_rw_signal(Environment::None),
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
        self.environment.set(c.environment);
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
        self.environment.set(Environment::None);
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
            environment: self.environment.get_untracked(),
        }
    }
}

/// What a schema-tree right-click landed on. Action data (DDL, AI prompt) is
/// precomputed when the menu opens, since the row has the context then.
#[derive(Clone)]
pub enum CtxKind {
    Database,
    /// A PostgreSQL namespace group — only rendered when a database has more than
    /// one, so this never appears on MySQL.
    Schema {
        database: String,
        /// A `CREATE` script for every table in the namespace, built lazily when
        /// the menu is staged (see `DbSchema::create_ddl_script`).
        ddl: String,
    },
    Table {
        database: String,
        /// PostgreSQL namespace (`None` on MySQL) — carried so the menu's actions
        /// (Open, ERD, Live Monitor) address the table the user right-clicked.
        schema: Option<String>,
        table: String,
        ddl: String,
    },
    /// A column. Carries its table, because the schema-editing entries act on
    /// the column *in* a table — a bare name can't be dropped.
    Field {
        source: TableSource,
        column: String,
    },
    /// An index or foreign-key row under a table.
    Key {
        source: TableSource,
        /// The index the row stands for, whole — a drop has to know whether
        /// PostgreSQL needs `DROP INDEX` or `ALTER TABLE … DROP CONSTRAINT`.
        index: Box<schemaic_core::schema::IndexInfo>,
        /// The foreign-key constraint this index backs, when it does. Dropping
        /// *that* is what the user means; the backing index needn't share its
        /// name (classicmodels' `customerNumber` backs `orders_ibfk_1`).
        foreign_key: Option<String>,
    },
}

/// Which database/table an open ER-diagram modal is showing, and how it was
/// seeded (a single table's neighbourhood, or the whole database). `Some` on
/// `OverlayUi::erd` means the modal is open. The `conn_id` is captured for
/// completeness; the schema is resolved from `db_nodes` by `database` (that list
/// is always the active connection's).
#[derive(Clone)]
pub struct ErdTarget {
    pub conn_id: u64,
    pub database: String,
    pub seed: schemaic_core::erd::DiagramSeed,
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
    /// Run one statement — **through the write guard**
    /// ([`schemaic_core::sql::run_verdict`]). A refused or held-back run sets
    /// [`OverlayUi::run_guard`] and executes nothing.
    ///
    /// Every way of running SQL goes through this, and there is deliberately no
    /// unguarded counterpart on this bundle: the guard used to be two closures
    /// inside the editor pane, so the command palette's `>run` and the AI chat's
    /// Insert & Run reached the raw action and executed writes past all three
    /// protections — including the read-only block that has no override by
    /// design. The raw action never leaves the app crate now; `run_anyway` is
    /// the only way back to it.
    pub run: Rc<dyn Fn(String)>,
    /// Re-run the active tab with a server-side filter/sort view applied (the grid
    /// filter bar / header sort). Unlike `run`, this does NOT record history, does
    /// NOT touch `tab.base_sql`, and does NOT reset `tab.grid_query` — so the base
    /// statement and the active filter/sort survive the re-run. `sql` is the
    /// already-rewritten statement (see `schemaic_core::filter::build_query`).
    /// Not guarded: it re-runs the `SELECT` the grid is already showing.
    pub apply_view: Rc<dyn Fn(String)>,
    /// Run several statements in order (Run Everything): one result tab each.
    /// Guarded exactly as [`Self::run`] is.
    pub run_all: Rc<dyn Fn(Vec<String>)>,
    /// Replay whatever [`OverlayUi::run_guard`] is holding and clear it — the
    /// guard bar's "Run anyway". A hard block (`pending: None`) replays nothing.
    pub run_anyway: Rc<dyn Fn()>,
    pub cancel: Rc<dyn Fn()>,
    /// Commit staged grid changes (cell edits + new-row inserts) transactionally.
    /// Arg 2 is an optional re-fetch request (present ⇒ splice the edited rows
    /// instead of full-re-running; `None` for inserts, which full-re-run); arg 3 is
    /// the completion callback, invoked on the UI thread with the outcome.
    pub commit_edits: CommitFn,
    /// Stream a result set to a file on a worker thread. The grid owns the save
    /// dialog and the snapshot; this does the rendering + writing, so a large
    /// export doesn't block the window.
    pub export_file: ExportFn,
    /// Set a tab's commit mode (by tab id). Switching to Manual only marks the
    /// tab — the connection is pinned and `BEGIN` issued lazily on its first
    /// statement. Switching back to Auto with a transaction still open is the
    /// caller's problem to resolve first (see [`OverlayUi::tx_prompt`]).
    pub set_tx_mode: Rc<dyn Fn(usize, TxMode)>,
    /// `COMMIT` the tab's open transaction; the tab stays in Manual, ready for
    /// the next one.
    pub commit_tx: Rc<dyn Fn(usize)>,
    /// `ROLLBACK` the tab's open transaction — also the way out of a PostgreSQL
    /// transaction a failed statement aborted.
    pub rollback_tx: Rc<dyn Fn(usize)>,
    pub add_tab: Rc<dyn Fn()>,
    pub close_tab: Rc<dyn Fn(usize)>,
    /// Close every tab of the active connection (the ones the strip shows).
    /// Pinned tabs stay, and the connection's last remaining tab clears in place
    /// rather than disappearing. Tabs holding an open transaction are asked
    /// about one at a time; answering Cancel stops the run.
    pub close_all_tabs: Rc<dyn Fn()>,
    /// Toggle a tab's pinned state (by id) and re-order the strip so pinned tabs
    /// stay contiguous at the left, in pin order.
    pub toggle_pin: Rc<dyn Fn(usize)>,
    /// Duplicate a tab (by id): a new tab with the same connection/database and
    /// query, opened right after the source and made active.
    pub duplicate_tab: Rc<dyn Fn(usize)>,
    /// Show a table in a tab: focus the tab already showing it, or open a fresh
    /// one ("Open").
    pub open_table: Rc<dyn Fn(TableSource)>,
    /// Always open the table in a brand-new tab, even if it's already open
    /// ("Open in new tab").
    pub open_table_new: Rc<dyn Fn(TableSource)>,
    /// Open the table (reusing an existing tab) and select + scroll the named
    /// column into view in the grid (schema-tree column double-click).
    pub open_table_col: Rc<dyn Fn(TableSource, String)>,
    /// Open a new query tab containing `sql` (does NOT run it).
    pub open_query: Rc<dyn Fn(String)>,
    /// Reopen the most-recently-closed tab (Ctrl+Shift+T): restores its query,
    /// connection/database, source, and name from a small ring. No-op when empty.
    pub reopen_closed_tab: Rc<dyn Fn()>,
    /// Whether [`Self::reopen_closed_tab`] has anything to restore for the active
    /// connection — same per-connection scoping it applies itself. The tab menu
    /// dims its entry rather than offering a click that does nothing.
    pub can_reopen_closed_tab: Rc<dyn Fn() -> bool>,
    /// Open a brand-new tab sourced from a table (so its grid stays editable)
    /// running `sql`, and auto-run it. Used by the grid's "Follow foreign key" to
    /// land on the referenced table filtered to a row.
    pub open_table_filtered: Rc<dyn Fn(TableSource, String)>,
    /// Switch the active tab to a database (remembers it as the new-tab default).
    pub set_active_db: Rc<dyn Fn(String)>,
    /// Open the DB CLI for the active connection in the terminal, optionally
    /// scoped to a database.
    pub open_db_cli: Rc<dyn Fn(Option<String>)>,
    /// Run `EXPLAIN` (or `EXPLAIN ANALYZE` when arg 2 is true) for a statement,
    /// filling the plan modal's state. Targets the active tab's connection/db.
    pub run_plan: Rc<dyn Fn(String, bool)>,
    /// Validate the statement `sql[lo..hi]` against the live DB (non-executing
    /// PREPARE) and deliver Tier-2 diagnostics back. Targets the active tab.
    pub validate_stmt: ValidateFn,
    /// Open the Live Monitor for a `(conn_id, database, table)`: start polling
    /// that table on an interval and reveal the change-log modal.
    pub open_monitor: MonitorFn,
    /// AI-fill a single grid cell (sample the base table → one-shot AI → stage).
    pub ai_fill: AiFillFn,
    /// AI-generate seed rows (Insert Row / Seed Table) → stage pending rows.
    pub ai_seed: AiSeedFn,
}

/// The global navigation keys — handled at BOTH the workspace root and inside the
/// editor (which `on_event_stop`s every KeyDown, so it can't rely on bubbling).
/// Ctrl+P Find Anywhere · Ctrl+T new tab · Ctrl+Shift+T reopen closed tab ·
/// Ctrl+W close tab · Ctrl+Tab cycle (Shift = reverse) · Ctrl+1..9 jump to the Nth tab.
#[derive(Clone)]
pub(crate) struct NavKeys {
    pub tabs: RwSignal<Vec<Tab>>,
    pub active: RwSignal<usize>,
    /// Needed because the strip shows only this connection's tabs, and both
    /// Ctrl+Tab and Ctrl+1..9 have to move within what's visible.
    pub active_conn: RwSignal<u64>,
    pub find_open: RwSignal<bool>,
    pub find_query: RwSignal<String>,
    pub add_tab: Rc<dyn Fn()>,
    pub close_tab: Rc<dyn Fn(usize)>,
    pub reopen_closed: Rc<dyn Fn()>,
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
        // Ctrl+Shift+P → command palette: Find-Anywhere pre-filled with ">".
        if shift {
            if ch == Some("p") {
                self.find_query.set(">".to_string());
                if !self.find_open.get_untracked() {
                    self.find_open.set(true);
                }
                return true;
            }
            // Ctrl+Shift+T → reopen the most-recently-closed tab.
            if ch == Some("t") {
                (self.reopen_closed)();
                return true;
            }
            // Other Ctrl+Shift+<letter/digit> belong to the panel toggles, not us.
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

    /// The tab list as `schemaic_core::tabsel` wants it: `(id, conn_id)` pairs.
    fn tab_refs(&self) -> Vec<(usize, u64)> {
        self.tabs.with_untracked(|v| {
            v.iter()
                .map(|t| (t.id, t.conn_id.get_untracked()))
                .collect()
        })
    }

    /// Ctrl+Tab / Ctrl+Shift+Tab.
    ///
    /// Goes through `tabsel` rather than walking `tabs` directly. Both questions
    /// it asks — which tabs count, and which is next — are connection-scoped,
    /// because the strip only shows the active connection's tabs. This used to
    /// be a private reimplementation over the *unfiltered* list, so it selected
    /// tabs the strip deliberately hides, including ones on a disconnected
    /// connection; the command palette's Next/Previous Tab called `tabsel` and
    /// was correct, so the same user action had two behaviours.
    fn cycle(&self, back: bool) {
        let step = if back { -1 } else { 1 };
        if let Some(next) = schemaic_core::tabsel::cycle(
            &self.tab_refs(),
            self.active_conn.get_untracked(),
            Some(self.active.get_untracked()),
            step,
        ) {
            self.active.set(next);
        }
    }

    /// Ctrl+1..9 → the nth tab **of the strip**, which is the nth of this
    /// connection's tabs, not the nth entry of the flat list.
    fn jump(&self, idx: usize) {
        if let Some(id) =
            schemaic_core::tabsel::nth(&self.tab_refs(), self.active_conn.get_untracked(), idx)
        {
            self.active.set(id);
        }
    }
}

/// Schema-tree signals (Copy bundle).
#[derive(Clone, Copy)]
pub struct SchemaUi {
    pub db_nodes: RwSignal<Vec<ConnNode>>,
    pub expanded: RwSignal<HashSet<String>>,
    /// The active tab's source table, highlighted in the tree.
    pub active_table: RwSignal<Option<TableSource>>,
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
    /// Read a file's opening records (off the UI thread) so the import modal can
    /// show what it found.
    pub import_probe: ImportProbeFn,
    /// Check a file and, if it's clean, load it in one transaction.
    pub import_run: ImportFn,
    /// Stop the import [`SchemaActions::import_run`] is running, rolling it back.
    /// A no-op when nothing is running.
    pub import_cancel: Rc<dyn Fn()>,
    /// Apply an approved DDL plan, then re-introspect the database it changed.
    pub run_ddl: DdlFn,
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
    /// Flip a connection's read-only flag by id and persist (status-bar shortcut
    /// for the Manage-Connections toggle).
    pub toggle_read_only: Rc<dyn Fn(u64)>,
    pub delete_conn: Rc<dyn Fn(u64)>,
    /// Test the draft's host + credentials (opens a throwaway connection/tunnel
    /// and pings); the result lands in [`ConnUi::conn_test`].
    pub test_conn: Rc<dyn Fn()>,
    /// Re-run the active connection's health check. Nothing re-checks on a
    /// timer, so this is how a recovered server gets noticed without switching
    /// connections — it's what the header's "Not connected" retry calls.
    pub recheck_conn: Rc<dyn Fn()>,
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
    /// Editor collapsed (RESULTS "expand" toggle): session-only flag driving the
    /// toolbar icon (expand ↔ shrink) and the editor's collapsed height (0 vs
    /// `editor_h`, instant). `editor_h` stays the restore height, so un-collapsing
    /// returns the editor to exactly where it was.
    pub editor_collapsed: RwSignal<bool>,
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
    /// Validate the statement under the cursor against the live DB (non-executing
    /// PREPARE) as you type. Drives the editor's Tier-2 diagnostics.
    pub live_validate: RwSignal<bool>,
    /// Whether the OS window currently has focus. Session-only (never persisted);
    /// set from the workspace root's window-focus events. The app's connection
    /// health poll reads it so a backgrounded Schemaic stops opening connections
    /// it isn't about to use, and re-checks the moment focus comes back.
    pub window_focused: RwSignal<bool>,
}

/// Where the shared `popup_menu` anchors when it's *not* opened at the cursor.
/// Two distinct placements share the one `popup_menu` channel, so the opener must
/// say which it wants — a bare tuple let the status-bar and toolbar cases blur into
/// one and mis-placed the grid's Copy dropdown at the footer.
#[derive(Clone, Copy)]
pub enum PopupAnchor {
    /// Toolbar dropdown (grid Copy): the panel drops a few px below the icon and
    /// grows downward, left-aligned under it (overlapping it); if that would spill
    /// past the window's bottom it flips to grow upward. `(icon_left, icon_right,
    /// icon_bottom)` in window coords; the width comes from `popup_width`.
    BelowIcon(f64, f64, f64),
    /// Status-bar segment menu: centered on the segment's x-range and sitting 5px
    /// above the footer, growing upward. `(seg_left, seg_right)` in window coords.
    AboveFooter(f64, f64),
}

/// Overlay signals (Copy bundle): the two menu channels, the cursor anchor, and
/// the Find / error modals. No callbacks.
#[derive(Clone, Copy)]
pub struct OverlayUi {
    /// Schema-tree right-click menu target; `None` when closed.
    pub context_menu: RwSignal<Option<CtxMenu>>,
    /// Generic popup menu (built `MenuEntry` list). Opens at `last_mouse` unless
    /// `popup_anchor` is set (a toolbar / status-bar dropdown anchored to a widget).
    pub popup_menu: RwSignal<Option<Vec<MenuEntry>>>,
    /// When set, `popup_menu` anchors to a widget (see `PopupAnchor`) instead of the
    /// cursor. Set by the grid's Copy dropdown (`BelowIcon`) and the status-bar
    /// segment menus (`AboveFooter`); cleared (`None`) by the cursor right-click menus.
    pub popup_anchor: RwSignal<Option<PopupAnchor>>,
    /// `min_width` (px) of the next `popup_menu`. An opener sets it before opening;
    /// it resets to 170 on close, so menus that don't set one get the default.
    pub popup_width: RwSignal<f64>,
    /// Last pointer position in window coords (anchors the context menu).
    pub last_mouse: RwSignal<(f64, f64)>,
    pub find_open: RwSignal<bool>,
    pub find_query: RwSignal<String>,
    /// Per-connection Find-Anywhere search history (recorded on activation, shown
    /// on open with an empty query). Persisted to `search_history.json` by the app.
    pub search_history: RwSignal<Vec<schemaic_core::search_history::SearchEntry>>,
    /// "View" modal for an error bar. When `error_modal_text` is `Some`, the modal
    /// shows that text (the grid's commit error); otherwise it falls back to the
    /// active tab's full query error (the editor error bar).
    pub error_modal_open: RwSignal<bool>,
    pub error_modal_text: RwSignal<Option<String>>,
    /// Pending "you have an open transaction" prompt, or `None`. Set by any
    /// action that would strand a transaction — switching back to Auto-commit,
    /// closing the tab, disconnecting, or changing the tab's database — and
    /// cleared when the user picks. See [`TxPrompt`].
    pub tx_prompt: RwSignal<Option<TxPrompt>>,
    /// Pending yes/no confirmation, or `None`. The shared channel for "are you
    /// sure" — set it rather than adding another modal. See [`Confirm`].
    pub confirm: RwSignal<Option<Confirm>>,
    /// Query-plan (EXPLAIN) modal: open flag, the running/loaded state, the
    /// statement being explained (re-run when the Analyze toggle flips), and the
    /// Analyze toggle itself.
    pub plan_open: RwSignal<bool>,
    pub plan_state: RwSignal<PlanState>,
    pub plan_sql: RwSignal<String>,
    pub plan_analyze: RwSignal<bool>,
    /// Live Monitor modal: open flag, the watched `database.table` label, the
    /// polled result's column names (for rendering field diffs), the timestamped
    /// change log (oldest-first), and the latest poll error (if any). The poll
    /// loop lives in the app; the modal only renders these + closing sets
    /// `monitor_open` false, which stops the loop.
    pub monitor_open: RwSignal<bool>,
    pub monitor_title: RwSignal<Option<String>>,
    pub monitor_cols: RwSignal<Vec<String>>,
    pub monitor_log: RwSignal<Vec<MonitorEntry>>,
    pub monitor_error: RwSignal<Option<String>>,
    /// Poll interval in seconds (the popup's dropdown). Read by the poll loop on
    /// each re-arm, so a change takes effect on the next tick. Session-only.
    pub monitor_interval: RwSignal<u64>,
    /// ER-diagram modal: `Some(target)` opens it for that database/seed.
    pub erd: RwSignal<Option<ErdTarget>>,
    /// A run the write guard held back, or `None`. Set by
    /// [`TabsActions::run`]/[`TabsActions::run_all`] — which *are* the guarded
    /// run path — and rendered as the editor's guard bar. It lives here, not in
    /// the editor pane, because the guard belongs to the run action and the bar
    /// is only its view: the two closures that used to hold it inside the pane's
    /// view body meant every other caller of the run action had no guard at all.
    /// One at a time, since only one tab is on screen.
    pub run_guard: RwSignal<Option<RunGuard>>,
}

/// A run request the write guard held back, replayed verbatim on "Run anyway".
#[derive(Clone, Debug)]
pub enum PendingRun {
    Single(String),
    Batch(Vec<String>),
}

/// A held-back run: what to tell the user, and what to replay if they insist.
///
/// `pending: None` is the **hard block** — a write on a read-only connection —
/// and the guard bar then shows no "Run anyway", because the product
/// deliberately offers no override for it. `Some(..)` is a soft warning (the
/// missing-`WHERE` net, or `confirm_writes`).
#[derive(Clone, Debug)]
pub struct RunGuard {
    pub message: String,
    pub pending: Option<PendingRun>,
}

/// One entry in the Live Monitor's change log: a detected
/// [`schemaic_core::monitor::RowChange`] plus the
/// elapsed-since-start timestamp (`M:SS`) at which the monitor observed it.
#[derive(Clone, Debug)]
pub struct MonitorEntry {
    pub at: String,
    pub change: schemaic_core::monitor::RowChange,
}

/// Open the Live Monitor for a table on a connection — starts polling that table
/// and reveals the modal. Built in the app, invoked from the grid toolbar.
pub type MonitorFn = Rc<dyn Fn(u64, TableSource)>;

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
    /// The file-import modal (opened from a table's schema context menu).
    pub import: ImportUi,
    /// The schema-editing modals: the table designer and the DDL preview.
    pub ddl: DdlUi,
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
    /// Favorited (bookmarked) databases (persisted to `favorites.json`), keyed by
    /// `(conn_id, database)` in favorite order (oldest first); set from the schema
    /// tree's right-click menu, shown as a gold star and sorted to the top.
    pub db_favorites: RwSignal<Vec<FavoriteRule>>,
    /// Persist the favorites to disk (after a menu toggle).
    pub save_db_favorites: Rc<dyn Fn()>,
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
    // Reset the popup width to the default whenever the popup closes (covers every
    // close path: Escape/action, the root pointer-down, and the grid's dismiss), so
    // a menu that doesn't set a width gets 170.
    {
        let popup_width = ui.overlay.popup_width;
        create_effect(move |_| {
            if popup_menu.get().is_none() && popup_width.get_untracked() != 170.0 {
                popup_width.set(170.0);
            }
        });
    }
    let db_menu_open = ui.schema.db_menu_open;
    let schema_menu_open = ui.schema.schema_menu_open;
    // Panel visibility is owned by the app (loaded from / saved to disk), so the
    // layout is restored on the next launch.
    let schema_visible = ui.layout.schema_visible;
    let right_panel = ui.layout.right_panel;
    let window_focused = ui.layout.window_focused;
    // Global tab/find navigation keys (shared with the editor's own handler).
    let navkeys = NavKeys {
        tabs: ui.tabs_ui.tabs,
        active: ui.tabs_ui.active,
        active_conn: ui.conn.active_conn,
        find_open: ui.overlay.find_open,
        find_query: ui.overlay.find_query,
        add_tab: ui.tab_actions.add_tab.clone(),
        close_tab: ui.tab_actions.close_tab.clone(),
        reopen_closed: ui.tab_actions.reopen_closed_tab.clone(),
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
        find_overlay(ui.clone()),
        // Error modal + open-transaction prompt + the shared confirm share one
        // tuple element, for the same 16-arity reason as monitor/ERD below (and
        // with the same fill-only-when-open wrapper, or it would eat every click).
        {
            let err_open = ui.overlay.error_modal_open;
            let tx_prompt = ui.overlay.tx_prompt;
            let confirm = ui.overlay.confirm;
            let import_open = ui.import.target;
            let designer_open = ui.ddl.designer;
            let view_open = ui.ddl.view;
            let ddl_preview_open = ui.ddl.preview;
            stack((
                error_modal_overlay(ui.clone()),
                tx_prompt_overlay(ui.clone()),
                confirm_overlay(ui.clone()),
                import_view::import_overlay(ui.clone()),
                table_designer::table_designer_overlay(ui.clone()),
                view_editor::view_editor_overlay(ui.clone()),
                ddl_preview::ddl_preview_overlay(ui.clone()),
            ))
            .style(move |s| {
                if err_open.get()
                    || tx_prompt.get().is_some()
                    || confirm.get().is_some()
                    || import_open.get().is_some()
                    || designer_open.get().is_some()
                    || view_open.get().is_some()
                    || ddl_preview_open.get().is_some()
                {
                    s.absolute().inset(0.0)
                } else {
                    s
                }
            })
        },
        plan_overlay(ui.clone()),
        // Monitor + ER-diagram modals share one tuple element (the workspace stack
        // is at Floem's 16-arity `ViewTuple` limit). The wrapper must fill the
        // window when either is open — so their own `.absolute().inset(0)` resolves
        // against the root and the dim backdrop covers everything — but stay
        // out-of-flow (zero-size) when both are closed, or it would intercept every
        // click meant for the app beneath it.
        {
            let mon_open = ui.overlay.monitor_open;
            let erd_open = ui.overlay.erd;
            stack((monitor_overlay(ui.clone()), erd_overlay(ui.clone()))).style(move |s| {
                if mon_open.get() || erd_open.get().is_some() {
                    s.absolute().inset(0.0)
                } else {
                    s
                }
            })
        },
        term_settings_overlay(ui.clone()),
        ai_settings_overlay(ui.clone()),
        theme_settings_overlay(ui.clone()),
        help_overlay(ui.clone()),
        manage_modal(ui.clone()),
        // **Last on purpose.** A sibling paints in tuple order, so anything after
        // this would cover it — and the shared popup menu is opened from *inside*
        // modals too (the designer's type shortcut), where being painted behind
        // the panel and its backdrop made it invisible. It's the topmost surface
        // in the app, which is what a menu should be.
        popup_menu_overlay(ui),
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
    // Publish window focus (for the connection health poll). These two events
    // don't need keyboard focus, so they reach the root regardless of which
    // widget is active.
    .on_event_cont(EventListener::WindowGotFocus, move |_| {
        window_focused.set(true)
    })
    .on_event_cont(EventListener::WindowLostFocus, move |_| {
        window_focused.set(false)
    })
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
                        if schema_panel_allowed() {
                            schema_visible.update(|v| *v = !*v);
                        }
                        return EventPropagation::Stop;
                    }
                    if m.shift() && c.eq_ignore_ascii_case("a") {
                        if right_panel_allowed() {
                            right_panel.update(|p| {
                                *p = if matches!(*p, RightPanel::Ai) {
                                    RightPanel::None
                                } else {
                                    RightPanel::Ai
                                };
                            });
                        }
                        return EventPropagation::Stop;
                    }
                    if c == "`" {
                        if right_panel_allowed() {
                            right_panel.update(|p| {
                                *p = if matches!(*p, RightPanel::Terminal) {
                                    RightPanel::None
                                } else {
                                    RightPanel::Terminal
                                };
                            });
                        }
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
            // Floem labels are drag-selectable by default (`Selectable` = true),
            // which reads as web-y in a native app: every caption, header, tree row
            // and status string would highlight under the pointer. Turn it off for
            // *every* label in the tree — text selection belongs to real text
            // surfaces (`text_input`/`edit_field`, the SQL editor, the terminal's
            // own selection model), which are separate views and unaffected.
            // A class rule cascades to the whole subtree, and Floem's dropdown
            // popup carries the ambient context style into its overlay, so that
            // gets it too; a tooltip tip only inherits `TooltipClass`, so
            // `tooltip_style` repeats the rule for those.
            .class(LabelClass, |s| s.selectable(false))
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

    // Environment badge: a capsule filled with the active connection's identity
    // colour, sitting 20px right of the switcher and shown only when that
    // connection has an environment set. Rebuilds when the environment changes; the
    // fill re-reads the colour inside `.style` so a colour switch follows without a
    // rebuild. The `margin_left` lives on the capsule (not the wrapper) so the
    // empty/no-environment case leaves no gap after the switcher.
    let badge = dyn_container(
        move || active_conn_env(connections, active_conn),
        move |env| match env.badge_label() {
            Some(lbl) => container(
                text(lbl).style(|s| s.color(theme::env_badge_text()).font_size(theme::FONT_BODY)),
            )
            .style(move |s| {
                s.margin_left(20.0)
                    .padding_vert(5.0)
                    .padding_horiz(10.0)
                    .border_radius(5.0)
                    .background(active_conn_color(connections, active_conn))
            })
            .into_any(),
            None => empty().into_any(),
        },
    );

    // Left cluster (dot + switcher + environment badge) and the right glyph
    // cluster, pinned to opposite edges via `justify_between` (a lone flex-grow
    // spacer under-fills — see the schema title-row note). The dot's own
    // `margin_left(15)` sets the left inset.
    // The switcher's left inset matches `section_title`'s 12px, so it lines up
    // with "SCHEMA" in the panel below it. (There's no status dot ahead of it
    // any more — health is now told by the Disconnected notice alone.)
    let left = h_stack((
        container(switcher).style(|s| s.margin_left(12.0)),
        badge,
        disconnected_notice(conn_status, ui.conn_actions.recheck_conn.clone()),
    ))
    .style(|s| s.flex_row().items_center());
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

/// "Disconnected" + a Retry button, shown in the header while the last health
/// check failed.
///
/// This carries the whole connection-health signal now that the status dot is
/// gone: a healthy connection says nothing (the schema tree populating is the
/// proof), and a dead one states the problem and offers the fix. Retry is the
/// *immediate* path, not the only one — the app's health poll does re-check on a
/// timer, but `health::tick` backs off exponentially after consecutive failures
/// (and skips while the window is unfocused), so a server that came back can
/// take minutes to be noticed on its own. Hidden
/// entirely otherwise (`.hide()`/`.flex()`, not opacity, so it costs no layout
/// space when healthy).
fn disconnected_notice(conn_status: RwSignal<ConnStatus>, recheck: Rc<dyn Fn()>) -> impl IntoView {
    let label = text("Disconnected").style(|s| {
        s.font_size(TOOLBAR_FONT)
            .color(theme::error())
            .margin_left(15.0)
    });
    // Same chrome as the ER-diagram toolbar buttons (`control_surface`), so the
    // app has one button vocabulary rather than a bespoke one per surface.
    let retry = text("Retry")
        .on_click_stop(move |_| (recheck)())
        .style(|s| {
            control_surface(s)
                .font_size(TOOLBAR_FONT)
                .color(theme::text())
                .margin_left(15.0)
                .padding_horiz(10.0)
                .padding_vert(5.0)
                .hover(|s| s.background(theme::control_hover()))
        });
    h_stack((label, retry)).style(move |s| {
        let s = s.flex_row().items_center();
        if conn_status.get().is_down() {
            s.flex()
        } else {
            s.hide()
        }
    })
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

/// The active connection's environment (its top-bar badge classification), or
/// `Environment::None` when there's no active connection. Reactive — call inside
/// a `.style(…)`/accessor closure so a connection or environment change re-runs it.
pub(crate) fn active_conn_env(
    connections: RwSignal<Vec<Connection>>,
    active_conn: RwSignal<u64>,
) -> Environment {
    let id = active_conn.get();
    connections.with(|cs| {
        cs.iter()
            .find(|c| c.id == id)
            .map(|c| c.environment)
            .unwrap_or_default()
    })
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

/// A gold star for a favorited database, or a zero-footprint `empty()` when it
/// isn't favorited (so un-favorited rows render exactly as before). `key` yields
/// the `(conn_id, database)` to look up reactively; `ml`/`mr` are the star's left/
/// right margins (applied only when drawn). Rebuilds when the favorite state or
/// key changes. The star colour is themable, so it's read inside the style closure.
pub(crate) fn favorite_star(
    db_favorites: RwSignal<Vec<FavoriteRule>>,
    key: impl Fn() -> Option<(u64, String)> + 'static,
    size: f32,
    ml: f64,
    mr: f64,
) -> impl IntoView {
    dyn_container(
        move || {
            key()
                .map(|(cid, db)| {
                    db_favorites.with(|r| schemaic_core::favorite::is_favorite(r, cid, &db))
                })
                .unwrap_or(false)
        },
        move |fav| {
            if fav {
                icons::icon(icons::STAR, size)
                    .style(move |s| {
                        s.color(theme::favorite_star())
                            .flex_shrink(0.0_f32)
                            .margin_left(ml)
                            .margin_right(mr)
                    })
                    .into_any()
            } else {
                empty().into_any()
            }
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
/// Whether the schema panel currently fits beside the center — window width ≥
/// (schema + center) min widths. Reactive on `window_size`. Below this the panel
/// is force-hidden and its toggle is a no-op. `(0,0)` (pre-first-resize) counts
/// as allowed so nothing is locked at startup before the first resize fires.
pub(crate) fn schema_panel_allowed() -> bool {
    // Outside `[1, threshold)` = pre-first-resize `(0,0)` or wide enough.
    let ww = window_size().get().0;
    !(1.0..PANELS_MIN_SCHEMA_W).contains(&ww)
}

/// Whether the right (AI/terminal/history) panel currently fits beside the schema
/// panel and the center — window width ≥ all three min widths. Reactive on
/// `window_size`.
pub(crate) fn right_panel_allowed() -> bool {
    let ww = window_size().get().0;
    !(1.0..PANELS_MIN_FULL_W).contains(&ww)
}

#[allow(clippy::too_many_arguments)]
fn h_resize_handle(
    from_right: bool,
    dim: RwSignal<f64>,
    // Effective (clamped) panel width → where the handle sits. May be less than
    // `dim` when the window is too narrow to honor the full intended width.
    edge: impl Fn() -> f64 + Copy + 'static,
    visible: impl Fn() -> bool + Copy + 'static,
    // Owned by the caller so the panel wrapper can drop its width transition while
    // dragging (the transition is for the collapse slide; during a resize it just
    // makes the width lag the pointer).
    dragging: RwSignal<bool>,
    // Drag clamp: floor `min_w`, ceiling `max_w()` (reactive — leaves the center +
    // the opposite panel their minimums).
    min_w: f64,
    max_w: impl Fn() -> f64 + Copy + 'static,
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
            let inset = edge() - RESIZE_HIT / 2.0;
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
                    // Floor at `min_w`; ceiling `max_w()` keeps the center (and the
                    // opposite panel) at their minimums so a drag can't swallow them.
                    let hi = max_w().max(min_w);
                    dim.update(|w| *w = (*w + d).clamp(min_w, hi));
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
    // Drag clamp: floor `min_h` (query editor min), ceiling `max_h()` (reactive —
    // leaves the results grid its minimum height).
    min_h: f64,
    max_h: impl Fn() -> f64 + Copy + 'static,
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
                    dim.update(|h| *h = (*h + d).clamp(min_h, max_h().max(min_h)));
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

    // Effective panel widths: 0 when hidden or locked away by the responsive
    // breakpoints, else the intended width clamped so the center keeps
    // `CENTER_MIN_W` (the right panel yields against the schema *minimum*; the
    // schema then yields against the right panel's *effective* width). The stored
    // `schema_w`/`right_w` are the user's intent and never mutated here, so a panel
    // restores to its full width when the window grows back.
    let eff_right_w = move || {
        if right_panel.get() == RightPanel::None || !right_panel_allowed() {
            return 0.0;
        }
        let ww = window_size().get().0;
        if ww < 1.0 {
            return right_w.get();
        }
        right_w.get().clamp(
            RIGHT_MIN_W,
            (ww - CENTER_MIN_W - SCHEMA_MIN_W).max(RIGHT_MIN_W),
        )
    };
    let eff_schema_w = move || {
        if !schema_visible.get() || !schema_panel_allowed() {
            return 0.0;
        }
        let ww = window_size().get().0;
        if ww < 1.0 {
            return schema_w.get();
        }
        schema_w.get().clamp(
            SCHEMA_MIN_W,
            (ww - CENTER_MIN_W - eff_right_w()).max(SCHEMA_MIN_W),
        )
    };

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
        s.width(eff_schema_w())
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
        s.width(eff_right_w())
    });

    // Drag handles overlay the panel boundaries (absolute → no layout impact).
    // Order them last so they paint over the panels' 1px separator lines.
    // Double-click resets to the hardcoded defaults; drag-end/reset persists.
    let commit = ui_persist.clone();
    let schema_handle = h_resize_handle(
        false,
        schema_w,
        eff_schema_w,
        move || schema_visible.get() && schema_panel_allowed(),
        schema_dragging,
        SCHEMA_MIN_W,
        // Leave the center + the right panel's effective width.
        move || window_size().get().0 - CENTER_MIN_W - eff_right_w(),
        theme::SCHEMA_W,
        commit,
    );
    let commit = ui_persist.clone();
    let right_handle = h_resize_handle(
        true,
        right_w,
        eff_right_w,
        move || right_panel.get() != RightPanel::None && right_panel_allowed(),
        right_dragging,
        RIGHT_MIN_W,
        // Leave the center + the schema panel at its minimum (it yields as needed).
        move || window_size().get().0 - CENTER_MIN_W - SCHEMA_MIN_W,
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
    let conn_status = ui.conn.conn_status;
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
    // The active tab's SQL dialect (MySQL/PostgreSQL), from its connection's
    // `db_type` — drives completion + diagnostics parsing. Same derivation shape as
    // `read_only`.
    let dialect = create_memo(move |_| {
        let id = active.get();
        let cid = tabs.with(|v| v.iter().find(|t| t.id == id).map(|t| t.conn_id.get()));
        match cid {
            Some(cid) => connections
                .with(|cs| {
                    cs.iter()
                        .find(|c| c.id == cid)
                        .map(|c| SqlDialect::from_db_type(&c.db_type))
                })
                .unwrap_or_default(),
            None => SqlDialect::default(),
        }
    });
    let live_validate = ui.layout.live_validate;
    let validate_stmt = ui.tab_actions.validate_stmt.clone();
    let run = ui.tab_actions.run.clone();
    let run_all = ui.tab_actions.run_all.clone();
    let run_anyway = ui.tab_actions.run_anyway.clone();
    let run_guard = ui.overlay.run_guard;
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
    let popup_width = ui.overlay.popup_width;
    let editor_h = ui.layout.editor_h;
    let editor_collapsed = ui.layout.editor_collapsed;
    // A width persisted under an older, looser floor could be below the current
    // query-editor minimum — lift it once on build (render clamps widths live, but
    // the editor height has no such render-time clamp).
    if editor_h.get_untracked() < QUERY_MIN_H {
        editor_h.set(QUERY_MIN_H);
    }
    // The active tab, resolved on demand (`Tab` is `Copy`).
    let active_tab = move || {
        let id = active.get_untracked();
        tabs.with_untracked(|v| v.iter().find(|t| t.id == id).copied())
    };
    // RESULTS "expand" toggle: flip the collapsed flag (editor height 0↔`editor_h`,
    // instant — an animated in-flow height reflows the whole grid per frame, which
    // never stayed smooth; not worth it). Maximize is per-tab: flip the live mirror
    // AND persist it onto the active tab so switching tabs restores each one's state.
    let toggle_collapse: Rc<dyn Fn()> = Rc::new(move || {
        let v = !editor_collapsed.get_untracked();
        editor_collapsed.set(v);
        if let Some(tab) = active_tab() {
            tab.results_maximized.set(v);
        }
    });
    // On tab switch, mirror the newly-active tab's stored maximize state into the
    // live render flag. Tracks `active` only (the toggle writes both, so this doesn't
    // need to react to `results_maximized`); guarded so a redundant set doesn't churn
    // the collapse-dependent views.
    create_effect(move |_| {
        let id = active.get();
        let stored =
            tabs.with_untracked(|v| v.iter().find(|t| t.id == id).map(|t| t.results_maximized));
        if let Some(sig) = stored {
            let m = sig.get_untracked();
            if editor_collapsed.get_untracked() != m {
                editor_collapsed.set(m);
            }
        }
    });
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
    let export_file = ui.tab_actions.export_file.clone();
    let apply_view = ui.tab_actions.apply_view.clone();
    let follow_fk = ui.tab_actions.open_table_filtered.clone();
    let open_monitor = ui.tab_actions.open_monitor.clone();
    let ai_fill = ui.tab_actions.ai_fill.clone();
    let ai_seed = ui.tab_actions.ai_seed.clone();
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
        active_conn: ui.conn.active_conn,
        find_open: ui.overlay.find_open,
        find_query: ui.overlay.find_query,
        add_tab: ui.tab_actions.add_tab.clone(),
        close_tab: ui.tab_actions.close_tab.clone(),
        reopen_closed: ui.tab_actions.reopen_closed_tab.clone(),
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

    // "Create view" from the editor's right-click: the statement becomes a new
    // view's body, in the tab's active database. Built here because the pane
    // takes callbacks, not the whole `Ui`.
    let create_view: Rc<dyn Fn(String)> = {
        let ui = ui.clone();
        Rc::new(move |select: String| {
            if let Some(db) = active_db.get_untracked() {
                view_editor::open_from_query(&ui, &db, &select);
            }
        })
    };

    // Editor area: the active tab's query editor — or, while that tab is
    // "flashing" closed, a solid placeholder of identical size, so nothing
    // below it shifts.
    let editor_area = dyn_container(
        move || (active.get(), flashing.get() == Some(active.get())),
        move |(id, is_flashing)| {
            if is_flashing {
                return editor_placeholder(editor_h, editor_collapsed).into_any();
            }
            match tabs.with_untracked(|v| v.iter().find(|t| t.id == id).copied()) {
                Some(tab) => query_pane(QueryPaneParams {
                    query: tab.query,
                    cursor_offset: tab.cursor_offset,
                    goto_open: tab.goto_open,
                    jump_offset: tab.jump_offset,
                    syntax: tab.diagnostics,
                    results: tab.results,
                    run: run.clone(),
                    run_all: run_all.clone(),
                    run_guard,
                    run_anyway: run_anyway.clone(),
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
                    editor_collapsed,
                    active_db,
                    dialect,
                    active_db_menu_open,
                    active_db_anchor,
                    read_only,
                    live_validate,
                    validate_stmt: validate_stmt.clone(),
                    popup_menu: popup,
                    popup_anchor,
                    popup_width,
                    open_plan: open_plan.clone(),
                    create_view: create_view.clone(),
                    nav: navkeys.clone(),
                    zoom: tab.font_zoom,
                    conn_status,
                })
                .into_any(),
                None => editor_placeholder(editor_h, editor_collapsed).into_any(),
            }
        },
    )
    .style(|s| {
        // No floor here — the child `query_pane` pins its own height (`editor_h`, or
        // 0 when collapsed) and is `flex_shrink(0)`, so this wrapper hugs it and can
        // reach 0 on collapse. (The divider clamps `editor_h ≥ QUERY_MIN_H` when open.)
        s.width_full()
            .flex_shrink(0.0_f32)
            .flex_col()
            .min_width(0.0)
            .min_height(0.0)
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
                editor_collapsed,
                toggle_collapse.clone(),
                GridCtx {
                    source: tab.source,
                    highlight_col: tab.highlight_col,
                    base_sql: tab.base_sql,
                    grid_query: tab.grid_query,
                    view_err: tab.view_err,
                    load_gen: tab.load_gen,
                    apply_view: apply_view.clone(),
                    db_nodes,
                    connections,
                    active_conn,
                    popup,
                    popup_anchor,
                    popup_width,
                    summarize: summarize.clone(),
                    follow_fk: follow_fk.clone(),
                    open_monitor: open_monitor.clone(),
                    ai_fill: ai_fill.clone(),
                    ai_seed: ai_seed.clone(),
                    dismiss: dismiss_menus.clone(),
                    commit: commit_edits.clone(),
                    export_file: export_file.clone(),
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
            .min_height(RESULTS_MIN_H)
            .min_width(0.0)
    });

    // Divider between editor and results, offset past the tab bar. Double-click
    // resets to the default editor height; drag-end/reset persists the layout.
    // Ceiling leaves the results grid `RESULTS_MIN_H` within the editor+results
    // region (window minus header/footer/tab-bar).
    let split_handle = v_resize_handle(
        TAB_BAR_H,
        editor_h,
        QUERY_MIN_H,
        move || {
            let wh = window_size().get().1;
            if wh < 1.0 {
                return f64::INFINITY;
            }
            wh - theme::HEADER_H - theme::FOOTER_H - TAB_BAR_H - RESULTS_MIN_H
        },
        EDITOR_H,
        ui.persist_layout.clone(),
    );
    // Hide the divider while the editor is collapsed — there's nothing to resize,
    // and its position tracks `editor_h` (unchanged during collapse), so it'd
    // otherwise float over the grid.
    let split_handle =
        split_handle.style(move |s| if editor_collapsed.get() { s.hide() } else { s });

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
            .min_width(CENTER_MIN_W)
    })
}

// The RESULTS pane for one tab. Single runs use the legacy single-grid view
// (`results`); a Run Everything batch shows a result-tab strip over the active
// statement's grid (`result_tabs` non-empty).
#[allow(clippy::too_many_arguments)]
fn results_section(
    results: RwSignal<QueryState>,
    result_tabs: RwSignal<Vec<ResultPanel>>,
    active_result: RwSignal<usize>,
    cancel: Rc<dyn Fn()>,
    // Editor-collapse toggle: the intent flag (drives the expand/shrink icon) and
    // the action that flips it + kicks off the tween.
    editor_collapsed: RwSignal<bool>,
    toggle_collapse: Rc<dyn Fn()>,
    gctx: GridCtx,
) -> impl IntoView {
    // Find-bar + error-bar signals (Copy) — captured before `gctx` moves into `body`.
    let (find_open, find_query, find_step) = (gctx.find_open, gctx.find_query, gctx.find_step);
    let (find_total, find_pos, find_more) = (gctx.find_total, gctx.find_pos, gctx.find_more);
    let (commit_err, error_open, error_text) = (gctx.commit_err, gctx.error_open, gctx.error_text);
    let view_err = gctx.view_err;
    // Live Monitor: watch the tab's source table. Captured before `gctx` moves.
    let open_monitor = gctx.open_monitor.clone();
    let (monitor_source, monitor_conn) = (gctx.source, gctx.conn_id);
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

    // Title row: "RESULTS" left; a Live-Monitor button + expand/shrink toggle right
    // (same widget + spacing as the Schema/AI title-bar icons — monitor `mr=2`,
    // toggle `mr=7` gives the 12px inter-icon gap and 12px edge inset). The toggle
    // swaps its glyph via `dyn_container` (a transform-transition on a small svg is
    // unreliable — see themes gotchas).
    let monitor_btn = {
        let open_monitor = open_monitor.clone();
        let enabled = move || monitor_source.get().is_some();
        toolbar_icon(icons::ACTIVITY, 5.0, 2.0, enabled, move || {
            if let Some(src) = monitor_source.get_untracked() {
                (open_monitor)(monitor_conn.get_untracked(), src);
            }
        })
    };
    let toggle_btn = dyn_container(
        move || editor_collapsed.get(),
        move |collapsed| {
            let markup = if collapsed {
                icons::SHRINK
            } else {
                icons::EXPAND
            };
            let t = toggle_collapse.clone();
            toolbar_icon(markup, 5.0, 7.0, || true, move || (t)()).into_any()
        },
    );
    let icons_group = h_stack((monitor_btn, toggle_btn))
        .style(|s| s.flex_row().items_start().flex_shrink(0.0_f32));
    let title_row = h_stack((section_title("RESULTS"), icons_group))
        .style(|s| s.width_full().flex_row().items_start().justify_between());

    let panel = v_stack((title_row, body)).style(|s| {
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
        grid_error_bar(commit_err, view_err, error_open, error_text),
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
    /// Render in the app's monospace face ([`MONO_FAMILY`]) instead of IBM Plex
    /// Sans — for a field whose content is *code* and wants column alignment
    /// (the DDL preview's generated SQL). Doesn't change the line height, so the
    /// auto-grow box math is unaffected.
    pub mono: bool,
    pub border_radius: f32,
    /// Read-only: no text edits (still handles Enter/Escape). Suppresses autofocus.
    pub read_only: bool,
    /// Fixed box height. `None` = derive from content (auto-grow for multiline).
    pub height: Option<f64>,
    /// Reactive override for the multiline auto-grow cap (rows). `None` =
    /// `CHAT_MAX_ROWS`. A signal so the cap can follow a resizing container (the
    /// value viewer caps at the results-panel height).
    pub max_rows: Option<RwSignal<usize>>,
    /// Multiline only: suppress soft word-wrap (long lines scroll horizontally
    /// instead). Keeps the box height a function of the *logical* line count, so
    /// content whose line count is constant (e.g. a row's JSON — one key per line)
    /// doesn't change height as values vary. Default `false` (wrap, as before).
    pub no_wrap: bool,
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
    /// Tab (e.g. accept the command-palette ghost completion). When set, the key
    /// is consumed here instead of inserting a tab / moving focus.
    pub on_tab: Option<Rc<dyn Fn()>>,
    /// A "move caret to end" pulse: when this signal changes, the field refocuses
    /// and drops the caret at the end of the text. Used after a programmatic
    /// completion (the palette's command → argument transition) so typing
    /// continues after the inserted text, not at the start.
    pub caret_end: Option<RwSignal<u64>>,
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
            mono: false,
            border_radius: 6.0,
            read_only: false,
            height: None,
            max_rows: None,
            no_wrap: false,
            text_color: None,
            placeholder_color: None,
            border_color: None,
            on_submit: None,
            on_escape: None,
            on_blur: None,
            on_arrow_up: None,
            on_arrow_down: None,
            on_tab: None,
            caret_end: None,
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
        mono,
        border_radius,
        read_only,
        height,
        max_rows,
        no_wrap,
        text_color,
        placeholder_color,
        border_color,
        on_submit,
        on_escape,
        on_blur,
        on_arrow_up,
        on_arrow_down,
        on_tab,
        caret_end,
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
    let wrap = if multiline && !no_wrap {
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
    let tab = on_tab.clone();
    let editor = text_editor_keys(text_sig.get_untracked(), move |editor_sig, kp, mods| {
        if let Some(esc) = &escape
            && matches!(kp.key, KeyInput::Keyboard(Key::Named(NamedKey::Escape), _))
        {
            (esc)();
            return CommandExecuted::Yes;
        }
        // Tab accepts an external completion (the palette ghost) when opted in.
        if let Some(cb) = &tab
            && matches!(kp.key, KeyInput::Keyboard(Key::Named(NamedKey::Tab), _))
            && !mods.shift()
        {
            (cb)();
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
            .font_family(vec![FamilyOwned::Name(
                if mono { MONO_FAMILY } else { "IBM Plex Sans" }.to_string(),
            )]);
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

    // Caret-to-end pulse: refocus and drop the caret at the end whenever the
    // signal changes (skipping the initial run). Deferred a tick so the
    // signal→doc reconcile above has applied the new text first.
    if let Some(pulse) = caret_end {
        let ed_ce = ed.clone();
        create_effect(move |prev: Option<u64>| {
            let v = pulse.get();
            if prev.is_some_and(|p| p != v) {
                let ed2 = ed_ce.clone();
                floem::action::exec_after(std::time::Duration::ZERO, move |_| {
                    if let Some(Some(vid)) = ed2.editor_view_id.try_get_untracked() {
                        vid.request_focus();
                    }
                    let len = ed2.doc().text().to_string().len();
                    ed2.cursor.update(|c| c.set_offset(len, false, false));
                });
            }
            v
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
                                // Override the field's text (I-beam) cursor — the ×
                                // is a button, not editable text.
                                .cursor(CursorStyle::Default)
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

/// A clickable status-bar segment that opens a `menu_panel` popup centred above
/// it (the Tabs/Spaces, AI-model and AI-effort menus, which share the one popup
/// channel). `owner` disambiguates which segment owns the open popup: a second
/// click on the *same* segment toggles it shut, while clicking a different one
/// switches menus. Its window rect is tracked (its x shifts as segments to its
/// left change width) so the popup can centre on it.
#[allow(clippy::too_many_arguments)]
fn status_menu_seg(
    label: impl Fn() -> String + 'static,
    owner: u8,
    build_entries: impl Fn() -> Vec<MenuEntry> + 'static,
    menu_owner: RwSignal<u8>,
    popup_menu: RwSignal<Option<Vec<MenuEntry>>>,
    popup_anchor: RwSignal<Option<PopupAnchor>>,
    popup_width: RwSignal<f64>,
    margin: f64,
) -> impl IntoView {
    let origin: RwSignal<(f64, f64)> = RwSignal::new((0.0, 0.0));
    let size: RwSignal<(f64, f64)> = RwSignal::new((0.0, 0.0));
    let build = Rc::new(build_entries);
    dyn_container(label, move |s| {
        text(s)
            .style(|s| s.font_size(theme::FONT_STATUS))
            .into_any()
    })
    .on_move(move |p| origin.set((p.x, p.y)))
    .on_resize(move |r| size.set((r.width(), r.height())))
    // Stop the pointer-down so the workspace-root "close on down" handler doesn't
    // fire for our own clicks (else down closes and up reopens — never toggling).
    .on_event_stop(EventListener::PointerDown, |_| {})
    .on_click_stop(move |_| {
        if popup_menu.get_untracked().is_some() && menu_owner.get_untracked() == owner {
            popup_menu.set(None);
            return;
        }
        menu_owner.set(owner);
        let (ox, _oy) = origin.get_untracked();
        let (sw, _sh) = size.get_untracked();
        popup_anchor.set(Some(PopupAnchor::AboveFooter(ox, ox + sw)));
        popup_width.set(125.0);
        popup_menu.set(Some((build)()));
    })
    .style(move |s| {
        s.margin_left(margin)
            .items_center()
            .color(theme::status_text())
            .hover(|s| s.color(theme::chip_active()))
    })
}

/// Wrap a left status-bar segment so it auto-hides once its right edge comes
/// within `FOOTER_COLLAPSE_GAP` px of the right-hand icon group (`ai_x` = the AI
/// icon's left edge, both in window coords). It tracks its own right edge, frozen
/// while it's hidden (updates only while shown) so the show/hide test reads a
/// stable full-layout position and can't oscillate. Segments hide right-to-left
/// (the rightmost's edge is largest, so it crosses the threshold first) and
/// reappear as the window widens.
fn collapsing_seg(view: impl IntoView + 'static, ai_x: RwSignal<f64>) -> impl IntoView {
    let x = RwSignal::new(0.0_f64);
    let w = RwSignal::new(0.0_f64);
    let edge = RwSignal::new(0.0_f64);
    // Whether the segment is currently shown, read untracked in the geometry
    // handlers so a hidden segment freezes its `edge` (no reactive cycle).
    let is_shown = move || {
        let ax = ai_x.get_untracked();
        ax < 1.0 || edge.get_untracked() + FOOTER_COLLAPSE_GAP <= ax
    };
    container(view)
        .on_move(move |p| {
            x.set(p.x);
            if w.get_untracked() > 0.0 && is_shown() {
                edge.set(p.x + w.get_untracked());
            }
        })
        .on_resize(move |r| {
            let cw = r.width();
            w.set(cw);
            if cw > 0.0 && is_shown() {
                edge.set(x.get_untracked() + cw);
            }
        })
        .style(move |s| {
            let ax = ai_x.get();
            if ax >= 1.0 && edge.get() + FOOTER_COLLAPSE_GAP > ax {
                s.hide()
            } else {
                s
            }
        })
}

fn footer(ui: Ui) -> impl IntoView {
    let schema_visible = ui.layout.schema_visible;
    let right_panel = ui.layout.right_panel;
    let connections = ui.conn.connections;
    let active_conn = ui.conn.active_conn;
    let tabs = ui.tabs_ui.tabs;
    let active = ui.tabs_ui.active;
    let soft_tabs = ui.layout.soft_tabs;
    let tab_width = ui.layout.tab_width;
    let word_wrap = ui.layout.word_wrap;
    let popup_menu = ui.overlay.popup_menu;
    let popup_anchor = ui.overlay.popup_anchor;
    let popup_width = ui.overlay.popup_width;
    let toggle_read_only = ui.conn_actions.toggle_read_only.clone();
    let resources = ui.resources;
    // Which status-bar segment owns the shared popup (0 none / 1 tabs / 2 model /
    // 3 effort) — lets a second click on the same segment toggle it shut.
    let menu_owner: RwSignal<u8> = RwSignal::new(0);
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
    // Live count of diagnostics in the active tab's SQL — *read* from the editor
    // pane's debounced analysis, not recomputed. Doing the analysis here meant a
    // full catalog rebuild and a full parse of the document on every keystroke,
    // undebounced, defeating the 120 ms debounce the pane implements for exactly
    // this call (measured at 20 ms per keypress on a 500-table schema).
    let warn_count = create_memo(move |_| {
        let id = active.get();
        tabs.with(|v| {
            v.iter()
                .find(|t| t.id == id)
                .map(|t| t.diagnostics.with(|d| d.len()))
        })
        .unwrap_or(0)
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
    // other; clicking the active one hides it (right column freed). A no-op while
    // the window is too narrow for the right panel (it's locked hidden).
    let set_right = move |target: RightPanel| {
        if !right_panel_allowed() {
            return;
        }
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
    // Active state reflects *effective* visibility (intent AND the window is wide
    // enough), so a panel locked away by a narrow window reads as inactive; its
    // toggle is a no-op until the window grows back.
    let schema_icon = toggle_icon(
        icons::FOLDER_TREE,
        move || schema_visible.get() && schema_panel_allowed(),
        move || {
            if schema_panel_allowed() {
                schema_visible.update(|v| *v = !*v);
            }
        },
    )
    .style(|s| s.margin_left(5.0));
    // The AI icon's left edge (window x) is the reference the left cluster
    // collapses against — it's the leftmost thing in the right-pinned group, so it
    // marches left as the window narrows.
    let ai_x = RwSignal::new(0.0_f64);
    let right_group = h_stack((
        toggle_icon_view(
            icons::icon_wh(icons::AI_LOGO, 16.0, 10.0).style(|s| s.flex_shrink(0.0_f32)),
            move || right_panel.get() == RightPanel::Ai && right_panel_allowed(),
            move || set_right(RightPanel::Ai),
        )
        .on_move(move |p| ai_x.set(p.x)),
        toggle_icon(
            icons::TIMELINE,
            move || right_panel.get() == RightPanel::History && right_panel_allowed(),
            move || set_right(RightPanel::History),
        ),
        toggle_icon(
            icons::TERMINAL,
            move || right_panel.get() == RightPanel::Terminal && right_panel_allowed(),
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
    let tabs_seg = status_menu_seg(
        move || {
            format!(
                "{}: {}",
                if soft_tabs.get() { "Spaces" } else { "Tabs" },
                tab_width.get()
            )
        },
        1,
        move || {
            let soft = soft_tabs.get_untracked();
            let w = tab_width.get_untracked();
            let mut entries = vec![
                if soft {
                    MenuEntry::action_colored("Spaces", theme::chip_active, move || {
                        soft_tabs.set(true)
                    })
                } else {
                    MenuEntry::action("Spaces", move || soft_tabs.set(true))
                },
                if !soft {
                    MenuEntry::action_colored("Tabs", theme::chip_active, move || {
                        soft_tabs.set(false)
                    })
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
            entries
        },
        menu_owner,
        popup_menu,
        popup_anchor,
        popup_width,
        15.0,
    );
    // Word wrap — click toggles it.
    let wrap_seg = dyn_container(
        move || word_wrap.get(),
        move |w| {
            text(if w { "Wrap" } else { "No wrap" })
                .style(|s| s.font_size(theme::FONT_STATUS))
                .into_any()
        },
    )
    .on_click_stop(move |_| word_wrap.update(|w| *w = !*w))
    .style(|s| {
        s.margin_left(15.0)
            .items_center()
            .color(theme::status_text())
            .hover(|s| s.color(theme::chip_active()))
    });
    // Warnings: amber triangle + amber count (click jumps to the first warning,
    // hovering to the brighter amber), or a green check (no text, inert) when clean.
    // Colour lives on the container so the icon (currentColor) + count inherit it.
    let warn_seg = dyn_container(
        move || warn_count.get(),
        move |n| {
            if n == 0 {
                icons::icon(icons::CIRCLE_CHECK, 15.0).into_any()
            } else {
                h_stack((
                    icons::icon(icons::TRIANGLE_ALERT, 16.0),
                    text(n.to_string()).style(|s| s.margin_left(5.0).font_size(theme::FONT_STATUS)),
                ))
                .style(|s| s.flex_row().items_center())
                .into_any()
            }
        },
    )
    .on_click_stop(move |_| {
        // Jump the editor to the first diagnostic (no-op when there are none) —
        // from the same published list the count came from, rather than a third
        // full analysis of the document.
        let id = active.get_untracked();
        tabs.with_untracked(|v| {
            if let Some(t) = v.iter().find(|t| t.id == id)
                && let Some(off) = t
                    .diagnostics
                    .with_untracked(|d| d.first().map(|d| d.range.0))
            {
                t.jump_offset.set(Some(off));
            }
        });
    })
    .style(move |s| {
        let s = s.margin_left(40.0).items_center();
        if warn_count.get() == 0 {
            s.color(theme::status_ok())
        } else {
            s.color(theme::status_warn())
                .hover(|s| s.color(theme::status_warn_hover()))
        }
    });
    // Read-only vs write mode (the active connection's setting). Read-only reads
    // as normal status text; write mode is amber so it stands out. Click toggles
    // it — a shortcut for the Manage-Connections read-only switch. Colour + hover
    // sit on the container (state-dependent) so the label inherits both: read-only
    // hovers to the shared accent, write mode to a brighter amber.
    let ro_seg = dyn_container(
        move || read_only.get(),
        move |ro| {
            text(if ro { "Read only" } else { "Write mode" })
                .style(|s| s.font_size(theme::FONT_STATUS))
                .into_any()
        },
    )
    .on_click_stop(move |_| {
        let id = active.get_untracked();
        let cid = tabs.with_untracked(|v| {
            v.iter()
                .find(|t| t.id == id)
                .map(|t| t.conn_id.get_untracked())
        });
        if let Some(cid) = cid {
            toggle_read_only(cid);
        }
    })
    .style(move |s| {
        let ro = read_only.get();
        // Called inside this closure so both follow a live theme switch.
        let base = if ro {
            theme::status_text()
        } else {
            theme::status_warn()
        };
        let hover = if ro {
            theme::chip_active()
        } else {
            theme::status_warn_hover()
        };
        s.margin_left(15.0)
            .items_center()
            .color(base)
            .hover(move |s| s.color(hover))
    });
    // ── Manual-transaction cluster ───────────────────────────────────────────
    // Mode ("Auto-commit" / "Manual"), then — only while a transaction is open —
    // the pill and its Commit / Rollback actions. Its own footer section: 40px
    // clear on each side (the gap the bar uses between sections, e.g. before the
    // diagnostics check and before CPU), 15px between the items inside it. The
    // transaction controls are a a set of related controls, not one more status
    // reading, so they shouldn't blend into the run of segments around them.
    let active_tab = move || {
        let id = active.get();
        tabs.with(|v| v.iter().find(|t| t.id == id).copied())
    };
    let tx_mode = create_memo(move |_| active_tab().map(|t| t.tx_mode.get()).unwrap_or_default());
    let tx_state = create_memo(move |_| active_tab().map(|t| t.tx.get()).unwrap_or_default());

    let set_tx_mode = ui.tab_actions.set_tx_mode.clone();
    let mode_seg = dyn_container(
        move || tx_mode.get(),
        move |m| {
            text(m.label())
                .style(|s| s.font_size(theme::FONT_STATUS))
                .into_any()
        },
    )
    .on_click_stop(move |_| {
        let id = active.get_untracked();
        // Flipping out of Manual with a transaction open is guarded app-side —
        // it raises the prompt instead of silently discarding the work.
        let next = match tx_mode.get_untracked() {
            TxMode::Auto => TxMode::Manual,
            TxMode::Manual => TxMode::Auto,
        };
        (set_tx_mode)(id, next);
    })
    .style(move |s| {
        // Manual is a held state worth noticing; Auto is the quiet default.
        let manual = tx_mode.get().is_manual();
        let base = if manual {
            theme::tx_open()
        } else {
            theme::status_text()
        };
        let hover = if manual {
            theme::tx_open_hover()
        } else {
            theme::chip_active()
        };
        // 40px: opens the transaction section.
        s.margin_left(40.0)
            .items_center()
            .color(base)
            .hover(move |s| s.color(hover))
    });

    // "Tx open · N stmts" — or why it can't go forward. Hidden when idle.
    let tx_pill = dyn_container(
        move || tx_state.get(),
        move |st| {
            text(schemaic_core::tx::pill_text(st).unwrap_or_default())
                .style(|s| s.font_size(theme::FONT_STATUS))
                .into_any()
        },
    )
    .style(move |s| {
        let st = tx_state.get();
        let s = s.margin_left(15.0).items_center().color(match st {
            TxState::Poisoned { .. } | TxState::Lost => theme::tx_danger(),
            _ => theme::tx_open(),
        });
        if st.is_open() || matches!(st, TxState::Lost) {
            s
        } else {
            s.hide()
        }
    });

    // Commit / Rollback. Commit disappears on an aborted transaction — Postgres
    // turns COMMIT into a ROLLBACK there, so offering it would be a lie.
    let commit_tx = ui.tab_actions.commit_tx.clone();
    let rollback_tx = ui.tab_actions.rollback_tx.clone();
    // Plain text segments like everything else in the bar (no padding or pill
    // chrome), so `margin_left(15)` is a true 15px gap matching the rest.
    let tx_action = move |label: &'static str,
                          color: fn() -> Color,
                          hover: fn() -> Color,
                          visible: Box<dyn Fn() -> bool>,
                          act: Rc<dyn Fn(usize)>| {
        text(label)
            .on_click_stop(move |_| (act)(active.get_untracked()))
            .style(move |s| {
                let s = s
                    .margin_left(15.0)
                    .items_center()
                    .font_size(theme::FONT_STATUS)
                    .color(color())
                    .hover(move |s| s.color(hover()));
                if visible() { s } else { s.hide() }
            })
    };
    let commit_seg = tx_action(
        "Commit",
        theme::tx_commit,
        theme::tx_commit_hover,
        Box::new(move || tx_state.get().can_commit()),
        commit_tx,
    );
    let rollback_seg = tx_action(
        "Rollback",
        theme::tx_rollback,
        theme::tx_rollback_hover,
        Box::new(move || tx_state.get().can_rollback()),
        rollback_tx,
    );

    // AI model + effort: click each to pick from the AI-panel options; the active
    // one is tinted the chip accent. Opens the AI section 40px after the
    // transaction controls, with CPU then RAM after (40px from effort).
    let model_seg = status_menu_seg(
        move || ai_model.get().label().to_string(),
        2,
        move || {
            let cur = ai_model.get_untracked().cli();
            AiModel::ALL
                .into_iter()
                .map(|m| {
                    if m.cli() == cur {
                        MenuEntry::action_colored(m.label(), theme::chip_active, move || {
                            ai_model.set(m)
                        })
                    } else {
                        MenuEntry::action(m.label(), move || ai_model.set(m))
                    }
                })
                .collect()
        },
        menu_owner,
        popup_menu,
        popup_anchor,
        popup_width,
        40.0,
    );
    let effort_seg = status_menu_seg(
        move || ai_effort.get().label().to_string(),
        3,
        move || {
            let cur = ai_effort.get_untracked().cli();
            AiEffort::ALL
                .into_iter()
                .map(|e| {
                    if e.cli() == cur {
                        MenuEntry::action_colored(e.label(), theme::chip_active, move || {
                            ai_effort.set(e)
                        })
                    } else {
                        MenuEntry::action(e.label(), move || ai_effort.set(e))
                    }
                })
                .collect()
        },
        menu_owner,
        popup_menu,
        popup_anchor,
        popup_width,
        15.0,
    );
    let cpu_seg = dyn_container(
        move || resources.get().cpu_label(),
        move |c| footer_text(format!("CPU: {c}")),
    )
    .style(|s| s.margin_left(40.0));
    let ram_seg = dyn_container(
        move || resources.get().ram_label(),
        move |r| footer_text(format!("RAM: {r}")),
    )
    .style(|s| s.margin_left(15.0));

    // The schema toggle always stays (it's a control, and leftmost); every status
    // segment after it collapses right-to-left as the AI icon nears it.
    let left_group = h_stack((
        schema_icon,
        collapsing_seg(cursor_seg, ai_x),
        collapsing_seg(tabs_seg, ai_x),
        collapsing_seg(wrap_seg, ai_x),
        collapsing_seg(warn_seg, ai_x),
        collapsing_seg(ro_seg, ai_x),
        collapsing_seg(mode_seg, ai_x),
        collapsing_seg(tx_pill, ai_x),
        collapsing_seg(commit_seg, ai_x),
        collapsing_seg(rollback_seg, ai_x),
        collapsing_seg(model_seg, ai_x),
        collapsing_seg(effort_seg, ai_x),
        collapsing_seg(cpu_seg, ai_x),
        collapsing_seg(ram_seg, ai_x),
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
#[allow(clippy::too_many_arguments)]
pub(crate) fn search_box(
    query: RwSignal<String>,
    on_escape: Rc<dyn Fn()>,
    on_arrow_up: Rc<dyn Fn()>,
    on_arrow_down: Rc<dyn Fn()>,
    on_submit: Rc<dyn Fn()>,
    on_tab: Rc<dyn Fn()>,
    caret_end: RwSignal<u64>,
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
            on_tab: Some(on_tab),
            caret_end: Some(caret_end),
            ..Default::default()
        },
    )
    .style(|s| s.width_full())
}
