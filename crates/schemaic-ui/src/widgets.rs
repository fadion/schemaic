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
use floem::kurbo::Point;
use floem::prelude::*;
use floem::reactive::Scope;
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
            // **Hand the keyboard back.** Floem clears `app_state.focus` when a
            // focused view is removed and does it *silently* — no
            // `focus_changed`, so no `FocusGained` lands anywhere — and a key
            // event then goes to the focused view and, failing that, only to the
            // root's own listeners. So closing a popup menu opened over a modal
            // left the modal underneath keyboard-dead: Escape did nothing, close
            // and Cancel still worked. The nested editors escaped it only by
            // accident, unmounting under the preview and being rebuilt.
            //
            // The same step `edit_field`'s Escape branch takes, for the same
            // reason.
            if let Some(r) = innermost_focus_root() {
                r.request_focus();
            }
        })
}

/// The innermost mounted [`focus_root`], or `None` when no overlay is open (a
/// field in the main workspace then simply drops focus on Escape).
pub(crate) fn innermost_focus_root() -> Option<floem::ViewId> {
    FOCUS_ROOTS.with_borrow(|s| s.last().map(|(id, _)| *id))
}

/// The innermost mounted overlay's [`FocusRing`], for the window root's Tab
/// backstop: with focus on a dropdown's popup list — or on nothing at all, which
/// is what a click on an unfocusable list row leaves behind — the key reaches
/// neither a ring member nor the modal's own root, and floem's fallback walks
/// the *whole window tree*, so Tab escaped the modal into the workspace behind
/// it. The root can step this ring instead.
pub(crate) fn innermost_focus_ring() -> Option<FocusRing> {
    FOCUS_ROOTS.with_borrow(|s| s.last().and_then(|(_, r)| r.clone()))
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
    last: Rc<std::cell::Cell<Option<floem::ViewId>>>,
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
    pub(crate) fn register(&self, tabindex: u32, id: floem::ViewId) {
        let mut e = self.entries.borrow_mut();
        e.retain(|(_, x)| *x != id);
        let at = e.partition_point(|(t, _)| *t <= tabindex);
        e.insert(at, (tabindex, id));
    }

    pub(crate) fn unregister(&self, id: floem::ViewId) {
        self.entries.borrow_mut().retain(|(_, x)| *x != id);
    }

    /// Remember `id` as where the walk should resume — what a control calls on
    /// its way out when it hands focus back to the modal root, so the root's own
    /// Tab continues from it instead of restarting at the top.
    pub(crate) fn remember(&self, id: floem::ViewId) {
        self.last.set(Some(id));
    }

    /// Where one step from `from` lands: the [remembered](Self::remember)
    /// position stands in when `from` isn't a ring member — which is every
    /// re-entry from a modal root, a popup list, or nowhere. Neither known:
    /// start at the near end.
    pub(crate) fn target(&self, from: floem::ViewId, backwards: bool) -> Option<floem::ViewId> {
        let e = self.entries.borrow();
        let find = |id: floem::ViewId| e.iter().position(|(_, x)| *x == id);
        let cur = find(from).or_else(|| self.last.get().and_then(find));
        ring_step(e.len(), cur, backwards).map(|n| e[n].1)
    }

    /// Move focus one step from `from`, per [`FocusRing::target`].
    pub(crate) fn step_from(&self, from: floem::ViewId, backwards: bool) {
        if let Some(id) = self.target(from, backwards) {
            self.last.set(Some(id));
            id.request_focus();
        }
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
            self.last.set(Some(id));
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
/// [`innermost_focus_ring`].
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
                step_ring.remember(id);
                id.clear_focus();
                if let Some(root) = innermost_focus_root() {
                    root.request_focus();
                }
                return EventPropagation::Stop;
            }
            EventPropagation::Continue
        })
        .on_cleanup(move || {
            cleanup_ring.unregister(id);
            on_dispose();
            // **Hand the keyboard back**, the same step `focus_root`'s cleanup
            // takes and for the same reason: floem clears `app_state.focus`
            // *silently* when the focused view is removed, so a control that
            // unmounts while focused leaves the keyboard nowhere and the modal
            // around it answers neither Escape nor Tab. One click on the
            // designer's list `+` did it — the click focuses the pane, and the
            // draft it edits is half the container's key.
            if at_cleanup.get() {
                if let Some(root) = innermost_focus_root() {
                    root.request_focus();
                }
            }
        })
}

// ===== moved from lib.rs (widgets cluster) =====
// A title bar for a modal panel, with a close (×) button.
pub(crate) fn modal_title(title: &'static str, close: Rc<dyn Fn()>) -> impl IntoView {
    modal_title_impl(title, close, true)
}

/// Like [`modal_title`] but without the bottom separator border — for modals
/// whose body already reads as a distinct block (the plan modal's boxed table).
pub(crate) fn modal_title_borderless(title: &'static str, close: Rc<dyn Fn()>) -> impl IntoView {
    modal_title_impl(title, close, false)
}

/// [`modal_title`] for a title that isn't known at compile time (the import
/// modal names the table it's loading into).
pub(crate) fn modal_title_owned(title: String, close: Rc<dyn Fn()>) -> impl IntoView {
    modal_title_impl(title, close, true)
}

fn modal_title_impl(title: impl Into<String>, close: Rc<dyn Fn()>, border: bool) -> impl IntoView {
    let title = title.into();
    h_stack((
        text(title).style(|s| s.font_size(15.0).font_bold().color(theme::text())),
        empty().style(|s| s.flex_grow(1.0_f32)),
        // Lucide X, 16px, vertically centred; `padding(6)` enlarges the click
        // hitbox (same idiom as `toolbar_icon`) so it's not fiddly to hit. Same
        // dim→bright colour as the old glyph.
        container(icons::icon(icons::X, 16.0))
            .on_click_stop(move |_| (close)())
            .style(|s| {
                s.flex_shrink(0.0_f32)
                    .items_center()
                    .padding(6.0)
                    .color(theme::text_dim())
                    .hover(|s| s.color(theme::text()))
            }),
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
pub(crate) fn row_button(
    glyph: &'static str,
    tip: &'static str,
    act: impl Fn() + 'static,
) -> AnyView {
    row_slot(crate::icons::icon(glyph, ROW_ICON as f32))
        .on_click_stop(move |_| act())
        // Colour-only hover, like every other icon button in the app.
        .style(|s| s.color(theme::text_dim()).hover(|s| s.color(theme::text())))
        .tooltip(move || text(tip).style(tooltip_style))
        .into_any()
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
    on_click: impl Fn() + 'static,
) -> impl IntoView {
    control_button_enabled(label, true, on_click)
}

/// [`control_button`] that can be inert — for one whose subject may be missing
/// (Edit, with nothing selected). Dimmed and unclickable rather than absent, on
/// the same grounds a disabled [`action_button`] keeps its place: a control that
/// comes and goes moves the row it sits in.
pub(crate) fn control_button_enabled(
    label: impl Into<String>,
    enabled: bool,
    on_click: impl Fn() + 'static,
) -> impl IntoView {
    text(label.into())
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
/// Disabled keeps the fill and fades the label ([`theme::btn_text_disabled`]),
/// rather than hiding or unfilling the button: which action is the affirmative
/// one shouldn't move around as a form becomes valid.
pub(crate) fn action_button(
    label: impl Into<String>,
    kind: ActionKind,
    enabled: bool,
    on_click: impl Fn() + 'static,
) -> impl IntoView {
    action_button_inner(label, None, kind, enabled, on_click)
}

/// [`action_button`] with a leading icon — the preview footer's Copy and Open in
/// editor. The glyph inherits the button's colour, so it follows the disabled
/// state without a second rule.
pub(crate) fn action_button_icon(
    label: impl Into<String>,
    icon: &'static str,
    kind: ActionKind,
    enabled: bool,
    on_click: impl Fn() + 'static,
) -> impl IntoView {
    action_button_inner(label, Some(icon), kind, enabled, on_click)
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
        .border_radius(5.0)
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

fn action_button_inner(
    label: impl Into<String>,
    icon: Option<&'static str>,
    kind: ActionKind,
    enabled: bool,
    on_click: impl Fn() + 'static,
) -> impl IntoView {
    let glyph: AnyView = match icon {
        Some(markup) => icons::icon(markup, 15.0)
            .style(|s| s.flex_shrink(0.0_f32).margin_right(ACTION_ICON_GAP))
            .into_any(),
        None => empty().into_any(),
    };
    h_stack((glyph, text(label.into())))
        .on_click_stop(move |_| {
            if enabled {
                on_click()
            }
        })
        .style(move |s| action_style(s, kind, enabled))
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
    face: V,
    on_click: F,
) -> impl IntoView + use<V, F> {
    // +2px against sub-pixel rounding, the same guard `loading_dots` uses.
    let w = measure_text_px_at(width_for, theme::FONT_BODY) + 2.0 * ACTION_PAD_H + 2.0;
    container(face)
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
pub(crate) fn dialog_button(
    label: impl Into<String>,
    color: fn() -> floem::peniko::Color,
    hover: fn() -> floem::peniko::Color,
    on_click: impl Fn() + 'static,
) -> impl IntoView {
    text_button(label, color, hover, true, (10.0, 5.0), on_click)
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
                .border_radius(6.0);
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

/// One menu row's content: `[icon] label [→]` (the chevron only for submenus).
/// `label_color` tints the label (a `fn` so it follows theme switches); `None`
/// uses the default text colour.
fn menu_row(
    icon: Option<MenuIcon>,
    label: String,
    label_color: Option<fn() -> floem::peniko::Color>,
    chevron: bool,
    disabled: bool,
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
}

/// Render one entry. `open_sub` is this level's "which sibling submenu is open"
/// signal — entering a leaf clears it, entering a submenu row sets it, so moving
/// between rows switches/closes submenus while moving *onto* an open submenu (it's
/// flush with the panel's right edge) keeps it open.
fn menu_entry_view(
    i: usize,
    entry: MenuEntry,
    open_sub: RwSignal<Option<usize>>,
    close: Rc<dyn Fn()>,
) -> AnyView {
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
        } => menu_row(icon, label, label_color, false, disabled)
            .on_click_stop(move |_| {
                if disabled {
                    return; // inert; the stop keeps the menu open
                }
                (action)();
                (close)();
            })
            .on_event(EventListener::PointerEnter, move |_| {
                open_sub.set(None);
                EventPropagation::Continue
            })
            .into_any(),
        MenuEntry::Sub {
            label,
            icon,
            children,
        } => {
            let n = children.len();
            // Submenus keep the standard width (they only appear in the grid menus).
            let sub = menu_stack(children, close, 170.0);
            // The parent row's window position/width, to decide edge-flips.
            let row_origin: RwSignal<Point> = RwSignal::new(Point::ZERO);
            let row_w = RwSignal::new(0.0_f64);
            // Wrap the panel in the absolute *container* (an absolute panel would
            // shrink-wrap and collapse its full-width rows to the text width); the
            // panel stays in-flow with its `min_width`, so rows fill it.
            let sub_wrap = container(sub).style(move |s| {
                if open_sub.get() != Some(i) {
                    return s.hide();
                }
                let (win_w, win_h) = window_size().get();
                let ro = row_origin.get();
                let rw = row_w.get();
                // Conservative size estimates (menu min_width + a row's ~34px).
                let sub_w = 210.0;
                let sub_h = n as f64 * 34.0 + 14.0;
                // Flip left if the submenu would spill past the right edge.
                let flip_x = win_w > 1.0 && ro.x + rw + sub_w > win_w;
                // Shift up if it would spill past the bottom edge (align to fit).
                let top = if win_h > 1.0 && ro.y - 6.0 + sub_h > win_h {
                    (win_h - sub_h - ro.y).max(-ro.y)
                } else {
                    -6.0 // lift so the submenu's first item lines up with this row
                };
                let s = s.absolute().inset_top(top);
                if flip_x {
                    s.inset_right_pct(100.0)
                } else {
                    s.inset_left_pct(100.0)
                }
            });
            stack((menu_row(icon, label, None, true, false), sub_wrap))
                .on_move(move |p| row_origin.set(p))
                .on_resize(move |r| row_w.set(r.width()))
                .on_event(EventListener::PointerEnter, move |_| {
                    open_sub.set(Some(i));
                    EventPropagation::Continue
                })
                .on_click_stop(|_| {}) // clicking the parent just holds it open
                .into_any()
        }
    }
}

/// One menu level: the styled panel of rows (used for the root and every submenu).
/// `width` is the panel's `min_width` (short labels never exceed it).
fn menu_stack(entries: Vec<MenuEntry>, close: Rc<dyn Fn()>, width: f64) -> impl IntoView {
    let open_sub: RwSignal<Option<usize>> = RwSignal::new(None);
    let rows: Vec<AnyView> = entries
        .into_iter()
        .enumerate()
        .map(|(i, e)| menu_entry_view(i, e, open_sub, close.clone()))
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
pub(crate) fn menu_panel_height(entries: &[MenuEntry]) -> f64 {
    entries
        .iter()
        .map(|e| match e {
            MenuEntry::Separator => 9.0,
            _ => 30.5,
        })
        .sum::<f64>()
        + 14.0
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

/// A reusable themed popup menu with nested submenus, `width` px wide. Returns the
/// panel; the caller positions it absolutely. Escape (and any action) calls `close`.
pub(crate) fn menu_panel(
    entries: Vec<MenuEntry>,
    close: Rc<dyn Fn()>,
    width: f64,
) -> impl IntoView {
    let esc = close.clone();
    focus_root(menu_stack(entries, close, width)).on_key_down(
        Key::Named(NamedKey::Escape),
        |_| true,
        move |_| (esc)(),
    )
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
        .on_event_cont(EventListener::PointerMove, {
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
        .border_radius(6.0)
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
/// the switch free (CLAUDE.md → *Themable colors reach reactive styles as
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
    use floem::text::{Attrs, AttrsList, FamilyOwned, LineHeightValue, TextLayout, Weight};
    let base_weight = if bold { Weight::BOLD } else { Weight::NORMAL };
    floem::views::rich_text(move || {
        let sans = [FamilyOwned::Name("IBM Plex Sans".to_string())];
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
    toggle_icon_view(
        icons::icon(glyph, 16.0).style(|s| s.flex_shrink(0.0_f32)),
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
    // Wrap the glyph in a container that carries the padding + click handler:
    // Floem hit-tests an `Svg` against its rendered content only (padding on the
    // svg grows layout but not the click target), whereas a container hit-tests its
    // whole padded box. The icon inherits the colour via `currentColor`, so the
    // active/hover tint set on the container reaches the svg.
    container(icon)
        .on_click_stop(move |_| on_click())
        .style(move |s| {
            // No pointer cursor — the app uses the normal cursor everywhere.
            let s = s
                .items_center()
                .flex_shrink(0.0_f32)
                .padding_vert(3.0)
                .padding_horiz(5.0);
            if active() {
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

    #[test]
    fn a_remembered_view_that_has_since_unmounted_starts_the_walk_over() {
        let (ring, ids) = ring_of(&[10, 20, 30]);
        ring.remember(ids[1]);
        ring.unregister(ids[1]);
        assert_eq!(ring.target(floem::ViewId::new(), false), Some(ids[0]));
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
}
