//! SQL syntax highlighting for the Floem code editor.
//!
//! Highlighting is driven through the editor's `Styling::apply_attr_styles`
//! hook: for each visible line we lex the text and add colored spans. The
//! tokenizer is a lightweight per-line SQL lexer (keywords / strings / numbers
//! / comments). It's deliberately behind this one function — a tree-sitter
//! grammar could replace `lex_line` later without touching the editor wiring.
//!
//! Limitation: because lexing is per-line, a `/* ... */` block comment spanning
//! multiple lines only colors the line(s) where the delimiters appear. Good
//! enough for now; tree-sitter would fix it.

use std::borrow::Cow;
use std::rc::Rc;

use floem::peniko::Color;
use floem::text::{Attrs, AttrsList, FamilyOwned};
use floem::views::editor::EditorStyle;
use floem::views::editor::core::buffer::rope_text::RopeText;
use floem::views::editor::id::EditorId;
use floem::views::editor::text::{Document, Styling};

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
}

impl SqlStyling {
    pub fn new(doc: Rc<dyn Document>) -> Self {
        Self {
            doc,
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
        for (start, end, tok) in lex_line(&content) {
            attrs.add_span(start..end, default.color(tok.color()));
        }
    }
}

/// Public: color spans for a standalone SQL line (byte ranges + color), for
/// callers outside the editor — e.g. the Ctrl+K diff, which renders each line as
/// colored segments rather than through the editor's `Styling` hook. Same lexer
/// as the editor, so highlighting matches exactly.
pub fn highlight_spans(line: &str) -> Vec<(usize, usize, Color)> {
    lex_line(line)
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
/// their theme color. (Lexing is per-line, so a multi-line `/* … */` only colors
/// the portion on each line — an unterminated construct runs to line end.)
fn lex_line(line: &str) -> Vec<(usize, usize, Tok)> {
    let b = line.as_bytes();
    let n = b.len();
    let mut out = Vec::new();
    let mut i = 0;

    while i < n {
        let c = b[i];

        // A string, backtick identifier, or comment: color by which one it is.
        if let Some(end) = schemaic_core::sql::skip_noncode(b, i) {
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
