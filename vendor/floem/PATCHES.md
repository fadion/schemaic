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
