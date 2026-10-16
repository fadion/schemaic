//! **A chart of a result** — the numbers behind the result grid's *Chart*:
//! which columns can be plotted, one column's values against another's labels,
//! a histogram of one column, and the axis a value range is drawn against.
//!
//! Pure over a [`ResultSet`], so the view only paints what this answers. Of
//! the rows **already on screen**: a chart of a capped result is a chart of
//! the rows read, and the view says how many those are.
//!
//! **A number is a cell that reads as one**, by its text rather than its
//! wire tag: a `decimal` or `numeric` column arrives as text from every
//! engine, and is the column most worth charting.

use crate::model::{CellTag, ResultSet};

/// What the chart draws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChartKind {
    /// One bar per row: the value column against the label column.
    #[default]
    Bar,
    /// The same pairs joined in row order — for a series over time.
    Line,
    /// How one column's values are distributed, in equal-width bins.
    Histogram,
}

impl ChartKind {
    pub const ALL: [ChartKind; 3] = [ChartKind::Bar, ChartKind::Line, ChartKind::Histogram];

    pub fn label(self) -> &'static str {
        match self {
            ChartKind::Bar => "Bar",
            ChartKind::Line => "Line",
            ChartKind::Histogram => "Histogram",
        }
    }
}

/// The most bars or points a chart draws: past it they are thinner than a
/// pixel on any panel the chart fits, and the rest are said rather than drawn.
pub const MAX_POINTS: usize = 2_000;

/// A cell's number, or `None` for a NULL, a non-number or a NaN.
fn number(rs: &ResultSet, row: usize, col: usize) -> Option<f64> {
    let cell = rs.cell(row, col)?;
    if cell.tag == CellTag::Null {
        return None;
    }
    cell.text()
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
}

/// The columns that can be plotted: every non-NULL cell reads as a number,
/// and at least one is not NULL.
pub fn numeric_columns(rs: &ResultSet) -> Vec<usize> {
    (0..rs.col_count())
        .filter(|&c| {
            let mut any = false;
            for r in 0..rs.row_count() {
                let Some(cell) = rs.cell(r, c) else { continue };
                if cell.tag == CellTag::Null {
                    continue;
                }
                if number(rs, r, c).is_none() {
                    return false;
                }
                any = true;
            }
            any
        })
        .collect()
}

/// The pickers' name for each column, one per column and no two alike — the
/// pickers map a chosen name back to its column, so a self-join's two `id`s
/// both answered the first. A repeated name, or one equal to a `reserved`
/// option the picker also offers (*Row number*), takes its 1-based position:
/// `id (3)`. The first of a name keeps it bare, so an ordinary result reads
/// as it always did.
pub fn picker_labels(names: &[String], reserved: &[&str]) -> Vec<String> {
    let mut taken: std::collections::HashSet<String> =
        reserved.iter().map(|r| r.to_string()).collect();
    let mut out = Vec::with_capacity(names.len());
    for (i, name) in names.iter().enumerate() {
        let mut label = name.clone();
        let mut n = i + 1;
        while taken.contains(&label) {
            label = format!("{name} ({n})");
            // A column already spelt `id (3)` pushes the next one along.
            n += names.len();
        }
        taken.insert(label.clone());
        out.push(label);
    }
    out
}

/// Labelled values to draw as bars or a line.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Series {
    pub labels: Vec<String>,
    pub values: Vec<f64>,
    /// Rows left out: a NULL or non-numeric value, or past [`MAX_POINTS`].
    pub skipped: usize,
}

/// Column `y`'s values, labelled by column `x`'s text — or by row number when
/// there is no `x` — in row order.
pub fn series(rs: &ResultSet, x: Option<usize>, y: usize) -> Series {
    let mut out = Series::default();
    for r in 0..rs.row_count() {
        let Some(v) = number(rs, r, y) else {
            out.skipped += 1;
            continue;
        };
        if out.values.len() == MAX_POINTS {
            out.skipped += 1;
            continue;
        }
        let label = match x.and_then(|x| rs.cell(r, x)) {
            Some(c) if c.tag == CellTag::Null => "NULL".to_string(),
            Some(c) => c.text().to_string(),
            None => (r + 1).to_string(),
        };
        out.labels.push(label);
        out.values.push(v);
    }
    out
}

/// One histogram bin: `[lo, hi)`, the last one closed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bin {
    pub lo: f64,
    pub hi: f64,
    pub count: usize,
}

/// Column `col`'s values in equal-width bins — Sturges' rule for how many,
/// at most 50 — and how many cells were not numbers.
pub fn histogram(rs: &ResultSet, col: usize) -> (Vec<Bin>, usize) {
    let mut values = Vec::new();
    let mut skipped = 0;
    for r in 0..rs.row_count() {
        match number(rs, r, col) {
            Some(v) => values.push(v),
            None => skipped += 1,
        }
    }
    (bins(&values), skipped)
}

/// [`histogram`]'s binning, over plain values.
pub fn bins(values: &[f64]) -> Vec<Bin> {
    if values.is_empty() {
        return Vec::new();
    }
    let lo = values.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if lo == hi {
        return vec![Bin {
            lo,
            hi,
            count: values.len(),
        }];
    }
    let n = ((values.len() as f64).log2().ceil() as usize + 1).clamp(1, 50);
    let width = (hi - lo) / n as f64;
    let mut out: Vec<Bin> = (0..n)
        .map(|i| Bin {
            lo: lo + width * i as f64,
            hi: if i + 1 == n {
                hi
            } else {
                lo + width * (i + 1) as f64
            },
            count: 0,
        })
        .collect();
    for &v in values {
        let i = (((v - lo) / width) as usize).min(n - 1);
        out[i].count += 1;
    }
    out
}

/// A value axis for `lo..=hi`: round tick values covering the range, about
/// `want` of them, from 1, 2 or 5 times a power of ten. A bar chart passes a
/// range that includes 0, so bars stand on it.
///
/// **Ticks are counted, never accumulated.** Each is `(first + i) × step`, and
/// the step is never finer than a few times the spacing of doubles at the
/// range's magnitude — distinct values a couple of ULPs apart (snowflake ids
/// minted in one millisecond) once picked a step adding which never moved
/// `t`, and the loop ran until memory did. The count is bounded besides.
pub fn ticks(lo: f64, hi: f64, want: usize) -> Vec<f64> {
    if !lo.is_finite() || !hi.is_finite() {
        return Vec::new();
    }
    let (lo, hi) = if lo == hi {
        (lo - 1.0, hi + 1.0)
    } else {
        (lo.min(hi), lo.max(hi))
    };
    // The spacing of doubles at the range's largest magnitude: a step below a
    // few of these cannot tell two ticks apart.
    let far = lo.abs().max(hi.abs());
    let ulp = f64::from_bits(far.to_bits() + 1) - far;
    let raw = ((hi - lo) / want.max(1) as f64).max(ulp * 4.0);
    if !raw.is_finite() {
        return Vec::new();
    }
    let mag = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 5.0, 10.0]
        .iter()
        .map(|m| m * mag)
        .find(|s| *s >= raw)
        .unwrap_or(10.0 * mag);
    let first = (lo / step).floor();
    let tick = |i: usize| (first + i as f64) * step;
    // Ample for `want` ticks of a step at least the raw one, plus the ends.
    let cap = want.max(1) * 4 + 4;
    let mut out: Vec<f64> = Vec::new();
    for i in 0..cap {
        let t = tick(i);
        if !t.is_finite() {
            break;
        }
        if out.last().is_some_and(|l| t <= *l) {
            continue;
        }
        let past = t > hi + step * 1e-9;
        // One tick past the top when the last fell short of it, so the axis
        // covers the range — and always at least two, so it has a height.
        if past && out.last().is_some_and(|l| *l >= hi) && out.len() >= 2 {
            break;
        }
        out.push(t);
        if past {
            break;
        }
    }
    // An axis that stops short of the top — a tick past it overflowed, near
    // `f64::MAX` — would draw the marks above the plot: none at all, and
    // `Plot::undrawable` says why.
    if out.len() < 2 || out.last().is_some_and(|l| *l < hi) {
        return Vec::new();
    }
    out
}

/// The labels of an axis whose ticks are `ticks` — **as precise as the axis
/// is fine**, so no two read alike.
///
/// Each label kept two decimals of its scaled value, so ticks closer than
/// that printed the same text: `100000000b` twice for snowflake ids a few
/// apart, `0` five times for a fraction column of 0..0.004, while a value
/// near `1e150` printed a hundred digits and left the plot no width. Here the
/// step sets the precision: while seven significant digits tell the ticks
/// apart, each is [`tick_label`]'s scaled form (`k`, `m`, `b`) with as many
/// decimals as the step needs, or an exponent past `1e15`; finer than that,
/// the first tick is written in full and the rest as its offset — `1e17`,
/// `+96`.
pub fn tick_labels(ticks: &[f64]) -> Vec<String> {
    let step = ticks
        .windows(2)
        .map(|w| w[1] - w[0])
        .filter(|d| *d > 0.0)
        .fold(f64::INFINITY, f64::min);
    let far = ticks.iter().fold(0.0f64, |m, t| m.max(t.abs()));
    if !step.is_finite() {
        return ticks.iter().map(|t| tick_label(*t)).collect();
    }
    let needed = if far > step {
        (far / step).log10().ceil() as i32 + 1
    } else {
        1
    };
    if needed > 7 {
        let base = ticks[0];
        return ticks
            .iter()
            .enumerate()
            .map(|(i, t)| {
                if i == 0 {
                    exponent(base, 4)
                } else {
                    format!("+{}", scaled(t - base, step))
                }
            })
            .collect();
    }
    if far >= 1e15 {
        return ticks
            .iter()
            .map(|t| exponent(*t, needed.max(1) as usize))
            .collect();
    }
    ticks.iter().map(|t| scaled(*t, step)).collect()
}

/// `v` in [`tick_label`]'s scaled form, with the decimals a step of `step`
/// needs to show.
fn scaled(v: f64, step: f64) -> String {
    let a = v.abs().max(step);
    // Past a billion billions a scaled number is all digits: an exponent,
    // with the digits the step needs.
    if a >= 1e15 {
        let digits = ((a / step).log10().ceil() as i32 + 1).clamp(1, 7);
        return exponent(v, digits as usize);
    }
    let (unit, suffix) = if a >= 1e9 {
        (1e9, "b")
    } else if a >= 1e6 {
        (1e6, "m")
    } else if a >= 1e4 {
        (1e3, "k")
    } else {
        (1.0, "")
    };
    let decimals = (-(step / unit).log10()).ceil().clamp(0.0, 6.0) as usize;
    format!("{}{suffix}", trim_to(v / unit, decimals))
}

/// `v` as `1.234e17`, with at most `digits` significant digits and no
/// trailing zeros in the mantissa.
fn exponent(v: f64, digits: usize) -> String {
    let s = format!("{:.*e}", digits.saturating_sub(1), v);
    match s.split_once('e') {
        Some((m, e)) => {
            let m = if m.contains('.') {
                m.trim_end_matches('0').trim_end_matches('.')
            } else {
                m
            };
            format!("{m}e{e}")
        }
        None => s,
    }
}

/// What a chart draws: a series of labelled values (a bar or line chart), or
/// a histogram's bins. The view paints one; the decisions about it — the
/// value axis's range, the labels under the axis, the line beneath the
/// canvas — are answered here, where a test can see them.
#[derive(Clone, Debug, PartialEq)]
pub enum Plot {
    Series(ChartKind, Series),
    Bins(Vec<Bin>),
}

impl Plot {
    /// Column `value` of `rs` charted as `kind`, labelled by `label` — and how
    /// many rows it left out.
    pub fn of(
        rs: &ResultSet,
        kind: ChartKind,
        value: usize,
        label: Option<usize>,
    ) -> (Plot, usize) {
        match kind {
            ChartKind::Histogram => {
                let (bins, skipped) = histogram(rs, value);
                (Plot::Bins(bins), skipped)
            }
            ChartKind::Bar | ChartKind::Line => {
                let s = series(rs, label, value);
                let skipped = s.skipped;
                (Plot::Series(kind, s), skipped)
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Plot::Series(_, s) => s.values.is_empty(),
            Plot::Bins(b) => b.is_empty(),
        }
    }

    /// Why this cannot be drawn, in the words the modal shows in place of the
    /// canvas — or `None` when it can. A value at or near `±f64::MAX` (a
    /// common "no upper bound" sentinel) leaves the axis no step a double
    /// holds ([`ticks`] answers none), and the canvas painted nothing with
    /// nothing saying why.
    pub fn undrawable(&self) -> Option<&'static str> {
        let (lo, hi) = self.range();
        ticks(lo, hi, 5)
            .is_empty()
            .then_some("These values are too large to draw.")
    }

    /// The value axis's range. A bar or bin stands on 0, so 0 is in it; a
    /// line follows its values, so a narrow band of large numbers is drawn
    /// as a band rather than flattened against a zero far below it.
    pub fn range(&self) -> (f64, f64) {
        let (lo, hi, zero) = match self {
            Plot::Series(k, s) => (
                s.values.iter().copied().fold(f64::INFINITY, f64::min),
                s.values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                *k == ChartKind::Bar,
            ),
            Plot::Bins(b) => (
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

    /// The labels under the x axis — first, middle and last — each with where
    /// it sits across the plot, `0.0..=1.0`.
    pub fn x_labels(&self) -> Vec<(f64, String)> {
        let cut = |s: &str| -> String {
            if s.chars().count() > 18 {
                format!("{}…", s.chars().take(17).collect::<String>())
            } else {
                s.to_string()
            }
        };
        match self {
            Plot::Series(_, s) => {
                let n = s.labels.len();
                if n == 0 {
                    return Vec::new();
                }
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
            Plot::Bins(b) => {
                let (Some(first), Some(last)) = (b.first(), b.last()) else {
                    return Vec::new();
                };
                let (first, last) = (first.lo, last.hi);
                let mid = (first + last) / 2.0;
                vec![
                    (0.0, tick_label(first)),
                    (0.5, tick_label(mid)),
                    (1.0, tick_label(last)),
                ]
            }
        }
    }
}

/// The line under the canvas: how many rows the result holds, how many were
/// not drawn and why, and whether those are all the rows. A series leaves out
/// a NULL or a row past [`MAX_POINTS`]; a histogram bins every number, so it
/// has no cap to name.
pub fn caption(rows: usize, kind: ChartKind, skipped: usize, truncated: bool) -> String {
    let mut line = format!("{} {}", rows, crate::text::plural(rows, "row", "rows"));
    if skipped > 0 {
        match kind {
            ChartKind::Histogram => line.push_str(&format!(" · {skipped} NULL, not drawn")),
            ChartKind::Bar | ChartKind::Line => line.push_str(&format!(
                " · {skipped} not drawn (NULL, or past {MAX_POINTS})"
            )),
        }
    }
    if truncated {
        line.push_str(" · the rows read, not the whole result");
    }
    line
}

/// A tick's text: no trailing `.0`, and a thousands-scale value shortened.
pub fn tick_label(v: f64) -> String {
    let a = v.abs();
    if a >= 1e9 {
        format!("{}b", trim(v / 1e9))
    } else if a >= 1e6 {
        format!("{}m", trim(v / 1e6))
    } else if a >= 1e4 {
        format!("{}k", trim(v / 1e3))
    } else {
        trim(v)
    }
}

fn trim(v: f64) -> String {
    trim_to(v, 2)
}

/// `v` with at most `decimals` decimals, no trailing zeros, and no `-0`.
fn trim_to(v: f64, decimals: usize) -> String {
    let s = format!("{v:.decimals$}");
    let s = if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.')
    } else {
        &s
    };
    if s == "-0" {
        "0".to_string()
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Column, ResultBuilder, Value};

    fn rs(cols: &[&str], rows: Vec<Vec<Value>>) -> ResultSet {
        let mut b = ResultBuilder::new(
            cols.iter()
                .map(|n| Column {
                    name: n.to_string(),
                    type_name: String::new(),
                    origin: None,
                })
                .collect(),
        );
        for r in rows {
            b.push_row(&r);
        }
        b.finish()
    }

    fn s(v: &str) -> Value {
        Value::Str(v.into())
    }

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// **Two columns of one name are two choices.** A self-join's `id`s
    /// shared a label, and the picker's lookup by name chose the first
    /// whichever was picked.
    #[test]
    fn repeated_column_names_get_distinct_picker_labels() {
        let got = picker_labels(&names(&["id", "name", "id"]), &[]);
        assert_eq!(got, ["id", "name", "id (3)"]);
    }

    #[test]
    fn distinct_names_are_left_as_they_are() {
        assert_eq!(
            picker_labels(&names(&["a", "b"]), &["Row number"]),
            ["a", "b"]
        );
        assert!(picker_labels(&[], &[]).is_empty());
    }

    /// A column named like a picker's own option, or like the label another
    /// duplicate would take, still comes out unique.
    #[test]
    fn a_label_never_collides_with_a_reserved_option_or_another_label() {
        let got = picker_labels(
            &names(&["Row number", "id", "id (3)", "id"]),
            &["Row number"],
        );
        assert_eq!(got.len(), 4);
        let unique: std::collections::HashSet<&String> = got.iter().collect();
        assert_eq!(unique.len(), 4, "{got:?}");
        assert!(!got.iter().any(|l| l == "Row number"), "{got:?}");
        assert_eq!(got[1], "id");
    }

    #[test]
    fn a_column_is_numeric_when_every_value_reads_as_a_number() {
        let r = rs(
            &["name", "total", "mixed", "empty"],
            vec![
                vec![s("a"), s("10.50"), Value::Int(1), Value::Null],
                vec![s("b"), Value::Null, s("x"), Value::Null],
                vec![s("c"), Value::Float(2.5), Value::Int(3), Value::Null],
            ],
        );
        assert_eq!(numeric_columns(&r), [1]);
    }

    #[test]
    fn a_series_pairs_labels_with_values_and_counts_what_it_left_out() {
        let r = rs(
            &["day", "n"],
            vec![
                vec![s("mon"), Value::Int(3)],
                vec![s("tue"), Value::Null],
                vec![Value::Null, s("7")],
            ],
        );
        let got = series(&r, Some(0), 1);
        assert_eq!(got.labels, ["mon", "NULL"]);
        assert_eq!(got.values, [3.0, 7.0]);
        assert_eq!(got.skipped, 1);
        assert_eq!(series(&r, None, 1).labels, ["1", "3"], "row numbers");
    }

    #[test]
    fn a_histogram_bins_every_value_and_closes_its_last_bin() {
        let b = bins(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);
        assert_eq!(b.len(), 4, "Sturges: log2(8) + 1");
        assert_eq!(b.iter().map(|b| b.count).sum::<usize>(), 8);
        assert_eq!(b.last().unwrap().hi, 8.0);
        assert_eq!(
            b.last().unwrap().count,
            2,
            "the maximum lands in the last bin"
        );
        assert_eq!(
            bins(&[4.0, 4.0]),
            [Bin {
                lo: 4.0,
                hi: 4.0,
                count: 2
            }]
        );
        assert!(bins(&[]).is_empty());
    }

    #[test]
    fn ticks_are_round_and_cover_the_range() {
        assert_eq!(ticks(0.0, 100.0, 5), [0.0, 20.0, 40.0, 60.0, 80.0, 100.0]);
        let t = ticks(3.0, 47.0, 4);
        assert!(t[0] <= 3.0 && *t.last().unwrap() >= 47.0, "{t:?}");
        // 44 over 4 ticks is a raw step of 11, so the round one is 20.
        assert_eq!(t, [0.0, 20.0, 40.0, 60.0]);
        assert!(
            ticks(5.0, 5.0, 4).len() > 1,
            "a flat range still gets an axis"
        );
        assert!(ticks(f64::NAN, 1.0, 4).is_empty());
    }

    /// Distinct values within a couple of ULPs of each other — snowflake ids
    /// minted in one millisecond — once looped forever: the round step fell
    /// below the spacing of doubles there, so adding it never moved `t`.
    #[test]
    fn ticks_terminate_at_extreme_magnitudes() {
        for (lo, hi) in [
            (1e17, 1e17 + 16.0),
            (2f64.powi(54), 2f64.powi(54) + 4.0),
            (1.8e18, 1.8e18 + 256.0),
            (1e17, 1e17),
            (-1e300, -1e300),
            (f64::MAX / 2.0, f64::MAX / 2.0),
        ] {
            let t = ticks(lo, hi, 5);
            assert!(t.len() >= 2 && t.len() <= 40, "({lo}, {hi}): {t:?}");
            assert!(
                t[0] <= lo && *t.last().unwrap() >= hi,
                "({lo}, {hi}): {t:?}"
            );
            assert!(
                t.windows(2).all(|w| w[0] < w[1]),
                "strictly rising, so the axis has a height: {t:?}"
            );
        }
    }

    #[test]
    fn a_line_range_does_not_force_zero() {
        let s = Series {
            labels: vec!["a".into(), "b".into()],
            values: vec![1e17, 1e17 + 16.0],
            skipped: 0,
        };
        assert_eq!(
            Plot::Series(ChartKind::Line, s.clone()).range(),
            (1e17, 1e17 + 16.0)
        );
        assert_eq!(
            Plot::Series(ChartKind::Bar, s).range(),
            (0.0, 1e17 + 16.0),
            "a bar stands on 0"
        );
        let neg = Series {
            labels: vec!["a".into()],
            values: vec![-5.0],
            skipped: 0,
        };
        assert_eq!(Plot::Series(ChartKind::Bar, neg).range(), (-5.0, 0.0));
        let bins = Plot::Bins(vec![
            Bin {
                lo: 10.0,
                hi: 20.0,
                count: 3,
            },
            Bin {
                lo: 20.0,
                hi: 30.0,
                count: 7,
            },
        ]);
        assert_eq!(bins.range(), (0.0, 7.0), "a bin's height is its count");
    }

    #[test]
    fn x_labels_are_first_middle_and_last() {
        let s = |n: usize| Series {
            labels: (0..n).map(|i| format!("r{i}")).collect(),
            values: vec![1.0; n],
            skipped: 0,
        };
        let at = |n| Plot::Series(ChartKind::Line, s(n)).x_labels();
        assert_eq!(at(1), [(0.5, "r0".to_string())]);
        assert_eq!(at(2).len(), 2);
        let five = at(5);
        assert_eq!(
            five.iter().map(|(_, l)| l.as_str()).collect::<Vec<_>>(),
            ["r0", "r2", "r4"]
        );
        assert_eq!(five[0].0, 0.1);
        assert!(Plot::Series(ChartKind::Bar, s(0)).x_labels().is_empty());
        let long = Series {
            labels: vec!["x".repeat(30)],
            values: vec![1.0],
            skipped: 0,
        };
        let cut = &Plot::Series(ChartKind::Bar, long).x_labels()[0].1;
        assert_eq!(cut.chars().count(), 18);
        assert!(cut.ends_with('…'));
        let bins = Plot::Bins(bins(&[0.0, 10.0, 20.0, 40.0]));
        assert_eq!(
            bins.x_labels(),
            [
                (0.0, "0".to_string()),
                (0.5, "20".to_string()),
                (1.0, "40".to_string())
            ]
        );
        assert!(Plot::Bins(Vec::new()).x_labels().is_empty());
    }

    #[test]
    fn a_histogram_caption_does_not_mention_the_point_cap() {
        let h = caption(10, ChartKind::Histogram, 3, false);
        assert_eq!(h, "10 rows · 3 NULL, not drawn");
        assert!(!h.contains(&MAX_POINTS.to_string()));
        assert_eq!(
            caption(1, ChartKind::Bar, 0, false),
            "1 row",
            "nothing left out, nothing said"
        );
        assert_eq!(
            caption(2_005, ChartKind::Line, 5, true),
            "2005 rows · 5 not drawn (NULL, or past 2000) · the rows read, not the whole result"
        );
    }

    #[test]
    fn a_plot_counts_what_it_left_out() {
        let r = rs(
            &["n"],
            vec![vec![Value::Int(1)], vec![Value::Null], vec![Value::Int(3)]],
        );
        let (p, skipped) = Plot::of(&r, ChartKind::Histogram, 0, None);
        assert!(matches!(p, Plot::Bins(_)));
        assert_eq!(skipped, 1);
        let (p, skipped) = Plot::of(&r, ChartKind::Line, 0, None);
        assert!(matches!(p, Plot::Series(ChartKind::Line, _)));
        assert_eq!(skipped, 1);
        assert!(!p.is_empty());
    }

    #[test]
    fn tick_labels_drop_noise_and_shorten_large_values() {
        assert_eq!(tick_label(20.0), "20");
        assert_eq!(tick_label(0.5), "0.5");
        assert_eq!(tick_label(25_000.0), "25k");
        assert_eq!(tick_label(1_500_000.0), "1.5m");
        assert_eq!(tick_label(-0.0), "0");
        assert_eq!(
            tick_labels(&[0.0, 20.0, 40.0]),
            ["0", "20", "40"],
            "a round axis reads as before"
        );
        assert_eq!(
            tick_labels(&[1_000_000.0, 1_500_000.0, 2_000_000.0]),
            ["1m", "1.5m", "2m"]
        );
    }

    /// **An axis's labels are as precise as its step** (S5-L1-01): each
    /// label kept two decimals of its scaled value, so ticks closer than
    /// that read the same — `100000000b` twice for 3ea81a7d's own snowflake
    /// ids, `0` five times for a fraction column of 0..0.004 — and a value
    /// near `1e150` made a label of a hundred digits that left the plot no
    /// width. Every axis `ticks` builds is labelled pairwise distinct, short.
    #[test]
    fn an_axis_never_labels_two_ticks_alike() {
        for (lo, hi) in [
            (1e17, 1e17 + 16.0),
            (2f64.powi(54), 2f64.powi(54) + 4.0),
            (1.8e18, 1.8e18 + 256.0),
            (1e17, 1e17),
            (-1e300, -1e300),
            (f64::MAX / 2.0, f64::MAX / 2.0),
            (0.0, 0.004),
            (101.234, 101.239),
            (1.7e12, 1.7e12 + 600_000.0),
            (i64::MAX as f64, i64::MAX as f64),
            (1e150, 5e150),
            (0.0, 100.0),
            (-5.0, 5.0),
        ] {
            let t = ticks(lo, hi, 5);
            let labels = tick_labels(&t);
            assert_eq!(labels.len(), t.len());
            for (i, a) in labels.iter().enumerate() {
                assert!(a.chars().count() <= 16, "({lo}, {hi}): {a:?} of {labels:?}");
                assert!(
                    !labels[i + 1..].contains(a),
                    "({lo}, {hi}): {a:?} twice in {labels:?} for {t:?}"
                );
            }
        }
        assert_eq!(tick_labels(&ticks(0.0, 0.004, 5))[1], "0.001");
        assert_eq!(tick_labels(&[1e17, 1e17 + 96.0]), ["1e17", "+96"]);
    }

    /// **A range too large to draw is said, not left blank** (S5-L1-02): at
    /// `±f64::MAX` the step overflowed and `ticks` came back empty — the
    /// canvas painted nothing, with nothing saying why — and near it the axis
    /// stopped short of the top, so the line ran above the plot.
    #[test]
    fn a_range_too_large_to_draw_has_no_axis_and_says_so() {
        for (lo, hi) in [
            (0.0, f64::MAX),
            (-f64::MAX, f64::MAX),
            (f64::MAX, f64::MAX),
            (1.7e308, 1.79e308),
        ] {
            assert!(
                ticks(lo, hi, 5).is_empty(),
                "({lo}, {hi}): {:?}",
                ticks(lo, hi, 5)
            );
        }
        let s = Series {
            labels: vec!["a".into()],
            values: vec![f64::MAX],
            skipped: 0,
        };
        let plot = Plot::Series(ChartKind::Bar, s);
        assert!(
            plot.undrawable()
                .is_some_and(|n| n.contains("too large to draw"))
        );
        let fine = Plot::Series(
            ChartKind::Bar,
            Series {
                labels: vec!["a".into()],
                values: vec![3.0],
                skipped: 0,
            },
        );
        assert_eq!(fine.undrawable(), None);
    }

    #[test]
    fn a_long_result_draws_at_most_the_cap() {
        let rows = (0..MAX_POINTS + 5)
            .map(|i| vec![Value::Int(i as i64)])
            .collect();
        let r = rs(&["n"], rows);
        let got = series(&r, None, 0);
        assert_eq!(got.values.len(), MAX_POINTS);
        assert_eq!(got.skipped, 5);
    }
}
