//! Layout & dimension constants shared across the UI views.
//!
//! These were previously scattered through `lib.rs`; collecting them here lets
//! the leaf view modules `use crate::consts::*` without depending on definition
//! order (and keeps the magic numbers in one auditable place). Domain data
//! tables (the SQL keyword lists) stay with their completion logic, not here.

use crate::theme;

// ── Schema tree ─────────────────────────────────────────────────────────────

/// Fixed row height — must match the `VirtualItemSize::Fixed` fed to the
/// virtual stack, or rows and viewport drift apart.
pub(crate) const ROW_H: f64 = 26.0;
/// Height of one row in the schema tree.
pub(crate) const TREE_ROW_H: f64 = 24.0;
/// Size of the schema tree's chevrons and database/table glyphs.
pub(crate) const SCHEMA_ICON: f64 = 16.0;
/// Gap from the chevron to the database/table glyph.
pub(crate) const CHEVRON_GAP: f64 = 7.0;
/// Gap from the database/table glyph to its label.
pub(crate) const ICON_GAP: f64 = 10.0;
/// Base left padding of a top-level (database) row.
pub(crate) const ROW_PAD: f64 = 10.0;
/// Extra indent applied to table rows (one level under their database).
pub(crate) const LEVEL_INDENT: f64 = 16.0;
/// Left padding of leaf rows (columns / keys / count capsules): aligned under
/// the parent table's *label* — table row pad + chevron + gap + glyph + gap.
/// (Aligning under the glyph left the leaves hanging left of the table name;
/// this tracks the label so tuning `CHEVRON_GAP` alone can't un-indent them.)
pub(crate) const LEAF_PAD: f64 =
    ROW_PAD + LEVEL_INDENT + SCHEMA_ICON + CHEVRON_GAP + SCHEMA_ICON + ICON_GAP;
/// Left padding of a leaf row that carries its own 16px icon (column type / key).
/// Columns nest one `LEVEL_INDENT` under their table — the same step tables get
/// under their database — so the icon sits one level right of the table's glyph.
pub(crate) const COL_PAD: f64 = LEAF_PAD - SCHEMA_ICON - ICON_GAP + LEVEL_INDENT;
/// Minimum width of a schema-tree row: short rows fill (nice hover), long rows
/// extend past it so the horizontal scrollbar kicks in.
pub(crate) const TREE_ROW_MIN_W: f64 = theme::SCHEMA_W - 20.0;

// ── Results grid (fixed-layout legacy) ──────────────────────────────────────

/// Fixed column width for M2 (per-column sizing / resize is a later polish).
pub(crate) const CELL_W: f64 = 190.0;

/// Fixed width of the results toolbar's Copy dropdown, so the overlay's edge-flip
/// right-aligns it flush to the icon (matches `menu_stack`'s 170px min width).
pub(crate) const GRID_COPY_MENU_W: f64 = 170.0;

// ── SQL editor ──────────────────────────────────────────────────────────────

/// Height of the query editor panel (a multiline SQL editor fills this box).
pub(crate) const EDITOR_H: f64 = 248.0;
/// Internal top padding of the SQL editor (breathing room from the border). The
/// editor's overlays (completion popup, statement-highlight border, squiggles,
/// Ctrl+K, run menu) anchor via `points_of_offset`, which is relative to the
/// editor's *content* — it doesn't count this view padding — so each of those
/// anchors adds `EDITOR_PAD_TOP` back to its `y`. (Right/bottom padding don't
/// move the content origin, so they need no compensation.)
pub(crate) const EDITOR_PAD_TOP: f64 = 5.0;

/// Height of the Ctrl+K diff scroll area. Flex/percentage heights don't resolve
/// through the absolute overlay in Floem, so the diff can't "fill the remaining
/// space" — we size it explicitly instead. DERIVED from `EDITOR_H` (minus the
/// fixed chrome stacked around the diff) rather than a bare constant, so it
/// tracks the editor if `EDITOR_H` changes — e.g. when the editor/results split
/// becomes resizable. Subtraction (not a proportion) is correct: the chrome is a
/// fixed pixel amount, so only the diff should absorb any extra height.
///   ~30 toolbar + 20 editor_wrap pad + ~35 question row + ~45 buttons + 20 diff
///   pad ≈ 150 of chrome — trimmed to 135 so a long diff + buttons + spacing
///   reach the bottom of the overlay with no dead space.
pub(crate) const CMDK_DIFF_CHROME: f64 = 135.0;
pub(crate) const CMDK_DIFF_H: f64 = EDITOR_H - CMDK_DIFF_CHROME;

/// Estimated width of the editor's line-number gutter (used to place the
/// completion popup near the caret).
pub(crate) const COMPLETION_GUTTER: f64 = 38.0;
/// Gap below the caret's line-bottom (`points_of_offset().1.y`) at which the
/// completion popup opens.
pub(crate) const COMPLETION_LINE_H: f64 = 3.0;
/// Height of one suggestion row — measured: `font_size(14)` + `padding_vert(5)`.
/// The popup is positioned by a style closure, which has no measured height to
/// read, so the flip-above decision predicts the list's height from its row count.
pub(crate) const COMPLETION_ROW_H: f64 = 24.0;
/// Tallest the suggestion list may grow before it scrolls internally.
pub(crate) const COMPLETION_MAX_H: f64 = 260.0;
/// The popup's 1px border, on both axes: the difference between the box its
/// content gets and the box a caller has to find room for.
pub(crate) const COMPLETION_BORDER: f64 = 2.0;
/// Breathing room kept between the popup and the editor pane's edges.
pub(crate) const COMPLETION_EDGE_PAD: f64 = 4.0;
/// Shortest a squeezed list is shrunk to (two rows) before it stops giving way —
/// below this it shows nothing worth reading, so it keeps the height instead.
pub(crate) const COMPLETION_MIN_H: f64 = 48.0;
/// Narrowest the popup is drawn, so a list of one-letter column names doesn't come
/// up as a sliver that jitters wider on the next keystroke.
pub(crate) const COMPLETION_MIN_W: f64 = 270.0;
/// Widest, past which a long detail (a function signature) ellipsizes rather than
/// dragging the whole box out and leaving every short row full of whitespace.
pub(crate) const COMPLETION_MAX_W: f64 = 520.0;
/// Slack added to each row's predicted width. Sizing the box to *exactly* the
/// widest row leaves that row sitting on its own ellipsis boundary, where a
/// sub-pixel disagreement between the measurement and the layout truncates the
/// detail (`main` → `m…`) while every shorter row renders clean. Cheap insurance:
/// the alternative to a few px of air is a visibly wrong string.
pub(crate) const COMPLETION_SLACK_W: f64 = 6.0;
/// A suggestion row's fixed chrome. Summed with the measured text these give the
/// width the popup wants, which a style closure has no measured value for — so
/// `completion_popup` builds its rows out of these same constants rather than
/// repeating the numbers, since a layout tweak that skipped the measurement would
/// silently mis-size the box.
///
/// The leading glyph plus its `margin_right`:
pub(crate) const COMPLETION_ICON_W: f64 = COMPLETION_ICON_SIZE + 7.0;
pub(crate) const COMPLETION_ICON_SIZE: f64 = 13.0;
/// The least the flex spacer holds between the name and the annotations:
pub(crate) const COMPLETION_GAP_W: f64 = 24.0;
/// The detail's `margin_left` — charged even for an empty detail, since the margin
/// is on the node rather than the text:
pub(crate) const COMPLETION_DETAIL_GAP: f64 = 18.0;
/// The row's `padding_horiz`, per side:
pub(crate) const COMPLETION_ROW_PAD: f64 = 10.0;
/// Suggestion name size (matches the editor) and annotation size.
pub(crate) const COMPLETION_NAME_SIZE: f32 = 14.0;
pub(crate) const COMPLETION_ANNOT_SIZE: f32 = 12.0;

/// The Ctrl+Enter run menu's width. The panel's `min_width` *and* what
/// `run_menu_pos` finds room for — one constant, because a placement computed
/// against a different width than the panel draws at is how the menu came to be
/// cut off at the editor's right edge in the first place.
pub(crate) const RUN_MENU_W: f64 = 170.0;
/// One run-menu row: `FONT_TITLE`'s line box plus its `padding_vert(8)`.
pub(crate) const RUN_MENU_ROW_H: f64 = 34.0;
/// The whole menu: its two rows, the panel's `padding_vert(6)` and its 1px border.
///
/// An estimate, like [`COMPLETION_ROW_H`], because a style closure has no measured
/// box to ask. Only the bottom clamp reads it, so being a few px out costs a few px
/// of position — never a cut-off row.
pub(crate) const RUN_MENU_H: f64 = 2.0 * RUN_MENU_ROW_H + 12.0 + 2.0;

/// Height of the wavy syntax-error underline (px).
pub(crate) const WAVE_H: f64 = 5.0;

/// Horizontal padding on the statement-highlight border so it clears the glyphs.
pub(crate) const HL_PAD: f64 = 3.0;
/// Editor-area x where the code text starts (past the gutter), for a 1-digit
/// line-number gutter. Measured — larger than `COMPLETION_GUTTER` (which the
/// completion popup hides behind its own padding); a tight border needs the
/// real value. `HL_DIGIT_W` widens it per extra line-number digit.
pub(crate) const HL_GUTTER: f64 = 56.0;
pub(crate) const HL_DIGIT_W: f64 = 8.0;

/// The app's monospace face — the bundled family the SQL editor resolves
/// `monospace` to. Anything rendering SQL as *code* outside the editor (the
/// Ctrl+K diff, an `edit_field` with `mono`) uses this, so they all match the
/// editor and follow it if the bundled face ever changes.
pub(crate) const MONO_FAMILY: &str = "IBM Plex Mono";

// ── Panel resize handles ────────────────────────────────────────────────────

/// Grab width of a panel-resize divider and the visible bar. Per-panel min/max
/// drag limits live below (`SCHEMA_MIN_W`/`RIGHT_MIN_W`/`CENTER_MIN_W`/…).
pub(crate) const RESIZE_HIT: f64 = 10.0;
pub(crate) const RESIZE_BAR: f64 = 3.0;

// ── Panel minimum dimensions + responsive breakpoints ───────────────────────

/// Minimum panel widths so a dragged — or auto-shrunk — panel stays legible
/// instead of squashing to a sliver. The center (query + results) is the
/// priority: the side panels yield width to keep it ≥ `CENTER_MIN_W`.
pub(crate) const SCHEMA_MIN_W: f64 = 250.0;
pub(crate) const RIGHT_MIN_W: f64 = 250.0;
pub(crate) const CENTER_MIN_W: f64 = 400.0;
/// Minimum heights for the query editor and the results grid (drag + flex floor).
pub(crate) const QUERY_MIN_H: f64 = 160.0;
pub(crate) const RESULTS_MIN_H: f64 = 190.0;
/// Responsive breakpoints on total window width. Below `PANELS_MIN_FULL_W` the
/// right panel (AI/terminal/history) is force-hidden and its toggle locked;
/// below `PANELS_MIN_SCHEMA_W` the schema panel is too. Each equals the summed
/// min widths of the panels that must fit, so a panel is only locked away once
/// there's genuinely no room for it beside the center.
pub(crate) const PANELS_MIN_FULL_W: f64 = SCHEMA_MIN_W + CENTER_MIN_W + RIGHT_MIN_W; // 900
pub(crate) const PANELS_MIN_SCHEMA_W: f64 = SCHEMA_MIN_W + CENTER_MIN_W; // 650
/// A left status-bar segment auto-hides once its right edge comes within this
/// many px of the footer's right-hand icon group (the AI icon's left edge), so
/// the two clusters never collide on a narrow window.
pub(crate) const FOOTER_COLLAPSE_GAP: f64 = 30.0;

/// How far above the footer bar's true centre its contents sit, in px.
///
/// A correction, not a design choice: everything in the bar — icons and labels
/// alike — read low against the 1px rule above it, which is the kind of drift
/// that is invisible until someone puts a straight edge on it. Applied once, as
/// `padding_bottom(FOOTER_LIFT * 2)` on the bar, so a later adjustment is one
/// number rather than a margin on each of fifteen segments.
///
/// Tuned by eye against [`crate::theme::FOOTER_H`], which it
/// belongs with. The bar's bottom is pinned to the window, so growing it lifts
/// its own centre too — when `FOOTER_H` went 26 → 28 this had to come back down
/// to 2 rather than up to 3, or the contents would have moved twice.
pub(crate) const FOOTER_LIFT: f64 = 2.0;

// ── Tab bar ─────────────────────────────────────────────────────────────────

/// Tab bar height. Flat, full-height tabs fill it edge to edge.
pub(crate) const TAB_BAR_H: f64 = 34.0;
/// Max width of a single query tab (px). The title truncates with an ellipsis
/// (+ full-name tooltip) past this; the inline rename field auto-widens up to it.
pub(crate) const TAB_MAX_W: f64 = 200.0;

// ── AI chat input ───────────────────────────────────────────────────────────

pub(crate) const CHAT_MAX_ROWS: usize = 6;
pub(crate) const CHAT_PAD_V: f64 = 6.0;
pub(crate) const CHAT_PAD_H: f64 = 10.0;

/// Height of a **compact single-line field** — the one every transient bar uses:
/// the editor's find / replace / go-to-line, the grid's find and go-to-row, and
/// the row panel's inputs.
///
/// It exists as one constant because leaving it off is not a neutral default but
/// a *different* control: [`crate::FieldCfg::height`] is `Option`, and `None`
/// derives the box from content as `line_h + CHAT_PAD_V * 2 + 3` — 34px at the
/// 13px font these bars use, against this 26. The grid's find bar shipped
/// without it and so stood 8px taller than the identical editor bar beside it.
/// A bar that means to be compact says so with this, and never with a literal.
pub(crate) const FIELD_INPUT_H: f64 = 26.0;

// ── Data grid (interactive: sizing, selection, export) ──────────────────────

pub(crate) const MIN_COL_W: f64 = 48.0;
pub(crate) const MAX_COL_W_INIT: f64 = 420.0;
pub(crate) const GRID_CHAR_W: f64 = 7.0; // ≈ advance width of the 13px cell font
pub(crate) const RESIZE_HIT_W: f64 = 7.0; // grab width of a column-resize divider
pub(crate) const GRID_HEADER_H: f64 = 40.0; // two-line header (name + type)
pub(crate) const GUTTER_W: f64 = 52.0; // row-number gutter width (frozen)
// Right padding for right-aligned (numeric) headers + cells — a touch more than
// the 10px sides so the value doesn't hug the edge/border. Header and cell share
// it so numbers line up under their column name.
pub(crate) const GRID_NUM_PAD_RIGHT: f64 = 14.0;
/// Horizontal padding inside a grid cell / header (the numeric side swaps the
/// right one for [`GRID_NUM_PAD_RIGHT`]).
pub(crate) const GRID_PAD_H: f64 = 10.0;
/// The divider drawn on a cell's right edge. Named because it is a *border*, so
/// it comes out of the content box — anything computing how much room a cell has
/// for its value has to subtract it (see `numeric_edit_pad_left`).
pub(crate) const GRID_CELL_DIVIDER: f64 = 1.0;
// Extra header width a key column needs over a plain one: its leading key icon
// (14px) + gap (8px) beyond the normal side padding. Added to the width estimate
// so a long type line (e.g. `INT UNSIGNED`) on a PK/FK column isn't clipped.
pub(crate) const HEADER_KEY_ICON_W: f64 = 22.0;
// Trailing-debounce delay for the schema-tree + query-history search boxes: the
// input stays live, but the expensive re-filter/re-expand fires once the typing
// pauses this long — so a single keystroke doesn't churn a large schema/history.
pub(crate) const SEARCH_DEBOUNCE_MS: u64 = 150;

/// Auto-hide delay (ms) for the overlay scrollbars after scrolling stops.
pub(crate) const SCROLL_HIDE_MS: u64 = 3000;

/// How close to the bottom (px) a tail-following list counts as "at the bottom":
/// under this it keeps following new content, past it the user is reading and the
/// follow is released. Wide enough that a small overshoot while scrolling near the
/// end doesn't drop the follow.
pub(crate) const FOLLOW_SLACK: f64 = 30.0;

// ── Menus / misc ────────────────────────────────────────────────────────────

/// Fixed width of the active-database menu (right-aligned under its trigger).
pub(crate) const DB_MENU_W: f64 = 170.0;

/// The masking glyph used by password fields. Must be a single ASCII byte so the
/// buffer's byte length tracks its char length and the cursor stays valid.
pub(crate) const MASK_CH: char = '*';

// ── Terminal ────────────────────────────────────────────────────────────────

/// Terminal font sizes offered in settings (logical px).
pub(crate) const TERM_FONT_SIZES: [u16; 5] = [12, 13, 14, 16, 18];

// ── Panel width arithmetic ──────────────────────────────────────────────────
//
// The stored `schema_w` / `right_w` are the user's *intent* and are never
// mutated by layout, so a panel restores to its full width when the window grows
// back. What is rendered is the intent clamped so the center keeps
// `CENTER_MIN_W`, which on a narrow window is narrower than the intent — and
// anything that sizes itself to the panel has to use the same number the shell
// renders it at, or it lays out content into a clipped wrapper.

/// Does the schema panel fit beside the center at this window width? `0` (or any
/// width under 1) is "not measured yet" and counts as allowed, so nothing is
/// hidden before the first resize.
pub(crate) fn schema_panel_fits(window_w: f64) -> bool {
    !(1.0..PANELS_MIN_SCHEMA_W).contains(&window_w)
}

/// Does the right (AI / terminal / history) panel fit beside the schema panel and
/// the center?
pub(crate) fn right_panel_fits(window_w: f64) -> bool {
    !(1.0..PANELS_MIN_FULL_W).contains(&window_w)
}

/// Rendered width of the right panel: 0 when closed or locked away, else the
/// intent clamped so the center and the schema panel's *minimum* still fit.
pub(crate) fn effective_right_w(window_w: f64, intended: f64, open: bool) -> f64 {
    if !open || !right_panel_fits(window_w) {
        return 0.0;
    }
    if window_w < 1.0 {
        return intended;
    }
    intended.clamp(
        RIGHT_MIN_W,
        (window_w - CENTER_MIN_W - SCHEMA_MIN_W).max(RIGHT_MIN_W),
    )
}

/// Rendered width of the schema panel: 0 when hidden or locked away, else the
/// intent clamped so the center keeps `CENTER_MIN_W` beside the right panel's
/// *effective* width (it yields to what the right panel actually takes, while the
/// right panel yields only to the schema panel's minimum).
pub(crate) fn effective_schema_w(window_w: f64, intended: f64, right_eff: f64, open: bool) -> f64 {
    if !open || !schema_panel_fits(window_w) {
        return 0.0;
    }
    if window_w < 1.0 {
        return intended;
    }
    intended.clamp(
        SCHEMA_MIN_W,
        (window_w - CENTER_MIN_W - right_eff).max(SCHEMA_MIN_W),
    )
}

#[cfg(test)]
mod width_tests {
    use super::*;

    /// The configuration the finding was observed in: an ordinary laptop window
    /// with the AI panel open, where the panel renders narrower than it lays
    /// itself out — the search box's clear button falling off the clipped edge.
    #[test]
    fn a_narrow_window_with_the_right_panel_open_clamps_the_schema_panel() {
        let right = effective_right_w(950.0, 350.0, true);
        assert_eq!(right, 300.0, "the right panel yields to the schema minimum");
        assert_eq!(effective_schema_w(950.0, 300.0, right, true), SCHEMA_MIN_W);
    }

    #[test]
    fn a_wide_window_renders_both_panels_at_the_intended_width() {
        let right = effective_right_w(1800.0, 350.0, true);
        assert_eq!(right, 350.0);
        assert_eq!(effective_schema_w(1800.0, 400.0, right, true), 400.0);
    }

    /// A user who has widened the panel meets the clamp at a *wider* window, which
    /// is the likelier configuration.
    #[test]
    fn a_widened_panel_starts_yielding_sooner() {
        let right = effective_right_w(1200.0, 350.0, true);
        assert_eq!(effective_schema_w(1200.0, 600.0, right, true), 450.0);
        assert_eq!(effective_schema_w(1200.0, 400.0, right, true), 400.0);
    }

    #[test]
    fn a_closed_or_locked_away_panel_is_zero_wide() {
        assert_eq!(effective_right_w(1800.0, 350.0, false), 0.0);
        assert_eq!(effective_schema_w(1800.0, 300.0, 0.0, false), 0.0);
        // Under the breakpoints the panels are locked away whatever the intent.
        assert_eq!(effective_right_w(880.0, 350.0, true), 0.0);
        assert_eq!(effective_schema_w(600.0, 300.0, 0.0, true), 0.0);
    }

    /// Before the first resize the window is (0, 0): render the intent rather than
    /// collapsing every panel to its minimum for a frame.
    #[test]
    fn an_unmeasured_window_renders_the_intended_widths() {
        assert_eq!(effective_right_w(0.0, 350.0, true), 350.0);
        assert_eq!(effective_schema_w(0.0, 300.0, 350.0, true), 300.0);
        assert!(schema_panel_fits(0.0) && right_panel_fits(0.0));
    }

    #[test]
    fn the_panel_breakpoints_are_the_summed_minimums() {
        assert!(!right_panel_fits(PANELS_MIN_FULL_W - 1.0));
        assert!(right_panel_fits(PANELS_MIN_FULL_W));
        assert!(!schema_panel_fits(PANELS_MIN_SCHEMA_W - 1.0));
        assert!(schema_panel_fits(PANELS_MIN_SCHEMA_W));
    }
}
