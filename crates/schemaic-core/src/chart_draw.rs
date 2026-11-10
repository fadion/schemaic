//! **Where a chart's marks go** — the geometry of a [`Plot`] in a box of a
//! given size, and that geometry written out as an SVG document.
//!
//! One layout, two painters: the chart modal's canvas draws a [`Scene`] and
//! its save icon writes the same [`Scene`] through [`to_svg`] (a PNG is
//! that document rasterised — the ER diagram's export path), so the file is the
//! chart on screen rather than a second drawing of it that drifts.
//!
//! Text is measured by the caller: the widths come from the app's font system,
//! which this crate does not have, so [`layout`] takes a measurer the way
//! `erd_export` takes its card metrics.

use std::fmt::Write as _;

use crate::chart::{self, ChartKind, Plot};
use crate::erd_export::{n, svg_text};

/// A line chart's stroke width.
pub const LINE_WIDTH: f64 = 2.0;
/// A line chart's point radius.
pub const DOT_RADIUS: f64 = 3.0;
/// The most points a line chart marks with a dot: past it they run together.
pub const MAX_DOTS: usize = 60;

/// The spacing the layout uses — parameters rather than constants, because the
/// canvas scales them with the UI and the export draws at a fixed size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    /// Room between the widest tick label and the plot, beyond its own width.
    pub gutter_pad: f64,
    /// Room left of the right and bottom edges.
    pub inset: f64,
    /// Between a tick label's right edge and the plot.
    pub tick_gap: f64,
    /// Between the plot's bottom edge and the labels under it.
    pub under_gap: f64,
    /// The widest gap between two bars.
    pub bar_gap_max: f64,
    /// Between two legend entries, and above the legend.
    pub legend_gap: f64,
}

impl Metrics {
    /// The spacing at UI scale 1 — the export's, and the canvas's before it
    /// scales each one (`chart_view`).
    pub const BASE: Metrics = Metrics {
        gutter_pad: 8.0,
        inset: 8.0,
        tick_gap: 6.0,
        under_gap: 4.0,
        bar_gap_max: 6.0,
        legend_gap: 12.0,
    };

    /// Each spacing through `f` — the UI's scale, which rounds per value and
    /// so is not one factor to multiply by.
    pub fn map(self, f: impl Fn(f64) -> f64) -> Metrics {
        Metrics {
            gutter_pad: f(self.gutter_pad),
            inset: f(self.inset),
            tick_gap: f(self.tick_gap),
            under_gap: f(self.under_gap),
            bar_gap_max: f(self.bar_gap_max),
            legend_gap: f(self.legend_gap),
        }
    }
}

/// An axis-aligned box, `x0 <= x1` and `y0 <= y1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Area {
    pub fn width(&self) -> f64 {
        self.x1 - self.x0
    }
    pub fn height(&self) -> f64 {
        self.y1 - self.y0
    }
}

/// Which edge of a label its position names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Start,
    End,
}

/// A piece of text placed: its measured box's left and top, and which edge
/// is the one that matters — a tick label is right-aligned to the plot, and
/// an SVG renderer whose glyphs are a hair wider than the canvas's still
/// lines it up there.
#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    pub left: f64,
    pub top: f64,
    pub w: f64,
    pub h: f64,
    pub text: String,
    pub align: Align,
}

/// What is drawn on the plot.
#[derive(Clone, Debug, PartialEq)]
pub enum Marks {
    /// A line through `points`, with a dot on each when `dots`.
    Line { points: Vec<(f64, f64)>, dots: bool },
    /// Bars or bins.
    Bars(Vec<Area>),
    /// A scatter's dots, one per pair, at their centres.
    Dots(Vec<(f64, f64)>),
    /// A stacked bar's segments, each with the stack it is a piece of.
    Stacks(Vec<(usize, Area)>),
}

/// One entry of a stacked bar's legend: a swatch in its stack's colour and
/// the stack's name beside it.
#[derive(Clone, Debug, PartialEq)]
pub struct LegendEntry {
    pub stack: usize,
    pub swatch: Area,
    pub label: Label,
}

/// A chart laid out in a `width` × `height` box.
#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    pub width: f64,
    pub height: f64,
    /// The plot area: the grid's extent, inside the tick labels.
    pub plot: Area,
    /// One horizontal grid line per tick, by its y.
    pub grid: Vec<f64>,
    /// One vertical grid line per X tick, by its x — a scatter's only.
    pub vgrid: Vec<f64>,
    pub ticks: Vec<Label>,
    pub marks: Marks,
    /// The zero line's y — or the axis's nearer end when 0 is off it.
    pub base: f64,
    /// The labels under the axis, kept inside the box.
    pub under: Vec<Label>,
    /// A stacked bar's legend, under the labels — empty for one stack, and for
    /// every other kind.
    pub legend: Vec<LegendEntry>,
    /// The stack that folds those past `chart::MAX_STACKS`, drawn in its own
    /// quiet colour ([`stack_fill`]).
    pub other_stack: Option<usize>,
}

/// The colour a stacked bar's segment `stack` is drawn in: the `mark` colour
/// when the chart has a single stack (it is then a bar chart), the folded
/// stack's `other`, and otherwise the `palette` in turn. One answer for the
/// canvas and the export, so the file's colours are the screen's.
pub fn stack_fill<'a, T>(
    scene: &Scene,
    stack: usize,
    palette: &'a [T],
    mark: &'a T,
    other: &'a T,
) -> &'a T {
    if scene.other_stack == Some(stack) {
        other
    } else if scene.legend.is_empty() || palette.is_empty() {
        mark
    } else {
        &palette[stack % palette.len()]
    }
}

/// `drawn` laid out in a `width` × `height` box, with text measured by
/// `measure` (`text → (width, height)`). `None` when there is nothing to draw:
/// no values, no axis for them ([`Plot::undrawable`]), or a box too small for
/// one.
///
/// **`measure` is called for each of [`Scene::ticks`] first, in order, then
/// for each label under the axis, then for each of [`Scene::legend`]** — a
/// contract, not an accident: the canvas keeps what it measured and draws the
/// tick labels from it, by position. (A scatter's labels under the axis are
/// its X ticks, and one that would overlap its neighbour is measured and then
/// left out, so [`Scene::under`] can be fewer than were measured.)
///
/// The tick labels are written in `figures`, the interface's marks — the
/// export is a picture of the chart on screen, not data, so it carries them
/// too.
pub fn layout(
    drawn: &Plot,
    width: f64,
    height: f64,
    m: &Metrics,
    figures: crate::text::NumberMarks,
    measure: &mut dyn FnMut(&str) -> (f64, f64),
) -> Option<Scene> {
    // Every mark's width below divides by the count.
    if drawn.is_empty() {
        return None;
    }
    let (lo, hi) = drawn.range();
    let ticks = chart::ticks(lo, hi, 5);
    let (&t0, &t1) = (ticks.first()?, ticks.last()?);
    // As one axis, so their precision follows the step (`tick_labels`).
    let texts: Vec<String> = chart::tick_labels(&ticks)
        .iter()
        .map(|t| crate::text::remark(t, figures))
        .collect();
    let sizes: Vec<(f64, f64)> = texts.iter().map(|t| measure(t)).collect();
    // A histogram's are bin edges and a scatter's X ticks, figures like the
    // value ticks; a series' are the label column's values — data, never
    // re-marked.
    let figured = matches!(drawn, Plot::Bins(_) | Plot::Points(_));
    let under_texts: Vec<(f64, String, (f64, f64))> = drawn
        .x_labels()
        .into_iter()
        .map(|(at, text)| {
            let text = if figured {
                crate::text::remark(&text, figures)
            } else {
                text
            };
            let size = measure(&text);
            (at, text, size)
        })
        .collect();
    // A legend only where there is more than one stack to tell apart.
    let legend_texts: Vec<(String, (f64, f64))> = match drawn {
        Plot::Stacked(s) if s.stacks.len() > 1 => s
            .stacks
            .iter()
            .map(|name| (name.clone(), measure(name)))
            .collect(),
        _ => Vec::new(),
    };
    let gutter = sizes.iter().map(|s| s.0).fold(0.0, f64::max) + m.gutter_pad;
    let label_h = sizes.first().map_or(12.0, |s| s.1);

    // The legend's entries, flowed left to right from the plot's left edge and
    // wrapped at the right one; its rows' height comes off the plot's.
    let swatch = (label_h * 0.75).round();
    let right = width - m.inset;
    let mut placed: Vec<(usize, f64, usize, f64, f64)> = Vec::new(); // (stack, x, row, w, h)
    let (mut x, mut row) = (gutter, 0usize);
    for (i, (_, (w, h))) in legend_texts.iter().enumerate() {
        let entry = swatch + m.tick_gap + w;
        if x > gutter && x + entry > right {
            row += 1;
            x = gutter;
        }
        placed.push((i, x, row, *w, *h));
        x += entry + m.legend_gap;
    }
    let row_h = label_h + m.under_gap;
    let legend_h = if placed.is_empty() {
        0.0
    } else {
        m.legend_gap + (row + 1) as f64 * row_h - m.under_gap
    };

    let plot = Area {
        x0: gutter,
        y0: label_h / 2.0,
        x1: right,
        y1: height - label_h - m.inset - legend_h,
    };
    if plot.width() <= 0.0 || plot.height() <= 0.0 || t1 <= t0 {
        return None;
    }
    let y_of = |v: f64| plot.y1 - (v - t0) / (t1 - t0) * plot.height();
    // A scatter's x is a number line ([`Plot::x_range`]); every other kind's
    // is a slot per label.
    let x_axis = drawn.x_range();
    if matches!(drawn, Plot::Points(_)) && x_axis.is_none() {
        return None;
    }
    let x_of_value = |v: f64| match x_axis {
        Some((a0, a1)) if a1 > a0 => plot.x0 + (v - a0) / (a1 - a0) * plot.width(),
        _ => plot.x0 + plot.width() / 2.0,
    };

    let grid: Vec<f64> = ticks.iter().map(|t| y_of(*t)).collect();
    let tick_labels = texts
        .into_iter()
        .zip(&sizes)
        .zip(&grid)
        .map(|((text, &(w, h)), &y)| Label {
            left: gutter - m.tick_gap - w,
            top: y - label_h / 2.0,
            w,
            h,
            text,
            align: Align::End,
        })
        .collect();

    let base = y_of(0.0_f64.clamp(t0, t1));
    let marks = match drawn {
        Plot::Series(ChartKind::Line, s) => {
            let count = s.values.len();
            let x_of = |i: usize| plot.x0 + (i as f64 + 0.5) / count as f64 * plot.width();
            Marks::Line {
                points: s
                    .values
                    .iter()
                    .enumerate()
                    .map(|(i, v)| (x_of(i), y_of(*v)))
                    .collect(),
                dots: count <= MAX_DOTS,
            }
        }
        Plot::Series(_, s) => {
            let slot = plot.width() / s.values.len() as f64;
            let gap = (slot * 0.2).min(m.bar_gap_max);
            Marks::Bars(
                s.values
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        let x = plot.x0 + slot * i as f64 + gap / 2.0;
                        let y = y_of(*v);
                        Area {
                            x0: x,
                            y0: y.min(base),
                            x1: x + (slot - gap).max(1.0),
                            y1: y.max(base),
                        }
                    })
                    .collect(),
            )
        }
        Plot::Bins(bins) => {
            let slot = plot.width() / bins.len() as f64;
            Marks::Bars(
                bins.iter()
                    .enumerate()
                    .map(|(i, b)| {
                        let x = plot.x0 + slot * i as f64;
                        Area {
                            x0: x + 0.5,
                            y0: y_of(b.count as f64),
                            x1: x + (slot - 1.0).max(1.0),
                            y1: base,
                        }
                    })
                    .collect(),
            )
        }
        Plot::Points(p) => Marks::Dots(
            p.xs.iter()
                .zip(&p.ys)
                .map(|(x, y)| (x_of_value(*x), y_of(*y)))
                .collect(),
        ),
        // Positive segments stack up from zero and negative ones down from
        // it, in stack order, so a stack sits at the same place in every bar
        // that has it.
        Plot::Stacked(s) => {
            let slot = plot.width() / s.labels.len() as f64;
            let gap = (slot * 0.2).min(m.bar_gap_max);
            let mut segments = Vec::new();
            for (i, bar) in s.values.iter().enumerate() {
                let x = plot.x0 + slot * i as f64 + gap / 2.0;
                let (mut up, mut down) = (0.0_f64, 0.0_f64);
                for (stack, v) in bar.iter().enumerate() {
                    if *v == 0.0 {
                        continue;
                    }
                    let side = if *v > 0.0 { &mut up } else { &mut down };
                    let (from, to) = (*side, *side + v);
                    *side = to;
                    let (ya, yb) = (y_of(from), y_of(to));
                    segments.push((
                        stack,
                        Area {
                            x0: x,
                            y0: ya.min(yb),
                            x1: x + (slot - gap).max(1.0),
                            y1: ya.max(yb),
                        },
                    ));
                }
            }
            Marks::Stacks(segments)
        }
    };

    let top = plot.y1 + m.under_gap;
    let mut under: Vec<Label> = under_texts
        .into_iter()
        .map(|(at, text, (w, h))| Label {
            left: (plot.x0 + at * plot.width() - w / 2.0).clamp(0.0, (width - w).max(0.0)),
            top,
            w,
            h,
            text,
            align: Align::Start,
        })
        .collect();
    let vgrid: Vec<f64> = match drawn {
        Plot::Points(_) => drawn
            .x_labels()
            .iter()
            .map(|(at, _)| plot.x0 + at * plot.width())
            .collect(),
        _ => Vec::new(),
    };
    // A scatter labels every X tick, and a long one can reach its neighbour:
    // every other one is kept until none touch.
    if matches!(drawn, Plot::Points(_)) {
        let overlaps = |ls: &[Label]| {
            ls.windows(2)
                .any(|w| w[0].left + w[0].w + m.tick_gap > w[1].left)
        };
        while under.len() > 2 && overlaps(&under) {
            under = under.into_iter().step_by(2).collect();
        }
    }

    let legend_top = top + label_h + m.legend_gap;
    let legend = placed
        .into_iter()
        .zip(legend_texts)
        .map(|((stack, x, row, w, h), (text, _))| {
            let row_top = legend_top + row as f64 * row_h;
            LegendEntry {
                stack,
                swatch: Area {
                    x0: x,
                    y0: row_top + (label_h - swatch) / 2.0,
                    x1: x + swatch,
                    y1: row_top + (label_h + swatch) / 2.0,
                },
                label: Label {
                    left: x + swatch + m.tick_gap,
                    top: row_top,
                    w,
                    h,
                    text,
                    align: Align::Start,
                },
            }
        })
        .collect();
    let other_stack = match drawn {
        Plot::Stacked(s) if s.other => Some(s.stacks.len() - 1),
        _ => None,
    };

    Some(Scene {
        width,
        height,
        plot,
        grid,
        vgrid,
        ticks: tick_labels,
        marks,
        base,
        under,
        legend,
        other_stack,
    })
}

/// The colours an exported chart is drawn in, as SVG colour strings — the
/// theme's at the moment of the export, the way the ER diagram's file is.
#[derive(Clone, Debug, PartialEq)]
pub struct SvgColors {
    pub background: String,
    pub grid: String,
    pub tick_text: String,
    pub under_text: String,
    pub mark: String,
    pub zero: String,
    /// A stacked bar's segment colours, in stack order ([`stack_fill`]).
    pub stacks: Vec<String>,
    /// The folded *Other* segment's.
    pub other: String,
}

/// A scatter dot's opacity: overlapping dots darken where they crowd, so a
/// dense cluster reads as one.
pub const DOT_OPACITY: f64 = 0.7;

/// `scene` as a standalone SVG document: the chart on a `background` rect,
/// with `pad` of it on every side, its text at `font_size`.
pub fn to_svg(scene: &Scene, colors: &SvgColors, font_size: f64, pad: f64) -> String {
    let c = colors;
    let (w, h) = (scene.width + 2.0 * pad, scene.height + 2.0 * pad);
    let mut out = String::with_capacity(2048);
    let _ = writeln!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\" \
         font-family=\"'IBM Plex Sans','Segoe UI',system-ui,sans-serif\">",
        n(w),
        n(h),
        n(w),
        n(h)
    );
    let _ = writeln!(
        out,
        "<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"{}\"/>",
        n(w),
        n(h),
        c.background
    );
    let _ = writeln!(out, "<g transform=\"translate({},{})\">", n(pad), n(pad));

    // Grid lines and their values.
    let p = &scene.plot;
    let mut d = String::new();
    for y in &scene.grid {
        let _ = write!(d, "M{} {}H{}", n(p.x0), n(*y), n(p.x1));
    }
    for x in &scene.vgrid {
        let _ = write!(d, "M{} {}V{}", n(*x), n(p.y0), n(p.y1));
    }
    if !d.is_empty() {
        let _ = writeln!(
            out,
            "<path d=\"{d}\" fill=\"none\" stroke=\"{}\" stroke-width=\"1\"/>",
            c.grid
        );
    }
    for l in &scene.ticks {
        text(&mut out, l, font_size, &c.tick_text);
    }

    match &scene.marks {
        Marks::Line { points, dots } => {
            let pts: Vec<String> = points
                .iter()
                .map(|(x, y)| format!("{},{}", n(*x), n(*y)))
                .collect();
            let _ = writeln!(
                out,
                "<polyline points=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{}\" \
                 stroke-linejoin=\"round\" stroke-linecap=\"round\"/>",
                pts.join(" "),
                c.mark,
                n(LINE_WIDTH)
            );
            if *dots {
                let _ = writeln!(out, "<g fill=\"{}\">", c.mark);
                for (x, y) in points {
                    let _ = writeln!(
                        out,
                        "<circle cx=\"{}\" cy=\"{}\" r=\"{}\"/>",
                        n(*x),
                        n(*y),
                        n(DOT_RADIUS)
                    );
                }
                out.push_str("</g>\n");
            }
        }
        Marks::Bars(bars) => {
            let _ = writeln!(out, "<g fill=\"{}\">", c.mark);
            for b in bars {
                let _ = writeln!(
                    out,
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>",
                    n(b.x0),
                    n(b.y0),
                    n(b.width()),
                    n(b.height())
                );
            }
            out.push_str("</g>\n");
        }
        Marks::Dots(points) => {
            let _ = writeln!(
                out,
                "<g fill=\"{}\" fill-opacity=\"{}\">",
                c.mark,
                n(DOT_OPACITY)
            );
            for (x, y) in points {
                let _ = writeln!(
                    out,
                    "<circle cx=\"{}\" cy=\"{}\" r=\"{}\"/>",
                    n(*x),
                    n(*y),
                    n(DOT_RADIUS)
                );
            }
            out.push_str("</g>\n");
        }
        Marks::Stacks(segments) => {
            for (stack, b) in segments {
                let _ = writeln!(
                    out,
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"/>",
                    n(b.x0),
                    n(b.y0),
                    n(b.width()),
                    n(b.height()),
                    stack_fill(scene, *stack, &c.stacks, &c.mark, &c.other)
                );
            }
        }
    }

    // The zero line, over the marks.
    let _ = writeln!(
        out,
        "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"1\"/>",
        n(p.x0),
        n(scene.base),
        n(p.x1),
        n(scene.base),
        c.zero
    );
    for l in &scene.under {
        text(&mut out, l, font_size, &c.under_text);
    }
    for e in &scene.legend {
        let s = &e.swatch;
        let _ = writeln!(
            out,
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"2\" fill=\"{}\"/>",
            n(s.x0),
            n(s.y0),
            n(s.width()),
            n(s.height()),
            stack_fill(scene, e.stack, &c.stacks, &c.mark, &c.other)
        );
        text(&mut out, &e.label, font_size, &c.under_text);
    }
    out.push_str("</g>\n</svg>\n");
    out
}

/// One label, centred on its measured box's middle line — the ER export's
/// `dominant-baseline`, so the two files' text sits the same way.
fn text(out: &mut String, l: &Label, font_size: f64, fill: &str) {
    let (x, anchor) = match l.align {
        Align::Start => (l.left, ""),
        Align::End => (l.left + l.w, " text-anchor=\"end\""),
    };
    let _ = writeln!(
        out,
        "<text x=\"{}\" y=\"{}\" font-size=\"{}\" fill=\"{}\"{anchor} dominant-baseline=\"central\">{}</text>",
        n(x),
        n(l.top + l.h / 2.0),
        n(font_size),
        fill,
        svg_text(&l.text)
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chart::{Bin, Series};

    /// Seven pixels a character, twelve high — a monospace stand-in for the
    /// app's font system.
    fn measure(s: &str) -> (f64, f64) {
        (s.chars().count() as f64 * 7.0, 12.0)
    }

    /// [`super::layout`] in English marks, which every test of the geometry is
    /// written in; it shadows the glob import.
    fn layout(
        drawn: &Plot,
        width: f64,
        height: f64,
        m: &Metrics,
        measure: &mut dyn FnMut(&str) -> (f64, f64),
    ) -> Option<Scene> {
        super::layout(
            drawn,
            width,
            height,
            m,
            crate::text::NumberMarks::ENGLISH,
            measure,
        )
    }

    fn lay(drawn: &Plot) -> Scene {
        layout(drawn, 600.0, 300.0, &Metrics::BASE, &mut measure).expect("drawable")
    }

    /// The tick labels are written in the marks given — German's decimal
    /// comma on a fractional axis — and measured as written.
    #[test]
    fn the_tick_labels_are_written_in_the_marks_given() {
        let de = crate::text::NumberMarks {
            decimal: ',',
            group: '.',
        };
        let mut asked: Vec<String> = Vec::new();
        let s = super::layout(
            &series(ChartKind::Line, &[0.1, 0.35]),
            600.0,
            300.0,
            &Metrics::BASE,
            de,
            &mut |t| {
                asked.push(t.to_string());
                measure(t)
            },
        )
        .expect("drawable");
        let ticks: Vec<&str> = s.ticks.iter().map(|l| l.text.as_str()).collect();
        assert!(ticks.iter().any(|t| t.contains(',')), "{ticks:?}");
        assert!(!ticks.iter().any(|t| t.contains('.')), "{ticks:?}");
        assert_eq!(asked[..ticks.len()], ticks[..]);
        // A series' labels are its label column's values, left as they came.
        assert!(s.under.iter().all(|l| l.text.starts_with('r')));
    }

    /// A histogram's labels under the axis are bin edges, and take the marks;
    /// a series' are data, and do not.
    #[test]
    fn only_a_histogram_s_edges_take_the_marks() {
        let de = crate::text::NumberMarks {
            decimal: ',',
            group: '.',
        };
        let bins = Plot::Bins(vec![
            Bin {
                lo: 0.5,
                hi: 1.5,
                count: 2,
            },
            Bin {
                lo: 1.5,
                hi: 2.5,
                count: 1,
            },
        ]);
        let s =
            super::layout(&bins, 600.0, 300.0, &Metrics::BASE, de, &mut measure).expect("drawable");
        let under: Vec<&str> = s.under.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(under, ["0,5", "1,5", "2,5"]);
        let labelled = Plot::Series(
            ChartKind::Bar,
            Series {
                labels: vec!["1.5".into(), "2.5".into()],
                values: vec![1.0, 2.0],
                skipped: 0,
            },
        );
        let s = super::layout(&labelled, 600.0, 300.0, &Metrics::BASE, de, &mut measure)
            .expect("drawable");
        let under: Vec<&str> = s.under.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(under, ["1.5", "2.5"]);
    }

    fn series(kind: ChartKind, values: &[f64]) -> Plot {
        Plot::Series(
            kind,
            Series {
                labels: (1..=values.len()).map(|i| format!("r{i}")).collect(),
                values: values.to_vec(),
                skipped: 0,
            },
        )
    }

    fn colors() -> SvgColors {
        SvgColors {
            background: "#101010".into(),
            grid: "#202020".into(),
            tick_text: "#303030".into(),
            under_text: "#404040".into(),
            mark: "#505050".into(),
            zero: "#606060".into(),
            stacks: vec!["#a00001".into(), "#a00002".into()],
            other: "#a0000f".into(),
        }
    }

    fn stacked(stacks: &[&str], other: bool, values: Vec<Vec<f64>>) -> Plot {
        Plot::Stacked(crate::chart::Stacked {
            labels: (1..=values.len()).map(|i| format!("b{i}")).collect(),
            stacks: stacks.iter().map(|s| s.to_string()).collect(),
            other,
            values,
            skipped: 0,
        })
    }

    fn segments(s: &Scene) -> &[(usize, Area)] {
        match &s.marks {
            Marks::Stacks(b) => b,
            other => panic!("expected stacks, got {other:?}"),
        }
    }

    /// A scatter's dots sit on both axes' scales: the smallest X leftmost,
    /// the largest Y highest, all inside the plot — and its X ticks are grid
    /// lines and the labels under the axis, in the interface's marks.
    #[test]
    fn a_scatter_places_each_dot_on_both_axes() {
        let drawn = Plot::Points(crate::chart::Points {
            xs: vec![0.5, 2.5, 1.5],
            ys: vec![10.0, 30.0, 20.0],
            skipped: 0,
        });
        let s = lay(&drawn);
        let Marks::Dots(d) = &s.marks else {
            panic!("expected dots");
        };
        assert_eq!(d.len(), 3);
        assert!(d[0].0 < d[2].0 && d[2].0 < d[1].0);
        assert!(d[1].1 < d[2].1 && d[2].1 < d[0].1);
        for (x, y) in d {
            assert!(*x >= s.plot.x0 && *x <= s.plot.x1, "{x}");
            assert!(*y >= s.plot.y0 && *y <= s.plot.y1, "{y}");
        }
        assert!(!s.vgrid.is_empty());
        assert_eq!(s.vgrid.len(), s.under.len());
        assert!(s.legend.is_empty());
        let de = crate::text::NumberMarks {
            decimal: ',',
            group: '.',
        };
        let s = super::layout(&drawn, 600.0, 300.0, &Metrics::BASE, de, &mut measure)
            .expect("drawable");
        assert!(
            s.under.iter().any(|l| l.text.contains(',')),
            "{:?}",
            s.under
        );
        let svg = to_svg(&s, &colors(), 12.0, 0.0);
        assert_eq!(svg.matches("<circle").count(), 3);
        assert!(svg.contains("fill-opacity"));
    }

    /// A scatter's X labels never overlap: a narrow box keeps every other one.
    #[test]
    fn a_scatter_s_x_labels_are_thinned_until_none_touch() {
        let drawn = Plot::Points(crate::chart::Points {
            xs: vec![1_000_001.5, 1_000_009.5],
            ys: vec![1.0, 2.0],
            skipped: 0,
        });
        let s = layout(&drawn, 160.0, 300.0, &Metrics::BASE, &mut measure).expect("drawable");
        assert!(s.under.len() >= 2);
        for w in s.under.windows(2) {
            assert!(w[0].left + w[0].w < w[1].left, "{:?}", s.under);
        }
    }

    /// Segments stack from zero, positive up and negative down, each bar's
    /// pieces touching; a zero is no segment.
    #[test]
    fn stacked_segments_stack_up_and_down_from_zero() {
        let s = lay(&stacked(
            &["x", "y"],
            false,
            vec![vec![3.0, 4.0], vec![-2.0, 0.0]],
        ));
        let seg = segments(&s);
        assert_eq!(seg.len(), 3, "the zero is left out: {seg:?}");
        let (a, b) = (&seg[0].1, &seg[1].1);
        assert_eq!((seg[0].0, seg[1].0), (0, 1));
        assert_eq!(a.y1, s.base, "the first stands on zero");
        assert_eq!(b.y1, a.y0, "the second on the first");
        assert!(b.height() > a.height(), "4 is taller than 3");
        let neg = &seg[2].1;
        assert_eq!(neg.y0, s.base, "a negative hangs from zero");
        assert!(neg.x0 > a.x1, "in the second bar");
    }

    /// Two stacks or more have a legend, under the labels and inside the
    /// box, in the palette's colours; the folded stack has its own.
    #[test]
    fn a_stacked_bar_has_a_legend_in_its_stacks_colours() {
        let s = lay(&stacked(
            &["north", "south", "Other"],
            true,
            vec![vec![1.0, 2.0, 3.0]],
        ));
        assert_eq!(s.legend.len(), 3);
        assert_eq!(s.other_stack, Some(2));
        let under_bottom = s.plot.y1 + 4.0 + 12.0;
        for e in &s.legend {
            assert!(e.swatch.y0 > under_bottom, "{e:?}");
            assert!(e.label.left + e.label.w <= s.width, "{e:?}");
            assert!(e.label.top + e.label.h <= s.height, "{e:?}");
            assert_eq!(e.label.left, e.swatch.x1 + 6.0);
        }
        let c = colors();
        assert_eq!(stack_fill(&s, 0, &c.stacks, &c.mark, &c.other), "#a00001");
        assert_eq!(stack_fill(&s, 1, &c.stacks, &c.mark, &c.other), "#a00002");
        assert_eq!(stack_fill(&s, 2, &c.stacks, &c.mark, &c.other), "#a0000f");
        let svg = to_svg(&s, &c, 12.0, 0.0);
        // Each segment and each swatch in its colour.
        assert_eq!(svg.matches("fill=\"#a00001\"").count(), 2);
        assert_eq!(svg.matches("fill=\"#a0000f\"").count(), 2);
        assert_eq!(
            svg.matches("<text").count(),
            s.ticks.len() + s.under.len() + 3
        );

        // One stack: no legend, the plot its full height, the bar's colour.
        let one = lay(&stacked(&[""], false, vec![vec![1.0]]));
        assert!(one.legend.is_empty());
        assert_eq!(one.plot.y1, lay(&series(ChartKind::Bar, &[1.0])).plot.y1);
        assert_eq!(stack_fill(&one, 0, &c.stacks, &c.mark, &c.other), "#505050");
    }

    /// A legend too wide for one row wraps, and the plot gives up the room.
    #[test]
    fn a_wide_legend_wraps_onto_more_rows() {
        let names: Vec<String> = (0..8).map(|i| format!("a long stack name {i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let narrow = layout(
            &stacked(&refs, false, vec![vec![1.0; 8]]),
            300.0,
            400.0,
            &Metrics::BASE,
            &mut measure,
        )
        .expect("drawable");
        let rows: std::collections::BTreeSet<i64> = narrow
            .legend
            .iter()
            .map(|e| e.label.top.round() as i64)
            .collect();
        assert!(rows.len() > 1, "{:?}", narrow.legend);
        for e in &narrow.legend {
            // Inside the right edge, unless it opens a row and is wider alone.
            assert!(
                e.label.left + e.label.w <= 300.0 - 8.0 || e.swatch.x0 == narrow.plot.x0,
                "{e:?}"
            );
            assert!(e.label.top + e.label.h <= 400.0, "{e:?}");
        }
    }

    /// The legend is measured last, after the labels under the axis.
    #[test]
    fn the_legend_is_measured_after_the_labels_under_the_axis() {
        let mut asked: Vec<String> = Vec::new();
        let s = layout(
            &stacked(&["p", "q"], false, vec![vec![1.0, 2.0], vec![3.0, 4.0]]),
            600.0,
            300.0,
            &Metrics::BASE,
            &mut |t| {
                asked.push(t.to_string());
                measure(t)
            },
        )
        .expect("drawable");
        let expected: Vec<String> = s
            .ticks
            .iter()
            .chain(&s.under)
            .chain(s.legend.iter().map(|e| &e.label))
            .map(|l| l.text.clone())
            .collect();
        assert_eq!(asked, expected);
    }

    fn bars(s: &Scene) -> &[Area] {
        match &s.marks {
            Marks::Bars(b) => b,
            other => panic!("expected bars, got {other:?}"),
        }
    }

    #[test]
    fn nothing_to_lay_out_without_an_axis_or_a_box() {
        let m = Metrics::BASE;
        // No values: no range, no ticks.
        assert_eq!(
            layout(&series(ChartKind::Bar, &[]), 600.0, 300.0, &m, &mut measure),
            None
        );
        // Values no step a double holds can span.
        let huge = series(ChartKind::Line, &[-f64::MAX, f64::MAX]);
        assert_eq!(layout(&huge, 600.0, 300.0, &m, &mut measure), None);
        // A box too small to hold the gutter.
        assert_eq!(
            layout(
                &series(ChartKind::Bar, &[1.0]),
                10.0,
                300.0,
                &m,
                &mut measure
            ),
            None
        );
    }

    #[test]
    fn the_gutter_is_the_widest_tick_label_and_the_labels_end_at_it() {
        let s = lay(&series(ChartKind::Bar, &[0.0, 12_000.0]));
        let widest = s.ticks.iter().map(|l| l.w).fold(0.0, f64::max);
        assert_eq!(s.plot.x0, widest + 8.0);
        for l in &s.ticks {
            assert_eq!(l.align, Align::End);
            assert_eq!(l.left + l.w, s.plot.x0 - 6.0, "{l:?}");
        }
        assert_eq!(s.ticks.len(), s.grid.len());
    }

    #[test]
    fn bars_stand_on_zero_either_side_of_it() {
        let s = lay(&series(ChartKind::Bar, &[5.0, -3.0, 0.0]));
        let b = bars(&s);
        assert_eq!(b.len(), 3);
        // Up from the zero line, down from it, and flat on it.
        assert_eq!(b[0].y1, s.base);
        assert!(b[0].y0 < s.base);
        assert_eq!(b[1].y0, s.base);
        assert!(b[1].y1 > s.base);
        assert_eq!(b[2].height(), 0.0);
        for (i, bar) in b.iter().enumerate() {
            assert!(
                bar.x0 >= s.plot.x0 && bar.x1 <= s.plot.x1,
                "bar {i} {bar:?}"
            );
        }
        // In order, apart, the gap no wider than the cap.
        let gap = b[1].x0 - b[0].x1;
        assert!(gap > 0.0 && gap <= 6.0, "{gap}");
    }

    #[test]
    fn a_line_has_one_point_per_value_and_dots_while_they_fit() {
        let s = lay(&series(ChartKind::Line, &[1.0, 3.0, 2.0]));
        let Marks::Line { points, dots } = &s.marks else {
            panic!("expected a line");
        };
        assert_eq!(points.len(), 3);
        assert!(dots);
        // Slot centres, left to right; the highest value highest up.
        assert!(points[0].0 < points[1].0 && points[1].0 < points[2].0);
        assert!(points[1].1 < points[2].1 && points[2].1 < points[0].1);

        let many: Vec<f64> = (0..=MAX_DOTS).map(|i| i as f64).collect();
        let Marks::Line { dots, .. } = lay(&series(ChartKind::Line, &many)).marks else {
            panic!("expected a line");
        };
        assert!(!dots, "{} points are too many to dot", MAX_DOTS + 1);
    }

    #[test]
    fn bins_stand_on_the_bottom_of_the_axis() {
        let s = lay(&Plot::Bins(vec![
            Bin {
                lo: 0.0,
                hi: 1.0,
                count: 2,
            },
            Bin {
                lo: 1.0,
                hi: 2.0,
                count: 4,
            },
        ]));
        let b = bars(&s);
        assert_eq!(b.len(), 2);
        assert_eq!(s.base, s.plot.y1);
        assert!(b.iter().all(|r| r.y1 == s.base));
        assert!(b[1].y0 < b[0].y0, "twice the count, taller");
    }

    #[test]
    fn labels_under_the_axis_stay_inside_the_box() {
        let long = "a label far too long to centre under the first bar";
        let drawn = Plot::Series(
            ChartKind::Bar,
            Series {
                labels: vec![long.into(), "b".into(), long.into()],
                values: vec![1.0, 2.0, 3.0],
                skipped: 0,
            },
        );
        let s = lay(&drawn);
        assert_eq!(s.under.len(), 3);
        for l in &s.under {
            assert!(l.left >= 0.0 && l.left + l.w <= s.width, "{l:?}");
            assert_eq!(l.top, s.plot.y1 + 4.0);
        }
    }

    /// The canvas draws its tick labels from what it measured, by position.
    #[test]
    fn the_tick_labels_are_measured_first_in_order_then_the_labels_under_the_axis() {
        let mut asked: Vec<String> = Vec::new();
        let s = layout(
            &series(ChartKind::Bar, &[3.0, 7.0, 12.0]),
            600.0,
            300.0,
            &Metrics::BASE,
            &mut |t| {
                asked.push(t.to_string());
                measure(t)
            },
        )
        .expect("drawable");
        let expected: Vec<String> = s
            .ticks
            .iter()
            .chain(&s.under)
            .map(|l| l.text.clone())
            .collect();
        assert_eq!(asked, expected);
    }

    #[test]
    fn the_svg_draws_every_mark_on_its_background_with_the_padding() {
        let s = lay(&series(ChartKind::Bar, &[1.0, 2.0]));
        let svg = to_svg(&s, &colors(), 12.0, 16.0);
        assert!(
            svg.starts_with(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"632\" height=\"332\""
            )
        );
        assert!(svg.ends_with("</svg>\n"));
        assert!(svg.contains("fill=\"#101010\""), "background");
        assert!(svg.contains("translate(16,16)"));
        // The background plus one per bar.
        assert_eq!(svg.matches("<rect").count(), 3);
        assert_eq!(svg.matches("<text").count(), s.ticks.len() + s.under.len());
        assert_eq!(svg.matches("text-anchor=\"end\"").count(), s.ticks.len());
        assert!(svg.contains("stroke=\"#606060\""), "the zero line");
    }

    #[test]
    fn the_svg_of_a_line_is_a_polyline_and_its_dots() {
        let s = lay(&series(ChartKind::Line, &[1.0, 3.0, 2.0]));
        let svg = to_svg(&s, &colors(), 12.0, 0.0);
        assert_eq!(svg.matches("<polyline").count(), 1);
        assert_eq!(svg.matches("<circle").count(), 3);
    }

    #[test]
    fn the_svg_escapes_a_label() {
        let drawn = Plot::Series(
            ChartKind::Bar,
            Series {
                labels: vec!["<a & b>".into()],
                values: vec![1.0],
                skipped: 0,
            },
        );
        let svg = to_svg(&lay(&drawn), &colors(), 12.0, 0.0);
        assert!(svg.contains("&lt;a &amp; b&gt;"), "{svg}");
        assert!(!svg.contains("<a &"));
    }
}
