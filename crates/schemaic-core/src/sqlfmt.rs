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

use crate::sql::skip_noncode;

/// Format `sql`, indenting each level with `indent_unit` (e.g. `"    "` or
/// `"\t"`). Token text is preserved verbatim; only whitespace/layout changes.
pub fn format_sql(sql: &str, indent_unit: &str) -> String {
    let toks = tokenize(sql);
    let mut f = Fmt::new(indent_unit);
    f.run(&toks);
    f.out.trim_end().to_string()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Word,
    Quoted,
    LineComment,
    BlockComment,
    Punct,
}

fn is_word_byte(c: u8) -> bool {
    // Bytes >= 0x80 are treated as word bytes so Unicode identifiers tokenize
    // whole (same rule as the editor's `tokenize_range`).
    c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80
}

/// Multi-character operators, longest first, so `->>` beats `->` beats `>`.
const OPS: &[&str] = &[
    "->>", "<=>", "->", ">=", "<=", "<>", "!=", ":=", "||", "&&", "<<", ">>",
];

fn tokenize(sql: &str) -> Vec<(Kind, &str)> {
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
        // Strings / backtick identifiers / comments — one verbatim slice, keyed
        // by the opening byte (`skip_noncode` only returns `Some` for a `--`/`/*`
        // here when it really is a comment).
        if let Some(j) = skip_noncode(b, i) {
            let kind = match c {
                b'#' | b'-' => Kind::LineComment,
                b'/' => Kind::BlockComment,
                _ => Kind::Quoted,
            };
            toks.push((kind, &sql[i..j]));
            i = j;
            continue;
        }
        if is_word_byte(c) {
            let s = i;
            i += 1;
            while i < n && is_word_byte(b[i]) {
                i += 1;
            }
            toks.push((Kind::Word, &sql[s..i]));
            continue;
        }
        // Punctuation / operators.
        let rest = &sql[i..];
        let len = OPS
            .iter()
            .find(|op| rest.len() >= op.len() && &rest[..op.len()] == **op)
            .map(|op| op.len())
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
            && !tight_left
            && !self.tight_next
            && self.need_space(kind, text)
        {
            self.out.push(' ');
        }
        self.tight_next = false;
        self.out.push_str(text);
        self.line_has_content = true;
        self.prev = Some((kind, text.to_string()));
    }

    fn need_space(&self, cur_kind: Kind, cur: &str) -> bool {
        let Some((pk, pt)) = &self.prev else {
            return false;
        };
        let pt = pt.as_str();
        // prev forces no following space
        if *pk == Kind::Punct && (pt == "(" || pt == ".") {
            return false;
        }
        // cur forces no preceding space
        if cur_kind == Kind::Punct && matches!(cur, ")" | "," | ";" | ".") {
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

    const IND: &str = "  ";

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
}
