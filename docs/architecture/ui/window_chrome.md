# `ui/window_chrome.rs`

`ui/window_chrome.rs` — the client-side window decorations: the caption buttons (minimize /
maximize-restore / close), the drag strip, and the eight resize zones. Draws what
`core::window_chrome::Chrome` decides, and contains no `cfg!(target_os = …)` of its own.
`WindowChrome` holds the `WindowId` plus a `maximized` mirror — `is_maximized()` is a query, not
a signal — which the root's `on_resize` re-`sync`s, so the glyph follows *every* route to
maximizing (the button, a drag to the screen edge, Win+Up). Three shapes are load-bearing:
the drag strip is **its own view between the header's clusters**, because Floem dispatches
`PointerDown` to the deepest view first and `on_click_stop` stops `Click`, never `PointerDown` —
a drag handler on the header itself would fire on the connection switcher; drag and
double-click are decided in **one** handler off `PointerDown`'s multi-click `count`, since
starting an OS move loop on press can eat the second press before `DoubleClick` fires; and
close calls `close_window`, **not `quit_app`**, because only the former runs
`WindowHandle::destroy` → `WindowClosed` → `flush_session`, the write that saves open tabs — the
same reason the auto-updater hands over with `close_window` rather than Velopack's
`apply_updates_and_restart` (`app/update.rs`). `quit_app` still ends the process, but from the
app's `WindowClosed` listener, after the flushes: on macOS Floem does not exit once the last
window is gone (*Floem 0.2 gotchas*), so a close that relied on it left the process running.
The eight zones are spread into the stack that wraps the app root as **loose siblings** —
never under a full-window parent, which would swallow every press in the app (see *Floem 0.2
gotchas*, "a full-window sibling ends the pointer walk"); they wrap the root rather than
joining its tuple because that one is at Floem's 16-arity limit.
`over_backdrop` answers the same class of problem — **a modal used to take the
window frame with it.** Every backdrop covers the whole window, the title bar included, so with
a modal up the window could not be dragged, minimized, maximized or closed until it was
dismissed — the resize edges were the only part of the frame that stayed reachable, precisely
because they are mounted outside the root. Its two views are **not** out there with them,
though, and the difference is exact: a resize zone has to be hit before the whole app, while
the band only has to out-paint *the header and the modal layer*. Mounted at the window root it
was also above the overlay menus — and a menu tall enough to take `menu_inset`'s
"bigger than the window" arm pins at y=0, so its first rows sat under this scrim and answered a
press with an OS window drag instead of with the row the pointer was on (hoisted submenus had
the same exposure). It lives inside the root's tuple now, after the modal layer and before
`date_pick_overlay` — which are precisely the three overlays that can open from *inside* a
modal. `over_backdrop` is one draggable strip across the
bar, raised only while `modal_backdrop_up` says a backdrop is on screen, carrying the scrim
colour itself (the modal layer no longer reaches the header) and stopping short of the caption
buttons at `controls_width` so the *header's own* buttons stay the live ones — one set of
close handlers, and the strip it leaves clear is exactly the strip that still works. It is
deliberately not a copy of the header: a press anywhere along it moves the window, because
nothing else up there does anything while a modal is up and a real title bar drags from
anywhere. It stays **under** the resize zones, which are the layer outside the root, so the top
corners still resize rather than drag. `controls_width` is the one place `control_w()` meets `Chrome::own_control_count`, and a
source-scanning test pins that against the buttons `controls` actually builds — a fourth
caption button added without touching the count would leave 46px of title bar dimmed and dead.
It returns **two** siblings, not one, for the reason the zones are eight: the header's
`border_bottom` (`theme::HEADER_BORDER`, inside the 40px box) is one rule across the whole
width, and the band stopping short of the buttons left it dimmed up to them and lit for the
last 138px. The second view dims that sliver, and is `pointer_events(false)` — paint only,
since it lies across the bottom edge of all three buttons and a 1px sibling on top of a control
still ends the walk. That flag's usual objection (it takes the subtree with it) costs nothing
on a view with no children.
**A press on the chrome hands the keyboard back**, deferred, after a title-bar drag and after a
caption press (`give_the_keyboard_back` → `widgets::hand_keyboard_back(None)`). Floem clears
focus at the top of *every* `PointerDown` dispatch and only a `keyboard_navigable` view re-takes
it during the walk that follows; the band and the caption buttons are neither navigable nor
inside anything that is, so a press on them left focus at `None`. Inside the app that is
invisible, since the window root's own listeners still see the key — over a **modal** it is not,
because the modal's `focus_root` requests focus once on build and nothing re-requests it, so the
panel went keyboard-dead: its ✕ and Cancel still worked, Tab recovered through the root's ring
backstop, and **Escape had no equivalent backstop**, so the one keyboard route out of the modal
was gone until the panel was clicked. It has one now — the window root forwards an unanswered
Escape to the innermost focus root (GitHub #15; *A key event goes to the focused view* in
*Floem 0.2 gotchas*) — but that backstop answers Escape alone, so the hand-back is still what
gives the panel's other keys a focused view to reach. `None` is the argument because that call already resolves
the innermost mounted focus root and falls back to the workspace's keyboard home, so the chrome
needs to know neither. Deferred because the clear happens *before* the listeners run, so a
request inside the same dispatch would be undone by it. **Close is excluded** — it is the one
press after which there is no window to hand a keyboard back to.
**The hand-back is a wrapper now, `chrome_press(f)`, and that shape is the fix rather than a
tidy-up.** It was a step each handler had to remember, and two of the three remembered it in the
wrong place: `draggable` and `zone` both opened with `if p.button.is_primary()` and called
`give_the_keyboard_back()` *inside* that branch — but `on_event_stop` consumes the press whatever
button sent it, and an early `return` does not change that (floem's `on_event_stop` returns
`Stop` unconditionally). So the standard Windows caption gesture, right-clicking the title bar,
took the keyboard and returned nothing, and over an open modal that meant Escape stopped closing
it until the panel itself was clicked; the same hole sat on all six resize edges and both 14×14
corners. Wrapping the handler leaves no branch for the call to end up inside, and it is
idempotent and deferred, so paying it on a press the handler does nothing else with costs
nothing. `control_button` gained a `PointerDown` leg of its own through the same wrapper,
`on_event_cont` so the press still becomes the `Click` a primary one does: `Click` is
primary-only, so a right-press on a caption button reached nothing at all while still clearing
focus. That leg is unconditional and separate from the `keeps_the_window` exclusion, which is
about the moment *after* `on_press()` has torn the window down.
**The eight resize zones owe it too, and that is the instance it bit on.** A zone is a bare
`empty()` with an `on_event_stop`, so it is neither navigable nor inside anything that is and it
ends the walk — dragging a window edge therefore left focus `None`. With a modal open Escape
stopped closing it, recoverable only with Tab through the workspace root's ring backstop, which
is why it read as intermittent rather than as a dead modal; with no modal the next keystroke
after a resize was dropped instead of landing at the caret.
`window_chrome::tests::every_chrome_press_hands_the_keyboard_back` covers both spellings — a
`PointerDown` handler must go through `chrome_press(`, a `Click` handler must call
`give_the_keyboard_back()` itself — and floors at three handlers overall.
**The two needles are not interchangeable, and that is the load-bearing part of the gate.** It
used to accept the call anywhere in a handler's span, which both `PointerDown` handlers satisfied
with the call sitting inside `if p.button.is_primary()`: the gate was green for as long as the
bug existed. Asking for the wrapper is a question a call in a branch cannot answer.
**And a maximized window's zones are hidden, because it has no frame to grab.** The `NorthEast`
zone is a 14×14 square pinned to the extreme top-right pixel — the Fitts's-law gesture for
closing a maximized window — and `North` is the top 5px of all three caption buttons; both
swallowed the press and did nothing with it, `drag_resize_window` sending a sizing message a
maximized window has no response to. The flag that answers this is the `maximized` mirror in the
same struct, kept true to the OS by `sync` on every route into maximization and read by nothing
but the caption glyph until now, and it is read *inside* the zone's style closure so the window
is followed with no rebuild. **`hide()` rather than `pointer_events(false)`**, for the reason
`resize_zones`' own doc already spells out: a hidden view is not laid out, so the walk skips it
and reaches the button underneath, where a `pointer_events(false)` sibling would still have to be
rejected by `should_send` and would take its subtree with it. Hidden and **not dropped** —
`a_maximized_window_hides_its_resize_zones` and `every_edge_still_has_a_zone` are the two halves,
the second because dropping them leaves a window winit has already stripped `WS_SIZEBOX` from.
