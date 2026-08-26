//! The snippet library: named saved queries, and the rules for which of them a
//! given connection may see — the pure, testable half.
//!
//! # A library is not a log
//!
//! [`crate::history`] keys its entries by `conn_id` and caps them per
//! connection, which is right for a record of what ran *there*. A library is the
//! opposite: a "find the running queries" snippet is wanted on every MySQL
//! connection, not on the one it happened to be saved from. So a snippet carries
//! a [`Scope`] — global, one engine, or one connection — and defaults to the
//! dialect of wherever it was saved. Nothing is capped, and nothing is evicted:
//! a library the app quietly forgets entries from is not a library.
//!
//! # A snippet's placeholders are already a feature
//!
//! A body may contain `:name` placeholders, and they need no machinery of their
//! own — inserting the body puts them in the editor and
//! [`crate::params`] gives them a parameters bar. That is why there is no
//! tab-stop syntax here: the second template mechanism would have to be taught
//! everything the first one already knows.

use serde::{Deserialize, Serialize};

use crate::intel::SqlDialect;
use crate::text_ops::contains_ignore_ascii_case;

/// Where a snippet is offered.
///
/// [`Scope::Unknown`] **keeps the text it didn't recognise**, and the
/// hand-written [`Serialize`] writes it back verbatim — the rule
/// [`crate::search_history::ObjectTag`] states in full. `snippets.json` is
/// rewritten whole on every change, so a bare `#[serde(other)]` unit variant
/// would mean that merely running this build once rewrote a newer build's
/// `"duckdb"` scope as the literal `"unknown"`, and going back would no longer
/// recognise its own snippet. Degrading the *file* is the point; degrading the
/// *value* is a different thing that looks like it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scope {
    /// Every connection.
    Global,
    /// Every connection of one engine — the default a Save assigns.
    Dialect(SqlDialect),
    /// One saved connection.
    Conn(u64),
    /// A scope this build doesn't know, preserved exactly as it was read. It is
    /// offered nowhere: this build cannot say which connections it meant.
    Unknown(String),
}

impl Scope {
    fn as_str(&self) -> String {
        match self {
            Scope::Global => "global".to_string(),
            Scope::Dialect(SqlDialect::MySql) => "mysql".to_string(),
            Scope::Dialect(SqlDialect::Postgres) => "postgres".to_string(),
            Scope::Dialect(SqlDialect::Sqlite) => "sqlite".to_string(),
            Scope::Conn(id) => format!("conn:{id}"),
            Scope::Unknown(s) => s.clone(),
        }
    }
}

impl Serialize for Scope {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.as_str())
    }
}

impl<'de> Deserialize<'de> for Scope {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(match s.as_str() {
            "global" => Scope::Global,
            "mysql" => Scope::Dialect(SqlDialect::MySql),
            "postgres" => Scope::Dialect(SqlDialect::Postgres),
            "sqlite" => Scope::Dialect(SqlDialect::Sqlite),
            other => match other.strip_prefix("conn:").map(str::parse::<u64>) {
                Some(Ok(id)) => Scope::Conn(id),
                // Includes `conn:` followed by something that isn't a number —
                // preserved rather than guessed at, like any other unknown.
                _ => Scope::Unknown(s),
            },
        })
    }
}

/// Who wrote a snippet. A shipped one can't be edited or deleted — the library
/// offers Duplicate instead, which yields an ordinary user copy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", from = "SourceRaw")]
pub enum Source {
    #[default]
    User,
    Builtin,
}

/// Parsing shim for [`Source`]; see [`crate::persist::RightPanelState`] for why
/// every persisted enum has one.
#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum SourceRaw {
    User,
    Builtin,
    #[serde(other)]
    Other,
}

impl From<SourceRaw> for Source {
    fn from(raw: SourceRaw) -> Self {
        match raw {
            SourceRaw::Builtin => Source::Builtin,
            SourceRaw::User | SourceRaw::Other => Source::User,
        }
    }
}

/// One saved query.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snippet {
    /// Stable identity — survives a rename, and is what an edit, a delete and a
    /// Find-Anywhere activation carry. Allocated by [`next_id`]; `0` is the id
    /// no allocation returns.
    pub id: u64,
    pub name: String,
    /// Completion trigger: typing it offers this snippet in the popup. `None` on
    /// most snippets — a library is found, not typed.
    #[serde(default)]
    pub abbrev: Option<String>,
    /// The SQL. May hold `:name` placeholders, which become parameters-bar rows
    /// once inserted; what they are is never stored here, because a stored list
    /// goes stale the first time the body is edited.
    pub body: String,
    pub scope: Scope,
    #[serde(default)]
    pub source: Source,
    /// Last inserted, unix epoch millis. `None` = never used, which sorts last.
    #[serde(default)]
    pub last_used: Option<u64>,
}

/// The persisted file (`snippets.json`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SnippetsFile {
    #[serde(default)]
    pub snippets: Vec<Snippet>,
}

/// One heading in the library panel. The connection's *name* is not here: the
/// core knows it as an id, and the panel resolves it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bucket {
    Conn(u64),
    Dialect(SqlDialect),
    Global,
}

/// A heading and the snippets under it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub bucket: Bucket,
    pub items: Vec<Snippet>,
}

/// May this snippet be offered on this connection?
pub fn applies(snippet: &Snippet, dialect: SqlDialect, conn_id: u64) -> bool {
    match &snippet.scope {
        Scope::Global => true,
        Scope::Dialect(d) => *d == dialect,
        Scope::Conn(id) => *id == conn_id,
        Scope::Unknown(_) => false,
    }
}

/// Whether a snippet matches the panel's free-text filter (ASCII
/// case-insensitive), across its name, abbrev and body. An empty or
/// whitespace-only filter matches everything.
///
/// The body is matched **whitespace-collapsed**, the way [`crate::history`]
/// matches a statement: the panel shows it collapsed to three lines, so a
/// two-word filter has to read across the newlines the user can't see.
pub fn matches_query(snippet: &Snippet, query: &str) -> bool {
    let q = query.trim();
    if q.is_empty() {
        return true;
    }
    contains_ignore_ascii_case(&snippet.name, q)
        || snippet
            .abbrev
            .as_deref()
            .is_some_and(|a| contains_ignore_ascii_case(a, q))
        || contains_ignore_ascii_case(&collapsed(&snippet.body), q)
}

/// The body as the panel shows it: whitespace collapsed to single spaces.
pub fn collapsed(body: &str) -> String {
    body.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The panel's groups for a connection, narrowed by the filter.
///
/// Buckets run **narrowest first** — this connection, then this engine, then
/// everywhere — so the snippets written for the database in front of you are the
/// ones at the top. An empty bucket is omitted rather than shown as a heading
/// with nothing under it.
pub fn grouped(all: &[Snippet], dialect: SqlDialect, conn_id: u64, query: &str) -> Vec<Group> {
    let buckets = [
        Bucket::Conn(conn_id),
        Bucket::Dialect(dialect),
        Bucket::Global,
    ];
    buckets
        .into_iter()
        .filter_map(|bucket| {
            let mut items: Vec<Snippet> = all
                .iter()
                .filter(|s| applies(s, dialect, conn_id))
                .filter(|s| matches_query(s, query))
                .filter(|s| in_bucket(s, bucket))
                .cloned()
                .collect();
            sort_for_panel(&mut items);
            (!items.is_empty()).then_some(Group { bucket, items })
        })
        .collect()
}

/// Where built-in ids start. Everything at or above this is shipped with the
/// app and lives in code; everything below it is the user's and lives in
/// `snippets.json`.
///
/// The split is what lets built-ins stay *code*: they are merged in at read time
/// by [`library`] rather than written into the user's file, so a later release
/// can fix one, and nobody's file fills up with content the app owns. [`next_id`]
/// refuses to allocate into this range, so a hand-edited file cannot collide
/// with the pack either.
pub const BUILTIN_ID_BASE: u64 = 1 << 32;

/// The snippets shipped with the app for an engine.
///
/// Deliberately small and answering the questions a DBA opens a client to ask —
/// what is running, what is big, what is unused, what is blocked. Every one of
/// them is a plain `SELECT`: a starter pack is the worst possible place for a
/// statement that changes something, and one that needed the write guard's
/// confirmation would teach the wrong thing about what these rows do.
///
/// **MySQL and MariaDB are one dialect here**, so every MySQL entry has to run on
/// both — which is why none of them reads `performance_schema` or `sys`, where
/// the two diverge and where MariaDB's tables are off by default.
pub fn builtins(dialect: SqlDialect) -> Vec<Snippet> {
    let rows: &[(u64, &str, &str, &str)] = match dialect {
        SqlDialect::MySql => &[
            (
                1,
                "Running queries",
                "ps",
                "SELECT id, user, host, db, command, time, state,\n       LEFT(info, 200) AS query\nFROM information_schema.processlist\nWHERE command <> 'Sleep'\nORDER BY time DESC;",
            ),
            (
                2,
                "Table sizes",
                "sizes",
                "SELECT table_name,\n       table_rows,\n       ROUND((data_length + index_length) / 1024 / 1024, 1) AS size_mb\nFROM information_schema.tables\nWHERE table_schema = DATABASE()\nORDER BY data_length + index_length DESC;",
            ),
            (
                3,
                "Open transactions",
                "trx",
                "SELECT trx_id, trx_state, trx_started,\n       trx_mysql_thread_id AS thread_id,\n       trx_rows_modified,\n       LEFT(trx_query, 200) AS query\nFROM information_schema.innodb_trx\nORDER BY trx_started;",
            ),
            (
                4,
                "Indexes by table",
                "idx",
                "SELECT table_name, index_name,\n       GROUP_CONCAT(column_name ORDER BY seq_in_index) AS columns_in_order,\n       non_unique\nFROM information_schema.statistics\nWHERE table_schema = DATABASE()\nGROUP BY table_name, index_name, non_unique\nORDER BY table_name, index_name;",
            ),
        ],
        SqlDialect::Postgres => &[
            (
                101,
                "Running queries",
                "ps",
                "SELECT pid, usename, state,\n       now() - query_start AS running_for,\n       left(query, 200) AS query\nFROM pg_stat_activity\nWHERE state <> 'idle' AND pid <> pg_backend_pid()\nORDER BY running_for DESC;",
            ),
            (
                102,
                "Table sizes",
                "sizes",
                "SELECT relname AS table_name,\n       pg_size_pretty(pg_total_relation_size(relid)) AS total_size,\n       n_live_tup AS live_rows\nFROM pg_stat_user_tables\nORDER BY pg_total_relation_size(relid) DESC;",
            ),
            (
                103,
                "Unused indexes",
                "idx",
                "SELECT relname AS table_name,\n       indexrelname AS index_name,\n       idx_scan AS scans,\n       pg_size_pretty(pg_relation_size(indexrelid)) AS index_size\nFROM pg_stat_user_indexes\nWHERE idx_scan = 0\nORDER BY pg_relation_size(indexrelid) DESC;",
            ),
            (
                104,
                "Blocked queries",
                "locks",
                "SELECT blocked.pid AS blocked_pid,\n       left(blocked.query, 120) AS blocked_query,\n       blocking.pid AS blocking_pid,\n       left(blocking.query, 120) AS blocking_query,\n       blocking.state AS blocking_state\nFROM pg_stat_activity blocked\nJOIN pg_stat_activity blocking\n  ON blocking.pid = ANY (pg_blocking_pids(blocked.pid));",
            ),
        ],
        SqlDialect::Sqlite => &[
            (
                201,
                "Schema of everything",
                "ddl",
                "SELECT type, name, tbl_name, sql\nFROM sqlite_master\nWHERE sql IS NOT NULL\nORDER BY type, name;",
            ),
            (
                202,
                "Tables at a glance",
                "tables",
                "SELECT m.name AS table_name,\n       (SELECT COUNT(*) FROM pragma_table_info(m.name)) AS columns,\n       (SELECT COUNT(*) FROM pragma_index_list(m.name)) AS indexes\nFROM sqlite_master m\nWHERE m.type = 'table' AND m.name NOT LIKE 'sqlite_%'\nORDER BY m.name;",
            ),
            (
                203,
                "Database size",
                "size",
                "SELECT page_count * page_size / 1024 AS size_kb,\n       page_count, page_size, freelist_count\nFROM pragma_page_count(), pragma_page_size(), pragma_freelist_count();",
            ),
            (
                204,
                "Indexes of a table",
                "idx",
                "SELECT il.name AS index_name,\n       il.\"unique\" AS is_unique,\n       group_concat(ii.name) AS columns_in_order\nFROM pragma_index_list(:table_name) il\nJOIN pragma_index_info(il.name) ii\nGROUP BY il.name, il.\"unique\"\nORDER BY il.name;",
            ),
        ],
    };
    rows.iter()
        .map(|(n, name, abbrev, body)| Snippet {
            id: BUILTIN_ID_BASE + n,
            name: name.to_string(),
            abbrev: Some(abbrev.to_string()),
            body: body.to_string(),
            scope: Scope::Dialect(dialect),
            source: Source::Builtin,
            // A shipped snippet has no "last used" to show — its row says
            // `Built-in` where a saved one says `3d ago` — and nothing could
            // record one anyway: it is not in the file that would remember it.
            last_used: None,
        })
        .collect()
}

/// The whole library a connection sees: the user's snippets, then the built-ins
/// for its engine.
///
/// **User first, deliberately.** [`by_abbrev`] breaks a tie on scope and then on
/// order, so a user snippet abbreviated `ps` shadows the shipped one of the same
/// spelling rather than the other way round.
pub fn library(user: &[Snippet], dialect: SqlDialect) -> Vec<Snippet> {
    let mut all = user.to_vec();
    all.extend(builtins(dialect));
    all
}

/// The scopes a snippet on this connection can be moved to, **narrowest first**
/// — the same order [`grouped`] puts the bands in, so the choice a picker offers
/// second lands under the heading that is second.
///
/// A [`Scope::Unknown`] is not among them and never needs to be: it is offered
/// nowhere, so there is no row to pick from in the first place.
pub fn scope_options(dialect: SqlDialect, conn_id: u64) -> Vec<Scope> {
    vec![Scope::Conn(conn_id), Scope::Dialect(dialect), Scope::Global]
}

/// Which heading a snippet belongs under. Its own scope decides — a snippet is
/// in exactly one bucket, so a global one never also appears under the engine.
fn in_bucket(snippet: &Snippet, bucket: Bucket) -> bool {
    match (&snippet.scope, bucket) {
        (Scope::Conn(a), Bucket::Conn(b)) => *a == b,
        (Scope::Dialect(a), Bucket::Dialect(b)) => *a == b,
        (Scope::Global, Bucket::Global) => true,
        _ => false,
    }
}

/// Most-recently-used first, then by name. A library sorted alphabetically
/// punishes you for having many; one sorted only by use hides everything you
/// haven't reached for yet behind an arbitrary order, so the never-used tail is
/// alphabetical.
fn sort_for_panel(items: &mut [Snippet]) {
    items.sort_by(|a, b| {
        b.last_used
            .cmp(&a.last_used)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

/// The snippet an abbrev triggers, if one is in scope here.
///
/// Whole-word and ASCII case-insensitive: a *prefix* must not expand, or typing
/// `psql` in a query would offer the snippet abbreviated `ps`. Ties go to the
/// narrowest scope, so a connection-specific abbrev shadows an engine-wide one
/// of the same spelling.
pub fn by_abbrev(
    all: &[Snippet],
    abbrev: &str,
    dialect: SqlDialect,
    conn_id: u64,
) -> Option<Snippet> {
    if abbrev.is_empty() {
        return None;
    }
    let mut hits: Vec<&Snippet> = all
        .iter()
        .filter(|s| applies(s, dialect, conn_id))
        .filter(|s| {
            s.abbrev
                .as_deref()
                .is_some_and(|a| a.eq_ignore_ascii_case(abbrev))
        })
        .collect();
    hits.sort_by_key(|s| match s.scope {
        Scope::Conn(_) => 0,
        Scope::Dialect(_) => 1,
        _ => 2,
    });
    hits.first().map(|s| (*s).clone())
}

/// The next free id: one past the highest in use, and `1` for an empty library
/// (`0` is the id no allocation returns).
///
/// **An id is reused if the snippet holding it is deleted** — delete the newest
/// and the next Save takes its number back. That is acceptable *because a
/// snippet id is never persisted outside `snippets.json`*: the panel and
/// Find-Anywhere resolve one against the live list in the same breath they read
/// it, and snippet activations are deliberately not recorded in
/// `search_history.json`. Anything that starts remembering an id across
/// sessions has to make this counter monotonic first — a `next_id` field on
/// [`SnippetsFile`] — or it will eventually resolve to somebody else's query.
pub fn next_id(all: &[Snippet]) -> u64 {
    all.iter()
        .map(|s| s.id)
        // Built-in ids are reserved and enormous; one that reached this list —
        // a hand-edited file, or a caller passing `library`'s merged view —
        // would otherwise push every future allocation into the pack's range.
        .filter(|id| *id < BUILTIN_ID_BASE)
        .max()
        .unwrap_or(0)
        + 1
}

/// Record that a snippet was just used. No-op for an id that is gone.
pub fn touch(all: &mut [Snippet], id: u64, now: u64) {
    if let Some(s) = all.iter_mut().find(|s| s.id == id) {
        s.last_used = Some(now);
    }
}

/// Delete a snippet by id.
pub fn remove(all: &mut Vec<Snippet>, id: u64) {
    all.retain(|s| s.id != id);
}

#[cfg(test)]
mod tests {
    use super::*;

    const MY: SqlDialect = SqlDialect::MySql;
    const PG: SqlDialect = SqlDialect::Postgres;

    fn snip(id: u64, name: &str, scope: Scope) -> Snippet {
        Snippet {
            id,
            name: name.to_string(),
            abbrev: None,
            body: format!("SELECT {id}"),
            scope,
            source: Source::User,
            last_used: None,
        }
    }

    // ── applies ─────────────────────────────────────────────────────────────

    #[test]
    fn a_global_snippet_applies_everywhere() {
        let s = snip(1, "anywhere", Scope::Global);
        assert!(applies(&s, MY, 1));
        assert!(applies(&s, PG, 2));
    }

    /// The reason scope isn't a bare `conn_id`: a "find the running queries"
    /// snippet is wanted on every MySQL connection, not the one it was saved on.
    #[test]
    fn a_dialect_snippet_applies_to_every_connection_of_that_engine() {
        let s = snip(1, "processlist", Scope::Dialect(MY));
        assert!(applies(&s, MY, 1));
        assert!(applies(&s, MY, 99));
        assert!(!applies(&s, PG, 1));
    }

    #[test]
    fn a_connection_snippet_applies_only_there() {
        let s = snip(1, "prod only", Scope::Conn(7));
        assert!(applies(&s, MY, 7));
        assert!(!applies(&s, MY, 8));
    }

    /// A scope written by a newer build. It must not vanish from the file, but
    /// it also must not be shown against an engine this build can't confirm.
    #[test]
    fn an_unknown_scope_applies_nowhere() {
        let s = snip(1, "from the future", Scope::Unknown("duckdb".to_string()));
        assert!(!applies(&s, MY, 1));
        assert!(!applies(&s, PG, 1));
    }

    // ── the persisted file ──────────────────────────────────────────────────

    #[test]
    fn every_scope_survives_a_write_and_a_read_back() {
        for scope in [
            Scope::Global,
            Scope::Dialect(MY),
            Scope::Dialect(PG),
            Scope::Dialect(SqlDialect::Sqlite),
            Scope::Conn(42),
        ] {
            let s = snip(1, "x", scope.clone());
            let json = serde_json::to_string(&s).expect("write");
            let back: Snippet = serde_json::from_str(&json).expect("read");
            assert_eq!(back.scope, scope);
        }
    }

    /// The reason [`Scope::Unknown`] carries its text: this build rewrites the
    /// whole file on every change, so a scope it doesn't know has to survive the
    /// round trip **verbatim** or a rollback loses the snippet's meaning.
    #[test]
    fn a_scope_from_a_newer_build_is_written_back_unchanged() {
        let json = r#"{"id":1,"name":"x","body":"SELECT 1","scope":"duckdb"}"#;
        let s: Snippet = serde_json::from_str(json).expect("read");
        assert_eq!(s.scope, Scope::Unknown("duckdb".to_string()));
        let written = serde_json::to_string(&s).expect("write");
        assert!(
            written.contains(r#""scope":"duckdb""#),
            "the scope must go back as it came: {written}"
        );
    }

    #[test]
    fn an_unparseable_connection_scope_is_preserved_not_guessed() {
        let json = r#"{"id":1,"name":"x","body":"SELECT 1","scope":"conn:abc"}"#;
        let s: Snippet = serde_json::from_str(json).expect("read");
        assert_eq!(s.scope, Scope::Unknown("conn:abc".to_string()));
    }

    #[test]
    fn an_unknown_source_reads_as_user_rather_than_failing_the_file() {
        let json = r#"{"id":1,"name":"x","body":"SELECT 1","scope":"global","source":"vendor"}"#;
        let s: Snippet = serde_json::from_str(json).expect("read");
        assert_eq!(s.source, Source::User);
    }

    #[test]
    fn a_file_without_the_optional_fields_still_loads() {
        let json = r#"{"snippets":[{"id":1,"name":"x","body":"SELECT 1","scope":"global"}]}"#;
        let f: SnippetsFile = serde_json::from_str(json).expect("read");
        assert_eq!(f.snippets.len(), 1);
        assert_eq!(f.snippets[0].abbrev, None);
        assert_eq!(f.snippets[0].last_used, None);
        assert_eq!(f.snippets[0].source, Source::User);
    }

    // ── matches_query ───────────────────────────────────────────────────────

    #[test]
    fn an_empty_filter_matches_everything() {
        assert!(matches_query(&snip(1, "x", Scope::Global), ""));
        assert!(matches_query(&snip(1, "x", Scope::Global), "   "));
    }

    #[test]
    fn the_filter_reads_name_abbrev_and_body() {
        let mut s = snip(1, "Running queries", Scope::Global);
        s.abbrev = Some("ps".to_string());
        s.body = "SELECT * FROM information_schema.processlist".to_string();
        assert!(matches_query(&s, "running"), "name, case-insensitively");
        assert!(matches_query(&s, "ps"), "abbrev");
        assert!(matches_query(&s, "PROCESSLIST"), "body");
        assert!(!matches_query(&s, "sakila"));
    }

    #[test]
    fn the_filter_reads_a_body_across_its_newlines() {
        let mut s = snip(1, "x", Scope::Global);
        s.body = "SELECT a\nFROM t".to_string();
        assert!(
            matches_query(&s, "a from"),
            "the panel shows the body collapsed, so the filter reads it that way"
        );
    }

    // ── grouped ─────────────────────────────────────────────────────────────

    #[test]
    fn groups_are_ordered_connection_then_dialect_then_global() {
        let all = vec![
            snip(1, "global", Scope::Global),
            snip(2, "mysql", Scope::Dialect(MY)),
            snip(3, "this conn", Scope::Conn(7)),
        ];
        let buckets: Vec<Bucket> = grouped(&all, MY, 7, "")
            .into_iter()
            .map(|g| g.bucket)
            .collect();
        assert_eq!(
            buckets,
            vec![Bucket::Conn(7), Bucket::Dialect(MY), Bucket::Global]
        );
    }

    #[test]
    fn a_group_with_nothing_in_it_is_not_rendered() {
        let all = vec![snip(1, "global", Scope::Global)];
        let groups = grouped(&all, MY, 7, "");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].bucket, Bucket::Global);
    }

    #[test]
    fn a_snippet_for_another_connection_is_not_grouped_at_all() {
        let all = vec![snip(1, "elsewhere", Scope::Conn(8))];
        assert!(grouped(&all, MY, 7, "").is_empty());
    }

    #[test]
    fn a_snippet_appears_under_exactly_one_heading() {
        let all = vec![snip(1, "global", Scope::Global)];
        let groups = grouped(&all, MY, 7, "");
        let total: usize = groups.iter().map(|g| g.items.len()).sum();
        assert_eq!(total, 1, "a global snippet is not also under the engine");
    }

    /// Most-recently-used first: a library sorted alphabetically punishes you
    /// for having many.
    #[test]
    fn within_a_group_recently_used_comes_first() {
        let mut a = snip(1, "alpha", Scope::Global);
        let mut b = snip(2, "beta", Scope::Global);
        let c = snip(3, "gamma", Scope::Global);
        a.last_used = Some(100);
        b.last_used = Some(200);
        let names: Vec<String> = grouped(&[a, b, c], MY, 7, "")[0]
            .items
            .iter()
            .map(|s| s.name.clone())
            .collect();
        assert_eq!(names, ["beta", "alpha", "gamma"]);
    }

    #[test]
    fn never_used_snippets_fall_back_to_name_order() {
        let all = vec![
            snip(1, "zulu", Scope::Global),
            snip(2, "alpha", Scope::Global),
        ];
        let names: Vec<String> = grouped(&all, MY, 7, "")[0]
            .items
            .iter()
            .map(|s| s.name.clone())
            .collect();
        assert_eq!(names, ["alpha", "zulu"]);
    }

    #[test]
    fn the_filter_narrows_the_groups() {
        let all = vec![
            snip(1, "orders", Scope::Global),
            snip(2, "customers", Scope::Dialect(MY)),
        ];
        let groups = grouped(&all, MY, 7, "orders");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].items.len(), 1);
        assert_eq!(groups[0].items[0].name, "orders");
    }

    // ── the built-in pack ───────────────────────────────────────────────────

    const EVERY_DIALECT: [SqlDialect; 3] = [MY, PG, SqlDialect::Sqlite];

    /// Every shipped statement has to *parse* in the dialect it is shipped for.
    /// It cannot be run from here — there is no server in a unit test — so this
    /// is the automatic half; the SQLite ones are executed for real in
    /// `schemaic-db`, and the MySQL/PostgreSQL ones were run by hand against
    /// MariaDB 10.11, MySQL 8.4 and PostgreSQL 16 when they were written.
    #[test]
    fn every_builtin_parses_in_its_own_dialect() {
        for d in EVERY_DIALECT {
            for s in builtins(d) {
                // A body may hold `:name` placeholders; the parser accepts those
                // in value positions and `neutralize` rescues the rest, which is
                // exactly what the editor's diagnostics do with it.
                let text = crate::params::neutralize(&s.body, d);
                assert!(
                    crate::intel::parses(&text, d),
                    "{d:?} built-in {:?} does not parse:\n{}",
                    s.name,
                    s.body
                );
            }
        }
    }

    /// A starter pack is the worst place for a statement that changes something:
    /// the rows are meant to be clicked without reading them.
    #[test]
    fn no_builtin_writes_anything() {
        for d in EVERY_DIALECT {
            for s in builtins(d) {
                assert!(
                    !crate::sql::contains_write(&s.body, d),
                    "{d:?} built-in {:?} is not a plain read",
                    s.name
                );
            }
        }
    }

    #[test]
    fn builtins_are_marked_shipped_and_scoped_to_their_engine() {
        for d in EVERY_DIALECT {
            for s in builtins(d) {
                assert_eq!(s.source, Source::Builtin, "{:?}", s.name);
                assert_eq!(s.scope, Scope::Dialect(d), "{:?}", s.name);
                assert!(s.last_used.is_none(), "{:?}", s.name);
                assert!(s.id >= BUILTIN_ID_BASE, "{:?} is in the user range", s.name);
            }
        }
    }

    /// Ids have to be unique *across* the packs, not just within one: the merged
    /// library of a MySQL connection and the palette's rows are keyed by them.
    #[test]
    fn no_two_builtins_share_an_id_or_a_name_within_an_engine() {
        let mut ids: Vec<u64> = Vec::new();
        for d in EVERY_DIALECT {
            let pack = builtins(d);
            let mut names: Vec<&str> = pack.iter().map(|s| s.name.as_str()).collect();
            names.sort_unstable();
            let before = names.len();
            names.dedup();
            assert_eq!(before, names.len(), "{d:?} ships two snippets of one name");
            ids.extend(pack.iter().map(|s| s.id));
        }
        let before = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(before, ids.len(), "two built-ins share an id");
    }

    #[test]
    fn the_library_is_the_users_snippets_then_the_engines_pack() {
        let user = vec![snip(1, "mine", Scope::Global)];
        let all = library(&user, MY);
        assert_eq!(all[0].name, "mine");
        assert_eq!(all.len(), 1 + builtins(MY).len());
        assert!(all[1..].iter().all(|s| s.source == Source::Builtin));
    }

    /// The reason the user's snippets come first: `by_abbrev` breaks a
    /// same-scope tie on order, and the shipped one must not shadow the one
    /// somebody wrote.
    #[test]
    fn a_users_abbrev_wins_over_a_shipped_one() {
        let mut mine = snip(1, "my own processlist", Scope::Dialect(MY));
        mine.abbrev = Some("ps".to_string());
        let all = library(&[mine], MY);
        let hit = by_abbrev(&all, "ps", MY, 7).expect("something answers ps");
        assert_eq!(hit.name, "my own processlist");
    }

    #[test]
    fn a_builtin_id_is_never_allocated_to_a_user_snippet() {
        // Even from a merged list, which is what a careless caller would pass.
        let all = library(&[snip(4, "mine", Scope::Global)], MY);
        assert_eq!(
            next_id(&all),
            5,
            "the pack's ids must not raise the counter"
        );
    }

    #[test]
    fn the_packs_land_under_the_engine_band() {
        let groups = grouped(&library(&[], MY), MY, 7, "");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].bucket, Bucket::Dialect(MY));
        assert_eq!(groups[0].items.len(), builtins(MY).len());
        // And a PostgreSQL pack is not offered on a MySQL connection.
        assert!(
            grouped(&builtins(PG), MY, 7, "").is_empty(),
            "the other engine's pack must not show up here"
        );
    }

    // ── scope_options ───────────────────────────────────────────────────────

    #[test]
    fn the_scope_choices_are_offered_narrowest_first() {
        assert_eq!(
            scope_options(MY, 7),
            vec![Scope::Conn(7), Scope::Dialect(MY), Scope::Global]
        );
    }

    /// The picker's order and the panel's bands are the same order, and this is
    /// what keeps them so: a row moved to the second choice must land under the
    /// second heading, not somewhere else in the list.
    #[test]
    fn the_scope_choices_match_the_band_order() {
        let all: Vec<Snippet> = scope_options(MY, 7)
            .into_iter()
            .enumerate()
            .map(|(i, scope)| snip(i as u64 + 1, &format!("s{i}"), scope))
            .collect();
        let bands: Vec<Bucket> = grouped(&all, MY, 7, "")
            .into_iter()
            .map(|g| g.bucket)
            .collect();
        assert_eq!(
            bands,
            vec![Bucket::Conn(7), Bucket::Dialect(MY), Bucket::Global]
        );
        for (choice, band) in scope_options(MY, 7).into_iter().zip(bands) {
            let one = vec![snip(1, "x", choice.clone())];
            assert_eq!(
                grouped(&one, MY, 7, "")[0].bucket,
                band,
                "{choice:?} must land under {band:?}"
            );
        }
    }

    // ── by_abbrev ───────────────────────────────────────────────────────────

    #[test]
    fn an_abbrev_finds_its_snippet() {
        let mut s = snip(1, "Running queries", Scope::Dialect(MY));
        s.abbrev = Some("ps".to_string());
        let all = vec![s];
        assert!(by_abbrev(&all, "ps", MY, 7).is_some());
        assert!(by_abbrev(&all, "nope", MY, 7).is_none());
    }

    #[test]
    fn an_abbrev_out_of_scope_does_not_expand() {
        let mut s = snip(1, "Running queries", Scope::Dialect(MY));
        s.abbrev = Some("ps".to_string());
        assert!(
            by_abbrev(&[s], "ps", PG, 7).is_none(),
            "a MySQL snippet must not expand on a PostgreSQL connection"
        );
    }

    #[test]
    fn an_abbrev_is_matched_whole_and_case_insensitively() {
        let mut s = snip(1, "x", Scope::Global);
        s.abbrev = Some("ps".to_string());
        let all = vec![s];
        assert!(by_abbrev(&all, "PS", MY, 7).is_some());
        assert!(
            by_abbrev(&all, "psx", MY, 7).is_none(),
            "a prefix is not a trigger"
        );
    }

    #[test]
    fn a_snippet_without_an_abbrev_is_never_triggered() {
        let all = vec![snip(1, "x", Scope::Global)];
        assert!(by_abbrev(&all, "", MY, 7).is_none());
        assert!(by_abbrev(&all, "x", MY, 7).is_none());
    }

    #[test]
    fn the_narrowest_scope_wins_a_shared_abbrev() {
        let mut wide = snip(1, "engine-wide", Scope::Dialect(MY));
        wide.abbrev = Some("ps".to_string());
        let mut narrow = snip(2, "this connection", Scope::Conn(7));
        narrow.abbrev = Some("ps".to_string());
        let hit = by_abbrev(&[wide, narrow], "ps", MY, 7).expect("one of them");
        assert_eq!(hit.name, "this connection");
    }

    // ── ids, touch, remove ──────────────────────────────────────────────────

    #[test]
    fn ids_only_go_up() {
        assert_eq!(next_id(&[]), 1, "0 is the id no allocation returns");
        let all = vec![snip(4, "a", Scope::Global), snip(2, "b", Scope::Global)];
        assert_eq!(next_id(&all), 5);
    }

    /// Pins the limit stated on [`next_id`]: deleting the highest id frees it
    /// for the next Save. Safe only while no snippet id outlives the file it
    /// came from — if that changes, this test is the one that has to change
    /// first, and the counter has to move onto [`SnippetsFile`].
    #[test]
    fn deleting_the_highest_id_frees_it_for_reuse() {
        let mut all = vec![snip(1, "a", Scope::Global), snip(9, "b", Scope::Global)];
        assert_eq!(next_id(&all), 10);
        remove(&mut all, 9);
        assert_eq!(next_id(&all), 2, "the id 9 held is handed out again");
    }

    #[test]
    fn touching_a_snippet_records_when_it_was_used() {
        let mut all = vec![snip(1, "a", Scope::Global), snip(2, "b", Scope::Global)];
        touch(&mut all, 2, 1_234);
        assert_eq!(all[0].last_used, None, "only the one asked for");
        assert_eq!(all[1].last_used, Some(1_234));
    }

    #[test]
    fn touching_an_id_that_is_gone_does_nothing() {
        let mut all = vec![snip(1, "a", Scope::Global)];
        touch(&mut all, 99, 1_234);
        assert_eq!(all[0].last_used, None);
    }

    #[test]
    fn removing_takes_exactly_one() {
        let mut all = vec![snip(1, "a", Scope::Global), snip(2, "b", Scope::Global)];
        remove(&mut all, 1);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, 2);
    }
}
