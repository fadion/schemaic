//! Named colour accessors — the stable call-site API (`theme::bg_panel()`, …).
//!
//! These now read the *active* theme from [`crate::themes`] (a reactive signal),
//! so switching themes at runtime re-runs every `.style(…)` closure that calls
//! one. Adding/altering a colour means a field on [`crate::themes::UiTheme`] (or
//! [`EditorTheme`]) plus an accessor here — the call sites never change.
//!
//! Editor-surface + syntax roles (`code_bg`, `suggest_*`, `syntax_underline`)
//! read the separate [`crate::themes::EditorTheme`] axis instead of the UI theme.

use floem::peniko::Color;

use crate::themes::{editor, ui};

// Re-export the switching API + kinds so callers use a single `theme::` surface.
pub use crate::themes::{
    EditorThemeKind, UiThemeKind, editor_font_size, editor_generation, editor_soft_tabs,
    editor_tab_width, editor_word_wrap, init, parse_hex, set_editor, set_editor_font,
    set_editor_soft_tabs, set_editor_tab_width, set_editor_word_wrap, set_ui,
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

// Wavy underline under a misspelled keyword.
pub fn syntax_underline() -> Color {
    editor().underline
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
pub fn row_active() -> Color {
    ui().row_active
}
// Keyboard-navigation cursor: the selected row while the schema panel has nav focus.
pub fn row_selected() -> Color {
    ui().row_selected
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
// Manage Connections: Delete button (icon+text), resting + hover.
pub fn conn_delete() -> Color {
    ui().conn_delete
}
pub fn conn_delete_hover() -> Color {
    ui().conn_delete_hover
}
// Manage Connections: Save button text, resting + hover.
pub fn conn_save() -> Color {
    ui().conn_save
}
pub fn conn_save_hover() -> Color {
    ui().conn_save_hover
}
// Manage Connections: Test button text, resting + hover.
pub fn conn_test() -> Color {
    ui().conn_test
}
pub fn conn_test_hover() -> Color {
    ui().conn_test_hover
}
// Manage Connections: Test result icon — success (failure reuses `conn_delete`).
pub fn conn_test_ok() -> Color {
    ui().conn_test_ok
}

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
// Fixed accents, theme-independent by design: the footer reads the same muted
// grey and the two semantic accents in every theme.

/// Muted grey for status-bar text + icons (`#6E7181`).
pub fn status_text() -> Color {
    Color::rgb8(0x6E, 0x71, 0x81)
}
/// Amber for the syntax-warning icon + count (`#E08A4B`).
pub fn status_warn() -> Color {
    Color::rgb8(0xE0, 0x8A, 0x4B)
}
/// Brighter amber for hovering the write-mode status segment (`#FFA461`).
pub fn status_warn_hover() -> Color {
    Color::rgb8(0xFF, 0xA4, 0x61)
}
/// Green for the "no warnings" check (`#71C371`).
pub fn status_ok() -> Color {
    Color::rgb8(0x71, 0xC3, 0x71)
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
pub const FONT_STATUS: f32 = 12.0;
