//! The Settings modal — every setting, a section to a page — and the keyboard
//! `Shortcuts` reference modal, plus their shared controls: a labelled toggle
//! row, a generic dropdown, and the themed switch. Each control binds straight to
//! its persisted signal.

use std::rc::Rc;

use floem::AnyView;
use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;

use crate::consts::{TERM_FONT_SIZES, chat_pad_h};
use crate::widgets::{
    ActionKind, FocusRing, MenuEntry, action_button, action_button_dyn, action_gap, autohide,
    focus_root_with_ring, form_hint, form_hint_style, form_label_style, form_section_header,
    modal_title, nothing, panel_style,
};
use crate::{
    AiActions, AiEffort, AiUi, ConnUi, FieldCfg, LayoutUi, SchemaScope, TermActions, TermCursor,
    TermUi, edit_field, icons, theme,
};
use schemaic_ai::harness::Agent;
use schemaic_core::cli_install::InstallState;

/// The Terminal section: shell + font size + cursor style dropdowns, and
/// copy-on-select / blink toggles. Every control binds straight to its
/// persisted signal — picking a shell respawns the terminal; the rest apply
/// live.
fn terminal_section(term: TermUi, actions: Rc<TermActions>, ring: FocusRing) -> AnyView {
    let shells = term.shells;
    let selected = term.shell_selected;
    let apply = actions.apply_shell.clone();
    let font_size = term.font_size;
    let copy_on_select = term.copy_on_select;
    let cursor_style = term.cursor_style;
    let cursor_blink = term.cursor_blink;
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
    let cursor_section = v_stack((settings_group_label("Cursor style"), cursor_dd)).style(group);
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
        ring,
        50,
    );
    section_page((
        form_section_header(SettingsSection::Terminal.label()),
        shell_section,
        font_section,
        cursor_section,
        copy_row,
        blink_row,
    ))
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
/// There is no un-focusable *live* variant, and [`themed_toggle`] is private so
/// there can't be: a switch nobody can Tab to is a control left out of the
/// modal's keyboard order by accident, and the two modals that had one — Query
/// plan and Live monitor — now carry rings of their own. The one switch outside
/// the ring is [`focusable_toggle_row_when`]'s greyed one, which nobody can flip
/// either.
pub(crate) fn focusable_toggle_row(
    title: &'static str,
    hint: &'static str,
    sig: RwSignal<bool>,
    ring: crate::widgets::FocusRing,
    tabindex: u32,
) -> impl IntoView {
    toggle_row_layout(
        title,
        hint,
        theme::text,
        focusable_toggle(sig, ring, tabindex),
    )
}

/// A [`focusable_toggle_row`] that only means something while `enabled` holds
/// — a setting that refines another, like *Predictions from past statements*
/// under *Inline predictions*. While it does not, the row is **greyed out**:
/// its title dims, and its switch, still showing its value faintly, is inert
/// to the pointer and **out of the Tab order**, the way a disabled
/// [`crate::widgets::in_ring_button`] is. The switch is rebuilt on `enabled`,
/// as Settings' Install and Remove buttons are, so the ring gains and loses
/// it with the state. Its value is kept either way.
pub(crate) fn focusable_toggle_row_when(
    title: &'static str,
    hint: &'static str,
    sig: RwSignal<bool>,
    enabled: RwSignal<bool>,
    ring: crate::widgets::FocusRing,
    tabindex: u32,
) -> impl IntoView {
    let switch = dyn_container(
        move || enabled.get(),
        move |on| match on {
            true => focusable_toggle(sig, ring.clone(), tabindex).into_any(),
            false => themed_toggle(sig, true).into_any(),
        },
    );
    let title_color = move || match enabled.get() {
        true => theme::text(),
        false => theme::text_dim(),
    };
    toggle_row_layout(title, hint, title_color, switch)
}

/// The layout every toggle row shares: title and hint on the left, `switch`
/// on the right.
fn toggle_row_layout(
    title: &'static str,
    hint: &'static str,
    title_color: impl Fn() -> floem::peniko::Color + 'static,
    switch: impl IntoView + 'static,
) -> impl IntoView {
    h_stack((
        // `flex_grow(1) + min_width(0)`: take the space left of the switch and be
        // allowed to shrink below the text's natural width, so a long hint wraps
        // instead of pushing the toggle past the panel edge.
        // The switch's primary label, so `theme::text()` rather than a caption's
        // colour: it is the thing being toggled, not a caption above it.
        v_stack((
            text(title).style(move |s| s.color(title_color()).font_size(theme::font_label())),
            form_hint(hint),
        ))
        .style(|s| {
            s.flex_col()
                .gap(theme::scaled(2.0))
                .flex_grow(1.0_f32)
                .min_width(0.0)
        }),
        switch,
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

// A small dim group heading over a setting's control, on any Settings page.
fn settings_group_label(t: impl Into<String>) -> impl IntoView {
    text(t.into()).style(form_label_style)
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
            // Guarded, like `focusable_dropdown`'s `pick` — and this control
            // needed it more, being the one a no-op click is *cheapest* to make:
            // the four segments are always on screen, and clicking the one
            // already selected re-fired `theme::set_ui_scale`, which rebuilds
            // the whole scaled palette, bumps `ui_generation` — rebuilding every
            // view in the window — and writes `ui_state.json` synchronously.
            // `RwSignal::set` to the same value still notifies every dependent.
            .on_click_stop(move |_| {
                if scale.get_untracked() != k {
                    scale.set(k);
                }
            })
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

/// How strongly an inert switch is drawn: its colours at this alpha, so its
/// value still reads, faintly, under a row that is greyed out.
const INERT_TOGGLE_ALPHA: f32 = 0.4;

// A dark-theme switch. Track + handle colours are driven by on/off state; the
// track brightens on hover, and the press (active) state is neutralised to match
// hover so there's no distracting flash on click. `inert`: drawn faded, with no
// hover, and disabled — a click flips nothing (`focusable_toggle_row_when`).
fn themed_toggle(sig: RwSignal<bool>, inert: bool) -> impl IntoView {
    use floem::peniko::Brush;
    use floem::style::Foreground;
    use floem::unit::PxPct;
    use floem::views::{ToggleButtonCircleRad, ToggleButtonInset};
    floem::views::toggle_button(move || sig.get())
        .on_toggle(move |v| {
            if !inert {
                sig.set(v)
            }
        })
        .disabled(move || inert)
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
            let (bg, bg_hover, handle) = match inert {
                false => (bg, bg_hover, handle),
                true => (
                    bg.multiply_alpha(INERT_TOGGLE_ALPHA),
                    bg.multiply_alpha(INERT_TOGGLE_ALPHA),
                    handle.multiply_alpha(INERT_TOGGLE_ALPHA),
                ),
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
    crate::widgets::in_focus_ring(themed_toggle(sig, false), ring, tabindex).on_event(
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
                // Guarded, though this `active` is the picker's own value and
                // not the tab selection: the term is free, and it keeps
                // `no_bare_active_set_gate` a rule with no exceptions rather
                // than one with a list.
                let pick = move || {
                    if active.get_untracked() != item {
                        active.set(item)
                    }
                };
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
                // Deferred: the panel is removed during this same update pass,
                // and a focus request into it would be undone by the removal.
                // Claiming too, so it does not race the panel's hand-back —
                // both are `exec_after(ZERO)` and floem's timer order is a
                // `HashMap`'s. See `widgets::menu_return`.
                crate::widgets::set_menu_return(crate::widgets::menu_return(move || {
                    ring.focus_at(tabindex)
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

/// The AI assistant section: harness + CLI path override + model + effort and
/// the rest.
///
/// **Changes commit as the page goes away** — a switch to another section, or
/// Settings closing — through `AiActions::apply`, which restarts the session
/// only where a session-affecting setting really changed and persists. As the
/// AI modal's own close did while it was a modal; a section's page is torn
/// down at exactly the two moments that close stood for. Deferred a tick, as
/// any write from a disposal is ([`crate::widgets`]'s scope rule): the page's
/// scope is mid-dispose when its cleanup runs.
fn ai_section(ai: AiUi, conn: ConnUi, actions: Rc<AiActions>, ring: FocusRing) -> AnyView {
    let harness = ai.harness;
    let enabled = ai.enabled;
    let unknown_key = ai.unknown_key;
    let cli_path = ai.cli_path;
    let model = ai.model;
    let effort = ai.effort;
    let instructions = ai.instructions;
    let scope = ai.schema_scope;
    let gutter = ai.gutter;
    // The active connection's data-access level, reactively — the modal reports
    // it, the connection form owns it.
    let connections = conn.connections;
    let active_conn = conn.active_conn;
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
    let apply = actions.apply.clone();
    let detect_path = actions.detect_path.clone();
    let constraint_notice = actions.constraint_notice.clone();
    let cli_ok = actions.cli_ok.clone();

    // **The choice is an `Agent`; the app holds it as two signals** —
    // `enabled`, and the `harness` an assistant that is on drives — so
    // everything below that takes a `Harness` goes on taking one. This
    // signal is the dropdown's view of the pair (`Agent::of`), written
    // back on a pick. What a pick writes is `Agent::apply`'s answer,
    // pure and tested in `schemaic-ai` — the decision used to live
    // here and in an app effect, and the bug sat between the two.
    let agent = RwSignal::new(Agent::of(enabled.get_untracked(), harness.get_untracked()));
    floem::reactive::create_effect(move |prev: Option<()>| {
        let picked = agent.get();
        if prev.is_none() {
            return;
        }
        let pick = picked.apply(enabled.get_untracked(), harness.get_untracked());
        if pick.drop_unknown_key {
            unknown_key.set(None);
        }
        if let Some(h) = pick.harness {
            harness.set(h);
        }
        enabled.set(pick.enabled);
    });
    // `Agent` is `Copy` and `Agent::label` is a plain
    // `fn(Agent) -> &'static str`, so it satisfies this widget's bounds
    // directly — no wrapper, and one `Agent` shared with the rest of
    // the workspace rather than a UI-side mirror of it.
    let agent_dd = focusable_dropdown(agent, Agent::ALL, Agent::label, ring.clone(), 5);
    // Each group is a label + its controls (6px gap); groups are spaced
    // a page's 16 apart, as every other section's are.
    let group = |s: floem::style::Style| s.flex_col().gap(theme::scaled(6.0));
    // **What this harness actually gives you, where the choice is made.**
    // `Constraint::notice` returns `None` on the strongest grade, so the
    // Claude case shows nothing and the two that matter — a session whose
    // built-in tools can still read the machine, or one we could not
    // establish any restriction for — say so here rather than nowhere.
    // Keyed on the harness *and* the path, because both change which
    // binary was probed.
    let constraint_notice = constraint_notice.clone();
    let notice_ok = cli_ok.clone();
    // Held as a memo rather than computed inside the container, because
    // the *style* has to ask the same question the child does — see
    // below.
    //
    // **A path that is not yet a file is not probed.** `constraint_notice`
    // resolved the binary and ran `--help` on it, synchronously, on this
    // thread — and this memo tracks the *live* field, so typing a path
    // asked a different question on every keystroke, one per prefix,
    // each an unresolvable path and each a spawn. The comment on the
    // app's closure said the per-(harness, path) cache made this cheap;
    // the path being part of the key is exactly what made every
    // keystroke a miss. There is nothing to say about a binary that is
    // not there anyway — the red "File doesn't exist." hint below is the
    // whole answer — so the half-typed states ask nothing at all, and
    // the two that resolve (empty → auto-detect, or a complete path) are
    // the stable keys the cache was built for. The rule is
    // `should_ask_for_a_notice`; this closure only applies it.
    //
    // The closure reads the *cache* now and never fills it, so a cold
    // key costs nothing on this thread either — see the app's
    // `constraint_notice`. **Nothing while the assistant is off**: there
    // is no CLI to grade, and the app does not probe one.
    let notice = floem::reactive::create_memo(move |_| {
        let _ = harness.get();
        let path = cli_path.get();
        if !enabled.get() {
            return None;
        }
        // The rule is `should_ask_for_a_notice`, not this closure: a
        // predicate with a recorded failure history and no test is one
        // nothing can hold to it. See that function.
        if !should_ask_for_a_notice(&path, |p| notice_ok(p)) {
            return None;
        }
        (constraint_notice)()
    });
    // **Hidden, not merely empty, on the silent grade.** A container
    // rendering `empty()` is still a flex child, so it collects the
    // group's gap on both sides — which is why Claude, the one harness
    // with nothing to say here, showed the widest gap between the
    // dropdown and the path field below it. Taffy drops a `Display::None`
    // child before the gaps are distributed, so this costs nothing.
    let harness_notice = dyn_container(
        move || notice.get(),
        move |why| match why {
            None => empty().into_any(),
            Some(why) => text(why)
                .style(|s| {
                    s.width_full()
                        .font_size(theme::font_hint())
                        .color(theme::plan_warn())
                })
                .into_any(),
        },
    )
    .style(move |s| match notice.with(|n| n.is_some()) {
        true => s,
        false => s.display(floem::style::Display::None),
    });
    // What off means, under the choice that made it — text, not a
    // control, so hiding it is safe for the ring (§ Floem: a `.hide()`n
    // control is still in the Tab order).
    let off_hint = text(
        "Nothing is sent to any agent, no agent CLI is started, and no AI action is \
                 offered. Pick a CLI to turn the assistant on.",
    )
    .style(move |s| {
        let s = s
            .width_full()
            .font_size(theme::font_hint())
            .color(theme::text_muted());
        if enabled.get() {
            s.display(floem::style::Display::None)
        } else {
            s
        }
    });
    let harness_section = v_stack((
        settings_group_label("Agent CLI"),
        agent_dd,
        harness_notice,
        off_hint,
    ))
    .style(group);

    // **Everything below the choice is built only while the assistant is
    // on**, and torn down when it goes off — not hidden: a hidden field
    // is still in the ring, and Tab would walk an off modal through a
    // CLI path, a model and four more controls nobody can see. Each binds
    // straight to an `AiUi` signal, so rebuilding loses nothing. Called
    // once per turn-on by the container at the foot of the body.
    let detect_path = detect_path.clone();
    let cli_ok = cli_ok.clone();
    let rest_ring = ring.clone();
    let build_rest = move || -> AnyView {
        let ring = rest_ring.clone();
        let detect_path = detect_path.clone();
        let cli_ok = cli_ok.clone();

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
        // **`width_full` is what makes these wrap.** A label sets its own
        // taffy width to the measured text, so it overflows the panel rather
        // than stretching to it — and an auto-detected path is long, has no
        // spaces, and is the one line here that reliably exceeds 460px. At
        // 100% the line is bounded and cosmic-text falls back to glyph-level
        // breaks, which is what a path wants anyway.
        let red = |s: floem::style::Style| {
            s.width_full()
                .font_size(theme::font_label())
                .color(theme::reject_bg())
        };
        // **Keyed on the harness as well as the path.** Both halves of this
        // hint are per-harness: detection resolves *that* CLI's binary, and
        // the failure line has to name it. Keyed on the path alone, it kept
        // reporting the previous harness's binary — and "Claude CLI not
        // found" — under a field the caption above had already relabelled.
        let hint = dyn_container(
            move || (cli_path.get(), harness.get()),
            move |(path, h)| {
                if path.trim().is_empty() {
                    match detect_path() {
                        Some(p) => text(format!("Auto-detected: {}", p)).style(|s| {
                            s.width_full()
                                .font_size(theme::font_label())
                                .color(theme::conn_ok())
                        }),
                        None => text(format!(
                            "Auto-detect failed. {} not found — give it a path above.",
                            h.label()
                        ))
                        .style(red),
                    }
                    .into_any()
                } else if cli_ok(path) {
                    empty().into_any()
                } else {
                    text("File doesn't exist.").style(red).into_any()
                }
            },
        );

        // **A field, not a dropdown.** Any id the CLI accepts is valid —
        // a dated snapshot, an alias newer than this build — so the control
        // has to take text. The suggestions sit under it as one-click
        // buttons rather than in a combo popup, which keeps the common case
        // to a single click without pretending the list is exhaustive.
        let model_field = edit_field(
            model,
            FieldCfg {
                placeholder: "Leave empty for the harness's default",
                clearable: true,
                focus: Some((ring.clone(), 20)),
                ..Default::default()
            },
        )
        .style(|s| s.width_full());
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

        // The label follows the harness: the field points at *that* CLI's
        // binary, and a box captioned "Claude Code CLI path" over a Codex
        // session is the kind of stale caption a user reasonably acts on.
        let cli_section = v_stack((
            label(move || format!("{} path", harness.get().label())).style(form_label_style),
            path_field,
            hint,
        ))
        .style(group);
        // Suggestions follow the harness, and an empty list renders nothing
        // — Antigravity's model names were never measured, and inventing a
        // menu for it would be guessing in the user's settings.
        let suggestions = dyn_container(
            move || harness.get(),
            move |h| {
                let picks = h.suggested_models();
                if picks.is_empty() {
                    return empty().into_any();
                }
                h_stack_from_iter(picks.iter().map(|m| {
                    let m = m.to_string();
                    let set = m.clone();
                    // Tinted when it is the value in effect — the same
                    // vocabulary the dropdowns use for "you are holding
                    // this one".
                    let is_cur = m.clone();
                    text(m)
                        .on_click_stop(move |_| model.set(set.clone()))
                        .style(move |s| {
                            let active = model.get() == is_cur;
                            s.padding_horiz(theme::scaled(8.0))
                                .padding_vert(theme::scaled(3.0))
                                .border(1.0)
                                .border_color(theme::field_border())
                                .border_radius(theme::scaled(4.0))
                                .font_size(theme::font_hint())
                                .color(if active {
                                    theme::chip_active()
                                } else {
                                    theme::text_muted()
                                })
                                .hover(|s| s.color(theme::accent_hover()))
                        })
                }))
                .style(|s| {
                    s.flex_row()
                        .flex_wrap(floem::style::FlexWrap::Wrap)
                        .width_full()
                        .gap(theme::scaled(6.0))
                })
                .into_any()
            },
        )
        // Hidden the same way the harness notice and the Effort row are: an
        // empty chip row is still a flex child, so it kept its 6px gap and
        // pushed the hint below it away from the field.
        .style(move |s| match harness.get().suggested_models().is_empty() {
            false => s,
            true => s.display(floem::style::Display::None),
        });
        let model_section = v_stack((
            settings_group_label("Model"),
            model_field,
            suggestions,
            // **The second sentence is about the chips, so it goes when they
            // do.** Under Antigravity, which suggests nothing, "the buttons
            // are shortcuts, not the whole list" pointed at buttons that
            // were not on screen — a hint that describes an absent control
            // reads as a bug in the modal rather than a note about the
            // field.
            label(move || {
                let base = "Any id this CLI accepts — an alias, or a dated snapshot to pin \
                                one across upgrades.";
                match harness.get().suggested_models().is_empty() {
                    true => base.to_string(),
                    false => {
                        format!("{base} The buttons are shortcuts, not the whole list.")
                    }
                }
            })
            .style(|s| {
                s.width_full()
                    .font_size(theme::font_hint())
                    .color(theme::text_muted())
            }),
        ))
        .style(group);
        // **Hidden, not disabled, where the harness has no such flag.**
        // Effort is Claude's `--effort` and Antigravity's; Codex takes no
        // equivalent, and a greyed control still says "this exists
        // for you and is off", which is a different and false claim. Asked
        // as a capability so a harness that grows the flag needs no edit
        // here.
        //
        // The dropdown is built *inside* the container because a floem view
        // is not `Clone` — it has to be constructed on each keyed rebuild.
        let effort_ring = ring.clone();
        // **Keyed on the levels, not on `supports_effort()`.** The child
        // reads `levels` from the value it is handed, so the key has to
        // carry everything the child reads: a `bool` that is true for both
        // Claude and Antigravity cannot tell the dropdown that the list
        // behind it changed, and it went on offering `xhigh` under a harness
        // whose flag does not take it.
        //
        // **Not because the key dedups — it does not.** The comment here
        // used to say the child "was never rebuilt", which is the belief
        // `docs/architecture.md`'s Floem gotcha exists to correct and the
        // one that cost the DDL editors their caret: `dyn_container` runs
        // `create_updater` and there is no equality check on that path
        // (`floem-0.2.0/src/views/dyn_container.rs`,
        // `floem_reactive-0.2.0/src/effect.rs`). The key is a *value handed
        // to the child*, not a change detector, and stating it as one here
        // — where it reads as the tidiest explanation in the file — teaches
        // the wrong contract to whoever copies it next.
        let effort_section = dyn_container(
            move || harness.get().effort_levels(),
            move |levels| {
                if levels.is_empty() {
                    return empty().into_any();
                }
                // Only the levels this harness's own flag takes — Claude has
                // a fourth (`xhigh`) that Antigravity does not advertise.
                // The rule lives in `AiEffort::offered_by` rather than here,
                // because a decision inside a view closure is one no test
                // can reach: the test that guarded this re-performed the
                // filter in its own body and stayed green whatever this
                // line did.
                let offered = AiEffort::offered_by(levels);
                // `width_full` on both levels: the page stretches its direct
                // children, which this container is, but nothing stretches the
                // container's own child — so the dropdown took its label's
                // width while every other one on the page ran full width.
                v_stack((
                    settings_group_label("Effort"),
                    focusable_dropdown(effort, offered, AiEffort::label, effort_ring.clone(), 30),
                ))
                .style(move |s| group(s).width_full())
                .into_any()
            },
        )
        // Hidden the same way the harness notice is, and for the same
        // reason: an empty container between two group gaps reads as one
        // double-width hole, which is what Codex showed between Model and
        // Custom instructions.
        .style(move |s| match harness.get().supports_effort() {
            true => s.width_full(),
            false => s.display(floem::style::Display::None),
        });
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
            // Not "Claude's" — the panel is driven by whichever CLI the user
            // picked, and this setting is about replies rather than about
            // any one vendor's.
            "Mark the assistant's replies with a coloured rule down their right edge. Off \
                 gives them the same margin on both sides.",
            gutter,
            ring.clone(),
            60,
        );
        v_stack((
            cli_section,
            model_section,
            effort_section,
            instr_section,
            scope_section,
            data_section,
            gutter_row,
        ))
        .style(|s| s.flex_col().gap(theme::scaled(16.0)).width_full())
        .into_any()
    };
    // The off arm is `nothing()`, whose `display:none` keeps the body's
    // gap from opening a hole under the choice (§ Floem, the `.hide()`
    // entry's else-arm).
    let rest = dyn_container(
        move || enabled.get(),
        move |on| {
            if on {
                build_rest()
            } else {
                crate::widgets::nothing().into_any()
            }
        },
    )
    .style(move |s| {
        let s = s.width_full();
        if enabled.get() {
            s
        } else {
            s.display(floem::style::Display::None)
        }
    });
    container(section_page((
        form_section_header(SettingsSection::Ai.label()),
        harness_section,
        rest,
    )))
    .on_cleanup(move || {
        let apply = apply.clone();
        floem::action::exec_after(std::time::Duration::ZERO, move |_| apply());
    })
    .into_any()
}

/// One shortcut line: description on the left, the key combo on the right in a
/// monospace pill.
fn shortcut_row(keys: &'static str, desc: &'static str) -> impl IntoView {
    h_stack((
        text(desc).style(|s| s.color(theme::text()).font_size(theme::font_body())),
        empty().style(|s| s.flex_grow(1.0_f32).min_width(12.0)),
        // Respelled for the platform here rather than in the table: one choke
        // point for every row the modal shows. See `shortcuts::keys_label`.
        text(crate::shortcuts::keys_label(keys)).style(|s| {
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

/// Format Code's wrap widths, in characters. **`0` leads and means off**,
/// which is the default and the formatter before the setting existed.
const WRAP_WIDTHS: [usize; 4] = [0, 80, 100, 120];

fn wrap_width_label(n: usize) -> String {
    if n == 0 {
        "Off".to_string()
    } else {
        format!("{n} characters")
    }
}

/// The statement-timeout choices, in seconds. **`0` leads and means off**,
/// which is the default and what every release before the setting did.
///
/// The shortest real option is a minute rather than a handful of seconds: this
/// cancels the statement the user asked for, and an import, a report or a
/// `CREATE INDEX` on a large table legitimately takes longer than a person's
/// patience. A timeout tight enough to kill honest work is worse than none.
pub(crate) const STATEMENT_TIMEOUTS: [u64; 6] = [0, 60, 300, 900, 1_800, 3_600];

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

/// Is this CLI-path field stable enough to be worth a `--help` probe?
///
/// `resolves` is "does this path name a runnable binary" — the same predicate
/// the red *File doesn't exist.* hint below the field uses.
///
/// **Two answers are stable and everything between them is a keystroke.** An
/// empty field means auto-detect, which is one key into the probe cache; a
/// complete path that resolves is another. A *half-typed* path is neither: the
/// cache is keyed by `(harness, resolved path)`, so typing
/// a path asked a different question on every keystroke — one per prefix, each
/// a miss and each a synchronous subprocess on the Floem UI thread. The
/// comment on the app's closure said the cache made this cheap; the path being
/// part of the key is exactly what made every keystroke a miss.
///
/// There is nothing to say about a binary that is not there anyway — the red
/// hint above the notice is the whole answer — so the half-typed states ask
/// nothing at all.
///
/// Pure and separate from the memo because a decision inside a `create_memo`
/// closure is a decision no test can reach, and this one has a recorded failure
/// mode.
fn should_ask_for_a_notice(path: &str, resolves: impl Fn(String) -> bool) -> bool {
    path.trim().is_empty() || resolves(path.to_string())
}

/// Where the app's log lives, as the Settings row's hint.
///
/// The **full path**, not "your config directory": the row exists because the
/// log was undiscoverable, and a description of the location is not the
/// location. It stays readable even where the button cannot work — a headless
/// or sandboxed machine with no file manager — which is the case the fallback
/// string is for.
///
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

/// The Settings row that writes the portable settings and snippets to a file
/// and reads one back (`core::settings_bundle`). The app owns both trips — a
/// file dialog, a write, a confirmation before anything is replaced.
/// `tabindex` is Export's; Import follows it.
fn bundle_row(
    export: Rc<dyn Fn()>,
    import: Rc<dyn Fn()>,
    ring: crate::widgets::FocusRing,
    tabindex: u32,
) -> impl IntoView {
    h_stack((
        v_stack((
            text("Settings file").style(|s| s.color(theme::text()).font_size(theme::font_label())),
            form_hint(
                "Your preferences and snippets in one file, to take to another machine. \
                 Connections, window layout, the terminal's shell and connection-specific \
                 snippets stay here.",
            ),
        ))
        .style(|s| {
            s.flex_col()
                .gap(theme::scaled(2.0))
                .flex_grow(1.0_f32)
                .min_width(0.0)
        }),
        action_button(
            "Export",
            ActionKind::Quiet,
            true,
            ring.clone(),
            tabindex,
            move || export(),
        ),
        action_button(
            "Import",
            ActionKind::Quiet,
            true,
            ring,
            tabindex + 1,
            move || import(),
        ),
    ))
    .style(|s| s.items_center().width_full().gap(theme::scaled(10.0)))
}

/// The Settings row that puts the `schemaic` command on `PATH`, or takes it
/// off again.
///
/// Only ever on a click — never at install time — and the outcome stays under
/// the row for the rest of the session, naming the path it resolved. Remove is
/// the only undo macOS and AppImage users get: nothing of ours runs when those
/// are thrown away (Windows also has the uninstall hook). The guard against a
/// second click while either is running is the app's, beside the launch; the
/// labels only say so. `tabindex` is Install's; Remove follows it.
///
/// **Remove exists only while it would do something** (`installed`), built in
/// a `dyn_container` so it also leaves the Tab ring when it goes — a button
/// that reports "nothing to remove" is one the row should not have offered.
///
/// **Install is disabled, not hidden, when there is nothing to install**
/// (`status`) — a deb/rpm on Linux, whose `/usr/bin` binary already is the
/// command. The hint says so and where, and a greyed button beside it reads as
/// "done" where an empty row would read as "missing". It was offered anyway,
/// under a hint promising a `~/.local/bin` link, and the click answered that
/// the command was already in `/usr/bin`.
///
/// **The outcome is a line of its own under the whole row**, not a third line
/// in the label column. In the column it made the column taller, so the
/// buttons — centred on the column — slid down to sit beside the message
/// instead of the label; and a long error sharing the row with the buttons was
/// laid out wider than the modal and clipped.
fn cli_row(
    cmd: crate::CliCommand,
    ring: crate::widgets::FocusRing,
    tabindex: u32,
) -> impl IntoView {
    let crate::CliCommand {
        install,
        remove,
        state,
        installed,
        status,
    } = cmd;
    let outcome_line = |msg: String, color: fn() -> floem::peniko::Color| {
        text(msg)
            .style(move |s| {
                s.width_full()
                    .min_width(0.0)
                    .color(color())
                    .font_size(theme::font_label())
            })
            .into_any()
    };
    let outcome = dyn_container(
        move || state.get(),
        move |st| match st {
            InstallState::Done(msg) => outcome_line(msg, theme::text_dim),
            InstallState::Failed(msg) => outcome_line(msg, theme::error),
            InstallState::Idle | InstallState::Running | InstallState::Removing => nothing(),
        },
    )
    // Hidden rather than empty with nothing to say, or it takes the column's
    // gap and this row sits taller than Log file above it — the same fix as the
    // AI modal's harness notice.
    .style(move |s| {
        match state.with(|st| matches!(st, InstallState::Done(_) | InstallState::Failed(_))) {
            true => s.width_full().min_width(0.0),
            false => s.display(floem::style::Display::None),
        }
    });
    let remove_slot = {
        let ring = ring.clone();
        dyn_container(
            move || installed.get(),
            move |yes| {
                if !yes {
                    return nothing();
                }
                let remove = remove.clone();
                action_button_dyn(
                    move || match state.get() {
                        InstallState::Removing => "Removing".to_string(),
                        _ => "Remove".to_string(),
                    },
                    ActionKind::Quiet,
                    true,
                    ring.clone(),
                    tabindex + 5,
                    move || remove(),
                )
            },
        )
        // Hidden when empty, for the same reason as `outcome`: an empty flex
        // child still takes the gap beside Install.
        .style(move |s| match installed.get() {
            true => s,
            false => s.display(floem::style::Display::None),
        })
    };
    // **Disabled when there is nothing to install** — a deb/rpm's `/usr/bin`
    // binary is already the command, and the hint above says so and where.
    // Rebuilt on that one bool, like Remove, since the button's enabled state
    // is fixed when it is built; the bool changes only after a click.
    let install_slot = dyn_container(
        move || status.with(|s| s.can_install),
        move |can| {
            let install = install.clone();
            action_button_dyn(
                move || match state.get() {
                    InstallState::Running => "Installing".to_string(),
                    _ => "Install".to_string(),
                },
                ActionKind::Quiet,
                can,
                ring.clone(),
                tabindex,
                move || install(),
            )
        },
    );
    v_stack((
        h_stack((
            v_stack((
                text("Command line")
                    .style(|s| s.color(theme::text()).font_size(theme::font_label())),
                floem::views::label(move || status.with(|s| s.hint.clone())).style(form_hint_style),
            ))
            .style(|s| {
                s.flex_col()
                    .gap(theme::scaled(2.0))
                    .flex_grow(1.0_f32)
                    .min_width(0.0)
            }),
            h_stack((install_slot, remove_slot))
                .style(|s| s.flex_row().items_center().gap(action_gap())),
        ))
        .style(|s| s.items_center().width_full().gap(theme::scaled(10.0))),
        outcome,
    ))
    .style(|s| s.flex_col().width_full().gap(theme::scaled(6.0)))
}

/// `Schemaic v0.24.0`, at the foot of the section list.
///
/// Dimmed and unbold on purpose: it is a caption identifying the panel, not a
/// section competing with the list above it — and at the list's foot rather
/// than atop one page, so every section shows it. The string comes from
/// [`schemaic_core::app_version_label`] so a release bump reaches it with no
/// edit here — see the tests beside that function for what that does and does
/// not guarantee. Inset as the rows are, so its text starts where theirs do.
fn settings_version_line() -> impl IntoView {
    text(schemaic_core::app_version_label()).style(|s| {
        s.font_size(theme::font_hint())
            .color(theme::text_dim())
            .padding_horiz(theme::scaled(12.0))
            .padding_vert(theme::scaled(8.0))
    })
}

/// The Settings modal's sections, in the order its list shows them — each a
/// page of its own on the right, the way Manage connections shows one
/// connection beside its list.
///
/// **Which one is showing outlives the modal** ([`LayoutUi::settings_section`]):
/// reopening lands where the user left, which is what changing a setting,
/// trying it and coming back to change it again wants. It is not persisted —
/// a fresh start opens on General.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SettingsSection {
    #[default]
    General,
    Editor,
    Formatting,
    Query,
    Terminal,
    Ai,
    Appearance,
}

impl SettingsSection {
    /// Every section, in the order the list shows them.
    pub const ALL: [SettingsSection; 7] = [
        SettingsSection::General,
        SettingsSection::Editor,
        SettingsSection::Formatting,
        SettingsSection::Query,
        SettingsSection::Terminal,
        SettingsSection::Ai,
        SettingsSection::Appearance,
    ];

    /// The section's name, in its list and as its page's heading.
    pub fn label(self) -> &'static str {
        match self {
            SettingsSection::General => "General",
            SettingsSection::Editor => "Editor",
            SettingsSection::Formatting => "Code formatting",
            SettingsSection::Query => "Query",
            SettingsSection::Terminal => "Terminal",
            SettingsSection::Ai => "AI assistant",
            SettingsSection::Appearance => "Appearance",
        }
    }
}

/// Open Settings on `section` — the door the terminal's and the AI panel's
/// gears use, each landing on its own section.
pub(crate) fn open_settings_at(layout: LayoutUi, section: SettingsSection) {
    layout.settings_section.set(section);
    layout.theme_settings_open.set(true);
}

/// The section list's width, added to the page's: the page keeps the width
/// the single-column modal had.
const SECTION_LIST_W: f64 = 170.0;

/// The page's width — the single-column modal's, which every setting's hint
/// was written to wrap within.
const SECTION_PAGE_W: f64 = 420.0;

/// Everything the Settings modal reaches out of the root bundle, one field per
/// domain its sections edit.
///
/// Holds no `Ui`, for `whole_ui_gate`'s reason and
/// [`crate::compare_view::CompareCtx`]'s: it is built by naming the reads at
/// its one call site, so what the modal touches stays written down there.
pub(crate) struct SettingsCtx {
    /// The app's own settings, and the modal's open flag and showing section.
    pub(crate) layout: LayoutUi,
    /// The Terminal section's settings, and the shell respawn a pick runs.
    pub(crate) term: TermUi,
    pub(crate) term_actions: Rc<TermActions>,
    /// The AI assistant section's settings and actions — the `ConnUi` is the
    /// active connection's data-access level, which the section reports.
    pub(crate) ai: AiUi,
    pub(crate) conn: ConnUi,
    pub(crate) ai_actions: Rc<AiActions>,
    /// General's two trips outside the app: the log's folder, and the
    /// command-line install.
    pub(crate) open_config_dir: Rc<dyn Fn()>,
    pub(crate) cli_command: crate::CliCommand,
    /// The settings file's two trips: write one, read one back.
    pub(crate) export_settings: Rc<dyn Fn()>,
    pub(crate) import_settings: Rc<dyn Fn()>,
}

/// The Settings modal: every setting the app has, a section to a page. The
/// terminal's and the assistant's took modals of their own until they joined
/// it, which is why [`SettingsCtx`] carries their bundles too.
pub(crate) fn settings_overlay(ctx: SettingsCtx) -> impl IntoView {
    let SettingsCtx {
        layout,
        term,
        term_actions,
        ai,
        conn,
        ai_actions,
        open_config_dir,
        cli_command,
        export_settings,
        import_settings,
    } = ctx;
    let open = layout.theme_settings_open;
    let section = layout.settings_section;

    dyn_container(
        move || open.get(),
        move |is_open| {
            if !is_open {
                return empty().into_any();
            }
            let close: Rc<dyn Fn()> = Rc::new(move || open.set(false));

            // The modal's Tab order — see `FocusRing`. The section list is one
            // stop at `NAV_TAB`, ahead of every control, with ↑/↓ moving
            // between sections. Only the showing page's controls are in the
            // ring: a page is built when its section is picked, and each
            // control leaves the ring as its page is torn down.
            let ring = crate::widgets::FocusRing::new();

            let page = {
                let ring = ring.clone();
                let open_config_dir = open_config_dir.clone();
                let cli_command = cli_command.clone();
                let (export_settings, import_settings) =
                    (export_settings.clone(), import_settings.clone());
                let term_actions = term_actions.clone();
                let ai_actions = ai_actions.clone();
                dyn_container(
                    move || section.get(),
                    move |s| {
                        let body = match s {
                            SettingsSection::General => general_section(
                                layout,
                                ring.clone(),
                                open_config_dir.clone(),
                                cli_command.clone(),
                                export_settings.clone(),
                                import_settings.clone(),
                            ),
                            SettingsSection::Editor => editor_section(layout, ring.clone()),
                            SettingsSection::Formatting => formatting_section(layout, ring.clone()),
                            SettingsSection::Query => query_section(layout, ring.clone()),
                            SettingsSection::Terminal => {
                                terminal_section(term, term_actions.clone(), ring.clone())
                            }
                            SettingsSection::Ai => {
                                ai_section(ai, conn, ai_actions.clone(), ring.clone())
                            }
                            SettingsSection::Appearance => appearance_section(layout, ring.clone()),
                        };
                        // A page of its own scrolls on its own, from the top
                        // each time it is shown; the list beside it stays put.
                        autohide(scroll(
                            body.style(|s| s.width_full().padding(theme::scaled(14.0))),
                        ))
                        .style(|s| s.flex_grow(1.0_f32).width_full().min_height(0.0))
                        .into_any()
                    },
                )
                .style(|s| s.flex_grow(1.0_f32).flex_col().min_width(0.0).height_full())
            };
            let body = h_stack((section_list(section, ring.clone()), page))
                .style(|s| s.width_full().flex_grow(1.0_f32).flex_row().min_height(0.0));

            // One height for every section, so switching never resizes the
            // modal; a long page scrolls inside it.
            let panel = v_stack((modal_title("Settings", close.clone(), ring.clone()), body))
                .on_click_stop(|_| {})
                .style(|s| {
                    panel_style(s)
                        .background(theme::bg_panel())
                        .width(crate::widgets::modal_w(SECTION_LIST_W + SECTION_PAGE_W))
                        .height(crate::widgets::modal_h(560.0))
                });

            let esc = close.clone();
            // Click-to-dismiss on a sibling behind the panel, never on the focus
            // root — floem fires `Click` there for Space. See
            // `widgets::dismiss_layer`.
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

/// The Settings modal's list of sections: one Tab stop, ↑/↓ between them,
/// the showing one in Manage connections' selected-row colours. The app's
/// version sits at its foot, where every section can see it.
fn section_list(section: RwSignal<SettingsSection>, ring: FocusRing) -> impl IntoView {
    let rows = v_stack_from_iter(SettingsSection::ALL.into_iter().map(move |s| {
        text(s.label())
            .on_click_stop(move |_| {
                if section.get_untracked() != s {
                    section.set(s);
                }
            })
            .style(move |st| {
                let st = st
                    .width_full()
                    .font_size(theme::font_body())
                    .padding_horiz(theme::scaled(12.0))
                    .padding_vert(theme::scaled(11.0));
                if section.get() == s {
                    st.color(theme::conn_list_sel_text())
                        .background(theme::conn_list_sel_bg())
                } else {
                    st.color(theme::conn_list_text())
                        .hover(|st| st.color(theme::conn_list_sel_text()))
                }
            })
    }))
    .style(|s| s.flex_col().width_full().gap(theme::scaled(5.0)));
    // Clamped, as Manage connections' list is: the ring around it is what
    // wraps.
    let list = crate::widgets::nav_group(
        rows,
        ring,
        crate::widgets::NAV_TAB,
        crate::widgets::NavAxis::Vertical,
        move |delta| {
            let all = SettingsSection::ALL;
            let cur = all
                .iter()
                .position(|s| *s == section.get_untracked())
                .unwrap_or(0);
            if let Some(next) = crate::widgets::list_step(all.len(), cur, delta) {
                section.set(all[next]);
            }
        },
    );
    // **The rows scroll, as the page does**, with the version pinned beneath
    // them: the panel's height is fixed and capped to the window, and an
    // unscrolled list of seven rows painted its last ones and the version
    // below the panel's bottom edge on a short window. `flex_grow` takes the
    // room the spacer used to, so on a tall one the version still sits at the
    // foot.
    let list = autohide(scroll(container(list).style(|s| s.width_full())))
        .style(|s| s.flex_grow(1.0_f32).width_full().min_height(0.0));
    v_stack((list, settings_version_line())).style(|s| {
        s.width(theme::scaled(SECTION_LIST_W))
            .flex_shrink(0.0_f32)
            .height_full()
            .flex_col()
            .border_right(1.0)
            .border_color(theme::border())
            .padding_vert(theme::scaled(6.0))
    })
}

/// A settings page: its section's heading, then its settings, a group's gap
/// apart.
///
/// **`width_full`, so a long hint wraps at the page rather than widening it.**
/// The page is usually the child the modal pads, but the AI section's sits one
/// container in (its teardown hook needs a view of its own), and without this
/// it took its longest line's width and ran off the panel's right edge.
fn section_page<V: ViewTuple + 'static>(children: V) -> AnyView {
    v_stack(children)
        .style(|s| s.flex_col().width_full().gap(theme::scaled(16.0)))
        .into_any()
}

/// A setting's caption and control stacked, the way every dropdown sits.
fn ctrl(s: floem::style::Style) -> floem::style::Style {
    s.flex_col().gap(theme::scaled(6.0))
}

/// The General section: the session, the log, the command-line tool.
///
/// **A section's Tab numbers follow its layout, not the order a control was
/// added**, spaced by 10 so one can be inserted between two — a control
/// appended by number rather than inserted at its place walks the user
/// backwards through the page: the statement timeout was written last and
/// numbered 230 while sitting *second* in its group, so Tab went row limit →
/// confirm → validate → back up to timeout. Each section keeps the block of
/// numbers it had in the single long form; they never collide, though only
/// one page is in the ring at a time.
fn general_section(
    layout: LayoutUi,
    ring: FocusRing,
    open_config_dir: Rc<dyn Fn()>,
    cli_command: crate::CliCommand,
    export_settings: Rc<dyn Fn()>,
    import_settings: Rc<dyn Fn()>,
) -> AnyView {
    let restore_tabs = layout.restore_tabs;
    let restore_row = focusable_toggle_row(
        "Restore tabs on startup",
        "Reopen the query tabs from your last session when the app starts.",
        restore_tabs,
        ring.clone(),
        10,
    );
    section_page((
        form_section_header(SettingsSection::General.label()),
        restore_row,
        log_row(open_config_dir, ring.clone(), 20),
        cli_row(cli_command, ring.clone(), 30),
        bundle_row(export_settings, import_settings, ring, 40),
    ))
}

/// The Editor section. (Tab width, spaces-vs-tabs, and word wrap live in the
/// status bar.)
fn editor_section(layout: LayoutUi, ring: FocusRing) -> AnyView {
    let editor_font = layout.editor_font;
    let editor_predictions = layout.editor_predictions;
    let editor_remembered_predictions = layout.editor_remembered_predictions;
    let table_aliases = layout.table_aliases;
    let rainbow_brackets = layout.rainbow_brackets;
    let show_whitespace = layout.show_whitespace;
    let indent_guides = layout.indent_guides;
    let font_dd = focusable_dropdown(
        editor_font,
        EDITOR_FONT_SIZES,
        editor_font_label,
        ring.clone(),
        100,
    );
    let font_section = v_stack((settings_group_label("Font size"), font_dd)).style(ctrl);
    let predictions_row = focusable_toggle_row(
        "Inline predictions",
        "Show the likely next words after the caret — the partner of a keyword, a \
                 join's foreign key, the columns to group by or insert. Tab, or → at the end of \
                 a line, takes them whole, Ctrl+→ (⌘→ on macOS) a word at a time, and Escape \
                 turns them down.",
        editor_predictions,
        ring.clone(),
        110,
    );
    // Greyed out while inline predictions are off: it refines them, and
    // means nothing without them.
    let remembered_row = focusable_toggle_row_when(
        "Predictions from past statements",
        "Also offer the rest of a line from a statement you ran, wrote in another \
                 tab or saved as a snippet. Switch off to keep past statements off the \
                 screen.",
        editor_remembered_predictions,
        editor_predictions,
        ring.clone(),
        120,
    );
    let aliases_row = focusable_toggle_row(
        "Table aliases",
        "When completion writes a table after FROM or JOIN, follow it with a short \
                 alias made of its initials — orders o, order_items oi — and use the alias \
                 in a join's ON condition.",
        table_aliases,
        ring.clone(),
        125,
    );
    let rainbow_row = focusable_toggle_row(
        "Rainbow brackets",
        "Colour each pair of brackets by how deeply it is nested, so a closing \
                 bracket can be matched to its opening one at a glance.",
        rainbow_brackets,
        ring.clone(),
        130,
    );
    let whitespace_row = focusable_toggle_row(
        "Show whitespace",
        "Draw spaces and tabs as faint marks, so a tab can be told from spaces.",
        show_whitespace,
        ring.clone(),
        140,
    );
    let guides_row = focusable_toggle_row(
        "Indent guides",
        "Draw a faint vertical line at each indent level.",
        indent_guides,
        ring.clone(),
        150,
    );
    section_page((
        form_section_header(SettingsSection::Editor.label()),
        font_section,
        predictions_row,
        remembered_row,
        aliases_row,
        rainbow_row,
        whitespace_row,
        guides_row,
    ))
}

/// The Code formatting section — what Format Code does, `FormatStyle` field
/// by field.
fn formatting_section(layout: LayoutUi, ring: FocusRing) -> AnyView {
    let keyword_case = layout.keyword_case;
    let function_case = layout.function_case;
    let type_case = layout.type_case;
    let comma_style = layout.comma_style;
    let statement_gap = layout.statement_gap;
    let wrap_width = layout.wrap_width;
    let river = layout.river;
    let align = layout.align;
    let join_on_own_line = layout.join_on_own_line;
    let case_dd = focusable_dropdown(
        keyword_case,
        schemaic_core::sqlfmt::KeywordCase::ALL,
        schemaic_core::sqlfmt::KeywordCase::label,
        ring.clone(),
        160,
    );
    let case_section = v_stack((
        settings_group_label("Keyword case"),
        case_dd,
        form_hint(
            "How Format Code writes keywords such as SELECT and FROM. Names, \
                     strings and comments keep their case.",
        ),
    ))
    .style(ctrl);
    let function_case_dd = focusable_dropdown(
        function_case,
        schemaic_core::sqlfmt::KeywordCase::ALL,
        schemaic_core::sqlfmt::KeywordCase::label,
        ring.clone(),
        163,
    );
    let function_case_section = v_stack((
        settings_group_label("Function case"),
        function_case_dd,
        form_hint(
            "How Format Code writes the engine's own functions, such as COUNT and \
                     COALESCE, where they are called. Your own functions keep their case.",
        ),
    ))
    .style(ctrl);
    let type_case_dd = focusable_dropdown(
        type_case,
        schemaic_core::sqlfmt::KeywordCase::ALL,
        schemaic_core::sqlfmt::KeywordCase::label,
        ring.clone(),
        166,
    );
    let type_case_section = v_stack((
        settings_group_label("Type case"),
        type_case_dd,
        form_hint(
            "How Format Code writes the engine's own data types, such as INT and \
                     VARCHAR. Types you created keep their case.",
        ),
    ))
    .style(ctrl);
    let commas_dd = focusable_dropdown(
        comma_style,
        schemaic_core::sqlfmt::CommaStyle::ALL,
        schemaic_core::sqlfmt::CommaStyle::label,
        ring.clone(),
        170,
    );
    let commas_section = v_stack((
        settings_group_label("Commas"),
        commas_dd,
        form_hint(
            "Where Format Code puts the comma of a list it breaks across lines. \
                     Lists kept on one line, like a function's arguments, are left alone \
                     unless the wrap width breaks them.",
        ),
    ))
    .style(ctrl);
    let gap_dd = focusable_dropdown(
        statement_gap,
        schemaic_core::sqlfmt::StatementGap::ALL,
        schemaic_core::sqlfmt::StatementGap::label,
        ring.clone(),
        180,
    );
    let gap_section = v_stack((
        settings_group_label("Between statements"),
        gap_dd,
        form_hint(
            "The blank lines Format Code leaves between two statements. As typed \
                     keeps a blank line wherever there was one.",
        ),
    ))
    .style(ctrl);
    let wrap_dd = focusable_dropdown(wrap_width, WRAP_WIDTHS, wrap_width_label, ring.clone(), 183);
    let wrap_section = v_stack((
        settings_group_label("Wrap width"),
        wrap_dd,
        form_hint(
            "A list Format Code keeps on one line, like a function's arguments, \
                     goes one item a line once its line would run past this width.",
        ),
    ))
    .style(ctrl);
    let river_row = focusable_toggle_row(
        "Right-align clause keywords",
        "Line SELECT, FROM, WHERE and the rest up on their right edge, each \
                 clause's body beside its keyword, instead of a keyword on a line of its own.",
        river,
        ring.clone(),
        185,
    );
    let align_row = focusable_toggle_row(
        "Align columns",
        "Line up the AS of a select list's aliases, the = of an UPDATE's \
                 assignments and the types of a table's columns.",
        align,
        ring.clone(),
        188,
    );
    let join_on_row = focusable_toggle_row(
        "Join condition on its own line",
        "Put a join's ON, and the AND and OR of its condition, on a line of \
                 their own under the join.",
        join_on_own_line,
        ring.clone(),
        190,
    );
    section_page((
        form_section_header(SettingsSection::Formatting.label()),
        case_section,
        function_case_section,
        type_case_section,
        commas_section,
        gap_section,
        wrap_section,
        river_row,
        align_row,
        join_on_row,
    ))
}

/// The Query section: what a run fetches, how long it may take, and what it
/// asks first.
fn query_section(layout: LayoutUi, ring: FocusRing) -> AnyView {
    let row_limit = layout.row_limit;
    let statement_timeout = layout.statement_timeout;
    let confirm_writes = layout.confirm_writes;
    let write_log_values = layout.write_log_values;
    let live_validate = layout.live_validate;
    let notify_finished = layout.notify_finished;
    let row_dd = focusable_dropdown(row_limit, ROW_LIMITS, row_limit_label, ring.clone(), 200);
    let row_section = v_stack((settings_group_label("Default row limit"), row_dd)).style(ctrl);
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
    let notify_row = focusable_toggle_row(
        "Notify when a run finishes in the background",
        "Show a desktop notification when a query, script, export or dump that \
                 took a second or more ends while Schemaic is not the active window. The \
                 taskbar button is highlighted either way. The notification never \
                 includes the SQL.",
        notify_finished,
        ring.clone(),
        240,
    );
    let write_log_row = focusable_toggle_row(
        "Keep values in the write log",
        "Every write Schemaic or schemaic exec runs is appended to write_log.jsonl in the \
                 settings folder, and never cleared. Values in its SQL are masked unless \
                 this is on; a password is masked either way.",
        write_log_values,
        ring.clone(),
        250,
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
    section_page((
        form_section_header(SettingsSection::Query.label()),
        row_section,
        timeout_section,
        confirm_row,
        validate_row,
        notify_row,
        write_log_row,
    ))
}

/// The Appearance section: the two themes and the interface scale.
fn appearance_section(layout: LayoutUi, ring: FocusRing) -> AnyView {
    let ui_theme = layout.ui_theme;
    let editor_theme = layout.editor_theme;
    let ui_scale = layout.ui_scale;
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
    let ui_section = v_stack((settings_group_label("Interface theme"), ui_dd)).style(ctrl);
    let editor_section = v_stack((settings_group_label("Editor theme"), editor_dd)).style(ctrl);
    // No hint under this one, unlike the settings that carry a
    // consequence: four segments named by their percentage, applying the
    // instant they are pressed, explain themselves better than a line of
    // prose restating them.
    let scale_section = v_stack((settings_group_label("Interface scale"), scale_dd)).style(ctrl);
    section_page((
        form_section_header(SettingsSection::Appearance.label()),
        ui_section,
        editor_section,
        scale_section,
        language_section(layout.ui_language, ring, 330),
    ))
}

/// *Language*: the system's, or a shipped catalogue by its own name.
///
/// **It applies on the next start**, unlike every other control on this page —
/// labels are built with their text and nothing re-renders for a setting changed
/// once — so the hint below says so, and says it only when the choice would draw
/// a different language from the one on screen (`LanguageChoice::needs_restart`).
fn language_section(
    choice: RwSignal<crate::i18n::LanguageChoice>,
    ring: FocusRing,
    tabindex: u32,
) -> impl IntoView {
    use crate::i18n::{LanguageChoice, tr};
    let dd = focusable_dropdown(
        choice,
        LanguageChoice::options(),
        LanguageChoice::label,
        ring,
        tabindex,
    );
    let hint = dyn_container(
        move || choice.get().needs_restart(),
        |restart| {
            if restart {
                form_hint(tr!("settings-language-restart")).into_any()
            } else {
                empty().into_any()
            }
        },
    );
    v_stack((settings_group_label(tr!("settings-language")), dd, hint)).style(ctrl)
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
pub(crate) fn help_overlay(layout: LayoutUi) -> impl IntoView {
    let open = layout.help_open;

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
        EDITOR_FONT_SIZES, ROW_LIMITS, STATEMENT_TIMEOUTS, WRAP_WIDTHS, editor_font_label,
        log_hint, row_limit_label, should_ask_for_a_notice, statement_timeout_label,
        term_font_label, thousands, wrap_width_label,
    };
    use crate::consts::TERM_FONT_SIZES;

    /// **A blocking `--help` spawn per keystroke.** The notice memo tracks the
    /// live field and the probe cache is keyed by the resolved path, so every
    /// half-typed state was a fresh key, a fresh miss, and a synchronous
    /// subprocess on the Floem UI thread. Only two states are stable enough to
    /// ask about: empty (auto-detect) and a path that resolves.
    #[test]
    fn only_a_settled_cli_path_is_worth_a_probe() {
        // Typing `C:\tools\claude.exe` one key at a time. Nothing resolves until
        // the last character, and the red "File doesn't exist." hint above the
        // notice is the whole answer in the meantime.
        let full = r"C:\tools\claude.exe";
        let resolves = |p: String| p == full;
        let asked = (1..full.len())
            .filter(|n| should_ask_for_a_notice(&full[..*n], resolves))
            .count();
        assert_eq!(asked, 0, "{asked} probes while the user was still typing");
        assert!(should_ask_for_a_notice(full, resolves), "the settled path");

        // Empty is auto-detect, which is the *other* stable key — and it is one
        // the cache is warmed for, so it must not be gated behind `resolves`.
        for blank in ["", "   ", "\t"] {
            assert!(
                should_ask_for_a_notice(blank, |_| false),
                "auto-detect asked nothing"
            );
        }
        // A path that will never resolve stays silent however long it gets.
        assert!(!should_ask_for_a_notice("/nope/claude", |_| false));
    }

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
        assert_eq!(wrap_width_label(90), "90 characters");
    }

    /// Every section is listed once, the list opens on the default, and no two
    /// share a name — the name is the page's heading and the row's only text.
    #[test]
    fn the_settings_sections_are_listed_once_and_named_apart() {
        use super::SettingsSection;
        let all = SettingsSection::ALL;
        assert_eq!(all[0], SettingsSection::default());
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a, b);
                assert_ne!(a.label(), b.label());
            }
        }
        // Exhaustive, so a section added to the enum stops this compiling —
        // the prompt to add it to `ALL` and to the count below as well. It
        // cannot prove `ALL` holds it: that is the count's job, and the
        // count is only as right as whoever edits it.
        for s in all {
            match s {
                SettingsSection::General
                | SettingsSection::Editor
                | SettingsSection::Formatting
                | SettingsSection::Query
                | SettingsSection::Terminal
                | SettingsSection::Ai
                | SettingsSection::Appearance => {}
            }
        }
        assert_eq!(all.len(), 7);
    }

    /// The wrap width's list leads with off, the default, and labels it so;
    /// every other width says its count.
    #[test]
    fn the_wrap_widths_lead_with_off() {
        assert_eq!(WRAP_WIDTHS[0], 0);
        assert_eq!(wrap_width_label(0), "Off");
        for n in &WRAP_WIDTHS[1..] {
            assert_eq!(wrap_width_label(*n), format!("{n} characters"));
        }
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
