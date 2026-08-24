//! **Which editor a column's values get**, and the value rules that editor
//! enforces — the pure half of the grid's type-aware cell editing.
//!
//! Every cell used to edit as text, which is the right default for a `varchar`
//! and the wrong one for the four column shapes whose legal values are already
//! written down: a boolean has two, an `ENUM` has its member list, a `SET` has
//! subsets of one, and a date has a calendar. This module reads a column's
//! **declared** type and answers which of those it is; the UI renders the
//! matching control, and a control that can only produce legal values refuses
//! bad input at the keystroke instead of at the round trip.
//!
//! **The declared type, not the wire type.** MySQL sends an `ENUM` over the wire
//! as `MYSQL_TYPE_STRING` and a `BOOLEAN` as `TINYINT`, so
//! [`crate::model::Column::type_name`] cannot answer either question — the member
//! list and the `tinyint(1)` width only exist in the catalogue
//! ([`crate::schema::ColumnInfo::type_name`], which is MySQL's `COLUMN_TYPE`,
//! PostgreSQL's `format_type`, SQLite's `decl_type`). A result whose schema
//! hasn't loaded therefore falls back to the wire type, where `DATE` and
//! `DATETIME` still resolve and the rest stay text: a missing control is a
//! smaller failure than a control over the wrong values.
//!
//! **A value the control cannot represent keeps the text editor.** That is
//! [`fits`], and it is the reason a `tinyint(1)` holding `7`, an `ENUM` column
//! holding the empty string MySQL writes for a rejected insert, and a `DATE`
//! holding `0000-00-00` all stay editable as what they are. A toggle rendered
//! over `7` would write `0` or `1` the moment it was touched, which is data loss
//! dressed up as a feature. An **empty** value is the exception: it is "nothing
//! chosen yet" (a NULL field switched to a value, a pending row's blank cell) and
//! every control opens on it unselected.

use crate::date::{Date, Stamp, Time};
use crate::intel::SqlDialect;
use crate::schema::DbSchema;
use crate::sql::skip_noncode;

/// The control a column's cells are edited with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CellEditor {
    /// A plain text field — the default, and the fallback for everything below
    /// when the value in hand doesn't fit ([`fits`]).
    Text,
    /// Two states, written back in the spelling this engine reads.
    Bool(BoolWire),
    /// One of a fixed list: MySQL's `ENUM`, or a PostgreSQL enum type.
    Enum(Vec<String>),
    /// Any subset of a fixed list, stored comma-joined — MySQL's `SET`.
    Set(Vec<String>),
    /// A calendar date, no time of day.
    Date,
    /// A calendar date **and** a time of day.
    DateTime,
}

/// How a boolean is spelled in the column it goes back to.
///
/// **The spelling the engine itself hands back**, not the one that reads best.
/// Every engine accepts several on the way in — PostgreSQL takes `true`, `t`,
/// `yes`, `on` and `1` alike — so the choice is free, and the round trip is what
/// decides it: a toggle that writes what the column already reads back is
/// recognised as a *revert* (`GridState::stage` compares against the original
/// text and un-stages an edit equal to it), while `true` written over a `t`
/// leaves a green cell whose `UPDATE` writes a value already there, and shows one
/// word in a column of letters until the re-fetch.
///
/// MySQL has no boolean type at all — `BOOLEAN` is an alias for `TINYINT(1)` and
/// the values are the integers. SQLite has neither type nor opinion: it stores
/// what it is given, and `1`/`0` is what its `BOOLEAN` columns conventionally
/// hold (and what its comparisons produce), so it follows MySQL. PostgreSQL's
/// text protocol answers in single letters, which is what the grid displays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoolWire {
    /// `1` / `0` — MySQL, MariaDB, SQLite.
    OneZero,
    /// `t` / `f` — PostgreSQL.
    Letter,
}

impl BoolWire {
    /// The literal to write for this state.
    pub fn text(self, on: bool) -> &'static str {
        match (self, on) {
            (BoolWire::OneZero, true) => "1",
            (BoolWire::OneZero, false) => "0",
            (BoolWire::Letter, true) => "t",
            (BoolWire::Letter, false) => "f",
        }
    }

    /// The spelling `dialect`'s boolean-ish columns take.
    pub fn of(dialect: SqlDialect) -> BoolWire {
        match dialect {
            SqlDialect::Postgres => BoolWire::Letter,
            SqlDialect::MySql | SqlDialect::Sqlite => BoolWire::OneZero,
        }
    }
}

/// Read a stored value as a boolean, in every spelling the three engines hand
/// one back in.
///
/// Deliberately wider than what [`BoolWire::text`] writes: PostgreSQL's text
/// protocol says `t`/`f`, its own literals say `true`/`false`/`yes`/`no`/`on`/
/// `off`, MySQL says `1`/`0`, and a SQLite column declared `BOOLEAN` holds
/// whichever of those was inserted. Anything else — including `7`, `-1` and the
/// empty string — is **not** a boolean, which is what keeps [`fits`] honest.
pub fn read_bool(text: &str) -> Option<bool> {
    let t = text.trim();
    if t.eq_ignore_ascii_case("1")
        || t.eq_ignore_ascii_case("t")
        || t.eq_ignore_ascii_case("true")
        || t.eq_ignore_ascii_case("yes")
        || t.eq_ignore_ascii_case("y")
        || t.eq_ignore_ascii_case("on")
    {
        return Some(true);
    }
    if t.eq_ignore_ascii_case("0")
        || t.eq_ignore_ascii_case("f")
        || t.eq_ignore_ascii_case("false")
        || t.eq_ignore_ascii_case("no")
        || t.eq_ignore_ascii_case("n")
        || t.eq_ignore_ascii_case("off")
    {
        return Some(false);
    }
    None
}

/// The editor for a **declared** column type, without a catalogue to consult.
///
/// Handles everything whose legal values are in the type text itself: MySQL's
/// `enum(…)`/`set(…)`/`tinyint(1)`, the `bool`/`boolean` spelling all three
/// engines accept, and the date/time family. A PostgreSQL enum column's type
/// text is just the type's *name*, so it lands on [`CellEditor::Text`] here and
/// is resolved by [`editor_for_column`], which has the schema to look it up in.
pub fn editor_for_type(declared: &str, dialect: SqlDialect) -> CellEditor {
    let t = declared.trim();
    let base = base_type(t);
    // MySQL's `BOOLEAN` *is* `tinyint(1)`, and the width is the only thing that
    // tells it apart from a small integer. Asked of the leading **word** rather
    // than the whole base, because MySQL's own modifiers travel with it and
    // `tinyint(1) unsigned` is still a boolean. PostgreSQL has no such spelling;
    // SQLite's declared types are arbitrary text, and a `tinyint(1)` in one is a
    // MySQL schema somebody imported.
    if base.split(' ').next() == Some("tinyint")
        && dialect != SqlDialect::Postgres
        && type_args(t) == "1"
    {
        return CellEditor::Bool(BoolWire::of(dialect));
    }
    match base.as_str() {
        "bool" | "boolean" => return CellEditor::Bool(BoolWire::of(dialect)),
        "enum" if dialect == SqlDialect::MySql => {
            let members = value_list(type_args(t), dialect);
            if !members.is_empty() {
                return CellEditor::Enum(members);
            }
        }
        "set" if dialect == SqlDialect::MySql => {
            let members = value_list(type_args(t), dialect);
            if !members.is_empty() {
                return CellEditor::Set(members);
            }
        }
        "date" => return CellEditor::Date,
        "datetime" | "timestamp" | "timestamptz" => return CellEditor::DateTime,
        // PostgreSQL's verbose spellings, which `format_type` really does return.
        "timestamp without time zone" | "timestamp with time zone" => {
            return CellEditor::DateTime;
        }
        _ => {}
    }
    CellEditor::Text
}

/// The editor for a column, **with** the catalogue its type may point into.
///
/// The only thing the schema adds is PostgreSQL's user-defined enum types, whose
/// members live in [`crate::schema::DbSchema::enums`] rather than in the column's
/// type text. A type name that names no enum falls through to
/// [`editor_for_type`]'s answer, so an unloaded or partial schema costs a
/// control, never a wrong one.
pub fn editor_for_column(
    declared: &str,
    dialect: SqlDialect,
    schema: Option<&DbSchema>,
) -> CellEditor {
    let plain = editor_for_type(declared, dialect);
    if plain != CellEditor::Text || dialect != SqlDialect::Postgres {
        return plain;
    }
    let Some(schema) = schema else {
        return plain;
    };
    // `format_type` qualifies a type only when it is out of the search path, so
    // both `mood` and `sales.mood` reach here. Arrays (`mood[]`) are not a single
    // enum value and are left as text.
    let name = declared.trim().trim_end_matches("[]");
    if name.len() != declared.trim().len() {
        return plain;
    }
    let (ns, base) = match name.rsplit_once('.') {
        Some((ns, base)) => (Some(ns), base),
        None => (None, name),
    };
    match schema.find_enum(ns, base) {
        Some(e) if !e.values.is_empty() => CellEditor::Enum(e.values.clone()),
        _ => plain,
    }
}

/// Can `editor`'s control represent `text` without changing it?
///
/// An empty value always fits — it means "nothing chosen yet", and every control
/// opens unselected on it. Everything else has to be a value the control could
/// itself have produced, because the alternative is a control that rewrites the
/// cell the first time it is touched. See the module doc.
pub fn fits(editor: &CellEditor, text: &str) -> bool {
    if text.is_empty() {
        return true;
    }
    match editor {
        CellEditor::Text => true,
        CellEditor::Bool(_) => read_bool(text).is_some(),
        CellEditor::Enum(members) => members.iter().any(|m| m == text),
        CellEditor::Set(members) => set_members(text)
            .iter()
            .all(|v| members.iter().any(|m| m == v)),
        CellEditor::Date => Date::parse(text).is_some(),
        CellEditor::DateTime => Stamp::parse(text).is_some(),
    }
}

/// One row of a picker: what it reads as, what choosing it writes, and whether
/// the value in hand is already it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickOption {
    /// What the user reads — a boolean's `true`, an enum's member.
    pub label: String,
    /// What choosing it writes into the cell. Not always the label: a boolean
    /// writes the engine's own spelling ([`BoolWire`]), and a `SET` member writes
    /// the *whole* value with that member toggled.
    pub value: String,
    /// The value in hand is already this one — the row a menu tints.
    pub held: bool,
}

/// The rows a picker offers for `editor`, given the value it currently holds.
///
/// **The one list**, so the grid's in-cell menu and the row panel's box can't
/// drift; and pure, so what a choice *writes* is decided here rather than in a
/// view. A `SET`'s option writes the whole toggled value (through
/// [`toggle_set_member`], declaration order and all), which is what lets one menu
/// serve a single choice and a subset alike. [`CellEditor::Text`] and the date
/// editors have nothing to list and return nothing.
pub fn pick_options(editor: &CellEditor, current: &str) -> Vec<PickOption> {
    match editor {
        CellEditor::Bool(wire) => {
            let held = read_bool(current);
            [false, true]
                .into_iter()
                .map(|on| PickOption {
                    label: if on { "true" } else { "false" }.to_string(),
                    value: wire.text(on).to_string(),
                    held: held == Some(on),
                })
                .collect()
        }
        CellEditor::Enum(members) => members
            .iter()
            .map(|m| PickOption {
                label: m.clone(),
                value: m.clone(),
                held: m == current,
            })
            .collect(),
        CellEditor::Set(members) => {
            let on = set_members(current);
            members
                .iter()
                .map(|m| PickOption {
                    label: m.clone(),
                    value: toggle_set_member(current, m, members),
                    held: on.iter().any(|v| v == m),
                })
                .collect()
        }
        CellEditor::Text | CellEditor::Date | CellEditor::DateTime => Vec::new(),
    }
}

/// What a picker's own box reads while holding `current` — the held option's
/// **label**, so a boolean reads `true` rather than the `1` it is stored as.
///
/// Empty for a value nothing is chosen for (the control shows its placeholder),
/// and for anything with no list the value itself.
pub fn held_label(editor: &CellEditor, current: &str) -> String {
    match editor {
        CellEditor::Bool(_) => pick_options(editor, current)
            .into_iter()
            .find(|o| o.held)
            .map(|o| o.label)
            .unwrap_or_default(),
        _ => current.to_string(),
    }
}

/// The members of a `SET` value, in the order the value lists them. Empty text is
/// the empty set (MySQL's own spelling for one), not a set holding `""`.
pub fn set_members(value: &str) -> Vec<&str> {
    if value.is_empty() {
        return Vec::new();
    }
    value.split(',').collect()
}

/// Add or remove one member of a `SET` value, returning the new value.
///
/// The result is always in **declaration order**, whatever order the old value
/// or the user's clicks were in — that is the order MySQL itself stores and
/// returns a `SET` in, so anything else would show as a change on the next read.
/// A member not in `members` is dropped rather than carried, for the same
/// reason: the server would not have stored it.
pub fn toggle_set_member(value: &str, member: &str, members: &[String]) -> String {
    let current = set_members(value);
    let on = current.contains(&member);
    members
        .iter()
        .filter(|m| {
            let held = current.iter().any(|v| v == m);
            if m.as_str() == member { !on } else { held }
        })
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(",")
}

/// The text to write when a day is picked for a cell currently holding `current`.
///
/// A [`CellEditor::Date`] column gets the bare date. A
/// [`CellEditor::DateTime`] column keeps the time of day it already had — along
/// with its fractional seconds and timezone offset — and gains midnight when
/// there was nothing there. Any other editor is not a date column and is left
/// alone; a caller that reached here with one has a bug, and rewriting the cell
/// would hide it.
pub fn set_date(editor: &CellEditor, current: &str, date: Date) -> String {
    match editor {
        CellEditor::Date => date.iso(),
        CellEditor::DateTime => match Stamp::parse(current) {
            Some(s) => match s.time() {
                Some(_) => s.with_date(date).render(),
                None => s.with_date(date).with_time(Time::MIDNIGHT).render(),
            },
            None => Stamp::from_date(date).with_time(Time::MIDNIGHT).render(),
        },
        _ => current.to_string(),
    }
}

/// The text to write for **now** — the calendar's `Now` / `Today` footer, and the
/// [`set_date`] pair.
///
/// The instant arrives as one reading of the clock ([`crate::date::local_now`]),
/// because two readings are two instants and local midnight lies between them: a
/// date from before it beside a time from after it is a stamp a day in the past,
/// from the one control whose whole job is the current instant.
///
/// A [`CellEditor::Date`] column takes the day and nothing else. A
/// [`CellEditor::DateTime`] column takes the whole instant *in the shape the value
/// already had* — and the two parts of "the shape" that a new instant invalidates
/// are dropped rather than kept:
///
/// * **the fraction**, which was the old value's microseconds, not this one's;
/// * **the offset**, which is *replaced* where the value had one at all. Keeping
///   it wrote a local wall-clock time under the old value's zone — on a
///   `timestamptz` read as `+00` from a UTC+2 machine, an instant two hours from
///   the one the button names. A value with no offset is not given one: MySQL's
///   `DATETIME` has nowhere to put it.
pub fn set_now(editor: &CellEditor, current: &str, now: (Date, Time, &str)) -> String {
    let (date, time, offset) = now;
    match editor {
        CellEditor::Date => date.iso(),
        CellEditor::DateTime => {
            let stamp = Stamp::parse(current).unwrap_or_else(|| Stamp::from_date(date));
            let stamp = stamp.with_date(date).with_time(time).without_frac();
            let stamp = if stamp.has_offset() {
                stamp.with_offset(offset)
            } else {
                stamp
            };
            stamp.render()
        }
        _ => current.to_string(),
    }
}

/// The day a value stands on, if it stands on one — the cell a calendar paints
/// as the current selection, and `None` for anything it cannot point at.
pub fn value_date(current: &str) -> Option<Date> {
    Stamp::parse(current)
        .map(|s| s.date())
        .or_else(|| Date::parse(current))
}

/// The date a picker should open on for a cell holding `current`: the value's own
/// day when it has one, else `today`. (The month grid is [`crate::date::month_cells`].)
pub fn picker_focus(current: &str, today: Date) -> Date {
    value_date(current).unwrap_or(today)
}

/// The leading type keyword of a declared type, lower-cased: `varchar(45)` →
/// `varchar`, `timestamp(3) without time zone` → `timestamp without time zone`,
/// `int unsigned` → `int unsigned`.
///
/// The parenthesised part is dropped **and the words after it kept**, which is
/// what makes PostgreSQL's `timestamp(3) with time zone` reachable from the same
/// match arm as its unparameterised spelling.
fn base_type(t: &str) -> String {
    let (head, tail) = match (t.find('('), t.rfind(')')) {
        // The *last* `)`, not the first: an ENUM member may contain one.
        (Some(i), Some(j)) if j > i => (&t[..i], &t[j + 1..]),
        _ => (t, ""),
    };
    format!("{} {}", head.trim(), tail.trim())
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

/// What is between a declared type's parentheses (`enum('a','b')` → `'a','b'`,
/// `tinyint(1)` → `1`), or the empty string.
fn type_args(t: &str) -> &str {
    let Some(i) = t.find('(') else {
        return "";
    };
    let Some(j) = t.rfind(')') else {
        return "";
    };
    if j <= i { "" } else { t[i + 1..j].trim() }
}

/// The values of an `ENUM`/`SET` parameter list — `'a','b''c'` → `["a", "b'c"]`.
///
/// The **inverse of [`crate::export::sql_literal`]**, which is what MySQL's
/// `COLUMN_TYPE` (and our own generated DDL) writes these with: a doubled quote
/// is one quote, and a backslash escapes the byte after it on MySQL only. The
/// literal boundaries come from [`skip_noncode`], the workspace's one SQL
/// boundary lexer, so the rule for where a quoted value ends is not restated
/// here — this only unescapes what it delimits.
///
/// Anything between the literals (the commas, and any whitespace) is skipped, and
/// a list holding no literal at all yields nothing — which is how
/// [`editor_for_type`] declines to offer an empty dropdown.
fn value_list(args: &str, dialect: SqlDialect) -> Vec<String> {
    let b = args.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        if b[i] != b'\'' {
            i += 1;
            continue;
        }
        let Some(end) = skip_noncode(b, i, dialect) else {
            i += 1;
            continue;
        };
        // `skip_noncode` returns the index past the closing quote; an unterminated
        // literal runs to the end of the input, in which case there is no closing
        // quote to trim.
        let closed = end > i + 1 && b[end - 1] == b'\'';
        let inner = &args[i + 1..if closed { end - 1 } else { end }];
        out.push(unescape_literal(inner, dialect));
        i = end;
    }
    out
}

/// Undo the escaping [`crate::export::sql_literal`] applies inside a string
/// literal's quotes.
fn unescape_literal(inner: &str, dialect: SqlDialect) -> String {
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                // A doubled quote is one quote. A **lone** one cannot occur
                // inside a literal the lexer delimited — and the branch is a
                // peek rather than a `next()` so that if one ever does, it
                // passes through untouched instead of eating the character
                // after it. Written the other way, `a'b` came out as `a''`,
                // which is a silent rewrite of somebody's enum member.
                if chars.peek() == Some(&'\'') {
                    chars.next();
                }
                out.push('\'');
            }
            // MySQL is the only one of the three with a backslash escape (see
            // `sql_literal`), and the only sequences a *type list* can carry are
            // the ones that quoting produced: `\\` and `\'`.
            '\\' if dialect == SqlDialect::MySql => match chars.next() {
                Some(n) => out.push(n),
                None => out.push('\\'),
            },
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::sql_literal;
    use crate::model::Value;
    use crate::schema::{DbSchema, EnumInfo};

    use SqlDialect::{MySql, Postgres, Sqlite};

    fn members(e: &CellEditor) -> Vec<String> {
        match e {
            CellEditor::Enum(v) | CellEditor::Set(v) => v.clone(),
            other => panic!("not a member list: {other:?}"),
        }
    }

    // ── Booleans ────────────────────────────────────────────────────────────

    #[test]
    fn the_boolean_spelling_is_the_engines_own() {
        assert_eq!(BoolWire::of(MySql).text(true), "1");
        assert_eq!(BoolWire::of(MySql).text(false), "0");
        assert_eq!(BoolWire::of(Sqlite).text(true), "1");
        assert_eq!(BoolWire::of(Postgres).text(true), "t");
        assert_eq!(BoolWire::of(Postgres).text(false), "f");
    }

    #[test]
    fn every_engines_boolean_spelling_reads_back() {
        for t in ["1", "t", "T", "true", "TRUE", "True", "yes", "on", "y"] {
            assert_eq!(read_bool(t), Some(true), "{t}");
        }
        for f in ["0", "f", "F", "false", "FALSE", "no", "off", "n"] {
            assert_eq!(read_bool(f), Some(false), "{f}");
        }
    }

    /// The values that must **not** read as booleans, because a toggle over one
    /// would rewrite it on first touch.
    #[test]
    fn a_value_that_is_not_a_boolean_reads_as_nothing() {
        for other in ["", "7", "-1", "2", "maybe", "null", "NULL", " "] {
            assert_eq!(read_bool(other), None, "{other:?}");
        }
    }

    #[test]
    fn a_boolean_column_is_a_boolean_on_every_engine() {
        assert_eq!(
            editor_for_type("boolean", Postgres),
            CellEditor::Bool(BoolWire::Letter)
        );
        assert_eq!(
            editor_for_type("bool", MySql),
            CellEditor::Bool(BoolWire::OneZero)
        );
        assert_eq!(
            editor_for_type("BOOLEAN", Sqlite),
            CellEditor::Bool(BoolWire::OneZero)
        );
    }

    /// MySQL's `BOOLEAN` reaches the catalogue as `tinyint(1)`; a `tinyint` of any
    /// other width is a small integer and must stay one.
    #[test]
    fn only_tinyint_of_width_one_is_a_mysql_boolean() {
        assert_eq!(
            editor_for_type("tinyint(1)", MySql),
            CellEditor::Bool(BoolWire::OneZero)
        );
        assert_eq!(editor_for_type("tinyint(4)", MySql), CellEditor::Text);
        assert_eq!(editor_for_type("tinyint", MySql), CellEditor::Text);
        assert_eq!(
            editor_for_type("tinyint(1) unsigned", MySql),
            CellEditor::Bool(BoolWire::OneZero)
        );
    }

    /// PostgreSQL has no `tinyint` at all, so the spelling can only be an
    /// imported schema's — and it is not that engine's boolean.
    #[test]
    fn postgres_has_no_tinyint_boolean() {
        assert_eq!(editor_for_type("tinyint(1)", Postgres), CellEditor::Text);
    }

    // ── ENUM / SET ──────────────────────────────────────────────────────────

    #[test]
    fn a_mysql_enum_offers_its_members_in_declaration_order() {
        let e = editor_for_type("enum('G','PG','PG-13','R','NC-17')", MySql);
        assert!(matches!(e, CellEditor::Enum(_)));
        assert_eq!(members(&e), ["G", "PG", "PG-13", "R", "NC-17"]);
    }

    #[test]
    fn a_mysql_set_is_its_own_editor() {
        let e = editor_for_type("set('a','b','c')", MySql);
        assert!(matches!(e, CellEditor::Set(_)));
        assert_eq!(members(&e), ["a", "b", "c"]);
    }

    #[test]
    fn enum_members_are_unescaped() {
        let e = editor_for_type(r"enum('it''s','a,b','back\\slash','quote\'d')", MySql);
        assert_eq!(members(&e), ["it's", "a,b", r"back\slash", "quote'd"]);
    }

    /// Splitting on commas is the obvious wrong reader: a member may contain one.
    #[test]
    fn a_comma_inside_a_member_does_not_split_it() {
        let e = editor_for_type("enum('a,b','c')", MySql);
        assert_eq!(members(&e), ["a,b", "c"]);
    }

    /// The list reader is the inverse of the writer every one of these was
    /// produced by, so a value survives the round trip whatever is in it.
    #[test]
    fn every_member_survives_the_quoting_it_was_written_with() {
        // MySQL is the only engine that writes a value list *into* a type, and
        // `sql_literal` is the writer that produced this one.
        let dialect = MySql;
        let values = ["plain", "it's", "a,b", r"C:\temp", "", "two words", "''"];
        let list = values
            .iter()
            .map(|v| sql_literal(&Value::Str(v.to_string()), dialect))
            .collect::<Vec<_>>()
            .join(",");
        assert_eq!(value_list(&list, dialect), values, "{list}");
    }

    /// A lone quote inside the literal is not something [`skip_noncode`] can
    /// hand over — but the branch that meets one must be a no-op, not a rewrite.
    #[test]
    fn a_lone_quote_passes_through_without_eating_what_follows() {
        assert_eq!(unescape_literal("a'b", MySql), "a'b");
        assert_eq!(unescape_literal("a''b", MySql), "a'b");
        assert_eq!(unescape_literal("trailing'", MySql), "trailing'");
    }

    #[test]
    fn an_enum_with_no_members_is_not_offered_as_a_dropdown() {
        assert_eq!(editor_for_type("enum()", MySql), CellEditor::Text);
        assert_eq!(editor_for_type("enum", MySql), CellEditor::Text);
    }

    /// `enum` and `set` are MySQL's spellings; PostgreSQL's enums are named types
    /// and SQLite has neither, so the parameter list can only be text there.
    #[test]
    fn only_mysql_reads_an_inline_enum_list() {
        assert_eq!(editor_for_type("enum('a','b')", Postgres), CellEditor::Text);
        assert_eq!(editor_for_type("enum('a','b')", Sqlite), CellEditor::Text);
    }

    #[test]
    fn a_postgres_enum_column_resolves_through_the_catalogue() {
        let schema = DbSchema {
            enums: vec![EnumInfo {
                name: "mood".into(),
                schema: Some("public".into()),
                values: vec!["sad".into(), "ok".into(), "happy".into()],
                comment: None,
            }],
            ..Default::default()
        };
        let e = editor_for_column("mood", Postgres, Some(&schema));
        assert_eq!(members(&e), ["sad", "ok", "happy"]);
        // Qualified the way `format_type` writes a type outside the search path.
        let q = editor_for_column("public.mood", Postgres, Some(&schema));
        assert_eq!(members(&q), ["sad", "ok", "happy"]);
    }

    #[test]
    fn a_postgres_type_that_names_no_enum_stays_text() {
        let schema = DbSchema::default();
        assert_eq!(
            editor_for_column("mood", Postgres, Some(&schema)),
            CellEditor::Text
        );
        assert_eq!(editor_for_column("mood", Postgres, None), CellEditor::Text);
    }

    /// An array of an enum is not one of its members, and a dropdown over it
    /// would write a value the column rejects.
    #[test]
    fn an_array_of_an_enum_is_not_a_dropdown() {
        let schema = DbSchema {
            enums: vec![EnumInfo {
                name: "mood".into(),
                schema: Some("public".into()),
                values: vec!["ok".into()],
                comment: None,
            }],
            ..Default::default()
        };
        assert_eq!(
            editor_for_column("mood[]", Postgres, Some(&schema)),
            CellEditor::Text
        );
    }

    /// The catalogue is only ever consulted for what the type text couldn't
    /// answer — a schema must not be able to turn a date into a dropdown.
    #[test]
    fn the_catalogue_never_overrides_a_type_the_text_already_answered() {
        let schema = DbSchema {
            enums: vec![EnumInfo {
                name: "date".into(),
                schema: Some("public".into()),
                values: vec!["nonsense".into()],
                comment: None,
            }],
            ..Default::default()
        };
        assert_eq!(
            editor_for_column("date", Postgres, Some(&schema)),
            CellEditor::Date
        );
    }

    // ── Dates ───────────────────────────────────────────────────────────────

    #[test]
    fn the_date_family_maps_to_a_calendar() {
        assert_eq!(editor_for_type("date", MySql), CellEditor::Date);
        assert_eq!(editor_for_type("DATE", Sqlite), CellEditor::Date);
        assert_eq!(editor_for_type("datetime", MySql), CellEditor::DateTime);
        assert_eq!(editor_for_type("datetime(6)", MySql), CellEditor::DateTime);
        assert_eq!(editor_for_type("timestamp", MySql), CellEditor::DateTime);
        assert_eq!(
            editor_for_type("timestamptz", Postgres),
            CellEditor::DateTime
        );
        assert_eq!(
            editor_for_type("timestamp(3) without time zone", Postgres),
            CellEditor::DateTime
        );
        assert_eq!(
            editor_for_type("timestamp with time zone", Postgres),
            CellEditor::DateTime
        );
    }

    /// MySQL's `TIME` is a *duration* (`-838:59:59` … `838:59:59`), not a clock
    /// reading, and `YEAR` is not a day — neither has a control that could
    /// represent every value it holds.
    #[test]
    fn time_and_year_columns_stay_text() {
        assert_eq!(editor_for_type("time", MySql), CellEditor::Text);
        assert_eq!(editor_for_type("time(6)", MySql), CellEditor::Text);
        assert_eq!(editor_for_type("year", MySql), CellEditor::Text);
    }

    #[test]
    fn an_ordinary_column_gets_the_text_editor() {
        for t in ["varchar(45)", "text", "int(11) unsigned", "json", "blob"] {
            assert_eq!(editor_for_type(t, MySql), CellEditor::Text, "{t}");
        }
    }

    // ── Fitting a value to a control ────────────────────────────────────────

    #[test]
    fn an_empty_value_fits_every_control() {
        for e in [
            CellEditor::Bool(BoolWire::OneZero),
            CellEditor::Enum(vec!["a".into()]),
            CellEditor::Set(vec!["a".into()]),
            CellEditor::Date,
            CellEditor::DateTime,
        ] {
            assert!(fits(&e, ""), "{e:?}");
        }
    }

    #[test]
    fn a_tinyint_holding_seven_keeps_its_text_editor() {
        let b = CellEditor::Bool(BoolWire::OneZero);
        assert!(fits(&b, "1"));
        assert!(fits(&b, "0"));
        assert!(!fits(&b, "7"));
    }

    #[test]
    fn an_enum_value_outside_the_member_list_does_not_fit() {
        let e = CellEditor::Enum(vec!["G".into(), "PG".into()]);
        assert!(fits(&e, "PG"));
        assert!(!fits(&e, "X"));
        assert!(!fits(&e, "pg"), "members are case-sensitive values");
    }

    #[test]
    fn a_set_fits_when_every_member_it_lists_is_one() {
        let s = CellEditor::Set(vec!["a".into(), "b".into()]);
        assert!(fits(&s, "a"));
        assert!(fits(&s, "a,b"));
        assert!(
            fits(&s, "b,a"),
            "the value's order is the server's business"
        );
        assert!(!fits(&s, "a,z"));
    }

    #[test]
    fn an_unrepresentable_date_does_not_fit_the_calendar() {
        assert!(fits(&CellEditor::Date, "2024-01-15"));
        assert!(!fits(&CellEditor::Date, "0000-00-00"));
        assert!(!fits(&CellEditor::Date, "2024-01-15 10:00:00"));
        assert!(fits(&CellEditor::DateTime, "2024-01-15 10:00:00"));
        assert!(fits(&CellEditor::DateTime, "2024-01-15"));
        assert!(!fits(&CellEditor::DateTime, "0000-00-00 00:00:00"));
    }

    // ── Writing a value back ────────────────────────────────────────────────

    fn day(y: i32, m: u32, d: u32) -> Date {
        Date::new(y, m, d).expect("valid")
    }

    /// A fixed reading of the clock — the shape `date::local_now` returns, with
    /// its three parts taken **together** so a test can't accidentally state an
    /// instant that never existed.
    fn now_at(h: u32, m: u32, s: u32, offset: &str) -> (Date, Time, &str) {
        (day(2024, 1, 15), Time::new(h, m, s).expect("valid"), offset)
    }

    #[test]
    fn picking_a_day_on_a_date_column_writes_the_bare_date() {
        assert_eq!(
            set_date(&CellEditor::Date, "2020-05-05", day(2024, 1, 15)),
            "2024-01-15"
        );
        assert_eq!(
            set_date(&CellEditor::Date, "", day(2024, 1, 15)),
            "2024-01-15"
        );
    }

    /// The composition the calendar actually performs: the day changes and
    /// nothing else does.
    #[test]
    fn picking_a_day_on_a_datetime_column_keeps_the_time_of_day() {
        assert_eq!(
            set_date(
                &CellEditor::DateTime,
                "2020-05-05 23:59:59.250+02",
                day(2024, 1, 15)
            ),
            "2024-01-15 23:59:59.250+02"
        );
    }

    #[test]
    fn picking_a_day_on_an_empty_datetime_starts_at_midnight() {
        assert_eq!(
            set_date(&CellEditor::DateTime, "", day(2024, 1, 15)),
            "2024-01-15 00:00:00"
        );
        // Same for a value the parser can make nothing of — the cell had no time.
        assert_eq!(
            set_date(
                &CellEditor::DateTime,
                "0000-00-00 00:00:00",
                day(2024, 1, 15)
            ),
            "2024-01-15 00:00:00"
        );
    }

    #[test]
    fn a_datetime_column_holding_only_a_date_gains_midnight() {
        assert_eq!(
            set_date(&CellEditor::DateTime, "2020-05-05", day(2024, 1, 15)),
            "2024-01-15 00:00:00"
        );
    }

    #[test]
    fn a_column_that_is_not_a_date_is_left_alone() {
        assert_eq!(
            set_date(&CellEditor::Text, "whatever", day(2024, 1, 15)),
            "whatever"
        );
        assert_eq!(
            set_now(&CellEditor::Text, "whatever", now_at(1, 2, 3, "+02:00")),
            "whatever"
        );
    }

    /// One instant, read once: a `DATE` column takes its day and a `DATETIME`
    /// column takes all of it. The date is **not** whatever the cell held — the
    /// button says *now*.
    #[test]
    fn stamping_now_writes_the_whole_instant() {
        assert_eq!(
            set_now(
                &CellEditor::DateTime,
                "2020-05-05 10:00:00",
                now_at(23, 5, 9, "+02:00")
            ),
            "2024-01-15 23:05:09"
        );
        assert_eq!(
            set_now(&CellEditor::Date, "2020-05-05", now_at(23, 5, 9, "+02:00")),
            "2024-01-15"
        );
    }

    /// **The offset is replaced, and the fraction dropped.** A `timestamptz` read
    /// as `+00` from a machine at UTC+2 kept that `+00` while the time written
    /// under it was local — an instant two hours from the one the button names —
    /// and carried the old value's microseconds along with it.
    #[test]
    fn stamping_now_states_the_instant_in_the_local_zone() {
        assert_eq!(
            set_now(
                &CellEditor::DateTime,
                "2020-05-05 10:00:00.123456+00",
                now_at(23, 5, 9, "+02:00")
            ),
            "2024-01-15 23:05:09+02:00"
        );
    }

    /// A column with nowhere to put an offset is not given one: MySQL's
    /// `DATETIME` rejects the statement, and it is the same rule the rest of
    /// `Stamp` follows — keep the shape the value came in.
    #[test]
    fn stamping_now_does_not_invent_an_offset() {
        assert_eq!(
            set_now(&CellEditor::DateTime, "", now_at(9, 0, 0, "+02:00")),
            "2024-01-15 09:00:00"
        );
        assert_eq!(
            set_now(
                &CellEditor::DateTime,
                "2020-05-05 10:00:00",
                now_at(9, 0, 0, "+02:00")
            ),
            "2024-01-15 09:00:00"
        );
    }

    /// What the calendar paints as selected — and, on a value it cannot point
    /// at, that it paints nothing rather than today.
    #[test]
    fn the_day_a_value_stands_on_is_the_day_it_names() {
        assert_eq!(value_date("2020-05-05"), Date::new(2020, 5, 5));
        assert_eq!(value_date("2020-05-05 10:00:00+02"), Date::new(2020, 5, 5));
        assert_eq!(value_date(""), None);
        assert_eq!(value_date("0000-00-00"), None);
        assert_eq!(value_date("not a date"), None);
    }

    #[test]
    fn a_picker_opens_on_the_value_it_has_and_today_when_it_has_none() {
        let today = day(2024, 6, 1);
        assert_eq!(picker_focus("2020-05-05", today), day(2020, 5, 5));
        assert_eq!(picker_focus("2020-05-05 10:00:00", today), day(2020, 5, 5));
        assert_eq!(picker_focus("", today), today);
        assert_eq!(picker_focus("0000-00-00", today), today);
    }

    // ── What a picker offers ────────────────────────────────────────────────

    /// A boolean's rows read as words and write as the engine's spelling — the
    /// whole reason the two are separate fields.
    #[test]
    fn a_boolean_offers_two_rows_reading_true_and_false() {
        let opts = pick_options(&CellEditor::Bool(BoolWire::Letter), "t");
        assert_eq!(
            opts.iter().map(|o| o.label.as_str()).collect::<Vec<_>>(),
            ["false", "true"]
        );
        assert_eq!(
            opts.iter().map(|o| o.value.as_str()).collect::<Vec<_>>(),
            ["f", "t"]
        );
        assert_eq!(
            opts.iter().map(|o| o.held).collect::<Vec<_>>(),
            [false, true]
        );
    }

    #[test]
    fn an_unset_boolean_holds_neither_row() {
        let opts = pick_options(&CellEditor::Bool(BoolWire::OneZero), "");
        assert!(opts.iter().all(|o| !o.held));
        assert_eq!(
            opts.iter().map(|o| o.value.as_str()).collect::<Vec<_>>(),
            ["0", "1"]
        );
    }

    #[test]
    fn an_enum_offers_its_members_as_themselves() {
        let opts = pick_options(&CellEditor::Enum(vec!["G".into(), "PG".into()]), "PG");
        assert_eq!(
            opts[0],
            PickOption {
                label: "G".into(),
                value: "G".into(),
                held: false
            }
        );
        assert_eq!(
            opts[1],
            PickOption {
                label: "PG".into(),
                value: "PG".into(),
                held: true
            }
        );
    }

    /// A `SET`'s row writes the **whole** toggled value, which is what lets one
    /// menu both add and remove — and why the value is not the label.
    #[test]
    fn a_set_row_writes_the_value_that_toggling_it_produces() {
        let s = CellEditor::Set(vec!["a".into(), "b".into(), "c".into()]);
        let opts = pick_options(&s, "b");
        assert_eq!(opts[0].value, "a,b", "picking `a` adds it");
        assert_eq!(opts[1].value, "", "picking the held `b` removes it");
        assert!(opts[1].held);
        assert_eq!(opts[2].value, "b,c");
    }

    #[test]
    fn a_control_with_no_list_offers_no_rows() {
        for e in [CellEditor::Text, CellEditor::Date, CellEditor::DateTime] {
            assert!(pick_options(&e, "x").is_empty(), "{e:?}");
        }
    }

    #[test]
    fn a_pickers_box_reads_the_held_label_not_the_stored_value() {
        assert_eq!(
            held_label(&CellEditor::Bool(BoolWire::OneZero), "1"),
            "true"
        );
        assert_eq!(
            held_label(&CellEditor::Bool(BoolWire::Letter), "f"),
            "false"
        );
        assert_eq!(held_label(&CellEditor::Bool(BoolWire::OneZero), ""), "");
        // Nothing else relabels: an enum member and a `SET` value are already
        // what they read as.
        assert_eq!(held_label(&CellEditor::Enum(vec!["G".into()]), "G"), "G");
        assert_eq!(held_label(&CellEditor::Set(vec!["a".into()]), "a"), "a");
    }

    // ── SET toggling ────────────────────────────────────────────────────────

    fn abc() -> Vec<String> {
        vec!["a".into(), "b".into(), "c".into()]
    }

    #[test]
    fn a_set_value_lists_its_members() {
        assert_eq!(set_members("a,b"), ["a", "b"]);
        assert_eq!(set_members("a"), ["a"]);
        assert!(set_members("").is_empty(), "the empty set holds nothing");
    }

    #[test]
    fn toggling_adds_and_removes_one_member() {
        assert_eq!(toggle_set_member("a", "b", &abc()), "a,b");
        assert_eq!(toggle_set_member("a,b", "a", &abc()), "b");
        assert_eq!(toggle_set_member("", "c", &abc()), "c");
        assert_eq!(toggle_set_member("c", "c", &abc()), "");
    }

    /// MySQL stores a `SET` in declaration order and hands it back that way, so a
    /// toggle that preserved click order would show as a change on the next read.
    #[test]
    fn a_toggled_set_comes_back_in_declaration_order() {
        assert_eq!(toggle_set_member("c,a", "b", &abc()), "a,b,c");
        assert_eq!(toggle_set_member("c", "a", &abc()), "a,c");
    }

    #[test]
    fn a_value_the_column_could_not_hold_is_dropped_by_a_toggle() {
        assert_eq!(toggle_set_member("a,zzz", "b", &abc()), "a,b");
    }
}
