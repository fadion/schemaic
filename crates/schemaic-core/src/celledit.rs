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
//! [`fits`], and it is the reason a `tinyint(1)` holding `7` and a `DATE` holding
//! `0000-00-00` stay editable as what they are. A toggle rendered over `7` would
//! write `0` or `1` the moment it was touched, which is data loss dressed up as a
//! feature. An **empty** value is the exception: it is "nothing chosen yet" (a
//! NULL field switched to a value, a pending row's blank cell) and every control
//! opens on it unselected.
//!
//! **The exception has a cost, and it is not the `ENUM` protection this used to
//! claim.** A MySQL `ENUM` holding the empty string — what a rejected insert
//! writes in non-strict mode — is *not* kept on the text editor: `fits` answers
//! `true` for `""` before it looks at the editor at all, so such a cell gets the
//! dropdown, and the dropdown has no row that writes `''` back. The cell is also
//! indistinguishable from a NULL one, because `start_edit` seeds a NULL cell from
//! the empty string too. Telling the two apart means telling `fits` *which cell*
//! it is looking at, which no caller can say without threading the row down to
//! it — so the exception stands, on the grounds that a NULL cell and a fresh
//! pending row are the common cases and a literal `''` in an `ENUM` is what a
//! misconfigured server produced once. Recorded rather than implied: the doc used
//! to promise the opposite in the sentence above, and a data-safety property is
//! the kind a reader trusts without re-deriving.

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
    /// A calendar date **and** a time of day, and what the column does with a
    /// UTC offset written into it ([`Zoned`]).
    DateTime(Zoned),
}

/// What the destination column does with a UTC offset in the literal written to
/// it — the one thing a datetime edit cannot read off the *value*.
///
/// **An offset is a property of the destination, not text carried from the old
/// value.** The old text was the server's rendering, and the two engines that
/// resolve an offset do not both put one in it: a MySQL `TIMESTAMP` is rendered
/// in the session zone with no tail at all, so "did the old value carry an
/// offset" answered *no* for the very column that most needs one — and **Now**
/// then sent a client wall clock that the server read as its own. Server at
/// `+00:00`, client at `+02`, and the stored instant is two hours in the future,
/// rendered back in the session zone so the cell re-reads as correct.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zoned {
    /// The column **resolves** an offset: PostgreSQL `timestamptz`, and MySQL's
    /// `TIMESTAMP`, which since 8.0.19 accepts `[+-]hh:mm` in a datetime literal
    /// and converts it to the session zone. Writing an instant here without one
    /// leaves the server to guess which zone the wall clock was in, and it
    /// guesses its own.
    Offset,
    /// The column has nowhere to put one and no zone to resolve it against:
    /// MySQL `DATETIME`, PostgreSQL `timestamp without time zone` (which parses
    /// an offset and **discards** it), SQLite's text stamps. The literal is
    /// stored as written, so the wall clock is the whole of the value and an
    /// offset would be noise at best.
    Naive,
}

/// Does a datetime column resolve a UTC offset, or store the wall clock as
/// written?
///
/// Type name **and** dialect, because the same word means different things: PG's
/// bare `timestamp` is zone-less while MySQL's `TIMESTAMP` is an instant stored
/// in UTC and rendered in the session zone. `base` is the leading type token,
/// lower-cased, as [`editor_for_type`] has it.
fn zoned_for_type(base: &str, dialect: SqlDialect) -> Zoned {
    match base {
        "timestamptz" | "timestamp with time zone" => Zoned::Offset,
        "timestamp" if dialect == SqlDialect::MySql => Zoned::Offset,
        _ => Zoned::Naive,
    }
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
        "datetime" | "timestamp" | "timestamptz" => {
            return CellEditor::DateTime(zoned_for_type(base.as_str(), dialect));
        }
        // PostgreSQL's verbose spellings, which `format_type` really does return.
        "timestamp without time zone" | "timestamp with time zone" => {
            return CellEditor::DateTime(zoned_for_type(base.as_str(), dialect));
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
    let Some((ns, base)) = split_type_name(name) else {
        return plain;
    };
    match schema.find_enum(ns.as_deref(), &base) {
        Some(e) if !e.values.is_empty() => CellEditor::Enum(e.values.clone()),
        _ => plain,
    }
}

/// A PostgreSQL type name as `format_type` writes it, split into `(namespace,
/// name)` **and unquoted** — the form the catalogue holds.
///
/// `format_type` quotes what needs quoting, and the naive `rsplit_once('.')` this
/// replaced kept the quotes: a type named `MyMood` arrived as `"MyMood"` and was
/// looked up under that spelling, which no catalogue row has, so every
/// mixed-case enum type silently kept the plain text field instead of its
/// dropdown. A quoted name may also *contain* the separator (`"a.b"`), where
/// splitting on the last dot picks a namespace out of the middle of one name.
///
/// `None` when the text isn't one or two identifiers — an unterminated quote, a
/// third part, trailing text. A name this cannot read is a name to leave alone,
/// not to guess at.
fn split_type_name(declared: &str) -> Option<(Option<String>, String)> {
    let (first, rest) = type_ident(declared)?;
    if rest.is_empty() {
        return Some((None, first));
    }
    let (second, rest) = type_ident(rest.strip_prefix('.')?)?;
    rest.is_empty().then_some((Some(first), second))
}

/// One identifier off the front: a `"…"` run (in which `""` is one literal quote
/// and a `.` is an ordinary character), or everything up to the next `.`.
///
/// The bare arm keeps spaces, because a bare multi-word type name (`character
/// varying`) is one name — it will simply match no enum.
fn type_ident(s: &str) -> Option<(String, &str)> {
    let Some(body) = s.strip_prefix('"') else {
        let end = s.find('.').unwrap_or(s.len());
        return (!s.is_empty()).then(|| (s[..end].to_string(), &s[end..]));
    };
    let (mut out, mut rest) = (String::new(), body);
    loop {
        let q = rest.find('"')?;
        out.push_str(&rest[..q]);
        rest = &rest[q + 1..];
        match rest.strip_prefix('"') {
            // A doubled quote inside the run is one quote of the name itself.
            Some(r) => {
                out.push('"');
                rest = r;
            }
            None => return Some((out, rest)),
        }
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
        CellEditor::DateTime(_) => Stamp::parse(text).is_some(),
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
                    // **The row already held writes the text that is there**, not
                    // the wire spelling. [`BoolWire`]'s own property is that
                    // choosing what the column already reads back is recognised
                    // as a revert — and that only holds where the engine's read
                    // spelling *is* its write spelling. SQLite has neither type
                    // nor opinion, so a `BOOLEAN` column can legally hold
                    // `'true'`: the picker showed that row as ticked, and
                    // clicking it to confirm staged `1`, a different stored value
                    // from an action whose only visible meaning was "yes, that
                    // one".
                    value: if held == Some(on) {
                        current.to_string()
                    } else {
                        wire.text(on).to_string()
                    },
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
        CellEditor::Text | CellEditor::Date | CellEditor::DateTime(_) => Vec::new(),
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
/// with its fractional seconds — and gains midnight when there was nothing
/// there. Any other editor is not a date column and is left alone; a caller that
/// reached here with one has a bug, and rewriting the cell would hide it.
///
/// **The offset does not come along, on either flavour of column.** It is the
/// old value's, and an offset qualifies a particular instant: `+01` on a Berlin
/// `timestamptz` is true in January and false in July, so carrying it onto a
/// picked July day restates the time of day an hour out — pick 15 July on
/// `2024-01-15 11:30:00+01` and the cell re-reads as `12:30:00+02`. The day
/// changed and so did the thing the user did not touch. Dropping it hands the
/// wall clock to the server to resolve in its session zone, which is what
/// "keep the time of day" means to whoever picked the day.
///
/// [`set_now`] does the opposite and for the same reason: *that* instant is the
/// client's, so it has to be stated. Here the instant is the server's rendering,
/// and the user changed only which day it falls on.
pub fn set_date(editor: &CellEditor, current: &str, date: Date) -> String {
    match editor {
        CellEditor::Date => date.iso(),
        CellEditor::DateTime(_) => match Stamp::parse(current) {
            Some(s) => match s.time() {
                Some(_) => s.with_date(date).with_offset("").render(),
                None => s
                    .with_date(date)
                    .with_time(Time::MIDNIGHT)
                    .with_offset("")
                    .render(),
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
/// * **the offset**, which is the *destination's* to decide and not the old
///   text's to carry ([`Zoned`]). A column that resolves an offset is given the
///   client's, because that is which instant "now" is; a column that cannot is
///   given none, because the wall clock is the whole of the value there.
///
/// **Asking the value was the bug.** The gate used to be "did the old text carry
/// a tail", which reads `false` for a MySQL `TIMESTAMP` — a column rendered in
/// the session zone, with no tail, that resolves an offset perfectly well. So the
/// one case that most needed the offset was the one that never got it: a server
/// at `+00:00` read a 12:35 wall clock from a UTC+2 machine as 12:35 UTC and
/// stored an instant two hours in the future, then rendered it back in its own
/// zone so the cell re-read as correct.
pub fn set_now(editor: &CellEditor, current: &str, now: (Date, Time, &str)) -> String {
    let (date, time, offset) = now;
    match editor {
        CellEditor::Date => date.iso(),
        CellEditor::DateTime(zoned) => {
            let stamp = Stamp::parse(current).unwrap_or_else(|| Stamp::from_date(date));
            let stamp = stamp.with_date(date).with_time(time).without_frac();
            stamp
                .with_offset(match zoned {
                    Zoned::Offset => offset,
                    // Not merely "don't add one": a tail the destination cannot
                    // resolve is noise on the way in, and on PostgreSQL's bare
                    // `timestamp` it is silently discarded — so leaving one there
                    // would suggest the instant was pinned when it was not.
                    Zoned::Naive => "",
                })
                .render()
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

    /// **A type whose name needs quoting is still that type.** `format_type`
    /// quotes what has to be quoted, and the quotes were being carried into the
    /// lookup — so every mixed-case enum type resolved to nothing and silently
    /// kept the plain text field.
    #[test]
    fn a_quoted_postgres_enum_type_still_resolves() {
        let schema = DbSchema {
            enums: vec![
                EnumInfo {
                    name: "MyMood".into(),
                    schema: Some("public".into()),
                    values: vec!["Sad".into(), "Happy".into()],
                    comment: None,
                },
                EnumInfo {
                    name: "od.d".into(),
                    schema: Some("my schema".into()),
                    values: vec!["one".into()],
                    comment: None,
                },
            ],
            ..Default::default()
        };
        for declared in ["\"MyMood\"", "public.\"MyMood\""] {
            let e = editor_for_column(declared, Postgres, Some(&schema));
            assert_eq!(members(&e), ["Sad", "Happy"], "{declared}");
        }
        // The separator inside a quoted part: splitting on the last dot took
        // `"my schema"."od` for a namespace and `d"` for a name.
        let e = editor_for_column("\"my schema\".\"od.d\"", Postgres, Some(&schema));
        assert_eq!(members(&e), ["one"]);
    }

    /// The parts, on their own — including the two shapes that have no name to
    /// look up and must not be guessed at.
    #[test]
    fn a_type_name_splits_on_the_separator_outside_the_quotes() {
        let split = |s: &str| split_type_name(s).map(|(ns, n)| (ns.unwrap_or_default(), n));
        assert_eq!(split("mood"), Some((String::new(), "mood".into())));
        assert_eq!(split("sales.mood"), Some(("sales".into(), "mood".into())));
        assert_eq!(split("\"MyMood\""), Some((String::new(), "MyMood".into())));
        // A doubled quote is one quote of the name.
        assert_eq!(split("\"a\"\"b\""), Some((String::new(), "a\"b".into())));
        // A bare multi-word type is one name; it simply matches no enum.
        assert_eq!(
            split("character varying"),
            Some((String::new(), "character varying".into()))
        );
        assert_eq!(split("\"unterminated"), None);
        assert_eq!(split("a.b.c"), None, "three parts is not a type name");
        assert_eq!(split("\"a\"trailing"), None);
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

    /// A datetime column that stores the wall clock as written — MySQL
    /// `DATETIME`, PostgreSQL `timestamp`, SQLite.
    fn naive() -> CellEditor {
        CellEditor::DateTime(Zoned::Naive)
    }

    /// One that resolves a UTC offset — MySQL `TIMESTAMP`, PostgreSQL
    /// `timestamptz`.
    fn zoned() -> CellEditor {
        CellEditor::DateTime(Zoned::Offset)
    }

    /// The calendar's columns, **and what each of them does with an offset**.
    /// The same word does not mean the same thing on two engines: MySQL's
    /// `TIMESTAMP` is an instant stored in UTC and rendered in the session zone,
    /// PostgreSQL's bare `timestamp` has no zone at all and discards one it is
    /// handed.
    #[test]
    fn the_date_family_maps_to_a_calendar() {
        use CellEditor::DateTime;
        assert_eq!(editor_for_type("date", MySql), CellEditor::Date);
        assert_eq!(editor_for_type("DATE", Sqlite), CellEditor::Date);
        // MySQL: `DATETIME` stores the wall clock as written; `TIMESTAMP`
        // resolves an offset and needs one.
        assert_eq!(editor_for_type("datetime", MySql), DateTime(Zoned::Naive));
        assert_eq!(
            editor_for_type("datetime(6)", MySql),
            DateTime(Zoned::Naive)
        );
        assert_eq!(
            editor_for_type("timestamp", MySql),
            DateTime(Zoned::Offset),
            "a MySQL TIMESTAMP is rendered in the session zone, so a bare wall \
             clock written to it means whatever the *server* is set to"
        );
        // PostgreSQL: the distinction is in the type's own name.
        assert_eq!(
            editor_for_type("timestamptz", Postgres),
            DateTime(Zoned::Offset)
        );
        assert_eq!(
            editor_for_type("timestamp with time zone", Postgres),
            DateTime(Zoned::Offset)
        );
        assert_eq!(
            editor_for_type("timestamp(3) without time zone", Postgres),
            DateTime(Zoned::Naive)
        );
        assert_eq!(
            editor_for_type("timestamp", Postgres),
            DateTime(Zoned::Naive),
            "PostgreSQL's bare `timestamp` is the zone-less one — the opposite \
             of MySQL's"
        );
        // SQLite stores what it is given and has no session zone to resolve
        // against, so a tail would be text nothing interprets.
        assert_eq!(editor_for_type("timestamp", Sqlite), DateTime(Zoned::Naive));
        assert_eq!(editor_for_type("datetime", Sqlite), DateTime(Zoned::Naive));
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

    /// The exception, and — since the module doc used to claim the opposite —
    /// what it costs: a MySQL `ENUM` holding a literal `''` gets the dropdown
    /// like a NULL cell does, and the dropdown cannot write `''` back. Pinned so
    /// the limitation is a decision on the record rather than a surprise.
    #[test]
    fn an_empty_value_fits_every_control() {
        for e in [
            CellEditor::Bool(BoolWire::OneZero),
            CellEditor::Enum(vec!["a".into()]),
            CellEditor::Set(vec!["a".into()]),
            CellEditor::Date,
            naive(),
        ] {
            assert!(fits(&e, ""), "{e:?}");
        }
        // No row of the picker writes it back — the cost, stated.
        assert!(
            !pick_options(&CellEditor::Enum(vec!["a".into(), "b".into()]), "")
                .iter()
                .any(|o| o.value.is_empty()),
            "the dropdown has no way back to the empty string"
        );
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
        assert!(fits(&naive(), "2024-01-15 10:00:00"));
        assert!(fits(&naive(), "2024-01-15"));
        assert!(!fits(&naive(), "0000-00-00 00:00:00"));
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
            set_date(&naive(), "2020-05-05 23:59:59.250", day(2024, 1, 15)),
            "2024-01-15 23:59:59.250"
        );
    }

    /// **And the offset is not part of the time of day.** `+01` on a Berlin
    /// `timestamptz` is true in January and false in July, so carrying it onto a
    /// picked July day restates the wall clock an hour out: the value below used
    /// to come back `2024-07-15 11:30:00+01`, which the server stores as
    /// `10:30Z` and renders back as **`12:30:00+02`**. The day changed and so did
    /// the one thing the user did not touch.
    ///
    /// Dropped rather than recomputed: recomputing needs the zone's DST rules,
    /// which `core` has no business carrying, and the server resolves a bare wall
    /// clock in its session zone — which is what "keep the time of day" means to
    /// whoever picked the day.
    #[test]
    fn picking_a_day_does_not_carry_the_old_values_offset() {
        assert_eq!(
            set_date(&zoned(), "2024-01-15 11:30:00+01", day(2024, 7, 15)),
            "2024-07-15 11:30:00"
        );
        // A `Z` is an offset too, and the same reasoning applies.
        assert_eq!(
            set_date(&zoned(), "2024-01-15 11:30:00Z", day(2024, 7, 15)),
            "2024-07-15 11:30:00"
        );
        // A column that cannot hold one had none to drop, and the fraction —
        // which *is* part of the time of day — stays either way.
        assert_eq!(
            set_date(&naive(), "2020-05-05 23:59:59.250", day(2024, 1, 15)),
            "2024-01-15 23:59:59.250"
        );
        // A value with no time of day gains midnight and still no tail.
        assert_eq!(
            set_date(&zoned(), "2024-01-15+01", day(2024, 7, 15)),
            "2024-07-15 00:00:00"
        );
    }

    #[test]
    fn picking_a_day_on_an_empty_datetime_starts_at_midnight() {
        assert_eq!(
            set_date(&naive(), "", day(2024, 1, 15)),
            "2024-01-15 00:00:00"
        );
        // Same for a value the parser can make nothing of — the cell had no time.
        assert_eq!(
            set_date(&naive(), "0000-00-00 00:00:00", day(2024, 1, 15)),
            "2024-01-15 00:00:00"
        );
    }

    #[test]
    fn a_datetime_column_holding_only_a_date_gains_midnight() {
        assert_eq!(
            set_date(&naive(), "2020-05-05", day(2024, 1, 15)),
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
            set_now(&naive(), "2020-05-05 10:00:00", now_at(23, 5, 9, "+02:00")),
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
                &zoned(),
                "2020-05-05 10:00:00.123456+00",
                now_at(23, 5, 9, "+02:00")
            ),
            "2024-01-15 23:05:09+02:00"
        );
    }

    /// **The destination decides the offset, not the old text.** A MySQL
    /// `TIMESTAMP` is rendered in the session zone and so never carries a tail —
    /// which is exactly the column that must be given one. Asking the value
    /// instead ("did the old text have an offset?") answered *no* here, so
    /// **Now** sent a bare client wall clock: server at `+00:00`, client at
    /// `+02`, and the stored instant is two hours in the future, rendered back in
    /// the session zone so the cell re-reads as correct.
    #[test]
    fn stamping_now_offsets_a_column_that_resolves_one_however_the_value_reads() {
        for current in [
            // A MySQL `TIMESTAMP`, as the server renders it: no tail at all.
            "2020-05-05 10:00:00",
            // A NULL cell switched to a value, or a pending row's blank.
            "",
            // Something the parser can make nothing of.
            "0000-00-00 00:00:00",
        ] {
            assert_eq!(
                set_now(&zoned(), current, now_at(9, 0, 0, "+02:00")),
                "2024-01-15 09:00:00+02:00",
                "current = {current:?}"
            );
        }
    }

    /// And the other half of the same rule: a column with nowhere to put an
    /// offset is never given one — MySQL's `DATETIME` and PostgreSQL's bare
    /// `timestamp`, where the wall clock *is* the value.
    #[test]
    fn stamping_now_does_not_invent_an_offset() {
        assert_eq!(
            set_now(&naive(), "", now_at(9, 0, 0, "+02:00")),
            "2024-01-15 09:00:00"
        );
        assert_eq!(
            set_now(&naive(), "2020-05-05 10:00:00", now_at(9, 0, 0, "+02:00")),
            "2024-01-15 09:00:00"
        );
        // Nor kept: a tail the destination cannot resolve is noise on the way in,
        // and PostgreSQL's `timestamp` discards it silently.
        assert_eq!(
            set_now(
                &naive(),
                "2020-05-05 10:00:00+05",
                now_at(9, 0, 0, "+02:00")
            ),
            "2024-01-15 09:00:00"
        );
    }

    /// **The clock read the production path actually makes.** `local_now` is
    /// called by the calendar's `Now` and by nothing else, and by no test — so
    /// the specifier its offset is rendered with (`%:z`, chosen because MySQL
    /// 8.0.19+ needs `[+-]hh:mm`) was asserted nowhere: `%z` yields `+0200`,
    /// which MySQL rejects, and the whole suite stayed green.
    ///
    /// No assertion about *what time it is* — the parts have to be valid and the
    /// offset has to survive a round trip through the writer, which is what a
    /// specifier typo breaks.
    #[test]
    fn the_clock_the_now_button_reads_round_trips_through_the_writer() {
        let (d, t, off) = crate::date::local_now();
        assert_eq!(
            Date::new(d.year, d.month, d.day),
            Some(d),
            "local_now handed out a date that is not one: {d:?}"
        );
        assert_eq!(
            Time::new(t.hour, t.minute, t.second),
            Some(t),
            "local_now handed out a time that is not one: {t:?}"
        );
        // `[+-]hh:mm`, and nothing else — the form both engines read.
        let b = off.as_bytes();
        assert!(
            off.len() == 6
                && (b[0] == b'+' || b[0] == b'-')
                && b[1].is_ascii_digit()
                && b[2].is_ascii_digit()
                && b[3] == b':'
                && b[4].is_ascii_digit()
                && b[5].is_ascii_digit(),
            "the offset must be [+-]hh:mm, got {off:?}"
        );
        // The round trip: what `Now` writes for a zoned column parses back, and
        // its tail is the offset the clock gave — `+0200` would not survive this.
        let written = set_now(&zoned(), "2024-01-15 10:00:00+00", (d, t, &off));
        let back = Stamp::parse(&written).expect("what Now writes must parse back");
        assert!(
            written.ends_with(&off),
            "the offset was reshaped on the way out: {written:?} vs {off:?}"
        );
        assert_eq!(back.date(), d);
        assert_eq!(back.time(), Some(t));
        assert!(back.has_offset());
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

    /// **Confirming the row already ticked must not rewrite the value.**
    /// `read_bool` is deliberately wider than `BoolWire::text` — it reads every
    /// spelling the three engines hand back — and SQLite is the one engine whose
    /// read spelling is whatever was inserted, so a `BOOLEAN` column can legally
    /// hold `'true'`. The picker showed *true* as held and writing
    /// `BoolWire::of(Sqlite).text(true)` for it staged `1`: a different stored
    /// value, from a click whose only visible meaning was "yes, that one" — and it
    /// breaks the revert property `BoolWire`'s own doc is built on.
    #[test]
    fn confirming_the_row_a_boolean_already_holds_writes_what_is_there() {
        for text in ["true", "TRUE", "yes", "on", "t"] {
            let opts = pick_options(&CellEditor::Bool(BoolWire::OneZero), text);
            assert!(opts[1].held, "{text}: the true row is the held one");
            assert_eq!(
                opts[1].value, text,
                "{text}: confirming it must stage the text the cell already has"
            );
            // The *other* row is a real change, so it writes the engine's own
            // spelling — that is what the wire is for.
            assert_eq!(opts[0].value, "0", "{text}");
            assert!(!opts[0].held, "{text}");
        }
        // And the false side of the same rule.
        let opts = pick_options(&CellEditor::Bool(BoolWire::OneZero), "off");
        assert_eq!(opts[0].value, "off");
        assert_eq!(opts[1].value, "1");
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
        for e in [CellEditor::Text, CellEditor::Date, naive()] {
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
