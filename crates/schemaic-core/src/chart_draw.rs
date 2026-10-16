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
    pub ticks: Vec<Label>,
    pub marks: Marks,
    /// The zero line's y — or the axis's nearer end when 0 is off it.
    pub base: f64,
    /// The labels under the axis, kept inside the box.
    pub under: Vec<Label>,
}

/// `drawn` laid out in a `width` × `height` box, with text measured by
/// `measure` (`text → (width, height)`). `None` when there is nothing to draw:
/// no values, no axis for them ([`Plot::undrawable`]), or a box too small for
/// one.
///
/// **`measure` is called for each of [`Scene::ticks`] first, in order, then
/// for each of [`Scene::under`]** — a contract, not an accident: the canvas
/// keeps what it measured and draws the tick labels from it, by position.
pub fn layout(
    drawn: &Plot,
    width: f64,
    height: f64,
    m: &Metrics,
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
    let texts = chart::tick_labels(&ticks);
    let sizes: Vec<(f64, f64)> = texts.iter().map(|t| measure(t)).collect();
    let gutter = sizes.iter().map(|s| s.0).fold(0.0, f64::max) + m.gutter_pad;
    let label_h = sizes.first().map_or(12.0, |s| s.1);
    let plot = Area {
        x0: gutter,
        y0: label_h / 2.0,
        x1: width - m.inset,
        y1: height - label_h - m.inset,
    };
    if plot.width() <= 0.0 || plot.height() <= 0.0 || t1 <= t0 {
        return None;
    }
    let y_of = |v: f64| plot.y1 - (v - t0) / (t1 - t0) * plot.height();

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
    };

    let under = drawn
        .x_labels()
        .into_iter()
        .map(|(at, text)| {
            let (w, h) = measure(&text);
            Label {
                left: (plot.x0 + at * plot.width() - w / 2.0).clamp(0.0, (width - w).max(0.0)),
                top: plot.y1 + m.under_gap,
                w,
                h,
                text,
                align: Align::Start,
            }
        })
        .collect();

    Some(Scene {
        width,
        height,
        plot,
        grid,
        ticks: tick_labels,
        marks,
        base,
        under,
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
}

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

    fn lay(drawn: &Plot) -> Scene {
        layout(drawn, 600.0, 300.0, &Metrics::BASE, &mut measure).expect("drawable")
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
        }
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
