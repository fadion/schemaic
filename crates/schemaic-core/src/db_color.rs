//! Identity colours — a small persisted store mapping a `(connection,
//! database)`, or a `(connection, database, table)`, to an `#rrggbb` hex,
//! mirroring the formatter store (`format.rs`). Display-only: a coloured dot
//! marks the database in the schema tree, the active-DB selector and its query
//! tabs, and a table in the schema tree, so objects are told apart at a glance;
//! a table's colour also tints its card header in the ER diagram, which is the
//! one surface where the colour is a fill rather than a dot. **Manual only** (set
//! from the schema tree's right-click menu); an unset object has no colour. The
//! editor edge rules stay connection-scoped (the production-red frame is the
//! louder safety signal) and don't read this.
//!
//! The two are **separate stores in one file**, not one store with an optional
//! table: nothing then has to remember to filter the database rules out of a
//! table lookup, and a database called `app` cannot lend its colour to a table
//! called `app`.

use serde::{Deserialize, Serialize};

/// A persisted database identity colour, keyed by the connection + database name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DbColorRule {
    pub conn_id: u64,
    pub database: String,
    /// `#rrggbb` hex.
    pub color: String,
}

/// A persisted table identity colour, keyed by the connection + database + the
/// table's **display** name.
///
/// `table` is [`crate::schema::TableSource::display`] — the bare name on
/// MySQL/SQLite and inside PostgreSQL's `public`, `schema.table` outside it. That
/// spelling is the key rather than a separate `schema` field because it is
/// already the identity the ER diagram gives a node, so the diagram can look a
/// colour up by node id with no reparsing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableColorRule {
    pub conn_id: u64,
    pub database: String,
    /// The table's display name (`table`, or `schema.table`).
    pub table: String,
    /// `#rrggbb` hex.
    pub color: String,
}

/// The persisted colour file (`db_colors.json`). `tables` is `#[serde(default)]`
/// so a file written before table colours existed still loads.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DbColorsFile {
    #[serde(default)]
    pub rules: Vec<DbColorRule>,
    #[serde(default)]
    pub tables: Vec<TableColorRule>,
}

/// Forget every colour belonging to `conn_id` — the connection was deleted, and
/// nothing keyed to it should outlive it.
pub fn clear_conn(rules: &mut Vec<DbColorRule>, conn_id: u64) {
    rules.retain(|r| r.conn_id != conn_id);
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

/// Forget every table colour belonging to `conn_id` — the connection was
/// deleted. The table-store half of [`clear_conn`]; both run together, since a
/// deleted connection leaves neither kind of rule behind.
pub fn table_clear_conn(rules: &mut Vec<TableColorRule>, conn_id: u64) {
    rules.retain(|r| r.conn_id != conn_id);
}

/// The colour set for a `(conn_id, database, table)`, or `None` if the user never
/// set one. `table` is the display name — see [`TableColorRule::table`].
pub fn table_lookup(
    rules: &[TableColorRule],
    conn_id: u64,
    database: &str,
    table: &str,
) -> Option<String> {
    rules
        .iter()
        .find(|r| r.conn_id == conn_id && r.database == database && r.table == table)
        .map(|r| r.color.clone())
}

/// Set (`Some`) or clear (`None`) a table's colour: drop any existing rule for
/// the key, then store the new one if a colour was given.
pub fn table_upsert(
    rules: &mut Vec<TableColorRule>,
    conn_id: u64,
    database: &str,
    table: &str,
    color: Option<String>,
) {
    rules.retain(|r| !(r.conn_id == conn_id && r.database == database && r.table == table));
    if let Some(color) = color {
        rules.push(TableColorRule {
            conn_id,
            database: database.to_string(),
            table: table.to_string(),
            color,
        });
    }
}

/// Which identity colour a **new connection** should get: the first preset
/// nobody is using, or — once every preset is taken — one of them chosen by
/// `seed`.
///
/// **The decision, separated from the clock that used to be inside it.** The
/// caller read `SystemTime::now().subsec_nanos()` in the middle of the
/// function, so neither branch had a deterministic entry point and the whole
/// thing had no test anywhere in the workspace, in the UI crate, with all three
/// callers in a third one. A wrong answer here is silent and durable: the
/// identity colour is what the environment badge, the tab-strip rule and the
/// editor frame use to tell production from local, so a regression that starts
/// handing out duplicates makes two connections look alike with nothing to
/// notice it.
///
/// **The first unused preset, not a random one.** The old spelling indexed the
/// unused pool by the seed too, so two connections made in the same second
/// could take the same colour while six others sat free. Comparison is
/// ASCII-case-insensitive, since a colour the user typed by hand is stored as
/// they typed it.
///
/// `presets` is empty only if a caller has nothing to offer, and the empty
/// string is the honest answer to that rather than a panic on `% 0`.
pub fn pick_color<'a>(presets: &[&'a str], used: &[String], seed: usize) -> &'a str {
    if presets.is_empty() {
        return "";
    }
    let is_used = |c: &str| used.iter().any(|u| u.eq_ignore_ascii_case(c));
    match presets.iter().find(|c| !is_used(c)) {
        Some(free) => free,
        None => presets[seed % presets.len()],
    }
}

#[cfg(test)]
mod pick_color_tests {
    use super::pick_color;

    const PRESETS: [&str; 3] = ["#E05252", "#E0C24B", "#52C77A"];

    fn used(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// With nothing taken, the **first** preset — not whatever the seed lands
    /// on. Two connections made in the same second used to be able to take the
    /// same colour with six others free.
    #[test]
    fn an_unused_preset_is_taken_in_order_whatever_the_seed() {
        for seed in [0, 1, 7, usize::MAX] {
            assert_eq!(pick_color(&PRESETS, &[], seed), "#E05252", "seed {seed}");
        }
        assert_eq!(pick_color(&PRESETS, &used(&["#E05252"]), 2), "#E0C24B");
        assert_eq!(
            pick_color(&PRESETS, &used(&["#E05252", "#E0C24B"]), 0),
            "#52C77A",
            "one left"
        );
    }

    /// A colour the user typed by hand is stored as they typed it, so the
    /// comparison cannot be case-sensitive or the palette hands out a duplicate
    /// that only differs in spelling.
    #[test]
    fn a_used_colour_counts_however_it_is_cased() {
        assert_eq!(pick_color(&PRESETS, &used(&["#e05252"]), 0), "#E0C24B");
        assert_eq!(pick_color(&PRESETS, &used(&["#E05252"]), 0), "#E0C24B");
    }

    /// Everything taken: fall back to the whole palette, and never panic on the
    /// modulo.
    #[test]
    fn a_full_palette_falls_back_to_the_seed_and_never_panics() {
        let all = used(&["#E05252", "#E0C24B", "#52C77A"]);
        assert_eq!(pick_color(&PRESETS, &all, 0), "#E05252");
        assert_eq!(pick_color(&PRESETS, &all, 4), "#E0C24B");
        assert_eq!(
            pick_color(&PRESETS, &all, usize::MAX),
            PRESETS[usize::MAX % 3]
        );
        // A single-entry palette has one answer at every seed.
        assert_eq!(pick_color(&["#E05252"], &used(&["#E05252"]), 99), "#E05252");
    }

    /// An empty pool answers with nothing rather than dividing by zero.
    #[test]
    fn an_empty_palette_is_an_empty_answer() {
        assert_eq!(pick_color(&[], &[], 0), "");
        assert_eq!(pick_color(&[], &used(&["#E05252"]), 3), "");
    }
}

#[cfg(test)]
mod table_tests {
    use super::*;

    #[test]
    fn table_upsert_sets_replaces_and_clears() {
        let mut rules = Vec::new();
        table_upsert(&mut rules, 1, "app", "orders", Some("#E05252".into()));
        assert_eq!(
            table_lookup(&rules, 1, "app", "orders").as_deref(),
            Some("#E05252")
        );
        // Replacing the same key overwrites in place (no duplicate).
        table_upsert(&mut rules, 1, "app", "orders", Some("#52C77A".into()));
        assert_eq!(rules.len(), 1);
        assert_eq!(
            table_lookup(&rules, 1, "app", "orders").as_deref(),
            Some("#52C77A")
        );
        // Another table in the same database is independent.
        table_upsert(&mut rules, 1, "app", "customers", Some("#5B8DEF".into()));
        assert_eq!(rules.len(), 2);
        table_upsert(&mut rules, 1, "app", "orders", None);
        assert_eq!(table_lookup(&rules, 1, "app", "orders"), None);
        assert_eq!(
            table_lookup(&rules, 1, "app", "customers").as_deref(),
            Some("#5B8DEF")
        );
    }

    /// All three parts of the key matter. A same-named table under another
    /// connection or another database is a different table, and colouring one
    /// must not colour the others.
    #[test]
    fn table_lookup_is_keyed_by_connection_and_database() {
        let mut rules = Vec::new();
        table_upsert(&mut rules, 1, "app", "orders", Some("#E05252".into()));
        assert_eq!(table_lookup(&rules, 2, "app", "orders"), None);
        assert_eq!(table_lookup(&rules, 1, "logs", "orders"), None);
        assert_eq!(table_lookup(&rules, 1, "app", "order"), None);
    }

    /// The key is the table's *display* name, so a PostgreSQL table outside
    /// `public` is `schema.table` and can't collide with a same-named table in
    /// another namespace.
    #[test]
    fn a_qualified_table_is_its_own_key() {
        let mut rules = Vec::new();
        table_upsert(&mut rules, 1, "app", "sales.orders", Some("#43C6C6".into()));
        table_upsert(&mut rules, 1, "app", "orders", Some("#9B6DE0".into()));
        assert_eq!(
            table_lookup(&rules, 1, "app", "sales.orders").as_deref(),
            Some("#43C6C6")
        );
        assert_eq!(
            table_lookup(&rules, 1, "app", "orders").as_deref(),
            Some("#9B6DE0")
        );
    }

    #[test]
    fn table_clear_conn_drops_only_that_connections_colours() {
        let mut rules = Vec::new();
        table_upsert(&mut rules, 1, "app", "orders", Some("#E05252".into()));
        table_upsert(&mut rules, 1, "logs", "events", Some("#E08A4B".into()));
        table_upsert(&mut rules, 2, "app", "orders", Some("#52C77A".into()));
        table_clear_conn(&mut rules, 1);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].conn_id, 2);
        // Clearing an absent connection is a no-op.
        table_clear_conn(&mut rules, 99);
        assert_eq!(rules.len(), 1);
    }

    /// The two stores share a file but never a key space: a database colour is
    /// not a colour for a table of the same name.
    #[test]
    fn database_and_table_colours_are_separate_stores() {
        let mut dbs = Vec::new();
        let mut tables = Vec::new();
        upsert(&mut dbs, 1, "app", Some("#E05252".into()));
        table_upsert(&mut tables, 1, "app", "app", Some("#52C77A".into()));
        assert_eq!(lookup(&dbs, 1, "app").as_deref(), Some("#E05252"));
        assert_eq!(
            table_lookup(&tables, 1, "app", "app").as_deref(),
            Some("#52C77A")
        );
    }

    /// A file written before table colours existed must still load, with an
    /// empty table store rather than a parse error.
    #[test]
    fn a_file_without_the_tables_key_still_loads() {
        // `r##` — the hex's `#` would close an `r#"…"#` literal.
        let json = r##"{"rules":[{"conn_id":1,"database":"app","color":"#E05252"}]}"##;
        let file: DbColorsFile = serde_json::from_str(json).expect("legacy file parses");
        assert_eq!(file.rules.len(), 1);
        assert!(file.tables.is_empty());
    }
}

#[cfg(test)]
mod clear_tests {
    use super::*;

    #[test]
    fn clear_conn_drops_only_that_connections_colours() {
        let mut rules = vec![
            DbColorRule {
                conn_id: 1,
                database: "shop".into(),
                color: "#aabbcc".into(),
            },
            DbColorRule {
                conn_id: 2,
                database: "shop".into(),
                color: "#ddeeff".into(),
            },
        ];
        clear_conn(&mut rules, 1);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].conn_id, 2);
        // Clearing an absent connection is a no-op.
        clear_conn(&mut rules, 99);
        assert_eq!(rules.len(), 1);
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
