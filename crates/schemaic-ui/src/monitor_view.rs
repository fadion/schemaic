//! The Live Monitor modal.
//!
//! Opened from the RESULTS title bar's **Monitor** button (which calls
//! `tab_actions.open_monitor` with the tab's source `(conn_id, database,
//! table)`). The app polls that table on an interval, diffs each snapshot against
//! the previous one (`schemaic_core::monitor`), and appends the resulting
//! inserts/updates/deletes to `overlay.monitor_log`; this modal renders that log
//! as a table — Time, Action, then one column per watched column, coloured as
//! Compare results colours its differences: an insert green, a delete red and
//! struck through, an update's changed values `old → new` beside the values it
//! left alone. Closing (✕ / Esc / Close) sets `overlay.monitor_open` false, which
//! stops the poll loop.
//!
//! The title names the table and wears a *Watching* or *Paused* badge. Under it,
//! the filter pills *All · Inserts · Updates · Deletes*, each with its count, and
//! three controls left of the interval dropdown, which are what make the log
//! something you can *use* rather than only watch: **Pause** holds the poll (the
//! loop keeps re-arming and skips the fetch, so resuming is free), **Clear**
//! empties the log without disturbing the baseline snapshot, and **Export**
//! writes it to a file through the ordinary [`schemaic_core::export`] renderers —
//! the log is projected to a `ResultSet` by
//! [`schemaic_core::monitor::log_result_set`] so no second renderer exists.
//! Export matters most on a delete, where the log is the only remaining record
//! of a row the database no longer has. The footer counts the log, or says what
//! went wrong and what the log cannot see.

use std::cell::RefCell;
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
use floem::reactive::{Memo, create_effect, create_memo};
use floem::views::{VirtualDirection, VirtualItemSize, VirtualVector, virtual_stack};

use schemaic_core::export::{ExportFormat, suggested_filename};
use schemaic_core::monitor::{
    ChangeKind, KindCounts, LOG_FORMATS, LogCell, action_label, kind_counts, log_columns,
    log_result_set, table_cells,
};

use crate::i18n::tr;
use crate::settings::focusable_dropdown;
use crate::theme::{font_body, font_label};
use crate::tooltip::TooltipExt;
use crate::widgets::{
    ACTION_TAB, ActionKind, TITLE_CLOSE_TAB, action_button, autohide_state, cell_text, count_pill,
    follow_after_scroll, in_ring_button, loading_dots, measure_text_px_at, modal_footer_split,
    modal_h, modal_pad_h, modal_w, panel_style, shift_hscroll, struck_text, thin_scroll,
    transition_text, with_scroll_gesture,
};
use crate::{MenuEntry, MonitorEntry, OverlayUi, PopupAnchor, TabsActions, icons, theme};

/// The log's rows are shorter than a chat bubble, so it counts as "at the bottom"
/// a little tighter than the AI panel's `follow_slack`. Scaled for the reason
/// that one is: it is compared against offsets measured in line boxes.
fn monitor_follow_slack() -> f64 {
    theme::scaled(24.0)
}

/// Modal size (fixed so the log scrolls within it). Compare results' width: a
/// column per watched column wants the room.
fn mon_w() -> f64 {
    modal_w(920.0)
}
/// The modal's height at 100% — passed through [`crate::widgets::modal_h`] at the
/// call site, which scales it and caps it against the window. See
/// `table_designer::PANEL_H` for why this comment used to claim the opposite.
const MON_H: f64 = 560.0;

/// The Tab order, top to bottom: the four filter pills, Pause, Clear, Export,
/// the interval, then Close (`ACTION_TAB`) and the title's ✕.
const PILLS_TAB: u32 = 6;
const PAUSE_TAB: u32 = 10;
const CLEAR_TAB: u32 = 11;
const EXPORT_TAB: u32 = 12;
const INTERVAL_TAB: u32 = 13;

// Column widths + shared row metrics (header and rows must agree so they align —
// which is also why they scale together).
fn time_w() -> f64 {
    theme::scaled(64.0)
}
fn act_w() -> f64 {
    theme::scaled(66.0)
}
fn col_gap() -> f64 {
    theme::scaled(24.0)
}
/// A watched column's narrowest and widest, **unscaled** — see [`ColumnWidths`].
const MIN_COL_W: f64 = 40.0;
const MAX_COL_W: f64 = 360.0;

/// One change row's **whole** vertical box, gap included.
///
/// **Decreed rather than derived**, and the row is given this height rather than
/// the `padding_vert(7)` + `gap(2)` it used to reach the same 29px with. That is
/// what [`VirtualItemSize::Fixed`] needs: `virtual_stack` places child `i` at
/// `i * item_size`, so a height computed one way for the layout and another way
/// for the stack is a list that drifts further out of place the further you
/// scroll. `blob_view`'s hex dump is the same shape for the same reason.
/// `conn_import`'s `ROW_VIEW_CAP` chose a cap over a virtual stack precisely
/// because *its* rows are two lines or three; a change row is always one, which
/// is what makes `Fixed` honest here.
///
/// **"Always one" is enforced, not observed.** The values in a change row are
/// whatever the engine stored, and a `TEXT` column holding a newline shapes to
/// two lines — so `monitor::table_cells` flattens every value through
/// `text::cell_line` on the way to `text()`, and [`entry_row`]'s empty slot is
/// given this height rather than hidden. Both are this contract, not tidiness:
/// `Fixed` is a promise about the child's height that nothing in floem checks.
fn mon_row_h() -> f64 {
    theme::scaled(29.0)
}

/// Row source for the change log's virtual stack.
///
/// **The sequence numbers, not the entries**, for the two reasons the list's own
/// comment gives: the keyed diff has to key on the entry rather than on its
/// position, because the log is a sliding window whose index set never changes;
/// and iterating `u64` keeps the whole `Vec<MonitorEntry>` from being deep-copied
/// twice per landing poll.
struct LogSeqs(Vec<u64>);

impl VirtualVector<u64> for LogSeqs {
    fn total_len(&self) -> usize {
        self.0.len()
    }

    fn slice(&mut self, range: std::ops::Range<usize>) -> impl Iterator<Item = u64> {
        let end = range.end.min(self.0.len());
        let start = range.start.min(end);
        self.0[start..end].iter().copied()
    }
}

/// Old-value tint (a delete, an update's old value) and new-value tint (an
/// insert, an update's new value) — Compare results' removed and added rows too
/// (`result_diff_view`). Theme roles, read per call so a live theme switch
/// repaints them: as fixed literals tuned for the dark panel they read at 1.9:1
/// (added) and 3.1:1 (removed) on the light one. `crate::contrast` holds both
/// at AA on `bg_panel` in every theme.
pub(crate) fn old_color() -> Color {
    theme::diff_removed_text()
}
pub(crate) fn new_color() -> Color {
    theme::diff_added_text()
}

/// A kind's colour — its Action label, its pill's dot. **A `fn() -> Color`**:
/// the rows are keyed on `entry.seq`, so a theme switch rebuilds none of them,
/// and `theme::chip_active()` resolved at build left the UPDATE labels in the
/// previous theme's colour beside INSERT and DELETE labels that repainted.
fn kind_color(kind: ChangeKind) -> fn() -> Color {
    match kind {
        ChangeKind::Insert => new_color,
        ChangeKind::Update => theme::chip_active,
        ChangeKind::Delete => old_color,
    }
}

/// The watched columns' widths, **grown as entries arrive and never shrunk**.
///
/// The rows are a virtual stack, so only the visible ones exist to be measured —
/// and a width that followed them would move every column as you scroll, the
/// header with it. So each entry is measured once, as it lands, and a column only
/// ever widens: a scrollbar reads a range that grows as more content arriving,
/// and one that shrinks as the view moving. Only a value longer by characters than the column's
/// longest so far is shaped, so a poll costs a few measurements rather than one
/// per cell.
///
/// Kept **unscaled** and scaled where a style reads them, so an interface-scale
/// change with the modal open moves the header and the rows together.
#[derive(Default)]
struct ColumnWidths {
    /// The columns these widths are for.
    heads: Vec<String>,
    /// Each column's longest value so far, in characters.
    longest: Vec<usize>,
    widths: Vec<f64>,
    /// The last entry measured.
    upto: Option<u64>,
}

impl ColumnWidths {
    /// Measure the entries not measured yet — all of them, from each column's
    /// heading up, when the columns changed or the log was cleared (its last
    /// `seq` went back). `true` when a width moved.
    fn take(&mut self, entries: &[MonitorEntry], heads: &[String], cols: &[String]) -> bool {
        let unit = theme::scaled(1.0);
        let fit = |px: f64| ((px / unit).ceil() + 2.0).clamp(MIN_COL_W, MAX_COL_W);
        let mut moved = false;
        if self.heads != heads || entries.last().map(|e| e.seq) < self.upto {
            self.heads = heads.to_vec();
            self.longest = vec![0; heads.len()];
            self.widths = heads
                .iter()
                .map(|h| fit(measure_text_px_at(h, font_label())))
                .collect();
            self.upto = None;
            moved = true;
        }
        let upto = self.upto;
        for e in entries.iter().filter(|e| upto.is_none_or(|u| e.seq > u)) {
            for (i, c) in table_cells(e, heads, cols).into_iter().enumerate() {
                let (chars, shown) = match c {
                    LogCell::Value(v) => (v.chars().count(), v),
                    LogCell::Changed(a, b) => {
                        let s = format!("{a} → {b}");
                        (s.chars().count(), s)
                    }
                    LogCell::Missing => continue,
                };
                if chars <= self.longest[i] {
                    continue;
                }
                self.longest[i] = chars;
                let w = fit(measure_text_px_at(&shown, font_body()));
                if w > self.widths[i] {
                    self.widths[i] = w;
                    moved = true;
                }
            }
        }
        if let Some(e) = entries.last() {
            self.upto = Some(e.seq);
        }
        moved
    }
}

/// Watched column `i`'s width at the current scale.
fn col_w(widths: RwSignal<Rc<Vec<f64>>>, i: usize) -> f64 {
    theme::scaled(widths.with(|w| w.get(i).copied().unwrap_or(MIN_COL_W)))
}

/// The table's full width: its padding, Time, Action, every watched column and
/// the gaps between them.
fn table_w(widths: RwSignal<Rc<Vec<f64>>>) -> f64 {
    let n = widths.with(|w| w.len());
    modal_pad_h() * 2.0
        + time_w()
        + act_w()
        + (0..n).map(|i| col_w(widths, i)).sum::<f64>()
        + col_gap() * (n + 1) as f64
}

/// What the area under the controls shows.
#[derive(Clone, PartialEq)]
enum Body {
    /// The log is empty.
    Waiting,
    /// The log holds nothing of the kind the filter picked.
    NoneOf(ChangeKind),
    /// The table, under these columns.
    Table(Vec<String>),
}

pub(crate) fn monitor_overlay(o: OverlayUi, tab_actions: Rc<TabsActions>) -> impl IntoView {
    let open = o.monitor_open;
    let title = o.monitor_title;
    let cols = o.monitor_cols;
    let log = o.monitor_log;
    let error = o.monitor_error;
    let partial = o.monitor_partial;
    let stopped = o.monitor_stopped;
    let length_only = o.monitor_length_only;
    let interval = o.monitor_interval;
    let paused = o.monitor_paused;
    let export_err = o.monitor_export_err;
    let exported = o.monitor_exported;
    let dropped = o.monitor_dropped;
    let confirm = o.confirm;
    // Where an export failure lands when the modal has already closed.
    let error_modal_text = o.error_modal_text;
    let error_modal_open = o.error_modal_open;
    // The shared popup channel — `popup_menu_overlay` is mounted last in the
    // workspace stack, so a menu raised from in here paints above this modal and
    // its backdrop rather than behind them.
    let popup_menu = o.popup_menu;
    let popup_anchor = o.popup_anchor;
    let export_file = tab_actions.export_file.clone();

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

            let filter: RwSignal<Option<ChangeKind>> = RwSignal::new(None);
            let counts = create_memo(move |_| log.with(|l| kind_counts(l)));
            // The export's columns: the watched table's, under the names each
            // entry was recorded with, so a mid-session `ALTER` moves no value
            // under another column's heading. A memo, so a poll that changes no
            // column notifies nothing.
            let heads = create_memo(move |_| log.with(|l| cols.with(|c| log_columns(l, c))));
            let widths: RwSignal<Rc<Vec<f64>>> = RwSignal::new(Rc::new(Vec::new()));
            {
                let measured = RefCell::new(ColumnWidths::default());
                create_effect(move |_| {
                    heads.with(|h| {
                        log.with(|l| {
                            cols.with(|c| {
                                let mut m = measured.borrow_mut();
                                if m.take(l, h, c) {
                                    widths.set(Rc::new(m.widths.clone()));
                                }
                            })
                        })
                    });
                });
            }

            let modal_header = title_bar(title, paused, stopped, close.clone(), ring.clone());

            // Pause / Clear / Export, left of the interval dropdown. All three are
            // in the modal's ring, in reading order, so the log is operable without
            // a pointer — a monitor is watched with both hands off the mouse.
            let pause_btn = in_ring_button(
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
                        tr!("monitor-resume")
                    } else {
                        tr!("monitor-pause")
                    };
                    text(t).style(crate::widgets::tooltip_style)
                }),
                ring.clone(),
                PAUSE_TAB,
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
                    title: tr!("monitor-clear-title"),
                    message: tr!("monitor-clear-message", count = n),
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
            let clear_btn = in_ring_button(
                crate::widgets::toolbar_icon(
                    icons::TRASH_2,
                    0.0,
                    0.0,
                    move || log.with(|l| !l.is_empty()),
                    move || (clear_click)(),
                )
                .tooltip(|| text(tr!("monitor-clear-tip")).style(crate::widgets::tooltip_style)),
                ring.clone(),
                CLEAR_TAB,
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
            let export_btn = in_ring_button(
                crate::widgets::toolbar_icon(
                    icons::DOWNLOAD,
                    0.0,
                    0.0,
                    move || log.with(|l| !l.is_empty()),
                    move || (export_click)(),
                )
                .tooltip(|| text(tr!("monitor-export-tip")).style(crate::widgets::tooltip_style)),
                ring.clone(),
                EXPORT_TAB,
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
            let buttons = h_stack((pause_btn, clear_btn, export_btn))
                .style(|s| s.flex_row().items_center().flex_shrink(0.0_f32));

            let interval_dd = container(focusable_dropdown(
                interval,
                [1u64, 2, 5, 10],
                interval_label,
                ring.clone(),
                INTERVAL_TAB,
            ))
            .style(|s| s.width(theme::scaled(84.0)).flex_shrink(0.0_f32));
            // The filter pills where the status line was — what that line said
            // is the title's badge and the footer's now.
            let controls = h_stack((
                filter_pills(counts, filter, ring.clone()),
                empty().style(|s| s.flex_grow(1.0_f32).min_width(theme::scaled(12.0))),
                buttons,
                interval_dd,
            ))
            .style(|s| {
                s.width_full()
                    .flex_row()
                    .items_center()
                    .gap(theme::scaled(10.0))
                    .padding_horiz(modal_pad_h())
                    .padding_vert(theme::scaled(12.0))
            });

            // **A memo, not the log.** `dyn_container` does not diff — it rebuilds
            // whenever a dependency fires — and `with` subscribes, so reading the log
            // here meant every poll that landed a single change tore down and rebuilt
            // the whole table: header, both scrolls and up to `LOG_CAP` rows, twice
            // cloning the log on the way. The list jumped back to the top (the new
            // scroll starts at zero and `follow` is false exactly when the reader has
            // scrolled away) and the header desynchronised from the body
            // horizontally, so reading history in a live monitor was impossible
            // except by pausing. The memo fires only when what the area *is*
            // changes: the log empties or fills, the filter picks a kind the log
            // has none of, or the columns change.
            let body = create_memo(move |_| {
                let n = counts.get();
                match filter.get() {
                    _ if n.of(None) == 0 => Body::Waiting,
                    Some(k) if n.of(Some(k)) == 0 => Body::NoneOf(k),
                    _ => Body::Table(heads.get()),
                }
            });

            let content = dyn_container(
                move || body.get(),
                move |b| {
                    let heads = match b {
                        Body::Table(heads) => Rc::new(heads),
                        Body::NoneOf(k) => {
                            let said = match k {
                                ChangeKind::Insert => tr!("monitor-no-inserts"),
                                ChangeKind::Update => tr!("monitor-no-updates"),
                                ChangeKind::Delete => tr!("monitor-no-deletes"),
                            };
                            return container(text(said).style(|s| {
                                s.color(theme::text_dim())
                                    .font_size(theme::scaled_font(13.0))
                            }))
                            .style(|s| s.size_full().items_center().justify_center())
                            .into_any();
                        }
                        // `paused` is tracked *here*, inside the empty branch, not in
                        // the outer selector: adding it there would rebuild the whole
                        // table on every pause toggle and throw away its scroll
                        // position, which is the thing someone pauses in order to read.
                        Body::Waiting => {
                            return container(dyn_container(
                                move || (paused.get(), stopped.get()),
                                move |(is_paused, is_stopped)| {
                                    // A stopped loop is waiting for nothing
                                    // either; the footer says why.
                                    if is_stopped {
                                        return text(tr!("monitor-empty-stopped"))
                                            .style(|s| {
                                                s.color(theme::text_dim())
                                                    .font_size(theme::scaled_font(13.0))
                                            })
                                            .into_any();
                                    }
                                    if is_paused {
                                        // Static, not `loading_dots`: cycling dots say
                                        // "any moment now", and a paused monitor is not
                                        // waiting for anything.
                                        return text(tr!("monitor-empty-paused"))
                                            .style(|s| {
                                                s.color(theme::text_dim())
                                                    .font_size(theme::scaled_font(13.0))
                                            })
                                            .into_any();
                                    }
                                    loading_dots(tr!("monitor-waiting"), theme::text_dim, font_body)
                                        .into_any()
                                },
                            ))
                            .style(|s| s.size_full().items_center().justify_center())
                            .into_any();
                        }
                    };
                    // Header: full-width separator bar whose labels scroll
                    // horizontally in sync with the body (reads `hscroll`; never
                    // writes it, and blocks its own wheel — the one-writer rule).
                    let table_header = container(
                        scroll(header_row(heads.clone(), widths))
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
                    // went on sliding.
                    //
                    // **And the stack iterates the sequence numbers, not the
                    // entries.** `log.get()` deep-copies the whole
                    // `Vec<MonitorEntry>` — every entry carries the watched
                    // row's full `Vec<Option<String>>` — and the stack copies
                    // it again: measured 1.19 / 2.81 / 5.34 ms per landing poll
                    // at 10 / 30 / 60 table columns, on the UI thread, at an
                    // interval as short as a second. A `Vec<u64>` at `LOG_CAP`
                    // is 8 KiB and copies in microseconds, and the row builder
                    // then clones the *one* entry it is for — which floem asks
                    // for only when the key is new. The filter picks which
                    // sequence numbers, so switching pills rebuilds nothing but
                    // the rows that come into view.
                    let seqs = create_memo(move |_| {
                        let f = filter.get();
                        log.with(|l| {
                            l.iter()
                                .filter(|e| f.is_none_or(|k| e.change.kind == k))
                                .map(|e| e.seq)
                                .collect::<Vec<u64>>()
                        })
                    });
                    // **Only the visible rows are built.** A `dyn_stack` built
                    // all of them, and at `LOG_CAP` that is 1,000 rows of
                    // fifteen-odd text views apiece: measured on MariaDB
                    // 10.11.14 against a ten-column table, the process grew from
                    // 336 MB with an empty log to 813 MB with a full one, dead
                    // linear at ~0.46 MB per row across 250/500/750/1,000.
                    // Windowed, the same full log measures 335 MB — which is
                    // also why a row may carry every watched column: a wide
                    // table costs views only in the twenty-odd rows mounted.
                    //
                    // The content width is [`ColumnWidths`]', fixed for what
                    // the log holds rather than for which rows are mounted.
                    let list = virtual_stack(
                        VirtualDirection::Vertical,
                        VirtualItemSize::Fixed(Box::new(mon_row_h)),
                        move || LogSeqs(seqs.get()),
                        |seq| *seq,
                        move |seq| entry_row(log, seq, heads.clone(), cols, widths),
                    )
                    .style(move |s| s.flex_col().min_width(table_w(widths)));
                    // **The 8px band is a wrapper's, not the stack's.**
                    // `virtual_stack` derives its window straight from
                    // `viewport.y0 / item_size` and knows nothing about the
                    // container's padding, while `compute_view_layout` hands it
                    // a viewport in its own border-box space — so padding on the
                    // stack itself makes the window short by `padding_top` and
                    // the first row visible at the top edge is never built. It
                    // also forces `height(content_size)` against taffy's
                    // border-box `size`, which would eat the 16px back out of
                    // the rows. Padding a parent leaves both exact: the
                    // viewport the stack is given is already translated into
                    // its own space.
                    let list = container(list)
                        .style(move |s| s.flex_col().padding_vert(theme::scaled(8.0)));
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
                        .style(|s| {
                            s.flex_col()
                                .flex_grow(1.0_f32)
                                .width_full()
                                .min_height(0.0)
                                .border_top(1.0)
                                .border_color(theme::border())
                        })
                        .into_any()
                },
            )
            .style(|s| s.flex_grow(1.0_f32).width_full().min_height(0.0));

            let line = create_memo(move |_| {
                log.with(|l| {
                    footer_line(
                        export_err.get(),
                        error.get(),
                        partial.get(),
                        // What was actually dropped, not what the length implies:
                        // a log resting *at* the cap has lost nothing yet.
                        dropped.get() > 0,
                        &length_only.get(),
                        l.len(),
                        l.first().map(|e| e.at.as_str()),
                    )
                })
            });
            let status = label(move || line.with(|(msg, _)| msg.clone()))
                .style(move |s| {
                    s.color(line.with(|(_, tone)| tone.color())())
                        .font_size(font_label())
                        .text_ellipsis()
                })
                // A poll error can outrun the footer; the whole of it is here.
                .tooltip(move || {
                    text(line.with(|(msg, _)| msg.clone())).style(crate::widgets::tooltip_style)
                });
            let done = close.clone();
            let footer = modal_footer_split(
                status,
                action_button(
                    // Neutral, as Compare results' is: the footer offers no
                    // decision to affirm.
                    tr!("dialog-close"),
                    ActionKind::Neutral,
                    true,
                    ring.clone(),
                    ACTION_TAB,
                    move || (done)(),
                ),
            );

            // The card is the flex_col itself (fixed height) so the scroll clips.
            let panel = v_stack((modal_header, controls, content, footer))
                .on_click_stop(|_| {})
                .style(|s| {
                    panel_style(s)
                        .width(mon_w())
                        .height(modal_h(MON_H))
                        .background(theme::bg_panel())
                        .border_color(theme::modal_border())
                });

            // **No click-away.** The change log is the one record of what a
            // deleted row held, and reopening the monitor starts a fresh log —
            // so a stray backdrop click threw it away without the confirmation
            // Clear asks for (`discard_needs_asking`). ✕, Close and Escape are
            // deliberate and stay. Nor a click handler on the focus root:
            // floem fires `Click` on the focused view for Space, the reflex for
            // "scroll this log" — see `widgets::dismiss_layer`.
            crate::widgets::focus_root_with_ring(stack((panel,)), ring)
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

/// The title badge's word and dot colour. **Stopped outranks Paused**: a loop
/// that has ended (a table with no row key) is not waiting for Resume, and
/// the badge read only `paused`, so it stayed a green *Watching* beside the
/// footer's "changes can't be tracked".
fn badge_state(paused: bool, stopped: bool) -> (String, fn() -> Color) {
    if stopped {
        (tr!("monitor-badge-stopped"), theme::error)
    } else if paused {
        (tr!("monitor-badge-paused"), theme::plan_warn)
    } else {
        (tr!("monitor-badge-watching"), new_color)
    }
}

/// The title row: *Live monitor*, the watched `database.table` in mono, a badge
/// saying whether it is *Watching*, *Paused* or *Stopped*, and the ✕ —
/// `modal_title`'s shape, which takes only a string.
fn title_bar(
    title: RwSignal<Option<String>>,
    paused: RwSignal<bool>,
    stopped: RwSignal<bool>,
    close: Rc<dyn Fn()>,
    ring: crate::widgets::FocusRing,
) -> impl IntoView {
    let table = title.get_untracked().unwrap_or_default();
    let state = move || badge_state(paused.get(), stopped.get());
    let badge = h_stack((
        // 1px low, to sit on the label's optical middle rather than its box's.
        icons::icon(icons::DOT, 6.0)
            .style(move |s| s.margin_top(theme::scaled(1.0)).color(state().1())),
        label(move || state().0),
    ))
    .style(|s| {
        s.items_center()
            .gap(theme::scaled(6.0))
            .font_size(theme::font_hint())
            .color(theme::text_dim())
            .background(theme::pill_hover_bg())
            // Half the badge's height, not a CSS-style `999`: floem does not
            // clamp a radius to the box, and 999 painted a circle round it.
            .border_radius(theme::scaled(10.0))
            .padding_horiz(theme::scaled(8.0))
            // The line box sits low in the badge: 2px more below evens it, as
            // Compare results' A/B badges do.
            .padding_top(theme::scaled(3.0))
            .padding_bottom(theme::scaled(5.0))
            .flex_shrink(0.0_f32)
    });
    let pressed = close.clone();
    h_stack((
        text(tr!("monitor-title")).style(|s| {
            s.font_size(theme::scaled_font(15.0))
                .font_bold()
                .color(theme::text())
                .flex_shrink(0.0_f32)
        }),
        text(table).style(|s| {
            s.font_family(crate::MONO_FAMILY.to_string())
                .font_size(font_label())
                .color(theme::text_dim())
                .min_width(0.0)
                .text_ellipsis()
        }),
        badge,
        empty().style(|s| s.flex_grow(1.0_f32)),
        in_ring_button(
            container(icons::icon(icons::X, 16.0))
                .on_click_stop(move |_| (close)())
                .style(|s| {
                    s.flex_shrink(0.0_f32)
                        .items_center()
                        .padding(theme::scaled(6.0))
                        .color(theme::text_dim())
                        .hover(|s| s.color(theme::text()))
                }),
            ring,
            TITLE_CLOSE_TAB,
            true,
            0.0,
            move || (pressed)(),
        ),
    ))
    .style(|s| {
        s.width_full()
            .flex_row()
            .items_center()
            .gap(theme::scaled(10.0))
            .min_width(0.0)
            .padding_horiz(modal_pad_h())
            .padding_vert(theme::scaled(10.0))
            .border_bottom(1.0)
            .border_color(theme::border())
    })
}

/// *All · Inserts · Updates · Deletes*, each with its count — Compare results'
/// pills ([`count_pill`]), a dot in each kind's colour. Tab stops `PILLS_TAB`
/// on, before Pause; Enter or Space picks.
fn filter_pills(
    counts: Memo<KindCounts>,
    filter: RwSignal<Option<ChangeKind>>,
    ring: crate::widgets::FocusRing,
) -> impl IntoView {
    let pill = move |name: String, which: Option<ChangeKind>, tabindex: u32| {
        count_pill(
            name,
            which.map(kind_color),
            move || counts.get().of(which).to_string(),
            move || filter.get() == which,
            move || {
                if filter.get_untracked() != which {
                    filter.set(which);
                }
            },
            ring.clone(),
            tabindex,
        )
    };
    h_stack((
        pill(tr!("monitor-filter-all"), None, PILLS_TAB),
        pill(
            tr!("monitor-filter-inserts"),
            Some(ChangeKind::Insert),
            PILLS_TAB + 1,
        ),
        pill(
            tr!("monitor-filter-updates"),
            Some(ChangeKind::Update),
            PILLS_TAB + 2,
        ),
        pill(
            tr!("monitor-filter-deletes"),
            Some(ChangeKind::Delete),
            PILLS_TAB + 3,
        ),
    ))
    .style(|s| {
        s.items_center()
            .gap(theme::scaled(4.0))
            .flex_shrink(0.0_f32)
    })
}

/// The column header row — Time, Action, then each watched column — sized to
/// match [`entry_row`]'s columns so they line up.
fn header_row(heads: Rc<Vec<String>>, widths: RwSignal<Rc<Vec<f64>>>) -> impl IntoView {
    // The width arrives as a closure, resolved inside the style — the rows call
    // the same functions in *their* style closures, so a header that captured the
    // numbers at build stopped sitting over its own columns the moment the
    // interface scale moved them.
    let cell = |t: String, w: Rc<dyn Fn() -> f64>| {
        text(t)
            .style(move |s| {
                s.color(theme::text_dim())
                    .font_size(font_label())
                    .width(w())
                    .flex_shrink(0.0_f32)
                    .min_width(0.0)
                    .text_ellipsis()
            })
            .into_any()
    };
    let mut cells = vec![
        cell(tr!("monitor-col-time"), Rc::new(time_w)),
        cell(tr!("monitor-col-action"), Rc::new(act_w)),
    ];
    cells.extend(
        heads
            .iter()
            .enumerate()
            .map(|(i, h)| cell(h.clone(), Rc::new(move || col_w(widths, i)))),
    );
    h_stack_from_iter(cells).style(move |s| {
        s.items_center()
            .gap(col_gap())
            .padding_horiz(modal_pad_h())
            .height(theme::scaled(34.0))
            .min_width(table_w(widths))
    })
}

/// One change row: its time, its kind, then a cell per watched column — an
/// insert's values green, a delete's red and struck through, an update's
/// changed values `old → new` beside the rest in `text_dim`.
///
/// **Takes the log and a sequence number rather than the entry**, so the list
/// above can iterate 8 KiB of `u64` instead of deep-copying every entry's cells
/// twice per landing poll. The log is appended in ascending `seq` and never
/// reordered, so the lookup is a binary search; a `seq` the log no longer holds
/// (the window slid while this was being built) renders blank — which is what
/// the row would have shown anyway.
///
/// **Blank at full height, not `widgets::nothing()`.** `nothing()` is
/// `display: none`, so it occupies zero rows; under a
/// `VirtualItemSize::Fixed` stack, which places child `i` at `i * item_size`
/// regardless, that puts every later row in the window 29 px above where the
/// stack has reserved space for it. Whatever this slot is, it is one row tall —
/// see [`mon_row_h`].
fn entry_row(
    log: RwSignal<Vec<MonitorEntry>>,
    seq: u64,
    heads: Rc<Vec<String>>,
    cols: RwSignal<Vec<String>>,
    widths: RwSignal<Rc<Vec<f64>>>,
) -> AnyView {
    let Some(entry) = log.with_untracked(|l| {
        l.binary_search_by_key(&seq, |e| e.seq)
            .ok()
            .map(|i| l[i].clone())
    }) else {
        return empty().style(|s| s.height(mon_row_h())).into_any();
    };
    let kind = entry.change.kind;
    let color = kind_color(kind);
    // **Under the names the entry was recorded with**, not the modal's current
    // list: a baseline restart answering an `ALTER` rewrites `monitor_cols`
    // mid-session while the log keeps entries indexed against the previous
    // list, so reading the signal here named every earlier change's field
    // wrong — the same defect the export had, on screen. `cols` is the
    // fallback, for a log written before entries carried their own.
    let cells = cols.with_untracked(|c| table_cells(&entry, &heads, c));
    let mut views: Vec<AnyView> = vec![
        text(entry.at)
            .style(|s| {
                s.color(theme::text_dim())
                    .font_size(font_label())
                    .width(time_w())
                    .flex_shrink(0.0_f32)
            })
            .into_any(),
        text(action_label(kind))
            .style(move |s| {
                s.color(color())
                    .font_size(font_label())
                    .font_bold()
                    .width(act_w())
                    .flex_shrink(0.0_f32)
            })
            .into_any(),
    ];
    for (i, c) in cells.into_iter().enumerate() {
        let content: AnyView = match (c, kind) {
            (LogCell::Missing, _) => empty().into_any(),
            (LogCell::Changed(old, new), _) => {
                transition_text(old, new, old_color, new_color).into_any()
            }
            (LogCell::Value(v), ChangeKind::Insert) => cell_text(v, new_color).into_any(),
            (LogCell::Value(v), ChangeKind::Delete) => struck_text(v, old_color).into_any(),
            (LogCell::Value(v), ChangeKind::Update) => cell_text(v, theme::text_dim).into_any(),
        };
        views.push(
            container(content)
                .style(move |s| {
                    s.width(col_w(widths, i))
                        .flex_shrink(0.0_f32)
                        .min_width(0.0)
                        .items_center()
                })
                .clip()
                .into_any(),
        );
    }
    h_stack_from_iter(views)
        .style(|s| {
            // **A height**, stated once — see [`mon_row_h`] for why the virtual
            // stack cannot be trusted with a height it did not decide.
            // `items_center` is what puts the line in the middle of it.
            s.items_center()
                .gap(col_gap())
                .padding_horiz(modal_pad_h())
                .height(mon_row_h())
        })
        .into_any()
}

/// How loudly the footer's line should read. Separate from the colour so the
/// line itself is a pure, testable decision — the colour is looked up through
/// [`Tone::color`], which hands back a `fn() -> Color` rather than a `Color`
/// because it is read inside a reactive style (see the themable-colour
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

/// What the footer says, and how loudly: *"200 events since 11:48:12 · newest
/// at the bottom"*, `since` being the oldest entry the log still holds.
///
/// Errors take the line outright, worst-to-mislead first: a **failed export**
/// beats everything, because the user believes they have a file and doesn't; a
/// **poll error** beats the rest, because nothing below it is true while polling
/// is broken.
///
/// Otherwise the count, then every caveat that currently applies, because they
/// *co-occur* and each one is a different reason the log isn't the whole story.
/// `partial` and `capped` are the two silent truncations — rows the poll can't
/// see, and changes the log has already dropped — and the second only became
/// load-bearing when the log became exportable. `length_only` is the third:
/// binary columns, whose changes a poll sees only when their length moves.
/// Whether the poll is paused or stopped is the title's badge, not this line.
fn footer_line(
    export_err: Option<String>,
    poll_err: Option<String>,
    partial: bool,
    capped: bool,
    length_only: &[String],
    events: usize,
    since: Option<&str>,
) -> (String, Tone) {
    if let Some(msg) = export_err.or(poll_err) {
        return (msg, Tone::Error);
    }
    let mut parts = vec![match since {
        Some(t) if events > 0 => tr!("monitor-events-since", count = events, since = t),
        _ => tr!("monitor-no-changes"),
    }];
    if partial {
        parts.push(tr!(
            "monitor-partial",
            cap = schemaic_core::monitor::ROW_CAP
        ));
    }
    if capped {
        parts.push(tr!("monitor-capped", cap = schemaic_core::monitor::LOG_CAP));
    }
    // A blob's cell is `<n bytes>`: rewritten at the same length, it is no
    // change to the poll.
    if !length_only.is_empty() {
        parts.push(tr!(
            "monitor-length-only",
            count = length_only.len(),
            columns = length_only.join(", ")
        ));
    }
    if parts.len() > 1 {
        return (parts.join(" · "), Tone::Warn);
    }
    if events > 0 {
        parts.push(tr!("monitor-newest-at-bottom"));
    }
    (parts.join(" · "), Tone::Calm)
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
    fallback_err: RwSignal<Option<schemaic_core::model::ModalError>>,
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
        .title(tr!("monitor-export-dialog"))
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
                more: Vec::new(),
                dialect: Default::default(),
                // The log is the rows in hand by definition — it is a record of
                // what the monitor saw, not a query anything could re-run.
                scope: crate::ExportScope::Fetched,
                // **Nothing here offers a Stop**, so this run stays out of the
                // app's single cancel slot — see `ExportRequest::stoppable`. It
                // is not a statement about size: the log is small, but the reason
                // is that there is no control bound to it, and a run in the slot
                // would refuse the grid's export (and be refused by it) over a
                // window the user cannot see.
                stoppable: false,
            },
            // `try_update`: the modal may have closed while the dialog was open
            // or the write ran, and a plain `set` would panic on a freed signal.
            Rc::new(move |outcome| match outcome {
                // The log on screen now has a copy on disk, which is what lets
                // Clear stop asking (`monitor::discard_needs_asking`).
                crate::ExportOutcome::Done(_) => {
                    exported.try_update(|v| *v = true);
                }
                // This cannot arrive, but **not for the reason it used to be**:
                // "a log export is never streamed" stopped being the reason when
                // the `Fetched` render was chunked and became stoppable between
                // blocks. What keeps it unreachable now is that nothing here
                // offers a Stop, which is exactly what `stoppable: false` above
                // says — so no token this run watches is ever cancelled.
                //
                // Spelled out rather than folded in with `Done` because the day
                // that changes, marking the log exported off a half-written file
                // is exactly the wrong answer: Clear would stop asking about a
                // log that was never fully saved.
                crate::ExportOutcome::Cancelled => {}
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
                //
                // `partial` is not read here for the same reason: the message is
                // taken as produced, and the truncated file it warns about is
                // already what the sentence above is about.
                crate::ExportOutcome::Failed { message: e, .. } => {
                    if open.try_get_untracked() == Some(true) {
                        export_err.try_update(|v| *v = Some(e));
                    } else {
                        // A file that couldn't be written — Schemaic's own
                        // report, so the modal offers no "Explain".
                        fallback_err
                            .try_update(|v| *v = Some(schemaic_core::model::ModalError::app(e)));
                        fallback_open.try_update(|v| *v = true);
                    }
                }
            }),
        );
    });
}

/// Poll-interval option labels for the dropdown.
fn interval_label(secs: u64) -> String {
    match secs {
        1 | 2 | 5 | 10 => tr!("monitor-interval", secs = secs),
        _ => "—".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Clear prompt and the interval dropdown read as they did in English
    /// before the catalogue — the footer's line and the badge have tests of
    /// their own below.
    #[test]
    fn the_clear_prompt_and_the_intervals_read_as_they_did_in_english() {
        let tail = "would be discarded. The log is the only record of what a deleted row held \
                    — the poll will not report a change again — and this can't be undone. \
                    Export it first if you want to keep it.";
        assert_eq!(
            tr!("monitor-clear-message", count = 1),
            format!("1 change {tail}")
        );
        assert_eq!(
            tr!("monitor-clear-message", count = 40),
            format!("40 changes {tail}")
        );
        let labels: Vec<String> = [1, 2, 5, 10, 3].into_iter().map(interval_label).collect();
        assert_eq!(labels, ["1s", "2s", "5s", "10s", "—"]);
    }

    /// **The virtual stack may ask for rows the log no longer has.**
    ///
    /// The log is a sliding window that a landing poll shortens from the front
    /// and lengthens at the back, and `virtual_stack` computes the range it
    /// wants from the viewport it last laid out — so a range whose end is past
    /// `total_len` is an ordinary race, not a bug in the caller. Indexing a
    /// slice with it panics, and a panic in a layout pass takes the window with
    /// it. Clamping is what the `slice` here does about that, and it is the one
    /// thing in `LogSeqs` worth asserting: the rest of the type is a length and
    /// a copy.
    #[test]
    fn a_range_past_the_end_of_the_log_is_clamped_rather_than_panicking() {
        let mut seqs = LogSeqs(vec![7, 8, 9]);
        assert_eq!(seqs.total_len(), 3);
        assert_eq!(seqs.slice(0..3).collect::<Vec<_>>(), vec![7, 8, 9]);
        assert_eq!(seqs.slice(1..2).collect::<Vec<_>>(), vec![8]);
        // Past the end in one direction, then in both.
        assert_eq!(seqs.slice(1..9).collect::<Vec<_>>(), vec![8, 9]);
        assert!(seqs.slice(5..9).collect::<Vec<_>>().is_empty());
        // And the empty log, which is every session's first frame and the state
        // Clear returns it to.
        let mut none = LogSeqs(Vec::new());
        assert_eq!(none.total_len(), 0);
        assert!(none.slice(0..20).collect::<Vec<_>>().is_empty());
    }

    /// `footer_line` with no error, 200 events since 11:48:12 — the two
    /// truncation flags are what most of these vary.
    fn line(partial: bool, capped: bool) -> (String, Tone) {
        footer_line(None, None, partial, capped, &[], 200, Some("11:48:12"))
    }

    /// A stopped loop is not waiting for anything: its badge says so, ahead of
    /// a pause, and never in the live green.
    #[test]
    fn a_stopped_monitor_never_reads_as_watching() {
        assert_eq!(badge_state(false, false).0, "Watching");
        assert_eq!(badge_state(true, false).0, "Paused");
        for paused in [false, true] {
            let (word, color) = badge_state(paused, true);
            assert_eq!(word, "Stopped");
            assert_eq!(color(), theme::error());
            assert_ne!(color(), new_color(), "never the live green");
        }
    }

    /// Binary columns are seen by length only, and the footer says which.
    #[test]
    fn a_binary_column_is_said_to_be_watched_by_length_only() {
        let (msg, tone) = footer_line(
            None,
            None,
            false,
            false,
            &["photo".to_string()],
            3,
            Some("10:00:00"),
        );
        assert_eq!(tone, Tone::Warn);
        assert!(
            msg.ends_with("photo is binary, so its changes are seen by length only"),
            "{msg}"
        );
        let (two, _) = footer_line(
            None,
            None,
            false,
            false,
            &["a".to_string(), "b".to_string()],
            0,
            None,
        );
        assert!(two.contains("a, b are binary"), "{two}");
    }

    #[test]
    fn a_calm_log_counts_its_events_since_the_oldest() {
        assert_eq!(
            line(false, false),
            (
                "200 events since 11:48:12 · newest at the bottom".to_string(),
                Tone::Calm
            )
        );
        let (one, _) = footer_line(None, None, false, false, &[], 1, Some("09:00:00"));
        assert_eq!(one, "1 event since 09:00:00 · newest at the bottom");
    }

    #[test]
    fn an_empty_log_says_nothing_has_changed() {
        assert_eq!(
            footer_line(None, None, false, false, &[], 0, None),
            ("No changes yet".to_string(), Tone::Calm)
        );
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
        let (msg, tone) = footer_line(
            Some(real.into()),
            Some("connection lost".into()),
            true,
            true,
            &[],
            5,
            Some("10:00:00"),
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
        let (msg, tone) = footer_line(
            None,
            Some("connection lost".into()),
            true,
            true,
            &[],
            5,
            Some("10:00:00"),
        );
        assert_eq!(tone, Tone::Error);
        assert_eq!(msg, "connection lost");
    }

    #[test]
    fn a_partial_window_names_the_row_cap() {
        let (msg, tone) = line(true, false);
        assert_eq!(tone, Tone::Warn);
        assert!(msg.starts_with("200 events since 11:48:12 · "), "{msg}");
        assert!(
            msg.contains(&schemaic_core::monitor::ROW_CAP.to_string()),
            "{msg}"
        );
    }

    #[test]
    fn a_full_log_says_the_oldest_are_dropping() {
        // The caveat that makes an export honest: a log at its cap has already
        // lost entries, and a file written from it looks complete.
        let (msg, tone) = line(false, true);
        assert_eq!(tone, Tone::Warn);
        assert!(msg.contains("oldest are dropping"), "{msg}");
        assert!(
            msg.contains(&schemaic_core::monitor::LOG_CAP.to_string()),
            "{msg}"
        );
    }

    #[test]
    fn co_occurring_caveats_are_both_said() {
        // These are different reasons the log isn't the whole story, so neither
        // replaces the other.
        let (msg, tone) = line(true, true);
        assert_eq!(tone, Tone::Warn);
        assert!(msg.contains("by primary key are watched"), "{msg}");
        assert!(msg.contains("oldest are dropping"), "{msg}");
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
