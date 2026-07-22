//! Per-database identity colours — a small persisted store mapping a
//! `(connection, database)` to an `#rrggbb` hex, mirroring the formatter store
//! (`format.rs`). Display-only: a coloured dot marks the database in the schema
//! tree, the active-DB selector, and its query tabs, so schemas are told apart at
//! a glance. **Manual only** (set from the schema tree's right-click menu); an
//! unset database has no colour. The editor edge rules stay connection-scoped
//! (the production-red frame is the louder safety signal) and don't read this.

use serde::{Deserialize, Serialize};

/// A persisted database identity colour, keyed by the connection + database name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DbColorRule {
    pub conn_id: u64,
    pub database: String,
    /// `#rrggbb` hex.
    pub color: String,
}

/// The persisted database-colour file (`db_colors.json`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DbColorsFile {
    #[serde(default)]
    pub rules: Vec<DbColorRule>,
}

/// The colour set for a `(conn_id, database)`, or `None` if the user never set one.
pub fn lookup(rules: &[DbColorRule], conn_id: u64, database: &str) -> Option<String> {
    rules
        .iter()
        .find(|r| r.conn_id == conn_id && r.database == database)
        .map(|r| r.color.clone())
}

/// Set (`Some`) or clear (`None`) a database's colour: drop any existing rule for
/// the key, then store the new one if a colour was given.
pub fn upsert(rules: &mut Vec<DbColorRule>, conn_id: u64, database: &str, color: Option<String>) {
    rules.retain(|r| !(r.conn_id == conn_id && r.database == database));
    if let Some(color) = color {
        rules.push(DbColorRule {
            conn_id,
            database: database.to_string(),
            color,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_sets_replaces_and_clears() {
        let mut rules = Vec::new();
        upsert(&mut rules, 1, "app", Some("#E05252".into()));
        assert_eq!(lookup(&rules, 1, "app").as_deref(), Some("#E05252"));
        // Replacing the same key overwrites in place (no duplicate).
        upsert(&mut rules, 1, "app", Some("#52C77A".into()));
        assert_eq!(rules.len(), 1);
        assert_eq!(lookup(&rules, 1, "app").as_deref(), Some("#52C77A"));
        // A different database on the same connection is independent.
        upsert(&mut rules, 1, "logs", Some("#5B8DEF".into()));
        assert_eq!(rules.len(), 2);
        // Clearing removes just that key.
        upsert(&mut rules, 1, "app", None);
        assert_eq!(lookup(&rules, 1, "app"), None);
        assert_eq!(lookup(&rules, 1, "logs").as_deref(), Some("#5B8DEF"));
    }

    #[test]
    fn lookup_is_keyed_by_connection() {
        let mut rules = Vec::new();
        upsert(&mut rules, 1, "app", Some("#E05252".into()));
        assert_eq!(lookup(&rules, 2, "app"), None);
    }
}
