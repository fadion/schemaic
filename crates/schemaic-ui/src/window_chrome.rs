//! Client-side window decorations — the caption buttons, the drag strip and the
//! resize border the app draws itself now that the window has no title bar.
//!
//! `schemaic_core::window_chrome::Chrome` decides *what* this platform leaves to
//! us; this module draws it. Every branch here asks that capability — there is
//! no `cfg!(target_os = ...)` in this file, and adding one is the regression.
//!
//! **The pieces, and why they are separate views.** Floem dispatches
//! `PointerDown` to the deepest view first and only stops when a handler says
//! so, and `on_click_stop` stops *`Click`*, not `PointerDown`. So a drag handler
//! hung on the whole header would also fire on the connection switcher and the
//! toolbar glyphs: pressing them would start dragging the window. The drag strip
//! is therefore its own view occupying only the gap *between* the header's two
//! clusters, and the buttons sit outside it.
//!
//! **Double-click and drag out of one handler.** Starting an OS move loop on
//! press means the second press of a double-click may never arrive as a
//! `DoubleClick`. Floem stamps every `PointerDown` with a multi-click `count`,
//! so both gestures are decided in the same handler before either commits:
//! `count >= 2` maximizes, anything else drags.

use floem::action::{drag_resize_window, drag_window, minimize_window};
use floem::close_window;
use floem::event::{Event, EventListener};
use floem::peniko::Color;
use floem::prelude::*;
use floem::style::{CursorStyle, Style};
use floem::window::{ResizeDirection, WindowId};
use floem::{AnyView, WindowIdExt};

use schemaic_core::window_chrome::Chrome;

use crate::{icons, theme};

/// Width of one caption button. 46px is the Windows 11 caption metric; macOS
/// never sees these (its traffic lights are the OS's own).
const CONTROL_W: f64 = 46.0;

/// The caption glyphs are drawn on a 10×10 box — see `icons::WINDOW_*`.
const GLYPH: f32 = 10.0;

/// How wide a grab band the window edges get. 5px is about the Windows frame's
/// own, and it is the most that can be stolen from the content beneath without
/// the schema panel's own edge feeling unreachable.
const EDGE: f64 = 5.0;

/// Corners take a longer bite than the edges so a diagonal resize is actually
/// hittable — a 5×5 corner is a coin toss between the two axes.
const CORNER: f64 = 14.0;

/// The window's own chrome: the id needed to talk to it, plus the maximized flag
/// the caption glyph reads.
///
/// `is_maximized()` is a plain query, not a signal, so the flag is mirrored here
/// and refreshed by [`WindowChrome::sync`] — which the shell calls on resize, so
/// snapping or maximizing by *any* route (our button, a drag to the screen edge,
/// the OS shortcut) lands on the same value.
#[derive(Clone, Copy)]
pub struct WindowChrome {
    window: WindowId,
    maximized: RwSignal<bool>,
    chrome: Chrome,
}

impl WindowChrome {
    pub fn new(window: WindowId) -> Self {
        Self {
            window,
            maximized: RwSignal::new(window.is_maximized()),
            chrome: Chrome::current(),
        }
    }

    /// Re-read the window's real maximized state. Cheap, and idempotent — the
    /// signal only notifies when the value actually changed.
    pub fn sync(self) {
        let now = self.window.is_maximized();
        if self.maximized.get_untracked() != now {
            self.maximized.set(now);
        }
    }

    /// Space the header must leave clear at its leading edge for controls the OS
    /// draws over our content (macOS traffic lights); 0 where we draw our own.
    pub fn leading_inset(self) -> f64 {
        self.chrome.leading_inset()
    }

    fn toggle_maximized(self) {
        let next = !self.window.is_maximized();
        self.window.maximized(next);
        self.maximized.set(next);
    }

    /// The empty band between the header's clusters: press-and-move drags the
    /// window, double-click maximizes it.
    ///
    /// Present on every platform. macOS keeps its native title bar behaviour,
    /// but the bar is transparent and our content runs underneath it, so the
    /// draggable region is ours to provide there too.
    pub fn drag_strip(self) -> impl IntoView {
        let this = self;
        empty()
            .on_event_stop(EventListener::PointerDown, move |e| {
                let Event::PointerDown(p) = e else { return };
                if !p.button.is_primary() {
                    return;
                }
                if p.count >= 2 {
                    this.toggle_maximized();
                } else {
                    drag_window();
                }
            })
            .style(|s| {
                // `flex_basis(0)` with the grow: a lone flex-grow spacer keeps its
                // `auto` basis and under-claims the gap (the same trap the schema
                // panel's title row documents), which here would leave a dead
                // strip of header that looks draggable and isn't.
                s.flex_grow(1.0_f32)
                    .flex_basis(0.0)
                    .min_width(0.0)
                    .height_full()
            })
    }

    /// Minimize / maximize / close, for the platforms where the OS stopped
    /// drawing them. Empty on macOS, which still has its traffic lights.
    pub fn controls(self) -> impl IntoView {
        if !self.chrome.draws_own_controls() {
            return empty().into_any();
        }
        let this = self;
        let maximized = self.maximized;
        let window = self.window;

        let minimize = control_button(
            icons::icon(icons::WINDOW_MINIMIZE, GLYPH).into_any(),
            theme::caption_hover,
            theme::text,
            minimize_window,
        );

        // The glyph is the window's state, so it is rebuilt from the signal
        // rather than swapped by style: a frame while restored, a double frame
        // while maximized.
        let maximize = control_button(
            dyn_container(
                move || maximized.get(),
                move |m| {
                    let glyph = if m {
                        icons::WINDOW_RESTORE
                    } else {
                        icons::WINDOW_MAXIMIZE
                    };
                    icons::icon(glyph, GLYPH).into_any()
                },
            )
            .into_any(),
            theme::caption_hover,
            theme::text,
            move || this.toggle_maximized(),
        );

        // **`close_window`, not `quit_app`.** Closing the window is what runs
        // `WindowHandle::destroy`, and that is what fires `WindowClosed` — which
        // is the event the app hangs `flush_session` off, the one write that
        // saves open tabs on the way out. `quit_app` exits the event loop
        // straight away and would drop the session. Floem exits on its own once
        // the last window is gone.
        let close = control_button(
            icons::icon(icons::WINDOW_CLOSE, GLYPH).into_any(),
            theme::caption_close_hover,
            theme::caption_close_glyph,
            move || close_window(window),
        );

        h_stack((minimize, maximize, close))
            .style(|s| s.flex_row().items_center().height_full())
            .into_any()
    }

    /// The eight grab zones around the window edge, as **loose siblings** to be
    /// spread into the stack that wraps the app root — edges first, corners
    /// last. See `workspace` for the mounting, and the note below for why they
    /// cannot be handed over as one view.
    ///
    /// **Never wrap these in a full-window container.** Floem's pointer dispatch
    /// walks a view's children in reverse, skips any whose layout rect misses
    /// the point (`EventCx::should_send`) — and then `break`s on the first child
    /// it *did* deliver to, whatever that child returned
    /// (`context.rs`, "if event.is_pointer() { break }"). A wrapper covering the
    /// window is therefore hit first and ends the walk for every press in the
    /// app, no matter that it holds no handler and returns `Continue`: it does
    /// not pass anything through, it swallows the window. Left in for one build,
    /// that is exactly what it did — nothing in the app was clickable.
    /// `Decorators::pointer_events(false)` is not the escape either; it makes
    /// `should_send` reject the wrapper *and with it the whole subtree*, so the
    /// zones inside would go dead instead. Eight small siblings have no such
    /// problem: each one misses the point and is skipped, and the walk carries
    /// on down to the app.
    ///
    /// All eight are empty on macOS, where the native frame still resizes the
    /// window. On Windows they are not a nicety — winit strips `WS_SIZEBOX` from
    /// an undecorated window, so without them it cannot be resized at all.
    pub fn resize_zones(self) -> [AnyView; 8] {
        if !self.chrome.draws_own_resize_border() {
            return std::array::from_fn(|_| empty().into_any());
        }
        [
            Edge::North,
            Edge::South,
            Edge::West,
            Edge::East,
            Edge::NorthWest,
            Edge::NorthEast,
            Edge::SouthWest,
            Edge::SouthEast,
        ]
        .map(|edge| zone(edge).into_any())
    }
}

/// One caption button: a full-height hit target with a centred glyph.
///
/// The hover colours arrive as `fn() -> Color` and are called *inside* the style
/// closure, so a theme switch repaints them (a captured `Color` would freeze the
/// button at the theme it was built under).
fn control_button(
    glyph: AnyView,
    hover_bg: fn() -> Color,
    hover_fg: fn() -> Color,
    on_press: impl Fn() + 'static,
) -> impl IntoView {
    container(glyph)
        .on_click_stop(move |_| on_press())
        .style(move |s| {
            s.width(CONTROL_W)
                .height_full()
                .items_center()
                .justify_center()
                .flex_shrink(0.0_f32)
                .color(theme::text_muted())
                .hover(|s| s.background(hover_bg()).color(hover_fg()))
        })
}

/// Which part of the frame a grab zone covers.
#[derive(Clone, Copy)]
enum Edge {
    North,
    South,
    West,
    East,
    NorthWest,
    NorthEast,
    SouthWest,
    SouthEast,
}

impl Edge {
    fn direction(self) -> ResizeDirection {
        match self {
            Edge::North => ResizeDirection::North,
            Edge::South => ResizeDirection::South,
            Edge::West => ResizeDirection::West,
            Edge::East => ResizeDirection::East,
            Edge::NorthWest => ResizeDirection::NorthWest,
            Edge::NorthEast => ResizeDirection::NorthEast,
            Edge::SouthWest => ResizeDirection::SouthWest,
            Edge::SouthEast => ResizeDirection::SouthEast,
        }
    }

    fn cursor(self) -> CursorStyle {
        match self {
            Edge::North => CursorStyle::NResize,
            Edge::South => CursorStyle::SResize,
            Edge::West => CursorStyle::WResize,
            Edge::East => CursorStyle::EResize,
            Edge::NorthWest | Edge::SouthEast => CursorStyle::NwseResize,
            Edge::NorthEast | Edge::SouthWest => CursorStyle::NeswResize,
        }
    }

    /// Place the zone by insets alone, so no zone needs to know the window size:
    /// an edge pins three sides and takes its thickness from the fourth, a
    /// corner pins two and is a fixed square. The edges stop `CORNER` short at
    /// each end, which is what leaves the corners uncontested.
    fn style(self, s: Style) -> Style {
        let s = s.absolute();
        match self {
            Edge::North => s
                .inset_top(0.0)
                .inset_left(CORNER)
                .inset_right(CORNER)
                .height(EDGE),
            Edge::South => s
                .inset_bottom(0.0)
                .inset_left(CORNER)
                .inset_right(CORNER)
                .height(EDGE),
            Edge::West => s
                .inset_left(0.0)
                .inset_top(CORNER)
                .inset_bottom(CORNER)
                .width(EDGE),
            Edge::East => s
                .inset_right(0.0)
                .inset_top(CORNER)
                .inset_bottom(CORNER)
                .width(EDGE),
            Edge::NorthWest => s
                .inset_top(0.0)
                .inset_left(0.0)
                .width(CORNER)
                .height(CORNER),
            Edge::NorthEast => s
                .inset_top(0.0)
                .inset_right(0.0)
                .width(CORNER)
                .height(CORNER),
            Edge::SouthWest => s
                .inset_bottom(0.0)
                .inset_left(0.0)
                .width(CORNER)
                .height(CORNER),
            Edge::SouthEast => s
                .inset_bottom(0.0)
                .inset_right(0.0)
                .width(CORNER)
                .height(CORNER),
        }
    }
}

fn zone(edge: Edge) -> impl IntoView {
    empty()
        .on_event_stop(EventListener::PointerDown, move |e| {
            let Event::PointerDown(p) = e else { return };
            if p.button.is_primary() {
                drag_resize_window(edge.direction());
            }
        })
        .style(move |s| edge.style(s).cursor(edge.cursor()))
}
