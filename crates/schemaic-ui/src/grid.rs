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

use schemaic_core::connection::Connection;
use schemaic_core::edit::{EditModel, analyze_edit, refetch_key, refetch_template};
use schemaic_core::export::{ExportFormat, suggested_filename};
use schemaic_core::filter::{FilterError, build_query, eq_condition};
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
    MenuEntry, autohide, autohide_state, centered_msg, loading_dots, measure_text_px,
    shift_hscroll, thin_scroll, toolbar_icon, verb_spinner,
};
use crate::{ConnNode, FieldCfg, PopupAnchor, edit_field, icons, theme};

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
    container(verb_spinner(theme::text_dim, theme::FONT_BODY)).style(|s| {
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
    /// Which columns are editable + each base table's WHERE key (from the
    /// result's per-column provenance). Computed once per result set.
    edit_model: RwSignal<Arc<EditModel>>,
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
    /// Ui-level popup-menu signal, for the header/cell right-click menus.
    popup: RwSignal<Option<Vec<MenuEntry>>>,
    /// Anchor for the popup: `Some(PopupAnchor::BelowIcon(..))` opens it under a
    /// toolbar icon (the Copy dropdown); `None` opens at the cursor.
    popup_anchor: RwSignal<Option<PopupAnchor>>,
    /// `min_width` of the next popup panel; the Copy dropdown sets it so a stale
    /// width from a prior (narrower) menu can't shrink it.
    popup_width: RwSignal<f64>,
    /// The result's source `(database, table)` — for the cell "AI Summary" context.
    source: RwSignal<Option<TableSource>>,
    /// Callbacks wrapped in signals so `GridState` stays `Copy`. `summarize`
    /// reveals the AI panel + sends a message; `dismiss` closes any open menu;
    /// `commit` executes staged edits.
    summarize: RwSignal<Option<SummarizeFn>>,
    dismiss: RwSignal<Option<Rc<dyn Fn()>>>,
    commit: RwSignal<Option<crate::CommitFn>>,
    /// Writes an export to disk on a worker thread (see [`crate::ExportFn`]).
    export_file: RwSignal<Option<crate::ExportFn>>,
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
    /// This tab's SQL dialect (from its connection's engine) — used to build
    /// engine-correct SQL for grid actions like Follow-FK.
    dialect: SqlDialect,
    /// Server-side filter/sort: the base SQL to splice into, the active
    /// filter/sort state (persists across result reloads), and the re-run callback
    /// (wrapped so `GridState` stays `Copy`). See `schemaic_core::filter`.
    base_sql: RwSignal<Option<String>>,
    grid_query: RwSignal<schemaic_core::filter::GridQuery>,
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

/// A result column's **real** name on its base table, from the wire provenance.
/// Empty for an expression column, which is never written to.
fn origin_column(rs: &ResultSet, ci: usize) -> String {
    rs.columns
        .get(ci)
        .and_then(|c| c.origin.as_ref())
        .map(|o| o.column.clone())
        .unwrap_or_default()
}

/// The `WHERE` identity of data row `di`: each key column's real name paired
/// with the row's **original** value.
///
/// The one builder for it. Every write this grid issues — update, delete, and
/// the row panel's immediate save — is aimed at the row this names, so a
/// difference between copies is a statement aimed somewhere else.
fn row_key(rs: &ResultSet, key_cols: &[usize], di: usize) -> Vec<(String, Value)> {
    key_cols
        .iter()
        .map(|&kci| {
            let val = rs
                .cell(di, kci)
                .map(|c| c.to_value())
                .unwrap_or(Value::Null);
            (origin_column(rs, kci), val)
        })
        .collect()
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
            edit_model: RwSignal::new(Arc::new(EditModel::default())),
            commit_busy: RwSignal::new(false),
            commit_seq: RwSignal::new(0),
            // Shared with the panel-level error bar (rendered in `results_section`).
            commit_wait: gctx.commit_wait,
            tx_holders: RwSignal::new(Some(gctx.tx_holders.clone())),
            commit_err: gctx.commit_err,
            popup: gctx.popup,
            popup_anchor: gctx.popup_anchor,
            popup_width: gctx.popup_width,
            source: gctx.source,
            summarize: RwSignal::new(Some(gctx.summarize.clone())),
            dismiss: RwSignal::new(Some(gctx.dismiss.clone())),
            commit: RwSignal::new(Some(gctx.commit.clone())),
            export_file: RwSignal::new(Some(gctx.export_file.clone())),
            sync_canonical: RwSignal::new(gctx.sync_canonical.clone()),
            formats: RwSignal::new(formats),
            conn_id: gctx.conn_id,
            dialect,
            base_sql: gctx.base_sql,
            grid_query: gctx.grid_query,
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

    /// Stage a value for data-row `di`, column `ci` (`None` = SQL NULL). If it
    /// equals the original the entry is dropped (no longer dirty).
    fn stage(&self, di: usize, ci: usize, val: Option<String>) {
        // Original as `Option<String>`: NULL → `None`.
        let orig = self.rs.get_untracked().cell(di, ci).map(|c| {
            if c.is_null() {
                None
            } else {
                Some(c.display().to_string())
            }
        });
        let orig = orig.unwrap_or(None);
        self.dirty.update(|d| {
            if orig == val {
                d.remove(&(di, ci)); // reverted to original → no longer dirty
            } else {
                d.insert((di, ci), val.clone());
            }
        });
        // A fresh edit clears a stale commit error.
        if self.commit_err.get_untracked().is_some() {
            self.commit_err.set(None);
        }
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
        if self.commit_err.get_untracked().is_some() {
            self.commit_err.set(None);
        }
    }

    /// Stage a value into pending new-row `pidx`, column `ci` (`None` = SQL NULL,
    /// empty string clears the cell back to "use default"). New rows have no
    /// original to diff against, so an empty `Some("")` reverts the cell to unset
    /// (server default) rather than inserting an empty string.
    fn stage_new(&self, pidx: usize, ci: usize, val: Option<String>) {
        self.new_rows.update(|rows| {
            if let Some(row) = rows.get_mut(pidx) {
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
        if self.commit_err.get_untracked().is_some() {
            self.commit_err.set(None);
        }
    }

    /// Append a blank pending row and return its index.
    fn add_new_row(&self) -> usize {
        let mut idx = 0;
        self.new_rows.update(|rows| {
            idx = rows.len();
            rows.push(HashMap::new());
        });
        if self.commit_err.get_untracked().is_some() {
            self.commit_err.set(None);
        }
        idx
    }

    /// Append a pending row pre-filled from data-row `data_idx` (Clone / Duplicate),
    /// and return its index. Copies every editable column's value (or explicit NULL)
    /// **except** auto-increment columns, which are left for the server to assign.
    fn add_cloned_row(&self, data_idx: usize) -> usize {
        let model = self.edit_model.get_untracked();
        let rs = self.rs.get_untracked();
        let ncols = rs.col_count();
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
        let mut idx = 0;
        self.new_rows.update(|rows| {
            idx = rows.len();
            rows.push(map);
        });
        if self.commit_err.get_untracked().is_some() {
            self.commit_err.set(None);
        }
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
        if self.commit_err.get_untracked().is_some() {
            self.commit_err.set(None);
        }
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
        if self.commit_err.get_untracked().is_some() {
            self.commit_err.set(None);
        }
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
            key: row_key(rs, &tbl.key_cols, di),
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
                    key: row_key(&rs, &tbl.key_cols, di),
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
        self.commit_err.set(None);
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

/// Exact rendered pixel width of `text` in the grid's cell font (the app default
/// sans — IBM Plex Sans — at `FONT_BODY`), via a throwaway `TextLayout`. Used to
/// Estimate a column's initial width from its header + a sample of cell values.
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
                HEADER_KEY_ICON_W
            } else {
                0.0
            };
            (chars as f64 * GRID_CHAR_W + 22.0 + icon).clamp(MIN_COL_W, MAX_COL_W_INIT)
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
    let icon = if has_key { HEADER_KEY_ICON_W } else { 0.0 };
    (chars as f64 * GRID_CHAR_W + 22.0 + icon).clamp(MIN_COL_W, 900.0)
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

/// The clipboard text for one cell of a pending new row: the staged value, the
/// literal `NULL` for a staged SQL NULL (matching how the cell renders it), and
/// empty for a cell still unset — that one has no value yet, only the server
/// default the cell previews as `<auto>`/`<default>`.
fn pending_cell_text(row: Option<&HashMap<usize, Option<String>>>, ci: usize) -> &str {
    match row.and_then(|r| r.get(&ci)) {
        Some(Some(t)) => t,
        Some(None) => "NULL",
        None => "",
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
    let w = |k: usize| widths.get(data_cols[k]).copied().unwrap_or(CELL_W);
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
    let rh = ROW_H;
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
    let Some((r0, c0, r1, c1)) = gs.bounds_untracked() else {
        return;
    };
    let rs = gs.rs.get_untracked();
    let order = gs.order.get_untracked();
    // Display rows past the real ones are the pending new rows, whose values live
    // in `new_rows` — resolving them through `order` would fall back to the display
    // index and copy a row of blanks.
    let nreal = rs.row_count();
    let new_rows = gs.new_rows.get_untracked();
    let mut out = String::new();
    for i in r0..=r1 {
        if i > r0 {
            out.push('\n');
        }
        let pending = (i >= nreal).then(|| new_rows.get(i - nreal));
        let di = order.get(i).copied().unwrap_or(i);
        for ci in c0..=c1 {
            if ci > c0 {
                out.push('\t');
            }
            match pending {
                Some(row) => out.push_str(pending_cell_text(row, ci)),
                None => out.push_str(rs.cell(di, ci).map(|c| c.display()).unwrap_or_default()),
            }
        }
    }
    let _ = floem::Clipboard::set_contents(out);
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
fn save_export(gs: GridState, format: ExportFormat) {
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
    let Some(export) = gs.export_file.get_untracked() else {
        return;
    };
    save_as(opts, move |file| {
        let Some(path) = file.and_then(|f| f.path.first().cloned()) else {
            return; // cancelled
        };
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
            },
            // `try_update`: the result-set scope may have been disposed while the
            // dialog was open or the write ran — a plain `set` would panic on the
            // freed signal.
            Rc::new(move |res| {
                if let Err(e) = res {
                    gs.commit_err.try_update(|v| *v = Some(e));
                }
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
            .width(RESIZE_HIT_W)
            .height(GRID_HEADER_H)
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
            let d = pe.pos.x - RESIZE_HIT_W / 2.0;
            gs.widths.update(|w| {
                if let Some(x) = w.get_mut(ci) {
                    *x = (*x + d).clamp(MIN_COL_W, 1200.0);
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
    /// Reveal the AI panel + send a message (used for the cell "AI Summary").
    pub(crate) summarize: Rc<dyn Fn(String)>,
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
    /// Splice sink: replace the tab's canonical result set (so a later rebuild is
    /// fresh). `None` on the multi-result path (no splice — full re-run instead).
    pub(crate) sync_canonical: Option<SyncCanonicalFn>,
    /// The tab's connection is read-only → disable all inline editing (an empty
    /// `EditModel`, so no cell is editable / committable). Reactive.
    pub(crate) read_only: Memo<bool>,
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
    /// Last commit error (grid write-back), shown in a bottom error bar at the
    /// panel level (like the find bar at the top). Cleared by the next edit/commit.
    pub(crate) commit_err: RwSignal<Option<String>>,
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
/// It carries two things, and they can't coincide — a write is either still
/// waiting or has come back and failed:
/// - an **error** (commit write-back, or a filter/sort re-run), in the red fill;
/// - a **wait note** for a write that is taking long enough to need explaining
///   ([`arm_wait_note`]), on the ordinary chrome surface, with a one-click
///   `Rollback` when exactly one transaction of the user's own could be the
///   holder. It uses the footer's `tx_rollback` colour deliberately: it is the
///   same action on the same surface, and the two should never diverge.
pub(crate) fn grid_error_bar(
    commit_err: RwSignal<Option<String>>,
    view_err: RwSignal<Option<String>>,
    commit_wait: RwSignal<Option<WaitNote>>,
    rollback_tx: Rc<dyn Fn(usize)>,
    error_open: RwSignal<bool>,
    error_text: RwSignal<Option<String>>,
) -> impl IntoView {
    // An error wins: it describes a write that is already over, while the wait
    // note describes one still in flight (and every path clears the note before
    // reporting a failure anyway).
    let current = move || {
        commit_err
            .get()
            .or_else(|| view_err.get())
            .map(Err)
            .or_else(|| commit_wait.get().map(Ok))
    };
    dyn_container(current, move |state| {
        let msg = match state {
            None => return empty().into_any(),
            Some(Ok(note)) => return wait_bar(note, rollback_tx.clone()).into_any(),
            Some(Err(msg)) => msg,
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
                        .font_size(theme::FONT_BODY)
                        .margin_right(8.0)
                })
                .into_any()
        } else {
            empty().into_any()
        };
        h_stack((
            text(one_line).style(|s| {
                s.color(theme::reject_text())
                    .font_size(theme::FONT_BODY)
                    .max_width_pct(80.0)
                    .text_ellipsis()
                    .margin_left(8.0)
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
        if commit_err.get().is_some() || view_err.get().is_some() || commit_wait.get().is_some() {
            s.absolute()
                .inset_left(5.0)
                .inset_right(5.0)
                .inset_bottom(5.0)
                .height(35.0)
        } else {
            s
        }
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
                    .font_size(theme::FONT_BODY)
                    .flex_shrink(0.0_f32)
                    .margin_left(12.0)
                    .margin_right(8.0)
                    .hover(|s| s.color(theme::tx_rollback_hover()))
            })
            .into_any(),
    };
    h_stack((
        text(note.text).style(|s| {
            s.color(theme::text())
                .font_size(theme::FONT_BODY)
                .max_width_pct(80.0)
                .text_ellipsis()
                .margin_left(8.0)
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
                    font_size: 13.0,
                    border_radius: 6.0,
                    height: Some(FIELD_INPUT_H),
                    on_submit: Some(Rc::new(move || step(true))),
                    on_escape: Some(Rc::new(move || (esc)())),
                    on_arrow_up: Some(Rc::new(move || step(false))),
                    on_arrow_down: Some(Rc::new(move || step(true))),
                    ..Default::default()
                },
            )
            .style(|s| s.width(180.0));
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
                            s.font_size(theme::FONT_LABEL)
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
                        .gap(8.0)
                        .padding_horiz(8.0)
                        .padding_vert(6.0)
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
                    font_size: 13.0,
                    border_radius: 6.0,
                    height: Some(FIELD_INPUT_H),
                    on_submit: Some(submit),
                    on_escape: Some(Rc::new(move || (esc)())),
                    ..Default::default()
                },
            )
            // Wider than the editor's: a row number runs to six figures where a
            // line number rarely leaves three.
            .style(|s| s.width(78.0));
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
                    .style(|s| s.font_size(theme::FONT_LABEL).color(theme::text_dim())),
                input,
                close_btn,
            ))
            .style(|s| {
                s.items_center()
                    .gap(8.0)
                    .padding_horiz(8.0)
                    .padding_vert(6.0)
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

    let toolbar = grid_toolbar(
        gs,
        nrows,
        ncols,
        elapsed,
        truncated,
        capped_columns,
        sort,
        rs.database.clone(),
    );

    // Header + body rebuild together on a sort change OR a freeze toggle (both
    // repartition the columns between the frozen pane and the scrolling pane).
    // Layout is two panes side by side: a frozen pane (row-number gutter + an
    // optional frozen first column) and a horizontally-scrolling data pane. Both
    // panes are vertical scrolls kept in lockstep through `gs.scroll_to` (the
    // shared offset — data pane also owns the horizontal `h_off`).
    let grid = dyn_container(
        // Rebuild on sort / freeze change, and when the number of pending new rows
        // changes (adding/removing a row extends the virtual-stack length).
        move || (sort.get(), gs.frozen.get(), gs.new_rows.with(|v| v.len())),
        move |(sort_val, frozen_col, new_len)| {
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

            // ── Headers ──
            let gutter_header = container(
                text("#").style(|s| s.font_size(11.0).color(theme::text_faint())),
            )
            .style(|s| {
                s.width(GUTTER_W)
                    .height(GRID_HEADER_H)
                    .flex_shrink(0.0_f32)
                    .items_center()
                    .justify_end()
                    .padding_horiz(8.0)
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
                        vec![col_spacer(w.left_pad, GRID_HEADER_H).into_any()];
                    for k in w.start..w.end {
                        kids.push(
                            header_cell(gs, hdr_cols[k], sort_val, sort, km.clone()).into_any(),
                        );
                    }
                    kids.push(col_spacer(w.right_pad, GRID_HEADER_H).into_any());
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
                        let max_y = ((total as f64 * ROW_H) - vp.height()).max(0.0);
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
                        .height(GRID_HEADER_H)
                        .min_width(0.0)
                        .background(theme::bg_header_row())
                });
            let header = h_stack((frozen_header, data_header))
                .style(|s| s.flex_row().width_full().height(GRID_HEADER_H));

            // ── Bodies ──
            let (grid_shown, grid_poke) = autohide_state();
            let order_f = order.clone();
            let frozen_body = scroll(
                virtual_stack(
                    VirtualDirection::Vertical,
                    VirtualItemSize::Fixed(Box::new(move || ROW_H)),
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
                .style(|s| s.flex_col()),
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
                        let max_y = ((total as f64 * ROW_H) - vp.height()).max(0.0);
                        let new_y = (vscroll.get_untracked() + dy).clamp(0.0, max_y);
                        gs.scroll_to.set(Some(Point::new(vp.x0, new_y)));
                    }
                }
                EventPropagation::Stop
            })
            .scroll_style(|s| s.hide_bars(true))
            .style(move |s| {
                let w = GUTTER_W
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
                    VirtualItemSize::Fixed(Box::new(move || ROW_H)),
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
                .style(|s| s.flex_col()),
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
            .on_event(EventListener::KeyDown, move |e| {
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

            let body = h_stack((frozen_body, data_body)).style(|s| {
                s.flex_row()
                    .flex_grow(1.0_f32)
                    .width_full()
                    .min_height(0.0)
                    .min_width(0.0)
            });

            v_stack((header, body))
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
            .min_height(120.0)
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
    // because the row count is — including the pending unsaved rows, which the
    // gutter numbers too, so "row N" means the same thing typed as it does read.
    //
    // Selecting the whole row rather than a cell is the same gesture a gutter
    // click makes (anchor at column 0, active at the last), so the row lights up
    // the way the user already knows. The scroll is asked for at column 0: a jump
    // should not also fling the viewport to the far right, which is what following
    // the *active* cell would do.
    create_effect(move |seen: Option<u64>| {
        let step = gs.goto_step.get();
        if seen == Some(step) || seen.is_none() {
            return step;
        }
        let total = nrows + gs.new_rows.with_untracked(|v| v.len());
        let target = gs
            .goto_query
            .with_untracked(|q| schemaic_core::model::goto_row_index(q, total));
        if let Some(i) = target {
            gs.anchor.set(Some((i, 0)));
            gs.active.set(Some((i, ncols.saturating_sub(1))));
            scroll_active_into_view(gs, i, 0);
        }
        // Always close, even on a miss — the editor's go-to-line does the same,
        // and a popup that stays open after Enter reads as "still working".
        // Closing is what hands the keyboard back, via the effect below; doing it
        // here as well would be a second path to the same place.
        gs.goto_open.set(false);
        gs.goto_query.set(String::new());
        step
    });
    // Only one of the two bars is up at a time — they share an anchor, so both
    // open would paint one over the other. The editor's pair does this too.
    create_effect(move |_| {
        if gs.goto_open.get() {
            gs.find_open.set(false);
        }
    });
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
fn refocus_grid(gs: GridState) {
    if let Some(f) = gs.focus_id.get_untracked() {
        floem::action::exec_after(std::time::Duration::from_millis(0), move |_| {
            f.request_focus();
        });
    }
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
    gs.commit_err.set(None);
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
    let pidx = gs.add_cloned_row(data_idx);
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
    if gs.ai_busy.get_untracked() {
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
    if gs.ai_busy.get_untracked() || count == 0 {
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
    if gs.ai_busy.get_untracked() {
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
                container(text(format!("{n}")).style(|s| s.font_size(theme::FONT_LABEL)))
                    .on_click_stop(move |_| {
                        gs.seed_buf.set(n.to_string());
                        (go)();
                    })
                    .style(|s| {
                        s.padding_horiz(10.0)
                            .padding_vert(4.0)
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
                        height: Some(30.0),
                        on_submit: Some(go),
                        on_escape: Some(Rc::new(esc)),
                        ..FieldCfg::default()
                    },
                )
                .style(|s| s.width(70.0))
            };
            let go_btn = go.clone();
            let panel = v_stack((
                text("Seed rows")
                    .style(|s| s.font_size(theme::FONT_LABEL).color(theme::text_muted())),
                h_stack((
                    field,
                    container(text("Generate").style(|s| s.font_size(theme::FONT_BODY)))
                        .on_click_stop(move |_| (go_btn)())
                        .style(|s| {
                            s.padding_horiz(12.0)
                                .padding_vert(6.0)
                                .border_radius(6.0)
                                .color(floem::peniko::Color::WHITE)
                                .background(theme::seed_button())
                                .cursor(CursorStyle::Default)
                                .hover(|s| s.background(theme::seed_button().multiply_alpha(0.85)))
                        }),
                ))
                .style(|s| s.gap(6.0).items_center()),
                h_stack((
                    preset(5, go.clone(), gs),
                    preset(10, go.clone(), gs),
                    preset(25, go.clone(), gs),
                    preset(50, go.clone(), gs),
                ))
                .style(|s| s.gap(6.0)),
            ))
            .style(|s| {
                crate::widgets::panel_style(s)
                    .absolute()
                    .inset_top(30.0) // just below the 28px toolbar
                    .inset_right(8.0)
                    .background(theme::bg_chrome())
                    .padding(12.0)
                    .gap(8.0)
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
const FIELD_NAME_W: f64 = 150.0;
/// Fixed height of a scalar field row — so toggling sentinel/`<null>` ↔ input never
/// reflows the rows below.
const FIELD_ROW_H: f64 = 32.0;

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
            s.font_size(13.0)
                .color(theme::text())
                .text_ellipsis()
                .min_width(0.0)
                .flex_grow(1.0_f32)
        }),
        text(type_name).style(|s| {
            s.font_size(13.0)
                .color(theme::text_faint())
                .margin_left(6.0)
                .flex_shrink(0.0_f32)
        }),
    ))
    .style(|s| {
        s.items_center()
            .width(FIELD_NAME_W)
            .flex_shrink(0.0_f32)
            .padding_right(10.0)
    })
}

/// A small borderless text button (the per-field Set-NULL / Set-value / Unset
/// affordances): no background, just a text-colour hover.
fn field_mini_btn(label: &'static str, action: impl Fn() + 'static) -> AnyView {
    container(text(label).style(|s| s.font_size(13.0)))
        .on_click_stop(move |_| action())
        .style(|s| {
            s.padding_horiz(4.0)
                .flex_shrink(0.0_f32)
                .color(theme::text_dim())
                .hover(|s| s.color(theme::text()))
        })
        .into_any()
}

/// The dim `<null>` sentinel shown for a NULL field / value.
fn null_sentinel() -> AnyView {
    text("<null>")
        .style(|s| s.font_size(13.0).color(theme::text_faint()))
        .into_any()
}

/// The editable value cell for a scalar field: a text input, plus (for a nullable
/// column) a NULL toggle. NULL is an explicit state — clearing the text to empty is
/// the empty string, not NULL. A `<null>` field re-enables on **double-click** (same
/// as its "Set value" button).
fn scalar_editor(gs: GridState, nullable: bool, autofocus: bool, f: FieldSig) -> AnyView {
    let make_field = move || {
        edit_field(
            f.buf,
            FieldCfg {
                background: theme::bg_editor,
                font_size: 13.0,
                autofocus,
                height: Some(FIELD_INPUT_H),
                // Escape closes the panel even while a field is focused.
                on_escape: Some(Rc::new(move || gs.edit_row_open.set(false))),
                ..Default::default()
            },
        )
    };
    if !nullable {
        return make_field().style(|s| s.width_full()).into_any();
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
                .style(|s| s.items_center().width_full().gap(8.0))
                .on_double_click_stop(move |_| f.is_null.set(false))
                .into_any()
            } else {
                h_stack((
                    make_field().style(|s| s.flex_grow(1.0_f32).min_width(0.0)),
                    field_mini_btn("Set NULL", move || f.is_null.set(true)),
                ))
                .style(|s| s.items_center().width_full().gap(6.0))
                .into_any()
            }
        },
    )
    .style(|s| s.width_full())
    .into_any()
}

/// True for a JSON/JSONB column type (MySQL `json`, Postgres `json`/`jsonb`).
fn is_json_type(type_name: &str) -> bool {
    let t = type_name.trim();
    t.eq_ignore_ascii_case("json") || t.eq_ignore_ascii_case("jsonb")
}

/// Left indent (px) per JSON tree depth level.
const JSON_INDENT: f64 = 15.0;

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
    let indent = r.depth as f64 * JSON_INDENT;
    let path = r.path.clone();

    let disclosure: AnyView = if matches!(r.kind, RowKind::Scalar) {
        empty()
            .style(|s| s.width(15.0).flex_shrink(0.0_f32))
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
            s.width(15.0)
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
            text(k.clone()).style(|s| s.font_size(13.0).color(theme::key_index())),
            text(":").style(|s| {
                s.font_size(13.0)
                    .color(theme::text_faint())
                    .margin_right(6.0)
            }),
        ))
        .style(|s| s.items_center().flex_shrink(0.0_f32))
        .into_any(),
        (None, Some(PathSeg::Index(i))) => text(format!("[{i}]"))
            .style(|s| {
                s.font_size(13.0)
                    .color(theme::text_faint())
                    .margin_right(6.0)
                    .flex_shrink(0.0_f32)
            })
            .into_any(),
        _ => empty().into_any(),
    };

    let value: AnyView = match &r.kind {
        RowKind::Object(n) => text(format!("{{{n}}}"))
            .style(|s| s.font_size(13.0).color(theme::text_faint()))
            .into_any(),
        RowKind::Array(n) => text(format!("[{n}]"))
            .style(|s| s.font_size(13.0).color(theme::text_faint()))
            .into_any(),
        RowKind::Scalar => {
            if is_editing {
                edit_field(
                    edit_buf,
                    FieldCfg {
                        background: theme::bg_deepest,
                        font_size: 13.0,
                        autofocus: true,
                        height: Some(FIELD_INPUT_H),
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
                container(text(vj).style(|s| s.font_size(13.0).color(theme::text())))
                    .on_click_stop(move |_| (start_edit)(p.clone(), vj2.clone()))
                    .style(|s| {
                        s.padding_horiz(4.0)
                            .padding_vert(1.0)
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
    .style(|s| s.items_center().width_full().min_height(FIELD_ROW_H))
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
                font_size: 13.0,
                height: Some(FIELD_INPUT_H),
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
                .padding(6.0)
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
                .style(|s| s.items_center().width_full().gap(8.0))
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
            s.font_size(13.0)
                .color(theme::text_dim())
                .text_ellipsis()
                .min_width(0.0)
                .flex_grow(1.0_f32)
        })
        .into_any()
}

/// One field row: the column label + its value editor (editable) or read-only cell.
fn field_row(
    gs: GridState,
    name: String,
    type_name: String,
    editable: bool,
    nullable: bool,
    autofocus: bool,
    f: FieldSig,
) -> AnyView {
    let is_json = is_json_type(&type_name);
    let editor = if !editable {
        readonly_value(f)
    } else if is_json {
        json_field(nullable, f, gs.commit_err)
    } else {
        scalar_editor(gs, nullable, autofocus, f)
    };
    h_stack((
        field_label(name, type_name),
        container(editor).style(|s| s.flex_grow(1.0_f32).min_width(0.0)),
    ))
    .style(move |s| {
        let s = s.width_full().gap(8.0).padding_vert(3.0);
        // A JSON tree grows tall — top-align the label and let the row grow. A scalar
        // row keeps a *fixed* height so toggling `<null>` ↔ input (which are different
        // natural heights) never reflows the rows below.
        if is_json {
            s.items_start().min_height(FIELD_ROW_H)
        } else {
            s.items_center().height(FIELD_ROW_H)
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
        move || (gs.edit_row_open.get(), gs.edit_row_di.get()),
        move |(open, di_opt)| {
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
                rows.push(field_row(
                    gs,
                    c.name.clone(),
                    type_name,
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
                    s.padding(4.0)
                        .color(theme::text_dim())
                        .hover(|s| s.color(theme::text()))
                });
            let trailing = if any_editable {
                let save_btn = container(icons::icon(icons::CHECK, 14.0))
                    .on_click_stop(move |_| (save)())
                    .style(|s| {
                        s.padding(4.0)
                            .color(theme::text_dim())
                            .hover(|s| s.color(theme::text()))
                    });
                h_stack((save_btn, close_btn))
                    .style(|s| s.flex_row().items_center().gap(4.0))
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
                            s.padding(4.0)
                                .color(theme::text_dim())
                                .hover(|s| s.color(theme::text()))
                        })
                        .into_any()
                } else {
                    btn.style(|s| s.padding(4.0).color(theme::text_faint()))
                        .into_any()
                }
            };
            let nav = h_stack((
                nav_chevron(icons::CHEVRON_UP, can_prev, false),
                nav_chevron(icons::CHEVRON_DOWN, can_next, true),
            ))
            .style(|s| s.flex_row().items_center().gap(2.0).margin_left(8.0));

            let head = h_stack((
                text(title).style(|s| s.font_size(theme::FONT_LABEL).color(theme::text_dim())),
                nav,
                empty().style(|s| s.flex_grow(1.0_f32)),
                trailing,
            ))
            .style(|s| {
                s.width_full()
                    .items_center()
                    .gap(4.0)
                    .height(24.0)
                    .flex_shrink(0.0_f32)
                    .padding_horiz(10.0)
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
            let fields = autohide(scroll(
                v_stack_from_iter(rows).style(|s| s.width_full().flex_col().padding_horiz(10.0)),
            ))
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
                        loading_dots("Saving", theme::text_dim, theme::FONT_LABEL).into_any()
                    } else {
                        empty().into_any()
                    }
                },
            )
            .style(|s| s.width_full().padding_horiz(10.0));

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
                        .gap(8.0)
                        .padding_vert(8.0)
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
    gs.edit_cell.set(None);
    gs.dirty.update(|d| d.clear());
    gs.new_rows.update(|r| r.clear());
    // The pending-row indices are about to be handed out again from zero.
    gs.new_rows_gen.update(|g| *g = g.wrapping_add(1));
    gs.del_rows.update(|d| d.clear());
    gs.commit_err.set(None);
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
    // Shift+Arrow no longer extends a multi-cell selection — keyboard nav always
    // moves the single active cell. (Mouse drag-select + copy still work.)
    let shift = false;
    let ctrl = m.control() || m.meta();
    let active_opt = gs.active.get_untracked();
    let (r, c) = active_opt.unwrap_or((0, 0));
    let last_r = rows - 1;
    let last_c = ncols - 1;
    let page = ((gs.vp.get_untracked().height() / ROW_H).floor() as usize).max(1);
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
            // Toggle the active real row's "marked for deletion" state (single
            // writable table only). No selection, or a pending row → no-op.
            if active_opt.is_some() && gs.edit_model.get_untracked().insert_target().is_some() => {
                if let Some(&di) = gs.order.get_untracked().get(r) {
                    gs.toggle_delete(di);
                } else {
                    return EventPropagation::Continue;
                }
            }
        _ => return EventPropagation::Continue,
    }
    EventPropagation::Stop
}

/// A uniform toolbar icon button: a 16px Lucide glyph in a padded hitbox (3px
/// vertical / 5px horizontal, matching the footer icons), coloured `text_muted`
/// and brightening to `text` on hover. `enabled` gates the click + hover; when it
/// returns false the glyph dims to 30% alpha and is inert.
/// A thin vertical divider between toolbar icon groups. Extra horizontal margin so
/// it sits clear of the icons on either side (combined with the group gap).
fn toolbar_sep() -> impl IntoView {
    empty().style(|s| {
        s.width(1.0)
            .height(14.0)
            .flex_shrink(0.0_f32)
            .margin_horiz(5.0)
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
                    font_size: theme::FONT_LABEL,
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
                                .margin_left(6.0)
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
                        .gap(4.0)
                        .width_full()
                        .height(34.0)
                        .flex_shrink(0.0_f32)
                        .background(theme::bg_deepest())
                        .padding_right(10.0)
                        .border_bottom(1.0)
                        .border_color(theme::border())
                })
                .into_any()
        },
    )
}

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
) -> impl IntoView {
    let cap = if truncated { " (capped)" } else { "" };
    // The database leads the line, because it is the fact that says what the rest
    // of the line is *about*. Taken from the result rather than the tab: the tab's
    // selection moves on, and a result that outlived it must not claim the new one
    // (`ResultSet::database`). A connection with no default database says nothing
    // rather than inventing a name.
    let scope = database.map(|d| format!("{d} · ")).unwrap_or_default();
    let stats = text(format!(
        "{scope}{} {}{cap} · {ncols} {} · {elapsed_ms} ms",
        human_count(nrows),
        plural(nrows, "row", "rows"),
        plural(ncols, "col", "cols"),
    ))
    .style(|s| s.color(theme::text_dim()).font_size(theme::FONT_LABEL));
    // A column whose 512 MiB text arena filled up renders blank from that row
    // on. Said out loud, because the alternative is the user discovering empty
    // cells partway down a result with nothing to attribute them to — and unlike
    // the row cap, this one loses data inside rows that are present.
    let arena_note = if capped_columns.is_empty() {
        empty().into_any()
    } else {
        text(format!(
            "· {} too large to hold in full — later rows show blank",
            capped_columns.join(", ")
        ))
        .style(|s| s.color(theme::error()).font_size(theme::FONT_LABEL))
        .into_any()
    };
    // Sorting a capped result reorders only the fetched subset — flag it.
    let caveat = dyn_container(
        move || truncated && sort.get().is_some(),
        move |show| {
            if show {
                text("· sorted subset (capped) — not the full order")
                    .style(|s| s.color(theme::error()).font_size(theme::FONT_LABEL))
                    .into_any()
            } else {
                empty().into_any()
            }
        },
    );
    // Commit / discard, shown only when there are staged changes (cell edits +
    // pending new rows + pending deletes). Sits first in the icon cluster, followed
    // by a separator. Commit is a green (grid_edit_staged #509950) button — check
    // glyph + the change count (Ctrl+Enter); discard a red (#9D3434) ✗. Both
    // background-free with the same padded hitbox as the other icons; brighten on
    // hover.
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
                    s.font_size(theme::FONT_LABEL)
                        .color(commit_color())
                        .margin_left(4.0)
                }),
            ))
            .on_click_stop(move |_| commit_grid(gs))
            .on_event_cont(EventListener::PointerEnter, move |_| commit_hov.set(true))
            .on_event_cont(EventListener::PointerLeave, move |_| commit_hov.set(false))
            .style(|s| {
                s.items_center()
                    .padding_vert(3.0)
                    .padding_horiz(5.0)
                    .cursor(CursorStyle::Default)
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
                    .padding_vert(3.0)
                    .padding_horiz(5.0)
                    .cursor(CursorStyle::Default)
            });
            h_stack((commit, discard, toolbar_sep()))
                .style(|s| s.items_center().flex_row().gap(3.0))
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
    let row_actions = dyn_container(
        move || gs.edit_model.get().insert_target().is_some(),
        move |show| {
            if !show {
                return empty().into_any();
            }
            h_stack((
                toolbar_icon(icons::PLUS, 0.0, 0.0, || true, move || add_pending_row(gs)),
                toolbar_icon(icons::MINUS, 0.0, 0.0, row_selected, move || {
                    if let Some(di) = selected_data_row() {
                        gs.toggle_delete(di);
                    }
                }),
                toolbar_icon(icons::COPY_PLUS, 0.0, 0.0, row_selected, move || {
                    if let Some(di) = selected_data_row() {
                        clone_row(gs, di);
                    }
                }),
                toolbar_sep(),
            ))
            .style(|s| s.items_center().flex_row().gap(3.0))
            .into_any()
        },
    );
    // Copy icon → themed dropdown (JSON / CSV / SQL). Same neutral styling + padded
    // hitbox as the other icons; `on_event_stop(PointerDown)` keeps the root
    // pointer-down dismissal from closing the menu the same click opens it. The
    // `on_move` tracks the glyph origin so the dropdown anchors under it.
    let copy_origin = RwSignal::new(Point::ZERO);
    let copy_hov = RwSignal::new(false);
    let copy_menu = container(
        icons::icon(icons::COPY, 16.0)
            .on_move(move |p| copy_origin.set(p))
            .style(move |s| {
                let c = if copy_hov.get() {
                    theme::text()
                } else {
                    theme::text_muted()
                };
                s.color(c).flex_shrink(0.0_f32)
            }),
    )
    .on_click_stop(move |_| {
        // Close any other open menu (schema eye/settings, connection switcher, …)
        // so this dropdown is mutually exclusive with them.
        if let Some(d) = gs.dismiss.get_untracked() {
            (d)();
        }
        // Anchor the panel just below the icon (left/right edges + bottom).
        let o = copy_origin.get_untracked();
        let sz = 16.0; // the COPY glyph size above
        gs.popup_width.set(GRID_COPY_MENU_W);
        gs.popup_anchor
            .set(Some(PopupAnchor::BelowIcon(o.x, o.x + sz, o.y + sz)));
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
    })
    .on_event_cont(EventListener::PointerEnter, move |_| copy_hov.set(true))
    .on_event_cont(EventListener::PointerLeave, move |_| copy_hov.set(false))
    .on_event_stop(EventListener::PointerDown, |_| {})
    .style(|s| {
        s.items_center()
            .padding_vert(3.0)
            .padding_horiz(5.0)
            .cursor(CursorStyle::Default)
    });

    // Download icon → the same format dropdown as Copy, but each choice opens a
    // save dialog and writes the file. Identical styling/anchoring to `copy_menu`
    // so the pair reads as one control: copy it, or save it.
    let save_origin = RwSignal::new(Point::ZERO);
    let save_hov = RwSignal::new(false);
    let save_menu = container(
        icons::icon(icons::DOWNLOAD, 16.0)
            .on_move(move |p| save_origin.set(p))
            .style(move |s| {
                let c = if save_hov.get() {
                    theme::text()
                } else {
                    theme::text_muted()
                };
                s.color(c).flex_shrink(0.0_f32)
            }),
    )
    .on_click_stop(move |_| {
        if let Some(d) = gs.dismiss.get_untracked() {
            (d)();
        }
        let o = save_origin.get_untracked();
        let sz = 16.0; // the DOWNLOAD glyph size above
        gs.popup_width.set(GRID_COPY_MENU_W);
        gs.popup_anchor
            .set(Some(PopupAnchor::BelowIcon(o.x, o.x + sz, o.y + sz)));
        gs.popup.set(Some(
            ExportFormat::ALL
                .iter()
                .map(|&f| MenuEntry::action(f.label(), move || save_export(gs, f)))
                .collect(),
        ));
    })
    .on_event_cont(EventListener::PointerEnter, move |_| save_hov.set(true))
    .on_event_cont(EventListener::PointerLeave, move |_| save_hov.set(false))
    .on_event_stop(EventListener::PointerDown, |_| {})
    .style(|s| {
        s.items_center()
            .padding_vert(3.0)
            .padding_horiz(5.0)
            .cursor(CursorStyle::Default)
    });

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
            container(
                icons::icon(icons::SPARKLES, 16.0)
                    .on_move(move |p| ai_origin.set(p))
                    .style(move |s| {
                        // Dimmed + inert while a request is in flight.
                        let c = if gs.ai_busy.get() {
                            theme::text_muted().multiply_alpha(0.3)
                        } else if ai_hov.get() {
                            theme::text()
                        } else {
                            theme::text_muted()
                        };
                        s.color(c).flex_shrink(0.0_f32)
                    }),
            )
            .on_click_stop(move |_| {
                if gs.ai_busy.get_untracked() {
                    return; // a generation is already running
                }
                // Mutually exclusive with the other toolbar/schema menus.
                if let Some(d) = gs.dismiss.get_untracked() {
                    (d)();
                }
                let o = ai_origin.get_untracked();
                let sz = 16.0; // the SPARKLES glyph size above
                gs.popup_width.set(GRID_COPY_MENU_W);
                gs.popup_anchor
                    .set(Some(PopupAnchor::BelowIcon(o.x, o.x + sz, o.y + sz)));
                // AI Fill Value targets the active cell — enabled only when an
                // editable cell is selected (a read-only/expression cell can't be
                // filled).
                let fill_enabled = gs
                    .active
                    .get_untracked()
                    .map(|(_, ci)| gs.edit_model.get_untracked().editable(ci))
                    .unwrap_or(false);
                gs.popup.set(Some(vec![
                    MenuEntry::action_icon(
                        "AI Fill Value",
                        (icons::SPARKLES, theme::key_foreign),
                        move || ai_fill_value(gs),
                    )
                    .disabled(!fill_enabled),
                    MenuEntry::action_icon(
                        "AI Insert Row",
                        (icons::SPARKLES, theme::key_foreign),
                        move || ai_insert_row(gs),
                    ),
                    MenuEntry::action_icon(
                        "AI Seed Table…",
                        (icons::SPARKLES, theme::key_foreign),
                        move || open_seed_popover(gs),
                    ),
                ]));
            })
            .on_event_cont(EventListener::PointerEnter, move |_| ai_hov.set(true))
            .on_event_cont(EventListener::PointerLeave, move |_| ai_hov.set(false))
            .on_event_stop(EventListener::PointerDown, |_| {})
            .style(|s| {
                s.items_center()
                    .padding_vert(3.0)
                    .padding_horiz(5.0)
                    .cursor(CursorStyle::Default)
            })
            .into_any()
        },
    );

    // The icon cluster — 3px between icons (on top of each icon's padded hitbox),
    // separators pushed further out by their own margin:
    // [commit ✓][discard ✗] │ [＋][－][clone] │ [✦ AI][copy].
    let icons_cluster = h_stack((commit_ctrl, row_actions, ai_menu, copy_menu, save_menu))
        .style(|s| s.items_center().flex_row().gap(3.0));

    h_stack((
        stats,
        arena_note,
        caveat,
        empty().style(|s| s.flex_grow(1.0_f32)),
        icons_cluster,
    ))
    .style(|s| {
        // Fixed height + centered so the commit control appearing/leaving never
        // nudges the grid up or down.
        s.width_full()
            .flex_row()
            .items_center()
            .gap(6.0)
            .height(28.0)
            .flex_shrink(0.0_f32)
            .padding_left(12.0)
            // Less right padding than left: the copy icon carries its own 5px hitbox
            // padding, so 7 + 5 lands its glyph ~12px from the edge (matching the
            // left inset) instead of too far in.
            .padding_right(7.0)
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

/// Row-number gutter cell (frozen). Clicking selects the whole display row. A
/// pending new row shows a `*` marker instead of a number.
fn gutter_cell(gs: GridState, pos: usize, ncols: usize, pending: Option<usize>) -> impl IntoView {
    let label = if pending.is_some() {
        "*".to_string()
    } else {
        format!("{}", pos + 1)
    };
    container(text(label).style(|s| s.font_size(theme::FONT_LABEL).color(theme::text_faint())))
        .on_click_stop(move |_| {
            gs.dismiss_overlays();
            gs.anchor.set(Some((pos, 0)));
            gs.active.set(Some((pos, ncols.saturating_sub(1))));
            if let Some(f) = gs.focus_id.get_untracked() {
                f.request_focus();
            }
        })
        .style(move |s| {
            let in_sel = matches!(gs.bounds(), Some((r0, _, r1, _)) if pos >= r0 && pos <= r1);
            let s = s
                .width(GUTTER_W)
                .height(ROW_H)
                .flex_shrink(0.0_f32)
                .items_center()
                .justify_end()
                .padding_horiz(8.0)
                .border_right(1.0)
                .border_color(theme::border());
            if in_sel {
                s.background(theme::accent().multiply_alpha(0.12))
            } else {
                s.background(theme::bg_header_row())
            }
        })
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
                .height(ROW_H)
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
            let mut kids: Vec<AnyView> = vec![col_spacer(w.left_pad, ROW_H).into_any()];
            for k in w.start..w.end {
                kids.push(cell_at(gs, pos, data_idx, cols[k], pending).into_any());
            }
            kids.push(col_spacer(w.right_pad, ROW_H).into_any());
            h_stack_from_iter(kids)
                .style(move |s| zebra_bg(s.flex_row().height(ROW_H).items_center(), pos))
                .into_any()
        },
    )
    .style(|s| s.height(ROW_H))
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
        let s = s.font_size(theme::FONT_LABEL).font_bold();
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
                    .margin_left(7.0)
                    .flex_shrink(0.0_f32)
            })
            .into_any()
    } else {
        empty()
            .style(|s| s.height(14.0).width(0.0).flex_shrink(0.0_f32))
            .into_any()
    };
    let name_row = h_stack((name_line, trailing)).style(|s| s.items_center());
    // SQL type, nudged 2px lower for a touch more breathing room under the name.
    let type_line =
        text(type_name).style(|s| s.font_size(11.0).color(theme::text_faint()).margin_top(2.0));
    let label = v_stack((name_row, type_line)).style(move |s| {
        let s = s
            .flex_col()
            .justify_center()
            .gap(1.0)
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
                .gap(8.0)
                .padding_left(8.0)
                .padding_right(10.0)
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
                    s.padding_left(10.0)
                        .padding_right(GRID_NUM_PAD_RIGHT)
                        .justify_end()
                } else {
                    s.padding_horiz(10.0).justify_start()
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
            gs.popup.set(Some(vec![
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
                MenuEntry::Separator,
                MenuEntry::action_icon(
                    "AI Summary",
                    (icons::SPARKLES, theme::key_foreign),
                    move || {
                        if let Some(s) = &sum {
                            (s)(msg.clone());
                        }
                    },
                ),
            ]));
        })
        .style(move |s| {
            // `with`, not `get`: `get` clones the whole widths `Vec` to read one
            // slot, and this closure re-runs for every visible header on any
            // selection change.
            let w = gs.widths.with(|ws| ws.get(ci).copied().unwrap_or(CELL_W));
            // Highlight the header when its column is within the cell selection.
            let col_sel = matches!(gs.bounds(), Some((_, c0, _, c1)) if ci >= c0 && ci <= c1);
            let formatted = gs
                .formats
                .with(|f| f.get(ci).map(|x| *x != ColumnFormat::None).unwrap_or(false));
            let s = s.width(w).height(GRID_HEADER_H).flex_shrink(0.0_f32);
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
                    return floem::views::text_input(gs.edit_buf)
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
                                    Key::Named(NamedKey::Escape) => {
                                        // Discard: just close the editor.
                                        gs.edit_cell.set(None);
                                        refocus_grid(gs);
                                        return EventPropagation::Stop;
                                    }
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
                            if gs.edit_cell.get_untracked() == Some((i, ci)) {
                                gs.edit_cell.set(None);
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
                                .font_size(theme::FONT_BODY)
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
                                let w = gs.widths.with(|ws| ws.get(ci).copied().unwrap_or(CELL_W));
                                let text_px = gs.edit_buf.with(|b| measure_text_px(b));
                                s.padding_left(numeric_edit_pad_left(w, text_px))
                            } else {
                                s
                            }
                        })
                        .into_any();
                }
                let edited = staged.is_some();
                // A pending new row's unset editable cell shows a placeholder for
                // what it'll do if left blank. `<required>` (NOT NULL, no default)
                // is tinted with the error colour — leaving it blank fails the
                // INSERT; `<auto>` / `<default>` are faint (the server fills them).
                let placeholder = !edited && pending.is_some() && (col_editable || auto_inc);
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
                        let s = s.font_size(theme::FONT_BODY);
                        if edited {
                            // Staged edit: white text over the green cell fill.
                            s.color(floem::peniko::Color::WHITE)
                        } else if is_null || placeholder {
                            // NULL originals + all pending-row placeholders
                            // (`<auto>`/`<required>`/`<null>`/`<default>`) render faint.
                            s.color(theme::text_faint())
                                .font_style(floem::text::Style::Italic)
                        } else if is_fk {
                            // Foreign-key value: underline it (in the text colour) as
                            // a "followable relation" affordance (Ctrl-click follows).
                            s.color(theme::text())
                                .border_bottom(1.0)
                                .border_color(theme::text())
                        } else {
                            s.color(theme::text())
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
                    // Single-cell selection only — no drag-select / shift-extend
                    // (the grid has no multi-cell actions).
                    set_active(gs, i, ci, false);
                    if let Some(fid) = gs.focus_id.get_untracked() {
                        fid.request_focus();
                    }
                    return EventPropagation::Stop;
                }
            }
            EventPropagation::Continue
        })
        .on_double_click_stop(move |_| {
            // Double-click edits an editable cell; on a read-only cell it does
            // nothing (viewing is via the right-click menu's View item).
            if gs.edit_model.get_untracked().editable(ci) {
                start_edit(gs, i, ci);
            } else {
                gs.active.set(Some((i, ci)));
                gs.anchor.set(Some((i, ci)));
            }
        })
        // Right-click → View · Edit · Copy · Set to NULL · AI Summary.
        .on_secondary_click_stop(move |_| {
            gs.active.set(Some((i, ci)));
            gs.anchor.set(Some((i, ci)));
            let rs = gs.rs.get_untracked();
            // Effective value: staged text/NULL, else the original (real rows only —
            // a pending new row has no original, so unset cells are empty).
            let staged_here_val: Option<Option<String>> = match pending {
                Some(p) => gs
                    .new_rows
                    .with_untracked(|rows| rows.get(p).and_then(|r| r.get(&ci).cloned())),
                None => gs.dirty.with_untracked(|d| d.get(&dkey).cloned()),
            };
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
                entries.push(MenuEntry::action("Edit Field", move || {
                    start_edit(gs, i, ci)
                }));
            }
            if pending.is_none() {
                entries.push(MenuEntry::action("Edit Row", move || {
                    open_edit_row(gs, data_idx)
                }));
            }
            entries.push(MenuEntry::action("Copy", move || {
                let _ = floem::Clipboard::set_contents(v_copy.clone());
            }));
            // Only when this column shows a formatted (non-raw) value.
            if fmt != ColumnFormat::None {
                entries.push(MenuEntry::action("Copy formatted", move || {
                    let _ = floem::Clipboard::set_contents(formatted_val.clone());
                }));
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
            // Set off from the row actions above it — asking about a value is a
            // different kind of act from editing or deleting one.
            entries.push(MenuEntry::Separator);
            entries.push(MenuEntry::action_icon(
                "AI Summary",
                (icons::SPARKLES, theme::key_foreign),
                move || {
                    if let Some(s) = &sum {
                        (s)(msg.clone());
                    }
                },
            ));
            gs.popup_anchor.set(None); // right-click → open at the cursor
            gs.popup.set(Some(entries));
        })
        .style(move |s| {
            // `with`, not `get` — see the header closure. This one runs for every
            // *cell* in the viewport on every selection change, so a drag-select
            // over a wide result cloned the widths `Vec` hundreds of times per
            // pointer move.
            let w = gs.widths.with(|ws| ws.get(ci).copied().unwrap_or(CELL_W));
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
            let s = s.width(w).height(ROW_H).flex_shrink(0.0_f32).items_center();
            // Right-aligned numeric cells get extra right padding (matching the
            // header) so the value clears the edge/border; text cells stay at 10px.
            let s = if numeric {
                s.padding_left(GRID_PAD_H)
                    .padding_right(GRID_NUM_PAD_RIGHT)
                    .justify_end()
            } else {
                s.padding_horiz(GRID_PAD_H).justify_start()
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
/// is padded rather than aligned (see the CLAUDE.md note).
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
    let content = w - GRID_PAD_H - GRID_NUM_PAD_RIGHT - GRID_CELL_DIVIDER;
    (content - text_px - SLACK).max(0.0)
}

/// Compact row-count label: `1000 → 1k`, `1250 → 1.25k`, `1_000_000 → 1m`.
/// Up to two decimals, trailing zeros trimmed. Under 1000 stays exact.
fn human_count(n: usize) -> String {
    let f = n as f64;
    let (val, suffix) = if f >= 1e9 {
        (f / 1e9, "b")
    } else if f >= 1e6 {
        (f / 1e6, "m")
    } else if f >= 1e3 {
        (f / 1e3, "k")
    } else {
        return n.to_string();
    };
    let s = format!("{val:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{s}{suffix}")
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

    // Single-column result of the given cells, so `compute_order` sorts column 0.
    fn rs_col(ty: &str, cells: Vec<Value>) -> ResultSet {
        ResultSet::from_rows(
            vec![col("c", ty)],
            cells.into_iter().map(|v| vec![v]).collect(),
        )
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
        w - GRID_PAD_H - GRID_NUM_PAD_RIGHT - GRID_CELL_DIVIDER - pad_left
    }

    #[test]
    fn a_numeric_edit_never_asks_for_more_width_than_the_cell_can_give() {
        // The regression: the padding was computed against `w - 20` while the cell
        // really offers `w - 10 - 14 - 1`, so the text node came out 5px short of
        // the text every time. Clipping is glyph-quantised, so "99" showed "9" and
        // "1" showed nothing at all.
        for w in [MIN_COL_W, 60.0, 100.0, CELL_W, 420.0] {
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
        assert_eq!(numeric_edit_pad_left(CELL_W, 400.0), 0.0);
        assert_eq!(numeric_edit_pad_left(MIN_COL_W, 60.0), 0.0);
    }

    #[test]
    fn a_short_value_is_pushed_right_to_meet_the_display_alignment() {
        // 190 - 10 - 14 - 1 = 165 of content; a 14px value sits at the right edge
        // (less the 2px of slack that keeps floem's clip off the boundary).
        assert_eq!(numeric_edit_pad_left(CELL_W, 14.0), 149.0);
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
    // The invariant CLAUDE.md states for the data pane: `gs.widths` stays
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

    #[test]
    fn pending_cell_text_reads_staged_values() {
        let mut row: HashMap<usize, Option<String>> = HashMap::new();
        row.insert(0, Some("hello".to_string()));
        row.insert(1, None);
        // Staged text, staged SQL NULL (rendered as the cell does), and an unset
        // cell — which has no value yet, only the server default.
        assert_eq!(pending_cell_text(Some(&row), 0), "hello");
        assert_eq!(pending_cell_text(Some(&row), 1), "NULL");
        assert_eq!(pending_cell_text(Some(&row), 2), "");
        // A pending row that no longer exists copies blank rather than panicking.
        assert_eq!(pending_cell_text(None, 0), "");
    }

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
