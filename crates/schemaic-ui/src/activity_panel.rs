//! The Server Activity panel: the sessions currently connected to the active
//! connection's server, what each is running, and who is blocking whom — on the
//! right column, same chrome as the AI / History / Terminal panels.
//!
//! Each row carries a state dot, the session id + `user@host`, how long it has
//! been in that state, its statement (monospace, wrapped to ~3 lines then
//! clipped), and a note when it is waiting on a lock or holding one someone else
//! wants. Right-clicking a row offers to cancel its statement or terminate it
//! outright, both behind the shared confirm.
//!
//! Above the list: a counts line, and — when a lock wait is in progress — a
//! banner naming the waiter, the holder, and a Kill button for the holder. The
//! banner is the reason the panel exists: the list already contains it, but
//! twenty rows down a list that reorders under you.
//!
//! **Everything the panel decides lives in [`schemaic_core::activity`]** — the
//! ordering, the counts, which wait is worth a banner, what a kill will do. This
//! file paints it. The polling timer and the fetch are the app's
//! ([`ActivityActions`](crate::ActivityActions)).

use std::rc::Rc;
use std::time::Duration;

use floem::kurbo::Point;
use floem::prelude::*;
use floem::reactive::create_effect;

use schemaic_core::activity::{self, KillKind, SessionInfo, SessionState};
use schemaic_core::text::plural;

use crate::consts::{MONO_FAMILY, SEARCH_DEBOUNCE_MS};
use crate::theme::{FONT_BODY, FONT_LABEL};
use crate::widgets::{
    MenuEntry, autohide, debounced, highlight_mono, highlight_text, section_title, toolbar_icon,
    tooltip_style,
};
use crate::{ActivityState, FieldCfg, Ui, edit_field, icons, theme};

/// What the panel says on a SQLite connection. Only reachable by a connection
/// switch or a restored layout — the footer toggle and the palette both decline
/// to open it there ([`schemaic_core::activity::supports_activity`]).
const SQLITE_HAS_NO_SESSIONS: &str = "SQLite has no server sessions — it runs inside \
     this process, so there is nothing else connected to see.";

/// The dot beside a session id, and the colour its state word is written in.
///
/// A `fn() -> Color` rather than a `Color`, because it is read inside reactive
/// styles and a colour captured at build time freezes at the theme that was
/// active then (docs/architecture.md → *Themable colors reach reactive styles as
/// `fn() -> Color`*).
fn state_color(state: SessionState) -> fn() -> floem::peniko::Color {
    match state {
        SessionState::Blocked => theme::error,
        SessionState::IdleInTx => theme::status_warn,
        SessionState::Running => theme::status_ok,
        SessionState::Idle => theme::text_muted,
    }
}

pub(crate) fn activity_panel(ui: Ui) -> impl IntoView {
    let right_w = ui.layout.right_w;
    let state = ui.activity.state;
    let interval = ui.activity.interval;
    let busy = ui.activity.busy;
    let menu_open = ui.activity.menu_open;
    let menu_anchor = ui.activity.menu_anchor;
    let refresh = ui.activity_actions.refresh.clone();
    let kill = ui.activity_actions.kill.clone();
    let overlay = ui.overlay;

    // Panel-local search filter, debounced like the history panel's so a burst of
    // typing re-filters once. Local to this panel build.
    let search_input = RwSignal::new(String::new());
    let search = debounced(search_input, Duration::from_millis(SEARCH_DEBOUNCE_MS));

    let title_row = {
        let refresh = refresh.clone();
        // A refresh that is already in flight is inert rather than queued: the
        // poll timer is usually the one asking, and stacking manual fetches on a
        // slow server is how a panel about load becomes load.
        let refresh_btn = toolbar_icon(
            icons::REFRESH_CW,
            5.0,
            2.0,
            move || !busy.get(),
            move || (refresh)(),
        )
        .tooltip(|| text("Refresh now").style(tooltip_style));
        let clock = interval_button(interval, menu_open, menu_anchor, overlay);
        let icons_group = h_stack((refresh_btn, clock))
            .style(|s| s.flex_row().items_start().flex_shrink(0.0_f32));
        h_stack((section_title("SERVER ACTIVITY"), icons_group))
            .style(|s| s.width_full().flex_row().items_start().justify_between())
    };

    // Counts + the lock-wait banner, both derived from the *whole* snapshot rather
    // than the filtered view: they are facts about the server, and narrowing the
    // list on screen must not make a blocked session disappear from the tally.
    let header = dyn_container(move || state.get(), {
        let kill = kill.clone();
        move |st| match st {
            ActivityState::Loaded {
                sessions,
                truncated,
            } => {
                let counts = counts_line(&sessions, truncated).into_any();
                let warn = banner(&sessions, kill.clone());
                v_stack((counts, warn))
                    .style(|s| s.width_full().flex_col())
                    .into_any()
            }
            _ => empty().into_any(),
        }
    })
    .style(|s| s.width_full().flex_shrink(0.0_f32));

    // A refused kill, above the list it did **not** replace. Its own container so
    // a snapshot arriving underneath doesn't rebuild it and it doesn't rebuild the
    // snapshot — the two are independent facts.
    let kill_error = ui.activity.kill_error;
    let kill_error_line = dyn_container(
        move || kill_error.get(),
        move |e| match e {
            Some(msg) => list_message(msg, theme::error),
            None => empty().into_any(),
        },
    )
    .style(|s| s.width_full().flex_shrink(0.0_f32));

    let list = dyn_container(
        move || (state.get(), search.get()),
        move |(st, q)| {
            let term = {
                let t = q.trim();
                (!t.is_empty()).then(|| t.to_string())
            };
            let sessions = match &st {
                ActivityState::Loaded { sessions, .. } => sessions,
                // No snapshot: say which kind of nothing this is.
                ActivityState::Unsupported => {
                    return list_message(SQLITE_HAS_NO_SESSIONS, theme::text_muted);
                }
                ActivityState::Failed(e) => return list_message(e.clone(), theme::error),
                // `Loading` and `Idle` both mean "ask again in a moment"; the
                // first fetch of a panel that just opened is fast enough that two
                // different messages would only ever flicker.
                _ => return list_message("Loading…", theme::text_muted),
            };
            let matched = sessions
                .iter()
                .filter(|s| activity::matches_query(s, &q))
                .cloned()
                .collect::<Vec<_>>();
            if matched.is_empty() {
                return list_message(
                    if term.is_some() {
                        "No matching sessions."
                    } else {
                        // The fetch excludes our own connection, so an empty list
                        // really does mean nobody else is here.
                        "No other sessions on this server."
                    },
                    theme::text_muted,
                );
            }
            // **Only the front of the list becomes views.** This whole container
            // is rebuilt on every poll — two to thirty seconds apart — so each row
            // is a teardown and a re-layout, text shaping included, and five
            // hundred of them is continuous churn in a panel whose subject is
            // load. `prepare` has already sorted what deserves attention to the
            // top; the rest is reachable through the search box and is counted
            // below rather than silently dropped.
            let (rows, undrawn) = activity::render_slice(&matched);
            let kill = kill.clone();
            // One clone per row, into the view that owns it — the row and its
            // context menu share it rather than taking a copy each.
            let mut items = rows
                .iter()
                .map(|s| session_row(Rc::new(s.clone()), term.clone(), kill.clone(), overlay))
                .collect::<Vec<_>>();
            if undrawn > 0 {
                items.push(list_message(
                    format!(
                        "{undrawn} more {} — narrow the search to see {}.",
                        plural(undrawn, "session matches", "sessions match"),
                        plural(undrawn, "it", "them")
                    ),
                    theme::text_faint,
                ));
            }
            v_stack_from_iter(items)
                .style(|s| s.flex_col().width_full())
                .into_any()
        },
    )
    .style(|s| s.flex_col().width_full());

    let scrolled =
        autohide(scroll(list)).style(|s| s.flex_grow(1.0_f32).width_full().min_height(0.0));

    v_stack((
        title_row,
        // The same 5px above / 10px below the search box as the schema and
        // history panels; spacers rather than margins so the flex-grow scroll's
        // height stays exact (a sibling's vertical margin isn't subtracted → the
        // list overflows the panel).
        empty().style(|s| s.height(5.0).flex_shrink(0.0_f32)),
        edit_field(
            search_input,
            FieldCfg {
                placeholder: "Search…",
                background: theme::bg_chrome,
                clearable: true,
                ..Default::default()
            },
        )
        .style(|s| s.margin_left(12.0).margin_right(12.0).flex_shrink(0.0_f32)),
        empty().style(|s| s.height(10.0).flex_shrink(0.0_f32)),
        header,
        kill_error_line,
        scrolled,
    ))
    .style(move |s| {
        s.width(right_w.get())
            .flex_shrink(0.0_f32)
            .height_full()
            .flex_col()
            .background(theme::bg_panel())
            .border_left(1.0)
            .border_color(theme::border())
    })
}

/// A status line where the list would be — empty, loading, failed, or an engine
/// with no sessions.
///
/// Top-left rather than centred, and deliberately not `widgets::centered_msg`:
/// this sits *inside* the scroll, where a `flex_grow` has no height to grow into,
/// so a "centred" message would only ever be a message with a large left margin.
/// The History panel's empty state stands in the same place.
fn list_message(
    msg: impl Into<String>,
    color: impl Fn() -> floem::peniko::Color + 'static,
) -> floem::AnyView {
    text(msg.into())
        .style(move |s| {
            // `width_full()` is what makes it *wrap*: a label sizes its own node
            // to the text it measured, so with nothing to be 100% *of*, the
            // SQLite explanation would be one long line the panel scrolls
            // sideways for.
            s.font_size(14.0)
                .color(color())
                .width_full()
                .line_height(1.4)
                .padding_top(10.0)
                .padding_horiz(12.0)
        })
        .into_any()
}

/// The clock in the title bar — opens the poll-interval menu.
///
/// **The same light grey as the refresh icon beside it**, not a state light. It
/// was tinted while polling, on the reasoning that "Off" and "every 2s" look
/// identical between ticks; but two icons a few pixels apart wearing different
/// colours reads as one of them being *active* in the toggle sense, which it
/// isn't. The interval is stated where a state belongs — the menu marks the
/// chosen row, and the tooltip says it in words.
///
/// The menu itself is `overlays::activity_menu_overlay`, at the root: this panel
/// is clipped for its collapse animation, so a dropdown drawn here would be cut
/// off at the panel's own edge. All this does is publish where the icon is and
/// flip the flag.
fn interval_button(
    interval: floem::reactive::Memo<u64>,
    menu_open: RwSignal<bool>,
    menu_anchor: RwSignal<Point>,
    overlay: crate::OverlayUi,
) -> impl IntoView {
    let hov = RwSignal::new(false);
    // The icon's bottom-right corner in window coordinates, which is what the
    // right-aligned menu hangs off. Recomputed from both halves so a panel resize
    // (which moves the icon without resizing it) still repoints the menu.
    let origin = RwSignal::new(Point::ZERO);
    let size = RwSignal::new((0.0_f64, 0.0_f64));
    create_effect(move |_| {
        let (o, (w, h)) = (origin.get(), size.get());
        menu_anchor.set(Point::new(o.x + w, o.y + h));
    });
    // **The style goes on before the tooltip, and that is load-bearing.**
    // `.tooltip()` wraps the view in a new one, so a `.style()` after it lands on
    // the *wrapper* while `on_move`/`on_resize` stay on the container inside —
    // which then reported a bare 16px glyph box with none of the padding around
    // it, and the menu hung 3px under that instead of under the control. The
    // schema eye's menu sits 3px under its **padded** box, and matching that is
    // the whole point of anchoring this the same way.
    let button = container(icons::icon(icons::CLOCK, 16.0).style(move |s| {
        s.flex_shrink(0.0_f32)
            .color(crate::widgets::menu_icon_color(menu_open.get(), hov.get()))
    }))
    .on_move(move |p| origin.set(p))
    .on_resize(move |r| size.set((r.width(), r.height())))
    .on_click_stop(move |_| {
        // Toggle, not open: clicking the icon of an open menu closes it, the way
        // the schema tree's eye and gear do. Opening unconditionally left the only
        // way out a click somewhere else.
        overlay.context_menu.set(None);
        overlay.popup_menu.set(None);
        menu_open.update(|o| *o = !*o);
    })
    // The root closes every open menu on pointer-down; this stops that from
    // firing first and turning the click above back into an open.
    .on_event_stop(
        floem::event::EventListener::PointerDown,
        crate::widgets::menu_trigger_press,
    )
    .on_event_cont(floem::event::EventListener::PointerEnter, move |_| {
        hov.set(true)
    })
    .on_event_cont(floem::event::EventListener::PointerLeave, move |_| {
        hov.set(false)
    })
    .style(|s| {
        s.items_center()
            .margin_top(5.0)
            .margin_right(7.0)
            .padding_horiz(5.0)
            .padding_vert(3.0)
            .cursor(floem::style::CursorStyle::Default)
    });

    button.tooltip(move || {
        let secs = interval.get();
        let msg = if secs == 0 {
            "Auto-refresh off".to_string()
        } else {
            format!("Auto-refresh every {secs}s")
        };
        text(msg).style(tooltip_style)
    })
}

/// `18 sessions   4 running   1 blocked   2 idle in txn` — each figure in the
/// colour its state wears on the rows below, spaced rather than punctuated.
///
/// A zero is left out rather than printed: `0 blocked` in warning red on a
/// healthy server is an alarm about nothing, and four such zeros would be the
/// permanent state of the line.
fn counts_line(sessions: &[SessionInfo], truncated: bool) -> impl IntoView {
    let sum = activity::summarize(sessions);
    let mut parts: Vec<floem::AnyView> = vec![
        text(sum.total_label(truncated))
            .style(|s| s.font_size(FONT_LABEL).color(theme::text_muted()))
            .into_any(),
    ];
    /// One figure in the counts line: how many, what to call them, and the
    /// colour that state wears on the rows below.
    type Figure = (usize, &'static str, fn() -> floem::peniko::Color);
    let figures: [Figure; 3] = [
        (sum.running, "running", theme::status_ok),
        (sum.blocked, "blocked", theme::error),
        (sum.idle_in_tx, "idle in txn", theme::status_warn),
    ];
    for (n, word, color) in figures {
        if n == 0 {
            continue;
        }
        parts.push(
            text(format!("{n} {word}"))
                .style(move |s| s.font_size(FONT_LABEL).color(color()))
                .into_any(),
        );
    }
    // The cap is a caveat about the list, not a figure about the server, so it
    // reads as a sentence fragment rather than joining the tally.
    //
    // **And it carries no number**, because there isn't one to carry: the fetch
    // stops at `MAX_SESSIONS + 1` rows, so the only thing anyone downstream can
    // count is the single row that proved there were more. Rendering that as
    // "1 more not shown" in front of a server holding four thousand sessions was
    // a figure that looked precise and was off by three and a half thousand.
    // Saying the list is capped is the whole of what this side of the wire knows.
    if truncated {
        parts.push(
            text("list capped")
                .style(|s| s.font_size(FONT_LABEL).color(theme::text_faint()))
                .into_any(),
        );
    }
    floem::views::stack_from_iter(parts).style(|s| {
        s.flex_row()
            .items_center()
            .width_full()
            .gap(12.0)
            .padding_horiz(12.0)
            .padding_bottom(9.0)
    })
}

/// The lock-wait banner, or nothing when no session is waiting.
///
/// It offers exactly one action — terminate the holder — because that is the one
/// that ends the wait. Cancelling the holder's *statement* does not: an
/// idle-in-transaction holder has no statement, and one that does keeps its locks
/// until the transaction ends either way.
fn banner(sessions: &[SessionInfo], kill: Rc<dyn Fn(i64, KillKind)>) -> floem::AnyView {
    let Some((waiter, holder)) = activity::lock_wait(sessions) else {
        return empty().into_any();
    };
    let sentence = activity::lock_wait_text(waiter, holder);
    let holder_id = holder.id;
    let heading = h_stack((
        icons::icon(icons::LOCK, 13.0)
            .style(|s| s.color(theme::status_warn()).flex_shrink(0.0_f32)),
        text("Lock wait").style(|s| {
            s.font_size(FONT_LABEL)
                .font_bold()
                .color(theme::status_warn())
        }),
    ))
    .style(|s| s.flex_row().items_center().gap(7.0));

    let body = text(sentence).style(|s| {
        s.font_size(FONT_BODY)
            .color(theme::text())
            .line_height(1.4)
            .width_full()
    });

    // The button carries the id it will kill. A button labelled just "Kill" on a
    // panel where several sessions are killable is the one misread that can't be
    // undone.
    //
    // Filled in the danger colour, and built here rather than through
    // `widgets::action_button`: that family is the modal-footer one and requires
    // a `FocusRing` this panel has none of. Same fill, hover and radius, like the
    // header's ring-less Retry.
    let kill_btn = text(format!("Kill {holder_id}"))
        .on_click_stop(move |_| (kill)(holder_id, KillKind::Session))
        .style(|s| {
            s.font_size(FONT_BODY)
                .background(theme::btn_danger())
                .color(theme::btn_danger_text())
                .padding_horiz(10.0)
                .padding_vert(4.0)
                .border_radius(5.0)
                .flex_shrink(0.0_f32)
                .cursor(floem::style::CursorStyle::Default)
                .hover(|s| s.background(theme::btn_danger_hover()))
        });

    v_stack((
        heading,
        body,
        h_stack((kill_btn,)).style(|s| s.width_full()),
    ))
    // **No `width_full()` here.** The card carries `margin_horiz(12)`, and a width
    // of 100% resolves to the full content box with the margins added *outside*
    // it — 24px wider than there is room for, so the right border and everything
    // near it fell off the panel's clip. A column-flex child stretches to fill the
    // cross axis on its own, and stretching does subtract the margins.
    .style(|s| {
        s.flex_col()
            .gap(7.0)
            .margin_horiz(12.0)
            .margin_bottom(10.0)
            .padding_horiz(10.0)
            .padding_vert(9.0)
            .background(theme::bg_editor())
            .border(1.0)
            .border_color(theme::status_warn())
            .border_radius(6.0)
    })
    .into_any()
}

/// One session row: the dot + id + account + age, its statement, then the state
/// word with the database, and a note when it is part of a lock wait.
///
/// Left-click does nothing on purpose. Every action this row offers ends someone
/// else's work, and none of them belongs one stray click away — they live in the
/// right-click menu, the same place the schema tree keeps Drop.
fn session_row(
    s: Rc<SessionInfo>,
    term: Option<String>,
    kill: Rc<dyn Fn(i64, KillKind)>,
    overlay: crate::OverlayUi,
) -> floem::AnyView {
    let color = state_color(s.state);
    // The identity group, then the age hard against the right edge.
    //
    // **Two children and `justify_between`**, not four children and a `flex_grow`
    // spacer. The age is the one thing in this panel that has to line up with
    // something — the dot's 12px on the left — and `space-between` states that
    // directly: the last child's right edge *is* the content box's. A spacer
    // arrives at the same place only if every base size in the row measured the
    // way you expect, and through two attempts the age sat ~20px short of the
    // padding while the statement beneath it clipped at exactly the right place.
    let identity = h_stack((
        icons::icon(icons::DOT, 6.0)
            .style(move |s| s.color(color()).flex_shrink(0.0_f32).margin_right(7.0)),
        text(s.id.to_string()).style(|s| {
            s.font_size(FONT_BODY)
                .font_family(MONO_FAMILY.to_string())
                .color(theme::text_dim())
                .flex_shrink(0.0_f32)
                .margin_right(7.0)
        }),
        // **Both of these grow**, and that is not cosmetic. Nesting the account
        // name one level deeper made taffy measure the group at min-content on
        // some pass, and a rich text re-wraps itself to whatever width it is
        // handed and then *reports the wrapped size* — which is narrower, so the
        // next pass hands it the same small width and it stays wrapped.
        // `schemaic@localhost` broke mid-word with half the row empty beside it.
        // Growing both means the name is always handed the room that is actually
        // there, which clears the wrap; `min_width(0)` keeps the real case — a
        // name genuinely too long for the row — shrinking rather than shoving the
        // age off the edge.
        highlight_text(s.who(), term.clone(), FONT_BODY, theme::text, false, 1.0)
            .style(|s| s.flex_grow(1.0_f32).min_width(0.0)),
    ))
    .style(|s| s.items_center().flex_grow(1.0_f32).min_width(0.0));

    let heading = h_stack((
        identity,
        text(activity::format_age(s.seconds)).style(|s| {
            s.font_size(FONT_LABEL)
                .color(theme::text_faint())
                .flex_shrink(0.0_f32)
        }),
    ))
    .style(|s| s.items_center().width_full().justify_between().gap(8.0));

    // Two wrapped lines of the statement, then clipped. One line less than the
    // history panel's three: a history row is a query you are looking *for*, so
    // more of it helps you recognise it, while an activity row is a query you are
    // deciding whether to kill — the verb, the table and roughly the shape are
    // the whole question, and a third line of `WHERE` clause pushes the next
    // session off screen.
    let max_h = (FONT_BODY as f64) * 1.4 * 2.0;
    let sql_view: Option<floem::AnyView> = s.sql.as_deref().map(|sql| {
        highlight_mono(
            schemaic_core::history::preview(sql),
            term.clone(),
            FONT_BODY,
            theme::text_dim,
            1.4,
        )
        // `width_full()` is load-bearing, not decoration: a rich text sizes its
        // own node to the text it measured, so with nothing to be 100% *of* it
        // lays out as one long line and the clip below merely cuts it off. The
        // width is what gives it something to wrap against.
        .style(move |s| s.width_full().max_height(max_h))
        .clip()
        .into_any()
    });

    // `Running · employees`. The database is joined by a middle dot rather than a
    // gap so a session attached to none (a `FLUSH TABLES` or a `SHOW STATUS`
    // scrape) leaves the state word alone instead of trailing a separator.
    let where_ = match s.database.as_deref() {
        Some(db) if !db.is_empty() => format!(" · {db}"),
        _ => String::new(),
    };
    let footer = h_stack((
        text(s.state.label()).style(move |s| s.font_size(FONT_LABEL).color(color())),
        text(where_).style(|s| {
            s.font_size(FONT_LABEL)
                .color(theme::text_muted())
                .min_width(0.0)
        }),
    ))
    .style(|s| s.items_center().width_full());

    let note_view: Option<floem::AnyView> = s.note().map(|n| {
        text(n)
            .style(|s| {
                s.font_size(FONT_LABEL)
                    .color(theme::text_muted())
                    .width_full()
            })
            .into_any()
    });

    let mut rows: Vec<floem::AnyView> = vec![heading.into_any()];
    rows.extend(sql_view);
    rows.push(footer.into_any());
    rows.extend(note_view);

    // **The row *is* the column stack** — one nesting level fewer than the
    // history panel's `container(inner)`, which bought nothing.
    //
    // The menu closure shares the row's `Rc` rather than taking a second copy of
    // the session: the list is rebuilt on every poll, and a per-row clone that
    // only the right-click path ever reads is a copy of every string on screen,
    // made and dropped a few seconds later, for a menu almost nobody opens.
    let menu_session = s;
    floem::views::stack_from_iter(rows)
        .on_secondary_click_stop(move |_| {
            overlay.context_menu.set(None);
            overlay.popup_anchor.set(None);
            overlay.popup_width.set(160.0);
            overlay
                .popup_menu
                .set(Some(row_menu(&menu_session, kill.clone())));
        })
        .style(|s| {
            s.flex_col()
                .width_full()
                .gap(4.0)
                .padding_horiz(12.0)
                .padding_vert(9.0)
                .border_bottom(1.0)
                .border_color(theme::border())
                .hover(|s| s.background(theme::row_hover_soft()))
        })
        .into_any()
}

/// A row's right-click menu: the two kills, then a copy of the statement.
///
/// "Cancel query" is *dimmed* rather than absent on a session with nothing
/// running, so the menu keeps the same shape on every row — an entry that moves
/// between rows is one you click by muscle memory and miss.
fn row_menu(s: &SessionInfo, kill: Rc<dyn Fn(i64, KillKind)>) -> Vec<MenuEntry> {
    let id = s.id;
    let cancel = kill.clone();
    let mut entries = vec![
        MenuEntry::action(KillKind::Query.label(), move || {
            (cancel)(id, KillKind::Query)
        })
        .disabled(!KillKind::Query.applies_to(s)),
        MenuEntry::action(KillKind::Session.label(), move || {
            (kill)(id, KillKind::Session)
        }),
    ];
    if let Some(sql) = s.sql.clone() {
        entries.push(MenuEntry::Separator);
        entries.push(MenuEntry::action("Copy statement", move || {
            let _ = floem::Clipboard::set_contents(sql.clone());
        }));
    }
    entries
}
