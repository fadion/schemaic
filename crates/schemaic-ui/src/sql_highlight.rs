//! SQL syntax highlighting for the Floem code editor.
//!
//! Highlighting is driven through the editor's `Styling::apply_attr_styles`
//! hook: for each visible line we lex the text and add colored spans. The
//! tokenizer is a lightweight per-line SQL lexer (keywords / strings / numbers
//! / comments). It's deliberately behind this one function — a tree-sitter
//! grammar could replace `lex_line` later without touching the editor wiring.
//!
//! Multi-line `/* … */` block comments are handled by a cheap cross-line pass
//! (`starts_in_block_comment`): before lexing a line, we check whether the text
//! before it ends inside an open block comment and, if so, colour the line's lead
//! as a comment. A *string* spanning lines is still coloured per-line (the rarer
//! case); a full tree-sitter grammar would generalize both.

use std::borrow::Cow;
use std::rc::Rc;

use floem::peniko::Color;
use floem::text::{Attrs, AttrsList, FamilyOwned};
use floem::views::editor::EditorStyle;
use floem::views::editor::core::buffer::rope_text::RopeText;
use floem::views::editor::id::EditorId;
use floem::views::editor::text::{Document, Styling};
use schemaic_core::intel::SqlDialect;

#[derive(Clone, Copy)]
enum Tok {
    Keyword,
    Str,
    Number,
    Comment,
}

impl Tok {
    // Token colours come from the active editor theme (One Dark Pro / Tokyo
    // Night / Catppuccin Latte), so highlighting follows the theme picker.
    fn color(self) -> Color {
        let t = crate::theme::editor_theme();
        match self {
            Tok::Keyword => t.keyword,
            Tok::Str => t.string,
            Tok::Number => t.number,
            Tok::Comment => t.comment,
        }
    }
}

/// Editor styling that applies SQL highlighting. Holds a handle to the document
/// so it can read each line's text on demand.
pub struct SqlStyling {
    doc: Rc<dyn Document>,
    family: Vec<FamilyOwned>,
    /// The connection's SQL dialect — so `#`-operators / `$tag$` bodies aren't
    /// coloured as comments on a PostgreSQL connection. Fixed at construction
    /// (the editor is rebuilt when the tab's connection changes).
    dialect: SqlDialect,
}

impl SqlStyling {
    pub fn new(doc: Rc<dyn Document>, dialect: SqlDialect) -> Self {
        Self {
            doc,
            dialect,
            // Explicit IBM Plex Mono (the bundled face) rather than the generic
            // `Monospace` — keeps the editor and the Ctrl+K diff on the exact
            // same family, not just both relying on the generic override.
            family: vec![FamilyOwned::Name("IBM Plex Mono".to_string())],
        }
    }
}

impl Styling for SqlStyling {
    // Tracks the editor-theme generation: a theme switch bumps it, which
    // invalidates the editor's cached layout so lines re-highlight in the new
    // palette. (A per-line lexer has no cross-line state, so edited lines are
    // re-highlighted on relayout regardless.)
    fn id(&self) -> u64 {
        crate::theme::editor_generation()
    }

    fn font_size(&self, _edid: EditorId, _line: usize) -> usize {
        crate::theme::editor_font_size().round() as usize
    }

    fn tab_width(&self, _edid: EditorId, _line: usize) -> usize {
        crate::theme::editor_tab_width()
    }

    fn font_family(&self, _edid: EditorId, _line: usize) -> Cow<'_, [FamilyOwned]> {
        Cow::Borrowed(&self.family)
    }

    fn apply_attr_styles(
        &self,
        _edid: EditorId,
        _style: &EditorStyle,
        line: usize,
        default: Attrs,
        attrs: &mut AttrsList,
    ) {
        let rope = self.doc.rope_text();
        if line >= rope.num_lines() {
            return;
        }
        let content = rope.line_content(line);
        // Does this line begin inside a `/* … */` opened on an earlier line?
        let start_in_block = line > 0 && {
            let start = rope.offset_of_line(line);
            starts_in_block_comment(&rope.slice_to_cow(0..start), self.dialect)
        };
        for (start, end, tok) in lex_line(&content, self.dialect, start_in_block) {
            attrs.add_span(start..end, default.color(tok.color()));
        }
    }
}

/// Whether `prefix` (all the editor text before the line being highlighted) ends
/// *inside* an unterminated `/* … */` block comment — so the next line starts in a
/// comment and its lead must be coloured as one. Built on the shared
/// `schemaic_core::sql::skip_noncode` primitive, so strings / line-comments /
/// dollar-quotes are skipped (a `/*` inside a string never opens a comment) and the
/// block boundary agrees with the rest of the SQL tooling.
fn starts_in_block_comment(prefix: &str, dialect: SqlDialect) -> bool {
    let b = prefix.as_bytes();
    // Fast path: no `/*` anywhere → definitely not inside a block comment.
    if !b.windows(2).any(|w| w == b"/*") {
        return false;
    }
    let n = b.len();
    let mut i = 0;
    while i < n {
        if let Some(end) = schemaic_core::sql::skip_noncode(b, i, dialect) {
            // A block comment reaching `end` is *terminated* only if `end` sits right
            // after a real `*/` (at least `/**/`, i.e. `end >= i + 4`, so the close
            // doesn't overlap the opening `/*`). Otherwise it runs into the next line.
            let is_block = b[i] == b'/' && b.get(i + 1) == Some(&b'*');
            if is_block && !(end >= i + 4 && &b[end - 2..end] == b"*/") {
                return true;
            }
            i = end.max(i + 1);
        } else {
            i += 1;
        }
    }
    false
}

/// Public: color spans for a standalone SQL line (byte ranges + color), for
/// callers outside the editor — e.g. the Ctrl+K diff, which renders each line as
/// colored segments rather than through the editor's `Styling` hook. Same lexer
/// as the editor, so highlighting matches exactly.
pub fn highlight_spans(line: &str, dialect: SqlDialect) -> Vec<(usize, usize, Color)> {
    // Standalone line (Ctrl+K diff) — no document context, so no cross-line block
    // state; a multi-line comment there stays coloured per-line.
    lex_line(line, dialect, false)
        .into_iter()
        .map(|(s, e, tok)| (s, e, tok.color()))
        .collect()
}

/// Lex a single line into colored token spans (byte offsets within the line).
/// Only tokens we color are returned; identifiers/operators keep the default.
///
/// String / backtick-identifier / comment boundaries come from the shared
/// `schemaic_core::sql::skip_noncode` primitive, so highlighting agrees with the
/// statement splitter and the WHERE guard on where those constructs begin and
/// end. Backtick identifiers keep the default color; comments and strings get
/// their theme color.
///
/// `start_in_block` = this line begins inside a `/* … */` block comment opened on
/// an earlier line (see [`starts_in_block_comment`]): its leading text up to the
/// closing `*/` (or the whole line, if it doesn't close here) is coloured as a
/// comment before normal lexing resumes. (A string spanning lines is still handled
/// per-line — the rarer case the TODO didn't call for.)
fn lex_line(line: &str, dialect: SqlDialect, start_in_block: bool) -> Vec<(usize, usize, Tok)> {
    let b = line.as_bytes();
    let n = b.len();
    let mut out = Vec::new();
    let mut i = 0;

    // Continuation of a block comment from a previous line: colour to the `*/`.
    if start_in_block {
        match line.find("*/") {
            Some(p) => {
                out.push((0, p + 2, Tok::Comment));
                i = p + 2;
            }
            None => {
                if n > 0 {
                    out.push((0, n, Tok::Comment));
                }
                return out;
            }
        }
    }

    while i < n {
        let c = b[i];

        // A string, identifier, dollar-quote, or comment: color by which one it is.
        if let Some(end) = schemaic_core::sql::skip_noncode(b, i, dialect) {
            let end = end.min(n);
            match c {
                b'`' => {} // quoted identifier: default color
                b'\'' | b'"' => out.push((i, end, Tok::Str)),
                _ => out.push((i, end, Tok::Comment)), // `--`, `#`, `/* */`
            }
            i = end;
            continue;
        }
        // number literal
        if c.is_ascii_digit() {
            let mut j = i + 1;
            while j < n && (b[j].is_ascii_digit() || b[j] == b'.') {
                j += 1;
            }
            out.push((i, j, Tok::Number));
            i = j;
            continue;
        }
        // word: keyword or identifier
        if c.is_ascii_alphabetic() || c == b'_' {
            let mut j = i + 1;
            while j < n && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                j += 1;
            }
            if is_keyword(&line[i..j]) {
                out.push((i, j, Tok::Keyword));
            }
            i = j;
            continue;
        }

        i += 1;
    }

    out
}

fn is_keyword(w: &str) -> bool {
    matches!(
        w.to_ascii_uppercase().as_str(),
        "SELECT"
            | "FROM"
            | "WHERE"
            | "AND"
            | "OR"
            | "NOT"
            | "NULL"
            | "IS"
            | "IN"
            | "LIKE"
            | "BETWEEN"
            | "EXISTS"
            | "INSERT"
            | "INTO"
            | "VALUES"
            | "UPDATE"
            | "SET"
            | "DELETE"
            | "CREATE"
            | "TABLE"
            | "VIEW"
            | "DROP"
            | "ALTER"
            | "ADD"
            | "COLUMN"
            | "PRIMARY"
            | "KEY"
            | "FOREIGN"
            | "REFERENCES"
            | "INDEX"
            | "UNIQUE"
            | "DEFAULT"
            | "JOIN"
            | "INNER"
            | "LEFT"
            | "RIGHT"
            | "OUTER"
            | "FULL"
            | "CROSS"
            | "ON"
            | "USING"
            | "AS"
            | "GROUP"
            | "BY"
            | "ORDER"
            | "HAVING"
            | "LIMIT"
            | "OFFSET"
            | "DISTINCT"
            | "UNION"
            | "ALL"
            | "CASE"
            | "WHEN"
            | "THEN"
            | "ELSE"
            | "END"
            | "ASC"
            | "DESC"
            | "TRUE"
            | "FALSE"
            | "COUNT"
            | "SUM"
            | "AVG"
            | "MIN"
            | "MAX"
            | "DATABASE"
            | "USE"
            | "SHOW"
            | "DESCRIBE"
            | "EXPLAIN"
            | "WITH"
            | "CONSTRAINT"
            | "CASCADE"
            | "ENGINE"
            | "INT"
            | "INTEGER"
            | "BIGINT"
            | "SMALLINT"
            | "TINYINT"
            | "MEDIUMINT"
            | "VARCHAR"
            | "CHAR"
            | "TEXT"
            | "DATE"
            | "DATETIME"
            | "TIMESTAMP"
            | "TIME"
            | "YEAR"
            | "DECIMAL"
            | "NUMERIC"
            | "FLOAT"
            | "DOUBLE"
            | "BOOLEAN"
            | "BOOL"
            | "JSON"
            | "BLOB"
            | "AUTO_INCREMENT"
            | "UNSIGNED"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comment_spans(line: &str, start_in_block: bool) -> Vec<(usize, usize)> {
        lex_line(line, SqlDialect::MySql, start_in_block)
            .into_iter()
            .filter(|(_, _, t)| matches!(t, Tok::Comment))
            .map(|(s, e, _)| (s, e))
            .collect()
    }

    #[test]
    fn block_open_state_across_lines() {
        let d = SqlDialect::MySql;
        // Unterminated `/*` → the next line starts inside the comment.
        assert!(starts_in_block_comment("SELECT 1; /* note", d));
        // Closed on the same line → not inside.
        assert!(!starts_in_block_comment("SELECT 1; /* note */", d));
        // Opened on an earlier line and still open.
        assert!(starts_in_block_comment("/* a\nb\n", d));
        // Opened then closed across lines → not inside after the close.
        assert!(!starts_in_block_comment("/* a\nb */\n", d));
        // A `/*` inside a string literal does not open a comment.
        assert!(!starts_in_block_comment("SELECT '/*';\n", d));
        // No `/*` at all.
        assert!(!starts_in_block_comment("SELECT 1;\n", d));
    }

    #[test]
    fn continuation_line_colours_comment_then_lexes_code() {
        // A line wholly inside a block comment → all comment.
        assert_eq!(comment_spans("still a comment", true), vec![(0, 15)]);
        // A line that closes the block partway → comment up to `*/`, then code.
        let line = "end */ SELECT";
        assert_eq!(comment_spans(line, true), vec![(0, 6)]); // "end */"
        // The trailing SELECT is lexed as a keyword (not swallowed by the comment).
        let kw = lex_line(line, SqlDialect::MySql, true)
            .into_iter()
            .find(|(_, _, t)| matches!(t, Tok::Keyword));
        assert!(kw.is_some_and(|(s, e, _)| &line[s..e] == "SELECT"));
    }

    #[test]
    fn line_not_in_block_is_unaffected() {
        // start_in_block = false → leading text isn't forced to a comment.
        assert!(comment_spans("SELECT 1", false).is_empty());
    }
}
