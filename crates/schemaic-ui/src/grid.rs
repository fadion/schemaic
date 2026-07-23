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

use schemaic_core::connection::Connection;
use schemaic_core::edit::{EditModel, analyze_edit, refetch_template};
use schemaic_core::format::{self, ColumnFormat, ColumnFormatRule};
use schemaic_core::model::{
    CommitDone, GridWrite, QueryState, RefetchRequest, RefetchRow, ResultSet, RowDelete, RowEdit,
    RowInsert, Value,
};
use schemaic_core::schema::SchemaState;
use schemaic_core::text_ops::contains_ignore_ascii_case;

use crate::consts::*;
use crate::widgets::{
    MenuEntry, autohide_state, centered_msg, measure_text_px, shift_hscroll, thin_scroll,
    toolbar_icon, verb_spinner,
};
use crate::{ConnNode, FieldCfg, bg_transparent, edit_field, icons, theme};

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
    // Key the container on the *phase* (a deduped Memo), not the whole QueryState.
    // A commit splice replaces the loaded Arc (Loaded→Loaded) — the phase is
    // unchanged, so the grid is NOT rebuilt and scroll/selection survive. A real
    // query still goes …→Running→Loaded, changing the phase and rebuilding fresh.
    let phase = create_memo(move |_| results.with(phase_of));
    // Splice sink handed to the grid: replace the canonical result set in place.
    // The phase Memo dedups, so this Loaded→Loaded set doesn't rebuild the grid;
    // it only refreshes the canonical for a later rebuild (tab switch away/back).
    let sync: Rc<dyn Fn(Arc<ResultSet>)> =
        Rc::new(move |rs: Arc<ResultSet>| results.set(QueryState::Loaded(rs)));
    dyn_container(
        move || phase.get(),
        move |ph| match ph {
            Phase::Idle => {
                centered_msg("Run a query  (Ctrl+Enter)", theme::text_muted()).into_any()
            }
            Phase::Running => running_view(cancel.clone()).into_any(),
            // The error text now lives in the editor's error bar (with View /
            // AI Fix), so Results just notes the failure.
            Phase::Failed => centered_msg("Query failed.", theme::text_dim()).into_any(),
            Phase::Cancelled => centered_msg("Query cancelled.", theme::text_dim()).into_any(),
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

/// A stable permutation of row indices for the given sort (identity when
/// `None`). Nulls sort last; numeric columns compare numerically.
fn compute_order(rs: &ResultSet, sort: SortState) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..rs.rows.len()).collect();
    if let Some((c, asc)) = sort {
        idx.sort_by(|&a, &b| {
            let o = match (
                rs.rows.get(a).and_then(|r| r.get(c)),
                rs.rows.get(b).and_then(|r| r.get(c)),
            ) {
                (Some(x), Some(y)) => cmp_value(x, y),
                _ => Ordering::Equal,
            };
            if asc { o } else { o.reverse() }
        });
    }
    idx
}

fn value_num(v: &Value) -> Option<f64> {
    match v {
        Value::Int(i) => Some(*i as f64),
        Value::UInt(u) => Some(*u as f64),
        Value::Float(f) => Some(*f),
        _ => None,
    }
}

fn cmp_value(a: &Value, b: &Value) -> Ordering {
    match (a, b) {
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Null, _) => Ordering::Greater,
        (_, Value::Null) => Ordering::Less,
        _ => match (value_num(a), value_num(b)) {
            (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
            _ => a.display().cmp(&b.display()),
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
                theme::text_dim(),
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
    viewer: RwSignal<bool>,
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
    /// Last commit error, shown in the toolbar until the next edit/commit.
    commit_err: RwSignal<Option<String>>,
    /// Ui-level popup-menu signal, for the header/cell right-click menus.
    popup: RwSignal<Option<Vec<MenuEntry>>>,
    /// Anchor for the popup: `Some((icon_left, icon_right, icon_bottom, w))` opens
    /// it under a toolbar icon (the Copy dropdown); `None` opens at the cursor.
    popup_anchor: RwSignal<Option<(f64, f64, f64, f64)>>,
    /// The result's source `(database, table)` — for the cell "AI Summary" context.
    source: RwSignal<Option<(String, String)>>,
    /// Callbacks wrapped in signals so `GridState` stays `Copy`. `summarize`
    /// reveals the AI panel + sends a message; `dismiss` closes any open menu;
    /// `commit` executes staged edits.
    summarize: RwSignal<Option<SummarizeFn>>,
    dismiss: RwSignal<Option<Rc<dyn Fn()>>>,
    commit: RwSignal<Option<crate::CommitFn>>,
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
    /// App-wide formatter-rule store (upserted + persisted on a menu choice).
    fmt_rules: RwSignal<Vec<ColumnFormatRule>>,
    /// Persist the formatter rules (wrapped so `GridState` stays `Copy`).
    save_formats: RwSignal<Option<Rc<dyn Fn()>>>,
    /// In-grid find (Ctrl+F): the bar's open state and its query. Match counts
    /// live in `GridCtx` (written by `grid_view`, read by the panel-level bar).
    find_open: RwSignal<bool>,
    find_query: RwSignal<String>,
}

impl GridState {
    fn new(rs: Arc<ResultSet>, gctx: &GridCtx) -> Self {
        let widths = init_widths(&rs);
        // Seed each column's display formatter from the persisted rules, keyed by
        // (conn_id, source table, column). Columns of an unsourced query (or with
        // no saved rule) start raw.
        let conn = gctx.conn_id.get_untracked();
        let src = gctx.source.get_untracked();
        let formats: Vec<ColumnFormat> = (0..rs.col_count())
            .map(|ci| {
                let (name, ty) = rs
                    .columns
                    .get(ci)
                    .map(|c| (c.name.as_str(), c.type_name.as_str()))
                    .unwrap_or(("", ""));
                // An explicit saved rule wins; otherwise fall back to the name/type
                // smart default (e.g. an int `*_at` column → Timestamp).
                let saved = match &src {
                    Some((db, table)) => gctx
                        .formats
                        .with_untracked(|rules| format::lookup(rules, conn, db, table, name)),
                    None => None,
                };
                saved.unwrap_or_else(|| format::smart_default(name, ty))
            })
            .collect();
        GridState {
            rs: RwSignal::new(rs),
            order: RwSignal::new(Arc::new(Vec::new())),
            widths: RwSignal::new(widths),
            active: RwSignal::new(None),
            anchor: RwSignal::new(None),
            viewer: RwSignal::new(false),
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
            // Shared with the panel-level error bar (rendered in `results_section`).
            commit_err: gctx.commit_err,
            popup: gctx.popup,
            popup_anchor: gctx.popup_anchor,
            source: gctx.source,
            summarize: RwSignal::new(Some(gctx.summarize.clone())),
            dismiss: RwSignal::new(Some(gctx.dismiss.clone())),
            commit: RwSignal::new(Some(gctx.commit.clone())),
            sync_canonical: RwSignal::new(gctx.sync_canonical.clone()),
            formats: RwSignal::new(formats),
            conn_id: gctx.conn_id,
            fmt_rules: gctx.formats,
            save_formats: RwSignal::new(Some(gctx.save_formats.clone())),
            // Shared with the find bar (rendered up at the RESULTS-panel level).
            find_open: gctx.find_open,
            find_query: gctx.find_query,
        }
    }

    /// Stage a value for data-row `di`, column `ci` (`None` = SQL NULL). If it
    /// equals the original the entry is dropped (no longer dirty).
    fn stage(&self, di: usize, ci: usize, val: Option<String>) {
        // Original as `Option<String>`: NULL → `None`.
        let orig = self
            .rs
            .get_untracked()
            .rows
            .get(di)
            .and_then(|r| r.get(ci))
            .map(|v| if v.is_null() { None } else { Some(v.display()) });
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
        if let Some(row) = rs.rows.get(data_idx) {
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
                if let Some(v) = row.get(ci) {
                    map.insert(ci, if v.is_null() { None } else { Some(v.display()) });
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
        let real_col = |ci: usize| -> String {
            rs.columns
                .get(ci)
                .and_then(|c| c.origin.as_ref())
                .map(|o| o.column.clone())
                .unwrap_or_default()
        };
        let mut edits = Vec::with_capacity(groups.len());
        for ((ti, di), mut sets) in groups {
            let Some(tbl) = model.table(ti) else { continue };
            sets.sort_by_key(|(ci, _)| *ci); // stable SET-clause order
            let set: Vec<(String, Option<String>)> =
                sets.into_iter().map(|(ci, v)| (real_col(ci), v)).collect();
            let key: Vec<(String, Value)> = tbl
                .key_cols
                .iter()
                .map(|&kci| {
                    let val = rs
                        .rows
                        .get(di)
                        .and_then(|r| r.get(kci))
                        .cloned()
                        .unwrap_or(Value::Null);
                    (real_col(kci), val)
                })
                .collect();
            edits.push(RowEdit {
                database: tbl.database.clone(),
                table: tbl.table.clone(),
                set,
                key,
            });
        }
        edits
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
        let real_col = |ci: usize| -> String {
            rs.columns
                .get(ci)
                .and_then(|c| c.origin.as_ref())
                .map(|o| o.column.clone())
                .unwrap_or_default()
        };
        let mut dis: Vec<usize> = del.into_iter().collect();
        dis.sort_unstable(); // deterministic DELETE order
        dis.into_iter()
            .filter_map(|di| {
                let row = rs.rows.get(di)?;
                let key: Vec<(String, Value)> = tbl
                    .key_cols
                    .iter()
                    .map(|&kci| (real_col(kci), row.get(kci).cloned().unwrap_or(Value::Null)))
                    .collect();
                Some(RowDelete {
                    database: tbl.database.clone(),
                    table: tbl.table.clone(),
                    key,
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
                let key = template
                    .key_cols
                    .iter()
                    .map(|&kci| match dirty.get(&(di, kci)) {
                        // The key column itself was edited → use the new value.
                        Some(Some(t)) => Value::Str(t.clone()),
                        Some(None) => Value::Null,
                        None => rs
                            .rows
                            .get(di)
                            .and_then(|r| r.get(kci))
                            .cloned()
                            .unwrap_or(Value::Null),
                    })
                    .collect();
                RefetchRow { data_row: di, key }
            })
            .collect();
        Some(RefetchRequest { template, rows })
    }

    /// Splice re-fetched rows into the live result set in place — `(data_row, new
    /// cells)`, cells aligned to the result columns — then clear the staged edits.
    /// Updates both the grid's live `rs` (cells re-read reactively) and the tab's
    /// canonical result set (so a later rebuild is fresh). No rebuild, so scroll /
    /// selection / widths survive.
    fn apply_splice(&self, rows: Vec<(usize, Vec<Value>)>) {
        if !rows.is_empty() {
            self.rs.update(|arc| {
                let rs = Arc::make_mut(arc);
                for (di, cells) in &rows {
                    if let Some(row) = rs.rows.get_mut(*di) {
                        if cells.len() == row.len() {
                            *row = cells.clone();
                        } else {
                            // Defensive: partial overwrite on a length mismatch.
                            for (i, v) in cells.iter().enumerate() {
                                if let Some(slot) = row.get_mut(i) {
                                    *slot = v.clone();
                                }
                            }
                        }
                    }
                }
            });
            if let Some(sync) = self.sync_canonical.get_untracked() {
                (sync)(self.rs.get_untracked());
            }
        }
        // Edits are now persisted and reflected as originals.
        self.dirty.update(|d| d.clear());
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
fn init_widths(rs: &ResultSet) -> Vec<f64> {
    let sample = rs.rows.len().min(200);
    rs.columns
        .iter()
        .enumerate()
        .map(|(ci, col)| {
            let mut chars = col.name.chars().count() + 3; // room for the sort arrow
            chars = chars.max(col.type_name.chars().count());
            for r in rs.rows.iter().take(sample) {
                if let Some(v) = r.get(ci) {
                    chars = chars.max(v.display().chars().count().min(60));
                }
            }
            (chars as f64 * GRID_CHAR_W + 22.0).clamp(MIN_COL_W, MAX_COL_W_INIT)
        })
        .collect()
}

/// Auto-fit width for one column over the whole result (double-click a divider).
fn autofit_width(rs: &ResultSet, ci: usize) -> f64 {
    let mut chars = rs
        .columns
        .get(ci)
        .map(|c| c.name.chars().count() + 3)
        .unwrap_or(6);
    for r in &rs.rows {
        if let Some(v) = r.get(ci) {
            chars = chars.max(v.display().chars().count().min(140));
        }
    }
    (chars as f64 * GRID_CHAR_W + 22.0).clamp(MIN_COL_W, 900.0)
}

fn cell_in(bounds: Option<(usize, usize, usize, usize)>, i: usize, ci: usize) -> bool {
    matches!(bounds, Some((r0, c0, r1, c1)) if i >= r0 && i <= r1 && ci >= c0 && ci <= c1)
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
#[derive(Clone, PartialEq)]
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
    let mut out = String::new();
    for i in r0..=r1 {
        if i > r0 {
            out.push('\n');
        }
        let di = order.get(i).copied().unwrap_or(i);
        for ci in c0..=c1 {
            if ci > c0 {
                out.push('\t');
            }
            let s = rs
                .rows
                .get(di)
                .and_then(|r| r.get(ci))
                .map(|v| v.display())
                .unwrap_or_default();
            out.push_str(&s);
        }
    }
    let _ = floem::Clipboard::set_contents(out);
}

// Thin clipboard-facing wrappers over `schemaic_core::export` — unwrap the
// grid's live `ResultSet` + display order and delegate to the pure functions.
fn export_json(gs: GridState) -> String {
    schemaic_core::export::export_json(
        gs.rs.get_untracked().as_ref(),
        gs.order.get_untracked().as_slice(),
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

fn export_csv(gs: GridState) -> String {
    schemaic_core::export::export_csv(
        gs.rs.get_untracked().as_ref(),
        gs.order.get_untracked().as_slice(),
    )
}

fn export_inserts(gs: GridState) -> String {
    let rs = gs.rs.get_untracked();
    let order = gs.order.get_untracked();
    let source = gs.source.get_untracked();
    schemaic_core::export::export_inserts(
        rs.as_ref(),
        order.as_slice(),
        source.as_ref().map(|(d, t)| (d.as_str(), t.as_str())),
    )
}

/// Pretty-print a cell value if it's JSON, else `None`.
fn pretty_json(s: &str) -> Option<String> {
    let t = s.trim_start();
    if !(t.starts_with('{') || t.starts_with('[')) {
        return None;
    }
    serde_json::from_str::<serde_json::Value>(s)
        .ok()
        .and_then(|v| serde_json::to_string_pretty(&v).ok())
}

/// A draggable column-resize divider pinned to the right edge of a header cell.
/// Drag adjusts that column's width; double-click auto-fits to content.
fn col_resize_handle(gs: GridState, ci: usize) -> impl IntoView {
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
        let rs = gs.rs.get_untracked();
        let w = autofit_width(&rs, ci);
        gs.widths.update(|ws| {
            if let Some(x) = ws.get_mut(ci) {
                *x = w;
            }
        });
    })
}

/// Bottom detail strip showing the focused cell's full value (pretty-printed if
/// JSON) in a read-only, word-wrapped, auto-growing field — small for short
/// values, growing up to `max_rows` (the results-panel height) then scrolling.
/// Shown only while `viewer` is on; follows the active cell live.
fn value_viewer(gs: GridState, max_rows: RwSignal<usize>) -> impl IntoView {
    let text_sig = RwSignal::new(String::new());
    let raw_sig = RwSignal::new(String::new());
    let title_sig = RwSignal::new("No cell selected".to_string());

    // Mirror the focused cell into the field whenever it (or the result) changes
    // while the viewer is open.
    create_effect(move |_| {
        if !gs.viewer.get() {
            return;
        }
        let rs = gs.rs.get();
        let order = gs.order.get();
        match gs.active.get() {
            Some((i, ci)) => {
                let di = order.get(i).copied().unwrap_or(i);
                let col = rs.columns.get(ci);
                let name = col.map(|c| c.name.clone()).unwrap_or_default();
                let ty = col.map(|c| c.type_name.clone()).unwrap_or_default();
                let raw = rs
                    .rows
                    .get(di)
                    .and_then(|r| r.get(ci))
                    .map(|v| v.display())
                    .unwrap_or_default();
                title_sig.set(format!("{name}  ·  {ty}  ·  Row {}", i + 1));
                text_sig.set(pretty_json(&raw).unwrap_or_else(|| raw.clone()));
                raw_sig.set(raw);
            }
            None => {
                title_sig.set("No cell selected".to_string());
                text_sig.set(String::new());
                raw_sig.set(String::new());
            }
        }
    });

    let copy_btn = container(icons::icon(icons::COPY, 14.0))
        .on_click_stop(move |_| {
            let _ = floem::Clipboard::set_contents(raw_sig.get_untracked());
        })
        .style(|s| {
            s.padding(4.0)
                .border_radius(4.0)
                .color(theme::text_dim())
                .hover(|s| s.background(theme::row_hover()).color(theme::text()))
        });
    let close_btn = container(icons::icon(icons::X, 14.0))
        .on_click_stop(move |_| gs.viewer.set(false))
        .style(|s| {
            s.padding(4.0)
                .border_radius(4.0)
                .color(theme::text_dim())
                .hover(|s| s.background(theme::row_hover()).color(theme::text()))
        });
    let head = h_stack((
        dyn_container(
            move || title_sig.get(),
            move |t| {
                text(t)
                    .style(|s| s.font_size(theme::FONT_LABEL).color(theme::text_dim()))
                    .into_any()
            },
        ),
        empty().style(|s| s.flex_grow(1.0_f32)),
        copy_btn,
        close_btn,
    ))
    .style(|s| {
        s.width_full()
            .items_center()
            .gap(4.0)
            .padding_horiz(10.0)
            .height(26.0)
            .flex_shrink(0.0_f32)
    });

    // Read-only, wrapping, auto-growing field (like the message box) — capped at
    // `max_rows` (the panel height). Transparent border/bg so it reads as text.
    let field = edit_field(
        text_sig,
        FieldCfg {
            multiline: true,
            read_only: true,
            max_rows: Some(max_rows),
            background: theme::bg_panel,
            border_color: Some(bg_transparent),
            ..Default::default()
        },
    )
    // Bound the width so the editor wraps (its box is content-sized otherwise).
    .style(|s| s.width_full());

    v_stack((head, field)).style(move |s| {
        let s = s
            .width_full()
            .flex_shrink(0.0_f32)
            .flex_col()
            .border_top(1.0)
            .border_color(theme::border())
            .background(theme::bg_panel());
        if gs.viewer.get() { s } else { s.hide() }
    })
}

/// A result column's key role in its source table (drives the header key icon).
#[derive(Clone, Copy, PartialEq)]
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

/// Per-column key roles for the result's source table, keyed by column name.
/// Empty when the tab wasn't opened from a table or its schema isn't loaded yet.
/// Primary keys win; single-column indexes are Foreign (if they back an FK) else
/// Index. Multi-column indexes are ignored (only single-column ones get a marker).
fn column_key_map(
    source: RwSignal<Option<(String, String)>>,
    db_nodes: RwSignal<Vec<ConnNode>>,
) -> HashMap<String, ColKey> {
    let mut map = HashMap::new();
    let Some((db, table)) = source.get_untracked() else {
        return map;
    };
    let nodes = db_nodes.get_untracked();
    let Some(node) = nodes.iter().find(|n| n.database == db) else {
        return map;
    };
    let SchemaState::Loaded(schema) = node.schema.get_untracked() else {
        return map;
    };
    let Some(t) = schema.tables.iter().find(|t| t.name == table) else {
        return map;
    };
    for c in &t.columns {
        if c.primary_key {
            map.insert(c.name.clone(), ColKey::Primary);
        }
    }
    for ix in &t.indexes {
        if ix.is_primary() || ix.columns.len() != 1 {
            continue;
        }
        let col = &ix.columns[0];
        if map.get(col) == Some(&ColKey::Primary) {
            continue;
        }
        if ix.foreign {
            map.insert(col.clone(), ColKey::Foreign); // FK wins over a plain index
        } else {
            map.entry(col.clone()).or_insert(ColKey::Index);
        }
    }
    map
}

/// Bundle of app-provided context the results grid needs, threaded from
/// `query_pane` down through the results-view chain.
#[derive(Clone)]
pub(crate) struct GridCtx {
    /// The active tab's source `(database, table)`, for key-icon lookup.
    pub(crate) source: RwSignal<Option<(String, String)>>,
    pub(crate) db_nodes: RwSignal<Vec<ConnNode>>,
    /// Saved connections + the active id, for the identity-colour rule drawn at
    /// the table's top edge (the "prominent colour" setting).
    pub(crate) connections: RwSignal<Vec<Connection>>,
    pub(crate) active_conn: RwSignal<u64>,
    /// Ui-level popup-menu signal (header/cell right-click menus).
    pub(crate) popup: RwSignal<Option<Vec<MenuEntry>>>,
    /// Popup anchor signal (icon-anchored toolbar dropdowns vs cursor menus).
    pub(crate) popup_anchor: RwSignal<Option<(f64, f64, f64, f64)>>,
    /// Reveal the AI panel + send a message (used for the cell "AI Summary").
    pub(crate) summarize: Rc<dyn Fn(String)>,
    /// Close any open popup / schema context menu (so a grid click dismisses them
    /// — grid cells consume the pointer-down, so the root handler never fires).
    pub(crate) dismiss: Rc<dyn Fn()>,
    /// Execute staged edits transactionally. Arg 2 is an optional re-fetch request
    /// (present ⇒ splice the edited rows instead of full-re-running); arg 3 is the
    /// completion callback, invoked on the UI thread with the [`CommitDone`].
    pub(crate) commit: crate::CommitFn,
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
    /// Last commit error (grid write-back), shown in a bottom error bar at the
    /// panel level (like the find bar at the top). Cleared by the next edit/commit.
    pub(crate) commit_err: RwSignal<Option<String>>,
    /// The workspace error modal (shared with the editor error bar): `error_open`
    /// reveals it; the grid's "View" first sets `error_text` to the full commit
    /// error so the modal shows that instead of the tab's query error.
    pub(crate) error_open: RwSignal<bool>,
    pub(crate) error_text: RwSignal<Option<String>>,
}

/// The grid's commit-error bar, rendered at the RESULTS-panel level so it pins to
/// the panel's bottom edge — same look/position as the editor error bar (the red
/// `reject_bg` fill, rounded, 5px insets, 35px tall). The one-lined message on the
/// left, a right-aligned **View** that opens the full error in the shared modal
/// (via a text override). Absolute → overlays the panel out of flow.
pub(crate) fn grid_error_bar(
    commit_err: RwSignal<Option<String>>,
    error_open: RwSignal<bool>,
    error_text: RwSignal<Option<String>>,
) -> impl IntoView {
    dyn_container(
        move || commit_err.get(),
        move |err| {
            let Some(msg) = err else {
                return empty().into_any();
            };
            // Collapse to a single line (a multi-line server error would spill out
            // the top); the full text stays available in the View modal.
            let one_line = msg.split_whitespace().collect::<Vec<_>>().join(" ");
            let full = msg;
            h_stack((
                text(one_line).style(|s| {
                    s.color(theme::reject_text())
                        .font_size(theme::FONT_BODY)
                        .max_width_pct(80.0)
                        .text_ellipsis()
                        .margin_left(8.0)
                }),
                empty().style(|s| s.flex_grow(1.0_f32)),
                text("View")
                    .on_click_stop(move |_| {
                        error_text.set(Some(full.clone()));
                        error_open.set(true);
                    })
                    .style(|s| {
                        s.color(theme::err_fix_btn())
                            .font_size(theme::FONT_BODY)
                            .margin_right(8.0)
                    }),
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
        },
    )
    .style(move |s| {
        if commit_err.get().is_some() {
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

fn grid_view(rs: Arc<ResultSet>, gctx: GridCtx) -> impl IntoView {
    let ncols = rs.col_count();
    let nrows = rs.row_count();
    // Per-column key roles (snapshot from the source table's schema at build).
    let key_map = Arc::new(column_key_map(gctx.source, gctx.db_nodes));
    let elapsed = rs.elapsed_ms;
    let truncated = rs.truncated;
    // Signals for the identity-colour rule at the table's top edge (below the
    // toolbar), captured before `gctx` fields move into the closures below.
    let (connections, active_conn) = (gctx.connections, gctx.active_conn);

    // Interactive state, created once and shared across sort rebuilds.
    let gs = GridState::new(rs.clone(), &gctx);
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
            analyze_edit(&rs_model, |db, table| {
                db_nodes.with_untracked(|nodes| {
                    nodes.iter().find(|n| n.database == db).and_then(|n| {
                        match n.schema.get_untracked() {
                            SchemaState::Loaded(s) => {
                                s.tables.iter().find(|t| t.name == table).cloned()
                            }
                            _ => None,
                        }
                    })
                })
            })
        };
        gs.edit_model.set(Arc::new(model));
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

    let toolbar = grid_toolbar(gs, nrows, ncols, elapsed, truncated, sort);

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
            let data_header = scroll(data_header_row)
                .scroll_to(move || Some(Point::new(h_off.get(), 0.0)))
                .scroll_style(|s| s.hide_bars(true).propagate_pointer_wheel(true))
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
                        Some(fc) => gs.widths.get().get(fc).copied().unwrap_or(0.0),
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

    // The viewer's auto-grow cap = how many rows fit in the results panel (so it
    // can expand, then scroll), but never taller than ~200px total. Derived from
    // the panel height tracked on the root `on_resize` below (~19px/row).
    let viewer_max = RwSignal::new(6usize);
    // Wrap the grid so a 2px identity-colour rule can pin to its top edge (right
    // below the toolbar) without taking layout space — the "prominent colour"
    // setting. The box inherits the grid's growth so the table still fills.
    let grid_boxed = stack((
        grid,
        crate::conn_edge_border(connections, active_conn, true),
    ))
    .style(|s| {
        s.flex_grow(1.0_f32)
            .flex_basis(0.0)
            .width_full()
            .flex_col()
            .min_height(120.0)
            .min_width(0.0)
    });
    v_stack((toolbar, grid_boxed, value_viewer(gs, viewer_max)))
        .on_resize(move |r| {
            // Cap so the viewer leaves the grid its `min_height` (~120) + toolbar
            // (~26) + the viewer's own header (~26): rows ≈ (panel − 172) / 19.
            // Hard ceiling of 8 rows keeps the whole inspector ≤ 200px
            // (header 26 + border 1 + field 8*19+15 ≈ 194).
            let rows = (((r.height() - 172.0) / 19.0).floor() as i64).clamp(1, 8) as usize;
            if viewer_max.get_untracked() != rows {
                viewer_max.set(rows);
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
                .rows
                .get(di)
                .and_then(|r| r.get(ci))
                .filter(|v| !v.is_null()) // original NULL → edit from empty
                .map(|v| v.display())
                .unwrap_or_default(),
        }
    };
    gs.edit_buf.set(seed);
    gs.edit_cell.set(Some((i, ci)));
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
    let done: Rc<dyn Fn(CommitDone)> = Rc::new(move |outcome| {
        gs.commit_busy.set(false);
        match outcome {
            // Fresh DB values for the edited rows — splice in place, keep scroll.
            CommitDone::Spliced(rows) => gs.apply_splice(rows),
            // The app re-ran the query; the grid is rebuilt fresh, nothing to do.
            CommitDone::FullReran => {}
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

/// Discard all staged changes — cell edits, pending new rows, and pending row
/// deletions (the toolbar ✗) — closing any open in-cell editor.
fn discard_edits(gs: GridState) {
    gs.edit_cell.set(None);
    gs.dirty.update(|d| d.clear());
    gs.new_rows.update(|r| r.clear());
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
        if let Some(v) = rs.rows.get(data).and_then(|r| r.get(ci)) {
            let fmt = formats.get(ci).copied().unwrap_or_default();
            if contains_ignore_ascii_case(&format::apply(fmt, v), &q) {
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
    if q.is_empty() {
        return (Vec::new(), false);
    }
    let rs = gs.rs.get_untracked();
    let order = gs.order.get_untracked();
    let formats = gs.formats.get_untracked();
    let ncols = rs.col_count();
    let mut hits = Vec::new();
    let mut more = false;
    let mut scanned = 0usize;
    'outer: for (dr, &data) in order.iter().enumerate() {
        let row = rs.rows.get(data);
        for ci in 0..ncols {
            if scanned >= FIND_COUNT_CELL_BUDGET {
                more = true;
                break 'outer;
            }
            scanned += 1;
            if let Some(v) = row.and_then(|r| r.get(ci)) {
                let fmt = formats.get(ci).copied().unwrap_or_default();
                if contains_ignore_ascii_case(&format::apply(fmt, v), &q) {
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
    if nrows == 0 || ncols == 0 {
        return EventPropagation::Continue;
    }
    let m = ke.modifiers;
    // Shift+Arrow no longer extends a multi-cell selection — keyboard nav always
    // moves the single active cell. (Mouse drag-select + copy still work.)
    let shift = false;
    let ctrl = m.control() || m.meta();
    let active_opt = gs.active.get_untracked();
    let (r, c) = active_opt.unwrap_or((0, 0));
    let last_r = nrows - 1;
    let last_c = ncols - 1;
    let page = ((gs.vp.get_untracked().height() / ROW_H).floor() as usize).max(1);
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
        Key::Named(NamedKey::ArrowDown) => set_active(gs, (r + 1).min(last_r), c, shift),
        Key::Named(NamedKey::ArrowUp) => set_active(gs, r.saturating_sub(1), c, shift),
        Key::Named(NamedKey::ArrowRight) => set_active(gs, r, (c + 1).min(last_c), shift),
        Key::Named(NamedKey::ArrowLeft) => set_active(gs, r, c.saturating_sub(1), shift),
        Key::Named(NamedKey::Home) => {
            if ctrl {
                set_active(gs, 0, 0, shift)
            } else {
                set_active(gs, r, 0, shift)
            }
        }
        Key::Named(NamedKey::End) => {
            if ctrl {
                set_active(gs, last_r, last_c, shift)
            } else {
                set_active(gs, r, last_c, shift)
            }
        }
        Key::Named(NamedKey::PageDown) => set_active(gs, (r + page).min(last_r), c, shift),
        Key::Named(NamedKey::PageUp) => set_active(gs, r.saturating_sub(page), c, shift),
        Key::Named(NamedKey::Escape) => {
            // Esc closes the find bar first (so it closes from anywhere in the grid,
            // not only when its input is focused), then the value viewer, then the
            // selection.
            if gs.find_open.get_untracked() {
                gs.find_open.set(false);
                gs.find_query.set(String::new());
            } else if gs.viewer.get_untracked() {
                gs.viewer.set(false);
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
fn grid_toolbar(
    gs: GridState,
    nrows: usize,
    ncols: usize,
    elapsed_ms: u128,
    truncated: bool,
    sort: RwSignal<SortState>,
) -> impl IntoView {
    let cap = if truncated { " (capped)" } else { "" };
    let stats = text(format!(
        "{} rows{cap} · {ncols} cols · {elapsed_ms} ms",
        human_count(nrows)
    ))
    .style(|s| s.color(theme::text_dim()).font_size(theme::FONT_LABEL));
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
        // Anchor the panel under the icon (left/right edges + bottom).
        let o = copy_origin.get_untracked();
        let sz = 16.0; // the COPY glyph size above
        gs.popup_anchor
            .set(Some((o.x, o.x + sz, o.y + sz, GRID_COPY_MENU_W)));
        gs.popup.set(Some(vec![
            MenuEntry::action("JSON", move || {
                let _ = floem::Clipboard::set_contents(export_json(gs));
            }),
            MenuEntry::action("CSV", move || {
                let _ = floem::Clipboard::set_contents(export_csv(gs));
            }),
            MenuEntry::action("SQL", move || {
                let _ = floem::Clipboard::set_contents(export_inserts(gs));
            }),
        ]));
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

    // The icon cluster — 3px between icons (on top of each icon's padded hitbox),
    // separators pushed further out by their own margin:
    // [commit ✓][discard ✗] │ [＋][－][clone] │ [copy].
    let icons_cluster = h_stack((commit_ctrl, row_actions, copy_menu))
        .style(|s| s.items_center().flex_row().gap(3.0));

    h_stack((
        stats,
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

/// Clickable, two-line header cell (name + SQL type). Sorts on click, shows a
/// chevron for the active sort, a key icon for PK/index/FK columns, a selected-
/// column background, and carries a right-edge resize divider.
/// Apply a display formatter to column `ci`: update the live per-column state (so
/// cells re-render) and, when the source table is known, upsert + persist the rule
/// so it survives restarts.
fn set_format(gs: GridState, ci: usize, fmt: ColumnFormat) {
    gs.formats.update(|v| {
        if ci < v.len() {
            v[ci] = fmt;
        }
    });
    if let Some((db, table)) = gs.source.get_untracked()
        && let Some(col) = gs
            .rs
            .get_untracked()
            .columns
            .get(ci)
            .map(|c| c.name.clone())
    {
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

fn header_cell(
    gs: GridState,
    ci: usize,
    sort_val: SortState,
    sort: RwSignal<SortState>,
    key_map: Arc<HashMap<String, ColKey>>,
) -> impl IntoView {
    let rs = gs.rs.get_untracked();
    let col = rs.columns.get(ci);
    let name = col.map(|c| c.name.clone()).unwrap_or_default();
    let type_name = col.map(|c| c.type_name.clone()).unwrap_or_default();
    let numeric = col.map(|c| c.is_numeric()).unwrap_or(false);
    let sorted = matches!(sort_val, Some((c, _)) if c == ci);
    let asc = matches!(sort_val, Some((c, true)) if c == ci);
    let key = key_map.get(&name).copied();

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
                let s = s
                    .height_full()
                    .width_full()
                    .items_center()
                    .padding_horiz(10.0);
                if numeric {
                    s.justify_end()
                } else {
                    s.justify_start()
                }
            })
            .into_any()
    };

    stack((content, col_resize_handle(gs, ci)))
        .on_click_stop(move |_| {
            gs.dismiss_overlays();
            cycle_sort(sort, ci);
        })
        // Right-click → Freeze this column (pin left) · Copy its values.
        .on_secondary_click_stop(move |_| {
            gs.dismiss_overlays();
            let freeze_item = if gs.frozen.get_untracked() == Some(ci) {
                MenuEntry::action("Unfreeze", move || gs.frozen.set(None))
            } else {
                MenuEntry::action("Freeze", move || gs.frozen.set(Some(ci)))
            };
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
            ]));
        })
        .style(move |s| {
            let w = gs.widths.get().get(ci).copied().unwrap_or(CELL_W);
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
                s.hover(|s| s.background(theme::accent().multiply_alpha(0.10)))
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
                    let (orig, orig_null) =
                        gs.rs
                            .with(|rs| match rs.rows.get(data_idx).and_then(|r| r.get(ci)) {
                                Some(v) => (format::apply(fmt, v), v.is_null()),
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
                                // text-align, so pad the left by exactly the free space
                                // — the buffer's *measured* width (re-runs as the buffer
                                // changes, keeping it right-anchored while typing). A
                                // value wider than the column clamps to `pad_left = 0`
                                // (full width, left-aligned + clip) like the display.
                                let w = gs.widths.get().get(ci).copied().unwrap_or(CELL_W);
                                let text_px = gs.edit_buf.with(|b| measure_text_px(b));
                                // Cell content box = column width minus the cell's 10px
                                // horizontal padding on each side.
                                let pad_left = ((w - 20.0) - text_px).max(0.0);
                                s.padding_left(pad_left)
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
    container(content)
        .on_event(EventListener::PointerDown, move |e| {
            if let Event::PointerDown(pe) = e {
                // Any click in a cell dismisses an open menu + the commit-error bar
                // (the pointer-down is consumed here, so the root dismissal handler
                // never sees it).
                gs.dismiss_overlays();
                if pe.button.is_primary() {
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
                        .rows
                        .get(data_idx)
                        .and_then(|r| r.get(ci))
                        .map(|v| v.display())
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
                rs.rows
                    .get(data_idx)
                    .and_then(|r| r.get(ci))
                    .map(|v| format::apply(fmt, v))
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
            // Context for the AI: the source table (if known) + this column.
            let from = match gs.source.get_untracked() {
                Some((db, table)) => format!(" from the `{db}.{table}` table"),
                None => String::new(),
            };
            let msg = format!(
                "Summarize this value{from}, column `{column}`:\n```\n{val}\n```\n\
                 If you can infer a pattern, format, or meaning from it, note that too."
            );

            let mut entries = vec![MenuEntry::action("View", move || {
                gs.viewer.set(true);
                // Keep focus on the grid so Esc closes the viewer.
                if let Some(f) = gs.focus_id.get_untracked() {
                    f.request_focus();
                }
            })];
            // A row marked for deletion isn't editable (it's going away) — only
            // View / Copy / Undo delete / AI Summary.
            if editable && !deleted {
                entries.push(MenuEntry::action("Edit", move || start_edit(gs, i, ci)));
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
            let w = gs.widths.get().get(ci).copied().unwrap_or(CELL_W);
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
            let s = s
                .width(w)
                .height(ROW_H)
                .flex_shrink(0.0_f32)
                .padding_horiz(10.0)
                .items_center();
            let s = if numeric {
                s.justify_end()
            } else {
                s.justify_start()
            };
            let formatted = gs
                .formats
                .with(|f| f.get(ci).map(|x| *x != ColumnFormat::None).unwrap_or(false));
            let s = if is_editing {
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
            } else {
                s
            };
            // Border on every column, last included, so a narrow table still shows
            // where the final column ends.
            s.border_right(1.0).border_color(theme::border())
        })
        // Clip so a value wider than the column doesn't spill over neighbours.
        .clip()
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
