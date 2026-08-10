//! An order-preserving, editable JSON tree for the row panel's JSON-column widget.
//!
//! A `JSON`/`JSONB` column comes back as text; this parses it into a [`JsonNode`]
//! tree the UI renders as a collapsible outline (one row per scalar / container),
//! lets the user edit a leaf **by path**, and re-serialises. Pure + unit-tested —
//! the UI is a thin reactive shell that owns only collapse state.
//!
//! **Object key order is preserved** (a JSON editor that reshuffled keys would be
//! maddening). `serde_json`'s `Value` sorts object keys unless the `preserve_order`
//! feature is on, so instead of going through `Value` we walk the document as
//! `RawValue`s, pushing object entries in the order the map visitor yields them —
//! document order, duplicates and all.
//!
//! **Numbers keep their source text, byte for byte.** They used to keep a
//! *canonical* form, which meant every number that isn't an exact `i64`/`u64`
//! round-tripped through an `f64`: `10.00` came back `10`, `1e3` came back `1000`,
//! and a 21-digit integer came back a different integer. Since editing one leaf
//! re-serialises the whole tree into the write, that silently rewrote numbers the
//! user never touched — so the raw text is what the tree stores. A leaf is
//! displayed and edited as its
//! **JSON scalar** (`"Kabul"`, `34.5`, `true`, `null`) — unambiguous about string
//! vs number vs null, and type changes fall out for free (`set_leaf` parses any
//! JSON value, even replacing a scalar with a nested object).

use std::fmt;

use serde::Deserialize;
use serde::de::{Deserializer, MapAccess, Visitor};
use serde_json::value::RawValue;

/// One node of a JSON document. Objects keep their entries in insertion (document)
/// order; numbers keep a canonical text form to avoid a float round-trip changing
/// how they read.
#[derive(Clone, Debug, PartialEq)]
pub enum JsonNode {
    Null,
    Bool(bool),
    /// A number in canonical text form (e.g. `34.5`, `-7`, `1000`).
    Num(String),
    Str(String),
    Array(Vec<JsonNode>),
    /// Object members, in document order.
    Object(Vec<(String, JsonNode)>),
}

/// One step of a path into a [`JsonNode`] tree: an object member or an array
/// element, both **by position**.
///
/// A member is addressed by its ordinal in document order rather than by key,
/// because JSON permits duplicate keys and PostgreSQL's `json` type (which stores
/// the text verbatim, unlike `jsonb` or MySQL's `JSON`) can hand one back. Keying
/// a path by name made `{"a":1,"a":2}` yield two rows with the *same* path, so
/// editing the second wrote the first and the two were indistinguishable to the
/// panel's collapse/edit state. The member's key is on [`TreeRow::label`], which
/// is what the UI displays.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PathSeg {
    /// Object member, by its index in the entry vector (document order).
    Member(usize),
    /// Array element, by index.
    Index(usize),
}

/// The kind of a flattened tree row (drives the UI's glyph + whether it collapses).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowKind {
    /// An object with N members (collapsible).
    Object(usize),
    /// An array with N elements (collapsible).
    Array(usize),
    /// A scalar leaf (not collapsible).
    Scalar,
}

/// One row of the flattened tree, in pre-order. `label` is the object key for an
/// object member, or `None` for an array element (the UI shows its index from the
/// last path segment). `value_json` is the leaf's editable JSON scalar text (`None`
/// for a container).
#[derive(Clone, Debug, PartialEq)]
pub struct TreeRow {
    pub path: Vec<PathSeg>,
    pub depth: usize,
    pub label: Option<String>,
    pub kind: RowKind,
    pub value_json: Option<String>,
}

impl JsonNode {
    /// Parse JSON text into a tree (object key order preserved). Returns the parse
    /// error message on invalid JSON.
    pub fn parse(s: &str) -> Result<JsonNode, String> {
        let raw: &RawValue = serde_json::from_str(s).map_err(|e| e.to_string())?;
        from_raw(raw)
    }

    /// Is this a scalar (non-container) node?
    pub fn is_scalar(&self) -> bool {
        !matches!(self, JsonNode::Array(_) | JsonNode::Object(_))
    }

    /// The compact JSON text of a **scalar** node (`"Kabul"`, `34.5`, `true`,
    /// `null`), or `None` for a container. This is what a leaf row displays and
    /// edits.
    pub fn scalar_text(&self) -> Option<String> {
        self.is_scalar().then(|| self.to_compact())
    }

    /// Compact (single-line) serialisation, object key order preserved.
    pub fn to_compact(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, None, 0);
        out
    }

    /// Pretty (multi-line, `indent`-space) serialisation, key order preserved.
    pub fn to_pretty(&self, indent: usize) -> String {
        let mut out = String::new();
        self.write(&mut out, Some(indent), 0);
        out
    }

    /// Serialise into `out`. `pretty` = `Some(indent_width)` for multi-line, `None`
    /// for compact. `level` is the current nesting depth.
    fn write(&self, out: &mut String, pretty: Option<usize>, level: usize) {
        let nl_indent = |out: &mut String, lvl: usize| {
            if let Some(w) = pretty {
                out.push('\n');
                for _ in 0..w * lvl {
                    out.push(' ');
                }
            }
        };
        match self {
            JsonNode::Null => out.push_str("null"),
            JsonNode::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            JsonNode::Num(n) => out.push_str(n),
            JsonNode::Str(s) => out.push_str(&encode_str(s)),
            JsonNode::Array(items) => {
                if items.is_empty() {
                    out.push_str("[]");
                    return;
                }
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    nl_indent(out, level + 1);
                    item.write(out, pretty, level + 1);
                }
                nl_indent(out, level);
                out.push(']');
            }
            JsonNode::Object(entries) => {
                if entries.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push('{');
                for (i, (k, v)) in entries.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    nl_indent(out, level + 1);
                    out.push_str(&encode_str(k));
                    out.push(':');
                    if pretty.is_some() {
                        out.push(' ');
                    }
                    v.write(out, pretty, level + 1);
                }
                nl_indent(out, level);
                out.push('}');
            }
        }
    }

    /// Borrow the node at `path` (empty path = the root).
    pub fn get(&self, path: &[PathSeg]) -> Option<&JsonNode> {
        let Some((seg, rest)) = path.split_first() else {
            return Some(self);
        };
        let child = match (self, seg) {
            (JsonNode::Object(entries), PathSeg::Member(i)) => entries.get(*i).map(|(_, v)| v),
            (JsonNode::Array(items), PathSeg::Index(i)) => items.get(*i),
            _ => None,
        }?;
        child.get(rest)
    }

    /// Mutably borrow the node at `path` (empty path = the root).
    pub fn get_mut(&mut self, path: &[PathSeg]) -> Option<&mut JsonNode> {
        let Some((seg, rest)) = path.split_first() else {
            return Some(self);
        };
        let child = match (self, seg) {
            (JsonNode::Object(entries), PathSeg::Member(i)) => entries.get_mut(*i).map(|(_, v)| v),
            (JsonNode::Array(items), PathSeg::Index(i)) => items.get_mut(*i),
            _ => None,
        }?;
        child.get_mut(rest)
    }

    /// Replace the node at `path` with `node`. Returns `false` if the path doesn't
    /// resolve (leaving the tree unchanged).
    pub fn set(&mut self, path: &[PathSeg], node: JsonNode) -> bool {
        match self.get_mut(path) {
            Some(slot) => {
                *slot = node;
                true
            }
            None => false,
        }
    }

    /// Replace the **leaf** at `path` from edited JSON `text` (any valid JSON value —
    /// `"str"`, a number, `true`/`false`, `null`, or even a nested object/array).
    /// Errors on invalid JSON or an unresolvable path.
    pub fn set_leaf(&mut self, path: &[PathSeg], text: &str) -> Result<(), String> {
        let node = JsonNode::parse(text)?;
        if self.set(path, node) {
            Ok(())
        } else {
            Err("path not found".to_string())
        }
    }

    /// Flatten to display rows in pre-order (root first, then its members depth-first),
    /// each carrying its `path`, `depth`, `label` (object key / `None` for an array
    /// element), `kind`, and — for a scalar — its editable JSON `value_json`.
    pub fn rows(&self) -> Vec<TreeRow> {
        let mut out = Vec::new();
        self.collect_rows(&mut Vec::new(), 0, None, &mut out);
        out
    }

    fn collect_rows(
        &self,
        path: &mut Vec<PathSeg>,
        depth: usize,
        label: Option<String>,
        out: &mut Vec<TreeRow>,
    ) {
        let kind = match self {
            JsonNode::Object(e) => RowKind::Object(e.len()),
            JsonNode::Array(a) => RowKind::Array(a.len()),
            _ => RowKind::Scalar,
        };
        out.push(TreeRow {
            path: path.clone(),
            depth,
            label,
            kind,
            value_json: self.scalar_text(),
        });
        match self {
            JsonNode::Object(entries) => {
                for (i, (k, v)) in entries.iter().enumerate() {
                    path.push(PathSeg::Member(i));
                    v.collect_rows(path, depth + 1, Some(k.clone()), out);
                    path.pop();
                }
            }
            JsonNode::Array(items) => {
                for (i, item) in items.iter().enumerate() {
                    path.push(PathSeg::Index(i));
                    item.collect_rows(path, depth + 1, None, out);
                    path.pop();
                }
            }
            _ => {}
        }
    }
}

/// JSON-encode a string (quotes + escapes) via `serde_json`, falling back to a bare
/// pair of quotes on the impossible error path.
fn encode_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

/// Object members in document order, values left **unparsed**.
///
/// `serde_json::Value` would sort the keys (without `preserve_order`) and drop
/// duplicates, and a plain `Vec<(String, _)>` doesn't deserialize from a JSON
/// map at all — hence the visitor, which pushes entries in the order
/// `next_entry` yields them.
struct RawObject<'a>(Vec<(String, &'a RawValue)>);

impl<'de> Deserialize<'de> for RawObject<'de> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ObjVisitor;

        impl<'de> Visitor<'de> for ObjVisitor {
            type Value = RawObject<'de>;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a JSON object")
            }

            fn visit_map<A>(self, mut map: A) -> Result<RawObject<'de>, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut entries = Vec::new();
                while let Some((k, v)) = map.next_entry::<String, &'de RawValue>()? {
                    entries.push((k, v));
                }
                Ok(RawObject(entries))
            }
        }

        deserializer.deserialize_map(ObjVisitor)
    }
}

/// Build a node from an already-validated raw JSON value, **keeping a number's
/// source text byte for byte**.
///
/// Going through serde's `visit_f64` (or `serde_json::Value`) sends every number
/// that isn't an exact `i64`/`u64` through an `f64` and back out in Rust's
/// shortest round-trip form: `10.00` became `10`, `1e3` became `1000`, and a
/// 21-digit integer became a *different* integer. That was invisible until Save,
/// which rewrote every number in a JSON column the user had merely browsed.
///
/// The raw text is only ever inspected at its first byte — the value has already
/// been parsed and validated by `serde_json`, so the leading byte determines the
/// kind unambiguously.
fn from_raw(raw: &RawValue) -> Result<JsonNode, String> {
    let text = raw.get().trim();
    let err = |e: serde_json::Error| e.to_string();
    match text.as_bytes().first() {
        Some(b'{') => {
            let RawObject(entries) = serde_json::from_str(text).map_err(err)?;
            let mut out = Vec::with_capacity(entries.len());
            for (k, v) in entries {
                out.push((k, from_raw(v)?));
            }
            Ok(JsonNode::Object(out))
        }
        Some(b'[') => {
            let items: Vec<&RawValue> = serde_json::from_str(text).map_err(err)?;
            items
                .into_iter()
                .map(from_raw)
                .collect::<Result<Vec<_>, _>>()
                .map(JsonNode::Array)
        }
        // Escapes and `\uXXXX` stay serde_json's job.
        Some(b'"') => serde_json::from_str::<String>(text)
            .map(JsonNode::Str)
            .map_err(err),
        Some(b't') | Some(b'f') => serde_json::from_str::<bool>(text)
            .map(JsonNode::Bool)
            .map_err(err),
        Some(b'n') => Ok(JsonNode::Null),
        // `-` or a digit: a number, kept exactly as written.
        _ => Ok(JsonNode::Num(text.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Object member by ordinal — `m(0)` is the first entry in document order.
    fn m(i: usize) -> PathSeg {
        PathSeg::Member(i)
    }

    #[test]
    fn parse_preserves_object_key_order() {
        // Keys deliberately out of alphabetical order.
        let n = JsonNode::parse(r#"{"zebra":1,"apple":2,"mango":3}"#).unwrap();
        let JsonNode::Object(entries) = &n else {
            panic!("expected object")
        };
        let keys: Vec<&str> = entries.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["zebra", "apple", "mango"], "document order kept");
    }

    #[test]
    fn compact_round_trips_and_keeps_order() {
        let src = r#"{"b":1,"a":[true,null,"x"],"c":{"n":2.5}}"#;
        let n = JsonNode::parse(src).unwrap();
        // Compact re-serialisation matches the (order-preserving) input exactly.
        assert_eq!(n.to_compact(), src);
    }

    #[test]
    fn pretty_indents_nested_structure() {
        let n = JsonNode::parse(r#"{"a":1,"b":[2,3]}"#).unwrap();
        let p = n.to_pretty(2);
        assert_eq!(p, "{\n  \"a\": 1,\n  \"b\": [\n    2,\n    3\n  ]\n}");
        // Pretty re-parses to the same tree.
        assert_eq!(JsonNode::parse(&p).unwrap(), n);
    }

    #[test]
    fn empty_containers_serialise_inline() {
        assert_eq!(JsonNode::parse("{}").unwrap().to_pretty(2), "{}");
        assert_eq!(JsonNode::parse("[]").unwrap().to_pretty(2), "[]");
    }

    #[test]
    fn strings_are_escaped() {
        let n = JsonNode::Str("a\"b\nc".to_string());
        assert_eq!(n.to_compact(), "\"a\\\"b\\nc\"");
        assert_eq!(JsonNode::parse(&n.to_compact()).unwrap(), n);
    }

    #[test]
    fn get_and_set_by_path() {
        let mut n = JsonNode::parse(r#"{"user":{"name":"Kabul","tags":["a","b"]}}"#).unwrap();
        // user → name
        let path = vec![m(0), m(0)];
        assert_eq!(n.get(&path), Some(&JsonNode::Str("Kabul".into())));
        // Set a nested leaf.
        assert!(n.set(&path, JsonNode::Str("Herat".into())));
        assert_eq!(n.get(&path), Some(&JsonNode::Str("Herat".into())));
        // Array index path: user → tags[1]
        let ap = vec![m(0), m(1), PathSeg::Index(1)];
        assert_eq!(n.get(&ap), Some(&JsonNode::Str("b".into())));
        // A bad path leaves the tree untouched and returns false.
        assert!(!n.set(&[m(9)], JsonNode::Null));
    }

    #[test]
    fn set_leaf_parses_json_and_allows_type_change() {
        let mut n = JsonNode::parse(r#"{"v":"old"}"#).unwrap();
        let p = vec![m(0)];
        // Edit a string leaf to a number (type change via JSON text).
        n.set_leaf(&p, "42").unwrap();
        assert_eq!(n.get(&p), Some(&JsonNode::Num("42".into())));
        // Replace with a nested object.
        n.set_leaf(&p, r#"{"x":true}"#).unwrap();
        assert_eq!(n.to_compact(), r#"{"v":{"x":true}}"#);
        // Invalid JSON is an error, tree unchanged.
        assert!(n.set_leaf(&p, "{not json").is_err());
        assert_eq!(n.to_compact(), r#"{"v":{"x":true}}"#);
    }

    #[test]
    fn rows_flatten_in_preorder_with_paths_and_leaves() {
        let n = JsonNode::parse(r#"{"a":1,"b":["x",{"c":2}]}"#).unwrap();
        let rows = n.rows();
        // root, a, b, b[0], b[1], b[1].c
        assert_eq!(rows.len(), 6);
        // Root object with 2 members.
        assert_eq!(rows[0].depth, 0);
        assert_eq!(rows[0].kind, RowKind::Object(2));
        assert!(rows[0].value_json.is_none());
        // "a": scalar leaf 1.
        assert_eq!(rows[1].label.as_deref(), Some("a"));
        assert_eq!(rows[1].kind, RowKind::Scalar);
        assert_eq!(rows[1].value_json.as_deref(), Some("1"));
        // "b": array of 2.
        assert_eq!(rows[2].label.as_deref(), Some("b"));
        assert_eq!(rows[2].kind, RowKind::Array(2));
        // b[0]: array element, no label, value "x".
        assert_eq!(rows[3].label, None);
        assert_eq!(rows[3].path, vec![m(1), PathSeg::Index(0)]);
        assert_eq!(rows[3].value_json.as_deref(), Some("\"x\""));
        // b[1]: object; b[1].c: leaf 2 at depth 3.
        assert_eq!(rows[4].kind, RowKind::Object(1));
        assert_eq!(rows[5].label.as_deref(), Some("c"));
        assert_eq!(rows[5].depth, 3);
        assert_eq!(rows[5].value_json.as_deref(), Some("2"));
    }

    #[test]
    fn duplicate_object_keys_get_distinct_paths() {
        // A PostgreSQL `json` column stores its text verbatim, so it really can
        // hold `{"a":1,"a":2}` (both `jsonb` and MySQL's `JSON` drop the first).
        // Keyed by name, the two members shared one path: editing the second wrote
        // the first, and the panel's collapse/edit state couldn't tell them apart.
        let mut n = JsonNode::parse(r#"{"a":1,"a":2}"#).unwrap();
        let rows = n.rows();
        assert_eq!(rows.len(), 3); // root + both members
        assert_eq!(rows[1].label.as_deref(), Some("a"));
        assert_eq!(rows[2].label.as_deref(), Some("a"));
        assert_ne!(rows[1].path, rows[2].path);
        assert_eq!(rows[1].value_json.as_deref(), Some("1"));
        assert_eq!(rows[2].value_json.as_deref(), Some("2"));
        // Editing the second leaves the first alone.
        n.set_leaf(&rows[2].path, "9").unwrap();
        assert_eq!(n.to_compact(), r#"{"a":1,"a":9}"#);
    }

    #[test]
    fn scalar_root_is_a_single_row() {
        let n = JsonNode::parse("\"just a string\"").unwrap();
        let rows = n.rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, RowKind::Scalar);
        assert_eq!(rows[0].value_json.as_deref(), Some("\"just a string\""));
    }

    #[test]
    fn numbers_keep_their_source_text() {
        // This test used to assert the same property over `[7,-3,34.5,1000]` —
        // four values chosen from exactly the set an `f64` round-trip survives, so
        // it passed while every number below came back changed. Editing one leaf
        // of a JSON column rewrote all of them, silently, in the row it wrote.
        for src in [
            "[7,-3,34.5,1000]",
            "10.00",
            "1.0",
            "1e3",
            "1.5E-7",
            "-0.0",
            // Past `f64`'s 17 significant digits, and past `u64::MAX`.
            "0.1234567890123456789",
            "123456789012345678901",
            "18446744073709551616",
            "123456789012345678.12345678",
            r#"{"price":10.00,"note":"x"}"#,
            r#"[{"a":1.0},[2.50,3e2]]"#,
        ] {
            let n = JsonNode::parse(src).unwrap_or_else(|e| panic!("{src}: {e}"));
            assert_eq!(n.to_compact(), src, "{src} must round-trip unchanged");
        }
    }

    #[test]
    fn a_number_the_user_types_keeps_the_form_they_typed() {
        // `set_leaf` goes through the same parse, so the editor can't normalise a
        // value behind the user either.
        let mut n = JsonNode::parse(r#"{"price":1}"#).unwrap();
        let rows = n.rows();
        n.set_leaf(&rows[1].path, "10.00").unwrap();
        assert_eq!(n.to_compact(), r#"{"price":10.00}"#);
    }

    #[test]
    fn numbers_are_still_parsed_as_numbers_not_strings() {
        // The text is kept, but the node is a `Num` — a leaf must not start
        // rendering or editing as a quoted string.
        let n = JsonNode::parse("10.00").unwrap();
        assert_eq!(n, JsonNode::Num("10.00".to_string()));
        assert_eq!(n.scalar_text().as_deref(), Some("10.00"));
    }

    #[test]
    fn malformed_numbers_are_still_rejected() {
        for bad in ["01", "1.", ".5", "+1", "1e", "--1", "0x10", "NaN", "1e+"] {
            assert!(
                JsonNode::parse(bad).is_err(),
                "{bad} is not valid JSON and must not parse"
            );
        }
    }
}
