//! Theme data + the runtime registry that swaps themes live.
//!
//! Two *independent* theme axes, so each can evolve on its own (and be imported
//! from disk later — see the "external themes" note below):
//!
//!   • [`UiTheme`]  — every chrome colour (surfaces, text, borders, accents,
//!     icons). Ships as [`UiThemeKind::Dark`] / [`UiThemeKind::Light`].
//!   • [`EditorTheme`] — the SQL editor surface + syntax token palette. Ships as
//!     One Dark Pro, Tokyo Night, and Catppuccin Latte.
//!
//! ## How switching works (live, no restart)
//!
//! The active themes live in two `RwSignal`s owned by a detached [`Scope`] (so
//! they persist for the whole process — they're never a child of a view/effect
//! scope that could dispose them). The `theme::*()` colour functions read those
//! signals via `.get()`, so every reactive `.style(…)` closure that calls one
//! subscribes and re-runs the instant the signal changes. The SQL editor is
//! additionally rebuilt (keyed on the editor-theme generation) so its base
//! foreground / gutter / token colours re-apply from scratch.
//!
//! ## Theme format decision (Zed-ish, data-first)
//!
//! A theme is a *flat struct of named colour roles* — the same shape Zed uses in
//! its theme JSON (`{"background": "#…", "text": "#…", …}`), minus Zed's editor
//! specifics we don't have. Colours are written as hex so a theme reads as pure
//! data; adding one is "fill in the roles." Keeping the roles flat and named (vs.
//! a big semantic tree) is what will make a `themes/*.json` loader trivial to add
//! later: deserialize into the same struct, register under its `name`. We keep
//! the built-ins in Rust for now (the picker is a fixed dropdown), but nothing
//! about the shape assumes that.

use std::rc::Rc;

use floem::peniko::Color;
use floem::reactive::{RwSignal, Scope, SignalGet, SignalUpdate};

/// Parse `#rgb` / `#rrggbb` / `#rrggbbaa` into a colour. Panics on a malformed
/// literal — the built-ins below are compile-time-fixed, so a bad string is a
/// dev bug we want to hear about immediately at startup.
fn c(s: &str) -> Color {
    parse_hex(s).unwrap_or_else(|| panic!("themes: invalid hex colour {s:?}"))
}

/// Fallible hex parser (kept public-ish in spirit for a future JSON loader).
pub fn parse_hex(s: &str) -> Option<Color> {
    let h = s.strip_prefix('#').unwrap_or(s);
    // `h.len()` is a **byte** count while the arms below index by byte on the
    // assumption of one byte per hex digit. One non-ASCII character makes those
    // disagree — `#aé` is 3 bytes, takes the `#rgb` arm, and slices through the
    // middle of `é`. `&str` indexing *panics* rather than returning `None`, so
    // the `Option` return type is not the guard it looks like, and the input is
    // persisted (a connection colour), so the crash repeats on every launch
    // during layout of the whole window. Reject non-ASCII up front; hex digits
    // are ASCII by definition, so nothing valid is lost.
    if !h.is_ascii() {
        return None;
    }
    let n = |i: usize, len: usize| u8::from_str_radix(&h[i..i + len], 16).ok();
    match h.len() {
        3 => {
            let r = u8::from_str_radix(&h[0..1], 16).ok()?;
            let g = u8::from_str_radix(&h[1..2], 16).ok()?;
            let b = u8::from_str_radix(&h[2..3], 16).ok()?;
            Some(Color::rgb8(r * 17, g * 17, b * 17))
        }
        6 => Some(Color::rgb8(n(0, 2)?, n(2, 2)?, n(4, 2)?)),
        8 => Some(Color::rgba8(n(0, 2)?, n(2, 2)?, n(4, 2)?, n(6, 2)?)),
        _ => None,
    }
}

// ── UI theme ─────────────────────────────────────────────────────────────────

/// Every chrome colour role. Field names mirror the `theme::*()` accessors 1:1.
#[derive(Clone)]
pub struct UiTheme {
    pub bg_deepest: Color,
    pub bg_chrome: Color,
    pub bg_panel: Color,
    pub bg_editor: Color,
    pub completion_border: Color,
    pub completion_active: Color,
    /// Outline of elevated modal panels (Find, error modal).
    pub modal_border: Color,
    pub ai_send_icon: Color,
    pub ai_send_icon_hover: Color,
    pub bubble_user_bg: Color,
    pub bubble_claude_bg: Color,
    pub bubble_claude_text: Color,
    pub bg_results: Color,
    pub bg_header_row: Color,
    pub border: Color,
    pub resize_handle: Color,
    pub field_border: Color,
    pub field_border_active: Color,
    pub query_highlight: Color,
    /// Box around the paren matching the one under the caret (bracket matching).
    pub bracket_match: Color,
    /// Matched-substring highlight in the command palette / Find results.
    pub match_highlight: Color,
    pub text: Color,
    pub text_dim: Color,
    pub text_muted: Color,
    pub placeholder: Color,
    pub chip_idle: Color,
    pub chip_active: Color,
    /// Active tab background; flat full-height tabs otherwise share the chrome bg.
    pub tab_active: Color,
    /// Vertical line between tabs (and the full-width strip separators).
    pub tab_separator: Color,
    /// Inactive tab label/×; brightens to `text` on hover and when active.
    pub tab_text: Color,
    /// The tab close (×) glyph — a fixed, muted tint (doesn't follow the label).
    pub tab_close: Color,
    pub accent: Color,
    pub cmdk_placeholder: Color,
    pub cmdk_text: Color,
    pub diff_add_bg: Color,
    pub diff_del_bg: Color,
    pub diff_add_marker: Color,
    pub diff_del_marker: Color,
    pub err_fix_btn: Color,
    pub approve_bg: Color,
    pub approve_text: Color,
    pub reject_bg: Color,
    pub reject_text: Color,
    pub text_faint: Color,
    pub row_hover: Color,
    /// A quieter row hover, for a list whose rows are *blocks* rather than
    /// lines — the query history, where one row is a heading, three lines of
    /// SQL and an outcome. `row_hover` across an area that size reads as a
    /// selection; this lifts the row just off `bg_panel` instead.
    pub row_hover_soft: Color,
    /// Query history: the band behind a recency group's header (TODAY / THIS
    /// WEEK / EARLIER). Deliberately **not** `row_hover` — a header that shares
    /// the hover colour looks like a hovered row — and deliberately a *shade of
    /// `bg_panel`* rather than a colour of its own: it is furniture dividing a
    /// list, so it keeps the panel's hue and only changes its level.
    pub group_header_bg: Color,
    pub row_active: Color,
    pub row_selected: Color,
    /// Pill tabs (the table designer's Table / Columns / …): the active pill's
    /// fill and its label, and the hover fill of an inactive one. Their own roles
    /// rather than the tree's `row_selected`/`row_hover`: a pill is a filled chip
    /// carrying its own text colour, and retuning it shouldn't repaint the schema
    /// tree's selection.
    pub pill_active_bg: Color,
    pub pill_active_text: Color,
    pub pill_hover_bg: Color,
    /// A modal footer's actions: a fill, its hover, and a label **tinted to
    /// match**, one triple per variant. The label follows the fill rather than
    /// staying one grey across all four, which is what buys the contrast back —
    /// a blue label on a blue fill reads far better than a neutral one does.
    pub btn_neutral: Color,
    pub btn_neutral_hover: Color,
    pub btn_neutral_text: Color,
    pub btn_primary: Color,
    pub btn_primary_hover: Color,
    pub btn_primary_text: Color,
    /// Recessed, for a side action that isn't part of the decision the footer is
    /// asking for (Copy, Open in editor) — darker than the panel rather than
    /// lighter, so it sits *under* the two that answer the question.
    pub btn_quiet: Color,
    pub btn_quiet_hover: Color,
    pub btn_quiet_text: Color,
    /// The affirmative action when the plan destroys something. A fill rather
    /// than the red *text* the confirm dialog uses, since these buttons carry
    /// their meaning in the fill.
    pub btn_danger: Color,
    pub btn_danger_hover: Color,
    pub btn_danger_text: Color,
    /// Manage Connections: the icon Test flashes in place of its label — pass and
    /// fail. Roles of their own rather than the app's `status_ok`/`error`: they
    /// are read on a `btn_neutral` fill, not on a panel, and they are the one
    /// place a colour still carries meaning now that the button around them is
    /// the same grey as Cancel.
    pub conn_test_ok: Color,
    pub conn_test_fail: Color,
    /// Manage Connections list: resting row text.
    pub conn_list_text: Color,
    /// Manage Connections list: hovered/selected row text.
    pub conn_list_sel_text: Color,
    /// Manage Connections list: selected row background (full-width).
    pub conn_list_sel_bg: Color,
    pub capsule_bg: Color,
    /// Schema db-visibility menu: a shown (enabled) row's text.
    pub db_toggle_on: Color,
    /// Schema db-visibility menu: a hidden (disabled) row's text.
    pub db_toggle_off: Color,
    pub db_icon: Color,
    pub table_icon: Color,
    pub view_icon: Color,
    pub key_primary: Color,
    pub key_index: Color,
    pub key_foreign: Color,
    /// Gold star marking a favorited database in the schema tree.
    pub favorite_star: Color,
    /// ER-diagram modal: dotted canvas background.
    pub erd_canvas: Color,
    /// ER-diagram modal: canvas dot-grid dot.
    pub erd_dot: Color,
    /// ER-diagram modal: table-node card background.
    pub erd_node_bg: Color,
    /// ER-diagram modal: table-node header (title) background.
    pub erd_node_header: Color,
    /// ER-diagram modal: column-row background when the row is an endpoint of the
    /// hovered edge (a touch lighter than the card background).
    pub erd_row_highlight: Color,
    /// ER-diagram modal: relationship (FK) edge line + cardinality markers.
    pub erd_edge: Color,
    /// ER-diagram modal: edge + markers when the relationship is hovered.
    pub erd_edge_hover: Color,
    /// ER-diagram modal: toolbar strip top/bottom border.
    pub erd_toolbar_border: Color,
    /// ER-diagram modal: toolbar control (button / count chip / zoom unit) border
    /// and the zoom unit's internal separators.
    pub erd_control_border: Color,
    /// Results grid: background of a column header whose column is selected.
    pub grid_col_sel: Color,
    /// Results grid: background of a cell with a staged (uncommitted) edit.
    pub grid_edit_staged: Color,
    /// Results grid: hover brighten for the commit (✓ + count) control.
    pub grid_edit_staged_hover: Color,
    /// Results grid: the discard-edits (✗) control.
    pub grid_edit_discard: Color,
    /// Results grid: hover brighten for the discard (✗) control.
    pub grid_edit_discard_hover: Color,
    pub search_hint: Color,
    pub error: Color,
    /// Query-plan modal: amber used for the heuristic warning rows/icons.
    pub plan_warn: Color,
    /// Query-plan modal: background tint behind the warnings panel + flagged rows.
    pub plan_warn_bg: Color,
    // ── Status bar + the semantic action accents ────────────────────────────
    // These were fixed literals in `theme.rs`, chosen against the dark footer
    // and described as "theme-independent by design". `UiTheme::light` then
    // moved `bg_deepest` from #14151A to #DCDFE6 and none of them moved with
    // it, so thirteen of fourteen fell under 3:1 — the open-transaction pill at
    // 1.48:1, on the only always-visible sign that a tab holds uncommitted
    // work. They are theme fields like everything else now, and
    // `crate::contrast` is what keeps them legible.
    /// Muted grey for status-bar text + icons.
    pub status_text: Color,
    /// Amber for the syntax-warning icon + count.
    pub status_warn: Color,
    /// Brighter amber for hovering the write-mode status segment.
    pub status_warn_hover: Color,
    /// Green for the "no warnings" check.
    pub status_ok: Color,
    /// Green CTA *fill* for the AI "Seed rows" Generate button (white text).
    pub seed_button: Color,
    /// The table designer's "N changes" count, when there are changes.
    pub change_count: Color,
    /// A tab in manual-commit mode, and its open-transaction pill.
    pub tx_open: Color,
    /// Hover for the clickable manual-mode / Commit / Rollback segments.
    pub tx_open_hover: Color,
    /// A transaction that can't go forward (PG aborted it, or the pinned
    /// connection died).
    pub tx_danger: Color,
    /// The status bar's Commit action, resting + hover.
    pub tx_commit: Color,
    pub tx_commit_hover: Color,
    /// The status bar's Rollback action, resting + hover.
    pub tx_rollback: Color,
    pub tx_rollback_hover: Color,
    /// The affirmative (destructive) button in a confirm modal, resting + hover.
    pub confirm_yes: Color,
    pub confirm_yes_hover: Color,
    pub conn_ok: Color,
    pub dropdown_hover: Color,
    pub dropdown_active: Color,
    pub code_action_bar: Color,
    pub jump_icon: Color,
    pub jump_icon_hover: Color,
    pub toggle_on: Color,
    pub toggle_on_hover: Color,
    pub toggle_off: Color,
    pub toggle_off_hover: Color,
    pub toggle_handle_on: Color,
    pub toggle_handle_off: Color,
    pub scrollbar: Color,
    pub scrollbar_hover: Color,
}

impl UiTheme {
    /// The original Zed-inspired dark palette (unchanged M0 values).
    pub fn dark() -> Self {
        Self {
            bg_deepest: c("#14151A"),
            bg_chrome: c("#18191F"),
            bg_panel: c("#1B1C23"),
            bg_editor: c("#1E1F26"),
            completion_border: c("#373942"),
            completion_active: c("#31384C"),
            modal_border: c("#2D2F39"),
            ai_send_icon: c("#2D2F39"),
            ai_send_icon_hover: c("#545D9E"),
            bubble_user_bg: c("#1F2028"),
            bubble_claude_bg: c("#22232D"),
            bubble_claude_text: c("#A2A4B0"),
            bg_results: c("#191A20"),
            bg_header_row: c("#21222A"),
            border: c("#2E303A"),
            resize_handle: c("#4C516B"),
            field_border: c("#24252D"),
            field_border_active: c("#303453"),
            query_highlight: c("#FF7373"),
            bracket_match: c("#7C8CA8"),
            match_highlight: c("#7C9CF0"),
            text: c("#C6C8D6"),
            text_dim: c("#7E8294"),
            text_muted: c("#585C6A"),
            placeholder: c("#323543"),
            chip_idle: c("#6E7181"),
            chip_active: c("#7694E3"),
            tab_active: c("#232532"),
            tab_separator: c("#2D2F39"),
            tab_text: c("#707485"),
            tab_close: c("#323543"),
            accent: c("#7C9CF0"),
            cmdk_placeholder: c("#353A43"),
            cmdk_text: c("#AAB1BE"),
            diff_add_bg: c("#1E3A24"),
            diff_del_bg: c("#462020"),
            diff_add_marker: c("#71C371"),
            diff_del_marker: c("#CF7B7B"),
            err_fix_btn: c("#EDC6C6"),
            approve_bg: c("#71C371"),
            approve_text: c("#173717"),
            reject_bg: c("#9D3434"),
            reject_text: c("#3F0D0D"),
            text_faint: c("#50556C"),
            row_hover: c("#171820"),
            // Just above `bg_panel` (#1B1C23), where `row_hover` sits just below
            // it — the same small step, lifting rather than recessing, so the two
            // can't be mistaken for each other.
            row_hover_soft: c("#1E1F28"),
            // A shade of `bg_panel` (#1B1C23) rather than a colour of its own:
            // one step down, keeping most of the panel's blue lean (R→B spread 6
            // against its 8) so the band reads as the same surface, darker.
            group_header_bg: c("#1A1B20"),
            row_active: c("#222432"),
            row_selected: c("#2B314D"),
            pill_active_bg: c("#7C9CF0"),
            pill_active_text: c("#14151A"),
            pill_hover_bg: c("#2D2F39"),
            btn_neutral: c("#393C4C"),
            btn_neutral_hover: c("#454A5E"),
            btn_neutral_text: c("#A5AAC9"),
            btn_primary: c("#283863"),
            btn_primary_hover: c("#31457B"),
            btn_primary_text: c("#8EA7EA"),
            btn_quiet: c("#14151A"),
            btn_quiet_hover: c("#0C0D11"),
            btn_quiet_text: c("#777A8C"),
            btn_danger: c("#862C2C"),
            btn_danger_hover: c("#9D3A3A"),
            btn_danger_text: c("#E28F8F"),
            conn_test_ok: c("#71C371"),
            conn_test_fail: c("#E28F8F"),
            conn_list_text: c("#C6C8D6"),
            conn_list_sel_text: c("#FFFFFF"),
            conn_list_sel_bg: c("#222432"),
            capsule_bg: c("#24283B"),
            db_toggle_on: c("#7694E3"),
            db_toggle_off: c("#474D73"),
            db_icon: c("#3D2F8C"),
            table_icon: c("#1E6E4C"),
            view_icon: c("#2FCAA6"),
            key_primary: c("#F9C24A"),
            key_index: c("#8394FF"),
            key_foreign: c("#B677EE"),
            favorite_star: c("#F9C24A"),
            erd_canvas: c("#151620"),
            erd_dot: c("#232532"),
            erd_node_bg: c("#22232E"),
            erd_node_header: c("#2B2D3A"),
            erd_row_highlight: c("#313348"),
            erd_edge: c("#3F4152"),
            erd_edge_hover: c("#464E9E"),
            erd_toolbar_border: c("#434553"),
            erd_control_border: c("#424553"),
            grid_col_sel: c("#292D3E"),
            grid_edit_staged: c("#509950"),
            grid_edit_staged_hover: c("#93FF93"),
            grid_edit_discard: c("#9D3434"),
            grid_edit_discard_hover: c("#F26C6C"),
            search_hint: c("#323543"),
            error: c("#E06C75"),
            plan_warn: c("#E5C07B"),
            plan_warn_bg: c("#1B1818"),
            status_text: c("#6E7181"),
            status_warn: c("#E08A4B"),
            status_warn_hover: c("#FFA461"),
            status_ok: c("#71C371"),
            // A fill, not text: white on the old #71C371 was 2.15:1 in both
            // palettes, the one accent here that needed to go *darker* on dark.
            seed_button: c("#2E7D32"),
            change_count: c("#71C371"),
            tx_open: c("#E0B24B"),
            tx_open_hover: c("#FFD070"),
            tx_danger: c("#E05A5A"),
            tx_commit: c("#71C371"),
            tx_commit_hover: c("#8FDC8F"),
            tx_rollback: c("#E05A5A"),
            tx_rollback_hover: c("#FF7B7B"),
            confirm_yes: c("#E05A5A"),
            confirm_yes_hover: c("#FF7B7B"),
            conn_ok: c("#509950"),
            dropdown_hover: c("#272D3E"),
            dropdown_active: c("#1C1F28"),
            code_action_bar: c("#22232D"),
            jump_icon: c("#323543"),
            jump_icon_hover: c("#535C89"),
            toggle_on: c("#5A86FA"),
            toggle_on_hover: c("#6D95FF"),
            toggle_off: c("#2E303B"),
            toggle_off_hover: c("#3A3C4A"),
            toggle_handle_on: c("#FFFFFF"),
            toggle_handle_off: c("#525765"),
            scrollbar: c("#232431"),
            scrollbar_hover: c("#2F3243"),
        }
    }

    /// A clean, professional light palette — soft neutral surfaces, near-black
    /// text, the app's blue deepened for contrast on light.
    pub fn light() -> Self {
        Self {
            bg_deepest: c("#DCDFE6"),
            bg_chrome: c("#E9EBF0"),
            bg_panel: c("#F1F2F6"),
            bg_editor: c("#F6F7F9"),
            completion_border: c("#D4D7E0"),
            completion_active: c("#DCE4FB"),
            modal_border: c("#D4D7E0"),
            ai_send_icon: c("#C7CBD6"),
            ai_send_icon_hover: c("#6E86D8"),
            bubble_user_bg: c("#E7E9F0"),
            bubble_claude_bg: c("#EFF0F4"),
            bubble_claude_text: c("#3A3E4A"),
            bg_results: c("#FCFCFD"),
            bg_header_row: c("#E6E8EF"),
            border: c("#D4D7E0"),
            resize_handle: c("#B7BEDC"),
            field_border: c("#D4D7E0"),
            field_border_active: c("#9DB0EC"),
            query_highlight: c("#E5484D"),
            bracket_match: c("#8894A8"),
            match_highlight: c("#3355C4"),
            text: c("#2B2E3A"),
            text_dim: c("#5C6270"),
            text_muted: c("#8A8F9E"),
            placeholder: c("#B9BDCA"),
            chip_idle: c("#9096A6"),
            chip_active: c("#3D66D6"),
            tab_active: c("#FFFFFF"),
            tab_separator: c("#D4D7E0"),
            tab_text: c("#8A8F9E"),
            tab_close: c("#C2C6D0"),
            accent: c("#3D66D6"),
            cmdk_placeholder: c("#B9BDCA"),
            cmdk_text: c("#2B2E3A"),
            diff_add_bg: c("#E2F3E6"),
            diff_del_bg: c("#FBE5E5"),
            diff_add_marker: c("#2E8C46"),
            diff_del_marker: c("#C4444A"),
            err_fix_btn: c("#F7DADA"),
            approve_bg: c("#3AA655"),
            approve_text: c("#08240F"),
            reject_bg: c("#D64545"),
            reject_text: c("#FFF2F2"),
            text_faint: c("#A6AAB8"),
            row_hover: c("#ECEEF3"),
            // Lighter than `row_hover`, i.e. nearer `bg_panel` (#F1F2F6): on a
            // light surface the quieter hover is the one closer to white.
            row_hover_soft: c("#EFF0F5"),
            // Same idea on the light surface: a step *down* from `bg_panel`
            // (#F1F2F6), since here darker is what reads as a band.
            group_header_bg: c("#EEEFF2"),
            row_active: c("#DEE3F5"),
            row_selected: c("#C9D4F7"),
            // The active pill keeps the dark theme's fill: it's a filled accent
            // chip that carries its own contrast, so it reads the same either way.
            // Only the hover fill has to follow the surface it sits on.
            pill_active_bg: c("#7C9CF0"),
            pill_active_text: c("#14151A"),
            pill_hover_bg: c("#E4E8F2"),
            // Inverted, not recoloured: the light theme puts the tint in the fill
            // and the depth in the label, so each variant reads the same way round
            // (neutral grey, affirmative blue, destructive red) either way.
            btn_neutral: c("#E2E5EC"),
            btn_neutral_hover: c("#D3D7E1"),
            btn_neutral_text: c("#454A5E"),
            btn_primary: c("#DCE3F5"),
            btn_primary_hover: c("#C9D4F7"),
            btn_primary_text: c("#2F4A94"),
            btn_quiet: c("#E7E9EF"),
            btn_quiet_hover: c("#DADDE6"),
            btn_quiet_text: c("#777A8C"),
            btn_danger: c("#F6DCDE"),
            btn_danger_hover: c("#EFC9CD"),
            btn_danger_text: c("#8C2A2A"),
            // Darker than the dark theme's, not lighter: these sit on a pale
            // fill, and the mid-green that reads on #393C4C manages 2.7:1 on
            // #E2E5EC — under the 3:1 an icon owes.
            conn_test_ok: c("#2F7D49"),
            conn_test_fail: c("#8C2A2A"),
            conn_list_text: c("#4A4E5E"),
            conn_list_sel_text: c("#1B1E2B"),
            conn_list_sel_bg: c("#DCE0EE"),
            capsule_bg: c("#DCE0EE"),
            db_toggle_on: c("#4763C9"),
            db_toggle_off: c("#AAB0CC"),
            db_icon: c("#6D5CD6"),
            table_icon: c("#2E9E6B"),
            view_icon: c("#1FA98A"),
            key_primary: c("#D99400"),
            key_index: c("#5A6EE0"),
            key_foreign: c("#9450D6"),
            favorite_star: c("#C68A1A"),
            erd_canvas: c("#EDEFF3"),
            erd_dot: c("#CDD2DC"),
            erd_node_bg: c("#FFFFFF"),
            erd_node_header: c("#EEF0F5"),
            // White can't go lighter, so a faint indigo wash tied to the edge accent.
            erd_row_highlight: c("#E6EAFB"),
            erd_edge: c("#9298AC"),
            erd_edge_hover: c("#464E9E"),
            erd_toolbar_border: c("#CDD2DC"),
            erd_control_border: c("#C4CAD6"),
            grid_col_sel: c("#DCE3F2"),
            grid_edit_staged: c("#509950"),
            grid_edit_staged_hover: c("#93FF93"),
            grid_edit_discard: c("#9D3434"),
            grid_edit_discard_hover: c("#F26C6C"),
            search_hint: c("#B9BDCA"),
            error: c("#C4444A"),
            plan_warn: c("#B7791F"),
            plan_warn_bg: c("#F5E7E7"),
            // The dark palette's accents inverted for a #DCDFE6 footer: the
            // greens, ambers and reds all go *darker* (a hover that brightens on
            // dark has to deepen on light to read as more prominent), and
            // `seed_button` — the one that is a fill under white text — goes
            // darker in both palettes instead.
            status_text: c("#6E7181"),
            status_warn: c("#8A5200"),
            status_warn_hover: c("#6F4200"),
            status_ok: c("#1B6B2E"),
            seed_button: c("#2E7D32"),
            change_count: c("#1B6B2E"),
            tx_open: c("#7D5A00"),
            tx_open_hover: c("#614600"),
            tx_danger: c("#A8322F"),
            tx_commit: c("#1B6B2E"),
            tx_commit_hover: c("#145423"),
            tx_rollback: c("#A8322F"),
            tx_rollback_hover: c("#8A2422"),
            confirm_yes: c("#A8322F"),
            confirm_yes_hover: c("#8A2422"),
            conn_ok: c("#2E8C46"),
            dropdown_hover: c("#E4E8F5"),
            dropdown_active: c("#EDEFF4"),
            code_action_bar: c("#E2E4EC"),
            jump_icon: c("#B7BEDC"),
            jump_icon_hover: c("#6E86D8"),
            toggle_on: c("#4C7EF3"),
            toggle_on_hover: c("#3D6BE0"),
            toggle_off: c("#C9CDD8"),
            toggle_off_hover: c("#BBC0CE"),
            toggle_handle_on: c("#FFFFFF"),
            toggle_handle_off: c("#FFFFFF"),
            scrollbar: c("#CDD1DC"),
            scrollbar_hover: c("#B4B9C8"),
        }
    }
}

// ── Editor theme ───────────────────────────────────────────────────────────

/// SQL editor surface + syntax token palette. `type_`/`constant` back the
/// autocomplete's table/database tints (they mirror token colours).
#[derive(Clone)]
pub struct EditorTheme {
    pub bg: Color,
    pub fg: Color,
    pub gutter_fg: Color,
    pub cursor: Color,
    pub selection: Color,
    pub current_line: Color,
    pub keyword: Color,
    pub string: Color,
    pub number: Color,
    pub comment: Color,
    pub function: Color,
    pub type_: Color,
    pub constant: Color,
    /// Wavy underline under a misspelled keyword.
    pub underline: Color,
    /// Wavy underline under a *definite* diagnostic error (unknown table or
    /// column, a syntax error). Semantic rather than a token colour, but it is
    /// drawn on the editor surface, so it belongs to this axis: a fixed red
    /// picked against a dark editor sat on Catppuccin Latte's #EFF1F5 at 2.9:1.
    pub diag_error: Color,
}

impl EditorTheme {
    pub fn one_dark_pro() -> Self {
        Self {
            bg: c("#282C34"),
            fg: c("#ABB2BF"),
            gutter_fg: c("#5C6370"),
            cursor: c("#528BFF"),
            selection: c("#3E4451"),
            current_line: c("#2C313C"),
            keyword: c("#C678DD"),
            string: c("#98C379"),
            number: c("#D19A66"),
            comment: c("#5C6370"),
            function: c("#61AFEF"),
            type_: c("#E5C07B"),
            constant: c("#56B6C2"),
            underline: c("#7E6E11"),
            diag_error: c("#E06C75"),
        }
    }

    pub fn tokyo_night() -> Self {
        Self {
            bg: c("#1A1B26"),
            fg: c("#A9B1D6"),
            gutter_fg: c("#565F89"),
            cursor: c("#C0CAF5"),
            selection: c("#283457"),
            current_line: c("#292E42"),
            keyword: c("#BB9AF7"),
            string: c("#9ECE6A"),
            number: c("#FF9E64"),
            comment: c("#565F89"),
            function: c("#7AA2F7"),
            type_: c("#E0AF68"),
            constant: c("#2AC3DE"),
            underline: c("#8A6D3B"),
            diag_error: c("#F7768E"),
        }
    }

    pub fn catppuccin_latte() -> Self {
        Self {
            bg: c("#EFF1F5"),
            fg: c("#4C4F69"),
            gutter_fg: c("#8C8FA1"),
            cursor: c("#DC8A78"),
            selection: c("#BCC0CC"),
            current_line: c("#E6E9EF"),
            keyword: c("#8839EF"),
            string: c("#40A02B"),
            number: c("#FE640B"),
            comment: c("#9CA0B0"),
            function: c("#1E66F5"),
            type_: c("#DF8E1D"),
            constant: c("#04A5E5"),
            underline: c("#DF8E1D"),
            diag_error: c("#D20F39"),
        }
    }
}

// ── Kinds (the picker's fixed built-in list) ─────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum UiThemeKind {
    Dark,
    Light,
}
impl UiThemeKind {
    pub const ALL: [UiThemeKind; 2] = [UiThemeKind::Dark, UiThemeKind::Light];
    pub fn label(self) -> &'static str {
        match self {
            UiThemeKind::Dark => "Dark",
            UiThemeKind::Light => "Light",
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            UiThemeKind::Dark => "dark",
            UiThemeKind::Light => "light",
        }
    }
    pub fn from_key(s: &str) -> UiThemeKind {
        match s {
            "light" => UiThemeKind::Light,
            _ => UiThemeKind::Dark,
        }
    }
    pub(crate) fn build(self) -> UiTheme {
        match self {
            UiThemeKind::Dark => UiTheme::dark(),
            UiThemeKind::Light => UiTheme::light(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EditorThemeKind {
    OneDarkPro,
    TokyoNight,
    CatppuccinLatte,
}
impl EditorThemeKind {
    /// Dropdown order. The default comes first — see
    /// `persist::default_editor_theme`, which this must agree with.
    pub const ALL: [EditorThemeKind; 3] = [
        EditorThemeKind::TokyoNight,
        EditorThemeKind::OneDarkPro,
        EditorThemeKind::CatppuccinLatte,
    ];
    pub fn label(self) -> &'static str {
        match self {
            EditorThemeKind::OneDarkPro => "One Dark Pro",
            EditorThemeKind::TokyoNight => "Tokyo Night",
            EditorThemeKind::CatppuccinLatte => "Catppuccin Latte",
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            EditorThemeKind::OneDarkPro => "one-dark-pro",
            EditorThemeKind::TokyoNight => "tokyo-night",
            EditorThemeKind::CatppuccinLatte => "catppuccin-latte",
        }
    }
    /// An unknown key resolves to the **default**, which is what a config
    /// written by a newer build (or hand-edited) degrades to — the same rule the
    /// persisted enums' `…Raw` shims follow. Keep this arm and
    /// `persist::default_editor_theme` in step.
    pub fn from_key(s: &str) -> EditorThemeKind {
        match s {
            "one-dark-pro" => EditorThemeKind::OneDarkPro,
            "catppuccin-latte" => EditorThemeKind::CatppuccinLatte,
            _ => EditorThemeKind::TokyoNight,
        }
    }
    pub(crate) fn build(self) -> EditorTheme {
        match self {
            EditorThemeKind::OneDarkPro => EditorTheme::one_dark_pro(),
            EditorThemeKind::TokyoNight => EditorTheme::tokyo_night(),
            EditorThemeKind::CatppuccinLatte => EditorTheme::catppuccin_latte(),
        }
    }
}

// ── Runtime registry (the live-switch machinery) ─────────────────────────────

struct ThemeState {
    // Held only so the detached scope (and thus the signals) never gets dropped.
    _scope: Scope,
    ui: RwSignal<Rc<UiTheme>>,
    // Bumped on every UI-theme change, for views that *can't* re-read a colour
    // reactively and have to be rebuilt instead. See `ui_generation`.
    ui_gen: RwSignal<u64>,
    editor: RwSignal<Rc<EditorTheme>>,
    // Bumped on every editor-theme change; the SQL editor's `Styling::id` reads
    // this so the editor invalidates its cached layout and re-highlights.
    editor_gen: RwSignal<u64>,
    // Editor content settings (global so `SqlStyling`/`editor_style` read them):
    // font size (px), tab width (columns), and whether Tab inserts spaces.
    editor_font: RwSignal<f32>,
    editor_tab_width: RwSignal<usize>,
    editor_soft_tabs: RwSignal<bool>,
    // Whether long editor lines wrap to the viewport width vs scroll horizontally.
    editor_word_wrap: RwSignal<bool>,
}

thread_local! {
    static STATE: std::cell::RefCell<Option<ThemeState>> = const { std::cell::RefCell::new(None) };
}

fn with_state<R>(f: impl FnOnce(&ThemeState) -> R) -> R {
    STATE.with(|cell| {
        if cell.borrow().is_none() {
            // Detached scope → not a child of any view/effect scope, so it (and
            // the signals under it) live for the whole process.
            let scope = Scope::new();
            let state = ThemeState {
                ui: scope.create_rw_signal(Rc::new(UiTheme::dark())),
                // The default, matching `persist::default_editor_theme` — this is
                // what paints for the frame before `theme::init` seeds the saved
                // choice.
                editor: scope.create_rw_signal(Rc::new(EditorTheme::tokyo_night())),
                ui_gen: scope.create_rw_signal(0u64),
                editor_gen: scope.create_rw_signal(0u64),
                editor_font: scope.create_rw_signal(14.0_f32),
                editor_tab_width: scope.create_rw_signal(4usize),
                editor_soft_tabs: scope.create_rw_signal(true),
                editor_word_wrap: scope.create_rw_signal(false),
                _scope: scope,
            };
            *cell.borrow_mut() = Some(state);
        }
        f(cell.borrow().as_ref().unwrap())
    })
}

/// Seed the active themes from persisted choices. Call once at startup, before
/// building the view tree.
pub fn init(ui: UiThemeKind, editor: EditorThemeKind) {
    set_ui(ui);
    set_editor(editor);
}

/// Swap the active UI theme (re-runs every reactive style closure).
pub fn set_ui(kind: UiThemeKind) {
    with_state(|st| {
        st.ui.set(Rc::new(kind.build()));
        st.ui_gen.update(|g| *g += 1);
    });
}

/// Swap the active editor theme (bumps the generation so the editor re-highlights).
pub fn set_editor(kind: EditorThemeKind) {
    with_state(|st| {
        st.editor.set(Rc::new(kind.build()));
        st.editor_gen.update(|g| *g += 1);
    });
}

/// The active UI theme. Reading a field subscribes the caller's reactive scope.
pub fn ui() -> Rc<UiTheme> {
    with_state(|st| st.ui.get())
}

/// The active editor theme.
pub fn editor() -> Rc<EditorTheme> {
    with_state(|st| st.editor.get())
}

/// Monotonic UI-theme generation, for the views a live switch can't reach.
///
/// The normal rule — pass `fn() -> Color` and call it *inside* the `.style`
/// closure — works because reading the theme signal there subscribes that
/// closure. It has one blind spot: a colour baked into a text `Attrs` list
/// (`markdown.rs`) isn't a style closure at all and can never re-evaluate, so
/// the only way to repaint it is to rebuild the view. Keying a `dyn_container`
/// on this is that rebuild, and it's the same trick `editor_generation` plays
/// for the SQL editor's cached layout.
///
/// Use it **only** where re-reading is impossible — a `dyn_container` keyed on
/// this discards and recreates its whole child scope on every theme switch.
pub fn ui_generation() -> u64 {
    with_state(|st| st.ui_gen.get())
}

/// Monotonic editor-theme generation (SQL editor `Styling::id`). Bumped by theme
/// AND font/tab-width changes so the editor invalidates its cached layout.
pub fn editor_generation() -> u64 {
    with_state(|st| st.editor_gen.get())
}

/// Bump the editor generation without changing any setting — invalidates the
/// mounted editor's cached layout so a per-tab font zoom (which doesn't go
/// through `set_editor_font`) takes effect immediately.
pub fn bump_editor_generation() {
    with_state(|st| st.editor_gen.update(|g| *g += 1));
}

/// The SQL-editor font size (px). Read by `SqlStyling::font_size`.
pub fn editor_font_size() -> f32 {
    with_state(|st| st.editor_font.get())
}

/// Set the SQL-editor font size (px); bumps the generation so it re-lays out.
pub fn set_editor_font(px: f32) {
    with_state(|st| {
        st.editor_font.set(px);
        st.editor_gen.update(|g| *g += 1);
    });
}

/// The editor tab width (columns). Read by `SqlStyling::tab_width` and the
/// `indent_style` in `editor_style`.
pub fn editor_tab_width() -> usize {
    with_state(|st| st.editor_tab_width.get())
}

/// Set the editor tab width; bumps the generation so it re-lays out.
pub fn set_editor_tab_width(w: usize) {
    with_state(|st| {
        st.editor_tab_width.set(w.clamp(1, 8));
        st.editor_gen.update(|g| *g += 1);
    });
}

/// Whether Tab inserts spaces (soft tabs) vs a literal `\t`. Read by `editor_style`.
pub fn editor_soft_tabs() -> bool {
    with_state(|st| st.editor_soft_tabs.get())
}

/// Set soft-tabs (spaces) vs hard tabs; bumps the generation so it re-lays out.
pub fn set_editor_soft_tabs(soft: bool) {
    with_state(|st| {
        st.editor_soft_tabs.set(soft);
        st.editor_gen.update(|g| *g += 1);
    });
}

/// Whether long editor lines wrap to the viewport width. Read by `editor_style`.
pub fn editor_word_wrap() -> bool {
    with_state(|st| st.editor_word_wrap.get())
}

/// Set word wrap; bumps the generation so the editor re-lays out.
pub fn set_editor_word_wrap(wrap: bool) {
    with_state(|st| {
        st.editor_word_wrap.set(wrap);
        st.editor_gen.update(|g| *g += 1);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A persisted colour reaches `parse_hex` from `connections.json` /
    /// `db_colors.json`, which the user can hand-edit and which nothing
    /// validates. A non-ASCII character used to panic *during layout of the
    /// whole window*, on every launch, with no way to recover from inside the
    /// app — so this must return `None`, never crash.
    #[test]
    fn parse_hex_rejects_non_ascii_instead_of_panicking() {
        // Byte lengths that land exactly on the 3 / 6 / 8 arms.
        assert_eq!(parse_hex("#aé"), None); // 3 bytes, splits inside 'é'
        assert_eq!(parse_hex("#éa"), None); // 3 bytes, splits at h[0..1]
        assert_eq!(parse_hex("#aaaaé"), None); // 6 bytes
        assert_eq!(parse_hex("#aaaaaaé"), None); // 8 bytes
        assert_eq!(parse_hex("#日本語"), None); // 9 bytes, no arm
    }

    #[test]
    fn parse_hex_still_reads_every_valid_form() {
        assert_eq!(parse_hex("#fff"), Some(Color::rgb8(255, 255, 255)));
        assert_eq!(parse_hex("#000"), Some(Color::rgb8(0, 0, 0)));
        assert_eq!(parse_hex("#ff8800"), Some(Color::rgb8(255, 136, 0)));
        assert_eq!(parse_hex("ff8800"), Some(Color::rgb8(255, 136, 0)));
        assert_eq!(parse_hex("#ff880080"), Some(Color::rgba8(255, 136, 0, 128)));
    }

    /// `c()`'s doc says a bad literal is "a dev bug we want to hear about
    /// immediately at startup". That isn't true of three of the five palettes:
    /// they are built only when the user *selects* them, so a five-digit hex in
    /// `UiTheme::light()` compiles, passes the suite, ships, and then panics the
    /// running app the moment someone opens Settings and picks Light.
    ///
    /// Constructing every palette here is what makes the comment true. It is
    /// also the whole test — a panic in `c()` fails it.
    #[test]
    fn every_builtin_palette_constructs() {
        for kind in UiThemeKind::ALL {
            let t = kind.build();
            // Touch a field so the construction can't be optimised away.
            assert!(
                t.text.a > 0,
                "{} has a transparent text colour",
                kind.label()
            );
        }
        for kind in EditorThemeKind::ALL {
            let t = kind.build();
            assert!(t.fg.a > 0, "{} has a transparent foreground", kind.label());
        }
    }

    /// The editor theme's default is stated in four places that must agree:
    /// `persist::default_editor_theme` (what a fresh config gets), `ALL[0]` (what
    /// the dropdown shows first), `from_key`'s fallback (what an unknown key
    /// degrades to) and the runtime seed. Three of them are silent if they drift
    /// — a mismatched `from_key` arm in particular would resolve a *newer*
    /// build's theme key to something other than the documented default.
    #[test]
    fn the_editor_theme_default_agrees_everywhere() {
        let default_key = schemaic_core::persist::UiState::default().editor_theme;
        assert_eq!(default_key, "tokyo-night");
        assert_eq!(EditorThemeKind::ALL[0].key(), default_key, "dropdown order");
        assert_eq!(
            EditorThemeKind::from_key("no-such-theme").key(),
            default_key,
            "an unknown key must resolve to the default"
        );
        assert_eq!(
            EditorThemeKind::from_key(&default_key).key(),
            default_key,
            "the default key must round-trip"
        );
    }

    #[test]
    fn every_editor_theme_key_round_trips() {
        // The keys are persisted, so a typo in one silently resets that user's
        // choice to the default on the next load.
        for kind in EditorThemeKind::ALL {
            assert_eq!(
                EditorThemeKind::from_key(kind.key()).key(),
                kind.key(),
                "{} does not round-trip",
                kind.label()
            );
        }
    }

    #[test]
    fn parse_hex_rejects_malformed_ascii() {
        assert_eq!(parse_hex("#gg"), None);
        assert_eq!(parse_hex("#zzz"), None);
        assert_eq!(parse_hex(""), None);
        assert_eq!(parse_hex("#"), None);
        assert_eq!(parse_hex("#aaaa"), None, "no 4-digit form");
    }
}
