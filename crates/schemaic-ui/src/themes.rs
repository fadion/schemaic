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
    pub row_active: Color,
    pub row_selected: Color,
    /// Manage Connections list: resting row text.
    pub conn_list_text: Color,
    /// Manage Connections list: hovered/selected row text.
    pub conn_list_sel_text: Color,
    /// Manage Connections list: selected row background (full-width).
    pub conn_list_sel_bg: Color,
    /// Manage Connections: Delete button (icon+text), resting + hover.
    pub conn_delete: Color,
    pub conn_delete_hover: Color,
    /// Manage Connections: Save button text, resting + hover.
    pub conn_save: Color,
    pub conn_save_hover: Color,
    /// Manage Connections: Test button text, resting + hover.
    pub conn_test: Color,
    pub conn_test_hover: Color,
    /// Manage Connections: Test result icon — success (failure reuses `conn_delete`).
    pub conn_test_ok: Color,
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
            row_active: c("#222432"),
            row_selected: c("#2B314D"),
            conn_list_text: c("#C6C8D6"),
            conn_list_sel_text: c("#FFFFFF"),
            conn_list_sel_bg: c("#222432"),
            conn_delete: c("#9D3434"),
            conn_delete_hover: c("#D46A6A"),
            conn_save: c("#7694E3"),
            conn_save_hover: c("#C1D1FB"),
            conn_test: c("#C6C8D6"),
            conn_test_hover: c("#FFFFFF"),
            conn_test_ok: c("#71C371"),
            capsule_bg: c("#24283B"),
            db_toggle_on: c("#7694E3"),
            db_toggle_off: c("#474D73"),
            db_icon: c("#3D2F8C"),
            table_icon: c("#1E6E4C"),
            view_icon: c("#2FCAA6"),
            key_primary: c("#F9C24A"),
            key_index: c("#8394FF"),
            key_foreign: c("#B677EE"),
            grid_col_sel: c("#292D3E"),
            grid_edit_staged: c("#509950"),
            grid_edit_staged_hover: c("#93FF93"),
            grid_edit_discard: c("#9D3434"),
            grid_edit_discard_hover: c("#F26C6C"),
            search_hint: c("#323543"),
            error: c("#E06C75"),
            plan_warn: c("#E5C07B"),
            plan_warn_bg: c("#1B1818"),
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
            row_active: c("#DEE3F5"),
            row_selected: c("#C9D4F7"),
            conn_list_text: c("#4A4E5E"),
            conn_list_sel_text: c("#1B1E2B"),
            conn_list_sel_bg: c("#DCE0EE"),
            conn_delete: c("#B33A3A"),
            conn_delete_hover: c("#D46A6A"),
            conn_save: c("#4B6CC9"),
            conn_save_hover: c("#7694E3"),
            conn_test: c("#4A4E5E"),
            conn_test_hover: c("#1B1E2B"),
            conn_test_ok: c("#3E9E5E"),
            capsule_bg: c("#DCE0EE"),
            db_toggle_on: c("#4763C9"),
            db_toggle_off: c("#AAB0CC"),
            db_icon: c("#6D5CD6"),
            table_icon: c("#2E9E6B"),
            view_icon: c("#1FA98A"),
            key_primary: c("#D99400"),
            key_index: c("#5A6EE0"),
            key_foreign: c("#9450D6"),
            grid_col_sel: c("#DCE3F2"),
            grid_edit_staged: c("#509950"),
            grid_edit_staged_hover: c("#93FF93"),
            grid_edit_discard: c("#9D3434"),
            grid_edit_discard_hover: c("#F26C6C"),
            search_hint: c("#B9BDCA"),
            error: c("#C4444A"),
            plan_warn: c("#B7791F"),
            plan_warn_bg: c("#F5E7E7"),
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
    fn build(self) -> UiTheme {
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
    pub const ALL: [EditorThemeKind; 3] = [
        EditorThemeKind::OneDarkPro,
        EditorThemeKind::TokyoNight,
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
    pub fn from_key(s: &str) -> EditorThemeKind {
        match s {
            "tokyo-night" => EditorThemeKind::TokyoNight,
            "catppuccin-latte" => EditorThemeKind::CatppuccinLatte,
            _ => EditorThemeKind::OneDarkPro,
        }
    }
    fn build(self) -> EditorTheme {
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
                editor: scope.create_rw_signal(Rc::new(EditorTheme::one_dark_pro())),
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
    with_state(|st| st.ui.set(Rc::new(kind.build())));
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

/// Monotonic editor-theme generation (SQL editor `Styling::id`). Bumped by theme
/// AND font/tab-width changes so the editor invalidates its cached layout.
pub fn editor_generation() -> u64 {
    with_state(|st| st.editor_gen.get())
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
