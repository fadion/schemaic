//! WCAG contrast maths, plus the table of foreground/background pairings the UI
//! actually paints — the gate that stops a theme shipping a colour combination
//! nobody can read.
//!
//! ## Why a table rather than a census
//!
//! The obvious check ("does any view inline a hex literal instead of calling a
//! [`crate::theme`] accessor?") is a grep, and a grep cannot see this class of
//! bug at all: a *paired* accessor whose foreground and background are chosen in
//! two different views is perfectly well-behaved code that happens to be
//! illegible. Three sites rendered `theme::reject_text()` as free-standing text
//! at 1.02:1 — in **both** themes — and every previous colour pass walked past
//! them. So the unit under test is the **pairing**, not the literal.
//!
//! [`UI_PAIRINGS`] therefore names one (foreground, background) combination per
//! real site, [`audit_ui`] measures each against the floor its role earns, and
//! the tests below run that over every built-in palette. A new theme is gated by
//! construction; a new *pairing* has to be added here, which is the one-time cost
//! of the approach.
//!
//! ## What is deliberately not in the table
//!
//! Only text and icons — the "can I read this" question. Separators, scrollbar
//! handles, dot grids, focus rings and the modal scrim are graphical furniture
//! whose whole job is to stay quiet, and holding them to a text floor would
//! either fail permanently or force the floor down until it meant nothing. The
//! two other exclusions are named at their entries: `env_badge_text` sits on a
//! user-chosen connection colour (no theme can promise a ratio there), and the
//! *syntax token* colours of the editor themes are faithful reproductions of
//! upstream palettes — see [`EDITOR_PAIRINGS`].
//!
//! One real pairing is missing on purpose. The completion popup draws **editor**
//! token colours on a **chrome** surface (`bg_deepest`, and `completion_active`
//! for the selected row), and the two theme axes are independent — so a dark
//! editor theme under the Light UI theme puts One Dark Pro's `type_` on #DCDFE6
//! at **1.29:1**, and 31 of the 48 (4 roles × 2 surfaces × 6 theme combinations)
//! miss AA. That is one structural defect with one fix — give the popup a
//! surface from the same axis as its text — not 31 palette entries, so it is
//! filed in `TODO.md` rather than baselined into a wall of exemptions here.

use floem::peniko::Color;

use crate::themes::{EditorTheme, UiTheme};

// ── WCAG 2.1 relative luminance + contrast ratio ────────────────────────────

/// One sRGB channel, linearised (WCAG 2.1 §relative luminance).
fn linearize(channel: u8) -> f64 {
    let s = channel as f64 / 255.0;
    if s <= 0.04045 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

/// Relative luminance of an **opaque** colour, 0.0 (black) to 1.0 (white).
///
/// Alpha is ignored: composite first with [`over`] if the colour is translucent.
pub fn relative_luminance(c: Color) -> f64 {
    0.2126 * linearize(c.r) + 0.7152 * linearize(c.g) + 0.0722 * linearize(c.b)
}

/// WCAG contrast ratio between two opaque colours: 1.0 (identical) to 21.0
/// (black on white). Symmetric — the order of the arguments doesn't matter.
pub fn contrast_ratio(a: Color, b: Color) -> f64 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// Composite a translucent `fg` over an opaque `bg` (source-over), so a
/// `multiply_alpha`'d wash can be measured as what the eye actually sees.
pub fn over(fg: Color, bg: Color) -> Color {
    let a = fg.a as f64 / 255.0;
    let mix = |f: u8, b: u8| (f as f64 * a + b as f64 * (1.0 - a)).round() as u8;
    Color::rgba8(mix(fg.r, bg.r), mix(fg.g, bg.g), mix(fg.b, bg.b), 255)
}

// ── Roles and floors ────────────────────────────────────────────────────────

/// How legible a pairing has to be, which depends on what it is *for*.
///
/// The floors are WCAG 2.1's, with one addition the spec doesn't have a level
/// for: [`Legibility::Recessive`]. Placeholders, disabled rows and watermark
/// hints are meant to recede, and holding them to body text's 4.5:1 would just
/// mean deleting the check. Their floor asserts the weaker property that they
/// are *perceptible* — the failure it exists to catch is 1.0:1 invisibility.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Legibility {
    /// Normal-size text a user is expected to read. WCAG AA: 4.5:1.
    Body,
    /// Icons, glyphs, large or bold text — WCAG's "large text / non-text
    /// contrast" level: 3.0:1.
    Icon,
    /// Deliberately dim: placeholders, disabled entries, watermarks.
    Recessive,
}

impl Legibility {
    pub fn floor(self) -> f64 {
        match self {
            Legibility::Body => 4.5,
            Legibility::Icon => 3.0,
            Legibility::Recessive => 2.0,
        }
    }
}

/// One (foreground, background) combination the UI really paints.
pub struct Pairing<T: 'static> {
    /// The `theme::` accessor providing the foreground.
    pub fg: &'static str,
    /// The surface it is drawn on.
    pub bg: &'static str,
    pub role: Legibility,
    /// Where this pairing happens, so a failure names a screen and not just two
    /// colours.
    pub site: &'static str,
    fg_of: fn(&T) -> Color,
    bg_of: fn(&T) -> Color,
}

impl<T> Pairing<T> {
    /// The measured ratio of this pairing in `theme`.
    pub fn ratio(&self, theme: &T) -> f64 {
        contrast_ratio((self.fg_of)(theme), (self.bg_of)(theme))
    }
}

/// A pairing that came in under its floor, with the number it managed.
#[derive(Debug)]
pub struct Failure {
    pub fg: &'static str,
    pub bg: &'static str,
    pub site: &'static str,
    pub ratio: f64,
    pub floor: f64,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} on {} = {:.2}:1 (needs {:.1}:1) — {}",
            self.fg, self.bg, self.ratio, self.floor, self.site
        )
    }
}

fn audit<T>(pairings: &'static [Pairing<T>], theme: &T) -> Vec<Failure> {
    pairings
        .iter()
        .filter_map(|p| {
            let ratio = p.ratio(theme);
            (ratio < p.role.floor()).then_some(Failure {
                fg: p.fg,
                bg: p.bg,
                site: p.site,
                ratio,
                floor: p.role.floor(),
            })
        })
        .collect()
}

/// Every [`UI_PAIRINGS`] entry that misses its floor in `theme`.
pub fn audit_ui(theme: &UiTheme) -> Vec<Failure> {
    audit(UI_PAIRINGS, theme)
}

/// Every [`EDITOR_PAIRINGS`] entry that misses its floor in `theme`.
pub fn audit_editor(theme: &EditorTheme) -> Vec<Failure> {
    audit(EDITOR_PAIRINGS, theme)
}

/// `pair!(text on bg_panel, Body, "where")` — the common case, both sides being
/// fields of the theme struct. The `fixed` arm takes an expression instead, for
/// the handful of colours that are the same in every theme by design (white on a
/// coloured button).
macro_rules! pair {
    ($fg:ident on $bg:ident, $role:ident, $site:expr) => {
        Pairing {
            fg: stringify!($fg),
            bg: stringify!($bg),
            role: Legibility::$role,
            site: $site,
            fg_of: |t| t.$fg,
            bg_of: |t| t.$bg,
        }
    };
    (fixed $fg:expr, $fg_name:expr, on $bg:ident, $role:ident, $site:expr) => {
        Pairing {
            fg: $fg_name,
            bg: stringify!($bg),
            role: Legibility::$role,
            site: $site,
            fg_of: |_| $fg,
            bg_of: |t| t.$bg,
        }
    };
}

/// Like `pair!`, for a control that fades *whole* — a disabled action, where
/// the label and the fill are both at half strength. Neither half is opaque, so
/// both have to be composited against the surface behind them before the pair
/// can be measured at all.
macro_rules! disabled {
    ($fg:ident($fa:expr) on $fill:ident($ba:expr) over $bg:ident, $role:ident, $site:expr) => {
        Pairing {
            fg: concat!(stringify!($fg), "@", stringify!($fa)),
            bg: concat!(
                stringify!($fill),
                "@",
                stringify!($ba),
                " over ",
                stringify!($bg)
            ),
            role: Legibility::$role,
            site: $site,
            fg_of: |t| {
                over(
                    t.$fg.multiply_alpha($fa),
                    over(t.$fill.multiply_alpha($ba), t.$bg),
                )
            },
            bg_of: |t| over(t.$fill.multiply_alpha($ba), t.$bg),
        }
    };
}

/// Like [`pair`], for a foreground on a translucent wash over a surface — the
/// results grid paints most of its cell states that way, and what the eye reads
/// is the composite, not the wash.
macro_rules! wash {
    ($fg:ident on $wash:ident($alpha:expr) over $bg:ident, $role:ident, $site:expr) => {
        Pairing {
            fg: stringify!($fg),
            bg: concat!(
                stringify!($wash),
                "@",
                stringify!($alpha),
                " over ",
                stringify!($bg)
            ),
            role: Legibility::$role,
            site: $site,
            fg_of: |t| t.$fg,
            bg_of: |t| over(t.$wash.multiply_alpha($alpha), t.$bg),
        }
    };
}

const WHITE: Color = Color::rgb8(0xFF, 0xFF, 0xFF);

/// Every foreground/background pairing the chrome paints, grouped by surface.
///
/// Each entry is a site that exists in the code — the point of the table is that
/// it describes the app, not an aspiration. When a view starts drawing a role on
/// a surface that isn't listed here, add the row.
pub const UI_PAIRINGS: &[Pairing<UiTheme>] = &[
    // ── The footer (`bg_deepest`), at FONT_STATUS = 12px, the smallest type in
    // the app. This is the surface [B16-L2-01] was about: every accent below
    // used to be a hardcoded literal picked against the dark footer, and the
    // light theme moved the footer to #DCDFE6 underneath them.
    pair!(status_text on bg_deepest, Body, "footer: Ln/Col, indent, wrap, CPU, RAM"),
    pair!(status_warn on bg_deepest, Body, "footer: N warnings, Write mode"),
    pair!(status_warn_hover on bg_deepest, Body, "footer: hovering Write mode"),
    pair!(status_ok on bg_deepest, Icon, "footer: the no-warnings check"),
    pair!(tx_open on bg_deepest, Body, "footer: Manual + the Tx open pill"),
    pair!(tx_open_hover on bg_deepest, Body, "footer: hovering Manual"),
    pair!(tx_danger on bg_deepest, Body, "footer: aborted/lost transaction pill"),
    pair!(tx_commit on bg_deepest, Body, "footer: Commit"),
    pair!(tx_commit_hover on bg_deepest, Body, "footer: hovering Commit"),
    pair!(tx_rollback on bg_deepest, Body, "footer: Rollback"),
    pair!(tx_rollback_hover on bg_deepest, Body, "footer: hovering Rollback"),
    pair!(chip_active on bg_deepest, Body, "footer: hovering a status segment"),
    pair!(text on bg_deepest, Body, "AI chat code block, completion doc popup"),
    pair!(text_dim on bg_deepest, Body, "completion popup: the doc line"),
    pair!(text_muted on bg_deepest, Icon, "completion popup: the kind label"),
    pair!(placeholder on bg_deepest, Recessive, "AI panel: the message field"),
    // ── Header / tab strip / dropdown menus (`bg_chrome`).
    pair!(text on bg_chrome, Body, "header: connection + active database"),
    pair!(text_dim on bg_chrome, Body, "header + tab strip labels"),
    pair!(text_muted on bg_chrome, Icon, "header: the three chrome icons"),
    pair!(text_faint on bg_chrome, Recessive, "connection menu: the endpoint line"),
    pair!(error on bg_chrome, Body, "header: an unreachable connection"),
    pair!(accent on bg_chrome, Body, "connection menu: Manage Connections"),
    pair!(conn_list_text on bg_chrome, Body, "connection menu: a connection name"),
    pair!(tab_text on bg_chrome, Body, "tab strip: an inactive tab's label"),
    pair!(tab_close on bg_chrome, Recessive, "tab strip: the close glyph"),
    pair!(text on tab_active, Body, "tab strip: the active tab's label"),
    // ── Side panels, modals and popup menus (`bg_panel`).
    pair!(text on bg_panel, Body, "schema tree, modals, menu rows"),
    pair!(text_dim on bg_panel, Body, "panel labels + every form caption"),
    pair!(text_muted on bg_panel, Icon, "section titles, secondary metadata"),
    pair!(text_faint on bg_panel, Recessive, "form hints, count capsules, empty states"),
    pair!(search_hint on bg_panel, Recessive, "schema tree: the search box hint"),
    pair!(error on bg_panel, Body, "modal + panel error lines"),
    pair!(accent on bg_panel, Body, "selected/active affordances in a panel"),
    pair!(match_highlight on bg_panel, Body, "Find Anywhere: the matched substring"),
    pair!(plan_warn on bg_panel, Body, "import modal: a pre-flight warning"),
    pair!(change_count on bg_panel, Body, "table designer: the N changes count"),
    // The in-form button surface — `theme::control_bg()` is `erd_canvas` under
    // another name. Two roles paint on it and neither had a row, so a palette
    // edit could have moved either silently, which is the one thing this table
    // exists to stop.
    pair!(text on erd_canvas, Body, "in-form buttons: Choose file…, Add value"),
    pair!(text_faint on erd_canvas, Recessive, "an in-form button with nothing to act on"),
    // [B16-L2-01] read these two as a fill under white text and set them aside
    // as "therefore fine". They aren't a fill: `footer_button` takes a colour
    // fn, so this is red *text* on the modal panel.
    pair!(confirm_yes on bg_panel, Body, "confirm modal: Yes; DDL preview: a destructive Apply"),
    pair!(confirm_yes_hover on bg_panel, Body, "confirm modal: hovering Yes"),
    pair!(tx_danger on bg_panel, Body, "transaction modal: an aborted transaction"),
    pair!(key_primary on bg_panel, Icon, "schema tree: the primary-key glyph"),
    pair!(key_index on bg_panel, Icon, "schema tree: the index glyph"),
    pair!(key_foreign on bg_panel, Icon, "schema tree: the foreign-key glyph"),
    pair!(favorite_star on bg_panel, Icon, "schema tree: a favourited database"),
    pair!(db_icon on bg_panel, Icon, "schema tree: the database glyph"),
    pair!(table_icon on bg_panel, Icon, "schema tree: the table glyph"),
    pair!(view_icon on bg_panel, Icon, "schema tree: the view glyph"),
    pair!(db_toggle_on on bg_panel, Body, "database visibility menu: a shown row"),
    pair!(db_toggle_off on bg_panel, Recessive, "database visibility menu: a hidden row"),
    // Manage Connections' footer actions used to be coloured text on the panel;
    // they are filled buttons now, so those seven pairings are the `btn_*` rows
    // above rather than seven of their own.
    pair!(conn_ok on bg_panel, Icon, "connection menu: the reachable dot"),
    pair!(text_muted on capsule_bg, Icon, "schema tree: the N cols / N keys capsule"),
    // ── Rows: hover, active and the keyboard-nav cursor.
    pair!(text on row_hover, Body, "schema tree: a hovered row"),
    // The query-history panel's own two surfaces. Both arrived with the panel and
    // neither had a row, so the gate measured nothing it paints — and the unit
    // here is the **pairing**, not the colour, so reusing a listed colour on a
    // new surface is exactly what it cannot see.
    pair!(text on row_hover_soft, Body, "history: a hovered row"),
    pair!(text_faint on row_hover_soft, Recessive, "history: its timestamp and outcome line"),
    pair!(text_faint on group_header_bg, Recessive, "history: the TODAY / THIS WEEK band"),
    pair!(text on row_active, Body, "schema tree: the selected row"),
    pair!(text on row_selected, Body, "schema tree: the keyboard-nav cursor row"),
    pair!(text_dim on row_selected, Body, "designer list: a selected row's detail"),
    pair!(key_primary on row_selected, Icon, "schema tree: a key glyph under the cursor"),
    pair!(pill_active_text on pill_active_bg, Body, "designer tabs: the active pill"),
    // Modal footer actions. Each variant's own label on its own fill, resting and
    // hovered, since a hover is a state a label is read in, not a decoration.
    pair!(btn_neutral_text on btn_neutral, Body, "modal footer: Cancel / Back"),
    pair!(btn_neutral_text on btn_neutral_hover, Body, "modal footer: hovering it"),
    pair!(btn_primary_text on btn_primary, Body, "modal footer: Preview SQL / Apply"),
    pair!(btn_primary_text on btn_primary_hover, Body, "modal footer: hovering it"),
    pair!(btn_quiet_text on btn_quiet, Body, "preview footer: Copy / Open in editor"),
    pair!(btn_quiet_text on btn_quiet_hover, Body, "preview footer: hovering one"),
    pair!(btn_danger_text on btn_danger, Body, "preview footer: Apply, on a destructive plan"),
    pair!(btn_danger_text on btn_danger_hover, Body, "preview footer: hovering it"),
    // Test's result icon, on the neutral fill it flashes inside. Both hover
    // states listed too: the pointer is on the button that was just pressed.
    pair!(conn_test_ok on btn_neutral, Icon, "Manage Connections: the test-passed tick"),
    pair!(conn_test_ok on btn_neutral_hover, Icon, "Manage Connections: hovering it"),
    pair!(conn_test_fail on btn_neutral, Icon, "Manage Connections: the test-failed cross"),
    pair!(conn_test_fail on btn_neutral_hover, Icon, "Manage Connections: hovering it"),
    // Preview SQL before there is anything to preview — the one action that sits
    // disabled for more than a moment, so it's the disabled state worth tracking.
    disabled!(btn_primary_text(0.5) on btn_primary(0.5) over bg_panel, Recessive, "modal footer: an action not yet available"),
    pair!(text on pill_hover_bg, Body, "designer tabs: a hovered pill"),
    pair!(text on dropdown_hover, Body, "settings dropdown: a hovered option"),
    pair!(text on dropdown_active, Body, "settings dropdown: the chosen option"),
    pair!(conn_list_sel_text on conn_list_sel_bg, Body, "Manage Connections: the selected row"),
    // ── Text inputs (`bg_editor` is the field surface, not the SQL editor).
    pair!(text on bg_editor, Body, "every text field's content"),
    pair!(placeholder on bg_editor, Recessive, "every text field's placeholder"),
    pair!(text_faint on bg_editor, Recessive, "import modal: an inferred value"),
    pair!(cmdk_text on bg_editor, Body, "Ctrl+K: the prompt line"),
    pair!(cmdk_placeholder on bg_editor, Recessive, "Ctrl+K: its placeholder"),
    // ── Results grid.
    pair!(text on bg_results, Body, "grid: a cell value"),
    pair!(text_faint on bg_results, Recessive, "grid: a NULL / auto placeholder"),
    pair!(text_dim on bg_header_row, Body, "grid: a column name"),
    pair!(text_faint on bg_header_row, Recessive, "grid: the type line under a name"),
    pair!(chip_active on bg_header_row, Body, "grid: the sorted column's name"),
    pair!(text_dim on grid_col_sel, Body, "grid: a selected column's header"),
    pair!(text on grid_edit_staged, Body, "grid: a cell with a staged edit"),
    wash!(text on accent(0.30) over bg_results, Body, "grid: the active cell"),
    wash!(text on accent(0.16) over bg_results, Body, "grid: a selected cell"),
    wash!(text on error(0.15) over bg_results, Body, "grid: a row marked for deletion"),
    wash!(text_faint on grid_edit_staged(0.15) over bg_results, Recessive, "grid: an unset cell of a pending row"),
    pair!(text on completion_active, Body, "completion popup: the selected row"),
    pair!(text_muted on completion_active, Icon, "completion popup: the kind label"),
    // ── AI panel.
    pair!(text on bubble_user_bg, Body, "AI panel: the user's recap bubble"),
    pair!(bubble_claude_text on bubble_claude_bg, Body, "AI panel: an assistant reply"),
    pair!(text on code_action_bar, Body, "AI panel: a code block's action icons"),
    // ── Query plan modal.
    pair!(text on plan_warn_bg, Body, "query plan: a flagged row"),
    pair!(plan_warn on plan_warn_bg, Body, "query plan: the warning text"),
    // ── ER diagram.
    pair!(text on erd_node_bg, Body, "ERD: an ordinary column's name"),
    pair!(key_primary on erd_node_bg, Body, "ERD: a primary-key column's name"),
    pair!(key_foreign on erd_node_bg, Body, "ERD: a foreign-key column's name"),
    pair!(text_muted on erd_node_bg, Body, "ERD: a column's type"),
    pair!(text_dim on erd_node_bg, Body, "ERD: a stub (cross-database) card's label"),
    pair!(text on erd_node_header, Body, "ERD: a card's table name"),
    pair!(text on erd_row_highlight, Body, "ERD: a row at the end of a hovered edge"),
    // ── Buttons whose foreground and background are chosen together. These are
    // the shape [B16-L2-01]'s sibling bug had: the two halves live in different
    // views, so nothing but a pairing check looks at them side by side.
    pair!(approve_text on approve_bg, Body, "Ctrl+K diff: Approve"),
    pair!(reject_text on reject_bg, Body, "Ctrl+K diff: Reject"),
    pair!(err_fix_btn on reject_bg, Body, "editor + grid error bar: View / AI Fix"),
    pair!(fixed WHITE, "white", on seed_button, Body, "grid: the AI seed-rows Generate button"),
    pair!(diff_add_marker on diff_add_bg, Body, "Ctrl+K diff: the + gutter marker"),
    pair!(diff_del_marker on diff_del_bg, Body, "Ctrl+K diff: the − gutter marker"),
    pair!(toggle_handle_on on toggle_on, Icon, "settings: an on toggle's knob"),
    pair!(toggle_handle_off on toggle_off, Icon, "settings: an off toggle's knob"),
];

/// The editor surface. Only the roles Schemaic *chose* are gated.
///
/// The syntax token colours are not: One Dark Pro, Tokyo Night and Catppuccin
/// Latte are reproductions of published palettes, and "fix the contrast" there
/// means "stop being that theme". `fg`, `gutter_fg` and the two diagnostic
/// underlines are Schemaic's own decisions on someone else's surface, which is
/// exactly where a mismatch can appear — `diag_error` was a fixed red picked
/// against a dark editor and then rendered on Latte's #EFF1F5.
pub const EDITOR_PAIRINGS: &[Pairing<EditorTheme>] = &[
    pair!(fg on bg, Body, "editor: the SQL itself"),
    pair!(gutter_fg on bg, Recessive, "editor: line numbers"),
    pair!(cursor on bg, Icon, "editor: the caret"),
    pair!(underline on bg, Icon, "editor: the keyword-typo squiggle"),
    pair!(diag_error on bg, Icon, "editor: the diagnostic-error squiggle"),
];

// ── The shortfall baseline ──────────────────────────────────────────────────

/// Pairings that don't reach their role's floor **today**, with the ratio each
/// currently manages (rounded down to 0.1).
///
/// Schemaic's chrome is a deliberately dim, low-contrast surface — the Zed
/// lineage the app is styled after is the same — and a good deal of it sits
/// under WCAG AA. Raising all of it is a visual-design decision, not a bug fix,
/// so this gate does not pretend to make it. What it does instead is ratchet:
///
///  * a pairing **not** listed here must meet its floor, so a new colour, a new
///    surface or a new theme is held to AA from the start;
///  * a pairing listed here may never drop below the number recorded, so the
///    dim end of the palette can't quietly get dimmer;
///  * an entry that now passes its floor must be **deleted** — the tests fail on
///    a stale entry, so the list can only shrink.
///
/// Read it as the inventory of what a contrast pass would have to fix. It is
/// deliberately not a list of colours to leave alone.
pub const UI_SHORTFALL: &[Shortfall] = {
    use Legibility::{Body, Icon, Recessive};
    &[
        ("dark", "status_text", "bg_deepest", Body, 3.7),
        ("dark", "text_muted", "bg_deepest", Icon, 2.7),
        ("dark", "placeholder", "bg_deepest", Recessive, 1.5),
        ("dark", "text_muted", "bg_chrome", Icon, 2.6),
        ("dark", "tab_text", "bg_chrome", Body, 3.7),
        ("dark", "tab_close", "bg_chrome", Recessive, 1.4),
        ("dark", "text_dim", "bg_panel", Body, 4.4),
        ("dark", "text_muted", "bg_panel", Icon, 2.5),
        // Modal footer actions. Tinting each label to its own fill put both *resting*
        // states over AA — they aren't listed here at all — so what's left is the
        // hovers, where lightening the fill under an unchanged label costs contrast,
        // and the destructive red, whose fill can only go so light before it stops
        // reading as a warning.
        ("dark", "btn_neutral_text", "btn_neutral_hover", Body, 3.8),
        ("dark", "btn_primary_text", "btn_primary_hover", Body, 3.9),
        // The recessed pair is the one whose *hover* clears 4.5 (it darkens), which is
        // why only its resting state is listed.
        ("dark", "btn_quiet_text", "btn_quiet", Body, 4.2),
        ("dark", "btn_danger_text", "btn_danger", Body, 3.5),
        ("dark", "btn_danger_text", "btn_danger_hover", Body, 2.7),
        ("dark", "search_hint", "bg_panel", Recessive, 1.4),
        ("dark", "db_icon", "bg_panel", Icon, 1.6),
        ("dark", "table_icon", "bg_panel", Icon, 2.7),
        ("dark", "text_muted", "capsule_bg", Icon, 2.1),
        ("dark", "text_dim", "row_selected", Body, 3.3),
        ("dark", "placeholder", "bg_editor", Recessive, 1.3),
        ("dark", "cmdk_placeholder", "bg_editor", Recessive, 1.4),
        ("dark", "text_dim", "bg_header_row", Body, 4.1),
        ("dark", "text_dim", "grid_col_sel", Body, 3.5),
        ("dark", "text_muted", "completion_active", Icon, 1.7),
        ("dark", "text_muted", "erd_node_bg", Body, 2.3),
        ("dark", "text_dim", "erd_node_bg", Body, 4.0),
        ("dark", "text", "grid_edit_staged", Body, 2.1),
        (
            "dark",
            "text_faint",
            "grid_edit_staged@0.15 over bg_results",
            Recessive,
            1.9,
        ),
        ("dark", "reject_text", "reject_bg", Body, 2.3),
        ("dark", "toggle_handle_off", "toggle_off", Icon, 1.8),
        ("light", "status_text", "bg_deepest", Body, 3.6),
        ("light", "chip_active", "bg_deepest", Body, 3.8),
        ("light", "text_muted", "bg_deepest", Icon, 2.4),
        ("light", "placeholder", "bg_deepest", Recessive, 1.4),
        ("light", "text_muted", "bg_chrome", Icon, 2.7),
        ("light", "text_faint", "bg_chrome", Recessive, 1.9),
        ("light", "error", "bg_chrome", Body, 4.1),
        ("light", "accent", "bg_chrome", Body, 4.3),
        ("light", "tab_text", "bg_chrome", Body, 2.7),
        ("light", "tab_close", "bg_chrome", Recessive, 1.4),
        ("light", "text_muted", "bg_panel", Icon, 2.8),
        ("light", "btn_quiet_text", "btn_quiet", Body, 3.5),
        ("light", "btn_quiet_text", "btn_quiet_hover", Body, 3.1),
        ("light", "search_hint", "bg_panel", Recessive, 1.6),
        ("light", "error", "bg_panel", Body, 4.3),
        ("light", "plan_warn", "bg_panel", Body, 3.2),
        ("light", "key_primary", "bg_panel", Icon, 2.2),
        ("light", "favorite_star", "bg_panel", Icon, 2.6),
        ("light", "view_icon", "bg_panel", Icon, 2.6),
        ("light", "db_toggle_off", "bg_panel", Recessive, 1.9),
        ("light", "text_muted", "capsule_bg", Icon, 2.4),
        ("light", "text_dim", "row_selected", Body, 4.1),
        ("light", "key_primary", "row_selected", Icon, 1.7),
        ("light", "placeholder", "bg_editor", Recessive, 1.7),
        ("light", "cmdk_placeholder", "bg_editor", Recessive, 1.7),
        ("light", "text_faint", "bg_header_row", Recessive, 1.8),
        ("light", "chip_active", "bg_header_row", Body, 4.2),
        ("light", "text_muted", "completion_active", Icon, 2.5),
        ("light", "plan_warn", "plan_warn_bg", Body, 3.0),
        ("light", "key_primary", "erd_node_bg", Body, 2.5),
        ("light", "text_muted", "erd_node_bg", Body, 3.2),
        ("light", "text", "grid_edit_staged", Body, 3.8),
        (
            "light",
            "text_faint",
            "grid_edit_staged@0.15 over bg_results",
            Recessive,
            1.9,
        ),
        ("light", "reject_text", "reject_bg", Body, 4.0),
        ("light", "err_fix_btn", "reject_bg", Body, 3.3),
        ("light", "diff_add_marker", "diff_add_bg", Body, 3.6),
        ("light", "diff_del_marker", "diff_del_bg", Body, 4.0),
        ("light", "toggle_handle_off", "toggle_off", Icon, 1.5),
    ]
};

/// Same, for the editor surface. All three are Schemaic's own choices on the
/// upstream palettes' backgrounds — `cursor` is Latte's rosewater, which is
/// faithful to that theme and a poor caret.
pub const EDITOR_SHORTFALL: &[Shortfall] = {
    use Legibility::Icon;
    &[
        ("one-dark-pro", "underline", "bg", Icon, 2.7),
        ("catppuccin-latte", "cursor", "bg", Icon, 2.3),
        ("catppuccin-latte", "underline", "bg", Icon, 2.3),
    ]
};

/// One baselined pairing:
/// `(theme key, foreground, background, the role it was excused at, ratio)`.
///
/// **The role is part of the key**, not decoration. Without it the gate matched
/// on `(fg, bg)` alone, so *reusing* a listed colour in a more demanding place
/// was invisible: `text_muted on bg_panel` was baselined as an **Icon** pairing
/// at 2.5, and when every form caption in the app was routed through that same
/// colour the audit went on passing — body text at 2.55:1, held to an icon's
/// floor by a row that meant something else. A new colour would have been held
/// to AA; a reused one was not. Now a role change is a key change, so the
/// pairing has to meet its new floor or somebody has to write the exemption down
/// on purpose.
pub type Shortfall = (&'static str, &'static str, &'static str, Legibility, f64);

/// The ratio a pairing is baselined at in `list`, if it is — at *that role*.
pub fn baselined(
    list: &[Shortfall],
    theme: &str,
    fg: &str,
    bg: &str,
    role: Legibility,
) -> Option<f64> {
    list.iter()
        .find(|(t, f, b, ro, _)| *t == theme && *f == fg && *b == bg && *ro == role)
        .map(|(.., r)| *r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::themes::{EditorThemeKind, UiThemeKind};

    #[test]
    fn contrast_ratio_spans_the_wcag_range() {
        let black = Color::rgb8(0, 0, 0);
        let white = Color::rgb8(255, 255, 255);
        assert!((contrast_ratio(black, white) - 21.0).abs() < 0.01);
        assert!((contrast_ratio(white, white) - 1.0).abs() < 0.001);
        assert!((contrast_ratio(black, black) - 1.0).abs() < 0.001);
    }

    #[test]
    fn contrast_ratio_is_symmetric() {
        let a = Color::rgb8(0x6E, 0x71, 0x81);
        let b = Color::rgb8(0xDC, 0xDF, 0xE6);
        assert!((contrast_ratio(a, b) - contrast_ratio(b, a)).abs() < 1e-12);
    }

    /// The numbers [B16-L2-01] was filed on, recomputed here so the maths itself
    /// is pinned to an independent measurement rather than to its own output.
    #[test]
    fn matches_the_ratios_the_finding_measured() {
        let light_footer = Color::rgb8(0xDC, 0xDF, 0xE6);
        let dark_footer = Color::rgb8(0x14, 0x15, 0x1A);
        let tx_open = Color::rgb8(0xE0, 0xB2, 0x4B); // the old, fixed value
        assert!((contrast_ratio(tx_open, light_footer) - 1.48).abs() < 0.01);
        // The same colour was never the problem — it was fine on the footer it
        // had been picked against.
        assert!(contrast_ratio(tx_open, dark_footer) > 8.0);
    }

    #[test]
    fn over_composites_towards_the_backdrop() {
        let bg = Color::rgb8(0, 0, 0);
        let half_white = Color::rgb8(255, 255, 255).multiply_alpha(0.5);
        let mixed = over(half_white, bg);
        assert_eq!(mixed.a, 255);
        assert!((mixed.r as i16 - 128).abs() <= 1, "got {}", mixed.r);
        // Fully opaque and fully transparent are the two endpoints.
        assert_eq!(over(Color::rgb8(10, 20, 30), bg), Color::rgb8(10, 20, 30));
        assert_eq!(over(Color::rgb8(10, 20, 30).multiply_alpha(0.0), bg), bg);
    }

    /// A theme's measured pairings, split into the three things the gate cares
    /// about: what fails its floor and isn't baselined, what has fallen *below*
    /// its baseline, and what now passes and should therefore leave the list.
    fn check<T>(
        pairings: &'static [Pairing<T>],
        shortfall: &[Shortfall],
        key: &str,
        label: &str,
        theme: &T,
        bad: &mut Vec<String>,
        stale: &mut Vec<String>,
    ) {
        for p in pairings {
            let r = p.ratio(theme);
            match baselined(shortfall, key, p.fg, p.bg, p.role) {
                None => {
                    if r < p.role.floor() {
                        bad.push(format!(
                            "[{label}] {} on {} = {r:.2}:1 (needs {:.1}:1) — {}",
                            p.fg,
                            p.bg,
                            p.role.floor(),
                            p.site
                        ));
                    }
                }
                Some(base) if r + 0.005 < base => bad.push(format!(
                    "[{label}] {} on {} got worse: {r:.2}:1, baselined at {base:.1}:1 — {}",
                    p.fg, p.bg, p.site
                )),
                Some(_) if r >= p.role.floor() => stale.push(format!(
                    "[{label}] {} on {} now meets {:.1}:1 at {r:.2}:1 — drop it from the baseline",
                    p.fg,
                    p.bg,
                    p.role.floor()
                )),
                Some(_) => {}
            }
        }
    }

    fn report(bad: Vec<String>, stale: Vec<String>) {
        assert!(
            bad.is_empty(),
            "contrast regressions:\n  {}",
            bad.join("\n  ")
        );
        assert!(
            stale.is_empty(),
            "the baseline can only shrink:\n  {}",
            stale.join("\n  ")
        );
    }

    /// The gate. Every pairing the chrome paints, in every built-in UI theme.
    #[test]
    fn every_ui_pairing_is_legible_in_every_theme() {
        let (mut bad, mut stale) = (Vec::new(), Vec::new());
        for kind in UiThemeKind::ALL {
            let t = kind.build();
            check(
                UI_PAIRINGS,
                UI_SHORTFALL,
                kind.key(),
                kind.label(),
                &t,
                &mut bad,
                &mut stale,
            );
        }
        report(bad, stale);
    }

    #[test]
    fn every_editor_pairing_is_legible_in_every_theme() {
        let (mut bad, mut stale) = (Vec::new(), Vec::new());
        for kind in EditorThemeKind::ALL {
            let t = kind.build();
            check(
                EDITOR_PAIRINGS,
                EDITOR_SHORTFALL,
                kind.key(),
                kind.label(),
                &t,
                &mut bad,
                &mut stale,
            );
        }
        report(bad, stale);
    }

    /// The ratchet is the whole design, so it gets its own test rather than
    /// being trusted because the real table happens to pass. `text` on
    /// `bg_deepest` is 12.6:1 in Dark, so a baseline claiming more than that is
    /// a regression, one claiming less is stale, and a floor above it fails.
    #[test]
    fn the_ratchet_catches_regression_stale_and_plain_failure() {
        const TABLE: &[Pairing<UiTheme>] = &[pair!(text on bg_deepest, Body, "probe")];
        let dark = UiThemeKind::Dark.build();
        let run = |shortfall: &[Shortfall]| {
            let (mut bad, mut stale) = (Vec::new(), Vec::new());
            check(
                TABLE, shortfall, "dark", "Dark", &dark, &mut bad, &mut stale,
            );
            (bad, stale)
        };

        // Meets its floor and isn't baselined: silence.
        let (bad, stale) = run(&[]);
        assert!(bad.is_empty() && stale.is_empty(), "{bad:?} {stale:?}");

        // Baselined above what it manages: it got worse.
        let (bad, stale) = run(&[("dark", "text", "bg_deepest", Legibility::Body, 20.0)]);
        assert_eq!(bad.len(), 1, "{bad:?}");
        assert!(bad[0].contains("got worse"), "{}", bad[0]);
        assert!(stale.is_empty());

        // Baselined but now passing: the entry has to go.
        let (bad, stale) = run(&[("dark", "text", "bg_deepest", Legibility::Body, 2.0)]);
        assert!(bad.is_empty(), "{bad:?}");
        assert_eq!(stale.len(), 1, "{stale:?}");
        assert!(stale[0].contains("drop it"), "{}", stale[0]);

        // A baseline for a *different* theme doesn't exempt this one.
        const FAILING: &[Pairing<UiTheme>] = &[pair!(placeholder on bg_editor, Body, "probe")];
        let fails = |shortfall: &[Shortfall]| {
            let (mut bad, mut stale) = (Vec::new(), Vec::new());
            check(
                FAILING, shortfall, "dark", "Dark", &dark, &mut bad, &mut stale,
            );
            (bad, stale)
        };
        let (bad, stale) = fails(&[("light", "placeholder", "bg_editor", Legibility::Body, 1.0)]);
        assert_eq!(bad.len(), 1, "{bad:?}");
        assert!(bad[0].contains("needs 4.5:1"), "{}", bad[0]);
        assert!(stale.is_empty());

        // Nor does a baseline recorded at a *different role*. This is the hole
        // the role in the key closes: `text_muted on bg_panel` was excused as an
        // Icon pairing, and routing every form caption through it made body text
        // inherit that excuse without a line of the table changing.
        let (bad, _) = fails(&[(
            "dark",
            "placeholder",
            "bg_editor",
            Legibility::Recessive,
            1.0,
        )]);
        assert_eq!(bad.len(), 1, "{bad:?}");
        assert!(bad[0].contains("needs 4.5:1"), "{}", bad[0]);
        // At the role it actually is, the same entry exempts it.
        let (bad, _) = fails(&[("dark", "placeholder", "bg_editor", Legibility::Body, 1.0)]);
        assert!(bad.is_empty(), "{bad:?}");
    }

    /// A baseline entry that names a pairing (or a theme) nobody has is worse
    /// than no entry: it silently exempts nothing while looking like it does.
    #[test]
    fn every_baseline_entry_names_a_real_pairing() {
        // The **role** has to match too, which is what makes it part of the key:
        // move a pairing to a harder role and its old exemption stops applying,
        // so the pairing must either meet the new floor or be baselined again
        // deliberately. Body text inherited an icon's excuse exactly once, and
        // this is the assertion that would have caught it.
        for (theme, fg, bg, role, _) in UI_SHORTFALL {
            assert!(
                UiThemeKind::ALL.iter().any(|k| k.key() == *theme),
                "no UI theme {theme:?}"
            );
            assert!(
                UI_PAIRINGS
                    .iter()
                    .any(|p| p.fg == *fg && p.bg == *bg && p.role == *role),
                "no pairing {fg} on {bg} at {role:?}"
            );
        }
        for (theme, fg, bg, role, _) in EDITOR_SHORTFALL {
            assert!(
                EditorThemeKind::ALL.iter().any(|k| k.key() == *theme),
                "no editor theme {theme:?}"
            );
            assert!(
                EDITOR_PAIRINGS
                    .iter()
                    .any(|p| p.fg == *fg && p.bg == *bg && p.role == *role),
                "no pairing {fg} on {bg} at {role:?}"
            );
        }
    }

    /// A pairing that names a role nobody paints is worse than no row at all —
    /// it reads as coverage. Every entry must at least be measurable.
    #[test]
    fn every_pairing_measures_something() {
        for kind in UiThemeKind::ALL {
            let t = kind.build();
            for p in UI_PAIRINGS {
                assert!(p.ratio(&t) >= 1.0, "{} on {}", p.fg, p.bg);
            }
        }
    }

    /// The status-bar accents [B16-L2-01] filed are the reason this module
    /// exists, so they are pinned by name rather than left to the table: none of
    /// them may re-enter the baseline.
    #[test]
    fn the_status_accents_meet_aa_in_both_themes() {
        const ACCENTS: &[&str] = &[
            "status_warn",
            "status_warn_hover",
            "status_ok",
            "tx_open",
            "tx_open_hover",
            "tx_danger",
            "tx_commit",
            "tx_commit_hover",
            "tx_rollback",
            "tx_rollback_hover",
            "change_count",
            "confirm_yes",
            "confirm_yes_hover",
        ];
        for name in ACCENTS {
            let rows: Vec<_> = UI_PAIRINGS.iter().filter(|p| p.fg == *name).collect();
            assert!(!rows.is_empty(), "{name} is painted nowhere in the table");
            for kind in UiThemeKind::ALL {
                let t = kind.build();
                for p in rows.iter() {
                    assert!(
                        baselined(UI_SHORTFALL, kind.key(), p.fg, p.bg, p.role).is_none(),
                        "{name} on {} is baselined in {}",
                        p.bg,
                        kind.label()
                    );
                    assert!(
                        p.ratio(&t) >= p.role.floor(),
                        "{name} on {} in {} = {:.2}:1",
                        p.bg,
                        kind.label(),
                        p.ratio(&t)
                    );
                }
            }
        }
    }
}
