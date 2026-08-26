//! **The two panel dividers, and the delay that lights them.**
//!
//! `h_resize_handle` drags the schema tree's and the right panel's edges;
//! `v_resize_handle` drags the split between the query editor and the results
//! grid. Both are absolute children positioned from an *effective* (clamped)
//! edge rather than from the width they set, both capture the pointer on press,
//! and both reset to a default on double-click.
//!
//! Not to be confused with `window_chrome::resize_zones`, which resizes the
//! **window**: those are eight loose siblings mounted outside the app root and
//! have to be hit before everything, while these two live inside the panels
//! they divide.
//!
//! [`DelayedHover`] is here rather than in `widgets.rs` because these are its
//! only two users and the delay is a property of *this* affordance — see its own
//! doc for why leaving is instant while arriving is not.

use std::rc::Rc;

use floem::event::{Event, EventListener, EventPropagation};
use floem::prelude::*;
use floem::style::CursorStyle;

use crate::consts::{RESIZE_HOVER_DELAY, resize_bar, resize_hit};
use crate::theme;

/// A divider's hover highlight, which arrives `RESIZE_HOVER_DELAY` after the
/// pointer settles rather than the moment it arrives.
///
/// Two signals that have to move together — the flag the style reads, and a
/// sequence number that tells a fired timer whether it is still the one in
/// charge — so they are one value rather than two the two handles each wire up
/// their own way.
///
/// **The sequence is what makes leaving instant.** There is no cancelling a
/// floem timer, so `leave` bumps the number and the pending arm, when it fires,
/// finds itself superseded and does nothing. That half is load-bearing at every
/// site.
///
/// The same comparison also covers a timer that outlives the scope that armed it
/// — `exec_after` is not cancelled on teardown either, and `try_get_untracked`
/// answers `None` for a disposed signal, which is not `Some(mine)`, so one check
/// retires both. **That half is defensive here and not reachable**, and the
/// reason this comment used to give for it was wrong: the editor/results splitter
/// is *not* per-tab. `v_resize_handle` is called once from `center`, itself built
/// once in `workspace`'s shell, and the per-tab `dyn_container`s (`editor_area`,
/// `results_area`) are its siblings rather than its parents — so these two
/// signals live in the workspace's own scope and cannot be disposed while the app
/// runs. The guard stays because a divider whose panel is rebuilt would reach it,
/// and because floem offers no way to cancel the timer that would make it
/// unnecessary; it is not evidence that this site has ever needed it.
#[derive(Clone, Copy)]
struct DelayedHover {
    lit: RwSignal<bool>,
    seq: RwSignal<u64>,
}

impl DelayedHover {
    fn new() -> Self {
        Self {
            lit: RwSignal::new(false),
            seq: RwSignal::new(0),
        }
    }

    /// Reactive: whether the bar should be painted.
    fn lit(self) -> bool {
        self.lit.get()
    }

    /// The pointer arrived — start the clock.
    fn enter(self) {
        let mine = self.seq.get_untracked().wrapping_add(1);
        self.seq.set(mine);
        floem::action::exec_after(RESIZE_HOVER_DELAY, move |_| {
            if self.seq.try_get_untracked() == Some(mine) {
                self.lit.set(true);
            }
        });
    }

    /// The pointer left — dark immediately, and any pending arm is void.
    fn leave(self) {
        self.seq.update(|n| *n = n.wrapping_add(1));
        self.lit.set(false);
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn h_resize_handle(
    from_right: bool,
    dim: RwSignal<f64>,
    // Effective (clamped) panel width → where the handle sits. May be less than
    // `dim` when the window is too narrow to honor the full intended width.
    edge: impl Fn() -> f64 + Copy + 'static,
    visible: impl Fn() -> bool + Copy + 'static,
    // Owned by the caller so the panel wrapper can drop its width transition while
    // dragging (the transition is for the collapse slide; during a resize it just
    // makes the width lag the pointer).
    dragging: RwSignal<bool>,
    // Drag clamp: floor `min_w()`, ceiling `max_w()` — both reactive, leaving the
    // center + the opposite panel their minimums. **A `fn`, not a number**: it is
    // `schema_min_w()`/`right_min_w()`, which are `scaled`, and a captured one
    // froze the floor at whatever scale the view was last built at. See
    // `scaled_arg_gate`.
    min_w: fn() -> f64,
    max_w: impl Fn() -> f64 + Copy + 'static,
    // Double-click resets `dim` to this default (animated, since not dragging).
    default: f64,
    // Persist the layout after a drag ends or a reset (debounced to gesture-end so
    // we don't write on every pixel).
    on_commit: Rc<dyn Fn()>,
) -> impl IntoView {
    // Dragging lights the bar with no delay: the press has already found the
    // divider, so there is nothing left to hint at.
    let hovered = DelayedHover::new();
    let bar = empty().style(move |s| {
        let s = s.width(resize_bar()).height_full();
        if hovered.lit() || dragging.get() {
            s.background(theme::resize_handle())
        } else {
            s
        }
    });
    let handle = container(bar);
    let id = handle.id();
    handle
        .style(move |s| {
            let s = s
                .absolute()
                .height_full()
                .items_center()
                .justify_center()
                .cursor(CursorStyle::ColResize)
                .width(if visible() { resize_hit() } else { 0.0 });
            let inset = edge() - resize_hit() / 2.0;
            if from_right {
                s.inset_right(inset)
            } else {
                s.inset_left(inset)
            }
        })
        .on_event(EventListener::PointerEnter, move |_| {
            hovered.enter();
            EventPropagation::Continue
        })
        .on_event(EventListener::PointerLeave, move |_| {
            hovered.leave();
            EventPropagation::Continue
        })
        .on_event_stop(EventListener::PointerDown, move |_| {
            dragging.set(true);
            id.request_active();
        })
        .on_event(EventListener::PointerMove, move |e| {
            if dragging.get_untracked() {
                if let Event::PointerMove(pe) = e {
                    // pe.pos is relative to the handle, whose center rides the
                    // boundary; (pos - center) is the pointer's offset from it, so
                    // adding it snaps the boundary to the pointer (negated for the
                    // right column, which grows leftward).
                    let d = pe.pos.x - resize_hit() / 2.0;
                    let d = if from_right { -d } else { d };
                    // Floor at `min_w`; ceiling `max_w()` keeps the center (and the
                    // opposite panel) at their minimums so a drag can't swallow them.
                    // Both read **here**, inside the gesture, so a scale change
                    // between build and drag moves the floor with everything else.
                    let lo = min_w();
                    let hi = max_w().max(lo);
                    dim.update(|w| *w = (*w + d).clamp(lo, hi));
                }
                EventPropagation::Stop
            } else {
                EventPropagation::Continue
            }
        })
        .on_event_stop(EventListener::PointerUp, {
            let on_commit = on_commit.clone();
            move |_| {
                dragging.set(false);
                id.clear_active();
                on_commit();
            }
        })
        .on_double_click_stop(move |_| {
            // The double-click's second PointerUp is consumed here (not by the
            // PointerUp handler), so clear the drag state ourselves — otherwise the
            // handle stays captured/active and keeps resizing on mouse-move.
            dragging.set(false);
            id.clear_active();
            // And darken the bar, for the same reason one step further on: the
            // reset below moves the handle **out from under a pointer that has not
            // moved**, so floem delivers no `PointerLeave` and nothing else would
            // ever turn the highlight off. The divider animated to its default
            // while still lit, and stayed lit there until the next mouse move
            // happened to trigger the leave. `leave` also voids a pending arm, so
            // a double-click inside the hover delay cannot light it afterwards.
            hovered.leave();
            dim.set(default);
            on_commit();
        })
}

// A horizontal divider between the query editor and the results grid (drags
// up/down). `base_top` offsets past the tab bar to the editor's bottom edge; `dim`
// is the editor height. Always shown (both areas are always present).
pub(crate) fn v_resize_handle(
    // **A `fn`, not a number**: `tab_bar_h()` is `scaled`, and a captured one left
    // the divider drawing at the previous scale's tab-bar height — ~20px low, over
    // the results toolbar, after 160% → 100%. See `scaled_arg_gate`.
    base_top: fn() -> f64,
    dim: RwSignal<f64>,
    // Effective (floored) editor height → where the handle sits. May be more than
    // `dim` when a height persisted under a lower floor is being rendered against
    // a higher one, which is why the handle can't position from `dim` itself: it
    // would float inside the grid, away from the edge it drags.
    edge: impl Fn() -> f64 + Copy + 'static,
    // Drag clamp: floor `min_h()` (query editor min), ceiling `max_h()` — both
    // reactive, leaving the results grid its minimum height. A `fn` for the same
    // reason `min_w` is.
    min_h: fn() -> f64,
    max_h: impl Fn() -> f64 + Copy + 'static,
    default: f64,
    on_commit: Rc<dyn Fn()>,
) -> impl IntoView {
    let hovered = DelayedHover::new();
    let dragging = RwSignal::new(false);
    let bar = empty().style(move |s| {
        let s = s.height(resize_bar()).width_full();
        if hovered.lit() || dragging.get() {
            s.background(theme::resize_handle())
        } else {
            s
        }
    });
    let handle = container(bar);
    let id = handle.id();
    handle
        .style(move |s| {
            s.absolute()
                .width_full()
                .items_center()
                .justify_center()
                .cursor(CursorStyle::RowResize)
                .height(resize_hit())
                .inset_top(base_top() + edge() - resize_hit() / 2.0)
        })
        .on_event(EventListener::PointerEnter, move |_| {
            hovered.enter();
            EventPropagation::Continue
        })
        .on_event(EventListener::PointerLeave, move |_| {
            hovered.leave();
            EventPropagation::Continue
        })
        .on_event_stop(EventListener::PointerDown, move |_| {
            dragging.set(true);
            id.request_active();
        })
        .on_event(EventListener::PointerMove, move |e| {
            if dragging.get_untracked() {
                if let Event::PointerMove(pe) = e {
                    let d = pe.pos.y - resize_hit() / 2.0;
                    // Read here, inside the gesture — see `min_w`'s twin above.
                    let lo = min_h();
                    dim.update(|h| *h = (*h + d).clamp(lo, max_h().max(lo)));
                }
                EventPropagation::Stop
            } else {
                EventPropagation::Continue
            }
        })
        .on_event_stop(EventListener::PointerUp, {
            let on_commit = on_commit.clone();
            move |_| {
                dragging.set(false);
                id.clear_active();
                on_commit();
            }
        })
        .on_double_click_stop(move |_| {
            // See h_resize_handle: clear the drag state the eaten PointerUp would
            // have, and darken the bar the reset moves out from under the pointer.
            dragging.set(false);
            id.clear_active();
            hovered.leave();
            dim.set(default);
            on_commit();
        })
}

#[cfg(test)]
mod double_click_gate {
    use std::path::Path;

    /// **A reset has to undo everything the gesture turned on**, and there are two
    /// of those, discovered a year apart in the same four lines.
    ///
    /// The double-click's second `PointerUp` is consumed by `on_double_click_stop`
    /// and never reaches the `PointerUp` handler, so anything that handler would
    /// have cleared has to be cleared here instead. `dragging`/`clear_active` was
    /// the first (the handle stayed captured and kept resizing on mouse-move).
    /// `hovered` is the second, and it is subtler: `dim.set(default)` moves the
    /// handle **out from under a pointer that never moved**, so floem delivers no
    /// `PointerLeave` at all — the bar stayed lit at the divider's new position
    /// until the next mouse move happened to trigger one.
    ///
    /// Deliberately weak, like the crate's four other source gates
    /// (`modals::modal_backdrop_gate`, `widgets::popup_anchor_gate`,
    /// `menu_trigger_gate`, `menu_panel_gate`): it asserts the two calls are
    /// *present* in each handler, not what they do. What makes it worth having is
    /// that these two handles are **twins** — every fix here is two edits, and
    /// "fixed one twin, left the other" is the shape this codebase's reviews keep
    /// catching. A third divider added later inherits the check for free.
    #[test]
    fn every_reset_darkens_the_divider_and_releases_the_pointer() {
        let src = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join("dividers.rs"),
        )
        .expect("dividers.rs");
        // This module quotes the calls it is looking for, so cut it off, and drop
        // comment lines for the same reason.
        let body = match src.find("#[cfg(test)]") {
            Some(i) => &src[..i],
            None => &src[..],
        };
        let body: String = body
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");

        let handlers: Vec<&str> = body
            .split(".on_double_click_stop(")
            .skip(1)
            .map(|rest| {
                let end = rest
                    .find("\n        })")
                    .expect("a double-click handler that closes at the builder's indent");
                &rest[..end]
            })
            .collect();

        // The floor: a rename of the builder method would otherwise leave nothing
        // to check and pass silently, which is what a source gate is most prone to.
        assert!(
            handlers.len() >= 2,
            "found {} double-click handlers in dividers.rs — both dividers have one, \
             so did `on_double_click_stop` get renamed?",
            handlers.len()
        );

        for (i, h) in handlers.iter().enumerate() {
            assert!(
                h.contains("hovered.leave()"),
                "double-click handler #{i} resets the divider without darkening it. \
                 `dim.set(default)` moves the handle out from under a pointer that \
                 has not moved, so no `PointerLeave` is delivered and the bar stays \
                 lit at the new position until the user happens to move the mouse."
            );
            assert!(
                h.contains("dragging.set(false)") && h.contains("clear_active()"),
                "double-click handler #{i} does not release the pointer. The \
                 double-click eats the second `PointerUp`, so this handler is the \
                 only place that runs — without these the handle stays captured and \
                 keeps resizing on mouse-move with no button down."
            );
        }
    }
}

/// **A length a handle is *given* must be a `fn`, not a number** — unless it is
/// on the list below with a reason.
///
/// Both dividers took their clamp floors and the editor's top offset as plain
/// `f64`, resolved by the caller at build time. Every one of them is a
/// `theme::scaled` metric, so every one froze at whatever the interface scale was
/// when the view was last built, and two visible bugs came out of the same three
/// parameters:
///
/// * `base_top` (`tab_bar_h()`) froze, so after 160% → 100% the query/results
///   divider drew ~20px low — over the results toolbar — because the style
///   closure re-ran with a stale offset baked into it.
/// * `min_w` (`schema_min_w()` / `right_min_w()`) froze, so after 160% → 100% the
///   schema and AI panels could not be dragged below **400px** — 250 scaled by
///   1.6 — and only relaunching the app cleared it. The *render* clamp was never
///   wrong: `effective_schema_w` calls `schema_min_w()` inside itself. Only the
///   drag floor was.
///
/// `min_h` was the same defect on the third parameter and had simply not been
/// noticed yet.
///
/// A source gate rather than a unit test, and that is not a compromise: the
/// arithmetic (`clamp(min, max)`) was correct throughout. What was wrong was
/// **when the number was read**, which no test of a pure function can see — the
/// same reason `theme::scaled`'s own doc says to call it inside the style closure.
/// The sibling parameters (`edge`, `max_w`, `visible`) were always `impl Fn`,
/// which is why nobody noticed the other three were not.
#[cfg(test)]
mod scaled_arg_gate {
    use std::path::Path;

    /// `(parameter, why a plain `f64` is right for it)`. Everything else in a
    /// handle's signature must be a `fn() -> f64` or a signal.
    const EXEMPT: &[(&str, &str)] = &[(
        "default",
        "the double-click reset target — `theme::SCHEMA_W` / `AI_W` / `EDITOR_H`, \
         which are seeds for a persisted user-dragged size and are deliberately \
         unscaled (see `consts.rs`'s exception list). A scale change must not \
         move where a reset lands, or the same double-click would give a \
         different width at each scale.",
    )];

    #[test]
    fn no_handle_takes_a_length_it_cannot_re_read() {
        let src = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join("dividers.rs"),
        )
        .expect("dividers.rs");
        let body = match src.find("#[cfg(test)]") {
            Some(i) => &src[..i],
            None => &src[..],
        };

        let mut checked = 0usize;
        let mut offenders: Vec<String> = Vec::new();
        for (name, sig) in body
            .split("pub(crate) fn ")
            .skip(1)
            .filter_map(|rest| {
                let name = rest.split('(').next()?.trim().to_string();
                let end = rest.find(") -> impl IntoView")?;
                Some((name, rest[..end].to_string()))
            })
            .filter(|(n, _)| n.ends_with("_resize_handle"))
        {
            checked += 1;
            for line in sig.lines() {
                let line = line.trim();
                if line.starts_with("//") {
                    continue;
                }
                let Some(param) = line.strip_suffix(": f64,").or(line.strip_suffix(": f64")) else {
                    continue;
                };
                if EXEMPT.iter().any(|(p, _)| *p == param) {
                    continue;
                }
                offenders.push(format!(
                    "{name}: `{param}: f64` is resolved at build time, so it \
                     freezes at the interface scale the view was last built at. \
                     Take it as `fn() -> f64` and call it where it is used. If it \
                     is genuinely not a scaled length, add it to EXEMPT with the \
                     reason."
                ));
            }
        }
        assert_eq!(
            checked, 2,
            "expected both resize handles; found {checked} — did one get renamed?"
        );
        assert!(offenders.is_empty(), "{}", offenders.join("\n"));
    }

    /// An exemption that no longer names a real parameter is a stale licence the
    /// next `f64` at that spelling would inherit.
    #[test]
    fn every_exemption_still_names_a_real_parameter() {
        let src = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join("dividers.rs"),
        )
        .expect("dividers.rs");
        let body = &src[..src.find("#[cfg(test)]").unwrap_or(src.len())];
        for (param, why) in EXEMPT {
            assert!(
                body.contains(&format!("{param}: f64")),
                "EXEMPT licenses `{param}: f64` ({why}), but no handle takes it \
                 any more — drop the entry"
            );
        }
    }
}
