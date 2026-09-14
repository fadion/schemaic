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
    DEFAULT_EDITOR_FONT, EDITOR_FONT_MAX, EDITOR_FONT_MIN, EditorThemeKind, UiScale, UiThemeKind,
    bump_editor_generation, clamped_editor_font, clamped_tab_width, editor_font_size,
    editor_generation, editor_soft_tabs, editor_tab_width, editor_word_wrap, init, parse_hex,
    scale_at, scale_font_at, scaled, scaled_font, set_editor, set_editor_font,
    set_editor_soft_tabs, set_editor_tab_width, set_editor_word_wrap, set_ui, set_ui_scale,
    ui_generation, ui_scale,
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
/// The close glyph while its red hover is showing. Fixed white, because the fill
/// underneath is one saturated red in every theme — `caption_close_hover`
/// `#C42B1C`, which measures 5.66:1 against white.
///
/// This used to cite [`env_badge_text_on`] as the same case. It was not: the
/// badge's fill is *eight* colours the app hands out itself, three of them pale,
/// and fixed white measured 1.75:1 on one of them. One fixed red and a palette
/// of eight are different questions, and the shared comment is what made the
/// badge's exclusion read as considered.
pub fn caption_close_glyph() -> Color {
    Color::rgb8(0xFF, 0xFF, 0xFF)
}
// Code-editor surface — driven by the active *editor* theme.
pub fn code_bg() -> Color {
    editor().bg
}

/// The surface a **syntax-coloured preview** paints on — the History panel's and
/// the Snippet Library's — and [`preview_fg`] the colour of the text on it that
/// is not a token.
///
/// **Both come from the editor axis, and that is the whole point.** The token
/// colours are reproductions of published palettes (One Dark Pro, Tokyo Night,
/// Catppuccin Latte) tuned against their own background, and the editor theme is
/// chosen *independently* of the light/dark UI theme — so pairing them with a UI
/// surface is a cross-axis combination nobody chose. Both previews used to paint
/// [`bg_editor`], which despite the name is the UI theme's **text-field**
/// surface (`contrast.rs`'s own entry says so), and on the Light UI theme with
/// the shipped-default Tokyo Night that put `string` at 1.70:1 and `number` at
/// 1.90:1 — under even the *Recessive* floor of 2.0, while the uncoloured text
/// beside them sat at 5.70:1. The text the colour was added to make stand out
/// was the only text made unreadable.
///
/// Named here rather than spelled at each site so the cross-axis gate in
/// `contrast.rs` measures the surface the previews actually paint.
pub fn preview_bg() -> Color {
    code_bg()
}

/// The base colour of a syntax-coloured preview's non-token text — see
/// [`preview_bg`] for why it is the editor's own foreground.
pub fn preview_fg() -> Color {
    editor().fg
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

/// Text on the top-bar environment badge, **for the fill it is drawn on**.
///
/// This was a fixed white, on the reasoning that the badge "sits on the
/// connection's identity colour, so it reads the same across UI themes". It read
/// the same and it read badly: measured against the eight shipped
/// `CONN_COLOR_PRESETS`, white is **1.75:1 on Amber** — under
/// `contrast::Legibility::Recessive`'s 2.0, the floor that exists only to catch
/// outright invisibility — and under Body on **eight of eight**. The badge's
/// whole job is to say which environment the active connection is, which is the
/// label you least want unreadable on a production connection.
///
/// The exclusion that let it ship said no theme can promise a ratio on "an
/// arbitrary connection colour". It is not arbitrary: the connection form's
/// colour control is a row of swatches over `CONN_COLOR_PRESETS` and has no
/// free-hex entry, and the very next clause of that same sentence says so about
/// the ERD card header, which washes *the same eight* and is measured. The badge
/// is the easier case — an undiluted fill, no wash alpha.
///
/// So the decision is made per fill, and it is made by **measuring** rather than
/// by a luminance threshold. A threshold is a guess at where the crossover is;
/// `contrast_ratio` is the function the pairing tables already hold every other
/// colour in this app to, and asking it directly cannot be off by a preset. On
/// the eight shipped today it answers dark for all eight — white's best is
/// 3.82:1 on Red, against dark's worst of 5.02:1 — and a darker ninth preset
/// would correctly get white without this being touched.
///
/// **A near-black rather than `#000`.** Nothing in this app's chrome is pure
/// black in any theme, and the margin is there: the worst case is 5.02:1
/// against Body's 4.5. `the_env_badge_label_is_legible_on_every_connection_preset`
/// is what says so, rather than this comment claiming it.
pub fn env_badge_text_on(fill: Color) -> Color {
    const DARK: Color = Color::rgb8(0x1A, 0x1A, 0x1F);
    const LIGHT: Color = Color::rgb8(0xFF, 0xFF, 0xFF);
    let (dark, light) = (
        crate::contrast::contrast_ratio(DARK, fill),
        crate::contrast::contrast_ratio(LIGHT, fill),
    );
    if dark >= light { DARK } else { LIGHT }
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

// AI chat. The user's question keeps a bubble — a dim right-aligned recap; the
// assistant's answer has no surface of its own, so its text sits on `bg_panel`.
pub fn bubble_user_bg() -> Color {
    ui().bubble_user_bg
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
// Hover brighten for an accent-coloured control with no fill behind it — the
// results strip's read-more offer. Moves away from the surface, which is not
// the same direction in both themes; see `UiTheme::accent_hover`.
pub fn accent_hover() -> Color {
    ui().accent_hover
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
// Editor error bar: "View" and "AI fix" text buttons.
pub fn err_fix_btn() -> Color {
    ui().err_fix_btn
}
pub fn err_fix_btn_hover() -> Color {
    ui().err_fix_btn_hover
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
/// with it. It is **text**, not a fill — `dialog_button` takes it as a colour fn.
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

// Chrome dimensions (logical px). Functions, not constants, because the
// interface scale multiplies them — see `UiScale`, and call them *inside* the
// style closure exactly as you would a colour.

/// Height of the window's header bar.
pub fn header_h() -> f64 {
    scaled(40.0)
}
/// The rule under the header, and part of its [`header_h`] (border-box), so it
/// occupies the bar's last logical pixel. Named because a second view has to
/// find it: the band `window_chrome::over_backdrop` lays over a modal stops
/// short of the caption buttons, and the border running on under them has to be
/// dimmed separately or the rule ends in a lit tail.
///
/// A `const`, and unscaled: it is a hairline, and a hairline is one physical
/// rule at every size — 2px of it at 200% would read as a border rather than a
/// seam.
pub const HEADER_BORDER: f64 = 1.0;
/// Height of the status-bar footer.
pub fn footer_h() -> f64 {
    scaled(28.0)
}
/// Default width of the schema sidebar.
///
/// Unscaled, deliberately: this is the seed for a **persisted, user-dragged**
/// width (`persist::UiState::schema_w`), and the stored number is the user's own
/// intent in px. Scaling a restored width would move a panel the user had placed
/// by hand every time the scale changed. What does scale is
/// [`crate::consts::schema_min_w`], so the panel can't be dragged narrower than
/// its own text.
pub const SCHEMA_W: f64 = 300.0;
/// Default width of the right column. AI and Terminal share it (see `TERM_W` in
/// lib.rs). Unscaled, for the reason on [`SCHEMA_W`].
pub const AI_W: f64 = 350.0;

// Type scale (logical px), at the active interface scale. Design rule: nothing
// smaller than `font_body()` anywhere except the status-bar footer
// (`font_status()`).

pub fn font_title() -> f32 {
    scaled_font(14.0)
}
pub fn font_body() -> f32 {
    scaled_font(13.0)
}
pub fn font_label() -> f32 {
    scaled_font(13.0)
}
/// A form hint — one step under the label it explains.
///
/// **The step is taken, not assumed.** Four base values a single pixel apart
/// (14/13/13/12/12) go through a rounding that is not size-preserving below
/// 1.0, so at 80% `font_body`, `font_label`, `font_hint` and `font_status` all
/// came back 10.0 — this doc was false, and the section header's "nothing
/// smaller than `font_body()` anywhere except the status-bar footer" had an
/// empty carve-out, because the footer was no longer smaller either. Every form
/// hint and every footer segment rendered at label size, at the one setting
/// chosen by someone who can least afford four type sizes reading as one.
///
/// `min` rather than a wider base: it changes **only** the scale that
/// collapsed. 80% goes 10 → 9; 100% (12), 130% (16) and 160% (19) are the
/// numbers the app already shipped, which a base change would have moved
/// everywhere.
pub fn font_hint() -> f32 {
    scaled_font(12.0).min(font_body() - 1.0)
}
/// The status-bar footer — the one carve-out the type scale's rule names, so it
/// keeps its step for [`font_hint`]'s reason and in the same way.
pub fn font_status() -> f32 {
    scaled_font(12.0).min(font_body() - 1.0)
}

/// **The gateable half of "themable colours reach reactive styles as
/// `fn() -> Color`, never a captured `Color`".**
///
/// The rule itself cannot be checked by scanning for `theme::` tokens, and five
/// review passes were spent establishing that before it was written down: a
/// `theme::` accessor returns a `Color` by value, so a site is wrong iff the
/// colour is produced *outside* the closure that paints it **and** the view
/// holding it is not rebuilt on that colour's axis — both properties of the
/// enclosing construct rather than of the line the token is on. The fully
/// corrected grep is ~2% precise (six true violations in 273 candidates), and
/// worse, the deciding site frequently holds no `theme::` token at all: the
/// crate has fifteen helpers returning a bare `Color`, each correct in itself,
/// with the rule decided at the caller. `monitor_view`'s was the proof — the
/// violation was `("INSERT", new_color())` in a tuple destructure while
/// `new_color`'s body, the only place a grep hits, was fine.
///
/// What *can* be asserted is the half that is **empty by construction**: every
/// colour a view is *given* is declared `fn() -> Color` in this crate, and the
/// five bare `: Color` parameter positions outside `themes.rs` are all in code
/// that measures or tints rather than paints. That emptiness is worth a gate
/// precisely because it would grow silently — `fn row(c: Color) -> impl IntoView`
/// compiles and reads fine, and is the argument-position capture the invariant
/// exists to prevent.
///
/// The template is `dividers::scaled_arg_gate`, which enforces the *size* half
/// of this same invariant, down to the `EXEMPT` triple and the floor test that
/// stops an exemption outliving what it licensed.
#[cfg(test)]
mod color_arg_gate {
    /// `(file, parameter, why a bare `Color` is right for it)`.
    ///
    /// Each of these is code that *computes with* a colour rather than painting
    /// one: nothing here is handed a colour to draw with later, so there is
    /// nothing to freeze at a theme.
    const EXEMPT: &[(&str, &str, &str)] = &[
        (
            "contrast.rs",
            "c",
            "`relative_luminance` — a measurement over a colour it is given, \
             with no view and no closure anywhere near it.",
        ),
        (
            "contrast.rs",
            "a",
            "`contrast_ratio`'s first operand, same reason.",
        ),
        (
            "contrast.rs",
            "b",
            "`contrast_ratio`'s second operand, same reason.",
        ),
        (
            "contrast.rs",
            "fg",
            "`over` composites two colours and returns one; the caller reads \
             both live and hands the result straight to a style closure.",
        ),
        (
            "contrast.rs",
            "bg",
            "`over`'s background operand, same reason.",
        ),
        (
            "sql_highlight.rs",
            "bg",
            "`band`, a closure **inside** the styling hook — re-entered on every \
             restyle, so the colour it takes was read live by its caller in the \
             same pass.",
        ),
        (
            "sql_highlight.rs",
            "c",
            "`tint`, the other closure inside the same hook: it fades a colour \
             its caller read live in this pass and hands the result straight \
             back.",
        ),
        (
            "theme.rs",
            "fill",
            "`env_badge_text_on` picks black or white *for* a fill it is given. \
             Its answer is a function of the argument, not of the theme, and the \
             caller reads the fill live.",
        ),
        (
            "erd_view.rs",
            "canvas",
            "`border_tint_alpha` measures the canvas's luminance, and \
             `tinted_border`'s third operand is what the tint is composited \
             over. Both return a number or a colour, never a view.",
        ),
        (
            "erd_view.rs",
            "tint",
            "`tinted_border` composites and returns a `Color`; its own doc tells \
             the caller to call it *inside* the style closure, which is where \
             the operands are read.",
        ),
        (
            "erd_view.rs",
            "header",
            "`tinted_border`'s surface operand, same reason.",
        ),
        (
            "erd_view.rs",
            "c",
            "`hex` formats a colour as `#rrggbb(aa)` for the SVG export — a \
             string, and nothing on screen.",
        ),
        (
            "overlays.rs",
            "c",
            "the two `fade` closures, which dim a colour their *caller* read \
             live in the same style closure (`fade(theme::match_highlight())`). \
             The read is reactive; this only multiplies an alpha.",
        ),
        (
            "settings.rs",
            "bg_hover",
            "`toggle_focus_ring` takes a `Style` and returns one, so it runs \
             inside the style closure by construction and its operand was read \
             there.",
        ),
        (
            "markdown.rs",
            "base",
            "**The known latent instance of the rule itself**, threaded: \
             `render_markdown` reads `bubble_claude_text()` once and passes it \
             down to `inline_text`, `md_list` and `md_table`. Graded latent by \
             A1.3-L2-01 on three verified counts — the only caller \
             (`ai_panel.rs`) sits inside a `dyn_container` whose key is a \
             *tracked* `(msg, theme::ui_generation())`, the capture is a \
             descendant of it, and `bubble_claude_text` is a **UI**-axis colour, \
             which is the axis `set_ui`/`set_ui_scale` bump. Listed rather than \
             fixed so that the day any of those three stops holding, this entry \
             is what a reader finds.",
        ),
    ];

    /// A colour a **view** is given is one it cannot re-read.
    #[test]
    fn no_view_is_handed_a_colour_it_cannot_re_read() {
        let mut offenders: Vec<String> = Vec::new();
        let mut checked = 0usize;
        for (file, code) in crate::source_gate::crate_sources() {
            // `themes.rs` is where the palettes are *defined* — structs of bare
            // `Color`s by definition, and the one thing every accessor reads.
            if file == "themes.rs" {
                continue;
            }
            for (n, line) in code.lines().enumerate() {
                let t = line.trim();
                if t.starts_with("//") || t.starts_with("///") {
                    continue;
                }
                // Every `<name>: Color` in the line — a parameter or a field —
                // however the type is spelled: bare, `peniko::Color`, or
                // `floem::peniko::Color`. `fn() -> Color`, the prescribed
                // spelling, has no `:` before the type and so never matches; nor
                // does a `-> Color` return, which is fine (the rule is about
                // what a view is *given*).
                let b = t.as_bytes();
                for at in 0..b.len() {
                    // An annotation colon: a single `:`, not the `::` of a path.
                    if b[at] != b':'
                        || b.get(at + 1) == Some(&b':')
                        || (at > 0 && b[at - 1] == b':')
                    {
                        continue;
                    }
                    // The type it annotates, whatever path it is spelled with.
                    let ty: String = t[at + 1..]
                        .trim_start()
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == ':')
                        .collect();
                    if ty.rsplit("::").next() != Some("Color") {
                        continue;
                    }
                    let before = &t[..at];
                    let name: String = before
                        .chars()
                        .rev()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect();
                    if name.is_empty() {
                        continue;
                    }
                    // `const WHITE: Color` / `static` / `let` are values, not
                    // things handed to a view.
                    let head = before[..before.len() - name.len()].trim_end();
                    if head.ends_with("const") || head.ends_with("static") || head.ends_with("let")
                    {
                        continue;
                    }
                    checked += 1;
                    if EXEMPT.iter().any(|(f, p, _)| *f == file && *p == name) {
                        continue;
                    }
                    offenders.push(format!(
                        "{file}:{}: `{name}: Color` is resolved where the caller \
                     built it, so it freezes at the theme that was active then — \
                     and a live theme switch repaints everything around it. Take \
                     it as `fn() -> Color` and call it inside the style closure, \
                     the way `FieldCfg::background` does. If it genuinely \
                     measures or composites rather than paints, add it to EXEMPT \
                     with the reason.",
                        n + 1
                    ));
                }
            }
        }
        assert!(
            checked >= EXEMPT.len(),
            "found only {checked} bare `Color` positions — fewer than the \
             exemptions claim, so this gate is no longer scanning what it thinks"
        );
        assert!(offenders.is_empty(), "\n{}", offenders.join("\n"));
    }

    /// An exemption that no longer names a real parameter is a stale licence the
    /// next `Color` at that spelling would inherit — `scaled_arg_gate`'s floor,
    /// for the same reason.
    #[test]
    fn every_exemption_still_names_a_real_parameter() {
        let sources = crate::source_gate::crate_sources();
        for (file, param, why) in EXEMPT {
            let code = sources
                .iter()
                .find(|(f, _)| f == file)
                .map(|(_, c)| c.as_str())
                .unwrap_or_else(|| panic!("EXEMPT names {file}, which is not in this crate"));
            // Spelled however the site spells the type — bare, `peniko::Color`
            // or the full path — which is what the scan above allows too.
            let still_there = code.split(&format!("{param}: ")).skip(1).any(|rest| {
                rest.chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == ':')
                    .collect::<String>()
                    .ends_with("Color")
            });
            assert!(
                still_there,
                "EXEMPT licenses `{param}: Color` in {file} ({why}), but nothing \
                 there takes it any more — drop the entry"
            );
        }
    }
}
