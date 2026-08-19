//! Named colour accessors — the stable call-site API (`theme::bg_panel()`, …).
//!
//! These now read the *active* theme from [`crate::themes`] (a reactive signal),
//! so switching themes at runtime re-runs every `.style(…)` closure that calls
//! one. Adding/altering a colour means a field on [`crate::themes::UiTheme`] (or
//! [`crate::themes::EditorTheme`]) plus an accessor here — the call sites never
//! change.
//!
//! Editor-surface + syntax roles (`code_bg`, `suggest_*`, `syntax_underline`)
//! read the separate [`crate::themes::EditorTheme`] axis instead of the UI theme.

use floem::peniko::Color;

use crate::themes::{editor, ui};

// Re-export the switching API + kinds so callers use a single `theme::` surface.
pub use crate::themes::{
    EditorThemeKind, UiThemeKind, bump_editor_generation, editor_font_size, editor_generation,
    editor_soft_tabs, editor_tab_width, editor_word_wrap, init, parse_hex, set_editor,
    set_editor_font, set_editor_soft_tabs, set_editor_tab_width, set_editor_word_wrap, set_ui,
    ui_generation,
};

/// The active editor theme struct (surface + token palette) — for the SQL editor
/// wiring and the per-line lexer.
pub fn editor_theme() -> std::rc::Rc<crate::themes::EditorTheme> {
    editor()
}

// Surfaces, from deepest chrome to the editor surface.
pub fn bg_deepest() -> Color {
    ui().bg_deepest
} // footer
pub fn bg_chrome() -> Color {
    ui().bg_chrome
} // header
pub fn bg_panel() -> Color {
    ui().bg_panel
} // side panels
pub fn bg_editor() -> Color {
    ui().bg_editor
}

// Window caption buttons (`ui::window_chrome`) — the app draws its own now that
// the window has no system title bar.
pub fn caption_hover() -> Color {
    ui().caption_hover
} // minimize / maximize
pub fn caption_close_hover() -> Color {
    ui().caption_close_hover
}
/// The close glyph while its red hover is showing. Fixed white, like
/// [`env_badge_text`] and for the same reason: the fill underneath is a
/// saturated colour in every theme, so the mark on it doesn't vary with one.
pub fn caption_close_glyph() -> Color {
    Color::rgb8(0xFF, 0xFF, 0xFF)
}
// Code-editor surface — driven by the active *editor* theme.
pub fn code_bg() -> Color {
    editor().bg
}
// Autocomplete popup: outline + selected/hovered row background.
pub fn completion_border() -> Color {
    ui().completion_border
}
pub fn completion_active() -> Color {
    ui().completion_active
} // code view
// Outline of elevated modal panels (Find palette, error modal).
pub fn modal_border() -> Color {
    ui().modal_border
}

// Autocomplete row colors, tinted by suggestion kind — mirror the editor theme's
// token palette so completions match the code.
pub fn suggest_keyword() -> Color {
    editor().keyword
}
pub fn suggest_function() -> Color {
    editor().function
}
pub fn suggest_table() -> Color {
    editor().type_
}
pub fn suggest_database() -> Color {
    editor().constant
}

// Wavy underline under a probable keyword typo (a heuristic warning). Amber,
// editor-themed so it follows the syntax palette.
pub fn syntax_underline() -> Color {
    editor().underline
}

// Wavy underline under a definite diagnostic error (unknown table/column, a
// syntax error). Red, and *editor*-themed like the amber warning beside it: it
// is drawn on the editor surface, so a light editor theme has to be able to move
// it (a fixed red sat on Catppuccin Latte's near-white at 2.9:1).
pub fn diag_error() -> Color {
    editor().diag_error
}

/// Text on the top-bar environment badge — always white; it sits on the
/// connection's identity colour, so it reads the same across UI themes.
pub fn env_badge_text() -> Color {
    Color::rgb8(0xFF, 0xFF, 0xFF)
}

// AI-panel message send/stop icon (inside the message field).
pub fn ai_send_icon() -> Color {
    ui().ai_send_icon
}
pub fn ai_send_icon_active() -> Color {
    field_border_active() // same as the focused input border
}
pub fn ai_send_icon_hover() -> Color {
    ui().ai_send_icon_hover
}

// AI chat bubbles. User = a dim recap (right-aligned); Claude = the response.
pub fn bubble_user_bg() -> Color {
    ui().bubble_user_bg
}
pub fn bubble_claude_bg() -> Color {
    ui().bubble_claude_bg
}
pub fn bubble_claude_text() -> Color {
    ui().bubble_claude_text
}
pub fn bg_results() -> Color {
    ui().bg_results
} // table view
pub fn bg_header_row() -> Color {
    ui().bg_header_row
} // grid header

// Lines.
pub fn border() -> Color {
    ui().border
}

/// The dark scrim behind a centered modal (50% black — theme-independent).
pub fn modal_backdrop() -> Color {
    Color::rgb8(0, 0, 0).multiply_alpha(0.5)
}

/// Soft drop shadow under floating tooltips (translucent black — theme-independent,
/// reads as a shadow on both light and dark chrome).
pub fn tooltip_shadow() -> Color {
    Color::rgb8(0, 0, 0).multiply_alpha(0.35)
}

// Panel resize divider: the 3px overlay shown while hovering/dragging a handle.
pub fn resize_handle() -> Color {
    ui().resize_handle
}

// Text-field outlines: resting vs. focused/active.
pub fn field_border() -> Color {
    ui().field_border
}
pub fn field_border_active() -> Color {
    ui().field_border_active
}
/// Border around the picked statement (Explain/Optimize/Run Current).
pub fn query_highlight() -> Color {
    ui().query_highlight
}

/// Box around the paren matching the one under the caret (bracket matching).
pub fn bracket_match() -> Color {
    ui().bracket_match
}

/// Matched-substring highlight (bold) in the command palette / Find results.
pub fn match_highlight() -> Color {
    ui().match_highlight
}

// Text.
pub fn text() -> Color {
    ui().text
}
pub fn text_dim() -> Color {
    ui().text_dim
}
pub fn text_muted() -> Color {
    ui().text_muted
}
/// Placeholder text in input fields (dimmer than `text_muted`).
pub fn placeholder() -> Color {
    ui().placeholder
}

// Status-bar panel toggles: idle (panel closed) vs active (panel open).
pub fn chip_idle() -> Color {
    ui().chip_idle
}
pub fn chip_active() -> Color {
    ui().chip_active
}

// Query + results tabs (flat, full-height).
pub fn tab_active() -> Color {
    ui().tab_active
}
// Vertical line between tabs + the full-width strip separators.
pub fn tab_separator() -> Color {
    ui().tab_separator
}
// Inactive tab label/×; brightens to `text` on hover and when active.
pub fn tab_text() -> Color {
    ui().tab_text
}
// The tab close (×) glyph — a fixed, muted tint, independent of the label colour.
pub fn tab_close() -> Color {
    ui().tab_close
}

// Accent (selection, active connection dot, focus).
pub fn accent() -> Color {
    ui().accent
}

// ── Inline (Ctrl+K) AI prompt + diff overlay ─────────────────────────────────
pub fn cmdk_placeholder() -> Color {
    ui().cmdk_placeholder
}
pub fn cmdk_text() -> Color {
    ui().cmdk_text
}
// Diff rows: tinted line backgrounds + brighter +/- gutter markers.
pub fn diff_add_bg() -> Color {
    ui().diff_add_bg
}
pub fn diff_del_bg() -> Color {
    ui().diff_del_bg
}
pub fn diff_add_marker() -> Color {
    ui().diff_add_marker
}
pub fn diff_del_marker() -> Color {
    ui().diff_del_marker
}
// Editor error bar: "View" and "AI Fix" text buttons.
pub fn err_fix_btn() -> Color {
    ui().err_fix_btn
}

// Approve / Reject buttons on the diff overlay.
pub fn approve_bg() -> Color {
    ui().approve_bg
}
pub fn approve_text() -> Color {
    ui().approve_text
}
pub fn reject_bg() -> Color {
    ui().reject_bg
}
pub fn reject_text() -> Color {
    ui().reject_text
}

// Faint slate for secondary metadata (e.g. the connection endpoint in the menu).
pub fn text_faint() -> Color {
    ui().text_faint
}

// Tree rows: hover and active (selected) backgrounds.
pub fn row_hover() -> Color {
    ui().row_hover
}
/// The quieter hover, for a list whose rows are blocks rather than lines (the
/// query history's).
pub fn row_hover_soft() -> Color {
    ui().row_hover_soft
}
/// The band behind a query-history recency header (TODAY / THIS WEEK /
/// EARLIER): a shade of [`bg_panel`], one step down at the same hue.
pub fn group_header_bg() -> Color {
    ui().group_header_bg
}
pub fn row_active() -> Color {
    ui().row_active
}
// Keyboard-navigation cursor: the selected row while the schema panel has nav focus.
pub fn row_selected() -> Color {
    ui().row_selected
}
/// The 1px rule drawn above and below the schema-tree row whose **context menu is
/// open**, so a menu the pointer has walked away from still says what it applies
/// to.
///
/// A rule and not a background, because the row underneath may already be
/// carrying one — the active database's, or the nav cursor's — and a marker that
/// replaced those would say less than it added.
///
/// `text_muted` rather than a field of its own in both palettes: the brief is
/// "quiet but legible on the panel and on a hovered row", which is the role that
/// token already fills, and it is the *right direction* in each palette rather
/// than one hex that happens to suit the dark one — darker than `text_dim` on dark
/// (`#585C6A` under `#7E8294`), lighter than it on light (`#8A8F9E` over
/// `#5C6270`). It began as `text_dim` and read as too bright for a 1px line, which
/// makes sense: `text_dim` is tuned to be *read* as text. Retune here if it ever
/// needs a hue of its own.
pub fn row_menu_edge() -> Color {
    ui().text_muted
}
// Pill tabs (the table designer's section tabs): the active pill's fill and
// label, and the hover fill of an inactive one.
pub fn pill_active_bg() -> Color {
    ui().pill_active_bg
}
pub fn pill_active_text() -> Color {
    ui().pill_active_text
}
pub fn pill_hover_bg() -> Color {
    ui().pill_hover_bg
}
// A modal footer's actions: a fill, its hover and a matching label, per variant.
// A disabled action keeps its fill and halves its label (see `action_button`), so
// the button holds its place rather than vanishing.
pub fn btn_neutral() -> Color {
    ui().btn_neutral
}
pub fn btn_neutral_hover() -> Color {
    ui().btn_neutral_hover
}
pub fn btn_neutral_text() -> Color {
    ui().btn_neutral_text
}
pub fn btn_primary() -> Color {
    ui().btn_primary
}
pub fn btn_primary_hover() -> Color {
    ui().btn_primary_hover
}
pub fn btn_primary_text() -> Color {
    ui().btn_primary_text
}
pub fn btn_quiet() -> Color {
    ui().btn_quiet
}
pub fn btn_quiet_hover() -> Color {
    ui().btn_quiet_hover
}
pub fn btn_quiet_text() -> Color {
    ui().btn_quiet_text
}
pub fn btn_danger() -> Color {
    ui().btn_danger
}
pub fn btn_danger_hover() -> Color {
    ui().btn_danger_hover
}
pub fn btn_danger_text() -> Color {
    ui().btn_danger_text
}
// Manage Connections: the pass/fail icon Test flashes in place of its label.
pub fn conn_test_ok() -> Color {
    ui().conn_test_ok
}
pub fn conn_test_fail() -> Color {
    ui().conn_test_fail
}
// Manage Connections list rows: resting text, hovered/selected text, selected bg.
pub fn conn_list_text() -> Color {
    ui().conn_list_text
}
pub fn conn_list_sel_text() -> Color {
    ui().conn_list_sel_text
}
pub fn conn_list_sel_bg() -> Color {
    ui().conn_list_sel_bg
}
// Manage Connections' three footer actions used to carry seven colours of their
// own — coloured *text*, plus a green tick and a red cross for the test result.
// They wear the shared `btn_*` fills now, like every other modal's footer, so
// those roles are gone rather than left tuneable: a colour nothing paints is one
// a later retune spends time on for no effect.

// Count-capsule fill ("N cols" / "N keys" under a table).
pub fn capsule_bg() -> Color {
    ui().capsule_bg
}

// Database visibility menu: row text — shown (enabled) vs hidden (disabled).
pub fn db_toggle_on() -> Color {
    ui().db_toggle_on
}
pub fn db_toggle_off() -> Color {
    ui().db_toggle_off
}

// Schema tree: database and table glyph tints.
pub fn db_icon() -> Color {
    ui().db_icon
}
pub fn table_icon() -> Color {
    ui().table_icon
}
// VIEW glyph tint (a table-cells-merge icon), distinct from base tables.
pub fn view_icon() -> Color {
    ui().view_icon
}

// Results grid: selected-column header bg.
pub fn grid_col_sel() -> Color {
    ui().grid_col_sel
}
pub fn grid_edit_staged() -> Color {
    ui().grid_edit_staged
}
pub fn grid_edit_staged_hover() -> Color {
    ui().grid_edit_staged_hover
}
pub fn grid_edit_discard() -> Color {
    ui().grid_edit_discard
}
pub fn grid_edit_discard_hover() -> Color {
    ui().grid_edit_discard_hover
}

// Schema tree: key/column accents by kind.
pub fn key_primary() -> Color {
    ui().key_primary
}
pub fn key_index() -> Color {
    ui().key_index
}
pub fn key_foreign() -> Color {
    ui().key_foreign
}
/// Gold star marking a favorited database in the schema tree.
pub fn favorite_star() -> Color {
    ui().favorite_star
}

// ER-diagram modal surfaces.
pub fn erd_canvas() -> Color {
    ui().erd_canvas
}
pub fn erd_dot() -> Color {
    ui().erd_dot
}
pub fn erd_node_bg() -> Color {
    ui().erd_node_bg
}
pub fn erd_node_header() -> Color {
    ui().erd_node_header
}
/// Column-row background when the row is an endpoint of the hovered edge.
pub fn erd_row_highlight() -> Color {
    ui().erd_row_highlight
}
pub fn erd_edge() -> Color {
    ui().erd_edge
}
pub fn erd_edge_hover() -> Color {
    ui().erd_edge_hover
}
/// ER-diagram toolbar strip top/bottom border.
pub fn erd_toolbar_border() -> Color {
    ui().erd_toolbar_border
}
/// ER-diagram toolbar control border + zoom-unit separators.
pub fn erd_control_border() -> Color {
    ui().erd_control_border
}

// ── Small toolbar controls ──────────────────────────────────────────────────
// The ER-diagram toolbar's button chrome, named for the role rather than the
// place now that the header's Retry uses it too. They deliberately share the
// ER-diagram control palette — same role, same surface, and they should retune
// together. Split into their own theme fields if that ever stops being true.
pub fn control_bg() -> Color {
    ui().erd_canvas
}
pub fn control_border() -> Color {
    ui().erd_control_border
}
pub fn control_hover() -> Color {
    ui().erd_node_bg
}

// Schema search placeholder / faint input text.
pub fn search_hint() -> Color {
    ui().search_hint
}

// Error text (failed queries).
pub fn error() -> Color {
    ui().error
}

// Query-plan modal: amber for heuristic warning rows/icons.
pub fn plan_warn() -> Color {
    ui().plan_warn
}

// Query-plan modal: background tint behind warnings + flagged rows.
pub fn plan_warn_bg() -> Color {
    ui().plan_warn_bg
}

// ── Status bar (footer) ──────────────────────────────────────────────────
// These used to be fixed literals, "theme-independent by design". They were
// chosen against a dark footer, and `UiTheme::light` later moved that footer to
// #DCDFE6 without them: thirteen of the fourteen ended up under 3:1, the
// open-transaction pill at 1.48:1. They are ordinary theme fields now, and
// `crate::contrast` is the gate that keeps every one of them legible on the
// surface it is actually painted on.

/// Muted grey for status-bar text + icons.
pub fn status_text() -> Color {
    ui().status_text
}
/// Amber for the syntax-warning icon + count.
pub fn status_warn() -> Color {
    ui().status_warn
}
/// Brighter amber for hovering the write-mode status segment.
pub fn status_warn_hover() -> Color {
    ui().status_warn_hover
}
/// Green for the "no warnings" check.
pub fn status_ok() -> Color {
    ui().status_ok
}
/// Green CTA *fill* for the AI "Seed rows" popover Generate button — the one
/// here that is a background (white text sits on it), so on a light theme it
/// wants the opposite treatment to the others.
pub fn seed_button() -> Color {
    ui().seed_button
}
/// The table designer's "N changes" count, when there *are* changes. Same value
/// as `status_ok` in both palettes and kept separate for the usual reason: that
/// one says "your SQL is clean", this one says "you have unsaved schema edits" —
/// they'd want retuning apart the moment either is touched.
pub fn change_count() -> Color {
    ui().change_count
}

/// A tab in manual-commit mode, and its open-transaction pill. Its own colour
/// rather than `status_warn`'s amber: an open transaction is a *state you're
/// holding*, not a warning about your SQL, and the two want to be retunable
/// apart.
pub fn tx_open() -> Color {
    ui().tx_open
}
/// Hover for the clickable manual-mode / Commit / Rollback footer segments.
pub fn tx_open_hover() -> Color {
    ui().tx_open_hover
}
/// A transaction that can't go forward — PostgreSQL aborted it, or the pinned
/// connection died. Used on the modal, where red reads cleanly against the
/// panel; the status bar's Rollback uses `tx_rollback` instead.
pub fn tx_danger() -> Color {
    ui().tx_danger
}
/// Green for the status bar's Commit action. Same value as `status_ok`, kept
/// separate: one is "your SQL is clean", this is an action.
pub fn tx_commit() -> Color {
    ui().tx_commit
}
/// Brighter green for hovering Commit.
pub fn tx_commit_hover() -> Color {
    ui().tx_commit_hover
}
/// Red for the status bar's Rollback action — the same red as the confirmation
/// modal's Roll back, so the discard action reads the same in both places. Kept
/// as its own fn so it can be warmed up (or taken back to the write-mode amber)
/// without touching the modal.
pub fn tx_rollback() -> Color {
    ui().tx_rollback
}
/// Brighter red for hovering Rollback.
pub fn tx_rollback_hover() -> Color {
    ui().tx_rollback_hover
}

/// The affirmative button in the generic confirm modal, and the destructive
/// Apply in the DDL preview. Starts at the same red as the transaction reds, but
/// kept separate on purpose: this one answers "yes, do the destructive thing"
/// for *any* action, so it should be retunable without dragging Rollback along
/// with it. It is **text**, not a fill — `footer_button` takes it as a colour.
pub fn confirm_yes() -> Color {
    ui().confirm_yes
}
/// Brighter red for hovering the confirm modal's Yes.
pub fn confirm_yes_hover() -> Color {
    ui().confirm_yes_hover
}

// Connection status: reachable (unreachable reuses `reject_bg`).
pub fn conn_ok() -> Color {
    ui().conn_ok
}

// Dropdown popup: hovered option row + the currently-selected option's resting bg.
pub fn dropdown_hover() -> Color {
    ui().dropdown_hover
}
pub fn dropdown_active() -> Color {
    ui().dropdown_active
}

// Code-block action bar (copy / insert / run), floated over the block.
pub fn code_action_bar() -> Color {
    ui().code_action_bar
}

// AI-panel jump-to-bottom button: chevron icon, resting + hover.
pub fn jump_icon() -> Color {
    ui().jump_icon
}
pub fn jump_icon_hover() -> Color {
    ui().jump_icon_hover
}

// Settings toggle switch: track + handle, by on/off state.
pub fn toggle_on() -> Color {
    ui().toggle_on
}
pub fn toggle_on_hover() -> Color {
    ui().toggle_on_hover
}
pub fn toggle_off() -> Color {
    ui().toggle_off
}
pub fn toggle_off_hover() -> Color {
    ui().toggle_off_hover
}
pub fn toggle_handle_on() -> Color {
    ui().toggle_handle_on
}
pub fn toggle_handle_off() -> Color {
    ui().toggle_handle_off
}

// Scrollbar handle: resting fill + brighter hover.
pub fn scrollbar() -> Color {
    ui().scrollbar
}
pub fn scrollbar_hover() -> Color {
    ui().scrollbar_hover
}

// Fixed chrome dimensions (logical px).
pub const HEADER_H: f64 = 40.0;
pub const FOOTER_H: f64 = 26.0;
pub const SCHEMA_W: f64 = 300.0;
// AI and Terminal share this width (see `TERM_W` in lib.rs).
pub const AI_W: f64 = 350.0;

// Type scale (logical px). Design rule: nothing smaller than 13px anywhere
// except the status-bar footer (`FONT_STATUS`).
pub const FONT_TITLE: f32 = 14.0;
pub const FONT_BODY: f32 = 13.0;
pub const FONT_LABEL: f32 = 13.0;
/// A form hint — one step under the label it explains.
pub const FONT_HINT: f32 = 12.0;
pub const FONT_STATUS: f32 = 12.0;
