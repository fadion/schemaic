//! Where and how big the window was, so the next launch opens it there.
//!
//! Persisted as `window.json` beside `ui_state.json` — its own file because it is
//! written once, when the window closes, and never by a settings save — and
//! loaded by `main` before the window exists, since floem takes a window's size
//! and position only as `WindowConfig` at creation.
//!
//! Two decisions live here, both pure:
//!
//! - **What to remember** ([`Tracker`]). The *normal* bounds — the ones a
//!   maximized window returns to — plus whether it was maximized. A maximized or
//!   minimized window's bounds are the screen's or nowhere's, and remembering
//!   them would reopen the window as a borderless screen-sized rectangle that
//!   un-maximizes into itself. The app samples the window on a short settle after
//!   a move or resize rather than on the event itself, because on Windows the
//!   move that maximizing causes arrives *before* winit learns the window is
//!   maximized (`WM_WINDOWPOSCHANGED` precedes the `WM_SIZE` that sets its flag).
//! - **What to restore** ([`restore`]). The size always, clamped to the screen it
//!   lands on; the position only when the window's title strip would land on a
//!   monitor present now — a position from a monitor since unplugged would open
//!   the window where nobody can grab it.

use serde::{Deserialize, Serialize};

use crate::window_chrome::Chrome;

/// The file in the config directory the place is kept in.
pub const FILE: &str = "window.json";

/// The smallest size a restored window opens at, logical pixels. Below this the
/// header's controls overlap; a hand-edited or damaged file must not reopen the
/// app as a sliver.
pub const MIN_WIDTH: f64 = 640.0;
pub const MIN_HEIGHT: f64 = 400.0;

/// How far below the window's top edge its title strip is probed, logical px —
/// inside the app's own header on every platform.
const STRIP_PROBE: f64 = 12.0;
/// How much of the title strip has to be on a monitor for the window to count as
/// reachable, logical px: enough to grab and drag back.
const STRIP_GRIP: f64 = 64.0;

/// The window as it was left, in `window.json`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowPlace {
    /// The outer top-left, in the logical pixels the OS reported at `scale`.
    /// `None` where the platform does not report a window's position (Wayland).
    #[serde(default)]
    pub position: Option<[f64; 2]>,
    /// The content size, logical pixels.
    pub width: f64,
    pub height: f64,
    /// The window's scale factor when `position` was read, which is what turns it
    /// back into the device pixels the monitors are laid out in.
    #[serde(default = "one")]
    pub scale: f64,
    /// Reopen maximized, over the normal bounds above.
    #[serde(default)]
    pub maximized: bool,
}

fn one() -> f64 {
    1.0
}

/// The window's state when it is sampled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    Normal,
    Maximized,
    Minimized,
}

/// The normal bounds seen so far this session.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tracker {
    normal: Option<WindowPlace>,
}

impl Tracker {
    /// Starts from what the launch opened the window with
    /// ([`Restored::place`]), so a window opened maximized and closed without
    /// ever being restored keeps the normal bounds it came with — fitted, so a
    /// damaged or out-of-range file is not written back as it was read.
    pub fn new(launched: Option<WindowPlace>) -> Self {
        Self {
            normal: launched.map(|p| WindowPlace {
                maximized: false,
                ..p
            }),
        }
    }

    /// A settled sample: the bounds `live` describes are the normal ones only
    /// while the window is [`Shown::Normal`].
    pub fn observe(&mut self, live: WindowPlace, shown: Shown) {
        if shown == Shown::Normal && usable(&live) {
            self.normal = Some(WindowPlace {
                maximized: false,
                ..live
            });
        }
    }

    /// What to write as the window closes. `live` is sampled at the close itself,
    /// which catches a move or resize whose settle had not fired yet.
    pub fn closing(mut self, live: WindowPlace, shown: Shown) -> Option<WindowPlace> {
        self.observe(live, shown);
        self.normal.map(|p| WindowPlace {
            // A window closed from the taskbar while minimized reopens normal:
            // winit cannot say what it was minimized *from* on every platform.
            maximized: shown == Shown::Maximized,
            ..p
        })
    }
}

/// A size worth keeping: finite and positive. Minimizing on Windows reports a
/// 0×0 window at −32000, which [`Shown::Minimized`] usually filters first — this
/// is the backstop for the sample that races it.
fn usable(p: &WindowPlace) -> bool {
    p.width.is_finite() && p.height.is_finite() && p.width > 0.0 && p.height > 0.0
}

/// A monitor present at launch, in device pixels, as winit reports it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Monitor {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub scale: f64,
    /// The OS's primary monitor — where a window it places itself opens.
    pub primary: bool,
}

/// Where the launch asks floem to put the window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LaunchPosition {
    /// Device pixels, for `WindowConfig::physical_position` (vendored floem's
    /// patch 11). winit places a physical position exactly; a logical one it
    /// converts with the scale of whichever monitor the window happens to be
    /// created on — the one the pointer is on (X11), or the one the shell
    /// launched it on (Windows) — which nothing here can know.
    Device([f64; 2]),
    /// Points, for `WindowConfig::position`, on a host that places windows in
    /// points ([`Chrome::places_windows_in_points`]): one space for every
    /// monitor, so the saved position goes back as it came.
    Points([f64; 2]),
}

/// What the launch opens the window with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Restored {
    /// The saved place fitted to the monitors present: sized to fit, its
    /// position dropped when unreachable. The window opens at its size and
    /// maximized state, and [`Tracker::new`] starts from it.
    pub place: WindowPlace,
    /// `place.position` as floem takes it, or `None` to let the OS place it.
    pub position: Option<LaunchPosition>,
}

/// `saved`, fitted to the monitors present now.
pub fn restore(saved: &WindowPlace, monitors: &[Monitor], chrome: Chrome) -> Restored {
    let scale = sane_scale(saved.scale);
    let mut width = finite_or(saved.width, MIN_WIDTH).max(MIN_WIDTH);
    let mut height = finite_or(saved.height, MIN_HEIGHT).max(MIN_HEIGHT);
    let points = chrome.places_windows_in_points();

    let mut position = None;
    let mut fit = None;
    if let Some([x, y]) = saved
        .position
        .filter(|[x, y]| x.is_finite() && y.is_finite())
    {
        // The window and the monitors in one space — device pixels, or points.
        let (wx, wy, ws) = if points {
            (x, y, 1.0)
        } else {
            (x * scale, y * scale, scale)
        };
        let landing = monitors.iter().find(|m| {
            let m = if points { in_points(m) } else { **m };
            let probe_y = wy + STRIP_PROBE * ws;
            let overlap = (wx + width * ws).min(m.x + m.width) - wx.max(m.x);
            probe_y >= m.y
                && probe_y < m.y + m.height
                && overlap >= (STRIP_GRIP * ws).min(width * ws)
        });
        if let Some(m) = landing {
            fit = Some(*m);
            position = Some((
                [x, y],
                if points {
                    LaunchPosition::Points([x, y])
                } else {
                    LaunchPosition::Device([wx, wy])
                },
            ));
        }
    }
    // No larger than the screen it lands on — or, placed by the OS, than the
    // primary, where the OS puts it. (Not the largest: a 4K screen beside a
    // laptop panel would let a window open past the panel's edges.)
    let fit = fit.or_else(|| monitors.iter().copied().find(|m| m.primary));
    if let Some(m) = fit {
        let [mw, mh] = logical_size(&m);
        width = width.min(mw).max(MIN_WIDTH.min(mw));
        height = height.min(mh).max(MIN_HEIGHT.min(mh));
    }
    Restored {
        place: WindowPlace {
            position: position.map(|(saved, _)| saved),
            width,
            height,
            scale,
            maximized: saved.maximized,
        },
        position: position.map(|(_, launch)| launch),
    }
}

/// A monitor's size in its own logical pixels — what a window's size is
/// measured in once it is on that monitor, on every host.
fn logical_size(m: &Monitor) -> [f64; 2] {
    let s = sane_scale(m.scale);
    [m.width / s, m.height / s]
}

/// A monitor in points: on a points host winit reports each monitor's device
/// pixels at its own scale.
fn in_points(m: &Monitor) -> Monitor {
    let s = sane_scale(m.scale);
    Monitor {
        x: m.x / s,
        y: m.y / s,
        width: m.width / s,
        height: m.height / s,
        scale: 1.0,
        primary: m.primary,
    }
}

fn sane_scale(s: f64) -> f64 {
    if s.is_finite() && s > 0.0 { s } else { 1.0 }
}

fn finite_or(v: f64, or: f64) -> f64 {
    if v.is_finite() { v } else { or }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window_chrome::Host;

    fn place(x: f64, y: f64, w: f64, h: f64) -> WindowPlace {
        WindowPlace {
            position: Some([x, y]),
            width: w,
            height: h,
            scale: 1.0,
            maximized: false,
        }
    }

    /// A monitor; the first one a test lists is the primary.
    fn mon(x: f64, y: f64, w: f64, h: f64, scale: f64) -> Monitor {
        Monitor {
            x,
            y,
            width: w,
            height: h,
            scale,
            primary: x == 0.0 && y == 0.0,
        }
    }

    fn size(r: &Restored) -> [f64; 2] {
        [r.place.width, r.place.height]
    }

    fn device(x: f64, y: f64) -> Option<LaunchPosition> {
        Some(LaunchPosition::Device([x, y]))
    }

    fn points(x: f64, y: f64) -> Option<LaunchPosition> {
        Some(LaunchPosition::Points([x, y]))
    }

    const WIN: Chrome = Chrome::of(Host::Windows);
    const MAC: Chrome = Chrome::of(Host::MacOs);

    // --- Tracker ---

    #[test]
    fn a_normal_sample_is_the_place_to_remember() {
        let mut t = Tracker::new(None);
        t.observe(place(100.0, 50.0, 1400.0, 900.0), Shown::Normal);
        assert_eq!(
            t.closing(place(100.0, 50.0, 1400.0, 900.0), Shown::Normal),
            Some(place(100.0, 50.0, 1400.0, 900.0))
        );
    }

    #[test]
    fn closing_maximized_keeps_the_normal_bounds_and_says_maximized() {
        let mut t = Tracker::new(None);
        t.observe(place(100.0, 50.0, 1400.0, 900.0), Shown::Normal);
        let screen = place(-8.0, -8.0, 2576.0, 1400.0);
        t.observe(screen, Shown::Maximized);
        let closed = t.closing(screen, Shown::Maximized).unwrap();
        assert_eq!(closed.position, Some([100.0, 50.0]));
        assert_eq!((closed.width, closed.height), (1400.0, 900.0));
        assert!(closed.maximized);
    }

    #[test]
    fn closing_minimized_keeps_the_normal_bounds_and_reopens_normal() {
        let mut t = Tracker::new(None);
        t.observe(place(100.0, 50.0, 1400.0, 900.0), Shown::Normal);
        let parked = place(-32000.0, -32000.0, 0.0, 0.0);
        let closed = t.closing(parked, Shown::Minimized).unwrap();
        assert_eq!(closed, place(100.0, 50.0, 1400.0, 900.0));
    }

    #[test]
    fn a_zero_sized_sample_is_not_remembered_even_when_called_normal() {
        let mut t = Tracker::new(None);
        t.observe(place(100.0, 50.0, 1400.0, 900.0), Shown::Normal);
        t.observe(place(-32000.0, -32000.0, 0.0, 0.0), Shown::Normal);
        let closed = t.closing(place(-32000.0, -32000.0, 0.0, 0.0), Shown::Minimized);
        assert_eq!(closed, Some(place(100.0, 50.0, 1400.0, 900.0)));
    }

    #[test]
    fn a_window_opened_maximized_and_never_restored_keeps_the_loaded_bounds() {
        let loaded = WindowPlace {
            maximized: true,
            ..place(200.0, 80.0, 1300.0, 850.0)
        };
        let t = Tracker::new(Some(loaded));
        let closed = t
            .closing(place(-8.0, -8.0, 2576.0, 1400.0), Shown::Maximized)
            .unwrap();
        assert_eq!(closed, loaded);
    }

    #[test]
    fn nothing_seen_and_nothing_loaded_writes_nothing_when_closed_minimized() {
        let t = Tracker::new(None);
        assert_eq!(t.closing(place(0.0, 0.0, 0.0, 0.0), Shown::Minimized), None);
    }

    #[test]
    fn un_maximizing_by_hand_closes_normal() {
        let loaded = WindowPlace {
            maximized: true,
            ..place(200.0, 80.0, 1300.0, 850.0)
        };
        let t = Tracker::new(Some(loaded));
        let closed = t
            .closing(place(200.0, 80.0, 1300.0, 850.0), Shown::Normal)
            .unwrap();
        assert!(!closed.maximized);
    }

    /// The composition `main` makes: the tracker starts from what `restore`
    /// opened, so a window opened maximized from an out-of-range file and closed
    /// still maximized writes back the fitted place, not the file it read.
    #[test]
    fn closing_maximized_from_an_unreachable_file_writes_back_the_fitted_place() {
        let saved = WindowPlace {
            maximized: true,
            ..place(5000.0, 100.0, 3000.0, 2000.0)
        };
        let r = restore(&saved, &[mon(0.0, 0.0, 1920.0, 1080.0, 1.0)], WIN);
        let closed = Tracker::new(Some(r.place))
            .closing(place(-8.0, -8.0, 1936.0, 1048.0), Shown::Maximized)
            .unwrap();
        assert_eq!(closed.position, None);
        assert_eq!((closed.width, closed.height), (1920.0, 1080.0));
        assert!(closed.maximized);
    }

    // --- restore ---

    #[test]
    fn a_place_on_the_one_monitor_comes_back_as_it_was() {
        let saved = place(100.0, 50.0, 1400.0, 900.0);
        let r = restore(&saved, &[mon(0.0, 0.0, 2560.0, 1440.0, 1.0)], WIN);
        assert_eq!(r.place, saved);
        assert_eq!(r.position, device(100.0, 50.0));
    }

    #[test]
    fn a_place_on_a_monitor_since_unplugged_keeps_its_size_and_loses_its_position() {
        // Saved on a second monitor to the right; only the laptop panel is left.
        let r = restore(
            &place(2700.0, 100.0, 1400.0, 900.0),
            &[mon(0.0, 0.0, 1920.0, 1080.0, 1.0)],
            WIN,
        );
        assert_eq!(r.position, None);
        assert_eq!(r.place.position, None);
        assert_eq!(size(&r), [1400.0, 900.0]);
    }

    #[test]
    fn a_place_the_os_will_put_on_the_primary_is_clamped_to_the_primary() {
        // Its third monitor is gone; a larger one than the primary is still
        // attached, but the OS opens an unplaced window on the primary.
        let r = restore(
            &place(9000.0, 100.0, 3000.0, 2000.0),
            &[
                mon(0.0, 0.0, 1920.0, 1080.0, 1.0),
                mon(1920.0, 0.0, 3840.0, 2160.0, 1.0),
            ],
            WIN,
        );
        assert_eq!(r.position, None);
        assert_eq!(size(&r), [1920.0, 1080.0]);
    }

    #[test]
    fn a_window_with_only_a_sliver_of_title_strip_on_screen_is_not_reachable() {
        // 20px of the strip on the monitor's right edge: too little to grab.
        let r = restore(
            &place(1900.0, 100.0, 1400.0, 900.0),
            &[mon(0.0, 0.0, 1920.0, 1080.0, 1.0)],
            WIN,
        );
        assert_eq!(r.position, None);
    }

    #[test]
    fn a_title_strip_above_every_monitor_is_not_reachable() {
        let r = restore(
            &place(100.0, -300.0, 1400.0, 900.0),
            &[mon(0.0, 0.0, 1920.0, 1080.0, 1.0)],
            WIN,
        );
        assert_eq!(r.position, None);
    }

    #[test]
    fn a_window_hanging_off_the_left_edge_by_most_of_its_width_still_counts() {
        let r = restore(
            &place(-1200.0, 100.0, 1400.0, 900.0),
            &[mon(0.0, 0.0, 1920.0, 1080.0, 1.0)],
            WIN,
        );
        assert_eq!(r.position, device(-1200.0, 100.0));
    }

    #[test]
    fn a_monitor_to_the_left_of_the_primary_is_found() {
        let r = restore(
            &place(-1800.0, 100.0, 1400.0, 900.0),
            &[
                mon(0.0, 0.0, 1920.0, 1080.0, 1.0),
                mon(-1920.0, 0.0, 1920.0, 1080.0, 1.0),
            ],
            WIN,
        );
        assert_eq!(r.position, device(-1800.0, 100.0));
    }

    #[test]
    fn a_size_larger_than_its_monitor_is_clamped_to_it() {
        let r = restore(
            &place(0.0, 0.0, 3000.0, 2000.0),
            &[mon(0.0, 0.0, 1920.0, 1080.0, 1.0)],
            WIN,
        );
        assert_eq!(size(&r), [1920.0, 1080.0]);
    }

    #[test]
    fn a_size_below_the_minimum_is_raised_to_it() {
        let r = restore(
            &place(10.0, 10.0, 120.0, 40.0),
            &[mon(0.0, 0.0, 1920.0, 1080.0, 1.0)],
            WIN,
        );
        assert_eq!(size(&r), [MIN_WIDTH, MIN_HEIGHT]);
    }

    #[test]
    fn a_damaged_file_opens_at_the_minimum_with_no_position_and_heals() {
        let saved = WindowPlace {
            position: Some([f64::NAN, 0.0]),
            width: f64::INFINITY,
            height: f64::NAN,
            scale: 0.0,
            maximized: false,
        };
        let r = restore(&saved, &[mon(0.0, 0.0, 1920.0, 1080.0, 1.0)], WIN);
        assert_eq!(r.position, None);
        assert_eq!(
            r.place,
            WindowPlace {
                position: None,
                width: MIN_WIDTH,
                height: MIN_HEIGHT,
                scale: 1.0,
                maximized: false,
            }
        );
    }

    #[test]
    fn no_monitors_reported_means_no_position() {
        let r = restore(&place(100.0, 50.0, 1400.0, 900.0), &[], WIN);
        assert_eq!(r.position, None);
        assert_eq!(size(&r), [1400.0, 900.0]);
    }

    #[test]
    fn no_saved_position_means_no_position() {
        let saved = WindowPlace {
            position: None,
            ..place(0.0, 0.0, 1400.0, 900.0)
        };
        let r = restore(&saved, &[mon(0.0, 0.0, 1920.0, 1080.0, 1.0)], WIN);
        assert_eq!(r.position, None);
        assert_eq!(size(&r), [1400.0, 900.0]);
    }

    #[test]
    fn maximized_comes_back() {
        let saved = WindowPlace {
            maximized: true,
            ..place(100.0, 50.0, 1400.0, 900.0)
        };
        let r = restore(&saved, &[mon(0.0, 0.0, 1920.0, 1080.0, 1.0)], WIN);
        assert!(r.place.maximized);
        assert_eq!(r.position, device(100.0, 50.0));
    }

    #[test]
    fn a_device_pixel_host_launches_at_the_saved_device_pixels() {
        // Saved on a 150% monitor right of a 100% primary: logical 2000 at 1.5
        // is device pixel 3000 — whatever monitor the window is created on.
        let saved = WindowPlace {
            scale: 1.5,
            ..place(2000.0, 100.0, 1200.0, 800.0)
        };
        let monitors = [
            mon(0.0, 0.0, 2560.0, 1440.0, 1.0),
            mon(2560.0, 0.0, 3840.0, 2160.0, 1.5),
        ];
        let r = restore(&saved, &monitors, WIN);
        assert_eq!(r.position, device(3000.0, 150.0));
        // The place keeps the saved units, for the tracker.
        assert_eq!(r.place.position, Some([2000.0, 100.0]));
    }

    #[test]
    fn a_device_pixel_host_checks_reachability_in_device_pixels() {
        // Logical 2000 read at scale 1.0 is device pixel 2000 — on the primary,
        // not the 150% monitor the same number would fall on at 1.5.
        let monitors = [
            mon(0.0, 0.0, 2560.0, 1440.0, 1.0),
            mon(2560.0, 0.0, 3840.0, 2160.0, 1.5),
        ];
        let r = restore(&place(2000.0, 100.0, 1200.0, 800.0), &monitors, WIN);
        assert_eq!(r.position, device(2000.0, 100.0));
        // Clamped to the primary's height, not the larger monitor's.
        let tall = restore(&place(100.0, 0.0, 1200.0, 3000.0), &monitors, WIN);
        assert_eq!(size(&tall)[1], 1440.0);
    }

    #[test]
    fn a_device_pixel_host_clamps_to_the_landing_monitors_logical_size() {
        let saved = WindowPlace {
            scale: 2.0,
            ..place(100.0, 50.0, 3000.0, 2000.0)
        };
        let r = restore(&saved, &[mon(0.0, 0.0, 3840.0, 2160.0, 2.0)], WIN);
        assert_eq!(size(&r), [1920.0, 1080.0]);
        assert_eq!(r.position, device(200.0, 100.0));
    }

    #[test]
    fn a_points_host_returns_the_position_as_saved_whatever_the_scales() {
        // macOS: a Retina panel at 2x, an external display at 1x to its right at
        // point 1512. winit reports the external's origin as device pixel 1512.
        let saved = WindowPlace {
            scale: 1.0,
            ..place(1700.0, 100.0, 1200.0, 800.0)
        };
        let monitors = [
            mon(0.0, 0.0, 3024.0, 1964.0, 2.0),
            mon(1512.0, 0.0, 2560.0, 1440.0, 1.0),
        ];
        let r = restore(&saved, &monitors, MAC);
        assert_eq!(r.position, points(1700.0, 100.0));
    }

    #[test]
    fn a_points_host_finds_the_retina_panel_in_points() {
        // The 2x panel is 1512 points wide, though its device width is 3024: a
        // strip starting at point 1460 has 52 points on it, short of the grip.
        let monitors = [mon(0.0, 0.0, 3024.0, 1964.0, 2.0)];
        let sliver = restore(&place(1460.0, 100.0, 1200.0, 800.0), &monitors, MAC);
        assert_eq!(sliver.position, None);
        let on = restore(&place(1000.0, 100.0, 1200.0, 800.0), &monitors, MAC);
        assert_eq!(on.position, points(1000.0, 100.0));
        assert_eq!(size(&on), [1200.0, 800.0]);
        let off = restore(&place(2000.0, 100.0, 1200.0, 800.0), &monitors, MAC);
        assert_eq!(off.position, None);
    }

    #[test]
    fn a_points_host_clamps_to_the_retina_panels_points_not_its_pixels() {
        let monitors = [mon(0.0, 0.0, 3024.0, 1964.0, 2.0)];
        let r = restore(&place(100.0, 100.0, 2000.0, 1200.0), &monitors, MAC);
        assert_eq!(r.position, points(100.0, 100.0));
        assert_eq!(size(&r), [1512.0, 982.0]);
    }

    #[test]
    fn a_file_with_only_a_size_parses() {
        let p: WindowPlace = serde_json::from_str(r#"{"width":1200,"height":800}"#).unwrap();
        assert_eq!(p.position, None);
        assert_eq!(p.scale, 1.0);
        assert!(!p.maximized);
    }
}
