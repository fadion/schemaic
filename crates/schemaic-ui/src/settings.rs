//! The settings modals (Terminal / AI Assistant / Appearance) + the keyboard
//! `Shortcuts` reference modal, plus their shared controls: a labelled toggle
//! row, a generic dropdown, and the themed switch. Each control binds straight to
//! its persisted signal.

use std::rc::Rc;

use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;

use crate::consts::{CHAT_PAD_H, TERM_FONT_SIZES};
use crate::widgets::{
    autohide, focus_root_with_ring, form_hint, form_label_style, modal_title, panel_style,
};
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

            // The modal's Tab order. Indices are spaced by 10 so a control can be
            // inserted between two without renumbering — see `FocusRing`.
            let ring = crate::widgets::FocusRing::new();

            let shell_dd = shell_dropdown(shells, selected, apply.clone(), ring.clone(), 10);
            let font_dd = focusable_dropdown(
                font_size,
                TERM_FONT_SIZES,
                term_font_label,
                ring.clone(),
                20,
            );
            let cursor_dd = focusable_dropdown(
                cursor_style,
                TermCursor::ALL,
                TermCursor::label,
                ring.clone(),
                30,
            );

            let group = |s: floem::style::Style| s.flex_col().gap(6.0);
            let shell_section = v_stack((settings_group_label("Shell"), shell_dd)).style(group);
            let font_section = v_stack((settings_group_label("Font size"), font_dd)).style(group);
            let cursor_section =
                v_stack((settings_group_label("Cursor style"), cursor_dd)).style(group);
            let copy_row = focusable_toggle_row(
                "Copy on selection",
                "Copy selected text to the clipboard the moment a selection ends.",
                copy_on_select,
                ring.clone(),
                40,
            );
            let blink_row = focusable_toggle_row(
                "Blink cursor",
                "Blink the cursor while the terminal is focused.",
                cursor_blink,
                ring.clone(),
                50,
            );
            // Kept for the root, which answers Tab by entering the ring.
            let root_ring = ring;

            let body = v_stack((
                shell_section,
                font_section,
                cursor_section,
                copy_row,
                blink_row,
            ))
            .style(|s| s.flex_col().gap(25.0).padding(14.0).width_full());

            let panel = v_stack((
                modal_title("Terminal", close.clone(), root_ring.clone()),
                body,
            ))
            .on_click_stop(|_| {})
            .style(|s| panel_style(s).background(theme::bg_panel()).width(420.0));

            let esc = close.clone();
            // Click-to-dismiss on a sibling behind the panel, never on the focus
            // root — floem fires `Click` there for Space. See
            // `widgets::dismiss_layer`.
            focus_root_with_ring(
                stack((crate::widgets::dismiss_layer(move || close()), panel)),
                root_ring,
            )
            .on_key_down(Key::Named(NamedKey::Escape), |_| true, move |_| esc())
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

/// Font-size dropdown label — computed from the value, so it can't name a
/// different size than the one in effect.
fn term_font_label(n: u16) -> String {
    n.to_string()
}

/// A shell picker as a dropdown (bound to the selected index over the dynamic
/// shell list). Picking one applies immediately (respawns the terminal).
fn shell_dropdown(
    shells: RwSignal<Vec<schemaic_term::ShellProfile>>,
    selected: RwSignal<usize>,
    apply: Rc<dyn Fn(usize)>,
    ring: crate::widgets::FocusRing,
    tabindex: u32,
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
    let dd = Dropdown::custom(move || selected.get(), main, opts, row).style(dropdown_box_style);
    in_ring_dropdown(dd, ring, tabindex, move |i| (apply)(i))
}

/// A self-describing toggle row — title + hint on the left, switch on the right
/// — whose switch is in a modal's Tab order and can be flipped with Space (or
/// Enter, which floem's own switch answers; see [`focusable_toggle`]).
///
/// There is no un-focusable variant, and [`themed_toggle`] is private so there
/// can't be: a switch nobody can Tab to is a control left out of the modal's
/// keyboard order by accident, and the two modals that had one — Query plan and
/// Live monitor — now carry rings of their own.
pub(crate) fn focusable_toggle_row(
    title: &'static str,
    hint: &'static str,
    sig: RwSignal<bool>,
    ring: crate::widgets::FocusRing,
    tabindex: u32,
) -> impl IntoView {
    h_stack((
        // `flex_grow(1) + min_width(0)`: take the space left of the switch and be
        // allowed to shrink below the text's natural width, so a long hint wraps
        // instead of pushing the toggle past the panel edge.
        // The switch's primary label, so `theme::text()` rather than a caption's
        // colour: it is the thing being toggled, not a caption above it.
        v_stack((
            text(title).style(|s| s.color(theme::text()).font_size(theme::FONT_LABEL)),
            form_hint(hint),
        ))
        .style(|s| s.flex_col().gap(2.0).flex_grow(1.0_f32).min_width(0.0)),
        focusable_toggle(sig, ring, tabindex),
    ))
    .style(|s| s.items_center().width_full().gap(10.0))
}

// A themed `<select>`-style dropdown for the settings modal. The closed box
// looks like an `edit_field` (dark surface, field border, chevron); the popup is
// a floating menu styled via the dropdown's `ScrollClass` (bg_panel + border).
// `active` is the source of truth; `label` renders each variant.
/// A settings dropdown.
///
/// `label` may return an owned `String`, so it can be *computed from the value*
/// rather than looked up. Three of these label functions used to be `match`es
/// with a `_` arm handing back the **default's** label, so any value outside the
/// option list read as (say) "200,000" while the app used something else, and no
/// popup row highlighted — the one modal whose job is to report settings
/// accurately, asserting a setting that wasn't in effect. No such value is
/// reachable today; the trap was the next ordinary edit to a list (adding a
/// 24 px size, dropping the 1M row limit), which would have mislabelled the
/// setting for every user who held the removed value.
fn settings_dropdown<T, S>(
    active: RwSignal<T>,
    options: impl IntoIterator<Item = T> + Clone + 'static,
    label: fn(T) -> S,
) -> floem::views::dropdown::Dropdown<T>
where
    T: Copy + PartialEq + 'static,
    // `&'static str` for the enums, whose labels are total over their variants
    // by construction; `String` for the numeric settings, which have to compute
    // theirs.
    S: Into<String> + 'static,
{
    use floem::views::dropdown::Dropdown;

    // Closed box: selected label on the left, chevron on the right.
    let main = move |cur: T| {
        h_stack((
            text(label(cur).into()).style(|s| s.color(theme::text()).font_size(theme::FONT_BODY)),
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
        text(label(item).into())
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
pub(crate) fn dropdown_box_style(s: floem::style::Style) -> floem::style::Style {
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
        // Keyboard focus wears the same lit border as hover — the affordance the
        // box already has — instead of floem's default focus outline, which is a
        // magenta ring belonging to no palette here.
        .focus(|s| s.border_color(theme::field_border_active()))
        .focus_visible(|s| s.outline(0.0))
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
// default hover tint) so the row content is the only thing that styles the
// option — see `settings_dropdown`'s `row`.
//
// **Except the selected state**, which is the keyboard's cursor through the
// list: floem's list moves `selection` on Up/Down, and blanking it (as the rest
// of this function does to floem's chrome) left arrowing through an open
// dropdown with nothing to look at — you could count keypresses and press
// Enter, but not see what you were about to choose. It paints the hover
// background on purpose: the pointer and the keyboard are the same act of
// pointing at a row, and the *resting* highlight for the value already in
// effect is a separate, dimmer colour applied by the row builder.
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
            s.background(theme::dropdown_hover())
                .hover(|s| s.background(theme::dropdown_hover()))
        })
}

// A small dim group heading inside the AI settings modal.
fn settings_group_label(t: &'static str) -> impl IntoView {
    text(t).style(form_label_style)
}

// A dark-theme switch. Track + handle colours are driven by on/off state; the
// track brightens on hover, and the press (active) state is neutralised to match
// hover so there's no distracting flash on click.
fn themed_toggle(sig: RwSignal<bool>) -> impl IntoView {
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
                // A transparent border at rest, so gaining focus changes only its
                // colour: giving the switch a border *on focus* would grow it by
                // 2px and nudge the row it sits in.
                .border(1.0)
                .border_color(floem::peniko::Color::TRANSPARENT)
                .border_radius(9.0)
                .flex_shrink(0.0_f32)
                .set(ToggleButtonInset, PxPct::Pct(12.0))
                .set(ToggleButtonCircleRad, PxPct::Pct(72.0))
                .set(Foreground, Some(Brush::Solid(handle)))
                .background(bg)
                .hover(move |s| s.background(bg_hover))
                .active(move |s| s.background(bg_hover))
                // Keyboard focus: the field's lit border, since a switch reached
                // by Tab had nothing to say it was the thing Space would flip.
                .focus(move |s| {
                    s.border_color(theme::field_border_active())
                        .hover(move |s| s.background(bg_hover))
                })
                .focus_visible(|s| s.outline(0.0))
        })
}

/// A [`themed_toggle`] in a modal's Tab order, operable from the keyboard:
/// **Space** flips it.
///
/// Space and not Enter, because floem answers Enter itself and the two do not
/// compose. `ToggleButton::event_before_children` calls `ontoggle(!state)` on
/// Enter and returns `Continue`, and the listener block then runs *every*
/// registered KeyDown listener, OR-folding without short-circuiting — so a
/// handler here that also inverted the signal flipped it twice and netted zero.
/// The switch is now flipped exactly once per press either way: by floem on
/// Enter, by this listener on Space, which `ToggleButton` ignores.
///
/// `disable_default_event` is not the answer to that overlap: the same gate
/// covers the whole listener block, so taking floem's Enter arm out would take
/// [`crate::widgets::in_focus_ring`]'s Tab/Escape handler with it.
///
/// Safe to add a second KeyDown listener on top of the ring's — floem keeps a
/// `Vec` of listeners per event type, so both run. (`on_cleanup` is the one with
/// a single slot.)
pub(crate) fn focusable_toggle(
    sig: RwSignal<bool>,
    ring: crate::widgets::FocusRing,
    tabindex: u32,
) -> impl IntoView {
    crate::widgets::in_focus_ring(themed_toggle(sig), ring, tabindex).on_event(
        floem::event::EventListener::KeyDown,
        move |e| {
            let floem::event::Event::KeyDown(ke) = e else {
                return floem::event::EventPropagation::Continue;
            };
            use floem::keyboard::{Key, NamedKey};
            // Space only — Enter is floem's, and claiming it here flips twice.
            if ke.key.logical_key == Key::Named(NamedKey::Space) {
                sig.update(|v| *v = !*v);
                return floem::event::EventPropagation::Stop;
            }
            floem::event::EventPropagation::Continue
        },
    )
}

/// A [`settings_dropdown`] in a modal's Tab order.
///
/// Floem's `Dropdown` answers half the keyboard on its own — the popup list is
/// keyboard-navigable in its own right, so Up/Down walk the options and Enter
/// accepts — but **its own open/close is taken over here**, and the rest of this
/// comment is why. Do not "simplify" by deleting the KeyDown handler and
/// `disable_default_event` below on the strength of floem having an Enter/Space
/// arm: that arm fires on *KeyUp*, and restoring it restores the bug this exists
/// to fix.
///
/// Choosing an option hands focus **back to the box**. Floem's dropdown removes
/// its popup without giving the keyboard to anything, and floem clears the focus
/// of a removed view silently — no `FocusGained` lands anywhere — so picking a
/// value with Enter dropped focus out of the modal entirely and the next Tab
/// resumed from wherever the app happened to put it.
///
/// Hung off `on_accept` rather than `on_open(false)`, which fires on *every*
/// close: closing by clicking another control would then yank focus back off the
/// thing just clicked. The trade is that dismissing the popup without choosing
/// still leaves focus adrift.
///
/// **Opening moves to KeyDown**, which is why the open state is driven from a
/// signal here rather than left to floem. Floem toggles the dropdown on *KeyUp*,
/// and the Enter that accepts an option is pressed in the popup and released
/// over the box we have just refocused — so the release reopened the menu every
/// time. `disable_default_event` takes that toggle out and `show_list` puts the
/// state under our control; `on_open` mirrors floem's own opens and closes back
/// into the signal, so a pointer click can't leave the two disagreeing. Arrow
/// keys open it too, the reflex a `<select>` trains.
pub(crate) fn focusable_dropdown<T, S>(
    active: RwSignal<T>,
    options: impl IntoIterator<Item = T> + Clone + 'static,
    label: fn(T) -> S,
    ring: crate::widgets::FocusRing,
    tabindex: u32,
) -> impl IntoView
where
    T: Copy + PartialEq + 'static,
    S: Into<String> + 'static,
{
    in_ring_dropdown(
        settings_dropdown(active, options, label),
        ring,
        tabindex,
        move |item| active.set(item),
    )
}

/// Put an already-built [`Dropdown`](floem::views::dropdown::Dropdown) in a
/// modal's Tab order, taking its open state over.
///
/// The behaviour and every reason for it are documented on
/// [`focusable_dropdown`]; this is the half that doesn't care what the options
/// are, so the app's *other* picker — [`crate::table_designer::focusable_owned_dropdown`],
/// which exists because a table name isn't `Copy` — joins the ring through the
/// same code rather than a second copy of four floem work-arounds.
///
/// `on_accept` is passed in rather than left on the dropdown because floem keeps
/// a **single** accept slot: whatever the builder set is replaced here, so the
/// caller's action has to arrive with the ring.
pub(crate) fn in_ring_dropdown<T>(
    dd: floem::views::dropdown::Dropdown<T>,
    ring: crate::widgets::FocusRing,
    tabindex: u32,
    on_accept: impl Fn(T) + 'static,
) -> impl IntoView
where
    T: Clone + 'static,
{
    use floem::event::{Event, EventListener, EventPropagation};
    use floem::keyboard::{Key, NamedKey};
    use floem::reactive::create_effect;

    let open = RwSignal::new(false);
    // Where the keyboard goes once the popup is gone: the ring entry *now* at
    // this tabindex, resolved when the focus request fires rather than the id
    // captured here. An accept can rebuild the box it came from — the PG trigger
    // editor's Function picker sits in a container keyed on the very signal its
    // own accept writes — and floem's focus request has no existence check, so a
    // captured id parked the keyboard on a removed view and killed the modal.
    let refocus = {
        let ring = ring.clone();
        move || {
            let ring = ring.clone();
            floem::action::exec_after(std::time::Duration::ZERO, move |_| ring.focus_at(tabindex));
        }
    };
    // While the popup is up it holds the keyboard, so Escape reaches neither this
    // box nor the enclosing modal — only the window root. Publish the way to
    // close so the root can, and withdraw it the moment the popup goes. The slot
    // is shared app-wide, so the entry is tagged: this dropdown must not clear
    // another's, which is what the build-time run of this effect (`open` false,
    // possibly while some other popup is up) would otherwise do.
    let token = crate::widgets::popup_token();
    create_effect({
        let refocus = refocus.clone();
        move |_| {
            if open.get() {
                let refocus = refocus.clone();
                crate::widgets::set_open_popup(
                    token,
                    Rc::new(move || {
                        open.set(false);
                        refocus();
                    }),
                );
            } else {
                crate::widgets::clear_open_popup(token);
            }
        }
    });
    let dd = dd
        .show_list(move || open.get())
        // Mirror floem's own state changes (a pointer click, a click-away close)
        // back into the signal. A redundant `OpenState` is a no-op inside the
        // dropdown, so this can't loop.
        .on_open(move |b| {
            if open.get_untracked() != b {
                open.set(b);
            }
        })
        .disable_default_event(|| (EventListener::KeyUp, true))
        // `on_accept` is a single slot, so this replaces the one the builder set
        // and has to carry the caller's action itself.
        .on_accept(move |item| {
            on_accept(item);
            // Deferred: the popup is removed during this same update pass, and a
            // focus request into it would be undone by the removal.
            refocus();
        });
    // The extra cleanup goes *through* the ring helper: floem keeps one cleanup
    // slot per view, so chaining a second one here would replace the ring's.
    // Without it, a dropdown disposed with its popup still up (click the
    // backdrop) left the global slot holding a closure over a dead scope, and
    // the next Escape anywhere in the app was swallowed by it.
    crate::widgets::in_focus_ring_with(dd, ring, tabindex, move || {
        crate::widgets::clear_open_popup(token)
    })
    .on_event(EventListener::KeyDown, move |e| {
        let Event::KeyDown(ke) = e else {
            return EventPropagation::Continue;
        };
        if matches!(
            ke.key.logical_key,
            Key::Named(NamedKey::Enter)
                | Key::Named(NamedKey::Space)
                | Key::Named(NamedKey::ArrowDown)
                | Key::Named(NamedKey::ArrowUp)
        ) {
            open.set(true);
            return EventPropagation::Stop;
        }
        EventPropagation::Continue
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

            // The modal's Tab order; indices spaced by 10 — see `FocusRing`.
            let ring = crate::widgets::FocusRing::new();

            let path_field = edit_field(
                cli_path,
                FieldCfg {
                    placeholder: "Leave empty to auto-detect",
                    clearable: true,
                    focus: Some((ring.clone(), 10)),
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

            let model_dd =
                focusable_dropdown(model, AiModel::ALL, AiModel::label, ring.clone(), 20);
            let effort_dd =
                focusable_dropdown(effort, AiEffort::ALL, AiEffort::label, ring.clone(), 30);
            let scope_dd = focusable_dropdown(
                scope,
                SchemaScope::ALL,
                SchemaScope::label,
                ring.clone(),
                50,
            );

            let instr_field = edit_field(
                instructions,
                FieldCfg {
                    placeholder: "Dialect, conventions, house rules…",
                    multiline: true,
                    focus: Some((ring.clone(), 40)),
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
            let queries_section = focusable_toggle_row(
                "Let the assistant run queries",
                "Read-only queries (SELECT/SHOW/…) to inspect your data.",
                run_queries,
                ring.clone(),
                60,
            );
            // Kept for the root, which answers Tab by entering the ring.
            let root_ring = ring;

            let body = v_stack((
                cli_section,
                model_section,
                effort_section,
                instr_section,
                scope_section,
                queries_section,
            ))
            .style(|s| s.flex_col().gap(25.0).padding(14.0).width_full());

            let panel = v_stack((
                modal_title("AI Assistant — Settings", close.clone(), root_ring.clone()),
                body,
            ))
            .on_click_stop(|_| {})
            .style(|s| panel_style(s).background(theme::bg_panel()).width(460.0));

            let esc = close.clone();
            // Click-to-dismiss on a sibling behind the panel, never on the focus
            // root — floem fires `Click` there for Space. See
            // `widgets::dismiss_layer`.
            focus_root_with_ring(
                stack((crate::widgets::dismiss_layer(move || close()), panel)),
                root_ring,
            )
            .on_key_down(Key::Named(NamedKey::Escape), |_| true, move |_| esc())
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

fn editor_font_label(px: f32) -> String {
    format!("{} px", px as i32)
}

fn row_limit_label(n: usize) -> String {
    thousands(n)
}

/// `1234567` → `"1,234,567"`. The row-limit dropdown's only formatting need, and
/// the reason its label no longer has a list to fall off the end of.
fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
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
    let live_validate = ui.layout.live_validate;
    let restore_tabs = ui.layout.restore_tabs;

    dyn_container(
        move || open.get(),
        move |is_open| {
            if !is_open {
                return empty().into_any();
            }
            let close: Rc<dyn Fn()> = Rc::new(move || open.set(false));

            let ctrl = |s: floem::style::Style| s.flex_col().gap(6.0);

            // The modal's Tab order: one group per section, spaced by 10 within
            // it and by 100 between them — see `FocusRing`.
            let ring = crate::widgets::FocusRing::new();

            // General group.
            let restore_row = focusable_toggle_row(
                "Restore tabs on startup",
                "Reopen the query tabs from your last session when the app starts.",
                restore_tabs,
                ring.clone(),
                10,
            );
            let general_group = v_stack((settings_section_header("General"), restore_row))
                .style(|s| s.flex_col().gap(16.0));

            // Editor group. (Tab width, spaces-vs-tabs, and word wrap live in the
            // status bar.)
            let font_dd = focusable_dropdown(
                editor_font,
                EDITOR_FONT_SIZES,
                editor_font_label,
                ring.clone(),
                100,
            );
            let font_section = v_stack((settings_group_label("Font size"), font_dd)).style(ctrl);
            let editor_group = v_stack((settings_section_header("Editor"), font_section))
                .style(|s| s.flex_col().gap(16.0));

            // Query group.
            let row_dd =
                focusable_dropdown(row_limit, ROW_LIMITS, row_limit_label, ring.clone(), 200);
            let row_section =
                v_stack((settings_group_label("Default row limit"), row_dd)).style(ctrl);
            let confirm_row = focusable_toggle_row(
                "Confirm before running writes",
                "Ask before executing any statement that modifies data or schema.",
                confirm_writes,
                ring.clone(),
                210,
            );
            let validate_row = focusable_toggle_row(
                "Live database validation",
                "Check the statement under the cursor against the database as you type \
                 (a non-executing PREPARE) to surface exact errors. Adds a DB round-trip \
                 on each pause.",
                live_validate,
                ring.clone(),
                220,
            );
            let query_group = v_stack((
                settings_section_header("Query"),
                row_section,
                confirm_row,
                validate_row,
            ))
            .style(|s| s.flex_col().gap(16.0));

            // Theme group.
            let ui_dd = focusable_dropdown(
                ui_theme,
                theme::UiThemeKind::ALL,
                theme::UiThemeKind::label,
                ring.clone(),
                300,
            );
            let editor_dd = focusable_dropdown(
                editor_theme,
                theme::EditorThemeKind::ALL,
                theme::EditorThemeKind::label,
                ring.clone(),
                310,
            );
            // Kept for the root, which answers Tab by entering the ring.
            let root_ring = ring;
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

            let panel = v_stack((
                modal_title("Settings", close.clone(), root_ring.clone()),
                body,
            ))
            .on_click_stop(|_| {})
            .style(|s| panel_style(s).background(theme::bg_panel()).width(420.0));

            let esc = close.clone();
            // Click-to-dismiss on a sibling behind the panel, never on the focus
            // root — floem fires `Click` there for Space. See
            // `widgets::dismiss_layer`.
            focus_root_with_ring(
                stack((crate::widgets::dismiss_layer(move || close()), panel)),
                root_ring,
            )
            .on_key_down(Key::Named(NamedKey::Escape), |_| true, move |_| esc())
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
//
// This is the app's *only* keyboard documentation, and for Ctrl+H and Ctrl+G it
// is the only affordance of any kind — so a binding missing here is a feature
// nobody can find. It renders straight from `shortcuts::SHORTCUTS`, which is the
// one place the list lives and where the reasoning about what earns a row is
// written down; a test there fails the build when a Ctrl/Alt+letter binding
// exists with no row. This view owns only the layout.
pub(crate) fn help_overlay(ui: Ui) -> impl IntoView {
    let open = ui.layout.help_open;

    dyn_container(
        move || open.get(),
        move |is_open| {
            if !is_open {
                return empty().into_any();
            }
            let close: Rc<dyn Fn()> = Rc::new(move || open.set(false));

            let body = v_stack_from_iter(
                crate::shortcuts::SHORTCUTS
                    .iter()
                    .map(|(title, rows)| shortcut_group(title, rows)),
            )
            .style(|s| s.flex_col().gap(25.0).padding(14.0).width_full());
            // Scroll the body so the modal never overflows the window.
            let body = autohide(scroll(body)).style(|s| s.width_full().max_height(560.0));

            // A ring for one button — the ✕, this modal's only control. Without
            // one the root has no Tab handler and Tab falls through to floem's
            // whole-window traversal, out of the modal into the workspace.
            let ring = crate::widgets::FocusRing::new();
            let panel = v_stack((modal_title("Shortcuts", close.clone(), ring.clone()), body))
                .on_click_stop(|_| {})
                .style(|s| panel_style(s).background(theme::bg_panel()).width(420.0));

            let esc = close.clone();
            focus_root_with_ring(
                stack((crate::widgets::dismiss_layer(move || close()), panel)),
                ring,
            )
            .on_key_down(Key::Named(NamedKey::Escape), |_| true, move |_| esc())
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

#[cfg(test)]
mod tests {
    use super::{
        EDITOR_FONT_SIZES, ROW_LIMITS, editor_font_label, row_limit_label, term_font_label,
        thousands,
    };
    use crate::consts::TERM_FONT_SIZES;

    /// The invariant the three dropdown labels used to rely on silently: that
    /// every offered value has a label of its own.
    ///
    /// They were `match`es whose `_` arm returned the **default's** label, so a
    /// value outside the list read as another value entirely and no popup row
    /// highlighted — the settings modal asserting a setting that wasn't in
    /// effect. No such value was reachable, and that was the whole risk: the
    /// trap was the next ordinary edit to one of these lists.
    #[test]
    fn every_offered_option_labels_as_itself() {
        for n in TERM_FONT_SIZES {
            assert_eq!(term_font_label(n), n.to_string());
        }
        for px in EDITOR_FONT_SIZES {
            assert_eq!(editor_font_label(px), format!("{} px", px as i32));
        }
        for n in ROW_LIMITS {
            assert!(
                row_limit_label(n).replace(',', "").parse::<usize>() == Ok(n),
                "{n} labelled {}",
                row_limit_label(n)
            );
        }
    }

    #[test]
    fn a_value_outside_the_list_labels_as_itself_too() {
        // The failure that is now unrepresentable: an off-list value used to
        // borrow the default's label. Hand-edited config, or a list this build
        // no longer offers.
        assert_eq!(term_font_label(21), "21");
        assert_eq!(editor_font_label(17.0), "17 px");
        assert_eq!(row_limit_label(50_000), "50,000");
        assert_ne!(row_limit_label(50_000), row_limit_label(200_000));
    }

    #[test]
    fn thousands_groups_from_the_right() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(12_345), "12,345");
        assert_eq!(thousands(1_000_000), "1,000,000");
    }
}
