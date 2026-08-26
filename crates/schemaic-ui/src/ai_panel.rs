//! The AI Assistant panel: the Claude Code chat surface on the right. Renders the
//! conversation (`message_bubble` → `render_segments`, prose as light markdown via
//! the shared `render_markdown`, tool calls as `tool_chip`s), the auto-following
//! scroll with a jump-to-bottom affordance, the live "thinking" elapsed timer, and
//! the message input (`ai_input_row`, or a disabled placeholder when Claude isn't
//! reachable). All state comes in via the `Ui` bundle (`ui.ai` / `ui.ai_actions`).

use std::rc::Rc;

use std::time::Duration;

use floem::AnyView;
use floem::kurbo::{Point, Rect};
use floem::prelude::*;
use floem::reactive::{Scope, create_effect};
use floem::style::{CursorStyle, ScaleX, ScaleY, Transition, TranslateX};
use floem::unit::PxPct;

use schemaic_core::transcript::{
    Recall, RecallDir, Seg, ToolCall, TurnStats, recall_apply, user_prompts,
};

use crate::consts::{chat_pad_h, follow_slack};
use crate::markdown::{CodeActions, render_markdown};
use crate::widgets::{
    autohide, autohide_state, follow_after_scroll, jump_to_bottom_button, next_floor,
    section_title, shift_hscroll, thin_scroll, toolbar_icon, verb_spinner, with_scroll_gesture,
};
use crate::{ChatMessage, FieldCfg, Role, Ui, edit_field, icons, theme};

// ===== moved from lib.rs (AI panel) =====

thread_local! {
    static AI_SEEN: std::cell::RefCell<Option<(RwSignal<usize>, Scope)>> =
        const { std::cell::RefCell::new(None) };
}

/// Count of chat messages already on screen. The conversation list rebuilds whole
/// on every change (including each streaming token), so bubbles at an index `>=`
/// this count when it (re)builds are the newly-appended ones — only they get the
/// entrance pop. A token that merely rebuilds the list in place doesn't grow the
/// count, so it never re-pops. Reset to 0 when the conversation empties (new chat).
/// Detached scope → lives for the whole process (like `window_size`).
fn ai_seen() -> RwSignal<usize> {
    AI_SEEN.with(|cell| {
        if cell.borrow().is_none() {
            let scope = Scope::new();
            let sig = scope.create_rw_signal(0usize);
            *cell.borrow_mut() = Some((sig, scope));
        }
        cell.borrow().as_ref().unwrap().0
    })
}

/// Treat the next `n` bubbles as already seen, so they mount without the
/// entrance pop.
///
/// Call this immediately *before* replacing the conversation wholesale — a
/// restore from disk or a connection switch. Those messages aren't arriving,
/// they're reappearing, and animating a whole conversation in at once reads as
/// a glitch rather than an arrival.
pub fn mark_messages_seen(n: usize) {
    ai_seen().set(n);
}

// ── AI panel: Claude Code chat ───────────────────────────────────────────────
pub(crate) fn ai_panel(ui: Ui) -> impl IntoView {
    let messages = ui.ai.messages;
    let input = ui.ai.input;
    let busy = ui.ai.busy;
    let send = ui.ai_actions.send.clone();
    let cancel = ui.ai_actions.cancel.clone();
    let new_chat_cb = ui.ai_actions.new_chat.clone();
    let regenerate = ui.ai_actions.regenerate.clone();
    let settings_open = ui.ai.settings_open;
    let gutter = ui.ai.gutter;
    let cli_path = ui.ai.cli_path;
    let cli_ok = ui.ai_actions.cli_ok.clone();
    // Actions for code-block bars: insert into a new query tab, and run.
    //
    // The chat is about the tab the user is looking at, so a code block from it
    // belongs to *that* tab's database — not to whichever database a brand-new
    // tab would have started on (the last one picked, else the first by name).
    let code_actions = CodeActions {
        insert: {
            let oq = ui.tab_actions.open_query.clone();
            let active_db = ui.tabs_ui.active_db;
            Rc::new(move |sql: String| (oq)(sql, active_db.get_untracked()))
        },
        run: ui.tab_actions.run.clone(),
        // A proposed table change goes to the DDL preview — the same modal, the
        // same Apply. Against that tab's database for the same reason a code
        // block is: the chat is about what the user is looking at.
        propose: {
            let ui = ui.clone();
            let tabs = ui.tabs_ui.tabs;
            let active = ui.tabs_ui.active;
            let active_conn = ui.conn.active_conn;
            Rc::new(move |proposal: schemaic_core::propose::Proposal| {
                // **The tab's database only counts on the tab's own
                // connection.** Switching connections doesn't move the focused
                // tab — a tab keeps the one it was opened on — so the unfiltered
                // `active_db` memo can name a database that lives somewhere
                // else, and `preview_proposal` pairs whatever it is given with
                // `edit_ctx`'s **active** connection and stamps that `conn_id`
                // into the plan `run_ddl` executes: an `ALTER` on prod against a
                // proposal written about dev.
                //
                // `tabsel::scoped_database` is that rule, and the call is here
                // rather than a second spelling of it because this is the one
                // caller that can destroy something. It was written out inline —
                // expression for expression identical, with no test — because
                // `schemaic-ui` cannot depend on `schemaic-app`, where it lived.
                let conn = active_conn.get_untracked();
                let tab = tabs.with_untracked(|v| {
                    v.iter()
                        .find(|t| t.id == active.get_untracked())
                        .map(|t| (t.conn_id.get_untracked(), t.database.get_untracked()))
                });
                let Some(db) = schemaic_core::tabsel::scoped_database(tab, conn, None) else {
                    return Err(
                        "This change is about a tab on a different connection — switch to it, or \
                         pick a database on this one, before applying a schema change."
                            .to_string(),
                    );
                };
                crate::ddl_preview::preview_proposal(&ui, &db, &proposal)
            })
        },
    };
    // Reactive: is Claude reachable for the current CLI-path value? Drives the
    // empty-state message and the disabled message box.
    let available = floem::reactive::create_memo(move |_| cli_ok(cli_path.get()));

    // Live "thinking" elapsed timer: (re)start a 100ms poll whenever a turn goes
    // busy; it stops itself once `busy` clears (the final summary takes over).
    let elapsed_ms = RwSignal::new(0u64);
    create_effect(move |_| {
        if busy.get() {
            elapsed_ms.set(0);
            tick_elapsed(std::time::Instant::now(), elapsed_ms, busy);
        }
    });

    // A `scroll` hands its child unbounded width, so the bubble list can't just
    // `width_full` (it would size to content and never wrap). Instead we measure
    // the scroll viewport's width (below) and pin the list to it, so the bubbles
    // track the panel as it's resized. Seeded to the default until first layout.
    let panel_w = RwSignal::new(theme::AI_W);

    // One view per message: a streamed chunk re-renders only the bubble it lands
    // in. The list used to rebuild whole on every chunk, re-parsing and
    // re-laying out every earlier message's markdown — quadratic over a
    // conversation, and it made the content momentarily *collapse* each time,
    // because a freshly built `rich_text` measures unwrapped on its first layout
    // pass and only wraps on the second. Floem clamps the scroll offset against
    // that short height, which is what dragged the reader around mid-answer.
    //
    // Two pieces make it per-message, and both are needed. `dyn_stack` keyed by
    // index keeps a bubble's view alive across changes to its neighbours — but
    // every read of `messages` tracks the *whole* vector, so a naive child would
    // still rebuild on every chunk. The per-message memo is the filter: it
    // re-runs on each change and notifies only when *this* message differs.
    let msg_count = floem::reactive::create_memo(move |_| messages.with(|m| m.len()));

    // Floor under the list's height, and the thing that finally stops a streamed
    // answer dragging the scroll around.
    //
    // The collapse above is unavoidable from here: floem's `RichText` reports its
    // *unwrapped* size from `layout` and only learns its wrap width in
    // `compute_layout`, which then requests a second pass — so a rebuilt bubble is
    // one line tall for one pass, every chunk. The scroll clamps its offset
    // against that short height, and a clamp cannot be undone after the fact
    // without fighting the next one, which is exactly what re-pinning the reader
    // turned into.
    //
    // A message only ever grows *while it streams* (a chunk appends; the spinner
    // is replaced by something taller), so the dips are measurement, never
    // content. Holding the highest height seen makes them invisible to the
    // scroll: the list renders a few pixels of slack for one frame instead of
    // collapsing, and the clamp never fires.
    //
    // **But that premise is about streaming, and does not hold across a
    // re-layout.** Dragging the panel wider re-wraps every bubble shorter while
    // the floor stays where the narrow layout put it, so ~300px of blank sat
    // under the last message, `content_h` measured the *floored* box, the
    // jump-to-bottom button lit up, and the next `bump` snapped to the bottom of
    // the blank. Nor does it hold across a conversation *switch*: a memo on the
    // message count dedups when the new conversation is the same length, so the
    // reset never ran.
    //
    // So the floor is invalidated by everything that legitimately changes the
    // content's true height — the message count, the wrap width, which
    // conversation this is, and **the interface scale** — and only *raised*
    // within one of those. The decision is `next_floor`, so the "release it" and
    // "hold it" halves can't drift.
    //
    // The scale is the fourth entry and the newest: it re-lays out every bubble
    // at a different type size, which is a re-wrap by another name. Left out, it
    // reproduced the panel-resize bug exactly — 200% → 150% → 100% each shrank
    // the real content while the floor held the tallest layout seen, leaving a
    // screen of blank under the last message that grew with every step down.
    let floor = RwSignal::new(0.0_f64);
    let active_conn = ui.conn.active_conn;
    create_effect(move |_| {
        // Width to the whole pixel: a fractional jitter from measurement is not a
        // re-wrap, and re-releasing the floor every frame would undo the point.
        let _key = (
            msg_count.get(),
            panel_w.get().round() as i64,
            active_conn.get(),
            theme::ui_scale(),
        );
        if floor.get_untracked() != 0.0 {
            floor.set(0.0);
        }
    });

    let convo = dyn_container(
        move || msg_count.get() == 0,
        move |is_empty| {
            if is_empty {
                ai_seen().set(0); // new/cleared conversation → next messages pop in
                // Left-aligned placeholder: 10px below the title, 15px from the
                // left, 14px. Flips to "Claude not connected." when Claude isn't
                // reachable (auto-detect failed, or a bad manual path).
                return dyn_container(
                    move || available.get(),
                    move |ok| {
                        let msg = if ok {
                            "Ask about your SQL..."
                        } else {
                            "Claude not connected."
                        };
                        text(msg)
                            .style(|s| {
                                s.font_size(theme::scaled_font(14.0))
                                    .color(theme::text_muted())
                                    .padding_top(theme::scaled(10.0))
                                    .padding_left(theme::scaled(12.0))
                            })
                            .into_any()
                    },
                )
                .into_any();
            }
            let actions = code_actions.clone();
            let regen = regenerate.clone();
            // Width pinned to the scroll viewport (`panel_w`, measured below) so a
            // scroll's unbounded child width doesn't stop the text wrapping, while
            // still tracking the panel as it's resized. 16px between messages —
            // wider than it was, because with Claude's turns unboxed the gap is
            // the only thing separating one turn from the next; the first label
            // sits 10px below the title, and messages carry their own margins.
            dyn_stack(
                // **The conversation's identity is in the item, not just the
                // index.** Keyed by index alone, floem retains item 0's scope
                // across every conversation change that does not pass through
                // zero messages — `dyn_stack` disposes an item scope only in
                // `remove_index`, for keys that leave the set — and the count is
                // the only thing this closure read, so switching between two
                // conversations of equal length was invisible to it. Anything
                // the item scope owns then belonged to the wrong conversation:
                // `attach_open` most of all, so expanding a card on connection A
                // and switching to B rendered B's attachment card already open,
                // printing rows nobody asked to see. The same "equal length is
                // invisible" trap the scroll floor above documents.
                move || {
                    let conn = active_conn.get();
                    (0..msg_count.get()).map(move |i| (conn, i))
                },
                |k| *k,
                move |(_conn, i)| {
                    let actions = actions.clone();
                    let regen = regen.clone();
                    // Newly appended bubbles get the entrance pop; ones that were
                    // already on screen don't.
                    //
                    // Consumed on the first *build*, not held for the item: the
                    // streaming bubble rebuilds on every chunk, and the pop starts
                    // from 94% scale, so a flag that stayed true replayed the
                    // animation per chunk and the bubble visibly pulsed. What the
                    // list rebuild used to get from `ai_seen` advancing underneath
                    // it, each item now owns.
                    let pop = Rc::new(std::cell::Cell::new(i >= ai_seen().get_untracked()));
                    if ai_seen().get_untracked() < i + 1 {
                        ai_seen().set(i + 1);
                    }
                    // Whether this message's attachment card is expanded, owned
                    // *here* for the same reason `pop` is: the `dyn_container`
                    // below rebuilds the bubble when `is_last` flips or the theme
                    // generation bumps, neither of which is about the card, and a
                    // flag living inside it was a fresh `false` after each — so
                    // reading what was sent while the answer streamed in closed
                    // the card the moment the answer arrived. This scope is the
                    // `dyn_stack` item's: it survives those rebuilds, and — since
                    // the item key carries the connection — it is disposed when
                    // the conversation changes as well as when the message goes.
                    // It used to say "disposed with the message", which was true
                    // only of *New chat*: that clears to zero messages and
                    // rebuilds the outer container, and nothing else did.
                    let attach_open = RwSignal::new(false);
                    // `is_last` rides in the memo rather than being captured: it
                    // drives the regenerate affordance, and appending a message
                    // flips it on the previously-last bubble, which is a change to
                    // that bubble even though its message didn't move.
                    //
                    // A **fingerprint**, not the message. The memo held an
                    // `Option<ChatMessage>`, so every streamed chunk ran all N of
                    // these closures — each a deep clone plus a deep `PartialEq`,
                    // over segments that include untruncated tool output — and
                    // kept a second resident copy of the whole conversation. The
                    // fingerprint reads lengths, allocates nothing, and is
                    // documented against exactly the mutations the stream makes.
                    let msg = floem::reactive::create_memo(move |_| {
                        messages
                            .with(|m| (m.get(i).map(ChatMessage::fingerprint), i + 1 == m.len()))
                    });
                    // Also keyed on the UI-theme generation. `render_markdown` bakes
                    // its body colour into a text `Attrs` list rather than a style
                    // closure, so it is the one place a live theme switch cannot
                    // reach by re-reading — without the rebuild, every message on
                    // screen keeps the old theme's text colour against the new
                    // background and the panel goes two-toned.
                    dyn_container(
                        move || (msg.get(), theme::ui_generation()),
                        move |((fp, is_last), _)| {
                            // The message itself is read here, untracked, once
                            // per *rebuild* — which the fingerprint above has
                            // already decided is warranted.
                            match fp.and_then(|_| messages.with_untracked(|m| m.get(i).cloned())) {
                                Some(m) => message_bubble(
                                    m,
                                    actions.clone(),
                                    elapsed_ms,
                                    is_last,
                                    regen.clone(),
                                    pop.replace(false),
                                    gutter,
                                    attach_open,
                                )
                                .into_any(),
                                // The vector shrank under the stack; the next diff
                                // drops this item.
                                None => empty().into_any(),
                            }
                        },
                    )
                },
            )
            .style(move |s| {
                s.flex_col()
                    .width(panel_w.get())
                    .min_height(floor.get())
                    .gap(theme::scaled(16.0))
                    .padding_top(theme::scaled(10.0))
                    .padding_bottom(theme::scaled(10.0))
            })
            .into_any()
        },
    );

    // Keep the view pinned to the newest content: on every change (a sent message,
    // and each streamed token) scroll to the bottom. The scroll is deferred one
    // tick — `dyn_container` rebuilds the bubbles when `messages` changes, and the
    // new content's height isn't measured until after layout, so scrolling
    // synchronously would clamp to the *old* (shorter) bottom and land above the
    // latest message. `exec_after(0)` fires after layout, so it reaches the true
    // bottom. The `bump` signal carries that post-layout trigger into `scroll_to`.
    let bump = RwSignal::new(0u64);
    // Whether to keep following the bottom. Seeded true, cleared the moment the
    // user scrolls away from it, and re-armed by the jump-to-bottom button or by
    // sending a new message — the same shape the Live Monitor's log uses. Without
    // it the effect below fires on every streamed *token*, so scrolling up to
    // re-read anything during a generation was impossible.
    let follow = RwSignal::new(true);
    let last_sent = RwSignal::new(0usize);
    create_effect(move |_| {
        // Sending re-arms the follow: the user is asking about what they just
        // typed. An assistant message arriving does not — that is the case where
        // they may be reading something further up.
        let sent = messages.with(|m| m.iter().filter(|msg| msg.role == Role::User).count());
        if sent != last_sent.get_untracked() {
            last_sent.set(sent);
            follow.set(true);
        }
        floem::action::exec_after(std::time::Duration::ZERO, move |_| {
            bump.try_update(|n| *n = n.wrapping_add(1));
        });
    });

    // Scroll-position tracking for the jump-to-bottom button: the unclipped
    // content height vs. the visible viewport's bottom edge.
    let content_h = RwSignal::new(0.0_f64);
    let view_rect = RwSignal::new(Rect::ZERO);

    // Inline the auto-hide wiring (rather than the `autohide` helper) so our
    // `on_scroll` can BOTH track the viewport and poke the bar-hide timer — a
    // single `on_scroll` callback would otherwise clobber the other.
    let (scroll_shown, scroll_poke) = autohide_state();
    // Was the move the reader's, or a relayout's? `content_h` is written a layout
    // pass ahead of the offset the last snap reached, so mid-stream the two
    // disagree through nobody's doing — see `follow_after_scroll`.
    let (scrolled, by_user) = with_scroll_gesture(scroll(convo.on_resize(move |r| {
        let h = r.height();
        content_h.set(h);
        // The raise half of `next_floor`; the effect above releases it early on
        // the things that are *known* to change the true height.
        //
        // **But the floor only holds while a turn is streaming, and that is what
        // makes a stale one impossible.** Its whole premise is the measurement dip
        // a rebuilt `RichText` reports mid-stream — outside a stream there is no
        // dip to hide, so holding anything there can only be wrong. Written as the
        // premise rather than as a list of invalidators because the list has been
        // incomplete twice: first the wrap width, then the interface scale, each
        // leaving a band of blank under the last message. Whatever the next missed
        // invalidator turns out to be, an idle panel now measures itself afresh
        // and the gap closes on the next layout instead of persisting.
        let next = next_floor(floor.get_untracked(), h, !busy.get_untracked());
        if next != floor.get_untracked() {
            floor.set(next);
        }
    })));
    let scrolled = scrolled
        .scroll_style(move |cs| thin_scroll(cs).hide_bars(!scroll_shown.get()))
        .on_scroll(move |vp| {
            view_rect.set(vp);
            // Only notify on a real flip: `set` never dedups, and a redundant
            // notify would re-snap while the user scrolls near the bottom.
            let keep = follow_after_scroll(
                follow.get_untracked(),
                (by_user)(),
                vp.y1,
                content_h.get_untracked(),
                follow_slack(),
            );
            if follow.get_untracked() != keep {
                follow.set(keep);
            }
            scroll_poke();
        })
        // `None` while scrolled up — that is what *releases* the follow. A
        // `scroll_to` target is sticky, so leaving `bump` un-read wouldn't be
        // enough: the last target stays applied. Nothing re-pins the reader,
        // because with the height floor above nothing moves them.
        .scroll_to(move || {
            bump.get();
            follow.get().then(|| Point::new(0.0, 1.0e9))
        })
        // Publish the viewport width so the bubble list can size to it (responsive
        // bubbles). The list is clipped by the scroll, so this can't feed back into
        // the scroll's own width — no layout loop.
        .on_resize(move |r| {
            if (r.width() - panel_w.get_untracked()).abs() > 0.5 {
                panel_w.set(r.width());
            }
        })
        .style(|s| s.flex_grow(1.0_f32).width_full().min_height(0.0));

    // Jump-to-bottom: offered exactly when the follow has been **released**, which
    // is what the button undoes. Derived from `follow` rather than re-deriving
    // the geometry, and that is the fix rather than a simplification: since the
    // follow became `follow_after_scroll`, the two deliberately answer
    // differently — a relayout dip leaves the position nowhere near the bottom
    // while the panel is still dutifully following, and the geometric form then
    // offered "you have scrolled away" and re-triggered its 150 ms transition
    // faster than it could settle. It also drops a per-chunk memo over
    // `content_h`, which is written on every measurement.
    //
    // `view_rect` still gates it, so the button can't flash before first layout.
    let show_jump =
        floem::reactive::create_memo(move |_| view_rect.get().height() > 1.0 && !follow.get());
    let jump = jump_to_bottom_button(
        move || show_jump.get(),
        move || {
            follow.set(true);
            bump.update(|n| *n = n.wrapping_add(1));
        },
    );
    let convo = stack((scrolled, jump))
        .style(|s| s.flex_col().flex_grow(1.0_f32).width_full().min_height(0.0));

    // Input: enabled when Claude is reachable, otherwise a disabled placeholder
    // box (no point sending into a black hole).
    // Two ways the box can be inert, and they mean different things: Claude
    // isn't reachable, or the database isn't. The second is recoverable from the
    // header's Retry, so say which it is rather than showing one dead box.
    let conn_status = ui.conn.conn_status;
    // Memoised, and that is load-bearing rather than an optimisation: the health
    // poll re-`set`s `conn_status` every tick, usually to the value it already
    // held, and a `set` never dedups. Read straight into the `dyn_container` key,
    // that rebuilt the whole input row on a timer — which destroys the message
    // field mid-sentence, so the box lost focus and re-measured its own height
    // from an unlaid-out editor. A memo only notifies when the pair really
    // changes. (Note it must stay a *pair* memo: two memos would be two keys.)
    let input_state =
        floem::reactive::create_memo(move |_| (available.get(), conn_status.get().is_down()));
    let input_row = dyn_container(
        move || input_state.get(),
        move |(claude_ok, db_down)| match (claude_ok, db_down) {
            (true, false) => {
                ai_input_row(input, messages, busy, send.clone(), cancel.clone()).into_any()
            }
            (_, true) => ai_input_disabled("Not connected to the database").into_any(),
            (false, _) => ai_input_disabled("Message…").into_any(),
        },
    )
    .style(|s| s.flex_shrink(0.0_f32));

    // Title row: "AI ASSISTANT" left; a new-chat button and the settings gear
    // right. The gear is rightmost (12px from the edge); new-chat sits 10px to its
    // left (matching the schema panel's eye→gear gap).
    let new_chat = toolbar_icon(
        icons::MESSAGE_SQUARE_PLUS,
        5.0,
        2.0,
        || true,
        move || (new_chat_cb)(),
    )
    .tooltip(|| text("New chat").style(crate::widgets::tooltip_style));
    let gear = toolbar_icon(
        icons::SLIDERS_VERTICAL,
        5.0,
        7.0,
        || true,
        move || settings_open.set(true),
    )
    .tooltip(|| text("AI settings…").style(crate::widgets::tooltip_style));
    let icons_group =
        h_stack((new_chat, gear)).style(|s| s.flex_row().items_start().flex_shrink(0.0_f32));
    let title_row = h_stack((section_title("AI ASSISTANT"), icons_group))
        .style(|s| s.width_full().flex_row().items_start().justify_between());

    // The chip sits between the conversation and the box, so "what will go with
    // this question" is read on the way to typing it.
    let staged = v_stack((attachment_chip(ui.ai.attachment), input_row))
        .style(|s| s.flex_col().width_full().flex_shrink(0.0_f32));

    v_stack((title_row, convo, staged)).style(move |s| {
        s.width(crate::widgets::right_panel_w().get())
            .flex_shrink(0.0_f32)
            .height_full()
            .flex_col()
            .background(theme::bg_panel())
            .border_left(1.0)
            .border_color(theme::border())
    })
}

// The AI message box: a full-width multiline field with an inline send/stop icon
// (play sends, square cancels while busy, dim play when empty). Enter submits.
//
// Ctrl+Up/Down recalls the user's own earlier questions from this conversation
// (`transcript::user_prompts` / `recall_step`). A recalled question is *shown*,
// not typed: it renders in the placeholder colour until the user touches it, and
// stepping again replaces it. It is nonetheless the field's real value, so
// sending one — Enter or the icon — sends exactly what is on screen.
fn ai_input_row(
    input: RwSignal<String>,
    messages: RwSignal<Vec<ChatMessage>>,
    busy: RwSignal<bool>,
    send: Rc<dyn Fn(String)>,
    cancel: Rc<dyn Fn()>,
) -> impl IntoView {
    let send_key = send.clone();
    // Where the recall sits: `None` is the empty box it started from.
    let recall = RwSignal::new(None::<usize>);
    let uncommitted = RwSignal::new(false);
    let caret_end = RwSignal::new(0u64);
    // An emptied box (sending clears it; so does deleting the text) ends the
    // recall, so the next Ctrl+Up starts from the newest question again rather
    // than resuming wherever the last walk stopped. Guarded sets: `set` never
    // dedups, and this runs on every keystroke.
    // `trim()`, like the send icon and Enter: they treat a box holding one space
    // as empty, `user_prompts` trims on the way in, and this row disagreeing with
    // itself about what "typed something" means is what made Ctrl+Up silently
    // refuse after a stray space.
    create_effect(move |_| {
        if input.with(|t| t.trim().is_empty()) {
            if recall.get_untracked().is_some() {
                recall.set(None);
            }
            if uncommitted.get_untracked() {
                uncommitted.set(false);
            }
        }
    });
    let step = move |dir: RecallDir| {
        // Whether it may happen, where it lands and what the box then holds is
        // `transcript::recall_apply` — in core with the arithmetic it was split
        // from, rather than a second, differently-worded copy of the same rules
        // in a view builder.
        let prompts = messages.with_untracked(|m| user_prompts(m));
        let action = input.with_untracked(|t| {
            recall_apply(
                t,
                uncommitted.get_untracked(),
                recall.get_untracked(),
                &prompts,
                dir,
            )
        });
        let Recall::Show { cursor, text } = action else {
            return;
        };
        recall.set(cursor);
        // Order matters only for the redundant-notify guard: clearing `input`
        // runs the effect above, which already resets the flag.
        input.set(text);
        let now_uncommitted = cursor.is_some();
        if uncommitted.get_untracked() != now_uncommitted {
            uncommitted.set(now_uncommitted);
        }
        caret_end.update(|n| *n = n.wrapping_add(1));
    };
    let icon: Rc<dyn Fn() -> AnyView> = {
        let send = send.clone();
        let cancel = cancel.clone();
        Rc::new(move || {
            let send = send.clone();
            let cancel = cancel.clone();
            dyn_container(
                move || (busy.get(), input.with(|t| t.trim().is_empty())),
                move |(is_busy, is_empty)| {
                    let send = send.clone();
                    let cancel = cancel.clone();
                    if is_busy {
                        // Stop — kills the in-flight request.
                        container(icons::icon(icons::SQUARE, 16.0))
                            .on_click_stop(move |_| (cancel)())
                            .style(|s| {
                                s.items_center()
                                    .color(theme::ai_send_icon_active())
                                    .cursor(CursorStyle::Default)
                                    .hover(|s| s.color(theme::ai_send_icon_hover()))
                            })
                            .into_any()
                    } else if is_empty {
                        // Inactive: dim base color, no action, arrow cursor.
                        container(icons::icon(icons::PLAY_LUCIDE, 16.0))
                            .style(|s| {
                                s.items_center()
                                    .color(theme::ai_send_icon())
                                    .cursor(CursorStyle::Default)
                            })
                            .into_any()
                    } else {
                        // Send.
                        container(icons::icon(icons::PLAY_LUCIDE, 16.0))
                            .on_click_stop(move |_| (send)(input.get_untracked()))
                            .style(|s| {
                                s.items_center()
                                    .color(theme::ai_send_icon_active())
                                    .cursor(CursorStyle::Default)
                                    .hover(|s| s.color(theme::ai_send_icon_hover()))
                            })
                            .into_any()
                    }
                },
            )
            .into_any()
        })
    };
    let field = edit_field(
        input,
        FieldCfg {
            placeholder: "Message…",
            multiline: true,
            // Enter mirrors the send icon's gating: ignore while a turn is in
            // flight (the icon is Stop, not Send) or when the box is empty.
            on_submit: Some(Rc::new(move || {
                let text = input.get_untracked();
                if !busy.get_untracked() && !text.trim().is_empty() {
                    (send_key)(text);
                }
            })),
            on_ctrl_arrow_up: Some(Rc::new(move || step(RecallDir::Older))),
            on_ctrl_arrow_down: Some(Rc::new(move || step(RecallDir::Newer))),
            uncommitted: Some(uncommitted),
            caret_end: Some(caret_end),
            trailing: Some(icon),
            ..Default::default()
        },
    )
    .style(|s| s.width_full());
    container(field).style(|s| {
        s.width_full()
            .padding(theme::scaled(8.0))
            .border_top(1.0)
            .border_color(theme::border())
    })
}

/// What a past turn actually carried, above the question it went with.
///
/// Collapsed to the summary line, expanding to **the exact text the model was
/// given** — not a re-rendering of the grid, the block itself. "What did I send
/// it?" should be answerable by looking, without trusting this code twice.
///
/// A conversation restored from disk has the summary and no rows (see
/// [`schemaic_core::transcript::Attachment`]), and says so rather than
/// pretending to an empty table.
///
/// `open` is handed in rather than owned here. The bubble around this card is
/// rebuilt for reasons that have nothing to do with the card — appending
/// Claude's reply flips `is_last` on the question, and a theme switch bumps the
/// generation — and a view-local `RwSignal::new(false)` is a *new* signal after
/// each of those, so a card the reader had opened to check what was sent snapped
/// shut under them mid-answer. The caller owns it in the `dyn_stack` item's
/// scope, which outlives the rebuild and dies with the message.
fn sent_attachment(
    a: schemaic_core::transcript::Attachment,
    open: RwSignal<bool>,
) -> impl IntoView {
    let retained = a.retained();
    // The cells, kept as-is and rendered only when the block is actually opened.
    // Building the table up front cost a 200-row string per bubble on every
    // rebuild, and a restored attachment (no rows by design) rendered a
    // two-line empty table nobody ever sees.
    let cells = Rc::new((a.columns, a.rows));
    let head = h_stack((
        icons::icon(icons::TABLE, 12.0).style(|s| s.color(theme::key_foreign())),
        // **`text_dim`, not `text_muted`.** These are body prose on a surface
        // where `text_muted` measures 2.42:1 (Light) — under AA and under the
        // icon floor — and it passed the gate only because the one row for that
        // pairing was written down at `Icon`. See `contrast::UI_PAIRINGS`, where
        // both of these now have `Body` rows of their own.
        text(a.summary).style(|s| {
            s.font_size(theme::font_hint())
                .font_family("IBM Plex Sans".to_string())
                .color(theme::text_dim())
                .flex_grow(1.0_f32)
                .min_width(0.0)
        }),
        // The chevron *is* the affordance: a restored attachment has no rows to
        // expand into, so it simply doesn't get one — nothing to say in words.
        dyn_container(
            move || retained.then(|| open.get()),
            move |open| match open {
                None => empty().into_any(),
                Some(open) => icons::icon(
                    if open {
                        icons::CHEVRON_UP
                    } else {
                        icons::CHEVRON_DOWN
                    },
                    12.0,
                )
                .into_any(),
            },
        )
        .style(|s| s.color(theme::text_muted())),
    ))
    .on_click_stop(move |_| {
        if retained {
            open.update(|o| *o = !*o);
        }
    })
    .style(move |s| {
        s.width_full()
            .flex_row()
            .items_center()
            .gap(theme::scaled(6.0))
            .apply_if(retained, |s| s.cursor(CursorStyle::Default))
    });
    let rows = dyn_container(move || open.get() && retained, {
        let cells = cells.clone();
        move |show| match show {
            false => empty().into_any(),
            // A wide table is the normal case here, so the horizontal bar has to
            // behave like every other one in the app: Shift+wheel drives it, and
            // it auto-hides instead of sitting permanently across the block.
            true => {
                let body = schemaic_core::prompt::pipe_table(&cells.0, &cells.1, ATTACH_VIEW_CHARS);
                autohide(shift_hscroll(text(body.trim_end().to_string()).style(
                    |s| {
                        s.font_family("monospace".to_string())
                            .font_size(theme::font_hint())
                            .color(theme::text_dim())
                    },
                )))
                .style(|s| s.width_full().max_height(attach_preview_h()))
                .into_any()
            }
        }
    })
    .style(|s| s.width_full());
    v_stack((head, rows)).style(|s| {
        s.flex_col()
            .width_full()
            .gap(theme::scaled(6.0))
            .padding(theme::scaled(8.0))
            .background(theme::bg_deepest())
            .border(1.0)
            .border_color(theme::border())
            .border_radius(6.0)
    })
}

/// Cell width in the transcript's copy of an attachment. Wider than the prompt's
/// own cap would matter for: this is only ever read by a person, and a value cut
/// short here would misrepresent what was sent.
const ATTACH_VIEW_CHARS: usize = 200;

/// How tall an expanded attachment preview would like to be, at 100%.
const ATTACH_PREVIEW_H: f64 = 220.0;

/// The most of the window an expanded attachment preview may take.
///
/// A third, because the preview is a **peek** — the block it sits in is one
/// message of a conversation that scrolls, and a preview taller than this pushes
/// the message it belongs to, and the answer under it, off the screen. It is a
/// share rather than a subtraction because there is no fixed chrome to subtract:
/// what surrounds it is more conversation.
const ATTACH_PREVIEW_WINDOW_SHARE: f64 = 3.0;

/// The tallest an expanded attachment preview may grow, given the window height.
///
/// **Not [`crate::widgets::modal_body_h`]**, which is what this used to call and
/// the only non-modal caller it had. That function caps a *modal's* scrolling
/// body, and its floor exists so a panel with a title and a footer laid out
/// around the body can't reduce the body to a sliver — a floor that makes no
/// sense here, where the surrounding thing is a scroll. It also made the preview
/// **larger** on exactly the windows with least room: at 160% the floor is 256px,
/// so on a 400px-tall window the preview took 64% of it, which is the shape this
/// function's own test refuses.
///
/// Pure, and separate from the signal read, so the arithmetic is testable — the
/// rest of this file's sizes are style closures and none of them is.
/// An unmeasured window (0) means "not yet" and takes the wanted size rather
/// than guessing, the same answer [`crate::widgets::cap_to`] gives.
fn attach_preview_cap(want: f64, win_h: f64) -> f64 {
    if win_h <= 1.0 {
        return want;
    }
    want.min(win_h / ATTACH_PREVIEW_WINDOW_SHARE)
}

/// [`attach_preview_cap`] at the live window height and interface scale.
fn attach_preview_h() -> f64 {
    attach_preview_cap(
        theme::scaled(ATTACH_PREVIEW_H),
        crate::widgets::window_size().get().1,
    )
}

/// The staged-attachment chip: what the *next* question will carry, sitting
/// directly over the message box.
///
/// It is the last point before data leaves the machine, so it says how much and
/// from where, and its × is a real cancel — the one gesture between "I clicked
/// Attach" and "my customers' rows left this machine". Absent (zero height) when
/// nothing is staged.
fn attachment_chip(
    staged: RwSignal<Option<schemaic_core::transcript::Attachment>>,
) -> impl IntoView {
    dyn_container(
        // Only the summary drives the rebuild: the rows can be megabytes, and a
        // `dyn_container` key clones whatever it reads on every notification.
        move || staged.with(|a| a.as_ref().map(|a| a.summary.clone())),
        move |summary| match summary {
            None => empty().into_any(),
            Some(summary) => h_stack((
                icons::icon(icons::TABLE, 13.0).style(|s| s.color(theme::key_foreign())),
                // The app's last consent surface — the sentence saying how many
                // of the user's rows are about to leave the machine — so it is
                // the last place to paint prose below the icon floor.
                text(summary).style(|s| {
                    s.font_size(theme::font_hint())
                        .font_family("IBM Plex Sans".to_string())
                        .color(theme::text_dim())
                        .flex_grow(1.0_f32)
                        .min_width(0.0)
                }),
                container(icons::icon(icons::X, 12.0))
                    .on_click_stop(move |_| staged.set(None))
                    .style(|s| {
                        s.items_center()
                            .color(theme::text_muted())
                            .cursor(CursorStyle::Default)
                            .hover(|s| s.color(theme::text()))
                    }),
            ))
            .style(|s| {
                s.width_full()
                    .flex_row()
                    .items_center()
                    .gap(theme::scaled(6.0))
                    .padding_horiz(theme::scaled(8.0))
                    .padding_vert(theme::scaled(5.0))
                    .border(1.0)
                    .border_radius(6.0)
                    .border_color(theme::border())
                    .background(theme::bg_panel())
            })
            .into_any(),
        },
    )
    // The padding is the chip's, not the slot's — an unconditional one would
    // leave an 8px band over the message box in every conversation that never
    // attaches anything. `with(is_some)` rather than `get`: this re-runs on each
    // change and the rows can be megabytes.
    .style(move |s| {
        let on = staged.with(|a| a.is_some());
        s.width_full().apply_if(on, |s| {
            s.padding_horiz(theme::scaled(8.0))
                .padding_top(theme::scaled(8.0))
                .padding_bottom(theme::scaled(4.0))
        })
    })
}

// The disabled message box shown when Claude isn't connected — matches the real
// box's metrics but is inert (dim placeholder, no send icon, no pointer events).
fn ai_input_disabled(placeholder: &'static str) -> impl IntoView {
    let box_ = container(text(placeholder).style(|s| {
        s.font_size(theme::font_body())
            .font_family("IBM Plex Sans".to_string())
            .color(theme::placeholder())
    }))
    .style(|s| {
        s.width_full()
            .height(theme::scaled(34.0))
            .padding_top(theme::scaled(9.0))
            .padding_left(chat_pad_h())
            .background(theme::bg_deepest())
            .border(1.0)
            .border_color(theme::field_border())
            .border_radius(6.0)
    });
    container(box_)
        .style(|s| {
            s.width_full()
                .padding(theme::scaled(8.0))
                .border_top(1.0)
                .border_color(theme::border())
        })
        .pointer_events(|| false)
}

// One message, styled by role — and the two roles are deliberately *not*
// symmetrical. The user's question is a small right-aligned bubble; Claude's
// turn is prose on the panel itself, ruled at its right edge. User messages are
// plain text; assistant/error turns render their segments (prose as light
// markdown, tool calls as chips) plus a cost footer; pending renders "Thinking…".
#[allow(clippy::too_many_arguments)] // a UI builder; grouping into a struct adds no clarity
fn message_bubble(
    m: ChatMessage,
    actions: CodeActions,
    elapsed_ms: RwSignal<u64>,
    is_last: bool,
    regenerate: Rc<dyn Fn()>,
    animate: bool,
    gutter: RwSignal<bool>,
    attach_open: RwSignal<bool>,
) -> impl IntoView {
    let is_user = m.role == Role::User;
    let label_txt = if is_user { "YOU" } else { "CLAUDE" };

    let body: AnyView = if is_user {
        // User's own message: a dim recap, under whatever data went with it —
        // the record of what was sent, kept where it was sent.
        // `text()`, not a dimmer step: the recap used to be `text_muted`, which
        // measures 2.4:1 on the bubble it sits in — the contrast pair claimed
        // `text` and so never caught it. The bubble has to stay just *above*
        // `bg_panel` to read as a bubble at all, and no mid-grey clears AA on a
        // surface that light, so the design's own answer (full text colour) is
        // also the only one that passes.
        let recap = text(m.text).style(|s| {
            s.width_full()
                .font_size(theme::scaled_font(14.0))
                .color(theme::text())
        });
        match m.attachment {
            Some(a) => v_stack((sent_attachment(a, attach_open), recap))
                .style(|s| s.flex_col().width_full().gap(theme::scaled(6.0)))
                .into_any(),
            None => recap.into_any(),
        }
    } else {
        // Assistant turn: "Thinking…" until the first token, then the streamed
        // segments — with a footer underneath (a live elapsed timer while the turn
        // runs, swapped for the final cost/token summary + actions once it finishes).
        // Prose only — tool chips are the assistant *using* tools, not content.
        let copy_text = m.prose();
        let content: AnyView = if m.pending && m.segs.is_empty() {
            verb_spinner(theme::text_muted, || theme::scaled_font(14.0)).into_any()
        } else {
            render_segments(m.segs, m.role, actions, !m.pending).into_any()
        };
        let footer = assistant_footer(
            m.pending, m.stats, elapsed_ms, copy_text, is_last, regenerate,
        );
        v_stack((content, footer))
            .style(|s| s.flex_col().width_full())
            .into_any()
    };

    // Who-said-it, as a small caps label rather than a name: uppercase and bold
    // at the label size, Claude's in the accent colour so the eye finds the start
    // of an answer without a box around it.
    let label = text(label_txt).style(move |s| {
        let s = s.font_size(theme::font_label()).font_bold();
        if is_user {
            s.color(theme::text_muted())
        } else {
            s.color(theme::accent())
        }
    });

    let bubble = if is_user {
        // Right-aligned and shrink-wrapped: the question sits in a low-contrast
        // bubble that hugs its own text (up to 88% of the panel), so a one-line
        // question reads as an aside against Claude's full-width answer. The
        // label sits at the bubble's right edge (12px inset).
        v_stack((
            h_stack((empty().style(|s| s.flex_grow(1.0_f32)), label))
                .style(|s| s.width_full().flex_row().padding_right(theme::scaled(12.0))),
            h_stack((
                empty().style(|s| s.flex_grow(1.0_f32)),
                container(body).style(|s| {
                    s.background(theme::bubble_user_bg())
                        .border_radius(7.0)
                        .padding_horiz(theme::scaled(10.0))
                        .padding_vert(theme::scaled(8.0))
                        .max_width_pct(88.0)
                }),
            ))
            .style(|s| s.width_full().flex_row().padding_horiz(theme::scaled(12.0))),
        ))
        .style(|s| s.flex_col().width_full().gap(theme::scaled(5.0)))
    } else {
        // Claude's turn is **not** a bubble. It sits directly on the panel and is
        // marked only by a 2px accent rule down its *right* edge — the side the
        // user's bubbles are on, so the two voices read as one column with a
        // margin rather than as two rows of chat. Full width, label left-aligned
        // at the same 12px inset.
        //
        // The rule is a setting (AI Settings → *Accent rule on replies*), so the
        // padding that clears it is read from the same signal: turned off, the
        // reply reclaims those 13px and its two insets match — a reply that kept
        // padding for a rule that isn't drawn would just sit off-centre.
        v_stack((
            container(label).style(|s| s.padding_left(theme::scaled(12.0))),
            container(body).style(move |s| {
                let s = s.margin_horiz(theme::scaled(12.0));
                if gutter.get() {
                    s.padding_right(theme::scaled(11.0))
                        .border_right(2.0)
                        .border_color(theme::accent())
                } else {
                    s
                }
            }),
        ))
        .style(|s| s.flex_col().width_full().gap(theme::scaled(5.0)))
    };

    // Entrance pop (slide in from the bubble's side + a slight scale), only on a
    // message's first appearance (`animate`) — the streaming bubble rebuilds on
    // every chunk, so re-popping each time makes it pulse. The caller is what
    // guarantees that: it hands `true` to the first build of a bubble and `false`
    // to every rebuild. User bubbles come from the right, Claude's from the left.
    // `shown` flips a frame after mount so the declared transitions interpolate
    // from the offset/scaled start to rest.
    let shown = RwSignal::new(!animate);
    if animate {
        floem::action::exec_after(Duration::ZERO, move |_| {
            shown.try_update(|v| *v = true);
        });
    }
    let dx = if is_user { 18.0 } else { -18.0 };
    bubble.style(move |s| {
        if !animate {
            return s;
        }
        let t = Transition::ease_in_out(Duration::from_millis(150));
        let s = s
            .transition(TranslateX, t.clone())
            .transition(ScaleX, t.clone())
            .transition(ScaleY, t);
        if shown.get() {
            s
        } else {
            s.translate_x(PxPct::Px(dx)).scale(94.0_f32)
        }
    })
}

// Render an assistant turn: prose segments as markdown, tool segments as chips,
// then a dim cost footer if the turn reported one.
fn render_segments(
    segs: Vec<Seg>,
    role: Role,
    actions: CodeActions,
    settled: bool,
) -> impl IntoView {
    let error_color = role == Role::Error;
    v_stack_from_iter(segs.into_iter().map(move |seg| match seg {
        Seg::Text(t) => {
            if error_color {
                text(t)
                    .style(|s| {
                        s.width_full()
                            .font_size(theme::font_body())
                            .color(theme::error())
                    })
                    .into_any()
            } else {
                render_markdown(&t, actions.clone(), settled).into_any()
            }
        }
        Seg::Tool(tc) => tool_chip(tc).into_any(),
    }))
    .style(|s| s.flex_col().gap(theme::scaled(6.0)).width_full())
}

/// A footer action icon (copy / regenerate): 16px, footer-text colour, brightens
/// on hover. No pointer cursor (native feel — see `docs/architecture.md`).
fn footer_icon(svg: &'static str, on_click: impl Fn() + 'static) -> impl IntoView {
    container(icons::icon(svg, 16.0))
        .on_click_stop(move |_| on_click())
        .style(|s| {
            s.items_center()
                .color(theme::text_muted())
                .hover(|s| s.color(theme::text()))
        })
}

/// Footer under an assistant turn: on the left a live elapsed timer (while
/// pending) or the final `time · ↑in ↓out` summary; on the right the Copy action
/// (every done turn) and Regenerate (last turn only), 10px apart. Nothing at all
/// when the turn is empty.
fn assistant_footer(
    pending: bool,
    stats: Option<TurnStats>,
    elapsed_ms: RwSignal<u64>,
    copy_text: String,
    is_last: bool,
    regenerate: Rc<dyn Fn()>,
) -> AnyView {
    let style =
        |s: floem::style::Style| s.font_size(theme::font_label()).color(theme::text_muted());
    let has_stats = stats.as_ref().is_some_and(|s| !s.is_empty());
    let has_text = !copy_text.is_empty();
    if !pending && !has_stats && !has_text {
        return empty().into_any();
    }

    // Left: summary (done) or the live timer (pending).
    let left: AnyView = if let Some(st) = stats.filter(|s| !s.is_empty()) {
        text(st.summary()).style(style).into_any()
    } else if pending {
        dyn_container(
            move || elapsed_ms.get(),
            // The same formatter the finished turn's footer uses — the live
            // counter is replaced by it in place when the turn ends.
            move |ms| {
                text(schemaic_core::transcript::elapsed_text(ms))
                    .style(style)
                    .into_any()
            },
        )
        .into_any()
    } else {
        empty().into_any()
    };

    // Right: Copy (any finished turn with text) + Regenerate (last turn only).
    let actions: AnyView = if !pending && has_text {
        let copy = footer_icon(icons::COPY, move || {
            let _ = floem::Clipboard::set_contents(copy_text.clone());
        });
        // Nudged 2px up: the icons are optically low against the summary text
        // beside them, whose glyphs sit above their own line box's centre.
        let icons_style = |s: floem::style::Style| {
            s.flex_row()
                .items_center()
                .gap(theme::scaled(10.0))
                .margin_top(theme::scaled(-2.0))
        };
        if is_last {
            h_stack((copy, footer_icon(icons::REFRESH_CW, move || (regenerate)())))
                .style(icons_style)
                .into_any()
        } else {
            h_stack((copy,)).style(icons_style).into_any()
        }
    } else {
        empty().into_any()
    };

    // No rule above it: the turn has no box for a rule to divide, so the footer
    // is separated by space alone (9px, the gap the rest of a turn's parts use).
    // Row: left content, then the actions pushed to the right edge.
    let row = h_stack((left, empty().style(|s| s.flex_grow(1.0_f32)), actions))
        .style(|s| s.width_full().flex_row().items_center());
    container(row)
        .style(|s| s.width_full().margin_top(theme::scaled(9.0)))
        .into_any()
}

/// Re-arm the elapsed-timer poll while a turn is in flight (stops once `busy`
/// clears). 100ms cadence keeps the sub-second `ms` readout lively.
fn tick_elapsed(start: std::time::Instant, elapsed_ms: RwSignal<u64>, busy: RwSignal<bool>) {
    floem::action::exec_after(std::time::Duration::from_millis(100), move |_| {
        // `busy` is app-level and outlives the AI panel; `elapsed_ms` lives in
        // the panel's child scope. Gate the write + re-arm on `elapsed_ms` still
        // being alive (`try_update` is `None` once its scope is disposed), so
        // closing the panel mid-turn can't hit a freed signal.
        let alive = elapsed_ms
            .try_update(|v| *v = start.elapsed().as_millis() as u64)
            .is_some();
        if alive && busy.try_get_untracked() == Some(true) {
            tick_elapsed(start, elapsed_ms, busy);
        }
    });
}

// A tool invocation rendered as a chip: a labeled header (tool name + status
// dot), the SQL it ran (if any), and its result once it returns.
fn tool_chip(tc: ToolCall) -> impl IntoView {
    let (dot_color, dot) = match (tc.result.is_some(), tc.is_error) {
        (false, _) => (theme::text_muted(), "○"), // running
        (true, false) => (theme::accent(), "●"),  // done ok
        (true, true) => (theme::error(), "●"),    // done error
    };
    let name = tc.short_name().to_string();
    let header = h_stack((
        text(dot).style(move |s| s.font_size(theme::scaled_font(9.0)).color(dot_color)),
        text(name).style(|s| {
            s.font_size(theme::font_label())
                .font_bold()
                .font_family("monospace".to_string())
                .color(theme::text_dim())
        }),
    ))
    .style(|s| s.flex_row().items_center().gap(theme::scaled(6.0)));

    let sql_view = match tc.sql.clone() {
        Some(sql) => text(sql.trim().to_string())
            .style(|s| {
                s.width_full()
                    .font_family("monospace".to_string())
                    .font_size(theme::font_body())
                    .color(theme::text())
            })
            .into_any(),
        None => empty().into_any(),
    };

    let result_is_error = tc.is_error;
    let result_view = match tc.result.clone() {
        Some(r) => text(truncate_result(&r))
            .style(move |s| {
                let c = if result_is_error {
                    theme::error()
                } else {
                    theme::text_dim()
                };
                s.width_full()
                    .font_family("monospace".to_string())
                    .font_size(theme::font_label())
                    .color(c)
                    .padding_top(theme::scaled(4.0))
                    .border_top(1.0)
                    .border_color(theme::border())
                    .margin_top(theme::scaled(4.0))
            })
            .into_any(),
        None => empty().into_any(),
    };

    v_stack((header, sql_view, result_view)).style(|s| {
        s.flex_col()
            .gap(theme::scaled(4.0))
            .width_full()
            .padding(theme::scaled(8.0))
            .background(theme::bg_deepest())
            .border(1.0)
            .border_color(theme::border())
            .border_radius(6.0)
    })
}

// Tool results (query tables) can be long; keep chips compact.
fn truncate_result(r: &str) -> String {
    const MAX_LINES: usize = 12;
    let lines: Vec<&str> = r.lines().collect();
    if lines.len() <= MAX_LINES {
        return r.trim_end().to_string();
    }
    let mut out = lines[..MAX_LINES].join("\n");
    out.push_str(&format!("\n… (+{} more lines)", lines.len() - MAX_LINES));
    out
}

#[cfg(test)]
mod attach_preview_tests {
    use super::*;

    /// **The defect, as arithmetic.** The preview used to be capped by
    /// `widgets::modal_body_h`, whose floor keeps a *modal's* body from becoming
    /// a sliver between a title and a footer. Applied to a block inside a
    /// scrolling conversation, that floor does the opposite of what a cap is
    /// for: it grows the preview on exactly the windows with least room. At 160%
    /// the floor is 256px, so on a 400px-tall window the preview took 64% of the
    /// screen and pushed the message it belongs to — and the answer under it —
    /// out of sight.
    ///
    /// The property, over every window a person might actually have, at every
    /// scale: **a peek is never more than a third of the window.**
    #[test]
    fn a_preview_never_takes_more_than_its_share_of_the_window() {
        for scale in [
            crate::theme::UiScale::Small,
            crate::theme::UiScale::Normal,
            crate::theme::UiScale::Large,
            crate::theme::UiScale::Huge,
        ] {
            crate::theme::set_ui_scale(scale);
            let want = theme::scaled(ATTACH_PREVIEW_H);
            for win_h in [300.0, 400.0, 600.0, 768.0, 1080.0, 1440.0, 2160.0] {
                let got = attach_preview_cap(want, win_h);
                assert!(
                    got <= win_h / ATTACH_PREVIEW_WINDOW_SHARE + 0.001,
                    "{scale:?} at {win_h}px: preview capped at {got}, which is \
                     more than a third of the window"
                );
                assert!(
                    got <= want + 0.001,
                    "{scale:?} at {win_h}px: {got} > {want}"
                );
            }
        }
        crate::theme::set_ui_scale(crate::theme::UiScale::Normal);
    }

    /// On a window with room, the cap is simply what the preview asked for —
    /// the share is a ceiling, not a target, so an ordinary screen is unaffected
    /// by this change.
    #[test]
    fn a_tall_window_gets_the_height_the_preview_wanted() {
        crate::theme::set_ui_scale(crate::theme::UiScale::Normal);
        assert_eq!(attach_preview_cap(220.0, 1080.0), 220.0);
        assert_eq!(attach_preview_cap(220.0, 660.0), 220.0);
        // Exactly at the boundary, and one pixel under it.
        assert_eq!(attach_preview_cap(220.0, 659.0), 659.0 / 3.0);
    }

    /// An unmeasured window means "not yet", not "zero" — the same answer
    /// `widgets::cap_to` gives, so the first frame is not a collapsed block that
    /// jumps open once a resize lands.
    #[test]
    fn an_unmeasured_window_takes_the_wanted_height() {
        assert_eq!(attach_preview_cap(220.0, 0.0), 220.0);
        assert_eq!(attach_preview_cap(220.0, 1.0), 220.0);
    }
}
