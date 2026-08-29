//! In-place rendering of an inline-AI (Ctrl+K) suggestion.
//!
//! The suggestion is shown *in the editor's own line flow* — the lines it
//! replaces stay where they are and the lines it proposes appear directly below
//! them, pushing the rest of the document down — rather than in a box floating
//! over the editor. That is the whole point of the design: the user sees exactly
//! what is being replaced and where.
//!
//! **The document is never touched to do it.** The added rows are Floem
//! *phantom text* (the facility inlay hints use): text that is combined into a
//! line's layout but is not in the rope. `doc.text()` keeps returning the user's
//! own SQL for the entire preview, so tab autosave, Ctrl+Enter, live validation,
//! completion and the outline all keep seeing the buffer the user actually has.
//! Splicing the preview into the rope instead would have been far less code and
//! would have put text the user never wrote in front of every one of those.
//!
//! Two halves, sharing one [`InlinePlan`] signal:
//!
//! - [`InlineDiffDoc`] — a `Document` that wraps the editor's real one and
//!   answers `phantom_text` with the added rows. Everything else delegates.
//! - [`segments`] — the row builder, which colours the block's text. Only
//!   `phantom_text` needs that; [`block_at`] answers the cheap question (is there
//!   a block, which side, how long) for the styling and the gutter strips, which
//!   ask it per line on every relayout.
//!
//! Floem calls `Styling::apply_attr_styles` with the line's *pre-phantom*
//! columns and only then adds the phantom spans, so an end-of-line block (the
//! normal case) leaves the real line's highlighting alone. A block that renders
//! *before* line 0 — the one insertion with no preceding line to hang off — does
//! shift it, which is what [`Block::prefix_len`] exists to tell the styling.

use std::rc::Rc;

use floem::keyboard::Modifiers;
use floem::peniko::Color;
use floem::reactive::{RwSignal, SignalGet, SignalUpdate, SignalWith};
use floem::views::editor::Editor;
use floem::views::editor::EditorStyle;
use floem::views::editor::command::{Command, CommandExecuted};
use floem::views::editor::core::buffer::rope_text::RopeText;
use floem::views::editor::core::editor::EditType;
use floem::views::editor::core::selection::Selection;
use floem::views::editor::core::xi_rope::Rope;
use floem::views::editor::id::EditorId;
use floem::views::editor::layout::TextLayoutLine;
use floem::views::editor::phantom_text::{PhantomText, PhantomTextKind, PhantomTextLine};
use floem::views::editor::text::{Document, DocumentPhantom, PreeditData};
use schemaic_core::diff::InlinePlan;
use schemaic_core::intel::SqlDialect;

use crate::sql_highlight::highlight_spans;

/// What the editor is currently showing about a Ctrl+K request, or `None` when
/// it is showing nothing.
///
/// One signal, read by [`InlineDiffDoc`] (which turns it into phantom rows) and
/// by `SqlStyling` (which fades lines and paints the row bands).
pub type InlinePreview = RwSignal<Option<InlineView>>;

/// The two things a Ctrl+K request puts on the editor's own surface.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum InlineView {
    /// The model is working on these document lines (half-open). They fade to
    /// say so; nothing has been proposed yet, so no rows are added and no band
    /// is painted.
    Working(std::ops::Range<usize>),
    /// A settled suggestion: the lines it replaces are faded and banded, and its
    /// own lines are added after them as phantom rows.
    Plan(InlinePlan),
}

impl InlineView {
    /// The settled plan, if this is one.
    pub fn plan(&self) -> Option<&InlinePlan> {
        match self {
            InlineView::Plan(p) => Some(p),
            InlineView::Working(_) => None,
        }
    }

    /// Is `line` one of the lines this view fades — because it is being worked
    /// on, or because a suggestion replaces it?
    pub fn fades(&self, line: usize) -> bool {
        match self {
            InlineView::Working(lines) => lines.contains(&line),
            InlineView::Plan(p) => p.hunks.iter().any(|h| h.del.contains(&line)),
        }
    }

    /// How far `line` fades. The two states are deliberately different depths:
    /// waiting is a stronger dim than replaced, because while waiting the faded
    /// text is *all* there is to look at, and once the suggestion lands the
    /// removed lines still have to be readable against the new ones.
    pub fn fade(&self) -> f32 {
        match self {
            InlineView::Working(_) => 0.45,
            InlineView::Plan(_) => 0.65,
        }
    }
}

/// The phantom rows for one document line, already split into coloured runs.
pub struct Segments {
    /// `(text, colour)` in render order. `None` = the editor's default
    /// foreground, which is what an unhighlighted identifier gets.
    pub parts: Vec<(String, Option<Color>)>,
    /// Render before the line's own content rather than after it.
    ///
    /// Note there is deliberately **no row count** here. How many rows the block
    /// occupies is a question for the layout, not for the plan — see
    /// [`row_split`], which is the only thing that should answer it.
    pub before: bool,
}

/// A phantom block's shape, without its coloured runs.
///
/// [`segments`] has to re-tokenise and re-allocate the whole suggestion, and only
/// [`InlineDiffDoc::phantom_text`] actually wants that. Everything else — the
/// styling that bands the rows, the strips that finish them over the gutter —
/// needs no more than "is there a block here, which side is it on, how long is
/// it", and those three callers run **per visible line, per relayout**. Asking
/// the expensive question to answer the cheap one had them re-highlighting the
/// suggestion several times a frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Block {
    /// Render before the line's own content rather than after it.
    pub before: bool,
    /// Total bytes of the block's text, matching what [`segments`] builds.
    pub len: usize,
}

impl Block {
    /// Bytes this block inserts *ahead of* the line's own content — nonzero only
    /// for a `before` block, which is the one case that shifts the real line's
    /// syntax spans.
    pub fn prefix_len(&self) -> usize {
        if self.before { self.len } else { 0 }
    }
}

/// The block anchored at `line`, if any — the cheap half of [`segments`].
///
/// Its `len` must agree with what [`segments`] builds byte for byte, since
/// `row_split` uses it to find where the line's own content resumes: one `\n` per
/// added row, plus each row's text, with an empty row costing the one byte of the
/// space `segments` substitutes for it.
pub fn block_at(view: &InlineView, line: usize) -> Option<Block> {
    let hunk = view
        .plan()?
        .hunks
        .iter()
        .find(|h| h.anchor == line && !h.add.is_empty())?;
    let len = hunk
        .add
        .iter()
        .map(|l| if l.is_empty() { 1 } else { l.len() })
        .sum::<usize>()
        + hunk.add.len();
    Some(Block {
        before: hunk.before,
        len,
    })
}

/// Build the phantom rows hanging off `line`, or `None` if no hunk anchors there.
///
/// Each added line contributes a `\n` (which is what makes Floem lay it out as
/// an extra visual row — see `TextLayoutLine::line_count`) followed by its
/// syntax-coloured runs. Colours are resolved *here*, on every call, rather than
/// cached with the plan: a theme switch bumps `SqlStyling::id`, which
/// invalidates the layout, and the rows are then rebuilt in the new palette.
///
/// An added line that is empty renders as a single space. Floem drops layouts
/// with no glyphs (`relevant_layouts` filters on exactly that), so a truly empty
/// row would collapse and the diff would be one line short of what it claims.
pub fn segments(view: &InlineView, line: usize, dialect: SqlDialect) -> Option<Segments> {
    let hunk = view
        .plan()?
        .hunks
        .iter()
        .find(|h| h.anchor == line && !h.add.is_empty())?;
    let mut parts: Vec<(String, Option<Color>)> = Vec::new();
    for (i, added) in hunk.add.iter().enumerate() {
        // A `before` block ends each row with the newline; an after-block starts
        // each row with one. Either way there is exactly one `\n` per added row,
        // and the line's own content sits on the other side of the block.
        if !hunk.before || i > 0 {
            parts.push(("\n".to_string(), None));
        }
        if added.is_empty() {
            parts.push((" ".to_string(), None));
        } else {
            let mut at = 0;
            for (s, e, color) in highlight_spans(added, dialect) {
                if s > at {
                    parts.push((added[at..s].to_string(), None));
                }
                parts.push((added[s..e].to_string(), Some(color)));
                at = e;
            }
            if at < added.len() {
                parts.push((added[at..].to_string(), None));
            }
        }
    }
    if hunk.before {
        parts.push(("\n".to_string(), None));
    }
    Some(Segments {
        parts,
        before: hunk.before,
    })
}

/// Split a line's visual rows into `(added, own)` — which of them are the phantom
/// block's and which are the line's own content.
///
/// **Asked of the layout, never counted from the plan.** The plan knows how many
/// *lines* the suggestion adds; what has to be banded and marked is *rows*, and
/// the two part company the moment a line is long enough to wrap. Deriving the
/// split by subtracting the added-line count from the row count was wrong in both
/// directions at once: with word wrap on it banded a wrapped continuation of the
/// *removed* line as an addition, and with wrap off a miscount by one left the
/// added rows with no band at all. `hit_position` answers it from the same text
/// layout the glyphs came out of, so it cannot disagree with what is on screen.
///
/// One function because there are two callers that must never differ:
/// `sql_highlight` bands the code column from inside the editor, and
/// `editor_pane::inline_band_runs` finishes the same bands over the gutter from
/// outside it. A second copy of this arithmetic is a seam waiting to drift.
pub fn row_split(
    layout: &TextLayoutLine,
    line_h: f64,
    block: Option<Block>,
    own_len: usize,
) -> (std::ops::Range<usize>, std::ops::Range<usize>) {
    let total = layout.line_count();
    let Some(block) = block else {
        // No block: every row is the line's own.
        return (0..0, 0..total);
    };
    // The one question only the layout can answer: which visual row the block
    // begins on. For a `before` block that is where the line's own text resumes
    // (the first byte past the phantom); for the usual end-of-line block it is the
    // index of the `\n` that ends the line's own content — which the layout puts
    // at the start of the **next** row, so it already names the block's first row.
    // Adding one to it pushed every band a row late: without wrap that left the
    // added rows outside the range and unbanded, and with wrap it banded the
    // suggestion's first row as a *deletion*.
    let col = if block.before {
        block.prefix_len()
    } else {
        own_len
    };
    let start = (layout.text.hit_position(col).point.y / line_h).round() as usize;
    split_rows(total, start, block.before)
}

/// The arithmetic half of [`row_split`], split out so it can be tested: given the
/// line's total visual rows and the row its phantom block starts on, which rows
/// are the block's `(added, own)`.
///
/// Separate from the layout question on purpose. Both of the defects this code
/// has shipped were *here* — a row count taken from the plan instead of the
/// layout, then an off-by-one on the start row — and neither was reachable by a
/// test while the arithmetic needed a real `TextLayoutLine` to exercise.
pub fn split_rows(
    total: usize,
    start: usize,
    before: bool,
) -> (std::ops::Range<usize>, std::ops::Range<usize>) {
    let start = start.min(total);
    if before {
        // The block leads: rows `0..start` are its, and the line's own content
        // picks up at `start`.
        (0..start, start..total)
    } else {
        // The block trails: the line's own rows run up to `start`, the block's
        // from there to the end.
        (start..total, 0..start)
    }
}

/// A `Document` that renders an inline-AI suggestion as phantom rows over the
/// editor's real document.
///
/// Everything except `phantom_text` delegates to `inner`, so the rope, the undo
/// history, IME preedit and every edit command stay exactly as they were — this
/// wrapper adds a *view* of a pending suggestion and nothing else.
pub struct InlineDiffDoc {
    inner: Rc<dyn Document>,
    preview: InlinePreview,
    dialect: SqlDialect,
}

impl InlineDiffDoc {
    pub fn new(inner: Rc<dyn Document>, preview: InlinePreview, dialect: SqlDialect) -> Self {
        Self {
            inner,
            preview,
            dialect,
        }
    }
}

/// Publish `plan` as the editor's inline preview (`None` clears it).
///
/// The `cache_rev` bump is the load-bearing half: phantom text is baked into a
/// line's cached `TextLayout` and that cache is keyed on `cache_rev`, so setting
/// the signal alone would change nothing on screen until the user's next
/// keystroke happened to invalidate it. This is the only way the preview should
/// be set — a bare `preview.set(…)` renders a frame late, or never.
pub fn set_preview(preview: InlinePreview, ed: &Editor, view: Option<InlineView>) {
    // Nothing to publish and nothing to invalidate. Worth the comparison: the
    // bump throws away every cached line layout in the editor, and the states
    // that publish `None` are the common ones — every Escape, Accept, Reject and
    // Ctrl+K passes through one, almost always with `None` already in place.
    if preview.with_untracked(|cur| cur == &view) {
        return;
    }
    preview.set(view);
    ed.doc().cache_rev().update(|rev| *rev += 1);
}

impl DocumentPhantom for InlineDiffDoc {
    /// **Only while a suggestion is actually on screen.**
    ///
    /// The trait default is `true`, and taking it would have been the quiet kind
    /// of wrong: Floem's `is_linear()` is `wrap == None && !has_multiline_phantom()`,
    /// and word wrap is off by default here, so answering `true` unconditionally
    /// takes the editor off its linear visual-line mapping for the whole session —
    /// on every document, for a feature that is live for a few seconds at a time.
    /// The wrapped `TextDocument` says `false` for any non-empty buffer, so
    /// delegating restores exactly the behaviour the editor had before it was
    /// wrapped, and the `true` is spent only when it buys something.
    fn has_multiline_phantom(&self, edid: EditorId, styling: &EditorStyle) -> bool {
        self.preview
            .get_untracked()
            .as_ref()
            .is_some_and(|v| v.plan().is_some())
            || self.inner.has_multiline_phantom(edid, styling)
    }

    fn phantom_text(&self, edid: EditorId, styling: &EditorStyle, line: usize) -> PhantomTextLine {
        // Start from the real document's own phantoms (placeholder, IME preedit)
        // so wrapping never costs the editor either of them.
        let mut out = self.inner.phantom_text(edid, styling, line);
        let Some(view) = self.preview.get_untracked() else {
            return out;
        };
        let Some(seg) = segments(&view, line, self.dialect) else {
            return out;
        };
        // The column the block hangs off: 0 to render before the line, else the
        // end of the line's content. `line_content` includes the line ending, and
        // Floem substitutes that for a space of equal byte length before
        // combining, so its length is the end column either way.
        let col = if seg.before {
            0
        } else {
            self.inner.rope_text().line_content(line).len()
        };
        // `combine_with_text` walks the list accumulating a column shift, so it
        // is only correct while the columns are non-decreasing. A `before` block
        // is at column 0 and has to lead; every other block is at end-of-line and
        // has to trail.
        let base = if seg.before { 0 } else { out.text.len() };
        for (at, (text, fg)) in seg.parts.into_iter().enumerate() {
            out.text.insert(
                base + at,
                PhantomText {
                    kind: PhantomTextKind::Completion,
                    col,
                    affinity: None,
                    text,
                    font_size: None,
                    fg,
                    bg: None,
                    under_line: None,
                },
            );
        }
        out
    }
}

impl Document for InlineDiffDoc {
    fn text(&self) -> Rope {
        self.inner.text()
    }

    fn cache_rev(&self) -> RwSignal<u64> {
        self.inner.cache_rev()
    }

    fn find_unmatched(&self, offset: usize, previous: bool, ch: char) -> usize {
        self.inner.find_unmatched(offset, previous, ch)
    }

    fn find_matching_pair(&self, offset: usize) -> usize {
        self.inner.find_matching_pair(offset)
    }

    fn preedit(&self) -> PreeditData {
        self.inner.preedit()
    }

    fn run_command(
        &self,
        ed: &Editor,
        cmd: &Command,
        count: Option<usize>,
        modifiers: Modifiers,
    ) -> CommandExecuted {
        self.inner.run_command(ed, cmd, count, modifiers)
    }

    fn receive_char(&self, ed: &Editor, c: &str) {
        self.inner.receive_char(ed, c)
    }

    fn edit(&self, iter: &mut dyn Iterator<Item = (Selection, &str)>, edit_type: EditType) {
        self.inner.edit(iter, edit_type)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemaic_core::diff::{InlineHunk, InlinePlan};

    fn plan(hunks: Vec<InlineHunk>) -> InlineView {
        let added = hunks.iter().map(|h| h.add.len()).sum();
        let removed = hunks.iter().map(|h| h.del.len()).sum();
        InlineView::Plan(InlinePlan {
            hunks,
            added,
            removed,
        })
    }

    fn hunk(del: std::ops::Range<usize>, add: &[&str], anchor: usize, before: bool) -> InlineHunk {
        InlineHunk {
            del,
            add: add.iter().map(|s| s.to_string()).collect(),
            anchor,
            before,
        }
    }

    /// The usual case: a block trailing the line's own content. One own row, two
    /// added rows, the block starting on row 1.
    #[test]
    fn a_trailing_block_takes_the_rows_from_its_start_on() {
        assert_eq!(split_rows(3, 1, false), (1..3, 0..1));
    }

    /// The wrapped case the layout is asked about: the line's own content took two
    /// rows, so the block starts on row 2 and the deletion band covers both.
    #[test]
    fn a_wrapped_own_line_keeps_all_of_its_rows() {
        assert_eq!(split_rows(4, 2, false), (2..4, 0..2));
    }

    #[test]
    fn a_leading_block_takes_the_rows_before_its_start() {
        assert_eq!(split_rows(3, 2, true), (0..2, 2..3));
    }

    /// The off-by-one that shipped: a start row one past the block's first row left
    /// the added rows outside the range entirely, so nothing was banded green and
    /// the whole diff read as deletions.
    #[test]
    fn a_start_row_at_the_end_bands_nothing_as_added() {
        assert_eq!(split_rows(2, 2, false), (2..2, 0..2));
    }

    /// Out of range is clamped rather than panicking on the reversed range that a
    /// `start > total` would otherwise build.
    #[test]
    fn a_start_row_past_the_end_is_clamped() {
        assert_eq!(split_rows(2, 9, false), (2..2, 0..2));
        assert_eq!(split_rows(2, 9, true), (0..2, 2..2));
        assert_eq!(split_rows(0, 0, false), (0..0, 0..0));
    }

    /// `block_at`'s length has to match what `segments` actually builds, because
    /// `row_split` uses it as a byte column into the combined line. One `\n` per
    /// added row plus each row's text.
    #[test]
    fn a_blocks_length_matches_the_bytes_segments_emits() {
        for before in [false, true] {
            let v = plan(vec![hunk(0..1, &["SELECT 1", "FROM t"], 0, before)]);
            let seg = segments(&v, 0, SqlDialect::MySql).expect("a block at line 0");
            let built: usize = seg.parts.iter().map(|(t, _)| t.len()).sum();
            let cheap = block_at(&v, 0).expect("the same block, cheaply");
            assert_eq!(cheap.len, built, "before={before}");
            assert_eq!(cheap.before, before);
        }
    }

    /// An empty added line renders as a space, so it still costs a byte — the one
    /// place the cheap length could silently drift from the built one.
    #[test]
    fn an_empty_added_line_costs_the_space_it_renders_as() {
        let v = plan(vec![hunk(0..1, &["SELECT 1", "", "FROM t"], 0, false)]);
        let seg = segments(&v, 0, SqlDialect::MySql).unwrap();
        let built: usize = seg.parts.iter().map(|(t, _)| t.len()).sum();
        assert_eq!(block_at(&v, 0).unwrap().len, built);
    }

    #[test]
    fn no_block_where_no_hunk_anchors() {
        let v = plan(vec![hunk(0..1, &["X"], 0, false)]);
        assert!(block_at(&v, 1).is_none());
        // A pure deletion anchors a hunk but adds nothing to hang there.
        let d = plan(vec![hunk(0..1, &[], 0, false)]);
        assert!(block_at(&d, 0).is_none());
        // And a request still in flight has no plan at all.
        assert!(block_at(&InlineView::Working(0..2), 0).is_none());
    }

    // ── the producer/consumer seam ──────────────────────────────────────────

    /// Every added row a plan claims has to be **reachable** — some document
    /// line has to anchor a block that renders it.
    ///
    /// The tests above hand-build their hunks, so none of them can see this:
    /// the renderer finds a block with `hunks.iter().find(|h| h.anchor == line)`,
    /// which returns the *first* hunk on an anchor, so two hunks sharing one
    /// anchor means the second is drawn nowhere. A hand-written fixture never
    /// produces that pair; `inline_plan` does, for a pure insertion after line 0
    /// alongside a `before` hunk at 0 — the shape `("b", "d\nb\nd")` builds, and
    /// the one that shipped drawing 1 of its 2 added lines.
    ///
    /// So the plan comes from `inline_plan` here, and the count is taken from
    /// `segments` — one `\n` per added row is the module's own contract, pinned
    /// by `a_blocks_length_matches_the_bytes_segments_emits`.
    #[test]
    fn every_line_a_plan_adds_is_drawn_somewhere() {
        for (old, new) in [
            ("b", "d\nb\nd"),
            ("b", "d\nb"),
            ("b", "b\nd"),
            ("a\nb\nc", "a\nB\nc"),
            ("a\nb\nc", "x\na\nb\nc\ny"),
            ("a\nb\nc", ""),
            ("", "a\nb"),
            ("a\nb\nc", "a\nb\nc"),
            ("a\nb\nc\nd", "d\nc\nb\na"),
        ] {
            let p = schemaic_core::diff::inline_plan(old, new);
            let v = InlineView::Plan(p.clone());
            // Every line of the old buffer, plus one past it: a hunk can anchor
            // at the last line, and `inline_plan` anchors a leading insertion at
            // line 0 with `before`.
            let lines = old.lines().count().max(1);
            let drawn: usize = (0..=lines)
                .filter_map(|l| segments(&v, l, SqlDialect::MySql))
                .map(|s| s.parts.iter().filter(|(t, _)| t == "\n").count())
                .sum();
            assert_eq!(
                drawn, p.added,
                "{old:?} → {new:?}: the plan claims {} added lines and the \
                 renderer draws {drawn}; hunks {:?}",
                p.added, p.hunks
            );
        }
    }

    /// The cheap and expensive halves have to agree on plans the differ really
    /// produces, not only on hand-built ones — `row_split` uses `block_at`'s
    /// `len` as a byte column into the text `segments` built, so a disagreement
    /// bands the wrong rows.
    #[test]
    fn the_cheap_and_full_blocks_agree_on_a_real_plan() {
        for (old, new) in [
            ("b", "d\nb\nd"),
            ("SELECT 1", "SELECT 1\nWHERE x = 1"),
            ("a\nb", "\na\n\nb"),
        ] {
            let p = schemaic_core::diff::inline_plan(old, new);
            let v = InlineView::Plan(p);
            for l in 0..=old.lines().count().max(1) {
                let cheap = block_at(&v, l);
                let full = segments(&v, l, SqlDialect::MySql);
                assert_eq!(
                    cheap.is_some(),
                    full.is_some(),
                    "{old:?} → {new:?} line {l}: the two halves disagree on \
                     whether a block is there"
                );
                if let (Some(c), Some(f)) = (cheap, full) {
                    assert_eq!(c.before, f.before, "{old:?} → {new:?} line {l}");
                    assert_eq!(
                        c.len,
                        f.parts.iter().map(|(t, _)| t.len()).sum::<usize>(),
                        "{old:?} → {new:?} line {l}: byte lengths differ"
                    );
                }
            }
        }
    }

    #[test]
    fn only_a_leading_block_shifts_the_lines_own_spans() {
        assert_eq!(
            Block {
                before: true,
                len: 12
            }
            .prefix_len(),
            12
        );
        assert_eq!(
            Block {
                before: false,
                len: 12
            }
            .prefix_len(),
            0
        );
    }
}
