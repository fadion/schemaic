//! Pure SQL pretty-printer: re-flow whitespace, indentation, and line breaks
//! **without changing any token's text** — keyword case is preserved exactly as
//! typed. Built on the same boundary lexer as the rest of [`crate::sql`]
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
    let toks = tokenize(sql, dialect);
    let mut f = Fmt::new(indent_unit);
    f.run(&toks);
    let out = f.out.trim_end();
    if sql.ends_with('\n') {
        format!("{out}\n")
    } else {
        out.to_string()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Word,
    Quoted,
    LineComment,
    BlockComment,
    Punct,
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
        SqlDialect::MySql | SqlDialect::Sqlite => false,
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

fn tokenize(sql: &str, dialect: SqlDialect) -> Vec<(Kind, &str)> {
    let b = sql.as_bytes();
    let n = b.len();
    let mut toks = Vec::new();
    let mut i = 0;
    while i < n {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
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
        if c.is_ascii_digit() {
            let s = i;
            while i < n && is_word_byte(b[i]) {
                i += 1;
            }
            if i < n && b[i] == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit) {
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
        if is_word_byte(c) {
            let s = i;
            i += 1;
            while i < n && is_word_byte(b[i]) {
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
        if crate::params::opens_placeholder(b, i) {
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
];

// Words that may follow a head on the same line (e.g. `GROUP BY`, `SELECT
// DISTINCT`, `UNION ALL`, `INSERT INTO`, `DELETE FROM`).
fn head_followers(up: &str) -> &'static [&'static str] {
    match up {
        "GROUP" | "ORDER" => &["BY"],
        "SELECT" => &["DISTINCT", "ALL", "DISTINCTROW", "SQL_CALC_FOUND_ROWS"],
        "UNION" => &["ALL", "DISTINCT"],
        "INSERT" | "REPLACE" => &["INTO"],
        "DELETE" => &["FROM"],
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
        )
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

struct Paren {
    subquery: bool,
    saved_base: usize,
    saved_content: usize,
}

struct Fmt<'a> {
    unit: &'a str,
    out: String,
    /// Pending line break to this indent level, materialized on the next emit.
    pending: Option<usize>,
    /// Emit a blank line (double newline) at the next materialized break.
    blank: bool,
    line_indent: usize,
    line_has_content: bool,
    /// Clause base indent for the current block; clause keywords print here.
    base: usize,
    /// Where clause bodies / commas / `AND`/`OR` break to (usually `base + 1`).
    content: usize,
    parens: Vec<Paren>,
    prev: Option<(Kind, String)>,
    /// Inside `BETWEEN … AND …`: the next `AND` stays inline.
    suppress_and: bool,
    /// Previous token was a unary sign → the next token is tight against it.
    tight_next: bool,
}

impl<'a> Fmt<'a> {
    fn new(unit: &'a str) -> Self {
        Fmt {
            unit,
            out: String::new(),
            pending: None,
            blank: false,
            line_indent: 0,
            line_has_content: false,
            base: 0,
            content: 0,
            parens: Vec::new(),
            prev: None,
            suppress_and: false,
            tight_next: false,
        }
    }

    fn break_to(&mut self, level: usize) {
        // Overwrites any prior pending break, so repeated breaks before real text
        // coalesce to the last requested level (no blank clause lines).
        self.pending = Some(level);
    }

    fn emit(&mut self, kind: Kind, text: &str, tight_left: bool) {
        if let Some(level) = self.pending.take() {
            if !self.out.is_empty() {
                self.out.push('\n');
                if self.blank {
                    self.out.push('\n');
                }
            }
            self.blank = false;
            for _ in 0..level {
                self.out.push_str(self.unit);
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
    fn would_fuse(&self, text: &str) -> bool {
        let Some((_, pt)) = &self.prev else {
            return false;
        };
        matches!(
            (pt.as_bytes().last(), text.as_bytes().first()),
            (Some(b'-'), Some(b'-')) | (Some(b'/'), Some(b'*')) | (Some(b'*'), Some(b'/'))
        )
    }

    fn need_space(&self, cur_kind: Kind, cur: &str) -> bool {
        let Some((pk, pt)) = &self.prev else {
            return false;
        };
        let pt = pt.as_str();
        // prev forces no following space. `::` is a cast, and a cast binds to
        // what it casts the way `.` binds to what it qualifies: `id :: text` is
        // legal PostgreSQL and reads like a mistake.
        if *pk == Kind::Punct && (pt == "(" || pt == "." || pt == "::") {
            return false;
        }
        // cur forces no preceding space
        if cur_kind == Kind::Punct && matches!(cur, ")" | "," | ";" | "." | "::") {
            return false;
        }
        if cur_kind == Kind::Punct && cur == "(" {
            // Function call `name(` / `)(` → tight; `KEYWORD (` → spaced.
            if *pk == Kind::Word && !is_keyword(pt) {
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

    fn in_expr_paren(&self) -> bool {
        matches!(self.parens.last(), Some(p) if !p.subquery)
    }

    fn run(&mut self, toks: &[(Kind, &str)]) {
        let mut k = 0;
        while k < toks.len() {
            let (kind, text) = toks[k];
            match kind {
                Kind::LineComment => {
                    self.emit(kind, text, false);
                    // A line comment runs to EOL; whatever follows must be on a
                    // new line at the current structural indent.
                    self.break_to(self.line_indent);
                }
                Kind::BlockComment | Kind::Quoted => {
                    self.emit(kind, text, false);
                }
                Kind::Word => {
                    let up = text.to_ascii_uppercase();
                    let in_expr = self.in_expr_paren();
                    if !in_expr && HEADS.contains(&up.as_str()) {
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
                        self.emit(Kind::Punct, "(", false);
                        self.parens.push(Paren {
                            subquery: sub,
                            saved_base: self.base,
                            saved_content: self.content,
                        });
                        if sub {
                            self.base = self.line_indent + 1;
                            self.content = self.base;
                            self.break_to(self.base);
                        }
                    }
                    ")" => {
                        if let Some(p) = self.parens.pop() {
                            if p.subquery {
                                let close = self.base.saturating_sub(1);
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
                        self.emit(Kind::Punct, ",", true);
                        // Break top-level list commas and subquery SELECT-list
                        // commas; keep function-argument / IN-list commas inline.
                        let breakable = match self.parens.last() {
                            None => true,
                            Some(p) => p.subquery,
                        };
                        if breakable {
                            self.break_to(self.content);
                        }
                    }
                    ";" => {
                        self.emit(Kind::Punct, ";", true);
                        self.base = 0;
                        self.content = 0;
                        self.parens.clear();
                        self.suppress_and = false;
                        self.prev = None;
                        self.blank = true;
                        self.break_to(0);
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
                    _ => {
                        self.emit(Kind::Punct, text, false);
                    }
                },
            }
            k += 1;
        }
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

    // These tests predate the dialect parameter and assert MySQL formatting; a thin
    // MySQL-defaulting wrapper (shadowing the glob import) keeps them unchanged.
    fn format_sql(sql: &str, indent_unit: &str) -> String {
        super::format_sql(sql, indent_unit, SqlDialect::MySql)
    }

    const IND: &str = "  ";

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
            for dialect in [SqlDialect::MySql, SqlDialect::Postgres, SqlDialect::Sqlite] {
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
            for dialect in [SqlDialect::MySql, SqlDialect::Postgres, SqlDialect::Sqlite] {
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
        for dialect in [SqlDialect::MySql, SqlDialect::Postgres, SqlDialect::Sqlite] {
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
        super::tokenize(s, dialect)
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
        for sql in shared {
            assert_preserves(sql, SqlDialect::MySql);
            assert_preserves(sql, SqlDialect::Postgres);
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
}
