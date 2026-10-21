# `ui/dividers.rs`

`dividers.rs` — the two **panel** dividers: `h_resize_handle` (the schema tree's and the right
panel's edges) and `v_resize_handle` (the editor/results split), plus the `DelayedHover` they
share. Not `window_chrome::resize_zones`, which resizes the *window* and is mounted outside the
app root. Both handles are absolute children positioned from an **effective** (clamped or
floored) edge rather than from the dimension they set: a width the window is too narrow to
honour, or a height persisted under a lower floor, would otherwise leave the handle floating
away from the edge it drags. **Both move their dimension through `nudge`, which publishes only if
the value actually moved.** They used to write straight through
(`dim.update(|w| *w = (*w + d).clamp(lo, hi))`), and a drag is 60–120 `PointerMove`s a second:
past the clamp every one of them computes the same number and floem notifies regardless. `lib.rs`
guards the two *width* republishes it owns, which is what kept the schema tree still, but that is
one consumer's protection and not the publisher's — `editor_h` has no republished twin, its
readers take it raw, and nothing below it held the line. One guard where the value is produced
answers for all three. `hi.max(lo)` stays the caller's job: on a window too small to hold both
panels' minimums the ceiling really is below the floor, and `clamp` panics on an inverted range.
`clamp_tests::a_drag_held_past_the_clamp_publishes_once` counts the notifications — ten publishes
where one is due, and a *moving* drag still publishing every frame, so the guard cannot swallow
the gesture it exists to carry. That test replaced this file's own note saying a notification
count was something no `#[test]` could see; it is (*Floem 0.2 gotchas*, `RwSignal::set` never
dedups). Both capture the pointer on press (`request_active`), and **both
undo the whole gesture inside `on_double_click_stop`** — the double-click eats the second
`PointerUp`, so that handler is the only one that runs and anything the `PointerUp` handler
would have cleared has to be cleared there. Two things qualify, found a year apart in the same
four lines: the drag state (`dragging` + `clear_active`, or the handle stays captured and keeps
resizing on mouse-move with no button down), and the **hover highlight**. The second is the
subtler one — `dim.set(default)` moves the handle out from under a pointer that has *not* moved,
so floem delivers no `PointerLeave` and nothing else ever turns the bar off: the divider
animated to its default still lit, and stayed lit there until the next mouse move happened to
trigger a leave. `hovered.leave()` also voids a pending arm, so a double-click *inside* the
hover delay cannot light the bar afterwards. `dividers::double_click_gate` asserts both calls
are present in **every** double-click handler in the file — the two handles are twins, so every
fix here is two edits, and it was watched failing on each of them in turn.
**The dividers light up on a *rest*, not on a pass.** The bar is an affordance — this edge
drags — and the dividers run the full height and width of the workspace, so answering on
`PointerEnter` lit one every time the pointer crossed from the schema tree to the editor or from
the editor to the results. `RESIZE_HOVER_DELAY` (200ms) is how long the pointer must settle
first — 500 was tried and is too long: it outlasts the gesture, so a pointer that has already
stopped on the divider reads as one the app has not noticed. Dragging is not delayed and the hit
band is never gated on the highlight: the delay is on the hint, never on the control. There is
no cancelling a floem timer, so the arm carries a **sequence number** and checks it with
`try_get_untracked` when it fires — one comparison that retires both the pointer having left
(`Some(newer)`) and the divider having been disposed inside the delay (`None`, since
`exec_after` timers outlive the scope that armed them). That second half is defensive and **not
currently reachable**: `v_resize_handle` is called once from `center`, itself built once in the
workspace shell, and the per-tab `dyn_container`s are its siblings rather than its parents, so
these signals live as long as the app does.
