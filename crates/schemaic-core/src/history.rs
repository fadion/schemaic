//! Query history: a persisted, newest-first log of executed statements, scoped
//! per connection in the UI.
//!
//! Each run records the connection id, the database it ran against, the SQL, and
//! a wall-clock timestamp (unix millis). The store is a flat `Vec`, capped at
//! [`MAX_PER_CONN`] entries **per connection** (oldest dropped) rather than
//! globally — so the file grows with the number of connections, and one busy
//! connection can't evict another's history. The UI filters it to the active
//! connection and renders newest-first. Pure + tested here; the app owns the
//! signal and persists the list via `persist::save_json`.

use serde::{Deserialize, Serialize};

use crate::intel::SqlDialect;
use crate::sql;

/// Cap on stored entries **per connection**; the oldest beyond this are dropped.
pub const MAX_PER_CONN: usize = 50;

/// One executed query.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Saved connection id the query ran against (history is scoped by this).
    pub conn_id: u64,
    /// Database in use when it ran (`None` = server-level, no `USE`).
    #[serde(default)]
    pub database: Option<String>,
    /// The SQL that was executed (stored whole; the UI previews a clamped form).
    pub sql: String,
    /// Wall-clock time it ran, unix epoch milliseconds.
    pub ts: u64,
    /// The originating tab's user-assigned name, if any (shown as a label in the
    /// history panel). `None` for tabs left at the default "Query N".
    #[serde(default)]
    pub tab_name: Option<String>,
    /// Wall-clock milliseconds the run took, or `None` while it is still
    /// unknown — see [`Outcome::Unknown`].
    ///
    /// Wall-clock, not the server's own timing, because it is what the user
    /// waited through: a statement that spent 50 seconds behind someone else's
    /// row lock is the one worth finding here, and it is the same measurement
    /// whether the run succeeded or failed.
    #[serde(default)]
    pub duration_ms: Option<u64>,
    /// Rows the run produced: returned for a `SELECT`, **affected** for a write.
    /// `None` when it failed, or when the statement reports neither.
    ///
    /// One field rather than two, and one word ("rows") in the panel: the two
    /// numbers answer the same question — how much did this touch — and a
    /// history row is three facts wide.
    #[serde(default)]
    pub rows: Option<u64>,
    /// How it went, filled in when the run lands.
    #[serde(default)]
    pub outcome: Outcome,
}

/// How a recorded run turned out.
///
/// Three states, not a bool, because "we don't know" is a real one and is what
/// an entry starts in: history is written when a run *launches* (so a run the
/// app doesn't outlive is still recorded), and a cancelled run never reports
/// anything. Displayed as nothing at all rather than guessed at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "OutcomeRaw")]
pub enum Outcome {
    /// Still running, cancelled, or recorded before this was tracked.
    #[default]
    Unknown,
    Ok,
    Failed,
}

/// Parsing shim for [`Outcome`]; see [`crate::persist::RightPanelState`] for why
/// every persisted enum has one. A value a newer build wrote reads as
/// [`Outcome::Unknown`] rather than failing the whole of `history.json`.
#[derive(Deserialize)]
enum OutcomeRaw {
    Unknown,
    Ok,
    Failed,
    #[serde(other)]
    Other,
}

impl From<OutcomeRaw> for Outcome {
    fn from(raw: OutcomeRaw) -> Self {
        match raw {
            OutcomeRaw::Ok => Outcome::Ok,
            OutcomeRaw::Failed => Outcome::Failed,
            OutcomeRaw::Unknown | OutcomeRaw::Other => Outcome::Unknown,
        }
    }
}

/// What a run turned out to be, as [`finish`] takes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunResult {
    pub duration_ms: u64,
    /// Rows returned or affected; `None` for a failure or a statement that
    /// reports neither.
    pub rows: Option<u64>,
    pub ok: bool,
}

/// The persisted history file (`history.json`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct HistoryFile {
    #[serde(default)]
    pub entries: Vec<HistoryEntry>,
}

/// Record a newly-run query at the front (newest-first). Blank SQL is ignored.
///
/// **A statement carrying a credential is not recorded at all**
/// ([`sql::carries_credential`] — `CREATE USER … IDENTIFIED BY '…'` and its
/// relatives). `history.json` is plaintext, lives beside the `connections.json`
/// whose secrets `core::secrets` takes care to keep out of files, and travels
/// with whatever backs that directory up; the `mysql` CLI's default `histignore`
/// makes the same omission. The check lives here, in `push`, so a second call
/// site can't skip it. `dialect` is the connection's — comment and string
/// boundaries differ per engine.
///
/// De-duplicates: a prior identical query on the same connection is dropped so the
/// re-run bubbles to the top with a fresh timestamp instead of stacking copies.
/// Then the connection is trimmed to its newest [`MAX_PER_CONN`] entries (other
/// connections untouched).
pub fn push(entries: &mut Vec<HistoryEntry>, entry: HistoryEntry, dialect: SqlDialect) {
    if entry.sql.trim().is_empty() {
        return;
    }
    if sql::carries_credential(&entry.sql, dialect) {
        return;
    }
    let conn = entry.conn_id;
    // Drop any earlier identical query for this connection (exact SQL match).
    entries.retain(|e| !(e.conn_id == conn && e.sql == entry.sql));
    entries.insert(0, entry);
    // Per-connection cap: keep only the newest `MAX_PER_CONN` for this connection
    // (the vec is newest-first, so `retain` keeps the leading matches and drops the
    // trailing/oldest ones). Entries for other connections pass through untouched.
    let mut kept = 0usize;
    entries.retain(|e| {
        if e.conn_id == conn {
            kept += 1;
            kept <= MAX_PER_CONN
        } else {
            true
        }
    });
}

/// Record how a run turned out, onto the entry [`push`] wrote when it launched.
/// Returns whether one was found.
///
/// **A second pass, rather than pushing at completion**, because the two moments
/// answer different questions: an entry has to exist while the query is still
/// running — a run the user cancels, or that the app doesn't outlive, is one
/// they may most want to see again — and only the completion knows the outcome.
///
/// Finding the entry by `(conn_id, sql)` works because [`push`] de-duplicates on
/// exactly that pair, so a connection holds at most one entry per statement, and
/// the first (newest) match is it.
///
/// That key is also the limit of what this can promise: **the last completion
/// wins, which is not always the newest run.** Two tabs on one connection
/// running the same statement text share the one entry, and if the earlier run
/// is the slower one it lands second and overwrites — so an entry stamped at the
/// newer run's launch can end up reporting the older run's duration and outcome.
/// Within a tab it can't happen: a second run cancels the first, and a cancelled
/// run reports nothing. Recording a run id per entry would fix it, at the cost of
/// making the de-duplication that keeps this log short answer to two keys.
///
/// Nothing to update is normal, not an error — history may have been cleared, or
/// the statement never recorded at all because it carries a credential.
pub fn finish(entries: &mut [HistoryEntry], conn_id: u64, sql: &str, result: RunResult) -> bool {
    let Some(e) = entries
        .iter_mut()
        .find(|e| e.conn_id == conn_id && e.sql == sql)
    else {
        return false;
    };
    e.duration_ms = Some(result.duration_ms);
    e.rows = result.rows;
    e.outcome = if result.ok {
        Outcome::Ok
    } else {
        Outcome::Failed
    };
    true
}

/// Which recency group an entry belongs under in the panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bucket {
    Today,
    ThisWeek,
    Earlier,
}

impl Bucket {
    /// The group header's text.
    pub fn label(self) -> &'static str {
        match self {
            Bucket::Today => "TODAY",
            Bucket::ThisWeek => "THIS WEEK",
            Bucket::Earlier => "EARLIER",
        }
    }
}

/// Which group a run belongs to, from how long ago it was (both unix millis).
///
/// **Elapsed time, not calendar days** — the same arithmetic [`relative_time`]
/// uses, and for the same reason: there is no timezone here, only millis. It
/// also avoids the reading a calendar boundary would force, where a query run
/// twenty minutes ago moves out of "today" because midnight passed. The two must
/// agree, since a row's "3d ago" is read directly under its header;
/// `buckets_agree_with_the_relative_labels_they_sit_over` is the test that pins
/// them together.
pub fn bucket(ts: u64, now: u64) -> Bucket {
    let secs = now.saturating_sub(ts) / 1000;
    if secs < 86_400 {
        Bucket::Today
    } else if secs < 7 * 86_400 {
        Bucket::ThisWeek
    } else {
        Bucket::Earlier
    }
}

/// `entries` split into its recency groups, newest group first, each keeping the
/// order it arrived in. A group with nothing in it is left out entirely — a
/// header with no rows under it is noise.
///
/// Grouped by bucket rather than by *runs* of equal buckets, so a list that
/// isn't strictly newest-first (nothing guarantees it beyond `push`'s own
/// ordering) can't render the same header twice.
pub fn group_by_recency(entries: Vec<HistoryEntry>, now: u64) -> Vec<(Bucket, Vec<HistoryEntry>)> {
    let (mut today, mut week, mut earlier) = (Vec::new(), Vec::new(), Vec::new());
    for e in entries {
        match bucket(e.ts, now) {
            Bucket::Today => today.push(e),
            Bucket::ThisWeek => week.push(e),
            Bucket::Earlier => earlier.push(e),
        }
    }
    [
        (Bucket::Today, today),
        (Bucket::ThisWeek, week),
        (Bucket::Earlier, earlier),
    ]
    .into_iter()
    .filter(|(_, v)| !v.is_empty())
    .collect()
}

/// A run's duration in the unit that suits its size: `48ms`, `1.2s`, `2m 05s`.
///
/// Sub-second runs keep whole milliseconds (the difference between 8 and 80 is
/// the whole point at that scale), seconds get one decimal, and past a minute it
/// reads as minutes and seconds — `93.4s` makes the reader do arithmetic.
pub fn format_duration(ms: u64) -> String {
    if ms < 1_000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1_000.0)
    } else {
        format!("{}m {:02}s", ms / 60_000, (ms % 60_000) / 1_000)
    }
}

/// Drop every entry belonging to `conn_id` (the panel's "clear history", which is
/// scoped to the connection currently shown).
pub fn clear_conn(entries: &mut Vec<HistoryEntry>, conn_id: u64) {
    entries.retain(|e| e.conn_id != conn_id);
}

/// How many entries [`clear_conn`] would delete for `conn_id`.
///
/// This exists so the confirmation modal's count and the deletion itself can't
/// answer to different predicates. Clearing history is destructive and has no
/// undo, so "Delete 12 recorded queries for this connection?" is a promise about
/// what the next click does — and the panel used to make that promise with its
/// own inline filter, several files away from the `retain` that fulfils it.
/// `count_conn_agrees_with_clear_conn` is the test that keeps the pair honest.
///
/// The count is the connection's **total**: the panel's search box narrows the
/// list on screen, not the delete.
pub fn count_conn(entries: &[HistoryEntry], conn_id: u64) -> usize {
    entries.iter().filter(|e| e.conn_id == conn_id).count()
}

/// A compact single-line preview of a SQL statement: runs of whitespace
/// (including newlines) collapse to one space, so a multi-line statement reads as
/// one flowing line that the UI can wrap to a few rows.
pub fn preview(sql: &str) -> String {
    sql.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether a history entry matches a free-text filter (ASCII case-insensitive),
/// checking the SQL, the database name, and the originating tab name. An empty
/// (or whitespace-only) query matches everything. The SQL is matched against its
/// whitespace-collapsed [`preview`], so a multi-word query reads across the
/// statement's original newlines — matching what the panel shows.
pub fn matches_query(entry: &HistoryEntry, query: &str) -> bool {
    use crate::text_ops::contains_ignore_ascii_case;
    let q = query.trim();
    if q.is_empty() {
        return true;
    }
    contains_ignore_ascii_case(&preview(&entry.sql), q)
        || entry
            .database
            .as_deref()
            .is_some_and(|d| contains_ignore_ascii_case(d, q))
        || entry
            .tab_name
            .as_deref()
            .is_some_and(|t| contains_ignore_ascii_case(t, q))
}

/// Human "time ago" for a history timestamp, given the current time (both unix
/// millis). Coarse buckets — seconds / minutes / hours / days / weeks.
pub fn relative_time(ts: u64, now: u64) -> String {
    let secs = now.saturating_sub(ts) / 1000;
    if secs < 60 {
        "just now".to_string()
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86_400 {
        format!("{}h ago", secs / 3600)
    } else if secs < 7 * 86_400 {
        format!("{}d ago", secs / 86_400)
    } else {
        format!("{}w ago", secs / (7 * 86_400))
    }
}

#[cfg(test)]
mod tests {
    use super::push as push_with;
    use super::*;

    fn entry(conn_id: u64, sql: &str, ts: u64) -> HistoryEntry {
        HistoryEntry {
            conn_id,
            database: Some("db".to_string()),
            sql: sql.to_string(),
            ts,
            tab_name: None,
            duration_ms: None,
            rows: None,
            outcome: Outcome::Unknown,
        }
    }

    fn ok(duration_ms: u64, rows: u64) -> RunResult {
        RunResult {
            duration_ms,
            rows: Some(rows),
            ok: true,
        }
    }

    #[test]
    fn finish_fills_in_what_the_run_turned_out_to_be() {
        let mut v = Vec::new();
        push(&mut v, entry(1, "SELECT 1", 100));
        assert!(finish(&mut v, 1, "SELECT 1", ok(48, 150)));
        assert_eq!(v[0].duration_ms, Some(48));
        assert_eq!(v[0].rows, Some(150));
        assert_eq!(v[0].outcome, Outcome::Ok);
    }

    /// A failure still took time — the 50-second lock wait is exactly the run
    /// worth finding later — but it returned nothing.
    #[test]
    fn a_failed_run_keeps_its_duration_and_reports_no_rows() {
        let mut v = Vec::new();
        push(&mut v, entry(1, "SELECT 1", 100));
        finish(
            &mut v,
            1,
            "SELECT 1",
            RunResult {
                duration_ms: 50_000,
                rows: None,
                ok: false,
            },
        );
        assert_eq!(v[0].duration_ms, Some(50_000));
        assert_eq!(v[0].rows, None);
        assert_eq!(v[0].outcome, Outcome::Failed);
    }

    /// Scoped to the connection, like everything else here: the same statement
    /// text is routinely run against two servers.
    #[test]
    fn finish_leaves_another_connections_identical_query_alone() {
        let mut v = Vec::new();
        push(&mut v, entry(2, "SELECT 1", 100));
        push(&mut v, entry(1, "SELECT 1", 200));
        finish(&mut v, 1, "SELECT 1", ok(48, 150));
        let other = v.iter().find(|e| e.conn_id == 2).unwrap();
        assert_eq!(other.outcome, Outcome::Unknown);
        assert_eq!(other.duration_ms, None);
    }

    /// The entry can be gone by the time the run lands — history cleared, or the
    /// statement never recorded at all because it carries a credential. Nothing
    /// to update is not an error.
    #[test]
    fn finish_is_a_no_op_when_the_entry_is_gone() {
        let mut v = Vec::new();
        push(&mut v, entry(1, "SELECT 1", 100));
        assert!(!finish(&mut v, 1, "SELECT 2", ok(1, 1)));
        assert!(!finish(&mut Vec::new(), 1, "SELECT 1", ok(1, 1)));
    }

    /// Only the newest match. `push` de-dupes, so a second one exists only
    /// transiently — but if it does, the run that just finished is the recent one.
    #[test]
    fn finish_updates_only_the_newest_match() {
        let mut v = vec![entry(1, "SELECT 1", 200), entry(1, "SELECT 1", 100)];
        finish(&mut v, 1, "SELECT 1", ok(48, 150));
        assert_eq!(v[0].outcome, Outcome::Ok);
        assert_eq!(v[1].outcome, Outcome::Unknown);
    }

    /// `history.json` predates these three fields, and a file that fails to
    /// parse loses every recorded query.
    #[test]
    fn an_entry_written_before_outcomes_still_loads() {
        let json = r#"{"conn_id":1,"database":"db","sql":"SELECT 1","ts":5}"#;
        let e: HistoryEntry = serde_json::from_str(json).unwrap();
        assert_eq!(e.sql, "SELECT 1");
        assert_eq!(e.duration_ms, None);
        assert_eq!(e.rows, None);
        assert_eq!(e.outcome, Outcome::Unknown);
    }

    /// An outcome a newer build wrote degrades to "unknown" rather than failing
    /// the whole file — the rule every persisted enum here follows.
    #[test]
    fn an_unknown_outcome_degrades_instead_of_failing_the_file() {
        let json = r#"{"conn_id":1,"sql":"SELECT 1","ts":5,"outcome":"Rolledback"}"#;
        let e: HistoryEntry = serde_json::from_str(json).unwrap();
        assert_eq!(e.outcome, Outcome::Unknown);
    }

    const HOUR: u64 = 3_600_000;
    const DAY: u64 = 24 * HOUR;

    #[test]
    fn recency_buckets_split_at_a_day_and_a_week() {
        let now = 100 * DAY;
        assert_eq!(bucket(now, now), Bucket::Today);
        assert_eq!(bucket(now - 23 * HOUR, now), Bucket::Today);
        // The boundaries themselves belong to the *older* group, so a bucket
        // never claims something its label doesn't cover.
        assert_eq!(bucket(now - DAY, now), Bucket::ThisWeek);
        assert_eq!(bucket(now - 6 * DAY, now), Bucket::ThisWeek);
        assert_eq!(bucket(now - 7 * DAY, now), Bucket::Earlier);
        assert_eq!(bucket(0, now), Bucket::Earlier);
    }

    /// A clock that went backwards (or an entry written on a machine slightly
    /// ahead) reads as most recent rather than underflowing into "earlier".
    #[test]
    fn a_future_timestamp_is_today() {
        assert_eq!(bucket(500, 100), Bucket::Today);
    }

    /// The header and the row beneath it are read together, so they must not be
    /// able to disagree: nothing labelled "Nd ago" with N < 7 may sit under
    /// EARLIER, and nothing labelled in hours may sit outside TODAY.
    #[test]
    fn buckets_agree_with_the_relative_labels_they_sit_over() {
        let now = 100 * DAY;
        for days in 0..14u64 {
            let ts = now - days * DAY;
            let label = relative_time(ts, now);
            match bucket(ts, now) {
                Bucket::Today => assert!(!label.ends_with("d ago"), "{label}"),
                Bucket::ThisWeek => assert!(label.ends_with("d ago"), "{label}"),
                Bucket::Earlier => assert!(
                    label.ends_with("w ago") || label.ends_with("d ago"),
                    "{label}"
                ),
            }
        }
    }

    #[test]
    fn grouping_keeps_the_buckets_in_order_and_drops_the_empty_ones() {
        let now = 100 * DAY;
        let v = vec![
            entry(1, "a", now - HOUR),
            entry(1, "b", now - 3 * DAY),
            entry(1, "c", now - 9 * DAY),
            entry(1, "d", now - 30 * DAY),
        ];
        let groups = group_by_recency(v, now);
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].0, Bucket::Today);
        assert_eq!(groups[1].0, Bucket::ThisWeek);
        assert_eq!(groups[2].0, Bucket::Earlier);
        // Order within a group is the order it arrived in (newest-first).
        assert_eq!(groups[2].1.len(), 2);
        assert_eq!(groups[2].1[0].sql, "c");
        assert_eq!(groups[2].1[1].sql, "d");
        // A bucket with nothing in it isn't a header with no rows under it.
        let only_old = group_by_recency(vec![entry(1, "x", now - 30 * DAY)], now);
        assert_eq!(only_old.len(), 1);
        assert_eq!(only_old[0].0, Bucket::Earlier);
        assert!(group_by_recency(Vec::new(), now).is_empty());
    }

    /// Grouping is by bucket, not by adjacency, so an out-of-order list can't
    /// produce the same header twice.
    #[test]
    fn grouping_cannot_repeat_a_header() {
        let now = 100 * DAY;
        let v = vec![
            entry(1, "old", now - 30 * DAY),
            entry(1, "new", now - HOUR),
            entry(1, "older", now - 40 * DAY),
        ];
        let groups = group_by_recency(v, now);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].0, Bucket::Today);
        assert_eq!(groups[1].1.len(), 2);
    }

    #[test]
    fn durations_read_in_the_unit_that_suits_them() {
        assert_eq!(format_duration(0), "0ms");
        assert_eq!(format_duration(48), "48ms");
        assert_eq!(format_duration(999), "999ms");
        assert_eq!(format_duration(1_000), "1.0s");
        assert_eq!(format_duration(1_240), "1.2s");
        assert_eq!(format_duration(59_900), "59.9s");
        assert_eq!(format_duration(60_000), "1m 00s");
        assert_eq!(format_duration(3_723_000), "62m 03s");
    }

    /// The tests that aren't about the dialect record as MySQL.
    fn push(entries: &mut Vec<HistoryEntry>, e: HistoryEntry) {
        push_with(entries, e, SqlDialect::MySql);
    }

    #[test]
    fn push_prepends_newest_first() {
        let mut v = Vec::new();
        push(&mut v, entry(1, "SELECT 1", 100));
        push(&mut v, entry(1, "SELECT 2", 200));
        assert_eq!(v[0].sql, "SELECT 2");
        assert_eq!(v[1].sql, "SELECT 1");
    }

    #[test]
    fn push_ignores_blank() {
        let mut v = Vec::new();
        push(&mut v, entry(1, "   \n  ", 100));
        assert!(v.is_empty());
    }

    #[test]
    fn push_caps_per_connection_and_keeps_other_conns() {
        let mut v = Vec::new();
        // Distinct SQL each time (else dedup collapses them), across MAX+10 runs.
        for i in 0..(MAX_PER_CONN + 10) {
            push(&mut v, entry(1, &format!("SELECT {i}"), i as u64));
        }
        // A different connection's single entry must survive the conn-1 trim.
        push(&mut v, entry(2, "SELECT other", 9999));

        let conn1 = v.iter().filter(|e| e.conn_id == 1).count();
        assert_eq!(conn1, MAX_PER_CONN);
        assert_eq!(v.iter().filter(|e| e.conn_id == 2).count(), 1);
        // Newest conn-1 query kept, oldest dropped.
        assert!(
            v.iter()
                .any(|e| e.sql == format!("SELECT {}", MAX_PER_CONN + 9))
        );
        assert!(!v.iter().any(|e| e.sql == "SELECT 0"));
    }

    #[test]
    fn push_dedups_same_query_and_bubbles_to_top() {
        let mut v = Vec::new();
        push(&mut v, entry(1, "SELECT 1", 100));
        push(&mut v, entry(1, "SELECT 2", 200));
        // Re-run the first query: one copy, now at the top with the fresh ts.
        push(&mut v, entry(1, "SELECT 1", 300));
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].sql, "SELECT 1");
        assert_eq!(v[0].ts, 300);
        assert_eq!(v[1].sql, "SELECT 2");
    }

    #[test]
    fn dedup_is_per_connection() {
        // The same SQL on two different connections stays as two entries.
        let mut v = Vec::new();
        push(&mut v, entry(1, "SELECT 1", 100));
        push(&mut v, entry(2, "SELECT 1", 200));
        assert_eq!(v.len(), 2);
    }

    // ── Credential statements are never recorded ──────────────────────────
    //
    // `history.json` is plaintext and sits beside the `connections.json` whose
    // secrets the product takes care to keep out of files. The `mysql` CLI's
    // default `histignore` makes the same omission.

    #[test]
    fn credential_ddl_is_not_recorded() {
        for sql in [
            "CREATE USER 'app'@'%' IDENTIFIED BY 'hunter2'",
            "ALTER USER 'app'@'%' IDENTIFIED BY 'hunter2'",
            "GRANT ALL ON *.* TO 'app'@'%' IDENTIFIED BY 'hunter2'",
            "SET PASSWORD FOR 'app'@'%' = 'hunter2'",
            "CREATE ROLE app WITH LOGIN PASSWORD 'hunter2'",
            "ALTER ROLE app WITH PASSWORD 'hunter2'",
            // A trailing semicolon, and a credential statement in a batch.
            "CREATE USER 'a' IDENTIFIED BY 'p';",
            "SELECT 1; SET PASSWORD = 'p'",
        ] {
            let mut v = Vec::new();
            push(&mut v, entry(1, sql, 100));
            assert!(v.is_empty(), "should not be recorded: {sql}");
        }
    }

    #[test]
    fn an_ordinary_statement_naming_a_password_column_is_still_recorded() {
        // Omitting is the safe direction, but not at the cost of dropping every
        // query someone writes against a users table.
        for sql in [
            "SELECT password FROM users",
            "UPDATE users SET last_login = NOW() WHERE id = 1",
            "ALTER TABLE users ADD COLUMN password varchar(64)",
            "CREATE TABLE users (id int, password varchar(64))",
            // The words only inside a string or a comment — the tokenizer's job.
            "SELECT 'IDENTIFIED BY' AS note",
            "SELECT 1 -- IDENTIFIED BY 'x'",
        ] {
            let mut v = Vec::new();
            push(&mut v, entry(1, sql, 100));
            assert_eq!(v.len(), 1, "should be recorded: {sql}");
        }
    }

    #[test]
    fn clear_conn_only_removes_that_connection() {
        let mut v = vec![entry(1, "a", 1), entry(2, "b", 2), entry(1, "c", 3)];
        clear_conn(&mut v, 1);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].conn_id, 2);
    }

    #[test]
    fn count_conn_counts_only_that_connection() {
        let v = vec![entry(1, "a", 1), entry(2, "b", 2), entry(1, "c", 3)];
        assert_eq!(count_conn(&v, 1), 2);
        assert_eq!(count_conn(&v, 2), 1);
        // A connection with no history: the panel's trash is inert here.
        assert_eq!(count_conn(&v, 3), 0);
        assert_eq!(count_conn(&[], 1), 0);
    }

    /// The confirmation modal names a number and the next click deletes; if the
    /// two ever answered to different predicates the modal would be lying about
    /// a destructive action with no undo.
    #[test]
    fn count_conn_agrees_with_clear_conn() {
        let base = vec![
            entry(1, "a", 1),
            entry(2, "b", 2),
            entry(1, "c", 3),
            entry(3, "d", 4),
            entry(1, "e", 5),
        ];
        for conn in [1, 2, 3, 99] {
            let mut v = base.clone();
            let promised = count_conn(&v, conn);
            let before = v.len();
            clear_conn(&mut v, conn);
            assert_eq!(
                before - v.len(),
                promised,
                "conn {conn}: modal promised {promised} deletions"
            );
            assert_eq!(count_conn(&v, conn), 0, "conn {conn}: nothing left over");
        }
    }

    #[test]
    fn matches_filters_by_sql_database_and_tab_name() {
        let mut e = entry(1, "SELECT * FROM film", 100);
        e.database = Some("sakila".to_string());
        e.tab_name = Some("My Films".to_string());
        // Empty / whitespace-only query matches everything.
        assert!(matches_query(&e, ""));
        assert!(matches_query(&e, "   "));
        // Case-insensitive substring of the SQL.
        assert!(matches_query(&e, "film"));
        assert!(matches_query(&e, "FILM"));
        // Database name and tab name.
        assert!(matches_query(&e, "sakila"));
        assert!(matches_query(&e, "my films"));
        // No match anywhere.
        assert!(!matches_query(&e, "zzz"));
        // Matches against the whitespace-collapsed preview, so a multi-word query
        // spans the original newline.
        let ml = entry(1, "SELECT *\n  FROM   film", 100);
        assert!(matches_query(&ml, "from film"));
        // A None tab_name doesn't match (and doesn't panic).
        let bare = entry(1, "SELECT 1", 100); // tab_name: None
        assert!(!matches_query(&bare, "nope"));
    }

    #[test]
    fn preview_collapses_whitespace() {
        assert_eq!(
            preview("SELECT *\n  FROM   film\nWHERE id = 1"),
            "SELECT * FROM film WHERE id = 1"
        );
    }

    #[test]
    fn relative_time_buckets() {
        let now = 10_000_000_000;
        assert_eq!(relative_time(now, now), "just now");
        assert_eq!(relative_time(now - 30_000, now), "just now");
        assert_eq!(relative_time(now - 5 * 60_000, now), "5m ago");
        assert_eq!(relative_time(now - 3 * 3_600_000, now), "3h ago");
        assert_eq!(relative_time(now - 2 * 86_400_000, now), "2d ago");
        assert_eq!(relative_time(now - 14 * 86_400_000, now), "2w ago");
    }
}
