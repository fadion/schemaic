//! Live-monitor change detection — pure over snapshots, no UI, no DB, no engine.
//!
//! The Live Monitor polls a bounded query on an interval and reports the row
//! changes since the previous poll. The polling/timer/DB live in the app; the
//! *change detection* is here so it's testable and engine-agnostic: give it two
//! [`Snapshot`]s (a prior capture and a fresh one) keyed by a row identity, and
//! [`diff_snapshots`] returns the inserts / updates / deletes between them,
//! updates carrying per-column `old → new` [`FieldChange`]s for display.
//!
//! Identity is a `Vec<String>` (the key columns' text, so composite keys work);
//! cell values are `Option<String>` (`None` = SQL `NULL`, kept distinct from the
//! empty string). Both are engine-neutral, so a future Postgres/SQLite fetch path
//! only has to produce a [`Snapshot`] — the diff is shared unchanged.

use std::collections::HashMap;

use crate::model::ResultSet;

/// Rows per poll. The monitor is bounded by construction — it never polls an
/// unbounded table — and this is that bound. Here rather than in the app because
/// the modal has to *name* it: past this many rows the monitor watches a page,
/// and the status line says so.
pub const ROW_CAP: usize = 1000;

/// One captured row: its identity `key` (the key columns' values, stringified,
/// in key order) and every column's value (`None` = NULL) in result-column order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotRow {
    pub key: Vec<String>,
    pub cells: Vec<Option<String>>,
}

/// A point-in-time capture of the monitored rows, in fetch order. Diffed against
/// the next capture to find what changed. Order carries meaning only when
/// [`Snapshot::ordered_window_full`] is set — otherwise the diff matches by `key`
/// and the sequence is just for display.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub rows: Vec<SnapshotRow>,
    /// These rows are the **first N of a deterministic total order**, and the
    /// table has more beyond them — i.e. an `ORDER BY key LIMIT n` that came back
    /// full.
    ///
    /// A bounded window has to be a *stable* one, and knowing it was full is what
    /// lets [`diff_snapshots`] tell a real change from the window sliding: delete
    /// a row inside the window and the next row is promoted into it, which reads
    /// as an insert of a row that existed all along. Only set this when the poll
    /// really was ordered — with an arbitrary order the tail suppression below
    /// would swallow real changes.
    pub ordered_window_full: bool,
}

impl Snapshot {
    /// Capture `rs` as a snapshot, taking row identity from `key_cols` (indices
    /// into the result columns — typically an [`crate::edit::EditModel`] table's
    /// `key_cols`). A key cell renders via `display()` (NULL → `"NULL"`; keys are
    /// NOT NULL in practice); other cells keep NULL distinct from `""`.
    pub fn from_result(rs: &ResultSet, key_cols: &[usize]) -> Self {
        let ncols = rs.col_count();
        let mut rows = Vec::with_capacity(rs.row_count());
        for r in 0..rs.row_count() {
            let cells = (0..ncols)
                .map(|c| match rs.cell(r, c) {
                    Some(cr) if !cr.is_null() => Some(cr.text().to_string()),
                    _ => None,
                })
                .collect();
            let key = key_cols
                .iter()
                .map(|&kc| {
                    rs.cell(r, kc)
                        .map(|cr| cr.display().to_string())
                        .unwrap_or_default()
                })
                .collect();
            rows.push(SnapshotRow { key, cells });
        }
        Snapshot {
            rows,
            ordered_window_full: false,
        }
    }

    /// Mark this capture as the full, ordered first page of a larger table — see
    /// [`Snapshot::ordered_window_full`]. The caller sets it only when it ordered
    /// the query *and* got back exactly the limit.
    #[must_use]
    pub fn ordered_window(mut self, full: bool) -> Self {
        self.ordered_window_full = full;
        self
    }
}

/// What happened to a row between two snapshots.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChangeKind {
    Insert,
    Update,
    Delete,
}

/// One column's value change within an [`ChangeKind::Update`] row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldChange {
    /// Result-column index (map to a name via the result's `columns`).
    pub col: usize,
    pub old: Option<String>,
    pub new: Option<String>,
}

/// A single detected change, in the shape the monitor log renders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowChange {
    pub kind: ChangeKind,
    /// The affected row's identity (key-column values).
    pub key: Vec<String>,
    /// For [`ChangeKind::Update`]: the columns whose value changed. Empty for
    /// insert/delete.
    pub fields: Vec<FieldChange>,
    /// The row's cells — the *new* values for insert/update, the *last-seen*
    /// values for delete — so the log can show the whole row, not just the key.
    pub cells: Vec<Option<String>>,
}

/// The row changes from `old` to `new`, matched by key: keys only in `new` are
/// inserts, keys only in `old` are deletes, keys in both with any differing cell
/// are updates (carrying the per-column diffs). Emission order is stable:
/// inserts/updates in `new`'s row order first, then deletes in `old`'s row order.
///
/// Callers skip the very first poll — diffing an empty `old` against the baseline
/// would report every row as an insert. Duplicate keys within a snapshot are not
/// expected (the monitor requires a real key); if present, the last row wins.
pub fn diff_snapshots(old: &Snapshot, new: &Snapshot) -> Vec<RowChange> {
    // key → index into old.rows (last wins on a duplicate key).
    let old_idx: HashMap<&[String], usize> = old
        .rows
        .iter()
        .enumerate()
        .map(|(i, r)| (r.key.as_slice(), i))
        .collect();
    let mut seen = vec![false; old.rows.len()];
    let mut changes = Vec::new();

    // When both captures are full ordered windows, the tail is where the window
    // *slid* rather than where the data changed: deleting a row inside the window
    // promotes the next row in, and inserting one demotes the last row out.
    // Neither is a change to the table, and reporting them was the whole defect —
    // a table over the limit logged insert/delete pairs that never happened.
    //
    // Both windows are the same ordered prefix, so everything up to the last row
    // they share is directly comparable and everything after it is not. That
    // boundary is positional, which matters: the keys are text, and comparing
    // them here would order `"1000"` before `"500"` where the server did not.
    let bounded = old.ordered_window_full && new.ordered_window_full;
    let (old_cut, new_cut) = if bounded {
        (
            last_common(&old.rows, &new.rows),
            last_common(&new.rows, &old.rows),
        )
    } else {
        (old.rows.len(), new.rows.len())
    };

    // Inserts + updates, in the new snapshot's order.
    for (ni, nr) in new.rows.iter().enumerate() {
        match old_idx.get(nr.key.as_slice()) {
            None if ni >= new_cut => {} // promoted into the window, not inserted
            None => changes.push(RowChange {
                kind: ChangeKind::Insert,
                key: nr.key.clone(),
                fields: Vec::new(),
                cells: nr.cells.clone(),
            }),
            Some(&oi) => {
                seen[oi] = true;
                let or = &old.rows[oi];
                if or.cells != nr.cells {
                    changes.push(RowChange {
                        kind: ChangeKind::Update,
                        key: nr.key.clone(),
                        fields: field_changes(&or.cells, &nr.cells),
                        cells: nr.cells.clone(),
                    });
                }
            }
        }
    }

    // Deletes: old rows never matched, in the old snapshot's order.
    for (oi, or) in old.rows.iter().enumerate() {
        if !seen[oi] && oi < old_cut {
            changes.push(RowChange {
                kind: ChangeKind::Delete,
                key: or.key.clone(),
                fields: Vec::new(),
                cells: or.cells.clone(),
            });
        }
    }

    changes
}

/// One past the index of the last row of `rows` whose key also appears in
/// `other` — the point beyond which two ordered windows stop being comparable.
///
/// `rows.len()` when the last row is shared (nothing to suppress); `0` when they
/// share nothing at all, which is a window that moved entirely and where no
/// insert or delete can be attributed.
fn last_common(rows: &[SnapshotRow], other: &[SnapshotRow]) -> usize {
    let keys: std::collections::HashSet<&[String]> =
        other.iter().map(|r| r.key.as_slice()).collect();
    rows.iter()
        .rposition(|r| keys.contains(r.key.as_slice()))
        .map_or(0, |i| i + 1)
}

/// Per-column `old → new` differences between two rows' cells. A missing index
/// (rows of unequal width — not expected for one query) counts as NULL.
fn field_changes(old: &[Option<String>], new: &[Option<String>]) -> Vec<FieldChange> {
    let n = old.len().max(new.len());
    (0..n)
        .filter_map(|i| {
            let o = old.get(i).cloned().flatten();
            let nw = new.get(i).cloned().flatten();
            (o != nw).then_some(FieldChange {
                col: i,
                old: o,
                new: nw,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Column, Value};

    fn row(key: &str, cells: &[Option<&str>]) -> SnapshotRow {
        SnapshotRow {
            key: vec![key.to_string()],
            cells: cells.iter().map(|c| c.map(|s| s.to_string())).collect(),
        }
    }

    fn snap(rows: Vec<SnapshotRow>) -> Snapshot {
        Snapshot {
            rows,
            ordered_window_full: false,
        }
    }

    /// The same, but flagged as the first N rows of an ordered table with more
    /// beyond — what a full `ORDER BY key LIMIT n` poll produces.
    fn window(rows: Vec<SnapshotRow>) -> Snapshot {
        Snapshot {
            rows,
            ordered_window_full: true,
        }
    }

    // ── a bounded window is not the whole table ──────────────────────────────

    #[test]
    fn a_row_promoted_into_the_window_is_not_an_insert() {
        // Case A from the finding: a 1,001-row table watched with LIMIT 3.
        // Deleting id 2 lets id 4 slide into the window. The delete is real; the
        // "insert" is a row that existed throughout.
        let old = window(vec![
            row("1", &[Some("a")]),
            row("2", &[Some("b")]),
            row("3", &[Some("c")]),
        ]);
        let new = window(vec![
            row("1", &[Some("a")]),
            row("3", &[Some("c")]),
            row("4", &[Some("d")]),
        ]);
        let out = diff_snapshots(&old, &new);
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].kind, ChangeKind::Delete);
        assert_eq!(out[0].key, vec!["2"]);
    }

    #[test]
    fn a_row_pushed_out_of_the_window_is_not_a_delete() {
        // The mirror: a real insert inside the window demotes the last row.
        let old = window(vec![
            row("1", &[Some("a")]),
            row("3", &[Some("c")]),
            row("4", &[Some("d")]),
        ]);
        let new = window(vec![
            row("1", &[Some("a")]),
            row("2", &[Some("b")]),
            row("3", &[Some("c")]),
        ]);
        let out = diff_snapshots(&old, &new);
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].kind, ChangeKind::Insert);
        assert_eq!(out[0].key, vec!["2"]);
    }

    #[test]
    fn an_update_inside_a_full_window_is_still_reported() {
        // Case B is fixed by ordering the query, but the update must survive the
        // tail suppression: it is matched by key on both sides.
        let old = window(vec![
            row("1", &[Some("a")]),
            row("2", &[Some("b")]),
            row("3", &[Some("c")]),
        ]);
        let new = window(vec![
            row("1", &[Some("a")]),
            row("2", &[Some("B!")]),
            row("3", &[Some("c")]),
        ]);
        let out = diff_snapshots(&old, &new);
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].kind, ChangeKind::Update);
        assert_eq!(out[0].key, vec!["2"]);
    }

    #[test]
    fn a_window_that_is_not_full_reports_everything() {
        // The control, and the case the sample databases hit: below the limit the
        // window *is* the table, so nothing may be suppressed.
        let old = snap(vec![row("1", &[Some("a")]), row("2", &[Some("b")])]);
        let new = snap(vec![row("1", &[Some("a")]), row("3", &[Some("c")])]);
        let out = diff_snapshots(&old, &new);
        assert_eq!(out.len(), 2, "{out:?}");
        assert!(
            out.iter()
                .any(|c| c.kind == ChangeKind::Insert && c.key == vec!["3"])
        );
        assert!(
            out.iter()
                .any(|c| c.kind == ChangeKind::Delete && c.key == vec!["2"])
        );
    }

    #[test]
    fn suppression_needs_both_snapshots_to_be_full_windows() {
        // A table that fell below the limit between polls: the tail of the *old*
        // window really did go, so those deletes are real.
        let old = window(vec![
            row("1", &[Some("a")]),
            row("2", &[Some("b")]),
            row("3", &[Some("c")]),
        ]);
        let new = snap(vec![row("1", &[Some("a")])]);
        let out = diff_snapshots(&old, &new);
        assert_eq!(out.len(), 2, "{out:?}");
        assert!(out.iter().all(|c| c.kind == ChangeKind::Delete));
    }

    #[test]
    fn a_change_before_the_last_common_row_is_never_suppressed() {
        // Suppression is confined to the tail: an insert and a delete in the
        // middle of a full window are both real and both reported.
        let old = window(vec![
            row("1", &[Some("a")]),
            row("2", &[Some("b")]),
            row("5", &[Some("e")]),
        ]);
        let new = window(vec![
            row("1", &[Some("a")]),
            row("3", &[Some("c")]),
            row("5", &[Some("e")]),
        ]);
        let out = diff_snapshots(&old, &new);
        assert_eq!(out.len(), 2, "{out:?}");
        assert!(
            out.iter()
                .any(|c| c.kind == ChangeKind::Insert && c.key == vec!["3"])
        );
        assert!(
            out.iter()
                .any(|c| c.kind == ChangeKind::Delete && c.key == vec!["2"])
        );
    }

    #[test]
    fn empty_to_baseline_is_all_inserts() {
        // The caller skips this, but the function must still be well-defined: an
        // empty prior snapshot means every row reads as new.
        let new = snap(vec![row("1", &[Some("a")]), row("2", &[Some("b")])]);
        let out = diff_snapshots(&Snapshot::default(), &new);
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|c| c.kind == ChangeKind::Insert));
        assert_eq!(out[0].key, vec!["1"]);
        assert_eq!(out[1].key, vec!["2"]);
    }

    #[test]
    fn no_changes_yields_nothing() {
        let a = snap(vec![row("1", &[Some("a")]), row("2", &[Some("b")])]);
        let b = a.clone();
        assert!(diff_snapshots(&a, &b).is_empty());
    }

    #[test]
    fn detects_a_pure_insert() {
        let old = snap(vec![row("1", &[Some("a")])]);
        let new = snap(vec![row("1", &[Some("a")]), row("2", &[Some("b")])]);
        let out = diff_snapshots(&old, &new);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, ChangeKind::Insert);
        assert_eq!(out[0].key, vec!["2"]);
        assert_eq!(out[0].cells, vec![Some("b".to_string())]);
        assert!(out[0].fields.is_empty());
    }

    #[test]
    fn detects_a_pure_delete() {
        let old = snap(vec![row("1", &[Some("a")]), row("2", &[Some("b")])]);
        let new = snap(vec![row("1", &[Some("a")])]);
        let out = diff_snapshots(&old, &new);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, ChangeKind::Delete);
        assert_eq!(out[0].key, vec!["2"]);
        // Delete carries the last-seen cells, not the key alone.
        assert_eq!(out[0].cells, vec![Some("b".to_string())]);
    }

    #[test]
    fn detects_an_update_with_field_level_diff() {
        let old = snap(vec![row("1", &[Some("a"), Some("x"), Some("keep")])]);
        let new = snap(vec![row("1", &[Some("A"), Some("x"), Some("keep")])]);
        let out = diff_snapshots(&old, &new);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, ChangeKind::Update);
        // Only column 0 changed.
        assert_eq!(
            out[0].fields,
            vec![FieldChange {
                col: 0,
                old: Some("a".to_string()),
                new: Some("A".to_string()),
            }]
        );
        assert_eq!(
            out[0].cells,
            vec![
                Some("A".to_string()),
                Some("x".to_string()),
                Some("keep".to_string())
            ]
        );
    }

    #[test]
    fn null_and_empty_string_are_distinct_changes() {
        // NULL → "" is a real change; "" → "" is not.
        let old = snap(vec![row("1", &[None, Some("")])]);
        let new = snap(vec![row("1", &[Some(""), Some("")])]);
        let out = diff_snapshots(&old, &new);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, ChangeKind::Update);
        assert_eq!(
            out[0].fields,
            vec![FieldChange {
                col: 0,
                old: None,
                new: Some(String::new()),
            }]
        );
    }

    #[test]
    fn mixed_insert_update_delete_ordering() {
        // 1 unchanged, 2 updated, 3 deleted, 4 inserted.
        let old = snap(vec![
            row("1", &[Some("a")]),
            row("2", &[Some("b")]),
            row("3", &[Some("c")]),
        ]);
        let new = snap(vec![
            row("1", &[Some("a")]),
            row("2", &[Some("B")]),
            row("4", &[Some("d")]),
        ]);
        let out = diff_snapshots(&old, &new);
        // Inserts/updates in new-order first (update of 2, insert of 4), then
        // deletes in old-order (3).
        let kinds: Vec<_> = out.iter().map(|c| (c.kind, c.key.clone())).collect();
        assert_eq!(
            kinds,
            vec![
                (ChangeKind::Update, vec!["2".to_string()]),
                (ChangeKind::Insert, vec!["4".to_string()]),
                (ChangeKind::Delete, vec!["3".to_string()]),
            ]
        );
    }

    #[test]
    fn empty_new_deletes_everything() {
        let old = snap(vec![row("1", &[Some("a")]), row("2", &[Some("b")])]);
        let out = diff_snapshots(&old, &Snapshot::default());
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|c| c.kind == ChangeKind::Delete));
    }

    #[test]
    fn composite_key_distinguishes_rows() {
        let compose = |k1: &str, k2: &str, v: &str| SnapshotRow {
            key: vec![k1.to_string(), k2.to_string()],
            cells: vec![Some(v.to_string())],
        };
        let old = snap(vec![compose("a", "1", "x"), compose("a", "2", "y")]);
        // Same first key part, different second → a distinct row updated.
        let new = snap(vec![compose("a", "1", "x"), compose("a", "2", "Y")]);
        let out = diff_snapshots(&old, &new);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, ChangeKind::Update);
        assert_eq!(out[0].key, vec!["a".to_string(), "2".to_string()]);
    }

    #[test]
    fn from_result_builds_keys_and_keeps_null_distinct() {
        let columns = vec![
            Column {
                name: "id".into(),
                type_name: "INT".into(),
                origin: None,
            },
            Column {
                name: "note".into(),
                type_name: "VARCHAR".into(),
                origin: None,
            },
        ];
        let rs = ResultSet::from_rows(
            columns,
            vec![
                vec![Value::Int(1), Value::Str("hi".into())],
                vec![Value::Int(2), Value::Null],
            ],
        );
        let snap = Snapshot::from_result(&rs, &[0]);
        assert_eq!(snap.rows.len(), 2);
        assert_eq!(snap.rows[0].key, vec!["1".to_string()]);
        assert_eq!(
            snap.rows[0].cells,
            vec![Some("1".to_string()), Some("hi".to_string())]
        );
        // NULL becomes `None`, not `Some("NULL")`, so it stays distinct from text.
        assert_eq!(snap.rows[1].cells, vec![Some("2".to_string()), None]);
    }
}
