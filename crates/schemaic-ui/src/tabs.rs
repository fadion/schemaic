//! The query-tab strip above the editor: `tab_bar` renders the row of flat,
//! full-height tabs (+ the "＋" new-tab button) from `ui.tabs_ui.tabs`, and
//! `tab_chip` is one tab (click to activate, ×/middle-click to close). A flashing
//! tab is hidden for the duration of its flash. `tab_bar` is wired into `center`.

use std::rc::Rc;

use floem::event::{Event, EventListener, EventPropagation};
use floem::prelude::*;

use crate::consts::TAB_BAR_H;
use crate::{Tab, Ui, icons, theme};

// ===== moved from lib.rs (tab bar) =====
// ── Tab bar ─────────────────────────────────────────────────────────────────
pub(crate) fn tab_bar(ui: Ui) -> impl IntoView {
    let tabs = ui.tabs_ui.tabs;
    let active = ui.tabs_ui.active;
    let flashing = ui.tabs_ui.flashing;
    let add_tab = ui.tab_actions.add_tab.clone();
    let close_tab = ui.tab_actions.close_tab.clone();
    // A flashing tab's chip is hidden for the duration of the flash. Flat,
    // full-height tabs sit flush (no gap); each draws its own right separator.
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
    .style(|s| s.flex_row());

    // The "+" is a flat, full-height button matching the tabs: chrome background,
    // the plus glyph with 10px breathing room each side, brightening on hover.
    let add = container(icons::icon(icons::PLUS, 16.0))
        .on_click_stop(move |_| (add_tab)())
        .style(|s| {
            s.flex_row()
                .items_center()
                .padding_horiz(10.0)
                .background(theme::bg_chrome())
                .color(theme::tab_text())
                .hover(|s| s.color(theme::text()))
        });

    h_stack((chips, add)).style(|s| {
        s.width_full()
            .flex_row()
            .height(TAB_BAR_H)
            .min_height(TAB_BAR_H)
            .flex_shrink(0.0_f32)
            .background(theme::bg_chrome())
            .border_bottom(1.0)
            .border_color(theme::border())
    })
}

fn tab_chip(tab: Tab, active: RwSignal<usize>, close_tab: Rc<dyn Fn(usize)>) -> impl IntoView {
    let close_x = close_tab.clone();
    // Close (×): 16px Lucide X, a fixed muted tint (`tab_close`), independent of
    // the label's dim→bright behaviour. The X glyph only fills the middle of its
    // 16px box (~3px transparent padding per side), so the margins are trimmed by
    // ~3px to land the *visual* gaps at 10px (icon→edge, text→icon).
    let close = icons::icon(icons::X, 16.0)
        .on_click_stop(move |_| (close_x)(tab.id))
        .style(|s| {
            s.flex_shrink(0.0_f32)
                .margin_right(7.0)
                .color(theme::tab_close())
        });

    // Text: 10px from the left edge; ~7px + the icon's ~3px inset ≈ 10px to the ×.
    // Colour inherited from the tab container.
    let label = text(format!("Query {}", tab.label))
        .style(|s| s.margin_left(10.0).margin_right(7.0).font_size(theme::FONT_BODY));

    h_stack((label, close))
        .on_click_stop(move |_| active.set(tab.id))
        // Middle-click (mouse-wheel button) closes the tab, as in most editors.
        // `Click`/`DoubleClick` only fire for the primary button, so this can't
        // clash with activating the tab or double-click-to-open.
        .on_event(EventListener::PointerDown, move |e| {
            if let Event::PointerDown(pe) = e
                && pe.button.is_auxiliary()
            {
                (close_tab)(tab.id);
                return EventPropagation::Stop;
            }
            EventPropagation::Continue
        })
        // Flat, full-height tab: chrome background (invisible against the strip)
        // when inactive, `tab_active` when active; a right separator line divides
        // it from the next tab. The container's `color` cascades to the label + ×.
        .style(move |s| {
            let s = s
                .flex_row()
                .items_center()
                .border_right(1.0)
                .border_color(theme::tab_separator());
            if active.get() == tab.id {
                s.background(theme::tab_active()).color(theme::text())
            } else {
                s.background(theme::bg_chrome())
                    .color(theme::tab_text())
                    .hover(|s| s.color(theme::text()))
            }
        })
}
