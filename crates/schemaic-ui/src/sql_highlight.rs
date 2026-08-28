//! SQL syntax highlighting for the Floem code editor.
//!
//! Highlighting is driven through the editor's `Styling::apply_attr_styles`
//! hook: for each visible line we lex the text and add colored spans. The
//! tokenizer is a lightweight per-line SQL lexer (keywords / strings / numbers
//! / comments). It's deliberately behind this one function — a tree-sitter
//! grammar could replace `lex_line` later without touching the editor wiring.
//!
//! Multi-line `/* … */` block comments are the one thing a per-line lexer can't
//! see, so `block_comment_lines` makes one forward pass over the document and
//! records, for every line, whether it *starts* inside an open block comment;
//! `SqlStyling` caches that per document revision, and a line that does gets its
//! lead coloured as a comment. A *string* spanning lines is still coloured
//! per-line (the rarer case); a full tree-sitter grammar would generalize both.

use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;

use floem::peniko::Color;
use floem::reactive::{RwSignal, SignalGet};
use floem::text::{Attrs, AttrsList, FamilyOwned};
use floem::views::editor::EditorStyle;
use floem::views::editor::core::buffer::rope_text::RopeText;
use floem::views::editor::id::EditorId;
use floem::views::editor::layout::{LineExtraStyle, TextLayoutLine};
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
    /// This tab's temporary font-size override (px) for Ctrl+scroll zoom; `None`
    /// follows the user's configured size. Per-tab, so zooming one editor doesn't
    /// touch others.
    zoom: RwSignal<Option<f32>>,
    /// The pending inline-AI (Ctrl+K) suggestion, shared with the
    /// [`InlineDiffDoc`](crate::inline_diff::InlineDiffDoc) that emits its added
    /// rows as phantom text. This half paints them: the replaced lines tinted
    /// and faded, the added rows on their own background.
    ///
    /// The two must read the *same* signal — the row backgrounds are positioned
    /// by counting the phantom rows the document emitted, so a second source of
    /// truth would paint bands over the wrong lines.
    preview: crate::inline_diff::InlinePreview,
    /// Per-line "starts inside a block comment" flags, with the document
    /// revision they were computed for.
    ///
    /// [`apply_attr_styles`](Styling::apply_attr_styles) is called per *visible
    /// line*, and it used to answer this by copying and re-lexing the whole
    /// document **before that line** — Θ(visible × document) per relayout, and a
    /// relayout happens on every keystroke. A 190 KB script opening with a
    /// `mysqldump` header comment cost ~11 ms of extra scanning per keypress.
    /// One forward pass answers it for every line at once, so the cost is paid
    /// once per edit instead of ~45 times.
    block_lines: RefCell<Option<(u64, Vec<bool>)>>,
}

impl SqlStyling {
    pub fn new(
        doc: Rc<dyn Document>,
        dialect: SqlDialect,
        zoom: RwSignal<Option<f32>>,
        preview: crate::inline_diff::InlinePreview,
    ) -> Self {
        Self {
            doc,
            dialect,
            zoom,
            preview,
            block_lines: RefCell::new(None),
            // Explicit IBM Plex Mono (the bundled face) rather than the generic
            // `Monospace` — keeps the editor and the Ctrl+K diff on the exact
            // same family, not just both relying on the generic override.
            family: vec![FamilyOwned::Name("IBM Plex Mono".to_string())],
        }
    }

    /// The effective editor font size (px): the tab's zoom override, else the
    /// user's configured size. Both reads are reactive so `id()`/`font_size()`
    /// re-run when either changes.
    fn effective_px(&self) -> f32 {
        self.zoom
            .get()
            .unwrap_or_else(crate::theme::editor_font_size)
    }

    /// Does `line` begin inside a `/* … */` opened earlier?
    ///
    /// Recomputes the whole document's flags when it has changed since the last
    /// call (`cache_rev` is bumped by every edit), and answers from the cache
    /// otherwise — so a relayout's ~45 line callbacks share one pass.
    fn starts_in_block(&self, line: usize) -> bool {
        if line == 0 {
            return false;
        }
        let rev = self.doc.cache_rev().get_untracked();
        let mut cache = self.block_lines.borrow_mut();
        if cache.as_ref().map(|(r, _)| *r) != Some(rev) {
            let text = self.doc.text().to_string();
            *cache = Some((rev, block_comment_lines(&text, self.dialect)));
        }
        cache
            .as_ref()
            .is_some_and(|(_, flags)| flags.get(line).copied().unwrap_or(false))
    }
}

impl Styling for SqlStyling {
    // Tracks the editor-theme generation: a theme switch bumps it, which
    // invalidates the editor's cached layout so lines re-highlight in the new
    // palette. (A per-line lexer has no cross-line state, so edited lines are
    // re-highlighted on relayout regardless.)
    fn id(&self) -> u64 {
        // Fold the effective font size into the cache key so a per-tab zoom (or a
        // settings font-size change) invalidates this editor's layout and re-lays
        // out at the new size — the generation covers theme/tab-width changes.
        (crate::theme::editor_generation() << 8) | (self.effective_px().round() as u64 & 0xFF)
    }

    fn font_size(&self, _edid: EditorId, _line: usize) -> usize {
        self.effective_px().round() as usize
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
        let view = self.preview.get_untracked();
        // A line that is being worked on, or that a settled suggestion replaces,
        // is faded — alpha on the token colour, since editor text has no opacity
        // of its own. The two states fade by different amounts; the view says
        // which, so this stays one rule rather than two.
        let fade = view.as_ref().filter(|v| v.fades(line)).map(|v| v.fade());
        // Floem hands this hook the line's PRE-phantom columns and only adds the
        // phantom spans afterwards, so an end-of-line block (every block but one)
        // needs no adjustment. The exception is a block rendering *before* line 0,
        // which pushes the line's own content right by its whole length.
        let shift = view
            .as_ref()
            .and_then(|v| crate::inline_diff::block_at(v, line))
            .map_or(0, |b| b.prefix_len());
        let tint = |c: Color| match fade {
            Some(a) => c.multiply_alpha(a),
            None => c,
        };
        let faded = fade.is_some();
        // Fade the whole line first — `lex_line` only colours the tokens it knows,
        // and an identifier left on the default colour would stay at full strength
        // in the middle of a faded row.
        if faded {
            attrs.add_span(
                shift..shift + content.len(),
                // The editor theme's own foreground — the same value
                // `editor_pane` feeds the editor as its base text colour.
                default.color(tint(crate::theme::editor_theme().fg)),
            );
        }
        // Does this line begin inside a `/* … */` opened on an earlier line?
        let start_in_block = self.starts_in_block(line);
        for (start, end, tok) in lex_line(&content, self.dialect, start_in_block) {
            attrs.add_span(shift + start..shift + end, default.color(tint(tok.color())));
        }
    }

    fn apply_layout_styles(
        &self,
        edid: EditorId,
        _style: &EditorStyle,
        line: usize,
        layout_line: &mut TextLayoutLine,
    ) {
        let Some(view) = self.preview.get_untracked() else {
            return;
        };
        // Only a settled suggestion paints bands. While the model is working there
        // is nothing to band — the lines just fade (`apply_attr_styles`), which is
        // the design's "dimmed, waiting" state and not a diff yet.
        let Some(plan) = view.plan() else {
            return;
        };
        let replaced = plan.hunks.iter().any(|h| h.del.contains(&line));
        let block = crate::inline_diff::block_at(&view, line);
        if !replaced && block.is_none() {
            return;
        }
        let line_h = f64::from(self.line_height(edid, line));
        // The line's own content length — the column an end-of-line phantom block
        // hangs off, and the same one `InlineDiffDoc::phantom_text` used to place it.
        let own_len = self.doc.rope_text().line_content(line).len();
        let (added_rows, own_rows) =
            crate::inline_diff::row_split(layout_line, line_h, block, own_len);
        // `width: None` is what makes Floem paint the band across the whole
        // viewport rather than just behind the glyphs — a diff row reads as a row.
        let mut band = |rows: std::ops::Range<usize>, bg: Color| {
            for row in rows {
                layout_line.extra_style.push(LineExtraStyle {
                    x: 0.0,
                    y: row as f64 * line_h,
                    width: None,
                    height: line_h,
                    bg_color: Some(bg),
                    under_line: None,
                    wave_line: None,
                });
            }
        };
        if replaced {
            band(own_rows, crate::theme::diff_del_bg());
        }
        band(added_rows, crate::theme::diff_add_bg());
    }
}

/// For every line of `text`, whether it **starts inside** a `/* … */` block
/// comment opened on an earlier line — so its lead must be coloured as a comment.
///
/// One forward pass for the whole document, rather than re-lexing the prefix per
/// line: the editor asks this ~45 times per relayout and a relayout happens on
/// every keystroke, so the per-line form was Θ(visible × document).
///
/// Built on the shared `schemaic_core::sql::skip_noncode` primitive, so
/// strings / line-comments / dollar-quotes are skipped (a `/*` inside a string
/// never opens a comment) and the block boundary agrees with the rest of the SQL
/// tooling. `flags[0]` is always false — line 0 has nothing before it.
fn block_comment_lines(text: &str, dialect: SqlDialect) -> Vec<bool> {
    let b = text.as_bytes();
    let n = b.len();
    // Where the block comments are. `skip_noncode` returns the end of whatever
    // non-code region starts at `i`; an unterminated block runs to EOF.
    let mut regions: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < n {
        match schemaic_core::sql::skip_noncode(b, i, dialect) {
            Some(end) => {
                if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
                    regions.push((i, end.min(n)));
                }
                i = end.max(i + 1);
            }
            None => i += 1,
        }
    }
    // A line starts inside a comment when its start offset falls *strictly*
    // inside a region: equal to the region's end means the `*/` closed it on the
    // previous line, and equal to its start means this line opens the comment
    // itself (which `lex_line` handles on its own).
    let mut flags = vec![false];
    if regions.is_empty() {
        flags.resize(1 + b.iter().filter(|&&c| c == b'\n').count(), false);
        return flags;
    }
    let mut r = 0;
    for (p, _) in b.iter().enumerate().filter(|&(_, &c)| c == b'\n') {
        let start = p + 1;
        while r < regions.len() && regions[r].1 <= start {
            r += 1;
        }
        flags.push(
            regions
                .get(r)
                .is_some_and(|&(lo, hi)| lo < start && start < hi),
        );
    }
    flags
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
/// an earlier line (see [`block_comment_lines`]): its leading text up to the
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

    fn flags(text: &str) -> Vec<bool> {
        block_comment_lines(text, SqlDialect::MySql)
    }

    #[test]
    fn block_open_state_across_lines() {
        // One flag per line, line 0 never inside.
        // Opened on line 0 and still open → lines 1 and 2 are inside it.
        assert_eq!(flags("/* a\nb\nc"), vec![false, true, true]);
        // Closed on line 1 → line 2 is back in code.
        assert_eq!(flags("/* a\nb */\nc"), vec![false, true, false]);
        // Closed on the line that opened it → nothing after is inside.
        assert_eq!(flags("SELECT 1; /* note */\nSELECT 2"), vec![false, false]);
        // Unterminated at EOF → every following line is inside.
        assert_eq!(flags("SELECT 1; /* note\nmore"), vec![false, true]);
        // A `/*` inside a string literal does not open a comment.
        assert_eq!(flags("SELECT '/*';\nSELECT 2"), vec![false, false]);
        // A `/*` inside a line comment doesn't either.
        assert_eq!(flags("-- /* not a comment\nSELECT 2"), vec![false, false]);
        // No `/*` at all — still one flag per line.
        assert_eq!(flags("SELECT 1;\nSELECT 2;\n"), vec![false, false, false]);
        assert_eq!(flags(""), vec![false]);
    }

    #[test]
    fn a_comment_closing_exactly_at_a_line_end_does_not_leak() {
        // The off-by-one that matters: the `*/` is the last thing on line 0, so
        // line 1 starts *at* the region's end and is code, not comment.
        assert_eq!(flags("/* a */\nSELECT 1"), vec![false, false]);
        // And one that opens at a line start doesn't mark its own line.
        assert_eq!(flags("SELECT 1\n/* a\nb"), vec![false, false, true]);
    }

    #[test]
    fn several_block_comments_are_tracked_in_order() {
        // Regions are consumed in order as the line starts advance; a later
        // comment must not be missed because an earlier one was passed.
        assert_eq!(
            flags("/* a\nb */ SELECT 1\n/* c\nd */\nSELECT 2"),
            vec![false, true, false, true, false]
        );
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
