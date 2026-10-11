//! Find Anywhere's **fourth tier**: objects whose stored *body* contains the
//! search term — a view's `SELECT`, a routine's or an event's body, a trigger's
//! statement.
//!
//! A different question from the three name tiers, which all ask
//! [`crate::schema::object_name_matches`]: "does this name contain the term"
//! versus "does this definition". So it is not routed through that predicate,
//! and the invariant that every schema-search surface matches *names* through it
//! is untouched — nothing here answers for a name.
//!
//! Everything it reads is already in the loaded [`DbSchema`]: every engine's
//! reader carries the bodies (see the struct's own doc), so the tier costs no
//! round trip. What it does cost is a scan of every body on every keystroke —
//! the palette is undebounced — which is why it waits for
//! [`MIN_TERM_CHARS`] characters and stops at the caller's `limit`.
//!
//! The match is a plain case-insensitive substring over the body **as stored**,
//! comments and string literals included: a body search is how you find the
//! routine that mentions a table in a comment, and a lexer-aware search would
//! hide exactly that.

use std::sync::Arc;

use crate::schema::{DbSchema, EventInfo, RoutineInfo, TriggerAction, object_name_matches};

/// How many characters the term must have before bodies are searched. One or
/// two letters match nearly every body in a database, so the tier would be
/// noise that costs a full scan per keystroke.
pub const MIN_TERM_CHARS: usize = 3;

/// The palette's cap on body rows — the tier sits below the name tiers, and a
/// term common enough to fill it is better narrowed than scrolled.
pub const BODY_HIT_CAP: usize = 50;

/// The longest snippet a row shows, in characters, ellipses excluded.
pub const SNIPPET_CHARS: usize = 60;

/// How many characters of the line a snippet keeps before the match when the
/// line has to be cut, so the hit is never pushed off the row's end.
const SNIPPET_LEAD: usize = 20;

/// The object a body hit belongs to — what the palette opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BodyOwner {
    /// A view, opened as the name tier opens one: as a table.
    View {
        schema: Option<String>,
        name: String,
    },
    /// A function or procedure, opened in its editor.
    Routine(Arc<RoutineInfo>),
    /// A MySQL/MariaDB scheduled event, opened in its editor.
    Event(Arc<EventInfo>),
    /// A trigger. The finder has no trigger destination of its own, so the
    /// palette opens the table (or view) it fires on.
    Trigger {
        schema: Option<String>,
        table: String,
        name: String,
    },
}

/// One body hit: the object, and the line the term was found on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BodyHit {
    pub owner: BodyOwner,
    /// The matching line, trimmed, its whitespace runs collapsed, and clamped
    /// to [`SNIPPET_CHARS`] around the match ([`body_snippet`]).
    pub snippet: String,
}

/// Is `needle` long enough to search bodies for? Counted in characters, not
/// bytes, so a three-letter Cyrillic term qualifies as an ASCII one does.
pub fn searches_bodies(needle: &str) -> bool {
    needle.chars().count() >= MIN_TERM_CHARS
}

/// The objects in `schema` whose body contains `needle`, at most `limit` of
/// them, each with a snippet of the matching line.
///
/// **An object a name tier already lists is skipped** — a view, routine or
/// event whose own name matches through [`object_name_matches`], which is the
/// predicate those tiers use. A trigger is never skipped: no name tier lists
/// triggers, so its body is the only way the palette reaches it.
///
/// Order: views, then triggers (by their table), then routines, then events —
/// catalogue order within each. Nothing is returned for a term shorter than
/// [`MIN_TERM_CHARS`].
pub fn body_hits(schema: &DbSchema, needle: &str, limit: usize) -> Vec<BodyHit> {
    let mut out = Vec::new();
    if limit == 0 || !searches_bodies(needle) {
        return out;
    }
    let mut push = |owner: BodyOwner, body: &str| -> bool {
        if let Some(snippet) = body_snippet(body, needle) {
            out.push(BodyHit { owner, snippet });
        }
        out.len() >= limit
    };
    for t in schema.tables.iter().filter(|t| t.is_view) {
        let Some(def) = t.view_definition.as_deref() else {
            continue;
        };
        if object_name_matches(&t.name, needle) {
            continue;
        }
        let owner = BodyOwner::View {
            schema: t.schema.clone(),
            name: t.name.clone(),
        };
        if push(owner, def) {
            return out;
        }
    }
    for t in &schema.tables {
        for tr in &t.triggers {
            let TriggerAction::Body(body) = &tr.action else {
                // PostgreSQL's: the body is the function's, found as a routine.
                continue;
            };
            let owner = BodyOwner::Trigger {
                schema: t.schema.clone(),
                table: t.name.clone(),
                name: tr.name.clone(),
            };
            if push(owner, body) {
                return out;
            }
        }
    }
    for r in &schema.routines {
        if object_name_matches(&r.name, needle) {
            continue;
        }
        if push(BodyOwner::Routine(r.clone()), &r.body) {
            return out;
        }
    }
    for e in &schema.events {
        if object_name_matches(&e.name, needle) {
            continue;
        }
        if push(BodyOwner::Event(e.clone()), &e.body) {
            return out;
        }
    }
    out
}

/// The first case-insensitive occurrence of `needle` in `hay`, by the rule
/// the editor's find bar uses ([`crate::text_ops::matches_at`]).
fn first_match(hay: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    let first = needle.as_bytes()[0];
    if first.is_ascii() {
        // A continuation byte is >= 0x80, so an ASCII first byte can only
        // line up with a character's start.
        let lower = first.to_ascii_lowercase();
        return hay
            .bytes()
            .enumerate()
            .filter(|&(_, b)| b.to_ascii_lowercase() == lower)
            .map(|(i, _)| i)
            .find(|&i| crate::text_ops::matches_at(hay, i, needle));
    }
    hay.char_indices()
        .map(|(i, _)| i)
        .find(|&i| crate::text_ops::matches_at(hay, i, needle))
}

/// The line of `body` holding the first case-insensitive occurrence of
/// `needle`, ready for a palette row: trimmed, every whitespace run collapsed
/// to one space, and — when longer than [`SNIPPET_CHARS`] — cut to a window
/// that keeps the match, an `…` marking each side that was cut.
///
/// `None` when the body does not contain the term (or the term is empty).
pub fn body_snippet(body: &str, needle: &str) -> Option<String> {
    let at = first_match(body, needle)?;
    let line_start = body[..at].rfind('\n').map_or(0, |i| i + 1);
    let line_end = body[at..].find('\n').map_or(body.len(), |i| at + i);
    let line = &body[line_start..line_end];
    // The needle is trimmed by the palette, so the match starts on a
    // non-space and survives the trim; `min` guards a caller that did not.
    let lead_ws = line.len() - line.trim_start().len();
    let line = line.trim();
    let at = (at - line_start).saturating_sub(lead_ws).min(line.len());
    let chars: Vec<char> = line.chars().collect();
    let at_char = line[..at].chars().count();
    let (start, end) = if chars.len() <= SNIPPET_CHARS {
        (0, chars.len())
    } else {
        let start = at_char.saturating_sub(SNIPPET_LEAD);
        let end = (start + SNIPPET_CHARS).min(chars.len());
        (end.saturating_sub(SNIPPET_CHARS), end)
    };
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    let mut in_space = false;
    for &c in &chars[start..end] {
        if c.is_whitespace() {
            if !in_space {
                out.push(' ');
            }
            in_space = true;
        } else {
            out.push(c);
            in_space = false;
        }
    }
    if end < chars.len() {
        out.push('…');
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{TableInfo, TriggerInfo};

    fn view(name: &str, def: &str) -> TableInfo {
        TableInfo {
            name: name.into(),
            is_view: true,
            view_definition: Some(def.into()),
            ..Default::default()
        }
    }

    fn routine(name: &str, body: &str) -> Arc<RoutineInfo> {
        Arc::new(RoutineInfo {
            name: name.into(),
            body: body.into(),
            ..Default::default()
        })
    }

    fn trigger(name: &str, on: &str, body: &str) -> TriggerInfo {
        TriggerInfo {
            name: name.into(),
            table: on.into(),
            action: TriggerAction::Body(body.into()),
            ..Default::default()
        }
    }

    fn names(hits: &[BodyHit]) -> Vec<String> {
        hits.iter()
            .map(|h| match &h.owner {
                BodyOwner::View { name, .. } => format!("view:{name}"),
                BodyOwner::Routine(r) => format!("routine:{}", r.name),
                BodyOwner::Event(e) => format!("event:{}", e.name),
                BodyOwner::Trigger { table, name, .. } => format!("trigger:{table}.{name}"),
            })
            .collect()
    }

    #[test]
    fn a_term_shorter_than_three_characters_searches_no_body() {
        let s = DbSchema {
            routines: vec![routine("f", "SELECT ab FROM t")],
            ..Default::default()
        };
        assert!(body_hits(&s, "ab", 50).is_empty());
        assert!(body_hits(&s, "", 50).is_empty());
        assert_eq!(names(&body_hits(&s, "ab ", 50)), ["routine:f"]);
        // Characters, not bytes: two Cyrillic letters are four bytes.
        assert!(!searches_bodies("щд"));
        assert!(searches_bodies("щдя"));
    }

    #[test]
    fn a_view_whose_select_mentions_the_term_is_a_hit_with_its_line() {
        let s = DbSchema {
            tables: vec![view("v_open", "SELECT id\nFROM Orders\nWHERE open")],
            ..Default::default()
        };
        let hits = body_hits(&s, "orders", 50);
        assert_eq!(names(&hits), ["view:v_open"]);
        assert_eq!(hits[0].snippet, "FROM Orders");
    }

    #[test]
    fn a_base_table_is_never_a_body_hit() {
        let mut t = view("orders_log", "SELECT 1 FROM payments");
        t.is_view = false;
        let s = DbSchema {
            tables: vec![t],
            ..Default::default()
        };
        assert!(body_hits(&s, "payments", 50).is_empty());
    }

    #[test]
    fn an_object_a_name_tier_lists_is_skipped() {
        let s = DbSchema {
            tables: vec![
                view("orders_view", "SELECT * FROM orders"),
                view("recent", "SELECT * FROM orders"),
            ],
            routines: vec![
                routine("orders_total", "SELECT SUM(x) FROM orders"),
                routine("total", "SELECT SUM(x) FROM orders"),
            ],
            ..Default::default()
        };
        assert_eq!(
            names(&body_hits(&s, "orders", 50)),
            ["view:recent", "routine:total"]
        );
    }

    #[test]
    fn a_trigger_body_is_a_hit_even_when_its_name_matches() {
        let mut t = view("orders", "");
        t.is_view = false;
        t.view_definition = None;
        t.triggers = vec![
            trigger(
                "audit_orders",
                "orders",
                "INSERT INTO audit_log VALUES (NEW.id)",
            ),
            trigger("other", "orders", "SET NEW.x = 1"),
        ];
        let s = DbSchema {
            tables: vec![t],
            ..Default::default()
        };
        let hits = body_hits(&s, "audit", 50);
        assert_eq!(names(&hits), ["trigger:orders.audit_orders"]);
        assert_eq!(hits[0].snippet, "INSERT INTO audit_log VALUES (NEW.id)");
    }

    #[test]
    fn a_postgres_trigger_has_no_body_of_its_own() {
        let mut t = view("orders", "");
        t.is_view = false;
        t.triggers = vec![TriggerInfo {
            name: "trg".into(),
            table: "orders".into(),
            action: TriggerAction::Function {
                name: "audit_fn".into(),
                args: vec![],
            },
            ..Default::default()
        }];
        let s = DbSchema {
            tables: vec![t],
            ..Default::default()
        };
        assert!(body_hits(&s, "audit", 50).is_empty());
    }

    #[test]
    fn an_event_body_is_searched() {
        let s = DbSchema {
            events: vec![Arc::new(EventInfo {
                name: "nightly".into(),
                body: "DELETE FROM sessions WHERE expired".into(),
                ..Default::default()
            })],
            ..Default::default()
        };
        assert_eq!(names(&body_hits(&s, "sessions", 50)), ["event:nightly"]);
    }

    #[test]
    fn the_match_ignores_case_on_both_sides() {
        let s = DbSchema {
            routines: vec![
                routine("a", "select * from CUSTOMERS"),
                routine("b", "SELECT 'Ärger'"),
            ],
            ..Default::default()
        };
        assert_eq!(names(&body_hits(&s, "Customers", 50)), ["routine:a"]);
        assert_eq!(names(&body_hits(&s, "ärger", 50)), ["routine:b"]);
    }

    #[test]
    fn hits_stop_at_the_limit() {
        let s = DbSchema {
            routines: (0..80)
                .map(|i| routine(&format!("r{i}"), "SELECT * FROM ledger"))
                .collect(),
            ..Default::default()
        };
        assert_eq!(body_hits(&s, "ledger", BODY_HIT_CAP).len(), BODY_HIT_CAP);
        assert_eq!(body_hits(&s, "ledger", 7).len(), 7);
        assert!(body_hits(&s, "ledger", 0).is_empty());
    }

    #[test]
    fn the_snippet_is_the_matching_line_trimmed_with_whitespace_collapsed() {
        let body = "BEGIN\r\n\t  UPDATE\tstock   SET qty = 0;  \r\nEND";
        assert_eq!(
            body_snippet(body, "stock").as_deref(),
            Some("UPDATE stock SET qty = 0;")
        );
    }

    #[test]
    fn no_match_and_an_empty_term_give_no_snippet() {
        assert_eq!(body_snippet("SELECT 1", "orders"), None);
        assert_eq!(body_snippet("SELECT 1", ""), None);
        assert_eq!(body_snippet("", "abc"), None);
    }

    #[test]
    fn a_long_line_is_clamped_around_the_match_with_ellipses() {
        let line = format!("{} target {}", "a".repeat(100), "b".repeat(100));
        let snip = body_snippet(&line, "target").unwrap();
        assert!(snip.starts_with('…') && snip.ends_with('…'), "{snip}");
        assert!(snip.contains("target"), "{snip}");
        assert_eq!(snip.chars().count(), SNIPPET_CHARS + 2, "{snip}");
    }

    #[test]
    fn a_match_near_either_end_is_cut_on_one_side_only() {
        let head = format!("target {}", "b".repeat(100));
        let snip = body_snippet(&head, "target").unwrap();
        assert!(snip.starts_with("target") && snip.ends_with('…'), "{snip}");
        let tail = format!("{} target", "a".repeat(100));
        let snip = body_snippet(&tail, "target").unwrap();
        assert!(snip.starts_with('…') && snip.ends_with("target"), "{snip}");
        assert_eq!(snip.chars().count(), SNIPPET_CHARS + 1, "{snip}");
    }

    #[test]
    fn a_multibyte_line_is_cut_on_character_boundaries() {
        let line = format!("{} ziel {}", "ä".repeat(90), "ö".repeat(90));
        let snip = body_snippet(&line, "ZIEL").unwrap();
        assert!(snip.contains("ziel"), "{snip}");
        assert_eq!(snip.chars().count(), SNIPPET_CHARS + 2, "{snip}");
    }
}
