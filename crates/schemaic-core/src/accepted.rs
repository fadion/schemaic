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

/// [`AcceptedFile::bonuses`], kept between keystrokes.
///
/// Completion asks for the map on every recompute, which runs undebounced, and
/// building it walks every rule of every connection and clones each word — up
/// to [`PER_CONN`] allocations and inserts a keystroke — while it changes only
/// when a word is accepted or forgotten, or when a word's age crosses one of
/// [`bonus`]'s steps. So the map is rebuilt only then: when the connection's
/// rules differ from the ones it was built from (compared, not cloned), or
/// `now` has left the span in which no bonus can change.
#[derive(Debug, Default)]
pub struct BonusCache {
    built: Option<Built>,
}

#[derive(Debug)]
struct Built {
    conn_id: u64,
    /// The connection's rules the map was built from, in file order.
    rules: Vec<AcceptedRule>,
    /// `now` when built, and the first second after it any bonus changes.
    at: u64,
    until: u64,
    map: HashMap<String, i32>,
}

impl BonusCache {
    /// `file.bonuses(conn_id, now)`, from the last build where it still holds.
    pub fn bonuses(
        &mut self,
        file: &AcceptedFile,
        conn_id: u64,
        now: u64,
    ) -> &HashMap<String, i32> {
        let mine = || file.rules.iter().filter(move |r| r.conn_id == conn_id);
        let holds = self.built.as_ref().is_some_and(|b| {
            b.conn_id == conn_id && b.at <= now && now < b.until && mine().eq(b.rules.iter())
        });
        if !holds {
            self.built = Some(Built {
                conn_id,
                rules: mine().cloned().collect(),
                at: now,
                until: mine()
                    .filter_map(|r| next_step(r.last, now))
                    .min()
                    .unwrap_or(u64::MAX),
                map: file.bonuses(conn_id, now),
            });
        }
        &self.built.as_ref().expect("built above").map
    }
}

/// The ages at which [`bonus`] changes its answer, in seconds.
const STEPS: [u64; 4] = [3600, 24 * 3600, 7 * 24 * 3600, 30 * 24 * 3600];

/// The first second after `now` at which a word last accepted at `last` earns
/// a different bonus, or `None` once it is past every step.
fn next_step(last: u64, now: u64) -> Option<u64> {
    let age = now.saturating_sub(last);
    STEPS
        .iter()
        .find(|&&s| s > age)
        .map(|s| last.saturating_add(*s))
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
    // Every age this answer turns on is one of `STEPS`, which `BonusCache`
    // reads to know when a bonus can next change.
    let [hour, day, week, month] = STEPS;
    let freq = match count {
        0 => return 0,
        1 => 4,
        2..=3 => 7,
        4..=9 => 10,
        _ => 12,
    };
    let aged = if age < week {
        freq
    } else if age < month {
        freq / 2
    } else {
        freq / 4
    };
    let fresh = if age < hour {
        4
    } else if age < day {
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

    /// **The cached map is always the fresh one**: across an accept, a
    /// forgotten connection, another connection's change, and every step of
    /// a word's age — and a keystroke that changes none of them reuses it.
    #[test]
    fn the_cached_bonuses_are_always_the_fresh_ones() {
        let mut f = AcceptedFile::default();
        let mut cache = BonusCache::default();
        let mut check = |f: &AcceptedFile, conn: u64, now: u64| {
            assert_eq!(cache.bonuses(f, conn, now), &f.bonuses(conn, now), "{now}");
        };
        f.record(1, "orders", 1_000);
        f.record(1, "orders", 1_000);
        f.record(2, "users", 1_000);
        check(&f, 1, 1_000);
        // Every step of the word's age, both sides of it.
        for step in STEPS {
            check(&f, 1, 1_000 + step - 1);
            check(&f, 1, 1_000 + step);
        }
        check(&f, 1, 1_000 + 40 * DAY);
        // An accept, on this connection and on another.
        f.record(1, "customers", 2_000);
        check(&f, 1, 2_000);
        f.record(2, "users", 2_001);
        check(&f, 1, 2_001);
        check(&f, 2, 2_001);
        clear_conn(&mut f.rules, 1);
        check(&f, 1, 2_002);
        // The clock going back is not "nothing changed".
        f.record(1, "orders", 5_000);
        check(&f, 1, 5_000);
        check(&f, 1, 4_000);
    }

    #[test]
    fn a_keystroke_that_changes_nothing_reuses_the_map() {
        let mut f = AcceptedFile::default();
        f.record(1, "orders", 1_000);
        let mut cache = BonusCache::default();
        cache.bonuses(&f, 1, 1_000);
        cache.bonuses(&f, 1, 1_010);
        assert_eq!(cache.built.as_ref().map(|b| b.at), Some(1_000));
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
