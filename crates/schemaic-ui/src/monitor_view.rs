//! The Live Monitor modal.
//!
//! Opened from the RESULTS title bar's **Monitor** button (which calls
//! `tab_actions.open_monitor` with the tab's source `(conn_id, database,
//! table)`). The app polls that table on an interval, diffs each snapshot against
//! the previous one (`schemaic_core::monitor`), and appends the resulting
//! inserts/updates/deletes to `overlay.monitor_log`; this modal renders that log
//! as a scrollable table (Time · Action · ID · Data). Closing (✕ / Esc /
//! backdrop) sets `overlay.monitor_open` false, which stops the poll loop.
//!
//! Three controls sit left of the interval dropdown, and they are what make the
//! log something you can *use* rather than only watch: **Pause** holds the poll
//! (the loop keeps re-arming and skips the fetch, so resuming is free),
//! **Clear** empties the log without disturbing the baseline snapshot, and
//! **Export** writes it to a file through the ordinary
//! [`schemaic_core::export`] renderers — the log is projected to a `ResultSet`
//! by [`schemaic_core::monitor::log_result_set`] so no second renderer exists.
//! Export matters most on a delete, where the log is the only remaining record
//! of a row the database no longer has.

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use floem::AnyView;
use floem::action::save_as;
use floem::event::EventListener;
use floem::file::{FileDialogOptions, FileSpec};
use floem::keyboard::{Key, NamedKey};
use floem::kurbo::{Point, Rect};
use floem::peniko::Color;
use floem::prelude::*;
use floem::reactive::create_effect;

use schemaic_core::export::{ExportFormat, suggested_filename};
use schemaic_core::monitor::{ChangeKind, LOG_FORMATS, RowChange, log_result_set};

use crate::settings::focusable_dropdown;
use crate::theme::{font_body, font_label};
use crate::widgets::{
    autohide_state, follow_after_scroll, loading_dots, modal_h, modal_w, panel_style,
    shift_hscroll, thin_scroll, with_scroll_gesture,
};
use crate::{MenuEntry, PopupAnchor};

/// The log's rows are shorter than a chat bubble, so it counts as "at the bottom"
/// a little tighter than the AI panel's `follow_slack`. Scaled for the reason
/// that one is: it is compared against offsets measured in line boxes.
fn monitor_follow_slack() -> f64 {
    theme::scaled(24.0)
}
use crate::{MonitorEntry, Ui, icons, theme};

/// Modal size (fixed so the log scrolls within it). The width carries the
/// sub-header's three icon buttons and the interval dropdown alongside a status
/// line that has to stay readable — at 660 the partial-window warning wrapped
/// into the controls.
fn mon_w() -> f64 {
    modal_w(760.0)
}
/// The modal's height at 100% — passed through [`crate::widgets::modal_h`] at the
/// call site, which scales it and caps it against the window. See
/// `table_designer::PANEL_H` for why this comment used to claim the opposite.
const MON_H: f64 = 510.0;

// Column widths + shared row metrics (header and rows must agree so they align —
// which is also why they scale together).
fn time_w() -> f64 {
    theme::scaled(54.0)
}
fn act_w() -> f64 {
    theme::scaled(66.0)
}
fn id_w() -> f64 {
    theme::scaled(72.0)
}
fn col_gap() -> f64 {
    theme::scaled(10.0)
}
fn row_pad_h() -> f64 {
    theme::scaled(14.0)
}

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
    let paused = ui.overlay.monitor_paused;
    let export_err = ui.overlay.monitor_export_err;
    let exported = ui.overlay.monitor_exported;
    let dropped = ui.overlay.monitor_dropped;
    let confirm = ui.overlay.confirm;
    // Where an export failure lands when the modal has already closed.
    let error_modal_text = ui.overlay.error_modal_text;
    let error_modal_open = ui.overlay.error_modal_open;
    // The shared popup channel — `popup_menu_overlay` is mounted last in the
    // workspace stack, so a menu raised from in here paints above this modal and
    // its backdrop rather than behind them.
    let popup_menu = ui.overlay.popup_menu;
    let popup_anchor = ui.overlay.popup_anchor;
    let export_file = ui.tab_actions.export_file.clone();

    dyn_container(
        move || open.get(),
        move |is_open| {
            if !is_open {
                return empty().into_any();
            }
            let close: Rc<dyn Fn()> = Rc::new(move || {
                open.set(false);
                // **The log is deliberately left alone.** It used to be emptied
                // here "for tidiness / no stale flash", which was harmless while
                // it could only be watched — and became a total, unprompted loss
                // the moment the same commit made it an exportable record: the
                // only copy of a deleted row's values, thrown away by Escape.
                // `open_monitor` resets every one of these signals on the way in,
                // so nothing stale can flash anyway.
                error.set(None);
                title.set(None);
                cols.set(Vec::new());
                // Pause is a property of the session being watched, not of the
                // modal: leaving it set would silently un-monitor the *next*
                // table someone opens.
                paused.set(false);
                export_err.set(None);
            });
            // One control, but it still gets a ring: without one the root has no
            // Tab handler, so Tab falls through to floem's whole-window traversal
            // and walks out of the modal into the workspace behind it.
            let ring = crate::widgets::FocusRing::new();
            // Filled once the Export button exists, so its menu opens under the
            // button rather than at the last cursor position — which, reached by
            // Tab and pressed with Enter, is wherever the pointer was left.
            let export_anchor: RwSignal<Option<floem::ViewId>> = RwSignal::new(None);

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
                text(heading).style(|s| {
                    s.font_size(theme::scaled_font(15.0))
                        .font_bold()
                        .color(theme::text())
                }),
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
            // on the left, then the Pause/Clear/Export controls and the poll-interval
            // dropdown on the right — inline with the subtitle, below the header's X.
            let status_text = dyn_container(
                move || {
                    status_line(
                        export_err.get(),
                        error.get(),
                        paused.get(),
                        partial.get(),
                        // What was actually dropped, not what the length implies:
                        // a log resting *at* the cap has lost nothing yet.
                        dropped.get() > 0,
                    )
                },
                move |(msg, tone)| {
                    text(msg).style(move |s| s.color(tone.color()()).font_size(font_label()))
                },
            )
            .style(|s| s.flex_grow(1.0_f32).min_width(0.0));

            // Pause / Clear / Export, left of the interval dropdown. All three are
            // in the modal's ring, in reading order, so the log is operable without
            // a pointer — a monitor is watched with both hands off the mouse.
            let pause_btn = crate::widgets::in_ring_button(
                dyn_container(
                    move || paused.get(),
                    // `toolbar_icon` takes a `&'static str`, so the play/pause swap
                    // is a rebuild of the *face*. The ring registers the wrapper,
                    // whose id is stable across it.
                    move |p| {
                        crate::widgets::toolbar_icon(
                            if p {
                                icons::CIRCLE_PLAY
                            } else {
                                icons::CIRCLE_PAUSE
                            },
                            0.0,
                            0.0,
                            || true,
                            // Read-modify-write rather than `set(!p)`: `p` is the
                            // value this face was built for, and the keyboard arm
                            // below toggles the same way.
                            move || paused.update(|v| *v = !*v),
                        )
                        .into_any()
                    },
                )
                .tooltip(move || {
                    let t = if paused.get() {
                        "Resume polling"
                    } else {
                        "Pause polling"
                    };
                    text(t).style(crate::widgets::tooltip_style)
                }),
                ring.clone(),
                10,
                true,
                0.0,
                move || paused.update(|p| *p = !*p),
            );
            // Clear and Export are dimmed on an empty log rather than removed, so
            // the row doesn't reflow the moment the first change lands. They stay
            // in the ring either way (`in_ring_button`'s `enabled` is decided once,
            // at build); each guards itself instead, so Enter on a dimmed button
            // does nothing.
            let has_log = move || log.with_untracked(|l| !l.is_empty());
            // **Clear asks, because the log is the only copy.** A `DELETE` it
            // recorded holds values the database no longer has, and a poll never
            // re-reports a change it has already reported — so this is
            // irreversible, and the button sits one glyph from Export with the
            // same metric and the same enabled/dimmed rule. The confirmation is
            // skipped exactly where it would be noise: an empty log, or one
            // already written to a file
            // ([`schemaic_core::monitor::discard_needs_asking`]).
            let clear: Rc<dyn Fn()> = Rc::new(move || {
                if !has_log() {
                    return;
                }
                let n = log.with_untracked(Vec::len);
                if !schemaic_core::monitor::discard_needs_asking(n, exported.get_untracked()) {
                    // Only the log — the baseline snapshot lives in the app and
                    // is deliberately untouched, so clearing loses the history
                    // you already read, never a change not yet reported.
                    log.set(Vec::new());
                    dropped.set(0);
                    return;
                }
                confirm.set(Some(crate::Confirm {
                    title: "Clear the log?".to_string(),
                    message: format!(
                        "{n} change{} would be discarded. The log is the only record \
                         of what a deleted row held — the poll will not report a \
                         change again — and this can't be undone. Export it first if \
                         you want to keep it.",
                        if n == 1 { "" } else { "s" }
                    ),
                    resolve: Rc::new(move |yes| {
                        if yes {
                            log.set(Vec::new());
                            exported.set(false);
                            dropped.set(0);
                        }
                    }),
                }));
            });
            let clear_click = clear.clone();
            let clear_key = clear.clone();
            let clear_btn = crate::widgets::in_ring_button(
                crate::widgets::toolbar_icon(
                    icons::TRASH_2,
                    0.0,
                    0.0,
                    move || log.with(|l| !l.is_empty()),
                    move || (clear_click)(),
                )
                .tooltip(|| text("Clear the log").style(crate::widgets::tooltip_style)),
                ring.clone(),
                11,
                true,
                0.0,
                move || (clear_key)(),
            );
            let export_file = export_file.clone();
            let export_menu: Rc<dyn Fn()> = {
                let export_file = export_file.clone();
                Rc::new(move || {
                    let export_file = export_file.clone();
                    popup_anchor.set(
                        export_anchor
                            .get_untracked()
                            .map(|id| id.layout_rect())
                            .map(|r| PopupAnchor::BelowIcon(r.x0, r.x1, r.y1)),
                    );
                    popup_menu.set(Some(
                        LOG_FORMATS
                            .iter()
                            .map(|&f| {
                                let export_file = export_file.clone();
                                MenuEntry::action(f.label(), move || {
                                    save_log(
                                        log,
                                        cols,
                                        title,
                                        export_err,
                                        exported,
                                        open,
                                        error_modal_text,
                                        error_modal_open,
                                        export_file.clone(),
                                        f,
                                    );
                                })
                            })
                            .collect(),
                    ));
                })
            };
            let export_click = export_menu.clone();
            let export_key = export_menu.clone();
            let export_btn = crate::widgets::in_ring_button(
                crate::widgets::toolbar_icon(
                    icons::DOWNLOAD,
                    0.0,
                    0.0,
                    move || log.with(|l| !l.is_empty()),
                    move || (export_click)(),
                )
                .tooltip(|| text("Export the log…").style(crate::widgets::tooltip_style)),
                ring.clone(),
                12,
                true,
                0.0,
                move || {
                    if has_log() {
                        (export_key)();
                    }
                },
            );
            // The ring wrapper's id, not the glyph's: it is the outermost view of
            // the control, so the menu opens under the whole button. Set after the
            // button exists, like `table_designer::suggest_chevron`.
            export_anchor.set(Some(export_btn.id()));
            let controls = h_stack((pause_btn, clear_btn, export_btn))
                .style(|s| s.flex_row().items_center().flex_shrink(0.0_f32));

            let interval_dd = container(focusable_dropdown(
                interval,
                [1u64, 2, 5, 10],
                interval_label,
                ring.clone(),
                13,
            ))
            .style(|s| s.width(theme::scaled(84.0)).flex_shrink(0.0_f32));
            let status = h_stack((status_text, controls, interval_dd)).style(|s| {
                s.width_full()
                    .flex_row()
                    .items_center()
                    .gap(10.0)
                    .padding_horiz(14.0)
                    .padding_top(10.0)
                    // 20px gap down to the table (or the empty-state message).
                    .padding_bottom(20.0)
            });

            // **A memo, not the log.** `dyn_container` does not diff — it rebuilds
            // whenever a dependency fires — and `with` subscribes, so reading the log
            // here meant every poll that landed a single change tore down and rebuilt
            // the whole table: header, both scrolls and up to `LOG_CAP` rows, twice
            // cloning the log on the way. The list jumped back to the top (the new
            // scroll starts at zero and `follow` is false exactly when the reader has
            // scrolled away) and the header desynchronised from the body
            // horizontally, so reading history in a live monitor was impossible
            // except by pausing. The memo fires only on the empty↔non-empty edge.
            let is_empty = floem::reactive::create_memo(move |_| log.with(|l| l.is_empty()));

            // Body: the change table (header + rows), or a centred placeholder while
            // empty — so an empty monitor shows just "Waiting…", not a bare header.
            // Rows are content-sized (no wrap), so it scrolls both axes.
            let content = dyn_container(
                move || is_empty.get(),
                move |empty_log| {
                    if empty_log {
                        // `paused` is tracked *here*, inside the empty branch, not in
                        // the outer selector: adding it there would rebuild the whole
                        // table on every pause toggle and throw away its scroll
                        // position, which is the thing someone pauses in order to read.
                        return container(dyn_container(
                            move || paused.get(),
                            move |is_paused| {
                                if is_paused {
                                    // Static, not `loading_dots`: cycling dots say
                                    // "any moment now", and a paused monitor is not
                                    // waiting for anything.
                                    return text("Paused.")
                                        .style(|s| {
                                            s.color(theme::text_dim())
                                                .font_size(theme::scaled_font(13.0))
                                        })
                                        .into_any();
                                }
                                loading_dots("Waiting", theme::text_dim, font_body).into_any()
                            },
                        ))
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
                    // **Keyed on the entry's own sequence number, not its position.**
                    // The log is a sliding window at `LOG_CAP`, so the index set
                    // stays `{0..999}` while the thousand changes it describes move
                    // — and floem reuses a view whose key didn't change, which would
                    // freeze the rendered list at the first thousand while Export
                    // went on sliding. Nothing hid that before because the whole
                    // table was being rebuilt per poll.
                    let list = dyn_stack(
                        move || log.get(),
                        |entry| entry.seq,
                        move |entry| entry_row(entry, cols),
                    )
                    .style(|s| {
                        s.flex_col()
                            .padding_horiz(row_pad_h())
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
                                monitor_follow_slack(),
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
                        .width(mon_w())
                        .height(modal_h(MON_H))
                        .background(theme::bg_panel())
                        .border_color(theme::modal_border())
                });

            let close_bg = close.clone();
            // On a sibling behind the panel, never on the focus root: Space is
            // the reflex for "scroll this log", and floem fires `Click` on the
            // focused view for it — which closed the modal, stopped the poll and
            // emptied the change log, deletes included. See
            // `widgets::dismiss_layer`.
            crate::widgets::focus_root_with_ring(
                stack((crate::widgets::dismiss_layer(move || (close_bg)()), panel)),
                ring,
            )
            .on_key_down(Key::Named(NamedKey::Escape), |_| true, move |_| (close)())
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
    // The width arrives as a `fn`, resolved inside the closure — the rows below
    // call `time_w()`/`act_w()`/`id_w()` in *their* style closures, so a header
    // that captured the numbers at build stopped sitting over its own columns the
    // moment the interface scale moved them. The comment on those three widths
    // states the requirement: header and rows must agree, which is also why they
    // scale together.
    let cell = |t: &'static str, w: Option<fn() -> f64>| {
        text(t).style(move |s| {
            let s = s
                .color(theme::text_muted())
                .font_size(font_label())
                .font_bold();
            match w {
                Some(w) => s.width(w()).flex_shrink(0.0_f32),
                None => s,
            }
        })
    };
    h_stack((
        cell("Time", Some(time_w)),
        cell("Action", Some(act_w)),
        cell("ID", Some(id_w)),
        cell("Data", None),
    ))
    .style(|s| {
        s.items_center()
            .gap(col_gap())
            .padding_horiz(row_pad_h())
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
                .font_size(font_label())
                .width(time_w())
                .flex_shrink(0.0_f32)
        }),
        text(label).style(move |s| {
            s.color(color)
                .font_size(font_label())
                .font_bold()
                .width(act_w())
                .flex_shrink(0.0_f32)
        }),
        text(key_text).style(|s| {
            s.color(theme::text())
                .font_size(font_body())
                .width(id_w())
                .flex_shrink(0.0_f32)
        }),
        data_view(&entry.change, &names),
    ))
    .style(|s| s.items_center().gap(col_gap()).padding_vert(7.0))
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
        text(t).style(move |s| s.color(c()).font_size(font_body()))
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

/// How loudly the sub-header's status line should read. Separate from the colour
/// so the line itself is a pure, testable decision — the colour is looked up
/// through [`Tone::color`], which hands back a `fn() -> Color` rather than a
/// `Color` because it is read inside a reactive style (see the themable-colour
/// invariant).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tone {
    Error,
    /// The monitor is working, but not on what you'd assume from the log alone.
    Warn,
    Calm,
}

impl Tone {
    fn color(self) -> fn() -> Color {
        match self {
            // `error()`, not `reject_text()`: the latter is the foreground for a
            // `reject_bg` pill and measures ~1.03:1 free-standing on the panel in
            // *both* themes — legible nowhere.
            Tone::Error => theme::error,
            Tone::Warn => theme::plan_warn,
            Tone::Calm => theme::text_dim,
        }
    }
}

/// What the sub-header's status line says, and how loudly.
///
/// Errors take the line outright, worst-to-mislead first: a **failed export**
/// beats everything, because the user believes they have a file and doesn't; a
/// **poll error** beats the rest, because nothing below it is true while polling
/// is broken.
///
/// Otherwise the line is a lead plus every caveat that currently applies, because
/// they *co-occur* and each one is a different reason the log isn't the whole
/// story: a paused monitor watching a capped page of an oversized table is three
/// things at once, and any of them dropped for brevity is the one the reader
/// needed. `partial` and `capped` are the two silent truncations — rows the poll
/// can't see, and changes the log has already dropped — and the second only
/// became load-bearing when the log became exportable.
fn status_line(
    export_err: Option<String>,
    poll_err: Option<String>,
    paused: bool,
    partial: bool,
    capped: bool,
) -> (String, Tone) {
    if let Some(msg) = export_err.or(poll_err) {
        return (msg, Tone::Error);
    }
    let mut caveats: Vec<String> = Vec::new();
    if partial {
        caveats.push(format!(
            "only the first {} rows by primary key are covered",
            schemaic_core::monitor::ROW_CAP
        ));
    }
    if capped {
        caveats.push(format!(
            "the log is at its {}-change cap, so the oldest are dropping",
            schemaic_core::monitor::LOG_CAP
        ));
    }
    if caveats.is_empty() {
        return match paused {
            true => (
                "Paused — the log keeps what it has; nothing new is being captured.".to_string(),
                Tone::Warn,
            ),
            false => (
                "Watching for inserts, updates and deletes — newest at the bottom.".to_string(),
                Tone::Calm,
            ),
        };
    }
    let lead = if paused { "Paused" } else { "Watching" };
    (format!("{lead} — {}.", caveats.join("; ")), Tone::Warn)
}

/// Write the change log to a file the user picks, in `format`.
///
/// The log is **snapshotted before the dialog opens**, for the reason
/// `grid::save_export` gives: the dialog is modal and slow, and here the poll
/// keeps appending behind it, so rendering afterwards would save a log the user
/// never saw. The rendering itself is the ordinary [`schemaic_core::export`]
/// path over [`log_result_set`]'s projection — the app owns the worker thread
/// that actually writes, exactly as it does for a results export.
#[allow(clippy::too_many_arguments)]
fn save_log(
    log: RwSignal<Vec<crate::MonitorEntry>>,
    cols: RwSignal<Vec<String>>,
    title: RwSignal<Option<String>>,
    export_err: RwSignal<Option<String>>,
    exported: RwSignal<bool>,
    open: RwSignal<bool>,
    fallback_err: RwSignal<Option<String>>,
    fallback_open: RwSignal<bool>,
    export_file: crate::ExportFn,
    format: ExportFormat,
) {
    export_err.set(None);
    // `sakila.actor-monitor.csv` — the watched table, marked as the log of it
    // rather than a dump of it. `suggested_filename` sanitizes and adds the
    // extension.
    let base = title.get_untracked().map(|t| format!("{t}-monitor"));
    let opts = FileDialogOptions::new()
        .title("Export change log")
        .default_name(suggested_filename(base.as_deref(), format))
        .allowed_types(vec![FileSpec {
            name: format.label(),
            extensions: format.extensions(),
        }]);
    let rs = Arc::new(log_result_set(&log.get_untracked(), &cols.get_untracked()));
    // The log is already in the order it should be read (oldest first) and the
    // export applies no sort of its own, so display order is row order.
    let order: Arc<Vec<usize>> = Arc::new((0..rs.row_count()).collect());
    save_as(opts, move |file| {
        let Some(path) = file.and_then(|f| f.path.first().cloned()) else {
            return; // cancelled
        };
        (export_file)(
            crate::ExportRequest {
                path,
                format,
                // `save_as` takes an `Fn`, so the snapshot is cloned per call —
                // two `Arc` bumps, not the rows.
                rs: rs.clone(),
                order: order.clone(),
                // No base table, and the dialect is unread: only the SQL renderer
                // consults either, and `LOG_FORMATS` deliberately doesn't offer
                // it — these rows are observations *about* a table, not rows of
                // one, so there is nothing to `INSERT INTO`.
                source: None,
                dialect: Default::default(),
            },
            // `try_update`: the modal may have closed while the dialog was open
            // or the write ran, and a plain `set` would panic on a freed signal.
            Rc::new(move |res| match res {
                // The log on screen now has a copy on disk, which is what lets
                // Clear stop asking (`monitor::discard_needs_asking`).
                Ok(()) => {
                    exported.try_update(|v| *v = true);
                }
                // **A failure that lands after the modal closed has to go
                // somewhere.** `monitor_export_err` is only visible inside the
                // modal, so reporting there and nowhere else left the user with
                // the truncated file `render_to` warns about and no idea it had
                // happened. The shared error modal is where every other failure
                // with no surface of its own goes.
                //
                // The message is used as the pipeline produced it: it already
                // begins "Export failed: …", and a second prefix here read
                // "Export failed — Export failed: Access is denied".
                Err(e) => {
                    if open.try_get_untracked() == Some(true) {
                        export_err.try_update(|v| *v = Some(e));
                    } else {
                        fallback_err.try_update(|v| *v = Some(e));
                        fallback_open.try_update(|v| *v = true);
                    }
                }
            }),
        );
    });
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `status_line` with no error and nothing paused/capped — the two truncation
    /// flags are what most of these vary.
    fn line(paused: bool, partial: bool, capped: bool) -> (String, Tone) {
        status_line(None, None, paused, partial, capped)
    }

    #[test]
    fn idle_watching_reads_calm() {
        let (msg, tone) = line(false, false, false);
        assert_eq!(tone, Tone::Calm);
        assert!(msg.starts_with("Watching for inserts"), "{msg}");
    }

    #[test]
    fn a_failed_export_outranks_everything_else() {
        // The user believes they have a file. Nothing else on this line matters
        // as much, and a poll error two seconds later must not bury it.
        //
        // **The message is the one the pipeline really produces** — `export_file`
        // formats `Export failed: {e}` and `save_log` passes it through. The
        // earlier fixture said "Export failed — access denied", which no code
        // path could make, and it pinned a wording that was in fact rendered
        // "Export failed — Export failed: Access is denied" on screen.
        let real = "Export failed: Access is denied. (os error 5)";
        let (msg, tone) = status_line(
            Some(real.into()),
            Some("connection lost".into()),
            true,
            true,
            true,
        );
        assert_eq!(tone, Tone::Error);
        assert_eq!(msg, real);
        assert_eq!(
            msg.matches("Export failed").count(),
            1,
            "the prefix must not be doubled: {msg}"
        );
    }

    #[test]
    fn a_poll_error_outranks_the_caveats() {
        let (msg, tone) = status_line(None, Some("connection lost".into()), false, true, true);
        assert_eq!(tone, Tone::Error);
        assert_eq!(msg, "connection lost");
    }

    #[test]
    fn paused_says_the_log_is_kept_and_nothing_is_captured() {
        let (msg, tone) = line(true, false, false);
        assert_eq!(tone, Tone::Warn);
        assert!(msg.starts_with("Paused —"), "{msg}");
        assert!(msg.contains("nothing new is being captured"), "{msg}");
    }

    #[test]
    fn a_partial_window_names_the_row_cap() {
        let (msg, tone) = line(false, true, false);
        assert_eq!(tone, Tone::Warn);
        assert!(msg.starts_with("Watching —"), "{msg}");
        assert!(
            msg.contains(&schemaic_core::monitor::ROW_CAP.to_string()),
            "{msg}"
        );
    }

    #[test]
    fn a_full_log_says_the_oldest_are_dropping() {
        // The caveat that makes an export honest: a log at its cap has already
        // lost entries, and a file written from it looks complete.
        let (msg, tone) = line(false, false, true);
        assert_eq!(tone, Tone::Warn);
        assert!(msg.contains("oldest are dropping"), "{msg}");
        assert!(
            msg.contains(&schemaic_core::monitor::LOG_CAP.to_string()),
            "{msg}"
        );
    }

    #[test]
    fn co_occurring_caveats_are_both_said() {
        // The case the earlier one-branch-per-state shape got wrong: these are
        // different reasons the log isn't the whole story, so neither replaces
        // the other.
        let (msg, tone) = line(false, true, true);
        assert_eq!(tone, Tone::Warn);
        assert!(msg.contains("by primary key are covered"), "{msg}");
        assert!(msg.contains("oldest are dropping"), "{msg}");
    }

    #[test]
    fn pausing_changes_the_lead_and_keeps_the_caveats() {
        let (msg, tone) = line(true, true, true);
        assert_eq!(tone, Tone::Warn);
        assert!(msg.starts_with("Paused —"), "{msg}");
        assert!(msg.contains("by primary key are covered"), "{msg}");
        assert!(msg.contains("oldest are dropping"), "{msg}");
    }

    #[test]
    fn every_line_is_one_sentence_ending_in_a_stop() {
        for paused in [false, true] {
            for partial in [false, true] {
                for capped in [false, true] {
                    let (msg, _) = line(paused, partial, capped);
                    assert!(msg.ends_with('.'), "{paused}/{partial}/{capped}: {msg}");
                }
            }
        }
    }

    #[test]
    fn the_tones_map_to_distinct_theme_colours() {
        // Guards the `fn() -> Color` indirection: a `Tone` that resolved to the
        // same colour as another would make a warning unreadable as one.
        let (e, w, c) = (
            Tone::Error.color()(),
            Tone::Warn.color()(),
            Tone::Calm.color()(),
        );
        assert_ne!(e, w);
        assert_ne!(w, c);
        assert_ne!(e, c);
    }
}
