//! Favorited (bookmarked) databases — a small persisted store of `(connection,
//! database)` keys, mirroring the colour store (`db_color.rs`). A favorited
//! database gets a star in the schema tree and sorts to the top of its
//! connection's list. **Order matters**: the `Vec` is kept in favorite order
//! (oldest first), so the tree can place the oldest favorite highest.

use serde::{Deserialize, Serialize};

/// A favorited database, keyed by the connection + database name. Position in the
/// [`FavoritesFile::rules`] vec is the favorite order (oldest first).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FavoriteRule {
    pub conn_id: u64,
    pub database: String,
}

/// The persisted favorites file (`favorites.json`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FavoritesFile {
    #[serde(default)]
    pub rules: Vec<FavoriteRule>,
}

/// Forget every favorite belonging to `conn_id` — the connection was deleted,
/// and nothing keyed to it should outlive it.
pub fn clear_conn(rules: &mut Vec<FavoriteRule>, conn_id: u64) {
    rules.retain(|r| r.conn_id != conn_id);
}

/// Whether `(conn_id, database)` is favorited.
pub fn is_favorite(rules: &[FavoriteRule], conn_id: u64, database: &str) -> bool {
    rules
        .iter()
        .any(|r| r.conn_id == conn_id && r.database == database)
}

/// Favorite rank (0 = oldest favorite, sorts highest), or `None` if not favorited.
pub fn rank(rules: &[FavoriteRule], conn_id: u64, database: &str) -> Option<usize> {
    rules
        .iter()
        .position(|r| r.conn_id == conn_id && r.database == database)
}

/// Toggle a database's favorite state: remove it if favorited, else append it
/// (newest favorites go last, so the oldest keeps rank 0). Returns the new state.
pub fn toggle(rules: &mut Vec<FavoriteRule>, conn_id: u64, database: &str) -> bool {
    if is_favorite(rules, conn_id, database) {
        rules.retain(|r| !(r.conn_id == conn_id && r.database == database));
        false
    } else {
        rules.push(FavoriteRule {
            conn_id,
            database: database.to_string(),
        });
        true
    }
}

#[cfg(test)]
mod clear_tests {
    use super::*;

    #[test]
    fn clear_conn_drops_only_that_connections_favorites() {
        let mut rules = vec![
            FavoriteRule {
                conn_id: 1,
                database: "shop".into(),
            },
            FavoriteRule {
                conn_id: 1,
                database: "blog".into(),
            },
            FavoriteRule {
                conn_id: 2,
                database: "shop".into(),
            },
        ];
        clear_conn(&mut rules, 1);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].conn_id, 2);
        clear_conn(&mut rules, 99);
        assert_eq!(rules.len(), 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_adds_and_removes() {
        let mut rules = Vec::new();
        assert!(toggle(&mut rules, 1, "app")); // now favorited
        assert!(is_favorite(&rules, 1, "app"));
        assert_eq!(rank(&rules, 1, "app"), Some(0));
        // Toggling again removes it, restoring the un-favorited (unranked) state.
        assert!(!toggle(&mut rules, 1, "app"));
        assert!(!is_favorite(&rules, 1, "app"));
        assert_eq!(rank(&rules, 1, "app"), None);
    }

    #[test]
    fn rank_preserves_favorite_order() {
        let mut rules = Vec::new();
        toggle(&mut rules, 1, "app");
        toggle(&mut rules, 1, "logs");
        toggle(&mut rules, 1, "cache");
        // Oldest favorite ranks highest (0).
        assert_eq!(rank(&rules, 1, "app"), Some(0));
        assert_eq!(rank(&rules, 1, "logs"), Some(1));
        assert_eq!(rank(&rules, 1, "cache"), Some(2));
        // Removing the middle one keeps the relative order of the rest.
        toggle(&mut rules, 1, "logs");
        assert_eq!(rank(&rules, 1, "app"), Some(0));
        assert_eq!(rank(&rules, 1, "cache"), Some(1));
    }

    #[test]
    fn keyed_by_connection() {
        let mut rules = Vec::new();
        toggle(&mut rules, 1, "app");
        assert!(is_favorite(&rules, 1, "app"));
        assert!(!is_favorite(&rules, 2, "app"));
    }
}
