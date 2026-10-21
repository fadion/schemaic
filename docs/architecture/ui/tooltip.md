# `ui/tooltip.rs`

`tooltip.rs` — the app's `.tooltip(…)`: floem 0.2's `Tooltip`, **anchored to the element
instead of the pointer**. floem opens its tip at the pointer plus 10px (`window_origin +
hover.pos + 10/scale` in its `update`, `floem-0.2.0/src/views/tooltip.rs:83-87`), and its
default theme adds as much again: `TooltipClass` carries a `margin(10.0)`
(`floem-0.2.0/src/theme.rs:261-272`) that the app's chrome, merged on top, never overrode — so
**the pointer plus 20px**, wherever on the element the pointer happened to rest when the delay
ran out. Where a tip landed was a fact about the hover rather than the control: on a 16px icon,
resting on its right half started the tip past the icon, far enough out to read as the next
one's. That was reported as every tooltip in the app opening shifted to the right of its icon —
and nothing in the repository or in floem had changed; it was floem's rule all along.
`anchor_below` (pure, pinned by `a_tip_opens_under_the_element_at_its_left_edge` and
`the_gap_is_measured_from_the_elements_bottom_edge`) opens it under the element's own box,
`theme::scaled(6.0)` below, **aligned to its left edge and not centred**: a tip is usually wider
than the icon it names, and a centred one would reach left over the neighbour as far as it
reaches right.
**The element's own box is the child's, not the wrapper's** — `element_box`, pure, pinned by
`a_childs_margin_is_not_part_of_the_box_the_tip_hangs_from` and
`without_a_laid_out_child_the_wrappers_box_is_used`. The wrapper is laid out around its child's
*margin* box, so hanging the tip from the wrapper put the editor's parameter-type chip's tip 7px
left of the chip it names — the chip carries `margin_left(theme::scaled(7.0))`, in
`editor_pane.rs`. `update` takes the first child's layout rect offset by the wrapper's window
origin, and falls back to the wrapper's own box only while the child has no layout.
`AnchoredTooltip` is floem's view copied with that one decision changed and the rest kept on
purpose — the hover delay (it carries `TooltipContainerClass` and reads floem's `Delay` through
its own `prop_extractor!`, so the root's 300 ms rule still sets it), the events that dismiss it
(leave, press, release, wheel, key down or up), no tip while a drag is held, and the overlay
removed on cleanup. The drag rule is the nearest the public API reaches, not floem's own:
`is_dragging()` goes false the moment a drag is released, whereas floem's view waited for its
`pub(crate)` `dragging` field (`floem-0.2.0/src/app_state.rs:36`) to clear, and so also held
tips off through a released drag's animation. It inlines floem's `default_compute_layout`,
which 0.2 does not re-export. Keeping the tip inside the window's **right** edge is still floem's
`OverlayView`, whose paint pulls an overflowing overlay back in by the overflow (to 5px inside).
**The bottom edge is the app's**, because that pull-back is a shift, not a flip: once the
status-bar icons had tooltips (GitHub #11), every one of them was pulled straight back up over
the icon it names. `tip_top(el, tip_h, window_h, gap)` answers under the element where the tip
fits inside `window_h - EDGE`, and otherwise **above** it, `(el.y0 - gap - tip_h).max(0)` — a
window too short for either side pins it to the top rather than over the element, and an
unmeasured window (`window_h` of 0) keeps it below. Pinned by
`a_tip_with_room_below_stays_under_the_element`,
`a_tip_with_no_room_below_opens_above_the_element`,
`an_unmeasured_window_keeps_the_tip_under_the_element` and
`a_window_too_short_for_either_side_pins_the_tip_to_the_top`. **The answer needs the tip's
height, which only layout knows**, and floem 0.2 cannot say "minus my own height" in style (a
`translate_y` percent is not of the view's size — *Floem 0.2 gotchas*). So `update` still opens
the overlay at `anchor_below`, and the tip carries a `shift: RwSignal<Option<f64>>` in a per-tip
`Scope` (`tip_scope`, disposed deferred by `dispose_deferred` on dismiss, on a re-open and on
cleanup). While it is `None` — the one frame before layout reports a size — the tip is painted
off-screen with `translate_x(-100_000)`; its `on_resize` then sets `tip_shift` from the measured
height and `widgets::window_size()`, and the style applies it as `translate_y`. **`tip_shift`
undoes floem's pull-back as well as making the move**: `OverlayView::paint` offsets the overlay
*before* the tip's own transform is applied, so the translate starts from where floem painted
the tip, not from where it was opened. `floem_pull_back` mirrors that clamp and `EDGE` (5.0) is
floem's margin, both copied from `floem-0.2.0/src/window_handle.rs:1561-1576` — a floem upgrade
that changes that clamp has to change them with it, and no test reads floem to notice.
**The chrome is `widgets::tooltip_style`, applied to the tip directly.** floem computed the tip's
style from the global `TooltipClass` rule off the tooltip view's style context, because the tip
is an overlay — a child of the window, not of the app root — and sees none of the root's class
rules. Applying the style itself is the same chrome without the indirection, **less what floem's
theme had put in `TooltipClass` underneath it**: the `margin(10.0)` above, half the reported
shift, which the tip no longer carries. With nothing reading the class, the root stylesheet's
`.class(TooltipClass, tooltip_style)` rule was dead, and it is gone. The theme's
`box_shadow_h_offset(2.0)` is *not* part of that difference, whatever a reading of the theme
suggests: the shadow is one property (`BoxShadowProp`, `floem-0.2.0/src/style.rs:1649`), and
`tooltip_style`'s shadow builders, handed a fresh `Style` by `.class`, had already replaced it
whole with a zero horizontal offset.
`TooltipContainerClass`'s `Delay` rule stays. The chrome is still a *later* style layer than
anything the tip closure set, which is what `widgets::tip_when`'s `hide()` rests on (*UI
conventions*).
**It is reached by floem's own method name, and that is the hazard.** floem's `TooltipExt`
arrives in every file through `floem::prelude::*`, and only an explicit
`use crate::tooltip::TooltipExt;` shadows it — so a file that calls `.tooltip(…)` without the
import still compiles and quietly gets the pointer-placed tip. `erd_view.rs` was that file while
this was being written. `tooltip_gate::every_file_that_calls_tooltip_imports_the_anchored_one`
scans `source_gate::crate_sources()` and fails any file that calls `.tooltip(` without naming
`tooltip::TooltipExt`, that imports `floem::views::TooltipExt` by name, or that contains
`views::tooltip(` — floem's free function, the other way round the anchored view. It has a floor
of more than five callers so a scan of the wrong tree cannot pass empty. The module is
crate-private, so a `.tooltip(` written in `schemaic-app` — also in that corpus, and with none
today — fails the gate with no import that could satisfy it.
