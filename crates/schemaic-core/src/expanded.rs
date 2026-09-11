//! Which tree nodes the SCHEMA panel has open — a small persisted store of
//! `(connection, node key)` pairs, mirroring the hidden
//! ([`crate::db_hidden`]), favourite and colour stores it shares a panel with.
//!
//! **It is keyed by connection because the same name means different databases
//! on different servers**, and every key this set holds is name-only:
//! `schema_tree::db_key` is `format!("db:{database}")`, and the same for
//! `tbl:`/`col:`/`sch:`/`objgrp:`. The set used to be a flat `Vec<String>`, so
//! expanding `sys` on a MariaDB connection left `sys` expanded on every other
//! MySQL-family connection too — a guaranteed collision, since
//! `information_schema`, `mysql`, `sys` and `performance_schema` exist on all of
//! them, and true of ordinary databases as well (`world` on both a MariaDB and a
//! PostgreSQL connection here). The second connection's node then rendered
//! already open, its whole table list built, and — with the size column on — the
//! stats effect immediately issued a `fetch_table_stats` against a database the
//! user had never opened there. It survived a restart, because the flat list was
//! persisted.
//!
//! **This is the third instance of one mistake**, and the other two are written
//! down as bugs in their own module docs: `db_hidden`'s flat `Vec<String>` hid
//! PostgreSQL's live `world` when a MariaDB `world` was hidden, and
//! `schema::tab_target`'s "last database" was one global signal until picking
//! `world` on MariaDB pointed a PostgreSQL tab at a different `world`. Both were
//! fixed by keying on the connection. This store is the same shape and was the
//! one left.
//!
//! Deleting a connection takes its rules with it ([`clear_conn`]), which the
//! flat set could not do: a deleted connection's open nodes stayed in
//! `ui_state.json` forever, auto-opening same-named databases on connections
//! created later.
//!
//! The runtime shape every consumer reads is still a `HashSet<String>` — the
//! keys open **on the connection being looked at** ([`keys_for`]) — because that
//! is the question the tree is asking, and it keeps every `contains` call in
//! `schema_tree` a one-argument one.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

/// One open tree node, keyed by the connection plus the node's own key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpandedRule {
    pub conn_id: u64,
    pub key: String,
}

/// Forget every expansion belonging to `conn_id` — the connection was deleted,
/// and nothing keyed to it should outlive it.
pub fn clear_conn(rules: &mut Vec<ExpandedRule>, conn_id: u64) {
    rules.retain(|r| r.conn_id != conn_id);
}

/// Is `key` open on `conn_id`?
pub fn is_expanded(rules: &[ExpandedRule], conn_id: u64, key: &str) -> bool {
    rules.iter().any(|r| r.conn_id == conn_id && r.key == key)
}

/// The keys open **on one connection** — the set the tree reads.
pub fn keys_for(rules: &[ExpandedRule], conn_id: u64) -> HashSet<String> {
    rules
        .iter()
        .filter(|r| r.conn_id == conn_id)
        .map(|r| r.key.clone())
        .collect()
}

/// Replace one connection's whole set, leaving every other connection's alone.
///
/// The mutator the app uses, because the tree's expansion lives in a
/// `RwSignal<HashSet<String>>` that a toggle, a Collapse all and a
/// collapse-this-database all rewrite wholesale — so "here is this connection's
/// set now" is the operation, and a per-key insert/remove would be a second
/// spelling of it.
///
/// **Order is not preserved and must not be relied on**: the runtime side is a
/// `HashSet`, so the on-disk order of one connection's keys is already whatever
/// the set iterated. What *is* stable is that other connections' rules keep
/// their relative order, which keeps a diff of `ui_state.json` readable.
pub fn set_keys(rules: &mut Vec<ExpandedRule>, conn_id: u64, keys: &HashSet<String>) {
    rules.retain(|r| r.conn_id != conn_id);
    rules.extend(keys.iter().map(|key| ExpandedRule {
        conn_id,
        key: key.clone(),
    }));
}

/// Read a legacy flat list of bare keys as rules.
///
/// **Applied to every connection**, which is what it meant when it was written:
/// the old set had no connection dimension, so a key was open everywhere at
/// once. That is the only honest reading — re-scoping it to whichever connection
/// happened to be active would silently collapse the tree on all the others, and
/// dropping it would collapse it everywhere. From the first toggle onwards each
/// connection's rules diverge normally.
///
/// **With no connections it answers `None`, and the caller must keep the legacy
/// list where it found it.** The outer loop is the connection ids, so an empty
/// one yields no rules for any number of keys — and the migration runs *once*,
/// with the flat field written empty from then on, so a launch where
/// `connections.json` failed to load would collapse every tree permanently with
/// nothing said and no way to retry. `Some(vec![])` and `None` are different
/// answers here: the first is "there was nothing to migrate", the second is "not
/// yet". This is `db_hidden::migrate_flat`'s argument verbatim, because it is
/// the same migration.
pub fn migrate_flat(keys: &[String], conn_ids: &[u64]) -> Option<Vec<ExpandedRule>> {
    if conn_ids.is_empty() {
        return None;
    }
    Some(
        conn_ids
            .iter()
            .flat_map(|&conn_id| {
                keys.iter().map(move |key| ExpandedRule {
                    conn_id,
                    key: key.clone(),
                })
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(keys: &[&str]) -> HashSet<String> {
        keys.iter().map(|k| k.to_string()).collect()
    }

    /// **The property the flat set could not even state.**
    ///
    /// Every key here is name-only, so `db:sys` on a MariaDB connection was
    /// `db:sys` on a MySQL one — and `information_schema`, `mysql`, `sys` and
    /// `performance_schema` are on *every* MySQL-family server, so the collision
    /// set was guaranteed and was the largest databases on the box. The second
    /// connection's node rendered already open, built its whole table list, and
    /// with the size column on fired a stats query against a database nobody had
    /// opened there.
    #[test]
    fn expanding_a_database_on_one_connection_does_not_open_it_on_another() {
        let mut rules = Vec::new();
        set_keys(
            &mut rules,
            1,
            &set(&["db:sys", "tbl:sys:innodb_lock_waits"]),
        );

        assert!(is_expanded(&rules, 1, "db:sys"));
        assert!(
            !is_expanded(&rules, 2, "db:sys"),
            "the same name on another server is a different database"
        );
        assert!(keys_for(&rules, 2).is_empty());
        assert_eq!(keys_for(&rules, 1).len(), 2);
    }

    /// Rewriting one connection's set is the whole operation — a toggle,
    /// Collapse all and collapse-this-database all rewrite the tree's set — and
    /// it must leave every other connection's alone.
    #[test]
    fn replacing_one_connections_set_leaves_the_others_untouched() {
        let mut rules = Vec::new();
        set_keys(&mut rules, 1, &set(&["db:shop", "db:world"]));
        set_keys(&mut rules, 2, &set(&["db:world"]));

        // Connection 1 collapses everything.
        set_keys(&mut rules, 1, &HashSet::new());
        assert!(keys_for(&rules, 1).is_empty());
        assert_eq!(keys_for(&rules, 2), set(&["db:world"]), "2 is untouched");

        // And expanding on 1 again does not touch 2 either.
        set_keys(&mut rules, 1, &set(&["db:analytics"]));
        assert_eq!(keys_for(&rules, 1), set(&["db:analytics"]));
        assert_eq!(keys_for(&rules, 2), set(&["db:world"]));
    }

    /// A deleted connection's expansions go with it — which the flat set could
    /// not do at all, so they stayed in `ui_state.json` forever and auto-opened
    /// same-named databases on connections created later.
    #[test]
    fn deleting_a_connection_takes_its_expansions() {
        let mut rules = Vec::new();
        set_keys(&mut rules, 1, &set(&["db:sys"]));
        set_keys(&mut rules, 2, &set(&["db:sys", "db:shop"]));

        clear_conn(&mut rules, 1);
        assert!(keys_for(&rules, 1).is_empty());
        assert_eq!(keys_for(&rules, 2), set(&["db:sys", "db:shop"]));

        // Clearing a connection with nothing stored is not an error.
        clear_conn(&mut rules, 99);
        assert_eq!(keys_for(&rules, 2).len(), 2);
    }

    /// The legacy list opened a key everywhere, so the migration does too —
    /// anything narrower silently collapses trees the user left open.
    #[test]
    fn a_legacy_flat_list_is_applied_to_every_connection() {
        let legacy = vec!["db:sys".to_string(), "db:world".to_string()];
        let rules = migrate_flat(&legacy, &[1, 2, 7]).expect("there are connections");
        for conn in [1, 2, 7] {
            assert_eq!(
                keys_for(&rules, conn),
                set(&["db:sys", "db:world"]),
                "connection {conn} keeps what the flat list meant"
            );
        }
        assert_eq!(rules.len(), 6);
    }

    /// **`None` and `Some(vec![])` are different answers.** With no connections
    /// loaded the migration has not happened and the flat list must stay where
    /// it is — a launch where `connections.json` failed to read would otherwise
    /// collapse every tree permanently, once, with no way to retry.
    #[test]
    fn no_connections_means_not_yet_rather_than_nothing_to_do() {
        assert_eq!(migrate_flat(&["db:sys".to_string()], &[]), None);
        assert_eq!(migrate_flat(&[], &[]), None);
        // Connections but no legacy keys *is* "nothing to migrate".
        assert_eq!(migrate_flat(&[], &[1, 2]), Some(Vec::new()));
    }
}
