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
    /// macOS only: the three lights sit at floem's hardcoded (11, 16) offset and
    /// run about 52px wide, so 72 clears them with room to breathe. Everywhere
    /// else our own controls are laid out in the header like any other child and
    /// need no reserved gap.
    pub const fn leading_inset(self) -> f64 {
        match self.host {
            Host::MacOs => 72.0,
            Host::Windows | Host::Linux => 0.0,
        }
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
