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

use schemaic_core::transcript::{RecallDir, Seg, ToolCall, TurnStats, recall_step, user_prompts};

use crate::consts::{CHAT_PAD_H, FOLLOW_SLACK};
use crate::markdown::{CodeActions, render_markdown};
use crate::widgets::{
    at_content_bottom, autohide_state, jump_to_bottom_button, section_title, thin_scroll,
    toolbar_icon, verb_spinner,
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
    let right_w = ui.layout.right_w;
    let settings_open = ui.ai.settings_open;
    let cli_path = ui.ai.cli_path;
    let cli_ok = ui.ai_actions.cli_ok.clone();
    // Actions for code-block bars: insert into a new query tab, and run.
    let code_actions = CodeActions {
        insert: ui.tab_actions.open_query.clone(),
        run: ui.tab_actions.run.clone(),
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
                                s.font_size(14.0)
                                    .color(theme::text_muted())
                                    .padding_top(10.0)
                                    .padding_left(12.0)
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
            // still tracking the panel as it's resized. 10px between messages; the
            // first label sits 10px below the title; bubbles carry their own margins.
            dyn_stack(
                move || 0..msg_count.get(),
                |i| *i,
                move |i| {
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
                    // `is_last` rides in the memo rather than being captured: it
                    // drives the regenerate affordance, and appending a message
                    // flips it on the previously-last bubble, which is a change to
                    // that bubble even though its message didn't move.
                    let msg = floem::reactive::create_memo(move |_| {
                        messages.with(|m| (m.get(i).cloned(), i + 1 == m.len()))
                    });
                    // Also keyed on the UI-theme generation. `render_markdown` bakes
                    // its body colour into a text `Attrs` list rather than a style
                    // closure, so it is the one place a live theme switch cannot
                    // reach by re-reading — without the rebuild, every message on
                    // screen keeps the old theme's text colour against the new
                    // background and the panel goes two-toned.
                    dyn_container(
                        move || (msg.get(), theme::ui_generation()),
                        move |((m, is_last), _)| match m {
                            Some(m) => message_bubble(
                                m,
                                actions.clone(),
                                elapsed_ms,
                                is_last,
                                regen.clone(),
                                pop.replace(false),
                            )
                            .into_any(),
                            // The vector shrank under the stack; the next diff
                            // drops this item.
                            None => empty().into_any(),
                        },
                    )
                },
            )
            .style(move |s| {
                s.flex_col()
                    .width(panel_w.get())
                    .gap(10.0)
                    .padding_top(10.0)
                    .padding_bottom(10.0)
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
    let scrolled = scroll(convo.on_resize(move |r| content_h.set(r.height())))
        .scroll_style(move |cs| thin_scroll(cs).hide_bars(!scroll_shown.get()))
        .on_scroll(move |vp| {
            view_rect.set(vp);
            // Only notify on a real flip: `set` never dedups, and a redundant
            // notify would re-snap while the user scrolls near the bottom.
            let at_bottom = at_content_bottom(content_h.get_untracked(), vp.y1, FOLLOW_SLACK);
            if follow.get_untracked() != at_bottom {
                follow.set(at_bottom);
            }
            scroll_poke();
        })
        // `None` while scrolled up — that is what *releases* the follow. A
        // `scroll_to` target is sticky, so leaving `bump` un-read wouldn't be
        // enough: the last target stays applied.
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

    // Jump-to-bottom: shown once the user has scrolled up off the latest content —
    // the negation of the same predicate that gates the follow, so the button
    // appears exactly when following has been released. Clicking it re-arms the
    // follow and bumps the trigger.
    let show_jump = floem::reactive::create_memo(move |_| {
        let vp = view_rect.get();
        vp.height() > 1.0 && !at_content_bottom(content_h.get(), vp.y1, FOLLOW_SLACK)
    });
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
    );
    let gear = toolbar_icon(
        icons::SLIDERS_VERTICAL,
        5.0,
        7.0,
        || true,
        move || settings_open.set(true),
    );
    let icons_group =
        h_stack((new_chat, gear)).style(|s| s.flex_row().items_start().flex_shrink(0.0_f32));
    let title_row = h_stack((section_title("AI ASSISTANT"), icons_group))
        .style(|s| s.width_full().flex_row().items_start().justify_between());

    v_stack((title_row, convo, input_row)).style(move |s| {
        s.width(right_w.get())
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
    create_effect(move |_| {
        if input.with(|t| t.is_empty()) {
            if recall.get_untracked().is_some() {
                recall.set(None);
            }
            if uncommitted.get_untracked() {
                uncommitted.set(false);
            }
        }
    });
    let step = move |dir: RecallDir| {
        // Only an empty box, or one still holding a recalled question, may be
        // rewritten — never text the user typed.
        if !uncommitted.get_untracked() {
            if !input.with_untracked(|t| t.is_empty()) {
                return;
            }
            recall.set(None);
        }
        let prompts = messages.with_untracked(|m| user_prompts(m));
        let next = recall_step(prompts.len(), recall.get_untracked(), dir);
        recall.set(next);
        match next {
            Some(i) => {
                input.set(prompts[i].clone());
                uncommitted.set(true);
            }
            None => {
                // Order matters only for the redundant-notify guard: clearing
                // `input` runs the effect above, which already resets the flag.
                input.set(String::new());
                if uncommitted.get_untracked() {
                    uncommitted.set(false);
                }
            }
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
            .padding(8.0)
            .border_top(1.0)
            .border_color(theme::border())
    })
}

// The disabled message box shown when Claude isn't connected — matches the real
// box's metrics but is inert (dim placeholder, no send icon, no pointer events).
fn ai_input_disabled(placeholder: &'static str) -> impl IntoView {
    let box_ = container(text(placeholder).style(|s| {
        s.font_size(theme::FONT_BODY)
            .font_family("IBM Plex Sans".to_string())
            .color(theme::placeholder())
    }))
    .style(|s| {
        s.width_full()
            .height(34.0)
            .padding_top(9.0)
            .padding_left(CHAT_PAD_H)
            .background(theme::bg_deepest())
            .border(1.0)
            .border_color(theme::field_border())
            .border_radius(6.0)
    });
    container(box_)
        .style(|s| {
            s.width_full()
                .padding(8.0)
                .border_top(1.0)
                .border_color(theme::border())
        })
        .pointer_events(|| false)
}

// One chat bubble, styled by role. User messages are plain text; assistant/
// error turns render their segments (prose as light markdown, tool calls as
// chips) plus a cost footer; pending renders "Thinking…".
fn message_bubble(
    m: ChatMessage,
    actions: CodeActions,
    elapsed_ms: RwSignal<u64>,
    is_last: bool,
    regenerate: Rc<dyn Fn()>,
    animate: bool,
) -> impl IntoView {
    let is_user = m.role == Role::User;
    let label_txt = if is_user { "You" } else { "Claude" };

    let body: AnyView = if is_user {
        // User's own message: a dim recap.
        text(m.text)
            .style(|s| s.width_full().font_size(14.0).color(theme::text_muted()))
            .into_any()
    } else {
        // Assistant turn: "Thinking…" until the first token, then the streamed
        // segments — with a footer underneath (a live elapsed timer while the turn
        // runs, swapped for the final cost/token summary + actions once it finishes).
        // Prose only — tool chips are the assistant *using* tools, not content.
        let copy_text = m.prose();
        let content: AnyView = if m.pending && m.segs.is_empty() {
            verb_spinner(theme::text_muted, 14.0).into_any()
        } else {
            render_segments(m.segs, m.role, actions).into_any()
        };
        let footer = assistant_footer(
            m.pending, m.stats, elapsed_ms, copy_text, is_last, regenerate,
        );
        v_stack((content, footer))
            .style(|s| s.flex_col().width_full())
            .into_any()
    };

    let label = text(label_txt).style(|s| s.font_size(theme::FONT_LABEL).color(theme::text_dim()));

    let bubble = if is_user {
        // Right-aligned: the label sits at the bubble's right edge (15px inset);
        // the bubble is inset 60px left / 15px right so it reads right-aligned.
        v_stack((
            h_stack((empty().style(|s| s.flex_grow(1.0_f32)), label))
                .style(|s| s.width_full().flex_row().padding_right(12.0)),
            container(body).style(|s| {
                s.background(theme::bubble_user_bg())
                    .border_radius(6.0)
                    .padding(10.0)
                    .margin_left(60.0)
                    .margin_right(12.0)
            }),
        ))
        .style(|s| s.flex_col().width_full().gap(2.0))
    } else {
        // Full-width Claude/error bubble; label left-aligned at the bubble's left
        // edge (15px inset).
        v_stack((
            container(label).style(|s| s.padding_left(12.0)),
            container(body).style(|s| {
                s.background(theme::bubble_claude_bg())
                    .border_radius(6.0)
                    .padding(10.0)
                    .margin_horiz(12.0)
            }),
        ))
        .style(|s| s.flex_col().width_full().gap(2.0))
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
fn render_segments(segs: Vec<Seg>, role: Role, actions: CodeActions) -> impl IntoView {
    let error_color = role == Role::Error;
    v_stack_from_iter(segs.into_iter().map(move |seg| match seg {
        Seg::Text(t) => {
            if error_color {
                text(t)
                    .style(|s| {
                        s.width_full()
                            .font_size(theme::FONT_BODY)
                            .color(theme::error())
                    })
                    .into_any()
            } else {
                render_markdown(&t, actions.clone()).into_any()
            }
        }
        Seg::Tool(tc) => tool_chip(tc).into_any(),
    }))
    .style(|s| s.flex_col().gap(6.0).width_full())
}

/// A footer action icon (copy / regenerate): 16px, footer-text colour, brightens
/// on hover. No pointer cursor (native feel — see CLAUDE.md).
fn footer_icon(svg: &'static str, on_click: impl Fn() + 'static) -> impl IntoView {
    container(icons::icon(svg, 16.0))
        .on_click_stop(move |_| on_click())
        .style(|s| {
            s.items_center()
                .color(theme::text_muted())
                .hover(|s| s.color(theme::text()))
        })
}

/// Footer under an assistant turn, below a 1px rule: on the left a live elapsed
/// timer (while pending) or the final `time · ↑in ↓out` summary; on the right the
/// Copy action (every done turn) and Regenerate (last turn only), 10px from the
/// edge, 7px apart. Nothing at all when the turn is empty.
fn assistant_footer(
    pending: bool,
    stats: Option<TurnStats>,
    elapsed_ms: RwSignal<u64>,
    copy_text: String,
    is_last: bool,
    regenerate: Rc<dyn Fn()>,
) -> AnyView {
    let style = |s: floem::style::Style| s.font_size(theme::FONT_LABEL).color(theme::text_muted());
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
        if is_last {
            h_stack((copy, footer_icon(icons::REFRESH_CW, move || (regenerate)())))
                .style(|s| s.flex_row().items_center().gap(10.0))
                .into_any()
        } else {
            copy.into_any()
        }
    } else {
        empty().into_any()
    };

    // 1px rule spanning the bubble's text column (aligned with the body text): 5px
    // gap above, 5px between the rule and the row. Row: left content, then the
    // actions pushed to the right edge.
    let row = h_stack((left, empty().style(|s| s.flex_grow(1.0_f32)), actions))
        .style(|s| s.width_full().flex_row().items_center());
    container(row)
        .style(|s| {
            s.width_full()
                .margin_top(5.0)
                .border_top(1.0)
                .border_color(theme::border())
                .padding_top(5.0)
        })
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
        text(dot).style(move |s| s.font_size(9.0).color(dot_color)),
        text(name).style(|s| {
            s.font_size(theme::FONT_LABEL)
                .font_bold()
                .font_family("monospace".to_string())
                .color(theme::text_dim())
        }),
    ))
    .style(|s| s.flex_row().items_center().gap(6.0));

    let sql_view = match tc.sql.clone() {
        Some(sql) => text(sql.trim().to_string())
            .style(|s| {
                s.width_full()
                    .font_family("monospace".to_string())
                    .font_size(theme::FONT_BODY)
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
                    .font_size(theme::FONT_LABEL)
                    .color(c)
                    .padding_top(4.0)
                    .border_top(1.0)
                    .border_color(theme::border())
                    .margin_top(4.0)
            })
            .into_any(),
        None => empty().into_any(),
    };

    v_stack((header, sql_view, result_view)).style(|s| {
        s.flex_col()
            .gap(4.0)
            .width_full()
            .padding(8.0)
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
