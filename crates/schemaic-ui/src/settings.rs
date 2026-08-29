//! The settings modals (Terminal / AI Assistant / Appearance) + the keyboard
//! `Shortcuts` reference modal, plus their shared controls: a labelled toggle
//! row, a generic dropdown, and the themed switch. Each control binds straight to
//! its persisted signal.

use std::rc::Rc;

use floem::AnyView;
use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;

use crate::consts::{TERM_FONT_SIZES, chat_pad_h};
use crate::widgets::{
    ActionKind, MenuEntry, action_button, autohide, focus_root_with_ring, form_hint,
    form_label_style, modal_title, panel_style,
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

            let group = |s: floem::style::Style| s.flex_col().gap(theme::scaled(6.0));
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
            .style(|s| {
                s.flex_col()
                    .gap(theme::scaled(25.0))
                    .padding(theme::scaled(14.0))
                    .width_full()
            });

            let panel = v_stack((
                modal_title("Terminal", close.clone(), root_ring.clone()),
                body,
            ))
            .on_click_stop(|_| {})
            .style(|s| {
                panel_style(s)
                    .background(theme::bg_panel())
                    .width(crate::widgets::modal_w(420.0))
            });

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

/// A shell picker (bound to the selected index over the dynamic shell list).
/// Picking one applies immediately (respawns the terminal).
///
/// The one picker in the app whose rows carry a **detail line**
/// ([`MenuEntry::action_detail`]): a shell's name does not always say which
/// binary it is — "PowerShell 7" and "Windows PowerShell" differ by `pwsh.exe`
/// against `powershell.exe`, and two WSL entries differ only by the `-d` that
/// follows. The closed box shows the name alone, which is what the row of the
/// modal has room for.
fn shell_dropdown(
    shells: RwSignal<Vec<schemaic_term::ShellProfile>>,
    selected: RwSignal<usize>,
    apply: Rc<dyn Fn(usize)>,
    ring: crate::widgets::FocusRing,
    tabindex: u32,
) -> impl IntoView {
    // Rebuilt per opening rather than captured: the shell list is detected
    // asynchronously and the modal can be open before it lands.
    let entries = move || {
        let apply = apply.clone();
        shells
            .get_untracked()
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let cmd = if p.args.is_empty() {
                    p.program.clone()
                } else {
                    format!("{} {}", p.program, p.args.join(" "))
                };
                let apply = apply.clone();
                MenuEntry::action_detail(
                    p.name.clone(),
                    cmd,
                    (selected.get_untracked() == i).then_some(theme::accent),
                    move || (apply)(i),
                )
            })
            .collect()
    };
    in_ring_picker(
        picker_box_content(move || {
            shells
                .get()
                .get(selected.get())
                .map(|p| p.name.clone())
                .unwrap_or_default()
        }),
        entries,
        ring,
        tabindex,
    )
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
            text(title).style(|s| s.color(theme::text()).font_size(theme::font_label())),
            form_hint(hint),
        ))
        .style(|s| {
            s.flex_col()
                .gap(theme::scaled(2.0))
                .flex_grow(1.0_f32)
                .min_width(0.0)
        }),
        focusable_toggle(sig, ring, tabindex),
    ))
    .style(|s| s.items_center().width_full().gap(theme::scaled(10.0)))
}

/// The closed box every picker in the app wears: a dark field-like surface with
/// a chevron, the same chrome an [`crate::edit_field`] draws, so a form of mixed
/// controls reads as one set.
///
/// It used to carry the floating popup's styling too — a `ScrollClass` surface
/// and three nested rules neutralising floem's default list chrome. All of that
/// went with floem's `Dropdown` ([`in_ring_picker`] says why): the panel is now a
/// [`crate::widgets::menu_panel`], which is themed where every other menu in the
/// app is themed.
pub(crate) fn dropdown_box_style(s: floem::style::Style) -> floem::style::Style {
    s.width_full()
        .height(theme::scaled(32.0))
        .items_center()
        .padding_horiz(chat_pad_h())
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
}

// A small dim group heading inside the AI settings modal.
fn settings_group_label(t: &'static str) -> impl IntoView {
    text(t).style(form_label_style)
}

/// The interface-scale picker: four segments, no popup.
///
/// **Where the floem overlay bug was found**, and the record is kept here because
/// it is what [`in_ring_picker`] exists to answer. `OverlayView::paint` (floem
/// 0.2, `window_handle.rs`) nudges an overlay back inside the window when it
/// would overflow — `cx.offset((-x, -y))` — and that nudge is **paint only**:
/// layout, and therefore hit-testing, stays where it was. So a floem `Dropdown`
/// whose popup runs past the window's bottom edge painted its rows in one place
/// and answered the pointer in another, and you had to hover the row *below* the
/// one you wanted. This control hit it first because it is the last row of the
/// last group of the tallest modal — and it hit it *at the middle of the range*
/// (150% then, 130% now), where the modal grows enough to push the popup past the
/// edge but not enough for the body to scroll instead.
///
/// Every other dropdown in the app carried the same latent bug, and the class is
/// now gone rather than the instances: nothing here is built on floem's
/// `Dropdown` any more (`no_floem_dropdown_gate` keeps it that way). **This
/// control stays segmented anyway** — it never needed a popup. Four short
/// options, all visible, one click each, which is how somebody choosing a scale
/// actually behaves, trying them in turn.
///
/// The segments wear the percentage itself (`UiScale::label`) rather than a name
/// like "Large": it is the number somebody opens this control to ask about, it
/// is short enough that four of them fit the row at every scale, and it cannot
/// drift from the factor it selects.
///
/// One Tab stop with Left/Right inside it ([`crate::widgets::nav_group`], the
/// rule the colour swatches and the designer's item list follow), and the arrows
/// *apply* as they move: every option is visible instantly and reversible by the
/// next press, so there is nothing to confirm. The step clamps rather than wraps
/// — this is a selection, and rolling from the largest back to the smallest would
/// only be a surprise.
fn scale_picker(
    scale: RwSignal<theme::UiScale>,
    ring: crate::widgets::FocusRing,
    tabindex: u32,
) -> AnyView {
    use crate::widgets::{NavAxis, list_step, nav_group};

    /// A segment's corner, inside the track's own [`CONTROL_RADIUS`](crate::widgets::CONTROL_RADIUS).
    /// Not scaled: a radius is a shape, and the 3px inset it sits in isn't the
    /// kind of measurement that grows with the type (the same call the hairlines
    /// make).
    const SEGMENT_RADIUS: f64 = 4.0;

    // One segment. The four share the track's width equally (`flex_grow` with a
    // zero basis — an `auto` basis would size each to its own label and leave
    // "Normal" wider than "Huge").
    //
    // **Every segment carries the 1px border, transparent when it isn't the
    // selected one.** A border is layout in floem (it comes out of the content
    // box), so colouring one in on selection alone would shift that segment's
    // label by a pixel as the selection moved along the row. This is the flat
    // equivalent of the design's `box-shadow: inset 0 0 0 1px`.
    let segments = theme::UiScale::ALL.map(|k| {
        text(k.label())
            .on_click_stop(move |_| scale.set(k))
            .style(move |s| {
                let on = scale.get() == k;
                let s = s
                    .flex_grow(1.0_f32)
                    .flex_basis(0.0)
                    .min_width(0.0)
                    .height_full()
                    .items_center()
                    .justify_center()
                    .border(1.0)
                    .border_radius(SEGMENT_RADIUS)
                    .font_size(theme::font_body());
                if on {
                    s.background(theme::control_bg())
                        .border_color(theme::accent())
                        .color(theme::accent())
                } else {
                    s.border_color(floem::peniko::Color::TRANSPARENT)
                        .color(theme::text_dim())
                        .hover(|s| s.color(theme::text()))
                }
            })
            .into_any()
    });

    // The track wears the field chrome the dropdowns above it wear — same height,
    // same surface, same border and radius — so the Appearance group reads as one
    // set of controls rather than a picker and two dropdowns.
    let row = h_stack_from_iter(segments).style(|s| {
        s.flex_row()
            .width_full()
            .height(theme::scaled(32.0))
            .items_center()
            .gap(theme::scaled(2.0))
            .padding(theme::scaled(3.0))
            .background(theme::bg_editor())
            .border(1.0)
            .border_color(theme::field_border())
            .border_radius(crate::widgets::CONTROL_RADIUS)
    });

    nav_group(row, ring, tabindex, NavAxis::Horizontal, move |delta| {
        let all = theme::UiScale::ALL;
        let cur = all
            .iter()
            .position(|k| *k == scale.get_untracked())
            .unwrap_or(0);
        if let Some(next) = list_step(all.len(), cur, delta) {
            scale.set(all[next]);
        }
    })
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
            // The track's radius is half its height — the same arithmetic as the
            // colour swatches in `connection_form`, and wrong in the same way when
            // it is frozen: at 160% an 18px track is 29px tall and a literal 9
            // leaves every Settings switch a rounded rectangle instead of a
            // stadium.
            let track_h = theme::scaled(18.0);
            let s = s
                .width(theme::scaled(36.0))
                .height(track_h)
                // Floem dresses every `ToggleButtonClass` in a 1px `#8c8c8c`
                // border (`theme::default_theme`'s `border_style`), which reads
                // as a grey outline around the dark off track and vanishes under
                // the lit on one.
                //
                // **This is also where the reported magenta ring came from.** The
                // same theme's `focus_style` carries
                // `.focus(|_| border_color(#724a8c))` — a *border colour*, on plain
                // `.focus` rather than `.focus_visible`, which is exactly why it
                // appeared on a mouse click and never on a Tab. Zeroing the width
                // answers both at once, and is free where a transparent colour
                // would not have been: the handle is placed from `layout.size`, so
                // the border costs no geometry either way, and a border that
                // cannot paint is one floem's `.focus` rule cannot colour.
                .border(0.0)
                .border_radius((track_h / 2.0) as f32)
                .flex_shrink(0.0_f32)
                .set(ToggleButtonInset, PxPct::Pct(12.0))
                .set(ToggleButtonCircleRad, PxPct::Pct(72.0))
                .set(Foreground, Some(Brush::Solid(handle)))
                .background(bg)
                .hover(move |s| s.background(bg_hover))
                .active(move |s| s.background(bg_hover))
                // Two further defaults from the same class, both of which must be
                // answered whether or not this switch is wearing a ring:
                //
                // - `.focus(|s| s.hover(|s| s.background(#eae6ec)))` — a near-white
                //   track for focused-and-hovered, which washes out both states.
                //   Restated here as this switch's own hover fill.
                // - `.focus_visible(|s| s.outline(3.0))` over an `outline_color` of
                //   `#d5d0d8`. Floem gates `FocusVisible` on `keyboard_navigation`,
                //   which only its own Tab traversal sets — but that flag latches
                //   *globally* once floem has stepped anywhere in the window, so it
                //   is suppressed here exactly as the fields do it.
                .focus(move |s| s.hover(move |s| s.background(bg_hover)))
                .focus_visible(|s| s.outline(0.0));
            // The one form control that follows the *buttons* rather than the
            // fields: a switch reached by Tab has nothing else to say it is the
            // thing Space will flip, but on a click the ring is noise, so it is
            // gated on `keyboard_nav` exactly as `button_focus_ring` is. Only the
            // ring is gated — the three lines above are unconditional, because
            // floem's defaults do not stop applying when the pointer is what moved
            // focus. Leaving them to this branch is what put a magenta border and
            // a white track under the mouse.
            //
            // An **outline** now, not the lit border this used to wear. The
            // border was a workaround for a cost that has gone: it was declared
            // transparent at rest so that lighting it on focus wouldn't grow the
            // switch by 2px and nudge its row. An outline is painted outside the
            // box and costs no layout at all, so the ring can be the buttons'
            // — same colour, same width — instead of a dim border that was the
            // most this could afford.
            toggle_focus_ring(s, bg_hover)
        })
}

/// The switch's focus ring, gated on [`crate::widgets::keyboard_nav`] — a
/// function of its own so the selector-ordering rule below can be asserted
/// without a window.
///
/// **Both selectors, set to the same outline.** Floem applies `Focus` first and
/// then `FocusVisible` (`style.rs`'s `apply_interact_state`), so the narrower of
/// the two wins whenever `app_state.keyboard_navigation` has latched — and it
/// latches globally the first time floem's own Tab traversal runs anywhere in the
/// window, which one Tab in the workspace does. With only `.focus` set, the
/// unconditional `.focus_visible(outline(0.0))` above — which is there to answer
/// floem's own 3px magenta default — erased this ring completely from that moment
/// on, and the switch showed no focus indication at all. `widgets::button_focus_ring`
/// sets the pair for exactly this reason; this site had lost its half.
pub(crate) fn toggle_focus_ring(
    s: floem::style::Style,
    bg_hover: floem::peniko::Color,
) -> floem::style::Style {
    if !crate::widgets::keyboard_nav().get() {
        return s;
    }
    let ring = move |s: floem::style::Style| {
        s.outline(2.0)
            .outline_color(theme::accent())
            .hover(move |s| s.background(bg_hover))
    };
    s.focus(ring).focus_visible(ring)
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

/// The settings modals' `<select>`: a value from a fixed list, bound to the
/// signal that value lives in, in a modal's Tab order.
///
/// The box, the menu and the keyboard are all [`in_ring_picker`]'s — this half
/// only knows what the options are and what picking one does. `label` may return
/// an owned `String`, so it can be *computed from the value* rather than looked
/// up: three of these label functions used to be `match`es with a `_` arm handing
/// back the **default's** label, so any value outside the option list read as
/// (say) "200,000" while the app used something else, and no row was tinted — the
/// one modal whose job is to report settings accurately, asserting a setting that
/// wasn't in effect. No such value is reachable today; the trap was the next
/// ordinary edit to a list (adding a 24 px size, dropping the 1M row limit),
/// which would have mislabelled the setting for every user who held the removed
/// value.
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
    let entries = move || {
        options
            .clone()
            .into_iter()
            .map(|item| {
                let text: String = label(item).into();
                let pick = move || active.set(item);
                // The value already in effect is **tinted**, not given a filled
                // background: the tint is this menu system's vocabulary for "you
                // are holding this one" (`cell_editors::pick_entries` says the
                // same thing the same way), and the fill is the *cursor*, which
                // moves with the arrows. Two states, two treatments — the floem
                // dropdown had to invent a dimmer fill to keep them apart.
                match active.get_untracked() == item {
                    true => MenuEntry::action_colored(text, theme::accent, pick),
                    false => MenuEntry::action(text, pick),
                }
            })
            .collect()
    };
    in_ring_picker(
        picker_box_content(move || label(active.get()).into()),
        entries,
        ring,
        tabindex,
    )
}

/// The closed box's content: the current value on the left, a chevron on the
/// right, both following the value reactively.
///
/// A `dyn_container` keyed on the string rather than a captured one: the label
/// is computed from a signal and every one of these boxes has to repaint when
/// its setting changes from anywhere else — the theme modal's own preview does
/// exactly that.
pub(crate) fn picker_box_content(label: impl Fn() -> String + 'static) -> AnyView {
    dyn_container(label, move |cur| {
        h_stack((
            text(cur).style(|s| {
                s.color(theme::text())
                    .font_size(theme::font_body())
                    .text_ellipsis()
                    .min_width(0.0)
            }),
            empty().style(|s| s.flex_grow(1.0_f32)),
            icons::icon(icons::CHEVRON_DOWN, 16.0)
                .style(|s| s.color(theme::text_dim()).flex_shrink(0.0_f32)),
        ))
        .style(|s| s.items_center().width_full().gap(theme::scaled(8.0)))
        .into_any()
    })
    .style(|s| s.width_full().min_width(0.0))
    .into_any()
}

/// A `<select>`-shaped box in a modal's Tab order that drops **the app's own
/// popup menu**, not a floem [`Dropdown`](floem::views::dropdown::Dropdown).
///
/// **Why it is not floem's.** `scale_picker`'s doc states the bug in full: floem
/// 0.2 nudges an overlay that would overflow the window back inside during
/// `paint` **only**, so layout and hit-testing stay where they were and the row
/// that lights up under the pointer is not the row the pointer is over. Every
/// dropdown in the app had that latent, and the interface scale is what makes it
/// reachable — a `modal_h(620)` editor is 992px tall at 160%, so a field near its
/// foot is within a popup's height of the window edge on any 1080p screen. No
/// bounded fix exists inside floem's control: the popup is an `add_overlay` at a
/// point the widget computes, and the nudge is `cx.offset((-x, -y))` against
/// `parent_size - 5.0`, so a `max_height` cap would have to be ≤ 5px to
/// guarantee no nudge. The app already owns a popup that places itself honestly
/// — [`crate::widgets::menu_panel`] flips at an edge from a *predicted* height
/// ([`crate::widgets::menu_panel_height`]) and lays out where it draws — so this
/// routes every picker through that instead. Removing the class, not the
/// instances.
///
/// **The box owns opening; the panel owns everything after.** A press (or Enter,
/// Space, Up, Down — the reflexes a `<select>` trains) calls
/// [`crate::widgets::open_picker`], which is also what closes it again on a
/// second press, and the panel then takes the keyboard itself: arrows walk,
/// Enter picks, Escape peels one layer. That is four floem work-arounds deleted
/// rather than moved — `disable_default_event`, the mirrored `on_open`, the
/// `show_list` signal and the single `on_accept` slot were all about floem's
/// dropdown answering half the keyboard on `KeyUp`, and none of it has anything
/// to answer here.
///
/// **Where focus goes when the menu closes** is still this box's problem, and it
/// is handed over rather than remembered: [`crate::widgets::set_menu_return`] is
/// taken by the panel as it builds, and calls `ring.focus_at(tabindex)` —
/// resolved when it fires, not captured, because an accept can rebuild the very
/// box it came from (the PG trigger editor's Function picker sits in a container
/// keyed on the signal its own accept writes) and floem's focus request has no
/// existence check. Set only when the opener was reached from the keyboard: after
/// a click, moving focus here would take the arrows away from whatever had them.
pub(crate) fn in_ring_picker(
    main: AnyView,
    entries: impl Fn() -> Vec<MenuEntry> + 'static,
    ring: crate::widgets::FocusRing,
    tabindex: u32,
) -> impl IntoView {
    use floem::event::{Event, EventListener, EventPropagation};
    use floem::keyboard::{Key, NamedKey};

    // The box's own id, so the menu opens under the box rather than at the
    // pointer — which is where it was left when the control was reached by Tab.
    // Filled once the view below exists.
    let anchor_id: RwSignal<Option<floem::ViewId>> = RwSignal::new(None);
    // See `widgets::close_picker`: where our menu is standing, held as a plain
    // value so the cleanup below reads nothing reactive.
    let standing: Rc<std::cell::Cell<Option<crate::PopupAnchor>>> = Rc::new(Default::default());
    // One closure, shared by the press and the key: two copies would be two
    // places for the "which of the four openings did you mean" answer to drift.
    let open: Rc<dyn Fn()> = {
        let standing = standing.clone();
        let ring = ring.clone();
        Rc::new(move || {
            // `None` only outside a built workspace — a unit test holding this
            // control on its own. A box that draws and declines to open is the
            // honest answer there; a panic is not.
            let Some(ch) = crate::widgets::menu_channel() else {
                return;
            };
            if crate::widgets::keyboard_nav().get_untracked() {
                let ring = ring.clone();
                crate::widgets::set_menu_return(Rc::new(move || {
                    let ring = ring.clone();
                    // Deferred: the panel is removed during this same update
                    // pass, and a focus request into it would be undone by the
                    // removal.
                    floem::action::exec_after(std::time::Duration::ZERO, move |_| {
                        ring.focus_at(tabindex)
                    });
                }));
            }
            let id = anchor_id.get_untracked();
            // **The box's measured width, not a number a caller guessed.** The
            // panel is the same control opened out, so it lines its edges up with
            // the box — and the box's width is whatever its container gave it,
            // which is why the two used to disagree at every scale but one.
            let w = id.map(|id| id.layout_rect().width()).unwrap_or(0.0);
            standing.set(crate::widgets::open_picker(ch, id, w, entries()));
        })
    };
    let key_open = open.clone();
    let boxed = container(main)
        // **Enter and Space arrive here, not at the KeyDown handler below**, and
        // that division is load-bearing rather than incidental — see the handler.
        .on_click_stop(move |_| open())
        // Without this the workspace root's close-on-pointer-down fires first and
        // the click then reopens: down closes, up reopens, and the box never
        // toggles.
        .on_event_stop(
            EventListener::PointerDown,
            crate::widgets::menu_trigger_press,
        )
        .style(dropdown_box_style);
    anchor_id.set(Some(boxed.id()));
    // The cleanup goes *through* the ring helper: floem keeps one cleanup slot
    // per view, so chaining a second one here would replace the ring's. Without
    // it, a picker disposed with its menu still up (click the backdrop) leaves a
    // stranded `menu_panel` over the app — and a stranded panel keeps its
    // `focus_root` registered, which is how a new query tab ends up declining
    // the keyboard.
    crate::widgets::in_focus_ring_with(boxed, ring, tabindex, move || {
        if let Some(ch) = crate::widgets::menu_channel() {
            crate::widgets::close_picker(ch, standing.get());
        }
    })
    // **The arrows only — Enter and Space must not be claimed here.** Floem
    // synthesises a `Click` on the focused view for Enter and Space
    // (`context.rs`, `is_keyboard_trigger`), and it does so **before** it runs
    // this listener; worse, its listener loop is a `fold` that runs *every*
    // handler and only then reports whether any of them processed the event, so
    // returning `Stop` here cannot call the synthesised click off. Claiming Enter
    // would therefore fire `open` twice for one press — and `open_picker`
    // *toggles*, so the menu would open and shut in the same keystroke, looking
    // exactly like a control that ignores the keyboard.
    //
    // The floem `Dropdown` this replaced was safe from that by accident: its
    // handler did `open.set(true)`, which is idempotent, so the second call was a
    // no-op. Moving to a toggle is what makes the division above mandatory.
    // Arrows are not keyboard triggers, so they have no click to collide with and
    // are claimed here.
    .on_event(EventListener::KeyDown, move |e| {
        let Event::KeyDown(ke) = e else {
            return EventPropagation::Continue;
        };
        if matches!(
            ke.key.logical_key,
            Key::Named(NamedKey::ArrowDown) | Key::Named(NamedKey::ArrowUp)
        ) {
            key_open();
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
    let gutter = ui.ai.gutter;
    // The active connection's data-access level, reactively — the modal reports
    // it, the connection form owns it.
    let connections = ui.conn.connections;
    let active_conn = ui.conn.active_conn;
    // **Named**, because it is the *active connection's* level and the grid a
    // step away obeys its own result's `conn_id`. Unnamed, the line could read
    // "Let it read data" over a grid whose attach actions are refused, and the
    // user would have no way to tell which of the two was lying.
    let active_ai_data = floem::reactive::create_memo(move |_| {
        let id = active_conn.get();
        connections.with(|cs| {
            let c = cs.iter().find(|c| c.id == id);
            (
                c.map(|c| c.name.clone()).unwrap_or_default(),
                c.and_then(|c| c.ai_data).unwrap_or_default(),
            )
        })
    });
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
                |s: floem::style::Style| s.font_size(theme::font_label()).color(theme::reject_bg());
            let hint = dyn_container(
                move || cli_path.get(),
                move |path| {
                    if path.trim().is_empty() {
                        match &detected {
                            Some(p) => text(format!("Auto-detected: {}", p)).style(|s| {
                                s.font_size(theme::font_label()).color(theme::conn_ok())
                            }),
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
            let group = |s: floem::style::Style| s.flex_col().gap(theme::scaled(6.0));
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
            // **What this setting is, said out loud.** It reads as a context
            // budget — how much structure is worth spending the model's window
            // on — and *Data access*, below and per-connection, is the consent
            // control. But a budget of zero that one tool call walks around is
            // neither, so `None` withholds `list_schema` and `describe_table`
            // too; the hint says so, because a setting whose reach a user has to
            // infer is one they will infer wrongly in the safe direction or the
            // unsafe one.
            let scope_section = v_stack((
                settings_group_label("Schema context"),
                scope_dd,
                text(
                    "How much database structure rides in every message. None also withholds \
                     the schema tools, so the assistant asks you for names instead of reading \
                     the catalogue itself.",
                )
                .style(|s| {
                    s.width_full()
                        .font_size(theme::font_hint())
                        .color(theme::text_muted())
                }),
            ))
            .style(group);
            // Data access is *not* settable here: it belongs to the connection,
            // because a scratch database and a client's production server are
            // not the same risk and one global answer forces the careless
            // setting on one of them. This section states what the connection in
            // front of the user says, and where to change it — a modal that
            // silently omitted the subject would read as "there is no such
            // setting".
            let data_section = v_stack((
                settings_group_label("Data access"),
                label(move || {
                    let (name, lvl) = active_ai_data.get();
                    if name.is_empty() {
                        format!("{} — {}", lvl.label(), lvl.hint())
                    } else {
                        format!("{name}: {} — {}", lvl.label(), lvl.hint())
                    }
                })
                .style(|s| {
                    s.width_full()
                        .font_size(theme::font_hint())
                        .color(theme::text_muted())
                }),
                text("Set per connection, in the connection's settings.").style(|s| {
                    s.width_full()
                        .font_size(theme::font_hint())
                        .color(theme::text_muted())
                }),
            ))
            .style(group);
            // Presentation, and the only setting here that is: it changes
            // nothing about what is sent or how a turn runs, so it does not
            // enter `ai_settings_now()` and closing the modal after flipping it
            // leaves the live conversation alone.
            let gutter_row = focusable_toggle_row(
                "Accent rule on replies",
                "Mark Claude's replies with a coloured rule down their right edge. Off gives them \
                 the same margin on both sides.",
                gutter,
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
                data_section,
                gutter_row,
            ))
            .style(|s| {
                s.flex_col()
                    .gap(theme::scaled(25.0))
                    .padding(theme::scaled(14.0))
                    .width_full()
            });

            let panel = v_stack((
                modal_title("AI Assistant — Settings", close.clone(), root_ring.clone()),
                body,
            ))
            .on_click_stop(|_| {})
            .style(|s| {
                panel_style(s)
                    .background(theme::bg_panel())
                    .width(crate::widgets::modal_w(460.0))
            });

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
        text(desc).style(|s| s.color(theme::text()).font_size(theme::font_body())),
        empty().style(|s| s.flex_grow(1.0_f32).min_width(12.0)),
        text(keys).style(|s| {
            s.color(theme::text_muted())
                .font_size(theme::font_label())
                .font_family("IBM Plex Mono".to_string())
                .background(theme::bg_deepest())
                .padding_horiz(theme::scaled(6.0))
                .padding_vert(theme::scaled(2.0))
                .border_radius(4.0)
        }),
    ))
    .style(|s| {
        s.width_full()
            .flex_row()
            .items_center()
            .padding_vert(theme::scaled(2.0))
    })
}

/// A titled group of shortcut rows (Global / Editor / Results grid).
fn shortcut_group(title: &'static str, rows: &[(&'static str, &'static str)]) -> impl IntoView {
    let rows: Vec<_> = rows.to_vec();
    v_stack((
        text(title).style(|s| {
            s.font_size(theme::font_label())
                .color(theme::text_dim())
                .margin_bottom(theme::scaled(2.0))
        }),
        v_stack_from_iter(rows.into_iter().map(|(k, d)| shortcut_row(k, d)))
            .style(|s| s.flex_col()),
    ))
    .style(|s| s.flex_col().gap(theme::scaled(2.0)))
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

/// The statement-timeout choices, in seconds. **`0` leads and means off**,
/// which is the default and what every release before the setting did.
///
/// The shortest real option is a minute rather than a handful of seconds: this
/// cancels the statement the user asked for, and an import, a report or a
/// `CREATE INDEX` on a large table legitimately takes longer than a person's
/// patience. A timeout tight enough to kill honest work is worse than none.
const STATEMENT_TIMEOUTS: [u64; 6] = [0, 60, 300, 900, 1_800, 3_600];

/// Label a statement timeout, **computed from the value** rather than looked up
/// — the trap `row_limit_label`'s doc comment describes, and the reason none of
/// these labels has a list to fall off the end of.
///
/// The wording itself lives in `core::persist` because the app's
/// timed-out-statement message quotes the same value back, and two spellings of
/// "15 minutes" is how the dropdown and the error come to disagree.
fn statement_timeout_label(secs: u64) -> String {
    schemaic_core::persist::statement_timeout_label(secs)
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
/// Where the app's log lives, as the Settings row's hint.
///
/// The **full path**, not "your config directory": the row exists because the
/// log was undiscoverable, and a description of the location is not the
/// location. It stays readable even where the button cannot work — a headless
/// or sandboxed machine with no file manager — which is the case the fallback
/// string is for.
/// **The file that is actually open, not a path derived from one that exists.**
/// This row's whole purpose is to answer "where is the log", so it has to answer
/// about the writer: a config directory can exist and be unwritable — a
/// locked-down profile, a read-only roaming mount, an ACL the user lost — and
/// `logging::init` then degrades to stdout with a warning that goes to a console
/// a release build does not have. The row went on naming a file nobody had
/// written, and a crash report gathered from it comes back empty with nobody able
/// to say why.
fn log_hint(log: Option<&std::path::Path>) -> String {
    match log {
        Some(p) => format!(
            "Diagnostics, including crashes, are written to {}.",
            p.display()
        ),
        None => "No log file could be opened on this machine, so nothing is being written to one."
            .to_string(),
    }
}

/// The Settings row that points at the log file, and opens the folder holding
/// it.
///
/// Reveals the *directory* rather than opening the file: the log has no natural
/// handler on Windows, `schemaic.log.1` is a second file worth reaching, and the
/// same folder is where `tabs.json` and `connections.json` live — everything
/// anyone would be asked for.
fn log_row(open: Rc<dyn Fn()>, ring: crate::widgets::FocusRing, tabindex: u32) -> impl IntoView {
    // Two different questions, deliberately: the *hint* asks whether a log is
    // being written (`persist::active_log`, recorded by whoever opened it), the
    // *button* asks whether there is a folder to reveal. A directory that exists
    // but could not be written to answers no to the first and yes to the second.
    let log = schemaic_core::persist::active_log();
    let enabled = schemaic_core::persist::config_dir().is_some();
    h_stack((
        v_stack((
            text("Log file").style(|s| s.color(theme::text()).font_size(theme::font_label())),
            form_hint(log_hint(log.as_deref())),
        ))
        .style(|s| {
            s.flex_col()
                .gap(theme::scaled(2.0))
                .flex_grow(1.0_f32)
                .min_width(0.0)
        }),
        action_button(
            "Open folder",
            ActionKind::Quiet,
            enabled,
            ring,
            tabindex,
            move || open(),
        ),
    ))
    .style(|s| s.items_center().width_full().gap(theme::scaled(10.0)))
}

fn settings_section_header(t: &'static str) -> impl IntoView {
    text(t).style(|s| {
        s.font_size(theme::font_body())
            .font_bold()
            .color(theme::text())
            .margin_bottom(theme::scaled(2.0))
    })
}

pub(crate) fn theme_settings_overlay(ui: Ui) -> impl IntoView {
    let open = ui.layout.theme_settings_open;
    let ui_theme = ui.layout.ui_theme;
    let editor_theme = ui.layout.editor_theme;
    let ui_scale = ui.layout.ui_scale;
    let editor_font = ui.layout.editor_font;
    let row_limit = ui.layout.row_limit;
    let statement_timeout = ui.layout.statement_timeout;
    let confirm_writes = ui.layout.confirm_writes;
    let live_validate = ui.layout.live_validate;
    let restore_tabs = ui.layout.restore_tabs;
    let open_config_dir = ui.open_config_dir.clone();

    dyn_container(
        move || open.get(),
        move |is_open| {
            if !is_open {
                return empty().into_any();
            }
            let close: Rc<dyn Fn()> = Rc::new(move || open.set(false));

            let ctrl = |s: floem::style::Style| s.flex_col().gap(theme::scaled(6.0));

            // The modal's Tab order: one group per section, spaced by 10 within
            // it and by 100 between them — see `FocusRing`.
            //
            // **The number follows the layout, not the order the control was
            // added.** A control appended by number rather than inserted at its
            // place walks the user backwards through the form: the statement
            // timeout was written last and numbered 230 while sitting *second*
            // in its group, so Tab went row limit → confirm → validate → back up
            // to timeout.
            let ring = crate::widgets::FocusRing::new();

            // General group.
            let restore_row = focusable_toggle_row(
                "Restore tabs on startup",
                "Reopen the query tabs from your last session when the app starts.",
                restore_tabs,
                ring.clone(),
                10,
            );
            let general_group = v_stack((
                settings_section_header("General"),
                restore_row,
                log_row(open_config_dir.clone(), ring.clone(), 20),
            ))
            .style(|s| s.flex_col().gap(theme::scaled(16.0)));

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
                .style(|s| s.flex_col().gap(theme::scaled(16.0)));

            // Query group.
            let row_dd =
                focusable_dropdown(row_limit, ROW_LIMITS, row_limit_label, ring.clone(), 200);
            let row_section =
                v_stack((settings_group_label("Default row limit"), row_dd)).style(ctrl);
            // 210, because `timeout_section` is the row *below* the row limit in
            // the group below — the two toggles come after it.
            let timeout_dd = focusable_dropdown(
                statement_timeout,
                STATEMENT_TIMEOUTS,
                statement_timeout_label,
                ring.clone(),
                210,
            );
            let confirm_row = focusable_toggle_row(
                "Confirm before running writes",
                "Ask before executing any statement that modifies data or schema.",
                confirm_writes,
                ring.clone(),
                220,
            );
            let validate_row = focusable_toggle_row(
                "Live database validation",
                "Check the statement under the cursor against the database as you type \
                 (a non-executing PREPARE) to surface exact errors. Adds a DB round-trip \
                 on each pause.",
                live_validate,
                ring.clone(),
                230,
            );
            let timeout_section = v_stack((
                settings_group_label("Statement timeout"),
                timeout_dd,
                form_hint(
                    "Cancel a statement that runs longer than this. Off by default — \
                     an import or a report can legitimately take a while.",
                ),
            ))
            .style(ctrl);
            let query_group = v_stack((
                settings_section_header("Query"),
                row_section,
                timeout_section,
                confirm_row,
                validate_row,
            ))
            .style(|s| s.flex_col().gap(theme::scaled(16.0)));

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
            let scale_dd = scale_picker(ui_scale, ring.clone(), 320);
            // Kept for the root, which answers Tab by entering the ring.
            let root_ring = ring;
            let ui_section = v_stack((settings_group_label("Interface theme"), ui_dd)).style(ctrl);
            let editor_section =
                v_stack((settings_group_label("Editor theme"), editor_dd)).style(ctrl);
            // No hint under this one, unlike the settings that carry a
            // consequence: four segments named by their percentage, applying the
            // instant they are pressed, explain themselves better than a line of
            // prose restating them.
            let scale_section =
                v_stack((settings_group_label("Interface scale"), scale_dd)).style(ctrl);
            let theme_group = v_stack((
                settings_section_header("Appearance"),
                ui_section,
                editor_section,
                scale_section,
            ))
            .style(|s| s.flex_col().gap(theme::scaled(16.0)));

            let body =
                v_stack((general_group, editor_group, query_group, theme_group)).style(|s| {
                    s.flex_col()
                        .gap(theme::scaled(28.0))
                        .padding(theme::scaled(14.0))
                        .width_full()
                });
            // Scroll so the taller grouped modal never overflows the window.
            let body = autohide(scroll(body)).style(|s| {
                s.width_full()
                    .max_height(crate::widgets::modal_body_h(560.0))
            });

            let panel = v_stack((
                modal_title("Settings", close.clone(), root_ring.clone()),
                body,
            ))
            .on_click_stop(|_| {})
            .style(|s| {
                panel_style(s)
                    .background(theme::bg_panel())
                    .width(crate::widgets::modal_w(420.0))
            });

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
            .style(|s| {
                s.flex_col()
                    .gap(theme::scaled(25.0))
                    .padding(theme::scaled(14.0))
                    .width_full()
            });
            // Scroll the body so the modal never overflows the window.
            let body = autohide(scroll(body)).style(|s| {
                s.width_full()
                    .max_height(crate::widgets::modal_body_h(560.0))
            });

            // A ring for one button — the ✕, this modal's only control. Without
            // one the root has no Tab handler and Tab falls through to floem's
            // whole-window traversal, out of the modal into the workspace.
            let ring = crate::widgets::FocusRing::new();
            let panel = v_stack((modal_title("Shortcuts", close.clone(), ring.clone()), body))
                .on_click_stop(|_| {})
                .style(|s| {
                    panel_style(s)
                        .background(theme::bg_panel())
                        .width(crate::widgets::modal_w(420.0))
                });

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
        EDITOR_FONT_SIZES, ROW_LIMITS, STATEMENT_TIMEOUTS, editor_font_label, log_hint,
        row_limit_label, statement_timeout_label, term_font_label, thousands,
    };
    use crate::consts::TERM_FONT_SIZES;

    /// The row's whole purpose is to *say where the log is*, so the hint must
    /// carry the full path and the file's name. "In your config directory" is
    /// the answer the user already didn't have.
    #[test]
    fn the_log_hint_names_the_file_and_its_full_path() {
        let hint = log_hint(Some(std::path::Path::new(
            "/home/x/.config/schemaic/schemaic.log",
        )));
        assert!(hint.contains("/home/x/.config/schemaic"), "{hint}");
        assert!(hint.contains("schemaic.log"), "{hint}");
    }

    /// **No writer, no promise.** The input used to be the config *directory*,
    /// which is a question about an environment variable — so on a machine whose
    /// config directory exists but is not writable (a locked-down profile, a
    /// read-only roaming mount, a lost ACL) the row named a file nobody had
    /// written, and a crash report gathered from it comes back empty. The input is
    /// now the log the process actually opened, so this arm covers both reasons
    /// there might not be one and names neither.
    #[test]
    fn the_log_hint_says_nothing_is_logged_when_no_file_was_opened() {
        let hint = log_hint(None);
        assert!(!hint.contains("schemaic.log"), "{hint}");
        assert!(
            hint.to_lowercase().contains("nothing is being written"),
            "{hint}"
        );
        // It must not name a cause it cannot know: an unwritable directory and a
        // missing one produce the same `None`.
        assert!(
            !hint.to_lowercase().contains("no config directory"),
            "the reason is not this function's to assert: {hint}"
        );
    }

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
        // Every timeout option reads distinctly, so no two rows of the dropdown
        // are the same words — which is the failure mode a shared `_` arm has.
        let labels: Vec<String> = STATEMENT_TIMEOUTS
            .iter()
            .map(|&s| statement_timeout_label(s))
            .collect();
        let mut seen = labels.clone();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), labels.len(), "duplicate labels in {labels:?}");
    }

    /// The list leads with `0`, and `0` is what makes the feature opt-in. An
    /// option list without it leaves a user who turned the timeout on with no
    /// row that turns it back off.
    #[test]
    fn the_timeout_list_offers_turning_it_off() {
        assert_eq!(STATEMENT_TIMEOUTS[0], 0);
        assert_eq!(statement_timeout_label(0), "No timeout");
    }

    /// The dropdown's label is `core::persist`'s, deliberately: the message a
    /// timed-out statement leaves in the results pane quotes the same words, so
    /// a local reimplementation here is how the two come to disagree.
    #[test]
    fn the_timeout_label_is_the_shared_one() {
        for &s in &STATEMENT_TIMEOUTS {
            assert_eq!(
                statement_timeout_label(s),
                schemaic_core::persist::statement_timeout_label(s)
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

/// **Nothing in this crate may build a floem `Dropdown` again.**
///
/// `in_ring_picker` removed a *class* of bug, not twenty-four instances of it:
/// floem 0.2's overlay nudge is paint-only (`scale_picker` states it in full), so
/// every popup floem places for us answers the pointer where its rows used to be
/// as soon as it nears a window edge — which the interface scale makes reachable
/// in any modal taller than about half the screen. The fix is only durable if the
/// next `<select>`-shaped control is built on the app's own menu, and the thing
/// that makes it durable is this, not the prose.
///
/// A source scan rather than a type-level guard, because there is no type to
/// remove: `floem::views::dropdown` is a public module of a dependency and
/// nothing stops a future call to it but a reader who knows. `doc_coverage.rs`
/// and `modal_backdrop_gate` are the same shape for the same reason.
///
/// **Comments are stripped**, or this gate would fail on the two doc paragraphs
/// that name the type in order to say not to use it — including
/// `in_ring_picker`'s own.
#[cfg(test)]
mod no_floem_dropdown_gate {
    /// Every spelling that reaches floem's dropdown: the path, the `use` that
    /// shortens it, and the constructor a shortened one calls.
    const SPELLINGS: &[&str] = &[
        "views::dropdown",
        "dropdown::Dropdown",
        "Dropdown::custom",
        "Dropdown::new",
    ];

    #[test]
    fn no_view_in_the_app_builds_a_floem_dropdown() {
        // `crate::source_gate::crate_sources`, for two reasons the old inline
        // scan got wrong. It read `schemaic-ui/src` alone, while the invariant is
        // stated app-wide ("every `<select>` in the app") and `schemaic-app`
        // builds views too — so a `Dropdown` added there passed the whole suite.
        // And it cut each file at the **first** `#[cfg(test)]`, which in
        // `widgets.rs` is an inline test-only `fn` at line 929: 87% of the file,
        // including the entire replacement menu system this gate exists to
        // protect, was never scanned. A planted `Dropdown` at line 2000 passed.
        let mut offenders: Vec<String> = Vec::new();
        for (name, code) in crate::source_gate::crate_sources() {
            for (i, line) in code.lines().enumerate() {
                if let Some(hit) = SPELLINGS.iter().find(|p| line.contains(**p)) {
                    offenders.push(format!(
                        "{name}:{} reaches floem's dropdown (`{hit}`) — use \
                         `settings::in_ring_picker`, which drops the app's own \
                         menu and therefore places itself honestly at a window \
                         edge",
                        i + 1
                    ));
                }
            }
        }
        assert!(offenders.is_empty(), "{}", offenders.join("\n"));
    }
}
