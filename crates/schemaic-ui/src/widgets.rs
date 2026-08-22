//! Reusable low-level UI widgets shared across the view modules: the themed
//! popup-menu system (`menu_panel` + `MenuEntry`), modal/panel style helpers, the
//! `window_size` global, and the auto-hiding / shift-scroll wrappers.
//!
//! These were defined late in `lib.rs` but used early throughout; collecting them
//! here lets the leaf view modules depend on them without an ordering deadlock.

use std::rc::Rc;
use std::time::Instant;

use floem::AnyView;
use floem::event::{Event, EventListener, EventPropagation};
use floem::keyboard::{Key, NamedKey};
use floem::kurbo::Rect;
use floem::prelude::*;
use floem::reactive::{Scope, create_effect};
use floem::style::Transition;
use floem::views::Scroll;
use floem::views::scroll::ScrollCustomStyle;

use crate::consts::*;
use crate::{icons, theme};

// ── Focus roots ─────────────────────────────────────────────────────────────
//
// Floem delivers a key event *directly* to the focused view and, when that view
// consumes it, to nobody else — a focused view's ancestors never see it, since
// the dispatch is `directed`. Floem's editor consumes every `KeyDown`, so while
// a text field has focus an enclosing modal's `on_key_down(Escape)` can't fire:
// the field swallows Escape and the modal has no way to close from the keyboard.
//
// So Escape in a field (one that hasn't claimed the key itself) hands focus back
// to the innermost mounted overlay, and the *next* Escape reaches that overlay's
// own handler — which is the two-step the user sees: blur, then close.
//
// A thread-local `Vec`, not a signal: this is UI-thread-only bookkeeping nothing
// renders from, and a signal would turn every open/close into a notification. A
// `Vec` rather than one slot because overlays nest (the DDL preview opens over
// the designer), and `retain` rather than `pop` because they don't always unmount
// innermost-first.
thread_local! {
    static FOCUS_ROOTS: std::cell::RefCell<Vec<(floem::ViewId, Option<FocusRing>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
    /// Where the keyboard goes when a focused control disappears and there is **no
    /// overlay above it** — see [`set_keyboard_home`].
    static KEYBOARD_HOME: std::cell::RefCell<Option<Rc<dyn Fn()>>> =
        const { std::cell::RefCell::new(None) };
}

/// Register the workspace's own "the keyboard lives here" action, for the case
/// [`innermost_focus_root`] cannot answer: a control removed while focused with no
/// modal open.
///
/// **The workspace is not a focus root, and making it one would not help.** A
/// modal's root carries the key handlers for its own contents, so handing focus
/// there restores the whole overlay's keyboard; the workspace's root carries
/// almost none — the arrows, `Del`, `Ctrl+Enter`, `Ctrl+F` and `F6` are all
/// listeners on the results grid's body. So the fallback has to name a *place*
/// rather than an ancestor, and the place is whatever the grid currently focuses
/// with (`refocus_grid`).
///
/// Without it, pressing the results toolbar's ✓ or ✗ **from the keyboard** left
/// nothing focused at all: the press removes the control that was pressed (the
/// commit block's `dyn_container` key changes), floem clears `app_state.focus`
/// silently when a focused view is removed, and the cleanup's
/// `hand_keyboard_back` had nowhere to hand to — from there the grid answered no
/// key, including the `F6` that would have got back into the strip, until the user
/// clicked a cell.
///
/// A closure rather than a `ViewId` because the answer has to be current at the
/// moment it is used: a grid rebuilds on every result, and a stored id from a
/// previous build names a view that no longer exists.
pub(crate) fn set_keyboard_home(home: Option<Rc<dyn Fn()>>) {
    KEYBOARD_HOME.with_borrow_mut(|h| *h = home);
}

/// Mark `view` as the overlay that owns the keyboard while it is mounted: it
/// takes focus on build (so its own key handlers fire straight away) and becomes
/// what Escape in a text field inside it returns focus to. Replaces the
/// `.keyboard_navigable().request_focus(|| {})` pair every modal used to spell
/// out — go through this instead, or the modal's Escape stops working the moment
/// a field is focused.
///
/// A field is *not* a focus root: the grid's inline cell editor takes focus the
/// same way and deliberately stays off this list. Don't chain your own
/// `.on_cleanup` onto the result either — floem keeps a single cleanup slot per
/// view, so a second one silently replaces the unregister.
pub(crate) fn focus_root<V: IntoView + 'static>(view: V) -> V::V {
    focus_root_inner(view, None)
}

fn focus_root_inner<V: IntoView + 'static>(view: V, ring: Option<FocusRing>) -> V::V {
    let view = view.into_view();
    let id = view.id();
    FOCUS_ROOTS.with_borrow_mut(|s| s.push((id, ring)));
    view.keyboard_navigable()
        .request_focus(|| {})
        .on_cleanup(move || {
            FOCUS_ROOTS.with_borrow_mut(|s| s.retain(|(x, _)| *x != id));
            // A root has no place in a ring, so there is nothing to remember —
            // the ring it *carried* went with it.
            hand_keyboard_back(None);
        })
}

/// **Hand the keyboard back** to the innermost mounted [`focus_root`], and
/// remember where in `ring` the walk had got to.
///
/// Floem clears `app_state.focus` when a focused view is removed and does it
/// *silently* — no `focus_changed`, so no `FocusGained` lands anywhere — and a
/// key event then goes to the focused view and, failing that, only to the window
/// root's own listeners. So closing a popup menu opened over a modal left the
/// modal underneath keyboard-dead: Escape did nothing, while its close button
/// and Cancel still worked.
///
/// The `remember` half is why this is one function rather than three copies of
/// two lines. Without it, `step_from` finds neither `from` (the root is not a
/// ring member) nor `last` (a removed `ViewId`), and `ring_step` returns 0 — so
/// Tab-ing to an enum value's ✕ and pressing Space removed the row and sent the
/// next Tab back to the **first** control in the modal. `FocusRing::focus_at`
/// already knew a captured id doesn't survive a rebuild and resolves by
/// tabindex; `last` was left as a raw id. The three sites that do this —
/// `focus_root`'s cleanup, `in_focus_ring_with`'s cleanup, and `edit_field`'s
/// Escape — spelled it out separately and **none** called `remember`, so
/// `git grep remember` found the gap in neither of the two that had a ring.
pub(crate) fn hand_keyboard_back(ring: Option<(&FocusRing, floem::ViewId)>) {
    if let Some((ring, leaving)) = ring {
        ring.remember(leaving);
    }
    if let Some(r) = innermost_focus_root() {
        r.request_focus();
        return;
    }
    // No overlay above: the workspace's own home, if it has registered one. This
    // is the half that used to be missing — see [`set_keyboard_home`] — and it is
    // why every site that hands the keyboard back can now do so without knowing
    // whether it is inside a modal.
    let home = KEYBOARD_HOME.with_borrow(|h| h.clone());
    if let Some(home) = home {
        home();
    }
}

/// A modal's click-outside-to-dismiss layer: a full-size, absolutely-positioned
/// sibling that sits **behind** the panel.
///
/// It exists so the dismiss listener is not on the [`focus_root`] itself. Floem
/// fires [`EventListener::Click`] on the **focused** view for any physical
/// Enter, NumpadEnter or Space, and a modal opens with focus on its own root —
/// so with `.on_click_stop(close)` chained onto that root, **Space closed the
/// modal**. Space is the reflex for "scroll this", and on the Live Monitor
/// closing also stops the poll and does `log.set(Vec::new())`, destroying every
/// change it had collected — deletes included, which is the one case where the
/// row is gone from the database and the log is the only remaining record.
///
/// Build it as the **first** child of the backdrop stack: floem dispatches a
/// pointer event to the first hit child in reverse paint order, so the panel,
/// added after, stays on top of it.
pub(crate) fn dismiss_layer(dismiss: impl Fn() + 'static) -> impl IntoView {
    empty()
        .on_click_stop(move |_| dismiss())
        .style(|s| s.absolute().inset(0.0))
}

/// The innermost mounted [`focus_root`], or `None` when no overlay is open (a
/// field in the main workspace then simply drops focus on Escape).
pub(crate) fn innermost_focus_root() -> Option<floem::ViewId> {
    FOCUS_ROOTS.with_borrow(|s| s.last().map(|(id, _)| *id))
}

/// The innermost mounted overlay that **has** a [`FocusRing`], as
/// `(root, ring)`, for the window root's Tab backstop.
///
/// With focus on a dropdown's popup list — or on nothing at all, which is what a
/// click on an unfocusable list row leaves behind — a Tab reaches neither a ring
/// member nor the modal's own root, and floem's fallback walks the *whole window
/// tree*. The root steps this ring instead.
///
/// **The innermost ring that exists, not the innermost root's.** Reading
/// `FOCUS_ROOTS.last()` and taking *its* ring meant a **ring-less** root on top
/// answered `None` and the backstop was skipped entirely: `menu_panel` registers
/// as one, so right-clicking a row in Manage Connections and pressing Tab sent
/// focus out into the workspace behind the modal — the one outcome the ring
/// exists to prevent. The pair is returned together so the root a step resumes
/// from is the root that ring's `remember` cursor belongs to.
pub(crate) fn innermost_ring_root() -> Option<(floem::ViewId, FocusRing)> {
    FOCUS_ROOTS.with_borrow(|s| {
        s.iter()
            .rev()
            .find_map(|(id, r)| r.clone().map(|r| (*id, r)))
    })
}

// ── Tab order ───────────────────────────────────────────────────────────────

/// Step a ring of `len` controls from `cur`, wrapping at both ends.
///
/// Wrapping rather than stopping is what makes the ring a *trap*: a modal's Tab
/// order must not walk out into the workspace behind it, and floem's own
/// traversal does exactly that — it iterates the whole window tree.
pub(crate) fn ring_step(len: usize, cur: Option<usize>, backwards: bool) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(match (cur, backwards) {
        (None, false) => 0,
        (None, true) => len - 1,
        (Some(i), false) => (i + 1) % len,
        (Some(i), true) => (i + len - 1) % len,
    })
}

/// A branch that wasn't built, as a view — for the else-arm of an
/// engine-conditional block inside a gapped stack.
///
/// `display: none`, not a bare `empty()`. This is the one place `hide()` is
/// still right: taffy excludes a `display:none` child from **gap** accounting
/// but counts a zero-sized one, so a plain `empty()` arm leaves a whole
/// [`FORM_GAP`] of dead space where the block would have been. The distinction
/// the range drew still holds — a *control* must never be built-and-hidden,
/// because a hidden view is still in the Tab ring — and nothing is hidden here:
/// there is nothing inside to reach.
pub(crate) fn nothing() -> floem::AnyView {
    empty().style(|s| s.hide()).into_any()
}

/// Where a modal's **navigation pane** sits — below every form control, because
/// it is what you use to choose *what* the form is editing: Manage Connections'
/// connection list, the designer's section strip. See [`nav_group`].
pub(crate) const NAV_TAB: u32 = 1;

/// Where a modal's **footer actions** start — Save, Cancel, Apply, Preview SQL,
/// Import, Back, Copy, Open in editor — one stop each, left to right.
///
/// Far above [`VALUE_TAB`] because the footer is the last thing in every modal
/// and a growing list must not be able to overtake it: a form that ends in a
/// hundred-thousand-row list is not a real case, but "the buttons come last" is
/// a rule, and a rule that holds only for short lists isn't one.
///
/// The gap is this wide because a row costs [`ROW_TAB_STRIDE`], not one index —
/// a test pins the arithmetic, and caught exactly this when the stride was
/// introduced and quietly cut the headroom tenfold.
pub(crate) const ACTION_TAB: u32 = 1_000_000;

/// The Tab stops one repeating row claims: its field(s) at the base, then its
/// buttons a little above them, so row *i* occupies
/// `VALUE_TAB + i * ROW_TAB_STRIDE ..= + ROW_TAB_STRIDE - 1` and can grow a
/// second field or a fourth button without renumbering its neighbours.
pub(crate) const ROW_TAB_STRIDE: u32 = 10;

/// Where a row's ↑ / ↓ / ✕ sit within its [`ROW_TAB_STRIDE`] block — after its
/// fields, which take the low half.
pub(crate) const ROW_BUTTON_TAB: u32 = 5;

/// The title bar's ✕ — **last** in the ring, past the footer.
///
/// It sits at the top-right on screen, but it is the same action the footer's
/// Cancel or Close already offers, so putting it first would mean every Tab
/// walk through a modal opened on "dismiss this". Last, it is the exit you
/// arrive at after everything the modal is for — and in the four footer-less
/// modals, where it is the only button, its position is moot.
pub(crate) const TITLE_CLOSE_TAB: u32 = ACTION_TAB + 100;

// The Tab order every modal in the app is laid out in, asserted at **compile
// time** rather than in a test, because it is arithmetic over constants and a
// violation should stop the build rather than wait for `cargo test`:
//
//   nav pane → its items → the form → a growing list of rows → the footer
//
// The last step is the one that needs guarding, because a growing block is
// unbounded upward and the footer must stay last. It has already been caught
// once: `ROW_TAB_STRIDE` made every row cost ten indices instead of one, which
// silently cut the headroom below `ACTION_TAB` tenfold on the day it landed.
/// The ceiling on a **fixed** form control's tabindex — the headroom the forms
/// have below [`VALUE_TAB`], where the growing blocks start.
///
/// The compile-time chain asserted `110 < VALUE_TAB` and called 110 "the highest
/// fixed control (a sequence's)". That was already false when it was written:
/// the **Settings** modal spaces its sections by hundreds and reaches 310, and
/// several forms pass 200. The number was harmless as a lower bound in a
/// constants-only assertion and stopped being harmless the moment something
/// checked a *registered* index against it — clicking Settings then panicked.
/// So it is stated as what it is, with room, and pinned by
/// `tests::every_band_the_app_uses_registers_cleanly` against the real indices.
pub(crate) const FIXED_TAB_END: u32 = 400;

const _: () = {
    assert!(NAV_TAB < crate::table_designer::LIST_TAB);
    assert!(
        crate::table_designer::LIST_TAB < 10,
        "before the first field"
    );
    assert!(
        FIXED_TAB_END < VALUE_TAB,
        "past the highest fixed control (a sequence's)"
    );
    // A row costs a whole stride, so the headroom is in rows, not indices.
    assert!(VALUE_TAB + 90_000 * ROW_TAB_STRIDE < ACTION_TAB);
    // A row's buttons sit above its own fields (a domain check has two) and
    // below the next row's, so Tab walks a value, then what you can do to it.
    assert!(1 < ROW_BUTTON_TAB);
    assert!(ROW_BUTTON_TAB + 2 < ROW_TAB_STRIDE);
};

/// A button in a modal's Tab order: focusable, reached by Tab, and pressed with
/// **Space or Enter**.
///
/// Both keys, unlike [`crate::settings::focusable_toggle`], which has to leave
/// Enter to floem's `ToggleButton`. Nothing here answers either key already, so
/// there is no second handler to double up with.
///
/// A **disabled** button is not a stop at all — it isn't registered and isn't
/// focusable, so Tab walks past it the way it walks past a label. It keeps its
/// place on screen (see [`action_button`]), which is a layout decision; being
/// skipped by the keyboard is the same answer the pointer already gives, since
/// its click handler is inert too.
///
/// There is deliberately **no default-Enter**: Enter in a field does not fire a
/// modal's affirmative action. The DDL preview's Apply is an irreversible
/// `ALTER`, and a key that means "newline" in one control and "apply the plan"
/// in another is the shape of defect this ring's own review was full of. Reach
/// the button and press it.
///
/// **The ring member is a wrapper this function builds, never the caller's own
/// view**, and that is a correctness rule rather than a layout preference. Two
/// things resolve by exact `ViewId` with no descendant propagation, and they
/// were resolving to *different* ids depending on the order each call site
/// happened to chain its decorators:
///
/// - Floem fires [`EventListener::Click`] on the **focused view** for any
///   physical Enter / NumpadEnter / Space (`context.rs`'s keyboard-trigger
///   path) and discards the result, then folds every registered `KeyDown`
///   listener without short-circuiting. So registering a view that already
///   carries `on_click_stop` made the arm below the *second* activation: one
///   Space added two columns, opened two file dialogs, started **two bulk
///   imports** of the same file.
/// - `.focus(…)` resolves by exact id too, so a caller that chained
///   `.tooltip()` (which allocates a fresh `ViewId`) before this call registered
///   an id that carries no [`button_focus_ring`] — every list-row ↑/↓/✕ was a
///   Tab stop that painted nothing.
///
/// A wrapper answers both at once: it never carries the caller's click listener,
/// so Space fires exactly once, and it carries the focus outline itself, so the
/// id in the ring is the id that paints. Callers therefore do **not** apply
/// `button_focus_ring` to the face they pass in — it would never fire there.
///
/// `radius` is the face's own corner radius, so the outline can follow it rather
/// than boxing it — see [`button_focus_ring`]. `0.0` for the icon buttons, whose
/// faces are square.
pub(crate) fn in_ring_button<V: IntoView + 'static>(
    view: V,
    ring: FocusRing,
    tabindex: u32,
    enabled: bool,
    radius: f64,
    on_press: impl Fn() + 'static,
) -> AnyView {
    // Built for both states, so enabling a button doesn't change its box: a
    // disabled action keeps its place in the footer (see [`action_button`]), and
    // it would not if one state had a flex item the other lacked.
    let wrapper = container(view).style(move |s| button_focus_ring(s, radius).flex_shrink(0.0_f32));
    if !enabled {
        return wrapper.into_any();
    }
    in_focus_ring(wrapper, ring, tabindex)
        .on_event(EventListener::KeyDown, move |e| {
            let Event::KeyDown(ke) = e else {
                return EventPropagation::Continue;
            };
            if matches!(
                ke.key.logical_key,
                Key::Named(NamedKey::Space) | Key::Named(NamedKey::Enter)
            ) {
                on_press();
                return EventPropagation::Stop;
            }
            EventPropagation::Continue
        })
        .into_any()
}

/// [`in_ring_button`] for a control in a **toolbar strip** rather than a modal
/// form — the results grid's icon cluster, which is the app's first ring outside
/// an overlay.
///
/// Two keys separate a strip from a modal's Tab order, and both are about the
/// strip being somewhere you *visit* rather than somewhere you are:
///
/// - **Left/Right step it**, because a horizontal row of icons is read that way.
///   Tab still works (that is [`in_focus_ring`]'s, and it wraps the same ring),
///   so neither reflex is wrong.
/// - **Escape calls `leave`**, which is how you get back out. A modal's Escape
///   hands the keyboard to the innermost [`focus_root`]; in the main workspace
///   there isn't one — `innermost_focus_root` is `None` and focus would simply be
///   dropped — so the strip has to name its own way home.
///
/// `leave` must **defer** its focus request (`exec_after(ZERO, …)`), because
/// `in_focus_ring`'s own Escape arm runs too — floem folds every `KeyDown`
/// listener without short-circuiting — and queues a `ClearFocus` for this pass.
/// A deferred request lands in a later tick and therefore wins, whichever order
/// the two listeners happen to run in. The grid's `refocus_grid` already works
/// this way, for the same reason.
pub(crate) fn in_strip_button<V: IntoView + 'static>(
    view: V,
    ring: FocusRing,
    tabindex: u32,
    enabled: bool,
    leave: impl Fn() + 'static,
    on_press: impl Fn() + 'static,
) -> AnyView {
    let button = in_ring_button(view, ring.clone(), tabindex, enabled, 0.0, on_press);
    if !enabled {
        return button; // not in the ring, so no key of ours can reach it
    }
    let id = button.id();
    button
        .on_event(EventListener::KeyDown, move |e| {
            let Event::KeyDown(ke) = e else {
                return EventPropagation::Continue;
            };
            match ke.key.logical_key {
                Key::Named(NamedKey::ArrowRight) => ring.step_from(id, false),
                Key::Named(NamedKey::ArrowLeft) => ring.step_from(id, true),
                Key::Named(NamedKey::Escape) => leave(),
                _ => return EventPropagation::Continue,
            }
            EventPropagation::Stop
        })
        .into_any()
}

/// Which arrows move inside a [`nav_group`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum NavAxis {
    /// A column of rows — Up/Down. The connection list.
    Vertical,
    /// A row of items — Left/Right. The designer's section strip.
    Horizontal,
}

/// A navigation pane that is **one** Tab stop, with the arrows moving inside it.
///
/// The rule the colour swatches and the designer's item list already follow: a
/// group of like things costs one stop, not N. A connection list of twenty as
/// twenty stops would make Tab-ing *past* it to the form cost twenty presses,
/// which is the common case — you are usually going somewhere else.
///
/// `step` takes ±1 and is the caller's, because clamping is not universal:
/// a *selection* clamps at its ends (jumping from the last item to the first is
/// only a surprise) while the Tab ring wraps, since wrapping is what stops Tab
/// leaving the modal. See [`list_step`] and [`ring_step`].
///
/// **A group shows no focus indication of its own**, unlike a
/// [button](in_ring_button). It is a container spanning a whole bar or pane, so
/// an outline around it reads as a stray border rather than as focus — and it
/// doesn't need one: what the arrows move is *already* highlighted, so the first
/// press says the group has the keyboard by moving the thing you were looking
/// at. A button has no such state and must say so itself.
///
/// Floem's default ring is still suppressed rather than left alone: it is a 3px
/// magenta outline belonging to no palette here, and while `FocusVisible` can't
/// fire from [`FocusRing`]'s own focus requests (see [`button_focus_ring`]),
/// floem latches `keyboard_navigation` globally once its *own* traversal has run
/// anywhere in the window.
pub(crate) fn nav_group<V: IntoView + 'static>(
    body: V,
    ring: FocusRing,
    tabindex: u32,
    axis: NavAxis,
    step: impl Fn(isize) + 'static,
) -> AnyView {
    let group = body
        .into_view()
        .style(|s| s.focus_visible(|s| s.outline(0.0)));
    in_focus_ring(group, ring, tabindex)
        .on_event(EventListener::KeyDown, move |e| {
            let Event::KeyDown(ke) = e else {
                return EventPropagation::Continue;
            };
            let (back, fwd) = match axis {
                NavAxis::Vertical => (NamedKey::ArrowUp, NamedKey::ArrowDown),
                NavAxis::Horizontal => (NamedKey::ArrowLeft, NamedKey::ArrowRight),
            };
            match &ke.key.logical_key {
                Key::Named(k) if *k == back => {
                    step(-1);
                    EventPropagation::Stop
                }
                Key::Named(k) if *k == fwd => {
                    step(1);
                    EventPropagation::Stop
                }
                _ => EventPropagation::Continue,
            }
        })
        .into_any()
}

/// The corner radius of a filled [`action_button`], and of the
/// [`control_surface`] family ([`control_button_enabled`], [`dialog_button`]).
///
/// Named because two views read each of them: the button's own face, and the
/// focus outline [`in_ring_button`] paints on the wrapper around it. Floem draws
/// an outline at *the painting view's* `border_radius` (`view::paint_outline`),
/// so a wrapper that doesn't know the face's radius draws a **square ring around
/// a rounded button** — which is what every ringed action button in the app did
/// until these were shared.
pub(crate) const ACTION_RADIUS: f64 = 5.0;
pub(crate) const CONTROL_RADIUS: f64 = 6.0;

/// The focus signal every ringed button wears: an **outline**, painted outside
/// the box, so gaining the keyboard costs no layout — the rule a swatch and a
/// list pane already follow, and the reason neither takes a border.
///
/// `radius` is the **face's** corner radius, and the wrapper takes it purely so
/// the outline can follow it: floem strokes the ring at the radius of the view
/// it paints on, plus half the stroke, which is exactly concentric when the two
/// agree and a square around a rounded chip when they don't. It sets nothing
/// visible on the wrapper itself, which has no fill and no border of its own.
///
/// Painted in `.focus`, **not** `.focus_visible`, and that is not a style
/// choice. Floem gates `FocusVisible` on `app_state.keyboard_navigation`, which
/// only its *own* `view_tab_navigation` ever sets — every path that reaches
/// `UpdateMessage::Focus`, which is all [`FocusRing`] has, leaves the flag
/// `false`. So a `focus_visible` rule on a ring member never fires at all, and
/// [`keyboard_nav`] is the app's own answer to the same question.
///
/// **Gating on it is what lets the ring be bright.** It used to be
/// `field_border_active` — `#303453` on the dark theme, a shade off the panel it
/// sits on — because anything legible was a distraction on a *mouse* click,
/// where a focus ring marks the thing you just pointed at and tells you nothing.
/// Drawn only when the keyboard put focus there, it is information again, so it
/// is `accent` now and actually findable at a glance.
pub(crate) fn button_focus_ring(s: floem::style::Style, radius: f64) -> floem::style::Style {
    let s = s.border_radius(radius);
    // Read reactively: this runs inside the caller's `.style` closure, so the
    // ring appears and disappears with the flag without rebuilding the button.
    if !keyboard_nav().get() {
        return s;
    }
    s.focus(|s| s.outline(2.0).outline_color(theme::accent()))
        // Floem's own is a 3px magenta ring belonging to no palette here.
        .focus_visible(|s| s.outline(2.0).outline_color(theme::accent()))
}

/// Where a **growing** block of Tab stops starts — a list of enum values, of
/// domain checks, of trigger arguments, of function settings — one stop per row
/// from here upwards.
///
/// Far above every fixed control on purpose. A block claiming `base + i` with a
/// dozen indices of headroom is safe only while it is the last thing in its
/// form: add one control after it and the 11th row collides, after which the two
/// order by registration (`register` inserts *after* an equal tabindex), so Tab
/// visits them in an order that depends on which happened to be built first. The
/// rule belongs beside [`FocusRing`] rather than in each editor, because the
/// hazard is the ring's, and a per-file constant is a per-file decision to get
/// right again.
pub(crate) const VALUE_TAB: u32 = 1000;

/// Step a *selection* of `len` items from `cur` by `delta`, **clamping** at both
/// ends. `None` when there is nowhere to go — an empty list, or a step that
/// would land where it started.
///
/// The deliberate counterpart to [`ring_step`]: the Tab ring wraps because
/// wrapping is what stops Tab escaping the modal, while a selection that jumps
/// from the last column to the first is only a surprise. Written as its own
/// function so the divergence is stated once and pinned by a test — and because
/// the clamp is where an empty list bites: `clamp(0, -1)` panics in debug, which
/// any table without CHECK constraints could reach.
pub(crate) fn list_step(len: usize, cur: usize, delta: isize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let next = (cur as isize + delta).clamp(0, len as isize - 1) as usize;
    (next != cur).then_some(next)
}

/// The Tab order over one modal's controls.
///
/// Schemaic has to own this rather than use floem's. Floem *does* have
/// `view_tab_navigation`, but it is `pub(crate)`, it walks the entire window
/// tree (so Tab would leave the modal), and — decisively — it only runs when
/// **nothing consumed the key**, while floem's text editor registers its
/// KeyDown listener with `on_event_stop` and so swallows every key including
/// Tab. A field with focus is exactly the case Tab has to work from.
///
/// Order is an explicit `tabindex`, not registration order, because a control
/// that appears later — the SSH block builds only once its toggle is on —
/// registers whenever it is switched on and would otherwise land at the end,
/// after the fields below it. Gaps are cheap; leave room between sections.
#[derive(Clone, Default)]
pub(crate) struct FocusRing {
    entries: Rc<std::cell::RefCell<Vec<(u32, floem::ViewId)>>>,
    /// Where the walk resumes when the key arrives somewhere that isn't a ring
    /// member — the modal's own root after an Escape, or the window root when
    /// focus is on a popup list or nowhere.
    ///
    /// Without it every such re-entry restarted at position 0, which made a
    /// `tab_indents` field (where Tab types an indent and Escape is the way out)
    /// a **trap**: every control after it in the ring was unreachable by forward
    /// Tab, because leaving the field always landed back at the top.
    ///
    /// A **tabindex**, not a `ViewId` — see [`FocusRing::remember`]. An id
    /// resolves only while the control it names is mounted, and the two commonest
    /// callers are a control that has just removed itself and one that has just
    /// been rebuilt.
    last: Rc<std::cell::Cell<Option<u32>>>,
}

/// Closes a control's open popup and returns the keyboard to it.
pub(crate) type PopupDismiss = Rc<dyn Fn()>;

/// Who owns the [`OPEN_POPUP`] slot. Handed out by [`popup_token`] and compared
/// on the way out, so a control can only ever clear *its own* entry.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct PopupToken(u64);

thread_local! {
    static OPEN_POPUP: std::cell::RefCell<Option<(PopupToken, PopupDismiss)>> =
        const { std::cell::RefCell::new(None) };
    static NEXT_POPUP_TOKEN: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// A fresh owner token, one per control that can put a popup up.
pub(crate) fn popup_token() -> PopupToken {
    NEXT_POPUP_TOKEN.with(|n| {
        let t = n.get();
        n.set(t + 1);
        PopupToken(t)
    })
}

/// Publish how to close the popup `token`'s control just opened.
///
/// Global rather than per-modal because only one such popup can be up at a time,
/// and — the deciding reason — Escape can only be answered from the *window
/// root*. A dropdown's popup takes the keyboard itself, so neither the box that
/// owns it nor the enclosing modal is the focused view; floem delivers a key to
/// the focused view and then only to listeners on the root, so the modal's own
/// Escape handler never runs while a popup is up.
///
/// One slot, but it carries **whose** popup it is: a control that publishes and
/// a control that withdraws are not the same control often enough to matter.
/// Floem queues B's open during dispatch and A's close at the end of the same
/// event, so clicking a second dropdown while the first is up ran
/// `set(Some(closeB))` and then A's clear — and an untagged clear emptied the
/// slot under B, after which Escape did nothing at all. The build-time run of
/// each dropdown's effect (with `open == false`) was a second way in.
pub(crate) fn set_open_popup(token: PopupToken, dismiss: PopupDismiss) {
    OPEN_POPUP.with_borrow_mut(|p| *p = Some((token, dismiss)));
}

/// Withdraw `token`'s entry, if the slot is still its. A no-op otherwise — see
/// [`set_open_popup`] for why that matters.
pub(crate) fn clear_open_popup(token: PopupToken) {
    OPEN_POPUP.with_borrow_mut(|p| {
        if p.as_ref().is_some_and(|(t, _)| *t == token) {
            *p = None;
        }
    });
}

/// Escape's **first** step: close an open popup and give the keyboard back to
/// the control that owns it. `true` when it did something, which is the root's
/// cue to stop there.
///
/// Escape peels one layer per press: this closes a popup, [`in_focus_ring`] then
/// blurs the control, and only then does the modal answer.
pub(crate) fn dismiss_open_popup() -> bool {
    let dismiss = OPEN_POPUP.with_borrow_mut(|p| p.take());
    match dismiss {
        Some((_, f)) => {
            f();
            true
        }
        None => false,
    }
}

impl FocusRing {
    pub(crate) fn new() -> FocusRing {
        FocusRing::default()
    }

    /// Add a control at `tabindex`, keeping the ring ordered. Re-registering the
    /// same view moves it rather than duplicating it.
    ///
    /// The one thing a single registration can be held to is the **ceiling**:
    /// nothing may sit past the title bar's ✕, which is last by construction.
    ///
    /// It deliberately does *not* police the band between the fixed controls and
    /// [`VALUE_TAB`], and the attempt to is worth recording. R2-L6-06 asks for a
    /// check that sees a real registered index, because the compile-time chain
    /// below the constants relates only constants — which is how the import
    /// modal's mapping rows came to claim `100 + i * 10`, a growing block based
    /// in the fixed range, with the build green. But **the hazard is the block,
    /// not the number**, and `register` is handed one index at a time: a
    /// legitimate fixed control at 200 (Settings' row-limit dropdown) and the
    /// first row of a misplaced block at 200 are indistinguishable here. A band
    /// assert therefore either passes the bug or, as this one did, panics the app
    /// on correct code — it shipped asserting fixed controls end at 110, a number
    /// taken from a stale comment while the app really registers up to
    /// [`FIXED_TAB_END`], and clicking Settings crashed.
    ///
    /// What covers the real rule instead: the growing blocks all read
    /// `VALUE_TAB + i * ROW_TAB_STRIDE` (grep is honest here — there are four),
    /// the compile-time chain keeps that band clear of the footer, and
    /// `tests::every_band_the_app_uses_registers_cleanly` walks the indices the
    /// app actually uses.
    pub(crate) fn register(&self, tabindex: u32, id: floem::ViewId) {
        debug_assert!(
            tabindex <= TITLE_CLOSE_TAB,
            "tabindex {tabindex} is past the title bar's ✕, which must stay last"
        );
        let mut e = self.entries.borrow_mut();
        e.retain(|(_, x)| *x != id);
        let at = e.partition_point(|(t, _)| *t <= tabindex);
        e.insert(at, (tabindex, id));
    }

    pub(crate) fn unregister(&self, id: floem::ViewId) {
        self.entries.borrow_mut().retain(|(_, x)| *x != id);
    }

    /// Remember where the walk should resume — what a control calls on its way
    /// out when it hands focus back to the modal root, so the root's own Tab
    /// continues from it instead of restarting at the top.
    ///
    /// Stored as the **tabindex**, not the `ViewId`. A raw id only resolves
    /// while the control it names is still mounted, and the two cases that call
    /// this most are exactly the ones where it isn't: a control removed by its
    /// own action (Tab to an enum value's ✕, press Space) and one rebuilt by it
    /// (a dropdown's accept). `focus_at` already knew this; `last` did not, so
    /// `target` found neither `from` nor `last`, `ring_step` returned 0, and the
    /// next Tab restarted at the modal's first control.
    pub(crate) fn remember(&self, id: floem::ViewId) {
        let t = self
            .entries
            .borrow()
            .iter()
            .find(|(_, x)| *x == id)
            .map(|(t, _)| *t);
        if t.is_some() {
            self.last.set(t);
        }
    }

    /// Where one step from `from` lands: the [remembered](Self::remember)
    /// position stands in when `from` isn't a ring member — which is every
    /// re-entry from a modal root, a popup list, or nowhere. Neither known:
    /// start at the near end.
    fn target_pos(&self, from: floem::ViewId, backwards: bool) -> Option<usize> {
        let e = self.entries.borrow();
        let cur = e
            .iter()
            .position(|(_, x)| *x == from)
            .or_else(|| self.resume_pos(&e));
        ring_step(e.len(), cur, backwards)
    }

    /// The position a remembered tabindex resumes from.
    ///
    /// Still registered: its own position, so a forward step moves past it.
    /// **Gone** — the control removed itself — the position *before* where it
    /// was, so a forward step lands on its neighbour rather than skipping it.
    /// `None` at the front of the ring is the same answer by another route:
    /// `ring_step` starts a forward walk at 0.
    fn resume_pos(&self, e: &[(u32, floem::ViewId)]) -> Option<usize> {
        let t = self.last.get()?;
        let p = e.partition_point(|(x, _)| *x < t);
        if e.get(p).is_some_and(|(x, _)| *x == t) {
            Some(p)
        } else {
            p.checked_sub(1)
        }
    }

    /// Where one step from `from` lands. See [`FocusRing::remember`].
    ///
    /// `step_from` is what the app calls; this is the same decision without the
    /// focus request, so the ring's walk can be asserted without a window.
    #[cfg(test)]
    fn target(&self, from: floem::ViewId, backwards: bool) -> Option<floem::ViewId> {
        let pos = self.target_pos(from, backwards)?;
        self.entries.borrow().get(pos).map(|(_, id)| *id)
    }

    /// Move focus one step from `from`, per `FocusRing::target` (test-only, so
    /// not linkable — it is this without the focus request).
    pub(crate) fn step_from(&self, from: floem::ViewId, backwards: bool) {
        let Some(pos) = self.target_pos(from, backwards) else {
            return;
        };
        let Some((tabindex, id)) = self.entries.borrow().get(pos).copied() else {
            return;
        };
        self.last.set(Some(tabindex));
        // Every keyboard-driven focus change in the app arrives here, which is
        // what makes this the whole "set" half of [`keyboard_nav`] — see there
        // for why it isn't a key listener on the window root.
        keyboard_nav().set(true);
        id.request_focus();
    }

    /// Whatever now sits at `tabindex`.
    pub(crate) fn at(&self, tabindex: u32) -> Option<floem::ViewId> {
        self.entries
            .borrow()
            .iter()
            .find(|(t, _)| *t == tabindex)
            .map(|(_, id)| *id)
    }

    /// Focus whatever now sits at `tabindex`.
    ///
    /// Deliberately by tabindex rather than by the `ViewId` a caller captured at
    /// build time: a control that refocuses itself *after* the update pass its
    /// own action started — a dropdown returning the keyboard once its popup is
    /// gone — may have been rebuilt by that very action, and floem's focus
    /// request has no existence check, so the captured id parked the keyboard on
    /// a removed view and left the modal dead. The tabindex is what survives the
    /// rebuild; the replacement re-registers under it.
    pub(crate) fn focus_at(&self, tabindex: u32) {
        if let Some(id) = self.at(tabindex) {
            self.last.set(Some(tabindex));
            id.request_focus();
        }
    }

    #[cfg(test)]
    fn ids(&self) -> Vec<floem::ViewId> {
        self.entries.borrow().iter().map(|(_, id)| *id).collect()
    }
}

/// A [`focus_root`] that also answers Tab by **entering** `ring`.
///
/// Without this the ring can only be joined by clicking a control first: a modal
/// opens with focus on its root, floem delivers a key to the focused view (and,
/// failing that, only to the window root's own listeners), and the root has no
/// Tab handler of its own — so Tab did nothing at all until something inside was
/// clicked. The same dead end follows every Escape, which hands focus back here
/// on purpose.
///
/// The root is not itself a ring member, so [`FocusRing::step_from`] resumes
/// from wherever the ring last was — and, on a freshly-opened modal that has
/// been nowhere yet, starts at the first control (the last, for Shift+Tab).
///
/// The ring is also published for the *window* root, which is where a key lands
/// when focus is on a dropdown's popup list or on nothing — see
/// [`innermost_ring_root`].
pub(crate) fn focus_root_with_ring<V: IntoView + 'static>(view: V, ring: FocusRing) -> V::V {
    let view = focus_root_inner(view, Some(ring.clone()));
    let id = view.id();
    view.on_event(EventListener::KeyDown, move |e| {
        let Event::KeyDown(ke) = e else {
            return EventPropagation::Continue;
        };
        if ke.key.logical_key == Key::Named(NamedKey::Tab) {
            ring.step_from(id, ke.modifiers.shift());
            return EventPropagation::Stop;
        }
        EventPropagation::Continue
    })
}

/// Put `view` in `ring` at `tabindex`: focusable, Tab / Shift+Tab move on, and
/// Escape blurs.
///
/// Escape hands focus back to the innermost [`focus_root`] rather than merely
/// dropping it, so the *next* Escape reaches the modal and closes it — the same
/// two-step, one-layer-per-press contract `edit_field` follows, and the reason a
/// control reached by Tab doesn't trap the keyboard.
///
/// Only those keys are consumed — everything else falls through, so a control
/// keeps whatever keyboard behaviour it has of its own.
///
/// Not for a text field: floem's editor never lets a key reach a listener like
/// this one, so [`crate::FieldCfg::focus`] carries the ring into the editor's
/// own key handler instead.
pub(crate) fn in_focus_ring<V: IntoView + 'static>(
    view: V,
    ring: FocusRing,
    tabindex: u32,
) -> impl IntoView {
    in_focus_ring_with(view, ring, tabindex, || {})
}

/// [`in_focus_ring`] for a control that needs teardown of its own.
///
/// Floem keeps a **single** cleanup slot per view, so chaining a second
/// `.on_cleanup` onto the result would silently replace the ring's unregister
/// *and* the focus hand-back below. Pass the extra work here instead.
pub(crate) fn in_focus_ring_with<V: IntoView + 'static>(
    view: V,
    ring: FocusRing,
    tabindex: u32,
    on_dispose: impl Fn() + 'static,
) -> impl IntoView {
    let view = view.into_view();
    let id = view.id();
    ring.register(tabindex, id);
    let step_ring = ring.clone();
    let cleanup_ring = ring;
    // "Was I focused?" mirrored into a plain `Cell`, not read back from floem at
    // cleanup time: by then the view is being removed and `app_state.focus` has
    // already been cleared.
    let focused = Rc::new(std::cell::Cell::new(false));
    let (gained, lost, at_cleanup) = (focused.clone(), focused.clone(), focused);
    view.keyboard_navigable()
        .on_event_cont(EventListener::FocusGained, move |_| gained.set(true))
        .on_event_cont(EventListener::FocusLost, move |_| lost.set(false))
        .on_event(EventListener::KeyDown, move |e| {
            let Event::KeyDown(ke) = e else {
                return EventPropagation::Continue;
            };
            if ke.key.logical_key == Key::Named(NamedKey::Tab) {
                step_ring.step_from(id, ke.modifiers.shift());
                return EventPropagation::Stop;
            }
            if ke.key.logical_key == Key::Named(NamedKey::Escape) {
                // Remember where the walk was before handing the keyboard back,
                // so the root's Tab resumes here rather than at the top.
                id.clear_focus();
                hand_keyboard_back(Some((&step_ring, id)));
                return EventPropagation::Stop;
            }
            EventPropagation::Continue
        })
        .on_cleanup(move || {
            // `remember` *before* unregistering, so the tabindex is still known:
            // the next Tab then resumes beside where the control was rather than
            // at the top of the modal. Tab to an enum value's ✕, press Space, and
            // the row — and the focused view with it — is gone.
            if at_cleanup.get() {
                hand_keyboard_back(Some((&cleanup_ring, id)));
            }
            cleanup_ring.unregister(id);
            on_dispose();
        })
}

// ===== moved from lib.rs (widgets cluster) =====
/// A title bar for a modal panel, with a close (×) button.
///
/// `ring` is **required**, and that is the point of the parameter: this ✕ was
/// the one button in every modal that never joined one — and in the four
/// footer-less modals (Settings, Terminal settings, AI settings, Shortcuts) it
/// is the *only* button, so "every modal button goes through `in_ring_button`"
/// was false in the chrome all fifteen of them wear.
pub(crate) fn modal_title(
    title: &'static str,
    close: Rc<dyn Fn()>,
    ring: FocusRing,
) -> impl IntoView {
    modal_title_impl(title, close, ring, true)
}

/// Like [`modal_title`] but without the bottom separator border — for modals
/// whose body already reads as a distinct block (the plan modal's boxed table).
pub(crate) fn modal_title_borderless(
    title: &'static str,
    close: Rc<dyn Fn()>,
    ring: FocusRing,
) -> impl IntoView {
    modal_title_impl(title, close, ring, false)
}

/// [`modal_title`] for a title that isn't known at compile time (the import
/// modal names the table it's loading into).
pub(crate) fn modal_title_owned(
    title: String,
    close: Rc<dyn Fn()>,
    ring: FocusRing,
) -> impl IntoView {
    modal_title_impl(title, close, ring, true)
}

fn modal_title_impl(
    title: impl Into<String>,
    close: Rc<dyn Fn()>,
    ring: FocusRing,
    border: bool,
) -> impl IntoView {
    let title = title.into();
    let pressed = close.clone();
    h_stack((
        text(title).style(|s| s.font_size(15.0).font_bold().color(theme::text())),
        empty().style(|s| s.flex_grow(1.0_f32)),
        // Lucide X, 16px, vertically centred; `padding(6)` enlarges the click
        // hitbox (same idiom as `toolbar_icon`) so it's not fiddly to hit. Same
        // dim→bright colour as the old glyph.
        in_ring_button(
            container(icons::icon(icons::X, 16.0))
                .on_click_stop(move |_| (close)())
                .style(|s| {
                    s.flex_shrink(0.0_f32)
                        .items_center()
                        .padding(6.0)
                        .color(theme::text_dim())
                        .hover(|s| s.color(theme::text()))
                }),
            ring,
            TITLE_CLOSE_TAB,
            true,
            0.0, // a square face — the padding is the hitbox, not a chip
            move || (pressed)(),
        ),
    ))
    .style(move |s| {
        s.width_full()
            .flex_row()
            .items_center()
            .padding_horiz(MODAL_PAD_H)
            .padding_vert(10.0)
            .border_bottom(if border { 1.0 } else { 0.0 })
            .border_color(theme::border())
    })
}

// ===== modal form chrome =====
// The shape every modal form in the app wears — Manage Connections set it, the
// import modal followed it, and the schema designer follows both. Collected here
// rather than copied a third time, because "consistent" that lives in three
// files stops being consistent the first time one of them is tweaked.

/// Gap between form rows.
pub(crate) const FORM_GAP: f64 = 18.0;

/// The inset every part of a modal shares: the title, the designer's tab strip,
/// the body, and the footer. One constant because the alignment is the point —
/// the title sat at 14 and the bodies at 20, so a form's first label started six
/// pixels right of the heading above it and of the buttons below it. A modal that
/// insets its content by hand is the drift this exists to stop.
pub(crate) const MODAL_PAD_H: f64 = 14.0;

/// The caption above a control, and the explanatory line under one.
///
/// Two style fns rather than two widgets, because a caption isn't always a bare
/// `text(…)` — some are computed, some live inside a `dyn_container`. Every form
/// surface in the app goes through these: a text field, a dropdown, a switch, the
/// colour picker. Settings was built in this style and the modals that came after
/// each re-spelled it slightly differently (`text_dim` here, `text_faint` there,
/// three font sizes), which is the drift these exist to end.
///
/// **`text_dim`, not `text_muted`.** A caption is body text a user is expected to
/// read, and this one fn paints every one in the app; `text_muted` is 2.55:1 on
/// `bg_panel` in the dark theme and 2.8:1 in the light one, against a `Body`
/// floor of 4.5. `text_dim` is 4.45:1, which is what these captions were before
/// they were collected here — the collecting was right, the colour it landed on
/// was not.
pub(crate) fn form_label_style(s: floem::style::Style) -> floem::style::Style {
    s.color(theme::text_dim()).font_size(theme::FONT_LABEL)
}

/// The hint under a control: recessive, a size down. See [`form_label_style`].
///
/// `text_faint`, not `text_muted` at 60% — the latter composites to 1.70:1,
/// under even the `Recessive` floor of 2.0, which no other foreground in
/// `UI_PAIRINGS` misses.
pub(crate) fn form_hint_style(s: floem::style::Style) -> floem::style::Style {
    s.color(theme::text_faint()).font_size(theme::FONT_HINT)
}

/// A form hint as a view — the common case, where the text is a literal.
pub(crate) fn form_hint(hint: impl Into<String>) -> impl IntoView {
    text(hint.into()).style(form_hint_style)
}

/// A labelled control: caption above, control below.
pub(crate) fn form_setting(label: &'static str, control: impl IntoView + 'static) -> impl IntoView {
    form_setting_owned(label.to_string(), control)
}

/// [`form_setting`] for a caption that isn't known at compile time.
pub(crate) fn form_setting_owned(label: String, control: impl IntoView + 'static) -> impl IntoView {
    v_stack((text(label).style(form_label_style), control))
        .style(|s| s.flex_col().gap(6.0).width_full())
}

/// A small bold section heading.
pub(crate) fn form_section(label: &'static str) -> impl IntoView {
    form_section_owned(label.to_string())
}

/// [`form_section`] for a heading that isn't known at compile time — the DDL
/// preview's "1 Change" / "3 Changes", where the count *is* the heading.
pub(crate) fn form_section_owned(label: String) -> impl IntoView {
    text(label).style(|s| {
        s.font_size(theme::FONT_BODY)
            .font_bold()
            .color(theme::text())
    })
}

/// A rule between sections: the same weight and colour as the one under a modal
/// header, inset with the rest of the body content.
///
/// The margin is 20 *minus* the enclosing stack's gap, which also applies above
/// and below — so the gap that lands on screen is exactly 20, not 28.
pub(crate) fn form_separator(stack_gap: f64) -> impl IntoView {
    empty().style(move |s| {
        s.width_full()
            .height(1.0)
            .flex_shrink(0.0_f32)
            .background(theme::border())
            .margin_vert(20.0 - stack_gap)
    })
}

/// The glyph size a list row's icon buttons paint at.
pub(crate) const ROW_ICON: f64 = 14.0;
/// The padding around it — the other half of [`row_slot`]'s footprint.
const ROW_ICON_PAD: f64 = 4.0;

/// One icon-button-shaped slot in a list row.
pub(crate) fn row_slot(inner: impl IntoView + 'static) -> impl IntoView {
    container(inner).style(|s| s.padding(ROW_ICON_PAD).flex_shrink(0.0_f32))
}

/// The small icon button an editable list's rows use — the enum values, a
/// domain's checks, a trigger's arguments, a function's settings.
///
/// `tabindex` sits in its row's [`ROW_TAB_STRIDE`] block, above the row's own
/// fields — so Tab walks a row's value, then what you can do to it, then the
/// next row.
pub(crate) fn row_button(
    glyph: &'static str,
    tip: &'static str,
    ring: FocusRing,
    tabindex: u32,
    act: impl Fn() + 'static,
) -> AnyView {
    let act = Rc::new(act);
    let pressed = act.clone();
    let button = row_slot(crate::icons::icon(glyph, ROW_ICON as f32))
        .on_click_stop(move |_| act())
        // Colour-only hover, like every other icon button in the app. The focus
        // outline is `in_ring_button`'s, on the wrapper it registers — putting
        // one here would paint on an id that never takes focus, which is what
        // `.tooltip()` below used to guarantee.
        .style(|s| s.color(theme::text_dim()).hover(|s| s.color(theme::text())))
        .tooltip(move || text(tip).style(tooltip_style));
    in_ring_button(button, ring, tabindex, true, 0.0, move || pressed())
}

/// [`row_button`]'s footprint with nothing in it — what a move button becomes on
/// the row it can't move: the first row's ↑, the last row's ↓.
///
/// An empty slot rather than `hide()`, which is `display: none` and takes the
/// space with it: the ↓ and the bin would slide left into where the ↑ and ↓ sit
/// on every other row, so the three icons would stand in a different place on the
/// first row, the last row, and the ones between. A one-value list, where *both*
/// arrows are dead, is the case that made it obvious.
pub(crate) fn row_gap() -> AnyView {
    row_slot(empty().style(|s| s.size(ROW_ICON, ROW_ICON))).into_any()
}

/// A bordered control button (Choose file…, + Column), wearing the same chrome as
/// the header's Retry and the ER-diagram toolbar rather than Floem's default
/// button.
pub(crate) fn control_button(
    label: impl Into<String>,
    ring: FocusRing,
    tabindex: u32,
    on_click: impl Fn() + 'static,
) -> AnyView {
    control_button_enabled(label, true, ring, tabindex, on_click)
}

/// [`control_button`] that can be inert — for one whose subject may be missing
/// (Edit, with nothing selected). Dimmed and unclickable rather than absent, on
/// the same grounds a disabled [`action_button`] keeps its place: a control that
/// comes and goes moves the row it sits in.
pub(crate) fn control_button_enabled(
    label: impl Into<String>,
    enabled: bool,
    ring: FocusRing,
    tabindex: u32,
    on_click: impl Fn() + 'static,
) -> AnyView {
    let on_click = Rc::new(on_click);
    let pressed = on_click.clone();
    let button = text(label.into())
        .on_click_stop(move |_| {
            if enabled {
                on_click()
            }
        })
        .style(move |s| {
            let s = control_surface(s)
                .font_size(theme::FONT_BODY)
                .padding_horiz(10.0)
                .padding_vert(5.0)
                .flex_shrink(0.0_f32);
            if enabled {
                s.color(theme::text())
                    .hover(|s| s.background(theme::control_hover()))
            } else {
                s.color(theme::text_faint())
            }
        });
    in_ring_button(button, ring, tabindex, enabled, CONTROL_RADIUS, move || {
        pressed()
    })
}

/// How much weight a modal action carries.
///
/// The variant is a **fill**, not a text colour: both labels are the same grey,
/// so what separates "the thing this footer exists to do" from "never mind" is
/// the background behind it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ActionKind {
    /// Dismissive — Cancel, Back.
    Neutral,
    /// The affirmative one — Preview SQL, Apply, Save.
    Primary,
    /// A side action that isn't part of the decision the footer is asking about
    /// — Copy, Open in editor. Recessed *below* the panel rather than raised
    /// above it, which is what keeps it out of the Neutral/Primary pair.
    Quiet,
    /// [`ActionKind::Primary`] for a plan that destroys something: same place,
    /// same weight, red. The preview's affirmative action has worn the colour of
    /// what it does since before these buttons were filled, and it is the last
    /// signal before an irreversible `ALTER`.
    Danger,
}

/// The gap between a footer's actions.
pub(crate) const ACTION_GAP: f64 = 10.0;

/// Gap between an action's icon and its label.
const ACTION_ICON_GAP: f64 = 7.0;

/// An action's horizontal padding. Named because [`action_face`] has to add it
/// back when it computes a width from a label.
const ACTION_PAD_H: f64 = 10.0;
/// Its vertical padding — the other half of [`action_height`].
const ACTION_PAD_V: f64 = 8.0;

/// The height every filled action holds, whatever its face: a label, a label
/// with a glyph beside it, or a glyph standing in for the label.
///
/// Explicit, because those faces don't agree on a height — a 16px icon is taller
/// than a 13px line box, so a button that flashed a confirmation *grew* while it
/// showed it, and the footer's whole row of buttons shifted with it. Measured off
/// the text rather than picked, so it stays right if `FONT_BODY` moves, and
/// cached because the answer can't change within a run (the family is global and
/// the size is a `const`). Thread-local: `TextLayout` goes through the global
/// `FontSystem`, which is the UI thread's.
fn action_height() -> f64 {
    thread_local! {
        static H: std::cell::OnceCell<f64> = const { std::cell::OnceCell::new() };
    }
    // A string with both an ascender and a descender, so the line box is the full
    // one a label gets rather than the one an x-height-only string reports.
    H.with(|h| *h.get_or_init(|| measure_text_h_at("Xg", theme::FONT_BODY) + 2.0 * ACTION_PAD_V))
}

/// A filled modal action. **Every** modal footer in the app is built from these
/// now — the schema editors, the DDL preview, Import and Manage Connections — so
/// keep it free of anything specific to one of them. The only actions that
/// aren't filled are the question dialogs' ([`dialog_button`]), which have no
/// footer bar to sit in.
///
/// Disabled keeps the fill and fades the label (its `ActionKind` label colour at half alpha),
/// rather than hiding or unfilling the button: which action is the affirmative
/// one shouldn't move around as a form becomes valid.
pub(crate) fn action_button(
    label: impl Into<String>,
    kind: ActionKind,
    enabled: bool,
    ring: FocusRing,
    tabindex: u32,
    on_click: impl Fn() + 'static,
) -> AnyView {
    action_button_inner(label, None, kind, enabled, ring, tabindex, on_click)
}

/// [`action_button`] with a leading icon — the preview footer's Copy and Open in
/// editor. The glyph inherits the button's colour, so it follows the disabled
/// state without a second rule.
pub(crate) fn action_button_icon(
    label: impl Into<String>,
    icon: &'static str,
    kind: ActionKind,
    enabled: bool,
    ring: FocusRing,
    tabindex: u32,
    on_click: impl Fn() + 'static,
) -> AnyView {
    action_button_inner(label, Some(icon), kind, enabled, ring, tabindex, on_click)
}

type ColorFn = fn() -> floem::peniko::Color;

/// The (fill, hovered fill, label) triple an [`ActionKind`] paints with.
fn action_colors(kind: ActionKind) -> (ColorFn, ColorFn, ColorFn) {
    match kind {
        ActionKind::Neutral => (
            theme::btn_neutral,
            theme::btn_neutral_hover,
            theme::btn_neutral_text,
        ),
        ActionKind::Primary => (
            theme::btn_primary,
            theme::btn_primary_hover,
            theme::btn_primary_text,
        ),
        ActionKind::Quiet => (
            theme::btn_quiet,
            theme::btn_quiet_hover,
            theme::btn_quiet_text,
        ),
        ActionKind::Danger => (
            theme::btn_danger,
            theme::btn_danger_hover,
            theme::btn_danger_text,
        ),
    }
}

/// The chrome every filled action wears, given its kind and whether it's live.
fn action_style(s: floem::style::Style, kind: ActionKind, enabled: bool) -> floem::style::Style {
    let (fill, fill_hover, label_color) = action_colors(kind);
    let s = s
        .flex_row()
        .items_center()
        .justify_center()
        .font_size(theme::FONT_BODY)
        .padding_horiz(ACTION_PAD_H)
        .padding_vert(ACTION_PAD_V)
        .height(action_height())
        .border_radius(ACTION_RADIUS)
        .flex_shrink(0.0_f32);
    if enabled {
        s.background(fill())
            .color(label_color())
            .hover(move |s| s.background(fill_hover()))
    } else {
        // The whole button fades, fill included — half-strength on both, so it
        // recedes as one object instead of a dim label sitting in a chip as solid
        // as the live one beside it. It still holds its place: which action is
        // affirmative shouldn't move as a form becomes valid.
        s.background(fill().multiply_alpha(0.5))
            .color(label_color().multiply_alpha(0.5))
    }
}

#[allow(clippy::too_many_arguments)] // a UI builder; grouping into a struct adds no clarity
fn action_button_inner(
    label: impl Into<String>,
    icon: Option<&'static str>,
    kind: ActionKind,
    enabled: bool,
    ring: FocusRing,
    tabindex: u32,
    on_click: impl Fn() + 'static,
) -> AnyView {
    let glyph: AnyView = match icon {
        Some(markup) => icons::icon(markup, 15.0)
            .style(|s| s.flex_shrink(0.0_f32).margin_right(ACTION_ICON_GAP))
            .into_any(),
        None => empty().into_any(),
    };
    let on_click = Rc::new(on_click);
    let pressed = on_click.clone();
    let button = h_stack((glyph, text(label.into())))
        .on_click_stop(move |_| {
            if enabled {
                on_click()
            }
        })
        .style(move |s| action_style(s, kind, enabled));
    in_ring_button(button, ring, tabindex, enabled, ACTION_RADIUS, move || {
        pressed()
    })
}

/// An [`action_button`] whose face the caller supplies and can swap, held at the
/// width its widest label needs.
///
/// For the two buttons that acknowledge themselves — Save flashing a check, Test
/// flashing its result — where the confirmation replaces the label rather than
/// sitting beside it. **Pinning the width is the whole point**: a button that
/// resizes when you press it shoves its neighbours sideways and stops reading as
/// the same button, which is why `width_for` is measured rather than left to the
/// face inside. The face is centred, so an icon lands in the middle of the button
/// the label just left; a face that needs to stay put across a change of its own
/// (an animated `Test…`) states its own width and is centred as a block.
pub(crate) fn action_face<V: IntoView + 'static, F: Fn() + 'static>(
    width_for: &str,
    kind: ActionKind,
    enabled: bool,
    ring: FocusRing,
    tabindex: u32,
    face: V,
    on_click: F,
) -> AnyView {
    // +2px against sub-pixel rounding, the same guard `loading_dots` uses.
    let w = measure_text_px_at(width_for, theme::FONT_BODY) + 2.0 * ACTION_PAD_H + 2.0;
    let on_click = Rc::new(on_click);
    let pressed = on_click.clone();
    let button = container(face)
        .on_click_stop(move |_| {
            if enabled {
                on_click()
            }
        })
        .style(move |s| {
            action_style(s, kind, enabled)
                .width(w)
                .justify_center()
                .padding_horiz(0.0)
        });
    in_ring_button(button, ring, tabindex, enabled, ACTION_RADIUS, move || {
        pressed()
    })
}

/// A text-only action: no fill, a colour that carries the meaning, brightening
/// on hover. What the **question dialogs** use — the transaction prompt and the
/// confirm modal, which each carried a private copy of it before (same
/// colour-fn signature, same hover, same radius, differing only in padding,
/// which is the reason they weren't already sharing one).
///
/// This is now the *only* text-button family left. Every modal with a footer bar
/// wears the filled [`action_button`] instead; a `footer_button` at a smaller
/// padding sat beside this one until the last of those footers moved over.
///
/// `ring` is **required**, like [`modal_title`]'s. These were the last ring-less
/// buttons in the app, and one of the two dialogs they build is the transaction
/// prompt — raised when something would strand an **open transaction**, with
/// Escape deliberately dead (there is no safe "never mind" for uncommitted
/// writes) and no ring published, so no key did anything at all and the user had
/// to reach for the mouse to answer a question about their own writes.
pub(crate) fn dialog_button(
    label: impl Into<String> + 'static,
    color: fn() -> floem::peniko::Color,
    hover: fn() -> floem::peniko::Color,
    ring: FocusRing,
    tabindex: u32,
    on_click: impl Fn() + 'static,
) -> AnyView {
    let on_click = Rc::new(on_click);
    let pressed = on_click.clone();
    in_ring_button(
        text_button(label, color, hover, true, (10.0, 5.0), move || on_click()),
        ring,
        tabindex,
        true,
        CONTROL_RADIUS, // `text_button`'s own
        move || pressed(),
    )
}

fn text_button(
    label: impl Into<String>,
    color: fn() -> floem::peniko::Color,
    hover: fn() -> floem::peniko::Color,
    enabled: bool,
    (pad_h, pad_v): (f64, f64),
    on_click: impl Fn() + 'static,
) -> impl IntoView {
    text(label.into())
        .on_click_stop(move |_| {
            if enabled {
                on_click()
            }
        })
        .style(move |s| {
            let s = s
                .font_size(theme::FONT_BODY)
                .padding_horiz(pad_h)
                .padding_vert(pad_v)
                .border_radius(CONTROL_RADIUS);
            if enabled {
                s.color(color()).hover(move |s| s.color(hover()))
            } else {
                // Dimmed and inert rather than hidden: the button staying put is
                // what makes it obvious which step you're on.
                s.color(theme::text_faint())
            }
        })
}

/// The bordered bar a modal's actions sit in — quiet actions on the right, the
/// affirmative one last.
pub(crate) fn modal_footer(actions: impl IntoView + 'static) -> impl IntoView {
    modal_footer_split(empty(), actions)
}

/// [`modal_footer`] with something pinned to the *left* as well — a status the
/// footer's buttons act on (the designer's change count), which belongs at the
/// far edge rather than crowded against them.
pub(crate) fn modal_footer_split(
    status: impl IntoView + 'static,
    actions: impl IntoView + 'static,
) -> impl IntoView {
    h_stack((
        status,
        empty().style(|s| s.flex_grow(1.0_f32).min_width(10.0)),
        actions,
    ))
    .style(|s| {
        s.width_full()
            .flex_row()
            .items_center()
            .padding_horiz(MODAL_PAD_H)
            .padding_vert(10.0)
            .border_top(1.0)
            .border_color(theme::border())
    })
}

pub(crate) fn menu_item_style(s: floem::style::Style) -> floem::style::Style {
    s.width_full()
        .flex_row()
        .items_center()
        .gap(8.0)
        .padding_horiz(12.0)
        .padding_vert(6.0)
        .color(theme::text())
        .hover(|s| s.background(theme::accent().multiply_alpha(0.15)))
}

pub(crate) fn panel_style(s: floem::style::Style) -> floem::style::Style {
    s.flex_col()
        .background(theme::bg_panel())
        .border(1.0)
        .border_color(theme::border())
        .border_radius(10.0)
}

/// The colour an icon that *opens a menu* paints itself: the accent while its
/// menu is up, `text()` under the pointer, `text_muted()` at rest.
///
/// **Open outranks hover**, which is the whole ordering question here — the
/// pointer is still on the icon it just clicked, so a hover that won would make
/// "this menu is open" and "you are about to open this menu" the same colour for
/// as long as the menu is up. The accent is what the rest of the app tints the
/// thing currently in play, and it is the one state a user can't otherwise see
/// on a dropdown that opens *below* the icon they are looking at.
///
/// One function because the app has six of these controls in five files (the
/// schema eye and gear, the activity clock, the results strip's copy, download
/// and AI icons), each of which had spelled its own two-arm hover match.
pub(crate) fn menu_icon_color(open: bool, hovered: bool) -> Color {
    if open {
        theme::accent()
    } else if hovered {
        theme::text()
    } else {
        theme::text_muted()
    }
}

/// The app's tooltip chrome, applied globally to Floem's `TooltipClass` (see the
/// root stylesheet in `lib.rs`) so every `.tooltip(…)` gets it — a compact
/// bordered panel matching the app's popovers, with a soft drop shadow lifting it
/// off the content. `color`/`font_size` are inherited, so a bare `text(…)` tip
/// picks them up.
///
/// A tip is mounted as a Floem *overlay* (a child of the window root), so the
/// root stylesheet's non-selectable-label rule can't reach it — this style is
/// applied directly to the tip view, so it carries the rule itself (as a direct
/// prop for a bare `text(…)` tip, and as a class rule for a wrapped one).
pub(crate) fn tooltip_style(s: floem::style::Style) -> floem::style::Style {
    s.background(theme::bg_panel())
        .color(theme::text())
        .font_size(theme::FONT_LABEL)
        .selectable(false)
        .class(floem::views::LabelClass, |s| s.selectable(false))
        .border(1.0)
        .border_color(theme::border())
        .border_radius(6.0)
        .padding_horiz(9.0)
        .padding_vert(6.0)
        .box_shadow_blur(12.0)
        .box_shadow_spread(0.0)
        .box_shadow_v_offset(3.0)
        .box_shadow_color(theme::tooltip_shadow())
}

/// A toolbar / title-bar icon button with a **padded hitbox** (5px horiz / 3px
/// vert). Hover (dim→bright) is driven from a signal via `PointerEnter`/`Leave`
/// on the padded container, so the *whole* box — not just the 16px glyph —
/// highlights and takes the click. `mt`/`mr` position it (pass `0.0` when the
/// caller lays out via separators/gaps, e.g. the results grid). `enabled` dims
/// the glyph to 30% and swallows the click when false. Shared by the results-grid
/// toolbar and the Schema/AI/Terminal/History panel title bars.
pub(crate) fn toolbar_icon(
    markup: &'static str,
    mt: f64,
    mr: f64,
    enabled: impl Fn() -> bool + Copy + 'static,
    on_click: impl Fn() + 'static,
) -> impl IntoView {
    let hov = RwSignal::new(false);
    container(icons::icon(markup, 16.0).style(move |s| {
        let c = if !enabled() {
            theme::text_muted().multiply_alpha(0.3)
        } else if hov.get() {
            theme::text()
        } else {
            theme::text_muted()
        };
        s.flex_shrink(0.0_f32).color(c)
    }))
    .on_click_stop(move |_| {
        if enabled() {
            on_click();
        }
    })
    .on_event_cont(EventListener::PointerEnter, move |_| hov.set(true))
    .on_event_cont(EventListener::PointerLeave, move |_| hov.set(false))
    .style(move |s| {
        s.items_center()
            .margin_top(mt)
            .margin_right(mr)
            .padding_horiz(5.0)
            .padding_vert(3.0)
            .cursor(floem::style::CursorStyle::Default)
    })
}

/// The stored window-size signal plus the scope that owns it.
type WindowSizeSlot = (RwSignal<(f64, f64)>, Scope);

thread_local! {
    static WINDOW_SIZE: std::cell::RefCell<Option<WindowSizeSlot>> =
        const { std::cell::RefCell::new(None) };
}

/// Live window size (the root stack's size — root is window-sized), for overlays
/// that need to flip/fit near a screen edge. Set once from `workspace`'s root
/// `on_resize`; read by `menu_panel`'s submenu edge-flip.
pub(crate) fn window_size() -> RwSignal<(f64, f64)> {
    WINDOW_SIZE.with(|cell| {
        if cell.borrow().is_none() {
            // Detached scope → lives for the whole process (like the theme state).
            let scope = Scope::new();
            let sig = scope.create_rw_signal((0.0, 0.0));
            *cell.borrow_mut() = Some((sig, scope));
        }
        cell.borrow().as_ref().unwrap().0
    })
}

/// The stored pointer-release nonce plus the scope that owns it.
type PointerReleaseSlot = (RwSignal<u64>, Scope);

thread_local! {
    static POINTER_RELEASED: std::cell::RefCell<Option<PointerReleaseSlot>> =
        const { std::cell::RefCell::new(None) };
}

/// Bumped whenever the pointer is released **anywhere in the window**. Set once
/// from `workspace`'s root `PointerUp`; read by anything holding a
/// button-is-down flag.
///
/// This exists because "the button came up" is the one pointer fact a view
/// cannot observe locally. A drag that begins in one view routinely ends
/// somewhere else entirely — floem delivers the release to whatever is under
/// the cursor — and there is no capture to fall back on: the grid's drag-select
/// is *driven* by other cells' `PointerEnter`, so `request_active` would stop
/// the very events that make it work.
///
/// The grid's `selecting` flag latched on for exactly that reason: released over
/// the status bar, the schema panel, or its own results toolbar, it stayed
/// armed, and moving the cursor back over the rows kept extending the selection
/// with no button held. Its own `PointerUp` covers releases inside the grid;
/// this covers the rest of the window, which is most of it.
///
/// A nonce rather than a bool: a flag would have to be un-set by someone, and
/// two consecutive releases must be two events. Readers track it and act on the
/// change.
pub(crate) fn pointer_released() -> RwSignal<u64> {
    POINTER_RELEASED.with(|cell| {
        if cell.borrow().is_none() {
            // Detached scope → lives for the whole process, like `window_size`.
            let scope = Scope::new();
            let sig = scope.create_rw_signal(0u64);
            *cell.borrow_mut() = Some((sig, scope));
        }
        cell.borrow().as_ref().unwrap().0
    })
}

/// The stored keyboard-navigation flag plus the scope that owns it.
type KeyboardNavSlot = (RwSignal<bool>, Scope);

thread_local! {
    static KEYBOARD_NAV: std::cell::RefCell<Option<KeyboardNavSlot>> =
        const { std::cell::RefCell::new(None) };
}

/// **Was the keyboard the last thing to move focus?** The app's own
/// `:focus-visible`, and the reason a focus ring can be bright without being
/// noise.
///
/// A focus outline is information the *keyboard* needs; on a mouse click it
/// marks what you just pointed at, which you already know, so a ring visible
/// enough to be useful under Tab is a distraction under the pointer. The web
/// solves this with `:focus-visible`, and floem has the same idea in
/// `FocusVisible` — but it is unreachable here, for the reason
/// [`button_focus_ring`] records: floem gates it on `app_state.keyboard_navigation`,
/// which only its own `view_tab_navigation` sets, and everything [`FocusRing`]
/// does goes through `UpdateMessage::Focus`. So this is that flag, kept by the
/// app.
///
/// **Set by [`FocusRing::step_from`], cleared by the root's `PointerDown`**, and
/// those two are chosen rather than a list of "navigation keys":
///
/// - Every keyboard-driven focus change in the app is a Tab or Shift+Tab through
///   the ring, so `step_from` *is* the definition — no key allowlist to keep in
///   step, and typing in a text field can't set it.
/// - Watching keys at the window root would have missed the only case that
///   matters. Floem delivers a key to the focused view and then to the root's
///   listeners **only if nothing consumed it**, and Tab is precisely the key the
///   ring consumes.
/// - The pointer half is nearly as awkward and in the opposite direction: the
///   root's catch-all `PointerDown` (`lib.rs`) sees every press **nothing else
///   consumed**, which is why this function exists at all — a trigger that stops
///   the press has to repay the clear itself. Five sites keep a bare `|_| {}` on
///   purpose (the menu panel, the tree's two menu bodies, the column popover, the
///   run menu), so the flag survives a click inside an open menu.
///
/// Deliberately *not* touched by [`FocusRing::focus_at`] or
/// [`hand_keyboard_back`]. Both move focus on behalf of something the user may
/// have reached either way — a dropdown handing the keyboard back once its popup
/// closes, a field unmounting under them — so leaving the flag alone keeps
/// whatever their last real gesture said, which is the answer in both directions.
pub(crate) fn keyboard_nav() -> RwSignal<bool> {
    KEYBOARD_NAV.with(|cell| {
        if cell.borrow().is_none() {
            // Detached scope → lives for the whole process, like `window_size`.
            let scope = Scope::new();
            let sig = scope.create_rw_signal(false);
            *cell.borrow_mut() = Some((sig, scope));
        }
        cell.borrow().as_ref().unwrap().0
    })
}

/// The `PointerDown` handler a **menu trigger** installs — what the bare `|_| {}`
/// it replaced was quietly getting wrong.
///
/// A trigger has to *stop* the press, and that part is not optional: the
/// workspace root closes `popup_menu` on any pointer-down, so an unswallowed
/// press closes the menu the click is about to open — or, on a trigger that
/// toggles, closes the menu the click then re-opens, which is the bug
/// [`menu_anchored_at`] exists next to.
///
/// But the root's handler does a **second** thing, and swallowing the event took
/// that with it: it clears [`keyboard_nav`], the app's `:focus-visible`. A press
/// on a trigger is still a pointer gesture, so the flag must fall here exactly as
/// it would have at the root, and this is not only about a stray outline:
///
/// - The focus ring stays painted after a *mouse* press, which is the one gesture
///   it is gated off in the first place.
/// - [`set_menu_return`] is armed only when `keyboard_nav` is set, and openers ask
///   it in the `Click` that follows this press. Left set, a menu opened **by
///   mouse** arms a return, and closing it drags the keyboard back to the trigger
///   — taking the arrow keys off whatever had them, which for the grid's toolbar
///   is the cell navigation. The whole reason that slot is conditional.
///
/// Guarded like the root's own write, which never dedups: an unguarded `set` on
/// every press would re-run the style closure of every view reading the flag.
///
/// **For a trigger, not a panel.** The other views that swallow a pointer-down —
/// [`menu_panel`], the schema tree's two menu bodies, the grid's column popover,
/// the editor's — do it so a click *inside* them isn't read as a click away, and
/// they keep the bare `|_| {}`: a click on a menu row is a gesture within
/// something the keyboard may legitimately still own, and where focus goes when
/// it closes is already [`set_menu_return`]'s answer, decided when the menu
/// opened rather than by the press that dismisses it.
pub(crate) fn menu_trigger_press(_: &floem::event::Event) {
    let kbd = keyboard_nav();
    if kbd.get_untracked() {
        kbd.set(false);
    }
}

// ── Reusable themed popup menu (with nested submenus) ───────────────────────
//
// `menu_panel(entries, close, width)` renders a themed popup (matching the schema
// / editor context menus) from a `Vec<MenuEntry>`; a `Sub` entry hover-expands a
// nested panel to its right. The caller positions the returned panel absolutely
// (at the cursor, etc.). Dismissal: the panel absorbs its own pointer-downs so a
// root-level "pointer-down anywhere closes the menu" handler only fires for
// clicks *outside*; Escape and any action also call `close`.

/// Icon markup + a colour accessor (a `fn` so the tint follows theme switches).
pub type MenuIcon = (&'static str, fn() -> floem::peniko::Color);

/// One entry in a [`menu_panel`]. Submenus nest arbitrarily (each level tracks
/// its own open child), though two levels is the common case.
#[derive(Clone)]
pub enum MenuEntry {
    Action {
        label: String,
        icon: Option<MenuIcon>,
        /// Optional label tint (a `fn` so it follows theme switches); `None` uses
        /// the default text colour. Used to mark a selected option.
        label_color: Option<fn() -> floem::peniko::Color>,
        /// Dimmed + inert (no click, no hover) — for an action that isn't currently
        /// applicable (e.g. "AI Fill Value" with no cell selected).
        disabled: bool,
        action: Rc<dyn Fn()>,
    },
    Sub {
        label: String,
        icon: Option<MenuIcon>,
        children: Vec<MenuEntry>,
    },
    Separator,
}

impl MenuEntry {
    pub(crate) fn action(label: impl Into<String>, action: impl Fn() + 'static) -> Self {
        MenuEntry::Action {
            label: label.into(),
            icon: None,
            label_color: None,
            disabled: false,
            action: Rc::new(action),
        }
    }
    pub(crate) fn action_icon(
        label: impl Into<String>,
        icon: MenuIcon,
        action: impl Fn() + 'static,
    ) -> Self {
        MenuEntry::Action {
            label: label.into(),
            icon: Some(icon),
            label_color: None,
            disabled: false,
            action: Rc::new(action),
        }
    }
    /// An action whose label is tinted (e.g. to mark the currently-selected option).
    pub(crate) fn action_colored(
        label: impl Into<String>,
        color: fn() -> floem::peniko::Color,
        action: impl Fn() + 'static,
    ) -> Self {
        MenuEntry::Action {
            label: label.into(),
            icon: None,
            label_color: Some(color),
            disabled: false,
            action: Rc::new(action),
        }
    }
    /// Mark this entry disabled (dimmed + inert). No-op on `Sub`/`Separator`.
    pub(crate) fn disabled(mut self, yes: bool) -> Self {
        if let MenuEntry::Action { disabled, .. } = &mut self {
            *disabled = yes;
        }
        self
    }
    pub(crate) fn sub(label: impl Into<String>, children: Vec<MenuEntry>) -> Self {
        MenuEntry::Sub {
            label: label.into(),
            icon: None,
            children,
        }
    }
}

/// What pressing Enter on a row does — the keyboard half of [`MenuEntry`], taken
/// off the entries before they are consumed into views.
#[derive(Clone)]
pub(crate) enum MenuAct {
    /// Run it and close the whole menu, exactly as a click does.
    Run(Rc<dyn Fn()>),
    /// Open this row's submenu instead. Enter and Right both do it.
    Open,
}

/// The rows a keyboard cursor may land on, in order, paired with what Enter does
/// there.
///
/// **Separators and disabled rows are not stops**, which is the whole reason this
/// is a function rather than a range over the entries: a cursor that could rest on
/// a separator would make Down look like it did nothing, and one that could rest
/// on a disabled row would offer an Enter that silently does nothing — the two
/// failures a menu's arrow keys are most often shipped with.
///
/// The index is into `entries`, so a row view can ask "am I the cursor?" by
/// comparing its own position, while the cursor itself steps through *this* list
/// and therefore cannot land anywhere it shouldn't.
pub(crate) fn menu_stops(entries: &[MenuEntry]) -> Vec<(usize, MenuAct)> {
    entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| match e {
            MenuEntry::Action { disabled: true, .. } | MenuEntry::Separator => None,
            MenuEntry::Action { action, .. } => Some((i, MenuAct::Run(action.clone()))),
            MenuEntry::Sub { .. } => Some((i, MenuAct::Open)),
        })
        .collect()
}

/// One menu row's content: `[icon] label [→]` (the chevron only for submenus).
/// `label_color` tints the label (a `fn` so it follows theme switches); `None`
/// uses the default text colour.
fn menu_row(
    icon: Option<MenuIcon>,
    label: String,
    label_color: Option<fn() -> floem::peniko::Color>,
    chevron: bool,
    disabled: bool,
    cursor: impl Fn() -> bool + 'static,
) -> impl IntoView {
    let mut kids: Vec<AnyView> = Vec::new();
    if let Some((svg, color)) = icon {
        kids.push(
            icons::icon(svg, 16.0)
                .style(move |s| {
                    let c = if disabled {
                        theme::text_muted().multiply_alpha(0.3)
                    } else {
                        color()
                    };
                    s.color(c).flex_shrink(0.0_f32)
                })
                .into_any(),
        );
    }
    kids.push(
        text(label)
            .style(move |s| {
                let c = if disabled {
                    theme::text_muted().multiply_alpha(0.3)
                } else {
                    label_color.map(|c| c()).unwrap_or_else(theme::text)
                };
                s.color(c)
            })
            .into_any(),
    );
    if chevron {
        kids.push(
            empty()
                .style(|s| s.flex_grow(1.0_f32).min_width(20.0))
                .into_any(),
        );
        kids.push(
            icons::icon(icons::CHEVRON_RIGHT, 14.0)
                .style(|s| s.color(theme::text_dim()).flex_shrink(0.0_f32))
                .into_any(),
        );
    }
    h_stack_from_iter(kids)
        .style(menu_item_style)
        .style(move |s| {
            let s = s.padding_vert(8.0);
            // A disabled row suppresses the hover highlight so it reads as inert.
            if disabled {
                s.hover(|h| h.background(floem::peniko::Color::TRANSPARENT))
            } else {
                s
            }
        })
        .style(move |s| {
            // The keyboard cursor wears the **hover** highlight rather than one of
            // its own: the pointer and the arrows move the same cursor (a row's
            // `PointerEnter` sets it), so two different marks would be two answers
            // to one question. `menu_item_style`'s hover already states the
            // colour; this is the same fill, applied because the keyboard is here.
            if cursor() {
                s.background(theme::accent().multiply_alpha(0.15))
            } else {
                s
            }
        })
}

/// Render one entry. `open_sub` is this level's "which sibling submenu is open"
/// signal — entering a leaf clears it, entering a submenu row sets it, so moving
/// between rows switches/closes submenus while moving *onto* an open submenu (it's
/// flush with the panel's right edge) keeps it open.
fn menu_entry_view(i: usize, entry: MenuEntry, level: MenuLevel, close: Rc<dyn Fn()>) -> AnyView {
    let open_sub = level.open_sub;
    let cursor = level.cursor;
    // The pointer and the keyboard share one cursor, so entering a row with the
    // mouse moves it — otherwise a menu opened by Tab and then grazed by the
    // pointer would show two highlights, and Enter would run the one the user
    // wasn't looking at.
    let take_cursor = move || cursor.set(Some(i));
    let is_cursor = move || cursor.get() == Some(i);
    match entry {
        MenuEntry::Separator => empty()
            .style(|s| {
                s.width_full()
                    .height(1.0)
                    .background(theme::border())
                    .margin_vert(4.0)
            })
            .into_any(),
        MenuEntry::Action {
            label,
            icon,
            label_color,
            disabled,
            action,
        } => menu_row(icon, label, label_color, false, disabled, is_cursor)
            .on_click_stop(move |_| {
                if disabled {
                    return; // inert; the stop keeps the menu open
                }
                (action)();
                (close)();
            })
            .on_event(EventListener::PointerEnter, move |_| {
                open_sub.set(None);
                // A disabled row is not a stop, so the keyboard can't rest there
                // and the pointer must not park the cursor there either.
                if !disabled {
                    take_cursor();
                }
                EventPropagation::Continue
            })
            .into_any(),
        MenuEntry::Sub {
            label,
            icon,
            children,
        } => {
            // The panel itself is **not** built here — see "the hoisted submenu".
            // This row only publishes what it would take to draw one, and
            // `submenu_layer` at the root of the window draws it.
            //
            // Publishing is an effect on `open_sub` rather than something the
            // `PointerEnter` below does, because the keyboard opens submenus too
            // (`MenuAct::Open` sets the same signal, and Right/Enter go through it).
            // One place, both ways in.
            let row_rect: RwSignal<Rect> = RwSignal::new(Rect::ZERO);
            {
                let children = children.clone();
                let close = close.clone();
                create_effect(move |_| {
                    if open_sub.get() == Some(i) {
                        hoisted_submenu().set(Some(OpenSubmenu {
                            entries: children.clone(),
                            row: row_rect.get(),
                            level,
                            close: close.clone(),
                        }));
                    }
                    // Closing is *not* this effect's job: every row's effect runs
                    // on every change, so clearing here would race the row that is
                    // opening. `menu_panel` clears when `open_sub` goes `None`.
                });
            }
            container(menu_row(icon, label, None, true, false, is_cursor))
                // The row's rect in window coordinates — `on_move` reports the
                // window origin (fired during layout, not on pointer movement),
                // `on_resize` the size. The layer needs both: it hangs the panel
                // off the row's right or left edge, and lines its first item up
                // with the row's top.
                .on_move(move |p| row_rect.update(|r| *r = Rect::from_origin_size(p, r.size())))
                .on_resize(move |b| {
                    row_rect.update(|r| *r = Rect::from_origin_size(r.origin(), b.size()))
                })
                .on_event(EventListener::PointerEnter, move |_| {
                    open_sub.set(Some(i));
                    take_cursor();
                    EventPropagation::Continue
                })
                .on_click_stop(|_| {}) // clicking the parent just holds it open
                .into_any()
        }
    }
}

/// One menu level: the styled panel of rows (used for the root and every submenu).
/// `width` is the panel's `min_width` (short labels never exceed it).
fn menu_stack(
    entries: Vec<MenuEntry>,
    level: MenuLevel,
    close: Rc<dyn Fn()>,
    width: f64,
) -> impl IntoView {
    let rows: Vec<AnyView> = entries
        .into_iter()
        .enumerate()
        .map(|(i, e)| menu_entry_view(i, e, level, close.clone()))
        .collect();
    v_stack_from_iter(rows)
        .on_event_stop(EventListener::PointerDown, |_| {})
        .style(move |s| {
            panel_style(s)
                .background(theme::bg_chrome())
                .min_width(width)
                .padding_vert(6.0)
                .font_size(theme::FONT_TITLE)
        })
}

/// Gap between the cursor and the corner of a menu opened at it.
pub(crate) const CURSOR_MENU_GAP: f64 = 3.0;

/// Estimated height (px) of the panel [`menu_panel`] builds for `entries`.
///
/// Summed per entry *kind*: an action row is ≈30.5px (14px line + 8px padding on
/// both sides − sub-pixel), a separator ≈9px (a 1px rule + 4px margins), plus the
/// panel's own 6px vertical padding and 1px border on both sides. Counting
/// separators as full rows shoved an upward-flipped panel tens of px too high.
///
/// It is an estimate on purpose: it decides *placement*, not whether to flip, and
/// measuring for real would mean laying the panel out first — which is what
/// produces an open-then-flip flicker.
///
/// Measured over [`tidy_separators`]'s output, not the list as given: the panel
/// drops the separators that divide nothing, and a height that counted them would
/// flip an upward-opening menu a rule too high.
pub(crate) fn menu_panel_height(entries: &[MenuEntry]) -> f64 {
    separator_keep(entries)
        .into_iter()
        .zip(entries)
        .filter(|(keep, _)| *keep)
        .map(|(_, e)| match e {
            MenuEntry::Separator => 9.0,
            _ => 30.5,
        })
        .sum::<f64>()
        + 14.0
}

/// `entries` with its separators tidied: leading and trailing ones dropped, runs
/// of two or more collapsed to one, and the same applied inside every submenu.
///
/// Applied by [`menu_panel`] itself (and by [`menu_panel_height`], so placement
/// measures what is drawn) rather than by each caller, because a builder pushes a
/// group's separator *before* it knows whether the group has any entries. The
/// schema tree's column menu pushes one and then asks
/// `overlays::field_entries` whether Edit column and Drop are offered at all — on a
/// view's column neither is, and what shipped was a rule with nothing under it: an
/// empty section between "Copy qualified name" and AI Explain. Every conditional
/// group in that tree can reach the same shape, so this is the one place it is
/// fixed.
pub(crate) fn tidy_separators(entries: Vec<MenuEntry>) -> Vec<MenuEntry> {
    let keep = separator_keep(&entries);
    entries
        .into_iter()
        .zip(keep)
        .filter(|(_, keep)| *keep)
        .map(|(e, _)| match e {
            MenuEntry::Sub {
                label,
                icon,
                children,
            } => MenuEntry::Sub {
                label,
                icon,
                children: tidy_separators(children),
            },
            other => other,
        })
        .collect()
}

/// Which of `entries` survive [`tidy_separators`], one flag per entry. Shared with
/// [`menu_panel_height`] so the panel that is measured and the panel that is drawn
/// cannot disagree about how many rules they have.
///
/// Two passes, because "is there anything after this separator" needs the whole
/// list: the first keeps a separator only when the last kept entry was a row, which
/// drops leading ones and collapses runs; the second walks back from the end and
/// drops the one trailing separator the first pass can leave.
fn separator_keep(entries: &[MenuEntry]) -> Vec<bool> {
    let mut keep = Vec::with_capacity(entries.len());
    // Whether the last kept entry was a separator. `None` = nothing kept yet, so a
    // separator here would be the panel's first row.
    let mut last_kept_is_separator: Option<bool> = None;
    for e in entries {
        let is_separator = matches!(e, MenuEntry::Separator);
        let k = !is_separator || last_kept_is_separator == Some(false);
        if k {
            last_kept_is_separator = Some(is_separator);
        }
        keep.push(k);
    }
    for i in (0..entries.len()).rev() {
        if !keep[i] {
            continue;
        }
        if matches!(entries[i], MenuEntry::Separator) {
            keep[i] = false;
        }
        break;
    }
    keep
}

/// Top-left corner for a panel opened **at the cursor**: `gap` px down and right
/// of it, flipped to the other side of the cursor on whichever axis would run past
/// the window edge, and never negative.
///
/// Shared by both menu channels. It used to live in the grid's overlay only, so a
/// right-click low in the schema tree ran its last entries — Truncate, Drop, AI
/// Explain — off the bottom of the window with no cue that they existed, while a
/// grid cell right-clicked at the same height flipped up correctly.
///
/// A `window` dimension of 0 (or less than 1) means "not measured yet" and
/// suppresses the flip on that axis: guessing at an unknown edge is worse than
/// opening down-right and being off by a frame.
pub(crate) fn cursor_menu_pos(
    cursor: (f64, f64),
    panel: (f64, f64),
    window: (f64, f64),
    gap: f64,
) -> (f64, f64) {
    let flip = |c: f64, size: f64, win: f64| {
        if win > 1.0 && c + gap + size > win {
            (c - size - gap).max(0.0)
        } else {
            c + gap
        }
    };
    (
        flip(cursor.0, panel.0, window.0),
        flip(cursor.1, panel.1, window.1),
    )
}

/// One menu level's keyboard state: where the cursor is, which sibling's submenu
/// is open, and the cursor of *that* submenu.
///
/// One level of nesting, deliberately, and it is the level the widget itself
/// supports — only the grid menus have submenus, and none of them nests twice.
/// `sub` being the parent's field rather than the child's own is what lets
/// [`menu_panel`]'s one key handler drive whichever level is open without walking
/// a tree it would then have to keep in step with the views.
#[derive(Clone, Copy)]
pub(crate) struct MenuLevel {
    cursor: RwSignal<Option<usize>>,
    open_sub: RwSignal<Option<usize>>,
    /// The cursor a child [`menu_stack`] uses. Unused at the child's own level.
    sub: MenuSub,
}

/// The child level's signals — the same three fields, minus a third generation.
#[derive(Clone, Copy)]
struct MenuSub {
    cursor: RwSignal<Option<usize>>,
    open_sub: RwSignal<Option<usize>>,
}

impl MenuLevel {
    fn new() -> MenuLevel {
        MenuLevel {
            cursor: RwSignal::new(None),
            open_sub: RwSignal::new(None),
            sub: MenuSub {
                cursor: RwSignal::new(None),
                open_sub: RwSignal::new(None),
            },
        }
    }
}

impl From<MenuSub> for MenuLevel {
    /// A child level, with a `sub` of its own that nothing opens — the widget
    /// stops at one level of submenu and this is where that stops.
    fn from(s: MenuSub) -> MenuLevel {
        MenuLevel {
            cursor: s.cursor,
            open_sub: s.open_sub,
            sub: MenuSub {
                cursor: RwSignal::new(None),
                open_sub: RwSignal::new(None),
            },
        }
    }
}

// ── The hoisted submenu ─────────────────────────────────────────────────────
//
// A submenu is drawn by [`submenu_layer`], a sibling at the **root** of the window
// stack, rather than as a child of the row it belongs to.
//
// It has to be. Floem hit-tests a subtree through `EventCx::should_send`, which
// tests `id.layout_rect().with_origin(layout.location)` — the *size* of the union
// of a view and its children, re-anchored at the view's **own** origin. An
// overflowing child therefore grows its parent's hit area rightward and downward
// only, and a submenu that flips to the left of its row (or shifts up past it)
// lands inside the union that produced the size and outside the rectangle that
// gets tested. It is `continue`d past and the pointer reaches whatever is
// underneath. Paint never consults `should_send`, so the thing renders perfectly
// and answers neither hover nor click — the failure reads as "this menu has no
// event handling", not as an edge-flip, which is how it survived unnoticed in
// every menu that opens near the right edge of the window.
//
// Hoisted, the submenu's only ancestor is the root stack, whose box is the window,
// so no ancestor can crop it however it flips. The cost is that it is no longer a
// view-tree descendant of the panel, which two things depended on: dismissal (the
// panel absorbed its children's pointer-downs — [`menu_stack`] does that for the
// hoisted copy too, being the same view) and the parent row's hover highlight
// (the keyboard cursor sits on that row while its submenu is open, and the cursor
// wears the hover fill, so the row stays lit).

/// The submenu currently expanded out of a [`menu_panel`] row.
///
/// Carried as data rather than as a view, because the layer that draws it is built
/// once at the root of the window and has to be able to draw *any* menu's submenu.
/// `MenuEntry` is `Clone` and the close action is an `Rc`, so this is cheap.
#[derive(Clone)]
pub(crate) struct OpenSubmenu {
    /// Already through [`tidy_separators`] — the parent panel tidies recursively
    /// before any row is built, so these are the entries as drawn and as measured.
    pub entries: Vec<MenuEntry>,
    /// The parent row's rect in **window** coordinates. The panel hangs off its
    /// right edge, or its left when there is no room.
    pub row: Rect,
    /// The **parent** level. The rows drawn from `entries` read `level.sub`, which
    /// is the cursor `menu_key` drives while the submenu is the open one.
    pub level: MenuLevel,
    /// The whole menu's close action, which an action row runs after its own.
    pub close: Rc<dyn Fn()>,
}

/// The stored open-submenu signal plus the scope that owns it.
type OpenSubmenuSlot = (RwSignal<Option<OpenSubmenu>>, Scope);

thread_local! {
    static OPEN_SUBMENU: std::cell::RefCell<Option<OpenSubmenuSlot>> =
        const { std::cell::RefCell::new(None) };
}

/// The channel [`submenu_layer`] draws from.
///
/// A **detached scope**, deliberately: the signal has to outlive any individual
/// menu, since what publishes into it is a row inside a panel that is disposed the
/// moment the menu closes. The same arrangement [`window_size`] uses.
pub(crate) fn hoisted_submenu() -> RwSignal<Option<OpenSubmenu>> {
    OPEN_SUBMENU.with(|cell| {
        if cell.borrow().is_none() {
            let scope = Scope::new();
            let sig = scope.create_rw_signal(None);
            *cell.borrow_mut() = Some((sig, scope));
        }
        cell.borrow().as_ref().unwrap().0
    })
}

/// A submenu panel's `min_width`. Submenus keep the standard menu width whatever
/// the parent asked for — a wide root menu doesn't make its children wide.
const SUBMENU_W: f64 = 170.0;

/// Conservative width estimate for the *decision* to flip a submenu left. Only the
/// decision — the placement itself is exact (`inset_right` pins the real panel's
/// right edge to the row's left edge whatever it measures), so an estimate here
/// costs at worst a flip that wasn't needed, never a gap or an open-then-flip
/// flicker.
const SUBMENU_FLIP_W: f64 = 210.0;

/// The window-level layer that draws the open submenu. **Last in the root stack**,
/// so it is over every other surface including the popup menu it belongs to.
///
/// It is out of flow and shrink-wrapped to the panel, not a full-window sheet: a
/// full-window layer would claim every pointer event in the window (Floem stops at
/// the first child whose bounds contain the point, handled or not), so a click
/// meant for the app underneath would be swallowed while a menu was open.
pub(crate) fn submenu_layer() -> impl IntoView {
    let open = hoisted_submenu();
    dyn_container(
        // Keyed on what identifies *which* submenu this is. `dyn_container` is
        // built on `create_updater` and doesn't diff, so this rebuilds on every
        // change to the channel, which is what we want: a different row's submenu
        // is a different panel.
        move || open.get().map(|s| (s.row, s.entries.len())),
        move |slot| {
            if slot.is_none() {
                return empty().into_any();
            }
            let Some(s) = open.get_untracked() else {
                return empty().into_any();
            };
            // `level.sub` is the child level — the cursor `menu_key` drives while
            // this submenu is open, which is what keeps the keyboard working
            // across the hoist.
            menu_stack(s.entries, s.level.sub.into(), s.close, SUBMENU_W).into_any()
        },
    )
    .style(move |st| {
        let Some(s) = open.get() else {
            return st;
        };
        let (x, y) = submenu_insets(s.row, window_size().get(), menu_panel_height(&s.entries));
        let st = st.absolute();
        let st = match x {
            SubX::Left(v) => st.inset_left(v),
            SubX::Right(v) => st.inset_right(v),
        };
        match y {
            SubY::Top(v) => st.inset_top(v),
            SubY::Bottom(v) => st.inset_bottom(v),
        }
    })
}

/// How the hoisted submenu pins horizontally: its left edge at a distance from the
/// window's left, or its **right** edge at a distance from the window's right.
#[derive(Clone, Copy, Debug, PartialEq)]
enum SubX {
    Left(f64),
    Right(f64),
}

/// How it pins vertically — top edge from the window's top, or bottom edge from
/// the window's bottom.
#[derive(Clone, Copy, Debug, PartialEq)]
enum SubY {
    Top(f64),
    Bottom(f64),
}

/// Lift, so the submenu's first item lines up with the row it came from rather
/// than starting below it.
const SUBMENU_LIFT: f64 = 6.0;

/// Where the hoisted submenu goes for a parent row at `row` (window coordinates),
/// in a `win` window, given the panel's estimated height `h`.
///
/// Flush to the row's right edge, or to its **left** edge when the window has no
/// room on the right — flush either way, so a diagonal move from the row onto the
/// submenu never crosses a gap it could close over. The flipped case pins the
/// *right* edge (`SubX::Right`, an inset from the window's right) rather than
/// computing a left edge from an assumed width: the panel is `min_width`, so its
/// real width isn't known here, and a guess would leave a visible gap between the
/// submenu and the row exactly when the flip happened. Vertically the same trick —
/// a panel that won't fit below the row pins its bottom to the window's, no
/// height arithmetic involved.
///
/// [`SUBMENU_FLIP_W`] and `h` are *estimates*, and only ever decide **which** edge
/// to pin. The pin itself is exact, so an estimate that is off costs at worst a
/// flip that wasn't needed — never a gap, and never an open-then-measure-then-move
/// flicker.
///
/// Pure, and tested, because every way this can be wrong is silent: a sign slip or
/// an `x1` where an `x0` belongs still renders a submenu, just not next to the row
/// that opened it. A degenerate window (nothing measured yet) never flips.
fn submenu_insets(row: Rect, win: (f64, f64), h: f64) -> (SubX, SubY) {
    let (win_w, win_h) = win;
    let x = if win_w > 1.0 && row.x1 + SUBMENU_FLIP_W > win_w {
        SubX::Right(win_w - row.x0)
    } else {
        SubX::Left(row.x1)
    };
    let y = if win_h > 1.0 && row.y0 - SUBMENU_LIFT + h > win_h {
        SubY::Bottom(0.0)
    } else {
        SubY::Top(row.y0 - SUBMENU_LIFT)
    };
    (x, y)
}

thread_local! {
    static MENU_RETURN: std::cell::RefCell<Option<Rc<dyn Fn()>>> =
        const { std::cell::RefCell::new(None) };
}

/// **Where focus goes when the popup menu about to open closes again.**
///
/// A menu panel is a [`focus_root`], so it takes the keyboard while it is up and
/// its teardown calls [`hand_keyboard_back`] — which hands to the innermost *other*
/// focus root. Inside a modal that is the modal. In the **main workspace** there
/// is none, so focus is simply dropped: close the grid toolbar's Copy menu with
/// Escape and nothing is focused, which meant F6 (a listener on the grid body)
/// stopped working entirely until something was clicked.
///
/// Set by an opener that was reached from the keyboard, and **taken by
/// [`menu_panel`] as it builds** — so the slot lives only between the two and
/// cannot go stale. A menu opened with no return set simply has none.
///
/// Only the keyboard wants this: after a *click*, moving focus to the control
/// that was clicked would take the arrow keys away from whatever had them
/// (the grid's own cell navigation). `keyboard_nav` is what an opener asks.
pub(crate) fn set_menu_return(f: Rc<dyn Fn()>) {
    MENU_RETURN.with_borrow_mut(|s| *s = Some(f));
}

fn take_menu_return() -> Option<Rc<dyn Fn()>> {
    MENU_RETURN.with_borrow_mut(|s| s.take())
}

/// **Is the menu currently up the one anchored at `mine`?** — how a trigger on the
/// shared `popup_menu` channel closes *its own* menu on a second press instead of
/// dismissing and rebuilding an identical panel.
///
/// The channel is one slot serving every opener in the app and carries no tag
/// saying who filled it, so the **anchor** stands in for one: a trigger compares
/// `popup_anchor` against the [`crate::PopupAnchor`] it would set itself. That is
/// self-invalidating by construction — every opener overwrites the anchor as it
/// opens, so there is no separate marker to go stale and nothing for the other
/// openers to reset. A tag beside the channel is the thing this replaced: written
/// only by the triggers that cared, it kept naming a status-bar segment after a
/// right-click elsewhere had replaced the menu, and the segment then closed
/// someone else's.
///
/// `open` is whether the channel holds anything at all, and it is checked **first**
/// for a reason: closing clears `popup_menu` but leaves `popup_anchor` naming the
/// last opener, so the anchor alone would still name this trigger long after
/// Escape dismissed its menu, and the next press would "toggle shut" a menu that
/// isn't there.
///
/// Named rather than inlined so the rule is stated once and tested. Spelled out at
/// each call site it is a copy of an `&&` that is only obviously right once you
/// know why the order matters.
pub(crate) fn menu_anchored_at(
    open: bool,
    anchor: Option<crate::PopupAnchor>,
    mine: crate::PopupAnchor,
) -> bool {
    open && anchor == Some(mine)
}

/// A reusable themed popup menu with nested submenus, `width` px wide. Returns the
/// panel; the caller positions it absolutely. Escape (and any action) calls `close`.
///
/// **Fully operable from the keyboard**, which it was not: the panel took focus
/// (it is a [`focus_root`]) and answered Escape, but nothing moved a cursor and no
/// row was marked, so a menu opened with Enter from a ringed button — the type
/// chevron in the designer, every dropdown built on this channel — could only be
/// finished with the mouse, and read as though it had never taken focus at all.
///
/// Up/Down step [`menu_stops`] and **wrap**, the swatches' rule rather than the
/// item list's: a menu is short and its ends are visibly adjacent, so wrapping is
/// the shorter path to the last entry rather than a surprise. Home/End jump.
/// Enter or Space runs the cursor row and closes; on a submenu row, Enter and
/// Right open it and take the cursor inside, Left comes back out. Escape closes
/// the submenu first if one is open, and only then the menu — so the key that
/// means "back" doesn't skip a level.
pub(crate) fn menu_panel(
    entries: Vec<MenuEntry>,
    close: Rc<dyn Fn()>,
    width: f64,
) -> impl IntoView {
    // First, before anything indexes into the list: a builder pushes a group's
    // separator before it knows whether the group has any entries, and a rule with
    // nothing under it is a visible empty section. Done here so `menu_stops` and the
    // rows below agree on what position each entry is at.
    let entries = tidy_separators(entries);
    let level = MenuLevel::new();
    // Taken at build, so the slot is empty again the moment this panel owns it.
    // Folded into `close`, which is what Escape and every action call — the one
    // path that matters, because it is the keyboard's. A click-away dismissal
    // sets the channel to `None` directly and skips this, which is right: the
    // pointer put focus wherever it clicked.
    let close = match take_menu_return() {
        None => close,
        Some(back) => Rc::new(move || {
            (close)();
            (back)();
        }) as Rc<dyn Fn()>,
    };
    // **Closing the hoisted submenu is this level's job, not a row's.** A `Sub`
    // row publishes when `open_sub` becomes its own index; every row's effect runs
    // on every change, so a row that also cleared would race the row that is
    // opening. Here there is one of them, and `None` means exactly one thing.
    //
    // It also runs once on open, with `open_sub` still `None`, which sweeps up a
    // submenu left behind by a menu that was dismissed by a click rather than
    // closed — the same case `workspace`'s channel effect covers from the other
    // side. This is the root level only; a submenu's own `menu_stack` never gets
    // this effect, and must not, or it would clear the channel it is drawn from.
    create_effect(move |_| {
        if level.open_sub.get().is_none() {
            hoisted_submenu().set(None);
        }
    });
    // Taken before the entries become views, which consumes them. Each `Sub`'s
    // children are kept for the same reason, keyed by the row they hang off.
    let stops = Rc::new(menu_stops(&entries));
    let sub_stops: Rc<std::collections::HashMap<usize, Vec<(usize, MenuAct)>>> = Rc::new(
        entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| match e {
                MenuEntry::Sub { children, .. } => Some((i, menu_stops(children))),
                _ => None,
            })
            .collect(),
    );
    let esc = close.clone();
    let act = close.clone();
    focus_root(menu_stack(entries, level, close, width))
        .on_key_down(
            Key::Named(NamedKey::Escape),
            |_| true,
            move |_| {
                // One level at a time: an open submenu is what Escape closes
                // first, so "back" never skips past the menu the user was in.
                if level.open_sub.get_untracked().is_some() {
                    level.open_sub.set(None);
                    level.sub.cursor.set(None);
                    return;
                }
                (esc)();
            },
        )
        .on_event(EventListener::KeyDown, move |e| {
            let Event::KeyDown(ke) = e else {
                return EventPropagation::Continue;
            };
            let Key::Named(k) = ke.key.logical_key else {
                return EventPropagation::Continue;
            };
            menu_key(k, level, &stops, &sub_stops, &act)
        })
}

/// One keypress against an open menu. Split out so the whole decision is in one
/// place rather than spread down a listener, and `Continue` is returned for
/// anything this doesn't claim.
fn menu_key(
    k: NamedKey,
    level: MenuLevel,
    stops: &[(usize, MenuAct)],
    sub_stops: &std::collections::HashMap<usize, Vec<(usize, MenuAct)>>,
    close: &Rc<dyn Fn()>,
) -> EventPropagation {
    // Which level the keys drive: the submenu when one is open, else the root.
    // The open row also names which stop list the submenu's cursor indexes.
    let open = level.open_sub.get_untracked();
    let (cursor, stops) = match open.and_then(|i| sub_stops.get(&i)) {
        Some(sub) => (level.sub.cursor, sub.as_slice()),
        None => (level.cursor, stops),
    };
    // A stop's *position in the list*, since the signal holds its entry index —
    // the two differ wherever a separator or a disabled row sits above.
    let pos = cursor
        .get_untracked()
        .and_then(|entry| stops.iter().position(|(i, _)| *i == entry));
    let step = |backwards: bool| {
        if let Some(p) = ring_step(stops.len(), pos, backwards)
            && let Some((entry, _)) = stops.get(p)
        {
            cursor.set(Some(*entry));
        }
    };
    match k {
        NamedKey::ArrowDown => step(false),
        NamedKey::ArrowUp => step(true),
        NamedKey::Home => {
            if let Some((entry, _)) = stops.first() {
                cursor.set(Some(*entry));
            }
        }
        NamedKey::End => {
            if let Some((entry, _)) = stops.last() {
                cursor.set(Some(*entry));
            }
        }
        // Into a submenu — but only from the root, since nothing nests twice.
        NamedKey::ArrowRight => {
            if open.is_some() {
                return EventPropagation::Continue;
            }
            let Some((entry, MenuAct::Open)) = pos.and_then(|p| stops.get(p)) else {
                return EventPropagation::Continue;
            };
            open_submenu(level, *entry, sub_stops);
        }
        // …and back out of it. The parent's cursor is still on the row that
        // opened it, so there is nothing to restore.
        NamedKey::ArrowLeft => {
            if open.is_none() {
                return EventPropagation::Continue;
            }
            level.open_sub.set(None);
            level.sub.cursor.set(None);
        }
        NamedKey::Enter | NamedKey::Space => {
            let Some((entry, act)) = pos.and_then(|p| stops.get(p)) else {
                return EventPropagation::Continue;
            };
            match act {
                MenuAct::Run(run) => {
                    (run)();
                    (close)();
                }
                // Enter on a submenu row opens it, as Right does — the row has no
                // action of its own, so closing the menu on it would be a keypress
                // that threw the menu away and did nothing.
                MenuAct::Open => open_submenu(level, *entry, sub_stops),
            }
        }
        _ => return EventPropagation::Continue,
    }
    EventPropagation::Stop
}

/// Open the submenu hanging off entry `row` and put the cursor on its first stop,
/// so the next Down is a *second* row rather than the arrival.
fn open_submenu(
    level: MenuLevel,
    row: usize,
    sub_stops: &std::collections::HashMap<usize, Vec<(usize, MenuAct)>>,
) {
    level.open_sub.set(Some(row));
    level
        .sub
        .cursor
        .set(sub_stops.get(&row).and_then(|s| s.first()).map(|(i, _)| *i));
}

/// Measure a string's rendered width (px) at `FONT_BODY`, through the same global
/// `FontSystem` the views paint with, so the measurement matches to the pixel.
/// Used to right-align the numeric grid editor and to size/ellipsize tab titles.
pub(crate) fn measure_text_px(text: &str) -> f64 {
    measure_text_px_at(text, theme::FONT_BODY)
}

/// Like [`measure_text_px`] but at an explicit font size (e.g. the 16px Find box).
pub(crate) fn measure_text_px_at(text: &str, size: f32) -> f64 {
    measure_text_px_weighted(text, size, false)
}

/// As [`measure_text_px_at`], but for text rendered `.font_bold()`. Bold glyphs are
/// wider, so measuring them at regular weight under-reports — enough that a name
/// sized to the regular measurement still ellipsizes when drawn bold.
pub(crate) fn measure_text_px_bold_at(text: &str, size: f32) -> f64 {
    measure_text_px_weighted(text, size, true)
}

/// As [`measure_text_px_at`], but in the app's monospace family — what the SQL
/// surfaces (the Ctrl+K diff, the DDL preview's script box) actually render with.
///
/// A measurement in the wrong family is the same class of error as one in the
/// wrong weight, and it is the one the diff used to make *without* measuring at
/// all: it multiplied a `chars().count()` by a comment-documented advance
/// (`8.43`). That is exact for ASCII and half the truth for a full-width glyph,
/// so a line with CJK text in a string literal reported a content width narrower
/// than it draws, and the horizontal scrollbar stopped before the end of the line.
pub(crate) fn measure_mono_px_at(text: &str, size: f32) -> f64 {
    measure_text_px_styled(text, size, false, true)
}

fn measure_text_px_weighted(text: &str, size: f32, bold: bool) -> f64 {
    measure_text_px_styled(text, size, bold, false)
}

/// [`measure_text_px_at`]'s other axis: the line box a label of this size
/// occupies. What [`action_height`] is built on, so a button's height comes from
/// the text it holds rather than from a number somebody picked.
fn measure_text_h_at(text: &str, size: f32) -> f64 {
    use floem::text::{Attrs, AttrsList, TextLayout};
    let mut layout = TextLayout::new();
    layout.set_text(text, AttrsList::new(Attrs::new().font_size(size)));
    layout.size().height
}

fn measure_text_px_styled(text: &str, size: f32, bold: bool, mono: bool) -> f64 {
    use floem::text::{Attrs, AttrsList, FamilyOwned, TextLayout, Weight};
    let family = [FamilyOwned::Name(crate::consts::MONO_FAMILY.into())];
    let mut attrs = Attrs::new().font_size(size);
    if bold {
        attrs = attrs.weight(Weight::BOLD);
    }
    if mono {
        attrs = attrs.family(&family);
    }
    let mut layout = TextLayout::new();
    layout.set_text(text, AttrsList::new(attrs));
    layout.size().width
}

/// Shared scrollbar styling. Handle color/thickness/rounding come from the global
/// `Handle` class on the root; this only adds the 3px edge inset (a per-scroll prop
/// that doesn't cascade) so the bar floats off the edge and clears the resize grip.
pub(crate) fn thin_scroll(s: ScrollCustomStyle) -> ScrollCustomStyle {
    s.vertical_track_inset(3.0).horizontal_track_inset(3.0)
}

/// Is the viewport parked at the bottom of the content, within `slack` px?
///
/// The predicate behind every tail-following list (the AI conversation, the Live
/// Monitor's log): while it holds, new content scrolls into view; once the user
/// scrolls up it goes false and the follow must be *released*, which — as
/// `[[floem-scroll-follow]]` records — means the `scroll_to` closure returning
/// `None`, not merely leaving its trigger un-bumped. A `scroll_to` target is
/// sticky: the last one stays applied.
///
/// It is also what decides whether to offer a jump-to-bottom button, so the two
/// can't disagree about where "the bottom" is.
pub(crate) fn at_content_bottom(content_h: f64, viewport_bottom: f64, slack: f64) -> bool {
    content_h - viewport_bottom <= slack
}

/// Backstop expiry on an unconsumed gesture: a wheel against the end of the
/// content moves nothing, so it raises no `on_scroll` to spend its stamp, and a
/// stamp left lying around would be spent by whatever scrolled next.
const SCROLL_GESTURE_MS: u64 = 500;

/// Answers whether the scroll that just happened was the user's. **Consuming** —
/// call it exactly once per `on_scroll`.
pub(crate) type ScrollGestureByUser = Rc<dyn Fn() -> bool>;

/// Attaches the pointer listeners a tail-following scroll needs, and returns it
/// with the `by_user` predicate to hand [`follow_after_scroll`].
///
/// One gesture explains exactly **one** scroll, which is why reading the flag
/// clears it. A time window alone is not enough and was itself a bug: the
/// relayout that a streamed chunk triggers lands within milliseconds of the
/// wheel that preceded it, so the clamp it causes was attributed to the reader,
/// recorded the clamped position as the one they had chosen, and then held them
/// at the top for the rest of the answer. The expiry above is only a backstop for
/// a gesture that never scrolled anything.
///
/// A scrollbar drag needs the `held` flag: floem scrolls it from pointer *moves*,
/// and moves must not count while nothing is pressed, or merely resting the
/// cursor over the conversation would make every relayout look deliberate.
///
/// **`held` is cleared by leaving as well as by releasing**, and it has to be.
/// The Scroll never requests active, so a `PointerUp` outside it is delivered
/// somewhere else entirely: press inside the conversation, drag out of the panel,
/// release, and the flag **latched on for the life of the view**. Every later
/// pointer move then stamped a gesture — precisely what the flag exists to
/// prevent — so the follow was released mid-answer by nothing but a cursor
/// passing over. `PointerLeave` closes it because leaving with the button down
/// is the only way to reach an unseen release: floem delivers `PointerUp` to
/// whatever is under the cursor, so a release back inside still lands here.
pub(crate) fn with_scroll_gesture(s: Scroll) -> (Scroll, ScrollGestureByUser) {
    // Plain cells, not signals: these are read from `on_scroll` on the very view
    // whose scrolling writes them, and they outlive nothing.
    let pending: Rc<std::cell::Cell<Option<Instant>>> = Rc::new(std::cell::Cell::new(None));
    let held: Rc<std::cell::Cell<bool>> = Rc::new(std::cell::Cell::new(false));
    let s = s
        // Registered listeners run in the scroll's `event_after_children`, ahead
        // of its own scrolling, so the stamp is in place before the `on_scroll`
        // it causes. `_cont` so the scroll still handles the event itself.
        .on_event_cont(EventListener::PointerWheel, {
            let p = pending.clone();
            move |_| p.set(Some(Instant::now()))
        })
        .on_event_cont(EventListener::PointerDown, {
            let (p, h) = (pending.clone(), held.clone());
            move |_| {
                h.set(true);
                p.set(Some(Instant::now()));
            }
        })
        .on_event_cont(EventListener::PointerUp, {
            let h = held.clone();
            move |_| h.set(false)
        })
        // The release we would otherwise never see: no pointer capture, so a
        // drag that ends outside this view delivers its `PointerUp` elsewhere and
        // the flag latches on forever. `PointerLeave` catches it first and
        // cheaply, which matters because the stamp it guards expires in 500 ms.
        .on_event_cont(EventListener::PointerLeave, {
            let h = held.clone();
            move |_| h.set(false)
        });
    // …and the backstop for what `PointerLeave` can still miss — a window that
    // loses focus mid-drag, or a release delivered to a view the pointer never
    // visibly left. The root sees every release; see [`pointer_released`].
    {
        let h = held.clone();
        floem::reactive::create_effect(move |_| {
            pointer_released().track();
            h.set(false);
        });
    }
    let s = s.on_event_cont(EventListener::PointerMove, {
        let (p, h) = (pending.clone(), held.clone());
        move |_| {
            if h.get() {
                p.set(Some(Instant::now()));
            }
        }
    });
    let by_user: ScrollGestureByUser = Rc::new(move || {
        matches!(pending.replace(None),
            Some(t) if t.elapsed() < std::time::Duration::from_millis(SCROLL_GESTURE_MS))
    });
    (s, by_user)
}

/// Whether a tail-following view should still be following after its viewport
/// moved. `by_user` says whether a wheel, drag or pointer-down caused the move.
///
/// The position alone cannot answer this, and every version that tried released
/// the follow permanently mid-stream. The reason is that a tail-following list
/// rebuilds its content as it grows, and a rebuilt child is measured *before*
/// its text re-wraps to the pinned panel width — wider means fewer lines, so the
/// content momentarily collapses to roughly half its height. While it is short,
/// floem clamps the offset to `y0 = 0` (content shorter than the viewport) and
/// reports that from `on_scroll`. Geometrically it is indistinguishable from the
/// reader jumping to the top: the viewport is nowhere near the bottom, and the
/// top edge moved up. Both readings released the follow, after which `scroll_to`
/// returns `None` for good — the content grows under a viewport that never moves
/// again, and floem's clamp early-returns without repainting, so even the
/// scrollbar sits stale until a manual scroll snaps it to the top.
///
/// What the geometry can't say, the input can: the *reader* leaving the bottom
/// is always a gesture, and a relayout never is. So a move that no gesture
/// explains leaves the decision alone, which is what makes the collapse
/// survivable — the next chunk re-pins to the bottom as if nothing happened.
///
/// Reaching the bottom re-arms, and it is tested first so the clamp that lands
/// there during a collapse (or on a cleared conversation) recovers the follow
/// rather than needing the user to ask for it back.
pub(crate) fn follow_after_scroll(
    following: bool,
    by_user: bool,
    viewport_bottom: f64,
    content_h: f64,
    slack: f64,
) -> bool {
    if at_content_bottom(content_h, viewport_bottom, slack) {
        true
    } else if by_user {
        false
    } else {
        following
    }
}

/// The height floor a tail-following list holds under itself, given the height
/// just measured.
///
/// The sibling decision to [`follow_after_scroll`], and it exists for the same
/// reason: a rebuilt `RichText` reports its **unwrapped** height for one layout
/// pass, so a streaming list momentarily collapses and floem's clamp drags the
/// reader with it. Holding the tallest height seen makes the dip invisible — the
/// list renders a few pixels of slack for one frame instead of collapsing.
///
/// `invalidated` is the other half, and it was missing. "A message only ever
/// grows" is true of *streaming* and false of a **re-layout**: dragging the panel
/// wider re-wraps every bubble shorter, and a floor that only ever rises left
/// ~300px of blank under the last message — which then measured as content, lit
/// the jump-to-bottom button, and snapped the next follow to the bottom of the
/// blank. It is false of a conversation *switch* too. So whatever legitimately
/// changes the true height (the message count, the wrap width, which
/// conversation) releases the floor, and only within one of those is it held.
pub(crate) fn next_floor(prev: f64, measured: f64, invalidated: bool) -> f64 {
    if invalidated {
        measured
    } else {
        prev.max(measured)
    }
}

/// Auto-hide: bars stay hidden until content is scrolled; each scroll shows them
/// and (re)arms a timer that hides them SCROLL_HIDE_MS after scrolling stops. The
/// generation guard ensures only the latest scroll's timer fires.
///
/// Per-scroll auto-hide state: a `shown` flag for `hide_bars(!shown)`, plus a
/// `poke()` to call from `on_scroll` (marks shown + re-arms the hide timer).
pub(crate) fn autohide_state() -> (RwSignal<bool>, Rc<dyn Fn()>) {
    let shown = RwSignal::new(false);
    let generation: RwSignal<u64> = RwSignal::new(0);
    let poke: Rc<dyn Fn()> = Rc::new(move || {
        // A scroll can fire *after* this scope was disposed — e.g. Floem defers a
        // `scroll_to` clamp and the pane rebuilt meanwhile (adding a grid row). The
        // signals are gone by then, so bail instead of unwrapping a disposed
        // `RwSignal` (`get_untracked` panics on `None`; `try_*` no-ops).
        let Some(cur) = generation.try_get_untracked() else {
            return;
        };
        shown.set(true);
        let g = cur.wrapping_add(1);
        generation.set(g);
        floem::action::exec_after(
            std::time::Duration::from_millis(SCROLL_HIDE_MS),
            move |_| {
                // Only hide if no later scroll re-armed the timer (and the view
                // still exists — try_get is None once its scope is disposed).
                if generation.try_get_untracked() == Some(g) {
                    shown.set(false);
                }
            },
        );
    });
    (shown, poke)
}

/// Wrap a scroll so its bars auto-hide (thin styling + `hide_bars` reactive on
/// scroll activity). Use for `scroll()`/`shift_hscroll()` views that don't need a
/// custom `on_scroll` of their own (the results grid wires this inline instead).
pub(crate) fn autohide(s: Scroll) -> Scroll {
    let (shown, poke) = autohide_state();
    s.scroll_style(move |cs| thin_scroll(cs).hide_bars(!shown.get()))
        .on_scroll(move |_| poke())
}

/// Wrap a child in a `scroll` that also treats **Shift + wheel** as horizontal
/// scrolling (common browser/app combo). The built-in scroll runs registered
/// `PointerWheel` listeners first, so we intercept Shift there and drive a
/// horizontal delta (signals don't dedupe, so repeated deltas re-fire).
pub(crate) fn shift_hscroll<V: IntoView + 'static>(child: V) -> Scroll {
    let wheel: RwSignal<floem::kurbo::Vec2> = RwSignal::new(floem::kurbo::Vec2::ZERO);
    scroll(child).scroll_delta(move || wheel.get()).on_event(
        EventListener::PointerWheel,
        move |e| {
            if let Event::PointerWheel(pe) = e
                && pe.modifiers.shift()
            {
                // Windows sends Shift+wheel as a vertical delta; map it to x.
                let dx = if pe.delta.x != 0.0 {
                    pe.delta.x
                } else {
                    pe.delta.y
                };
                if dx != 0.0 {
                    wheel.set(floem::kurbo::Vec2::new(dx, 0.0));
                }
                return EventPropagation::Stop;
            }
            EventPropagation::Continue
        },
    )
}

/// Wrap a child in a horizontal `scroll` with **permanently hidden bars** that
/// pans on a *plain* (vertical) wheel — the tab strips, where there's no vertical
/// axis, so the main wheel should nudge tabs sideways and keep overflowed tabs
/// reachable. Both wheel axes map to x; the built-in scroll runs our listener
/// before its own wheel handling, so `Stop` suppresses any default scrolling.
pub(crate) fn wheel_hscroll<V: IntoView + 'static>(child: V) -> Scroll {
    let wheel: RwSignal<floem::kurbo::Vec2> = RwSignal::new(floem::kurbo::Vec2::ZERO);
    scroll(child)
        .scroll_style(|cs| cs.hide_bars(true))
        .scroll_delta(move || wheel.get())
        .on_event(EventListener::PointerWheel, move |e| {
            if let Event::PointerWheel(pe) = e {
                let dx = if pe.delta.x != 0.0 {
                    pe.delta.x
                } else {
                    pe.delta.y
                };
                if dx != 0.0 {
                    wheel.set(floem::kurbo::Vec2::new(dx, 0.0));
                    return EventPropagation::Stop;
                }
            }
            EventPropagation::Continue
        })
}

// ── Shared bits (section headers, centered messages, panel-toggle icon) ──
/// Font size for small toolbar controls (ER-diagram toolbar, header Retry).
pub(crate) const TOOLBAR_FONT: f32 = 13.0;

/// The chrome shared by small toolbar controls: bordered, rounded surface.
/// Callers add their own padding and hover — see `control_button` in the ERD
/// toolbar and the header's Retry.
pub(crate) fn control_surface(s: floem::style::Style) -> floem::style::Style {
    s.background(theme::control_bg())
        .border(1.0)
        .border_color(theme::control_border())
        .border_radius(CONTROL_RADIUS)
}

pub(crate) fn section_title(t: &'static str) -> impl IntoView {
    text(t).style(|s| {
        s.font_size(theme::FONT_TITLE)
            .font_bold()
            .color(theme::text_muted())
            .padding_horiz(12.0)
            .padding_vert(8.0)
    })
}

/// A centred status line filling its container (empty state, failure, cancel).
///
/// `color` is a **function**, not a `Color`: a colour read once at build freezes
/// at the theme that was active then, so every caller of this — eleven of them —
/// would keep painting the old palette after a live theme switch. Passing the
/// accessor and calling it *inside* the reactive `.style` closure is what makes
/// the switch free (docs/architecture.md → *Themable colors reach reactive styles as
/// `fn() -> Color`*).
pub(crate) fn centered_msg(
    msg: impl Into<String>,
    color: impl Fn() -> floem::peniko::Color + 'static,
) -> impl IntoView {
    let msg = msg.into();
    container(text(msg).style(move |s| s.color(color()))).style(|s| {
        s.flex_grow(1.0_f32)
            .width_full()
            .items_center()
            .justify_center()
            .padding(16.0)
    })
}

/// Whimsical "verb spinner" verbs (Claude Code's set, trimmed of the very long
/// ones so they fit a compact loader). One is picked at random each time a loader
/// mounts; `loading_dots` then animates the trailing dots.
pub(crate) const SPINNER_VERBS: &[&str] = &[
    "Accomplishing",
    "Actioning",
    "Actualizing",
    "Architecting",
    "Baking",
    "Beaming",
    "Beboppin'",
    "Befuddling",
    "Billowing",
    "Blanching",
    "Bloviating",
    "Boogieing",
    "Boondoggling",
    "Booping",
    "Bootstrapping",
    "Brewing",
    "Burrowing",
    "Calculating",
    "Canoodling",
    "Caramelizing",
    "Cascading",
    "Catapulting",
    "Cerebrating",
    "Channeling",
    "Channelling",
    "Choreographing",
    "Churning",
    "Clauding",
    "Coalescing",
    "Cogitating",
    "Combobulating",
    "Composing",
    "Computing",
    "Concocting",
    "Considering",
    "Contemplating",
    "Cooking",
    "Crafting",
    "Creating",
    "Crunching",
    "Crystallizing",
    "Cultivating",
    "Deciphering",
    "Deliberating",
    "Determining",
    "Dilly-dallying",
    "Doing",
    "Doodling",
    "Drizzling",
    "Ebbing",
    "Effecting",
    "Elucidating",
    "Embellishing",
    "Enchanting",
    "Envisioning",
    "Evaporating",
    "Fermenting",
    "Finagling",
    "Flambeing",
    "Flowing",
    "Flummoxing",
    "Fluttering",
    "Forging",
    "Forming",
    "Frolicking",
    "Frosting",
    "Gallivanting",
    "Galloping",
    "Garnishing",
    "Generating",
    "Germinating",
    "Gitifying",
    "Grooving",
    "Gusting",
    "Harmonizing",
    "Hashing",
    "Hatching",
    "Herding",
    "Honking",
    "Hullaballooing",
    "Hyperspacing",
    "Ideating",
    "Imagining",
    "Improvising",
    "Incubating",
    "Inferring",
    "Infusing",
    "Ionizing",
    "Jitterbugging",
    "Julienning",
    "Kneading",
    "Leavening",
    "Levitating",
    "Lollygagging",
    "Manifesting",
    "Marinating",
    "Meandering",
    "Metamorphosing",
    "Misting",
    "Moonwalking",
    "Moseying",
    "Mulling",
    "Mustering",
    "Musing",
    "Nebulizing",
    "Nesting",
    "Newspapering",
    "Noodling",
    "Nucleating",
    "Orbiting",
    "Orchestrating",
    "Osmosing",
    "Perambulating",
    "Percolating",
    "Perusing",
    "Philosophising",
    "Pollinating",
    "Pondering",
    "Pontificating",
    "Pouncing",
    "Precipitating",
    "Processing",
    "Proofing",
    "Propagating",
    "Puttering",
    "Puzzling",
    "Quantumizing",
    "Razzmatazzing",
    "Reticulating",
    "Roosting",
    "Ruminating",
    "Sauteing",
    "Scampering",
    "Schlepping",
    "Scurrying",
    "Seasoning",
    "Shenaniganing",
    "Shimmying",
    "Simmering",
    "Skedaddling",
    "Sketching",
    "Slithering",
    "Smooshing",
    "Sock-hopping",
    "Spelunking",
    "Spinning",
    "Sprouting",
    "Stewing",
    "Sublimating",
    "Swirling",
    "Swooping",
    "Symbioting",
    "Synthesizing",
    "Tempering",
    "Thinking",
    "Thundering",
    "Tinkering",
    "Tomfoolering",
    "Topsy-turvying",
    "Transfiguring",
    "Transmuting",
    "Twisting",
    "Undulating",
    "Unfurling",
    "Unravelling",
    "Vibing",
    "Waddling",
    "Wandering",
    "Warping",
    "Whirlpooling",
    "Whirring",
    "Whisking",
    "Wibbling",
    "Working",
    "Wrangling",
    "Zesting",
    "Zigzagging",
];

/// Pick a random spinner verb. Seeded off the wall clock (std-only) — good enough
/// for a cosmetic loader; a fresh verb each time a loader mounts.
pub(crate) fn pick_spinner_verb() -> &'static str {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as usize)
        .unwrap_or(0);
    SPINNER_VERBS[seed % SPINNER_VERBS.len()]
}

/// A "verb spinner" loader: a random whimsical verb + animated trailing dots
/// (`loading_dots`). Shared by the AI panel, the Ctrl+K inline AI, and the query
/// runner so they all read the same. The verb is fixed for the life of this
/// loader instance; the dots cycle.
pub(crate) fn verb_spinner(color: fn() -> floem::peniko::Color, font_size: f32) -> impl IntoView {
    loading_dots(pick_spinner_verb(), color, font_size)
}

/// Trailing debounce: returns a signal that mirrors `src` but only settles
/// `delay` after the last change. Bind the input widget to `src` (so typing stays
/// responsive) and let the expensive consumers read the returned signal, so a
/// large re-filter/re-layout fires once per burst, not per keystroke. The write is
/// `try_update`-guarded so a timer that outlives the owning scope (panel closed
/// mid-burst) is a no-op rather than a panic on a freed signal.
pub(crate) fn debounced(src: RwSignal<String>, delay: std::time::Duration) -> RwSignal<String> {
    let out = RwSignal::new(src.get_untracked());
    let generation = std::rc::Rc::new(std::cell::Cell::new(0u64));
    floem::reactive::create_effect(move |_| {
        let v = src.get(); // track every change
        let g = generation.get() + 1;
        generation.set(g);
        let generation = generation.clone();
        floem::action::exec_after(delay, move |_| {
            if generation.get() == g {
                let _ = out.try_update(|s| *s = v.clone());
            }
        });
    });
    out
}

/// A wrapping text view that tints every case-insensitive occurrence of `term`
/// with the search-match colour (bold), the rest in `base` — the same match rule
/// (`text_ops::find_matches`) and colour as the global-search palette, but built on
/// `rich_text` so the highlight survives line-wrapping (the palette's segment
/// h-stack can't wrap). `term` empty / no match → plain text. Colours are read
/// inside the layout closure so a live theme switch re-tints. Use `.style()` on the
/// returned view for layout (width, clip, max-height).
pub(crate) fn highlight_text(
    full: String,
    term: Option<String>,
    font_size: f32,
    base: impl Fn() -> floem::peniko::Color + 'static,
    bold: bool,
    line_height: f32,
) -> floem::views::RichText {
    highlight_text_in(
        "IBM Plex Sans",
        full,
        term,
        font_size,
        base,
        bold,
        line_height,
    )
}

/// [`highlight_text`] in the app's monospace face — for text that is *code*, and
/// still has to carry a search highlight (the history panel's SQL preview).
pub(crate) fn highlight_mono(
    full: String,
    term: Option<String>,
    font_size: f32,
    base: impl Fn() -> floem::peniko::Color + 'static,
    line_height: f32,
) -> floem::views::RichText {
    highlight_text_in(
        crate::consts::MONO_FAMILY,
        full,
        term,
        font_size,
        base,
        false,
        line_height,
    )
}

/// The shared body: the family is the only thing the two differ in, and a
/// `rich_text` builds its own `Attrs`, so it can't be set from the outside with
/// a `.style()` the way an ordinary label's font can.
fn highlight_text_in(
    family: &'static str,
    full: String,
    term: Option<String>,
    font_size: f32,
    base: impl Fn() -> floem::peniko::Color + 'static,
    bold: bool,
    line_height: f32,
) -> floem::views::RichText {
    use floem::text::{Attrs, AttrsList, FamilyOwned, LineHeightValue, TextLayout, Weight};
    let base_weight = if bold { Weight::BOLD } else { Weight::NORMAL };
    floem::views::rich_text(move || {
        let sans = [FamilyOwned::Name(family.to_string())];
        let lh = LineHeightValue::Normal(line_height);
        let base_attrs = Attrs::new()
            .family(&sans)
            .font_size(font_size)
            .color(base())
            .weight(base_weight)
            .line_height(lh);
        let mut list = AttrsList::new(base_attrs);
        if let Some(t) = term.as_deref().filter(|t| !t.is_empty()) {
            let hit = Attrs::new()
                .family(&sans)
                .font_size(font_size)
                .color(theme::match_highlight())
                .weight(Weight::BOLD)
                .line_height(lh);
            for &start in schemaic_core::text_ops::find_matches(&full, t).iter() {
                list.add_span(start..start + t.len(), hit);
            }
        }
        let mut layout = TextLayout::new();
        layout.set_text(&full, list);
        layout
    })
}

/// An animated loading label — `prefix` followed by a cycling `.` → `..` → `...`
/// on a 400ms timer (instead of a static `…`). The timer self-reschedules and
/// stops when the view's scope is disposed (`try_update` → `None`), so it can't
/// outlive a `dyn_container` rebuild (same pattern as the AI elapsed timer).
pub(crate) fn loading_dots(
    prefix: &'static str,
    color: fn() -> floem::peniko::Color,
    font_size: f32,
) -> impl IntoView {
    let step = RwSignal::new(1usize);
    // Reserve the full `prefix...` width up front so the label keeps a fixed size
    // as the dots cycle (1→2→3) — otherwise it reflows, jittering when centred (the
    // query runner) or shoving a neighbour (Ctrl+K's Cancel). +2px guards sub-pixel
    // rounding so the 3-dot state never exceeds the reserved box.
    let w = measure_text_px_at(&format!("{prefix}..."), font_size) + 2.0;
    fn tick(step: RwSignal<usize>) {
        floem::action::exec_after(std::time::Duration::from_millis(400), move |_| {
            if step
                .try_update(|n| *n = if *n >= 3 { 1 } else { *n + 1 })
                .is_some()
            {
                tick(step);
            }
        });
    }
    tick(step);
    dyn_container(
        move || step.get(),
        move |n| {
            text(format!("{prefix}{}", ".".repeat(n)))
                .style(move |s| s.color(color()).font_size(font_size).min_width(w))
                .into_any()
        },
    )
}

// A status-bar panel toggle rendered as a 16px icon: `chip_active` when its
// panel is open, `chip_idle` (brightening on hover) when closed.
pub(crate) fn toggle_icon(
    glyph: &'static str,
    active: impl Fn() -> bool + 'static,
    on_click: impl Fn() + 'static,
) -> floem::views::Container {
    toggle_icon_gated(glyph, || true, active, on_click)
}

/// [`toggle_icon`] whose panel isn't always available — dimmed to 30% and inert
/// while `enabled` is false, the same disabled face [`toolbar_icon`] wears.
///
/// For the Server Activity toggle on a SQLite connection: the panel behind it has
/// nothing to show for that engine, and a toggle that opens an explanation is a
/// worse answer than one that visibly isn't offered.
pub(crate) fn toggle_icon_gated(
    glyph: &'static str,
    enabled: impl Fn() -> bool + Copy + 'static,
    active: impl Fn() -> bool + 'static,
    on_click: impl Fn() + 'static,
) -> floem::views::Container {
    toggle_icon_view_gated(
        icons::icon(glyph, 16.0).style(|s| s.flex_shrink(0.0_f32)),
        enabled,
        active,
        on_click,
    )
}

/// Like [`toggle_icon`] but takes a pre-built icon view — for non-square glyphs
/// (e.g. the footer AI wordmark) that can't go through `icons::icon`'s square size.
pub(crate) fn toggle_icon_view(
    icon: impl IntoView + 'static,
    active: impl Fn() -> bool + 'static,
    on_click: impl Fn() + 'static,
) -> floem::views::Container {
    toggle_icon_view_gated(icon, || true, active, on_click)
}

/// The body of both — see [`toggle_icon_gated`] for what `enabled` is for.
pub(crate) fn toggle_icon_view_gated(
    icon: impl IntoView + 'static,
    enabled: impl Fn() -> bool + Copy + 'static,
    active: impl Fn() -> bool + 'static,
    on_click: impl Fn() + 'static,
) -> floem::views::Container {
    // Wrap the glyph in a container that carries the padding + click handler:
    // Floem hit-tests an `Svg` against its rendered content only (padding on the
    // svg grows layout but not the click target), whereas a container hit-tests its
    // whole padded box. The icon inherits the colour via `currentColor`, so the
    // active/hover tint set on the container reaches the svg.
    container(icon)
        .on_click_stop(move |_| {
            if enabled() {
                on_click()
            }
        })
        .style(move |s| {
            // No pointer cursor — the app uses the normal cursor everywhere.
            let s = s
                .items_center()
                .flex_shrink(0.0_f32)
                .padding_vert(3.0)
                .padding_horiz(5.0);
            if !enabled() {
                s.color(theme::chip_idle().multiply_alpha(0.3))
            } else if active() {
                s.color(theme::chip_active())
            } else {
                s.color(theme::chip_idle())
                    .hover(|s| s.color(theme::chip_active()))
            }
        })
}

/// A 22px jump-to-bottom circle (chevron-down) that fades in only while `show()`
/// is true and is inert (no pointer events) otherwise. Absolutely positioned
/// bottom-right (10px/10px) inside its parent stack. Shared by the AI panel and
/// the terminal. Fades via alpha (Floem has no opacity prop); the icon owns its
/// own colour + transition since an inherited colour won't animate a child svg.
pub(crate) fn jump_to_bottom_button(
    show: impl Fn() -> bool + Copy + 'static,
    on_click: impl Fn() + 'static,
) -> impl IntoView {
    let hovered = RwSignal::new(false);
    let anim = || Transition::ease_in_out(std::time::Duration::from_millis(150));
    let icon = icons::icon(icons::CHEVRON_DOWN, 16.0).style(move |s| {
        let color = if !show() {
            theme::jump_icon().multiply_alpha(0.0)
        } else if hovered.get() {
            theme::jump_icon_hover()
        } else {
            theme::jump_icon()
        };
        s.color(color).transition_color(anim())
    });
    container(icon)
        .on_click_stop(move |_| on_click())
        .on_event(EventListener::PointerEnter, move |_| {
            hovered.set(true);
            EventPropagation::Continue
        })
        .on_event(EventListener::PointerLeave, move |_| {
            hovered.set(false);
            EventPropagation::Continue
        })
        .style(move |s| {
            let bg = if show() {
                theme::bg_deepest()
            } else {
                theme::bg_deepest().multiply_alpha(0.0)
            };
            s.absolute()
                .inset_right(10.0)
                .inset_bottom(10.0)
                .width(22.0)
                .height(22.0)
                .border_radius(11.0)
                .items_center()
                .justify_center()
                .background(bg)
                .transition_background(anim())
        })
        .pointer_events(show)
}

/// What a modal's Escape / ✕ should do, given whether work it started is still
/// running.
///
/// This exists because two modals got it wrong the same way. The import modal
/// and the DDL preview each have **three** exits — a footer button, Escape, and
/// the title bar's ✕ — and in both, only the footer button knew that closing
/// mid-flight is not allowed. The other two called a bare `close`, so pressing
/// Escape during a bulk import hid a transaction that then ran to completion and
/// committed, reporting its outcome into signals whose only reader had just been
/// unmounted; and pressing Escape during a MySQL `ALTER` threw away the
/// "statement 3 of 5 failed, 2 already stuck" report, leaving a half-migrated
/// table and a stale schema tree with no indication anything had happened.
///
/// Both modals' footers carried a comment explaining why the guard was there.
/// Neither comment could reach the other two exits, which is the argument for a
/// named function over a repeated `if busy`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExitAction {
    /// Nothing in flight — close.
    Close,
    /// Work is running and *can* be stopped: stop it and stay open, so the
    /// outcome still has a reader.
    Cancel,
    /// Work is running and cannot be stopped: refuse. Closing would orphan the
    /// outcome, and there is nothing to cancel.
    Ignore,
}

/// The exit decision. See [`ExitAction`].
///
/// `cancellable` is what separates the two callers: the import modal holds a
/// cancellation token the load actually observes, while `run_ddl` is handed a
/// fresh token nothing holds — and on MySQL each DDL statement has already
/// committed anyway, so there is no meaningful "stop".
pub(crate) fn exit_action(busy: bool, cancellable: bool) -> ExitAction {
    match (busy, cancellable) {
        (false, _) => ExitAction::Close,
        (true, true) => ExitAction::Cancel,
        (true, false) => ExitAction::Ignore,
    }
}

/// Whether a modal's **destructive** action may launch: only when nothing of its
/// own is already in flight, and only when the plan isn't marked read-only.
///
/// [`exit_action`]'s counterpart, and it exists for the same reason: the app has
/// two of these actions — the DDL preview's Apply and Import — and they
/// *disagreed*. Apply asked `if p.read_only || d.applying.get_untracked()
/// { return; }`; Import set its busy flag and never read it, resting instead on
/// a comment claiming "one at a time by construction: its Import button is
/// disabled while one is in flight". That is true of the next update pass and
/// false within a single key dispatch, so one Space on Import spawned **two**
/// bulk loads of the same file: both validated clean, both opened a
/// transaction, both committed — a 10,000-row CSV landed 20,000 rows — and the
/// second launch overwrote the cancellation token, so the first load could no
/// longer be stopped and nothing on screen knew it existed.
///
/// A guard that has to be re-derived at each site is one that will be derived
/// differently, so it is one function with the launch inside the same
/// synchronous step that reads it. The disabled button stays: it is what *says*
/// the action is unavailable. This is what makes it so.
pub(crate) fn accept_launch(in_flight: bool, read_only: bool) -> bool {
    !in_flight && !read_only
}

#[cfg(test)]
mod menu_icon_tests {
    use super::*;

    /// Open wins over hover, and the three states are three different colours.
    /// The pointer is still on an icon the moment after it is clicked, so an
    /// order that let hover win would leave the open state invisible exactly
    /// when it is true.
    #[test]
    fn open_outranks_hover_and_every_state_is_distinct() {
        assert_eq!(menu_icon_color(true, false), theme::accent());
        assert_eq!(menu_icon_color(true, true), theme::accent());
        assert_eq!(menu_icon_color(false, true), theme::text());
        assert_eq!(menu_icon_color(false, false), theme::text_muted());
        assert_ne!(menu_icon_color(true, true), menu_icon_color(false, true));
        assert_ne!(menu_icon_color(false, true), menu_icon_color(false, false));
    }
}

#[cfg(test)]
mod measure_tests {
    use super::*;

    #[test]
    fn bold_measures_wider_than_regular() {
        // Measure against the *bundled* faces, as the app does. Without this the
        // global `FontSystem` falls back to whatever the host has installed, and
        // the assertion becomes a claim about that machine's fonts: it held on
        // Windows and failed on a bare Linux runner, where the fallback's bold
        // measured *narrower* than its regular.
        crate::fonts::load_fonts();
        // The whole reason `measure_text_px_bold_at` exists: the ER-diagram card
        // header is drawn `.font_bold()`, and sizing it from the regular
        // measurement made every card narrower than its own title. For a
        // schema-qualified name the gap exceeds `node_width`'s 6px slack, so the
        // name ellipsized nowhere near the max card width.
        for name in ["analytics.daily_revenue", "sales.line_items", "orders"] {
            let reg = measure_text_px_at(name, 13.0);
            let bold = measure_text_px_bold_at(name, 13.0);
            assert!(
                bold > reg,
                "{name}: bold {bold} should exceed regular {reg}"
            );
        }
        // A long qualified name drifts by more than the slack — the actual bug.
        let n = "analytics.daily_revenue";
        assert!(
            measure_text_px_bold_at(n, 13.0) - measure_text_px_at(n, 13.0) > 6.0,
            "the regression this guards needs the drift to exceed node_width's slack"
        );
    }

    #[test]
    fn a_char_count_does_not_predict_the_width() {
        // The Ctrl+K diff used to size its scroll extent as `chars().count()`
        // times a fixed advance. `char` is not the unit text is laid out in: a
        // combining mark is a `char` of its own and adds no advance at all, so
        // the width came out over the truth for any accented line. Ctrl+K sends
        // the editor buffer, so a literal in the user's own SQL reaches it.
        //
        // The mirror case — a full-width CJK glyph taking *two* advances for one
        // `char` — is what this test used to assert, and it can't be pinned here:
        // `MONO_FAMILY` carries no CJK coverage, so those glyphs resolve through
        // whatever the machine happens to have installed. It failed on the bare
        // Linux CI runner, which has nothing, while passing locally on a fallback
        // box that measures identically to an unrenderable private-use codepoint
        // — so it was asserting on the fallback, not on a glyph. The combining
        // mark is in the bundled font and therefore measures the same everywhere.
        crate::fonts::load_fonts();
        let advance = measure_mono_px_at("0", 14.0);
        let ascii = "abcdefgh"; // 8 chars, 8 advances
        let accented = "e\u{0301}"; // 2 chars, 1 advance

        // The baseline the old arithmetic assumed, and where it held.
        assert!((measure_mono_px_at(ascii, 14.0) - 8.0 * advance).abs() < 1.0);
        // …and where it didn't: two chars occupying a single advance.
        assert!(
            measure_mono_px_at(accented, 14.0) < 1.5 * advance,
            "a combining mark must not measure as an advance of its own"
        );
    }

    #[test]
    fn the_mono_measurement_is_not_the_proportional_one() {
        // Measuring monospace text with the default family is the same class of
        // error as measuring bold text at regular weight — silently narrow.
        crate::fonts::load_fonts();
        let s = "iiiiiiiiii"; // the letter proportional fonts make narrowest
        assert!(
            measure_mono_px_at(s, 14.0) > measure_text_px_at(s, 14.0),
            "monospace must not be measured with the proportional family"
        );
    }
}

#[cfg(test)]
mod exit_tests {
    use super::*;

    /// The property both [B7.2-L1-01] and [B2-L1-01] are about: while work the
    /// modal started is still running, **no exit closes it**. Two modals had
    /// three exits each, and in both only the footer button knew this.
    #[test]
    fn no_exit_closes_a_modal_while_its_work_is_running() {
        for cancellable in [true, false] {
            assert_ne!(
                exit_action(true, cancellable),
                ExitAction::Close,
                "cancellable={cancellable}"
            );
        }
    }

    #[test]
    fn idle_always_closes() {
        assert_eq!(exit_action(false, true), ExitAction::Close);
        assert_eq!(exit_action(false, false), ExitAction::Close);
    }

    /// The import modal: the load holds a cancellation token, so Escape means
    /// "stop the write and roll it back", not "hide it".
    #[test]
    fn busy_and_cancellable_cancels_rather_than_closing() {
        assert_eq!(exit_action(true, true), ExitAction::Cancel);
    }

    /// The DDL preview: `run_ddl` is handed a token nothing holds, and MySQL
    /// commits each statement implicitly, so there is nothing to cancel — the
    /// only honest answer is to refuse the exit and keep the modal that owns the
    /// outcome on screen.
    #[test]
    fn busy_and_uncancellable_refuses_the_exit() {
        assert_eq!(exit_action(true, false), ExitAction::Ignore);
    }

    /// The property the Critical was: a destructive action must not launch a
    /// second time while its first launch is still in flight. One Space on
    /// Import used to spawn two bulk loads of the same file — both committed —
    /// because the disabled button was the only guard and it takes effect on a
    /// later update pass, not within the key dispatch that fired twice.
    #[test]
    fn a_launch_in_flight_refuses_a_second_one() {
        assert!(!accept_launch(true, false));
        assert!(!accept_launch(true, true));
    }

    /// The other half, which only the DDL preview had: a plan the app has marked
    /// read-only never runs, in flight or not.
    #[test]
    fn a_read_only_plan_never_launches() {
        assert!(!accept_launch(false, true));
    }

    #[test]
    fn idle_and_writable_launches() {
        assert!(accept_launch(false, false));
    }
}

/// Placing the hoisted submenu. See [`submenu_insets`] for why this is a function
/// rather than four lines inside a style closure.
#[cfg(test)]
mod submenu_place_tests {
    use super::*;

    /// A row 170 wide, 30 tall, at (400, 200) — a menu comfortably mid-window.
    fn row() -> Rect {
        Rect::new(400.0, 200.0, 570.0, 230.0)
    }

    #[test]
    fn a_submenu_with_room_sits_flush_on_the_row_s_right_edge() {
        let (x, y) = submenu_insets(row(), (1400.0, 900.0), 120.0);
        assert_eq!(x, SubX::Left(570.0));
        assert_eq!(y, SubY::Top(194.0));
    }

    #[test]
    fn a_submenu_with_no_room_pins_its_right_edge_to_the_row_s_left_one() {
        // 570 + 210 > 700, so it flips. `Right(300)` in a 700-wide window puts the
        // panel's right edge at x=400 — the row's left edge, flush, whatever the
        // panel measures.
        let (x, _) = submenu_insets(row(), (700.0, 900.0), 120.0);
        assert_eq!(x, SubX::Right(300.0));
        let win_w = 700.0;
        let SubX::Right(inset) = x else {
            panic!("flipped")
        };
        assert_eq!(
            win_w - inset,
            row().x0,
            "right edge lands on the row's left"
        );
    }

    #[test]
    fn a_submenu_that_would_overhang_the_bottom_pins_to_it() {
        // 200 - 6 + 400 > 500.
        let (_, y) = submenu_insets(row(), (1400.0, 500.0), 400.0);
        assert_eq!(y, SubY::Bottom(0.0));
    }

    #[test]
    fn the_flip_is_by_a_hair_not_by_a_margin() {
        // Exactly enough room on the right is not a flip; one pixel less is.
        let win = (570.0 + SUBMENU_FLIP_W, 900.0);
        assert_eq!(submenu_insets(row(), win, 120.0).0, SubX::Left(570.0));
        assert_eq!(
            submenu_insets(row(), (win.0 - 1.0, win.1), 120.0).0,
            SubX::Right(win.0 - 1.0 - 400.0)
        );
    }

    #[test]
    fn an_unmeasured_window_never_flips() {
        // `window_size` starts at (0, 0) and is set from the root's `on_resize`. A
        // submenu opened before that must not be flung to an edge that isn't there.
        let (x, y) = submenu_insets(row(), (0.0, 0.0), 120.0);
        assert_eq!(x, SubX::Left(570.0));
        assert_eq!(y, SubY::Top(194.0));
    }

    #[test]
    fn a_row_at_the_window_origin_still_places_forward() {
        let (x, y) = submenu_insets(Rect::new(0.0, 0.0, 170.0, 30.0), (1400.0, 900.0), 120.0);
        assert_eq!(x, SubX::Left(170.0));
        // The lift may take the top negative at the very top of the window; that is
        // the same 6px the un-hoisted version applied, and clamping it would
        // misalign the first item against its row.
        assert_eq!(y, SubY::Top(-6.0));
    }
}

/// Driving an open menu from the keyboard. `menu_key` is the whole decision and
/// takes only signals, so the arrow/Enter behaviour asserts without a window —
/// which is the point, because every way of getting it wrong is silent: a cursor
/// that can rest on a separator makes Down look dead, and one that can rest on a
/// disabled row offers an Enter that does nothing.
#[cfg(test)]
mod menu_key_tests {
    use super::*;
    use std::cell::Cell;

    /// `[Action, Separator, Action(disabled), Sub[Action, Action]]` — one of each
    /// thing the cursor has to treat differently, in one menu.
    fn entries(hits: Rc<Cell<u32>>) -> Vec<MenuEntry> {
        let a = hits.clone();
        let b = hits.clone();
        vec![
            MenuEntry::action("first", move || a.set(a.get() + 1)),
            MenuEntry::Separator,
            MenuEntry::action("inert", || {}).disabled(true),
            MenuEntry::sub(
                "more",
                vec![
                    MenuEntry::action("child", move || b.set(b.get() + 100)),
                    MenuEntry::action("other", || {}),
                ],
            ),
        ]
    }

    struct Menu {
        level: MenuLevel,
        stops: Vec<(usize, MenuAct)>,
        subs: std::collections::HashMap<usize, Vec<(usize, MenuAct)>>,
        close: Rc<dyn Fn()>,
        hits: Rc<Cell<u32>>,
        closed: Rc<Cell<u32>>,
    }

    fn menu() -> Menu {
        let hits = Rc::new(Cell::new(0));
        let closed = Rc::new(Cell::new(0));
        let es = entries(hits.clone());
        let subs = es
            .iter()
            .enumerate()
            .filter_map(|(i, e)| match e {
                MenuEntry::Sub { children, .. } => Some((i, menu_stops(children))),
                _ => None,
            })
            .collect();
        let c = closed.clone();
        Menu {
            level: MenuLevel::new(),
            stops: menu_stops(&es),
            subs,
            close: Rc::new(move || c.set(c.get() + 1)),
            hits,
            closed,
        }
    }

    impl Menu {
        /// `true` when the menu **claimed** the key (`EventPropagation::Stop`),
        /// which is as much of the return value as any caller cares about —
        /// floem's enum is neither `PartialEq` nor `Debug`.
        fn press(&self, k: NamedKey) -> bool {
            matches!(
                menu_key(k, self.level, &self.stops, &self.subs, &self.close),
                EventPropagation::Stop
            )
        }
        fn cursor(&self) -> Option<usize> {
            self.level.cursor.get_untracked()
        }
        fn sub_cursor(&self) -> Option<usize> {
            self.level.sub.cursor.get_untracked()
        }
    }

    /// **A separator and a disabled row are not stops.** The indices kept are the
    /// *entry* indices, so a row view can still ask whether it is the cursor.
    #[test]
    fn only_the_rows_a_cursor_may_rest_on_are_stops() {
        let stops = menu_stops(&entries(Rc::new(Cell::new(0))));
        assert_eq!(
            stops.iter().map(|(i, _)| *i).collect::<Vec<_>>(),
            vec![0, 3],
            "the separator at 1 and the disabled row at 2 are skipped"
        );
        assert!(matches!(stops[0].1, MenuAct::Run(_)));
        assert!(matches!(stops[1].1, MenuAct::Open));
    }

    /// The first Down on a freshly-opened menu lands on the first entry, and the
    /// first Up on the last — the ring's answer for a cursor that is nowhere yet.
    #[test]
    fn the_first_arrow_enters_from_the_matching_end() {
        let m = menu();
        m.press(NamedKey::ArrowDown);
        assert_eq!(m.cursor(), Some(0));

        let m = menu();
        m.press(NamedKey::ArrowUp);
        assert_eq!(m.cursor(), Some(3));
    }

    /// Down walks stop to stop — **over** the separator and the disabled row, not
    /// onto them — and wraps, a menu being short enough that its ends read as
    /// adjacent.
    #[test]
    fn down_skips_the_gaps_and_wraps() {
        let m = menu();
        m.press(NamedKey::ArrowDown);
        m.press(NamedKey::ArrowDown);
        assert_eq!(m.cursor(), Some(3), "1 and 2 are not stops");
        m.press(NamedKey::ArrowDown);
        assert_eq!(m.cursor(), Some(0), "wrapped");
    }

    #[test]
    fn home_and_end_jump_to_the_outer_stops() {
        let m = menu();
        m.press(NamedKey::End);
        assert_eq!(m.cursor(), Some(3));
        m.press(NamedKey::Home);
        assert_eq!(m.cursor(), Some(0));
    }

    /// Enter on an action is a click: run it, then close.
    #[test]
    fn enter_runs_the_cursor_row_and_closes() {
        let m = menu();
        m.press(NamedKey::ArrowDown);
        assert!(m.press(NamedKey::Enter));
        assert_eq!(m.hits.get(), 1);
        assert_eq!(m.closed.get(), 1);
    }

    /// **Enter on a submenu row must not close the menu.** The row has no action
    /// of its own, so treating it like one would be a keypress that threw the
    /// menu away and did nothing — it opens instead, exactly as Right does, and
    /// arrives with the cursor on the first child.
    #[test]
    fn enter_on_a_submenu_opens_it_rather_than_closing() {
        for key in [NamedKey::Enter, NamedKey::ArrowRight] {
            let m = menu();
            m.press(NamedKey::End);
            m.press(key);
            assert_eq!(m.closed.get(), 0, "{key:?} must not close the menu");
            assert_eq!(m.level.open_sub.get_untracked(), Some(3));
            assert_eq!(m.sub_cursor(), Some(0), "{key:?} lands on the first child");
        }
    }

    /// With a submenu open the arrows drive **it**, and the parent's cursor stays
    /// where it was — it is still on the row holding the submenu open.
    #[test]
    fn the_arrows_follow_the_hoisted_submenu() {
        let m = menu();
        m.press(NamedKey::End);
        m.press(NamedKey::ArrowRight);
        m.press(NamedKey::ArrowDown);
        assert_eq!(m.sub_cursor(), Some(1));
        assert_eq!(m.cursor(), Some(3), "the parent row keeps the cursor");
        assert!(m.press(NamedKey::Enter));
        assert_eq!(m.hits.get(), 0, "the child's action ran, not the parent's");
        assert_eq!(m.closed.get(), 1);
    }

    /// Left comes back out one level, and leaves the parent's cursor on the row
    /// that opened it — so Down from there is the *next* row, not a re-entry.
    #[test]
    fn left_leaves_the_submenu_without_closing_the_menu() {
        let m = menu();
        m.press(NamedKey::End);
        m.press(NamedKey::ArrowRight);
        assert!(m.press(NamedKey::ArrowLeft));
        assert_eq!(m.level.open_sub.get_untracked(), None);
        assert_eq!(m.sub_cursor(), None);
        assert_eq!(m.cursor(), Some(3));
        assert_eq!(m.closed.get(), 0);
    }

    /// **The return slot is consumed, not merely read.** It lives only between
    /// the opener and the panel that takes it, which is what stops a menu opened
    /// later — from a right-click somewhere else entirely — inheriting a return
    /// to a control the user has long since left.
    #[test]
    fn a_menu_return_is_taken_once_and_only_once() {
        let fired = Rc::new(Cell::new(0));
        let f = fired.clone();
        set_menu_return(Rc::new(move || f.set(f.get() + 1)));

        let taken = take_menu_return();
        assert!(taken.is_some(), "the panel that builds first gets it");
        assert!(
            take_menu_return().is_none(),
            "and the next one gets nothing"
        );

        (taken.unwrap())();
        assert_eq!(fired.get(), 1);
    }

    // The grid toolbar's Copy icon, and Save's a few px to its right.
    const COPY_AT: crate::PopupAnchor = crate::PopupAnchor::BelowIcon(100.0, 116.0, 40.0);
    const SAVE_AT: crate::PopupAnchor = crate::PopupAnchor::BelowIcon(126.0, 142.0, 40.0);

    #[test]
    fn a_trigger_owns_the_menu_anchored_at_its_own_place() {
        assert!(menu_anchored_at(true, Some(COPY_AT), COPY_AT));
    }

    /// The whole point of checking `open` first. Closing clears `popup_menu` but
    /// leaves `popup_anchor` naming whoever opened last, so an anchor-only test
    /// would report the Copy menu still open after Escape dismissed it — and the
    /// next press would close nothing instead of opening.
    #[test]
    fn a_dismissed_menu_is_no_longer_owned() {
        assert!(!menu_anchored_at(false, Some(COPY_AT), COPY_AT));
    }

    /// Adjacent triggers: whichever menu is up, only its own may close it.
    /// Pressing Save while Copy's menu is open opens Save's, which is what the
    /// opener does when this returns false.
    #[test]
    fn a_neighbours_menu_is_not_mine() {
        assert!(!menu_anchored_at(true, Some(COPY_AT), SAVE_AT));
    }

    /// A cell or header right-click sets the anchor to `None` (open at the
    /// cursor) on this same channel, and a press on Copy must open Copy's menu
    /// rather than dismiss that one.
    #[test]
    fn a_cursor_menu_belongs_to_no_trigger() {
        assert!(!menu_anchored_at(true, None, COPY_AT));
    }

    /// The two placements share the channel, so the variants must not compare
    /// equal on their numbers — a status-bar segment sitting at the same
    /// coordinates as a toolbar icon is not that icon's menu.
    #[test]
    fn a_footer_menu_is_never_a_toolbar_dropdown() {
        assert!(!menu_anchored_at(
            true,
            Some(crate::PopupAnchor::AboveFooter(100.0, 116.0)),
            COPY_AT
        ));
    }

    /// Two status-bar segments are told apart by their own x-range, the same way
    /// two toolbar icons are told apart by theirs. This is the case the
    /// `menu_owner` tag used to answer.
    #[test]
    fn two_footer_segments_are_told_apart() {
        let tabs = crate::PopupAnchor::AboveFooter(10.0, 60.0);
        let model = crate::PopupAnchor::AboveFooter(70.0, 120.0);
        assert!(menu_anchored_at(true, Some(tabs), tabs));
        assert!(!menu_anchored_at(true, Some(tabs), model));
    }

    /// Left at the root is not the menu's key — nothing is open to leave, so it
    /// passes through rather than being swallowed.
    #[test]
    fn a_key_the_menu_has_no_use_for_passes_through() {
        let m = menu();
        assert!(!m.press(NamedKey::ArrowLeft));
        assert!(!m.press(NamedKey::Tab));
        // Enter with the cursor nowhere: there is no row to run.
        assert!(!m.press(NamedKey::Enter));
        assert_eq!(m.closed.get(), 0);
    }
}

#[cfg(test)]
mod menu_placement_tests {
    use super::*;

    const PANEL: (f64, f64) = (170.0, 350.0);
    const WINDOW: (f64, f64) = (1200.0, 800.0);

    #[test]
    fn a_menu_with_room_opens_down_and_right_of_the_cursor() {
        assert_eq!(
            cursor_menu_pos((100.0, 100.0), PANEL, WINDOW, 3.0),
            (103.0, 103.0)
        );
    }

    /// The schema tree is a full-height left column, so its lower half is where
    /// most right-clicks land — and a table's menu is a dozen entries.
    #[test]
    fn a_menu_near_the_bottom_flips_above_the_cursor() {
        let (x, y) = cursor_menu_pos((100.0, 700.0), PANEL, WINDOW, 3.0);
        assert_eq!(x, 103.0, "horizontal is unaffected");
        assert_eq!(y, 347.0);
        assert!(y + PANEL.1 <= 700.0, "the panel ends above the cursor");
    }

    #[test]
    fn a_menu_near_the_right_edge_flips_left_of_the_cursor() {
        let (x, _) = cursor_menu_pos((1150.0, 100.0), PANEL, WINDOW, 3.0);
        assert_eq!(x, 977.0);
        assert!(x + PANEL.0 <= 1150.0);
    }

    #[test]
    fn a_menu_in_the_far_corner_flips_both_ways() {
        let (x, y) = cursor_menu_pos((1150.0, 700.0), PANEL, WINDOW, 3.0);
        assert_eq!((x, y), (977.0, 347.0));
    }

    /// A panel taller (or wider) than the space on either side clamps to the
    /// window edge rather than going negative, where it would be unreachable.
    #[test]
    fn a_panel_bigger_than_the_window_clamps_to_the_origin() {
        let (x, y) = cursor_menu_pos((50.0, 60.0), (400.0, 900.0), WINDOW, 3.0);
        assert_eq!((x, y), (53.0, 0.0));
    }

    /// Before the root has measured itself the window is (0, 0). Flipping against
    /// an unknown edge would put every menu in the top-left corner.
    #[test]
    fn an_unmeasured_window_never_flips() {
        assert_eq!(
            cursor_menu_pos((900.0, 700.0), PANEL, (0.0, 0.0), 3.0),
            (903.0, 703.0)
        );
    }

    #[test]
    fn separators_are_not_counted_as_rows() {
        let sep = menu_panel_height(&[MenuEntry::Separator]);
        let row = menu_panel_height(&[MenuEntry::action("x", || {})]);
        assert!(
            sep < row,
            "a separator is a rule, not a row: {sep} vs {row}"
        );
        // Chrome is counted once, not per entry.
        let two =
            menu_panel_height(&[MenuEntry::action("a", || {}), MenuEntry::action("b", || {})]);
        assert!((two - (row * 2.0 - 14.0)).abs() < 0.001);
    }

    /// A submenu row is one row in the panel that hosts it — its children are
    /// drawn in their own panel and must not be added to this one's height.
    #[test]
    fn a_submenu_counts_as_a_single_row() {
        let sub = MenuEntry::Sub {
            label: "Copy".into(),
            icon: None,
            children: vec![
                MenuEntry::action("CSV", || {}),
                MenuEntry::action("JSON", || {}),
            ],
        };
        assert_eq!(
            menu_panel_height(std::slice::from_ref(&sub)),
            menu_panel_height(&[MenuEntry::action("Copy", || {})])
        );
    }
}

/// Separators are pushed by a menu builder *before* it knows whether the group
/// they open has any entries — the schema tree's column menu pushes one and then
/// asks `field_entries` whether Edit column and Drop are offered at all, and on a
/// view's column neither is. What shipped was a rule with nothing under it: an
/// empty section between "Copy qualified name" and AI Explain. Every conditional
/// group in that tree can reach the same shape, so the tidying belongs where rows
/// become a panel rather than in each arm that might need it.
#[cfg(test)]
mod menu_separator_tests {
    use super::*;

    /// Labels of the kept entries, with `"—"` standing for a separator, so a
    /// whole menu shape reads as one line.
    fn shape(entries: Vec<MenuEntry>) -> Vec<String> {
        tidy_separators(entries)
            .iter()
            .map(|e| match e {
                MenuEntry::Separator => "—".to_string(),
                MenuEntry::Action { label, .. } | MenuEntry::Sub { label, .. } => label.clone(),
            })
            .collect()
    }

    fn act(label: &str) -> MenuEntry {
        MenuEntry::action(label, || {})
    }

    /// The reported bug: a view's column menu, with both write entries absent.
    #[test]
    fn a_group_whose_entries_were_all_withheld_leaves_no_empty_section() {
        assert_eq!(
            shape(vec![
                act("Copy name"),
                act("Copy qualified name"),
                MenuEntry::Separator, // the write group — nothing was offered
                MenuEntry::Separator, // AI Explain's group
                act("AI Explain"),
            ]),
            vec!["Copy name", "Copy qualified name", "—", "AI Explain"]
        );
    }

    #[test]
    fn a_group_with_entries_keeps_its_separator() {
        assert_eq!(
            shape(vec![
                act("Copy name"),
                MenuEntry::Separator,
                act("Edit column"),
                MenuEntry::Separator,
                act("AI Explain"),
            ]),
            vec!["Copy name", "—", "Edit column", "—", "AI Explain"]
        );
    }

    /// A leading separator is a rule above the first row, and a trailing one a
    /// rule under the last — both are visible, and neither divides anything.
    #[test]
    fn separators_at_either_end_are_dropped() {
        assert_eq!(
            shape(vec![
                MenuEntry::Separator,
                act("Open"),
                MenuEntry::Separator
            ]),
            vec!["Open"]
        );
    }

    #[test]
    fn a_menu_of_nothing_but_separators_renders_nothing() {
        assert!(shape(vec![MenuEntry::Separator, MenuEntry::Separator]).is_empty());
    }

    /// Three withheld groups in a row collapse to one rule, not three.
    #[test]
    fn a_long_run_collapses_to_a_single_rule() {
        assert_eq!(
            shape(vec![
                act("a"),
                MenuEntry::Separator,
                MenuEntry::Separator,
                MenuEntry::Separator,
                act("b"),
            ]),
            vec!["a", "—", "b"]
        );
    }

    /// A submenu is built the same conditional way (the object menu's `Create`
    /// children vary by engine), and is its own panel.
    #[test]
    fn a_submenus_own_separators_are_tidied_too() {
        let tidied = tidy_separators(vec![MenuEntry::Sub {
            label: "Create".into(),
            icon: None,
            children: vec![MenuEntry::Separator, act("Table"), MenuEntry::Separator],
        }]);
        let MenuEntry::Sub { children, .. } = &tidied[0] else {
            panic!("the submenu row itself survived");
        };
        assert_eq!(children.len(), 1);
    }

    /// The panel is placed by its measured height, so measuring must count the
    /// rows that will actually be drawn — otherwise a menu with a withheld group
    /// flips at the window edge as though it were a separator taller.
    #[test]
    fn the_measured_height_ignores_a_separator_that_wont_be_drawn() {
        let untidy = vec![
            act("Copy name"),
            MenuEntry::Separator,
            MenuEntry::Separator,
            act("AI Explain"),
        ];
        assert_eq!(
            menu_panel_height(&untidy),
            menu_panel_height(&tidy_separators(untidy.clone()))
        );
    }
}

#[cfg(test)]
mod ring_tests {
    use super::*;

    #[test]
    fn tab_walks_forward_and_wraps_at_the_end() {
        assert_eq!(ring_step(3, Some(0), false), Some(1));
        assert_eq!(ring_step(3, Some(1), false), Some(2));
        // Wraps rather than escaping the modal.
        assert_eq!(ring_step(3, Some(2), false), Some(0));
    }

    #[test]
    fn shift_tab_walks_backward_and_wraps_at_the_start() {
        assert_eq!(ring_step(3, Some(2), true), Some(1));
        assert_eq!(ring_step(3, Some(1), true), Some(0));
        assert_eq!(ring_step(3, Some(0), true), Some(2));
    }

    /// Focus sat on something unregistered (a click on the modal's chrome), so
    /// the walk starts at whichever end the key implies rather than nowhere.
    #[test]
    fn a_tab_from_outside_the_ring_enters_at_the_near_end() {
        assert_eq!(ring_step(3, None, false), Some(0));
        assert_eq!(ring_step(3, None, true), Some(2));
    }

    #[test]
    fn an_empty_ring_has_nowhere_to_go() {
        assert_eq!(ring_step(0, None, false), None);
        assert_eq!(ring_step(0, Some(0), true), None);
    }

    #[test]
    fn a_single_control_keeps_focus_on_itself() {
        assert_eq!(ring_step(1, Some(0), false), Some(0));
        assert_eq!(ring_step(1, Some(0), true), Some(0));
    }

    // ── Selection inside a list (`list_pane`) ───────────────────────────────

    #[test]
    fn a_selection_walks_one_item_at_a_time() {
        assert_eq!(list_step(3, 0, 1), Some(1));
        assert_eq!(list_step(3, 2, -1), Some(1));
    }

    /// The documented divergence from [`ring_step`]: the Tab ring wraps so Tab
    /// can't leave the modal; a *selection* clamps, because jumping from the
    /// last column to the first is only a surprise.
    #[test]
    fn a_selection_clamps_at_both_ends_where_the_tab_ring_wraps() {
        assert_eq!(list_step(3, 2, 1), None, "clamped, not wrapped");
        assert_eq!(list_step(3, 0, -1), None);
        assert_eq!(ring_step(3, Some(2), false), Some(0), "the ring wraps");
        assert_eq!(ring_step(3, Some(0), true), Some(2));
    }

    /// A table with no CHECK constraints reaches this, and the clamp is where it
    /// would bite: `clamp(0, -1)` panics in debug.
    #[test]
    fn an_empty_list_has_nowhere_to_step() {
        assert_eq!(list_step(0, 0, 1), None);
        assert_eq!(list_step(0, 0, -1), None);
    }

    #[test]
    fn a_single_item_list_never_moves() {
        assert_eq!(list_step(1, 0, 1), None);
        assert_eq!(list_step(1, 0, -1), None);
    }

    // ── The ring itself ─────────────────────────────────────────────────────
    //
    // `ViewId::new()` is public and `request_focus` only queues an update
    // message, so the whole ordering rule tests without an app — which is worth
    // having, because every way of getting it wrong is silent: flip
    // `partition_point`'s `<=` to `<` and controls sharing a tabindex reverse;
    // drop `register`'s `retain` and `edit_field`'s re-registering effect
    // duplicates its field, so Tab visits the same box twice.

    fn ring_of(spec: &[u32]) -> (FocusRing, Vec<floem::ViewId>) {
        let ring = FocusRing::new();
        let ids: Vec<_> = spec
            .iter()
            .map(|t| {
                let id = floem::ViewId::new();
                ring.register(*t, id);
                id
            })
            .collect();
        (ring, ids)
    }

    #[test]
    fn the_ring_orders_by_tabindex_not_registration() {
        // The SSH block registers only once its toggle is on, i.e. after the
        // fields below it on screen.
        let (ring, ids) = ring_of(&[30, 10, 20]);
        assert_eq!(ring.ids(), vec![ids[1], ids[2], ids[0]]);
    }

    // ── the app's `:focus-visible` ──────────────────────────────────────────
    //
    // `keyboard_nav` is a `thread_local`, so each test *thread* has its own —
    // but cargo runs several tests per thread, so these set it explicitly rather
    // than assuming a starting value. A leftover `true` from a neighbour that
    // happened to land on the same thread would make a green run mean nothing.

    /// **Stepping the ring is the whole "set" half.** Asserted through
    /// `step_from` rather than by calling the setter, because the claim is that
    /// no keyboard-driven focus change can bypass it — a key listener added
    /// somewhere else would have to be found by eye.
    #[test]
    fn a_tab_through_the_ring_arms_the_focus_ring() {
        let (ring, ids) = ring_of(&[10, 20]);
        keyboard_nav().set(false);
        ring.step_from(ids[0], false);
        assert!(keyboard_nav().get_untracked());
    }

    /// Shift+Tab is the same gesture backwards, and a step that finds nowhere to
    /// go is still a keypress — the flag tracks the *gesture*, not whether focus
    /// actually moved.
    #[test]
    fn stepping_backwards_arms_it_too() {
        let (ring, ids) = ring_of(&[10, 20]);
        keyboard_nav().set(false);
        ring.step_from(ids[1], true);
        assert!(keyboard_nav().get_untracked());
    }

    /// **Handing the keyboard back is not a keyboard gesture.** A dropdown
    /// returning focus once its popup closes, and a field unmounting under the
    /// user, both move focus on behalf of something they may have reached with
    /// the mouse — so these leave the flag exactly as the last real gesture set
    /// it, in both directions.
    #[test]
    fn refocusing_by_tabindex_leaves_the_flag_alone() {
        let (ring, _) = ring_of(&[10, 20]);
        keyboard_nav().set(false);
        ring.focus_at(10);
        assert!(
            !keyboard_nav().get_untracked(),
            "a mouse-driven hand-back must not light the ring"
        );
        keyboard_nav().set(true);
        ring.focus_at(20);
        assert!(
            keyboard_nav().get_untracked(),
            "nor must it put out a ring the keyboard earned"
        );
    }

    #[test]
    fn re_registering_a_view_moves_it_rather_than_duplicating_it() {
        let (ring, ids) = ring_of(&[10, 20]);
        ring.register(30, ids[0]);
        assert_eq!(ring.ids(), vec![ids[1], ids[0]]);
    }

    /// A `dyn_container` swap builds the newcomer *before* removing the outgoing
    /// view, so the two share a tabindex for one update pass. The survivor has
    /// to keep the slot in both directions.
    #[test]
    fn a_tie_is_broken_in_favour_of_the_survivor() {
        let (ring, ids) = ring_of(&[10, 20, 30]);
        let incoming = floem::ViewId::new();
        ring.register(20, incoming);
        assert_eq!(ring.ids(), vec![ids[0], ids[1], incoming, ids[2]]);
        ring.unregister(ids[1]);
        assert_eq!(ring.ids(), vec![ids[0], incoming, ids[2]]);
    }

    #[test]
    fn a_tab_from_a_member_steps_to_its_neighbour_and_wraps() {
        let (ring, ids) = ring_of(&[10, 20, 30]);
        assert_eq!(ring.target(ids[0], false), Some(ids[1]));
        assert_eq!(ring.target(ids[2], false), Some(ids[0]));
        assert_eq!(ring.target(ids[0], true), Some(ids[2]));
    }

    /// The reason `remember` exists: leaving a `tab_indents` field (where Tab
    /// types an indent and Escape is the only way out) hands focus to the modal
    /// root, and re-entering at position 0 made every control after the field
    /// unreachable by forward Tab.
    #[test]
    fn re_entering_from_outside_resumes_where_the_ring_last_was() {
        let (ring, ids) = ring_of(&[10, 20, 30]);
        let root = floem::ViewId::new();
        assert_eq!(
            ring.target(root, false),
            Some(ids[0]),
            "nowhere yet: the top"
        );
        ring.remember(ids[1]);
        assert_eq!(ring.target(root, false), Some(ids[2]));
        assert_eq!(ring.target(root, true), Some(ids[0]));
    }

    /// **A control removed by its own action resumes at its neighbour**, not at
    /// the top of the modal. Tab to an enum value's ✕ and press Space: the row
    /// goes, the focused view with it, and the next Tab used to restart at the
    /// modal's first control — because `last` was a raw `ViewId` and `target`
    /// resolves it by looking it up in the ring it has just left.
    #[test]
    fn a_remembered_control_that_has_since_unmounted_resumes_at_its_neighbour() {
        let (ring, ids) = ring_of(&[10, 20, 30]);
        ring.remember(ids[1]);
        ring.unregister(ids[1]);
        assert_eq!(ring.target(floem::ViewId::new(), false), Some(ids[2]));
    }

    /// The same, at the front of the ring: the removed control's neighbour is
    /// the first entry, and a forward walk starts there anyway.
    #[test]
    fn removing_the_first_control_resumes_at_the_new_first() {
        let (ring, ids) = ring_of(&[10, 20, 30]);
        ring.remember(ids[0]);
        ring.unregister(ids[0]);
        assert_eq!(ring.target(floem::ViewId::new(), false), Some(ids[1]));
    }

    /// A control that is still there is stepped *past*, which is the ordinary
    /// Escape-then-Tab case and the one the memory was added for.
    #[test]
    fn a_remembered_control_still_mounted_is_stepped_past() {
        let (ring, ids) = ring_of(&[10, 20, 30]);
        ring.remember(ids[1]);
        assert_eq!(ring.target(floem::ViewId::new(), false), Some(ids[2]));
        assert_eq!(ring.target(floem::ViewId::new(), true), Some(ids[0]));
    }

    /// And a control **rebuilt** under the same tabindex — a dropdown's accept
    /// replaces the box — is found again, where a captured id would be stale.
    #[test]
    fn a_remembered_control_rebuilt_under_its_tabindex_is_still_found() {
        let (ring, ids) = ring_of(&[10, 20, 30]);
        ring.remember(ids[1]);
        let rebuilt = floem::ViewId::new();
        ring.register(20, rebuilt);
        ring.unregister(ids[1]);
        assert_eq!(ring.target(floem::ViewId::new(), false), Some(ids[2]));
    }

    /// **A disabled button is not a Tab stop.** It keeps its place on screen —
    /// which action is the affirmative one shouldn't move as a form becomes
    /// valid — but the keyboard walks past it, the same answer the pointer
    /// already gives, since its click handler is inert too. Landing on Preview
    /// SQL and pressing Space to nothing is worse than not landing there.
    #[test]
    fn a_disabled_button_is_not_a_tab_stop() {
        let ring = FocusRing::new();
        let live = in_ring_button(
            empty(),
            ring.clone(),
            ACTION_TAB,
            true,
            ACTION_RADIUS,
            || {},
        );
        let dead = in_ring_button(
            empty(),
            ring.clone(),
            ACTION_TAB + 10,
            false,
            ACTION_RADIUS,
            || {},
        );
        assert_eq!(ring.ids(), vec![live.id()], "only the live one registered");
        assert_ne!(dead.id(), live.id());
        assert_eq!(ring.at(ACTION_TAB + 10), None);
    }

    #[test]
    #[should_panic(expected = "must stay last")]
    fn a_tabindex_past_the_title_close_is_refused() {
        FocusRing::new().register(TITLE_CLOSE_TAB + 1, floem::ViewId::new());
    }

    /// **`FocusVisible` is applied after `Focus`, so the narrower of the two
    /// wins.** Floem gates the second on `app_state.keyboard_navigation`, which
    /// latches globally the first time its own Tab traversal runs anywhere in the
    /// window — one Tab in the workspace does it, and nothing but a pointer press
    /// resets it. A ring member that sets only `.focus` and suppresses
    /// `.focus_visible` (which every control here must, to answer floem's own 3px
    /// magenta default) therefore shows **no focus indication at all** from that
    /// moment on. That is what happened to the Settings switch.
    ///
    /// Asserted over both ring builders, as the composed `Style` floem would
    /// resolve: the outline under `[Focus, FocusVisible]` may never be narrower
    /// than the one under `[Focus]`.
    #[test]
    fn a_rings_focus_visible_outline_is_never_narrower_than_its_focus_one() {
        use floem::style::{Outline, Style, StyleSelector};
        // The ring is gated on the app's own flag, so the question only exists
        // when it is set.
        keyboard_nav().set(true);
        let width = |s: Style, sel: &[StyleSelector]| {
            let px = s.apply_selectors(sel).get(Outline);
            // `PxPct` — a ring is always in px.
            format!("{px:?}")
        };
        for (what, s) in [
            (
                "button_focus_ring",
                button_focus_ring(Style::new(), ACTION_RADIUS),
            ),
            (
                "themed_toggle",
                crate::settings::toggle_focus_ring(
                    Style::new().focus_visible(|s| s.outline(0.0)),
                    theme::accent(),
                ),
            ),
        ] {
            let focus = width(s.clone(), &[StyleSelector::Focus]);
            let visible = width(s, &[StyleSelector::Focus, StyleSelector::FocusVisible]);
            assert_eq!(
                focus, visible,
                "{what}: the ring must survive floem's FocusVisible pass"
            );
            assert_ne!(
                focus,
                format!("{:?}", floem::unit::PxPct::Px(0.0)),
                "{what}"
            );
        }
    }

    /// **The half that was missing: with no overlay open, there was nowhere to
    /// hand the keyboard to.** A control removed while focused — the results
    /// toolbar's ✓/✗ pressed from the keyboard removes itself — left
    /// `app_state.focus` at `None` and the grid answering no key at all, including
    /// the `F6` that would have got back into the strip.
    ///
    /// The ring half is asserted beside it: the tabindex has to survive the
    /// control's removal, or the next Tab restarts at the top of the strip instead
    /// of resuming beside where the control was.
    #[test]
    fn handing_the_keyboard_back_falls_back_to_the_workspaces_home() {
        let ring = FocusRing::new();
        let leaving = floem::ViewId::new();
        ring.register(ACTION_TAB + 20, leaving);

        let called = Rc::new(std::cell::Cell::new(0u32));
        let n = called.clone();
        set_keyboard_home(Some(Rc::new(move || n.set(n.get() + 1))));
        // No `focus_root` is registered — which is the workspace, every time.
        assert_eq!(innermost_focus_root(), None, "the premise");

        hand_keyboard_back(Some((&ring, leaving)));
        assert_eq!(called.get(), 1, "the workspace's home was asked");
        // And the walk resumes beside the control that left, by tabindex — the id
        // itself no longer resolves.
        assert_eq!(ring.last.get(), Some(ACTION_TAB + 20));

        // Registering nothing is the honest empty state: no home, no panic, no
        // focus moved.
        set_keyboard_home(None);
        hand_keyboard_back(None);
        assert_eq!(called.get(), 1);
    }

    /// **Every index the app really registers must go in without complaint**,
    /// and this list is the app's, not an idealised one.
    ///
    /// It is here because the opposite mistake shipped: a `debug_assert` that
    /// fixed controls end at 110 — a number lifted from a stale comment, while
    /// the Settings modal spaces its sections by hundreds and reaches 310 —
    /// panicked the app on the *correct* code the moment anyone opened Settings.
    /// A guard on registration has to be checked against reality before it is
    /// checked against intent.
    #[test]
    fn every_band_the_app_uses_registers_cleanly() {
        let ring = FocusRing::new();
        for t in [
            NAV_TAB,
            crate::table_designer::LIST_TAB,
            // Form fields: spaced by 10 within a section, by 100 between them.
            // 200/210/220 and 300/310 are the Settings modal; 21 and 31 are a
            // suggestion chevron sitting one past its field.
            5,
            10,
            21,
            31,
            60,
            100,
            140,
            200,
            220,
            310,
            FIXED_TAB_END,
            // Growing blocks, and the footer they must stay clear of.
            VALUE_TAB,
            VALUE_TAB + 90_000 * ROW_TAB_STRIDE,
            ACTION_TAB,
            ACTION_TAB + 20,
            TITLE_CLOSE_TAB,
        ] {
            ring.register(t, floem::ViewId::new());
        }
    }

    /// **The ring member is a wrapper, never the face the caller passed in.**
    /// Two things resolve by exact `ViewId` and were resolving to different ids
    /// depending on how each call site happened to chain its decorators: floem
    /// fires `Click` on the *focused* view for Space/Enter and then folds every
    /// registered `KeyDown` listener, so registering a face that already carried
    /// `on_click_stop` made the ring's own arm a **second** activation (one
    /// Space added two columns, opened two file dialogs, started two bulk
    /// imports); and `.focus(…)` resolves by exact id too, so a face decorated
    /// with `.tooltip()` — which allocates a fresh `ViewId` — put an id in the
    /// ring that carried no outline. A wrapper answers both, and this test is
    /// what stops the registration sliding back onto the face.
    #[test]
    fn a_ring_button_registers_a_wrapper_not_the_face_it_was_given() {
        let ring = FocusRing::new();
        let face = empty();
        let face_id = face.id();
        let button = in_ring_button(face, ring.clone(), ACTION_TAB, true, ACTION_RADIUS, || {});
        assert_ne!(
            button.id(),
            face_id,
            "the face carries the caller's click listener; it must stay out of the ring"
        );
        assert_eq!(ring.ids(), vec![button.id()]);

        // The disabled arm is wrapped too, so enabling a button doesn't change
        // its box — a footer action must not move as a form becomes valid.
        let dead_face = empty();
        let dead_face_id = dead_face.id();
        let dead = in_ring_button(
            dead_face,
            ring.clone(),
            ACTION_TAB + 10,
            false,
            ACTION_RADIUS,
            || {},
        );
        assert_ne!(dead.id(), dead_face_id);
    }

    /// What a dropdown refocuses through after its popup closes: the accept can
    /// have rebuilt the box, so the id captured at build time is gone and only
    /// the tabindex still names the control.
    #[test]
    fn a_tabindex_resolves_to_whatever_currently_holds_it() {
        let (ring, ids) = ring_of(&[10, 20]);
        assert_eq!(ring.at(20), Some(ids[1]));
        let rebuilt = floem::ViewId::new();
        ring.register(20, rebuilt);
        ring.unregister(ids[1]);
        assert_eq!(ring.at(20), Some(rebuilt));
        assert_eq!(ring.at(99), None);
    }
}

#[cfg(test)]
mod popup_slot_tests {
    use super::*;
    use std::cell::Cell;

    fn counting() -> (PopupDismiss, Rc<Cell<u32>>) {
        let n = Rc::new(Cell::new(0));
        let seen = n.clone();
        (Rc::new(move || n.set(n.get() + 1)), seen)
    }

    #[test]
    fn escape_closes_the_published_popup_once() {
        let token = popup_token();
        let (dismiss, seen) = counting();
        set_open_popup(token, dismiss);
        assert!(dismiss_open_popup());
        assert_eq!(seen.get(), 1);
        assert!(!dismiss_open_popup(), "the slot is empty again");
        assert_eq!(seen.get(), 1);
    }

    #[test]
    fn nothing_open_means_escape_falls_through_to_the_modal() {
        assert!(!dismiss_open_popup());
    }

    /// The bug the token exists for. Floem queues B's open during dispatch and
    /// A's close at the end of the same event, so clicking a second dropdown
    /// while the first is up runs `set(B)` and *then* A's clear — and an
    /// untagged clear emptied the slot under B, after which Escape did nothing.
    /// The build-time run of every dropdown's effect (with `open == false`) was
    /// a second way in, so merely constructing one wiped the slot.
    #[test]
    fn a_control_can_only_clear_its_own_entry() {
        let (a, b) = (popup_token(), popup_token());
        let (dismiss_b, seen_b) = counting();
        set_open_popup(b, dismiss_b);
        clear_open_popup(a);
        assert!(dismiss_open_popup(), "B's popup is still closeable");
        assert_eq!(seen_b.get(), 1);
    }

    #[test]
    fn clearing_your_own_entry_empties_the_slot() {
        let token = popup_token();
        let (dismiss, seen) = counting();
        set_open_popup(token, dismiss);
        clear_open_popup(token);
        assert!(!dismiss_open_popup());
        assert_eq!(seen.get(), 0);
    }

    /// Two tokens are never equal, which is what makes the check above mean
    /// anything.
    #[test]
    fn every_control_gets_a_token_of_its_own() {
        assert_ne!(popup_token(), popup_token());
    }
}

#[cfg(test)]
mod follow_tests {
    use super::*;

    /// A streaming answer grows the content under a parked viewport: the moment
    /// the gap opens past the slack, following must stop. The AI panel re-pinned
    /// on every *token*, so scrolling up mid-answer was impossible.
    #[test]
    fn following_stops_once_the_viewport_is_off_the_bottom() {
        assert!(
            at_content_bottom(1000.0, 1000.0, 30.0),
            "exactly at the end"
        );
        assert!(at_content_bottom(1000.0, 980.0, 30.0), "within the slack");
        assert!(
            !at_content_bottom(1000.0, 900.0, 30.0),
            "scrolled up to read"
        );
    }

    /// The boundary is inclusive, so a viewport sitting exactly `slack` px off the
    /// bottom keeps following rather than flapping between the two states.
    #[test]
    fn the_slack_boundary_still_counts_as_the_bottom() {
        assert!(at_content_bottom(1000.0, 970.0, 30.0));
        assert!(!at_content_bottom(1000.0, 969.9, 30.0));
    }

    /// Content shorter than the viewport (a fresh conversation) is trivially at
    /// the bottom — the follow must not be released before anything arrives.
    #[test]
    fn content_shorter_than_the_viewport_is_at_the_bottom() {
        assert!(at_content_bottom(100.0, 400.0, 30.0));
        assert!(at_content_bottom(0.0, 0.0, 30.0));
    }

    /// The regression this function exists for, with the numbers off the real
    /// log: a rebuild collapsed the content, floem clamped the offset to the top,
    /// and `on_scroll` reported a viewport nowhere near the bottom of a 732px
    /// document. No gesture caused it, so the follow must survive it.
    #[test]
    fn a_relayout_that_yanks_the_viewport_to_the_top_keeps_following() {
        assert!(follow_after_scroll(true, false, 673.0, 732.8, 30.0));
    }

    /// Mid-stream the measured height also runs ahead of the offset the last
    /// snap reached. Also not a reader.
    #[test]
    fn content_growing_under_a_parked_viewport_keeps_following() {
        assert!(follow_after_scroll(true, false, 1000.0, 1200.0, 30.0));
    }

    /// The one thing that does release it: the reader scrolling away.
    #[test]
    fn a_user_scroll_away_from_the_bottom_releases_the_follow() {
        assert!(!follow_after_scroll(true, true, 800.0, 1200.0, 30.0));
    }

    /// A gesture that leaves the viewport at the bottom isn't leaving — a wheel
    /// nudge against the end of the content must not stop the follow.
    #[test]
    fn a_gesture_that_stays_at_the_bottom_keeps_following() {
        assert!(follow_after_scroll(true, true, 1000.0, 1000.0, 30.0));
    }

    /// Reaching the bottom re-arms, whether the user scrolled back down to it or
    /// a clamp landed there during a collapse.
    #[test]
    fn reaching_the_bottom_re_arms() {
        // Scrolled back down to the end.
        assert!(follow_after_scroll(false, true, 1000.0, 1000.0, 30.0));
        // Content collapsed below the viewport height: trivially at the bottom.
        assert!(follow_after_scroll(false, false, 673.0, 512.4, 30.0));
    }

    /// A released follow stays released while the content keeps growing — only
    /// the bottom (or the jump button) brings it back.
    #[test]
    fn a_released_follow_is_not_re_armed_by_growth_alone() {
        assert!(!follow_after_scroll(false, false, 800.0, 2000.0, 30.0));
    }

    /// The floor's whole job: a rebuilt `RichText` measures unwrapped for one
    /// pass, so the list momentarily halves. Holding the tallest height seen is
    /// what makes floem's clamp never fire.
    #[test]
    fn a_measurement_dip_does_not_lower_the_floor() {
        assert_eq!(next_floor(1200.0, 600.0, false), 1200.0);
        assert_eq!(
            next_floor(1200.0, 1400.0, false),
            1400.0,
            "growth raises it"
        );
    }

    /// And the half that was missing. "A message only ever grows" is true of
    /// streaming and false of a **re-layout**: dragging the panel wider re-wraps
    /// every bubble shorter, and a floor that only ever rose left ~300px of
    /// blank under the last message — measured as content, lighting the
    /// jump-to-bottom button, and snapping the next follow to the bottom of it.
    #[test]
    fn a_relayout_releases_the_floor_to_the_height_it_just_measured() {
        assert_eq!(next_floor(1200.0, 900.0, true), 900.0);
        // Not a maximum in this arm: the point is that it may go *down*.
        assert_eq!(next_floor(1200.0, 1400.0, true), 1400.0);
    }
}

/// **The anchor-before-the-channel gate.**
///
/// [`menu_anchored_at`] decides whether a trigger owns the menu that is up by
/// comparing `popup_anchor` against the [`crate::PopupAnchor`] the trigger would
/// set itself. That works only because the rule holds at *every* opener: the
/// anchor is what makes it self-invalidating, so an opener that fills the popup
/// channel without writing the anchor first inherits the previous opener's
/// value. The menu then opens at the previous trigger's rect, across the window
/// from the control that raised it, and the previous trigger reports *its* menu
/// as open — so pressing that one closes the new menu instead of opening its own.
/// That is the exact defect `c911c69` / `0ac0d6d` / `7a1f4eb` were three commits
/// to remove, and `docs/architecture.md` says in as many words that the test
/// which would catch it doesn't exist.
///
/// The sites are not greppable by one name — that is the whole difficulty. They
/// are `overlay.popup_menu.set(Some(…))`, `gs.popup.set(Some(…))` and a local
/// `popup.set(Some(…))`, across seven modules. So this reads the source, in the
/// spirit of [`crate::shortcuts`]'s `KEY_FILES` and `core/tests/doc_coverage.rs`:
/// deliberately weak, and there to catch the one failure that recurs.
///
/// **What it checks.** Walking each file in order, every write that *fills* the
/// channel must have an anchor write between it and the previous fill (or the
/// start of the file). That is a stand-in for "in the same opener, before it" —
/// openers don't interleave, and a thirteenth one that forgets the anchor has
/// nothing between it and its predecessor's fill.
#[cfg(test)]
mod popup_anchor_gate {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    /// Openers that deliberately don't write the anchor, each with the reason.
    /// Empty is the healthy state — a site leaves this list the moment it starts
    /// setting one, and joins it only with a reason a reader can check.
    const EXEMPT: &[(&str, u32, &str)] = &[];

    fn src_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
    }

    /// The file with its `#[cfg(test)]` module cut off — test data is full of
    /// `.set(Some(…))` and a gate that cries wolf gets deleted.
    fn production_code(src: &str) -> &str {
        match src.find("#[cfg(test)]") {
            Some(i) => &src[..i],
            None => src,
        }
    }

    /// The file's *logical* lines, each with the 1-based number it starts at.
    ///
    /// `rustfmt` breaks `gs.popup_anchor.set(Some(anchor_below(…)))` across two
    /// lines at the `.`, so a per-line scan reads the receiver and the call
    /// separately and sees neither. Joining a continuation onto its owner is what
    /// makes the two spellings the same text — and getting this wrong is silent:
    /// the gate reported two correct openers as offenders.
    fn logical_lines(src: &str) -> Vec<(u32, String)> {
        let mut out: Vec<(u32, String)> = Vec::new();
        for (i, raw) in src.lines().enumerate() {
            let t = raw.trim_start();
            match out.last_mut() {
                Some((_, prev)) if t.starts_with('.') => prev.push_str(t),
                _ => out.push((i as u32 + 1, t.to_string())),
            }
        }
        out
    }

    /// Does this line *fill* the popup channel? `set(None)` closes it and needs
    /// no anchor; `popup_width.set(…)` is a different signal whose name merely
    /// starts the same way, which is why the suffix is matched exactly.
    fn fills_channel(line: &str) -> bool {
        let t = line.trim_start();
        for name in ["popup_menu", "popup"] {
            let pat = format!("{name}.set(Some(");
            if let Some(i) = t.find(&pat) {
                // Nothing but a path may precede it: `overlay.popup_menu`,
                // `gs.popup`, or the bare local.
                let before = &t[..i];
                let path_only = before
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '_' || c == '.');
                // `gs.popup_menu` must not also count as `popup` — the longer
                // name is tried first, and a hit on `popup` whose char before is
                // a word byte is part of a longer name.
                let boundary = before.chars().last().is_none_or(|c| c == '.');
                if path_only && boundary {
                    return true;
                }
            }
        }
        false
    }

    /// Does this line write the anchor? Both spellings: the shared
    /// `popup_anchor`, and `table_designer`'s local, which is simply `anchor`
    /// because its popup is local too.
    fn sets_anchor(line: &str) -> bool {
        let t = line.trim_start();
        ["popup_anchor.set(", "anchor.set("]
            .iter()
            .any(|p| t.contains(p))
    }

    #[test]
    fn every_opener_sets_the_anchor_before_filling_the_popup_channel() {
        let exempt: BTreeSet<(&str, u32)> = EXEMPT.iter().map(|(f, l, _)| (*f, *l)).collect();
        let mut offenders: Vec<String> = Vec::new();
        let mut fills = 0usize;

        let mut files: Vec<PathBuf> = std::fs::read_dir(src_dir())
            .expect("the crate's own src")
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "rs"))
            .collect();
        files.sort();

        for path in files {
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            let src = std::fs::read_to_string(&path).expect("readable");
            let mut anchored = false;
            for (lineno, line) in logical_lines(production_code(&src)) {
                if sets_anchor(&line) {
                    anchored = true;
                }
                if fills_channel(&line) {
                    fills += 1;
                    if !anchored && !exempt.contains(&(name.as_str(), lineno)) {
                        offenders.push(format!(
                            "{name}:{lineno} fills the popup channel with no \
                             `popup_anchor.set` since the previous opener"
                        ));
                    }
                    // The next fill is a different opener and needs its own.
                    anchored = false;
                }
            }
        }

        assert!(offenders.is_empty(), "{}", offenders.join("\n"));
        // The scan has to still be finding the sites: a renamed channel would
        // otherwise make this pass by seeing nothing at all.
        assert!(
            fills >= 10,
            "the scan found only {fills} openers — has the channel been renamed?"
        );
    }
}
