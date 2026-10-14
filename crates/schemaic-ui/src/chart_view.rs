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
use floem::text::{Attrs, AttrsList, TextLayout};
use floem::{View, ViewId};
use floem_renderer::Renderer;

use schemaic_core::chart::{self, Bin, ChartKind, Series};
use schemaic_core::model::ResultSet;

use crate::settings::focusable_dropdown;
use crate::widgets::{
    ACTION_TAB, ActionKind, FocusRing, action_button, dismiss_layer, focus_root_with_ring,
    modal_footer_split, modal_pad_h, modal_title_owned, panel_style,
};
use crate::{OverlayUi, theme};

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
const ROW_NUMBER: &str = "Row number";

pub(crate) fn chart_overlay(overlay: OverlayUi) -> impl IntoView {
    let target = overlay.chart;
    dyn_container(
        move || target.get().map(|t| (Arc::as_ptr(&t.rs) as usize, t)),
        move |open| {
            let Some((_, t)) = open else {
                return empty().into_any();
            };
            let close: Rc<dyn Fn()> = Rc::new(move || target.set(None));
            let ring = FocusRing::new();
            let rs = t.rs.clone();
            let numeric = chart::numeric_columns(&rs);
            let names: Vec<String> = rs.columns.iter().map(|c| c.name.clone()).collect();

            // The choices. The value column starts on the last numeric one —
            // the measure usually follows its labels — and the label column on
            // the first column that is not it.
            let kind = RwSignal::new(ChartKind::Bar);
            let value = RwSignal::new(numeric.last().copied());
            let label =
                RwSignal::new((0..names.len()).find(|c| Some(*c) != numeric.last().copied()));

            let controls = controls(&names, &numeric, kind, value, label, ring.clone());
            let body = {
                let rs = rs.clone();
                dyn_container(
                    move || (kind.get(), value.get(), label.get()),
                    move |(k, v, l)| chart_body(&rs, k, v, l),
                )
                .style(|s| s.width_full())
            };
            let title =
                modal_title_owned(format!("Chart — {}", t.title), close.clone(), ring.clone());
            let done = close.clone();
            let footer = modal_footer_split(
                empty(),
                action_button(
                    "Close",
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

/// The three pickers: kind, value column, label column (not for a histogram).
fn controls(
    names: &[String],
    numeric: &[usize],
    kind: RwSignal<ChartKind>,
    value: RwSignal<Option<usize>>,
    label: RwSignal<Option<usize>>,
    ring: FocusRing,
) -> AnyView {
    let caption = |s: &'static str| text(s).style(crate::widgets::form_label_style);
    let col_w = || theme::scaled(200.0);
    // A picked name → the first column of that name; two columns sharing a
    // name pick the first.
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
    let mut label_options = vec![ROW_NUMBER.to_string()];
    label_options.extend(names.iter().cloned());
    let label_ring = ring.clone();
    let label_group = dyn_container(
        move || kind.get() == ChartKind::Histogram,
        move |histogram| {
            if histogram {
                return empty().into_any();
            }
            let lookup = lookup.clone();
            let field = crate::table_designer::focusable_owned_dropdown(
                move || {
                    label
                        .get()
                        .map(name_of)
                        .unwrap_or_else(|| ROW_NUMBER.to_string())
                },
                label_options.clone(),
                col_w,
                label_ring.clone(),
                30,
                move |picked| label.set(lookup(&picked).filter(|_| picked != ROW_NUMBER)),
            );
            v_stack((caption("Labels"), field))
                .style(|s| s.flex_col().gap(theme::scaled(6.0)))
                .into_any()
        },
    );
    h_stack((
        v_stack((
            caption("Chart"),
            focusable_dropdown(kind, ChartKind::ALL, ChartKind::label, ring, 10)
                .style(|s| s.width(theme::scaled(140.0))),
        ))
        .style(|s| s.flex_col().gap(theme::scaled(6.0))),
        v_stack((caption("Values"), value_field)).style(|s| s.flex_col().gap(theme::scaled(6.0))),
        label_group,
    ))
    .style(|s| s.gap(theme::scaled(16.0)).items_start())
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
        return note("No column of numbers to chart in this result.".to_string());
    };
    let rows = rs.row_count();
    let (drawn, skipped) = match kind {
        ChartKind::Histogram => {
            let (bins, skipped) = chart::histogram(rs, v);
            (Drawn::Bins(bins), skipped)
        }
        ChartKind::Bar | ChartKind::Line => {
            let s = chart::series(rs, label, v);
            let skipped = s.skipped;
            (Drawn::Series(kind, s), skipped)
        }
    };
    if drawn.is_empty() {
        return note("Every value in that column is NULL.".to_string());
    }
    let mut line = format!(
        "{} {}",
        rows,
        schemaic_core::text::plural(rows, "row", "rows")
    );
    if skipped > 0 {
        line.push_str(&format!(
            " · {skipped} not drawn (NULL, or past {})",
            chart::MAX_POINTS
        ));
    }
    if rs.truncated {
        line.push_str(" · the rows read, not the whole result");
    }
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

/// What the canvas paints.
enum Drawn {
    Series(ChartKind, Series),
    Bins(Vec<Bin>),
}

impl Drawn {
    fn is_empty(&self) -> bool {
        match self {
            Drawn::Series(_, s) => s.values.is_empty(),
            Drawn::Bins(b) => b.is_empty(),
        }
    }

    /// The value axis's range: a bar or bin stands on 0, so 0 is in it.
    fn range(&self) -> (f64, f64) {
        let (lo, hi, zero) = match self {
            Drawn::Series(k, s) => (
                s.values.iter().copied().fold(f64::INFINITY, f64::min),
                s.values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                *k == ChartKind::Bar,
            ),
            Drawn::Bins(b) => (
                0.0,
                b.iter().map(|b| b.count).max().unwrap_or(0) as f64,
                true,
            ),
        };
        if zero {
            (lo.min(0.0), hi.max(0.0))
        } else {
            (lo, hi)
        }
    }

    /// The labels under the x axis: first, middle and last.
    fn x_labels(&self) -> Vec<(f64, String)> {
        let cut = |s: &str| -> String {
            if s.chars().count() > 18 {
                format!("{}…", s.chars().take(17).collect::<String>())
            } else {
                s.to_string()
            }
        };
        match self {
            Drawn::Series(_, s) => {
                let n = s.labels.len();
                let mut picks = vec![0];
                if n > 2 {
                    picks.push(n / 2);
                }
                if n > 1 {
                    picks.push(n - 1);
                }
                picks
                    .into_iter()
                    .map(|i| ((i as f64 + 0.5) / n as f64, cut(&s.labels[i])))
                    .collect()
            }
            Drawn::Bins(b) => {
                let (first, last) = (b[0].lo, b[b.len() - 1].hi);
                let mid = (first + last) / 2.0;
                vec![
                    (0.0, chart::tick_label(first)),
                    (0.5, chart::tick_label(mid)),
                    (1.0, chart::tick_label(last)),
                ]
            }
        }
    }
}

/// The painted chart: grid lines and value labels on the left, the marks,
/// and three labels under the axis. Colours are read at paint time, so a
/// theme switch repaints in the new ones.
struct ChartCanvas {
    id: ViewId,
    drawn: Rc<Drawn>,
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
        let (lo, hi) = self.drawn.range();
        let ticks = chart::ticks(lo, hi, 5);
        let (Some(&t0), Some(&t1)) = (ticks.first(), ticks.last()) else {
            return;
        };
        let labels: Vec<TextLayout> = ticks
            .iter()
            .map(|t| layout(&chart::tick_label(*t), font, theme::text_faint))
            .collect();
        let gutter = labels.iter().map(|l| l.size().width).fold(0.0, f64::max) + theme::scaled(8.0);
        let label_h = labels.first().map_or(12.0, |l| l.size().height);
        let plot = Rect::new(
            gutter,
            label_h / 2.0,
            size.width - theme::scaled(8.0),
            size.height - label_h - theme::scaled(8.0),
        );
        if plot.width() <= 0.0 || plot.height() <= 0.0 || t1 <= t0 {
            return;
        }
        let y_of = |v: f64| plot.y1 - (v - t0) / (t1 - t0) * plot.height();

        // Grid lines and their values.
        let mut grid = BezPath::new();
        for (t, l) in ticks.iter().zip(&labels) {
            let y = y_of(*t);
            grid.move_to((plot.x0, y));
            grid.line_to((plot.x1, y));
            let w = l.size().width;
            cx.draw_text(
                l,
                Point::new(gutter - theme::scaled(6.0) - w, y - label_h / 2.0),
            );
        }
        cx.stroke(&grid, theme::border(), &Stroke::new(1.0));

        let mark = theme::accent();
        let base = y_of(0.0_f64.clamp(t0, t1));
        match self.drawn.as_ref() {
            Drawn::Series(ChartKind::Line, s) => {
                let n = s.values.len();
                let x_of = |i: usize| plot.x0 + (i as f64 + 0.5) / n as f64 * plot.width();
                let mut path = BezPath::new();
                for (i, v) in s.values.iter().enumerate() {
                    let p = Point::new(x_of(i), y_of(*v));
                    if i == 0 {
                        path.move_to(p)
                    } else {
                        path.line_to(p)
                    }
                }
                cx.stroke(&path, mark, &Stroke::new(2.0));
                // Points, while there is room to tell them apart.
                if n <= 60 {
                    for (i, v) in s.values.iter().enumerate() {
                        cx.fill(&Circle::new((x_of(i), y_of(*v)), 3.0), mark, 0.0);
                    }
                }
            }
            Drawn::Series(_, s) => {
                let n = s.values.len();
                let slot = plot.width() / n as f64;
                let gap = (slot * 0.2).min(theme::scaled(6.0));
                for (i, v) in s.values.iter().enumerate() {
                    let x = plot.x0 + slot * i as f64 + gap / 2.0;
                    let y = y_of(*v);
                    let r = Rect::new(x, y.min(base), x + (slot - gap).max(1.0), y.max(base));
                    cx.fill(&r, mark, 0.0);
                }
            }
            Drawn::Bins(bins) => {
                let n = bins.len();
                let slot = plot.width() / n as f64;
                for (i, b) in bins.iter().enumerate() {
                    let x = plot.x0 + slot * i as f64;
                    let y = y_of(b.count as f64);
                    let r = Rect::new(x + 0.5, y, x + (slot - 1.0).max(1.0), base);
                    cx.fill(&r, mark, 0.0);
                }
            }
        }
        // The zero line, over the marks.
        cx.stroke(
            &Line::new((plot.x0, base), (plot.x1, base)),
            theme::text_faint(),
            &Stroke::new(1.0),
        );
        // The labels under the axis, kept inside the canvas.
        for (at, text) in self.drawn.x_labels() {
            let l = layout(&text, font, theme::text_dim);
            let w = l.size().width;
            let x = (plot.x0 + at * plot.width() - w / 2.0).clamp(0.0, (size.width - w).max(0.0));
            cx.draw_text(&l, Point::new(x, plot.y1 + theme::scaled(4.0)));
        }
    }
}
