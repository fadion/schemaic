# `core/window_chrome.rs`

`core/window_chrome.rs` — which half of the window frame the app draws itself, now that it
launches with `WindowConfig::show_titlebar(false)`. `Chrome::current()` answers per `Host`
(Windows/Linux/macOS): `draws_own_controls`, `own_control_count`, `draws_own_resize_border`,
`wants_drop_shadow`, `leading_inset`, `header_shows_mark`, `traffic_lights_offset`. **Ask the capability, never `cfg!(target_os = …)` at the
use site** — the same
rule the engines follow, for the same reason. `ui::shortcuts::PRIMARY_IS_CMD` is the second
holder of it: one `cfg!` for whether the primary shortcut modifier is Cmd, read by both the
Shortcuts modal's labels and the key handlers so the two cannot disagree. The split is not
cosmetic: floem reads that one
flag as *undecorated* on Windows/Linux but as a *transparent* title bar over a full-size content
view on macOS, so the traffic lights, the native resize border and the move behaviour all
survive there. What macOS costs us instead is space — the lights are drawn over our header, so
`leading_inset` reserves it — 82pt: the lights start `TRAFFIC_LIGHTS_X` (11) in, 14pt circles 23pt
apart, and end near 71 (measured on macOS 26.7), so the connection switcher, which leads the row
there, starts about 11pt after the green light — the margin the lights keep from the window's
edge. The 72 it used to be cleared them but sat the header's first object (the logo, then) hard
against the green light
(`the_leading_inset_clears_the_lights_at_their_offset` keeps at least the lights' own 11pt margin
spare). **The mark is not drawn on macOS at all** (`header_shows_mark`, read by the header in
`ui/lib.rs` through `ui::window_chrome::WindowChrome::shows_mark`): the lights already mark the
window's corner and the menu bar and the Dock name the app, so the logo was a fourth object
crowding them rather than an identity nothing else gave. Where the controls are ours they sit at
the trailing edge, and the mark is the only thing at the leading one that says whose window it is.
The switcher drops the 12px margin it otherwise keeps after the mark and sits flush at the inset —
that gap was the mark's, not the lights'. The two answers are one or the other: a host shows the
mark exactly when it reserves no leading inset
(`the_header_leads_with_the_mark_only_where_the_os_draws_no_controls_there`), so the test fails on
a host given both, or neither. The Windows half is the one with teeth: winit strips
`WS_CAPTION | WS_SIZEBOX` from an undecorated window, so without our own edge zones the window
cannot be resized at all. `ui::window_chrome` draws what this module decides.
`own_control_count` is the *how much* to `draws_own_controls`'s *whether*, and it has a caller
of its own: the band the app lays over a modal's backdrop to keep the title bar working has to
stop exactly where the caption buttons begin. A count, not a width — the pixels are the UI's.
`traffic_lights_offset(bar_h)` is where macOS's lights go, `Some` only there
(`only_os_drawn_controls_get_a_traffic_lights_offset`). **Floem's own placement centred them in
nothing**: `show_titlebar(false)` applies winit's `with_traffic_lights_offset(11.0, 16.0)`, a
constant for a standard title bar, and on Schemaic's taller header the lights' centre sat at
13.75pt, 5.75 above the middle of the 39pt bar. The answer is `(TRAFFIC_LIGHTS_X, bar_h / 2 + LIGHT_CENTRE_ABOVE_OFFSET)`, and
the `2.25` is **measured, not derived** — winit 0.29's `position_traffic_lights` sizes the
title-bar container to button height plus y and never sets a button's y, so where a light lands is
AppKit's layout; on macOS 26.7, 2× captures at y = 16, 21 and 26 put the red light's centre at
13.75, 18.75 and 23.75pt. A macOS release that lays the container out differently moves the
constant, and the symptom is lights off-centre by the difference, nothing worse. `bar_h` is the
visible bar, *above* the header's 1pt bottom rule: `app/main.rs` passes
`theme::header_h_at(scale) - theme::HEADER_BORDER` through `WindowConfig::with_mac_os_config`,
which floem applies after its `show_titlebar` block and so overrides the 16. The capture after the
change put the centre at 19.5pt on a 39pt bar
(`the_traffic_lights_are_centred_in_the_bar_they_are_placed_for` pins the arithmetic at all four
scales' bars). **It is taken once, at window creation** — winit has
no setter — so a UI-scale change mid-session re-centres the lights only at the next launch. That is
also why `main` reads `ui_state` an extra time for the saved `UiScale`: the regular load is inside
`app_view`, after the window exists, and `header_h_at` is `header_h`'s non-reactive face for the
same reason, there being no theme registry yet to scale against.
Two capabilities here are not about the frame. `shows_window_before_sizing` (Windows only):
winit's Win32 backend shows a new window before it applies the requested size, maximize and
position, and leaves it blank white until the GPU's first frame, so the app asks Floem to create
the window hidden and show it with that frame drawn — not everywhere, because
Wayland has no hidden state (`only_windows_shows_a_window_before_sizing_it`; `window_place.md`
has the launch). And `places_windows_in_points`: whether the
OS lays every monitor out in one space of points (macOS) or in device pixels (Windows, X11),
which decides how `core::window_place::restore` turns a saved position back into the
`WindowConfig::position` winit will accept. It lives on `Chrome` so that question too is asked
rather than spelled `cfg!(target_os = "macos")` in `main`
(`only_macos_places_windows_in_points`).
