//! **Chart** — a bar, stacked bar, line, scatter or histogram of the result on screen
//! (`core::chart`), raised from the results strip's chart icon.
//!
//! Of the rows already read: the modal says how many those are, and a capped
//! result is a chart of the rows read, not of the table. The canvas is one
//! painted view (`ChartCanvas`) — floem 0.2 has no plotting, and the ER
//! diagram's edges and the frozen header's hatching are drawn the same way —
//! with its axis text laid out by floem's own text engine, so it is the app's
//! face at the app's scale.

use std::rc::Rc;
use std::sync::Arc;

use floem::AnyView;
use floem::context::PaintCx;
use floem::keyboard::{Key, NamedKey};
use floem::kurbo::{BezPath, Circle, Line, Point, Rect, Stroke};
use floem::prelude::*;
use floem::reactive::create_effect;
use floem::text::{Attrs, AttrsList, TextLayout};
use floem::{View, ViewId};
use floem_renderer::Renderer;

use schemaic_core::chart::{self, ChartKind, Plot};
use schemaic_core::chart_draw;
use schemaic_core::erd_export::{self, ErdExportFormat};
use schemaic_core::model::ResultSet;

use crate::i18n::tr;
use crate::settings::focusable_dropdown;
use crate::tooltip::TooltipExt;
use crate::widgets::{
    ACTION_TAB, ActionKind, FocusRing, action_button, dismiss_layer, focus_root_with_ring,
    modal_footer_split, modal_pad_h, modal_title_owned, panel_style,
};
use crate::{MenuEntry, OverlayUi, theme};

/// What the modal charts: the result, and what to call it.
#[derive(Clone)]
pub struct ChartTarget {
    pub rs: Arc<ResultSet>,
    pub title: String,
}

/// The modal's width, unscaled.
const PANEL_W: f64 = 760.0;
/// The chart-kind picker's width, unscaled.
const KIND_W: f64 = 140.0;
/// A column picker's width, unscaled — sized so the widest row (a stacked
/// bar's four pickers and the export icon) fits the panel.
const COL_W: f64 = 150.0;
/// The gap between the picker row's controls, unscaled.
const ROW_GAP: f64 = 16.0;
/// Room the export icon takes at the row's end, unscaled — generous.
#[cfg(test)]
const EXPORT_ALLOWANCE_W: f64 = 28.0;

/// Modal width and the canvas's height.
fn panel_w() -> f64 {
    theme::scaled(PANEL_W)
}
fn canvas_h() -> f64 {
    theme::scaled(360.0)
}

/// The label column's picker entry for "no column — number the rows".
///
/// The picker hands back the text it shows, so the pick is compared against
/// this — one call per build, both sides in the same language.
fn row_number() -> String {
    tr!("chart-row-number")
}

/// A chart kind's name — `ChartKind::label`'s word in the interface's
/// language; `english_names_every_chart_kind_as_core_does` holds them equal.
fn chart_kind_label(kind: ChartKind) -> String {
    match kind {
        ChartKind::Bar => tr!("chart-kind-bar"),
        ChartKind::Line => tr!("chart-kind-line"),
        ChartKind::Histogram => tr!("chart-kind-histogram"),
        ChartKind::Scatter => tr!("chart-kind-scatter"),
        ChartKind::StackedBar => tr!("chart-kind-stacked-bar"),
    }
}

/// A stacked bar's segment colours, in stack order: the connection colour
/// presets — saturated mid-tones chosen to read on both themes — taken so
/// that neighbouring stacks differ most in hue. Read at paint time, as every
/// theme colour is.
fn stack_palette() -> Vec<floem::peniko::Color> {
    // Blue, orange, green, purple, amber, teal, pink, red.
    [5, 1, 3, 6, 2, 4, 7, 0]
        .iter()
        .map(|&i| (crate::CONN_COLOR_PRESETS[i].2)())
        .collect()
}

/// `rs` charted as `kind` from `picks`, its folded stack named in the
/// interface's language — the canvas's plot and the export's alike.
fn plot_of(rs: &ResultSet, kind: ChartKind, picks: chart::Picks) -> (Plot, usize) {
    let (mut drawn, skipped) = Plot::of(rs, kind, picks);
    if let Plot::Stacked(s) = &mut drawn {
        s.name_other(&tr!("chart-other"));
    }
    (drawn, skipped)
}

/// The line under the canvas — core's `chart::caption` in the interface's
/// language; `english_captions_every_chart_as_core_does` holds them equal.
fn caption_text(rows: usize, kind: ChartKind, skipped: usize, truncated: bool) -> String {
    let mut parts = vec![tr!("chart-caption-rows", n = rows)];
    if skipped > 0 {
        parts.push(match kind {
            ChartKind::Histogram => tr!("chart-caption-skipped-null", skipped = skipped),
            ChartKind::Bar | ChartKind::Line | ChartKind::Scatter | ChartKind::StackedBar => tr!(
                "chart-caption-skipped",
                skipped = skipped,
                max = chart::MAX_POINTS
            ),
        });
    }
    if truncated {
        parts.push(tr!("chart-caption-truncated"));
    }
    parts.join(" · ")
}

/// The save icon's place in the Tab ring: after the pickers (10, 20, 30, 35).
const EXPORT_TAB: u32 = 40;

/// How long a "Saved …" stays in the footer — the ER diagram's linger.
const NOTICE_LINGER: std::time::Duration = std::time::Duration::from_secs(4);

/// A saved chart's size and text, in the file's own units: fixed rather than
/// the canvas's, so a chart saved at a zoomed UI is the same picture as one
/// saved at 100%. A PNG is that at [`EXPORT_PNG_SCALE`].
const EXPORT_W: f64 = 800.0;
const EXPORT_H: f64 = 400.0;
const EXPORT_FONT: f32 = 12.0;
/// The background's margin round the chart in a saved file.
const EXPORT_PAD: f64 = 16.0;
const EXPORT_PNG_SCALE: f32 = 2.0;

/// `drawn` as the document `fmt` saves — `None` when there is nothing to
/// draw. The SVG is built here, on the UI thread, because its labels are
/// measured by floem's font system — and it is bounded whatever the result's
/// size: a series or a scatter draws at most [`chart::MAX_POINTS`] marks, a
/// stacked bar at most that many bars of [`chart::MAX_STACKS`]` + 1` segments,
/// and a histogram at most 50 bins. A PNG's rasterise is left to the worker.
fn export_doc(drawn: &Plot, fmt: ErdExportFormat) -> Option<crate::ErdDoc> {
    let scene = chart_draw::layout(
        drawn,
        EXPORT_W,
        EXPORT_H,
        &chart_draw::Metrics::BASE,
        crate::i18n::number_marks(),
        &mut |t| measure(t, EXPORT_FONT),
    )?;
    use crate::erd_view::hex;
    // The canvas's colours, in the theme of the moment — as the ER diagram's
    // file is — on the panel colour the canvas sits on.
    let colors = chart_draw::SvgColors {
        background: hex(theme::bg_panel()),
        grid: hex(theme::border()),
        tick_text: hex(theme::text_faint()),
        under_text: hex(theme::text_dim()),
        mark: hex(theme::accent()),
        zero: hex(theme::text_faint()),
        stacks: stack_palette().into_iter().map(hex).collect(),
        other: hex(theme::text_faint()),
    };
    let svg = chart_draw::to_svg(&scene, &colors, f64::from(EXPORT_FONT), EXPORT_PAD);
    match fmt {
        ErdExportFormat::Png => Some(crate::ErdDoc::Raster(svg, EXPORT_PNG_SCALE)),
        ErdExportFormat::Svg => Some(crate::ErdDoc::Text(svg)),
        // Not offered: a chart is a picture.
        ErdExportFormat::Mermaid
        | ErdExportFormat::Dbml
        | ErdExportFormat::PlantUml
        | ErdExportFormat::Dot => None,
    }
}

pub(crate) fn chart_overlay(overlay: OverlayUi, export: crate::ErdExportFn) -> impl IntoView {
    let target = overlay.chart;
    dyn_container(
        move || target.get().map(|t| (Arc::as_ptr(&t.rs) as usize, t)),
        move |open| {
            let Some((_, t)) = open else {
                return empty().into_any();
            };
            // Closing takes the save menu with it, for the ER diagram's reason:
            // the menu rides the app's shared popup, which outlives this
            // modal, and its entries hold this scope's signals. Every exit —
            // ✕, Escape, the backdrop, Close — routes through here.
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
            let rs = t.rs.clone();
            let numeric = chart::numeric_columns(&rs);
            // The pickers' names, distinct (`chart::picker_labels`): they map
            // a choice back to its column by name.
            let names = chart::picker_labels(
                &rs.columns
                    .iter()
                    .map(|c| c.name.clone())
                    .collect::<Vec<_>>(),
                &[row_number().as_str()],
            );

            // The choices. The value column starts on the last numeric one —
            // the measure usually follows its labels — and the label column on
            // the first column that is not it.
            let kind = RwSignal::new(ChartKind::Bar);
            let first_value = numeric.last().copied();
            let value = RwSignal::new(first_value);
            let first_label = (0..names.len()).find(|c| Some(*c) != first_value);
            let label = RwSignal::new(first_label);
            // A scatter's X starts on another numeric column — the value
            // column itself when it is the only one — and a stacked bar's
            // segments on the first column that is neither of the other two.
            let x = RwSignal::new(
                numeric
                    .iter()
                    .copied()
                    .find(|c| Some(*c) != first_value)
                    .or(first_value),
            );
            let by = RwSignal::new(
                (0..names.len()).find(|c| Some(*c) != first_value && Some(*c) != first_label),
            );
            let picks = move || (kind.get(), value.get(), label.get(), x.get(), by.get());

            let body = {
                let rs = rs.clone();
                dyn_container(picks, move |(k, v, l, x, by)| {
                    chart_body(
                        &rs,
                        k,
                        v.map(|value| chart::Picks {
                            value,
                            label: l,
                            x,
                            by,
                        }),
                    )
                })
                .style(|s| s.width_full())
            };
            let title = modal_title_owned(
                tr!("chart-title", tab = t.title.clone()),
                close.clone(),
                ring.clone(),
            );

            // ── Save ──────────────────────────────────────────────────────
            // What the last save did, and whether it failed — said here, since
            // the app's error modal is painted under this one (the ER diagram's
            // reason for its own notice).
            let notice: RwSignal<Option<(String, bool)>> = RwSignal::new(None);
            let notice_seq = RwSignal::new(0_u64);
            let say: Rc<dyn Fn(String, bool)> = Rc::new(move |msg: String, failed: bool| {
                // `try_get_untracked`: the save reports from its completion,
                // which can land after the modal closed and took this scope.
                let Some(prev) = notice_seq.try_get_untracked() else {
                    return;
                };
                let seq = prev.wrapping_add(1);
                notice_seq.set(seq);
                notice.set(Some((msg, failed)));
                // A confirmation fades; a failure stays, being the only place
                // the reason is written down.
                if !failed {
                    floem::action::exec_after(NOTICE_LINGER, move |_| {
                        if notice_seq.try_get_untracked() == Some(seq) {
                            notice.set(None);
                        }
                    });
                }
            });
            // A notice is about the chart it was said of: a new choice in the
            // pickers clears it, or "Nothing to save." would still stand over
            // a column that can be saved.
            create_effect(move |prev: Option<()>| {
                let _ = picks();
                if prev.is_some() {
                    notice.set(None);
                }
            });
            let save: Rc<dyn Fn(ErdExportFormat)> = {
                let rs = rs.clone();
                let export = export.clone();
                let name = t.title.clone();
                Rc::new(move |fmt| {
                    // The chart as it is **now**, before the dialog opens: the
                    // pickers are this modal's, and the dialog outlives nothing.
                    //
                    // `try_get_untracked`: a menu entry can outlive this scope —
                    // the target replaced under an open menu disposes it without
                    // a `close` — and a bare read of a disposed signal panics.
                    // Nothing to save for a chart that is gone.
                    let (Some(k), Some(v), Some(l), Some(x), Some(by)) = (
                        kind.try_get_untracked(),
                        value.try_get_untracked(),
                        label.try_get_untracked(),
                        x.try_get_untracked(),
                        by.try_get_untracked(),
                    ) else {
                        return;
                    };
                    let doc = v.and_then(|value| {
                        let picks = chart::Picks {
                            value,
                            label: l,
                            x,
                            by,
                        };
                        let (drawn, _) = plot_of(&rs, k, picks);
                        export_doc(&drawn, fmt)
                    });
                    let Some(doc) = doc else {
                        (say)(tr!("chart-nothing-to-save"), true);
                        return;
                    };
                    crate::erd_view::save_document(
                        doc,
                        &tr!("chart-save-dialog"),
                        erd_export::file_stem(&tr!("chart-file-stem", name = name.clone()), fmt),
                        fmt,
                        export.clone(),
                        say.clone(),
                    );
                })
            };
            // The Live Monitor's export control — a bare glyph in the Tab
            // ring, its menu on the app's shared popup, the one surface
            // painted above this modal — since a feature wanted this rarely
            // is better a glyph than two footer buttons.
            let popup = overlay.popup_menu;
            let popup_anchor = overlay.popup_anchor;
            let popup_width = overlay.popup_width;
            // Set once the button exists: the ring wrapper's id, so the menu
            // opens under the whole control (`monitor_view`'s rule).
            let export_at: RwSignal<Option<ViewId>> = RwSignal::new(None);
            let open_export: Rc<dyn Fn()> = {
                let save = save.clone();
                Rc::new(move || {
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
                    let entries = [ErdExportFormat::Png, ErdExportFormat::Svg]
                        .into_iter()
                        .map(|f| {
                            let save = save.clone();
                            MenuEntry::action(f.label(), move || (save)(f))
                        })
                        .collect();
                    popup_width.set(crate::erd_view::export_menu_w());
                    popup_anchor.set(Some(mine));
                    popup.set(Some(entries));
                })
            };
            let export_button = {
                let click = open_export.clone();
                crate::widgets::in_ring_button(
                    crate::widgets::toolbar_icon(
                        crate::icons::DOWNLOAD,
                        0.0,
                        0.0,
                        || true,
                        move || (click)(),
                    )
                    .tooltip(|| text(tr!("chart-save-tip")).style(crate::widgets::tooltip_style)),
                    ring.clone(),
                    EXPORT_TAB,
                    true,
                    0.0,
                    move || (open_export)(),
                )
            };
            export_at.set(Some(export_button.id()));
            let controls = controls(
                &names,
                &numeric,
                Pickers {
                    kind,
                    value,
                    label,
                    x,
                    by,
                },
                ring.clone(),
                export_button,
            );

            let status = dyn_container(
                move || notice.get(),
                move |n| match n {
                    None => empty().into_any(),
                    Some((msg, true)) => crate::widgets::footer_error(msg),
                    Some((msg, false)) => crate::widgets::ellipsized_text(msg, |s| {
                        s.color(theme::text_dim()).font_size(theme::font_label())
                    })
                    .into_any(),
                },
            )
            .style(|s| s.min_width(0.0).flex_shrink(1.0_f32));

            let done = close.clone();
            let footer = modal_footer_split(
                status,
                action_button(
                    tr!("dialog-close"),
                    ActionKind::Primary,
                    true,
                    ring.clone(),
                    ACTION_TAB,
                    move || (done)(),
                ),
            );
            let panel = v_stack((
                title,
                v_stack((controls, body)).style(|s| {
                    s.flex_col()
                        .gap(theme::scaled(12.0))
                        .padding(modal_pad_h())
                        .width_full()
                }),
                footer,
            ))
            .on_click_stop(|_| {})
            .style(|s| {
                panel_style(s)
                    .background(theme::bg_panel())
                    .width(panel_w())
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

/// The modal's choices, each a signal the pickers write and the body reads.
#[derive(Clone, Copy)]
struct Pickers {
    kind: RwSignal<ChartKind>,
    value: RwSignal<Option<usize>>,
    label: RwSignal<Option<usize>>,
    /// A scatter's X column.
    x: RwSignal<Option<usize>>,
    /// A stacked bar's segment column; `None` is one segment.
    by: RwSignal<Option<usize>>,
}

/// Which of the pickers after the value column a kind shows.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Extra {
    /// None: a histogram's labels are its bins.
    Nothing,
    /// The label column.
    Labels,
    /// A scatter's X column, in place of the labels.
    XAxis,
    /// The label column and the segment column.
    LabelsAndStacks,
}

fn extra_of(kind: ChartKind) -> Extra {
    match kind {
        ChartKind::Histogram => Extra::Nothing,
        ChartKind::Bar | ChartKind::Line => Extra::Labels,
        ChartKind::Scatter => Extra::XAxis,
        ChartKind::StackedBar => Extra::LabelsAndStacks,
    }
}

/// The pickers — kind, value column, and what the kind reads besides
/// ([`Extra`]) — and the export icon after them.
fn controls(
    names: &[String],
    numeric: &[usize],
    p: Pickers,
    ring: FocusRing,
    export: AnyView,
) -> AnyView {
    let Pickers {
        kind,
        value,
        label,
        x,
        by,
    } = p;
    let caption = |s: String| text(s).style(crate::widgets::form_label_style);
    let col_w = || theme::scaled(COL_W);
    // A picked name → its column; `names` are distinct, so two columns the
    // result names alike are two choices.
    let lookup = {
        let names = names.to_vec();
        move |picked: &str| names.iter().position(|n| n == picked)
    };
    // In a signal, which is `Copy`, because the picker's face is a `Copy` closure.
    let names_sig = RwSignal::new(names.to_vec());
    let name_of = move |c: usize| names_sig.with(|n| n.get(c).cloned().unwrap_or_default());
    let value_field = {
        let options: Vec<String> = numeric.iter().map(|&c| names[c].clone()).collect();
        let lookup = lookup.clone();
        crate::table_designer::focusable_owned_dropdown(
            move || value.get().map(name_of).unwrap_or_default(),
            options,
            col_w,
            ring.clone(),
            20,
            move |picked| value.set(lookup(&picked)),
        )
    };
    // The pickers a kind does not read are not there at all — built or not
    // rather than hidden, since a hidden control is still a stop in the
    // modal's Tab ring.
    let row_number = row_number();
    let mut label_options = vec![row_number.clone()];
    label_options.extend(names.iter().cloned());
    let mut by_options = vec![tr!("chart-stack-none")];
    by_options.extend(names.iter().cloned());
    let x_options: Vec<String> = numeric.iter().map(|&c| names[c].clone()).collect();
    let extra_ring = ring.clone();
    let labelled = move |title: String, field: AnyView| {
        v_stack((caption(title), field))
            .style(|s| s.flex_col().gap(theme::scaled(6.0)))
            .into_any()
    };
    let extra_group = dyn_container(
        move || extra_of(kind.get()),
        move |extra| {
            let ring = extra_ring.clone();
            // Told apart by position, not by text: a column named exactly
            // like *Row number* is still that column.
            let label_field = || {
                labelled(
                    tr!("chart-caption-labels"),
                    crate::table_designer::focusable_indexed_dropdown(
                        move || label.get().map_or(0, |c| c + 1),
                        label_options.clone(),
                        col_w,
                        ring.clone(),
                        30,
                        move |picked| label.set(crate::table_designer::after_none_row(picked)),
                    )
                    .into_any(),
                )
            };
            match extra {
                Extra::Nothing => empty().into_any(),
                Extra::Labels => label_field(),
                Extra::XAxis => {
                    let lookup = lookup.clone();
                    labelled(
                        tr!("chart-caption-x"),
                        crate::table_designer::focusable_owned_dropdown(
                            move || x.get().map(name_of).unwrap_or_default(),
                            x_options.clone(),
                            col_w,
                            ring.clone(),
                            30,
                            move |picked| x.set(lookup(&picked)),
                        )
                        .into_any(),
                    )
                }
                Extra::LabelsAndStacks => {
                    let stacks = labelled(
                        tr!("chart-caption-stack-by"),
                        crate::table_designer::focusable_indexed_dropdown(
                            move || by.get().map_or(0, |c| c + 1),
                            by_options.clone(),
                            col_w,
                            ring.clone(),
                            35,
                            move |picked| by.set(crate::table_designer::after_none_row(picked)),
                        )
                        .into_any(),
                    );
                    h_stack((label_field(), stacks))
                        .style(|s| s.gap(theme::scaled(ROW_GAP)).items_center())
                        .into_any()
                }
            }
        },
    );
    h_stack((
        v_stack((
            caption(tr!("chart-caption-kind")),
            focusable_dropdown(kind, ChartKind::ALL, chart_kind_label, ring, 10)
                .style(|s| s.width(theme::scaled(KIND_W))),
        ))
        .style(|s| s.flex_col().gap(theme::scaled(6.0))),
        v_stack((caption(tr!("chart-caption-values")), value_field))
            .style(|s| s.flex_col().gap(theme::scaled(6.0))),
        extra_group,
        // Pushed to the row's right end, and centred on the **dropdowns**
        // rather than the row: under a blank caption, the pickers' own
        // caption and gap, so the row's centring puts the icon's middle on
        // theirs at any UI scale without a measured nudge.
        empty().style(|s| s.flex_grow(1.0_f32)),
        v_stack((caption(" ".to_string()), export))
            .style(|s| s.flex_col().items_center().gap(theme::scaled(6.0))),
    ))
    .style(|s| s.gap(theme::scaled(ROW_GAP)).items_center().width_full())
    .into_any()
}

/// The canvas and the line under it — or why there is nothing to draw.
fn chart_body(rs: &ResultSet, kind: ChartKind, picks: Option<chart::Picks>) -> AnyView {
    let note = |msg: String| {
        container(text(msg).style(|s| s.color(theme::text_dim()).font_size(theme::font_body())))
            .style(|s| {
                s.height(canvas_h())
                    .width_full()
                    .items_center()
                    .justify_center()
            })
            .into_any()
    };
    let Some(picks) = picks else {
        return note(tr!("chart-no-numbers"));
    };
    let (drawn, skipped) = plot_of(rs, kind, picks);
    if drawn.is_empty() {
        // A scatter needs both of a row's numbers, so its empty chart is
        // about pairs rather than one column.
        return note(match drawn {
            Plot::Points(_) => tr!("chart-no-pairs"),
            _ => tr!("chart-all-null"),
        });
    }
    // Said, where the canvas would otherwise paint nothing.
    if drawn.undrawable().is_some() {
        return note(tr!("chart-too-large"));
    }
    let line = caption_text(rs.row_count(), kind, skipped, rs.truncated);
    v_stack((
        ChartCanvas {
            id: ViewId::new(),
            drawn: Rc::new(drawn),
        }
        .style(|s| s.width_full().height(canvas_h())),
        text(line).style(|s| s.color(theme::text_faint()).font_size(theme::font_hint())),
    ))
    .style(|s| s.flex_col().gap(theme::scaled(6.0)).width_full())
    .into_any()
}

/// The painted chart: grid lines and value labels on the left, the marks,
/// the labels under the axis, and a stacked bar's legend. Colours are read at paint time, so a
/// theme switch repaints in the new ones.
struct ChartCanvas {
    id: ViewId,
    drawn: Rc<Plot>,
}

fn layout(text: &str, size: f32, color: fn() -> floem::peniko::Color) -> TextLayout {
    let mut l = TextLayout::new();
    l.set_text(
        text,
        AttrsList::new(Attrs::new().font_size(size).color(color())),
    );
    l
}

impl View for ChartCanvas {
    fn id(&self) -> ViewId {
        self.id
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let size = self.id.get_size().unwrap_or_default();
        let font = theme::font_hint();
        // Where everything goes is `chart_draw::layout`'s — the export draws
        // the same scene, so the file is this canvas and not a second drawing.
        //
        // The layouts it measures are kept, in the order it measures them —
        // the tick labels first — so a tick label is shaped once per paint,
        // not once to measure and again to draw. Built in the ticks' colour;
        // the few labels under the axis are drawn in their own.
        let mut shaped: Vec<TextLayout> = Vec::new();
        let Some(scene) = chart_draw::layout(
            &self.drawn,
            size.width,
            size.height,
            &chart_draw::Metrics::BASE.map(theme::scaled),
            crate::i18n::number_marks(),
            &mut |t| {
                let l = layout(t, font, theme::text_faint);
                let s = l.size();
                shaped.push(l);
                (s.width, s.height)
            },
        ) else {
            return;
        };
        let p = &scene.plot;
        let rect = |a: &chart_draw::Area| Rect::new(a.x0, a.y0, a.x1, a.y1);

        // Grid lines and their values.
        let mut grid = BezPath::new();
        for y in &scene.grid {
            grid.move_to((p.x0, *y));
            grid.line_to((p.x1, *y));
        }
        for x in &scene.vgrid {
            grid.move_to((*x, p.y0));
            grid.line_to((*x, p.y1));
        }
        cx.stroke(&grid, theme::border(), &Stroke::new(1.0));
        for (l, text) in scene.ticks.iter().zip(&shaped) {
            cx.draw_text(text, Point::new(l.left, l.top));
        }

        let mark = theme::accent();
        let palette = stack_palette();
        let other = theme::text_faint();
        match &scene.marks {
            chart_draw::Marks::Line { points, dots } => {
                let mut path = BezPath::new();
                for (i, &pt) in points.iter().enumerate() {
                    if i == 0 {
                        path.move_to(pt)
                    } else {
                        path.line_to(pt)
                    }
                }
                cx.stroke(&path, mark, &Stroke::new(chart_draw::LINE_WIDTH));
                // Points, while there is room to tell them apart.
                if *dots {
                    for &pt in points {
                        cx.fill(&Circle::new(pt, chart_draw::DOT_RADIUS), mark, 0.0);
                    }
                }
            }
            chart_draw::Marks::Bars(bars) => {
                for b in bars {
                    cx.fill(&rect(b), mark, 0.0);
                }
            }
            chart_draw::Marks::Dots(points) => {
                let dot = mark.multiply_alpha(chart_draw::DOT_OPACITY as f32);
                for &pt in points {
                    cx.fill(&Circle::new(pt, chart_draw::DOT_RADIUS), dot, 0.0);
                }
            }
            chart_draw::Marks::Stacks(segments) => {
                for (stack, b) in segments {
                    let fill = *chart_draw::stack_fill(&scene, *stack, &palette, &mark, &other);
                    cx.fill(&rect(b), fill, 0.0);
                }
            }
        }
        // The zero line, over the marks.
        cx.stroke(
            &Line::new((p.x0, scene.base), (p.x1, scene.base)),
            theme::text_faint(),
            &Stroke::new(1.0),
        );
        // The labels under the axis, kept inside the canvas.
        for l in &scene.under {
            cx.draw_text(
                &layout(&l.text, font, theme::text_dim),
                Point::new(l.left, l.top),
            );
        }
        // A stacked bar's legend: a swatch in each stack's colour, its name
        // beside it.
        for e in &scene.legend {
            let fill = *chart_draw::stack_fill(&scene, e.stack, &palette, &mark, &other);
            cx.fill(&rect(&e.swatch).to_rounded_rect(2.0), fill, 0.0);
            cx.draw_text(
                &layout(&e.label.text, font, theme::text_dim),
                Point::new(e.label.left, e.label.top),
            );
        }
    }
}

/// A label's box at `size`, by floem's text engine — the measurer
/// `chart_draw::layout` takes, for the canvas and the export alike.
fn measure(text: &str, size: f32) -> (f64, f64) {
    let s = layout(text, size, theme::text).size();
    (s.width, s.height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemaic_core::chart::Series;

    /// **The widest picker row fits inside the modal.** A stacked bar shows
    /// four pickers — kind, values, labels, stack by — and the export icon;
    /// at three 200px column pickers that row needed ~848px of the 732px
    /// inside a 760px panel's padding, so *Stack by* ran past the modal's edge
    /// and the icon sat outside it. Unscaled widths: every one is multiplied
    /// by the same UI scale.
    #[test]
    fn the_stacked_bar_picker_row_fits_the_panel() {
        let row = KIND_W + 3.0 * COL_W + EXPORT_ALLOWANCE_W + 5.0 * ROW_GAP;
        let inside = PANEL_W - 2.0 * crate::widgets::MODAL_PAD_H;
        assert!(row <= inside, "row {row} > room {inside}");
    }

    /// English names each chart kind as core's `ChartKind::label` does.
    #[test]
    fn english_names_every_chart_kind_as_core_does() {
        for kind in ChartKind::ALL {
            assert_eq!(chart_kind_label(kind), kind.label(), "{kind:?}");
        }
    }

    /// English captions every chart as core's `chart::caption` does — one
    /// row and several, each kind's skipped note, and a capped read.
    #[test]
    fn english_captions_every_chart_as_core_does() {
        for kind in ChartKind::ALL {
            for (rows, skipped, truncated) in [(1, 0, false), (42, 3, false), (5000, 0, true)] {
                assert_eq!(
                    caption_text(rows, kind, skipped, truncated),
                    chart::caption(rows, kind, skipped, truncated),
                    "{kind:?} {rows} {skipped} {truncated}"
                );
            }
        }
    }

    /// The undrawable note reads as core's English.
    #[test]
    fn english_refuses_an_undrawable_chart_as_core_does() {
        let huge = Plot::Series(
            ChartKind::Line,
            Series {
                labels: vec!["a".into(), "b".into()],
                values: vec![-f64::MAX, f64::MAX],
                skipped: 0,
            },
        );
        assert_eq!(huge.undrawable(), Some(tr!("chart-too-large").as_str()));
    }

    fn bars() -> Plot {
        Plot::Series(
            ChartKind::Bar,
            Series {
                labels: vec!["a".into(), "b".into()],
                values: vec![1.0, 2.0],
                skipped: 0,
            },
        )
    }

    /// The save's whole picture path: a plot → its document → the worker's
    /// bytes. A PNG at the fixed export size, padding included, whatever the
    /// canvas on screen measures — and an SVG of the same chart.
    #[test]
    fn a_saved_chart_is_a_png_of_the_export_size_and_an_svg() {
        let Some(png) = export_doc(&bars(), ErdExportFormat::Png) else {
            panic!("a drawable chart has a PNG");
        };
        let png = png.into_bytes().expect("rasterises");
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "PNG magic");
        let w = u32::from_be_bytes(png[16..20].try_into().unwrap());
        let h = u32::from_be_bytes(png[20..24].try_into().unwrap());
        let scale = f64::from(EXPORT_PNG_SCALE);
        assert_eq!(
            (w, h),
            (
                ((EXPORT_W + 2.0 * EXPORT_PAD) * scale) as u32,
                ((EXPORT_H + 2.0 * EXPORT_PAD) * scale) as u32
            )
        );

        let Some(svg) =
            export_doc(&bars(), ErdExportFormat::Svg).and_then(crate::ErdDoc::into_text)
        else {
            panic!("a drawable chart has an SVG");
        };
        assert!(svg.starts_with("<svg"), "{svg}");
        assert_eq!(svg.matches("<rect").count(), 3, "background + two bars");
    }

    /// The folded segment is named in the interface's words, which are core's
    /// in English — and a saved stacked bar draws each segment and its legend
    /// swatch in the palette.
    #[test]
    fn a_stacked_bar_names_its_other_segment_and_saves_in_the_palette() {
        use schemaic_core::model::{Column, ResultBuilder, Value};
        let mut b = ResultBuilder::new(
            ["l", "by", "v"]
                .iter()
                .map(|n| Column {
                    name: n.to_string(),
                    type_name: String::new(),
                    origin: None,
                })
                .collect(),
        );
        for i in 0..chart::MAX_STACKS + 2 {
            b.push_row(&[
                Value::Str("a".into()),
                Value::Str(format!("s{i}")),
                Value::Int(1),
            ]);
        }
        let rs = b.finish();
        let picks = chart::Picks {
            value: 2,
            label: Some(0),
            by: Some(1),
            ..chart::Picks::default()
        };
        let (drawn, _) = plot_of(&rs, ChartKind::StackedBar, picks);
        let Plot::Stacked(s) = &drawn else {
            panic!("a stacked bar");
        };
        assert_eq!(s.stacks.last().map(String::as_str), Some(chart::OTHER));
        assert_eq!(tr!("chart-other"), chart::OTHER);

        let svg = export_doc(&drawn, ErdExportFormat::Svg)
            .and_then(crate::ErdDoc::into_text)
            .expect("an SVG");
        let first = crate::erd_view::hex(stack_palette()[0]);
        assert_eq!(svg.matches(&format!("fill=\"{first}\"")).count(), 2);
    }

    /// Each kind shows the pickers it reads, and no other.
    #[test]
    fn each_kind_shows_the_pickers_it_reads() {
        assert!(extra_of(ChartKind::Histogram) == Extra::Nothing);
        assert!(extra_of(ChartKind::Bar) == Extra::Labels);
        assert!(extra_of(ChartKind::Line) == Extra::Labels);
        assert!(extra_of(ChartKind::Scatter) == Extra::XAxis);
        assert!(extra_of(ChartKind::StackedBar) == Extra::LabelsAndStacks);
    }

    /// Only the two pictures are offered; a text format is no chart.
    #[test]
    fn a_chart_has_no_text_format_and_an_empty_plot_has_no_file() {
        assert!(export_doc(&bars(), ErdExportFormat::Mermaid).is_none());
        assert!(export_doc(&Plot::Bins(Vec::new()), ErdExportFormat::Png).is_none());
    }
}
