//! **Compare results** — two results of one tab side by side as their
//! differences (`core::result_diff`): rows added, removed and changed,
//! matched by a key column, raised from a result chip's *Compare with*.
//!
//! Of the two results as they were read — a kept one is a snapshot, a live one
//! is what the panel holds when the comparison opens — and of their rows only:
//! nothing is fetched.
//!
//! The two statements sit on cards, **A** (the earlier) and **B**; the ⇄
//! between them compares the other way round, B against A, and the letters
//! stay with their statements. The differences are a table under the shared
//! columns, filtered by the pills above it, coloured as the Live monitor
//! colours its changes: changed in the UPDATE blue, added green, removed red and
//! struck through.

use std::rc::Rc;
use std::sync::Arc;

use floem::AnyView;
use floem::action::save_as;
use floem::event::EventListener;
use floem::file::{FileDialogOptions, FileSpec};
use floem::keyboard::{Key, NamedKey};
use floem::kurbo::Point;
use floem::peniko::Color;
use floem::prelude::*;
use floem::reactive::create_effect;
use floem::views::{VirtualDirection, VirtualItemSize, VirtualVector, virtual_stack};

use schemaic_core::export::{ExportFormat, suggested_filename};
use schemaic_core::model::ResultSet;
use schemaic_core::result_diff::{self, DiffCell, ResultDiff, RowChange, TableRow};

use crate::tooltip::TooltipExt;
use crate::widgets::{
    ACTION_TAB, ActionKind, FocusRing, action_button, autohide_state, cell_text, dismiss_layer,
    focus_root_with_ring, in_ring_button, measure_text_px_at, measure_text_px_bold_at,
    modal_body_h, modal_footer_split, modal_pad_h, modal_title_owned, modal_w, panel_style,
    shift_hscroll, struck_text, thin_scroll, transition_text,
};
use crate::{MenuEntry, OverlayUi, icons, theme};

/// One result the modal compares: its rows, the statement that produced them
/// and when that ran (epoch seconds; `0` for never).
#[derive(Clone)]
pub struct DiffSide {
    pub rs: Arc<ResultSet>,
    pub sql: String,
    pub ran_at: u64,
}

/// The two results the modal compares.
#[derive(Clone)]
pub struct DiffTarget {
    /// The earlier result, **A** — "removed" rows are its until the sides
    /// are swapped.
    pub left: DiffSide,
    /// The later one, **B**.
    pub right: DiffSide,
}

/// The Tab order, top to bottom: the ⇄, the four filter pills, the key picker,
/// the export icon, then Close (`ACTION_TAB`) and the title's ✕.
const SWAP_TAB: u32 = 5;
const PILLS_TAB: u32 = 6;
const PICKER_TAB: u32 = 10;
const EXPORT_TAB: u32 = 11;

/// How long a "Saved …" stays in the footer — the chart's and the ER
/// diagram's linger.
const NOTICE_LINGER: std::time::Duration = std::time::Duration::from_secs(4);

/// The key picker's entry for "no key — the whole row".
const WHOLE_ROW: &str = "Whole row";

/// The widest a column grows to fit its values; a longer one ends in `…`.
fn max_col_w() -> f64 {
    theme::scaled(360.0)
}
fn min_col_w() -> f64 {
    theme::scaled(40.0)
}
fn col_gap() -> f64 {
    theme::scaled(24.0)
}
/// One table row's whole height — decreed, as the Live monitor's
/// `mon_row_h` is, because `VirtualItemSize::Fixed` places row `i` at
/// `i * height` whatever the row actually measures. `row_cells` keeps every
/// cell to one line, which is what makes it honest.
fn row_h() -> f64 {
    theme::scaled(29.0)
}
/// The table's own height, the same for every filter, so switching pills does
/// not move the footer.
fn table_h() -> f64 {
    modal_body_h(320.0)
}

/// The Live monitor's colours, so a removed row here reads as a deleted one
/// there.
fn removed_color() -> Color {
    crate::monitor_view::old_color()
}
fn added_color() -> Color {
    crate::monitor_view::new_color()
}
fn change_color(c: RowChange) -> fn() -> Color {
    match c {
        RowChange::Changed => theme::chip_active,
        RowChange::Added => added_color,
        RowChange::Removed => removed_color,
    }
}

/// One comparison, for one direction and one key.
struct Computed {
    /// Bumped by each recomputation: part of the table's rebuild key.
    generation: u64,
    /// The side compared first is B (the ⇄ was pressed).
    swapped: bool,
    outcome: Result<Outcome, String>,
}

struct Outcome {
    diff: Rc<ResultDiff>,
    /// The table by reference — each row's cells are built by
    /// `result_diff::row_cells` when the virtual stack mounts it.
    rows: Rc<Vec<TableRow>>,
    /// The two results in compared order, for those cells.
    first: Arc<ResultSet>,
    second: Arc<ResultSet>,
    widths: Widths,
}

impl Outcome {
    fn count(&self, filter: Option<RowChange>) -> usize {
        match filter {
            None => self.rows.len(),
            Some(RowChange::Changed) => self.diff.changed.len(),
            Some(RowChange::Added) => self.diff.right_only.len(),
            Some(RowChange::Removed) => self.diff.left_only.len(),
        }
    }
}

/// The table's column widths, **measured once and kept unscaled** — each is
/// scaled where a style reads it, so an interface-scale change with the modal
/// open moves the header and the rows together, as the text inside them moves.
#[derive(Clone)]
struct Widths {
    /// One per shared column.
    cols: Rc<Vec<f64>>,
    /// The *Change* column's.
    change: f64,
}

impl Widths {
    fn col(&self, i: usize) -> f64 {
        theme::scaled(self.cols.get(i).copied().unwrap_or(0.0))
    }

    fn change(&self) -> f64 {
        theme::scaled(self.change)
    }

    /// The table's full width: its padding, every column and the gaps.
    fn total(&self) -> f64 {
        modal_pad_h() * 2.0
            + self.change()
            + (0..self.cols.len()).map(|i| self.col(i)).sum::<f64>()
            + col_gap() * self.cols.len() as f64
    }
}

fn compute(t: &DiffTarget, swapped: bool, key: Option<String>, generation: u64) -> Computed {
    let (first, second) = if swapped {
        (&t.right.rs, &t.left.rs)
    } else {
        (&t.left.rs, &t.right.rs)
    };
    let key: Vec<String> = key.into_iter().collect();
    let outcome = result_diff::diff(first, second, &key)
        .map(|diff| {
            let rows = result_diff::table_rows(&diff);
            // Widths from a bounded sample of built rows, not all of them:
            // building every differing row's cells is the cost `row_cells`
            // exists to defer to the rows a view mounts.
            let sample: Vec<Vec<DiffCell>> = rows
                .iter()
                .take(WIDTH_SAMPLE)
                .map(|r| result_diff::row_cells(first, second, &diff, r))
                .collect();
            // Measured at the scale of the moment, stored at 1×.
            let unit = theme::scaled(1.0);
            let cols = column_widths(&diff.shared, &sample)
                .into_iter()
                .map(|w| w / unit)
                .collect();
            let change = [RowChange::Changed, RowChange::Added, RowChange::Removed]
                .into_iter()
                .map(|c| measure_text_px_bold_at(c.label(), theme::font_label()))
                .fold(0.0, f64::max)
                .ceil()
                / unit
                + 2.0;
            Outcome {
                diff: Rc::new(diff),
                rows: Rc::new(rows),
                first: first.clone(),
                second: second.clone(),
                widths: Widths {
                    cols: Rc::new(cols),
                    change,
                },
            }
        })
        .map_err(|e| e.to_string());
    Computed {
        generation,
        swapped,
        outcome,
    }
}

/// How many table rows' cells are built to size the columns: a column's width
/// is the longest of these, so a longer value further down ends in `…`.
const WIDTH_SAMPLE: usize = 2_000;

/// Each column wide enough for its header and its longest values, between
/// [`min_col_w`] and [`max_col_w`]. Only the few values longest by characters
/// are measured — shaping every cell of a large result would stall the open.
fn column_widths(names: &[String], rows: &[Vec<DiffCell>]) -> Vec<f64> {
    const MEASURED: usize = 3;
    let arrow = measure_text_px_at(" → ", theme::font_body());
    let measure = |c: &DiffCell| match c {
        DiffCell::Value(v) => measure_text_px_at(v, theme::font_body()),
        DiffCell::Changed(a, b) => {
            measure_text_px_at(a, theme::font_body())
                + arrow
                + measure_text_px_at(b, theme::font_body())
        }
    };
    let chars = |c: &DiffCell| match c {
        DiffCell::Value(v) => v.chars().count(),
        DiffCell::Changed(a, b) => a.chars().count() + b.chars().count() + 3,
    };
    names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let mut longest: Vec<(usize, &DiffCell)> = Vec::with_capacity(MEASURED + 1);
            for c in rows.iter().filter_map(|r| r.get(i)) {
                let n = chars(c);
                if longest.len() < MEASURED || longest.last().is_some_and(|(m, _)| n > *m) {
                    longest.push((n, c));
                    longest.sort_by_key(|(m, _)| std::cmp::Reverse(*m));
                    longest.truncate(MEASURED);
                }
            }
            let widest = longest
                .iter()
                .map(|(_, c)| measure(c))
                .fold(measure_text_px_at(name, theme::font_label()), f64::max);
            (widest.ceil() + 2.0).clamp(min_col_w(), max_col_w())
        })
        .collect()
}

pub(crate) fn result_diff_overlay(
    overlay: OverlayUi,
    export_file: crate::ExportFn,
) -> impl IntoView {
    let target = overlay.result_diff;
    dyn_container(
        move || target.with(Option::is_some),
        move |open| {
            let Some(t) = open.then(|| target.get_untracked()).flatten() else {
                return empty().into_any();
            };
            let t = Rc::new(t);
            // Closing takes the export menu with it: the menu rides the app's
            // shared popup, which outlives this modal, and its entries hold
            // this scope's signals (the chart's and the ER diagram's reason).
            // Every exit — ✕, Escape, the backdrop, Close — routes through here.
            let close: Rc<dyn Fn()> = {
                let popup = overlay.popup_menu;
                let popup_anchor = overlay.popup_anchor;
                Rc::new(move || {
                    popup.set(None);
                    popup_anchor.set(None);
                    target.set(None);
                })
            };
            let ring = FocusRing::new();
            // The key picker offers only what can be compared: a binary column
            // reads `<n bytes>` in every row, so it would match nothing apart.
            let shared = result_diff::key_columns(&t.left.rs, &t.right.rs);
            let key = RwSignal::new(
                result_diff::suggested_key(&t.left.rs, &t.right.rs)
                    .into_iter()
                    .next(),
            );
            let swapped = RwSignal::new(false);
            let filter: RwSignal<Option<RowChange>> = RwSignal::new(None);
            let computed = RwSignal::new(Rc::new(compute(&t, false, key.get_untracked(), 0)));
            {
                let t = t.clone();
                create_effect(move |generation: Option<u64>| {
                    let (sw, k) = (swapped.get(), key.get());
                    // The first run is the value `computed` was made with.
                    let Some(g) = generation else { return 0 };
                    computed.set(Rc::new(compute(&t, sw, k, g + 1)));
                    g + 1
                });
            }

            let mut options = vec![WHOLE_ROW.to_string()];
            options.extend(shared.iter().cloned());
            let picker = crate::table_designer::focusable_owned_dropdown(
                move || key.get().unwrap_or_else(|| WHOLE_ROW.to_string()),
                options,
                || theme::scaled(200.0),
                ring.clone(),
                PICKER_TAB,
                move |picked| key.set((picked != WHOLE_ROW).then_some(picked)),
            );

            // ── Export ───────────────────────────────────────────────────
            // What the last export did — said in the footer, in place of the
            // summary, since the app's error modal is painted under this one.
            let notice: RwSignal<Option<(String, bool)>> = RwSignal::new(None);
            let notice_seq = RwSignal::new(0_u64);
            let say: Rc<dyn Fn(String, bool)> = {
                let fallback_err = overlay.error_modal_text;
                let fallback_open = overlay.error_modal_open;
                Rc::new(move |msg: String, failed: bool| {
                    // The completion can land after the modal closed and took
                    // this scope. A failure then still has to be said
                    // somewhere — the shared error modal, as the Live Monitor
                    // does; a confirmation to nobody is dropped.
                    let Some(prev) = notice_seq.try_get_untracked() else {
                        if failed {
                            fallback_err.try_update(|v| {
                                *v = Some(schemaic_core::model::ModalError::app(msg))
                            });
                            fallback_open.try_update(|v| *v = true);
                        }
                        return;
                    };
                    let seq = prev.wrapping_add(1);
                    notice_seq.set(seq);
                    notice.set(Some((msg, failed)));
                    // A confirmation fades; a failure stays, being the only
                    // place the reason is written down.
                    if !failed {
                        floem::action::exec_after(NOTICE_LINGER, move |_| {
                            if notice_seq.try_get_untracked() == Some(seq) {
                                notice.set(None);
                            }
                        });
                    }
                })
            };
            // A notice is about the comparison it was said of: a new key,
            // direction or filter clears it.
            create_effect(move |prev: Option<()>| {
                let _ = (computed.with(|c| c.generation), filter.get());
                if prev.is_some() {
                    notice.set(None);
                }
            });
            let save: Rc<dyn Fn(ExportFormat)> = {
                let export_file = export_file.clone();
                let say = say.clone();
                Rc::new(move |format| {
                    // `try_get_untracked`, as a backstop rather than a known
                    // path: `close` takes the menu down on every exit, and the
                    // container is keyed on being open, so nothing replaces
                    // this scope under an open menu today. A menu that did
                    // outlive it would otherwise panic on a disposed signal.
                    let (Some(c), Some(f)) =
                        (computed.try_get_untracked(), filter.try_get_untracked())
                    else {
                        return;
                    };
                    // The icon's own test, again: the menu can stand open
                    // over a change of key or filter.
                    let Some(o) = c.outcome.as_ref().ok().filter(|o| o.count(f) > 0) else {
                        (say)("Nothing to export.".to_string(), true);
                        return;
                    };
                    save_diff(o, f, format, export_file.clone(), say.clone());
                })
            };
            let popup = overlay.popup_menu;
            let popup_anchor = overlay.popup_anchor;
            let popup_width = overlay.popup_width;
            // Set once the button exists: the ring wrapper's id, so the menu
            // opens under the whole control (`monitor_view`'s rule).
            let export_at: RwSignal<Option<floem::ViewId>> = RwSignal::new(None);
            let open_export: Rc<dyn Fn()> = Rc::new(move || {
                let Some(r) = export_at.get_untracked().map(|id| id.layout_rect()) else {
                    return;
                };
                let mine = crate::PopupAnchor::BelowIcon(r.x0, r.x1, r.y1);
                // Pressing it again closes its own menu.
                if crate::widgets::menu_anchored_at(
                    popup.get_untracked().is_some(),
                    popup_anchor.get_untracked(),
                    mine,
                ) {
                    popup.set(None);
                    return;
                }
                let entries = result_diff::DIFF_FORMATS
                    .into_iter()
                    .map(|f| {
                        let save = save.clone();
                        MenuEntry::action(f.label(), move || (save)(f))
                    })
                    .collect();
                popup_width.set(crate::erd_view::export_menu_w());
                popup_anchor.set(Some(mine));
                popup.set(Some(entries));
            });
            // The Live Monitor's export control: a bare glyph in the Tab ring.
            // Dimmed while there is nothing to export — the key cannot match
            // rows, or the filter shows none (identical results, or a pill
            // with no rows), where a file of headings would be "saved".
            let comparable = move || {
                let f = filter.get();
                computed.with(|c| c.outcome.as_ref().is_ok_and(|o| o.count(f) > 0))
            };
            let export_button = {
                let click = open_export.clone();
                let key_press = open_export.clone();
                in_ring_button(
                    crate::widgets::toolbar_icon(
                        icons::DOWNLOAD,
                        0.0,
                        0.0,
                        comparable,
                        move || (click)(),
                    )
                    .tooltip(|| {
                        text("Export the comparison…").style(crate::widgets::tooltip_style)
                    }),
                    ring.clone(),
                    EXPORT_TAB,
                    true,
                    0.0,
                    move || {
                        if comparable() {
                            (key_press)()
                        }
                    },
                )
            };
            export_at.set(Some(export_button.id()));

            let controls = h_stack((
                filter_pills(computed, filter, ring.clone()),
                empty().style(|s| s.flex_grow(1.0_f32).min_width(theme::scaled(12.0))),
                // **A gap in a row of their own, not a `margin_right` on the
                // label.** With the margin, the picker ended 10px short of the B
                // card's edge above it — measured in the running app, and gone
                // with this spelling.
                h_stack((
                    text("Match rows by").style(|s| {
                        s.color(theme::text_dim())
                            .font_size(theme::font_label())
                            .flex_shrink(0.0_f32)
                    }),
                    picker,
                    // Set apart from the picker by more than the label is:
                    // it acts on the comparison, not on how rows are matched.
                    container(export_button).style(|s| s.margin_left(theme::scaled(6.0))),
                ))
                .style(|s| {
                    s.items_center()
                        .gap(theme::scaled(10.0))
                        .flex_shrink(0.0_f32)
                }),
            ))
            .style(|s| s.items_center().width_full());
            let top = v_stack((
                sides(&t, swapped, ring.clone()),
                controls,
                notes(computed, t.left.rs.truncated || t.right.rs.truncated),
            ))
            .style(|s| {
                s.flex_col()
                    .gap(theme::scaled(14.0))
                    .padding_horiz(modal_pad_h())
                    .padding_top(modal_pad_h())
                    .padding_bottom(theme::scaled(12.0))
                    .width_full()
            });
            let table = dyn_container(
                move || (computed.with(|c| c.generation), filter.get()),
                move |(_, f)| table_view(computed.get_untracked(), f),
            )
            .style(|s| {
                s.width_full()
                    .height(table_h())
                    .border_top(1.0)
                    .border_color(theme::border())
            });

            let title =
                modal_title_owned("Compare results".to_string(), close.clone(), ring.clone());
            let done = close.clone();
            // The export's notice, while there is one, else the summary.
            let status = dyn_container(
                move || notice.get(),
                move |n| match n {
                    None => summary_line(computed).into_any(),
                    Some((msg, true)) => crate::widgets::footer_error(msg),
                    Some((msg, false)) => text(msg)
                        .style(|s| s.color(theme::text_dim()).font_size(theme::font_label()))
                        .into_any(),
                },
            )
            .style(|s| s.min_width(0.0).flex_shrink(1.0_f32));
            let footer = modal_footer_split(
                status,
                action_button(
                    // Neutral: closing is the only thing the footer offers,
                    // and there is no decision for it to affirm.
                    "Close",
                    ActionKind::Neutral,
                    true,
                    ring.clone(),
                    ACTION_TAB,
                    move || (done)(),
                ),
            );
            let panel = v_stack((title, top, table, footer))
                .on_click_stop(|_| {})
                .style(|s| {
                    panel_style(s)
                        .background(theme::bg_panel())
                        .width(modal_w(920.0))
                });
            let esc = close.clone();
            focus_root_with_ring(stack((dismiss_layer(move || close()), panel)), ring)
                .on_key_down(Key::Named(NamedKey::Escape), |_| true, move |_| esc())
                .style(|s| {
                    s.size_full()
                        .items_center()
                        .justify_center()
                        .background(theme::modal_backdrop())
                })
                .into_any()
        },
    )
    .style(move |s| {
        if target.with(Option::is_some) {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}

/// Export the comparison's table — the rows `filter` shows, under the
/// direction and key on screen — as `format`, through the ordinary export
/// pipeline (`result_diff::export_result_set`'s projection, the app's worker
/// writing it), as the Live Monitor saves its log.
///
/// **Snapshotted before the dialog opens**, for `grid::save_export`'s reason:
/// the dialog is modal and slow, and the file should be what the user was
/// looking at when they chose the format.
fn save_diff(
    o: &Outcome,
    filter: Option<RowChange>,
    format: ExportFormat,
    export_file: crate::ExportFn,
    say: Rc<dyn Fn(String, bool)>,
) {
    // The snapshot is reference counts — the two results, the diff, the
    // table — and the filter; the projection waits for a path, so a cancelled
    // dialog costs nothing and the dialog opens at once on a large one.
    let (first, second) = (o.first.clone(), o.second.clone());
    let (diff, rows) = (o.diff.clone(), o.rows.clone());
    let opts = FileDialogOptions::new()
        .title("Export comparison")
        .default_name(suggested_filename(Some("comparison"), format))
        .allowed_types(vec![FileSpec {
            name: format.label(),
            extensions: format.extensions(),
        }]);
    save_as(opts, move |file| {
        let Some(path) = file.and_then(|f| f.path.first().cloned()) else {
            return; // cancelled
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "the comparison".to_string());
        let shown: Vec<TableRow> = rows
            .iter()
            .copied()
            .filter(|r| r.shown_by(filter))
            .collect();
        let rs = Arc::new(result_diff::export_result_set(
            &first, &second, &diff, &shown,
        ));
        // The table's own order; the export applies no sort of its own.
        let order: Arc<Vec<usize>> = Arc::new((0..rs.row_count()).collect());
        let say = say.clone();
        (export_file)(
            crate::ExportRequest {
                path,
                format,
                rs,
                order,
                // No base table, and the dialect is unread: only the SQL
                // renderer consults either, and `DIFF_FORMATS` does not offer
                // it — these rows are observations about two results, not rows
                // of a table.
                source: None,
                more: Vec::new(),
                dialect: Default::default(),
                // The comparison is of rows in hand; nothing could re-run it.
                scope: crate::ExportScope::Fetched,
                // Nothing here offers a Stop, so this run stays out of the
                // app's single cancel slot (`ExportRequest::stoppable`).
                stoppable: false,
            },
            // Spelled out rather than a catch-all, the way `save_log` spells
            // its own: `Cancelled` cannot arrive while nothing offers a Stop,
            // and the day it can, reading it as `Done` would claim a file that
            // was never wholly written. The message is used as the pipeline
            // produced it — it already begins "Export failed: …".
            Rc::new(move |outcome| match outcome {
                // **The sentence is `export_note`'s**, as the grid's export
                // is: it states the rows written and whatever the file holds
                // less of — an XLSX cell cut to the format's limit — which a
                // bare "Saved" would hide in a file that promises whole
                // values. Asked with `true`, it always speaks. A caveat stays
                // up, as a failure does, rather than fading.
                crate::ExportOutcome::Done(tally) => {
                    let msg = schemaic_core::export::export_note(&tally, &name, true)
                        .expect("export_note always speaks when asked with `true`");
                    (say)(msg, tally.has_caveat())
                }
                crate::ExportOutcome::Cancelled => {}
                crate::ExportOutcome::Failed { message, .. } => (say)(message, true),
            }),
        );
    });
}

/// The footer's line: what the comparison found, or nothing beside an error
/// the table already shows.
fn summary_line(computed: RwSignal<Rc<Computed>>) -> impl IntoView {
    label(move || {
        computed.with(|c| match &c.outcome {
            Ok(o) => o.diff.summary(),
            Err(_) => String::new(),
        })
    })
    .style(|s| {
        s.color(theme::text_dim())
            .font_size(theme::font_label())
            .text_ellipsis()
    })
}

/// The two statement cards with the ⇄ between them: the side compared first
/// on the left, so a swap moves the cards and the letters go with them.
fn sides(t: &Rc<DiffTarget>, swapped: RwSignal<bool>, ring: FocusRing) -> impl IntoView + use<> {
    let card_at = |second: bool| {
        let t = t.clone();
        dyn_container(
            move || swapped.get(),
            move |sw| {
                // A is the left side of the target, B the right.
                let (side, letter) = match (sw, second) {
                    (false, false) | (true, true) => (&t.left, "A"),
                    _ => (&t.right, "B"),
                };
                side_card(side, letter).into_any()
            },
        )
        .style(|s| s.flex_grow(1.0_f32).flex_basis(0.0).min_width(0.0))
    };
    let swap = in_ring_button(
        container(icons::icon(icons::ARROW_LEFT_RIGHT, 16.0))
            .on_click_stop(move |_| swapped.update(|s| *s = !*s))
            .style(|s| {
                s.items_center()
                    .padding(theme::scaled(6.0))
                    .border_radius(6.0)
                    .color(theme::text_dim())
                    .hover(|s| s.color(theme::text()).background(theme::pill_hover_bg()))
            }),
        ring,
        SWAP_TAB,
        true,
        6.0,
        move || swapped.update(|s| *s = !*s),
    );
    h_stack((
        card_at(false),
        container(swap).style(|s| s.flex_shrink(0.0_f32).items_center()),
        card_at(true),
    ))
    .style(|s| s.width_full().items_center().gap(theme::scaled(10.0)))
}

/// A statement's card: its letter, when it ran, its rows and time, and the
/// statement itself — on one line per three of text, then cut.
fn side_card(side: &DiffSide, letter: &'static str) -> impl IntoView + use<> {
    let ran = schemaic_core::date::Time::at_local(side.ran_at)
        .filter(|_| side.ran_at > 0)
        .map(|t| format!("Ran {}", t.hms()))
        .unwrap_or_default();
    let n = side.rs.row_count();
    let facts = format!(
        "{n} {} · {} ms",
        schemaic_core::text::plural(n, "row", "rows"),
        side.rs.elapsed_ms
    );
    let sql = side.sql.split_whitespace().collect::<Vec<_>>().join(" ");
    let line = 1.5_f32;
    let head = h_stack((
        text(letter).style(|s| {
            s.font_size(theme::font_hint())
                .font_bold()
                .color(theme::text())
                .background(theme::pill_hover_bg())
                .border_radius(4.0)
                .padding_horiz(theme::scaled(5.0))
                // The line box sits low in the badge: 2px more below evens it.
                .padding_top(theme::scaled(1.0))
                .padding_bottom(theme::scaled(3.0))
                .flex_shrink(0.0_f32)
        }),
        text(ran).style(|s| {
            s.font_size(theme::font_label())
                .color(theme::text_dim())
                .flex_shrink(0.0_f32)
        }),
        empty().style(|s| s.flex_grow(1.0_f32).min_width(theme::scaled(8.0))),
        text(facts).style(|s| {
            s.font_size(theme::font_label())
                .color(theme::text_dim())
                .min_width(0.0)
                .text_ellipsis()
        }),
    ))
    .style(|s| s.items_center().gap(theme::scaled(8.0)).width_full());
    // Three line boxes, then clipped — the activity panel's statement, one line
    // longer. Computed in the closure so the cut follows the interface scale.
    let body = text(sql)
        .style(move |s| {
            s.font_family(crate::MONO_FAMILY.to_string())
                .font_size(theme::font_label())
                .line_height(line)
                .color(theme::text())
                .width_full()
                .min_width(0.0)
                .max_height(f64::from(theme::font_label() * line) * 3.0)
        })
        .clip();
    v_stack((head, body)).style(|s| {
        s.flex_col()
            .gap(theme::scaled(8.0))
            .width_full()
            .min_width(0.0)
            .padding(theme::scaled(12.0))
            .background(theme::bg_editor())
            .border(1.0)
            .border_color(theme::border())
            .border_radius(8.0)
    })
}

/// *All · Changed · Added · Removed*, each with its count — the table
/// designer's pills, a coloured dot naming each kind. Each is a Tab stop
/// (`PILLS_TAB` on, after the ⇄ and before the key picker) that Enter or Space
/// picks.
fn filter_pills(
    computed: RwSignal<Rc<Computed>>,
    filter: RwSignal<Option<RowChange>>,
    ring: FocusRing,
) -> impl IntoView {
    let pick = move |which: Option<RowChange>| {
        if filter.get_untracked() != which {
            filter.set(which);
        }
    };
    let pill = move |name: &'static str, which: Option<RowChange>, tabindex: u32| {
        crate::widgets::count_pill(
            name,
            which.map(change_color),
            move || {
                computed.with(|c| match &c.outcome {
                    Ok(o) => o.count(which).to_string(),
                    Err(_) => String::new(),
                })
            },
            move || filter.get() == which,
            move || pick(which),
            ring.clone(),
            tabindex,
        )
    };
    h_stack((
        pill("All", None, PILLS_TAB),
        pill("Changed", Some(RowChange::Changed), PILLS_TAB + 1),
        pill("Added", Some(RowChange::Added), PILLS_TAB + 2),
        pill("Removed", Some(RowChange::Removed), PILLS_TAB + 3),
    ))
    .style(|s| {
        s.items_center()
            .gap(theme::scaled(4.0))
            .flex_shrink(0.0_f32)
    })
}

/// What the table cannot show: columns one side alone has, binary columns
/// (whose `<n bytes>` text is not their value), rows whose key was taken, a
/// capped result. Hidden when there is none.
fn notes(computed: RwSignal<Rc<Computed>>, capped: bool) -> impl IntoView {
    let lines = move || {
        computed.with(|c| {
            let Ok(o) = &c.outcome else {
                return Vec::new();
            };
            let (first, second) = if c.swapped { ("B", "A") } else { ("A", "B") };
            let d = &o.diff;
            let mut notes = Vec::new();
            if !d.only_left.is_empty() {
                notes.push(format!("Only in {first}: {}", d.only_left.join(", ")));
            }
            if !d.only_right.is_empty() {
                notes.push(format!("Only in {second}: {}", d.only_right.join(", ")));
            }
            if !d.uncompared.is_empty() {
                notes.push(format!("Binary, not compared: {}", d.uncompared.join(", ")));
            }
            if d.repeated_keys > 0 {
                notes.push(format!(
                    "{} {} a key already taken on its side, and {} left out.",
                    d.repeated_keys,
                    schemaic_core::text::plural(d.repeated_keys, "row repeats", "rows repeat"),
                    schemaic_core::text::plural(d.repeated_keys, "is", "are")
                ));
            }
            if capped {
                notes.push("A result was capped: rows past its cap are not compared.".to_string());
            }
            notes
        })
    };
    label(move || lines().join("  ·  ")).style(move |s| {
        let empty = lines().is_empty();
        let s = s
            .font_size(theme::font_hint())
            .color(theme::text_faint())
            .width_full()
            .min_width(0.0);
        if empty { s.hide() } else { s }
    })
}

/// Row source for the table's virtual stack: indices into the outcome's rows.
struct Picked(Rc<Vec<usize>>);

impl VirtualVector<usize> for Picked {
    fn total_len(&self) -> usize {
        self.0.len()
    }

    fn slice(&mut self, range: std::ops::Range<usize>) -> impl Iterator<Item = usize> {
        let end = range.end.min(self.0.len());
        let start = range.start.min(end);
        self.0[start..end].iter().copied()
    }
}

/// The table for one comparison and one filter: a header that follows the
/// body sideways, and the body — only its visible rows built — scrolling both
/// ways, Shift+wheel included.
fn table_view(computed: Rc<Computed>, filter: Option<RowChange>) -> AnyView {
    let o = match &computed.outcome {
        Ok(o) => o,
        Err(e) => {
            return message(e.clone(), theme::error).into_any();
        }
    };
    let picked: Rc<Vec<usize>> = Rc::new(
        o.rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.shown_by(filter))
            .map(|(i, _)| i)
            .collect(),
    );
    if picked.is_empty() {
        let said = match filter {
            _ if o.rows.is_empty() => "No differences.",
            Some(RowChange::Changed) => "No changed rows.",
            Some(RowChange::Added) => "No added rows.",
            Some(RowChange::Removed) => "No removed rows.",
            None => "No differences.",
        };
        return message(said.to_string(), theme::text_dim).into_any();
    }
    let widths = o.widths.clone();
    // The one writer of the horizontal offset is the body; the header reads it
    // and blocks its own wheel — the Live monitor's one-writer rule.
    let hscroll = RwSignal::new(0.0_f64);
    let head = {
        let w = widths.clone();
        let mut cells: Vec<AnyView> = vec![header_cell("Change".to_string(), w, None)];
        cells.extend(
            o.diff
                .shared
                .iter()
                .enumerate()
                .map(|(i, n)| header_cell(n.clone(), widths.clone(), Some(i))),
        );
        let w = widths.clone();
        h_stack_from_iter(cells).style(move |s| {
            s.items_center()
                .gap(col_gap())
                .padding_horiz(modal_pad_h())
                .height(theme::scaled(34.0))
                .min_width(w.total())
        })
    };
    let header = container(
        scroll(head)
            .scroll_style(|cs| cs.hide_bars(true))
            .scroll_to(move || Some(Point::new(hscroll.get(), 0.0)))
            .on_event_stop(EventListener::PointerWheel, |_| {})
            .style(|s| s.width_full()),
    )
    .style(|s| {
        s.width_full()
            .flex_shrink(0.0_f32)
            .border_bottom(1.0)
            .border_color(theme::border())
    });
    let rows = o.rows.clone();
    let (first, second, diff) = (o.first.clone(), o.second.clone(), o.diff.clone());
    let w = widths.clone();
    let list = virtual_stack(
        VirtualDirection::Vertical,
        VirtualItemSize::Fixed(Box::new(row_h)),
        move || Picked(picked.clone()),
        |i| *i,
        // A row's cells are built here, as the stack mounts it.
        move |i| {
            let row = rows[i];
            let cells = result_diff::row_cells(&first, &second, &diff, &row);
            table_row(row.change, &cells, &widths)
        },
    )
    .style(move |s| s.flex_col().min_width(w.total()));
    // The band is a wrapper's, not the stack's: padding on a virtual stack
    // shortens its window (see the Live monitor's list).
    let list = container(list).style(|s| s.flex_col().padding_vert(theme::scaled(4.0)));
    let (shown, poke) = autohide_state();
    let body = shift_hscroll(list)
        .scroll_style(move |cs| thin_scroll(cs).hide_bars(!shown.get()))
        .on_scroll(move |vp| {
            if hscroll.try_get_untracked().is_some_and(|x| x != vp.x0) {
                hscroll.set(vp.x0);
            }
            poke();
        })
        .style(|s| s.flex_grow(1.0_f32).width_full().min_height(0.0));
    v_stack((header, body))
        .style(|s| s.flex_col().size_full().min_height(0.0))
        .into_any()
}

fn message(said: String, color: fn() -> Color) -> impl IntoView {
    container(text(said).style(move |s| s.color(color()).font_size(theme::font_body())))
        .style(|s| s.size_full().items_center().justify_center())
}

/// A column's header: the shared column `col`, or *Change* for `None`.
fn header_cell(name: String, widths: Widths, col: Option<usize>) -> AnyView {
    text(name)
        .style(move |s| {
            s.color(theme::text_dim())
                .font_size(theme::font_label())
                .width(col.map_or_else(|| widths.change(), |i| widths.col(i)))
                .flex_shrink(0.0_f32)
                .text_ellipsis()
        })
        .into_any()
}

/// One row of the table: its kind, then a cell per shared column.
fn table_row(change: RowChange, row_cells: &[DiffCell], widths: &Widths) -> impl IntoView + use<> {
    let color = change_color(change);
    let w = widths.clone();
    let mut cells: Vec<AnyView> = vec![
        text(change.label())
            .style(move |s| {
                s.color(color())
                    .font_size(theme::font_label())
                    .font_bold()
                    .width(w.change())
                    .flex_shrink(0.0_f32)
            })
            .into_any(),
    ];
    for (i, c) in row_cells.iter().enumerate() {
        let w = widths.clone();
        let content: AnyView = match (c, change) {
            (DiffCell::Changed(before, after), _) => {
                transition_text(before.clone(), after.clone(), removed_color, added_color)
                    .into_any()
            }
            (DiffCell::Value(v), RowChange::Removed) => {
                struck_text(v.clone(), removed_color).into_any()
            }
            (DiffCell::Value(v), RowChange::Added) => cell_text(v.clone(), added_color).into_any(),
            (DiffCell::Value(v), RowChange::Changed) => {
                cell_text(v.clone(), theme::text_dim).into_any()
            }
        };
        cells.push(
            container(content)
                .style(move |s| {
                    s.width(w.col(i))
                        .flex_shrink(0.0_f32)
                        .min_width(0.0)
                        .items_center()
                })
                .clip()
                .into_any(),
        );
    }
    h_stack_from_iter(cells).style(|s| {
        s.items_center()
            .gap(col_gap())
            .padding_horiz(modal_pad_h())
            .height(row_h())
    })
}
