# Floem 0.2.0, vendored

This is the `floem` crate as published to crates.io at 0.2.0 (upstream commit
`574638db95a4cf898f6156fa1db375de7a1d49c3`), used through `[patch.crates-io]`
in the workspace `Cargo.toml`. Everything under `src/` is upstream's, byte for
byte, except the changes listed below — and line endings, which git normalizes
(upstream ships `src/nav.rs` with CRLF).

**Left out of the copy:** `docs/`, `.github/`, `.devcontainer/`,
`CHANGELOG.md`, `CONTRIBUTING.md` and `Cargo.toml.orig` — none of them is read
by the build. `Cargo.toml` is the normalized manifest crates.io serves.

**Added for vendoring, not changes to Floem:**

- this file;
- `rustfmt.toml`, which keeps `cargo fmt --all` — which also formats local
  path dependencies — from reformatting upstream's code;
- a `[lints]` section at the end of `Cargo.toml` allowing every lint. Cargo caps
  a registry crate's warnings but not a path dependency's, so without it
  upstream's dozen warnings print on every build of the workspace. It is the
  manifest, not the source, so `src/` stays upstream's.

To take a new Floem release: vendor it the same way, re-apply every entry
below, and drop any that upstream has absorbed. When the list is empty, delete
this directory and the `[patch]` entry.

## Changes against upstream

Each change is marked in the source with a `schemaic patch (PATCHES.md)`
comment, so `grep -rn "schemaic patch" src` finds all of them.

### 1. `WindowConfig::app_id` — the window's application id on Linux

**Why.** Floem 0.2 has no way to name a window's application. winit sets the
Wayland `app_id` only when a name is passed through its platform attributes,
and Floem never passes one, so under GNOME on Wayland (Ubuntu's default) the
shell cannot match the window to `io.github.fadion.Schemaic.desktop` and the
taskbar shows a generic icon. On X11, winit falls back to the executable's name
for `WM_CLASS`, which the desktop entry's `StartupWMClass=schemaic` was
matching.

**What.**

- `src/window.rs`: a `pub(crate) app_id: Option<String>` field on
  `WindowConfig`, `None` by default, and a `pub fn app_id(self, impl
  Into<String>)` builder beside `title`.
- `src/app_handle.rs`, `new_window`: the field is destructured with the others
  and, on free Unix (not macOS/iOS/Android/Emscripten/wasm), passed to winit as
  `WindowBuilderExtX11::with_name(app_id, app_id)`. The X11 and Wayland
  `with_name` both set the same `platform_specific.name`, which the X11
  backend turns into `WM_CLASS` and the Wayland one into `app_id`, so one call
  covers both.

Schemaic passes `schemaic_core::APP_ID`. Upstream Floem (after 0.2) may grow
its own setter; if it does, drop this entry and use theirs.

### 2. The gutter fills every caret's band before drawing any number

**Why.** `EditorGutterView::paint` drew the line numbers top to bottom and,
on reaching the primary caret's line, filled the current-line band for *every*
caret. A secondary caret above the primary one therefore had its band painted
over its own, already-drawn number (Alt+click lines 1 and 3: line 1's number
vanished), and once the primary caret's line scrolled out of view no caret's
band was filled at all.

**What.** `src/views/editor/gutter.rs`, `paint`: the band-filling block is
moved, unchanged, out of the number loop to just before it, inside the same
`screen_lines.with_untracked`. Nothing else in the method changes.

### 3. Whitespace marks are centred in their line, as the text is

**Why.** With `RenderWhitespace` on, `EditorView::paint_text` draws each `·` and
`→` from a text layout of its own, at the line's top. The line's text layout is
built with the editor's line height (`LineHeightValue::Px(style.line_height)`),
which is what centres a line's glyphs in a line taller than the font; the
marks' layout was not, so it took the font's natural height and every mark rode
at the top of the line, above the middle of the text beside it.

**What.** `src/views/editor/view.rs`, `paint_text`: the marks' `Attrs` gain
`.line_height(LineHeightValue::Px(style.line_height(edid, line)))`, the same
value the line's own layout is given in `Editor::new_text_layout`. Nothing
else changes; the marks keep their x, colour, family and size.

### 4. A captured view hears its release wherever it lands, for every button

**Why.** A pointer event goes straight to the active view (`request_active`)
once one is set — `WindowHandle::event` routes it there with `directed = true`.
For a **primary** release, `EventCx::unconditional_view_event`'s `PointerUp`
arm runs the view's `PointerUp` listeners unconditionally. Any other button's
release falls through to the generic listener block, which runs a listener only
when the event's point lies inside the view's own rect. So a middle-button drag
released outside the view — elsewhere in the window, or outside it — never
reached the view that captured it, while every move still did: Schemaic's ER
diagram went on panning with no button held.

**What.** `src/context.rs`, `unconditional_view_event`, the generic listener
block: `should_run` is also true for a `PointerUp` of a non-primary button when
the event is `directed`. Primary releases are excluded because the `PointerUp`
arm has already run their listeners; undirected dispatch is unchanged.

### 5. `CursorStyle::Grabbing` — the closed hand

**Why.** Floem 0.2's `CursorStyle` names fifteen cursors and no grab cursor,
though winit's `CursorIcon` has one. Schemaic's ER diagram wears it while its
canvas is being panned.

**What.**

- `src/style.rs`: a `Grabbing` variant at the end of `CursorStyle`.
- `src/window_handle.rs`, `set_cursor`: `CursorStyle::Grabbing` maps to
  `CursorIcon::Grabbing`.

Only the one variant the app uses; winit's `Grab` (the open hand) and the rest
are not added. Upstream may grow its own; if it does, drop this entry.

### 6. `ViewId::apply_event` is `pub`

**Why.** A `KeyDown` goes to the focused view and, unanswered, to the window
root's own listeners — never up through ancestors. So with a modal mounted and
focus on nothing (floem clears it on a press it does not place) or on a view
behind the modal, the modal's Escape listener never hears the key. Schemaic's
window root forwards an unanswered Escape to the innermost modal's `KeyDown`
listeners, and running a view's listeners for an event is exactly what
`ViewId::apply_event` does — but it was `pub(crate)`.

**What.** `src/id.rs`: `apply_event` is `pub`. Its body is unchanged.

### 7. The editor's pointer cursor is a signal, and follows it without a move

**Why.** `editor_content` styled the text view `.cursor(CursorStyle::Text)`, a
constant in the view's own style, which outranks any class an ancestor could
apply — so nothing outside could change it. Schemaic shows an arrow over a word
Ctrl+click would open. And Floem resolves the cursor only on a pointer move
(`WindowHandle::event` clears it, `EventCx` refills it from the hovered views),
so a cursor that changes with the pointer still — Ctrl pressed over a word —
kept the old one until the mouse twitched.

**What.**

- `src/views/editor/mod.rs`: a `pub pointer_cursor: RwSignal<CursorStyle>`
  field on `Editor`, `CursorStyle::Text` in `new_direct`'s literal.
- `src/views/editor/view.rs`, `editor_content`: the text view's style reads
  `pointer_cursor.get()` instead of the constant (and the now-unused
  `CursorStyle` import goes).
- `src/context.rs`, `StyleCx::style_view`: when a restyle changes the view's
  cursor, the view is hovered and its old cursor is the one showing
  (`app_state.cursor`), the new one replaces it; `set_cursor` at the end of
  the update then applies it.

### 8. A view removed in an update batch hears no more of that batch

**Why.** `process_update_messages` takes a whole batch of queued messages and
delivers each `UpdateMessage::State` through `id.view()`. `ViewId::remove`
drops the id from `view_ids` but never from `views`, so a view removed by an
earlier message in the same batch still resolves and still runs `update`. A
`dyn_container` swap removes its old child and then disposes the child's scope,
both synchronously. A nested `dyn_container` in that child, with its own
message queued in the same batch, then runs its child closure against signals
that no longer exist. Schemaic hit this on a connection switch: the switch
queued the results area's swap, and the cleared schema tree made the result
grid's key map change, which queued a rebuild of the grid's body. The body
rebuilt after its `GridState` was disposed and the app panicked in
`get_untracked`. It happened only when both messages landed in one batch, so it
was intermittent.

**What.**

- `src/id.rs`: `ViewId::is_registered`, whether the id is still in `view_ids`.
- `src/window_handle.rs`, `process_update_messages`: a `State` message whose id
  is no longer registered is dropped rather than delivered.
- `src/window_handle.rs`, `process_deferred_update_messages`: the same check
  for the deferred queue (`update_state_deferred`), which delivered through
  `id.view()` just the same.

`views` still keeps a removed view's entry. Evicting it as well would change
when a removed view's closures are dropped, which is a wider change than this
crash needs.
