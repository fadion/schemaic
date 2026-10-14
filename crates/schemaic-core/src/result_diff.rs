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

use std::collections::HashMap;

use crate::model::{CellTag, ResultSet};

/// One row's key or whole-row identity: each cell's text, `None` for NULL.
type RowKey = Vec<Option<String>>;

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
}

impl ResultDiff {
    /// Nothing to report: the same rows with the same values.
    pub fn is_identical(&self) -> bool {
        self.left_only.is_empty() && self.right_only.is_empty() && self.changed.is_empty()
    }

    /// The headline: "3 added, 1 removed, 2 changed, 40 the same".
    pub fn summary(&self) -> String {
        if self.is_identical() {
            return format!(
                "No differences — {} {} the same",
                self.same,
                crate::text::plural(self.same, "row", "rows")
            );
        }
        let mut parts = Vec::new();
        for (n, word) in [
            (self.right_only.len(), "added"),
            (self.left_only.len(), "removed"),
            (self.changed.len(), "changed"),
        ] {
            if n > 0 {
                parts.push(format!("{n} {word}"));
            }
        }
        parts.push(format!("{} the same", self.same));
        parts.join(", ")
    }
}

/// Why two results cannot be compared by the key asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffError {
    /// A key column one side does not have.
    MissingKey(String),
}

impl std::fmt::Display for DiffError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiffError::MissingKey(c) => {
                write!(f, "both results need a column named {c} to match rows by")
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

/// A key to start from: a shared column named `id` (any case), else the
/// first shared column whose values are unique and never NULL on both sides,
/// else none — the whole row.
pub fn suggested_key(left: &ResultSet, right: &ResultSet) -> Vec<String> {
    let shared = shared_columns(left, right);
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
    let shared = shared_columns(left, right);
    let names =
        |rs: &ResultSet| -> Vec<String> { rs.columns.iter().map(|c| c.name.clone()).collect() };
    let only_left: Vec<String> = names(left)
        .into_iter()
        .filter(|n| !shared.contains(n))
        .collect();
    let only_right: Vec<String> = names(right)
        .into_iter()
        .filter(|n| !shared.contains(n))
        .collect();
    for k in key {
        if !shared.contains(k) {
            return Err(DiffError::MissingKey(k.clone()));
        }
    }
    let cols = |rs: &ResultSet, which: &[String]| -> Vec<usize> {
        which.iter().filter_map(|n| col(rs, n)).collect()
    };
    let mut out = ResultDiff {
        shared: shared.clone(),
        only_left,
        only_right,
        ..ResultDiff::default()
    };
    if key.is_empty() {
        // Whole-row identity, as a multiset.
        let (lc, rc) = (cols(left, &shared), cols(right, &shared));
        let mut counts: HashMap<RowKey, Vec<usize>> = HashMap::new();
        for r in 0..left.row_count() {
            counts.entry(row_key(left, r, &lc)).or_default().push(r);
        }
        for r in 0..right.row_count() {
            match counts.get_mut(&row_key(right, r, &rc)) {
                Some(rows) if !rows.is_empty() => {
                    rows.remove(0);
                    out.same += 1;
                }
                _ => out.right_only.push(r),
            }
        }
        let mut gone: Vec<usize> = counts.into_values().flatten().collect();
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

/// One row as a line of the diff: `name = value` for each of `rs`'s columns,
/// `NULL` for a null, each value cut at 40 characters.
pub fn row_line(rs: &ResultSet, row: usize) -> String {
    rs.columns
        .iter()
        .enumerate()
        .map(|(c, col)| format!("{} = {}", col.name, shown(cell(rs, row, c))))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// A changed row as a line: its key, then `column: before → after` for each
/// value that differs.
pub fn change_line(left: &ResultSet, right: &ResultSet, key: &[String], ch: &Changed) -> String {
    let mut parts: Vec<String> = key
        .iter()
        .filter_map(|k| {
            Some(format!(
                "{k} = {}",
                shown(cell(left, ch.left, col(left, k)?))
            ))
        })
        .collect();
    for c in &ch.columns {
        let (Some(lc), Some(rc)) = (col(left, c), col(right, c)) else {
            continue;
        };
        parts.push(format!(
            "{c}: {} → {}",
            shown(cell(left, ch.left, lc)),
            shown(cell(right, ch.right, rc))
        ));
    }
    parts.join(" · ")
}

fn shown(v: Option<String>) -> String {
    match v {
        None => "NULL".to_string(),
        Some(s) if s.chars().count() > 40 => format!("{}…", s.chars().take(39).collect::<String>()),
        Some(s) => s,
    }
}

fn col(rs: &ResultSet, name: &str) -> Option<usize> {
    rs.columns.iter().position(|c| c.name == name)
}

fn cell(rs: &ResultSet, row: usize, col: usize) -> Option<String> {
    let c = rs.cell(row, col)?;
    (c.tag != CellTag::Null).then(|| c.text().to_string())
}

fn row_key(rs: &ResultSet, row: usize, cols: &[usize]) -> RowKey {
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
        assert_eq!(d.summary(), "1 added, 1 removed, 1 changed, 1 the same");
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
        assert_eq!(d.summary(), "No differences — 1 row the same");
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
    fn rows_and_changes_read_as_lines() {
        let a = rs(&["id", "qty", "note"], &[&[i(3), i(7), Value::Null]]);
        let b = rs(&["id", "qty", "note"], &[&[i(3), i(9), s("x")]]);
        assert_eq!(row_line(&a, 0), "id = 3 · qty = 7 · note = NULL");
        let d = diff(&a, &b, &["id".into()]).unwrap();
        assert_eq!(
            change_line(&a, &b, &["id".into()], &d.changed[0]),
            "id = 3 · qty: 7 → 9 · note: NULL → x"
        );
        let long = rs(&["v"], &[&[s(&"z".repeat(60))]]);
        assert!(row_line(&long, 0).ends_with('…'));
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

    #[test]
    fn the_suggested_key_is_id_else_a_unique_column_else_the_row() {
        let a = rs(&["code", "ID"], &[&[s("a"), i(1)]]);
        assert_eq!(suggested_key(&a, &a), ["ID"]);
        let u = rs(&["n", "code"], &[&[i(1), s("a")], &[i(1), s("b")]]);
        assert_eq!(suggested_key(&u, &u), ["code"]);
        let none = rs(&["n"], &[&[i(1)], &[i(1)]]);
        assert!(suggested_key(&none, &none).is_empty());
    }
}
