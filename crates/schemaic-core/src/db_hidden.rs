//! Databases the user has put away with the SCHEMA panel's eye — a small
//! persisted store of `(connection, database)` keys, mirroring the favorites
//! (`favorite.rs`) and colour (`db_color.rs`) stores it shares a context menu
//! with.
//!
//! **It is keyed by connection because the same name means different databases
//! on different servers.** The set used to be a flat `Vec<String>` of bare
//! names, so hiding `world` on a MariaDB connection also hid PostgreSQL's live
//! `world` — gone from the tree, from Find-Anywhere, from the toolbar's
//! selector, from autocomplete, from what the assistant is told exists, and from
//! `list_schema`'s overview, with nothing on the second connection saying why.
//! Its two siblings on the very same menu were keyed correctly all along; this
//! one was the exception.
//!
//! Deleting a connection takes its rules with it ([`clear_conn`]), which the
//! flat set could not do at all: a deleted connection's hidden names stayed in
//! `ui_state.json` forever, hiding same-named databases on connections created
//! later.
//!
//! The runtime shape every consumer reads is still a `HashSet<String>` — the
//! names hidden **for the connection being looked at** ([`names_for`]) — because
//! that is the question every surface is asking, and it keeps
//! [`crate::schema::db_visible`] a two-argument predicate.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

/// A hidden database, keyed by the connection + database name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DbHiddenRule {
    pub conn_id: u64,
    pub database: String,
}

/// Forget every hidden-database rule belonging to `conn_id` — the connection was
/// deleted, and nothing keyed to it should outlive it.
pub fn clear_conn(rules: &mut Vec<DbHiddenRule>, conn_id: u64) {
    rules.retain(|r| r.conn_id != conn_id);
}

/// Whether `(conn_id, database)` is hidden.
pub fn is_hidden(rules: &[DbHiddenRule], conn_id: u64, database: &str) -> bool {
    rules
        .iter()
        .any(|r| r.conn_id == conn_id && r.database == database)
}

/// Toggle a database's hidden state. Returns the new state.
pub fn toggle(rules: &mut Vec<DbHiddenRule>, conn_id: u64, database: &str) -> bool {
    if is_hidden(rules, conn_id, database) {
        rules.retain(|r| !(r.conn_id == conn_id && r.database == database));
        false
    } else {
        rules.push(DbHiddenRule {
            conn_id,
            database: database.to_string(),
        });
        true
    }
}

/// The names hidden **on one connection** — the set every consumer reads.
pub fn names_for(rules: &[DbHiddenRule], conn_id: u64) -> HashSet<String> {
    rules
        .iter()
        .filter(|r| r.conn_id == conn_id)
        .map(|r| r.database.clone())
        .collect()
}

/// Read a legacy flat list of bare names as rules.
///
/// **Applied to every connection**, which is what it meant when it was written:
/// the old set had no connection dimension, so it hid the name everywhere. That
/// is the only honest reading — re-scoping it to whichever connection happened
/// to be active would silently *unhide* databases on all the others, and
/// dropping it would silently unhide them everywhere. From the first toggle
/// onwards each connection's rules diverge normally.
pub fn migrate_flat(names: &[String], conn_ids: &[u64]) -> Vec<DbHiddenRule> {
    conn_ids
        .iter()
        .flat_map(|&conn_id| {
            names.iter().map(move |database| DbHiddenRule {
                conn_id,
                database: database.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> Vec<DbHiddenRule> {
        vec![DbHiddenRule {
            conn_id: 1,
            database: "world".into(),
        }]
    }

    /// The bug this store exists to end: two servers carrying the same database
    /// name, and hiding it on one taking it off the other.
    #[test]
    fn hiding_on_one_connection_leaves_another_connections_same_named_database_visible() {
        let r = rules();
        assert!(is_hidden(&r, 1, "world"));
        assert!(!is_hidden(&r, 2, "world"));
        assert_eq!(
            names_for(&r, 1),
            ["world".to_string()].into_iter().collect()
        );
        assert!(names_for(&r, 2).is_empty());
    }

    #[test]
    fn toggling_is_its_own_inverse_and_per_connection() {
        let mut r = Vec::new();
        assert!(toggle(&mut r, 1, "world"));
        assert!(toggle(&mut r, 2, "world"));
        assert!(!toggle(&mut r, 1, "world"));
        assert!(!is_hidden(&r, 1, "world"));
        assert!(is_hidden(&r, 2, "world"));
    }

    /// A deleted connection's rules go with it — the flat set could not do this,
    /// so its names went on hiding same-named databases on connections created
    /// afterwards.
    #[test]
    fn deleting_a_connection_forgets_what_it_hid() {
        let mut r = rules();
        r.push(DbHiddenRule {
            conn_id: 2,
            database: "scratch".into(),
        });
        clear_conn(&mut r, 1);
        assert!(!is_hidden(&r, 1, "world"));
        assert!(is_hidden(&r, 2, "scratch"));
    }

    /// The legacy list had no connection dimension, so it hid the name
    /// everywhere — and that is what it has to keep meaning across the upgrade.
    #[test]
    fn a_legacy_flat_list_stays_hidden_on_every_connection() {
        let r = migrate_flat(&["world".to_string(), "archive".to_string()], &[1, 7]);
        assert_eq!(r.len(), 4);
        for conn_id in [1, 7] {
            assert!(is_hidden(&r, conn_id, "world"));
            assert!(is_hidden(&r, conn_id, "archive"));
        }
        assert!(!is_hidden(&r, 2, "world"));
        // Nothing hidden, or no connections yet: nothing to migrate.
        assert!(migrate_flat(&[], &[1]).is_empty());
        assert!(migrate_flat(&["world".to_string()], &[]).is_empty());
    }
}
