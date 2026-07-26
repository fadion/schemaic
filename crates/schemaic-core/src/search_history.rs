//! Per-connection **search history** for the Find-Anywhere palette — the pure,
//! testable model. Mirrors [`crate::history`] (query history): a flat
//! `Vec<SearchEntry>` keyed by a `conn_id` field, newest-first, deduped, and
//! capped per connection. Persisted to `search_history.json`.
//!
//! An entry is recorded only when a result is **activated** (clicked / Enter),
//! not merely surfaced by a search. The palette shows the recent entries for the
//! active connection when it opens with an empty query.

use serde::{Deserialize, Serialize};

/// How many entries to keep (and show) per connection.
pub const MAX_PER_CONN: usize = 10;

/// One activated search result: a table (`column: None`) or a specific column.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchEntry {
    pub conn_id: u64,
    pub database: String,
    pub table: String,
    #[serde(default)]
    pub column: Option<String>,
}

/// Persisted shape: `{ "entries": [...] }` (matches `HistoryFile`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SearchHistoryFile {
    #[serde(default)]
    pub entries: Vec<SearchEntry>,
}

impl SearchEntry {
    /// Same target (ignoring `conn_id`) — used for dedup within a connection.
    fn same_target(&self, other: &SearchEntry) -> bool {
        self.conn_id == other.conn_id
            && self.database == other.database
            && self.table == other.table
            && self.column == other.column
    }
}

/// Record an activated result: drop any prior identical entry (so re-selecting
/// bubbles it to the top with the latest position), insert at the front, then cap
/// **this** connection's entries at [`MAX_PER_CONN`] (other connections untouched).
pub fn push(entries: &mut Vec<SearchEntry>, entry: SearchEntry) {
    entries.retain(|e| !e.same_target(&entry));
    let conn = entry.conn_id;
    entries.insert(0, entry);
    // Cap only this connection's entries; leave others in place.
    let mut kept = 0;
    entries.retain(|e| {
        if e.conn_id != conn {
            return true;
        }
        kept += 1;
        kept <= MAX_PER_CONN
    });
}

/// The recent entries for `conn`, newest-first, up to [`MAX_PER_CONN`].
pub fn recent(entries: &[SearchEntry], conn: u64) -> Vec<SearchEntry> {
    entries
        .iter()
        .filter(|e| e.conn_id == conn)
        .take(MAX_PER_CONN)
        .cloned()
        .collect()
}

/// Forget every entry for one connection.
pub fn clear_conn(entries: &mut Vec<SearchEntry>, conn_id: u64) {
    entries.retain(|e| e.conn_id != conn_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(conn: u64, table: &str, column: Option<&str>) -> SearchEntry {
        SearchEntry {
            conn_id: conn,
            database: "db".into(),
            table: table.into(),
            column: column.map(|c| c.into()),
        }
    }

    #[test]
    fn push_inserts_newest_first() {
        let mut v = Vec::new();
        push(&mut v, entry(1, "a", None));
        push(&mut v, entry(1, "b", None));
        assert_eq!(
            recent(&v, 1),
            vec![entry(1, "b", None), entry(1, "a", None)]
        );
    }

    #[test]
    fn push_dedups_and_bubbles_to_top() {
        let mut v = Vec::new();
        push(&mut v, entry(1, "a", None));
        push(&mut v, entry(1, "b", None));
        push(&mut v, entry(1, "a", None)); // re-select "a"
        assert_eq!(
            recent(&v, 1),
            vec![entry(1, "a", None), entry(1, "b", None)]
        );
        assert_eq!(v.len(), 2); // no duplicate
    }

    #[test]
    fn table_and_column_are_distinct_targets() {
        let mut v = Vec::new();
        push(&mut v, entry(1, "users", None));
        push(&mut v, entry(1, "users", Some("email")));
        assert_eq!(recent(&v, 1).len(), 2);
    }

    #[test]
    fn push_caps_per_connection() {
        let mut v = Vec::new();
        for i in 0..15 {
            push(&mut v, entry(1, &format!("t{i}"), None));
        }
        let r = recent(&v, 1);
        assert_eq!(r.len(), MAX_PER_CONN);
        assert_eq!(r[0], entry(1, "t14", None)); // newest kept
        assert_eq!(r[MAX_PER_CONN - 1], entry(1, "t5", None)); // t0..t4 dropped
    }

    #[test]
    fn cap_is_per_connection_not_global() {
        let mut v = Vec::new();
        for i in 0..15 {
            push(&mut v, entry(1, &format!("t{i}"), None));
        }
        push(&mut v, entry(2, "other", None));
        assert_eq!(recent(&v, 1).len(), MAX_PER_CONN); // conn 1 still capped
        assert_eq!(recent(&v, 2), vec![entry(2, "other", None)]); // conn 2 kept
    }

    #[test]
    fn recent_filters_by_connection() {
        let mut v = Vec::new();
        push(&mut v, entry(1, "a", None));
        push(&mut v, entry(2, "b", None));
        assert_eq!(recent(&v, 1), vec![entry(1, "a", None)]);
        assert_eq!(recent(&v, 2), vec![entry(2, "b", None)]);
        assert!(recent(&v, 3).is_empty());
    }

    #[test]
    fn clear_conn_only_clears_that_connection() {
        let mut v = Vec::new();
        push(&mut v, entry(1, "a", None));
        push(&mut v, entry(2, "b", None));
        clear_conn(&mut v, 1);
        assert!(recent(&v, 1).is_empty());
        assert_eq!(recent(&v, 2), vec![entry(2, "b", None)]);
    }
}
