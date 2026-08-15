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
/// *checked* and the aggregate degrades rather than wrapping — a silently wrong
/// total is worse than an absent one.
///
/// The headroom above is the **sum's**. The average used to consume six orders
/// of it before dividing, so a `DECIMAL(38,10)` selection totalling ≥1.7 × 10³²
/// — inside the ceiling this paragraph advertises — produced no aggregate at
/// all, average *and* Sum/Min/Max. [`NumericAggregates::avg`] is now an
/// `Option` and divides before it scales, so the only thing an unrepresentable
/// mean costs is the mean.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Fixed {
    pub units: i128,
    pub scale: u32,
}

/// What a cell's text turned out to be. See [`Fixed::parse`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Parsed {
    /// A number this module can represent exactly.
    Value(Fixed),
    /// A number, but one no `i128` can hold — a `DOUBLE` cell of `1e39` arrives
    /// as forty digits.
    Overflow,
    /// Not a number at all: `"n/a"`, `"1e3"`, an empty cell.
    NotANumber,
}

impl Fixed {
    /// Parse a decimal literal as the wire delivers it: optional sign, digits,
    /// optional fraction. No exponent — a `DECIMAL` column never sends one, and
    /// accepting `1e3` here would mean accepting it from a `VARCHAR` too.
    ///
    /// **`Overflow` is a third answer, not a second kind of failure**, and that
    /// distinction is the one place this module could print a number that is
    /// simply untrue. Every *post*-parse overflow already degraded the whole
    /// aggregate to `None`; this one returned the same `None` as `"n/a"`, and
    /// [`aggregate`] then treated the cell as present-but-not-a-number —
    /// counted in `rows`, left out of the arithmetic. So a `DOUBLE` column
    /// holding `1e39` and `2` reported `Sum 2`: not a refusal to answer, an
    /// answer that was wrong, under a selection the user could see two values
    /// in. `Value::Float` stores `f64::to_string()`, which never uses exponent
    /// form, so the forty digits really do arrive here.
    pub fn parse(s: &str) -> Parsed {
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
            return Parsed::NotANumber;
        }
        if !int_part.bytes().all(|b| b.is_ascii_digit())
            || !frac_part.bytes().all(|b| b.is_ascii_digit())
        {
            return Parsed::NotANumber;
        }
        let mut units: i128 = 0;
        for b in int_part.bytes().chain(frac_part.bytes()) {
            let Some(next) = units
                .checked_mul(10)
                .and_then(|u| u.checked_add((b - b'0') as i128))
            else {
                return Parsed::Overflow;
            };
            units = next;
        }
        Parsed::Value(Fixed {
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
    /// `None` when the mean can't be represented — which is a far narrower case
    /// than it was, since the division now happens before the scaling.
    ///
    /// Its own `Option` because it is the only member that can fail on its own:
    /// carried as a plain `Fixed`, an average that overflowed took Sum, Min and
    /// Max down with it through `numeric_of`'s `?`, and a total that fits was
    /// withheld because the *mean* of it didn't.
    pub avg: Option<Fixed>,
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
    aggregate_texts(column, cells.map(|c| (!c.is_null()).then(|| c.text())))
}

/// [`aggregate`] over cell *texts*, `None` meaning NULL.
///
/// The stored cell is not always the cell on screen: a staged (green) edit and a
/// pending new row are both visible and neither is in the [`crate::model::ResultSet`].
/// The readout has to add up what the user can *see*, or it sits under an edit
/// it silently doesn't include — and the whole point of a total beside a
/// selection is that the two agree.
pub fn aggregate_texts<'a>(
    column: &Column,
    cells: impl Iterator<Item = Option<&'a str>>,
) -> Aggregates {
    let numeric_column = column.is_numeric();
    let mut agg = Aggregates::default();
    // `None` once anything has overflowed: there is then no honest number to
    // report, only a total over some of what was selected.
    let mut fold = Some(Fold::default());
    for cell in cells {
        agg.rows += 1;
        let Some(text) = cell else {
            continue;
        };
        agg.non_null += 1;
        if numeric_column {
            match Fixed::parse(text) {
                Parsed::Value(f) => fold = fold.and_then(|acc| acc.push(f)),
                // A number too wide to hold degrades the whole aggregate, the
                // same answer every later overflow gives. Skipping it — which is
                // what an untyped `None` bought — left a Sum computed over
                // *some* of the selected values and labelled as if it were all
                // of them.
                Parsed::Overflow => fold = None,
                // Deliberately not: the column type says what the cell should
                // be, and a wire value that isn't a number is not something to
                // guess about. It stays counted in `rows`/`non_null`.
                Parsed::NotANumber => {}
            }
        }
    }
    agg.numeric = fold.and_then(Fold::finish);
    agg
}

/// The running total, min and max, held at the widest scale seen so far.
///
/// A fold rather than a `Vec<Fixed>` and a second pass over it. The vector cost
/// 32 bytes per selected cell, allocated and freed on **every** recompute — 6.4
/// MB for a Ctrl+A over a 200k-row column, on the UI thread, once per
/// auto-repeat of Shift+↑ and once per cell crossed in a drag. Nothing in the
/// arithmetic needs the values kept, only the widest scale, and the fold can
/// raise its own accumulators when it meets one.
#[derive(Clone, Copy, Debug, Default)]
struct Fold {
    scale: u32,
    sum: i128,
    min: i128,
    max: i128,
    n: i128,
}

impl Fold {
    /// `None` on overflow, which is how every arithmetic failure here reports:
    /// no number rather than a wrong one.
    fn push(mut self, v: Fixed) -> Option<Fold> {
        if v.scale > self.scale {
            let factor = 10i128.checked_pow(v.scale - self.scale)?;
            self.sum = self.sum.checked_mul(factor)?;
            self.min = self.min.checked_mul(factor)?;
            self.max = self.max.checked_mul(factor)?;
            self.scale = v.scale;
        }
        // Overflows on scale *spread* alone: `0.000…1` at scale 34 beside
        // `100000` needs 10³⁹ units for the second value, whatever the total
        // would have been. Left as a degrade rather than fixed, deliberately —
        // the fix is to cap the working scale, which would make this the one
        // place in the module that rounds, and rounding the inputs to a Sum is
        // precisely what the fixed-point arithmetic exists to avoid. It reports
        // no number, never a wrong one.
        let u = v.rescale(self.scale)?.units;
        self.sum = self.sum.checked_add(u)?;
        if self.n == 0 {
            self.min = u;
            self.max = u;
        } else {
            self.min = self.min.min(u);
            self.max = self.max.max(u);
        }
        self.n += 1;
        Some(self)
    }

    /// `None` when nothing parsed — all NULL, all unparseable, or a non-numeric
    /// column, which never pushes at all.
    fn finish(self) -> Option<NumericAggregates> {
        if self.n == 0 {
            return None;
        }
        let scale = self.scale;
        Some(NumericAggregates {
            sum: Fixed {
                units: self.sum,
                scale,
            },
            avg: mean(self.sum, scale, self.n),
            min: Fixed {
                units: self.min,
                scale,
            },
            max: Fixed {
                units: self.max,
                scale,
            },
        })
    }
}

/// The mean of `n` values totalling `sum` at `scale`, carrying
/// [`AVG_EXTRA_SCALE`] places its inputs never had — then shedding the ones that
/// turned out to be zeros.
///
/// **Divides before it scales.** `sum × 10⁶ / n` spends six orders of the
/// accumulator's headroom up front, so any total above ~1.7 × 10³² overflowed —
/// well inside the range the sum itself handles, and the doc above claimed as
/// comfortable. Taking the quotient and the remainder separately means only the
/// *quotient* is scaled, and the remainder is smaller than `n`, so its own
/// scaling can't overflow for any selection a grid can hold.
fn mean(sum: i128, scale: u32, n: i128) -> Option<Fixed> {
    let factor = 10i128.checked_pow(AVG_EXTRA_SCALE)?;
    let (q, r) = (sum / n, sum % n);
    let units = q
        .checked_mul(factor)?
        .checked_add(r.checked_mul(factor)? / n)?;
    Some(
        Fixed {
            units,
            scale: scale + AVG_EXTRA_SCALE,
        }
        .trimmed(),
    )
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
            // Omitted, not printed as a placeholder, when the mean can't be
            // represented: the other three are exact and saying so about them is
            // the whole job. An `Avg —` beside them would read as a value.
            if let Some(avg) = n.avg {
                parts.push(format!("Avg {}", grouped(&avg.text())));
            }
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
            Parsed::Value(Fixed {
                units: 12,
                scale: 0
            })
        );
        assert_eq!(
            Fixed::parse("12.34"),
            Parsed::Value(Fixed {
                units: 1234,
                scale: 2
            })
        );
        assert_eq!(
            Fixed::parse("-0.50"),
            Parsed::Value(Fixed {
                units: -50,
                scale: 2
            })
        );
        assert_eq!(
            Fixed::parse("  7 "),
            Parsed::Value(Fixed { units: 7, scale: 0 })
        );
        assert_eq!(
            Fixed::parse(".5"),
            Parsed::Value(Fixed { units: 5, scale: 1 }),
            "a leading dot is what some servers send for 0.5"
        );
    }

    #[test]
    fn rejects_what_is_not_a_plain_decimal() {
        for s in ["", "  ", "abc", "1e3", "1.2.3", "1,234", "0x10", "-", "1 2"] {
            assert_eq!(Fixed::parse(s), Parsed::NotANumber, "{s:?}");
        }
    }

    /// **A number too wide is not the same answer as "not a number".** Both were
    /// `None`, so [`aggregate`] skipped an unrepresentable value exactly as it
    /// skips `"n/a"` — and reported a Sum over the rest as if it were the whole
    /// selection.
    #[test]
    fn a_value_wider_than_i128_is_overflow_not_a_parse_failure() {
        assert_eq!(Fixed::parse(&"9".repeat(39)), Parsed::Overflow);
        assert_eq!(
            Fixed::parse(&format!("-{}", "9".repeat(39))),
            Parsed::Overflow
        );
        // The fraction counts toward the same accumulator, so the digits either
        // side of the point are one budget.
        assert_eq!(
            Fixed::parse(&format!("{}.{}", "9".repeat(20), "9".repeat(19))),
            Parsed::Overflow
        );
        // 38 nines still fits.
        assert!(matches!(Fixed::parse(&"9".repeat(38)), Parsed::Value(_)));
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
        assert_eq!(n.avg.unwrap().text(), "5");
        assert_eq!(n.min.text(), "2");
        assert_eq!(n.max.text(), "9");
    }

    /// The average is the one operation that divides, so it may carry decimals
    /// its inputs never had — and sheds the ones that are zeros.
    #[test]
    fn the_average_carries_decimals_its_inputs_did_not() {
        let a = agg_of("int", &[Value::Int(1), Value::Int(2)]);
        assert_eq!(a.numeric.unwrap().avg.unwrap().text(), "1.5");
        let b = agg_of("int", &[Value::Int(1), Value::Int(3), Value::Int(3)]);
        assert_eq!(b.numeric.unwrap().avg.unwrap().text(), "2.333333");
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
        assert_eq!(n.avg.unwrap().text(), "3", "divides by 2, not 3");
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
        assert_eq!(
            n.avg.unwrap().text(),
            "1",
            "averaged over the one value that parsed"
        );
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
    ///
    /// Overflow site 3 of 4: the running `checked_add`. The other three are
    /// below; they are tested apart because they did **not** degrade alike, and
    /// that is how three different answers to one question shipped.
    #[test]
    fn an_overflowing_total_degrades_instead_of_wrapping() {
        let huge = i128::MAX.to_string();
        let a = agg_of("numeric", &[Value::Str(huge.clone()), Value::Str(huge)]);
        assert_eq!(a.non_null, 2);
        assert!(a.numeric.is_none());
    }

    /// Overflow site 1 of 4, and the only one that ever produced a *wrong*
    /// number: a value wider than i128 was skipped like `"n/a"`, so the bar
    /// reported the total of everything else under a selection that visibly
    /// contained more. `1e39` is what a `DOUBLE` column really sends — through
    /// `f64::to_string()`, which never uses exponent form, so it arrives as
    /// forty digits.
    #[test]
    fn a_value_too_wide_to_hold_degrades_rather_than_being_left_out() {
        let a = agg_of("double", &[Value::Float(1e39), Value::Float(2.0)]);
        assert_eq!((a.rows, a.non_null), (2, 2), "both cells are still counted");
        assert!(
            a.numeric.is_none(),
            "reported {:?} — a Sum over part of the selection",
            a.numeric
        );
    }

    /// Overflow site 2 of 4: bringing values to a common scale. Driven by the
    /// *spread* of scales rather than by magnitude, so it can erase a total that
    /// would have fitted comfortably. Left as a degrade on purpose — capping the
    /// working scale would make this the module's first rounding decision, and
    /// rounding the inputs to a Sum is what the fixed-point arithmetic exists to
    /// avoid. It never reports a wrong number, only no number.
    #[test]
    fn scale_spread_alone_can_erase_the_arithmetic() {
        let tiny = format!("0.{}1", "0".repeat(33)); // scale 34
        let a = agg_of(
            "decimal(65,34)",
            &[Value::Str(tiny), Value::Str("100000".into())],
        );
        assert_eq!(a.non_null, 2);
        assert!(a.numeric.is_none());
    }

    /// Overflow site 4 of 4, and the one that must **not** take the others with
    /// it. The average alone divides, so it alone can fail to be representable;
    /// carried as a plain `Fixed` it propagated `?` and withheld an exact Sum,
    /// Min and Max because the *mean* of them didn't fit.
    #[test]
    fn an_unrepresentable_average_costs_only_the_average() {
        // A single value large enough that `× 10^AVG_EXTRA_SCALE` cannot fit,
        // while the sum itself is exact.
        let huge = i128::MAX.to_string();
        let a = agg_of("numeric", &[Value::Str(huge.clone())]);
        let n = a
            .numeric
            .as_ref()
            .expect("the sum fits and must still be reported");
        assert_eq!(n.sum.text(), huge);
        assert_eq!(n.min.text(), huge);
        assert_eq!(n.max.text(), huge);
        assert!(n.avg.is_none());
        let s = a.summary();
        assert!(s.contains("Sum "), "got {s}");
        assert!(!s.contains("Avg"), "an absent average is omitted, got {s}");
    }

    /// The headroom argument in [`Fixed`]'s own doc, made a test: a
    /// `DECIMAL(38,10)` selection whose units total ~2 × 10³⁴ is what that
    /// paragraph calls comfortable, and it produced **no aggregate at all**
    /// because the average spent six orders of magnitude before dividing.
    #[test]
    fn a_sum_that_fits_still_reports_when_the_average_would_have_overflowed() {
        // 1000 values of 2 × 10²¹ at scale 10: the total is 2 × 10³⁴ units,
        // inside i128 and outside i128 ÷ 10⁶ — the case `Fixed`'s own doc calls
        // comfortable, and which produced no aggregate at all.
        let v = format!("2{}.{}", "0".repeat(21), "0".repeat(10));
        let rows = vec![Value::Str(v); 1000];
        let a = agg_of("decimal(38,10)", &rows);
        let n = a.numeric.expect("the total fits and must be reported");
        assert_eq!(
            n.sum.text(),
            format!("2{}.{}", "0".repeat(24), "0".repeat(10))
        );
        assert_eq!(
            n.avg
                .expect("dividing before scaling keeps this one representable")
                .text(),
            format!("2{}", "0".repeat(21))
        );
    }

    /// `Value::Float` is the one input path this crate formats itself, and no
    /// test used it — which is why the forty-digit string it can produce went
    /// unnoticed. Ordinary floats must still aggregate.
    #[test]
    fn a_float_column_aggregates_through_its_own_rendering() {
        let a = agg_of("double", &[Value::Float(1.5), Value::Float(2.25)]);
        let n = a.numeric.unwrap();
        assert_eq!(n.sum.text(), "3.75");
        assert_eq!(n.min.text(), "1.50");
        assert_eq!(n.max.text(), "2.25");
    }

    /// `NaN` and the infinities render as words, so they are cells that aren't
    /// numbers — present, counted, and out of the arithmetic, exactly like
    /// `"n/a"`. Distinct from the overflow above, which degrades everything.
    #[test]
    fn a_float_that_is_not_a_number_is_skipped_not_overflowed() {
        let a = agg_of(
            "double",
            &[
                Value::Float(f64::NAN),
                Value::Float(2.0),
                Value::Float(f64::INFINITY),
            ],
        );
        assert_eq!((a.rows, a.non_null), (3, 3));
        let n = a.numeric.expect("the one real value still aggregates");
        assert_eq!(n.sum.text(), "2");
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
