//! The Live Monitor modal.
//!
//! Opened from the RESULTS title bar's **Monitor** button (which calls
//! `tab_actions.open_monitor` with the tab's source `(conn_id, database,
//! table)`). The app polls that table on an interval, diffs each snapshot against
//! the previous one (`schemaic_core::monitor`), and appends the resulting
//! inserts/updates/deletes to `overlay.monitor_log`; this modal renders that log
//! as a scrollable table (Time · Action · ID · Data). Closing (✕ / Esc /
//! backdrop) sets `overlay.monitor_open` false, which stops the poll loop.

use std::rc::Rc;
use std::time::Duration;

use floem::AnyView;
use floem::event::EventListener;
use floem::keyboard::{Key, NamedKey};
use floem::kurbo::{Point, Rect};
use floem::peniko::Color;
use floem::prelude::*;
use floem::reactive::create_effect;

use schemaic_core::monitor::{ChangeKind, RowChange};

use crate::settings::settings_dropdown;
use crate::theme::{FONT_BODY, FONT_LABEL};
use crate::widgets::{
    autohide_state, focus_root, follow_after_scroll, loading_dots, panel_style, shift_hscroll,
    thin_scroll, with_scroll_gesture,
};

/// The log's rows are shorter than a chat bubble, so it counts as "at the bottom"
/// a little tighter than the AI panel's `FOLLOW_SLACK`.
const MONITOR_FOLLOW_SLACK: f64 = 24.0;
use crate::{MonitorEntry, Ui, icons, theme};

/// Modal size (fixed so the log scrolls within it).
const MON_W: f64 = 660.0;
const MON_H: f64 = 460.0;

// Column widths + shared row metrics (header and rows must agree so they align).
const TIME_W: f64 = 54.0;
const ACT_W: f64 = 66.0;
const ID_W: f64 = 72.0;
const COL_GAP: f64 = 10.0;
const ROW_PAD_H: f64 = 14.0;

/// Old-value tint (removed) and new-value tint (added), shared by updates + inserts.
fn old_color() -> Color {
    Color::rgb8(0x9D, 0x34, 0x34)
}
fn new_color() -> Color {
    Color::rgb8(0x71, 0xC3, 0x71)
}

pub(crate) fn monitor_overlay(ui: Ui) -> impl IntoView {
    let open = ui.overlay.monitor_open;
    let title = ui.overlay.monitor_title;
    let cols = ui.overlay.monitor_cols;
    let log = ui.overlay.monitor_log;
    let error = ui.overlay.monitor_error;
    let partial = ui.overlay.monitor_partial;
    let interval = ui.overlay.monitor_interval;

    dyn_container(
        move || open.get(),
        move |is_open| {
            if !is_open {
                return empty().into_any();
            }
            let close: Rc<dyn Fn()> = Rc::new(move || {
                open.set(false);
                // Free the log + reset state so a reopen starts clean (the app also
                // resets on open, so this is just tidiness / no stale flash).
                log.set(Vec::new());
                error.set(None);
                title.set(None);
                cols.set(Vec::new());
            });

            // Scroll bookkeeping: `bump` triggers a scroll-to-bottom, `view_rect` +
            // `content_h` decide whether we're at the bottom, `hscroll` mirrors the
            // body's horizontal offset onto the (vertically-fixed) table header.
            let bump = RwSignal::new(0u64);
            let content_h = RwSignal::new(0.0_f64);
            let view_rect = RwSignal::new(Rect::ZERO);
            let hscroll = RwSignal::new(0.0_f64);
            // Whether to auto-follow: true while the user is at (or near) the bottom.
            // Flipped by `on_scroll`; read by `scroll_to`. Floem's `scroll_to` keeps
            // re-applying its last target as content grows, so following is expressed
            // by *returning `None` while scrolled up* (which clears the target the
            // instant you scroll away) rather than by gating the bump.
            let follow = RwSignal::new(true);
            // Bump one tick after each log change (deferred so the new row has laid
            // out and a scroll-to-bottom reaches the true bottom). Whether that bump
            // actually scrolls is decided by `follow` inside `scroll_to`.
            create_effect(move |_| {
                log.with(|_| ());
                floem::action::exec_after(Duration::ZERO, move |_| {
                    let _ = bump.try_update(|n| *n = n.wrapping_add(1));
                });
            });

            // Modal header — matches `modal_title` (title 15/bold, dim→bright X,
            // 14/10 padding, bottom separator) but with a dynamic title string.
            let heading = title
                .get_untracked()
                .map(|t| format!("Live Monitor — {t}"))
                .unwrap_or_else(|| "Live Monitor".to_string());
            let close_x = close.clone();
            let modal_header = h_stack((
                text(heading).style(|s| s.font_size(15.0).font_bold().color(theme::text())),
                empty().style(|s| s.flex_grow(1.0_f32)),
                container(icons::icon(icons::X, 16.0))
                    .on_click_stop(move |_| (close_x)())
                    .style(|s| {
                        s.flex_shrink(0.0_f32)
                            .items_center()
                            .color(theme::text_dim())
                            .hover(|s| s.color(theme::text()))
                    }),
            ))
            .style(|s| {
                s.width_full()
                    .flex_row()
                    .items_center()
                    .padding_horiz(14.0)
                    .padding_vert(10.0)
                    .border_bottom(1.0)
                    .border_color(theme::border())
            });

            // Sub-line under the separator: a live note (or the latest poll error)
            // on the left, and the poll-interval dropdown on the right — inline with
            // the subtitle, below the header's X.
            let status_text = dyn_container(
                move || (error.get(), partial.get()),
                move |(err, partial)| match err {
                    // `error()`, not `reject_text()`: the latter is the foreground
                    // for a `reject_bg` pill and measures ~1.03:1 free-standing on
                    // the panel in *both* themes — legible nowhere.
                    Some(msg) => text(msg)
                        .style(|s| s.color(theme::error()).font_size(FONT_LABEL))
                        .into_any(),
                    // A table bigger than the poll's cap is watched a page at a
                    // time, ordered by its key. Changes past that page can't be
                    // seen at all, so the status line says which rows are covered
                    // rather than implying the whole table is.
                    None if partial => text(format!(
                        "Watching the first {} rows by primary key — changes beyond \
                         them aren't visible.",
                        schemaic_core::monitor::ROW_CAP
                    ))
                    .style(|s| s.color(theme::plan_warn()).font_size(FONT_LABEL))
                    .into_any(),
                    None => {
                        text("Watching for inserts, updates and deletes — newest at the bottom.")
                            .style(|s| s.color(theme::text_dim()).font_size(FONT_LABEL))
                            .into_any()
                    }
                },
            )
            .style(|s| s.flex_grow(1.0_f32).min_width(0.0));
            let interval_dd = container(settings_dropdown(
                interval,
                [1u64, 2, 5, 10],
                interval_label,
            ))
            .style(|s| s.width(84.0).flex_shrink(0.0_f32));
            let status = h_stack((status_text, interval_dd)).style(|s| {
                s.width_full()
                    .flex_row()
                    .items_center()
                    .gap(10.0)
                    .padding_horiz(14.0)
                    .padding_top(10.0)
                    // 20px gap down to the table (or the empty-state message).
                    .padding_bottom(20.0)
            });

            // Body: the change table (header + rows), or a centred placeholder while
            // empty — so an empty monitor shows just "No changes yet", not a bare
            // header. Rows are content-sized (no wrap), so it scrolls both axes.
            let content = dyn_container(
                move || log.with(|l| l.is_empty()),
                move |empty_log| {
                    if empty_log {
                        return container(loading_dots("Waiting", theme::text_dim, 13.0))
                            .style(|s| s.size_full().items_center().justify_center())
                            .into_any();
                    }
                    // Header: full-width separator bar whose labels scroll
                    // horizontally in sync with the body (reads `hscroll`; never
                    // writes it, and blocks its own wheel — the one-writer rule).
                    let table_header = container(
                        scroll(header_row())
                            .scroll_style(|cs| cs.hide_bars(true))
                            .scroll_to(move || Some(Point::new(hscroll.get(), 0.0)))
                            .on_event_stop(EventListener::PointerWheel, |_| {})
                            .style(|s| s.width_full()),
                    )
                    .style(|s| {
                        s.width_full()
                            .flex_shrink(0.0_f32)
                            .border_bottom(1.0)
                            .border_color(theme::border())
                    });
                    let (shown, poke) = autohide_state();
                    let list = dyn_stack(
                        move || log.get().into_iter().enumerate().collect::<Vec<_>>(),
                        |(i, _)| *i,
                        move |(_, entry)| entry_row(entry, cols),
                    )
                    .style(|s| {
                        s.flex_col()
                            .padding_horiz(ROW_PAD_H)
                            .padding_vert(8.0)
                            .gap(2.0)
                    });
                    let (list_scroll, by_user) = with_scroll_gesture(shift_hscroll(
                        list.on_resize(move |r| content_h.set(r.height())),
                    ));
                    let list_scroll = list_scroll
                        .scroll_style(move |cs| thin_scroll(cs).hide_bars(!shown.get()))
                        .on_scroll(move |vp| {
                            view_rect.set(vp);
                            hscroll.set(vp.x0);
                            // Released only by the reader: a poll appending rows moves
                            // the bottom without anyone asking for it.
                            // Only notify on a real flip: `set` never dedups, and a
                            // redundant notify would re-snap while the user scrolls
                            // near the bottom.
                            let keep = follow_after_scroll(
                                follow.get_untracked(),
                                (by_user)(),
                                vp.y1,
                                content_h.get_untracked(),
                                MONITOR_FOLLOW_SLACK,
                            );
                            if follow.get_untracked() != keep {
                                follow.set(keep);
                            }
                            poke();
                        })
                        // Follow the bottom on `bump` — but only while `follow`;
                        // `None` when scrolled up leaves the view put (and clears
                        // any sticky target). Preserves the horizontal offset.
                        .scroll_to(move || {
                            bump.get();
                            follow
                                .get()
                                .then(|| Point::new(view_rect.get_untracked().x0, 1.0e9))
                        })
                        .style(|s| s.flex_grow(1.0_f32).width_full().min_height(0.0));
                    v_stack((table_header, list_scroll))
                        .style(|s| s.flex_col().flex_grow(1.0_f32).width_full().min_height(0.0))
                        .into_any()
                },
            )
            .style(|s| s.flex_grow(1.0_f32).width_full().min_height(0.0));

            // The card is the flex_col itself (fixed height) so the scroll clips.
            let panel = v_stack((modal_header, status, content))
                .on_click_stop(|_| {})
                .style(|s| {
                    panel_style(s)
                        .width(MON_W)
                        .height(MON_H)
                        .background(theme::bg_panel())
                        .border_color(theme::modal_border())
                });

            let close_bg = close.clone();
            focus_root(container(panel))
                .on_key_down(Key::Named(NamedKey::Escape), |_| true, move |_| (close)())
                .on_click_stop(move |_| (close_bg)())
                .style(|s| {
                    s.size_full()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .background(theme::modal_backdrop())
                })
                .into_any()
        },
    )
    .style(move |s| {
        if open.get() {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}

/// The bold, left-aligned column header row (`Time · Action · ID · Data`), sized
/// to match [`entry_row`]'s columns so they line up.
fn header_row() -> impl IntoView {
    let cell = |t: &'static str, w: Option<f64>| {
        text(t).style(move |s| {
            let s = s
                .color(theme::text_muted())
                .font_size(FONT_LABEL)
                .font_bold();
            match w {
                Some(w) => s.width(w).flex_shrink(0.0_f32),
                None => s,
            }
        })
    };
    h_stack((
        cell("Time", Some(TIME_W)),
        cell("Action", Some(ACT_W)),
        cell("ID", Some(ID_W)),
        cell("Data", None),
    ))
    .style(|s| {
        s.items_center()
            .gap(COL_GAP)
            .padding_horiz(ROW_PAD_H)
            .padding_vert(6.0)
    })
}

/// One change row: `[time] [KIND] [key] [data]` — content-sized (no wrap) so a
/// long change list scrolls horizontally instead of wrapping.
fn entry_row(entry: MonitorEntry, cols: RwSignal<Vec<String>>) -> impl IntoView {
    let names = cols.get_untracked();
    let (label, color) = match entry.change.kind {
        ChangeKind::Insert => ("INSERT", new_color()),
        ChangeKind::Update => ("UPDATE", theme::chip_active()),
        ChangeKind::Delete => ("DELETE", old_color()),
    };
    let key_text = entry.change.key.join(", ");
    h_stack((
        text(entry.at).style(|s| {
            s.color(theme::text_dim())
                .font_size(FONT_LABEL)
                .width(TIME_W)
                .flex_shrink(0.0_f32)
        }),
        text(label).style(move |s| {
            s.color(color)
                .font_size(FONT_LABEL)
                .font_bold()
                .width(ACT_W)
                .flex_shrink(0.0_f32)
        }),
        text(key_text).style(|s| {
            s.color(theme::text())
                .font_size(FONT_BODY)
                .width(ID_W)
                .flex_shrink(0.0_f32)
        }),
        data_view(&entry.change, &names),
    ))
    .style(|s| s.items_center().gap(COL_GAP).padding_vert(7.0))
}

/// The Data column, as one non-wrapping line of coloured spans: for an update,
/// `col: old → new` (old red, new green); for an insert, `col=value` (all
/// green); for a delete, the same `col=value` in the *old* colour.
///
/// A delete used to render nothing, on the rationale that the key column already
/// identifies the row — while `diff_snapshots` was deliberately cloning every
/// deleted row's cells so the log *could* show them. A delete is precisely the
/// case where the row is gone from the database and this log is the only
/// remaining record of what it held, so the core's answer was the right one.
fn data_view(change: &RowChange, cols: &[String]) -> impl IntoView + use<> {
    let name = |ci: usize| cols.get(ci).cloned().unwrap_or_else(|| "?".to_string());
    // The colour arrives as a fn and is called *inside* the style closure. These
    // rows are keyed on the log index, so a theme switch rebuilds none of them —
    // a `Color` read here would be the old theme's for as long as the modal
    // stays open. `old_color`/`new_color` are fixed red/green by design and pass
    // as fns too, so there is one shape rather than two.
    let dim: fn() -> Color = theme::text_dim;
    let span = move |t: String, c: fn() -> Color| {
        text(t).style(move |s| s.color(c()).font_size(FONT_BODY))
    };
    let mut spans: Vec<AnyView> = Vec::new();
    match change.kind {
        ChangeKind::Update => {
            for (i, f) in change.fields.iter().enumerate() {
                if i > 0 {
                    spans.push(span(",   ".to_string(), dim).into_any());
                }
                spans.push(span(format!("{}: ", name(f.col)), dim).into_any());
                spans.push(span(cell(&f.old), old_color).into_any());
                spans.push(span(" → ".to_string(), dim).into_any());
                spans.push(span(cell(&f.new), new_color).into_any());
            }
        }
        // Same shape for both, differing only in the colour that says whether
        // the values are arriving or leaving.
        ChangeKind::Insert | ChangeKind::Delete => {
            let value_color: fn() -> Color = if change.kind == ChangeKind::Insert {
                new_color
            } else {
                old_color
            };
            for (i, (n, c)) in cols.iter().zip(&change.cells).enumerate() {
                if i > 0 {
                    spans.push(span(", ".to_string(), dim).into_any());
                }
                spans.push(span(format!("{n}="), dim).into_any());
                spans.push(span(cell(c), value_color).into_any());
            }
        }
    }
    h_stack_from_iter(spans).style(|s| s.flex_row().items_center().flex_shrink(0.0_f32))
}

/// Poll-interval option labels for the dropdown.
fn interval_label(secs: u64) -> &'static str {
    match secs {
        1 => "1s",
        2 => "2s",
        5 => "5s",
        10 => "10s",
        _ => "—",
    }
}

/// Render one cell value: `NULL` for a missing value, the text otherwise.
fn cell(c: &Option<String>) -> String {
    match c {
        Some(s) => s.clone(),
        None => "NULL".to_string(),
    }
}
