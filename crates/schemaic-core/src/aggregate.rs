//! Aggregates over a selected range of grid cells — the readout the results
//! panel shows once you select more than one cell.
//!
//! Pure and exact. The arithmetic is **fixed-point, not `f64`**, and that is the
//! whole reason this module has any substance: [`crate::model::Column::is_numeric`]
//! counts `DECIMAL`/`NUMERIC` as numeric, while [`crate::model::Value`]
//! deliberately leaves those cells as `Str` so the text protocol's digits are
//! never rounded or reformatted. A money column is precisely the case anyone
//! wants a `Sum` for, so summing it through a float would reintroduce, in the one
//! number the user reads, the error the storage model goes out of its way to
//! avoid — `Sum: 45.599999999999994` under a column of tidy prices.
//!
//! So values are parsed into [`Fixed`] (an `i128` of units at a decimal scale),
//! summed and compared at a common scale, and formatted back. Only the average
//! divides, and it says so by carrying more decimals than its inputs.

use crate::model::{CellRef, Column};

/// A fixed-point decimal: `units` × 10⁻ˢᶜᵃˡᵉ.
///
/// `i128` because the sum of a wide column over a big selection has to fit:
/// 200k rows of `BIGINT` is ~2 × 10²⁴, and the same at scale 10 is ~2 × 10³⁴,
/// both comfortably inside i128's ~1.7 × 10³⁸. Beyond that the arithmetic is
/// *checked* and the aggregate degrades to `None` rather than wrapping — a
/// silently wrong total is worse than an absent one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Fixed {
    pub units: i128,
    pub scale: u32,
}

impl Fixed {
    /// Parse a decimal literal as the wire delivers it: optional sign, digits,
    /// optional fraction. No exponent — a `DECIMAL` column never sends one, and
    /// accepting `1e3` here would mean accepting it from a `VARCHAR` too.
    pub fn parse(s: &str) -> Option<Fixed> {
        let s = s.trim();
        let (neg, digits) = match s.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, s.strip_prefix('+').unwrap_or(s)),
        };
        let (int_part, frac_part) = match digits.split_once('.') {
            Some((i, f)) => (i, f),
            None => (digits, ""),
        };
        if int_part.is_empty() && frac_part.is_empty() {
            return None;
        }
        if !int_part.bytes().all(|b| b.is_ascii_digit())
            || !frac_part.bytes().all(|b| b.is_ascii_digit())
        {
            return None;
        }
        let mut units: i128 = 0;
        for b in int_part.bytes().chain(frac_part.bytes()) {
            units = units.checked_mul(10)?.checked_add((b - b'0') as i128)?;
        }
        Some(Fixed {
            units: if neg { -units } else { units },
            scale: frac_part.len() as u32,
        })
    }

    /// The same value carried to `scale` decimal places, or `None` on overflow.
    /// Only ever used to *raise* the scale, so no rounding decision arises.
    fn rescale(self, scale: u32) -> Option<Fixed> {
        if scale < self.scale {
            return None;
        }
        let factor = 10i128.checked_pow(scale - self.scale)?;
        Some(Fixed {
            units: self.units.checked_mul(factor)?,
            scale,
        })
    }

    /// Render with the decimal point its scale implies, keeping trailing zeros —
    /// a `DECIMAL(10,2)` column sums to `45.60`, not `45.6`, because that is what
    /// the column's own cells look like.
    pub fn text(self) -> String {
        if self.scale == 0 {
            return self.units.to_string();
        }
        let neg = self.units < 0;
        let mag = self.units.unsigned_abs().to_string();
        let scale = self.scale as usize;
        let padded = if mag.len() <= scale {
            format!("{}{mag}", "0".repeat(scale - mag.len() + 1))
        } else {
            mag
        };
        let split = padded.len() - scale;
        format!(
            "{}{}.{}",
            if neg { "-" } else { "" },
            &padded[..split],
            &padded[split..]
        )
    }

    /// Drop trailing fractional zeros (`2.500` → `2.5`, `3.000` → `3`). Used for
    /// the average only, which manufactures decimals its inputs never had.
    fn trimmed(self) -> Fixed {
        let mut f = self;
        while f.scale > 0 && f.units % 10 == 0 {
            f.units /= 10;
            f.scale -= 1;
        }
        f
    }
}

/// Extra decimal places the average carries beyond its inputs, so that the mean
/// of two integers can be `1.5` rather than `1`.
const AVG_EXTRA_SCALE: u32 = 6;

/// What a selection of cells in one column adds up to.
///
/// `rows` counts every selected cell, `non_null` those that hold a value — the
/// two differ exactly when the selection covers NULLs, which is worth seeing,
/// since it is close to the denominator [`NumericAggregates::avg`] divided by
/// (that one counts the values that *parsed*, which is `non_null` minus any cell
/// a numeric column holds that isn't a number).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Aggregates {
    pub rows: usize,
    pub non_null: usize,
    /// `None` for a non-numeric column, and for a numeric one whose selected
    /// cells are all NULL, all unparseable, or overflow the accumulator.
    pub numeric: Option<NumericAggregates>,
}

/// The arithmetic half, present only when the column is numeric and at least one
/// selected cell parsed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NumericAggregates {
    pub sum: Fixed,
    pub avg: Fixed,
    pub min: Fixed,
    pub max: Fixed,
}

/// Aggregate `cells` under `column`.
///
/// Non-numeric columns get counts only — there is nothing to sum in a name, and
/// a lexicographic min/max reads as a bug more often than it answers a question.
/// A numeric cell that doesn't parse is counted as present but left out of the
/// arithmetic: the column type says what it should be, and a wire value that
/// isn't a number is not something to guess about.
pub fn aggregate<'a>(column: &Column, cells: impl Iterator<Item = CellRef<'a>>) -> Aggregates {
    let numeric_column = column.is_numeric();
    let mut agg = Aggregates::default();
    let mut vals: Vec<Fixed> = Vec::new();
    for cell in cells {
        agg.rows += 1;
        if cell.is_null() {
            continue;
        }
        agg.non_null += 1;
        if numeric_column && let Some(f) = Fixed::parse(cell.text()) {
            vals.push(f);
        }
    }
    agg.numeric = numeric_of(&vals);
    agg
}

/// Sum / average / min / max over the parsed values, at their common scale.
fn numeric_of(vals: &[Fixed]) -> Option<NumericAggregates> {
    let scale = vals.iter().map(|f| f.scale).max()?;
    let mut sum = 0i128;
    let mut min = i128::MAX;
    let mut max = i128::MIN;
    for v in vals {
        let u = v.rescale(scale)?.units;
        sum = sum.checked_add(u)?;
        min = min.min(u);
        max = max.max(u);
    }
    // The average divides, so it carries extra places its inputs never had, then
    // sheds the ones that turned out to be zeros.
    let n = vals.len() as i128;
    let avg = Fixed {
        units: sum.checked_mul(10i128.checked_pow(AVG_EXTRA_SCALE)?)? / n,
        scale: scale + AVG_EXTRA_SCALE,
    }
    .trimmed();
    Some(NumericAggregates {
        sum: Fixed { units: sum, scale },
        avg,
        min: Fixed { units: min, scale },
        max: Fixed { units: max, scale },
    })
}

/// Group a rendered number's integer part in threes: `1234567.89` →
/// `1,234,567.89`.
///
/// Applied to the **arithmetic only**, not to the row and NULL counts beside it:
/// a total is a quantity you read off and compare, where a selection count is
/// small and reads as a plain number. It groups the integer part and leaves the
/// fraction alone, which is what separates `1,234.5678` from a phone number.
fn grouped(s: &str) -> String {
    let (sign, rest) = match s.strip_prefix('-') {
        Some(r) => ("-", r),
        None => ("", s),
    };
    let (int_part, frac) = match rest.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (rest, None),
    };
    let mut out = String::with_capacity(rest.len() + rest.len() / 3 + 1);
    for (i, c) in int_part.chars().enumerate() {
        // A separator every three digits, counted from the right.
        if i > 0 && (int_part.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    match frac {
        Some(f) => format!("{sign}{out}.{f}"),
        None => format!("{sign}{out}"),
    }
}

impl Aggregates {
    /// The readout, as the panel shows it: `10 rows · Sum 45,600.60 ·
    /// Avg 4.56 · Min 1.00 · Max 9.99`, or just the counts for a column with no
    /// arithmetic.
    ///
    /// The NULL count appears only when there is one, so an ordinary selection
    /// isn't padded with `0 null`.
    pub fn summary(&self) -> String {
        let mut parts = vec![format!(
            "{} {}",
            self.rows,
            crate::text::plural(self.rows, "row", "rows")
        )];
        let nulls = self.rows - self.non_null;
        if nulls > 0 {
            parts.push(format!("{nulls} null"));
        }
        if let Some(n) = &self.numeric {
            parts.push(format!("Sum {}", grouped(&n.sum.text())));
            parts.push(format!("Avg {}", grouped(&n.avg.text())));
            parts.push(format!("Min {}", grouped(&n.min.text())));
            parts.push(format!("Max {}", grouped(&n.max.text())));
        }
        parts.join(" · ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ResultSet, Value};

    fn col(type_name: &str) -> Column {
        Column {
            name: "c".to_string(),
            type_name: type_name.to_string(),
            origin: None,
        }
    }

    /// Aggregate one column of a result built from `rows`.
    fn agg_of(type_name: &str, rows: &[Value]) -> Aggregates {
        let rs = ResultSet::from_rows(
            vec![col(type_name)],
            rows.iter().map(|v| vec![v.clone()]).collect(),
        );
        let cells: Vec<CellRef> = (0..rs.row_count()).filter_map(|r| rs.cell(r, 0)).collect();
        aggregate(&rs.columns[0], cells.into_iter())
    }

    #[test]
    fn parses_a_decimal_literal_as_the_wire_sends_it() {
        assert_eq!(
            Fixed::parse("12"),
            Some(Fixed {
                units: 12,
                scale: 0
            })
        );
        assert_eq!(
            Fixed::parse("12.34"),
            Some(Fixed {
                units: 1234,
                scale: 2
            })
        );
        assert_eq!(
            Fixed::parse("-0.50"),
            Some(Fixed {
                units: -50,
                scale: 2
            })
        );
        assert_eq!(Fixed::parse("  7 "), Some(Fixed { units: 7, scale: 0 }));
        assert_eq!(
            Fixed::parse(".5"),
            Some(Fixed { units: 5, scale: 1 }),
            "a leading dot is what some servers send for 0.5"
        );
    }

    #[test]
    fn rejects_what_is_not_a_plain_decimal() {
        for s in ["", "  ", "abc", "1e3", "1.2.3", "1,234", "0x10", "-", "1 2"] {
            assert_eq!(Fixed::parse(s), None, "{s:?}");
        }
    }

    /// The reason this module doesn't use `f64`. In binary floating point
    /// `0.1 + 0.2 != 0.3`; here the sum of a price column is exact and keeps the
    /// column's own number of decimals.
    #[test]
    fn a_money_column_sums_exactly() {
        let a = agg_of(
            "decimal(10,2)",
            &[
                Value::Str("0.10".into()),
                Value::Str("0.20".into()),
                Value::Str("45.30".into()),
            ],
        );
        let n = a.numeric.unwrap();
        assert_eq!(n.sum.text(), "45.60");
        assert_eq!(n.min.text(), "0.10");
        assert_eq!(n.max.text(), "45.30");
    }

    /// Mixed scales meet at the widest, so nothing is truncated on the way in.
    #[test]
    fn values_of_different_scales_sum_at_the_widest() {
        let a = agg_of(
            "numeric",
            &[Value::Str("1.5".into()), Value::Str("2.25".into())],
        );
        assert_eq!(a.numeric.unwrap().sum.text(), "3.75");
    }

    #[test]
    fn integers_sum_without_inventing_decimals() {
        let a = agg_of("int", &[Value::Int(2), Value::Int(4), Value::Int(9)]);
        let n = a.numeric.unwrap();
        assert_eq!(n.sum.text(), "15");
        assert_eq!(n.avg.text(), "5");
        assert_eq!(n.min.text(), "2");
        assert_eq!(n.max.text(), "9");
    }

    /// The average is the one operation that divides, so it may carry decimals
    /// its inputs never had — and sheds the ones that are zeros.
    #[test]
    fn the_average_carries_decimals_its_inputs_did_not() {
        let a = agg_of("int", &[Value::Int(1), Value::Int(2)]);
        assert_eq!(a.numeric.unwrap().avg.text(), "1.5");
        let b = agg_of("int", &[Value::Int(1), Value::Int(3), Value::Int(3)]);
        assert_eq!(b.numeric.unwrap().avg.text(), "2.333333");
    }

    /// NULLs are counted but excluded from the arithmetic, and the average
    /// divides by the values it actually had.
    #[test]
    fn nulls_are_counted_and_left_out_of_the_arithmetic() {
        let a = agg_of("int", &[Value::Int(2), Value::Null, Value::Int(4)]);
        assert_eq!(a.rows, 3);
        assert_eq!(a.non_null, 2);
        let n = a.numeric.unwrap();
        assert_eq!(n.sum.text(), "6");
        assert_eq!(n.avg.text(), "3", "divides by 2, not 3");
    }

    #[test]
    fn a_selection_of_only_nulls_has_no_arithmetic() {
        let a = agg_of("int", &[Value::Null, Value::Null]);
        assert_eq!((a.rows, a.non_null), (2, 0));
        assert!(a.numeric.is_none());
    }

    /// There is nothing to sum in a name — the counts still answer "how many did
    /// I select", which is most of what the readout is for on a text column.
    #[test]
    fn a_text_column_gets_counts_and_no_arithmetic() {
        let a = agg_of(
            "varchar(50)",
            &[
                Value::Str("alice".into()),
                Value::Null,
                Value::Str("bob".into()),
            ],
        );
        assert_eq!((a.rows, a.non_null), (3, 2));
        assert!(a.numeric.is_none());
        assert_eq!(a.summary(), "3 rows · 1 null");
    }

    /// A numeric column whose cell isn't a number is counted as present and left
    /// out of the sum, rather than guessed at or silently treated as zero.
    #[test]
    fn an_unparseable_cell_in_a_numeric_column_is_skipped_not_zeroed() {
        let a = agg_of(
            "decimal(10,2)",
            &[Value::Str("1.00".into()), Value::Str("n/a".into())],
        );
        assert_eq!((a.rows, a.non_null), (2, 2));
        let n = a.numeric.unwrap();
        assert_eq!(n.sum.text(), "1.00");
        assert_eq!(n.avg.text(), "1", "averaged over the one value that parsed");
    }

    #[test]
    fn an_empty_selection_aggregates_to_nothing() {
        let a = agg_of("int", &[]);
        assert_eq!((a.rows, a.non_null), (0, 0));
        assert!(a.numeric.is_none());
        assert_eq!(a.summary(), "0 rows");
    }

    /// A total too large for the accumulator is absent rather than wrapped —
    /// a silently wrong sum is worse than no sum.
    #[test]
    fn an_overflowing_total_degrades_instead_of_wrapping() {
        let huge = i128::MAX.to_string();
        let a = agg_of("numeric", &[Value::Str(huge.clone()), Value::Str(huge)]);
        assert_eq!(a.non_null, 2);
        assert!(a.numeric.is_none());
    }

    #[test]
    fn groups_the_integer_part_in_threes() {
        assert_eq!(grouped("1234"), "1,234");
        assert_eq!(grouped("10000"), "10,000");
        assert_eq!(grouped("1000000"), "1,000,000");
        assert_eq!(grouped("999"), "999", "nothing to group");
        assert_eq!(grouped("-1234567"), "-1,234,567");
    }

    /// The fraction is left alone — grouping it too is what would turn a total
    /// into something that reads like a phone number.
    #[test]
    fn grouping_leaves_the_decimals_alone() {
        assert_eq!(grouped("1234.5678"), "1,234.5678");
        assert_eq!(grouped("0.50"), "0.50");
        assert_eq!(grouped("-0.05"), "-0.05");
        assert_eq!(grouped("12345.60"), "12,345.60");
    }

    /// The counts stay plain: only the arithmetic is grouped.
    #[test]
    fn the_row_count_is_not_grouped() {
        let rows: Vec<Value> = (0..1500).map(Value::Int).collect();
        let a = agg_of("int", &rows);
        let s = a.summary();
        assert!(s.starts_with("1500 rows"), "got {s}");
        assert!(s.contains("Sum 1,124,250"), "got {s}");
    }

    #[test]
    fn the_summary_reads_as_one_line() {
        let a = agg_of(
            "decimal(10,2)",
            &[Value::Str("1.00".into()), Value::Str("2.00".into())],
        );
        assert_eq!(
            a.summary(),
            "2 rows · Sum 3.00 · Avg 1.5 · Min 1.00 · Max 2.00"
        );
    }

    #[test]
    fn negative_values_render_with_their_sign() {
        assert_eq!(
            Fixed {
                units: -5,
                scale: 2
            }
            .text(),
            "-0.05"
        );
        assert_eq!(
            Fixed {
                units: -5,
                scale: 0
            }
            .text(),
            "-5"
        );
    }
}
