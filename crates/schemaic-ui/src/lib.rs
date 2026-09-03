//! Schemaic UI (Floem).
//!
//! M2: the three-pane shell plus a **virtualized** Results grid — a frozen
//! header over a `scroll(virtual_stack(...))` that renders only the visible
//! rows, so millions of rows stay smooth. Rows are keyed by index and the view
//! fn indexes into a shared `Arc<ResultSet>` (no per-row cloning). Layout
//! follows FEATURES §1.

mod account_editor;
mod activity_panel;
mod ai_panel;
mod blob_view;
pub use ai_panel::mark_messages_seen;
pub use blob_view::{BlobPane, BlobState, BlobTarget, BlobUi};
mod cell_editors;
mod completion;
mod connection_form;
mod connection_import;
mod consts;
pub mod contrast;
mod database_editor;
mod ddl_preview;
mod dividers;
mod dump_view;
mod editor_pane;
pub mod erd_raster;
mod erd_view;
mod event_editor;
pub mod fonts;
mod grid;
mod history_panel;
pub mod icons;
mod import_view;
mod inline_diff;
mod markdown;
mod modals;
mod monitor_view;
mod object_editor;
mod overlays;
mod plan_view;
mod properties;
mod routine_editor;
mod schema_tree;
mod script_view;
/// The tree-node key builders. Public because the persisted expanded-node set is
/// the app's to edit (collapsing a database drops every `tbl:<db>:*` key), and
/// the format belongs to exactly one module.
pub use schema_tree::{column_key_named, db_key, table_key_named, table_key_prefix};
mod settings;
mod shortcuts;
mod snippet_edit;
mod snippet_panel;
/// The shared machinery behind this crate's source gates. Test-only.
#[cfg(test)]
mod source_gate;
pub mod sql_highlight;
mod table_designer;
mod tabs;
pub mod theme;
pub mod themes;
mod trigger_editor;
mod users_view;
mod view_editor;
mod widgets;
mod window_chrome;

use activity_panel::activity_panel;
use ai_panel::ai_panel;
use consts::*;
use dividers::{h_resize_handle, v_resize_handle};
use editor_pane::{QueryPaneParams, editor_placeholder, query_pane};
use grid::{
    GridCtx, Phase, grid_error_bar, grid_find_bar, grid_goto_bar, grid_selection_bar, loaded_view,
    phase_of, running_view,
};
use history_panel::history_panel;
// The modal overlays are imported by `modals`, which is the only thing that
// mounts them — the root tuple below carries one entry for the whole layer.
use overlays::{
    active_db_menu_overlay, activity_menu_overlay, conn_menu_overlay, context_menu_overlay,
    date_pick_overlay, db_visibility_overlay, popup_menu_overlay, schema_settings_overlay,
};
use schema_tree::{schema_panel, schema_panel_w};
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
use floem::reactive::{Memo, Scope, batch, create_effect, create_memo, untrack};
use floem::style::{CursorStyle, Transition, Width};
use floem::text::FamilyOwned;
use floem::unit::Px;
use floem::views::editor::command::CommandExecuted;
use floem::views::editor::core::editor::EditType;
use floem::views::editor::core::mode::Mode;
use floem::views::editor::core::selection::Selection;
use floem::views::editor::keypress::default_key_handler;
use floem::views::editor::keypress::key::KeyInput;
use floem::views::editor::text::{WrapMethod, default_dark_color};
use floem::views::scroll::{Handle, Rounded, Thickness, Track};
use floem::views::{
    Decorators, Delay, LabelClass, TextInputClass, TooltipClass, TooltipContainerClass,
};
use floem::window::WindowId;
use schemaic_core::connection::{AiData, ConnStatus, Connection, Environment, SshAuth, SslMode};
use schemaic_core::db_color::{DbColorRule, TableColorRule};
use schemaic_core::favorite::FavoriteRule;
use schemaic_core::format::ColumnFormatRule;
use schemaic_core::history::HistoryEntry;
use schemaic_core::intel::SqlDialect;
use schemaic_core::model::{CommitDone, GridWrite, QueryState, RefetchRequest};
use schemaic_core::resource::ResourceSample;
use schemaic_core::tx::{TabTx, TxMode, TxState, write_blocking_tabs};
use schemaic_core::update::UpdateState;

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
    /// Which rows to write — the ones on screen, or every row the statement has.
    pub scope: ExportScope,
    /// Will the caller show a **Stop** for this run?
    ///
    /// **The question the app's single cancel slot really asks.** That slot
    /// exists so one Stop always reaches the run it is pointed at, and it refuses
    /// a second export to keep it that way — but only a run someone can stop
    /// needs to be in it. The grid raises the progress modal and so answers
    /// `true`; the Live Monitor's log export shows nothing and answers `false`,
    /// which is what keeps it out of the slot.
    ///
    /// Without this the two would contend: both write `Fetched` rows, so a log
    /// export begun while a grid export ran would be refused with
    /// *"An export is already running"* — a message about a window the user
    /// cannot see, for a save that has nothing to do with theirs. That is a
    /// regression this field exists to prevent, and it appeared the moment the
    /// `Fetched` path started taking a token at all.
    pub stoppable: bool,
}

/// How much of a result an export covers.
///
/// The distinction exists because a grid is capped and a file is not. The row
/// cap is what keeps a result openable — nobody scrolls two million rows — and
/// for years "export" quietly meant "export whatever the cap fetched", which is
/// the right answer only by coincidence.
pub enum ExportScope {
    /// The rows the grid already holds, in the order it shows them — including
    /// any client-side sort. A snapshot taken before the save dialog opens, so
    /// what lands in the file is what the user was looking at when they asked.
    Fetched,
    /// Re-run `sql` with **no cap** and stream every row it returns.
    ///
    /// The statement, not the table: a filtered or sorted grid exports what it is
    /// showing all of, and a table tab's statement is `SELECT * FROM t` anyway.
    /// This is deliberately a *second read* of the server rather than a
    /// continuation of the first — the rows on screen may be minutes old, and an
    /// export that stitched a stale page onto fresh ones would be neither.
    AllRows {
        conn_id: u64,
        /// The database the statement ran under, from the result rather than the
        /// tab — a tab's selection moves, and the export must re-run in the scope
        /// the rows came from. See `ResultSet::database`.
        database: Option<String>,
        sql: String,
    },
}

/// How an export ended.
///
/// Three outcomes and not a `Result`, because "the user stopped it" is neither a
/// success nor a failure and reporting it as either is a lie: a red bar for a
/// deliberate Cancel, or a cheerful count for a file that stops halfway.
/// Mirrors `ImportOutcome`, which answers the same question for the other
/// direction.
pub enum ExportOutcome {
    /// Written: the rows in the file, and what the file could not carry
    /// (`schemaic_core::export::ExportTally`). The caveats travel with the count
    /// rather than being a private fact of the emitter, because CSV and JSON
    /// have no comment syntax to put them in and the bar is the only place left.
    Done(schemaic_core::export::ExportTally),
    /// Stopped by the user. **The partial file is left where it is**, and the
    /// message says so — deleting it would be the one irreversible thing this
    /// path could do, and the save dialog may well have been pointed at a file
    /// that already mattered.
    Cancelled,
    /// Failed. `partial` says whether the destination was already created: the
    /// write opens with `File::create`, which truncates, so a failure after that
    /// point leaves a fragment where the user's previous file was — and the
    /// message has to say so. An export refused *before* the write (no
    /// connection, one already running) must not claim a file it never touched.
    Failed { message: String, partial: bool },
}

/// What an ER-diagram export writes.
///
/// Captured **before** the save dialog opens, not after: the dialog is modal and
/// the diagram's signals belong to the modal behind it, so a callback that went
/// back for them would be reading a scope that may already be gone. It also means
/// the file is a picture of what the user was looking at when they chose the
/// format — the same rule the results grid's snapshot follows.
///
/// What is captured is as little as the UI thread is *obliged* to do. For a
/// What a schema + data dump writes, and where.
///
/// The **plan** it will follow is built in the app from a freshly introspected
/// schema, not from anything captured here: the tree's catalog can be stale, and
/// a dump that emits a `CREATE TABLE` the server has since changed is a corrupt
/// backup. This carries only what the user chose.
pub struct DumpRequest {
    pub path: std::path::PathBuf,
    pub conn_id: u64,
    pub database: String,
    /// The chosen tables, by display name (`sales.orders`, or bare where the
    /// engine has no namespaces) — the same spelling the tree and the ER diagram
    /// key nodes by, so the picker's rows resolve without a second identity.
    pub tables: Vec<String>,
    pub opts: schemaic_core::dump::DumpOptions,
    pub dialect: SqlDialect,
}

/// What a **folder** export writes, and where — the file-per-table sibling of
/// [`DumpRequest`], behind the schema tree's `Export ▸ CSV` and its four
/// non-SQL siblings.
///
/// The plan is built in the app from a freshly introspected schema, exactly as a
/// dump's is and for the same reason; this carries only what the user chose.
/// `folder` is a directory that already exists — the dialog that produced it
/// only offers directories — and each table's file name is decided by
/// [`schemaic_core::dump::file_plan`], never here.
pub struct FilesRequest {
    pub folder: std::path::PathBuf,
    pub conn_id: u64,
    pub database: String,
    /// The chosen tables, by display name — [`DumpRequest::tables`]' spelling,
    /// because it is the same picker.
    pub tables: Vec<String>,
    /// Never [`schemaic_core::export::ExportFormat::Sql`]: that format is what
    /// the dump path is, and offering it here would be a second writer for one
    /// file format.
    pub format: schemaic_core::export::ExportFormat,
    pub dialect: SqlDialect,
}

/// How a folder export ended.
///
/// **Its own type rather than [`DumpOutcome`]**, because all three sentences it
/// has to produce are about a *folder* and none of `DumpOutcome`'s are. A
/// stopped dump leaves one `.part` sibling beside a destination it never
/// touched; a stopped folder export leaves the files that finished plus a
/// fragment of the one that did not — so borrowing the dump's wording would name
/// a file the user never asked for and say nothing about the ones they got.
/// `files` is what makes each honest: how many were completed and published.
pub enum FilesOutcome {
    /// Written. `files` is how many the folder gained, `tally` is every table's
    /// [`ExportTally`](schemaic_core::export::ExportTally) folded into one (see
    /// `ExportTally::absorb`) so one sentence can carry what the files could not
    /// hold, and `missing` is the ticked tables this run's own introspection
    /// could not find — the same guarantee [`DumpOutcome::Done`] makes.
    ///
    /// **All three arms carry `missing`**, not just this one. A folder two files
    /// short of what was ticked looks exactly like a complete one, and a stopped
    /// or failed export is *more* likely to be inspected than a finished one —
    /// so the arm least likely to be read was the one that used to drop it.
    Done {
        files: usize,
        tally: schemaic_core::export::ExportTally,
        missing: Vec<String>,
    },
    /// Stopped by the user. The completed files stay: they are whole, they are
    /// what the user asked for, and each was published by the same rename a
    /// dump's is.
    Cancelled { files: usize, missing: Vec<String> },
    /// Failed. `files` counts the ones already published, which is what makes
    /// this different from an export that failed outright — the folder is not
    /// empty, and a message that did not say so would send the user looking for
    /// nothing.
    Failed {
        message: String,
        files: usize,
        missing: Vec<String>,
    },
}

pub type FilesDoneFn = Rc<dyn Fn(FilesOutcome)>;
/// Introspect and write one file per table **off the UI thread**, reporting each
/// table through [`DumpUi::progress`] — the same channel a dump uses, because it
/// is the same modal and the same "3 of 12" — and the end through
/// [`FilesDoneFn`].
pub type FilesFn = Rc<dyn Fn(FilesRequest, FilesDoneFn)>;

/// The database a script modal is open on — the load half of [`DumpTarget`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptTarget {
    pub conn_id: u64,
    pub database: String,
    /// The PostgreSQL namespace the entry was opened on, when it was. It does
    /// **not** scope the run: a script's statements name their own objects, and
    /// pretending otherwise would promise a containment this cannot deliver. It
    /// is carried so the modal can say which node was right-clicked.
    pub schema: Option<String>,
    pub dialect: SqlDialect,
}

/// What to run, and where. The load half of [`DumpRequest`].
///
/// **Only [`ScriptRequest::approved`] can build one**, and it is the write
/// guard. The fields are readable and none of them is settable, so there is no
/// spelling of "run this file" that does not go through `sql::script_verdict`
/// first — which is what the previous shape could not promise. The guard was an
/// exhaustive `match` in the launcher whose `Block` arm returned; deleting that
/// one `return` left the whole workspace green while a read-only connection ran
/// the file, because every test asked the *function* and none asked the seam.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptRequest {
    path: std::path::PathBuf,
    conn_id: u64,
    database: String,
    dialect: SqlDialect,
}

impl ScriptRequest {
    /// The guard and the request, in one step: `Err` is the message for the
    /// error bar, `Ok` is a run that has passed `sql::script_verdict`.
    ///
    /// The `Confirm` arm is an `Ok` here, and that is the decision this app has
    /// already written down: the script panel *is* the confirmation, and it is
    /// a better one than the verdict's sentence — it names the statement counts
    /// and, in red, how many of them destroy data, above a button marked Run.
    /// What must not happen is that arm being reached silently, so it is
    /// matched rather than caught by a wildcard.
    pub fn approved(
        policy: schemaic_core::sql::GuardPolicy,
        path: std::path::PathBuf,
        file_name: &str,
        conn_id: u64,
        database: String,
        dialect: SqlDialect,
    ) -> Result<ScriptRequest, String> {
        match schemaic_core::sql::script_verdict(policy, file_name) {
            // Refused with no override, by design: the read-only block has
            // none, and here it applies to a file nobody has read.
            schemaic_core::sql::RunVerdict::Block(why) => Err(why),
            schemaic_core::sql::RunVerdict::Confirm(_) => Ok(()),
            // `script_verdict` never returns this; matched rather than caught
            // by a wildcard so that if it ever does, the change is made here on
            // purpose.
            schemaic_core::sql::RunVerdict::Allow => Ok(()),
        }
        .map(|()| ScriptRequest {
            path,
            conn_id,
            database,
            dialect,
        })
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    pub fn conn_id(&self) -> u64 {
        self.conn_id
    }

    /// The database the script runs *in*. Always present, because the run is
    /// launched from a database or schema node — and because
    /// `sql::script_verdict` blocks outright without one.
    pub fn database(&self) -> &str {
        &self.database
    }

    pub fn dialect(&self) -> SqlDialect {
        self.dialect
    }
}

/// How far a running script has got.
///
/// **Bytes, not statements**, and not for want of a count: a file's statement
/// total cannot be known without reading the file, which is the thing being
/// done. Its byte length is known at `open`, so it is the one denominator that
/// is honest from the first message. See `schemaic_core::script::probe`, which
/// reports every count it *can* give as a floor for the same reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScriptProgress {
    pub bytes_done: u64,
    /// `0` when the file's length could not be read, which the label must treat
    /// as "unknown" rather than dividing by.
    pub bytes_total: u64,
}

/// The file a grid export is writing, captured when it starts.
///
/// `Some` on [`ExportUi::target`] ⇒ the modal is up. Held rather than re-read
/// because the export outlives the grid that launched it — a re-run replaces the
/// `GridState` while the write goes on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportTarget {
    /// The destination's file name, for the modal's one line.
    pub name: String,
    /// The denominator, when the export has an honest one.
    ///
    /// **On the target and not on each progress message**, because it is a
    /// property of the export rather than of a moment in it: the writer that
    /// reports the rows has never heard of it, so a message carrying the field
    /// would carry `None` and overwrite it on the first block.
    pub total: Option<u64>,
    /// Is [`Self::total`] an *estimate*?
    ///
    /// The two scopes differ, and saying so is the whole point. A `Fetched`
    /// export knows exactly how many rows it holds, so `1,234 of 5,000` is a
    /// promise it keeps. An `AllRows` export is re-reading the server, and its
    /// figure is the catalogue's guess — the same one the `~All rows (M)` entry
    /// was named after — so it is shown as `~180k`, a real count larger than it
    /// is ordinary, and nothing may divide by it or treat it as a ceiling.
    ///
    /// Carried rather than inferred from the scope, because by the time the
    /// modal draws, the scope is gone.
    pub approx: bool,
    /// Which export raised this modal — `grid::next_export_id`'s.
    ///
    /// **Only the export that raised the modal may report into it or close it**
    /// (`grid::export_modal_closes`). Without it the tail of *any* finished
    /// export writes here: a second request is refused synchronously by the
    /// app's single cancel slot, and its refusal would overwrite the running
    /// export's progress line and take its Stop off the screen for exactly as
    /// long as it mattered.
    pub run: u64,
}

/// The grid export's modal — **the single affordance** a grid export has, from
/// the moment it starts to the moment the user dismisses it.
///
/// It is raised by **every** grid export, both scopes, and it does not close
/// itself. While the write runs it shows the row count and a **Stop**; when the
/// write ends it shows how it ended and a **Close**. One surface, two states.
///
/// **Both halves of that are corrections, and the same one twice.** The modal
/// began as the streamed scope's alone, on the reasoning that a `Fetched` export
/// renders rows already in memory and has nothing to report — true only because
/// that path rendered in one block, which is now `SliceChunks`' job to fix. And
/// once every export raised it, the ones that finish in a frame or two raised it
/// *invisibly*: a modal that closes itself the instant the write ends cannot
/// confirm anything about a write that was never slow enough to watch. Reporting
/// the outcome here is what makes a fast export and a slow one look the same to
/// the person doing it.
///
/// So the outcome is **no longer on the grid's bar**. It was there because the
/// bar could say `Exporting…` and then `Exported 16k rows to employees.csv` in
/// one strip, which was the right answer while the bar was the only surface;
/// with a modal in front of it that strip is a second place saying the same
/// thing to nobody, behind a backdrop. One affordance says all of it.
#[derive(Clone, Copy)]
pub struct ExportUi {
    /// The file being written; `Some` ⇒ the modal is up.
    ///
    /// It stays `Some` after the write ends — that is what makes this a
    /// confirmation and not just a progress bar. Only the footer's Close clears
    /// it.
    pub target: RwSignal<Option<ExportTarget>>,
    /// The finished sentence, once an export succeeds — `export_note`'s, the
    /// same one the bar used to show.
    ///
    /// `Some` on either this or [`Self::error`] is what the footer reads to
    /// become **Close** instead of **Stop**: the run is over, so there is
    /// nothing left to stop.
    pub done: RwSignal<Option<String>>,
    /// How it failed, or that it was stopped.
    ///
    /// **A cancel lands here, not in `done`.** Stopping was the user's own
    /// doing, so it is neither a success nor a failure — but the sentence it
    /// carries is about a file that is *not* what was asked for, and putting
    /// that under a green tick is the one reading it must not get.
    pub error: RwSignal<Option<String>>,
    /// Rows written so far. `None` until the first block lands, which the modal
    /// shows as an animated "Starting" rather than as zero — a stalled export
    /// and one that has genuinely written no rows look identical otherwise.
    ///
    /// **Rows and not bytes**, the opposite of [`ScriptProgress`]'s choice and
    /// for the same underlying reason: report what you can honestly count. A
    /// script's statement total cannot be known without reading the file, so it
    /// counts bytes; an export's rows are exactly what it is producing.
    pub progress: RwSignal<Option<u64>>,
}

/// How far a running dump has got — one message per table, as it starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DumpProgress {
    /// 1-based, so it reads as "3 of 12" without arithmetic at the label.
    pub index: usize,
    pub total: usize,
    pub table: String,
    /// Rows written **before** this table — the running total.
    pub rows: u64,
}

/// How a dump ended. The three arms `ExportOutcome` has, for the same reasons:
/// a cancel is neither a success nor a failure, and `partial` says whether a
/// fragment was left behind.
pub enum DumpOutcome {
    /// Written: how many tables the file covers, and the export renderer's own
    /// [`ExportTally`](schemaic_core::export::ExportTally).
    ///
    /// **The tally, not a row count.** It carries what the file could *not* hold
    /// — the binary columns written as `NULL`, the values past the arena ceiling
    /// left blank — and those are the difference between a backup and a file
    /// that looks like one. A dump reporting only `rows` told the user "Wrote 5
    /// tables and 115k rows." about a file whose every blob was `NULL`. Same
    /// reasoning as [`ExportOutcome::Done`], which carries it for the same
    /// reason.
    Done {
        tables: usize,
        tally: schemaic_core::export::ExportTally,
        /// Tables the user ticked that the dump's own fresh introspection could
        /// not find — see [`DumpPlan::missing`](schemaic_core::dump::DumpPlan::missing).
        /// A backup one table short of what was asked for is exactly the case
        /// that must not read as a clean success.
        missing: Vec<String>,
    },
    Cancelled,
    Failed {
        message: String,
        partial: bool,
    },
}

/// picture that is the measured [`SvgScene`](schemaic_core::erd_export::SvgScene)
/// and nothing more: the measuring goes through floem's font system and so cannot
/// leave this thread, but building the document out of it is pure — 34 ms of
/// string work and a 5 MB allocation at 500 tables — and joins the rasterise and
/// the write on the worker.
#[derive(Clone)]
pub enum ErdDoc {
    /// A finished document, written as-is (Mermaid, DBML, PlantUML, Graphviz).
    Text(String),
    /// A measured scene, plus the scale to rasterise at when the target is a PNG.
    /// `None` writes the SVG itself — see [`crate::erd_raster`] for the raster.
    Scene(Box<schemaic_core::erd_export::SvgScene>, Option<f32>),
}

impl ErdDoc {
    /// The document as text, building it here if this is still a scene.
    ///
    /// The clipboard is synchronous, so *Copy as SVG* — the one caller that
    /// cannot hand the work to a worker and come back — pays for the document on
    /// the UI thread. `None` for a PNG, which this channel cannot hold at all.
    pub fn into_text(self) -> Option<String> {
        match self {
            ErdDoc::Text(s) => Some(s),
            ErdDoc::Scene(scene, None) => Some(schemaic_core::erd_export::to_svg(&scene)),
            ErdDoc::Scene(_, Some(_)) => None,
        }
    }

    /// The bytes to write. Called on the worker: this is where the document is
    /// built and, for a PNG, rasterised.
    pub fn into_bytes(self) -> Result<Vec<u8>, String> {
        match self {
            ErdDoc::Text(s) => Ok(s.into_bytes()),
            ErdDoc::Scene(scene, scale) => {
                let svg = schemaic_core::erd_export::to_svg(&scene);
                match scale {
                    Some(scale) => crate::erd_raster::png_from_svg(&svg, scale),
                    None => Ok(svg.into_bytes()),
                }
            }
        }
    }
}

/// What to write, and where.
pub struct ErdExportRequest {
    pub path: std::path::PathBuf,
    pub doc: ErdDoc,
}

/// Write an exported ER diagram to a file **off the UI thread**, reporting via
/// [`ExportDoneFn`]. Building the document and rasterising it are the slow halves
/// and run here ([`ErdDoc::into_bytes`]); the modal owns the save dialog and the
/// measurement the font system pins to the UI thread.
pub type ErdExportFn = Rc<dyn Fn(ErdExportRequest, ExportDoneFn)>;

/// One binary cell's bytes, and where to write them.
#[derive(Clone)]
pub struct BlobSaveRequest {
    pub path: std::path::PathBuf,
    /// The whole value. The panel refuses to offer a save of a **truncated**
    /// buffer, so what arrives here is always the value and never its front —
    /// see `blob::BlobValue::truncated`.
    pub bytes: std::sync::Arc<schemaic_core::blob::BlobValue>,
    /// Which opening of the panel asked for this save — handed back to
    /// [`BlobUi::saved_at`], which drops a report from a superseded one.
    pub epoch: u64,
}

/// Write a binary cell's bytes to a file **off the UI thread**, reporting the
/// path written or why it failed into [`BlobUi::saved`].
///
/// Off-thread for the same reason the ERD export is: the buffer is up to
/// `blob::FETCH_CAP`, and a 64 MiB `fs::write` on the UI thread is a frozen
/// window. The panel owns the save dialog; this only writes.
pub type BlobSaveFn = Rc<dyn Fn(BlobSaveRequest)>;

/// Raise the binary-cell panel on one cell and fetch its bytes.
///
/// The `u64` is the connection the **rows** came from (`conn_at_load`), not the
/// tab's current one: a blob re-read over a different connection is a different
/// database's row with the same key.
pub type ViewBlobFn = Rc<dyn Fn(u64, schemaic_core::blob::BlobRef, BlobTarget)>;

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
    /// needs it to warn about a load held in memory
    /// ([`schemaic_core::import::memory_warning`]).
    pub file_bytes: u64,
    /// The workbook's sheet names, for Excel; empty otherwise.
    ///
    /// Carried back with the sample because they come off the **same parse** of
    /// the workbook ([`schemaic_core::import::read_workbook_sample`]). A second
    /// call to fetch them would open and inflate the whole file again — on
    /// every settings change, since each one re-probes — and a picker populated
    /// by its own read could list sheets from a *different* file than the
    /// preview beside it.
    pub sheets: Vec<String>,
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
    /// Which MySQL-family server this database is on, so per-flavour controls
    /// can hide what the server can't express rather than offering it and
    /// failing at apply — the rule `trigger_editor`'s per-engine form follows.
    pub flavour: schemaic_core::schema::ServerFlavour,
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

/// Where an object lives, for a schema-editor's modal title: `database` on
/// MySQL, `database.schema` wherever there is a namespace level.
///
/// Every one of these modals used to carry an "In {database}.{schema}" row above
/// its first field, saying something the title was the natural place for. This is
/// that row's rule, moved into the title — deliberately **not**
/// `schema::display_name`, which drops `public` because it is the search-path
/// default. That's right for a tab title or a tree row, which name an *object*;
/// this names a **place**, and the row it replaces spelled `public` out.
pub fn object_location(database: &str, schema: Option<&str>) -> String {
    match schema {
        Some(s) => format!("{database}.{s}"),
        None => database.to_string(),
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

/// What the trigger editor is editing: **one table's whole set of triggers**.
/// Doubles as its open flag.
///
/// A set rather than one trigger because that's how the modal works — a list of
/// the table's triggers with the selected one's form beside it, so a single plan
/// can drop one, edit another and add a third. It is still its own plan, not a
/// designer tab: a trigger needs its own statement and can't join the table's
/// coalesced `ALTER TABLE`.
#[derive(Clone, Debug)]
pub struct TriggerTarget {
    pub conn_id: u64,
    pub database: String,
    /// PostgreSQL namespace of the table; `None` on MySQL.
    pub schema: Option<String>,
    pub table: String,
    pub dialect: SqlDialect,
    /// Whether the target is a **view**. PostgreSQL's timing rules are exact
    /// opposites on a table and a view (`INSTEAD OF` only on the latter,
    /// row-level `BEFORE`/`AFTER` only on the former), so the form can't offer
    /// the right options without knowing.
    pub is_view: bool,
    /// The introspected triggers the draft started from — the left-hand side of
    /// the diff.
    pub current: Vec<schemaic_core::schema::TriggerInfo>,
    pub read_only: bool,
}

impl TriggerTarget {
    /// The modal's title subject: the table whose triggers these are.
    pub fn display(&self) -> String {
        schemaic_core::schema::display_name(self.schema.as_deref(), &self.table)
    }

    /// What the model needs to judge a timing against.
    pub fn host(&self) -> schemaic_core::ddl::TriggerHost {
        schemaic_core::ddl::TriggerHost::of(self.is_view)
    }
}

/// What the routine editor is editing. Doubles as its open flag.
///
/// Separate from [`TriggerTarget`] rather than a mode of it: a routine has its
/// own lifetime, outlives every trigger bound to it, and is reachable without
/// going through a trigger at all — from the schema tree, from Find-Anywhere,
/// and from the Create menu.
#[derive(Clone, Debug)]
pub struct RoutineTarget {
    pub conn_id: u64,
    pub database: String,
    pub dialect: SqlDialect,
    pub current: Option<schemaic_core::schema::RoutineInfo>,
    pub read_only: bool,
}

/// Which routine to read the real body and session state for.
pub struct RoutineSrcRequest {
    pub conn_id: u64,
    pub database: String,
    /// The routine's name **on the server** — the identity a `SHOW CREATE`
    /// addresses, not whatever the draft has been renamed to.
    pub name: String,
    pub kind: schemaic_core::schema::RoutineKind,
}
/// Reports one routine's source on the UI thread, keyed by the name asked for so
/// a late reply can be matched back to the routine it belongs to. `None` means
/// the server had nothing to say (PostgreSQL, or a definition the account may
/// not read).
pub type RoutineSrcDoneFn = Rc<dyn Fn(String, Option<schemaic_core::schema::RoutineSource>)>;
/// Read one MySQL routine's body **as written** off-thread.
///
/// The counterpart of [`TriggerSrcFn`], and not an optimisation for the same
/// reason: `information_schema` resolves the body's escapes on MySQL 8, and
/// every edit on that engine begins with a `DROP` that commits on its own — so
/// a restate built from the resolved text can fail after the only copy is gone.
/// See `schemaic_core::schema::RoutineSource`.
pub type RoutineSrcFn = Rc<dyn Fn(RoutineSrcRequest, RoutineSrcDoneFn)>;

/// What the event editor is editing. Doubles as its open flag.
///
/// The same shape as [`RoutineTarget`] and a separate type for the same reason
/// the routine's is separate from the trigger's: an event is its own object,
/// reached from the schema tree, from Find-Anywhere and from the Create menu.
///
/// No `dialect` fork inside the form — unlike a routine, an event exists on one
/// engine only ([`schemaic_core::ddl::supports_event_editing`]) — but the field
/// is carried all the same, because every plan this modal builds is emitted in a
/// dialect and reading it off the connection here is what keeps a change set
/// from having to guess.
#[derive(Clone, Debug)]
pub struct EventTarget {
    pub conn_id: u64,
    pub database: String,
    pub dialect: SqlDialect,
    pub current: Option<schemaic_core::schema::EventInfo>,
    pub read_only: bool,
}

/// Which event to read the real body and session state for.
pub struct EventSrcRequest {
    pub conn_id: u64,
    pub database: String,
    /// The event's name **on the server** — the identity a `SHOW CREATE`
    /// addresses, not whatever the draft has been renamed to.
    pub name: String,
}
/// Reports one event's source on the UI thread, keyed by the name asked for so a
/// late reply can be matched back to the event it belongs to.
pub type EventSrcDoneFn = Rc<dyn Fn(String, Option<schemaic_core::schema::EventSource>)>;
/// Read one MySQL event's body **as written** off-thread — the counterpart of
/// [`RoutineSrcFn`]. See `schemaic_core::schema::EventSource`.
pub type EventSrcFn = Rc<dyn Fn(EventSrcRequest, EventSrcDoneFn)>;

/// What the object editor is editing — an enum type, a domain or a sequence.
/// Doubles as its open flag.
///
/// `dependents` is read off the schema **when the editor opens**, not when the
/// plan is built: they are the columns a rebuild would have to re-cast, and they
/// are found by the type's *current* name — which the draft may be in the middle
/// of changing.
#[derive(Clone, Debug)]
pub struct ObjectTarget {
    pub conn_id: u64,
    pub database: String,
    pub schema: Option<String>,
    pub dialect: SqlDialect,
    pub current: Option<schemaic_core::schema::ObjectItem>,
    pub dependents: Vec<schemaic_core::ddl::TypeDependent>,
    pub read_only: bool,
}

impl ObjectTarget {
    /// The object's display name, for the modal title and the preview subject.
    pub fn display(&self) -> String {
        schemaic_core::schema::display_name(
            self.schema.as_deref(),
            self.current.as_ref().map(|c| c.name()).unwrap_or_default(),
        )
    }
}

/// Which container the database editor is making — a database, or one of
/// PostgreSQL's namespaces inside one.
///
/// Two kinds in one modal because the form is the same form: a name, and an
/// owner where the engine has owners. They stay distinguishable rather than
/// being folded into "a container" because the two are **not the same statement
/// at the same level** — a database is server-level and a namespace is not (see
/// [`DdlScope`]), and on MySQL the first exists and the second does not.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ContainerKind {
    Database,
    Schema,
}

impl ContainerKind {
    /// The word the modal title, the confirm and the preview subject all use, so
    /// one edit changes them together.
    ///
    /// It named three callers and had one: both Drop confirms hard-coded
    /// `"Drop database"` / `"Drop schema"`, which are the two modals where the
    /// wording is the last thing a user reads before something irreversible.
    /// They go through here now, so the claim is true.
    pub fn label(self) -> &'static str {
        match self {
            ContainerKind::Database => "database",
            ContainerKind::Schema => "schema",
        }
    }
}

/// The database editor's target; doubles as its open flag.
///
/// There is no `current`, unlike every other target here: this modal only ever
/// creates. A container is dropped from its own row's menu and altered from
/// nowhere — neither engine offers a rename that is safe to perform (see
/// `ddl::DatabaseDraft`) — so an "edit" state would be a form with nothing to
/// put in it.
#[derive(Clone, Debug)]
pub struct DatabaseTarget {
    pub conn_id: u64,
    pub kind: ContainerKind,
    /// The database a new **namespace** goes in, and what the plan then runs
    /// against. `None` for [`ContainerKind::Database`], which is server-level
    /// and has no database to run in — see [`DdlScope::Server`].
    pub database: Option<String>,
    pub dialect: SqlDialect,
    pub read_only: bool,
}

/// The account editor's target; doubles as its open flag.
///
/// Like [`DatabaseTarget`] it has no "edit" state: an account is created here
/// and dropped from its row in the browser, and neither engine offers a rename.
/// Changing what an account *may do* is the other form ([`GrantTarget`]), which
/// is a different question with a different answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountTarget {
    pub conn_id: u64,
    /// The database the plan runs in. Unlike a container's this is **not**
    /// optional and not a "what to avoid": an account change takes the ordinary
    /// in-database route (see `ddl::is_account_change`), so there has to be one.
    pub database: String,
    pub dialect: SqlDialect,
    pub read_only: bool,
}

/// The grant editor's target; doubles as its open flag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrantTarget {
    pub conn_id: u64,
    pub database: String,
    /// The account the statement is about. Carried whole rather than by name
    /// because `users::account_sql` needs the host too, and a MySQL account
    /// *is* the pair.
    pub account: schemaic_core::users::Principal,
    pub dialect: SqlDialect,
    pub read_only: bool,
}

/// One lazy trigger-function fetch — see `schemaic_db::Db::trigger_functions`.
///
/// A plain code span, not an intra-doc link: this crate doesn't depend on
/// `schemaic-db` (the app owns that side and hands the result across), so the
/// path can't resolve from here.
#[derive(Clone, Debug)]
pub struct TriggerFnRequest {
    pub conn_id: u64,
    pub database: String,
}

/// Hands the fetched trigger functions back on the UI thread.
pub type TriggerFnDoneFn = Rc<dyn Fn(Vec<schemaic_core::schema::RoutineInfo>)>;

/// Fetches a database's trigger functions off the UI thread.
///
/// Lazy, not part of the schema fetch: a function body is only needed for the
/// one being bound or edited — the same call [`ViewAlgoFn`] makes.
pub type TriggerFnFn = Rc<dyn Fn(TriggerFnRequest, TriggerFnDoneFn)>;

/// Hands the fetched role names back on the UI thread.
pub type RolesDoneFn = Rc<dyn Fn(Vec<String>)>;

/// Fetches the server's roles off the UI thread — the database editor's Owner
/// shortcut. Takes only a `conn_id`: roles are cluster-wide, and the caller may
/// be about to create a database that does not exist yet.
///
/// Lazy for the reason [`TriggerFnFn`] is, and **failure is silent by design**:
/// this feeds a menu beside a field that stays free text, so an empty list
/// costs a shortcut and never a value.
pub type RolesFn = Rc<dyn Fn(u64, RolesDoneFn)>;

/// Which section of the designer is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesignerTab {
    Table,
    Columns,
    Indexes,
    ForeignKeys,
    Checks,
}

impl DesignerTab {
    pub const ALL: [DesignerTab; 5] = [
        DesignerTab::Table,
        DesignerTab::Columns,
        DesignerTab::Indexes,
        DesignerTab::ForeignKeys,
        DesignerTab::Checks,
    ];
    pub fn label(self) -> &'static str {
        match self {
            DesignerTab::Table => "Table",
            DesignerTab::Columns => "Columns",
            DesignerTab::Indexes => "Indexes",
            DesignerTab::ForeignKeys => "Foreign keys",
            DesignerTab::Checks => "Checks",
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
    /// Where this plan runs — see [`DdlScope`]. Carried on the preview so
    /// `ddl_preview::apply` passes it straight through to the request, rather
    /// than the app re-deriving it from statements it can only see as strings.
    pub scope: DdlScope,
    /// What the plan is about, for the title ("orders", or a new table's name).
    pub subject: String,
    /// Does `subject` live **in** `database`, so the title may write
    /// `database.subject`?
    ///
    /// Not the same question as [`DdlScope`], which decides which *runner* the
    /// plan takes. An account change takes the in-database runner (a PostgreSQL
    /// grant has to run in the database whose catalogue holds the object) and is
    /// nonetheless server-wide, so it inherited a qualifier that reads as a
    /// scope it does not have: `CREATE USER 'app'@'%'` titled `shop.app@%`.
    /// Read off the change set by `ddl_preview::preview_of`, like `scope`.
    pub qualified: bool,
    /// One plain-language line per change.
    pub changes: Vec<String>,
    /// What the plan destroys, in plain language. Non-empty ⇒ the modal says so
    /// before the Apply button, in the error colour.
    pub destructive: Vec<String>,
    /// What that block calls itself — see
    /// [`schemaic_core::ddl::ChangeSet::risk_heading`]. Off the change set, so a
    /// consequence that *is* undone does not appear under "This can't be undone"
    /// two entries away from `DROP USER`.
    pub risk_heading: &'static str,
    /// What the plan asks for that this engine **can't express** — see
    /// [`schemaic_core::ddl::ChangeSet::unsupported`]. Non-empty ⇒ the modal
    /// names each one and Apply refuses, because `statements` is then less than
    /// the change list above it and running it would do part of an edit.
    pub withheld: Vec<String>,
    pub statements: Vec<String>,
    /// The same plan as one script a **client** can run — see
    /// [`schemaic_core::ddl::ChangeSet::export_script`]. What "Copy" and "Open in
    /// editor" hand over; `statements` is what goes on the wire.
    ///
    /// **Not the same text as `statements`, when the plan carries a password.**
    /// Both exits put this somewhere durable and neither is private, so
    /// `export_script` replaces an account password with a placeholder and says
    /// so at the top. The modal renders `statements`, which is the real thing.
    pub script: String,
    pub read_only: bool,
    /// The engine this plan is for.
    ///
    /// Carried on the preview rather than looked up at each exit, so the
    /// question *"may this apply be stopped"* is asked of the plan that is
    /// actually running — a connection switch behind the modal must not change
    /// the answer half way through. Read through
    /// `ddl::ddl_rolls_back_as_a_whole`, never matched on here.
    pub dialect: SqlDialect,
}

/// Where a DDL plan runs, and therefore what has to be re-read afterwards.
///
/// **Not a detail of the statements — a property of the plan**, decided by
/// `schemaic_core::ddl::is_server_level` where the `Change` is still a `Change`.
/// By the time a plan reaches the app it is a `Vec<String>`, and asking "is this
/// a `CREATE DATABASE`?" of a string is the hand-rolled scanner the architecture
/// keeps out of this codebase.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DdlScope {
    /// Runs **inside** [`DdlRunRequest::database`], in a transaction where the
    /// engine has one, and that database is re-introspected afterwards. Every
    /// plan the designer and the object editors build.
    #[default]
    Database,
    /// Runs at the **server** level: `CREATE DATABASE` / `DROP DATABASE`, which
    /// cannot run on a connection to the database they are about and which
    /// PostgreSQL refuses inside a transaction. Takes `Db::run_server_ddl`, and
    /// afterwards the connection's database *list* is what changed — so the
    /// refresh re-lists rather than re-introspecting a database that may be gone.
    Server,
}

/// Run a generated DDL plan against a database.
pub struct DdlRunRequest {
    pub conn_id: u64,
    /// The database the plan is about. Under [`DdlScope::Server`] this is the
    /// database being created or dropped — the one the run must *not* connect
    /// to — rather than the one it runs on.
    pub database: String,
    pub scope: DdlScope,
    pub statements: Vec<String>,
}

/// How a DDL apply ended.
///
/// Three outcomes rather than a `Result`, because the modal has to tell "it
/// didn't run" from "it ran and failed": while an apply is in flight every exit
/// refuses (see [`widgets::exit_action`]), so an outcome the modal can't
/// recognise leaves it stuck on "Applying…" with no way out.
pub enum DdlOutcome {
    /// The whole plan is in effect.
    Applied,
    /// Carries a message that already says which statement failed and how much
    /// of the plan stuck (see `schemaic_db::DdlError`).
    Failed(String),
    /// Nothing ran — the app asked something first (an open transaction on this
    /// connection) and the user backed out. The plan is untouched and Apply is
    /// live again, which is what "Cancel" meant.
    Declined,
}

/// Reports a DDL run's outcome on the UI thread.
pub type DdlDoneFn = Rc<dyn Fn(DdlOutcome)>;
/// Apply a DDL plan off the UI thread, then re-introspect the database.
pub type DdlFn = Rc<dyn Fn(DdlRunRequest, DdlDoneFn)>;

/// Which view to read an `ALGORITHM` for.
pub struct ViewAlgoRequest {
    pub conn_id: u64,
    pub database: String,
    pub view: String,
}
/// Reports the algorithm on the UI thread. `None` covers both "the server said
/// `UNDEFINED`" and "the query failed" — neither is worth interrupting an edit
/// for, and both leave the emitter writing what it writes today.
pub type ViewAlgoDoneFn = Rc<dyn Fn(Option<String>)>;
/// Read one MySQL view's `ALGORITHM` off-thread.
///
/// Its own action rather than part of the schema fetch because it costs a
/// `SHOW CREATE VIEW` **per view** — see `schemaic_db::Db::view_algorithm`, a
/// code span rather than a link for the reason [`TriggerFnRequest`] gives —
/// so it's paid once, for the view actually being edited.
pub type ViewAlgoFn = Rc<dyn Fn(ViewAlgoRequest, ViewAlgoDoneFn)>;

/// Which trigger to read the real body and session state for.
pub struct TriggerSrcRequest {
    pub conn_id: u64,
    pub database: String,
    /// The trigger's name **on the server** — the identity a `SHOW CREATE`
    /// addresses, not whatever the draft has been renamed to.
    pub trigger: String,
}
/// Reports one trigger's source on the UI thread, keyed by the name asked for
/// so the caller can match it back to the row it belongs to. `None` means the
/// server had nothing to say (PostgreSQL, MariaDB, or a failed read).
pub type TriggerSrcDoneFn = Rc<dyn Fn(String, Option<schemaic_core::schema::TriggerSource>)>;
/// Read one MySQL trigger's body **as written** off-thread.
///
/// Its own action for the same reason [`ViewAlgoFn`] is — it costs a
/// `SHOW CREATE TRIGGER` per trigger — but unlike that one it is not an
/// optimisation: on MySQL 8 `information_schema` hands back a body whose
/// escapes are already resolved, so this is the *only* source that can
/// recreate a trigger without changing or destroying it. See
/// `schemaic_core::schema::TriggerSource`.
pub type TriggerSrcFn = Rc<dyn Fn(TriggerSrcRequest, TriggerSrcDoneFn)>;

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
    /// The body editor's auto-grow cap, in rows — shared by whichever editor is
    /// open, since only one ever is. A signal because that's what
    /// [`FieldCfg::max_rows`] takes; nothing changes it mid-edit.
    pub view_rows: RwSignal<usize>,
    /// The trigger editor's target; doubles as its open flag.
    pub trigger: RwSignal<Option<TriggerTarget>>,
    /// The table's triggers being edited. Same rule as `draft`: one value,
    /// because the footer's change count is
    /// [`schemaic_core::ddl::diff_triggers`] of exactly it. The selected row and
    /// the structural-edit counter are the designer's `selected`/`rev` — the two
    /// modals are mutually exclusive, so there is nothing to keep apart.
    pub trigger_draft: RwSignal<schemaic_core::ddl::TriggerSetDraft>,
    /// The routine editor's target; doubles as its open flag.
    pub routine: RwSignal<Option<RoutineTarget>>,
    pub routine_draft: RwSignal<schemaic_core::ddl::RoutineDraft>,
    /// **The Body field's text, owned outside the form.**
    ///
    /// Every other control in that modal seeds a view-local signal once at
    /// build, which is right for a field only the user writes. The body is not
    /// one: MySQL's `SHOW CREATE` reply lands after the form is up and has to
    /// correct it. Routing that correction through `routine` — the overlay's own
    /// `dyn_container` key — meant the whole modal was torn down and rebuilt to
    /// deliver it, `FocusRing` and all, and the caret went with the old scope if
    /// the reply arrived mid-word. Written here instead, `edit_field` reconciles
    /// the doc in place and keeps the caret (see its signal → doc effect).
    pub routine_body: RwSignal<String>,
    /// Whether the routine editor is still waiting for its `SHOW CREATE`.
    ///
    /// MySQL only, and load-bearing rather than cosmetic: until the read lands
    /// the draft holds `information_schema`'s **escape-resolved** copy of the
    /// body, and applying that recreates a routine whose literals are no longer
    /// what was written — after a `DROP` that committed on its own. So Preview
    /// waits for it, and the footer says why.
    pub routine_source_pending: RwSignal<bool>,
    /// The `SHOW CREATE` landed, it **did** correct the body, and the draft had
    /// already moved — so what the draft rests on is still the escape-resolved
    /// copy and applying it recreates the routine from text the server will
    /// refuse, after a `DROP` that has committed.
    ///
    /// The third outcome of the same reply `routine_source_pending` waits for
    /// ([`schemaic_core::ddl::SourceOutcome`]); waiting is what closes the
    /// window for the *untouched* draft, and this is what closes it for the one
    /// the user had already typed into. Preview refuses while it is set, and the
    /// footer says why — a refusal is enough here, a three-way merge is not
    /// needed.
    pub routine_body_stale: RwSignal<bool>,
    /// The event editor's target; doubles as its open flag.
    pub event: RwSignal<Option<EventTarget>>,
    pub event_draft: RwSignal<schemaic_core::ddl::EventDraft>,
    /// **The event Body field's text, owned outside the form** — the same
    /// arrangement `routine_body` is, and for the same reason: MySQL's
    /// `SHOW CREATE` reply lands after the form is up and has to correct the
    /// field without tearing the modal down and taking the caret with it.
    pub event_body: RwSignal<String>,
    /// Whether the event editor is still waiting for its `SHOW CREATE`.
    ///
    /// Preview waits on it for the reason `routine_source_pending` explains,
    /// with one difference worth knowing: an event is altered in place, so a
    /// body restated from the catalogue's escape-resolved copy is *refused*
    /// rather than lost. The wait is still right — a refusal after Apply is a
    /// worse way to learn this than a footer that says so.
    pub event_source_pending: RwSignal<bool>,
    /// The `SHOW CREATE` landed, it **did** correct the body, and the draft had
    /// already moved — the third outcome of the same reply
    /// ([`schemaic_core::ddl::SourceOutcome`]). See `routine_body_stale`.
    pub event_body_stale: RwSignal<bool>,
    /// The account editor's target; doubles as its open flag.
    pub account: RwSignal<Option<AccountTarget>>,
    /// The account being created. Same rule as `draft`: one value, because the
    /// plan is built from exactly it.
    ///
    /// **It holds a password while the form is open**, and nothing else in the
    /// app does. It is cleared on every open and on Cancel, it is never
    /// persisted and never logged, and the one place its value becomes visible
    /// is the preview's SQL — which is the app's one gate between a plan and a
    /// server, and so the one place a statement may not be shown with a field
    /// blanked out. See `users::account_draft_sql`.
    pub account_draft: RwSignal<schemaic_core::users::AccountDraft>,
    /// The grant editor's target; doubles as its open flag.
    pub grant: RwSignal<Option<GrantTarget>>,
    pub grant_draft: RwSignal<schemaic_core::users::GrantDraft>,
    /// The database editor's target; doubles as its open flag. See
    /// [`DatabaseTarget`] for why it has no "edit" state.
    pub database: RwSignal<Option<DatabaseTarget>>,
    /// The database or namespace being created. Same rule as `draft`: one value,
    /// because the plan is built from exactly it.
    pub database_draft: RwSignal<schemaic_core::ddl::DatabaseDraft>,
    /// The server's roles, fetched when a PostgreSQL database editor opens —
    /// what its Owner chevron offers. Empty on MySQL, and empty until the fetch
    /// lands, which costs a shortcut and never a value: the field is free text,
    /// and `suggest_chevron` reads this **when pressed** rather than when the
    /// form is built, so a late reply needs no rebuild and takes no caret with
    /// it.
    pub roles: RwSignal<Vec<String>>,
    /// The object editor's target; doubles as its open flag.
    pub object: RwSignal<Option<ObjectTarget>>,
    /// The enum / domain / sequence being edited. Same rule as `draft`: one
    /// value, because the footer's change count is
    /// [`schemaic_core::ddl::ObjectDraft::change_set`] of exactly it.
    pub object_draft: RwSignal<schemaic_core::ddl::ObjectDraft>,
    /// Fields whose text isn't a number yet — a sequence's bounds are typed, and
    /// the draft holds `i64`, so a half-typed `-` has nowhere to live in it.
    /// Kept beside the draft rather than in it: they are a property of the *form*,
    /// and a draft carrying them couldn't be diffed.
    pub object_errors: RwSignal<Vec<String>>,
    /// Bumped on every structural edit to a list inside the object editor (an
    /// enum value added or moved, a domain constraint removed). The rows are
    /// keyed on it for the reason the designer's form is: removing row *n*
    /// leaves the index alone while the item at it is now a different one.
    pub object_rev: RwSignal<u64>,
    /// The database's trigger functions, fetched when a PostgreSQL trigger
    /// editor opens — what its "Function" dropdown offers. Empty on MySQL, and
    /// empty until the fetch lands, which is why the dropdown keeps whatever the
    /// draft already names rather than resetting to the first entry.
    pub functions: RwSignal<Vec<schemaic_core::schema::RoutineInfo>>,
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
    /// Bumped on every **preview** open. An apply is off-thread and can outlive
    /// the modal that asked for it, so its callback checks this before writing.
    pub generation: RwSignal<u64>,
    /// Bumped when a DDL **editor session** starts. Every editor's `open` does
    /// it — designer, view, trigger, routine and object — and nothing else
    /// writes it, which is what lets [`crate::widgets::overlay_open_key`] treat
    /// it as "these contents were replaced wholesale".
    ///
    /// Separate from `generation` because the two answer different questions and
    /// one counter could not answer both. The lazy per-object fetches
    /// (`trigger_editor::fetch_functions`, `view_editor::fetch_algorithm`)
    /// guarded on `generation`, which is bumped only by `ddl_preview`'s
    /// `open_preview` — so an in-flight fetch from `db1.orders` landed with the
    /// generation unchanged and overwrote the list for a modal now open on
    /// `db2.invoices`, while opening the *preview* mid-fetch discarded the
    /// result permanently and left the dropdown empty for good.
    ///
    /// **A nested function editor bumps it too, and that costs a round trip
    /// rather than correctness.** The trigger editor's *New function…* and
    /// *Edit function…* go through `routine_editor::open` like any other, so a
    /// `fetch_functions` reply in flight at that moment comes back stamped with
    /// the old session and is dropped. Nothing is left stale by it: the panel
    /// renders nothing while that editor is up (its key treats an open routine
    /// editor as it treats the preview), and the return trip asks again —
    /// `trigger_editor::refetch_functions_on_return`, which has to run anyway
    /// so that a function just *created* reaches the dropdown. Don't "fix" the
    /// bump by skipping it: it is also what invalidates the routine editor's
    /// own `fetch_source`, and what tells [`crate::widgets::overlay_open_key`]
    /// the contents were replaced.
    pub session: RwSignal<u64>,
}

/// Reports an export's outcome back on the UI thread.
pub type ExportDoneFn = Rc<dyn Fn(ExportOutcome)>;
/// Stream a result set to a file **off the UI thread**, reporting via
/// [`ExportDoneFn`]. Writing a large export inline froze the window for as long
/// as it took; the grid owns the save dialog, the app owns the worker.
pub type ExportFn = Rc<dyn Fn(ExportRequest, ExportDoneFn)>;

pub type DumpDoneFn = Rc<dyn Fn(DumpOutcome)>;
/// Introspect and write a schema + data dump **off the UI thread**, reporting
/// each table through [`DumpUi::progress`] and the end through [`DumpDoneFn`].
pub type DumpFn = Rc<dyn Fn(DumpRequest, DumpDoneFn)>;
/// The names a dump's picker offers: one database's tables and views, by display
/// name. Its own read because it must be right for a database the tree has never
/// been expanded on — a picker built from the cached tree would silently offer
/// nothing there.
pub type DumpTablesFn = Rc<dyn Fn(u64, String, Rc<dyn Fn(Result<Vec<String>, String>)>)>;

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

/// Stage result rows as an attachment on the AI panel's next question (grid →
/// app): reveal the panel and put the rows in [`AiUi::attachment`].
///
/// It **stages**, and that is the whole design. Nothing is sent until the user
/// types a question and hits send, so the rows on their screen reach the model
/// by one deliberate gesture with a visible chip in between — never as a side
/// effect of asking something unrelated.
pub type AttachFn = Rc<dyn Fn(schemaic_core::transcript::Attachment)>;

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
    /// This tab's results, one panel per statement — **every** run's, not only a
    /// batch's, and never empty: a tab with nothing run yet holds one `Idle`
    /// panel. The strip that renders them is always on screen, which is what
    /// makes pinning one a visible affordance rather than a hidden mode.
    ///
    /// Order is `resultsel`'s: the pinned panels first, in pin order, then
    /// whatever the last run left.
    pub result_tabs: RwSignal<Vec<ResultPanel>>,
    /// Which panel is shown, by [`ResultPanel::id`] — never an index, which
    /// pinning and closing both move.
    pub active_result: RwSignal<u64>,
    /// The next panel id for this tab. Ids are per-tab and never reused, so a
    /// result that lands after its panel was closed finds nothing rather than
    /// something else's panel.
    pub next_panel: RwSignal<u64>,
    /// The saved connection id this tab's query runs against.
    pub conn_id: RwSignal<u64>,
    /// The database `USE`d for this tab's queries.
    ///
    /// `None` before the connection's database list has loaded. **That is not
    /// "the server level"**, which is what this said and what it meant until
    /// the connection gained a configured **Database**: an unbound tab now runs
    /// in that database on both engines (`Db::open`'s fallback), unqualified
    /// `DROP TABLE` included, with nothing on screen naming it. MySQL used to
    /// answer `ERROR 1046 No database selected`, which was a hard stop.
    ///
    /// The window is usually short — a schema load binds every unbound tab of
    /// the connection — but it is not always: an empty or unreadable listing
    /// leaves tabs `None` for the session.
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
    /// The editor's selected byte range, mirrored out by the same effect;
    /// `None` when the caret is a point.
    ///
    /// Read by the AI panel, which sends the **selection** as the editor context
    /// when there is one rather than the whole buffer — a 47 KB script's worth of
    /// unrelated statements is both noise to the model and text the user never
    /// meant to send. It is a *range*, not the text, so nothing is duplicated
    /// per keystroke; resolve it through `core::text_ops::selected_text`, which
    /// degrades to `None` when the mirror has drifted a keystroke out of step
    /// with `query`.
    pub selection: RwSignal<Option<(usize, usize)>>,
    /// Text the editor should insert at the caret, then clear — the same
    /// request-and-clear shape as [`Tab::jump_offset`] and [`Tab::format_req`],
    /// and for the same reason: the pane owns the `Editor`, so a caller that
    /// rewrote `query` directly would fight the editor's own document and lose
    /// the undo history with it.
    ///
    /// Set by the snippet library (inserting a body) and cleared by the pane.
    pub insert_req: RwSignal<Option<String>>,
    /// What this tab's `:name` query parameters are set to — the parameters
    /// bar's store, and what `TabsActions::run` substitutes with.
    ///
    /// **Session-only, deliberately.** It is not in `SavedTab` and never reaches
    /// `tabs.json`: a parameter value is often an id and sometimes an email or a
    /// token pasted into a `WHERE`, and `schemaic_core::params::ParamValue`
    /// carries no `Serialize` derive to make that hard to undo by accident.
    ///
    /// The *rows* are not stored — they are derived from `query` on every edit
    /// through `params::names`, so they cannot drift from the statement they
    /// belong to. This holds only the values, including ones for names the query
    /// has momentarily lost (`params::set_value` never removes one).
    pub params: RwSignal<Vec<schemaic_core::params::Binding>>,
    /// Opens this tab's Go-to-line popup. Set by Ctrl+G in the editor or by
    /// clicking the Ln/Col segment in the status bar; the editor pane owns the view.
    pub goto_open: RwSignal<bool>,
    /// A byte offset the editor should jump the caret to (move + centre + focus),
    /// then clear. Set by the status-bar warning count to reach the first warning.
    pub jump_offset: RwSignal<Option<usize>>,
    /// Set to ask the editor pane to reformat this tab (the command palette's
    /// "Format Code"), which it does and then clears — the same request-and-clear
    /// shape as `jump_offset`.
    ///
    /// It has to go through the pane rather than rewriting `query`, because the
    /// mounted editor owns its document: the palette command *did* rewrite the
    /// signal, and the formatted text was never shown — the next keystroke
    /// overwrote it back from the doc. Routing it here also means one formatter
    /// (`format_editor`), so the palette can't drift from Ctrl+Alt+L, and the
    /// result lands as one undoable edit instead of a pane remount.
    pub format_req: RwSignal<bool>,
    /// A run error to fix with the AI, which the editor pane does and then
    /// clears — the same request-and-clear shape as [`Tab::format_req`], and for
    /// a stricter version of the same reason.
    ///
    /// The fix opens the pane's Ctrl+K overlay, and that state (`CmdK`) is the
    /// pane's own: it is created inside `query_pane` and never leaves it, so the
    /// error modal — rendered by the workspace, over any tab — has no handle to
    /// reach it with. Set by the modal's "AI fix"; cleared by the pane.
    ///
    /// **It carries the message rather than saying "the current one"**: the
    /// modal shows the error it opened on, and a run that lands while it is up
    /// moves `results` under it (its dismiss layer stops clicks, not queries).
    /// Asked for the tab's *current* error, the fix would then quietly answer a
    /// different question than the one on screen — or, on a run that succeeded,
    /// none at all.
    pub fix_req: RwSignal<Option<String>>,
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
    /// A one-off row cap for this tab, overriding the global setting — set by
    /// the capped notice's "read more" action, cleared on a fresh manual run.
    ///
    /// Per-tab and transient on purpose: getting past the cap once, for one
    /// result, is not a decision about every query the user will ever run, and
    /// making it one is what the global setting already is.
    pub row_cap_override: RwSignal<Option<usize>>,
    /// A filter/sort re-run's DB error, shown as a dismissible bar at the bottom of
    /// the *table* (the previous results stay put — unlike a manual run, which
    /// replaces the grid with the error). Cleared on a table click / new run.
    pub view_err: RwSignal<Option<String>>,
    /// True while a **view re-run** is in flight — the filter/sort splice, and the
    /// capped notice's "read all rows".
    ///
    /// Such a re-run deliberately leaves the current table on screen, so nothing
    /// else on the panel says one is happening: the grid looks idle, and the
    /// affordance that started it is still sitting there inviting a second click.
    /// On a large table that is a second full read of it. A manual run has no
    /// need of this — it replaces the grid with `QueryState::Running`, which is
    /// the same statement said louder.
    ///
    /// Owned by the app's run path (`run_query_core`), which is the only thing
    /// that knows when the re-run lands, and cleared by a manual run too: that
    /// supersedes the view run, whose own landing then returns early and never
    /// reaches its clear.
    pub view_busy: RwSignal<bool>,
    /// The `.sql` file this tab is bound to, or `None` for a scratch tab. Set by
    /// Open (Ctrl+O) and by Save As, and persisted with the tab. A tab with a path
    /// takes its title from the file name and shows a modified dot when it has
    /// drifted from what's on disk.
    pub path: RwSignal<Option<std::path::PathBuf>>,
    /// The file's contents as of the last open / save / reload — the thing
    /// [`Tab::modified`] compares `query` against. `None` means "unknown", which
    /// only happens for a session restored while dirty; it reads as modified,
    /// which is the safe direction.
    pub disk_sql: RwSignal<Option<String>>,
    /// Everything about the file's bytes that isn't its text — the line endings
    /// and BOM a save has to put back, and whether the read was **lossy**, which
    /// is what stops a save silently destroying every byte Schemaic couldn't
    /// read as UTF-8 (see [`schemaic_core::sqlfile`]). Meaningless without
    /// `path`.
    pub file_format: RwSignal<schemaic_core::sqlfile::SqlFormat>,
    /// Bumped when the tab's text is replaced *from outside the editor* (a reload
    /// from disk). Part of the editor pane's container key, because the Floem
    /// editor owns its own document once mounted: writing `query` alone would
    /// leave the visible text stale until the next keystroke overwrote the signal
    /// back from the doc.
    pub reload_gen: RwSignal<u64>,
}

impl Tab {
    /// The result state the user is actually **looking at** in this tab.
    ///
    /// A Run-Everything batch fills `result_tabs` and shows the one
    /// [`shown_panel`] picks — which is *not* always `result_tabs[active_result]`:
    /// a stale index (Result 7 selected, then a batch of three) falls back to the
    /// first panel, and the AI has to describe the same statement the pane does.
    /// Every other run leaves `result_tabs` empty and uses `results`.
    pub fn shown_result(&self) -> QueryState {
        self.result_tabs
            .with_untracked(|panels| {
                shown_panel(panels, self.active_result.get_untracked()).map(|p| p.state.clone())
            })
            .unwrap_or(QueryState::Idle)
    }

    /// This tab's shown result, for the editor — see [`ShownResult`].
    pub fn shown(&self) -> ShownResult {
        ShownResult {
            panels: self.result_tabs,
            active: self.active_result,
        }
    }

    /// The shown panel, as far as the *strip's rules* are concerned.
    pub fn shown_panel_id(&self) -> Option<u64> {
        self.result_tabs
            .with_untracked(|p| shown_panel(p, self.active_result.get_untracked()).map(|p| p.id))
    }

    /// Is the panel on screen a frozen (pinned) one? — the question every
    /// affordance that writes or re-reads has to ask. See [`ResultPanel::frozen`].
    pub fn shown_frozen(&self) -> bool {
        let active = self.active_result.get_untracked();
        self.result_tabs
            .with_untracked(|p| shown_panel(p, active).is_some_and(|p| p.frozen()))
    }

    /// [`Tab::shown_frozen`]'s reactive half — is **panel `id`** frozen, as a
    /// value that *changes*, for everything that has to follow a pin rather than
    /// sample it.
    ///
    /// **The pair exists because the one-shot form was the wrong one for a view.**
    /// Pinning the result on screen moves neither its id nor its phase, so it
    /// does not rebuild the grid: a `bool` read in the grid's builder went on
    /// saying "not pinned" for as long as that grid stayed mounted, and the
    /// result the user had just pinned stayed editable until the user switched
    /// away and back.
    ///
    /// **And it is about one panel, not about whichever panel is shown** — a
    /// distinction that crashed the app. Asked the second way, running a query
    /// with a pin present flipped this for the grid *being torn down*: the
    /// answer changed in the same update pass that unmounted it, the edit-model
    /// effect re-ran, the toolbar's `ai_menu` — a `dyn_container` keyed on that
    /// model — rebuilt its child, and the new child's style effect read a
    /// `GridState` signal whose scope had just been disposed. A grid must only
    /// ever hear about its own panel, and a panel that has *gone* keeps its last
    /// answer rather than reporting `false` on its way out, so closing the shown
    /// result cannot flip it at teardown either.
    ///
    /// Only a caller acting *now*, inside an event handler, may take the
    /// untracked [`Tab::shown_frozen`] instead.
    pub fn panel_frozen_memo(&self, id: u64) -> Memo<bool> {
        let panels = self.result_tabs;
        create_memo(move |prev: Option<&bool>| {
            panels.with(|v| match v.iter().find(|p| p.id == id) {
                Some(p) => p.frozen(),
                None => prev.copied().unwrap_or(false),
            })
        })
    }

    /// The strip, reduced to what [`schemaic_core::resultsel`] answers about.
    ///
    /// Every caller takes it from here rather than spelling the `map` again: the
    /// tuple is built positionally, so a hand-rolled copy is what the compiler
    /// cannot check when [`schemaic_core::resultsel::PanelRef`] changes shape.
    pub(crate) fn panel_refs(&self) -> Vec<schemaic_core::resultsel::PanelRef> {
        self.result_tabs
            .with_untracked(|v| v.iter().map(|p| (p.id, p.pinned)).collect())
    }

    /// Start a run: replace the unpinned panels with one `Running` panel per
    /// statement, and show the first of them. Returns the fresh ids, in
    /// statement order, for the landing to write its states back into.
    ///
    /// The panels that go are disposed **deferred** — they are being removed from
    /// under the views reading them, and disposing a scope inside the update that
    /// unmounts its readers is the crash that rule exists to prevent.
    pub fn begin_run(&self, stmts: &[String]) -> Vec<u64> {
        let now = now_secs();
        let mut next = self.next_panel.get_untracked();
        let fresh: Vec<ResultPanel> = stmts
            .iter()
            .enumerate()
            .map(|(i, sql)| {
                let panel = ResultPanel::new(self.cx, next, sql, now, i + 1);
                next += 1;
                panel
            })
            .collect();
        self.next_panel.set(next);
        let ids: Vec<u64> = fresh.iter().map(|p| p.id).collect();
        let order = schemaic_core::resultsel::after_run(&self.panel_refs(), &ids);
        let active = schemaic_core::resultsel::active_after_run(&self.panel_refs(), &ids);
        // **One notification, not two.** The list and the selection are one move,
        // and separately they are not: between them `active_result` names a panel
        // the list no longer holds, `shown_panel` falls through to its first-panel
        // fallback, and the results body builds that panel's grid in full —
        // `init_widths` sampling 200 rows and all — only to throw it away when the
        // selection lands. With a pinned result present that happened on every
        // single run.
        batch(|| {
            self.rebuild(&order, fresh);
            if let Some(id) = active {
                self.active_result.set(id);
            }
        });
        ids
    }

    /// Rewrite the strip to `order`, keeping the panels it still names (plus any
    /// of `adding` it names) and disposing the rest.
    fn rebuild(&self, order: &[u64], adding: Vec<ResultPanel>) {
        let mut dropped: Vec<Scope> = Vec::new();
        self.result_tabs.update(|panels| {
            let mut pool: Vec<ResultPanel> = std::mem::take(panels);
            pool.extend(adding);
            for id in order {
                match pool.iter().position(|p| p.id == *id) {
                    Some(at) => panels.push(pool.remove(at)),
                    None => continue,
                }
            }
            dropped.extend(pool.into_iter().map(|p| p.cx));
        });
        // Deferred, per the disposal rule: these scopes own signals the views
        // being unmounted are still reading in this same update pass.
        if !dropped.is_empty() {
            floem::action::exec_after(std::time::Duration::ZERO, move |_| {
                for cx in dropped {
                    cx.dispose();
                }
            });
        }
    }

    /// Report one panel's outcome, by id. A panel closed while its statement was
    /// still running is simply gone, and its result lands nowhere — the same
    /// answer the run-generation check gives a superseded run.
    pub fn set_panel_state(&self, id: u64, state: QueryState) {
        // Asked before writing, because `update` notifies whether or not the
        // closure changed anything: a cancelled batch reports statement after
        // statement into panels that are gone, and each of those would otherwise
        // re-run the strip's `dyn_stack`, the body's key memo, every chip's
        // reactive closures and the editor's error bar for a write that did
        // nothing at all.
        if !self.holds_panel(id) {
            return;
        }
        self.result_tabs.update(|panels| {
            if let Some(p) = panels.iter_mut().find(|p| p.id == id) {
                p.state = state;
            }
        });
    }

    /// Report a whole batch's outcomes, by id, in **one** `update`.
    ///
    /// **The notification count is the cost, not the work.** A landing batch
    /// wrote its panels one at a time, and every one of those writes re-ran the
    /// strip's `dyn_stack`, the body's key memo, every chip's reactive closures
    /// and the editor's error bar — measured 0.67 ms → **263 ms** at 400
    /// statements, against a pre-range version that wrote them in one
    /// `result_tabs.update`.
    ///
    /// **Not `floem::reactive::batch`**, which makes it *worse* (816 ms at 400,
    /// measured): `add_pending_effect` dedups with a linear scan, so batching N
    /// notifications turns the cost quadratic instead of removing it. One
    /// `update` is the fix, because one `update` is one notification.
    ///
    /// By id and not by position, for the reason `set_panel_state` gives: the
    /// strip may have been pinned, reordered or partly closed while the batch
    /// ran. A panel that is gone takes no state, exactly as before.
    pub fn set_panel_states(&self, states: impl IntoIterator<Item = (u64, QueryState)>) {
        let states: Vec<(u64, QueryState)> = states.into_iter().collect();
        // The same pre-check `set_panel_state` makes, over the whole batch: a
        // cancelled run reports into panels that are all gone, and an `update`
        // notifies whether or not its closure changed anything.
        if states.is_empty() || !states.iter().any(|(id, _)| self.holds_panel(*id)) {
            return;
        }
        self.result_tabs.update(|panels| {
            for (id, state) in states {
                if let Some(p) = panels.iter_mut().find(|p| p.id == id) {
                    p.state = state;
                }
            }
        });
    }

    /// Rebuild **this panel's** grid on its next read, even if its state went
    /// `Loaded` → `Loaded`.
    ///
    /// What a filter/sort re-run bumps. Per panel, because the tab-wide nonce it
    /// replaces rebuilt whichever panel was *shown* — losing that result's
    /// scroll and selection for a re-run that landed somewhere else entirely.
    pub fn bump_panel_load(&self, id: u64) {
        if !self.holds_panel(id) {
            return;
        }
        self.result_tabs.update(|panels| {
            if let Some(p) = panels.iter_mut().find(|p| p.id == id) {
                p.load_gen = p.load_gen.wrapping_add(1);
            }
        });
    }

    /// Is `id` still in the strip? — the pre-check the two by-id writers make so
    /// a write that would match nothing doesn't notify anyway.
    fn holds_panel(&self, id: u64) -> bool {
        self.result_tabs
            .with_untracked(|v| v.iter().any(|p| p.id == id))
    }

    /// Has nothing run here at all? — one panel, still idle, nothing pinned.
    ///
    /// Two callers, one rule. It is the results half of the blank-slate test that
    /// decides whether a new tab may reuse this one in place; and it is what
    /// **hides the results strip**, because a bar holding one chip that says
    /// nothing is chrome charging rent on a pane that has only "Run a query" to
    /// show. A tab always holds a panel now, so "the strip is empty" stopped
    /// being the question and "the strip is the one it was born with" is.
    ///
    /// **Tracked**, so the strip's own style can ask it — the same reason
    /// `GridState::current_statement`'s reads are. It costs the other caller
    /// nothing: `place_tab` runs inside an event handler, where there is no
    /// effect to subscribe.
    pub fn results_untouched(&self) -> bool {
        self.result_tabs.with(|v| {
            v.len() == 1
                && v.first()
                    .is_some_and(|p| !p.pinned && matches!(p.state, QueryState::Idle))
        })
    }

    /// Back to one empty result — a tab being respawned as a fresh one. Closes
    /// **everything**, pins included: the tab this belongs to is not the tab that
    /// pinned them.
    pub fn reset_results(&self) {
        let ids: Vec<u64> = self
            .result_tabs
            .with_untracked(|v| v.iter().map(|p| p.id).collect());
        self.close_panels(&ids);
    }

    /// Pin or unpin a panel, moving it to the pinned block's boundary.
    ///
    /// The flag and the reordering are one move — batched, so no reader sees a
    /// strip that is pinned but not yet sorted.
    pub fn set_pinned(&self, id: u64, pinned: bool) {
        batch(|| {
            self.result_tabs.update(|panels| {
                if let Some(p) = panels.iter_mut().find(|p| p.id == id) {
                    p.pinned = pinned;
                }
            });
            let order = schemaic_core::resultsel::pin_order(&self.panel_refs(), id, pinned);
            self.rebuild(&order, Vec::new());
        });
    }

    /// Close `ids`, landing the strip on [`schemaic_core::resultsel::active_after_removal`]'s
    /// answer. A tab whose last panel goes gets a fresh idle one, so the strip is
    /// never empty.
    pub fn close_panels(&self, ids: &[u64]) {
        let refs = self.panel_refs();
        let next = schemaic_core::resultsel::active_after_removal(
            &refs,
            ids,
            self.active_result.get_untracked(),
        );
        let order: Vec<u64> = refs
            .iter()
            .map(|(id, _)| *id)
            .filter(|id| !ids.contains(id))
            .collect();
        // Batched for `begin_run`'s reason: the list and the selection are one
        // move, and a view that sees the half-applied state builds a grid nobody
        // will look at.
        batch(|| match next {
            Some(id) => {
                self.rebuild(&order, Vec::new());
                self.active_result.set(id);
            }
            None => {
                let id = self.next_panel.get_untracked();
                self.next_panel.set(id + 1);
                self.rebuild(&[id], vec![ResultPanel::idle(self.cx, id)]);
                self.active_result.set(id);
            }
        });
    }

    /// Record the statement a panel's rows **actually came from**.
    ///
    /// A filter/sort re-run replaces a panel's result without opening a new panel
    /// — that is what keeps its table on screen — so the statement it was born
    /// with stops describing what is in it. The chip's tooltip reads this, which
    /// matters most for a *pinned* filtered result: it is frozen, so the filter
    /// row that would otherwise say `WHERE …` is not offered, and the tooltip is
    /// then the only thing that can say what the rows are. The chip's **label**
    /// deliberately doesn't move — it names the result, and a name that changes
    /// under the pointer is a name nobody can point at.
    pub fn set_panel_sql(&self, id: u64, sql: &str) {
        if !self.holds_panel(id) {
            return;
        }
        self.result_tabs.update(|panels| {
            if let Some(p) = panels.iter_mut().find(|p| p.id == id) {
                p.sql = sql.to_string();
            }
        });
    }

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
            // One idle panel, so the strip has a chip from the first frame — an
            // empty strip is a bar with nothing in it, which reads as broken
            // rather than as "nothing has run".
            result_tabs: cx.create_rw_signal(vec![ResultPanel::idle(cx, 0)]),
            active_result: cx.create_rw_signal(0),
            next_panel: cx.create_rw_signal(1),
            conn_id: cx.create_rw_signal(conn_id),
            database: cx.create_rw_signal(database),
            source: cx.create_rw_signal(None),
            name: cx.create_rw_signal(None),
            pinned: cx.create_rw_signal(false),
            editing: cx.create_rw_signal(false),
            edit_buf: cx.create_rw_signal(String::new()),
            cursor_offset: cx.create_rw_signal(0),
            selection: cx.create_rw_signal(None),
            insert_req: cx.create_rw_signal(None),
            params: cx.create_rw_signal(Vec::new()),
            goto_open: cx.create_rw_signal(false),
            jump_offset: cx.create_rw_signal(None),
            format_req: cx.create_rw_signal(false),
            fix_req: cx.create_rw_signal(None),
            diagnostics: cx.create_rw_signal(Vec::new()),
            highlight_col: cx.create_rw_signal(None),
            results_maximized: cx.create_rw_signal(false),
            tx_mode: cx.create_rw_signal(TxMode::default()),
            tx: cx.create_rw_signal(TxState::default()),
            font_zoom: cx.create_rw_signal(None),
            base_sql: cx.create_rw_signal(None),
            grid_query: cx.create_rw_signal(schemaic_core::filter::GridQuery::default()),
            row_cap_override: cx.create_rw_signal(None),
            view_err: cx.create_rw_signal(None),
            view_busy: cx.create_rw_signal(false),
            path: cx.create_rw_signal(None),
            disk_sql: cx.create_rw_signal(None),
            file_format: cx.create_rw_signal(schemaic_core::sqlfile::SqlFormat::default()),
            reload_gen: cx.create_rw_signal(0),
        }
    }

    /// The tab's display title: its user-assigned name, else its file name, else
    /// the default "Query N". Reads the `name` and `path` signals reactively, so
    /// callers in a reactive scope re-run on a rename or a Save As.
    ///
    /// A user-assigned name wins over the file name on purpose: renaming a tab is
    /// an explicit act, and a Save As shouldn't silently undo it.
    pub fn title(&self) -> String {
        if let Some(name) = self.name.get() {
            return name;
        }
        match self.path.get() {
            Some(p) => schemaic_core::sqlfile::tab_title(&p),
            None => format!("Query {}", self.label),
        }
    }

    /// Has this file-backed tab drifted from the file on disk?
    ///
    /// Always false for a tab with no file: an ordinary query tab is persisted
    /// with the session and has nothing to be unsaved *against*, so a modified
    /// marker on one would mean nothing. Reactive (tracks `query`).
    pub fn modified(&self) -> bool {
        if self.path.with(|p| p.is_none()) {
            return false;
        }
        // `None` disk text = a session restored mid-edit, which we can't verify
        // without re-reading the file. Reads as modified; a save or a reload
        // settles it.
        self.disk_sql.with(|disk| match disk {
            Some(d) => self.query.with(|q| q != d),
            None => true,
        })
    }
}

/// The result the editor is talking about: the shown panel's state, as one
/// readable-and-dismissable handle.
///
/// The editor's error bar, its "AI fix" and its "Explain" all ask "what did the
/// run say?", and its `.update` dismisses that answer when the text changes.
/// They used to read a `RwSignal<QueryState>` on the tab, which stopped being
/// the truth the moment every result became a panel — this is the same four
/// operations against the panel the strip is showing, so there is one answer
/// rather than a signal to keep in step with the strip.
#[derive(Clone, Copy)]
pub struct ShownResult {
    panels: RwSignal<Vec<ResultPanel>>,
    active: RwSignal<u64>,
}

impl ShownResult {
    /// The shown result's state, tracked.
    pub fn get(&self) -> QueryState {
        let active = self.active.get();
        self.panels
            .with(|p| shown_panel(p, active).map(|p| p.state.clone()))
            .unwrap_or(QueryState::Idle)
    }

    pub fn get_untracked(&self) -> QueryState {
        let active = self.active.get_untracked();
        self.panels
            .with_untracked(|p| shown_panel(p, active).map(|p| p.state.clone()))
            .unwrap_or(QueryState::Idle)
    }

    /// The message the **editor's error bar** should show for this result, or
    /// `None`.
    ///
    /// **One line, and it needs a name.** `a24d3cc` deleted `shown_panel_error`
    /// along with its two tests, and the decision moved into an inline
    /// `create_memo` in `editor_pane.rs` — a file with no test module at all —
    /// so "only a failure puts anything in the bar" became a property nothing
    /// guarded. Widening it (a `Cancelled` carrying a message, a `Loaded` with a
    /// warning) would put text in the red bar for a run that did not fail, and
    /// with it a *View*, an *AI fix* and an *Explain* that all act on an error
    /// there isn't one of.
    pub fn bar_message(state: &QueryState) -> Option<String> {
        match state {
            QueryState::Failed(m) => Some(m.clone()),
            _ => None,
        }
    }

    /// Is the shown result a **frozen** (pinned) snapshot? Tracked, like
    /// [`Self::get`].
    ///
    /// The editor's error bar asks it. A kept failure's message cannot be typed
    /// away — that is the point of a pin — so **AI fix** and **Explain** stayed
    /// on offer over it and resolved a stale message against the *current*
    /// buffer: the model was asked to repair live SQL for an error from a
    /// statement no longer in it, and Explain additionally moved the statement
    /// highlight. The frozen exemption was created without revisiting the two
    /// consumers the "typing dismisses it" rule existed to protect.
    pub fn frozen(&self) -> bool {
        let active = self.active.get();
        self.panels
            .with(|p| shown_panel(p, active).is_some_and(|p| p.frozen()))
    }

    /// Dismiss a stale error: any edit to the text means the message no longer
    /// describes it. A no-op unless the shown result actually failed — the strip
    /// is not something typing should otherwise disturb.
    ///
    /// **And never on a kept result**, which is the same rule as everywhere else:
    /// a pinned panel is a record of what happened, and typing in the editor is
    /// not an event that happened to it. Without this, the one way to mutate a
    /// frozen panel would be the keyboard.
    pub fn dismiss_error(&self) {
        let active = self.active.get_untracked();
        let id = self
            .panels
            .with_untracked(|p| shown_panel(p, active).map(|p| p.id));
        let Some(id) = id else { return };
        self.panels.update(|panels| {
            if let Some(p) = panels
                .iter_mut()
                .find(|p| p.id == id && !p.frozen() && matches!(p.state, QueryState::Failed(_)))
            {
                p.state = QueryState::Idle;
            }
        });
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

/// Which database a dump is being composed for. `Some` on [`DumpUi::target`] is
/// what makes the modal show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DumpTarget {
    pub conn_id: u64,
    pub database: String,
    /// The PostgreSQL namespace the entry was opened on, when it was opened on
    /// one — the picker is then that namespace's tables rather than the whole
    /// database's.
    pub schema: Option<String>,
    pub dialect: SqlDialect,
    /// Which format the menu entry asked for — the modal is one panel serving
    /// six, and this is the whole of the difference between them.
    ///
    /// [`ExportFormat::Sql`](schemaic_core::export::ExportFormat::Sql) is the
    /// dump: one `.sql` file, and the six content and replay options that decide
    /// what goes in it. Every other format writes **one file per table into a
    /// folder**, and those options have nothing to say about it — a CSV has no
    /// `CREATE TABLE` to drop first and no transaction to wrap — so the modal
    /// hides them rather than showing controls the file cannot honour.
    pub format: schemaic_core::export::ExportFormat,
}

impl DumpTarget {
    /// Does this target write a folder of files rather than one `.sql`?
    ///
    /// **Asked as a capability, never as `format == Sql` at the six places that
    /// want it** — the modal branches on this for its title, its options, its
    /// dialog, its launch, its progress and its report, and a comparison spelled
    /// out six times is six chances for one of them to disagree with the others.
    pub fn writes_folder(&self) -> bool {
        !matches!(self.format, schemaic_core::export::ExportFormat::Sql)
    }
}

/// The script modal's state — the same Copy-bundle shape [`DumpUi`] uses, and
/// reset the same way on every open.
///
/// **Its own bundle rather than an arm of [`ImportUi`]**, though the user
/// reaches both through *Import*. The two share the word and the frame and
/// almost nothing else: a CSV import is twenty-odd signals about delimiters,
/// headers, null tokens and a column mapping, and a script has no columns to map
/// and no dialect to sniff. Folding it in would have meant branching every one
/// of those signals on a scope that leaves them all unread.
#[derive(Clone, Copy)]
pub struct ScriptUi {
    /// The database being loaded into, and the open flag.
    pub target: RwSignal<Option<ScriptTarget>>,
    /// The chosen file. `None` until one is picked.
    pub path: RwSignal<Option<std::path::PathBuf>>,
    /// True while the probe is reading the file's opening statements.
    pub probing: RwSignal<bool>,
    /// What the probe found — the second step's whole content.
    pub probe: RwSignal<Option<schemaic_core::script::Probe>>,
    /// True while the run itself is going. Guards a second launch and is what
    /// turns every exit into a cancel, exactly as [`DumpUi::running`] does.
    pub running: RwSignal<bool>,
    pub progress: RwSignal<Option<ScriptProgress>>,
    pub error: RwSignal<Option<String>>,
    /// The finished sentence, once a run succeeds.
    pub done: RwSignal<Option<String>>,
    /// Bumped on every open, so a late outcome can't report into a modal that
    /// has since been reopened on another database.
    pub generation: RwSignal<u64>,
}

/// Delivers a [`schemaic_core::script::Probe`] back onto the UI thread.
pub type ScriptProbeDoneFn = Rc<dyn Fn(Result<schemaic_core::script::Probe, String>)>;
/// Read the opening statements of a `.sql` file, off the UI thread, so the modal
/// can say what the file will do before it does it.
pub type ScriptProbeFn = Rc<dyn Fn(std::path::PathBuf, SqlDialect, ScriptProbeDoneFn)>;
/// Delivers a finished run's outcome back onto the UI thread.
pub type ScriptDoneFn = Rc<dyn Fn(schemaic_core::script::RunOutcome)>;
/// Run a `.sql` script against a database.
pub type ScriptFn = Rc<dyn Fn(ScriptRequest, ScriptDoneFn)>;

/// The dump modal's state — the same Copy-bundle shape [`ImportUi`] uses, reset
/// on open rather than owned by a per-open scope, and for the same reason.
#[derive(Clone, Copy)]
pub struct DumpUi {
    /// The database being dumped, and the open flag.
    pub target: RwSignal<Option<DumpTarget>>,
    /// Every table and view the picker offers, by display name.
    pub tables: RwSignal<Vec<String>>,
    /// The checked subset — what actually reaches the file.
    pub chosen: RwSignal<Vec<String>>,
    /// True while the list is being read.
    pub listing: RwSignal<bool>,
    /// The options, one signal per control — the shape [`ImportUi`] uses, and for
    /// the same reason: a toggle binds to a `bool`, and a single struct signal
    /// would make every control a read-modify-write of the other five.
    /// [`DumpUi::options`] is what assembles them.
    pub structure: RwSignal<bool>,
    pub data: RwSignal<bool>,
    pub other_objects: RwSignal<bool>,
    pub drop_if_exists: RwSignal<bool>,
    pub wrap_transaction: RwSignal<bool>,
    pub disable_fk_checks: RwSignal<bool>,
    /// True while the dump itself is running. Guards a second launch and is what
    /// turns every exit into a cancel — see `import_view`'s `exit_action`, which
    /// answers the same question for the same reason.
    pub running: RwSignal<bool>,
    /// The table currently being written, once one is.
    pub progress: RwSignal<Option<DumpProgress>>,
    pub error: RwSignal<Option<String>>,
    /// The finished sentence, once an export succeeds — built where the file's
    /// name is still in scope, so it can name both the file and whatever the
    /// export could not carry.
    pub done: RwSignal<Option<String>>,
    /// Bumped on every open, so a late outcome can't report into a modal that has
    /// since been reopened on another database.
    pub generation: RwSignal<u64>,
}

impl DumpUi {
    /// The six controls as the one value the core planner takes. A **tracked**
    /// read, so a button gating on `is_empty()` re-evaluates when a toggle moves.
    pub fn options(&self) -> schemaic_core::dump::DumpOptions {
        schemaic_core::dump::DumpOptions {
            structure: self.structure.get(),
            data: self.data.get(),
            other_objects: self.other_objects.get(),
            drop_if_exists: self.drop_if_exists.get(),
            wrap_transaction: self.wrap_transaction.get(),
            disable_fk_checks: self.disable_fk_checks.get(),
        }
    }
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
    /// Every sheet in the chosen workbook, in workbook order — the sheet
    /// picker's options. Empty for every format but Excel, and empty until a
    /// probe has read the file.
    pub sheets: RwSignal<Vec<String>>,
    /// The worksheet to import. `None` is the first one, which is both the
    /// default and what a single-sheet workbook wants — see
    /// [`schemaic_core::import::ReadConfig::sheet`].
    pub sheet: RwSignal<Option<String>>,
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
    /// True while a **probe** is reading the file.
    ///
    /// Separate from [`loading`](Self::loading) because the two mean opposite
    /// things to every exit. One flag meant both, and `exit_action` therefore
    /// routed Escape / ✕ / Cancel during a *read* to `import_cancel`, which
    /// cancels a token only the load ever writes — so a large file with one
    /// unterminated quote left a modal that no key and no button could dismiss.
    /// The same conflation reached `import::target_survives`, whose third
    /// parameter is documented in core as "a load is running": a table that
    /// really had gone during a probe took the Cancel arm, cancelled nothing,
    /// and left `target` set — the one outcome Close exists to produce.
    pub reading: RwSignal<bool>,
    /// True while the check-and-load transaction is running. See
    /// [`reading`](Self::reading) for why these are two flags.
    pub loading: RwSignal<bool>,
    /// Set while the modal writes settings into its own controls, so the effect
    /// that re-reads the file on a settings change doesn't treat the app's own
    /// answer as a new question and loop.
    pub applying: RwSignal<bool>,
    /// Bumped on every open. A probe or an import is off-thread and can outlive
    /// the modal that asked for it, so its callback checks this before writing —
    /// otherwise closing a running import and opening the modal on another table
    /// lets the first one report its result into the second one's state.
    pub generation: RwSignal<u64>,
    /// Bumped on every *probe*, which [`generation`](Self::generation) is too
    /// coarse to separate: several probes of the same file are routinely in
    /// flight at once (typing `\t` into Delimiter is three edits) and they
    /// report in completion order, so the pair is what tells the newest from an
    /// overtaken one. See [`schemaic_core::import::probe_verdict`].
    pub probe_seq: RwSignal<u64>,
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

/// One result in the results strip — one statement's, with its own lifecycle.
///
/// **Every result is one of these**, a single run's included: the strip is always
/// on screen, so there is one representation of "a result" rather than a strip
/// for a batch and a bare grid for everything else. A tab with nothing run yet
/// holds one `Idle` panel, which is what the strip shows.
///
/// A panel the user has **pinned** survives the next run (`resultsel::after_run`)
/// — the whole point of the strip — and pinning freezes it: see
/// [`ResultPanel::frozen`].
#[derive(Clone)]
pub struct ResultPanel {
    /// This panel's own child scope, under the tab's. Everything in [`PanelView`]
    /// is created in it, so closing the panel can `dispose()` it and reclaim them
    /// — the same rule a tab follows, and for the same reason.
    pub cx: Scope,
    /// Stable identity, unique within the tab. **Not the index**: panels are
    /// closed, pinned and reordered, and an index would move under every one of
    /// those — which is also why the strip's `dyn_stack` is keyed on it.
    pub id: u64,
    /// What the chip says: the statement, previewed (`history::preview`), or
    /// "Result N" when there is nothing to preview.
    pub label: String,
    /// The statement that produced it, in full — the chip's tooltip, and what
    /// tells two pinned results of the same table apart.
    pub sql: String,
    pub state: QueryState,
    /// Kept: a run replaces the unpinned panels and leaves this one alone.
    pub pinned: bool,
    /// When the run that produced it was launched (epoch seconds), for the
    /// tooltip's "4 min ago" — a pinned result's age is the thing that decides
    /// whether it is still the "before" the user wanted.
    pub ran_at: u64,
    /// Per-panel grid state, restored when the strip comes back to it.
    pub view: PanelView,
    /// Fresh-load nonce, **per panel**: part of the results body's rebuild key,
    /// so a filter/sort re-run rebuilds the grid despite `Loaded` → `Loaded`
    /// while an in-place commit splice (which does not bump it) still skips the
    /// rebuild.
    ///
    /// It was one nonce on the *tab*, which is one panel too coarse: a filter
    /// re-run landing on any panel bumped it, so the panel actually on screen
    /// was rebuilt too and lost its scroll position and its selection. The
    /// widths, the sort and a frozen column survive a rebuild; those two do not.
    pub load_gen: u64,
}

impl ResultPanel {
    /// A fresh panel in the tab's scope, `Running` until its statement lands.
    pub fn new(tab_cx: Scope, id: u64, sql: &str, ran_at: u64, ordinal: usize) -> ResultPanel {
        let cx = tab_cx.create_child();
        let preview = schemaic_core::history::preview(sql);
        ResultPanel {
            cx,
            id,
            label: if preview.trim().is_empty() {
                format!("Result {ordinal}")
            } else {
                preview
            },
            sql: sql.to_string(),
            state: QueryState::Running,
            pinned: false,
            ran_at,
            view: PanelView::new(cx),
            load_gen: 0,
        }
    }

    /// The empty panel a tab with nothing run yet shows, so the strip is never a
    /// bar with no chips in it.
    pub fn idle(tab_cx: Scope, id: u64) -> ResultPanel {
        let cx = tab_cx.create_child();
        ResultPanel {
            cx,
            id,
            label: "Result 1".to_string(),
            sql: String::new(),
            state: QueryState::Idle,
            pinned: false,
            ran_at: 0,
            view: PanelView::new(cx),
            load_gen: 0,
        }
    }

    /// **Is this result frozen?** — i.e. pinned, and therefore not a thing to
    /// write to or re-read.
    ///
    /// A pin exists to hold a result *as it was*, and two of the grid's
    /// affordances would quietly destroy that. **Editing**: a write's `WHERE` is
    /// the key columns and their original values ([`schemaic_core::model::RowEdit`]),
    /// with no guard on the columns being set — so a commit from a snapshot taken
    /// twenty minutes ago is a well-formed `UPDATE` that overwrites whatever
    /// landed in between, and the 1-row safety net cannot see it (one row *is*
    /// matched). **Server-side filter/sort**: it re-runs the statement and swaps
    /// the result in, which is exactly the "before" the user asked to keep.
    ///
    /// Everything that only rearranges what is already held stays live —
    /// client-side sort, find, copy, export, the formatters.
    pub fn frozen(&self) -> bool {
        self.pinned
    }
}

/// Per-panel grid state: what the strip has to put back when it returns to a
/// panel, rather than rebuild from scratch.
///
/// **Signals rather than plain fields on [`ResultPanel`].** The panel list is one
/// signal holding a `Vec`, so writing a column width into it would clone and
/// re-notify the whole strip on every mouse-move of a resize drag. These are
/// written by the mounted grid at that rate, so they are the panel's own signals
/// and the list never hears about them.
///
/// Selection is deliberately **not** here: it is where the user last clicked,
/// which is not a property of the result, and restoring it puts a highlight
/// somewhere nobody put it.
#[derive(Clone, Copy)]
pub struct PanelView {
    /// Measured column widths, `None` until the grid has measured them once.
    pub widths: RwSignal<Option<Vec<f64>>>,
    /// The `grid_char_w()` those widths were measured against — see
    /// `grid::GridState::widths_at`, which this restores along with them.
    pub widths_at: RwSignal<f64>,
    /// The client-side sort: `(column, ascending)`.
    pub sort: RwSignal<Option<(usize, bool)>>,
    /// The frozen (pinned-left) column's absolute index.
    ///
    /// **`frozen_col`, not `frozen`.** A *frozen column* and a *frozen result*
    /// are different things one field apart here — `gctx.panel.frozen_col` is an
    /// index, `gctx.panel_frozen` is whether the result is pinned — and the two
    /// spelled the same way is a wrong-variable bug waiting for the next reader.
    pub frozen_col: RwSignal<Option<usize>>,
}

impl PanelView {
    fn new(cx: Scope) -> PanelView {
        PanelView {
            widths: cx.create_rw_signal(None),
            widths_at: cx.create_rw_signal(0.0),
            sort: cx.create_rw_signal(None),
            frozen_col: cx.create_rw_signal(None),
        }
    }
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
    /// True while a re-introspection of this database is in flight.
    ///
    /// **What makes "the model on screen may already be out of date" askable.**
    /// `SchemaState::begin_refresh` deliberately keeps a `Loaded` database
    /// loaded while it refetches, so the tree doesn't blank — but that also
    /// means `Loaded` stopped meaning "current", and the schema *editors* seed
    /// their draft from it. Applying an `ALTER` starts a refresh and reports
    /// before it lands, so within that window `table_designer::loaded_table`
    /// still answers with the **pre-apply** `TableInfo`; one more edit to the
    /// same column then emits a MySQL `MODIFY COLUMN` restating the old
    /// definition, silently reverting what was just applied. `risks()`
    /// discloses nothing, because from the plan's view the type did not change.
    ///
    /// Read by `loaded_table`, the one funnel all four editor entry points
    /// already go through. Nothing renders it: at these durations an indicator
    /// is a glyph flickering for a frame or two, which reads as a rendering
    /// fault rather than as progress — the same call `begin_refresh` makes.
    pub refreshing: RwSignal<bool>,
    /// This database's table statistics, for the tree's size column. Its own
    /// lifecycle rather than a field on `schema`, because it is fetched by a
    /// **separate, slower and optional** query — see [`DbStatsState`].
    pub stats: RwSignal<DbStatsState>,
}

impl ConnNode {
    pub fn new(cx: Scope, id: usize, name: &str, database: &str) -> ConnNode {
        ConnNode {
            id,
            name: name.to_string(),
            database: database.to_string(),
            schema: cx.create_rw_signal(SchemaState::Loading),
            refreshing: cx.create_rw_signal(false),
            stats: cx.create_rw_signal(DbStatsState::Idle),
        }
    }
}

/// Lifecycle of one database's table statistics — the schema tree's size column.
///
/// Four states rather than an `Option`, because this signal is also the fetch's
/// **trigger and its guard**: the app's effect fetches exactly for the nodes
/// sitting at `Idle`, and moving one to `Loading` is what stops it firing again
/// on the next tick. Setting a node back to `Idle` is how a refresh asks for
/// fresh figures.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum DbStatsState {
    /// Nobody has asked. Where every node starts, and where a refresh puts it
    /// back.
    #[default]
    Idle,
    Loading,
    Loaded(schemaic_core::stats::SchemaStats),
    /// The fetch failed, or this engine publishes no statistics. Either way the
    /// column stays empty and nothing retries until a refresh — a size column
    /// that re-queried a failing server on every expand would be worse than no
    /// column.
    Unavailable,
}

/// One database's statistics slot in the schema tree's cache, found by name —
/// `None` when this connection has no such node.
///
/// Handed out as the signal rather than as the figures inside it so each caller
/// decides whether to **track** it: the results toolbar wants its row estimate to
/// appear when a fetch lands, while a context menu is built once on the click and
/// must not re-enter while it is open. Nothing here *asks* for a fetch — see
/// [`DbStatsState`] for who does.
///
/// The **lookup** is untracked either way, which is what makes this the
/// short-lived caller's version: replacing the node list (a connection-wide
/// refresh) leaves a captured slot pointing at a disposed signal. Anything that
/// keeps the slot across renders tracks the list too — `grid_view`'s `row_total`
/// memo does.
pub(crate) fn db_stats_slot(
    db_nodes: RwSignal<Vec<ConnNode>>,
    database: &str,
) -> Option<RwSignal<DbStatsState>> {
    db_nodes.with_untracked(|nodes| {
        nodes
            .iter()
            .find(|n| n.database == database)
            .map(|n| n.stats)
    })
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
    /// The database file, for SQLite — the one engine with no server. Kept
    /// alongside the server coordinates rather than replacing them so switching
    /// the engine picker back and forth doesn't discard either set.
    pub file: RwSignal<String>,
    /// The database a connection opens in when nothing else is selected. Empty
    /// means the driver decides — on PostgreSQL that is a probe, which a hosted
    /// provider permitting only its own database refuses outright.
    pub database: RwSignal<String>,
    pub ssh_enabled: RwSignal<bool>,
    pub ssh_host: RwSignal<String>,
    pub ssh_port: RwSignal<String>,
    pub ssh_user: RwSignal<String>,
    pub ssh_password: RwSignal<String>,
    /// SSH auth method + the key-pair fields (used when `ssh_auth == KeyPair`).
    pub ssh_auth: RwSignal<SshAuth>,
    pub ssh_key_path: RwSignal<String>,
    pub ssh_key_passphrase: RwSignal<String>,
    /// How the transport is secured, and the files that mode needs. Independent
    /// of the SSH block above: a tunnelled connection may still want the server
    /// to prove who it is at the far end.
    pub tls_mode: RwSignal<SslMode>,
    pub tls_ca_path: RwSignal<String>,
    pub tls_client_cert_path: RwSignal<String>,
    pub tls_client_key_path: RwSignal<String>,
    /// Chosen identity colour (a `#rrggbb` hex), or `None` for no colour.
    pub color: RwSignal<Option<String>>,
    /// Draw the identity colour as a prominent editor frame (off by default).
    pub prominent_color: RwSignal<bool>,
    /// Read-only guard-rail (off by default): disables cell edits + blocks writes.
    pub read_only: RwSignal<bool>,
    /// Environment this connection points at, shown as a top-bar badge. Defaults
    /// to `Environment::None` (no badge).
    pub environment: RwSignal<Environment>,
    /// How much of this connection's data the AI assistant may see. The form
    /// always holds a resolved level, so saving an old connection also settles
    /// its `None` — the user has now been shown a value and left it standing.
    pub ai_data: RwSignal<AiData>,
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
            file: cx.create_rw_signal(String::new()),
            database: cx.create_rw_signal(String::new()),
            ssh_enabled: cx.create_rw_signal(false),
            ssh_host: cx.create_rw_signal(String::new()),
            ssh_port: cx.create_rw_signal("22".to_string()),
            ssh_user: cx.create_rw_signal(String::new()),
            ssh_password: cx.create_rw_signal(String::new()),
            ssh_auth: cx.create_rw_signal(SshAuth::Password),
            ssh_key_path: cx.create_rw_signal(String::new()),
            ssh_key_passphrase: cx.create_rw_signal(String::new()),
            tls_mode: cx.create_rw_signal(SslMode::default()),
            tls_ca_path: cx.create_rw_signal(String::new()),
            tls_client_cert_path: cx.create_rw_signal(String::new()),
            tls_client_key_path: cx.create_rw_signal(String::new()),
            color: cx.create_rw_signal(None),
            prominent_color: cx.create_rw_signal(false),
            read_only: cx.create_rw_signal(false),
            environment: cx.create_rw_signal(Environment::None),
            ai_data: cx.create_rw_signal(AiData::default()),
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
        self.file.set(c.file.clone());
        self.database.set(c.database.clone());
        self.ssh_enabled.set(c.ssh.enabled);
        self.ssh_host.set(c.ssh.host.clone());
        self.ssh_port.set(c.ssh.port.to_string());
        self.ssh_user.set(c.ssh.user.clone());
        self.ssh_password.set(c.ssh.password.clone());
        self.ssh_auth.set(c.ssh.auth);
        self.ssh_key_path.set(c.ssh.key_path.clone());
        self.ssh_key_passphrase.set(c.ssh.key_passphrase.clone());
        self.tls_mode.set(c.tls.mode);
        self.tls_ca_path.set(c.tls.ca_path.clone());
        self.tls_client_cert_path
            .set(c.tls.client_cert_path.clone());
        self.tls_client_key_path.set(c.tls.client_key_path.clone());
        self.color.set(c.color.clone());
        self.prominent_color.set(c.prominent_color);
        self.read_only.set(c.read_only);
        self.environment.set(c.environment);
        // An unset level resolves to the default here, so the form shows the
        // level actually in force rather than a blank the user has to guess at.
        self.ai_data.set(c.ai_data.unwrap_or_default());
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
        self.file.set(String::new());
        self.database.set(String::new());
        self.ssh_enabled.set(false);
        self.ssh_host.set(String::new());
        self.ssh_port.set("22".to_string());
        self.ssh_user.set(String::new());
        self.ssh_password.set(String::new());
        self.ssh_auth.set(SshAuth::Password);
        self.ssh_key_path.set(String::new());
        self.ssh_key_passphrase.set(String::new());
        self.tls_mode.set(SslMode::default());
        self.tls_ca_path.set(String::new());
        self.tls_client_cert_path.set(String::new());
        self.tls_client_key_path.set(String::new());
        self.color.set(None);
        self.prominent_color.set(false);
        self.read_only.set(false);
        self.environment.set(Environment::None);
        self.ai_data.set(AiData::default());
    }

    /// Build a `Connection` from the current form values (with the given id).
    ///
    /// **A SQLite connection is saved without server coordinates at all** — see
    /// [`Connection::sanitized`], which this returns through. The form's own
    /// signals are untouched, so switching the picker back before saving restores
    /// what was typed.
    pub fn to_connection(&self, id: u64) -> Connection {
        let db_type = self.db_type.get_untracked();
        Connection {
            id,
            name: self.name.get_untracked(),
            host: self.host.get_untracked(),
            // A blank or unparseable port falls back to *this engine's* default.
            // It used to be a bare 3306, so clearing a PostgreSQL connection's
            // port and saving pointed it at the MySQL port — and redisplayed
            // 3306, so the value shown was the wrong one rather than the blank
            // the user left.
            port: self
                .port
                .get_untracked()
                .trim()
                .parse()
                .unwrap_or_else(|_| schemaic_core::connection::default_port(&db_type)),
            db_type,
            user: self.user.get_untracked(),
            password: self.password.get_untracked(),
            file: self.file.get_untracked().trim().to_string(),
            database: self.database.get_untracked().trim().to_string(),
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
            tls: schemaic_core::connection::Tls {
                mode: self.tls_mode.get_untracked(),
                ca_path: self.tls_ca_path.get_untracked().trim().to_string(),
                client_cert_path: self.tls_client_cert_path.get_untracked().trim().to_string(),
                client_key_path: self.tls_client_key_path.get_untracked().trim().to_string(),
            },
            color: self.color.get_untracked(),
            prominent_color: self.prominent_color.get_untracked(),
            read_only: self.read_only.get_untracked(),
            environment: self.environment.get_untracked(),
            ai_data: Some(self.ai_data.get_untracked()),
        }
        .sanitized()
    }
}

/// What a schema-tree right-click landed on. Action data (DDL, AI prompt) is
/// precomputed when the menu opens, since the row has the context then.
#[derive(Clone)]
pub enum CtxKind {
    Database {
        /// A `CREATE` script for the whole database — every namespace of it,
        /// built lazily when the menu is staged (see
        /// `DbSchema::create_ddl_script_all`). Empty when the schema hasn't
        /// loaded, in which case the entry isn't offered.
        ddl: String,
    },
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
    Field { source: TableSource, column: String },
    /// One standalone object — an enum type, a domain, a sequence, or a stored
    /// function or procedure. Carries the whole object rather than its name,
    /// because the menu needs it: its `CREATE` for Copy DDL, its current state
    /// to seed the editor without a second lookup that could disagree with the
    /// row, and — for a routine — the argument types that a `DROP` has to name
    /// and a name alone cannot supply.
    Object {
        database: String,
        item: Box<schemaic_core::schema::ObjectItem>,
        /// Its `CREATE`, built when the menu is staged.
        ddl: String,
    },
    /// One of the `Types`/`Domains`/`Sequences`/`Functions`/`Procedures`
    /// **folders**, which hold the [`CtxKind::Object`] rows. A folder is
    /// structural, so its menu is about the *set*: the script for what's in it,
    /// and creating one more of the one kind it holds — which is the entry the
    /// database node's `Create` submenu makes you find the long way round.
    ObjectGroup {
        database: String,
        /// The namespace the folder sits under (`None` when the tree is flat).
        schema: Option<String>,
        kind: schemaic_core::ddl::ObjectKind,
        /// A `CREATE` script for every object in the folder, built when the
        /// menu is staged. Empty for a folder that is somehow empty, in which
        /// case the entry isn't offered — but a folder with nothing in it
        /// renders no row at all, so that case is unreachable from the tree.
        ddl: String,
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
    /// Where to open, in window coords — `None` means "at the pointer", which is
    /// what a right-click wants and every opener but one uses.
    ///
    /// The exception is the **keyboard**: Shift+F10 on the schema tree raises the
    /// menu for the row the cursor is on, and the pointer may be anywhere at all
    /// (or never have been in the tree). Carried on the menu rather than in a
    /// signal beside it because it is a property of *this* opening, and the
    /// channel holds exactly one menu at a time.
    pub at: Option<(f64, f64)>,
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
    /// The engine label when the terminal is holding a **DB CLI** session
    /// (`schemaic_core::connection::engine_label`), `None` for an ordinary
    /// shell. Shown beside the panel title: the terminal is the one panel not
    /// bound to the active connection, so after switching connections a `mysql>`
    /// prompt is still talking to the server it was opened on, with nothing
    /// otherwise saying so.
    pub db_label: RwSignal<Option<String>>,
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
    /// Draw the accent rule down the right edge of the assistant's replies.
    /// Presentation only — it costs the reply 13px of width when on, and when
    /// off the reply's two insets are equal.
    pub gutter: RwSignal<bool>,
    /// Latest inline (Ctrl+K) generation result, previewed by the popup.
    pub inline: RwSignal<InlineAiState>,
    /// Result rows staged for the next question by the grid's "Attach to chat",
    /// shown as a chip over the input until the turn is sent or the chip is
    /// dismissed.
    ///
    /// Staging rather than sending straight away is the consent step: attaching
    /// is one gesture, the chip says exactly how much data it holds, and nothing
    /// leaves the machine until the user sends the question it belongs to.
    pub attachment: RwSignal<Option<schemaic_core::transcript::Attachment>>,
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
    /// Stop the running streamed export. The app owns the cancellation token, as
    /// it does for query runs and imports.
    pub export_cancel: Rc<dyn Fn()>,
    /// Write a binary cell's bytes to a file on a worker thread. The
    /// binary-cell panel owns the save dialog and decides whether a save may be
    /// offered at all; this writes.
    pub save_blob: BlobSaveFn,
    /// Raise the binary-cell panel on a grid cell and fetch its bytes — see
    /// [`ViewBlobFn`].
    pub view_blob: ViewBlobFn,
    /// Stop the binary-cell panel's in-flight read.
    ///
    /// The panel's exit calls this before closing, for `export_cancel`'s reason
    /// narrowed to a read: the fetch can be a 64 MiB transfer, and a panel
    /// dismissed on the first frame would otherwise hold a connection busy
    /// streaming a payload whose only reader has gone. Idempotent — cancelling
    /// a finished or absent read does nothing.
    pub cancel_blob: Rc<dyn Fn()>,
    /// Write an exported ER diagram to a file on a worker thread. The diagram
    /// modal owns the save dialog and renders the document; this rasterises (PNG
    /// only) and writes.
    pub export_erd: ErdExportFn,
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
    /// Close every tab of the active connection **except** the one named — the
    /// same set [`TabsActions::close_all_tabs`] takes, less that tab. Pinned
    /// tabs stay, and the kept tab is made active, since the right-click that
    /// asked for this may well have landed on a tab that wasn't.
    pub close_other_tabs: Rc<dyn Fn(usize)>,
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
    /// Open a new query tab containing `sql` (does NOT run it), bound to the
    /// database the statement is **for**.
    ///
    /// Pass the database whenever the SQL already names its subject — every
    /// schema-tree Generate entry does — or the tab binds to wherever a *new*
    /// tab would start, which is the last database picked, else the connection's
    /// first by name. `None` is for a snippet that belongs to no database in
    /// particular.
    pub open_query: Rc<dyn Fn(String, Option<String>)>,
    /// Ctrl+O — pick a `.sql` file and open it in a tab (reusing a blank one, as
    /// every other "open something in a tab" path does). A file already open on
    /// this connection is activated rather than opened twice.
    pub open_sql_file: Rc<dyn Fn()>,
    /// Ctrl+S — write the tab (by id) back to its file. A tab with no file yet
    /// falls through to [`Self::save_sql_file_as`], so this is always the answer
    /// to "save this".
    pub save_sql_file: Rc<dyn Fn(usize)>,
    /// Ctrl+Shift+S — pick a path and write the tab (by id) there, binding it to
    /// the new file.
    pub save_sql_file_as: Rc<dyn Fn(usize)>,
    /// Re-read the tab's file from disk, discarding unsaved edits (confirmed
    /// first when there are any). No-op for a tab with no file.
    pub reload_sql_file: Rc<dyn Fn(usize)>,
    /// Reopen the most-recently-closed tab (Ctrl+Shift+T): restores its query,
    /// connection/database, source, and name from a small ring. No-op when empty.
    pub reopen_closed_tab: Rc<dyn Fn()>,
    /// Whether [`Self::reopen_closed_tab`] has anything to restore for the active
    /// connection — same per-connection scoping it applies itself. The tab menu
    /// dims its entry rather than offering a click that does nothing.
    pub can_reopen_closed_tab: Rc<dyn Fn() -> bool>,
    /// Whether "Close other tabs" on this tab has anything to close, so the entry
    /// can be dimmed rather than silently doing nothing. Answered by the same
    /// `core::tabsel::others_to_close` the action itself calls.
    pub can_close_other_tabs: Rc<dyn Fn(usize) -> bool>,
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
/// Ctrl+W close tab · Ctrl+Tab cycle (Shift = reverse) · Ctrl+1..9 jump to the Nth tab ·
/// Ctrl+O open a `.sql` file · Ctrl+S save · Ctrl+Shift+S save as.
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
    pub open_file: Rc<dyn Fn()>,
    pub save_file: Rc<dyn Fn(usize)>,
    pub save_file_as: Rc<dyn Fn(usize)>,
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
            // Ctrl+Shift+S → Save As.
            if ch == Some("s") {
                (self.save_file_as)(self.active.get_untracked());
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
            // Ctrl+O / Ctrl+S — open a `.sql` file, and save the active tab to
            // one (Save As when it hasn't got a file yet).
            Some("o") => {
                (self.open_file)();
                true
            }
            Some("s") => {
                (self.save_file)(self.active.get_untracked());
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
    /// Bumped by the app whenever a refresh resets the [`ConnNode::stats`] slots
    /// back to [`DbStatsState::Idle`].
    ///
    /// **The reset is a write nobody watches**, deliberately: an effect that
    /// tracked the slots it fills would re-enter on its own first `Loading` write.
    /// So the refresh announces itself here instead, and *every* consumer that
    /// asks for statistics has to take this as a dependency — the tree's size
    /// column does, and the results toolbar's `of ~4.2m` didn't, which is how a
    /// Refresh deleted that figure from an on-screen capped result for good (its
    /// asker read nothing tracked and ran once, while the memo that *prints* the
    /// figure was live on the slot the refresh had just cleared).
    pub stats_gen: RwSignal<u64>,
    pub expanded: RwSignal<HashSet<String>>,
    /// The active tab's source table, highlighted in the tree.
    pub active_table: RwSignal<Option<TableSource>>,
    /// Names of databases hidden from the schema panel and search.
    pub hidden_dbs: Memo<HashSet<String>>,
    /// Show each table's on-disk size at the right edge of its tree row.
    ///
    /// Off by default and persisted. It is the cheap half of the properties
    /// work and answers the comparison question a modal can't — "which of these
    /// is the big one" — but it costs a catalogue query per expanded database,
    /// so it is opt-in.
    pub table_sizes: RwSignal<bool>,
    /// Whether the database-visibility menu is open.
    pub db_menu_open: RwSignal<bool>,
    /// Whether the SCHEMA settings menu (Refresh) is open.
    pub schema_menu_open: RwSignal<bool>,
    /// Window position of the *bottom-left* of the eye icon that opens the
    /// visibility menu, and of the gear that opens the settings menu — written by
    /// the icons themselves (`on_move`/`on_resize`), read by the two overlays.
    ///
    /// They used to be placed by arithmetic on `theme::SCHEMA_W`, a build-time
    /// default, while the panel renders at the live (persisted, and clamped on a
    /// narrow window) width — so any resize in either direction, or just a narrow
    /// window, detached both menus from their icons.
    pub db_menu_anchor: RwSignal<Point>,
    pub schema_menu_anchor: RwSignal<Point>,
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
    /// One database's table and view names, for the dump picker.
    pub dump_tables: DumpTablesFn,
    /// Introspect, plan and write a schema + data dump.
    pub dump_run: DumpFn,
    /// Introspect, plan and write one file per table into a folder — the same
    /// picker's other five formats.
    pub files_run: FilesFn,
    /// Stop the dump [`SchemaActions::dump_run`] is writing, **or the folder
    /// export [`SchemaActions::files_run`] is writing**. A no-op when nothing is
    /// running; the partial file is left as a `.part` sibling, never renamed over
    /// the destination.
    ///
    /// One stop for both because there is one modal: it refuses a second launch
    /// while either runs, so the two can never be in flight together — and giving
    /// them separate slots would only create the case where the footer's Stop
    /// reaches the wrong one.
    pub dump_cancel: Rc<dyn Fn()>,
    /// Stop the import [`SchemaActions::import_run`] is running, rolling it back.
    /// A no-op when nothing is running.
    pub import_cancel: Rc<dyn Fn()>,
    /// Read a `.sql` file's opening statements so the modal can report what the
    /// file will do before running it.
    pub script_probe: ScriptProbeFn,
    /// Run a `.sql` script against a database, statement by statement.
    pub script_run: ScriptFn,
    /// Stop the script [`SchemaActions::script_run`] is running. **What has
    /// already been applied stays applied** unless the file opened its own
    /// transaction — which is why the outcome carries a count rather than a
    /// reassurance.
    pub script_cancel: Rc<dyn Fn()>,
    /// Apply an approved DDL plan, then re-introspect the database it changed.
    pub run_ddl: DdlFn,
    /// Stop the DDL plan [`SchemaActions::run_ddl`] is applying.
    ///
    /// **Only meaningful where the engine rolls a plan back as a whole** —
    /// `ddl::ddl_rolls_back_as_a_whole`, which is what the modal's exit asks
    /// before offering this. On MySQL each statement has already committed, so
    /// stopping would leave a half-migrated table whose only report is the
    /// modal that would then be closing; that arm still refuses.
    pub ddl_cancel: Rc<dyn Fn()>,
    /// Read a MySQL view's `ALGORITHM`, which no bulk query reports.
    pub view_algorithm: ViewAlgoFn,
    pub trigger_functions: TriggerFnFn,
    /// Read the server's roles, for the database editor's Owner shortcut.
    pub roles: RolesFn,
    /// Read a MySQL trigger's body as written, which `information_schema`
    /// cannot report faithfully.
    pub trigger_source: TriggerSrcFn,
    /// The same for a MySQL routine's body, and for the same reason — see
    /// [`RoutineSrcFn`].
    pub routine_source: RoutineSrcFn,
    /// The same for a MySQL event's body — see [`EventSrcFn`].
    pub event_source: EventSrcFn,
    /// Fetch table statistics for the properties modal (whole database, one
    /// round trip) and drop the result into `overlay.properties_state`.
    pub table_stats: Rc<dyn Fn(PropertiesTarget)>,
    /// Run the exact `SELECT COUNT(*)` behind the properties modal's **Count
    /// rows**. Separate from [`SchemaActions::table_stats`] because it is a full
    /// scan the user asked for, not part of opening the panel.
    pub count_rows: Rc<dyn Fn(PropertiesTarget)>,
    /// Stop a `COUNT(*)` that is still running — on the **server**, not just in the
    /// UI. A no-op when none is.
    ///
    /// It exists because the scan is unbounded and abandoning the answer is not the
    /// same as stopping the work: without it a count on a 200M-row table held its
    /// connection for minutes after the panel had gone, and reopening offered the
    /// button again, so N opens stacked N concurrent full scans. The app also calls
    /// it whenever the modal's target changes, which covers Escape, the ✕, the
    /// backdrop and reopening on another table.
    pub count_cancel: Rc<dyn Fn()>,
    /// Fetch the server's accounts for the Users and privileges browser, into
    /// `overlay.users_state`.
    pub principals: Rc<dyn Fn(UsersTarget)>,
    /// Fetch one account's privileges, into `overlay.users_grants`. Separate
    /// from [`SchemaActions::principals`] for the reason
    /// [`SchemaActions::count_rows`] is separate from `table_stats`: it is a
    /// second round trip, per selection, and a failure to read one account's
    /// grants must not take the list of accounts down with it.
    pub grants: Rc<dyn Fn(UsersTarget, schemaic_core::users::Principal)>,
    /// Toggle the schema tree's size column, persisting the choice. Switching it
    /// on is what makes the app fetch statistics for the expanded databases.
    pub toggle_table_sizes: Rc<dyn Fn()>,
    /// Fill one database's [`ConnNode::stats`] — `(conn_id, database)` — if
    /// nobody has yet. A no-op on a node already loading, loaded, or known
    /// unavailable, and on an engine that publishes nothing, so a caller may ask
    /// freely; the slot is the guard (see [`DbStatsState`]).
    ///
    /// It exists because the size column is opt-in while the results toolbar's
    /// `1,000 of ~4.2m` is not: a capped result is exactly the moment the total
    /// is worth a catalogue query, and without this the line would only ever
    /// appear for users who had already switched the tree column on.
    pub db_stats: Rc<dyn Fn(u64, String)>,
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

/// The object the properties modal is describing.
///
/// Carries the namespace as well as the name because that is the table's
/// identity (`sales.orders` and `archive.orders` are different tables), and the
/// connection id so a fetch that lands after the user has switched connections
/// can be discarded rather than shown against the wrong server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropertiesTarget {
    pub conn_id: u64,
    pub database: String,
    pub schema: Option<String>,
    pub table: String,
    /// A view (or materialized view) rather than a base table. Views have
    /// structure but, on two of the three engines, no statistics — the modal
    /// says which rather than showing an empty grid of figures.
    pub is_view: bool,
}

impl PropertiesTarget {
    /// How the object is named in the UI — `table`, or `schema.table` outside
    /// PostgreSQL's `public`.
    pub fn display(&self) -> String {
        schemaic_core::schema::display_name(self.schema.as_deref(), &self.table)
    }
}

/// UI-facing lifecycle of the properties modal's statistics fetch.
#[derive(Clone, Debug, Default)]
pub enum PropertiesState {
    /// The fetch is in flight (the state the modal opens in).
    #[default]
    Loading,
    /// Loaded. The stats may still be *empty* — a view on MySQL has no figures —
    /// which the modal renders as "nothing to report" rather than as zeroes.
    ///
    /// Boxed: `TableStats` is a wide struct of optional figures, and this enum
    /// lives in a signal every other variant of which is a word or two.
    Loaded(Box<schemaic_core::stats::TableStats>),
    /// This engine publishes no per-table statistics at all
    /// ([`schemaic_core::stats::supports_table_stats`]). Distinct from an empty
    /// `Loaded`: the modal explains the engine rather than the table, and still
    /// offers the exact count, which every engine can answer.
    Unsupported,
    /// The fetch failed (message shown in the modal body).
    Failed(String),
}

/// What the **Users and privileges** browser is open on.
///
/// Server-level, so there is no object here — an account belongs to the server,
/// not to a database. `database` is carried anyway because PostgreSQL's schema,
/// table and sequence privileges live in the catalogue of the database holding
/// the object, so the grant list is only ever as wide as one database and has to
/// say which (see `users::pg_scope_note`). `None` is a connection with nothing
/// selected: the cluster-wide half still answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsersTarget {
    /// Read rather than taken from the active connection at fetch time, for the
    /// reason [`PropertiesTarget`] carries one: the browser describes the server
    /// it was opened on, even if the switcher has since moved.
    pub conn_id: u64,
    pub database: Option<String>,
    /// Captured at open beside `conn_id`, and for the same reason: everything
    /// the browser renders — the statements' highlighting, which write actions
    /// it offers — is about the server it was opened on, and reading the
    /// *active* connection's dialect instead would be the coupling `conn_id`
    /// exists to avoid. Its two sibling targets ([`AccountTarget`],
    /// [`GrantTarget`]) carry one for the same reason.
    pub dialect: SqlDialect,
}

/// The account list's four states.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum UsersState {
    /// The fetch is in flight (the state the browser opens in).
    #[default]
    Loading,
    /// The accounts, **and what the list does not cover** — see
    /// [`schemaic_core::users::Principals::note`]. It was a bare `Vec`, and an
    /// application account with no `SELECT` on `mysql` browsed a list of exactly
    /// itself under a footer reading "1 account", indistinguishable from a
    /// server that has one.
    Loaded(schemaic_core::users::Principals),
    /// This engine has no accounts at all
    /// ([`schemaic_core::users::supports_users`]) — SQLite. Distinct from an
    /// empty `Loaded`, which would read as "a server with no users".
    Unsupported,
    /// The fetch failed (message shown in the browser's list pane). The likely
    /// cause is the connection lacking `SELECT` on `mysql` and on nothing else
    /// either, which is a sentence worth showing rather than an empty list.
    Failed(String),
}

/// One account's privileges, per selection.
///
/// `Idle` is its own state rather than an empty `Loaded`: before a row is
/// picked, the pane invites a selection instead of claiming the account with no
/// name has no privileges.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum GrantsState {
    #[default]
    Idle,
    Loading,
    Loaded(schemaic_core::users::Grants),
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
    /// The Import Connections modal, raised from the same list.
    pub import: ConnImportUi,
}

/// The Import Connections modal's signals (Copy bundle).
///
/// A second modal rather than a section of Manage Connections: what it shows is
/// a *review list* of proposals, and every control on it — the paste field,
/// Choose a file, Scan installed clients, Select all, Import — is about the
/// whole list rather than about the one connection the form beside it is
/// editing.
#[derive(Clone, Copy)]
pub struct ConnImportUi {
    /// Whether the modal is open.
    pub open: RwSignal<bool>,
    /// True while this machine is being searched for source files. The walk is
    /// short, but it is filesystem work on a home directory whose size we don't
    /// control, so the list says what it is doing rather than appearing empty.
    pub scanning: RwSignal<bool>,
    /// Whether a scan of the installed clients has **finished** in this opening.
    ///
    /// The empty list means three different things and they need three
    /// different sentences (`connection_import::empty_message`): before a scan
    /// it is an invitation, during one it is progress, after one it is the
    /// answer — and without this bool, pressing *Scan installed clients* on a
    /// machine with none of them installed leaves the screen byte-for-byte
    /// identical, which reads as a dead button.
    pub scanned: RwSignal<bool>,
    /// Every connection on offer, in `conn_import`'s order.
    pub rows: RwSignal<Vec<schemaic_core::conn_import::Imported>>,
    /// Which of them are ticked, **by index into `rows`**.
    ///
    /// A set, so a row's read is O(1) — the dump modal's table picker learned
    /// that the hard way, quadratically.
    ///
    /// **Indices are safe only because `rows` never reorders.** Every source
    /// appends through `conn_import::merge_rows`, which pushes at the end and
    /// answers a repeat with the position already holding it; the sole thing
    /// that clears `rows` is `open_import`, which resets this set in the same
    /// breath. A source that replaced the list instead would leave every ticked
    /// index pointing at a different connection.
    pub chosen: RwSignal<std::collections::HashSet<usize>>,
    /// Entries a source held that are not on offer, so the modal can say what it
    /// left behind instead of quietly shortening the list.
    pub skipped: RwSignal<Vec<schemaic_core::conn_import::Skipped>>,
    /// The paste field's contents.
    pub paste: RwSignal<String>,
    /// Why the pasted text isn't a connection URL, if it isn't one.
    pub paste_error: RwSignal<Option<String>>,
    /// Why the file the user picked could not be read.
    ///
    /// **Its own slot, because it is not the paste field's news.** A failed
    /// *Choose a file…* used to write into `paste_error`, so a file button's
    /// failure was rendered under the **Connection URL** field — and stayed
    /// there over a good URL typed afterwards, since only a paste clears that
    /// signal.
    pub file_error: RwSignal<Option<String>>,
    /// The sentence shown after connections are added.
    pub done: RwSignal<Option<String>>,
}

/// Connection-management callbacks (owned by the app).
pub struct ConnActions {
    pub switch_conn: Rc<dyn Fn(u64)>,
    pub select_conn: Rc<dyn Fn(u64)>,
    pub new_conn: Rc<dyn Fn()>,
    /// Copy a saved connection by id — server, credentials and guard-rails
    /// carried, new identity — persist it, and select the copy for editing.
    /// Offered from the connection list's right-click menu.
    pub duplicate_conn: Rc<dyn Fn(u64)>,
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
    /// Open the Import Connections modal, empty.
    ///
    /// It deliberately does **not** scan: the modal offers three ways in — a
    /// pasted URL, a file, and the installed clients — and only the third one
    /// touches the filesystem, so it is the one the user asks for rather than
    /// the one that happens because a dialog opened.
    pub open_import: Rc<dyn Fn()>,
    /// Search this machine for the files DBeaver, the JetBrains IDEs and the
    /// command-line clients keep their connections in.
    pub scan_installed_clients: Rc<dyn Fn()>,
    /// Parse whatever is in the paste field and add it to the review list.
    pub add_pasted_url: Rc<dyn Fn()>,
    /// Pick a source file by hand, for a layout the search doesn't cover (a
    /// project's own `.idea/dataSources.xml`, an exported file, a `.env`).
    pub choose_import_file: Rc<dyn Fn()>,
    /// Save the ticked rows as real connections — the only step that writes.
    pub import_chosen: Rc<dyn Fn()>,
}

/// Query-history signals (Copy bundle). The full list across all connections;
/// the panel filters it to the active connection.
#[derive(Clone, Copy)]
pub struct HistoryUi {
    pub entries: RwSignal<Vec<HistoryEntry>>,
}

/// The snippet library's state (Copy bundle).
#[derive(Clone, Copy)]
pub struct SnippetsUi {
    /// The whole library this connection sees: the user's snippets **plus the
    /// built-in pack for its engine** (`snippet::library`, merged once in the
    /// app). The panel narrows them further itself through `snippet::grouped`,
    /// because which ones apply is a decision and it lives in the core with
    /// tests.
    ///
    /// A `Memo`, not the persisted signal: writing a built-in into the user's
    /// `snippets.json` would freeze it there, where a later release could not
    /// fix it.
    pub items: Memo<Vec<schemaic_core::snippet::Snippet>>,
    /// Whether [`SnippetActions::save_current`] would save anything — the `+` is
    /// dimmed rather than offering a click that does nothing. A memo, not a
    /// predicate closure: it has to re-run as the editor's text changes, and the
    /// toolbar icon takes a `Copy` reader.
    pub can_save: Memo<bool>,
}

/// Snippet-library callbacks (owned by the app).
pub struct SnippetActions {
    /// Insert a snippet's body at the caret of the active tab, and record the
    /// use. The click action of a library row.
    pub insert: Rc<dyn Fn(schemaic_core::snippet::Snippet)>,
    /// Open the body in a new tab instead — the row menu's entry, for a snippet
    /// you want beside what you are writing rather than inside it.
    pub open_in_tab: Rc<dyn Fn(schemaic_core::snippet::Snippet)>,
    /// Save the active tab's selection (or its whole buffer) as a new snippet,
    /// returning the new id so the panel can open its name for editing. `None`
    /// when there was nothing to save.
    pub save_current: Rc<dyn Fn() -> Option<u64>>,
    /// Rename a snippet (persists).
    pub rename: Rc<dyn Fn(u64, String)>,
    /// Set or clear a snippet's expansion abbrev — `None` removes it, which is
    /// the only way to take a trigger back off a snippet.
    pub set_abbrev: Rc<dyn Fn(u64, Option<String>)>,
    /// Replace a snippet's SQL — the snippet editor's only reason to exist.
    pub set_body: Rc<dyn Fn(u64, String)>,
    /// Open the snippet editor on a snippet (name, abbrev and body together).
    pub edit: Rc<dyn Fn(u64)>,
    /// Move a snippet to another scope — which connections it is offered on.
    /// The choices come from `snippet::scope_options`, so the picker's order and
    /// the panel's bands cannot drift apart.
    pub set_scope: Rc<dyn Fn(u64, schemaic_core::snippet::Scope)>,
    /// Copy a snippet to a new one — the only way to edit a built-in.
    pub duplicate: Rc<dyn Fn(u64)>,
    /// Delete a snippet, behind the shared confirm.
    pub remove: Rc<dyn Fn(u64)>,
}

/// Query-history callbacks (owned by the app).
pub struct HistoryActions {
    /// Clear the history for the currently-active connection (persists).
    pub clear: Rc<dyn Fn()>,
    /// Reopen a history entry in a new tab: seeds the SQL, the database it ran
    /// against, and the originating tab's custom name (does NOT run it).
    pub open: Rc<dyn Fn(HistoryEntry)>,
    /// Delete one entry (the row menu), persisting. Takes the whole entry
    /// because `(conn_id, sql)` is what identifies it — see
    /// [`schemaic_core::history::remove`]. Unlike [`Self::clear`] this is not
    /// behind a confirm: it destroys one row that the user right-clicked, and
    /// re-running the statement records it again.
    pub remove: Rc<dyn Fn(HistoryEntry)>,
}

/// What the Server Activity panel currently has to show.
///
/// A refresh that lands on a `Loaded` panel replaces the snapshot in place — it
/// never passes back through `Loading`. The list would otherwise blank out on
/// every poll, which on a two-second interval is a panel that flashes rather than
/// one that updates.
#[derive(Clone, Debug, PartialEq)]
pub enum ActivityState {
    /// Nothing asked for yet — the panel has not been open on this connection.
    Idle,
    /// The first fetch for this connection is in flight.
    Loading,
    /// A snapshot, already ordered and capped by
    /// [`schemaic_core::activity::prepare`]. `truncated` says the cap left
    /// sessions out, and is `false` in every ordinary case.
    ///
    /// A flag rather than a count, because a count would be a lie: the fetch
    /// asks for `MAX_SESSIONS + 1` rows, so anything `prepare` could subtract is
    /// `1` whether the server holds five hundred and one sessions or four
    /// thousand.
    /// `Rc`, not a plain `Vec`: this state is read out of a signal by three
    /// `dyn_container` keys per poll, and each read used to deep-copy up to five
    /// hundred `SessionInfo` — every one carrying an unbounded `sql`, which on
    /// MySQL is `PROCESSLIST.INFO`, the client's **complete** statement text
    /// with no server-side clip. A refcount bump says the same thing.
    Loaded {
        sessions: Rc<Vec<schemaic_core::activity::SessionInfo>>,
        truncated: bool,
    },
    /// The fetch failed — the message is the server's, shown in place of the list.
    Failed(String),
    /// This connection's engine has no server sessions
    /// ([`schemaic_core::activity::supports_activity`]).
    Unsupported,
}

/// Server-activity signals (Copy bundle).
#[derive(Clone, Copy)]
pub struct ActivityUi {
    pub state: RwSignal<ActivityState>,
    /// The **active connection's** auto-refresh interval in seconds; `0` is off.
    /// Derived, not settable — the store it comes from is keyed by `conn_id`
    /// ([`schemaic_core::activity::IntervalRule`]), so the panel reads this and
    /// writes through [`ActivityActions::set_interval`]. A plain `RwSignal` here
    /// would need an effect writing back into the store and another reading out
    /// of it, which is a loop waiting to be closed.
    pub interval: Memo<u64>,
    /// A fetch is in flight. Only the refresh button reads it — the list keeps
    /// showing the previous snapshot meanwhile.
    pub busy: RwSignal<bool>,
    /// The last kill that failed, shown as a line above the list and cleared by
    /// the next refresh or kill.
    ///
    /// **Separate from [`ActivityState::Failed`], and that separation is the
    /// point.** A kill that is refused — no `CONNECTION_ADMIN`, no
    /// `pg_signal_backend` — says nothing about the snapshot on screen, but
    /// routing it through the panel's state threw the whole session list away and
    /// replaced it with the error string. That is the list someone was reading
    /// mid-incident, and with auto-refresh set to Off nothing brought it back.
    /// `Failed` is for "there is no snapshot"; this is for "the snapshot stands,
    /// and the thing you just asked for didn't happen".
    pub kill_error: RwSignal<Option<String>>,
    /// Whether the clock's poll-interval dropdown is open.
    ///
    /// It lives up here rather than inside the panel because the panel is
    /// **clipped** — `body` wraps the right column in a `clip()` for the
    /// collapse animation — so a menu drawn inside it would be cut off at the
    /// panel's own edge. Like the schema tree's eye and gear menus, it is a
    /// root-level overlay positioned from an anchor the icon publishes.
    pub menu_open: RwSignal<bool>,
    /// The clock icon's bottom-**right** corner in window coordinates, published
    /// by the icon's `on_move`/`on_resize`. Right, not left, because the panel is
    /// against the window's right edge and the menu is right-aligned to it — see
    /// `activity_menu_overlay`.
    pub menu_anchor: RwSignal<floem::kurbo::Point>,
}

/// Server-activity callbacks (owned by the app).
pub struct ActivityActions {
    /// Fetch the active connection's sessions now. A no-op while one is already
    /// in flight.
    ///
    /// The *timer* behind it runs only while the panel is open and the window has
    /// focus — every tick is a connect and an authenticate against the server
    /// being watched, and a panel polling behind another window is load charged to
    /// a server nobody is looking at. This callback itself always asks, since the
    /// two things that call it by hand (the refresh button, and the tidy-up after
    /// a kill) are the user looking.
    pub refresh: Rc<dyn Fn()>,
    /// Cancel a statement or terminate a session, behind the shared confirm, then
    /// refresh. The confirm is raised by the app rather than here so the panel
    /// can't offer a kill that skips it.
    pub kill: Rc<dyn Fn(i64, schemaic_core::activity::KillKind)>,
    /// Set the **active connection's** poll interval (seconds; `0` is off) and
    /// persist it. Which connection that is belongs to the app, not the panel.
    pub set_interval: Rc<dyn Fn(u64)>,
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
    /// Interface scale (chrome type + layout metrics); drives
    /// `theme::set_ui_scale`. Not the editor or terminal font — those have their
    /// own size settings (see [`theme::UiScale`]).
    pub ui_scale: RwSignal<theme::UiScale>,
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
    /// Cancel a running statement after this many seconds; **`0` is off**, and
    /// off is the default. Read once per run at the app boundary and handed to
    /// the run's own cancellation token — see `persist::statement_timeout`.
    pub statement_timeout: RwSignal<u64>,
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
///
/// **`PartialEq` is load-bearing, not a convenience.** `popup_menu` is one slot
/// with no tag saying who filled it, so a trigger that wants to *close* the menu
/// it opened — rather than dismiss and rebuild the identical panel — has to
/// recognise it, and the anchor is the only part of the open state that names a
/// place rather than a payload. The grid's toolbar dropdowns compare against
/// their own (see `grid::grid_toolbar`), which works precisely because every
/// opener overwrites this signal with its own placement as it opens: there is no
/// separate marker to go stale, and nothing for the other openers to reset.
///
/// The comparison is exact, floats included, and that is the intent — both sides
/// are computed from one origin signal, so they agree until a relayout moves the
/// icon, at which point the anchor is stale anyway and failing the test merely
/// reopens.
#[derive(Clone, Copy, PartialEq)]
pub enum PopupAnchor {
    /// Toolbar dropdown (grid Copy): the panel drops a few px below the icon and
    /// grows downward, left-aligned under it (overlapping it); if that would spill
    /// past the window's bottom it flips to grow upward. `(icon_left, icon_right,
    /// icon_bottom)` in window coords; the width comes from `popup_width`.
    BelowIcon(f64, f64, f64),
    /// A menu dropping from a **box** — an enum field, a grid cell open for
    /// editing: left edge flush with the control's own, so the list lines up with
    /// the value it replaces. Flips to right-aligned at the window's right edge,
    /// and **above the box** when there is no room below it. `(box_left,
    /// box_right, box_top, box_bottom)` in window coords.
    ///
    /// It carries `box_top` where [`PopupAnchor::BelowIcon`] does not, and that is
    /// the difference between the two flips: a box is a control the user is
    /// looking at and clicking — the row panel's enum field, an open cell — so a
    /// menu that flipped up from its *bottom* edge covered the very thing it is
    /// about (`widgets::box_menu_inset`). An icon's menu deliberately overlaps its
    /// 28px glyph.
    ///
    /// Distinct from `BelowIcon` for a second reason: that one tucks the panel
    /// under the glyph by opening 40px left of its **right** edge — which,
    /// measured from a 200px cell, puts the menu most of the way across it.
    BelowBox(f64, f64, f64, f64),
    /// Status-bar segment menu: centered on the segment's x-range and sitting 5px
    /// above the footer, growing upward. `(seg_left, seg_right)` in window coords.
    AboveFooter(f64, f64),
}

/// An open calendar: the buffer it edits, what a picked day writes into it, and
/// the control it dropped from.
///
/// The **anchor is its identity**, exactly as [`PopupAnchor`] is a menu's: one
/// channel, no tag saying who filled it, so the control that opened it recognises
/// its own panel by the rect it would anchor at and closes it instead of
/// reopening (`cell_editors::toggle_calendar`).
///
/// `buf` belongs to the view that opened this — a row panel field, an inline cell
/// editor — and outlives it only in the moment before something clears the
/// channel. The overlay therefore reads it through `try_get`, and closes itself
/// when the answer is "that signal is gone".
#[derive(Clone)]
pub struct DatePick {
    /// The text the calendar edits, the same signal the field beside it binds to.
    pub buf: RwSignal<String>,
    /// Which of the two date editors this column is — it decides what a picked
    /// day writes (`celledit::set_date` keeps a datetime's time of day) and
    /// whether the footer offers **Now** or **Today**.
    pub editor: schemaic_core::celledit::CellEditor,
    /// The control's rect in window coords: `(left, right, top, bottom)`, and the
    /// panel's identity.
    ///
    /// **Both** vertical edges, unlike [`PopupAnchor::BelowBox`]'s three numbers:
    /// the panel drops from the bottom but flips above the *top*, so that a
    /// calendar with no room below it doesn't cover the button that opened it —
    /// and that button is also the one that closes it
    /// (`widgets::box_menu_inset`).
    pub anchor: (f64, f64, f64, f64),
    /// What to run once a day (or **Now**) has been written into `buf` — and
    /// only then, which is why it is here rather than on the channel: the panel
    /// is closed by a pick, by Escape and by a click away, and only the opener
    /// knows what to make of the difference.
    ///
    /// `None` in the row panel, where the field beside the calendar is what
    /// commits. The grid's inline cell editor fills it in: a cell has no Save
    /// button in reach, so choosing a day stages the edit and closes the editor,
    /// exactly as choosing from an in-cell picker does.
    pub on_pick: Option<Rc<dyn Fn()>>,
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
    /// The open date picker, or `None`. Its own channel rather than a `MenuEntry`
    /// list because a calendar is a *grid*, not a menu — but it lives at the
    /// window root for the same reason the menus do: the field it drops from sits
    /// in a scrolling strip at the bottom of the results area, and a panel nested
    /// under it would be clipped by that scroll and by the area's own edge. It is
    /// on `widgets::MenuFlags` for the same reason again: everything that
    /// dismisses a menu dismisses this. See [`DatePick`].
    pub date_pick: RwSignal<Option<DatePick>>,
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
    /// Whether the text in `error_modal_text` is a **statement** failure — a
    /// Run-Everything statement's own error — as opposed to a commit error, a
    /// failed export or a server that never answered. It decides whether the
    /// modal may offer "AI fix" and "Explain": those act on a statement, and an
    /// override was treated as "not one" across the board, which left the single
    /// case `intel::error_fix_range` exists for with no fix affordance anywhere.
    /// Cleared alongside the text.
    pub error_modal_fixable: RwSignal<bool>,
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
    /// The last poll filled its row cap, so the monitor is watching a *page* of
    /// the table rather than the table. Said in the status line, because a change
    /// beyond that page is invisible and no amount of diffing can recover it.
    pub monitor_partial: RwSignal<bool>,
    /// Poll interval in seconds (the popup's dropdown). Read by the poll loop on
    /// each re-arm, so a change takes effect on the next tick. Session-only.
    pub monitor_interval: RwSignal<u64>,
    /// The Pause toggle. The loop keeps re-arming while paused and skips only the
    /// *fetch*, so resuming costs nothing and needs no fresh `open_monitor` — but
    /// it also means the baseline snapshot ages: the first poll after a resume
    /// diffs against the pre-pause table and logs the **net** change, stamped at
    /// the resume. That is the log's existing rule (an entry is timestamped when
    /// a poll observed it, not when it happened), just at a coarser interval.
    pub monitor_paused: RwSignal<bool>,
    /// A change-log export that failed to write, held until the next export
    /// attempt or a close.
    ///
    /// Deliberately **not** `monitor_error`: the poll loop clears that on its
    /// next success, so a write failure reported there would vanish within one
    /// interval — on the one action whose whole point is that the log outlives
    /// the modal.
    pub monitor_export_err: RwSignal<Option<String>>,
    /// **The log as it now stands has been written to a file.** Set by a
    /// successful export, cleared the moment the poll appends anything, and reset
    /// by `open_monitor`.
    ///
    /// It is what makes the Clear confirmation worth reading rather than a
    /// reflex: the log is the only record of a deleted row's values, so throwing
    /// it away is irreversible — unless there is a copy on disk, which is the
    /// ordinary case after an export
    /// ([`schemaic_core::monitor::discard_needs_asking`]).
    pub monitor_exported: RwSignal<bool>,
    /// How many entries the log has dropped off the top to stay within
    /// [`schemaic_core::monitor::LOG_CAP`].
    ///
    /// The status line's "the oldest are dropping" caveat reads this rather than
    /// the log's length: at exactly the cap nothing has been dropped yet, and a
    /// record that claims a loss it hasn't had is the same failure as one that
    /// hides a loss it has. Set from
    /// [`schemaic_core::monitor::trim_log`]'s return.
    pub monitor_dropped: RwSignal<usize>,
    /// ER-diagram modal: `Some(target)` opens it for that database/seed.
    pub erd: RwSignal<Option<ErdTarget>>,
    /// Table-properties modal: `Some(target)` opens it for that object, and the
    /// statistics fetch it kicks off lands in `properties_state`.
    ///
    /// The exact `COUNT(*)` is tracked apart from that state on purpose. It is a
    /// second, slower request over the same object, and folding it into
    /// `properties_state` would mean a failed count replacing statistics that
    /// loaded perfectly well — so `properties_counting` drives only the button's
    /// own spinner, `properties_count_err` only the line beneath it, and a
    /// successful count is written into the loaded [`PropertiesState::Loaded`]'s
    /// `exact_rows` where the rest of the panel can read it.
    pub properties: RwSignal<Option<PropertiesTarget>>,
    pub properties_state: RwSignal<PropertiesState>,
    pub properties_counting: RwSignal<bool>,
    pub properties_count_err: RwSignal<Option<String>>,
    /// Users and privileges browser: `Some(target)` opens it for that server.
    ///
    /// The selection and the grant fetch it drives are tracked apart from
    /// `users_state`, for the reason the properties modal's exact count is: they
    /// are a second request per row, and folding them together would let one
    /// account's unreadable grants replace a list of accounts that loaded fine.
    pub users: RwSignal<Option<UsersTarget>>,
    pub users_state: RwSignal<UsersState>,
    /// The filter box's text. Not persisted — a filter is about the question
    /// being asked right now, and a stale one on reopening reads as a short list.
    pub users_filter: RwSignal<String>,
    pub users_selected: RwSignal<Option<schemaic_core::users::Principal>>,
    pub users_grants: RwSignal<GrantsState>,
    /// Bumped by every account fetch, and compared when its answer lands —
    /// `DdlUi::generation`'s pattern, for the same reason and after the same
    /// failure.
    ///
    /// Two `fetch_principals` are routinely in flight on an *identical*
    /// `UsersTarget` (a create is a fetch, and so is closing the preview after
    /// it), and the reporter's only guard was target identity — so the later
    /// request was not guaranteed to be the last writer. The list could settle
    /// on the **pre-mutation** snapshot: an account just created missing, or,
    /// worse, one just dropped listed again with a live Drop beside it. The
    /// browser has no other way back to the truth than closing and reopening.
    pub users_generation: RwSignal<u64>,
    /// The snippet whose body is being edited, or `None`. One at a time, like
    /// every other editor here.
    pub snippet_edit: RwSignal<Option<u64>>,
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

/// One entry in the Live Monitor's change log. Defined in the core beside the
/// diff that produces it, because the log's **export** is a pure projection of
/// these entries ([`schemaic_core::monitor::log_result_set`]) and belongs with
/// the rest of the change detection; re-exported here so the signal's type still
/// reads as a UI type at every use site.
pub use schemaic_core::monitor::MonitorEntry;

/// Open the Live Monitor for a table on a connection — starts polling that table
/// and reveals the modal. Built in the app, invoked from the grid toolbar.
pub type MonitorFn = Rc<dyn Fn(u64, TableSource)>;

/// Open the table-properties modal for a table on a connection — the results
/// toolbar's entry point, beside the schema tree's own context-menu entry.
///
/// Built where the whole [`Ui`] is in reach (`workspace`) rather than in the app,
/// because everything it needs is already there: the modal fetches its own
/// figures, and whether the object is a view comes from the loaded schema.
pub type PropertiesFn = Rc<dyn Fn(u64, TableSource)>;

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
    /// The schema + data dump modal (opened from a database's context menu).
    pub dump: DumpUi,
    /// The grid export's progress modal — the one surface that says how far an
    /// export has got, for both scopes.
    pub export: ExportUi,
    /// The binary-cell panel — the hex/preview modal a `<n bytes>` cell opens
    /// into, and the only surface in the app that holds a blob's real bytes.
    pub blob: BlobUi,
    /// The script-load modal — **Import** on a database or namespace node, the
    /// inverse of the *Export* directly above it in the same menu.
    pub script: ScriptUi,
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
    // The snippet library — grouped like history, and beside it in the panel
    // column.
    pub snippets: SnippetsUi,
    pub snippet_actions: Rc<SnippetActions>,
    // Server activity (the sessions on the connected server) — grouped.
    pub activity: ActivityUi,
    pub activity_actions: Rc<ActivityActions>,
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
    /// Per-table identity colours (persisted to the same `db_colors.json`), keyed
    /// by `(conn_id, database, display name)`; set from the schema tree, shown as
    /// a dot on the table row and as a tint on the table's ER-diagram card header.
    pub table_colors: RwSignal<Vec<TableColorRule>>,
    /// Persist both colour stores to disk (after a menu upsert). One closure for
    /// the pair, because they share one file.
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
    /// How far the background auto-updater has got. Driven at the app boundary
    /// (Velopack talks to the GitHub Releases feed on a worker thread); the
    /// header's update chip renders whatever [`UpdateState::label`] returns, which
    /// for most of the life of most sessions is nothing at all. Transient (never
    /// persisted).
    pub update_state: RwSignal<UpdateState>,
    /// Restart into the staged update. Only ever called while
    /// [`UpdateState::is_actionable`] holds — the header chip is inert otherwise
    /// — because Velopack exits the process to hand over to the updater.
    pub apply_update: Rc<dyn Fn()>,
    /// Reveal the app's config directory in the OS file manager — the folder
    /// holding `schemaic.log` alongside `tabs.json` and the rest of the state.
    ///
    /// The log is written and rotated and, until this existed, **finding it was
    /// the user's problem**: the one artefact a crash report needs lived at a
    /// path nothing in the app ever said out loud. Spawning a file manager is a
    /// process launch, which is the app boundary's job and not a view's — hence
    /// a callback rather than a `Command` in `settings.rs`.
    pub open_config_dir: Rc<dyn Fn()>,
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
    Snippets,
    Activity,
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
            S::Snippets => RightPanel::Snippets,
            S::Activity => RightPanel::Activity,
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
            RightPanel::Snippets => S::Snippets,
            RightPanel::Activity => S::Activity,
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
/// (connection menu, Find Anywhere, Manage Connections) stacked on top, and the
/// window's own resize border over all of it.
///
/// Takes the `WindowId` because the window has no title bar of its own any more
/// (`WindowConfig::show_titlebar(false)`): the caption buttons in the header are
/// ours, and they need the window to minimize, maximize and close it. See
/// `window_chrome`.
pub fn workspace(ui: Ui, window: WindowId) -> impl IntoView {
    let chrome = window_chrome::WindowChrome::new(window);
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
    // Drop the hoisted submenu when no menu is open at all. `menu_panel` clears it
    // whenever its own `open_sub` goes `None`, which covers Escape and running an
    // entry — but a **click-away dismissal** sets the channel to `None` directly
    // and takes the panel's whole scope with it, so that effect never runs again
    // and the submenu would be left floating over the app with nothing behind it.
    // Both channels, because a submenu can come from either.
    create_effect(move |_| {
        if popup_menu.get().is_none()
            && context_menu.get().is_none()
            && widgets::hoisted_submenu().get_untracked().is_some()
        {
            widgets::hoisted_submenu().set(None);
        }
    });
    let root_menus = widgets::MenuFlags::of(&ui);
    // **Published once, here, before any view that opens a menu is built.** Every
    // `<select>`-shaped control in the app drops the shared popup menu, and most
    // of them are built by helpers two calls deep from anything holding a `Ui` —
    // `connection_form::server_fields`, `routine_editor::bound_choice`. See
    // `widgets::set_menu_channel` for why a singleton is published rather than
    // threaded.
    widgets::set_menu_channel(widgets::PopupChannel {
        menus: root_menus,
        anchor: ui.overlay.popup_anchor,
        width: ui.overlay.popup_width,
    });
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
        open_file: ui.tab_actions.open_sql_file.clone(),
        save_file: ui.tab_actions.save_sql_file.clone(),
        save_file_as: ui.tab_actions.save_sql_file_as.clone(),
    };
    let shell = v_stack((
        header(ui.clone(), chrome),
        body(ui.clone(), schema_visible, right_panel),
        footer(ui.clone()),
    ))
    .style(|s| {
        s.size_full()
            .flex_col()
            .background(theme::bg_editor())
            .color(theme::text())
            .font_size(theme::font_title())
    });

    // The modal layer and the title-bar band ask the same question, so it is
    // asked once and handed to both. The two must not be able to disagree: a
    // band without a backdrop dims a live header, a backdrop without a band is
    // the bug that whole layer exists to fix. (The three *group* predicates it
    // also reads are its own business — see `modals`.)
    let modal_up = modals::modal_backdrop_up(&ui);
    // Two views, placed **inside** `root` rather than out with the resize zones —
    // see the tuple below for where and why.
    let [band, band_border] = chrome.over_backdrop(modal_up);

    let root = stack((
        shell,
        conn_menu_overlay(ui.clone()),
        active_db_menu_overlay(ui.clone()),
        db_visibility_overlay(ui.clone()),
        schema_settings_overlay(ui.clone()),
        activity_menu_overlay(ui.clone()),
        context_menu_overlay(ui.clone()),
        // **Every modal, in one layer that starts below the title bar** — the
        // layer, its paint order and the four predicates that raise it live in
        // `modals`, which states in full why the box stops at `header_h()` and
        // why a modal mounted outside it silently takes the title bar with it.
        // It is one entry here because five becoming one is the room floem's
        // 16-arity `ViewTuple` limit leaves the next overlay.
        modals::modal_layer(ui.clone(), modal_up),
        // **The title-bar band, above every modal and below every overlay that
        // can open over one.** It dims the header a modal cannot reach (the layer
        // above starts at `header_h()`) and makes it draggable while one is up, so
        // it has to be painted after the modal layer — but it was mounted out with
        // the resize zones, *above `root` entirely*, and a sibling that paints
        // later is also hit first. The three overlays below are precisely the ones
        // that open from inside a modal, and a tall one takes `menu_inset`'s
        // "bigger than the window" arm and pins at y=0: its top `header_h()` px
        // then rendered under the band's scrim and answered presses with an OS
        // window drag instead of with the row the pointer was on. Here, the band
        // covers the header and nothing that stands over it.
        band,
        band_border,
        // **After every modal, on purpose.** A sibling paints in tuple order, so
        // anything before this is covered by it — and the shared popup menu is
        // opened from *inside* modals too (the designer's type shortcut), where
        // being painted behind the panel and its backdrop made it invisible. Only
        // `submenu_layer` below sits above it, and that is this menu's own
        // submenu rather than a surface competing with it.
        // Below the menus: a calendar is dismissed by the same press that opens
        // one, and never draws over one.
        date_pick_overlay(ui.clone()),
        popup_menu_overlay(ui),
        // **After even the popup menu**, because it draws that menu's own open
        // submenu. A submenu is hoisted out of the row it belongs to and drawn
        // here instead: nested under its row it would be painted but never
        // hit-tested whenever it had to flip left or up, since Floem grows a
        // parent's hit area rightward and downward only. See `widgets`'
        // "the hoisted submenu" and the Floem gotcha it points at.
        //
        // Out of flow and shrink-wrapped to the panel, like every other overlay
        // here — a full-window layer would swallow clicks meant for the app.
        widgets::submenu_layer(),
    ))
    // Track the pointer in window coordinates (root-local == window) so the
    // schema context menu can anchor at the cursor.
    .on_event(EventListener::PointerMove, move |e| {
        if let Some(p) = e.point() {
            last_mouse.set((p.x, p.y));
        }
        EventPropagation::Continue
    })
    // Publish "the pointer came up", wherever it came up. A drag that starts in
    // one view routinely ends in another — floem delivers the release to
    // whatever is under the cursor — so a view holding a button-is-down flag
    // cannot see the end of its own gesture. `_cont`, so this observes without
    // taking the event from anyone. See `widgets::pointer_released`.
    .on_event_cont(EventListener::PointerUp, |_| {
        widgets::pointer_released().update(|n| *n = n.wrapping_add(1));
    })
    // Publish the window size (for menu edge-flipping), and re-read the window's
    // maximized state — the caption glyph is a mirror of it, and this is the one
    // event every route to maximizing (our button, a drag to the screen edge,
    // Win+Up, the OS restoring a snapped window) has in common.
    .on_resize(move |r| {
        window_size().set((r.width(), r.height()));
        chrome.sync();
    })
    // Publish window focus (for the connection health poll). These two events
    // don't need keyboard focus, so they reach the root regardless of which
    // widget is active.
    .on_event_cont(EventListener::WindowGotFocus, move |_| {
        window_focused.set(true)
    })
    .on_event_cont(EventListener::WindowLostFocus, move |_| {
        window_focused.set(false)
    })
    // Any pointer-down anywhere closes an open schema context menu (OS-like:
    // a fresh right-click collapses the previous menu). The menu panel itself
    // stops pointer-downs, so this doesn't fire when interacting with it; and a
    // right-click on another row closes the old menu here (on down) while that
    // row's own handler opens the new one (on up) — one gesture.
    .on_event(EventListener::PointerDown, move |_| {
        // The shared list, so a menu added later is closed here without anyone
        // remembering to extend this.
        root_menus.close_except(None);
        // The "clear" half of the app's `:focus-visible`
        // (`widgets::keyboard_nav`): from here on the focus ring stays dark until
        // the next Tab, because on a pointer gesture it marks what the user just
        // pointed at.
        //
        // **The root sees every press nothing else consumed** — which is not the
        // same as every press, and the difference is load-bearing. Floem's
        // dispatch stops at the first descendant that processes a pointer event
        // and runs a view's own listeners only after that walk, so every
        // `on_event_stop(PointerDown, …)` in the app hides the press from here.
        // That is why a press-swallowing menu trigger has to repay the clear
        // itself, through `widgets::menu_trigger_press`, which states the rule in
        // full — and why the panel sites keep a bare `|_| {}` on purpose, so the
        // flag survives a click on a menu panel or a popover. **Every panel this
        // handler can close owes that absorb**, or it is torn down on the press
        // and the row's click lands on nothing; `widgets::menu_panel_gate` is
        // that half of the bargain.
        //
        // `set` is guarded because it never dedups, and an unguarded write on
        // every click in the app would re-run every button's style closure.
        let kbd = widgets::keyboard_nav();
        if kbd.get_untracked() {
            kbd.set(false);
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
                    .font_size(theme::font_body())
                    .cursor(CursorStyle::Text)
                    .cursor_color(floem::peniko::Brush::Solid(theme::accent()))
                    .border(1.0)
                    .border_color(theme::field_border())
                    .border_radius(6.0)
                    .padding_horiz(theme::scaled(6.0))
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
    });

    // **The resize zones are spread here as eight siblings, not handed over as
    // one view.** They have to be hit before the app, and siblings are hit in
    // reverse tuple order, so they go after `root` — corners after edges, so a
    // corner wins the overlap. What they must *not* do is share a full-window
    // parent: Floem ends its pointer walk at the first child the point lands in,
    // so such a parent eats every press in the app rather than passing the misses
    // through (`window_chrome::WindowChrome::resize_zones` states it in full —
    // it cost a build where nothing was clickable). Eight small siblings are each
    // skipped on a miss, and the walk reaches `root`.
    //
    // An outer stack is the honest shape anyway — the frame is not one more
    // overlay in the app, it is the window around all of them.
    //
    // **The title-bar band is not out here, and that is deliberate.** It solves
    // the same problem the zones do — a modal's backdrop covered the header, so
    // the window could not be dragged, minimized, maximized or closed until the
    // modal was dismissed — but it only ever has to be above *the header and the
    // modal layer*, not above the app. Mounted here it was also above the menu
    // overlays, and a menu tall enough to pin at the top of the window then had
    // its first rows answering with a window drag. It sits inside `root` instead,
    // between the modal layer and the overlays that open over one; the zones stay
    // out here, so a press in the top corners still resizes rather than drags,
    // which is the order the frame already reads in.
    let [n, s, w, e, nw, ne, sw, se] = chrome.resize_zones();
    stack((root, n, s, w, e, nw, ne, sw, se))
        // **The window's keyboard fallback, and it has to be on _this_ view.**
        //
        // Floem hands a `KeyDown` to the focused view; if nothing consumes it, the
        // only other place it goes is `main_view.apply_event(…)` — and
        // `ViewId::apply_event` reads the listeners on **that one id**, without
        // walking children. `main_view` is precisely what the app's view function
        // returned, which is this stack. The focus path is no help either: it
        // dispatches *downward* from the focused view and never up through its
        // ancestors, so an ancestor's listener is not a bubble target.
        //
        // It lived on `root` until this was found, and `root` is a child of this
        // stack. `69fd7aa` put it there — the resize zones needed an outer wrapper
        // and the handler stayed behind — so every branch below went dead whenever
        // focus was outside the SQL editor, which answers the same keys in its own
        // handler and hid it. `window_key_gate` is what keeps it here.
        //
        // What this answers, in order: Escape closing an open dropdown popup, the
        // Tab-trap backstop, `NavKeys` (Ctrl+P/Shift+P/T/W/Tab/1-9, Ctrl+O/S), and
        // the three panel toggles Ctrl+Shift+E / Ctrl+Shift+A / Ctrl+`.
        .on_event(EventListener::KeyDown, move |e| {
            if let Event::KeyDown(ke) = e {
                let m = ke.modifiers;
                // Escape closing a control's popup has to be answered here, and
                // only here: the popup takes the keyboard itself, so neither the
                // control that owns it nor the modal around it is the focused
                // view. Nothing open → falls through, and the modal's own Escape
                // handles the layer below (see `widgets::dismiss_open_popup`).
                if matches!(ke.key.logical_key, Key::Named(NamedKey::Escape))
                    && widgets::dismiss_open_popup()
                {
                    return EventPropagation::Stop;
                }
                // The Tab trap's backstop, and the mirror of the Escape branch
                // above: a plain Tab only gets this far when nothing in the overlay
                // consumed it, which means focus is on a dropdown's popup list or
                // on nothing at all (floem clears it silently when a focused view
                // is removed, and a click on an unfocusable row leaves it cleared).
                // Floem's own fallback walks the **whole window tree**, so from
                // either state Tab left the modal for the workspace behind it — the
                // one thing the ring exists to prevent. Step the innermost
                // overlay's ring instead.
                if matches!(ke.key.logical_key, Key::Named(NamedKey::Tab))
                    && !m.control()
                    && let Some((ring_root, ring)) = widgets::innermost_ring_root()
                {
                    ring.step_from(ring_root, m.shift());
                    return EventPropagation::Stop;
                }
                // **Nothing past here fires while a modal is up.**
                //
                // The two branches above are modal-aware by design and stay: the
                // Escape one closes a control's popup *inside* a modal, and the
                // Tab one steps the innermost modal's own focus ring. Everything
                // below acts on the workspace behind the backdrop, and none of it
                // should.
                //
                // A modal's focus root consumes only Tab
                // (`widgets::focus_root_with_ring` returns `Continue` for every
                // other key), so KeyDown reaches this handler whenever focus is on
                // the root or on a button rather than in a text field — floem's
                // editor is what swallowed these keys before, which is why the
                // failure read as intermittent. Two of them are serious:
                //
                // - **Ctrl+W mid-confirm.** `close_tabs_seq` parks its
                //   continuation in the single-slot `Confirm` signal; a second
                //   `set` from `guard_close` overwrote it, so the chain's
                //   `resolve` was dropped and the remaining tabs were neither
                //   closed nor reported. `overlays.rs`'s own comment says this
                //   must not happen — the focus root simply could not deliver it.
                // - **Ctrl+P.** Find Anywhere is the modal layer's *bottom* entry,
                //   right for a palette raised before a modal and wrong for one
                //   raised while one is up: it mounted invisibly behind the modal
                //   and its autofocusing field took the keyboard, so typing went
                //   into a search box nobody could see and Enter opened a row of
                //   an invisible list.
                //
                // At `v0.19.0` this handler sat on an inner stack floem never
                // reached, so none of these branches ran at all; `0bbd6ef`
                // restored the reachability without restoring a modal guard.
                if modal_up() {
                    return EventPropagation::Continue;
                }
                if m.control() {
                    // Global nav (Ctrl+P/T/W/Tab/1-9) — also wired inside the
                    // editor, which stops KeyDown; here it catches every other
                    // focus (grid, schema, nothing).
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
        .style(|s| s.size_full())
}

// ── Header ────────────────────────────────────────────────────────────────
/// The app mark, drawn at the head of the header.
///
/// Embedded rather than read from disk: 3.7 KB in the binary costs less than a
/// load path that can fail, and the header is built before anything that could
/// report the failure exists.
const LOGO_PNG: &[u8] = include_bytes!("../../../assets/icon-64.png");

/// The header pill both connection triggers wear — the switcher and, on a first
/// run with nothing saved, the New connection button that stands in its place.
///
/// Shared rather than spelled twice because the two swap in and out of the same
/// slot: a difference in margin or radius would show up as the header twitching
/// the moment the first connection is saved. The caller sets the border colour
/// (the active connection's identity colour, or the accent) and may re-state the
/// horizontal padding — the button does, because its content is the switcher's
/// mirrored (glyph leading, label trailing) and those two numbers were tuned for
/// the switcher's order.
fn switcher_chrome(s: floem::style::Style) -> floem::style::Style {
    s.padding_left(theme::scaled(11.0))
        .padding_right(theme::scaled(7.0))
        .padding_vert(theme::scaled(3.0))
        .margin_top(theme::scaled(7.0))
        .margin_bottom(theme::scaled(7.0))
        .items_center()
        // Opaque fill (same color as the header) so the 1px border has a solid
        // backing and renders crisply — an outline over a transparent interior
        // anti-aliases on both edges and looks blurry.
        .background(theme::bg_chrome())
        .border(1.0)
        .border_radius(5.0)
        .hover(|s| s.background(theme::bg_panel()))
}

fn header(ui: Ui, chrome: window_chrome::WindowChrome) -> impl IntoView {
    let connections = ui.conn.connections;
    let active_conn = ui.conn.active_conn;
    let menus = widgets::MenuFlags::of(&ui);
    let conn_menu_open = ui.conn.conn_menu_open;
    let conn_status = ui.conn.conn_status;
    let find_open = ui.overlay.find_open;
    let theme_settings_open = ui.layout.theme_settings_open;
    let help_open = ui.layout.help_open;

    // Connection switcher: shows the active connection's name; click toggles the
    // dropdown (rendered as an overlay so it floats above the app).
    //
    // **Elided at the same limit the menu rows use**, since it is the same name
    // and nothing else in the header is free to move for it: the button has no
    // width of its own, so a long name stretched it and pushed the environment
    // badge and everything after it along the bar.
    let conn_label = move || {
        connections.with(|cs| {
            cs.iter()
                .find(|c| c.id == active_conn.get())
                .map(|c| schemaic_core::connection::elide_name(&c.name, consts::CONN_NAME_CHARS))
                .unwrap_or_else(|| "No connection".to_string())
        })
    };
    // **The full name, on hover.** Eliding at 15 characters removed the one
    // surface in the chrome that showed a connection's whole name, so two
    // connections sharing a 15-character prefix became indistinguishable in both
    // places a connection is chosen. Display-only — nothing truncated ever
    // reaches a connect — but "which of these two is prod" is exactly the
    // question the header is there to answer. Empty when nothing was cut, so an
    // ordinary name raises no tooltip at all.
    let conn_tip = move || {
        connections.with(|cs| {
            cs.iter()
                .find(|c| c.id == active_conn.get())
                .map(|c| c.name.clone())
                .filter(|n| schemaic_core::connection::elide_name(n, consts::CONN_NAME_CHARS) != *n)
                .unwrap_or_default()
        })
    };
    let switcher = move || {
        container(
            h_stack((
                label(conn_label)
                    .style(|s| s.color(theme::text()))
                    .tooltip(move || text(conn_tip()).style(crate::widgets::tooltip_style)),
                icons::icon(icons::CHEVRON_DOWN, 16.0)
                    .style(move |s| s.color(active_conn_color(connections, active_conn))),
            ))
            .style(|s| s.flex_row().items_center().gap(theme::scaled(6.0))),
        )
        .on_click_stop(move |_| {
            // Mutual exclusivity is the trigger's own job once it absorbs the
            // press: the root's `close_except(None)` no longer runs for it, so
            // opening this one has to close the others itself — the shape the
            // schema eye, the gear and the activity clock already have.
            menus.close_except(Some(widgets::MenuId::Connection));
            conn_menu_open.update(|o| *o = !*o);
        })
        // **A menu trigger absorbs its own pointer-down**, which is the premise
        // `MenuFlags::close_except(None)` at the workspace root rests on:
        // `on_click_stop` registers a `Click` handler and nothing else, so the
        // root's dismissal ran first, closed this menu, and the `Click` above
        // reopened it — the switcher could not be shut from the control that
        // opened it. It was one of two triggers on the shared list without this
        // decorator, and the only two.
        .on_event_stop(
            EventListener::PointerDown,
            crate::widgets::menu_trigger_press,
        )
        .style(move |s| {
            switcher_chrome(s).border_color(active_conn_color(connections, active_conn))
        })
        .into_any()
    };

    // **Nothing saved yet — so there is nothing to switch between.** The switcher
    // then reads "No connection" and its menu offers one route to the only thing
    // a first run can do, three clicks down: open the menu, Manage Connections,
    // New connection. This is that action, at the top of the funnel, in the
    // switcher's exact place and chrome so the header doesn't move when the first
    // connection lands and the switcher takes the slot back. Accent-coloured
    // throughout, because unlike the switcher it *is* the thing to press.
    //
    // The condition is "no connections saved", not "none active": a user who has
    // connections but hasn't picked one still wants the switcher — that is what
    // choosing one is done with.
    let new_conn = ui.conn_actions.new_conn.clone();
    let manage_open = ui.conn.manage_open;
    let conn_trigger = dyn_container(
        move || connections.with(|cs| cs.is_empty()),
        move |none_saved| {
            if !none_saved {
                return switcher();
            }
            let new_conn = new_conn.clone();
            container(
                h_stack((icons::icon(icons::PLUS, 16.0), text("New connection")))
                    // One colour on the row, inherited by the label and by the
                    // `currentColor` glyph — the update chip beside it is built the
                    // same way, and it is what makes the pair read as one object.
                    .style(|s| {
                        s.flex_row()
                            .items_center()
                            .gap(theme::scaled(6.0))
                            .color(theme::accent())
                    }),
            )
            .on_click_stop(move |_| {
                // Draft first, then open: the modal reads the draft as it builds,
                // so opening first shows the previous one for a frame.
                (new_conn)();
                manage_open.set(true);
            })
            // Padding tightened 2px on the left and loosened 3px on the right
            // against `switcher_chrome`'s, which is the one place the two
            // occupants deliberately differ: those numbers were set for a text
            // label leading and a chevron trailing, and this row is the mirror
            // — the plus carries its own sidebearing into the left inset, and a
            // label ending flush needs more room after it than a glyph does.
            // Optical, not geometric: the pill's *edges* still land where the
            // switcher's do, which is what keeps the header still.
            .style(|s| {
                switcher_chrome(s)
                    .padding_left(theme::scaled(9.0))
                    .padding_right(theme::scaled(10.0))
                    .border_color(theme::accent())
            })
            .into_any()
        },
    );

    // Auto-update offer — the one member of this cluster that isn't always there.
    // `UpdateState::label()` is `None` while idle, while a check is in flight and
    // when one fails, and this renders a zero-footprint `empty()` for all of them,
    // so for most of most sessions the header looks exactly as it did before the
    // feature existed. It only ever appears with something to say.
    //
    // Shaped as the connection switcher on the other side of the header — 1px
    // border, 5px radius, opaque header-coloured fill so the border renders crisp
    // — at the database selector's font size, and tinted like the glyphs it sits
    // beside (`text_muted`, brightening to `text` on hover). The colour is set on
    // the container so the label *and* the `currentColor` icon inherit it, and the
    // border brightens with them so the chip reads as one object.
    let update_state = ui.update_state;
    let apply_update = ui.apply_update.clone();
    let update_chip = dyn_container(
        move || update_state.get(),
        move |st| {
            let Some(caption) = st.label() else {
                return empty().into_any();
            };
            let chip = container(
                h_stack((
                    // Sized against the label rather than against the other
                    // header glyphs: four strokes in a circle read heavier than
                    // the single-stroke chevron next door, so matching their 16px
                    // would leave the icon shouting over an 11px caption.
                    icons::icon(icons::REFRESH_CW, 13.0),
                    // **Upper-cased, and that is what squares the chip up.** In
                    // mixed case the lone capital R sat against a run of x-height
                    // letters, so the glyph block was taller on its left than its
                    // right and no symmetric padding could centre it — the text
                    // read as sitting high however the numbers were tuned. All
                    // caps is one uniform band, which centres against equal
                    // padding, and 11px keeps that band from dominating a chip
                    // whose neighbours are bare glyphs.
                    //
                    // The design also called for 0.06em tracking, which Floem 0.2
                    // cannot do: neither its `Style` nor the cosmic-text `Attrs`
                    // beneath it exposes letter spacing, and faking it by padding
                    // the string would wreck the metrics this is trying to fix.
                    text(caption.to_uppercase()).style(|s| s.font_size(theme::scaled_font(11.0))),
                ))
                .style(|s| s.flex_row().items_center().gap(theme::scaled(6.0))),
            )
            .style(|s| {
                s.flex_shrink(0.0_f32)
                    .padding_horiz(theme::scaled(9.0))
                    .padding_vert(theme::scaled(3.0))
                    // 10px more than the 16px the glyphs keep between themselves,
                    // so the chip reads as its own thing rather than as a fourth
                    // member of the search/help/settings run.
                    .margin_right(theme::scaled(26.0))
                    .items_center()
                    .background(theme::bg_chrome())
                    .border(1.0)
                    .border_color(theme::text_muted())
                    .border_radius(5.0)
                    .color(theme::text_muted())
                    .hover(|s| s.color(theme::text()).border_color(theme::text()))
            });
            // Clickable only once an update is staged: "Updating… 40%" wears the
            // same chip but is a progress readout, and a click on it mid-download
            // would have nothing to apply.
            if st.is_actionable() {
                let apply = apply_update.clone();
                chip.on_click_stop(move |_| apply()).into_any()
            } else {
                chip.into_any()
            }
        },
    );

    // Find-anywhere trigger: a plain Lucide search glyph, 24px, 20px from the
    // header's right edge (brightens on hover like the schema-panel icons).
    let search = icons::icon(icons::SEARCH, 20.0)
        .on_click_stop(move |_| find_open.set(true))
        .style(|s| {
            s.flex_shrink(0.0_f32)
                .margin_right(theme::scaled(16.0))
                .color(theme::text_muted())
                .hover(|s| s.color(theme::text()))
        });

    // Keyboard-shortcuts help, 20px, just left of the settings gear — same look
    // and spacing as the other header glyphs.
    let help = icons::icon(icons::CIRCLE_QUESTION, 20.0)
        .on_click_stop(move |_| help_open.set(true))
        .style(|s| {
            s.flex_shrink(0.0_f32)
                .margin_right(theme::scaled(16.0))
                .color(theme::text_muted())
                .hover(|s| s.color(theme::text()))
        });

    // App settings (theme picker), 20px, sitting just right of the search glyph.
    let settings = icons::icon(icons::SETTINGS, 20.0)
        .on_click_stop(move |_| theme_settings_open.set(true))
        .style(|s| {
            s.flex_shrink(0.0_f32)
                .margin_right(theme::scaled(20.0))
                .color(theme::text_muted())
                .hover(|s| s.color(theme::text()))
        });
    // The glyph cluster, then the window's caption buttons hard against the right
    // edge — the header *is* the title bar now. `settings` keeps its 20px right
    // margin, which becomes the gap between the app's glyphs and the OS-ish
    // controls; the buttons themselves take no outer margin, because a caption
    // button that stops short of the corner misses the pointer thrown at it.
    let right = h_stack((update_chip, search, help, settings, chrome.controls()))
        .style(|s| s.items_center().height_full());

    // Environment badge: a capsule filled with the active connection's identity
    // colour, sitting 12px right of the switcher and shown only when that
    // connection has an environment set. That 12px is the same figure as the
    // header's left inset and the logo-to-switcher gap, so the whole left
    // cluster steps at one rhythm; it was 20 while the switcher led the row and
    // had only the window edge to balance against. Rebuilds when the environment changes; the
    // fill re-reads the colour inside `.style` so a colour switch follows without a
    // rebuild. The `margin_left` lives on the capsule (not the wrapper) so the
    // empty/no-environment case leaves no gap after the switcher.
    let badge = dyn_container(
        move || active_conn_env(connections, active_conn),
        move |env| match env.badge_label() {
            Some(lbl) => container(text(lbl).style(|s| {
                s.color(theme::env_badge_text())
                    .font_size(theme::font_body())
            }))
            .style(move |s| {
                s.margin_left(theme::scaled(12.0))
                    .padding_vert(theme::scaled(5.0))
                    .padding_horiz(theme::scaled(10.0))
                    .border_radius(5.0)
                    .background(active_conn_color(connections, active_conn))
            })
            .into_any(),
            None => empty().into_any(),
        },
    );

    // **An `img()`, not an `icons::icon()`.** Floem's `svg()` always hands
    // `draw_svg` a tint brush — `svg_color()`, else `text_color()`, else black,
    // never `None` — so every SVG it draws comes out in a single colour. That is
    // exactly what `icons` wants from a Lucide glyph and exactly wrong for a
    // four-colour mark, which would arrive as a silhouette. `img()` decodes the
    // PNG once, at construction, and draws it untinted.
    //
    // The source is 64px for a 20px box, so it stays crisp past 3x display
    // scaling, and its corners are fully transparent — it composites straight
    // onto `bg_chrome()` in either theme with no plate behind it.
    let logo = img(|| LOGO_PNG.to_vec()).style(|s| {
        s.width(theme::scaled(20.0))
            .height(theme::scaled(20.0))
            .margin_left(theme::scaled(12.0))
            .flex_shrink(0.0_f32)
    });

    // Left cluster (logo + switcher + environment badge) and the right glyph
    // cluster, pinned to opposite edges via `justify_between` (a lone flex-grow
    // spacer under-fills — see the schema title-row note).
    //
    // **The 12px left inset belongs to the logo now, and that is a trade rather
    // than a drift.** It used to sit on the switcher, matching `section_title`'s
    // inset so the switcher lined up with "SCHEMA" in the panel below; nothing
    // can hold that alignment once something else stands to its left. The logo
    // inherits the inset, the switcher follows a 12px gap after it, and the
    // column below now lines up with the mark instead.
    let left = h_stack((
        logo,
        container(conn_trigger).style(|s| s.margin_left(theme::scaled(12.0))),
        badge,
        disconnected_notice(conn_status, ui.conn_actions.recheck_conn.clone()),
    ))
    .style(move |s| {
        // Leading space for window controls the *OS* draws over our content —
        // macOS's traffic lights, which survive a hidden title bar and would
        // otherwise land on top of the connection switcher. Zero elsewhere,
        // where the controls are ours and live in `right`.
        s.flex_row()
            .items_center()
            .padding_left(chrome.leading_inset())
    });
    // The gap between the clusters is the drag region — pressing it moves the
    // window, double-clicking it maximizes. It has to be a view of its own: a
    // drag handler on the header would also fire on the switcher and the glyphs,
    // because `on_click_stop` stops `Click` and never sees `PointerDown`.
    // `justify_between` stays as the belt to the strip's braces — it pins both
    // clusters whatever the strip claims.
    h_stack((left, chrome.drag_strip(), right)).style(|s| {
        s.width_full()
            .height(theme::header_h())
            .min_height(theme::header_h())
            .flex_shrink(0.0_f32)
            .flex_row()
            .items_center()
            .justify_between()
            .background(theme::bg_chrome())
            // Named, because `window_chrome::over_backdrop` has to dim the run
            // of this rule that passes under the caption buttons separately —
            // the band it lays over the bar stops short of them.
            .border_bottom(theme::HEADER_BORDER)
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
        s.font_size(toolbar_font())
            .color(theme::error())
            .margin_left(theme::scaled(15.0))
    });
    // Same chrome as the ER-diagram toolbar buttons (`control_surface`), so the
    // app has one button vocabulary rather than a bespoke one per surface.
    let retry = text("Retry")
        .on_click_stop(move |_| (recheck)())
        .style(|s| {
            control_surface(s)
                .font_size(toolbar_font())
                .color(theme::text())
                .margin_left(theme::scaled(15.0))
                .padding_horiz(theme::scaled(10.0))
                .padding_vert(theme::scaled(5.0))
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

/// A small identity dot (6px — matching the connection status dot) for whatever
/// `hex` resolves to, or a zero-footprint `empty()` when it resolves to `None`, so
/// an uncoloured row renders exactly as it did before the colour existed.
///
/// `hex` is read reactively, so the dot appears, changes and disappears with the
/// rule behind it; `ml`/`mr`/`mt` are the dot's margins (left / right / top),
/// applied only when a dot is drawn — `mt` fine-tunes its vertical alignment
/// against the neighbouring text. The colour is a fixed identity hex (not
/// themable), so capturing it by value in the style closure is correct.
///
/// The database and table stores each get a wrapper below; this is the shared
/// half, so both dots stay the same dot.
pub(crate) fn color_dot(
    hex: impl Fn() -> Option<String> + 'static,
    ml: f64,
    mr: f64,
    mt: f64,
) -> impl IntoView {
    dyn_container(hex, move |hex| {
        match hex.as_deref().and_then(theme::parse_hex) {
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
        }
    })
}

/// [`color_dot`] for a database: `key` yields the `(conn_id, database)` to look up.
pub(crate) fn db_color_dot(
    db_colors: RwSignal<Vec<DbColorRule>>,
    key: impl Fn() -> Option<(u64, String)> + 'static,
    ml: f64,
    mr: f64,
    mt: f64,
) -> impl IntoView {
    color_dot(
        move || {
            key().and_then(|(cid, db)| {
                db_colors.with(|rules| schemaic_core::db_color::lookup(rules, cid, &db))
            })
        },
        ml,
        mr,
        mt,
    )
}

/// [`color_dot`] for a table: `key` yields the `(conn_id, database, display name)`
/// to look up. The third part is [`schemaic_core::schema::TableSource::display`],
/// which is what [`TableColorRule`] is keyed by.
pub(crate) fn table_color_dot(
    table_colors: RwSignal<Vec<TableColorRule>>,
    key: impl Fn() -> Option<(u64, String, String)> + 'static,
    ml: f64,
    mr: f64,
    mt: f64,
) -> impl IntoView {
    color_dot(
        move || {
            key().and_then(|(cid, db, table)| {
                table_colors
                    .with(|rules| schemaic_core::db_color::table_lookup(rules, cid, &db, &table))
            })
        },
        ml,
        mr,
        mt,
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
    schema_panel_fits(window_size().get().0)
}

/// Whether the right (AI/terminal/history) panel currently fits beside the schema
/// panel and the center — window width ≥ all three min widths. Reactive on
/// `window_size`.
pub(crate) fn right_panel_allowed() -> bool {
    right_panel_fits(window_size().get().0)
}

/// The one sentence a panel toggle says when the window has taken its panel away.
///
/// Worded as the way *out*, not as a diagnosis: the state is transient and the
/// user fixes it by dragging an edge, so the tip names that rather than the
/// breakpoint arithmetic behind it.
pub(crate) const PANEL_NARROW_TIP: &str = "The window is too narrow for this panel — widen it to \
                                           bring the panel back.";

/// How a footer panel toggle should look and behave right now.
///
/// **Both halves from one call, because they are the same fact.** A toggle is
/// inert exactly when its panel cannot be shown, and the tooltip exists to say
/// why — so asking two predicates is asking one question twice, and the two ways
/// they drift are both bad: a dimmed toggle that explains nothing, and a live
/// toggle claiming the window is too narrow. Computed together, neither is
/// reachable.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct PanelToggle {
    /// Does the click do anything — and is the glyph drawn at full strength?
    pub enabled: bool,
    /// Why not, when there is something worth saying. `None` for every enabled
    /// state *and* for the permanently-unavailable one, which is a separate
    /// decision — see below.
    pub tip: Option<&'static str>,
}

/// `fits` — is the window wide enough for this panel's column?
/// `offered` — does the panel have anything behind it at all (Server Activity on
/// an engine with no sessions is the only `false` today).
///
/// **Only the narrow case gets a tip, and that asymmetry is deliberate.** A
/// window too narrow is transient and the user is one drag from fixing it, so the
/// sentence is worth showing. An engine with no server sessions is permanent, and
/// `toggle_icon`'s own doc already took that call: *"a toggle that opens an
/// explanation is a worse answer than one that visibly isn't offered."* This
/// function does not reopen it — it only makes sure the two disabled states can't
/// be confused, since a narrow window silences nothing.
///
/// Narrow wins when both apply: it is the half the user can act on.
pub(crate) fn panel_toggle(fits: bool, offered: bool) -> PanelToggle {
    if !fits {
        return PanelToggle {
            enabled: false,
            tip: Some(PANEL_NARROW_TIP),
        };
    }
    PanelToggle {
        enabled: offered,
        tip: None,
    }
}

pub use widgets::may_launch_destructive;

/// [`right_panel_allowed`] for the app crate.
///
/// **Tracked**, deliberately, for the caller it was added for: the Server
/// Activity poll's gate, which has to re-arm when the window crosses the
/// breakpoint in either direction. Every other consumer of "is the right panel
/// actually showing" already pairs the panel signal with this one — the four
/// footer toggles and the resize handle — and the poll was the one place reading
/// the signal alone, so it kept a connect + authenticate + `PROCESSLIST` going
/// every couple of seconds for a 0px panel whose toggle was inert.
///
/// The app asks it through one `activity_polling` closure with two callers — that
/// effect, and `reset_activity`, which runs **outside** any effect where the
/// tracking is simply inert. One closure rather than two spellings because this
/// gate has already grown once, and a second copy is how the next conjunct
/// reaches one caller and not the other.
pub fn right_panel_visible() -> bool {
    right_panel_allowed()
}

/// Reveal the AI panel before sending it a message — every "Ask AI" / "AI
/// Explain" / "AI Summary" entry point goes through here.
///
/// Two things it gets right that a bare `set(Ai)` does not. **`RwSignal::set`
/// never dedups**, so setting `Ai` while the AI panel is *already* open notifies
/// anyway, and the `dyn_container` keyed on it disposes the live panel and
/// rebuilds it — mid-turn, taking the running turn's `elapsed_ms` with it, which
/// panicked the footer that was still updating from it. And on a window too
/// narrow for the right column the panel is locked away, so revealing it means
/// changing a signal that nothing will show.
///
/// The schema tree's **AI Explain** didn't reveal at all: with the right column
/// on Terminal, History or closed, it sent the prompt into a panel the user
/// couldn't see and looked like it had done nothing.
pub(crate) fn reveal_ai_panel(right_panel: RwSignal<RightPanel>) {
    reveal_panel(right_panel, RightPanel::Ai);
}

/// Show `which` in the right column — the one door, for every panel.
///
/// Two guards, and both have bitten. A redundant `set` still notifies (floem
/// never dedups), which disposes the open panel's child scope and rebuilds it
/// mid-turn — that is how a `set(Ai)` while the AI panel was streaming freed the
/// `elapsed_ms` its footer was reading. And a window too narrow to show the
/// column at all would otherwise have a signal set that nothing renders.
pub(crate) fn reveal_panel(right_panel: RwSignal<RightPanel>, which: RightPanel) {
    if right_panel_allowed() && right_panel.get_untracked() != which {
        right_panel.set(which);
    }
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
    // `center_min_w()` (the right panel yields against the schema *minimum*; the
    // schema then yields against the right panel's *effective* width). The stored
    // `schema_w`/`right_w` are the user's intent and never mutated here, so a panel
    // restores to its full width when the window grows back.
    let eff_right_w = move || {
        effective_right_w(
            window_size().get().0,
            right_w.get(),
            right_panel.get() != RightPanel::None,
        )
    };
    let eff_schema_w = move || {
        effective_schema_w(
            window_size().get().0,
            schema_w.get(),
            eff_right_w(),
            schema_visible.get(),
        )
    };
    // Publish the width the panel is *rendered* at, for everything inside it that
    // sizes to the panel (the tree rows' `min_width`, the search box). The panel
    // used to publish the *intent*, so under the clamp it laid its content out
    // wider than the wrapper it is clipped by: the search box's clear button was
    // cut off and the tree kept a horizontal scrollbar it didn't need.
    //
    // **Guarded, like `reveal_panel` above and for the same reason**: floem
    // never dedups a `set`, so an unchanged width still invalidates the style
    // closure of every rendered tree row (`schema_tree::tree_row_min_w` reads
    // this signal). A drag is 60–120 `PointerMove`s a second and each one
    // re-runs this effect — so a drag held past `schema_min_w()`, where the
    // clamp returns the same number every frame, restyled the whole tree for no
    // visual change. `schema_tree`'s note on not memoising the per-row closure
    // rests on "a restyle is a theme switch, a scale change or a panel resize,
    // not a frame"; a resize *is* frames, and this is what makes the premise
    // true again rather than paying for a memo.
    create_effect(move |_| {
        let w = eff_schema_w();
        if schema_panel_w().get_untracked() != w {
            schema_panel_w().set(w);
        }
    });
    // The same for the right column — the AI / terminal / history panels size
    // themselves to it, and the clamp is what they must follow (see
    // `widgets::right_panel_w`). A closed panel publishes nothing: `eff` is 0
    // then, and a panel that reopened at 0 would have to wait for a resize.
    create_effect(move |_| {
        let w = eff_right_w();
        if w > 0.0 && widgets::right_panel_w().get_untracked() != w {
            widgets::right_panel_w().set(w);
        }
    });

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
            RightPanel::Snippets => {
                crate::snippet_panel::snippet_panel(ui_right.clone()).into_any()
            }
            RightPanel::Activity => activity_panel(ui_right.clone()).into_any(),
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
        schema_min_w,
        // Leave the center + the right panel's effective width.
        move || window_size().get().0 - center_min_w() - eff_right_w(),
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
        right_min_w,
        // Leave the center + the schema panel at its minimum (it yields as needed).
        move || window_size().get().0 - center_min_w() - schema_min_w(),
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
    // How much of the active tab's connection the assistant may see. Same
    // derivation shape as `dialect`, and read from the **tab's** connection for
    // `grid::ai_data_of`'s reason: a tab keeps the connection it was opened on, so
    // reading the active one would let a production error be judged by a local
    // database's setting. `None` (never configured) falls to `AiData::default()`,
    // which is the same conservative default every other consumer takes.
    let ai_data = create_memo(move |_| {
        let id = active.get();
        let cid = tabs.with(|v| v.iter().find(|t| t.id == id).map(|t| t.conn_id.get()));
        cid.and_then(|cid| {
            connections.with(|cs| cs.iter().find(|c| c.id == cid).and_then(|c| c.ai_data))
        })
        .unwrap_or_default()
    });
    let live_validate = ui.layout.live_validate;
    let validate_stmt = ui.tab_actions.validate_stmt.clone();
    let run = ui.tab_actions.run.clone();
    let run_all = ui.tab_actions.run_all.clone();
    let run_anyway = ui.tab_actions.run_anyway.clone();
    let run_guard = ui.overlay.run_guard;
    let cancel = ui.tab_actions.cancel.clone();
    // For the results bar's wait note: the way out when the transaction holding
    // a write up is one of the user's own.
    let rollback_tx = ui.tab_actions.rollback_tx.clone();
    let db_nodes = ui.schema.db_nodes;
    let stats_gen = ui.schema.stats_gen;
    let inline_ai = ui.ai.inline;
    let ai_attachment = ui.ai.attachment;
    let inline_ai_run = ui.ai_actions.inline_run.clone();
    let inline_ai_cancel = ui.ai_actions.inline_cancel.clone();
    let error_modal_open = ui.overlay.error_modal_open;
    let error_modal_text = ui.overlay.error_modal_text;
    let error_modal_fixable = ui.overlay.error_modal_fixable;
    let schema_visible = ui.layout.schema_visible;
    let right_panel = ui.layout.right_panel;
    let ai_send = ui.ai_actions.send.clone();
    let context_menu = ui.overlay.context_menu;
    let popup = ui.overlay.popup_menu;
    let popup_anchor = ui.overlay.popup_anchor;
    let popup_width = ui.overlay.popup_width;
    let editor_h = ui.layout.editor_h;
    let editor_collapsed = ui.layout.editor_collapsed;
    // No lift here. A height persisted under an older, looser floor — or under a
    // smaller interface scale — is floored at render by
    // `consts::effective_editor_h`, the way the panel widths are, so the stored
    // intent survives and comes back when the floor lowers again. See that
    // function for what mutating it here used to cost.
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
    // **A failed statement un-collapses the editor.**
    //
    // A statement's own error lives in the editor's error bar, under the SQL
    // that produced it and beside the Explain and AI fix that act on it — and
    // *Collapse the editor*, a button in the RESULTS pane itself, sets that pane
    // to height 0. So the server's text, **View**, **AI fix** and **Explain**
    // were all unreachable for exactly the run that needed them, with nothing on
    // screen saying why the chip had gone red.
    //
    // Revealing the bar in place is not an option: two recorded Floem hazards
    // say a child overflowing up out of a zero-height parent is painted and
    // never hit-tested, so the buttons would be visible and dead. Restoring the
    // pane is the honest move, and it is what the user would do by hand.
    //
    // The per-tab `results_maximized` is written too, so the state the mirror
    // above restores on a tab switch agrees with what is on screen. Guarded on
    // the transition into `Failed`, so a second read of the same error does not
    // fight a user who deliberately re-collapses it.
    create_effect(move |was_failed: Option<bool>| {
        let id = active.get();
        // `shown()`, whose reads are **tracked** — `shown_result()` samples, so
        // this effect would never re-run when a statement actually failed.
        let failed = tabs
            .with(|v| v.iter().find(|t| t.id == id).map(|t| t.shown()))
            .is_some_and(|s| matches!(s.get(), QueryState::Failed(_)));
        if failed && was_failed != Some(true) && editor_collapsed.get_untracked() {
            editor_collapsed.set(false);
            if let Some(tab) = active_tab() {
                tab.results_maximized.set(false);
            }
        }
        failed
    });
    // Reveal the AI panel + send a message (the grid cell "AI Summary" builds a
    // context-rich prompt itself, so this just reveals + forwards).
    let summarize: Rc<dyn Fn(String)> = {
        let ai = ai_send.clone();
        Rc::new(move |msg: String| {
            reveal_ai_panel(right_panel);
            (ai)(msg);
        })
    };
    // Stage grid rows for the AI panel's next question: reveal the panel and put
    // them in `AiUi::attachment`. Nothing is sent here — see [`AttachFn`].
    let attach: AttachFn = Rc::new(move |a: schemaic_core::transcript::Attachment| {
        reveal_ai_panel(right_panel);
        ai_attachment.set(Some(a));
    });
    // Close any other open menu — grid cells consume the pointer-down, so the root
    // dismissal handler never fires for clicks inside the grid, and the toolbar Copy
    // dropdown calls this before opening so it's mutually exclusive with the schema
    // eye/settings (and other) dropdowns.
    // The same list every menu trigger uses — see `widgets::MenuFlags`.
    let all_menus = widgets::MenuFlags::of(&ui);
    let dismiss_menus: Rc<dyn Fn()> = Rc::new(move || all_menus.close_except(None));
    let commit_edits = ui.tab_actions.commit_edits.clone();
    let export_file = ui.tab_actions.export_file.clone();
    let apply_view = ui.tab_actions.apply_view.clone();
    let follow_fk = ui.tab_actions.open_table_filtered.clone();
    let open_monitor = ui.tab_actions.open_monitor.clone();
    let db_stats = ui.schema_actions.db_stats.clone();
    let view_blob = ui.tab_actions.view_blob.clone();
    // Properties for a query tab's source table — the results toolbar's entry.
    // Built here because `GridCtx` takes callbacks, not the whole `Ui` (like
    // `create_view` below), and because only the loaded schema knows whether the
    // object is a view; an unloaded one is described as a table, which is what
    // the panel would show anyway.
    let open_properties: PropertiesFn = {
        let ui = ui.clone();
        Rc::new(move |conn_id: u64, src: TableSource| {
            let is_view =
                table_designer::loaded_table(&ui, &src.database, src.schema.as_deref(), &src.table)
                    .is_some_and(|t| t.is_view);
            properties::open_for_table(
                &ui,
                conn_id,
                &src.database,
                src.schema.as_deref(),
                &src.table,
                is_view,
            );
        })
    };
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
        open_file: ui.tab_actions.open_sql_file.clone(),
        save_file: ui.tab_actions.save_sql_file.clone(),
        save_file_as: ui.tab_actions.save_sql_file_as.clone(),
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
        move || {
            let id = active.get();
            // The active tab's reload generation is part of the key: the Floem
            // editor owns its document once mounted (edits flow doc → `query`,
            // never back), so a reload from disk only becomes visible by
            // remounting the pane on the new text. `with_untracked` for the
            // lookup — tracking the whole `tabs` vector here would rebuild the
            // editor every time any tab was opened or closed.
            let reloads = tabs
                .with_untracked(|v| v.iter().find(|t| t.id == id).copied())
                .and_then(|t| t.reload_gen.try_get())
                .unwrap_or(0);
            (id, flashing.get() == Some(id), reloads)
        },
        move |(id, is_flashing, _)| {
            if is_flashing {
                return editor_placeholder(editor_h, editor_collapsed).into_any();
            }
            match tabs.with_untracked(|v| v.iter().find(|t| t.id == id).copied()) {
                Some(tab) => query_pane(QueryPaneParams {
                    query: tab.query,
                    cursor_offset: tab.cursor_offset,
                    selection: tab.selection,
                    goto_open: tab.goto_open,
                    jump_offset: tab.jump_offset,
                    format_req: tab.format_req,
                    insert_req: tab.insert_req,
                    syntax: tab.diagnostics,
                    fix_req: tab.fix_req,
                    results: tab.shown(),
                    run: run.clone(),
                    run_all: run_all.clone(),
                    run_guard,
                    snippets: ui.snippets.items,
                    active_conn: ui.conn.active_conn,
                    params: tab.params,
                    run_anyway: run_anyway.clone(),
                    db_nodes,
                    hidden_dbs: ui.schema.hidden_dbs,
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
                    ai_data,
                    active_db_menu_open,
                    active_db_anchor,
                    menus: all_menus,
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
        // reach 0 on collapse. (The divider clamps `editor_h ≥ query_min_h()` when open.)
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
                tab,
                cancel.clone(),
                editor_collapsed,
                toggle_collapse.clone(),
                GridCtx {
                    source: tab.source,
                    highlight_col: tab.highlight_col,
                    base_sql: tab.base_sql,
                    grid_query: tab.grid_query,
                    row_cap_override: tab.row_cap_override,
                    view_err: tab.view_err,
                    view_busy: tab.view_busy,
                    apply_view: apply_view.clone(),
                    db_nodes,
                    stats_gen,
                    connections,
                    active_conn,
                    popup,
                    popup_anchor,
                    popup_width,
                    menus: all_menus,
                    last_mouse: ui.overlay.last_mouse,
                    summarize: summarize.clone(),
                    attach: attach.clone(),
                    follow_fk: follow_fk.clone(),
                    open_monitor: open_monitor.clone(),
                    open_properties: open_properties.clone(),
                    db_stats: db_stats.clone(),
                    view_blob: view_blob.clone(),
                    ai_fill: ai_fill.clone(),
                    ai_seed: ai_seed.clone(),
                    dismiss: dismiss_menus.clone(),
                    commit: commit_edits.clone(),
                    export_file: export_file.clone(),
                    // Two of the panel-scoped fields are filled in by the results
                    // body, which is the only place that knows *which* panel is
                    // being drawn — see `results_multi`.
                    sync_canonical: None,
                    panel: None,
                    // A placeholder: the body replaces it with a memo over the
                    // panel it is actually drawing (`Tab::panel_frozen_memo`).
                    // It cannot be answered here, because "frozen" is a fact
                    // about *one* panel and this context outlives every one of
                    // them.
                    panel_frozen: create_memo(|_| false),
                    read_only,
                    tx_mode: tab.tx_mode,
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
                    // Go-to-row state (Ctrl+G), alongside find and for the same
                    // reason: the popup renders at the panel level.
                    goto_open: RwSignal::new(false),
                    goto_query: RwSignal::new(String::new()),
                    goto_step: RwSignal::new(0u64),
                    // Selection aggregates, written by the mounted grid.
                    sel_summary: RwSignal::new(None),
                    // Commit-status bar (bottom) — its own per-tab-render signals;
                    // "View" opens the shared workspace error modal with its text.
                    commit_err: RwSignal::new(None),
                    commit_note: RwSignal::new(None),
                    commit_wait: RwSignal::new(None),
                    // **Window-scoped, unlike the three bar signals above it.**
                    // The bar's export flag used to sit beside them and needed a
                    // tab id to survive a tab switch mid-export; the modal has no
                    // such problem, and an export the user navigates away from
                    // still reports into it rather than onto a disposed signal.
                    export_modal: ui.export,
                    tx_holders: {
                        // Answered when a write has been waiting a while, not at
                        // build time: the user can open (or end) a transaction in
                        // another tab while this one is queued behind it.
                        let tabs = tabs;
                        Rc::new(move || {
                            let conn = tab.conn_id.get_untracked();
                            let snapshot = tabs.with_untracked(|v| {
                                v.iter()
                                    .map(|t| TabTx {
                                        tab_id: t.id,
                                        conn_id: t.conn_id.get_untracked(),
                                        state: t.tx.get_untracked(),
                                    })
                                    .collect::<Vec<_>>()
                            });
                            write_blocking_tabs(&snapshot, conn, tab.id)
                                .into_iter()
                                .filter_map(|id| {
                                    tabs.with_untracked(|v| {
                                        v.iter().find(|t| t.id == id).map(|t| (id, t.title()))
                                    })
                                })
                                .collect()
                        })
                    },
                    rollback_tx: rollback_tx.clone(),
                    error_open: error_modal_open,
                    error_text: error_modal_text,
                    error_fixable: error_modal_fixable,
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
            .min_height(results_min_h())
            .min_width(0.0)
    });

    // Divider between editor and results, offset past the tab bar. Double-click
    // resets to the default editor height; drag-end/reset persists the layout.
    // Ceiling leaves the results grid `results_min_h()` within the editor+results
    // region (window minus header/footer/tab-bar).
    let split_handle = v_resize_handle(
        tab_bar_h,
        editor_h,
        move || effective_editor_h(editor_h.get(), editor_collapsed.get()),
        query_min_h,
        move || {
            let wh = window_size().get().1;
            if wh < 1.0 {
                return f64::INFINITY;
            }
            wh - theme::header_h() - theme::footer_h() - tab_bar_h() - results_min_h()
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
            .min_width(center_min_w())
    })
}

// The RESULTS pane for one tab: the strip of this tab's results, always on
// screen, over the grid of whichever one is shown.
#[allow(clippy::too_many_arguments)]
fn results_section(
    tab: Tab,
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
    let (goto_open, goto_query, goto_step) = (gctx.goto_open, gctx.goto_query, gctx.goto_step);
    let sel_summary = gctx.sel_summary;
    let (commit_err, error_open, error_text) = (gctx.commit_err, gctx.error_open, gctx.error_text);
    let error_fixable = gctx.error_fixable;
    let commit_note = gctx.commit_note;
    let (commit_wait, rollback_tx) = (gctx.commit_wait, gctx.rollback_tx.clone());
    let view_err = gctx.view_err;
    let (result_tabs, active_result) = (tab.result_tabs, tab.active_result);
    // Everything the bottom bar can show, as one value — see `grid::BarSignals`.
    // A *statement's* failure is not among them: it goes to the editor's error
    // bar, which is the one place a run error is reported now that every result
    // is a panel. It used to be reported in both, and before that in one each for
    // a batch and a single run.
    let bars = crate::grid::BarSignals {
        commit_err,
        commit_note,
        view_err,
        commit_wait,
    };
    // **The panel-level bars must not outlive the grid they describe.** All three
    // are mounted here, outside `body`, while their only writer lives inside
    // `grid_view`, which exists only under `Phase::Loaded` — so running a
    // statement that failed left the panel showing "Query failed." with the
    // previous result's total still pinned to its edge, indefinitely, and left
    // the Go-to-row popup floating there looking live while Enter bumped a nonce
    // with nobody listening and no close path but Escape.
    //
    // One effect on the result state closes both, and it is the state itself
    // that is tracked rather than the grid's disposal: a `dyn_container` child's
    // cleanup is not somewhere a sibling's signals can be reached.
    create_effect(move |_| {
        let ai = active_result.get();
        // The result the strip is *showing* is the one whose grid is mounted.
        // "Any result in the tab" was the same bug one level along — switching
        // from a loaded Result 1 to a failed Result 2 left the selection summary
        // and the find bar over a pane with no grid under them.
        if !result_tabs.with(|v| shown_panel_loaded(v, ai)) {
            sel_summary.set(None);
            goto_open.set(false);
            find_open.set(false);
        }
    });
    // **And the bars that describe an operation are about the result it ran on.**
    // `view_err`, `commit_err` and `commit_note` are tab-level and are cleared by
    // the grid actions that supersede them — a new commit, a fresh filter re-run,
    // a click on the filter bar — none of which switching results is. So a bad
    // `WHERE` typed on the live result kept its red bar over a *pinned* snapshot
    // the user then clicked, and a failed commit followed them onto a result that
    // cannot be committed at all. Cleared on a change of shown panel, which is
    // the moment they stop describing what is on screen.
    //
    // `commit_wait` is deliberately **not** cleared: it stands for a write still
    // in flight and carries the one-click Rollback, so it belongs to the
    // connection rather than to whichever result is on top of it.
    create_effect(move |prev: Option<Option<u64>>| {
        let ai = active_result.get();
        let shown = result_tabs.with(|v| shown_panel(v, ai).map(|p| p.id));
        if let Some(before) = prev
            && before != shown
        {
            view_err.set(None);
            commit_err.set(None);
            commit_note.set(None);
        }
        shown
    });
    // Properties + Live Monitor: both act on the tab's source table. Captured
    // before `gctx` moves.
    let open_monitor = gctx.open_monitor.clone();
    let open_properties = gctx.open_properties.clone();
    let (monitor_source, monitor_conn) = (gctx.source, gctx.conn_id);
    let body = results_multi(tab, cancel, gctx).style(|s| {
        s.flex_grow(1.0_f32)
            .width_full()
            .flex_col()
            .min_height(0.0)
            .min_width(0.0)
    });

    // Title row: "RESULTS" left; a Properties button, a Live-Monitor button and
    // the expand/shrink toggle right (same widget + spacing as the Schema/AI
    // title-bar icons — `mr=2` between, `mr=7` on the last for the 12px inter-icon
    // gap and 12px edge inset). The toggle swaps its glyph via `dyn_container` (a
    // transform-transition on a small svg is unreliable — see themes gotchas).
    //
    // Properties leads, because it describes the table as it stands while the
    // monitor watches it change — the same order the schema tree's menu puts them
    // in. Both are gated on the tab having a source table, and both then let their
    // own panel answer what it can't ("no statistics for a view", "no row key for
    // this table") rather than the button being silently dead.
    let has_source = move || monitor_source.get().is_some();
    let properties_btn = {
        let open_properties = open_properties.clone();
        toolbar_icon(icons::TABLE_PROPERTIES, 5.0, 2.0, has_source, move || {
            if let Some(src) = monitor_source.get_untracked() {
                (open_properties)(monitor_conn.get_untracked(), src);
            }
        })
        .tooltip(|| text("Table properties…").style(widgets::tooltip_style))
    };
    let monitor_btn = {
        let open_monitor = open_monitor.clone();
        toolbar_icon(icons::ACTIVITY, 5.0, 2.0, has_source, move || {
            if let Some(src) = monitor_source.get_untracked() {
                (open_monitor)(monitor_conn.get_untracked(), src);
            }
        })
        .tooltip(|| text("Live Monitor…").style(widgets::tooltip_style))
    };
    let toggle_btn = dyn_container(
        move || editor_collapsed.get(),
        move |collapsed| {
            let (markup, tip) = if collapsed {
                (icons::SHRINK, "Restore the editor")
            } else {
                (icons::EXPAND, "Collapse the editor")
            };
            let t = toggle_collapse.clone();
            toolbar_icon(markup, 5.0, 7.0, || true, move || (t)())
                .tooltip(move || text(tip).style(widgets::tooltip_style))
                .into_any()
        },
    );
    let icons_group = h_stack((properties_btn, monitor_btn, toggle_btn))
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
    // Overlay the find bar and the go-to-row popup at the panel's top edge + the
    // commit-error bar at the bottom (a `stack` anchors the absolute bars to the
    // panel). Find and goto share the top-right anchor; `grid_view` keeps at most
    // one of them open, so they never paint over each other.
    stack((
        panel,
        grid_find_bar(
            find_open, find_query, find_step, find_total, find_pos, find_more,
        ),
        grid_goto_bar(goto_open, goto_query, goto_step),
        grid_error_bar(bars, rollback_tx, error_open, error_text, error_fixable),
        // Last, so it paints over the panel — and it lifts itself above the
        // bottom bar when that one is up, through the **same** predicate
        // `grid_error_bar` decides its own visibility with. It used to be a
        // second copy of the same four-way `is_some`, which the note surface
        // would have had to be added to twice; the two disagreeing is the pair
        // of them drawn on top of each other.
        grid_selection_bar(sel_summary, move || bars.any_up()),
    ))
    .style(|s| {
        s.width_full()
            .flex_grow(1.0_f32)
            .flex_col()
            .min_height(0.0)
            .min_width(0.0)
    })
}

/// The result the strip is **showing**: the panel with id `active`, or the first
/// one when that id is stale — a selected panel outlives the run that closes it,
/// and something has to be on screen either way.
///
/// One function so the pane, the error bar, the bars-clearing effect **and the
/// AI panel** can't disagree about which statement they are describing.
pub fn shown_panel(panels: &[ResultPanel], active: u64) -> Option<&ResultPanel> {
    panels
        .iter()
        .find(|p| p.id == active)
        .or_else(|| panels.first())
}

/// Whether a grid is mounted under the strip — i.e. the **shown** statement
/// loaded one. What the panel-level bars (find, go-to-row, the selection summary)
/// are allowed to be up for.
fn shown_panel_loaded(panels: &[ResultPanel], active: u64) -> bool {
    matches!(
        shown_panel(panels, active).map(|p| &p.state),
        Some(QueryState::Loaded(_))
    )
}

// The results strip and the grid under it — **every** result, a single run's
// included, since the strip is always on screen.
fn results_multi(tab: Tab, cancel: Rc<dyn Fn()>, gctx: GridCtx) -> impl IntoView {
    let (result_tabs, active_result) = (tab.result_tabs, tab.active_result);
    let strip_ctx = gctx.clone();
    // **The key is the shown panel's id, its phase, and the load nonce** — a
    // deduped `Memo`, never the `QueryState` itself.
    //
    // The id is what makes switching results a remount: two panels can both be
    // `Loaded` and they are different results. The phase is what keeps an
    // *in-place commit splice* from being one: the splice replaces the panel's
    // `Arc` (`Loaded` → `Loaded`) without touching id or nonce, so the key is
    // unchanged and the grid — with its scroll, its selection and its column
    // widths — is not rebuilt. A real run passes through `Running`, and a
    // filter/sort re-run bumps the nonce; both rebuild, which is what they mean.
    let key = create_memo(move |_| {
        let ai = active_result.get();
        // **The nonce is the shown panel's own**, so a filter re-run landing on
        // a different panel cannot rebuild this one out from under its scroll
        // position and its selection.
        result_tabs.with(|v| shown_panel(v, ai).map(|p| (p.id, phase_of(&p.state), p.load_gen)))
    });
    let body = dyn_container(
        move || key.get(),
        move |shown| {
            let Some((id, phase, _gen)) = shown else {
                return empty().into_any();
            };
            // Read untracked: the memo above, not the `Arc`, drives rebuilds.
            let panel = result_tabs.with_untracked(|v| v.iter().find(|p| p.id == id).cloned());
            let state = panel.as_ref().map(|p| p.state.clone());
            let mut gctx = gctx.clone();
            gctx.panel = panel.as_ref().map(|p| p.view);
            // A memo over **this** panel — created here, in the scope that dies
            // with this grid, and keyed on the id rather than on "whichever is
            // shown". See `Tab::panel_frozen_memo` for why the difference is a
            // crash and not a nuance.
            gctx.panel_frozen = tab.panel_frozen_memo(id);
            if phase == Phase::Loaded {
                // The splice sink, pointed at *this* panel: an in-place commit
                // replaces the result this grid is showing, and only that one.
                gctx.sync_canonical = Some(Rc::new(move |rs| {
                    tab.set_panel_state(id, QueryState::Loaded(rs))
                }));
            }
            build_result_body(state, cancel.clone(), gctx)
        },
    )
    .style(|s| {
        s.flex_grow(1.0_f32)
            .width_full()
            .flex_col()
            .min_height(0.0)
            .min_width(0.0)
    });

    v_stack((result_tab_strip(tab, strip_ctx), body)).style(|s| {
        s.flex_grow(1.0_f32)
            .width_full()
            .flex_col()
            .min_height(0.0)
            .min_width(0.0)
    })
}

/// What the pane shows for one panel's state.
///
/// `None` is a strip with nothing selectable in it, which the tab's idle panel
/// means it can't be — kept as the same empty pane an `Idle` panel draws, since
/// "no result" is what both of them are.
fn build_result_body(
    state: Option<QueryState>,
    cancel: Rc<dyn Fn()>,
    gctx: GridCtx,
) -> floem::AnyView {
    match state {
        None => empty().into_any(),
        // The tab's own opening state, and the only prompt in the app that says
        // how to run something — it is the first thing on screen in a new tab.
        Some(QueryState::Idle) => {
            centered_msg("Run a query  (Ctrl+Enter)", theme::text_muted).into_any()
        }
        Some(QueryState::Running) => running_view(cancel).into_any(),
        // The message itself goes to the panel-level error bar (see
        // `shown_panel_error`), exactly as a single run's goes to the editor's
        // — so the pane only notes the failure, as `grid_view` does under
        // `Phase::Failed`. It used to *be* the pane, and one long server error
        // then ran across the window and out over the schema sidebar.
        Some(QueryState::Failed(_)) => {
            centered_msg("Statement failed.", theme::text_dim).into_any()
        }
        Some(QueryState::Cancelled) => centered_msg("Query cancelled.", theme::text_dim).into_any(),
        Some(QueryState::Loaded(rs)) => loaded_view(rs, gctx),
    }
}

// The results strip: one chip per result, click to switch, right-click for the
// same verbs the query strip offers (Pin, Close, Close others, Close all).
// Borrows the query tab bar's look, and is **always on screen** — a pin is only a
// visible affordance if the thing it acts on is.
fn result_tab_strip(tab: Tab, gctx: GridCtx) -> impl IntoView {
    let result_tabs = tab.result_tabs;
    let chips = dyn_stack(
        move || result_tabs.get(),
        // **Keyed on the id alone.** The label is derived from the statement and
        // never changes for a given panel, and the id is what survives pinning
        // (which reorders) and closing (which renumbers nothing) — keying on the
        // position would rebuild every chip to the right of a close.
        |p: &ResultPanel| p.id,
        move |panel| result_tab_chip(panel, tab, gctx.clone()),
    )
    .style(|s| s.flex_row().height_full());

    // Chips pan horizontally on the plain wheel (no visible bars) so overflowed
    // result tabs stay reachable — same treatment as the query strip.
    let scroller =
        wheel_hscroll(chips).style(|s| s.flex_shrink(1.0_f32).min_width(0.0).height_full());

    // **What the pins are holding**, at the strip's right edge, and only once
    // that is news.
    //
    // Pinning removed the 200k row cap as *the* memory bound and put nothing in
    // its place: a pin survives every later run, ten of them at 200k x 50
    // measured 2.00 GB live, and `retained_bytes` had exactly one consumer — a
    // per-chip tooltip nobody opens ten of. The policy is
    // `resultsel::pin_verdict`, which warns rather than refuses: pinning is a
    // deliberate act with a deliberate cost, and an app that silently declines
    // to keep what it was asked to keep is worse than one that says the price.
    let kept_note = dyn_container(
        move || {
            let (kept, bytes) = result_tabs.with(|v| {
                v.iter()
                    .filter(|p| p.pinned)
                    .fold((0usize, 0u64), |(n, b), p| {
                        let add = match &p.state {
                            QueryState::Loaded(rs) => rs.retained_bytes() as u64,
                            _ => 0,
                        };
                        (n + 1, b + add)
                    })
            });
            // Asked about the strip as it stands: every pin here is one the user
            // already made, so the "addition" is nothing.
            match schemaic_core::resultsel::pin_verdict(bytes, 0) {
                schemaic_core::resultsel::PinVerdict::Fine => None,
                schemaic_core::resultsel::PinVerdict::Heavy(total) => Some((kept, total)),
            }
        },
        |heavy| {
            match heavy {
            None => empty().into_any(),
            Some((kept, total)) => text(format!(
                "{kept} pinned · {}",
                schemaic_core::stats::format_bytes(total)
            ))
            .tooltip(|| {
                text(
                    "Pinned results are held in memory until you close them.                      Close the ones you are done with to give it back.",
                )
                .style(crate::widgets::tooltip_style)
            })
            .style(|s| {
                s.font_size(theme::font_label())
                    .color(theme::text_muted())
                    .padding_horiz(theme::scaled(10.0))
                    .flex_shrink(0.0_f32)
            })
            .into_any(),
        }
        },
    );

    // Flat, full-height result tabs. Unlike the query strip, this one adds a
    // full-width **top** separator too (the query strip sits below the header,
    // which already provides one).
    h_stack((scroller, kept_note)).style(move |s| {
        let s = s
            .width_full()
            .flex_row()
            .height(tab_bar_h())
            .min_height(tab_bar_h())
            .flex_shrink(0.0_f32)
            .background(theme::bg_chrome())
            .border_top(1.0)
            .border_bottom(1.0)
            .border_color(theme::border());
        // **Nothing has run: no bar.** The strip earns its 28px by holding
        // results to choose between, and a tab that has never run one has a
        // single chip saying nothing over a pane that says "Run a query" — two
        // pieces of furniture for one empty state, and the borrowed height comes
        // off the grid for the rest of the session. It appears with the first run
        // — `results_untouched` is false from the moment a panel opens, `Running`
        // included, so it is up before the rows are — and goes again only where
        // the pane is empty for the same reason: every result closed, or the tab
        // respawned. It cannot flicker under a tab that is working.
        //
        // By `hide()` rather than by not mounting it: the codebase's rule for a
        // reactive show-hide, and the one that keeps the chips' state — and the
        // hazards of building views inside a strip that is changing — out of it.
        if tab.results_untouched() { s.hide() } else { s }
    })
}

/// How much width a result chip's label may take: the chip's cap, less
/// everything drawn beside the text — the chip's 10px left padding, the label's
/// own 6px right margin, and the 14px trailing glyph with its 7px margin.
///
/// Spelled out rather than left to flex shrink, because that is not what a
/// `max_width` on the row does: the text lays out at its natural width and
/// overflows the chip. The query strip pays the same arithmetic in
/// `tabs::tab_title_avail`, and for the same reason — a title that eats its own
/// × is a chip that cannot be closed.
fn result_title_avail() -> f64 {
    tab_max_w() - theme::scaled(37.0)
}

fn result_tab_chip(panel: ResultPanel, tab: Tab, gctx: GridCtx) -> impl IntoView {
    let (result_tabs, active_result) = (tab.result_tabs, tab.active_result);
    let id = panel.id;
    // Read reactively rather than off the captured `panel`: the chip is keyed on
    // the id, so it is *not* rebuilt when its statement lands or when it is
    // pinned — both of which it has to show.
    let state_of = move |f: fn(&QueryState) -> bool| {
        move || result_tabs.with(|v| v.iter().find(|p| p.id == id).is_some_and(|p| f(&p.state)))
    };
    let is_err = state_of(|s| matches!(s, QueryState::Failed(_)));
    let pinned =
        move || result_tabs.with(|v| v.iter().find(|p| p.id == id).is_some_and(|p| p.pinned));
    let is_active =
        move || result_tabs.with(|v| shown_panel(v, active_result.get()).map(|p| p.id)) == Some(id);

    // What this result is, in full: the statement, when it ran, and what holding
    // on to it costs. The last is the whole reason the strip can say "keep this"
    // without hand-waving about memory — see `ResultSet::retained_bytes`.
    // Read off the *live* panel, not the captured one: a filter re-run rewrites
    // the statement a panel's rows came from (`Tab::set_panel_sql`), and the
    // result it is holding — the thing the size is about — arrives after the chip
    // is built.
    let tip = move || {
        let Some(panel) = result_tabs.with_untracked(|v| v.iter().find(|p| p.id == id).cloned())
        else {
            return String::new();
        };
        let mut lines = vec![if panel.sql.trim().is_empty() {
            "Nothing has run in this tab yet.".to_string()
        } else {
            panel.sql.clone()
        }];
        let mut foot: Vec<String> = Vec::new();
        if panel.ran_at > 0 {
            foot.push(schemaic_core::history::relative_time(
                panel.ran_at,
                now_secs(),
            ));
        }
        if let QueryState::Loaded(rs) = &panel.state {
            foot.push(schemaic_core::stats::format_bytes(
                rs.retained_bytes() as u64
            ));
        }
        if panel.pinned {
            foot.push("kept — read-only".to_string());
        }
        if !foot.is_empty() {
            lines.push(foot.join(" · "));
        }
        lines.join("\n")
    };

    // Trailing icon, on the query strip's rule and with its footprint: a
    // clickable × normally, a non-clickable pin when pinned (a pinned result
    // can't be closed — unpin it first).
    //
    // **Both glyphs, one hidden — not a `dyn_container` swapping them.** `pinned`
    // reads the panel list, which is also what removes this chip, so a rebuild
    // keyed on it would construct a child inside a view being torn down: the
    // hazard under *Floem 0.2 gotchas*, and the reason `widgets::check_box` and
    // the dump modal's picker both toggle by style. Nothing here reads a
    // per-view signal today, so it did not crash; that is not a property to
    // leave resting on what the next edit happens to add.
    let icon_style = |s: floem::style::Style| {
        s.flex_shrink(0.0_f32)
            .margin_right(theme::scaled(7.0))
            .color(theme::tab_close())
    };
    let pin_glyph = icons::icon(icons::PIN, 14.0).style(move |s| {
        let s = icon_style(s);
        if pinned() { s } else { s.hide() }
    });
    let close_glyph = icons::icon(icons::X, 14.0)
        .on_click_stop(move |_| tab.close_panels(&[id]))
        .style(move |s| {
            let s = icon_style(s).hover(|s| s.color(theme::text()));
            if pinned() { s.hide() } else { s }
        });

    // **The cap is on the label, not on the chip.** A statement is arbitrarily
    // long, and `max_width` on the row only clips it: the text still lays itself
    // out at its natural width, runs under the × and out over the next chip.
    // Capping the *text* is what makes `text_ellipsis` fire — the query strip's
    // rule (`tab_title_avail`), and this is its arithmetic.
    let label = text(panel.label.clone())
        .style(|s| {
            s.margin_right(theme::scaled(6.0))
                .max_width(result_title_avail())
                .font_size(theme::font_body())
                .text_ellipsis()
        })
        // **The tooltip goes on the label, not on the chip** — and this is the
        // whole of a bug that survived two attempts at it. `tooltip()` does not
        // decorate a view, it *wraps* it: `h_stack(…).on_click_stop(…).tooltip(…)
        // .style(…)` puts the background, the border, the height and the padding
        // on the wrapper and leaves the click listeners on the stack inside it.
        // The chip you see is the wrapper; the chip that listens is a
        // content-sized box within it, so clicking anywhere but the text hit the
        // wrapper and nothing happened. The query strip tooltips its label too,
        // which is why editor tabs never had it.
        .tooltip(move || text(tip()).style(crate::widgets::tooltip_style));

    // Colour is set on the tab container and cascades to the label.
    let chip = h_stack((label, pin_glyph, close_glyph))
        .on_click_stop(move |_| active_result.set(id))
        // Middle-click closes, as it does on a query tab (and as there, a pinned
        // one no-ops — `close_panels` is reached through the same guard).
        .on_event(EventListener::PointerDown, move |e| {
            if let Event::PointerDown(pe) = e
                && pe.button.is_auxiliary()
            {
                if schemaic_core::resultsel::can_close(&tab.panel_refs(), id) {
                    tab.close_panels(&[id]);
                }
                return EventPropagation::Stop;
            }
            EventPropagation::Continue
        })
        .on_secondary_click_stop(move |_| result_chip_menu(id, tab, &gctx))
        .style(move |s| {
            // **The chip is the hitbox, so the chip has to be the whole chip** —
            // which means the style and the listeners must land on the *same*
            // view (see the label's tooltip note for what happens when they
            // don't). `height_full` claims the strip's height and `padding_left`
            // puts the label's inset inside this box, where a `margin_left` on
            // the label left it outside.
            let s = s
                .flex_row()
                .items_center()
                .height_full()
                .padding_left(theme::scaled(10.0))
                .max_width(tab_max_w())
                .border_right(1.0)
                .border_color(theme::tab_separator());
            let s = if is_active() {
                s.background(theme::tab_active())
            } else {
                s.background(theme::bg_chrome())
            };
            // The chip's own background is `tab_active`/`bg_chrome`, never
            // `reject_bg`, so a failed statement's label needs the free-standing
            // error colour rather than the red pill's foreground.
            if is_err() {
                s.color(theme::error())
            } else if is_active() {
                s.color(theme::text())
            } else {
                s.color(theme::tab_text()).hover(|s| s.color(theme::text()))
            }
        });

    // **Reveal the shown chip**, exactly as the query strip one row above does
    // (`tabs.rs`, with the same comment). The strip had none, and its case is
    // the stronger of the two: its selection is set *programmatically* on every
    // run and every close, and pins fill it up — so a fresh run's chip landed
    // past the right edge and the strip appeared not to have responded at all.
    //
    // Deferred one tick, and that is load-bearing rather than cautious:
    // `Id::scroll_to` refuses on a hidden subtree, and a freshly-mounted chip
    // has not been laid out yet.
    let cid = chip.id();
    create_effect(move |_| {
        if is_active() {
            floem::action::exec_after(std::time::Duration::ZERO, move |_| cid.scroll_to(None));
        }
    });
    chip
}

/// The chip's right-click menu — the query strip's verbs, less the ones that are
/// about a *query* (rename, duplicate, the file group).
///
/// The same rules answer both strips ([`schemaic_core::resultsel`] and
/// `tabsel`), so "Close all" spares the pins here exactly as it does there, and
/// an entry with nothing to act on is **dimmed rather than missing** — a menu
/// that keeps its shape is the one the hand learns.
fn result_chip_menu(id: u64, tab: Tab, gctx: &GridCtx) {
    use schemaic_core::resultsel;
    let refs = tab.panel_refs();
    let pinned = refs.iter().any(|(i, p)| *i == id && *p);
    let mut entries = vec![MenuEntry::action(
        if pinned { "Unpin" } else { "Pin" },
        move || tab.set_pinned(id, !pinned),
    )];
    // Omitted rather than dimmed when pinned, which is what the query strip does
    // with the same entry: the pin above it is the way to get it back.
    if resultsel::can_close(&refs, id) {
        entries.push(MenuEntry::action("Close", move || tab.close_panels(&[id])));
    }
    entries.push(MenuEntry::Separator);
    let others = resultsel::others_to_close(&refs, id);
    entries.push(
        // Asked again at the click, not captured: the strip can have moved while
        // the menu stood open (a batch landing, another chip closed).
        MenuEntry::action("Close other results", move || {
            tab.close_panels(&resultsel::others_to_close(&tab.panel_refs(), id));
        })
        .disabled(others.is_empty()),
    );
    let all = resultsel::all_to_close(&refs);
    entries.push(
        MenuEntry::action("Close all results", move || {
            tab.close_panels(&resultsel::all_to_close(&tab.panel_refs()));
        })
        .disabled(all.is_empty()),
    );
    (gctx.dismiss)();
    gctx.popup_anchor.set(None); // right-click → open at the cursor
    gctx.popup_width.set(170.0);
    gctx.popup.set(Some(entries));
}

/// Now, in epoch seconds — for a result's age in the strip's tooltip.
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
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
    let db_label = ui.term.db_label;
    let font_size = ui.term.font_size;
    let copy_on_select = ui.term.copy_on_select;
    let cursor_style = ui.term.cursor_style;

    // Custom scrollback scrollbar state (the terminal isn't a Floem scroll): a
    // `shown` flag toggled by scroll activity, hidden 3s after it stops.
    let (bar_shown, bar_poke) = autohide_state();
    let bar_poke_wheel = bar_poke.clone();

    // Title row: "TERMINAL" left; open-DB-CLI + restart + settings gear right,
    // each 10px apart (gear 12px from the edge), matching the AI panel's spacing.
    let db_cli_btn = toolbar_icon(icons::DATABASE, 5.0, 2.0, || true, move || (open_cli)(None))
        .tooltip(|| text("Open the database CLI").style(widgets::tooltip_style));
    let restart_btn = toolbar_icon(icons::REFRESH_CW, 5.0, 2.0, || true, move || (restart)())
        .tooltip(|| text("Restart the terminal").style(widgets::tooltip_style));
    let gear = toolbar_icon(
        icons::SLIDERS_VERTICAL,
        5.0,
        7.0,
        || true,
        move || settings_open.set(true),
    )
    .tooltip(|| text("Terminal settings…").style(widgets::tooltip_style));
    let icons_group = h_stack((db_cli_btn, restart_btn, gear))
        .style(|s| s.flex_row().items_start().flex_shrink(0.0_f32));
    // The engine, but only while this is a DB CLI session — see `TermUi::db_label`.
    // `section_title`'s own 12px right padding is the gap; the matching vertical
    // padding lines the two up without depending on either font's line height.
    // Grouped WITH the title rather than added to the row: `justify_between` over
    // three children would strand it in the middle of the panel.
    //
    // Family + size are stated rather than inherited: this row sits directly above
    // a monospace surface, and the badge is chrome, so it matches the title beside
    // it. One step dimmer than that title (`text_faint` against its `text_muted`)
    // and unbolded — it answers a question the user only asks after switching
    // connections, and shouldn't compete with the panel's name the rest of the time.
    let engine = dyn_container(
        move || db_label.get(),
        move |label| match label {
            Some(l) => text(l)
                .style(|s| {
                    s.font_size(theme::font_title())
                        .font_family("IBM Plex Sans".to_string())
                        .color(theme::text_faint())
                        .padding_vert(theme::scaled(8.0))
                })
                .into_any(),
            None => empty().into_any(),
        },
    )
    .style(|s| s.flex_shrink(0.0_f32));
    let title_group = h_stack((section_title("TERMINAL"), engine))
        .style(|s| s.flex_row().items_start().min_width(0.0));
    let title_row = h_stack((title_group, icons_group))
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

    let surface = shift_hscroll(grid);
    // The surface's own id, so a selection drag can capture the pointer (see the
    // `PointerDown` arm below) — the same idiom as the resize handles.
    let surface_id = surface.id();
    let surface = surface
        .style(|s| {
            s.flex_grow(1.0_f32)
                .width_full()
                .min_height(0.0)
                .min_width(0.0)
                .padding(term_pad())
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
                // Capture the pointer, as both resize-handle drags do. The
                // terminal is a narrow right-hand column and a selection
                // naturally runs off its left edge, so the release lands outside
                // the surface — which never saw it, leaving `dragging` true. The
                // selection then kept extending under a pointer with no button
                // held, and with copy-on-select each stray release pushed the
                // accident to the clipboard.
                surface_id.request_active();
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
                surface_id.clear_active();
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
            let top = term_pad() + ratio * (track_h - thumb_h);
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
    // free. Positions match the grid: the surface pads `term_pad()`, cells are
    // cw×ch. The padding is scaled and this overlay is the surface's *sibling*,
    // so it has to read the same metric rather than restate it.
    let cursor_overlay = empty()
        .style(move |s| {
            let sc = screen.get();
            let (cw, ch) = term_cell_wh(font_size.get());
            let color = term_color(schemaic_term::CURSOR);
            match (cursor_style.get(), sc.cursor) {
                (TermCursor::Bar, Some((r, c))) => s
                    .absolute()
                    .inset_left(term_pad() + c as f64 * cw)
                    .inset_top(term_pad() + r as f64 * ch)
                    .width(2.0)
                    .height(ch)
                    .background(color),
                (TermCursor::Underline, Some((r, c))) => s
                    .absolute()
                    .inset_left(term_pad() + c as f64 * cw)
                    .inset_top(term_pad() + r as f64 * ch + ch - 2.0)
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

    v_stack((title_row, body)).style(move |s| {
        // The width the shell *renders* this column at, not `right_w` (the
        // user's stored intent) — see `widgets::right_panel_w`. The terminal is
        // the fourth panel in this column and the one that got left behind when
        // the AI, history and activity panels were converted: at 200% the shell
        // reserves the scaled minimum and the panel drew the intent, leaving a
        // strip of the layer behind it showing through.
        s.width(widgets::right_panel_w().get())
            .flex_shrink(0.0_f32)
            .height_full()
            .min_height(0.0)
            .flex_col()
            .background(theme::bg_panel())
            .border_left(1.0)
            .border_color(theme::border())
    })
}

/// Box size of [`FieldCfg::trailing`]'s action — the icon's own 16px, so pinning
/// the box doesn't move it horizontally.
fn trailing_size() -> f64 {
    theme::scaled(16.0)
}

/// The gaps either side of [`FieldCfg::trailing`]'s action. The right one is
/// negative on purpose — it pulls the control 4px closer to the box edge, for a
/// 14px gap rather than the padding's 10.
///
/// Named because two things read them: the control's own layout, and
/// [`placeholder_right_inset`], which has to reserve exactly the width they take.
fn trailing_gap_l() -> f64 {
    theme::scaled(6.0)
}
fn trailing_gap_r() -> f64 {
    -theme::scaled(4.0)
}

/// The right inset of [`edit_field`]'s placeholder overlay, measured from the
/// box's padding edge the way its left one is.
///
/// **A placeholder is absolutely positioned, so nothing in the flow bounds it.**
/// With a left inset only, a string longer than the field laid out at its
/// natural width and painted *over* the border and into whatever sat beside it —
/// not clipped, not ellipsized. Found by eye on the view editor's SQLite
/// **Column names** row, and true of every field in the app whose placeholder
/// outgrows its width. Bounding the overlay on the right is what turns that
/// overflow into an ellipsis.
///
/// Symmetrical with the left inset when nothing sits beside the editor. A
/// [`FieldCfg::trailing`] action is **in flow** and shortens the editor, so the
/// placeholder stops short of it by the control's own width plus what its
/// margins add — read from the constants that place it, so moving the control
/// can't leave this behind. The clearable × needs no room: it shows only while
/// the field has text, which is exactly when the placeholder is hidden.
fn placeholder_right_inset(has_trailing: bool) -> f64 {
    if has_trailing {
        chat_pad_h() + trailing_size() + trailing_gap_l() + trailing_gap_r()
    } else {
        chat_pad_h()
    }
}

/// A `fn`-pointer transparent background for [`FieldCfg::background`] (the
/// Ctrl+K field, whose surface is owned by an animated outer container).
pub(crate) fn bg_transparent() -> floem::peniko::Color {
    floem::peniko::Color::TRANSPARENT
}

/// The line height [`edit_field`] gives its editor, from the field's type size —
/// ≈1.46× the app's body rhythm (13→19, 20→29).
///
/// One function because two places must agree to the pixel: the editor's own
/// `Styling::line_height`, and the box-height/padding arithmetic in `edit_field`
/// that has to match what the editor then renders.
fn field_line_h(px: f32) -> f64 {
    (px as f64 * 1.46).round()
}

/// [`edit_field`]'s editor styling.
///
/// A hand-written `Styling` rather than floem's `SimpleStyling` for one reason:
/// the builder takes the font size **by value**, so the size a field was built
/// at is the size it draws for ever. Reading it here — through the same
/// `fn() -> f32` the rest of the field's geometry uses — is what lets a field
/// follow the interface scale, and it is the trick `sql_highlight::SqlStyling`
/// already plays for the SQL editor's own font.
struct FieldStyling {
    size: fn() -> f32,
    family: Vec<floem::text::FamilyOwned>,
}

impl floem::views::editor::text::Styling for FieldStyling {
    /// The editor caches its laid-out lines against this id, so the size has to
    /// be *in* it: reporting a new `font_size` while the id stood still would
    /// leave the old layout on screen. Same shape as `SqlStyling::id`.
    fn id(&self) -> u64 {
        (theme::ui_generation() << 8) | ((self.size)().round() as u64 & 0xFF)
    }

    fn font_size(&self, _edid: floem::views::editor::id::EditorId, _line: usize) -> usize {
        (self.size)().round() as usize
    }

    fn line_height(&self, _edid: floem::views::editor::id::EditorId, _line: usize) -> f32 {
        field_line_h((self.size)()) as f32
    }

    fn font_family(
        &self,
        _edid: floem::views::editor::id::EditorId,
        _line: usize,
    ) -> std::borrow::Cow<'_, [floem::text::FamilyOwned]> {
        std::borrow::Cow::Borrowed(&self.family)
    }
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
    /// The field's type size. A `fn` (not an `f32`) for the reason
    /// [`FieldCfg::background`] is one: the box height, the vertical padding, the
    /// placeholder's position and the editor's own styling are all derived from
    /// it, and a size resolved once at build freezes all four. It defaults to
    /// [`theme::font_body`], so a field follows the interface scale without its
    /// caller doing anything.
    pub font_size: fn() -> f32,
    /// Render in the app's monospace face ([`MONO_FAMILY`]) instead of IBM Plex
    /// Sans — for a field whose content is *code* and wants column alignment
    /// (the DDL preview's generated SQL). Doesn't change the line height, so the
    /// auto-grow box math is unaffected.
    pub mono: bool,
    pub border_radius: f32,
    /// Read-only: no text edits (still handles Enter/Escape). Suppresses autofocus.
    pub read_only: bool,
    /// **Read-only while this reads true**, dimmed to say so — and *without the
    /// field being rebuilt*, which is the whole reason it is a signal rather
    /// than [`FieldCfg::read_only`]'s second spelling.
    ///
    /// The Ctrl+K prompt is what needs it: a question that has been sent is no
    /// longer the user's to type in, but rebuilding the field to say so re-runs
    /// its layout, and for one frame the fresh field has almost no width — the
    /// question wraps, the latched row count is wrong, and the bar opens a row
    /// taller with its text stranded at the top. That is why both the freeze and
    /// the dim were dropped when the bar was rebuilt around a stable field. Both
    /// are back here as things the *built* field reads: the colour inside its
    /// reactive style (like [`FieldCfg::uncommitted`]), and the editor's own
    /// `read_only` signal through an effect, which floem re-reads per keystroke.
    pub frozen: Option<Memo<bool>>,
    /// Enter **never** inserts a line break, whatever the modifiers — for a
    /// `multiline` box holding one *question* rather than a body. Wrapping and
    /// auto-grow are unaffected; only the key's meaning changes.
    ///
    /// [`Self::multiline`] otherwise decides two things at once: wrap-and-grow,
    /// and "Enter may break the line". They are the same answer for a snippet
    /// body and opposite answers for the Ctrl+K prompt, which wants to grow with
    /// a long question and has nothing a second line would mean. Leaving them
    /// tied put stray newlines in that prompt via whichever path reached the
    /// no-submit arm, and every fix that kept the tie was a fix to one path.
    pub enter_never_breaks: bool,
    /// Fixed box height. `None` = derive from content (auto-grow for multiline).
    ///
    /// A `fn`, like [`Self::font_size`] and for the same reason — the box has to
    /// hold a line of type that follows the interface scale. A resolved `f64`
    /// froze it: `Some(40.0)` beside `font_size: font_title` gave a 40px box a
    /// 41px line box at 200% and clipped the text, and even a `Some(field_input_h())`
    /// baked the scale in force when the field was built.
    pub height: Option<fn() -> f64>,
    /// Reactive override for the multiline auto-grow cap (rows). `None` =
    /// `CHAT_MAX_ROWS`. A signal so the cap can follow a resizing container (the
    /// value viewer caps at the results-panel height).
    pub max_rows: Option<RwSignal<usize>>,
    /// Multiline only: the **floor** the auto-grow box starts at, in rows.
    /// Default `1`, which is what a box that grows purely with its content does.
    ///
    /// A snippet's body wants 3: at one row the field reads as a single-line
    /// input and gives no sign that Enter is allowed in it. Clamped against
    /// [`FieldCfg::max_rows`] at use, so a floor above the cap cannot invert the
    /// two (`clamp` panics when `min > max`).
    pub min_rows: usize,
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
    /// Ctrl+Arrow Up / Down (the AI panel's prompt-history recall). Separate
    /// hooks from [`FieldCfg::on_arrow_up`] because they answer a different
    /// question: the plain arrows drive a list *beside* the field, these rewrite
    /// the field itself. When set, the key is consumed here.
    pub on_ctrl_arrow_up: Option<Rc<dyn Fn()>>,
    pub on_ctrl_arrow_down: Option<Rc<dyn Fn()>>,
    /// "This text was put here, not typed" — the field renders its whole buffer
    /// in [`FieldCfg::placeholder_color`] while set, and the next key that isn't
    /// a recall key (`on_ctrl_arrow_*`) or a bare modifier commits it: the flag
    /// clears and the caret jumps to the end.
    ///
    /// Escape is the exception — it *discards* rather than commits, emptying the
    /// field and keeping focus, so changing your mind about a recalled entry
    /// costs one key rather than a selection and a delete.
    ///
    /// The commit is synchronous, inside the key handler, because floem inserts
    /// a plain character *after* the handler returns (§ Floem: `text_editor_keys`
    /// inserts unconditionally) — deferring the caret move by even a tick would
    /// land that character wherever the caret happened to be sitting and only
    /// then jump to the end.
    pub uncommitted: Option<RwSignal<bool>>,
    /// Tab (e.g. accept the command-palette ghost completion). When set, the key
    /// is consumed here instead of inserting a tab / moving focus.
    pub on_tab: Option<Rc<dyn Fn()>>,
    /// Place the field in a modal's Tab order at this index.
    ///
    /// The ring has to reach *inside* the editor's key handler: floem registers
    /// the editor's KeyDown listener with `on_event_stop`, so no listener bolted
    /// on from outside — and not floem's own Tab traversal either — ever sees a
    /// key while a field has focus. [`FieldCfg::on_tab`] wins if both are set,
    /// since that one is an explicit override for a specific key.
    pub focus: Option<(widgets::FocusRing, u32)>,
    /// **Tab types an indent here instead of leaving.** For a field holding
    /// *code* — a trigger body, a function body, a view's `SELECT` — where
    /// indenting is ordinary typing and losing it to focus movement would be a
    /// worse trade than the extra key it costs to get out.
    ///
    /// The field still joins the ring, so Tab can still *arrive*; only the step
    /// away is suppressed, and floem's own `InsertTab` then runs. Escape is the
    /// way out, as it is from any field — it blurs to the enclosing
    /// [`widgets::focus_root`], whose Tab re-enters the ring **at the control
    /// after this one**, because the blur tells the ring where it was
    /// ([`widgets::FocusRing::remember`]).
    ///
    /// That last clause is the whole of why such a field can sit anywhere in a
    /// ring. Re-entry used to restart at position 0, which made any placement
    /// but the last one a trap: Tab typed an indent, Escape went to the root,
    /// and the root's Tab came back to the top — so every control below the
    /// field was unreachable by forward Tab.
    ///
    /// Not for prose (the AI settings' custom instructions) and not for a
    /// read-only script box (the DDL preview's), where there is no indent to
    /// type and Tab moving on is simply the better behaviour.
    pub tab_indents: bool,
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
            font_size: theme::font_body,
            mono: false,
            border_radius: 6.0,
            read_only: false,
            frozen: None,
            enter_never_breaks: false,
            height: None,
            max_rows: None,
            min_rows: 1,
            no_wrap: false,
            text_color: None,
            placeholder_color: None,
            border_color: None,
            on_submit: None,
            on_escape: None,
            on_blur: None,
            on_arrow_up: None,
            on_arrow_down: None,
            on_ctrl_arrow_up: None,
            on_ctrl_arrow_down: None,
            uncommitted: None,
            on_tab: None,
            focus: None,
            tab_indents: false,
            caret_end: None,
            trailing: None,
        }
    }
}

/// True for a key that only modifies other keys, so pressing it alone means the
/// user hasn't typed anything yet.
///
/// [`FieldCfg::uncommitted`] needs it: the recall keys are Ctrl+Arrow, and the
/// Ctrl arrives as its own key-down first — treated as input, it would commit
/// the recalled text before the arrow that was meant to replace it.
fn is_modifier_key(k: &Key) -> bool {
    matches!(
        k,
        Key::Named(
            NamedKey::Alt
                | NamedKey::AltGraph
                | NamedKey::CapsLock
                | NamedKey::Control
                | NamedKey::Fn
                | NamedKey::FnLock
                | NamedKey::Meta
                | NamedKey::NumLock
                | NamedKey::ScrollLock
                | NamedKey::Shift
                | NamedKey::Symbol
                | NamedKey::SymbolLock
                | NamedKey::Super
                | NamedKey::Hyper
        )
    )
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

/// What Tab does in an [`edit_field`] — see [`tab_action`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum TabAction {
    /// Run [`FieldCfg::on_tab`] (the command palette's ghost completion).
    Callback,
    /// Move to the next control in [`FieldCfg::focus`]'s ring.
    StepRing,
    /// Leave the key to floem, whose editor maps it to `InsertTab`.
    Insert,
}

/// [`edit_field`]'s three-way Tab precedence, as one decision rather than three
/// sequential `if`s.
///
/// `on_tab` first, because it is an explicit override for this specific key;
/// then [`FieldCfg::tab_indents`], which is the field saying Tab is typing here;
/// then the ring. A field with none of them lets floem insert a tab, which is
/// also what a `tab_indents` field does — the difference between those two is
/// only reachable through the ring, so the enum keeps them distinct rather than
/// collapsing to a bool.
///
/// **Shift+Tab never runs `on_tab`.** Accepting a completion is a forward
/// motion, and its one caller (the palette) has no ring, so a shifted Tab there
/// falls through to `Insert` — which is what the code did before this was
/// written down, and what the doc on `on_tab` says.
pub(crate) fn tab_action(on_tab: bool, tab_indents: bool, in_ring: bool, shift: bool) -> TabAction {
    if on_tab && !shift {
        TabAction::Callback
    } else if in_ring && !tab_indents {
        TabAction::StepRing
    } else {
        TabAction::Insert
    }
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
        frozen,
        enter_never_breaks,
        height,
        max_rows,
        min_rows,
        no_wrap,
        text_color,
        placeholder_color,
        border_color,
        on_submit,
        on_escape,
        on_blur,
        on_arrow_up,
        on_arrow_down,
        on_ctrl_arrow_up,
        on_ctrl_arrow_down,
        uncommitted,
        on_tab,
        focus,
        tab_indents,
        caret_end,
        trailing,
    } = cfg;
    // An in-flow trailing action (like the clearable ×) shrinks the editor.
    let has_side = clearable || trailing.is_some();
    // Line height derived from the font so the box height matches the rendered
    // text (≈1.46× the app's body rhythm: 13→19, 16→23).
    //
    // Closures, not values: every one of these is a *derived* size, and the font
    // they derive from follows the interface scale. Resolved once here they would
    // hold the size the field was built at — which is what left a 13px value
    // inside a box grown for 20px type, with the trailing action (scaled through
    // `icons::icon`) overhanging the field's own edge.
    let line_h = move || field_line_h(font_size());
    // Keep as `fn`s (not resolved Colors) so the style closures below can call
    // them and follow a live theme switch (§7.4).
    let text_color: fn() -> floem::peniko::Color = text_color.unwrap_or(theme::text);
    let placeholder_color: fn() -> floem::peniko::Color =
        placeholder_color.unwrap_or(theme::placeholder);
    // With a fixed height, centre the single line vertically; otherwise use the
    // standard vertical padding and let the height follow the content.
    let pad_v = move || match height {
        Some(hf) => ((hf() - line_h()) / 2.0 - 2.0).max(2.0),
        None => chat_pad_v(),
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
    let ctrl_up = on_ctrl_arrow_up.clone();
    let ctrl_down = on_ctrl_arrow_down.clone();
    let tab = on_tab.clone();
    let key_focus = focus.clone();
    let editor = text_editor_keys(text_sig.get_untracked(), move |editor_sig, kp, mods| {
        // Ctrl+Arrow recall, before the plain-arrow hooks: the modifier is what
        // tells the two apart, and the plain-arrow branch below doesn't look at
        // it.
        if mods.control()
            && let Some(cb) = match &kp.key {
                KeyInput::Keyboard(Key::Named(NamedKey::ArrowUp), _) => ctrl_up.as_ref(),
                KeyInput::Keyboard(Key::Named(NamedKey::ArrowDown), _) => ctrl_down.as_ref(),
                _ => None,
            }
        {
            (cb)();
            return CommandExecuted::Yes;
        }
        // Escape *discards* recalled text instead of committing it: the recall
        // put it there, so the way back out is the empty box it started from,
        // with the caret still in it. Consuming the key keeps focus — the field
        // has answered this Escape, and the next one blurs as usual, which is
        // the same one-layer-per-press step the modals take.
        if let Some(flag) = uncommitted
            && flag.get_untracked()
            && matches!(kp.key, KeyInput::Keyboard(Key::Named(NamedKey::Escape), _))
        {
            flag.set(false);
            text_sig.set(String::new());
            return CommandExecuted::Yes;
        }
        // Anything else — but not a bare modifier, or holding Ctrl down for the
        // recall keys above would itself commit — turns recalled text into the
        // user's own. Caret to the end first: floem applies a plain character
        // after this handler returns, so it lands after the text rather than
        // inside it.
        if let Some(flag) = uncommitted
            && flag.get_untracked()
            && !matches!(&kp.key, KeyInput::Keyboard(k, _) if is_modifier_key(k))
        {
            flag.set(false);
            editor_sig.with_untracked(|e| {
                let len = e.doc().text().to_string().len();
                e.cursor.update(|c| c.set_offset(len, false, false));
            });
        }
        if matches!(kp.key, KeyInput::Keyboard(Key::Named(NamedKey::Escape), _)) {
            match &escape {
                Some(esc) => (esc)(),
                // No handler of its own: Escape blurs the field. Floem hands a key
                // event to the focused view and, when it consumes one, to nobody
                // else — so a field that keeps Escape leaves an enclosing modal
                // with no way to close from the keyboard. Focus goes back to the
                // innermost overlay rather than merely away, because with focus on
                // nothing the key reaches only the root view, not that overlay's
                // own handler; the *next* Escape then closes it. The clear is
                // unconditional so a field outside any overlay still blurs.
                None => {
                    if let Some(vid) =
                        editor_sig.with_untracked(|e| e.editor_view_id.get_untracked())
                    {
                        // Tell the ring where the walk was before handing the
                        // keyboard back, so the root's Tab resumes *after* this
                        // field. Without it re-entry always restarted at position
                        // 0, which made a `tab_indents` field — where Escape is
                        // the only way out — a trap: every control below it in
                        // the ring was unreachable by forward Tab.
                        vid.clear_focus();
                        widgets::hand_keyboard_back(
                            key_focus.as_ref().map(|(ring, _)| (ring, vid)),
                        );
                    } else {
                        widgets::hand_keyboard_back(None);
                    }
                }
            }
            return CommandExecuted::Yes;
        }
        if matches!(kp.key, KeyInput::Keyboard(Key::Named(NamedKey::Tab), _)) {
            match tab_action(
                tab.is_some(),
                tab_indents,
                key_focus.is_some(),
                mods.shift(),
            ) {
                TabAction::Callback => {
                    if let Some(cb) = &tab {
                        (cb)();
                    }
                    return CommandExecuted::Yes;
                }
                TabAction::StepRing => {
                    if let Some((ring, _)) = &key_focus
                        && let Some(me) =
                            editor_sig.with_untracked(|e| e.editor_view_id.get_untracked())
                    {
                        ring.step_from(me, mods.shift());
                        return CommandExecuted::Yes;
                    }
                }
                TabAction::Insert => {}
            }
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
            // Plain Enter submits (multiline: only plain — Shift/Ctrl+Enter are
            // the newline). **A field with nothing to submit to does not eat the
            // key**: a multiline one lets it through and breaks the line, which
            // is what Enter means in a box of SQL. Swallowing it unconditionally
            // left every body field — the snippet editor's, the view editor's —
            // with an Enter that did nothing at all and a Shift+Enter nobody
            // would guess at.
            // `enter_never_breaks` opts a multiline field out of the newline
            // entirely — it is the *question* case, where a second line means
            // nothing and the box is multiline only to wrap and grow.
            let plain = !mods.shift() && !mods.control();
            // `enter_never_breaks` is folded in here rather than into `plain`: once
            // a field cannot break its line, no modifier combination changes what
            // Enter means, so the guards below stop consulting `plain` at all.
            let breaks = multiline && !enter_never_breaks;
            match &submit {
                Some(cb) if !breaks || plain => {
                    (cb)();
                    return CommandExecuted::Yes;
                }
                // A field that cannot break its line never inserts a newline,
                // submit or not: swallowing the key is what keeps its text intact.
                None if !breaks => return CommandExecuted::Yes,
                _ => {}
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
    // `frozen` drives the same signal the builder above sets once, so floem's own
    // per-keystroke checks (`TextDocument::receive_char` / `run_command`, both of
    // which read it untracked at the moment of the key) do the enforcing — there
    // is no second gate here to fall out of step with them. A fixed `read_only`
    // wins whatever the signal says, and the caret is taken down with the edits:
    // a blinking caret in a field that will not take a character is the wrong
    // promise, and it is the same treatment `read_only` gets on focus below.
    if let Some(frozen) = frozen {
        let ed_frozen = ed.clone();
        create_effect(move |_| {
            let stop = read_only || frozen.get();
            ed_frozen.read_only.set(stop);
            if stop {
                ed_frozen.cursor_info.hidden.set(true);
                ed_frozen
                    .cursor_info
                    .blink_timer
                    .set(floem::action::TimerToken::INVALID);
            } else if focused.get_untracked() {
                ed_frozen.cursor_info.reset();
            }
        });
    }

    // Plain styling in the app's body font, with an explicit line height so the
    // box-height math below matches the rendered lines.
    //
    // `FieldStyling`, not a `SimpleStyling`: the builder takes the size *by
    // value*, so a field built at 100% kept drawing 13px text after the interface
    // scale moved. (Wrapping is controlled by `wrap_method` on `editor_style`
    // below either way — `SimpleStyling` drops its own in `build()`.)
    let styling = FieldStyling {
        size: font_size,
        family: vec![FamilyOwned::Name(
            if mono { MONO_FAMILY } else { "IBM Plex Sans" }.to_string(),
        )],
    };

    // A multiline field that doesn't wrap can overflow sideways, so it needs the
    // two affordances every other scroll surface here has: Shift+wheel, and a bar
    // that fades out when nothing is scrolling.
    //
    // Both have to reach the editor's *internal* scroll, which this code doesn't
    // own — `shift_hscroll`/`autohide` wrap a `Scroll` and there is none to wrap.
    // So the wheel listener goes straight onto that view (the parent of
    // `editor_view_id`, the same handle `editor_pane` uses), and the bar is faded
    // through its `Handle` colour rather than `hide_bars`, since alpha is the one
    // lever reachable from a style class (§ Floem: no `opacity` property).
    let (bar_shown, bar_poke) = widgets::autohide_state();
    if multiline {
        let ed_wheel = ed.clone();
        if let Some(scroll_id) = ed.editor_view_id.get_untracked().and_then(|c| c.parent()) {
            let poke = bar_poke.clone();
            scroll_id.add_event_listener(
                EventListener::PointerWheel,
                Box::new(move |e| {
                    let Event::PointerWheel(pe) = e else {
                        return EventPropagation::Continue;
                    };
                    // Any wheel over the field is scroll activity worth showing
                    // the bar for, shifted or not.
                    (poke)();
                    if !pe.modifiers.shift() {
                        return EventPropagation::Continue;
                    }
                    // Windows delivers shift+wheel as a vertical delta; map it to
                    // x. Floem's scroll runs registered listeners before its own
                    // wheel handling, so `Stop` suppresses the vertical scroll.
                    let dx = if pe.delta.x != 0.0 {
                        pe.delta.x
                    } else {
                        pe.delta.y
                    };
                    if dx != 0.0 {
                        ed_wheel.scroll_delta.set(floem::kurbo::Vec2::new(dx, 0.0));
                    }
                    EventPropagation::Stop
                }),
            );
        }
        // Caret movement scrolls too, and that never goes through the wheel.
        let ed_vp = ed.clone();
        let poke = bar_poke.clone();
        create_effect(move |prev: Option<()>| {
            ed_vp.viewport.track();
            // Not on the first run — that's establishing tracking, and a bar
            // flashing on every field that mounts would be worse than none.
            if prev.is_some() {
                (poke)();
            }
        });
    }

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
                // Recalled-but-not-yet-owned text reads as the placeholder it
                // stands in for. Read inside the reactive style, so committing
                // repaints it without rebuilding the field.
                //
                // A frozen field dims *its own* colour rather than switching to a
                // theme grey: what it has to say is "this text is no longer
                // yours to edit", and half-strength of whatever the field was
                // painted in says that in both themes and in a field whose
                // caller overrode the colour (the Ctrl+K bar does).
                .color(if frozen.is_some_and(|f| f.get()) {
                    text_color().multiply_alpha(0.5)
                } else if uncommitted.is_some_and(|u| u.get()) {
                    placeholder_color()
                } else {
                    text_color()
                })
                .background(floem::peniko::Color::TRANSPARENT)
                .class(Handle, move |s| {
                    if multiline {
                        // The chat box shows a thin bar past the row cap. Faded
                        // out while idle rather than hidden: alpha is what a
                        // style class can reach, and it transitions.
                        s.set(Thickness, Px(6.0))
                            .set(Rounded, true)
                            .background(theme::scrollbar().multiply_alpha(if bar_shown.get() {
                                1.0
                            } else {
                                0.0
                            }))
                            .transition_background(floem::style::Transition::ease_in_out(
                                std::time::Duration::from_millis(200),
                            ))
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
                // **Say so, because a hand-back may be queued behind this.** A
                // field that autofocuses on mount is very often mounted *by* the
                // gesture that closed something else, and closing an overlay
                // queues `widgets::hand_keyboard_back`'s deferred home — the
                // Ctrl+K bar opened from the editor's right-click menu being the
                // case that shipped broken. See `widgets::claim_keyboard`.
                widgets::claim_keyboard();
                vid.request_focus();
                // Land the caret at the end of any seeded text — a programmatic
                // focus on a prefilled field (e.g. the inline tab rename) should
                // sit after the text, not before it. (Empty fields: end == 0.)
                let len = ed_af.doc().text().to_string().len();
                ed_af.cursor.update(|c| c.set_offset(len, false, false));
            }
        });
    }

    // Join the modal's Tab order. The ring holds the *inner* editor view, not
    // this one: that is what `request_focus` has to target (autofocus above
    // takes the same id) and what the key handler reports as "me". It only
    // exists once the editor is built, so register from an effect on the signal
    // that publishes it rather than at build time, and remember what was
    // registered so cleanup can withdraw it without reading a disposed signal.
    let registered: Rc<std::cell::Cell<Option<floem::ViewId>>> =
        Rc::new(std::cell::Cell::new(None));
    if let Some((ring, tabindex)) = focus.clone() {
        let ed_reg = ed.clone();
        let reg = registered.clone();
        create_effect(move |_| {
            if let Some(vid) = ed_reg.editor_view_id.get() {
                ring.register(tabindex, vid);
                reg.set(Some(vid));
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
            let vp = ed_rows.viewport.get();
            // **`screen_lines` as well as `viewport`, and the second one is what
            // makes the count true.** A width change arrives on `viewport`, but
            // floem answers it in an effect of its own on that same signal:
            // `ed.lines.set_wrap(Width(viewport.width()))`, which only *clears*
            // the line layouts — they are rebuilt lazily, later. So a count read
            // on the `viewport` edge alone is computed from the layout of the
            // **previous** width, and `Lines::last_vline` caches its answer, so a
            // box that measured two rows at a first-frame width stayed two rows
            // until the next keystroke re-measured it. That is the Ctrl+K bar
            // opening a row taller with its question stranded at the top — the
            // rebuild-driven half of it was fixed by keeping the field across the
            // Idle → Busy transition, and this is the other half, the one that
            // survived and showed up about one opening in seven.
            // `screen_lines` changes once those layouts exist again
            // (`update_screen_lines` walks the visual lines, which is what builds
            // them), so it is the first moment the count is worth reading.
            ed_rows.screen_lines.track();
            // Not before the first layout. `EditorWidth` wrapping at a zero width
            // puts every character on its own visual line, so measuring there
            // reports one row per character and inflates the box — which then
            // stays inflated until the next keystroke re-measures it. The width
            // arriving is itself a viewport change, so this effect re-runs.
            if vp.width() < 1.0 {
                return;
            }
            let n = ed_rows.last_vline().get() + 1;
            if rows.get_untracked() != n {
                rows.set(n);
            }
        });
    }

    // **Repaint when the box gains or loses a row.**
    //
    // The height is derived from `rows`, which this edit has just changed — but
    // the editor has already painted itself against the height it had *before*
    // it. So the caret sitting on the brand-new last line is outside the
    // viewport that was drawn, and simply isn't there: press Shift+Enter in a
    // body field and the caret vanishes until the next keystroke, which repaints
    // against a box that has since grown. Nothing is wrong with the caret's
    // position — only with when it was drawn.
    //
    // Deferred a tick, so the new height has been through layout by the time the
    // paint is asked for, and `try_get_untracked` because the field can be
    // disposed inside that tick (a modal closed on the same keypress).
    if multiline {
        let ed_paint = ed.clone();
        create_effect(move |prev: Option<usize>| {
            let n = rows.get();
            if prev.is_some_and(|p| p != n) {
                let ed_paint = ed_paint.clone();
                floem::action::exec_after(std::time::Duration::ZERO, move |_| {
                    if let Some(Some(vid)) = ed_paint.editor_view_id.try_get_untracked() {
                        vid.request_paint();
                    }
                });
            }
            n
        });
    }

    // `focused` mirrored as plain, non-reactive state, for the deferred blur
    // below to read — and for the cleanup, which has to know whether this field
    // held the keyboard as it went. Neither can use the signal: by the time they
    // run, the field's scope may already be disposed, and a `None` there can't
    // be told apart from "focus came back" — the one thing they need to know.
    let focus_now = std::rc::Rc::new(std::cell::Cell::new(false));

    // Caret focus-gating + border focus tracking. The focus-lost effect is
    // created second so it wins the initial run → the field starts unfocused
    // (unless `autofocus` re-focuses it right after).
    {
        let focus_now = focus_now.clone();
        let focus_now_f = focus_now.clone();
        let ed_focus = ed.clone();
        create_effect(move |_| {
            ed_focus.editor_view_focused.track();
            focused.set(true);
            focus_now_f.set(true);
            // A read-only field can still be focused (to receive Enter/Escape),
            // but shows no blinking caret. **`frozen` counts here too**, and
            // untracked so this stays a focus effect: the Ctrl+K bar opens
            // already `Busy` on an AI fix, so a focus that ignored the freeze
            // would put a blinking caret back in a field that takes no keys —
            // the one state where the two effects race, and focus arrives last.
            if read_only || frozen.is_some_and(|f| f.get_untracked()) {
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
            focus_now.set(false);
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
                // Deferred a tick, because a focus-lost does NOT mean the field
                // was blurred. Floem clears `app_state.focus` on *every*
                // pointer-down and re-requests it as a queued update message
                // (window_handle.rs), so a click inside an already-focused field
                // raises a real FocusLost→FocusGained pair. Firing `on_blur` on
                // the Lost half committed and closed the field the user had only
                // clicked into to move the caret — the tab rename, and the row
                // panel's JSON leaf editor. One tick later the queued FocusGained
                // has landed, so `focus_now` separates a click from a real blur.
                let cb = cb.clone();
                let focus_now = focus_now.clone();
                floem::action::exec_after(std::time::Duration::ZERO, move |_| {
                    if !focus_now.get() {
                        (cb)();
                    }
                });
            }
        });
    }

    // Placeholder overlay: shown only when EMPTY *and* unfocused, positioned over
    // where the first line of text renders — and bounded on *both* sides, so one
    // longer than the field ellipsizes at its edge instead of painting across the
    // border (see `placeholder_right_inset`).
    let ph_top = move || pad_v() + (line_h() - font_size() as f64) / 2.0;
    let has_trailing = trailing.is_some();
    let placeholder = dyn_container(
        move || text_sig.with(|t| t.is_empty()) && !focused.get(),
        move |show| {
            if show {
                text(placeholder)
                    .style(move |s| {
                        s.font_size(font_size())
                            .font_family("IBM Plex Sans".to_string())
                            .color(placeholder_color())
                            // The trim itself. `width_full` is what makes it
                            // definite — a label sizes to its content otherwise
                            // and overflows the box that was meant to bound it —
                            // and `min_width(0)` is what lets it shrink below
                            // that content width in the first place.
                            .width_full()
                            .min_width(0.0)
                            .text_ellipsis()
                    })
                    .into_any()
            } else {
                empty().into_any()
            }
        },
    )
    .style(move |s| {
        s.absolute()
            .inset_left(chat_pad_h())
            .inset_right(placeholder_right_inset(has_trailing))
            .inset_top(ph_top())
    })
    // Let clicks fall through to the editor beneath — otherwise clicking on the
    // placeholder text (which sits on top) fails to focus the field.
    .pointer_events(|| false);

    // Trailing × that empties the value. In-flow beside the editor (which
    // flex-grows) — NOT an absolute overlay — so the editor's width is bounded
    // and its text can never scroll underneath the ×.
    let inner: AnyView = if let Some(trailing) = trailing {
        // Trailing action (e.g. the AI send/stop icon) in-flow beside the editor,
        // right-aligned — same spot as the clearable ×. The negative right margin
        // pulls it 4px closer to the box edge (14px gap).
        //
        // Fixed size, pinned to the **bottom**: a multiline box grows with its
        // text, and an action that merely centred in it wandered down the box as
        // the message got taller (and stretched with it). `align_self` overrides
        // the row's `items_center` for this one child; at a single row the box is
        // one line tall, so bottom and centre are the same place and nothing about
        // the common case changes.
        let side = container(trailing()).style(|s| {
            s.flex_shrink(0.0_f32)
                .width(trailing_size())
                .height(trailing_size())
                .items_center()
                .justify_center()
                .align_self(Some(floem::taffy::style::AlignItems::FlexEnd))
                .margin_left(trailing_gap_l())
                .margin_right(trailing_gap_r())
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
                                .margin_left(theme::scaled(6.0))
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

    // A click anywhere in the BOX focuses the field. The editor is the only child
    // that takes focus and it spans only the content box, so everything around it
    // — the 1px border, the 10px horizontal padding, the `pad_v` vertical padding,
    // and the trailing gutter — was dead: a text cursor, and nothing on click.
    // That is most of the box: a default single-line field is 34px tall with a
    // 20px editor in it, so the top and bottom fifths of *every* field in the app
    // missed, and `clearable` makes the right edge widest at ~32px (10px padding +
    // a 22px button slot).
    //
    // Gated on the editor's own rect rather than on propagation: floem's editor
    // handles `PointerDown` with `on_event_cont`, so it does NOT consume the event
    // and this listener runs for in-text clicks too. Without the gate, clicking
    // mid-text would focus correctly and then have the caret yanked elsewhere.
    let ed_click = ed.clone();
    // Leave the Tab order when the field unmounts (the SSH block folding away),
    // or the ring would hand focus to a view that no longer exists. Reads the id
    // recorded at registration rather than the editor's signal, which by cleanup
    // time may already be disposed.
    let ring_cleanup = focus.map(|(ring, _)| ring);
    stack((inner, placeholder))
        .on_cleanup(move || {
            // Hand the keyboard back if this field held it — floem clears the
            // focus of a removed view *silently*, so a field that unmounts while
            // focused (a branch folding away under the control that changed it)
            // otherwise leaves the modal around it answering neither Escape nor
            // Tab. Same step `focus_root`'s and `in_focus_ring`'s cleanups take,
            // through the same function, so the `remember` half can't be left out
            // of one of the three again. Before unregistering, so the tabindex is
            // still there to be remembered.
            if focus_now.get() {
                widgets::hand_keyboard_back(ring_cleanup.as_ref().zip(registered.get()));
            }
            if let (Some(ring), Some(vid)) = (ring_cleanup.as_ref(), registered.get()) {
                ring.unregister(vid);
            }
        })
        .on_event_cont(EventListener::PointerDown, move |e| {
            let Event::PointerDown(pe) = e else { return };
            // The editor sits at the content origin (1px border + the box padding)
            // and is exactly as large as its own viewport; the rest is chrome.
            let (left, top) = (1.0 + chat_pad_h(), 1.0 + pad_v());
            let vp = ed_click.viewport.get_untracked();
            if pe.pos.x >= left
                && pe.pos.x <= left + vp.width()
                && pe.pos.y >= top
                && pe.pos.y <= top + vp.height()
            {
                return; // on the text — the editor already placed the caret
            }
            let Some(Some(vid)) = ed_click.editor_view_id.try_get_untracked() else {
                return;
            };
            vid.request_focus();
            // Caret at the nearest text position to the click, in *content* coords
            // so a scrolled (multiline) field maps correctly. `offset_of_point`
            // clamps a point outside the text to the nearest line and its nearest
            // column, which is exactly what a click in the surrounding chrome
            // wants: the padding above the first line lands on it, the gutter past
            // the end of a line lands at its end.
            let p = Point::new(pe.pos.x - left + vp.x0, pe.pos.y - top + vp.y0);
            let (off, _) = ed_click.offset_of_point(Mode::Insert, p);
            ed_click.cursor.update(|c| c.set_offset(off, false, false));
        })
        .style(move |s| {
            // Fixed height when given; else derive from content. +3 (auto case): the
            // 1px top/bottom borders (border-box) plus a hair of slack so the editor's
            // viewport fully contains its content and no phantom scrollbar shows.
            //
            // **Deliberately not scaled**, unlike the `pad_v()` and `line_h()` it is
            // added to. All three pixels are hairlines: two are borders, which stay 1px
            // at every scale, and the third absorbs a rounding error that doesn't grow
            // with the box — each part is already rounded to a whole pixel before it
            // gets here. Scaling it would just add unexplained height at 130/160%.
            let h = match height {
                Some(hf) => hf(),
                None => {
                    // Effective cap: a reactive `max_rows` (viewer) else the default.
                    let cap_n = max_rows.map(|m| m.get()).unwrap_or(cap).max(1);
                    // The floor never exceeds the cap: `clamp` panics if it does,
                    // and a caller asking for more rows than the box may show is
                    // asking for the cap.
                    let floor = min_rows.max(1).min(cap_n);
                    rows.get().clamp(floor, cap_n) as f64 * line_h() + pad_v() * 2.0 + 3.0
                }
            };
            // No flex_grow baked in: in a vertical stack that would stretch the box's
            // HEIGHT and blow past `h`. Callers that need to fill a row (the chat box)
            // add flex_grow themselves.
            let s = s
                .min_width(0.0)
                .height(h)
                .padding_horiz(chat_pad_h())
                .padding_vert(pad_v())
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
        .style(|st| {
            st.color(theme::status_text())
                .font_size(theme::font_status())
        })
        .into_any()
}

/// A clickable status-bar segment that opens a `menu_panel` popup centred above
/// it (the Tabs/Spaces, AI-model and AI-effort menus, which share the one popup
/// channel). A second click on the *same* segment toggles it shut, while clicking
/// a different one switches menus — which segment the open menu belongs to is
/// [`widgets::menu_anchored_at`]'s question, answered from the anchor rather than
/// a tag of its own. Its window rect is tracked (its x shifts as segments to its
/// left change width) so the popup can centre on it, and that same rect is what
/// identifies it.
fn status_menu_seg(
    label: impl Fn() -> String + 'static,
    build_entries: impl Fn() -> Vec<MenuEntry> + 'static,
    popup_menu: RwSignal<Option<Vec<MenuEntry>>>,
    popup_anchor: RwSignal<Option<PopupAnchor>>,
    popup_width: RwSignal<f64>,
    margin: f64,
) -> impl IntoView {
    let origin: RwSignal<(f64, f64)> = RwSignal::new((0.0, 0.0));
    let size: RwSignal<(f64, f64)> = RwSignal::new((0.0, 0.0));
    let build = Rc::new(build_entries);
    // One spelling of this segment's placement, because the value that *places*
    // the panel is the value that says the open menu is this segment's — see
    // `widgets::menu_anchored_at`. Written twice, a pixel of drift would leave the
    // menu opening correctly and silently refusing to toggle shut.
    let anchor_here = move || {
        let (ox, _oy) = origin.get_untracked();
        let (sw, _sh) = size.get_untracked();
        PopupAnchor::AboveFooter(ox, ox + sw)
    };
    dyn_container(label, move |s| {
        text(s)
            .style(|s| s.font_size(theme::font_status()))
            .into_any()
    })
    .on_move(move |p| origin.set((p.x, p.y)))
    .on_resize(move |r| size.set((r.width(), r.height())))
    // Stop the pointer-down so the workspace-root "close on down" handler doesn't
    // fire for our own clicks (else down closes and up reopens — never toggling),
    // and do the one thing that handler owes us in return — see
    // `widgets::menu_trigger_press`.
    .on_event_stop(EventListener::PointerDown, widgets::menu_trigger_press)
    .on_click_stop(move |_| {
        // A second press closes what the first opened. This used to ask a
        // `menu_owner: RwSignal<u8>` tag written only by these segments, which
        // went stale the moment anything else filled the shared channel: open a
        // segment's menu, right-click a grid cell, press the segment again, and it
        // closed the cell's menu instead of opening its own. The anchor cannot
        // drift that way — every opener overwrites it.
        if crate::widgets::menu_anchored_at(
            popup_menu.get_untracked().is_some(),
            popup_anchor.get_untracked(),
            anchor_here(),
        ) {
            popup_menu.set(None);
            return;
        }
        popup_anchor.set(Some(anchor_here()));
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
/// within `footer_collapse_gap()` px of the right-hand icon group (`ai_x` = the AI
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
        ax < 1.0 || edge.get_untracked() + footer_collapse_gap() <= ax
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
            if ax >= 1.0 && edge.get() + footer_collapse_gap() > ax {
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
    // enough), so a panel locked away by a narrow window reads as inactive.
    //
    // **And it now looks inert, because it is.** Below its breakpoint a panel is
    // force-hidden and its toggle could do nothing — but it went on reading as a
    // live control, so a click that changed a signal nothing renders was
    // indistinguishable from a broken button. `panel_toggle` answers the disabled
    // face and the tooltip from one call, and `tip_when` shows that tooltip only
    // while there is something to say. Each toggle reads the predicate for the
    // panel *it* opens — the schema tree has a narrower breakpoint than the right
    // column, so the two disable at different widths.
    let schema_toggle = move || panel_toggle(schema_panel_allowed(), true);
    let schema_icon = tip_when(
        toggle_icon(
            icons::FOLDER_TREE,
            move || schema_toggle().enabled,
            move || schema_visible.get() && schema_panel_allowed(),
            move || {
                if schema_panel_allowed() {
                    schema_visible.update(|v| *v = !*v);
                }
            },
        )
        .style(|s| s.margin_left(theme::scaled(5.0))),
        move || schema_toggle().tip,
    );
    // Does the *active connection's* engine have server sessions to show? The
    // Activity panel is a property of the connection, not of the focused tab, so
    // this reads `active_conn` rather than the tab's dialect the way `read_only`
    // above does.
    let activity_ok = create_memo(move |_| {
        let cid = active_conn.get();
        connections.with(|cs| {
            cs.iter().find(|c| c.id == cid).is_some_and(|c| {
                schemaic_core::activity::supports_activity(SqlDialect::from_db_type(&c.db_type))
            })
        })
    });

    // The AI icon's left edge (window x) is the reference the left cluster
    // collapses against — it's the leftmost thing in the right-pinned group, so it
    // marches left as the window narrows.
    let ai_x = RwSignal::new(0.0_f64);
    // The three plain right-column toggles share one answer: the only thing that
    // can take their panel away is the window.
    let right_toggle = move || panel_toggle(right_panel_allowed(), true);
    // Server Activity has a second reason to be inert, and `panel_toggle` is what
    // keeps the two from being confused: the engine gate stops the panel being
    // *opened* where it has nothing to show — never its being closed, since a
    // panel left open by a connection switch (or restored from the last session)
    // has to stay dismissable from its own icon, the one place anyone looks. A
    // narrow window overrides both, because that is the reason the user can act on.
    let activity_toggle = move || {
        panel_toggle(
            right_panel_allowed(),
            activity_ok.get() || right_panel.get() == RightPanel::Activity,
        )
    };
    let right_group = h_stack((
        tip_when(
            toggle_icon_view(
                icons::icon_wh(icons::AI_LOGO, 16.0, 10.0).style(|s| s.flex_shrink(0.0_f32)),
                move || right_toggle().enabled,
                move || right_panel.get() == RightPanel::Ai && right_panel_allowed(),
                move || set_right(RightPanel::Ai),
            )
            // Before the tooltip wrapper, so the x published here stays the
            // icon's own — `.tooltip()` allocates a fresh `ViewId` around it.
            .on_move(move |p| ai_x.set(p.x)),
            move || right_toggle().tip,
        ),
        tip_when(
            toggle_icon(
                icons::TIMELINE,
                move || right_toggle().enabled,
                move || right_panel.get() == RightPanel::History && right_panel_allowed(),
                move || set_right(RightPanel::History),
            ),
            move || right_toggle().tip,
        ),
        // The snippet library, next to the history it is the counterpart of: one
        // is what ran, the other is what you keep.
        tip_when(
            toggle_icon(
                icons::BOOKMARK,
                move || right_toggle().enabled,
                move || right_panel.get() == RightPanel::Snippets && right_panel_allowed(),
                move || set_right(RightPanel::Snippets),
            ),
            move || right_toggle().tip,
        ),
        // Server Activity. Inert on a connection whose engine has no sessions —
        // the toggle is the panel's front door, and a SQLite connection has
        // nothing behind it (`activity::supports_activity`). That case stays
        // silent on purpose; see `panel_toggle`.
        tip_when(
            toggle_icon(
                icons::ACTIVITY_SQUARE,
                move || activity_toggle().enabled,
                move || right_panel.get() == RightPanel::Activity && right_panel_allowed(),
                move || set_right(RightPanel::Activity),
            ),
            move || activity_toggle().tip,
        ),
        tip_when(
            toggle_icon(
                icons::TERMINAL,
                move || right_toggle().enabled,
                move || right_panel.get() == RightPanel::Terminal && right_panel_allowed(),
                move || set_right(RightPanel::Terminal),
            )
            .style(|s| s.margin_right(theme::scaled(5.0))),
            move || right_toggle().tip,
        ),
    ))
    .style(|s| s.flex_row().items_center().gap(theme::scaled(10.0)));

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
                .style(|s| s.font_size(theme::font_status()))
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
        s.margin_left(theme::scaled(40.0))
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
                .style(|s| s.font_size(theme::font_status()))
                .into_any()
        },
    )
    .on_click_stop(move |_| word_wrap.update(|w| *w = !*w))
    .style(|s| {
        s.margin_left(theme::scaled(15.0))
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
                    text(n.to_string()).style(|s| {
                        s.margin_left(theme::scaled(5.0))
                            .font_size(theme::font_status())
                    }),
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
        let s = s.margin_left(theme::scaled(40.0)).items_center();
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
                .style(|s| s.font_size(theme::font_status()))
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
        s.margin_left(theme::scaled(15.0))
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

    // Does the active tab's engine have a manual-transaction mode at all?
    //
    // SQLite doesn't yet — `schemaic_db::session::Session::open` refuses one,
    // because a pinned `rusqlite::Connection` is blocking and `!Sync` and needs a
    // thread of its own. The segment is hidden rather than left clickable: a
    // control that reports an error every time it is pressed is worse than one
    // that isn't there, and the cluster below it (the pill, Commit, Rollback)
    // only ever appears while a transaction is open, which can't happen here.
    let conns_for_tx = ui.conn.connections;
    let manual_supported = create_memo(move |_| {
        let Some(tab) = active_tab() else { return true };
        let cid = tab.conn_id.get();
        conns_for_tx.with(|cs| {
            cs.iter()
                .find(|c| c.id == cid)
                .map(|c| !schemaic_core::connection::is_sqlite(&c.db_type))
                // An unknown connection keeps the segment: hiding chrome on a
                // lookup miss would be a worse guess than showing it.
                .unwrap_or(true)
        })
    });

    let set_tx_mode = ui.tab_actions.set_tx_mode.clone();
    let mode_seg = dyn_container(
        move || tx_mode.get(),
        move |m| {
            text(m.label())
                .style(|s| s.font_size(theme::font_status()))
                .into_any()
        },
    )
    .on_click_stop(move |_| {
        // The hidden segment can't be clicked, but the guard is stated here too:
        // this is the one path into a mode the engine has no session for.
        if !manual_supported.get_untracked() {
            return;
        }
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
        if !manual_supported.get() {
            return s.hide();
        }
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
        s.margin_left(theme::scaled(40.0))
            .items_center()
            .color(base)
            .hover(move |s| s.color(hover))
    });

    // "Tx open · N stmts" — or why it can't go forward. Hidden when idle.
    let tx_pill = dyn_container(
        move || tx_state.get(),
        move |st| {
            text(schemaic_core::tx::pill_text(st).unwrap_or_default())
                .style(|s| s.font_size(theme::font_status()))
                .into_any()
        },
    )
    .style(move |s| {
        let st = tx_state.get();
        let s = s
            .margin_left(theme::scaled(15.0))
            .items_center()
            .color(match st {
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
                    .margin_left(theme::scaled(15.0))
                    .items_center()
                    .font_size(theme::font_status())
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
        popup_menu,
        popup_anchor,
        popup_width,
        40.0,
    );
    let effort_seg = status_menu_seg(
        move || ai_effort.get().label().to_string(),
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
        popup_menu,
        popup_anchor,
        popup_width,
        15.0,
    );
    let cpu_seg = dyn_container(
        move || resources.get().cpu_label(),
        move |c| footer_text(format!("CPU: {c}")),
    )
    .style(|s| s.margin_left(theme::scaled(40.0)));
    let ram_seg = dyn_container(
        move || resources.get().ram_label(),
        move |r| footer_text(format!("RAM: {r}")),
    )
    .style(|s| s.margin_left(theme::scaled(15.0)));

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
            .height(theme::footer_h())
            .min_height(theme::footer_h())
            .flex_shrink(0.0_f32)
            .flex_row()
            .items_center()
            // Everything in the bar sits 2px high of centre. `items_center`
            // centres within the **content box**, and 4px of bottom padding takes
            // 4px off the bottom of it — moving the centre, and so every icon and
            // label with it, up by exactly half that. Bottom padding rather than a
            // margin on each group because taffy's `height` here is the border
            // box: the bar's own height and the footer's edge don't move, only
            // what is inside it.
            .padding_bottom(footer_lift() * 2.0)
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

/// The Find palette's type size — the palette's own step up from body text.
///
/// **One name because two views must agree to the pixel**: the field draws the
/// query at it, and `overlays`' ghost-completion measures the query at it to place
/// itself right after the last glyph. The ghost measured a literal `16.0` while
/// the field drew `scaled_font(16.0)`, so at 200% it was placed at half the
/// rendered width — on top of the query rather than after it.
pub(crate) fn palette_font() -> f32 {
    theme::scaled_font(16.0)
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
            font_size: palette_font,
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

#[cfg(test)]
mod field_layout_tests {
    use super::{chat_pad_h, placeholder_right_inset, trailing_size};

    /// With nothing beside the editor the overlay spans the content box exactly —
    /// the same edges the text it stands in for starts and ends at.
    #[test]
    fn a_plain_field_bounds_the_placeholder_symmetrically() {
        assert_eq!(placeholder_right_inset(false), chat_pad_h());
    }

    /// With a trailing action it stops clear of it. Asserted as the **property**
    /// — the placeholder can never reach the control — rather than by restating
    /// the sum, which would agree with itself however the margins move.
    #[test]
    fn a_trailing_action_is_never_painted_over() {
        let inset = placeholder_right_inset(true);
        assert!(
            inset >= chat_pad_h() + trailing_size(),
            "{inset} leaves the control uncovered by only part of its width"
        );
    }

    /// And reserving that room is the only thing that widens the gutter: a field
    /// without the action must not pay for it.
    #[test]
    fn a_field_without_the_action_reserves_nothing_for_it() {
        assert!(placeholder_right_inset(false) < placeholder_right_inset(true));
    }
}

#[cfg(test)]
mod result_strip_tests {
    use super::{PanelView, ResultPanel, shown_panel, shown_panel_loaded};
    use floem::reactive::Scope;
    use schemaic_core::model::QueryState;
    use std::sync::Arc;

    fn panel(id: u64, state: QueryState) -> ResultPanel {
        let cx = Scope::new();
        ResultPanel {
            cx,
            id,
            label: format!("Result {id}"),
            sql: String::new(),
            state,
            pinned: false,
            ran_at: 0,
            view: PanelView::new(cx),
            load_gen: 0,
        }
    }

    fn loaded(id: u64) -> ResultPanel {
        panel(
            id,
            QueryState::Loaded(Arc::new(schemaic_core::model::ResultSet::default())),
        )
    }

    /// A selected panel outlives the run that closed it, and every reader falls
    /// back to the first one — the bars, the pane and the AI all have to agree
    /// about which statement they are describing, or one of them reports on a
    /// result nobody is looking at.
    #[test]
    fn a_stale_selection_reads_the_first_panel_as_the_body_does() {
        let panels = vec![panel(1, QueryState::Failed("boom".into())), loaded(2)];
        assert_eq!(shown_panel(&panels, 77).map(|p| p.id), Some(1));
        assert!(!shown_panel_loaded(&panels, 77));
    }

    /// **The panel is found by id, not by position.** Pinning reorders the strip
    /// and a run rewrites it, so an index would silently show a different result
    /// than the one the user clicked — and the fallback above would hide it,
    /// since a wrong index is a *valid* index.
    #[test]
    fn a_panel_is_found_by_its_id_wherever_it_has_moved_to() {
        let panels = vec![loaded(9), panel(4, QueryState::Failed("boom".into()))];
        assert_eq!(shown_panel(&panels, 4).map(|p| p.id), Some(4));
        assert!(shown_panel_loaded(&panels, 9));
    }

    #[test]
    fn an_empty_strip_reports_nothing() {
        assert!(shown_panel(&[], 0).is_none());
        assert!(!shown_panel_loaded(&[], 0));
    }

    /// **The panel-level bars follow the grid that is mounted**, which is the
    /// shown statement's — not "some statement in the batch has a grid". With
    /// `any`, switching from a loaded Result 1 to a failed Result 2 left the
    /// selection summary and the find bar floating over a pane with no grid under
    /// them.
    #[test]
    fn only_the_shown_statement_decides_whether_a_grid_is_mounted() {
        let panels = vec![loaded(1), panel(2, QueryState::Failed("boom".into()))];
        assert!(shown_panel_loaded(&panels, 1));
        assert!(!shown_panel_loaded(&panels, 2));
    }

    /// A pinned panel is a frozen one, which is what every write and re-read
    /// affordance gates on. Stated here rather than left to each call site: the
    /// two questions are the same question, and the first place they came apart
    /// would be a commit against a snapshot.
    #[test]
    fn a_pinned_panel_is_frozen_and_an_ordinary_one_is_not() {
        let mut p = loaded(1);
        assert!(!p.frozen());
        p.pinned = true;
        assert!(p.frozen());
    }
}

/// The strip's rules **as the tab applies them** — `resultsel` is unit-tested on
/// its own, and every bug these guard was in the composition rather than in it.
#[cfg(test)]
mod result_panel_tab_tests {
    use super::Tab;
    use floem::prelude::{SignalGet, SignalUpdate, SignalWith};
    use floem::reactive::Scope;
    use schemaic_core::model::QueryState;

    fn tab() -> Tab {
        Tab::new(Scope::new(), 1, "", 7, None)
    }

    fn ids(t: &Tab) -> Vec<u64> {
        t.result_tabs
            .with_untracked(|v| v.iter().map(|p| p.id).collect())
    }

    /// **What decides whether a new tab silently eats this one.** `place_tab`
    /// reuses a blank tab in place and disposes what was there, so an answer of
    /// `true` over a tab holding a real result destroys it with no undo.
    #[test]
    fn only_an_untouched_tab_reads_as_a_blank_slate() {
        let t = tab();
        assert!(t.results_untouched(), "a tab with nothing run");

        let first = t.begin_run(&["SELECT 1".to_string()])[0];
        // `Running`, not yet landed — and already not a blank slate, which is
        // what puts the strip on screen with the run rather than with its rows.
        assert!(!t.results_untouched(), "a result is in flight here");

        t.set_panel_state(first, QueryState::Failed("boom".into()));
        assert!(!t.results_untouched(), "and a failed one still counts");

        // Back to one idle panel — and *that* is a blank slate again, which is
        // what makes a closed-and-respawned tab reusable.
        t.reset_results();
        assert!(t.results_untouched());

        // A pinned idle panel is not: the user has said they want it kept, and
        // reuse would throw it away.
        let id = t.shown_panel_id().unwrap();
        t.set_pinned(id, true);
        assert!(!t.results_untouched());
    }

    /// The chip's name: the statement, or a positional fallback when there is no
    /// statement to preview. A blank chip is unpointable-at, and whitespace-only
    /// SQL is the input that produces one.
    #[test]
    fn a_chip_is_named_for_its_statement_or_by_its_position() {
        let t = tab();
        let ids = t.begin_run(&["  \n\t ".to_string(), "SELECT 1".to_string()]);
        let labels: Vec<String> = t.result_tabs.with_untracked(|v| {
            ids.iter()
                .filter_map(|id| v.iter().find(|p| p.id == *id).map(|p| p.label.clone()))
                .collect()
        });
        assert_eq!(labels[0], "Result 1", "nothing to preview → its position");
        assert_eq!(labels[1], "SELECT 1");
    }

    #[test]
    fn a_new_tab_shows_one_idle_result() {
        let t = tab();
        assert_eq!(ids(&t).len(), 1, "the strip is never empty");
        assert!(t.results_untouched());
        assert!(matches!(t.shown_result(), QueryState::Idle));
    }

    /// The feature, end to end at the tab: pin a result, run again, and the pin
    /// is still there — with the fresh result shown, not the pin.
    #[test]
    fn a_pinned_result_survives_the_next_run_and_the_run_is_what_is_shown() {
        let t = tab();
        let first = t.begin_run(&["SELECT 1".to_string()])[0];
        t.set_pinned(first, true);
        let second = t.begin_run(&["SELECT 2".to_string()])[0];
        assert_eq!(ids(&t), vec![first, second], "the pin leads the strip");
        assert_eq!(t.shown_panel_id(), Some(second));
        assert!(t.shown_panel_id() != Some(first));
    }

    /// And an *unpinned* one does not — this is the half that makes the pin mean
    /// something, and the half a "keep the last N results" strip would fail.
    #[test]
    fn an_unpinned_result_is_replaced_by_the_next_run() {
        let t = tab();
        let first = t.begin_run(&["SELECT 1".to_string()])[0];
        let second = t.begin_run(&["SELECT 2".to_string()])[0];
        assert_eq!(ids(&t), vec![second]);
        assert!(!ids(&t).contains(&first));
    }

    #[test]
    fn a_batch_opens_one_panel_per_statement_and_shows_the_first() {
        let t = tab();
        let batch = t.begin_run(&["SELECT 1".to_string(), "SELECT 2".to_string()]);
        assert_eq!(batch.len(), 2);
        assert_eq!(ids(&t), batch);
        assert_eq!(t.shown_panel_id(), Some(batch[0]));
    }

    /// A statement's result is written by **id**, so a strip reordered by a pin
    /// while the batch was running still lands each result on its own panel.
    #[test]
    fn a_result_lands_on_its_own_panel_after_the_strip_is_reordered() {
        let t = tab();
        let batch = t.begin_run(&["SELECT 1".to_string(), "SELECT 2".to_string()]);
        t.set_pinned(batch[1], true);
        t.set_panel_state(batch[0], QueryState::Failed("boom".into()));
        let state = t
            .result_tabs
            .with_untracked(|v| v.iter().find(|p| p.id == batch[1]).map(|p| p.state.clone()));
        assert!(
            matches!(state, Some(QueryState::Running)),
            "the second statement's panel took the first's result"
        );
    }

    /// **The bulk writer must answer exactly as the one-at-a-time writer did.**
    /// It exists only to cut the notification count (0.67 ms → 263 ms at 400
    /// statements was the cost of writing one at a time), so any difference in
    /// *outcome* would be a regression bought with performance.
    ///
    /// Both halves of `set_panel_state`'s contract, over a strip that was
    /// pinned, reordered and partly closed while the batch ran: results land by
    /// id, and a closed panel takes nothing.
    #[test]
    fn a_batch_written_at_once_lands_exactly_where_one_at_a_time_would() {
        let t = tab();
        let batch = t.begin_run(&[
            "SELECT 1".to_string(),
            "SELECT 2".to_string(),
            "SELECT 3".to_string(),
        ]);
        // Pin the last, which moves it; close the middle one.
        t.set_pinned(batch[2], true);
        t.close_panels(&[batch[1]]);

        t.set_panel_states([
            (batch[0], QueryState::Failed("one".into())),
            (batch[1], QueryState::Failed("two".into())),
            (batch[2], QueryState::Failed("three".into())),
        ]);

        let state = |id: u64| {
            t.result_tabs
                .with_untracked(|v| v.iter().find(|p| p.id == id).map(|p| p.state.clone()))
        };
        assert!(
            matches!(state(batch[0]), Some(QueryState::Failed(ref m)) if m == "one"),
            "reordering must not move a result"
        );
        assert!(matches!(state(batch[2]), Some(QueryState::Failed(ref m)) if m == "three"));
        assert!(state(batch[1]).is_none(), "a closed panel takes nothing");
    }

    /// **Only a failure puts anything in the editor's error bar** — the
    /// property whose two tests were deleted with `shown_panel_error` when the
    /// decision moved into an inline memo in a file with no test module.
    ///
    /// Every other state is `None`, and the list is written out so a state added
    /// later has to be classified on purpose. Widening it would put text in the
    /// red bar for a run that did not fail, and with it a *View*, an *AI fix*
    /// and an *Explain* acting on an error there isn't one of.
    #[test]
    fn only_a_failed_result_reaches_the_error_bar() {
        assert_eq!(
            super::ShownResult::bar_message(&QueryState::Failed("boom".into())).as_deref(),
            Some("boom")
        );
        assert!(super::ShownResult::bar_message(&QueryState::Idle).is_none());
        assert!(super::ShownResult::bar_message(&QueryState::Running).is_none());
        assert!(super::ShownResult::bar_message(&QueryState::Cancelled).is_none());
        // A `Loaded` result carries rows, not a message — including one that was
        // truncated or that wrote nothing, which are outcomes and not errors.
        let t = tab();
        let id = t.begin_run(&["SELECT 1".to_string()])[0];
        t.set_panel_state(id, QueryState::Failed("boom".into()));
        assert_eq!(
            super::ShownResult::bar_message(&t.shown_result()).as_deref(),
            Some("boom"),
            "and it is the shown panel's own message"
        );
    }

    /// A result landing after its panel was closed goes nowhere — the panel-level
    /// equivalent of the run-generation check, and the reason `set_panel_state`
    /// looks its panel up rather than trusting an index.
    #[test]
    fn a_result_for_a_closed_panel_lands_nowhere() {
        let t = tab();
        let batch = t.begin_run(&["SELECT 1".to_string(), "SELECT 2".to_string()]);
        t.close_panels(&[batch[0]]);
        t.set_panel_state(batch[0], QueryState::Failed("boom".into()));
        assert_eq!(ids(&t), vec![batch[1]]);
    }

    #[test]
    fn closing_every_panel_leaves_one_idle_one() {
        let t = tab();
        let batch = t.begin_run(&["SELECT 1".to_string(), "SELECT 2".to_string()]);
        t.close_panels(&batch);
        assert_eq!(ids(&t).len(), 1);
        assert!(t.results_untouched(), "and it is a blank slate again");
        assert_eq!(t.shown_panel_id(), ids(&t).first().copied());
    }

    /// "Close all" spares the pins, so the strip it leaves is the kept results —
    /// and what is shown has to be one of *them*.
    #[test]
    fn close_all_leaves_the_pins_and_shows_one_of_them() {
        let t = tab();
        let batch = t.begin_run(&["SELECT 1".to_string(), "SELECT 2".to_string()]);
        t.set_pinned(batch[0], true);
        let to_close = schemaic_core::resultsel::all_to_close(
            &t.result_tabs
                .with_untracked(|v| v.iter().map(|p| (p.id, p.pinned)).collect::<Vec<_>>()),
        );
        t.close_panels(&to_close);
        assert_eq!(ids(&t), vec![batch[0]]);
        assert_eq!(t.shown_panel_id(), Some(batch[0]));
    }

    /// A filter re-run rewrites the panel it re-reads rather than opening a new
    /// one — that is what keeps its table on screen — so the statement the panel
    /// reports has to move with it, or a pinned filtered result would describe
    /// itself by the query it no longer holds.
    /// **The regression this is here for**: pinning has to reach a grid that is
    /// already mounted. Pinning moves neither the shown panel's id nor its
    /// phase, so nothing rebuilds — a `bool` sampled where the grid is built
    /// stayed `false`, and the result the user had just pinned went on offering
    /// its editing, its row actions and its filter until they switched away and
    /// back. The answer has to be a value that changes, so this holds the memo
    /// to *changing* rather than to being right once.
    #[test]
    fn pinning_the_shown_result_is_visible_without_anything_being_rebuilt() {
        let t = tab();
        let first = t.begin_run(&["SELECT 1".to_string()])[0];
        let frozen = t.panel_frozen_memo(first);
        assert!(!frozen.get_untracked(), "a fresh result is not frozen");

        t.set_pinned(first, true);
        assert!(
            frozen.get_untracked(),
            "the pin did not reach a reader that was already looking"
        );

        t.set_pinned(first, false);
        assert!(!frozen.get_untracked(), "and unpinning gives it back");
    }

    /// **The crash this is here for.** A grid is built for *one* panel, and its
    /// answer must not move because a different panel became the shown one: that
    /// change arrives in the same update pass that unmounts the grid, so a flip
    /// re-ran its edit-model effect, rebuilt the toolbar's `ai_menu` and had the
    /// new child read a `GridState` signal that had just been disposed. Running a
    /// query with a pin present did exactly that, every time.
    #[test]
    fn a_panels_frozen_answer_ignores_what_the_strip_does_around_it() {
        let t = tab();
        let kept = t.begin_run(&["SELECT 1".to_string()])[0];
        t.set_pinned(kept, true);
        let kept_frozen = t.panel_frozen_memo(kept);
        assert!(kept_frozen.get_untracked());

        // The run that used to crash: a fresh panel appears, the shown panel
        // changes, and the pinned panel's own answer must not move a millimetre.
        let live = t.begin_run(&["SELECT 2".to_string()])[0];
        let live_frozen = t.panel_frozen_memo(live);
        assert!(kept_frozen.get_untracked(), "the pin is still a pin");
        assert!(!live_frozen.get_untracked(), "and the new result is not");

        // Nor when the strip merely switches between them.
        t.active_result.set(kept);
        assert!(kept_frozen.get_untracked());
        assert!(!live_frozen.get_untracked());
    }

    /// And a panel that is *gone* keeps its last answer instead of reporting
    /// `false` on its way out — closing the shown result would otherwise flip it
    /// at teardown, which is the same hazard from the other direction.
    #[test]
    fn a_closed_panels_answer_does_not_change_as_it_leaves() {
        let t = tab();
        let batch = t.begin_run(&["SELECT 1".to_string(), "SELECT 2".to_string()]);
        t.set_pinned(batch[0], true);
        let frozen = t.panel_frozen_memo(batch[0]);
        assert!(frozen.get_untracked());

        t.close_panels(&[batch[0], batch[1]]);
        assert!(
            frozen.get_untracked(),
            "a departing panel's answer changed under the view being unmounted"
        );
    }

    /// Typing dismisses the error bar by clearing the shown panel — which must
    /// not reach a kept one, or the keyboard would be the one way to mutate a
    /// frozen result.
    #[test]
    fn typing_clears_a_live_failure_but_never_a_kept_one() {
        let t = tab();
        let live = t.begin_run(&["SELECT 1".to_string()])[0];
        t.set_panel_state(live, QueryState::Failed("boom".into()));
        t.shown().dismiss_error();
        assert!(matches!(t.shown_result(), QueryState::Idle));

        let kept = t.begin_run(&["SELECT 2".to_string()])[0];
        t.set_panel_state(kept, QueryState::Failed("boom".into()));
        t.set_pinned(kept, true);
        t.shown().dismiss_error();
        assert!(
            matches!(t.shown_result(), QueryState::Failed(_)),
            "a kept failure is a record, not a stale bar"
        );
    }

    #[test]
    fn a_filtered_rerun_restates_its_panel_without_opening_one() {
        let t = tab();
        let first = t.begin_run(&["SELECT * FROM staff".to_string()])[0];
        t.set_panel_sql(first, "SELECT * FROM staff WHERE active = 1");
        assert_eq!(ids(&t), vec![first], "no second panel");
        let sql = t
            .result_tabs
            .with_untracked(|v| v.iter().find(|p| p.id == first).map(|p| p.sql.clone()));
        assert_eq!(sql.as_deref(), Some("SELECT * FROM staff WHERE active = 1"));
    }

    /// **A re-run's rebuild is that panel's**, and it used to be the tab's: one
    /// `load_gen` on the tab meant a filter re-run landing on any panel rebuilt
    /// whichever panel was *shown*, losing that result's scroll position and
    /// selection. The widths, the sort and a frozen column survive a rebuild;
    /// those two do not.
    ///
    /// The other half matters as much: an in-place commit splice deliberately
    /// does **not** bump the nonce, so it still skips the rebuild — getting that
    /// wrong leaves a stale table on screen, which is why the nonce exists.
    #[test]
    fn a_filtered_rerun_rebuilds_its_own_panel_and_no_other() {
        let t = tab();
        let batch = t.begin_run(&["SELECT 1".to_string(), "SELECT 2".to_string()]);
        let load_of = |id: u64| {
            t.result_tabs
                .with_untracked(|v| v.iter().find(|p| p.id == id).map(|p| p.load_gen))
        };
        assert_eq!(load_of(batch[0]), Some(0));
        assert_eq!(load_of(batch[1]), Some(0));

        t.bump_panel_load(batch[1]);
        assert_eq!(load_of(batch[1]), Some(1), "the panel that re-ran");
        assert_eq!(
            load_of(batch[0]),
            Some(0),
            "the panel on screen must not be rebuilt for someone else's re-run"
        );

        // A commit splice writes the panel's state and bumps nothing, so the
        // grid is not rebuilt and the rows are replaced in place.
        t.set_panel_state(batch[1], QueryState::Cancelled);
        assert_eq!(load_of(batch[1]), Some(1), "a state write is not a re-run");

        // A panel that is gone takes no bump, as with every other by-id writer.
        t.close_panels(&[batch[1]]);
        t.bump_panel_load(batch[1]);
        assert_eq!(load_of(batch[1]), None);
    }

    #[test]
    fn a_reset_tab_drops_its_pins_too() {
        let t = tab();
        let first = t.begin_run(&["SELECT 1".to_string()])[0];
        t.set_pinned(first, true);
        t.reset_results();
        assert!(t.results_untouched());
        assert!(!ids(&t).contains(&first));
    }
}

#[cfg(test)]
mod field_key_tests {
    use super::{TabAction, tab_action};

    /// The documented order: an explicit `on_tab` beats everything.
    #[test]
    fn an_explicit_tab_handler_wins() {
        assert_eq!(
            tab_action(true, false, true, false),
            TabAction::Callback,
            "even with a ring"
        );
        assert_eq!(tab_action(true, true, true, false), TabAction::Callback);
    }

    /// Accepting a completion is a forward motion, so a shifted Tab isn't one.
    /// The palette (the only `on_tab` caller) has no ring, so it falls through
    /// to floem.
    #[test]
    fn shift_tab_never_accepts_a_completion() {
        assert_eq!(tab_action(true, false, false, true), TabAction::Insert);
        assert_eq!(tab_action(true, false, true, true), TabAction::StepRing);
    }

    #[test]
    fn a_ringed_field_steps_in_both_directions() {
        assert_eq!(tab_action(false, false, true, false), TabAction::StepRing);
        assert_eq!(tab_action(false, false, true, true), TabAction::StepRing);
    }

    /// A field holding code types the indent — Escape is the way out, and the
    /// ring re-enters after it (`FocusRing::remember`). Shift+Tab is suppressed
    /// too, deliberately: half a step-away would be stranger than none.
    #[test]
    fn a_code_field_types_the_indent_instead_of_leaving() {
        assert_eq!(tab_action(false, true, true, false), TabAction::Insert);
        assert_eq!(tab_action(false, true, true, true), TabAction::Insert);
    }

    #[test]
    fn a_plain_field_outside_any_ring_inserts() {
        assert_eq!(tab_action(false, false, false, false), TabAction::Insert);
        assert_eq!(tab_action(false, true, false, false), TabAction::Insert);
    }

    /// `is_modifier_key` decides whether a keypress **commits** recalled text.
    /// The recall keys are Ctrl+Arrow and the Ctrl arrives as its own key-down
    /// first, so a missing arm would commit the recalled question before the
    /// arrow meant to replace it — with the suite green, since it is a `matches!`
    /// over fourteen variants under no exhaustiveness pressure.
    #[test]
    fn every_modifier_is_recognised_as_typing_nothing() {
        use super::is_modifier_key;
        use floem::keyboard::{Key, NamedKey};
        for k in [
            NamedKey::Alt,
            NamedKey::AltGraph,
            NamedKey::CapsLock,
            NamedKey::Control,
            NamedKey::Fn,
            NamedKey::FnLock,
            NamedKey::Meta,
            NamedKey::NumLock,
            NamedKey::ScrollLock,
            NamedKey::Shift,
            NamedKey::Symbol,
            NamedKey::SymbolLock,
            NamedKey::Super,
            NamedKey::Hyper,
        ] {
            assert!(is_modifier_key(&Key::Named(k)), "{k:?}");
        }
    }

    /// The other half: a key that really does type something must not be taken
    /// for a modifier, or the recalled text would never commit at all.
    #[test]
    fn a_key_that_types_is_not_a_modifier() {
        use super::is_modifier_key;
        use floem::keyboard::{Key, NamedKey};
        assert!(!is_modifier_key(&Key::Character("a".into())));
        assert!(!is_modifier_key(&Key::Character(" ".into())));
        for k in [
            NamedKey::Enter,
            NamedKey::Backspace,
            NamedKey::Delete,
            NamedKey::Space,
            NamedKey::ArrowUp,
            NamedKey::Escape,
            NamedKey::Tab,
        ] {
            assert!(!is_modifier_key(&Key::Named(k)), "{k:?}");
        }
    }
}

#[cfg(test)]
mod panel_toggle_tests {
    use super::{PANEL_NARROW_TIP, panel_toggle};

    #[test]
    fn a_panel_that_fits_and_is_offered_is_live_and_silent() {
        let t = panel_toggle(true, true);
        assert!(t.enabled);
        assert_eq!(t.tip, None, "nothing to explain about a working button");
    }

    #[test]
    fn a_window_too_narrow_disables_the_toggle_and_says_why() {
        let t = panel_toggle(false, true);
        assert!(!t.enabled);
        assert_eq!(t.tip, Some(PANEL_NARROW_TIP));
    }

    /// The pre-existing decision, kept: Server Activity on an engine with no
    /// sessions is dim and says nothing, because the state is permanent and a
    /// toggle that opens an explanation is worse than one visibly not offered.
    #[test]
    fn a_panel_the_engine_does_not_offer_is_disabled_but_silent() {
        let t = panel_toggle(true, false);
        assert!(!t.enabled);
        assert_eq!(t.tip, None);
    }

    /// Both reasons at once — the actionable one is the one shown, because
    /// widening the window is a thing the user can do and changing engines is not.
    #[test]
    fn narrow_speaks_even_when_the_panel_is_also_unoffered() {
        let t = panel_toggle(false, false);
        assert!(!t.enabled);
        assert_eq!(t.tip, Some(PANEL_NARROW_TIP));
    }

    /// **The invariant the single call exists to hold**, over every input: a tip
    /// never accompanies a live toggle. That is the drift worth pinning — a
    /// button that works while insisting the window is too narrow for it.
    #[test]
    fn a_tip_never_appears_on_an_enabled_toggle() {
        for fits in [true, false] {
            for offered in [true, false] {
                let t = panel_toggle(fits, offered);
                assert!(
                    !(t.enabled && t.tip.is_some()),
                    "fits={fits} offered={offered} produced {t:?}"
                );
            }
        }
    }
}

/// **The write guard, at the seam rather than at the function.**
///
/// `sql::script_verdict` had three tests and all three asked the function. The
/// gap was its composition with the launcher — an exhaustive `match` whose
/// `Block` arm returned — so deleting one `return` left the workspace green
/// while a read-only connection ran a whole `.sql` file. That is CLAUDE.md's
/// named shape, reproduced exactly.
///
/// Making `ScriptRequest`'s fields private and its constructor the guard is
/// what moves the property from "the launcher remembers to check" to "there is
/// nothing to run without a check". These tests then pin the constructor, which
/// is now the only door.
#[cfg(test)]
mod script_launch_gate {
    use super::*;
    use schemaic_core::intel::SqlDialect;
    use schemaic_core::sql::GuardPolicy;

    fn policy(read_only: bool, no_database: bool, confirm_writes: bool) -> GuardPolicy {
        GuardPolicy {
            read_only,
            confirm_writes,
            dialect: SqlDialect::MySql,
            no_database,
        }
    }

    fn request(policy: GuardPolicy) -> Result<ScriptRequest, String> {
        ScriptRequest::approved(
            policy,
            std::path::PathBuf::from("/tmp/dump.sql"),
            "dump.sql",
            7,
            "shop".to_string(),
            SqlDialect::MySql,
        )
    }

    /// A script is unconditionally a write, so a read-only connection is
    /// refused *without the file being read* — and there is no override.
    #[test]
    fn a_read_only_connection_cannot_produce_a_runnable_request() {
        let err = request(policy(true, false, false)).expect_err("read-only must refuse");
        assert!(err.to_lowercase().contains("read-only"), "{err}");
    }

    /// And no database refuses outright rather than only when some statement
    /// needs one — see `GuardPolicy::no_database`.
    #[test]
    fn no_database_cannot_produce_a_runnable_request_either() {
        assert!(request(policy(false, true, false)).is_err());
    }

    /// The `Confirm` arm is an `Ok`, and that is a decision this app has
    /// written down: the panel is the confirmation. What must not happen is the
    /// arm being reached silently.
    #[test]
    fn an_ordinary_connection_produces_a_request_carrying_what_it_was_given() {
        let r = request(policy(false, false, false)).expect("a plain connection may run a script");
        assert_eq!(r.path(), std::path::Path::new("/tmp/dump.sql"));
        assert_eq!(r.conn_id(), 7);
        assert_eq!(r.database(), "shop");
        assert_eq!(r.dialect(), SqlDialect::MySql);
    }

    /// **The property the whole shape exists for**, asserted over every policy
    /// rather than the two above: a request exists exactly when the verdict is
    /// not a `Block`. A caller cannot widen this, because a caller cannot build
    /// a `ScriptRequest` at all.
    #[test]
    fn a_request_exists_exactly_when_the_verdict_is_not_a_block() {
        for read_only in [false, true] {
            for no_database in [false, true] {
                for confirm_writes in [false, true] {
                    let policy = policy(read_only, no_database, confirm_writes);
                    let blocked = matches!(
                        schemaic_core::sql::script_verdict(policy, "dump.sql"),
                        schemaic_core::sql::RunVerdict::Block(_)
                    );
                    assert_eq!(
                        request(policy).is_err(),
                        blocked,
                        "{policy:?} disagreed with its own verdict"
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod export_target_tests {
    use super::*;
    use schemaic_core::export::ExportFormat;

    fn target(format: ExportFormat) -> DumpTarget {
        DumpTarget {
            conn_id: 1,
            database: "sakila".to_string(),
            schema: None,
            dialect: SqlDialect::MySql,
            format,
        }
    }

    /// The one question the Export modal asks about itself, and it decides six
    /// things — the title, whether the dump's options are built, which dialog
    /// opens, which writer launches, and how the outcome is worded. It is a
    /// predicate rather than a `format == Sql` at each of those, because six
    /// spellings of one comparison are six chances for one to disagree.
    ///
    /// Stated over **every** format rather than over a sample, so a seventh has
    /// to answer it: the failure mode is a new format silently falling onto
    /// whichever side of a comparison it happens to land on, which is the same
    /// hazard CLAUDE.md's "ask a capability, never an engine" rule names.
    #[test]
    fn only_sql_writes_a_single_file() {
        for f in ExportFormat::ALL {
            assert_eq!(
                target(f).writes_folder(),
                f != ExportFormat::Sql,
                "{} is on the wrong side of the split",
                f.label()
            );
        }
        // Spelled out, because these two are the whole point: SQL is the dump,
        // and the rest are folders.
        assert!(!target(ExportFormat::Sql).writes_folder());
        assert!(target(ExportFormat::Csv).writes_folder());
    }
}

#[cfg(test)]
mod window_key_gate {
    use std::path::Path;

    /// **A window-level key handler has to be on the view the app returns.**
    ///
    /// Floem delivers a `KeyDown` to the focused view; if nothing consumes it, the
    /// only other place it goes is `WindowHandle::main_view.apply_event(…)`, and
    /// `ViewId::apply_event` reads the listeners on **that one id** — it does not
    /// walk children. `main_view` is exactly what the app's view function returned.
    /// So a `KeyDown` listener attached to anything *inside* that view is never
    /// reached: not by the focus path, which dispatches downward from the focused
    /// view and never up through its ancestors, and not by the fallback.
    ///
    /// That is not hypothetical. `69fd7aa` wrapped `workspace`'s root in an outer
    /// stack for the eight window resize zones and left this listener on the inner
    /// `root`, which silently moved it off `main_view` — and every branch in it
    /// went dead whenever focus was outside the SQL editor: Escape closing an open
    /// dropdown, the Tab-trap backstop, `NavKeys` (Ctrl+P, Ctrl+Shift+P, Ctrl+T/W,
    /// Ctrl+Tab, Ctrl+1..9, Ctrl+O/S) and the three panel toggles. It looked fine
    /// because `editor_pane` answers the same keys itself, and the editor usually
    /// has focus. It survived a release.
    ///
    /// The check is deliberately crude — where the listener sits relative to the
    /// resize zones, which are built from `chrome.resize_zones()` on the line
    /// before the returned stack. A precise one would need to parse the builder
    /// chain; this one costs nothing and fails on exactly the mistake that was
    /// made.
    #[test]
    fn the_window_key_handler_is_on_the_view_workspace_returns() {
        let src = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join("lib.rs"),
        )
        .expect("lib.rs");
        // This module quotes the names it looks for, so cut the tests off —
        // through `source_gate`, which cuts each `#[cfg(test)]` **item** rather
        // than everything after the first one. The positional cut broke the
        // moment a `#[cfg(test)] mod source_gate;` was declared near the top of
        // this file: the gate then read 45 lines and failed with "`workspace` is
        // gone", which is precisely the silent-blind-spot failure mode inverted
        // into a loud one.
        let body = crate::source_gate::production_code(&src);
        let body = body.as_str();
        let at = body
            .find("pub fn workspace(")
            .expect("`workspace` is gone — this gate is stale");
        let end = body[at..]
            .find("\n}")
            .expect("`workspace`'s end — this gate is stale");
        let f = &body[at..at + end];

        let zones = f
            .find("chrome.resize_zones()")
            .expect("the resize zones are gone — this gate is stale");
        let keys = f
            .find("EventListener::KeyDown")
            .expect("the window key handler is gone — this gate is stale");

        assert!(
            keys > zones,
            "`workspace`'s KeyDown listener is attached before the resize zones are \
             built, which means it is on the inner root rather than on the stack \
             `workspace` returns. Floem hands an unconsumed key to the returned \
             view's own listeners and to nothing else, so every branch in that \
             handler is dead whenever focus is outside the SQL editor — which \
             answers the same keys itself and hides it."
        );

        // **And it refuses to act while a modal is up.** The handler is only
        // reachable at all because it is on the returned view, and a modal's
        // focus root consumes nothing but Tab — so without this term Ctrl+W
        // mid-confirm overwrote the single-slot `Confirm` and dropped
        // `close_tabs_seq`'s continuation, and Ctrl+P mounted Find Anywhere
        // invisibly *behind* the open modal with its autofocusing field taking
        // the keyboard.
        //
        // The position is the assertion, not the presence: the Escape and Tab
        // branches above it are modal-aware by design and must keep running, so
        // the guard has to sit after them and before `navkeys.handle`.
        let guard = f
            .find("if modal_up() {")
            .expect("the modal guard is gone — Ctrl+W now closes a tab mid-confirm");
        let nav = f
            .find("navkeys.handle(")
            .expect("`navkeys.handle` is gone — this gate is stale");
        let tab_trap = f
            .find("innermost_ring_root()")
            .expect("the Tab-trap backstop is gone — this gate is stale");
        assert!(
            guard < nav,
            "the modal guard must come before `navkeys.handle`, or the workspace \
             shortcuts fire from behind a modal backdrop"
        );
        assert!(
            guard > tab_trap,
            "the modal guard must come *after* the Escape and Tab branches: those \
             two exist to serve the modal that is up, and an early return above \
             them takes the modal's own focus ring and popup dismissal with it"
        );
    }

    /// The invariant is *"the KeyDown listener is on the view the app's view
    /// function returned"*, and that function is `schemaic-app`'s `app_view` —
    /// not `ui::workspace`. The gate above asserts a position **inside**
    /// `workspace`, which is necessary and not sufficient: wrapping
    /// `workspace()` in a `container(...)` over in `schemaic-app` — exactly
    /// `69fd7aa`'s original mistake, one crate over — puts the handler back on an
    /// inner view with this gate still green.
    #[test]
    fn the_app_returns_the_workspace_itself_and_does_not_wrap_it() {
        let src = crate::source_gate::crate_sources()
            .into_iter()
            .find(|(n, _)| n == "schemaic-app/main.rs")
            .map(|(_, code)| code)
            .expect("schemaic-app's main.rs");
        let lines: Vec<&str> = src.lines().collect();
        let at = lines
            .iter()
            .position(|l| l.contains("schemaic_ui::workspace("))
            .expect("`app_view` no longer builds the workspace — this gate is stale");
        // The call has to *open* its line: `container(schemaic_ui::workspace(…))`
        // is a wrapper, and a wrapper moves the listener off the returned view.
        assert!(
            lines[at]
                .trim_start()
                .starts_with("schemaic_ui::workspace("),
            "`workspace()` is not the head of its expression ({}). Floem hands an \
             unconsumed key to the returned view's own listeners and to nothing \
             else, so the window key handler inside `workspace` goes dead — and \
             this gate's sibling stays green, because that one only looks inside \
             `schemaic-ui`.",
            lines[at].trim()
        );
        // …and the expression it heads must not itself be an argument, which is
        // what a wrapper broken across lines looks like.
        let prev = lines[..at]
            .iter()
            .rev()
            .map(|l| l.trim())
            .find(|l| !l.is_empty() && !l.starts_with("//"))
            .unwrap_or("");
        assert!(
            !prev.ends_with('(') && !prev.ends_with("(("),
            "`workspace()` is the argument of `{prev}` — same failure as above, \
             written across two lines."
        );
    }
}
