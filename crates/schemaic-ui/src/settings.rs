//! The settings modals (Terminal / AI Assistant / Appearance) + the keyboard
//! `Shortcuts` reference modal, plus their shared controls: a labelled toggle
//! row, a generic dropdown, and the themed switch. Each control binds straight to
//! its persisted signal.

use std::rc::Rc;

use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;

use crate::consts::{CHAT_PAD_H, TERM_FONT_SIZES};
use crate::widgets::{autohide, modal_title, panel_style};
use crate::{AiEffort, AiModel, FieldCfg, SchemaScope, TermCursor, Ui, edit_field, icons, theme};

// ===== moved from lib.rs (settings modals) =====
// The Terminal settings pane: shell + font size + cursor style dropdowns, and
// copy-on-select / blink toggles. Every control binds straight to its persisted
// signal — picking a shell respawns the terminal; the rest apply live.
pub(crate) fn term_settings_overlay(ui: Ui) -> impl IntoView {
    let open = ui.term.settings_open;
    let shells = ui.term.shells;
    let selected = ui.term.shell_selected;
    let apply = ui.term_actions.apply_shell.clone();
    let font_size = ui.term.font_size;
    let copy_on_select = ui.term.copy_on_select;
    let cursor_style = ui.term.cursor_style;
    let cursor_blink = ui.term.cursor_blink;

    dyn_container(
        move || open.get(),
        move |is_open| {
            if !is_open {
                return empty().into_any();
            }
            let close: Rc<dyn Fn()> = Rc::new(move || open.set(false));

            let shell_dd = shell_dropdown(shells, selected, apply.clone());
            let font_dd = settings_dropdown(font_size, TERM_FONT_SIZES, term_font_label);
            let cursor_dd = settings_dropdown(cursor_style, TermCursor::ALL, TermCursor::label);

            let group = |s: floem::style::Style| s.flex_col().gap(6.0);
            let shell_section = v_stack((settings_group_label("Shell"), shell_dd)).style(group);
            let font_section = v_stack((settings_group_label("Font size"), font_dd)).style(group);
            let cursor_section =
                v_stack((settings_group_label("Cursor style"), cursor_dd)).style(group);
            let copy_row = settings_toggle_row(
                "Copy on selection",
                "Copy selected text to the clipboard the moment a selection ends.",
                copy_on_select,
            );
            let blink_row = settings_toggle_row(
                "Blink cursor",
                "Blink the cursor while the terminal is focused.",
                cursor_blink,
            );

            let body = v_stack((
                shell_section,
                font_section,
                cursor_section,
                copy_row,
                blink_row,
            ))
            .style(|s| s.flex_col().gap(25.0).padding(14.0).width_full());

            let panel = v_stack((modal_title("Terminal", close.clone()), body))
                .on_click_stop(|_| {})
                .style(|s| panel_style(s).background(theme::bg_panel()).width(420.0));

            let esc = close.clone();
            container(panel)
                .keyboard_navigable()
                .request_focus(|| {})
                .on_key_down(Key::Named(NamedKey::Escape), |_| true, move |_| esc())
                .on_click_stop(move |_| close())
                .style(|s| {
                    s.size_full()
                        .items_center()
                        .justify_center()
                        .background(theme::modal_backdrop())
                })
                .into_any()
        },
    )
    .style(move |s| {
        if open.get() {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}

/// Font-size dropdown label (`settings_dropdown` needs a `fn(T) -> &'static str`,
/// so the offered sizes are matched to literals here).
fn term_font_label(n: u16) -> &'static str {
    match n {
        12 => "12",
        14 => "14",
        16 => "16",
        18 => "18",
        _ => "13",
    }
}

/// A shell picker as a dropdown (bound to the selected index over the dynamic
/// shell list). Picking one applies immediately (respawns the terminal).
fn shell_dropdown(
    shells: RwSignal<Vec<schemaic_term::ShellProfile>>,
    selected: RwSignal<usize>,
    apply: Rc<dyn Fn(usize)>,
) -> impl IntoView {
    use floem::views::dropdown::Dropdown;

    let main = move |cur: usize| {
        let name = shells
            .get()
            .get(cur)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        h_stack((
            text(name).style(|s| s.color(theme::text()).font_size(theme::FONT_BODY)),
            empty().style(|s| s.flex_grow(1.0_f32)),
            icons::icon(icons::CHEVRON_DOWN, 16.0)
                .style(|s| s.color(theme::text_dim()).flex_shrink(0.0_f32)),
        ))
        .style(|s| s.items_center().width_full().gap(8.0))
        .into_any()
    };
    // Popup row: shell name + its program/args, with the active row highlighted.
    let row = move |i: usize| {
        let (name, sub): (String, String) = shells
            .get_untracked()
            .get(i)
            .map(|p| {
                let sub = if p.args.is_empty() {
                    p.program.clone()
                } else {
                    format!("{} {}", p.program, p.args.join(" "))
                };
                (p.name.clone(), sub)
            })
            .unwrap_or_default();
        v_stack((
            text(name).style(|s| s.color(theme::text()).font_size(theme::FONT_BODY)),
            text(sub).style(|s| s.color(theme::text_faint()).font_size(theme::FONT_LABEL)),
        ))
        .style(move |s| {
            let s = s
                .width_full()
                .padding_horiz(12.0)
                .padding_vert(6.0)
                .flex_col()
                .gap(2.0)
                .hover(|s| s.background(theme::dropdown_hover()));
            if selected.get() == i {
                s.background(theme::dropdown_active())
            } else {
                s
            }
        })
        .into_any()
    };

    let opts: Vec<usize> = (0..shells.get_untracked().len()).collect();
    Dropdown::custom(move || selected.get(), main, opts, row)
        .on_accept(move |i| (apply)(i))
        .style(dropdown_box_style)
}

/// A self-describing toggle row: title + hint on the left, switch on the right.
/// Shared by the AI and Terminal settings panes.
pub(crate) fn settings_toggle_row(
    title: &'static str,
    hint: &'static str,
    sig: RwSignal<bool>,
) -> impl IntoView {
    h_stack((
        // `flex_grow(1) + min_width(0)`: take the space left of the switch and be
        // allowed to shrink below the text's natural width, so a long hint wraps
        // instead of pushing the toggle past the panel edge.
        v_stack((
            text(title).style(|s| s.font_size(theme::FONT_BODY).color(theme::text())),
            text(hint).style(|s| s.font_size(theme::FONT_LABEL).color(theme::text_faint())),
        ))
        .style(|s| s.flex_col().gap(2.0).flex_grow(1.0_f32).min_width(0.0)),
        themed_toggle(sig),
    ))
    .style(|s| s.items_center().width_full().gap(10.0))
}

// A single selectable option row (check when active), used by the AI settings
// model/effort lists. Mirrors the terminal shell-picker rows.
// A themed `<select>`-style dropdown for the settings modal. The closed box
// looks like an `edit_field` (dark surface, field border, chevron); the popup is
// a floating menu styled via the dropdown's `ScrollClass` (bg_panel + border).
// `active` is the source of truth; `label` renders each variant.
pub(crate) fn settings_dropdown<T>(
    active: RwSignal<T>,
    options: impl IntoIterator<Item = T> + Clone + 'static,
    label: fn(T) -> &'static str,
) -> impl IntoView
where
    T: Copy + PartialEq + 'static,
{
    use floem::views::dropdown::Dropdown;

    // Closed box: selected label on the left, chevron on the right.
    let main = move |cur: T| {
        h_stack((
            text(label(cur)).style(|s| s.color(theme::text()).font_size(theme::FONT_BODY)),
            empty().style(|s| s.flex_grow(1.0_f32)),
            icons::icon(icons::CHEVRON_DOWN, 16.0)
                .style(|s| s.color(theme::text_dim()).flex_shrink(0.0_f32)),
        ))
        .style(|s| s.items_center().width_full().gap(8.0))
        .into_any()
    };

    // Popup row: the label fills the whole row and carries padding + hover +
    // the resting highlight for the currently-active value. (Floem's list
    // `selection` resets to None each open, so we key the highlight off `active`
    // rather than the list's selected state. `ListItemClass` below is neutralised
    // so this is the only styling.)
    let row = move |item: T| {
        text(label(item))
            .style(move |s| {
                let s = s
                    .width_full()
                    .padding_horiz(12.0)
                    .padding_vert(6.0)
                    .color(theme::text())
                    .font_size(theme::FONT_BODY)
                    .hover(|s| s.background(theme::dropdown_hover()));
                if active.get() == item {
                    s.background(theme::dropdown_active())
                } else {
                    s
                }
            })
            .into_any()
    };

    Dropdown::custom(move || active.get(), main, options, row)
        .on_accept(move |item| active.set(item))
        .style(dropdown_box_style)
}

// The closed-box + floating-popup styling shared by every settings dropdown: a
// dark field-like box with a chevron, and a `bg_panel` menu surface with Floem's
// default list chrome neutralised (see `dropdown_item_style`).
fn dropdown_box_style(s: floem::style::Style) -> floem::style::Style {
    use floem::views::scroll::ScrollClass;
    use floem::views::{ListClass, ListItemClass};
    s.width_full()
        .height(32.0)
        .items_center()
        .padding_horiz(CHAT_PAD_H)
        .background(theme::bg_editor())
        .border(1.0)
        .border_color(theme::field_border())
        .border_radius(6.0)
        .hover(|s| s.border_color(theme::field_border_active()))
        // The floating popup (the dropdown's inner scroll) — a menu surface that
        // clears the global scrollbar styling automatically.
        .class(ScrollClass, move |s| {
            s.background(theme::bg_panel())
                .border(1.0)
                .border_color(theme::border())
                .border_radius(8.0)
                .padding_vert(4.0)
                .min_width(150.0)
                // Override Floem's default list chrome. The item rule is nested
                // under `ListClass` so it's inherited from the same nearest
                // ancestor (the list) as the default's `ListClass > ListItemClass`
                // rule and thus wins over it.
                .class(ListClass, |s| {
                    s.border(0.0)
                        .outline(0.0)
                        .focus_visible(|s| s.outline(0.0))
                        .focus(|s| {
                            s.outline(0.0)
                                .border(0.0)
                                .class(ListItemClass, dropdown_item_style)
                        })
                        .class(ListItemClass, dropdown_item_style)
                })
                .class(ListItemClass, dropdown_item_style)
        })
}

// Neutralise Floem's built-in list-item chrome (side margin, padding, border,
// default hover/selected tint) so the row content is the only thing that styles
// the option — see `settings_dropdown`'s `row`.
fn dropdown_item_style(s: floem::style::Style) -> floem::style::Style {
    let transparent = floem::peniko::Color::TRANSPARENT;
    s.margin(0.0)
        .width_full()
        .padding(0.0)
        .border(0.0)
        .border_radius(0.0)
        .background(transparent)
        .hover(|s| s.background(transparent))
        .selected(move |s| {
            s.background(transparent)
                .hover(|s| s.background(transparent))
        })
}

// A small dim group heading inside the AI settings modal.
fn settings_group_label(t: &'static str) -> impl IntoView {
    text(t).style(|s| s.font_size(theme::FONT_LABEL).color(theme::text_muted()))
}

// A dark-theme switch. Track + handle colours are driven by on/off state; the
// track brightens on hover, and the press (active) state is neutralised to match
// hover so there's no distracting flash on click.
pub(crate) fn themed_toggle(sig: RwSignal<bool>) -> impl IntoView {
    use floem::peniko::Brush;
    use floem::style::Foreground;
    use floem::unit::PxPct;
    use floem::views::{ToggleButtonCircleRad, ToggleButtonInset};
    floem::views::toggle_button(move || sig.get())
        .on_toggle(move |v| sig.set(v))
        .style(move |s| {
            let (bg, bg_hover, handle) = if sig.get() {
                (
                    theme::toggle_on(),
                    theme::toggle_on_hover(),
                    theme::toggle_handle_on(),
                )
            } else {
                (
                    theme::toggle_off(),
                    theme::toggle_off_hover(),
                    theme::toggle_handle_off(),
                )
            };
            s.width(36.0)
                .height(18.0)
                .border(0.0)
                .border_radius(9.0)
                .flex_shrink(0.0_f32)
                .set(ToggleButtonInset, PxPct::Pct(12.0))
                .set(ToggleButtonCircleRad, PxPct::Pct(72.0))
                .set(Foreground, Some(Brush::Solid(handle)))
                .background(bg)
                .hover(move |s| s.background(bg_hover))
                .active(move |s| s.background(bg_hover))
                .focus(move |s| s.hover(move |s| s.background(bg_hover)))
        })
}

// AI Assistant settings: CLI path override + model + effort. Changes commit when
// the modal closes (the `ai_apply` callback restarts the session and persists).
pub(crate) fn ai_settings_overlay(ui: Ui) -> impl IntoView {
    let open = ui.ai.settings_open;
    let cli_path = ui.ai.cli_path;
    let model = ui.ai.model;
    let effort = ui.ai.effort;
    let instructions = ui.ai.instructions;
    let scope = ui.ai.schema_scope;
    let run_queries = ui.ai.run_queries;
    let apply = ui.ai_actions.apply.clone();
    let detected = ui.ai_actions.detected_path.clone();
    let cli_ok = ui.ai_actions.cli_ok.clone();

    dyn_container(
        move || open.get(),
        move |is_open| {
            if !is_open {
                return empty().into_any();
            }
            let close: Rc<dyn Fn()> = {
                let apply = apply.clone();
                Rc::new(move || {
                    open.set(false);
                    (apply)();
                })
            };

            let path_field = edit_field(
                cli_path,
                FieldCfg {
                    placeholder: "Leave empty to auto-detect",
                    clearable: true,
                    ..Default::default()
                },
            )
            .style(|s| s.width_full());
            // Hint below the field, reacting to the path's value:
            //  • empty + detected → green "Auto-detected: <path>"
            //  • empty + not detected → red "Auto-detect failed…"
            //  • manual path that resolves → hidden
            //  • manual path that doesn't → red "File doesn't exist."
            let detected = detected.clone();
            let cli_ok = cli_ok.clone();
            let red =
                |s: floem::style::Style| s.font_size(theme::FONT_LABEL).color(theme::reject_bg());
            let hint = dyn_container(
                move || cli_path.get(),
                move |path| {
                    if path.trim().is_empty() {
                        match &detected {
                            Some(p) => text(format!("Auto-detected: {}", p))
                                .style(|s| s.font_size(theme::FONT_LABEL).color(theme::conn_ok())),
                            None => text("Auto-detect failed. Claude CLI not found.").style(red),
                        }
                        .into_any()
                    } else if cli_ok(path) {
                        empty().into_any()
                    } else {
                        text("File doesn't exist.").style(red).into_any()
                    }
                },
            );

            let model_dd = settings_dropdown(model, AiModel::ALL, AiModel::label);
            let effort_dd = settings_dropdown(effort, AiEffort::ALL, AiEffort::label);
            let scope_dd = settings_dropdown(scope, SchemaScope::ALL, SchemaScope::label);

            let instr_field = edit_field(
                instructions,
                FieldCfg {
                    placeholder: "Dialect, conventions, house rules…",
                    multiline: true,
                    ..Default::default()
                },
            )
            .style(|s| s.width_full());

            // Each group is a label + its controls (6px gap); groups are spaced
            // 25px apart.
            let group = |s: floem::style::Style| s.flex_col().gap(6.0);
            let cli_section = v_stack((
                settings_group_label("Claude Code CLI path"),
                path_field,
                hint,
            ))
            .style(group);
            let model_section = v_stack((settings_group_label("Model"), model_dd)).style(group);
            let effort_section = v_stack((settings_group_label("Effort"), effort_dd)).style(group);
            let instr_section =
                v_stack((settings_group_label("Custom instructions"), instr_field)).style(group);
            let scope_section =
                v_stack((settings_group_label("Schema context"), scope_dd)).style(group);
            // Self-describing toggle row (label + hint on the left, switch right).
            let queries_section = settings_toggle_row(
                "Let the assistant run queries",
                "Read-only queries (SELECT/SHOW/…) to inspect your data.",
                run_queries,
            );

            let body = v_stack((
                cli_section,
                model_section,
                effort_section,
                instr_section,
                scope_section,
                queries_section,
            ))
            .style(|s| s.flex_col().gap(25.0).padding(14.0).width_full());

            let panel = v_stack((modal_title("AI Assistant — Settings", close.clone()), body))
                .on_click_stop(|_| {})
                .style(|s| panel_style(s).background(theme::bg_panel()).width(460.0));

            let esc = close.clone();
            container(panel)
                .keyboard_navigable()
                .request_focus(|| {})
                .on_key_down(Key::Named(NamedKey::Escape), |_| true, move |_| esc())
                .on_click_stop(move |_| close())
                .style(|s| {
                    s.size_full()
                        .items_center()
                        .justify_center()
                        .background(theme::modal_backdrop())
                })
                .into_any()
        },
    )
    .style(move |s| {
        if open.get() {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}

/// One shortcut line: description on the left, the key combo on the right in a
/// monospace pill.
fn shortcut_row(keys: &'static str, desc: &'static str) -> impl IntoView {
    h_stack((
        text(desc).style(|s| s.color(theme::text()).font_size(theme::FONT_BODY)),
        empty().style(|s| s.flex_grow(1.0_f32).min_width(12.0)),
        text(keys).style(|s| {
            s.color(theme::text_muted())
                .font_size(theme::FONT_LABEL)
                .font_family("IBM Plex Mono".to_string())
                .background(theme::bg_deepest())
                .padding_horiz(6.0)
                .padding_vert(2.0)
                .border_radius(4.0)
        }),
    ))
    .style(|s| s.width_full().flex_row().items_center().padding_vert(2.0))
}

/// A titled group of shortcut rows (Global / Editor / Results grid).
fn shortcut_group(title: &'static str, rows: &[(&'static str, &'static str)]) -> impl IntoView {
    let rows: Vec<_> = rows.to_vec();
    v_stack((
        text(title).style(|s| {
            s.font_size(theme::FONT_LABEL)
                .color(theme::text_dim())
                .margin_bottom(2.0)
        }),
        v_stack_from_iter(rows.into_iter().map(|(k, d)| shortcut_row(k, d)))
            .style(|s| s.flex_col()),
    ))
    .style(|s| s.flex_col().gap(2.0))
}

// ── Settings modal ───────────────────────────────────────────────────────────
// Grouped by function: General (startup/session behaviour), Editor (font /
// indentation), Query (row cap / write confirmation), Theme (interface +
// SQL-editor themes). Every control binds
// straight to its persisted signal; an effect in the app mirrors the value into
// the live registry (editor font/tab/soft-tabs) or uses it directly (row limit,
// confirm-writes), and saves — so a change applies and sticks instantly.
const EDITOR_FONT_SIZES: [f32; 8] = [11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 18.0, 20.0];
const ROW_LIMITS: [usize; 5] = [1_000, 10_000, 100_000, 200_000, 1_000_000];

fn editor_font_label(px: f32) -> &'static str {
    match px as i32 {
        11 => "11 px",
        12 => "12 px",
        13 => "13 px",
        15 => "15 px",
        16 => "16 px",
        18 => "18 px",
        20 => "20 px",
        _ => "14 px",
    }
}
fn row_limit_label(n: usize) -> &'static str {
    match n {
        1_000 => "1,000",
        10_000 => "10,000",
        100_000 => "100,000",
        1_000_000 => "1,000,000",
        _ => "200,000",
    }
}

// A bold section header separating the functional groups.
fn settings_section_header(t: &'static str) -> impl IntoView {
    text(t).style(|s| {
        s.font_size(theme::FONT_BODY)
            .font_bold()
            .color(theme::text())
            .margin_bottom(2.0)
    })
}

pub(crate) fn theme_settings_overlay(ui: Ui) -> impl IntoView {
    let open = ui.layout.theme_settings_open;
    let ui_theme = ui.layout.ui_theme;
    let editor_theme = ui.layout.editor_theme;
    let editor_font = ui.layout.editor_font;
    let row_limit = ui.layout.row_limit;
    let confirm_writes = ui.layout.confirm_writes;
    let restore_tabs = ui.layout.restore_tabs;

    dyn_container(
        move || open.get(),
        move |is_open| {
            if !is_open {
                return empty().into_any();
            }
            let close: Rc<dyn Fn()> = Rc::new(move || open.set(false));

            let ctrl = |s: floem::style::Style| s.flex_col().gap(6.0);

            // General group.
            let restore_row = settings_toggle_row(
                "Restore tabs on startup",
                "Reopen the query tabs from your last session when the app starts.",
                restore_tabs,
            );
            let general_group = v_stack((settings_section_header("General"), restore_row))
                .style(|s| s.flex_col().gap(16.0));

            // Editor group. (Tab width, spaces-vs-tabs, and word wrap live in the
            // status bar.)
            let font_dd = settings_dropdown(editor_font, EDITOR_FONT_SIZES, editor_font_label);
            let font_section = v_stack((settings_group_label("Font size"), font_dd)).style(ctrl);
            let editor_group = v_stack((settings_section_header("Editor"), font_section))
                .style(|s| s.flex_col().gap(16.0));

            // Query group.
            let row_dd = settings_dropdown(row_limit, ROW_LIMITS, row_limit_label);
            let row_section =
                v_stack((settings_group_label("Default row limit"), row_dd)).style(ctrl);
            let confirm_row = settings_toggle_row(
                "Confirm before running writes",
                "Ask before executing any statement that modifies data or schema.",
                confirm_writes,
            );
            let query_group = v_stack((settings_section_header("Query"), row_section, confirm_row))
                .style(|s| s.flex_col().gap(16.0));

            // Theme group.
            let ui_dd =
                settings_dropdown(ui_theme, theme::UiThemeKind::ALL, theme::UiThemeKind::label);
            let editor_dd = settings_dropdown(
                editor_theme,
                theme::EditorThemeKind::ALL,
                theme::EditorThemeKind::label,
            );
            let ui_section = v_stack((settings_group_label("Interface theme"), ui_dd)).style(ctrl);
            let editor_section =
                v_stack((settings_group_label("Editor theme"), editor_dd)).style(ctrl);
            let theme_group =
                v_stack((settings_section_header("Theme"), ui_section, editor_section))
                    .style(|s| s.flex_col().gap(16.0));

            let body = v_stack((general_group, editor_group, query_group, theme_group))
                .style(|s| s.flex_col().gap(28.0).padding(14.0).width_full());
            // Scroll so the taller grouped modal never overflows the window.
            let body = autohide(scroll(body)).style(|s| s.width_full().max_height(560.0));

            let panel = v_stack((modal_title("Settings", close.clone()), body))
                .on_click_stop(|_| {})
                .style(|s| panel_style(s).background(theme::bg_panel()).width(420.0));

            let esc = close.clone();
            container(panel)
                .keyboard_navigable()
                .request_focus(|| {})
                .on_key_down(Key::Named(NamedKey::Escape), |_| true, move |_| esc())
                .on_click_stop(move |_| close())
                .style(|s| {
                    s.size_full()
                        .items_center()
                        .justify_center()
                        .background(theme::modal_backdrop())
                })
                .into_any()
        },
    )
    .style(move |s| {
        if open.get() {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}

// ── Shortcuts: keyboard-reference modal ──────────────────────────────────────
// Opened from the header's help (?) glyph. Same modal chrome as the Settings
// modal; the body is a read-only reference of the app's keyboard shortcuts,
// scrollable so it never overflows the window.
pub(crate) fn help_overlay(ui: Ui) -> impl IntoView {
    let open = ui.layout.help_open;

    dyn_container(
        move || open.get(),
        move |is_open| {
            if !is_open {
                return empty().into_any();
            }
            let close: Rc<dyn Fn()> = Rc::new(move || open.set(false));

            let body = v_stack((
                shortcut_group(
                    "Global",
                    &[
                        ("Ctrl+P", "Find Anywhere"),
                        ("Ctrl+Shift+P", "Command palette"),
                        ("Ctrl+T", "New query tab"),
                        ("Ctrl+W", "Close query tab"),
                        ("Ctrl+Tab", "Cycle tabs (Shift = reverse)"),
                        ("Ctrl+1…9", "Jump to tab"),
                        ("Ctrl+Shift+E", "Toggle schema panel"),
                        ("Ctrl+Shift+A", "Toggle AI panel"),
                        ("Ctrl+`", "Toggle terminal"),
                    ],
                ),
                shortcut_group(
                    "Editor",
                    &[
                        ("Ctrl+Enter", "Run query"),
                        ("Ctrl+Space", "Autocomplete"),
                        ("Ctrl+K", "Inline AI edit"),
                        ("Ctrl+F", "Find in editor"),
                        ("Ctrl+/", "Toggle line comment"),
                        ("Ctrl+D", "Duplicate line / selection"),
                        ("Ctrl+X", "Delete line"),
                        ("Ctrl+Alt+L", "Format SQL"),
                    ],
                ),
                shortcut_group(
                    "Results grid",
                    &[
                        ("Ctrl+F", "Find in results"),
                        ("Ctrl+C", "Copy"),
                        ("Ctrl+A", "Select all"),
                        ("Enter", "Edit cell / open value"),
                        ("Ctrl+Enter", "Commit edits"),
                    ],
                ),
            ))
            .style(|s| s.flex_col().gap(25.0).padding(14.0).width_full());
            // Scroll the body so the modal never overflows the window.
            let body = autohide(scroll(body)).style(|s| s.width_full().max_height(560.0));

            let panel = v_stack((modal_title("Shortcuts", close.clone()), body))
                .on_click_stop(|_| {})
                .style(|s| panel_style(s).background(theme::bg_panel()).width(420.0));

            let esc = close.clone();
            container(panel)
                .keyboard_navigable()
                .request_focus(|| {})
                .on_key_down(Key::Named(NamedKey::Escape), |_| true, move |_| esc())
                .on_click_stop(move |_| close())
                .style(|s| {
                    s.size_full()
                        .items_center()
                        .justify_center()
                        .background(theme::modal_backdrop())
                })
                .into_any()
        },
    )
    .style(move |s| {
        if open.get() {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}
