# `core/window_place.rs`

`window_place.rs` — where and how big the window was when it closed, so the next launch opens it
there: `WindowPlace`, kept as `window.json` (`window_place::FILE`) in the config directory beside
`ui_state.json`. **Its own file, not a field of `UiState`**, because it is written once, as the
window closes, and never by a settings save — and it has to be read in `main` *before* the window
exists, since Floem takes a window's size and position only as `WindowConfig` at creation. A
`WindowPlace` is the outer top-left in the logical pixels the OS reported (`None` where the
platform reports no position at all — Wayland), the **content** size in logical pixels, the
`scale` the position was read at, and `maximized`. Every field but the size is
`#[serde(default)]`, so a file holding only a size reads as one with no position at scale 1
(`a_file_with_only_a_size_parses`). Two decisions live here, both pure; the sampling and the one
write are the app's (`WindowPlaceTracker`, at the bottom of *app/main.rs*).
**What to remember is the *normal* bounds, never the maximized ones.** `Tracker` keeps the last
sample taken while the window was `Shown::Normal`, and `closing` writes those with `maximized`
set only when the window was closed maximized. A maximized window's bounds are the screen's, and
remembering them would reopen it as a screen-sized rectangle that un-maximizes into itself
(`closing_maximized_keeps_the_normal_bounds_and_says_maximized`). The tracker is **seeded from
what the launch opened** — `Restored::place`, the file fitted to the monitors, not the file as
read — because a window opened maximized and closed without ever being restored produces no
normal sample at all, and without the seed would have nothing to write but the screen
(`a_window_opened_maximized_and_never_restored_keeps_the_loaded_bounds`); fitted, so an
out-of-range or damaged file is written back healed rather than as it was read
(`closing_maximized_from_an_unreachable_file_writes_back_the_fitted_place`). A window closed
**minimized reopens normal**, over its last normal bounds: winit cannot say on every platform
what it was minimized *from* (`closing_minimized_keeps_the_normal_bounds_and_reopens_normal`).
A zero-sized sample is never remembered even when called normal — minimizing on Windows reports a
0×0 window at −32000, and `Shown::Minimized` usually filters it first; `usable` is the backstop
for the sample that races it (`a_zero_sized_sample_is_not_remembered_even_when_called_normal`).
With nothing seen and nothing loaded, `closing` answers `None` and nothing is written.
**What to restore** (`restore`) is the size always and the position only when it is reachable.
The size is raised to `MIN_WIDTH`×`MIN_HEIGHT` (640×400) — below that the header's controls
overlap, and a hand-edited or damaged file must not reopen the app as a sliver — and clamped to
the logical size of the monitor it lands on, or, when the position is dropped, of the
**primary**, where the OS puts an unplaced window — not the largest monitor, which beside a
laptop panel would let the window open past the panel's edges (or to the monitor itself, where
it is smaller than the minimum). Logical means the monitor's own scale on every host, so a
Retina panel clamps at its points, not its pixels
(`a_place_the_os_will_put_on_the_primary_is_clamped_to_the_primary`,
`a_points_host_clamps_to_the_retina_panels_points_not_its_pixels`). The position is kept only when the window's **title strip**
lands on a monitor present at launch: probed `STRIP_PROBE` (12 logical px) below the top edge,
inside the app's own header on every platform, with at least `STRIP_GRIP` (64 logical px — or
the window's whole width, if narrower) of it overlapping the monitor horizontally, which is
enough to grab and drag back. Otherwise the position is dropped and the OS places the window: a
position from a monitor since unplugged, or one with only a sliver of strip on screen, would open
it where nobody can grab it (`a_place_on_a_monitor_since_unplugged_keeps_its_size_and_loses_its_position`,
`a_window_with_only_a_sliver_of_title_strip_on_screen_is_not_reachable`,
`a_title_strip_above_every_monitor_is_not_reachable`). A window hanging off an edge by most of
its width still counts. One thing the code deliberately leaves alone: `maximized` comes back
whether or not the position did, so a window closed maximized on an unplugged monitor reopens
maximized wherever the OS put it.
**The coordinates are the part with teeth, and they differ by host — asked through
`Chrome::places_windows_in_points`, never a `cfg!`.** On a device-pixel host (Windows, X11) the
monitors are laid out in device pixels, so the saved logical position is turned into device
pixels with the saved `scale`, checked against the monitors there, and **launched in device
pixels** — `LaunchPosition::Device`, through `WindowConfig::physical_position` (vendored Floem's
patch 11), which winit places exactly. A logical `WindowConfig::position` would be converted with
the scale of whichever monitor the window is created on — the one the shell launched it on
(Windows), the one under the pointer (X11) — which nothing at launch can know; an earlier version
guessed the primary's and could open the window off-screen on a mixed-scale setup. A window saved
at logical 2000 on a 150% monitor right of a 100% primary launches at device pixel 3000
(`a_device_pixel_host_launches_at_the_saved_device_pixels`,
`a_device_pixel_host_checks_reachability_in_device_pixels`). On a points host (macOS) winit hands
the position to the OS as points unconverted and reports each monitor's device pixels at that
monitor's own scale, so the monitors are brought into points (`in_points`) and the position goes
back as saved (`LaunchPosition::Points`, through the ordinary `WindowConfig::position`) — a 2×
Retina panel 3024 device pixels wide is 1512 points, and a strip starting at point 1460 is a
sliver on it (`a_points_host_finds_the_retina_panel_in_points`). A non-finite size, position or
scale degrades to the minimum with no position rather than reaching winit
(`a_damaged_file_opens_at_the_minimum_with_no_position_and_heals`).
**The app half.** `main` builds the `Application` first so it can read `available_monitors()`
and `primary_monitor()` (which marks `Monitor::primary`), loads the file with
`persist::load_json::<Option<WindowPlace>>`, calls `restore`, and sets `WindowConfig::size` and
the position `restore` chose — 1280×820, placed by the OS, when nothing is saved. `WindowConfig` has no maximized flag, so `app_view` calls `WindowIdExt::maximized(&window,
true)` itself, which the window's update queue drains at the end of the turn that creates it.
That resize — and the one Windows makes to a tall window during creation — lands before the GPU
is ready, which upstream Floem dropped, leaving the surface at the launch size and the rest of
the window black; vendored Floem's patch 9 (`vendor/floem/PATCHES.md`) keeps it. **On Windows
the window is created hidden and Floem shows it with its first frame drawn**
(`WindowConfig::show_after_first_frame`, patch 10, asked through
`Chrome::shows_window_before_sizing`): winit shows a new Win32 window before sizing, maximizing
and placing it, and the GPU arrives after that, so a tall window was seen growing out of the
default size with the new strip black, a maximized one was seen at its normal bounds first, and
every launch flashed the window class's white. The maximize `app_view` queues is applied in the
creation turn, before the GPU is ready — winit re-hides a hidden window it maximizes — so the
window is first seen maximized, drawn, in the theme's colours.
`WindowPlaceTracker` keeps the content size `WindowResized` last reported — the one size Floem
reports on every platform, Wayland included — and **samples the window on a 300 ms trailing
settle after a `WindowResized` or `WindowMoved`, never inside the event**: on Windows the move
that maximizing causes (`WM_WINDOWPOSCHANGED`) is reported before winit sets its maximized flag
in `WM_SIZE`, so reading `is_maximized()` in the move handler records the maximized bounds as the
normal ones (*Floem 0.2 gotchas*). Each event cancels the timer the one before it set, so a
burst — a drag — holds one timer and samples once.
The sample is `is_minimized`/`is_maximized`, `position_on_screen_including_frame` and `scale`.
**`place.flush()` in the `WindowClosed` listener is the only write of `window.json`**, after the
session, filter and reopen-ring flushes and before `quit_app`: it samples once more, for a move
whose settle had not fired, and `persist::save_json`s what `closing` returns. Because it rides
that listener it shares its caveat (*app/overview.md*): whether macOS's own *Quit* fires
`WindowClosed` is unverified, and if it does not, a place changed this session is not kept on
that path.
