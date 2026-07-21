//! The query-tab strip above the editor: `tab_bar` renders the row of tab chips
//! (+ the "＋" new-tab button) from `ui.tabs_ui.tabs`, and `tab_chip` is one
//! top-rounded tab (click to activate, ×/middle-click to close). A flashing tab
//! is hidden for the duration of its flash. `tab_bar` is wired into `center`.

use std::rc::Rc;

use floem::event::{Event, EventListener, EventPropagation};
use floem::prelude::*;

use crate::consts::{TAB_BAR_H, TAB_RADIUS, TAB_TOP_GAP};
use crate::{Tab, Ui, icons, theme};

// ===== moved from lib.rs (tab bar) =====
// ── Tab bar ─────────────────────────────────────────────────────────────────
pub(crate) fn tab_bar(ui: Ui) -> impl IntoView {
    let tabs = ui.tabs_ui.tabs;
    let active = ui.tabs_ui.active;
    let flashing = ui.tabs_ui.flashing;
    let add_tab = ui.tab_actions.add_tab.clone();
    let close_tab = ui.tab_actions.close_tab.clone();
    // A flashing tab's chip is hidden for the duration of the flash.
    let chips = dyn_stack(
        move || {
            tabs.get()
                .into_iter()
                .filter(|t| flashing.get() != Some(t.id))
                .collect::<Vec<_>>()
        },
        |t: &Tab| t.id,
        move |t| tab_chip(t, active, close_tab.clone()),
    )
    // `items_stretch` (default) so tabs fill the bar's full height — flush at the
    // bottom, no floating capsule. 5px between tabs.
    .style(|s| s.flex_row().gap(5.0));

    // The "+" is a tab-shaped button (bg like an inactive tab); the plus glyph
    // gets 14px of breathing room on each side. Same oversize-and-clip trick as
    // the tabs so its top corners round too.
    let add = container(icons::icon(icons::PLUS, 16.0).style(|s| s.color(theme::text_muted())))
        .on_click_stop(move |_| (add_tab)())
        .style(|s| {
            s.flex_row()
                .items_center()
                .padding_horiz(14.0)
                .margin_top(TAB_TOP_GAP)
                .height(TAB_BAR_H - TAB_TOP_GAP + TAB_RADIUS)
                .padding_bottom(TAB_RADIUS)
                .border_radius(TAB_RADIUS)
                .background(theme::bg_editor())
                // Hover cue that it's clickable (TODO affordance pass).
                .hover(|s| s.background(theme::tab_hover()).color(theme::text()))
        });

    // First tab 10px from the left; 5px between the last tab and the "+".
    // `items_start` keeps the over-tall tabs anchored to the top; `.clip()` cuts
    // off their bottom `TAB_RADIUS` px (Floem has only a uniform border-radius, so
    // this hides the rounded BOTTOM corners → tabs read as top-rounded and flush
    // at the bottom, not fully-rounded capsules).
    h_stack((chips, add))
        .style(|s| {
            s.width_full()
                .flex_row()
                .items_start()
                .gap(5.0)
                .padding_left(10.0)
                .height(TAB_BAR_H)
                .min_height(TAB_BAR_H)
                .flex_shrink(0.0_f32)
                .background(theme::bg_chrome())
                .border_bottom(1.0)
                .border_color(theme::border())
        })
        .clip()
}

fn tab_chip(tab: Tab, active: RwSignal<usize>, close_tab: Rc<dyn Fn(usize)>) -> impl IntoView {
    let close_x = close_tab.clone();
    // Close (×): 16px Lucide X, 8px from the tab's right edge; its tint depends
    // on whether the tab is active.
    let close = icons::icon(icons::X, 16.0)
        .on_click_stop(move |_| (close_x)(tab.id))
        .style(move |s| {
            let s = s.flex_shrink(0.0_f32).margin_right(8.0);
            if active.get() == tab.id {
                s.color(theme::tab_close_active())
            } else {
                s.color(theme::search_hint())
            }
        });

    // Text: 8px from the left edge, 12px from the × icon. Active is brighter
    // (#757A8B) than inactive (#585C6A) for legibility.
    let tab_id = tab.id;
    let label = text(format!("Query {}", tab.label)).style(move |s| {
        let s = s.margin_left(8.0).margin_right(12.0);
        if active.get() == tab_id {
            s.color(theme::tab_text_active())
        } else {
            s.color(theme::text_muted())
        }
    });

    h_stack((label, close))
        .on_click_stop(move |_| active.set(tab.id))
        // Middle-click (mouse-wheel button) closes the tab, as in most editors.
        // `Click`/`DoubleClick` only fire for the primary button, so this can't
        // clash with activating the tab or double-click-to-open. Closing the
        // last tab is handled specially by `close_tab` (clear + brief flash).
        .on_event(EventListener::PointerDown, move |e| {
            if let Event::PointerDown(pe) = e
                && pe.button.is_auxiliary() {
                    (close_tab)(tab.id);
                    return EventPropagation::Stop;
                }
            EventPropagation::Continue
        })
        // Top-rounded tab: drawn `TAB_RADIUS` px taller than the bar with a
        // uniform radius, then the bar's `.clip()` shaves off the bottom (rounded)
        // corners — so the top is rounded and the bottom is flush and square.
        // `padding_bottom(TAB_RADIUS)` re-centers the label in the visible area.
        .style(move |s| {
            let s = s
                .flex_row()
                .items_center()
                .margin_top(TAB_TOP_GAP)
                .height(TAB_BAR_H - TAB_TOP_GAP + TAB_RADIUS)
                .padding_bottom(TAB_RADIUS)
                .border_radius(TAB_RADIUS)
                .font_size(theme::FONT_BODY);
            if active.get() == tab.id {
                s.background(theme::tab_active())
            } else {
                s.background(theme::bg_editor())
                    .hover(|s| s.background(theme::tab_hover()))
            }
        })
}
