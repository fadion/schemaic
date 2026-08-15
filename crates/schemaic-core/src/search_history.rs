//! Per-connection **search history** for the Find-Anywhere palette — the pure,
//! testable model. Mirrors [`crate::history`] (query history): a flat
//! `Vec<SearchEntry>` keyed by a `conn_id` field, newest-first, deduped, and
//! capped per connection. Persisted to `search_history.json`.
//!
//! An entry is recorded only when a result is **activated** (clicked / Enter),
//! not merely surfaced by a search. The palette shows the recent entries for the
//! active connection when it opens with an empty query.

use serde::{Deserialize, Serialize};

/// How many entries to keep (and show) per connection.
pub const MAX_PER_CONN: usize = 10;

/// Which kind of PostgreSQL standalone object an entry points at, when it points
/// at one rather than at a table.
///
/// A tag of its own rather than [`crate::ddl::ObjectKind`] because this one is
/// **persisted**: a kind written by a newer build must not fail to parse and take
/// every connection's history with it — the rule `SshAuth` and `Environment`
/// follow in [`crate::connection`]. An unrecognised kind resolves to no object,
/// so the row is dropped from the recents list the same way an entry for a
/// since-renamed table is.
///
/// [`ObjectTag::Unknown`] **keeps the text it didn't recognise**, and the hand-
/// written `Serialize` writes it back verbatim. A bare unit variant with
/// `#[serde(other)]` was the obvious spelling and is silently destructive: the
/// app rewrites the whole of `search_history.json` on every change, so merely
/// running this build once would have rewritten a newer build's `"collation"` as
/// the literal `"unknown"`, and going back would no longer recognise its own
/// entry. Degrading the *file* is the point; degrading the *value* is a
/// different thing that happens to look like it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObjectTag {
    Enum,
    Domain,
    Sequence,
    /// A kind this build doesn't know, preserved exactly as it was read.
    Unknown(String),
}

impl ObjectTag {
    /// The live kind this tag names, or `None` for one this build doesn't know.
    pub fn kind(&self) -> Option<crate::ddl::ObjectKind> {
        match self {
            ObjectTag::Enum => Some(crate::ddl::ObjectKind::Enum),
            ObjectTag::Domain => Some(crate::ddl::ObjectKind::Domain),
            ObjectTag::Sequence => Some(crate::ddl::ObjectKind::Sequence),
            ObjectTag::Unknown(_) => None,
        }
    }

    /// The tag for a live kind.
    pub fn of(kind: crate::ddl::ObjectKind) -> Self {
        match kind {
            crate::ddl::ObjectKind::Enum => ObjectTag::Enum,
            crate::ddl::ObjectKind::Domain => ObjectTag::Domain,
            crate::ddl::ObjectKind::Sequence => ObjectTag::Sequence,
        }
    }

    /// The persisted spelling — what a known kind is written as, or verbatim what
    /// an unknown one arrived as.
    fn as_str(&self) -> &str {
        match self {
            ObjectTag::Enum => "enum",
            ObjectTag::Domain => "domain",
            ObjectTag::Sequence => "sequence",
            ObjectTag::Unknown(s) => s,
        }
    }
}

impl Serialize for ObjectTag {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ObjectTag {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(match s.as_str() {
            "enum" => ObjectTag::Enum,
            "domain" => ObjectTag::Domain,
            "sequence" => ObjectTag::Sequence,
            _ => ObjectTag::Unknown(s),
        })
    }
}

/// One activated search result: a table (`column: None`), a specific column, or —
/// when `object` is set — a PostgreSQL enum / domain / sequence, whose name is
/// carried in `table`.
///
/// The name shares the `table` field rather than getting one of its own so that
/// every file written before objects were searchable still loads unchanged; the
/// `object` tag is what says how to read it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchEntry {
    pub conn_id: u64,
    pub database: String,
    /// PostgreSQL namespace of `table` (`None` on MySQL, and on entries written
    /// before multi-schema browsing existed — `#[serde(default)]` keeps those
    /// files loading).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    pub table: String,
    #[serde(default)]
    pub column: Option<String>,
    /// Set when this entry names a standalone object rather than a table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<ObjectTag>,
}

/// Persisted shape: `{ "entries": [...] }` (matches `HistoryFile`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SearchHistoryFile {
    #[serde(default)]
    pub entries: Vec<SearchEntry>,
}

impl SearchEntry {
    /// Same target, **`conn_id` included** — used for dedup. The connection is
    /// part of the identity on purpose: without it, activating the same table on
    /// two connections would collapse into one entry. So is `object`: a type and
    /// a table may share a name in one namespace, and they are different places
    /// to go back to.
    fn same_target(&self, other: &SearchEntry) -> bool {
        self.conn_id == other.conn_id
            && self.database == other.database
            && self.schema == other.schema
            && self.table == other.table
            && self.column == other.column
            && self.object == other.object
    }
}

/// Record an activated result: drop any prior identical entry (so re-selecting
/// bubbles it to the top with the latest position), insert at the front, then cap
/// **this** connection's entries at [`MAX_PER_CONN`] (other connections untouched).
pub fn push(entries: &mut Vec<SearchEntry>, entry: SearchEntry) {
    entries.retain(|e| !e.same_target(&entry));
    let conn = entry.conn_id;
    entries.insert(0, entry);
    // Cap only this connection's entries; leave others in place.
    let mut kept = 0;
    entries.retain(|e| {
        if e.conn_id != conn {
            return true;
        }
        kept += 1;
        kept <= MAX_PER_CONN
    });
}

/// The recent entries for `conn`, newest-first, up to [`MAX_PER_CONN`].
pub fn recent(entries: &[SearchEntry], conn: u64) -> Vec<SearchEntry> {
    entries
        .iter()
        .filter(|e| e.conn_id == conn)
        .take(MAX_PER_CONN)
        .cloned()
        .collect()
}

/// Forget every entry for one connection.
pub fn clear_conn(entries: &mut Vec<SearchEntry>, conn_id: u64) {
    entries.retain(|e| e.conn_id != conn_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(conn: u64, table: &str, column: Option<&str>) -> SearchEntry {
        SearchEntry {
            conn_id: conn,
            database: "db".into(),
            schema: None,
            table: table.into(),
            column: column.map(|c| c.into()),
            object: None,
        }
    }

    #[test]
    fn same_named_tables_in_two_schemas_are_separate_entries() {
        // Without the namespace in the identity, opening `sales.orders` would
        // silently replace the `public.orders` entry (and vice versa).
        let mut v = Vec::new();
        let mk = |ns: &str| SearchEntry {
            schema: Some(ns.into()),
            ..entry(1, "orders", None)
        };
        push(&mut v, mk("public"));
        push(&mut v, mk("sales"));
        assert_eq!(v.len(), 2);
        // Re-pushing one of them dedups it to the front without dropping the other.
        push(&mut v, mk("public"));
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].schema.as_deref(), Some("public"));
    }

    #[test]
    fn push_inserts_newest_first() {
        let mut v = Vec::new();
        push(&mut v, entry(1, "a", None));
        push(&mut v, entry(1, "b", None));
        assert_eq!(
            recent(&v, 1),
            vec![entry(1, "b", None), entry(1, "a", None)]
        );
    }

    #[test]
    fn push_dedups_and_bubbles_to_top() {
        let mut v = Vec::new();
        push(&mut v, entry(1, "a", None));
        push(&mut v, entry(1, "b", None));
        push(&mut v, entry(1, "a", None)); // re-select "a"
        assert_eq!(
            recent(&v, 1),
            vec![entry(1, "a", None), entry(1, "b", None)]
        );
        assert_eq!(v.len(), 2); // no duplicate
    }

    #[test]
    fn table_and_column_are_distinct_targets() {
        let mut v = Vec::new();
        push(&mut v, entry(1, "users", None));
        push(&mut v, entry(1, "users", Some("email")));
        assert_eq!(recent(&v, 1).len(), 2);
    }

    /// A PostgreSQL enum and a table may share a name in one namespace, and they
    /// are different things to go back to. Without the kind in the identity,
    /// activating one would silently replace the other's entry.
    #[test]
    fn an_object_and_a_same_named_table_are_distinct_targets() {
        let mut v = Vec::new();
        push(&mut v, entry(1, "status", None));
        push(
            &mut v,
            SearchEntry {
                object: Some(ObjectTag::Enum),
                ..entry(1, "status", None)
            },
        );
        assert_eq!(recent(&v, 1).len(), 2);
        // And two kinds of object under one name stay apart too.
        push(
            &mut v,
            SearchEntry {
                object: Some(ObjectTag::Domain),
                ..entry(1, "status", None)
            },
        );
        assert_eq!(recent(&v, 1).len(), 3);
    }

    #[test]
    fn an_object_entry_dedups_and_bubbles_like_any_other() {
        let mut v = Vec::new();
        let obj = |name: &str| SearchEntry {
            object: Some(ObjectTag::Sequence),
            ..entry(1, name, None)
        };
        push(&mut v, obj("a"));
        push(&mut v, obj("b"));
        push(&mut v, obj("a"));
        assert_eq!(recent(&v, 1), vec![obj("a"), obj("b")]);
        assert_eq!(v.len(), 2);
    }

    /// An entry written before objects were searchable has no `object` field —
    /// it must still load, as a table.
    #[test]
    fn a_file_without_the_object_field_still_loads_as_a_table() {
        let f: SearchHistoryFile =
            serde_json::from_str(r#"{"entries":[{"conn_id":1,"database":"db","table":"users"}]}"#)
                .expect("legacy entry loads");
        assert_eq!(f.entries.len(), 1);
        assert_eq!(f.entries[0].object, None);
    }

    /// A kind written by a *newer* build degrades rather than failing the whole
    /// file and losing every connection's history — the rule `SshAuth` and
    /// `Environment` follow. It resolves to no live kind, so the row is simply
    /// dropped from the recents list, exactly as a table that has since been
    /// renamed is.
    #[test]
    fn an_unknown_object_kind_degrades_instead_of_failing_the_file() {
        let f: SearchHistoryFile = serde_json::from_str(
            r#"{"entries":[{"conn_id":1,"database":"db","table":"x","object":"collation"},
                           {"conn_id":1,"database":"db","table":"y"}]}"#,
        )
        .expect("a newer kind must not fail the file");
        assert_eq!(f.entries.len(), 2, "the sibling entry survives");
        assert_eq!(
            f.entries[0].object,
            Some(ObjectTag::Unknown("collation".into()))
        );
        assert_eq!(f.entries[0].object.as_ref().and_then(|t| t.kind()), None);
    }

    /// Why `Unknown` keeps its text: the app rewrites the *entire* file on every
    /// change, so an unrecognised kind has to survive a load-and-save by this
    /// build untouched. Writing back a bare `"unknown"` would destroy a newer
    /// build's entry merely by running this one once.
    #[test]
    fn an_unknown_object_kind_is_written_back_verbatim() {
        let src = r#"{"entries":[{"conn_id":1,"database":"db","table":"x","object":"collation"}]}"#;
        let f: SearchHistoryFile = serde_json::from_str(src).unwrap();
        let out = serde_json::to_string(&f).unwrap();
        assert!(
            out.contains(r#""object":"collation""#),
            "the original kind must survive a round trip, got {out}"
        );
        assert!(!out.contains("unknown"));
    }

    #[test]
    fn object_tags_round_trip_through_json() {
        for tag in [
            ObjectTag::Enum,
            ObjectTag::Domain,
            ObjectTag::Sequence,
            ObjectTag::Unknown("collation".into()),
        ] {
            let e = SearchEntry {
                object: Some(tag.clone()),
                ..entry(1, "x", None)
            };
            let s = serde_json::to_string(&e).unwrap();
            assert_eq!(
                serde_json::from_str::<SearchEntry>(&s).unwrap(),
                e,
                "{tag:?}"
            );
        }
    }

    #[test]
    fn push_caps_per_connection() {
        let mut v = Vec::new();
        for i in 0..15 {
            push(&mut v, entry(1, &format!("t{i}"), None));
        }
        let r = recent(&v, 1);
        assert_eq!(r.len(), MAX_PER_CONN);
        assert_eq!(r[0], entry(1, "t14", None)); // newest kept
        assert_eq!(r[MAX_PER_CONN - 1], entry(1, "t5", None)); // t0..t4 dropped
    }

    #[test]
    fn cap_is_per_connection_not_global() {
        let mut v = Vec::new();
        for i in 0..15 {
            push(&mut v, entry(1, &format!("t{i}"), None));
        }
        push(&mut v, entry(2, "other", None));
        assert_eq!(recent(&v, 1).len(), MAX_PER_CONN); // conn 1 still capped
        assert_eq!(recent(&v, 2), vec![entry(2, "other", None)]); // conn 2 kept
    }

    #[test]
    fn recent_filters_by_connection() {
        let mut v = Vec::new();
        push(&mut v, entry(1, "a", None));
        push(&mut v, entry(2, "b", None));
        assert_eq!(recent(&v, 1), vec![entry(1, "a", None)]);
        assert_eq!(recent(&v, 2), vec![entry(2, "b", None)]);
        assert!(recent(&v, 3).is_empty());
    }

    #[test]
    fn clear_conn_only_clears_that_connection() {
        let mut v = Vec::new();
        push(&mut v, entry(1, "a", None));
        push(&mut v, entry(2, "b", None));
        clear_conn(&mut v, 1);
        assert!(recent(&v, 1).is_empty());
        assert_eq!(recent(&v, 2), vec![entry(2, "b", None)]);
    }
}
