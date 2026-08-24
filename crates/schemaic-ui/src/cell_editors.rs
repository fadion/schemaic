//! **Type-aware value controls** — the widgets a cell whose legal values are
//! already written down is edited with, instead of a text field.
//!
//! Which control a column gets is [`schemaic_core::celledit`]'s decision, and
//! every rule about what a control may write is over there too — including what
//! each row of a picker *writes*, which is not always what it reads
//! ([`celledit::pick_options`]). This module is the reactive shell. Each builder
//! binds to the same `RwSignal<String>` buffer a text field would have bound to,
//! so the write path, the NULL toggle and the staged-edit machinery around it are
//! unchanged: a control is only ever a different way of typing the same string.
//!
//! Three shapes, each chosen for the failure it avoids:
//!
//! * **A picker** — booleans and enums alike: a box that opens the app's own
//!   [`crate::widgets::menu_panel`], not a floem
//!   [`Dropdown`](floem::views::dropdown::Dropdown). The reason is a floem 0.2 bug
//!   documented in `settings::scale_picker`: an overlay that would overflow the
//!   window bottom is nudged back in **at paint only**, so its rows answer the
//!   pointer where they used to be. The row panel is a strip at the *bottom* of
//!   the results area, which is exactly where that lands — and the shared menu
//!   channel already flips at the edges, walks with the arrow keys and dismisses
//!   on Escape. A boolean is a two-row picker rather than a switch or a segmented
//!   track for the same reason it is not a checkbox: the value has a **third**
//!   state here, "nothing chosen yet", which an empty cell in a pending row is in.
//! * **Set chips** — one per member, toggled in place, in the row panel. A menu
//!   closes on the first pick, and picking is the one thing a subset does
//!   repeatedly. (In a cell, where there is no room for chips, a `SET` uses the
//!   picker and toggles one member per opening.)
//! * **A calendar** — a panel that drops from the field, in the window's own
//!   overlay layer so it is clipped by nothing and flips at the edges. The text
//!   field *stays* beside it: typing a date is often faster, a `TIMESTAMP`'s time
//!   of day has no calendar to come from, and a value the picker cannot represent
//!   (`0000-00-00`) still has to be editable. The same panel serves a row-panel
//!   field and an open grid cell — the overlay layer is the only place a panel can
//!   stand over the grid at all — and [`DatePick::on_pick`] is the one thing they
//!   ask of it differently: a cell has no Save button in reach, so a chosen day
//!   stages the edit there, while the row panel's field is what commits.

use std::rc::Rc;

use floem::AnyView;
use floem::event::EventPropagation;
use floem::prelude::*;
use floem::style::FlexWrap;

use schemaic_core::celledit::{self, CellEditor};
use schemaic_core::date::{self, Date};

use crate::widgets::{MenuEntry, MenuFlags, MenuId, MenuInset, menu_inset};
use crate::{DatePick, FieldCfg, PopupAnchor, edit_field, icons, theme};

/// What a control needs to put a menu up: every menu flag in the app, where to
/// anchor this one, and the panel's minimum width.
///
/// It carries the whole [`MenuFlags`] rather than just the channel it fills
/// because a trigger owes two things, not one — fill `menus.popup`, and close
/// everything else, since it swallows the press the workspace root would have
/// closed them on (see [`MenuId`]). `Copy`, like the `GridState` it is built
/// from.
#[derive(Clone, Copy)]
pub(crate) struct PopupChannel {
    /// Reached as `menus.popup` — the field name is what
    /// `widgets::popup_anchor_gate` scans for (`popup.set(Some(`), so this
    /// opener is on its list too.
    pub menus: MenuFlags,
    pub anchor: RwSignal<Option<PopupAnchor>>,
    pub width: RwSignal<f64>,
}

/// The field-like surface a picker box and the calendar toggle wear — the same
/// chrome [`crate::edit_field`] draws, so a row of mixed controls reads as one
/// set.
fn field_box(s: floem::style::Style) -> floem::style::Style {
    s.items_center()
        .height(crate::consts::field_input_h())
        .padding_horiz(theme::scaled(8.0))
        .background(theme::bg_editor())
        .border(1.0)
        .border_color(theme::field_border())
        .border_radius(6.0)
        .hover(|s| s.border_color(theme::field_border_active()))
}

// ── Pickers (boolean, enum, set) ────────────────────────────────────────────

/// The menu rows for `editor`'s options, tinted where the value already holds
/// one ([`MenuEntry::action_colored`]'s stated purpose — it needs no icon column,
/// and for a `SET` *every* held member is tinted because every one of them is).
///
/// `pick` is handed the option's **value**, which is not its label: a boolean's
/// row reads `true` and writes the engine's own spelling, and a `SET`'s row
/// writes the whole value with that member toggled.
pub(crate) fn pick_entries(
    editor: &CellEditor,
    current: &str,
    pick: Rc<dyn Fn(&str)>,
) -> Vec<MenuEntry> {
    celledit::pick_options(editor, current)
        .into_iter()
        .map(|o| {
            let pick = pick.clone();
            let act = move || (pick)(&o.value);
            if o.held {
                MenuEntry::action_colored(o.label, theme::accent, act)
            } else {
                MenuEntry::action(o.label, act)
            }
        })
        .collect()
}

/// Open (or close) a picker's menu under the control that raised it.
///
/// `anchor` is the control's **own** view id, whose `layout_rect` floem already
/// keeps in window coordinates — the frame [`PopupAnchor`] is stated in. Reached
/// by Tab, a control has no cursor to open at, and the shared channel's fallback
/// is `last_mouse`: without this the menu opened wherever the pointer was left.
///
/// A second press closes what the first opened, and the "is that mine?" test is
/// **recomputed** rather than remembered: these controls sit in a scrolling
/// panel, so with a menu up the control may have moved, and then the press
/// reopens at the new position — which is the better answer anyway.
pub(crate) fn open_picker(
    ch: PopupChannel,
    anchor: Option<floem::ViewId>,
    width: f64,
    entries: Vec<MenuEntry>,
) {
    let here = anchor
        .map(|id| id.layout_rect())
        .map(|r| PopupAnchor::BelowBox(r.x0, r.x1, r.y1));
    if here.is_some_and(|mine| {
        crate::widgets::menu_anchored_at(
            ch.menus.popup.get_untracked().is_some(),
            ch.anchor.get_untracked(),
            mine,
        )
    }) {
        ch.menus.popup.set(None);
        return;
    }
    // This trigger absorbs its own pointer-down, so the workspace root's
    // `close_except(None)` never runs for it and every *other* menu — the schema
    // tree's eye, a calendar, the connection switcher — would be left on screen
    // beside this one. A stranded `menu_panel` keeps its `focus_root` registered,
    // which is how a new query tab ends up declining the keyboard.
    ch.menus.close_except(Some(MenuId::Popup));
    ch.anchor.set(here);
    // At least as wide as the control it drops from, so the menu doesn't read as
    // a different control than the one that opened it.
    ch.width.set(width.max(theme::scaled(150.0)));
    ch.menus.popup.set(Some(entries));
}

/// Width of a picker box in the row panel — wide enough for an enum's longest
/// member without stretching to the field column's whole width.
///
/// A `fn`, and *called inside* the style closure rather than resolved into one:
/// a captured number cannot re-run when the interface scale changes, so the box
/// kept its old width while the type inside it grew (`themes::UiScale`'s rule).
fn pick_field_w() -> f64 {
    theme::scaled(220.0)
}

/// A `<select>`-shaped box bound to `buf`, offering `editor`'s options through
/// the shared popup menu (see the module doc for why it is a menu and not a
/// `Dropdown`). Used by the row panel for booleans and enums alike.
///
/// **Reachable by keyboard, like the text field it stands in for.** A control the
/// Tab order walks past is a column that cannot be set without a mouse — and this
/// one *replaces* the field for its column, so there is nothing else to reach.
/// Three parts to that, none of them optional:
///
/// * `keyboard_navigable`, which puts it in floem's own Tab walk **and** is what
///   makes Enter/Space work: floem fires [`EventListener::Click`] on the focused
///   view for either key (`context.rs`), so the same handler the pointer uses
///   opens the menu. Once open, the menu takes the keyboard itself and walks with
///   the arrows.
/// * `autofocus`, because the row panel focuses its first editable field on open,
///   and a panel whose first column is an `ENUM` opened with the keyboard
///   *nowhere* — the arrows still driving the grid behind it.
/// * `on_escape`, the same contract `edit_field` gives: Escape closes the row
///   panel. With the menu up it never reaches here, the panel being the focused
///   view then and Escape peeling one layer at a time.
pub(crate) fn pick_field(
    buf: RwSignal<String>,
    editor: CellEditor,
    ch: PopupChannel,
    autofocus: bool,
    on_escape: Option<Rc<dyn Fn()>>,
) -> AnyView {
    // The box's own id, so the menu opens under the box rather than at the
    // pointer — which is where it was left when the control was reached by Tab.
    // Filled once the view below exists.
    let anchor_id: RwSignal<Option<floem::ViewId>> = RwSignal::new(None);
    let for_open = editor.clone();
    let open = move || {
        let entries = pick_entries(
            &for_open,
            &buf.get_untracked(),
            Rc::new(move |v: &str| buf.set(v.to_string())),
        );
        open_picker(ch, anchor_id.get_untracked(), pick_field_w(), entries);
    };
    let boxed = h_stack((
        dyn_container(
            move || celledit::held_label(&editor, &buf.get()),
            move |v| {
                let unset = v.is_empty();
                text(if unset { "Choose…".to_string() } else { v })
                    .style(move |s| {
                        s.font_size(theme::font_body())
                            .text_ellipsis()
                            .min_width(0.0)
                            .color(if unset {
                                theme::placeholder()
                            } else {
                                theme::text()
                            })
                    })
                    .into_any()
            },
        )
        .style(|s| s.flex_grow(1.0_f32).min_width(0.0)),
        icons::icon(icons::CHEVRON_DOWN, 14.0)
            .style(|s| s.color(theme::text_dim()).flex_shrink(0.0_f32)),
    ))
    // Enter and Space arrive here too — floem synthesises a `Click` on the
    // focused view for both, which is the whole of this control's keyboard
    // activation.
    .on_click_stop(move |_| open())
    // Without this the workspace root's close-on-pointer-down fires first and
    // the click then reopens: down closes, up reopens, and the box never toggles.
    .on_event_stop(
        floem::event::EventListener::PointerDown,
        crate::widgets::menu_trigger_press,
    )
    .on_event(floem::event::EventListener::KeyDown, move |e| {
        escape(e, &on_escape)
    })
    .keyboard_navigable()
    // The app's own ring, gated on `keyboard_nav` — so it marks a control the
    // keyboard reached and stays dark on a click, which is the only time it says
    // anything. The radius is `field_box`'s, or floem strokes a square ring
    // around a rounded box.
    .style(|s| {
        crate::widgets::button_focus_ring(field_box(s), 6.0)
            .width(pick_field_w())
            .gap(theme::scaled(6.0))
    });
    anchor_id.set(Some(boxed.id()));
    focus_on_mount(autofocus, anchor_id);
    boxed.into_any()
}

/// Answer Escape with `on_escape`, if there is one — the contract
/// [`crate::edit_field`] gives, worn by the controls that stand in for it.
///
/// A `fn` rather than a closure per control: `EventPropagation::Stop` on the key
/// is the load-bearing half (the row panel's own handler would otherwise close it
/// twice over), and it is the half easiest to leave out.
fn escape(e: &floem::event::Event, on_escape: &Option<Rc<dyn Fn()>>) -> EventPropagation {
    let floem::event::Event::KeyDown(ke) = e else {
        return EventPropagation::Continue;
    };
    if !matches!(
        ke.key.logical_key,
        floem::keyboard::Key::Named(floem::keyboard::NamedKey::Escape)
    ) {
        return EventPropagation::Continue;
    }
    match on_escape {
        Some(esc) => {
            (esc)();
            EventPropagation::Stop
        }
        None => EventPropagation::Continue,
    }
}

/// Take the keyboard on mount, one tick late, if this control is the row panel's
/// first editable field.
///
/// Deferred for [`crate::edit_field`]'s reason (the view has to exist), and read
/// through `try_get_untracked` for its other one: the control may be disposed
/// before the timer fires — an overlay opened and closed in the same tick — and
/// focusing a `ViewId` that is gone leaves the window's focus pointing at nothing,
/// which is keyboard-dead rather than merely wrong. A disposed signal answers
/// `None`, so the id is only ever asked for while its owner is alive.
fn focus_on_mount(autofocus: bool, id: RwSignal<Option<floem::ViewId>>) {
    if !autofocus {
        return;
    }
    floem::action::exec_after(std::time::Duration::ZERO, move |_| {
        if let Some(Some(id)) = id.try_get_untracked() {
            id.request_focus();
        }
    });
}

/// The **in-cell** face of a picker: the value and a chevron, filling the cell on
/// the edit surface, while the menu it opened stands over the grid.
///
/// Edge to edge — no padding of its own beyond the text inset, and the cell drops
/// its own while this is up (`grid::data_cell`), so the control reads as the cell
/// rather than as a box sitting inside one.
pub(crate) fn pick_cell_face(buf: RwSignal<String>, editor: CellEditor) -> AnyView {
    h_stack((
        dyn_container(
            move || celledit::held_label(&editor, &buf.get()),
            move |v| {
                let unset = v.is_empty();
                text(if unset { "Choose…".to_string() } else { v })
                    .style(move |s| {
                        s.font_size(theme::font_body())
                            .text_ellipsis()
                            .min_width(0.0)
                            // `text_faint`, not the field `placeholder`: this one
                            // sits on the edit surface, where the placeholder
                            // colour manages 1.5:1 — see `contrast::UI_PAIRS`.
                            .color(if unset {
                                theme::text_faint()
                            } else {
                                theme::text()
                            })
                    })
                    .into_any()
            },
        )
        .style(|s| s.flex_grow(1.0_f32).min_width(0.0)),
        icons::icon(icons::CHEVRON_DOWN, 12.0)
            .style(|s| s.color(theme::text_dim()).flex_shrink(0.0_f32)),
    ))
    .style(|s| {
        s.flex_row()
            .width_full()
            .height_full()
            .items_center()
            .gap(theme::scaled(4.0))
            .padding_horiz(crate::consts::grid_pad_h())
            .background(theme::control_bg())
    })
    .into_any()
}

// ── Set ─────────────────────────────────────────────────────────────────────

/// A chip's corner — a pill, and the radius the focus ring has to follow: floem
/// strokes an outline at the *painting* view's radius, so a chip and its ring
/// disagreeing is a square drawn around a pill.
const CHIP_RADIUS: f64 = 10.0;

/// One chip per member of a `SET`, lit when the value holds it. Clicking toggles
/// through [`celledit::pick_options`], whose value for a member is the whole
/// `SET` with that member flipped — which is what keeps the result in the
/// engine's declaration order.
///
/// **Each chip is its own Tab stop**, and a stop per member is the right shape
/// here rather than one stop with an inner cursor: a `SET` is a row of
/// independent toggles, which is what a checkbox group is, and Enter/Space on the
/// focused one flips it through the very handler the pointer uses (floem
/// synthesises the `Click`). `autofocus` lands on the first member for the same
/// reason [`pick_field`] takes it — the row panel focuses its first editable
/// field, and a `SET` column that swallowed that focus request left the panel
/// keyboard-dead.
pub(crate) fn set_control(
    buf: RwSignal<String>,
    editor: CellEditor,
    autofocus: bool,
    on_escape: Option<Rc<dyn Fn()>>,
) -> AnyView {
    // The first chip's id, for `autofocus`. Filled below, read one tick later.
    let first_id: RwSignal<Option<floem::ViewId>> = RwSignal::new(None);
    let chips: Vec<AnyView> = celledit::pick_options(&editor, "")
        .into_iter()
        .enumerate()
        .map(|(i, o)| {
            let on_escape = on_escape.clone();
            let (name, ed) = (o.label, editor.clone());
            let held = {
                let name = name.clone();
                move || {
                    buf.with(|v| {
                        celledit::pick_options(&ed, v)
                            .into_iter()
                            .any(|o| o.label == name && o.held)
                    })
                }
            };
            let toggle = {
                let (name, ed) = (name.clone(), editor.clone());
                move || {
                    let next = celledit::pick_options(&ed, &buf.get_untracked())
                        .into_iter()
                        .find(|o| o.label == name)
                        .map(|o| o.value);
                    if let Some(v) = next {
                        buf.set(v);
                    }
                }
            };
            let chip = text(name)
                .on_click_stop(move |_| toggle())
                .on_event(floem::event::EventListener::KeyDown, move |e| {
                    escape(e, &on_escape)
                })
                .keyboard_navigable()
                .style(move |s| {
                    let s = crate::widgets::button_focus_ring(s, CHIP_RADIUS)
                        .padding_horiz(theme::scaled(8.0))
                        .padding_vert(theme::scaled(3.0))
                        .border(1.0)
                        .font_size(theme::font_body());
                    if held() {
                        s.background(theme::control_bg())
                            .border_color(theme::accent())
                            .color(theme::accent())
                    } else {
                        s.background(theme::bg_editor())
                            .border_color(theme::field_border())
                            .color(theme::text_dim())
                            .hover(|s| {
                                s.color(theme::text())
                                    .border_color(theme::field_border_active())
                            })
                    }
                });
            if i == 0 {
                first_id.set(Some(chip.id()));
            }
            chip.into_any()
        })
        .collect();
    focus_on_mount(autofocus, first_id);
    h_stack_from_iter(chips)
        .style(|s| {
            s.flex_row()
                .flex_wrap(FlexWrap::Wrap)
                .width_full()
                .gap(theme::scaled(6.0))
                .padding_vert(theme::scaled(2.0))
        })
        .into_any()
}

// ── Date / datetime ─────────────────────────────────────────────────────────

/// Width of one day cell in the calendar — seven of them plus the panel's
/// padding is the panel's width.
fn day_w() -> f64 {
    theme::scaled(30.0)
}

/// Height of one day cell.
fn day_h() -> f64 {
    theme::scaled(24.0)
}

/// The calendar panel's own size, computed rather than measured: the panel is a
/// fixed grid, and [`crate::overlays::date_pick_overlay`] needs the real numbers
/// to decide which way it flips at a window edge. An estimate there is a panel
/// that lands a few pixels off, or off-screen entirely.
pub(crate) fn calendar_size() -> (f64, f64) {
    let pad = 8.0 * 2.0 + 2.0; // padding both sides + the 1px border both sides
    let rows = day_h() * 6.0;
    let chrome = theme::scaled(22.0) * 2.0 + theme::scaled(16.0); // header, footer, weekday row
    let gaps = 4.0 * 3.0;
    (day_w() * 7.0 + pad, rows + chrome + gaps + pad)
}

/// A text field bound to `buf` with a calendar toggle beside it. The calendar
/// itself is an **overlay** ([`DatePick`] → `date_pick_overlay`), so it is
/// clipped by neither the row panel's scroll nor the results area, and flips at
/// the window's edges.
///
/// `editor` decides what a picked day *writes* — [`celledit::set_date`] keeps a
/// datetime's time of day, its fraction and its offset — and whether the footer
/// offers **Now** (a datetime) or **Today** (a date).
pub(crate) fn date_control(
    buf: RwSignal<String>,
    editor: CellEditor,
    autofocus: bool,
    on_escape: Option<Rc<dyn Fn()>>,
    menus: MenuFlags,
) -> AnyView {
    let pick = menus.date_pick;
    // **Is the open calendar this field's?** One channel serves every date field
    // in the panel, and asking only whether *a* calendar is open answered yes for
    // all of them: `created_at`'s open panel lit `updated_at`'s button too, and
    // `updated_at`'s teardown closed `created_at`'s panel. The buffer is the
    // identity — the anchor moves when the panel scrolls, and a control being
    // disposed no longer has a rect to compare.
    let mine = move || pick.with_untracked(|p| p.as_ref().is_some_and(|d| d.buf == buf));
    let field = edit_field(
        buf,
        FieldCfg {
            background: theme::bg_editor,
            font_size: theme::font_body,
            autofocus,
            height: Some(crate::consts::field_input_h),
            on_escape,
            ..Default::default()
        },
    )
    .style(|s| s.flex_grow(1.0_f32).min_width(0.0));

    let anchor_id: RwSignal<Option<floem::ViewId>> = RwSignal::new(None);
    let toggle = container(icons::icon(icons::CALENDAR, 15.0))
        .on_click_stop(move |_| toggle_calendar(menus, anchor_id.get_untracked(), buf, &editor))
        // The same bargain a menu trigger makes: the root closes the calendar on
        // any pointer-down, so an unswallowed press would close what this click
        // is about to open.
        .on_event_stop(
            floem::event::EventListener::PointerDown,
            crate::widgets::menu_trigger_press,
        )
        .style(move |s| {
            let s = field_box(s)
                .width(crate::consts::field_input_h())
                .justify_center()
                .padding(0.0);
            // Tracked, so the button un-lights when the calendar closes.
            if pick.with(|p| p.as_ref().is_some_and(|d| d.buf == buf)) {
                s.color(theme::accent()).border_color(theme::accent())
            } else {
                s.color(theme::text_dim()).hover(|s| s.color(theme::text()))
            }
        });
    anchor_id.set(Some(toggle.id()));

    h_stack((field, toggle))
        // **The calendar cannot outlive the field.** It edits `buf`, which belongs
        // to this field's scope — a closed row panel, a stepped row, a switched
        // tab all dispose it, and a panel still reading a disposed signal is a
        // panic in a style closure rather than a stale value. The overlay checks
        // too (`try_get`), but only when it rebuilds; this is what fires when
        // nothing rebuilds it.
        .on_cleanup(move || {
            if mine() {
                pick.set(None);
            }
        })
        .style(|s| s.width_full().items_center().gap(theme::scaled(6.0)))
        .into_any()
}

/// Open the calendar under `anchor`, or close it if it is already this control's.
///
/// Recomputed rather than remembered, for [`open_picker`]'s reason: the control
/// may have moved under an open panel, and reopening at the new place is the
/// better answer than refusing to close.
pub(crate) fn toggle_calendar(
    menus: MenuFlags,
    anchor: Option<floem::ViewId>,
    buf: RwSignal<String>,
    editor: &CellEditor,
) {
    if menus
        .date_pick
        .with_untracked(|p| p.as_ref().is_some_and(|d| d.buf == buf))
    {
        menus.date_pick.set(None);
        return;
    }
    // The row panel's field is what commits there, so a picked day only writes
    // the buffer — see [`DatePick::on_pick`].
    open_calendar(menus, anchor, buf, editor, None);
}

/// Fill the channel: the calendar for `buf`, dropped from `anchor`'s rect,
/// replacing whatever was up. The one place a [`DatePick`] is built.
///
/// A no-op when `anchor` has **no laid-out rect**, which is not the same test as
/// "no id": `ViewId::layout_rect` answers `Rect::ZERO` for a view that has never
/// been through layout rather than answering nothing, so the emptiness has to be
/// asked about explicitly — an unasked question here is a panel in the window's
/// top-left corner, and a caller reading "did it open?" from the channel gets
/// `true` for it. It is why the grid's cell editor opens this one tick late.
pub(crate) fn open_calendar(
    menus: MenuFlags,
    anchor: Option<floem::ViewId>,
    buf: RwSignal<String>,
    editor: &CellEditor,
    on_pick: Option<Rc<dyn Fn()>>,
) {
    let Some(rect) = anchor
        .map(|id| id.layout_rect())
        .filter(|r| r.width() > 0.0 && r.height() > 0.0)
    else {
        return;
    };
    // The opener absorbs its own press, so nothing else will close the menus
    // this is opening over — see `open_picker` for the whole bargain.
    menus.close_except(Some(MenuId::DatePick));
    // No panel is up, so no press of one is outstanding. Nothing should be able
    // to leave a stale one behind ([`take_calendar_press`] takes it), but a flag
    // read by a *different* view than the one that sets it is worth resetting at
    // the one moment its answer is known.
    take_calendar_press();
    menus.date_pick.set(Some(DatePick {
        buf,
        editor: editor.clone(),
        anchor: (rect.x0, rect.x1, rect.y1),
        on_pick,
    }));
}

thread_local! {
    /// Set by [`calendar_panel`]'s own pointer-down swallow, taken by whoever
    /// asks. See [`take_calendar_press`].
    static CALENDAR_PRESS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// **Did the calendar just take a pointer press?** Taken, not read: the answer is
/// about the event being dispatched right now, and it must not survive it.
///
/// This exists because floem takes the window focus on *every* pointer-down and
/// hands it back only to a focusable view under the cursor — a day, a month arrow
/// and the Now button are none of them. A field the panel drops from therefore
/// sees a click in the panel and a press of Escape as the same thing, a
/// `FocusLost`, and the two have to mean opposite things: the first must leave the
/// editor alone (the pick is about to land in it), the second must close it. The
/// panel says which one this is, because it is the only one that knows.
///
/// The panel's own press arrives *before* the focus change it causes — floem
/// clears the focus, dispatches, and only then emits `FocusLost` — so a flag set
/// during dispatch is readable by the handler that follows it, in that order and
/// no other.
pub(crate) fn take_calendar_press() -> bool {
    CALENDAR_PRESS.replace(false)
}

/// The month grid itself: header (‹ month year ›), weekday initials, six weeks of
/// days, and a footer that jumps to today. Rendered by the overlay, which owns
/// the placement; this owns only what it looks like and what a click writes.
pub(crate) fn calendar_panel(pick: DatePick, close: Rc<dyn Fn()>) -> AnyView {
    let DatePick {
        buf,
        editor,
        on_pick,
        ..
    } = pick;
    // **Closing after a value was written**, which is not the same thing as
    // closing: Escape and a click away arrive at `close` too, and the opener's
    // `on_pick` must not run for those. Every path that writes `buf` ends here
    // instead — the day cells and the Now/Today footer — so the two cannot drift.
    let done: Rc<dyn Fn()> = match on_pick {
        Some(picked) => {
            let close = close.clone();
            Rc::new(move || {
                (picked)();
                (close)();
            })
        }
        None => close,
    };
    let today = Date::today();
    // The month on show, seeded from the value the field holds when it opened.
    let focus = celledit::picker_focus(&buf.get_untracked(), today);
    let shown: RwSignal<(i32, u32)> = RwSignal::new((focus.year, focus.month));
    let step = move |months: i32| {
        let (y, m) = shown.get_untracked();
        let Some(first) = Date::new(y, m, 1) else {
            return;
        };
        let moved = first.add_months(months);
        shown.set((moved.year, moved.month));
    };
    let arrow = move |icon: &'static str, months: i32| {
        container(icons::icon(icon, 14.0))
            .on_click_stop(move |_| step(months))
            .style(|s| {
                s.padding(theme::scaled(3.0))
                    .border_radius(4.0)
                    .color(theme::text_dim())
                    .hover(|s| s.color(theme::text()).background(theme::row_hover()))
            })
            .into_any()
    };
    let header = h_stack((
        arrow(icons::CHEVRON_LEFT, -1),
        dyn_container(
            move || shown.get(),
            move |(y, m)| {
                text(format!("{} {y}", date::month_name(m)))
                    .style(|s| s.font_size(theme::font_body()).color(theme::text()))
                    .into_any()
            },
        )
        .style(|s| s.flex_grow(1.0_f32).items_center().justify_center()),
        arrow(icons::CHEVRON_RIGHT, 1),
    ))
    .style(move |s| s.width_full().height(theme::scaled(22.0)).items_center());

    let headings = h_stack_from_iter(date::WEEKDAY_INITIALS.iter().map(|d| {
        text(*d)
            .style(|s| {
                s.width(day_w())
                    .justify_center()
                    .font_size(theme::font_hint())
                    .color(theme::text_faint())
            })
            .into_any()
    }))
    .style(move |s| s.flex_row().height(theme::scaled(16.0)));

    // Rebuilt per month rather than per day-cell, so paging doesn't leave 42
    // reactive closures each re-deciding which month they are in.
    let (ed, closer) = (editor.clone(), done.clone());
    let grid = dyn_container(
        move || shown.get(),
        move |(y, m)| {
            let (ed, closer) = (ed.clone(), closer.clone());
            let rows: Vec<AnyView> = date::month_cells(y, m)
                .chunks(7)
                .map(|week| {
                    let days: Vec<AnyView> = week
                        .iter()
                        .map(|d| day_cell(*d, m, today, buf, ed.clone(), closer.clone()))
                        .collect();
                    h_stack_from_iter(days).style(|s| s.flex_row()).into_any()
                })
                .collect();
            v_stack_from_iter(rows).into_any()
        },
    );

    let picks_time = editor == CellEditor::DateTime;
    let now_label = if picks_time { "Now" } else { "Today" };
    let footer = h_stack((
        empty().style(|s| s.flex_grow(1.0_f32)),
        text(now_label)
            .on_click_stop(move |_| {
                // Read **now**, not the `today` this panel was built with: a
                // calendar left open across local midnight would otherwise write
                // yesterday's date beside the current time — a stamp a day in the
                // past, from the one button whose whole job is "the current
                // instant". And read it **once**, which is the same bug at a
                // smaller scale: two readings straddle that midnight too.
                let (date, time, offset) = date::local_now();
                buf.set(celledit::set_now(
                    &editor,
                    &buf.get_untracked(),
                    (date, time, &offset),
                ));
                (done)();
            })
            .style(|s| {
                s.padding_horiz(theme::scaled(8.0))
                    .padding_vert(theme::scaled(2.0))
                    .border_radius(4.0)
                    .font_size(theme::font_body())
                    .color(theme::accent())
                    .hover(|s| s.background(theme::row_hover()))
            }),
    ))
    .style(move |s| s.width_full().height(theme::scaled(22.0)).items_center());

    let (w, h) = calendar_size();
    v_stack((header, headings, grid, footer))
        // A click inside the panel is not a click away — the root's dismissal
        // handler must not see it (the rule every menu panel follows). Nor is it
        // the field it dropped from being left, which is the other thing that has
        // to be told apart from a press here ([`take_calendar_press`]). Every
        // press inside the panel arrives here: the days and the footer act on
        // `Click`, which is a pointer-*up*, so the down still bubbles to this.
        .on_event_stop(floem::event::EventListener::PointerDown, |_| {
            CALENDAR_PRESS.set(true)
        })
        .style(move |s| {
            s.gap(theme::scaled(4.0))
                .padding(theme::scaled(8.0))
                .width(w)
                .height(h)
                .background(theme::bg_panel())
                .border(1.0)
                .border_color(theme::border())
                .border_radius(8.0)
        })
        .into_any()
}

/// One day in the month grid: the number, dimmed when it belongs to a
/// neighbouring month, ringed when it is today, filled when it is the value's own
/// day. Clicking it writes through [`celledit::set_date`] and then runs `done` —
/// the panel's *value was written* exit, which closes it and, for the grid's cell
/// editor, stages the edit (see [`calendar_panel`]).
fn day_cell(
    day: Date,
    month: u32,
    today: Date,
    buf: RwSignal<String>,
    editor: CellEditor,
    done: Rc<dyn Fn()>,
) -> AnyView {
    let ed = editor.clone();
    text(day.day.to_string())
        .on_click_stop(move |_| {
            buf.set(celledit::set_date(&ed, &buf.get_untracked(), day));
            (done)();
        })
        .style(move |s| {
            // Tracked: typing in the field moves the selection with it.
            let picked = buf.with(|v| celledit::value_date(v)) == Some(day);
            let s = s
                .width(day_w())
                .height(day_h())
                .items_center()
                .justify_center()
                .border(1.0)
                .border_radius(4.0)
                .font_size(theme::font_body());
            let s = if day == today {
                s.border_color(theme::accent())
            } else {
                s.border_color(floem::peniko::Color::TRANSPARENT)
            };
            // **The picked day has no hover state at all.** A hover fill is a
            // *state* style in floem and wins over the base background, so the
            // one day already filled with the accent turned grey under the
            // pointer — the selection reading as un-selected on the only cell you
            // are pointing at. Nothing is being offered by hovering it either: it
            // is where the value already is.
            if picked {
                // The app's **active pill** — a saturated blue fill with text
                // picked to be legible on it, in both themes, and already
                // measured (`contrast::UI_PAIRS`, "designer tabs: the active
                // pill"). An `accent` fill under `bg_deepest` text was the
                // obvious spelling and lands at 3.87:1 in Light, under AA.
                s.background(theme::pill_active_bg())
                    .color(theme::pill_active_text())
            } else if day.month == month {
                s.color(theme::text())
                    .hover(|s| s.background(theme::row_hover()))
            } else {
                s.color(theme::text_faint())
                    .hover(|s| s.background(theme::row_hover()))
            }
        })
        .into_any()
}

/// Where the calendar goes: left edge on the control's, dropping below it —
/// flipped to right-aligned at the window's right edge and upward at its bottom,
/// which is the placement rule [`PopupAnchor::BelowBox`] states for a menu.
///
/// Pure, and the panel's size is exact rather than estimated ([`calendar_size`]),
/// so the flip is a fact rather than a guess: this is the whole reason a fixed
/// grid was worth building instead of letting the panel size itself.
pub(crate) fn calendar_insets(
    anchor: (f64, f64, f64),
    size: (f64, f64),
    win: (f64, f64),
) -> (f64, MenuInset) {
    let (left, right, bottom) = anchor;
    let ((w, h), (ww, wh)) = (size, win);
    let x = if ww > 1.0 && left + w > ww {
        (right - w).max(0.0)
    } else {
        left.max(0.0)
    };
    (x, menu_inset(bottom, h, wh, 4.0))
}

/// **Every control that stands in for a text field owes the keyboard the same
/// contract that field had.** A signature scan, in the style of
/// `widgets::menu_trigger_gate`, because what went wrong was not a wrong
/// calculation — it was two parameters that were never passed.
///
/// `typed_editor` hands the row panel's `autofocus` and its panel-closing Escape
/// to whichever control the column's type asks for. Two of the four ignored both,
/// and the result was a column that could not be set without a mouse: nothing
/// took the keyboard when the panel opened on an `ENUM`, and Tab walked past the
/// control as though it were a label.
///
/// **What it can and can't see.** It reads the parameter lists, so it catches a
/// fifth control added without them and it catches either being dropped from an
/// existing one — which is exactly how this arrived. It cannot see that the
/// control *uses* them; that half is `focus_on_mount` and the `escape` helper,
/// which exist so there is one implementation to read rather than four.
#[cfg(test)]
mod row_panel_focus_gate {
    /// The controls `grid::typed_editor` can build. `CellEditor::Text` is not
    /// here: that arm returns `scalar_editor`, the field itself.
    const CONTROLS: &[&str] = &["pick_field", "set_control", "date_control"];

    /// The source of this module, minus its own tests.
    fn source() -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("cell_editors.rs");
        let src = std::fs::read_to_string(path).expect("this module's own source");
        match src.find("#[cfg(test)]") {
            Some(i) => src[..i].to_string(),
            None => src,
        }
    }

    #[test]
    fn every_row_panel_control_takes_the_focus_and_the_escape() {
        let src = source();
        for name in CONTROLS {
            let at = src
                .find(&format!("fn {name}("))
                .unwrap_or_else(|| panic!("{name} is gone — the list above is stale"));
            // The parameter list: from the name to the return type.
            let sig = &src[at..];
            let end = sig.find("-> AnyView").expect("a view builder");
            let sig = &sig[..end];
            assert!(
                sig.contains("autofocus"),
                "{name} ignores the row panel's autofocus — a column whose \
                 control never takes the keyboard cannot be set without a mouse"
            );
            assert!(
                sig.contains("on_escape"),
                "{name} ignores the row panel's Escape — `edit_field` gives that \
                 contract and a control standing in for one owes it too"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: (f64, f64) = (230.0, 220.0);
    const WIN: (f64, f64) = (1000.0, 700.0);

    /// The mechanism the grid's `FocusLost` rule rests on, not the rule itself
    /// (that one lives in a view and no test here reaches it): a press is
    /// **spent** by the first asker. A flag that lingered would be handed to the
    /// next focus loss instead — which is Escape, the press already having been
    /// answered — and Escape would go on closing nothing.
    #[test]
    fn a_calendar_press_is_spent_once() {
        assert!(
            !take_calendar_press(),
            "no panel has been pressed in this thread"
        );
        CALENDAR_PRESS.set(true);
        assert!(take_calendar_press(), "the press the panel reported");
        assert!(
            !take_calendar_press(),
            "and it is gone — a second asker is a different event"
        );
    }

    #[test]
    fn a_calendar_drops_from_the_controls_left_edge() {
        let (x, y) = calendar_insets((100.0, 130.0, 40.0), SIZE, WIN);
        assert_eq!(x, 100.0);
        assert_eq!(y, MenuInset::Start(44.0), "4px below the control");
    }

    #[test]
    fn a_calendar_at_the_right_edge_is_right_aligned_on_the_control() {
        let (x, _) = calendar_insets((900.0, 930.0, 40.0), SIZE, WIN);
        assert_eq!(x, 700.0, "its right edge lands on the control's");
    }

    /// The row panel is a strip at the *bottom* of the results area, so this is
    /// the common case there rather than an edge case.
    #[test]
    fn a_calendar_with_no_room_below_grows_upward() {
        let (_, y) = calendar_insets((100.0, 130.0, 650.0), SIZE, WIN);
        // Expressed from the window's bottom so the panel's own height decides
        // where it starts — `menu_inset`'s rule.
        assert_eq!(y, MenuInset::End(700.0 - 650.0 + 4.0));
    }

    #[test]
    fn a_calendar_taller_than_the_window_pins_to_the_top() {
        let (_, y) = calendar_insets((100.0, 130.0, 500.0), (230.0, 900.0), WIN);
        assert_eq!(y, MenuInset::Start(0.0));
    }

    #[test]
    fn an_unmeasured_window_never_flips() {
        let (x, y) = calendar_insets((100.0, 130.0, 40.0), SIZE, (0.0, 0.0));
        assert_eq!(x, 100.0);
        assert_eq!(y, MenuInset::Start(44.0));
    }
}
