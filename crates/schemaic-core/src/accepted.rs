//! Completion items the user has accepted, per connection — a small persisted
//! store (`accepted.json`) that ranks the suggestion list by how often and how
//! recently each word was taken ([`AcceptedFile::bonuses`], read by
//! [`crate::rank`]).
//!
//! Keyed by the lower-cased text alone: the list a caret offers already
//! decides *what* a word is there, and this only says how much the user has
//! wanted it before. Bounded per connection ([`PER_CONN`]) by forgetting the
//! longest-unused words, so the file stays small however long it is used.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// How many words one connection keeps.
pub const PER_CONN: usize = 500;

/// The largest bonus a word can earn — under a prefix match's 40, so a word
/// that matches what is typed better still wins, and about the in-statement
/// recency bonus's 18 ([`crate::rank::recency_bonus`]).
pub const MAX_BONUS: i32 = 16;

/// One accepted word: how many times, and when last (Unix seconds).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptedRule {
    pub conn_id: u64,
    /// Lower-cased.
    pub text: String,
    pub count: u32,
    pub last: u64,
}

/// The persisted store (`accepted.json`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AcceptedFile {
    #[serde(default)]
    pub rules: Vec<AcceptedRule>,
}

impl AcceptedFile {
    /// `text` was accepted on `conn_id` at `now`. Past [`PER_CONN`] words for
    /// the connection, the one unused longest goes.
    pub fn record(&mut self, conn_id: u64, text: &str, now: u64) {
        let key = text.to_lowercase();
        if key.is_empty() {
            return;
        }
        match self
            .rules
            .iter_mut()
            .find(|r| r.conn_id == conn_id && r.text == key)
        {
            Some(r) => {
                r.count = r.count.saturating_add(1);
                r.last = r.last.max(now);
            }
            None => self.rules.push(AcceptedRule {
                conn_id,
                text: key,
                count: 1,
                last: now,
            }),
        }
        let mine = self.rules.iter().filter(|r| r.conn_id == conn_id).count();
        if mine > PER_CONN
            && let Some(oldest) = self
                .rules
                .iter()
                .enumerate()
                .filter(|(_, r)| r.conn_id == conn_id)
                .min_by_key(|(_, r)| r.last)
                .map(|(i, _)| i)
        {
            self.rules.remove(oldest);
        }
    }

    /// Each of `conn_id`'s words with the ranking bonus it has earned at
    /// `now` ([`bonus`]), keyed lower-cased; words that earn none are left out.
    pub fn bonuses(&self, conn_id: u64, now: u64) -> HashMap<String, i32> {
        self.rules
            .iter()
            .filter(|r| r.conn_id == conn_id)
            .filter_map(|r| {
                let b = bonus(r.count, now.saturating_sub(r.last));
                (b > 0).then(|| (r.text.clone(), b))
            })
            .collect()
    }
}

/// Forget every word accepted on `conn_id` — the connection was deleted, and
/// nothing keyed to it should outlive it.
pub fn clear_conn(rules: &mut Vec<AcceptedRule>, conn_id: u64) {
    rules.retain(|r| r.conn_id != conn_id);
}

/// The bonus a word accepted `count` times, last `age` seconds ago, earns:
/// frequency in steps (4 for once, up to 12), halved past a week and quartered
/// past a month, plus a little for the last hour (4) or day (2). Capped at
/// [`MAX_BONUS`].
pub fn bonus(count: u32, age: u64) -> i32 {
    const HOUR: u64 = 3600;
    const DAY: u64 = 24 * HOUR;
    let freq = match count {
        0 => return 0,
        1 => 4,
        2..=3 => 7,
        4..=9 => 10,
        _ => 12,
    };
    let aged = if age < 7 * DAY {
        freq
    } else if age < 30 * DAY {
        freq / 2
    } else {
        freq / 4
    };
    let fresh = if age < HOUR {
        4
    } else if age < DAY {
        2
    } else {
        0
    };
    (aged + fresh).min(MAX_BONUS)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 24 * 3600;

    #[test]
    fn a_word_accepted_more_often_or_more_lately_earns_more() {
        assert_eq!(bonus(0, 0), 0);
        assert!(bonus(2, DAY) > bonus(1, DAY));
        assert!(bonus(10, DAY) > bonus(4, DAY));
        assert!(bonus(3, 60) > bonus(3, 2 * DAY));
        assert!(bonus(3, 2 * DAY) > bonus(3, 10 * DAY));
        assert!(bonus(3, 10 * DAY) > bonus(3, 40 * DAY));
        // However much, never past the cap.
        assert_eq!(bonus(u32::MAX, 0), MAX_BONUS);
    }

    #[test]
    fn recording_counts_a_word_per_connection_whatever_its_case() {
        let mut f = AcceptedFile::default();
        f.record(1, "Orders", 100);
        f.record(1, "orders", 200);
        f.record(2, "orders", 50);
        f.record(1, "", 300);
        let mine = f.bonuses(1, 200);
        assert_eq!(mine.get("orders"), Some(&bonus(2, 0)));
        assert_eq!(mine.len(), 1);
        assert_eq!(f.bonuses(2, 200).get("orders"), Some(&bonus(1, 150)));
        assert!(f.bonuses(3, 200).is_empty());
    }

    #[test]
    fn a_full_connection_forgets_its_longest_unused_word_only() {
        let mut f = AcceptedFile::default();
        f.record(9, "other", 0);
        for i in 0..PER_CONN as u64 {
            f.record(1, &format!("w{i}"), 10 + i);
        }
        f.record(1, "new", 10_000);
        let mine = f.bonuses(1, 10_000);
        assert_eq!(f.rules.iter().filter(|r| r.conn_id == 1).count(), PER_CONN);
        assert!(!mine.contains_key("w0"));
        assert!(mine.contains_key("w1") && mine.contains_key("new"));
        // Another connection's words are not this one's to evict.
        assert!(f.rules.iter().any(|r| r.conn_id == 9));
    }

    #[test]
    fn clearing_a_connection_leaves_the_others() {
        let mut f = AcceptedFile::default();
        f.record(1, "a", 0);
        f.record(2, "b", 0);
        clear_conn(&mut f.rules, 1);
        assert_eq!(f.rules.len(), 1);
        assert_eq!(f.rules[0].conn_id, 2);
    }
}
