//! Pure SQL pretty-printer: re-flow whitespace, indentation, and line breaks
//! **without changing any token's text** — except keyword case when the
//! user's setting asks [`apply_keyword_case`] for upper or lower case; the
//! default keeps it exactly as typed. Built on the same boundary lexer as the rest of [`crate::sql`]
//! (`skip_noncode`), so string literals, `--` / `#` line comments, `/* … */`
//! block comments, and backtick identifiers all tokenize on the same boundaries
//! and are emitted verbatim (a `;` or keyword hidden inside one never affects
//! layout).
//!
//! The style is the common "block" layout (as produced by tools like
//! `sqlformat`): each clause keyword (SELECT / FROM / WHERE / …) on its own line,
//! its body indented one level, list items broken on top-level commas, and
//! `AND` / `OR` on their own lines. Subqueries indent one level per paren; commas
//! and clause keywords *inside* a function/expression paren stay inline. This is
//! a deliberate first pass — solid and predictable, meant to be tuned later.

use crate::intel::SqlDialect;
use crate::sql::skip_noncode;

/// The byte range Format Code may re-flow, for a caret at `start..end` in
/// `full` — or `None` when there is nothing safe to do.
///
/// **[`format_sql`] lexes what it is handed from byte 0 and takes that for a
/// real token boundary**, which is true of a whole document and of a selection
/// that begins and ends in code, and false of one that begins inside a string,
/// a quoted identifier or a comment. With the document
/// `select 1 -- keep a, b\nfrom t` and `keep a, b` selected, the fragment lexed
/// as the words `keep`, `a`, a comma and `b`; the comma broke, and the document
/// became `select 1 -- keep a,\nb\nfrom t` — `b` now **outside** the comment, a
/// stray token, and the statement no longer parses.
///
/// An empty selection is the whole document, which is what Format Code has
/// always done with a bare caret. A selection that is not formattable answers
/// `None`: falling back to the whole document would silently reformat every
/// line the user did not select, which is the worse of the two surprises.
///
/// Here rather than at the call site so the decision is testable — the
/// `format_sql` caller is a floem view — and beside the lexer whose assumption
/// it is protecting.
pub fn formattable_range(
    full: &str,
    start: usize,
    end: usize,
    dialect: SqlDialect,
) -> Option<(usize, usize)> {
    let (lo, hi) = (
        start.min(end).min(full.len()),
        start.max(end).min(full.len()),
    );
    if lo == hi {
        return Some((0, full.len()));
    }
    let in_code =
        |at: usize| crate::pairs::region_at(full, at, dialect) == crate::pairs::Region::Code;
    (in_code(lo) && in_code(hi)).then_some((lo, hi))
}

/// Format `sql`, indenting each level with `indent_unit` (e.g. `"    "` or
/// `"\t"`). Token text is preserved verbatim; only whitespace/layout changes.
/// `dialect` selects the boundary rules (comments/quotes/dollar-quotes) so
/// PostgreSQL `#`-operators and `$tag$` bodies aren't mistaken for comments.
///
/// **A trailing newline is layout too, and is kept if the input had one.** Since
/// Format Code reaches a `.sql` file on disk, trimming it turned a reformat into
/// a reformat *plus* `\ No newline at end of file` in the diff — a change to a
/// line the user did not touch, on the one artefact Schemaic cannot regenerate.
/// One newline either way: the trailing blank lines a document collects are
/// still whitespace to re-flow.
pub fn format_sql(sql: &str, indent_unit: &str, dialect: SqlDialect) -> String {
    format_sql_under(sql, indent_unit, dialect, None, CommaStyle::Trailing)
}

/// [`format_sql`] of a fragment that begins where `delim` is the statement
/// terminator a MySQL `DELIMITER` set (`None` for `;`) — see
/// [`terminator_at`].
fn format_sql_under(
    sql: &str,
    indent_unit: &str,
    dialect: SqlDialect,
    mut delim: Option<String>,
    commas: CommaStyle,
) -> String {
    let toks = tokenize(sql, dialect, &mut delim);
    let mut f = Fmt::new(indent_unit, sql, dialect);
    f.leading_commas = commas == CommaStyle::Leading;
    f.run(&toks);
    let out = f.out.trim_end();
    if sql.ends_with('\n') {
        format!("{out}\n")
    } else {
        out.to_string()
    }
}

/// How Format Code cases keywords — Settings → Editor. `Keep`, the default,
/// is the formatter as it always was: no token's text changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum KeywordCase {
    #[default]
    Keep,
    Upper,
    Lower,
}

impl KeywordCase {
    /// Every choice, in the order Settings lists them.
    pub const ALL: [KeywordCase; 3] = [KeywordCase::Keep, KeywordCase::Upper, KeywordCase::Lower];

    /// The setting's label.
    pub fn label(self) -> &'static str {
        match self {
            KeywordCase::Keep => "As typed",
            KeywordCase::Upper => "Upper case",
            KeywordCase::Lower => "Lower case",
        }
    }
}

/// Read leniently: a word this version does not know is `Keep`, so a setting
/// a later version wrote never fails the settings file it sits in.
impl<'de> serde::Deserialize<'de> for KeywordCase {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let word = String::deserialize(d)?;
        Ok(match word.as_str() {
            "upper" => KeywordCase::Upper,
            "lower" => KeywordCase::Lower,
            _ => KeywordCase::Keep,
        })
    }
}

/// Where Format Code puts the comma of a list it breaks across lines —
/// Settings → Editor. `Trailing`, the default, is the formatter as it always
/// was; `Leading` opens each continued line with it (`, b`), which keeps a
/// line's removal a one-line diff. Only the commas the layout breaks at move:
/// an `IN (1, 2)` or a function's arguments, kept on one line, are untouched.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CommaStyle {
    #[default]
    Trailing,
    Leading,
}

impl CommaStyle {
    /// Every choice, in the order Settings lists them.
    pub const ALL: [CommaStyle; 2] = [CommaStyle::Trailing, CommaStyle::Leading];

    /// The setting's label.
    pub fn label(self) -> &'static str {
        match self {
            CommaStyle::Trailing => "End of line",
            CommaStyle::Leading => "Start of line",
        }
    }
}

/// Read leniently, as [`KeywordCase`] is.
impl<'de> serde::Deserialize<'de> for CommaStyle {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let word = String::deserialize(d)?;
        Ok(match word.as_str() {
            "leading" => CommaStyle::Leading,
            _ => CommaStyle::Trailing,
        })
    }
}

/// The user's Format Code settings, carried together. A bare [`KeywordCase`]
/// converts into one with the default commas, so a caller — or a test — that
/// asks only about case says nothing about commas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FormatStyle {
    pub case: KeywordCase,
    pub commas: CommaStyle,
}

impl From<KeywordCase> for FormatStyle {
    fn from(case: KeywordCase) -> Self {
        FormatStyle {
            case,
            ..FormatStyle::default()
        }
    }
}

/// `sql` with its keywords in `case` — applied over Format Code's output.
///
/// **A keyword is a word every server of the dialect reserves**
/// ([`crate::intel::is_reserved_word_on`] with the flavour not known), which
/// can never be an unquoted name, **or one this formatter lays out as a
/// keyword** ([`is_keyword`]: clause heads, joins, `BY`, `AND`…), which it
/// already reads as one wherever it stands — PostgreSQL reserves neither `BY`
/// nor `INSERT`, and `ORDER by` is what the reserved list alone wrote. No
/// wider: a general keyword list would recase names, and an unquoted table
/// called `status` is case-sensitive on a MySQL server on Linux. For the same
/// reason a word beside a `.` is left alone: MySQL takes a reserved word as a
/// qualified name (`shop.order`). Strings, comments and quoted names are
/// tokens of their own and never words. ASCII case only, so every byte stays
/// where it was and a caret mapped over the formatted text still lands.
///
/// **A layout keyword the dialect does not reserve may be a name all the
/// same** — `top`, `full`, `include`, `offset`, `end` on MySQL, `partition`
/// and `window` on SQL Server — and where a name's case can matter
/// ([`names_may_be_case_sensitive`]) it is recased only where it stands as the
/// keyword ([`stands_as_name`]): `select * from top` written `FROM TOP` was
/// *Table 'db.TOP' doesn't exist* on MariaDB 10.11.
pub fn apply_keyword_case(sql: &str, dialect: SqlDialect, case: KeywordCase) -> String {
    apply_keyword_case_under(sql, dialect, case, None)
}

/// [`apply_keyword_case`] of a fragment that begins where `delim` is the
/// statement terminator in force, as [`format_sql_under`] takes it.
fn apply_keyword_case_under(
    sql: &str,
    dialect: SqlDialect,
    case: KeywordCase,
    mut delim: Option<String>,
) -> String {
    if case == KeywordCase::Keep {
        return sql.to_string();
    }
    let toks = tokenize(sql, dialect, &mut delim);
    let base = sql.as_ptr() as usize;
    let mut out = sql.to_string();
    let dot = |t: Option<&(Kind, &str)>| matches!(t, Some((Kind::Punct, ".")));
    let guard_names = names_may_be_case_sensitive(dialect);
    for (i, (kind, text)) in toks.iter().enumerate() {
        if *kind != Kind::Word
            || dot(i.checked_sub(1).and_then(|p| toks.get(p)))
            || dot(toks.get(i + 1))
        {
            continue;
        }
        let reserved =
            crate::intel::is_reserved_word_on(text, dialect, crate::schema::ServerFlavour::Unknown);
        if !(reserved || is_keyword(text)) || (!reserved && guard_names && stands_as_name(&toks, i))
        {
            continue;
        }
        let at = text.as_ptr() as usize - base;
        let recased = match case {
            KeywordCase::Upper => text.to_ascii_uppercase(),
            KeywordCase::Lower => text.to_ascii_lowercase(),
            KeywordCase::Keep => continue,
        };
        out.replace_range(at..at + text.len(), &recased);
    }
    out
}

/// May an unquoted name's **case** tell two objects apart in `dialect`?
///
/// MySQL's table and database names are case-sensitive on a server on Linux
/// (`lower_case_table_names = 0`), and SQL Server's names follow the
/// database's collation, case-sensitive under a `_CS_` one. PostgreSQL folds
/// an unquoted name to lower case and SQLite matches names without regard to
/// case, so recasing one there changes nothing it names. An exhaustive
/// `match`, for the reason [`ops`] gives.
fn names_may_be_case_sensitive(dialect: SqlDialect) -> bool {
    match dialect {
        SqlDialect::MySql | SqlDialect::MsSql => true,
        SqlDialect::Postgres | SqlDialect::Sqlite => false,
    }
}

/// Does the layout keyword at `toks[i]` — one the dialect does not reserve —
/// stand where a name would rather than where its keyword does?
///
/// **Conservative in the name's favour**: a word left in its case is a
/// cosmetic miss, a name recased is a different table. So it is a name
/// straight after a word that introduces one (`FROM top`, `AS offset`,
/// `UPDATE top`), and otherwise wherever its own keyword's shape is missing:
/// `FULL`/`NATURAL` ahead of a join word, `ANY`/`SOME`/`INCLUDE` ahead of
/// `(`, `TOP` ahead of a count, `PARTITION` ahead of `BY` or `(`, `END`
/// closing an open `CASE` or `BEGIN` — and for the rest, a name is what is
/// followed by a list's comma, a close, a comparison, the end, or a clause
/// word (`select offset from t`).
fn stands_as_name(toks: &[(Kind, &str)], i: usize) -> bool {
    let code = |k: &usize| !matches!(toks[*k].0, Kind::LineComment | Kind::BlockComment);
    let prev = (0..i).rev().find(code).map(|k| toks[k]);
    let next = (i + 1..toks.len()).find(code).map(|k| toks[k]);
    let word_in = |t: Option<(Kind, &str)>, set: &[&str]| matches!(t, Some((Kind::Word, w)) if set.iter().any(|s| w.eq_ignore_ascii_case(s)));
    let paren_next = matches!(next, Some((Kind::Punct, "(")));
    if word_in(prev, &["FROM", "JOIN", "INTO", "UPDATE", "TABLE", "AS"]) {
        return true;
    }
    let text = toks[i].1;
    match text.to_ascii_uppercase().as_str() {
        "FULL" | "NATURAL" => !word_in(next, &["JOIN", "OUTER", "LEFT", "RIGHT", "INNER"]),
        "ANY" | "SOME" | "INCLUDE" => !paren_next,
        "TOP" => {
            !(paren_next
                || matches!(next, Some((Kind::Word, w)) if w.starts_with(|c: char| c.is_ascii_digit())))
        }
        "PARTITION" => !(paren_next || word_in(next, &["BY"])),
        "END" => !closes_an_open_block(&toks[..i]),
        _ => match next {
            None | Some((Kind::Terminator | Kind::Directive, _)) => true,
            Some((Kind::Punct, p)) => matches!(
                p,
                "," | ")" | ";" | "=" | "<" | ">" | "<=" | ">=" | "<>" | "!=" | "<=>"
            ),
            Some((Kind::Word, _)) => word_in(
                next,
                &[
                    "FROM", "WHERE", "AS", "AND", "OR", "IS", "IN", "LIKE", "BETWEEN", "ON",
                    "THEN", "ELSE", "END", "WHEN", "ORDER", "GROUP", "HAVING", "UNION", "JOIN",
                    "SET",
                ],
            ),
            Some(_) => false,
        },
    }
}

/// Is a `CASE` or a body's `BEGIN` open at the end of `toks`, for an `END` to
/// close? An `END IF`/`LOOP`/`WHILE`/`REPEAT`/`FOR` closes neither, and a
/// transaction's `BEGIN` opens nothing.
fn closes_an_open_block(toks: &[(Kind, &str)]) -> bool {
    let mut open = 0usize;
    for (k, &(kind, w)) in toks.iter().enumerate() {
        if kind != Kind::Word {
            continue;
        }
        let up = w.to_ascii_uppercase();
        match up.as_str() {
            "CASE" if !(k > 0 && word_is(toks, k - 1, "END")) => open += 1,
            "BEGIN" if toks.len() > k + 1 && Fmt::opens_body(toks, k) => open += 1,
            "END" => {
                let names = ["IF", "LOOP", "WHILE", "REPEAT", "FOR"]
                    .iter()
                    .any(|n| word_is(toks, k + 1, n));
                if !names {
                    open = open.saturating_sub(1);
                }
            }
            _ => {}
        }
    }
    open > 0
}

/// The text Format Code writes over `full[lo..hi]` — [`format_sql`]'s, with
/// the range's own leading and trailing whitespace put back unless the range
/// is the whole document.
///
/// **That whitespace belongs to the document around the range.** `format_sql`
/// trims both ends, which is right for a whole buffer and wrong for a
/// selection: selecting `select a from ` out of `select a from t` — a
/// Ctrl+Shift+→ selection ends after the space — wrote back a fragment ending
/// in `from`, and the splice fused it with `t` into `fromt`.
///
/// **A fragment is formatted under the terminator in force where it
/// begins** ([`terminator_at`]). The `DELIMITER //` line that makes `//` one
/// is above the statement, not in it, so *Format statement* lexed the
/// routine's `END//` as two `/`s and wrote `END / /` — no terminator at all,
/// and the client ran the routine on through `DELIMITER ;` and whatever
/// followed.
pub fn format_range(
    full: &str,
    lo: usize,
    hi: usize,
    indent_unit: &str,
    dialect: SqlDialect,
    style: impl Into<FormatStyle>,
) -> String {
    let FormatStyle { case, commas } = style.into();
    if lo == 0 && hi == full.len() {
        let formatted = format_sql_under(full, indent_unit, dialect, None, commas);
        return apply_keyword_case(&formatted, dialect, case);
    }
    let frag = &full[lo..hi];
    let body = frag.trim();
    if body.is_empty() {
        return frag.to_string();
    }
    let lead = &frag[..frag.len() - frag.trim_start().len()];
    let trail = &frag[frag.trim_end().len()..];
    let delim = terminator_at(full, lo, dialect);
    let formatted = format_sql_under(body, indent_unit, dialect, delim.clone(), commas);
    format!(
        "{lead}{}{trail}",
        apply_keyword_case_under(&formatted, dialect, case, delim)
    )
}

/// The statement terminator a MySQL `DELIMITER` line set above `lo` in
/// `full`, or `None` where it is `;` — the lexer's own state at `lo`, so a
/// directive is taken exactly where [`tokenize`] takes one.
fn terminator_at(full: &str, lo: usize, dialect: SqlDialect) -> Option<String> {
    let mut delim = None;
    if let Some(head) = full.get(..lo) {
        tokenize(head, dialect, &mut delim);
    }
    delim
}

/// `sql` on one line, for the editor's *Copy as single line*: every run of
/// whitespace in code becomes one space, a block comment's runs too, and a
/// `--`/`#` comment becomes `/* … */` — it would otherwise swallow everything
/// after it. Strings, quoted names and dollar-quoted bodies are copied as
/// they are, line breaks and all: those are data, and a statement that
/// changed them would not be the one it was copied from.
///
/// `None` when the text cannot be one line and mean the same: a `GO` or
/// `DELIMITER` line, which a client only reads standing on its own line, or a
/// line comment whose text would not lex back as one block comment (a `*/` in
/// it, or a `/*` where comments nest) — or, where a line break joins two
/// strings into one ([`newline_continues_strings`]), a string continued on
/// the next line: PostgreSQL reads `'foo'⏎'bar'` as `'foobar'`, and
/// `'foo' 'bar'` is a syntax error there.
pub fn single_line(sql: &str, dialect: SqlDialect) -> Option<String> {
    let b = sql.as_bytes();
    let collapse = |s: &str| s.split_ascii_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = String::with_capacity(sql.len());
    let mut space = false;
    let mut line_start = true;
    // The last piece was a string, and a line break has passed since —
    // through blanks and line comments only, which is what PostgreSQL's
    // lexer lets stand between a string and its continuation.
    let mut after_string = false;
    let mut newline_since = false;
    // A MySQL executable comment (`/*!…*/`, `/*M!…*/`) is code to the lexer,
    // so a line comment inside one is lexed as one — and rewritten, its `*/`
    // would close the executable comment. Where one has opened, a line comment
    // refuses the copy (conservatively: even after it has closed).
    let mut executable = false;
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_whitespace() {
            line_start |= b[i] == b'\n';
            newline_since |= b[i] == b'\n';
            space = !out.is_empty();
            i += 1;
            continue;
        }
        if std::mem::take(&mut line_start)
            && (crate::sql::delimiter_directive(sql, i, dialect).is_some()
                || crate::sql::go_directive(sql, i, dialect).is_some())
        {
            return None;
        }
        let (piece, next, line_comment) = match skip_noncode(b, i, dialect) {
            Some(j) => {
                let j = j.max(i + 1);
                let span = &sql[i..j];
                // The lexer hands back an executable comment's marker alone —
                // on MySQL; elsewhere `/*!` opens a whole comment, `*/` and all.
                executable |=
                    (span.starts_with("/*!") || span.starts_with("/*M!")) && !span.ends_with("*/");
                let line_body = span.strip_prefix("--").or_else(|| span.strip_prefix('#'));
                let piece = match line_body {
                    Some(body) => {
                        // A line comment ends at a line break.
                        newline_since = true;
                        let body = collapse(body);
                        if body.is_empty() {
                            i = j;
                            continue;
                        }
                        if executable {
                            return None;
                        }
                        let block = format!("/* {body} */");
                        if !crate::sql::is_one_comment(&block, dialect) {
                            return None;
                        }
                        block
                    }
                    None if span.starts_with("/*") => collapse(span),
                    None => span.to_string(),
                };
                (piece, j, line_body.is_some())
            }
            None => {
                let ch = sql[i..].chars().next()?;
                (ch.to_string(), i + ch.len_utf8(), false)
            }
        };
        if !line_comment {
            let string = piece.len() > 1 && piece.starts_with('\'') && piece.ends_with('\'');
            if string
                && std::mem::take(&mut after_string)
                && newline_since
                && newline_continues_strings(dialect)
            {
                return None;
            }
            after_string = string;
            newline_since = false;
        }
        if std::mem::take(&mut space) {
            out.push(' ');
        }
        out.push_str(&piece);
        i = next;
    }
    Some(out)
}

/// What *Copy as single line* copies, [`single_line`] already applied: the
/// selection `sel` when there is one — refused, like Format Code's, when an
/// end is not in code ([`formattable_range`]), since the fragment would be
/// lexed from a boundary that isn't one — or else the statement at `offset`,
/// the right-click's. `None` leaves the menu entry out.
pub fn single_line_copy(
    full: &str,
    sel: (usize, usize),
    offset: usize,
    dialect: SqlDialect,
) -> Option<String> {
    single_line_target(full, sel, offset, dialect)
        .and_then(|(lo, hi)| single_line(&full[lo..hi], dialect))
}

/// The text *Copy as single line* and *Make single line* take: the selection
/// `sel` when there is one, refused when an end is not in code, else the
/// statement at `offset` — trimmed of its outer whitespace either way, which
/// belongs to the lines around it. `None` when that leaves nothing.
fn single_line_target(
    full: &str,
    sel: (usize, usize),
    offset: usize,
    dialect: SqlDialect,
) -> Option<(usize, usize)> {
    let (lo, hi) = if sel.0 != sel.1 {
        formattable_range(full, sel.0, sel.1, dialect)?
    } else {
        crate::sql::statement_range(full, offset, dialect)
    };
    let text = full.get(lo..hi)?;
    let body = text.trim();
    if body.is_empty() {
        return None;
    }
    let lo = lo + (text.len() - text.trim_start().len());
    Some((lo, lo + body.len()))
}

/// What *Make single line* writes: [`single_line_copy`]'s text, and the range
/// `(lo, hi)` of `full` it replaces. `None` when there is nothing to take, when
/// [`single_line`] refuses it, or when it is one line already — the edit would
/// change nothing, so the menu leaves the entry out.
pub fn single_line_in_place(
    full: &str,
    sel: (usize, usize),
    offset: usize,
    dialect: SqlDialect,
) -> Option<(usize, usize, String)> {
    let (lo, hi) = single_line_target(full, sel, offset, dialect)?;
    let text = single_line(&full[lo..hi], dialect)?;
    (text != full[lo..hi]).then_some((lo, hi, text))
}

/// What *Format statement* writes: the statement at `offset`, formatted with
/// `indent_unit`, and the range `(lo, hi)` of `full` it replaces. `None` when
/// there is no statement there or it is formatted already.
pub fn format_statement(
    full: &str,
    offset: usize,
    indent_unit: &str,
    dialect: SqlDialect,
    style: impl Into<FormatStyle>,
) -> Option<(usize, usize, String)> {
    let (lo, hi) = crate::sql::statement_range(full, offset, dialect);
    if full.get(lo..hi)?.trim().is_empty() {
        return None;
    }
    let text = format_range(full, lo, hi, indent_unit, dialect, style);
    (text != full[lo..hi]).then_some((lo, hi, text))
}

/// Where a caret at `caret` in `old` belongs once `old[lo..hi]` is replaced by
/// `new`, a re-flow of it: unmoved before the range, shifted by the change in
/// length after it, and inside it after as many non-whitespace bytes of the
/// range as preceded it ([`caret_after_format`]).
///
/// Inside a [`single_line`] rewrite that count is approximate — a `--` comment
/// gains the `/*` and `*/` it becomes — so a caret past such a comment lands a
/// few bytes early, and never outside the new text.
pub fn caret_after_replace(old: &str, lo: usize, hi: usize, caret: usize, new: &str) -> usize {
    if caret <= lo {
        caret
    } else if caret >= hi {
        caret - (hi - lo) + new.len()
    } else {
        lo + caret_after_format(&old[lo..hi], caret - lo, new)
    }
}

/// Where a caret at `old_caret` in `old` belongs in `new`, the text
/// [`format_sql`] made of it: after as many non-whitespace bytes as preceded
/// it.
///
/// Formatting keeps every token's text and moves only the whitespace between
/// tokens, so that count finds the same character on the other side. A caret
/// in whitespace lands straight after the token before it.
pub fn caret_after_format(old: &str, old_caret: usize, new: &str) -> usize {
    let want = old.as_bytes()[..old_caret.min(old.len())]
        .iter()
        .filter(|c| !c.is_ascii_whitespace())
        .count();
    if want == 0 {
        return 0;
    }
    let mut seen = 0;
    for (i, c) in new.bytes().enumerate() {
        if !c.is_ascii_whitespace() {
            seen += 1;
            if seen == want {
                return i + 1;
            }
        }
    }
    new.len()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Word,
    Quoted,
    LineComment,
    BlockComment,
    Punct,
    /// A client directive that owns its whole line — SQL Server's `GO`,
    /// MySQL's `DELIMITER`.
    Directive,
    /// The statement terminator a MySQL `DELIMITER` set, in place of `;`.
    Terminator,
}

use crate::sql::is_word_byte;

/// Multi-character operators of `dialect`, longest first, so `->>` beats `->`
/// beats `>`.
///
/// **Per dialect, and it has to be.** Anything not matched here becomes one
/// `Punct` token *per byte*, and `need_space` then puts a space between the two
/// halves — so an operator missing from the table is not laid out badly, it is
/// **split**, and the buffer Format Code writes back holds SQL the server will
/// not parse. The list used to be one MySQL-shaped table, which meant
/// `select id::text from t` came out `select id : : text` and PostgreSQL
/// 16.15 answered *syntax error at or near ":"* — on a rewrite of the user's
/// own document, and after a Ctrl+S of the `.sql` file behind it. `::` is not an
/// exotic operator; it is how PostgreSQL spells every cast.
///
/// The same rule as [`crate::sql::skip_noncode`] and
/// [`crate::intel::ident_quote`]: a lexical table is the one thing that really
/// is per dialect, and an exhaustive `match` so a fourth arm has to answer.
///
/// **PostgreSQL's table is short on purpose** — see
/// [`operators_are_composable`]. `::` and `:=` are there because `:` is *not*
/// one of PostgreSQL's operator characters, so the composition rule cannot
/// reach them.
fn ops(dialect: SqlDialect) -> &'static [&'static str] {
    match dialect {
        SqlDialect::MySql => &[
            "->>", "<=>", "->", ">=", "<=", "<>", "!=", ":=", "||", "&&", "<<", ">>",
        ],
        SqlDialect::Postgres => &["::", ":="],
        // No user-defined operators and a fixed set, so a table is complete.
        SqlDialect::Sqlite => &["->>", "->", ">=", "<=", "<>", "!=", "==", "||", "<<", ">>"],
        // Fixed too: comparisons (including the old `!<`/`!>`), compound
        // assignment, `::` before a CLR type's static method, and `||`
        // concatenation (SQL Server 2025).
        SqlDialect::MsSql => &[
            ">=", "<=", "<>", "!=", "!<", "!>", "+=", "-=", "*=", "/=", "%=", "&=", "^=", "|=",
            "::", "||",
        ],
    }
}

/// Does `dialect` let an operator be **composed** out of its operator
/// characters, so that no table of spellings can ever be complete?
///
/// PostgreSQL does: `CREATE OPERATOR` builds a name out of
/// `+ - * / < > = ~ ! @ # % ^ & | ` + "`" + ` ?`, and the built-in set already
/// spans `@>`, `<@`, `#>>`, `?|`, `!~*`, `||/` and `-|-`. Enumerating them is a
/// list that is wrong the moment an extension is installed, so the run is
/// consumed by the engine's own rule instead. MySQL and SQLite have fixed
/// operator sets and no `CREATE OPERATOR`, so their tables above are complete.
///
/// An exhaustive `match`, for the reason [`ops`] gives.
fn operators_are_composable(dialect: SqlDialect) -> bool {
    match dialect {
        SqlDialect::Postgres => true,
        SqlDialect::MySql | SqlDialect::Sqlite | SqlDialect::MsSql => false,
    }
}

/// Does a line break between two string literals make them **one** string
/// in `dialect`, which the same two on one line are not?
///
/// PostgreSQL's (the SQL standard's rule): `'foo'⏎'bar'` is `'foobar'`, and
/// `'foo' 'bar'` is a syntax error. MySQL joins adjacent strings on one line
/// as well, so a line break changes nothing there; SQLite and SQL Server
/// join them nowhere. An exhaustive `match`, for the reason [`ops`] gives.
pub(crate) fn newline_continues_strings(dialect: SqlDialect) -> bool {
    match dialect {
        SqlDialect::Postgres => true,
        SqlDialect::MySql | SqlDialect::Sqlite | SqlDialect::MsSql => false,
    }
}

/// Is `c` one of the characters PostgreSQL builds an operator name from?
fn is_operator_byte(c: u8) -> bool {
    matches!(
        c,
        b'+' | b'-'
            | b'*'
            | b'/'
            | b'<'
            | b'>'
            | b'='
            | b'~'
            | b'!'
            | b'@'
            | b'#'
            | b'%'
            | b'^'
            | b'&'
            | b'|'
            | b'`'
            | b'?'
    )
}

/// How many bytes at `i` are **one** composed operator, or `None` when there is
/// no run of two or more there.
///
/// PostgreSQL's own two rules, and nothing beyond them:
///
/// - The run stops where a comment or a quote begins, which is
///   [`skip_noncode`]'s question — so `a<--b` is `a`, `<`, and then the comment
///   `--b`, exactly as the server reads it.
/// - **A name may not end in `+` or `-` unless it also holds one of
///   `~ ! @ # % ^ & | ` + "`" + ` ?`.** Without the back-off `x=-1` would lex as
///   `x`, `=-`, `1`, and the formatter would emit `x =- 1` — a token the server
///   does not have.
fn composed_operator_len(b: &[u8], i: usize, dialect: SqlDialect) -> Option<usize> {
    let mut end = i;
    while end < b.len() && is_operator_byte(b[end]) {
        // A comment or quote inside the run ends it, asked of the one boundary
        // lexer rather than by re-spelling `--` and `/*` here.
        if end > i && skip_noncode(b, end, dialect).is_some() {
            break;
        }
        end += 1;
    }
    let holds_special = |s: &[u8]| {
        s.iter().any(|c| {
            matches!(
                c,
                b'~' | b'!' | b'@' | b'#' | b'%' | b'^' | b'&' | b'|' | b'`' | b'?'
            )
        })
    };
    while end - i > 1 && matches!(b[end - 1], b'+' | b'-') && !holds_special(&b[i..end]) {
        end -= 1;
    }
    (end - i > 1).then_some(end - i)
}

/// How many bytes at `b[i]` are a **sigil** — a prefix that belongs to the
/// name, number or quoted name after it, so the two are one token and
/// `need_space` never separates them — or `0`.
///
/// Every engine's own parameter and variable spellings, measured as the space
/// breaking them: T-SQL's `@x`/`@@x`/`#t`/`##t` ([`crate::sql::t_sql_name_prefix`]);
/// a `$` before a word byte on all four — PostgreSQL's `$1`, SQLite's `$p`,
/// MySQL's `$p` (a name there), T-SQL's money `$1.50` and `MERGE`'s
/// `$action` — a dollar *quote* having been taken whole by `skip_noncode`
/// before this is asked; SQLite's numbered `?1` and its `@p`; and MySQL's
/// user variable `@x`, its system `@@x`, and `@` before a quoted name —
/// `@'q v'`, and the host of an account, `'u'@'h'`, whose spaced form
/// MySQL 8 and MariaDB 10.11 both refuse. PostgreSQL's `@` is an operator
/// (`@ -5`) and is left to the operator arm. `:name` is the app's own
/// placeholder, [`crate::params::opens_placeholder`]'s.
///
/// An exhaustive `match`, for the reason [`ops`] gives.
fn sigil_len(b: &[u8], i: usize, dialect: SqlDialect) -> usize {
    let tsql = crate::sql::t_sql_name_prefix(b, i, dialect);
    if tsql > 0 {
        return tsql;
    }
    let word_at = |k: usize| b.get(k).is_some_and(|&c| is_word_byte(c));
    match b.get(i) {
        Some(b'$') if word_at(i + 1) => 1,
        Some(b'?') if b.get(i + 1).is_some_and(u8::is_ascii_digit) => match dialect {
            SqlDialect::Sqlite => 1,
            SqlDialect::MySql | SqlDialect::Postgres | SqlDialect::MsSql => 0,
        },
        Some(b'@') => match dialect {
            SqlDialect::MySql => {
                let len = if b.get(i + 1) == Some(&b'@') { 2 } else { 1 };
                let quoted = len == 1 && matches!(b.get(i + 1), Some(b'\'' | b'`'));
                if word_at(i + len) || quoted { len } else { 0 }
            }
            SqlDialect::Sqlite => usize::from(word_at(i + 1)),
            SqlDialect::Postgres | SqlDialect::MsSql => 0,
        },
        _ => 0,
    }
}

/// Where the name that ends just before `toks[k]` begins — back over `a.b.c`,
/// quoted or not — or `None` when no name ends there.
fn name_start(toks: &[(Kind, &str)], k: usize) -> Option<usize> {
    let mut i = k;
    loop {
        match i.checked_sub(1).map(|j| toks[j].0) {
            Some(Kind::Word | Kind::Quoted) => i -= 1,
            _ => return None,
        }
        match i.checked_sub(1).map(|j| toks[j]) {
            Some((Kind::Punct, ".")) => i -= 1,
            _ => return Some(i),
        }
    }
}

/// Is `toks[j]` the word `w`, in any case?
fn word_is(toks: &[(Kind, &str)], j: usize, w: &str) -> bool {
    matches!(toks.get(j), Some((Kind::Word, t)) if t.eq_ignore_ascii_case(w))
}

/// Does the next token begin a statement — is the last one before it, comments
/// aside, the statement terminator in force (`delim`, or `;` when that is
/// `None`), a directive, or nothing at all?
fn at_statement_start(toks: &[(Kind, &str)], delim: Option<&str>) -> bool {
    match toks
        .iter()
        .rev()
        .find(|(k, _)| !matches!(k, Kind::LineComment | Kind::BlockComment))
    {
        None => true,
        Some((Kind::Directive | Kind::Terminator, _)) => true,
        Some((Kind::Punct, ";")) => delim.is_none(),
        Some(_) => false,
    }
}

/// `sql`'s tokens, with `delim` the terminator a MySQL `DELIMITER` set where
/// `sql` begins (`None` for `;`, and for a whole document), left as the one
/// in force where it ends.
fn tokenize<'s>(
    sql: &'s str,
    dialect: SqlDialect,
    delim: &mut Option<String>,
) -> Vec<(Kind, &'s str)> {
    let b = sql.as_bytes();
    let n = b.len();
    let mut toks: Vec<(Kind, &str)> = Vec::new();
    // The brackets the code so far leaves open, for the placeholder arm: the
    // bytes are fed here as each arm consumes them, so a `[` in a string or a
    // comment is never one.
    let mut brackets = crate::params::Brackets::default();
    let mut i = 0;
    while i < n {
        let c = b[i];
        if c.is_ascii_whitespace() {
            brackets.step(b, i);
            i += 1;
            continue;
        }
        // **A `DELIMITER` line is the client's, like `GO`**, and only where a
        // statement begins — after the current terminator, a directive or
        // nothing, comments aside — which is where `sql::statement_ranges`
        // takes one. Read as SQL it came out `DELIMITER $ $`, joined to the
        // statement after it. One verbatim line; `None` off MySQL.
        if matches!(c, b'd' | b'D')
            && at_statement_start(&toks, delim.as_deref())
            && let Some((end, token)) = crate::sql::delimiter_directive(sql, i, dialect)
        {
            toks.push((Kind::Directive, sql[i..end].trim_end()));
            *delim = (token != ";").then_some(token);
            i = end;
            continue;
        }
        // The terminator it set ends a statement as `;` does, and is one
        // token — `//` spaced to `/ /` is no terminator at all.
        if let Some(d) = delim.as_deref()
            && b[i..].starts_with(d.as_bytes())
        {
            toks.push((Kind::Terminator, &sql[i..i + d.len()]));
            brackets = crate::params::Brackets::default();
            i += d.len();
            continue;
        }
        // **A `GO` line is the client's batch separator, not SQL**, and is one
        // only on a line of its own: laid out as the ordinary word the arm
        // below would make of it, it joined the line before and the script
        // lost its batches. One verbatim token, count and comment included —
        // `sql::go_directive` is the one definition, and answers `None` on
        // every dialect but SQL Server's.
        if crate::sql::at_line_start(b, i)
            && let Some(end) = crate::sql::go_directive(sql, i, dialect)
        {
            toks.push((Kind::Directive, sql[i..end].trim_end()));
            i = end;
            continue;
        }
        // **An executable comment is one verbatim slice here, `*/` included.**
        // Ahead of `skip_noncode`, which deliberately ends that span just past
        // the *marker* so the security gates see the body as the code the
        // server runs. This is the fourth caller of that function and the one
        // asking a different question — what may I re-flow — and taking the
        // gates' answer split the closing `*/` into two puncts with a space
        // between them, leaving the statement an unterminated comment.
        if let Some(j) = crate::sql::executable_comment_end(b, i, dialect) {
            toks.push((Kind::BlockComment, &sql[i..j]));
            i = j;
            continue;
        }
        // A `*/` with no opener in this slice — what a selection ending past
        // the close of an executable comment hands us — is still one token, for
        // the same reason: `ops()` has no entry for it, so the two bytes would
        // otherwise reach the punctuation path separately and be spaced apart.
        if c == b'*' && b.get(i + 1) == Some(&b'/') {
            toks.push((Kind::Punct, &sql[i..i + 2]));
            i += 2;
            continue;
        }
        // Strings / identifiers / dollar-quotes / comments — one verbatim slice,
        // keyed by the opening byte (`skip_noncode` only returns `Some` for a
        // `--`/`/*`/`#` here when it really is a comment in this dialect).
        if let Some(j) = skip_noncode(b, i, dialect) {
            let kind = match c {
                b'#' | b'-' => Kind::LineComment,
                b'/' => Kind::BlockComment,
                _ => Kind::Quoted,
            };
            toks.push((kind, &sql[i..j]));
            i = j;
            continue;
        }
        // **A number is one token, exponent and all.** Ahead of the word arm
        // because two of a numeric literal's three parts are spelled in bytes
        // that arm calls punctuation: `1.5e-3` lexed as
        // `["1", ".", "5e", "-", "3"]`, and while `need_space` suppresses the
        // space around `.`, nothing said anything about a `-` after a word — the
        // `"-" | "+"` arm asked `is_keyword("5e")`, got `false`, called the sign
        // binary and spaced it. Format Code wrote `1.5e - 3` back over the
        // user's buffer, and `1.5e` is not a number on any of the three: SQLite
        // lexes it `TK_ILLEGAL`, PostgreSQL answers *trailing junk after numeric
        // literal*, MySQL reads `1.5` and an identifier `e`.
        //
        // The shape all three lexers share, and no more of it: a `.` is taken
        // only ahead of a digit, and a sign only ahead of a digit and behind the
        // `e` the word scan already swallowed. So `select a-1` keeps its binary
        // minus and `select 1 e` keeps its two tokens.
        //
        // **A number may also begin or end at its point**, `.5` and `1.`, on
        // all four. Lexed as a `.` beside a word, `need_space` kept it tight
        // and wrote `between .1` as `between.1` (MariaDB: ERROR 1064) and
        // `1. as x` as `1.as x` (PostgreSQL: *trailing junk*). A point
        // straight after a name, a quote or a `)` is a qualifier's, as the
        // engines read it, and a second point (`db..t`) is never a number's.
        let point_number = c == b'.'
            && b.get(i + 1).is_some_and(u8::is_ascii_digit)
            && !i.checked_sub(1).is_some_and(|p| {
                is_word_byte(b[p]) || matches!(b[p], b'.' | b')' | b']' | b'"' | b'`' | b'\'')
            });
        if c.is_ascii_digit() || point_number {
            let s = i;
            i += usize::from(point_number);
            while i < n && is_word_byte(b[i]) {
                i += 1;
            }
            // `1.` — a point ending an integer, with no name or second point
            // after it, and not a qualified name's part (`t.1.x`).
            let integer = !point_number && b[s..i].iter().all(u8::is_ascii_digit);
            let qualified = matches!(toks.last(), Some((Kind::Punct, ".")))
                && s.checked_sub(1).is_some_and(|p| b[p] == b'.');
            if integer
                && !qualified
                && b.get(i) == Some(&b'.')
                && !b.get(i + 1).is_some_and(|&c| is_word_byte(c) || c == b'.')
            {
                toks.push((Kind::Word, &sql[s..i + 1]));
                i += 1;
                continue;
            }
            // `.` ahead of a digit, or of the exponent a bare point may have
            // (`1.e-5` is a number on all four).
            let exponent_at = |k: usize| {
                matches!(b.get(k), Some(b'e' | b'E'))
                    && match b.get(k + 1) {
                        Some(b'+' | b'-') => b.get(k + 2).is_some_and(u8::is_ascii_digit),
                        Some(c) => c.is_ascii_digit(),
                        None => false,
                    }
            };
            if !point_number
                && i < n
                && b[i] == b'.'
                && (b.get(i + 1).is_some_and(u8::is_ascii_digit) || exponent_at(i + 1))
            {
                i += 1;
                while i < n && is_word_byte(b[i]) {
                    i += 1;
                }
            }
            if matches!(b[i - 1], b'e' | b'E')
                && matches!(b.get(i), Some(b'+' | b'-'))
                && b.get(i + 1).is_some_and(u8::is_ascii_digit)
            {
                i += 1;
                while i < n && b[i].is_ascii_digit() {
                    i += 1;
                }
            }
            toks.push((Kind::Word, &sql[s..i]));
            continue;
        }
        // **A T-SQL `@x`, `@@ROWCOUNT`, `#t` or `##g` is one word.** Left to
        // the operator arm below, `@`/`#` became a `Punct` and `need_space`
        // separated it from its name — `SELECT @ x` is Msg 137, `CREATE TABLE
        // # t` Msg 102 — so one Format Code broke nearly every T-SQL script.
        // The other engines' sigils are [`sigil_len`]'s.
        let prefix = sigil_len(b, i, dialect);
        if is_word_byte(c) || prefix > 0 {
            let s = i;
            i += prefix.max(1);
            // `continues_name`, so `h#` and `f@x` stay whole on T-SQL — up to
            // a `DELIMITER`'s terminator, which `END$$` ends in.
            while i < n
                && crate::sql::continues_name(b[i], dialect)
                && !delim
                    .as_ref()
                    .is_some_and(|d| b[i..].starts_with(d.as_bytes()))
            {
                i += 1;
            }
            // **A literal's prefix is part of the literal**, so the two are one
            // token here as they are in every engine's own lexer. Otherwise
            // `need_space` puts a space between them, and a space is not
            // whitespace there: `E'esc\'aped'` is `esc'aped` on PostgreSQL 16
            // while `E 'esc\'aped'` is *ERROR: unterminated quoted string* —
            // measured — and `X'ff'`, `B'101'` and MySQL's `_utf8mb4'x'`
            // introducer are the same shape. Only where the input had them
            // adjacent: if the user wrote a space the engine already read two
            // tokens, and this keeps whichever it was.
            let mut end = i;
            // PostgreSQL's `U&'…'` is the one prefix not spelled in word bytes.
            if end < n && b[end] == b'&' && sql[s..end].eq_ignore_ascii_case("U") {
                end += 1;
            }
            if end < n
                && (b[end] == b'\'' || crate::intel::ident_quote(dialect, b[end]).is_some())
                && let Some(j) = skip_noncode(b, end, dialect)
            {
                toks.push((Kind::Quoted, &sql[s..j]));
                i = j;
                continue;
            }
            toks.push((Kind::Word, &sql[s..i]));
            continue;
        }
        // **A `:name` placeholder is one token.** `params`' doc owns the
        // spelling — "a placeholder is `:` followed by an identifier" — and
        // this modelled `:` only as part of `::` or `:=`, so a bare one fell
        // through as a one-byte `Punct` and `need_space` put a space after it.
        // One Ctrl+Alt+L emptied the parameters bar, lost every value bound to a
        // name, and left a buffer that is a syntax error on all three engines.
        // The module's contract is "without changing any token's text", and a
        // placeholder is a token of the language this editor speaks.
        //
        // Ahead of the table, and unambiguously so: no operator spelling puts an
        // identifier byte after the `:` — `::` and `:=` both do not.
        //
        // **`params`' own predicate, not half of it.** Asking only "identifier
        // byte next" made `a[lo:hi]` one token; `need_space` wrote it back as
        // `lo :hi`, which `params` *does* read as a placeholder — so the
        // formatter created a parameter by moving a space, the exact inverse of
        // the defect this arm exists to fix.
        let opens = crate::params::opens_placeholder(b, i, &brackets);
        if opens {
            brackets.step(b, i);
            let mut end = i + 1;
            while end < b.len() && crate::sql::is_word_byte(b[end]) {
                end += 1;
            }
            toks.push((Kind::Word, &sql[i..end]));
            i = end;
            continue;
        }
        // Punctuation / operators. The table first, since its entries are the
        // spellings the composition rule cannot reach; then the run, for the
        // engine that composes.
        //
        // `get`, not `[..n]`: the guard below used to be `rest.len() >=
        // op.len()`, which proves the slice is in *range* and says nothing about
        // it being a char boundary — and the slice is taken for every candidate
        // before any comparison, so **Format Code panicked** on any punctuation
        // byte with a multi-byte character within `op.len() - 1` bytes of it
        // (`create table t (имя text)`), on the floem UI thread, on all three
        // dialects.
        let rest = &sql[i..];
        let len = ops(dialect)
            .iter()
            .find(|op| rest.get(..op.len()) == Some(**op))
            .map(|op| op.len())
            .or_else(|| {
                operators_are_composable(dialect)
                    .then(|| composed_operator_len(b, i, dialect))
                    .flatten()
            })
            .unwrap_or(1);
        toks.push((Kind::Punct, &sql[i..i + len]));
        for k in i..i + len {
            brackets.step(b, k);
        }
        i += len;
    }
    toks
}

// Clause keywords that begin a new line at the block's base indent; their body
// then indents one level.
const HEADS: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "HAVING",
    "LIMIT",
    "OFFSET",
    "VALUES",
    "SET",
    "UNION",
    "EXCEPT",
    "INTERSECT",
    "RETURNING",
    "INSERT",
    "UPDATE",
    "DELETE",
    "REPLACE",
    "WITH",
    "GROUP",
    "ORDER",
    "WINDOW",
];

// Words that may follow a head on the same line (e.g. `GROUP BY`, `SELECT
// DISTINCT`, `UNION ALL`, `INSERT INTO`, `DELETE FROM`).
fn head_followers(up: &str) -> &'static [&'static str] {
    match up {
        "GROUP" | "ORDER" => &["BY"],
        "SELECT" => &["DISTINCT", "ALL", "DISTINCTROW", "SQL_CALC_FOUND_ROWS"],
        "UNION" => &["ALL", "DISTINCT"],
        // SQLite's conflict clause, `INSERT OR IGNORE INTO`, and MySQL's
        // `INSERT IGNORE INTO`.
        "INSERT" => &[
            "OR", "REPLACE", "IGNORE", "ABORT", "FAIL", "ROLLBACK", "INTO",
        ],
        "REPLACE" => &["INTO"],
        "UPDATE" => &["OR", "REPLACE", "IGNORE", "ABORT", "FAIL", "ROLLBACK"],
        "DELETE" => &["FROM"],
        "WITH" => &["RECURSIVE"],
        _ => &[],
    }
}

// Join keywords that begin a new line within a FROM body.
const JOIN_STARTS: &[&str] = &[
    "JOIN",
    "INNER",
    "LEFT",
    "RIGHT",
    "FULL",
    "CROSS",
    "NATURAL",
    "STRAIGHT_JOIN",
];
// Join modifiers: a join keyword right after one of these continues the same
// join clause inline (so `LEFT OUTER JOIN` doesn't break three times).
const JOIN_MODS: &[&str] = &[
    "LEFT",
    "RIGHT",
    "FULL",
    "INNER",
    "OUTER",
    "CROSS",
    "NATURAL",
    "STRAIGHT_JOIN",
];

// Keywords that take a space before `(` (`IN (…)`, `EXISTS (…)`, `VALUES (…)`)
// as opposed to a function call (`count(…)`), which stays tight. Function names
// aren't here, so `name(` renders tight while `KEYWORD (` gets a space.
fn is_keyword(w: &str) -> bool {
    let up = w.to_ascii_uppercase();
    HEADS.contains(&up.as_str())
        || JOIN_STARTS.contains(&up.as_str())
        || matches!(
            up.as_str(),
            "AND"
                | "OR"
                | "NOT"
                | "IN"
                | "IS"
                | "EXISTS"
                | "BETWEEN"
                | "LIKE"
                | "RLIKE"
                | "REGEXP"
                | "ON"
                | "USING"
                | "AS"
                | "BY"
                | "ALL"
                | "ANY"
                | "SOME"
                | "DISTINCT"
                | "CASE"
                | "WHEN"
                | "THEN"
                | "ELSE"
                | "END"
                | "INTO"
                | "VALUES"
                | "ASC"
                | "DESC"
                | "NULL"
                | "TRUE"
                | "FALSE"
                | "OVER"
                | "PARTITION"
                | "RETURNING"
                | "DEFAULT"
                // T-SQL's `TOP (n)` is a clause's parens, not a call's.
                | "TOP"
                // A key's or a check's column list: `PRIMARY KEY (id)`,
                // `UNIQUE (email)`, `CHECK (qty > 0)`.
                | "KEY"
                | "UNIQUE"
                | "CHECK"
                // An index's covered columns: `INCLUDE (x)`.
                | "INCLUDE"
        )
}

/// Does the engine have T-SQL's `OUTPUT` clause on a write? SQL Server's
/// alone — elsewhere `output` is a name.
fn has_output_clause(dialect: SqlDialect) -> bool {
    match dialect {
        SqlDialect::MsSql => true,
        SqlDialect::MySql | SqlDialect::Postgres | SqlDialect::Sqlite => false,
    }
}

/// Is `w` a clause keyword that is also a function, so a `(` straight after it
/// makes it a call — `LEFT(s, 3)`, `RIGHT(…)`, `REPLACE(…)` and MySQL's
/// `INSERT(…)` — rather than the clause it also spells? None of those clauses
/// is ever followed by `(` (`LEFT JOIN`, `REPLACE INTO`, `INSERT INTO`).
fn is_called_keyword(w: &str) -> bool {
    ["LEFT", "RIGHT", "REPLACE", "INSERT"]
        .iter()
        .any(|k| w.eq_ignore_ascii_case(k))
}

// Punctuation after which a `+`/`-` is unary (so the following number stays
// tight: `= -1`, `(-1`), not a binary operator.
fn unary_context(prev_punct: &str) -> bool {
    matches!(
        prev_punct,
        "(" | ","
            | "="
            | "<"
            | ">"
            | "<="
            | ">="
            | "<>"
            | "!="
            | "<=>"
            | ":="
            | "+"
            | "-"
            | "*"
            | "/"
            | "%"
            | "||"
            | "&&"
            | "<<"
            | ">>"
            | "->"
            | "->>"
    )
}

/// Where a line starts: `lv` indent units, then `col` spaces of alignment.
///
/// Two parts because a unit may be a tab, and an alignment never can be — a
/// column lined up under a word is a count of characters. The block layout
/// only ever moves `lv`; `col` is the river's and an aligned list's. Adding a
/// number adds levels, which is what every `base + 1` in the layout means.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Ind {
    lv: usize,
    col: usize,
}

impl Ind {
    /// `lv` levels and no alignment.
    fn lv(lv: usize) -> Ind {
        Ind { lv, col: 0 }
    }

    /// One level out, never below the margin.
    fn up(self) -> Ind {
        Ind {
            lv: self.lv.saturating_sub(1),
            ..self
        }
    }
}

impl std::ops::Add<usize> for Ind {
    type Output = Ind;
    fn add(self, levels: usize) -> Ind {
        Ind {
            lv: self.lv + levels,
            ..self
        }
    }
}

#[derive(Clone)]
struct Paren {
    subquery: bool,
    saved_base: Ind,
    saved_content: Ind,
    /// A `CREATE TABLE`'s body ([`Fmt::opens_table_body`]): one item a line,
    /// the `)` back at this indent.
    listed: Option<Ind>,
}

#[derive(Clone)]
struct Fmt<'a> {
    unit: &'a str,
    out: String,
    /// Pending line break to this indent, materialized on the next emit.
    pending: Option<Ind>,
    /// Emit a blank line (double newline) at the next materialized break.
    blank: bool,
    line_indent: Ind,
    line_has_content: bool,
    /// Clause base indent for the current block; clause keywords print here.
    base: Ind,
    /// Where clause bodies / commas / `AND`/`OR` break to (usually `base + 1`).
    content: Ind,
    parens: Vec<Paren>,
    /// How many `[` are open — an array constructor's or a subscript's. Their
    /// commas are an element list's, never a clause list's, and the brackets
    /// and a slice's `:` sit tight: `ARRAY[1,2]` came out `array [ 1,⏎  2 ]`.
    brackets: usize,
    prev: Option<(Kind, String)>,
    /// Inside `BETWEEN … AND …`: the next `AND` stays inline.
    suppress_and: bool,
    /// Previous token was a unary sign → the next token is tight against it.
    tight_next: bool,
    /// The text the tokens were cut from, for what lay between two of them.
    src: &'a str,
    /// A `GO` on a line of its own is a batch separator here
    /// ([`SqlDialect::batch_separator`]), so a name spelled `go` must not be
    /// left on one.
    go_separates: bool,
    /// A `go` name had to start its line: the next token stays on it.
    glue_next: bool,
    /// The paren depth of the `LIMIT` clause being laid out, if one is: its
    /// comma is MySQL's `LIMIT offset, count`, not a list's. A depth, so the
    /// comma after a subquery's `LIMIT` closes back into an ordinary list.
    limit_depth: Option<usize>,
    /// The `(` being written is a table body's, set off from the name before
    /// it ([`Fmt::opens_table_body`]).
    spaced_paren: bool,
    /// The `CASE`s open, innermost last.
    cases: Vec<CaseFrame>,
    /// The statement's first word, upper-cased — what an `OUTPUT` is a
    /// clause of ([`has_output_clause`]).
    verb: Option<String>,
    /// `OUTPUT` can be a write's clause here ([`has_output_clause`]).
    output_clause: bool,
    /// The bodies open, innermost last, each by the word that opened it:
    /// `BEGIN`, or where an `END` names what it closes
    /// ([`crate::pairs::names_its_block_ends`]) `IF`, `LOOP`, `WHILE`, `FOR`,
    /// `REPEAT` — `UNTIL` once a `REPEAT` reaches its condition. A statement
    /// inside one starts that many levels in, and its `;` returns there
    /// rather than to the margin.
    blocks: Vec<&'static str>,
    /// An `IF`/`ELSEIF`, `WHILE` or `FOR` header being written: the word that
    /// ends it and opens the body (`THEN`, `DO`), and the block it opens.
    awaiting: Option<(&'static str, &'static str)>,
    /// Compound statements are bodies here ([`crate::pairs::names_its_block_ends`]).
    named_ends: bool,
    /// Operators are composed of operator characters here
    /// ([`operators_are_composable`]), so two written tight are one.
    composable: bool,
    /// A broken list's comma opens the next line rather than ending this one
    /// ([`CommaStyle::Leading`]).
    leading_commas: bool,
}

/// One open `CASE`: the indent of the line it opened on, the paren depth it
/// opened at (its `WHEN`/`ELSE`/`END` are the ones at that depth), and whether
/// it is laid out — not inside a function's parens, where it stays inline.
#[derive(Clone)]
struct CaseFrame {
    indent: Ind,
    depth: usize,
    laid_out: bool,
}

impl<'a> Fmt<'a> {
    fn new(unit: &'a str, src: &'a str, dialect: SqlDialect) -> Self {
        Fmt {
            src,
            go_separates: dialect.batch_separator(),
            glue_next: false,
            unit,
            out: String::new(),
            pending: None,
            blank: false,
            line_indent: Ind::default(),
            line_has_content: false,
            base: Ind::default(),
            content: Ind::default(),
            parens: Vec::new(),
            brackets: 0,
            prev: None,
            suppress_and: false,
            tight_next: false,
            limit_depth: None,
            spaced_paren: false,
            cases: Vec::new(),
            verb: None,
            output_clause: has_output_clause(dialect),
            blocks: Vec::new(),
            awaiting: None,
            named_ends: crate::pairs::names_its_block_ends(dialect),
            composable: operators_are_composable(dialect),
            leading_commas: false,
        }
    }

    /// Open a body of `kind` after the word just written: what follows starts
    /// a statement one level in.
    fn open_block(&mut self, kind: &'static str) {
        self.blocks.push(kind);
        self.base = Ind::lv(self.blocks.len());
        self.content = self.base;
        self.prev = None;
        self.break_to(self.base);
    }

    /// Is the next token one where a statement starts — after a `;`, a body's
    /// opening word, or a label's `:`?
    fn at_statement_start(&self) -> bool {
        match &self.prev {
            None => true,
            Some((Kind::Punct, p)) => p == ":",
            Some(_) => false,
        }
    }

    /// Does the `END` at `toks[k]` close the innermost body? A bare `END` (or
    /// T-SQL's `END TRY`/`CATCH`) closes a `BEGIN`; an `END IF` and its kin
    /// the statement they name; an `END CASE` a `CASE` statement — from inside
    /// its last branch's body, or from a `CASE` with none.
    fn closes_block(&self, toks: &[(Kind, &str)], k: usize) -> bool {
        let Some(&top) = self.blocks.last() else {
            return false;
        };
        let named = match toks.get(k + 1) {
            Some((Kind::Word, w)) => ["IF", "LOOP", "WHILE", "REPEAT", "FOR", "CASE"]
                .into_iter()
                .find(|t| w.eq_ignore_ascii_case(t)),
            _ => None,
        };
        match named {
            None => top == "BEGIN",
            Some("REPEAT") => top == "UNTIL",
            // From inside its last branch's body, or a `CASE` with none.
            Some("CASE") => top == "WHEN" || top == "CASE",
            Some(n) => top == n,
        }
    }

    /// Is the `IF` at `toks[k]` a statement's? `IF (…)` is when `THEN`
    /// follows the parens, and otherwise the `IF()` function.
    fn if_statement(toks: &[(Kind, &str)], k: usize) -> bool {
        let mut rest = toks[k + 1..]
            .iter()
            .filter(|(kind, _)| !matches!(kind, Kind::LineComment | Kind::BlockComment));
        if !matches!(rest.next(), Some((Kind::Punct, "("))) {
            return true;
        }
        let mut depth = 1usize;
        for (kind, t) in rest.by_ref() {
            match (kind, *t) {
                (Kind::Punct, "(") => depth += 1,
                (Kind::Punct, ")") => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
        matches!(rest.next(), Some((Kind::Word, w)) if w.eq_ignore_ascii_case("THEN"))
    }

    /// After a `BEGIN`/`END` at `toks[k]`: T-SQL's `TRY`/`CATCH` stays with
    /// it. How many tokens it took.
    fn emit_block_follower(&mut self, toks: &[(Kind, &str)], k: usize) -> usize {
        match toks.get(k + 1) {
            Some((Kind::Word, w))
                if w.eq_ignore_ascii_case("TRY") || w.eq_ignore_ascii_case("CATCH") =>
            {
                self.emit(Kind::Word, w, false);
                1
            }
            _ => 0,
        }
    }

    /// Does the `BEGIN` at `toks[k]` open a body? Not a transaction's:
    /// `BEGIN TRAN`, `TRANSACTION`, `WORK`, `DISTRIBUTED`, or a bare `BEGIN;`.
    fn opens_body(toks: &[(Kind, &str)], k: usize) -> bool {
        let next = toks[k + 1..]
            .iter()
            .find(|(kind, _)| !matches!(kind, Kind::LineComment | Kind::BlockComment));
        match next {
            None | Some((Kind::Terminator, _)) | Some((Kind::Punct, ";")) => false,
            Some((Kind::Word, w)) => !["TRAN", "TRANSACTION", "WORK", "DISTRIBUTED"]
                .iter()
                .any(|t| w.eq_ignore_ascii_case(t)),
            Some(_) => true,
        }
    }

    /// The innermost open `CASE`, if it opened at the current paren depth.
    fn case_here(&self) -> Option<&CaseFrame> {
        self.cases.last().filter(|f| f.depth == self.parens.len())
    }

    fn break_to(&mut self, level: Ind) {
        // Overwrites any prior pending break, so repeated breaks before real text
        // coalesce to the last requested level (no blank clause lines).
        self.pending = Some(level);
    }

    /// The input between tokens `a` and `b`, both slices of `src` with `a`
    /// first — whitespace only, since every other byte is some token's.
    fn between(&self, a: &str, b: &str) -> &'a str {
        let base = self.src.as_ptr() as usize;
        let a_end = a.as_ptr() as usize - base + a.len();
        let b_start = b.as_ptr() as usize - base;
        self.src.get(a_end..b_start).unwrap_or("")
    }

    fn emit(&mut self, kind: Kind, text: &str, tight_left: bool) {
        if std::mem::take(&mut self.glue_next) {
            self.pending = None;
            self.blank = false;
        }
        if let Some(level) = self.pending.take() {
            if !self.out.is_empty() {
                self.out.push('\n');
                if self.blank {
                    self.out.push('\n');
                }
            }
            self.blank = false;
            for _ in 0..level.lv {
                self.out.push_str(self.unit);
            }
            for _ in 0..level.col {
                self.out.push(' ');
            }
            self.line_indent = level;
            self.line_has_content = false;
        } else if self.line_has_content
            && (self.would_fuse(text)
                || (!tight_left && !self.tight_next && self.need_space(kind, text)))
        {
            self.out.push(' ');
        }
        self.tight_next = false;
        self.out.push_str(text);
        self.line_has_content = true;
        self.prev = Some((kind, text.to_string()));
    }

    /// Would writing `text` with no space in front of it butt two characters
    /// together into something the lexer reads as one token?
    ///
    /// **The case that shipped is `- -1`.** Both signs are unary — the first
    /// because the previous token is `=` or `*`, the second because
    /// `unary_context` lists `-` itself — so the first set `tight_next` and the
    /// second was emitted with no space, giving `--1`. On PostgreSQL and SQLite
    /// `--` opens a line comment with no whitespace required, so the rest of the
    /// line is gone: measured on PG 16.15, `select 1 * --1` →
    /// *syntax error at end of input*. MySQL requires the whitespace, so the
    /// same output still means `-(-1)` there and the divergence was silent.
    ///
    /// Not dialect-gated even so. `- -1` is correct on all three engines and
    /// the alternative is a formatter whose output means different things on
    /// different servers, which is the opposite of this module's contract —
    /// "re-flow whitespace, indentation and line breaks **without changing any
    /// token's text**". `/*` and `*/` are guarded on the same principle, though
    /// no valid statement is known to produce either.
    ///
    /// **Where operators compose ([`operators_are_composable`]) any two
    /// operator characters fuse**: PostgreSQL reads `-~` as one operator, so
    /// a unary `-` written tight against a prefix `~` turned `- ~5` (6) into
    /// `-~ 5`, *operator does not exist* on 16.15 — and `- @ -3` into `-@ -3`.
    fn would_fuse(&self, text: &str) -> bool {
        let Some((pk, pt)) = &self.prev else {
            return false;
        };
        let (Some(&l), Some(&r)) = (pt.as_bytes().last(), text.as_bytes().first()) else {
            return false;
        };
        matches!((l, r), (b'-', b'-') | (b'/', b'*') | (b'*', b'/'))
            || (self.composable && *pk == Kind::Punct && is_operator_byte(l) && is_operator_byte(r))
    }

    fn need_space(&self, cur_kind: Kind, cur: &str) -> bool {
        // A line comment kept on a line of code is set off from it — asked
        // first, since after a `;` the statement has ended and `prev` with it.
        if cur_kind == Kind::LineComment {
            return true;
        }
        let Some((pk, pt)) = &self.prev else {
            return false;
        };
        let pt = pt.as_str();
        // prev forces no following space. `::` is a cast, and a cast binds to
        // what it casts the way `.` binds to what it qualifies: `id :: text` is
        // legal PostgreSQL and reads like a mistake.
        if *pk == Kind::Punct && (pt == "(" || pt == "." || pt == "::" || pt == "[") {
            return false;
        }
        // An account's host sits against its user, `'u'@'h'`, as it is written.
        // The spaced `'u' @'h'` runs too (measured); it only reads wrong.
        if *pk == Kind::Quoted && (cur.starts_with("@'") || cur.starts_with("@`")) {
            return false;
        }
        // cur forces no preceding space
        if cur_kind == Kind::Punct && matches!(cur, ")" | "," | ";" | "." | "::" | "]") {
            return false;
        }
        // A subscript or an array constructor's `[` binds to what it follows
        // (`arr[1]`, `ARRAY[1]`, `m[1][2]`), and a slice's `:` to both bounds.
        if cur_kind == Kind::Punct && cur == "[" {
            return !(matches!(pk, Kind::Word | Kind::Quoted)
                || (*pk == Kind::Punct && (pt == "]" || pt == ")")));
        }
        if self.brackets > 0 && ((cur_kind == Kind::Punct && cur == ":") || pt == ":") {
            return false;
        }
        // A compound statement's `IF (a > b) THEN` is no `IF()` call.
        if cur_kind == Kind::Punct
            && cur == "("
            && self.awaiting.is_some()
            && ["IF", "ELSEIF", "WHILE"]
                .iter()
                .any(|w| pt.eq_ignore_ascii_case(w))
        {
            return true;
        }
        if cur_kind == Kind::Punct && cur == "(" && self.spaced_paren {
            return true;
        }
        if cur_kind == Kind::Punct && cur == "(" {
            // Function call `name(` / `)(` → tight; `KEYWORD (` → spaced —
            // save a keyword that is also a function, whose `(` is a call's.
            if *pk == Kind::Word && (!is_keyword(pt) || is_called_keyword(pt)) {
                return false;
            }
            if *pk == Kind::Punct && pt == ")" {
                return false;
            }
            return true;
        }
        true
    }

    fn prev_word_upper(&self) -> Option<String> {
        match &self.prev {
            Some((Kind::Word, t)) => Some(t.to_ascii_uppercase()),
            _ => None,
        }
    }

    /// Is the `(` at `toks[k]` a table's body — `CREATE [TEMPORARY] TABLE [IF
    /// NOT EXISTS] name (` — whose items go one a line?
    fn opens_table_body(&self, toks: &[(Kind, &str)], k: usize) -> bool {
        if self.verb.as_deref() != Some("CREATE") {
            return false;
        }
        let Some(mut i) = name_start(toks, k) else {
            return false;
        };
        let word = |j: usize, w: &str| word_is(toks, j, w);
        // `IF NOT EXISTS` between `TABLE` and the name.
        if i >= 3 && word(i - 1, "EXISTS") && word(i - 2, "NOT") && word(i - 3, "IF") {
            i -= 3;
        }
        i >= 1 && word(i - 1, "TABLE")
    }

    /// Is the `(` at `toks[k]` a column list after a name — `REFERENCES u
    /// (id)`, `INDEX ix (a)`, `KEY k (a)`, a view's `VIEW v (x, y)`, an index's
    /// `ON t (a)` and its method's `USING btree (a)`? Set off from the name, as
    /// a table body's `(` is, rather than read as a call's.
    fn opens_column_list(&self, toks: &[(Kind, &str)], k: usize) -> bool {
        let Some(i) = name_start(toks, k).filter(|&i| i > 0) else {
            return false;
        };
        let Some((Kind::Word, before)) = toks.get(i - 1) else {
            return false;
        };
        let creates = matches!(self.verb.as_deref(), Some("CREATE"));
        match before.to_ascii_uppercase().as_str() {
            "REFERENCES" | "INDEX" | "KEY" => true,
            "VIEW" => creates || self.verb.as_deref() == Some("ALTER"),
            // A join's `ON f(x)` is a call; only an index's `ON` names a table.
            "ON" | "USING" => {
                creates
                    && toks[..i]
                        .iter()
                        .rev()
                        .take_while(|&&(kind, t)| {
                            !matches!(
                                (kind, t),
                                (Kind::Terminator | Kind::Directive, _) | (Kind::Punct, ";")
                            )
                        })
                        .any(|&(kind, t)| kind == Kind::Word && t.eq_ignore_ascii_case("INDEX"))
            }
            _ => false,
        }
    }

    /// Does the word at `toks[k]` open the first of an `ALTER TABLE`'s several
    /// actions — `ALTER TABLE [IF EXISTS] [ONLY] name ACTION …, ACTION …`?
    /// Then each goes on a line of its own, one in from the header. A single
    /// action keeps the header's line, and a comma inside its own parens
    /// (`ADD INDEX ix (a, b)`) is not a second.
    fn opens_alter_actions(&self, toks: &[(Kind, &str)], k: usize) -> bool {
        if self.verb.as_deref() != Some("ALTER") || !self.parens.is_empty() {
            return false;
        }
        let Some(mut i) = name_start(toks, k) else {
            return false;
        };
        let word = |j: usize, w: &str| word_is(toks, j, w);
        // Still in the header: what ends there is `IF EXISTS` or `ONLY`.
        if ["IF", "EXISTS", "ONLY"].iter().any(|w| word(k - 1, w)) {
            return false;
        }
        if i >= 1 && word(i - 1, "ONLY") {
            i -= 1;
        }
        if i >= 2 && word(i - 1, "EXISTS") && word(i - 2, "IF") {
            i -= 2;
        }
        if !(i >= 2 && word(i - 1, "TABLE") && word(i - 2, "ALTER")) {
            return false;
        }
        let mut depth = 0usize;
        for &(kind, t) in &toks[k..] {
            match (kind, t) {
                (Kind::Terminator | Kind::Directive, _) | (Kind::Punct, ";") => break,
                (Kind::Punct, "(") => depth += 1,
                (Kind::Punct, ")") => depth = depth.saturating_sub(1),
                (Kind::Punct, ",") if depth == 0 => return true,
                _ => {}
            }
        }
        false
    }

    /// Is the head word `up` at `toks[k]` only **spelling** a clause, inside
    /// something else that uses the word? Then it stays on its line:
    ///
    /// - `DELETE`/`UPDATE`/`INSERT` naming an event — a foreign key's
    ///   `ON DELETE`, a lock's `FOR UPDATE`, a trigger's `AFTER INSERT`,
    ///   `BEFORE UPDATE`, `INSERT OR UPDATE`, `UPDATE OF`;
    /// - `SET` in `CHARACTER SET`, an action's `ON DELETE SET NULL`, and any
    ///   `SET` of an `ALTER` (`ALTER COLUMN c SET DEFAULT 0`);
    /// - `REPLACE` of `CREATE OR REPLACE`;
    /// - `WITH` that opens no CTE list — `WITH ROLLUP`, `WITH CHECK OPTION`,
    ///   `TIMESTAMP WITH TIME ZONE`, a table hint's `WITH (NOLOCK)` — which is
    ///   every `WITH` not followed by `RECURSIVE` or by a name and then `AS`
    ///   or a column list.
    fn only_spells_a_clause(
        up: &str,
        prev: Option<&str>,
        verb: Option<&str>,
        toks: &[(Kind, &str)],
        k: usize,
    ) -> bool {
        let word_at = |i: usize| match toks.get(i) {
            Some((Kind::Word, w)) => Some(w.to_ascii_uppercase()),
            _ => None,
        };
        match up {
            "DELETE" | "UPDATE" | "INSERT" => matches!(
                prev,
                Some("ON" | "FOR" | "BEFORE" | "AFTER" | "OF" | "OR" | "INSTEAD")
            ),
            "SET" => {
                matches!(prev, Some("CHARACTER" | "DELETE" | "UPDATE")) || verb == Some("ALTER")
            }
            "REPLACE" => prev == Some("OR"),
            "WITH" => {
                let opens = word_at(k + 1).as_deref() == Some("RECURSIVE")
                    || (word_at(k + 1).is_some()
                        && (word_at(k + 2).as_deref() == Some("AS")
                            || matches!(toks.get(k + 2), Some((Kind::Punct, "(")))));
                !opens
            }
            _ => false,
        }
    }

    fn in_expr_paren(&self) -> bool {
        matches!(self.parens.last(), Some(p) if !p.subquery)
    }

    fn run(&mut self, toks: &[(Kind, &str)]) {
        let mut k = 0;
        while k < toks.len() {
            k = self.step(toks, k);
        }
    }

    /// Lay out `toks[k]` — and any follower it takes with it, a head's `BY`
    /// or a `BEGIN`'s `TRY` — answering the index of the next token to lay
    /// out. One token at a time so a layout can be tried and taken back.
    fn step(&mut self, toks: &[(Kind, &str)], mut k: usize) -> usize {
        {
            let (kind, text) = toks[k];
            // What the input had between this token and the one before it.
            let prev_tok = k.checked_sub(1).map(|j| toks[j]);
            let gap = prev_tok.map_or("", |(_, p)| self.between(p, text));
            match kind {
                Kind::LineComment => {
                    // A comment that trailed its line's code stays on that
                    // line: the break the code before it asked for — a list
                    // comma's, a clause head's — is taken after it instead.
                    // Moved down past the break, `a, -- first` read as the
                    // next item's.
                    let trailing = prev_tok.is_some() && !gap.contains('\n');
                    let deferred = if trailing { self.pending.take() } else { None };
                    self.emit(kind, text, false);
                    // A line comment runs to EOL; whatever follows must be on a
                    // new line at the current structural indent.
                    self.break_to(deferred.unwrap_or(self.line_indent));
                }
                Kind::BlockComment => {
                    self.emit(kind, text, false);
                }
                Kind::Quoted => {
                    // **A string continued on the next line stays there.**
                    // PostgreSQL joins `'foo'⏎'bar'` into one literal only
                    // because of the newline; on one line it is a syntax error.
                    let continued = gap.contains('\n')
                        && text.ends_with('\'')
                        && prev_tok.is_some_and(|(pk, p)| pk == Kind::Quoted && p.ends_with('\''));
                    if continued {
                        self.break_to(self.line_indent);
                    }
                    self.emit(kind, text, false);
                }
                Kind::Directive => {
                    // On a line of its own at the margin, directly under the
                    // batch it ends; the next batch starts afresh after a
                    // blank line, as a statement does after its `;`.
                    self.blank = false;
                    self.break_to(Ind::default());
                    self.emit(kind, text, false);
                    // A batch's end closes whatever body it left open.
                    self.blocks.clear();
                    self.awaiting = None;
                    self.base = Ind::default();
                    self.content = Ind::default();
                    self.parens.clear();
                    self.suppress_and = false;
                    self.prev = None;
                    self.blank = true;
                    self.break_to(Ind::default());
                }
                Kind::Terminator => {
                    // Tight against the statement, as `;` is — unless it is
                    // spelled in word bytes and would run into a word.
                    let tight = !text.as_bytes().first().is_some_and(|&c| is_word_byte(c));
                    self.emit(kind, text, tight);
                    self.end_statement();
                }
                Kind::Word => {
                    let up = text.to_ascii_uppercase();
                    let in_expr = self.in_expr_paren();
                    if self.prev.is_none() {
                        self.verb = Some(up.clone());
                    }
                    // A write's `OUTPUT inserted.id`, not a parameter's
                    // direction (`@x int OUTPUT`) nor a column (`output = 1`).
                    let output_head = up == "OUTPUT"
                        && self.output_clause
                        && matches!(
                            self.verb.as_deref(),
                            Some("INSERT" | "UPDATE" | "DELETE" | "MERGE")
                        )
                        && matches!(toks.get(k + 1), Some((Kind::Word, _)));
                    // **A word after `.` is a name**: MySQL reads `t.order` as
                    // a column only with nothing between the two, and laid out
                    // as a clause it became `t.⏎order` — `ORDER` again.
                    let after_dot = matches!(&self.prev, Some((Kind::Punct, p)) if p == ".");
                    let called = is_called_keyword(&up)
                        && matches!(toks.get(k + 1), Some((Kind::Punct, "(")));
                    if self.opens_alter_actions(toks, k) {
                        self.content = self.base + 1;
                        self.break_to(self.content);
                    }
                    // `CREATE OR REPLACE` is no boolean `OR`. (SQLite's `INSERT
                    // OR IGNORE` is its head's follower.)
                    let verb_or = up == "OR"
                        && self.verb.as_deref() == Some("CREATE")
                        && self.prev_word_upper().as_deref() == Some("CREATE");
                    // A view's `WITH [CASCADED | LOCAL] CHECK OPTION` ends its
                    // query on a line of its own.
                    let check_option = up == "WITH"
                        && !in_expr
                        && matches!(self.verb.as_deref(), Some("CREATE" | "ALTER"))
                        && (word_is(toks, k + 2, "OPTION") || word_is(toks, k + 3, "OPTION"))
                        && (word_is(toks, k + 1, "CHECK") || word_is(toks, k + 2, "CHECK"));
                    if self.go_separates && up == "GO" && !after_dot && self.pending.is_some() {
                        // **Never alone on a line where `GO` separates
                        // batches**, which a name `go` would be once its
                        // neighbours broke around it. Kept on the line before
                        // — or, where a line comment ends that one, with the
                        // token after it on its own.
                        if matches!(prev_tok, Some((Kind::LineComment, _))) {
                            self.emit(Kind::Word, text, false);
                            self.glue_next = true;
                        } else {
                            self.pending = None;
                            self.blank = false;
                            self.emit(Kind::Word, text, false);
                        }
                    } else if after_dot {
                        self.emit(Kind::Word, text, false);
                    } else if let Some((ends, kind)) = self.awaiting
                        && up == ends
                        && !in_expr
                        && self.case_here().is_none()
                    {
                        // `THEN`/`DO` ends the header: the body opens.
                        self.emit(Kind::Word, text, false);
                        self.awaiting = None;
                        self.open_block(kind);
                    } else if self.named_ends && !in_expr && self.at_statement_start() && {
                        let opener = matches!(up.as_str(), "LOOP" | "REPEAT" | "WHILE" | "FOR");
                        opener || (up == "IF" && Self::if_statement(toks, k))
                    } {
                        // A compound statement: `LOOP` and `REPEAT` open their
                        // body at once, the others after their header.
                        self.emit(Kind::Word, text, false);
                        match up.as_str() {
                            "LOOP" => self.open_block("LOOP"),
                            "REPEAT" => self.open_block("REPEAT"),
                            _ => {
                                self.awaiting = Some(match up.as_str() {
                                    "IF" => ("THEN", "IF"),
                                    "WHILE" => ("DO", "WHILE"),
                                    _ => ("DO", "FOR"),
                                });
                                self.content = self.base + 1;
                            }
                        }
                    } else if !in_expr
                        && self.case_here().is_none()
                        && self.blocks.last() == Some(&"IF")
                        && (up == "ELSEIF" || up == "ELSE")
                    {
                        // An `IF`'s next branch, at the `IF`'s level.
                        self.blocks.pop();
                        let level = Ind::lv(self.blocks.len());
                        self.base = level;
                        self.content = level;
                        self.break_to(level);
                        self.emit(Kind::Word, text, false);
                        if up == "ELSE" {
                            self.open_block("IF");
                        } else {
                            self.awaiting = Some(("THEN", "IF"));
                            self.content = level + 1;
                        }
                    } else if up == "UNTIL" && !in_expr && self.blocks.last() == Some(&"REPEAT") {
                        // A `REPEAT`'s condition, at its level; the `END
                        // REPEAT` after it stays on its line.
                        self.blocks.pop();
                        self.blocks.push("UNTIL");
                        let level = Ind::lv(self.blocks.len() - 1);
                        self.base = level;
                        self.content = level + 1;
                        self.break_to(level);
                        self.emit(Kind::Word, text, false);
                    } else if self.named_ends
                        && !in_expr
                        && up == "CASE"
                        && self.at_statement_start()
                    {
                        // A `CASE` statement: its operand stays on the line,
                        // and its branches open bodies as an `IF`'s do.
                        self.emit(Kind::Word, text, false);
                        self.blocks.push("CASE");
                        self.content = Ind::lv(self.blocks.len());
                    } else if !in_expr
                        && self.case_here().is_none()
                        && matches!(self.blocks.last(), Some(&("CASE" | "WHEN")))
                        && (up == "WHEN" || up == "ELSE")
                    {
                        // A `CASE` statement's next branch, one level in from
                        // the `CASE`; its statements one further.
                        if self.blocks.last() == Some(&"WHEN") {
                            self.blocks.pop();
                        }
                        let level = Ind::lv(self.blocks.len());
                        self.base = level;
                        self.content = level;
                        self.break_to(level);
                        self.emit(Kind::Word, text, false);
                        if up == "ELSE" {
                            self.open_block("WHEN");
                        } else {
                            self.awaiting = Some(("THEN", "WHEN"));
                            self.content = level + 1;
                        }
                    } else if called || verb_or {
                        self.emit(Kind::Word, text, false);
                    } else if check_option {
                        self.break_to(self.base);
                        self.emit(Kind::Word, text, false);
                    } else if up == "CASE" && self.prev_word_upper().as_deref() != Some("END") {
                        // Opens a frame; `END CASE`'s word closes one instead.
                        self.emit(Kind::Word, text, false);
                        self.cases.push(CaseFrame {
                            indent: self.line_indent,
                            depth: self.parens.len(),
                            laid_out: !in_expr,
                        });
                    } else if (up == "WHEN" || up == "ELSE")
                        && let Some(f) = self.case_here().filter(|f| f.laid_out)
                    {
                        self.break_to(f.indent + 1);
                        self.emit(Kind::Word, text, false);
                    } else if up == "END" && self.case_here().is_some() {
                        if let Some(f) = self.cases.pop()
                            && f.laid_out
                        {
                            self.break_to(f.indent);
                        }
                        self.emit(Kind::Word, text, false);
                    } else if up == "BEGIN" && !in_expr && Self::opens_body(toks, k) {
                        // A body: on its own line unless it ends a header
                        // (`AS BEGIN`, `THEN BEGIN`), with T-SQL's TRY/CATCH.
                        // A label's `l: BEGIN` keeps its line too.
                        let ends_header = matches!(
                            self.prev_word_upper().as_deref(),
                            Some("AS" | "THEN" | "ELSE" | "DO")
                        ) || matches!(&self.prev, Some((Kind::Punct, p)) if p == ":");
                        if !ends_header {
                            self.break_to(self.base);
                        }
                        self.emit(Kind::Word, text, false);
                        k += self.emit_block_follower(toks, k);
                        self.open_block("BEGIN");
                    } else if up == "END" && !in_expr && self.closes_block(toks, k) {
                        // Back to the opener's level — on the `UNTIL`'s line
                        // for an `END REPEAT`.
                        let popped = self.blocks.pop();
                        // An `END CASE` closes its last branch and the `CASE`.
                        if popped == Some("WHEN") {
                            self.blocks.pop();
                        }
                        let until = popped == Some("UNTIL");
                        let level = Ind::lv(self.blocks.len());
                        self.base = level;
                        self.content = level;
                        if !until {
                            self.break_to(level);
                        }
                        self.emit(Kind::Word, text, false);
                        k += self.emit_block_follower(toks, k);
                    } else if !in_expr
                        && ((HEADS.contains(&up.as_str())
                            && !Self::only_spells_a_clause(
                                &up,
                                self.prev_word_upper().as_deref(),
                                self.verb.as_deref(),
                                toks,
                                k,
                            ))
                            || output_head)
                    {
                        self.limit_depth = (up == "LIMIT").then_some(self.parens.len());
                        self.break_to(self.base);
                        self.emit(Kind::Word, text, false);
                        // Attach any followers on the same line.
                        let folls = head_followers(&up);
                        while k + 1 < toks.len()
                            && toks[k + 1].0 == Kind::Word
                            && folls.iter().any(|f| toks[k + 1].1.eq_ignore_ascii_case(f))
                        {
                            self.emit(Kind::Word, toks[k + 1].1, false);
                            k += 1;
                        }
                        self.content = self.base + 1;
                        self.break_to(self.content);
                    } else if !in_expr
                        && (self.is_join_break(&up)
                            || ((up == "AND" || up == "OR") && !(up == "AND" && self.suppress_and)))
                    {
                        // A join keyword (JOIN/LEFT/…) or a boolean AND/OR both
                        // break to the content indent before emitting.
                        self.break_to(self.content);
                        self.emit(Kind::Word, text, false);
                    } else {
                        if up == "AND" && self.suppress_and {
                            self.suppress_and = false;
                        }
                        if up == "BETWEEN" {
                            self.suppress_and = true;
                        }
                        self.emit(Kind::Word, text, false);
                    }
                }
                Kind::Punct => match text {
                    "(" => {
                        let sub = self.next_is_subquery(toks, k);
                        let body = !sub && self.opens_table_body(toks, k);
                        // A table's body or a column list is set off from its
                        // name; a call's `(` would sit against it.
                        self.spaced_paren = body || (!sub && self.opens_column_list(toks, k));
                        self.emit(Kind::Punct, "(", false);
                        self.spaced_paren = false;
                        self.parens.push(Paren {
                            subquery: sub,
                            saved_base: self.base,
                            saved_content: self.content,
                            listed: body.then_some(self.line_indent),
                        });
                        if sub {
                            self.base = self.line_indent + 1;
                            self.content = self.base;
                            self.break_to(self.base);
                        } else if body {
                            self.content = self.line_indent + 1;
                            self.break_to(self.content);
                        }
                    }
                    ")" => {
                        if let Some(p) = self.parens.pop() {
                            if let Some(at) = p.listed {
                                self.break_to(at);
                                self.emit(Kind::Punct, ")", true);
                                self.base = p.saved_base;
                                self.content = p.saved_content;
                            } else if p.subquery {
                                let close = self.base.up();
                                self.break_to(close);
                                self.emit(Kind::Punct, ")", true);
                                self.base = p.saved_base;
                                self.content = p.saved_content;
                            } else {
                                self.emit(Kind::Punct, ")", true);
                            }
                        } else {
                            self.emit(Kind::Punct, ")", true);
                        }
                    }
                    "," => {
                        // Break top-level list commas and subquery SELECT-list
                        // commas; keep function-argument / IN-list commas inline.
                        let breakable = self.brackets == 0
                            && self.limit_depth != Some(self.parens.len())
                            && match self.parens.last() {
                                None => true,
                                Some(p) => p.subquery || p.listed.is_some(),
                            };
                        if breakable && self.leading_commas {
                            // The same break, taken before the comma.
                            self.break_to(self.content);
                            self.emit(Kind::Punct, ",", true);
                        } else {
                            self.emit(Kind::Punct, ",", true);
                            if breakable {
                                self.break_to(self.content);
                            }
                        }
                    }
                    ";" => {
                        self.emit(Kind::Punct, ";", true);
                        self.end_statement();
                    }
                    "-" | "+" => {
                        let unary = match &self.prev {
                            None => true,
                            Some((Kind::Punct, pt)) => unary_context(pt),
                            Some((Kind::Word, t)) => is_keyword(t),
                            _ => false,
                        };
                        self.emit(Kind::Punct, text, false);
                        if unary {
                            self.tight_next = true;
                        }
                    }
                    // A label's colon sits against its name: `l: LOOP`.
                    ":" if self.brackets == 0
                        && matches!(&self.prev, Some((Kind::Word, _)))
                        && matches!(toks.get(k + 1), Some((Kind::Word, w))
                            if ["BEGIN", "LOOP", "WHILE", "REPEAT", "FOR"]
                                .iter()
                                .any(|t| w.eq_ignore_ascii_case(t))) =>
                    {
                        self.emit(Kind::Punct, text, true);
                    }
                    "[" => {
                        self.emit(Kind::Punct, text, false);
                        self.brackets += 1;
                    }
                    "]" => {
                        self.emit(Kind::Punct, text, false);
                        self.brackets = self.brackets.saturating_sub(1);
                    }
                    _ => {
                        self.emit(Kind::Punct, text, false);
                    }
                },
            }
        }
        k + 1
    }

    /// A statement's terminator was just written: the next one starts afresh
    /// at the margin, after a blank line.
    fn end_statement(&mut self) {
        // Back to the open body's level, not the margin — and no blank line
        // between a body's statements.
        let level = Ind::lv(self.blocks.len());
        self.base = level;
        self.content = level;
        self.parens.clear();
        self.brackets = 0;
        self.suppress_and = false;
        self.cases.clear();
        self.awaiting = None;
        self.prev = None;
        self.blank = level.lv == 0;
        self.break_to(level);
    }

    fn is_join_break(&self, up: &str) -> bool {
        if !JOIN_STARTS.contains(&up) {
            return false;
        }
        match self.prev_word_upper() {
            Some(p) => !JOIN_MODS.contains(&p.as_str()),
            None => true,
        }
    }

    // A `(` starts a subquery block (indented) when its first non-comment token
    // is a SELECT/WITH/VALUES; otherwise it's an inline expression/function paren.
    fn next_is_subquery(&self, toks: &[(Kind, &str)], k: usize) -> bool {
        for t in &toks[k + 1..] {
            match t.0 {
                Kind::LineComment | Kind::BlockComment => continue,
                Kind::Word => {
                    let up = t.1.to_ascii_uppercase();
                    return matches!(up.as_str(), "SELECT" | "WITH" | "VALUES" | "TABLE");
                }
                _ => return false,
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn my(sql: &str) -> String {
        super::format_sql(sql, "    ", SqlDialect::MySql)
    }

    #[test]
    fn a_create_tables_body_is_one_item_a_line() {
        assert_eq!(
            my(
                "CREATE TABLE t (id int NOT NULL, name varchar(40) CHARACTER SET utf8mb4, \
                PRIMARY KEY (id), CONSTRAINT fk FOREIGN KEY (a) REFERENCES b (id) \
                ON DELETE CASCADE ON UPDATE SET NULL)"
            ),
            "CREATE TABLE t (\n    id int NOT NULL,\n    name varchar(40) CHARACTER SET utf8mb4,\n    \
             PRIMARY KEY (id),\n    CONSTRAINT fk FOREIGN KEY (a) REFERENCES b (id) \
             ON DELETE CASCADE ON UPDATE SET NULL\n)"
        );
        assert_eq!(
            super::format_sql(
                "create table if not exists sales.t (id int, n numeric(10, 2)) engine=InnoDB",
                "  ",
                SqlDialect::MySql
            ),
            "create table if not exists sales.t (\n  id int,\n  n numeric(10, 2)\n) engine = InnoDB"
        );
    }

    #[test]
    fn a_word_that_only_spells_a_clause_does_not_open_one() {
        // A foreign key's actions, a charset, a row lock, a rollup.
        assert_eq!(
            my(
                "ALTER TABLE t ADD CONSTRAINT fk FOREIGN KEY (a) REFERENCES b (id) ON DELETE CASCADE"
            ),
            "ALTER TABLE t ADD CONSTRAINT fk FOREIGN KEY (a) REFERENCES b (id) ON DELETE CASCADE"
        );
        assert_eq!(
            my("SELECT a, count(*) FROM t GROUP BY a WITH ROLLUP"),
            "SELECT\n    a,\n    count(*)\nFROM\n    t\nGROUP BY\n    a WITH ROLLUP"
        );
        assert_eq!(
            my("SELECT * FROM t WHERE id = 1 FOR UPDATE"),
            "SELECT\n    *\nFROM\n    t\nWHERE\n    id = 1 FOR UPDATE"
        );
        assert_eq!(
            my("ALTER TABLE t MODIFY c varchar(9) CHARACTER SET latin1"),
            "ALTER TABLE t MODIFY c varchar(9) CHARACTER SET latin1"
        );
        // A real clause still opens: a CTE's `WITH`, an UPDATE's `SET`.
        assert!(my("WITH x AS (SELECT 1) SELECT * FROM x").starts_with("WITH\n"));
        assert!(my("UPDATE t SET a = 1").contains("\nSET\n"));
    }

    /// Upper or lower case for keywords — the words every server of the
    /// dialect reserves, and the formatter's own layout keywords — and nothing
    /// else: identifiers, strings, comments and quoted names keep their case.
    /// Keep leaves the text alone.
    #[test]
    fn keyword_case_recases_keywords_and_nothing_else() {
        use KeywordCase::*;
        let pg = SqlDialect::Postgres;
        let sql = "select Name, 'select' from Users -- from\nwhere x is not null order by \"from\"";
        assert_eq!(
            apply_keyword_case(sql, pg, Upper),
            "SELECT Name, 'select' FROM Users -- from\nWHERE x IS NOT NULL ORDER BY \"from\""
        );
        assert_eq!(
            apply_keyword_case("SELECT a FROM t WHERE b IS NULL", pg, Lower),
            "select a from t where b is null"
        );
        assert_eq!(apply_keyword_case(sql, pg, Keep), sql);
        // A word beside a `.` is a name — MySQL takes a reserved word there,
        // and a table's name may be case-sensitive.
        assert_eq!(
            apply_keyword_case("select t.order from shop.order", SqlDialect::MySql, Upper),
            "SELECT t.order FROM shop.order"
        );
        // MariaDB frees `rank`, so it is no keyword to recase on a MySQL tab.
        assert_eq!(
            apply_keyword_case("select rank from t", SqlDialect::MySql, Upper),
            "SELECT rank FROM t"
        );
        // PostgreSQL reserves neither `INSERT` nor `BY`; the formatter's own
        // keywords are cased all the same.
        assert_eq!(
            apply_keyword_case("insert into t values (1)", pg, Upper),
            "INSERT INTO t VALUES (1)"
        );
    }

    /// **A layout keyword the server does not reserve may be a name, and a
    /// name's case matters on MySQL and SQL Server** — a MySQL table on
    /// Linux, anything under a case-sensitive collation. `from top` written
    /// `FROM TOP` was ERROR 1146 on MariaDB 10.11. There such a word is
    /// recased only where it stands as the keyword.
    #[test]
    fn keyword_case_leaves_an_unreserved_keyword_used_as_a_name() {
        use KeywordCase::*;
        use SqlDialect::{MsSql, MySql, Postgres};
        for (d, sql) in [
            (MySql, "select * from top"),
            (MySql, "select * from full where x = 1"),
            (MySql, "select * from include, top"),
            (MySql, "select offset, end from t"),
            (MySql, "select a as offset from t"),
            (MySql, "update top set a = 1"),
            (MsSql, "select partition, include, offset from t"),
            (MsSql, "select * from window"),
        ] {
            let up = apply_keyword_case(sql, d, Upper);
            assert_eq!(
                up.to_ascii_lowercase(),
                sql,
                "{d:?}: lower-cased differently: {up}"
            );
            for w in [
                "top",
                "full",
                "include",
                "offset",
                "end",
                "partition",
                "window",
            ] {
                if sql
                    .split(|c: char| !c.is_ascii_alphanumeric())
                    .any(|x| x == w)
                {
                    assert!(
                        up.split(|c: char| !c.is_ascii_alphanumeric())
                            .any(|x| x == w),
                        "{d:?}: recased the name {w}: {up}"
                    );
                }
            }
        }
        // Where it is the keyword, it is still recased.
        for (d, sql, want) in [
            (
                MySql,
                "select a from t full join u on 1 = 1 limit 1 offset 5",
                "SELECT a FROM t FULL JOIN u ON 1 = 1 LIMIT 1 OFFSET 5",
            ),
            (
                MySql,
                "select case when a then 1 end from t where a = any (select 1)",
                "SELECT CASE WHEN a THEN 1 END FROM t WHERE a = ANY (SELECT 1)",
            ),
            (
                MySql,
                "create procedure p() begin select 1; end",
                // `begin` is neither reserved nor a layout keyword.
                "CREATE PROCEDURE p() begin SELECT 1; END",
            ),
            (
                MsSql,
                "select sum(a) over (partition by b) from t order by a offset 5 rows",
                "SELECT sum(a) OVER (PARTITION BY b) FROM t ORDER BY a OFFSET 5 rows",
            ),
            (
                MsSql,
                "create index ix on t (a) include (b)",
                "CREATE INDEX ix ON t (a) INCLUDE (b)",
            ),
        ] {
            assert_eq!(apply_keyword_case(sql, d, Upper), want, "{d:?}");
        }
        // PostgreSQL folds an unquoted name to lower case, so recasing one
        // there changes nothing it names: the old rule stands.
        assert_eq!(
            apply_keyword_case("select * from include", Postgres, Upper),
            "SELECT * FROM INCLUDE"
        );
    }

    /// The setting is saved as a lower-case word, and an unknown or missing
    /// one reads as Keep — the formatter's behaviour before it existed.
    #[test]
    fn keyword_case_persists_as_a_word() {
        assert_eq!(KeywordCase::default(), KeywordCase::Keep);
        assert_eq!(
            serde_json::to_string(&KeywordCase::Upper).unwrap(),
            "\"upper\""
        );
        let back: KeywordCase = serde_json::from_str("\"lower\"").unwrap();
        assert_eq!(back, KeywordCase::Lower);
        // A word a later version writes must not fail the whole settings file.
        let later: KeywordCase = serde_json::from_str("\"title\"").unwrap();
        assert_eq!(later, KeywordCase::Keep);
    }

    /// Format Code and *Format statement* write the case, and a statement
    /// laid out already but cased otherwise is still theirs to rewrite.
    #[test]
    fn formatting_writes_the_keyword_case() {
        let pg = SqlDialect::Postgres;
        let format_sql = super::format_sql;
        let laid_out = format_sql("SELECT a FROM t", "  ", pg);
        assert_eq!(
            format_statement(&laid_out, 0, "  ", pg, KeywordCase::Keep),
            None
        );
        assert_eq!(
            format_statement(&laid_out, 0, "  ", pg, KeywordCase::Lower),
            Some((0, laid_out.len(), format_sql("select a from t", "  ", pg)))
        );
        let full = "x; select a from t;";
        assert_eq!(
            format_range(full, 3, full.len(), "  ", pg, KeywordCase::Upper),
            format_sql("SELECT a FROM t;", "  ", pg)
        );
    }

    /// **Leading commas move exactly the commas the layout breaks at**: each
    /// line the default ends with `,` becomes the next line opening with `, `
    /// at the same indent — and nothing else changes, so an `IN (1, 2)` or a
    /// function's arguments, which stay on one line, keep theirs inline.
    #[test]
    fn leading_commas_move_the_breaking_commas_and_no_others() {
        let sql = "select a, coalesce(b, 0) as b, c from t, u \
                   where x in (1, 2) and y = (select max(z), 1 from w)";
        let trailing = format_range(sql, 0, sql.len(), IND, SqlDialect::MySql, KeywordCase::Keep);
        let leading = format_range(
            sql,
            0,
            sql.len(),
            IND,
            SqlDialect::MySql,
            FormatStyle {
                case: KeywordCase::Keep,
                commas: CommaStyle::Leading,
            },
        );
        // The default with each line-ending comma moved onto the next line.
        let mut expected: Vec<String> = Vec::new();
        let mut carry = false;
        for line in trailing.lines() {
            let indent = &line[..line.len() - line.trim_start().len()];
            let mut body = line.trim_start().to_string();
            if std::mem::take(&mut carry) {
                body = format!(", {body}");
            }
            if let Some(stripped) = body.strip_suffix(',') {
                body = stripped.to_string();
                carry = true;
            }
            expected.push(format!("{indent}{body}"));
        }
        assert!(
            trailing.contains(",\n"),
            "the fixture breaks at commas:\n{trailing}"
        );
        assert_eq!(leading, expected.join("\n"), "trailing:\n{trailing}");
        assert!(
            leading.contains("(1, 2)") && leading.contains("(b, 0)"),
            "{leading}"
        );
    }

    /// A line comment after a breaking comma still ends its line: the column
    /// after it must never land on the comment's line, where it would be
    /// commented out of the query.
    #[test]
    fn a_leading_comma_never_puts_code_after_a_line_comment() {
        let leading = FormatStyle {
            case: KeywordCase::Keep,
            commas: CommaStyle::Leading,
        };
        let sql = "select a, -- the key\n b, c from t";
        let out = format_range(sql, 0, sql.len(), IND, SqlDialect::MySql, leading);
        for line in out.lines() {
            if let Some(at) = line.find("--") {
                assert_eq!(line[at..].trim_end(), "-- the key", "{out}");
            }
        }
        assert!(out.contains('b') && out.contains('c'), "{out}");
        assert_eq!(
            super::format_sql(&out, IND, SqlDialect::MySql).replace([' ', '\n', ','], ""),
            super::format_sql(sql, IND, SqlDialect::MySql).replace([' ', '\n', ','], ""),
            "the same tokens either way:\n{out}"
        );
    }

    /// The setting reads leniently, as Keyword case's does: a word this
    /// version does not know is the default.
    #[test]
    fn comma_style_defaults_to_end_of_line_and_reads_leniently() {
        assert_eq!(CommaStyle::default(), CommaStyle::Trailing);
        let back: CommaStyle = serde_json::from_str("\"leading\"").unwrap();
        assert_eq!(back, CommaStyle::Leading);
        let odd: CommaStyle = serde_json::from_str("\"diagonal\"").unwrap();
        assert_eq!(odd, CommaStyle::Trailing);
        assert_eq!(
            serde_json::to_string(&CommaStyle::Leading).unwrap(),
            "\"leading\""
        );
    }

    // These tests predate the dialect parameter and assert MySQL formatting; a thin
    // MySQL-defaulting wrapper (shadowing the glob import) keeps them unchanged.
    fn format_sql(sql: &str, indent_unit: &str) -> String {
        super::format_sql(sql, indent_unit, SqlDialect::MySql)
    }

    const IND: &str = "  ";

    #[test]
    fn single_line_collapses_whitespace_in_code_and_comments() {
        use SqlDialect::MySql;
        let sql = "  SELECT a,\n\t  b\r\nFROM t /* two\n   lines */\nWHERE x = 1;\n";
        assert_eq!(
            single_line(sql, MySql).unwrap(),
            "SELECT a, b FROM t /* two lines */ WHERE x = 1;"
        );
        assert_eq!(single_line("", MySql).unwrap(), "");
    }

    #[test]
    fn single_line_turns_a_line_comment_into_a_block_comment() {
        use SqlDialect::{MySql, Postgres};
        assert_eq!(
            single_line("SELECT a -- the key\n  , b # note\nFROM t", MySql).unwrap(),
            "SELECT a /* the key */ , b /* note */ FROM t"
        );
        // An empty comment just goes.
        assert_eq!(single_line("a --\nb", MySql).unwrap(), "a b");
        // `#` is no comment on PostgreSQL.
        assert_eq!(single_line("a #- b\nc", Postgres).unwrap(), "a #- b c");
    }

    #[test]
    fn single_line_keeps_a_strings_line_breaks() {
        use SqlDialect::{MySql, Postgres};
        assert_eq!(
            single_line("INSERT INTO t\nVALUES ('a\n  b')", MySql).unwrap(),
            "INSERT INTO t VALUES ('a\n  b')"
        );
        let body = "$$\nBEGIN\n  -- keep\nEND\n$$";
        assert_eq!(
            single_line(&format!("CREATE FUNCTION f() AS\n{body}"), Postgres).unwrap(),
            format!("CREATE FUNCTION f() AS {body}")
        );
    }

    #[test]
    fn single_line_refuses_what_one_line_would_change() {
        use SqlDialect::{MsSql, MySql, Postgres};
        assert_eq!(single_line("SELECT 1\nGO\nSELECT 2", MsSql), None);
        assert_eq!(single_line("DELIMITER //\nSELECT 1//", MySql), None);
        assert_eq!(single_line("a -- x */ y\nb", MySql), None);
        // Where comments nest, a `/*` in the text would leave one open.
        assert_eq!(single_line("a -- x /* y\nb", Postgres), None);
        assert_eq!(
            single_line("a -- x /* y\nb", MySql).unwrap(),
            "a /* x /* y */ b"
        );
        // Inside a MySQL executable comment the `*/` of the rewritten comment
        // would close the executable one; outside one, a line comment is fine.
        assert_eq!(single_line("/*!50001 SELECT 1 -- n\n */", MySql), None);
        assert_eq!(single_line("/*!50001 SELECT 1 */ -- n\nx", MySql), None);
        assert_eq!(
            single_line("SELECT 1 -- n\n", MySql).unwrap(),
            "SELECT 1 /* n */"
        );
        // Off MySQL `/*!` opens an ordinary comment, closed where it ends.
        assert_eq!(
            single_line("/*! a */ SELECT 1 -- n\n", Postgres).unwrap(),
            "/*! a */ SELECT 1 /* n */"
        );
        // `GO` is only a directive on SQL Server.
        assert_eq!(single_line("SELECT 1\nGO", MySql).unwrap(), "SELECT 1 GO");
    }

    /// **PostgreSQL joins two strings into one only across a newline**, so
    /// `'foo'⏎'bar'` is `'foobar'` there and `'foo' 'bar'` is a syntax error
    /// (16.15). A line comment between them still carries the newline; a
    /// block comment breaks the continuation, and elsewhere two strings on
    /// one line mean what they did on two.
    #[test]
    fn single_line_refuses_a_postgres_string_continued_on_the_next_line() {
        use SqlDialect::{MySql, Postgres};
        for sql in [
            "select 'foo'\n'bar'",
            "select 'a' -- c\n'b'",
            "select E'a\\n'\n  'b' from t",
        ] {
            assert_eq!(single_line(sql, Postgres), None, "{sql:?}");
        }
        assert_eq!(
            single_line("select 'a' /* c */\n'b'", Postgres).unwrap(),
            "select 'a' /* c */ 'b'"
        );
        assert_eq!(
            single_line("select 'a',\n'b'", Postgres).unwrap(),
            "select 'a', 'b'"
        );
        assert_eq!(
            single_line("select 'foo'\n'bar'", MySql).unwrap(),
            "select 'foo' 'bar'"
        );
        // And *Make single line* leaves the entry out.
        assert_eq!(
            single_line_in_place("select 'foo'\n'bar';", (0, 0), 0, Postgres),
            None
        );
    }

    #[test]
    fn single_line_copy_takes_the_selection_or_the_statement_at_the_click() {
        use SqlDialect::MySql;
        let doc = "SELECT 1;\nSELECT a,\n  b FROM t;\n";
        // A bare caret: the statement it is in, without its padding.
        assert_eq!(
            single_line_copy(doc, (12, 12), 12, MySql).unwrap(),
            "SELECT a, b FROM t;"
        );
        // A selection: exactly that, wherever the click was.
        assert_eq!(
            single_line_copy(doc, (10, 23), 2, MySql).unwrap(),
            "SELECT a, b"
        );
        // A selection starting inside a string is no fragment to lex.
        assert_eq!(single_line_copy("SELECT 'a\nb' x", (9, 14), 0, MySql), None);
        assert_eq!(single_line_copy("  \n  ", (1, 1), 1, MySql), None);
    }

    /// Splice an edit's answer back into its document.
    fn splice(doc: &str, (lo, hi, text): (usize, usize, String)) -> String {
        format!("{}{text}{}", &doc[..lo], &doc[hi..])
    }

    /// *Make single line* rewrites the statement at the caret, or the
    /// selection, in place — and nothing around it: the line breaks either
    /// side of it are the neighbours'.
    #[test]
    fn single_line_in_place_rewrites_only_the_statement_or_selection() {
        use SqlDialect::MySql;
        let doc = "SELECT 1;\nSELECT a,\n  b -- n\nFROM t;\nSELECT 3;\n";
        let edit = single_line_in_place(doc, (12, 12), 12, MySql).unwrap();
        assert_eq!(&doc[edit.0..edit.1], "SELECT a,\n  b -- n\nFROM t;");
        assert_eq!(
            splice(doc, edit),
            "SELECT 1;\nSELECT a, b /* n */ FROM t;\nSELECT 3;\n"
        );
        // A selection: exactly that, its outer whitespace left where it was.
        let edit = single_line_in_place(doc, (17, 28), 0, MySql).unwrap();
        assert_eq!(&doc[edit.0..edit.1], "a,\n  b -- n");
        assert_eq!(edit.2, "a, b /* n */");
    }

    /// Nothing to do is no entry: a statement already on one line, blank
    /// text, a selection starting in a string, and what [`single_line`]
    /// refuses.
    #[test]
    fn single_line_in_place_declines_what_it_would_not_change() {
        use SqlDialect::{MsSql, MySql};
        assert_eq!(
            single_line_in_place("SELECT 1;\nSELECT 2;", (3, 3), 3, MySql),
            None
        );
        assert_eq!(single_line_in_place("  \n  ", (1, 1), 1, MySql), None);
        assert_eq!(
            single_line_in_place("SELECT 'a\nb' x", (9, 14), 0, MySql),
            None
        );
        assert_eq!(
            single_line_in_place("SELECT\n1\nGO\nSELECT 2", (0, 18), 0, MsSql),
            None
        );
    }

    /// *Format statement* re-flows the statement at the caret and leaves the
    /// others as they were, unformatted or not.
    #[test]
    fn format_statement_formats_only_the_statement_at_the_caret() {
        use SqlDialect::MySql;
        let doc = "select a,b from t;\nselect c,d from u where x=1;\nselect e,f from v;\n";
        let keep = KeywordCase::Keep;
        let edit = format_statement(doc, 25, IND, MySql, keep).unwrap();
        assert_eq!(&doc[edit.0..edit.1], "select c,d from u where x=1;");
        assert_eq!(edit.2, format_sql("select c,d from u where x=1;", IND));
        let out = splice(doc, edit);
        assert!(out.starts_with("select a,b from t;\n"), "{out}");
        assert!(out.ends_with("\nselect e,f from v;\n"), "{out}");
        // Formatted already, or no statement at all: nothing to write.
        let formatted = format_sql("select c from u", IND);
        assert_eq!(format_statement(&formatted, 2, IND, MySql, keep), None);
        assert_eq!(format_statement("  \n ", 1, IND, MySql, keep), None);
        assert_eq!(format_statement("", 0, IND, MySql, keep), None);
    }

    /// **A statement under a MySQL `DELIMITER` keeps its terminator whole.**
    /// The fragment *Format statement* and a selection hand the formatter
    /// holds the `//` but not the `DELIMITER` line that made it one, so it
    /// lexed as two `/`s and came back `END / /` — no terminator at all, and
    /// the client ran the routine on through `DELIMITER ;` and what follows.
    #[test]
    fn a_fragment_under_a_delimiter_keeps_its_terminator() {
        use SqlDialect::MySql;
        let keep = KeywordCase::Keep;
        for (d, end) in [("//", "END//"), (";;", "END;;"), ("$$", "END$$")] {
            let doc = format!(
                "DELIMITER {d}\nCREATE PROCEDURE p()\nBEGIN\nSELECT 1;\nEND{d}\nDELIMITER ;\n"
            );
            let at = doc.find("BEGIN").unwrap();
            let (lo, hi, text) = format_statement(&doc, at, IND, MySql, keep).unwrap();
            assert!(text.ends_with(end), "{d}: {text}");
            let out = splice(&doc, (lo, hi, text));
            assert_eq!(
                crate::sql::executable_statements(&out, MySql).len(),
                crate::sql::executable_statements(&doc, MySql).len(),
                "{out}"
            );
            // A selection over the routine and its terminator, likewise.
            let lo = doc.find("CREATE").unwrap();
            let hi = doc.find(end).unwrap() + end.len();
            let text = format_range(&doc, lo, hi, IND, MySql, KeywordCase::Upper);
            assert!(text.ends_with(end), "{d}: {text}");
        }
    }

    /// A caret before the rewritten range stays, one after it moves by the
    /// change in length, and one inside keeps its place among the tokens.
    #[test]
    fn caret_after_replace_keeps_its_place_on_every_side_of_the_range() {
        let old = "x;\nselect a,\n  b;\ny";
        let (lo, hi) = (3, 17);
        assert_eq!(&old[lo..hi], "select a,\n  b;");
        let new = "select a, b;";
        assert_eq!(caret_after_replace(old, lo, hi, 1, new), 1);
        // On `y`: two bytes of whitespace were taken out before it.
        assert_eq!(caret_after_replace(old, lo, hi, 18, new), 16);
        assert_eq!(&"x;\nselect a, b;\ny"[16..], "y");
        // Just after `b`: still just after it.
        let at = caret_after_replace(old, lo, hi, 16, new);
        assert_eq!(&format!("x;\n{new}\ny")[..at], "x;\nselect a, b");
    }

    /// An array constructor's or a subscript's brackets sit tight, a slice's
    /// `:` binds both bounds, and a comma inside them is an element list's —
    /// never a select list's line break. `ARRAY[1,2]` came out
    /// `array [ 1,⏎  2 ]`.
    #[test]
    fn brackets_are_neither_spaced_nor_broken() {
        let pg = |s: &str| super::format_sql(s, IND, SqlDialect::Postgres);
        let got = pg("select array[1,2], a[ 1 ], m[1][2:3] from t");
        assert_eq!(
            got,
            "select\n  array[1, 2],\n  a[1],\n  m[1][2:3]\nfrom\n  t"
        );
        assert_eq!(pg(&got), got, "a second format changes nothing");
        assert_eq!(
            pg("select 1 from t where x = any(array[1,2]) and y = 1"),
            "select\n  1\nfrom\n  t\nwhere\n  x = any (array[1, 2])\n  and y = 1"
        );
    }

    /// **A trailing newline is part of the file, and Format Code now reaches
    /// files.** Trimming it made a reformat also produce
    /// `\ No newline at end of file` in the diff — a change to a line nobody
    /// touched. Asserted as a *property* over the corpus the other tests use,
    /// since the failure is one character and easy to reintroduce.
    #[test]
    fn a_trailing_newline_survives_exactly_as_it_arrived() {
        for src in [
            "select a, b from t where a=1",
            "SELECT 1;",
            "-- just a comment",
            "insert into t (a) values (1), (2);",
            "select * from t /* block */ where a in (1,2)",
            "",
        ] {
            for suffix in ["", "\n", "\n\n", "  \n"] {
                let input = format!("{src}{suffix}");
                let got = format_sql(&input, IND);
                assert_eq!(
                    got.ends_with('\n'),
                    input.ends_with('\n'),
                    "{input:?} → {got:?}"
                );
                // And never more than one, whatever the input collected.
                assert!(!got.ends_with("\n\n"), "{input:?} → {got:?}");
            }
        }
    }

    /// **A comment that trailed its line's code stays on that line.** After a
    /// list comma it moved down onto the next item's line — `a, -- first`
    /// came out over `b`, reading as `b`'s. A comment on a line of its own
    /// keeps one.
    #[test]
    fn a_trailing_comment_stays_with_the_code_it_followed() {
        assert_eq!(
            format_sql("select a, -- first\n  b from t", IND),
            "select\n  a, -- first\n  b\nfrom\n  t"
        );
        assert_eq!(
            format_sql("select -- columns\n a from t", IND),
            "select -- columns\n  a\nfrom\n  t"
        );
        // After a `;`, on the statement it ended — not atop the next one.
        assert_eq!(
            format_sql("select 1; -- done\nselect 2", IND),
            "select\n  1; -- done\n\nselect\n  2"
        );
        let own = "select a,\n  -- about b\n  b from t";
        assert_eq!(
            format_sql(own, IND),
            "select\n  a,\n  -- about b\n  b\nfrom\n  t"
        );
        for sql in ["select a, -- first\n  b from t", own] {
            let once = format_sql(sql, IND);
            assert_eq!(format_sql(&once, IND), once, "idempotent: {sql}");
        }
    }

    /// A `CASE` is laid out: each `WHEN` and the `ELSE` one level under the
    /// line it opens on, `END` back at that line's level. Inside a function's
    /// parens it stays on its line, and nested ones nest.
    #[test]
    fn a_case_lays_out_its_branches() {
        assert_eq!(
            format_sql(
                "select case when a = 1 then 'one' when a = 2 then 'two' else 'many' end as label, b from t",
                IND
            ),
            "select\n  case\n    when a = 1 then 'one'\n    when a = 2 then 'two'\n    else 'many'\n  end as label,\n  b\nfrom\n  t"
        );
        assert_eq!(
            format_sql("select coalesce(case when a then 1 end, 0) from t", IND),
            "select\n  coalesce(case when a then 1 end, 0)\nfrom\n  t"
        );
        assert_eq!(
            format_sql(
                "select case when a then case when b then 1 else 2 end end from t",
                IND
            ),
            "select\n  case\n    when a then case\n      when b then 1\n      else 2\n    end\n  end\nfrom\n  t"
        );
        // A routine's `END CASE` closes with its own word.
        let routine = format_sql("case x when 1 then set y = 1; end case;", IND);
        assert!(routine.contains("end case;"), "{routine}");
        for sql in [
            "select case when a = 1 then 'one' else 'many' end as label, b from t",
            "select case when a then case when b then 1 else 2 end end from t",
        ] {
            let once = format_sql(sql, IND);
            assert_eq!(format_sql(&once, IND), once, "idempotent: {sql}");
        }
    }

    /// A `BEGIN … END` body is indented, its statements one level in with no
    /// blank line between them, and `END` back at its `BEGIN`'s level; T-SQL's
    /// `BEGIN TRY` / `END CATCH` keep their word; a transaction's `BEGIN`
    /// opens nothing.
    #[test]
    fn a_begin_end_body_is_indented() {
        let ms = |s: &str| super::format_sql(s, IND, SqlDialect::MsSql);
        assert_eq!(
            ms("create procedure p as begin select 1; select a from t; end;"),
            "create procedure p as begin\n  select\n    1;\n  select\n    a\n  from\n    t;\nend;"
        );
        assert_eq!(
            ms("begin try select 1; end try begin catch select 2; end catch"),
            "begin try\n  select\n    1;\nend try\nbegin catch\n  select\n    2;\nend catch"
        );
        let tx = ms("begin transaction; select 1; commit;");
        assert!(tx.contains("\nselect\n  1;"), "{tx}");
        let my = super::format_sql("begin; select 1; commit;", IND, SqlDialect::MySql);
        assert!(my.contains("\nselect\n  1;"), "{my}");
        for s in [
            "create procedure p as begin select 1; select a from t; end;",
            "begin try select 1; end try begin catch select 2; end catch",
        ] {
            assert_eq!(ms(&ms(s)), ms(s), "idempotent: {s}");
        }
    }

    /// MySQL's compound statements indent their bodies as `BEGIN` does: an
    /// `IF`'s branches under `THEN`, `ELSEIF` and `ELSE`, a `WHILE`'s under
    /// `DO`, a labelled `LOOP`'s and a `REPEAT`'s — whose `UNTIL` ends it on
    /// its `END REPEAT`'s line — with each `END` back at its opener's level.
    #[test]
    fn a_compound_statement_body_is_indented() {
        let my = |s: &str| super::format_sql(s, IND, SqlDialect::MySql);
        let routine = "create procedure p() begin declare i int default 0; \
             if i > 1 then set i = 2; elseif i = 0 then select 1; else select a from t; end if; \
             while i < 3 do set i = i + 1; end while; \
             l: loop leave l; end loop l; \
             repeat set i = i + 1; until i > 5 end repeat; end";
        assert_eq!(
            my(routine),
            "create procedure p()\nbegin\n  declare i int default 0;\n  if i > 1 then\n    set\n      i = 2;\n  elseif i = 0 then\n    select\n      1;\n  else\n    select\n      a\n    from\n      t;\n  end if;\n  while i < 3 do\n    set\n      i = i + 1;\n  end while;\n  l: loop\n    leave l;\n  end loop l;\n  repeat\n    set\n      i = i + 1;\n  until i > 5 end repeat;\nend"
        );
        assert_eq!(my(&my(routine)), my(routine), "idempotent");
        // `IF (…) THEN` is the statement; `IF(…)` a function, and an
        // `END CASE` closes no `BEGIN`.
        assert_eq!(
            my("if (a > 1) then select 1; end if;"),
            "if (a > 1) then\n  select\n    1;\nend if;"
        );
        assert_eq!(my("select if(a, 1, 2)"), "select\n  if(a, 1, 2)");
        let case = my("begin case x when 1 then select 1; end case; select 2; end");
        assert!(case.ends_with("\n  select\n    2;\nend"), "{case}");
        // T-SQL's IF has no END IF and opens nothing.
        let ms = super::format_sql("if @a = 1 select 1; select 2;", IND, SqlDialect::MsSql);
        assert!(ms.contains("\n\nselect\n  2;"), "{ms}");
    }

    /// A MySQL `CASE` **statement** lays its branches out as `IF` does: each
    /// `WHEN … THEN` and the `ELSE` one level in, their statements one more,
    /// and `END CASE` back at the `CASE`. The `;` ending a branch's statement
    /// used to clear the frame, and every branch came out flat.
    #[test]
    fn a_case_statement_lays_out_its_branches() {
        let my = |s: &str| super::format_sql(s, IND, SqlDialect::MySql);
        let routine = "begin case x when 1 then set y = 1; when 2 then select 2; \
             else set y = 3; end case; select 4; end";
        assert_eq!(
            my(routine),
            "begin\n  case x\n    when 1 then\n      set\n        y = 1;\n    when 2 then\n      \
             select\n        2;\n    else\n      set\n        y = 3;\n  end case;\n  select\n    \
             4;\nend"
        );
        assert_eq!(my(&my(routine)), my(routine), "idempotent");
        // A searched CASE, and a CASE expression inside a branch, are as before.
        let searched = my("case when a > 1 then select case when b then 1 else 2 end; end case;");
        assert_eq!(
            searched,
            "case\n  when a > 1 then\n    select\n      case\n        when b then 1\n        else 2\n      \
             end;\nend case;"
        );
    }

    /// T-SQL's `TOP (n)` keeps its space, and a write's `OUTPUT` clause takes
    /// a line of its own — but not a procedure parameter's `OUTPUT`, nor a
    /// column of that name, nor anywhere but SQL Server.
    #[test]
    fn top_keeps_its_space_and_output_is_a_clause() {
        let ms = |s: &str| super::format_sql(s, IND, SqlDialect::MsSql);
        assert_eq!(
            ms("select top (5) a from t"),
            "select\n  top (5) a\nfrom\n  t"
        );
        assert_eq!(
            ms("update t set n = 1 output inserted.id where id = 2"),
            "update\n  t\nset\n  n = 1\noutput\n  inserted.id\nwhere\n  id = 2"
        );
        assert_eq!(
            ms("delete from t output deleted.id, deleted.name where id = 1"),
            "delete from\n  t\noutput\n  deleted.id,\n  deleted.name\nwhere\n  id = 1"
        );
        let proc = ms("create procedure p @x int output as select 1");
        assert!(proc.contains("@x int output"), "{proc}");
        let col = ms("update t set output = 1");
        assert!(col.contains("output = 1"), "{col}");
        let my = super::format_sql("update t set a = 1 output x", IND, SqlDialect::MySql);
        assert!(!my.contains("\noutput"), "{my}");
        for s in [
            "select top (5) a from t",
            "update t set n = 1 output inserted.id where id = 2",
        ] {
            assert_eq!(ms(&ms(s)), ms(s), "idempotent: {s}");
        }
    }

    /// `WITH RECURSIVE` is one head, `WINDOW` is a clause of its own, and
    /// MySQL's `LIMIT offset, count` is one clause's two numbers, not a list.
    #[test]
    fn recursive_window_and_limit_lay_out_as_clauses() {
        let got = format_sql("with recursive r as (select 1) select * from r", IND);
        // The head's layout, as a plain `WITH` gets: `with⏎  r as (`.
        assert!(got.starts_with("with recursive\n  r as (\n"), "{got}");
        assert_eq!(
            format_sql("select a from t window w as (partition by b)", IND),
            "select\n  a\nfrom\n  t\nwindow\n  w as (partition by b)"
        );
        assert_eq!(
            format_sql("select a from t limit 10, 20", IND),
            "select\n  a\nfrom\n  t\nlimit\n  10, 20"
        );
        // A select list after the LIMIT's statement still breaks.
        assert_eq!(
            format_sql("select a from t limit 1, 2; select b, c from u", IND),
            "select\n  a\nfrom\n  t\nlimit\n  1, 2;\n\nselect\n  b,\n  c\nfrom\n  u"
        );
        for sql in [
            "with recursive r as (select 1) select * from r",
            "select a from t window w as (partition by b)",
            "select a from t limit 10, 20",
        ] {
            let once = format_sql(sql, IND);
            assert_eq!(format_sql(&once, IND), once, "idempotent: {sql}");
        }
    }

    #[test]
    fn simple_select_preserves_case() {
        let got = format_sql("select a, b from t where a=1 and b=2", IND);
        assert_eq!(
            got,
            "select\n  a,\n  b\nfrom\n  t\nwhere\n  a = 1\n  and b = 2"
        );
    }

    #[test]
    fn joins_break_but_modifiers_stay_inline() {
        let got = format_sql(
            "SELECT a FROM x JOIN y ON x.id = y.id LEFT OUTER JOIN z ON z.k = x.k",
            IND,
        );
        assert_eq!(
            got,
            "SELECT\n  a\nFROM\n  x\n  JOIN y ON x.id = y.id\n  LEFT OUTER JOIN z ON z.k = x.k"
        );
    }

    #[test]
    fn subquery_indents_one_level_per_paren() {
        let got = format_sql("SELECT a FROM (SELECT b FROM t) x", IND);
        assert_eq!(
            got,
            "SELECT\n  a\nFROM\n  (\n    SELECT\n      b\n    FROM\n      t\n  ) x"
        );
    }

    #[test]
    fn function_calls_and_in_lists_stay_inline() {
        let got = format_sql("SELECT count(*), max(a) FROM t WHERE a IN (1, 2, 3)", IND);
        assert_eq!(
            got,
            "SELECT\n  count(*),\n  max(a)\nFROM\n  t\nWHERE\n  a IN (1, 2, 3)"
        );
    }

    #[test]
    fn hash_and_dash_comments_and_backticks_preserved() {
        let got = format_sql("select `from`, 'a;b' # note\nfrom t", IND);
        assert_eq!(got, "select\n  `from`,\n  'a;b' # note\nfrom\n  t");
    }

    #[test]
    fn multiple_statements_split_with_blank_line() {
        let got = format_sql("select 1; select 2", IND);
        assert_eq!(got, "select\n  1;\n\nselect\n  2");
    }

    #[test]
    fn between_and_stays_inline() {
        let got = format_sql("SELECT a FROM t WHERE a BETWEEN 1 AND 10 AND b = 2", IND);
        assert_eq!(
            got,
            "SELECT\n  a\nFROM\n  t\nWHERE\n  a BETWEEN 1 AND 10\n  AND b = 2"
        );
    }

    #[test]
    fn group_and_order_by_attach_and_break_list() {
        let got = format_sql("SELECT a, b FROM t GROUP BY a, b ORDER BY a DESC", IND);
        assert_eq!(
            got,
            "SELECT\n  a,\n  b\nFROM\n  t\nGROUP BY\n  a,\n  b\nORDER BY\n  a DESC"
        );
    }

    #[test]
    fn negative_numbers_stay_tight() {
        let got = format_sql("SELECT a FROM t WHERE a = -1", IND);
        assert_eq!(got, "SELECT\n  a\nFROM\n  t\nWHERE\n  a = -1");
    }

    /// **Format Code must not invent a query parameter, and it did — by moving
    /// a space.**
    ///
    /// `e10abee` made `:name` one token so a reformat stopped emptying the
    /// parameters bar. It copied half of `params`' rule: `params` also declines
    /// a `:` that follows an identifier byte, a `]`, or sits in an open
    /// subscript, so `a[lo:hi]` carries no placeholder. To `tokenize` it was one,
    /// `need_space` put a space in front of it, and at `a [ lo :hi ]` `params`
    /// agrees — the bar gains `hi`, and the run is held until a name the user
    /// never wrote is bound. The commit's own test asserts `!out.contains(": ")`
    /// and is blind to the ` :` spelling this produces.
    ///
    /// The property, not the spelling: reformatting changes no token's text, so
    /// it changes no statement's parameter list.
    #[test]
    fn formatting_never_changes_the_parameter_list() {
        for sql in [
            "SELECT a[lo:hi] FROM t WHERE x = :p",
            "SELECT a[1:n] FROM t",
            "SELECT arr[i:j] FROM t",
            "SELECT a[:hi] FROM t",
            "SELECT m[1][2:3] FROM t",
            "SELECT * FROM t WHERE id = :id",
            "SELECT ARRAY[:a, :b]",
            "my_loop:LOOP SELECT 1; END LOOP",
        ] {
            for dialect in SqlDialect::ALL {
                let before = crate::params::names(sql, dialect);
                let after = crate::params::names(&super::format_sql(sql, IND, dialect), dialect);
                assert_eq!(before, after, "{dialect:?} {sql:?}");
            }
        }
    }

    /// **An executable comment is one token to the formatter, `*/` included.**
    ///
    /// `skip_comment` ends a `/*! … */` span just past the *marker*, so the
    /// security gates see the body as the code the server runs. `tokenize` is a
    /// fourth caller of that function and took the same answer: the body became
    /// ordinary tokens and the trailing `*/` fell out as two one-byte puncts,
    /// which `need_space` then separated. Format Code turned
    /// `/*!40101 SET NAMES utf8 */;` into an **unterminated comment**,
    /// swallowing that statement and everything after it in the file — and
    /// idempotently, so a second Format did not repair it.
    #[test]
    fn an_executable_comment_survives_formatting_whole() {
        for sql in [
            "/*!40101 SET NAMES utf8 */;",
            "SELECT /*!40001 SQL_NO_CACHE */ * FROM t;",
            "INSERT /*!IGNORE*/ INTO t VALUES (1);",
            "/*!40000 ALTER TABLE `orders` DISABLE KEYS */;",
            "/*M!100000 SET @x = 1 */;",
        ] {
            for dialect in SqlDialect::ALL {
                let got = super::format_sql(sql, IND, dialect);
                assert!(!got.contains("* /"), "{dialect:?} {sql:?} -> {got:?}");
                assert_eq!(
                    got.matches("*/").count(),
                    sql.matches("*/").count(),
                    "{dialect:?} {sql:?} -> {got:?}"
                );
            }
        }
        // The run is reproduced verbatim on the engines that execute it — the
        // formatter's contract is that a token's text is preserved.
        let got = super::format_sql("/*!40101 SET NAMES utf8 */;", IND, SqlDialect::MySql);
        assert!(got.contains("/*!40101 SET NAMES utf8 */"), "{got:?}");
        // A statement count round trip over a dump preamble: reformatting must
        // not change how many statements the boundary lexer sees.
        let preamble = "/*!40101 SET @OLD_CHARSET=@@CHARACTER_SET_CLIENT */;\n\
                        /*!40101 SET NAMES utf8mb4 */;\n\
                        SELECT 1;\n";
        let before = crate::sql::executable_statements(preamble, SqlDialect::MySql).len();
        let after = crate::sql::executable_statements(
            &super::format_sql(preamble, IND, SqlDialect::MySql),
            SqlDialect::MySql,
        )
        .len();
        assert_eq!(before, after, "statement count changed");
        // A stray `*/` with no opener — what a selection ending past the close
        // of an executable comment hands the formatter — stays one token too.
        let got = super::format_sql("SET NAMES utf8 */", IND, SqlDialect::MySql);
        assert!(got.contains("*/"), "{got:?}");
    }

    /// **A selection whose ends are not in code is not formattable.**
    ///
    /// `format_sql` lexes what it is handed from byte 0 and takes that for a
    /// token boundary. Selecting `keep a, b` inside `select 1 -- keep a, b` and
    /// pressing Format Code lexed the fragment as words and a comma, broke the
    /// comma, and left the document `select 1 -- keep a,\nb\nfrom t` — `b` now
    /// outside the comment, and the statement no longer parses.
    #[test]
    fn a_selection_inside_a_comment_or_a_string_is_not_formattable() {
        let full = "select 1 -- keep a, b\nfrom t";
        let inside = full.find("keep").unwrap();
        for dialect in SqlDialect::ALL {
            assert_eq!(
                super::formattable_range(full, inside, inside + "keep a, b".len(), dialect),
                None,
                "{dialect:?}"
            );
            // A string literal is the same question.
            let s = "select 'keep a, b' from t";
            let at = s.find("keep").unwrap();
            assert_eq!(
                super::formattable_range(s, at, at + 4, dialect),
                None,
                "{dialect:?}"
            );
            // An empty selection is the whole document, which is what Format
            // Code has always done with a bare caret.
            assert_eq!(
                super::formattable_range(full, 3, 3, dialect),
                Some((0, full.len())),
                "{dialect:?}"
            );
            // And an ordinary selection in code is still formattable, either
            // way round, including one that *contains* the comment.
            assert_eq!(
                super::formattable_range(full, 0, 8, dialect),
                Some((0, 8)),
                "{dialect:?}"
            );
            assert_eq!(
                super::formattable_range(full, 8, 0, dialect),
                Some((0, 8)),
                "{dialect:?}"
            );
            assert_eq!(
                super::formattable_range(full, 0, full.len(), dialect),
                Some((0, full.len())),
                "{dialect:?}"
            );
        }
    }

    /// **Two unary signs must not fuse into a comment opener.** Both are unary
    /// — the first because the previous token is `*` or `=`, the second because
    /// `unary_context` lists `-` itself — so the tight-next rule butted them
    /// together into `--1`, which on PostgreSQL and SQLite opens a line comment
    /// with no whitespace required. Measured on PG 16.15: `select 1 * --1` →
    /// *syntax error at end of input*. The formatter had rewritten the user's
    /// own buffer into that.
    #[test]
    fn two_unary_signs_do_not_fuse_into_a_line_comment() {
        for dialect in [SqlDialect::Postgres, SqlDialect::Sqlite, SqlDialect::MySql] {
            for (sql, want) in [
                ("select 1 * - -1", "select\n  1 * - -1"),
                (
                    "select a from t where b = - -1",
                    "select\n  a\nfrom\n  t\nwhere\n  b = - -1",
                ),
            ] {
                assert_eq!(
                    super::format_sql(sql, IND, dialect),
                    want,
                    "{sql} on {dialect:?}"
                );
            }
            // A single unary sign is still tight, which is the case that must
            // not change.
            assert_eq!(
                super::format_sql("select a from t where a = -1", IND, dialect),
                "select\n  a\nfrom\n  t\nwhere\n  a = -1",
                "{dialect:?}"
            );
        }
    }

    /// **Where operators compose, a unary sign is not written against the
    /// operator after it.** PostgreSQL builds an operator's name out of any
    /// run of operator characters, so `- ~5` written tight is `-~ 5`, the
    /// operator `-~` — *operator does not exist: -~ integer* on 16.15, as
    /// `- @ -3` became `-@ -3` and `2 * - |/ 25` became `2 * -|/ 25`.
    #[test]
    fn a_unary_sign_does_not_fuse_into_a_composed_operator() {
        let pg = SqlDialect::Postgres;
        for (sql, fused) in [
            ("select - ~5", "-~"),
            ("select - @ -3", "-@"),
            ("select 2 * - |/ 25", "-|/"),
        ] {
            let out = super::format_sql(sql, IND, pg);
            assert!(!out.contains(fused), "{sql}: {out}");
            assert_preserves(sql, pg);
        }
        // A sign before a number is still tight.
        assert!(super::format_sql("select - ~5, -5", IND, pg).contains("-5"));
    }

    #[test]
    fn tab_indent_unit() {
        let got = format_sql("select a from t", "\t");
        assert_eq!(got, "select\n\ta\nfrom\n\tt");
    }

    #[test]
    fn idempotent() {
        let once = format_sql("select a,b from t where x=1 and y=2", IND);
        let twice = format_sql(&once, IND);
        assert_eq!(once, twice);
    }

    // ── The headline contract, and the dialects it has to hold under ──────
    //
    // Ctrl+Alt+L rewrites the user's buffer in place, so a formatter that
    // mangles a token doesn't produce ugly SQL — it produces *different* SQL,
    // and the one class of that which is dangerous (a PostgreSQL `--` read as
    // MySQL, uncommenting a statement) was a Critical in this review. These are
    // the tests that would have failed the day it was introduced.

    /// Every non-whitespace character, in order. Formatting may re-flow
    /// whitespace and nothing else, so this string is an invariant.
    ///
    /// **Kept, but it is not the contract.** It cannot see whitespace inserted
    /// *inside* a token — strip the whitespace and `id : : text` is
    /// indistinguishable from `id::text` — which is why the assertion below is a
    /// token-sequence comparison and this one only guards against a *dropped*
    /// or reordered character, which a token comparison of a mis-tokenized
    /// input would miss.
    fn tokens(s: &str) -> String {
        s.chars().filter(|c| !c.is_whitespace()).collect()
    }

    /// The module's own tokenizer over a string, as texts.
    ///
    /// This is what "without changing any token's text" means, and it is the
    /// assertion the block was missing: `formatting_preserves_every_token` was
    /// green while its own corpus line `select data #> '{a,b}' from t` came out
    /// `data # > '{a,b}'`, which PostgreSQL 16.15 rejects.
    fn token_texts(s: &str, dialect: SqlDialect) -> Vec<String> {
        super::tokenize(s, dialect, &mut None)
            .into_iter()
            .map(|(_, t)| t.to_string())
            .collect()
    }

    fn assert_preserves(sql: &str, dialect: SqlDialect) {
        let out = super::format_sql(sql, IND, dialect);
        assert_eq!(
            tokens(&out),
            tokens(sql),
            "{dialect:?} lost or moved a character: {sql}\n{out}"
        );
        assert_eq!(
            token_texts(&out, dialect),
            token_texts(sql, dialect),
            "{dialect:?} mangled a token: {sql}\n{out}"
        );
    }

    /// **Format Code panicked on ordinary SQL**, on all three dialects, on the
    /// floem UI thread.
    ///
    /// The operator table was tried with `&rest[..op.len()]` — a `&str` sliced
    /// at a byte count behind a *length* guard, which says nothing about a char
    /// boundary. The slice is taken for **every** candidate spelling before any
    /// comparison, so the panic needs no operator to match: only a punctuation
    /// byte with a multi-byte character starting within `op.len() - 1` bytes of
    /// it. MySQL and SQLite lead with `"->>"` and PostgreSQL with `"::"`, so a
    /// 2-byte character one byte after a punct byte reaches it everywhere.
    #[test]
    fn a_multi_byte_character_beside_punctuation_is_formatted_not_panicked_on() {
        for sql in [
            "create table t (\u{438}\u{43c}\u{44f} text)",
            "select a-\u{e9} from t",
            "select a.\u{e9} from t",
            "select * from t where b=\u{e9}",
            "select 1+\u{e9}",
            "select a;\u{e9}",
            "select (\u{4e2d}\u{6587}) from t",
            "select a, \u{e9} from t",
        ] {
            for d in [SqlDialect::MySql, SqlDialect::Postgres, SqlDialect::Sqlite] {
                assert_preserves(sql, d);
            }
        }
    }

    /// **A `:name` placeholder is a token of the language this editor speaks**,
    /// and the formatter split every one into `: name`.
    ///
    /// `params`' own doc owns the spelling — "a placeholder is `:` followed by
    /// an identifier" — but `tokenize` modelled `:` only as part of `::` or
    /// `:=`, so a bare one fell through as a one-byte `Punct` and `need_space`
    /// put a space after it. One Ctrl+Alt+L then emptied the parameters bar, lost
    /// every value bound to a name, and left a buffer that is a syntax error on
    /// all three engines.
    ///
    /// Asserted through `params::names` as well as through the tokens, since the
    /// bar is what the user loses — the module's `assert_preserves` compares two
    /// runs of the *same* tokenizer and is structurally blind to a token it does
    /// not model.
    #[test]
    fn a_named_parameter_survives_formatting() {
        let sql = "select * from t where id = :id and n > :n";
        for d in [SqlDialect::MySql, SqlDialect::Postgres, SqlDialect::Sqlite] {
            let out = super::format_sql(sql, IND, d);
            assert_eq!(
                crate::params::names(&out, d),
                crate::params::names(sql, d),
                "{d:?} lost the parameters bar:\n{out}"
            );
            assert!(!out.contains(": "), "{d:?} split the placeholder:\n{out}");
            assert_preserves(sql, d);
        }
        // The operators that really do start with `:` are untouched.
        assert_preserves("select a::text from t", SqlDialect::Postgres);
        assert_preserves("select @x := 1", SqlDialect::MySql);
        // A `[` in a string earlier on the line opens no subscript, so the
        // placeholder after it is still one token.
        let sql = "select * from t where note = 'see item[' and id = :id";
        let out = super::format_sql(sql, IND, SqlDialect::Postgres);
        assert_eq!(
            crate::params::names(&out, SqlDialect::Postgres),
            ["id"],
            "{out}"
        );
        assert!(!out.contains(": "), "split the placeholder:\n{out}");
    }

    #[test]
    fn formatting_preserves_every_token_on_both_dialects() {
        let shared = [
            "select a,b from t where x=1 and y=2",
            "select 'a string with  spaces and -- a fake comment' from t",
            "select /* block */ a from t /* trailing */",
            "insert into t values ('multi\nline'), ('x')",
            "select \"quoted\" from t",
            "select a from t where b in (1,2,3) order by a desc",
            // `=-` is not an operator on any of the three: without the
            // trailing-sign back-off the run rule would make it one.
            "select a from t where x=-1",
        ];
        // On every engine, SQL Server and SQLite included: the shared shapes
        // lex alike on all four.
        for sql in shared {
            for d in SqlDialect::ALL {
                assert_preserves(sql, d);
            }
        }
        // SQL Server's own: bracketed names (a `]]` inside one), `N'…'`,
        // variables and temporary tables, compound assignment, `TOP (n)`,
        // `OUTPUT`, `APPLY`, and `::` calling a static method of a type.
        for sql in [
            "select [order id], [a]]b] from [dbo].[t]",
            "select N'unicode' as [x] from t where name = N'é'",
            "declare @n int = 1; set @n += 2; select @n, @@rowcount",
            "select top (5) * from #work join ##shared s on s.id = #work.id",
            "update t set n -= 1, m *= 2 output inserted.id where id = @id",
            "select a.x from a cross apply (select top 1 b.y from b where b.a = a.id) c",
            "select geography::Point(47.6, -122.3, 4326).STAsText()",
            "select x from t where y <> 1 and z !< 2 and w !> 3",
        ] {
            assert_preserves(sql, SqlDialect::MsSql);
            let once = super::format_sql(sql, IND, SqlDialect::MsSql);
            assert_eq!(
                super::format_sql(&once, IND, SqlDialect::MsSql),
                once,
                "not idempotent: {sql}"
            );
        }
        // Dialect-only shapes, each meaningless or differently-lexed elsewhere.
        assert_preserves(
            "select `back ticked` from t # trailing\nselect 2",
            SqlDialect::MySql,
        );
        // MySQL's charset introducer, which is how its own catalogue writes a
        // string default it recorded as an expression.
        assert_preserves("select _utf8mb4'draft', X'ff' from t", SqlDialect::MySql);
        for sql in [
            "select E'esc\\'aped' from t",
            "create function f() returns int as $$ select 1; $$ language sql",
            "select data #> '{a,b}' from t",
            // A prefixed literal, whose prefix is part of the literal: the
            // spaced form is a syntax error, measured on 16.15.
            "select E'esc\\'aped', U&'\\0041', B'101', X'ff' from t",
            // The cast, which is how PostgreSQL spells every cast, and the
            // operators the old MySQL-shaped table split into single bytes.
            "select count(*)::int, created_at::date from t where x::text = 'a'",
            "select '{\"a\":1}'::jsonb #>> '{a}' from t",
            "select a from t where tags @> '{x}' and '{y}' <@ tags",
            "select a from t where name ~* 'x' and other !~* 'y'",
            "select a from t where meta ?| array['x'] and meta ?& array['y']",
            "select |/ 4.0, ||/ 27.0",
            "select a from t where r -|- s",
            // A comment that starts inside what would otherwise be an operator
            // run — the run has to stop where the server's does.
            "select a <--b\nfrom t",
        ] {
            assert_preserves(sql, SqlDialect::Postgres);
        }
        // SQLite's own two: `->>` from JSON, and `==`.
        for sql in [
            "select data ->> '$.a' from t where x == 1",
            "select a from t where b<>1 and c||d = 'x'",
        ] {
            assert_preserves(sql, SqlDialect::Sqlite);
        }
    }

    /// A number is one token, exponent and all.
    ///
    /// **Character-level on purpose.** `assert_preserves` tokenizes both sides
    /// with the tokenizer under test, so while `1.5e-3` lexed as
    /// `["1", ".", "5e", "-", "3"]` the round-trip agreed with itself and
    /// passed — the token census cannot fail on a defect *in* the census's own
    /// splitting. `1.5e` is not a number on any of the three engines, so the
    /// spaced output is a statement the server rejects.
    #[test]
    fn a_scientific_notation_number_is_not_split_at_its_sign() {
        for d in [SqlDialect::MySql, SqlDialect::Postgres, SqlDialect::Sqlite] {
            for lit in ["1.5e-3", "1e-3", "2E-10", "1.5e+3", "1.5", "1e5", "0.5"] {
                let sql = format!("select {lit} from t");
                let out = super::format_sql(&sql, IND, d);
                assert!(out.contains(lit), "{d:?} split {lit}:\n{out}");
            }
            // The sign is still binary where it really is binary.
            let out = super::format_sql("select a-1 from t", IND, d);
            assert!(out.contains("a - 1"), "{d:?}:\n{out}");
            // …and a bare `e` is still its own word.
            let out = super::format_sql("select 1 e from t", IND, d);
            assert!(out.contains("1 e"), "{d:?}:\n{out}");
        }
    }

    /// A prefixed literal comes out as it went in, because the prefix is part
    /// of the token. `E 'x\\'y'` is not ugly SQL, it is a different statement.
    #[test]
    fn a_literals_prefix_stays_attached_to_it() {
        let out = super::format_sql("select E'esc\\'aped' from t", IND, SqlDialect::Postgres);
        assert!(out.contains("E'esc\\'aped'"), "{out}");
        let out = super::format_sql("select _utf8mb4'draft' from t", IND, SqlDialect::MySql);
        assert!(out.contains("_utf8mb4'draft'"), "{out}");
        // And a space the user wrote is still a space: two tokens stay two.
        let out = super::format_sql("select E , 'x' from t", IND, SqlDialect::Postgres);
        assert!(out.contains("E,"), "{out}");
    }

    /// And the layout the fix is *for*: a cast binds to what it casts, so the
    /// output reads like SQL rather than merely parsing as it.
    #[test]
    fn a_cast_is_emitted_tight() {
        let out = super::format_sql(
            "select count(*)::int from t where x::text = 'a'",
            IND,
            SqlDialect::Postgres,
        );
        assert!(out.contains("count(*)::int"), "{out}");
        assert!(out.contains("x::text"), "{out}");
    }

    #[test]
    fn a_line_comment_keeps_the_newline_that_ends_it() {
        // Token preservation alone can't see this: strip the whitespace and a
        // swallowed newline looks identical. But losing it puts the next
        // statement *inside* the comment, which is the same class of harm as
        // reading `--` on the wrong dialect.
        for dialect in [SqlDialect::MySql, SqlDialect::Postgres] {
            let out = super::format_sql("-- note\nselect 1", IND, dialect);
            let after = out.split_once("-- note").expect("comment survived").1;
            assert!(after.starts_with('\n'), "{dialect:?}: {out:?}");
        }
        // MySQL's `#` is the same shape; on PostgreSQL it is an operator, and
        // the token-preservation test above covers that side.
        let out = super::format_sql("# note\nselect 1", IND, SqlDialect::MySql);
        let after = out.split_once("# note").expect("comment survived").1;
        assert!(after.starts_with('\n'), "{out:?}");
    }

    #[test]
    fn a_postgres_dash_comment_is_never_treated_as_code() {
        // The regression guard for the Critical: `-- ` hides the rest of its
        // line on *both* engines. If the formatter ever re-flows a PG statement
        // as if `--` weren't a comment, the `select 2` here moves onto the
        // comment's line and silently becomes part of it.
        let out = super::format_sql("select 1 -- hidden\nselect 2", IND, SqlDialect::Postgres);
        let line = out
            .lines()
            .find(|l| l.contains("-- hidden"))
            .expect("comment survived");
        assert!(!line.contains("select 2"), "{out:?}");
    }

    /// **A `GO` line is the client's batch separator and keeps a line of its
    /// own.** Tokenized as a word it was laid out as one — `1 GO` on the
    /// `SELECT`'s line — and the script lost every batch it had: the next
    /// `CREATE PROCEDURE` swallowed whatever followed it.
    #[test]
    fn a_sql_server_go_line_survives_formatting() {
        let d = SqlDialect::MsSql;
        let sql = "SELECT 1\nGO\nCREATE PROCEDURE p AS SELECT 2\ngo 3 -- again\nSELECT 3\n";
        let out = super::format_sql(sql, IND, d);
        assert_eq!(
            crate::sql::executable_statements(&out, d),
            crate::sql::executable_statements(sql, d)
                .iter()
                .map(|s| super::format_sql(s, IND, d))
                .collect::<Vec<_>>(),
            "{out}"
        );
        assert!(out.lines().any(|l| l == "GO"), "{out}");
        assert!(out.lines().any(|l| l == "go 3 -- again"), "{out}");
        // Elsewhere `go` is only a word.
        let mysql = super::format_sql("SELECT 1\nGO\n", IND, SqlDialect::MySql);
        assert!(!mysql.lines().any(|l| l == "GO"), "{mysql}");
    }

    /// **A MySQL `DELIMITER` line is the client's, and passes through verbatim
    /// on a line of its own.** Read as SQL, `DELIMITER $$` came out
    /// `DELIMITER $ $` joined to the `CREATE` after it, `END$$` and the
    /// `DELIMITER ;` line were run together, and `//` was split into `/ /` —
    /// a script no client could split any more. The terminator it sets ends a
    /// statement the way `;` does.
    #[test]
    fn a_mysql_delimiter_line_survives_formatting() {
        let d = SqlDialect::MySql;
        for (sql, lines) in [
            (
                "DELIMITER $$\nCREATE PROCEDURE p() BEGIN SELECT 1; END$$\nDELIMITER ;\nSELECT 2;\n",
                &["DELIMITER $$", "DELIMITER ;"][..],
            ),
            (
                "delimiter //\ncreate trigger tr before insert on t for each row \
                 begin set new.a=1; end //\nDELIMITER ;\nselect 1;",
                &["delimiter //", "DELIMITER ;"],
            ),
            (
                "SELECT 1;\n-- routines\nDELIMITER ;;\nCREATE FUNCTION f() RETURNS INT RETURN 1;;\n\
                 CREATE FUNCTION g() RETURNS INT RETURN 2;;\nDELIMITER ;\n",
                &["DELIMITER ;;", "DELIMITER ;"],
            ),
        ] {
            let out = super::format_sql(sql, IND, d);
            for l in lines {
                assert!(
                    out.lines().any(|x| x == *l),
                    "{l:?} not on its own line:\n{out}"
                );
            }
            assert!(!out.contains("/ /") && !out.contains("$ $"), "{out}");
            assert_eq!(
                crate::sql::executable_statements(&out, d),
                crate::sql::executable_statements(sql, d)
                    .iter()
                    .map(|s| super::format_sql(s, IND, d))
                    .collect::<Vec<_>>(),
                "{out}"
            );
            assert_eq!(super::format_sql(&out, IND, d), out, "not idempotent");
        }
        // Mid-statement, and on other engines, the word is only a word.
        let out = super::format_sql("SELECT delimiter FROM t", IND, d);
        assert!(out.contains("delimiter"), "{out}");
        let pg = super::format_sql("DELIMITER $$\nSELECT 1;", IND, SqlDialect::Postgres);
        assert!(!pg.lines().any(|l| l == "DELIMITER"), "{pg}");
    }

    /// **A T-SQL variable, system function or temporary table is one token.**
    /// `@` and `#` fell to the operator arm, `need_space` separated them from
    /// the name, and the formatted batch no longer compiled: `SELECT @ x` is
    /// Msg 137, `SELECT @ @ VERSION` and `CREATE TABLE # t` are Msg 102
    /// (measured on 2022). A name that *holds* one of these bytes — `h#`,
    /// `f@x` — is one word too.
    #[test]
    fn a_t_sql_variable_or_temp_table_keeps_its_prefix() {
        let d = SqlDialect::MsSql;
        let sql = "select @x = @@ROWCOUNT, #t.a, ##g.b, dbo.h#(1), f@x from #t join ##g on 1=1; \
                   DECLARE @s nvarchar(max) = N'x''y'; EXEC sp_executesql @s";
        let out = super::format_sql(sql, IND, d);
        for kept in [
            "@x",
            "@@ROWCOUNT",
            "#t.a",
            "##g.b",
            "h#(1)",
            "f@x",
            "#t",
            "##g",
            "@s",
        ] {
            assert!(out.contains(kept), "{kept} split: {out}");
        }
        for split in ["@ ", "# "] {
            assert!(!out.contains(split), "{split:?} in {out}");
        }
        assert_preserves(sql, d);
        assert_eq!(crate::params::names(&out, d), crate::params::names(sql, d));
    }

    /// **An engine's own parameter or variable is one token.** The formatter
    /// knew `:name` and T-SQL's `@x`, and split every other sigil from its
    /// name: PostgreSQL's `$1` came out `$ 1`, SQLite's `?1`, `$p` and `@p`
    /// came out `? 1`, `$ p` and `@ p`, and MySQL's `@x`, `@@version` and the
    /// account `'u'@'h'` came out `@ x`, `@ @ version` and `'u' @ 'h'` — each
    /// a syntax error (MySQL 8 and MariaDB 10.11 measured) where the input
    /// ran. MySQL's `$p` is a name too.
    #[test]
    fn a_native_parameter_or_variable_keeps_its_sigil() {
        for (d, sql, kept) in [
            (
                SqlDialect::Postgres,
                "select * from t where a = $1 and b = $2::int",
                &["$1", "$2::int"][..],
            ),
            (
                SqlDialect::Sqlite,
                "select * from t where a = ?1 and b = $p and c = @q and d = :r and e = ?",
                &["?1", "$p", "@q", ":r"],
            ),
            (
                SqlDialect::MySql,
                "set @x := 1; select @x, @@version, @@session.sql_mode, $p from t \
                 where u = 'a'@'h' and v = @'q v'",
                &[
                    "@x",
                    "@@version",
                    "@@session.sql_mode",
                    "$p",
                    "@'h'",
                    "@'q v'",
                ],
            ),
            (
                SqlDialect::MsSql,
                "select $1.50, @x, @@ROWCOUNT, $action from #t",
                &["$1.50", "@x", "@@ROWCOUNT", "$action", "#t"],
            ),
        ] {
            let out = super::format_sql(sql, IND, d);
            for k in kept {
                assert!(out.contains(k), "{d:?}: {k} split: {out}");
            }
            for split in ["$ ", "@ ", "? 1"] {
                assert!(!out.contains(split), "{d:?}: {split:?} in {out}");
            }
            assert_preserves(sql, d);
        }
        // PostgreSQL's `@` is an operator, and stays one.
        assert_eq!(
            token_texts("select @ -5, @x from t", SqlDialect::Postgres),
            ["select", "@", "-", "5", ",", "@", "x", "from", "t"]
        );
    }

    /// **A `$` inside a name is part of it on every engine.** MySQL's bare
    /// identifier is `[0-9a-zA-Z$_]` and SQLite's tokenizer continues one
    /// through `$`, but `continues_name` stopped both at it, so Format Code
    /// wrote `a $ b` — a syntax error on both, from a valid `a$b`.
    #[test]
    fn a_dollar_inside_a_name_keeps_the_name_whole() {
        for d in [
            SqlDialect::MySql,
            SqlDialect::Sqlite,
            SqlDialect::Postgres,
            SqlDialect::MsSql,
        ] {
            let sql = "select a$b, c from t$x where a$b = 1";
            let out = super::format_sql(sql, IND, d);
            for kept in ["a$b", "t$x"] {
                assert!(out.contains(kept), "{d:?}: {kept} split: {out}");
            }
            assert!(!out.contains(" $ "), "{d:?}: {out}");
            assert_preserves(sql, d);
        }
    }

    /// **A word after `.` is a name, never a clause.** `t.order` came out
    /// `t.⏎order`, and MySQL reads a keyword as a name only straight after the
    /// `.`: with a newline between them `order` is `ORDER` again, and the
    /// statement a syntax error.
    #[test]
    fn a_word_after_a_dot_is_laid_out_as_a_name() {
        let sql = "select t.order, p.left, t.set, x.values, y.and from t where t.from = 1";
        for d in SqlDialect::ALL {
            let out = super::format_sql(sql, IND, d);
            for kept in [
                "t.order",
                "p.left",
                "t.set",
                "x.values",
                "y.and",
                "t.from = 1",
            ] {
                assert!(out.contains(kept), "{d:?}: {kept} split: {out}");
            }
            assert_preserves(sql, d);
        }
    }

    /// **A clause keyword that is also a function is a call when `(` follows
    /// it**: `LEFT(`, `RIGHT(`, `REPLACE(` and MySQL's `INSERT(` are everyday
    /// string functions, and laid out as clauses they broke the select list.
    #[test]
    fn a_keyword_called_as_a_function_stays_a_call() {
        assert_eq!(
            format_sql(
                "select left(name,3), replace(a,'x','y'), right(b, 2), insert(s, 1, 2, 'x') from t",
                IND
            ),
            "select\n  left(name, 3),\n  replace(a, 'x', 'y'),\n  right(b, 2),\n  \
             insert(s, 1, 2, 'x')\nfrom\n  t"
        );
        // The clauses they also spell are untouched.
        assert_eq!(
            format_sql("select * from a left join b on a.id = b.id", IND),
            "select\n  *\nfrom\n  a\n  left join b on a.id = b.id"
        );
        assert_eq!(
            format_sql("replace into t (a) values (1)", IND),
            "replace into\n  t(a)\nvalues\n  (1)"
        );
    }

    /// **`1.e-5` is one number**: a `.` with no digit after it but an exponent
    /// is still the literal's, so the sign is not spaced as a binary minus.
    #[test]
    fn a_number_with_a_bare_point_before_its_exponent_is_one_token() {
        for d in SqlDialect::ALL {
            let sql = "select 1.e-5, 1.E+5, 2.e3 from t";
            let out = super::format_sql(sql, IND, d);
            for kept in ["1.e-5", "1.E+5", "2.e3"] {
                assert!(out.contains(kept), "{d:?}: {kept} split: {out}");
            }
            assert_preserves(sql, d);
        }
    }

    /// **A number may begin or end at its point, and stays set off from the
    /// words around it.** `.5` lexed as a `.` and a `5`, `1.` as a `1` and a
    /// `.`, and `need_space` keeps every `.` tight — so `between .1` came out
    /// `between.1`, which MariaDB 10.11 refuses (ERROR 1064), and `1. as x`
    /// came out `1.as x`, PostgreSQL 16's *trailing junk after numeric
    /// literal*. A point against a name is still a qualifier's.
    #[test]
    fn a_number_with_a_leading_or_trailing_point_keeps_its_spaces() {
        for d in SqlDialect::ALL {
            let out = super::format_sql(
                "select 0.5 between .1 and .9, case when x then .5 else .25 end from t",
                IND,
                d,
            );
            for kept in ["between .1", "and .9", "then .5", "else .25"] {
                assert!(out.contains(kept), "{d:?}: {kept}: {out}");
            }
            let out = super::format_sql("select 1. as x, 2. from t where y=.5", IND, d);
            for kept in ["1. as x", "2.\nfrom", "y = .5"] {
                assert!(out.contains(kept), "{d:?}: {kept}: {out}");
            }
            // Signs and parens around one stay tight as they do around any number.
            let out = super::format_sql("select f(-.5, (.5), 1.)", IND, d);
            assert!(out.contains("f(-.5, (.5), 1.)"), "{d:?}: {out}");
            // A qualifier's point is still tight, a digit-led name's too.
            let out = super::format_sql("select f(t.a, t.1x, s.t.c) from s.t", IND, d);
            assert!(
                out.contains("f(t.a, t.1x, s.t.c)") && out.contains("s.t"),
                "{out}"
            );
        }
        assert_preserves("select a from db..t", SqlDialect::MsSql);
    }

    /// **A name `go` never lands alone on a line where `GO` is a directive.**
    /// There it is the batch separator, so the formatted script split in two
    /// broken batches — and a second Format Code read it as one.
    #[test]
    fn a_name_spelled_go_never_becomes_a_batch_separator() {
        for sql in [
            "select a, go\nfrom t",
            "select a, go, b from t",
            "select go from t",
            "select a, -- c\n go from t",
        ] {
            let out = super::format_sql(sql, IND, SqlDialect::MsSql);
            for line in out.lines() {
                let line = line.trim_start();
                assert!(
                    crate::sql::go_directive(line, 0, SqlDialect::MsSql).is_none(),
                    "{sql:?} → {out:?}"
                );
            }
            assert_eq!(
                super::format_sql(&out, IND, SqlDialect::MsSql),
                out,
                "{sql:?}"
            );
            assert_preserves(sql, SqlDialect::MsSql);
        }
    }

    /// **Adjacent string literals keep the newline between them.** On
    /// PostgreSQL `'foo'⏎'bar'` is one string, `'foobar'`, *only* because of
    /// the newline; joined onto one line it is a syntax error. Literals written
    /// on one line stay on one.
    #[test]
    fn string_literals_continued_across_a_line_stay_on_two_lines() {
        let out = super::format_sql("select 'foo'\n'bar' from t", IND, SqlDialect::Postgres);
        assert!(!out.contains("'foo' 'bar'"), "{out}");
        assert!(out.contains("'foo'\n"), "{out}");
        assert_eq!(super::format_sql(&out, IND, SqlDialect::Postgres), out);
        let out = super::format_sql("select 'a' 'b' from t", IND, SqlDialect::MySql);
        assert!(out.contains("'a' 'b'"), "{out}");
    }

    /// **A selection's outer whitespace is the document's, not the
    /// fragment's.** Formatting `select a from ` out of `select a from t`
    /// trimmed the trailing space, and the splice fused `from` with `t`.
    #[test]
    fn formatting_a_range_keeps_the_whitespace_around_it() {
        let keep = KeywordCase::Keep;
        let full = "select a from t";
        let out = format_range(full, 0, 14, IND, SqlDialect::MySql, keep);
        assert!(out.ends_with(' '), "{out:?}");
        let full = "select a from t where x=1";
        let out = format_range(full, 15, full.len(), IND, SqlDialect::MySql, keep);
        assert!(out.starts_with(' '), "{out:?}");
        assert_eq!(
            format_range("a  \n ", 1, 5, IND, SqlDialect::MySql, keep),
            "  \n "
        );
        // The whole document is `format_sql`'s, trailing newline rule and all.
        let full = "  select a from t\n\n";
        assert_eq!(
            format_range(full, 0, full.len(), IND, SqlDialect::MySql, keep),
            super::format_sql(full, IND, SqlDialect::MySql)
        );
    }

    /// The caret keeps its place among the tokens: formatting moves only the
    /// whitespace between them, so the count of non-blank bytes before it
    /// finds the same spot in the result.
    #[test]
    fn the_caret_keeps_its_place_through_a_format() {
        let old = "select a, b from t where x = 1";
        let new = format_sql(old, IND);
        let caret = old.find("rom").unwrap();
        let at = caret_after_format(old, caret, &new);
        assert_eq!(&new[at..at + 3], "rom");
        assert_eq!(caret_after_format(old, 0, &new), 0);
        assert_eq!(caret_after_format(old, old.len(), &new), new.len());
        // In the whitespace after a token, it lands right after the token.
        let caret = old.find(" from").unwrap();
        let at = caret_after_format(old, caret, &new);
        assert!(new[..at].ends_with('b'), "{:?}", &new[..at]);
    }

    #[test]
    fn an_alter_tables_actions_go_one_a_line() {
        assert_eq!(
            my(
                "ALTER TABLE t ADD COLUMN a int NOT NULL DEFAULT 0, DROP COLUMN b, \
                MODIFY c varchar(10), ADD INDEX ix (a, c)"
            ),
            "ALTER TABLE t\n    ADD COLUMN a int NOT NULL DEFAULT 0,\n    DROP COLUMN b,\n    \
             MODIFY c varchar(10),\n    ADD INDEX ix (a, c)"
        );
        assert_eq!(
            super::format_sql(
                "alter table if exists only s.t alter column a set default 0, \
                 alter column b drop not null",
                "    ",
                SqlDialect::Postgres
            ),
            "alter table if exists only s.t\n    alter column a set default 0,\n    \
             alter column b drop not null"
        );
    }

    #[test]
    fn a_single_alter_action_keeps_its_line() {
        let one =
            "ALTER TABLE t ADD CONSTRAINT fk FOREIGN KEY (a) REFERENCES u (id) ON DELETE CASCADE";
        assert_eq!(my(one), one);
        // A comma inside the action's own parens is not a second action.
        let cols = "ALTER TABLE t ADD INDEX ix (a, b)";
        assert_eq!(my(cols), cols);
    }

    #[test]
    fn an_indexs_column_list_is_set_off_from_its_table() {
        assert_eq!(
            my("CREATE UNIQUE INDEX ix_a ON t(a, b DESC) WHERE a > 0"),
            "CREATE UNIQUE INDEX ix_a ON t (a, b DESC)\nWHERE\n    a > 0"
        );
        let pg = "CREATE INDEX ix ON t USING btree (lower(name)) INCLUDE (x)";
        assert_eq!(super::format_sql(pg, "    ", SqlDialect::Postgres), pg);
        // A join's `ON f(x)` is still a call.
        assert!(my("SELECT a FROM t JOIN u ON f(u.id) = t.id").contains("ON f(u.id)"));
    }

    #[test]
    fn a_views_header_keeps_its_line_and_its_check_option_takes_one() {
        assert_eq!(
            my("CREATE OR REPLACE VIEW v (x, y) AS SELECT a, b FROM t WITH CHECK OPTION"),
            "CREATE OR REPLACE VIEW v (x, y) AS\nSELECT\n    a,\n    b\nFROM\n    t\nWITH CHECK OPTION"
        );
        let out = my("CREATE DEFINER=`root`@`%` VIEW v AS SELECT a FROM t WITH LOCAL CHECK OPTION");
        assert!(
            out.starts_with("CREATE DEFINER = `root`@`%` VIEW v AS\n"),
            "{out}"
        );
        assert!(out.ends_with("\nWITH LOCAL CHECK OPTION"), "{out}");
        assert!(my("CREATE USER 'a'@'h'").ends_with("'a'@'h'"));
    }

    #[test]
    fn an_or_that_spells_a_statement_is_no_boolean() {
        for (d, sql) in [
            (SqlDialect::Postgres, "CREATE OR REPLACE FUNCTION"),
            (SqlDialect::Sqlite, "INSERT OR REPLACE INTO"),
            (SqlDialect::Sqlite, "UPDATE OR IGNORE"),
            (SqlDialect::MySql, "INSERT IGNORE INTO"),
        ] {
            let out = super::format_sql(&format!("{sql} t"), "    ", d);
            assert!(out.starts_with(sql), "{out}");
        }
    }
}
