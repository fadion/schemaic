//! The query-tab strip above the editor: `tab_bar` renders the row of flat,
//! full-height tabs (+ the "＋" new-tab button) from `ui.tabs_ui.tabs`, and
//! `tab_chip` is one tab (click to activate, ×/middle-click to close). A flashing
//! tab is hidden for the duration of its flash. `tab_bar` is wired into `center`.

use std::rc::Rc;

use floem::AnyView;
use floem::event::{Event, EventListener, EventPropagation};
use floem::prelude::*;
use floem::reactive::create_effect;
use floem::style::TextOverflow;
use floem::views::TooltipExt;

use schemaic_core::db_color::DbColorRule;

use crate::consts::{TAB_BAR_H, TAB_MAX_W};
use crate::widgets::{measure_text_px, wheel_hscroll};
use crate::{FieldCfg, Tab, Ui, bg_transparent, db_color_dot, edit_field, icons, theme};

// ===== moved from lib.rs (tab bar) =====
// ── Tab bar ─────────────────────────────────────────────────────────────────
pub(crate) fn tab_bar(ui: Ui) -> impl IntoView {
    let tabs = ui.tabs_ui.tabs;
    let active = ui.tabs_ui.active;
    let flashing = ui.tabs_ui.flashing;
    let add_tab = ui.tab_actions.add_tab.clone();
    let close_tab = ui.tab_actions.close_tab.clone();
    let db_colors = ui.db_colors;
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
        move |t| tab_chip(t, active, close_tab.clone(), db_colors),
    )
    .style(|s| s.flex_row().height_full());

    // The chips pan horizontally on the plain wheel (no visible bars), so tabs
    // that overflow the strip stay reachable. The region shrinks to fit the space
    // left of the "+" (flex_shrink + min_width(0)); when the tabs fit, it's
    // content-sized and the "+" sits right after the last tab.
    let scroller =
        wheel_hscroll(chips).style(|s| s.flex_shrink(1.0_f32).min_width(0.0).height_full());

    // The "+" is a flat, full-height button matching the tabs: chrome background,
    // the plus glyph with 10px breathing room each side, brightening on hover. It
    // never scrolls away (flex_shrink(0)).
    let add = container(icons::icon(icons::PLUS, 16.0))
        .on_click_stop(move |_| (add_tab)())
        .style(|s| {
            s.flex_row()
                .items_center()
                .flex_shrink(0.0_f32)
                .padding_horiz(10.0)
                .background(theme::bg_chrome())
                .color(theme::tab_text())
                .hover(|s| s.color(theme::text()))
        });

    h_stack((scroller, add)).style(|s| {
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

// Width available to the title *text* inside a full-width (200px) tab: the tab
// max minus the left margin (10), label→× gap (7), the × box (16) and its right
// margin (7). A title wider than this ellipsizes and gains a tooltip.
const TAB_TITLE_AVAIL: f64 = TAB_MAX_W - 40.0;

fn tab_chip(
    tab: Tab,
    active: RwSignal<usize>,
    close_tab: Rc<dyn Fn(usize)>,
    db_colors: RwSignal<Vec<DbColorRule>>,
) -> impl IntoView {
    // Commit the inline rename: an empty/blank name reverts to the default
    // "Query N" (stored as `None`). Called from Enter and from focus-loss.
    let commit: Rc<dyn Fn()> = Rc::new(move || {
        let new = tab.edit_buf.get_untracked().trim().to_string();
        tab.name.set(if new.is_empty() { None } else { Some(new) });
        tab.editing.set(false);
    });

    // Content swaps between the display label and the inline rename field. Keyed
    // on `(editing, title)` so it rebuilds when either the mode or the (possibly
    // renamed) title changes; the title read tracks the `name` signal.
    let commit_field = commit.clone();
    let close_content = close_tab.clone();
    let content = dyn_container(
        move || (tab.editing.get(), tab.title()),
        move |(editing, title)| -> AnyView {
            if editing {
                // Inline rename field. `edit_field` (unlike floem's `text_input`,
                // which swallows Escape into its own `clear_focus`) routes Escape
                // to `on_escape`, so we can *discard* on Escape and *commit* on
                // Enter / click-away. The blur commit is guarded on `editing` so
                // Enter/Escape (which set `editing = false` first) don't re-fire it.
                let commit_enter = commit_field.clone();
                let commit_blur = commit_field.clone();
                let cfg = FieldCfg {
                    background: bg_transparent,
                    border_color: Some(bg_transparent),
                    border_radius: 0.0,
                    font_size: theme::FONT_BODY,
                    autofocus: true,
                    height: Some(TAB_BAR_H),
                    on_submit: Some(commit_enter),
                    on_escape: Some(Rc::new(move || tab.editing.set(false))),
                    on_blur: Some(Rc::new(move || {
                        if tab.editing.get_untracked() {
                            (commit_blur)();
                        }
                    })),
                    ..FieldCfg::default()
                };
                // Width auto-grows with the typed text from a small base up to the
                // tab max (the chip's `max_width` is the hard cap).
                return edit_field(tab.edit_buf, cfg)
                    .style(move |s| {
                        let w = (tab.edit_buf.with(|b| measure_text_px(b)) + 24.0)
                            .clamp(60.0, TAB_MAX_W - 2.0);
                        s.width(w)
                    })
                    .into_any();
            }

            // Display: label (ellipsized past the tab width) + close ×. A title
            // that would be clipped gets a tooltip with its full text; a title
            // that fits gets none.
            let truncated = measure_text_px(&title) > TAB_TITLE_AVAIL;
            let full = title.clone();
            // Left inset moved to the row's `padding_left` so the (optional) DB
            // colour dot can lead the label without shifting the text when absent.
            let label = text(title).style(|s| {
                s.margin_right(7.0)
                    .max_width(TAB_TITLE_AVAIL)
                    .text_overflow(TextOverflow::Ellipsis)
                    .font_size(theme::FONT_BODY)
            });
            let label: AnyView = if truncated {
                label
                    .tooltip(move || {
                        text(full.clone()).style(|s| {
                            s.padding_horiz(8.0)
                                .padding_vert(5.0)
                                .background(theme::bg_panel())
                                .border(1.0)
                                .border_color(theme::border())
                                .border_radius(4.0)
                                .color(theme::text())
                                .font_size(theme::FONT_BODY)
                        })
                    })
                    .into_any()
            } else {
                label.into_any()
            };

            // Close (×): 16px Lucide X, fixed muted tint (`tab_close`). The glyph
            // fills only the middle of its 16px box (~3px transparent padding per
            // side), so the margins are trimmed ~3px to land 10px *visual* gaps.
            let close_x = close_content.clone();
            let close = icons::icon(icons::X, 16.0)
                .on_click_stop(move |_| (close_x)(tab.id))
                .style(|s| {
                    s.flex_shrink(0.0_f32)
                        .margin_right(7.0)
                        .color(theme::tab_close())
                });
            // Small DB-identity dot leading the label (only when this tab's
            // database has a colour; zero-footprint otherwise).
            let dot = db_color_dot(
                db_colors,
                move || tab.database.get().map(|db| (tab.conn_id.get(), db)),
                0.0,
                6.0,
                -1.0,
            );
            h_stack((dot, label, close))
                .style(|s| s.flex_row().items_center().padding_left(10.0))
                .into_any()
        },
    );

    let chip = content
        .on_click_stop(move |_| active.set(tab.id))
        // Double-click a tab to rename it in place: seed the buffer with the
        // current title and switch to the field. Guarded so double-clicking
        // *inside* the field (word-select) doesn't reset the buffer mid-edit.
        .on_event_stop(EventListener::DoubleClick, move |_| {
            if !tab.editing.get_untracked() {
                tab.edit_buf.set(tab.title());
                tab.editing.set(true);
            }
        })
        // Middle-click (mouse-wheel button) closes the tab, as in most editors.
        // `Click`/`DoubleClick` only fire for the primary button, so this can't
        // clash with activating the tab or double-click-to-rename.
        .on_event(EventListener::PointerDown, move |e| {
            if let Event::PointerDown(pe) = e
                && pe.button.is_auxiliary()
            {
                (close_tab)(tab.id);
                return EventPropagation::Stop;
            }
            EventPropagation::Continue
        })
        // Flat, full-height tab capped at `TAB_MAX_W`: chrome background (invisible
        // against the strip) when inactive, `tab_active` when active; a right
        // separator line divides it from the next tab. The container's `color`
        // cascades to the label + ×.
        .style(move |s| {
            let s = s
                .flex_row()
                .items_center()
                .max_width(TAB_MAX_W)
                .border_right(1.0)
                .border_color(theme::tab_separator());
            if active.get() == tab.id {
                s.background(theme::tab_active()).color(theme::text())
            } else {
                s.background(theme::bg_chrome())
                    .color(theme::tab_text())
                    .hover(|s| s.color(theme::text()))
            }
        });

    // Reveal the active tab in the (bar-less) horizontal strip: Ctrl+number can
    // activate a tab scrolled off the right edge, and a newly created tab is
    // appended past it. Deferred one tick (`exec_after(0)`) so a freshly-mounted
    // chip is laid out before we scroll to it (see the schema tree's nav scroll).
    let cid = chip.id();
    create_effect(move |_| {
        if active.get() == tab.id {
            floem::action::exec_after(std::time::Duration::ZERO, move |_| cid.scroll_to(None));
        }
    });
    chip
}
