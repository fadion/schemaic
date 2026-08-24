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

use crate::consts::{tab_bar_h, tab_max_w};
use crate::widgets::{MenuEntry, measure_text_px, wheel_hscroll};
use crate::{FieldCfg, Tab, Ui, bg_transparent, db_color_dot, edit_field, icons, theme};

// ===== moved from lib.rs (tab bar) =====
// ── Tab bar ─────────────────────────────────────────────────────────────────
pub(crate) fn tab_bar(ui: Ui) -> impl IntoView {
    let tabs = ui.tabs_ui.tabs;
    let flashing = ui.tabs_ui.flashing;
    let active_conn = ui.conn.active_conn;
    let conn_status = ui.conn.conn_status;
    let add_tab = ui.tab_actions.add_tab.clone();
    // Each chip gets its own `Ui` handle — for the close/pin/duplicate actions and
    // the shared popup-menu channel that the right-click context menu opens on.
    let chip_ui = ui;
    // A flashing tab's chip is hidden for the duration of the flash. Flat,
    // full-height tabs sit flush (no gap); each draws its own right separator.
    // Only the active connection's tabs: a tab belongs to a connection, so
    // showing five MariaDB tabs beside three Postgres ones just loses the user.
    let chips = dyn_stack(
        move || {
            let conn = active_conn.get();
            tabs.get()
                .into_iter()
                .filter(|t| t.conn_id.get() == conn && flashing.get() != Some(t.id))
                .collect::<Vec<_>>()
        },
        // Key on (id, label): the label is a plain field read at build time (not a
        // signal), so including it makes a renumber (e.g. reopen-closed-tab
        // restoring the original "Query N") rebuild the chip with the new number.
        |t: &Tab| (t.id, t.label),
        move |t| tab_chip(t, chip_ui.clone()),
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
    // Dimmed while the connection is known-dead, but still clickable: the click
    // re-checks and opens the tab if the server is back (see the app's
    // connection gate). The header's "Not connected · Retry" is the other way in.
    let add = container(icons::icon(icons::PLUS, 16.0).style(move |s| {
        if conn_status.get().is_down() {
            s.color(theme::text_muted().multiply_alpha(0.4))
        } else {
            s
        }
    }))
    .on_click_stop(move |_| (add_tab)())
    .style(|s| {
        s.flex_row()
            .items_center()
            .flex_shrink(0.0_f32)
            .padding_horiz(theme::scaled(10.0))
            .background(theme::bg_chrome())
            .color(theme::tab_text())
            .hover(|s| s.color(theme::text()))
    });

    h_stack((scroller, add)).style(|s| {
        s.width_full()
            .flex_row()
            .height(tab_bar_h())
            .min_height(tab_bar_h())
            .flex_shrink(0.0_f32)
            .background(theme::bg_chrome())
            .border_bottom(1.0)
            .border_color(theme::border())
    })
}

// Width available to the title *text* inside a full-width (200px) tab: the tab
// max minus the left margin (10), label→× gap (7), the × box (16) and its right
// margin (7). A title wider than this ellipsizes and gains a tooltip.
fn tab_title_avail() -> f64 {
    tab_max_w() - theme::scaled(40.0)
}

// A present DB-identity dot leads the label with its own 12px footprint (the 6px
// glyph + its 6px right margin — see `db_color_dot` below). It's *not* covered by
// `tab_title_avail()`'s 40, so the label cap must shed it when a dot shows; else a
// full-width truncated title pushes the × past the chip cap and clips it.
fn tab_dot_w() -> f64 {
    theme::scaled(12.0)
}

// And the same for the file glyph a `.sql`-backed tab leads its title with:
// 14px plus a 5px right margin, neither of them in `tab_title_avail()`'s 40 either.
// A tab can show both, and then the title sheds both.
fn tab_file_w() -> f64 {
    theme::scaled(19.0)
}

fn tab_chip(tab: Tab, ui: Ui) -> impl IntoView {
    let active = ui.tabs_ui.active;
    let close_tab = ui.tab_actions.close_tab.clone();
    let db_colors = ui.db_colors;
    let toggle_pin = ui.tab_actions.toggle_pin.clone();
    let duplicate = ui.tab_actions.duplicate_tab.clone();
    let reopen = ui.tab_actions.reopen_closed_tab.clone();
    let can_reopen = ui.tab_actions.can_reopen_closed_tab.clone();
    let close_all = ui.tab_actions.close_all_tabs.clone();
    let close_others = ui.tab_actions.close_other_tabs.clone();
    let can_close_others = ui.tab_actions.can_close_other_tabs.clone();
    let open_file = ui.tab_actions.open_sql_file.clone();
    let save_file = ui.tab_actions.save_sql_file.clone();
    let save_file_as = ui.tab_actions.save_sql_file_as.clone();
    let reload_file = ui.tab_actions.reload_sql_file.clone();
    let overlay = ui.overlay;

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
    let close_mid = close_tab.clone();
    let content = dyn_container(
        // Keyed on pinned too, so toggling pin swaps the × for the pin indicator,
        // and on `modified` so the italic title follows it. The modified read
        // tracks `query`, so this closure re-runs on every keystroke — but the key
        // only *changes* when the flag flips, which is the one thing that has to
        // rebuild the row.
        //
        // The **path** is in the key as its display string, not as an
        // `is_some()`: it drives the file icon *and* the tooltip, and a Save As
        // from one file to another on a tab that also carries a user-assigned name
        // moves neither the title nor the icon — the tooltip would have gone on
        // naming the old file.
        // The interface scale is in the key because `truncated` below is a
        // *structural* decision, not a style one: it chooses whether to attach a
        // tooltip at all, and a tooltip can't be added or removed from inside a
        // `.style(…)` closure. Both sides of that comparison scale (the measured
        // title, and `tab_title_avail()`), so without a rebuild a tab that starts
        // ellipsizing at a larger scale ellipsizes with no tooltip — losing the
        // one cue that says what the clipped title is.
        move || {
            (
                tab.editing.get(),
                tab.title(),
                tab.pinned.get(),
                tab.modified(),
                tab.path
                    .with(|p| p.as_ref().map(|p| p.to_string_lossy().into_owned())),
                theme::ui_scale(),
            )
        },
        move |(editing, title, pinned, modified, path, _scale)| -> AnyView {
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
                    font_size: theme::font_body,
                    autofocus: true,
                    height: Some(tab_bar_h),
                    on_submit: Some(commit_enter),
                    on_escape: Some(Rc::new(move || tab.editing.set(false))),
                    on_blur: Some(Rc::new(move || {
                        // `try_get_untracked`, not `get_untracked`: the blur is
                        // armed as a `Duration::ZERO` timer, nothing cancels it,
                        // and `get_untracked` is `try_get_untracked().unwrap()`.
                        // If the tab's scope is ever disposed between the arm
                        // and the fire, a late callback must degrade to a no-op
                        // rather than panic the app. Hardening: no input was
                        // found that reaches it — floem runs `handle_timer` at
                        // the top of every event-loop callback, so the timer
                        // fires before the click that would dispose anything.
                        if tab.editing.try_get_untracked() == Some(true) {
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
                            .clamp(60.0, tab_max_w() - 2.0);
                        s.width(w)
                    })
                    .into_any();
            }

            // Whether this tab currently shows a DB-identity dot (its footprint
            // eats into the title width — see `tab_dot_w()`). Read reactively so the
            // label cap follows a colour assigned/cleared while the tab is open.
            let has_dot = move || {
                tab.database.get().is_some_and(|db| {
                    db_colors
                        .with(|r| schemaic_core::db_color::lookup(r, tab.conn_id.get(), &db))
                        .is_some()
                })
            };

            // Display: label (ellipsized past the tab width) + close ×. A title
            // that would be clipped gets a tooltip with its full text; a title
            // that fits gets none. Both leading glyphs eat into the title's width.
            let file_w = if path.is_some() { tab_file_w() } else { 0.0 };
            let avail =
                move || tab_title_avail() - if has_dot() { tab_dot_w() } else { 0.0 } - file_w;
            let truncated = measure_text_px(&title) > avail();
            // Left inset moved to the row's `padding_left` so the (optional) DB
            // colour dot can lead the label without shifting the text when absent.
            //
            // **Italic is the unsaved marker.** It was a small accent dot before
            // the ×, which read as a second glyph on a chip that can already carry
            // the DB-identity dot — two dots of different meanings, 6px apart. The
            // slant costs no width, so it also needs nothing shed from the title
            // cap. (Measured upright: an italic face is a hair wider, and being a
            // hair late to ellipsize is not worth a second text measurement.)
            let label = text(title.clone()).style(move |s| {
                let s = s
                    .margin_right(theme::scaled(7.0))
                    .max_width(avail())
                    .text_overflow(TextOverflow::Ellipsis)
                    .font_size(theme::font_body());
                if modified {
                    s.font_style(floem::text::Style::Italic)
                } else {
                    s
                }
            });
            // Tooltip chrome comes from the global `TooltipClass` style (see
            // `tooltip_style`), so the tip is just its text.
            //
            // A file tab tips its **full path**, which subsumes the truncated-title
            // case and answers the question the chip can't: *which* `orders.sql`.
            // A modified tab also says what the italic means, since nothing else on
            // screen does. (`modified` implies a path — `Tab::modified` is false
            // without one — so there is no unsaved-but-pathless arm to write.)
            let tip = match (path, truncated, modified) {
                (Some(p), _, true) => Some(format!("{p} — unsaved changes")),
                (Some(p), _, false) => Some(p),
                (None, true, _) => Some(title),
                (None, false, _) => None,
            };
            let label: AnyView = match tip {
                Some(tip) => label.tooltip(move || text(tip.clone())).into_any(),
                None => label.into_any(),
            };

            // Trailing icon (16px, muted `tab_close` tint, same footprint either
            // way so pinning doesn't shift the title width): a clickable × to
            // close, or — when pinned — a non-clickable pin indicator (a pinned
            // tab can't be closed; unpin via the context menu first).
            let close: AnyView = if pinned {
                icons::icon(icons::PIN, 16.0)
                    .style(|s| {
                        s.flex_shrink(0.0_f32)
                            .margin_right(theme::scaled(7.0))
                            .color(theme::tab_close())
                    })
                    .into_any()
            } else {
                let close_x = close_content.clone();
                icons::icon(icons::X, 16.0)
                    .on_click_stop(move |_| (close_x)(tab.id))
                    .style(|s| {
                        s.flex_shrink(0.0_f32)
                            .margin_right(theme::scaled(7.0))
                            .color(theme::tab_close())
                            // Brighten on hover to the tab's full-brightness text
                            // colour (same one the inactive-tab hover uses).
                            .hover(|s| s.color(theme::text()))
                    })
                    .into_any()
            };
            // Small DB-identity dot leading the label (only when this tab's
            // database has a colour; zero-footprint otherwise).
            let dot = db_color_dot(
                db_colors,
                move || tab.database.get().map(|db| (tab.conn_id.get(), db)),
                0.0,
                6.0,
                -1.0,
            );
            // A `.sql`-backed tab leads its title with a dim file glyph — the
            // standing sign that this tab *is* a file, where the italic is only
            // the transient sign that it has drifted from one. Tinted
            // `tab_close`, the same muted tint as the trailing ×/pin, so it reads
            // as chrome rather than as a third piece of state competing with the
            // DB-identity dot beside it. Zero-footprint when there's no file.
            let file_icon: AnyView = if file_w > 0.0 {
                icons::icon(icons::FILE, 14.0)
                    .style(|s| {
                        s.flex_shrink(0.0_f32)
                            .margin_right(theme::scaled(5.0))
                            .color(theme::tab_close())
                    })
                    .into_any()
            } else {
                empty().into_any()
            };
            h_stack((dot, file_icon, label, close))
                .style(|s| {
                    s.flex_row()
                        .items_center()
                        .padding_left(theme::scaled(10.0))
                })
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
        // clash with activating the tab or double-click-to-rename. (A pinned tab's
        // close is gated in `close_tab`, so middle-click no-ops on it.)
        .on_event(EventListener::PointerDown, move |e| {
            if let Event::PointerDown(pe) = e
                && pe.button.is_auxiliary()
            {
                (close_mid)(tab.id);
                return EventPropagation::Stop;
            }
            EventPropagation::Continue
        })
        // Right-click context menu — reuses the generic popup channel and its
        // cursor edge-flipping (`popup_anchor = None` ⇒ open at the cursor, flips
        // left/up near the window edge, so a right-most tab doesn't overflow). A
        // pinned tab shows Unpin and omits Close (unpin to close).
        //
        // Three groups: the clicked tab, then its `.sql` file, then the strip
        // (reopen the last close, or close the lot) — the last being the reason
        // the separators were there before a file group needed fencing off.
        .on_secondary_click_stop(move |_| {
            overlay.context_menu.set(None);
            overlay.popup_anchor.set(None);
            let pinned = tab.pinned.get_untracked();
            let mut entries = vec![
                MenuEntry::action(if pinned { "Unpin" } else { "Pin" }, {
                    let toggle = toggle_pin.clone();
                    move || (toggle)(tab.id)
                }),
                // Same inline rename the double-click opens — the field carries
                // its own commit/discard rules, so this only has to seed the
                // buffer and switch the chip into edit mode. Offered on a pinned
                // tab too: a name has nothing to do with whether it can close.
                MenuEntry::action("Rename", move || {
                    if !tab.editing.get_untracked() {
                        tab.edit_buf.set(tab.title());
                        tab.editing.set(true);
                    }
                }),
                MenuEntry::action("Duplicate", {
                    let duplicate = duplicate.clone();
                    move || (duplicate)(tab.id)
                }),
            ];
            if !pinned {
                let close = close_tab.clone();
                entries.push(MenuEntry::action("Close", move || (close)(tab.id)));
            }
            // The `.sql` file group, fenced off on both sides: these are the only
            // entries about a file rather than about the tab, and Ctrl+O is
            // otherwise reachable by keyboard alone. Open leads it, matching the
            // key order and the palette's.
            entries.push(MenuEntry::Separator);
            // Sentence case and no trailing ellipsis, like every other entry here
            // ("Reopen last tab", "Reload from disk") — this menu doesn't mark the
            // entries that open something, and two of the three that do are the
            // ones the user is least likely to be surprised by.
            entries.push(MenuEntry::action("Open file", {
                let open = open_file.clone();
                move || (open)()
            }));
            // Save is offered on a tab with no file too — it opens Save as, which
            // is the answer to "save this" there.
            entries.push(MenuEntry::action("Save", {
                let save = save_file.clone();
                move || (save)(tab.id)
            }));
            entries.push(MenuEntry::action("Save as", {
                let save_as = save_file_as.clone();
                move || (save_as)(tab.id)
            }));
            entries.push(
                // Dimmed rather than hidden on a tab with no file, the way
                // "Reopen last tab" is when the ring is empty — the menu keeps
                // one shape and says why an entry is unavailable by looking it.
                MenuEntry::action("Reload from disk", {
                    let reload = reload_file.clone();
                    move || (reload)(tab.id)
                })
                .disabled(tab.path.get_untracked().is_none()),
            );
            entries.push(MenuEntry::Separator);
            entries.push(
                MenuEntry::action("Reopen last tab", {
                    let reopen = reopen.clone();
                    move || (reopen)()
                })
                // Dimmed when this connection has nothing closed to restore —
                // the ring is per-connection, like the strip.
                .disabled(!(can_reopen)()),
            );
            // Before "Close all tabs", and unlike it this one *is* about the
            // clicked tab — it is the one kept. Offered on a pinned tab too: a
            // pinned tab is already the one that survives everything, so
            // "close the others" is exactly as meaningful there.
            entries.push(
                MenuEntry::action("Close other tabs", {
                    let close_others = close_others.clone();
                    move || (close_others)(tab.id)
                })
                // Dimmed with nothing else to close — the app's own opening
                // state, where it used to return before the confirm with no
                // dialog and no message, one row below an entry that *is*
                // dimmed for the same kind of reason.
                .disabled(!(can_close_others)(tab.id)),
            );
            entries.push(MenuEntry::action("Close all tabs", {
                let close_all = close_all.clone();
                move || (close_all)()
            }));
            // Wider than the old Pin/Duplicate/Close set needed — "Close other
            // tabs" is the longest label here. It's a `min_width`, so a longer
            // one would widen the panel rather than be clipped.
            overlay.popup_width.set(150.0);
            overlay.popup_menu.set(Some(entries));
        })
        // Flat, full-height tab capped at `tab_max_w()`: chrome background (invisible
        // against the strip) when inactive, `tab_active` when active; a right
        // separator line divides it from the next tab. The container's `color`
        // cascades to the label + ×.
        .style(move |s| {
            let s = s
                .flex_row()
                .items_center()
                .max_width(tab_max_w())
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
