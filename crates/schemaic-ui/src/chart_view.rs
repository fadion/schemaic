//! **Chart** — a bar, line or histogram of the result on screen
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

/// Modal width and the canvas's height.
fn panel_w() -> f64 {
    theme::scaled(760.0)
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
    }
}

/// The line under the canvas — core's `chart::caption` in the interface's
/// language; `english_captions_every_chart_as_core_does` holds them equal.
fn caption_text(rows: usize, kind: ChartKind, skipped: usize, truncated: bool) -> String {
    let mut parts = vec![tr!("chart-caption-rows", n = rows)];
    if skipped > 0 {
        parts.push(match kind {
            ChartKind::Histogram => tr!("chart-caption-skipped-null", skipped = skipped),
            ChartKind::Bar | ChartKind::Line => tr!(
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

/// The save icon's place in the Tab ring: after the three pickers (10, 20, 30).
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
/// size: a series draws at most [`chart::MAX_POINTS`] marks and a histogram
/// at most 50 bins. A PNG's rasterise is left to the worker.
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
            let value = RwSignal::new(numeric.last().copied());
            let label =
                RwSignal::new((0..names.len()).find(|c| Some(*c) != numeric.last().copied()));

            let body = {
                let rs = rs.clone();
                dyn_container(
                    move || (kind.get(), value.get(), label.get()),
                    move |(k, v, l)| chart_body(&rs, k, v, l),
                )
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
                let _ = (kind.get(), value.get(), label.get());
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
                    let (Some(k), Some(v), Some(l)) = (
                        kind.try_get_untracked(),
                        value.try_get_untracked(),
                        label.try_get_untracked(),
                    ) else {
                        return;
                    };
                    let doc = v.and_then(|v| {
                        let (drawn, _) = Plot::of(&rs, k, v, l);
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
                kind,
                value,
                label,
                ring.clone(),
                export_button,
            );

            let status = dyn_container(
                move || notice.get(),
                move |n| match n {
                    None => empty().into_any(),
                    Some((msg, true)) => crate::widgets::footer_error(msg),
                    Some((msg, false)) => text(msg)
                        .style(|s| s.color(theme::text_dim()).font_size(theme::font_label()))
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

/// The three pickers — kind, value column, label column (not for a
/// histogram) — and the export icon after them.
fn controls(
    names: &[String],
    numeric: &[usize],
    kind: RwSignal<ChartKind>,
    value: RwSignal<Option<usize>>,
    label: RwSignal<Option<usize>>,
    ring: FocusRing,
    export: AnyView,
) -> AnyView {
    let caption = |s: String| text(s).style(crate::widgets::form_label_style);
    let col_w = || theme::scaled(200.0);
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
    // A histogram's labels are its bins, so the picker is not there at all —
    // built or not rather than hidden, since a hidden control is still a stop
    // in the modal's Tab ring.
    let row_number = row_number();
    let mut label_options = vec![row_number.clone()];
    label_options.extend(names.iter().cloned());
    let label_ring = ring.clone();
    let label_group = dyn_container(
        move || kind.get() == ChartKind::Histogram,
        move |histogram| {
            if histogram {
                return empty().into_any();
            }
            let lookup = lookup.clone();
            let picked_none = row_number.clone();
            let field = crate::table_designer::focusable_owned_dropdown(
                // A fn item, not a captured `String`: the picker wants a
                // `Copy` closure.
                move || label.get().map(name_of).unwrap_or_else(self::row_number),
                label_options.clone(),
                col_w,
                label_ring.clone(),
                30,
                move |picked| label.set(lookup(&picked).filter(|_| picked != picked_none)),
            );
            v_stack((caption(tr!("chart-caption-labels")), field))
                .style(|s| s.flex_col().gap(theme::scaled(6.0)))
                .into_any()
        },
    );
    h_stack((
        v_stack((
            caption(tr!("chart-caption-kind")),
            focusable_dropdown(kind, ChartKind::ALL, chart_kind_label, ring, 10)
                .style(|s| s.width(theme::scaled(140.0))),
        ))
        .style(|s| s.flex_col().gap(theme::scaled(6.0))),
        v_stack((caption(tr!("chart-caption-values")), value_field))
            .style(|s| s.flex_col().gap(theme::scaled(6.0))),
        label_group,
        // Pushed to the row's right end, and centred on the **dropdowns**
        // rather than the row: under a blank caption, the pickers' own
        // caption and gap, so the row's centring puts the icon's middle on
        // theirs at any UI scale without a measured nudge.
        empty().style(|s| s.flex_grow(1.0_f32)),
        v_stack((caption(" ".to_string()), export))
            .style(|s| s.flex_col().items_center().gap(theme::scaled(6.0))),
    ))
    .style(|s| s.gap(theme::scaled(16.0)).items_center().width_full())
    .into_any()
}

/// The canvas and the line under it — or why there is nothing to draw.
fn chart_body(
    rs: &ResultSet,
    kind: ChartKind,
    value: Option<usize>,
    label: Option<usize>,
) -> AnyView {
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
    let Some(v) = value else {
        return note(tr!("chart-no-numbers"));
    };
    let (drawn, skipped) = Plot::of(rs, kind, v, label);
    if drawn.is_empty() {
        return note(tr!("chart-all-null"));
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
/// and three labels under the axis. Colours are read at paint time, so a
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
        cx.stroke(&grid, theme::border(), &Stroke::new(1.0));
        for (l, text) in scene.ticks.iter().zip(&shaped) {
            cx.draw_text(text, Point::new(l.left, l.top));
        }

        let mark = theme::accent();
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

    /// Only the two pictures are offered; a text format is no chart.
    #[test]
    fn a_chart_has_no_text_format_and_an_empty_plot_has_no_file() {
        assert!(export_doc(&bars(), ErdExportFormat::Mermaid).is_none());
        assert!(export_doc(&Plot::Bins(Vec::new()), ErdExportFormat::Png).is_none());
    }
}
