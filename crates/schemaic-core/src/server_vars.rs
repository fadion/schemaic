//! Server variables — the active connection's settings, as the engine reports
//! them, for a read-only, searchable list. Pure over an already-fetched list:
//! no DB, no UI.
//!
//! The *fetch* lives in `schemaic-db`, one query per engine, because each
//! publishes its settings somewhere else: `SHOW GLOBAL VARIABLES` (and
//! `SHOW GLOBAL STATUS`) on MySQL/MariaDB, `pg_settings` on PostgreSQL,
//! `sys.configurations` on SQL Server, and a fixed list of PRAGMAs on SQLite
//! ([`SQLITE_PRAGMAS`]). What every engine's answer has in common is a name and
//! a value — a [`ServerVar`] — and the decisions the viewer makes about it
//! (which engines have a second, *status* list; what a search matches) are
//! here, where they can be tested without a server.

use crate::intel::SqlDialect;
use crate::text_ops::contains_ignore_ascii_case;

/// One setting, as the server reported it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerVar {
    /// The setting's name, as the engine spells it.
    pub name: String,
    /// Its current value, as text — PostgreSQL's with its unit appended.
    pub value: String,
    /// The engine's description of the setting, where it has one
    /// (PostgreSQL's `short_desc`, SQL Server's `description`).
    pub detail: Option<String>,
}

/// Which of the server's two lists to show.
///
/// **Variables** are the settings — what the server was configured to do.
/// **Status** is its counters — what it has done since it started (`Uptime`,
/// `Threads_connected`). Only MySQL/MariaDB publish the second as a list of
/// its own ([`supports_status`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum VarKind {
    #[default]
    Variables,
    Status,
}

/// Does `dialect` have settings to show at all?
///
/// Every engine this build speaks does — **SQLite included**, though it is a
/// library and not a server: what it has instead of server variables is its
/// PRAGMAs, and those are the settings a user of a SQLite file asks after
/// (`journal_mode`, `foreign_keys`, `page_size`). An exhaustive `match`, so a
/// fifth engine has to answer it.
pub fn supports_server_vars(dialect: SqlDialect) -> bool {
    match dialect {
        SqlDialect::MySql | SqlDialect::Postgres | SqlDialect::MsSql | SqlDialect::Sqlite => true,
    }
}

/// Does `dialect` publish a *status* list beside its variables?
///
/// MySQL/MariaDB's `SHOW GLOBAL STATUS` is that list. PostgreSQL spreads its
/// counters over a dozen `pg_stat_*` views with no single name/value shape,
/// SQL Server over its DMVs, and SQLite has no server to count — so the
/// viewer's Variables | Status switch is offered on MySQL alone, rather than
/// opening onto an empty list elsewhere.
pub fn supports_status(dialect: SqlDialect) -> bool {
    match dialect {
        SqlDialect::MySql => true,
        SqlDialect::Postgres | SqlDialect::MsSql | SqlDialect::Sqlite => false,
    }
}

/// Does `var` match the viewer's search `query`?
///
/// An ASCII case-insensitive substring over the name, the value and the detail, so
/// *"utf8"* finds every setting whose value is a UTF-8 character set and
/// *"parallel"* finds SQL Server's settings by their description. An empty
/// query matches everything.
pub fn var_matches(var: &ServerVar, query: &str) -> bool {
    let query = query.trim();
    query.is_empty()
        || contains_ignore_ascii_case(&var.name, query)
        || contains_ignore_ascii_case(&var.value, query)
        || var
            .detail
            .as_deref()
            .is_some_and(|d| contains_ignore_ascii_case(d, query))
}

/// The PRAGMAs a SQLite connection shows as its settings, in display order.
///
/// **Every one is read without an assignment** — `PRAGMA journal_mode`, never
/// `PRAGMA journal_mode = …` — because a PRAGMA given a value *changes* the
/// setting, and this is a read-only viewer. They are named here rather than
/// discovered (`pragma_list` names every PRAGMA, including the ones that act —
/// `optimize`, `integrity_check`, `wal_checkpoint`), so nothing that does work
/// can be reached by reading it.
pub const SQLITE_PRAGMAS: &[&str] = &[
    "journal_mode",
    "synchronous",
    "foreign_keys",
    "page_size",
    "page_count",
    "freelist_count",
    "cache_size",
    "encoding",
    "user_version",
    "application_id",
    "auto_vacuum",
    "busy_timeout",
    "temp_store",
];

/// How many matching settings the viewer builds rows for at once.
///
/// A MariaDB server reports some six hundred variables and a MySQL 8 server's
/// status list is longer, and the list is rebuilt on every keystroke of the
/// search. Nobody reads six hundred settings by scrolling: the search box is
/// how the rest are reached, and [`render_slice`] counts them rather than
/// dropping them silently — `activity::RENDER_CAP`'s rule.
pub const RENDER_CAP: usize = 300;

/// The settings matching `query`, the first [`RENDER_CAP`] of them, and how
/// many more matched beyond that.
///
/// References, not copies: only the rows that are drawn are cloned, by the
/// view that owns them.
pub fn render_slice<'a>(vars: &'a [ServerVar], query: &str) -> (Vec<&'a ServerVar>, usize) {
    let mut shown = Vec::new();
    let mut more = 0;
    for v in vars.iter().filter(|v| var_matches(v, query)) {
        if shown.len() < RENDER_CAP {
            shown.push(v);
        } else {
            more += 1;
        }
    }
    (shown, more)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_slice_keeps_every_match_under_the_cap() {
        let vars = vec![
            var("max_connections", "151", None),
            var("port", "3306", None),
            var("max_allowed_packet", "16777216", None),
        ];
        let (shown, more) = render_slice(&vars, "max");
        let names: Vec<&str> = shown.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, ["max_connections", "max_allowed_packet"]);
        assert_eq!(more, 0);
    }

    #[test]
    fn render_slice_counts_the_matches_past_the_cap() {
        let vars: Vec<ServerVar> = (0..RENDER_CAP + 7)
            .map(|i| var(&format!("v{i}"), "x", None))
            .chain(std::iter::once(var("other", "y", None)))
            .collect();
        let (shown, more) = render_slice(&vars, "v");
        assert_eq!(shown.len(), RENDER_CAP);
        assert_eq!(more, 7, "the non-matching row is not counted as more");
        assert_eq!(shown[0].name, "v0", "the list keeps the server's order");
    }

    #[test]
    fn render_slice_of_nothing_is_nothing() {
        let (shown, more) = render_slice(&[], "");
        assert!(shown.is_empty());
        assert_eq!(more, 0);
    }

    fn var(name: &str, value: &str, detail: Option<&str>) -> ServerVar {
        ServerVar {
            name: name.to_string(),
            value: value.to_string(),
            detail: detail.map(str::to_string),
        }
    }

    #[test]
    fn every_engine_has_settings_to_show() {
        for d in SqlDialect::ALL {
            assert!(supports_server_vars(d), "{d:?}");
        }
    }

    #[test]
    fn only_mysql_has_a_status_list() {
        assert!(supports_status(SqlDialect::MySql));
        assert!(!supports_status(SqlDialect::Postgres));
        assert!(!supports_status(SqlDialect::MsSql));
        assert!(!supports_status(SqlDialect::Sqlite));
    }

    #[test]
    fn the_default_kind_is_variables() {
        assert_eq!(VarKind::default(), VarKind::Variables);
    }

    #[test]
    fn an_empty_query_matches_everything() {
        let v = var("max_connections", "151", None);
        assert!(var_matches(&v, ""));
        assert!(var_matches(&v, "   "));
    }

    #[test]
    fn a_query_matches_the_name_case_insensitively() {
        let v = var("max_connections", "151", None);
        assert!(var_matches(&v, "MAX_CONN"));
        assert!(var_matches(&v, "connections"));
    }

    #[test]
    fn a_query_matches_the_value() {
        let v = var("character_set_server", "utf8mb4", None);
        assert!(var_matches(&v, "UTF8"));
    }

    #[test]
    fn a_query_matches_the_detail() {
        let v = var(
            "max degree of parallelism",
            "0",
            Some("Maximum Degree Of Parallelism"),
        );
        assert!(var_matches(&v, "degree"));
        let w = var(
            "cost threshold",
            "5",
            Some("cost threshold for PARALLELISM"),
        );
        assert!(var_matches(&w, "parallelism"));
    }

    #[test]
    fn a_query_that_is_nowhere_does_not_match() {
        let v = var("max_connections", "151", Some("Sets the maximum"));
        assert!(!var_matches(&v, "wal_level"));
        let bare = var("port", "3306", None);
        assert!(!var_matches(&bare, "socket"));
    }

    #[test]
    fn the_sqlite_pragmas_are_bare_names_and_unique() {
        assert!(SQLITE_PRAGMAS.contains(&"journal_mode"));
        assert!(SQLITE_PRAGMAS.contains(&"foreign_keys"));
        for p in SQLITE_PRAGMAS {
            // A name only — an `=` or a `(` would be an assignment.
            assert!(
                p.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'),
                "{p}"
            );
        }
        let mut seen = std::collections::HashSet::new();
        for p in SQLITE_PRAGMAS {
            assert!(seen.insert(p), "duplicate {p}");
        }
    }
}
