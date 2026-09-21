//! The query-history store: the persisted entry list, the run-id allocator, and
//! the five closures that mutate it.
//!
//! Carved out of `app_view` because of an invariant, not because of a line
//! count — the procedure under *Splitting `lib.rs` / `main.rs`* in
//! `docs/architecture.md`. **Every mutation of this store is followed by a save,
//! and which save is policy**: an ordinary [`persist::save_json`] for a run that
//! was recorded or finished, and [`persist::save_json_erasing`] for the three
//! paths that *remove* — the panel's trash button, a row's Delete, and a
//! connection being deleted. The distinction is not cosmetic and is the reason
//! this module exists as one: an ordinary save copies the pre-change generation
//! to `history.json.bak`, so a confirm reading *"This can't be undone"* left
//! every statement it named on disk, and "not reconstructable from what's left"
//! — the claim connection deletion makes — was false for the same reason.
//!
//! That policy is now stated once, in [`save`], which is also the only place the
//! `"history.json"` name and the [`HistoryFile`] wrapper are written. Before the
//! cut there were five copies of that four-line block in `app_view`, each
//! choosing its own save, which is exactly the shape a policy drifts out of.
//! `the_removal_paths_erase` is the gate that holds it.
//!
//! **What is deliberately *not* here.** `open_history` — the history panel's
//! double-click — takes a [`HistoryEntry`] and opens a *tab* from it, so it
//! needs the tab id allocator and the placement closure and belongs to that
//! domain; it only reads an entry and never touches this store. It stays in
//! `app_view` and is put into `HistoryActions` beside these at the `Ui` literal.

use std::cell::Cell;
use std::rc::Rc;

use floem::prelude::*;

use schemaic_core::connection::Connection;
use schemaic_core::history::{HistoryEntry, HistoryFile, RunResult};
use schemaic_core::intel::SqlDialect;

use crate::persist;

/// Record a run's statements and hand back one run id each, in order —
/// `(conn_id, database, statements, tab_name)`. The ids are what
/// [`FinishHistoryFn`] later reports those runs' outcomes against.
///
/// A **slice**, and one file write for the lot. It took a single statement and
/// wrote the whole of `history.json` each time — clone the cross-connection
/// vector, serialize it, temp file, read-back, `.bak`, rename — so Run Everything
/// on a hundred-statement migration did that a hundred times in one UI-thread
/// handler, ~500 fs operations and O(N × min(N, MAX_PER_CONN)) entry
/// serializations, before the batch was even spawned. `finish_history` was given
/// this shape by an earlier fix; only the launch half was left.
pub(crate) type RecordHistoryFn =
    Rc<dyn Fn(u64, Option<String>, &[String], Option<String>) -> Vec<u64>>;

/// Fill in how runs went — `(run id, outcome)` per run, onto the history
/// entries their launch already wrote — and delete the entries of runs that
/// never happened.
///
/// A **slice**, not one run, because Run Everything lands a whole batch at once
/// and each recorded run would otherwise cost a full rewrite of `history.json`.
/// One call for the whole slice, and one file write for both halves.
pub(crate) type FinishHistoryFn = Rc<dyn Fn(&[(u64, RunResult)], &[u64])>;

/// Where the session's run-id counter starts: past **every** id on disk, across
/// all connections. Each `record` then hands out `seed + 1`, `+ 2`, …
///
/// Three properties, and the whole of the argument that a landing run reports
/// against the entry it launched:
///
/// - **Global, not per-connection.** Ids are matched by `finish` without a
///   connection filter, so a per-connection seed would let two connections issue
///   the same id and let one run's outcome land on the other's entry.
/// - **Only ever counting up.** Re-deriving `max + 1` per push would reuse an id
///   the moment the per-connection cap evicted the entry holding the maximum —
///   while the run holding it was still in flight.
/// - **Never zero.** Entries written before run ids exist carry `0`, so the
///   first id handed out must not be one, or a landing run would claim a legacy
///   entry. `max().unwrap_or(0)` on an empty history seeds 0 and the first
///   allocation is 1.
pub(crate) fn run_id_seed(entries: &[HistoryEntry]) -> u64 {
    entries.iter().map(|e| e.run_id).max().unwrap_or(0)
}

/// Hand out `n` run ids from `next`, each one past everything issued before it.
///
/// **A function because a test could not otherwise reach it.** This was three
/// lines inside `record`'s `map`, and `a_run_id_is_seeded_past_every_id_on_disk`
/// asserted [`run_id_seed`] alone — so the name said "past every id on disk"
/// while the `+ 1` that makes an allocated id *past* the seed was untested. Both
/// mutations the seed's own doc names survived it: drop the `+ 1` and every run
/// of a session reuses the maximum id already on disk, so the first run to land
/// writes its timing and outcome onto a pre-existing entry — a row in the
/// panel whose duration and row count belong to a query somebody ran last week.
///
/// The three properties [`run_id_seed`] documents are only true of the pair:
/// the seed supplies "global" and "never zero", and this supplies "only ever
/// counting up", which is the one that stops an id being reused while the run
/// holding it is still in flight.
pub(crate) fn allocate(next: &Cell<u64>, n: usize) -> Vec<u64> {
    (0..n)
        .map(|_| {
            let id = next.get() + 1;
            next.set(id);
            id
        })
        .collect()
}

/// The store's file. A `const` rather than five string literals, so the gate
/// below can say "named once" and mean it.
const FILE: &str = "history.json";

/// **The one place the save policy is chosen.** See the module doc for why
/// `Erasing` is not interchangeable with `Replacing` here.
fn save(entries: RwSignal<Vec<HistoryEntry>>, saving: persist::Saving) {
    let file = HistoryFile {
        entries: entries.get_untracked(),
    };
    match saving {
        persist::Saving::Erasing => persist::save_json_erasing(FILE, &file),
        persist::Saving::Replacing => persist::save_json(FILE, &file),
    }
}

/// The store, as `app_view` receives it: the signal the panel renders, and the
/// five closures that are allowed to write it — which is also the count
/// `the_removal_paths_erase` asserts, as 3 erasing plus 2 replacing.
///
/// `record` and `finish` go to the run paths rather than into `HistoryActions` —
/// they are how a query run reports itself, not something the panel offers.
pub(crate) struct HistoryStore {
    /// Persisted, newest-first across all connections; the panel filters to the
    /// active one.
    pub(crate) entries: RwSignal<Vec<HistoryEntry>>,
    pub(crate) record: RecordHistoryFn,
    pub(crate) finish: FinishHistoryFn,
    /// Clear the active connection's history — the panel's trash button.
    pub(crate) clear: Rc<dyn Fn()>,
    /// Delete one entry — the row's menu.
    pub(crate) remove: Rc<dyn Fn(HistoryEntry)>,
    /// Drop a deleted connection's history. Called from `app_view`'s
    /// connection-deletion block, which erases twelve stores in a row and keeps
    /// its own shape by each of them being one line.
    pub(crate) clear_conn: Rc<dyn Fn(u64)>,
}

/// Load the store and build its closures.
///
/// `connections` is read for the *dialect* only — `history::push` skips
/// credential-bearing statements, and where a string or comment ends differs per
/// engine — and `active_conn` is what the panel's trash button clears.
pub(crate) fn wire(
    connections: RwSignal<Vec<Connection>>,
    active_conn: RwSignal<u64>,
) -> HistoryStore {
    let entries = RwSignal::new(persist::load_json::<HistoryFile>(FILE).entries);

    // Run ids, handed out by `record` and quoted back by `finish` — see
    // `HistoryEntry::run_id`. Seeded past every id on disk, and only ever
    // counting up, so an id can't be reused while the run holding it is still in
    // flight (which re-deriving `max + 1` per push would allow, once the
    // per-connection cap evicted the entry holding the maximum).
    let run_ids: Rc<Cell<u64>> = Rc::new(Cell::new(entries.with_untracked(|v| run_id_seed(v))));

    // Record an executed query into the history (newest-first, capped) and
    // persist it. Called from every run path (single Run, Run Current, Run
    // Everything).
    let record: RecordHistoryFn = {
        let run_ids = run_ids.clone();
        Rc::new(
            move |conn_id: u64,
                  database: Option<String>,
                  stmts: &[String],
                  tab_name: Option<String>| {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                // The connection's own dialect: `push` skips credential-bearing
                // statements, and where a string or comment ends differs per
                // engine.
                let dialect = connections
                    .with_untracked(|cs| {
                        cs.iter()
                            .find(|c| c.id == conn_id)
                            .map(|c| SqlDialect::from_db_type(&c.db_type))
                    })
                    .unwrap_or_default();
                // The whole batch's ids up front, through `allocate` — see its
                // doc for why the `+ 1` is not written inline here any more.
                let ids = allocate(&run_ids, stmts.len());
                // **One batch, not N pushes.** The per-connection cap applied on
                // each push let a script longer than the cap evict the whole of
                // the connection's real history — and its own dispatched
                // statements with it — before anything ran; `finish` then
                // dropped the tail and left nothing at all. `push_batch` defers
                // the cap to `history::trim`, which runs once the outcomes are
                // known. See `history::push_batch`.
                let batch: Vec<_> = stmts
                    .iter()
                    .zip(ids.iter().copied())
                    .map(|(sql, run_id)| {
                        HistoryEntry {
                            conn_id,
                            database: database.clone(),
                            sql: sql.clone(),
                            ts,
                            run_id,
                            tab_name: tab_name.clone(),
                            // Filled in by `finish` when the run lands.
                            duration_ms: None,
                            rows: None,
                            rows_capped: false,
                            outcome: schemaic_core::history::Outcome::Unknown,
                        }
                    })
                    .collect();
                let mut wrote = false;
                entries.update(|v| {
                    wrote = schemaic_core::history::push_batch(v, batch, dialect) > 0;
                });
                // Skipped when nothing was recorded — the same skip `finish`
                // documents. A credential-bearing statement records nothing and
                // used to cost a whole atomic rewrite for it.
                if wrote {
                    save(entries, persist::Saving::Replacing);
                }
                ids
            },
        )
    };

    // Fill in how runs went, on the entries `record` wrote when they launched
    // (see `history::finish` for why it is two passes and not one).
    //
    // Persists once for the whole slice, not once per statement: a save clones
    // the entire history, serializes it, and does an atomic write (temp file,
    // read-back, `.bak`, rename). Run Everything on a migration script lands a
    // hundred statements in a single UI-thread callback, and one write each
    // froze the window for as long as that took. The write is also skipped
    // entirely when nothing was updated — a credential-bearing statement is
    // never recorded, and would otherwise cost a file write for nothing.
    // `dropped` is the runs that never happened — the tail of a script that
    // stopped, which was pushed at launch and would otherwise evict the
    // connection's real history under `MAX_PER_CONN`. See `history::drop_runs`.
    let finish: FinishHistoryFn = Rc::new(move |runs: &[(u64, RunResult)], dropped: &[u64]| {
        let updated = entries.try_update(|v| {
            let mut any = schemaic_core::history::drop_runs(v, dropped);
            for (run_id, result) in runs {
                any |= schemaic_core::history::finish(v, *run_id, *result);
            }
            // **The cap, now that the outcomes are known.** `push_batch`
            // deliberately leaves the connection over it at launch so a script
            // cannot evict history before anything has run; this is the only
            // moment anything can tell a statement that ran from one that was
            // never sent. Idempotent, so a batch that fitted reports no change
            // and costs no write.
            any |= schemaic_core::history::trim(v);
            any
        });
        if updated != Some(true) {
            return;
        }
        save(entries, persist::Saving::Replacing);
    });

    // Clear the active connection's history (the panel's trash button),
    // persisting.
    let clear: Rc<dyn Fn()> = Rc::new(move || {
        let conn = active_conn.get_untracked();
        entries.update(|v| schemaic_core::history::clear_conn(v, conn));
        // **Erasing.** The confirm behind this button reads "This can't be
        // undone", and the ordinary save left every statement it named in
        // `history.json.bak` until the next query run.
        save(entries, persist::Saving::Erasing);
    });

    // Delete one history entry (the row's menu), persisting. The write is
    // skipped when nothing matched — a row already gone shouldn't cost a full
    // rewrite of the file, which is what `remove`'s bool is for.
    let remove: Rc<dyn Fn(HistoryEntry)> = Rc::new(move |entry: HistoryEntry| {
        let mut hit = false;
        entries.update(|v| {
            hit = schemaic_core::history::remove(v, entry.conn_id, &entry.sql);
        });
        if hit {
            // Erasing: the point of this save is that the row is gone, and the
            // ordinary one would have left it in `history.json.bak`.
            save(entries, persist::Saving::Erasing);
        }
    });

    // A deleted connection's history goes with it. Erasing for the reason the
    // deletion block states: the connection shouldn't be reconstructable from
    // what's left on disk, and the ordinary save keeps the pre-deletion
    // generation as `.bak`.
    let clear_conn: Rc<dyn Fn(u64)> = Rc::new(move |id: u64| {
        entries.update(|v| schemaic_core::history::clear_conn(v, id));
        save(entries, persist::Saving::Erasing);
    });

    HistoryStore {
        entries,
        record,
        finish,
        clear,
        remove,
        clear_conn,
    }
}

#[cfg(test)]
mod tests {
    use super::{allocate, run_id_seed};
    use schemaic_core::history::{HistoryEntry, Outcome};
    use std::cell::Cell;

    /// The whole of the run-id allocator's correctness argument, which was
    /// untested: deleting the `+ 1` at the call site or narrowing the seed to the
    /// active connection left the suite green.
    #[test]
    fn a_run_id_is_seeded_past_every_id_on_disk() {
        let e = |conn_id: u64, run_id: u64| HistoryEntry {
            conn_id,
            database: None,
            sql: "SELECT 1".into(),
            ts: 0,
            run_id,
            tab_name: None,
            duration_ms: None,
            rows: None,
            rows_capped: false,
            outcome: Outcome::Unknown,
        };
        // Across **all** connections: `finish` matches by id with no connection
        // filter, so a per-connection seed would let one run's outcome land on
        // another connection's entry.
        assert_eq!(run_id_seed(&[e(1, 3), e(2, 9), e(1, 5)]), 9);
        // Empty history seeds 0, so the first id handed out is 1 — never the 0
        // that entries written before run ids carry.
        assert_eq!(run_id_seed(&[]), 0);
        assert_eq!(run_id_seed(&[e(1, 0), e(1, 0)]), 0);

        // **And "past", which is the word this test's name uses and which the
        // seed alone cannot supply.** The `+ 1` lived inside `record`'s closure
        // where nothing could reach it, so both mutations `run_id_seed`'s doc
        // names survived this test: without it every run of a session reuses
        // the maximum id on disk, and the first run to land writes its timing
        // onto a pre-existing entry.
        let on_disk = [e(1, 3), e(2, 9), e(1, 5)];
        let next = Cell::new(run_id_seed(&on_disk));
        let first = allocate(&next, 3);
        assert_eq!(
            first,
            vec![10, 11, 12],
            "the first ids of a session must be past every id on disk"
        );
        assert!(
            first
                .iter()
                .all(|id| on_disk.iter().all(|e| e.run_id != *id)),
            "an allocated id collides with one already in the history"
        );
        // A second batch continues rather than restarting, which is what stops
        // an id being reused while the run holding it is still in flight.
        assert_eq!(allocate(&next, 2), vec![13, 14]);
        // Never zero, even from an empty history — a legacy entry carries 0 and
        // `finish` matches by id.
        let fresh = Cell::new(run_id_seed(&[]));
        assert_eq!(allocate(&fresh, 1), vec![1]);
        // A batch of nothing takes nothing, so a run that records no statement
        // does not burn an id its outcome would then look for.
        assert!(allocate(&fresh, 0).is_empty());
        assert_eq!(fresh.get(), 1);
    }

    /// **The module's reason for existing, as a gate.** The three paths that
    /// remove history must erase, because the confirm behind two of them says
    /// "This can't be undone" and the third is a connection deletion promising
    /// the connection is not reconstructable from what is left on disk — and an
    /// ordinary save copies the pre-change generation to `history.json.bak`.
    ///
    /// Reads this file's own source, so it is the `source_gate` family's shape
    /// and carries that family's hazard: **a gate in the same file as its
    /// subject is its own first match**, which is why the needles below are
    /// assembled from fragments rather than written literally, and why the floor
    /// is asserted first — a scan that finds nothing must fail, not pass.
    #[test]
    fn the_removal_paths_erase() {
        // Through `production_code`, which blanks comment lines *and* the
        // `#[cfg(test)]` module below — so this gate does not count the prose
        // above that explains the policy, nor its own needles. Counting the raw
        // text found 12 `history.json`s where there are two calls, which is the
        // same class of mistake as a gate finding its own line.
        let raw = include_str!("history_store.rs");
        let src = schemaic_ui::source_gate::production_code(raw);
        let body = src
            .split_once("pub(crate) fn wire(")
            .expect("`wire` is what this gate reads")
            .1;
        let erasing = ["Saving", "::", "Erasing"].concat();
        let replacing = ["Saving", "::", "Replacing"].concat();
        // **Each closure paired with the verb it must use, not five verbs
        // counted over one body.** Counting holds just as well when the verbs
        // are *swapped* — give `remove` the `Replacing` and `record` the
        // `Erasing` and the totals are identical, while a deleted run survives
        // in `history.json.bak`. `let_regions` cuts `wire` at its bindings so
        // each assertion is about the closure it names.
        let regions = schemaic_ui::source_gate::let_regions(
            body,
            &["record", "finish", "clear", "remove", "clear_conn"],
        )
        .expect("every closure this gate names is still bound in `wire`");
        for (name, region) in &regions {
            let want_erasing = matches!(name.as_str(), "clear" | "remove" | "clear_conn");
            let (want, other, why) = if want_erasing {
                (
                    &erasing,
                    &replacing,
                    "removes rows, so the `.bak` must go too",
                )
            } else {
                (&replacing, &erasing, "is an ordinary save")
            };
            assert_eq!(
                region.matches(want.as_str()).count(),
                1,
                "`{name}` {why} — it should save exactly once, with \
                 `{want}`"
            );
            assert_eq!(
                region.matches(other.as_str()).count(),
                0,
                "`{name}` saves with `{other}`, which is the wrong policy for \
                 it — swapping two verbs keeps every total in this gate intact"
            );
        }
        // …and the totals stay, as a floor: they are what catches a closure
        // *deleted* rather than mis-saved.
        assert_eq!(
            body.matches(&erasing).count(),
            3,
            "expected exactly three erasing saves in `wire` — the trash button, \
             a row's Delete, and connection deletion"
        );
        assert_eq!(
            body.matches(&replacing).count(),
            2,
            "expected exactly two ordinary saves in `wire` — recording a run and \
             finishing one"
        );
        // And the file is named exactly once, by `FILE`, so the policy cannot be
        // sidestepped by a sixth copy of the save block growing back with its
        // own literal.
        assert_eq!(
            src.matches(&["history", ".json"].concat()).count(),
            1,
            "`history.json` should appear once in this module's code, as `FILE`"
        );
    }
}
