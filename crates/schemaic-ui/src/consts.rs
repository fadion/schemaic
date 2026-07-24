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

/// Monospace family used by the Ctrl+K diff (matches the editor exactly).
pub(crate) const DIFF_MONO: &str = "IBM Plex Mono";

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

// ── Data grid (interactive: sizing, selection, export) ──────────────────────

pub(crate) const MIN_COL_W: f64 = 48.0;
pub(crate) const MAX_COL_W_INIT: f64 = 420.0;
pub(crate) const GRID_CHAR_W: f64 = 7.0; // ≈ advance width of the 13px cell font
pub(crate) const RESIZE_HIT_W: f64 = 7.0; // grab width of a column-resize divider
pub(crate) const GRID_HEADER_H: f64 = 40.0; // two-line header (name + type)
pub(crate) const GUTTER_W: f64 = 52.0; // row-number gutter width (frozen)

/// Auto-hide delay (ms) for the overlay scrollbars after scrolling stops.
pub(crate) const SCROLL_HIDE_MS: u64 = 3000;

// ── Menus / misc ────────────────────────────────────────────────────────────

/// Fixed width of the active-database menu (right-aligned under its trigger).
pub(crate) const DB_MENU_W: f64 = 170.0;

/// The masking glyph used by password fields. Must be a single ASCII byte so the
/// buffer's byte length tracks its char length and the cursor stays valid.
pub(crate) const MASK_CH: char = '*';

// ── Terminal ────────────────────────────────────────────────────────────────

/// Terminal font sizes offered in settings (logical px).
pub(crate) const TERM_FONT_SIZES: [u16; 5] = [12, 13, 14, 16, 18];
