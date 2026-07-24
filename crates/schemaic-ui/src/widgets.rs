//! Reusable low-level UI widgets shared across the view modules: the themed
//! popup-menu system (`menu_panel` + `MenuEntry`), modal/panel style helpers, the
//! `window_size` global, and the auto-hiding / shift-scroll wrappers.
//!
//! These were defined late in `lib.rs` but used early throughout; collecting them
//! here lets the leaf view modules depend on them without an ordering deadlock.

use std::rc::Rc;

use floem::AnyView;
use floem::event::{Event, EventListener, EventPropagation};
use floem::keyboard::{Key, NamedKey};
use floem::kurbo::Point;
use floem::prelude::*;
use floem::reactive::Scope;
use floem::style::Transition;
use floem::views::Scroll;
use floem::views::scroll::ScrollCustomStyle;

use crate::consts::*;
use crate::{icons, theme};

// ===== moved from lib.rs (widgets cluster) =====
// A title bar for a modal panel, with a close (×) button.
pub(crate) fn modal_title(title: &'static str, close: Rc<dyn Fn()>) -> impl IntoView {
    modal_title_impl(title, close, true)
}

/// Like [`modal_title`] but without the bottom separator border — for modals
/// whose body already reads as a distinct block (the plan modal's boxed table).
pub(crate) fn modal_title_borderless(title: &'static str, close: Rc<dyn Fn()>) -> impl IntoView {
    modal_title_impl(title, close, false)
}

fn modal_title_impl(title: &'static str, close: Rc<dyn Fn()>, border: bool) -> impl IntoView {
    h_stack((
        text(title).style(|s| s.font_size(15.0).font_bold().color(theme::text())),
        empty().style(|s| s.flex_grow(1.0_f32)),
        // Lucide X, 16px, 14px from the right edge (via the header's padding),
        // vertically centred. Same dim→bright colour as the old glyph.
        container(icons::icon(icons::X, 16.0))
            .on_click_stop(move |_| (close)())
            .style(|s| {
                s.flex_shrink(0.0_f32)
                    .items_center()
                    .color(theme::text_dim())
                    .hover(|s| s.color(theme::text()))
            }),
    ))
    .style(move |s| {
        s.width_full()
            .flex_row()
            .items_center()
            .padding_horiz(14.0)
            .padding_vert(10.0)
            .border_bottom(if border { 1.0 } else { 0.0 })
            .border_color(theme::border())
    })
}

pub(crate) fn menu_item_style(s: floem::style::Style) -> floem::style::Style {
    s.width_full()
        .flex_row()
        .items_center()
        .gap(8.0)
        .padding_horiz(12.0)
        .padding_vert(6.0)
        .color(theme::text())
        .hover(|s| s.background(theme::accent().multiply_alpha(0.15)))
}

pub(crate) fn panel_style(s: floem::style::Style) -> floem::style::Style {
    s.flex_col()
        .background(theme::bg_panel())
        .border(1.0)
        .border_color(theme::border())
        .border_radius(10.0)
}

/// The app's tooltip chrome, applied globally to Floem's `TooltipClass` (see the
/// root stylesheet in `lib.rs`) so every `.tooltip(…)` gets it — a compact
/// bordered panel matching the app's popovers, with a soft drop shadow lifting it
/// off the content. `color`/`font_size` are inherited, so a bare `text(…)` tip
/// picks them up.
pub(crate) fn tooltip_style(s: floem::style::Style) -> floem::style::Style {
    s.background(theme::bg_panel())
        .color(theme::text())
        .font_size(theme::FONT_LABEL)
        .border(1.0)
        .border_color(theme::border())
        .border_radius(6.0)
        .padding_horiz(9.0)
        .padding_vert(6.0)
        .box_shadow_blur(12.0)
        .box_shadow_spread(0.0)
        .box_shadow_v_offset(3.0)
        .box_shadow_color(theme::tooltip_shadow())
}

/// A toolbar / title-bar icon button with a **padded hitbox** (5px horiz / 3px
/// vert). Hover (dim→bright) is driven from a signal via `PointerEnter`/`Leave`
/// on the padded container, so the *whole* box — not just the 16px glyph —
/// highlights and takes the click. `mt`/`mr` position it (pass `0.0` when the
/// caller lays out via separators/gaps, e.g. the results grid). `enabled` dims
/// the glyph to 30% and swallows the click when false. Shared by the results-grid
/// toolbar and the Schema/AI/Terminal/History panel title bars.
pub(crate) fn toolbar_icon(
    markup: &'static str,
    mt: f64,
    mr: f64,
    enabled: impl Fn() -> bool + Copy + 'static,
    on_click: impl Fn() + 'static,
) -> impl IntoView {
    let hov = RwSignal::new(false);
    container(icons::icon(markup, 16.0).style(move |s| {
        let c = if !enabled() {
            theme::text_muted().multiply_alpha(0.3)
        } else if hov.get() {
            theme::text()
        } else {
            theme::text_muted()
        };
        s.flex_shrink(0.0_f32).color(c)
    }))
    .on_click_stop(move |_| {
        if enabled() {
            on_click();
        }
    })
    .on_event_cont(EventListener::PointerEnter, move |_| hov.set(true))
    .on_event_cont(EventListener::PointerLeave, move |_| hov.set(false))
    .style(move |s| {
        s.items_center()
            .margin_top(mt)
            .margin_right(mr)
            .padding_horiz(5.0)
            .padding_vert(3.0)
            .cursor(floem::style::CursorStyle::Default)
    })
}

/// The stored window-size signal plus the scope that owns it.
type WindowSizeSlot = (RwSignal<(f64, f64)>, Scope);

thread_local! {
    static WINDOW_SIZE: std::cell::RefCell<Option<WindowSizeSlot>> =
        const { std::cell::RefCell::new(None) };
}

/// Live window size (the root stack's size — root is window-sized), for overlays
/// that need to flip/fit near a screen edge. Set once from `workspace`'s root
/// `on_resize`; read by `menu_panel`'s submenu edge-flip.
pub(crate) fn window_size() -> RwSignal<(f64, f64)> {
    WINDOW_SIZE.with(|cell| {
        if cell.borrow().is_none() {
            // Detached scope → lives for the whole process (like the theme state).
            let scope = Scope::new();
            let sig = scope.create_rw_signal((0.0, 0.0));
            *cell.borrow_mut() = Some((sig, scope));
        }
        cell.borrow().as_ref().unwrap().0
    })
}

// ── Reusable themed popup menu (with nested submenus) ───────────────────────
//
// `menu_panel(entries, close, width)` renders a themed popup (matching the schema
// / editor context menus) from a `Vec<MenuEntry>`; a `Sub` entry hover-expands a
// nested panel to its right. The caller positions the returned panel absolutely
// (at the cursor, etc.). Dismissal: the panel absorbs its own pointer-downs so a
// root-level "pointer-down anywhere closes the menu" handler only fires for
// clicks *outside*; Escape and any action also call `close`.

/// Icon markup + a colour accessor (a `fn` so the tint follows theme switches).
pub type MenuIcon = (&'static str, fn() -> floem::peniko::Color);

/// One entry in a [`menu_panel`]. Submenus nest arbitrarily (each level tracks
/// its own open child), though two levels is the common case.
#[derive(Clone)]
pub enum MenuEntry {
    Action {
        label: String,
        icon: Option<MenuIcon>,
        /// Optional label tint (a `fn` so it follows theme switches); `None` uses
        /// the default text colour. Used to mark a selected option.
        label_color: Option<fn() -> floem::peniko::Color>,
        action: Rc<dyn Fn()>,
    },
    Sub {
        label: String,
        icon: Option<MenuIcon>,
        children: Vec<MenuEntry>,
    },
    Separator,
}

impl MenuEntry {
    pub(crate) fn action(label: impl Into<String>, action: impl Fn() + 'static) -> Self {
        MenuEntry::Action {
            label: label.into(),
            icon: None,
            label_color: None,
            action: Rc::new(action),
        }
    }
    pub(crate) fn action_icon(
        label: impl Into<String>,
        icon: MenuIcon,
        action: impl Fn() + 'static,
    ) -> Self {
        MenuEntry::Action {
            label: label.into(),
            icon: Some(icon),
            label_color: None,
            action: Rc::new(action),
        }
    }
    /// An action whose label is tinted (e.g. to mark the currently-selected option).
    pub(crate) fn action_colored(
        label: impl Into<String>,
        color: fn() -> floem::peniko::Color,
        action: impl Fn() + 'static,
    ) -> Self {
        MenuEntry::Action {
            label: label.into(),
            icon: None,
            label_color: Some(color),
            action: Rc::new(action),
        }
    }
    pub(crate) fn sub(label: impl Into<String>, children: Vec<MenuEntry>) -> Self {
        MenuEntry::Sub {
            label: label.into(),
            icon: None,
            children,
        }
    }
}

/// One menu row's content: `[icon] label [→]` (the chevron only for submenus).
/// `label_color` tints the label (a `fn` so it follows theme switches); `None`
/// uses the default text colour.
fn menu_row(
    icon: Option<MenuIcon>,
    label: String,
    label_color: Option<fn() -> floem::peniko::Color>,
    chevron: bool,
) -> impl IntoView {
    let mut kids: Vec<AnyView> = Vec::new();
    if let Some((svg, color)) = icon {
        kids.push(
            icons::icon(svg, 16.0)
                .style(move |s| s.color(color()).flex_shrink(0.0_f32))
                .into_any(),
        );
    }
    kids.push(
        text(label)
            .style(move |s| s.color(label_color.map(|c| c()).unwrap_or_else(theme::text)))
            .into_any(),
    );
    if chevron {
        kids.push(
            empty()
                .style(|s| s.flex_grow(1.0_f32).min_width(20.0))
                .into_any(),
        );
        kids.push(
            icons::icon(icons::CHEVRON_RIGHT, 14.0)
                .style(|s| s.color(theme::text_dim()).flex_shrink(0.0_f32))
                .into_any(),
        );
    }
    h_stack_from_iter(kids)
        .style(menu_item_style)
        .style(|s| s.padding_vert(8.0))
}

/// Render one entry. `open_sub` is this level's "which sibling submenu is open"
/// signal — entering a leaf clears it, entering a submenu row sets it, so moving
/// between rows switches/closes submenus while moving *onto* an open submenu (it's
/// flush with the panel's right edge) keeps it open.
fn menu_entry_view(
    i: usize,
    entry: MenuEntry,
    open_sub: RwSignal<Option<usize>>,
    close: Rc<dyn Fn()>,
) -> AnyView {
    match entry {
        MenuEntry::Separator => empty()
            .style(|s| {
                s.width_full()
                    .height(1.0)
                    .background(theme::border())
                    .margin_vert(4.0)
            })
            .into_any(),
        MenuEntry::Action {
            label,
            icon,
            label_color,
            action,
        } => menu_row(icon, label, label_color, false)
            .on_click_stop(move |_| {
                (action)();
                (close)();
            })
            .on_event(EventListener::PointerEnter, move |_| {
                open_sub.set(None);
                EventPropagation::Continue
            })
            .into_any(),
        MenuEntry::Sub {
            label,
            icon,
            children,
        } => {
            let n = children.len();
            // Submenus keep the standard width (they only appear in the grid menus).
            let sub = menu_stack(children, close, 170.0);
            // The parent row's window position/width, to decide edge-flips.
            let row_origin: RwSignal<Point> = RwSignal::new(Point::ZERO);
            let row_w = RwSignal::new(0.0_f64);
            // Wrap the panel in the absolute *container* (an absolute panel would
            // shrink-wrap and collapse its full-width rows to the text width); the
            // panel stays in-flow with its `min_width`, so rows fill it.
            let sub_wrap = container(sub).style(move |s| {
                if open_sub.get() != Some(i) {
                    return s.hide();
                }
                let (win_w, win_h) = window_size().get();
                let ro = row_origin.get();
                let rw = row_w.get();
                // Conservative size estimates (menu min_width + a row's ~34px).
                let sub_w = 210.0;
                let sub_h = n as f64 * 34.0 + 14.0;
                // Flip left if the submenu would spill past the right edge.
                let flip_x = win_w > 1.0 && ro.x + rw + sub_w > win_w;
                // Shift up if it would spill past the bottom edge (align to fit).
                let top = if win_h > 1.0 && ro.y - 6.0 + sub_h > win_h {
                    (win_h - sub_h - ro.y).max(-ro.y)
                } else {
                    -6.0 // lift so the submenu's first item lines up with this row
                };
                let s = s.absolute().inset_top(top);
                if flip_x {
                    s.inset_right_pct(100.0)
                } else {
                    s.inset_left_pct(100.0)
                }
            });
            stack((menu_row(icon, label, None, true), sub_wrap))
                .on_move(move |p| row_origin.set(p))
                .on_resize(move |r| row_w.set(r.width()))
                .on_event(EventListener::PointerEnter, move |_| {
                    open_sub.set(Some(i));
                    EventPropagation::Continue
                })
                .on_click_stop(|_| {}) // clicking the parent just holds it open
                .into_any()
        }
    }
}

/// One menu level: the styled panel of rows (used for the root and every submenu).
/// `width` is the panel's `min_width` (short labels never exceed it).
fn menu_stack(entries: Vec<MenuEntry>, close: Rc<dyn Fn()>, width: f64) -> impl IntoView {
    let open_sub: RwSignal<Option<usize>> = RwSignal::new(None);
    let rows: Vec<AnyView> = entries
        .into_iter()
        .enumerate()
        .map(|(i, e)| menu_entry_view(i, e, open_sub, close.clone()))
        .collect();
    v_stack_from_iter(rows)
        .on_event_stop(EventListener::PointerDown, |_| {})
        .style(move |s| {
            panel_style(s)
                .background(theme::bg_chrome())
                .min_width(width)
                .padding_vert(6.0)
                .font_size(theme::FONT_TITLE)
        })
}

/// A reusable themed popup menu with nested submenus, `width` px wide. Returns the
/// panel; the caller positions it absolutely. Escape (and any action) calls `close`.
pub(crate) fn menu_panel(
    entries: Vec<MenuEntry>,
    close: Rc<dyn Fn()>,
    width: f64,
) -> impl IntoView {
    let esc = close.clone();
    menu_stack(entries, close, width)
        .keyboard_navigable()
        .request_focus(|| {})
        .on_key_down(Key::Named(NamedKey::Escape), |_| true, move |_| (esc)())
}

/// Measure a string's rendered width (px) at `FONT_BODY`, through the same global
/// `FontSystem` the views paint with, so the measurement matches to the pixel.
/// Used to right-align the numeric grid editor and to size/ellipsize tab titles.
pub(crate) fn measure_text_px(text: &str) -> f64 {
    use floem::text::{Attrs, AttrsList, TextLayout};
    let attrs = Attrs::new().font_size(theme::FONT_BODY);
    let mut layout = TextLayout::new();
    layout.set_text(text, AttrsList::new(attrs));
    layout.size().width
}

/// Shared scrollbar styling. Handle color/thickness/rounding come from the global
/// `Handle` class on the root; this only adds the 3px edge inset (a per-scroll prop
/// that doesn't cascade) so the bar floats off the edge and clears the resize grip.
pub(crate) fn thin_scroll(s: ScrollCustomStyle) -> ScrollCustomStyle {
    s.vertical_track_inset(3.0).horizontal_track_inset(3.0)
}

/// Auto-hide: bars stay hidden until content is scrolled; each scroll shows them
/// and (re)arms a timer that hides them SCROLL_HIDE_MS after scrolling stops. The
/// generation guard ensures only the latest scroll's timer fires.
///
/// Per-scroll auto-hide state: a `shown` flag for `hide_bars(!shown)`, plus a
/// `poke()` to call from `on_scroll` (marks shown + re-arms the hide timer).
pub(crate) fn autohide_state() -> (RwSignal<bool>, Rc<dyn Fn()>) {
    let shown = RwSignal::new(false);
    let generation: RwSignal<u64> = RwSignal::new(0);
    let poke: Rc<dyn Fn()> = Rc::new(move || {
        shown.set(true);
        let g = generation.get_untracked().wrapping_add(1);
        generation.set(g);
        floem::action::exec_after(
            std::time::Duration::from_millis(SCROLL_HIDE_MS),
            move |_| {
                // Only hide if no later scroll re-armed the timer (and the view
                // still exists — try_get is None once its scope is disposed).
                if generation.try_get_untracked() == Some(g) {
                    shown.set(false);
                }
            },
        );
    });
    (shown, poke)
}

/// Wrap a scroll so its bars auto-hide (thin styling + `hide_bars` reactive on
/// scroll activity). Use for `scroll()`/`shift_hscroll()` views that don't need a
/// custom `on_scroll` of their own (the results grid wires this inline instead).
pub(crate) fn autohide(s: Scroll) -> Scroll {
    let (shown, poke) = autohide_state();
    s.scroll_style(move |cs| thin_scroll(cs).hide_bars(!shown.get()))
        .on_scroll(move |_| poke())
}

/// Wrap a child in a `scroll` that also treats **Shift + wheel** as horizontal
/// scrolling (common browser/app combo). The built-in scroll runs registered
/// `PointerWheel` listeners first, so we intercept Shift there and drive a
/// horizontal delta (signals don't dedupe, so repeated deltas re-fire).
pub(crate) fn shift_hscroll<V: IntoView + 'static>(child: V) -> Scroll {
    let wheel: RwSignal<floem::kurbo::Vec2> = RwSignal::new(floem::kurbo::Vec2::ZERO);
    scroll(child).scroll_delta(move || wheel.get()).on_event(
        EventListener::PointerWheel,
        move |e| {
            if let Event::PointerWheel(pe) = e
                && pe.modifiers.shift()
            {
                // Windows sends Shift+wheel as a vertical delta; map it to x.
                let dx = if pe.delta.x != 0.0 {
                    pe.delta.x
                } else {
                    pe.delta.y
                };
                if dx != 0.0 {
                    wheel.set(floem::kurbo::Vec2::new(dx, 0.0));
                }
                return EventPropagation::Stop;
            }
            EventPropagation::Continue
        },
    )
}

/// Wrap a child in a horizontal `scroll` with **permanently hidden bars** that
/// pans on a *plain* (vertical) wheel — the tab strips, where there's no vertical
/// axis, so the main wheel should nudge tabs sideways and keep overflowed tabs
/// reachable. Both wheel axes map to x; the built-in scroll runs our listener
/// before its own wheel handling, so `Stop` suppresses any default scrolling.
pub(crate) fn wheel_hscroll<V: IntoView + 'static>(child: V) -> Scroll {
    let wheel: RwSignal<floem::kurbo::Vec2> = RwSignal::new(floem::kurbo::Vec2::ZERO);
    scroll(child)
        .scroll_style(|cs| cs.hide_bars(true))
        .scroll_delta(move || wheel.get())
        .on_event(EventListener::PointerWheel, move |e| {
            if let Event::PointerWheel(pe) = e {
                let dx = if pe.delta.x != 0.0 {
                    pe.delta.x
                } else {
                    pe.delta.y
                };
                if dx != 0.0 {
                    wheel.set(floem::kurbo::Vec2::new(dx, 0.0));
                    return EventPropagation::Stop;
                }
            }
            EventPropagation::Continue
        })
}

// ── Shared bits (section headers, centered messages, panel-toggle icon) ──
pub(crate) fn section_title(t: &'static str) -> impl IntoView {
    text(t).style(|s| {
        s.font_size(theme::FONT_TITLE)
            .font_bold()
            .color(theme::text_muted())
            .padding_horiz(12.0)
            .padding_vert(8.0)
    })
}

pub(crate) fn centered_msg(msg: impl Into<String>, color: floem::peniko::Color) -> impl IntoView {
    let msg = msg.into();
    container(text(msg).style(move |s| s.color(color))).style(|s| {
        s.flex_grow(1.0_f32)
            .width_full()
            .items_center()
            .justify_center()
            .padding(16.0)
    })
}

/// Whimsical "verb spinner" verbs (Claude Code's set, trimmed of the very long
/// ones so they fit a compact loader). One is picked at random each time a loader
/// mounts; `loading_dots` then animates the trailing dots.
pub(crate) const SPINNER_VERBS: &[&str] = &[
    "Accomplishing",
    "Actioning",
    "Actualizing",
    "Architecting",
    "Baking",
    "Beaming",
    "Beboppin'",
    "Befuddling",
    "Billowing",
    "Blanching",
    "Bloviating",
    "Boogieing",
    "Boondoggling",
    "Booping",
    "Bootstrapping",
    "Brewing",
    "Burrowing",
    "Calculating",
    "Canoodling",
    "Caramelizing",
    "Cascading",
    "Catapulting",
    "Cerebrating",
    "Channeling",
    "Channelling",
    "Choreographing",
    "Churning",
    "Clauding",
    "Coalescing",
    "Cogitating",
    "Combobulating",
    "Composing",
    "Computing",
    "Concocting",
    "Considering",
    "Contemplating",
    "Cooking",
    "Crafting",
    "Creating",
    "Crunching",
    "Crystallizing",
    "Cultivating",
    "Deciphering",
    "Deliberating",
    "Determining",
    "Dilly-dallying",
    "Doing",
    "Doodling",
    "Drizzling",
    "Ebbing",
    "Effecting",
    "Elucidating",
    "Embellishing",
    "Enchanting",
    "Envisioning",
    "Evaporating",
    "Fermenting",
    "Finagling",
    "Flambeing",
    "Flowing",
    "Flummoxing",
    "Fluttering",
    "Forging",
    "Forming",
    "Frolicking",
    "Frosting",
    "Gallivanting",
    "Galloping",
    "Garnishing",
    "Generating",
    "Germinating",
    "Gitifying",
    "Grooving",
    "Gusting",
    "Harmonizing",
    "Hashing",
    "Hatching",
    "Herding",
    "Honking",
    "Hullaballooing",
    "Hyperspacing",
    "Ideating",
    "Imagining",
    "Improvising",
    "Incubating",
    "Inferring",
    "Infusing",
    "Ionizing",
    "Jitterbugging",
    "Julienning",
    "Kneading",
    "Leavening",
    "Levitating",
    "Lollygagging",
    "Manifesting",
    "Marinating",
    "Meandering",
    "Metamorphosing",
    "Misting",
    "Moonwalking",
    "Moseying",
    "Mulling",
    "Mustering",
    "Musing",
    "Nebulizing",
    "Nesting",
    "Newspapering",
    "Noodling",
    "Nucleating",
    "Orbiting",
    "Orchestrating",
    "Osmosing",
    "Perambulating",
    "Percolating",
    "Perusing",
    "Philosophising",
    "Pollinating",
    "Pondering",
    "Pontificating",
    "Pouncing",
    "Precipitating",
    "Processing",
    "Proofing",
    "Propagating",
    "Puttering",
    "Puzzling",
    "Quantumizing",
    "Razzmatazzing",
    "Reticulating",
    "Roosting",
    "Ruminating",
    "Sauteing",
    "Scampering",
    "Schlepping",
    "Scurrying",
    "Seasoning",
    "Shenaniganing",
    "Shimmying",
    "Simmering",
    "Skedaddling",
    "Sketching",
    "Slithering",
    "Smooshing",
    "Sock-hopping",
    "Spelunking",
    "Spinning",
    "Sprouting",
    "Stewing",
    "Sublimating",
    "Swirling",
    "Swooping",
    "Symbioting",
    "Synthesizing",
    "Tempering",
    "Thinking",
    "Thundering",
    "Tinkering",
    "Tomfoolering",
    "Topsy-turvying",
    "Transfiguring",
    "Transmuting",
    "Twisting",
    "Undulating",
    "Unfurling",
    "Unravelling",
    "Vibing",
    "Waddling",
    "Wandering",
    "Warping",
    "Whirlpooling",
    "Whirring",
    "Whisking",
    "Wibbling",
    "Working",
    "Wrangling",
    "Zesting",
    "Zigzagging",
];

/// Pick a random spinner verb. Seeded off the wall clock (std-only) — good enough
/// for a cosmetic loader; a fresh verb each time a loader mounts.
pub(crate) fn pick_spinner_verb() -> &'static str {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as usize)
        .unwrap_or(0);
    SPINNER_VERBS[seed % SPINNER_VERBS.len()]
}

/// A "verb spinner" loader: a random whimsical verb + animated trailing dots
/// (`loading_dots`). Shared by the AI panel, the Ctrl+K inline AI, and the query
/// runner so they all read the same. The verb is fixed for the life of this
/// loader instance; the dots cycle.
pub(crate) fn verb_spinner(color: fn() -> floem::peniko::Color, font_size: f32) -> impl IntoView {
    loading_dots(pick_spinner_verb(), color, font_size)
}

/// An animated loading label — `prefix` followed by a cycling `.` → `..` → `...`
/// on a 400ms timer (instead of a static `…`). The timer self-reschedules and
/// stops when the view's scope is disposed (`try_update` → `None`), so it can't
/// outlive a `dyn_container` rebuild (same pattern as the AI elapsed timer).
/// Rendered pixel width of `text` at `font_size` in the app's default font, via a
/// throwaway `TextLayout` (same global `FontSystem` the label renders with).
pub(crate) fn measure_px(text: &str, font_size: f32) -> f64 {
    use floem::text::{Attrs, AttrsList, TextLayout};
    let mut layout = TextLayout::new();
    layout.set_text(text, AttrsList::new(Attrs::new().font_size(font_size)));
    layout.size().width
}

pub(crate) fn loading_dots(
    prefix: &'static str,
    color: fn() -> floem::peniko::Color,
    font_size: f32,
) -> impl IntoView {
    let step = RwSignal::new(1usize);
    // Reserve the full `prefix...` width up front so the label keeps a fixed size
    // as the dots cycle (1→2→3) — otherwise it reflows, jittering when centred (the
    // query runner) or shoving a neighbour (Ctrl+K's Cancel). +2px guards sub-pixel
    // rounding so the 3-dot state never exceeds the reserved box.
    let w = measure_px(&format!("{prefix}..."), font_size) + 2.0;
    fn tick(step: RwSignal<usize>) {
        floem::action::exec_after(std::time::Duration::from_millis(400), move |_| {
            if step
                .try_update(|n| *n = if *n >= 3 { 1 } else { *n + 1 })
                .is_some()
            {
                tick(step);
            }
        });
    }
    tick(step);
    dyn_container(
        move || step.get(),
        move |n| {
            text(format!("{prefix}{}", ".".repeat(n)))
                .style(move |s| s.color(color()).font_size(font_size).min_width(w))
                .into_any()
        },
    )
}

// A status-bar panel toggle rendered as a 16px icon: `chip_active` when its
// panel is open, `chip_idle` (brightening on hover) when closed.
pub(crate) fn toggle_icon(
    glyph: &'static str,
    active: impl Fn() -> bool + 'static,
    on_click: impl Fn() + 'static,
) -> floem::views::Container {
    toggle_icon_view(
        icons::icon(glyph, 16.0).style(|s| s.flex_shrink(0.0_f32)),
        active,
        on_click,
    )
}

/// Like [`toggle_icon`] but takes a pre-built icon view — for non-square glyphs
/// (e.g. the footer AI wordmark) that can't go through `icons::icon`'s square size.
pub(crate) fn toggle_icon_view(
    icon: impl IntoView + 'static,
    active: impl Fn() -> bool + 'static,
    on_click: impl Fn() + 'static,
) -> floem::views::Container {
    // Wrap the glyph in a container that carries the padding + click handler:
    // Floem hit-tests an `Svg` against its rendered content only (padding on the
    // svg grows layout but not the click target), whereas a container hit-tests its
    // whole padded box. The icon inherits the colour via `currentColor`, so the
    // active/hover tint set on the container reaches the svg.
    container(icon)
        .on_click_stop(move |_| on_click())
        .style(move |s| {
            // No pointer cursor — the app uses the normal cursor everywhere.
            let s = s
                .items_center()
                .flex_shrink(0.0_f32)
                .padding_vert(3.0)
                .padding_horiz(5.0);
            if active() {
                s.color(theme::chip_active())
            } else {
                s.color(theme::chip_idle())
                    .hover(|s| s.color(theme::chip_active()))
            }
        })
}

/// A 22px jump-to-bottom circle (chevron-down) that fades in only while `show()`
/// is true and is inert (no pointer events) otherwise. Absolutely positioned
/// bottom-right (10px/10px) inside its parent stack. Shared by the AI panel and
/// the terminal. Fades via alpha (Floem has no opacity prop); the icon owns its
/// own colour + transition since an inherited colour won't animate a child svg.
pub(crate) fn jump_to_bottom_button(
    show: impl Fn() -> bool + Copy + 'static,
    on_click: impl Fn() + 'static,
) -> impl IntoView {
    let hovered = RwSignal::new(false);
    let anim = || Transition::ease_in_out(std::time::Duration::from_millis(150));
    let icon = icons::icon(icons::CHEVRON_DOWN, 16.0).style(move |s| {
        let color = if !show() {
            theme::jump_icon().multiply_alpha(0.0)
        } else if hovered.get() {
            theme::jump_icon_hover()
        } else {
            theme::jump_icon()
        };
        s.color(color).transition_color(anim())
    });
    container(icon)
        .on_click_stop(move |_| on_click())
        .on_event(EventListener::PointerEnter, move |_| {
            hovered.set(true);
            EventPropagation::Continue
        })
        .on_event(EventListener::PointerLeave, move |_| {
            hovered.set(false);
            EventPropagation::Continue
        })
        .style(move |s| {
            let bg = if show() {
                theme::bg_deepest()
            } else {
                theme::bg_deepest().multiply_alpha(0.0)
            };
            s.absolute()
                .inset_right(10.0)
                .inset_bottom(10.0)
                .width(22.0)
                .height(22.0)
                .border_radius(11.0)
                .items_center()
                .justify_center()
                .background(bg)
                .transition_background(anim())
        })
        .pointer_events(show)
}
