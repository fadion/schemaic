//! Query parameters: `:name` placeholders in a statement — the pure half.
//!
//! A placeholder is `:` followed by an identifier, and **named only**. There is
//! no `?` and no `$1`: `?` is a live JSON operator in PostgreSQL (`?`, `?|`,
//! `?&`) and `$1` collides with dollar-quoting, so a positional spelling would
//! have to mean different things per dialect — and a name is what the parameters
//! bar has to label its rows with anyway.
//!
//! # Substitution is textual, and happens *before* the write guard
//!
//! The value is rendered into the SQL here rather than bound by the driver, for
//! three reasons that all bite: `ORDER BY :col` and `LIMIT :n` are what people
//! actually reach for and no driver binds those; a batch run hands the executor
//! whole statements; and the three engines spell a bind marker three ways.
//!
//! The cost of that choice is that the text a placeholder expands to is SQL, so
//! the run guard must see the **substituted** statement. `TabsActions::run` runs
//! [`substitute`] first and passes the result to `sql::run_verdict` — a
//! [`ParamValue::Raw`] otherwise carries a write past a guard that was reading a
//! statement the engine never receives. Values themselves are quoted through
//! [`crate::export::sql_literal`], the one literal writer.
//!
//! # Three things that look like a placeholder and are not
//!
//! - **`::`** — PostgreSQL's cast. It has to be consumed *whole*: skipping one
//!   byte leaves a `:` in front of a word, which is precisely the shape being
//!   looked for, so `id::text` would yield a parameter called `text`.
//! - **`:=`** — MySQL's assignment operator. Excluded by requiring a name to
//!   start where [`crate::sql::is_word_start`] says one can.
//! - **`arr[lo:hi]`** — a PostgreSQL array slice. Excluded by requiring the byte
//!   *before* the `:` not to be part of a word (or a `]`), which a placeholder's
//!   never is and a slice bound's always is.
//!
//! Everything inside a string, comment, dollar-quoted body or quoted identifier
//! is skipped by [`crate::sql::skip_noncode`], per the one-boundary-lexer rule —
//! this module adds no scanner of its own.

use std::fmt;

use crate::export::sql_literal;
use crate::intel::SqlDialect;
use crate::model::Value;
use crate::sql::{is_word_byte, is_word_start, skip_noncode};

/// One `:name` occurrence in a statement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParamRef {
    pub name: String,
    /// Byte offset of the `:`.
    pub start: usize,
    /// Byte offset just past the last byte of the name.
    pub end: usize,
}

/// What a parameter is set to.
///
/// **Deliberately not `Serialize`/`Deserialize`.** A parameter value is often an
/// id, and often enough an email address or a token pasted into a `WHERE` — it
/// lives on the tab for the session and is never written to `tabs.json`. The
/// missing derive is the enforcement; a future `#[derive(Serialize)]` here is the
/// change to refuse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParamValue {
    /// A string, quoted and escaped for the dialect.
    Text(String),
    /// A numeric literal, emitted bare. Kept as text rather than an `f64` so a
    /// bigint past 2^53 and a trailing zero both survive; validated on the way
    /// out, since "emitted bare" is the whole risk.
    Number(String),
    /// SQL `NULL`.
    Null,
    /// Emitted verbatim — the deliberate escape hatch for a column name, an
    /// `ORDER BY` tail, a `LIMIT`. The parameters bar marks these.
    Raw(String),
}

/// Which of the four [`ParamValue`] shapes a bar row is set to, without its
/// text — what the row's kind chip cycles through.
///
/// Split out so the bar can change the kind while the typed text stays put:
/// a value typed as `Text` and then switched to `Raw` is the same string, and
/// re-typing it to change how it is quoted would be the wrong instrument.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ParamKind {
    #[default]
    Text,
    Number,
    Null,
    Raw,
}

impl ParamKind {
    /// The chip's label. Short enough to sit in a row that is mostly input.
    pub fn label(self) -> &'static str {
        match self {
            ParamKind::Text => "abc",
            ParamKind::Number => "123",
            ParamKind::Null => "NULL",
            ParamKind::Raw => "SQL",
        }
    }

    /// The next kind, for the chip's click. Ordered by how often each is
    /// wanted, with the unquoted one last.
    pub fn next(self) -> ParamKind {
        match self {
            ParamKind::Text => ParamKind::Number,
            ParamKind::Number => ParamKind::Null,
            ParamKind::Null => ParamKind::Raw,
            ParamKind::Raw => ParamKind::Text,
        }
    }

    /// Does this kind have text of its own? `NULL` does not, so the row's field
    /// is inert rather than empty-and-editable.
    pub fn takes_text(self) -> bool {
        !matches!(self, ParamKind::Null)
    }
}

impl ParamValue {
    /// The value a kind plus the row's text makes.
    pub fn of(kind: ParamKind, text: &str) -> ParamValue {
        match kind {
            ParamKind::Text => ParamValue::Text(text.to_string()),
            ParamKind::Number => ParamValue::Number(text.to_string()),
            ParamKind::Null => ParamValue::Null,
            ParamKind::Raw => ParamValue::Raw(text.to_string()),
        }
    }

    /// Which kind this is.
    pub fn kind(&self) -> ParamKind {
        match self {
            ParamValue::Text(_) => ParamKind::Text,
            ParamValue::Number(_) => ParamKind::Number,
            ParamValue::Null => ParamKind::Null,
            ParamValue::Raw(_) => ParamKind::Raw,
        }
    }

    /// The text the row's field shows. Empty for `NULL`, which has none.
    pub fn text(&self) -> &str {
        match self {
            ParamValue::Text(s) | ParamValue::Number(s) | ParamValue::Raw(s) => s,
            ParamValue::Null => "",
        }
    }
}

/// Set (or clear) one name's value in a bar's store, leaving the rest alone.
///
/// Appends a name the store hasn't seen. A name the *query* no longer contains
/// is deliberately left in place: [`bindings_for`] decides which rows are shown,
/// and dropping the value here would mean an edit that briefly removes a
/// placeholder — a retyped `WHERE`, an undo — throws away what was typed into
/// it.
pub fn set_value(store: &mut Vec<Binding>, name: &str, value: Option<ParamValue>) {
    match store.iter_mut().find(|b| b.name == name) {
        Some(b) => b.value = value,
        None => store.push(Binding {
            name: name.to_string(),
            value,
        }),
    }
}

/// One row of the parameters bar: a name the SQL asks for, and what the user has
/// put in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub name: String,
    /// `None` until a value is typed. The run is held rather than guessed at —
    /// an empty string and "no value yet" are different answers.
    pub value: Option<ParamValue>,
}

/// Why a substitution could not be performed. Rendered into the run-guard bar
/// through its [`fmt::Display`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParamError {
    /// Names the statement asks for that have no value, in first-appearance
    /// order. All of them at once: the bar shows every empty row, and reporting
    /// one at a time would walk the user through them.
    Missing(Vec<String>),
    /// A [`ParamValue::Number`] whose text is not a numeric literal. It would be
    /// emitted bare, so `1 OR 1=1` has to be refused rather than run.
    NotANumber { name: String, text: String },
}

impl fmt::Display for ParamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParamError::Missing(names) => {
                let list = names
                    .iter()
                    .map(|n| format!(":{n}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "No value for {list}")
            }
            ParamError::NotANumber { name, text } => {
                write!(f, ":{name} is a number, but reads \"{text}\"")
            }
        }
    }
}

/// Every placeholder in `sql`, in source order, with the byte range of each
/// (the `:` included). Repeats are kept — [`substitute`] replaces all of them.
pub fn scan(sql: &str, dialect: SqlDialect) -> Vec<ParamRef> {
    let b = sql.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        // Strings, comments, dollar-quoted bodies, quoted identifiers.
        if let Some(j) = skip_noncode(b, i, dialect) {
            i = j.max(i + 1);
            continue;
        }
        if b[i] != b':' {
            i += 1;
            continue;
        }
        // A cast, consumed whole — see the module docs.
        if b.get(i + 1) == Some(&b':') {
            i += 2;
            continue;
        }
        // An array slice's `:` is attached to the bound before it; a
        // placeholder's `:` never is.
        let attached = i > 0 && (is_word_byte(b[i - 1]) || b[i - 1] == b']');
        if attached || !b.get(i + 1).copied().is_some_and(is_word_start) {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < b.len() && is_word_byte(b[j]) {
            j += 1;
        }
        out.push(ParamRef {
            name: sql[i + 1..j].to_string(),
            start: i,
            end: j,
        });
        i = j;
    }
    out
}

/// The distinct placeholder names, in the order they first appear — the order
/// the parameters bar lists its rows in.
///
/// Case-sensitive: `:id` and `:ID` are two parameters. Folding them would have
/// to pick a case to show in the bar and a rule for which of two typed values
/// wins, and no engine is being asked about the name at all.
pub fn names(sql: &str, dialect: SqlDialect) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for r in scan(sql, dialect) {
        if !out.contains(&r.name) {
            out.push(r.name);
        }
    }
    out
}

/// The parameters bar's rows for `sql`, keeping any value already typed for a
/// name that is still there.
///
/// The rows are *derived from the SQL on every edit*, never stored, so they
/// cannot drift from the statement they belong to; carrying the typed values
/// across is what stops an edit elsewhere in the query from emptying the bar.
pub fn bindings_for(sql: &str, dialect: SqlDialect, existing: &[Binding]) -> Vec<Binding> {
    names(sql, dialect)
        .into_iter()
        .map(|name| {
            let value = existing
                .iter()
                .find(|b| b.name == name)
                .and_then(|b| b.value.clone());
            Binding { name, value }
        })
        .collect()
}

/// `sql` with every placeholder replaced by its bound value, or the reason it
/// can't be.
///
/// **This is what runs.** Its output is what `sql::run_verdict` must be given
/// and what the history log records — the statement the engine actually sees.
pub fn substitute(
    sql: &str,
    bindings: &[Binding],
    dialect: SqlDialect,
) -> Result<String, ParamError> {
    let refs = scan(sql, dialect);
    if refs.is_empty() {
        return Ok(sql.to_string());
    }
    let value_of = |name: &str| {
        bindings
            .iter()
            .find(|b| b.name == name)
            .and_then(|b| b.value.as_ref())
    };
    let mut missing: Vec<String> = Vec::new();
    for r in &refs {
        if value_of(&r.name).is_none() && !missing.contains(&r.name) {
            missing.push(r.name.clone());
        }
    }
    if !missing.is_empty() {
        return Err(ParamError::Missing(missing));
    }
    let mut out = String::with_capacity(sql.len());
    let mut cut = 0;
    for r in &refs {
        let value = value_of(&r.name).expect("every name has a value; checked above");
        out.push_str(&sql[cut..r.start]);
        out.push_str(&render(&r.name, value, dialect)?);
        cut = r.end;
    }
    out.push_str(&sql[cut..]);
    Ok(out)
}

/// The SQL text one value becomes.
fn render(name: &str, value: &ParamValue, dialect: SqlDialect) -> Result<String, ParamError> {
    Ok(match value {
        ParamValue::Text(s) => sql_literal(&Value::Str(s.clone()), dialect),
        ParamValue::Null => sql_literal(&Value::Null, dialect),
        ParamValue::Raw(s) => s.clone(),
        ParamValue::Number(s) => {
            let trimmed = s.trim();
            if !is_numeric_literal(trimmed) {
                return Err(ParamError::NotANumber {
                    name: name.to_string(),
                    text: s.clone(),
                });
            }
            trimmed.to_string()
        }
    })
}

/// Drop the diagnostics that [`neutralize`]'s rewrite is responsible for.
///
/// `_id` is a perfectly good identifier and names nothing, so the analysis
/// reports it as an unknown column — a squiggle under text the user wrote
/// correctly. Anything overlapping a placeholder's range goes; everything else
/// in the statement survives, which is the point of neutralising rather than
/// skipping analysis altogether.
pub fn strip_param_diagnostics(
    diagnostics: Vec<crate::intel::Diagnostic>,
    refs: &[ParamRef],
) -> Vec<crate::intel::Diagnostic> {
    if refs.is_empty() {
        return diagnostics;
    }
    diagnostics
        .into_iter()
        .filter(|d| {
            !refs
                .iter()
                .any(|r| d.range.0 < r.end && r.start < d.range.1)
        })
        .collect()
}

/// Substitute a whole run's statements, then ask the write guard about the
/// **result** — the one entry point a run action should use.
///
/// The ordering is the point, and it is structural here rather than a rule
/// stated in a comment somewhere upstream. A [`ParamValue::Raw`] expands to
/// arbitrary SQL, so a guard shown the *template* is reading a statement the
/// engine will never receive: `:action FROM t` carries no write, and
/// `DELETE FROM t` — which is what runs — carries one. Substituting first and
/// handing back both halves means a caller cannot get a verdict without also
/// getting the statements it was reached for.
///
/// `Err` is a parameter problem, and it stops the run *before* the guard is
/// consulted: there is nothing to judge yet.
pub fn prepare_run(
    stmts: &[String],
    bindings: &[Binding],
    policy: crate::sql::GuardPolicy,
) -> Result<(Vec<String>, crate::sql::RunVerdict), ParamError> {
    let prepared = stmts
        .iter()
        .map(|s| substitute(s, bindings, policy.dialect))
        .collect::<Result<Vec<_>, _>>()?;
    let verdict = crate::sql::run_verdict(&prepared, policy);
    Ok((prepared, verdict))
}

/// Is `s` a plain numeric literal — optional sign, digits with at most one
/// decimal point, optional exponent, and nothing else?
///
/// Hand-rolled rather than `str::parse::<f64>()`: `f64` accepts `inf` and `NaN`
/// (neither of which is SQL), and rounds a bigint that has to be emitted exactly
/// as typed.
fn is_numeric_literal(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = usize::from(matches!(b.first(), Some(b'-' | b'+')));
    let mut digits = 0_usize;
    let mut dot = false;
    while i < b.len() {
        match b[i] {
            d if d.is_ascii_digit() => digits += 1,
            b'.' if !dot => dot = true,
            b'e' | b'E' if digits > 0 => {
                i += 1;
                if matches!(b.get(i), Some(b'-' | b'+')) {
                    i += 1;
                }
                let exp_start = i;
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
                return i > exp_start && i == b.len();
            }
            _ => return false,
        }
        i += 1;
    }
    digits > 0
}

/// `sql` with each placeholder's `:` rewritten to `_`, so the text parses as
/// ordinary SQL with **every byte offset preserved**.
///
/// This is what the `intel` layer analyses while the user is typing: a `:id` is
/// a syntax error to the parser, and one of those blanks the diagnostics for the
/// whole statement. `_id` is an identifier, so the statement parses and the rest
/// of the diagnostics survive; the caller suppresses unknown-column reports over
/// the ranges [`scan`] returns, since `_id` names nothing.
///
/// It is not a complete answer, and isn't meant to be: a placeholder standing
/// where the grammar demands a literal rather than an expression (MySQL's
/// `LIMIT :n`) still fails to parse, and falls back the way any unparseable
/// statement does.
pub fn neutralize(sql: &str, dialect: SqlDialect) -> String {
    let refs = scan(sql, dialect);
    if refs.is_empty() {
        return sql.to_string();
    }
    let mut out = String::with_capacity(sql.len());
    let mut cut = 0;
    for r in &refs {
        out.push_str(&sql[cut..r.start]);
        // One ASCII byte for one ASCII byte: the offsets downstream are the
        // point of this function.
        out.push('_');
        out.push_str(&sql[r.start + 1..r.end]);
        cut = r.end;
    }
    out.push_str(&sql[cut..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const MY: SqlDialect = SqlDialect::MySql;
    const PG: SqlDialect = SqlDialect::Postgres;
    const LITE: SqlDialect = SqlDialect::Sqlite;

    fn found(sql: &str, dialect: SqlDialect) -> Vec<String> {
        scan(sql, dialect).into_iter().map(|r| r.name).collect()
    }

    fn bound(name: &str, value: ParamValue) -> Binding {
        Binding {
            name: name.to_string(),
            value: Some(value),
        }
    }

    // ── scan ────────────────────────────────────────────────────────────────

    #[test]
    fn finds_a_named_placeholder() {
        let sql = "SELECT * FROM t WHERE id = :id";
        assert_eq!(
            scan(sql, MY),
            vec![ParamRef {
                name: "id".to_string(),
                start: 27,
                end: 30,
            }]
        );
        assert_eq!(&sql[27..30], ":id", "the range covers the colon");
    }

    #[test]
    fn a_placeholder_inside_a_string_literal_is_not_one() {
        assert!(found("SELECT ':id' FROM t", MY).is_empty());
        assert!(found("SELECT ':id' FROM t", PG).is_empty());
        // MySQL reads `"` as a string too; PostgreSQL reads it as an identifier,
        // and `skip_noncode` skips either — neither yields a placeholder.
        assert!(found("SELECT \":id\" FROM t", MY).is_empty());
        assert!(found("SELECT \":id\" FROM t", PG).is_empty());
    }

    #[test]
    fn a_placeholder_in_a_comment_is_not_one() {
        assert!(found("SELECT 1 -- :id\n", MY).is_empty());
        assert!(found("SELECT /* :id */ 1", MY).is_empty());
        assert!(found("SELECT 1 # :id\n", MY).is_empty());
    }

    #[test]
    fn a_placeholder_inside_a_quoted_identifier_is_not_one() {
        assert!(found("SELECT `a:id` FROM t", MY).is_empty());
        assert!(found("SELECT [a:id] FROM t", LITE).is_empty());
    }

    /// PostgreSQL's cast operator. Skipping one byte would leave the second `:`
    /// followed by a word — exactly the shape of a placeholder — so `::` has to
    /// be consumed whole.
    #[test]
    fn a_postgres_cast_is_not_a_placeholder() {
        assert!(found("SELECT id::text FROM t", PG).is_empty());
        assert_eq!(found("SELECT * FROM t WHERE id = :id::text", PG), ["id"]);
    }

    #[test]
    fn the_mysql_assignment_operator_is_not_a_placeholder() {
        assert!(found("SET @x := 1", MY).is_empty());
        assert!(found("SELECT @rownum:=@rownum+1 FROM t", MY).is_empty());
    }

    #[test]
    fn a_placeholder_in_a_dollar_quoted_body_is_not_one() {
        assert!(found("CREATE FUNCTION f() AS $$ SELECT :id $$ LANGUAGE sql", PG).is_empty());
        // MySQL has no dollar-quoting, so there the same text is a placeholder —
        // the skip is a dialect capability, not a constant.
        assert_eq!(found("SELECT $$ :id $$", MY), ["id"]);
    }

    #[test]
    fn a_digit_cannot_start_a_placeholder_name() {
        assert!(found("SELECT arr[1:3] FROM t", PG).is_empty());
    }

    /// An array slice whose bounds are names. The `:` is attached to the word
    /// before it, which a placeholder's never is.
    #[test]
    fn an_array_slice_between_names_is_not_a_placeholder() {
        assert!(found("SELECT arr[lo:hi] FROM t", PG).is_empty());
        assert!(found("SELECT arr[1:hi] FROM t", PG).is_empty());
    }

    #[test]
    fn a_bare_colon_yields_nothing() {
        assert!(found("SELECT 1 :", MY).is_empty());
        assert!(found("SELECT 1 ::", PG).is_empty());
        assert!(found(":", MY).is_empty());
    }

    #[test]
    fn a_name_runs_over_non_ascii_word_bytes() {
        // The `>= 0x80` half of `sql::is_word_byte` — a name must not split
        // inside a multi-byte character.
        assert_eq!(found("SELECT * FROM t WHERE n = :naïve", MY), ["naïve"]);
        assert_eq!(found("SELECT * FROM t WHERE n = :ünver", MY), ["ünver"]);
    }

    #[test]
    fn a_name_stops_at_punctuation() {
        assert_eq!(found("WHERE a = :id,b = :x)", MY), ["id", "x"]);
    }

    #[test]
    fn names_are_distinct_in_first_appearance_order() {
        let sql = "WHERE a = :b AND c = :a AND d = :b";
        assert_eq!(names(sql, MY), ["b", "a"]);
    }

    #[test]
    fn names_are_case_sensitive() {
        assert_eq!(names("WHERE a = :id AND b = :ID", MY), ["id", "ID"]);
    }

    // ── substitute ──────────────────────────────────────────────────────────

    #[test]
    fn substitution_is_a_no_op_without_placeholders() {
        let sql = "SELECT 1";
        assert_eq!(substitute(sql, &[], MY), Ok(sql.to_string()));
    }

    #[test]
    fn text_is_quoted_through_sql_literal() {
        let out = substitute(
            "SELECT * FROM t WHERE name = :n",
            &[bound("n", ParamValue::Text("O'Brien".to_string()))],
            MY,
        );
        assert_eq!(
            out,
            Ok("SELECT * FROM t WHERE name = 'O''Brien'".to_string())
        );
    }

    /// The composition, not the quoter: a backslash is an escape in MySQL and a
    /// literal byte in PostgreSQL, and `substitute` must be passing the dialect
    /// down rather than picking one.
    #[test]
    fn a_backslash_in_text_follows_the_dialect() {
        let path = ParamValue::Text("C:\\tmp".to_string());
        assert_eq!(
            substitute("SELECT :p", &[bound("p", path.clone())], MY),
            Ok("SELECT 'C:\\\\tmp'".to_string())
        );
        assert_eq!(
            substitute("SELECT :p", &[bound("p", path)], PG),
            Ok("SELECT 'C:\\tmp'".to_string())
        );
    }

    #[test]
    fn null_and_number_are_emitted_bare() {
        assert_eq!(
            substitute(
                "SELECT :a, :b",
                &[
                    bound("a", ParamValue::Null),
                    bound("b", ParamValue::Number("42".to_string())),
                ],
                MY
            ),
            Ok("SELECT NULL, 42".to_string())
        );
    }

    #[test]
    fn a_number_may_be_signed_or_fractional_or_exponent() {
        for text in ["-1", "+2", "3.5", "1e6", "-2.5E-3", "0"] {
            assert_eq!(
                substitute(
                    "SELECT :n",
                    &[bound("n", ParamValue::Number(text.to_string()))],
                    MY
                ),
                Ok(format!("SELECT {text}")),
                "{text} is a number"
            );
        }
    }

    #[test]
    fn a_number_that_is_not_one_is_an_error() {
        for text in ["", "abc", "1 OR 1=1", "1.2.3", "1e", "--"] {
            assert_eq!(
                substitute(
                    "SELECT :n",
                    &[bound("n", ParamValue::Number(text.to_string()))],
                    MY
                ),
                Err(ParamError::NotANumber {
                    name: "n".to_string(),
                    text: text.to_string(),
                }),
                "{text} is not a number"
            );
        }
    }

    #[test]
    fn raw_is_emitted_verbatim() {
        assert_eq!(
            substitute(
                "SELECT * FROM t ORDER BY :col",
                &[bound("col", ParamValue::Raw("created_at DESC".to_string()))],
                MY
            ),
            Ok("SELECT * FROM t ORDER BY created_at DESC".to_string())
        );
    }

    #[test]
    fn every_occurrence_of_a_name_is_replaced() {
        assert_eq!(
            substitute(
                "SELECT :a FROM t WHERE x = :a OR y = :a",
                &[bound("a", ParamValue::Number("7".to_string()))],
                MY
            ),
            Ok("SELECT 7 FROM t WHERE x = 7 OR y = 7".to_string())
        );
    }

    /// The offsets are into the *source*, so a value longer or shorter than the
    /// placeholder must not shift the ones after it.
    #[test]
    fn values_of_differing_length_land_in_the_right_places() {
        assert_eq!(
            substitute(
                "WHERE a = :x AND b = :yy AND c = :z",
                &[
                    bound("x", ParamValue::Text("a-much-longer-value".to_string())),
                    bound("yy", ParamValue::Null),
                    bound("z", ParamValue::Number("1".to_string())),
                ],
                MY
            ),
            Ok("WHERE a = 'a-much-longer-value' AND b = NULL AND c = 1".to_string())
        );
    }

    /// The seam between `scan` and `substitute`: a lookalike inside a string is
    /// not a placeholder, so it must survive the rewrite untouched.
    #[test]
    fn a_lookalike_in_a_string_survives_substitution() {
        assert_eq!(
            substitute(
                "WHERE note = ':id' AND id = :id",
                &[bound("id", ParamValue::Number("5".to_string()))],
                MY
            ),
            Ok("WHERE note = ':id' AND id = 5".to_string())
        );
    }

    #[test]
    fn a_missing_value_is_an_error_naming_every_empty_one() {
        let out = substitute(
            "WHERE a = :x AND b = :y AND c = :z",
            &[
                bound("y", ParamValue::Null),
                Binding {
                    name: "x".to_string(),
                    value: None,
                },
            ],
            MY,
        );
        assert_eq!(
            out,
            Err(ParamError::Missing(vec!["x".to_string(), "z".to_string()]))
        );
    }

    #[test]
    fn an_error_says_which_parameters_it_means() {
        let msg = ParamError::Missing(vec!["x".to_string(), "z".to_string()]).to_string();
        assert!(msg.contains(":x") && msg.contains(":z"), "{msg}");
        let msg = ParamError::NotANumber {
            name: "n".to_string(),
            text: "abc".to_string(),
        }
        .to_string();
        assert!(msg.contains(":n") && msg.contains("abc"), "{msg}");
    }

    // ── bindings_for ────────────────────────────────────────────────────────

    #[test]
    fn a_new_name_gets_an_empty_row() {
        assert_eq!(
            bindings_for("WHERE a = :x", MY, &[]),
            vec![Binding {
                name: "x".to_string(),
                value: None,
            }]
        );
    }

    #[test]
    fn editing_the_sql_keeps_a_value_already_typed() {
        let existing = vec![bound("x", ParamValue::Number("7".to_string()))];
        assert_eq!(
            bindings_for("WHERE a = :x AND b = :new", MY, &existing),
            vec![
                bound("x", ParamValue::Number("7".to_string())),
                Binding {
                    name: "new".to_string(),
                    value: None,
                },
            ]
        );
    }

    #[test]
    fn a_name_no_longer_in_the_sql_drops_its_row() {
        let existing = vec![
            bound("x", ParamValue::Null),
            bound("gone", ParamValue::Null),
        ];
        assert_eq!(
            bindings_for("WHERE a = :x", MY, &existing),
            vec![bound("x", ParamValue::Null)]
        );
    }

    #[test]
    fn rows_follow_the_order_the_names_appear_in() {
        let existing = vec![bound("b", ParamValue::Null), bound("a", ParamValue::Null)];
        let rows = bindings_for("WHERE a = :a AND b = :b", MY, &existing);
        assert_eq!(
            rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            ["a", "b"]
        );
    }

    // ── kinds and the bar's store ───────────────────────────────────────────

    #[test]
    fn a_kind_round_trips_through_a_value() {
        for kind in [
            ParamKind::Text,
            ParamKind::Number,
            ParamKind::Null,
            ParamKind::Raw,
        ] {
            assert_eq!(ParamValue::of(kind, "7").kind(), kind);
        }
    }

    #[test]
    fn switching_kind_keeps_the_typed_text() {
        let typed = "created_at DESC";
        let as_text = ParamValue::of(ParamKind::Text, typed);
        let as_raw = ParamValue::of(as_text.kind().next().next().next(), as_text.text());
        assert_eq!(as_raw, ParamValue::Raw(typed.to_string()));
    }

    #[test]
    fn null_carries_no_text() {
        assert_eq!(ParamValue::of(ParamKind::Null, "ignored"), ParamValue::Null);
        assert_eq!(ParamValue::Null.text(), "");
        assert!(!ParamKind::Null.takes_text());
        assert!(ParamKind::Text.takes_text());
    }

    #[test]
    fn cycling_a_kind_four_times_returns_to_it() {
        let mut kind = ParamKind::Text;
        for _ in 0..4 {
            kind = kind.next();
        }
        assert_eq!(kind, ParamKind::Text);
    }

    #[test]
    fn setting_a_value_updates_in_place_or_appends() {
        let mut store = vec![bound("a", ParamValue::Null)];
        set_value(&mut store, "a", Some(ParamValue::Number("1".to_string())));
        set_value(&mut store, "b", Some(ParamValue::Null));
        assert_eq!(
            store,
            vec![
                bound("a", ParamValue::Number("1".to_string())),
                bound("b", ParamValue::Null),
            ]
        );
    }

    /// A placeholder that momentarily leaves the query — a retyped `WHERE`, an
    /// undo — must not take the typed value with it.
    #[test]
    fn a_value_survives_the_placeholder_leaving_and_returning() {
        let mut store: Vec<Binding> = Vec::new();
        set_value(&mut store, "id", Some(ParamValue::Number("7".to_string())));
        // The query loses the placeholder: the row goes, the store doesn't.
        assert!(bindings_for("SELECT 1", MY, &store).is_empty());
        assert_eq!(bindings_for("SELECT :id", MY, &store), store);
    }

    // ── strip_param_diagnostics ─────────────────────────────────────────────

    fn diag(range: (usize, usize)) -> crate::intel::Diagnostic {
        crate::intel::Diagnostic {
            range,
            severity: crate::intel::Severity::Error,
            message: "unknown column".to_string(),
        }
    }

    #[test]
    fn a_diagnostic_over_a_placeholder_is_dropped() {
        let sql = "SELECT * FROM t WHERE id = :id";
        let refs = scan(sql, MY);
        let kept = strip_param_diagnostics(vec![diag((27, 30))], &refs);
        assert!(kept.is_empty(), "the `_id` the analysis saw is ours");
    }

    #[test]
    fn a_diagnostic_elsewhere_in_the_statement_survives() {
        let sql = "SELECT nope FROM t WHERE id = :id";
        let refs = scan(sql, MY);
        let kept = strip_param_diagnostics(vec![diag((7, 11))], &refs);
        assert_eq!(
            kept.len(),
            1,
            "an unknown column is still an unknown column"
        );
    }

    /// A statement-wide diagnostic (a syntax error spanning the whole thing)
    /// overlaps the placeholder, so it goes with it — the alternative is a
    /// permanent error on a statement the user has not finished parameterising.
    #[test]
    fn a_diagnostic_overlapping_a_placeholder_is_dropped() {
        let sql = "SELECT * FROM t WHERE id = :id";
        let refs = scan(sql, MY);
        assert!(strip_param_diagnostics(vec![diag((0, sql.len()))], &refs).is_empty());
    }

    #[test]
    fn without_placeholders_every_diagnostic_survives() {
        let kept = strip_param_diagnostics(vec![diag((0, 5)), diag((7, 9))], &[]);
        assert_eq!(kept.len(), 2);
    }

    // ── prepare_run ─────────────────────────────────────────────────────────

    fn policy(read_only: bool) -> crate::sql::GuardPolicy {
        crate::sql::GuardPolicy {
            read_only,
            confirm_writes: false,
            dialect: MY,
            no_database: false,
        }
    }

    /// The reason this function exists. The guard has to judge what the engine
    /// will receive, not the template: a `Raw` value expands to arbitrary SQL,
    /// so a plain `SELECT` template can arrive at the engine carrying a `DELETE`
    /// the guard was never shown.
    ///
    /// A `Raw` value is the user's own text, so this is not an injection guard —
    /// it is the read-only connection meaning what it says.
    #[test]
    fn a_raw_value_that_turns_a_read_into_a_write_is_blocked() {
        let stmts = vec!["SELECT * FROM t WHERE x = :p".to_string()];
        let bindings = vec![bound("p", ParamValue::Raw("1; DELETE FROM t".to_string()))];

        // The guard on its own, asked about the template, allows it.
        assert_eq!(
            crate::sql::run_verdict(&stmts, policy(true)),
            crate::sql::RunVerdict::Allow,
            "the template really does look harmless — that is the trap"
        );

        let (prepared, verdict) = prepare_run(&stmts, &bindings, policy(true)).expect("bound");
        assert_eq!(
            prepared,
            vec!["SELECT * FROM t WHERE x = 1; DELETE FROM t".to_string()]
        );
        assert!(
            matches!(verdict, crate::sql::RunVerdict::Block(_)),
            "the guard must see the substituted statement, got {verdict:?}"
        );
    }

    #[test]
    fn the_statements_handed_back_are_the_substituted_ones() {
        let stmts = vec![
            "SELECT * FROM t WHERE id = :id".to_string(),
            "SELECT :id".to_string(),
        ];
        let bindings = vec![bound("id", ParamValue::Number("7".to_string()))];
        let (prepared, verdict) = prepare_run(&stmts, &bindings, policy(false)).expect("bound");
        assert_eq!(
            prepared,
            vec![
                "SELECT * FROM t WHERE id = 7".to_string(),
                "SELECT 7".to_string(),
            ]
        );
        assert_eq!(verdict, crate::sql::RunVerdict::Allow);
    }

    #[test]
    fn an_unfilled_parameter_stops_the_run_before_the_guard() {
        let stmts = vec!["DELETE FROM t WHERE id = :id".to_string()];
        let out = prepare_run(&stmts, &[], policy(true));
        assert_eq!(
            out,
            Err(ParamError::Missing(vec!["id".to_string()])),
            "the parameter is reported, not the read-only block behind it"
        );
    }

    #[test]
    fn a_run_without_parameters_is_the_guards_own_answer() {
        let stmts = vec!["DELETE FROM t".to_string()];
        let (prepared, verdict) = prepare_run(&stmts, &[], policy(false)).expect("no parameters");
        assert_eq!(prepared, stmts);
        assert!(
            matches!(verdict, crate::sql::RunVerdict::Confirm(_)),
            "an unguarded DELETE with no WHERE still confirms, got {verdict:?}"
        );
    }

    // ── neutralize ──────────────────────────────────────────────────────────

    #[test]
    fn neutralize_keeps_byte_offsets_and_yields_an_identifier() {
        let sql = "SELECT * FROM t WHERE id = :id AND x = :other";
        let out = neutralize(sql, MY);
        assert_eq!(out, "SELECT * FROM t WHERE id = _id AND x = _other");
        assert_eq!(out.len(), sql.len(), "offsets must survive for diagnostics");
    }

    #[test]
    fn neutralize_leaves_a_lookalike_in_a_string_alone() {
        let sql = "SELECT ':id' FROM t";
        assert_eq!(neutralize(sql, MY), sql);
    }

    #[test]
    fn neutralize_is_a_no_op_without_placeholders() {
        let sql = "SELECT id::text FROM t";
        assert_eq!(neutralize(sql, PG), sql);
    }
}
