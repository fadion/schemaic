//! SQL syntax highlighting for the Floem code editor.
//!
//! Highlighting is driven through the editor's `Styling::apply_attr_styles`
//! hook: for each visible line we lex the text and add colored spans. The
//! tokenizer is a lightweight per-line SQL lexer (keywords / strings / numbers
//! / comments). It's deliberately behind this one function — a tree-sitter
//! grammar could replace `lex_line_with` later without touching the editor wiring.
//!
//! What a per-line lexer can't see is a region opened on an earlier line — a
//! `/* … */` comment, a multi-line string, a dollar-quoted body — so
//! `carried_lines` makes one forward pass over the document and records, for
//! every line, the region it *starts* inside and where on the line it closes;
//! `SqlStyling` caches that per document revision, and a line that starts in
//! one gets its lead coloured as that region.

use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;

use floem::peniko::Color;
use floem::reactive::{Memo, RwSignal, SignalGet, SignalWith};
use floem::text::{Attrs, AttrsList, FamilyOwned};
use floem::views::editor::EditorStyle;
use floem::views::editor::core::buffer::rope_text::RopeText;
use floem::views::editor::id::EditorId;
use floem::views::editor::layout::{LineExtraStyle, TextLayoutLine};
use floem::views::editor::text::{Document, Styling};
use schemaic_core::intel::SqlDialect;
use schemaic_core::sql::{NonCode, continues_name, is_word_byte, is_word_start, t_sql_name_prefix};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tok {
    Keyword,
    Str,
    Number,
    Comment,
    /// A variable (`@x`, `@@version`) or a `:name` placeholder.
    Variable,
    /// A code bracket at this nesting depth, with rainbow brackets on.
    Bracket(usize),
    /// A builtin called, or one the engine calls bare (`intel::word_class`).
    Function,
    /// A type where only a type can stand (`::integer`, `VARCHAR(40)`).
    Type,
    /// `TRUE`, `FALSE`, `NULL`.
    Constant,
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
            Tok::Variable => t.variable,
            Tok::Function => t.function,
            Tok::Type => t.type_,
            Tok::Constant => t.constant,
            // Three of the palette's own colours, in turn — the gold, orchid,
            // blue round most editors give nested brackets.
            Tok::Bracket(depth) => [t.type_, t.keyword, t.function][depth % 3],
        }
    }
}

/// Editor styling that applies SQL highlighting. Holds a handle to the document
/// so it can read each line's text on demand.
pub struct SqlStyling {
    doc: Rc<dyn Document>,
    family: Vec<FamilyOwned>,
    /// The connection's SQL dialect — so `#`-operators / `$tag$` bodies aren't
    /// coloured as comments on a PostgreSQL connection. **Followed, not fixed
    /// at construction**: the editor is not rebuilt when its tab moves to an
    /// engine of another kind, and a styling holding the old value went on
    /// colouring a SQL Server tab's `#temp` as a MySQL comment. `id()` reads it,
    /// so a change re-lays the lines out, and the block-comment cache is keyed
    /// on it.
    dialect: Memo<SqlDialect>,
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
    /// Per-line "starts inside a non-code region" answers ([`LineCarry`]),
    /// with the document revision they were computed for.
    ///
    /// [`apply_attr_styles`](Styling::apply_attr_styles) is called per *visible
    /// line*, and it used to answer this by copying and re-lexing the whole
    /// document **before that line** — Θ(visible × document) per relayout, and a
    /// relayout happens on every keystroke. A 190 KB script opening with a
    /// `mysqldump` header comment cost ~11 ms of extra scanning per keypress.
    /// One forward pass answers it for every line at once, so the cost is paid
    /// once per edit instead of ~45 times.
    block_lines: CarryCache,
    /// Per-line bracket depth at the line's start, for rainbow brackets, with
    /// the revision and the text it was computed for — taken forward from the
    /// last edit's as `block_lines` is, and only while rainbow brackets are on.
    bracket_lines: RefCell<Option<(BlockKey, String, schemaic_core::pairs::LineDepths)>>,
}

/// What a [`CarryCache`]'s answers were computed for: the document
/// revision and the dialect.
type BlockKey = (u64, SqlDialect);

/// [`carried_lines`]' answer for a document, with what it was computed for —
/// the revision and dialect, and the text, which the next edit's answer is
/// taken forward from ([`carried_lines_after`]).
type CarryCache = RefCell<Option<(BlockKey, String, Vec<Option<LineCarry>>)>>;

/// The non-code region `line` of `doc` begins inside, opened above it. One
/// pass per document revision and dialect, kept in `cache` — what
/// [`SqlStyling`] and [`FieldSql`] both ask. After an edit the pass is
/// [`carried_lines_after`], about the edit's own lines; a new dialect, or the
/// first ask, is the whole [`carried_lines`].
fn carry_at_cached(
    doc: &dyn Document,
    dialect: SqlDialect,
    cache: &CarryCache,
    line: usize,
) -> Option<LineCarry> {
    if line == 0 {
        return None;
    }
    let key = (doc.cache_rev().get_untracked(), dialect);
    let mut cache = cache.borrow_mut();
    if cache.as_ref().map(|(k, _, _)| *k) != Some(key) {
        let text = doc.text().to_string();
        let carries = match cache.take() {
            Some(((_, d), old, prev)) if d == dialect => {
                carried_lines_after(&old, &prev, &text, dialect)
            }
            _ => carried_lines(&text, dialect),
        };
        *cache = Some((key, text, carries));
    }
    cache
        .as_ref()
        .and_then(|(_, _, carries)| carries.get(line).copied().flatten())
}

/// The SQL editor's colouring for an `edit_field` that holds SQL — the DDL
/// preview and the view, trigger and routine bodies — through the same lexer
/// and the same cross-line state ([`carried_lines`]), without the tab editor's zoom,
/// inline diff or rainbow brackets. Its field sits on the preview surface
/// (`theme::preview_bg`), which is the one the editor palette is measured on.
pub(crate) struct FieldSql {
    doc: Rc<dyn Document>,
    dialect: SqlDialect,
    block_lines: CarryCache,
}

impl FieldSql {
    pub(crate) fn new(doc: Rc<dyn Document>, dialect: SqlDialect) -> Self {
        Self {
            doc,
            dialect,
            block_lines: RefCell::new(None),
        }
    }

    /// What the field's layout cache key must also follow: the editor theme,
    /// whose palette these colours are.
    pub(crate) fn generation(&self) -> u64 {
        crate::theme::editor_generation()
    }

    /// Colour `line` — `Styling::apply_attr_styles`' body.
    pub(crate) fn apply(&self, line: usize, default: Attrs, attrs: &mut AttrsList) {
        let rope = self.doc.rope_text();
        if line >= rope.num_lines() {
            return;
        }
        let content = rope.line_content(line);
        let carry = carry_at_cached(&*self.doc, self.dialect, &self.block_lines, line);
        for (start, end, tok) in lex_line_with(&content, self.dialect, carry, None) {
            attrs.add_span(start..end, default.color(tok.color()));
        }
    }
}

impl SqlStyling {
    pub fn new(
        doc: Rc<dyn Document>,
        dialect: Memo<SqlDialect>,
        zoom: RwSignal<Option<f32>>,
        preview: crate::inline_diff::InlinePreview,
    ) -> Self {
        Self {
            doc,
            dialect,
            zoom,
            preview,
            block_lines: RefCell::new(None),
            bracket_lines: RefCell::new(None),
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

    /// The non-code region `line` begins inside, opened earlier.
    ///
    /// Takes the answers forward over the edit when the document has changed
    /// since the last call (`cache_rev` is bumped by every edit;
    /// [`carried_lines_after`]), and answers from the cache otherwise — so a
    /// relayout's ~45 line callbacks share one pass.
    fn carry_at(&self, line: usize) -> Option<LineCarry> {
        carry_at_cached(
            &*self.doc,
            self.dialect.get_untracked(),
            &self.block_lines,
            line,
        )
    }

    /// How many brackets are open where `line` begins
    /// (`pairs::line_depths`), cached per revision and taken forward over an
    /// edit (`pairs::line_depths_after`) as [`carry_at`](Self::carry_at) is.
    fn depth_at(&self, line: usize) -> usize {
        let key = (
            self.doc.cache_rev().get_untracked(),
            self.dialect.get_untracked(),
        );
        let mut cache = self.bracket_lines.borrow_mut();
        if cache.as_ref().map(|(k, _, _)| *k) != Some(key) {
            let text = self.doc.text().to_string();
            let depths = match cache.take() {
                Some(((_, d), old, prev)) if d == key.1 => {
                    schemaic_core::pairs::line_depths_after(&old, &prev, &text, d)
                }
                _ => schemaic_core::pairs::line_depths(&text, key.1),
            };
            *cache = Some((key, text, depths));
        }
        cache
            .as_ref()
            .and_then(|(_, _, depths)| depths.depth.get(line).copied())
            .unwrap_or(0)
    }
}

/// The value `SqlStyling::id` returns — floem folds it into the key its cached
/// `TextLayoutLine`s live under, so anything baked into a layout that this key
/// does not move with **cannot repaint**.
///
/// Four inputs, and every one of them is something baked in:
///
/// - `editor_gen` — the token colours, which come from `editor()`, and the tab
///   width. Bumped by `set_editor` and the font/tab setters.
/// - `ui_gen` — `diff_add_bg` / `diff_del_bg`, the inline-diff bands
///   `apply_layout_styles` writes into `LineExtraStyle`. Those read `ui()`,
///   which `set_ui` and `set_ui_scale` bump and `editor_gen` knows nothing
///   about. Without this term an interface-theme switch left the bands behind
///   the code in the old palette while the gutter strips finishing the very
///   same bands — ordinary style closures, calling the same two accessors —
///   took the new one.
/// - `px` — the effective font size, so a per-tab zoom re-lays out.
/// - `dialect` — which spans are comments, strings and names, so a tab moved to
///   another engine re-lexes its lines. It is not monotonic, but it has its own
///   bits, so it cannot collide with the other terms.
///
/// The generations are **summed** rather than XOR'd: both are monotonic
/// counters, so their sum climbs on every bump of either and never returns to
/// a value the cache has already seen. An XOR does — `2 ^ 3` and `0 ^ 1` are
/// both 1, two states one bump apart sharing a key. (The sum is not injective
/// over `(editor, ui)` pairs, and does not need to be: in one process the pair
/// only ever moves forward.)
fn layout_cache_key(editor_gen: u64, ui_gen: u64, px: f32, dialect: SqlDialect) -> u64 {
    let engine = match dialect {
        SqlDialect::MySql => 0,
        SqlDialect::Postgres => 1,
        SqlDialect::Sqlite => 2,
        SqlDialect::MsSql => 3,
    };
    (editor_gen.wrapping_add(ui_gen) << 11) | (engine << 8) | (px.round() as u64 & 0xFF)
}

impl Styling for SqlStyling {
    // Tracks the editor-theme generation: a theme switch bumps it, which
    // invalidates the editor's cached layout so lines re-highlight in the new
    // palette. (A per-line lexer has no cross-line state, so edited lines are
    // re-highlighted on relayout regardless.)
    //
    // **And the UI generation, because two of the colours this styling bakes in
    // come from that axis.** `apply_layout_styles` writes `diff_del_bg()` /
    // `diff_add_bg()` into each banded line's `LineExtraStyle`, and those read
    // `ui()`, not `editor()`. `set_ui` bumps `ui_gen` alone, so the cached
    // layouts survived an interface-theme switch and kept painting the old
    // palette — while the gutter and right-padding strips that finish the same
    // bands (`editor_pane::inline_band_runs`) are ordinary style closures
    // calling the same two accessors and repainted at once. One band, two
    // colours, disagreeing at the code column, and disagreeing line-to-line as
    // scrolling rebuilt some of them. `editor_generation`'s own doc describes
    // this hazard for its sibling axis; it solved it for one axis of two.
    fn id(&self) -> u64 {
        layout_cache_key(
            crate::theme::editor_generation(),
            crate::theme::ui_generation(),
            self.effective_px(),
            self.dialect.get(),
        )
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
        // **`with_untracked`, not `get_untracked`.** The latter *clones* — the
        // whole `InlinePlan`, every hunk, every added line's `String` — and
        // this hook runs once per line laid out: 90 whole-plan clones for a
        // 45-line viewport, 8.1 ms of them at a 3,000-line suggestion, per
        // relayout, and `set_preview` bumps `cache_rev` precisely to force one.
        // `set_preview` twelve lines from here already reads this same signal
        // with `with_untracked` and says why. Both answers are `Copy`, so
        // nothing escapes the borrow.
        //
        // `fade`: a line that is being worked on, or that a settled suggestion
        // replaces, is faded — alpha on the token colour, since editor text has
        // no opacity of its own. The two states fade by different amounts; the
        // view says which, so this stays one rule rather than two.
        //
        // `shift`: floem hands this hook the line's PRE-phantom columns and
        // only adds the phantom spans afterwards, so an end-of-line block
        // (every block but one) needs no adjustment. The exception is a block
        // rendering *before* line 0, which pushes the line's own content right
        // by its whole length.
        let (fade, shift) = self.preview.with_untracked(|v| {
            let Some(v) = v.as_ref() else {
                return (None, 0);
            };
            (
                Some(v).filter(|v| v.fades(line)).map(|v| v.fade()),
                crate::inline_diff::block_at(v, line).map_or(0, |b| b.prefix_len()),
            )
        });
        let tint = |c: Color| match fade {
            Some(a) => c.multiply_alpha(a),
            None => c,
        };
        let faded = fade.is_some();
        // Fade the whole line first — `lex_line_with` only colours the tokens it knows,
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
        // Does this line begin inside a string or comment opened above it?
        let carry = self.carry_at(line);
        let dialect = self.dialect.get_untracked();
        let brackets = crate::theme::editor_rainbow_brackets().then(|| self.depth_at(line));
        for (start, end, tok) in lex_line_with(&content, dialect, carry, brackets) {
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
        // `with_untracked` for the same reason `apply_attr_styles` uses it —
        // `get_untracked` deep-copies the whole plan, and this is the second of
        // the two hooks that run per line, per relayout. Everything taken out
        // is `Copy`.
        //
        // Only a settled suggestion paints bands. While the model is working there
        // is nothing to band — the lines just fade (`apply_attr_styles`), which is
        // the design's "dimmed, waiting" state and not a diff yet.
        //
        // `replaced` is `InlineView::fades`, not a third spelling of it. "Is this
        // line replaced" was written out here, in `InlineView::fades` and in
        // `editor_pane`'s band gate, with nothing asserting the three agreed —
        // and this one and `fades` are the *same* question (the `Plan` guard has
        // already been established), so a change to either left the
        // code column faded where the bands were not, or banded where the fade
        // was not, with nothing red. `editor_pane`'s is a narrower question and
        // stays its own; see the comment there.
        let Some((replaced, block)) = self.preview.with_untracked(|v| {
            let v = v.as_ref()?;
            v.plan()?;
            Some((v.fades(line), crate::inline_diff::block_at(v, line)))
        }) else {
            return;
        };
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
/// comment opened on an earlier line: `carried_lines` narrowed to comments,
/// for the tests that pin that case.
#[cfg(test)]
fn block_comment_lines(text: &str, dialect: SqlDialect) -> Vec<bool> {
    carried_lines(text, dialect)
        .into_iter()
        .map(|c| c.is_some_and(|c| c.kind == Some(NonCode::Comment)))
        .collect()
}

/// A non-code region a line **starts inside** — a string, a dollar-quoted
/// body, a quoted name or a comment opened on an earlier line: what it is, and
/// the byte column on this line just past its close (`None` when it runs on
/// past the line).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LineCarry {
    pub(crate) kind: Option<NonCode>,
    pub(crate) end: Option<usize>,
}

/// [`carried_lines`] of `new`, from `prev` — `carried_lines(old)` — where an
/// edit turned `old` into `new`: what lies before the edit is reused, the scan
/// restarts at the last line before it that starts in code, and it stops at
/// the first line start past the edit that is in code in both texts, the old
/// answer standing for everything after it, its lines shifted by what the
/// edit added or removed. So a keystroke rescans about its own lines, not the
/// document; an edit that opens a comment or a string rescans as far as the
/// text it changes runs.
///
/// Finding the edit compares the two texts from each end — a memory compare,
/// far cheaper than the lexer. `carried_lines_after_an_edit_match_a_full_pass`
/// holds the two equal over a corpus of edits on every engine.
pub(crate) fn carried_lines_after(
    old: &str,
    prev: &[Option<LineCarry>],
    new: &str,
    dialect: SqlDialect,
) -> Vec<Option<LineCarry>> {
    let (o, n) = (old.as_bytes(), new.as_bytes());
    let (pre, suf) = schemaic_core::text::common_affixes(o, n);
    if pre == o.len() && pre == n.len() {
        return prev.to_vec();
    }
    let newlines = schemaic_core::text::count_newlines;
    // Lines the edit added, less those it removed.
    let shift =
        newlines(&n[pre..n.len() - suf]) as isize - newlines(&o[pre..o.len() - suf]) as isize;
    // Back from the edited line to the last one that starts in code — all of
    // it before the edit, so the same in both texts.
    let mut line = newlines(&n[..pre]).min(prev.len().saturating_sub(1));
    let mut start = n[..pre]
        .iter()
        .rposition(|&c| c == b'\n')
        .map_or(0, |p| p + 1);
    while line > 0 && prev[line].is_some() {
        line -= 1;
        start = n[..start - 1]
            .iter()
            .rposition(|&c| c == b'\n')
            .map_or(0, |p| p + 1);
    }
    let mut out: Vec<Option<LineCarry>> = prev[..=line].to_vec();
    let common_from = n.len() - suf;
    // At a line start in code past the edit, where the old text's same line
    // started in code too, everything after is the old answer.
    let resync = |out: &mut Vec<Option<LineCarry>>, s: usize| -> bool {
        if s == 0 || s - 1 < common_from {
            return false;
        }
        let Some(old_line) = (out.len() as isize - 1 - shift)
            .try_into()
            .ok()
            .filter(|&l: &usize| l < prev.len())
        else {
            return false;
        };
        if prev[old_line].is_some() {
            return false;
        }
        out.extend_from_slice(&prev[old_line + 1..]);
        true
    };
    let line_end = |s: usize| {
        n[s..]
            .iter()
            .position(|&c| c == b'\n')
            .map_or(n.len(), |p| s + p)
    };
    let mut i = start;
    while i < n.len() {
        match schemaic_core::sql::skip_noncode(n, i, dialect) {
            Some(end) => {
                let end = end.min(n.len());
                let kind = schemaic_core::sql::noncode_kind(n, i, dialect);
                for p in (i..end).filter(|&p| n[p] == b'\n') {
                    let s = p + 1;
                    if s < end {
                        out.push(Some(LineCarry {
                            kind,
                            end: (end <= line_end(s)).then(|| end - s),
                        }));
                    } else {
                        out.push(None);
                        if resync(&mut out, s) {
                            return out;
                        }
                    }
                }
                i = end.max(i + 1);
            }
            None => {
                if n[i] == b'\n' {
                    out.push(None);
                    if resync(&mut out, i + 1) {
                        return out;
                    }
                }
                i += 1;
            }
        }
    }
    out
}

/// For every line of `text`, the non-code region it starts inside, if any
/// ([`LineCarry`]) — every kind `skip_noncode` knows, not `/* … */` alone,
/// which was all this once carried. A per-line lexer cannot see a string that
/// opened above it, so a multi-line string's second line read as code and a
/// PostgreSQL function's `$$` body as keywords; nested comments close where
/// the dialect says they do. `[0]` is always `None` — line 0 has nothing
/// before it.
///
/// One forward pass for the whole document, rather than re-lexing the prefix
/// per line: the editor asks this ~45 times per relayout and a relayout
/// happens on every keystroke, so the per-line form was Θ(visible × document).
/// Built on the shared `schemaic_core::sql::skip_noncode` primitive, so every
/// boundary agrees with the rest of the SQL tooling. After an edit,
/// [`carried_lines_after`] answers the same from the previous answer.
pub(crate) fn carried_lines(text: &str, dialect: SqlDialect) -> Vec<Option<LineCarry>> {
    let b = text.as_bytes();
    let n = b.len();
    // Every non-code region that spans a line break; an unterminated one runs
    // to EOF.
    let mut regions: Vec<(usize, usize, Option<NonCode>)> = Vec::new();
    let mut i = 0;
    while i < n {
        match schemaic_core::sql::skip_noncode(b, i, dialect) {
            Some(end) => {
                let end = end.min(n);
                if b[i..end].contains(&b'\n') {
                    regions.push((i, end, schemaic_core::sql::noncode_kind(b, i, dialect)));
                }
                i = end.max(i + 1);
            }
            None => i += 1,
        }
    }
    let mut carries = vec![None];
    let mut r = 0;
    let breaks: Vec<usize> = b
        .iter()
        .enumerate()
        .filter(|&(_, &c)| c == b'\n')
        .map(|(p, _)| p)
        .collect();
    for (k, &p) in breaks.iter().enumerate() {
        let start = p + 1;
        let line_end = breaks.get(k + 1).copied().unwrap_or(n);
        while r < regions.len() && regions[r].1 <= start {
            r += 1;
        }
        // *Strictly* inside: at its end the region closed on the line above,
        // and at its start this line opens it (which `lex_line_with` handles).
        carries.push(
            regions
                .get(r)
                .filter(|&&(lo, hi, _)| lo < start && start < hi)
                .map(|&(_, hi, kind)| LineCarry {
                    kind,
                    end: (hi <= line_end).then(|| hi - start),
                }),
        );
    }
    carries
}

/// Public: color spans for a standalone SQL line (byte ranges + color), for
/// callers outside the editor — e.g. the Ctrl+K diff, which renders each line as
/// colored segments rather than through the editor's `Styling` hook. Same lexer
/// as the editor, so highlighting matches exactly.
pub fn highlight_spans(line: &str, dialect: SqlDialect) -> Vec<(usize, usize, Color)> {
    // Standalone line (Ctrl+K diff) — no document context, so no cross-line block
    // state; a multi-line comment there stays coloured per-line.
    let brackets = crate::theme::editor_rainbow_brackets().then_some(0);
    lex_line_with(line, dialect, None, brackets)
        .into_iter()
        .map(|(s, e, tok)| (s, e, tok.color()))
        .collect()
}

/// [`highlight_spans`] for a run of lines read as one text — the Ctrl+K
/// diff's added rows — so a string or comment one line opens is carried into
/// the next ([`carried_lines`]). One `Vec` of spans per line, in order.
pub fn highlight_lines<S: AsRef<str>>(
    lines: &[S],
    dialect: SqlDialect,
) -> Vec<Vec<(usize, usize, Color)>> {
    highlight_lines_in(lines, dialect, &[])
}

/// [`highlight_lines`] for rows whose carries are already known — each
/// row's from the document it sits in ([`carried_lines`] over that
/// document), which is what lets a comment opened above them carry in. A
/// `context` that is not one carry per row is ignored, and the rows are lexed
/// as a text of their own.
pub(crate) fn highlight_lines_in<S: AsRef<str>>(
    lines: &[S],
    dialect: SqlDialect,
    context: &[Option<LineCarry>],
) -> Vec<Vec<(usize, usize, Color)>> {
    let carries = if context.len() == lines.len() && !lines.is_empty() {
        context.to_vec()
    } else {
        let text = lines
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<_>>()
            .join("\n");
        carried_lines(&text, dialect)
    };
    let brackets = crate::theme::editor_rainbow_brackets().then_some(0);
    lines
        .iter()
        .zip(carries)
        .map(|(line, carry)| {
            lex_line_with(line.as_ref(), dialect, carry, brackets)
                .into_iter()
                .map(|(s, e, tok)| (s, e, tok.color()))
                .collect()
        })
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
/// `carry` = the non-code region this line begins inside, opened on an earlier
/// line ([`carried_lines`]): its leading text up to the region's close (or the
/// whole line, if it doesn't close here) takes that region's colour before
/// normal lexing resumes.
///
/// `brackets` is the depth the line starts at (`pairs::bracket_depth_at_lines`)
/// with rainbow brackets on: each code bracket is then coloured by its nesting.
/// `None` leaves brackets alone.
fn lex_line_with(
    line: &str,
    dialect: SqlDialect,
    carry: Option<LineCarry>,
    brackets: Option<usize>,
) -> Vec<(usize, usize, Tok)> {
    let mut depth = brackets;
    let b = line.as_bytes();
    let n = b.len();
    let mut out = Vec::new();
    let mut i = 0;

    // The rest of a region opened on an earlier line: coloured as its kind to
    // its close, or the whole line when it runs on. A quoted name keeps the
    // default colour, as one opened on this line does.
    if let Some(c) = carry {
        let end = c.end.unwrap_or(n).min(n);
        let tok = match c.kind {
            Some(NonCode::Literal) => Some(Tok::Str),
            Some(NonCode::Comment) => Some(Tok::Comment),
            Some(NonCode::Identifier) | None => None,
        };
        if let Some(tok) = tok
            && end > 0
        {
            out.push((0, end, tok));
        }
        if c.end.is_none() {
            return out;
        }
        i = end;
    } else if let Some((lo, hi)) = schemaic_core::sql::line_directive_span(line, dialect) {
        // A client directive — `GO`, `DELIMITER` — in the keyword colour with
        // its argument, as SSMS and DataGrip colour it; a `GO` line's comment
        // is lexed below as any other.
        out.push((lo, hi, Tok::Keyword));
        i = hi;
    }

    // The `:name` placeholders the parameters bar fills, by its own rule
    // (`params::scan`) — which already tells a cast, a `:=` and a slice from
    // one — over the line from where code starts.
    let from = i;
    let mut params = if line[from..].contains(':') {
        schemaic_core::params::scan(&line[from..], dialect)
    } else {
        Vec::new()
    }
    .into_iter()
    .map(|p| (from + p.start, from + p.end))
    .peekable();

    while i < n {
        let c = b[i];

        // One the lexer has stepped past is not waited for, or it would hold
        // back every placeholder after it on the line.
        while params.peek().is_some_and(|&(lo, _)| lo < i) {
            params.next();
        }
        if let Some(&(lo, hi)) = params.peek()
            && lo == i
        {
            params.next();
            out.push((lo, hi, Tok::Variable));
            i = hi;
            continue;
        }

        // A string, identifier, dollar-quote, or comment: color by which one it
        // is — asked of `core::sql::noncode_kind`, which answers with the same
        // per-dialect predicates the lexer used to *find* the span.
        //
        // This was a byte `match` with no dialect in it: `` ` `` took the
        // identifier arm, `'`/`"` the string arm, and **everything else** the
        // comment arm. `skip_noncode` also answers `Some` for `$` on PostgreSQL
        // and `[` on SQLite, so both landed there — every dollar-quoted
        // function body and `DO` block in a PostgreSQL editor was greyed out as
        // if commented, and `"Name"` painted as a string on the two engines
        // where it is a name. The `dialect` field above promised the opposite.
        if let Some(end) = schemaic_core::sql::skip_noncode(b, i, dialect) {
            let end = end.min(n);
            match schemaic_core::sql::noncode_kind(b, i, dialect) {
                // A quoted identifier is a name: default colour, like a bare one.
                Some(NonCode::Identifier) | None => {}
                Some(NonCode::Literal) => out.push((i, end, Tok::Str)),
                Some(NonCode::Comment) => out.push((i, end, Tok::Comment)),
            }
            i = end;
            continue;
        }
        // number literal, whole: `0x1F`, `1.5E-3`
        if c.is_ascii_digit() {
            let j = number_end(b, i);
            out.push((i, j, Tok::Number));
            i = j;
            continue;
        }
        // A variable — T-SQL's and MySQL's `@x`, `@@version` — or a T-SQL
        // temporary table (`#t`, `##t`) is a name, so `@select` and `#order`
        // are no keywords. The variable wears the variable colour; the table
        // keeps the default, as a bare one does.
        if c == b'@' || t_sql_name_prefix(b, i, dialect) > 0 {
            let mut j = i + 1;
            while j < n && (matches!(b[j], b'@' | b'#') || is_word_byte(b[j])) {
                j += 1;
            }
            if c == b'@' {
                out.push((i, j, Tok::Variable));
            }
            i = j;
            continue;
        }
        // word: keyword or identifier
        //
        // **`core::sql`'s predicates, not an ASCII rule** — architecture
        // invariant 11, and this was its first site outside `core/sql.rs`. The
        // ASCII spelling stepped over each byte of a multi-byte character with
        // the `i += 1` fallthrough and then lexed the ASCII tail as a fresh
        // word, so `SELECT éselect FROM t` painted `select` as a keyword
        // *inside* a legal column name — and the same spans reach the Ctrl+K
        // diff through `highlight_spans`. It surfaced only when a keyword
        // happened to follow the non-ASCII byte (`naïve_from`'s tail is not
        // one), which is why nobody saw it. `is_keyword` folds ASCII only, so a
        // whole non-ASCII word now correctly matches nothing.
        //
        // A name runs on through the engine's own continuation bytes (`a$from`
        // is one name), and a word after a `.` is a member (`t.order`): neither
        // is a keyword however it is spelled. One letter glued to a `'` is a
        // literal's prefix (`N'…'`, `E'…'`, `X'…'`, `B'…'`), coloured with it.
        if is_word_start(c) {
            let mut j = i + 1;
            while j < n && (is_word_byte(b[j]) || continues_name(b[j], dialect)) {
                j += 1;
            }
            let member = i > 0 && b[i - 1] == b'.';
            if j == i + 1
                && matches!(c.to_ascii_uppercase(), b'N' | b'E' | b'X' | b'B')
                && b.get(j) == Some(&b'\'')
            {
                out.push((i, j, Tok::Str));
            } else if !member {
                // The engine's own reading first (`intel::word_class`): a
                // called builtin, a type where only a type can stand and a
                // constant take their own colours — `COUNT(` and `VARCHAR(40)`
                // too, which the fixed list below holds as keywords. Then the
                // fixed list, and the words the engine reserves.
                use schemaic_core::intel::WordClass;
                let tok = match schemaic_core::intel::word_class(line, i, j, dialect) {
                    Some(WordClass::Function) => Some(Tok::Function),
                    Some(WordClass::Type) => Some(Tok::Type),
                    Some(WordClass::Constant) => Some(Tok::Constant),
                    _ if is_keyword(&line[i..j]) => Some(Tok::Keyword),
                    Some(WordClass::Keyword) => Some(Tok::Keyword),
                    None => None,
                };
                if let Some(tok) = tok {
                    out.push((i, j, tok));
                }
            }
            i = j;
            continue;
        }

        // A code bracket, by its nesting: an opener at the depth it opens, a
        // closer at its opener's.
        if let Some(d) = depth.as_mut()
            && matches!(c, b'(' | b'[' | b')' | b']')
        {
            let at = if matches!(c, b'(' | b'[') {
                *d += 1;
                *d - 1
            } else {
                *d = d.saturating_sub(1);
                *d
            };
            out.push((i, i + 1, Tok::Bracket(at)));
        }

        i += 1;
    }

    out
}

/// Where the number starting at `b[i]` (a digit) ends: a hex literal `0x1F`,
/// or digits with an optional fraction and exponent (`1.5E-3`) — an `e` is the
/// exponent's only with a digit after it, so `1end` stops at `1`.
fn number_end(b: &[u8], i: usize) -> usize {
    let n = b.len();
    let digits = |mut j: usize, ok: fn(&u8) -> bool| {
        while j < n && ok(&b[j]) {
            j += 1;
        }
        j
    };
    if b[i] == b'0' && matches!(b.get(i + 1), Some(b'x' | b'X')) {
        let j = digits(i + 2, u8::is_ascii_hexdigit);
        if j > i + 2 {
            return j;
        }
    }
    let mut j = digits(i, u8::is_ascii_digit);
    if b.get(j) == Some(&b'.') {
        j = digits(j + 1, u8::is_ascii_digit);
    }
    if matches!(b.get(j), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(b.get(j + 1), Some(b'+' | b'-')));
        let k = digits(j + 1 + sign, u8::is_ascii_digit);
        if k > j + 1 + sign {
            j = k;
        }
    }
    j
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

    /// [`lex_line_with`] with rainbow brackets off; `start_in_block` is a
    /// `/* … */` carried in from above, ending at the line's first `*/`.
    fn lex_line(line: &str, dialect: SqlDialect, start_in_block: bool) -> Vec<(usize, usize, Tok)> {
        let carry = start_in_block.then(|| LineCarry {
            kind: Some(NonCode::Comment),
            end: line.find("*/").map(|p| p + 2),
        });
        lex_line_with(line, dialect, carry, None)
    }

    /// Each line of `text` lexed with what [`carried_lines`] carries into
    /// it, as `(line, the span's text, its kind)` for strings and comments.
    fn doc_spans(text: &str, dialect: SqlDialect) -> Vec<(usize, String, &'static str)> {
        let carries = carried_lines(text, dialect);
        let mut out = Vec::new();
        for (n, line) in text.split('\n').enumerate() {
            for (s, e, t) in lex_line_with(line, dialect, carries[n], None) {
                let kind = match t {
                    Tok::Str => "string",
                    Tok::Comment => "comment",
                    Tok::Keyword => "keyword",
                    _ => continue,
                };
                out.push((n, line[s..e].to_string(), kind));
            }
        }
        out
    }

    /// Every `(old, new)` a corpus of edits makes: each base document, edited
    /// at every byte by inserting each snippet and by deleting one or three
    /// bytes — the edits that open and close comments, strings and dollar
    /// quotes, and add and remove lines.
    pub(crate) fn edit_corpus() -> Vec<(String, String)> {
        let bases = [
            "select 1;\n/* a\nb */ select (2);\nselect 'x\ny' from t;\n-- c\nselect [3];\n",
            "select $$a\nb$$, $q$c\n$q$;\n\nselect (1,\n 2)\n;",
            "a\n\n(b\n)\n'c\n'\n/*\n*/\n",
        ];
        let snippets = ["/*", "*/", "'", "\n", "x", "(", ")", "$$", "--", "\n\n"];
        let mut out = Vec::new();
        for base in bases {
            for at in 0..=base.len() {
                if !base.is_char_boundary(at) {
                    continue;
                }
                for s in snippets {
                    out.push((
                        base.to_string(),
                        format!("{}{s}{}", &base[..at], &base[at..]),
                    ));
                }
                for n in [1, 3] {
                    if at + n <= base.len() {
                        out.push((
                            base.to_string(),
                            format!("{}{}", &base[..at], &base[at + n..]),
                        ));
                    }
                }
            }
        }
        out
    }

    /// **The incremental pass answers what the whole pass does**, for every
    /// edit of the corpus, on every dialect.
    #[test]
    fn carried_lines_after_an_edit_match_a_full_pass() {
        for d in [
            SqlDialect::MySql,
            SqlDialect::Postgres,
            SqlDialect::MsSql,
            SqlDialect::Sqlite,
        ] {
            for (old, new) in edit_corpus() {
                let prev = carried_lines(&old, d);
                assert_eq!(
                    carried_lines_after(&old, &prev, &new, d),
                    carried_lines(&new, d),
                    "{d:?}: {old:?} -> {new:?}"
                );
            }
        }
    }

    /// **A run of lines is lexed as one text**, for the Ctrl+K diff's added
    /// rows: a comment one row opens colours the next, which `highlight_spans`
    /// lexing each row alone could not.
    #[test]
    fn a_run_of_lines_carries_its_regions() {
        let lines = ["select 1 /* first", "still a comment */ from t"];
        let spans = highlight_lines(&lines, SqlDialect::MySql);
        let comment = crate::theme::editor_theme().comment;
        assert_eq!(spans.len(), 2);
        assert!(
            spans[1]
                .iter()
                .any(|&(s, e, c)| (s, e) == (0, 18) && c == comment),
            "{:?}",
            spans[1]
        );
        assert_eq!(highlight_lines(&[] as &[&str], SqlDialect::MySql).len(), 0);
    }

    /// **A string, a dollar-quoted body and a nested comment carry across
    /// lines** as a `/* … */` always did: the lines inside them take their
    /// colour, and what follows the close on its line is code again. They
    /// were lexed per line, so a multi-line string's second line read as
    /// code and a function body's `begin` as a keyword.
    #[test]
    fn every_noncode_region_carries_across_lines() {
        use SqlDialect::*;
        let s = doc_spans("select 'a\nfrom b' from t", MySql);
        assert!(s.contains(&(1, "from b'".to_string(), "string")), "{s:?}");
        assert!(s.contains(&(1, "from".to_string(), "keyword")), "{s:?}");
        assert!(!s.contains(&(1, "from b".to_string(), "keyword")), "{s:?}");
        let pg = doc_spans("do $$\nbegin\nend $$; select 1", Postgres);
        assert!(pg.contains(&(1, "begin".to_string(), "string")), "{pg:?}");
        assert!(pg.contains(&(2, "end $$".to_string(), "string")), "{pg:?}");
        assert!(pg.contains(&(2, "select".to_string(), "keyword")), "{pg:?}");
        // PostgreSQL's comments nest; MySQL's do not.
        let nested = "/* a /* b */\nc */ select 1";
        let pg = doc_spans(nested, Postgres);
        assert!(pg.contains(&(1, "c */".to_string(), "comment")), "{pg:?}");
        let my = doc_spans(nested, MySql);
        assert!(
            !my.iter().any(|(n, _, k)| *n == 1 && *k == "comment"),
            "{my:?}"
        );
    }

    /// The text of every span of one kind, on one dialect.
    ///
    /// **Every other test in this block is `SqlDialect::MySql`**, which is why
    /// the classification of a non-code span could be dialect-blind with the
    /// suite green: MySQL is the one engine where `"…"` really is a string,
    /// `$$` opens nothing and `[` is ordinary punctuation, so its answers were
    /// right by construction. The `dialect` parameter could have been replaced
    /// by a constant.
    fn spans_of(line: &str, dialect: SqlDialect, want: &str) -> Vec<String> {
        lex_line(line, dialect, false)
            .into_iter()
            .filter(|(_, _, t)| {
                matches!(
                    (t, want),
                    (Tok::Comment, "comment")
                        | (Tok::Str, "string")
                        | (Tok::Keyword, "keyword")
                        | (Tok::Number, "number")
                        | (Tok::Variable, "variable")
                )
            })
            .map(|(s, e, _)| line[s..e].to_string())
            .collect()
    }

    /// A client directive line is coloured as a keyword, its argument with it
    /// — `GO 5`, `DELIMITER $$` — on the engine that has it; a `GO` line's
    /// comment keeps the comment colour, and a line inside a block comment
    /// is comment whatever it says.
    #[test]
    fn a_directive_line_is_coloured_as_a_keyword() {
        use SqlDialect::{MsSql, MySql, Postgres};
        assert_eq!(spans_of("GO", MsSql, "keyword"), ["GO"]);
        assert_eq!(spans_of("  go 5 -- next", MsSql, "keyword"), ["go 5"]);
        assert_eq!(spans_of("  go 5 -- next", MsSql, "comment"), ["-- next"]);
        assert_eq!(spans_of("DELIMITER $$", MySql, "keyword"), ["DELIMITER $$"]);
        assert_eq!(spans_of("DELIMITER ;", MySql, "keyword"), ["DELIMITER ;"]);
        assert!(spans_of("GO", MySql, "keyword").is_empty());
        assert!(spans_of("DELIMITER $$", Postgres, "keyword").is_empty());
        let in_block = lex_line("GO", MsSql, true);
        assert!(
            in_block.iter().all(|&(_, _, t)| matches!(t, Tok::Comment)),
            "a GO inside a block comment"
        );
    }

    /// A keyword spelling that is a member, a variable or part of a longer
    /// name is a name: `t.order`, `@select`, `@@version`, `a$from`.
    #[test]
    fn a_keyword_spelling_after_a_dot_an_at_or_in_a_dollar_name_is_not_coloured() {
        use SqlDialect::{MsSql, MySql, Postgres};
        assert_eq!(
            spans_of("SELECT t.order FROM t", MySql, "keyword"),
            ["SELECT", "FROM"]
        );
        assert_eq!(
            spans_of("SELECT @select, @@version", MsSql, "keyword"),
            ["SELECT"]
        );
        assert_eq!(spans_of("SET @from = 1", MySql, "keyword"), ["SET"]);
        assert_eq!(
            spans_of("SELECT * FROM #order", MsSql, "keyword"),
            ["SELECT", "FROM"]
        );
        assert_eq!(
            spans_of("SELECT a$from FROM t", Postgres, "keyword"),
            ["SELECT", "FROM"]
        );
    }

    /// A variable — `@x`, `@@version` — and a `:name` placeholder the
    /// parameters bar fills (`params::scan`) wear the variable colour; a
    /// PostgreSQL cast, MySQL's `:=`, an array slice and a `:name` in a string
    /// do not, nor a T-SQL temporary table.
    #[test]
    fn variables_and_placeholders_are_coloured() {
        use SqlDialect::{MsSql, MySql, Postgres};
        assert_eq!(
            spans_of("SELECT @total, @@version FROM #t", MsSql, "variable"),
            ["@total", "@@version"]
        );
        assert_eq!(spans_of("SET @n := 1", MySql, "variable"), ["@n"]);
        assert_eq!(
            spans_of(
                "SELECT * FROM t WHERE id = :id LIMIT :limit",
                MySql,
                "variable"
            ),
            [":id", ":limit"]
        );
        assert_eq!(
            spans_of(
                "SELECT * FROM t WHERE id = :id LIMIT :limit",
                MySql,
                "keyword"
            ),
            ["SELECT", "FROM", "WHERE", "LIMIT"],
            "a placeholder named like a keyword is a placeholder"
        );
        assert!(spans_of("SELECT id::text, a[lo:hi], ':x'", Postgres, "variable").is_empty());
        let in_block = lex_line("WHERE id = :id */ AND a = :b", MySql, true);
        let vars: Vec<_> = in_block
            .iter()
            .filter(|(_, _, t)| matches!(t, Tok::Variable))
            .map(|&(s, e, _)| &"WHERE id = :id */ AND a = :b"[s..e])
            .collect();
        assert_eq!(vars, [":b"], "past a block comment's end only");
    }

    /// The engine's own reading wins over the fixed list: a called builtin is
    /// a function, `VARCHAR(40)` a type, `TRUE` a constant — and `LEFT` before
    /// `JOIN`, not called, stays a keyword.
    #[test]
    fn functions_types_and_constants_take_their_own_colours() {
        let line = "SELECT COUNT(x), CAST(y AS VARCHAR(40)), TRUE FROM a LEFT JOIN b ON true";
        let kinds: Vec<(&str, Tok)> = lex_line(line, SqlDialect::MySql, false)
            .into_iter()
            .map(|(s, e, t)| (&line[s..e], t))
            .collect();
        let of = |w: &str| kinds.iter().find(|(x, _)| *x == w).map(|(_, t)| *t);
        assert_eq!(of("SELECT"), Some(Tok::Keyword));
        assert_eq!(of("COUNT"), Some(Tok::Function));
        assert_eq!(of("CAST"), Some(Tok::Function));
        assert_eq!(of("VARCHAR"), Some(Tok::Type));
        assert_eq!(of("TRUE"), Some(Tok::Constant));
        assert_eq!(of("LEFT"), Some(Tok::Keyword));
        assert_eq!(of("x"), None);
    }

    /// With rainbow brackets on, a code bracket is coloured by its nesting —
    /// an opener by the depth it opens at, a closer by its opener's — starting
    /// from the depth the line begins at; one in a string, comment or quoted
    /// name is not a bracket. Off, no bracket is coloured.
    #[test]
    fn rainbow_brackets_are_coloured_by_depth() {
        let depths = |line: &str, start: usize, d: SqlDialect| -> Vec<(String, usize)> {
            lex_line_with(line, d, None, Some(start))
                .into_iter()
                .filter_map(|(s, e, t)| match t {
                    Tok::Bracket(n) => Some((line[s..e].to_string(), n)),
                    _ => None,
                })
                .collect()
        };
        let pair = |s: &str, n: usize| (s.to_string(), n);
        assert_eq!(
            depths("f(a, g(b), '(') [x]", 0, SqlDialect::MsSql),
            [pair("(", 0), pair("(", 1), pair(")", 1), pair(")", 0)]
        );
        assert_eq!(
            depths("  c))", 2, SqlDialect::MySql),
            [pair(")", 1), pair(")", 0)]
        );
        assert_eq!(depths(")", 0, SqlDialect::MySql), [pair(")", 0)]);
        assert!(
            lex_line_with("f(a)", SqlDialect::MySql, None, None)
                .iter()
                .all(|(_, _, t)| !matches!(t, Tok::Bracket(_)))
        );
    }

    /// A number is coloured whole — exponent, hex — and a literal's prefix
    /// with its string.
    #[test]
    fn whole_numbers_and_literal_prefixes_are_coloured() {
        use SqlDialect::{MsSql, MySql, Postgres};
        assert_eq!(
            spans_of("SELECT 1e10, 1.5E-3, 0x1F, 42", MySql, "number"),
            ["1e10", "1.5E-3", "0x1F", "42"]
        );
        assert_eq!(spans_of("SELECT N'é'", MsSql, "string"), ["N", "'é'"]);
        assert_eq!(
            spans_of("SELECT E'\\n'", Postgres, "string"),
            ["E", "'\\n'"]
        );
        assert_eq!(spans_of("SELECT X'0A'", MySql, "string"), ["X", "'0A'"]);
        // A name ending in such a letter is not a prefix.
        assert_eq!(spans_of("SELECT colN 'x'", MySql, "string"), ["'x'"]);
    }

    /// **A non-code span is coloured by what it is on *this* engine.**
    ///
    /// `lex_line` asked the dialect-aware lexer where the span was and then
    /// classified it by its opening byte with no dialect: `` ` `` was an
    /// identifier, `'`/`"` a string, and **everything else** a comment.
    /// `skip_noncode` also answers `Some` for `$` on PostgreSQL and `[` on
    /// SQLite, so both landed on the comment arm — every dollar-quoted function
    /// body and `DO` block in a PostgreSQL editor was greyed out as if
    /// commented. The field doc promised exactly the opposite.
    #[test]
    fn a_quoted_span_is_classified_per_dialect() {
        use SqlDialect::{MySql, Postgres, Sqlite};

        // A dollar-quoted body is a *literal* on PostgreSQL, and nothing at all
        // on the two engines that have no such syntax.
        let pg_body = "SELECT $body$ SELECT 1; $body$";
        assert_eq!(
            spans_of(pg_body, Postgres, "string"),
            ["$body$ SELECT 1; $body$"]
        );
        assert!(spans_of(pg_body, Postgres, "comment").is_empty());
        for d in [MySql, Sqlite] {
            assert!(spans_of(pg_body, d, "comment").is_empty(), "{d:?}");
            assert!(spans_of(pg_body, d, "string").is_empty(), "{d:?}");
        }

        // A bracket-quoted identifier is a name on SQLite: neither colour.
        let brackets = "SELECT [my col] FROM t";
        assert!(spans_of(brackets, Sqlite, "comment").is_empty());
        assert!(spans_of(brackets, Sqlite, "string").is_empty());

        // `"…"` is a string on MySQL and an identifier on the other two.
        let dq = r#"SELECT "Name" FROM t"#;
        assert_eq!(spans_of(dq, MySql, "string"), [r#""Name""#]);
        for d in [Postgres, Sqlite] {
            assert!(spans_of(dq, d, "string").is_empty(), "{d:?}");
            assert!(spans_of(dq, d, "comment").is_empty(), "{d:?}");
        }

        // A backtick identifier stays a name where it is one.
        let bt = "SELECT `Name` FROM t";
        for d in [MySql, Sqlite] {
            assert!(spans_of(bt, d, "string").is_empty(), "{d:?}");
            assert!(spans_of(bt, d, "comment").is_empty(), "{d:?}");
        }

        // The three comment openers still colour as comments where each is one,
        // which is what the `_` arm was reaching for.
        assert_eq!(spans_of("SELECT 1 -- note", MySql, "comment"), ["-- note"]);
        assert_eq!(
            spans_of("SELECT 1 /* n */", Postgres, "comment"),
            ["/* n */"]
        );
        assert_eq!(spans_of("SELECT 1 # note", MySql, "comment"), ["# note"]);
        // …and `#` is an operator on PostgreSQL, not a comment. (The one half of
        // the field doc's promise that was already true.)
        assert!(spans_of("SELECT 1 # 2", Postgres, "comment").is_empty());
        // An ordinary string literal is a string everywhere.
        for d in [MySql, Postgres, Sqlite] {
            assert_eq!(spans_of("SELECT 'x' FROM t", d, "string"), ["'x'"], "{d:?}");
        }
    }

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

    /// **Architecture invariant 11, asked of the scanner.** The invariant's own
    /// tests in `core/sql.rs` assert the two predicates and nothing that uses
    /// them, which is why this file's ASCII word rule was green: the ASCII tail
    /// of a non-ASCII identifier was lexed as a fresh word and
    /// `SELECT éselect FROM t` painted `select` as a keyword inside a legal
    /// column name. The same spans reach the Ctrl+K diff.
    #[test]
    fn a_keyword_inside_a_non_ascii_identifier_is_not_a_keyword() {
        let keywords = |line: &str| -> Vec<String> {
            lex_line(line, SqlDialect::MySql, false)
                .into_iter()
                .filter(|(_, _, t)| matches!(t, Tok::Keyword))
                .map(|(s, e, _)| line[s..e].to_string())
                .collect()
        };
        assert_eq!(keywords("SELECT éselect FROM t"), ["SELECT", "FROM"]);
        // The counterweight that hid it: a tail that is not a keyword was
        // always painted correctly, so only some identifiers showed the bug.
        assert_eq!(keywords("SELECT naïve_from FROM t"), ["SELECT", "FROM"]);
        // A whole non-ASCII word, and one whose *head* is a keyword.
        assert_eq!(keywords("SELECT 日本語 FROM t"), ["SELECT", "FROM"]);
        assert_eq!(keywords("SELECT selecté FROM t"), ["SELECT", "FROM"]);
        // And an ordinary keyword is still one.
        assert_eq!(keywords("select 1 where a"), ["select", "where"]);
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

    /// **Both theme axes move the layout cache key, and the font size moves it
    /// independently of either.**
    ///
    /// A colour baked into a cached `TextLayoutLine` cannot re-evaluate; the
    /// only thing that repaints it is this key changing. The diff bands come
    /// from the UI axis and the key knew only the editor one, so an
    /// interface-theme switch repainted the gutter strips and left the bands
    /// behind the code in the old palette.
    #[test]
    fn the_layout_cache_key_moves_on_either_theme_axis() {
        let my = SqlDialect::MySql;
        let base = layout_cache_key(3, 7, 14.0, my);
        assert_ne!(base, layout_cache_key(4, 7, 14.0, my), "the editor axis");
        assert_ne!(base, layout_cache_key(3, 8, 14.0, my), "the interface axis");
        assert_ne!(base, layout_cache_key(3, 7, 15.0, my), "the font size");
        // A tab moved to another engine re-lexes: every dialect its own key.
        let keys: Vec<u64> = SqlDialect::ALL
            .iter()
            .map(|&d| layout_cache_key(3, 7, 14.0, d))
            .collect();
        for (i, k) in keys.iter().enumerate() {
            assert!(!keys[..i].contains(k), "two dialects share a key");
        }

        // The property that matters is that the key never *returns* to a value
        // it has already had, since floem's cache is keyed on it: both counters
        // only climb, so the sum climbs with them. An XOR would not — `2^3 == 1`
        // and `0^1 == 1`, two different states one bump apart sharing a key.
        let mut prev = layout_cache_key(0, 0, 14.0, my);
        let mut seen = vec![prev];
        for (e, u) in [(1, 0), (1, 1), (2, 1), (2, 4), (9, 4)] {
            let k = layout_cache_key(e, u, 14.0, my);
            assert!(k > prev, "the key went backwards at ({e}, {u})");
            assert!(!seen.contains(&k), "the key repeated at ({e}, {u})");
            seen.push(k);
            prev = k;
        }
    }

    /// The composition, through the accessors the styling actually calls: a
    /// **real** interface-theme switch has to move the key. Asserting
    /// `layout_cache_key` alone would pass with `id()` still handing it a
    /// constant for `ui_gen`, which is exactly the bug.
    #[test]
    fn switching_the_interface_theme_moves_the_key_the_styling_returns() {
        use crate::themes;
        let key = || {
            layout_cache_key(
                crate::theme::editor_generation(),
                crate::theme::ui_generation(),
                14.0,
                SqlDialect::MySql,
            )
        };
        let before = key();
        themes::set_ui(themes::UiThemeKind::Light);
        let after_light = key();
        assert_ne!(before, after_light, "set_ui did not move the key");
        themes::set_ui(themes::UiThemeKind::Dark);
        assert_ne!(after_light, key(), "and back again");

        // `id()` must be spelled over both accessors, not just this test.
        let src = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join("sql_highlight.rs"),
        )
        .expect("sql_highlight.rs");
        let body = crate::source_gate::production_code(&src);
        let at = body
            .find("fn id(&self)")
            .expect("`id` is gone — gate is stale");
        let end = at + body[at..].find("\n    }").expect("`id`'s end");
        let f = &body[at..end];
        for term in [
            "layout_cache_key(",
            "editor_generation()",
            "ui_generation()",
            // Tracked, so a tab moved to another engine re-lexes its lines.
            "self.dialect.get()",
        ] {
            assert!(
                f.contains(term),
                "`Styling::id` no longer reads {term} — a colour from that axis \
                 is baked into the cached layout and can never repaint"
            );
        }
    }
}
