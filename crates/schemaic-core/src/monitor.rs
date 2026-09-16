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

use crate::model::{Column, ResultSet, Value};

/// Rows per poll. The monitor is bounded by construction — it never polls an
/// unbounded table — and this is that bound. Here rather than in the app because
/// the modal has to *name* it: past this many rows the monitor watches a page,
/// and the status line says so.
pub const ROW_CAP: usize = 1000;

/// Changes kept in the log. Past this the oldest drop off the top, which the
/// modal — like [`ROW_CAP`] — has to be able to *name*: once the log can be
/// exported, a silently truncated one is a record that looks complete and isn't.
pub const LOG_CAP: usize = 1000;

/// Trim `log` to [`LOG_CAP`], oldest first, and say how many entries went.
///
/// **One function for one rule, because the two halves of it lived in different
/// crates and disagreed.** The app trimmed on `len > LOG_CAP` and the modal
/// printed *"the oldest are dropping"* on `len >= LOG_CAP` — so a log sitting at
/// exactly the cap with nothing dropped carried a caveat that was false, on a
/// record whose whole value is that it can be trusted. The count is the honest
/// signal: it is only ever non-zero when something really has been thrown away.
pub fn trim_log<T>(log: &mut Vec<T>) -> usize {
    let over = log.len().saturating_sub(LOG_CAP);
    if over > 0 {
        log.drain(0..over);
    }
    over
}

/// Does throwing this log away need asking first?
///
/// **The log is the only copy.** A `DELETE` it recorded holds values the database
/// no longer has and no snapshot keeps — the baseline is the *current* page — and
/// a poll never re-reports a change it has already reported. So a discard is
/// irreversible in the way the confirm modal exists for, and it is reached by a
/// trash icon sitting one glyph from Export.
///
/// Two things make it *not* worth asking: an empty log, where there is nothing to
/// lose, and one already written to a file, where there is a second copy. Both
/// are the ordinary case, which is why the question isn't simply "is it
/// destructive".
pub fn discard_needs_asking(log_len: usize, exported: bool) -> bool {
    log_len > 0 && !exported
}

/// Has the watched table's shape moved out from under the baseline?
///
/// **A snapshot is positional, all the way down.** [`Snapshot::from_result`]
/// builds each row's `cells` by column index, [`diff_snapshots`] compares two
/// `cells` vectors, [`field_changes`] walks them by index, and the modal renders
/// each `FieldChange { col }` against the column-name list captured on the
/// *baseline* poll. So an `ALTER TABLE orders ADD COLUMN note TEXT AFTER id` in
/// another session, while the monitor is open, shifts every cell from index 1 on
/// by one place: every row in the window is logged as an UPDATE, and each field
/// is named from the pre-`ALTER` order — the log claims `name: 'Ada' →
/// '2024-01-03'` for a column that never changed, in a record the modal exports
/// and treats as the only copy.
///
/// The mirror case is quieter and worse: a `DROP COLUMN` before a key column
/// leaves the resolved key indices pointing past the new width,
/// `from_result`'s fallback gives *every* row the empty key, the map collapses
/// to one entry, and the diff reports the whole window deleted.
///
/// Compared by **name and position**, not as a set: two columns swapped is the
/// same shift, and a rename at the same position is a different column as far as
/// a positional diff is concerned. Case-sensitively, because a server that
/// changes a column's case has changed the text the log will print.
///
/// The answer is to restart the baseline, not to diff across it — nothing here
/// can say what happened to a row while the table's shape was changing.
pub fn baseline_is_stale(old_cols: &[String], new_cols: &[String]) -> bool {
    old_cols != new_cols
}

/// What one poll of the monitor's timer should do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TickAction {
    /// Let the loop end. Nothing re-arms after this, so the monitor is over.
    Stop,
    /// Skip this poll and arm the next one.
    Reschedule,
    /// Fetch the page and diff it against the baseline.
    Fetch,
}

/// The decision one timer tick makes, given what the modal is doing.
///
/// **Pause is not Stop, and the difference is one arm.** The three lines this
/// replaces were the commit's headline behaviour and sat inside a
/// floem-scheduled function, where no test could reach them: swap the two
/// statements and a paused monitor never re-arms, so Resume does nothing and the
/// only way back is to close and reopen the modal — with the log gone. Nothing
/// would have failed.
///
/// What the arms mean, in the order they are asked:
///
/// - **`open == false`** — the modal is closed (or its signal is disposed, which
///   the caller reads as the same thing). Nothing to poll for and nothing to
///   re-arm: the loop ends here, and this is the *only* way it ends.
/// - **`superseded`** — a newer generation is running, because the user
///   re-targeted the monitor. This timer belongs to the old target, so it must
///   not fetch *and* must not re-arm, or two loops poll forever.
/// - **`paused`** — skip the fetch and arm the next tick anyway. Resuming is
///   then free (the loop was never broken) and the baseline deliberately ages,
///   so the first poll after a resume reports the **net** change across the
///   pause rather than nothing at all.
pub fn tick_action(open: bool, superseded: bool, paused: bool) -> TickAction {
    if !open || superseded {
        TickAction::Stop
    } else if paused {
        TickAction::Reschedule
    } else {
        TickAction::Fetch
    }
}

/// One captured row: its identity `key` (the key columns' values, stringified,
/// in key order) and every column's value (`None` = NULL) in result-column order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotRow {
    pub key: Vec<String>,
    pub cells: Vec<Option<String>>,
}

/// A point-in-time capture of the monitored rows, in fetch order. Diffed against
/// the next capture to find what changed. Order carries meaning only when
/// [`Snapshot::ordered`] is set — otherwise the diff matches by `key` and the
/// sequence is just for display. [`Snapshot::window_full`] is the second half:
/// see `diff_snapshots`, which asks the two independently.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub rows: Vec<SnapshotRow>,
    /// The poll asked the server for a **deterministic total order**
    /// (`ORDER BY key`), so position means something and two windows can be
    /// compared as prefixes of one sequence.
    pub ordered: bool,
    /// The fetch came back at its row limit, so the table has more rows than
    /// these.
    ///
    /// **Two facts, not one.** They were a single `ordered_window_full`, and
    /// conflating them is what made the monitor's log false. A bounded window
    /// has to be a *stable* one: delete a row inside it and the next row is
    /// promoted in, which reads as an insert of a row that existed all along,
    /// so [`diff_snapshots`] suppresses the tail — but only for a window it
    /// knows is an ordered prefix. A window that is **full but unordered** is
    /// neither of the two states the old flag could express, and it landed on
    /// the "report everything" branch: measured on MariaDB 10.11, deleting one
    /// row of a 1,200-row table with no primary key logged a phantom INSERT
    /// beside the real DELETE, and on PostgreSQL 16.15 an `UPDATE` moved the
    /// heap tuple out of the window entirely and was logged as a **DELETE of
    /// the updated row**, with the update never reported at all. The log is
    /// exportable and `discard_needs_asking` exists because it is treated as a
    /// record someone keeps.
    pub window_full: bool,
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
            ordered: false,
            window_full: false,
        }
    }

    /// Record how this capture was taken: whether the query carried an
    /// `ORDER BY` over the row-identity key, and whether it came back at its
    /// limit. See [`Snapshot::window_full`] for why both are needed.
    #[must_use]
    pub fn window(mut self, ordered: bool, full: bool) -> Self {
        self.ordered = ordered;
        self.window_full = full;
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
    // **Each cut asks about the *other* window**, because that is the one whose
    // silence has to be interpreted. A key missing from the new snapshot is a
    // real delete only if the new snapshot would have shown it; a key new to
    // this snapshot is a real insert only if the old one would have shown it.
    //
    // Three states per window, and the old code could only express two. A
    // window below the limit **is** the table, so absence from it is real. A
    // window at the limit *and ordered* is a stable prefix, so absence before
    // the last row the two share is real and everything after it is the window
    // sliding. A window at the limit and **unordered** is neither: which rows
    // came back is arbitrary, and absence from it means nothing at all.
    //
    // That third state used to land on "report everything", which is what put
    // changes that never happened into an exportable log — measured, a phantom
    // INSERT beside a real DELETE on MariaDB 10.11, and on PostgreSQL 16.15 an
    // `UPDATE` that moved the heap tuple out of the window logged as a
    // **DELETE of the row it updated**, with the update never reported. Rows
    // both windows hold are still diffed either way, so a real change to a
    // visible row is still seen.
    let ordered = old.ordered && new.ordered;
    let old_cut = if !new.window_full {
        old.rows.len()
    } else if ordered {
        last_common(&old.rows, &new.rows)
    } else {
        0
    };
    let new_cut = if !old.window_full {
        new.rows.len()
    } else if ordered {
        last_common(&new.rows, &old.rows)
    } else {
        0
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

/// One entry in the Live Monitor's change log: a detected [`RowChange`] plus the
/// elapsed-since-start timestamp (`M:SS`) at which the monitor *observed* it.
///
/// The timestamp is an observation time, not an event time — a change is stamped
/// when a poll saw it, so with a 10-second interval it can be up to ten seconds
/// late, and several changes to one row between two polls collapse into the one
/// the diff can see.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonitorEntry {
    pub at: String,
    pub change: RowChange,
    /// A number that only ever goes up, assigned by [`append_changes`] — the log's
    /// identity for this entry.
    ///
    /// **It exists so the rendered list can be keyed on the entry rather than on
    /// its position.** The log is a sliding window: at [`LOG_CAP`] each appended
    /// entry drops one off the front, so positions `0..999` describe a *different*
    /// thousand changes after every poll while the index set stays identical —
    /// and floem's keyed diff, which reuses a view whose key didn't change, would
    /// render the first thousand changes for ever while the log and its export went
    /// on sliding underneath.
    pub seq: u64,
    /// The watched table's column list **as it was when this entry was
    /// recorded**.
    ///
    /// **Everything about a change is positional, and the position's meaning can
    /// move under the log.** A `FieldChange { col: 1 }` means "the second
    /// column"; which column that *is* comes from a list held beside the log,
    /// not in it. That was sound while the list could only be written once per
    /// monitor session — until the baseline restart that answers an `ALTER`
    /// mid-session re-opened the branch that writes it. From that moment the log
    /// held entries recorded against the old list and the export rendered all of
    /// them against the new one: after
    /// `ALTER TABLE orders ADD COLUMN note TEXT AFTER id`, every earlier UPDATE
    /// was exported under `note` instead of `name`, and every earlier
    /// INSERT/DELETE row — "the only remaining record of a row the database no
    /// longer has", per [`log_result_set`] — had each value one column left of
    /// its heading. Silently: the restart notice goes to the poll's error slot,
    /// which the next poll clears.
    ///
    /// An `Arc`, because every entry of a poll shares one list and the log runs
    /// to [`LOG_CAP`].
    pub cols: std::sync::Arc<Vec<String>>,
}

/// Stamp `changes` with `at`, append them to `log`, and trim it to [`LOG_CAP`] —
/// returning how many entries the trim dropped.
///
/// One function because the three parts are one rule: an entry's sequence number
/// has to be assigned where it is appended (it is `last + 1`, and the log is never
/// reordered), and the cap has to be applied in the same breath or the caveat the
/// status line prints could disagree with what the log holds.
pub fn append_changes(
    log: &mut Vec<MonitorEntry>,
    at: &str,
    changes: Vec<RowChange>,
    cols: &std::sync::Arc<Vec<String>>,
) -> usize {
    let first = log.last().map(|e| e.seq + 1).unwrap_or(0);
    for (seq, change) in (first..).zip(changes) {
        log.push(MonitorEntry {
            at: at.to_string(),
            change,
            seq,
            // Taken here rather than read at render time, for the reason
            // `MonitorEntry::cols` gives: the list beside the log can be
            // rewritten mid-session and the entries cannot follow it.
            cols: cols.clone(),
        });
    }
    trim_log(log)
}

/// How many of a change's columns the modal's **Data** column renders before it
/// stops and counts the rest.
///
/// **A bound on views, not on the log.** The log holds every column and
/// [`log_result_set`] exports every column; this is only what one row of the
/// change table builds. An `Insert`/`Delete` row renders one span per column
/// *name* plus one per *value* and an `Update` four per changed field, uncapped
/// in the table's width — so at `LOG_CAP` entries a 60-column table put ~123,000
/// views in a mounted list showing about twenty of them, paid by every layout and
/// paint pass for as long as the modal is open (23,000 at 10 columns, 63,000 at
/// 30 — counted, not estimated).
///
/// Twelve because the Data column is one non-wrapping line that is already
/// scrolled horizontally to read: past a dozen `col=value` pairs nobody is
/// reading it as a line, they are reading the export.
pub const DATA_COLS_SHOWN: usize = 12;

/// What the Data column says in place of the columns it did not render, or
/// `None` when it rendered them all.
///
/// The disclosure is the tested half: a line that silently stops at twelve of
/// sixty is the defect this would otherwise introduce, and the caller has the
/// whole export one button away.
pub fn data_overflow_label(total: usize) -> Option<String> {
    let hidden = total.checked_sub(DATA_COLS_SHOWN).filter(|n| *n > 0)?;
    Some(format!(", +{hidden} more"))
}

/// Separates a changed column's old and new value in an exported cell
/// (`old → new`), matching what the modal renders as two coloured spans.
const TRANSITION: &str = " → ";

/// Fallback name for a change whose column index the watched result never named
/// — see [`log_result_set`], which widens rather than truncates.
fn unnamed_column(i: usize) -> String {
    format!("column_{}", i + 1)
}

/// The export formats offered for a change log: everything the results grid
/// offers **except SQL**.
///
/// SQL is excluded because it renders `INSERT INTO <table> …`, and a change log
/// has no such table. Its rows are not rows of the watched table — they are
/// observations *about* it, a third of them deletions — so every statement that
/// export produced would be a plausible-looking lie about where the data goes.
pub const LOG_FORMATS: [crate::export::ExportFormat; 5] = [
    crate::export::ExportFormat::Json,
    crate::export::ExportFormat::Csv,
    crate::export::ExportFormat::Markdown,
    crate::export::ExportFormat::Html,
    crate::export::ExportFormat::Xlsx,
];

/// Project a change log into a [`ResultSet`] so it exports through the ordinary
/// renderers in [`crate::export`] rather than a second set written for it.
///
/// The shape is one row per logged change: `Time`, `Action`, `Key`, then one
/// column per column of the watched table (`cols`, as captured by the baseline
/// poll). `Key` rather than the modal's space-constrained `ID`, because the key
/// can be composite and the file has to describe itself once it's away from the
/// modal; the values are the key columns joined with `, `, exactly as rendered.
///
/// The per-column cells carry the whole row, which is more than the modal shows:
///
/// - **Insert** — the new values.
/// - **Delete** — the last-seen values, which is the only remaining record of a
///   row the database no longer has.
/// - **Update** — `old → new` in the columns that changed, and the (unchanged)
///   value everywhere else. That mixes a transition and a value in one column,
///   deliberately: the alternative is either dropping the context the unchanged
///   columns give, or two rows per update, and this is the shape someone reads.
///
/// NULL is a real NULL cell wherever a value stands alone, so each format renders
/// it its own way; inside an `old → new` transition it can only be the literal
/// text `NULL`, since the transition is one cell.
///
/// **Width comes from the data, not from `cols`.** If a change carries more cells
/// than the baseline named columns — which shouldn't happen, one query produced
/// both — the extra columns are named [`unnamed_column`] rather than dropped. An
/// export is the record someone keeps; silently narrowing it is the one failure
/// mode that can't be noticed later.
pub fn log_result_set(entries: &[MonitorEntry], cols: &[String]) -> ResultSet {
    // **Every entry is projected under the names it was recorded with**, not
    // under whichever list happens to be current. A baseline restart rewrites
    // that list mid-session, and the log cannot follow it: rendering an entry's
    // index `i` as `cols[i]` put every pre-`ALTER` change under the wrong
    // heading and shifted every pre-`ALTER` row one column off it, in the one
    // artefact that is the only remaining record of a row the database no longer
    // has. So the output columns are the *union* of the names seen, in the order
    // they were first seen, and each entry's value lands under its own name —
    // which needs no gap marker, because nothing is mis-stated to begin with.
    //
    // `cols` still leads, so an unchanged session exports exactly as before: its
    // entries all carry that same list, and the union is it.
    // An entry with no list of its own is one recorded before entries carried
    // them — a restored log, or a test fixture. `cols` is its answer, which is
    // what the single-list form did for every entry.
    fn names_of<'a>(e: &'a MonitorEntry, cols: &'a [String]) -> &'a [String] {
        if e.cols.is_empty() { cols } else { &e.cols }
    }
    let width_of = |e: &MonitorEntry| {
        e.change
            .fields
            .iter()
            .map(|f| f.col + 1)
            .chain(std::iter::once(e.change.cells.len()))
            .chain(std::iter::once(names_of(e, cols).len()))
            .max()
            .unwrap_or(0)
    };
    // The name an entry gives its index `i`, with the same fallbacks the single
    // list had.
    let name_at = |names: &[String], i: usize| {
        names
            .get(i)
            .filter(|n| !n.is_empty())
            .cloned()
            .unwrap_or_else(|| unnamed_column(i))
    };

    let width = entries
        .iter()
        .map(width_of)
        .max()
        .unwrap_or(0)
        .max(cols.len());
    let mut headings: Vec<String> = Vec::new();
    let push = |h: &mut Vec<String>, n: String| {
        if !h.contains(&n) {
            h.push(n);
        }
    };
    for i in 0..cols.len() {
        push(&mut headings, name_at(cols, i));
    }
    for e in entries {
        for i in 0..width_of(e) {
            push(&mut headings, name_at(names_of(e, cols), i));
        }
    }
    // An index nobody named — a log wider than every list it carries, which the
    // single-list form covered with a placeholder.
    for i in headings.len()..width {
        push(&mut headings, unnamed_column(i));
    }

    let text_col = |name: String| Column {
        name,
        type_name: "TEXT".to_string(),
        origin: None,
    };
    let columns: Vec<Column> = ["Time", "Action", "Key"]
        .into_iter()
        .map(|n| text_col(n.to_string()))
        .chain(headings.iter().cloned().map(text_col))
        .collect();

    let rows = entries
        .iter()
        .map(|e| {
            let mut row = Vec::with_capacity(3 + headings.len());
            row.push(Value::Str(e.at.clone()));
            row.push(Value::Str(action_label(e.change.kind).to_string()));
            row.push(Value::Str(e.change.key.join(", ")));
            // This entry's index for each heading, or nothing where its own
            // column list has no such name.
            let w = width_of(e);
            let mine: Vec<Option<usize>> = headings
                .iter()
                .map(|h| (0..w).find(|&i| name_at(names_of(e, cols), i) == *h))
                .collect();
            for at in mine {
                row.push(match at {
                    Some(i) => log_cell(&e.change, i),
                    None => Value::Null,
                });
            }
            row
        })
        .collect();

    ResultSet::from_rows(columns, rows)
}

/// The exported spelling of a change kind — the modal's own labels, so a file and
/// the screen it came from say the same word.
fn action_label(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Insert => "INSERT",
        ChangeKind::Update => "UPDATE",
        ChangeKind::Delete => "DELETE",
    }
}

/// One watched-table column of an exported change row: the `old → new` transition
/// when this update touched column `i`, otherwise the row's value there.
fn log_cell(change: &RowChange, i: usize) -> Value {
    if let Some(f) = change.fields.iter().find(|f| f.col == i) {
        return Value::Str(format!(
            "{}{TRANSITION}{}",
            null_text(&f.old),
            null_text(&f.new)
        ));
    }
    match change.cells.get(i).cloned().flatten() {
        Some(v) => Value::Str(v),
        None => Value::Null,
    }
}

/// A value as it reads *inside* a transition cell, where a real NULL can't be
/// expressed. Matches [`Value::display`], which is what the grid shows.
fn null_text(v: &Option<String>) -> &str {
    v.as_deref().unwrap_or("NULL")
}

#[cfg(test)]
mod discard_tests {
    use super::*;

    /// All four cases, because the two "no" answers are the ones that keep the
    /// prompt from becoming noise every reader learns to dismiss.
    #[test]
    fn only_an_unexported_non_empty_log_is_worth_asking_about() {
        assert!(discard_needs_asking(40, false), "the case that loses data");
        assert!(!discard_needs_asking(40, true), "there is a copy on disk");
        assert!(!discard_needs_asking(0, false), "nothing to lose");
        assert!(!discard_needs_asking(0, true));
    }

    #[test]
    fn one_change_is_still_a_change() {
        assert!(discard_needs_asking(1, false));
    }

    /// **Exactly at the cap, nothing has been dropped.** That was the disagreement
    /// between the two crates: the trim said `>`, the caveat said `>=`, and the
    /// first time a log filled it claimed a loss that hadn't happened.
    #[test]
    fn a_full_log_has_dropped_nothing_until_it_overflows() {
        let mut log: Vec<usize> = (0..LOG_CAP).collect();
        assert_eq!(trim_log(&mut log), 0);
        assert_eq!(log.len(), LOG_CAP);
        assert_eq!(log[0], 0, "and nothing came off the front");
    }

    #[test]
    fn an_overflowing_log_drops_the_oldest_and_says_how_many() {
        let mut log: Vec<usize> = (0..LOG_CAP + 3).collect();
        assert_eq!(trim_log(&mut log), 3);
        assert_eq!(log.len(), LOG_CAP);
        assert_eq!(log[0], 3, "the oldest three went, in order");
    }

    #[test]
    fn a_short_log_is_untouched() {
        let mut log: Vec<usize> = vec![1, 2, 3];
        assert_eq!(trim_log(&mut log), 0);
        assert_eq!(log, vec![1, 2, 3]);
        let mut empty: Vec<usize> = Vec::new();
        assert_eq!(trim_log(&mut empty), 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── The poll lifecycle ───────────────────────────────────────────────────

    /// **The one that would have caught Pause becoming Stop.** A paused tick has
    /// to re-arm: that is what makes Resume free, and what lets the baseline age
    /// so the first poll after a resume reports the net change across the pause.
    /// Inverting the two statements is a one-character edit that leaves the whole
    /// suite green without this.
    #[test]
    fn a_paused_tick_reschedules_rather_than_stopping() {
        assert_eq!(tick_action(true, false, true), TickAction::Reschedule);
        assert_ne!(tick_action(true, false, true), TickAction::Stop);
    }

    #[test]
    fn an_open_unpaused_monitor_fetches() {
        assert_eq!(tick_action(true, false, false), TickAction::Fetch);
    }

    /// Closing the modal is the only thing that ends the loop.
    #[test]
    fn a_closed_monitor_stops() {
        assert_eq!(tick_action(false, false, false), TickAction::Stop);
        assert_eq!(tick_action(false, false, true), TickAction::Stop);
    }

    /// A superseded generation must neither fetch nor re-arm — re-arming would
    /// leave the old target's loop polling beside the new one, forever.
    #[test]
    fn a_superseded_generation_stops_even_when_open() {
        assert_eq!(tick_action(true, true, false), TickAction::Stop);
        // And pausing doesn't rescue it: the timer belongs to a target the user
        // has already left.
        assert_eq!(tick_action(true, true, true), TickAction::Stop);
    }

    /// **A shift, not a set difference.** The whole snapshot pipeline is
    /// positional, so what matters is whether cell *i* still means the same
    /// column — which an added, dropped, reordered or renamed column all break,
    /// and which nothing about the set of names alone can answer.
    #[test]
    fn a_column_added_dropped_or_moved_makes_the_baseline_stale() {
        let c = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let base = c(&["id", "name", "total"]);
        assert!(!baseline_is_stale(&base, &base));
        // Added mid-table — the case that logs every row as an UPDATE.
        assert!(baseline_is_stale(
            &base,
            &c(&["id", "note", "name", "total"])
        ));
        // Added at the end still shifts nothing, but the next poll's rows are a
        // different width and the diff would report every one of them.
        assert!(baseline_is_stale(
            &base,
            &c(&["id", "name", "total", "note"])
        ));
        // Dropped before the key — the case that collapses every key to empty.
        assert!(baseline_is_stale(&base, &c(&["name", "total"])));
        // Reordered, same set: a set comparison would call this unchanged.
        assert!(baseline_is_stale(&base, &c(&["id", "total", "name"])));
        // Renamed in place: the values still line up, but the log would print
        // the old name against them.
        assert!(baseline_is_stale(&base, &c(&["id", "full_name", "total"])));
        // And case is a change, because it is a change to what the log prints.
        assert!(baseline_is_stale(&base, &c(&["id", "Name", "total"])));
    }

    fn row(key: &str, cells: &[Option<&str>]) -> SnapshotRow {
        SnapshotRow {
            key: vec![key.to_string()],
            cells: cells.iter().map(|c| c.map(|s| s.to_string())).collect(),
        }
    }

    fn snap(rows: Vec<SnapshotRow>) -> Snapshot {
        Snapshot {
            rows,
            ordered: false,
            window_full: false,
        }
    }

    /// The same, but flagged as the first N rows of an ordered table with more
    /// beyond — what a full `ORDER BY key LIMIT n` poll produces.
    fn window(rows: Vec<SnapshotRow>) -> Snapshot {
        Snapshot {
            rows,
            ordered: true,
            window_full: true,
        }
    }

    // ── a bounded window is not the whole table ──────────────────────────────

    /// **What it costs to call an arbitrary window an ordered prefix.**
    ///
    /// Every other `window()` fixture here builds *both* sides as honest ordered
    /// prefixes, so nothing pinned the price of `ordered` being wrong — which is
    /// how the Live Monitor came to stamp it from a **second** evaluation of the
    /// order key, taken on the reply, after the fetch the flag describes had
    /// already been sent. Poll 1 goes out unordered because the schema has not
    /// landed; the schema lands while it is in flight; the reply is stamped
    /// ordered.
    ///
    /// `ordered` is what licenses `last_common`: two windows that really are
    /// prefixes of one sequence can be compared up to the last row they share,
    /// and beyond it the window merely slid. An arbitrary window shares rows
    /// with the ordered page by coincidence, so that cut lands wherever the
    /// coincidence ends — and every row of the arbitrary sample before it that
    /// the ordered page does not hold is reported as a **DELETE**, cells and
    /// all, into a log the modal's own docs call the only remaining record of
    /// what a deleted row held. The rows are still in the table; they sort past
    /// position `ROW_CAP`.
    ///
    /// Told the truth, `diff_snapshots` attributes nothing at either edge — an
    /// arbitrary window's silence means nothing, which is the third state
    /// `window_full` was split out to express.
    #[test]
    fn an_arbitrary_window_called_ordered_reports_deletes_of_rows_that_are_still_there() {
        // The arbitrary sample poll 1 came back with, and the ordered first page
        // poll 2 fetched. They share `a` and `d`; `b` and `c` sort past the cap.
        let old_rows = vec![
            row("a", &[Some("1")]),
            row("b", &[Some("2")]),
            row("c", &[Some("3")]),
            row("d", &[Some("4")]),
        ];
        let new_rows = vec![
            row("a", &[Some("1")]),
            row("d", &[Some("4")]),
            row("e", &[Some("5")]),
            row("f", &[Some("6")]),
        ];
        let lie = diff_snapshots(
            &Snapshot {
                rows: old_rows.clone(),
                ordered: true,
                window_full: true,
            },
            &Snapshot {
                rows: new_rows.clone(),
                ordered: true,
                window_full: true,
            },
        );
        let deleted: Vec<&Vec<String>> = lie
            .iter()
            .filter(|c| c.kind == ChangeKind::Delete)
            .map(|c| &c.key)
            .collect();
        assert_eq!(
            deleted,
            vec![&vec!["b".to_string()], &vec!["c".to_string()]],
            "this is the damage the second evaluation buys: {lie:?}"
        );

        let honest = diff_snapshots(
            &Snapshot {
                rows: old_rows,
                ordered: false,
                window_full: true,
            },
            &Snapshot {
                rows: new_rows,
                ordered: true,
                window_full: true,
            },
        );
        assert!(
            honest.is_empty(),
            "an arbitrary window's silence means nothing, at either edge: \
             {honest:?}"
        );
    }

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

    /// **What a pause costs, stated as a diff.** A paused tick keeps re-arming
    /// and leaves the baseline alone, so the snapshot the first poll after a
    /// resume compares against is however many intervals old the pause was. The
    /// claim that behaviour rests on is that such a comparison still reports the
    /// change — the *net* one, stamped at the resume — rather than nothing.
    ///
    /// Here the row was updated twice and another was deleted while the monitor
    /// was paused. One update comes back, carrying the latest value and not the
    /// intermediate one, and the delete comes back too. What is lost to a pause
    /// is the intermediate step, and that is the documented trade rather than a
    /// silent gap.
    #[test]
    fn a_stale_baseline_still_reports_the_net_change() {
        let old = window(vec![
            row("1", &[Some("a")]),
            row("2", &[Some("b")]),
            row("3", &[Some("c")]),
        ]);
        // Two intervals later: `1` went a → a2 → a3, and `2` is gone.
        let new = window(vec![row("1", &[Some("a3")]), row("3", &[Some("c")])]);
        let out = diff_snapshots(&old, &new);
        assert_eq!(out.len(), 2, "{out:?}");
        let upd = out
            .iter()
            .find(|c| c.kind == ChangeKind::Update)
            .expect("the update survives the staleness");
        assert_eq!(upd.key, vec!["1"]);
        assert_eq!(
            upd.cells[0].as_deref(),
            Some("a3"),
            "the net value, not the intermediate one"
        );
        assert_eq!(
            upd.fields.len(),
            1,
            "one column changed across the pause: {upd:?}"
        );
        assert!(
            out.iter()
                .any(|c| c.kind == ChangeKind::Delete && c.key == vec!["2"]),
            "{out:?}"
        );
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

    /// A window that came back **at the limit with no `ORDER BY`** — the shape
    /// every table with no primary key polled in.
    fn unordered_full(rows: Vec<SnapshotRow>) -> Snapshot {
        Snapshot {
            rows,
            ordered: false,
            window_full: true,
        }
    }

    /// **A full window with no order attributes nothing at its edges.**
    ///
    /// The exportable log filled with changes that never happened: measured on
    /// MariaDB 10.11, deleting one row of a 1,200-row keyed-but-PK-less table
    /// promoted `0000001001` into the second window and logged it as an
    /// INSERT; on PostgreSQL 16.15 an `UPDATE` moved the heap tuple out of the
    /// window and was logged as a **DELETE of the row it updated**, with the
    /// update never reported at all.
    ///
    /// Which rows an unordered `LIMIT` returns is the server's choice, so a
    /// key in one window and not the other says nothing about the table.
    #[test]
    fn two_full_unordered_windows_report_nothing_at_their_edges() {
        let old = unordered_full(vec![
            row("1", &[Some("a")]),
            row("2", &[Some("b")]),
            row("3", &[Some("c")]),
        ]);
        let new = unordered_full(vec![
            row("1", &[Some("a")]),
            row("3", &[Some("c")]),
            row("9", &[Some("i")]),
        ]);
        assert_eq!(
            diff_snapshots(&old, &new),
            Vec::new(),
            "`2` may still be there and `9` may always have been"
        );
    }

    /// **But a row both windows hold is still diffed.** Suppressing the edges
    /// must not turn the monitor off — an `UPDATE` to a row that stayed in
    /// view is exactly what it is for, and it is attributable whatever the
    /// order.
    #[test]
    fn an_update_inside_two_full_unordered_windows_is_still_reported() {
        let old = unordered_full(vec![row("1", &[Some("a")]), row("2", &[Some("b")])]);
        let new = unordered_full(vec![row("1", &[Some("a")]), row("2", &[Some("CHANGED")])]);
        let out = diff_snapshots(&old, &new);
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].kind, ChangeKind::Update);
        assert_eq!(out[0].key, vec!["2"]);
    }

    /// **Each cut asks about the *other* window.** A new snapshot below the
    /// limit *is* the table, so a key missing from it really is gone — even
    /// though the old window was full and neither was ordered.
    #[test]
    fn a_short_new_window_still_proves_a_delete() {
        let old = unordered_full(vec![row("1", &[Some("a")]), row("2", &[Some("b")])]);
        let new = snap(vec![row("1", &[Some("a")])]);
        let out = diff_snapshots(&old, &new);
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].kind, ChangeKind::Delete);
        assert_eq!(out[0].key, vec!["2"]);
    }

    /// …and the mirror: an old snapshot below the limit proves an insert, even
    /// though the new window is a full unordered one.
    #[test]
    fn a_short_old_window_still_proves_an_insert() {
        let old = snap(vec![row("1", &[Some("a")])]);
        let new = unordered_full(vec![row("1", &[Some("a")]), row("2", &[Some("b")])]);
        let out = diff_snapshots(&old, &new);
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].kind, ChangeKind::Insert);
        assert_eq!(out[0].key, vec!["2"]);
    }

    /// **Order alone is not enough, and fullness alone is not enough** — the
    /// two facts the single flag used to conflate. A full *ordered* pair still
    /// suppresses only its tail, which is the behaviour that already existed
    /// and must not have moved.
    #[test]
    fn a_full_ordered_pair_still_suppresses_only_its_tail() {
        let old = window(vec![
            row("1", &[Some("a")]),
            row("2", &[Some("b")]),
            row("3", &[Some("c")]),
        ]);
        let new = window(vec![
            row("1", &[Some("a")]),
            row("2", &[Some("b")]),
            row("4", &[Some("d")]),
        ]);
        // `3`/`4` are past the last row the two share: the window slid.
        assert_eq!(diff_snapshots(&old, &new), Vec::new());
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

    // ── exporting the change log ─────────────────────────────────────────────

    fn cells(v: &[Option<&str>]) -> Vec<Option<String>> {
        v.iter().map(|c| c.map(|s| s.to_string())).collect()
    }

    fn entry(at: &str, kind: ChangeKind, key: &str, cells_in: &[Option<&str>]) -> MonitorEntry {
        MonitorEntry {
            at: at.to_string(),
            change: RowChange {
                kind,
                key: vec![key.to_string()],
                fields: Vec::new(),
                cells: cells(cells_in),
            },
            seq: 0,
            // Empty: these fixtures predate the per-entry list, and that is the
            // case `log_result_set` answers with the list it is handed — the
            // compatibility path a restored log takes too.
            cols: std::sync::Arc::new(Vec::new()),
        }
    }

    /// **The sequence number only ever goes up, across the trim.** It is what the
    /// rendered list is keyed on, and a log at the cap slides — so if the numbers
    /// restarted, or repeated, floem would reuse a row's view for a different
    /// change and the list would stop matching the log it exports.
    #[test]
    fn appended_entries_are_numbered_monotonically_across_the_cap() {
        let change = |k: &str| RowChange {
            kind: ChangeKind::Insert,
            key: vec![k.to_string()],
            fields: Vec::new(),
            cells: Vec::new(),
        };
        let none = std::sync::Arc::new(Vec::new());
        let mut log: Vec<MonitorEntry> = Vec::new();
        assert_eq!(
            append_changes(&mut log, "0:01", vec![change("a")], &none),
            0
        );
        assert_eq!(
            append_changes(&mut log, "0:02", vec![change("b")], &none),
            0
        );
        assert_eq!(log.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(log[1].at, "0:02", "each carries its own observation time");

        // Fill to the cap, then past it: the front is dropped and the numbers keep
        // climbing, so no two live entries ever share one.
        let rest: Vec<RowChange> = (0..LOG_CAP).map(|i| change(&i.to_string())).collect();
        let dropped = append_changes(&mut log, "0:03", rest, &none);
        assert_eq!(dropped, 2, "the two oldest went");
        assert_eq!(log.len(), LOG_CAP);
        assert_eq!(log.first().unwrap().seq, 2);
        assert_eq!(log.last().unwrap().seq, (LOG_CAP + 1) as u64);
        let mut seqs: Vec<u64> = log.iter().map(|e| e.seq).collect();
        let before = seqs.len();
        seqs.dedup();
        assert_eq!(seqs.len(), before, "no repeats");
        assert!(seqs.windows(2).all(|w| w[0] < w[1]), "and strictly rising");
    }

    /// Every cell of one exported row, as display text (`NULL` for a real NULL),
    /// so a test can assert the whole row in one line.
    fn row_text(rs: &ResultSet, r: usize) -> Vec<String> {
        (0..rs.col_count())
            .map(|c| match rs.cell(r, c) {
                Some(cr) if !cr.is_null() => cr.text().to_string(),
                _ => "NULL".to_string(),
            })
            .collect()
    }

    fn col_names(rs: &ResultSet) -> Vec<String> {
        rs.columns.iter().map(|c| c.name.clone()).collect()
    }

    #[test]
    fn log_export_leads_with_time_action_key_then_the_watched_columns() {
        let log = [entry(
            "0:07",
            ChangeKind::Insert,
            "3",
            &[Some("3"), Some("bob")],
        )];
        let rs = log_result_set(&log, &["id".into(), "name".into()]);
        assert_eq!(col_names(&rs), ["Time", "Action", "Key", "id", "name"]);
        assert_eq!(rs.row_count(), 1);
        assert_eq!(row_text(&rs, 0), ["0:07", "INSERT", "3", "3", "bob"]);
    }

    #[test]
    fn log_export_of_an_update_shows_the_transition_and_keeps_the_context() {
        let mut e = entry(
            "1:20",
            ChangeKind::Update,
            "3",
            &[Some("3"), Some("bobby"), Some("x")],
        );
        e.change.fields = vec![FieldChange {
            col: 1,
            old: Some("bob".into()),
            new: Some("bobby".into()),
        }];
        let rs = log_result_set(&[e], &["id".into(), "name".into(), "tag".into()]);
        // Changed column reads `old → new`; unchanged ones keep their value, so
        // the exported row still says which row this was.
        assert_eq!(
            row_text(&rs, 0),
            ["1:20", "UPDATE", "3", "3", "bob → bobby", "x"]
        );
    }

    #[test]
    fn log_export_writes_a_standalone_null_as_a_real_null_cell() {
        let log = [entry("0:02", ChangeKind::Delete, "9", &[Some("9"), None])];
        let rs = log_result_set(&log, &["id".into(), "note".into()]);
        assert_eq!(rs.row_count(), 1);
        assert!(
            !rs.cell(0, 3).unwrap().is_null(),
            "the key column has a value"
        );
        // Not the text "NULL": each format gets to render a NULL its own way.
        assert!(rs.cell(0, 4).unwrap().is_null());
    }

    #[test]
    fn log_export_spells_a_null_inside_a_transition_because_the_cell_is_text() {
        let mut e = entry("0:30", ChangeKind::Update, "1", &[Some("1"), None]);
        e.change.fields = vec![FieldChange {
            col: 1,
            old: Some("set".into()),
            new: None,
        }];
        let rs = log_result_set(&e_vec(e), &["id".into(), "note".into()]);
        assert_eq!(row_text(&rs, 0)[4], "set → NULL");
        // …and it is text, not a NULL cell — the transition is one cell and only
        // one of its two halves is null.
        assert!(!rs.cell(0, 4).unwrap().is_null());
    }

    fn e_vec(e: MonitorEntry) -> Vec<MonitorEntry> {
        vec![e]
    }

    #[test]
    fn log_export_of_a_delete_keeps_the_last_seen_row() {
        // The row is gone from the database; this export is the only record of it.
        let log = [entry(
            "2:00",
            ChangeKind::Delete,
            "7",
            &[Some("7"), Some("gone")],
        )];
        let rs = log_result_set(&log, &["id".into(), "name".into()]);
        assert_eq!(row_text(&rs, 0), ["2:00", "DELETE", "7", "7", "gone"]);
    }

    #[test]
    fn log_export_joins_a_composite_key_the_way_the_modal_renders_it() {
        let mut e = entry("0:05", ChangeKind::Insert, "a", &[Some("a"), Some("2")]);
        e.change.key = vec!["a".into(), "2".into()];
        let rs = log_result_set(&e_vec(e), &["part".into(), "n".into()]);
        assert_eq!(row_text(&rs, 0)[2], "a, 2");
    }

    #[test]
    fn log_export_widens_rather_than_dropping_a_column_the_baseline_never_named() {
        // Shouldn't happen — one query produced both — but narrowing an export is
        // the failure nobody can notice afterwards, so the data wins.
        let log = [entry(
            "0:01",
            ChangeKind::Insert,
            "1",
            &[Some("1"), Some("b"), Some("c")],
        )];
        let rs = log_result_set(&log, &["id".into()]);
        assert_eq!(
            col_names(&rs),
            ["Time", "Action", "Key", "id", "column_2", "column_3"]
        );
        assert_eq!(row_text(&rs, 0), ["0:01", "INSERT", "1", "1", "b", "c"]);
    }

    #[test]
    fn log_export_keeps_the_named_columns_when_a_change_is_narrower() {
        // The mirror: a short `cells` pads with NULL instead of losing a column.
        let log = [entry("0:01", ChangeKind::Insert, "1", &[Some("1")])];
        let rs = log_result_set(&log, &["id".into(), "name".into()]);
        assert_eq!(col_names(&rs), ["Time", "Action", "Key", "id", "name"]);
        assert!(rs.cell(0, 4).unwrap().is_null());
    }

    #[test]
    fn log_export_of_an_empty_log_is_headers_and_no_rows() {
        let rs = log_result_set(&[], &["id".into(), "name".into()]);
        assert_eq!(col_names(&rs), ["Time", "Action", "Key", "id", "name"]);
        assert_eq!(rs.row_count(), 0);
    }

    #[test]
    fn log_export_survives_a_log_with_no_columns_captured_yet() {
        let rs = log_result_set(&[], &[]);
        assert_eq!(col_names(&rs), ["Time", "Action", "Key"]);
        assert_eq!(rs.row_count(), 0);
    }

    #[test]
    fn log_export_orders_rows_oldest_first_as_logged() {
        let log = [
            entry("0:01", ChangeKind::Insert, "1", &[Some("1")]),
            entry("0:09", ChangeKind::Delete, "1", &[Some("1")]),
        ];
        let rs = log_result_set(&log, &["id".into()]);
        assert_eq!(rs.row_count(), 2);
        assert_eq!(row_text(&rs, 0)[1], "INSERT");
        assert_eq!(row_text(&rs, 1)[1], "DELETE");
    }

    #[test]
    fn log_formats_offer_every_grid_format_but_sql() {
        use crate::export::ExportFormat;
        let dropped: Vec<_> = ExportFormat::ALL
            .iter()
            .filter(|f| !LOG_FORMATS.contains(f))
            .collect();
        assert_eq!(dropped, [&ExportFormat::Sql]);
    }

    #[test]
    fn log_export_renders_through_the_ordinary_csv_renderer() {
        // The point of projecting to a `ResultSet`: no second renderer exists.
        let log = [entry(
            "0:07",
            ChangeKind::Insert,
            "3",
            &[Some("3"), Some("bob")],
        )];
        let rs = log_result_set(&log, &["id".into(), "name".into()]);
        let order: Vec<usize> = (0..rs.row_count()).collect();
        let out = crate::export::ExportFormat::Csv.render(
            &rs,
            &order,
            None,
            crate::intel::SqlDialect::MySql,
        );
        assert!(out.starts_with("Time,Action,Key,id,name"), "{out}");
        assert!(out.contains("0:07,INSERT,3,3,bob"), "{out}");
    }

    /// **The NULL rule, where it becomes bytes.** `log_result_set`'s central
    /// promise is that a value standing alone is a *real* NULL cell — so each
    /// format renders it its own way — while inside an `old → new` transition it
    /// can only be the literal text `NULL`, there being no way to put a real
    /// NULL inside a string. Every existing test asserts the first half at the
    /// `ResultSet` level (`cell(..).is_null()`) and none checks what a renderer
    /// writes, so nothing distinguished the intended outcome from the failure
    /// mode the design exists to avoid: the four characters `NULL` in all four
    /// formats, indistinguishable from a column whose value is the string
    /// "NULL".
    ///
    /// Table-driven over [`LOG_FORMATS`] so a sixth format has to state its
    /// answer rather than inherit CSV's by omission — which is exactly what
    /// Excel had to do when it arrived: its spelling of absent is an *empty
    /// cell*, and a worksheet is the one place where that is genuinely not the
    /// same value as an empty string.
    #[test]
    fn every_log_format_renders_a_null_its_own_way() {
        use crate::export::ExportFormat;
        // One update: `note` went `hi` → NULL (a transition), and `extra` is a
        // standalone NULL because this change never touched it.
        let log = [MonitorEntry {
            at: "0:07".into(),
            change: RowChange {
                kind: ChangeKind::Update,
                key: vec!["3".into()],
                fields: vec![FieldChange {
                    col: 0,
                    old: Some("hi".into()),
                    new: None,
                }],
                cells: vec![None, None],
            },
            seq: 0,
            cols: std::sync::Arc::new(Vec::new()),
        }];
        let rs = log_result_set(&log, &["note".into(), "extra".into()]);
        let order: Vec<usize> = (0..rs.row_count()).collect();
        let render = |f: ExportFormat| f.render(&rs, &order, None, crate::intel::SqlDialect::MySql);

        for f in LOG_FORMATS.into_iter().filter(|f| f.is_text()) {
            let out = render(f);
            // The transition is text in every format, and says NULL on its right.
            assert!(
                out.contains(&format!("hi{TRANSITION}NULL")),
                "{f:?}: the transition lost its wording\n{out}"
            );
        }

        // Excel is the one format whose output isn't a string, so it is read
        // back as cells. Same two properties: the transition keeps its wording,
        // and the standalone NULL is an *empty cell* — not the text "NULL",
        // which would be indistinguishable from a value that really is "NULL".
        {
            use calamine::{Data, Reader, Xlsx};
            let mut buf = Vec::new();
            ExportFormat::Xlsx
                .render_to(&mut buf, &rs, &order, None, crate::intel::SqlDialect::MySql)
                .expect("writing to a Vec cannot fail");
            let mut wb: Xlsx<_> = Xlsx::new(std::io::Cursor::new(buf)).expect("a valid workbook");
            let name = wb.sheet_names()[0].clone();
            let range = wb.worksheet_range(&name).expect("the first sheet");
            let header: Vec<String> = range
                .rows()
                .next()
                .unwrap()
                .iter()
                .map(|c| c.to_string())
                .collect();
            let row = range.rows().nth(1).expect("one logged change");
            let note = header
                .iter()
                .position(|h| h == "note")
                .expect("a note column");
            let extra = header
                .iter()
                .position(|h| h == "extra")
                .expect("an extra column");
            assert_eq!(row[note].to_string(), format!("hi{TRANSITION}NULL"));
            assert_eq!(
                row[extra],
                Data::Empty,
                "Excel: an empty cell, not the text NULL"
            );
        }
        // …and the standalone NULL is each format's own spelling of absent.
        let csv = render(ExportFormat::Csv);
        assert!(
            csv.lines().nth(1).is_some_and(|l| l.ends_with(',')),
            "CSV: an empty field, not the text NULL\n{csv}"
        );
        let json = render(ExportFormat::Json);
        assert!(
            json.contains("\"extra\": null") || json.contains("\"extra\":null"),
            "JSON: a real null, not a string\n{json}"
        );
        assert!(
            !json.contains("\"extra\": \"NULL\""),
            "JSON: the string NULL is the failure mode\n{json}"
        );
    }

    /// The other two decisions a change log only meets through a renderer, both
    /// real guards in `core::export` and neither pinned from this side.
    #[test]
    fn the_log_projection_meets_the_renderers_own_guards() {
        use crate::export::ExportFormat;
        // A watched table with a column of its own called `Time` — the same name
        // as the observation timestamp the projection puts in front of it.
        let log = [entry("0:07", ChangeKind::Insert, "3", &[Some("14:00")])];
        let rs = log_result_set(&log, &["Time".into()]);
        let order: Vec<usize> = (0..rs.row_count()).collect();
        let json = ExportFormat::Json.render(&rs, &order, None, crate::intel::SqlDialect::MySql);
        // De-duplicated rather than merged: a JSON object can hold one `Time`,
        // and the row's value must not overwrite the timestamp.
        assert!(json.contains("\"Time\""), "{json}");
        assert!(json.contains("Time_2"), "the duplicate is renamed\n{json}");
        assert!(json.contains("0:07") && json.contains("14:00"), "{json}");

        // CSV neutralises a cell a spreadsheet would read as a formula.
        let log = [entry(
            "0:08",
            ChangeKind::Insert,
            "4",
            &[Some("=cmd|'/c calc'!A1")],
        )];
        let rs = log_result_set(&log, &["note".into()]);
        let order: Vec<usize> = (0..rs.row_count()).collect();
        let csv = ExportFormat::Csv.render(&rs, &order, None, crate::intel::SqlDialect::MySql);
        assert!(
            !csv.contains(",=cmd"),
            "the leading `=` must not survive bare\n{csv}"
        );
    }

    /// **The Data column is a line, not the record.** One span per column name
    /// plus one per value — four per field on an update — uncapped in the
    /// table's width, times `LOG_CAP` rows: 23,000 views at 10 columns, 63,000
    /// at 30, 123,000 at 60, in a mounted list showing about twenty, paid by
    /// every layout and paint pass while the modal is open.
    ///
    /// What must not happen is the quiet version: a line that stops at twelve of
    /// sixty and looks like the whole change.
    #[test]
    fn a_wide_change_says_how_much_of_it_is_off_the_line() {
        assert_eq!(data_overflow_label(0), None);
        assert_eq!(data_overflow_label(DATA_COLS_SHOWN), None, "exactly full");
        assert_eq!(
            data_overflow_label(DATA_COLS_SHOWN + 1).as_deref(),
            Some(", +1 more")
        );
        assert_eq!(
            data_overflow_label(60).as_deref(),
            Some(&format!(", +{} more", 60 - DATA_COLS_SHOWN)[..])
        );
    }

    /// **A column list that moved under the log must not re-head what was
    /// recorded before it moved.**
    ///
    /// Everything about a change is positional: `FieldChange { col: 1 }` means
    /// "the second column", and which column that *is* came from a list held
    /// beside the log. That was sound while the list could only be written once
    /// per monitor session — until the baseline restart answering an `ALTER`
    /// re-opened the branch that writes it. `orders(id, name, total)`, a few
    /// UPDATEs to `name`, then
    /// `ALTER TABLE orders ADD COLUMN note TEXT AFTER id`: every earlier UPDATE
    /// exported under `note`, and every earlier INSERT/DELETE row — "the only
    /// remaining record of a row the database no longer has" — one column left
    /// of its heading. Nothing in the file said so: the restart notice goes to
    /// the poll's error slot, which the next poll clears.
    #[test]
    fn a_change_is_exported_under_the_names_it_was_recorded_with() {
        let before = std::sync::Arc::new(vec![
            "id".to_string(),
            "name".to_string(),
            "total".to_string(),
        ]);
        let after = std::sync::Arc::new(vec![
            "id".to_string(),
            "note".to_string(),
            "name".to_string(),
            "total".to_string(),
        ]);
        let update = |col: usize, old: &str, new: &str| RowChange {
            kind: ChangeKind::Update,
            key: vec!["7".to_string()],
            fields: vec![FieldChange {
                col,
                old: Some(old.to_string()),
                new: Some(new.to_string()),
            }],
            cells: Vec::new(),
        };
        let mut log: Vec<MonitorEntry> = Vec::new();
        // Recorded under the old list: index 1 is `name`.
        append_changes(&mut log, "0:01", vec![update(1, "ann", "bob")], &before);
        // …then the table gained a column, and index 1 is now `note`.
        append_changes(&mut log, "0:09", vec![update(1, "x", "y")], &after);

        let rs = log_result_set(&log, &after);
        let heading: Vec<&str> = rs.columns.iter().map(|c| c.name.as_str()).collect();
        let at = |n: &str| heading.iter().position(|h| *h == n).expect(n);
        let cell = |row: usize, n: &str| rs.cell(row, at(n)).expect("in range").display();

        // The pre-`ALTER` change is still headed `name`, and `note` — a column
        // that did not exist when it was recorded — says nothing about it.
        assert_eq!(cell(0, "name"), "ann → bob", "{heading:?}");
        assert_eq!(cell(0, "note"), "NULL", "{heading:?}");
        // The post-`ALTER` change is headed `note`, which is what index 1 means
        // now.
        assert_eq!(cell(1, "note"), "x → y", "{heading:?}");
        assert_eq!(cell(1, "name"), "NULL", "{heading:?}");
        // Every name from both shapes is present, once.
        for n in ["id", "note", "name", "total"] {
            assert_eq!(
                heading.iter().filter(|h| **h == n).count(),
                1,
                "{n} in {heading:?}"
            );
        }
    }

    /// **The bound is on the view, and on nothing else.** The export is the
    /// record, and it must still carry every column of every change — which is
    /// the property that makes truncating the line acceptable at all.
    #[test]
    fn the_export_still_carries_the_columns_the_line_does_not() {
        let wide: Vec<String> = (0..60).map(|i| format!("c{i}")).collect();
        let cells: Vec<Option<String>> = (0..60).map(|i| Some(format!("v{i}"))).collect();
        let mut log: Vec<MonitorEntry> = Vec::new();
        append_changes(
            &mut log,
            "12:00:00",
            vec![RowChange {
                kind: ChangeKind::Insert,
                key: vec!["1".to_string()],
                fields: Vec::new(),
                cells,
            }],
            &std::sync::Arc::new(wide.clone()),
        );

        let rs = log_result_set(&log, &wide);
        assert_eq!(rs.row_count(), 1, "one change, one row");
        let order: Vec<usize> = (0..rs.row_count()).collect();
        let data = crate::export::ExportFormat::Csv.render(
            &rs,
            &order,
            None,
            crate::intel::SqlDialect::MySql,
        );
        // The export gives every watched column a column of its own, so the
        // header names all sixty and the row carries all sixty values —
        // including the forty-eight the modal's one-line Data column stops at.
        for i in [0usize, DATA_COLS_SHOWN, DATA_COLS_SHOWN + 1, 59] {
            assert!(
                data.contains(&format!("c{i}")),
                "column {i} is missing from the export: {data}"
            );
            assert!(
                data.contains(&format!("v{i}")),
                "column {i}'s value is missing from the export: {data}"
            );
        }
        assert!(
            !data.contains("more"),
            "the export is the record, not the line: {data}"
        );
    }
}
