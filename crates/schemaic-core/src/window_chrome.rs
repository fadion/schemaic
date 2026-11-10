//! Which parts of the window frame *we* draw, and which the OS still draws.
//!
//! The app runs with `WindowConfig::show_titlebar(false)`, and floem reads that
//! one flag very differently per platform (see floem 0.2's
//! `app_handle::new_window`):
//!
//! - **Windows / Linux** — the window becomes genuinely undecorated. winit
//!   strips `WS_CAPTION | WS_SIZEBOX` on Windows, so there is no title bar,
//!   *and no resize border*. Both are ours to draw.
//! - **macOS** — decorations stay; the title bar just goes transparent and the
//!   content view runs full-size underneath it. The traffic lights, the native
//!   resize border and the window-move behaviour are all still the system's.
//!   What we owe it is *space*: the lights are drawn over our header, so the
//!   header's leading edge has to start clear of them.
//!
//! Ask a **capability** here, never `cfg!(target_os = ...)` at the use site. It
//! is the same rule the DB engines follow (`ddl::supports_change` and friends):
//! a `target_os = "macos"` check compiles cleanly while silently sorting a
//! fourth platform onto whichever side it happens to fall, and it scatters the
//! reason for the decision across every call site instead of stating it once
//! here.

/// The window systems this app draws chrome for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Host {
    Windows,
    /// X11 or Wayland. Both are undecorated under `show_titlebar(false)`; what
    /// differs (server-side shadow) is covered by [`Chrome::wants_drop_shadow`].
    Linux,
    MacOs,
}

impl Host {
    /// The host this binary was built for.
    ///
    /// Anything not one of the three named above is treated as [`Host::Linux`]:
    /// the other targets floem supports with a real window (the BSDs) are
    /// freedesktop platforms, and an undecorated window there behaves as it
    /// does under X11/Wayland.
    pub const fn current() -> Self {
        #[cfg(target_os = "windows")]
        {
            Host::Windows
        }
        #[cfg(target_os = "macos")]
        {
            Host::MacOs
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            Host::Linux
        }
    }
}

/// The traffic lights' leading offset, in points: floem's own default, which
/// [`Chrome::leading_inset`] is sized around and nothing has had cause to move.
pub const TRAFFIC_LIGHTS_X: f64 = 11.0;

/// How far above the y handed to winit as the traffic lights' offset their
/// centre lands, in points from the window's top edge: an offset of `y` puts
/// the centre at `y - 2.25`.
///
/// **Measured, not documented.** winit 0.29 sizes the title-bar container to
/// `button height + y` and never sets a button's y, so where the button ends
/// up is AppKit's layout. On macOS 26.7 (Apple Silicon, 2× captures of the
/// window at y = 16, 21 and 26) the red light's 14pt circle was centred at
/// 13.75, 18.75 and 23.75pt — one point per point of y, 2.25 short of it. A
/// macOS release that lays the container out differently moves this, and the
/// symptom is lights off-centre by the difference, nothing worse.
pub const LIGHT_CENTRE_ABOVE_OFFSET: f64 = 2.25;

/// What this host leaves to us once the title bar is off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chrome {
    host: Host,
}

impl Chrome {
    /// The chrome split for the running platform.
    pub const fn current() -> Self {
        Self {
            host: Host::current(),
        }
    }

    /// The chrome split for a named host — the tests' entry point, and the way
    /// to reason about a platform you are not currently building for.
    pub const fn of(host: Host) -> Self {
        Self { host }
    }

    pub const fn host(self) -> Host {
        self.host
    }

    /// Do we draw minimize / maximize / close ourselves?
    ///
    /// False on macOS, where the traffic lights survive a hidden title bar and
    /// drawing a second set would be both wrong and unusable.
    pub const fn draws_own_controls(self) -> bool {
        !matches!(self.host, Host::MacOs)
    }

    /// How many caption buttons we draw ourselves: minimize, maximize and close,
    /// or none where the OS still draws them.
    ///
    /// [`Self::draws_own_controls`] answers *whether*; this answers *how much of
    /// the title bar's trailing edge they occupy*, which is a different question
    /// with a caller of its own. A modal's backdrop covers the whole window,
    /// including the title bar, and the band the app then lays over it to keep
    /// the window draggable has to stop exactly where the buttons begin — cover
    /// them and they are as unreachable as they were before, stop short and a
    /// strip of title bar is left undimmed and dead. The count is here rather
    /// than at that band because it is a fact about *this platform's chrome*,
    /// and the module rule is to ask the capability, never the host.
    ///
    /// A count, not a width: the pixels are the UI's
    /// (`ui::window_chrome::control_w()`, the Windows caption metric scaled by
    /// the interface setting), and core has no business holding a measurement it
    /// cannot check.
    pub const fn own_control_count(self) -> usize {
        if self.draws_own_controls() { 3 } else { 0 }
    }

    /// Do we have to provide our own resize handles?
    ///
    /// This is the one that bites: an undecorated Windows window has no
    /// `WS_SIZEBOX`, so *the OS will not resize it at all* — without our own
    /// edge zones calling `drag_resize_window`, the window is stuck at its
    /// launch size.
    pub const fn draws_own_resize_border(self) -> bool {
        !matches!(self.host, Host::MacOs)
    }

    /// Do we ask the OS to keep painting a drop shadow behind the frameless
    /// window (`WindowConfig::undecorated_shadow`)?
    ///
    /// Windows-only in floem 0.2 — it maps to winit's `with_undecorated_shadow`,
    /// which is a `WindowBuilderExtWindows` method and compiles to nothing
    /// elsewhere. Wayland has no server-side shadow to ask for.
    pub const fn wants_drop_shadow(self) -> bool {
        matches!(self.host, Host::Windows)
    }

    /// Free space to leave at the header's leading edge, in logical pixels, for
    /// window controls the *OS* draws over our content.
    ///
    /// macOS only: the three lights start [`TRAFFIC_LIGHTS_X`] in, 14pt circles
    /// 23pt apart, so the green one ends near 71 (measured on macOS 26.7). 72
    /// cleared them but sat the header's first object — the logo, then; the
    /// connection switcher now, see [`Self::header_shows_mark`] — hard against
    /// the green light; 82 leaves about 11pt after it, the same margin the
    /// lights keep from the window's edge.
    /// Everywhere else our own controls are laid out in the header like any
    /// other child and need no reserved gap.
    pub const fn leading_inset(self) -> f64 {
        match self.host {
            Host::MacOs => 82.0,
            Host::Windows | Host::Linux => 0.0,
        }
    }

    /// Does the header lead with the app's mark?
    ///
    /// Not on macOS: there the traffic lights already mark the window's corner
    /// and the menu bar and the Dock name the app, so the mark was a fourth
    /// object crowding the lights rather than an identity nothing else gave.
    /// The connection switcher leads the row instead, at [`Self::leading_inset`].
    /// Where the controls are ours they sit at the trailing edge, and the mark
    /// is the only thing at the leading one that says whose window it is.
    pub const fn header_shows_mark(self) -> bool {
        match self.host {
            Host::MacOs => false,
            Host::Windows | Host::Linux => true,
        }
    }

    /// Where winit should put the traffic lights (`MacOSWindowConfig::
    /// traffic_lights_offset`) so that they sit vertically centred in a header
    /// whose visible bar — above its bottom rule — is `bar_h` points tall.
    /// `None` off macOS, where there are no OS-drawn controls to place.
    ///
    /// **Floem's own default, (11, 16), centred them in nothing.** It is a
    /// constant chosen for a standard-height title bar; Schemaic's header is
    /// taller, and the lights sat about 6pt above its middle. The y of the
    /// offset is not the lights' top or centre either — winit resizes the
    /// title-bar container to it and leaves AppKit to place the buttons — so the
    /// relation is [`LIGHT_CENTRE_ABOVE_OFFSET`], measured rather than derived.
    ///
    /// Taken once, at window creation: winit has no setter for it afterwards, so
    /// the caller passes the bar height at the interface scale the app launches
    /// with, and a scale changed mid-session re-centres them at the next launch.
    pub fn traffic_lights_offset(self, bar_h: f64) -> Option<(f64, f64)> {
        match self.host {
            Host::MacOs => Some((TRAFFIC_LIGHTS_X, bar_h / 2.0 + LIGHT_CENTRE_ABOVE_OFFSET)),
            Host::Windows | Host::Linux => None,
        }
    }

    /// Does the OS lay windows out in one space of points across every monitor,
    /// rather than in device pixels?
    ///
    /// macOS does: winit hands `WindowConfig::position` to it as points
    /// unconverted, and reports each monitor's device pixels at that monitor's
    /// own scale. Windows and X11 lay monitors out in device pixels and convert a
    /// logical position with the scale of the monitor the window is created on,
    /// so there a position is launched in device pixels instead —
    /// `window_place::LaunchPosition`. (Wayland reports no window position at all.)
    pub const fn places_windows_in_points(self) -> bool {
        matches!(self.host, Host::MacOs)
    }

    /// Does the OS show a new window before winit has sized, maximized and
    /// placed it — so the window has to be created hidden and shown once it
    /// has a frame (floem's `WindowConfig::show_after_first_frame`)?
    ///
    /// Windows does: winit's Win32 backend makes the window visible first, at
    /// the system's default size, and only then applies the requested size,
    /// the maximize and the position. A window taller than that default shows
    /// the strip it grows into black, a window restored maximized is seen at
    /// its normal bounds and then maximizing, and every window is blank white —
    /// the window class's background — until the GPU has drawn a frame.
    /// Wayland is the reason this is not simply always on: it has no hidden
    /// state, and winit ignores the request there.
    pub const fn shows_window_before_sizing(self) -> bool {
        matches!(self.host, Host::Windows)
    }
}

impl Default for Chrome {
    fn default() -> Self {
        Self::current()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Host; 3] = [Host::Windows, Host::Linux, Host::MacOs];

    #[test]
    fn windows_owns_the_whole_frame() {
        let c = Chrome::of(Host::Windows);
        assert!(c.draws_own_controls());
        assert!(c.draws_own_resize_border());
        assert!(c.wants_drop_shadow());
        assert_eq!(c.leading_inset(), 0.0);
    }

    #[test]
    fn linux_owns_the_frame_but_gets_no_shadow() {
        let c = Chrome::of(Host::Linux);
        assert!(c.draws_own_controls());
        assert!(c.draws_own_resize_border());
        // `undecorated_shadow` is a Windows-only winit extension; asking for it
        // anywhere else is a no-op, and pretending otherwise would have the UI
        // skip its own border treatment on a platform that has no shadow.
        assert!(!c.wants_drop_shadow());
        assert_eq!(c.leading_inset(), 0.0);
    }

    #[test]
    fn macos_keeps_its_native_frame_and_charges_us_space_for_it() {
        let c = Chrome::of(Host::MacOs);
        assert!(!c.draws_own_controls());
        assert!(!c.draws_own_resize_border());
        assert!(!c.wants_drop_shadow());
        assert!(c.leading_inset() > 0.0);
    }

    /// Controls and the resize border come off *together* — they are two halves
    /// of the same fact (the OS stopped drawing the frame). A future host added
    /// with one but not the other is either a window that cannot be resized or
    /// one with two sets of buttons.
    #[test]
    fn controls_and_resize_border_are_decided_together() {
        for host in ALL {
            let c = Chrome::of(host);
            assert_eq!(
                c.draws_own_controls(),
                c.draws_own_resize_border(),
                "{host:?} draws one half of the frame but not the other"
            );
        }
    }

    /// The lights' centre — the offset's y less what AppKit's layout takes off
    /// it — lands on the bar's middle, at every interface scale's bar height.
    #[test]
    fn the_traffic_lights_are_centred_in_the_bar_they_are_placed_for() {
        let c = Chrome::of(Host::MacOs);
        // The header's visible bar at 80%, 100%, 130% and 160%: 40 scaled, less
        // its 1pt bottom rule.
        for bar_h in [31.0, 39.0, 51.0, 63.0] {
            let (x, y) = c.traffic_lights_offset(bar_h).expect("macOS places them");
            assert_eq!(x, TRAFFIC_LIGHTS_X);
            assert_eq!(y - LIGHT_CENTRE_ABOVE_OFFSET, bar_h / 2.0, "bar {bar_h}");
        }
        // The case the measurement came from: a 39pt bar wants 21.75, where
        // floem's 16 put the centre at 13.75 — 5.75pt high.
        assert_eq!(c.traffic_lights_offset(39.0), Some((11.0, 21.75)));
    }

    /// The mark leads the header where nothing else stands at its leading edge,
    /// and gives way to the traffic lights where they do.
    #[test]
    fn the_header_leads_with_the_mark_only_where_the_os_draws_no_controls_there() {
        assert!(Chrome::of(Host::Windows).header_shows_mark());
        assert!(Chrome::of(Host::Linux).header_shows_mark());
        assert!(!Chrome::of(Host::MacOs).header_shows_mark());
        for host in ALL {
            let c = Chrome::of(host);
            assert_eq!(
                c.header_shows_mark(),
                c.leading_inset() == 0.0,
                "{host:?}: the mark and a leading inset for OS controls are one or the other"
            );
        }
    }

    /// Only the host whose lights the OS draws is given an offset for them.
    #[test]
    fn only_os_drawn_controls_get_a_traffic_lights_offset() {
        for host in ALL {
            let c = Chrome::of(host);
            assert_eq!(
                c.traffic_lights_offset(39.0).is_some(),
                !c.draws_own_controls(),
                "{host:?}"
            );
        }
    }

    /// The lights still clear the leading inset at their offset: three buttons
    /// 14pt wide, 23pt apart (measured on macOS 26.7), starting at the offset's x.
    #[test]
    fn the_leading_inset_clears_the_lights_at_their_offset() {
        let c = Chrome::of(Host::MacOs);
        let (x, _) = c.traffic_lights_offset(39.0).unwrap();
        // …with at least the lights' own margin from the window's edge to spare
        // before the connection switcher.
        assert!(x + 2.0 * 23.0 + 14.0 + TRAFFIC_LIGHTS_X <= c.leading_inset());
    }

    /// The leading inset exists only to clear controls *someone else* draws, so
    /// it is reserved exactly when we do not draw our own.
    #[test]
    fn leading_inset_is_reserved_only_for_os_drawn_controls() {
        for host in ALL {
            let c = Chrome::of(host);
            assert_eq!(
                c.leading_inset() > 0.0,
                !c.draws_own_controls(),
                "{host:?} reserves header space that nothing occupies (or fails to)"
            );
        }
    }

    /// The trailing edge is reserved for buttons exactly when there are buttons
    /// to reserve it for — the same shape as the leading inset above, at the
    /// other end of the bar. A host that counted controls it doesn't draw would
    /// leave a dead 138px hole in the drag band; one that drew controls it
    /// doesn't count would have the band lie on top of them.
    #[test]
    fn controls_are_counted_exactly_where_they_are_drawn() {
        for host in ALL {
            let c = Chrome::of(host);
            assert_eq!(
                c.own_control_count() > 0,
                c.draws_own_controls(),
                "{host:?} disagrees with itself about whether it has caption buttons"
            );
        }
        assert_eq!(Chrome::of(Host::Windows).own_control_count(), 3);
        assert_eq!(Chrome::of(Host::Linux).own_control_count(), 3);
        assert_eq!(Chrome::of(Host::MacOs).own_control_count(), 0);
    }

    /// A shadow is something we ask the OS for *because* the frame is gone. A
    /// host that kept its decorations already has one.
    #[test]
    fn only_a_frameless_host_asks_for_a_shadow() {
        for host in ALL {
            let c = Chrome::of(host);
            if c.wants_drop_shadow() {
                assert!(
                    c.draws_own_controls(),
                    "{host:?} asks for an undecorated shadow while still decorated"
                );
            }
        }
    }

    #[test]
    fn only_windows_shows_a_window_before_sizing_it() {
        assert!(Chrome::of(Host::Windows).shows_window_before_sizing());
        assert!(!Chrome::of(Host::MacOs).shows_window_before_sizing());
        assert!(!Chrome::of(Host::Linux).shows_window_before_sizing());
    }

    #[test]
    fn only_macos_places_windows_in_points() {
        assert!(Chrome::of(Host::MacOs).places_windows_in_points());
        assert!(!Chrome::of(Host::Windows).places_windows_in_points());
        assert!(!Chrome::of(Host::Linux).places_windows_in_points());
    }

    #[test]
    fn current_matches_the_build_target() {
        let expected = if cfg!(target_os = "windows") {
            Host::Windows
        } else if cfg!(target_os = "macos") {
            Host::MacOs
        } else {
            Host::Linux
        };
        assert_eq!(Chrome::current().host(), expected);
        assert_eq!(Chrome::default(), Chrome::of(expected));
    }
}
