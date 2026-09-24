//! The app's tooltip: floem's, anchored to the element instead of the pointer.
//!
//! floem 0.2's `Tooltip` opens its tip at **the pointer plus 10px** — and its
//! default theme gives `TooltipClass` a `margin(10.0)` on top, which the app's
//! chrome never overrode, so in practice **the pointer plus 20px** — wherever on
//! the element the pointer happened to be when the hover delay ran out. On a
//! 16px icon that is a tip whose position depends on the hover rather than on
//! the icon: rest on its right half and the tip starts well past the icon, far
//! enough out to read as belonging to the next one. [`AnchoredTooltip`] is the
//! same view with one decision changed — the tip opens under the element's own
//! box ([`element_box`]), at its left edge ([`anchor_below`]) — so it lands in
//! the same place every time.
//!
//! Everything else is floem's, deliberately: the same hover delay (read from
//! `TooltipContainerClass`'s `Delay`, which the workspace root sets), the same
//! events that dismiss it (leaving, pressing, releasing, scrolling, a key), no
//! tip while a drag is held (floem also waits out a released drag's animation,
//! through a field it does not expose), and the same overlay, whose paint
//! already pulls a tip that would run off the window's right or bottom edge back
//! inside it.
//!
//! **The chrome is applied to the tip directly.** floem computes the tip's
//! style from `TooltipClass` off the tooltip view's style context, because the
//! tip is an overlay — a child of the window, not of the app root — and so
//! never sees the root's class rules. This applies [`widgets::tooltip_style`]
//! instead, and so no longer picks up what floem's *theme* put in that class
//! underneath the app's rule: the 10px margin above, which was half the
//! reported shift. (Not the theme's shadow offset — the app's shadow builders
//! had already replaced that whole property.)
//!
//! Reached as `.tooltip(…)` through [`TooltipExt`], which every file that calls
//! it imports by name so the import shadows floem's own `TooltipExt` from the
//! prelude glob. `tooltip_gate` below holds each such file to it.

use std::cell::RefCell;
use std::rc::Rc;

use floem::action::{TimerToken, add_overlay, exec_after, remove_overlay};
use floem::context::{ComputeLayoutCx, EventCx, StyleCx, UpdateCx};
use floem::event::{Event, EventPropagation};
use floem::kurbo::{Point, Rect, Size};
use floem::prelude::*;
use floem::prop_extractor;
use floem::views::{Delay, TooltipContainerClass};
use floem::{AnyView, View, ViewId};

use crate::{theme, widgets};

prop_extractor! {
    TipStyle {
        delay: Delay,
    }
}

/// How far below the element a tip opens, before interface scaling.
const GAP: f64 = 6.0;

/// Where a tip opens, given the hovered element's window origin and size: under
/// its bottom edge by `gap`, aligned to its left edge.
///
/// Not centred: a tip is usually wider than the icon it names, and a centred
/// one would reach left over the neighbouring icon as far as it reaches right.
/// Starting at the element's own left edge keeps the tip visibly *its* tip.
fn anchor_below(origin: Point, size: Size, gap: f64) -> Point {
    Point::new(origin.x, origin.y + size.height + gap)
}

/// The hovered element's own box in window coordinates: the child's layout box
/// (`child`, relative to the wrapper) offset by the wrapper's window origin, or
/// the wrapper's box when the child has no layout yet.
///
/// **The child's box, not the wrapper's.** The wrapper is laid out around the
/// child's *margin* box, so reading its size put a tip `margin_left` to the left
/// of a chip that has one, and `margin_bottom` further below one that has that.
fn element_box(wrapper_origin: Point, wrapper_size: Size, child: Option<Rect>) -> Rect {
    match child {
        Some(c) => c + wrapper_origin.to_vec2(),
        None => Rect::from_origin_size(wrapper_origin, wrapper_size),
    }
}

/// A view that shows `tip` under its child after a hover. See the module doc.
pub(crate) struct AnchoredTooltip {
    id: ViewId,
    /// The pending show, keyed so only the latest hover's timer opens a tip.
    hover: Option<TimerToken>,
    overlay: Rc<RefCell<Option<ViewId>>>,
    tip: Rc<dyn Fn() -> AnyView>,
    style: TipStyle,
    /// This view's window origin as of the last layout.
    window_origin: Option<Point>,
}

/// Wrap `child` so hovering it shows `tip` under it.
pub(crate) fn tooltip<V: IntoView + 'static, T: IntoView + 'static>(
    child: V,
    tip: impl Fn() -> T + 'static,
) -> AnchoredTooltip {
    let id = ViewId::new();
    id.set_children(vec![child.into_view()]);
    let overlay = Rc::new(RefCell::new(None));
    AnchoredTooltip {
        id,
        hover: None,
        overlay: overlay.clone(),
        tip: Rc::new(move || tip().into_any()),
        style: Default::default(),
        window_origin: None,
    }
    // The class the workspace root sets the hover delay on.
    .class(TooltipContainerClass)
    .on_cleanup(move || {
        if let Some(overlay_id) = overlay.borrow_mut().take() {
            remove_overlay(overlay_id);
        }
    })
}

/// `.tooltip(…)`, anchored. Import it by name in any file that calls it — the
/// name is floem's too, and only an explicit import shadows the prelude's.
pub(crate) trait TooltipExt {
    fn tooltip<V: IntoView + 'static>(self, tip: impl Fn() -> V + 'static) -> AnchoredTooltip;
}

impl<T: IntoView + 'static> TooltipExt for T {
    fn tooltip<V: IntoView + 'static>(self, tip: impl Fn() -> V + 'static) -> AnchoredTooltip {
        tooltip(self, tip)
    }
}

impl AnchoredTooltip {
    fn dismiss(&mut self) {
        self.hover = None;
        if let Some(id) = self.overlay.borrow_mut().take() {
            remove_overlay(id);
        }
    }
}

impl View for AnchoredTooltip {
    fn id(&self) -> ViewId {
        self.id
    }

    fn debug_name(&self) -> std::borrow::Cow<'static, str> {
        "AnchoredTooltip".into()
    }

    fn update(&mut self, _cx: &mut UpdateCx, state: Box<dyn std::any::Any>) {
        let Ok(token) = state.downcast::<TimerToken>() else {
            return;
        };
        if self.hover != Some(*token) || self.overlay.borrow().is_some() {
            return;
        }
        let Some(origin) = self.window_origin else {
            return;
        };
        let child = self
            .id
            .children()
            .first()
            .and_then(|c| c.get_layout())
            .map(|l| {
                Rect::from_origin_size(
                    (l.location.x as f64, l.location.y as f64),
                    (l.size.width as f64, l.size.height as f64),
                )
            });
        let el = element_box(origin, self.id.get_size().unwrap_or_default(), child);
        let at = anchor_below(el.origin(), el.size(), theme::scaled(GAP));
        let tip = self.tip.clone();
        let overlay_id = add_overlay(at, move |_| tip().style(widgets::tooltip_style));
        *self.overlay.borrow_mut() = Some(overlay_id);
    }

    fn style_pass(&mut self, cx: &mut StyleCx<'_>) {
        self.style.read(cx);
        for child in self.id.children() {
            cx.style_view(child);
        }
    }

    fn event_before_children(&mut self, cx: &mut EventCx, event: &Event) -> EventPropagation {
        match event {
            Event::PointerMove(_) => {
                // Every move restarts the delay, as floem's does — the tip opens
                // once the pointer has rested, not on the way across.
                if self.overlay.borrow().is_none() && !cx.app_state().is_dragging() {
                    let id = self.id;
                    self.hover = Some(exec_after(self.style.delay(), move |token| {
                        id.update_state(token);
                    }));
                }
            }
            Event::PointerLeave
            | Event::PointerDown(_)
            | Event::PointerUp(_)
            | Event::PointerWheel(_)
            | Event::KeyUp(_)
            | Event::KeyDown(_) => self.dismiss(),
            _ => {}
        }
        EventPropagation::Continue
    }

    fn compute_layout(&mut self, cx: &mut ComputeLayoutCx) -> Option<Rect> {
        self.window_origin = Some(cx.window_origin());
        // floem's `default_compute_layout`, which it does not re-export.
        let mut rect: Option<Rect> = None;
        for child in self.id.children() {
            if let Some(child_rect) = cx.compute_view_layout(child) {
                rect = Some(rect.map_or(child_rect, |r| r.union(child_rect)));
            }
        }
        rect
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Under the element, not after the pointer.** floem's own tooltip opened
    /// at the pointer plus 10px (plus its theme's 10px margin), so the same
    /// icon's tip landed somewhere
    /// different on every hover, and a hover on an icon's right half put the
    /// tip past the icon entirely.
    #[test]
    fn a_tip_opens_under_the_element_at_its_left_edge() {
        let at = anchor_below(Point::new(120.0, 8.0), Size::new(26.0, 22.0), 6.0);
        assert_eq!(at, Point::new(120.0, 36.0));
    }

    /// The gap is the only thing between the element's bottom edge and the tip.
    #[test]
    fn the_gap_is_measured_from_the_elements_bottom_edge() {
        let at = anchor_below(Point::new(0.0, 0.0), Size::new(10.0, 40.0), 0.0);
        assert_eq!(at, Point::new(0.0, 40.0));
    }

    /// **The element's own box, not the wrapper's.** The wrapper is laid out
    /// around the child's *margin* box, so a chip with `margin_left(7)` — the
    /// editor's parameter-type chip — put its tip 7px left of the chip it names.
    #[test]
    fn a_childs_margin_is_not_part_of_the_box_the_tip_hangs_from() {
        let child = Rect::from_origin_size((7.0, 0.0), (40.0, 18.0));
        let el = element_box(Point::new(100.0, 50.0), Size::new(47.0, 18.0), Some(child));
        assert_eq!(el, Rect::from_origin_size((107.0, 50.0), (40.0, 18.0)));
    }

    /// A wrapper with no laid-out child falls back to its own box.
    #[test]
    fn without_a_laid_out_child_the_wrappers_box_is_used() {
        let el = element_box(Point::new(3.0, 4.0), Size::new(20.0, 10.0), None);
        assert_eq!(el, Rect::from_origin_size((3.0, 4.0), (20.0, 10.0)));
    }
}

/// **Every `.tooltip(…)` is the anchored one.** The method has floem's name,
/// and floem's `TooltipExt` arrives in every file through `floem::prelude::*`;
/// only an explicit import of [`TooltipExt`] shadows it. A file that calls
/// `.tooltip(…)` without that import therefore still compiles, and quietly
/// opens its tips at the pointer again — which is how one of the fourteen
/// files was missed while this was being written.
#[cfg(test)]
mod tooltip_gate {
    #[test]
    fn every_file_that_calls_tooltip_imports_the_anchored_one() {
        let sources = crate::source_gate::crate_sources();
        let callers: Vec<&(String, String)> = sources
            .iter()
            .filter(|(_, code)| code.contains(".tooltip("))
            .collect();
        assert!(
            callers.len() > 5,
            "found {} files calling `.tooltip(` — this gate is scanning the wrong tree",
            callers.len()
        );
        let missing: Vec<&str> = callers
            .iter()
            .filter(|(_, code)| !code.contains("tooltip::TooltipExt"))
            .map(|(name, _)| name.as_str())
            .collect();
        assert!(
            missing.is_empty(),
            "these files call `.tooltip(` without `use crate::tooltip::TooltipExt;`, so \
             they get floem's pointer-placed tooltip: {missing:?}"
        );
        for (name, code) in &sources {
            assert!(
                !code.contains("floem::views::TooltipExt"),
                "`{name}` imports floem's TooltipExt by name, which shadows the anchored one"
            );
            // floem's free function is the other way round the anchored view.
            assert!(
                !code.contains("views::tooltip("),
                "`{name}` builds floem's own tooltip, which opens at the pointer"
            );
        }
    }
}
