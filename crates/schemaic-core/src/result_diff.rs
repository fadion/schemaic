//! **A diff of two results** — the rows one result has that the other has not,
//! and the rows both have with different values, matched by a key the user
//! picks (a primary key, or any columns that name a row). Behind the results
//! strip's *Compare with*: a kept result against a fresh run of the same
//! statement, or two kept ones.
//!
//! Pure over two [`ResultSet`]s. **Columns are matched by name**, so a column
//! one side has and the other has not is reported and left out of the
//! comparison rather than failing it. **A key repeated within one side** is
//! counted and its later rows ignored: the diff can only say which row a key
//! names when it names one.
//!
//! **With no key, the whole row is the key**: a row is then either in both or
//! in one, never "changed", and a row repeated n times on one side and m on the
//! other is |n − m| rows added or removed. That is the honest answer for a
//! result with nothing that identifies a row.
//!
//! Values compare as the grid shows them — by their text, NULL being its own
//! value — so `1` and `1.0` from two engines differ, as they would on screen.
//! **Except where that text is not the value**: a binary column's cells read
//! `<n bytes>`, which would call two different blobs of one length the same,
//! so such a column is left out of the comparison, never a key, and named as
//! not compared ([`ResultDiff::uncompared`]).

use std::collections::HashMap;

use crate::model::{CellTag, ResultSet};

/// One row's key or whole-row identity: each cell's text, `None` for NULL —
/// **borrowed from the result**, never copied: a `String` per cell cost
/// 0.35 s keyed and 0.68 s whole-row per open or swap at 200k × 20
/// (release), on the UI thread.
type RowKey<'a> = Vec<Option<&'a str>>;

/// A row present in both results with at least one value that differs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Changed {
    /// The row's index in the first result and in the second.
    pub left: usize,
    pub right: usize,
    /// The compared columns whose values differ, by name.
    pub columns: Vec<String>,
}

/// What two results differ by.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResultDiff {
    /// The columns both have, in the first result's order — the ones compared.
    pub shared: Vec<String>,
    /// Columns only the first result has, and only the second.
    pub only_left: Vec<String>,
    pub only_right: Vec<String>,
    /// Rows of the first result with no match in the second ("removed").
    pub left_only: Vec<usize>,
    /// Rows of the second result with no match in the first ("added").
    pub right_only: Vec<usize>,
    pub changed: Vec<Changed>,
    /// Rows matched with every compared value the same.
    pub same: usize,
    /// Rows ignored because their key was already taken on the same side.
    pub repeated_keys: usize,
    /// Columns both results have that were **not compared**, because one side
    /// or the other holds raw bytes there: such a cell's text is
    /// [`crate::model::binary_display`]'s `<n bytes>`, not its value, so two
    /// different blobs of one length would read as the same. Not in
    /// [`Self::shared`], never a key, and said in the summary.
    pub uncompared: Vec<String>,
}

impl ResultDiff {
    /// Nothing to report: the same rows with the same values.
    pub fn is_identical(&self) -> bool {
        self.left_only.is_empty() && self.right_only.is_empty() && self.changed.is_empty()
    }

    /// The headline: "2 changed · 3 added · 1 removed · 40 unchanged", a kind
    /// with no rows left out — the filter pills' order.
    pub fn summary(&self) -> String {
        if self.is_identical() {
            return format!(
                "No differences · {} {} unchanged{}",
                self.same,
                crate::text::plural(self.same, "row", "rows"),
                self.uncompared_clause()
            );
        }
        let mut parts = Vec::new();
        for (n, word) in [
            (self.changed.len(), "changed"),
            (self.right_only.len(), "added"),
            (self.left_only.len(), "removed"),
        ] {
            if n > 0 {
                parts.push(format!("{n} {word}"));
            }
        }
        parts.push(format!("{} unchanged", self.same));
        parts.join(" · ") + &self.uncompared_clause()
    }

    /// `" · pw_hash not compared"` when a binary column was left out, so
    /// "unchanged" is never read as covering it.
    fn uncompared_clause(&self) -> String {
        if self.uncompared.is_empty() {
            String::new()
        } else {
            format!(" · {} not compared", self.uncompared.join(", "))
        }
    }
}

/// What a row of the comparison table is: a row both results have with values
/// that differ, or a row only one of them has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowChange {
    Changed,
    /// Only in the second result.
    Added,
    /// Only in the first.
    Removed,
}

impl RowChange {
    /// The table's *Change* column.
    pub fn label(self) -> &'static str {
        match self {
            RowChange::Changed => "CHANGED",
            RowChange::Added => "ADDED",
            RowChange::Removed => "REMOVED",
        }
    }
}

/// One cell of the comparison table, as one line of text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffCell {
    /// The row's own value (an added or removed row), or one both sides agree on.
    Value(String),
    /// A value that differs: the first result's, then the second's.
    Changed(String, String),
}

/// One row of the comparison table, **by reference**: what kind it is and
/// where it is — a removed row's index in the first result, an added row's in
/// the second, a changed row's in [`ResultDiff::changed`]. No cell text: that
/// is built by [`row_cells`] for the rows a view actually mounts, since
/// building every differing cell up front was a string or three per cell of
/// every differing row, on the UI thread, at open and on every swap or key
/// change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableRow {
    pub change: RowChange,
    pub at: usize,
}

/// The differences in `d` as a table under its shared columns: the first
/// result's changed and removed rows in the order it holds them, then the rows
/// only the second has, in its order. `d` must be `diff(left, right, …)`.
pub fn table_rows(d: &ResultDiff) -> Vec<TableRow> {
    // Changed and removed rows by their place in the first result.
    let mut firsts: Vec<(usize, TableRow)> = d
        .left_only
        .iter()
        .map(|&r| {
            (
                r,
                TableRow {
                    change: RowChange::Removed,
                    at: r,
                },
            )
        })
        .chain(d.changed.iter().enumerate().map(|(i, ch)| {
            (
                ch.left,
                TableRow {
                    change: RowChange::Changed,
                    at: i,
                },
            )
        }))
        .collect();
    firsts.sort_by_key(|(r, _)| *r);
    let mut added = d.right_only.clone();
    added.sort_unstable();
    firsts
        .into_iter()
        .map(|(_, row)| row)
        .chain(added.into_iter().map(|r| TableRow {
            change: RowChange::Added,
            at: r,
        }))
        .collect()
}

/// One table row's cells, one per [`ResultDiff::shared`] column, each one line
/// of text. `d` must be `diff(left, right, …)` and `row` one of its
/// [`table_rows`]; a row out of range has no cells.
pub fn row_cells(
    left: &ResultSet,
    right: &ResultSet,
    d: &ResultDiff,
    row: &TableRow,
) -> Vec<DiffCell> {
    let whole = |rs: &ResultSet, r: usize| -> Vec<DiffCell> {
        d.shared
            .iter()
            .map(|n| DiffCell::Value(table_value(col(rs, n).and_then(|c| cell(rs, r, c)))))
            .collect()
    };
    match row.change {
        RowChange::Removed => whole(left, row.at),
        RowChange::Added => whole(right, row.at),
        RowChange::Changed => {
            let Some(ch) = d.changed.get(row.at) else {
                return Vec::new();
            };
            d.shared
                .iter()
                .map(|name| {
                    let before = col(left, name).and_then(|c| cell(left, ch.left, c));
                    if ch.columns.contains(name) {
                        let after = col(right, name).and_then(|c| cell(right, ch.right, c));
                        DiffCell::Changed(table_value(before), table_value(after))
                    } else {
                        DiffCell::Value(table_value(before))
                    }
                })
                .collect()
        }
    }
}

/// A value as one line of a table cell — [`crate::text::cell_line`].
fn table_value(v: Option<&str>) -> String {
    crate::text::cell_line(v)
}

/// Why two results cannot be compared by the key asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffError {
    /// A key column one side does not have.
    MissingKey(String),
    /// A key column holding raw bytes, whose cells all read `<n bytes>`.
    BinaryKey(String),
}

impl std::fmt::Display for DiffError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiffError::MissingKey(c) => {
                write!(f, "both results need a column named {c} to match rows by")
            }
            DiffError::BinaryKey(c) => {
                write!(
                    f,
                    "{c} holds binary values, which are not compared, so it cannot match rows"
                )
            }
        }
    }
}

/// The columns both results have, by name, in `left`'s order.
pub fn shared_columns(left: &ResultSet, right: &ResultSet) -> Vec<String> {
    left.columns
        .iter()
        .map(|c| c.name.clone())
        .filter(|n| right.columns.iter().any(|c| &c.name == n))
        .collect()
}

/// Does column `name` of `rs` hold raw bytes — by its type, or because the
/// backend saw a blob in it ([`ResultSet::binary_columns`], SQLite's per-value
/// answer)? Its cells' text is then `<n bytes>`, a placeholder, not the value.
fn is_binary(rs: &ResultSet, name: &str) -> bool {
    col(rs, name).is_some_and(|c| rs.columns[c].is_binary() || rs.binary_columns.contains(&c))
}

/// The shared columns that **can** be compared — and so offered as a key: a
/// column binary on either side is not, since its cells are placeholders.
pub fn key_columns(left: &ResultSet, right: &ResultSet) -> Vec<String> {
    shared_columns(left, right)
        .into_iter()
        .filter(|n| !is_binary(left, n) && !is_binary(right, n))
        .collect()
}

/// A key to start from: a comparable shared column ([`key_columns`]) named
/// `id` (any case), else the first one whose values are unique and never NULL
/// on both sides, else none — the whole row. A binary `id` is passed over:
/// every `BINARY(16)` UUID reads `<16 bytes>`, so as a key it matched the
/// first row and called every other a repeat.
pub fn suggested_key(left: &ResultSet, right: &ResultSet) -> Vec<String> {
    let shared = key_columns(left, right);
    if let Some(id) = shared.iter().find(|n| n.eq_ignore_ascii_case("id")) {
        return vec![id.clone()];
    }
    let unique = |rs: &ResultSet, name: &str| {
        let Some(c) = col(rs, name) else { return false };
        let mut seen = std::collections::HashSet::new();
        (0..rs.row_count()).all(|r| match cell(rs, r, c) {
            Some(v) => seen.insert(v),
            None => false,
        })
    };
    shared
        .into_iter()
        .find(|n| unique(left, n) && unique(right, n))
        .into_iter()
        .collect()
}

/// `left` against `right`, rows matched by the `key` columns — or by the whole
/// shared row when `key` is empty.
pub fn diff(left: &ResultSet, right: &ResultSet, key: &[String]) -> Result<ResultDiff, DiffError> {
    let both = shared_columns(left, right);
    let names =
        |rs: &ResultSet| -> Vec<String> { rs.columns.iter().map(|c| c.name.clone()).collect() };
    let only_left: Vec<String> = names(left)
        .into_iter()
        .filter(|n| !both.contains(n))
        .collect();
    let only_right: Vec<String> = names(right)
        .into_iter()
        .filter(|n| !both.contains(n))
        .collect();
    for k in key {
        if !both.contains(k) {
            return Err(DiffError::MissingKey(k.clone()));
        }
    }
    // A binary column is left out of the comparison and said to be: its text
    // is `<n bytes>`, so two blobs of one length would read as the same.
    let compared = key_columns(left, right);
    if let Some(k) = key.iter().find(|k| !compared.contains(k)) {
        return Err(DiffError::BinaryKey(k.clone()));
    }
    let uncompared: Vec<String> = both
        .iter()
        .filter(|n| !compared.contains(n))
        .cloned()
        .collect();
    let shared = compared;
    let cols = |rs: &ResultSet, which: &[String]| -> Vec<usize> {
        which.iter().filter_map(|n| col(rs, n)).collect()
    };
    let mut out = ResultDiff {
        shared: shared.clone(),
        only_left,
        only_right,
        uncompared,
        ..ResultDiff::default()
    };
    if key.is_empty() {
        // Whole-row identity, as a multiset. Each bucket is consumed from the
        // front through a cursor: `remove(0)` shifted the whole bucket per
        // match, quadratic in a column of few distinct values — 300k equal
        // rows took about 25 s (a debug test build), on the UI thread.
        let (lc, rc) = (cols(left, &shared), cols(right, &shared));
        let mut counts: HashMap<RowKey, (Vec<usize>, usize)> = HashMap::new();
        for r in 0..left.row_count() {
            counts.entry(row_key(left, r, &lc)).or_default().0.push(r);
        }
        for r in 0..right.row_count() {
            match counts.get_mut(&row_key(right, r, &rc)) {
                Some((rows, next)) if *next < rows.len() => {
                    *next += 1;
                    out.same += 1;
                }
                _ => out.right_only.push(r),
            }
        }
        let mut gone: Vec<usize> = counts
            .into_values()
            .flat_map(|(rows, next)| rows.into_iter().skip(next))
            .collect();
        gone.sort_unstable();
        out.left_only = gone;
        return Ok(out);
    }
    let (lk, rk) = (cols(left, key), cols(right, key));
    let compared: Vec<&String> = shared.iter().filter(|n| !key.contains(n)).collect();
    let pairs: Vec<(String, usize, usize)> = compared
        .iter()
        .filter_map(|n| Some(((*n).clone(), col(left, n)?, col(right, n)?)))
        .collect();
    // A key with a NULL in it names no row, as in SQL: such a row matches
    // nothing and is its side's alone.
    let null_key = |k: &RowKey| k.iter().any(Option::is_none);
    // A repeated key on the left keeps its first row, as the right does.
    let mut first: HashMap<RowKey, usize> = HashMap::new();
    let mut unkeyed_left = Vec::new();
    for r in 0..left.row_count() {
        let k = row_key(left, r, &lk);
        if null_key(&k) {
            unkeyed_left.push(r);
            continue;
        }
        match first.entry(k) {
            std::collections::hash_map::Entry::Occupied(_) => out.repeated_keys += 1,
            std::collections::hash_map::Entry::Vacant(v) => {
                v.insert(r);
            }
        }
    }
    let mut matched = std::collections::HashSet::new();
    let mut seen_right = std::collections::HashSet::new();
    for r in 0..right.row_count() {
        let k = row_key(right, r, &rk);
        if null_key(&k) {
            out.right_only.push(r);
            continue;
        }
        if !seen_right.insert(k.clone()) {
            out.repeated_keys += 1;
            continue;
        }
        match first.get(&k) {
            Some(&l) => {
                matched.insert(l);
                let differ: Vec<String> = pairs
                    .iter()
                    .filter(|(_, lc, rc)| cell(left, l, *lc) != cell(right, r, *rc))
                    .map(|(n, ..)| n.clone())
                    .collect();
                if differ.is_empty() {
                    out.same += 1;
                } else {
                    out.changed.push(Changed {
                        left: l,
                        right: r,
                        columns: differ,
                    });
                }
            }
            None => out.right_only.push(r),
        }
    }
    out.left_only = first
        .values()
        .copied()
        .filter(|l| !matched.contains(l))
        .chain(unkeyed_left)
        .collect();
    out.left_only.sort_unstable();
    Ok(out)
}

fn col(rs: &ResultSet, name: &str) -> Option<usize> {
    rs.columns.iter().position(|c| c.name == name)
}

fn cell(rs: &ResultSet, row: usize, col: usize) -> Option<&str> {
    let c = rs.cell(row, col)?;
    (c.tag != CellTag::Null).then(|| c.text())
}

fn row_key<'a>(rs: &'a ResultSet, row: usize, cols: &[usize]) -> RowKey<'a> {
    cols.iter().map(|&c| cell(rs, row, c)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Column, ResultBuilder, Value};

    fn rs(cols: &[&str], rows: &[&[Value]]) -> ResultSet {
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
            b.push_row(r);
        }
        b.finish()
    }

    fn i(v: i64) -> Value {
        Value::Int(v)
    }
    fn s(v: &str) -> Value {
        Value::Str(v.into())
    }

    #[test]
    fn rows_matched_by_key_are_added_removed_changed_or_the_same() {
        let before = rs(
            &["id", "name", "qty"],
            &[
                &[i(1), s("a"), i(5)],
                &[i(2), s("b"), i(6)],
                &[i(3), s("c"), i(7)],
            ],
        );
        let after = rs(
            &["id", "name", "qty"],
            &[
                &[i(1), s("a"), i(5)],
                &[i(3), s("c"), i(9)],
                &[i(4), s("d"), i(1)],
            ],
        );
        let d = diff(&before, &after, &["id".to_string()]).unwrap();
        assert_eq!(d.left_only, [1], "id 2 removed");
        assert_eq!(d.right_only, [2], "id 4 added");
        assert_eq!(
            d.changed,
            [Changed {
                left: 2,
                right: 1,
                columns: vec!["qty".into()]
            }]
        );
        assert_eq!(d.same, 1);
        assert_eq!(d.summary(), "1 changed · 1 added · 1 removed · 1 unchanged");
    }

    #[test]
    fn the_summary_leaves_out_a_kind_with_no_rows() {
        let a = rs(&["id"], &[&[i(1)], &[i(2)]]);
        let b = rs(&["id"], &[&[i(1)], &[i(2)], &[i(3)]]);
        let d = diff(&a, &b, &["id".into()]).unwrap();
        assert_eq!(d.summary(), "1 added · 2 unchanged");
    }

    fn v(s: &str) -> DiffCell {
        DiffCell::Value(s.into())
    }

    /// A table row with its cells built, as a mounted row is.
    #[derive(Debug, PartialEq)]
    struct DiffRow {
        change: RowChange,
        cells: Vec<DiffCell>,
    }

    /// The whole table built — `table_rows` then `row_cells` for each, which
    /// is the composition the view makes one mounted row at a time.
    fn table(left: &ResultSet, right: &ResultSet, d: &ResultDiff) -> Vec<DiffRow> {
        table_rows(d)
            .iter()
            .map(|r| DiffRow {
                change: r.change,
                cells: row_cells(left, right, d, r),
            })
            .collect()
    }

    /// The table's rows say where each row is, and a row index that does not
    /// name a change builds no cells rather than panicking.
    #[test]
    fn table_rows_hold_places_and_row_cells_builds_one_row() {
        let a = rs(&["id", "v"], &[&[i(1), s("old")], &[i(2), s("x")]]);
        let b = rs(&["id", "v"], &[&[i(1), s("new")], &[i(3), s("y")]]);
        let d = diff(&a, &b, &["id".into()]).unwrap();
        assert_eq!(
            table_rows(&d),
            [
                TableRow {
                    change: RowChange::Changed,
                    at: 0
                },
                TableRow {
                    change: RowChange::Removed,
                    at: 1
                },
                TableRow {
                    change: RowChange::Added,
                    at: 1
                },
            ]
        );
        assert_eq!(
            row_cells(&a, &b, &d, &table_rows(&d)[0]),
            [v("1"), DiffCell::Changed("old".into(), "new".into())]
        );
        let stray = TableRow {
            change: RowChange::Changed,
            at: 9,
        };
        assert!(row_cells(&a, &b, &d, &stray).is_empty());
    }

    /// The table reads the first result top to bottom — its changed and
    /// removed rows where they stood — then the rows only the second has, in
    /// its order; every cell under the shared columns.
    #[test]
    fn the_table_lists_the_first_results_rows_in_order_then_the_added() {
        let before = rs(
            &["id", "name", "gone"],
            &[
                &[i(1), s("a"), s("x")],
                &[i(2), s("b"), s("x")],
                &[i(3), s("c"), s("x")],
                &[i(4), s("d"), s("x")],
            ],
        );
        let after = rs(
            &["id", "name"],
            &[
                &[i(9), s("z")],
                &[i(4), s("D")],
                &[i(1), s("a")],
                &[i(8), s("y")],
                &[i(3), s("c")],
            ],
        );
        let d = diff(&before, &after, &["id".into()]).unwrap();
        let rows = table(&before, &after, &d);
        assert_eq!(
            rows,
            [
                DiffRow {
                    change: RowChange::Removed,
                    cells: vec![v("2"), v("b")],
                },
                DiffRow {
                    change: RowChange::Changed,
                    cells: vec![v("4"), DiffCell::Changed("d".into(), "D".into())],
                },
                DiffRow {
                    change: RowChange::Added,
                    cells: vec![v("9"), v("z")],
                },
                DiffRow {
                    change: RowChange::Added,
                    cells: vec![v("8"), v("y")],
                },
            ]
        );
    }

    /// Swapping the sides is the same diff read the other way: what was
    /// added is removed, and every change runs backwards.
    #[test]
    fn swapping_the_sides_swaps_added_for_removed_and_reverses_each_change() {
        let a = rs(&["id", "v"], &[&[i(1), s("old")], &[i(2), s("x")]]);
        let b = rs(&["id", "v"], &[&[i(1), s("new")], &[i(3), s("y")]]);
        let key = ["id".to_string()];
        let back = diff(&b, &a, &key).unwrap();
        let rows = table(&b, &a, &back);
        assert_eq!(
            rows.iter().map(|r| r.change).collect::<Vec<_>>(),
            [RowChange::Changed, RowChange::Removed, RowChange::Added]
        );
        assert_eq!(
            rows[0].cells[1],
            DiffCell::Changed("new".into(), "old".into())
        );
        assert_eq!(rows[1].cells, [v("3"), v("y")]);
        assert_eq!(rows[2].cells, [v("2"), v("x")]);
    }

    /// A table cell is one line: NULL says so, a line break becomes a space,
    /// and a long value is cut — the comparison still ran on the whole of it.
    #[test]
    fn a_table_cell_is_one_short_line() {
        let long = "z".repeat(500);
        let a = rs(
            &["v"],
            &[
                &[Value::Null],
                &[s("one\r\ntwo")],
                &[s(&long)],
                &[s("a\u{2028}b\u{2029}c\u{85}d\u{0b}e\u{0c}f")],
            ],
        );
        let b = rs(&["v"], &[]);
        let d = diff(&a, &b, &[]).unwrap();
        let rows = table(&a, &b, &d);
        assert_eq!(rows[0].cells, [v("NULL")]);
        assert_eq!(rows[1].cells, [v("one two")]);
        assert_eq!(rows[3].cells, [v("a b c d e f")], "every mandatory break");
        let DiffCell::Value(cut) = &rows[2].cells[0] else {
            panic!("a removed row's cell is a value")
        };
        assert!(cut.ends_with('…') && cut.chars().count() < long.len());
    }

    #[test]
    fn whole_row_identity_has_no_changed_rows_in_the_table() {
        let a = rs(&["x"], &[&[i(1)], &[i(2)]]);
        let b = rs(&["x"], &[&[i(2)], &[i(3)]]);
        let d = diff(&a, &b, &[]).unwrap();
        let rows = table(&a, &b, &d);
        assert_eq!(
            rows.iter().map(|r| r.change).collect::<Vec<_>>(),
            [RowChange::Removed, RowChange::Added]
        );
    }

    #[test]
    fn without_a_key_rows_are_counted_as_a_multiset() {
        let a = rs(&["x"], &[&[i(1)], &[i(1)], &[i(2)]]);
        let b = rs(&["x"], &[&[i(1)], &[i(2)], &[i(2)]]);
        let d = diff(&a, &b, &[]).unwrap();
        assert_eq!(d.left_only, [1], "one of the two 1s is gone");
        assert_eq!(d.right_only, [2], "a second 2 arrived");
        assert_eq!(d.same, 2);
        assert!(d.changed.is_empty());
    }

    #[test]
    fn a_column_on_one_side_only_is_reported_and_not_compared() {
        let a = rs(&["id", "v", "old"], &[&[i(1), s("x"), s("o")]]);
        let b = rs(&["id", "v", "new"], &[&[i(1), s("x"), s("n")]]);
        let d = diff(&a, &b, &["id".into()]).unwrap();
        assert!(d.is_identical());
        assert_eq!(
            (d.only_left.as_slice(), d.only_right.as_slice()),
            (&["old".to_string()][..], &["new".to_string()][..])
        );
        assert_eq!(d.summary(), "No differences · 1 row unchanged");
    }

    #[test]
    fn null_is_its_own_value_and_text_compares_as_shown() {
        let a = rs(&["id", "v"], &[&[i(1), Value::Null], &[i(2), s("1")]]);
        let b = rs(&["id", "v"], &[&[i(1), s("")], &[i(2), s("1.0")]]);
        let d = diff(&a, &b, &["id".into()]).unwrap();
        assert_eq!(d.changed.len(), 2, "NULL ≠ '' and 1 ≠ 1.0");
    }

    /// A NULL key names no row, as in SQL: two rows with a NULL key are not
    /// the same row, so each is its own side's only.
    #[test]
    fn a_null_key_matches_nothing() {
        let a = rs(&["k", "v"], &[&[Value::Null, s("a")]]);
        let b = rs(&["k", "v"], &[&[Value::Null, s("a")]]);
        let d = diff(&a, &b, &["k".into()]).unwrap();
        assert_eq!(
            (d.left_only.as_slice(), d.right_only.as_slice()),
            (&[0][..], &[0][..])
        );
        assert_eq!(d.same, 0);
        assert_eq!(d.repeated_keys, 0);
    }

    #[test]
    fn a_repeated_key_is_counted_and_its_later_rows_ignored() {
        let a = rs(&["k", "v"], &[&[i(1), s("a")], &[i(1), s("b")]]);
        let b = rs(&["k", "v"], &[&[i(1), s("a")]]);
        let d = diff(&a, &b, &["k".into()]).unwrap();
        assert_eq!(d.repeated_keys, 1);
        assert!(d.is_identical());
    }

    #[test]
    fn a_key_one_side_lacks_is_refused() {
        let a = rs(&["id"], &[]);
        let b = rs(&["code"], &[]);
        assert_eq!(
            diff(&a, &b, &["id".into()]),
            Err(DiffError::MissingKey("id".into()))
        );
    }

    /// Columns with their types — what makes one binary.
    fn typed(cols: &[(&str, &str)], rows: &[&[Value]]) -> ResultSet {
        let mut b = ResultBuilder::new(
            cols.iter()
                .map(|(n, t)| Column {
                    name: n.to_string(),
                    type_name: t.to_string(),
                    origin: None,
                })
                .collect(),
        );
        for r in rows {
            b.push_row(r);
        }
        b.finish()
    }

    /// A binary cell's text is `<n bytes>`, not its value: two blobs of one
    /// length read the same whatever their bytes. Such a column is said to be
    /// uncompared rather than reported "unchanged".
    #[test]
    fn a_binary_column_is_named_as_not_compared() {
        let cols = [
            ("id", "INT"),
            ("pw_hash", "VARBINARY(32)"),
            ("name", "TEXT"),
        ];
        let a = typed(&cols, &[&[i(1), s("<32 bytes>"), s("x")]]);
        let b = typed(&cols, &[&[i(1), s("<32 bytes>"), s("x")]]);
        let d = diff(&a, &b, &["id".into()]).unwrap();
        assert_eq!(d.uncompared, ["pw_hash"]);
        assert_eq!(d.shared, ["id", "name"], "the compared columns");
        assert!(
            d.summary().contains("pw_hash not compared"),
            "{}",
            d.summary()
        );
        // Whole-row identity leaves it out too, and says so.
        let whole = diff(&a, &b, &[]).unwrap();
        assert_eq!(whole.uncompared, ["pw_hash"]);
        // Binary on one side is enough: that side's text is a placeholder.
        let text_b = typed(
            &[("id", "INT"), ("pw_hash", "TEXT"), ("name", "TEXT")],
            &[&[i(1), s("<32 bytes>"), s("x")]],
        );
        assert_eq!(diff(&a, &text_b, &[]).unwrap().uncompared, ["pw_hash"]);
        // A plain result says nothing extra.
        assert!(diff(&a, &b, &["id".into()]).unwrap().uncompared.len() == 1);
        let plain = rs(&["id"], &[&[i(1)]]);
        assert!(diff(&plain, &plain, &[]).unwrap().uncompared.is_empty());
        assert!(
            !diff(&plain, &plain, &[])
                .unwrap()
                .summary()
                .contains("compared")
        );
    }

    /// Every `BINARY(16)` UUID reads `<16 bytes>`, so as a key it would match
    /// the first row and call every other a repeat: it is never suggested, and
    /// asked for, it is refused.
    #[test]
    fn a_binary_column_is_never_a_key() {
        let cols = [("id", "BINARY(16)"), ("code", "TEXT")];
        let a = typed(
            &cols,
            &[&[s("<16 bytes>"), s("a")], &[s("<16 bytes>"), s("b")]],
        );
        assert_eq!(suggested_key(&a, &a), ["code"]);
        assert_eq!(
            diff(&a, &a, &["id".into()]),
            Err(DiffError::BinaryKey("id".into()))
        );
        assert_eq!(key_columns(&a, &a), ["code"]);
        // SQLite marks a blob per value rather than by type.
        let mut blob = rs(&["id", "code"], &[&[s("<4 bytes>"), s("a")]]);
        blob.binary_columns = vec![0];
        assert_eq!(suggested_key(&blob, &blob), ["code"]);
    }

    /// Whole-row identity over many equal rows was quadratic — each match
    /// shifted its whole bucket down by one — and runs on the UI thread.
    #[test]
    fn whole_row_identity_over_many_equal_rows_is_linear() {
        // Large enough that the quadratic shift (≈ n²/2 moves) cannot hide
        // under the bound below, as 100k rows could behind a fast `memmove`.
        let n = 300_000;
        let rows: Vec<Vec<Value>> = (0..n).map(|_| vec![s("same")]).collect();
        let refs: Vec<&[Value]> = rows.iter().map(Vec::as_slice).collect();
        let a = rs(&["status"], &refs);
        let b = rs(&["status"], &refs[..n - 1]);
        let started = std::time::Instant::now();
        let d = diff(&a, &b, &[]).unwrap();
        let took = started.elapsed();
        assert_eq!(d.same, n - 1);
        assert_eq!(d.left_only, [n - 1], "the last of the left's copies");
        assert!(took.as_secs_f64() < 5.0, "{took:?}");
    }

    #[test]
    fn the_suggested_key_is_id_else_a_unique_column_else_the_row() {
        let a = rs(&["code", "ID"], &[&[s("a"), i(1)]]);
        assert_eq!(suggested_key(&a, &a), ["ID"]);
        let u = rs(&["n", "code"], &[&[i(1), s("a")], &[i(1), s("b")]]);
        assert_eq!(suggested_key(&u, &u), ["code"]);
        let none = rs(&["n"], &[&[i(1)], &[i(1)]]);
        assert!(suggested_key(&none, &none).is_empty());
    }

    /// **A row's key is the result's own text, not a copy of it.** A
    /// `String` per compared cell was most of what an open or a swap cost:
    /// at 200k × 20, release, 0.35 s keyed and 0.68 s whole-row before,
    /// 0.13 s and 0.32 s borrowed. A timing bound cannot tell the two apart
    /// in a loaded debug run (1.2 s against 0.6 s), so this pins the cause:
    /// the key points into the result.
    #[test]
    fn a_row_key_borrows_the_results_text() {
        let a = rs(&["id", "name"], &[&[i(1), s("a")]]);
        let k: Vec<Option<&str>> = row_key(&a, 0, &[0, 1]);
        for (c, text) in k.iter().enumerate() {
            let held = a.cell(0, c).unwrap().text();
            assert!(
                std::ptr::eq(text.unwrap().as_ptr(), held.as_ptr()),
                "column {c} was copied"
            );
        }
    }
}
