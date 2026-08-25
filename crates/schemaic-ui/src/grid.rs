//! The results grid: the `scroll(virtual_stack(...))` data table built per result
//! set around the `Copy` bundle of signals `GridState`. Covers the frozen/data
//! two-pane layout, per-column widths + resize, selection + keyboard nav, sorting,
//! per-column freeze, the value viewer, inline write-back edit (`start_edit` →
//! `commit_grid`), CSV/JSON/SQL export, key-icon mapping, and the header/cell
//! right-click menus. `GridState`/`GridCtx` are the shared bundles; `results_view`
//! and `loaded_view` are the entry points wired into `results_section`. The pure
//! export/edit logic lives in `schemaic_core::{export, edit}`; this keeps thin
//! wrappers over the grid's live state.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use floem::AnyView;
use floem::event::{Event, EventListener, EventPropagation};
use floem::keyboard::{Key, NamedKey};
use floem::kurbo::{Point, Rect};
use floem::prelude::*;
use floem::reactive::{Memo, create_effect, create_memo};
use floem::style::CursorStyle;
use floem::views::{VirtualDirection, VirtualItemSize, VirtualVector};

use floem::action::save_as;
use floem::file::{FileDialogOptions, FileSpec};

use schemaic_core::celledit::{self, CellEditor};
use schemaic_core::connection::{AiData, Connection};
use schemaic_core::edit::{EditModel, analyze_edit, refetch_key, refetch_template, row_key};
use schemaic_core::export::{ExportFormat, suggested_filename};
use schemaic_core::filter::{FilterError, build_query, eq_condition, rerun_statement};
use schemaic_core::format::{self, ColumnFormat, ColumnFormatRule};
use schemaic_core::intel::SqlDialect;
use schemaic_core::jsontree::{JsonNode, PathSeg, RowKind, TreeRow};
use schemaic_core::model::{
    CellRef, CellTag, CommitDone, GridWrite, QueryState, RefetchRequest, RefetchRow, ResultSet,
    RowDelete, RowEdit, RowInsert, Value, drop_committed,
};
use schemaic_core::rowjson::{self, ColSpec};
use schemaic_core::schema::{DbSchema, ForeignKeyInfo, SchemaState, TableInfo, TableSource};
use schemaic_core::summary;
use schemaic_core::text::{hides_detail, plural};
use schemaic_core::text_ops::contains_ignore_ascii_case;
use schemaic_core::tx::{WRITE_WAIT_MS, WaitNote, write_wait_note};

use crate::consts::*;
use crate::widgets::{
    MenuEntry, autohide, autohide_state, centered_msg, in_strip_button, loading_dots,
    measure_text_px, shift_hscroll, thin_scroll, toolbar_icon, verb_spinner,
};
use crate::{ConnNode, FieldCfg, PopupAnchor, cell_editors, edit_field, icons, theme};

// ===== moved from lib.rs (results grid) =====
/// The lifecycle phase of a [`QueryState`], without its payload — a deduped key
/// for the results container so an Arc-only change (an inline-edit splice) doesn't
/// rebuild the grid.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Running,
    Loaded,
    Failed,
    Cancelled,
}

fn phase_of(qs: &QueryState) -> Phase {
    match qs {
        QueryState::Idle => Phase::Idle,
        QueryState::Running => Phase::Running,
        QueryState::Loaded(_) => Phase::Loaded,
        QueryState::Failed(_) => Phase::Failed,
        QueryState::Cancelled => Phase::Cancelled,
    }
}

pub(crate) fn results_view(
    results: RwSignal<QueryState>,
    cancel: Rc<dyn Fn()>,
    gctx: GridCtx,
) -> impl IntoView {
    // Key the container on the *phase* + a fresh-load nonce (a deduped Memo), not
    // the whole QueryState. A commit splice replaces the loaded Arc (Loaded→Loaded)
    // *without* bumping the nonce — the key is unchanged, so the grid is NOT rebuilt
    // and scroll/selection survive. A real query (…→Running→Loaded) changes the
    // phase, and a filter/sort re-run (Loaded→Loaded) bumps the nonce; both rebuild.
    let load_gen = gctx.load_gen;
    let phase = create_memo(move |_| (results.with(phase_of), load_gen.get()));
    // Splice sink handed to the grid: replace the canonical result set in place.
    // The phase Memo dedups, so this Loaded→Loaded set doesn't rebuild the grid;
    // it only refreshes the canonical for a later rebuild (tab switch away/back).
    let sync: Rc<dyn Fn(Arc<ResultSet>)> =
        Rc::new(move |rs: Arc<ResultSet>| results.set(QueryState::Loaded(rs)));
    dyn_container(
        move || phase.get(),
        move |(ph, _gen)| match ph {
            Phase::Idle => centered_msg("Run a query  (Ctrl+Enter)", theme::text_muted).into_any(),
            Phase::Running => running_view(cancel.clone()).into_any(),
            // The error text now lives in the editor's error bar (with View /
            // AI Fix), so Results just notes the failure.
            Phase::Failed => centered_msg("Query failed.", theme::text_dim).into_any(),
            Phase::Cancelled => centered_msg("Query cancelled.", theme::text_dim).into_any(),
            Phase::Loaded => {
                // The Arc is read untracked — the phase Memo, not the Arc, drives
                // rebuilds; a splice updates the grid's live `rs` + this canonical.
                let QueryState::Loaded(rs) = results.get_untracked() else {
                    return empty().into_any();
                };
                let mut gctx = gctx.clone();
                gctx.sync_canonical = Some(sync.clone());
                loaded_view(rs, gctx)
            }
        },
    )
    .style(|s| {
        s.flex_grow(1.0_f32)
            .width_full()
            .flex_col()
            .min_height(0.0)
            .min_width(0.0)
    })
}

// "Running query…" with a Cancel button (kills the query server-side).
pub(crate) fn running_view(_cancel: Rc<dyn Fn()>) -> impl IntoView {
    // Just the verb spinner now (the Cancel button was removed); `_cancel` is kept
    // in the signature so callers/plumbing are unchanged.
    container(verb_spinner(theme::text_dim, theme::font_body)).style(|s| {
        s.flex_grow(1.0_f32)
            .width_full()
            .flex_col()
            .items_center()
            .justify_center()
    })
}

/// Row source for the virtual stack: just indices (`usize`). Zero per-row
/// data; the view fn indexes into the shared `Arc<ResultSet>`.
struct RowRange {
    len: usize,
}

impl VirtualVector<usize> for RowRange {
    fn total_len(&self) -> usize {
        self.len
    }
    fn slice(&mut self, range: Range<usize>) -> impl Iterator<Item = usize> {
        range
    }
}

/// Current sort of the grid: `(column index, ascending)`, or `None` for the
/// original (query) order.
type SortState = Option<(usize, bool)>;

/// Cycle a column's sort: unsorted/other → ASC → DESC → unsorted.
fn cycle_sort(sort: RwSignal<SortState>, ci: usize) {
    sort.update(|s| {
        *s = match *s {
            Some((c, true)) if c == ci => Some((ci, false)),
            Some((c, false)) if c == ci => None,
            _ => Some((ci, true)),
        };
    });
}

/// A precomputed sort key for one cell: whether it's NULL, its numeric value if
/// it's a numeric-*tagged* cell, and its (borrowed, arena-slice) text. Built once
/// per row so a sort parses each numeric cell a single time (O(n)) instead of on
/// every comparison (O(n log n)) — see [`compute_order`].
struct SortKey<'a> {
    null: bool,
    num: Option<f64>,
    text: &'a str,
}

/// A stable permutation of row indices for the given sort (identity when
/// `None`). Nulls sort last; two numeric cells compare numerically; anything
/// else compares by displayed text — same ordering as a per-pair comparison,
/// but with the per-cell key parsed once up front (decorate-sort).
fn compute_order(rs: &ResultSet, sort: SortState) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..rs.row_count()).collect();
    if let Some((c, asc)) = sort {
        // Decorate: one key per row (each numeric cell parsed exactly once).
        let keys: Vec<SortKey> = (0..rs.row_count())
            .map(|r| match rs.cell(r, c) {
                Some(cell) => SortKey {
                    null: cell.is_null(),
                    num: cell_num(cell),
                    text: cell.text(),
                },
                None => SortKey {
                    null: true,
                    num: None,
                    text: "",
                },
            })
            .collect();
        idx.sort_by(|&a, &b| {
            let o = cmp_key(&keys[a], &keys[b]);
            if asc { o } else { o.reverse() }
        });
    }
    idx
}

/// Numeric view of a cell for sorting — only for numeric-*tagged* cells (a `Str`
/// that happens to look like a number is compared as text, matching the old
/// `Value`-variant gate). Parses the arena text to `f64`.
fn cell_num(c: CellRef) -> Option<f64> {
    match c.tag {
        CellTag::Int | CellTag::UInt | CellTag::Float => c.text().parse::<f64>().ok(),
        _ => None,
    }
}

/// Compare two precomputed [`SortKey`]s: NULLs sort last; two numeric cells
/// compare numerically; anything else compares by text.
fn cmp_key(a: &SortKey, b: &SortKey) -> Ordering {
    match (a.null, b.null) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => match (a.num, b.num) {
            (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
            _ => a.text.cmp(b.text),
        },
    }
}

// A Loaded result renders either the data grid or, for a statement that
// returned no result set (UPDATE/INSERT/DELETE/DDL), an "N rows affected" line.
pub(crate) fn loaded_view(rs: Arc<ResultSet>, gctx: GridCtx) -> AnyView {
    match rs.affected {
        Some(n) => {
            let s = if n == 1 { "" } else { "s" };
            centered_msg(
                format!("{n} row{s} affected · {} ms", rs.elapsed_ms),
                theme::text_dim,
            )
            .into_any()
        }
        None => grid_view(rs, gctx).into_any(),
    }
}

// ── Data grid: interactive state, sizing, selection, export ─────────────────

/// The AI-summary callback: reveals the AI panel and sends a prompt for a cell.
type SummarizeFn = Rc<dyn Fn(String)>;
/// Splice sink: replace the tab's canonical result set after an in-place commit.
type SyncCanonicalFn = Rc<dyn Fn(Arc<ResultSet>)>;
/// "Follow foreign key" callback: open the referenced table in a new tab running
/// the given filter `sql`. The grid builds the SQL from a FK + row.
type FollowFn = Rc<dyn Fn(TableSource, String)>;
/// Re-run the active tab with a rewritten (filtered/sorted) statement — the
/// server-side filter/sort callback (`TabsActions::apply_view`).
type ApplyViewFn = Rc<dyn Fn(String)>;
/// Staged cell edits grouped `(table_idx, data_row) → [(result_ci, new_value)]`,
/// ordered (BTreeMap) so a failing commit reproduces identically.
type EditGroups = BTreeMap<(usize, usize), Vec<(usize, Option<String>)>>;

/// Per-result interactive grid state. `Copy` (every field is an `RwSignal`, which
/// is `Copy`) so it threads freely into the many cell/handler closures. Created
/// once per result set and shared across sort rebuilds. Selection is tracked in
/// *display* coordinates (`(display_row, col)`) so it stays put visually on sort;
/// `order` carries the active display→data-row permutation for copy/export/viewer.
#[derive(Clone, Copy)]
struct GridState {
    rs: RwSignal<Arc<ResultSet>>,
    order: RwSignal<Arc<Vec<usize>>>,
    widths: RwSignal<Vec<f64>>,
    /// The `grid_char_w()` the stored [`GridState::widths`] were measured against.
    ///
    /// They are pixels, and pixels do not follow the interface scale on their own
    /// — floem calls a `dyn_container`'s builder outside the effect that wraps its
    /// key, so `init_widths`' read subscribes nothing and the grid has no scale
    /// term in its key. This is what lets the change be *carried* instead:
    /// `rescale_widths` needs the ratio between the width a character used to
    /// take and the width it takes now, and only the widths themselves know the
    /// first half of that.
    widths_at: RwSignal<f64>,
    active: RwSignal<Option<(usize, usize)>>,
    anchor: RwSignal<Option<(usize, usize)>>,
    /// The frozen column: its *absolute* index, pinned to the left of the grid
    /// (`None` = nothing frozen). Set from the header right-click menu.
    frozen: RwSignal<Option<usize>>,
    scroll_to: RwSignal<Option<Point>>,
    vp: RwSignal<Rect>,
    focus_id: RwSignal<Option<floem::ViewId>>,
    /// The cell currently open for inline editing (display coords).
    edit_cell: RwSignal<Option<(usize, usize)>>,
    /// Live buffer for the in-progress edit.
    edit_buf: RwSignal<String>,
    /// Staged edits keyed by `(data_row, col)` → new text. Applied to the DB
    /// only on an explicit commit (Ctrl+Enter / the toolbar ✓).
    /// `Some(text)` sets a new value; `None` stages a SQL `NULL`.
    dirty: RwSignal<HashMap<(usize, usize), Option<String>>>,
    /// Staged new rows (the "+ Row" button), each a map of result-column index →
    /// value (`Some` = value, `None` = SQL NULL; absent = server default). They
    /// render below the real rows (display index `nrows + pending_index`) and
    /// `INSERT` on commit. Cleared on commit / discard.
    new_rows: RwSignal<Vec<HashMap<usize, Option<String>>>>,
    /// Data-row indices marked for deletion (the toolbar count + a red row tint);
    /// they `DELETE` on commit. Cleared on commit / discard.
    del_rows: RwSignal<HashSet<usize>>,
    /// True between a cell's pointer-down and the release that ends it — the
    /// drag-select gate. Each cell's `PointerEnter` extends the range while it is
    /// set, so no pointer capture is needed. Cleared by the body's `PointerUp`
    /// **and** by a double-click, since floem's `DoubleClick` swallows the second
    /// `PointerUp` and the flag would otherwise stay armed with no button down.
    selecting: RwSignal<bool>,
    /// The same gate for a drag down the **gutter**, kept separate on purpose:
    /// one shared flag would let a row drag that wandered over a data cell
    /// collapse to that cell's column, which is the opposite of what dragging
    /// row numbers asks for. Cleared by the same pointer-up effect.
    row_selecting: RwSignal<bool>,
    /// Which columns are editable + each base table's WHERE key (from the
    /// result's per-column provenance). Computed once per result set.
    edit_model: RwSignal<Arc<EditModel>>,
    /// Which **control** each result column's values are edited with — a boolean
    /// track, an enum menu, a `SET`'s chips, a calendar, or plain text. Resolved
    /// from the column's *declared* type (see `column_editors`) and kept in a
    /// signal because that type comes from the schema, which may land after the
    /// grid is already on screen.
    editors: RwSignal<Rc<Vec<CellEditor>>>,
    /// True while a commit is executing (disables re-entry).
    commit_busy: RwSignal<bool>,
    /// Bumped per commit, so a wait-note timer armed for an earlier one is
    /// recognisable as stale when it fires during a later one.
    commit_seq: RwSignal<u64>,
    /// Set once the in-flight commit has been outstanding long enough to be
    /// worth explaining (see [`write_wait_note`]); rendered in the same bottom
    /// bar as `commit_err`, cleared when the write returns.
    commit_wait: RwSignal<Option<WaitNote>>,
    /// Which of the user's other tabs hold an open transaction that this tab's
    /// write could be queued behind — the wait note's subject.
    tx_holders: RwSignal<Option<TxHoldersFn>>,
    /// Last commit error, shown in the toolbar until the next edit/commit.
    commit_err: RwSignal<Option<String>>,
    /// The same bar's **note** surface — see [`GridCtx::commit_note`]. Cleared
    /// alongside `commit_err` by [`GridState::clear_bar`].
    commit_note: RwSignal<Option<String>>,
    /// Ui-level popup-menu signal, for the header/cell right-click menus.
    popup: RwSignal<Option<Vec<MenuEntry>>>,
    /// Anchor for the popup: `Some(PopupAnchor::BelowIcon(..))` opens it under a
    /// toolbar icon (the Copy dropdown); `None` opens at the cursor.
    popup_anchor: RwSignal<Option<PopupAnchor>>,
    /// `min_width` of the next popup panel; the Copy dropdown sets it so a stale
    /// width from a prior (narrower) menu can't shrink it.
    popup_width: RwSignal<f64>,
    /// Every menu flag in the app — the channel a picker fills (`menus.popup`),
    /// the calendar's (`menus.date_pick`), and the rest, which a trigger that
    /// swallows its own press has to close itself.
    menus: crate::widgets::MenuFlags,
    /// The pointer in window coords — read only by [`reclaim_keyboard`].
    last_mouse: RwSignal<(f64, f64)>,
    /// The result's source `(database, table)` — for the cell "AI Summary" context.
    source: RwSignal<Option<TableSource>>,
    /// Callbacks wrapped in signals so `GridState` stays `Copy`. `summarize`
    /// reveals the AI panel + sends a message; `dismiss` closes any open menu;
    /// `commit` executes staged edits.
    summarize: RwSignal<Option<SummarizeFn>>,
    /// Stage result rows for the AI panel's next question (see
    /// [`crate::AttachFn`]) — the "Attach to chat" menu actions.
    attach: RwSignal<Option<crate::AttachFn>>,
    dismiss: RwSignal<Option<Rc<dyn Fn()>>>,
    commit: RwSignal<Option<crate::CommitFn>>,
    /// Writes an export to disk on a worker thread (see [`crate::ExportFn`]).
    export_file: RwSignal<Option<crate::ExportFn>>,
    /// The connection this result was **loaded on**, snapshotted at build.
    ///
    /// A tab can be rebound to another connection while a result stays on
    /// screen, and the live `conn_id` moves with it. The export re-runs the
    /// statement the rows came from, so it must re-run it where they came from —
    /// the same argument `ResultSet::database` already makes for the scope, and
    /// the two have to agree or the export runs a statement against a server
    /// that never saw it.
    conn_at_load: u64,
    /// True while a **streamed** export is running — the affordance that offers
    /// to stop it. Panel-level rather than per-result: an export outlives the
    /// grid it was started from (a re-run replaces the `GridState`), and a Cancel
    /// that vanished the moment the rows refreshed would be a Cancel the user
    /// cannot reach for exactly as long as it matters.
    exporting: RwSignal<Option<ExportRun>>,
    /// The tab this grid belongs to — the other half of [`ExportRun`], and the
    /// only thing that tells "an export is running" from "an export is running
    /// *here*".
    tab_id: usize,
    /// Update the tab's *canonical* result set after a splice, so a later grid
    /// rebuild (tab switch away/back) reflects the committed values. `None` for the
    /// multi-result path, which stays on the full-re-run commit flow. Present ⇒ the
    /// grid attempts the splice optimization.
    sync_canonical: RwSignal<Option<SyncCanonicalFn>>,
    /// Per-column display formatter (by absolute column index; `None` = raw),
    /// seeded from the persisted rules and updated by the header "Format as" menu.
    formats: RwSignal<Vec<ColumnFormat>>,
    /// This tab's connection id (keys formatter rules with `source`).
    conn_id: RwSignal<u64>,
    /// Saved connections, for this result's AI data-access level (`ai_data_of`).
    /// Held as the signal rather than resolved once at build, so locking a
    /// connection down takes effect on the grid already on screen.
    connections: RwSignal<Vec<Connection>>,
    /// This tab's SQL dialect (from its connection's engine) — used to build
    /// engine-correct SQL for grid actions like Follow-FK.
    dialect: SqlDialect,
    /// This tab's commit mode — see [`GridCtx::tx_mode`]. The export menu's, and
    /// held as the signal rather than resolved at build because the mode is
    /// toggled from the footer while the grid stays mounted.
    tx_mode: RwSignal<schemaic_core::tx::TxMode>,
    /// Server-side filter/sort: the base SQL to splice into, the active
    /// filter/sort state (persists across result reloads), and the re-run callback
    /// (wrapped so `GridState` stays `Copy`). See `schemaic_core::filter`.
    base_sql: RwSignal<Option<String>>,
    grid_query: RwSignal<schemaic_core::filter::GridQuery>,
    /// A one-off, per-tab row cap the capped notice's read-more action sets; the
    /// app's run path reads it in place of the global setting.
    row_cap_override: RwSignal<Option<usize>>,
    apply_view: RwSignal<Option<ApplyViewFn>>,
    /// A filter/sort error — a bad WHERE fragment / un-rewritable base (client-side)
    /// or a live DB error from the re-run (tab-level). Rendered in the grid's bottom
    /// bar; cleared on any table click (`dismiss_overlays`) or a new run.
    view_err: RwSignal<Option<String>>,
    /// App-wide formatter-rule store (upserted + persisted on a menu choice).
    fmt_rules: RwSignal<Vec<ColumnFormatRule>>,
    /// Persist the formatter rules (wrapped so `GridState` stays `Copy`).
    save_formats: RwSignal<Option<Rc<dyn Fn()>>>,
    /// In-grid find (Ctrl+F): the bar's open state and its query. Match counts
    /// live in `GridCtx` (written by `grid_view`, read by the panel-level bar).
    find_open: RwSignal<bool>,
    find_query: RwSignal<String>,
    /// Go to row (Ctrl+G): the popup's open state, its buffer, and the submit
    /// nonce it bumps on Enter. Same split as find — the popup renders at the
    /// panel level, and `grid_view` does the jump because only it knows how many
    /// rows there are.
    goto_open: RwSignal<bool>,
    goto_query: RwSignal<String>,
    goto_step: RwSignal<u64>,
    /// Per result-column foreign-key "follow" specs (keyed by result-column index;
    /// a composite FK maps each member column to the same spec). Populated by
    /// `grid_view` from the source table's schema; empty when there's nothing to
    /// follow. Read by the cell menu to offer "Follow".
    follow: RwSignal<Rc<HashMap<usize, Rc<FollowSpec>>>>,
    /// Open the referenced table filtered to the followed row (wrapped so
    /// `GridState` stays `Copy`).
    follow_fk: RwSignal<Option<FollowFn>>,
    /// AI-fill a single cell (wrapped so `GridState` stays `Copy`).
    ai_fill: RwSignal<Option<crate::AiFillFn>>,
    /// AI-generate seed rows (wrapped so `GridState` stays `Copy`).
    ai_seed: RwSignal<Option<crate::AiSeedFn>>,
    /// True while an AI seed-data action is in flight — disables the sparkle icon
    /// and runs the pulse clock (`ai_pulse`).
    ai_busy: RwSignal<bool>,
    /// Real-row cells `(data_idx, col)` currently being AI-generated — painted with
    /// the pulsing purple "generating" wash (Fill Value).
    ai_gen: RwSignal<HashSet<(usize, usize)>>,
    /// Pending new-row indices currently being AI-generated — the whole row pulses
    /// purple (Insert Row / Seed Table).
    ai_gen_rows: RwSignal<HashSet<usize>>,
    /// Bumped whenever the pending rows are thrown away wholesale, so an
    /// in-flight AI seed can tell that the indices it captured no longer mean
    /// what they meant. `add_new_row` re-allocates from zero, and discard isn't
    /// blocked during a generation — so discarding mid-generation and adding a
    /// fresh row landed the reply's values on top of what the user was typing.
    new_rows_gen: RwSignal<u64>,
    /// Pulse phase (radians) advanced by a ~45ms tick while `ai_busy`; the
    /// generating cells read it to breathe their wash. `pulse_running` guards
    /// against starting a second tick loop.
    ai_pulse: RwSignal<f64>,
    pulse_running: RwSignal<bool>,
    /// "AI Seed Table…" count popover: whether it's open + its (text) row count.
    seed_open: RwSignal<bool>,
    seed_buf: RwSignal<String>,
    /// "Edit Row" structured panel: open flag, the target data-row, an inline
    /// validation/commit error, and an in-flight guard. Per-field editor state is
    /// local to the panel view (rebuilt per row). Commits immediately on Save (its
    /// own path, not the staged `dirty` batch).
    edit_row_open: RwSignal<bool>,
    edit_row_di: RwSignal<Option<usize>>,
    /// True while this panel's Save is in flight. Its *errors* have no signal of
    /// their own — they share `commit_err`, the panel-level bar (see the status
    /// line in `edit_row_panel`).
    edit_row_saving: RwSignal<bool>,
}

/// Which streamed export is running, and which tab launched it.
///
/// Two questions that used to be one `bool`, and each of them had a failure.
///
/// **`tab`** is why the flag can outlive a tab switch. The export's cancellation
/// token is app-global (`main::export_token`), and the flag was created inside the
/// per-active-tab-render `GridCtx`: switching away disposed it, switching back
/// built a fresh `false`, and the running export was left with no Cancel anywhere
/// on screen while the token stayed occupied — so it could be neither stopped nor
/// replaced, for as long as the stream ran. The signal is created once for the
/// window now, and the tab it names is what decides whose bar draws the Cancel.
///
/// **`id`** is why only the export that raised the flag can lower it. The tail of
/// a finished request cleared the flag whenever *its own* scope was streamed —
/// which is true of a second `All rows` request, the only kind the app refuses.
/// Asking for one while a stream was running therefore tore down the running
/// export's Cancel, which is the same failure the `streaming` guard had been added
/// to fix, one case out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ExportRun {
    id: u64,
    tab: usize,
}

impl ExportRun {
    /// A fresh run for `tab`. The id is monotonic per process, so no two requests
    /// can claim each other's flag — and it is a counter rather than a token
    /// clone because the comparison is all that is wanted, and `ExportRun` has to
    /// stay `Copy` for `BarSignals`' sake.
    fn new(tab: usize) -> ExportRun {
        thread_local! {
            static NEXT: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
        }
        let id = NEXT.with(|n| {
            let v = n.get();
            n.set(v + 1);
            v
        });
        ExportRun { id, tab }
    }
}

/// What the flag should read once `finished` has reported.
///
/// `None` only when the run that reported is the run the flag is holding. Every
/// other case leaves it exactly as it was — which covers both of the failures
/// this replaced: a `Fetched` save (which raised no flag, so it matches nothing)
/// and a second `All rows` request refused synchronously (which is `streaming`,
/// and so cleared the flag under the export that was actually running).
fn export_finished(current: Option<ExportRun>, finished: ExportRun) -> Option<ExportRun> {
    match current {
        Some(run) if run == finished => None,
        other => other,
    }
}

/// A result column's **real** name on its base table, from the wire provenance.
/// Empty for an expression column, which is never written to.
fn origin_column(rs: &ResultSet, ci: usize) -> String {
    rs.columns
        .get(ci)
        .and_then(|c| c.origin.as_ref())
        .map(|o| o.column.clone())
        .unwrap_or_default()
}

impl GridState {
    fn new(rs: Arc<ResultSet>, gctx: &GridCtx, key_map: &HashMap<usize, ColKey>) -> Self {
        let widths = init_widths(&rs, key_map);
        // Seed each column's display formatter from the persisted rules, keyed by
        // (conn_id, the column's own table, its real name) — see `format_key`. A
        // column with no saved rule (or no table) starts on the smart default.
        let conn = gctx.conn_id.get_untracked();
        // This tab's SQL dialect, from its connection's engine (drives Follow-FK SQL).
        let dialect = gctx
            .connections
            .with_untracked(|cs| {
                cs.iter()
                    .find(|c| c.id == conn)
                    .map(|c| SqlDialect::from_db_type(&c.db_type))
            })
            .unwrap_or_default();
        let formats: Vec<ColumnFormat> = (0..rs.col_count())
            .map(|ci| {
                let (name, ty) = rs
                    .columns
                    .get(ci)
                    .map(|c| (c.name.as_str(), c.type_name.as_str()))
                    .unwrap_or(("", ""));
                // An explicit saved rule wins; otherwise fall back to the name/type
                // smart default (e.g. an int `*_at` column → Timestamp).
                let saved = format_key(&rs, ci).and_then(|(db, table, col)| {
                    gctx.formats
                        .with_untracked(|rules| format::lookup(rules, conn, &db, &table, &col))
                });
                saved.unwrap_or_else(|| format::smart_default(name, ty))
            })
            .collect();
        GridState {
            rs: RwSignal::new(rs),
            order: RwSignal::new(Arc::new(Vec::new())),
            widths: RwSignal::new(widths),
            widths_at: RwSignal::new(grid_char_w()),
            active: RwSignal::new(None),
            anchor: RwSignal::new(None),
            frozen: RwSignal::new(None),
            scroll_to: RwSignal::new(None),
            vp: RwSignal::new(Rect::ZERO),
            focus_id: RwSignal::new(None),
            edit_cell: RwSignal::new(None),
            edit_buf: RwSignal::new(String::new()),
            dirty: RwSignal::new(HashMap::new()),
            new_rows: RwSignal::new(Vec::new()),
            del_rows: RwSignal::new(HashSet::new()),
            selecting: RwSignal::new(false),
            row_selecting: RwSignal::new(false),
            edit_model: RwSignal::new(Arc::new(EditModel::default())),
            // Text everywhere until `grid_view`'s effect resolves the columns.
            editors: RwSignal::new(Rc::new(Vec::new())),
            commit_busy: RwSignal::new(false),
            commit_seq: RwSignal::new(0),
            // Shared with the panel-level error bar (rendered in `results_section`).
            commit_wait: gctx.commit_wait,
            tx_holders: RwSignal::new(Some(gctx.tx_holders.clone())),
            commit_err: gctx.commit_err,
            commit_note: gctx.commit_note,
            popup: gctx.popup,
            popup_anchor: gctx.popup_anchor,
            popup_width: gctx.popup_width,
            menus: gctx.menus,
            last_mouse: gctx.last_mouse,
            source: gctx.source,
            summarize: RwSignal::new(Some(gctx.summarize.clone())),
            attach: RwSignal::new(Some(gctx.attach.clone())),
            dismiss: RwSignal::new(Some(gctx.dismiss.clone())),
            commit: RwSignal::new(Some(gctx.commit.clone())),
            export_file: RwSignal::new(Some(gctx.export_file.clone())),
            conn_at_load: conn,
            exporting: gctx.exporting,
            tab_id: gctx.tab_id,
            sync_canonical: RwSignal::new(gctx.sync_canonical.clone()),
            formats: RwSignal::new(formats),
            conn_id: gctx.conn_id,
            connections: gctx.connections,
            dialect,
            tx_mode: gctx.tx_mode,
            base_sql: gctx.base_sql,
            grid_query: gctx.grid_query,
            row_cap_override: gctx.row_cap_override,
            apply_view: RwSignal::new(Some(gctx.apply_view.clone())),
            view_err: gctx.view_err,
            fmt_rules: gctx.formats,
            save_formats: RwSignal::new(Some(gctx.save_formats.clone())),
            // Shared with the find bar (rendered up at the RESULTS-panel level).
            find_open: gctx.find_open,
            find_query: gctx.find_query,
            goto_open: gctx.goto_open,
            goto_query: gctx.goto_query,
            goto_step: gctx.goto_step,
            // Empty until `grid_view` resolves the source table's FKs into it.
            follow: RwSignal::new(Rc::new(HashMap::new())),
            follow_fk: RwSignal::new(Some(gctx.follow_fk.clone())),
            ai_fill: RwSignal::new(Some(gctx.ai_fill.clone())),
            ai_seed: RwSignal::new(Some(gctx.ai_seed.clone())),
            ai_busy: RwSignal::new(false),
            ai_gen: RwSignal::new(HashSet::new()),
            ai_gen_rows: RwSignal::new(HashSet::new()),
            new_rows_gen: RwSignal::new(0),
            ai_pulse: RwSignal::new(0.0),
            pulse_running: RwSignal::new(false),
            seed_open: RwSignal::new(false),
            seed_buf: RwSignal::new(String::new()),
            edit_row_open: RwSignal::new(false),
            edit_row_di: RwSignal::new(None),
            edit_row_saving: RwSignal::new(false),
        }
    }

    /// Whether this result supports server-side filter/sort: it was produced by a
    /// manual run we captured (`base_sql`) *and* came from a single writable base
    /// table (`insert_target`), so we can splice a WHERE/ORDER BY into the base SQL
    /// and re-run. Mirrors the row-action eligibility gate.
    fn filterable(&self) -> bool {
        self.base_sql.get_untracked().is_some()
            && self.edit_model.get_untracked().insert_target().is_some()
    }

    /// The real column name for display column `ci` (its wire origin), used to build
    /// filter conditions / ORDER BY against the base table. `None` for an expression
    /// column (no origin) or an out-of-range index.
    fn real_col(&self, ci: usize) -> Option<String> {
        self.rs
            .get_untracked()
            .columns
            .get(ci)
            .and_then(|c| c.origin.as_ref())
            .map(|o| o.column.clone())
    }

    /// Rebuild the tab's statement from `base_sql` + the current `grid_query` and
    /// re-run it (server-side filter/sort). A bad-condition / un-rewritable message
    /// goes to the bottom error bar (`view_err`), same place as a live DB error; a
    /// successful re-run clears it (via `apply_view`).
    fn apply_grid_query(&self) {
        let Some(base) = self.base_sql.get_untracked() else {
            return;
        };
        let gq = self.grid_query.get_untracked();
        match build_query(&base, &gq.filter, &gq.sort, self.dialect) {
            Ok(Some(sql)) => {
                if let Some(run) = self.apply_view.get_untracked() {
                    run(sql);
                }
            }
            Ok(None) => self.view_err.set(Some(
                "Can't filter this query — not a simple single-table SELECT".into(),
            )),
            Err(FilterError::BadCondition(msg)) => self.view_err.set(Some(msg)),
        }
    }

    /// The statement that produced the result currently on screen and **may be
    /// executed a second time**, for a re-run that changes nothing about the
    /// query itself — the capped notice's "read N rows", the export's "All rows".
    ///
    /// A two-line wrapper over [`rerun_statement`], which is where both halves
    /// of the decision live and are tested: the rewrite, and the write guard.
    /// The guard is not spelled out again here on purpose — a term a caller can
    /// delete is a term the suite cannot hold, and this call site had no test at
    /// all while the pure predicate beside it had a full table of them.
    ///
    /// **Not [`GridState::apply_grid_query`]**, which is the wrong tool here even
    /// though it would usually work: it treats a base it cannot rewrite as a
    /// *filter* failure and says "Can't filter this query — not a simple
    /// single-table SELECT". A join or a CTE is perfectly re-runnable at a bigger
    /// cap, and telling the user their filter is at fault when they have no
    /// filter is worse than the cap they were trying to get past.
    ///
    /// With no filter or sort the base *is* the statement. With one, the rewrite
    /// already succeeded once — that is how the filter got applied — so this asks
    /// again rather than caching the answer.
    fn current_statement(&self) -> Option<String> {
        let base = self.base_sql.get_untracked()?;
        rerun_statement(&base, &self.grid_query.get_untracked(), self.dialect)
    }

    /// Append a cell-derived condition (`col = 'val'` / `IS NULL` / negated) to the
    /// filter with ` AND `, then apply. Used by the cell "Filter by / Exclude" menu.
    fn add_filter_condition(&self, ci: usize, value: Option<&str>, negate: bool) {
        let Some(col) = self.real_col(ci) else {
            return;
        };
        let cond = eq_condition(&col, value, negate, self.dialect);
        self.grid_query.update(|gq| {
            if gq.filter.trim().is_empty() {
                gq.filter = cond;
            } else {
                gq.filter = format!("{} AND {}", gq.filter.trim(), cond);
            }
        });
        self.apply_grid_query();
    }

    /// Cycle server-side sort on display column `ci` (unsorted/other → ASC → DESC →
    /// unsorted), replacing any prior sort, then apply. No-op if the column has no
    /// real origin.
    fn cycle_server_sort(&self, ci: usize) {
        let Some(col) = self.real_col(ci) else {
            return;
        };
        self.grid_query.update(|gq| {
            let next = match gq.sort.first() {
                Some((c, true)) if *c == col => Some((col.clone(), false)), // ASC → DESC
                Some((c, false)) if *c == col => None,                      // DESC → off
                _ => Some((col.clone(), true)),                             // → ASC
            };
            gq.sort = next.into_iter().collect();
        });
        self.apply_grid_query();
    }

    /// The active server-side sort direction for display column `ci`, if any —
    /// drives the header chevron/label styling for eligible results.
    fn server_sort_dir(&self, ci: usize) -> Option<bool> {
        let col = self.real_col(ci)?;
        self.grid_query
            .with(|gq| gq.sort.iter().find(|(c, _)| *c == col).map(|(_, asc)| *asc))
    }

    /// Take down whatever the bottom bar is saying — **both** surfaces.
    ///
    /// One method rather than the seven identical copies of
    /// `if commit_err.is_some() { commit_err.set(None) }` this replaced: the
    /// bar grew a second signal (`commit_note`), and a message left standing on
    /// the surface one of those copies had never heard of is a note about a
    /// paste that three edits have since replaced.
    ///
    /// The `is_some` guard is kept and is not redundant: a floem signal never
    /// dedups, so an unconditional `set(None)` on every keystroke would
    /// invalidate the bar's container for a bar that is already down.
    fn clear_bar(&self) {
        if self.commit_err.get_untracked().is_some() {
            self.commit_err.set(None);
        }
        if self.commit_note.get_untracked().is_some() {
            self.commit_note.set(None);
        }
    }

    /// Stage a value for data-row `di`, column `ci` (`None` = SQL NULL). If it
    /// equals the original the entry is dropped (no longer dirty).
    fn stage(&self, di: usize, ci: usize, val: Option<String>) {
        self.stage_many(vec![(di, ci, val)]);
    }

    /// [`GridState::stage`] for a whole batch — **one** signal update for the
    /// lot, and the one copy of the revert-to-original rule (`stage` is a
    /// one-element call into this).
    ///
    /// The batching is not a micro-optimisation. `dirty` is read by the painter
    /// and by every derived view, so updating it per cell makes a 10,000-cell
    /// paste ten thousand invalidations of the grid — each one re-resolving the
    /// visible cells — where the user asked for one edit.
    fn stage_many(&self, cells: Vec<(usize, usize, Option<String>)>) {
        if cells.is_empty() {
            return;
        }
        let rs = self.rs.get_untracked();
        self.dirty.update(|d| {
            for (di, ci, val) in cells {
                // Original as `Option<String>`: NULL → `None`.
                let orig = rs
                    .cell(di, ci)
                    .map(|c| {
                        if c.is_null() {
                            None
                        } else {
                            Some(c.display().to_string())
                        }
                    })
                    .unwrap_or(None);
                if orig == val {
                    d.remove(&(di, ci)); // reverted to original → no longer dirty
                } else {
                    d.insert((di, ci), val);
                }
            }
        });
        // A fresh edit clears a stale message.
        self.clear_bar();
    }

    /// Stage an **explicit** value into a real cell, always recording it as an edit
    /// even when it equals the original. Used by AI Fill Value: an AI fill is an
    /// explicit "set this value" action, so the result is always visible (green) —
    /// otherwise, when the model returns a value equal to the current one (common
    /// when editing an already-coherent row), nothing would appear to happen.
    /// Manual inline edits use [`GridState::stage`], which clears when typed back to original.
    fn stage_set(&self, di: usize, ci: usize, val: Option<String>) {
        self.dirty.update(|d| {
            d.insert((di, ci), val);
        });
        self.clear_bar();
    }

    /// Stage a value into pending new-row `pidx`, column `ci` (`None` = SQL NULL,
    /// empty string clears the cell back to "use default"). New rows have no
    /// original to diff against, so an empty `Some("")` reverts the cell to unset
    /// (server default) rather than inserting an empty string.
    fn stage_new(&self, pidx: usize, ci: usize, val: Option<String>) {
        self.stage_new_many(vec![(pidx, ci, val)]);
    }

    /// [`GridState::stage_new`] for a whole batch, one signal update — see
    /// [`GridState::stage_many`] for why that matters.
    fn stage_new_many(&self, cells: Vec<(usize, usize, Option<String>)>) {
        if cells.is_empty() {
            return;
        }
        self.new_rows.update(|rows| {
            for (pidx, ci, val) in cells {
                let Some(row) = rows.get_mut(pidx) else {
                    continue;
                };
                match &val {
                    Some(s) if s.is_empty() => {
                        row.remove(&ci); // blank → fall back to the DB default
                    }
                    _ => {
                        row.insert(ci, val);
                    }
                }
            }
        });
        self.clear_bar();
    }

    /// Append a blank pending row and return its index.
    fn add_new_row(&self) -> usize {
        let mut idx = 0;
        self.new_rows.update(|rows| {
            idx = rows.len();
            rows.push(HashMap::new());
        });
        self.clear_bar();
        idx
    }

    /// Append a pending row per entry of `data_idxs` (Clone / Duplicate),
    /// returning the first one's index. Copies every editable column's value (or
    /// explicit NULL) **except** auto-increment columns, which are left for the
    /// server to assign.
    ///
    /// **One `new_rows.update` for the whole batch**, not one per row.
    ///
    /// One write, not N, and that is the whole of it: `new_rows` is the body
    /// `dyn_container`'s key, so each write tears the grid down and rebuilds it,
    /// re-running `compute_order` over the *whole* result. "Duplicate 100 rows"
    /// on a 200,000-row result was a hundred full rebuilds and a hundred
    /// O(n log n) sorts, on the UI thread, for one menu click.
    fn add_cloned_rows(&self, data_idxs: &[usize]) -> usize {
        let model = self.edit_model.get_untracked();
        let rs = self.rs.get_untracked();
        let ncols = rs.col_count();
        let maps: Vec<HashMap<usize, Option<String>>> = data_idxs
            .iter()
            .map(|&data_idx| {
                let mut map: HashMap<usize, Option<String>> = HashMap::new();
                if data_idx < rs.row_count() {
                    for ci in 0..ncols {
                        if !model.editable(ci) {
                            continue;
                        }
                        let auto = rs
                            .columns
                            .get(ci)
                            .and_then(|c| c.origin.as_ref())
                            .map(|o| o.flags.auto_increment)
                            .unwrap_or(false);
                        if auto {
                            continue; // server assigns the auto-increment key
                        }
                        if let Some(c) = rs.cell(data_idx, ci) {
                            map.insert(
                                ci,
                                if c.is_null() {
                                    None
                                } else {
                                    Some(c.display().to_string())
                                },
                            );
                        }
                    }
                }
                map
            })
            .collect();
        let mut idx = 0;
        self.new_rows.update(|rows| {
            idx = rows.len();
            rows.extend(maps);
        });
        self.clear_bar();
        idx
    }

    /// Toggle data-row `data_idx`'s "marked for deletion" state. Marking a row also
    /// drops any staged cell edits on it (a delete supersedes an update, so the row
    /// can never be both `UPDATE`d and `DELETE`d in one commit).
    fn toggle_delete(&self, data_idx: usize) {
        let now_marked = self.del_rows.try_update(|d| {
            if d.remove(&data_idx) {
                false
            } else {
                d.insert(data_idx);
                true
            }
        });
        if now_marked == Some(true) {
            self.dirty
                .update(|m| m.retain(|(di, _), _| *di != data_idx));
        }
        self.clear_bar();
    }

    /// Do this grid's signals still exist?
    ///
    /// **Call this first in anything that runs later than the frame it was
    /// scheduled in** — an `exec_after` tick, or a callback returned from a
    /// query, a commit or an AI turn. Every `GridState` signal is created in the
    /// child scope of a `dyn_container`, so switching tabs, closing one, or
    /// re-running the query disposes all of them; `get_untracked` is
    /// `try_get_untracked().unwrap()`, so a read after that **panics and takes
    /// the app with it**. This was not theoretical: typing one character in the
    /// find bar and pressing Ctrl+Tab within 150 ms crashed the app on the first
    /// attempt.
    ///
    /// `set` is deliberately asymmetric here — floem silently no-ops writes to a
    /// disposed signal — which is why the crashes all sit on *reads* and why the
    /// unguarded callbacks looked fine for so long.
    ///
    /// The file had this guard in five places and was missing it in five others,
    /// each spelled slightly differently. This is the one obvious thing for the
    /// next deferred callback to call.
    fn alive(&self) -> bool {
        self.rs.try_get_untracked().is_some()
    }

    /// Close any open popup menu and clear a lingering commit-error bar. Called
    /// from every grid click surface (cell / gutter / header) so a click anywhere
    /// on the table dismisses the error bar (its own clicks don't reach here).
    fn dismiss_overlays(&self) {
        if let Some(d) = self.dismiss.get_untracked() {
            (d)();
        }
        self.clear_bar();
        // A click anywhere on the table also dismisses a filter/sort error bar.
        if self.view_err.get_untracked().is_some() {
            self.view_err.set(None);
        }
    }

    /// Commit the in-progress inline edit (if any) into `dirty` / `new_rows`.
    fn commit_edit(&self) {
        let Some((i, ci)) = self.edit_cell.get_untracked() else {
            return;
        };
        let new = self.edit_buf.get_untracked();
        let nrows = self.rs.get_untracked().row_count();
        if i >= nrows {
            self.stage_new(i - nrows, ci, Some(new));
        } else {
            let order = self.order.get_untracked();
            let di = order.get(i).copied().unwrap_or(i);
            self.stage(di, ci, Some(new));
        }
        self.edit_cell.set(None);
    }

    /// Turn the staged `dirty` map into one [`RowEdit`] per (base table, row),
    /// using the edit model's provenance for real column names + the WHERE key.
    /// One base table's `UPDATE` for data row `di`: the `SET` list from `sets`
    /// (result-column index → new value, `None` = SQL NULL) and the `WHERE` key
    /// from the row's **original** values.
    ///
    /// Shared by the staged-batch builder and the row panel's immediate save,
    /// which had ~35 identical lines each. The WHERE key's construction is the
    /// part that matters: it is the identity every write on this path is aimed
    /// at, and it had drifted into three copies (a fourth, `build_refetch`,
    /// deliberately differs — it keys by the *post-edit* value, because the
    /// re-fetch has to find the row the write just left).
    fn row_edit_for(
        model: &schemaic_core::edit::EditModel,
        rs: &ResultSet,
        ti: usize,
        di: usize,
        mut sets: Vec<(usize, Option<String>)>,
    ) -> Option<RowEdit> {
        let tbl = model.table(ti)?;
        sets.sort_by_key(|(ci, _)| *ci); // stable SET-clause order
        let set = sets
            .into_iter()
            .map(|(ci, v)| (origin_column(rs, ci), v))
            .collect();
        Some(RowEdit {
            database: tbl.database.clone(),
            schema: tbl.schema.clone(),
            table: tbl.table.clone(),
            set,
            key: row_key(rs, tbl, di),
        })
    }

    fn build_edits(&self) -> Vec<RowEdit> {
        let model = self.edit_model.get_untracked();
        let rs = self.rs.get_untracked();
        let dirty = self.dirty.get_untracked();
        // (table_idx, data_row) → [(result_ci, new_value)]  (None = SQL NULL).
        // BTreeMap (+ sorted sets below) so the UPDATE order and SET-clause order
        // are deterministic — a failing commit reproduces identically (§7.5).
        let mut groups: EditGroups = BTreeMap::new();
        for ((di, ci), new) in &dirty {
            let Some(ti) = model.table_index(*ci) else {
                continue; // read-only column somehow staged — skip defensively
            };
            groups
                .entry((ti, *di))
                .or_default()
                .push((*ci, new.clone()));
        }
        groups
            .into_iter()
            .filter_map(|((ti, di), sets)| Self::row_edit_for(&model, &rs, ti, di, sets))
            .collect()
    }

    /// Like [`GridState::build_edits`], but for one data row `di` from an explicit change set
    /// (result-column index → new value, `None` = SQL NULL) rather than the staged
    /// `dirty` map — used by the whole-row JSON editor, which commits immediately.
    /// A join row edits >1 base table, so this may return several `RowEdit`s; the
    /// WHERE key comes from the ORIGINAL row (PK columns are read-only in the editor).
    fn build_row_edits(&self, di: usize, changes: &[(usize, Option<String>)]) -> Vec<RowEdit> {
        let model = self.edit_model.get_untracked();
        let rs = self.rs.get_untracked();
        // Group changed columns by their base table (deterministic SQL via BTreeMap).
        let mut groups: BTreeMap<usize, Vec<(usize, Option<String>)>> = BTreeMap::new();
        for (ci, v) in changes {
            if let Some(ti) = model.table_index(*ci) {
                groups.entry(ti).or_default().push((*ci, v.clone()));
            }
        }
        groups
            .into_iter()
            .filter_map(|(ti, sets)| Self::row_edit_for(&model, &rs, ti, di, sets))
            .collect()
    }

    /// A single-row refetch (splice in place after the whole-row edit commits).
    /// `changes` is the same change set the `UPDATE` was built from, because a key
    /// column is editable here too and the row must be found by the key the write
    /// just left in the table. `None` when the result isn't spliceable
    /// (multi-result path / no clean template).
    fn build_row_refetch(
        &self,
        di: usize,
        changes: &[(usize, Option<String>)],
    ) -> Option<RefetchRequest> {
        self.sync_canonical.get_untracked()?;
        let rs = self.rs.get_untracked();
        let model = self.edit_model.get_untracked();
        let template = refetch_template(&rs, &model)?;
        let edited: HashMap<usize, Option<String>> = changes.iter().cloned().collect();
        let key = refetch_key(&template, &rs, di, &edited);
        Some(RefetchRequest {
            template,
            rows: vec![RefetchRow { data_row: di, key }],
        })
    }

    /// Turn the staged `new_rows` into one [`RowInsert`] each, targeting the
    /// result's single writable table (`insert_target`). Empty when the result
    /// isn't a single-table insert destination. Each pending row's set cells map
    /// to real column names in ascending column order (deterministic SQL); a row
    /// with no set cells inserts an all-defaults row.
    fn build_inserts(&self) -> Vec<RowInsert> {
        let model = self.edit_model.get_untracked();
        let Some(tbl) = model.insert_target() else {
            return Vec::new();
        };
        let rs = self.rs.get_untracked();
        let real_col = |ci: usize| -> Option<String> {
            rs.columns
                .get(ci)
                .and_then(|c| c.origin.as_ref())
                .map(|o| o.column.clone())
        };
        self.new_rows
            .get_untracked()
            .iter()
            .map(|row| {
                let mut cis: Vec<usize> = row.keys().copied().collect();
                cis.sort_unstable();
                let cols = cis
                    .into_iter()
                    .filter_map(|ci| {
                        real_col(ci).map(|name| (name, row.get(&ci).cloned().flatten()))
                    })
                    .collect();
                RowInsert {
                    database: tbl.database.clone(),
                    schema: tbl.schema.clone(),
                    table: tbl.table.clone(),
                    cols,
                }
            })
            .collect()
    }

    /// Turn the rows marked for deletion into one [`RowDelete`] each (WHERE key from
    /// the single writable table's `key_cols` + the row's original values). Empty
    /// unless the result is a single-table insert/delete destination.
    fn build_deletes(&self) -> Vec<RowDelete> {
        let del = self.del_rows.get_untracked();
        if del.is_empty() {
            return Vec::new();
        }
        let model = self.edit_model.get_untracked();
        let Some(tbl) = model.insert_target() else {
            return Vec::new();
        };
        let rs = self.rs.get_untracked();
        let mut dis: Vec<usize> = del.into_iter().collect();
        dis.sort_unstable(); // deterministic DELETE order
        dis.into_iter()
            .filter_map(|di| {
                if di >= rs.row_count() {
                    return None;
                }
                Some(RowDelete {
                    database: tbl.database.clone(),
                    schema: tbl.schema.clone(),
                    table: tbl.table.clone(),
                    key: row_key(&rs, tbl, di),
                })
            })
            .collect()
    }

    /// Build a re-fetch request for the just-staged edits, so a commit can splice
    /// DB truth back in instead of re-running the whole query. `None` unless the
    /// result is a single base table with every column real-origined (see
    /// `refetch_template`) *and* the tab exposes a canonical sink (single-result
    /// path only). Each edited data row's key uses its **post-edit** values (a key
    /// column may itself have been edited).
    fn build_refetch(&self) -> Option<RefetchRequest> {
        self.sync_canonical.get_untracked()?; // multi-result path → no splice
        let rs = self.rs.get_untracked();
        let model = self.edit_model.get_untracked();
        let template = refetch_template(&rs, &model)?;
        let dirty = self.dirty.get_untracked();
        // Distinct edited data rows, sorted for deterministic order.
        let mut data_rows: Vec<usize> = dirty.keys().map(|(di, _)| *di).collect();
        data_rows.sort_unstable();
        data_rows.dedup();
        let rows = data_rows
            .into_iter()
            .map(|di| {
                // This row's staged edits, by result column — a key column among
                // them is what the row now answers to.
                let edited: HashMap<usize, Option<String>> = dirty
                    .iter()
                    .filter(|((d, _), _)| *d == di)
                    .map(|((_, ci), v)| (*ci, v.clone()))
                    .collect();
                let key = refetch_key(&template, &rs, di, &edited);
                RefetchRow { data_row: di, key }
            })
            .collect();
        Some(RefetchRequest { template, rows })
    }

    /// Splice re-fetched rows into the live result set in place — `(data_row, new
    /// cells)`, cells aligned to the result columns — then un-stage the edits this
    /// commit covered. Updates both the grid's live `rs` (cells re-read reactively)
    /// and the tab's canonical result set (so a later rebuild is fresh). No rebuild,
    /// so scroll / selection / widths survive.
    ///
    /// `committed` is the staged-key set the write was assembled from — *not* the
    /// whole map. The row panel commits on its own path, so wiping the map here
    /// threw away green cell edits elsewhere in the grid unwritten, and an edit
    /// staged during a commit's round-trip went the same way.
    fn apply_splice(&self, rows: Vec<(usize, Vec<Value>)>, committed: &HashSet<(usize, usize)>) {
        // A splice whose target no longer exists is a no-op, not a crash. The
        // commit is async and the grid it belongs to may be gone by the time it
        // returns — re-running the query in the *same* tab is enough, and the
        // app's own `still_active` downgrade only covers the neighbouring case
        // (switching tabs). On a row another session holds a lock, the window is
        // `innodb_lock_wait_timeout` wide — 50 s by default, open-ended in a
        // Manual transaction — so this is not a millisecond race.
        //
        // The `set`s below would no-op harmlessly on their own; it is
        // `sync_canonical.get_untracked()` and `rs.get_untracked()` that panic.
        if !self.alive() {
            return;
        }
        if !rows.is_empty() {
            self.rs.update(|arc| {
                Arc::make_mut(arc).splice_rows(&rows);
            });
            if let Some(sync) = self.sync_canonical.get_untracked() {
                (sync)(self.rs.get_untracked());
            }
        }
        // These edits are now persisted and reflected as originals.
        self.dirty.update(|d| drop_committed(d, committed));
        self.clear_bar();
    }

    /// Selection rectangle `(r0, c0, r1, c1)` inclusive, display coords.
    fn bounds(&self) -> Option<(usize, usize, usize, usize)> {
        let a = self.active.get()?;
        let anc = self.anchor.get().unwrap_or(a);
        Some((
            a.0.min(anc.0),
            a.1.min(anc.1),
            a.0.max(anc.0),
            a.1.max(anc.1),
        ))
    }
    fn bounds_untracked(&self) -> Option<(usize, usize, usize, usize)> {
        let a = self.active.get_untracked()?;
        let anc = self.anchor.get_untracked().unwrap_or(a);
        Some((
            a.0.min(anc.0),
            a.1.min(anc.1),
            a.0.max(anc.0),
            a.1.max(anc.1),
        ))
    }
}

/// Whether a reading of the Go-to-row **nonce** means "jump now".
///
/// `seen` is the effect's previous value: `None` on its build run, which must not
/// jump — the effect is created whenever the grid is, and jumping there would
/// move the selection every time a result loaded. An unchanged `step` must not
/// re-fire either: the effect re-runs when anything else it reads changes, and a
/// second jump to the same row would fight a scroll the user had since made.
pub(crate) fn goto_fires(seen: Option<u64>, step: u64) -> bool {
    seen.is_some_and(|s| s != step)
}

/// Keep at most one of the grid's two panel-level bars open: opening either
/// closes the other.
///
/// They share one anchor at the panel's top-right, so both open paints one over
/// the other — and the one on top takes every keystroke while the one underneath
/// is the one you can see. The editor's find/goto pair does the same.
///
/// **Both directions, which is the fix.** The exclusion tracked `goto_open`
/// alone and the Ctrl+F arm was a bare `find_open.set(true)`, so Ctrl+G then
/// Ctrl+F left both mounted: the goto bar painted over the find bar while the
/// find field autofocused and swallowed the typing, and Escape appeared to do
/// nothing. One function so a third bar joins here rather than adding a third
/// half-rule, and so the pair can be asserted at all.
///
/// The sets are guarded: `RwSignal::set` never dedups, and a redundant write
/// would re-run the other effect and dispose a field mid-keystroke.
pub(crate) fn one_bar_at_a_time(find_open: RwSignal<bool>, goto_open: RwSignal<bool>) {
    create_effect(move |_| {
        if goto_open.get() && find_open.get_untracked() {
            find_open.set(false);
        }
    });
    create_effect(move |_| {
        if find_open.get() && goto_open.get_untracked() {
            goto_open.set(false);
        }
    });
}

/// Whether `Del` over `rows` should **mark** them all or unmark them all.
///
/// The whole range is driven to one state rather than each row flipping its own:
/// on a mixed selection a per-row toggle both marks and unmarks, which reads as
/// the key doing nothing. Any unmarked row in range means "mark them all"; only
/// an already-fully-marked range unmarks.
///
/// An **empty** range answers "unmark", which is what `all` over nothing gives.
/// Unreachable from the key — the caller returns early on an empty range — and
/// the harmless answer if it ever isn't: unmarking nothing.
pub(crate) fn delete_vote(marked: impl Fn(usize) -> bool, rows: &[usize]) -> bool {
    !rows.iter().all(|di| marked(*di))
}

/// What a selection rectangle means for the aggregates bar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SelKind {
    /// No readout: nothing selected, or a lone cell (which aggregates to
    /// itself).
    Nothing,
    /// A row selection — counts only, no column named.
    WholeRow,
    /// A range read against this column.
    Column(usize),
}

/// Whether a selection gets arithmetic, and about which column.
///
/// Three rules, and they were all inline in an effect while the arithmetic they
/// gate had seventeen tests — so flipping `ncols > 1` to `>= 1` broke the
/// single-column exemption with nothing failing.
///
/// **Which column**: the one the selection *started* on — the anchor's — so
/// dragging from `price` across to `name` still reports `price`. The anchor
/// rather than the rectangle is the point: `bounds` is normalised and has
/// forgotten which corner you began at.
///
/// **A span covering every column is a row selection** (a gutter click, Ctrl+A,
/// the Ctrl+G jump), and its anchor column is column 0 — usually an id, whose
/// sum means nothing — so those get counts only. A **single-column result is
/// exempt**: there, covering every column is covering the one you meant.
pub(crate) fn selection_kind(
    bounds: Option<(usize, usize, usize, usize)>,
    anchor: Option<(usize, usize)>,
    ncols: usize,
) -> SelKind {
    let Some((r0, c0, r1, c1)) = bounds else {
        return SelKind::Nothing;
    };
    if r0 == r1 && c0 == c1 {
        return SelKind::Nothing;
    }
    if ncols > 1 && c1 - c0 + 1 == ncols {
        return SelKind::WholeRow;
    }
    match anchor {
        Some((_, col)) => SelKind::Column(col),
        None => SelKind::Nothing,
    }
}

/// Exact rendered pixel width of `text` in the grid's cell font (the app default
/// sans — IBM Plex Sans — at `font_body()`), via a throwaway `TextLayout`. Used to
/// Estimate a column's initial width from its header + a sample of cell values.
/// The room a grid cell spends on itself, beyond the value it shows.
///
/// `grid_pad_h()` on each side plus the right-hand divider — which is a *border*,
/// so it comes out of the content box as well. Both width estimators reserved a
/// flat `22.0` for it, which is a pixel generous at Normal and short by 6px at
/// 130% and 11px at 160%: **Auto-fit clipped the value it exists to fit**.
/// `numeric_edit_pad_left` composes the same three terms correctly, and is the
/// reason the shortfall is a composition bug rather than a wrong constant.
fn cell_chrome_w() -> f64 {
    2.0 * grid_pad_h() + GRID_CELL_DIVIDER
}

/// Carry stored column widths across an interface-scale change.
///
/// `gs.widths` is measured in pixels from `grid_char_w()` and then *stored*, and
/// the grid's rebuild key has no scale term — floem wraps only a `dyn_container`'s
/// key closure in an effect and calls the builder outside it, so the `grid_char_w()`
/// read in `init_widths` subscribes nothing. Raising the scale with a result open
/// therefore grew every cell's font, padding and row height while the columns
/// stayed cut for the old one: text at 21px in columns measured for 13px type,
/// ellipsized across the board until the statement was re-run or every divider
/// double-clicked. Lowering it left every column ~1.6x wider than its content.
///
/// **Every column is carried, including one the user dragged.** A width chosen to
/// fit that column's content is a width that should still fit it when the content
/// is 1.6x bigger, so there is no case for exempting it — and no `dragged` set to
/// maintain. `min_w` is applied *after*, because `min_col_w()` scales too: a
/// column dragged to 48px at Normal is under the 77px floor at Huge.
///
/// A `ratio` that is not a positive finite number leaves the widths alone. The old
/// measurement is a better answer than a column collapsed to the floor.
fn rescale_widths(widths: &[f64], ratio: f64, min_w: f64) -> Vec<f64> {
    if !ratio.is_finite() || ratio <= 0.0 {
        return widths.to_vec();
    }
    if ratio == 1.0 {
        return widths.to_vec();
    }
    widths.iter().map(|w| (w * ratio).max(min_w)).collect()
}

fn init_widths(rs: &ResultSet, key_map: &HashMap<usize, ColKey>) -> Vec<f64> {
    let sample = rs.row_count().min(200);
    rs.columns
        .iter()
        .enumerate()
        .map(|(ci, col)| {
            let mut chars = col.name.chars().count() + 3; // room for the sort arrow
            chars = chars.max(col.type_name.chars().count());
            for r in 0..sample {
                if let Some(c) = rs.cell(r, ci) {
                    chars = chars.max(c.display().chars().count().min(60));
                }
            }
            // A key column's header carries a leading key icon; budget for it so the
            // name/type line isn't squeezed (and clipped) by the icon + gap.
            let icon = if key_map.contains_key(&ci) {
                header_key_icon_w()
            } else {
                0.0
            };
            (chars as f64 * grid_char_w() + cell_chrome_w() + icon)
                .clamp(min_col_w(), max_col_w_init())
        })
        .collect()
}

/// Auto-fit width for one column over the whole result (double-click a divider).
/// `has_key` budgets for the header's leading key icon, and the type-name line is
/// included so a long type (e.g. `INT UNSIGNED`) isn't clipped after auto-fit.
fn autofit_width(rs: &ResultSet, ci: usize, has_key: bool) -> f64 {
    let mut chars = rs
        .columns
        .get(ci)
        .map(|c| c.name.chars().count() + 3)
        .unwrap_or(6);
    if let Some(c) = rs.columns.get(ci) {
        chars = chars.max(c.type_name.chars().count());
    }
    for r in 0..rs.row_count() {
        if let Some(c) = rs.cell(r, ci) {
            chars = chars.max(c.display().chars().count().min(140));
        }
    }
    let icon = if has_key { header_key_icon_w() } else { 0.0 };
    (chars as f64 * grid_char_w() + cell_chrome_w() + icon).clamp(min_col_w(), 900.0)
}

fn cell_in(bounds: Option<(usize, usize, usize, usize)>, i: usize, ci: usize) -> bool {
    matches!(bounds, Some((r0, c0, r1, c1)) if i >= r0 && i <= r1 && ci >= c0 && ci <= c1)
}

/// A keyboard navigation move, resolved by [`nav_target`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Nav {
    Down,
    Up,
    Right,
    Left,
    RowStart,
    RowEnd,
    First,
    Last,
    PageDown,
    PageUp,
}

/// Where `nav` moves the active cell from display position `(r, c)`.
///
/// `rows` is the **display** row total — real rows *plus* the pending new rows
/// rendered below them — because the selection lives in display space. Clamping
/// to the real-row count instead maps a pending-row position back *up* into the
/// real rows (Arrow-Down jumping backwards) and leaves a pending row unreachable
/// by keyboard.
fn nav_target(
    rows: usize,
    cols: usize,
    page: usize,
    (r, c): (usize, usize),
    nav: Nav,
) -> (usize, usize) {
    let last_r = rows.saturating_sub(1);
    let last_c = cols.saturating_sub(1);
    match nav {
        Nav::Down => ((r + 1).min(last_r), c),
        Nav::Up => (r.saturating_sub(1), c),
        Nav::Right => (r, (c + 1).min(last_c)),
        Nav::Left => (r, c.saturating_sub(1)),
        Nav::RowStart => (r, 0),
        Nav::RowEnd => (r, last_c),
        Nav::First => (0, 0),
        Nav::Last => (last_r, last_c),
        Nav::PageDown => ((r + page).min(last_r), c),
        Nav::PageUp => (r.saturating_sub(page), c),
    }
}

/// The grid's cell values, read out of the signals once, for the surfaces that
/// resolve a cell in [`schemaic_core::edit::GridCells`] rather than paint it.
///
/// Everything here is an `Arc` clone or a small map clone, and both callers are
/// one-shot user gestures (Ctrl+C, Attach to chat) — never a frame.
///
/// The rule itself lives in core because it kept going out one source short in
/// the view: first without `gs.dirty`, so an uncommitted edit was on screen
/// while the pre-edit value went to the model, and then without the column's
/// formatter, so a `Timestamp` column sent the epoch integer the cell does not
/// show. Nothing in `schemaic-ui` can construct a `GridState` to test either.
fn grid_cells<'a>(
    rs: &'a ResultSet,
    order: &'a [usize],
    formats: &'a [ColumnFormat],
    dirty: &'a HashMap<(usize, usize), Option<String>>,
    new_rows: &'a [HashMap<usize, Option<String>>],
) -> schemaic_core::edit::GridCells<'a> {
    schemaic_core::edit::GridCells {
        rs,
        order,
        formats,
        dirty,
        new_rows,
    }
}

/// Set the focused cell, optionally extending the range (shift) from the anchor.
fn set_active(gs: GridState, i: usize, ci: usize, extend: bool) {
    if extend {
        if gs.anchor.get_untracked().is_none() {
            gs.anchor.set(gs.active.get_untracked().or(Some((i, ci))));
        }
    } else {
        gs.anchor.set(Some((i, ci)));
    }
    gs.active.set(Some((i, ci)));
    scroll_active_into_view(gs, i, ci);
}

/// The window of `data_cols` (indices into `data_cols`) intersecting the
/// horizontal viewport, plus the pixel widths of the hidden columns on each side.
/// `start..end` + `left_pad` + `right_pad` always span the full
/// `sum(widths[data_cols])`, so rendering only the visible columns between two
/// spacers leaves the data pane's scroll geometry (and header alignment) unchanged.
#[derive(Clone, Debug, PartialEq)]
struct ColWindow {
    start: usize,
    end: usize,
    left_pad: f64,
    right_pad: f64,
}

/// Compute the visible-column window for a horizontal viewport `vp` over the data
/// columns, widening by `overscan` columns each side so a small scroll doesn't
/// expose a blank edge before the window memo updates.
fn compute_window(vp: Rect, widths: &[f64], data_cols: &[usize], overscan: usize) -> ColWindow {
    let n = data_cols.len();
    let w = |k: usize| widths.get(data_cols[k]).copied().unwrap_or(cell_w());
    // Pre-layout (viewport not measured yet) — render an initial slice so the first
    // frame isn't blank; the memo recomputes once `on_resize` seeds `gs.vp`.
    if vp.width() <= 1.0 {
        let end = n.min(16);
        let right_pad: f64 = (end..n).map(w).sum();
        return ColWindow {
            start: 0,
            end,
            left_pad: 0.0,
            right_pad,
        };
    }
    let left = vp.x0;
    let right = vp.x0 + vp.width();
    let (mut start, mut end) = (n, n);
    let mut x = 0.0;
    for k in 0..n {
        let cw = w(k);
        if start == n && x + cw > left {
            start = k; // first column whose right edge crosses into the viewport
        }
        if x >= right {
            end = k; // first column fully past the viewport's right edge
            break;
        }
        x += cw;
    }
    let start = start.min(n).saturating_sub(overscan);
    let end = (end + overscan).min(n);
    let left_pad: f64 = (0..start).map(w).sum();
    let right_pad: f64 = (end..n).map(w).sum();
    ColWindow {
        start,
        end,
        left_pad,
        right_pad,
    }
}

/// A zero-content filler of a fixed width — stands in for the columns hidden on
/// either side of the visible window so the row keeps its full scrollable width.
fn col_spacer(w: f64, h: f64) -> impl IntoView {
    empty().style(move |s| s.width(w).height(h).flex_shrink(0.0_f32))
}

/// Nudge the body scroll so `(i, ci)` is visible (keyboard nav).
fn scroll_active_into_view(gs: GridState, i: usize, ci: usize) {
    let vp = gs.vp.get_untracked();
    if vp.width() <= 0.0 {
        return;
    }
    let rh = row_h();
    let (mut nx, mut ny) = (vp.x0, vp.y0);
    let y0 = i as f64 * rh;
    if y0 < vp.y0 {
        ny = y0;
    } else if y0 + rh > vp.y0 + vp.height() {
        ny = y0 + rh - vp.height();
    }
    // Horizontal scroll applies only to data-pane columns; the frozen column lives
    // in its own always-visible pane. Compute the target x in *data-pane* space —
    // widths summed excluding the frozen column — matching the column-virtualized
    // spacer math, so scroll-into-view lands correctly even under a freeze.
    let widths = gs.widths.get_untracked();
    let frozen = gs.frozen.get_untracked();
    if frozen != Some(ci) {
        let x0: f64 = (0..ci)
            .filter(|j| frozen != Some(*j))
            .map(|j| widths.get(j).copied().unwrap_or(0.0))
            .sum();
        let x1 = x0 + widths.get(ci).copied().unwrap_or(0.0);
        if x0 < vp.x0 {
            nx = x0;
        } else if x1 > vp.x0 + vp.width() {
            nx = x1 - vp.width();
        }
    }
    gs.scroll_to.set(Some(Point::new(nx.max(0.0), ny.max(0.0))));
}

/// Copy the current selection to the clipboard as TSV (a lone cell → raw value).
fn copy_selection(gs: GridState) {
    let Some(rect) = gs.bounds_untracked() else {
        return;
    };
    let (rs, order) = (gs.rs.get_untracked(), gs.order.get_untracked());
    let (dirty, new_rows) = (gs.dirty.get_untracked(), gs.new_rows.get_untracked());
    let formats = gs.formats.get_untracked();
    let cells = grid_cells(&rs, &order, &formats, &dirty, &new_rows);
    // Columns in the order they are drawn, not the order they are indexed: a
    // frozen column is pinned to the left of the grid while its cells keep their
    // absolute index, so a selection that crosses it reads one way on screen and
    // another in the range. The clipboard's consumer is outside this grid and has
    // only the order to go on.
    let _ = floem::Clipboard::set_contents(cells.tsv(rect, gs.frozen.get_untracked()));
}

/// Paste the clipboard over the selection, staged as ordinary green edits.
///
/// **Staged, not written.** Everything a pasted cell touches goes through
/// [`GridState::stage`]/[`GridState::stage_new`] exactly as a typed edit does,
/// so the write-back plan, the one-row safety net and the Commit/Discard pair
/// apply unchanged — a paste is a batch of edits the user can still look at and
/// throw away, not a write.
///
/// Deliberately **not** interpreted: the block is split on tabs and newlines and
/// nothing else (see `core::edit::parse_tsv_block`), and a cell reading `NULL`
/// stages the four-character string, because that is what the copy side wrote
/// and a paste that quietly turned text into SQL `NULL` would be editing the
/// user's data on their behalf.
fn paste_selection(gs: GridState) {
    // An open inline editor owns Ctrl+V — it is a text field, and pasting into
    // the *grid* from inside one would replace the cell being typed into (and
    // its neighbours) instead of inserting at the caret. Explicit rather than
    // relying on the field to swallow the key first: this one is destructive
    // over a block, so being wrong about the dispatch order is expensive.
    if gs.edit_cell.get_untracked().is_some() {
        return;
    }
    let Ok(text) = floem::Clipboard::get_contents() else {
        return;
    };
    let Some(rect) = gs.bounds_untracked() else {
        return;
    };
    let block = schemaic_core::edit::parse_tsv_block(&text);
    let rs = gs.rs.get_untracked();
    let nrows = rs.row_count();
    // Display rows, pending new rows included: a paste that stopped at the last
    // *fetched* row could not fill the rows the user just added, which is one of
    // the two things this feature is for.
    let rows = nrows + gs.new_rows.with_untracked(Vec::len);
    let model = gs.edit_model.get_untracked();
    // `frozen` is what "the column beside the anchor" means: the grid draws the
    // frozen column first while every cell keeps its absolute index, so a block
    // walked in index order lands in columns the user never pointed at — and the
    // far-left one is the column they were protecting by freezing it.
    let frozen = gs.frozen.get_untracked();
    let plan = schemaic_core::edit::plan_paste(&block, rect, rows, rs.col_count(), frozen, |ci| {
        model.editable(ci)
    });
    if plan.cells.is_empty() && plan.dropped == 0 && plan.read_only == 0 {
        return;
    }
    let order = gs.order.get_untracked();
    let del = gs.del_rows.get_untracked();
    let mut skipped_deleted = 0usize;
    // **Before staging drains the cell list.** See `PastePlan::counts`.
    let counts = plan.counts();
    // Collected, then staged in **two** signal updates rather than one per cell:
    // `dirty` and `new_rows` are read by the painter and by every derived view,
    // so a per-cell update would invalidate the grid once for every cell of a
    // paste the user made in a single gesture.
    let (mut real, mut pending) = (Vec::new(), Vec::new());
    for (row, ci, value) in plan.cells {
        if row >= nrows {
            pending.push((row - nrows, ci, Some(value)));
            continue;
        }
        let di = order.get(row).copied().unwrap_or(row);
        // A row marked for deletion is going away; editing it would stage a
        // change to something the same commit deletes. Same rule `start_edit`
        // follows on Enter.
        if del.contains(&di) {
            skipped_deleted += 1;
            continue;
        }
        real.push((di, ci, Some(value)));
    }
    gs.stage_many(real);
    gs.stage_new_many(pending);
    // After staging, because `stage` clears this bar on every edit. Silence
    // would be the wrong answer here: a paste that discarded half a spreadsheet
    // looks exactly like one that worked.
    //
    // **Which surface** is `PasteReport`'s to decide, in `core::edit` where it
    // is tested: a partial paste is a success with a caveat and goes on the
    // ordinary chrome, while a paste that landed *nothing* is a failure and
    // earns the red fill. Both used to be errors, so "Pasted 5 cells, skipping
    // 1 in read-only columns." was rendered indistinguishably from a write-back
    // that failed.
    match schemaic_core::edit::paste_report(counts, skipped_deleted) {
        schemaic_core::edit::PasteReport::Clean => {}
        schemaic_core::edit::PasteReport::Notice(m) => gs.commit_note.set(Some(m)),
        schemaic_core::edit::PasteReport::Failed(m) => gs.commit_err.set(Some(m)),
    }
}

/// Rows of the grid as the user sees them — display order, staged edits and
/// pending new rows included — over the rectangle `(r0, c0, r1, c1)`, capped at
/// `core::prompt::ATTACH_ROW_CAP`.
///
/// Reads the *displayed* value, not the stored one, for the same reason the
/// clipboard does: what the user selected is what is on screen, and an
/// attachment that quietly disagreed with the grid would be answered about as
/// though it were the grid.
fn attached_rows(
    gs: GridState,
    rect: (usize, usize, usize, usize),
) -> (Vec<String>, Vec<Vec<String>>, usize) {
    let (rs, order) = (gs.rs.get_untracked(), gs.order.get_untracked());
    let (dirty, new_rows) = (gs.dirty.get_untracked(), gs.new_rows.get_untracked());
    let formats = gs.formats.get_untracked();
    let cells = grid_cells(&rs, &order, &formats, &dirty, &new_rows);
    cells.attached(
        rect,
        schemaic_core::prompt::ATTACH_ROW_CAP,
        gs.frozen.get_untracked(),
    )
}

/// How much of **this result's** connection the assistant may see.
///
/// The result's own connection, not the active one: a tab keeps the connection
/// it was opened on, so reading the active one would let a grid of production
/// rows be judged by a local database's setting.
fn ai_data_of(gs: GridState) -> AiData {
    let id = gs.conn_id.get_untracked();
    gs.connections
        .with_untracked(|cs| cs.iter().find(|c| c.id == id).and_then(|c| c.ai_data))
        .unwrap_or_default()
}

/// What the *current selection* would send, as the phrase the attach entry reads
/// — `core::model::attach_scope_label` over the selected rectangle, with the row
/// count capped at what an attachment actually carries.
fn selection_scope_label(gs: GridState) -> String {
    let (rows, cols) = match gs.bounds_untracked() {
        Some((r0, c0, r1, c1)) => (r1.saturating_sub(r0) + 1, c1.saturating_sub(c0) + 1),
        None => (1, 1),
    };
    schemaic_core::model::attach_scope_label(
        rows.min(schemaic_core::prompt::ATTACH_ROW_CAP),
        cols,
        gs.rs.get_untracked().col_count(),
    )
}

/// Label for the whole-result attach action, counting what would actually be
/// sent — a result over the cap says so *before* the click.
///
/// Phrased by `attach_scope_label` like the selection one, so the two entries
/// can't come to describe the same thing differently.
fn attach_label(gs: GridState) -> String {
    let ncols = gs.rs.get_untracked().col_count();
    let n = gs
        .order
        .get_untracked()
        .len()
        .min(schemaic_core::prompt::ATTACH_ROW_CAP);
    format!(
        "Attach {} to chat",
        schemaic_core::model::attach_scope_label(n, ncols, ncols)
    )
}

/// Stage the current selection (or the whole result, when `whole`) as the AI
/// panel's next attachment.
fn attach_to_chat(gs: GridState, whole: bool) {
    // Checked here as well as on the menu entries. The entries are the UI; this
    // is the rule — a keyboard path, a future caller or a menu built a moment
    // before the connection was locked down all arrive here.
    if !ai_data_of(gs).may_attach() {
        return;
    }
    let rs = gs.rs.get_untracked();
    let bounds = if whole {
        // The committed rows in display order (filters and sort applied) —
        // pending new rows are the user's unsaved draft, not a result.
        let (nrows, ncols) = (gs.order.get_untracked().len(), rs.col_count());
        if nrows == 0 || ncols == 0 {
            return;
        }
        (0, 0, nrows - 1, ncols - 1)
    } else {
        match gs.bounds_untracked() {
            Some(b) => b,
            None => return,
        }
    };
    let (columns, rows, total) = attached_rows(gs, bounds);
    if columns.is_empty() || rows.is_empty() {
        return;
    }
    // The summary is what survives to disk, and what the user reads back later
    // to know what they sent — so it counts what was *taken*, not what was
    // selected, and names the source when the result has one.
    let source = source_table(gs)
        .map(|t| format!(" from {t}"))
        .unwrap_or_default();
    let capped = if total > rows.len() {
        format!(" (the first {} of {total} selected)", rows.len())
    } else {
        String::new()
    };
    let summary = format!(
        "{} {} × {} {}{source}{capped}",
        rows.len(),
        if rows.len() == 1 { "row" } else { "rows" },
        columns.len(),
        if columns.len() == 1 {
            "column"
        } else {
            "columns"
        },
    );
    if let Some(f) = gs.attach.get_untracked() {
        (f)(schemaic_core::transcript::Attachment {
            summary,
            // The count **before** the cap. `rows` is already capped, so this is
            // the only place the prompt's "the first 200 of 5,000" note can come
            // from — computed from `rows.len()` it would read 200 of 200 and the
            // model would take the sample for the set.
            total_rows: total,
            columns,
            rows,
        });
    }
}

// Thin clipboard-facing wrappers over `schemaic_core::export` — unwrap the
// grid's live `ResultSet` + display order and delegate to the pure functions.

/// Render the whole result in `format`. The single dispatch point for both the
/// copy menu and the save-to-file menu, so the two can't drift.
fn render_export(gs: GridState, format: ExportFormat) -> String {
    let rs = gs.rs.get_untracked();
    let order = gs.order.get_untracked();
    let source = gs.source.get_untracked();
    format.render(
        rs.as_ref(),
        order.as_slice(),
        source
            .as_ref()
            .map(|s| (s.database.as_str(), s.schema.as_deref(), s.table.as_str())),
        // The tab's own connection dialect — an exported `INSERT` has to load into
        // the engine the rows came from.
        gs.dialect,
    )
}

fn export_column_json(gs: GridState, ci: usize) -> String {
    schemaic_core::export::export_column_json(
        gs.rs.get_untracked().as_ref(),
        gs.order.get_untracked().as_slice(),
        ci,
    )
}

fn export_column_csv(gs: GridState, ci: usize) -> String {
    schemaic_core::export::export_column_csv(
        gs.rs.get_untracked().as_ref(),
        gs.order.get_untracked().as_slice(),
        ci,
    )
}

/// Save the whole result to a file the user picks. Opens the system save dialog
/// (pre-filled with the source table's name + the format's extension), then
/// streams the rendering straight into the file; a cancelled dialog does nothing.
/// A write failure surfaces in the grid's error bar — the same place a failed
/// commit reports.
///
/// The rows are **snapshotted** before the dialog opens, not rendered: the dialog
/// is modal and slow, and the grid's result could be re-run or the tab switched
/// while it's up, so the export has to be of what the user was looking at when
/// they asked. `ResultSet` and the display order are behind `Arc`s, so holding
/// that snapshot costs a refcount rather than a copy — and a cancelled dialog now
/// costs nothing at all, where it used to pay a full render first.
/// The Download icon's menu: a format per entry, or — when there is more of the
/// result than the grid fetched — a scope step in front of them.
///
/// **The scope step appears only when the two scopes differ.** An uncapped result
/// *is* every row, so offering to fetch them again would be a choice between a
/// thing and itself, and the flat five-item menu the user already knows is the
/// right menu for it. It also needs a statement to re-run
/// (`current_statement`) — a result spliced together by a refetch, or one with no
/// captured `base_sql`, has none, and the cap is then the only honest scope on
/// offer.
fn export_menu(
    gs: GridState,
    row_total: Option<schemaic_core::stats::RowCount>,
    // Whether a column-header sort is applied. Not read off `GridState` because
    // it does not live there: the sort is `grid_view`'s, threaded to the header
    // and the toolbar.
    sorted: bool,
) -> Vec<MenuEntry> {
    let per_format = |scope_all: bool| -> Vec<MenuEntry> {
        ExportFormat::ALL
            .iter()
            .map(|&f| MenuEntry::action(f.label(), move || save_export(gs, f, scope_all)))
            .collect()
    };
    let truncated = gs.rs.with_untracked(|rs| rs.truncated);
    // **The write guard reaches this menu too.** "All rows" re-executes the
    // statement, so it is a path running user SQL and answers to
    // `filter::rerun_statement`'s refusal — flat rather than a `Confirm`,
    // because a Save dialog is not a moment at which to ask whether to run an
    // `UPDATE … RETURNING` again. Not offering it is the whole enforcement: the
    // scope cannot be chosen, so it cannot be requested.
    //
    // The refusal is *inside* `current_statement`, not a second term ANDed onto
    // it here, and that is the fix rather than a tidy-up: the term this line
    // used to carry could be deleted with the whole suite still green, and
    // `save_export` re-read the same signal at click time without re-asking it.
    // One tested function, asked by everything that re-runs.
    let rerunnable = gs.current_statement().is_some();
    if !truncated || !rerunnable {
        return per_format(false);
    }
    let fetched = gs.rs.with_untracked(|rs| rs.row_count());
    vec![
        MenuEntry::sub(
            format!(
                "Fetched rows ({})",
                schemaic_core::text::human_count(fetched)
            ),
            per_format(false),
        ),
        // Named off the same total the stats line reports, and `~` for the same
        // reason it does: the figure is an estimate from the catalogue, and a
        // menu that promised an exact count the export then missed would be
        // worse than one that never claimed it.
        MenuEntry::sub(
            {
                let size = match row_total {
                    Some(n) => format!("~{}", schemaic_core::text::human_count(n.value() as usize)),
                    None => String::new(),
                };
                // **Every way the file will differ from the screen, said at the
                // point of choice** — after the file is written is too late.
                // Which differences those are, and how they read, is
                // `export::all_rows_label`'s, in core where it is tested; this
                // line only answers whether each holds.
                //
                // The transaction one is the larger of the two and was the
                // undisclosed one: `All rows` re-runs on a *fresh* connection,
                // outside this tab's pinned session, so a manual-transaction
                // tab's uncommitted inserts are on screen and absent from the
                // file, and rows it deleted are in the file and gone from the
                // screen.
                schemaic_core::export::all_rows_label(
                    &size,
                    sorted,
                    gs.tx_mode.get_untracked() == schemaic_core::tx::TxMode::Manual,
                )
            },
            per_format(true),
        ),
    ]
}

fn save_export(gs: GridState, format: ExportFormat, all_rows: bool) {
    let default_name = suggested_filename(
        gs.source
            .get_untracked()
            .as_ref()
            .map(|s| s.display())
            .as_deref(),
        format,
    );
    let opts = FileDialogOptions::new()
        .title("Export results")
        .default_name(default_name)
        .allowed_types(vec![FileSpec {
            name: format.label(),
            extensions: format.extensions(),
        }]);
    let rs = gs.rs.get_untracked();
    let order = gs.order.get_untracked();
    let source = gs.source.get_untracked();
    let dialect = gs.dialect;
    // The statement is snapshotted with the rows and for the same reason: the
    // dialog is modal and slow, and a filter typed while it stood open must not
    // change what the export was asked for.
    let statement = gs.current_statement();
    // The connection the *rows* came from, not the tab's current one — see
    // `GridState::conn_at_load`. Paired with `rs.database` so both halves of
    // "where did this result come from" answer as of the same moment.
    let conn_id = gs.conn_at_load;
    let database = rs.database.clone();
    let Some(export) = gs.export_file.get_untracked() else {
        return;
    };
    save_as(opts, move |file| {
        let Some(path) = file.and_then(|f| f.path.first().cloned()) else {
            return; // cancelled
        };
        let scope = match (all_rows, statement.clone()) {
            (true, Some(sql)) => crate::ExportScope::AllRows {
                conn_id,
                database: database.clone(),
                sql,
            },
            // No statement to re-run: the menu only offers "All rows" when there
            // is one, so this is the belt to that brace rather than a path the
            // user can take.
            _ => crate::ExportScope::Fetched,
        };
        let streaming = matches!(scope, crate::ExportScope::AllRows { .. });
        // Claimed before the flag goes up, so the closure below can compare
        // against it rather than against its own scope.
        let run = ExportRun::new(gs.tab_id);
        if streaming {
            // Clear the bar's *stale* messages first: `Exporting` outranks a
            // note but not an error, so a failure left standing from an earlier
            // commit or filter would hide the Cancel for as long as the export
            // ran. `clear_bar` covers the commit pair; the filter/sort error is
            // the third dismissible message and needs saying separately, the way
            // `dismiss_overlays` says it.
            //
            // A stalled write's note (`commit_wait`) and a batch statement's own
            // failure are deliberately *not* cleared: neither is stale — the
            // first is live and offers a Rollback more urgent than this Cancel,
            // and the second means no grid is mounted to export from.
            gs.clear_bar();
            if gs.view_err.get_untracked().is_some() {
                gs.view_err.set(None);
            }
            gs.exporting.set(Some(run));
        }
        let shown = path.file_name().map(|n| n.to_string_lossy().into_owned());
        // `save_as` takes an `Fn`, so the snapshot is cloned rather than moved —
        // two `Arc` bumps and a small `Option<TableSource>`, not the rows.
        (export)(
            crate::ExportRequest {
                path,
                format,
                rs: rs.clone(),
                order: order.clone(),
                source: source.clone(),
                dialect,
                scope,
            },
            // `try_update`: the result-set scope may have been disposed while the
            // dialog was open or the write ran — a plain `set` would panic on the
            // freed signal.
            Rc::new(move |outcome| {
                let name = shown.clone().unwrap_or_else(|| "the file".to_string());
                // **The message goes up before the flag comes down**, and the
                // order is the whole reason the bar reads as one strip. Signal
                // writes take effect one at a time, so clearing `exporting`
                // first leaves a moment with no state at all — `any_up` false —
                // and the bar collapses and reopens, which reads as two
                // unrelated messages rather than an operation told in two
                // tenses.
                match outcome {
                    // **Which sentence, and whether there is one at all, is
                    // `export_note`'s** — in `core::export` where it is tested.
                    // Only the streamed scope announces a row count (a snapshot
                    // of what is already on screen has nothing to report that
                    // the screen doesn't show), but a *loss* is announced in
                    // either scope: a CSV that blanked a blob column looked
                    // exactly like one that had nothing to blank.
                    crate::ExportOutcome::Done(tally) => {
                        if let Some(msg) =
                            schemaic_core::export::export_note(&tally, &name, streaming)
                        {
                            gs.commit_note.try_update(|v| *v = Some(msg));
                        }
                    }
                    // A note, not an error: stopping was the user's own doing.
                    // It says the file is short because the alternative — a
                    // silent partial file — is the thing this whole path is
                    // careful about, and deleting it is not ours to do.
                    crate::ExportOutcome::Cancelled => {
                        gs.commit_note.try_update(|v| {
                            *v = Some(format!("Export cancelled — {name} is incomplete"))
                        });
                    }
                    // The same sentence the Cancel arm above uses, for the same
                    // reason, whenever the file was already created — a failure
                    // is the case where the user is least likely to look, and
                    // `File::create` truncated whatever was at that path before
                    // the first row arrived.
                    crate::ExportOutcome::Failed { message, partial } => {
                        let msg = schemaic_core::export::export_failure_note(
                            &message,
                            partial.then_some(name.as_str()),
                        );
                        gs.commit_err.try_update(|v| *v = Some(msg));
                    }
                }
                // **Only the export that raised the flag may lower it** — and
                // now that is what the code does rather than what the comment
                // said. Testing `streaming` tests *this request's* scope, which
                // is `true` for a second `All rows` request too: the app refuses
                // that one synchronously, and the refusal's own tail then cleared
                // the **running** export's flag and took its Cancel away. The
                // `Fetched` case the guard was added for is covered by the same
                // comparison, since a `Fetched` request never raised a flag to
                // match.
                gs.exporting.try_update(|v| *v = export_finished(*v, run));
            }),
        );
    });
}

/// A draggable column-resize divider pinned to the right edge of a header cell.
/// Drag adjusts that column's width; double-click auto-fits to content.
fn col_resize_handle(gs: GridState, ci: usize, has_key: bool) -> impl IntoView {
    let dragging = RwSignal::new(false);
    let h = empty();
    let hid = h.id();
    h.style(|s| {
        s.absolute()
            .inset_right(0.0)
            .inset_top(0.0)
            .width(resize_hit_w())
            .height(grid_header_h())
            .cursor(CursorStyle::ColResize)
    })
    .on_event(EventListener::PointerDown, move |e| {
        if let Event::PointerDown(pe) = e
            && pe.button.is_primary()
        {
            dragging.set(true);
            hid.request_active();
            return EventPropagation::Stop;
        }
        EventPropagation::Continue
    })
    .on_event(EventListener::PointerMove, move |e| {
        if dragging.get_untracked()
            && let Event::PointerMove(pe) = e
        {
            // Same moving-handle trick as `v_resize_handle`: the divider
            // re-centres on the column edge each frame, so the offset from
            // centre is the incremental delta.
            let d = pe.pos.x - resize_hit_w() / 2.0;
            gs.widths.update(|w| {
                if let Some(x) = w.get_mut(ci) {
                    *x = (*x + d).clamp(min_col_w(), 1200.0);
                }
            });
            return EventPropagation::Stop;
        }
        EventPropagation::Continue
    })
    .on_event(EventListener::PointerUp, move |_| {
        if dragging.get_untracked() {
            dragging.set(false);
            hid.clear_active();
        }
        EventPropagation::Continue
    })
    .on_double_click_stop(move |_| {
        // A `DoubleClick` consumes the second `PointerUp`, so the PointerUp handler
        // above never runs to end the drag — clear it here too, or the divider stays
        // "stuck" resizing on every hover until the next click.
        if dragging.get_untracked() {
            dragging.set(false);
            hid.clear_active();
        }
        let rs = gs.rs.get_untracked();
        let w = autofit_width(&rs, ci, has_key);
        gs.widths.update(|ws| {
            if let Some(x) = ws.get_mut(ci) {
                *x = w;
            }
        });
    })
}

/// A result column's key role in its source table (drives the header key icon).
#[derive(Clone, Copy, PartialEq, Debug)]
enum ColKey {
    Primary,
    Foreign,
    Index,
}

impl ColKey {
    fn svg(self) -> &'static str {
        match self {
            ColKey::Primary => icons::KEY_ROUND,
            ColKey::Foreign | ColKey::Index => icons::KEY_SQUARE,
        }
    }
    fn color(self) -> floem::peniko::Color {
        match self {
            ColKey::Primary => theme::key_primary(),
            ColKey::Index => theme::key_index(),
            ColKey::Foreign => theme::key_foreign(),
        }
    }
}

/// Where a persisted column formatter lives for result column `ci`: the real
/// `(database, table, column)` its value came from, with the table under the name
/// the UI shows it by (`schema.table` outside PostgreSQL's `public`, so a rule on
/// `sales.orders` can't leak onto `public.orders`).
///
/// The identity is the column's **own** provenance, not the tab's source table.
/// Keying on the source meant a hand-written query in a table-opened tab both
/// read and wrote rules under a table its columns never came from: a Timestamp
/// saved on `customers.created_at` rendered `orders.created_at` as a datetime.
/// `None` for an expression column — it belongs to no table, so there is nothing
/// to save a rule against; it still formats for the life of the result.
fn format_key(rs: &ResultSet, ci: usize) -> Option<(String, String, String)> {
    let o = rs.columns.get(ci)?.origin.as_ref()?;
    let table = TableSource::new(o.database.clone(), o.schema.clone(), o.table.clone());
    Some((o.database.clone(), table.display(), o.column.clone()))
}

/// The tab's source table and the loaded schema it lives in — the five steps
/// every "what does this result inherit from its table" question starts with.
///
/// It is one function because it used to be four copies, and the copies drifted:
/// the namespace check that keeps `public.orders` apart from `sales.orders` was
/// added to one of them and not the others.
fn source_schema(
    source: RwSignal<Option<TableSource>>,
    db_nodes: RwSignal<Vec<ConnNode>>,
) -> Option<(TableSource, Arc<DbSchema>)> {
    let src = source.get_untracked()?;
    let nodes = db_nodes.get_untracked();
    let node = nodes.iter().find(|n| n.database == src.database)?;
    let SchemaState::Loaded(schema) = node.schema.get_untracked() else {
        return None;
    };
    Some((src, schema))
}

/// Per-column key roles for the result's source table, keyed by **result-column
/// index**. Empty when the tab wasn't opened from a table or its schema isn't
/// loaded yet. Primary keys win; single-column indexes are Foreign (if they back
/// an FK) else Index. Multi-column indexes are ignored (only single-column ones
/// get a marker).
///
/// Indices, not names: a tab keeps its source when the user types a different
/// query into it, so `SELECT o.customerNumber FROM orders o` in a tab opened from
/// `customers` used to paint that column with `customers`' gold primary-key icon,
/// and `SELECT 1 AS customerNumber` earned it on a literal.
/// [`ResultSet::origin_columns`] is the identity rule.
fn column_key_map(
    rs: &ResultSet,
    source: RwSignal<Option<TableSource>>,
    db_nodes: RwSignal<Vec<ConnNode>>,
) -> HashMap<usize, ColKey> {
    let Some((src, schema)) = source_schema(source, db_nodes) else {
        return HashMap::new();
    };
    let Some(t) = schema.find_table(src.schema.as_deref(), &src.table) else {
        return HashMap::new();
    };
    key_roles(
        t,
        &rs.origin_columns(&src.database, src.schema.as_deref(), &src.table),
    )
}

/// Which editor control each result column's cells get, by result-column index.
///
/// **The declared type, not the wire type**, wherever the catalogue has one:
/// MySQL hands an `ENUM` over the wire as a string and a `BOOLEAN` as `TINYINT`,
/// so the member list and the `tinyint(1)` width only exist in the schema (see
/// [`schemaic_core::celledit`]). A column whose base table isn't loaded — a
/// schema still fetching, an expression column, a database that isn't in this
/// connection's tree — falls back to the wire type, where the date family still
/// resolves and everything else is text.
///
/// Every read here is **tracked**: a schema arriving after the grid was built
/// re-runs the effect that calls this, and the controls appear.
fn column_editors(
    rs: &ResultSet,
    db_nodes: RwSignal<Vec<ConnNode>>,
    dialect: SqlDialect,
) -> Vec<CellEditor> {
    rs.columns
        .iter()
        .map(|c| {
            let declared = c.origin.as_ref().and_then(|o| {
                db_nodes.with(|nodes| {
                    let node = nodes.iter().find(|n| n.database == o.database)?;
                    let SchemaState::Loaded(schema) = node.schema.get() else {
                        return None;
                    };
                    // The namespace is part of the table's identity — two
                    // PostgreSQL schemas may hold same-named tables, and the
                    // wrong one's column would answer with the wrong type.
                    let t = schema.tables.iter().find(|t| {
                        t.name == o.table && t.schema.as_deref() == o.schema.as_deref()
                    })?;
                    let col = t.columns.iter().find(|cc| cc.name == o.column)?;
                    Some((col.type_name.clone(), schema.clone()))
                })
            });
            match declared {
                Some((ty, schema)) => celledit::editor_for_column(&ty, dialect, Some(&schema)),
                None => celledit::editor_for_type(&c.type_name, dialect),
            }
        })
        .collect()
}

/// The control column `ci`'s cells get, or [`CellEditor::Text`] before the
/// resolver has run (and for a column index the result doesn't have).
fn cell_editor(gs: GridState, ci: usize) -> CellEditor {
    gs.editors
        .with_untracked(|e| e.get(ci).cloned())
        .unwrap_or(CellEditor::Text)
}

/// The role precedence, over a table's own columns and the result columns they
/// landed in (`real name → result index`). Pure half of [`column_key_map`].
fn key_roles(t: &TableInfo, by_name: &HashMap<&str, usize>) -> HashMap<usize, ColKey> {
    let mut map = HashMap::new();
    let at = |col: &str| by_name.get(col).copied();
    for c in &t.columns {
        if c.primary_key
            && let Some(ci) = at(&c.name)
        {
            map.insert(ci, ColKey::Primary);
        }
    }
    for ix in &t.indexes {
        if ix.is_primary() || ix.columns.len() != 1 {
            continue;
        }
        let Some(ci) = at(&ix.columns[0].name) else {
            continue;
        };
        if map.get(&ci) == Some(&ColKey::Primary) {
            continue;
        }
        if ix.foreign {
            map.insert(ci, ColKey::Foreign); // FK wins over a plain index
        } else {
            map.entry(ci).or_insert(ColKey::Index);
        }
    }
    map
}

/// Everything the cell "Follow" action needs for one foreign key: the FK (its
/// target), the result-column indices holding its referencing columns (so a
/// clicked row's key values can be read), and the source database (used when the
/// FK names no schema). Shared by every referencing column of a composite key.
#[derive(Clone)]
struct FollowSpec {
    fk: ForeignKeyInfo,
    /// Result-column indices for `fk.columns`, in the same order.
    value_cols: Vec<usize>,
    default_schema: String,
}

/// Resolve, per result-column, the foreign key it participates in — for the cell
/// "Follow" menu. Only the source table's own columns (matched by provenance) are
/// considered, so a joined result maps the correct side; a FK whose referencing
/// columns aren't all present in the result is skipped (can't read its values).
/// Empty unless the tab has a source table with loaded schema and FKs.
fn build_follow_specs(
    rs: &ResultSet,
    source: RwSignal<Option<TableSource>>,
    db_nodes: RwSignal<Vec<ConnNode>>,
) -> HashMap<usize, Rc<FollowSpec>> {
    let mut map = HashMap::new();
    let Some((src, schema)) = source_schema(source, db_nodes) else {
        return map;
    };
    let (db, table) = (&src.database, &src.table);
    let Some(t) = schema.find_table(src.schema.as_deref(), table) else {
        return map;
    };
    if t.foreign_keys.is_empty() {
        return map;
    }
    // Real column name (of this source table) → its result-column index.
    let real_to_ci = rs.origin_columns(db, src.schema.as_deref(), table);
    for fk in &t.foreign_keys {
        let value_cols: Option<Vec<usize>> = fk
            .columns
            .iter()
            .map(|c| real_to_ci.get(c.as_str()).copied())
            .collect();
        let Some(value_cols) = value_cols else {
            continue; // a referencing column isn't in the result → can't follow
        };
        let spec = Rc::new(FollowSpec {
            fk: fk.clone(),
            value_cols: value_cols.clone(),
            default_schema: db.clone(),
        });
        for ci in value_cols {
            map.insert(ci, spec.clone());
        }
    }
    map
}

/// Execute a "follow foreign key" for data-row `data_idx` under `spec`: read the
/// row's key values for the FK's referencing columns (from the committed result),
/// build the referenced-table query, and hand it to the app. Shared by the cell
/// menu's "Follow relation" and the Ctrl-click shortcut.
fn follow_relation(gs: GridState, data_idx: usize, spec: &FollowSpec) {
    let rs = gs.rs.get_untracked();
    let values: Vec<Value> = spec
        .value_cols
        .iter()
        .map(|&vc| {
            rs.cell(data_idx, vc)
                .map(|c| c.to_value())
                .unwrap_or(Value::Null)
        })
        .collect();
    if let Some(ft) =
        schemaic_core::schema::follow_target(&spec.fk, &values, &spec.default_schema, gs.dialect)
        && let Some(cb) = gs.follow_fk.get_untracked()
    {
        (cb)(TableSource::new(ft.database, ft.schema, ft.table), ft.sql);
    }
}

/// Which of the user's *other* tabs hold an open transaction on this tab's
/// connection — `(tab id, title)`, in tab order. Answered live (the set changes
/// under the write), by the UI root, which is what holds the tab list; the rule
/// itself is `schemaic_core::tx::write_blocking_tabs`.
pub(crate) type TxHoldersFn = Rc<dyn Fn() -> Vec<(usize, String)>>;

/// Bundle of app-provided context the results grid needs, threaded from
/// `query_pane` down through the results-view chain.
#[derive(Clone)]
pub(crate) struct GridCtx {
    /// The active tab's source `(database, table)`, for key-icon lookup.
    pub(crate) source: RwSignal<Option<TableSource>>,
    /// A column name to select + scroll into view once the grid loads, then clear
    /// (schema-tree column double-click → open table + highlight column). The grid
    /// consumes it via an effect, so re-requesting on an already-loaded tab works.
    pub(crate) highlight_col: RwSignal<Option<String>>,
    /// The exact SQL last run manually — the base the server-side filter/sort
    /// splice into (`schemaic_core::filter::build_query`). `None` until first run.
    pub(crate) base_sql: RwSignal<Option<String>>,
    /// The active server-side filter/sort for this result (persists across result
    /// reloads; reset on a fresh manual run).
    pub(crate) grid_query: RwSignal<schemaic_core::filter::GridQuery>,
    /// A one-off row cap for this tab, set by the capped notice's read-more
    /// action and read by the app's run path in place of the global setting.
    pub(crate) row_cap_override: RwSignal<Option<usize>>,
    /// A filter/sort re-run's DB error (tab-level) — rendered in the grid's bottom
    /// bar so the current table stays put. Cleared on a table click / new run.
    pub(crate) view_err: RwSignal<Option<String>>,
    /// Fresh-load nonce (tab-level): part of the results-view container key so a
    /// `Loaded`→`Loaded` filter/sort re-run rebuilds the grid, while an in-place
    /// commit splice (which doesn't bump it) still skips the rebuild.
    pub(crate) load_gen: RwSignal<u64>,
    /// Re-run the active tab with a rewritten (filtered/sorted) statement — no
    /// history, preserves `base_sql`/`grid_query` (see `TabsActions::apply_view`).
    pub(crate) apply_view: ApplyViewFn,
    pub(crate) db_nodes: RwSignal<Vec<ConnNode>>,
    /// The refresh announcement the statistics slots don't make themselves — see
    /// [`crate::SchemaUi::stats_gen`]. The toolbar's total is read from a slot a
    /// Refresh clears, so the ask has to re-run when it does.
    pub(crate) stats_gen: RwSignal<u64>,
    /// Saved connections + the active id, for the identity-colour rule drawn at
    /// the table's top edge (the "prominent colour" setting).
    pub(crate) connections: RwSignal<Vec<Connection>>,
    pub(crate) active_conn: RwSignal<u64>,
    /// Ui-level popup-menu signal (header/cell right-click menus).
    pub(crate) popup: RwSignal<Option<Vec<MenuEntry>>>,
    /// Popup anchor signal (icon-anchored toolbar dropdowns vs cursor menus).
    pub(crate) popup_anchor: RwSignal<Option<PopupAnchor>>,
    /// `min_width` of the next popup panel (the Copy dropdown sets its own).
    pub(crate) popup_width: RwSignal<f64>,
    /// Every menu flag in the app, including the date picker's channel — what a
    /// control that opens one needs in order to close the others (see
    /// [`crate::widgets::MenuId`]).
    pub(crate) menus: crate::widgets::MenuFlags,
    /// The pointer in window coords, for the one question the grid cannot ask
    /// floem: where the keyboard went — see [`reclaim_keyboard`].
    pub(crate) last_mouse: RwSignal<(f64, f64)>,
    /// Reveal the AI panel + send a message (used for the cell "AI Summary").
    pub(crate) summarize: Rc<dyn Fn(String)>,
    /// Stage result rows as an attachment on the AI panel's next question.
    pub(crate) attach: crate::AttachFn,
    /// Follow a foreign key: open the referenced `(database, table)` in a new tab
    /// running the given filter `sql` (built by the grid from a FK + the row).
    pub(crate) follow_fk: FollowFn,
    /// Open the Live Monitor for `(conn_id, database, table)` — watch the result's
    /// base table for row changes. Offered whenever the *tab* has a source table
    /// (`results_section`'s button is gated on `source.is_some()`), which is
    /// deliberately weaker than the row-action group's `insert_target`: a table
    /// with no usable row key passes it, and the monitor answers with "No row key
    /// for this table" rather than the button being silently absent.
    pub(crate) open_monitor: crate::MonitorFn,
    /// Open the table-properties modal for the tab's source table. Gated exactly
    /// as `open_monitor` is, and for the same reason — the panel answers "no
    /// statistics for this object" itself, which is more use than a missing
    /// button.
    pub(crate) open_properties: crate::PropertiesFn,
    /// Ask the app to fetch this database's table statistics if nobody has
    /// (see [`crate::SchemaActions::db_stats`]) — called once per capped result,
    /// which is where the `1,000 of ~4.2m` total comes from.
    pub(crate) db_stats: Rc<dyn Fn(u64, String)>,
    /// AI-fill a single cell (sample base table → one-shot AI → stage the result).
    pub(crate) ai_fill: crate::AiFillFn,
    /// AI-generate seed rows (Insert Row / Seed Table) → stage pending rows.
    pub(crate) ai_seed: crate::AiSeedFn,
    /// Close any open popup / schema context menu (so a grid click dismisses them
    /// — grid cells consume the pointer-down, so the root handler never fires).
    pub(crate) dismiss: Rc<dyn Fn()>,
    /// Execute staged edits transactionally. Arg 2 is an optional re-fetch request
    /// (present ⇒ splice the edited rows instead of full-re-running); arg 3 is the
    /// completion callback, invoked on the UI thread with the [`CommitDone`].
    pub(crate) commit: crate::CommitFn,
    /// Write an export to disk off the UI thread (see [`crate::ExportFn`]).
    pub(crate) export_file: crate::ExportFn,
    /// The streamed export running in this window, if any — see [`ExportRun`].
    /// **Window-scoped**, so it outlives a tab switch the way its cancellation
    /// token does.
    pub(crate) exporting: RwSignal<Option<ExportRun>>,
    /// Which tab this context belongs to, so an export can be told from an export
    /// *here*.
    pub(crate) tab_id: usize,
    /// Stop the running export (the app owns the cancellation token).
    pub(crate) export_cancel: Rc<dyn Fn()>,
    /// Splice sink: replace the tab's canonical result set (so a later rebuild is
    /// fresh). `None` on the multi-result path (no splice — full re-run instead).
    pub(crate) sync_canonical: Option<SyncCanonicalFn>,
    /// The tab's connection is read-only → disable all inline editing (an empty
    /// `EditModel`, so no cell is editable / committable). Reactive.
    pub(crate) read_only: Memo<bool>,
    /// The tab's commit mode. Read by the export menu and nothing else here: an
    /// `All rows` export re-runs the statement on a **fresh connection**, so a
    /// `TxMode::Manual` tab's uncommitted rows are on screen and absent from the
    /// file, which the label has to say before the file is written.
    pub(crate) tx_mode: RwSignal<schemaic_core::tx::TxMode>,
    /// The tab's connection id — keys per-column display formatters together with
    /// the source `(database, table)`.
    pub(crate) conn_id: RwSignal<u64>,
    /// App-wide per-column display-formatter rules (persisted). The grid reads it
    /// to seed each column's format and upserts on a menu choice.
    pub(crate) formats: RwSignal<Vec<ColumnFormatRule>>,
    /// Persist the formatter rules to disk (called after an upsert).
    pub(crate) save_formats: Rc<dyn Fn()>,
    /// In-grid find (Ctrl+F). State lives here (at the RESULTS-panel level) so the
    /// find bar can render at the panel's top edge — above the grid — while the
    /// search runs in `grid_view` (which has the row data). `find_step` is a
    /// monotonic nonce + direction the bar bumps on next/prev/submit; `grid_view`
    /// watches `find_query` (incremental) and `find_step` (directional).
    pub(crate) find_open: RwSignal<bool>,
    pub(crate) find_query: RwSignal<String>,
    pub(crate) find_step: RwSignal<(u64, bool)>,
    /// Match count for the find bar's `pos/total` readout. `find_pos` is the
    /// 1-based index of the current match (0 when the selection isn't on a match);
    /// `find_total` is the number of matches; `find_more` is set when the scan hit
    /// its cell budget, so `total` is a lower bound (rendered with a `+`).
    pub(crate) find_total: RwSignal<usize>,
    pub(crate) find_pos: RwSignal<usize>,
    pub(crate) find_more: RwSignal<bool>,
    /// Go to row (Ctrl+G). Same split as find, and for the same reason: the popup
    /// renders at the panel level while `grid_view` performs the jump, since only
    /// it knows the row count. `goto_step` is a nonce the popup bumps on Enter —
    /// a nonce rather than watching `goto_query`, because the jump happens on
    /// submit, not on every keystroke.
    pub(crate) goto_open: RwSignal<bool>,
    pub(crate) goto_query: RwSignal<String>,
    pub(crate) goto_step: RwSignal<u64>,
    /// The selection-aggregates line, written by `grid_view` (which has the cells)
    /// and rendered by the panel-level bar (which can sit at the panel's bottom
    /// edge) — the same split as the find bar, for the same reason. `None` when
    /// nothing multi-cell is selected, which is when the bar is hidden.
    ///
    /// One signal is enough because only one result grid is mounted at a time:
    /// `results_multi` is a tab strip keyed on `active_result`, not several grids
    /// side by side.
    pub(crate) sel_summary: RwSignal<Option<String>>,
    /// Last commit error (grid write-back), shown in a bottom error bar at the
    /// panel level (like the find bar at the top). Cleared by the next edit/commit.
    pub(crate) commit_err: RwSignal<Option<String>>,
    /// A **note** for the same bar, on the ordinary chrome rather than the red
    /// fill: something worth saying about an operation that *worked*.
    ///
    /// Its own signal rather than a flag beside `commit_err`, because the two
    /// are read by different things — the bar picks a surface, and every other
    /// predicate in this file asks "is an error up". A partial paste used
    /// `commit_err` and so reported an ordinary success in the colour that means
    /// a write-back failed. See `core::edit::PasteReport`.
    pub(crate) commit_note: RwSignal<Option<String>>,
    /// What to say about a write that hasn't come back yet, once it has been
    /// waiting long enough to be worth saying anything (`tx::write_wait_note`).
    /// Shares the bottom bar with `commit_err` — a write is either still waiting
    /// or has failed, never both.
    pub(crate) commit_wait: RwSignal<Option<WaitNote>>,
    /// The tabs the wait note can name (see [`TxHoldersFn`]).
    pub(crate) tx_holders: TxHoldersFn,
    /// `ROLLBACK` another tab's transaction — the wait note's one-click way out
    /// when exactly one transaction of the user's own could be the holder.
    pub(crate) rollback_tx: Rc<dyn Fn(usize)>,
    /// The workspace error modal (shared with the editor error bar): `error_open`
    /// reveals it; the grid's "View" first sets `error_text` to the full commit
    /// error so the modal shows that instead of the tab's query error.
    pub(crate) error_open: RwSignal<bool>,
    pub(crate) error_text: RwSignal<Option<String>>,
}

/// The grid's commit-status bar, rendered at the RESULTS-panel level so it pins to
/// the panel's bottom edge — same look/position as the editor error bar (the red
/// `reject_bg` fill, rounded, 5px insets, 35px tall). The one-lined message on the
/// left, a right-aligned **View** that opens the full error in the shared modal
/// (via a text override). Absolute → overlays the panel out of flow.
///
/// It carries three things, and the **surface is the message**:
/// - an **error** in the red fill: the shown Run-Everything statement's own
///   failure (`batch_err` — a batch has no editor bar to put it in), else a commit
///   write-back or a filter/sort re-run;
/// - a **wait note** for a write that is taking long enough to need explaining
///   ([`arm_wait_note`]), on the ordinary chrome surface, with a one-click
///   `Rollback` when exactly one transaction of the user's own could be the
///   holder. It uses the footer's `tx_rollback` colour deliberately: it is the
///   same action on the same surface, and the two should never diverge;
/// - a **note** on that same ordinary surface, for something worth saying about
///   an operation that *worked* — a partial paste is the one today. It is last
///   in the order because it is the only one of the three that is not a problem:
///   an error or a stalled write is more urgent than a caveat, and the three
///   can only coincide if an edit landed and a write failed in the same frame.
///
/// The note exists because there was no non-red channel and a partial paste used
/// the red one, so "Pasted 5 cells, skipping 1 in read-only columns." — an
/// ordinary success — was indistinguishable from a write-back that failed.
pub(crate) fn grid_error_bar(
    bars: BarSignals,
    rollback_tx: Rc<dyn Fn(usize)>,
    export_cancel: Rc<dyn Fn()>,
    error_open: RwSignal<bool>,
    error_text: RwSignal<Option<String>>,
) -> impl IntoView {
    let BarSignals {
        batch_err,
        commit_err,
        commit_note,
        view_err,
        commit_wait,
        ..
    } = bars;
    // The batch error leads, because a failed statement has no grid mounted at
    // all: a commit error left over from another result tab's grid would be
    // reporting on something the user isn't looking at, and the statement's own
    // failure would have nowhere to go. Then an error over the wait note — it
    // describes a write that is already over, while the note describes one still in
    // flight (and every path clears the note before reporting a failure anyway).
    let current = move || {
        batch_err
            .get()
            .or_else(|| commit_err.get())
            .or_else(|| view_err.get())
            .map(BarState::Error)
            .or_else(|| commit_wait.get().map(BarState::Wait))
            // Above the note and below the two problems. A running export is not
            // a problem, so an error or a stalled write outranks it; but it is
            // *live*, and the note it will itself be replaced by is not, so a
            // leftover note must not sit on top of a Cancel the user may need.
            .or_else(|| bars.exporting_here().then_some(BarState::Exporting))
            .or_else(|| commit_note.get().map(BarState::Note))
    };
    dyn_container(current, move |state| {
        let msg = match state {
            None => return empty().into_any(),
            Some(BarState::Wait(note)) => return wait_bar(note, rollback_tx.clone()).into_any(),
            Some(BarState::Note(m)) => return note_bar(m).into_any(),
            Some(BarState::Exporting) => {
                return export_bar(export_cancel.clone()).into_any();
            }
            Some(BarState::Error(msg)) => msg,
        };
        // Collapse to a single line (a multi-line server error would spill out
        // the top); the full text stays available in the View modal.
        let one_line = msg.split_whitespace().collect::<Vec<_>>().join(" ");
        let full = msg;
        // View only when the bar is hiding something — a server error with a
        // DETAIL under it. On a short one-liner (a JSON parse error, a NOT-NULL
        // rejection) it opens a modal repeating the same words.
        let view: AnyView = if hides_detail(&full, BAR_ONE_LINE_CHARS) {
            text("View")
                .on_click_stop(move |_| {
                    error_text.set(Some(full.clone()));
                    error_open.set(true);
                })
                .style(|s| {
                    s.color(theme::err_fix_btn())
                        .font_size(theme::font_body())
                        .margin_right(theme::scaled(8.0))
                })
                .into_any()
        } else {
            empty().into_any()
        };
        h_stack((
            text(one_line).style(|s| {
                s.color(theme::reject_text())
                    .font_size(theme::font_body())
                    .max_width_pct(80.0)
                    .text_ellipsis()
                    .margin_left(theme::scaled(8.0))
            }),
            empty().style(|s| s.flex_grow(1.0_f32)),
            view,
        ))
        .style(|s| {
            s.flex_row()
                .items_center()
                .width_full()
                .height_full()
                .background(theme::reject_bg())
                .border_radius(5.0)
        })
        .into_any()
    })
    .style(move |s| {
        if bars.any_up() {
            // The height and the inset the selection summary lifts itself over —
            // `consts::grid_selection_lift` reads the same two.
            s.absolute()
                .inset_left(crate::consts::GRID_BAR_INSET)
                .inset_right(crate::consts::GRID_BAR_INSET)
                .inset_bottom(crate::consts::GRID_BAR_INSET)
                .height(crate::consts::grid_bar_h())
        } else {
            s
        }
    })
}

/// Which of the four the bar is showing. An enum rather than the
/// `Result<WaitNote, String>` this was, because a third arm arrived and
/// `Ok`/`Err` had already stopped meaning success and failure — and a fourth has
/// since, so the shape has earned itself twice.
///
/// Four *states*, on two surfaces: `Exporting` and `Note` share the ordinary
/// chrome (`export_bar` draws `note_bar`'s exact fill), because they are one
/// operation told in two tenses.
enum BarState {
    Error(String),
    Wait(WaitNote),
    Exporting,
    Note(String),
}

/// Everything the bottom bar can be showing, as one value.
///
/// A struct rather than five parameters, for two reasons that point the same
/// way. [`grid_error_bar`] was at seven arguments and the note surface made it
/// eight; and **[`BarSignals::any_up`] is asked from two places** — the bar's own
/// style and the selection summary that has to lift itself above it — which were
/// two hand-written copies of the same four-way `is_some`, so the note would
/// have had to be added to both. A bar that is up while the selection summary
/// thinks it isn't is the two of them drawn on top of each other.
#[derive(Clone, Copy)]
pub(crate) struct BarSignals {
    /// The shown Run-Everything statement's own failure — a batch has no editor
    /// bar to put it in.
    pub(crate) batch_err: Memo<Option<String>>,
    pub(crate) commit_err: RwSignal<Option<String>>,
    pub(crate) commit_note: RwSignal<Option<String>>,
    pub(crate) view_err: RwSignal<Option<String>>,
    pub(crate) commit_wait: RwSignal<Option<WaitNote>>,
    /// True while a streamed export is running. The bar's fourth state, and the
    /// only one that is *not* the tail of an operation: it stands for the whole
    /// length of one, which is why it is the only state here carrying a control
    /// that stops something rather than explaining it. The action itself is a
    /// parameter of [`grid_error_bar`] beside `rollback_tx`, for the reason this
    /// struct is `Copy`: an `Rc` in here would cost every reader of `any_up` a
    /// clone.
    pub(crate) exporting: RwSignal<Option<ExportRun>>,
    /// Which tab this bar belongs to, so [`BarSignals::exporting_here`] can tell
    /// an export running *here* from one running in another tab. The signal is
    /// window-wide; the bar is not.
    pub(crate) tab_id: usize,
}

impl BarSignals {
    /// Is the bar up at all? **The one predicate.**
    pub(crate) fn any_up(&self) -> bool {
        self.batch_err.with(Option::is_some)
            || self.commit_err.with(Option::is_some)
            || self.commit_note.with(Option::is_some)
            || self.view_err.with(Option::is_some)
            || self.commit_wait.with(Option::is_some)
            || self.exporting_here()
    }

    /// Is a streamed export running **in this tab**?
    ///
    /// The signal outlives a tab switch on purpose — that is what keeps the Cancel
    /// reachable — so every reader has to ask about its own tab rather than about
    /// the window. Reading it any other way is how the flag came to be drawn on a
    /// tab that had nothing to do with the export.
    pub(crate) fn exporting_here(&self) -> bool {
        let tab = self.tab_id;
        self.exporting.with(|e| e.is_some_and(|r| r.tab == tab))
    }
}

/// The bar while a streamed export runs: `Exporting…` on the note's surface,
/// with **Cancel** at the right where the error bar puts **View** and the wait
/// note puts **Rollback**.
///
/// **On the bar rather than on the stats line**, which is where it started. That
/// line is the result's own, and it is the crowded end of the panel — a capped
/// result already spends it on `db · 1k of ~16k rows · read 5k rows`, and a
/// fourth clause pushed the whole thing toward the icons. The bar is empty
/// almost always, is the width of the grid, and already owns the report this
/// turns into: the same strip says `Exporting… Cancel` and then
/// `Exported 16k rows to employees.csv`, so the export occupies one place from
/// beginning to end instead of announcing itself in one and reporting in
/// another.
fn export_bar(cancel: Rc<dyn Fn()>) -> impl IntoView {
    h_stack((
        text("Exporting…").style(|s| {
            s.color(theme::text())
                .font_size(theme::font_body())
                .margin_left(theme::scaled(8.0))
        }),
        empty().style(|s| s.flex_grow(1.0_f32)),
        // `err_fix_btn` is the bar's own action colour — what **View** uses two
        // arms up. Cancel is the same kind of thing in the same place, and a
        // second accent here would read as a different class of control.
        text("Cancel")
            .on_click_stop(move |_| (cancel)())
            .style(|s| {
                s.color(theme::err_fix_btn())
                    .font_size(theme::font_body())
                    .margin_right(theme::scaled(8.0))
                    .cursor(CursorStyle::Pointer)
            }),
    ))
    .style(|s| {
        s.flex_row()
            .items_center()
            .width_full()
            .height_full()
            // The note's surface, because this *is* the note — one operation
            // told in two tenses, and a different fill for the first half would
            // read as two unrelated messages.
            .background(theme::bg_deepest())
            .border(1.0)
            .border_color(theme::border())
            .border_radius(5.0)
    })
}

/// The note half of [`grid_error_bar`]: one line on the ordinary chrome, no
/// action and no **View**.
///
/// No `View` because a note is short by construction — it is a sentence this
/// codebase wrote, not a server's — and a modal repeating the same words is the
/// call [`grid_error_bar`] already makes for a one-line error.
fn note_bar(msg: String) -> impl IntoView {
    h_stack((
        text(msg).style(|s| {
            s.color(theme::text())
                .font_size(theme::font_body())
                .max_width_pct(90.0)
                .text_ellipsis()
                .margin_left(theme::scaled(8.0))
        }),
        empty().style(|s| s.flex_grow(1.0_f32)),
    ))
    .style(|s| {
        s.flex_row()
            .items_center()
            .width_full()
            .height_full()
            // The wait note's surface exactly: the two are the same kind of
            // thing (the bar saying something that isn't an error) and a second
            // neutral fill would read as a third state that doesn't exist.
            .background(theme::bg_deepest())
            .border(1.0)
            .border_color(theme::border())
            .border_radius(5.0)
    })
}

/// Roughly how many characters of a message the error bar shows before it
/// ellipsizes — the threshold for offering **View**. Approximate on purpose: the
/// bar is as wide as the results panel, so the exact figure moves with the
/// window, and the messages this decides between (a parse error against a
/// multi-line server error) are nowhere near each other in length.
const BAR_ONE_LINE_CHARS: usize = 90;

/// The wait-note half of [`grid_error_bar`]: the sentence, and — when there is
/// exactly one candidate — the button that ends it.
///
/// The tab name is the user's to choose and can be any length, so the button
/// clips it: an untruncated one pushed the button past the bar's right edge and
/// squeezed the sentence out.
fn wait_bar(note: WaitNote, rollback_tx: Rc<dyn Fn(usize)>) -> impl IntoView {
    const TAB_NAME_MAX: usize = 10;
    let action = match note.rollback {
        None => empty().into_any(),
        Some((tab_id, title)) => text(format!("Roll back {}", truncate(&title, TAB_NAME_MAX)))
            .on_click_stop(move |_| (rollback_tx)(tab_id))
            .style(|s| {
                // Never shrinks: the sentence is the part that may ellipsize, and
                // `margin_left` is a real gap even when the flex spacer between
                // them has been squeezed to nothing.
                s.color(theme::tx_rollback())
                    .font_size(theme::font_body())
                    .flex_shrink(0.0_f32)
                    .margin_left(theme::scaled(12.0))
                    .margin_right(theme::scaled(8.0))
                    .hover(|s| s.color(theme::tx_rollback_hover()))
            })
            .into_any(),
    };
    h_stack((
        text(note.text).style(|s| {
            s.color(theme::text())
                .font_size(theme::font_body())
                .max_width_pct(80.0)
                .text_ellipsis()
                .margin_left(theme::scaled(8.0))
        }),
        empty().style(|s| s.flex_grow(1.0_f32)),
        action,
    ))
    .style(|s| {
        s.flex_row()
            .items_center()
            .width_full()
            .height_full()
            .background(theme::bg_deepest())
            .border(1.0)
            .border_color(theme::border())
            .border_radius(5.0)
    })
}

/// The in-grid find bar (Ctrl+F), rendered at the RESULTS-panel level so it sits
/// at the panel's top-right edge (the search itself runs in `grid_view`, driven by
/// these signals). Absolute → overlays the panel out of flow.
pub(crate) fn grid_find_bar(
    find_open: RwSignal<bool>,
    find_query: RwSignal<String>,
    find_step: RwSignal<(u64, bool)>,
    find_total: RwSignal<usize>,
    find_pos: RwSignal<usize>,
    find_more: RwSignal<bool>,
) -> impl IntoView {
    // Bump the (nonce, forward) command so `grid_view`'s directional effect re-runs.
    let step = move |forward: bool| {
        let (n, _) = find_step.get_untracked();
        find_step.set((n.wrapping_add(1), forward));
    };
    dyn_container(
        move || find_open.get(),
        move |open| {
            if !open {
                return empty().into_any();
            }
            let close: Rc<dyn Fn()> = Rc::new(move || {
                find_open.set(false);
                find_query.set(String::new());
            });
            let esc = close.clone();
            let input = edit_field(
                find_query,
                FieldCfg {
                    placeholder: "Find in results",
                    autofocus: true,
                    font_size: theme::font_body,
                    border_radius: 6.0,
                    height: Some(field_input_h),
                    on_submit: Some(Rc::new(move || step(true))),
                    on_escape: Some(Rc::new(move || (esc)())),
                    on_arrow_up: Some(Rc::new(move || step(false))),
                    on_arrow_down: Some(Rc::new(move || step(true))),
                    ..Default::default()
                },
            )
            .style(|s| s.width(theme::scaled(180.0)));
            // `pos/total` readout (like the editor find bar). Blank until there's a
            // query; `find_more` adds a `+` when the scan hit its cell budget.
            let count = dyn_container(
                move || {
                    (
                        find_query.get().is_empty(),
                        find_pos.get(),
                        find_total.get(),
                        find_more.get(),
                    )
                },
                move |(is_empty, pos, total, more)| {
                    if is_empty {
                        return empty().into_any();
                    }
                    let label = format!("{pos}/{total}{}", if more { "+" } else { "" });
                    text(label)
                        .style(|s| {
                            s.font_size(theme::font_label())
                                .color(theme::text_dim())
                                .min_width(30.0)
                        })
                        .into_any()
                },
            );
            let icon_btn = |markup: &'static str, sz: f32, on: Rc<dyn Fn()>| {
                container(icons::icon(markup, sz))
                    .on_click_stop(move |_| (on)())
                    .style(|s| {
                        s.items_center()
                            .color(theme::text_dim())
                            .hover(|s| s.color(theme::text()))
                    })
            };
            let prev_btn = icon_btn(icons::CHEVRON_UP, 15.0, Rc::new(move || step(false)));
            let next_btn = icon_btn(icons::CHEVRON_DOWN, 15.0, Rc::new(move || step(true)));
            let close_btn = icon_btn(icons::X, 14.0, close.clone());
            h_stack((input, count, prev_btn, next_btn, close_btn))
                .style(|s| {
                    s.items_center()
                        .gap(theme::scaled(8.0))
                        .padding_horiz(theme::scaled(8.0))
                        .padding_vert(theme::scaled(6.0))
                        .background(theme::bg_panel())
                        .border(1.0)
                        .border_color(theme::border())
                        .border_radius(8.0)
                })
                .into_any()
        },
    )
    .style(|s| s.absolute().inset_top(5.0).inset_right(5.0))
}

/// The selection-aggregates bar: what the current multi-cell selection adds up
/// to, pinned to the bottom edge of the RESULTS panel.
///
/// Rendered at panel level, like the find bar, so it can sit at the panel's edge
/// rather than scrolling with the rows — and computed in `grid_view`, which has
/// the cells. It is hidden entirely when nothing multi-cell is selected, so it
/// costs no room in the ordinary case.
///
/// `error_shown` lifts it clear of [`grid_error_bar`], the panel's other
/// bottom-anchored overlay. They coincide exactly when a bulk delete fails, so
/// "it's transient" is not an answer — that is the moment both have something to
/// say.
pub(crate) fn grid_selection_bar(
    sel_summary: RwSignal<Option<String>>,
    error_shown: impl Fn() -> bool + 'static + Copy,
) -> impl IntoView {
    dyn_container(
        move || sel_summary.get(),
        move |summary| match summary {
            None => empty().into_any(),
            Some(text_line) => text(text_line)
                .style(|s| {
                    // `bg_deepest`, the footer's surface — darker than the panel
                    // it floats over, which is what makes the readout legible
                    // against the grid behind it. Already a gated pairing with
                    // `text_dim` (the completion popup's doc line), so this
                    // introduces no combination the contrast test hasn't judged.
                    s.color(theme::text_dim())
                        .font_size(theme::font_label())
                        .padding_horiz(theme::scaled(10.0))
                        .padding_vert(theme::scaled(4.0))
                        .background(theme::bg_deepest())
                        .border(1.0)
                        .border_color(theme::border())
                        .border_radius(6.0)
                })
                .into_any(),
        },
    )
    // **Clicks fall through it.** It is the last child of the results stack and
    // has no interactive content, so without this it swallowed every pointer
    // event over the cells it covers — no selection, no menu dismissal, no drag
    // release, in the bottom-right corner of every grid. `grid_error_bar`
    // legitimately keeps its events: it owns a clickable **View**.
    .pointer_events(|| false)
    .style(move |s| {
        if sel_summary.with(Option::is_some) {
            // Above `grid_error_bar` when that one is up, at the edge otherwise —
            // and both halves of that geometry are stated once, in
            // `consts::grid_selection_lift`, because the bar's height scales.
            s.absolute()
                .inset_right(crate::consts::SELECTION_BAR_INSET)
                .inset_bottom(crate::consts::grid_selection_lift(error_shown()))
        } else {
            s
        }
    })
}

/// The grid's **go to row** popup (Ctrl+G) — the editor's go-to-line popup, for
/// the results grid. Rendered at the RESULTS-panel level beside the find bar, and
/// at the same anchor: only one of the two is ever open (opening this one closes
/// find, in `grid_view`), exactly as the editor does with its own pair.
///
/// The jump itself is `grid_view`'s, driven by the `goto_step` nonce — the row
/// count lives there, not here.
pub(crate) fn grid_goto_bar(
    goto_open: RwSignal<bool>,
    goto_query: RwSignal<String>,
    goto_step: RwSignal<u64>,
) -> impl IntoView {
    dyn_container(
        move || goto_open.get(),
        move |open| {
            if !open {
                return empty().into_any();
            }
            let close: Rc<dyn Fn()> = Rc::new(move || {
                goto_open.set(false);
                goto_query.set(String::new());
            });
            // Enter bumps the nonce; `grid_view` resolves it and closes the popup,
            // so an out-of-range number closes it too rather than sitting there
            // looking broken — the editor's popup makes the same call.
            let submit: Rc<dyn Fn()> = Rc::new(move || {
                goto_step.update(|n| *n = n.wrapping_add(1));
            });
            let esc = close.clone();
            let input = edit_field(
                goto_query,
                FieldCfg {
                    placeholder: "",
                    autofocus: true,
                    font_size: theme::font_body,
                    border_radius: 6.0,
                    height: Some(field_input_h),
                    on_submit: Some(submit),
                    on_escape: Some(Rc::new(move || (esc)())),
                    ..Default::default()
                },
            )
            // Wider than the editor's: a row number runs to six figures where a
            // line number rarely leaves three.
            .style(|s| s.width(theme::scaled(78.0)));
            let close_x = close.clone();
            let close_btn = container(icons::icon(icons::X, 14.0))
                .on_click_stop(move |_| (close_x)())
                .style(|s| {
                    s.items_center()
                        .color(theme::text_dim())
                        .hover(|s| s.color(theme::text()))
                });
            h_stack((
                text("Go to row:")
                    .style(|s| s.font_size(theme::font_label()).color(theme::text_dim())),
                input,
                close_btn,
            ))
            .style(|s| {
                s.items_center()
                    .gap(theme::scaled(8.0))
                    .padding_horiz(theme::scaled(8.0))
                    .padding_vert(theme::scaled(6.0))
                    .background(theme::bg_panel())
                    .border(1.0)
                    .border_color(theme::border())
                    .border_radius(8.0)
            })
            .into_any()
        },
    )
    .style(|s| s.absolute().inset_top(5.0).inset_right(5.0))
}

fn grid_view(rs: Arc<ResultSet>, gctx: GridCtx) -> impl IntoView {
    let ncols = rs.col_count();
    let nrows = rs.row_count();
    // Per-column key roles (snapshot from the source table's schema at build).
    let key_map = Arc::new(column_key_map(&rs, gctx.source, gctx.db_nodes));
    let elapsed = rs.elapsed_ms;
    let truncated = rs.truncated;
    // Named, not indexed: the toolbar has to say *which* column went blank.
    let capped_columns: Vec<String> = rs
        .capped_columns
        .iter()
        .filter_map(|&i| rs.columns.get(i).map(|c| c.name.clone()))
        .collect();
    // Signals for the identity-colour rule at the table's top edge (below the
    // toolbar), captured before `gctx` fields move into the closures below.
    let (connections, active_conn) = (gctx.connections, gctx.active_conn);

    // Interactive state, created once and shared across sort rebuilds.
    let gs = GridState::new(rs.clone(), &gctx, &key_map);
    // Resolve the source table's foreign keys → per-column "Follow" specs (once).
    gs.follow
        .set(Rc::new(build_follow_specs(&rs, gctx.source, gctx.db_nodes)));
    // Editability: which columns can be written back, and each base table's
    // WHERE key — derived from the result's per-column provenance + schema. The
    // closure looks up a base table's schema from the live `db_nodes` signals.
    // A read-only connection yields an *empty* model (nothing editable/committable)
    // — recomputed reactively so toggling read-only live disables/enables editing.
    let db_nodes = gctx.db_nodes;
    let read_only = gctx.read_only;
    let rs_model = rs.clone();
    create_effect(move |_| {
        let model = if read_only.get() {
            EditModel::default()
        } else {
            analyze_edit(&rs_model, |db, ns, table| {
                db_nodes.with_untracked(|nodes| {
                    nodes.iter().find(|n| n.database == db).and_then(|n| {
                        match n.schema.get_untracked() {
                            SchemaState::Loaded(s) => s
                                .tables
                                .iter()
                                // Match the namespace too: a database may hold
                                // same-named tables in two PostgreSQL schemas,
                                // and picking the wrong one hands the write the
                                // wrong key columns.
                                .find(|t| t.name == table && t.schema.as_deref() == ns)
                                .cloned(),
                            _ => None,
                        }
                    })
                })
            })
        };
        gs.edit_model.set(Arc::new(model));
    });
    // Which control each column's cells edit with. Its own effect, tracking the
    // schema signals `column_editors` reads: a database whose introspection lands
    // *after* the grid was built upgrades a text field into a dropdown or a
    // calendar in place, rather than leaving the result it arrived too late for
    // editing as text until the next run.
    let rs_editors = rs.clone();
    create_effect(move |_| {
        let next = column_editors(&rs_editors, db_nodes, gs.dialect);
        // **Only when it actually changed.** A signal never dedups, and this
        // effect re-runs on any schema movement at all — a refresh that comes
        // back identical included. The row panel is keyed on this value, so an
        // unchanged re-set would rebuild it under the user and take the fields
        // they were typing into with it.
        if gs.editors.with_untracked(|cur| cur.as_slice() != next) {
            gs.editors.set(Rc::new(next));
        }
    });

    // ── What the capped notice compares against ──────────────────────────────
    // `1,000 of ~4.2m` — what the result *would* have held. Only sayable when the
    // loaded rows are a capped read of a whole table, which is the strict half:
    // a table's row estimate is a total this query had only if the query took the
    // table entire (`intel::full_table_source`), and `grid_query` must be empty
    // because a spliced filter re-runs a statement that is not `base_sql`.
    //
    // Resolved once here, from the SQL that produced this result — a fresh run
    // rebuilds the grid, so an untracked read is the snapshot, not a stale one.
    let result_db = rs.database.clone();
    let scanned: Option<(String, Option<String>, String)> =
        if truncated && gs.grid_query.with_untracked(|q| q.is_empty()) {
            gs.base_sql.with_untracked(|sql| {
                let t = schemaic_core::intel::full_table_source(sql.as_deref()?, gs.dialect)?;
                let (database, schema) = schemaic_core::stats::catalogue_key(
                    gs.dialect,
                    t.qualifier.as_deref(),
                    result_db.as_deref(),
                )?;
                Some((database, schema, t.name))
            })
        } else {
            None
        };
    // A capped result is the moment the total is worth a catalogue query, so ask
    // for one — the app no-ops if this database's figures are already in hand or
    // in flight. In an effect rather than inline so the request lands after this
    // build, not during it (the same reason `properties_overlay` fetches from
    // one).
    //
    // **And again whenever a refresh throws the figures away.** A schema Refresh
    // resets every `ConnNode::stats` to `Idle`; the memo below is live on that
    // slot and drops the total the moment it happens, so an ask that ran only at
    // build time meant `of ~2.84m` vanished from an unchanged on-screen result and
    // never came back — unless the opt-in size column happened to be on *and* that
    // database expanded, which is the tree's own refetch, not this one. Tracking
    // `stats_gen` is what makes the two halves symmetric; a repeated ask is free
    // (the slot is the guard).
    if let Some((database, ..)) = scanned.clone() {
        let ask = gctx.db_stats.clone();
        let conn_id = gctx.conn_id;
        let stats_gen = gctx.stats_gen;
        create_effect(move |_| {
            stats_gen.track();
            (ask)(conn_id.get_untracked(), database.clone());
        });
    }
    // Reactive, so the figure appears when that fetch lands. The node lookup is
    // tracked too (unlike `crate::db_stats_slot`'s): a connection-wide refresh
    // replaces the whole node list, and a slot captured from the old one is a
    // disposed signal.
    let db_nodes_stats = gctx.db_nodes;
    let (tab_conn, active_conn_stats) = (gctx.conn_id, gctx.active_conn);
    let row_total: Memo<Option<schemaic_core::stats::RowCount>> = create_memo(move |_| {
        let (database, schema, table) = scanned.as_ref()?;
        // `db_nodes` is the *active* connection's tree. A tab bound to another
        // one names its databases in the same words and would read a different
        // server's figures out of it.
        if tab_conn.get() != active_conn_stats.get() {
            return None;
        }
        let slot = db_nodes_stats.with(|nodes| {
            nodes
                .iter()
                .find(|n| n.database == *database)
                .map(|n| n.stats)
        })?;
        slot.with(|st| match st {
            crate::DbStatsState::Loaded(set) => set
                .find(schema.as_deref(), table)
                .and_then(schemaic_core::stats::TableStats::row_count),
            _ => None,
        })
    });

    // Column-highlight request from a schema-tree column double-click: select the
    // whole named column (header + every cell) and scroll it into view, then clear
    // the request. An effect — not a one-shot read — so re-requesting the highlight
    // on an already-open tab (which rebuilds this grid) still fires. The scroll is
    // deferred a tick so the panes have mounted and `gs.vp` is measured (a fresh
    // build runs this effect before the first layout, when the viewport is zero).
    let highlight_col = gctx.highlight_col;
    let rs_hl = rs.clone();
    create_effect(move |_| {
        let Some(name) = highlight_col.get() else {
            return;
        };
        highlight_col.set(None);
        let Some(ci) = rs_hl.columns.iter().position(|c| c.name == name) else {
            return;
        };
        // Anchor at the last row, active at the first, so the whole column is
        // selected while the scroll target (active) keeps the view at the top.
        let last = nrows.saturating_sub(1);
        floem::action::exec_after(std::time::Duration::ZERO, move |_| {
            // The grid may have been disposed (tab switched/closed) before this
            // tick — bail if its signals are freed rather than panicking on a read.
            if gs.active.try_get_untracked().is_none() {
                return;
            }
            gs.anchor.set(Some((last, ci)));
            gs.active.set(Some((0, ci)));
            scroll_active_into_view(gs, 0, ci);
            if let Some(f) = gs.focus_id.get_untracked() {
                f.request_focus();
            }
        });
    });

    // **Carry the column widths across an interface-scale change.** Everything
    // else in the grid follows the scale by itself, because its style closures
    // call the scaled fns and reading one there subscribes the closure. Stored
    // pixels cannot: `gs.widths` is seeded once and the rebuild key below has no
    // scale term, so raising the scale with a result open used to grow every
    // cell's text inside columns still cut for the old size.
    //
    // An effect rather than a scale term in the rebuild key, deliberately: keying
    // the grid on the scale would discard the whole child scope and take the
    // selection, the scroll position and every staged edit with it. This
    // recomputes one signal. `grid_char_w()` is read *inside* the effect, so the
    // effect is what subscribes.
    create_effect(move |_| {
        let now = grid_char_w();
        let before = gs.widths_at.get_untracked();
        if now == before {
            return;
        }
        gs.widths_at.set(now);
        let floor = min_col_w();
        let ratio = now / before;
        gs.widths.update(|w| *w = rescale_widths(w, ratio, floor));
    });

    // Horizontal offset shared between the header and the body so columns stay
    // aligned as the body scrolls sideways. Persists across sort rebuilds.
    let h_off = RwSignal::new(0.0_f64);
    // Authoritative vertical offset published by the data pane; the frozen pane
    // follows it. Kept separate from `gs.scroll_to` (the keyboard/gutter command
    // channel) so no single scroll view both reads and writes the same signal —
    // that would re-enter layout and hang.
    let vscroll = RwSignal::new(0.0_f64);
    // Click a header to sort by that column (ASC → DESC → reset).
    let sort: RwSignal<SortState> = RwSignal::new(None);

    // **The app's first focus ring outside an overlay.** It holds the toolbar
    // strip only: F6 enters it from the grid body, arrows and Tab walk it,
    // Escape returns to the grid. A ring wraps, which is what makes a modal's Tab
    // order a trap — here that same property keeps the walk inside the strip
    // instead of falling into floem's whole-window traversal, and Escape is the
    // deliberate way out rather than the last Tab.
    let strip = crate::widgets::FocusRing::new();
    let toolbar = grid_toolbar(
        gs,
        nrows,
        ncols,
        elapsed,
        truncated,
        capped_columns,
        sort,
        rs.database.clone(),
        row_total,
        strip.clone(),
    );

    // Header + body rebuild together on a sort change OR a freeze toggle (both
    // repartition the columns between the frozen pane and the scrolling pane).
    // Layout is two panes side by side: a frozen pane (row-number gutter + an
    // optional frozen first column) and a horizontally-scrolling data pane. Both
    // panes are vertical scrolls kept in lockstep through `gs.scroll_to` (the
    // shared offset — data pane also owns the horizontal `h_off`).
    // The body rebuilds on every sort/freeze/new-row change, so the ring is cloned
    // per build rather than captured once.
    let strip_for_body = strip.clone();
    let grid = dyn_container(
        // Rebuild on sort / freeze change, and when the number of pending new rows
        // changes (adding/removing a row extends the virtual-stack length).
        move || (sort.get(), gs.frozen.get(), gs.new_rows.with(|v| v.len())),
        move |(sort_val, frozen_col, new_len)| {
            let strip_entry = strip_for_body.clone();
            let rs = gs.rs.get_untracked();
            // Total displayed rows = real rows + pending new rows (rendered below).
            let total = nrows + new_len;
            // The frozen column (if any), clamped to the valid range. The data
            // pane renders every *other* column, in order — cells keep their
            // absolute `ci`, so selection/sort/resize stay consistent.
            let frozen_col = frozen_col.filter(|&c| c < ncols);
            let data_cols: Arc<Vec<usize>> =
                Arc::new((0..ncols).filter(|ci| Some(*ci) != frozen_col).collect());
            let order = Arc::new(compute_order(&rs, sort_val));
            gs.order.set(order.clone());

            // Column virtualization: the window of `data_cols` intersecting the
            // horizontal viewport (+ overscan). A memo, so it recomputes on scroll
            // but — because `create_memo` dedups on `PartialEq` — only *notifies*
            // (rebuilding header + row cells) when the visible column set actually
            // changes, not every pixel. The header and every data row read this
            // SAME `win`, so the two panes stay column-aligned.
            let win_cols = data_cols.clone();
            let win: Memo<ColWindow> = create_memo(move |_| {
                gs.widths
                    .with(|w| compute_window(gs.vp.get(), w, &win_cols, 2))
            });

            // How tall the two bodies lay themselves out: the rows plus the
            // **virtual space** under them, so the last row can be scrolled up to
            // the top instead of sitting on the bottom edge (`body_scroll_h` — the
            // SQL editor's rule, and the wheel clamps below share it).
            //
            // This *overrides* the height `virtual_stack` gives itself (the bare
            // `rows × row_h`), which is what floem's scroll measures its range
            // against — a `margin_bottom` would not do: `Scroll::child_size` reads
            // the child's layout size, and margin falls outside it.
            //
            // A memo for the same reason `win` is one: it recomputes on every
            // scroll but, dedupped on `PartialEq`, only *notifies* — and so only
            // re-lays the bodies out — when the pane is actually resized. Both
            // panes read this SAME height; a frozen pane one row shorter would
            // clamp its own `scroll_to` early and drift out of line with the data
            // rows exactly when the virtual space came into view.
            let body_h: Memo<f64> = create_memo(move |_| {
                let rh = row_h();
                body_scroll_h(total as f64 * rh, gs.vp.get().height(), rh)
            });

            // ── Headers ──
            let gutter_header = container(text("#").style(|s| {
                s.font_size(theme::scaled_font(11.0))
                    .color(theme::text_faint())
            }))
            .style(|s| {
                s.width(gutter_w())
                    .height(grid_header_h())
                    .flex_shrink(0.0_f32)
                    .items_center()
                    .justify_end()
                    .padding_horiz(theme::scaled(8.0))
                    .border_right(1.0)
                    .border_color(theme::border())
                    .background(theme::bg_header_row())
            });
            let mut fhead: Vec<AnyView> = vec![gutter_header.into_any()];
            if let Some(fc) = frozen_col {
                fhead.push(header_cell(gs, fc, sort_val, sort, key_map.clone()).into_any());
            }
            let frozen_header = h_stack_from_iter(fhead).style(|s| {
                s.flex_row()
                    .flex_shrink(0.0_f32)
                    .background(theme::bg_header_row())
            });
            let km = key_map.clone();
            let hdr_cols = data_cols.clone();
            // Virtualized header: leading spacer + the visible window's header cells
            // + trailing spacer, rebuilt (via `win`) only when the visible column
            // set changes. Same window + spacers as the body rows keep them aligned.
            let data_header_row = dyn_container(
                move || win.get(),
                move |w| {
                    let mut kids: Vec<AnyView> =
                        vec![col_spacer(w.left_pad, grid_header_h()).into_any()];
                    for k in w.start..w.end {
                        kids.push(
                            header_cell(gs, hdr_cols[k], sort_val, sort, km.clone()).into_any(),
                        );
                    }
                    kids.push(col_spacer(w.right_pad, grid_header_h()).into_any());
                    h_stack_from_iter(kids)
                        .style(|s| s.flex_row().background(theme::bg_header_row()))
                        .into_any()
                },
            )
            .style(|s| s.flex_row().background(theme::bg_header_row()));
            let wheel_cols = data_cols.clone();
            let data_header = scroll(data_header_row)
                .scroll_to(move || Some(Point::new(h_off.get(), 0.0)))
                // A pure follower of `h_off`, which only the data pane writes — so
                // like the frozen pane it must not scroll itself. `propagate_pointer_wheel`
                // is not that guard: floem applies the delta *first* and propagates
                // only if the viewport didn't move, so a vertical delta propagated
                // (a 40px-tall header has no room) while a **horizontal** one
                // scrolled the header alone and stayed there. Every header cell then
                // sat over the wrong column — including its sort target and its
                // resize divider — until the body happened to be scrolled.
                //
                // Forward both axes into the shared channel the data pane follows
                // instead, exactly as the frozen pane forwards its vertical wheel.
                .on_event(EventListener::PointerWheel, move |e| {
                    if let Event::PointerWheel(pe) = e {
                        let vp = gs.vp.get_untracked();
                        let content_w = gs.widths.with_untracked(|w| {
                            wheel_cols
                                .iter()
                                .map(|&ci| w.get(ci).copied().unwrap_or(0.0))
                                .sum::<f64>()
                        });
                        let max_x = (content_w - vp.width()).max(0.0);
                        // Against the bodies' laid-out height, virtual space and
                        // all — clamping to the rows alone would stop the wheel a
                        // viewport early over the header, and only there.
                        let max_y = (body_h.get_untracked() - vp.height()).max(0.0);
                        gs.scroll_to.set(Some(Point::new(
                            (h_off.get_untracked() + pe.delta.x).clamp(0.0, max_x),
                            (vscroll.get_untracked() + pe.delta.y).clamp(0.0, max_y),
                        )));
                    }
                    EventPropagation::Stop
                })
                .scroll_style(|s| s.hide_bars(true))
                .style(|s| {
                    s.flex_grow(1.0_f32)
                        .height(grid_header_h())
                        .min_width(0.0)
                        .background(theme::bg_header_row())
                });
            let header = h_stack((frozen_header, data_header))
                .style(|s| s.flex_row().width_full().height(grid_header_h()));

            // ── Bodies ──
            let (grid_shown, grid_poke) = autohide_state();
            let order_f = order.clone();
            let frozen_body = scroll(
                virtual_stack(
                    VirtualDirection::Vertical,
                    VirtualItemSize::Fixed(Box::new(row_h)),
                    move || RowRange { len: total },
                    |i| *i,
                    move |i| {
                        if i < nrows {
                            frozen_row(gs, i, order_f[i], frozen_col, ncols, None)
                        } else {
                            frozen_row(gs, i, 0, frozen_col, ncols, Some(i - nrows))
                        }
                    },
                )
                .style(move |s| s.flex_col().height(body_h.get())),
            )
            .scroll_to(move || Some(Point::new(0.0, vscroll.get())))
            // Pure follower of `vscroll` (written by the data pane): NO `on_scroll`
            // — a view must never both read and write the offset it's driven by, or
            // the two authorities fight during layout and re-enter forever. It must
            // not scroll *itself* on the wheel either (it would desync from the data
            // pane, which has no way to follow it). But swallowing the wheel outright
            // left the gutter/frozen column a dead zone — hovering it while scrolling
            // did nothing. Instead we forward the wheel to the shared scroll channel
            // the data pane follows (`gs.scroll_to`, same one keyboard nav uses): the
            // data pane scrolls, republishes `vscroll`, and this pane follows. Floem
            // applies `child_viewport + delta` in pixels, so reusing `delta.y` with
            // the same sign matches the native data-pane feel exactly.
            .on_event(EventListener::PointerWheel, move |e| {
                if let Event::PointerWheel(pe) = e {
                    let dy = pe.delta.y;
                    if dy != 0.0 {
                        let vp = gs.vp.get_untracked();
                        // Same height the bodies lay out to — see the header's twin.
                        let max_y = (body_h.get_untracked() - vp.height()).max(0.0);
                        let new_y = (vscroll.get_untracked() + dy).clamp(0.0, max_y);
                        gs.scroll_to.set(Some(Point::new(vp.x0, new_y)));
                    }
                }
                EventPropagation::Stop
            })
            .scroll_style(|s| s.hide_bars(true))
            .style(move |s| {
                let w = gutter_w()
                    + match frozen_col {
                        Some(fc) => gs.widths.with(|w| w.get(fc).copied().unwrap_or(0.0)),
                        None => 0.0,
                    };
                s.width(w)
                    .flex_shrink(0.0_f32)
                    .min_height(0.0)
                    .border_top(1.0)
                    .border_color(theme::border())
            });

            let order_d = order.clone();
            let body_cols = data_cols.clone();
            let data_body = shift_hscroll(
                virtual_stack(
                    VirtualDirection::Vertical,
                    VirtualItemSize::Fixed(Box::new(row_h)),
                    move || RowRange { len: total },
                    |i| *i,
                    move |i| {
                        if i < nrows {
                            data_row(gs, i, order_d[i], body_cols.clone(), None, win)
                        } else {
                            data_row(gs, i, 0, body_cols.clone(), Some(i - nrows), win)
                        }
                    },
                )
                .style(move |s| s.flex_col().height(body_h.get())),
            )
            .scroll_to(move || gs.scroll_to.get())
            .on_scroll(move |rect| {
                if (h_off.get_untracked() - rect.x0).abs() > 0.5 {
                    h_off.set(rect.x0);
                }
                // Publish the vertical offset for the frozen pane to follow. This
                // pane never writes `gs.scroll_to` (which it reads via `scroll_to`)
                // — doing so would re-enter its own layout and hang.
                if (vscroll.get_untracked() - rect.y0).abs() > 0.5 {
                    vscroll.set(rect.y0);
                }
                gs.vp.set(rect);
                grid_poke();
            })
            // Seed the viewport size before any scroll happens (keeping the
            // current scroll offset), so Page keys / scroll-into-view work on the
            // first keypress rather than being dead until the first scroll (§7.4).
            .on_resize(move |rect| {
                let cur = gs.vp.get_untracked();
                gs.vp.set(Rect::from_origin_size(cur.origin(), rect.size()));
            })
            .scroll_style(move |s| thin_scroll(s).hide_bars(!grid_shown.get()))
            .keyboard_navigable()
            // Ends a drag-select. On the body rather than the cells because the
            // release routinely lands outside the cell the drag began in — and
            // past the last row, or outside the grid entirely.
            .on_event_cont(EventListener::PointerUp, move |_| {
                gs.selecting.set(false);
                gs.row_selecting.set(false);
            })
            .on_event(EventListener::KeyDown, move |e| {
                // **F6 steps out to the toolbar** — the OS-conventional "next
                // pane" key, and free here where every other reflex is taken
                // (Tab hops cells while editing, the arrows move the selection,
                // Escape closes the find/goto bars). Handled at the view rather
                // than in `grid_key` because it is about *focus*, not grid state.
                //
                // `step_from` with the body's own id, which is not a ring member:
                // it enters at the first control, or resumes where the strip was
                // last left. It also arms `keyboard_nav`, so the ring it lands on
                // is visible.
                if let Event::KeyDown(ke) = e
                    && ke.key.logical_key == Key::Named(NamedKey::F6)
                    && let Some(from) = gs.focus_id.get_untracked()
                {
                    strip_entry.step_from(from, false);
                    return EventPropagation::Stop;
                }
                grid_key(gs, nrows, ncols, e)
            })
            .style(|s| {
                s.flex_grow(1.0_f32)
                    .min_height(0.0)
                    .min_width(0.0)
                    .border_top(1.0)
                    .border_color(theme::border())
            });
            gs.focus_id.set(Some(data_body.id()));
            // **And this is where the keyboard goes when a control disappears while
            // focused with no modal open** — the toolbar's ✓/✗ pressed from the
            // keyboard being the case that lost it (see
            // `widgets::set_keyboard_home`). Registered here rather than beside the
            // toolbar because `focus_id` is what `refocus_grid` uses and this is
            // where it is set: the arrows, `Del`, `Ctrl+Enter` and `F6` are all
            // listeners on this body, so it is the only place the grid's keyboard
            // actually lives.
            crate::widgets::set_keyboard_home(Some(std::rc::Rc::new(move || refocus_grid(gs))));

            let body = h_stack((frozen_body, data_body)).style(|s| {
                s.flex_row()
                    .flex_grow(1.0_f32)
                    .width_full()
                    .min_height(0.0)
                    .min_width(0.0)
            });

            v_stack((header, body))
                // Ends a drag-select **wherever in the grid the button comes
                // up**. The data body has its own copy for the common case, but
                // it is one of several siblings: floem dispatches a pointer event
                // to the first hit child in reverse paint order and stops, so a
                // release over the frozen pane, the header, or past the last row
                // never reached it. Releases *outside* the grid entirely — the
                // status bar, the schema panel, the results toolbar — are the
                // root's `pointer_released`, tracked by the effect beside the
                // find bar below. There is deliberately no pointer capture (it
                // would stop the other cells' `PointerEnter`, which *is* the
                // drag).
                .on_event_cont(EventListener::PointerUp, move |_| {
                    gs.selecting.set(false);
                    gs.row_selecting.set(false);
                })
                .style(|s| {
                    s.flex_grow(1.0_f32)
                        .width_full()
                        .flex_col()
                        .min_height(0.0)
                        .min_width(0.0)
                })
                .into_any()
        },
    )
    // `flex_basis(0)`: the grid fills only the space *left over* after the (auto-
    // height) value viewer, instead of using its content as the basis and refusing
    // to shrink — so a tall viewer takes room from the grid, not off-screen. A
    // `min_height` keeps some grid visible even when the viewer is large (the
    // viewer's cap is sized to respect this, so nothing overflows).
    .style(|s| {
        s.flex_grow(1.0_f32)
            .flex_basis(0.0)
            .width_full()
            .flex_col()
            .min_height(theme::scaled(120.0))
            .min_width(0.0)
    });

    // In-grid find (Ctrl+F). The bar itself is rendered up at the RESULTS-panel
    // level (`results_section`) so it can sit at the panel's top edge; here we only
    // drive the search against the row data. Incremental as the query changes:
    create_effect(move |_| {
        if !gs.find_open.get() {
            return;
        }
        let _ = gs.find_query.get();
        grid_find(gs, true, true);
    });
    // Directional next/prev: the bar bumps `find_step` (nonce, forward).
    let find_step = gctx.find_step;
    create_effect(move |_| {
        let (_, forward) = find_step.get();
        grid_find(gs, forward, false);
    });
    // Closing the find bar has to hand the keyboard back to the grid.
    //
    // Escape and the ✕ only flip `find_open`, which disposes the field's view —
    // and floem clears `app_state.focus` **silently** on removal (no
    // `FocusGained` lands anywhere), so the grid was left focused on nothing and
    // the next Ctrl+F reached nobody until the user clicked a cell. Watching the
    // flag rather than patching the bar's `close()` covers every way the bar can
    // shut, and is the only place that *can*: the bar is built a level up in
    // `results_section`, where `focus_id` doesn't exist.
    //
    // Only on a true→false edge, so the build-time run doesn't steal focus from
    // whatever the user is actually typing in.
    create_effect(move |was_open: Option<bool>| {
        let open = gs.find_open.get();
        if was_open == Some(true)
            && !open
            && let Some(f) = gs.focus_id.get_untracked()
        {
            f.request_focus();
        }
        open
    });
    // Go to row (Ctrl+G). The popup bumps `goto_step` on Enter; the jump is here
    // because the row count is, and it counts what the gutter *numbers*, so
    // "row N" means the same thing typed as it does read.
    //
    // Selecting the whole row rather than a cell is the same gesture a gutter
    // click makes (anchor at column 0, active at the last), so the row lights up
    // the way the user already knows. The scroll is asked for at column 0: a jump
    // should not also fling the viewport to the far right, which is what following
    // the *active* cell would do.
    create_effect(move |seen: Option<u64>| {
        let step = gs.goto_step.get();
        if !goto_fires(seen, step) {
            return step;
        }
        // `nrows` — the **numbered** rows only. A pending unsaved row's gutter
        // reads `*`, not a number, so counting them in made "row 101" land on a
        // row showing no number at all, and made a row of 9s stop one short of
        // the last row that does.
        let target = gs
            .goto_query
            .with_untracked(|q| schemaic_core::model::goto_target(q, nrows, ncols));
        if let Some(t) = target {
            gs.anchor.set(Some(t.anchor));
            gs.active.set(Some(t.active));
            scroll_active_into_view(gs, t.active.0, t.scroll_col);
        }
        // Always close, even on a miss — the editor's go-to-line does the same,
        // and a popup that stays open after Enter reads as "still working".
        // Closing is what hands the keyboard back, via the effect below; doing it
        // here as well would be a second path to the same place.
        gs.goto_open.set(false);
        gs.goto_query.set(String::new());
        step
    });
    // Selection aggregates. Rendered by the panel-level bar; computed here,
    // because this is where the cells are. See [`selection_kind`] for which
    // column the arithmetic is about and when there is none.
    let sel_summary = gctx.sel_summary;
    create_effect(move |_| {
        // **Everything the number is computed from is tracked**, not only where
        // the selection is. The effect used to read `active`/`anchor` and take
        // `rs` and `order` untracked, so an in-memory sort — which rewrites
        // `order` and deliberately leaves the selection in display coordinates —
        // left the previous total standing under the same highlighted cells now
        // holding different values. A pure-`UPDATE` commit splice did the same,
        // and a staged edit was never in the total at all. The find-count effect
        // fifty lines below tracks `order` for exactly this reason.
        let (rs, order) = (gs.rs.get(), gs.order.get());
        gs.dirty.track();
        gs.new_rows.track();
        let kind = selection_kind(gs.bounds(), gs.anchor.get(), ncols);
        let anchor_col = match kind {
            SelKind::Nothing => {
                sel_summary.set(None);
                return;
            }
            // Count the span without reading a column nobody chose.
            SelKind::WholeRow => None,
            SelKind::Column(c) => Some(c),
        };
        let Some((r0, _, r1, _)) = gs.bounds() else {
            sel_summary.set(None);
            return;
        };
        let column = anchor_col.and_then(|c| rs.columns.get(c));
        if anchor_col.is_some() && column.is_none() {
            sel_summary.set(None);
            return;
        }
        let agg = match (anchor_col, column) {
            (Some(ci), Some(column)) => {
                // Read the cell **as the grid draws it**: a staged edit wins over
                // the stored value, and a pending new row has only staged cells.
                // The two branches also now count the same rows — the span — where
                // the per-column one used to drop every pending row on the floor
                // while the whole-row one kept them, so Ctrl+A and a drag over the
                // same five rows reported 5 and 3.
                gs.dirty.with_untracked(|dirty| {
                    gs.new_rows.with_untracked(|pending| {
                        let cells = (r0..=r1).map(|d| match order.get(d).copied() {
                            Some(di) => match dirty.get(&(di, ci)) {
                                Some(staged) => staged.as_deref(),
                                None => rs
                                    .cell(di, ci)
                                    .and_then(|c| (!c.is_null()).then(|| c.text())),
                            },
                            // Past the real rows: a pending row, whose unset cells
                            // are a server default rather than a value.
                            None => pending
                                .get(d - order.len())
                                .and_then(|m| m.get(&ci))
                                .and_then(|v| v.as_deref()),
                        });
                        schemaic_core::aggregate::aggregate_texts(column, cells)
                    })
                })
            }
            _ => schemaic_core::aggregate::Aggregates {
                rows: r1 - r0 + 1,
                // No column, so no cell to be NULL — the counts are all a row
                // selection can honestly report.
                non_null: r1 - r0 + 1,
                numeric: None,
            },
        };
        let label = match column {
            Some(column) => format!("{} · {}", column.name, agg.summary()),
            None => agg.summary(),
        };
        sel_summary.set(Some(label));
    });
    // **The rest of the window's pointer-ups.** A drag-select is armed by a
    // cell's `PointerDown` and continued by other cells' `PointerEnter`, which
    // is why it can't take pointer capture — capture would stop exactly those
    // events. So the release is delivered wherever the cursor happens to be, and
    // for a drag that leaves the table that is the status bar, the schema panel,
    // or the results panel's own toolbar and filter bar. None of them is inside
    // the grid, so none reaches the handler on it: the flag stayed armed, and
    // coming back over the rows with no button held kept extending the range.
    //
    // `widgets::pointer_released` is bumped by the workspace root, which every
    // release reaches. The guard matters as much as the clear — this effect runs
    // on **every** pointer-up in the app, and `set` never dedups, so an
    // unguarded write would notify every cell's style closure each time anyone
    // clicked anything.
    create_effect(move |_| {
        crate::widgets::pointer_released().track();
        if !gs.alive() {
            return;
        }
        if gs.selecting.get_untracked() {
            gs.selecting.set(false);
        }
        if gs.row_selecting.get_untracked() {
            gs.row_selecting.set(false);
        }
    });
    one_bar_at_a_time(gs.find_open, gs.goto_open);
    // And hand the keyboard back when the goto popup closes, for the same reason
    // the find bar does above.
    create_effect(move |was_open: Option<bool>| {
        let open = gs.goto_open.get();
        if was_open == Some(true)
            && !open
            && let Some(f) = gs.focus_id.get_untracked()
        {
            f.request_focus();
        }
        open
    });

    // Match count for the `pos/total` readout. The full grid scan is potentially
    // expensive (a String per cell), so it's DEBOUNCED off the keystroke path: each
    // query change schedules a scan ~150ms later, and a newer change supersedes it
    // (generation guard) — the selection still jumps instantly (effect above); only
    // the number lags briefly. `find_hits` (ascending linear positions) also lets
    // next/prev update `find_pos` without re-scanning.
    let find_hits: RwSignal<Arc<Vec<usize>>> = RwSignal::new(Arc::new(Vec::new()));
    let find_total = gctx.find_total;
    let find_more = gctx.find_more;
    let count_gen = Rc::new(std::cell::Cell::new(0u64));
    create_effect(move |_| {
        // Re-count when the query, sort order, or per-column formatters change.
        let _ = gs.find_query.get();
        let _ = gs.order.get();
        let _ = gs.formats.get();
        if gs.find_query.with(|q| q.is_empty()) {
            find_hits.set(Arc::new(Vec::new()));
            find_total.set(0);
            find_more.set(false);
            return;
        }
        let g = count_gen.get() + 1;
        count_gen.set(g);
        let gen_at = count_gen.clone();
        floem::action::exec_after(std::time::Duration::from_millis(150), move |_| {
            if gen_at.get() != g {
                return; // superseded by a newer query/order/format change
            }
            // The generation check above can't stand in for this: `gen_at` is an
            // `Rc<Cell>` owned by the closure, so it survives the grid's disposal
            // untouched and happily passes. 150 ms is comfortably inside a
            // Ctrl+Tab after a keystroke — this is the reproduced crash.
            if !gs.alive() {
                return;
            }
            let (hits, more) = grid_find_hits(gs);
            find_total.set(hits.len());
            find_more.set(more);
            find_hits.set(Arc::new(hits));
        });
    });
    // `find_pos` = 1-based rank of the active cell among the matches (0 if the
    // selection isn't on one). Recomputed reactively when the selection moves
    // (next/prev) or the hit list rebuilds — a binary search, no re-scan.
    let find_pos = gctx.find_pos;
    create_effect(move |_| {
        let hits = find_hits.get();
        let pos = match gs.active.get() {
            Some((dr, ci)) => {
                let lin = dr * ncols + ci;
                hits.binary_search(&lin).map(|i| i + 1).unwrap_or(0)
            }
            None => 0,
        };
        find_pos.set(pos);
    });

    // Max pixel height of the **whole** row panel — header, fields and status —
    // derived from the results-area height on the root `on_resize` below. It caps
    // the panel rather than just its list because the list is only one of three
    // children: capping the list let the chrome push the panel past the area, and
    // the overflow is what clipped the last fields (their scroll viewport ran off
    // the bottom, so scrolling could never bring them into view).
    let edit_row_max = RwSignal::new(320usize);
    // Wrap the grid so a 2px identity-colour rule can pin to its top edge (right
    // below the toolbar) without taking layout space — the "prominent colour"
    // setting. The box inherits the grid's growth so the table still fills.
    let grid_boxed = stack((
        grid,
        crate::conn_edge_border(connections, active_conn, true),
    ))
    .style(move |s| {
        s.flex_grow(1.0_f32)
            .flex_basis(0.0)
            .width_full()
            .flex_col()
            // The row panel is allowed to squeeze the table: while it's open the
            // user is editing a row, not reading the grid. Holding the floor here
            // is what made the panel's share unsatisfiable — flexbox honours a
            // min-height over a sibling's size, so the panel overflowed the area
            // instead of the grid giving way.
            .min_height(if gs.edit_row_open.get() { 0.0 } else { 120.0 })
            .min_width(0.0)
    });
    v_stack((
        toolbar,
        filter_bar(gs),
        grid_boxed,
        seed_popover(gs),
        edit_row_panel(gs, edit_row_max),
    ))
    .on_resize(move |r| {
        // The row panel may cover up to 70% of the results area, then its field
        // list scrolls. Floored so a short window still shows a few fields, and
        // ceilinged so the toolbar above it always has somewhere to sit.
        let h = r.height();
        let cap = (h * 0.70).max(140.0).min((h - 60.0).max(0.0)) as usize;
        if edit_row_max.get_untracked() != cap {
            edit_row_max.set(cap);
        }
    })
    .style(|s| {
        s.flex_grow(1.0_f32)
            .width_full()
            .flex_col()
            .min_height(0.0)
            .min_width(0.0)
    })
}

/// Handle a key press while the grid body is focused: move the active cell,
/// extend the selection (shift), copy (Ctrl+C), select-all, open the viewer.
/// Return keyboard focus to the grid body after an in-cell edit ends. Deferred
/// past the current event so the text_input's disposal (which would otherwise
/// grab focus back) doesn't leave the grid unable to receive arrow/Enter keys.
///
/// **The body's id is read inside the tick, never before it.** Floem's focus
/// request has no existence check — `UpdateMessage::Focus` assigns
/// `app_state.focus` whether or not the id still resolves — so a captured id
/// parks the keyboard on a removed view and *every* key is then dropped until a
/// click. The action that hands the keyboard back here is very often the same one
/// that rebuilt the body: the toolbar's ✗ discards, `discard_edits` clears
/// `new_rows`, and the body's `dyn_container` is keyed on its length, so the id
/// this held one line earlier was already gone by the time the tick landed. The
/// grid then answered no key at all — no arrows, and not the `F6` that would have
/// got back into the strip. `grid_toolbar`'s `focus_icon` states the same rule for
/// the menus, and resolves by tabindex for it.
fn refocus_grid(gs: GridState) {
    floem::action::exec_after(std::time::Duration::from_millis(0), move |_| {
        // `try_get_untracked`: this is also the workspace's registered keyboard
        // home (`widgets::set_keyboard_home`), which outlives the grid that
        // registered it — a tab switch disposes the grid's scope, and a read of a
        // freed signal is not a question with an answer. A disposed grid answers
        // `None` and nothing moves.
        if let Some(Some(f)) = gs.focus_id.try_get_untracked() {
            f.request_focus();
        }
    });
}

/// Should a blurred inline editor hand the keyboard back to the grid?
///
/// **floem's `text_input` answers Escape itself** — `handle_key_down` calls
/// `clear_focus()` and reports the key as handled, so the grid's own Escape
/// handler is never reached (a view's `event_before_children` runs before its
/// listeners, and a processed event stops there). All the grid hears is
/// `FocusLost`, and the keyboard is left on **nothing**: no arrows, no Enter, no
/// Ctrl+Enter, until the user clicks a cell. That is what this recovers.
///
/// But `FocusLost` is also what a click on anything else produces, and floem
/// exposes no way to ask where the focus went (`AppState::focus` is private, and
/// `focus_changed` has already assigned it by the time a listener runs). So the
/// answer is taken from the **pointer**: a blur with the cursor over the grid is
/// Escape, or a click on another cell, and both want the grid to keep the
/// keyboard; a blur with the cursor elsewhere is a click on that elsewhere, which
/// has just taken the keyboard and must keep it — the SQL editor above being the
/// one that would smart.
///
/// The corner it gets wrong is Escape pressed with the pointer parked outside the
/// grid, which leaves the keyboard where floem left it: a click away from
/// recovery, and no worse than before this existed. The honest fix is upstream —
/// a text input that lets its host answer Escape.
fn reclaim_keyboard(pointer: (f64, f64), grid: Rect) -> bool {
    grid.contains(Point::new(pointer.0, pointer.1))
}

/// Anything [`clear_if_any`] can ask whether it is already empty.
///
/// A trait rather than three call-site guards so the question is asked the same
/// way of every staging collection, `Option` included — an editor that is already
/// closed and a set that is already empty are the same case.
///
/// The bodies below read as infinite recursion and are not: an **inherent** method
/// wins name resolution over a trait method, so `self.is_empty()` in
/// `impl Clearable for Vec<T>` is `Vec::is_empty`. `clear_tests` calls all three,
/// which is what says so rather than the reader having to trust it.
trait Clearable {
    fn is_empty(&self) -> bool;
    fn clear(&mut self);
}

impl<T> Clearable for Option<T> {
    fn is_empty(&self) -> bool {
        self.is_none()
    }
    fn clear(&mut self) {
        *self = None;
    }
}

impl<T> Clearable for Vec<T> {
    fn is_empty(&self) -> bool {
        self.is_empty()
    }
    fn clear(&mut self) {
        self.clear();
    }
}

impl<K: Eq + std::hash::Hash, V> Clearable for HashMap<K, V> {
    fn is_empty(&self) -> bool {
        self.is_empty()
    }
    fn clear(&mut self) {
        self.clear();
    }
}

impl<T: Eq + std::hash::Hash> Clearable for HashSet<T> {
    fn is_empty(&self) -> bool {
        self.is_empty()
    }
    fn clear(&mut self) {
        self.clear();
    }
}

/// Empty a signal's collection, **notifying only if it held anything**.
///
/// `RwSignal::update` runs its subscribers unconditionally — floem_reactive's
/// `update_value` calls `run_effects()` with no equality check — so clearing what
/// is already empty is not the no-op it reads as: it rebuilds every
/// `dyn_container` keyed on the signal. `discard_edits` clears three collections
/// and the grid body is keyed on one of them, so discarding a single cell edit
/// tore the body down, recomputed the sort order over every row and built it
/// again, to arrive at the same `0` — and took the keyboard with it (see
/// [`refocus_grid`]). Unit-tested in `clear_tests`, including the floem fact.
fn clear_if_any<C: Clearable + 'static>(sig: RwSignal<C>) {
    if sig.with_untracked(|c| c.is_empty()) {
        return;
    }
    sig.update(|c| c.clear());
}

/// The result's source table, qualified, for an AI prompt's context — `None`
/// for an arbitrary SELECT that isn't backed by one table.
fn source_table(gs: GridState) -> Option<String> {
    gs.source
        .get_untracked()
        .map(|src| format!("{}.{}", src.database, src.display()))
}

/// Open the inline editor on the cell at display `(i, ci)`, seeding the buffer
/// with its current value (a staged edit if present, else the original).
fn start_edit(gs: GridState, i: usize, ci: usize) {
    let nrows = gs.rs.get_untracked().row_count();
    // A real row marked for deletion isn't editable (it's going away).
    if i < nrows {
        let di = gs.order.get_untracked().get(i).copied().unwrap_or(i);
        if gs.del_rows.with_untracked(|d| d.contains(&di)) {
            return;
        }
    }
    gs.active.set(Some((i, ci)));
    gs.anchor.set(Some((i, ci)));
    let seed = if i >= nrows {
        // Pending new row: seed from its staged cell (empty = "use default").
        match gs
            .new_rows
            .with_untracked(|rows| rows.get(i - nrows).and_then(|r| r.get(&ci).cloned()))
        {
            Some(Some(t)) => t,
            _ => String::new(),
        }
    } else {
        let order = gs.order.get_untracked();
        let di = order.get(i).copied().unwrap_or(i);
        let cur = gs.dirty.with_untracked(|d| d.get(&(di, ci)).cloned());
        match cur {
            Some(Some(t)) => t,          // staged text
            Some(None) => String::new(), // staged NULL → edit from empty
            None => gs
                .rs
                .get_untracked()
                .cell(di, ci)
                .filter(|c| !c.is_null()) // original NULL → edit from empty
                .map(|c| c.display().to_string())
                .unwrap_or_default(),
        }
    };
    gs.edit_buf.set(seed);
    gs.edit_cell.set(Some((i, ci)));
}

/// Start the clock on a write that has just been handed to the app: if it is
/// still in flight [`WRITE_WAIT_MS`] from now, say so — and name what of the
/// user's own might be holding it up.
///
/// The wait is the *point* of this, so there is no cancelling and no timeout;
/// the write keeps waiting and the note keeps standing until it returns. Called
/// by both write paths (the staged batch and the row panel's Save), and the
/// per-commit `commit_seq` is what stops an earlier commit's timer from
/// narrating a later one.
fn arm_wait_note(gs: GridState) {
    let seq = gs.commit_seq.get_untracked().wrapping_add(1);
    gs.commit_seq.set(seq);
    gs.commit_wait.set(None);
    floem::action::exec_after(
        std::time::Duration::from_millis(WRITE_WAIT_MS as u64),
        move |_| {
            // Fires later than the frame that scheduled it, so the grid may be
            // gone (tab switched, query re-run) — see `GridState::alive`.
            if !gs.alive()
                || gs.commit_seq.get_untracked() != seq
                || !gs.commit_busy.get_untracked()
            {
                return;
            }
            let holders = gs
                .tx_holders
                .get_untracked()
                .map(|f| (f)())
                .unwrap_or_default();
            gs.commit_wait.set(write_wait_note(WRITE_WAIT_MS, &holders));
        },
    );
}

/// Execute all staged edits (Ctrl+Enter or the toolbar ✓). The app runs them
/// transactionally, then — when the result is a spliceable single table — re-
/// fetches just the edited rows and hands them back so the grid splices them in
/// place (no re-run, scroll/selection preserved); otherwise it re-runs the whole
/// query. A failure is surfaced in `commit_err` and the staged edits are kept.
fn commit_grid(gs: GridState) {
    if gs.commit_busy.get_untracked() {
        return;
    }
    // Flush any open in-cell edit into `dirty` / `new_rows` first.
    if gs.edit_cell.get_untracked().is_some() {
        gs.commit_edit();
    }
    // The staged keys this write is assembled from. Nothing stops the user staging
    // another edit while the commit is in flight, and that one hasn't been written.
    let committed: HashSet<(usize, usize)> = gs.dirty.get_untracked().keys().copied().collect();
    let write = GridWrite {
        updates: gs.build_edits(),
        inserts: gs.build_inserts(),
        deletes: gs.build_deletes(),
    };
    if write.is_empty() {
        return;
    }
    let Some(commit) = gs.commit.get_untracked() else {
        return;
    };
    // An insert or delete changes row membership/ordering, so it can't splice in
    // place — force a full re-run (rows then land in their real positions). Pure
    // UPDATE commits still splice.
    let refetch = if write.inserts.is_empty() && write.deletes.is_empty() {
        gs.build_refetch()
    } else {
        None
    };
    gs.commit_busy.set(true);
    gs.commit_err.set(None);
    arm_wait_note(gs);
    let done: Rc<dyn Fn(CommitDone)> = Rc::new(move |outcome| {
        gs.commit_busy.set(false);
        gs.commit_wait.set(None);
        match outcome {
            // Fresh DB values for the edited rows — splice in place, keep scroll.
            CommitDone::Spliced(rows) => gs.apply_splice(rows, &committed),
            // The app re-ran the query; the grid is rebuilt fresh, nothing to do.
            CommitDone::FullReran => {}
            CommitDone::Failed(msg) => gs.commit_err.set(Some(msg)),
        }
    });
    (commit)(write, refetch, done);
}

/// Per-column context for the whole-row JSON editor: name, editability (PK /
/// expression / binary → read-only), nullability, and the row's original value.
fn row_colspecs(gs: GridState, di: usize) -> Vec<ColSpec> {
    let rs = gs.rs.get_untracked();
    let model = gs.edit_model.get_untracked();
    rs.columns
        .iter()
        .enumerate()
        .map(|(ci, c)| ColSpec {
            name: c.name.clone(),
            editable: model.editable(ci),
            nullable: c
                .origin
                .as_ref()
                .map(|o| !o.flags.not_null)
                .unwrap_or(false),
            value: rs
                .cell(di, ci)
                .map(|cell| cell.to_value())
                .unwrap_or(Value::Null),
        })
        .collect()
}

/// Point the view/edit panel at data-row `di` and clear any error. The per-field
/// editors are rebuilt from the row by the panel's `dyn_container` (keyed on the
/// row), so walking to another row here discards unsaved edits. Does NOT touch
/// `edit_row_open` (the caller owns opening).
fn load_edit_row(gs: GridState, di: usize) {
    gs.edit_row_di.set(Some(di));
    // Through `clear_bar`, not a bare `commit_err.set(None)`: the bar grew a
    // second surface, and a note about a paste three edits ago is exactly what
    // the copies this replaced left standing.
    gs.clear_bar();
}

fn open_edit_row(gs: GridState, di: usize) {
    load_edit_row(gs, di);
    gs.seed_open.set(false);
    gs.edit_row_open.set(true);
}

/// Display position (0-based) of data row `di` in the current sort order; the
/// identity when unsorted (`order` empty). Mirrors the `row_no` math.
fn edit_row_disp(gs: GridState, di: usize) -> usize {
    gs.order
        .get_untracked()
        .iter()
        .position(|&d| d == di)
        .unwrap_or(di)
}

/// Walk the view panel to the previous/next real row in display order, discarding
/// any unsaved edits (the buffer is reloaded). No-op at the first/last row.
fn edit_row_step(gs: GridState, forward: bool) {
    let Some(di) = gs.edit_row_di.get_untracked() else {
        return;
    };
    let nrows = gs.rs.get_untracked().row_count();
    let disp = edit_row_disp(gs, di);
    let new_disp = if forward {
        if disp + 1 >= nrows {
            return;
        }
        disp + 1
    } else {
        if disp == 0 {
            return;
        }
        disp - 1
    };
    let new_di = gs
        .order
        .get_untracked()
        .get(new_disp)
        .copied()
        .unwrap_or(new_disp);
    load_edit_row(gs, new_di);
}

/// Commit the panel's per-field `state` for existing row `di` as an `UPDATE`,
/// immediately (its own path — not the staged `dirty` batch). `state` is each
/// field's current value (`None` = SQL NULL). On a validation error or DB failure
/// the message shows in the panel and it stays open; on success the panel closes and
/// the row splices in place.
fn commit_row_update(
    gs: GridState,
    di: usize,
    cols: &[ColSpec],
    state: Vec<(usize, Option<String>)>,
) {
    if gs.edit_row_saving.get_untracked() || gs.commit_busy.get_untracked() {
        return;
    }
    let changes = match rowjson::update_changes(cols, &state) {
        Ok(c) => c,
        Err(msg) => {
            gs.commit_err.set(Some(msg));
            return;
        }
    };
    let updates = gs.build_row_edits(di, &changes);
    if updates.is_empty() {
        gs.edit_row_open.set(false); // nothing changed
        return;
    }
    // This path writes only this row's changed columns, so only those leave the
    // staged map — a green edit anywhere else is still uncommitted and stays.
    let committed: HashSet<(usize, usize)> = changes.iter().map(|(ci, _)| (di, *ci)).collect();
    let Some(commit) = gs.commit.get_untracked() else {
        return;
    };
    let write = GridWrite {
        updates,
        inserts: Vec::new(),
        deletes: Vec::new(),
    };
    let refetch = gs.build_row_refetch(di, &changes);
    gs.edit_row_saving.set(true);
    gs.commit_busy.set(true);
    gs.commit_err.set(None);
    arm_wait_note(gs);
    let done: Rc<dyn Fn(CommitDone)> = Rc::new(move |outcome| {
        gs.commit_busy.set(false);
        gs.commit_wait.set(None);
        gs.edit_row_saving.set(false);
        match outcome {
            CommitDone::Spliced(rows) => {
                gs.apply_splice(rows, &committed);
                gs.edit_row_open.set(false);
            }
            CommitDone::FullReran => gs.edit_row_open.set(false),
            CommitDone::Failed(msg) => gs.commit_err.set(Some(msg)),
        }
    });
    (commit)(write, refetch, done);
}

/// The next (`forward`) / previous editable column after `ci`, if any — used to
/// hop between cells while filling a row with Tab / Enter.
fn next_editable_col(gs: GridState, ci: usize, forward: bool) -> Option<usize> {
    let model = gs.edit_model.get_untracked();
    let ncols = gs.rs.get_untracked().col_count();
    if forward {
        (ci + 1..ncols).find(|&c| model.editable(c))
    } else {
        (0..ci).rev().find(|&c| model.editable(c))
    }
}

/// Stage the in-progress edit at display row `i`, column `ci`, then hop to the
/// next/prev editable cell in the same row (Tab / Enter data entry). When there's
/// no next cell, close the editor and return focus to the grid.
fn advance_edit(gs: GridState, i: usize, ci: usize, pending: Option<usize>, forward: bool) {
    let v = gs.edit_buf.get_untracked();
    match pending {
        Some(p) => gs.stage_new(p, ci, Some(v)),
        None => {
            let order = gs.order.get_untracked();
            let di = order.get(i).copied().unwrap_or(i);
            gs.stage(di, ci, Some(v));
        }
    }
    match next_editable_col(gs, ci, forward) {
        Some(nc) => {
            start_edit(gs, i, nc);
            scroll_active_into_view(gs, i, nc);
        }
        None => {
            gs.edit_cell.set(None);
            refocus_grid(gs);
        }
    }
}

/// Duplicate data-row `data_idx` into a new pending row (right-click "Duplicate
/// row"): pre-filled from its values (minus auto-increment), then scrolled into
/// view + selected. Not auto-opened for editing — it's already populated, so the
/// user tweaks what they need (e.g. a natural key) and commits.
fn clone_row(gs: GridState, data_idx: usize) {
    clone_rows(gs, &[data_idx]);
}

/// [`clone_row`] for a gutter selection: one `new_rows` write for the whole
/// batch, one scroll, one selection — see [`GridState::add_cloned_rows`].
fn clone_rows(gs: GridState, data_idxs: &[usize]) {
    if data_idxs.is_empty() {
        return;
    }
    let pidx = gs.add_cloned_rows(data_idxs);
    let rs = gs.rs.get_untracked();
    let nrows = rs.row_count();
    let ncols = rs.col_count();
    let disp = nrows + pidx;
    let model = gs.edit_model.get_untracked();
    let first = (0..ncols).find(|&ci| model.editable(ci)).unwrap_or(0);
    floem::action::exec_after(std::time::Duration::ZERO, move |_| {
        // One tick is a smaller window than the find bar's 150 ms, but it is not
        // no window: `scroll_active_into_view` reads `gs.vp`.
        if !gs.alive() {
            return;
        }
        gs.active.set(Some((disp, first)));
        gs.anchor.set(Some((disp, first)));
        scroll_active_into_view(gs, disp, first);
    });
}

/// Append a blank pending new row (the toolbar "+ Row"), then scroll it into view
/// and open its first editable cell for editing. The edit start is deferred one
/// tick so the pane rebuild (its length grew) mounts the new row first.
fn add_pending_row(gs: GridState) {
    let pidx = gs.add_new_row();
    let rs = gs.rs.get_untracked();
    let nrows = rs.row_count();
    let ncols = rs.col_count();
    let disp = nrows + pidx;
    let model = gs.edit_model.get_untracked();
    let first_editable = (0..ncols).find(|&ci| model.editable(ci));
    match first_editable {
        Some(ci) => {
            floem::action::exec_after(std::time::Duration::ZERO, move |_| {
                // `start_edit` reads `gs.rs`.
                if !gs.alive() {
                    return;
                }
                start_edit(gs, disp, ci);
                scroll_active_into_view(gs, disp, ci);
            });
        }
        None => {
            gs.active.set(Some((disp, 0)));
            gs.anchor.set(Some((disp, 0)));
        }
    }
}

// AI seed-data actions (toolbar sparkle menu). Each will drive the one-shot AI
// pipeline — bottom-sample the base table → build a prompt from the DDL + sample →
// `inline_args` one-shot call → parse → stage as green pending edits (never
// auto-committed). Stubbed for now; the toolbar menu + wiring land first.
/// Start the "generating" pulse clock if it isn't already running. A single
/// self-rescheduling tick advances `ai_pulse` while `ai_busy`; the generating
/// cells read the phase to breathe their purple wash. Reused by every AI seed-data
/// action.
fn start_ai_pulse(gs: GridState) {
    if gs.pulse_running.get_untracked() {
        return;
    }
    gs.pulse_running.set(true);
    ai_pulse_tick(gs);
}

/// One pulse step. Stops (and clears `pulse_running`) once nothing is generating,
/// and bails silently if the result-set scope has been disposed — per the
/// perpetual-`exec_after` rule, every read is a `try_*` so a late tick after
/// shutdown can't panic on a freed signal.
fn ai_pulse_tick(gs: GridState) {
    if !matches!(gs.ai_busy.try_get_untracked(), Some(true)) {
        let _ = gs.pulse_running.try_update(|v| *v = false);
        return;
    }
    if gs.ai_pulse.try_update(|p| *p += 0.2).is_none() {
        return; // scope disposed
    }
    floem::action::exec_after(std::time::Duration::from_millis(45), move |_| {
        ai_pulse_tick(gs)
    });
}

/// Stage an AI-filled value into the active cell — a real row (`dirty`) or a
/// pending new row (`new_rows`), matching the double-click edit path.
fn stage_fill(gs: GridState, disp: usize, ci: usize, pending: Option<usize>, val: Option<String>) {
    match pending {
        Some(p) => gs.stage_new(p, ci, val),
        None => {
            let di = gs.order.get_untracked().get(disp).copied().unwrap_or(disp);
            // Force the edit (even if it equals the current value) so an AI fill is
            // always visibly staged — see `stage_set`.
            gs.stage_set(di, ci, val);
        }
    }
}

/// AI-fill the active editable cell: gather the target table/column + the rest of
/// this row (for coherence), hand the request to the app (which samples the base
/// table + runs the one-shot AI call), and stage the parsed result as a normal
/// green edit. Nothing auto-commits. A no-op unless an editable cell is selected.
fn ai_fill_value(gs: GridState) {
    // The prompt carries this row's other values and a sample of the column, so
    // this is a data path — refused here as well as hidden from the menu, for
    // the same reason `attach_to_chat` checks twice.
    if gs.ai_busy.get_untracked() || !ai_data_of(gs).may_attach() {
        return;
    }
    let Some((disp, ci)) = gs.active.get_untracked() else {
        return;
    };
    let model = gs.edit_model.get_untracked();
    if !model.editable(ci) {
        return;
    }
    let Some(ti) = model.table_index(ci) else {
        return;
    };
    let Some(et) = model.table(ti) else {
        return;
    };
    let source = TableSource::new(et.database.clone(), et.schema.clone(), et.table.clone());
    let rs = gs.rs.get_untracked();
    let ncols = rs.col_count();
    let nrows = rs.row_count();
    let pending = (disp >= nrows).then(|| disp - nrows);
    // Real column name (provenance), falling back to the result column name.
    let col_name = |cj: usize| -> String {
        rs.columns
            .get(cj)
            .map(|c| {
                c.origin
                    .as_ref()
                    .map(|o| o.column.clone())
                    .unwrap_or_else(|| c.name.clone())
            })
            .unwrap_or_default()
    };
    let column = col_name(ci);
    // Row context: the current value of every *other* column belonging to the
    // same base table (staged edit wins over the stored cell).
    let order = gs.order.get_untracked();
    let mut row_context: Vec<(String, Option<String>)> = Vec::new();
    for cj in 0..ncols {
        if cj == ci || model.table_index(cj) != Some(ti) {
            continue;
        }
        let val: Option<String> = match pending {
            Some(p) => gs
                .new_rows
                .with_untracked(|rows| rows.get(p).and_then(|r| r.get(&cj).cloned()).flatten()),
            None => {
                let di = order.get(disp).copied().unwrap_or(disp);
                match gs.dirty.with_untracked(|d| d.get(&(di, cj)).cloned()) {
                    Some(v) => v, // staged edit
                    None => rs
                        .cell(di, cj)
                        .and_then(|c| (!c.is_null()).then(|| c.display().to_string())),
                }
            }
        };
        row_context.push((col_name(cj), val));
    }
    let Some(cb) = gs.ai_fill.get_untracked() else {
        return;
    };
    // Mark the target real-row cell "generating" (purple pulse); a pending-row
    // cell is left unmarked for now — Insert Row / Seed Table will mark their rows.
    let gen_cell: Option<(usize, usize)> = pending
        .is_none()
        .then(|| (order.get(disp).copied().unwrap_or(disp), ci));
    if let Some(cell) = gen_cell {
        gs.ai_gen.update(|g| {
            g.insert(cell);
        });
    }
    gs.ai_busy.set(true);
    gs.commit_err.set(None);
    start_ai_pulse(gs);
    let req = crate::AiFillRequest {
        conn_id: gs.conn_id.get_untracked(),
        source,
        column,
        row_context,
    };
    let done: crate::AiFillDoneFn = Rc::new(move |res| {
        // The result-set scope may have been disposed (tab switched / re-run) while
        // the request was in flight — `try_update` no-ops instead of panicking.
        if gs.ai_busy.try_update(|b| *b = false).is_none() {
            return;
        }
        if let Some(cell) = gen_cell {
            gs.ai_gen.try_update(|g| {
                g.remove(&cell);
            });
        }
        match res {
            crate::AiFillResult::Value(v) => stage_fill(gs, disp, ci, pending, Some(v)),
            crate::AiFillResult::Null => stage_fill(gs, disp, ci, pending, None),
            crate::AiFillResult::Failed(e) => gs.commit_err.set(Some(e)),
        }
    });
    (cb)(req, done);
}

fn ai_insert_row(gs: GridState) {
    ai_seed_rows(gs, 1);
}

/// Shared core of Insert Row (count = 1) and Seed Table (count = N): append
/// `count` blank pending rows, mark them generating (pulsing purple), ask the app
/// to AI-generate the rows, then stage each returned row's values into its pending
/// row. On failure the skeleton rows are rolled back and the error surfaced.
/// Nothing auto-commits — the staged rows commit like any manual `+ Row`.
fn ai_seed_rows(gs: GridState, count: usize) {
    // Seeding samples the table to imitate it, so it sends rows like the rest —
    // one gate, checked at the action rather than only at the menu.
    if gs.ai_busy.get_untracked() || count == 0 || !ai_data_of(gs).may_attach() {
        return;
    }
    let model = gs.edit_model.get_untracked();
    let Some(et) = model.insert_target() else {
        return;
    };
    let source = TableSource::new(et.database.clone(), et.schema.clone(), et.table.clone());
    let rs = gs.rs.get_untracked();
    let ncols = rs.col_count();
    // Columns the model should fill (editable, non-auto-increment) + a name→ci map
    // to stage the reply back. Auto-increment/expression columns are left to the
    // server default.
    let mut fill_columns: Vec<String> = Vec::new();
    let mut name_to_ci: HashMap<String, usize> = HashMap::new();
    for cj in 0..ncols {
        if !model.editable(cj) {
            continue;
        }
        let Some(col) = rs.columns.get(cj) else {
            continue;
        };
        let auto = col
            .origin
            .as_ref()
            .map(|o| o.flags.auto_increment)
            .unwrap_or(false);
        if auto {
            continue;
        }
        let name = col
            .origin
            .as_ref()
            .map(|o| o.column.clone())
            .unwrap_or_else(|| col.name.clone());
        name_to_ci.insert(name.clone(), cj);
        fill_columns.push(name);
    }
    let Some(cb) = gs.ai_seed.get_untracked() else {
        return;
    };
    // Append the skeleton rows + mark them generating; bring the first into view.
    let pidxs: Vec<usize> = (0..count).map(|_| gs.add_new_row()).collect();
    gs.ai_gen_rows.update(|s| {
        for &p in &pidxs {
            s.insert(p);
        }
    });
    gs.ai_busy.set(true);
    gs.commit_err.set(None);
    start_ai_pulse(gs);
    if let Some(&p0) = pidxs.first() {
        scroll_active_into_view(gs, rs.row_count() + p0, 0);
    }
    let req = crate::AiSeedRequest {
        conn_id: gs.conn_id.get_untracked(),
        source,
        fill_columns,
        count,
    };
    // The pending rows these indices refer to, as of now. Discard is not blocked
    // during a generation, and `add_new_row` hands out indices from zero again —
    // so without this the reply staged into whatever row now sits at index 0,
    // overwriting what the user had typed into it.
    let rows_gen = gs.new_rows_gen.get_untracked();
    let done: crate::AiSeedDoneFn = Rc::new(move |res| {
        if gs.ai_busy.try_update(|b| *b = false).is_none() {
            return; // scope disposed
        }
        gs.ai_gen_rows.try_update(|s| {
            for &p in &pidxs {
                s.remove(&p);
            }
        });
        if gs.new_rows_gen.try_get_untracked() != Some(rows_gen) {
            return; // the rows this reply was for are gone
        }
        match res {
            crate::AiSeedResult::Rows(rows) => {
                for (i, &pidx) in pidxs.iter().enumerate() {
                    let Some(row) = rows.get(i) else { continue };
                    for (name, val) in row {
                        if let Some(&ci) = name_to_ci.get(name) {
                            gs.stage_new(pidx, ci, val.clone());
                        }
                    }
                }
                // A short reply leaves the extra skeleton rows blank to fill/discard.
            }
            crate::AiSeedResult::Failed(e) => {
                remove_pending_rows(gs, &pidxs);
                gs.commit_err.set(Some(e));
            }
        }
    });
    (cb)(req, done);
}

/// Remove specific pending rows by index — rolls back the skeleton rows when an AI
/// seed request fails. Removes high indices first so the lower ones stay valid, and
/// bounds-checks in case the rows were cleared meanwhile (discard).
fn remove_pending_rows(gs: GridState, pidxs: &[usize]) {
    gs.new_rows.try_update(|rows| {
        let mut idxs: Vec<usize> = pidxs.to_vec();
        idxs.sort_unstable_by(|a, b| b.cmp(a));
        for p in idxs {
            if p < rows.len() {
                rows.remove(p);
            }
        }
    });
}

/// Open the "AI Seed Table" count popover, seeding the input with a default of 10.
fn open_seed_popover(gs: GridState) {
    // Gated like the generation it leads to: a popover that takes a row count
    // and then silently does nothing (because `ai_seed_rows` refuses) is worse
    // than one that never opens. Reachable if the level is tightened while the
    // menu is up.
    if gs.ai_busy.get_untracked() || !ai_data_of(gs).may_attach() {
        return;
    }
    gs.seed_buf.set("10".to_string());
    gs.seed_open.set(true);
}

/// Max rows a single Seed Table request will generate (no point in hundreds here).
const SEED_ROW_CAP: usize = 50;

/// The "AI Seed Table" count popover: a small panel (numeric field, preset chips,
/// Generate button) anchored under the toolbar, over a click-catcher backdrop.
/// Kick a seed with `n` rows (clamped 1..=SEED_ROW_CAP) and close. Mounted last in
/// `grid_view`'s stack so it draws over the grid; hidden unless `seed_open`.
fn seed_popover(gs: GridState) -> impl IntoView {
    dyn_container(
        move || gs.seed_open.get(),
        move |open| {
            if !open {
                return empty().into_any();
            }
            // Parse the field, clamp, close, and generate.
            let go: Rc<dyn Fn()> = Rc::new(move || {
                let n = gs
                    .seed_buf
                    .get_untracked()
                    .trim()
                    .parse::<usize>()
                    .unwrap_or(0)
                    .clamp(1, SEED_ROW_CAP);
                gs.seed_open.set(false);
                ai_seed_rows(gs, n);
            });
            // Preset chips generate immediately; the field is for a custom count.
            let preset = |n: usize, go: Rc<dyn Fn()>, gs: GridState| {
                container(text(format!("{n}")).style(|s| s.font_size(theme::font_label())))
                    .on_click_stop(move |_| {
                        gs.seed_buf.set(n.to_string());
                        (go)();
                    })
                    .style(|s| {
                        s.padding_horiz(theme::scaled(10.0))
                            .padding_vert(theme::scaled(4.0))
                            .border(1.0)
                            .border_color(theme::border())
                            .border_radius(6.0)
                            .cursor(CursorStyle::Default)
                            .hover(|s| s.background(theme::accent().multiply_alpha(0.15)))
                    })
            };
            let field = {
                let go = go.clone();
                let esc = move || gs.seed_open.set(false);
                edit_field(
                    gs.seed_buf,
                    FieldCfg {
                        autofocus: true,
                        height: Some(|| theme::scaled(30.0)),
                        on_submit: Some(go),
                        on_escape: Some(Rc::new(esc)),
                        ..FieldCfg::default()
                    },
                )
                .style(|s| s.width(theme::scaled(70.0)))
            };
            let go_btn = go.clone();
            let panel = v_stack((
                text("Seed rows")
                    .style(|s| s.font_size(theme::font_label()).color(theme::text_muted())),
                h_stack((
                    field,
                    container(text("Generate").style(|s| s.font_size(theme::font_body())))
                        .on_click_stop(move |_| (go_btn)())
                        .style(|s| {
                            s.padding_horiz(theme::scaled(12.0))
                                .padding_vert(theme::scaled(6.0))
                                .border_radius(6.0)
                                .color(floem::peniko::Color::WHITE)
                                .background(theme::seed_button())
                                .cursor(CursorStyle::Default)
                                .hover(|s| s.background(theme::seed_button().multiply_alpha(0.85)))
                        }),
                ))
                .style(|s| s.gap(theme::scaled(6.0)).items_center()),
                h_stack((
                    preset(5, go.clone(), gs),
                    preset(10, go.clone(), gs),
                    preset(25, go.clone(), gs),
                    preset(50, go.clone(), gs),
                ))
                .style(|s| s.gap(theme::scaled(6.0))),
            ))
            .style(|s| {
                crate::widgets::panel_style(s)
                    .absolute()
                    .inset_top(30.0) // just below the 28px toolbar
                    .inset_right(8.0)
                    .background(theme::bg_chrome())
                    .padding(theme::scaled(12.0))
                    .gap(theme::scaled(8.0))
            })
            .on_event_stop(EventListener::PointerDown, |_| {});
            // Backdrop: an outside click closes the popover.
            stack((
                empty()
                    .style(|s| s.absolute().inset(0.0))
                    .on_click_stop(move |_| gs.seed_open.set(false)),
                panel,
            ))
            .style(|s| s.absolute().inset(0.0))
            .into_any()
        },
    )
    // The `dyn_container` must itself fill the grid area when open — its child is
    // absolute (out of flow), so without this it collapses to 0×0 and the panel's
    // insets resolve against nothing. Closed → in-flow + empty, so it takes no space
    // and never intercepts clicks.
    .style(move |s| {
        if gs.seed_open.get() {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}

/// Fixed width of a field row's left-hand column-name label.
fn field_name_w() -> f64 {
    theme::scaled(150.0)
}
/// Fixed height of a scalar field row — so toggling sentinel/`<null>` ↔ input never
/// reflows the rows below.
fn field_row_h() -> f64 {
    theme::scaled(32.0)
}

/// Per-field editing state for the structured row panel: the raw text buffer, a
/// NULL flag, and the field editor's own pre-write flush (see [`flush_fields`]).
/// Created fresh per opened row (so the panel's `dyn_container` disposes them on
/// close / row-step).
#[derive(Clone, Copy)]
struct FieldSig {
    ci: usize,
    buf: RwSignal<String>,
    is_null: RwSignal<bool>,
    /// Set by an editor that keeps a buffer of its own — the JSON tree, whose leaf
    /// input only reaches `buf` on submit/blur. Returns whether the pending edit
    /// committed. `None` when the field's editor writes `buf` directly, and cleared
    /// again whenever such an editor is torn down (its signals go with it).
    flush: RwSignal<Option<Rc<dyn Fn() -> bool>>>,
}

/// One field signal per column, seeded from the row's values (empty buffer + null
/// flag for a SQL NULL).
fn field_sigs(cols: &[ColSpec]) -> Vec<FieldSig> {
    cols.iter()
        .enumerate()
        .map(|(ci, c)| FieldSig {
            ci,
            buf: RwSignal::new(rowjson::field_value_text(&c.value)),
            is_null: RwSignal::new(c.value.is_null()),
            flush: RwSignal::new(None),
        })
        .collect()
}

/// Ask every field to commit its in-progress editor edit into its buffer, before
/// the write is assembled from those buffers.
///
/// Clicking Save does **not** blur the field being typed into — floem moves focus
/// on a pointer-down only for a `keyboard_navigable` view, and the Save button is a
/// plain container — so a JSON leaf edit would otherwise never reach `buf` and Save
/// would write the JSON it replaced (or decide nothing changed at all). This is the
/// row panel's counterpart to `commit_grid`'s "flush any open in-cell edit first".
///
/// Returns false if any field's pending edit *couldn't* commit (unparseable JSON —
/// the editor shows its own inline error): the caller must not go on to write the
/// stale buffer. Every field is asked regardless, so one failure doesn't strand
/// another field's valid edit.
fn flush_fields(sigs: &[FieldSig]) -> bool {
    sigs.iter()
        .fold(true, |ok, f| match f.flush.get_untracked() {
            Some(flush) => (flush)() && ok,
            None => ok,
        })
}

/// Gather each field's current value (`None` = staged NULL) for the write assembly.
fn field_state(sigs: &[FieldSig]) -> Vec<(usize, Option<String>)> {
    sigs.iter()
        .map(|f| {
            let v = (!f.is_null.get_untracked()).then(|| f.buf.get_untracked());
            (f.ci, v)
        })
        .collect()
}

/// The left-hand column label in a field row: name + dim type.
fn field_label(name: String, type_name: String) -> impl IntoView {
    h_stack((
        text(name).style(|s| {
            s.font_size(theme::scaled_font(13.0))
                .color(theme::text())
                .text_ellipsis()
                .min_width(0.0)
                .flex_grow(1.0_f32)
        }),
        text(type_name).style(|s| {
            s.font_size(theme::scaled_font(13.0))
                .color(theme::text_faint())
                .margin_left(theme::scaled(6.0))
                .flex_shrink(0.0_f32)
        }),
    ))
    .style(|s| {
        s.items_center()
            .width(field_name_w())
            .flex_shrink(0.0_f32)
            .padding_right(theme::scaled(10.0))
    })
}

/// A small borderless text button (the per-field Set-NULL / Set-value / Unset
/// affordances): no background, just a text-colour hover.
fn field_mini_btn(label: &'static str, action: impl Fn() + 'static) -> AnyView {
    container(text(label).style(|s| s.font_size(theme::scaled_font(13.0))))
        .on_click_stop(move |_| action())
        .style(|s| {
            s.padding_horiz(theme::scaled(4.0))
                .flex_shrink(0.0_f32)
                .color(theme::text_dim())
                .hover(|s| s.color(theme::text()))
        })
        .into_any()
}

/// The dim `<null>` sentinel shown for a NULL field / value.
fn null_sentinel() -> AnyView {
    text("<null>")
        .style(|s| {
            s.font_size(theme::scaled_font(13.0))
                .color(theme::text_faint())
        })
        .into_any()
}

/// A field's control wrapped in its NULL toggle: the control itself for a NOT
/// NULL column, and for a nullable one either the `<null>` sentinel with a "Set
/// value" affordance or the control with "Set NULL" beside it.
///
/// NULL is an explicit state — clearing the text to empty is the empty string,
/// not NULL. A `<null>` field re-enables on **double-click** (same as its "Set
/// value" button).
///
/// `control` is a builder rather than a view because the `dyn_container` rebuilds
/// it every time the null flag flips, and a control holding signals of its own
/// (the calendar's open month) must be rebuilt with them, not moved.
fn nullable_field(nullable: bool, f: FieldSig, control: Rc<dyn Fn() -> AnyView>) -> AnyView {
    if !nullable {
        return container((control)()).style(|s| s.width_full()).into_any();
    }
    dyn_container(
        move || f.is_null.get(),
        move |is_null| {
            if is_null {
                h_stack((
                    null_sentinel(),
                    empty().style(|s| s.flex_grow(1.0_f32)),
                    field_mini_btn("Set value", move || f.is_null.set(false)),
                ))
                .style(|s| s.items_center().width_full().gap(theme::scaled(8.0)))
                .on_double_click_stop(move |_| f.is_null.set(false))
                .into_any()
            } else {
                h_stack((
                    container((control)()).style(|s| s.flex_grow(1.0_f32).min_width(0.0)),
                    field_mini_btn("Set NULL", move || f.is_null.set(true)),
                ))
                .style(|s| s.items_center().width_full().gap(theme::scaled(6.0)))
                .into_any()
            }
        },
    )
    .style(|s| s.width_full())
    .into_any()
}

/// The editable value cell for a scalar field: a text input in its NULL toggle.
fn scalar_editor(gs: GridState, nullable: bool, autofocus: bool, f: FieldSig) -> AnyView {
    let make_field: Rc<dyn Fn() -> AnyView> = Rc::new(move || {
        edit_field(
            f.buf,
            FieldCfg {
                background: theme::bg_editor,
                font_size: theme::font_body,
                autofocus,
                height: Some(field_input_h),
                // Escape closes the panel even while a field is focused.
                on_escape: Some(Rc::new(move || gs.edit_row_open.set(false))),
                ..Default::default()
            },
        )
        .style(|s| s.width_full())
        .into_any()
    });
    nullable_field(nullable, f, make_field)
}

/// The control this field may actually use: its column's, unless the value in
/// hand can't be represented by it — in which case the plain text field, which
/// can represent anything.
///
/// An empty buffer fits everything ([`celledit::fits`]), which is what hands a
/// NULL field its dropdown the moment "Set value" turns it into one.
fn fitting_editor(editor: CellEditor, f: &FieldSig) -> CellEditor {
    if f.buf.with_untracked(|b| celledit::fits(&editor, b)) {
        editor
    } else {
        CellEditor::Text
    }
}

/// What an **open** cell is: a text field, a picker in place of one, or a text
/// field with a calendar standing over the grid beside it.
///
/// Three shapes rather than "a control or not", because the two controls a cell
/// can hold want opposite things from the cell around them — see [`cell_fills`].
#[derive(Clone, Debug, PartialEq, Eq)]
enum CellShape {
    /// A plain text field, filling the cell on its own inset.
    Text,
    /// The in-cell picker ([`cell_pick_editor`]) — a boolean, an enum, a `SET`:
    /// the value and a chevron, with the list drawn over the grid.
    Pick(CellEditor),
    /// A text field *and* the calendar ([`cell_calendar_editor`]): a date is
    /// often faster typed, and a `DATETIME`'s time of day has no calendar to come
    /// from, so the field stays and the panel drops from the cell.
    Calendar(CellEditor),
}

/// The shape this cell's **open editor** takes: its column's control, unless the
/// value in hand can't be represented by it ([`fitting_editor`]'s rule, asked of
/// the buffer `start_edit` seeded).
///
/// Asked twice per open editor: once by the cell's content, to build it, and once
/// by the cell's own style, to drop the padding a text field wants and a picker
/// must not have. One function, because a cell padded like a text field around a
/// control that fills it is a gap down one side and a clipped chevron on the
/// other — and the two answers must agree keystroke for keystroke.
fn open_cell_shape(gs: GridState, ci: usize) -> CellShape {
    let e = cell_editor(gs, ci);
    gs.edit_buf.with_untracked(|b| cell_shape(e, b))
}

/// [`open_cell_shape`] without the signals: which shape `editor` takes over a
/// cell already holding `buf`.
fn cell_shape(editor: CellEditor, buf: &str) -> CellShape {
    if !celledit::fits(&editor, buf) {
        return CellShape::Text;
    }
    match editor {
        CellEditor::Bool(_) | CellEditor::Enum(_) | CellEditor::Set(_) => CellShape::Pick(editor),
        CellEditor::Date | CellEditor::DateTime(_) => CellShape::Calendar(editor),
        CellEditor::Text => CellShape::Text,
    }
}

/// How a cell's text is weighted — which is the only thing that distinguishes
/// **the values it is about to write**.
///
/// A cell paints one of five ways, and four of them were an `if` chain inside the
/// style closure. The fifth is the reason this is a function: a staged SQL NULL
/// and a staged four-character string reading `NULL` went through the *same* arm,
/// so `middle_name = NULL` and `middle_name = 'NULL'` were pixel-identical before
/// the Commit that writes one of them. Copy a NULL cell and paste it (the
/// clipboard carries the display text, uninterpreted — deliberately) and the
/// grid had nothing to audit: `WHERE middle_name IS NULL` stops matching a row
/// that looks exactly like the ones it does match.
///
/// The italic is not a new vocabulary: it is the treatment a NULL *original* has
/// always had, which is the point — "there is no value here" reads the same
/// whether the emptiness is stored or staged, and a value that merely spells
/// `NULL` reads as the text it is.
///
/// A decision, so it is testable: `staged` is `dirty`'s entry for the cell
/// (`None` = nothing staged, `Some(None)` = staged SQL NULL, `Some(Some(t))` =
/// staged text), and the last three are the painter's own flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CellInk {
    /// A staged edit — white on the green fill.
    Staged,
    /// A staged **SQL NULL** — white, and italic like every other absence.
    StagedNull,
    /// No value: a NULL original, or a pending row's `<auto>`/`<required>`/
    /// `<null>`/`<default>` placeholder.
    Absent,
    /// A foreign key, underlined as a followable relation.
    Fk,
    Plain,
}

fn cell_ink(
    staged: Option<&Option<String>>,
    is_null: bool,
    placeholder: bool,
    is_fk: bool,
) -> CellInk {
    match staged {
        Some(None) => CellInk::StagedNull,
        Some(Some(_)) => CellInk::Staged,
        // The order is the painter's: a NULL original outranks the FK underline,
        // because there is no key in the cell to follow.
        None if is_null || placeholder => CellInk::Absent,
        None if is_fk => CellInk::Fk,
        None => CellInk::Plain,
    }
}

/// Does the control take the **whole cell**, padding and all?
///
/// Only a picker does: it carries its own surface and puts the text back on the
/// display's inset itself, so a padded cell around it leaves a strip of the row
/// showing down each side. A calendar's cell holds an ordinary text field, which
/// wants the ordinary padding — the panel is not in the cell at all.
fn cell_fills(shape: &CellShape) -> bool {
    matches!(shape, CellShape::Pick(_))
}

/// The editable value cell for a field whose column has a **type-aware** control
/// ([`schemaic_core::celledit`]): the control, in the same NULL toggle a text
/// field gets.
///
/// The caller has already established that the field's value
/// [`celledit::fits`] the control — a value that doesn't keeps
/// [`scalar_editor`], which is what stops a toggle from rewriting a `tinyint(1)`
/// holding `7`.
fn typed_editor(
    gs: GridState,
    editor: CellEditor,
    nullable: bool,
    autofocus: bool,
    f: FieldSig,
) -> AnyView {
    // Escape closes the panel even while a control has the keyboard — the same
    // contract `scalar_editor` gives, and **every** control here owes it, not
    // just the ones with a text field in them. (A control with its own popup up
    // takes the first Escape: that one is on the shared popup registry, which the
    // window root peels off before anything else — see
    // `widgets::dismiss_open_popup`. The root only gets the key when the popup
    // took the keyboard, which a *menu* does and the calendar does not; the date
    // control peels its own, in `cell_editors::peeling_escape`.)
    let open = gs.edit_row_open;
    let close_panel = move || -> Option<Rc<dyn Fn()>> { Some(Rc::new(move || open.set(false))) };
    let control: Rc<dyn Fn() -> AnyView> = match editor {
        // A boolean is a two-row picker, the same control an enum gets: its
        // values are as listed as an enum's, and one control for both is one
        // thing to learn (and none to invent).
        editor @ (CellEditor::Bool(_) | CellEditor::Enum(_)) => {
            let ch = cell_editors::PopupChannel {
                menus: gs.menus,
                anchor: gs.popup_anchor,
                width: gs.popup_width,
            };
            // `autofocus` and the Escape contract reach a picker too. Dropping
            // them here — as this match did for all three of these arms — is a
            // column that cannot be set without a mouse: nothing takes the
            // keyboard when the panel opens on it, and Tab walks straight past.
            Rc::new(move || {
                cell_editors::pick_field(f.buf, editor.clone(), ch, autofocus, close_panel())
            })
        }
        editor @ CellEditor::Set(_) => Rc::new(move || {
            cell_editors::set_control(f.buf, editor.clone(), autofocus, close_panel())
        }),
        editor @ (CellEditor::Date | CellEditor::DateTime(_)) => Rc::new(move || {
            cell_editors::date_control(f.buf, editor.clone(), autofocus, close_panel(), gs.menus)
        }),
        CellEditor::Text => return scalar_editor(gs, nullable, autofocus, f),
    };
    nullable_field(nullable, f, control)
}

/// True for a JSON/JSONB column type (MySQL `json`, Postgres `json`/`jsonb`).
fn is_json_type(type_name: &str) -> bool {
    let t = type_name.trim();
    t.eq_ignore_ascii_case("json") || t.eq_ignore_ascii_case("jsonb")
}

/// Left indent (px) per JSON tree depth level.
fn json_indent() -> f64 {
    theme::scaled(15.0)
}

/// Is `path` hidden because one of its ancestor container paths is collapsed?
///
/// The range starts at 0 because the **root** container's path is the empty
/// vector — skipping `n = 0` meant collapsing the root flipped its chevron and
/// hid nothing, which is the one value collapsing exists for. The row's own path
/// stays out of the range, so a collapsed container still renders itself.
fn json_path_hidden(path: &[PathSeg], collapsed: &HashSet<Vec<PathSeg>>) -> bool {
    (0..path.len()).any(|n| collapsed.contains(&path[..n].to_vec()))
}

/// One rendered row of the JSON tree: indent, disclosure (for containers), key /
/// index label, and value — a `{n}` / `[n]` summary for a container, a click-to-edit
/// scalar for a leaf (the row being edited shows an inline `edit_field`).
#[allow(clippy::too_many_arguments)]
fn json_row_view(
    r: &TreeRow,
    is_editing: bool,
    collapsed: RwSignal<HashSet<Vec<PathSeg>>>,
    editing: RwSignal<Option<Vec<PathSeg>>>,
    edit_buf: RwSignal<String>,
    err: RwSignal<Option<String>>,
    start_edit: Rc<dyn Fn(Vec<PathSeg>, String)>,
    commit_current: Rc<dyn Fn()>,
) -> AnyView {
    let indent = r.depth as f64 * json_indent();
    let path = r.path.clone();

    let disclosure: AnyView = if matches!(r.kind, RowKind::Scalar) {
        empty()
            .style(|s| s.width(theme::scaled(15.0)).flex_shrink(0.0_f32))
            .into_any()
    } else {
        let is_collapsed = collapsed.get_untracked().contains(&path);
        let p = path.clone();
        container(icons::icon(
            if is_collapsed {
                icons::CHEVRON_RIGHT
            } else {
                icons::CHEVRON_DOWN
            },
            12.0,
        ))
        .on_click_stop(move |_| {
            collapsed.update(|c| {
                if !c.remove(&p) {
                    c.insert(p.clone());
                }
            });
        })
        .style(|s| {
            s.width(theme::scaled(15.0))
                .flex_shrink(0.0_f32)
                .items_center()
                .color(theme::text_dim())
                .hover(|s| s.color(theme::text()))
        })
        .into_any()
    };

    // Label: `key:` for an object member, `[i]` for an array element, none at root.
    let label: AnyView = match (&r.label, r.path.last()) {
        (Some(k), _) => h_stack((
            text(k.clone()).style(|s| {
                s.font_size(theme::scaled_font(13.0))
                    .color(theme::key_index())
            }),
            text(":").style(|s| {
                s.font_size(theme::scaled_font(13.0))
                    .color(theme::text_faint())
                    .margin_right(theme::scaled(6.0))
            }),
        ))
        .style(|s| s.items_center().flex_shrink(0.0_f32))
        .into_any(),
        (None, Some(PathSeg::Index(i))) => text(format!("[{i}]"))
            .style(|s| {
                s.font_size(theme::scaled_font(13.0))
                    .color(theme::text_faint())
                    .margin_right(theme::scaled(6.0))
                    .flex_shrink(0.0_f32)
            })
            .into_any(),
        _ => empty().into_any(),
    };

    let value: AnyView = match &r.kind {
        RowKind::Object(n) => text(format!("{{{n}}}"))
            .style(|s| {
                s.font_size(theme::scaled_font(13.0))
                    .color(theme::text_faint())
            })
            .into_any(),
        RowKind::Array(n) => text(format!("[{n}]"))
            .style(|s| {
                s.font_size(theme::scaled_font(13.0))
                    .color(theme::text_faint())
            })
            .into_any(),
        RowKind::Scalar => {
            if is_editing {
                edit_field(
                    edit_buf,
                    FieldCfg {
                        background: theme::bg_deepest,
                        font_size: theme::font_body,
                        autofocus: true,
                        height: Some(field_input_h),
                        on_submit: Some(commit_current.clone()),
                        on_blur: Some(commit_current.clone()),
                        on_escape: Some(Rc::new(move || {
                            editing.set(None);
                            err.set(None);
                        })),
                        ..Default::default()
                    },
                )
                .style(|s| s.flex_grow(1.0_f32).min_width(0.0))
                .into_any()
            } else {
                let vj = r.value_json.clone().unwrap_or_default();
                let vj2 = vj.clone();
                let p = path.clone();
                container(
                    text(vj).style(|s| s.font_size(theme::scaled_font(13.0)).color(theme::text())),
                )
                .on_click_stop(move |_| (start_edit)(p.clone(), vj2.clone()))
                .style(|s| {
                    s.padding_horiz(theme::scaled(4.0))
                        .padding_vert(theme::scaled(1.0))
                        .border_radius(3.0)
                        .hover(|s| s.background(theme::bg_deepest()))
                })
                .into_any()
            }
        }
    };

    h_stack((
        empty().style(move |s| s.width(indent).flex_shrink(0.0_f32)),
        disclosure,
        label,
        value,
    ))
    .style(|s| s.items_center().width_full().min_height(field_row_h()))
    .into_any()
}

/// The interactive JSON tree editor for a JSON-typed column value. Parses the field
/// buffer into a tree; each leaf is click-to-edit as a JSON scalar (`"str"`, a
/// number, `true`, `null`, or even a nested object), containers collapse/expand.
/// Every committed edit re-serialises the tree back into the field buffer, so Save
/// writes the updated JSON. Falls back to a raw-text field if the value isn't valid
/// JSON.
fn json_editor(f: FieldSig, sink: RwSignal<Option<String>>) -> AnyView {
    let Ok(root) = JsonNode::parse(&f.buf.get_untracked()) else {
        // The raw fallback is bound straight to `f.buf`, so there is nothing to
        // flush — and leaving a previous editor's flush installed would call into
        // signals this rebuild just disposed.
        f.flush.set(None);
        return edit_field(
            f.buf,
            FieldCfg {
                background: theme::bg_editor,
                font_size: theme::font_body,
                height: Some(field_input_h),
                ..Default::default()
            },
        )
        .style(|s| s.width_full())
        .into_any();
    };
    let tree = RwSignal::new(root);
    let collapsed: RwSignal<HashSet<Vec<PathSeg>>> = RwSignal::new(HashSet::new());
    let editing: RwSignal<Option<Vec<PathSeg>>> = RwSignal::new(None);
    let edit_buf = RwSignal::new(String::new());
    let err: RwSignal<Option<String>> = RwSignal::new(None);

    // The message goes to the panel-level bar (`sink`), which is pinned to the
    // bottom of the panel and can't be scrolled away from — this editor used to
    // render it inline, which on a nested value put it several scrolls down from
    // the Save that failed. The signal stays local because the *box* keeps a red
    // outline: the bar says what went wrong, the outline says which field.
    //
    // `pushed` is what was last handed over, so clearing takes back only our own
    // message — by then the bar may be showing a failed write instead.
    let pushed: RwSignal<Option<String>> = RwSignal::new(None);
    create_effect(move |_| {
        let now = err.get();
        match &now {
            Some(msg) => sink.set(Some(msg.clone())),
            None => {
                if pushed.get_untracked().is_some()
                    && sink.get_untracked() == pushed.get_untracked()
                {
                    sink.set(None);
                }
            }
        }
        pushed.set(now);
    });

    // Commit the currently-edited leaf: parse the JSON scalar, write it into the
    // tree, re-serialise into the field buffer. Invalid JSON shows an inline error
    // and keeps the leaf in edit mode.
    //
    // A leaf that came back **unchanged** touches nothing. Opening a leaf and
    // moving to another one commits the first, so a re-serialise here would rewrite
    // the field buffer — and `update_changes` compares text, so merely *browsing* a
    // JSON value by clicking two leaves would have put the column in the `UPDATE`.
    let commit_current: Rc<dyn Fn()> = Rc::new(move || {
        // `try_get_untracked`, not `get_untracked`: this is wired as an
        // `on_blur`, which `edit_field` arms as a `Duration::ZERO` timer that
        // nothing cancels, and `get_untracked` is `try_get_untracked().unwrap()`.
        // Reading through the fallible form means a callback that fires after
        // the panel's scope is disposed is a no-op instead of a panic.
        // Hardening, not a repro: floem runs `handle_timer` at the top of every
        // event-loop callback, so the timer lands before the click that would
        // dispose anything.
        let Some(Some(path)) = editing.try_get_untracked() else {
            return;
        };
        let Some(buf) = edit_buf.try_get_untracked() else {
            return;
        };
        match JsonNode::parse(&buf) {
            Ok(node) => {
                if tree.with_untracked(|t| t.get(&path) != Some(&node)) {
                    tree.update(|t| {
                        t.set(&path, node);
                    });
                    f.buf.set(tree.get_untracked().to_compact());
                    f.is_null.set(false);
                }
                editing.set(None);
                err.set(None);
            }
            Err(e) => err.set(Some(format!("Invalid JSON: {e}"))),
        }
    });
    // Start editing a leaf, committing any in-progress edit first (so switching
    // leaves never loses or misattributes the previous one). If the in-progress
    // edit was invalid, stay on it (don't switch).
    let start_edit: Rc<dyn Fn(Vec<PathSeg>, String)> = {
        let commit_current = commit_current.clone();
        Rc::new(move |path, value| {
            (commit_current)();
            if editing.get_untracked().is_none() {
                edit_buf.set(value);
                editing.set(Some(path));
                err.set(None);
            }
        })
    };
    // Save's pre-write flush: same commit, reporting whether the leaf actually
    // closed (an unparseable one stays open with its error showing).
    f.flush.set(Some({
        let commit_current = commit_current.clone();
        Rc::new(move || {
            (commit_current)();
            editing.get_untracked().is_none()
        })
    }));

    let rows_view = dyn_container(
        move || (tree.get(), collapsed.get(), editing.get()),
        move |(root, collapsed_set, editing_path)| {
            let mut views: Vec<AnyView> = Vec::new();
            for r in root.rows() {
                if json_path_hidden(&r.path, &collapsed_set) {
                    continue;
                }
                let is_editing = editing_path.as_ref() == Some(&r.path);
                views.push(json_row_view(
                    &r,
                    is_editing,
                    collapsed,
                    editing,
                    edit_buf,
                    err,
                    start_edit.clone(),
                    commit_current.clone(),
                ));
            }
            v_stack_from_iter(views)
                .style(|s| s.width_full().flex_col())
                .into_any()
        },
    )
    .style(|s| s.width_full());

    container(rows_view)
        .style(move |s| {
            // Red outline while this value's pending edit won't parse — the bar
            // carries the words, this says where they came from. It outlives a
            // dismissed bar on purpose: the field is still invalid.
            let border = if err.get().is_some() {
                theme::error()
            } else {
                theme::border()
            };
            s.width_full()
                .flex_col()
                .padding(theme::scaled(6.0))
                .border(1.0)
                .border_color(border)
                .border_radius(6.0)
                .background(theme::bg_editor())
        })
        .into_any()
}

/// A JSON-typed editable field: the tree editor, wrapped (for a nullable column) in
/// the same NULL toggle as scalar fields. Activating a NULL field seeds an empty
/// object so the tree has something to edit.
fn json_field(nullable: bool, f: FieldSig, sink: RwSignal<Option<String>>) -> AnyView {
    if !nullable {
        return json_editor(f, sink);
    }
    dyn_container(
        move || f.is_null.get(),
        move |is_null| {
            if is_null {
                // The tree editor (and its flush) went with this rebuild.
                f.flush.set(None);
                // Enabling a NULL JSON field seeds an empty object so the tree has
                // something to edit; double-click does the same as "Set value".
                let enable = move || {
                    if JsonNode::parse(&f.buf.get_untracked()).is_err() {
                        f.buf.set("{}".to_string());
                    }
                    f.is_null.set(false);
                };
                h_stack((
                    null_sentinel(),
                    empty().style(|s| s.flex_grow(1.0_f32)),
                    field_mini_btn("Set value", enable),
                ))
                .style(|s| s.items_center().width_full().gap(theme::scaled(8.0)))
                .on_double_click_stop(move |_| enable())
                .into_any()
            } else {
                json_editor(f, sink)
            }
        },
    )
    .style(|s| s.width_full())
    .into_any()
}

/// The read-only value cell: dim text (NULL → `<null>`), shown for context, no caret.
fn readonly_value(f: FieldSig) -> AnyView {
    if f.is_null.get_untracked() {
        return null_sentinel();
    }
    text(f.buf.get_untracked())
        .style(|s| {
            s.font_size(theme::scaled_font(13.0))
                .color(theme::text_dim())
                .text_ellipsis()
                .min_width(0.0)
                .flex_grow(1.0_f32)
        })
        .into_any()
}

/// One field row: the column label + its value editor (editable) or read-only cell.
///
/// `typed` is the column's type-aware control, already narrowed to what this
/// field's **own value** fits (see [`typed_editor`]); [`CellEditor::Text`] is the
/// plain input, and is what a value no control can represent comes back as.
#[allow(clippy::too_many_arguments)] // a UI builder; grouping into a struct adds no clarity
fn field_row(
    gs: GridState,
    name: String,
    type_name: String,
    typed: CellEditor,
    editable: bool,
    nullable: bool,
    autofocus: bool,
    f: FieldSig,
) -> AnyView {
    let is_json = is_json_type(&type_name);
    // The one control that can outgrow a line: a `SET`'s chips wrap. (A date's
    // calendar is an overlay, so its row stays a row.)
    let grows = is_json || matches!(typed, CellEditor::Set(_));
    let editor = if !editable {
        readonly_value(f)
    } else if is_json {
        json_field(nullable, f, gs.commit_err)
    } else {
        typed_editor(gs, typed, nullable, autofocus, f)
    };
    // A top-aligned row leaves the label's own box the height of its text, which
    // is 13px against a 26px control — so in that case the label is given the
    // control's height and centres its text inside it. Without this the name sits
    // a few pixels above the value it names, on those rows only.
    let label = field_label(name, type_name);
    let label = if grows && !is_json {
        label.style(|s| s.height(field_input_h())).into_any()
    } else {
        label.into_any()
    };
    h_stack((
        label,
        container(editor).style(|s| s.flex_grow(1.0_f32).min_width(0.0)),
    ))
    .style(move |s| {
        let s = s
            .width_full()
            .gap(theme::scaled(8.0))
            .padding_vert(theme::scaled(3.0));
        // A JSON tree grows tall — top-align the label and let the row grow. A scalar
        // row keeps a *fixed* height so toggling `<null>` ↔ input (which are different
        // natural heights) never reflows the rows below.
        if is_json {
            s.items_start().min_height(field_row_h())
        } else if grows {
            // The same floor, and the same height while closed — but the row may
            // exceed it once the chips wrap or the calendar unfolds, and the label
            // stays on the control's first line instead of centring against it.
            s.items_start().min_height(field_row_h())
        } else {
            s.items_center().height(field_row_h())
        }
    })
    .into_any()
}

/// The "Edit Row" panel — an in-flow strip at the bottom of the results area (like
/// the "View" value viewer), integrated with the grid rather than a floating popup.
/// Renders the row as a **structured, per-field editor** (one row per column):
/// read-only columns are shown for context (no caret), editable ones get a text
/// input with an explicit NULL toggle. Save commits immediately (`commit_row_update`);
/// the field list scrolls past `max_rows` and the grid above shrinks to fit.
fn edit_row_panel(gs: GridState, max_rows: RwSignal<usize>) -> impl IntoView {
    dyn_container(
        // Keyed on (open, current row) so walking to another row (prev/next) rebuilds
        // the panel — header number, chevron state, and the per-field editors refresh
        // together (and the old row's field signals dispose).
        //
        // **And on the editors' identity**, because each field's control is
        // resolved once here at build (`fitting_editor` below): a schema landing
        // after the panel was opened would otherwise leave every column on the
        // plain text editor until the user stepped away and back. The `Rc`'s
        // address is the identity, and the effect that fills it only replaces the
        // value when the resolution really moved — so this rebuilds when a column
        // gains a control, and not on every schema refresh.
        move || {
            (
                gs.edit_row_open.get(),
                gs.edit_row_di.get(),
                Rc::as_ptr(&gs.editors.get()) as usize,
            )
        },
        move |(open, di_opt, _)| {
            let (true, Some(di)) = (open, di_opt) else {
                return empty().into_any();
            };
            let close: Rc<dyn Fn()> = Rc::new(move || gs.edit_row_open.set(false));

            // Build the per-field editors from the row.
            let cols = row_colspecs(gs, di);
            let sigs = field_sigs(&cols);
            let rs = gs.rs.get_untracked();
            let first_editable = cols.iter().position(|c| c.editable);
            let any_editable = first_editable.is_some();
            let mut rows: Vec<AnyView> = Vec::with_capacity(cols.len());
            for (ci, c) in cols.iter().enumerate() {
                let type_name = rs
                    .columns
                    .get(ci)
                    .map(|col| col.type_name.clone())
                    .unwrap_or_default();
                let autofocus = first_editable == Some(ci);
                // The column's control, narrowed to what *this row's* value fits:
                // a value no control can represent (a `tinyint(1)` holding 7, an
                // ENUM holding something MySQL rejected into it) keeps the text
                // field, so opening the row can never rewrite it.
                let typed = fitting_editor(cell_editor(gs, ci), &sigs[ci]);
                rows.push(field_row(
                    gs,
                    c.name.clone(),
                    type_name,
                    typed,
                    c.editable,
                    c.nullable,
                    autofocus,
                    sigs[ci],
                ));
            }

            // Save gathers the per-field state and commits an UPDATE.
            let cols_rc = Rc::new(cols);
            let save: Rc<dyn Fn()> = {
                let cols_rc = cols_rc.clone();
                let sigs = sigs.clone();
                Rc::new(move || {
                    // Clicking Save doesn't blur the field being typed into, so ask
                    // each editor to commit into its buffer first. A field that
                    // can't (invalid JSON) shows its own error and stops the write.
                    if !flush_fields(&sigs) {
                        return;
                    }
                    commit_row_update(gs, di, &cols_rc, field_state(&sigs))
                })
            };

            let row_no = edit_row_disp(gs, di) + 1;
            let title = match gs.source.get_untracked() {
                // Qualified outside `public`, so two same-named tables read apart.
                Some(src) if !src.table.is_empty() => {
                    format!("Row {row_no}  ·  {}", src.display())
                }
                _ => format!("Row {row_no}"),
            };

            // Header icons — Save (✓, when editable) then Close (✕). Save commits
            // immediately; Close cancels.
            let close_x = close.clone();
            let close_btn = container(icons::icon(icons::X, 14.0))
                .on_click_stop(move |_| (close_x)())
                .style(|s| {
                    s.padding(theme::scaled(4.0))
                        .color(theme::text_dim())
                        .hover(|s| s.color(theme::text()))
                });
            let trailing = if any_editable {
                let save_btn = container(icons::icon(icons::CHECK, 14.0))
                    .on_click_stop(move |_| (save)())
                    .style(|s| {
                        s.padding(theme::scaled(4.0))
                            .color(theme::text_dim())
                            .hover(|s| s.color(theme::text()))
                    });
                h_stack((save_btn, close_btn))
                    .style(|s| s.flex_row().items_center().gap(theme::scaled(4.0)))
                    .into_any()
            } else {
                close_btn.into_any()
            };
            // Prev/next-row chevrons, just after the title. Stepping discards unsaved
            // edits (the panel rebuilds).
            let disp = edit_row_disp(gs, di);
            let nrows = rs.row_count();
            let can_prev = disp > 0;
            let can_next = disp + 1 < nrows;
            let nav_chevron = |icon: &'static str, enabled: bool, forward: bool| {
                let btn = container(icons::icon(icon, 14.0));
                if enabled {
                    btn.on_click_stop(move |_| edit_row_step(gs, forward))
                        .style(|s| {
                            s.padding(theme::scaled(4.0))
                                .color(theme::text_dim())
                                .hover(|s| s.color(theme::text()))
                        })
                        .into_any()
                } else {
                    btn.style(|s| s.padding(theme::scaled(4.0)).color(theme::text_faint()))
                        .into_any()
                }
            };
            let nav = h_stack((
                nav_chevron(icons::CHEVRON_UP, can_prev, false),
                nav_chevron(icons::CHEVRON_DOWN, can_next, true),
            ))
            .style(|s| {
                s.flex_row()
                    .items_center()
                    .gap(theme::scaled(2.0))
                    .margin_left(theme::scaled(8.0))
            });

            let head = h_stack((
                text(title).style(|s| s.font_size(theme::font_label()).color(theme::text_dim())),
                nav,
                empty().style(|s| s.flex_grow(1.0_f32)),
                trailing,
            ))
            .style(|s| {
                s.width_full()
                    .items_center()
                    .gap(theme::scaled(4.0))
                    .height(theme::scaled(24.0))
                    .flex_shrink(0.0_f32)
                    .padding_horiz(theme::scaled(10.0))
            });

            // The field list scrolls (app-standard auto-hiding bars) once the panel
            // hits its cap. The scroll spans the full panel width so its bar sits at
            // the standard edge inset; the row content keeps its 10px horizontal
            // padding *inside* the scroll.
            //
            // `min_height(0)` is what makes the cap work: a flex child defaults to a
            // content-sized minimum, so a long field list refuses to shrink and the
            // panel grows past its own max — which is exactly the clipping this is
            // meant to prevent. With it, the list yields and scrolls instead.
            let fields =
                autohide(scroll(v_stack_from_iter(rows).style(|s| {
                    s.width_full().flex_col().padding_horiz(theme::scaled(10.0))
                })))
                .style(|s| s.width_full().min_height(0.0));

            // Status line: that a save is in flight. Save commits over the network
            // and can queue behind another session's lock, so without this the ✓
            // looks dead. (What the wait is, once it's long enough to have a cause
            // worth naming, is the panel-level bar's job — see `arm_wait_note`.)
            //
            // Errors are NOT here. A failure used to render inline, underneath the
            // field it came from — which on a JSON column meant inside a nested
            // container, several scrolls down, so a save that didn't happen looked
            // like a save that did nothing. They go to `commit_err` instead: the
            // same red bar, pinned to the bottom of the panel, that a grid commit
            // failure uses. It is the same class of message — this write didn't
            // happen, here's why — and it can't be scrolled away from.
            let err_line = dyn_container(
                move || gs.edit_row_saving.get(),
                move |saving| {
                    if saving {
                        loading_dots("Saving", theme::text_dim, theme::font_label).into_any()
                    } else {
                        empty().into_any()
                    }
                },
            )
            .style(|s| s.width_full().padding_horiz(theme::scaled(10.0)));

            // In-flow strip attached to the grid's bottom edge (border_top + panel
            // background), not a floating overlay. Horizontal padding lives on the
            // children (not here) so the field scroll can span full-width and pin its
            // bar to the edge. Escape closes it (handled at the results-area level).
            v_stack((head, fields, err_line))
                // A click anywhere in the panel dismisses the error bar, the same
                // way a click on the grid does (`dismiss_overlays` is on the cell /
                // gutter / header surfaces, which the panel isn't one of). Passive:
                // fields, chevrons and Save all still get the event.
                .on_event_cont(EventListener::PointerDown, move |_| gs.dismiss_overlays())
                // The cap sits here rather than on the wrapper below, because this
                // is the element whose children have to give way: over the cap, the
                // column shrinks its shrinkable child — the field list, which has
                // `min_height(0)` and scrolls — while the fixed header keeps its
                // size. Capping the wrapper instead would leave this stack at its
                // content height and clip it.
                .style(move |s| {
                    s.width_full()
                        .flex_col()
                        .min_height(0.0)
                        .max_height(max_rows.get() as f64)
                        .gap(theme::scaled(8.0))
                        .padding_vert(theme::scaled(8.0))
                        .border_top(1.0)
                        .border_color(theme::border())
                        .background(theme::bg_panel())
                })
                .into_any()
        },
    )
    .style(|s| s.width_full().flex_shrink(0.0_f32).min_height(0.0))
}

/// Discard all staged changes — cell edits, pending new rows, and pending row
/// deletions (the toolbar ✗) — closing any open in-cell editor.
fn discard_edits(gs: GridState) {
    // Every one of these is guarded, because a discard mostly throws away *one*
    // kind of staged change and announcing the other two anyway is what rebuilt
    // the grid body under the keyboard — see [`clear_if_any`].
    clear_if_any(gs.edit_cell);
    clear_if_any(gs.dirty);
    if gs.new_rows.with_untracked(|r| !r.is_empty()) {
        clear_if_any(gs.new_rows);
        // The pending-row indices are about to be handed out again from zero, so an
        // in-flight AI seed must be told the ones it captured no longer mean what
        // they meant. Only when rows were actually thrown away: with none staged
        // there is nothing whose indices could have moved.
        gs.new_rows_gen.update(|g| *g = g.wrapping_add(1));
    }
    clear_if_any(gs.del_rows);
    // **The whole bar, not just its error surface.** Discard's meaning is "none of
    // that is true any more", and the bar's *note* surface is the one that can hold
    // a sentence about the edits it just threw away — `Pasted 5 cells, skipping 1
    // in read-only columns.` describing five cells that no longer exist, standing
    // until some other path happens to clear it. `clear_bar` is the one spelling;
    // this site read `clear_if_any(gs.commit_err)` rather than
    // `commit_err.get_untracked().is_some()`, which is why `7a5e458`'s sweep for
    // the old shape found the other two copies and not this one.
    gs.clear_bar();
}

/// Move the grid selection to the next (`forward`) / previous cell whose
/// displayed value contains the find query (ASCII-case-insensitive), scanning in
/// row-major display order and wrapping. `from_current` includes the active cell
/// (incremental "find as you type"); next/prev step off it. Movement only — the
/// `pos/total` count is maintained separately (`grid_find_hits`).
fn grid_find(gs: GridState, forward: bool, from_current: bool) {
    let q = gs.find_query.get_untracked();
    if q.is_empty() {
        return;
    }
    let rs = gs.rs.get_untracked();
    let order = gs.order.get_untracked();
    let formats = gs.formats.get_untracked();
    let nrows = order.len();
    let ncols = rs.col_count();
    if nrows == 0 || ncols == 0 {
        return;
    }
    let total = nrows * ncols;
    let (cr, cc) = gs.active.get_untracked().unwrap_or((0, 0));
    let start = cr * ncols + cc;
    for off in 0..total {
        let lin = if forward {
            (start + if from_current { off } else { off + 1 }) % total
        } else {
            (start + total * 2 - off - 1) % total
        };
        let (dr, ci) = (lin / ncols, lin % ncols);
        let data = order[dr];
        if let Some(c) = rs.cell(data, ci) {
            let fmt = formats.get(ci).copied().unwrap_or_default();
            if contains_ignore_ascii_case(&format::apply(fmt, &c.to_value()), &q) {
                gs.active.set(Some((dr, ci)));
                gs.anchor.set(Some((dr, ci)));
                scroll_active_into_view(gs, dr, ci);
                return;
            }
        }
    }
}

/// Cap on cells scanned when counting matches for the find bar's `total`. A wide
/// *and* tall grid (rows × cols) can exceed this; the count then shows a trailing
/// `+` (`find_more`) and the scan stops. Chosen so the one-off debounced scan
/// stays well under a frame's worth of jank even on a large result set.
const FIND_COUNT_CELL_BUDGET: usize = 2_000_000;
/// Cap on collected match positions (memory bound); also flips `find_more`.
const FIND_MAX_HITS: usize = 100_000;

/// Scan the grid and collect the linear positions (`display_row * ncols + col`,
/// ascending) of every cell whose *displayed* value contains the query. Bounded
/// by [`FIND_COUNT_CELL_BUDGET`] / [`FIND_MAX_HITS`]; the bool is `find_more` (the
/// scan was truncated, so the count is a lower bound). Runs debounced off the UI
/// thread's keystroke path so a big grid doesn't stutter typing.
fn grid_find_hits(gs: GridState) -> (Vec<usize>, bool) {
    let q = gs.find_query.get_untracked();
    let rs = gs.rs.get_untracked();
    let order = gs.order.get_untracked();
    let formats = gs.formats.get_untracked();
    find_hits(&rs, &order, &formats, &q)
}

/// Which cells match `q`, as **display** positions (`display_row * ncols + col`),
/// and whether the scan stopped early.
///
/// Split out from [`grid_find_hits`] so the decision is testable without a live
/// grid — it was on A5's untested list, and it is what a 150 ms debounce calls
/// from a timer that may outlive the grid.
///
/// Two things it must get right and neither is obvious from the call site.
/// `order` is the display→data mapping, so the hits are where the user is
/// *looking*, not where the row sits in the result. And matching is against the
/// **displayed** text (`format::apply`), because the find bar searches what is on
/// screen: an epoch column formatted as a date has to be findable by the date.
///
/// `more` is true when either budget cut the scan short, so the bar can say
/// "500+" rather than reporting a floor as a total.
fn find_hits(
    rs: &ResultSet,
    order: &[usize],
    formats: &[ColumnFormat],
    q: &str,
) -> (Vec<usize>, bool) {
    if q.is_empty() {
        return (Vec::new(), false);
    }
    let ncols = rs.col_count();
    let mut hits = Vec::new();
    let mut more = false;
    let mut scanned = 0usize;
    'outer: for (dr, &data) in order.iter().enumerate() {
        for ci in 0..ncols {
            if scanned >= FIND_COUNT_CELL_BUDGET {
                more = true;
                break 'outer;
            }
            scanned += 1;
            if let Some(c) = rs.cell(data, ci) {
                let fmt = formats.get(ci).copied().unwrap_or_default();
                if contains_ignore_ascii_case(&format::apply(fmt, &c.to_value()), q) {
                    hits.push(dr * ncols + ci);
                    if hits.len() >= FIND_MAX_HITS {
                        more = true;
                        break 'outer;
                    }
                }
            }
        }
    }
    (hits, more)
}

fn grid_key(gs: GridState, nrows: usize, ncols: usize, e: &Event) -> EventPropagation {
    let Event::KeyDown(ke) = e else {
        return EventPropagation::Continue;
    };
    // The selection lives in *display* space, which is the real rows plus the
    // pending new rows rendered below them — so that, not `nrows`, is what every
    // navigation arm clamps against (see `nav_target`).
    let rows = nrows + gs.new_rows.with_untracked(|v| v.len());
    if rows == 0 || ncols == 0 {
        return EventPropagation::Continue;
    }
    let m = ke.modifiers;
    // Shift+Arrow extends the range from the anchor, the keyboard half of
    // shift-click and drag-select. `set_active` keeps the anchor put and moves
    // only the active end.
    let shift = m.shift();
    let ctrl = m.control() || m.meta();
    let active_opt = gs.active.get_untracked();
    let (r, c) = active_opt.unwrap_or((0, 0));
    let last_r = rows - 1;
    let last_c = ncols - 1;
    let page = ((gs.vp.get_untracked().height() / row_h()).floor() as usize).max(1);
    let go = |nav: Nav| {
        let (nr, nc) = nav_target(rows, ncols, page, (r, c), nav);
        set_active(gs, nr, nc, shift);
    };
    // With no cell selected yet, the first navigation keypress selects the
    // origin (0,0) instead of moving off it — otherwise Arrow-Down would skip
    // row 0 (and Arrow-Right column 0) on the very first press (§7.4).
    let is_nav = matches!(
        &ke.key.logical_key,
        Key::Named(
            NamedKey::ArrowDown
                | NamedKey::ArrowUp
                | NamedKey::ArrowRight
                | NamedKey::ArrowLeft
                | NamedKey::Home
                | NamedKey::End
                | NamedKey::PageDown
                | NamedKey::PageUp
        )
    );
    if active_opt.is_none() && is_nav {
        set_active(gs, 0, 0, shift);
        return EventPropagation::Stop;
    }
    match &ke.key.logical_key {
        Key::Named(NamedKey::ArrowDown) => go(Nav::Down),
        Key::Named(NamedKey::ArrowUp) => go(Nav::Up),
        Key::Named(NamedKey::ArrowRight) => go(Nav::Right),
        Key::Named(NamedKey::ArrowLeft) => go(Nav::Left),
        Key::Named(NamedKey::Home) => go(if ctrl { Nav::First } else { Nav::RowStart }),
        Key::Named(NamedKey::End) => go(if ctrl { Nav::Last } else { Nav::RowEnd }),
        Key::Named(NamedKey::PageDown) => go(Nav::PageDown),
        Key::Named(NamedKey::PageUp) => go(Nav::PageUp),
        Key::Named(NamedKey::Escape) => {
            // Esc closes the find bar or the goto popup first (so either closes
            // from anywhere in the grid, not only when its input is focused), then
            // the row view/edit panel, then the selection.
            if gs.find_open.get_untracked() {
                gs.find_open.set(false);
                gs.find_query.set(String::new());
            } else if gs.goto_open.get_untracked() {
                gs.goto_open.set(false);
                gs.goto_query.set(String::new());
            } else if gs.edit_row_open.get_untracked() {
                gs.edit_row_open.set(false);
            } else {
                gs.active.set(None);
                gs.anchor.set(None);
            }
        }
        Key::Named(NamedKey::Enter) if ctrl => commit_grid(gs),
        Key::Named(NamedKey::Enter) => {
            // Enter edits the active cell when it's editable; on a read-only
            // cell it does nothing (viewing is via the right-click View item).
            if gs.edit_model.get_untracked().editable(c) {
                start_edit(gs, r, c);
            }
        }
        Key::Character(s) if ctrl && matches!(s.as_str(), "c" | "C") => copy_selection(gs),
        Key::Character(s) if ctrl && matches!(s.as_str(), "v" | "V") => paste_selection(gs),
        Key::Character(s) if ctrl && matches!(s.as_str(), "a" | "A") => {
            gs.anchor.set(Some((0, 0)));
            gs.active.set(Some((last_r, last_c)));
        }
        Key::Character(s) if ctrl && matches!(s.as_str(), "f" | "F") => {
            gs.find_open.set(true); // its input autofocuses on mount
        }
        Key::Character(s) if ctrl && matches!(s.as_str(), "g" | "G") => {
            gs.goto_open.set(true); // its input autofocuses on mount
        }
        Key::Named(NamedKey::Delete)
            // Toggle "marked for deletion" over every real row the selection
            // covers (single writable table only). No selection → no-op.
            //
            // The whole range is driven to *one* state rather than each row
            // flipping its own: on a mixed selection a per-row toggle both marks
            // and unmarks, which reads as the key doing nothing. Any unmarked row
            // in range means "mark them all"; only an already-fully-marked range
            // unmarks. A pending row has nothing to delete and is skipped.
            if active_opt.is_some() && gs.edit_model.get_untracked().insert_target().is_some() => {
                let Some((r0, _, r1, _)) = gs.bounds_untracked() else {
                    return EventPropagation::Continue;
                };
                let order = gs.order.get_untracked();
                let rows: Vec<usize> = (r0..=r1).filter_map(|d| order.get(d).copied()).collect();
                if rows.is_empty() {
                    return EventPropagation::Continue;
                }
                // **One notification each, not one per row.** `toggle_delete`
                // writes `del_rows` *and* `dirty`, and every mounted cell's style
                // closure and content container tracks both — so Ctrl+A then Del
                // on a result at the default 200k row limit fired 400,000
                // synchronous notifications and locked the window. The
                // two-keystroke gesture this feature exists to enable was the one
                // that couldn't be used. Observable behaviour is unchanged.
                let mark = gs
                    .del_rows
                    .with_untracked(|d| delete_vote(|di| d.contains(&di), &rows));
                gs.del_rows.update(|d| {
                    for di in &rows {
                        if mark {
                            d.insert(*di);
                        } else {
                            d.remove(di);
                        }
                    }
                });
                // Marking supersedes an update: a row can't be both `UPDATE`d and
                // `DELETE`d in one commit.
                if mark {
                    let doomed: std::collections::HashSet<usize> = rows.into_iter().collect();
                    gs.dirty.update(|m| m.retain(|(di, _), _| !doomed.contains(di)));
                }
                gs.clear_bar();
            }
        _ => return EventPropagation::Continue,
    }
    EventPropagation::Stop
}

/// A thin vertical divider between toolbar icon groups. Extra horizontal margin
/// so it sits clear of the icons on either side — 8px, on top of the cluster's
/// own 3px gap, for 11px of air between the rule and the nearest glyph. The
/// icons carry a padded hitbox rather than a visible edge, so a divider set at
/// the plain group gap reads as *part of* the group beside it instead of the
/// boundary between two.
fn toolbar_sep() -> impl IntoView {
    empty().style(|s| {
        s.width(1.0)
            .height(theme::scaled(14.0))
            .flex_shrink(0.0_f32)
            .margin_horiz(theme::scaled(8.0))
            .background(theme::border())
    })
}

/// Toolbar above the grid: row/col/timing stats (+ a caveat when a sort is
/// applied to a capped result), plus the row-action / commit / copy icons.
/// DataGrip-style server-side filter field for the toolbar. Shown only for
/// filter/sort-eligible results (a single writable base table we can re-query);
/// hidden otherwise so we never imply a full-table filter we can't deliver. The
/// typed `WHERE` fragment is spliced into the base statement and re-run on Enter;
/// a clear ✕ (when a filter is active) and an inline red error round it out.
fn filter_bar(gs: GridState) -> impl IntoView {
    dyn_container(
        move || gs.base_sql.get().is_some() && gs.edit_model.get().insert_target().is_some(),
        move |eligible| {
            if !eligible {
                return empty().into_any();
            }
            // Local buffer, seeded from the persisted filter (which survives the
            // result reloads a filter/sort re-run triggers).
            let buf = RwSignal::new(gs.grid_query.with_untracked(|q| q.filter.clone()));
            let apply = move || {
                let text = buf.get_untracked().trim().to_string();
                gs.grid_query.update(|q| q.filter = text);
                gs.apply_grid_query();
            };
            let field = edit_field(
                buf,
                FieldCfg {
                    placeholder: "WHERE",
                    background: theme::bg_deepest,
                    font_size: theme::font_label,
                    // Borderless + square: the field's background is the row's, so
                    // the whole row reads as one input.
                    border_color: Some(crate::bg_transparent),
                    border_radius: 0.0,
                    on_submit: Some(Rc::new(apply)),
                    ..Default::default()
                },
            )
            .style(|s| s.flex_grow(1.0_f32).height_full());
            // Clear ✕ — only while a filter is actually applied. Empties the field
            // and re-runs unfiltered.
            let clear = dyn_container(
                move || gs.grid_query.with(|q| !q.filter.trim().is_empty()),
                move |active| {
                    if !active {
                        return empty().into_any();
                    }
                    container(icons::icon(icons::X, 16.0).style(|s| s.color(theme::text())))
                        .on_click_stop(move |_| {
                            buf.set(String::new());
                            gs.grid_query.update(|q| q.filter.clear());
                            gs.view_err.set(None);
                            gs.apply_grid_query();
                        })
                        // Match the Schema search field's clear ×.
                        .style(|s| {
                            s.flex_shrink(0.0_f32)
                                .items_center()
                                .margin_left(theme::scaled(6.0))
                                .color(theme::text())
                                .cursor(CursorStyle::Default)
                                .hover(|s| s.color(theme::text_dim()))
                        })
                        .into_any()
                },
            );
            // The whole row is the field: it fills full width + height, sharing the
            // field's background, with the clear ✕ sitting flush on the right. Filter
            // errors surface in the grid's bottom bar, not inline here.
            h_stack((field, clear))
                // Interacting with the filter field also dismisses the error bar.
                .on_event_cont(EventListener::PointerDown, move |_| gs.view_err.set(None))
                .style(|s| {
                    s.items_center()
                        .flex_row()
                        .gap(theme::scaled(4.0))
                        .width_full()
                        .height(theme::scaled(34.0))
                        .flex_shrink(0.0_f32)
                        .background(theme::bg_deepest())
                        .padding_right(theme::scaled(10.0))
                        .border_bottom(1.0)
                        .border_color(theme::border())
                })
                .into_any()
        },
    )
}

/// The toolbar strip's Tab order, left to right as the icons read. Local to this
/// ring — it holds nothing else — and spaced by ten so a control can be added
/// between two without renumbering.
const TB_COMMIT: u32 = 10;
const TB_DISCARD: u32 = 20;
const TB_ADD: u32 = 30;
const TB_DELETE: u32 = 40;
const TB_CLONE: u32 = 50;
const TB_AI: u32 = 60;
const TB_COPY: u32 = 70;
const TB_SAVE: u32 = 80;

#[allow(clippy::too_many_arguments)]
fn grid_toolbar(
    gs: GridState,
    nrows: usize,
    ncols: usize,
    elapsed_ms: u128,
    truncated: bool,
    capped_columns: Vec<String>,
    sort: RwSignal<SortState>,
    database: Option<String>,
    // What the whole statement would have returned, when the loaded rows are a
    // capped read of one whole table and its total is in hand — see the
    // `row_total` memo in `grid_view`, which is where those conditions are
    // decided. `None` means the line says only what it read.
    row_total: Memo<Option<schemaic_core::stats::RowCount>>,
    strip: crate::widgets::FocusRing,
) -> impl IntoView {
    // Escape's way home. Deferred inside `refocus_grid`, which is what makes it
    // win over the `ClearFocus` that `in_focus_ring`'s own Escape arm queues in
    // the same pass — see `in_strip_button`.
    let leave = move || refocus_grid(gs);
    // **Closing a menu must give the keyboard back to the icon that opened it.**
    // The panel is a `focus_root` with no other root above it out here, so its
    // teardown drops focus altogether and F6 — a listener on the grid body — had
    // nothing to fire on. Asked of `keyboard_nav` because it is only true of a
    // menu the keyboard raised: after a *click*, taking focus to the icon would
    // take the arrow keys away from the grid's own cell navigation.
    //
    // By tabindex and deferred, both for the reasons `in_ring_dropdown` gives:
    // the strip may have been rebuilt by the action just run, and floem's focus
    // request has no existence check, so a captured id can park the keyboard on a
    // removed view.
    let focus_icon = move |tabindex: u32, ring: &crate::widgets::FocusRing| {
        let ring = ring.clone();
        floem::action::exec_after(std::time::Duration::ZERO, move |_| ring.focus_at(tabindex));
    };
    let publish_return = move |tabindex: u32, ring: &crate::widgets::FocusRing| {
        if !crate::widgets::keyboard_nav().get_untracked() {
            return;
        }
        let ring = ring.clone();
        crate::widgets::set_menu_return(Rc::new(move || focus_icon(tabindex, &ring)));
    };
    // The three dropdown icons all place their panel the same way, and this is the
    // one spelling of it — because the *same* value is what tells an icon the menu
    // already up is its own (see [`PopupAnchor`]). Written twice, an anchor that
    // drifted by a pixel would leave the menu opening correctly and silently
    // refusing to toggle shut.
    let anchor_below = |o: Point| {
        let sz = 16.0; // every toolbar glyph above
        PopupAnchor::BelowIcon(o.x, o.x + sz, o.y + sz)
    };
    // Is the menu currently up the one this icon opened? The rule, and why the
    // anchor is what answers it, is `widgets::menu_anchored_at` — shared with the
    // status bar's segments, which toggle off the same channel.
    let menu_is_mine = move |origin: RwSignal<Point>| {
        crate::widgets::menu_anchored_at(
            gs.popup.get_untracked().is_some(),
            gs.popup_anchor.get_untracked(),
            anchor_below(origin.get_untracked()),
        )
    };
    // The same question asked *reactively*, for the icon's own colour: these run
    // inside `.style()`, which has to re-evaluate when the menu opens or closes,
    // so they subscribe where `menu_is_mine` (an event-handler read) deliberately
    // does not.
    let menu_is_mine_live = move |origin: RwSignal<Point>| {
        crate::widgets::menu_anchored_at(
            gs.popup.with(|p| p.is_some()),
            gs.popup_anchor.get(),
            anchor_below(origin.get()),
        )
    };
    // **An icon that moves under its own open menu carries the anchor with it.**
    // Identity here is the anchor (see `menu_anchored_at`), and these icons are
    // right-aligned in the strip — so a window resize, or the AI panel opening,
    // changed `o.x` and the two stopped comparing equal: the panel stayed at the
    // pixel it opened at, the icon reverted from the accent although its menu was
    // still up, and clicking it re-opened instead of closing. Re-stamping keeps
    // the two the same value *and* makes the overlay place the panel under the
    // icon, since it reads the stored anchor.
    let follow_menu = move |origin: RwSignal<Point>, p: Point| {
        let was_mine = crate::widgets::menu_anchored_at(
            gs.popup.with_untracked(|m| m.is_some()),
            gs.popup_anchor.get_untracked(),
            anchor_below(origin.get_untracked()),
        );
        origin.set(p);
        if was_mine {
            gs.popup_anchor.set(Some(anchor_below(p)));
        }
    };
    // Pressing the icon again closes its menu, rather than dismissing and
    // rebuilding the identical panel — the toggle the schema panel's eye and gear
    // and the connection switcher already have.
    //
    // `publish_return` is deliberately *not* called on this path. That slot is
    // consumed by the next `menu_panel` as it builds, and no panel is being built
    // here, so arming it would leave a return for the next keyboard-opened menu
    // anywhere in the app to take. The keyboard is handed back directly instead,
    // and only when it was the keyboard that pressed: the panel is a `focus_root`
    // with no other root above it out here, so its teardown would otherwise drop
    // focus and leave F6 with nothing to fire on.
    let close_mine = move |tabindex: u32, ring: &crate::widgets::FocusRing| {
        gs.popup.set(None);
        if crate::widgets::keyboard_nav().get_untracked() {
            focus_icon(tabindex, ring);
        }
    };
    // The database leads the line, because it is the fact that says what the rest
    // of the line is *about*. Taken from the result rather than the tab: the tab's
    // selection moves on, and a result that outlived it must not claim the new one
    // (`ResultSet::database`). A connection with no default database says nothing
    // rather than inventing a name.
    let scope = database.map(|d| format!("{d} · ")).unwrap_or_default();
    // A `label` rather than `text`, for the one part of this line that isn't
    // settled at build time: a capped result's total arrives from a catalogue
    // query, and the line reads `1,000 of ~4.2m rows` once it does. The row
    // segment — figure, noun, and the `(capped)` notice when the figure hasn't
    // already made it — is `stats::rows_read_clause`'s whole job, because it is
    // the composition of those three that has to read well, not each alone.
    //
    // **This is the segment that gives way when the strip runs out of room**
    // (`min_width(0)` + `text_ellipsis` below, against a `flex_shrink(0)` icon
    // cluster): it is the only part of the line that is pure description, so a
    // narrow panel eats the database name and the timing before it touches an
    // action or a warning.
    let stats = label(move || {
        format!(
            "{scope}{} · {ncols} {} · {elapsed_ms} ms",
            schemaic_core::stats::rows_read_clause(nrows, row_total.get(), truncated),
            plural(ncols, "col", "cols"),
        )
    })
    .style(|s| {
        s.color(theme::text_dim())
            .font_size(theme::font_label())
            .min_width(0.0)
            .text_ellipsis()
    });
    // A column whose 512 MiB text arena filled up renders blank from that row
    // on. Said out loud, because the alternative is the user discovering empty
    // cells partway down a result with nothing to attribute them to — and unlike
    // the row cap, this one loses data inside rows that are present.
    let arena_note = if capped_columns.is_empty() {
        empty().into_any()
    } else {
        // Ellipsizable like `stats`, and for the opposite reason: the column
        // list can be long enough to push the whole strip on its own, and a
        // warning nobody can read because it shoved the buttons off the edge is
        // worse than one that ends in `…`.
        text(format!(
            "· {} too large to hold in full — later rows show blank",
            capped_columns.join(", ")
        ))
        .style(|s| {
            s.color(theme::error())
                .font_size(theme::font_label())
                .min_width(0.0)
                .text_ellipsis()
        })
        .into_any()
    };
    // Sorting a capped result reorders only the fetched subset — flag it.
    let caveat = dyn_container(
        move || truncated && sort.get().is_some(),
        move |show| {
            if show {
                text("· sorted subset (capped) — not the full order")
                    .style(|s| {
                        s.color(theme::error())
                            .font_size(theme::font_label())
                            .min_width(0.0)
                            .text_ellipsis()
                    })
                    .into_any()
            } else {
                empty().into_any()
            }
        },
    )
    // The `dyn_container` is what the strip lays out, so the squeeze has to be
    // allowed through here as well — a wrapper at its min-content width would
    // hold the text at full size however the child is styled.
    .style(|s| s.min_width(0.0));
    // **Getting past the cap, for this result only.**
    //
    // The cap is a client-side cutoff of the result stream (`db::collect_rows`),
    // not a `LIMIT`/`OFFSET`, so there is no cursor to advance and nothing to
    // "load more" of: the action **re-runs the whole statement** with a bigger
    // ceiling, and on an unordered query the second read can legitimately
    // disagree with the first. So the label names the number rather than saying
    // "more", and the verb is *read*, not *load*.
    //
    // Shown only where the re-run can actually happen — the statement is rebuilt
    // from `base_sql`, which a Run Everything panel does not have. A missing
    // action beats one that does nothing.
    //
    // **And only where it may happen.** The offer is gated on the *content* of
    // the statement, not on `base_sql` merely existing: `base_sql` is a tab
    // signal that every manual run overwrites, for any statement kind, so a
    // capped `SELECT` followed by a `DELETE` used to leave this link drawn over
    // the new base — and clicking it re-ran the `DELETE`, through `apply_view`,
    // which is a bare run with no verdict. `rerun_statement` refuses a write
    // outright (see its doc for why a `Confirm` would be the wrong answer from a
    // link that says *read*), and the click below asks it again.
    let read_more = dyn_container(
        move || {
            truncated
                && gs.base_sql.with(|b| {
                    b.as_deref().is_some_and(|base| {
                        rerun_statement(base, &gs.grid_query.get(), gs.dialect).is_some()
                    })
                })
        },
        move |show| {
            if !show {
                return empty().into_any();
            }
            // **Accent-coloured, because it is the only thing on this line that
            // does something.** It used to be `text_dim` like the description
            // beside it and reached the accent only under the pointer, which
            // put the whole affordance behind a hover: a user who never swept
            // that word never learned the cap could be lifted at all. The
            // separator stays dim — it belongs to the line, not to the offer,
            // and a blue `·` reads as part of the link.
            //
            // Two views rather than one string, and the colour is driven off an
            // explicit hovered signal rather than `.hover()`: a parent's hover
            // colour does not cascade to a child (the same reason the commit
            // control keeps `commit_hov`), and the click target is the pair, so
            // the dot has to brighten the words with it.
            let offer_hov = RwSignal::new(false);
            // A `label`, and for the same reason `stats` beside it is one: the
            // total arrives from a catalogue query after the strip is built, and
            // it is what decides whether the offer is a step ("read 5k rows") or
            // the whole thing ("read all rows"). Built once, the offer named a
            // million rows for a table with 292 thousand.
            h_stack((
                text("·").style(|s| s.color(theme::text_dim()).font_size(theme::font_label())),
                label(move || schemaic_core::stats::read_more_offer(nrows, row_total.get()).1)
                    .style(move |s| {
                        // Stays blue on hover and steps *away from the surface*
                        // (`accent_hover`) rather than going white: the accent is
                        // what says the words are pressable, and a hover that
                        // trades it for the same colour as ordinary text reads as
                        // the link switching off at the moment it is aimed at.
                        let c = if offer_hov.get() {
                            theme::accent_hover()
                        } else {
                            theme::accent()
                        };
                        s.color(c).font_size(theme::font_label())
                    }),
            ))
            .style(|s| s.flex_row().items_center().gap(theme::scaled(4.0)))
            .on_event_cont(EventListener::PointerEnter, move |_| offer_hov.set(true))
            .on_event_cont(EventListener::PointerLeave, move |_| offer_hov.set(false))
            .on_click_stop(move |_| {
                let Some(sql) = gs.current_statement() else {
                    return;
                };
                // Asked again at the click rather than captured: the total
                // may have landed since the label was last drawn, and the
                // cap has to match the words the user just read.
                let (cap, _) =
                    schemaic_core::stats::read_more_offer(nrows, row_total.get_untracked());
                // The override is per-tab and transient: the next manual run
                // clears it, because getting past the cap once is not a
                // decision about every query the user will ever run.
                gs.row_cap_override.set(Some(cap));
                if let Some(run) = gs.apply_view.get_untracked() {
                    run(sql);
                }
            })
            .into_any()
        },
    )
    // The one piece of prose on this line that is a *control*: it keeps its full
    // width while `stats` gives way, because a half-word offer is not one, and
    // clipping the click target is worse than clipping the description.
    .style(|s| s.flex_shrink(0.0_f32));

    // Commit / discard, shown only when there are staged changes (cell edits +
    // pending new rows + pending deletes). Sits first in the icon cluster, followed
    // by a separator. Commit is a green (grid_edit_staged #509950) button — check
    // glyph + the change count (Ctrl+Enter); discard a red (#9D3434) ✗. Both
    // background-free with the same padded hitbox as the other icons; brighten on
    // hover.
    // A clone per rebuilding block: each `dyn_container` child closure owns what
    // it captures, and re-clones inside because it runs on every rebuild.
    let strip_commit = strip.clone();
    let strip_rows = strip.clone();
    let strip_ai = strip.clone();
    let commit_ctrl = dyn_container(
        move || {
            (
                gs.dirty.with(|d| d.len())
                    + gs.new_rows.with(|v| v.len())
                    + gs.del_rows.with(|d| d.len()),
                gs.commit_busy.get(),
            )
        },
        move |(n, busy)| {
            if n == 0 {
                return empty().into_any();
            }
            let label = if busy {
                "Committing…".to_string()
            } else {
                format!("{n}")
            };
            // Hover brightens glyph + count; a parent `.hover` colour won't cascade
            // to the child icon/text, so drive it off an explicit hovered signal.
            let commit_hov = RwSignal::new(false);
            let commit_color = move || {
                if commit_hov.get() {
                    theme::grid_edit_staged_hover()
                } else {
                    theme::grid_edit_staged()
                }
            };
            let commit = h_stack((
                icons::icon(icons::CIRCLE_CHECK, 16.0)
                    .style(move |s| s.color(commit_color()).flex_shrink(0.0_f32)),
                text(label).style(move |s| {
                    s.font_size(theme::font_label())
                        .color(commit_color())
                        .margin_left(theme::scaled(4.0))
                }),
            ))
            .on_click_stop(move |_| commit_grid(gs))
            .on_event_cont(EventListener::PointerEnter, move |_| commit_hov.set(true))
            .on_event_cont(EventListener::PointerLeave, move |_| commit_hov.set(false))
            .style(|s| {
                s.items_center()
                    .padding_vert(theme::scaled(3.0))
                    .padding_horiz(theme::scaled(5.0))
                    .cursor(CursorStyle::Default)
            })
            // The count beside the glyph is the one label in the strip, and a bare
            // number says neither what it counts nor what pressing it does. Built
            // from this rebuild's `n`/`busy` rather than read reactively: the
            // `dyn_container` above already keys on both, so the face and its tip
            // are replaced together.
            .tooltip(move || {
                let t = if busy {
                    "Committing…".to_string()
                } else if n == 1 {
                    "Commit 1 change (Ctrl+Enter)".to_string()
                } else {
                    format!("Commit {n} changes (Ctrl+Enter)")
                };
                text(t).style(crate::widgets::tooltip_style)
            });
            let discard_hov = RwSignal::new(false);
            let discard = container(icons::icon(icons::CIRCLE_X, 16.0).style(move |s| {
                let c = if discard_hov.get() {
                    theme::grid_edit_discard_hover()
                } else {
                    theme::grid_edit_discard()
                };
                s.color(c).flex_shrink(0.0_f32)
            }))
            .on_click_stop(move |_| discard_edits(gs))
            .on_event_cont(EventListener::PointerEnter, move |_| discard_hov.set(true))
            .on_event_cont(EventListener::PointerLeave, move |_| discard_hov.set(false))
            .style(|s| {
                s.items_center()
                    .padding_vert(theme::scaled(3.0))
                    .padding_horiz(theme::scaled(5.0))
                    .cursor(CursorStyle::Default)
            })
            // "all", because it throws away the pending deletes and new rows too,
            // not just the edited cells — the ✗ sits next to a count that reads
            // like it belongs to the ✓ alone.
            .tooltip(|| text("Discard all pending changes").style(crate::widgets::tooltip_style));
            let (r1, r2) = (strip_commit.clone(), strip_commit.clone());
            h_stack((
                in_strip_button(commit, r1, TB_COMMIT, true, leave, move || commit_grid(gs)),
                in_strip_button(discard, r2, TB_DISCARD, true, leave, move || {
                    discard_edits(gs)
                }),
                toolbar_sep(),
            ))
            .style(|s| s.items_center().flex_row().gap(theme::scaled(3.0)))
            .into_any()
        },
    );
    // (A commit failure now shows in the panel-level error bar at the bottom — the
    // editor error-bar pattern — instead of inline in the toolbar.)
    // Row actions — new / delete / clone — shown only when the result maps to a
    // single writable table (`insert_target`; a join or read-only result hides the
    // group). Delete + clone need a real row selected; disabled (30% dim) otherwise.
    let row_selected = move || gs.active.get().map(|(r, _)| r < nrows).unwrap_or(false);
    let selected_data_row = move || -> Option<usize> {
        let (r, _) = gs.active.get_untracked()?;
        if r >= nrows {
            return None; // a pending new row isn't a deletable/clonable data row
        }
        gs.order.get_untracked().get(r).copied()
    };
    // Tracks `row_selected` as well as insertability: − and clone are disabled
    // without a selected row, and a disabled control is deliberately *not* a ring
    // member, so the strip has to be rebuilt when that flips.
    let row_actions = dyn_container(
        move || {
            (
                gs.edit_model.get().insert_target().is_some(),
                row_selected(),
            )
        },
        move |(show, live)| {
            if !show {
                return empty().into_any();
            }
            // The face keeps its own click listener and the ring gets an
            // equivalent one for Enter/Space — `in_ring_button`'s rule, and the
            // reason it is two closures rather than one: the registered view is
            // the *wrapper*, and floem fires `Click` on the focused view for
            // Enter, so a click listener there would activate twice.
            //
            // `live` is a snapshot, which is only sound because the `dyn_container`
            // above tracks `row_selected()` too: a disabled control is not a ring
            // member, so the strip has to be rebuilt when that changes or Tab
            // would keep walking onto a control that no longer does anything.
            let del = move || {
                if let Some(di) = selected_data_row() {
                    gs.toggle_delete(di);
                }
            };
            let clone = move || {
                if let Some(di) = selected_data_row() {
                    clone_row(gs, di);
                }
            };
            let (r1, r2, r3) = (strip_rows.clone(), strip_rows.clone(), strip_rows.clone());
            // Tips on the *face*, before `in_strip_button` wraps it — `.tooltip()`
            // allocates a fresh `ViewId`, and it is the wrapper that carries the
            // ring's focus outline and Enter/Space arm (see `in_ring_button`).
            // Decorating the wrapper instead would put an id in the ring that
            // paints nothing, which is the bug `row_button` documents.
            //
            // Delete and clone keep their tip while dimmed, and it names the
            // selection: "the selected row" is also the answer to why the glyph is
            // inert right now.
            h_stack((
                in_strip_button(
                    toolbar_icon(icons::PLUS, 0.0, 0.0, || true, move || add_pending_row(gs))
                        .tooltip(|| text("Add a row").style(crate::widgets::tooltip_style)),
                    r1,
                    TB_ADD,
                    true,
                    leave,
                    move || add_pending_row(gs),
                ),
                in_strip_button(
                    toolbar_icon(icons::MINUS, 0.0, 0.0, row_selected, del).tooltip(|| {
                        text("Mark the selected row for deletion (Del)")
                            .style(crate::widgets::tooltip_style)
                    }),
                    r2,
                    TB_DELETE,
                    live,
                    leave,
                    del,
                ),
                in_strip_button(
                    toolbar_icon(icons::COPY_PLUS, 0.0, 0.0, row_selected, clone).tooltip(|| {
                        text("Clone the selected row").style(crate::widgets::tooltip_style)
                    }),
                    r3,
                    TB_CLONE,
                    live,
                    leave,
                    clone,
                ),
                toolbar_sep(),
            ))
            .style(|s| s.items_center().flex_row().gap(theme::scaled(3.0)))
            .into_any()
        },
    );
    // Copy icon → themed dropdown (JSON / CSV / SQL). Same neutral styling + padded
    // hitbox as the other icons; `on_event_stop(PointerDown)` keeps the root
    // pointer-down dismissal from closing the menu the same click opens it. The
    // `on_move` tracks the glyph origin so the dropdown anchors under it.
    let copy_origin = RwSignal::new(Point::ZERO);
    let copy_hov = RwSignal::new(false);
    // Named, because the pointer and the keyboard both raise it: the face keeps
    // the click listener and `in_strip_button` gets the same action for
    // Enter/Space (see `in_ring_button` on why they cannot be one listener).
    let strip_copy = strip.clone();
    // `Rc`, not a bare closure: it captures the ring (not `Copy`) and is used
    // twice — the face's click listener and the ring's Enter/Space arm, which
    // `in_ring_button` requires to be separate listeners.
    let open_copy: Rc<dyn Fn()> = Rc::new(move || {
        // A second press closes what the first opened.
        if menu_is_mine(copy_origin) {
            close_mine(TB_COPY, &strip_copy);
            return;
        }
        // Close any other open menu (schema eye/settings, connection switcher, …)
        // so this dropdown is mutually exclusive with them.
        if let Some(d) = gs.dismiss.get_untracked() {
            (d)();
        }
        publish_return(TB_COPY, &strip_copy);
        // Anchor the panel just below the icon (left/right edges + bottom).
        // `on_move` reports the *view's* window origin — floem fires it during
        // layout, not on pointer movement — so this is right however the menu was
        // raised.
        gs.popup_width.set(grid_copy_menu_w());
        gs.popup_anchor
            .set(Some(anchor_below(copy_origin.get_untracked())));
        gs.popup.set(Some(
            ExportFormat::ALL
                .iter()
                .map(|&f| {
                    MenuEntry::action(f.label(), move || {
                        let _ = floem::Clipboard::set_contents(render_export(gs, f));
                    })
                })
                .collect(),
        ));
    });
    let copy_click = open_copy.clone();
    let copy_menu = container(
        icons::icon(icons::COPY, 16.0)
            .on_move(move |p| follow_menu(copy_origin, p))
            .style(move |s| {
                s.color(crate::widgets::menu_icon_color(
                    menu_is_mine_live(copy_origin),
                    copy_hov.get(),
                ))
                .flex_shrink(0.0_f32)
            }),
    )
    .on_click_stop(move |_| (copy_click)())
    .on_event_cont(EventListener::PointerEnter, move |_| copy_hov.set(true))
    .on_event_cont(EventListener::PointerLeave, move |_| copy_hov.set(false))
    .on_event_stop(
        EventListener::PointerDown,
        crate::widgets::menu_trigger_press,
    )
    .style(|s| {
        s.items_center()
            .padding_vert(theme::scaled(3.0))
            .padding_horiz(theme::scaled(5.0))
            .cursor(CursorStyle::Default)
    })
    // Trailing `…` because it raises the format menu rather than copying — the
    // convention the menu-opening icons in the monitor toolbar already follow.
    // Deliberately *not* labelled Ctrl+C: that key copies the selection straight
    // to the clipboard, which is a different action from this menu.
    .tooltip(|| text("Copy the results…").style(crate::widgets::tooltip_style));

    // Download icon → the same format dropdown as Copy, but each choice opens a
    // save dialog and writes the file. Identical styling/anchoring to `copy_menu`
    // so the pair reads as one control: copy it, or save it.
    let save_origin = RwSignal::new(Point::ZERO);
    let save_hov = RwSignal::new(false);
    let strip_save = strip.clone();
    let open_save: Rc<dyn Fn()> = Rc::new(move || {
        if menu_is_mine(save_origin) {
            close_mine(TB_SAVE, &strip_save);
            return;
        }
        if let Some(d) = gs.dismiss.get_untracked() {
            (d)();
        }
        publish_return(TB_SAVE, &strip_save);
        gs.popup_width.set(grid_copy_menu_w());
        gs.popup_anchor
            .set(Some(anchor_below(save_origin.get_untracked())));
        gs.popup.set(Some(export_menu(
            gs,
            row_total.get_untracked(),
            sort.with_untracked(Option::is_some),
        )));
    });
    let save_click = open_save.clone();
    let save_menu = container(
        icons::icon(icons::DOWNLOAD, 16.0)
            .on_move(move |p| follow_menu(save_origin, p))
            .style(move |s| {
                s.color(crate::widgets::menu_icon_color(
                    menu_is_mine_live(save_origin),
                    save_hov.get(),
                ))
                .flex_shrink(0.0_f32)
            }),
    )
    .on_click_stop(move |_| (save_click)())
    .on_event_cont(EventListener::PointerEnter, move |_| save_hov.set(true))
    .on_event_cont(EventListener::PointerLeave, move |_| save_hov.set(false))
    .on_event_stop(
        EventListener::PointerDown,
        crate::widgets::menu_trigger_press,
    )
    .style(|s| {
        s.items_center()
            .padding_vert(theme::scaled(3.0))
            .padding_horiz(theme::scaled(5.0))
            .cursor(CursorStyle::Default)
    })
    // Named against its twin: the two icons are identical but for the glyph, and
    // "Export" would leave the pair reading as two spellings of the same thing.
    .tooltip(|| text("Save the results to a file…").style(crate::widgets::tooltip_style));

    // AI seed-data menu → purple-sparkle actions (Fill Value / Insert Row / Seed
    // Table). Gated on a single writable table, like the row actions above. The
    // trigger is a neutral toolbar sparkle (same styling as the copy icon); the
    // *menu items* carry the purple sparkle, matching "AI Summary" / "Ask AI". The
    // menu anchors below the icon via the shared `ui.popup_menu` channel. Actions
    // are stubbed pending the one-shot AI pipeline.
    let ai_menu = dyn_container(
        move || gs.edit_model.get().insert_target().is_some(),
        move |show| {
            if !show {
                return empty().into_any();
            }
            let ai_origin = RwSignal::new(Point::ZERO);
            let ai_hov = RwSignal::new(false);
            let strip_ai_open = strip_ai.clone();
            let open_ai: Rc<dyn Fn()> = Rc::new(move || {
                // Ahead of the busy guard: a generation started while the menu was
                // up must not leave it stuck open with its own icon inert.
                if menu_is_mine(ai_origin) {
                    close_mine(TB_AI, &strip_ai_open);
                    return;
                }
                if gs.ai_busy.get_untracked() {
                    return; // a generation is already running
                }
                // Mutually exclusive with the other toolbar/schema menus.
                if let Some(d) = gs.dismiss.get_untracked() {
                    (d)();
                }
                publish_return(TB_AI, &strip_ai_open);
                gs.popup_width.set(grid_copy_menu_w());
                gs.popup_anchor
                    .set(Some(anchor_below(ai_origin.get_untracked())));
                // AI Fill Value targets the active cell — enabled only when an
                // editable cell is selected (a read-only/expression cell can't be
                // filled).
                let fill_enabled = gs
                    .active
                    .get_untracked()
                    .map(|(_, ci)| gs.edit_model.get_untracked().editable(ci))
                    .unwrap_or(false);
                // Every entry in this menu puts real values in a prompt: Fill and
                // Insert carry the row being completed, Seed samples the table
                // to imitate it, and Attach is rows outright. So on a
                // schema-only connection the menu has nothing to offer, and says
                // that rather than opening empty.
                let entries = if !ai_data_of(gs).may_attach() {
                    // One `popup.set` per opener (see the `popup_anchor_gate`
                    // test): the refusal is an entry, not an early return.
                    vec![
                        MenuEntry::action("AI actions send data — off for this connection", || {})
                            .disabled(true),
                    ]
                } else {
                    vec![
                        MenuEntry::action_icon(
                            "AI fill value",
                            (icons::SPARKLES, theme::key_foreign),
                            move || ai_fill_value(gs),
                        )
                        .disabled(!fill_enabled),
                        MenuEntry::action_icon(
                            "AI insert row",
                            (icons::SPARKLES, theme::key_foreign),
                            move || ai_insert_row(gs),
                        ),
                        MenuEntry::action_icon(
                            "AI seed table…",
                            (icons::SPARKLES, theme::key_foreign),
                            move || open_seed_popover(gs),
                        ),
                        MenuEntry::Separator,
                        // Below the separator because it is the one entry here that
                        // *sends rows out* rather than asking for rows in. The label
                        // says how many, so the count is read before the click, not
                        // discovered in the chip afterwards.
                        MenuEntry::action_icon(
                            attach_label(gs),
                            (icons::SPARKLES, theme::key_foreign),
                            move || attach_to_chat(gs, true),
                        )
                        .disabled(gs.order.get_untracked().is_empty()),
                    ]
                };
                gs.popup.set(Some(entries));
            });
            let ai_click = open_ai.clone();
            let face = container(
                icons::icon(icons::SPARKLES, 16.0)
                    .on_move(move |p| follow_menu(ai_origin, p))
                    .style(move |s| {
                        // Dimmed + inert while a request is in flight — that arm
                        // stays first: a busy icon is not a control right now,
                        // whatever the menu or the pointer is doing.
                        let c = if gs.ai_busy.get() {
                            theme::text_muted().multiply_alpha(0.3)
                        } else {
                            crate::widgets::menu_icon_color(
                                menu_is_mine_live(ai_origin),
                                ai_hov.get(),
                            )
                        };
                        s.color(c).flex_shrink(0.0_f32)
                    }),
            )
            .on_click_stop(move |_| (ai_click)())
            .on_event_cont(EventListener::PointerEnter, move |_| ai_hov.set(true))
            .on_event_cont(EventListener::PointerLeave, move |_| ai_hov.set(false))
            .on_event_stop(
                EventListener::PointerDown,
                crate::widgets::menu_trigger_press,
            )
            .style(|s| {
                s.items_center()
                    .padding_vert(theme::scaled(3.0))
                    .padding_horiz(theme::scaled(5.0))
                    .cursor(CursorStyle::Default)
            })
            // A bare sparkle is the least self-describing glyph in the strip, and
            // it is the one control here that writes rows. Read reactively for the
            // in-flight state, since `ai_busy` dims this face without rebuilding
            // the block around it — a tip still offering to generate would be the
            // only thing on screen disagreeing with the greyed glyph.
            .tooltip(move || {
                let t = if gs.ai_busy.get() {
                    "Generating…"
                } else {
                    "Generate rows with AI…"
                };
                text(t).style(crate::widgets::tooltip_style)
            });
            // `open_ai` no-ops while a request is in flight, so the control stays
            // in the ring rather than leaving and re-entering it on every
            // generation — a Tab stop that came and went with a background task
            // would move the strip under the user mid-walk.
            in_strip_button(face, strip_ai.clone(), TB_AI, true, leave, move || {
                (open_ai)()
            })
            .into_any()
        },
    );

    // The icon cluster — 3px between icons (on top of each icon's padded hitbox),
    // separators pushed further out by their own 8px margin:
    // [commit ✓][discard ✗] │ [＋][－][clone] │ [✦ AI][copy][save].
    //
    // Every one of them carries a tooltip. Nothing here is labelled but the commit
    // count, and that is a bare number; the glyphs that do the least reversible
    // work (discard, delete, AI) are among the least self-describing.
    let icons_cluster = h_stack((
        commit_ctrl,
        row_actions,
        ai_menu,
        in_strip_button(copy_menu, strip.clone(), TB_COPY, true, leave, move || {
            (open_copy)()
        }),
        in_strip_button(save_menu, strip.clone(), TB_SAVE, true, leave, move || {
            (open_save)()
        }),
    ))
    // **The half of the strip that never gives way.** Everything to its left is
    // words and can be ellipsized; these are the only way to commit, export or
    // add a row, and a flex row shrinks its children before it overflows — so
    // without this the description won the fight and pushed the buttons off the
    // right edge of a narrow panel.
    .style(|s| {
        s.items_center()
            .flex_row()
            .gap(theme::scaled(3.0))
            .flex_shrink(0.0_f32)
    });

    h_stack((
        stats,
        read_more,
        arena_note,
        caveat,
        // Shrinks to nothing before anything else does — it is only here to
        // push the icons right when the line is short.
        empty().style(|s| s.flex_grow(1.0_f32).min_width(0.0)),
        icons_cluster,
    ))
    .style(|s| {
        // Fixed height + centered so the commit control appearing/leaving never
        // nudges the grid up or down.
        s.width_full()
            .flex_row()
            .items_center()
            .gap(theme::scaled(6.0))
            .height(theme::scaled(28.0))
            .flex_shrink(0.0_f32)
            .padding_left(theme::scaled(12.0))
            // Less right padding than left: the copy icon carries its own 5px hitbox
            // padding, so 7 + 5 lands its glyph ~12px from the edge (matching the
            // left inset) instead of too far in.
            .padding_right(theme::scaled(7.0))
            .border_bottom(1.0)
            .border_color(theme::border())
    })
}

// `pos` = display position (drives zebra striping + selection coords); `data_idx`
// = index into the result set (post-sort permutation).

/// Build one data cell at `(pos, ci)`. Only the column's static `numeric` flag is
/// captured here; the cell's *value* is read reactively from `gs.rs` inside
/// `data_cell`, so a post-commit splice updates it in place. `pending` is
/// `Some(pending_index)` for a staged new row, `None` for a real result row.
fn cell_at(
    gs: GridState,
    pos: usize,
    data_idx: usize,
    ci: usize,
    pending: Option<usize>,
) -> impl IntoView {
    let numeric = gs
        .rs
        .get_untracked()
        .columns
        .get(ci)
        .map(|c| c.is_numeric())
        .unwrap_or(false);
    data_cell(gs, pos, data_idx, ci, numeric, pending)
}

/// Row-number gutter cell (frozen). Clicking selects the whole display row, and
/// dragging down the gutter selects the rows it crosses. A pending new row shows
/// a `*` marker instead of a number.
fn gutter_cell(gs: GridState, pos: usize, ncols: usize, pending: Option<usize>) -> impl IntoView {
    let label = if pending.is_some() {
        "*".to_string()
    } else {
        format!("{}", pos + 1)
    };
    container(text(label).style(|s| s.font_size(theme::font_label()).color(theme::text_faint())))
        // Selection happens on **press**, not on click, because that is what arms
        // the drag — a click fires only after the release, by which time the
        // rows the pointer crossed are gone.
        .on_event(EventListener::PointerDown, move |e| {
            if let Event::PointerDown(pe) = e {
                gs.dismiss_overlays();
                if pe.button.is_primary() {
                    // Shift extends from the row the last gesture anchored on, so
                    // click-then-shift-click picks a range like everywhere else.
                    let anchor_row = if pe.modifiers.shift() {
                        gs.anchor.get_untracked().map(|(r, _)| r).unwrap_or(pos)
                    } else {
                        pos
                    };
                    let (anchor, active) =
                        schemaic_core::model::row_range_selection(anchor_row, pos, ncols);
                    gs.anchor.set(Some(anchor));
                    gs.active.set(Some(active));
                    // A *row* drag, kept apart from the cells' `selecting` flag:
                    // sharing one would let a drag that started in the gutter
                    // collapse to a single column the moment it crossed a cell.
                    gs.row_selecting.set(true);
                    if let Some(f) = gs.focus_id.get_untracked() {
                        f.request_focus();
                    }
                    return EventPropagation::Stop;
                }
            }
            EventPropagation::Continue
        })
        // Drag-select down the gutter: the anchor stays on the row the press
        // landed on, the active end follows the pointer. Ended by the body's
        // pointer-up effect, wherever the release happens.
        .on_event_cont(EventListener::PointerEnter, move |_| {
            if gs.row_selecting.get_untracked() {
                let anchor_row = gs.anchor.get_untracked().map(|(r, _)| r).unwrap_or(pos);
                let (anchor, active) =
                    schemaic_core::model::row_range_selection(anchor_row, pos, ncols);
                gs.anchor.set(Some(anchor));
                gs.active.set(Some(active));
            }
        })
        // `DoubleClick` swallows the second `PointerUp`, so neither this cell's
        // release nor the body's ever arrives to end the drag — the flag would
        // stay armed with no button held and the next hover would drag a
        // selection out of nowhere. Exactly the guard `data_cell` carries.
        .on_double_click_stop(move |_| {
            gs.row_selecting.set(false);
            gs.selecting.set(false);
        })
        // Right-click → the row menu. A press inside the current selection keeps
        // it (that is how "attach these 5 rows" is reached at all); outside, it
        // selects the row under the cursor first, so the menu always describes
        // something the user can see.
        .on_secondary_click_stop(move |_| {
            let inside =
                matches!(gs.bounds_untracked(), Some((r0, _, r1, _)) if pos >= r0 && pos <= r1);
            if !inside {
                let (anchor, active) = schemaic_core::model::row_selection(pos, ncols);
                gs.anchor.set(Some(anchor));
                gs.active.set(Some(active));
            }
            gs.popup_anchor.set(None); // right-click → open at the cursor
            gs.popup.set(Some(gutter_menu(gs, pos, pending)));
        })
        .style(move |s| {
            let in_sel = matches!(gs.bounds(), Some((r0, _, r1, _)) if pos >= r0 && pos <= r1);
            let s = s
                .width(gutter_w())
                .height(row_h())
                .flex_shrink(0.0_f32)
                .items_center()
                .justify_end()
                .padding_horiz(theme::scaled(8.0))
                .border_right(1.0)
                .border_color(theme::border());
            if in_sel {
                s.background(theme::accent().multiply_alpha(0.12))
            } else {
                s.background(theme::bg_header_row())
            }
        })
}

/// The **real** data-row indices the selection covers, in display order, or just
/// `pos`'s when the selection doesn't include it (a menu must act on what the
/// click pointed at).
///
/// Pending new rows are left out: they have no committed row to duplicate or
/// mark for deletion, and the row actions are offered only for real ones.
/// The decision is `edit::selected_data_rows` — it decides which rows a delete
/// acts on, and the write-back's 1-row net checks the count rather than the
/// identity, so it belongs where it can be tested.
fn selected_data_rows(gs: GridState, pos: usize) -> Vec<usize> {
    let order = gs.order.get_untracked();
    let selection = gs.bounds_untracked().map(|(r0, _, r1, _)| (r0, r1));
    schemaic_core::edit::selected_data_rows(&order, selection, pos)
}

/// Mark (or unmark) every row in `idxs` for deletion, touching only the ones
/// that would change — `toggle_delete` on a mixed selection would flip half of
/// it the wrong way.
fn set_rows_deleted(gs: GridState, idxs: &[usize], deleted: bool) {
    for di in idxs {
        let is = gs.del_rows.with_untracked(|d| d.contains(di));
        if is != deleted {
            gs.toggle_delete(*di);
        }
    }
}

/// The gutter's right-click menu: what can be done to **rows** as such.
///
/// Deliberately not the cell menu. That one is built around one cell's value —
/// Edit Field, Copy, Filter by this value — none of which a row-number click has
/// picked out, and offering them would answer a gesture about rows with actions
/// about a column.
fn gutter_menu(gs: GridState, pos: usize, pending: Option<usize>) -> Vec<MenuEntry> {
    let mut entries = vec![MenuEntry::action("Copy", move || copy_selection(gs))];
    // Row actions, on the same terms the cell menu offers them: real (already
    // committed) rows of a single writable table.
    let model = gs.edit_model.get_untracked();
    if pending.is_none() && model.insert_target().is_some() {
        // **Every selected row, not just the one clicked.** The gesture that
        // opened this menu selected rows, and the attach entry below already
        // counts them — narrowing to one here would have the same menu describe
        // five rows in one line and quietly act on one in the next.
        let idxs = selected_data_rows(gs, pos);
        let n = idxs.len();
        let all_deleted = gs
            .del_rows
            .with_untracked(|d| idxs.iter().all(|i| d.contains(i)));
        let plural = |verb: &str| {
            if n == 1 {
                format!("{verb} row")
            } else {
                format!("{verb} {n} rows")
            }
        };
        entries.push(MenuEntry::Separator);
        let dup = idxs.clone();
        entries.push(MenuEntry::action(plural("Duplicate"), move || {
            clone_rows(gs, &dup);
        }));
        let del = idxs;
        entries.push(MenuEntry::action(
            if all_deleted {
                "Undo delete".to_string()
            } else {
                plural("Delete")
            },
            move || set_rows_deleted(gs, &del, !all_deleted),
        ));
    }
    if ai_data_of(gs).may_attach() {
        entries.push(MenuEntry::Separator);
        entries.push(MenuEntry::action_icon(
            format!("Attach {} to chat", selection_scope_label(gs)),
            (icons::SPARKLES, theme::key_foreign),
            move || attach_to_chat(gs, false),
        ));
    }
    entries
}

/// Zebra-stripe an odd display row (shared by the frozen and data panes so both
/// panes of the same row stripe identically).
fn zebra_bg(s: floem::style::Style, pos: usize) -> floem::style::Style {
    if pos % 2 == 1 {
        s.background(theme::bg_editor())
    } else {
        s
    }
}

/// Frozen-pane row: the gutter + (optionally) the frozen column.
fn frozen_row(
    gs: GridState,
    pos: usize,
    data_idx: usize,
    frozen_col: Option<usize>,
    ncols: usize,
    pending: Option<usize>,
) -> impl IntoView {
    let mut children: Vec<AnyView> = vec![gutter_cell(gs, pos, ncols, pending).into_any()];
    if let Some(fc) = frozen_col {
        children.push(cell_at(gs, pos, data_idx, fc, pending).into_any());
    }
    h_stack_from_iter(children).style(move |s| {
        zebra_bg(
            s.flex_row()
                .height(row_h())
                .items_center()
                .flex_shrink(0.0_f32),
            pos,
        )
    })
}

/// Data-pane row: cells for `cols` (every column except the frozen one, in order).
fn data_row(
    gs: GridState,
    pos: usize,
    data_idx: usize,
    cols: Arc<Vec<usize>>,
    pending: Option<usize>,
    win: Memo<ColWindow>,
) -> impl IntoView {
    // Column-virtualized: leading spacer + only the visible window's cells +
    // trailing spacer. Keyed on `win`, so a horizontal-scroll boundary crossing
    // rebuilds the visible rows' cells; during vertical scroll `win` is stable, so
    // a freshly created row builds only the ~10-14 on-screen cells (the fling win).
    dyn_container(
        move || win.get(),
        move |w| {
            let mut kids: Vec<AnyView> = vec![col_spacer(w.left_pad, row_h()).into_any()];
            for k in w.start..w.end {
                kids.push(cell_at(gs, pos, data_idx, cols[k], pending).into_any());
            }
            kids.push(col_spacer(w.right_pad, row_h()).into_any());
            h_stack_from_iter(kids)
                .style(move |s| zebra_bg(s.flex_row().height(row_h()).items_center(), pos))
                .into_any()
        },
    )
    .style(|s| s.height(row_h()))
}

/// Apply a display formatter to column `ci`: update the live per-column state (so
/// cells re-render) and, when the source table is known, upsert + persist the rule
/// so it survives restarts.
fn set_format(gs: GridState, ci: usize, fmt: ColumnFormat) {
    gs.formats.update(|v| {
        if ci < v.len() {
            v[ci] = fmt;
        }
    });
    // Saved against the column's *own* table, which is what `GridState::new` then
    // looks it up by — so the rule is found again wherever that column appears,
    // and never applied to a same-named column of another table. An expression
    // column belongs to no table: it formats for this result and is not persisted.
    if let Some((db, table, col)) = format_key(&gs.rs.get_untracked(), ci) {
        let conn = gs.conn_id.get_untracked();
        gs.fmt_rules
            .update(|rules| format::upsert(rules, conn, &db, &table, &col, fmt));
        if let Some(save) = gs.save_formats.get_untracked() {
            (save)();
        }
    }
}

/// The "Format as" submenu entries for a column header (current choice checked).
fn format_submenu(gs: GridState, ci: usize) -> Vec<MenuEntry> {
    let cur = gs
        .formats
        .with_untracked(|f| f.get(ci).copied().unwrap_or(ColumnFormat::None));
    ColumnFormat::MENU
        .iter()
        .map(|&fmt| {
            if fmt == cur {
                // Selected: tint the label (no checkmark).
                MenuEntry::action_colored(fmt.label(), theme::chip_active, move || {
                    set_format(gs, ci, fmt)
                })
            } else {
                MenuEntry::action(fmt.label(), move || set_format(gs, ci, fmt))
            }
        })
        .collect()
}

/// Clickable, two-line header cell (name + SQL type). Sorts on click, shows a
/// chevron for the active sort, a key icon for PK/index/FK columns, a selected-
/// column background, and carries a right-edge resize divider.
fn header_cell(
    gs: GridState,
    ci: usize,
    sort_val: SortState,
    sort: RwSignal<SortState>,
    key_map: Arc<HashMap<usize, ColKey>>,
) -> impl IntoView {
    let rs = gs.rs.get_untracked();
    let col = rs.columns.get(ci);
    let name = col.map(|c| c.name.clone()).unwrap_or_default();
    let type_name = col.map(|c| c.type_name.clone()).unwrap_or_default();
    let numeric = col.map(|c| c.is_numeric()).unwrap_or(false);
    // Sort indicator: for filter/sort-eligible results the order lives in the
    // server-side `grid_query` (real column name); otherwise it's the client sort.
    // The two are mutually exclusive (eligible results never client-sort).
    let server_dir = gs.server_sort_dir(ci);
    let sorted = server_dir.is_some() || matches!(sort_val, Some((c, _)) if c == ci);
    let asc = match server_dir {
        Some(a) => a,
        None => matches!(sort_val, Some((c, true)) if c == ci),
    };
    let key = key_map.get(&ci).copied();

    // Name + (when sorted) a chevron 7px to its right, both in the sort colour.
    let name_line = text(name).style(move |s| {
        let s = s.font_size(theme::font_label()).font_bold();
        if sorted {
            s.color(theme::chip_active())
        } else {
            s.color(theme::text_dim())
        }
    });
    // A 14px-tall trailing slot in both states so the sorted chevron doesn't grow
    // the row (which would nudge the type line down). The unsorted slot is
    // zero-width, so it adds no horizontal gap.
    let trailing = if sorted {
        let chev = if asc {
            icons::CHEVRON_UP
        } else {
            icons::CHEVRON_DOWN
        };
        icons::icon(chev, 14.0)
            .style(|s| {
                s.color(theme::chip_active())
                    .margin_left(theme::scaled(7.0))
                    .flex_shrink(0.0_f32)
            })
            .into_any()
    } else {
        empty()
            .style(|s| {
                s.height(theme::scaled(14.0))
                    .width(0.0)
                    .flex_shrink(0.0_f32)
            })
            .into_any()
    };
    let name_row = h_stack((name_line, trailing)).style(|s| s.items_center());
    // SQL type, nudged 2px lower for a touch more breathing room under the name.
    let type_line = text(type_name).style(|s| {
        s.font_size(theme::scaled_font(11.0))
            .color(theme::text_faint())
            .margin_top(theme::scaled(2.0))
    });
    let label = v_stack((name_row, type_line)).style(move |s| {
        let s = s
            .flex_col()
            .justify_center()
            .gap(theme::scaled(1.0))
            .min_width(0.0)
            .height_full();
        if numeric && key.is_none() {
            s.items_end()
        } else {
            s.items_start()
        }
    });

    // Optional key icon at the left (8px from the edge, 8px from the label).
    let content = if let Some(k) = key {
        h_stack((
            icons::icon(k.svg(), 14.0).style(move |s| s.color(k.color()).flex_shrink(0.0_f32)),
            label,
        ))
        .style(|s| {
            s.flex_row()
                .items_center()
                .height_full()
                .width_full()
                .gap(theme::scaled(8.0))
                .padding_left(theme::scaled(8.0))
                .padding_right(theme::scaled(10.0))
        })
        .into_any()
    } else {
        container(label)
            .style(move |s| {
                let s = s.height_full().width_full().items_center();
                if numeric {
                    // Right-aligned: extra right padding so the value doesn't hug the
                    // edge/border. Kept in sync with `data_cell` so the header lines
                    // up over its column's values.
                    s.padding_left(theme::scaled(10.0))
                        .padding_right(grid_num_pad_right())
                        .justify_end()
                } else {
                    s.padding_horiz(theme::scaled(10.0)).justify_start()
                }
            })
            .into_any()
    };

    stack((content, col_resize_handle(gs, ci, key.is_some())))
        .on_click_stop(move |_| {
            gs.dismiss_overlays();
            // Eligible results sort server-side (full-table ORDER BY re-run); others
            // fall back to today's in-memory sort of the loaded page.
            if gs.filterable() {
                gs.cycle_server_sort(ci);
            } else {
                cycle_sort(sort, ci);
            }
        })
        // Right-click → Freeze this column (pin left) · Copy its values.
        .on_secondary_click_stop(move |_| {
            gs.dismiss_overlays();
            let freeze_item = if gs.frozen.get_untracked() == Some(ci) {
                MenuEntry::action("Unfreeze", move || gs.frozen.set(None))
            } else {
                MenuEntry::action("Freeze", move || gs.frozen.set(Some(ci)))
            };
            // "AI Summary" for the whole column: what is this field *for*? The
            // prompt carries a sample of the loaded values, which usually settles
            // it where the name alone wouldn't. Sampled from what's on screen —
            // no query, so the menu stays instant.
            let sum = gs.summarize.get_untracked();
            let rs = gs.rs.get_untracked();
            let (column, type_name) = rs
                .columns
                .get(ci)
                .map(|c| (c.name.clone(), c.type_name.clone()))
                .unwrap_or_default();
            let msg = summary::column_prompt(
                source_table(gs).as_deref(),
                &column,
                &type_name,
                &summary::sample_column(&rs, ci, summary::COLUMN_SAMPLE),
            );
            gs.popup_anchor.set(None); // right-click → open at the cursor
            let mut entries = vec![
                freeze_item,
                MenuEntry::sub("Format as", format_submenu(gs, ci)),
                MenuEntry::Separator,
                MenuEntry::sub(
                    "Copy",
                    vec![
                        MenuEntry::action("CSV", move || {
                            let _ = floem::Clipboard::set_contents(export_column_csv(gs, ci));
                        }),
                        MenuEntry::action("JSON", move || {
                            let _ = floem::Clipboard::set_contents(export_column_json(gs, ci));
                        }),
                    ],
                ),
            ];
            // The column summary's prompt carries a sample of the values, so it
            // is a data path like the cell one and is absent on a schema-only
            // connection. (Copy is not: the clipboard is the user's own machine.)
            if ai_data_of(gs).may_attach() {
                entries.push(MenuEntry::Separator);
                entries.push(MenuEntry::action_icon(
                    "AI summary",
                    (icons::SPARKLES, theme::key_foreign),
                    move || {
                        // Asked again at the launch, not only at the build.
                        // `msg` was captured with real cell values in it, and a
                        // menu can outlive the level that permitted it — the
                        // connection's data access is changed from a settings
                        // panel that does not close an open menu.
                        if !ai_data_of(gs).may_attach() {
                            return;
                        }
                        if let Some(s) = &sum {
                            (s)(msg.clone());
                        }
                    },
                ));
            }
            gs.popup.set(Some(entries));
        })
        .style(move |s| {
            // `with`, not `get`: `get` clones the whole widths `Vec` to read one
            // slot, and this closure re-runs for every visible header on any
            // selection change.
            let w = gs.widths.with(|ws| ws.get(ci).copied().unwrap_or(cell_w()));
            // Highlight the header when its column is within the cell selection.
            let col_sel = matches!(gs.bounds(), Some((_, c0, _, c1)) if ci >= c0 && ci <= c1);
            let formatted = gs
                .formats
                .with(|f| f.get(ci).map(|x| *x != ColumnFormat::None).unwrap_or(false));
            let s = s.width(w).height(grid_header_h()).flex_shrink(0.0_f32);
            let s = if col_sel {
                s.background(theme::grid_col_sel())
            } else if formatted {
                // At-a-glance cue this column shows a formatted (not raw) value.
                s.background(theme::dropdown_active())
                    .hover(|s| s.background(theme::accent().multiply_alpha(0.10)))
            } else {
                // Opaque header background (not transparent over the header row) so
                // a live resize where header cells briefly overlap occludes cleanly.
                s.background(theme::bg_header_row())
                    .hover(|s| s.background(theme::accent().multiply_alpha(0.10)))
            };
            // Border on every column, last included, so a narrow table still shows
            // where the final column ends.
            s.border_right(1.0).border_color(theme::border())
        })
}

/// Stage the inline editor's buffer into the cell it is open on and close it —
/// **Enter's contract**, called by the type-aware controls, which have no text to
/// type and so commit on a choice instead.
///
/// In a pending new row Enter hops to the next editable cell (fast data entry),
/// exactly as it does from the text input; in a real row it just closes.
fn keep_cell_edit(gs: GridState, i: usize, data_idx: usize, ci: usize, pending: Option<usize>) {
    // **Belt to the picker's `on_cleanup` brace**, and the same guard
    // `drop_cell_edit` opens with two functions below. Every caller is a choice
    // made in a menu on the *window-global* popup channel, which can still be on
    // screen after this grid's scope is gone; `gs.stage` reads `edit_buf` and
    // `rs` with `get_untracked`, i.e. `try_get_untracked().unwrap()`, so a stray
    // click would panic rather than no-op.
    if !gs.alive() {
        return;
    }
    if pending.is_some() {
        advance_edit(gs, i, ci, pending, true);
    } else {
        gs.stage(data_idx, ci, Some(gs.edit_buf.get_untracked()));
        gs.edit_cell.set(None);
        refocus_grid(gs);
    }
}

/// Close the inline editor **without** staging (Escape, focus lost) — but only if
/// this cell is still the open one: a Tab/Enter hop has already repointed
/// `edit_cell` at the next cell, and this cell's own teardown must not clobber it.
fn drop_cell_edit(gs: GridState, i: usize, ci: usize, refocus: bool) {
    // Reached from a focus-loss and from a menu dismissal, both of which can fire
    // while the grid is being torn down — see `GridState::alive`.
    if !gs.alive() || gs.edit_cell.get_untracked() != Some((i, ci)) {
        return;
    }
    gs.edit_cell.set(None);
    if refocus {
        refocus_grid(gs);
    }
}

/// The in-cell picker: the value with a chevron filling the cell, and the shared
/// popup menu over it listing what the column may hold — a boolean's two words,
/// an enum's members, a `SET`'s members.
///
/// **One control for all three**, because a cell has room for a value and a
/// chevron and nothing else, and because a menu is the one list that can be drawn
/// over a grid at all (the popup layer is outside the scroll that would clip it).
/// The menu is opened against the cell's own rect, one tick after the face is
/// built, since a view has no `layout_rect` until it has been laid out.
///
/// Choosing commits straight away (there is nothing else to type), so a `SET`
/// toggles **one** member per opening — the row panel's chips are where a subset
/// is assembled in one go. Dismissing the menu any way at all closes the editor,
/// which is the `popup` effect below: the channel is written by the root's
/// click-away handler too, so watching the flag is the only way to hear about
/// every dismissal.
///
/// Dates are not here: their calendar is a panel, not a list, and the field it
/// drops from stays — see [`cell_calendar_editor`].
fn cell_pick_editor(
    gs: GridState,
    i: usize,
    data_idx: usize,
    ci: usize,
    pending: Option<usize>,
    editor: CellEditor,
) -> AnyView {
    let ch = cell_editors::PopupChannel {
        menus: gs.menus,
        anchor: gs.popup_anchor,
        width: gs.popup_width,
    };
    let face = cell_editors::pick_cell_face(gs.edit_buf, editor.clone());
    let anchor = face.id();
    // Whether *we* put a menu up. Without it the closing effect below fires on
    // its first run — before the deferred open — and shuts the editor instantly.
    let opened = RwSignal::new(false);
    // Where our menu is standing, if it is. A plain `Cell` and not a signal:
    // `on_cleanup` below is the only reader, and reading a signal of the scope
    // being disposed is the hazard the cleanup exists to prevent.
    let standing: Rc<std::cell::Cell<Option<crate::PopupAnchor>>> = Rc::new(Default::default());
    let open = {
        let editor = editor.clone();
        let standing = standing.clone();
        move || {
            // The option's *value*, which for a `SET` is the whole value with that
            // member toggled and for a boolean the engine's own spelling — see
            // `celledit::pick_options`.
            let pick: Rc<dyn Fn(&str)> = Rc::new(move |v: &str| {
                gs.edit_buf.set(v.to_string());
                keep_cell_edit(gs, i, data_idx, ci, pending);
            });
            let entries = cell_editors::pick_entries(&editor, &gs.edit_buf.get_untracked(), pick);
            let width = gs
                .widths
                .with_untracked(|w| w.get(ci).copied().unwrap_or(cell_w()));
            standing.set(cell_editors::open_picker(ch, Some(anchor), width, entries));
        }
    };
    // One tick later: `layout_rect` is what the menu anchors to, and this view has
    // none until the frame that built it has been laid out.
    let first = open.clone();
    floem::action::exec_after(std::time::Duration::ZERO, move |_| {
        if !gs.alive() || gs.edit_cell.get_untracked() != Some((i, ci)) {
            return;
        }
        (first)();
        opened.set(true);
    });
    create_effect(move |_| {
        let up = gs.popup.get().is_some();
        if opened.get() && !up {
            drop_cell_edit(gs, i, ci, true);
        }
    });
    face.on_event_stop(
        EventListener::PointerDown,
        crate::widgets::menu_trigger_press,
    )
    // Clicking the face toggles the menu it opened — `open_picker` recognises
    // its own menu and closes it rather than reopening.
    .on_click_stop(move |_| (open)())
    // **The menu cannot outlive the cell that opened it** — the same rule
    // `cell_calendar_editor` states two functions below, and the same hazard: its
    // entries are `Rc` closures over this grid's signals, and the only thing that
    // clears the shared channel is a pointer-down. Ctrl+Tab or Ctrl+Enter
    // disposed this scope with the list still on screen, and clicking a row then
    // ran `keep_cell_edit` → `get_untracked` on a freed signal, which panics and
    // takes every tab's uncommitted edits with it.
    .on_cleanup(move || cell_editors::close_picker(ch, standing.get()))
    .into_any()
}

/// Is the **cell editor's own** calendar up?
///
/// One channel serves every date control in the app and it carries no tag saying
/// who filled it, so the buffer is the identity ([`DatePick`]) — and the buffer a
/// cell editor binds to is always `edit_buf`, which no row-panel field ever is.
fn cell_calendar_up(gs: GridState) -> bool {
    gs.menus
        .date_pick
        .with_untracked(|p| p.as_ref().is_some_and(|d| d.buf == gs.edit_buf))
}

/// The in-cell date editor: `field` (an ordinary text input over the cell) with
/// the calendar standing over the grid, dropped from the cell's own rect.
///
/// **The panel, not the keyboard, owns this editor's lifetime.** Every click
/// inside the calendar costs the field its focus (see the `FocusLost` guard in
/// [`data_cell`]), so the usual "focus left → close, discarding" rule cannot
/// apply while it is up. What replaces it is the channel: choosing a day stages
/// the edit and closes ([`keep_cell_edit`], the same commit an in-cell picker
/// makes — a cell has no Save button in reach), and *any* other way the panel
/// goes away closes the editor without staging, which is what the effect below
/// watches for.
///
/// **Which press cost the field its focus is the panel's to say**
/// ([`cell_editors::take_calendar_press`]), and asking that rather than "is a
/// panel open" is what keeps Escape working: floem's `text_input` answers Escape
/// by dropping the window focus and reporting the key handled, so this editor
/// hears about it as a `FocusLost` and nothing else hears about it at all.
/// Standing down for the whole time a panel was up therefore ate Escape — no
/// editor closed, no panel closed, and a grid with no keyboard.
///
/// The field keeps working throughout — it still has the value, Enter still
/// stages it and Tab still hops — but only because the guard **hands the caret
/// back** after the press it stood down for. Standing down alone left the field
/// mounted and deaf, which is the state a `DATETIME`'s time of day cannot be
/// typed in, and typing is half the reason the field is there beside the panel.
fn cell_calendar_editor(
    gs: GridState,
    i: usize,
    data_idx: usize,
    ci: usize,
    pending: Option<usize>,
    editor: CellEditor,
    field: AnyView,
) -> AnyView {
    // The cell's own rect is the anchor, so the panel drops from the cell rather
    // than from wherever the pointer was — the field fills it.
    let anchor = field.id();
    // Whether *we* put a panel up. Without it the closing effect below fires on
    // its first run — before the deferred open — and shuts the editor instantly.
    let opened = RwSignal::new(false);
    // One tick later, for `cell_pick_editor`'s reason: `layout_rect` is what the
    // panel anchors to, and this view has none until the frame that built it has
    // been laid out.
    floem::action::exec_after(std::time::Duration::ZERO, move |_| {
        if !gs.alive() || gs.edit_cell.get_untracked() != Some((i, ci)) {
            return;
        }
        let on_pick: Rc<dyn Fn()> = Rc::new(move || keep_cell_edit(gs, i, data_idx, ci, pending));
        cell_editors::open_calendar(gs.menus, Some(anchor), gs.edit_buf, &editor, Some(on_pick));
        // Asked, not assumed: a cell with no rect yet opens nothing, and an
        // `opened` set anyway is an effect that closes the editor on sight.
        opened.set(cell_calendar_up(gs));
    });
    create_effect(move |_| {
        // Subscribed to the channel, then asked the one predicate that knows
        // *whose* panel is up — the alternative was a second copy of the identity
        // test, tracked, drifting from the two untracked callers.
        gs.menus.date_pick.track();
        if opened.get() && !cell_calendar_up(gs) {
            drop_cell_edit(gs, i, ci, true);
        }
    });
    field
        // A press in the panel that cost the field nothing (it had no focus to
        // lose) leaves its flag standing, and the `FocusLost` guard would spend it
        // on the next thing that *is* a real focus loss. Getting the caret back is
        // the moment that can't be true any more, so it is where the flag is
        // dropped.
        .on_event_cont(EventListener::FocusGained, |_| {
            cell_editors::take_calendar_press();
        })
        // **The panel cannot outlive the cell that opened it.** It edits
        // `edit_buf`, which the next cell to be edited takes over, so a panel left
        // standing would write into a different cell than the one it dropped from.
        // A commit clears it on the way past (the pick closes the panel itself),
        // but a switched tab, a re-fetch and a scrolled-away row do not.
        .on_cleanup(move || {
            if cell_calendar_up(gs) {
                gs.menus.date_pick.set(None);
            }
        })
        .into_any()
}

fn data_cell(
    gs: GridState,
    i: usize,
    data_idx: usize,
    ci: usize,
    numeric: bool,
    pending: Option<usize>,
) -> impl IntoView {
    let dkey = (data_idx, ci);
    // For a pending new row, an unset editable cell shows a faint placeholder for
    // what happens if left blank: `<auto>` (auto-increment), `<required>` (NOT NULL
    // with no default — must be filled or the INSERT errors), `<null>` (nullable →
    // inserts NULL), else `<default>` (NOT NULL with an explicit default).
    let (auto_inc, no_default, not_null) = gs
        .rs
        .get_untracked()
        .columns
        .get(ci)
        .and_then(|c| c.origin.as_ref())
        .map(|o| (o.flags.auto_increment, o.flags.no_default, o.flags.not_null))
        .unwrap_or((false, false, false));
    let col_editable = gs.edit_model.get_untracked().editable(ci);
    // This cell's column is a foreign key with a resolvable target (real rows
    // only) → offer "Follow" (menu + Ctrl-click) and underline the value.
    let follow_spec: Option<Rc<FollowSpec>> = if pending.is_none() {
        gs.follow.get_untracked().get(&ci).cloned()
    } else {
        None
    };
    let is_fk = follow_spec.is_some();
    // Content: an inline editor when this cell is open for editing, otherwise the
    // (possibly edited) value. The original value is read from `gs.rs` here so a
    // post-commit splice (which updates `gs.rs`) refreshes the cell in place;
    // dirty values show the pending edit.
    let content = dyn_container(
        move || {
            // `None` = not staged; `Some(None)` = staged NULL; `Some(Some(t))` = text.
            // A pending new row reads from `new_rows` (no original); real rows read
            // the staged edit from `dirty` and the original from `rs`.
            let fmt = gs
                .formats
                .with(|f| f.get(ci).copied().unwrap_or(ColumnFormat::None));
            let (staged, orig, orig_null): (Option<Option<String>>, String, bool) = match pending {
                Some(p) => {
                    let staged = gs
                        .new_rows
                        .with(|rows| rows.get(p).and_then(|r| r.get(&ci).cloned()));
                    (staged, String::new(), false)
                }
                None => {
                    let staged = gs.dirty.with(|d| d.get(&dkey).cloned());
                    let (orig, orig_null) = gs.rs.with(|rs| match rs.cell(data_idx, ci) {
                        Some(c) => (format::apply(fmt, &c.to_value()), c.is_null()),
                        None => (String::new(), true),
                    });
                    (staged, orig, orig_null)
                }
            };
            (gs.edit_cell.get() == Some((i, ci)), staged, orig, orig_null)
        },
        {
            move |(is_editing, staged, orig, is_null): (
                bool,
                Option<Option<String>>,
                String,
                bool,
            )| {
                if is_editing {
                    // A column whose values are already written down edits with
                    // its own control rather than a text field. A picker replaces
                    // the field outright; a date keeps it (typing a date is often
                    // faster, and a `DATETIME`'s time of day has no calendar to
                    // come from) and drops the calendar over the grid beside it.
                    let shape = open_cell_shape(gs, ci);
                    if let CellShape::Pick(e) = shape {
                        return cell_pick_editor(gs, i, data_idx, ci, pending, e);
                    }
                    // The field's own id, so the `FocusLost` guard below can hand
                    // the caret back to it. Filled once the view exists — the
                    // handler only ever reads it at event time.
                    let field_id: RwSignal<Option<floem::ViewId>> = RwSignal::new(None);
                    let field = floem::views::text_input(gs.edit_buf)
                        .on_event(EventListener::KeyDown, move |e| {
                            if let Event::KeyDown(ke) = e {
                                match &ke.key.logical_key {
                                    Key::Named(NamedKey::Enter) => {
                                        // Stage the current cell. In a pending new row
                                        // Enter hops to the next editable cell (fast
                                        // data entry); in a real row it just closes.
                                        if pending.is_some() {
                                            advance_edit(gs, i, ci, pending, true);
                                        } else {
                                            gs.stage(
                                                data_idx,
                                                ci,
                                                Some(gs.edit_buf.get_untracked()),
                                            );
                                            gs.edit_cell.set(None);
                                            refocus_grid(gs);
                                        }
                                        return EventPropagation::Stop;
                                    }
                                    Key::Named(NamedKey::Tab) => {
                                        // Tab / Shift+Tab hop to the next / previous
                                        // editable cell (staging the current one).
                                        // Intercepted so it doesn't move window focus.
                                        advance_edit(gs, i, ci, pending, !ke.modifiers.shift());
                                        return EventPropagation::Stop;
                                    }
                                    // **Escape is not here**, and cannot be:
                                    // floem's `text_input` handles it in
                                    // `event_before_children` (clearing the
                                    // window focus) and reports it processed, so
                                    // no listener of ours runs. What that leaves
                                    // behind is picked up by `FocusLost` below.
                                    _ => {}
                                }
                            }
                            EventPropagation::Continue
                        })
                        // Losing focus (Esc, clicking elsewhere, etc.) discards —
                        // only Enter keeps the value. Guard: close only if THIS cell
                        // is still the open editor — a Tab/Enter hop has already
                        // repointed `edit_cell` to the next cell, and this input's
                        // focus-loss must not clobber that.
                        .on_event(EventListener::FocusLost, move |_| {
                            // **A press inside this cell's own calendar is not
                            // the user leaving.** Floem takes the window focus on
                            // *every* pointer-down and hands it back only to a
                            // focusable view under the cursor — and a day, a month
                            // arrow and the Now button are none of them. So the
                            // first click in the panel arrived here as a focus
                            // loss, closed the editor, and the pick it was about
                            // to make landed on a cell that was no longer being
                            // edited. What closes the editor in that case is the
                            // panel itself (`cell_calendar_editor`).
                            //
                            // **The press, not the panel, is the question.**
                            // Standing down whenever a panel was merely *open*
                            // meant Escape — which reaches this handler and
                            // nothing else, `text_input` having answered it by
                            // dropping the window focus — closed neither the
                            // editor nor the panel, and left the grid with no
                            // keyboard and no way back but the mouse.
                            if cell_calendar_up(gs) && cell_editors::take_calendar_press() {
                                // **And hand the caret straight back**, or the
                                // field survives the click without the keyboard:
                                // paging a month would leave a `DATETIME`'s time
                                // of day untypable and Enter/Tab going to the
                                // window root, which is the one thing keeping the
                                // field beside the panel was for. Floem gives the
                                // focus back only to a focusable view under the
                                // cursor, and a month arrow is not one — so it has
                                // to be asked for here. (The caret lands at the end
                                // of the value: `text_input` drops its selection
                                // and moves the cursor there on regaining focus.)
                                if let Some(id) = field_id.get_untracked() {
                                    id.request_focus();
                                }
                                return EventPropagation::Continue;
                            }
                            if gs.edit_cell.get_untracked() == Some((i, ci)) {
                                gs.edit_cell.set(None);
                                // Escape came through here rather than through a
                                // key handler, and took the keyboard with it —
                                // see `reclaim_keyboard` for why the pointer is
                                // what decides whether to take it back.
                                let over = gs
                                    .focus_id
                                    .get_untracked()
                                    .map(|f| f.layout_rect())
                                    .is_some_and(|r| {
                                        reclaim_keyboard(gs.last_mouse.get_untracked(), r)
                                    });
                                if over {
                                    refocus_grid(gs);
                                }
                            }
                            EventPropagation::Continue
                        })
                        .request_focus(|| {})
                        // Fill the whole cell (its own `dyn_container` is set to
                        // fill while editing) with no field chrome, so it reads as
                        // editing the cell in place rather than a nested input.
                        // The global `TextInputClass` paints inputs `bg_deepest`
                        // in every state (incl. `:focus`, which is always on while
                        // editing), so we must clear the background per-state too.
                        .style(move |s| {
                            let clear = floem::peniko::Color::TRANSPARENT;
                            let s = s
                                .width_full()
                                .height_full()
                                .items_center()
                                .font_size(theme::font_body())
                                .color(theme::text())
                                .background(clear)
                                .border(0.0)
                                .border_radius(0.0)
                                .padding(0.0)
                                .hover(|s| s.background(clear).border(0.0))
                                .active(|s| s.background(clear).border(0.0))
                                .focus(|s| {
                                    s.background(clear)
                                        .border(0.0)
                                        .hover(|s| s.background(clear))
                                });
                            if numeric {
                                // Right-align the editor to match the right-aligned
                                // numeric display, so entering edit mode doesn't jump
                                // the value to the left. Floem's text_input has no
                                // text-align, so pad the left by the free space — the
                                // buffer's *measured* width (re-runs as the buffer
                                // changes, keeping it right-anchored while typing). A
                                // value wider than the column clamps to `pad_left = 0`
                                // (full width, left-aligned + clip) like the display.
                                let w =
                                    gs.widths.with(|ws| ws.get(ci).copied().unwrap_or(cell_w()));
                                let text_px = gs.edit_buf.with(|b| measure_text_px(b));
                                s.padding_left(numeric_edit_pad_left(w, text_px))
                            } else {
                                s
                            }
                        })
                        .into_any();
                    field_id.set(Some(field.id()));
                    return match shape {
                        CellShape::Calendar(e) => {
                            cell_calendar_editor(gs, i, data_idx, ci, pending, e, field)
                        }
                        _ => field,
                    };
                }
                let edited = staged.is_some();
                // A pending new row's unset editable cell shows a placeholder for
                // what it'll do if left blank. `<required>` (NOT NULL, no default)
                // is tinted with the error colour — leaving it blank fails the
                // INSERT; `<auto>` / `<default>` are faint (the server fills them).
                let placeholder = !edited && pending.is_some() && (col_editable || auto_inc);
                // How the text is *weighted* — and the reason it is a decision
                // rather than a chain of `if`s in the closure below. See
                // [`cell_ink`].
                let ink = cell_ink(staged.as_ref(), is_null, placeholder, is_fk);
                let src = match &staged {
                    Some(Some(t)) => t.clone(),       // staged text
                    Some(None) => "NULL".to_string(), // staged SQL NULL
                    None if placeholder => {
                        if auto_inc {
                            "<auto>".to_string()
                        } else if no_default {
                            "<required>".to_string()
                        } else if !not_null {
                            "<null>".to_string()
                        } else {
                            "<default>".to_string()
                        }
                    }
                    None => orig.clone(), // original (live from `rs`)
                };
                // Preview only: flatten newlines/tabs to spaces so a multiline
                // value stays a single grid row (the viewer shows it verbatim).
                let src = src.replace(['\r', '\n', '\t'], " ");
                let shown = truncate(&src, 200);
                text(shown)
                    .style(move |s| {
                        let s = s.font_size(theme::font_body());
                        match ink {
                            // Staged edit: white text over the green cell fill.
                            CellInk::Staged => s.color(floem::peniko::Color::WHITE),
                            // A staged **SQL NULL**, in the same italic the grid
                            // has always used for "there is no value here" — the
                            // one thing that tells it apart from a staged
                            // four-character string reading `NULL`.
                            CellInk::StagedNull => s
                                .color(floem::peniko::Color::WHITE)
                                .font_style(floem::text::Style::Italic),
                            // NULL originals + all pending-row placeholders
                            // (`<auto>`/`<required>`/`<null>`/`<default>`) render faint.
                            CellInk::Absent => s
                                .color(theme::text_faint())
                                .font_style(floem::text::Style::Italic),
                            // Foreign-key value: underline it (in the text colour) as
                            // a "followable relation" affordance (Ctrl-click follows).
                            CellInk::Fk => s
                                .color(theme::text())
                                .border_bottom(1.0)
                                .border_color(theme::text()),
                            CellInk::Plain => s.color(theme::text()),
                        }
                    })
                    .into_any()
            }
        },
    )
    // While editing this cell, fill it so the in-place editor's `width_full`/
    // `height_full` resolves against a definite box (otherwise it collapses to
    // ~0). Non-editing cells stay content-sized so numeric right-align works.
    .style(move |s| {
        if gs.edit_cell.get() == Some((i, ci)) {
            s.width_full().height_full().items_center()
        } else {
            s
        }
    });
    let fs_click = follow_spec.clone();
    let fs_menu = follow_spec; // moved into the right-click menu closure below
    container(content)
        .on_event(EventListener::PointerDown, move |e| {
            if let Event::PointerDown(pe) = e {
                // Any click in a cell dismisses an open menu + the commit-error bar
                // (the pointer-down is consumed here, so the root dismissal handler
                // never sees it).
                gs.dismiss_overlays();
                if pe.button.is_primary() {
                    // Ctrl/Cmd+click a foreign-key cell follows the relation (a
                    // shortcut for the menu's "Follow relation"). Skipped while this
                    // cell is being edited — then the click just edits the text.
                    let ctrl = pe.modifiers.control() || pe.modifiers.meta();
                    let editing_here = gs.edit_cell.get_untracked() == Some((i, ci));
                    if ctrl
                        && !editing_here
                        && let Some(spec) = &fs_click
                    {
                        set_active(gs, i, ci, false);
                        follow_relation(gs, data_idx, spec);
                        return EventPropagation::Stop;
                    }
                    // Shift extends the range from the existing anchor; a plain
                    // click starts a new one and arms drag-select, which the
                    // cells' `PointerEnter` continues until the button is
                    // released (the release is caught at the body level, since
                    // the pointer may well be outside this cell by then).
                    set_active(gs, i, ci, pe.modifiers.shift());
                    gs.selecting.set(true);
                    if let Some(fid) = gs.focus_id.get_untracked() {
                        fid.request_focus();
                    }
                    return EventPropagation::Stop;
                }
            }
            EventPropagation::Continue
        })
        // Drag-select: while the button is down, entering a cell moves the active
        // end and leaves the anchor where the drag started. No pointer capture —
        // the flag is enough, and the body's `PointerUp` ends it wherever the
        // pointer happens to be.
        .on_event_cont(EventListener::PointerEnter, move |_| {
            if gs.selecting.get_untracked() {
                set_active(gs, i, ci, true);
            }
        })
        .on_double_click_stop(move |_| {
            // `DoubleClick` consumes the second `PointerUp`, so the drag flags
            // have to be cleared here too or one stays armed with no button held
            // and the next hover drags a selection out of nowhere.
            gs.selecting.set(false);
            gs.row_selecting.set(false);
            // Double-click edits an editable cell; on a read-only cell it does
            // nothing (viewing is via the right-click menu's View item).
            if gs.edit_model.get_untracked().editable(ci) {
                start_edit(gs, i, ci);
            } else {
                gs.active.set(Some((i, ci)));
                gs.anchor.set(Some((i, ci)));
            }
        })
        // Right-click → View · Edit · Copy · Set to NULL · AI summary.
        .on_secondary_click_stop(move |_| {
            // A right-click **inside** the selection keeps it. It used to
            // collapse to the clicked cell unconditionally, which made the menu
            // unable to act on a block: selecting 3×3 and reaching for the menu
            // destroyed the very thing the entry was about, and the entry then
            // offered one cell. Outside the selection it still starts a new one,
            // since that click means "this cell".
            if !cell_in(gs.bounds_untracked(), i, ci) {
                gs.active.set(Some((i, ci)));
                gs.anchor.set(Some((i, ci)));
            }
            let rs = gs.rs.get_untracked();
            // Effective value: staged text/NULL, else the original (real rows only —
            // a pending new row has no original, so unset cells are empty).
            let staged_here_val: Option<Option<String>> = match pending {
                Some(p) => gs
                    .new_rows
                    .with_untracked(|rows| rows.get(p).and_then(|r| r.get(&ci).cloned())),
                None => gs.dirty.with_untracked(|d| d.get(&dkey).cloned()),
            };
            // The painter's own three-way resolution; `edit::GridCells::text` is
            // the same rule for the clipboard and the AI attachment, which read
            // the grid rather than paint it.
            let val = match staged_here_val {
                Some(Some(t)) => t,
                Some(None) => "NULL".to_string(),
                None => match pending {
                    Some(_) => String::new(),
                    None => rs
                        .cell(data_idx, ci)
                        .map(|c| c.display().to_string())
                        .unwrap_or_default(),
                },
            };
            let v_copy = val.clone();
            // "Copy formatted" (only offered when this column has a formatter):
            // the cell's displayed text — the formatted original, or the staged raw
            // value if there's a pending edit (no formatting shown then).
            let fmt = gs
                .formats
                .with_untracked(|f| f.get(ci).copied().unwrap_or(ColumnFormat::None));
            let staged_here = match pending {
                Some(p) => gs.new_rows.with_untracked(|rows| {
                    rows.get(p).map(|r| r.contains_key(&ci)).unwrap_or(false)
                }),
                None => gs.dirty.with_untracked(|d| d.contains_key(&dkey)),
            };
            let formatted_val = if fmt != ColumnFormat::None && !staged_here && pending.is_none() {
                rs.cell(data_idx, ci)
                    .map(|c| format::apply(fmt, &c.to_value()))
                    .unwrap_or_else(|| val.clone())
            } else {
                val.clone()
            };
            let sum = gs.summarize.get_untracked();
            let column = rs
                .columns
                .get(ci)
                .map(|c| c.name.clone())
                .unwrap_or_default();
            let model = gs.edit_model.get_untracked();
            let editable = model.editable(ci);
            // Real row + a single writable table → row-level actions (clone/delete)
            // are available. `deleted` = this real row is already marked for deletion.
            let can_rows = pending.is_none() && model.insert_target().is_some();
            let deleted =
                pending.is_none() && gs.del_rows.with_untracked(|d| d.contains(&data_idx));
            // Nullable = editable + the base column isn't NOT NULL.
            let nullable = editable
                && rs
                    .columns
                    .get(ci)
                    .and_then(|c| c.origin.as_ref())
                    .map(|o| !o.flags.not_null)
                    .unwrap_or(false);
            // Server-side "Filter by / Exclude this value" — real rows of a
            // filter-eligible result whose column maps to a real base-table column.
            // `filter_val` is the cell's raw (unformatted) value, or `None` for NULL
            // (→ `IS NULL` / `IS NOT NULL`).
            let can_filter = pending.is_none() && gs.filterable() && gs.real_col(ci).is_some();
            let filter_val: Option<String> = rs
                .cell(data_idx, ci)
                .and_then(|c| (!c.is_null()).then(|| c.display().to_string()));
            // Context for the AI: the source table (if known), this column's
            // type, the rest of the cell's row, and a sample of the column's
            // other values — all already loaded, so the assistant can answer
            // without a round-trip (and without `run_query`, which the settings
            // may have turned off). A pending new row has no committed row to
            // quote, so it contributes no row context.
            let type_name = rs
                .columns
                .get(ci)
                .map(|c| c.type_name.clone())
                .unwrap_or_default();
            let row_ctx = match pending {
                Some(_) => Vec::new(),
                None => summary::sample_row(&rs, data_idx, ci, summary::CELL_ROW_FIELDS),
            };
            let msg = summary::cell_prompt(
                source_table(gs).as_deref(),
                &column,
                &type_name,
                &val,
                &row_ctx,
                &summary::sample_column(&rs, ci, summary::COLUMN_SAMPLE),
            );

            // "Edit Field" edits this single cell inline; "Edit Row" opens the
            // whole-row structured panel (read-only fields shown for context). A row
            // marked for deletion isn't editable, and a pending new row has no
            // committed row to open in the panel (it's filled via inline cell edits).
            let mut entries: Vec<MenuEntry> = Vec::new();
            if editable && !deleted {
                entries.push(MenuEntry::action("Edit field", move || {
                    start_edit(gs, i, ci)
                }));
            }
            if pending.is_none() {
                entries.push(MenuEntry::action("Edit row", move || {
                    open_edit_row(gs, data_idx)
                }));
            }
            // Right-clicking inside a block keeps it selected, so the entry is
            // about the block — and says which of the two it is, since Ctrl+C
            // and the gutter menu's Copy both mean the whole selection.
            let scope = schemaic_core::edit::copy_scope(gs.bounds_untracked(), i, ci);
            entries.push(MenuEntry::action(scope.label(), move || match scope {
                schemaic_core::edit::CopyScope::Selection => copy_selection(gs),
                schemaic_core::edit::CopyScope::Cell => {
                    let _ = floem::Clipboard::set_contents(v_copy.clone());
                }
            }));
            // Only when this column shows a formatted (non-raw) value.
            if fmt != ColumnFormat::None {
                entries.push(MenuEntry::action("Copy formatted", move || {
                    let _ = floem::Clipboard::set_contents(formatted_val.clone());
                }));
            }
            // Beside Copy, and gated the same way Edit Field is: a paste is a
            // batch of edits, so a result nothing can be typed into has nothing
            // to paste into either. The action still lands on the *selection*,
            // not on this cell — Ctrl+V and this entry do the same thing.
            if editable && !deleted {
                entries.push(MenuEntry::action("Paste", move || paste_selection(gs)));
            }
            // Server-side filter: splice this value into the base query's WHERE and
            // re-run (full table). NULL cells become IS NULL / IS NOT NULL.
            if can_filter {
                entries.push(MenuEntry::Separator);
                let v1 = filter_val.clone();
                entries.push(MenuEntry::action("Filter by this value", move || {
                    gs.add_filter_condition(ci, v1.as_deref(), false);
                }));
                let v2 = filter_val.clone();
                entries.push(MenuEntry::action("Exclude this value", move || {
                    gs.add_filter_condition(ci, v2.as_deref(), true);
                }));
            }
            // "Follow relation" — this cell's column is a foreign key with a known
            // target. Real rows only (a pending new row has no committed key to
            // navigate to). Opens the referenced table filtered to this row's key.
            // (Static label — table names can get long; Ctrl-click is the shortcut.)
            if let Some(spec) = &fs_menu {
                let spec = spec.clone();
                entries.push(MenuEntry::action("Follow relation", move || {
                    follow_relation(gs, data_idx, &spec);
                }));
            }
            if nullable && !deleted {
                entries.push(MenuEntry::action("Set to NULL", move || match pending {
                    Some(p) => gs.stage_new(p, ci, None),
                    None => gs.stage(data_idx, ci, None),
                }));
            }
            // Row actions (single writable table, real rows): duplicate + delete.
            if can_rows {
                entries.push(MenuEntry::Separator);
                entries.push(MenuEntry::action("Duplicate row", move || {
                    clone_row(gs, data_idx);
                }));
                let del_label = if deleted { "Undo delete" } else { "Delete row" };
                entries.push(MenuEntry::action(del_label, move || {
                    gs.toggle_delete(data_idx);
                }));
            }
            // Both of these send *values* — the summary prompt carries this
            // cell, its row and a sample of its column — so both are absent
            // entirely on a connection set to schema-only. Absent rather than
            // disabled: a greyed "AI Summary" invites a hunt for the reason,
            // while the connection form is where the answer lives.
            if ai_data_of(gs).may_attach() {
                // Set off from the row actions above it — asking about a value is a
                // different kind of act from editing or deleting one.
                entries.push(MenuEntry::Separator);
                entries.push(MenuEntry::action_icon(
                    "AI summary",
                    (icons::SPARKLES, theme::key_foreign),
                    move || {
                        // Asked again at the launch, not only at the build.
                        // `msg` was captured with real cell values in it, and a
                        // menu can outlive the level that permitted it — the
                        // connection's data access is changed from a settings
                        // panel that does not close an open menu.
                        if !ai_data_of(gs).may_attach() {
                            return;
                        }
                        if let Some(s) = &sum {
                            (s)(msg.clone());
                        }
                    },
                ));
                // Attaching sends data the user picked, so it is spelled out with
                // the count rather than hidden behind "Ask AI": the label is the
                // consent notice, and it names exactly what is about to travel —
                // a lone cell is one *column*, a gutter selection is whole rows.
                entries.push(MenuEntry::action_icon(
                    format!("Attach {} to chat", selection_scope_label(gs)),
                    (icons::SPARKLES, theme::key_foreign),
                    move || attach_to_chat(gs, false),
                ));
            }
            gs.popup_anchor.set(None); // right-click → open at the cursor
            gs.popup.set(Some(entries));
        })
        .style(move |s| {
            // `with`, not `get` — see the header closure. This one runs for every
            // *cell* in the viewport on every selection change, so a drag-select
            // over a wide result cloned the widths `Vec` hundreds of times per
            // pointer move.
            let w = gs.widths.with(|ws| ws.get(ci).copied().unwrap_or(cell_w()));
            let sel = cell_in(gs.bounds(), i, ci);
            let is_active = gs.active.get() == Some((i, ci));
            let is_dirty = match pending {
                Some(p) => gs
                    .new_rows
                    .with(|rows| rows.get(p).map(|r| r.contains_key(&ci)).unwrap_or(false)),
                None => gs.dirty.with(|d| d.contains_key(&dkey)),
            };
            let is_editing = gs.edit_cell.get() == Some((i, ci));
            // A real row marked for deletion (its edits were cleared when marked).
            let deleted = pending.is_none() && gs.del_rows.with(|d| d.contains(&data_idx));
            // This cell is currently being AI-generated — breathe a purple wash. A
            // real cell (Fill Value) is keyed by `(data_idx, ci)`; a pending row
            // (Insert Row / Seed Table) pulses whole-row.
            let generating = match pending {
                Some(p) => gs.ai_gen_rows.with(|s| s.contains(&p)),
                None => gs.ai_gen.with(|g| g.contains(&(data_idx, ci))),
            };
            let s = s
                .width(w)
                .height(row_h())
                .flex_shrink(0.0_f32)
                .items_center();
            // Right-aligned numeric cells get extra right padding (matching the
            // header) so the value clears the edge/border; text cells stay at 10px.
            //
            // **A picker open in the cell takes the whole cell** ([`cell_fills`]):
            // it carries its own surface, and a padded cell around it leaves a
            // strip of the row showing down each side — a box inside a box, which
            // is what it read as. The control puts the text back on the same inset
            // the display uses (`pick_cell_face`), so nothing moves as the editor
            // opens.
            let s = if is_editing && cell_fills(&open_cell_shape(gs, ci)) {
                s.padding(0.0).justify_start()
            } else if numeric {
                s.padding_left(grid_pad_h())
                    .padding_right(grid_num_pad_right())
                    .justify_end()
            } else {
                s.padding_horiz(grid_pad_h()).justify_start()
            };
            let formatted = gs
                .formats
                .with(|f| f.get(ci).map(|x| *x != ColumnFormat::None).unwrap_or(false));
            let s = if generating {
                // Dark purple wash that breathes via the pulse phase (key_foreign =
                // the sparkle purple). Reading `ai_pulse` here (and only here)
                // subscribes just the generating cells to the tick.
                let t = 0.5 + 0.5 * gs.ai_pulse.get().sin();
                s.background(theme::key_foreign().multiply_alpha((0.18 + 0.20 * t) as f32))
            } else if is_editing {
                // No highlight while editing, so the chromeless in-place editor
                // sits over the plain cell and reads as editing the cell itself.
                s
            } else if deleted {
                // Marked for deletion — red wash across the whole row (wins over
                // selection so it stays obvious).
                s.background(theme::error().multiply_alpha(0.15))
            } else if is_dirty {
                // Staged (uncommitted) edit — solid green fill.
                s.background(theme::grid_edit_staged())
            } else if is_active {
                s.background(theme::accent().multiply_alpha(0.30))
            } else if sel {
                s.background(theme::accent().multiply_alpha(0.16))
            } else if pending.is_some() {
                // Faint green wash across an un-set cell of a pending new row, so the
                // whole row reads as "being added" even before any cell is filled.
                s.background(theme::grid_edit_staged().multiply_alpha(0.15))
            } else if formatted {
                // At-a-glance cue this is a formatted (not raw DB) value.
                s.background(theme::dropdown_active())
            } else if i % 2 == 1 {
                // Opaque zebra fill (matches the row's `zebra_bg`). Cells must carry
                // their own background — not rely on the transparent-over-row-stripe
                // trick — so a live column resize, where neighbouring cells briefly
                // overlap before the flex row re-lays-out, occludes cleanly instead
                // of painting text over text.
                s.background(theme::bg_editor())
            } else {
                s.background(theme::bg_results())
            };
            // A generating cell gets a full purple frame (matching the sparkle);
            // otherwise the usual right divider so a narrow table still shows where
            // the final column ends.
            if generating {
                s.border(GRID_CELL_DIVIDER)
                    .border_color(theme::key_foreign())
            } else {
                s.border_right(GRID_CELL_DIVIDER)
                    .border_color(theme::border())
            }
        })
        // Clip so a value wider than the column doesn't spill over neighbours.
        .clip()
}

/// Left padding that right-aligns `text_px` of text inside the in-place editor of
/// a numeric cell `w` px wide — Floem 0.2 has no `text-align`, so the free space
/// is padded rather than aligned (see the `docs/architecture.md` note).
///
/// **It must be computed from the cell's real content box**, which is the column
/// width less the cell's own padding *and* its right divider — a border, so it
/// comes out of the content too. This got it wrong by 5px (it assumed a plain
/// 10px on both sides and no border), and 5px cost a whole character: floem sizes
/// the input's inner text node to `content − padding_left`, clips the moment the
/// text is a single pixel wider than that node, and clips on **glyph
/// boundaries**. A 2-digit id showed one digit; a 1-digit id showed none at all.
///
/// `SLACK` keeps that cliff at arm's length. The measurement here and floem's own
/// are the same layout of the same string, but the node width goes through an f32
/// percentage resolution, and being a hair under is not a hair of clipping — it is
/// a lost digit. Two pixels of slack is invisible against the value's right edge.
fn numeric_edit_pad_left(w: f64, text_px: f64) -> f64 {
    const SLACK: f64 = 2.0;
    let content = w - grid_pad_h() - grid_num_pad_right() - GRID_CELL_DIVIDER;
    (content - text_px - SLACK).max(0.0)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemaic_core::model::Column;
    use schemaic_core::schema::{ColumnInfo, IndexColumn, IndexInfo};

    fn col(name: &str, ty: &str) -> Column {
        Column {
            name: name.to_string(),
            type_name: ty.to_string(),
            origin: None,
        }
    }

    /// **A tab switch must not be able to lose a running export's Cancel, and a
    /// second request must not be able to take it away.**
    ///
    /// Both used to be one `bool`. The flag lived in the per-active-tab-render
    /// context while the export's cancellation token is app-global, so switching
    /// tabs disposed it and switching back built a fresh `false` — the stream ran
    /// on for minutes with no control bound to it and every further `All rows`
    /// request refused. And the tail of a finished request cleared the flag
    /// whenever *its own* scope was streamed, which is true of the second `All
    /// rows` request the app refuses synchronously, so asking for one tore down
    /// the running export's Cancel.
    #[test]
    fn only_the_export_that_raised_the_flag_can_lower_it() {
        let a = ExportRun::new(7);
        let b = ExportRun::new(7);
        assert_ne!(a, b, "two runs in one tab are still two runs");

        // The run that reported is the one holding the flag: it comes down.
        assert_eq!(export_finished(Some(a), a), None);
        // A different run reported — including a later one in the same tab, which
        // is the refused second `All rows` request. The flag stays up.
        assert_eq!(export_finished(Some(a), b), Some(a));
        assert_eq!(export_finished(Some(b), a), Some(b));
        // Nothing was running: a `Fetched` save's tail, which raised no flag.
        assert_eq!(export_finished(None, a), None);
    }

    /// And the bar asks about *its own* tab, because the signal is window-wide on
    /// purpose — reading it as a bare "is an export running" is how the Cancel got
    /// drawn on a tab that had nothing to do with it.
    #[test]
    fn the_export_bar_belongs_to_the_tab_that_started_it() {
        let exporting = RwSignal::new(None);
        let bars = |tab_id: usize| BarSignals {
            batch_err: create_memo(move |_| None),
            commit_err: RwSignal::new(None),
            commit_note: RwSignal::new(None),
            view_err: RwSignal::new(None),
            commit_wait: RwSignal::new(None),
            exporting,
            tab_id,
        };
        let here = bars(3);
        let elsewhere = bars(4);
        assert!(!here.exporting_here());
        assert!(!here.any_up());

        exporting.set(Some(ExportRun::new(3)));
        assert!(here.exporting_here(), "the tab that started it");
        assert!(here.any_up());
        assert!(
            !elsewhere.exporting_here(),
            "a tab that had nothing to do with it"
        );
        assert!(!elsewhere.any_up());
    }

    /// Run `f` at `scale`, then put the scale back — the registry is a
    /// `thread_local` and `--test-threads=1` shares one across tests, where a
    /// leaked scale is a *silent* failure (every number still self-consistent,
    /// just 1.6x what the next test expects). Same shape as `consts::scale_tests`.
    fn at_scale<R>(scale: crate::theme::UiScale, f: impl FnOnce() -> R) -> R {
        crate::theme::set_ui_scale(scale);
        let out = f();
        crate::theme::set_ui_scale(crate::theme::UiScale::Normal);
        out
    }

    /// **A column width estimate must reserve what the cell actually spends on
    /// chrome.** A cell pays `grid_pad_h()` on each side and its right divider is
    /// a border, so it comes out of the content box too — 21px at Normal, and
    /// scaling with the interface. Both estimators reserved a flat `22.0`, which
    /// is generous by 1px at Normal and short by 6px at 130% and 11px at 160%, so
    /// **Auto-fit clipped the value it exists to fit**. `numeric_edit_pad_left`
    /// composes the same three terms correctly, 7,000 lines away.
    ///
    /// The assertion is the relation, not the number: whatever the estimate is, it
    /// must leave room for `chars` characters *plus* the chrome. Pinning `21.0`
    /// would pass just as well with both sides wrong.
    #[test]
    fn a_width_estimate_reserves_the_chrome_the_cell_really_spends() {
        use crate::theme::UiScale;
        let rs = ResultSet::from_rows(
            vec![col("id", "INT")],
            vec![vec![Value::Str("1234567890".into())]],
        );
        let key_map = HashMap::new();
        for scale in [
            UiScale::Small,
            UiScale::Normal,
            UiScale::Large,
            UiScale::Huge,
        ] {
            at_scale(scale, || {
                let chrome = 2.0 * grid_pad_h() + GRID_CELL_DIVIDER;
                // The widest thing in the column is the 10-character value; the
                // header is `id` + 3 for the sort arrow, and the type line `INT`.
                let text = 10.0 * grid_char_w();
                let want = text + chrome;

                let est = init_widths(&rs, &key_map)[0];
                assert!(
                    est >= want || est >= max_col_w_init(),
                    "{scale:?}: init_widths reserved {est}, needs {want} \
                     ({text} of text + {chrome} of chrome)"
                );
                let fit = autofit_width(&rs, 0, false);
                assert!(
                    fit >= want,
                    "{scale:?}: autofit reserved {fit}, needs {want} \
                     ({text} of text + {chrome} of chrome)"
                );
            });
        }
    }

    /// **A stored width does not follow the scale, so it has to be carried.**
    ///
    /// `gs.widths` is seeded once from `init_widths` and the grid's rebuild key has
    /// no scale term, so raising the scale with a result open grew every cell's
    /// text inside columns still measured for the old one — text at 21px in
    /// columns cut for 13px type, ellipsized across the board until the statement
    /// was re-run or every divider double-clicked. Lowering it left every column
    /// ~1.6x wider than its content.
    ///
    /// Every column is carried, including one the user dragged: a width they chose
    /// to fit that column's *content* is a width that should still fit it when the
    /// content is 1.6x bigger. The new floor is applied after, because
    /// `min_col_w()` scales too — a column dragged to 48px at Normal is below the
    /// 77px floor at Huge.
    #[test]
    fn rescaling_widths_carries_every_column_and_re_applies_the_floor() {
        // Doubling: everything doubles.
        assert_eq!(
            rescale_widths(&[100.0, 250.0], 2.0, 10.0),
            vec![200.0, 500.0]
        );
        // Halving, with the floor biting on the narrow one only.
        assert_eq!(rescale_widths(&[100.0, 40.0], 0.5, 60.0), vec![60.0, 60.0]);
        // The identity is exact — a no-op ratio must not drift a single column,
        // because the effect that calls this runs on every scale *write*, not only
        // on a change.
        let widths = vec![63.0, 194.5, 900.0];
        assert_eq!(rescale_widths(&widths, 1.0, 10.0), widths);
        // Nothing to carry.
        assert!(rescale_widths(&[], 2.0, 10.0).is_empty());
        // A ratio that isn't a ratio leaves the widths alone rather than
        // collapsing them to the floor: the old measurement is a better answer
        // than zero.
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(rescale_widths(&widths, bad, 10.0), widths, "{bad}");
        }
    }

    // Single-column result of the given cells, so `compute_order` sorts column 0.
    fn rs_col(ty: &str, cells: Vec<Value>) -> ResultSet {
        ResultSet::from_rows(
            vec![col("c", ty)],
            cells.into_iter().map(|v| vec![v]).collect(),
        )
    }

    use schemaic_core::celledit::{BoolWire, Zoned};

    /// A datetime column that stores the wall clock as written. Which flavour is
    /// beside the point for everything in this module — the grid asks what
    /// *shape* a cell takes, and both take a calendar.
    fn naive() -> CellEditor {
        CellEditor::DateTime(Zoned::Naive)
    }

    // ── Type-aware editors: which control a column's cells get ──
    //
    // `celledit` decides what a *type* means; these cover the two joins the grid
    // makes around it — result column → its base column's **declared** type, and
    // column control → this cell's value.

    /// A result column carrying provenance, as a table-backed query produces.
    fn origin_col(name: &str, wire_ty: &str, db: &str, ns: Option<&str>, table: &str) -> Column {
        Column {
            name: name.to_string(),
            type_name: wire_ty.to_string(),
            origin: Some(schemaic_core::model::ColumnOrigin {
                database: db.to_string(),
                schema: ns.map(str::to_string),
                table: table.to_string(),
                column: name.to_string(),
                flags: Default::default(),
                binary: false,
                implicit_key: false,
            }),
        }
    }

    /// One database node holding `tables`, in the state the tree keeps them.
    fn node(database: &str, schema: Option<DbSchema>) -> ConnNode {
        let state = match schema {
            Some(s) => SchemaState::Loaded(Arc::new(s)),
            None => SchemaState::Loading,
        };
        ConnNode {
            id: 0,
            name: "conn".into(),
            database: database.to_string(),
            schema: RwSignal::new(state),
            refreshing: RwSignal::new(false),
            stats: RwSignal::new(crate::DbStatsState::Idle),
        }
    }

    fn schema_with(ns: Option<&str>, table: &str, cols: &[(&str, &str)]) -> DbSchema {
        DbSchema {
            tables: vec![TableInfo {
                name: table.to_string(),
                schema: ns.map(str::to_string),
                columns: cols
                    .iter()
                    .map(|(n, ty)| ColumnInfo {
                        name: n.to_string(),
                        type_name: ty.to_string(),
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    /// The whole point of resolving through the schema: MySQL hands an `ENUM`
    /// over the wire as a string and a `BOOLEAN` as `TINYINT`, so the wire type
    /// can answer neither question and the catalogue's `COLUMN_TYPE` must.
    #[test]
    fn a_columns_control_comes_from_its_declared_type_not_the_wire_one() {
        let rs = ResultSet::from_rows(
            vec![
                origin_col("rating", "CHAR", "sakila", None, "film"),
                origin_col("active", "TINYINT", "sakila", None, "film"),
            ],
            vec![vec![Value::Str("PG".into()), Value::Int(1)]],
        );
        let nodes = RwSignal::new(vec![node(
            "sakila",
            Some(schema_with(
                None,
                "film",
                &[("rating", "enum('G','PG')"), ("active", "tinyint(1)")],
            )),
        )]);
        let editors = column_editors(&rs, nodes, SqlDialect::MySql);
        assert_eq!(
            editors[0],
            CellEditor::Enum(vec!["G".into(), "PG".into()]),
            "the member list only exists in the catalogue"
        );
        assert_eq!(
            editors[1],
            CellEditor::Bool(BoolWire::OneZero),
            "and so does the tinyint(1) width"
        );
    }

    /// No provenance (an expression), an unloaded schema, or a table the node
    /// doesn't hold: the wire type is what is left, and it still knows a date.
    #[test]
    fn a_column_with_nothing_to_resolve_against_falls_back_to_the_wire_type() {
        let rs = ResultSet::from_rows(
            vec![
                col("now()", "DATETIME"),
                origin_col("created", "DATETIME", "app", None, "orders"),
                origin_col("name", "VARCHAR", "app", None, "orders"),
            ],
            vec![vec![Value::Null, Value::Null, Value::Null]],
        );
        // The database is in the tree but its introspection hasn't landed.
        let nodes = RwSignal::new(vec![node("app", None)]);
        let editors = column_editors(&rs, nodes, SqlDialect::MySql);
        // Both are `DATETIME`: the wire type is all there is, and it stores the
        // wall clock as written.
        assert_eq!(editors[0], naive());
        assert_eq!(editors[1], naive());
        assert_eq!(editors[2], CellEditor::Text);
    }

    /// A namespace is part of a table's identity — `sales.orders` and
    /// `public.orders` are different tables, and reading the wrong one's column
    /// would offer a control over the wrong values.
    #[test]
    fn a_same_named_table_in_another_namespace_is_not_the_source() {
        let rs = ResultSet::from_rows(
            vec![origin_col("state", "TEXT", "app", Some("sales"), "orders")],
            vec![vec![Value::Str("ok".into())]],
        );
        let nodes = RwSignal::new(vec![node(
            "app",
            Some(schema_with(Some("public"), "orders", &[("state", "mood")])),
        )]);
        assert_eq!(
            column_editors(&rs, nodes, SqlDialect::Postgres),
            vec![CellEditor::Text]
        );
    }

    /// The keyboard recovery's rule, and the two cases it has to tell apart —
    /// see [`reclaim_keyboard`] for why the pointer is what decides.
    #[test]
    fn a_blur_over_the_grid_takes_the_keyboard_back_and_one_elsewhere_does_not() {
        let grid = Rect::new(0.0, 100.0, 800.0, 600.0);
        // Escape, with the cursor still on the cell that was double-clicked.
        assert!(reclaim_keyboard((400.0, 300.0), grid));
        // A click in the SQL editor above the grid — it has just taken the
        // keyboard, and yanking it back is the regression this guards.
        assert!(!reclaim_keyboard((400.0, 40.0), grid));
        // The results toolbar, below the editor and above the table body.
        assert!(!reclaim_keyboard((400.0, 95.0), grid));
        // A grid that has never been laid out claims nothing.
        assert!(!reclaim_keyboard((400.0, 300.0), Rect::ZERO));
    }

    /// The other join: a column's control is only offered for a value it could
    /// have produced. This is the seam — the classification is per *column*, and
    /// one cell of it may hold something no control can represent.
    #[test]
    fn a_value_the_control_cannot_represent_keeps_the_text_field() {
        let bool_ctl = CellEditor::Bool(BoolWire::OneZero);
        let seven = FieldSig {
            ci: 0,
            buf: RwSignal::new("7".to_string()),
            is_null: RwSignal::new(false),
            flush: RwSignal::new(None),
        };
        assert_eq!(fitting_editor(bool_ctl.clone(), &seven), CellEditor::Text);

        let one = FieldSig {
            buf: RwSignal::new("1".to_string()),
            ..seven
        };
        assert_eq!(fitting_editor(bool_ctl.clone(), &one), bool_ctl);
    }

    /// A NULL field's buffer is empty, and "nothing chosen yet" fits every
    /// control — otherwise "Set value" would hand back a text box on a column
    /// that has a dropdown.
    #[test]
    fn an_empty_field_still_gets_its_control() {
        let empty = FieldSig {
            ci: 0,
            buf: RwSignal::new(String::new()),
            is_null: RwSignal::new(true),
            flush: RwSignal::new(None),
        };
        let enum_ctl = CellEditor::Enum(vec!["a".into()]);
        assert_eq!(fitting_editor(enum_ctl.clone(), &empty), enum_ctl);
        assert_eq!(fitting_editor(CellEditor::Date, &empty), CellEditor::Date);
    }

    // ── What an open cell is (`cell_shape`) ──

    /// A date cell is the third shape, not the second: the calendar drops from it
    /// while the text field stays, because a `DATETIME`'s time of day has no
    /// calendar to come from and a typed date is often faster than a paged one.
    #[test]
    fn a_date_cell_keeps_its_field_and_gets_a_calendar() {
        assert_eq!(
            cell_shape(CellEditor::Date, "2026-08-24"),
            CellShape::Calendar(CellEditor::Date)
        );
        assert_eq!(
            cell_shape(naive(), "2026-08-24 19:16:07"),
            CellShape::Calendar(naive())
        );
        // A pending row's blank cell: "nothing chosen yet" fits every control, so
        // the calendar is there to enter the date *with*.
        assert_eq!(
            cell_shape(CellEditor::Date, ""),
            CellShape::Calendar(CellEditor::Date)
        );
    }

    /// **A staged SQL NULL and a staged string spelling `NULL` are different
    /// writes, so they cannot paint the same.** Copy a NULL cell and paste it and
    /// the clipboard's display text comes back as the four characters — by
    /// design, since nothing may reinterpret a block that came from outside — so
    /// the grid is where the difference has to be visible, before the Commit that
    /// writes `middle_name = 'NULL'` into a column where `IS NULL` will then never
    /// match it again.
    ///
    /// Both are still white on the green fill: the fill is what says "staged",
    /// and it stays the same claim. The italic is the absence, which is the
    /// treatment a NULL *original* has always had.
    #[test]
    fn a_staged_null_does_not_paint_like_a_staged_word_null() {
        let sql_null: Option<Option<String>> = Some(None);
        let the_word: Option<Option<String>> = Some(Some("NULL".to_string()));
        assert_ne!(
            cell_ink(sql_null.as_ref(), false, false, false),
            cell_ink(the_word.as_ref(), false, false, false),
            "a staged NULL and a staged \"NULL\" are the two writes this cell \
             could make; nothing else in the grid tells them apart"
        );
        assert_eq!(
            cell_ink(sql_null.as_ref(), false, false, false),
            CellInk::StagedNull
        );
        assert_eq!(
            cell_ink(the_word.as_ref(), false, false, false),
            CellInk::Staged
        );
        // Any other staged text is ordinary staged text — the case above is not a
        // rule about the string `NULL`, it is a rule about a staged *absence*.
        let other: Option<Option<String>> = Some(Some("Ada".to_string()));
        assert_eq!(
            cell_ink(other.as_ref(), false, false, false),
            CellInk::Staged
        );
        // A staged value outranks every unstaged treatment, including a NULL
        // original underneath it and the FK underline.
        assert_eq!(
            cell_ink(the_word.as_ref(), true, true, true),
            CellInk::Staged
        );
    }

    /// And the four unstaged weightings, in the painter's own order — a NULL
    /// original outranks the foreign-key underline, because there is no key in
    /// the cell to follow.
    #[test]
    fn an_unstaged_cell_keeps_the_weighting_it_always_had() {
        assert_eq!(cell_ink(None, true, false, false), CellInk::Absent);
        assert_eq!(cell_ink(None, false, true, false), CellInk::Absent);
        assert_eq!(cell_ink(None, true, false, true), CellInk::Absent);
        assert_eq!(cell_ink(None, false, false, true), CellInk::Fk);
        assert_eq!(cell_ink(None, false, false, false), CellInk::Plain);
    }

    /// The same seam [`fitting_editor`] guards, on the cell's side: MySQL's
    /// zero date is a legal DATE value and no calendar can show it, so the cell
    /// stays a plain field rather than one that rewrites itself on first touch.
    #[test]
    fn a_date_no_calendar_can_show_keeps_the_plain_field() {
        assert_eq!(cell_shape(CellEditor::Date, "0000-00-00"), CellShape::Text);
        assert_eq!(cell_shape(naive(), "0000-00-00 00:00:00"), CellShape::Text);
    }

    /// **The composition, which is where this can go wrong**: the cell's padding
    /// is decided from the same shape its content is, and only a picker — which
    /// carries its own surface — may have it dropped. A calendar's cell holds an
    /// ordinary text field on the ordinary inset, so the value doesn't jump
    /// sideways as the editor opens.
    #[test]
    fn only_a_picker_takes_the_whole_cell() {
        let fills = |e: CellEditor, buf: &str| cell_fills(&cell_shape(e, buf));
        assert!(fills(CellEditor::Bool(BoolWire::OneZero), "1"));
        assert!(fills(CellEditor::Enum(vec!["G".into()]), "G"));
        assert!(fills(CellEditor::Set(vec!["a".into()]), "a"));
        assert!(!fills(CellEditor::Date, "2026-08-24"));
        assert!(!fills(naive(), "2026-08-24 19:16:07"));
        assert!(!fills(CellEditor::Text, "anything"));
        // And the fallback keeps the field's padding too — a `tinyint(1)` holding
        // 7 edits as text, in a cell that still reads like the display.
        assert!(!fills(CellEditor::Bool(BoolWire::OneZero), "7"));
    }

    // ── Header key icons (`key_roles`) ──

    fn table_with(cols: &[(&str, bool)], indexes: Vec<IndexInfo>) -> TableInfo {
        TableInfo {
            name: "t".into(),
            columns: cols
                .iter()
                .map(|(n, pk)| ColumnInfo {
                    name: n.to_string(),
                    primary_key: *pk,
                    ..Default::default()
                })
                .collect(),
            indexes,
            ..Default::default()
        }
    }

    fn index(name: &str, cols: &[&str], foreign: bool) -> IndexInfo {
        IndexInfo {
            name: name.into(),
            columns: cols
                .iter()
                .map(|c| IndexColumn {
                    name: c.to_string(),
                    ..Default::default()
                })
                .collect(),
            foreign,
            ..Default::default()
        }
    }

    fn at(pairs: &[(&'static str, usize)]) -> HashMap<&'static str, usize> {
        pairs.iter().copied().collect()
    }

    #[test]
    fn key_roles_marks_the_result_column_a_key_landed_in() {
        let t = table_with(
            &[("id", true), ("owner_id", false)],
            vec![index("fk_owner", &["owner_id"], true)],
        );
        // The result selected them in the other order, and aliased both.
        let roles = key_roles(&t, &at(&[("owner_id", 0), ("id", 1)]));
        assert_eq!(roles.get(&0), Some(&ColKey::Foreign));
        assert_eq!(roles.get(&1), Some(&ColKey::Primary));
    }

    /// The bug: a key column of the *source* table that the result doesn't
    /// actually contain must not decorate whatever column shares its name.
    #[test]
    fn key_roles_skips_a_key_column_that_is_not_in_the_result() {
        let t = table_with(&[("customerNumber", true)], Vec::new());
        assert!(key_roles(&t, &HashMap::new()).is_empty());
    }

    #[test]
    fn key_roles_ranks_primary_over_foreign_over_index() {
        let t = table_with(
            &[("id", true), ("owner_id", false), ("email", false)],
            vec![
                index("PRIMARY", &["id"], false),
                index("ix_owner", &["owner_id"], false),
                index("fk_owner", &["owner_id"], true),
                index("ix_email", &["email"], false),
            ],
        );
        let roles = key_roles(&t, &at(&[("id", 0), ("owner_id", 1), ("email", 2)]));
        assert_eq!(
            roles.get(&0),
            Some(&ColKey::Primary),
            "PK is not downgraded"
        );
        assert_eq!(
            roles.get(&1),
            Some(&ColKey::Foreign),
            "FK beats plain index"
        );
        assert_eq!(roles.get(&2), Some(&ColKey::Index));
    }

    #[test]
    fn key_roles_ignores_a_multi_column_index() {
        let t = table_with(
            &[("a", false), ("b", false)],
            vec![index("ix_ab", &["a", "b"], false)],
        );
        assert!(key_roles(&t, &at(&[("a", 0), ("b", 1)])).is_empty());
    }

    // ── JSON tree collapse ──

    /// Object member by ordinal — `m(0)` is the first entry in document order.
    fn m(i: usize) -> PathSeg {
        PathSeg::Member(i)
    }

    #[test]
    fn collapsing_the_json_root_hides_its_members() {
        // The root container's path is the empty vector, so it is `n = 0` that
        // matches it — the range used to start at 1 and the root collapsed to
        // nothing at all.
        let collapsed: HashSet<Vec<PathSeg>> = [vec![]].into_iter().collect();
        assert!(
            !json_path_hidden(&[], &collapsed),
            "the root renders itself"
        );
        assert!(json_path_hidden(&[m(0)], &collapsed));
        assert!(json_path_hidden(&[m(0), PathSeg::Index(0)], &collapsed));
    }

    #[test]
    fn collapsing_a_nested_json_container_hides_only_its_own_subtree() {
        let collapsed: HashSet<Vec<PathSeg>> = [vec![m(0)]].into_iter().collect();
        assert!(!json_path_hidden(&[], &collapsed));
        assert!(!json_path_hidden(&[m(0)], &collapsed));
        assert!(!json_path_hidden(&[m(1)], &collapsed));
        assert!(json_path_hidden(&[m(0), m(3)], &collapsed));
        assert!(json_path_hidden(
            &[m(0), m(3), PathSeg::Index(2)],
            &collapsed
        ));
    }

    #[test]
    fn nothing_is_hidden_when_nothing_is_collapsed() {
        let collapsed = HashSet::new();
        assert!(!json_path_hidden(&[], &collapsed));
        assert!(!json_path_hidden(&[m(0), m(1)], &collapsed));
    }

    // ── The row panel's pre-write flush ──
    //
    // The JSON tree editor keeps its leaf in a buffer of its own and only writes
    // the re-serialised tree into the field buffer on submit/blur — and clicking
    // Save never blurs it (floem moves focus on a pointer-down only for a
    // `keyboard_navigable` view). So the write has to ask each field to flush
    // before it reads the buffers.

    fn sig(ci: usize, value: &str) -> FieldSig {
        FieldSig {
            ci,
            buf: RwSignal::new(value.to_string()),
            is_null: RwSignal::new(false),
            flush: RwSignal::new(None),
        }
    }

    // ── The numeric cell's in-place editor (`numeric_edit_pad_left`) ──────

    /// The width floem gives the input's inner text node: the cell's content box
    /// less the padding we add. It clips the text — on a glyph boundary — the
    /// moment the text is wider than this, so the padding must always leave room.
    fn text_node_w(w: f64, pad_left: f64) -> f64 {
        w - grid_pad_h() - grid_num_pad_right() - GRID_CELL_DIVIDER - pad_left
    }

    #[test]
    fn a_numeric_edit_never_asks_for_more_width_than_the_cell_can_give() {
        // The regression: the padding was computed against `w - 20` while the cell
        // really offers `w - 10 - 14 - 1`, so the text node came out 5px short of
        // the text every time. Clipping is glyph-quantised, so "99" showed "9" and
        // "1" showed nothing at all.
        for w in [min_col_w(), 60.0, 100.0, cell_w(), 420.0] {
            for text_px in [0.0, 7.0, 14.0, 49.0, 120.0] {
                let pad = numeric_edit_pad_left(w, text_px);
                assert!(pad >= 0.0, "w={w} text={text_px}");
                if pad > 0.0 {
                    assert!(
                        text_node_w(w, pad) >= text_px,
                        "clipped: w={w} text={text_px} pad={pad}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_value_wider_than_its_column_is_left_aligned_and_clipped() {
        // Same as the display: no room to right-align into, so start at the left
        // edge and let floem clip to the caret.
        assert_eq!(numeric_edit_pad_left(cell_w(), 400.0), 0.0);
        assert_eq!(numeric_edit_pad_left(min_col_w(), 60.0), 0.0);
    }

    #[test]
    fn a_short_value_is_pushed_right_to_meet_the_display_alignment() {
        // 190 - 10 - 14 - 1 = 165 of content; a 14px value sits at the right edge
        // (less the 2px of slack that keeps floem's clip off the boundary).
        assert_eq!(numeric_edit_pad_left(cell_w(), 14.0), 149.0);
    }

    #[test]
    fn a_pending_field_edit_reaches_the_buffer_before_the_write_is_assembled() {
        let sigs = vec![sig(0, "{\"a\":1}"), sig(1, "x")];
        let buf = sigs[0].buf;
        // What the JSON editor installs: commit the open leaf into the buffer.
        sigs[0].flush.set(Some(Rc::new(move || {
            buf.set("{\"a\":2}".to_string());
            true
        })));

        assert!(flush_fields(&sigs));
        assert_eq!(
            field_state(&sigs),
            vec![
                (0, Some("{\"a\":2}".to_string())),
                (1, Some("x".to_string()))
            ],
            "the write must see the typed value, not the one it replaced"
        );
    }

    #[test]
    fn a_field_that_cannot_flush_stops_the_write() {
        let sigs = vec![sig(0, "{\"a\":1}"), sig(1, "y")];
        let touched = RwSignal::new(false);
        // An unparseable leaf: the editor shows its own error and keeps the leaf
        // open, so the buffer still holds the *stale* JSON — writing it would be
        // exactly the failure this flush exists to prevent.
        sigs[0].flush.set(Some(Rc::new(move || false)));
        sigs[1].flush.set(Some(Rc::new(move || {
            touched.set(true);
            true
        })));

        assert!(!flush_fields(&sigs));
        assert!(
            touched.get_untracked(),
            "every field flushes — one failure must not short-circuit the rest"
        );
    }

    #[test]
    fn fields_with_no_pending_editor_flush_trivially() {
        let sigs = vec![sig(0, "a"), sig(1, "b")];
        assert!(flush_fields(&sigs));
        assert_eq!(
            field_state(&sigs),
            vec![(0, Some("a".to_string())), (1, Some("b".to_string()))]
        );
    }

    // ── Column virtualization (`compute_window`) ──
    //
    // The invariant docs/architecture.md states for the data pane: `gs.widths` stays
    // full-length and each row's total width = `sum(widths[data_cols])`, the
    // spacers making up the hidden columns. If that ever stops holding, nothing
    // fails — the two panes just drift out of column alignment and
    // `scroll_active_into_view` scrolls to the wrong x.

    /// `left_pad + Σ widths[data_cols[start..end]] + right_pad` — what must equal
    /// the full `Σ widths[data_cols]` for every viewport.
    fn spanned(w: &ColWindow, widths: &[f64], data_cols: &[usize]) -> f64 {
        let visible: f64 = data_cols[w.start..w.end]
            .iter()
            .map(|&c| widths[c])
            .sum::<f64>();
        w.left_pad + visible + w.right_pad
    }

    fn total(widths: &[f64], data_cols: &[usize]) -> f64 {
        data_cols.iter().map(|&c| widths[c]).sum()
    }

    // Deliberately uneven, so an off-by-one in either pad shows up as a mismatch
    // rather than cancelling out.
    const W: [f64; 8] = [80.0, 120.0, 60.0, 200.0, 90.0, 150.0, 110.0, 70.0];

    fn vp_at(x0: f64, width: f64) -> Rect {
        Rect::new(x0, 0.0, x0 + width, 400.0)
    }

    #[test]
    fn compute_window_spacers_always_make_up_the_hidden_columns() {
        let all: Vec<usize> = (0..W.len()).collect();
        let sum = total(&W, &all);
        // Every scroll position, at three viewport widths, with and without
        // overscan — the pads must always account for exactly what isn't rendered.
        for width in [150.0, 400.0, 1200.0] {
            for overscan in [0, 2] {
                let mut x0 = 0.0;
                while x0 <= sum + 100.0 {
                    let w = compute_window(vp_at(x0, width), &W, &all, overscan);
                    assert!(
                        (spanned(&w, &W, &all) - sum).abs() < 1e-9,
                        "x0={x0} width={width} overscan={overscan} → {w:?}"
                    );
                    x0 += 25.0;
                }
            }
        }
    }

    #[test]
    fn compute_window_before_layout_renders_a_non_empty_slice() {
        // `vp.width() <= 1.0` is the pre-layout branch: the first frame must not be
        // blank, and its right pad still covers everything it skipped.
        let all: Vec<usize> = (0..W.len()).collect();
        let w = compute_window(Rect::ZERO, &W, &all, 2);
        assert_eq!((w.start, w.end), (0, W.len()));
        assert_eq!(w.left_pad, 0.0);
        assert_eq!(w.right_pad, 0.0);
        // With more columns than the initial slice, the tail is padded, not dropped.
        let widths: Vec<f64> = (0..40).map(|i| 50.0 + i as f64).collect();
        let cols: Vec<usize> = (0..40).collect();
        let w = compute_window(Rect::ZERO, &widths, &cols, 2);
        assert_eq!((w.start, w.end), (0, 16));
        assert!((spanned(&w, &widths, &cols) - total(&widths, &cols)).abs() < 1e-9);
    }

    #[test]
    fn compute_window_at_the_origin_has_no_left_pad() {
        let all: Vec<usize> = (0..W.len()).collect();
        let w = compute_window(vp_at(0.0, 200.0), &W, &all, 2);
        assert_eq!(w.start, 0);
        assert_eq!(w.left_pad, 0.0);
    }

    #[test]
    fn compute_window_scrolled_fully_right_reaches_the_last_column() {
        let all: Vec<usize> = (0..W.len()).collect();
        let sum = total(&W, &all);
        let w = compute_window(vp_at(sum - 150.0, 150.0), &W, &all, 2);
        assert_eq!(w.end, W.len());
        assert_eq!(w.right_pad, 0.0);
        assert!(
            w.start > 0,
            "a mid-scroll window should skip leading columns"
        );
    }

    #[test]
    fn compute_window_sums_over_data_cols_only_under_a_freeze() {
        // The frozen column is absent from `data_cols` but still present in
        // `widths` — its width must not leak into either spacer.
        let data_cols: Vec<usize> = (0..W.len()).filter(|&c| c != 3).collect();
        let sum = total(&W, &data_cols);
        assert!((sum - (total(&W, &(0..W.len()).collect::<Vec<_>>()) - W[3])).abs() < 1e-9);
        for x0 in [0.0, 90.0, 250.0, 600.0] {
            let w = compute_window(vp_at(x0, 200.0), &W, &data_cols, 2);
            assert!((spanned(&w, &W, &data_cols) - sum).abs() < 1e-9, "x0={x0}");
        }
    }

    #[test]
    fn compute_window_on_no_columns_is_empty_rather_than_panicking() {
        for vp in [Rect::ZERO, vp_at(0.0, 400.0), vp_at(500.0, 400.0)] {
            let w = compute_window(vp, &W, &[], 2);
            assert_eq!((w.start, w.end), (0, 0));
            assert_eq!((w.left_pad, w.right_pad), (0.0, 0.0));
        }
    }

    #[test]
    fn compute_window_overscan_widens_the_visible_window() {
        // Same viewport, more overscan → a window at least as wide on each side,
        // never narrower (that is what keeps a small scroll from exposing a blank).
        let all: Vec<usize> = (0..W.len()).collect();
        let vp = vp_at(200.0, 200.0);
        let tight = compute_window(vp, &W, &all, 0);
        let loose = compute_window(vp, &W, &all, 2);
        assert!(loose.start <= tight.start && loose.end >= tight.end);
        assert!(loose.start < tight.start || loose.end > tight.end);
    }

    // ── Keyboard navigation over the display grid (real rows + pending new rows) ──

    // 3 real rows + 2 pending = 5 display rows, 4 columns, one viewport page = 2.
    fn nav(from: (usize, usize), n: Nav) -> (usize, usize) {
        nav_target(5, 4, 2, from, n)
    }

    #[test]
    fn nav_target_moves_down_into_the_pending_rows() {
        // The regression: clamping to the *real* row count sent Arrow-Down from the
        // last real row nowhere, and from a pending row backwards into the real ones.
        assert_eq!(nav((2, 1), Nav::Down), (3, 1));
        assert_eq!(nav((3, 1), Nav::Down), (4, 1));
    }

    #[test]
    fn nav_target_clamps_at_the_last_display_row() {
        assert_eq!(nav((4, 1), Nav::Down), (4, 1));
        assert_eq!(nav((3, 1), Nav::PageDown), (4, 1));
        assert_eq!(nav((0, 0), Nav::Last), (4, 3));
    }

    #[test]
    fn nav_target_clamps_at_the_last_column() {
        assert_eq!(nav((1, 3), Nav::Right), (1, 3));
        assert_eq!(nav((1, 0), Nav::RowEnd), (1, 3));
    }

    #[test]
    fn nav_target_saturates_at_the_origin() {
        assert_eq!(nav((0, 0), Nav::Up), (0, 0));
        assert_eq!(nav((0, 0), Nav::Left), (0, 0));
        assert_eq!(nav((1, 2), Nav::PageUp), (0, 2));
        assert_eq!(nav((3, 2), Nav::First), (0, 0));
        assert_eq!(nav((3, 2), Nav::RowStart), (3, 0));
    }

    #[test]
    fn nav_target_on_an_empty_grid_stays_at_the_origin() {
        // `grid_key` returns early here, but the helper must not underflow.
        for n in [Nav::Down, Nav::Right, Nav::Last, Nav::RowEnd, Nav::PageDown] {
            assert_eq!(nav_target(0, 0, 2, (0, 0), n), (0, 0));
        }
    }

    // `pending_cell_text_reads_staged_values` moved with the rule it pinned:
    // `core::edit::tests::a_pending_new_row_reads_only_what_was_typed`.

    #[test]
    fn compute_order_none_is_identity() {
        let rs = rs_col("INT", vec![Value::Int(3), Value::Int(1), Value::Int(2)]);
        assert_eq!(compute_order(&rs, None), vec![0, 1, 2]);
    }

    #[test]
    fn compute_order_numeric_ascending_and_descending() {
        // Numeric-tagged cells compare numerically, not lexically (10 > 9).
        let rs = rs_col(
            "INT",
            vec![Value::Int(10), Value::Int(9), Value::Int(-1), Value::Int(2)],
        );
        assert_eq!(compute_order(&rs, Some((0, true))), vec![2, 3, 1, 0]);
        assert_eq!(compute_order(&rs, Some((0, false))), vec![0, 1, 3, 2]);
    }

    #[test]
    fn compute_order_nulls_sort_last_ascending() {
        // Ascending: non-nulls in order (1, 2), NULL last.
        let rs = rs_col("INT", vec![Value::Int(2), Value::Null, Value::Int(1)]);
        assert_eq!(compute_order(&rs, Some((0, true))), vec![2, 0, 1]);
        // Descending reverses the whole comparator (unchanged from the original
        // sort), so a NULL lands first — asserted here to pin that behavior.
        assert_eq!(compute_order(&rs, Some((0, false))), vec![1, 0, 2]);
    }

    #[test]
    fn compute_order_text_is_lexical() {
        // String-tagged cells compare as text ("10" < "9" lexically).
        let rs = rs_col(
            "VARCHAR",
            vec![
                Value::Str("banana".into()),
                Value::Str("apple".into()),
                Value::Str("cherry".into()),
            ],
        );
        assert_eq!(compute_order(&rs, Some((0, true))), vec![1, 0, 2]);
    }

    #[test]
    fn compute_order_matches_naive_pairwise_on_mixed_column() {
        // Decorate-sort must order identically to a per-pair comparison even when a
        // column mixes numeric-tagged and string-tagged cells (the defensive case):
        // numeric compares numerically only when *both* are numeric, else by text.
        let cells = vec![
            Value::Int(100),
            Value::Str("apple".into()),
            Value::Null,
            Value::Int(9),
            Value::Str("9zzz".into()),
        ];
        let rs = rs_col("MIXED", cells);
        let got = compute_order(&rs, Some((0, true)));
        // Reference: sort indices with a fresh per-pair comparator over cells.
        let mut want: Vec<usize> = (0..rs.row_count()).collect();
        want.sort_by(|&a, &b| {
            let (x, y) = (rs.cell(a, 0).unwrap(), rs.cell(b, 0).unwrap());
            match (x.is_null(), y.is_null()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => match (cell_num(x), cell_num(y)) {
                    (Some(p), Some(q)) => p.partial_cmp(&q).unwrap_or(Ordering::Equal),
                    _ => x.display().cmp(y.display()),
                },
            }
        });
        assert_eq!(got, want);
    }
}

#[cfg(test)]
mod delete_vote_tests {
    use super::*;

    /// A mixed selection **marks**: a per-row toggle there both marks and
    /// unmarks, which reads as the key doing nothing.
    #[test]
    fn a_mixed_range_marks_them_all() {
        let marked = [1usize];
        assert!(delete_vote(|di| marked.contains(&di), &[1, 2, 3]));
    }

    #[test]
    fn a_range_with_nothing_marked_marks() {
        assert!(delete_vote(|_| false, &[1, 2, 3]));
    }

    /// Only an already-fully-marked range unmarks — the second press of Del.
    #[test]
    fn a_fully_marked_range_unmarks() {
        assert!(!delete_vote(|_| true, &[1, 2, 3]));
    }

    /// Unreachable from the key, which returns early on an empty range, and
    /// harmless if it ever isn't: unmarking nothing.
    #[test]
    fn an_empty_range_unmarks_nothing() {
        assert!(!delete_vote(|_| true, &[]));
    }
}

#[cfg(test)]
mod one_bar_tests {
    use super::*;
    use floem::reactive::Scope;

    /// Opening either bar closes the other — **both** directions. The exclusion
    /// tracked `goto_open` alone, so Ctrl+F over an open Go-to-row left both
    /// mounted on one anchor and you typed into the one you couldn't see.
    #[test]
    fn opening_either_bar_closes_the_other() {
        let scope = Scope::new();
        let find = scope.create_rw_signal(false);
        let goto = scope.create_rw_signal(false);
        one_bar_at_a_time(find, goto);

        // Ctrl+G, then Ctrl+F — the direction that was missing.
        goto.set(true);
        find.set(true);
        assert!(find.get_untracked());
        assert!(!goto.get_untracked(), "Ctrl+F must close the goto bar");

        // And the direction that already worked.
        goto.set(true);
        assert!(goto.get_untracked());
        assert!(!find.get_untracked(), "Ctrl+G must close the find bar");

        scope.dispose();
    }

    /// Closing one must not open or re-close the other: `set` never dedups, so an
    /// unguarded write here would re-run the sibling effect and dispose a field
    /// the user is typing into.
    #[test]
    fn closing_a_bar_leaves_the_other_alone() {
        let scope = Scope::new();
        let find = scope.create_rw_signal(false);
        let goto = scope.create_rw_signal(false);
        one_bar_at_a_time(find, goto);

        find.set(true);
        find.set(false);
        assert!(!find.get_untracked());
        assert!(!goto.get_untracked());
        scope.dispose();
    }
}

#[cfg(test)]
mod goto_fires_tests {
    use super::*;

    /// The build run must not jump: this effect is created whenever the grid is,
    /// and jumping there would move the selection every time a result loaded.
    #[test]
    fn the_first_run_never_jumps() {
        assert!(!goto_fires(None, 0));
        assert!(!goto_fires(None, 7));
    }

    /// The effect re-runs when anything else it reads changes, and a second jump
    /// to the same row would fight a scroll the user had since made.
    #[test]
    fn an_unchanged_nonce_does_not_re_fire() {
        assert!(!goto_fires(Some(7), 7));
    }

    #[test]
    fn a_bumped_nonce_jumps() {
        assert!(goto_fires(Some(7), 8));
        // It only has to *differ* — a wrap is a bump like any other.
        assert!(goto_fires(Some(u64::MAX), 0));
    }
}

#[cfg(test)]
mod row_selection_tests {
    use super::*;
    use schemaic_core::model::{goto_target, row_selection};

    /// The **link between the two halves**: a row gesture — whether it came from
    /// the gutter or from Ctrl+G — must read as a *row* to the aggregates bar,
    /// not as column 0's arithmetic, which is usually an id whose sum means
    /// nothing. Core owns the gesture and `grid.rs` owns the reading, so the
    /// agreement between them can only be asserted here.
    #[test]
    fn a_row_gesture_reads_as_a_row_selection() {
        let (anchor, active) = row_selection(4, 5);
        let bounds = Some((anchor.0, anchor.1, active.0, active.1));
        assert_eq!(selection_kind(bounds, Some(anchor), 5), SelKind::WholeRow);
    }

    /// On a **one-column** result a row gesture selects a single cell, and the
    /// lone-cell rule wins over the single-column exemption — a cell aggregates
    /// to itself, so there is nothing to say. Worth pinning because the two
    /// rules meet here and either reading looks defensible in isolation.
    #[test]
    fn a_row_gesture_on_a_single_column_result_is_one_cell() {
        let (anchor, active) = row_selection(4, 1);
        assert_eq!(active, (4, 0), "the last column is also the first");
        let bounds = Some((anchor.0, anchor.1, active.0, active.1));
        assert_eq!(selection_kind(bounds, Some(anchor), 1), SelKind::Nothing);
    }

    /// And the jump lands on the same shape the gutter click makes, so it reads
    /// as a row selection too.
    #[test]
    fn a_jump_reads_as_a_row_selection() {
        let t = goto_target("5", 100, 5).unwrap();
        let bounds = Some((t.anchor.0, t.anchor.1, t.active.0, t.active.1));
        assert_eq!(selection_kind(bounds, Some(t.anchor), 5), SelKind::WholeRow);
    }
}

#[cfg(test)]
mod selection_kind_tests {
    use super::*;

    /// A lone cell aggregates to itself, so there is nothing worth saying.
    #[test]
    fn a_single_cell_gets_no_readout() {
        assert_eq!(
            selection_kind(Some((3, 1, 3, 1)), Some((3, 1)), 5),
            SelKind::Nothing
        );
    }

    #[test]
    fn nothing_selected_gets_no_readout() {
        assert_eq!(selection_kind(None, None, 5), SelKind::Nothing);
    }

    /// The column is the **anchor's**, not the rectangle's left edge: dragging
    /// leftward from `price` still reports `price`, because `bounds` is
    /// normalised and has forgotten which corner the drag began at.
    #[test]
    fn the_column_is_the_one_the_selection_started_on() {
        // Dragged from (0,3) leftward to (4,1): the rect starts at column 1.
        assert_eq!(
            selection_kind(Some((0, 1, 4, 3)), Some((0, 3)), 8),
            SelKind::Column(3)
        );
    }

    /// A span over every column is a row selection — the gutter click, Ctrl+A,
    /// the Ctrl+G jump — whose anchor column is 0, usually an id nobody wants a
    /// sum of.
    #[test]
    fn a_span_over_every_column_is_counts_only() {
        assert_eq!(
            selection_kind(Some((0, 0, 9, 4)), Some((0, 0)), 5),
            SelKind::WholeRow
        );
    }

    /// The exemption: in a one-column result, covering every column *is*
    /// covering the column you meant. Flipping the `ncols > 1` guard breaks
    /// exactly this and nothing else.
    #[test]
    fn a_single_column_result_still_aggregates() {
        assert_eq!(
            selection_kind(Some((0, 0, 9, 0)), Some((0, 0)), 1),
            SelKind::Column(0)
        );
    }

    /// A partial span within a wide result names its anchor's column even when
    /// it covers several — the readout is about one column by construction.
    #[test]
    fn a_partial_span_names_the_anchor_column() {
        assert_eq!(
            selection_kind(Some((0, 2, 3, 3)), Some((0, 2)), 5),
            SelKind::Column(2)
        );
    }
}

#[cfg(test)]
mod find_hits_tests {
    use super::*;
    use schemaic_core::model::Column;

    fn c(name: &str) -> Column {
        Column {
            name: name.to_string(),
            type_name: "VARCHAR".to_string(),
            origin: None,
        }
    }

    fn grid(rows: &[[&str; 2]]) -> ResultSet {
        ResultSet::from_rows(
            vec![c("a"), c("b")],
            rows.iter()
                .map(|r| r.iter().map(|s| Value::Str(s.to_string())).collect())
                .collect(),
        )
    }

    fn fmts() -> Vec<ColumnFormat> {
        vec![ColumnFormat::None; 2]
    }

    #[test]
    fn an_empty_query_matches_nothing() {
        let rs = grid(&[["alpha", "beta"]]);
        assert_eq!(find_hits(&rs, &[0], &fmts(), ""), (Vec::new(), false));
    }

    #[test]
    fn hits_are_display_positions_not_data_positions() {
        // `order` is the display→data mapping, so a sorted grid must report the
        // cell where the user is *looking*. Row 1 of the data is shown first.
        let rs = grid(&[["zulu", "x"], ["alpha", "y"]]);
        let (hits, more) = find_hits(&rs, &[1, 0], &fmts(), "zulu");
        assert!(!more);
        // "zulu" is data row 0, shown second → display row 1, column 0.
        assert_eq!(hits, vec![2], "display row 1, column 0");
    }

    #[test]
    fn matching_is_case_insensitive_and_substring() {
        let rs = grid(&[["Alpha", "beta"]]);
        assert_eq!(find_hits(&rs, &[0], &fmts(), "LPH").0, vec![0]);
    }

    #[test]
    fn every_matching_cell_in_a_row_is_its_own_hit() {
        let rs = grid(&[["match", "match"]]);
        assert_eq!(find_hits(&rs, &[0], &fmts(), "match").0, vec![0, 1]);
    }

    /// The two budgets exist so a huge result can't freeze the UI counting, and
    /// `more` is what lets the bar say "500+" honestly rather than a wrong total.
    #[test]
    fn the_hit_cap_reports_more_rather_than_a_wrong_total() {
        let rows: Vec<[&str; 2]> = vec![["hit", "hit"]; FIND_MAX_HITS];
        let rs = grid(&rows);
        let order: Vec<usize> = (0..rows.len()).collect();
        let (hits, more) = find_hits(&rs, &order, &fmts(), "hit");
        assert_eq!(hits.len(), FIND_MAX_HITS);
        assert!(more, "capped, so the count is a floor not a total");
    }

    #[test]
    fn the_scan_budget_also_reports_more() {
        // Enough cells to exhaust the cell budget with no match at all.
        let rows: Vec<[&str; 2]> = vec![["x", "y"]; FIND_COUNT_CELL_BUDGET];
        let rs = grid(&rows);
        let order: Vec<usize> = (0..rows.len()).collect();
        let (hits, more) = find_hits(&rs, &order, &fmts(), "nomatch");
        assert!(hits.is_empty());
        assert!(more, "the scan stopped early, so 0 is not a real total");
    }

    /// The find bar searches what is on screen, so a formatted column has to be
    /// matched on its *displayed* text — searching an epoch column for the date
    /// you can see must work.
    #[test]
    fn matching_uses_the_displayed_value_not_the_raw_one() {
        let rs = ResultSet::from_rows(
            vec![c("when")],
            vec![vec![Value::Int(0)], vec![Value::Int(86_400)]],
        );
        let formats = vec![ColumnFormat::Timestamp];
        let (hits, _) = find_hits(&rs, &[0, 1], &formats, "1970");
        assert_eq!(hits.len(), 2, "both epochs render as 1970 dates");
        // And the raw value is not what was searched.
        assert!(find_hits(&rs, &[0, 1], &formats, "86400").0.is_empty());
    }
}

#[cfg(test)]
mod clear_tests {
    use super::*;

    /// **`RwSignal::update` notifies whether or not the value changed** —
    /// floem_reactive's `update_value` calls `run_effects()` with no equality check
    /// — so clearing an already-empty staging collection is not the no-op it reads
    /// as. It re-runs every `dyn_container` keyed on the signal, and the grid body
    /// is keyed on `new_rows.len()`: discarding a single cell edit rebuilt the whole
    /// body, recomputed the sort order, and replaced `focus_id` — out from under the
    /// keyboard hand-back the same discard had already put in flight. From there the
    /// grid answered no key at all. See `discard_edits` and `refocus_grid`.
    #[test]
    fn clearing_what_is_already_empty_notifies_nobody() {
        let sig: RwSignal<Vec<u32>> = RwSignal::new(Vec::new());
        let runs = Rc::new(std::cell::Cell::new(0u32));
        let r = runs.clone();
        create_effect(move |_| {
            sig.with(|v| v.len());
            r.set(r.get() + 1);
        });
        assert_eq!(runs.get(), 1, "the effect's first run");

        clear_if_any(sig);
        assert_eq!(runs.get(), 1, "nothing to clear, so nothing was rebuilt");

        sig.update(|v| v.push(7));
        assert_eq!(runs.get(), 2, "a real change notifies");
        clear_if_any(sig);
        assert_eq!(runs.get(), 3, "and so does a real clear");
        assert!(sig.get_untracked().is_empty(), "which actually cleared it");

        // The unguarded spelling, for contrast — this is the floem fact the guard
        // exists for, and it is the whole bug.
        sig.update(|v| v.clear());
        assert_eq!(runs.get(), 4, "`update` notifies with nothing to do");
    }

    /// The same for the `Option` signals a discard resets: an editor that is
    /// already closed shouldn't announce closing again.
    #[test]
    fn clearing_a_none_option_notifies_nobody() {
        let sig: RwSignal<Option<(usize, usize)>> = RwSignal::new(None);
        let runs = Rc::new(std::cell::Cell::new(0u32));
        let r = runs.clone();
        create_effect(move |_| {
            sig.get();
            r.set(r.get() + 1);
        });
        assert_eq!(runs.get(), 1);

        clear_if_any(sig);
        assert_eq!(runs.get(), 1, "already None");

        sig.set(Some((1, 2)));
        assert_eq!(runs.get(), 2);
        clear_if_any(sig);
        assert_eq!(runs.get(), 3);
        assert_eq!(sig.get_untracked(), None);
    }

    /// Every collection a discard clears has to be askable, or the guard is only
    /// applied where someone remembered to write an impl.
    #[test]
    fn every_staging_collection_answers_the_question() {
        let dirty: RwSignal<HashMap<(usize, usize), Option<String>>> =
            RwSignal::new(HashMap::from([((0, 0), None)]));
        let rows: RwSignal<Vec<HashMap<usize, Option<String>>>> =
            RwSignal::new(vec![HashMap::new()]);
        let del: RwSignal<HashSet<usize>> = RwSignal::new(HashSet::from([3]));

        clear_if_any(dirty);
        clear_if_any(rows);
        clear_if_any(del);

        assert!(dirty.get_untracked().is_empty());
        assert!(rows.get_untracked().is_empty());
        assert!(del.get_untracked().is_empty());
    }
}
