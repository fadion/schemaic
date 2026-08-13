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

// ── Focus roots ─────────────────────────────────────────────────────────────
//
// Floem delivers a key event *directly* to the focused view and, when that view
// consumes it, to nobody else — a focused view's ancestors never see it, since
// the dispatch is `directed`. Floem's editor consumes every `KeyDown`, so while
// a text field has focus an enclosing modal's `on_key_down(Escape)` can't fire:
// the field swallows Escape and the modal has no way to close from the keyboard.
//
// So Escape in a field (one that hasn't claimed the key itself) hands focus back
// to the innermost mounted overlay, and the *next* Escape reaches that overlay's
// own handler — which is the two-step the user sees: blur, then close.
//
// A thread-local `Vec`, not a signal: this is UI-thread-only bookkeeping nothing
// renders from, and a signal would turn every open/close into a notification. A
// `Vec` rather than one slot because overlays nest (the DDL preview opens over
// the designer), and `retain` rather than `pop` because they don't always unmount
// innermost-first.
thread_local! {
    static FOCUS_ROOTS: std::cell::RefCell<Vec<floem::ViewId>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Mark `view` as the overlay that owns the keyboard while it is mounted: it
/// takes focus on build (so its own key handlers fire straight away) and becomes
/// what Escape in a text field inside it returns focus to. Replaces the
/// `.keyboard_navigable().request_focus(|| {})` pair every modal used to spell
/// out — go through this instead, or the modal's Escape stops working the moment
/// a field is focused.
///
/// A field is *not* a focus root: the grid's inline cell editor takes focus the
/// same way and deliberately stays off this list. Don't chain your own
/// `.on_cleanup` onto the result either — floem keeps a single cleanup slot per
/// view, so a second one silently replaces the unregister.
pub(crate) fn focus_root<V: IntoView + 'static>(view: V) -> V::V {
    let view = view.into_view();
    let id = view.id();
    FOCUS_ROOTS.with_borrow_mut(|s| s.push(id));
    view.keyboard_navigable()
        .request_focus(|| {})
        .on_cleanup(move || FOCUS_ROOTS.with_borrow_mut(|s| s.retain(|x| *x != id)))
}

/// The innermost mounted [`focus_root`], or `None` when no overlay is open (a
/// field in the main workspace then simply drops focus on Escape).
pub(crate) fn innermost_focus_root() -> Option<floem::ViewId> {
    FOCUS_ROOTS.with_borrow(|s| s.last().copied())
}

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

/// [`modal_title`] for a title that isn't known at compile time (the import
/// modal names the table it's loading into).
pub(crate) fn modal_title_owned(title: String, close: Rc<dyn Fn()>) -> impl IntoView {
    modal_title_impl(title, close, true)
}

fn modal_title_impl(title: impl Into<String>, close: Rc<dyn Fn()>, border: bool) -> impl IntoView {
    let title = title.into();
    h_stack((
        text(title).style(|s| s.font_size(15.0).font_bold().color(theme::text())),
        empty().style(|s| s.flex_grow(1.0_f32)),
        // Lucide X, 16px, vertically centred; `padding(6)` enlarges the click
        // hitbox (same idiom as `toolbar_icon`) so it's not fiddly to hit. Same
        // dim→bright colour as the old glyph.
        container(icons::icon(icons::X, 16.0))
            .on_click_stop(move |_| (close)())
            .style(|s| {
                s.flex_shrink(0.0_f32)
                    .items_center()
                    .padding(6.0)
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

// ===== modal form chrome =====
// The shape every modal form in the app wears — Manage Connections set it, the
// import modal followed it, and the schema designer follows both. Collected here
// rather than copied a third time, because "consistent" that lives in three
// files stops being consistent the first time one of them is tweaked.

/// Gap between form rows.
pub(crate) const FORM_GAP: f64 = 18.0;

/// A labelled control: caption above, control below.
pub(crate) fn form_setting(label: &'static str, control: impl IntoView + 'static) -> impl IntoView {
    form_setting_owned(label.to_string(), control)
}

/// [`form_setting`] for a caption that isn't known at compile time.
pub(crate) fn form_setting_owned(label: String, control: impl IntoView + 'static) -> impl IntoView {
    v_stack((
        text(label).style(|s| s.color(theme::text_dim()).font_size(theme::FONT_LABEL)),
        control,
    ))
    .style(|s| s.flex_col().gap(6.0).width_full())
}

/// A small bold section heading.
pub(crate) fn form_section(label: &'static str) -> impl IntoView {
    text(label).style(|s| {
        s.font_size(theme::FONT_BODY)
            .font_bold()
            .color(theme::text())
    })
}

/// A rule between sections: the same weight and colour as the one under a modal
/// header, inset with the rest of the body content.
///
/// The margin is 20 *minus* the enclosing stack's gap, which also applies above
/// and below — so the gap that lands on screen is exactly 20, not 28.
pub(crate) fn form_separator(stack_gap: f64) -> impl IntoView {
    empty().style(move |s| {
        s.width_full()
            .height(1.0)
            .flex_shrink(0.0_f32)
            .background(theme::border())
            .margin_vert(20.0 - stack_gap)
    })
}

/// A bordered control button (Choose file…, + Column), wearing the same chrome as
/// the header's Retry and the ER-diagram toolbar rather than Floem's default
/// button.
pub(crate) fn control_button(
    label: impl Into<String>,
    on_click: impl Fn() + 'static,
) -> impl IntoView {
    text(label.into())
        .on_click_stop(move |_| on_click())
        .style(|s| {
            control_surface(s)
                .font_size(theme::FONT_BODY)
                .color(theme::text())
                .padding_horiz(10.0)
                .padding_vert(5.0)
                .flex_shrink(0.0_f32)
                .hover(|s| s.background(theme::control_hover()))
        })
}

/// A footer action, styled like Manage Connections' Test / Save: text only, a
/// colour that carries the meaning, brightening on hover.
pub(crate) fn footer_button(
    label: impl Into<String>,
    color: fn() -> floem::peniko::Color,
    hover: fn() -> floem::peniko::Color,
    enabled: bool,
    on_click: impl Fn() + 'static,
) -> impl IntoView {
    text_button(label, color, hover, enabled, (6.0, 4.0), on_click)
}

/// [`footer_button`] at the roomier size the question dialogs use.
///
/// The transaction prompt and the confirm modal each carried a private copy of
/// this — same colour-fn signature, same hover, same radius, differing only in
/// padding, which is the reason they weren't already sharing one. The size is a
/// parameter now rather than a third implementation, so a change to how a text
/// button behaves reaches all of them.
pub(crate) fn dialog_button(
    label: impl Into<String>,
    color: fn() -> floem::peniko::Color,
    hover: fn() -> floem::peniko::Color,
    on_click: impl Fn() + 'static,
) -> impl IntoView {
    text_button(label, color, hover, true, (10.0, 5.0), on_click)
}

fn text_button(
    label: impl Into<String>,
    color: fn() -> floem::peniko::Color,
    hover: fn() -> floem::peniko::Color,
    enabled: bool,
    (pad_h, pad_v): (f64, f64),
    on_click: impl Fn() + 'static,
) -> impl IntoView {
    text(label.into())
        .on_click_stop(move |_| {
            if enabled {
                on_click()
            }
        })
        .style(move |s| {
            let s = s
                .font_size(theme::FONT_BODY)
                .padding_horiz(pad_h)
                .padding_vert(pad_v)
                .border_radius(6.0);
            if enabled {
                s.color(color()).hover(move |s| s.color(hover()))
            } else {
                // Dimmed and inert rather than hidden: the button staying put is
                // what makes it obvious which step you're on.
                s.color(theme::text_faint())
            }
        })
}

/// The bordered bar a modal's actions sit in — quiet actions on the right, the
/// affirmative one last.
pub(crate) fn modal_footer(actions: impl IntoView + 'static) -> impl IntoView {
    modal_footer_split(empty(), actions)
}

/// [`modal_footer`] with something pinned to the *left* as well — a status the
/// footer's buttons act on (the designer's change count), which belongs at the
/// far edge rather than crowded against them.
pub(crate) fn modal_footer_split(
    status: impl IntoView + 'static,
    actions: impl IntoView + 'static,
) -> impl IntoView {
    h_stack((
        status,
        empty().style(|s| s.flex_grow(1.0_f32).min_width(10.0)),
        actions,
    ))
    .style(|s| {
        s.width_full()
            .flex_row()
            .items_center()
            .padding_horiz(14.0)
            .padding_vert(10.0)
            .border_top(1.0)
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
///
/// A tip is mounted as a Floem *overlay* (a child of the window root), so the
/// root stylesheet's non-selectable-label rule can't reach it — this style is
/// applied directly to the tip view, so it carries the rule itself (as a direct
/// prop for a bare `text(…)` tip, and as a class rule for a wrapped one).
pub(crate) fn tooltip_style(s: floem::style::Style) -> floem::style::Style {
    s.background(theme::bg_panel())
        .color(theme::text())
        .font_size(theme::FONT_LABEL)
        .selectable(false)
        .class(floem::views::LabelClass, |s| s.selectable(false))
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
        /// Dimmed + inert (no click, no hover) — for an action that isn't currently
        /// applicable (e.g. "AI Fill Value" with no cell selected).
        disabled: bool,
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
            disabled: false,
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
            disabled: false,
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
            disabled: false,
            action: Rc::new(action),
        }
    }
    /// Mark this entry disabled (dimmed + inert). No-op on `Sub`/`Separator`.
    pub(crate) fn disabled(mut self, yes: bool) -> Self {
        if let MenuEntry::Action { disabled, .. } = &mut self {
            *disabled = yes;
        }
        self
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
    disabled: bool,
) -> impl IntoView {
    let mut kids: Vec<AnyView> = Vec::new();
    if let Some((svg, color)) = icon {
        kids.push(
            icons::icon(svg, 16.0)
                .style(move |s| {
                    let c = if disabled {
                        theme::text_muted().multiply_alpha(0.3)
                    } else {
                        color()
                    };
                    s.color(c).flex_shrink(0.0_f32)
                })
                .into_any(),
        );
    }
    kids.push(
        text(label)
            .style(move |s| {
                let c = if disabled {
                    theme::text_muted().multiply_alpha(0.3)
                } else {
                    label_color.map(|c| c()).unwrap_or_else(theme::text)
                };
                s.color(c)
            })
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
        .style(move |s| {
            let s = s.padding_vert(8.0);
            // A disabled row suppresses the hover highlight so it reads as inert.
            if disabled {
                s.hover(|h| h.background(floem::peniko::Color::TRANSPARENT))
            } else {
                s
            }
        })
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
            disabled,
            action,
        } => menu_row(icon, label, label_color, false, disabled)
            .on_click_stop(move |_| {
                if disabled {
                    return; // inert; the stop keeps the menu open
                }
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
            stack((menu_row(icon, label, None, true, false), sub_wrap))
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

/// Gap between the cursor and the corner of a menu opened at it.
pub(crate) const CURSOR_MENU_GAP: f64 = 3.0;

/// Estimated height (px) of the panel [`menu_panel`] builds for `entries`.
///
/// Summed per entry *kind*: an action row is ≈30.5px (14px line + 8px padding on
/// both sides − sub-pixel), a separator ≈9px (a 1px rule + 4px margins), plus the
/// panel's own 6px vertical padding and 1px border on both sides. Counting
/// separators as full rows shoved an upward-flipped panel tens of px too high.
///
/// It is an estimate on purpose: it decides *placement*, not whether to flip, and
/// measuring for real would mean laying the panel out first — which is what
/// produces an open-then-flip flicker.
pub(crate) fn menu_panel_height(entries: &[MenuEntry]) -> f64 {
    entries
        .iter()
        .map(|e| match e {
            MenuEntry::Separator => 9.0,
            _ => 30.5,
        })
        .sum::<f64>()
        + 14.0
}

/// Top-left corner for a panel opened **at the cursor**: `gap` px down and right
/// of it, flipped to the other side of the cursor on whichever axis would run past
/// the window edge, and never negative.
///
/// Shared by both menu channels. It used to live in the grid's overlay only, so a
/// right-click low in the schema tree ran its last entries — Truncate, Drop, AI
/// Explain — off the bottom of the window with no cue that they existed, while a
/// grid cell right-clicked at the same height flipped up correctly.
///
/// A `window` dimension of 0 (or less than 1) means "not measured yet" and
/// suppresses the flip on that axis: guessing at an unknown edge is worse than
/// opening down-right and being off by a frame.
pub(crate) fn cursor_menu_pos(
    cursor: (f64, f64),
    panel: (f64, f64),
    window: (f64, f64),
    gap: f64,
) -> (f64, f64) {
    let flip = |c: f64, size: f64, win: f64| {
        if win > 1.0 && c + gap + size > win {
            (c - size - gap).max(0.0)
        } else {
            c + gap
        }
    };
    (
        flip(cursor.0, panel.0, window.0),
        flip(cursor.1, panel.1, window.1),
    )
}

/// A reusable themed popup menu with nested submenus, `width` px wide. Returns the
/// panel; the caller positions it absolutely. Escape (and any action) calls `close`.
pub(crate) fn menu_panel(
    entries: Vec<MenuEntry>,
    close: Rc<dyn Fn()>,
    width: f64,
) -> impl IntoView {
    let esc = close.clone();
    focus_root(menu_stack(entries, close, width)).on_key_down(
        Key::Named(NamedKey::Escape),
        |_| true,
        move |_| (esc)(),
    )
}

/// Measure a string's rendered width (px) at `FONT_BODY`, through the same global
/// `FontSystem` the views paint with, so the measurement matches to the pixel.
/// Used to right-align the numeric grid editor and to size/ellipsize tab titles.
pub(crate) fn measure_text_px(text: &str) -> f64 {
    measure_text_px_at(text, theme::FONT_BODY)
}

/// Like [`measure_text_px`] but at an explicit font size (e.g. the 16px Find box).
pub(crate) fn measure_text_px_at(text: &str, size: f32) -> f64 {
    measure_text_px_weighted(text, size, false)
}

/// As [`measure_text_px_at`], but for text rendered `.font_bold()`. Bold glyphs are
/// wider, so measuring them at regular weight under-reports — enough that a name
/// sized to the regular measurement still ellipsizes when drawn bold.
pub(crate) fn measure_text_px_bold_at(text: &str, size: f32) -> f64 {
    measure_text_px_weighted(text, size, true)
}

/// As [`measure_text_px_at`], but in the app's monospace family — what the SQL
/// surfaces (the Ctrl+K diff, the DDL preview's script box) actually render with.
///
/// A measurement in the wrong family is the same class of error as one in the
/// wrong weight, and it is the one the diff used to make *without* measuring at
/// all: it multiplied a `chars().count()` by a comment-documented advance
/// (`8.43`). That is exact for ASCII and half the truth for a full-width glyph,
/// so a line with CJK text in a string literal reported a content width narrower
/// than it draws, and the horizontal scrollbar stopped before the end of the line.
pub(crate) fn measure_mono_px_at(text: &str, size: f32) -> f64 {
    measure_text_px_styled(text, size, false, true)
}

fn measure_text_px_weighted(text: &str, size: f32, bold: bool) -> f64 {
    measure_text_px_styled(text, size, bold, false)
}

fn measure_text_px_styled(text: &str, size: f32, bold: bool, mono: bool) -> f64 {
    use floem::text::{Attrs, AttrsList, FamilyOwned, TextLayout, Weight};
    let family = [FamilyOwned::Name(crate::consts::MONO_FAMILY.into())];
    let mut attrs = Attrs::new().font_size(size);
    if bold {
        attrs = attrs.weight(Weight::BOLD);
    }
    if mono {
        attrs = attrs.family(&family);
    }
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

/// Is the viewport parked at the bottom of the content, within `slack` px?
///
/// The predicate behind every tail-following list (the AI conversation, the Live
/// Monitor's log): while it holds, new content scrolls into view; once the user
/// scrolls up it goes false and the follow must be *released*, which — as
/// `[[floem-scroll-follow]]` records — means the `scroll_to` closure returning
/// `None`, not merely leaving its trigger un-bumped. A `scroll_to` target is
/// sticky: the last one stays applied.
///
/// It is also what decides whether to offer a jump-to-bottom button, so the two
/// can't disagree about where "the bottom" is.
pub(crate) fn at_content_bottom(content_h: f64, viewport_bottom: f64, slack: f64) -> bool {
    content_h - viewport_bottom <= slack
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
        // A scroll can fire *after* this scope was disposed — e.g. Floem defers a
        // `scroll_to` clamp and the pane rebuilt meanwhile (adding a grid row). The
        // signals are gone by then, so bail instead of unwrapping a disposed
        // `RwSignal` (`get_untracked` panics on `None`; `try_*` no-ops).
        let Some(cur) = generation.try_get_untracked() else {
            return;
        };
        shown.set(true);
        let g = cur.wrapping_add(1);
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
/// Font size for small toolbar controls (ER-diagram toolbar, header Retry).
pub(crate) const TOOLBAR_FONT: f32 = 13.0;

/// The chrome shared by small toolbar controls: bordered, rounded surface.
/// Callers add their own padding and hover — see `control_button` in the ERD
/// toolbar and the header's Retry.
pub(crate) fn control_surface(s: floem::style::Style) -> floem::style::Style {
    s.background(theme::control_bg())
        .border(1.0)
        .border_color(theme::control_border())
        .border_radius(6.0)
}

pub(crate) fn section_title(t: &'static str) -> impl IntoView {
    text(t).style(|s| {
        s.font_size(theme::FONT_TITLE)
            .font_bold()
            .color(theme::text_muted())
            .padding_horiz(12.0)
            .padding_vert(8.0)
    })
}

/// A centred status line filling its container (empty state, failure, cancel).
///
/// `color` is a **function**, not a `Color`: a colour read once at build freezes
/// at the theme that was active then, so every caller of this — eleven of them —
/// would keep painting the old palette after a live theme switch. Passing the
/// accessor and calling it *inside* the reactive `.style` closure is what makes
/// the switch free (CLAUDE.md → *Themable colors reach reactive styles as
/// `fn() -> Color`*).
pub(crate) fn centered_msg(
    msg: impl Into<String>,
    color: impl Fn() -> floem::peniko::Color + 'static,
) -> impl IntoView {
    let msg = msg.into();
    container(text(msg).style(move |s| s.color(color()))).style(|s| {
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

/// Trailing debounce: returns a signal that mirrors `src` but only settles
/// `delay` after the last change. Bind the input widget to `src` (so typing stays
/// responsive) and let the expensive consumers read the returned signal, so a
/// large re-filter/re-layout fires once per burst, not per keystroke. The write is
/// `try_update`-guarded so a timer that outlives the owning scope (panel closed
/// mid-burst) is a no-op rather than a panic on a freed signal.
pub(crate) fn debounced(src: RwSignal<String>, delay: std::time::Duration) -> RwSignal<String> {
    let out = RwSignal::new(src.get_untracked());
    let generation = std::rc::Rc::new(std::cell::Cell::new(0u64));
    floem::reactive::create_effect(move |_| {
        let v = src.get(); // track every change
        let g = generation.get() + 1;
        generation.set(g);
        let generation = generation.clone();
        floem::action::exec_after(delay, move |_| {
            if generation.get() == g {
                let _ = out.try_update(|s| *s = v.clone());
            }
        });
    });
    out
}

/// A wrapping text view that tints every case-insensitive occurrence of `term`
/// with the search-match colour (bold), the rest in `base` — the same match rule
/// (`text_ops::find_matches`) and colour as the global-search palette, but built on
/// `rich_text` so the highlight survives line-wrapping (the palette's segment
/// h-stack can't wrap). `term` empty / no match → plain text. Colours are read
/// inside the layout closure so a live theme switch re-tints. Use `.style()` on the
/// returned view for layout (width, clip, max-height).
pub(crate) fn highlight_text(
    full: String,
    term: Option<String>,
    font_size: f32,
    base: impl Fn() -> floem::peniko::Color + 'static,
    bold: bool,
    line_height: f32,
) -> floem::views::RichText {
    use floem::text::{Attrs, AttrsList, FamilyOwned, LineHeightValue, TextLayout, Weight};
    let base_weight = if bold { Weight::BOLD } else { Weight::NORMAL };
    floem::views::rich_text(move || {
        let sans = [FamilyOwned::Name("IBM Plex Sans".to_string())];
        let lh = LineHeightValue::Normal(line_height);
        let base_attrs = Attrs::new()
            .family(&sans)
            .font_size(font_size)
            .color(base())
            .weight(base_weight)
            .line_height(lh);
        let mut list = AttrsList::new(base_attrs);
        if let Some(t) = term.as_deref().filter(|t| !t.is_empty()) {
            let hit = Attrs::new()
                .family(&sans)
                .font_size(font_size)
                .color(theme::match_highlight())
                .weight(Weight::BOLD)
                .line_height(lh);
            for &start in schemaic_core::text_ops::find_matches(&full, t).iter() {
                list.add_span(start..start + t.len(), hit);
            }
        }
        let mut layout = TextLayout::new();
        layout.set_text(&full, list);
        layout
    })
}

/// An animated loading label — `prefix` followed by a cycling `.` → `..` → `...`
/// on a 400ms timer (instead of a static `…`). The timer self-reschedules and
/// stops when the view's scope is disposed (`try_update` → `None`), so it can't
/// outlive a `dyn_container` rebuild (same pattern as the AI elapsed timer).
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
    let w = measure_text_px_at(&format!("{prefix}..."), font_size) + 2.0;
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

/// What a modal's Escape / ✕ should do, given whether work it started is still
/// running.
///
/// This exists because two modals got it wrong the same way. The import modal
/// and the DDL preview each have **three** exits — a footer button, Escape, and
/// the title bar's ✕ — and in both, only the footer button knew that closing
/// mid-flight is not allowed. The other two called a bare `close`, so pressing
/// Escape during a bulk import hid a transaction that then ran to completion and
/// committed, reporting its outcome into signals whose only reader had just been
/// unmounted; and pressing Escape during a MySQL `ALTER` threw away the
/// "statement 3 of 5 failed, 2 already stuck" report, leaving a half-migrated
/// table and a stale schema tree with no indication anything had happened.
///
/// Both modals' footers carried a comment explaining why the guard was there.
/// Neither comment could reach the other two exits, which is the argument for a
/// named function over a repeated `if busy`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExitAction {
    /// Nothing in flight — close.
    Close,
    /// Work is running and *can* be stopped: stop it and stay open, so the
    /// outcome still has a reader.
    Cancel,
    /// Work is running and cannot be stopped: refuse. Closing would orphan the
    /// outcome, and there is nothing to cancel.
    Ignore,
}

/// The exit decision. See [`ExitAction`].
///
/// `cancellable` is what separates the two callers: the import modal holds a
/// cancellation token the load actually observes, while `run_ddl` is handed a
/// fresh token nothing holds — and on MySQL each DDL statement has already
/// committed anyway, so there is no meaningful "stop".
pub(crate) fn exit_action(busy: bool, cancellable: bool) -> ExitAction {
    match (busy, cancellable) {
        (false, _) => ExitAction::Close,
        (true, true) => ExitAction::Cancel,
        (true, false) => ExitAction::Ignore,
    }
}

#[cfg(test)]
mod measure_tests {
    use super::*;

    #[test]
    fn bold_measures_wider_than_regular() {
        // Measure against the *bundled* faces, as the app does. Without this the
        // global `FontSystem` falls back to whatever the host has installed, and
        // the assertion becomes a claim about that machine's fonts: it held on
        // Windows and failed on a bare Linux runner, where the fallback's bold
        // measured *narrower* than its regular.
        crate::fonts::load_fonts();
        // The whole reason `measure_text_px_bold_at` exists: the ER-diagram card
        // header is drawn `.font_bold()`, and sizing it from the regular
        // measurement made every card narrower than its own title. For a
        // schema-qualified name the gap exceeds `node_width`'s 6px slack, so the
        // name ellipsized nowhere near the max card width.
        for name in ["analytics.daily_revenue", "sales.line_items", "orders"] {
            let reg = measure_text_px_at(name, 13.0);
            let bold = measure_text_px_bold_at(name, 13.0);
            assert!(
                bold > reg,
                "{name}: bold {bold} should exceed regular {reg}"
            );
        }
        // A long qualified name drifts by more than the slack — the actual bug.
        let n = "analytics.daily_revenue";
        assert!(
            measure_text_px_bold_at(n, 13.0) - measure_text_px_at(n, 13.0) > 6.0,
            "the regression this guards needs the drift to exceed node_width's slack"
        );
    }

    #[test]
    fn a_char_count_does_not_predict_the_width() {
        // The Ctrl+K diff used to size its scroll extent as `chars().count()`
        // times a fixed advance. `char` is not the unit text is laid out in: a
        // combining mark is a `char` of its own and adds no advance at all, so
        // the width came out over the truth for any accented line. Ctrl+K sends
        // the editor buffer, so a literal in the user's own SQL reaches it.
        //
        // The mirror case — a full-width CJK glyph taking *two* advances for one
        // `char` — is what this test used to assert, and it can't be pinned here:
        // `MONO_FAMILY` carries no CJK coverage, so those glyphs resolve through
        // whatever the machine happens to have installed. It failed on the bare
        // Linux CI runner, which has nothing, while passing locally on a fallback
        // box that measures identically to an unrenderable private-use codepoint
        // — so it was asserting on the fallback, not on a glyph. The combining
        // mark is in the bundled font and therefore measures the same everywhere.
        crate::fonts::load_fonts();
        let advance = measure_mono_px_at("0", 14.0);
        let ascii = "abcdefgh"; // 8 chars, 8 advances
        let accented = "e\u{0301}"; // 2 chars, 1 advance

        // The baseline the old arithmetic assumed, and where it held.
        assert!((measure_mono_px_at(ascii, 14.0) - 8.0 * advance).abs() < 1.0);
        // …and where it didn't: two chars occupying a single advance.
        assert!(
            measure_mono_px_at(accented, 14.0) < 1.5 * advance,
            "a combining mark must not measure as an advance of its own"
        );
    }

    #[test]
    fn the_mono_measurement_is_not_the_proportional_one() {
        // Measuring monospace text with the default family is the same class of
        // error as measuring bold text at regular weight — silently narrow.
        crate::fonts::load_fonts();
        let s = "iiiiiiiiii"; // the letter proportional fonts make narrowest
        assert!(
            measure_mono_px_at(s, 14.0) > measure_text_px_at(s, 14.0),
            "monospace must not be measured with the proportional family"
        );
    }
}

#[cfg(test)]
mod exit_tests {
    use super::*;

    /// The property both [B7.2-L1-01] and [B2-L1-01] are about: while work the
    /// modal started is still running, **no exit closes it**. Two modals had
    /// three exits each, and in both only the footer button knew this.
    #[test]
    fn no_exit_closes_a_modal_while_its_work_is_running() {
        for cancellable in [true, false] {
            assert_ne!(
                exit_action(true, cancellable),
                ExitAction::Close,
                "cancellable={cancellable}"
            );
        }
    }

    #[test]
    fn idle_always_closes() {
        assert_eq!(exit_action(false, true), ExitAction::Close);
        assert_eq!(exit_action(false, false), ExitAction::Close);
    }

    /// The import modal: the load holds a cancellation token, so Escape means
    /// "stop the write and roll it back", not "hide it".
    #[test]
    fn busy_and_cancellable_cancels_rather_than_closing() {
        assert_eq!(exit_action(true, true), ExitAction::Cancel);
    }

    /// The DDL preview: `run_ddl` is handed a token nothing holds, and MySQL
    /// commits each statement implicitly, so there is nothing to cancel — the
    /// only honest answer is to refuse the exit and keep the modal that owns the
    /// outcome on screen.
    #[test]
    fn busy_and_uncancellable_refuses_the_exit() {
        assert_eq!(exit_action(true, false), ExitAction::Ignore);
    }
}

#[cfg(test)]
mod menu_placement_tests {
    use super::*;

    const PANEL: (f64, f64) = (170.0, 350.0);
    const WINDOW: (f64, f64) = (1200.0, 800.0);

    #[test]
    fn a_menu_with_room_opens_down_and_right_of_the_cursor() {
        assert_eq!(
            cursor_menu_pos((100.0, 100.0), PANEL, WINDOW, 3.0),
            (103.0, 103.0)
        );
    }

    /// The schema tree is a full-height left column, so its lower half is where
    /// most right-clicks land — and a table's menu is a dozen entries.
    #[test]
    fn a_menu_near_the_bottom_flips_above_the_cursor() {
        let (x, y) = cursor_menu_pos((100.0, 700.0), PANEL, WINDOW, 3.0);
        assert_eq!(x, 103.0, "horizontal is unaffected");
        assert_eq!(y, 347.0);
        assert!(y + PANEL.1 <= 700.0, "the panel ends above the cursor");
    }

    #[test]
    fn a_menu_near_the_right_edge_flips_left_of_the_cursor() {
        let (x, _) = cursor_menu_pos((1150.0, 100.0), PANEL, WINDOW, 3.0);
        assert_eq!(x, 977.0);
        assert!(x + PANEL.0 <= 1150.0);
    }

    #[test]
    fn a_menu_in_the_far_corner_flips_both_ways() {
        let (x, y) = cursor_menu_pos((1150.0, 700.0), PANEL, WINDOW, 3.0);
        assert_eq!((x, y), (977.0, 347.0));
    }

    /// A panel taller (or wider) than the space on either side clamps to the
    /// window edge rather than going negative, where it would be unreachable.
    #[test]
    fn a_panel_bigger_than_the_window_clamps_to_the_origin() {
        let (x, y) = cursor_menu_pos((50.0, 60.0), (400.0, 900.0), WINDOW, 3.0);
        assert_eq!((x, y), (53.0, 0.0));
    }

    /// Before the root has measured itself the window is (0, 0). Flipping against
    /// an unknown edge would put every menu in the top-left corner.
    #[test]
    fn an_unmeasured_window_never_flips() {
        assert_eq!(
            cursor_menu_pos((900.0, 700.0), PANEL, (0.0, 0.0), 3.0),
            (903.0, 703.0)
        );
    }

    #[test]
    fn separators_are_not_counted_as_rows() {
        let sep = menu_panel_height(&[MenuEntry::Separator]);
        let row = menu_panel_height(&[MenuEntry::action("x", || {})]);
        assert!(
            sep < row,
            "a separator is a rule, not a row: {sep} vs {row}"
        );
        // Chrome is counted once, not per entry.
        let two =
            menu_panel_height(&[MenuEntry::action("a", || {}), MenuEntry::action("b", || {})]);
        assert!((two - (row * 2.0 - 14.0)).abs() < 0.001);
    }

    /// A submenu row is one row in the panel that hosts it — its children are
    /// drawn in their own panel and must not be added to this one's height.
    #[test]
    fn a_submenu_counts_as_a_single_row() {
        let sub = MenuEntry::Sub {
            label: "Copy".into(),
            icon: None,
            children: vec![
                MenuEntry::action("CSV", || {}),
                MenuEntry::action("JSON", || {}),
            ],
        };
        assert_eq!(
            menu_panel_height(std::slice::from_ref(&sub)),
            menu_panel_height(&[MenuEntry::action("Copy", || {})])
        );
    }
}

#[cfg(test)]
mod follow_tests {
    use super::*;

    /// A streaming answer grows the content under a parked viewport: the moment
    /// the gap opens past the slack, following must stop. The AI panel re-pinned
    /// on every *token*, so scrolling up mid-answer was impossible.
    #[test]
    fn following_stops_once_the_viewport_is_off_the_bottom() {
        assert!(
            at_content_bottom(1000.0, 1000.0, 30.0),
            "exactly at the end"
        );
        assert!(at_content_bottom(1000.0, 980.0, 30.0), "within the slack");
        assert!(
            !at_content_bottom(1000.0, 900.0, 30.0),
            "scrolled up to read"
        );
    }

    /// The boundary is inclusive, so a viewport sitting exactly `slack` px off the
    /// bottom keeps following rather than flapping between the two states.
    #[test]
    fn the_slack_boundary_still_counts_as_the_bottom() {
        assert!(at_content_bottom(1000.0, 970.0, 30.0));
        assert!(!at_content_bottom(1000.0, 969.9, 30.0));
    }

    /// Content shorter than the viewport (a fresh conversation) is trivially at
    /// the bottom — the follow must not be released before anything arrives.
    #[test]
    fn content_shorter_than_the_viewport_is_at_the_bottom() {
        assert!(at_content_bottom(100.0, 400.0, 30.0));
        assert!(at_content_bottom(0.0, 0.0, 30.0));
    }
}
