//! Query history: a persisted, newest-first log of executed statements, scoped
//! per connection in the UI.
//!
//! Each run records the connection id, the database it ran against, the SQL, and
//! a wall-clock timestamp (unix millis). The store is a flat `Vec` capped at
//! [`MAX_ENTRIES`] (oldest dropped); the UI filters it to the active connection
//! and renders newest-first. Pure + tested here; the app owns the signal and
//! persists the list via `persist::save_json`.

use serde::{Deserialize, Serialize};

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
}

/// The persisted history file (`history.json`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct HistoryFile {
    #[serde(default)]
    pub entries: Vec<HistoryEntry>,
}

/// Record a newly-run query at the front (newest-first). Blank SQL is ignored.
///
/// De-duplicates: a prior identical query on the same connection is dropped so the
/// re-run bubbles to the top with a fresh timestamp instead of stacking copies.
/// Then the connection is trimmed to its newest [`MAX_PER_CONN`] entries (other
/// connections untouched).
pub fn push(entries: &mut Vec<HistoryEntry>, entry: HistoryEntry) {
    if entry.sql.trim().is_empty() {
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

/// Drop every entry belonging to `conn_id` (the panel's "clear history", which is
/// scoped to the connection currently shown).
pub fn clear_conn(entries: &mut Vec<HistoryEntry>, conn_id: u64) {
    entries.retain(|e| e.conn_id != conn_id);
}

/// A compact single-line preview of a SQL statement: runs of whitespace
/// (including newlines) collapse to one space, so a multi-line statement reads as
/// one flowing line that the UI can wrap to a few rows.
pub fn preview(sql: &str) -> String {
    sql.split_whitespace().collect::<Vec<_>>().join(" ")
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
    use super::*;

    fn entry(conn_id: u64, sql: &str, ts: u64) -> HistoryEntry {
        HistoryEntry {
            conn_id,
            database: Some("db".to_string()),
            sql: sql.to_string(),
            ts,
            tab_name: None,
        }
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

    #[test]
    fn clear_conn_only_removes_that_connection() {
        let mut v = vec![entry(1, "a", 1), entry(2, "b", 2), entry(1, "c", 3)];
        clear_conn(&mut v, 1);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].conn_id, 2);
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
