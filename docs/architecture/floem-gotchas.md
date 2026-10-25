# Floem 0.2 gotchas (learned the hard way)

- **`floem` is vendored and patched, so a Floem bump is a re-vendor, not a version edit.**
  `vendor/floem/` is the published 0.2.0 crate — its `src/`, the normalised manifest crates.io
  serves, `LICENSE` and `README` — used through `[patch.crates-io] floem = { path = "vendor/floem" }`
  in the root `Cargo.toml`; the `floem = { version = "0.2.0", … }` line above it is unchanged and
  still carries the feature list. `vendor/floem/PATCHES.md` is the record: every change against
  upstream, each marked in the source with a `schemaic patch (PATCHES.md)` comment so one grep finds
  them all, and the procedure for a new release — vendor it the same way, re-apply the list, drop
  what upstream has absorbed, and delete the directory and the `[patch]` entry once the list is
  empty. The first entry is `WindowConfig::app_id`, which Floem 0.2 has no counterpart for; why the
  app needs it is under `app::update`'s Linux packaging. The second moves the gutter's
  current-line band fill in `EditorGutterView::paint` ahead of the number loop: upstream filled
  every caret's band on reaching the *primary* caret's line, over a secondary caret's number
  already drawn above it, and filled none once the primary's line scrolled off. The third gives
  the whitespace marks' `Attrs` in `EditorView::paint_text` the line's line height
  (`LineHeightValue::Px(style.line_height(edid, line))`, as `Editor::new_text_layout` gives the line
  itself): the `·`/`→` are laid out separately from the line, and without it they took the font's
  natural height and rode at the top of any line taller than the font. The fourth lets a captured
  view hear a **non-primary** release wherever it lands. With a view active (`request_active`)
  every pointer event is routed to it with `directed = true`; `unconditional_view_event`'s
  `PointerUp` arm runs a primary release's listeners unconditionally, but any other button's fell
  through to the generic listener block, which is gated on the view's own rect. Moves are not, so
  a middle-drag pan of the ER diagram released outside the canvas — elsewhere in the app or outside
  the window — never ended, and moving back went on panning with no button held (GitHub #13); a
  left-drag ended correctly. `should_run` there is now also true for a directed `PointerUp` of a
  non-primary button; undirected dispatch is unchanged. The OS does deliver that release — winit
  captures the mouse for the middle button too (0.30.13, Windows `WM_MBUTTONDOWN` →
  `capture_mouse`) — so the drop was Floem's alone. No test covers it. The fifth adds
  `CursorStyle::Grabbing`, mapped to winit's `CursorIcon::Grabbing` in `WindowHandle::set_cursor`:
  Floem 0.2 names fifteen cursors and no grab cursor, and the ER diagram wears the closed hand
  while its canvas is panned (GitHub #14). Only that one variant — winit's open-hand `Grab` and
  the rest are not added. No test covers it either. The sixth makes `ViewId::apply_event` `pub`,
  its body unchanged: a `KeyDown` goes to the focused view and, unanswered, to the window root's
  own listeners and never up through ancestors, so with a modal mounted and focus on nothing or
  behind it the modal's Escape listener never heard the key (GitHub #15). The window root now
  forwards such an Escape to the innermost focus root's listeners, and running a view's listeners
  for an event is exactly what `apply_event` does — see *A key event goes to the focused view*,
  below. The seventh makes the editor's pointer cursor a signal, `Editor::pointer_cursor`
  (`CursorStyle::Text` by default), which `editor_content`'s style reads in place of the constant
  `.cursor(CursorStyle::Text)` it set in the text view's own style — a style no ancestor class
  outranks, so nothing outside could change it; the query editor wears an arrow over a Ctrl+hover
  underline (`editor_pane.rs`). It also has `StyleCx::style_view` hand a hovered view's new cursor
  over at restyle time when its old one is the cursor showing — for any view, not only the
  editor's. Floem resolves the cursor only on a pointer move, so without it a Ctrl pressed with
  the mouse still kept the I-beam until the mouse moved. No test covers it. The eighth stops a view
  removed earlier in an update batch from hearing the rest of it. `process_update_messages` takes
  the window's whole queue and delivers each `UpdateMessage::State` through `id.view()`, but
  `ViewId::remove` drops the id from `view_ids` and never from `views`, so a removed view still
  resolves and still runs `update` — and a `dyn_container`'s `swap_val` removes its old child and
  then disposes that child's scope, both synchronously, so a nested `dyn_container` in the child
  with its own message in the same batch rebuilt against disposed signals. The patch adds
  `ViewId::is_registered` (`pub(crate)`, membership of `view_ids`) and drops a `State` message whose
  id fails it, on the immediate and the deferred queue alike. The crash it fixed, and what it leaves uncovered, are under *A view must not
  subscribe to a signal that changes as part of unmounting it*, below. No test covers it. **Three measures
  keep upstream's code out of the workspace's checks, and each looks removable**: `exclude = ["vendor"]` in the root
  manifest, since a path dependency inside the workspace directory would otherwise join it as a
  member; `vendor/floem/rustfmt.toml` with `disable_all_formatting = true`, since `cargo fmt --all`
  formats local path dependencies too and CI's `--check` would fail on code that is not ours; and a
  `[lints]` table appended to the vendored manifest allowing every rust and clippy lint, since Cargo
  caps a registry crate's warnings but not a path dependency's. The last is in the manifest rather
  than the source so that `src/` stays upstream's byte for byte, apart from what `PATCHES.md` lists.
  **`vendor/tiberius/` (SQL Server's driver) is vendored on the same terms** and under the same
  three measures, so `exclude = ["vendor"]` now covers two crates; its patches are under `db::mssql`.
- **floem's file dialogs run on a thread with no runtime, so the `rfd` backend must need none.**
  `file_action::open_file`/`save_as` — every open and save dialog in the app — run
  `rfd::FileDialog` on a bare `std::thread::spawn`. floem's `rfd-tokio` feature puts rfd → `ashpd`
  → zbus on zbus's `tokio` backend, which calls `tokio::task::spawn` and panics on that thread
  ("there is no reactor running"), so on Linux **no dialog ever opened**: the thread died before
  any portal request, the `create_ext_action` callback never fired, and nothing was reported.
  Windows and macOS use native dialogs with no D-Bus and never saw it. The feature is
  `rfd-async-std`, which despite the name pulls in no async-std — ashpd's `async-std` is
  `zbus/async-io` plus `async-fs`/`async-net` — and zbus's `async-io` executor needs no runtime.
  Cargo unifies features, so any dependency enabling `zbus/tokio` brings the bug back; that is why
  the guard, `conn::secrets`'s `zbus_is_not_on_its_tokio_backend`, reads `Cargo.lock` rather than a
  manifest (the keyring there is the other zbus user — see `schemaic-conn` — and `app::notify`'s
  `notify-rust` a third, on zbus 5 beside the other two's zbus 4).
- **Floem does not exit on macOS when the last window closes.** `AppHandle::close_window` calls
  `event_loop.exit()` on an empty window list under `#[cfg(not(target_os = "macos"))]` only, so on
  a Mac the process outlived its one window and *Restart to update* waited on it. The app's
  `WindowClosed` listener calls `floem::quit_app()` after its flushes (*app/main.rs*); don't drop
  it as redundant because the platform you test on exits without it.
- **On Windows, the move that maximizing causes arrives before the window knows it is maximized.**
  `WM_WINDOWPOSCHANGED` — Floem's `WindowMoved` — is reported before the `WM_SIZE` in which winit
  sets its maximized flag, so `is_maximized()` read inside the move handler says *normal* and the
  screen's bounds get recorded as the window's own. `WindowPlaceTracker` (*app/main.rs*) samples
  on a 300 ms trailing settle after the last move or resize instead, and once more at the close;
  don't move the read back into the event because it looks more immediate
  (`core/window_place.rs`'s entry).
- **One `on_scroll` per scroll** — setting it twice clobbers. `autohide` sets its own; a scroll
  needing custom `on_scroll` must inline `autohide_state()` (results grid + AI convo).
  `autohide_tracking` is `autohide` plus publishing the visible rect into a signal, from the same
  handler, for a list that draws against its scroll position (`banded_list`'s sticky header) — one
  wrapper rather than a second `on_scroll` chained after `autohide`'s, which would replace it.
- **No `opacity` property.** Fade via color alpha (`multiply_alpha`) + `.transition_*`. Toggle
  visibility with `.hide()`/`.flex()` (display none/flex).
- **Inherited color doesn't animate to a child.** A parent's `.transition_color()` won't fade a
  child svg's `currentColor` — set color + transition on the element itself.
- **Style precedence**: a view's own (direct) style beats ancestor class styles; nearest ancestor
  class wins. Nest class overrides accordingly (dropdown popup restyle nests under `ListClass`).
- **`DoubleClick` consumes the second `PointerUp`** — clear drag/press state in the double-click
  handler too, not only in `PointerUp`.
- **`.tooltip()` wraps the view; it does not decorate it.** `tooltip(child, tip)` builds a *new*
  view and makes the old one its child, so everything chained after it lands on the wrapper and
  everything before it stays on the inner view. Split a control across that line and it comes apart:
  the style — background, border, padding, height — paints the wrapper while the listeners sit on a
  content-sized box inside it. **The results strip shipped exactly that**: chip styled after
  `.tooltip()`, click handlers before, so the chip you saw was the wrapper and the chip that
  answered a click was the text, and clicking the padding did nothing. `activity_panel`'s clock
  button was bitten from the other side — `on_move`/`on_resize` on the inner container reported a
  bare 16px glyph box, and its menu hung 3px under that instead of under the padded control.
  So: **style and listeners on the same view, and put the tooltip on the innermost thing that
  should carry it** (the query strip and the results strip both tooltip their *label*), or chain
  `.style()` before `.tooltip()` when the wrapper is genuinely what you want to place.
- **`img()` fails silently, and it does not honour its own `ObjectFit`.** Bytes it cannot decode
  draw *nothing* — no placeholder, no error, an empty box — so what a caller can name and what the
  renderer can show have to be kept as one list: the binary-cell panel's `blob::BlobKind::is_image`
  is `!= Opaque`, and the two formats beyond floem's `default-image-formats` that it claims (GIF,
  BMP) are enabled as `image-gif`/`image-bmp` on the floem dependency in the root `Cargo.toml`,
  with `the_renderable_kinds_are_exactly_the_ones_floem_is_built_to_decode` pinning the half a test
  can reach. And it paints into whatever box layout hands it regardless of fit, which is why that
  panel shows an image at natural size in a scroll: fitting it would mean scaling by hand from the
  dimensions. **It also decodes eagerly, at construction, to RGBA** — so what an `img()` costs is
  the header's declared width x height x 4, and a small blob can be an enormous allocation. That is
  why the binary-cell panel measures before it builds one (`blob::preview_verdict`), and why the
  view is built once and hidden rather than rebuilt per pane switch.
  **And that decode cannot be moved off the UI thread while floem 0.2.0 is the dependency**, which
  is why the binary panel's preview still blocks the thread that opens it. `img(impl Fn() ->
  Vec<u8>)` is the only public entry point and it runs `image::load_from_memory` and `into_rgba8`
  inline on the calling thread; the variant that would take an already-decoded image,
  `img_dynamic(impl Fn() -> peniko::Image)`, is `pub(crate)` and used only by floem's own inspector,
  and the SHA-256 in `Img::update` is floem's too, over the decoded RGBA. So what is left is a
  bounded, one-off, panel-open cost — `PREVIEW_EDGE_CAP` holds either edge to 4096, so the decode it
  admits is at most ~16.8 megapixels of RGBA — and closing it needs an API floem does not export,
  upstream's or one a `PATCHES.md` entry adds to the vendored copy (above), rather than a change
  in the panel.
  **And the RAM is not the fatal limit — the texture is.** floem's images live in the same atlas as
  its glyphs, and that atlas grows to `2 × max(width, height)` with no clamp of its own; wgpu's
  default `max_texture_dimension_2d` is 8192, and no floem crate installs an `on_uncaptured_error`
  handler, so a `create_texture` that fails validation takes the **process** down rather than the
  image. So the number that has to be inside the GPU's limit is a *dimension*, not an area, and an
  ordinary 6000 × 4000 photograph — 24 megapixels, well inside any sane RAM budget — is 12000 past
  it. `blob::PREVIEW_EDGE_CAP` (4096) is that second gate, and it is why the panel checks two caps
  and not one.
- **A view must not subscribe to a signal that changes as part of unmounting it** — the change and
  the teardown land in the same update pass, and whichever order they run in, a nested
  `dyn_container` inside the doomed view rebuilds a child whose style/effect closures then read
  signals whose scope has already been disposed. The symptom is floem's own
  `Option::unwrap()` on a `None` in `floem_reactive::read`, with the panic pointing at whatever
  signal the *new* child happened to read first — never at the subscription that caused it.
  The results strip shipped this: the grid's "is this result pinned?" flag was a memo over
  *whichever panel is shown*, so running a query while a result was pinned changed it for the grid
  being torn down, re-ran that grid's edit-model effect, rebuilt the toolbar's `ai_menu` and read a
  disposed `GridState` signal. The fix is not to defer or guard the read — it is to scope the
  subscription to a fact about the view itself (`Tab::panel_frozen_memo(id)`, one panel), and to
  have a departing entity keep its last answer rather than report a "gone" value on the way out.
  The neighbouring rule (**show and hide with `.hide()`, never a rebuild**) is a defence against the
  same hazard: a view that is never rebuilt has no new child to read a dead signal.
  **One ordering of that race was Floem's own, and vendored patch 8 now closes it at delivery.** A
  `dyn_container` rebuilds through a queued `UpdateMessage::State`, and `process_update_messages`
  used to deliver one to a view removed earlier in the same batch: `ViewId::remove` leaves the
  entry in `views`, so `id.view()` still found it. A connection switch hit exactly that,
  intermittently — twice in seven tries in the sandboxed app. The switch queued `results_area`'s
  swap (keyed on the active tab); clearing the schema tree (`db_nodes.set`) re-ran the grid's
  key-map effect, which bumped `key_gen` for a result with a source table and so queued a rebuild of
  the grid body's `dyn_container` (`body_rebuild_key`). When both landed in one batch the swap
  removed the grid and disposed `GridState`'s scope first, the body's message was delivered anyway,
  and its closure's `gs.rs.get_untracked()` took the same unwrap. A source-less `SELECT 1` has no
  key map to change and could not trigger it. A `State` message whose id fails
  `ViewId::is_registered` is now dropped. **That narrows the hazard; it does not retire this rule,
  nor `Tab::panel_frozen_memo`.** The guard covers one route — a queued `State` message to a
  removed id, on the immediate queue and on the deferred one (`update_state_deferred`, delivered by
  `process_deferred_update_messages`). An effect or style closure that runs without a message is
  untouched, and `views` still keeps every removed view's entry, so `id.view()` on one still
  answers. Whether the patch alone would have stopped the `ai_menu` crash above was not
  tested; the memo answers a question about the grid's own panel either way, and stays.
- **A view's own `event_before_children` runs *before* its listeners, and a processed event stops
  there** (`context.rs::unconditional_view_event`) — so `on_event(KeyDown, …)` on a built-in view
  never sees a key that view handles itself. **`floem::views::text_input` handles Escape by calling
  `app_state.clear_focus()`**, which is how the grid's inline cell editor, when it was a
  `text_input`, came to leave the keyboard on *nothing*: its own Escape arm was dead code, and after
  a press the arrows, Enter and Ctrl+Enter all did nothing until a cell was clicked. There is no
  `disable_default_event` escape hatch worth using either — it is per *listener kind*, so
  suppressing KeyDown suppresses typing. What the host did hear was `FocusLost`, indistinguishable
  from a click on anything else, so the pointer had to guess which it was — and an Escape pressed
  with the pointer parked outside the grid still left the keyboard on nothing. `edit_field` does not
  have the problem — it is a `text_editor` with a command hook, so its `on_escape` is real — and the
  cell editor is one now (no `text_input` is left in the app; *Data grid* has why it moved), so its
  Escape is a key handler that hands the keyboard back unconditionally. **The pointer still decides
  for a real blur**, because floem exposes no way to ask where the focus went (`AppState::focus` is
  private, and by the time a listener runs `focus_changed` has already assigned the new one):
  `grid::reclaim_keyboard` takes the keyboard back for a click-away only when the pointer is over
  the grid — another cell, or chrome that takes no focus — and leaves it with the something-else a
  click elsewhere landed on.
- **An overlay that would overflow the window is nudged back in *paint only*.**
  `OverlayView::paint` (`floem-0.2.0/src/window_handle.rs`) does
  `cx.offset((-x, -y))` when `window_origin + size` passes `parent_size - 5` on either axis — and
  nothing corresponding happens to its layout, so hit-testing keeps answering at the *un-nudged*
  position. Everything inside such an overlay is drawn in one place and clicked in another, by
  however much it overflowed.
  - This reaches the app through **`floem::views::dropdown::Dropdown`**, whose popup is an overlay
    opened at `box_origin + box_height`: a dropdown low enough that its list runs past the window's
    bottom paints its rows shifted up while the pointer still lands on the row *below* the one you
    see. Every dropdown in the app could hit it; the interface-scale picker did first, because it
    was the last row of the last group of the tallest modal — and only at 150% (130% now), the one
    scale where that modal grows enough to push the popup off the bottom but not enough for its body
    to scroll instead. Two rounds of plausible fixes (rebuilding the control, chasing a stale
    `window_origin`) changed nothing, because nothing on our side was wrong.
  - **So nothing in this app uses `floem::views::dropdown::Dropdown` any more** — see
    **No floem `Dropdown`** under *Architecture invariants*. There is no bounded fix inside floem's
    control (a `max_height` cap would have to be ≤ 5px to guarantee no nudge), so the class was
    removed rather than the instances: every `<select>`-shaped control drops a `widgets::menu_panel`
    through `settings::in_ring_picker`, and that panel is placed by the app's own arithmetic
    (`menu_panel_height` predicts, `box_menu_inset` flips) and laid out where it is drawn. (The
    QUERY toolbar's database selector and the header's connection switcher are `<select>`-shaped
    and are *not* that panel — `overlays.rs` hangs them by hand — but they are no floem overlay
    either, so they too are laid out where they are drawn; what they lack is the flip, *Popup
    menus*.) The interface scale stays a segmented control (`settings::scale_picker`) because it never needed a
    popup, not because dropdowns are still unsafe.
  - **The nudge is a shift, not a flip, so an overlay hung under something at the window's bottom
    is pulled back up over it.** That put every status-bar icon's tooltip on the icon it names
    (GitHub #11). The fix cannot live in style: floem 0.2's `translate_y(pct(…))` is **not** a
    percentage of the view's own size — the `PxPct::Pct` arms of the `translate_x`/`translate_y`
    match in `vendor/floem/src/context.rs` turn `Pct(p)` into `p / 100` *pixels* — so "up by my own height" is inexpressible, and the height has to be measured (`on_resize`)
    and the translate computed. A translate also lands *after* the nudge, so it must undo it too.
    `tooltip.rs` (`tip_top`, `tip_shift`) is the worked example.
- **Enter and Space arrive at a focused view as a synthesised `Click`, *before* its KeyDown
  listeners run — and `Stop` cannot call it off.** `EventCx::unconditional_view_event`
  (`floem-0.2.0/src/context.rs`) applies `EventListener::Click` on the focused view for any
  `is_keyboard_trigger()` key (Enter, NumpadEnter, Space — matched on the **physical** key), and it
  does so in the `match &event` block *above* the generic listener dispatch. Worse, that dispatch is
  `handlers.iter().fold(false, |handled, h| handled | h(&event).is_processed())` — every handler
  runs and only then is the result reported, so one returning `EventPropagation::Stop` neither skips
  its siblings nor unwinds the click that already fired.
  - So a control with both an `on_click` and an `on_event(KeyDown, …)` gets **two** calls for one
    Enter. That is harmless when the action is idempotent (`open.set(true)`, which is what the old
    floem-`Dropdown` wrapper did and why nobody noticed) and a live bug the moment it **toggles**:
    `settings::in_ring_picker` opens through `widgets::open_picker`, which closes a menu already
    standing, so claiming Enter there would open and shut the menu in one keystroke, which looks
    exactly like a control that ignores the keyboard. It claims only Up/Down, which are not
    keyboard triggers and so have no click to collide with, and lets the synthesised click carry
    Enter and Space.
  - **Going the other way — dropping a KeyDown arm and letting the synthesised Click be the whole
    keyboard answer — is not a valid simplification, and it reads like the shorter fix.**
    `Event::is_keyboard_trigger` has **no modifier term**, so it fires for `Ctrl+Enter` and
    `Ctrl+Space` too: it presses exactly the chords `widgets::presses` exists to refuse, and
    `Ctrl+Enter` is the grid's Commit. The two must therefore be kept on **two `ViewId`s** rather
    than collapsed onto one, which is what `widgets::key_pressable` and `in_ring_button` both do.
- **A child that overflows *left* or *up* of its parent is painted but never hit-tested.** Floem
  hit-tests a subtree through `EventCx::should_send` (`floem-0.2.0/src/context.rs`), which builds the
  rect it tests as `id.layout_rect().with_origin(layout.location)` — it takes the **size** of the
  union of the view and its children (window coords, `compute_view_layout`) but re-anchors it at the
  view's **own** origin. So an overflowing child grows the parent's hit area rightward and downward
  only: a child placed at a negative offset is inside the union that produced the size, and outside
  the rectangle that gets tested. It is `continue`d past, and the pointer reaches whatever sits
  underneath instead. Nothing about this is visible in paint, which doesn't consult `should_send` —
  the thing renders perfectly and simply doesn't answer the mouse, hover states included, so the
  failure reads as "this view has no event handling" rather than as a placement bug.
  **The fix is to hoist the overflowing view to a layer whose own box already contains it**, not to
  stop flipping — off-screen is not better than unclickable. Menu submenus do exactly that
  (`widgets::submenu_layer`, last in the root stack: see *Popup menus*), which is the pattern to
  copy for anything else that has to position a child at a negative inset. A full-window layer is
  the wrong shape for it — that swallows every click meant for the app, per the overlay rule below;
  the layer has to shrink-wrap the thing it hoists.
- **Absolute overlays** (placeholders, action bars, badges) intercept clicks — every one that covers
  something clickable needs `.pointer_events(|| false)` so clicks fall through. Out of flow is not
  out of the hit test: Floem walks a view's children back-to-front looking for a pointer target and
  stops at the first one whose bounds contain the point, whether or not that child handles the event
  (`floem-0.2.0/src/context.rs`, `unconditional_view_event`: the loop is
  `for child in children.into_iter().rev()` and ends `if event.is_pointer() { break }`), and an
  overlay is typically the last child. A view that has opted out is `continue`d past instead —
  `EventCx::should_send` returns false on the flag and the walk reaches the sibling underneath
  (`Decorators::pointer_events`, `floem-0.2.0/src/views/decorator.rs:175`); Floem's own inspector
  marks its overlays the same way. It bites even when the overlay renders **nothing**, because an
  empty box still has bounds: the schema tree's size badge, once it became a panel-wide absolute
  box, swallowed every click on a table row's chevron and name with the size column switched *off*
  as well — which is what made the breakage look unrelated to the feature that introduced it.
  **The opt-out is not inheritable, so it is no help to an overlay that has its own hit targets.**
  `should_send` is asked about the *child*, and answering no `continue`s past that whole subtree —
  the flagged view's descendants are never offered the event, however much they want it. So an
  overlay that both covers the window and contains something clickable cannot be one view: it has
  to be spread as loose siblings, each small enough to be skipped on a miss. The window's eight
  resize zones (`ui::window_chrome::resize_zones`) are exactly that shape, and they were a
  full-window wrapper first — one holding no handler at all, on the theory that a view returning
  `Continue` passes the press on. It does not; the walk had already ended at it, and **nothing in
  the app was clickable**.
- **An absolute child resolves against its direct parent, and that is a lever, not just a hazard.**
  Floem does not look for a nearest *positioned* ancestor the way CSS does: an
  `absolute().inset(0)` child fills whichever box its parent happens to be. Every modal backdrop in
  the app is written that way, so what they cover is decided entirely by the wrapper they are
  mounted in — and `modals::modal_layer` uses that to hold all of them in **one modal layer inset
  `HEADER_H` from the top** (*all*: the membership test is "does this view paint
  `modal_backdrop()`", which is why the Find Anywhere palette is in there beside the DDL editors),
  which is what leaves the title bar free for
  `window_chrome::over_backdrop`'s drag band. The alternative, hoisting the band above a
  full-window layer, would have put it on top of whatever the modal had in that strip: a 620px
  panel centred in a 700px window reaches into the top 40px, and its close × would have been
  answering to the window's caption buttons. **Which is the same trap the band then fell into one
  layer up**: mounted outside the root it was above the *overlay* menus too, and the ones that open
  from inside a modal reach into that strip whenever they are tall enough to pin at y=0. Painting
  later is also being hit first, so "what must this out-paint?" and "what must this be hit before?"
  are one question in Floem, and the band's answer to both is the header and the backdrop. The same lever has a matching trap — a wrapper that
  is *zero-sized* (the out-of-flow state every one of these overlays takes when closed) gives its
  absolute children a zero box, so a modal left out of the layer's `modal_backdrop_up` predicate
  does not open half-right, it does not open at all. That is deliberate: the loud failure is the
  guard that keeps the layer and the predicate in step — and it is only loud to whoever *opens*
  that modal, which is not the person adding it. **It also only covers one direction**, which is why
  the claim no longer rests on it alone: a surface that paints a backdrop and is mounted *outside*
  the layer resolves its `inset(0)` against the root, looks perfect, and silently restores the exact
  bug the layer was written to fix — the backdrop over the title bar, the drag band never rising, and
  a window that cannot be moved, minimised or closed with nothing on screen saying why. Three of the
  layer's members are loose children with no group wrapper to remind anyone, so that is the shape the
  next overlay will take. `modals::modal_backdrop_gate` asserts **which files** paint one — over
  both view crates now, through `source_gate::crate_sources` — with
  `window_chrome.rs` as the documented exception (`over_backdrop` paints the same scrim across the
  title bar while a modal is up and is mounted in the workspace root, after the layer and before the
  overlay menus). Only `theme.rs`/`themes.rs` are skipped, as the colour's own definition:
  **`lib.rs` came off that skip-list when the layer moved out of it**, since it was there for prose
  the scan already strips and it was blinding the gate to the file holding the root tuple — the one
  place a stray modal would most plausibly be mounted as a *sibling* of the layer rather than inside
  it. It is deliberately weak in the way its three siblings are — files, not counts,
  because a count fails on an innocent refactor and a gate that cries wolf gets deleted, while the
  failure worth catching is a backdrop appearing somewhere *new*, and a new place is nearly always a
  new file. That weakness has since taken a shape worth naming: `dump_view.rs` paints **two**
  backdrops now — the dump modal, and the grid export's progress modal built beside it —
  and because the file was already on the list, the second arrived with nothing checking it. A
  file-granular gate cannot see a second overlay added to a file it already knows, which is the price
  of not counting. A second test pins the other direction on the *returned closure* rather than the
  function — binding a predicate and then not `||`-ing it in is precisely the mistake, and it leaves
  the binding's name in the body, so scanning the function would pass — asserting the terms are in
  the answer: the three grouped predicates (`ddl`/`workspace`/`settings`) and the open signals the
  layer raises directly — five of the six there now are. A group added without joining it fails
  there instead of opening into a zero-sized box.
  **It reads that closure's source, so a narrowing *above* it is invisible to the gate — which is
  the point.** `whole_ui_gate`'s pass over `modals.rs` changed `modal_backdrop_up`'s signature and
  the `let` lines binding those terms and nothing inside the closure, so the gate neither broke nor
  went blind: what the predicate *takes* is free to change, how its answer is *written* is not. That
  is the bargain `overlays.rs`'s two submenu call spellings keep with `menu_order_gate`, read from
  the other end.
  **That gate's own list has to be kept current, and it goes stale exactly when a modal leaves a
  group**: the shared confirm became the layer's last entry and took its own term in
  `modal_backdrop_up`, and the array in the test was extended with it only afterwards. The grid
  export's progress modal has since made the same move, and its `export_open` term is **not** in
  that array — so that term is now the one thing here nothing pins. The list only guards what it
  names. **The event editor shipped absent from the
  predicate** and painted nothing at all, so the schema-editing half of that list is now
  `ddl_editors_up(DdlUi)`, split out of `ddl_modals_up(OverlayUi, ImportUi, DumpUi, ScriptUi,
  DdlUi)` for the one reason that matters here:
  it can be tested — `DdlUi` is a fixture `ddl_preview`'s tests already build, where the aggregate
  wants five bundles.
  `ddl_preview::tests::every_editor_reaches_the_three_lists_that_must_know_about_it`
  raises each of the nine editor targets **alone** and asserts all three lists see it:
  `ddl_editors_up`, which gives the group its box; `close_editors`, which must clear it; and
  `has_editor_behind`, which decides whether the preview's exit says **Back** or **Cancel**. One
  invariant read from three ends — the event editor shipped absent from the first and the exit label
  shipped naming two of the nine, which is why it sits beside `close_editors_clears_every_editor`
  rather than being folded into it.
- **And nothing bounds them.** An absolute child is out of flow, so text in one that is longer than
  the box lays out at its natural width and **paints across the border** into whatever sits beside
  it — not clipped, not ellipsized. `edit_field`'s placeholder did this for every field in the app
  whose placeholder outgrew its width, and it is invisible until someone opens that exact modal (it
  was found by eye on the view editor's SQLite **Column names** row). The fix is an inset on *both*
  sides — `inset_left` **and** `inset_right`, which is what gives the overlay a definite width — plus
  `width_full().min_width(0.0).text_ellipsis()` on the text inside it: `width_full` because a label
  otherwise sizes to its content and overflows the box meant to bound it, `min_width(0)` because
  without it the label refuses to shrink below that content width. `placeholder_right_inset` is where
  `edit_field` decides how much room the in-flow trailing action needs, and
  `field_button_right_inset` where it decides for a `FieldCfg::button`, whose width is measured
  rather than fixed.
  **A centred column is the same trap in flow, with a different fix.** `items_center()` sizes a child
  to its own content rather than stretching it, so a label in one is as unbounded as an absolute
  child is: the export progress modal's result sentence and its `File: <name>` line each carried
  `min_width(0)` and nothing bounding the other end, and a long caveat or a long file name laid out
  at its natural width and painted across the panel's borders. `max_width_pct(100)` is the bound
  there, and deliberately not `width_full()` — floem's default `TextOverflow` is `Wrap`, so bounding
  the box *is* the wrap, and a one-line note stays centred under `items_center()` where
  `width_full()` would have left-aligned it.
- **A row's right edge is not the panel's, and an absolute inset is measured from the *border* box.**
  Two facts that only bite together. Rows inside a horizontal scroll stretch to the **widest** row,
  not to the viewport — the SCHEMA tree is deliberately not `width_full` so it can scroll — so a
  `flex_grow` spacer right-aligns a child to whatever the longest row happens to be. The schema
  tree's size column was pushed out that way and disappeared off the right of the viewport the
  moment any table was expanded, its indented column rows having widened every row in the tree.
  The fix is to leave the flow: `absolute().inset_left(0).width(<the panel width>)` with
  `justify_end()`, and `inset_left` rather than `inset_right` because the right edge is the moving
  one. That works at every indent level because taffy resolves an absolute child's inset from the
  parent's **border** box and never adds its padding (taffy 0.4.4, `compute/flexbox.rs`,
  `perform_absolute_layout_on_absolute_children`: `offset_main = start + border.main_start`), so one
  inset lands identically on rows carrying different per-level `padding_left` — no depth arithmetic.
  That same formula has a sharp edge, because the parent's **border** is *added* to the inset where
  its padding is not (`start + constants.border.cross_start + margin`): a border that comes and goes
  moves a start-anchored absolute child by its width, even though the parent's own height never
  changes. The schema tree's 1px open-menu rule (`menu_mark`) shifted `size_badge` down 1px on every
  right-click that way, while the row's flex children sat still. Anchor to an edge only if no border
  will ever appear on it; otherwise centre — `align_self(AlignItems::Center)` takes taffy's other
  branch, which subtracts `content_box_inset` at both ends and cancels a symmetric rule.
  What you give up is the flow's collision handling: the in-flow sibling now runs *under* the
  overlay instead of pushing it along — in paint order and in the hit test both, so the overlay
  also needs the `.pointer_events(|| false)` the absolute-overlays bullet above is about.
- **Deferred layout**: `exec_after(Duration::ZERO, …)` runs after layout settles — so
  `scroll_to(bottom)` clamps against new content height, not stale.
- **`viewport` announces a width change; the editor's *lines* are not rebuilt yet.** Floem reacts to
  `ed.viewport` in an effect of its own — `lines.set_wrap(Width(viewport.width()))` — which only
  **clears** the line layouts, rebuilding them lazily. So anything measured from a *viewport* edge
  (`last_vline`, and so any wrapped row count) is computed from the previous width's layout, and
  `Lines::last_vline` caches its answer, which turns one bad frame into a permanently wrong box that
  only a keystroke re-measures. Track **`screen_lines`** for that: floem `update`s it once
  `update_screen_lines` has walked the visual lines, and walking them is what rebuilds the layouts.
  `edit_field`'s auto-grow tracks both — the guard is `viewport.width() >= 1.0` (a zero-width
  measurement wraps at `MIN_WRAPPED_WIDTH` and inflates the box), the *count* comes after
  `screen_lines`.
- **A memo or effect that tracks `screen_lines` must not make floem build a text layout.**
  `points_of_offset`, `text_layout` and `line_point_of_line_col` build one for a line that has none,
  building one sets floem's `LayoutEvent` signal, and its listener in `create_view_effects` updates
  `screen_lines` — so the tracker re-runs from inside its own run and the memo's `set` panics with
  "RefCell already borrowed". After every edit no line has a layout until floem's next layout pass,
  because bumping `cache_rev` drops the cache. Check `Lines::try_get_text_layout` first and answer
  "not yet": the pass rewrites `screen_lines`, and the tracker runs again with the layouts there.
  This is the price of the bullet above. `editor_points` is the instance (under `editor_pane.rs`),
  found when the squiggle memo crashed the app while someone typed into a 7 MB script, and
  `layout_if_built` its counterpart for a caller that needs the line's layout itself.
  `a_screen_lines_reader_never_lays_a_line_out` is the source gate (both under `editor_pane.rs`).
- **`.get()` clones the whole value — use `.with()` to read part of a collection.** `SignalGet::get`
  is documented as *"try to **clone** and return the current value"*, so `widths.get().get(ci)`
  allocates and frees the entire `Vec` to read one slot, and `expanded.get().contains(&k)` clones the
  whole `HashSet` to answer one lookup. Harmless once; ruinous in a **per-item** closure, where it
  turns an O(n) update into O(n²) — the ERD's position map cost 8.4 ms per pointer move at 500 cards
  before the fix. `.with(|v| …)` borrows and tracks identically, so it is a drop-in. This was the
  review's most-repeated mechanical defect: reach for `with` on any collection, and keep `get` for
  small `Copy` values (`bool`, `f64`, `Option<usize>`).
  **`consts::get_clone_gate` is the source gate over it, and its needle required the question to sit
  *immediately* against the `.get()`.** A borrowing adaptor or a field access in between changes
  nothing at all about the clone, so `filter.get().trim().is_empty()` — in a per-row reactive
  closure — and `d.account_draft.get().name.trim().is_empty()`, which clones a whole draft to test
  one field, both walked past it, and the gate could be described as having no population left while
  it had four. `no_reactive_read_clones_a_collection_to_ask_about_it` now walks the whole `.get()`
  chain to where the expression ends and fails only when every segment is a borrowing adaptor
  (`trim`, `as_str`, `as_ref`, `as_deref`), a field access, or one of the questions itself — so
  `.get().unwrap_or_default().len()`, which genuinely needs the owned value, still passes. The
  widening found four live sites: two in `schema_tree.rs`, one each in `account_editor.rs` and
  `snippet_edit.rs`, all now `with`.
- **`RwSignal::set` never dedups** — setting a signal to its current value still notifies, re-running
  dependent `dyn_container`s (which dispose + rebuild their child scope + owned signals). Guard panel
  reveals: `if !matches!(right_panel.get_untracked(), Ai) { set(Ai) }` — a redundant `set(Ai)` while
  the AI panel is open disposes its `elapsed_ms` mid-update and the rebuilt footer panics on the
  freed signal.
  **A *style* closure is the same fact at a different price**, and it is the one a drag pays.
  `floem_reactive`'s `set` and `update` are both a bare `update_value` (`write.rs:34-51`) with no
  equality check anywhere, so a redundant write re-runs every style closure reading the signal as
  well as every `dyn_container` keyed on it. Both panel-width publishers in `lib.rs` are therefore
  guarded on `get_untracked() != w`, the shape `reveal_panel` already used: a divider drag is
  60–120 `PointerMove`s a second and each one re-runs the effect that publishes the *rendered*
  width, which `schema_tree::tree_row_min_w` reads from the style closure of every rendered tree
  row — so a drag held past `schema_min_w()`, where the clamp returns the same number every frame,
  restyled the whole tree for no visual change at all. It is also what keeps `schema_tree`'s reason
  for **not** memoising that per-row closure true: a restyle is a theme switch, a scale change or a
  panel resize, not a frame — and a resize *is* frames, so the guard buys the premise back instead
  of paying for a memo.
  **A consumer's guard is not the publisher's, and the third dimension had no consumer to hide
  behind.** Those two are republished widths; `editor_h` has no republished twin, its readers take it
  raw, and nothing below it held the line — so the guard now sits where the value is produced, in
  `dividers::nudge`, which both handles move through and which publishes only when the clamped result
  actually differs. One guard at the source answers for all three.
  **And the three theme setters are guarded on their argument, inside the setter rather than at each
  picker**, because the callers were what proved unreliable: of the three controls reaching
  `theme::set_ui`/`set_editor`/`set_ui_scale`, the two dropdowns guarded and the interface-scale
  segments did not. A `set` on `ui` re-runs every reactive style closure in the window, the `ui_gen`
  bump beside it rebuilds every view that cannot re-read a colour, and the settings modal persists
  `ui_state.json` synchronously off the same change — a whole-window rebuild and a disk write for
  clicking the row that is already highlighted. A built theme has no identity to compare, so what is
  stored is the **kind** (`ThemeState::ui_kind`/`editor_kind`), as `Option` so `init`'s seeding
  always applies however it compares to the default the frame before it painted with.
  **A guard beside an unguarded write in the same closure is the spelling to look for.**
  `edit_field`'s document callback guarded its `text_sig.set` and not the `rows.set` three lines
  below it — same closure, same keystroke — while the viewport twin computing the identical number
  *does* carry the term; and in a single-line field that count is a constant by construction, so
  every keystroke in every field in the app republished it and re-ran the box's style for nothing.
  **`update` doesn't dedup either, and can't** — `floem_reactive`'s `update_value` calls
  `run_effects()` with no equality check, so `sig.update(|c| c.clear())` on an *already empty*
  collection is not the no-op it reads as. `discard_edits` cleared all three staging collections
  unconditionally, and the grid body's `dyn_container` key holds `new_rows.len()`: discarding one
  cell edit tore the body down, recomputed the sort order over every row and built a new one to
  arrive at the same `0` — and moved `focus_id` out from under the keyboard hand-back that the same
  discard had already put in flight. `widgets::clear_if_any` is the guard (`Clearable`, over
  `Option` as well as the collections, so "the editor is already closed" is the same case), and
  `grid::clear_tests` pins the floem fact itself by counting effect runs.
  **It lives in `widgets.rs` and is `pub` on the crate root because it was neither, and that was
  structural rather than untidy.** Module-private to `grid.rs` and re-exported from nowhere, the
  guard was unreachable from `schemaic-app` — the one crate that had live violations of the rule it
  enforces, and the crate the source gates were widened to cover on the grounds that it builds views
  too. Both violations were the schema tree's collapses: `collapse_all`'s
  `expanded.update(|set| set.clear())` notified every dependent of the expansion set *and wrote the
  UI state to disk* on an already-collapsed tree, and `collapse_db` did the same through a `retain`
  that removed nothing — `update` does not care whether the predicate kept everything, which is the
  half easiest to miss, since a predicate reads as a condition already. `retain_if_any` and
  `retain_pairs_if_any` are the partial-clear siblings written for those, the second over a map for
  the grid's three `dirty` filters, whose common case is that the deleted rows had no staged edit.
  They are **two functions rather than one trait**, unlike `Clearable`: a `HashSet` hands the
  predicate a `&T` and a `HashMap` a `(&K, &V)`, so covering both wants a generic associated type
  and more lifetime machinery than the two bodies it would save. The predicate runs twice, once to
  ask and once to do — a membership test over a few hundred short strings against a rebuild of every
  mounted subtree, so the trade is not close.
  **The rule is about call sites rather than about the function, so the pin is a source gate**:
  `no_view_code_clears_or_retains_a_signal_unguarded` scans both view crates for the two spellings
  it names, floored on finding the guarded sites so a rename or a moved `src` cannot make it pass by
  matching nothing. Its `EXEMPT` allowlist is `(file, line fragment, why)` — writes that always
  change something, so the notification is the point: a dismissed error message, the three
  `retain`s that delete a tab or a connection by an id that
  came out of the list being filtered, and a connection import row's own `notes` — a `Vec` field
  on the row rather than a collection signal, rewritten on the next line. `every_clear_exemption_still_names_a_real_write` is the floor
  under *that*, `scaled_arg_gate`'s instrument applied a second time, because an exemption matching
  nothing is a hole and not a permission.
  **That counting is available to any test, and is worth knowing before you decide something is only
  checkable with the app running:** `RwSignal::new` + `create_effect` + a `Cell` counter work in a
  plain `#[test]` with no window and no runtime, so *how many times* a write notifies is an ordinary
  assertion. `grid::mark_rows`' `marking_a_selection_notifies_once_however_many_rows` is the second
  user — a thousand rows, one notification, where the per-row spelling made it 1,001 — and it counts
  rather than times, because a wall-clock assertion is a flake on a busy machine and the count is
  what was wrong. What still needs the app is anything that has to be *laid out* or painted.
  **Two notes in this tree said the opposite, and both were wrong.** `dividers::clamp_tests` carried
  one saying a notification count was something no `#[test]` could see;
  `a_drag_held_past_the_clamp_publishes_once` now counts them, ten publishes where one is due, with a
  moving drag still publishing every frame so the guard cannot swallow the gesture. And
  `typing_over_a_clean_result_republishes_nothing` counts them for `ShownResult::dismiss_error`,
  because a test of the *outcome* is green against that defect — the state ends up `Idle` either way,
  which is why the neighbouring `typing_clears_a_live_failure_but_never_a_kept_one` passed before and
  after. It was watched red at 11 publishes where 1 was due.
  **`dismiss_error` is the editor's document callback — once per keystroke — and `update` notifies
  whether or not the closure changed anything.** Its three real conditions sat *inside* the closure,
  so every character typed republished the whole result-panel list: the strip's `dyn_stack`
  re-cloning every `ResultPanel` and two `String`s apiece, the pinned-bytes fold over every `Loaded`
  panel, the body's key memo, every mounted grid's frozen memo. This file's own measurement for that
  notification is **0.67 ms at 400 panels**, which Run Everything on a migration file reaches, and it
  falls on the keystroke path three other findings have already measured as budget-constrained. The
  early return that was there caught only "no shown panel", which a tab cannot be in — it always
  holds one — while `Tab::set_panel_state` three methods up asks first, under a comment stating this
  rule verbatim.
  **And `right_content` is the publisher whose input is a same-value write *by construction*.** It
  exists to hold the last *non-None* right panel, so closing and reopening the same one writes it
  over itself: Ctrl+Shift+A twice takes `right_panel` Ai → None → Ai, the `None` edge was already
  skipped and the way back was not. So `right_inner`'s `dyn_container` disposed the panel's child
  scope and rebuilt it — `ai_panel`'s child-scoped `elapsed_ms` freed and the "thinking" timer
  restarted from zero mid-turn, every bubble's markdown re-parsed, the reader's scroll gone — and the
  whole point of `right_content`, that the content lingers clipped through the collapse rather than
  popping out, defeated. The two publishers immediately above it in the same function both carry the
  term.
  **`set` doesn't dedup either, and the active tab is where that costs most.** `RwSignal::set` to the
  value already there still notifies every dependent (floem 0.2 compares nothing, in `set` or in
  `create_updater`), and `TabsUi::active` is `results_area`'s and `editor_area`'s `dyn_container`
  key — so a redundant write disposes the grid's child scope and rebuilds the editor's `Document`,
  costing the caret, the undo stack, the scroll position and the cell selection. Every no-op
  spelling of "go to this tab" did exactly that: clicking the chip you are already on, Ctrl+Tab with
  one tab open (`tabsel::cycle` over `n == 1` returns the current id), Ctrl+1 on the first chip,
  Find-Anywhere opening the tab that is already active, double-clicking a table already open, and
  *Close other tabs* on the tab you are on — the one tab that action promises to keep. (Staged edits
  and pending rows went with it too; they live on `PanelView` now and survive it, which is a
  different fix.) `schemaic_ui::activate(active, id)` is the one guarded writer, and it is a
  **function rather than a rule in a comment** because the rule was already written down and applied
  at one of fourteen sites: `open_table_col` guards its own `set` under a comment stating it
  verbatim, thirty-five lines from an `open_table` that does not. `no_bare_active_set_gate` (in
  `ui/lib.rs`) is what keeps the next one from happening — it reads both view crates for a bare
  `active.set(`, keying on the bare receiver because `gs.active.set(…)` is the grid's *cell*
  selection and a different signal entirely, and admits a write only when that line or the one above
  it carries `active.get_untracked() !=` (rustfmt puts the `if` there). It also asserts it still
  finds at least two guarded writes, so a rename cannot make it pass by seeing nothing.
  **An effect that starts a process pays for a same-value write in processes.** `main.rs`'s
  `warm_probe_cache` effect reads `ai_cli_path`, and the harness-switch effect writes it back on its
  own first run, so at every launch it ran twice — both before the first probe had filled the cache
  — and spawned two `claude --help`s. The guard there is the effect's own return value rather than
  a check at the writer: it returns the `(Harness, resolved bin)` key it warmed and skips a run whose
  key matches, because what must not repeat is the spawn, whoever writes. No test reaches it — the
  effect is a closure in `main.rs`.
  **A `dyn_container` key re-runs on notification, not on change**, which is the same fact where it
  costs most. The view has no equality check of its own — `create_updater` calls `on_change` on every
  re-run and `swap_val` then disposes the child scope and rebuilds it unconditionally — so a key
  closure reading the target signal *directly* rebuilds the whole modal on any write to it, including
  one that leaves `is_some()` exactly where it was. Both DDL editors that fetch something after
  opening were spelled that way, and a fetch patching `current` is such a write: landing mid-keystroke
  on a slow link it took the caret with the disposed scope (floem clears `app_state.focus` when a
  view is removed), the following characters went nowhere, and `FocusRing`'s remembered cursor reset
  with the ring. `widgets::overlay_open_key(session, open, over)` is that key as a **memo** — *am I
  open, and is something stacked over me?* — taken by the routine and view editors, with
  `an_overlay_key_ignores_a_patch_to_what_it_is_keyed_on` counting rebuilds against both spellings.
  **The `session` term is what makes dedupping the two bools safe:** presence alone answers
  `(true, false)` on both sides of *reopening the editor on a different object*, so the form would
  keep the one that just closed; `DdlUi::session` is bumped by each editor's `open` and written
  nowhere else, so it says "these contents were replaced wholesale" exactly when a fetch's patch does
  not. **The trigger editor's key is deliberately left un-memoised**, and that asymmetry is the part
  a tidy-up would break: its `form` is built once per `(selected, rev)` and its Body field seeds at
  build, so there the rebuild is the only thing that delivers MySQL's escape-corrected trigger body —
  dedup that key and a trigger silently goes on editing `information_schema`'s resolved copy. Doing
  it properly needs a body signal per row, the way the routine editor got one; there is a note at the
  key saying so. The view editor took the memo safely for the opposite reason: `fetch_algorithm`
  patches a term in the diff and not a control, so its rebuild delivered nothing to the screen.
  **The session term has a second job, and the binary-cell panel is where it shows.** A key that
  dedups by *shape* will not admit a payload that replaces one `Ready` value with another, so
  `BlobUi::loaded_file` — a file loaded over the bytes on screen — bumps the epoch to get a rebuild
  at all, and bumps it **last**, because the builder reads the payload untracked and the key must
  not admit it before the state is there. Both orderings are a one-line mistake with a settled,
  plausible-looking panel as the symptom: no bump and the old image stays over the new file's
  status line; bump first and the rebuild renders the value being replaced.
  **And a queued key can be one revision behind its own signals, so the builder must read the state
  rather than the key it was handed.** Two terms of a keying memo written in one turn queue *two*
  values — the memo notifies on each — and floem builds both, in order, against whatever the signals
  hold at processing time. A key is therefore a trigger, not a description of what is about to be
  built. The binary panel asserted the two agreed (`debug_assert_eq!(phase_of(&state), phase)`) and
  panicked on *Edit binary* over a NULL cell, where the app called `open` and then reported `Empty`
  in the same turn: the first queued key said `Loading` over a state that was already `Empty`. It
  was fixed on both sides, and only one of them is local to that panel — the caller stopped making
  the transition (`BlobUi::open` takes the state it opens in), and the builder now ignores the
  phase, which turns a stale key back into what it always was, a redundant rebuild with the right
  content.
  **The two account forms are the same bug in its other flavour**, found later and in two more
  places: no fetch anywhere near them, just a key reading the draft signal that every keystroke
  writes, so typing a name rebuilt the form around the field being typed in. They take a memo over
  a pure *shape* function (`account_editor::{account_form_shape, grant_form_shape}`) rather than
  `overlay_open_key`, because what must not rebuild them is a value **inside** the form rather than
  a patch arriving from outside it.
  **The grid's cells are the same fact at the other scale, and there the fix has a second half.**
  `data_cell`'s content key reads four **grid-wide** signals — `formats`, `dirty`/`new_rows`, `rs`,
  `edit_cell` — to compute something about **one** cell, so before it was a memo one notification of
  `dirty` rebuilt the content view of every mounted cell: ~1,000 at a maximised window's ~40 × 25,
  and a Tab-hop during data entry writes `dirty` once and `edit_cell` twice, so ~3,000. Computing
  the tuple inside the key closure does not help — the Ctrl+K popup tried exactly that — the dedup
  has to be a `create_memo`, the same device the column window (`win`) uses. The second half is that
  **the tuple must not carry a `CellEdit` directly**: its derived `PartialEq` compares the buffers,
  so a memo over a staged blob would `memcmp` as much as the 64 MiB `blob::LOAD_CAP` allows one to
  hold, on every equality check — precisely the cost the memo was added to remove. `grid::StagedFace`
  is the private newtype that stands in its place, with a hand-written `PartialEq` comparing two
  `CellEdit::Bytes` by `Arc::ptr_eq` and delegating everything else to the derive. `Arc::ptr_eq` is
  the safe direction of wrong: two `Arc`s over identical bytes compare unequal and cost one needless
  rebuild of one cell — the behaviour before the memo — and nothing unequal can ever be called
  equal.
  **The schema tree is the same fact at every level of one view, and `widgets::dedup_key` is the
  remedy applied wholesale.** `expanded` is one `RwSignal<HashSet<String>>` — the *active
  connection's* keys since `core::expanded`, but still one signal, and one signal is all this
  argument needs — and every
  children container and every chevron in the tree subscribes to it, so expanding one table in a
  500-table database re-ran `db_node`'s children key — which deep-copies every `TableInfo` in the
  database (measured, release: 2.77 ms at 500 tables × 25 columns, 6.78 ms at 1000 × 30, the clone
  alone) — and then constructed 500 fresh `table_node`s, each six views with two nested
  `dyn_container`s and two effects. Nor was it scoped to the database being touched: expanding a
  table in `analytics` rebuilt `shop`'s children too, and every chevron in both. `dedup_key` is a
  one-line `create_memo` wrapper round a key closure, the same device and the same framework fact as
  `widgets::overlay_open_key` — which is a memo over a *specific* key rather than the generic
  wrapper, because its third term exists to keep the dedup honest.
  **It lives in `widgets.rs`, not in `schema_tree.rs`, because two places pay for it and for one
  reason: a shared map keyed by node.** The ER diagram's `collapsed` is a
  `RwSignal<HashMap<String, bool>>` and each card's row stack is a `dyn_container` keyed on it, so
  every card subscribes to every card's entry — clicking one card's "+N more" toggle rebuilt
  **every** card's rows: ~3,000 rows, ~12,000 views and 3,000 memos at 500 nodes × 25 columns
  (clone-only floor, measured, release: 0.313 ms at that size, 0.662 ms at 1000 × 30) for a change
  concerning six of them. That file had already applied this remedy to the *other half of the same
  key* — the find's matched columns came out of the tuple so a highlight stopped being a rebuild —
  and left the collapse half a raw shared read, which is why the pin there is two tests:
  `one_cards_collapse_does_not_notify_another_cards_rows` asserts the property over `dedup_key` with
  the call site's own key expression **and over the undeduped shape beside it**, so the premise is
  pinned too, and `every_container_keyed_on_the_collapse_map_dedups` reads the module's source for a
  `dyn_container` keyed on `collapsed` that does not go through the wrapper. A test of the memo
  alone would pass against a call site that never acquired one, which is the failure mode this
  section has already paid for twice. **`SchemaKey` is what makes a `SchemaState` usable in such a key**: it
  compares `Loaded` by `Arc::ptr_eq`, because a derived `PartialEq` would compare a whole `DbSchema`
  by content on every notification — far worse than the rebuild being avoided — while what the
  container actually needs to know is "is this the same catalogue I built from", and a refresh
  replaces the `Arc`. It holds the `Arc` rather than reducing it to a pointer, so a later allocation
  cannot reuse the address, compare equal and leave a stale subtree; that is the one way this
  optimisation can go wrong, and `erd_view::EdgeGeometry` is the same shape for the same reason.
  `Failed` compares its whole `ModalError`, `shown` included, so the same English worded anew is a
  new row to draw (`a_schema_key_compares_the_catalogue_by_identity` has that case).
  **And only the key closure is wrapped in an effect — the *builder* is called outside it**, so a
  scaled metric read there subscribes nothing and freezes at the scale the view was built at. Two
  sites paid for that. `schema_tree`'s `SchemaTreeCtx` therefore carries `indent_levels: u32`, a
  **count and not a length**: it was `level_indent()` in pixels, read in the `Loaded` builder, so on a
  live change to 160% every row's `padding_left` (`col_pad() + indent`) grew in its `col_pad()` term
  while the indent stayed at 16 instead of 26 — a schema group's children 10px left of where the same
  call site put their parent, per level, in a panel mounted for the life of the window and so never
  rebuilt. It self-healed on a collapse/expand, which is what made it read as a glitch. The count is
  multiplied by `level_indent()` *inside* each style closure, and a count cannot be frozen at a scale
  because it does not know about one. The grid's stored column widths are the same fact where the
  fix has to be an effect instead — see `rescale_widths` under *Data grid*.
  **A *signal* read in a builder freezes the same way, and a `create_memo` is no protection.** Only
  the key closure is wrapped in `create_updater`; the child comes from `swap_val` inside
  `View::update`, so `memo.get()` at a call site inside a builder is exactly as frozen as a captured
  `Color`. Server Activity's kill surfaces were the second instance after the schema tree's, and its
  memo's own comment already said in the imperative that the two containers below must read it
  inside their own bodies — which they did, having been handed the *value*. The remedy is to carry
  the `Memo` in and read it where reading means something: `.get()` in a style closure, which
  re-runs, and `.get_untracked()` in a press or a tip closure, which are called when asked rather
  than when built. The Snippet Library's rows are the third, and the one that shows what makes the
  class hard to see: its list is keyed on `(groups, search, dialect)` and the frozen read is
  `active_conn`, which is in none of them — but `groups` is *computed from* `active_conn`, so the key
  looks as if it covers the read. It does not whenever the grouping happens to come out equal, which
  `snippet::grouped` makes the ordinary case by dropping an empty bucket: with no connection-scoped
  snippet on either side the two `Vec<Group>` compare equal, the memo never notifies, and *Show in →
  This connection* wrote the connection the panel was opened on. **A key term derived from the signal
  you are reading is not the same as the signal**, and that is the version of this bug to look for.
  **No gate came out of any of the three.** A crude scan finds **61** `dyn_container` builders
  across `schemaic-ui` + `schemaic-app` that read a signal with `.get()`; most are reading something
  already in their own key, which is harmless, and nothing mechanical separates those from the frozen
  ones. **That sorting pass has since been walked by hand, and came back clean — with one hole,
  named below.** Every site visited either reads only what its own key already carries, or reads
  elsewhere inside a nested tracking scope (a `.style()`, a label, a tooltip), or is a transient
  right-click or dropdown menu deliberately reading `get_untracked()` at raise time, which is that
  genre's convention rather than a lapse; `schemaic-app/src/main.rs` contributes nothing at all, its
  two hits for the name being comments. One site was fragile rather than wrong and now carries a
  test — the Users browser's container, written up under `users_view.rs`.
  **The hole was `grid.rs` and `editor_pane.rs`**, the two largest populations, and they have since
  been walked site by site as well — 23 `dyn_container` call sites in the first, 19 in the second.
  That walk turned up the only two genuinely frozen reads in the whole sweep, one per file. The
  grid's header chevron is written up under *Data grid*: `header_cell` computed its sort indicator
  from `gs.server_sort_dir(ci)`, a tracked `grid_query` read, at build time, while being called from
  the grid body's builder and the virtualised header row's — neither key mentions `grid_query`. The
  Ctrl+K verdict footer is under `editor_pane.rs`: its `N hunks · X − / Y +` line came from a
  `cmdk.preview.get()` in the builder of a container keyed on `verdict_shape`, which reads
  `cmdk.open` and whether `inline_ai` is `Ready` and never the plan. Both reads now sit in a
  closure, and each leaves a `source_gate`-family scan of its own file behind it —
  `grid::header_sort_tests::a_server_sort_read_sits_in_a_tracking_scope` and
  `editor_pane::cmdk_preview_gate::a_preview_read_sits_in_a_tracking_scope`, each asserting that
  every read of that one signal in that one file sits **inside** a `move |…|` closure. Both were
  watched failing against the unfixed tree, naming `grid.rs:9305` and `editor_pane.rs:661`.
  **Both asked proximity first, and proximity is not the property**: the test was "does `move |`
  appear anywhere in the three lines above the read", and the single commonest three-line neighbour
  in either file is an unrelated `.style(move |s| …)`, which satisfies that without the read being
  inside anything. Each gate's *message* stated the real question the whole time.
  `source_gate::inside_a_closure(code, at)` is the question itself, shared by the two: find the
  nearest `move |` above, step past the parameter list, and walk the brackets forward — a closure
  that has already closed takes the depth below zero before the read is reached. It is narrower than
  the truth in exactly one direction, a closure whose body is a bare expression ending at a `,`
  rather than a bracket not being detected as closed, and still strictly stronger than counting
  lines. It is `#[cfg(test)]` like the corpus walkers beside it, both callers being gates.
  The cmdk half also **selects on the signal rather than on one accessor**: keying on
  `.get()`/`.with()` meant `cmdk.preview.get_untracked()` — the same bug one spelling along —
  matched no needle, was skipped, and left the gate to fail on its own stale count with a message
  reading as a maintenance nudge. It now matches all four accessors and refuses the two untracked
  ones by name. (Passing the signal on, `inline_band_runs(&ed, cmdk.preview)`, is still not a read:
  there is no accessor after the dot.) They are
  per-file and per-signal on purpose: the general gate is still the one the paragraph above says
  cannot be written, and this is what a hand walk can leave behind so a swept file does not quietly
  reacquire the bug.
  **"The only two" was wrong: `editor_pane.rs` held three more, none of them spelled as a read.**
  The bracket and occurrence boxes computed `let cx = content_x(&ed)` in their builders and the Run
  Current border computed `statement_line_boxes` in its; `content_x` reads the code font's size and
  `statement_line_boxes` that and the viewport and the screen lines, all inside the helper, so the
  builder shows a plain function call. A zoom left all three where it found them until the caret
  moved (GitHub #20). The fix and its gate, `no_overlay_freezes_its_geometry_in_an_untracked_builder`,
  are under `editor_pane.rs`'s code column. **A helper that reads a signal is a read**, wherever the
  signal itself is spelled.
  **The rule both fixes are instances of: a *structural* choice that depends on a signal — which
  child is built — needs that signal in the container's key or a container of its own. Only a
  choice about *style* may be left to a `.style()` closure**, which floem does re-run. The chevron
  needs both halves at once, which is why it is the clearer of the two: the column name's colour is
  a style and reads the indicator inside its style closure, while sorted-versus-unsorted is a
  chevron icon against a zero-width spacer — different *children*, which no style closure can switch
  between — so that slot is a nested `dyn_container` over the same closure. Reaching for the style
  closure alone is the tidier-looking mistake here, and it silently keeps whichever child the
  builder happened to make first.
  Where the read has a *later* moment that wants it — a press, a tip, a right-click that builds a
  menu — the remedy is to move the read there rather than to widen the key, which is what both the
  activity panel's row menu and the snippet row's do.
  **The opposite error costs as much and reads as tidier**: a key narrower than what the child
  renders never rebuilds it at all. `dump_view::export_progress_overlay` keys on
  `(target, done, error)` for exactly that reason — keyed on `target` alone, which is the one signal
  that says whether the modal is up, a finished export went on showing a red Stop over a file that
  was already written.
- **`create_effect` runs its body once immediately, so an effect that *saves* what it watches saves
  what it has just loaded.** The Find-Anywhere search history is the one store the app persists
  through an effect at all, and that first run wrote
  `search_history.json` back before the window was drawn — so its `.bak` was always exactly one
  launch old even in normal operation, and on a launch that loaded defaults (a read failure, a
  removed file) the write put an empty primary on disk and the launch after that rotated the empty
  file over the last real copy. The guard is the effect's own `prev` argument, `None` on the first
  run and nothing else: the same shape the routine editor's write-back uses so seeding is never
  mistaken for an edit, and the resize hint's poke uses so no bar flashes on mount. A signal that is
  *loaded* and then *watched* is the general case — the first run establishes tracking and has
  nothing to report.
  **The second thing an effect cannot say is *which kind* of save this is**, which is why that
  store now has an explicit `Rc<dyn Fn(persist::Saving)>` saver beside the effect rather than
  instead of it. An effect sees a changed signal and nothing else, so it saves `Replacing` — right
  for an ordinary Find-Anywhere activation, where the previous generation as `.bak` is the recovery
  wanted, and wrong for the connection delete, where it copied the pre-prune file whole to
  `search_history.json.bak`. `delete_conn_now` calls the saver with `Saving::Erasing` straight
  after the prune, and the ordering is safe either way floem schedules the effect: if the effect
  runs inside `update` it writes a `.bak` holding the pre-prune file and the erasing save then
  removes it, and if it runs after, it copies the already-pruned primary. What matters is only that
  the explicit save is *after* the prune.
- **An effect that writes the signals it reads must read them untracked — and an outside write to
  them is then invisible, so it needs a generation counter.** The schema tree's size-column effect
  scans every `ConnNode::stats` slot for `Idle` and writes `Loading` into each one it fetches;
  tracking those reads would make the effect its own dependency, re-entering it mid-loop and
  double-fetching every database it had not yet reached. `get_untracked` there is load-bearing, not
  an optimisation. The half that is easy to miss is the other side: a refresh resetting those same
  slots to `Idle` now changes nothing the effect watches, so the sizes went blank and only returned
  when an unrelated dependency happened to re-run it. The answer is a bare counter beside the state
  — `main.rs`'s `stats_gen: RwSignal<u64>`, bumped by `start_fetch` immediately after the reset and
  `track()`ed by the effect. **Every consumer of those slots has to take that dependency, not just
  the one the counter was added for**: the results toolbar's asker read nothing tracked, so it ran
  once per grid build while the memo beside it stayed live on the slot — the half that can only *lose*
  the figure. It reaches the UI as `SchemaUi::stats_gen` and travels to the grid on `GridCtx`. Don't
  lean on a state signal that "obviously" already changed: the
  connection-wide refresh does `set` `db_nodes`, which the effect *does* track, but it does so
  before `start_fetch` resets the slots, so that run still saw them `Loaded` and found nothing to do.
- **Don't read a locally-scoped signal inside a `dyn_container` child keyed on a *parent/shared*
  signal.** The child rebuilds when the shared signal changes — and if it changes *while the
  enclosing view is disposing* (e.g. `active_db` updates as the query pane is replaced on opening a
  table), the rebuild reads the freed local signal → panic. Fix: read the local signal in a stable
  outer scope and let the child inherit (put the hover `color()` on the parent `h_stack`, not the
  `active_db`-keyed child). Reading *global* signals (theme, `connections`/`active_conn`) there is
  fine — they never dispose.
- **No `text-align` in Floem 0.2.** Text paints from a fixed left origin; `Style` only has
  `text_overflow`. To right-align an inline editor (numeric grid cells), pad left by the free space
  — measure with a throwaway `TextLayout` at `font_body()` (same global `FontSystem` →
  pixel-exact), recomputed reactively on the buffer (`widgets::measure_text_px`). The padding goes
  on the field's **box**, through `FieldCfg::inset_left` — padding floem's editor itself is a no-op,
  it being a scroll view (see the SQL editor's entry below) — and `edit_field` reads that one
  `pad_left` closure for the placeholder, the `suggest` ghost and the click-in-chrome mapping too,
  so none of them drifts from the text. **"Free space" is the cell's real content box**
  (`grid::numeric_edit_pad_left`, tested): the column width less its padding *and* its 1px right
  divider, which is a border and so comes out of the content too. When the editor was floem's
  `text_input`, it sized its inner text node to `content − padding_left` and clipped as soon as the
  text was one pixel wider — **on a glyph boundary**, so a 5px error was a whole missing character:
  a padding computed against `col_w − 20` showed one digit of a 2-digit id and *nothing* for a
  1-digit one. The `edit_field` that replaced it has the same `content − padding_left` viewport but
  *scrolls* a value a hair too wide to keep the caret in view, so the cliff is now a pixel or two of
  scroll rather than a lost digit. Leave the couple of px of slack anyway: the caret is drawn past
  the last digit, and the slack is what keeps it inside.
- **SQL editor padding is a no-op; inset via a wrapper.** The editor is a scroll view — its own
  `padding_*` is ignored. Wrap it in a container carrying the border + padding (`editor_box`); the
  editor fills it flush. Top padding shifts the content origin, so `points_of_offset`-anchored
  overlays (completion popup, statement highlight, squiggles, Ctrl+K's question bar and verdict
  footer, run menu) each add back **`EDITOR_TEXT_TOP`** to their `y`, and `pointer_offset` takes it
  back off a pointer. That is `1.0 + EDITOR_PAD_TOP` — `editor_box`'s 1px border *and* its padding,
  because `editor_area`, the space every overlay is placed in, is the border box. Every overlay
  used to add `EDITOR_PAD_TOP` alone and sat a pixel above its text, and the run outline hid it
  with a hand-tuned `+ 1.0` of its own (GitHub #9); `EDITOR_PAD_TOP` is now only `editor_box`'s
  `padding_top`, and the custom scrollbars' `ESBAR_TOP` is `EDITOR_TEXT_TOP` too.
  `an_overlay_starts_below_the_border_and_the_padding_both` pins the occurrence box, the squiggle
  hover box and the statement highlight against a literal `1.0 + 5.0`, so the constant redefined
  as the padding alone fails it. **Ctrl+K's *diff* is not on that
  list and is not an overlay at all** — it is phantom rows inside the editor's own line flow
  (`inline_diff`), so it is laid out, scrolled and padded as text; only the bar above it and the
  footer below it are anchored, and the footer's `y` comes from `inline_footer_y`, which adds
  `EDITOR_TEXT_TOP` like the rest. The built-in scrollbars float at the *content* edge (can only inset
  inward), so they're **replaced with custom overlay scrollbars** (`v_scrollbar`/`h_scrollbar` in
  `editor_area`): built-in bars hidden (zero-`Thickness` + transparent `Handle`), two `empty()`
  thumbs pinned to the border (`inset_right/bottom(3)`) with `autohide_state()`. Geometry from
  `ed.viewport` (offset `x0`/`y0` + visible `width()`/`height()`) vs. content (`ed.max_line_width()`,
  `(ed.last_vline()+1) * ed.line_height(0)`), in `v_geo`/`h_geo` shared by the style closure and drag
  handler. **The vertical content is taller than the text**: the editor sets `ScrollBeyondLastLine`,
  so Floem lays it out as `max(text, viewport)` plus a bottom margin of `min(viewport, text) − one
  line` — the virtual space that lets the last row be scrolled up to the top, which makes the maximum
  scroll `text − one line` whether or not the document overflows. `scrollbar_geo` (tested) adds that
  margin back through `consts::body_scroll_h`, the same function the results grid is sized by;
  measured against the text alone the thumb bottomed out a viewport early, and a document that
  merely *fits* showed no bar while the wheel still moved it. Thumb `.style()` reads `viewport.get()` **and**
  `query.get()` (content size isn't a signal). **Draggable**: `PointerDown` records grab offset +
  `id.request_active()` (pointer capture); each `PointerMove` sets `ed.scroll_to.set(Some(Vec2))`
  (it's `Option<Vec2>`, not `Point`). Thumbs use `scrollbar_hover()` + `CursorStyle::Default`.
- **Phantom text is how you put rows in the editor that are not in the document — and its list is
  order-sensitive.** A `Document`'s `phantom_text` hook returns text that is combined into a line's
  layout without ever entering the rope; `inline_diff` renders the whole Ctrl+K suggestion, the
  inline prediction and the editor's inlay hints through it. It may be **multiline** — a `\n` in a phantom lays
  out as an extra visual row, which is exactly why `TextLayoutLine::line_count` counts the line's
  nonempty layouts instead of assuming one per logical line, and why anything positioning by row has
  to go through that count. `PhantomTextLine.text` is walked **in stored order**, accumulating a
  column shift as it goes (`combine_with_text`), and floem never re-sorts it: the columns must be
  non-decreasing, so a block rendering *before* the line (column 0) has to lead the list and an
  end-of-line block has to trail it, and a phantom at a mid-line column — the inline prediction at
  the caret, an inlay hint before its value — has to be inserted at its place (`inline_diff::phantom_slot`). Per-token colour therefore works by pushing one `PhantomText`
  per token — each carries a single `fg`, and the order you give them is the order they render in.
  **An added line that is empty has to render as a single space**: `relevant_layouts()` filters out
  layouts with no glyphs, so a genuinely empty row collapses and the block comes out one row shorter
  than it claims. And **writing the signal a phantom is built from changes nothing on screen by
  itself** — phantom text is baked into the line's cached `TextLayout` and that cache is keyed on
  `cache_rev`, so the write has to bump it or the rows appear a frame late, or not until the next
  keystroke happens to invalidate the line. `inline_diff::set_preview` and its twin `set_ghost` are
  the two places that pair is spelled out, and the only ways the Ctrl+K preview and the inline
  prediction should be set. The inlay hints split it: `inline_diff::set_hints` only writes, and
  the bump is an effect on the signal in `editor_pane`, so every write to the pane's `hints`
  reaches the screen whoever makes it.
- **Wrap the editor's document *last*: `TextEditor`'s document-callback builders silently do
  nothing on a wrapped doc.** `update`, `placeholder` and `pre_command` each resolve the document
  with `downcast_rc::<TextDocument>()` and are written `if let Some(doc) = self.text_doc() { … }`
  (`floem-0.2.0/src/views/text_editor.rs:480,516,527`) — no error, no warning, no return value to
  check. `use_doc` installed *before* `.update(…)` therefore registered the `query.set(text)` sync
  on nothing, and the signal every consumer treats as the tab's SQL stopped following the editor
  entirely: autosave, Run, diagnostics and Ctrl+K's own statement lookup all read whatever text the
  tab happened to open with. It surfaced as "Ctrl+K always picks the first statement", which is
  what a stale `query` looks like once `statement_range` clamps an out-of-range offset — the
  wrapper was two months of features away from the symptom. So `use_doc` is the **last** builder in
  `editor_pane`'s chain, after `.update(…)`, and `SqlStyling` is built over the *inner* document on
  purpose (same rope, same `cache_rev` — the wrapper delegates both — and no dependency on a
  wrapper installed later). **There is no regression test**: it is a view-wiring failure in a
  builder chain that needs a running Floem app to observe, so the comment at the call site and this
  entry are the whole guard.
  The same family, one level down: **a `Document` wrapper inherits the trait default for everything
  it does not override, and the default is not always what the wrapped document said.**
  `InlineDiffDoc` took `has_multiline_phantom`'s default of `true`, where the `TextDocument` under it
  answers `false` for any non-empty buffer. Floem's `is_linear()` is
  `wrap == None && !has_multiline_phantom()` and word wrap is off by default here, so the wrapper
  quietly took the SQL editor off its linear visual-line mapping — every document, the whole session,
  for a feature that is live for a few seconds at a time. It now answers `true` only while a plan is
  published and delegates otherwise. Overriding one method is not the end of the audit: read the
  trait's other defaults and ask which of them the wrapped document was answering differently.
- **`ed.read_only` is a shared flag with two owners, and it does not gate `Document::edit`.**
  Anything that flips it must **save what it found and restore that**, never assume `false`, and
  must not treat it as a lock on the document. Floem checks it in `receive_char` and `run_command`
  only, so `doc.edit_single` — which goes through `Document::edit` — writes straight through it.
  Both halves of that were live bugs at once. The auto-pair handler flips it true for one key
  dispatch to suppress Floem's unconditional built-in char insert (the editor inserts the typed
  character after the handler returns, ignoring `CommandExecuted`), and its comment claimed to be the
  only reader; then the Ctrl+K preview began freezing the buffer with the same flag while a
  suggestion is on screen. A bracket typed during a diff therefore edited *through* the freeze, and
  the handler's deferred `set(false)` then cleared the freeze for the rest of the preview. The
  handler now returns early when the flag is already set and restores the value it found. A flag two
  features reach for is not a lock; if a third one arrives, this is the paragraph it has to read.
  **The answer to the second half is a funnel: `editor_pane::edit_untyped`.** Every programmatic
  edit in that pane goes through it, it asks `read_only` itself and it **refuses** — it answers
  `bool` rather than looking and proceeding, and the `bool` is `#[must_use]`: Tab, Shift+Tab,
  Ctrl+/, Alt+↑↓ and the snippet library each set the caret after a refused edit, selecting text
  that had not changed, until the compiler was made to ask. That one check replaced nine missing ones: the preview
  holds the keyboard while it is up, so Ctrl+/, Alt+Up, Tab, Ctrl+D (caret or selection — both go
  through `text_ops::duplicate_lines` now, floem's `DuplicateLineDown` having put the newline-less
  last line's copy on the same line), Ctrl+Alt+L, the
  paired Backspace, the find bar's Replace and the snippet library all reached the document through
  it, each moving every byte after the caret while `cmdk.start`/`end` stayed put — so Accept spliced
  the approved suggestion into the middle of a different token. `edit_untyped_frozen` is the one
  named exception, for Accept, which *is* the edit the freeze exists to protect and runs while the
  flag is still set; it is a separate function rather than a `bool` parameter so the exception is a
  name a reader can grep for and nothing acquires it by passing `true`. Both halves are held by
  `editor_freeze_gate`, a source gate rather than a unit test because the decision is one line and
  the thing that went wrong was a *census* — nine of ten sites not asking: one test reads
  `edit_untyped`'s own body for the check and the refusal, the other scans for a raw `.edit_single(`
  with no `read_only` question within 30 logical lines above it, `FREEZE-EXEMPT` being the marker the
  funnel's own pair carries. Against the unfixed tree it reports nine offenders.
  **`edit_untyped_multi` is the funnel for an edit at several carets** (a `text_ops::MultiEdit`):
  one `Document::edit` of every replacement, so one undo step, then every caret's selection set
  from the `MultiEdit` — refused while frozen, as `edit_untyped` is. `several_regions` answers the
  regions only when there are several, so a lone caret keeps the single-caret path. The
  several-caret auto-pair is `auto_pair_several`, which asks the freeze itself and suppresses
  floem's insert through `read_only` as the lone path does, and spends the `comp.typed` one-shot
  when every caret only stepped over. **The gate counts it**: it scans for `.edit(&mut` as well as
  `.edit_single(`, since the multi edit is the same ungated door. The new helpers sit below
  `edit_untyped_frozen`, and the multi-caret auto-pair branch is a helper call ahead of the lone
  path's freeze check, so that every edit the gate counts keeps its check within its 30-line
  window.
- **The editor has two indent units, and floem's edit commands read the one we cannot set.** The
  `indent_style` `editor_pane` hands `editor_style` from the tab settings is the *style's*, which
  floem reads only to space the indent guides (drawn only while Settings → Editor → *Indent
  guides* is on). `InsertTab` (with `smart_tab`), `OutdentLine` and
  `InsertNewLine`'s `auto_indent` take `Buffer::indent_unit` from `floem-editor-core`, which is
  `IndentStyle::DEFAULT_INDENT` — `Spaces(4)` — and has no setter: `Buffer::detect_indent`, the
  only way to move it, prefers whatever the text already holds, and nothing here calls it. So
  turning `TextDocument::auto_indent` on is not how to make Enter indent — it would indent by four
  spaces under any setting — and every indenting key is computed in `core::text_ops` and applied
  through `edit_untyped` instead: Tab, Shift+Tab and Enter (under `text_ops.rs`).
- **The `EditType` a programmatic edit passes is a decision about undo, not a label.** Floem
  groups undo steps by it: `EditType::breaks_undo_group` (`floem-editor-core` 0.2, `editor.rs`)
  keeps an edit in the previous group only when both are `InsertChars` or both are `Delete` —
  the kinds floem's own typing and Backspace use — and starts a new group for every other kind.
  `InsertChars` reads as the natural type for an insertion, and that is how Tab, Shift+Tab and
  Ctrl+D came to pass it to `edit_untyped`, and Ctrl+X on bare carets `Delete`: each merged with
  the typing or the Backspaces around it, and one Ctrl+Z took both away. They now pass `Indent`,
  `Outdent`, `Other` and `Cut`. The pane's other programmatic edits already carried a kind that
  never merges — `InsertNewline` for Enter, `ToggleComment`, `MoveLine`, `Completion`, `Paste`
  for the Ctrl+K accept, `Other` for the Ctrl+Shift+J/U and Ctrl+Shift+/ arms, the format, the find
  bar's Replace and Replace All and the snippet library. **Three keep a merging kind on purpose**,
  all through a raw `doc.edit_single` rather than the funnel: the auto-pair insert is
  `InsertChars` because it *is* a typed character, the `END`/`WHEN`/`ELSE` dedent
  (`text_ops::dedent_typed_word`) is `InsertChars` because it is the typed letter that finishes the
  word, so it undoes with that typing rather than alone, and the empty-pair Backspace is `Delete`
  because it is a Backspace — the auto-pair's and the Backspace's several-caret forms keep the same kinds through
  `edit_untyped_multi`, as Tab, Ctrl+/, Ctrl+D and Alt+↑/↓ keep theirs. What it costs: two Tabs in a row are two undo steps, since a
  non-merging kind does not merge with itself either. What it does not cover: every keystroke is
  `InsertChars`, so a run of typing is still one undo step however long — nothing breaks it at
  word boundaries. No test can fail on any of this, the choice being made at view call sites and
  the merge being floem's; the comments at the Tab, Shift+Tab, Ctrl+D and Ctrl+X sites are what a tidy-up
  back to `InsertChars` has to read.
- **Shift+wheel → horizontal scroll in the editor.** The editor owns its scroll internally, so
  `shift_hscroll` can't reach it. Register a `PointerWheel` listener on the internal scroll view —
  reached via `ed.editor_view_id.get_untracked().and_then(|c| c.parent())` (the content view's parent
  *is* the scroll) + `ViewId::add_event_listener`. Floem's `Scroll` runs registered listeners in
  `event_after_children` before its default scrolling, so returning `Stop` for shift+wheel suppresses
  vertical scroll; push the horizontal delta through `ed.scroll_delta` (Windows delivers shift+wheel
  as vertical `delta.y` → map to x). Event flow: a child's `event_after_children` runs *between* the
  parent's before/after, and a child that consumes a pointer event stops propagation — so a
  `PointerWheel` `on_event` on an ancestor never sees a wheel the inner scroll consumed. Target the
  scroll's own `ViewId`.
- **A pointer event stops at the first child that accepts pointer events — handled or not.**
  `EventCx`'s child loop (`floem-0.2.0/src/context.rs:143-158`) walks the children in reverse,
  dispatches to the first one `should_send` passes, and then does `if event.is_pointer() { break }`.
  For a pointer event (**the wheel included**) that break ends the search whether or not anything
  was processed, so an absolute overlay lying across another view kills scrolling over itself, and
  *not handling* the event does not help — there is no "pass it on" once your view is eligible.
  There are two ways out, and which one applies is decided by whether the overlay needs clicks.
  **Forward the wheel** if it does: take the event and push its delta into the target's own scroll
  channel (`ed.scroll_delta`), which is what Floem's editor gutter does with this exact problem
  (`floem-0.2.0/src/views/editor/view.rs:1112`) and what the inline-diff verdict bar does, so it can
  hold its Accept/Reject buttons and set a cursor while a wheel over it still scrolls the document.
  **The forward goes on every eligible view, not on the container** — the `break` happens at
  whichever *child* the pointer is over, so the bar's own handler covered the bar and nothing else:
  scrolling worked over the strip and died over the two words and the change count, which reads as
  intermittent swallowing rather than as a missing handler. All four carry it.
  **`pointer_events(false)`** otherwise: it takes the view out of `should_send` entirely, and is
  right for something with nothing to click — the inline-diff band strips, the statement highlight,
  the squiggles.
  **The squiggles are in that list because they have nothing to hover, and they used to be listed
  there while also carrying a tooltip.** Each diagnostic strip had `.pointer_events(|| true)` and a
  `.tooltip()` with the diagnostic's message, under a container comment claiming the strips
  "re-enable pointer events so hovering the underline reveals the message". They never did, and
  could not: the flag is not inheritable, so the container's `false` `continue`d past the whole
  subtree and the `Tooltip` node never saw a `PointerMove`. The message the squiggle is drawn *for*
  had no surface in the editor at all. Both decorators are gone, and the message no longer travels
  in the memo that positions the strips.
  **And the repair is not to move the flag down**, which is the reading this bullet invites: a
  view's `layout_rect` is unioned with its children's, so leaving the intermediate `v_stack`
  eligible gives it the bounding box of every squiggle on screen — and the walk `break`s on the
  first child containing the point whether or not that child consumes it, so two errors twenty lines
  apart would make the rectangle between them dead to selection, scrolling and the caret. It wants
  the loose-siblings remedy stated above, which for this overlay means the strips becoming direct
  children of `editor_area`'s `stack` — already at its 16-child limit, and a varying number of
  squiggles cannot be fixed tuple slots anyway. `erd_view`'s card header is the same lesson where it
  happens to be easy: the tooltip's owner *is* the small view, so it simply drops the opt-out.
  **What shipped makes nothing hoverable instead.** The squiggles stay click-through, and
  `editor_area` — the ancestor, not an overlay — listens for `PointerMove` with `on_event_cont` and
  hit-tests the position against the boxes the waves are drawn from (`diag_hits`, see
  `editor_pane.rs` under *Crates*). It sees every move over the editor because of two facts about
  dispatch, either of which a tidy-up could break: floem's editor content view takes `PointerMove`
  with `on_event_cont` too (`floem-0.2.0/src/views/editor/view.rs:1152`), so it never stops the
  event; and `unconditional_view_event` runs a view's own listeners *after* its child walk when no
  child reported the event processed (`context.rs:143-168`, then its `PointerMove` arm), so an
  ancestor hears every move its children let through, in its own coordinates. Keep that listener
  `cont`: it watches a move the editor has already had and has no business claiming it. The tip it
  raises is paint-only and shares `signature_popup`'s click-through slot, so where a tooltip would
  have needed a hit rect this route adds none — nothing can eat a click, a selection or a wheel.
  **The corollary is that the ancestor hears moves over its overlays too**: none of the bars drawn
  over the text consumes `PointerMove`, so `editor_area` heard a move over the run menu as a move
  over the squiggle beneath it and opened the tip over the menu. Each such overlay now marks the
  moves it receives (`marks_pointer`, see `editor_pane.rs`), which relies on the same ordering —
  child first, ancestor after, in one dispatch.
  **Except while a button is held over the text: then the ancestor hears no move at all.** Floem's
  content view calls `request_active()` on every press (`views/editor/view.rs:1147`), and while
  `app_state.active` is set every pointer event goes to that one view
  (`window_handle.rs:294-319`, `unconditional_view_event(id, …, true)`) — no walk by position, so
  nothing bubbles to `editor_area` — until the release clears it. (A non-primary release outside
  the active view's rect used to be dropped there, so a middle-drag never heard its own end;
  vendored Floem's fourth patch delivers it, above.) Alt+Shift+drag's box selection
  is built in exactly that listener and so never drew a box. An ancestor that needs the drag has to
  hand the capture back, as the Alt+Shift `PointerDown` does with `clear_active()` on
  `ed.editor_view_id`, and **the order is load-bearing**: `UpdateMessage::ClearActive` is honoured
  only when its id is the active one (`window_handle.rs:862-866`), so it works because the
  ancestor's listener runs after the content view's and its clear is queued behind the `Active`
  request. Issued from anywhere that runs before floem's press, it is silently dropped.
  **A container introduced for layout or arity reasons is a hit target too**, and this is the trap
  rather than any one overlay. `absolute().inset(0)` on a wrapper whose children are small and
  edge-pinned turns a few thin overlays into a single pane-sized one, and nothing about it says so:
  it paints nothing and looks like grouping. `stack`'s **16-child limit** is what tempts the edit —
  wrapping two views to free a slot is the obvious move — and doing it to the editor's two custom
  scrollbars put a full-pane, pointer-taking view above the editor, which by the rule above ate
  every click, drag and wheel in it. The editor was dead to input, from a change that was about
  arity. They are listed one by one now (exactly 16), with a comment at the site saying never to
  group them. The `stack((editor_box, inline_band_view))` nesting is safe by contrast because it
  wraps the editor *itself* rather than floating above it: the question is never how big the wrapper
  is, but whether it sits over other interactive views. `stack((signature_popup(..), diag_tip_view))`
  is an arity wrapper that *does* float over the editor at full pane size, and it is safe only
  because it is `pointer_events(false)` with nothing to hover beneath it — as is
  `stack((syntax_view, ctrl_link_view))`, the squiggles and the Ctrl+hover underline, on the same
  terms.
- **A programmatic `cursor` change shows a phantom caret on an *unfocused* editor.** `edit_field`'s
  signal→doc reconcile sets `cursor` to preserve caret position, but floem's internal "reset cursor
  blinking" effect tracks `ed.cursor` → `cursor_info.reset()` → shows + blinks the caret even with no
  focus (made Name/User/Password look focused). Fix: only write `cursor` in the reconcile when
  `focused.get_untracked()`. The stale offset is safe — floem clamps `offset.min(text.len())` and a
  real click resets it. (Don't hide the caret *after* the write — floem's reset effect runs after
  yours and re-shows it.)
- **A perpetual self-rescheduling `exec_after` tick must read signals with `try_get_untracked`.** The
  terminal cursor-blink tick reschedules forever; at shutdown the scope disposes its signals and the
  last timer panics on `get_untracked`. Guard every read with `try_get_untracked` and stop
  rescheduling once any returns `None`.
  **And *every* read goes inside the guard, not just the one that raised it.** `edit_field`'s
  caret-to-end timer asked `try_get_untracked` for the editor's view id, requested focus, and then
  called `ed2.doc()` — which is `get_untracked().unwrap()` on the same signal — **outside** the
  guard, so it detected a disposed field and panicked on it one line later. The autofocus twin
  forty-five lines above has both reads inside the guard, which is what the second one was copied
  from and what it dropped. A guard that names one read is a guard a second read walks past.
- **A deferred action's generation guard must live in the same scope as the state it clears, or in a
  longer-lived one.** The idiom above usually has a second half: the timer compares itself against a
  generation before acting (`if save_gen.try_get_untracked() == Some(g)`), which asks *did something
  newer happen?* and stands down on `Some(newer)` and on `None` alike. Put that guard in a
  shorter-lived scope than the signal it clears and the question inverts into a permanent refusal —
  every late timer reads `None` from a disposed generation and declines to do the one thing it was
  armed for. Manage Connections' `save_flash` was correctly hoisted into the modal's stable scope so
  that a late timer would never fire on a dead signal, but `save_gen` stayed inside `conn_form`,
  which the open/close `dyn_container` disposes: close the modal inside the 2s `SAVE_FLASH` and the
  check mark was still sitting on the Save button the next time it opened. Hoisting the flash out
  was necessary and not sufficient. **And a transient confirmation is better withdrawn when the
  surface showing it goes away** than left to its own timer, which is the fix taken there — a check
  reporting a save from a previous visit is wrong even while its two seconds are still running.
- **`.clip()` makes a flex item shrink-to-content — it won't stretch to its parent**, so a
  `flex_grow` spacer inside it collapses (right-aligned children stop reaching the edge). Put
  `.clip()` on a container with a *definite* size (the fixed-width `v_stack`), not the flex row you
  depend on for stretch. (Bit the find/replace bar's `All` alignment.)
  **`.clip()` is not a style — it is a node, so styling a clipped view means styling two of them.**
  `floem::views::clip` (`floem-0.2.0/src/views/clip.rs`) wraps the child in an unstyled `Clip`, so a
  `.style(…)` written *before* it lands on the child and the `Clip` sits between that child and the
  parent with `width: auto`, `min_width: auto`. That stays invisible until the clipped view wraps
  text. A `RichText` soft-wraps only when it is handed a width narrower than the line it holds, and
  both SQL previews collapse a body to one long line (`snippet::collapsed`, `history::preview`), so
  the wrapping is entirely at the mercy of the width the `Clip` resolves to. In a `flex_col` parent
  width is the *cross* axis, where no content-based minimum applies: the auto-width `Clip` stretches
  to the row and the text wrapped for free, which is why the snippet library and Query History
  previews wrapped without anyone asking them to. Wrap the same view in a `container` to give it a
  background and floem's `container` is a **row** (taffy's default direction, and `Container` sets
  none) — the `Clip` becomes a main-axis flex item, whose automatic minimum size is its content's,
  i.e. the whole statement. It took that width, the text's `width_full` resolved against it, and the
  library's three-line preview became one line cut off at the panel edge (fixed in `ecfe595`, after
  two attempts that put the same `min_width(0)` on the nodes either side of the `Clip` — the
  container above it and the text below — and changed nothing, because neither is the flex item the
  parent measures). The fix is `min_width(0)` on the `Clip` — a
  `.style(…)` applied *after* `.clip()` — and both panels now carry it on all three of the text, the
  `Clip` and the surface container. Adding a background container around a wrapping text view is
  what turns the omitted second style into a visible bug.
- **`tooltip()` re-parents its child, so every style chained *after* it lands on the wrapper and not
  on your view.** It is not a decorator: the app's `tooltip::tooltip`, like the
  `floem::views::tooltip` it was copied from (floem-0.2.0, `src/views/tooltip.rs:45-51`), mints its
  own `ViewId` and calls `id.set_children(vec![child])`, so
  the view you wrote is no longer the view the next `.style()` in the chain sees. That is the
  general hazard with **any constructor that wraps rather than decorates**, and the `.clip()` bullet
  above is the same fact read from the other end — but the two fail in *opposite* directions, so
  neither rule generalises from the other: with `.clip()` you must style **after** to reach the
  wrapper, with `.tooltip()` you must style **before** to reach your own view. The connection
  switcher's rows chained `menu_item_style` after `.tooltip(…)`, so the whole shared row style —
  `width_full()`, `flex_row()`, the gap, the horizontal padding — went on the tooltip wrapper while
  the `h_stack` underneath kept `width: auto` and sized to its own content. Its `flex_grow` spacer
  therefore never saw free space, collapsed to its 20px floor, and each connection's `host:port` sat
  a fixed 20px past its own name at a different offset per row instead of flush at the panel's edge.
  Two things hide it. The **hover background is in that same shared style**, so it sat on the
  full-width wrapper and every row highlighted edge to edge — the one cue that would have said "this
  row is narrow" was displaced along with everything else. And it presents as a *width* bug, which
  makes the panel's `min_width(400)` the natural suspect; the panel is innocent, and taffy
  right-aligns under a `min_width` perfectly well — the tree was rebuilt headlessly on taffy 0.4.4
  and the `min_width` variant lands the endpoints flush at the same right edge the explicit-`width`
  variant does. So do not go looking for a definite width here; the fix is the **order** — style the
  child, *then* wrap, then give the wrapper `width_full()` so the row has a width to resolve its own
  `width_full()` against. The wrap is `widgets::tip_when` now rather than a bare `.tooltip(…)` (see
  *A tooltip that appears only sometimes* under *UI conventions*), and none of this moved:
  `tip_when` returns the `.tooltip()` wrapper itself, so the row is still styled before the call
  and the `width_full()` chained after it still lands on the wrapper. This is the only menu in the
  app whose rows carry a tooltip, which is why it is the only one that had it.
- **`.tooltip()`'s wrapper is the flex item, so an ellipsizing text under it needs `min_width(0)`
  twice.** The same re-parenting as the bullet above, read for shrinking rather than styling:
  `Tooltip` (floem-0.2.0, `src/views/tooltip.rs:45`) is a view of its own, and it is *that* view a
  row lays out — with taffy's automatic minimum, its content's width. So
  `text(…).style(|s| s.min_width(0.0).text_ellipsis()).tooltip(…)` relaxes the minimum on a node
  the row never measures: the wrapper keeps the text's full width, the text is never handed a
  narrower box to end a line in, and the line **clips without its `…`** — which reads as an
  ellipsis that silently did nothing rather than as a layout bug. The fix is a second
  `.style(|s| s.min_width(0.0).flex_shrink(1.0))` chained *after* `.tooltip(…)`, keeping the
  text's own `min_width(0)` and `text_ellipsis` before it. `widgets::ellipsized_text` shipped
  without the second half in `2cdfbd64`, and Manage Connections' *New*/*Import*/*Export* rows and
  the Properties *Count rows* hint still overflowed in the sandboxed app under
  doubled text; `32a2cafb` added it there and on the Live Monitor's status line
  (`monitor_view.rs`, inside `h_stack((icon, line))`). Prefer `ellipsized_text` to spelling the
  pair by hand. The table designer's index hint and the editor's guard-bar message were two more
  bare spellings, and are now `ellipsized_text`. The guard bar's message also lacked the
  `min_width(0)` the error bar's has, so a long or translated warning pushed *Run anyway*, the
  held run's one mouse release, off the bar; that text is `flex_shrink(0)` now. The same holds for
  a **hide**: the Live Monitor's binary-column icon was hidden inside its `.tooltip()`, so the
  zero-sized wrapper still took the row's `gap` (next entry). The hide goes after `.tooltip(…)`.
- **A `VirtualItemSize::Fixed` is a promise about the child's height, and the child has to keep it.**
  `virtual_stack` places child `i` at `i * item_size` and derives the range it builds from the same
  arithmetic, so a row whose real height is *assembled* — padding plus a parent `gap` — drifts
  further out of place the further you scroll even when the two agree on the pitch at row zero, and
  the rows it believes are on screen stop being the ones that are. State the number once and give it
  to the row: `monitor_view::mon_row_h` is that number and `entry_row` takes it as a `.height(…)`,
  where it used to reach the same 29px through `padding_vert(7.0)` and a 2px stack gap;
  `blob_view`'s hex dump is the same shape. The corollary decides *which* lists may be virtualised
  at all — `Fixed` is honest only for a row that is always one line, which is why
  `conn_import::ROW_VIEW_CAP` took a cap instead: its rows are two lines or three, so virtualising
  them means a `VirtualItemSize::Fn` restating the layout, with no screenshot harness to catch it
  being wrong. **"Always one line" is a promise about the row's *content*, so something has to keep
  it**: `core::text::cell_line` flattens Unicode's mandatory line breaks before the text reaches
  `text()`, because a watched `TEXT` column holding an address or pretty-printed JSON shapes to two
  lines, lays out past its own box and paints across the row below — the premise was stated in three
  places and checked in none until a test held it. Both fixed-row tables keep it through that one
  function — the monitor's via `monitor::table_cells`, Compare results' via
  `result_diff::row_cells` — pinned by `a_cell_line_is_one_short_line`,
  `the_table_cells_show_each_kind_whole` and `a_table_cell_is_one_short_line`. **And a slot
  that renders nothing still owes its height**: `widgets::nothing()` is `display: none`, so
  `entry_row`'s missing-`seq` branch is an `empty()` given `mon_row_h()`; hiding it puts every later
  row in the window 29px above the space the stack reserved for it. **And a vertically-virtualised
  list of content-sized rows has a horizontal problem
  too**: only the mounted rows contribute to the content width, so it moves as you scroll, and
  anything mirroring the scroll offset — the monitor's table header — is dragged sideways by a
  purely vertical gesture. The answer is a `min_width` decided from the **data** rather than from
  the mounted rows: Compare results measures its columns once per outcome, and the monitor's
  `ColumnWidths` measures each entry as it lands and only ever widens (reasoned from the layout
  rather than observed — see `monitor_view`'s entry).
- **Padding on a virtual stack is an arithmetic error — put it on a parent.** `virtual_stack`
  derives start, end and `before_size` from `viewport.y0 / item_size` with no knowledge of any
  padding of its own, and `view_style` forces `height(content_size)`, which taffy reads as a border
  box. So a `padding_vert(8)` on the stack makes the window short by the top inset — the first row
  at the top edge is never built — *and* takes the 16px back out of the rows it does build. A
  wrapping `container` carrying the band leaves both exact, because the viewport the stack is handed
  is already translated into its own space. `monitor_view`'s change log is where this was found and
  where the wrapper now is; nothing in floem objects to the padded spelling, and nothing on screen
  says which of the two you have except a first row that never appears.
- **`s.hide()`/`s.flex()` (display none/flex) beat height/scale for a reactive show-hide** — adds/
  removes the element from layout cleanly (no clip/overflow/leftover space). Prefer it to animating
  height when you don't need the animation.
- **In-flow reveal animations are janky; only `.absolute()` transforms animate smoothly.** A
  `.transition(Height, …)` on an in-flow element reflows its container every frame and Floem only
  steps transitions on redraw ticks → ~5fps. Smooth animation here means animating an `.absolute()`
  overlay's inset/size so nothing reflows — which is what the Ctrl+K box did to grow from prompt to
  diff, until it stopped being a box (`inline_diff`) and there was nothing left to animate between.
  Either animate an absolute overlay or toggle `display`.
- **`Style::rotate` is RADIANS** (kurbo `Affine::rotate`, centre-pivoted) — pass `FRAC_PI_2` for 90°,
  not `90.0` (~14 turns → glyph vanishes). `scale` is a `Pct`. Both transition via
  `.transition(Rotation/ScaleX, …)`. Even correct, a transform-transition on a small `svg` proved
  unreliable — an icon swap is the safe chevron-flip fallback.
- **`edit_field` lets you control Escape/blur; `text_input` eats Escape — and takes no IME.** There
  is no `text_input` left in the app, and a new one would bring both back. `text_input` handles
  Escape internally (`clear_focus`, `event_before_children`, returns `Stop`), so your
  `on_event(KeyDown)` never sees it; and it never calls `set_ime_allowed` nor handles
  `ImePreedit`/`ImeCommit`, so a field built on it cannot take Chinese, Japanese or Korean at all —
  which is what moved the grid's cell editor, the last one, off it. `edit_field` is on floem's
  editor engine, which composes, and routes Escape to `on_escape` and focus-loss to `on_blur`
  (guarded to skip the mount run) — use it for discard-on-Escape / commit-on-blur (inline rename,
  find/replace, the cell editor). **The editor consumes every key it is sent**, so a key policy on
  an `edit_field` is a `FieldCfg` hook or nothing: a `KeyDown` listener bolted on from outside hears
  no key at all.
- **A key event goes to the focused view and, if consumed, to *nobody* else.** The dispatch is
  `directed` — no bubbling to ancestors, and with focus on nothing only the root view's own
  listeners run.
  **So a `KeyDown`/`KeyUp` listener on a container around the focused view hears nothing** — not
  even the keys the focused view leaves alone, which go past it to the root (`window_handle.rs`
  `event()`: `unconditional_view_event` on the focus, then `main_view.apply_event`). The SQL
  editor's Ctrl+hover underline needed a lone Ctrl's press and release, and `editor_area` is
  exactly such a container: the editor `on_event_stop`s every `KeyDown`, a lone Ctrl included, and
  takes no `KeyUp`, which therefore reaches the root alone. A view that has to follow a key it is
  not focused for reads a window-wide signal noted where the key does arrive —
  `shortcuts::modifier_keys`, written from the editor's key handler and the root's `KeyDown`
  and `KeyUp` (see `editor_pane.rs` under *Crates*). `editor_area`'s own `KeyDown` listener, which
  retires the diagnostic tip, is the instance that was already in the tree under a comment
  claiming otherwise.
  **"The root view" means the view the app's view function *returned*, and nothing inside it.**
  That distinction is the whole of a bug this survived a release with. `WindowHandle` keeps
  `main_view` = `view_fn(window_id).id()`, and the unconsumed-key fallback is
  `main_view.apply_event(listener, event)` — `ViewId::apply_event` reads the listeners registered on
  **that one id** and never walks children. The focus path is no help either: it dispatches
  *downward* from the focused view, so an ancestor is not a bubble target. `69fd7aa` wrapped
  `workspace`'s root in an outer stack to hold the eight window resize zones and left the KeyDown
  listener on the inner `root` — one level down, and therefore unreachable. Every branch in it went
  dead whenever focus was outside the SQL editor: Escape closing an open dropdown, the Tab-trap
  backstop, `NavKeys` (Ctrl+P, Ctrl+Shift+P, Ctrl+T/W, Ctrl+Tab, Ctrl+1-9, Ctrl+O/S) and the three
  panel toggles. **It looked fine because `editor_pane` answers the same keys in its own handler
  and the editor usually has focus** — the app's most-used surface masking the failure everywhere
  else. `lib::window_key_gate` now asserts the listener is attached after `chrome.resize_zones()`,
  i.e. onto the returned stack; it is crude on purpose (a precise check would have to parse the
  builder chain) and fails on exactly the mistake that was made.
  **Restoring its reachability restored the branches without restoring a guard**, so the handler
  now opens with `if modal_up() { return Continue }` — everything below it acts on the workspace
  *behind* the backdrop, and none of it should. The Escape and Tab branches sit above the guard on
  purpose: the first closes a control's popup *inside* a modal, the second steps the innermost
  modal's own focus ring, and both are modal-aware by design (so is the Escape backstop between
  them, below). A modal's focus root consumes nothing
  but Tab, so KeyDown reaches this handler whenever focus is on its root or on a button rather than
  in a text field — floem's editor is what swallowed these keys before, which is why the failure
  read as intermittent. Two were serious: Ctrl+W mid-confirm `set` the single-slot `Confirm` signal
  over `close_tabs_seq`'s parked continuation, so the chain's `resolve` was dropped and the
  remaining tabs were neither closed nor reported; and Ctrl+P mounted Find Anywhere — the modal
  layer's *bottom* entry, right for a palette raised before a modal and wrong for one raised while
  one is up — invisibly behind the modal, where its autofocusing field took the keyboard and Enter
  opened a row of a list nobody could see. The **position** is what the gate asserts, not the
  presence: after `innermost_ring_root()`, before `navkeys.handle`.
  Floem's editor consumes every `KeyDown`, so a focused `edit_field` used to swallow
  Escape and leave the enclosing modal unclosable from the keyboard. The fix is a two-step the user
  can see: Escape in a field with no `on_escape` of its own blurs it and hands focus back to the
  innermost mounted `widgets::focus_root`, and the *next* Escape reaches that overlay's handler.
  So an overlay that wants keys goes through `focus_root` (which is also what registers it) —
  `.keyboard_navigable().request_focus(|| {})` on its own leaves it deaf as soon as a field is
  focused. A field is *not* a focus root (the grid's inline cell editor deliberately isn't one).
  **The same two-step covers an overlay over an overlay, and it is the `on_cleanup` half that
  makes it work.** Floem clears `app_state.focus` when a focused view is removed and does it
  *silently* — `if self.focus == Some(id) { self.focus = None }`, no `focus_changed`, so no
  `FocusGained` lands anywhere — so a popup menu opened over a modal took the keyboard and gave it
  back to nobody: Escape closed the menu and then the modal answered nothing, while its close
  button and Cancel still worked. `focus_root`'s cleanup therefore hands focus to the new innermost
  root. That the first Escape closes the *menu* and the second the modal is the intended
  behaviour — one keypress dismissing two layers is the bug, not the fix. Don't chain your own
  `.on_cleanup` onto a `focus_root`: floem keeps one cleanup slot per view, so a second silently
  replaces both the unregister and the hand-back.
  **And an Escape nobody answered is forwarded to the innermost focus root**, from the window
  root's KeyDown listener, straight after `dismiss_open_popup` and above the `modal_up()` guard: if
  `widgets::innermost_focus_root()` is `Some`, its KeyDown listeners are run with the event through
  `ViewId::apply_event`, and a processed answer stops there. Every modal closes there, a menu panel
  dismisses there and the transaction prompt cancels there. The two-step above assumed the
  keyboard was somewhere inside the modal, and GitHub #15 was a modal it never reached: the Trigger
  editor (with no triggers), Edit table, Live monitor, ER diagram, Export and Import ignored Escape
  until something in them was clicked, because they open from the schema tree's right-click menu,
  whose return landed a tick after the modal's `focus_root` had taken focus and took it back to the
  tree (*A menu the keyboard opened gives focus back*, below — the root cause, fixed there). A modal
  with an autofocusing field raced that return and won about half the time. The backstop is the
  second half of the fix and covers what no restore can: focus on nothing (floem clears it on a
  press it does not place, and when a focused view is removed) or on a view behind the modal. It
  only ever sees an Escape the focused view left unconsumed, so a field inside a modal that answers
  Escape itself still does, and the two-step stands. `apply_event` was `pub(crate)` in Floem 0.2;
  making it `pub` is vendored Floem's sixth patch (*`floem` is vendored and patched*, above). No
  test pins the forwarding.
- **A view that takes focus *on mount* must first check that no overlay owns the keyboard**
  (`widgets::innermost_focus_root().is_some()` → don't). The query pane focuses its editor when it
  is built, which is right for every route it was written for — each is a tab the user just asked
  to look at — but the pane is also rebuilt whenever the *active tab* changes, and that can happen
  behind an open modal: deleting a connection from Manage Connections takes its tabs with it, so
  the editor stole the keyboard out from under the modal, which then had to be clicked again
  before it answered Escape. The registry is the honest test — a list of the modals that are open
  would rot on the next one added. A deferred *restore* asks a form of the same question: a menu's
  return, landing after an entry had opened a modal — *Rename alias* from the SQL editor's menu,
  Edit table and the rest from the schema tree's — took the keyboard from the dialog, so
  `widgets::menu_return` stands down when the innermost root is no longer the one it was armed
  under (*A menu the keyboard opened gives focus back*, below).
- **The results toolbar is the one `FocusRing` outside an overlay.** Every other ring belongs to a
  modal, and a ring *wraps* precisely so a modal's Tab order is a trap — which is why the workspace
  has none. Scoped to a single strip that property is the right one: the walk stays in the toolbar
  instead of falling into floem's whole-window traversal, and **Escape** is the deliberate way out
  rather than the last Tab. **F6** enters it from the grid body (`step_from` with the body's own
  non-member id, so it enters at the first control or resumes where the strip was left, and arms
  `keyboard_nav` on the way); ←/→ walk it, Tab does too, Enter/Space activates. `widgets::in_strip_button`
  is `in_ring_button` plus those two keys, and its `leave` must **defer** its focus request —
  `in_focus_ring`'s own Escape arm runs too (floem folds every KeyDown listener) and queues a
  `ClearFocus` in the same pass, so only a request landing in a later tick wins. **And it carries
  the panel's own bindings**, through a `keys` closure it consults for modified keys only
  (`widgets::strip_defers`): the panel is a *sibling* of the strip, floem carries an unconsumed
  `KeyDown` to neither an ancestor nor a sibling, so `Ctrl+Enter` on the ✓ did nothing under a
  tooltip advertising it. Two smaller rules
  ride along: a disabled control is not a ring member, so the block holding − and clone tracks the
  row selection as well as insertability or Tab would walk onto controls that had since gone live;
  and closing a menu raised from the strip has to hand focus **back to the icon**
  (`widgets::set_menu_return`), because the panel is a `focus_root` with no other root above it out
  here and its teardown would otherwise drop focus entirely — which left F6, a listener on the grid
  body, with nothing to fire on.
- **Tab navigation is ours, not floem's, and a modal's Tab order is a `widgets::FocusRing`.**
  Floem has `view_tab_navigation`, and it is unusable here on three counts: it is `pub(crate)`; it
  walks the *whole window tree*, so Tab would leave the modal for the workspace behind it; and it
  only runs when **nothing consumed the key**, while floem's editor registers KeyDown with
  `on_event_stop` and so swallows every key — a focused field being exactly the case Tab must work
  from. So a control joins the ring: `widgets::in_focus_ring` for anything but a text field (it adds
  `keyboard_navigable`, Tab/Shift+Tab, and the Escape blur), and `FieldCfg::focus` for a field,
  which carries the ring *inside* the editor's own key handler because nothing bolted on from
  outside can see a key there. It registers the **inner** editor view — the id that actually takes
  focus — and withdraws it on unmount. Order is an explicit `tabindex`, spaced, **not** registration
  order: a section built later (the SSH block, once its toggle is on) would otherwise register after
  the fields below it on screen.
  **The number follows the layout, and the spacing is what keeps it able to.** Two ways it has gone
  wrong, both invisible to whoever added the control and both found by review rather than by use.
  A control *appended by number* rather than inserted at its place walks the user backwards: the
  settings modal's statement-timeout dropdown was written last and numbered 230 while sitting
  second in its group, so Tab ran row limit → confirm → validate → back up to timeout. And a block
  that **grows into the next one** collides: the event editor's schedule sub-form reached
  `TAB_SCHED + 30` = 60, which was also its `TAB_BODY`, and `FocusRing::register` inserts *after* an
  equal index rather than erroring — so the order fell out of which control happened to build
  first. A variable-length block therefore gets a decade of its own with nothing above it until the
  next hundred (`event_editor`'s `TAB_SCHED` 30–99, `TAB_OPT` 200–999), and
  `event_editor::tests::no_two_controls_claim_the_same_tab_stop` is the pin. A picker joins through
  `settings::in_ring_picker`, which is the whole apparatus below in one place; `focusable_dropdown`
  (settings' `Copy` picker) and `table_designer::focusable_owned_dropdown` (the designer/editors',
  since a table name isn't `Copy`) are both thin wrappers over it, and neither has an un-focusable
  sibling left — a second one would only be a way to leave a control out of the ring by accident.
  **The modal's root is what *enters* the ring**, via `widgets::focus_root_with_ring`. A key goes
  to the focused view and, unhandled, only to the window root — and a modal opens with focus on its
  own `focus_root`, which is also where Escape hands it back. Without a Tab handler there the ring
  could only be joined by clicking a control first, so Tab did nothing at all on a freshly-opened
  modal. The root isn't a ring member, so `step_from` falls back to the ring's **remembered cursor**
  (`FocusRing::remember`, set by every Escape blur and every step) and, on a ring that has been
  nowhere yet, starts at the first control (the last, for Shift+Tab). The memory is not a nicety: a
  `tab_indents` field can only be left by Escape, so re-entering at position 0 made every control
  after it unreachable by forward Tab.
  **The window root is the backstop** (`lib.rs`'s root KeyDown, beside the Escape branch). A plain
  Tab reaches it only when nothing in the overlay consumed the key — focus is on a dropdown's popup
  list, or on *nothing*, which is what floem leaves behind whenever a focused view is removed or an
  unfocusable row is clicked — and floem's own fallback then walks the whole window tree. So the
  root steps `widgets::innermost_ring_root()` instead, which is why `FOCUS_ROOTS` carries
  `(ViewId, Option<FocusRing>)` rather than a bare id.
  **Every cleanup that can run while focused hands the keyboard back** — `focus_root`,
  `in_focus_ring` and `edit_field` all do, because floem clears `app_state.focus` *silently* on
  removal (no `focus_changed`, so no `FocusGained` lands anywhere) and the modal around the
  departing control is then deaf to Escape and Tab alike. One click on the designer's list `+` was
  enough. "Was I focused?" is mirrored into an `Rc<Cell<bool>>`, never read back from a signal that
  may already be disposed. Note floem keeps **one** cleanup slot per view, so a control needing its
  own teardown passes it to `widgets::in_focus_ring_with` rather than chaining a second
  `.on_cleanup`, which would silently replace the unregister *and* the hand-back.
  **And there is somewhere to hand it to outside a modal**, which there was not:
  `innermost_focus_root()` is `None` in the workspace, so a ring control that removed itself while
  focused — the results toolbar's ✓/✗ pressed from the keyboard does exactly that, since the commit
  block's `dyn_container` key is what they change — left *nothing* focused, and from there the grid
  answered no key at all, F6 included, until the user clicked a cell. `hand_keyboard_back` now falls
  back to `widgets::set_keyboard_home`: an action the workspace registers, rather than an ancestor.
  It has to be a place and not a root — a modal's root carries the key handlers for its own contents,
  while the workspace's carries almost none (the arrows, `Del`, `Ctrl+Enter`, `Ctrl+F` and `F6` are
  all listeners on the results grid's body) — and a closure rather than a `ViewId`, because a grid
  rebuilds per result and a stored id names a view that has gone. `grid_view` registers
  `refocus_grid` where it sets `focus_id`; `refocus_grid` reads that signal with `try_get_untracked`,
  since the home outlives the grid that registered it.
  **And it reads it *inside* its deferred tick**, which is the other half of the same rule and was
  missing for a release: the read was hoisted out and the id captured, so the hand-back landed one
  tick later on whatever had been there when it was *scheduled*. That is exactly the window the ✗
  opens — `discard_edits` notifies `new_rows`, the body's `dyn_container` key, so the body the id
  named was already gone — and floem's focus request has no existence check
  (`UpdateMessage::Focus` assigns `app_state.focus` whether or not the id resolves), so the keyboard
  was parked on a removed view and **every** key was dropped until a click. The ✓ escaped it only
  because `commit_busy` isn't in the body's key. Defer the *resolution*, not just the request —
  `grid_toolbar`'s `focus_icon` says the same thing and resolves by tabindex.
  **And a deferred hand-back stands down for a claim taken after it** — `widgets::claim_keyboard`,
  quoted back through `keyboard_claim` / `keyboard_claim_unchanged`. Being deferred is what put
  `refocus_grid` in a race with the *other* immediate timer the same gesture schedules:
  `edit_field`'s autofocus. Pick **Optimise** (or any of the three AI fixes) off the editor's
  right-click menu and one update pass opens the Ctrl+K bar — queueing its prompt field's autofocus
  — and tears the menu panel down, whose `focus_root` cleanup calls `hand_keyboard_back`, which
  finds no other overlay and queues the workspace's home. Two timers due immediately, scheduled
  microseconds apart, and whichever lands last owns the keyboard: when that was the grid, the bar
  stood open with the keyboard behind it and Escape cleared a cell selection instead of closing it.
  It read as intermittent — about one opening in three — because it *was*. So an autofocus, and the
  Ctrl+K bar's own two hand-offs to the editor, **claim**; `refocus_grid` snapshots the claim when
  it is scheduled and refuses to fire if it has moved. A generation, not a "focus is held" flag,
  because floem's `AppState::focus` is private to it and a flag of our own would need clearing by
  every path that drops focus, floem's included; a counter answers the only question a deferred
  mover has — *is my hand-back still the latest word on the keyboard?* — and settles the race in
  both orders, which is the property the test pins.
  **Every deferred hand-back to the editor must say which it is — claim, or stand down — and that is
  a workspace gate now.** Focus is handed back a frame late in several places (the run menu, Ctrl+K
  twice, the find close, the goto close and submit, the completion row's click) because the focus
  floem takes is cleared during the frame's own dispatch, so asking for it back inside the handler is
  undone by the frame that stole it. Deferring makes the request *land*; it does not make it *win*,
  and two immediate timers queued in one pass are decided by whichever lands last. A **mover** — the
  user did something whose whole point is that the editor ends up focused — calls `claim_keyboard`;
  a **hand-back** reads the generation instead, through `keyboard_claim_unchanged` or, for the
  tab-mount autofocus, `innermost_focus_root` (an overlay owns the keyboard while it is up). A block
  that does neither wins or loses by timing. Two of the five siblings claimed and three did not,
  which is the failure this is one gate rather than five notes about: a hand-back written without
  looking at the other four. `source_gate::tests::a_deferred_editor_hand_back_claims_or_stands_down`
  walks every `exec_after` in the workspace that holds a `request_focus()` on an `editor_view_id` and
  fails on one that does neither, with a floor so it cannot pass by matching nothing. It is scoped to
  the **editor's** id deliberately: the grid's keyboard home is the other side of the same protocol
  and reads the generation rather than claiming it, and a picker's mount autofocus is a third
  question — a rule wide enough for all three would have to be "or does something else", which is not
  a rule.
  This is the durable form of what
  `set_menu_return` does for one case: fixing the sites one at a time is what produced the tree's
  cursor regression below.
  **And a press that dismissed a menu suppresses the hand-back — but only when that press placed
  focus.** `widgets::begin_pointer_dismissal` is marked by the workspace root's `PointerDown` before
  it closes the menus and read by every `focus_root` cleanup through `hand_back_wanted`, because
  floem has already put focus where the click landed: right-click in the SQL editor, then left-click
  back into it, and the panel's teardown handed the keyboard to `refocus_grid` while the caret sat
  where the press put it, so typing inserted nothing and the arrows moved the cell selection. A flag
  and not a `claim_keyboard`, because the cleanup is scheduled in `process_update` after this
  handler has returned and a claim taken here would sit *inside* the snapshot `refocus_grid` took;
  cleared on the next tick rather than at the end of the handler, so the teardown in between can
  still read it.
  **The premise had no source, and outside a modal it was simply false** (S19-L1-01). Inside one a
  press always lands on something navigable — `focus_root_inner` makes the modal's own root
  navigable and its view is `size_full()`, so an ancestor takes the keyboard on the unwind — but the
  workspace has no such ancestor and most of what it shows takes no focus at all: the status bar's
  text, a panel title row, the tab-strip background, the results header. Closing the results
  toolbar's Copy menu by clicking the row-count segment left **nothing** holding the keyboard, and
  the suppression made that permanent — no arrow, no `Del`, no `Ctrl+Enter`, no `Ctrl+F`, not even
  the `F6` that would have got back into the strip, until a cell was clicked. Reproduced and fixed
  in the running app: before, ↓ after the gesture did nothing; after, the selection moves. So a
  second one-tick flag answers the question the first one assumed — `note_pointer_focus` /
  `pointer_placed_focus`, mirroring the pair above — and `begin_pointer_dismissal` marks nothing
  unless the press placed focus.
  **The views that take the keyboard are what report it, because floem will not.** `app_state.focus`
  is `pub(crate)` to floem, no `ViewId` accessor answers it, and the `FocusGained` that would is
  dispatched *after* the root's handler returns (`window_handle.rs:361`). What makes a view's report
  reliable is floem's own ordering: focus is cleared at the top of every `PointerDown`
  (`window_handle.rs:227-231`), the deepest-first child walk gives it to the first
  `keyboard_navigable` view under the point (`context.rs:176-181`, `AppState::update_focus` being
  first-wins), and a view's own listeners run **last** (`context.rs:405-423`) — so every view under
  the press has reported by the time the root asks. `widgets::takes_pointer_focus` is the decorator
  to spell beside every `.keyboard_navigable()`, and it is `on_event_cont` and never
  `on_event_stop`, or the press stops reaching the root that closes the menus. It wraps
  `focus_root_inner`, `key_pressable` and `in_focus_ring_with` — which is most sites by inheritance
  — plus the grid body, `pick_field` and the set chip, the schema tree, the terminal surface and
  `edit_field`'s editor (the grid's inline cell editor among them); the SQL editor calls
  `note_pointer_focus()` directly at the top of the `PointerDown` handler it already had.
  `menu_return_gate::every_navigable_view_reports_the_press_that_focuses_it` fails on a navigable
  view with no report above it; what it cannot see is floem's own focusable views, which carry no
  `.keyboard_navigable()` for a needle to match and are a census kept by hand in
  `takes_pointer_focus`'s doc.
- **A view that must survive its own click cannot be keyed on a signal a `PointerDown` will
  change.** An `on_click`/`on_click_stop` fires on the `PointerUp`, so the view has to still be in
  the tree when the release lands — and floem takes `app_state.focus` on the press
  (`window_handle.rs:227-231`), so any `FocusLost` effect that press causes runs in the middle of
  the same gesture. The completion popup was both halves at once: it is a `dyn_container` keyed on
  `(comp.open, comp.items)` and the SQL editor's `editor_view_focus_lost` effect closed `comp.open`
  unconditionally, so a press on a completion row tore that row out of the view tree before its own
  release. Clicking a suggestion inserted **nothing** and left the editor with no keyboard focus:
  `accept_completion` never ran, and neither did the deferred `claim_keyboard` hand-back the row's
  click carries — one of the places the deferred-hand-back rule above enumerates, written for the
  moment *after* an accept and assuming the accept had happened, so it sat green over a distinct
  defect earlier in the same gesture. Keyboard accept (↓ then Enter) worked throughout, because
  nothing takes focus away mid-keystroke, so the popup looked healthy to anyone driving it from the
  keyboard.
  **The fix is a third one-tick press flag**, beside `begin_pointer_dismissal`/`hand_back_wanted`
  and `note_pointer_focus`/`pointer_placed_focus` above: the popup container reports its own press
  through `widgets::note_pointer_on_completion`, and the editor's blur closes `comp.open`/`comp.sig`
  only `if blur_dismisses_completion()` — which is still true of the presses that *should* dismiss
  it, on the schema panel, the terminal or another tab. The report is `on_event_cont` and never
  `on_event_stop`, for the reason `takes_pointer_focus` gives: the press must go on reaching the
  workspace root that closes the menus. And it is reliable for the reason that decorator is — a
  view's own listeners run during event dispatch (`context.rs:405-423`) while `FocusLost` is
  dispatched from `focus_changed` *after* that loop returns (`window_handle.rs:361`) — so a press on
  the popup has always reported by the time the blur asks. `cell_editors::take_calendar_press` is
  the same shape one layer down, for the in-cell calendar.
  `widgets::tests::the_completion_popup_reports_its_press_and_the_blur_reads_it` is a **source**
  gate rather than a unit test, because neither half is wrong on its own: the predicate was not
  wrong, it did not exist, and the defect was the composition of two views. Each half asserts the
  surrounding term still exists (`on_click_stop` in `completion.rs`, `editor_view_focus_lost` in
  `editor_pane.rs`), so a rename cannot make it pass by matching nothing.
- **A `.hide()`n control is still in the Tab order** — `hide()` is `display: none`, so the view is
  still in the tree and still registered in the ring, and Tab moves focus onto something nobody can
  see. Every engine-conditional block that was built-and-hidden is therefore now **built
  conditionally**: import's CSV and Excel settings, the designer's MySQL-only engine/collation and
  `ON UPDATE` and PostgreSQL-only index method/predicate, the view editor's MySQL options, PG
  recreate toggle and SQLite-only column list, the trigger form's `Fires`/`When` and SQLite-only
  `Of columns`. Settings' AI assistant section applies it to a setting rather than an engine: with the
  assistant off, nothing below the *Agent CLI* choice is built (`settings.rs`).
  Nothing is lost by rebuilding — each of those binds
  straight to a draft or a persisted signal — and a control an engine can't express shouldn't be
  reachable at all, which is the same call `trigger_editor`'s per-engine form already made.
  **The else-arm is still `display:none`, via `widgets::nothing()`** — taffy skips a `display:none`
  child when it distributes `gap` but counts a zero-sized one, so a bare `empty()` arm leaves a
  whole `form_gap()` of dead space where the block would have been. The rule is about *controls*: an
  arm with nothing inside it has nothing to be Tab-reachable. Where the conditional is a
  `dyn_container`, the hide goes on the **container** — that is the flex child, not its inner view.
- **Buttons are in the ring too, and Space or Enter presses them — but there is no default Enter.**
  Every button a modal has goes through `widgets::in_ring_button` (which the builders —
  `action_button`/`action_button_icon`/`action_face`/`control_button`/`control_button_enabled`/
  `field_button`/`row_button` and `modal_title`'s ✕ — call for you; the ring parameter is
  *required*, so a modal button that isn't reachable won't compile). Enter in a *field* fires
  nothing: the DDL preview's Apply is an
  irreversible `ALTER`, and a key meaning "newline" in one control and "apply the plan" in another
  is the shape of defect the ring's own review was full of. **A disabled button is not a stop** —
  it keeps its place on screen (which action is affirmative shouldn't move as a form becomes valid)
  but the keyboard walks past it, since its click handler is inert anyway.
  That is a **build-time** decision — `enabled` is a plain `bool`, not a signal — so a control whose
  availability changes while the modal is open has two spellings and they are not interchangeable.
  Either the block holding it is rebuilt when availability changes (the results strip's − and clone,
  which track the row selection as well as insertability, or Tab would walk onto controls that had
  since gone live), or the control is registered unconditionally and the action guards itself. The
  Live Monitor's Clear and Export take the second: they *dim* on an empty log rather than leaving
  the ring, because rebuilding would reflow the sub-header the moment the first change lands, and
  Enter on a dimmed one does nothing because each closure re-checks the log. Passing `has_log()`
  into `enabled` is the tidy-looking edit that breaks this — it is read once, at build.
  The results toolbar's copy, save and chart icons take the second too, since they became gated on the
  result having rows: ring membership is unchanged (`in_strip_button(…, true, …)`), the icons dim on
  a 0-row result, and the `Rc<dyn Fn()>` each shares between its click listener and its Enter/Space
  arm carries the refusal — so the pointer and the keyboard refuse together, where dimming the face
  alone would have left Enter opening a menu drawn as unavailable (*Data grid*).
  **The ring member is a wrapper `in_ring_button` builds, never the caller's own view**, and that
  is a correctness rule rather than a layout preference. Two things resolve by exact `ViewId` with
  no descendant propagation, and they were resolving to *different* ids depending on each call
  site's decorator order: floem fires `EventListener::Click` on the **focused** view for any
  physical Enter/Space and then folds every registered `KeyDown` listener, so registering a face
  that already carried `on_click_stop` made the ring's own arm a **second** activation (one Space
  added two columns, opened two file dialogs, started two bulk imports); and `.focus(…)` resolves
  by exact id too, so a face decorated with `.tooltip()` — a fresh `ViewId` — put an id in the ring
  that carried no outline. A caller therefore styles only the *face* (padding, hover, the click
  listener, its tooltip) and never applies `button_focus_ring` itself;
  `widgets::a_ring_button_registers_a_wrapper_not_the_face_it_was_given` pins which of the two views
  is the one registered.
  **Its ring-less sibling `widgets::key_pressable` splits that same pair the other way, and has to** —
  worth knowing before adding a click listener near either. It builds its own container, so a
  caller's `on_click_stop` would sit *inside* the focus ring rather than on it; it therefore binds
  the click as well as the KeyDown arm, over one `Rc`'d action. Leaving the pointer to the caller
  here is what shipped the row panel's *Set NULL* and the activity panel's lock-wait Kill dead to a
  mouse click, and `widgets::key_pressable_gate` now holds the helper to both listeners and rejects a
  caller that binds the click a second time (one press, two actions).
  **Both listeners must sit on two different views, and that is the correctness half rather than a
  layout one.** Chained onto one container they ran the action *twice* per keypress, by the exact
  mechanism above: floem applies `Click` to the focused view for any physical Enter/NumpadEnter/Space
  and **discards the result**, then folds that same view's KeyDown listeners without short-circuiting.
  The row panel's blob affordance takes the caret on mount when its column is the first editable one,
  so one **Space** on it issued two `view_blob` reads of up to `FETCH_CAP` — the second cancelling
  the first's token mid-transfer, on a Manual tab's pinned session — and the activity panel's
  lock-wait **Kill** raised `Confirm` twice into a single-slot signal.
  `key_pressable_gate::the_click_and_the_key_arm_are_not_on_the_same_view` pins the split, and
  `::a_sparkle_action_can_be_pressed_from_the_keyboard` holds the third button family
  (`widgets::sparkle_action`) to one of the two helpers.
  Order is `NAV_TAB` → `LIST_TAB` → the form (10, 20, … within a section, by 100 between them, up
  to `FIXED_TAB_END`) → `VALUE_TAB` + `i * ROW_TAB_STRIDE` for a growing list → `ACTION_TAB` for
  the footer → `TITLE_CLOSE_TAB` for the title bar's ✕ (last, since
  it is the same action the footer's Cancel already offers), and that chain is asserted at
  **compile time** in `widgets.rs` (`const _: () = { … }`). It has already caught one regression:
  adding `ROW_TAB_STRIDE` cut the footer's headroom tenfold the day it landed.
  The compile-time chain relates *constants* only and cannot see a number a control actually
  claims — the import modal's mapping rows claimed `100 + i * 10`, a growing block based in the
  fixed range, with the build green. **A per-registration check cannot close that gap, and trying
  it caused a crash**: `FocusRing::register` is handed one index at a time, so a legitimate fixed
  control at 200 (Settings' row-limit dropdown) and the first row of a misplaced block at 200 are
  indistinguishable there — a band `debug_assert` asserting fixed controls end at 110, a number
  taken from a stale comment while Settings really reaches 310, panicked the app on correct code.
  `register` therefore asserts only the **ceiling** (nothing past the ✕). What covers the real
  rule is that every growing block reads `VALUE_TAB + i * ROW_TAB_STRIDE` — four sites, greppable
  — and `ring_tests::every_band_the_app_uses_registers_cleanly`, whose list is the app's real
  indices rather than an idealised one. Check a guard against reality before checking it against
  intent.
- **A modal's click-to-dismiss goes on `widgets::dismiss_layer`, never on the `focus_root`.** Floem
  fires `Click` on the focused view for Enter and Space, and a modal opens with focus on its own
  root — so `.on_click_stop(close)` there meant **Space closed the modal**. On the Live Monitor
  that also stopped the poll and emptied the change log (deletes included, which is the one record
  of a row that is gone); on the confirm dialog it answered `false` to a question nobody had read.
  The layer is an absolutely-positioned sibling built *before* the panel, so the panel stays on top
  of it. The transaction prompt deliberately has none: clicking away from a question about
  uncommitted writes is not an answer.
  **Which modals close on a backdrop click** is the user's decision, written here so it is not
  asked again: **a modal that could lose data on a stray click has no click-away** — only its ✕
  and Escape close it — and every other modal closes on a backdrop click through `dismiss_layer`.
  Escape stays in both because pressing it is deliberate; a click on the backdrop often is not.
  **Every modal closes on Escape, every time** — the user's decision on GitHub #15, raised by
  modals that ignored it until clicked. The only exceptions are a busy modal's, decided by
  `widgets::exit_action`: Escape *stops* cancellable work in flight (a dump, an import, a script,
  a DDL apply that rolls back as a whole) and the modal stays open to report it, and it is ignored
  (`ExitAction::Ignore`) only while `ddl_preview` applies DDL that cannot be rolled back. The transaction prompt answers Escape with
  *Cancel* — it abandons the action that raised the prompt and leaves the transaction as it was;
  it used to swallow the key — and so does its ✕, which it has had only since GitHub #25; it still
  has no click-away.
  Each root in the no-click-away set carries a one-line comment pointing at this passage, and each
  is there for a named reason: a **draft with no other copy** — `table_designer`, `view_editor`,
  `trigger_editor`, `routine_editor`, `event_editor`, `object_editor` and `database_editor`, whose
  `close` sets the target to `None` and the next open reseeds the draft, and
  `snippet_edit`, whose name, abbreviation and body are saved only by Save; **selections nothing
  else holds** — `connection_import`'s ticked list, of which nothing is written until Import; a
  **record nothing else holds** — `monitor_view`, whose change log is the one copy of what a
  deleted row held, and which reopening starts afresh, so a stray click discarded it without the
  confirmation its own Clear asks for (`discard_needs_asking`); a
  **running operation a close would cancel** — `dump_view::dump_overlay` and `script_view`, and
  `import_view`, which holds a column mapping as well; and the transaction prompt above.
  `dump_view::export_progress_overlay` has neither a title row nor a click-away **by design**: it
  is a one-button progress box whose only exit is its Stop/Close decision, and Escape routes
  through that same decision. `ddl_preview` and `blob_view` were *given* a click-away under this
  rule: a replacement loaded into the blob viewer is staged into the grid's edit buffer the moment
  it lands, so closing it only cancels a read. **The preview's is narrower than its `exit`**: an idle
  plan is regenerated from the editor left open behind it, but the first spelling routed the
  backdrop through `exit`, so on PostgreSQL and SQLite a stray click mid-apply was a Stop that
  rolled the apply back, and after a failed MySQL apply it closed the only copy of "statement 3 of
  5 failed, 2 already committed". `backdrop_action` closes only an idle preview with no error on
  screen; ✕, Escape and Stop keep `exit`
  (`the_backdrop_closes_only_an_idle_preview_with_nothing_to_report`). `account_editor.rs`'s account and
  grant editors are in the no-click-away set too, through the one `modal_shell` they share: each
  holds a draft — a name, a password, grants — that `close` resets to its default, so a stray click
  would discard everything typed since the open. The form does re-seed from its target on open,
  and its comment once took that to mean "nothing is lost"; it is true of what was saved and not
  of what was typed, so a re-seed is no argument for a click-away.
- **An overlay's `inset(0.0)` style and its content must read the *same* predicate** — one closure,
  used twice, never two spellings of "is this showing". A dropdown whose content is conditional on
  more than its open flag (`open && !db_nodes.is_empty()`, the rule that keeps an empty menu off
  the screen) but whose style still tests `open` alone stretches a transparent, handler-less
  container over the whole window with **no panel and no `dismiss_layer` on it**. It swallows every
  click in the app — including the one on the trigger that would close it — and the Escape handler
  it would need lives inside the content that never mounted. The window repaints perfectly and
  answers nothing, so it reads as a hang and ends in a killed process; the active-database menu
  shipped exactly that, reachable from any connection with no databases. The trigger **guards its
  own launch** in the same step (the `widgets::accept_launch` rule, applied to a menu), and the
  overlay clears a flag whose panel can't render, so a later load can't pop a menu nobody asked for.
- **The QUERY toolbar's database selector names a database only while the connection has loaded
  it** (`core::schema::shown_database`), and reads "No database" otherwise. A tab's `database` is
  *saved state* that outlives the connection being reachable — it has to, or a server coming back
  would leave every tab pointing somewhere new — but a connection that loaded nothing shows an
  empty tree and a "Disconnected" header, and a toolbar still naming a database is the one surface
  claiming otherwise, when that name can't be listed, selected or read. So the rule decides the
  **label**, and the binding is left alone for a recovered connection to restore from. Membership
  rather than "the list is empty", so a database dropped server-side stops being named the moment a
  reload no longer carries it — the same test `set_active_db` already applies before selecting one.
  A **button's** focus signal is an outline, painted in `.focus`, *not* `.focus_visible` — floem
  gates `FocusVisible` on `app_state.keyboard_navigation`, which only its own `view_tab_navigation`
  ever sets, so a `focus_visible` rule on a ring member usually never fires at all. A **group**
  (below) deliberately shows nothing.
  **`widgets::keyboard_nav` is the app's own `:focus-visible`**, and the reason the ring can be
  `accent` rather than something apologetic. A focus outline is information the *keyboard* needs;
  under the pointer it marks what you just clicked, so a ring bright enough to be useful under Tab
  is noise under the mouse — which is why it used to be `field_border_active`, `#303453` against
  the dark panel, a shade off the surface it sat on. The flag is **set in `FocusRing::step_from`**
  and **cleared on the root's `PointerDown`**, and both halves are the way they are for a reason:
  every keyboard-driven focus change in the app is a Tab through the ring, so `step_from` *is* the
  definition (no key allowlist to keep in step, and typing in a field can't arm it), while a key
  listener on the window root — the obvious spelling — would have missed the only case that
  matters, since floem bubbles a key to the root **only if nothing consumed it** and Tab is exactly
  what the ring consumes. `FocusRing::focus_at` and `hand_keyboard_back` deliberately leave it
  alone: both move focus on behalf of something the user may have reached either way (a dropdown
  handing the keyboard back, a field unmounting under them), so the last real gesture stands.
  It rides in a detached-scope `thread_local` signal, the shape `window_size` and `pointer_released`
  already use. Buttons, the Settings toggle switch and the colour swatches all read it.
  **A view that swallows a pointer-down inherits the clearing**, and forgetting that is a live bug
  rather than a stray outline. A menu trigger must stop the press so the root's "close on down"
  doesn't fire for the click it is about to act on — and stopping it takes the root's `keyboard_nav`
  clear with it. Left set, `set_menu_return` is armed for a menu opened **by mouse** (the opener asks
  the flag in the `Click` that follows), so closing that menu drags the keyboard back to the trigger
  and pulls the arrow keys off whatever had them — the grid's cell navigation, in the case that
  motivated the conditional slot. `widgets::menu_trigger_press` is that handler, and every trigger
  installs it in place of a bare `|_| {}`: the grid's Copy / Save / AI icons, the status-bar
  segments, the schema panel's eye and gear, the Server Activity clock, the connection switcher, the
  QUERY toolbar's database selector, and `suggest_chevron`. The *panels* that swallow a
  press keep the bare closure, and the distinction is the point: they do it so a click inside isn't
  read as a click away, and where focus goes when one closes was settled by `set_menu_return` when
  it opened, not by the press that dismisses it.
  **Gate the ring on it and nothing else — least of all the answers to floem's own defaults.**
  `settings::themed_toggle` briefly put its whole focus block behind the flag's early return, and
  the flag being false is exactly the mouse path, so a click on the switch got floem's
  `ToggleButtonClass` styling back: a 1px `#8c8c8c` border (grey around the dark off-track,
  invisible on the lit on-track), the faint magenta ring users reported, and a near-white
  `#eae6ec` track when focused *and* hovered. All three come from `floem-0.2.0`'s
  `theme::default_theme` — the magenta is `border_color(#724a8c)` applied on plain **`.focus`**, not
  `.focus_visible`, which is why the pointer saw it and the keyboard never did. The border is
  answered with `.border(0.0)` rather than a transparent colour: the handle is positioned from
  `layout.size`, so the width costs no geometry, and a border that cannot paint is a border floem's
  `.focus` rule cannot colour. `button_focus_ring`'s identical early return is safe only because it
  decorates a bare wrapper container, and floem's border/focus defaults reach widgets through
  **classes** (`ToggleButtonClass`, `ButtonClass`, `TextInputClass`), never plain containers — so a
  view sitting on a classed widget has to answer those defaults on every path, not inside a branch.
  **And a ring must set `focus_visible` to the same outline as `focus`, or answering those defaults
  erases it.** Floem applies the `Focus` map first and the `FocusVisible` map after
  (`style.rs`'s `apply_interact_state`), gating the second on `app_state.keyboard_navigation` — which
  latches **globally** the first time floem's own Tab traversal runs anywhere in the window and is
  reset by nothing but a pointer press onto a navigable view. So a control that suppresses
  `focus_visible` (as every one here must, to kill floem's 3px magenta) and paints its ring only under
  `focus` shows *no focus indication at all* from the first Tab onward. `button_focus_ring` sets the
  pair; `settings::toggle_focus_ring` — extracted for exactly this — now does too, and
  `widgets::ring_tests` asserts over the composed `Style` that a ring's `FocusVisible` outline is
  never narrower than its `Focus` one, for both builders.
  **The outline follows the face's corner radius, and `in_ring_button` has to be told it.** Floem
  strokes an outline at *the painting view's* `border_radius` (`view::paint_outline`), and the ring
  member is a wrapper around the face — so with no radius on the wrapper every rounded button in
  the app wore a **square** ring. `widgets::ACTION_RADIUS`/`CONTROL_RADIUS` are read by both the
  face's own style and the wrapper, so the two cannot drift; icon buttons pass `0.0`, their faces
  being genuinely square.
- **A group of like things is *one* Tab stop, and arrows move within it.** Manage Connections'
  colour swatches (`connection_form::color_picker`) and connection list, Settings' section list
  (`settings::section_list`), the designer's section
  strip, and the designer/trigger item list (`table_designer::list_pane`) each take a single ring
  slot: Tab reaches the group, Left/Right or Up/Down move inside it, Tab leaves. `widgets::nav_group`
  is the shared one (the swatches and `list_pane` predate it and carry their own scrolling and
  chrome). **A list whose rows act when chosen** — *Copy to*'s target picker, *Find usages*' rows —
  is `widgets::pick_group` over it: the arrows move a cursor (`cursor_row`, `row_selected`,
  scrolled into view a tick later) rather than a selection, Enter or Space picks the row under it,
  the first arrow lands on the near end as `ring_step` enters a ring, and `FocusLost` drops the
  cursor. Eight swatches or twenty columns as individual stops would
  make crossing a pane the user is only passing through cost twenty keypresses.
  **A `nav_group` paints no focus indication**, unlike a button: it wraps a whole bar or pane, so
  an outline around it reads as a stray border — and it doesn't need one, because what the arrows
  move is already highlighted, so the first press announces the focus by moving the thing you were
  looking at. Floem's own 3px magenta ring is still suppressed (`focus_visible → outline(0)`),
  since `keyboard_navigation` latches globally once floem's traversal has run anywhere.
  `list_pane` makes the opposite call and recolours the border it already has — it has one to
  recolour, which costs no layout; that is the test, not a house style. Two details each
  had a reason: the **ring wraps and these clamp** — wrapping is what stops Tab escaping the modal,
  while a selection that jumps from the last column to the first is only a surprise (the swatches
  *do* wrap, being a short fixed ring where the ends are visibly adjacent) — and the group's focus
  indication must cost **no layout**, so the list recolours the border it already has and a swatch
  takes an `outline` (painted outside the box) rather than a border that would nudge the row along.
  The swatch cursor is a signal of its own, cleared on `FocusLost`, so the halo never outlives the
  focus it stands for — and it is gated on `keyboard_nav` like a button's ring, because it says
  which swatch the *arrows* will land on, while which one is **chosen** is already said by the
  white border it wears.
- **In a field holding *code*, Tab is typing.** `FieldCfg::tab_indents` suppresses only the ring's
  step-away, so Tab still *arrives* at the field and floem's own `InsertTab` then runs; Escape is
  the way out, blurring to the `focus_root` whose Tab re-enters the ring **after this field** (the
  blur calls `FocusRing::remember`, which is what makes a mid-ring placement legal). Set on the
  trigger body,
  the PG function body and the view editor's `SELECT`. Deliberately not on prose (the AI settings'
  custom instructions) or on the DDL preview's read-only script box, where there is no indent to
  type and Tab moving on is simply better.
- **A field offering suggestions takes Tab only while its ghost is showing.** `lib.rs`'s
  `tab_action` is `edit_field`'s whole Tab precedence as one decision — `on_shift_tab` for the
  shifted key ahead of everything (`TabAction::BackCallback`; the grid's cell editor hopping back a
  cell, which without it fell to floem and typed a tab into the value), then `on_tab`, then a showing
  `FieldCfg::suggest` ghost (`TabAction::Accept`), then `tab_indents`, then the ring, then floem's
  `InsertTab` — and the ghost arm is gated on a ghost being *on screen*, not on the field having a
  list: with none showing, Tab means what it meant before the field had suggestions. Accepting
  keeps the keyboard in the field, so a second Tab moves on; `text::field_suggestion` answers
  `None` once the text already is an option, which is what stops an exact `int` being "completed"
  to `integer` by the key meant to leave. **Shift+Tab never accepts** — a completion is a forward
  motion — and walks back past the ghost. `field_key_tests` pins the order
  (`a_showing_ghost_is_accepted_before_the_ring_steps`, `shift_tab_never_accepts_a_completion`,
  `a_shift_tab_handler_answers_shift_tab_only`).
- **A popup that takes the keyboard can only be closed from the *window root*.** The corollary of
  the directed dispatch above, and it is not obvious: a modal's Escape handler runs because its
  `focus_root` is the *focused view*, not because it is an ancestor — there is no bubbling in
  between. So while a dropdown's popup is up, the box that owns it, its ring, and the modal around
  it are all bypassed, and floem's list returns `Continue` for Escape, so the key reaches the root
  and finds nothing. `widgets::set_open_popup`/`dismiss_open_popup` is that slot (thread-local: only
  one popup can be up), consulted from `workspace`'s root KeyDown fallback. Escape then peels one
  layer per press — root closes the popup, `in_focus_ring` blurs the control, the modal closes.
  **The slot is tagged with a `PopupToken`, and a control may only clear its own entry.** Floem
  queues B's open during dispatch and A's close at the end of the *same* event, so opening a second
  dropdown over an open one ran `set(B)` and then A's clear — and an untagged clear emptied the slot
  under B, after which Escape did nothing at all. The build-time run of each dropdown's effect (with
  `open == false`) was a second way in, so merely constructing one wiped it. `clear_open_popup` is
  also what the dropdown's `in_focus_ring_with` teardown calls: a control disposed with its popup
  still up (click the backdrop) otherwise left a closure over a dead scope in the slot, and the next
  Escape *anywhere in the app* was swallowed by it.
- **Floem's `Dropdown` toggles on `KeyUp`, which reopens it after a keyboard accept.** Enter is
  *pressed* in the popup (accepting, which closes it) and *released* over the box focus has just
  returned to, so the release opened it again — no delay fixes that, the release is tens of ms
  later. `settings::focusable_dropdown` therefore takes the open state over: `disable_default_event`
  drops floem's KeyUp toggle (its only use), `show_list` drives the state from a signal, `on_open`
  mirrors floem's own opens/closes back into it so a pointer click can't leave the two disagreeing,
  and opening moves to KeyDown (Enter/Space/Up/Down). Also: `on_accept` is a **single slot**, so
  overriding it means repeating whatever the previous one did.
- **A list item's `selected` style is the keyboard's cursor — don't neutralise it.** The dropdown
  popup blanked floem's selected tint (the resting highlight for the value in effect is applied by
  the row builder instead), which left arrowing through an open dropdown with nothing to look at:
  you could count keypresses and press Enter, but not see what you were about to choose.
- **`text_editor_keys` inserts a typed char *unconditionally* — your handler's `CommandExecuted`
  return is ignored for plain character keys.** Floem's `editor_content` KeyDown listener discards the
  handler's result and then, if `mods` (minus SHIFT/ALTGR) is empty, calls `receive_char(c)` for any
  `Key::Character`. So the existing custom edits (Tab indent, Ctrl+/, Ctrl+D…) only work because they're
  on keys that *don't* trigger that path (Tab is `Named`; Ctrl-combos keep a modifier). To fully take
  over a **plain character** (auto-close pairs), suppress the built-in insert by setting the editor's
  own `ed.read_only` true for the rest of the dispatch and restoring it on the next tick via
  `exec_after(ZERO)` — `receive_char` early-returns on `read_only`, nothing else reads it, and the
  handler→`receive_char` step is synchronous so there's no race/flicker (see the auto-pair block in
  `editor_pane`). Named keys (Backspace) never hit `receive_char`, so returning `Yes` is enough there.
- **Only *typing* may open the suggestion popup** (`completion::popup_may_open`), and what counts as
  typing is decided in the key handler (`completion::types_a_character`) because it cannot be
  decided anywhere else. Every document change schedules a recompute a tick later, and the recompute
  sees only the document — where a typed `x` and Ctrl+X are indistinguishable. So Ctrl+X (cut
  line) and Ctrl+Z opened a list wherever the caret happened to land, and Enter opened the
  `auto_show` list on the new blank line: three basic edits that never asked for a suggestion.
  `Ctrl`/`Alt` mean command, never text — the same test the auto-pair block applies, with the same
  AltGr cost, and the `ctrl` argument is `shortcuts::primary_held` so Cmd counts as command too on
  macOS rather than firing a shortcut *and* typing a character — while Space is typing however the
  platform reports it (`Named(Space)` or `" "`),
  because the empty-prefix list after `WHERE ` hangs off it. A list already open keeps recomputing
  on any edit, so Backspace still refines it and closes it when the prefix goes; the rule governs
  what may **start** showing one. `Completion::typed` is **a one-shot**, cleared by
  `recompute_completions` as it reads it — not every edit arrives through a key at all, and a
  context-menu paste, an IME commit or dropped text would otherwise be judged by whatever the last
  keystroke was: type `sel`, dismiss the list, right-click → Paste, and the popup opened on the
  pasted text because the flag was still standing from the `l`. `Completion::suppress` is the other
  half and a different thing: a one-shot that closes an open list after an edit the app itself made
  (`edit_untyped`, accept). The inline prediction is asked *before* both and answers to neither —
  it is not the popup, and `suppress` is spent by the very accept the next ghost has to follow
  (under `completion.rs`).
- **`Editor::points_of_offset` returns *content* coords, not viewport-relative** (`.y` is `vline_y`,
  the absolute document y; the gutter view subtracts `viewport.y0` itself). Overlays pinned in
  `editor_area` must subtract `ed.viewport.get()` `x0`/`y0` to follow scroll, and add
  `EDITOR_TEXT_TOP` (the border and the padding — the *SQL editor padding* bullet) to `y` — see `span_box_at`
  (bracket matching, identifier occurrences), `underline_seg_at`, `statement_line_boxes_at`, all
  tested against a scrolled viewport. The caret-anchored popups do it one step later:
  `completion::set_anchor` stores the caret line in content coords and
  `completion_popup`/`signature_popup` subtract the viewport
  *inside their style closures*, because those are placed once per edit and would otherwise stay
  pinned to the scroll position the popup opened at. The signature hint's own two placement
  numbers are closures over `theme::scaled` for the same class of reason — it is lifted by its own
  height (two lines of text plus padding, all of which scale) and nudged right of the caret by air,
  and frozen at `48` the lift was correct at Normal only: from 130% the popup's bottom fell below
  the caret's line top and covered the statement being typed, which is the one thing a hint about
  that statement must not do. **`editor_area` also doesn't clip**, so an
  overlay must bound itself: a box wider than the visible code column paints straight out of the
  editor and over the panel beside it. **All three text-overlay geometries clamp horizontally**, to
  one shared fold — `visible_hi(content_x, vp)`, which is `content_x + vp.width()`, or infinity when
  the width is still `0` ("not laid out yet", so it clamps nothing rather than blanking the
  overlay). `statement_line_boxes_at` was clamped first, when a statement's border drew across the
  panel; `span_box_at` and `underline_seg_at` share that same unclipped container and were left
  unclamped, so they did the same thing one token at a time — scroll a query right until a
  highlighted identifier leaves the viewport and its occurrence box painted at a **negative** x,
  over the schema panel. The panel clips its own children, but it is painted *before* the editor, so
  a sibling overlay's stray rectangle lands on top of it regardless. Both therefore take the viewport
  `Rect` rather than just its origin — the clamp needs the width — clamp left to `content_x` and
  right to the fold, and return `None` when nothing of the span is left inside it, rather than a stub
  pinned to whichever edge it fell off. `geometry_tests` pins all four cases (past the fold, scrolled
  off to the left, the half-visible trim, the unmeasured viewport) for both functions, and
  `horizontal_scroll_shifts_every_overlay_left` deliberately measures a token far enough right that
  it does *not* clamp at the offset it scrolls to: a token that clamps cannot also demonstrate the
  translation. For the *text* overlays the vertical half needs no clamp — floem won't place an offset
  outside its screen lines, and `editor_points` drops what it won't place. The suggestion list is
  the exception on **both** axes, since it occupies space no line does, and it bounds itself against
  `editor_area`'s tracked size (`area_h`/`area_w`) rather than against the text. Vertically,
  `completion::popup_placement` hangs it below the caret when it fits, flips it above the caret line
  when it doesn't, and shortens it to the roomier side when neither holds; left to hang below
  unconditionally it drew itself down across the results grid. Horizontally, `completion::popup_w`
  sizes it and `completion::popup_x` slides it left to keep the right edge inside the pane — a flat
  `COMPLETION_GUTTER + caret.x` ran a completion near the right edge off the pane, where `.clip()`
  cut every row's annotations off mid-word (worst with the AI panel hidden, where the editor's right
  edge *is* the window's). Both predict rather than measure, because a style closure gets neither a
  laid-out height nor width: the height from `COMPLETION_ROW_H × rows`, pinned when flipped so an
  under-estimate costs a scrolled pixel rather than an overlap — the selected function's summary
  footer counts as one of those rows, and comes off the scroll's own cap (under `completion.rs`);
  the width from
  `completion::natural_width`, which sums the row chrome (the `COMPLETION_*_W` consts, which
  `completion_popup` builds its rows from so the two can't drift) with `widgets::measure_text_px_at`
  over the row text. That measurement runs in `completion::set_items` — once per recompute, not per
  style pass — and its result lives on `Completion::width`. It replaced a flat `min_width(320)`,
  which left a list of one-letter column names three-quarters empty; a row too wide for the box now
  ellipsizes its two dim annotation columns, and never the name being picked. Two details of that
  are load-bearing, both learned from the same symptom (`main` truncating to `m…` on the *widest*
  row of a table list while every shorter row rendered clean). The row list takes an **explicit**
  width, not `width_full()`: a percentage resolves against a definite parent width, and a `scroll`
  lays its child out against max-content available space, so the percentage quietly became "as wide
  as the widest row" and the rows never stretched to the box at all. And `row_width` carries
  `completion_slack_w()` of air and rounds up, because a box sized to exactly its widest row puts
  that row on its own ellipsis boundary, where a sub-pixel disagreement between the measurement and
  the layout is a visibly wrong string.
  The **Ctrl+Enter run menu** is the fourth caret-anchored overlay and obeys both halves the same
  way, through `editor_pane::run_menu_pos`: `run_menu` stores the caret's line-bottom in content
  coords (via `content_x`, the code column the highlight boxes use — `COMPLETION_GUTTER`, still a
  constant, is an under-estimate the suggestion list hides behind its own padding) and the style
  closure subtracts
  `ed.viewport`, then keeps the panel inside `content_x + vp.width()` — the same fold
  `statement_line_boxes_at` clamps to, rather than `area_w`. Horizontally it **flips** left of the
  caret, so the menu stays beside the statement it is about to run; vertically it **clamps**, since
  flipping would need the caret line's top edge and covering a line beats a jump. The flip alone is
  not enough: an anchor already past the fold (a caret scrolled out to the right) flips to somewhere
  still past it. `run_menu_w()` is one metric for the panel's `min_width` and the placement's
  arithmetic — comparing an unscrolled anchor against a width the panel wasn't drawn at is what cut
  the menu off at the editor's right edge on a long line. **And that metric is measured, with
  `scaled(170)` as its floor**: the widest of the two labels at `font_title`, plus
  `menu_item_style`'s `widgets::MENU_ROW_PAD_H` on both sides and the panel's 2px of border. A
  constant 170 was right while the labels were English; longer ones drew the panel wider than the
  170 the placement had made room for, and near the editor's right edge it was cut off again — the
  same bug through the other half of the metric, seen in the sandboxed app under
  doubled text and gone there after.
- **The floem editor owns its document once mounted, and the sync is one-way — writing the `query`
  signal from outside is not merely invisible, it is lost.** Every edit runs
  `query.set(doc.text())` from the editor's `.update` callback (`editor_pane.rs`) — **the one copy
  of the document an edit makes**: what runs per keystroke or caret move borrows `query` (`with`,
  `track`, `completion::with_text`, which falls back to copying the document only if `query`'s
  length disagrees with it), and a debounce reads `query` when it fires rather than copying the
  text at the keystroke. The caches that take an answer forward over an edit — the statement index
  and the styling's carried regions and bracket depths — keep **no** copy: they follow the pane's
  `text::EditLog`, which `.update` fills from the edit's deltas before it sets `query` (under
  `completion.rs` and `sql_highlight.rs`); until then each kept one of its own. It used to be a
  dozen copies a keystroke — `query.get()` to track, every
  open tab's text cloned by the tabs-persist effect, eager clones for each debounce — ~0.5 ms each
  at 4 MiB. A planned edit (auto-pair, Backspace) is computed under the borrow and applied after
  it ends, since the edit re-enters `.update` and sets `query`. There is no effect pushing `query`
  back into the doc, so a write from outside a mounted editor changes nothing
  the user can see and the next keystroke overwrites the signal back from the doc. Replacing a tab's
  text from outside — reload from disk — therefore bumps `Tab::reload_gen`, which is part of the
  `editor_area` `dyn_container` key in `lib.rs` (`(active id, is_flashing, reload_gen)`), and the
  pane remounts on the new text. That key reads the tab out of `tabs` with `with_untracked` and only
  *then* tracks `reload_gen`: tracking the whole vector there would rebuild the editor every time
  any tab was opened or closed.
  A remount is the right answer only when the *whole document* is being replaced by something the
  user cannot have been mid-edit in. For an edit — anything that should be undoable and should keep
  the caret — the write has to reach the mounted editor instead, and the way in is a
  request-and-clear signal on the tab that the pane consumes (`Tab::format_req`, `Tab::jump_offset`).
  This is the shape the palette's "Format Code" now uses. It is worth knowing what it did before,
  because that is the failure mode: it called `sqlfmt::format_sql` itself and wrote the result into
  `t.query`, which nothing read back, so the command silently did nothing at all — and the bug was
  invisible in review, because the line that "applies" the format looks exactly like the line that
  would work if the sync went both ways. It also meant two formatters, so the palette could drift
  from Ctrl+Alt+L; `format_req` (`Some(FormatReq::Code)`) reaches the same
  `editor_pane::format_editor`. The keys now come the same way: the key handler asks
  `editor_pane::format_chord` (Ctrl+Alt+L, Ctrl+Alt+Shift+L, Ctrl+Alt+J, matched by *physical* key
  because Windows delivers Ctrl+Alt as AltGr) and sets `format_req` rather than formatting itself.
  When it called `format_editor` directly, its Ctrl+Alt+L arm did not exclude Shift, so
  Ctrl+Alt+Shift+L — now *Format statement* — formatted the whole document
  (`each_format_chord_asks_for_its_own_request`, `other_chords_ask_for_nothing`).
  That function writes `sqlfmt::format_range`'s text, not `format_sql`'s — the `sqlfmt.rs` entry
  has the `fromt` splice that cost — and returns without touching the caret when `edit_untyped`
  refuses, where under the Ctrl+K preview's freeze it used to move the caret anyway. A formatted
  selection stays selected, over its new extent; a formatted document keeps the caret among the
  same tokens through `sqlfmt::caret_after_format`, where it used to jump to the end.
