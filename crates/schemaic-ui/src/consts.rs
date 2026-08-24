//! Layout & dimension metrics shared across the UI views.
//!
//! These were previously scattered through `lib.rs`; collecting them here lets
//! the leaf view modules `use crate::consts::*` without depending on definition
//! order (and keeps the magic numbers in one auditable place). Domain data
//! tables (the SQL keyword lists) stay with their completion logic, not here.
//!
//! ## Functions, not constants
//!
//! Anything that boxes, indents or spaces *text* is a `fn() -> f64` reading the
//! interface scale ([`crate::theme::UiScale`]) — the same rule the colours
//! follow, and for the same reason: a `.style(…)` closure that **calls** the
//! metric re-runs when the setting changes, while one that captured a `const`
//! can never re-evaluate. Call them inside the closure.
//!
//! What stays a `const`, deliberately:
//!
//!   • **Hairlines** (`GRID_CELL_DIVIDER`, `COMPLETION_BORDER`) — a rule is one
//!     physical line at every size; 2px of it reads as a border, not a seam.
//!   • **Editor-relative metrics** (`EDITOR_PAD_TOP`, `COMPLETION_GUTTER`,
//!     `HL_GUTTER`, `HL_DIGIT_W`, `WAVE_H`, `HL_PAD`) — they measure against the
//!     *code* font, which has its own size setting and which the interface scale
//!     deliberately doesn't touch. Scaling them would slide the statement
//!     highlight and the squiggles off the glyphs they mark.
//!   • **Seeds for persisted, user-dragged sizes** (`theme::SCHEMA_W`,
//!     `theme::AI_W`, `EDITOR_H`) — see [`crate::theme::SCHEMA_W`].
//!   • Anything that isn't a length: durations, counts, thresholds.

use crate::theme;
use crate::theme::scaled;

// ── Schema tree ─────────────────────────────────────────────────────────────

/// Fixed row height — must match the `VirtualItemSize::Fixed` fed to the
/// virtual stack, or rows and viewport drift apart. (The `Fixed` variant takes a
/// closure, so calling this inside it keeps the two in step when the interface
/// scale changes.)
pub(crate) fn row_h() -> f64 {
    scaled(26.0)
}
/// Height of one row in the schema tree.
pub(crate) fn tree_row_h() -> f64 {
    scaled(24.0)
}
/// Size of the schema tree's chevrons and database/table glyphs — the **base**,
/// which [`crate::icons::icon`] scales at the call site. [`schema_icon`] is the
/// same number already scaled, for the indent arithmetic below; handing *that*
/// to `icon` would scale it twice.
pub(crate) const SCHEMA_ICON_BASE: f32 = 16.0;
/// Rendered size of those glyphs — what the row indents have to reserve.
pub(crate) fn schema_icon() -> f64 {
    scaled(SCHEMA_ICON_BASE as f64)
}
/// Gap from the chevron to the database/table glyph.
pub(crate) fn chevron_gap() -> f64 {
    scaled(7.0)
}
/// Gap from the database/table glyph to its label.
pub(crate) fn icon_gap() -> f64 {
    scaled(10.0)
}
/// Base left padding of a top-level (database) row.
pub(crate) fn row_pad() -> f64 {
    scaled(10.0)
}
/// Extra indent applied to table rows (one level under their database).
pub(crate) fn level_indent() -> f64 {
    scaled(16.0)
}
/// Left padding of leaf rows (columns / keys / count capsules): aligned under
/// the parent table's *label* — table row pad + chevron + gap + glyph + gap.
/// (Aligning under the glyph left the leaves hanging left of the table name;
/// this tracks the label so tuning `chevron_gap` alone can't un-indent them.)
///
/// Summed from the *scaled* parts rather than scaled after the fact, so the
/// indent lands on the same pixel the glyphs it aligns under actually occupy.
pub(crate) fn leaf_pad() -> f64 {
    row_pad() + level_indent() + schema_icon() + chevron_gap() + schema_icon() + icon_gap()
}
/// Left padding of a leaf row that carries its own icon (column type / key).
/// Columns nest one `level_indent` under their table — the same step tables get
/// under their database — so the icon sits one level right of the table's glyph.
pub(crate) fn col_pad() -> f64 {
    leaf_pad() - schema_icon() - icon_gap() + level_indent()
}
/// Minimum width of a schema-tree row: short rows fill (nice hover), long rows
/// extend past it so the horizontal scrollbar kicks in.
pub(crate) const TREE_ROW_MIN_W: f64 = theme::SCHEMA_W - 20.0;

// ── Results grid (fixed-layout legacy) ──────────────────────────────────────

/// Fixed column width for M2 (per-column sizing / resize is a later polish).
pub(crate) fn cell_w() -> f64 {
    scaled(190.0)
}

/// Fixed width of the results toolbar's Copy dropdown, so the overlay's edge-flip
/// right-aligns it flush to the icon (matches `menu_stack`'s 170px min width).
pub(crate) fn grid_copy_menu_w() -> f64 {
    scaled(170.0)
}

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

/// Everything stacked around the Ctrl+K diff, which is what the diff's own height
/// is the editor area minus. Flex/percentage heights don't resolve through the
/// absolute overlay in Floem, so the diff can't "fill the remaining space" — it is
/// sized explicitly, and something has to say how much is not it. Subtraction (not
/// a proportion) is correct: only the diff should absorb any extra height.
///   ~30 toolbar + 20 editor_wrap pad + ~35 question row + ~45 buttons + 20 diff
///   pad ≈ 150 of chrome — trimmed to 135 so a long diff + buttons + spacing
///   reach the bottom of the overlay with no dead space.
///
/// **Scaled, unlike the [`EDITOR_H`] seed the editor's height starts at.** Every
/// element it accounts for is scaled now — the toolbar row is `scaled(42)`, the
/// option rows `scaled(35)`, the buttons `action_height()` — so a fixed 135 stopped
/// describing the chrome it stands for, and at the top scale roughly 216px of
/// chrome was being subtracted as 135. `EDITOR_H` stays unscaled because it is a
/// persisted drag seed; this is a *composition of scaled parts*, which is the distinction the
/// module doc draws.
pub(crate) fn cmdk_diff_chrome() -> f64 {
    scaled(135.0)
}
/// The diff's own height: what is left of `area` — the editor area the overlay
/// actually fills — once its chrome is taken out.
///
/// **`area`, not [`EDITOR_H`].** The constant is a *drag seed*: the editor pane is
/// resizable and the expanded overlay is sized from the measured
/// `editor_area` height (`cmdk_popup`'s `area_h`), so deriving from 248 sized the
/// diff for a pane nobody had any more — short in a pane dragged taller, and, once
/// the chrome became scaled while the constant did not, longer than the whole
/// overlay at Large and Huge.
///
/// **And it floors at zero, which is the whole point of a floor here.** The old
/// `.max(scaled(60))` was written to keep something readable, but when the chrome
/// alone doesn't fit, adding 60 more *guarantees* the overflow instead of
/// preventing it — and what overflows is the bottom of the column, which is
/// Accept/Discard. A pane too short for the chrome shows no diff and keeps its
/// buttons; enlarging the editor is the way back, and it is a gesture the user has.
pub(crate) fn cmdk_diff_h(area: f64) -> f64 {
    (area - cmdk_diff_chrome()).max(0.0)
}

/// Estimated width of the editor's line-number gutter (used to place the
/// completion popup near the caret).
pub(crate) const COMPLETION_GUTTER: f64 = 38.0;
/// Gap below the caret's line-bottom (`points_of_offset().1.y`) at which the
/// completion popup opens.
pub(crate) const COMPLETION_LINE_H: f64 = 3.0;
/// Height of one suggestion row — measured: `font_size(14)` + `padding_vert(5)`.
/// The popup is positioned by a style closure, which has no measured height to
/// read, so the flip-above decision predicts the list's height from its row count.
pub(crate) fn completion_row_h() -> f64 {
    scaled(24.0)
}
/// Tallest the suggestion list may grow before it scrolls internally.
pub(crate) fn completion_max_h() -> f64 {
    scaled(260.0)
}
/// The popup's 1px border, on both axes: the difference between the box its
/// content gets and the box a caller has to find room for.
pub(crate) const COMPLETION_BORDER: f64 = 2.0;
/// Breathing room kept between the popup and the editor pane's edges.
pub(crate) fn completion_edge_pad() -> f64 {
    scaled(4.0)
}
/// Shortest a squeezed list is shrunk to (two rows) before it stops giving way —
/// below this it shows nothing worth reading, so it keeps the height instead.
pub(crate) fn completion_min_h() -> f64 {
    scaled(48.0)
}
/// Narrowest the popup is drawn, so a list of one-letter column names doesn't come
/// up as a sliver that jitters wider on the next keystroke.
pub(crate) fn completion_min_w() -> f64 {
    scaled(270.0)
}
/// Widest, past which a long detail (a function signature) ellipsizes rather than
/// dragging the whole box out and leaving every short row full of whitespace.
pub(crate) fn completion_max_w() -> f64 {
    scaled(520.0)
}
/// Slack added to each row's predicted width. Sizing the box to *exactly* the
/// widest row leaves that row sitting on its own ellipsis boundary, where a
/// sub-pixel disagreement between the measurement and the layout truncates the
/// detail (`main` → `m…`) while every shorter row renders clean. Cheap insurance:
/// the alternative to a few px of air is a visibly wrong string.
pub(crate) fn completion_slack_w() -> f64 {
    scaled(6.0)
}
/// A suggestion row's fixed chrome. Summed with the measured text these give the
/// width the popup wants, which a style closure has no measured value for — so
/// `completion_popup` builds its rows out of these same metrics rather than
/// repeating the numbers, since a layout tweak that skipped the measurement would
/// silently mis-size the box.
///
/// The leading glyph plus its `margin_right`:
pub(crate) fn completion_icon_w() -> f64 {
    completion_icon_size() + scaled(7.0)
}
/// The suggestion glyph's **base** size — what `completion_popup` hands
/// [`crate::icons::icon`], which scales it. [`completion_icon_size`] is the same
/// number *after* scaling, for the row-width arithmetic; passing that to `icon`
/// instead would scale it twice.
pub(crate) const COMPLETION_ICON_BASE: f32 = 13.0;
pub(crate) fn completion_icon_size() -> f64 {
    scaled(COMPLETION_ICON_BASE as f64)
}
/// The least the flex spacer holds between the name and the annotations:
pub(crate) fn completion_gap_w() -> f64 {
    scaled(24.0)
}
/// The detail's `margin_left` — charged even for an empty detail, since the margin
/// is on the node rather than the text:
pub(crate) fn completion_detail_gap() -> f64 {
    scaled(18.0)
}
/// The row's `padding_horiz`, per side:
pub(crate) fn completion_row_pad() -> f64 {
    scaled(10.0)
}
/// Suggestion name size and annotation size.
///
/// The name's base is the editor's default (14px), and this used to be described
/// as "matches the editor" — which it can only do by coincidence: the code font
/// has been settable for longer than that comment, and the interface scale moves
/// this and not that. It is the *popup's* type size — app chrome that happens to
/// list identifiers, sized with the rest of the chrome.
pub(crate) fn completion_name_size() -> f32 {
    theme::scaled_font(14.0)
}
pub(crate) fn completion_annot_size() -> f32 {
    theme::scaled_font(12.0)
}

/// The Ctrl+Enter run menu's width. The panel's `min_width` *and* what
/// `run_menu_pos` finds room for — one metric, because a placement computed
/// against a different width than the panel draws at is how the menu came to be
/// cut off at the editor's right edge in the first place.
pub(crate) fn run_menu_w() -> f64 {
    scaled(170.0)
}
/// One run-menu row: `theme::font_title()`'s line box plus its `padding_vert(8)`.
pub(crate) fn run_menu_row_h() -> f64 {
    scaled(34.0)
}
/// The whole menu: its two rows, the panel's `padding_vert(6)` and its 1px border.
///
/// An estimate, like [`completion_row_h`], because a style closure has no measured
/// box to ask. Only the bottom clamp reads it, so being a few px out costs a few px
/// of position — never a cut-off row.
pub(crate) fn run_menu_h() -> f64 {
    2.0 * run_menu_row_h() + scaled(12.0) + 2.0
}

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
/// drag limits live below (`schema_min_w`/`right_min_w`/`center_min_w`/…).
///
/// Scaled: the hit band is a pointer target, and at 160% the rest of the window
/// has grown around it — a 10px band on a display where everything else grew by
/// half again is the thing the scale exists to fix.
pub(crate) fn resize_hit() -> f64 {
    scaled(10.0)
}
pub(crate) fn resize_bar() -> f64 {
    scaled(3.0)
}

/// How long the pointer must **rest** on a divider before its bar lights up.
///
/// The highlight is an affordance — *this edge can be dragged* — and an
/// affordance that answers instantly answers far too often: the dividers run the
/// full height and width of the workspace, so crossing from the schema tree to
/// the editor, or from the editor to the results, lit one on the way past.
///
/// **200ms, and it was 500 first.** Half a second does stop the flashing, but it
/// also outlasts the gesture it is meant to serve: a pointer that has arrived on
/// the divider and stopped is already waiting, and half a second of nothing
/// reads as the app failing to notice. A fifth of a second is past the speed
/// anything crosses the workspace at while still landing inside the pause a hand
/// makes when it arrives somewhere on purpose.
///
/// Dragging is not delayed — it lights the bar the moment the press lands, and
/// the press works from the first pixel of the hit band whatever the bar is
/// doing. The delay is on the hint, never on the control.
pub(crate) const RESIZE_HOVER_DELAY: std::time::Duration = std::time::Duration::from_millis(200);

// ── Panel minimum dimensions + responsive breakpoints ───────────────────────

/// Minimum panel widths so a dragged — or auto-shrunk — panel stays legible
/// instead of squashing to a sliver. The center (query + results) is the
/// priority: the side panels yield width to keep it ≥ `CENTER_MIN_W`.
pub(crate) fn schema_min_w() -> f64 {
    scaled(250.0)
}
pub(crate) fn right_min_w() -> f64 {
    scaled(250.0)
}
pub(crate) fn center_min_w() -> f64 {
    scaled(400.0)
}
/// Minimum heights for the query editor and the results grid (drag + flex floor).
pub(crate) fn query_min_h() -> f64 {
    scaled(160.0)
}
pub(crate) fn results_min_h() -> f64 {
    scaled(190.0)
}
/// Responsive breakpoints on total window width. Below `panels_min_full_w` the
/// right panel (AI/terminal/history) is force-hidden and its toggle locked;
/// below `panels_min_schema_w` the schema panel is too. Each equals the summed
/// min widths of the panels that must fit, so a panel is only locked away once
/// there's genuinely no room for it beside the center.
///
/// Summed from the scaled minimums, which is what makes the breakpoints move
/// with the scale: at 160% three panels genuinely don't fit a 900px window, and
/// a breakpoint that stayed at 900 would keep them there, overlapping.
pub(crate) fn panels_min_full_w() -> f64 {
    schema_min_w() + center_min_w() + right_min_w() // 900 at Normal
}
pub(crate) fn panels_min_schema_w() -> f64 {
    schema_min_w() + center_min_w() // 650 at Normal
}
/// A left status-bar segment auto-hides once its right edge comes within this
/// many px of the footer's right-hand icon group (the AI icon's left edge), so
/// the two clusters never collide on a narrow window.
pub(crate) fn footer_collapse_gap() -> f64 {
    scaled(30.0)
}

/// How far above the footer bar's true centre its contents sit, in px.
///
/// A correction, not a design choice: everything in the bar — icons and labels
/// alike — read low against the 1px rule above it, which is the kind of drift
/// that is invisible until someone puts a straight edge on it. Applied once, as
/// `padding_bottom(footer_lift() * 2)` on the bar, so a later adjustment is one
/// number rather than a margin on each of fifteen segments.
///
/// Tuned by eye against [`crate::theme::footer_h`], which it
/// belongs with — and scaled with it, since a correction for a 28px bar is not
/// the correction for a 56px one. The bar's bottom is pinned to the window, so
/// growing it lifts its own centre too — when the footer went 26 → 28 this had to
/// come back down to 2 rather than up to 3, or the contents would have moved
/// twice.
pub(crate) fn footer_lift() -> f64 {
    scaled(2.0)
}

// ── Tab bar ─────────────────────────────────────────────────────────────────

/// Tab bar height. Flat, full-height tabs fill it edge to edge.
pub(crate) fn tab_bar_h() -> f64 {
    scaled(34.0)
}
/// Max width of a single query tab (px). The title truncates with an ellipsis
/// (+ full-name tooltip) past this; the inline rename field auto-widens up to it.
pub(crate) fn tab_max_w() -> f64 {
    scaled(200.0)
}

// ── AI chat input ───────────────────────────────────────────────────────────

pub(crate) const CHAT_MAX_ROWS: usize = 6;
pub(crate) fn chat_pad_v() -> f64 {
    scaled(6.0)
}
pub(crate) fn chat_pad_h() -> f64 {
    scaled(10.0)
}

/// Height of a **compact single-line field** — the one every transient bar uses:
/// the editor's find / replace / go-to-line, the grid's find and go-to-row, and
/// the row panel's inputs.
///
/// It exists as one metric because leaving it off is not a neutral default but
/// a *different* control: [`crate::FieldCfg::height`] is `Option`, and `None`
/// derives the box from content as `line_h + chat_pad_v() * 2 + 3` — 34px at the
/// 13px font these bars use, against this 26. The grid's find bar shipped
/// without it and so stood 8px taller than the identical editor bar beside it.
/// A bar that means to be compact says so with this, and never with a literal.
pub(crate) fn field_input_h() -> f64 {
    scaled(26.0)
}

// ── Data grid (interactive: sizing, selection, export) ──────────────────────

pub(crate) fn min_col_w() -> f64 {
    scaled(48.0)
}
pub(crate) fn max_col_w_init() -> f64 {
    scaled(420.0)
}
/// ≈ advance width of one character in the cell font, which is
/// [`crate::theme::font_body`] — so this scales with it. The column-width
/// estimate multiplies it by a character count, and an unscaled 7px against
/// 26px type would size every column to a third of its text.
pub(crate) fn grid_char_w() -> f64 {
    scaled(7.0)
}
/// Grab width of a column-resize divider.
pub(crate) fn resize_hit_w() -> f64 {
    scaled(7.0)
}
/// Two-line header (name + type).
pub(crate) fn grid_header_h() -> f64 {
    scaled(40.0)
}
/// Row-number gutter width (frozen).
pub(crate) fn gutter_w() -> f64 {
    scaled(52.0)
}
/// Right padding for right-aligned (numeric) headers + cells — a touch more than
/// the [`grid_pad_h`] sides so the value doesn't hug the edge/border. Header and
/// cell share it so numbers line up under their column name.
pub(crate) fn grid_num_pad_right() -> f64 {
    scaled(14.0)
}
/// Horizontal padding inside a grid cell / header (the numeric side swaps the
/// right one for [`grid_num_pad_right`]).
pub(crate) fn grid_pad_h() -> f64 {
    scaled(10.0)
}
/// The divider drawn on a cell's right edge. Named because it is a *border*, so
/// it comes out of the content box — anything computing how much room a cell has
/// for its value has to subtract it (see `numeric_edit_pad_left`).
pub(crate) const GRID_CELL_DIVIDER: f64 = 1.0;
/// Extra header width a key column needs over a plain one: its leading key icon
/// (14px) + gap (8px) beyond the normal side padding. Added to the width estimate
/// so a long type line (e.g. `INT UNSIGNED`) on a PK/FK column isn't clipped.
pub(crate) fn header_key_icon_w() -> f64 {
    scaled(22.0)
}
/// Height of the results grid's error/note strip (`grid::grid_error_bar`).
pub(crate) fn grid_bar_h() -> f64 {
    scaled(35.0)
}
/// The inset both floating bars keep from the panel edge. Not scaled — it is air,
/// like every other `padding` still literal here (see the module doc), and both
/// bars have to read it the *same* way or the gap between them is decided twice.
pub(crate) const GRID_BAR_INSET: f64 = 5.0;
/// How far off the bottom the selection summary sits: clear of the error bar when
/// that one is up, at the edge otherwise.
///
/// **One function, because the two bars used to state the same geometry twice.**
/// The error bar's own height became `scaled` and this lift stayed the literal 45
/// it had been derived as (`35 + 5 + 5`), so from Large upward the summary painted
/// *on top of* the bar it exists to sit above — the overlap the shared `any_up`
/// predicate is there to prevent.
pub(crate) fn grid_selection_lift(error_up: bool) -> f64 {
    if error_up {
        // The bar's top edge (its own inset + its height), plus this one's gap.
        grid_bar_h() + GRID_BAR_INSET * 2.0
    } else {
        SELECTION_BAR_INSET
    }
}
/// The selection summary's own inset at the panel edge — wider than
/// [`GRID_BAR_INSET`] because it floats over the cells rather than spanning them.
pub(crate) const SELECTION_BAR_INSET: f64 = 8.0;
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
///
/// **A length, so it scales** — and the module doc's "thresholds may stay a
/// `const`" does not cover it. What it is compared against is a scroll offset
/// measured in rows and line boxes, both of which scale: 30px is more than one
/// 26px grid row at Normal and *less* than one 52px row at Huge, so a single row
/// of overshoot — one wheel notch — silently dropped tail-following.
pub(crate) fn follow_slack() -> f64 {
    scaled(30.0)
}

// ── Menus / misc ────────────────────────────────────────────────────────────

/// Fixed width of the active-database menu (right-aligned under its trigger).
pub(crate) fn db_menu_w() -> f64 {
    scaled(170.0)
}

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
// `center_min_w()`, which on a narrow window is narrower than the intent — and
// anything that sizes itself to the panel has to use the same number the shell
// renders it at, or it lays out content into a clipped wrapper.
//
// The intents are px the user dragged, so they are *not* scaled; the minimums
// they are clamped against are, which is how a panel stops being draggable down
// to a width its own text no longer fits.

/// Does the schema panel fit beside the center at this window width? `0` (or any
/// width under 1) is "not measured yet" and counts as allowed, so nothing is
/// hidden before the first resize.
pub(crate) fn schema_panel_fits(window_w: f64) -> bool {
    !(1.0..panels_min_schema_w()).contains(&window_w)
}

/// Does the right (AI / terminal / history) panel fit beside the schema panel and
/// the center?
pub(crate) fn right_panel_fits(window_w: f64) -> bool {
    !(1.0..panels_min_full_w()).contains(&window_w)
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
        right_min_w(),
        (window_w - center_min_w() - schema_min_w()).max(right_min_w()),
    )
}

/// Rendered height of the query editor: 0 while collapsed, else the stored
/// intent floored at [`query_min_h`].
///
/// **The floor is applied here, at render, and never written back** — the rule
/// the panel widths above follow, and the one `editor_h` was the exception to.
/// `body` used to lift the signal itself (`editor_h.set(query_min_h())`) on
/// build, which the layout-persist effect then saved: harmless while the floor
/// was a `const` and a one-time migration for configs written under a looser one,
/// destructive once the floor started scaling. Choosing Huge rewrote a 200px
/// editor to 320 and *kept* it there on the way back to Normal, with the dragged
/// value gone and nothing to recover it from.
pub(crate) fn effective_editor_h(intended: f64, collapsed: bool) -> f64 {
    if collapsed {
        0.0
    } else {
        intended.max(query_min_h())
    }
}

/// Rendered width of the schema panel: 0 when hidden or locked away, else the
/// intent clamped so the center keeps `center_min_w()` beside the right panel's
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
        schema_min_w(),
        (window_w - center_min_w() - right_eff).max(schema_min_w()),
    )
}

#[cfg(test)]
mod scale_tests {
    use super::*;
    use crate::theme::UiScale;

    /// Run `f` at `scale`, then put the scale back.
    ///
    /// The registry is a `thread_local`, and libtest gives each test its own
    /// thread — but not when it is run with `--test-threads=1`, where several
    /// tests share one. Restoring is what keeps a scaled metric from leaking into
    /// the next test's assertions (which is a *silent* failure: every number
    /// would still be self-consistent, just 1.5× what the test expects).
    fn at<R>(scale: UiScale, f: impl FnOnce() -> R) -> R {
        crate::theme::set_ui_scale(scale);
        let out = f();
        crate::theme::set_ui_scale(UiScale::Normal);
        out
    }

    /// **The whole point of `Normal` being the exact identity**, checked through
    /// the metrics rather than the scaler: every number here is what the app
    /// shipped with before the setting existed, so a fresh install and an
    /// upgrade must lay out to the pixel as they did. Written out as literals on
    /// purpose — comparing a metric against `scaled(<its own base>)` would pass
    /// even if both moved.
    #[test]
    fn the_metrics_at_normal_are_the_numbers_the_app_shipped_with() {
        at(UiScale::Normal, || {
            assert_eq!(row_h(), 26.0);
            assert_eq!(tree_row_h(), 24.0);
            assert_eq!(schema_icon(), 16.0);
            assert_eq!(leaf_pad(), 75.0);
            assert_eq!(col_pad(), 65.0);
            assert_eq!(cell_w(), 190.0);
            assert_eq!(grid_header_h(), 40.0);
            assert_eq!(gutter_w(), 52.0);
            assert_eq!(grid_char_w(), 7.0);
            assert_eq!(field_input_h(), 26.0);
            assert_eq!(tab_bar_h(), 34.0);
            assert_eq!(tab_max_w(), 200.0);
            assert_eq!(run_menu_h(), 2.0 * 34.0 + 12.0 + 2.0);
            assert_eq!(completion_row_h(), 24.0);
            assert_eq!(completion_icon_w(), 20.0);
            assert_eq!(panels_min_full_w(), 900.0);
            assert_eq!(panels_min_schema_w(), 650.0);
            assert_eq!(crate::theme::header_h(), 40.0);
            assert_eq!(crate::theme::footer_h(), 28.0);
            assert_eq!(crate::theme::font_body(), 13.0);
        });
    }

    /// A derived indent is summed from **scaled parts**, not scaled after the
    /// fact, so it lands on the pixel the glyphs it aligns under actually
    /// occupy. The two are not the same number: at 80% the six parts round up
    /// to 61 while the sum (75) rounds down to 60 — which is exactly the kind of
    /// one-pixel disagreement that leaves the column rows hanging left of the
    /// table name they are meant to align with.
    #[test]
    fn a_derived_indent_is_summed_from_the_scaled_parts() {
        at(UiScale::Small, || {
            let parts = row_pad()
                + level_indent()
                + schema_icon()
                + chevron_gap()
                + schema_icon()
                + icon_gap();
            assert_eq!(leaf_pad(), parts);
            assert_eq!(leaf_pad(), 61.0);
            assert_eq!(crate::theme::scaled(75.0), 60.0, "and not this");
        });
    }

    /// The popup's width arithmetic and the glyph `icons::icon` actually draws
    /// have to agree, and they reach the same number by different routes: the
    /// arithmetic scales [`COMPLETION_ICON_BASE`] here, the view hands the *base*
    /// to `icon`, which scales it there.
    ///
    /// **What this can and can't catch.** It pins that the box reserves exactly
    /// one scaling — and, second assertion, that a second one would be a
    /// *visible* amount (17 → 22 at 130%), which is what the popup got when the
    /// call site read `icons::icon(icon, completion_icon_size() as f32)`, the
    /// obvious spelling. It cannot see the call site itself: the argument to a
    /// view constructor isn't observable from a unit test, so that half of the
    /// rule lives in the doc comments on `icons::icon` and on the base above.
    #[test]
    fn the_suggestion_glyph_is_scaled_exactly_once() {
        for scale in UiScale::ALL {
            at(scale, || {
                let once = crate::theme::scaled_font(COMPLETION_ICON_BASE) as f64;
                assert_eq!(
                    completion_icon_size(),
                    once,
                    "{} sizes the box and the glyph differently",
                    scale.label()
                );
                if scale != UiScale::Normal {
                    assert_ne!(
                        crate::theme::scaled(once),
                        once,
                        "{}: a second scaling has to be visible, or this rule \
                         wouldn't matter",
                        scale.label()
                    );
                }
            });
        }
    }

    /// The responsive breakpoints are summed from the *scaled* minimums, so they
    /// move with the scale. This is the composition that matters: at 160% three
    /// panels genuinely do not fit a 900px window, and a breakpoint frozen at
    /// 900 would keep them all on screen, overlapping.
    #[test]
    fn the_panel_breakpoints_move_with_the_scale() {
        at(UiScale::Normal, || {
            assert!(right_panel_fits(900.0));
            assert!(schema_panel_fits(650.0));
        });
        at(UiScale::Huge, || {
            assert!(!right_panel_fits(900.0), "1440 of minimums fit in 900");
            assert!(!schema_panel_fits(650.0));
            assert!(right_panel_fits(1800.0));
        });
        at(UiScale::Small, || {
            assert!(right_panel_fits(720.0), "0.8 × 900");
        });
    }

    /// **The diff never adds to an overflow.** It is the one child of the Ctrl+K
    /// column with a fixed height, so what it takes is what the buttons below it
    /// have left — and it is drawn in the *measured* editor area, which the user
    /// can drag and the scale can grow.
    ///
    /// The two numbers this pins are the two the old spelling got wrong: it read
    /// the unscaled `EDITOR_H` (so a pane dragged to 600 still sized a 113px diff
    /// at Normal) and it floored at `scaled(60)` (so at the 200% scale then
    /// offered, where 270 of chrome already overflowed a 248 box, it asked for
    /// 120 more). The 150px area is what keeps that second arm live now that the
    /// top scale is 160%: 216 of chrome fits the 248 seed, but not a pane
    /// dragged short.
    #[test]
    fn the_cmdk_diff_takes_what_is_left_and_never_more() {
        for scale in UiScale::ALL {
            at(scale, || {
                for area in [150.0, EDITOR_H, 300.0, 600.0, 1000.0] {
                    let (diff, chrome) = (cmdk_diff_h(area), cmdk_diff_chrome());
                    if chrome < area {
                        assert_eq!(
                            diff + chrome,
                            area,
                            "{}: a {area}px editor is filled exactly",
                            scale.label()
                        );
                    } else {
                        assert_eq!(
                            diff,
                            0.0,
                            "{}: {chrome} of chrome doesn't fit {area} — the diff \
                             must not ask for more on top of it",
                            scale.label()
                        );
                    }
                }
            });
        }
        at(UiScale::Huge, || {
            assert_eq!(cmdk_diff_h(EDITOR_H), EDITOR_H - 216.0, "216 of chrome");
            assert_eq!(cmdk_diff_h(600.0), 600.0 - 216.0);
            assert_eq!(cmdk_diff_h(150.0), 0.0, "216 of chrome in a 150px pane");
        });
    }

    /// The selection summary sits **above** the error bar, at every scale: past
    /// the bar's own top edge, which is its inset plus its scaled height. A
    /// literal lift cleared it at Normal and was inside it from Large up.
    #[test]
    fn the_selection_summary_clears_the_error_bar_at_every_scale() {
        at(UiScale::Normal, || {
            assert_eq!(
                grid_selection_lift(true),
                45.0,
                "the number the app shipped with"
            );
        });
        for scale in UiScale::ALL {
            at(scale, || {
                assert!(
                    grid_selection_lift(true) > GRID_BAR_INSET + grid_bar_h(),
                    "{}: the summary is inside the bar it sits above",
                    scale.label()
                );
                assert_eq!(
                    grid_selection_lift(false),
                    SELECTION_BAR_INSET,
                    "{}: with no bar up it sits at the edge",
                    scale.label()
                );
            });
        }
    }

    /// A dragged panel width is the user's own px and is *not* scaled — but the
    /// minimum it clamps against is, so raising the scale widens a panel that
    /// was sitting at the old floor rather than leaving its text clipped.
    #[test]
    fn a_persisted_panel_width_is_clamped_by_a_scaled_minimum() {
        at(UiScale::Normal, || {
            assert_eq!(effective_schema_w(1800.0, 260.0, 0.0, true), 260.0);
        });
        at(UiScale::Huge, || {
            assert_eq!(
                effective_schema_w(1800.0, 260.0, 0.0, true),
                400.0,
                "the stored 260 is under the scaled minimum"
            );
        });
    }
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
        assert_eq!(
            effective_schema_w(950.0, 300.0, right, true),
            schema_min_w()
        );
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

    /// **The stored editor height survives a scale it doesn't fit.**
    ///
    /// `body` used to lift the signal to the floor on build, and the layout-persist
    /// effect saved it: at Huge a dragged 200 became 256 and stayed 256 after
    /// switching back, with the 200 gone. Flooring at render keeps the intent, so
    /// the round trip returns it.
    #[test]
    fn a_stored_editor_height_is_floored_for_render_not_rewritten() {
        let stored = 200.0;
        crate::theme::set_ui_scale(crate::theme::UiScale::Huge);
        assert_eq!(query_min_h(), 256.0);
        assert_eq!(
            effective_editor_h(stored, false),
            256.0,
            "rendered at the scaled floor"
        );
        crate::theme::set_ui_scale(crate::theme::UiScale::Normal);
        assert_eq!(
            effective_editor_h(stored, false),
            stored,
            "and the stored intent is what comes back"
        );
    }

    #[test]
    fn a_collapsed_editor_is_zero_high_whatever_it_stores() {
        // Zero, not the floor: the results grid takes the whole region, and the
        // stored height is the restore value for un-collapsing.
        assert_eq!(effective_editor_h(400.0, true), 0.0);
        assert_eq!(effective_editor_h(0.0, true), 0.0);
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
        assert!(!right_panel_fits(panels_min_full_w() - 1.0));
        assert!(right_panel_fits(panels_min_full_w()));
        assert!(!schema_panel_fits(panels_min_schema_w() - 1.0));
        assert!(schema_panel_fits(panels_min_schema_w()));
    }
}
