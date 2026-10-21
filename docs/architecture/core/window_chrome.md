# `core/window_chrome.rs`

`core/window_chrome.rs` — which half of the window frame the app draws itself, now that it
launches with `WindowConfig::show_titlebar(false)`. `Chrome::current()` answers per `Host`
(Windows/Linux/macOS): `draws_own_controls`, `own_control_count`, `draws_own_resize_border`,
`wants_drop_shadow`, `leading_inset`. **Ask the capability, never `cfg!(target_os = …)` at the
use site** — the same
rule the engines follow, for the same reason. `ui::shortcuts::PRIMARY_IS_CMD` is the second
holder of it: one `cfg!` for whether the primary shortcut modifier is Cmd, read by both the
Shortcuts modal's labels and the key handlers so the two cannot disagree. The split is not
cosmetic: floem reads that one
flag as *undecorated* on Windows/Linux but as a *transparent* title bar over a full-size content
view on macOS, so the traffic lights, the native resize border and the move behaviour all
survive there. What macOS costs us instead is space — the lights are drawn over our header, so
`leading_inset` reserves it. The Windows half is the one with teeth: winit strips
`WS_CAPTION | WS_SIZEBOX` from an undecorated window, so without our own edge zones the window
cannot be resized at all. `ui::window_chrome` draws what this module decides.
`own_control_count` is the *how much* to `draws_own_controls`'s *whether*, and it has a caller
of its own: the band the app lays over a modal's backdrop to keep the title bar working has to
stop exactly where the caption buttons begin. A count, not a width — the pixels are the UI's.
