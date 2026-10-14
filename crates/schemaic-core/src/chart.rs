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
pub fn ticks(lo: f64, hi: f64, want: usize) -> Vec<f64> {
    if !lo.is_finite() || !hi.is_finite() {
        return Vec::new();
    }
    let (lo, hi) = if lo == hi {
        (lo - 1.0, hi + 1.0)
    } else {
        (lo.min(hi), lo.max(hi))
    };
    let raw = (hi - lo) / want.max(1) as f64;
    let mag = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 5.0, 10.0]
        .iter()
        .map(|m| m * mag)
        .find(|s| *s >= raw)
        .unwrap_or(10.0 * mag);
    let start = (lo / step).floor() * step;
    let mut out = Vec::new();
    let mut t = start;
    while t <= hi + step * 1e-9 {
        // Snap away the float noise of repeated addition.
        out.push((t / step).round() * step);
        t += step;
    }
    if out.last().is_some_and(|l| *l < hi) {
        out.push(t);
    }
    out
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
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
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

    #[test]
    fn tick_labels_drop_noise_and_shorten_large_values() {
        assert_eq!(tick_label(20.0), "20");
        assert_eq!(tick_label(0.5), "0.5");
        assert_eq!(tick_label(25_000.0), "25k");
        assert_eq!(tick_label(1_500_000.0), "1.5m");
        assert_eq!(tick_label(-0.0), "0");
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
