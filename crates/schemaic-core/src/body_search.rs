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
//! **The scan is over bodies folded once per schema load**, not per keystroke
//! ([`BodyIndex`], cached on the schema). Asking the find bar's rule at every
//! character of every body cost, in a release build over 10,000 routines of
//! 4 KB, about 17–30 ms a keystroke for an ASCII term and **166–204 ms** for one
//! starting with a non-ASCII letter (`заказ`, `éte`) — on the UI thread, for the
//! precise searches the tier exists for, since only a term matching little
//! scans everything. A folded copy is searched with `str::find` instead, and
//! [`crate::text_ops::fold_for_search`] keeps it offset-for-offset with the
//! body, so the hit is read back out of the body as stored.
//!
//! The match is a plain case-insensitive substring over the body **as stored**,
//! comments and string literals included: a body search is how you find the
//! routine that mentions a table in a comment, and a lexer-aware search would
//! hide exactly that.

use std::sync::{Arc, OnceLock};

use crate::schema::{DbSchema, EventInfo, RoutineInfo, TriggerAction, object_name_matches};
use crate::text_ops::fold_for_search;

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
/// them, each with a snippet of the matching line — [`body_hits_where`] keeping
/// every hit.
pub fn body_hits(schema: &DbSchema, needle: &str, limit: usize) -> Vec<BodyHit> {
    body_hits_where(schema, needle, limit, |_| true)
}

/// The objects in `schema` whose body contains `needle` and that `keep`
/// accepts, at most `limit` of them, each with a snippet of the matching line.
///
/// **`needle` is the term as typed, not lowercased** — the match folds case
/// itself, character for character and width for width, and a lowercased term
/// no longer lines up with the text it was typed from: `İ` lowercases to two
/// characters and `ẞ` to the narrower `ß`, so `İstanbul` and `STRAẞE` found no
/// body that spelled them exactly as typed.
///
/// **`keep` is asked before a hit counts against `limit`.** The palette drops
/// what it cannot open — a C function, whose `prosrc` is a link symbol that
/// matches a term like `geography`, a `verbatim` T-SQL routine — and dropping
/// them after the cap was spent let fifty such rows crowd out the user's own
/// routine that mentions the term, and show nothing at all.
///
/// **An object a name tier already lists is skipped** — a view, routine or
/// event whose own name matches through [`object_name_matches`], which is the
/// predicate those tiers use. A trigger is never skipped: no name tier lists
/// triggers, so its body is the only way the palette reaches it.
///
/// Order: views, then triggers (by their table), then routines, then events —
/// catalogue order within each. Nothing is returned for a term shorter than
/// [`MIN_TERM_CHARS`].
pub fn body_hits_where(
    schema: &DbSchema,
    needle: &str,
    limit: usize,
    mut keep: impl FnMut(&BodyOwner) -> bool,
) -> Vec<BodyHit> {
    let mut out = Vec::new();
    if limit == 0 || !searches_bodies(needle) {
        return out;
    }
    let folded_needle = fold_for_search(needle);
    let name_needle = needle.to_lowercase();
    for entry in &schema.body_index.get(schema).entries {
        let Some((owner, body)) = entry.source.resolve(schema) else {
            continue;
        };
        let name = match &owner {
            BodyOwner::View { name, .. } => Some(name.as_str()),
            BodyOwner::Routine(r) => Some(r.name.as_str()),
            BodyOwner::Event(e) => Some(e.name.as_str()),
            BodyOwner::Trigger { .. } => None,
        };
        if name.is_some_and(|n| object_name_matches(n, &name_needle)) {
            continue;
        }
        let Some(at) = entry.folded.find(&folded_needle) else {
            continue;
        };
        if !keep(&owner) {
            continue;
        }
        out.push(BodyHit {
            snippet: snippet_at(body, at),
            owner,
        });
        if out.len() >= limit {
            break;
        }
    }
    out
}

/// Every body in a schema, folded once ([`fold_for_search`]) and in the order
/// [`body_hits_where`] reports them. Built on a schema's first body search and
/// kept on it ([`BodyIndexCache`]), so each keystroke after that is a
/// `str::find` per body rather than the find bar's rule asked at every
/// character.
#[derive(Debug, Default)]
pub struct BodyIndex {
    entries: Vec<BodyEntry>,
}

#[derive(Debug)]
struct BodyEntry {
    source: BodySource,
    /// The body, folded: every byte offset is the same character's offset in
    /// the body as stored.
    folded: String,
}

/// Where in the schema a [`BodyEntry`]'s body lives — positions, not copies,
/// since the index lives on that very schema.
#[derive(Clone, Copy, Debug)]
enum BodySource {
    View(usize),
    Trigger(usize, usize),
    Routine(usize),
    Event(usize),
}

impl BodySource {
    fn resolve(self, schema: &DbSchema) -> Option<(BodyOwner, &str)> {
        Some(match self {
            BodySource::View(t) => {
                let t = schema.tables.get(t)?;
                (
                    BodyOwner::View {
                        schema: t.schema.clone(),
                        name: t.name.clone(),
                    },
                    t.view_definition.as_deref()?,
                )
            }
            BodySource::Trigger(t, i) => {
                let t = schema.tables.get(t)?;
                let tr = t.triggers.get(i)?;
                let TriggerAction::Body(body) = &tr.action else {
                    return None;
                };
                (
                    BodyOwner::Trigger {
                        schema: t.schema.clone(),
                        table: t.name.clone(),
                        name: tr.name.clone(),
                    },
                    body.as_str(),
                )
            }
            BodySource::Routine(i) => {
                let r = schema.routines.get(i)?;
                (BodyOwner::Routine(r.clone()), r.body.as_str())
            }
            BodySource::Event(i) => {
                let e = schema.events.get(i)?;
                (BodyOwner::Event(e.clone()), e.body.as_str())
            }
        })
    }
}

impl BodyIndex {
    fn build(schema: &DbSchema) -> BodyIndex {
        let mut entries = Vec::new();
        let mut add = |source: BodySource, body: &str| {
            if !body.is_empty() {
                entries.push(BodyEntry {
                    source,
                    folded: fold_for_search(body),
                });
            }
        };
        for (i, t) in schema.tables.iter().enumerate().filter(|(_, t)| t.is_view) {
            if let Some(def) = t.view_definition.as_deref() {
                add(BodySource::View(i), def);
            }
        }
        for (i, t) in schema.tables.iter().enumerate() {
            for (j, tr) in t.triggers.iter().enumerate() {
                // PostgreSQL's has no body of its own: it is the function's,
                // found as a routine.
                if let TriggerAction::Body(body) = &tr.action {
                    add(BodySource::Trigger(i, j), body);
                }
            }
        }
        for (i, r) in schema.routines.iter().enumerate() {
            add(BodySource::Routine(i), &r.body);
        }
        for (i, e) in schema.events.iter().enumerate() {
            add(BodySource::Event(i), &e.body);
        }
        BodyIndex { entries }
    }
}

/// A schema's [`BodyIndex`], built on its first body search.
///
/// **A clone starts empty.** A `DbSchema` is cloned to be changed — a
/// comparison strips definers, a test builds one variant from another — and a
/// clone that carried its source's index would search bodies it no longer
/// has.
#[derive(Debug, Default)]
pub struct BodyIndexCache(OnceLock<BodyIndex>);

impl Clone for BodyIndexCache {
    fn clone(&self) -> Self {
        BodyIndexCache::default()
    }
}

impl BodyIndexCache {
    fn get(&self, schema: &DbSchema) -> &BodyIndex {
        self.0.get_or_init(|| BodyIndex::build(schema))
    }
}

/// The palette's cap on open-tab rows ([`text_hits`]).
pub const TAB_HIT_CAP: usize = 20;

/// How many bytes of open-tab text [`text_hits`] scans per keystroke, across
/// every tab. The palette is undebounced and tab text is neither folded nor
/// cached, so a tab past what is left of it is skipped rather than scanned —
/// a 7 MB script would otherwise cost a full case-folding walk per character
/// typed, on the UI thread.
pub const TAB_SCAN_BUDGET: usize = 4 * 1024 * 1024;

/// One open-text hit: which of the texts, where its first match starts, and the
/// matching line as a palette row shows it ([`body_snippet`]'s shape).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextHit {
    pub index: usize,
    pub at: usize,
    pub snippet: String,
}

/// Find Anywhere's **open tabs** tier: the texts holding `needle`, one hit
/// each at its first match, in the order given, at most `cap` of them.
///
/// The body tier's rule — the term as typed, matched case-insensitively by the
/// find bar's own fold, comments and strings included, and nothing for a term
/// shorter than [`MIN_TERM_CHARS`] — with the one difference that the text is
/// scanned as it is rather than out of a folded index, which is what
/// `budget` bounds: a text longer than what remains of it is not scanned.
pub fn text_hits(texts: &[&str], needle: &str, cap: usize, budget: usize) -> Vec<TextHit> {
    let mut scan = TextScan::new(needle, cap, budget);
    for (index, text) in texts.iter().enumerate() {
        scan.offer(index, text);
    }
    scan.finish()
}

/// [`text_hits`] one text at a time — for a caller whose texts each sit behind
/// a borrow of their own (a tab's text signal), which a `&[&str]` cannot
/// gather without copying every one.
pub struct TextScan<'n> {
    needle: &'n str,
    searches: bool,
    cap: usize,
    left: usize,
    hits: Vec<TextHit>,
}

impl<'n> TextScan<'n> {
    pub fn new(needle: &'n str, cap: usize, budget: usize) -> Self {
        TextScan {
            needle,
            searches: searches_bodies(needle),
            cap,
            left: budget,
            hits: Vec::new(),
        }
    }

    /// Whether another text can still add anything — short term, cap reached.
    pub fn done(&self) -> bool {
        !self.searches || self.hits.len() >= self.cap
    }

    /// Search `text`, the `index`th, unless it is longer than what remains of
    /// the budget.
    pub fn offer(&mut self, index: usize, text: &str) {
        if self.done() || text.len() > self.left {
            return;
        }
        self.left -= text.len();
        if let Some(at) = first_match(text, self.needle) {
            self.hits.push(TextHit {
                index,
                at,
                snippet: snippet_at(text, at),
            });
        }
    }

    pub fn finish(self) -> Vec<TextHit> {
        self.hits
    }
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
    Some(snippet_at(body, first_match(body, needle)?))
}

/// [`body_snippet`]'s row text for a match at byte `at` of `body`.
///
/// **Counted, not collected.** A MySQL view's definition is one line however
/// long it is, and gathering the line into a `Vec<char>` to cut sixty
/// characters out of it allocated four bytes per character of the whole
/// definition, per hit, per keystroke.
fn snippet_at(body: &str, at: usize) -> String {
    let line_start = body[..at].rfind('\n').map_or(0, |i| i + 1);
    let line_end = body[at..].find('\n').map_or(body.len(), |i| at + i);
    let line = &body[line_start..line_end];
    // The needle is trimmed by the palette, so the match starts on a
    // non-space and survives the trim; `min` guards a caller that did not.
    let lead_ws = line.len() - line.trim_start().len();
    let line = line.trim();
    let at = (at - line_start).saturating_sub(lead_ws).min(line.len());
    let len = line.chars().count();
    let at_char = line[..at].chars().count();
    let (start, end) = if len <= SNIPPET_CHARS {
        (0, len)
    } else {
        let start = at_char.saturating_sub(SNIPPET_LEAD);
        let end = (start + SNIPPET_CHARS).min(len);
        (end.saturating_sub(SNIPPET_CHARS), end)
    };
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    let mut in_space = false;
    for c in line.chars().skip(start).take(end - start) {
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
    if end < len {
        out.push('…');
    }
    out
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

    /// **The folded index finds what the find bar's rule finds, where it
    /// finds it.** The tier searches a copy folded once per schema rather than
    /// asking `matches_at` at every character, and that is only sound while
    /// the two agree — over every script and width the fold distinguishes.
    #[test]
    fn the_folded_search_finds_what_the_find_bar_rule_finds() {
        let bodies = [
            "SELECT * FROM Orders WHERE city = 'İstanbul' AND street = 'STRAẞE'",
            "UPDATE заказы SET статус = 'Готов' -- ÉTÉ été Été",
            "SELECT 'ΣΊΣΥΦΟΣ', 'σίσυφος', 'ǅ', 'Kelvin K', 'Ω Ohm Ω'",
            "naïve façade Ärger ärger ÄRGER 東京 東京都 ﬁle FILE",
            "",
        ];
        let needles = [
            "orders",
            "ORDERS",
            "İstanbul",
            "istanbul",
            "straẞe",
            "STRASSE",
            "заказ",
            "ЗАКАЗЫ",
            "статус",
            "été",
            "ÉTÉ",
            "σίσυφος",
            "ΣΊΣΥΦΟΣ",
            "ǆ",
            "ǅ",
            "kelvin k",
            "k",
            "ω",
            "ärger",
            "東京",
            "ﬁle",
            "file",
            "zzq",
        ];
        for body in bodies {
            let folded = fold_for_search(body);
            assert_eq!(folded.len(), body.len(), "the fold keeps every offset");
            for needle in needles {
                assert_eq!(
                    folded.find(&fold_for_search(needle)),
                    first_match(body, needle),
                    "{needle:?} in {body:?}"
                );
            }
        }
    }

    /// A term typed with a capital that lowercases wider (`İ`) or narrower
    /// (`ẞ`) finds the body that spells it as typed, and its lowercase still
    /// finds an ordinary capital.
    #[test]
    fn a_term_is_matched_as_typed() {
        let s = DbSchema {
            routines: vec![
                routine("a", "WHERE city = 'İstanbul'"),
                routine("b", "WHERE street = 'HAUPTSTRAẞE'"),
                routine("c", "WHERE name = 'Ärger'"),
            ],
            ..Default::default()
        };
        assert_eq!(names(&body_hits(&s, "İstanbul", 50)), ["routine:a"]);
        assert_eq!(names(&body_hits(&s, "STRAẞE", 50)), ["routine:b"]);
        assert_eq!(names(&body_hits(&s, "ärger", 50)), ["routine:c"]);
        // The name tiers' skip still reads the lowercase of what was typed.
        let named = DbSchema {
            routines: vec![routine("Orders_Total", "SELECT 1 FROM Orders")],
            ..Default::default()
        };
        assert!(body_hits(&named, "ORDERS", 50).is_empty());
    }

    /// `keep` is asked before a hit spends the limit, so the rows a caller will
    /// drop cannot crowd out the ones it will show.
    #[test]
    fn hits_a_caller_drops_do_not_spend_the_limit() {
        let mut routines: Vec<_> = (0..10)
            .map(|i| routine(&format!("c{i}"), "geography_distance"))
            .collect();
        routines.push(routine("mine", "SELECT b::geography"));
        let s = DbSchema {
            routines,
            ..Default::default()
        };
        let hits = body_hits_where(
            &s,
            "geography",
            3,
            |o| matches!(o, BodyOwner::Routine(r) if r.name == "mine"),
        );
        assert_eq!(names(&hits), ["routine:mine"]);
    }

    /// **A clone searches its own bodies.** The index is built on a schema's
    /// first body search and kept on it; a clone is made to be changed, and
    /// one carrying its source's index would search bodies it no longer has.
    #[test]
    fn a_cloned_schema_does_not_search_its_sources_bodies() {
        let s = DbSchema {
            routines: vec![routine("f", "SELECT * FROM ledger")],
            ..Default::default()
        };
        assert_eq!(names(&body_hits(&s, "ledger", 50)), ["routine:f"]);
        let mut changed = s.clone();
        changed.routines = vec![routine("f", "SELECT * FROM journal")];
        assert!(body_hits(&changed, "ledger", 50).is_empty());
        assert_eq!(names(&body_hits(&changed, "journal", 50)), ["routine:f"]);
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

    // -- Open tabs ----------------------------------------------------------

    #[test]
    fn a_tab_holding_the_term_is_one_hit_at_its_first_match() {
        let tabs = [
            "SELECT 1;",
            "-- orders\nSELECT * FROM Orders o\nJOIN orders x",
            "x",
        ];
        let hits = text_hits(&tabs, "orders", 10, usize::MAX);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].index, 1);
        assert_eq!(hits[0].at, 3);
        assert_eq!(hits[0].snippet, "-- orders");
    }

    #[test]
    fn the_tab_tier_folds_case_as_typed_and_waits_for_three_characters() {
        let tabs = ["SELECT * FROM ЗАКАЗЫ"];
        assert_eq!(text_hits(&tabs, "заказ", 10, usize::MAX).len(), 1);
        assert!(text_hits(&tabs, "se", 10, usize::MAX).is_empty());
        assert!(text_hits(&tabs, "", 10, usize::MAX).is_empty());
    }

    #[test]
    fn the_tab_tier_stops_at_its_cap_and_its_byte_budget() {
        let tabs = ["abc one", "abc two", "abc three"];
        let idx = |hits: Vec<TextHit>| hits.iter().map(|h| h.index).collect::<Vec<_>>();
        assert_eq!(idx(text_hits(&tabs, "abc", 2, usize::MAX)), [0, 1]);
        // A tab longer than what is left of the budget is skipped, and a
        // smaller one after it is still searched.
        let big = format!("{}abc", "x".repeat(100));
        let tabs = [big.as_str(), "abc small"];
        assert_eq!(idx(text_hits(&tabs, "abc", 10, 50)), [1]);
        assert_eq!(idx(text_hits(&tabs, "abc", 10, 1_000)), [0, 1]);
    }
}
