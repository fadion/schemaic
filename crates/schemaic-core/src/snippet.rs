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
    all.iter().map(|s| s.id).max().unwrap_or(0) + 1
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
