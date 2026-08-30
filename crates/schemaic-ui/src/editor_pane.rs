//! The center editor pane: the SQL editor (Floem's editor engine) plus everything
//! layered over it — the Ctrl+K inline-AI bar and verdict footer (`cmdk_popup`),
//! the run/AI anchored menus, the statement-highlight + syntax-squiggle overlays
//! and their geometry helpers
//! (`underline_seg`/`highlight_pick`/`statement_line_boxes`/`wavy_svg`), the
//! catalog-aware diagnostics bridge (`compute_diagnostics` → `schemaic_core::
//! intel`), and the custom overlay scrollbars. `query_pane` is the entry point
//! wired into `center`;
//! `editor_placeholder` is the no-tab fallback. Autocomplete lives in
//! `completion`, statement/guard logic in `schemaic_core::sql`.
//!
//! The Ctrl+K *suggestion* is deliberately not in that list: it is drawn inside
//! the editor's own line flow as phantom rows (`crate::inline_diff`), not layered
//! over it, so only the bar and the footer are overlays here.

use std::rc::Rc;

use floem::event::{Event, EventListener, EventPropagation};
use floem::keyboard::{Key, NamedKey};
use floem::kurbo::{Point, Rect, Vec2};
use floem::prelude::*;
use floem::reactive::{Memo, create_effect, create_memo};
use floem::style::CursorStyle;
use floem::unit::Px;
use floem::views::editor::Editor;
use floem::views::editor::command::{Command, CommandExecuted};
use floem::views::editor::core::buffer::rope_text::RopeText;
use floem::views::editor::core::command::EditCommand;
use floem::views::editor::core::cursor::CursorAffinity;
use floem::views::editor::core::editor::EditType;
use floem::views::editor::core::indent::IndentStyle;
use floem::views::editor::core::selection::Selection;
use floem::views::editor::gutter::GutterClass;
use floem::views::editor::keypress::default_key_handler;
use floem::views::editor::keypress::key::KeyInput;
use floem::views::editor::text::WrapMethod;
use floem::views::scroll::{Handle, Thickness};
use schemaic_core::connection::ConnStatus;

use schemaic_core::diff::{self, inline_plan, line_span};
use schemaic_core::intel::{self, Diagnostic, Severity, SqlDialect};
use schemaic_core::model::QueryState;
use schemaic_core::pairs::{self, PairAction};
use schemaic_core::params;
use schemaic_core::prompt::{self, FixOrigin};
use schemaic_core::sql::{statement_range, statement_ranges};
use schemaic_core::text_ops::{
    find_matches, matches_at, move_line, offset_of_line, replace_all, soft_tab_indent,
    soft_tab_outdent, toggle_line_comment,
};

use crate::completion::{
    Completion, CompletionCtx, accept_completion, completion_popup, recompute_completions,
    signature_popup, types_a_character, update_signature_help,
};
use crate::consts::*;
use crate::inline_diff;
use crate::widgets::*;
use crate::{
    ConnNode, CtxMenu, FieldCfg, InlineAiRequest, InlineAiState, NavKeys, PopupAnchor, RightPanel,
    ValidateDoneFn, ValidateFn, bg_transparent, edit_field, icons, reveal_ai_panel, sql_highlight,
    theme, thumb_len,
};

/// Editor font-zoom (Ctrl+scroll): px bounds + per-notch step. Temporary, per-tab.
const ZOOM_MIN: f32 = 6.0;
const ZOOM_MAX: f32 = 48.0;
const ZOOM_STEP: f32 = 1.0;

// ===== moved from lib.rs (editor pane) =====
// Stand-in shown where the query editor sits while a tab flashes closed. Same
// footprint as `query_pane`'s outer box (see EDITOR_H there) — just the editor
// surface color — so the results grid below never moves.
pub(crate) fn editor_placeholder(
    editor_h: RwSignal<f64>,
    editor_collapsed: RwSignal<bool>,
) -> impl IntoView {
    empty().style(move |s| {
        let collapsed = editor_collapsed.get();
        let h = crate::consts::effective_editor_h(editor_h.get(), collapsed);
        let s = s
            .width_full()
            .height(h)
            .min_height(h)
            .min_width(0.0)
            .flex_shrink(0.0_f32)
            .background(theme::bg_editor())
            .border_color(theme::border());
        if collapsed {
            s.border_bottom(0.0)
        } else {
            s.border_bottom(1.0)
        }
    })
}

/// Track height, thumb height and maximum scroll for a vertical scrollbar over
/// `lines` rows of `line_h` in a `viewport_h`-tall viewport — or `None` when
/// there is nothing to scroll, which is what hides the bar.
///
/// Lifted out of the editor's `v_geo` so the arithmetic can be asserted: the bug
/// it carried was in the *input* (buffer lines instead of visual ones), but the
/// threshold and the thumb ratio had never been tested either.
///
/// **The scrollable height is not the text height.** The editor runs with
/// `ScrollBeyondLastLine`, so Floem lays its content out with the virtual space
/// under it — [`body_scroll_h`], which the results grid is sized by too.
/// Measured against the text alone the thumb would hit the bottom of the track a
/// whole viewport before the wheel ran out, and a document that merely *fits*
/// would show no bar at all while still scrolling.
fn scrollbar_geo(lines: usize, line_h: f64, viewport_h: f64) -> Option<(f64, f64, f64)> {
    let content_h = body_scroll_h(lines as f64 * line_h, viewport_h, line_h);
    if content_h <= viewport_h + 1.0 || viewport_h <= 0.0 {
        return None;
    }
    let thumb_h = thumb_len(viewport_h / content_h * viewport_h, viewport_h);
    Some((viewport_h, thumb_h, (content_h - viewport_h).max(1.0)))
}

/// Apply an edit the user did **not** type, without popping the completion list.
///
/// Every document change re-runs the completion recompute a tick later, and that
/// recompute has no way to tell a keystroke from a Replace, a comment toggle, a
/// line move or a reformat. So clicking **Replace** in the find bar opened a list
/// of every table in the database, anchored at line 1, while the find counter
/// read `0/0` — a suggestion list for a caret the user had not put there.
///
/// `comp.suppress` is the one-shot the accept path already uses so its own splice
/// doesn't re-open the popup over the word it just inserted. This is the one
/// place that remembers to set it, which is the difference between a fix and a
/// rule every future programmatic edit has to be told about. **Typed** edits —
/// auto-pair insertion, the paired backspace — deliberately do not come through
/// here: there the popup *should* follow the caret.
fn edit_untyped(ed: &Editor, comp: Completion, sel: Selection, text: &str, ty: EditType) {
    comp.suppress.set(true);
    ed.doc().edit_single(sel, text, ty);
}

/// Reformat the SQL in `ed` (the Ctrl+Alt+L action, also the editor's right-click
/// "Format SQL"). Formats the current selection if there is one, else the whole
/// document; indentation follows the editor's tab-width / soft-tabs settings and
/// keyword case is preserved. Applied as one `edit_single` (a single undo step);
/// a no-op when the text is already formatted.
fn format_editor(ed: &Editor, comp: Completion, dialect: SqlDialect) {
    let doc = ed.doc();
    let full = doc.text().to_string();
    let (a, b) = ed.cursor.get_untracked().get_selection().unwrap_or((0, 0));
    let (sel_lo, sel_hi) = (a.min(b), a.max(b));
    let (start, end) = if sel_lo != sel_hi {
        (sel_lo, sel_hi)
    } else {
        (0, full.len())
    };
    let unit = if theme::editor_soft_tabs() {
        " ".repeat(theme::editor_tab_width())
    } else {
        "\t".to_string()
    };
    let formatted = schemaic_core::sqlfmt::format_sql(&full[start..end], &unit, dialect);
    if formatted == full[start..end] {
        return;
    }
    edit_untyped(
        ed,
        comp,
        Selection::region(start, end),
        &formatted,
        EditType::Other,
    );
    let caret = start + formatted.len();
    ed.cursor.update(|cc| cc.set_offset(caret, false, false));
}

// The inline-AI suggestion is rendered *in the editor's line flow* by
// `crate::inline_diff` (phantom rows) plus `sql_highlight`'s row backgrounds, not
// by a view here. `schemaic_core::diff::inline_plan` is the pure half that says
// which document lines it touches; `cmdk_popup` below draws only the question bar
// and the verdict footer that close the block.

#[allow(clippy::too_many_arguments)] // a UI builder; grouping into a struct adds no clarity
fn cmdk_popup(
    cmdk: CmdK,
    inline_ai: RwSignal<InlineAiState>,
    run: Rc<dyn Fn(InlineAiRequest)>,
    cancel: Rc<dyn Fn()>,
    query: RwSignal<String>,
    ed: Editor,
    // Approving a diff is an edit nobody typed, so it goes through
    // `edit_untyped` — which needs the completion state to suppress the popup.
    comp: Completion,
    // Editor-area height, so the verdict bar can tell when the block it closes has
    // scrolled out of the pane and take itself off screen with it.
    area_h: RwSignal<f64>,
) -> impl IntoView {
    // The editor's own view id, so closing the overlay can hand focus back to
    // the editor (else focus is left dangling after the input is torn down).
    let editor_view_id = ed.editor_view_id;
    // The editor's own scroll, so the verdict bar can forward a wheel to it.
    let scroll_delta = ed.scroll_delta;

    /// The question field opens as one line and grows with the question to this
    /// many rows, then scrolls. Three, because the bar sits *between* two lines of
    /// the user's own SQL — a taller one stops reading as an annotation on the
    /// statement above it and starts reading as a panel that has displaced it.
    const CMDK_MAX_ROWS: usize = 3;
    // `edit_field` takes the cap as a signal so it can follow a resizing
    // container; this one is fixed, and owned out here so the `dyn_container`
    // rebuilding the field on every state change can't dispose it.
    let cmdk_rows = RwSignal::new(CMDK_MAX_ROWS);

    // Publish the suggestion into the editor's own line flow (and take it back
    // down again). An *effect* over the state rather than a set inside each
    // transition, because every way out of `Ready` has to remove the phantom
    // rows — approve, reject, Escape, a second Ctrl+K, a tab switch that
    // cancels the generation — and those don't share a single caller to hang it
    // off. Anything that isn't a settled suggestion clears the preview, so the
    // rows cannot outlive the state that justified them.
    {
        let ed_preview = ed.clone();
        create_effect(move |_| {
            let state = inline_ai.get();
            let settled = matches!(state, InlineAiState::Ready(_));
            // The buffer and the trigger range, read **only** by the two states
            // that need them. Idle and Failed are the common transitions — every
            // Escape, Accept, Reject and Ctrl+K passes through one — and copying a
            // whole 190KB script to answer them cost more than the feature does.
            //
            // The **document's** text, not `query`: everything below resolves byte
            // offsets that were captured against the document, and resolving those
            // against anything else is precisely the bug that made Ctrl+K pick the
            // wrong statement when the signal fell out of step. `query` is in step
            // again, but the offsets belong to the rope, so the rope answers them —
            // the same rule `accept` and the Ctrl+K widening follow.
            let acted_on = || {
                let full = ed_preview.doc().text().to_string();
                // Clamp to char boundaries: the trigger range is a byte range
                // captured earlier and the doc may have changed under it, so a
                // naive slice can land mid-codepoint and panic.
                let end = floor_char_boundary(&full, cmdk.end.get_untracked().min(full.len()));
                let start = floor_char_boundary(&full, cmdk.start.get_untracked().min(end));
                (full, start, end)
            };
            let view = match state {
                // Fade the lines the request is about while it is in flight, so
                // the editor shows what is being worked on. Line numbers, not
                // byte offsets — that is what the styling hook is keyed on.
                InlineAiState::Busy => {
                    let (full, start, end) = acted_on();
                    Some(inline_diff::InlineView::Working(line_span(
                        &full, start, end,
                    )))
                }
                InlineAiState::Ready(sql) => {
                    let (full, start, end) = acted_on();
                    // `inline_splice`, not a `format!` here and another in
                    // `accept`: the preview and the splice have to be the same
                    // decision, or a CRLF buffer plans empty while Accept rewrites
                    // every line ending in the range.
                    let (_, new_full) = diff::inline_splice(&full, start, end, &sql);
                    let plan = inline_plan(&full, &new_full);
                    // A suggestion identical to what the user already has has no
                    // rows to draw; leaving it unset keeps the editor untouched
                    // and lets the footer say so instead.
                    (!plan.is_empty()).then_some(inline_diff::InlineView::Plan(plan))
                }
                _ => None,
            };
            // **Does this pane own the request?** `inline_ai` is one global
            // signal and `CmdK` is per pane — see `inline_pane_action`.
            let act = inline_pane_action(cmdk.open.get_untracked(), settled);
            inline_diff::set_preview(
                cmdk.preview,
                &ed_preview,
                act.draw.then_some(view).flatten(),
            );
            // Freeze the buffer while a suggestion is on screen. The phantom rows
            // are anchored to line *numbers* and the plan was computed against the
            // text as it was, so an edit underneath would leave the rows sitting on
            // lines they no longer describe and Accept splicing at offsets that
            // have moved. The old overlay got this for free by covering the editor;
            // this one deliberately does not cover it, so the freeze has to be
            // asked for. Accept or reject to get the buffer back.
            ed_preview.read_only.set(act.freeze);
            // The verdict state has no field in it, so focus would be left dangling
            // where the prompt used to be. Hand it back to the editor, which is
            // both where the suggestion now is and what answers Enter/Escape for
            // it. Deferred, because the field is torn down on this same tick.
            if act.focus {
                floem::action::exec_after(std::time::Duration::from_millis(0), move |_| {
                    if let Some(Some(vid)) = editor_view_id.try_get_untracked() {
                        // Claimed for the same reason the prompt field's autofocus
                        // is: a menu-opened request whose reply lands promptly
                        // reaches here with the menu's own deferred hand-back
                        // still queued behind it (`widgets::claim_keyboard`).
                        crate::widgets::claim_keyboard();
                        vid.request_focus();
                    }
                });
            }
        });
    }

    // These three all mutate the state that drives the overlay's `dyn_container`,
    // which tears the prompt field down. Since they're invoked from INSIDE the
    // editor's key handler (Enter/Escape), doing that synchronously would dispose
    // the field while the editor is still mid-handler on the stack → it then
    // reads its own disposed signals and panics. So each defers its body one tick
    // (`exec_after(0)`), after the key handler has unwound.
    let discard = {
        let cancel = cancel.clone();
        move || {
            // Abort any in-flight generation (no-op if none) + reset to Idle,
            // close the overlay, and return focus to the editor.
            let cancel = cancel.clone();
            floem::action::exec_after(std::time::Duration::from_millis(0), move |_| {
                // Bail if the tab (this `cmdk`'s scope) closed in the same tick.
                if cmdk.open.try_get_untracked().is_none() {
                    return;
                }
                // Close FIRST (→ overlay renders `empty()`), so `cancel`'s reset
                // to Idle can't briefly re-create a compact prompt field.
                cmdk.open.set(false);
                (cancel)();
                if let Some(Some(vid)) = editor_view_id.try_get_untracked() {
                    crate::widgets::claim_keyboard();
                    vid.request_focus();
                }
            });
        }
    };
    let submit = {
        let run = run.clone();
        move || {
            let run = run.clone();
            floem::action::exec_after(std::time::Duration::from_millis(0), move |_| {
                let Some(typed) = cmdk.input.try_get_untracked() else {
                    return; // tab closed in the same tick
                };
                if typed.trim().is_empty() {
                    return;
                }
                // The app's own instruction wins over the label in the box, when
                // the box was filled in by the app — see `CmdK::intent`. This is
                // what makes a *retry* of a failed AI fix send the fenced,
                // provenance-flagged prompt rather than `Fix this error: <server
                // text>` interpolated into Schemaic's own prose.
                let intent = match cmdk.intent.try_get_untracked().flatten() {
                    Some((label, instruction)) if label == typed => instruction,
                    _ => typed,
                };
                let current_sql = query.get_untracked();
                let (s, e) = (cmdk.start.get_untracked(), cmdk.end.get_untracked());
                let selection = if s != e {
                    current_sql.get(s..e).map(|x| x.to_string())
                } else {
                    None
                };
                inline_ai.set(InlineAiState::Busy);
                (run)(InlineAiRequest {
                    intent,
                    current_sql,
                    selection,
                });
            });
        }
    };
    let accept = {
        let ed = ed.clone();
        move || {
            let ed = ed.clone();
            floem::action::exec_after(std::time::Duration::from_millis(0), move |_| {
                // Bail if the tab (this `cmdk`/`ed` scope) closed in the same tick.
                if cmdk.open.try_get_untracked().is_none() {
                    inline_ai.set(InlineAiState::Idle);
                    return;
                }
                if let InlineAiState::Ready(sql) = inline_ai.get_untracked() {
                    let (s, e) = (cmdk.start.get_untracked(), cmdk.end.get_untracked());
                    // Revalidate byte offsets against the CURRENT doc and clamp to
                    // char boundaries (C12): the doc may have changed since trigger,
                    // and raw byte slicing panics mid-codepoint.
                    let doc = ed.doc();
                    let full = doc.text().to_string();
                    let s = floor_char_boundary(&full, s);
                    let e = floor_char_boundary(&full, e);
                    // The same `inline_splice` the preview used, so what lands is
                    // what the plan described — line endings included.
                    let (text, _) = diff::inline_splice(&full, s, e, &sql);
                    edit_untyped(&ed, comp, Selection::region(s, e), &text, EditType::Paste);
                    ed.cursor
                        .update(|c| c.set_offset(s + text.len(), false, false));
                }
                cmdk.open.set(false);
                inline_ai.set(InlineAiState::Idle);
            });
        }
    };

    // Hand the verdict to the editor's key handler (see `CmdK::verdict`). Both
    // closures already defer their bodies a tick, which is exactly what being
    // called from inside that handler requires.
    cmdk.verdict.set(Some((
        Rc::new(accept.clone()) as Rc<dyn Fn()>,
        Rc::new(discard.clone()) as Rc<dyn Fn()>,
    )));

    // The two shapes the overlay has: closed, asking (Idle/Busy/Failed), or
    // settled. See the key comment below for why this is a `Memo` and not the
    // closure it looks like it could be.
    let verdict_shape = create_memo(move |_| {
        (
            cmdk.open.get(),
            matches!(inline_ai.get(), InlineAiState::Ready(_)),
        )
    });
    // A request is in flight: the question has been sent and the field is frozen
    // until it lands. Outside the `dyn_container` below, so it outlives the
    // child — and a `Memo` so the field's style re-runs only when the answer to
    // *this* question changes, not on every `inline_ai` transition.
    let asking = create_memo(move |_| matches!(inline_ai.get(), InlineAiState::Busy));
    let content = dyn_container(
        // `ui_generation` used to be in this key: the child resolved two things no
        // style closure could re-read — `diff_view`'s `content_w` (the diff's
        // scroll range, measured from the font at build time) and the text `Attrs`
        // its syntax colouring was built from — so a live scale change left the
        // rows rendering 1.6x inside a `min_width` computed at 100%. Both went
        // with the box: the diff is phantom rows in the editor's own line flow
        // now, laid out by the editor at the editor's own font. What is left here
        // is a bar and a footer whose every metric is read inside a style closure
        // (`theme::scaled(…)`, `icons::icon`'s own closure, `FieldCfg`'s
        // `fn() -> f32` sizes), so nothing is frozen at build time and there is
        // no measurement left for a generation to invalidate.
        // **Keyed on `Ready`-or-not, not on the state.** The prompt field must
        // survive the Idle → Busy transition: rebuilding it re-runs its layout,
        // and for one frame the fresh field has almost no width, so a question
        // that fits on one line wraps — `edit_field` measures its row count from
        // the wrapped layout, latches the wrong number, and the bar opens a row
        // taller with its text stranded at the top. That looked exactly like a
        // newline had been inserted, which is what sent three rounds of fixes at
        // the Enter key; the caret could never actually reach a second line.
        // Everything that differs between Idle, Busy and Failed is reactive
        // *inside* this branch instead.
        //
        // **The key has to be a `Memo`, and that is not a detail.** `dyn_container`
        // rebuilds on every *run* of this closure, not on every change of its value:
        // it wires the closure through `create_updater`, whose callback swaps the
        // child unconditionally (floem `views/dyn_container.rs`). So a key that
        // merely *computed* the same `(open, ready)` still rebuilt the field the
        // moment `inline_ai` changed — writing the key as a tuple bought nothing,
        // and Idle → Busy went on destroying and re-laying-out the field. `Memo`
        // is the piece that makes the key mean what it reads as: it only notifies
        // when the value actually differs (`floem_reactive::create_memo`).
        move || verdict_shape.get(),
        move |(open, ready)| {
            if !open {
                return empty().into_any();
            }
            // Enter accepts a settled suggestion, and otherwise asks — including
            // after a failure, where it retries the question still sitting in the
            // box. It reads the state *when pressed* rather than being rebuilt per
            // state, and does nothing while a request is in flight.
            //
            // **This is never `None`, and that is the point.** A multiline
            // `edit_field` with no `on_submit` does not swallow Enter, it breaks
            // the line — the right default for a body field, and wrong for a
            // one-question box, which is what `enter_never_breaks` now says
            // outright.
            let on_submit: Option<Rc<dyn Fn()>> = Some(if ready {
                let accept_k = accept.clone();
                Rc::new(accept_k) as Rc<dyn Fn()>
            } else {
                let submit_k = submit.clone();
                Rc::new(move || {
                    if !matches!(inline_ai.get_untracked(), InlineAiState::Busy) {
                        submit_k();
                    }
                })
            });
            let discard_esc = discard.clone();
            // One row: the sparkle, the question, and either the send affordance
            // or the spinner. The field is borderless and transparent because the
            // outer container owns the bar's surface (background + accent rule).
            //
            // Multiline with a 3-row cap: the bar opens as a single line and grows
            // with the question to three, then scrolls with the auto-hiding bar the
            // rest of the app uses. That is `edit_field`'s existing chat-compose
            // behaviour, capped lower — not a second auto-grow input.
            let input_row = edit_field(
                cmdk.input,
                FieldCfg {
                    placeholder: "Ask the AI Assistant for help.",
                    background: bg_transparent,
                    font_size: theme::font_body,
                    multiline: true,
                    max_rows: Some(cmdk_rows),
                    // Multiline to wrap and grow with a long question — never to
                    // hold two lines. Enter asks; it does not break the line.
                    enter_never_breaks: true,
                    autofocus: true,
                    // A sent question is dimmed and takes no more typing, and it
                    // is `FieldCfg::frozen` — a signal the *built* field reads —
                    // rather than `read_only` plus a colour, which are resolved
                    // at build and so meant rebuilding the field to change. That
                    // rebuild is what broke the bar's layout, and dropping the
                    // freeze with it left the box editable mid-flight, promising
                    // an edit that could not reach the request already sent.
                    frozen: Some(asking),
                    text_color: Some(theme::cmdk_text),
                    placeholder_color: Some(theme::cmdk_placeholder),
                    border_color: Some(bg_transparent),
                    on_submit,
                    on_escape: Some(Rc::new(discard_esc)),
                    ..Default::default()
                },
            )
            .style(|s| s.flex_grow(1.0_f32).min_width(0.0));
            // The one part of the bar that does swap with the request, so it swaps
            // on its own rather than taking the field down with it.
            let submit_click = submit.clone();
            // The verb is picked HERE, once per opening, rather than inside the
            // slot — because the slot has to reserve the spinner's width before
            // the spinner exists. See the `min_width` below.
            let spinner_verb = pick_spinner_verb();
            let trailing = dyn_container(
                move || matches!(inline_ai.get(), InlineAiState::Busy),
                move |busy| {
                    if busy {
                        // The spinner verb + animated dots the AI panel and the
                        // query runner already use, rather than the design's
                        // pulsing dot — one loader vocabulary across the app.
                        loading_dots(spinner_verb, theme::text_dim, theme::font_hint).into_any()
                    } else {
                        let submit_click = submit_click.clone();
                        container(icons::icon(icons::PLAY_LUCIDE, 15.0))
                            .on_click_stop(move |_| submit_click())
                            .style(|s| {
                                s.items_center()
                                    .color(theme::ai_send_icon())
                                    .cursor(CursorStyle::Default)
                                    .hover(|s| s.color(theme::ai_send_icon_hover()))
                            })
                            .into_any()
                    }
                },
            )
            .style(move |s| {
                s.flex_shrink(0.0_f32)
                    .items_center()
                    // **Hold the spinner's width open while the icon is showing.**
                    // The field beside this slot flex-grows into whatever is left,
                    // so swapping a 15px play icon for an 80px "Pondering..." on
                    // submit took ~70px off the question — the text re-wrapped,
                    // `edit_field`'s row count went 1 → 2, and the box grew a line.
                    // That is what "Enter inserts a newline" was: not the key at
                    // all, which is why clicking the send icon did it too. Both run
                    // the same submit, and the caret never moved.
                    //
                    // `justify_end` keeps the icon on the right edge where it was;
                    // the reserved space opens to its left, over the field's
                    // transparent tail.
                    .justify_end()
                    .min_width(loading_dots_w(spinner_verb, theme::font_hint()))
            });
            let input_row = h_stack((
                container(icons::icon(icons::SPARKLES, 14.0))
                    .style(|s| s.flex_shrink(0.0_f32).color(theme::key_foreign())),
                input_row,
                trailing,
            ))
            .style(|s| {
                s.flex_row()
                    .items_center()
                    .width_full()
                    .min_width(0.0)
                    .gap(theme::scaled(8.0))
                    .padding_left(theme::scaled(6.0))
                    .padding_right(theme::scaled(8.0))
                // No vertical padding of its own: `edit_field` already carries
                // `chat_pad_v` above and below its text, and adding a second helping
                // here is what made a one-line question sit in a bar deep enough to
                // look like two.
            });

            // The failure message, reactive like the trailing slot — a failure must
            // not rebuild the field either.
            let body = dyn_container(
                move || match inline_ai.get() {
                    InlineAiState::Failed(msg) => Some(msg),
                    _ => None,
                },
                move |msg| match msg {
                    None => empty().into_any(),
                    Some(msg) => container(
                        text(msg).style(|s| s.color(theme::error()).font_size(theme::font_body())),
                    )
                    .style(|s| {
                        s.width_full()
                            .padding_horiz(theme::scaled(10.0))
                            .padding_bottom(theme::scaled(6.0))
                    })
                    .into_any(),
                },
            )
            .style(|s| s.width_full().min_width(0.0));

            // With a suggestion on screen the question has already been answered
            // and the answer is in the editor itself: the bar goes away and the
            // verdict footer is the whole overlay. The suggestion itself is not
            // drawn here at all — it is in the editor's own line flow, as phantom
            // rows (`inline_diff`), put there by the effect at the top of this
            // function.
            let content = if ready {
                {
                    let discard_b = discard.clone();
                    let accept_b = accept.clone();
                    let summary = match cmdk.preview.get().as_ref().and_then(|v| v.plan()) {
                        Some(p) => format!(
                            "{} hunk{} · {} − / {} +",
                            p.hunks.len(),
                            if p.hunks.len() == 1 { "" } else { "s" },
                            p.removed,
                            p.added
                        ),
                        None => "No changes suggested".to_string(),
                    };
                    // Floem gives no way to pass a pointer event on once a view is
                    // eligible for it — the child walk `break`s on the first such
                    // view, handled or not (`context.rs:143-158`) — so a 26px bar
                    // lying across the document would kill scrolling over itself.
                    // Forward the wheel to the editor's own scroll instead, which
                    // is what Floem's gutter does with the same problem
                    // (`view.rs:1112`).
                    //
                    // **Every view in here that takes pointer events needs this,
                    // not just the bar.** The `break` happens at whichever child
                    // is under the pointer, so with the handler only on the row,
                    // scrolling worked over the bar but died over the two words —
                    // which reads as "sometimes it swallows the scroll".
                    let fwd_wheel = move |e: &Event| {
                        if let Event::PointerWheel(pe) = e {
                            scroll_delta.set(pe.delta);
                        }
                    };
                    h_stack((
                        container(text("Accept"))
                            .on_click_stop(move |_| accept_b())
                            .on_event_stop(EventListener::PointerWheel, fwd_wheel)
                            .style(|s| {
                                s.color(theme::diff_add_marker())
                                    .font_size(theme::font_body())
                                    .cursor(CursorStyle::Default)
                                    .hover(|s| {
                                        s.color(theme::diff_add_marker().multiply_alpha(0.8))
                                    })
                            }),
                        container(text("Reject"))
                            .on_click_stop(move |_| discard_b())
                            .on_event_stop(EventListener::PointerWheel, fwd_wheel)
                            .style(|s| {
                                s.color(theme::diff_del_marker())
                                    .font_size(theme::font_body())
                                    .cursor(CursorStyle::Default)
                                    .hover(|s| {
                                        s.color(theme::diff_del_marker().multiply_alpha(0.8))
                                    })
                            }),
                        empty().style(|s| s.flex_grow(1.0_f32)),
                        text(summary)
                            .style(|s| s.color(theme::text_muted()).font_size(theme::font_body()))
                            .on_event_stop(EventListener::PointerWheel, fwd_wheel),
                    ))
                    .style(move |s| {
                        s.flex_row()
                            .items_center()
                            .width_full()
                            .min_width(0.0)
                            .height_full()
                            .gap(theme::scaled(12.0))
                            // Aligns the verdict with the code column, so the bar
                            // reads as the block's last row.
                            .padding_left(content_x_of(&query.get()))
                            .padding_right(theme::scaled(10.0))
                            // Nothing here is selectable, so the editor's I-beam
                            // (which this sits on top of) would be a lie.
                            .cursor(CursorStyle::Default)
                    })
                    .on_event_stop(EventListener::PointerWheel, fwd_wheel)
                    .into_any()
                }
            } else {
                v_stack((input_row, body))
                    .style(|s| s.flex_col().width_full().min_width(0.0))
                    .into_any()
            };
            // Clip the INNER content (not the absolute outer container — clipping
            // that hides the whole overlay) so long input/diff text stays inside
            // the border. Belt-and-suspenders over the per-element clips below.
            container(content)
                .style(|s| {
                    // `justify_center` is what actually centres the row in the
                    // verdict bar. The row asks for `height_full` + `items_center`,
                    // but this wrapper is the one that fills the box's definite
                    // height, and its default `justify_start` pinned the row to the
                    // top — which is why the words hugged the bar's top edge and
                    // why the empty space below them belonged to a view with no
                    // wheel handler.
                    s.flex_col()
                        .justify_center()
                        .width_full()
                        .height_full()
                        .min_width(0.0)
                        .min_height(0.0)
                })
                .clip()
                .into_any()
        },
    )
    .style(|s| {
        s.width_full()
            .height_full()
            .flex_col()
            .min_width(0.0)
            .min_height(0.0)
    });

    // The absolute box lives on a STABLE `container` (a real flex parent that
    // stretches its child to the box's definite size). A `dyn_container` styled
    // absolute sizes to its child instead, so `height_full`/`flex_grow` inside
    // never resolved against a definite height — that's why a tall diff
    // overflowed and pushed the buttons off. With this wrapper the height chain
    // resolves and the diff scroll bounds + scrolls correctly.
    let ed_geo = ed.clone();
    container(content)
        .style(move |s| {
            if !cmdk.open.get() {
                return s;
            }
            // Two shapes, both anchored into the editor's line flow rather than
            // floating over it. Asking + working is a bar directly under the
            // statement being acted on; the verdict is a footer directly under the
            // phantom rows the suggestion added. Neither covers the editor any more,
            // which is the whole point of the redesign — so there is nothing left to
            // animate between, and the old compact↔expanded transition is gone with
            // the box it was growing.
            // Both bars are inset 1px from the editor's left and right edges so they
            // sit *inside* its border rather than painting over it.
            let s = s
                .absolute()
                .flex_col()
                .justify_center()
                .inset_left(1.0)
                .inset_right(1.0);
            match inline_ai.get() {
                InlineAiState::Ready(_) => {
                    // Directly below the block. `inline_footer_y` returns `None` when
                    // the block is not fully on screen, and the bar is parked off the
                    // top rather than left floating over the toolbar.
                    let y = inline_footer_y(&ed_geo, cmdk.preview, area_h).unwrap_or(-9999.0);
                    s.inset_top(y)
                        .height(theme::scaled(VERDICT_BAR_H))
                        // **Asymmetric on purpose**, and the two numbers are the
                        // bar's whole geometry: 1px border, then a 21px band for the
                        // words with 7px above it and 1px below.
                        //
                        // The row inside is centred in that band, so an even split
                        // centres the words' *line box* — and a line box carries
                        // descender space below the glyphs that almost nothing in
                        // "Accept · Reject · 1 hunk" fills. Geometrically centred
                        // therefore reads high. Spending the padding at the top drops
                        // the glyphs onto the optical centre instead. Scaled like
                        // every other metric here, so the split tracks the interface
                        // scale rather than drifting at 130%.
                        .padding_top(theme::scaled(7.0))
                        .padding_bottom(theme::scaled(1.0))
                        .background(theme::bg_deepest())
                        .border_top(1.0)
                        .border_color(theme::border())
                }
                _ => {
                    // `p.y` is the bottom edge of the statement's last line in the
                    // editor's **content** coordinates, so the viewport has to come off
                    // it — without that the bar was placed by how far down the document
                    // the statement is rather than by where it is on screen, and opening
                    // Ctrl+K in a scrolled editor put the bar at the bottom of the pane.
                    let p = cmdk.point.get();
                    s.inset_top(p.y - ed_geo.viewport.get().y0 + theme::scaled(4.0))
                        .background(theme::bg_deepest())
                        .border_left(2.0)
                        .border_color(theme::accent())
                        .border_radius(0.0)
                }
            }
        })
        // On the outer box, not only on the row inside it. The row is content-height
        // and centred, so the bar's padding, its border and the space above and below
        // the words all belong to *this* view — and those were exactly the places a
        // scroll died: it worked across the words' own band and nowhere else.
        .on_event_stop(EventListener::PointerWheel, move |e| {
            if let Event::PointerWheel(pe) = e {
                scroll_delta.set(pe.delta);
            }
        })
}

/// What "Optimize" actually asks the model, as opposed to the two-word label the
/// prompt box shows. One constant, because the launch and a **retry** of it have
/// to send the same thing — see [`CmdK::intent`].
const OPTIMIZE_INTENT: &str = "Rewrite this SQL query to be more efficient and readable while      preserving its exact result set. Return only the SQL.";

/// Editor-local state for the inline (Ctrl+K) AI prompt popup. `start`/`end` are
/// the doc byte-range captured at trigger time — equal ⇒ generate/insert at the
/// caret; distinct ⇒ transform that selection. `point` anchors the popup.
#[derive(Clone, Copy)]
struct CmdK {
    open: RwSignal<bool>,
    point: RwSignal<Point>,
    input: RwSignal<String>,
    /// `(the label the app put in the box, the instruction to actually send)`,
    /// when the box was filled in by the app rather than typed by the user.
    ///
    /// **`input` is a label; this is the prompt.** `prompt::ai_fix_prompt`
    /// returns both, and only its `intent` carries the fence and the
    /// `UNTRUSTED_NOTE`: the error text it quotes is server-controlled, and that
    /// module's own doc says that is exactly why it must not be interpolated into
    /// Schemaic's prose. The box holds `Fix this error: <server text>`, and a
    /// *retry* — the generation failed, the field comes back with that line still
    /// in it, Enter — sent it as the instruction, unfenced and unflagged.
    ///
    /// The **label** is kept rather than a "has the user typed" flag so that no
    /// effect has to watch the field: `submit` uses the instruction exactly while
    /// the box still reads what the app wrote, and the first keystroke makes the
    /// two differ.
    intent: RwSignal<Option<(String, String)>>,
    start: RwSignal<usize>,
    end: RwSignal<usize>,
    /// The suggestion currently rendered *in the editor's line flow* as phantom
    /// rows, or `None`. Set only through [`inline_diff::set_preview`], which also
    /// invalidates the layout cache the rows are baked into.
    preview: inline_diff::InlinePreview,
    /// `(accept, reject)`, published by `cmdk_popup` once it has built them.
    ///
    /// The suggestion is drawn in the editor's own lines now, so the verdict state
    /// has no text field in it — and a field is what used to catch Enter and
    /// Escape. The editor has focus instead, so its key handler answers for them,
    /// and this is how it reaches the same two closures the footer's buttons call.
    /// One implementation, two ways in; not a second copy of the logic.
    verdict: RwSignal<Option<Verdict>>,
}

/// `(accept, reject)` for a settled Ctrl+K suggestion — see [`CmdK::verdict`].
type Verdict = (Rc<dyn Fn()>, Rc<dyn Fn()>);

// ── Unsafe-statement guard ───────────────────────────────────────────────────
//
// A DELETE or UPDATE with no WHERE clause rewrites/erases every row — almost
// always a mistake, so it is caught before running and confirmed.
//
// The guard itself is **not here**. It lives on the run action (the app's
// `guarded_run`/`guarded_run_all`, over `schemaic_core::sql::run_verdict`), and
// this pane only renders what it held back — `ui.overlay.run_guard`, arriving as
// `QueryPaneParams::run_guard`, drawn by `guard_bar`. It used to be two closures
// in this view body, which is precisely why the command palette and the AI chat
// could run writes past all three protections: a guard in one caller of a shared
// action is a guard the next caller opts out of by omission. Don't move it back.

// ── Query parameters bar ─────────────────────────────────────────────────────
//
// One row per distinct `:name` in the tab's SQL, under the editor and above the
// results. It is a *bar*, not a modal asking on every run: values stay put while
// you re-run, which is the whole difference between this and the dialog other
// clients pop up each time.
//
// Like the guard bar, the decision isn't here — `params::prepare_run` on the run
// action substitutes and judges. This view only collects values into the tab's
// store.

/// The parameters bar for a tab, or nothing when its SQL has no placeholders.
fn params_bar(
    query: RwSignal<String>,
    store: RwSignal<Vec<params::Binding>>,
    dialect: Memo<SqlDialect>,
) -> impl IntoView {
    // Rows follow the **names**, not the values. A memo over the values would
    // rebuild the row being typed into on every keystroke, and floem takes the
    // focus and the caret with a rebuilt view.
    let names = create_memo(move |_| query.with(|q| params::names(q, dialect.get())));
    let list = dyn_stack(
        move || names.get(),
        |name: &String| name.clone(),
        move |name| param_row(name, store),
    )
    .style(|s| s.flex_row().items_center().gap(theme::scaled(14.0)));

    // One view, styled away when there is nothing to show, rather than a
    // `dyn_container` that rebuilds the whole bar — and with it every value
    // field — each time the query gains or loses its last placeholder.
    //
    // `wheel_hscroll`, the tab strip's scroller: no bar at all, and the plain
    // wheel pans sideways. This bar has no vertical axis of its own, so a wheel
    // over it would otherwise do nothing while a row sat off the right edge, and
    // an autohiding bar under a row of 24px fields is chrome taller than the gap
    // it lives in.
    container(wheel_hscroll(list).style(|s| s.width_full())).style(move |s| {
        if names.get().is_empty() {
            return s.height(0.0).width_full();
        }
        s.width_full()
            .height(theme::scaled(36.0))
            .flex_shrink(0.0_f32)
            .items_center()
            .padding_horiz(theme::scaled(13.0))
            .background(theme::bg_panel())
            .border_top(1.0)
            .border_color(theme::border())
    })
}

/// One parameter: its name, a value field, and the kind chip that says how the
/// value reaches the SQL.
fn param_row(name: String, store: RwSignal<Vec<params::Binding>>) -> impl IntoView {
    // Seeded once, from whatever the store already holds for this name — the
    // `bound_signal` shape the table designer uses. Seeding through the effect
    // would write the store back to the value it already has, and `set` never
    // dedups.
    let existing = store.with_untracked(|s| {
        s.iter()
            .find(|b| b.name == name)
            .and_then(|b| b.value.clone())
    });
    let kind = RwSignal::new(
        existing
            .as_ref()
            .map_or_else(Default::default, |v| v.kind()),
    );
    let value = RwSignal::new(
        existing
            .as_ref()
            .map_or_else(String::new, |v| v.text().to_string()),
    );
    // An untouched row writes nothing, and *that* — no entry in the store — is
    // what holds the run. An empty string can't be the test for an unanswered
    // row, because `''` is a legitimate value someone may mean.
    let write_name = name.clone();
    create_effect(move |prev: Option<(String, params::ParamKind)>| {
        let now = (value.get(), kind.get());
        if prev.is_some_and(|p| p != now) {
            let bound = params::ParamValue::of(now.1, &now.0);
            store.update(|s| params::set_value(s, &write_name, Some(bound)));
        }
        now
    });

    let name_label = text(format!(":{name}")).style(|s| {
        s.font_family(MONO_FAMILY.to_string())
            .font_size(theme::font_body())
            .color(theme::text_dim())
            .flex_shrink(0.0_f32)
    });

    // `NULL` has no text of its own, so the field goes away entirely rather than
    // standing there holding a value that no longer reaches the SQL. Nothing is
    // lost by hiding it: the chip beside it already says `NULL`, and the text is
    // still in `value`, so it comes back with the field when the kind changes
    // again. A greyed field, or the word NULL sitting in a field-shaped box, both
    // read as something that could still be typed into.
    let value_view = dyn_container(
        move || kind.get(),
        move |k| {
            if !k.takes_text() {
                return empty().into_any();
            }
            edit_field(
                value,
                FieldCfg {
                    placeholder: "value",
                    // A `Raw` value is SQL and reads as code; the other two are
                    // data. Same rule the DDL preview's fields follow.
                    mono: matches!(k, params::ParamKind::Raw),
                    height: Some(field_input_h),
                    ..Default::default()
                },
            )
            // Narrow on purpose. A parameter's value is a word or a number far
            // more often than a sentence, and the bar's real constraint is how
            // many rows fit before one has to be scrolled to — the field is the
            // only part of a row whose width is a choice rather than its content.
            //
            // The gap to the chip is the field's own margin rather than the
            // row's `gap`, because a `gap` is spent on the zero-width `empty()`
            // above too: a `NULL` row would keep 7px of field-shaped space it no
            // longer has a field for.
            .style(|s| s.width(theme::scaled(98.0)).margin_left(theme::scaled(7.0)))
            .into_any()
        },
    );

    let chip = label(move || kind.get().label().to_string())
        .on_click_stop(move |_| kind.update(|k| *k = k.next()))
        .style(|s| {
            s.font_family(MONO_FAMILY.to_string())
                .font_size(theme::font_label())
                .color(theme::text_muted())
                .padding_horiz(theme::scaled(6.0))
                .height(theme::scaled(18.0))
                .items_center()
                .border(1.0)
                .border_color(theme::border())
                .border_radius(4.0)
                .margin_left(theme::scaled(7.0))
                .hover(|s| s.color(theme::text()).border_color(theme::text_faint()))
        })
        .tooltip(|| text("Text, number, NULL, or raw SQL — click to change").style(tooltip_style));

    // Name, then the kind, then the field: the chip qualifies the name — it says
    // what `:age` *is* — and the field is the answer to both. Behind the field it
    // read as a unit attached to the value, and it is also the control that makes
    // the field disappear, which is easier to connect to a click that happens
    // before it in the row than after.
    h_stack((name_label, chip, value_view))
        .style(|s| s.flex_row().items_center().flex_shrink(0.0_f32))
}

// ── Diagnostics (catalog-aware) ──────────────────────────────────────────────

/// Compute the editor's diagnostics for `sql`: catalog-aware unknown-table /
/// unknown-column errors, syntax errors on completed statements, and probable
/// keyword-typo warnings — all from the pure `schemaic_core::intel` engine, which
/// parses each statement with a real dialect AST and resolves references against
/// the introspected schema. `active_db` scopes unqualified references (a tab's
/// selected database); the whole `db_nodes` catalog is still available so an
/// explicit `otherdb.table` resolves. `pub(crate)` so the status bar reports a
/// live count from the same analysis that draws the squiggles.
pub(crate) fn compute_diagnostics(
    sql: &str,
    db_nodes: RwSignal<Vec<ConnNode>>,
    active_db: Option<&str>,
    dialect: SqlDialect,
) -> Vec<Diagnostic> {
    let catalog = crate::completion::build_catalog(db_nodes, active_db);
    // A `:name` is a syntax error to the parser, and one of those blanks the
    // diagnostics for the whole statement — so the analysis reads a neutralised
    // copy (`:id` → `_id`, byte offsets intact) and the reports that land on the
    // placeholders themselves are dropped again. Everything else in the
    // statement is still checked while the query is parameterised.
    let refs = params::scan(sql, dialect);
    if refs.is_empty() {
        return intel::diagnostics(sql, &catalog, dialect);
    }
    let analysable = params::neutralize(sql, dialect);
    let diagnostics = intel::diagnostics(&analysable, &catalog, dialect);
    params::strip_param_diagnostics(diagnostics, &refs)
}

/// What one editor pane should do about the inline-AI state it is watching.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InlinePaneAction {
    /// Draw the suggestion's phantom rows in this editor.
    pub draw: bool,
    /// Hold the buffer `read_only` — the plan is anchored to line numbers, so an
    /// edit underneath it would leave the rows describing lines that moved.
    pub freeze: bool,
    /// Take the keyboard, because the prompt field that had it is being torn
    /// down and the editor is what answers Enter/Escape for the verdict.
    pub focus: bool,
}

/// The three of them, from whether this pane **owns** the request and whether the
/// request has settled.
///
/// **`inline_ai` is one global signal; `CmdK` is per pane.** The panes are keyed
/// on the active tab, so switching tabs disposes one and builds another with a
/// fresh `CmdK` — `open: false`, `start == end == 0`. Nothing gated the publish
/// effect on ownership, so a suggestion left un-answered in tab A was published
/// into tab B: its SQL drawn as an insertion at line 0 of a document it had
/// nothing to do with, and tab B's editor set `read_only`, which really does stop
/// typing. There was no way out — the verdict footer and Escape are both gated on
/// `open`, which tab B's `CmdK` has as `false` — so every tab visited afterwards
/// came up frozen until another Ctrl+K happened to reset the state.
///
/// So `open` gates all three. It is the pane's own answer to "was this asked for
/// here", and the only one available: the request carries no pane id, and floem
/// offers no scope-cleanup hook to clear the signal on teardown.
pub(crate) fn inline_pane_action(open: bool, settled: bool) -> InlinePaneAction {
    InlinePaneAction {
        draw: open,
        freeze: open && settled,
        focus: open && settled,
    }
}

/// Which of the Ctrl+K bar's two verdict keys the **editor's own** key handler
/// answers, given the bar's state.
pub(crate) struct CmdkEditorKeys {
    /// Enter approves the suggestion sitting in the editor's lines.
    pub accept_on_enter: bool,
    /// Escape takes the bar down — abandoning a suggestion, or abandoning a
    /// request that has not landed yet.
    pub reject_on_escape: bool,
}

/// The editor answers the verdict keys for the states in which the prompt field
/// **is not the thing holding the keyboard**.
///
/// `Ready` is the obvious one: the field is torn down when the suggestion lands,
/// so Enter and Escape have nowhere else to go and the suggestion is sitting in
/// the editor's own lines anyway.
///
/// `Busy` is the one this used to miss. A request is not always started from the
/// field — "Optimize", and the error bar's *Fix with AI*, open the bar already
/// `Busy` from a menu the user clicked, and the editor keeps the keyboard. So
/// Escape reached this handler, matched no branch, and the only way out of a
/// running request was the mouse: it closed the bar while prompting (the field
/// answers Escape) and while previewing a diff (`Ready`, below), and did nothing
/// in between.
///
/// `Idle` and `Failed` are left alone deliberately. Both have a live, focused,
/// unfrozen field, whose `on_escape` already closes the bar — and a branch here
/// would take Escape away from the completion popup the user can open by typing
/// in the editor underneath.
pub(crate) fn cmdk_editor_keys(open: bool, state: &InlineAiState) -> CmdkEditorKeys {
    CmdkEditorKeys {
        accept_on_enter: open && matches!(state, InlineAiState::Ready(_)),
        reject_on_escape: open && matches!(state, InlineAiState::Ready(_) | InlineAiState::Busy),
    }
}

/// An inline SVG wavy line `width` px wide and [`WAVE_H`] tall — a smooth sine
/// squiggle (quadratic beziers) drawn under a misspelled keyword. A thin (1px)
/// stroke and gentle amplitude read as a wave rather than a thick band.
fn wavy_svg(width: f64) -> String {
    let width = width.max(2.0);
    let hp: f64 = 3.0; // half-period (px)
    let a: f64 = 1.5; // amplitude (px)
    let c = WAVE_H / 2.0; // centerline
    // First half-wave arcs up via a quadratic; each `T` then reflects the prior
    // control point, so the wave alternates up/down smoothly and continuously.
    let mut d = format!("M0 {c:.2}");
    let mut x = hp.min(width);
    d.push_str(&format!("Q{:.2} {:.2} {x:.2} {c:.2}", hp / 2.0, c - a));
    while x < width - 0.01 {
        let nx = (x + hp).min(width);
        d.push_str(&format!("T{nx:.2} {c:.2}"));
        x = nx;
    }
    // `currentColor` — Floem tints the svg view from its `.color(...)` style
    // (an explicit stroke here is ignored, which is why the color looked wrong).
    format!(
        "<svg width=\"{width:.2}\" height=\"{WAVE_H}\" viewBox=\"0 0 {width:.2} {WAVE_H}\" \
         xmlns=\"http://www.w3.org/2000/svg\">\
         <path d=\"{d}\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1\" \
         stroke-linecap=\"round\"/></svg>"
    )
}

/// The single `char` of `s`, or `None` if it isn't exactly one character.
fn single_char(s: &str) -> Option<char> {
    let mut it = s.chars();
    match (it.next(), it.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    }
}

// ── Overlay geometry ─────────────────────────────────────────────────────
//
// Every overlay pinned in `editor_area` (which does not scroll) has the same two
// problems to solve, and three of the four used to solve neither.
//
// 1. **`Editor::points_of_offset` returns absolute *document* y**, so the result
//    has to have the viewport origin subtracted or the overlay is drawn wherever
//    the text would be if the editor were scrolled to the top.
// 2. **It answers `(Point::ZERO, Point::ZERO)` for an offset it cannot place** —
//    `screen_lines` is built with no overscan, so anything outside the visible
//    range falls into that arm. Consumed as a position, that draws the overlay
//    at the editor's top-left: a 2px squiggle stub carrying the tooltip of an
//    error twenty lines away, and nothing at all under the actual error.
//
// So the arithmetic lives in `*_at` functions that take the point lookup as a
// closure and the viewport origin as a pair. That makes them pure and testable
// without an `Editor`, which is the whole reason the rules above were never
// pinned before. The `ed`-taking wrappers below are the adapters.
//
// The caret-anchored popups (`completion_popup`, `signature_popup`) obey rule 1 by
// a different route, because they are positioned once per *edit* rather than
// repainted every frame: `completion::set_anchor` stores the caret line in content
// coords and the popup's style closure subtracts `ed.viewport` reactively, so the
// popup keeps up with a scroll that happens while it is open. Baking the
// subtraction into the stored anchor would freeze it at the scroll position it
// opened at — which is the bug that walked the suggestion list down over the
// results grid. Rule 2 doesn't reach them: the caret is on screen by definition.

/// The x origin of the code column in `editor_area` coords: the gutter widens
/// with the line-number digit count, since it sizes to the last line number.
/// `points_of_offset().x` is text-layout-relative (0 = code start), so this is
/// what turns it into editor-area x.
fn content_x_of(sql: &str) -> f64 {
    let total_lines = sql.bytes().filter(|&c| c == b'\n').count() + 1;
    let digits = total_lines.to_string().len();
    HL_GUTTER + digits.saturating_sub(1) as f64 * HL_DIGIT_W
}

/// Top-left corner for the Ctrl+Enter run menu, in `editor_area` coords.
///
/// `anchor` is what the key handler stored: the caret's line-bottom with the gutter
/// added, in editor-**content** coords — so this is where rule 1 above is paid, and
/// reactively, so the menu keeps up with a scroll under it (the suggestion list's
/// route, for the same reason). `menu` is the panel's `(width, height)`.
///
/// It is then kept inside the **visible code column** — `content_x + vp.width()`,
/// the same fold [`statement_line_boxes_at`] clamps to — because `editor_area`
/// neither scrolls nor clips: a menu placed past that edge is drawn over the pane
/// beside it and cut off mid-panel, which is what a caret near the end of a long
/// line used to do. Horizontally it **flips** to the caret's left, so the menu stays
/// beside what it is about to run; vertically it **clamps**, since a flip needs the
/// caret line's top edge and covering a line is cheaper than a jump.
///
/// The clamp is not redundant with the flip: an anchor already past the fold (a
/// caret scrolled out to the right) flips to somewhere still past it.
///
/// A zero-sized viewport is "not measured yet", not "no room" — it keeps the plain
/// anchor rather than pinning the menu to the editor's top-left.
fn run_menu_pos(anchor: Point, menu: (f64, f64), content_x: f64, vp: Rect) -> Point {
    let (menu_w, menu_h) = menu;
    let (x, y) = (anchor.x - vp.x0, anchor.y - vp.y0);
    if vp.width() <= 0.0 || vp.height() <= 0.0 {
        return Point::new(x.max(0.0), y.max(0.0));
    }
    let fold = content_x + vp.width();
    let flipped = if x + menu_w > fold { x - menu_w } else { x };
    Point::new(
        flipped.min(fold - menu_w).max(0.0),
        y.min(EDITOR_PAD_TOP + vp.height() - menu_h).max(0.0),
    )
}

/// Did the editor actually place this offset?
///
/// floem's "not on screen" answer is *both* points at the origin. A genuinely
/// placed offset 0 is distinguishable without knowing the offset: its **bottom**
/// point carries the line height, so only an unplaced lookup yields the pair
/// `(ZERO, ZERO)`.
fn placed(top: Point, bot: Point) -> bool {
    !(top == Point::ZERO && bot == Point::ZERO)
}

/// Adapt an [`Editor`] to the point lookup the `*_at` functions take: `None`
/// when the offset isn't on screen.
///
/// **Tracks `screen_lines`, and that is not incidental.** `points_of_offset`
/// reads it *untracked*, so an overlay whose style closure tracked only
/// `viewport` re-ran with last frame's line positions: boxes drifted a few rows
/// onto unrelated text and — the tell — did **not** return to their old places
/// when the editor scrolled back, because the staleness was carried rather than
/// computed. Tracking it here fixes every caller at once, since this is the one
/// funnel they all go through.
fn editor_points(ed: &Editor) -> impl Fn(usize) -> Option<(Point, Point)> + '_ {
    ed.screen_lines.track();
    move |off| {
        let (top, bot) = ed.points_of_offset(off, CursorAffinity::Backward);
        placed(top, bot).then_some((top, bot))
    }
}

/// Vertical room the Ctrl+K prompt bar needs below the line it opens under: the
/// gap plus a bar of one or two rows. Generous rather than exact — over-scrolling
/// by a few pixels is invisible, and `scroll_beyond_last_line` means there is
/// always somewhere to go.
const CMDK_BAR_RESERVE: f64 = 52.0;

/// The share of the error bar its message may take, which is the same
/// `max_width_pct` the message is capped at — so what is left for the buttons is
/// `1 - ERROR_BAR_MSG_PCT`, and that is the budget below.
const ERROR_BAR_MSG_PCT: f64 = 0.60;

/// The error bar's horizontal inset, before the interface scale. It is padding on
/// the bar rather than a margin on each end's child, so both edges are the same
/// by construction — see the bar's own style for what went wrong when they were
/// two numbers.
const ERROR_BAR_PAD: f64 = 8.0;

/// Has an error bar `bar_w` wide the room for **Explain** beside *View* and
/// *AI fix*?
///
/// The rule is the message's own cap read from the other side. The message may
/// take 60% of the bar, so the buttons have the remaining 40% to fit in — and
/// when they don't, *Explain* is the one that goes, because the *View* modal
/// offers the same explanation and the other two have nowhere else to be.
///
/// Stating it as a share rather than a pixel floor is what makes it hold at every
/// interface scale, and every term measured rather than assumed for the same
/// reason: at 160% the three labels need half again the room, so a breakpoint
/// written at 100% would crowd them there — while a *share* is the same
/// proportion at any scale, since the bar scales with them.
///
/// The first attempt at this put a pixel minimum on the message and let the
/// buttons take whatever was left. It let all three through on a bar where the
/// message ended up ellipsized to a few words with the buttons packed against it,
/// which is the arrangement it was written to prevent — the floor was a guess,
/// and a guess about the wrong quantity: what makes the bar look crowded is the
/// buttons' *proportion* of it, not the message's absolute width.
fn error_bar_fits_explain(bar_w: f64) -> bool {
    let fs = theme::font_body();
    let sparkle = |label: &str| {
        theme::scaled_font(16.0) as f64
            + theme::scaled(5.0)
            + crate::widgets::measure_text_px_at(label, fs)
    };
    // Every gap and label to the right of the message, in bar order.
    let buttons = theme::scaled(10.0)
        + crate::widgets::measure_text_px_at("View", fs)
        + theme::scaled(20.0)
        + sparkle("Explain")
        + sparkle("AI fix");
    // `bar_w` is the border box; the share is of what the children actually get,
    // which is what `max_width_pct` resolves against too.
    let content = (bar_w - 2.0 * theme::scaled(ERROR_BAR_PAD)).max(0.0);
    buttons <= content * (1.0 - ERROR_BAR_MSG_PCT)
}

/// The verdict footer's height, before the interface scale. **Two places need it
/// to agree** — the style that draws the bar and [`inline_footer_y`], which decides
/// whether the bar still fits on screen below the block — so it is one number
/// rather than two literals that drift apart the first time the bar is resized.
const VERDICT_BAR_H: f64 = 30.0;

/// One AI fix: the byte range the model may rewrite, the problems it is being
/// asked to fix, and where those came from. The pane's `fix_with_ai`, shared by
/// the error bar, the error modal and the editor menu's "AI fix".
type FixFn = Rc<dyn Fn((usize, usize), Vec<String>, FixOrigin)>;

/// Anchor the Ctrl+K bar under `end`, scrolling the editor if the bar would not
/// fit below it.
///
/// The bar is an overlay, so the editor has no idea it is there and will happily
/// leave the line it hangs off flush against the bottom of the pane — which opened
/// the prompt clipped, or entirely out of sight, for any statement near the end of
/// a full screen. The stored point stays in the editor's **content** coordinates
/// (the style closure subtracts the viewport, so the bar tracks a later scroll);
/// only the fit test converts to screen coordinates.
fn anchor_cmdk(ed: &Editor, cmdk: CmdK, end: usize, area_h: RwSignal<f64>) {
    let (_, mut below) = ed.points_of_offset(end, CursorAffinity::Backward);
    below.y += EDITOR_PAD_TOP;
    cmdk.point.set(below);
    // Untracked, like the viewport read beside it: this runs from a key handler,
    // and a tracked read here would quietly give any future caller inside an
    // effect a dependency on the pane's height.
    if let Some(by) = cmdk_scroll_overflow(
        below.y,
        ed.viewport.get_untracked().y0,
        theme::scaled(CMDK_BAR_RESERVE),
        area_h.get_untracked(),
    ) {
        ed.scroll_delta.set(Vec2::new(0.0, by));
    }
}

/// How far the editor has to scroll for the Ctrl+K bar, anchored under document
/// y `anchor_y`, to be fully on screen — or `None` when it already is.
///
/// `reserve` is the bar's own height plus the gap it wants beneath it; `vp_y0` is
/// the viewport's top in document coordinates, and `area_h` the pane's height.
///
/// **A pure function because `c058b98` deleted the three tests that used to guard
/// this arithmetic.** They belonged to `cmdk_diff_chrome`, which went with the
/// overlay; the property moved here and into [`footer_fits`] and arrived with
/// none. A sign error is invisible on screen until the one buffer that is long
/// enough to scroll.
pub(crate) fn cmdk_scroll_overflow(
    anchor_y: f64,
    vp_y0: f64,
    reserve: f64,
    area_h: f64,
) -> Option<f64> {
    let overflow = (anchor_y - vp_y0) + reserve - area_h;
    (overflow > 0.0).then_some(overflow)
}

/// Does the verdict footer, `bar_h` tall and placed at `y` in the pane's own
/// coordinates, fit inside a pane `area_h` tall?
///
/// **Both edges.** The bar is positioned by line geometry against an editor that
/// scrolls *under* it, and it is absolutely positioned in the pane rather than
/// clipped to the editor — so without the `y >= 0.0` half it rides up over the
/// toolbar and the tab strip when the suggestion scrolls off the top, and without
/// the other half it hangs below the pane when the suggestion is near the bottom.
///
/// A pane shorter than the bar itself fits nothing, which is the `area_h = 0`
/// case a collapsed editor produces.
pub(crate) fn footer_fits(y: f64, bar_h: f64, area_h: f64) -> bool {
    y >= 0.0 && y + bar_h <= area_h
}

/// The y (in `editor_area` coords) of the first pixel *below* the phantom rows a
/// pending Ctrl+K suggestion has added — where the verdict footer goes.
///
/// The block's own bottom is not something `points_of_offset` will report: the
/// added rows are phantom text on the anchor line, and a caret offset at the end
/// of that line maps to a column *before* them, so its `bot` is the bottom of the
/// line's own row. The next document line's top is the honest answer, because the
/// phantom rows are exactly what pushed it down — and the last-line case, which
/// has no next line, is the one that has to count the rows itself.
///
/// Reactive: `screen_lines` is tracked, so the footer follows a scroll under it
/// rather than staying where the suggestion arrived.
fn inline_footer_y(
    ed: &Editor,
    preview: inline_diff::InlinePreview,
    area_h: RwSignal<f64>,
) -> Option<f64> {
    ed.screen_lines.track();
    let view = preview.get()?;
    let hunk = view.plan()?.hunks.last()?;
    let rope = ed.rope_text();
    let vp_y = ed.viewport.get().y0;
    let (top, bot) = if hunk.anchor + 1 < rope.num_lines() {
        let off = rope.offset_of_line(hunk.anchor + 1);
        let (top, _) = ed.points_of_offset(off, CursorAffinity::Backward);
        (top, top)
    } else {
        // Last line of the document: there is no next line to ask, so step past
        // the anchor's own rows. **`line_count()`, not `add.len()`** — it counts
        // the line's *visual* rows, phantom and soft-wrapped alike, and with word
        // wrap on a long added line occupies more rows than it has lines. Counting
        // lines put the bar one row short and left the tail of the suggestion
        // stranded below it.
        let off = rope.offset_of_line(hunk.anchor);
        let (mut t, _) = ed.points_of_offset(off, CursorAffinity::Backward);
        t.y += ed.text_layout(hunk.anchor).line_count() as f64
            * f64::from(ed.line_height(hunk.anchor));
        (t, t)
    };
    let y = top.y + EDITOR_PAD_TOP - vp_y;
    // Off screen when the block has scrolled out of the pane. The bar is placed by
    // line geometry against an editor that scrolls under it, so without this it
    // rides up over the toolbar and the tab strip — it is absolutely positioned in
    // the pane, not clipped to the editor.
    let fits = footer_fits(y, theme::scaled(VERDICT_BAR_H), area_h.get());
    (placed(top, bot) && fits).then_some(y)
}

/// The inline-diff bands as one `(y, height, is_add)` entry **per banded line** in
/// `editor_area` coords, top to bottom — each hunk's deleted lines, then the lines
/// it adds. Per line rather than per run so the gutter strip can carry a `−`/`+`
/// on each.
///
/// Every line the diff touches is banded by its **visual** rows, through the same
/// [`inline_diff::row_split`] the code column's own bands go through — so the two
/// halves of a band cover the same rows even where word wrap has given a line more
/// rows than it has lines.
///
/// This exists because a band cannot be painted edge-to-edge from where the rest
/// of it is painted. `sql_highlight`'s `LineExtraStyle` covers the code column
/// correctly (under the glyphs, scrolling with them), but the editor's content
/// lives inside a clipping `scroll` with the gutter as a *sibling painted before
/// it*, so nothing drawn from inside can reach the gutter or the wrapper's right
/// padding. Those two strips are text-free, so an overlay can finish the band
/// there without covering anything — which is the one job this has.
///
/// Which of one hunk's lines this frame has any reason to ask the layout about,
/// given the buffer lines `visible` (inclusive) currently on screen: the deleted
/// range narrowed to the viewport, plus the anchor of a pure insertion when that
/// is on screen.
///
/// **A filter, never a clamp.** A line outside the viewport is dropped, not
/// pulled to the nearest one — the same rule `top_of` states for a line past the
/// end of the document, and for the same reason: a band placed against whatever
/// text happens to be at the clamped line is worse than no band.
///
/// This is here because [`inline_band_runs`] re-runs on **every scroll tick**
/// (it tracks `screen_lines`), and it used to call `points_of_offset` for every
/// line of every hunk before finding out the line was off screen — and floem
/// answers that with a linear scan of `screen_lines`. So a whole-buffer
/// suggestion, which `fix_with_ai` produces routinely when
/// `intel::error_fix_range` cannot locate the error's token, cost
/// `O(deleted × visible)` per frame with none of it drawing anything. Bounded by
/// the viewport, it is `O(visible)`.
///
/// `None` for `visible` is an editor with nothing laid out yet, which places no
/// offset at all — so there is nothing to ask about either.
pub(crate) fn visible_hunk_lines(
    del: std::ops::Range<usize>,
    anchor: usize,
    has_add: bool,
    visible: Option<(usize, usize)>,
) -> (std::ops::Range<usize>, Option<usize>) {
    // A pure insertion has no deleted line to hang the block off, so the anchor
    // still has to be visited for its added rows.
    let anchor = (del.is_empty() && has_add).then_some(anchor);
    let Some((first, last)) = visible else {
        return (0..0, None);
    };
    let start = del.start.max(first);
    let end = del.end.min(last + 1);
    let clipped = match start < end {
        true => start..end,
        false => 0..0,
    };
    (clipped, anchor.filter(|a| *a >= first && *a <= last))
}

/// Reactive on `screen_lines`, so the strips follow a scroll like the rest.
fn inline_band_runs(ed: &Editor, preview: inline_diff::InlinePreview) -> Vec<(f64, f64, bool)> {
    ed.screen_lines.track();
    let Some(view) = preview.get() else {
        return Vec::new();
    };
    let Some(plan) = view.plan() else {
        return Vec::new();
    };
    let rope = ed.rope_text();
    let vp_y = ed.viewport.get().y0;
    // `None` for a line that is off screen **or past the end of the document**.
    // The plan is computed against the buffer as it was, so a hunk can outlive the
    // lines it names; clamping such a line to the last one (as this used to) put
    // its band against whatever text happens to be there now, which is worse than
    // not drawing it.
    let last_line = rope.num_lines().saturating_sub(1);
    let top_of = |line: usize| -> Option<f64> {
        if line > last_line {
            return None;
        }
        let off = rope.offset_of_line(line);
        let (top, bot) = ed.points_of_offset(off, CursorAffinity::Backward);
        placed(top, bot).then_some(top.y + EDITOR_PAD_TOP - vp_y)
    };
    let mut rows = Vec::new();
    // The lines on screen, so the walk below is bounded by the viewport rather
    // than by the size of the plan — see `visible_hunk_lines`.
    let visible = ed
        .screen_lines
        .get()
        .rvline_range()
        .map(|(f, l)| (f.line, l.line));
    // Every line the diff touches, banded by its **visual** rows. Each line is
    // asked for its own split through `inline_diff::row_split`, the same function
    // the code column's bands go through, so the two halves of a band cannot
    // disagree about which rows they cover — including where word wrap has given a
    // line more rows than it has lines.
    for hunk in &plan.hunks {
        let (del_lines, anchor) =
            visible_hunk_lines(hunk.del.clone(), hunk.anchor, !hunk.add.is_empty(), visible);
        for line in del_lines.chain(anchor) {
            let Some(top) = top_of(line) else { continue };
            let line_h = f64::from(ed.line_height(line));
            let own_len = rope.line_content(line).len();
            let (added, own) = inline_diff::row_split(
                &ed.text_layout(line),
                line_h,
                inline_diff::block_at(&view, line),
                own_len,
            );
            // Only a line the hunk actually removes is banded as a deletion. On a
            // **pure insertion** the visited line is the anchor — an untouched
            // context line the block merely hangs off — and banding its own rows
            // put a red strip and a `−` in the gutter beside a line nothing had
            // happened to, while the code column beside it stayed clean. The same
            // gate as `sql_highlight`'s `replaced`, for the same reason.
            if hunk.del.contains(&line) {
                for r in own {
                    rows.push((top + r as f64 * line_h, line_h, false));
                }
            }
            for r in added {
                rows.push((top + r as f64 * line_h, line_h, true));
            }
        }
    }
    rows
}

/// Pixel box `(x, y, w, h)` in `editor_area` coords around the single-line byte
/// span `[lo, hi]`, for the caret-driven highlight overlays (bracket matching,
/// identifier occurrences). `None` when either end is off screen.
///
/// The horizontal edges are **snapped to whole pixels** (floor left, ceil right)
/// so the 1px border lands crisply on the device grid — otherwise a glyph at a
/// fractional x antialiases ~1px off (looked like the box was biased right).
fn span_box_at(
    points: impl Fn(usize) -> Option<(Point, Point)>,
    sql: &str,
    lo: usize,
    hi: usize,
    vp: (f64, f64),
) -> Option<(f64, f64, f64, f64)> {
    let content_x = content_x_of(sql);
    let (top, bot) = points(lo)?;
    let (end, _) = points(hi)?;
    let left = (content_x + top.x - vp.0).floor();
    let right = (content_x + end.x - vp.0).ceil();
    let w = (right - left).max(4.0);
    let y = top.y + EDITOR_PAD_TOP - vp.1;
    Some((left, y, w, bot.y - top.y))
}

/// Pixel underline segment `(x, y, width)` in `editor_area` coords for the word
/// `[lo, hi]` (assumed single-line). `None` when either end is off screen — a
/// diagnostic outside the visible region must render *nothing*, not a stub.
fn underline_seg_at(
    points: impl Fn(usize) -> Option<(Point, Point)>,
    sql: &str,
    lo: usize,
    hi: usize,
    vp: (f64, f64),
) -> Option<(f64, f64, f64)> {
    let content_x = content_x_of(sql);
    let (top, bot) = points(lo)?;
    let (end, _) = points(hi)?;
    // `content_x` slightly over-estimates the code start (the padded statement-
    // highlight border masked it; a tight underline exposes it), so nudge left to
    // sit flush with the glyphs.
    const WAVE_X_ADJUST: f64 = 3.0;
    let x0 = content_x + top.x - WAVE_X_ADJUST - vp.0;
    let x1 = content_x + end.x - WAVE_X_ADJUST - vp.0;
    // Sit the wave ~2px below the glyphs (bot.y is the line's bottom; the
    // descenders end a few px above it, so drop the wave's top to just past them).
    // +`EDITOR_PAD_TOP` for the editor's top padding, −`vp.1` for the scroll.
    let y = bot.y - WAVE_H + 2.0 + EDITOR_PAD_TOP - vp.1;
    Some((x0, y, (x1 - x0).max(2.0)))
}

/// Pixel box in `editor_area` coords around the single-line span `[lo, hi]`.
/// `None` when off screen.
fn span_box(sql: &str, ed: &Editor, lo: usize, hi: usize) -> Option<(f64, f64, f64, f64)> {
    let vp = ed.viewport.get();
    span_box_at(editor_points(ed), sql, lo, hi, (vp.x0, vp.y0))
}

// (There is no `ed`-taking `underline_seg` wrapper: the squiggle overlay has to
// rebuild its view when the geometry moves — the wave's width is baked into the
// SVG markup — so it calls `underline_seg_at` from inside a memo instead. See
// `syntax_view`.)

/// Set the picked-statement highlight to `[lo, hi]` — but only when it's ONE OF
/// SEVERAL statements (a lone query needs no highlight, per the spec). "Several"
/// = some alphanumeric content exists outside the picked range.
fn highlight_pick(sql: &str, lo: usize, hi: usize, highlight: RwSignal<Option<(usize, usize)>>) {
    let others = sql[..lo].chars().any(|c| c.is_alphanumeric())
        || sql[hi..].chars().any(|c| c.is_alphanumeric());
    highlight.set(if others { Some((lo, hi)) } else { None });
}

/// Per-line pixel boxes (x, y, w, h in `editor_area` coords) covering the picked
/// statement's byte range `[lo, hi]`, for the DataGrip-style border. One box per
/// line the statement touches, sized to that line's slice of the statement, so
/// the right edges "staircase". `points_of_offset` gives the caret top/bottom at
/// an offset; `.x` is content-relative (add the gutter), `.y` is editor-relative.
///
/// Boxes are **clamped to the viewport's visible width**. These overlays are laid
/// out in `editor_area`, which doesn't scroll and doesn't clip, so a statement
/// wider than the visible code column drew its border straight out of the editor
/// and across whatever sits beside it. Vertical needs no such clamp: floem won't
/// place an offset outside its screen lines, and `editor_points` drops those.
///
/// `vp` is the editor's viewport rect — origin *and* size, since the clamp needs
/// the width. A zero-width one (before first layout) means "unknown", not "no
/// room": it clamps nothing, rather than blanking the highlight.
fn statement_line_boxes_at(
    points: impl Fn(usize) -> Option<(Point, Point)>,
    sql: &str,
    lo: usize,
    hi: usize,
    vp: Rect,
) -> Vec<(f64, f64, f64, f64)> {
    let content_x = content_x_of(sql);
    // The visible slice of the code column, in `editor_area` coords.
    let vis_hi = if vp.width() > 0.0 {
        content_x + vp.width()
    } else {
        f64::INFINITY
    };
    let mut boxes = Vec::new();
    let mut pos = lo;
    loop {
        let line_start = sql[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let nl = sql[pos..].find('\n').map(|i| pos + i);
        let line_end = nl.unwrap_or(sql.len());
        let seg_lo = lo.max(line_start);
        let seg_hi = hi.min(line_end);
        // A line the editor can't place contributes no box. Skipping it is what
        // makes the border stop at the fold instead of drawing a stray rectangle
        // over the gutter.
        if seg_hi >= seg_lo
            && let (Some((top, bot)), Some((end, _))) = (points(seg_lo), points(seg_hi))
        {
            // Inflate horizontally by HL_PAD so the border clears the glyphs (a
            // tight box clips them). Only horizontal — the vertical extent must
            // stay one line tall (+1) so adjacent lines' borders overlap into a
            // single 1px middle border.
            // Clamped left to the code column (never over the line-number gutter)
            // and right to the fold.
            let x0 = (content_x + top.x - HL_PAD - vp.x0).max(content_x);
            let x1 = content_x + end.x + HL_PAD - vp.x0;
            // The 6px floor keeps an empty statement visible, but never at the
            // cost of reaching past the fold.
            let w = (x1 - x0).max(6.0).min(vis_hi - x0);
            if w > 0.0 {
                boxes.push((x0, top.y + EDITOR_PAD_TOP - vp.y0, w, bot.y - top.y));
            }
        }
        match nl {
            Some(n) if n < hi => pos = n + 1,
            _ => break,
        }
    }
    boxes
}

/// Per-line boxes for the picked statement, in `editor_area` coords.
fn statement_line_boxes(sql: &str, ed: &Editor, lo: usize, hi: usize) -> Vec<(f64, f64, f64, f64)> {
    statement_line_boxes_at(editor_points(ed), sql, lo, hi, ed.viewport.get())
}

/// The full set of signals/callbacks `query_pane` threads into the editor and its
/// overlays. Bundled so the builder takes a single argument.
pub(crate) struct QueryPaneParams {
    pub query: RwSignal<String>,
    /// Caret byte offset, mirrored out of the editor for the status-bar Ln/Col.
    pub cursor_offset: RwSignal<usize>,
    /// The selected byte range, mirrored out alongside it — see
    /// [`Tab::selection`](crate::Tab::selection).
    pub selection: RwSignal<Option<(usize, usize)>>,
    /// Opens the Go-to-line popup (Ctrl+G, or a status-bar Ln/Col click).
    pub goto_open: RwSignal<bool>,
    /// When set, jump the caret to this byte offset (move + centre + focus), then
    /// clear it. Driven by the status-bar warning count.
    pub jump_offset: RwSignal<Option<usize>>,
    /// When set, reformat this tab and clear it — the palette's "Format Code".
    /// See [`Tab::format_req`](crate::Tab::format_req) for why it comes through
    /// the pane instead of being written straight into `query`.
    pub format_req: RwSignal<bool>,
    /// When set, insert this text at the caret and clear it — the snippet
    /// library. See [`Tab::insert_req`](crate::Tab::insert_req).
    pub insert_req: RwSignal<Option<String>>,
    /// Where this pane publishes its (debounced) offline diagnostics. Lives on
    /// the tab so the status bar reads the analysis rather than repeating it —
    /// see [`Tab::diagnostics`](crate::Tab::diagnostics).
    pub syntax: RwSignal<Vec<Diagnostic>>,
    /// When set, AI-fix *this* run error and clear it — the error modal's "AI
    /// fix", which sends the message it was showing rather than a bare "go".
    /// See [`Tab::fix_req`](crate::Tab::fix_req).
    pub fix_req: RwSignal<Option<String>>,
    pub results: RwSignal<QueryState>,
    /// The **guarded** run action ([`crate::TabsActions::run`]) — a held-back run
    /// lands in `run_guard` and nothing executes.
    pub run: Rc<dyn Fn(String)>,
    /// The guarded batch run ([`crate::TabsActions::run_all`]).
    pub run_all: Rc<dyn Fn(Vec<String>)>,
    /// What the write guard is holding, if anything — rendered as the guard bar.
    pub run_guard: RwSignal<Option<crate::RunGuard>>,
    /// The snippet library, for abbrev expansion in the completion popup, and
    /// the connection its scopes are judged against.
    pub snippets: Memo<Vec<schemaic_core::snippet::Snippet>>,
    pub active_conn: RwSignal<u64>,
    /// This tab's `:name` parameter values — the parameters bar's store. The bar
    /// collects into it; the run action substitutes with it.
    /// See [`Tab::params`](crate::Tab::params).
    pub params: RwSignal<Vec<params::Binding>>,
    /// The guard bar's "Run anyway" ([`crate::TabsActions::run_anyway`]).
    pub run_anyway: Rc<dyn Fn()>,
    pub db_nodes: RwSignal<Vec<ConnNode>>,
    /// Databases the SCHEMA panel's eye has hidden — the database selector must
    /// not offer one (`schema::db_visible`), so its trigger has to know
    /// whether anything is left to offer.
    pub hidden_dbs: floem::reactive::Memo<std::collections::HashSet<String>>,
    pub inline_ai: RwSignal<InlineAiState>,
    pub inline_ai_run: Rc<dyn Fn(InlineAiRequest)>,
    pub inline_ai_cancel: Rc<dyn Fn()>,
    pub error_modal_open: RwSignal<bool>,
    pub schema_visible: RwSignal<bool>,
    pub right_panel: RwSignal<RightPanel>,
    pub ai_send: Rc<dyn Fn(String)>,
    pub context_menu: RwSignal<Option<CtxMenu>>,
    pub editor_h: RwSignal<f64>,
    /// Collapsed → the pane renders at height 0 (instant) so the RESULTS grid takes
    /// the whole region; `editor_h` stays the restore height. See `LayoutUi`.
    pub editor_collapsed: RwSignal<bool>,
    pub active_db: Memo<Option<String>>,
    /// The SQL dialect of the active tab's connection (MySQL/PostgreSQL), driving
    /// completion + diagnostics parsing. Reactive — follows the tab's `conn_id`.
    pub dialect: Memo<SqlDialect>,
    /// How much of the active tab's connection the assistant may see. The **tab's**
    /// connection, not the active one, for `grid::ai_data_of`'s reason. Gates the
    /// engine's error text out of the AI fix and Explain prompts on any level
    /// below `Full` — see [`prompt::result_shape`](schemaic_core::prompt::result_shape).
    pub ai_data: Memo<schemaic_core::connection::AiData>,
    pub active_db_menu_open: RwSignal<bool>,
    pub active_db_anchor: RwSignal<Point>,
    /// Every menu-open flag in the app, so the database selector can close the
    /// others when it opens. It has to do that itself: it absorbs its own
    /// pointer-down, so the workspace root's `close_except(None)` never runs for
    /// it — the same bargain the schema eye, the gear and the activity clock
    /// take.
    pub menus: crate::widgets::MenuFlags,
    /// The active tab's connection is marked read-only. The *write* guard reads
    /// this on the run action; the pane only uses it to hide "Create view".
    pub read_only: Memo<bool>,
    /// Whether to validate the statement under the cursor against the live DB as
    /// you type (Tier-2 diagnostics). Persisted setting.
    pub live_validate: RwSignal<bool>,
    /// Requests a debounced DB validation of the statement under the cursor.
    pub validate_stmt: ValidateFn,
    pub popup_menu: RwSignal<Option<Vec<MenuEntry>>>,
    pub popup_anchor: RwSignal<Option<PopupAnchor>>,
    pub popup_width: RwSignal<f64>,
    pub open_plan: Rc<dyn Fn(String)>,
    /// Opens the view editor on the statement under the right-click — the
    /// editor's half of "Create view". Takes the statement text; the database
    /// and namespace are the caller's to resolve.
    pub create_view: Rc<dyn Fn(String)>,
    pub nav: NavKeys,
    /// This tab's temporary font-size override (px) for Ctrl+scroll zoom; `None`
    /// follows the user's configured size. Driven here, read by `SqlStyling`.
    pub zoom: RwSignal<Option<f32>>,
    /// Live reachability, for dimming the Run button while the connection is
    /// known-dead. The action itself is gated by the app.
    pub conn_status: RwSignal<ConnStatus>,
}

pub(crate) fn query_pane(p: QueryPaneParams) -> impl IntoView {
    let QueryPaneParams {
        query,
        cursor_offset,
        selection,
        goto_open,
        jump_offset,
        format_req,
        insert_req,
        syntax,
        fix_req,
        results,
        run,
        run_all,
        run_guard: guard,
        snippets,
        active_conn,
        params: tab_params,
        run_anyway,
        db_nodes,
        hidden_dbs,
        inline_ai,
        inline_ai_run,
        inline_ai_cancel,
        error_modal_open,
        schema_visible,
        right_panel,
        ai_send,
        context_menu,
        editor_h,
        editor_collapsed,
        active_db,
        dialect,
        ai_data,
        active_db_menu_open,
        active_db_anchor,
        menus,
        read_only,
        live_validate,
        validate_stmt,
        popup_menu,
        popup_anchor,
        popup_width,
        open_plan,
        create_view,
        nav,
        zoom,
        conn_status,
    } = p;
    let comp = Completion {
        items: RwSignal::new(Vec::new()),
        width: RwSignal::new(0.0),
        sel: RwSignal::new(0),
        open: RwSignal::new(false),
        point: RwSignal::new(Point::ZERO),
        line_top: RwSignal::new(0.0),
        suppress: RwSignal::new(false),
        typed: RwSignal::new(false),
        sig: RwSignal::new(None),
        sig_point: RwSignal::new(Point::ZERO),
    };
    let cmdk = CmdK {
        open: RwSignal::new(false),
        point: RwSignal::new(Point::ZERO),
        input: RwSignal::new(String::new()),
        intent: RwSignal::new(None),
        start: RwSignal::new(0),
        end: RwSignal::new(0),
        preview: RwSignal::new(None),
        verdict: RwSignal::new(None),
    };
    // Editor-area height, tracked so the completion popup knows whether its list
    // fits below the caret, and so the Ctrl+K bars can tell when they would fall
    // off the bottom of the pane. Declared up here because the editor's own key
    // handler — built below — needs it to scroll the prompt into view.
    let area_h: RwSignal<f64> = RwSignal::new(EDITOR_H);
    // Right-click editor menu (Ask AI / Explain / Optimize). It's routed through
    // the app-wide `popup_menu` overlay (rendered at the workspace root) so it
    // floats *over* the results pane instead of being clipped by the editor area,
    // and only edge-flips against the window. `menu_offset` is the caret offset
    // the editor moved to on the right-click (so actions scope to the statement
    // there).
    let menu_offset: RwSignal<usize> = RwSignal::new(0usize);

    // Ctrl+Enter run menu (Run Current / Run Everything), shown when the editor
    // holds more than one statement. `run_menu` holds the anchor point (editor-
    // area coords) when open; `run_menu_offset` is the caret offset at trigger
    // time, so Run Current can re-derive the statement under the caret.
    let run_menu: RwSignal<Option<Point>> = RwSignal::new(None);
    let run_menu_offset: RwSignal<usize> = RwSignal::new(0usize);
    // Which run-menu row is keyboard-selected (0 = Run Current, the default).
    let run_sel: RwSignal<usize> = RwSignal::new(0usize);

    // In-editor find (Ctrl+F) / replace (Ctrl+H): a small bar over the editor.
    // `find_hits` holds the byte offset of each match (recomputed as the query
    // changes); `find_idx` is the current match. Selecting a match sets the editor
    // selection (so it's highlighted) and centres it. `find_replace_visible`
    // expands the second row (the replacement field + Replace / All buttons).
    let find_open: RwSignal<bool> = RwSignal::new(false);
    let find_query: RwSignal<String> = RwSignal::new(String::new());
    let find_replace: RwSignal<String> = RwSignal::new(String::new());
    let find_replace_visible: RwSignal<bool> = RwSignal::new(false);
    let find_hits: RwSignal<Vec<usize>> = RwSignal::new(Vec::new());
    let find_idx: RwSignal<usize> = RwSignal::new(0usize);

    // Go-to-line (Ctrl+G, or clicking Ln/Col in the status bar): a small popup
    // styled like the find bar. `goto_open` lives on the `Tab` so the status bar
    // can open it too; `goto_query` backs its one input. Mutually exclusive with
    // the find bar (opening one closes the other) since both float at the top-right.
    let goto_query: RwSignal<String> = RwSignal::new(String::new());
    // Opening the popup from anywhere (incl. the status-bar click, which doesn't go
    // through the Ctrl+G handler) closes the find bar so the two never overlap.
    create_effect(move |_| {
        if goto_open.get() && find_open.get_untracked() {
            find_open.set(false);
        }
    });

    // The DataGrip-style border around the statement picked by Explain / Optimize
    // / Run Current: the byte range of that statement, or None. Cleared on any
    // edit or click in the editor (see below). Defined here (above the editor) so
    // the Ctrl+Enter key handler can set it.
    let highlight: RwSignal<Option<(usize, usize)>> = RwSignal::new(None);

    // Bracket matching: the byte offsets of the paren adjacent to the caret and
    // its partner (or None). Recomputed from caret + text below; drawn as two
    // faint boxes by `bracket_match_view`.
    let bracket_match: RwSignal<Option<(usize, usize)>> = RwSignal::new(None);

    // Highlight-all-occurrences: byte ranges of every occurrence of the identifier
    // under the caret (empty when the caret isn't on a repeated identifier).
    // Recomputed from caret + text below; drawn by `occurrences_view`.
    let ident_occurrences: RwSignal<Vec<(usize, usize)>> = RwSignal::new(Vec::new());

    // While the run menu is open, tie the statement highlight to the selection:
    // "Run Current" highlights the statement under the caret; "Run Everything"
    // (which acts on all of them) drops the single-statement highlight. Moving
    // back re-applies it. (When the menu is closed this is a no-op — the run
    // actions / edits own the highlight then.)
    create_effect(move |_| {
        if run_menu.get().is_some() {
            if run_sel.get() == 0 {
                let sql = query.get_untracked();
                let (lo, hi) = statement_range(
                    &sql,
                    run_menu_offset.get_untracked(),
                    dialect.get_untracked(),
                );
                highlight_pick(&sql, lo, hi, highlight);
            } else {
                highlight.set(None);
            }
        }
    });

    // Catalog-aware diagnostics (the squiggles) — recomputed on
    // every edit, seeded from the initial text.
    // Tier-2 (DB-validated) diagnostics for the statement under the cursor, when
    // `live_validate` is on. Debounced: each edit bumps `val_gen`, and only the
    // latest generation's deferred round-trip fires (and its result is accepted).
    let db_diag: RwSignal<Vec<Diagnostic>> = RwSignal::new(Vec::new());
    let val_gen: Rc<std::cell::Cell<u64>> = Rc::new(std::cell::Cell::new(0));
    // Offline diagnostics are likewise debounced: a burst of keystrokes bumps
    // `diag_gen` and only the latest generation's deferred pass re-parses, so we
    // don't re-parse the whole document on every character.
    let diag_gen: Rc<std::cell::Cell<u64>> = Rc::new(std::cell::Cell::new(0));
    // Seed the tab's diagnostics from the text this pane opens on, and re-run when
    // the *catalog* moves under them — unknown-table/unknown-column errors depend
    // on it, so a set computed before introspection landed is stale. Tracks
    // `db_nodes`/`active_db` only (never `query`, which is the debounced path
    // below), and rides the same generation guard so a burst of per-database
    // schema arrivals coalesces into one parse.
    {
        let dgen = diag_gen.clone();
        create_effect(move |_| {
            db_nodes.track();
            let adb = active_db.get();
            let g = dgen.get().wrapping_add(1);
            dgen.set(g);
            let dgen = dgen.clone();
            let dia = dialect.get_untracked();
            floem::action::exec_after(std::time::Duration::from_millis(120), move |_| {
                if dgen.get() != g || syntax.try_get_untracked().is_none() {
                    return;
                }
                let Some(q) = query.try_get_untracked() else {
                    return;
                };
                syntax.set(compute_diagnostics(&q, db_nodes, adb.as_deref(), dia));
            });
        });
    }
    // Turning validation off clears any lingering DB squiggle.
    create_effect(move |_| {
        if !live_validate.get() {
            db_diag.set(Vec::new());
        }
    });

    // The editor key handler needs its own clone (it's a `move` closure).
    // `run` is already the guarded run action — see `TabsActions::run`.
    let guarded_run_key = run.clone();

    // Only one menu open at a time: opening any one closes the others. (Their
    // dismiss catchers cover different regions, so a click in one doesn't reach
    // the others.)
    create_effect(move |_| {
        if popup_menu.get().is_some() {
            context_menu.set(None);
            run_menu.set(None);
        }
    });
    create_effect(move |_| {
        if context_menu.get().is_some() {
            popup_menu.set(None);
            run_menu.set(None);
        }
    });
    create_effect(move |_| {
        if run_menu.get().is_some() {
            popup_menu.set(None);
            context_menu.set(None);
        }
    });

    // The editor is the source of truth; every edit syncs back to `query`. The
    // custom key handler drives autocomplete (nav/accept/dismiss) and the
    // existing Ctrl+Enter / Shift+Tab shortcuts; everything else falls through.
    let editor = text_editor_keys(query.get_untracked(), move |editor_sig, kp, mods| {
        // **Before any branch below can return.** Every key is classified as
        // typing or not, because the recompute that a resulting edit schedules
        // runs a tick later with nothing but the document to go on — and a
        // document cannot tell a typed `x` from Ctrl+X. Only typing (or
        // Ctrl+Space) may open a closed popup; see `completion::popup_may_open`.
        comp.typed.set(match &kp.key {
            KeyInput::Keyboard(key, _) => types_a_character(key, mods.control(), mods.alt()),
            _ => false,
        });
        // Any keypress dismisses the unsafe-run notice (and doesn't execute). The
        // Ctrl+Enter branch below may re-raise it if the run is still unsafe.
        if guard.get_untracked().is_some() {
            guard.set(None);
        }
        // Global navigation (Ctrl+P/T/W/Tab/1-9). Handled here because the editor
        // `on_event_stop`s KeyDown, so the workspace-root handler never sees it
        // while the editor is focused. Checked before completion so Ctrl+Tab cycles
        // tabs rather than being eaten as a completion-accept Tab.
        if mods.control() {
            let is_tab = matches!(kp.key, KeyInput::Keyboard(Key::Named(NamedKey::Tab), _));
            let ch = match &kp.key {
                KeyInput::Keyboard(Key::Character(c), _) => Some(c.as_str().to_ascii_lowercase()),
                _ => None,
            };
            if nav.handle(mods.shift(), ch.as_deref(), is_tab) {
                return CommandExecuted::Yes;
            }
        }
        // Enter accepts / Escape rejects the Ctrl+K bar from the editor. Which
        // states that covers is `cmdk_editor_keys`', where it is written down and
        // tested; both closures are the footer buttons' own, reached through
        // `cmdk.verdict`, so there is one accept and one reject in the pane.
        // Checked before the find/goto/completion branches below so a stale popup
        // can't eat the verdict.
        {
            let keys = cmdk_editor_keys(cmdk.open.get_untracked(), &inline_ai.get_untracked());
            if let Some((accept, reject)) = cmdk.verdict.get_untracked() {
                if keys.accept_on_enter
                    && matches!(kp.key, KeyInput::Keyboard(Key::Named(NamedKey::Enter), _))
                {
                    accept();
                    return CommandExecuted::Yes;
                }
                if keys.reject_on_escape
                    && matches!(kp.key, KeyInput::Keyboard(Key::Named(NamedKey::Escape), _))
                {
                    reject();
                    return CommandExecuted::Yes;
                }
            }
        }
        // Esc closes the find bar even when focus is back in the editor (its input's
        // own on_escape covers the focused-input case) — so Esc closes it "anywhere".
        if find_open.get_untracked()
            && matches!(kp.key, KeyInput::Keyboard(Key::Named(NamedKey::Escape), _))
        {
            find_open.set(false);
            find_query.set(String::new());
            find_replace.set(String::new());
            return CommandExecuted::Yes;
        }
        // Same for the Go-to-line popup — Esc closes it from anywhere in the editor.
        if goto_open.get_untracked()
            && matches!(kp.key, KeyInput::Keyboard(Key::Named(NamedKey::Escape), _))
        {
            goto_open.set(false);
            goto_query.set(String::new());
            return CommandExecuted::Yes;
        }
        // Escape dismisses the suggestion list and/or signature help, whichever is
        // showing (consuming the key only when it actually dismissed something).
        if matches!(kp.key, KeyInput::Keyboard(Key::Named(NamedKey::Escape), _)) {
            let had_sig = comp.sig.get_untracked().is_some();
            let had_list = comp.open.get_untracked();
            comp.sig.set(None);
            comp.open.set(false);
            if had_sig || had_list {
                return CommandExecuted::Yes;
            }
        }
        if comp.open.get_untracked() {
            let len = comp.items.with_untracked(|v| v.len());
            if len > 0 {
                if matches!(
                    kp.key,
                    KeyInput::Keyboard(Key::Named(NamedKey::ArrowDown), _)
                ) {
                    comp.sel.update(|i| *i = (*i + 1) % len);
                    return CommandExecuted::Yes;
                }
                if matches!(kp.key, KeyInput::Keyboard(Key::Named(NamedKey::ArrowUp), _)) {
                    comp.sel.update(|i| *i = (*i + len - 1) % len);
                    return CommandExecuted::Yes;
                }
                let accept_enter = !mods.control()
                    && matches!(kp.key, KeyInput::Keyboard(Key::Named(NamedKey::Enter), _));
                let accept_tab = !mods.shift()
                    && matches!(kp.key, KeyInput::Keyboard(Key::Named(NamedKey::Tab), _));
                if accept_enter || accept_tab {
                    editor_sig.with_untracked(|e| accept_completion(e, comp));
                    return CommandExecuted::Yes;
                }
            }
            // A caret-moving key (not list-nav/accept above) leaves the popup
            // anchored to a stale position → close it, but let the caret still
            // move (fall through to the default handler). Typing recomputes it.
            if matches!(
                kp.key,
                KeyInput::Keyboard(
                    Key::Named(
                        NamedKey::ArrowLeft
                            | NamedKey::ArrowRight
                            | NamedKey::Home
                            | NamedKey::End
                            | NamedKey::PageUp
                            | NamedKey::PageDown
                    ),
                    _,
                )
            ) {
                comp.open.set(false);
            }
        }
        // Soft-tab indent: floem's built-in InsertTab uses the buffer's own fixed
        // indent width (4) and ignores our configured tab width, so when soft tabs
        // are on we compute and apply the spaces ourselves (via the tested pure
        // `soft_tab_indent`). Hard tabs fall through to the default (a literal
        // `\t`, whose display width already follows SqlStyling::tab_width).
        if !mods.control()
            && !mods.shift()
            && !comp.open.get_untracked()
            && matches!(kp.key, KeyInput::Keyboard(Key::Named(NamedKey::Tab), _))
            && theme::editor_soft_tabs()
        {
            let tw = theme::editor_tab_width();
            editor_sig.with_untracked(|e| {
                let full = e.doc().text().to_string();
                let (a, b) = e.cursor.get_untracked().get_selection().unwrap_or_else(|| {
                    let o = e.cursor.get_untracked().offset();
                    (o, o)
                });
                let ed = soft_tab_indent(&full, a, b, tw);
                edit_untyped(
                    e,
                    comp,
                    Selection::region(ed.start, ed.end),
                    &ed.text,
                    EditType::InsertChars,
                );
                e.cursor
                    .update(|c| c.set_insert(Selection::region(ed.sel.0, ed.sel.1)));
            });
            return CommandExecuted::Yes;
        }
        // Soft-tab outdent (Shift+Tab): the inverse of the above. Same reason —
        // floem's built-in outdent uses the buffer's fixed indent width — so we
        // remove one level (a leading tab, or up to `tw` spaces) per line ourselves.
        if !mods.control()
            && mods.shift()
            && !comp.open.get_untracked()
            && matches!(kp.key, KeyInput::Keyboard(Key::Named(NamedKey::Tab), _))
            && theme::editor_soft_tabs()
        {
            let tw = theme::editor_tab_width();
            editor_sig.with_untracked(|e| {
                let full = e.doc().text().to_string();
                let (a, b) = e.cursor.get_untracked().get_selection().unwrap_or_else(|| {
                    let o = e.cursor.get_untracked().offset();
                    (o, o)
                });
                let ed = soft_tab_outdent(&full, a, b, tw);
                // Skip the edit when nothing changes (no undo churn), but still
                // consume the key so it never falls through to floem's fixed-width
                // outdent.
                if ed.text != full[ed.start..ed.end] {
                    edit_untyped(
                        e,
                        comp,
                        Selection::region(ed.start, ed.end),
                        &ed.text,
                        EditType::InsertChars,
                    );
                    e.cursor
                        .update(|c| c.set_insert(Selection::region(ed.sel.0, ed.sel.1)));
                }
            });
            return CommandExecuted::Yes;
        }
        // Ctrl+Space: force the completion popup open in the current context,
        // even with no prefix typed. Read the caret directly (no edit is in
        // flight, so it isn't lagging — no need to defer like the `.update` path).
        if mods.control() {
            let space = matches!(kp.key, KeyInput::Keyboard(Key::Named(NamedKey::Space), _))
                || matches!(&kp.key, KeyInput::Keyboard(Key::Character(c), _) if c.as_str() == " ");
            if space {
                let adb = active_db.get_untracked();
                editor_sig.with_untracked(|e| {
                    recompute_completions(
                        e,
                        CompletionCtx {
                            db_nodes,
                            hidden_dbs,
                            active_db: adb.as_deref(),
                            dialect: dialect.get_untracked(),
                            snippets,
                            conn_id: active_conn.get_untracked(),
                        },
                        comp,
                        true,
                    )
                });
                return CommandExecuted::Yes;
            }
        }
        // Ctrl+K: open the inline AI prompt. Capture the caret offset (and the
        // selection range, if any) so Accept knows where to insert / what to
        // replace, plus the caret point for anchoring the popup.
        if mods.control()
            && let KeyInput::Keyboard(Key::Character(c), _) = &kp.key
            && c.as_str().eq_ignore_ascii_case("k")
        {
            editor_sig.with_untracked(|e| {
                let cur = e.cursor.get_untracked();
                let offset = cur.offset();
                let (a, b) = cur.get_selection().unwrap_or((offset, offset));
                // Normalize: a backward selection (dragged or shift-selected
                // right-to-left) reports start > end, which makes the later
                // `sql.get(start..end)` slice return None → the snippet is
                // dropped and Ctrl+K silently ignores the selection.
                let (start, end) = (a.min(b), a.max(b));
                // With nothing selected, take the statement the caret is in —
                // the same "what does this key act on" answer Ctrl+Enter gives,
                // through the same `sql::statement_range`. The old behaviour was
                // a bare caret, so an unselected Ctrl+K asked the model to edit
                // an insertion point and the diff had nothing to replace.
                //
                // Read the text from the **document**, not the `query` signal:
                // the offsets being looked up are the document's own, so anything
                // else risks resolving them against text that isn't what they
                // index. `accept` re-reads the doc for the same reason.
                let (start, end) = if start == end {
                    let sql = e.doc().text().to_string();
                    statement_range(&sql, start, dialect.get_untracked())
                } else {
                    (start, end)
                };
                cmdk.start.set(start);
                cmdk.end.set(end);
                // Show what the key picked, by actually selecting it. The design
                // puts the editor's selection colour behind the statement in the
                // asking state, and the honest way to get that is the editor's own
                // selection rather than a lookalike overlay — the user can then
                // see, extend or replace the range with the gestures they already
                // have, instead of being told about it.
                e.cursor
                    .update(|cc| cc.set_insert(Selection::region(start, end)));
                // Anchor to the BOTTOM of the caret's line (absolute
                // screen coords via `points_of_offset().1`), not the
                // per-line baseline `line_point_of_offset` returns —
                // that baseline is ~0 on an empty line but ~font-ascent
                // once the line has glyphs, which shoved the box down
                // whenever the editor had code.
                // Anchored to the END of what Ctrl+K is acting on, not to the
                // caret: the bar sits under the whole highlighted statement, so
                // a caret in the middle of one doesn't split it in two.
                anchor_cmdk(e, cmdk, end, area_h);
            });
            cmdk.input.set(String::new());
            cmdk.intent.set(None);
            inline_ai.set(InlineAiState::Idle);
            comp.open.set(false);
            cmdk.open.set(true);
            return CommandExecuted::Yes;
        }
        // Ctrl+Alt+L — reformat SQL (DataGrip's shortcut). Match the *physical* L
        // key, not the produced character: on Windows Ctrl+Alt is delivered as
        // AltGr, so the logical `Key::Character` may not be "l".
        if mods.control() && mods.alt() {
            use floem::keyboard::{KeyCode, PhysicalKey};
            if matches!(
                kp.key,
                KeyInput::Keyboard(_, PhysicalKey::Code(KeyCode::KeyL))
            ) {
                editor_sig.with_untracked(|e| format_editor(e, comp, dialect.get_untracked()));
                return CommandExecuted::Yes;
            }
        }
        // Editor line operations (DataGrip-ish): Ctrl+/ toggle line comment,
        // Ctrl+D duplicate line/selection, Ctrl+X delete line. `--` is the SQL
        // comment token; Floem's built-in ToggleLineComment hardcodes an empty
        // token, so the toggle is computed in `core::text_ops` and applied as one
        // full-buffer edit (a single undo step). Ctrl+X diverges from DataGrip
        // (which cuts) — here it deletes the line.
        if mods.control()
            && !mods.shift()
            && let KeyInput::Keyboard(Key::Character(c), _) = &kp.key
        {
            let c = c.as_str();
            if c == "/" {
                editor_sig.with_untracked(|e| {
                    let doc = e.doc();
                    let full = doc.text().to_string();
                    let cur = e.cursor.get_untracked();
                    let off = cur.offset();
                    let (a, b) = cur.get_selection().unwrap_or((off, off));
                    let edit =
                        toggle_line_comment(&full, a.min(b), a.max(b), dialect.get_untracked());
                    edit_untyped(
                        e,
                        comp,
                        Selection::region(0, full.len()),
                        &edit.text,
                        EditType::ToggleComment,
                    );
                    e.cursor
                        .update(|cc| cc.set_insert(Selection::region(edit.sel.0, edit.sel.1)));
                });
                return CommandExecuted::Yes;
            }
            if c.eq_ignore_ascii_case("d") {
                editor_sig.with_untracked(|e| {
                    let has_sel = e
                        .cursor
                        .get_untracked()
                        .get_selection()
                        .is_some_and(|(a, b)| a != b);
                    if has_sel {
                        // Selection: duplicate the spanned line(s) (Floem default).
                        e.doc().run_command(
                            e,
                            &Command::Edit(EditCommand::DuplicateLineDown),
                            Some(1),
                            mods,
                        );
                    } else {
                        // Bare caret: copy the whole current line onto a fresh line
                        // below. Done manually because Floem's DuplicateLineDown
                        // slices `line_start..next_line_start`, so on a line with no
                        // trailing newline (the last line) the copy has no `\n` and
                        // lands on the *same* line. Prepending `\n` guarantees a new
                        // line below regardless.
                        let doc = e.doc();
                        let full = doc.text().to_string();
                        let off = e.cursor.get_untracked().offset();
                        let ls = full[..off].rfind('\n').map(|i| i + 1).unwrap_or(0);
                        let le = full[off..]
                            .find('\n')
                            .map(|i| off + i)
                            .unwrap_or(full.len());
                        let insert = format!("\n{}", &full[ls..le]);
                        edit_untyped(
                            e,
                            comp,
                            Selection::region(le, le),
                            &insert,
                            EditType::InsertChars,
                        );
                        // Keep the caret at the same column on the duplicated line.
                        let new_caret = le + 1 + (off - ls);
                        e.cursor.update(|c| c.set_offset(new_caret, false, false));
                    }
                });
                return CommandExecuted::Yes;
            }
            if c.eq_ignore_ascii_case("x") {
                // Selection-aware: with a selection, fall through to the default
                // handler's Ctrl+X = cut; on a bare caret, delete the line.
                let has_sel = editor_sig.with_untracked(|e| {
                    e.cursor
                        .get_untracked()
                        .get_selection()
                        .is_some_and(|(a, b)| a != b)
                });
                if !has_sel {
                    editor_sig.with_untracked(|e| {
                        e.doc().run_command(
                            e,
                            &Command::Edit(EditCommand::DeleteLine),
                            Some(1),
                            mods,
                        );
                    });
                    return CommandExecuted::Yes;
                }
                // else: fall through → default handler cuts the selection.
            }
            if c.eq_ignore_ascii_case("f") {
                // Open the find bar in find-only mode (collapse any replace row left
                // over from a previous Ctrl+H). Its input autofocuses on mount.
                goto_open.set(false);
                find_replace_visible.set(false);
                find_open.set(true);
                return CommandExecuted::Yes;
            }
            if c.eq_ignore_ascii_case("g") {
                // Open the Go-to-line popup (autofocuses on mount). An effect closes
                // the find bar so the two never overlap at the top-right.
                goto_open.set(true);
                return CommandExecuted::Yes;
            }
            if c.eq_ignore_ascii_case("h") {
                // Open the find bar with the replace row expanded.
                find_replace_visible.set(true);
                if !find_open.get_untracked() {
                    find_open.set(true);
                }
                return CommandExecuted::Yes;
            }
        }
        // Panel toggles (also handled at the workspace root for non-editor
        // focus). Ctrl+Shift+E = Schema, Ctrl+Shift+A = AI, Ctrl+` = Terminal.
        // AI/Terminal share the right slot, so each key shows-or-hides its panel.
        if mods.control()
            && let KeyInput::Keyboard(Key::Character(c), _) = &kp.key
        {
            let c = c.as_str();
            if mods.shift() && c.eq_ignore_ascii_case("e") {
                if crate::schema_panel_allowed() {
                    schema_visible.update(|v| *v = !*v);
                }
                return CommandExecuted::Yes;
            }
            if mods.shift() && c.eq_ignore_ascii_case("a") {
                if crate::right_panel_allowed() {
                    right_panel.update(|p| {
                        *p = if matches!(*p, RightPanel::Ai) {
                            RightPanel::None
                        } else {
                            RightPanel::Ai
                        };
                    });
                }
                return CommandExecuted::Yes;
            }
            if c == "`" {
                if crate::right_panel_allowed() {
                    right_panel.update(|p| {
                        *p = if matches!(*p, RightPanel::Terminal) {
                            RightPanel::None
                        } else {
                            RightPanel::Terminal
                        };
                    });
                }
                return CommandExecuted::Yes;
            }
        }
        if mods.control() && matches!(kp.key, KeyInput::Keyboard(Key::Named(NamedKey::Enter), _)) {
            let sql = query.get_untracked();
            editor_sig.with_untracked(|e| {
                let offset = e.cursor.get_untracked().offset();
                let (lo, hi) = statement_range(&sql, offset, dialect.get_untracked());
                // Multiple statements → highlight the one under the caret and open
                // the Run Current / Run Everything menu at the caret. A lone
                // statement just runs (no menu, no highlight).
                let multi = sql[..lo].chars().any(|c| c.is_alphanumeric())
                    || sql[hi..].chars().any(|c| c.is_alphanumeric());
                if multi {
                    highlight_pick(&sql, lo, hi, highlight);
                    run_menu_offset.set(offset);
                    run_sel.set(0);
                    let (_, below) = e.points_of_offset(offset, CursorAffinity::Backward);
                    // Same gutter as the statement-highlight boxes so the menu sits
                    // under the caret — `COMPLETION_GUTTER` (which the completion
                    // popup hides behind its own padding) underestimates the real
                    // gutter, so the menu drifted ~18px left (§7.4). Through
                    // `content_x_of`, which is also what `run_menu_pos` finds the
                    // code column's right edge from.
                    //
                    // Stored in **content** coords, scroll included: the view
                    // subtracts the viewport itself, so the menu keeps up with a
                    // scroll rather than freezing where the caret was when it opened.
                    run_menu.set(Some(Point::new(
                        content_x_of(&sql) + below.x,
                        below.y + 4.0 + EDITOR_PAD_TOP,
                    )));
                    comp.open.set(false);
                } else {
                    (guarded_run_key)(sql.clone());
                }
            });
            return CommandExecuted::Yes;
        }
        if mods.shift() && matches!(kp.key, KeyInput::Keyboard(Key::Named(NamedKey::Tab), _)) {
            let cmd = Command::Edit(EditCommand::OutdentLine);
            editor_sig.with_untracked(|editor| {
                editor.doc().run_command(editor, &cmd, Some(1), mods);
            });
            return CommandExecuted::Yes;
        }
        // Alt+Up / Alt+Down: move the current line(s) up/down. Overrides Floem's
        // built-in MoveLineUp/Down, which slices line ranges assuming a trailing
        // `\n` and so merges the newline-less last line into its neighbour.
        // Computed in `core::text_ops::move_line`, applied as one full-buffer edit.
        if mods.alt()
            && !mods.control()
            && !mods.shift()
            && matches!(
                kp.key,
                KeyInput::Keyboard(Key::Named(NamedKey::ArrowUp | NamedKey::ArrowDown), _)
            )
        {
            let up = matches!(kp.key, KeyInput::Keyboard(Key::Named(NamedKey::ArrowUp), _));
            editor_sig.with_untracked(|e| {
                let doc = e.doc();
                let full = doc.text().to_string();
                let cur = e.cursor.get_untracked();
                let off = cur.offset();
                let (a, b) = cur.get_selection().unwrap_or((off, off));
                if let Some(edit) = move_line(&full, a.min(b), a.max(b), up) {
                    edit_untyped(
                        e,
                        comp,
                        Selection::region(0, full.len()),
                        &edit.text,
                        EditType::MoveLine,
                    );
                    e.cursor
                        .update(|c| c.set_insert(Selection::region(edit.sel.0, edit.sel.1)));
                }
            });
            return CommandExecuted::Yes;
        }
        // Auto-close bracket/quote pairs, type-over an existing closer, and wrap a
        // selection — via the pure, boundary-aware `core::pairs`. Plain character
        // input only (Ctrl/Alt combos are handled above and never reach here).
        //
        // Floem's editor inserts a typed character *unconditionally* after this
        // handler returns (it ignores our `CommandExecuted`), so when we take over
        // we must suppress that built-in insert: we flip the editor's `read_only`
        // true for the remainder of this key dispatch and restore it next tick.
        // The handler → built-in-insert step is synchronous, so there's no race or
        // flicker — the built-in `receive_char` sees `read_only` and no-ops.
        //
        // **`read_only` has a second owner**: the Ctrl+K preview freezes the buffer
        // with it while a suggestion is on screen (see the publish effect). So this
        // bows out entirely when the flag is already set, and restores what it
        // found rather than assuming `false`. Both halves matter — `doc.edit_single`
        // below goes through `Document::edit`, which `read_only` does *not* gate
        // (only `receive_char`/`run_command` are), so without the first check a
        // typed bracket edited straight through the freeze; and without the second
        // the deferred restore then cleared the freeze for the rest of the preview.
        if !mods.control()
            && !mods.alt()
            && let KeyInput::Keyboard(Key::Character(cs), _) = &kp.key
            && let Some(ch) = single_char(cs)
            && matches!(ch, '(' | ')' | '\'' | '"' | '`')
        {
            let dia = dialect.get_untracked();
            let acted = editor_sig.with_untracked(|e| {
                let ro = e.read_only;
                // Already frozen — by the Ctrl+K preview, the only other owner.
                // An edit here would bypass the freeze, so there is nothing to do.
                let was = ro.get_untracked();
                if was {
                    return false;
                }
                let doc = e.doc();
                let full = doc.text().to_string();
                let cur = e.cursor.get_untracked();
                let off = cur.offset();
                let (a, b) = cur.get_selection().unwrap_or((off, off));
                let Some(action) = pairs::auto_pair(&full, a, b, ch, dia) else {
                    return false;
                };
                // Suppress the built-in char insert for the rest of this dispatch,
                // then put the flag back the way it was rather than assuming false.
                ro.set(true);
                floem::action::exec_after(std::time::Duration::ZERO, move |_| {
                    if ro.try_get_untracked().is_some() {
                        ro.set(was);
                    }
                });
                match action {
                    PairAction::Insert {
                        start,
                        end,
                        insert,
                        sel,
                    } => {
                        doc.edit_single(
                            Selection::region(start, end),
                            &insert,
                            EditType::InsertChars,
                        );
                        e.cursor
                            .update(|c| c.set_insert(Selection::region(sel.0, sel.1)));
                    }
                    PairAction::Skip { caret } => {
                        e.cursor.update(|c| c.set_offset(caret, false, false));
                        // **Spend the typing one-shot here**, because nothing
                        // else will. It is set for every key before any branch
                        // can return, and consumed in exactly one place —
                        // `recompute_completions`, which only runs from the
                        // editor's `.update`, which floem fires only for
                        // *document deltas*. A type-over moves the caret and
                        // edits nothing, so the flag stood until the next
                        // document change — and if that arrived without a
                        // keypress (an OS-menu paste, a drop) it was judged as
                        // typing and popped the suggestion list. That is the
                        // exact bug the one-shot was introduced to fix.
                        comp.typed.set(false);
                    }
                }
                true
            });
            if acted {
                return CommandExecuted::Yes;
            }
            // Otherwise fall through so the character inserts normally.
        }
        // Backspace between an empty auto-inserted pair (`(|)`, `'|'`, …) deletes
        // both halves. Backspace is a Named key, so Floem's unconditional
        // char-insert never fires for it — returning `Yes` just pre-empts the
        // default DeleteBackward.
        if !mods.control()
            && !mods.alt()
            && !mods.shift()
            && matches!(
                kp.key,
                KeyInput::Keyboard(Key::Named(NamedKey::Backspace), _)
            )
        {
            let dia = dialect.get_untracked();
            let acted = editor_sig.with_untracked(|e| {
                let cur = e.cursor.get_untracked();
                if cur.get_selection().is_some_and(|(a, b)| a != b) {
                    return false; // let the default delete a real selection
                }
                let off = cur.offset();
                let full = e.doc().text().to_string();
                let Some((s, en)) = pairs::backspace_pair(&full, off, dia) else {
                    return false;
                };
                e.doc()
                    .edit_single(Selection::region(s, en), "", EditType::Delete);
                e.cursor.update(|c| c.set_offset(s, false, false));
                true
            });
            if acted {
                return CommandExecuted::Yes;
            }
        }
        default_key_handler(editor_sig)(kp, mods)
    });
    let ed = editor.editor().clone();
    let ed_cmdk = ed.clone(); // for the Ctrl+K popup (the editor's `.update` moves `ed`)
    let ed_menu = ed.clone(); // right-click handler: read caret offset
    let ed_menu2 = ed.clone(); // menu actions: anchor point for "Ask AI"
    let ed_fix = ed.clone(); // AI fix: anchor point for the Ctrl+K bar
    let ed_run = ed.clone(); // run menu: re-focus the editor after running
    let ed_hl = ed.clone(); // statement-highlight overlay geometry
    let ed_band = ed.clone(); // inline-diff band strips (gutter + right padding)
    let ed_syntax = ed.clone(); // syntax-squiggle overlay geometry
    let ed_bm = ed.clone(); // bracket-matching: recompute offsets on caret/text
    let ed_bm2 = ed.clone(); // bracket-matching overlay geometry
    let ed_occ = ed.clone(); // occurrences: recompute ranges on caret/text
    let ed_occ2 = ed.clone(); // occurrences overlay geometry
    let ed_vbar = ed.clone(); // custom vertical scrollbar geometry
    let ed_hbar = ed.clone(); // custom horizontal scrollbar geometry
    let ed_vdrag = ed.clone(); // vertical scrollbar drag → scroll
    let ed_hdrag = ed.clone(); // horizontal scrollbar drag → scroll
    let ed_bar_poke = ed.clone(); // auto-hide: poke the bars on scroll/resize
    let ed_wheel = ed.clone(); // shift+wheel → horizontal scroll
    let ed_find = ed.clone(); // Ctrl+F find: select + centre a match
    let ed_fmt = ed.clone(); // right-click "Format SQL"
    let ed_cursor = ed.clone(); // mirror caret offset out for the status bar
    let ed_goto = ed.clone(); // Ctrl+G go-to-line: move caret + centre
    let ed_jump = ed.clone(); // status-bar warning count: jump to first warning
    let ed_fmt_req = ed.clone(); // palette "Format Code": reformat on request
    let ed_mount = ed.clone(); // take focus once this pane is on screen

    // Focus the editor as soon as the pane exists — typing after opening a tab
    // should go into the query, not nowhere.
    //
    // One place covers every route because `center` rebuilds this pane whenever the
    // active tab changes: opening a tab, switching to one, restoring the session,
    // and each way the schema tree reaches a tab (double-click, Open, Open in new
    // tab, Find Anywhere). None of those callers has to remember to ask, and a new
    // one gets it for free.
    //
    // Deferred a tick, as `edit_field`'s autofocus is: the request has to land
    // after the view is mounted, or it is set on a view not yet in the tree and
    // dropped. The caret is wherever a fresh editor puts it — offset 0, i.e. line
    // 1, col 1.
    floem::action::exec_after(std::time::Duration::ZERO, move |_| {
        // The tab can be closed inside the same tick that opened it.
        let Some(Some(vid)) = ed_mount.editor_view_id.try_get_untracked() else {
            return;
        };
        // …and the pane can be rebuilt *behind* an open overlay, which owns the
        // keyboard for as long as it is mounted. Deleting a connection from
        // Manage Connections is the case: it takes that connection's tabs with
        // it, so the active tab changes and this pane is rebuilt — and the
        // editor then took focus out from under the modal the user was still
        // working in, which had to be clicked again before it answered anything.
        // Every route this autofocus is *for* is a tab the user just asked to
        // look at, and none of them leaves an overlay up.
        if crate::widgets::innermost_focus_root().is_some() {
            return;
        }
        vid.request_focus();
    });

    // Mirror the caret's byte offset into the tab's `cursor_offset` signal so the
    // status bar can render Ln/Col, and its selected range into `selection` for
    // the AI panel. Tracks `ed.cursor`, so it fires on every caret move /
    // selection change; disposed with this pane when the tab closes.
    create_effect(move |_| {
        let cur = ed_cursor.cursor.get();
        cursor_offset.set(cur.offset());
        // A point selection is floem's spelling of "no selection" (a caret is
        // still a region, `a == b`), and the tab's signal must mean the same, or
        // every turn reports a selection of nothing.
        // Ordered, because a selection dragged upwards arrives end-first and a
        // reversed range reads as "nothing selected" downstream.
        selection.set(
            cur.get_selection()
                .map(|(a, b)| (a.min(b), a.max(b)))
                .filter(|(a, b)| a != b),
        );
    });

    // Bracket matching: recompute the matched-paren offsets on every caret move
    // (tracks `ed.cursor`) and edit (tracks `query`). Pure/boundary-aware via
    // `core::pairs::match_paren`, so parens inside strings/comments are ignored.
    create_effect(move |_| {
        let caret = ed_bm.cursor.get().offset();
        let sql = query.get();
        bracket_match.set(pairs::match_paren(&sql, caret, dialect.get_untracked()));
    });

    // Highlight all occurrences of the identifier under the caret. Same triggers
    // as bracket matching (caret + text); pure/boundary-aware via
    // `core::pairs::identifier_occurrences` (keywords/numbers/strings excluded).
    create_effect(move |_| {
        let caret = ed_occ.cursor.get().offset();
        let sql = query.get();
        ident_occurrences.set(pairs::identifier_occurrences(
            &sql,
            caret,
            dialect.get_untracked(),
        ));
    });

    // Jump the caret to a byte offset requested from outside (the status-bar
    // warning count), then clear the request. Centres + refocuses like Go-to-line.
    create_effect(move |_| {
        let Some(off) = jump_offset.get() else {
            return;
        };
        ed_jump.cursor.update(|c| c.set_offset(off, false, false));
        ed_jump.center_window();
        if let Some(Some(vid)) = ed_jump.editor_view_id.try_get_untracked() {
            vid.request_focus();
        }
        jump_offset.set(None);
    });

    // Reformat on request from outside the pane (the palette's "Format Code"),
    // then clear it — same shape as the jump above, and deliberately the *same*
    // `format_editor` Ctrl+Alt+L and the right-click "Format SQL" use, so the
    // three can't disagree about what formatting means. The palette used to set
    // `query` itself, which the mounted editor never reads back (see
    // `Tab::format_req`), so the command silently did nothing.
    create_effect(move |_| {
        if !format_req.get() {
            return;
        }
        format_editor(&ed_fmt_req, comp, dialect.get_untracked());
        format_req.set(false);
    });

    // Insert text at the caret on request (the snippet library), then clear —
    // the same shape again. It goes through `edit_untyped` rather than `query`
    // for the reason `format_req` does: the mounted editor owns the document,
    // and a caller that rewrites the signal is writing behind it.
    {
        let ed_ins = ed_fmt.clone();
        create_effect(move |_| {
            let Some(body) = insert_req.get() else {
                return;
            };
            insert_req.set(None);
            if body.is_empty() {
                return;
            }
            // Over the selection when there is one — inserting a snippet with
            // text selected means "replace this with it", as typing would.
            let (a, b) = ed_ins
                .cursor
                .get_untracked()
                .get_selection()
                .unwrap_or_else(|| {
                    let off = ed_ins.cursor.get_untracked().offset();
                    (off, off)
                });
            let (lo, hi) = (a.min(b), a.max(b));
            edit_untyped(
                &ed_ins,
                comp,
                Selection::region(lo, hi),
                &body,
                EditType::Other,
            );
            // Caret after the inserted text, so typing continues where the
            // snippet left off rather than in front of it.
            ed_ins
                .cursor
                .update(|cc| cc.set_offset(lo + body.len(), false, false));
        });
    }

    // ── AI fix ──────────────────────────────────────────────────────────────
    //
    // One action, asked for three ways: the error bar, the modal behind its
    // "View" (through `fix_req`), and the editor menu's "AI fix" over a
    // squiggled statement. It opens the Ctrl+K overlay pre-filled and goes
    // straight to Busy (skipping the compact prompt); the user only approves or
    // rejects the resulting diff, and nothing runs.
    //
    // `range` is what the model is allowed to rewrite, and it is the caller's to
    // decide because only the caller knows which statement it means. The wording
    // is `prompt::ai_fix_prompt`'s, so the three can't drift apart in what they
    // ask the model for — the point of doing this as one action.
    let fix_with_ai: FixFn = {
        let inline_ai_run = inline_ai_run.clone();
        let ed_fix = ed_fix.clone();
        Rc::new(move |(lo, hi), problems: Vec<String>, origin| {
            let sql = query.get_untracked();
            let Some(stmt) = sql.get(lo..hi).filter(|s| !s.trim().is_empty()) else {
                return;
            };
            let stmt = stmt.to_string();
            let Some(p) = prompt::ai_fix_prompt(&problems, origin, ai_data.get_untracked()) else {
                return;
            };
            cmdk.start.set(lo);
            cmdk.end.set(hi);
            cmdk.input.set(p.input.clone());
            // The fenced, provenance-flagged instruction, kept beside the label
            // in the box so a retry sends *this* — see `CmdK::intent`.
            cmdk.intent.set(Some((p.input.clone(), p.intent.clone())));
            // **Select what will be replaced**, exactly as Ctrl+K and "Ask AI"
            // do — and this is the entry point that needs it most: the range is
            // `error_fix_range`'s choice rather than a gesture of the user's, so
            // without the selection nothing on screen tells them whether one
            // statement or the whole script is about to be rewritten.
            ed_fix
                .cursor
                .update(|cc| cc.set_insert(Selection::region(lo, hi)));
            // Under the statement being fixed, and scrolled into view — without
            // this the bar opened wherever the last Ctrl+K left its anchor.
            anchor_cmdk(&ed_fix, cmdk, hi, area_h);
            inline_ai.set(InlineAiState::Busy);
            cmdk.open.set(true);
            (inline_ai_run)(InlineAiRequest {
                intent: p.intent,
                current_sql: sql.clone(),
                selection: Some(stmt),
            });
        })
    };

    // A run error's fix: the failing statement, not the whole buffer. What ran
    // may be a script, and rewriting all of it to correct the one statement that
    // failed is what `intel::error_fix_range` exists to stop.
    //
    // Takes the message rather than reading `results` itself, because its two
    // callers hold different answers to "which error": the bar is showing the
    // live one, the modal is showing the one it opened on.
    let fix_error: Rc<dyn Fn(String)> = {
        let fix_with_ai = fix_with_ai.clone();
        Rc::new(move |err: String| {
            let range = query
                .with_untracked(|sql| intel::error_fix_range(sql, &err, dialect.get_untracked()));
            (fix_with_ai)(range, vec![err], FixOrigin::Run);
        })
    };
    let ai_fix: Rc<dyn Fn()> = {
        let fix_error = fix_error.clone();
        Rc::new(move || {
            let QueryState::Failed(err) = results.get_untracked() else {
                return;
            };
            (fix_error)(err);
        })
    };

    // The other half of the pair, and it deliberately goes somewhere else: a fix
    // is a diff in the editor with an Approve behind it, an explanation is prose
    // in the chat panel, which is where the menu's own "Explain" sends a
    // statement. Reveal first, then send — a message into a hidden panel reads
    // as the button doing nothing.
    //
    // It highlights the statement it asked about, like every other action here
    // that scopes to one, so the answer and the SQL it is about are visibly the
    // same statement.
    let explain_error: Rc<dyn Fn()> = {
        let ai_send = ai_send.clone();
        Rc::new(move || {
            let QueryState::Failed(err) = results.get_untracked() else {
                return;
            };
            let sql = query.get_untracked();
            let (lo, hi) = intel::error_fix_range(&sql, &err, dialect.get_untracked());
            let Some(p) =
                prompt::explain_error_prompt(sql.get(lo..hi), &err, ai_data.get_untracked())
            else {
                return;
            };
            crate::reveal_ai_panel(right_panel);
            (ai_send)(p);
            highlight_pick(&sql, lo, hi, highlight);
        })
    };

    // The same fix asked for from the error modal ("View" on the bar). It has to
    // come through a request signal because `cmdk` is this pane's own state —
    // the modal is rendered by the workspace, which has no way to reach it. Same
    // request-and-clear shape as `format_req`, carrying the message the modal
    // was showing.
    {
        let fix_error = fix_error.clone();
        create_effect(move |_| {
            let Some(err) = fix_req.get() else {
                return;
            };
            fix_req.set(None);
            (fix_error)(err);
        });
    }

    // Builds the right-click menu entries (Ask AI / Explain / Optimize) for the
    // app-wide `popup_menu` overlay. Rebuilt per right-click; each action reads
    // `menu_offset` (the caret the right-click landed on) lazily, so it scopes to
    // the statement there. `menu_panel` auto-closes after an action runs.
    let build_editor_menu: Rc<dyn Fn() -> Vec<MenuEntry>> = {
        let ed_menu_act = ed_menu2.clone();
        let ai_send = ai_send.clone();
        let inline_ai_run = inline_ai_run.clone();
        let open_plan = open_plan.clone();
        let ed_fmt = ed_fmt.clone();
        let create_view = create_view.clone();
        let fix_with_ai = fix_with_ai.clone();
        Rc::new(move || {
            let ed_ask = ed_menu_act.clone();
            let ai_explain = ai_send.clone();
            let run_optimize = inline_ai_run.clone();
            let show_plan = open_plan.clone();
            let ed_format = ed_fmt.clone();
            let make_view = create_view.clone();
            // "AI fix" — offered only over a statement the editor has something
            // to say about, which is what the user is pointing at when they
            // right-click a wavy underline. Both sources, because both draw one:
            // the offline squiggles and the DB-validated ones.
            //
            // The range and the messages are **re-derived when the entry runs**,
            // like every other action in this menu, and the build-time pass only
            // decides whether to offer it. Captured, the offsets could outlive
            // the text they described — a reload or an insert between the
            // right-click and the click leaves `sql.get(lo..hi)` answering
            // `None`, and the entry then does nothing at all with nothing said.
            //
            // `with_untracked`, not `get_untracked`: this needs two offsets, and
            // the `get` it replaces copied the whole buffer out of the rope to
            // find them — on every right-click, in a script that can be 190 KB.
            let statement_problems = move || {
                let (lo, hi) = query.with_untracked(|sql| {
                    statement_range(sql, menu_offset.get_untracked(), dialect.get_untracked())
                });
                let mut diags = syntax.get_untracked();
                db_diag.with_untracked(|d| diags.extend_from_slice(d));
                ((lo, hi), intel::problems_in_range(&diags, lo, hi))
            };
            let fix_entry = {
                let fix_with_ai = fix_with_ai.clone();
                let has_problems = !statement_problems().1.is_empty();
                has_problems.then(|| {
                    MenuEntry::action_icon(
                        "AI fix",
                        (icons::SPARKLES, theme::key_foreign),
                        move || {
                            let (range, problems) = statement_problems();
                            (fix_with_ai)(range, problems, FixOrigin::Editor);
                        },
                    )
                })
            };
            // The three AI actions carry the sparkle (matching AI Summary in the
            // grid); Plan + Format sit below the separator as plain actions.
            let mut entries = vec![
                MenuEntry::action_icon(
                    "Ask AI",
                    (icons::SPARKLES, theme::key_foreign),
                    move || {
                        let off = menu_offset.get_untracked();
                        // Widen to the statement the right-click landed in, exactly
                        // as Ctrl+K does — this is the same action reached another
                        // way, so it must pick the same thing. Left as a bare caret
                        // it asked the model to edit an insertion point.
                        let sql = ed_ask.doc().text().to_string();
                        let (start, end) = statement_range(&sql, off, dialect.get_untracked());
                        cmdk.start.set(start);
                        cmdk.end.set(end);
                        ed_ask
                            .cursor
                            .update(|cc| cc.set_insert(Selection::region(start, end)));
                        anchor_cmdk(&ed_ask, cmdk, end, area_h);
                        cmdk.input.set(String::new());
                        cmdk.intent.set(None);
                        inline_ai.set(InlineAiState::Idle);
                        cmdk.open.set(true);
                    },
                ),
                MenuEntry::action_icon(
                    "Explain",
                    (icons::SPARKLES, theme::key_foreign),
                    move || {
                        let sql = query.get_untracked();
                        let (lo, hi) = statement_range(
                            &sql,
                            menu_offset.get_untracked(),
                            dialect.get_untracked(),
                        );
                        if let Some(stmt) = sql.get(lo..hi).filter(|s| !s.is_empty()) {
                            reveal_ai_panel(right_panel);
                            (ai_explain)(format!("Explain this SQL query:\n```sql\n{stmt}\n```"));
                            highlight_pick(&sql, lo, hi, highlight);
                        }
                    },
                ),
                MenuEntry::action_icon(
                    "Optimize",
                    (icons::SPARKLES, theme::key_foreign),
                    move || {
                        let sql = query.get_untracked();
                        let (lo, hi) = statement_range(
                            &sql,
                            menu_offset.get_untracked(),
                            dialect.get_untracked(),
                        );
                        if let Some(stmt) = sql.get(lo..hi).filter(|s| !s.is_empty()) {
                            let stmt = stmt.to_string();
                            cmdk.start.set(lo);
                            cmdk.end.set(hi);
                            cmdk.input.set("Optimize this query".to_string());
                            // The box shows a label; the instruction below is
                            // what the model is actually sent, retry included.
                            cmdk.intent.set(Some((
                                "Optimize this query".to_string(),
                                OPTIMIZE_INTENT.to_string(),
                            )));
                            inline_ai.set(InlineAiState::Busy);
                            cmdk.open.set(true);
                            (run_optimize)(InlineAiRequest {
                                intent: "Rewrite this SQL query to be more efficient and \
                                readable while preserving its exact result set. Return \
                                only the SQL."
                                    .to_string(),
                                current_sql: sql.clone(),
                                selection: Some(stmt),
                            });
                            highlight_pick(&sql, lo, hi, highlight);
                        }
                    },
                ),
                MenuEntry::Separator,
                MenuEntry::action("Plan", move || {
                    let sql = query.get_untracked();
                    let dia = dialect.get_untracked();
                    let (lo, hi) = statement_range(&sql, menu_offset.get_untracked(), dia);
                    if let Some(stmt) = sql.get(lo..hi).filter(|s| !s.trim().is_empty()) {
                        // **The substituted statement, like every other path that
                        // sends text to the server.** EXPLAIN was handed the raw
                        // template and answered `ERROR 1064 … near ':id'`, about
                        // text the user has already filled in in the bar directly
                        // below. `167c87b` added exactly this to live-validate and
                        // not here. A value still missing leaves the template
                        // alone; the plan modal then shows the engine's complaint,
                        // which is the same thing Run does.
                        let stmt = params::substitute(stmt, &tab_params.get_untracked(), dia)
                            .unwrap_or_else(|_| stmt.to_string());
                        (show_plan)(stmt);
                        highlight_pick(&sql, lo, hi, highlight);
                    }
                }),
                MenuEntry::action("Format", move || {
                    format_editor(&ed_format, comp, dialect.get_untracked())
                }),
            ];
            // First when it's there: over a squiggle it is the most specific
            // thing this menu can offer, and the one the user came for.
            if let Some(fix) = fix_entry {
                entries.insert(0, fix);
            }
            // "Create view" only when there's a query to make one *out of* — the
            // statement the right-click landed on has to be something a view's
            // body may be (`can_be_view_body`, the same rule the editor's own
            // validation uses). Shown rather than disabled: on a `DELETE`, or on
            // a read-only connection, the entry has nothing to offer.
            let sql = query.get_untracked();
            let (lo, hi) =
                statement_range(&sql, menu_offset.get_untracked(), dialect.get_untracked());
            let stmt = sql.get(lo..hi).unwrap_or_default().trim().to_string();
            if !read_only.get_untracked()
                && active_db.get_untracked().is_some()
                // …and an engine this build can emit a `CREATE VIEW` for. The
                // view emitter is MySQL/PostgreSQL-shaped — see
                // `ddl::supports_view_editing` — so on SQLite the entry would
                // open a modal ending at a statement the engine has no form of.
                && schemaic_core::ddl::supports_view_editing(dialect.get_untracked())
                && schemaic_core::ddl::can_be_view_body(&stmt)
            {
                // Its own group, as in the schema tree: it's the one entry here
                // that ends at a statement against the database.
                entries.push(MenuEntry::Separator);
                entries.push(MenuEntry::action("Create view", move || {
                    (make_view)(stmt.clone());
                }));
            }
            entries
        })
    };

    // Shift+wheel scrolls horizontally. The editor owns its scroll internally, so
    // `shift_hscroll` (which wraps our own `scroll()`) can't reach it; instead we
    // register a `PointerWheel` listener directly on the internal scroll view — the
    // parent of `editor_view_id`. Floem's scroll runs its own listeners *before* its
    // default wheel handling (see `Scroll::event_after_children`), so returning
    // `Stop` suppresses the vertical scroll and we drive a horizontal delta through
    // `scroll_delta` (the same channel the built-in wheel/`shift_hscroll` use).
    if let Some(scroll_id) = ed.editor_view_id.get_untracked().and_then(|c| c.parent()) {
        scroll_id.add_event_listener(
            EventListener::PointerWheel,
            Box::new(move |e| {
                if let Event::PointerWheel(pe) = e {
                    // Ctrl+wheel zooms the editor font (temporary, per-tab). Checked
                    // before shift so it wins; scroll up = zoom in.
                    if pe.modifiers.control() {
                        let dy = pe.delta.y;
                        if dy != 0.0 {
                            let cur = zoom.get_untracked().unwrap_or_else(theme::editor_font_size);
                            let next = (cur + if dy < 0.0 { ZOOM_STEP } else { -ZOOM_STEP })
                                .clamp(ZOOM_MIN, ZOOM_MAX);
                            if Some(next) != zoom.get_untracked() {
                                zoom.set(Some(next));
                                // Invalidate the editor's cached layout so the new
                                // size takes effect now (same lever as the font
                                // setting; only the active tab's editor is mounted).
                                theme::bump_editor_generation();
                            }
                        }
                        return EventPropagation::Stop;
                    }
                    if pe.modifiers.shift() {
                        // Windows delivers shift+wheel as a vertical delta; map it to x.
                        let dx = if pe.delta.x != 0.0 {
                            pe.delta.x
                        } else {
                            pe.delta.y
                        };
                        if dx != 0.0 {
                            ed_wheel.scroll_delta.set(floem::kurbo::Vec2::new(dx, 0.0));
                        }
                        return EventPropagation::Stop;
                    }
                }
                EventPropagation::Continue
            }),
        );
    }

    // The editor's scroll rect, lifted out of `ed` before the closures below consume
    // it. The caret-anchored popups subtract its origin to turn `points_of_offset`'s
    // document coordinates into `editor_area` ones (see the "Overlay geometry" note).
    let ed_vp = ed.viewport;
    // Editor-area width, tracked so right-click / run menus flip leftward and the
    // completion popup slides left instead of being clipped by the pane edge (they
    // all live in editor-area coords).
    let area_w: RwSignal<f64> = RwSignal::new(0.0);

    // Hide the editor's blinking caret AND the current-line highlight whenever the
    // editor isn't focused (e.g. focus is on the schema panel, terminal, or the
    // Ctrl+K overlay — and, crucially, on first load: the editor must not look
    // active before the user clicks into it). Floem's `text_editor_keys` hardcodes
    // the caret's `is_active` to always-true and paints the current-line band
    // unconditionally, so we drive both off the editor's focus triggers:
    //   • focus lost / initial → pin the caret hidden, invalidate the blink timer
    //     (its pending tick then no-ops and stops rescheduling), and clear the
    //     `editor_focused` flag so the style drops the current-line highlight;
    //   • focus gained → `reset()` restarts a fresh blink and sets the flag.
    // Effects run once at creation in order, so the focus-lost effect (created
    // second) wins → the editor starts unfocused-looking on load.
    // Actual keyboard-focus state, tracked off the editor's own focus triggers.
    // Drives the caret colour + current-line highlight so an unfocused editor
    // reads as inert — in particular it must look inert on app load, where Floem
    // never gives it real focus but still paints a blinking caret.
    //
    // Why we can't just hide the caret via `cursor_info.hidden`: Floem hardcodes
    // the editor's `is_active` to `true` (`text_editor_keys`) and, via
    // `create_view_effects`, calls `cursor_info.reset()` on every cursor change —
    // which forces `hidden` back to `false` and re-arms the blink out from under
    // any value we set. So instead we let the caret paint but make its *colour*
    // transparent while unfocused (see `editor_style`'s `cursor_color` below); a
    // transparent caret is invisible regardless of the blink.
    let editor_focused = RwSignal::new(false);
    {
        let ed_focus = ed.clone();
        create_effect(move |_| {
            ed_focus.editor_view_focused.track();
            editor_focused.set(true);
        });
        let ed_blur = ed.clone();
        create_effect(move |_| {
            ed_blur.editor_view_focus_lost.track();
            editor_focused.set(false);
            // Clicking away from the editor (schema panel, terminal, another tab)
            // dismisses a stray completion popup + signature help too.
            comp.open.set(false);
            comp.sig.set(None);
        });
        // Signature help follows the *caret*, not just edits: recompute on every
        // cursor change (typing, arrow keys, click) so it tracks the active parameter
        // and hides the moment the caret leaves the call's parentheses. Focus is read
        // untracked so a programmatic cursor change on an unfocused editor can't pop a
        // phantom hint (blur clears it separately).
        let ed_caret = ed.clone();
        create_effect(move |_| {
            ed_caret.cursor.track();
            query.track(); // also on text edits that don't move the caret (forward-delete)
            if editor_focused.get_untracked() {
                update_signature_help(&ed_caret, comp, dialect.get_untracked());
            }
        });
    }
    // Floem's editor over-reports its content width on the first layout, so a
    // spurious horizontal scrollbar shows until something makes the bars look
    // again. Neither input the bars measure is a signal — `max_line_width()` is a
    // plain read and the viewport is only correct once the final editor style has
    // been applied — so the bar closures have no way to see the layout settle.
    // This generation is what tells them: bumped once the layout has, it re-runs
    // both style closures against the settled numbers.
    //
    // It must NOT be a scroll. This was a 1px `scroll_delta`, which re-ran the
    // closures as a side effect of moving the viewport — and left it moved: a
    // document wide enough to scroll stayed parked 1px in, so every tab switch
    // nudged the text left by a pixel and only a short document clamped back.
    let bar_gen: RwSignal<u64> = RwSignal::new(0);
    floem::action::exec_after(std::time::Duration::from_millis(200), move |_| {
        // Guard: the tab (and this editor's scope) may be gone within 200 ms.
        let _ = bar_gen.try_update(|g| *g += 1);
    });
    // The document the Ctrl+K phantom-row wrapper will go around. Taken here, but
    // installed at the *end* of the builder chain — see the `use_doc` call after
    // `.update(…)` for why the order is load-bearing.
    let inner_doc = editor.doc();
    let input = editor
        // `SqlStyling` reads the INNER document deliberately: same rope, same
        // `cache_rev` signal (the wrapper delegates both), and it must not depend
        // on a wrapper installed later in this chain.
        .styling(sql_highlight::SqlStyling::new(
            inner_doc.clone(),
            dialect.get_untracked(),
            zoom,
            cmdk.preview,
        ))
        // `smart_tab` makes Tab insert spaces to the next tab stop; without it
        // Tab inserts a literal '\t' while OutdentLine assumes space indentation,
        // so Shift+Tab removes ALL indentation instead of one level.
        // `wrap_method` follows the word-wrap setting (default off = scroll long
        // SQL lines horizontally; on = wrap to the viewport width).
        .editor_style(move |s| {
            // Editor-theme driven (One Dark Pro / Tokyo Night / Catppuccin Latte).
            // Reactive: `editor_style` re-runs when the editor-theme signal
            // changes, so cursor/selection/gutter re-apply live. (Base fg/bg live
            // on the view `.style()` below, which feeds the editor's text color.)
            let t = theme::editor_theme();
            // Indentation settings (reactive: editing them re-runs this closure).
            let soft = theme::editor_soft_tabs();
            let tw = theme::editor_tab_width();
            let wrap = theme::editor_word_wrap();
            // Caret paints transparent while the editor is unfocused — Floem always
            // paints (and blinks) the caret regardless of focus, so hiding it any
            // other way is fought by its internal blink reset (see the focus block
            // above). This is what keeps the editor from looking active on load.
            let caret = if editor_focused.get() {
                t.cursor
            } else {
                floem::peniko::Color::TRANSPARENT
            };
            let s = s
                .cursor_color(caret)
                .selection_color(t.selection)
                .gutter_dim_color(t.gutter_fg)
                .gutter_accent_color(t.fg)
                .indent_guide_color(t.selection)
                .visible_whitespace(t.selection)
                // Soft tabs → insert spaces to the next stop; hard tabs → literal
                // `\t`. `tab_width` sets both the indent size and the display width.
                .smart_tab(soft)
                .indent_style(if soft {
                    IndentStyle::Spaces(tw as u8)
                } else {
                    IndentStyle::Tabs
                })
                // Word wrap wraps long lines to the viewport width; off = scroll
                // horizontally (the original behaviour).
                .wrap_method(if wrap {
                    WrapMethod::EditorWidth
                } else {
                    WrapMethod::None
                })
                // Tuck the line numbers closer to the editor's left edge
                // (Floem's default is 25).
                .gutter_left_padding(14.0)
                // Virtual space below the document: the last row can be scrolled
                // up to the top of the viewport, so the line you're editing never
                // has to sit on the bottom edge. Floem adds it as a bottom margin
                // of `min(viewport, text) − one line` — which `scrollbar_geo`
                // has to count, since our own scrollbars measure the content.
                .scroll_beyond_last_line(true);
            // Current-line highlight — body AND gutter — only while focused; an
            // unfocused editor looks inert. The gutter band in particular must not
            // linger on blur (it also scrolled oddly with horizontal scroll).
            if editor_focused.get() {
                s.current_line_color(t.current_line)
                    .gutter_current_color(t.current_line)
            } else {
                let clear = floem::peniko::Color::TRANSPARENT;
                s.current_line_color(clear).gutter_current_color(clear)
            }
        })
        .update(move |_| {
            let text = ed.doc().text().to_string();
            query.set(text.clone());
            // Defer the completion recompute one tick. Inside `.update` the doc
            // is already edited but the caret hasn't advanced past the just-typed
            // char yet, so reading `cursor.offset()` here yields a prefix one char
            // behind (and misses a just-typed `.`). exec_after(0) runs after the
            // edit settles, when the caret is correct. The disposed-signal guard
            // covers the tab being closed within that tick (exec_after timers
            // aren't cancelled on scope teardown).
            {
                let ed = ed.clone();
                floem::action::exec_after(std::time::Duration::ZERO, move |_| {
                    if comp.open.try_get_untracked().is_none() {
                        return;
                    }
                    let adb = active_db.get_untracked();
                    recompute_completions(
                        &ed,
                        CompletionCtx {
                            db_nodes,
                            hidden_dbs,
                            active_db: adb.as_deref(),
                            dialect: dialect.get_untracked(),
                            snippets,
                            conn_id: active_conn.get_untracked(),
                        },
                        comp,
                        false,
                    );
                });
            }
            // Re-run catalog-aware diagnostics (drives the squiggles), debounced so
            // rapid typing coalesces into a single parse. Only the latest generation
            // applies; the disposed-signal guard covers a tab closed within the tick.
            {
                let g = diag_gen.get().wrapping_add(1);
                diag_gen.set(g);
                let text = text.clone();
                let dgen = diag_gen.clone();
                floem::action::exec_after(std::time::Duration::from_millis(120), move |_| {
                    if dgen.get() != g || syntax.try_get_untracked().is_none() {
                        return;
                    }
                    syntax.set(compute_diagnostics(
                        &text,
                        db_nodes,
                        active_db.get_untracked().as_deref(),
                        dialect.get_untracked(),
                    ));
                });
            }
            // Tier-2: debounced live DB validation of the statement under the
            // cursor. Clear any stale DB squiggle immediately, then (if enabled)
            // schedule a round-trip that fires only if this is still the latest edit
            // and the statement parses cleanly (never nag a half-typed fragment).
            db_diag.set(Vec::new());
            if live_validate.get_untracked() {
                let g = val_gen.get().wrapping_add(1);
                val_gen.set(g);
                let text2 = text.clone();
                let ed2 = ed.clone();
                let validate = validate_stmt.clone();
                let vgen = val_gen.clone();
                // Snapshot the dialect now (plain value) — reading the memo inside the
                // deferred closure could touch a disposed scope if the tab closed.
                let dia = dialect.get_untracked();
                floem::action::exec_after(std::time::Duration::from_millis(500), move |_| {
                    if vgen.get() != g {
                        return;
                    }
                    let caret = match ed2.cursor.try_get_untracked() {
                        Some(c) => c.offset(),
                        None => return, // pane disposed
                    };
                    let (lo, hi) = statement_range(&text2, caret, dialect.get_untracked());
                    if lo >= hi || !intel::parses(&text2[lo..hi], dia) {
                        return;
                    }
                    // A statement with an unfilled `:name` is a *template*, and
                    // there is nothing to validate: the engine has never seen it
                    // and would answer with a syntax error about the placeholder.
                    // `intel::parses` does not stop this — `sqlparser` accepts
                    // `:id` as a placeholder in every dialect we speak, so the
                    // round-trip would happen and the squiggle would be the
                    // server's complaint about text the user is still filling in.
                    if !params::scan(&text2[lo..hi], dia).is_empty() {
                        return;
                    }
                    let vgen2 = vgen.clone();
                    let on_done: ValidateDoneFn = Rc::new(move |diags| {
                        // Ignore a result the user has already typed past, and skip
                        // if the pane was disposed while the round-trip was in flight.
                        if vgen2.get() == g && db_diag.try_get_untracked().is_some() {
                            db_diag.set(diags);
                        }
                    });
                    validate(text2.clone(), lo, hi, on_done);
                });
            }
            // Any edit (typing, or the AI-fix Approve) dismisses a stale error
            // bar — the error no longer describes the current text.
            if matches!(results.get_untracked(), QueryState::Failed(_)) {
                results.set(QueryState::Idle);
            }
            // Typing clears the picked-statement highlight (it no longer maps to
            // the edited text).
            highlight.set(None);
        })
        // Wrap the document so a pending Ctrl+K suggestion can be rendered as
        // phantom rows *in the line flow*, without the rope ever holding text the
        // user didn't write (`inline_diff`).
        //
        // **This has to come after `.update(…)` above, and after any other
        // `TextEditor` builder that registers a document callback**
        // (`placeholder`, `pre_command`). Those all resolve the document with
        // `downcast_rc::<TextDocument>()` and *silently do nothing* when it isn't
        // one — no error, no warning. Wrapping first therefore dropped the
        // `query.set(…)` sync on the floor: the signal every consumer treats as
        // the tab's SQL stopped following the editor at all, so autosave, Run,
        // diagnostics and Ctrl+K's own statement lookup were reading whatever text
        // the tab happened to open with.
        .use_doc(Rc::new(inline_diff::InlineDiffDoc::new(
            inner_doc,
            cmdk.preview,
            dialect.get_untracked(),
        )) as Rc<dyn floem::views::editor::text::Document>)
        .style(|s| {
            s.width_full()
                .flex_grow(1.0_f32)
                .min_height(0.0)
                // Editor-theme surface + default text colour. `.color()` here
                // feeds the editor's `TextColor`, which is the fallback colour for
                // every glyph a syntax token doesn't override (identifiers,
                // punctuation) — essential for the light editor theme.
                .color(theme::editor_theme().fg)
                .background(theme::code_bg())
                .class(GutterClass, |s| s.background(theme::code_bg()))
                // NB: padding here is a no-op — the editor is a scroll view and its
                // content scrolls *under* its own padding. All breathing room lives
                // on the wrapper (`editor_box`) instead.
                // Hide the editor's built-in scrollbars entirely (zero-thickness,
                // transparent): they float at the *content* edge, so they'd sit atop
                // the last line / inside the wrapper padding. We paint custom overlay
                // bars pinned to the border instead (see `v_scrollbar`/`h_scrollbar`
                // below), which also lets them auto-hide like the app's other panels.
                .class(Handle, |s| {
                    s.set(Thickness, Px(0.0))
                        .background(floem::peniko::Color::TRANSPARENT)
                })
        })
        // Right-click → editor AI menu. The editor's own handler already ran on
        // this PointerDown (its `right_click` moved the caret to the click via an
        // accurate hit-test), so we just read the caret offset — no coordinate
        // guessing — and anchor the menu at the click point.
        .on_event(EventListener::PointerDown, move |e| {
            if let Event::PointerDown(pe) = e {
                // Any click in the editor clears the picked-statement highlight
                // and dismisses the unsafe-run notice (without executing).
                highlight.set(None);
                if guard.get_untracked().is_some() {
                    guard.set(None);
                }
                // Ctrl+middle-click resets the temporary font zoom to the user's size.
                if pe.button.is_auxiliary() && pe.modifiers.control() {
                    if zoom.get_untracked().is_some() {
                        zoom.set(None);
                        theme::bump_editor_generation();
                    }
                    return EventPropagation::Stop;
                }
                if pe.button.is_secondary() {
                    let off = ed_menu.cursor.get_untracked().offset();
                    menu_offset.set(off);
                    // Open at the cursor (window coords via `last_mouse`), rendered
                    // at the workspace root so it floats over the results pane.
                    popup_anchor.set(None);
                    popup_width.set(120.0);
                    popup_menu.set(Some((build_editor_menu)()));
                    comp.open.set(false);
                    return EventPropagation::Stop;
                }
            }
            EventPropagation::Continue
        });

    // Wrapper owns the border, rounding, and ALL the breathing room — padding on
    // the editor view itself is ignored (its content scrolls under it), so the
    // insets must live here. The editor fills the padded content box; the top
    // inset is the `EDITOR_PAD_TOP` const the overlays compensate against. Left
    // has no padding (the gutter stays flush via `gutter_left_padding`). The custom
    // overlay scrollbars below are pinned to the border (not the content edge), so
    // this padding now cleanly separates the code from both the border and the bars.
    let editor_box = container(input)
        // A click in the editor repositions the caret (no edit fires, so the
        // recompute path doesn't run) → dismiss a stale completion popup here.
        // `cont` so the editor still handles the click and places the caret (TODO).
        .on_event_cont(EventListener::PointerDown, move |_| {
            comp.open.set(false);
            // Also dismiss the Ctrl+K prompt — but **only while it is still just a
            // prompt**. A click used to be safe to treat as "never mind" because
            // the working and diff states covered the editor, so a click in them
            // could not land here at all. Neither covers it now: the diff in
            // particular sits *in* the lines, so clicking the very thing being
            // decided on — or anywhere near it — threw the suggestion away, and a
            // generation the user waited for is far too expensive to lose to a
            // stray click. Escape dismisses, and Reject is right there.
            if cmdk.open.get_untracked() && matches!(inline_ai.get_untracked(), InlineAiState::Idle)
            {
                cmdk.open.set(false);
            }
        })
        .style(|s| {
            s.flex_grow(1.0_f32)
                .width_full()
                .flex_col()
                .min_height(0.0)
                .min_width(0.0)
                .background(theme::code_bg())
                .border(1.0)
                .border_color(theme::border())
                .border_radius(6.0)
                .padding_top(EDITOR_PAD_TOP)
                .padding_bottom(theme::scaled(10.0))
                .padding_right(theme::scaled(5.0))
        });

    // Custom overlay scrollbars, replacing the editor's built-in bars (hidden
    // above). Two wins over the built-ins: (1) they pin to the editor *border*
    // instead of floating at the (padding-inset) content edge, so they clear the
    // code; (2) they auto-hide 3s after scroll activity, like the terminal / schema
    // tree / AI panel. Both are read-only indicators (no drag), positioned in
    // `editor_area` coords (== `editor_box`'s border box). Geometry derives from the
    // editor's live `viewport` (scroll offset + visible size) vs. the content size
    // (`max_line_width` / `(last_line+1) * line_height`). `query.get()` re-runs the
    // closure on edits (content size isn't a signal); `viewport.get()` on scroll.
    let (esbar_shown, esbar_poke) = autohide_state();
    {
        // Poke the auto-hide timer whenever the viewport moves (scroll or resize).
        let poke = esbar_poke.clone();
        create_effect(move |_| {
            ed_bar_poke.viewport.track();
            poke();
        });
    }
    // Content top inside `editor_area`: 1px border + the top padding inset.
    const ESBAR_TOP: f64 = 1.0 + EDITOR_PAD_TOP;
    // Vertical bar geometry, shared by the style closure and the drag handler:
    // returns `(track_h, thumb_h, max_scroll)` for the current viewport/content, or
    // `None` when there's no vertical overflow.
    let v_geo = move |ed: &floem::views::editor::Editor| -> Option<(f64, f64, f64)> {
        // **Visual** lines, not buffer lines. Under word wrap one buffer line can
        // occupy thirty visual rows, and counting newlines said the content fit —
        // so the thumb was hidden and, since the editor's own bars are disabled,
        // a wrapped document had no vertical scrollbar at all while the wheel
        // still scrolled it. `last_vline` degrades to `last_line` with wrap off.
        // (The horizontal twin was always right: `max_line_width` is measured
        // from laid-out text, which is what made the asymmetry visible.)
        let lines = ed.last_vline().get() + 1;
        scrollbar_geo(
            lines,
            ed.line_height(0) as f64,
            ed.viewport.get_untracked().height(),
        )
    };
    // Drag state: hover (for the hover tint), whether a drag is in flight, and the
    // grab offset within the thumb captured on press.
    let v_hover = RwSignal::new(false);
    let v_drag = RwSignal::new(false);
    let v_grab = RwSignal::new(0.0_f64);
    let v_scrollbar = {
        let v = empty();
        let vid = v.id();
        v.style(move |s| {
            let _ = query.get(); // re-run on edits (content height isn't a signal)
            let _ = bar_gen.get(); // re-run once the first layout has settled
            let vp = ed_vbar.viewport.get();
            let Some((track_h, thumb_h, max_scroll)) = v_geo(&ed_vbar) else {
                return s.hide();
            };
            if !esbar_shown.get() && !v_drag.get() {
                return s.hide();
            }
            let ratio = (vp.y0 / max_scroll).clamp(0.0, 1.0);
            let top = ESBAR_TOP + ratio * (track_h - thumb_h);
            let hot = v_hover.get() || v_drag.get();
            s.absolute()
                .inset_top(top)
                .inset_right(3.0)
                .width(6.0)
                .height(thumb_h)
                .border_radius(3.0)
                .cursor(CursorStyle::Default)
                .background(if hot {
                    theme::scrollbar_hover()
                } else {
                    theme::scrollbar()
                })
        })
        .on_event(EventListener::PointerEnter, move |_| {
            v_hover.set(true);
            EventPropagation::Continue
        })
        .on_event(EventListener::PointerLeave, move |_| {
            v_hover.set(false);
            EventPropagation::Continue
        })
        .on_event(EventListener::PointerDown, move |e| {
            if let Event::PointerDown(pe) = e
                && pe.button.is_primary()
            {
                v_grab.set(pe.pos.y); // offset within the thumb where grabbed
                v_drag.set(true);
                vid.request_active(); // capture moves even off the thumb
                return EventPropagation::Stop;
            }
            EventPropagation::Continue
        })
        .on_event(EventListener::PointerMove, move |e| {
            if v_drag.get_untracked()
                && let Event::PointerMove(pe) = e
            {
                if let Some((track_h, thumb_h, max_scroll)) = v_geo(&ed_vdrag) {
                    let vp = ed_vdrag.viewport.get_untracked();
                    let cur_rel = vp.y0 / max_scroll * (track_h - thumb_h);
                    // `pe.pos.y` is relative to the (moving) thumb origin, so the
                    // delta from the grab offset is how far to shift the thumb.
                    let new_rel =
                        (cur_rel + pe.pos.y - v_grab.get_untracked()).clamp(0.0, track_h - thumb_h);
                    let y = new_rel / (track_h - thumb_h) * max_scroll;
                    ed_vdrag
                        .scroll_to
                        .set(Some(floem::kurbo::Vec2::new(vp.x0, y)));
                }
                return EventPropagation::Stop;
            }
            EventPropagation::Continue
        })
        .on_event(EventListener::PointerUp, move |_| {
            if v_drag.get_untracked() {
                v_drag.set(false);
                vid.clear_active();
            }
            EventPropagation::Continue
        })
    };
    // Horizontal bar geometry: `(avail, thumb_w, max_scroll)`, mirroring `v_geo`.
    // `avail` is the usable track width (short of the vertical bar); needs `area_w`.
    let h_geo = move |ed: &floem::views::editor::Editor| -> Option<(f64, f64, f64)> {
        let vp = ed.viewport.get_untracked();
        let vw = vp.width();
        let content_w = ed.max_line_width();
        if content_w <= vw + 1.0 || vw <= 0.0 {
            return None;
        }
        let avail = (area_w.get_untracked() - 6.0 - 12.0).max(1.0);
        let thumb_w = thumb_len(vw / content_w * avail, avail);
        Some((avail, thumb_w, (content_w - vw).max(1.0)))
    };
    let h_hover = RwSignal::new(false);
    let h_drag = RwSignal::new(false);
    let h_grab = RwSignal::new(0.0_f64);
    let h_scrollbar = {
        let h = empty();
        let hid = h.id();
        h.style(move |s| {
            let _ = query.get(); // re-run on edits (content width isn't a signal)
            let _ = area_w.get(); // re-run on pane resize
            let _ = bar_gen.get(); // re-run once the first layout has settled
            let vp = ed_hbar.viewport.get();
            let Some((avail, thumb_w, max_scroll)) = h_geo(&ed_hbar) else {
                return s.hide();
            };
            if !esbar_shown.get() && !h_drag.get() {
                return s.hide();
            }
            // Track spans from 6px in from the left border to a gap short of the
            // vertical bar (which occupies the rightmost ~9px).
            let ratio = (vp.x0 / max_scroll).clamp(0.0, 1.0);
            let left = 6.0 + ratio * (avail - thumb_w);
            let hot = h_hover.get() || h_drag.get();
            s.absolute()
                .inset_left(left)
                .inset_bottom(3.0)
                .height(6.0)
                .width(thumb_w)
                .border_radius(3.0)
                .cursor(CursorStyle::Default)
                .background(if hot {
                    theme::scrollbar_hover()
                } else {
                    theme::scrollbar()
                })
        })
        .on_event(EventListener::PointerEnter, move |_| {
            h_hover.set(true);
            EventPropagation::Continue
        })
        .on_event(EventListener::PointerLeave, move |_| {
            h_hover.set(false);
            EventPropagation::Continue
        })
        .on_event(EventListener::PointerDown, move |e| {
            if let Event::PointerDown(pe) = e
                && pe.button.is_primary()
            {
                h_grab.set(pe.pos.x);
                h_drag.set(true);
                hid.request_active();
                return EventPropagation::Stop;
            }
            EventPropagation::Continue
        })
        .on_event(EventListener::PointerMove, move |e| {
            if h_drag.get_untracked()
                && let Event::PointerMove(pe) = e
            {
                if let Some((avail, thumb_w, max_scroll)) = h_geo(&ed_hdrag) {
                    let vp = ed_hdrag.viewport.get_untracked();
                    let cur_rel = vp.x0 / max_scroll * (avail - thumb_w);
                    let new_rel =
                        (cur_rel + pe.pos.x - h_grab.get_untracked()).clamp(0.0, avail - thumb_w);
                    let x = new_rel / (avail - thumb_w) * max_scroll;
                    ed_hdrag
                        .scroll_to
                        .set(Some(floem::kurbo::Vec2::new(x, vp.y0)));
                }
                return EventPropagation::Stop;
            }
            EventPropagation::Continue
        })
        .on_event(EventListener::PointerUp, move |_| {
            if h_drag.get_untracked() {
                h_drag.set(false);
                hid.clear_active();
            }
            EventPropagation::Continue
        })
    };

    // Editor + floating popups (autocomplete, Ctrl+K AI) share one relatively-
    // positioned box so the (absolute) popups anchor to the editor's coordinates.
    let cmdk_view = cmdk_popup(
        cmdk,
        inline_ai,
        inline_ai_run.clone(),
        inline_ai_cancel,
        query,
        ed_cmdk,
        comp,
        area_h,
    );
    // Error bar: pinned to the editor's bottom (5px inset) when the last run
    // failed. Truncated error + View (opens the modal) + Explain + AI fix.
    // Cleared by any edit (see the editor `.update`). border_radius rounds the
    // filled bar; no `.clip()` (clipping the absolute container would hide it).
    //
    // Its measured width, so the bar can drop **Explain** rather than let the
    // buttons collide. Lives out here because the bar's content is rebuilt on
    // every `results` change and the width outlives that.
    let error_bar_w = RwSignal::new(0.0_f64);
    let error_bar = {
        let ai_fix = ai_fix.clone();
        dyn_container(
            move || results.get(),
            move |state| {
                let msg = match state {
                    QueryState::Failed(m) => m,
                    _ => return empty().into_any(),
                };
                // Collapse to a single line: a multi-line error otherwise makes the
                // text taller than the bar and spills out the top (`text_ellipsis`
                // only trims one line). The full text is still in the View modal.
                let one_line = msg.split_whitespace().collect::<Vec<_>>().join(" ");
                let ai_fix = ai_fix.clone();
                let explain_error = explain_error.clone();
                h_stack((
                    // **`min_width(0)`, and it is the whole reason the buttons
                    // stopped overlapping.** A flex item defaults to
                    // `min-width: auto`, which refuses to shrink below its
                    // content — so a long error did not ellipsize any further
                    // than 60%, it pushed, and the two right-hand buttons were
                    // laid out on top of each other. The cap says how much of the
                    // bar the message may *take*; this says it must yield the
                    // rest.
                    text(one_line).style(|s| {
                        s.color(theme::reject_text())
                            .font_size(theme::font_body())
                            .max_width_pct(ERROR_BAR_MSG_PCT * 100.0)
                            .min_width(0.0)
                            .text_ellipsis()
                    }),
                    // Every button here brightens under the pointer: they are
                    // words on a coloured bar with no border to read as a
                    // control, so the hover *is* the affordance. No pointer
                    // cursor — see *UI conventions*.
                    //
                    // The two AI actions sit together at the far edge, which is
                    // what the sparkle marks: *View* opens a window, those two
                    // reach a model. `flex_shrink(0)` because a button that
                    // shrinks is never what the layout wants — the message is the
                    // part that yields.
                    text("View")
                        .on_click_stop(move |_| error_modal_open.set(true))
                        .style(|s| {
                            s.color(theme::err_fix_btn())
                                .font_size(theme::font_body())
                                .margin_left(theme::scaled(10.0))
                                .flex_shrink(0.0_f32)
                                .hover(|s| s.color(theme::err_fix_btn_hover()))
                        }),
                    empty().style(|s| s.flex_grow(1.0_f32)),
                    // Dropped rather than crowded when the bar is too narrow to
                    // hold all three with the message still readable. It is the
                    // one of the three that has a second home — the *View* modal
                    // offers the same explanation — so it is the one that can go.
                    dyn_container(move || error_bar_fits_explain(error_bar_w.get()), {
                        let explain_error = explain_error.clone();
                        move |fits: bool| {
                            if !fits {
                                return empty().into_any();
                            }
                            let explain_error = explain_error.clone();
                            crate::widgets::sparkle_action(
                                "Explain",
                                theme::err_fix_btn,
                                theme::err_fix_btn_hover,
                                move || (explain_error)(),
                            )
                            // The gap to *AI fix* belongs to the button, not
                            // to the slot: on the container it would leave
                            // 20px of nothing behind when Explain is dropped.
                            .style(|s| s.margin_right(theme::scaled(20.0)))
                            .into_any()
                        }
                    }),
                    crate::widgets::sparkle_action(
                        "AI fix",
                        theme::err_fix_btn,
                        theme::err_fix_btn_hover,
                        move || (ai_fix)(),
                    ),
                ))
                .on_resize(move |r| {
                    if (error_bar_w.get_untracked() - r.width()).abs() > 0.5 {
                        error_bar_w.set(r.width());
                    }
                })
                .style(|s| {
                    // **The bar owns both horizontal insets**, as padding, rather
                    // than the message and the last button each carrying a margin
                    // of their own. Written as two margins they were the same
                    // number and still did not read as one: the right-hand button
                    // is a row (icon, gap, label) whose box ends past the last
                    // glyph, so the same 8px looked wider after the words than
                    // before them. One padding makes the two edges the same by
                    // construction, and leaves no second place to change.
                    s.flex_row()
                        .items_center()
                        .width_full()
                        .height_full()
                        .padding_horiz(theme::scaled(ERROR_BAR_PAD))
                        .background(theme::reject_bg())
                        .border_radius(5.0)
                })
                .into_any()
            },
        )
        .style(move |s| {
            if matches!(results.get(), QueryState::Failed(_)) {
                s.absolute()
                    .inset_left(float_inset())
                    .inset_right(float_inset())
                    .inset_bottom(float_inset())
                    .height(theme::scaled(35.0))
            } else {
                s
            }
        })
    };

    // Unsafe-run guard bar: same look/position as the error bar (red, pinned to
    // the editor bottom), but pre-run — a warning + a "Run anyway" text button
    // (where AI Fix sits). Dismissed by any editor click or keypress (handled in
    // the key handler / PointerDown above); "Run anyway" replays the held run.
    let guard_bar = {
        let run_anyway = run_anyway.clone();
        dyn_container(
            move || guard.get(),
            move |g| {
                let Some(g) = g else {
                    return empty().into_any();
                };
                let run_anyway = run_anyway.clone();
                // A soft guard (unsafe WHERE) offers "Run anyway"; a hard block
                // (read-only, `pending: None`) shows only the message.
                let action: floem::AnyView = if g.pending.is_some() {
                    text("Run anyway")
                        .on_click_stop(move |_| (run_anyway)())
                        .style(|s| {
                            s.color(theme::err_fix_btn())
                                .font_size(theme::font_body())
                                .margin_right(theme::scaled(8.0))
                        })
                        .into_any()
                } else {
                    empty().into_any()
                };
                h_stack((
                    text(g.message).style(|s| {
                        s.color(theme::reject_text())
                            .font_size(theme::font_body())
                            .max_width_pct(70.0)
                            .text_ellipsis()
                            .margin_left(theme::scaled(8.0))
                    }),
                    empty().style(|s| s.flex_grow(1.0_f32)),
                    action,
                ))
                .style(|s| {
                    s.flex_row()
                        .items_center()
                        .width_full()
                        .height_full()
                        .background(theme::reject_bg())
                        .border_radius(5.0)
                })
                .into_any()
            },
        )
        .style(move |s| {
            if guard.get().is_some() {
                s.absolute()
                    .inset_left(float_inset())
                    .inset_right(float_inset())
                    .inset_bottom(float_inset())
                    .height(theme::scaled(35.0))
            } else {
                s
            }
        })
    };

    // Ctrl+Enter run menu (multi-statement editor): Run Current runs the
    // statement under the caret; Run Everything runs all statements as a batch.
    // It's keyboard-driven (opened by a shortcut): ↑/↓ move the selection (Run
    // Current is selected by default), Enter runs it, Escape dismisses. The mouse
    // still works — hovering moves the selection so both share one highlight.
    let run_menu_view = {
        let run = run.clone();
        let run_all = run_all.clone();
        let ed_rm = ed_run;
        dyn_container(
            move || run_menu.get(),
            move |pos| {
                let Some(pos) = pos else {
                    return empty().into_any();
                };
                // Opening the menu stole focus (it's keyboard-navigable); return
                // it to the editor after running so the caret stays put. Deferred
                // a frame — same as the autofocus path — so the editor view exists.
                let refocus: Rc<dyn Fn()> = {
                    let ed = ed_rm.clone();
                    Rc::new(move || {
                        let ed = ed.clone();
                        floem::action::exec_after(std::time::Duration::ZERO, move |_| {
                            if let Some(Some(vid)) = ed.editor_view_id.try_get_untracked() {
                                vid.request_focus();
                            }
                        });
                    })
                };
                // Two items; ↑/↓ wrap. Keep in sync with the rows below.
                const RUN_MENU_N: usize = 2;
                let row = |idx: usize, label: &str, action: Rc<dyn Fn()>| {
                    let label = label.to_string();
                    container(text(label).style(|s| s.color(theme::text())))
                        .on_click_stop(move |_| (action)())
                        // Hovering moves the keyboard selection, so mouse and
                        // keyboard drive a single highlight.
                        .on_event(EventListener::PointerMove, move |_| {
                            if run_sel.get_untracked() != idx {
                                run_sel.set(idx);
                            }
                            EventPropagation::Continue
                        })
                        .style(menu_item_style)
                        .style(move |s| {
                            let s = s.padding_vert(theme::scaled(8.0));
                            if run_sel.get() == idx {
                                s.background(theme::accent().multiply_alpha(0.15))
                            } else {
                                s
                            }
                        })
                };
                let run_current: Rc<dyn Fn()> = {
                    let run = run.clone();
                    let refocus = refocus.clone();
                    Rc::new(move || {
                        let sql = query.get_untracked();
                        let (lo, hi) = statement_range(
                            &sql,
                            run_menu_offset.get_untracked(),
                            dialect.get_untracked(),
                        );
                        if let Some(stmt) = sql.get(lo..hi).filter(|s| !s.trim().is_empty()) {
                            (run)(stmt.to_string());
                        }
                        // Down with the menu, as on Escape and Run Everything. The
                        // outline is the menu's selection — what is *about* to run —
                        // so once it has run it has nothing left to say, and there
                        // is no gesture that dismisses it: Escape and arrow keys
                        // don't reach it, leaving it up until the next click or
                        // keystroke in the editor.
                        highlight.set(None);
                        run_menu.set(None);
                        (refocus)();
                    })
                };
                let run_everything: Rc<dyn Fn()> = {
                    let run_all = run_all.clone();
                    let refocus = refocus.clone();
                    Rc::new(move || {
                        let sql = query.get_untracked();
                        let stmts: Vec<String> = statement_ranges(&sql, dialect.get_untracked())
                            .into_iter()
                            .filter_map(|(lo, hi)| sql.get(lo..hi).map(|s| s.to_string()))
                            .collect();
                        (run_all)(stmts);
                        highlight.set(None);
                        run_menu.set(None);
                        (refocus)();
                    })
                };
                // Enter runs whichever row is selected.
                let activate: Rc<dyn Fn()> = {
                    let rc = run_current.clone();
                    let re = run_everything.clone();
                    Rc::new(move || {
                        if run_sel.get_untracked() == 0 {
                            (rc)()
                        } else {
                            (re)()
                        }
                    })
                };
                let panel = focus_root(v_stack((
                    row(0, "Run Current", run_current),
                    row(1, "Run Everything", run_everything),
                )))
                .on_key_down(
                    Key::Named(NamedKey::ArrowDown),
                    |_| true,
                    move |_| run_sel.update(|i| *i = (*i + 1) % RUN_MENU_N),
                )
                .on_key_down(
                    Key::Named(NamedKey::ArrowUp),
                    |_| true,
                    move |_| run_sel.update(|i| *i = (*i + RUN_MENU_N - 1) % RUN_MENU_N),
                )
                .on_key_down(Key::Named(NamedKey::Enter), |_| true, move |_| (activate)())
                // Dismissing has to hand focus back, exactly as running does.
                // The panel took focus so ↑/↓/Enter drive it, and floem clears
                // `app_state.focus` on no path when a focused view is *removed* —
                // so Escape left the keyboard pointing at a destroyed view and
                // typing went nowhere until the user clicked into the editor.
                .on_key_down(Key::Named(NamedKey::Escape), |_| true, {
                    let refocus = refocus.clone();
                    move |_| {
                        run_menu.set(None);
                        // The highlight came up with the menu (it is what "Run
                        // Current" is pointing at), so it goes down with it —
                        // dismissing ran nothing, and an outline left standing
                        // over a statement reads as one about to run. It used to
                        // linger until the next keystroke.
                        highlight.set(None);
                        (refocus)();
                    }
                })
                .on_event_stop(EventListener::PointerDown, |_| {})
                .style(|s| {
                    panel_style(s)
                        .background(theme::bg_chrome())
                        .min_width(run_menu_w())
                        .padding_vert(theme::scaled(6.0))
                        .font_size(theme::font_title())
                });
                let positioned = container(panel).style(move |s| {
                    // `pos` is a *content* anchor, so the viewport comes off here —
                    // a reactive read, which is also what keeps the menu on the
                    // caret while the editor scrolls under it. `run_menu_pos` then
                    // flips it left of the caret at the code column's right edge and
                    // clamps it into the pane. It used to compare the unscrolled
                    // anchor against `area_w` and got cut off at that edge.
                    let p = run_menu_pos(
                        pos,
                        (run_menu_w(), run_menu_h()),
                        content_x_of(&query.get_untracked()),
                        ed_vp.get(),
                    );
                    s.absolute().inset_left(p.x).inset_top(p.y)
                });
                // Same for a click outside: the catcher stops propagation, so the
                // click that dismissed the menu never reaches the editor either.
                let catcher = empty()
                    .on_event_stop(EventListener::PointerDown, move |_| {
                        run_menu.set(None);
                        // Same as Escape — and needed here for the same reason the
                        // catcher exists: it stops the event, so the editor's own
                        // pointer-down (which clears the highlight) never runs.
                        highlight.set(None);
                        (refocus)();
                    })
                    .style(|s| s.absolute().inset(0.0));
                stack((catcher, positioned))
                    .style(|s| s.absolute().inset(0.0))
                    .into_any()
            },
        )
        .style(move |s| {
            if run_menu.get().is_some() {
                s.absolute().inset(0.0)
            } else {
                s
            }
        })
    };

    // DataGrip-style border around the statement picked by Explain/Optimize.
    // Click-through (`pointer_events(false)`) so clicks reach the editor (which
    // clears the highlight); a thin absolute box per line the statement touches.
    // The two strips finishing the diff's bands — over the gutter and over the
    // wrapper's right padding. Click-through, because they lie across the document
    // and Floem's wheel routing gives no way to pass an event on once a view is
    // eligible for it (see the pointer-routing note on `cmdk_popup`'s verdict bar,
    // which solves the same problem the other way because it needs clicks).
    let inline_band_view = {
        let ed = ed_band;
        dyn_container(
            move || inline_band_runs(&ed, cmdk.preview),
            move |runs| {
                if runs.is_empty() {
                    return empty().into_any();
                }
                // The gutter's width follows the line-number digit count, exactly
                // as the code column's origin does; the code column ends where the
                // scroll viewport does, and everything past that is the wrapper's
                // right padding.
                let content_x = content_x_of(&query.get_untracked());
                let content_right = content_x + ed_vp.get().width();
                v_stack_from_iter(runs.into_iter().flat_map(move |(y, h, is_add)| {
                    let bg = move || {
                        if is_add {
                            theme::diff_add_bg()
                        } else {
                            theme::diff_del_bg()
                        }
                    };
                    // Left: the gutter. Right: pinned between the content's right
                    // edge and the editor's, so it stretches without needing the
                    // area's width — and, crucially, starts *past* the last glyph
                    // column. This overlay paints over the editor, so a strip that
                    // reached back into the code would cover the code.
                    // The gutter, carrying this row's marker in place of the line
                    // number the band covers — which is what the design puts there.
                    let marker = if is_add { "+" } else { "−" };
                    let fg = move || {
                        if is_add {
                            theme::diff_add_marker()
                        } else {
                            theme::diff_del_marker()
                        }
                    };
                    let left = container(text(marker).style(move |s| {
                        s.color(fg())
                            .font_family(MONO_FAMILY.to_string())
                            .font_size(theme::editor_font_size())
                    }))
                    .style(move |s| {
                        s.absolute()
                            .inset_left(0.0)
                            .inset_top(y)
                            // Stop `HL_PAD` short of the code column. `HL_GUTTER`
                            // is a measured estimate that runs a shade generous —
                            // fine for the statement-highlight *border* it was
                            // tuned for, which pads outward by exactly this much to
                            // clear the glyphs, but a filled band inherits no such
                            // margin and was painting over the first character.
                            .width((content_x - HL_PAD).max(0.0))
                            .height(h)
                            .flex_row()
                            .items_center()
                            .justify_end()
                            // Line the marker up with the digits it replaces.
                            .padding_right(theme::scaled(HL_DIGIT_W))
                            .background(bg())
                    });
                    let right = empty().style(move |s| {
                        s.absolute()
                            .inset_left(content_right)
                            .inset_right(0.0)
                            .inset_top(y)
                            .height(h)
                            .background(bg())
                    });
                    [left.into_any(), right.into_any()]
                }))
                .style(|s| s.absolute().inset(0.0))
                .into_any()
            },
        )
        .style(|s| s.absolute().inset(0.0))
        // Confined to the editor: the strips are placed by line geometry, and a
        // block scrolled past the top of the pane would otherwise paint its bands
        // over the toolbar above it.
        .clip()
        .pointer_events(|| false)
    };

    let highlight_view = {
        let ed = ed_hl;
        dyn_container(
            move || highlight.get(),
            move |range| match range {
                None => empty().into_any(),
                Some((lo, hi)) => {
                    let sql = query.get_untracked();
                    let boxes = statement_line_boxes(&sql, &ed, lo, hi);
                    v_stack_from_iter(boxes.into_iter().map(|(x, y, w, h)| {
                        empty().style(move |s| {
                            s.absolute()
                                .inset_left(x)
                                .inset_top(y)
                                .width(w)
                                // +1 so adjacent lines' borders overlap into one
                                // 1px line (no doubled middle border).
                                .height(h + 1.0)
                                .border(1.0)
                                .border_radius(3.0)
                                .border_color(theme::query_highlight())
                        })
                    }))
                    .style(|s| s.absolute().inset(0.0))
                    .into_any()
                }
            },
        )
        .style(|s| s.absolute().inset(0.0))
        .pointer_events(|| false)
    };

    // Bracket matching: two faint boxes around the paren adjacent to the caret and
    // its partner. Click-through like the other overlays; each box reads the editor
    // `viewport` (inside `char_box`) so it tracks scroll.
    let bracket_match_view = {
        let ed = ed_bm2;
        dyn_container(
            move || bracket_match.get(),
            move |m| match m {
                None => empty().into_any(),
                Some((p, q)) => {
                    let sql = query.get_untracked();
                    let (edp, edq) = (ed.clone(), ed.clone());
                    let (sqp, sqq) = (sql.clone(), sql);
                    v_stack((
                        empty().style(move |s| match span_box(&sqp, &edp, p, p + 1) {
                            // Scrolled out of view: draw nothing rather than a
                            // box at the editor's origin.
                            None => s.hide(),
                            Some((x, y, w, h)) => s
                                .absolute()
                                .inset_left(x)
                                .inset_top(y)
                                .width(w)
                                .height(h)
                                .border(1.0)
                                .border_radius(2.0)
                                .border_color(theme::bracket_match().multiply_alpha(0.5)),
                        }),
                        empty().style(move |s| match span_box(&sqq, &edq, q, q + 1) {
                            None => s.hide(),
                            Some((x, y, w, h)) => s
                                .absolute()
                                .inset_left(x)
                                .inset_top(y)
                                .width(w)
                                .height(h)
                                .border(1.0)
                                .border_radius(2.0)
                                .border_color(theme::bracket_match().multiply_alpha(0.5)),
                        }),
                    ))
                    .style(|s| s.absolute().inset(0.0))
                    .into_any()
                }
            },
        )
        .style(|s| s.absolute().inset(0.0))
        .pointer_events(|| false)
    };

    // Highlight all occurrences of the identifier under the caret: one faint box
    // per occurrence, sharing the bracket-match colour + opacity. Click-through;
    // each box reads the editor `viewport` (via `span_box`) so it tracks scroll.
    let occurrences_view = {
        let ed = ed_occ2;
        dyn_container(
            move || ident_occurrences.get(),
            move |ranges| {
                if ranges.is_empty() {
                    return empty().into_any();
                }
                let sql = query.get_untracked();
                let ed = ed.clone();
                v_stack_from_iter(ranges.into_iter().map(move |(lo, hi)| {
                    let ed = ed.clone();
                    let sql = sql.clone();
                    empty().style(move |s| match span_box(&sql, &ed, lo, hi) {
                        // An occurrence scrolled out of view draws nothing —
                        // this is what produced stray boxes over unrelated text.
                        None => s.hide(),
                        Some((x, y, w, h)) => s
                            .absolute()
                            .inset_left(x)
                            .inset_top(y)
                            .width(w)
                            .height(h)
                            .border(1.0)
                            .border_radius(2.0)
                            .border_color(theme::bracket_match().multiply_alpha(0.5)),
                    })
                }))
                .style(|s| s.absolute().inset(0.0))
                .into_any()
            },
        )
        .style(|s| s.absolute().inset(0.0))
        .pointer_events(|| false)
    };

    // Wavy underlines under diagnostics: red for definite errors (unknown table/
    // column, syntax), amber for probable keyword typos. Overlay laid over the
    // editor; each squiggle carries a hover tooltip with the diagnostic message.
    // The container is click-through (`pointer_events(false)`) so text selection is
    // unaffected — only the individual squiggle strips re-enable pointer events so
    // hovering the underline (drawn in the descender gap, below the glyphs) reveals
    // the message without stealing clicks meant for the text.
    let syntax_view = {
        let ed = ed_syntax;
        // The squiggle's *width* is baked into the SVG markup (floem's `svg()`
        // takes a `String`, not a signal), so unlike the other overlays this one
        // can't just recompute inside a `.style()` closure — the view itself has
        // to be rebuilt when the geometry moves. A memo is what makes that
        // affordable: it tracks `viewport`/`screen_lines` and so re-runs on every
        // scroll, but memos dedup on `PartialEq`, so the container below only
        // rebuilds when a squiggle actually changes position, width, severity or
        // message. Same trick the grid uses for its column window.
        let segs = create_memo(move |_| {
            let mut diags = syntax.get();
            diags.extend(db_diag.get());
            let sql = query.get();
            let vp = ed.viewport.get();
            let points = editor_points(&ed);
            diags
                .iter()
                .filter_map(|d| {
                    // `None` = off screen. Rendering nothing is the point: this
                    // used to collapse to a 2px stub at the editor's top-left
                    // carrying the tooltip of an error twenty lines away.
                    underline_seg_at(&points, &sql, d.range.0, d.range.1, (vp.x0, vp.y0))
                        .map(|(x, y, w)| (x, y, w, d.severity, d.message.clone()))
                })
                .collect::<Vec<_>>()
        });
        dyn_container(
            move || segs.get(),
            move |segs: Vec<(f64, f64, f64, Severity, String)>| {
                if segs.is_empty() {
                    return empty().into_any();
                }
                v_stack_from_iter(segs.into_iter().map(|(x, y, w, sev, msg)| {
                    floem::views::svg(wavy_svg(w))
                        .style(move |s| {
                            s.absolute()
                                .inset_left(x)
                                .inset_top(y)
                                // A slightly taller hit area than the wave so the
                                // hover is catchable, still within the descender gap.
                                .height(WAVE_H + 4.0)
                                .width(w)
                                .color(match sev {
                                    Severity::Error => theme::diag_error(),
                                    Severity::Warning => theme::syntax_underline(),
                                })
                        })
                        .pointer_events(|| true)
                        .tooltip(move || {
                            text(msg.clone())
                                .style(|s| s.font_size(theme::scaled_font(12.0)).max_width(360.0))
                        })
                }))
                .style(|s| s.absolute().inset(0.0))
                .into_any()
            },
        )
        .style(|s| s.absolute().inset(0.0))
        .pointer_events(|| false)
    };

    // Run button: a Lucide play overlay pinned to the editor's bottom-right
    // (7px insets). The whole pill is clickable (not just the glyph); hover
    // brightens the play. Runs the current query — Ctrl+Enter still works too.
    let run_overlay = {
        let run = run.clone();
        let hovered = RwSignal::new(false);
        container(icons::icon(icons::PLAY_LUCIDE, 16.0).style(move |s| {
            // Dimmed to 30% (background stays) when there's nothing to run, or
            // while the connection is known-dead. It stays *clickable* when
            // disconnected on purpose: the click re-checks the connection and
            // runs if the server is back, which is the only recovery path
            // besides the header's Retry.
            let empty = query.with(|q| q.trim().is_empty());
            let down = conn_status.get().is_down();
            let base = if !empty && !down && hovered.get() {
                theme::grid_edit_staged_hover()
            } else {
                theme::approve_bg()
            };
            let color = if empty || down {
                base.multiply_alpha(0.3)
            } else {
                base
            };
            s.color(color)
        }))
        .on_click_stop(move |_| {
            // No-op while the query is empty.
            if query.with_untracked(|q| q.trim().is_empty()) {
                return;
            }
            (run)(query.get_untracked())
        })
        .on_event(EventListener::PointerEnter, move |_| {
            hovered.set(true);
            EventPropagation::Continue
        })
        .on_event(EventListener::PointerLeave, move |_| {
            hovered.set(false);
            EventPropagation::Continue
        })
        .style(move |s| {
            let s = s
                .absolute()
                .inset_right(float_inset())
                .inset_bottom(float_inset())
                .items_center()
                .justify_center()
                .padding_left(theme::scaled(10.0))
                .padding_right(theme::scaled(8.0))
                .padding_vert(theme::scaled(8.0))
                .background(theme::bg_chrome())
                .border_radius(5.0);
            // The overlay is anchored to the editor's bottom edge; when the editor is
            // collapsed to height 0 it would otherwise paint a sliver over the panel
            // separator, so drop it from layout entirely.
            if editor_collapsed.get() { s.hide() } else { s }
        })
    };

    // ── In-editor find bar (Ctrl+F) ──────────────────────────────────────────
    // Select + centre the match at byte `off` of length `len`.
    let reveal: Rc<dyn Fn(usize, usize)> = {
        let ed = ed_find.clone();
        Rc::new(move |off: usize, len: usize| {
            ed.cursor
                .update(|c| c.set_insert(Selection::region(off, off + len)));
            ed.center_window();
        })
    };
    // Recompute matches whenever the query changes while the bar is open, and jump
    // to the first. The haystack is read from `query` (kept in sync with the doc).
    {
        let reveal = reveal.clone();
        // The document is **tracked**: an edit while the bar is open moves every
        // later match, and the hit list is what Replace edits by. Reading it
        // untracked meant a hit computed before the edit was used after it —
        // typing `-- ` at the head of a query turned `SELECT a FROM t;` into
        // `-- SELECT a FRx t;`, destroying the `OM` of `FROM` while the `t;` the
        // user searched for was left alone. It also froze the `n/total` counter
        // on the document as it was when the bar opened.
        //
        // The effect's previous value is the query it last ran for, which is what
        // separates the two reasons it runs: a *new query* starts at match 1 and
        // reveals it, an *edit* keeps the user where they are and scrolls nothing.
        create_effect(move |prev: Option<String>| {
            if !find_open.get() {
                return prev.unwrap_or_default();
            }
            let q = find_query.get();
            let hits = if q.is_empty() {
                Vec::new()
            } else {
                find_matches(&query.get(), &q)
            };
            if prev.as_deref() != Some(q.as_str()) {
                find_idx.set(0);
                if let Some(&first) = hits.first() {
                    reveal(first, q.len());
                }
            } else if !hits.is_empty() {
                // Same query, edited document: hold the position, clamped.
                find_idx.update(|i| *i = (*i).min(hits.len() - 1));
            }
            find_hits.set(hits);
            q
        });
    }
    // Step to the next (+1) / previous (-1) match, wrapping.
    let go: Rc<dyn Fn(i64)> = {
        let reveal = reveal.clone();
        Rc::new(move |delta: i64| {
            let n = find_hits.with_untracked(|v| v.len());
            if n == 0 {
                return;
            }
            let next = (find_idx.get_untracked() as i64 + delta).rem_euclid(n as i64) as usize;
            find_idx.set(next);
            let off = find_hits.with_untracked(|v| v[next]);
            reveal(off, find_query.with_untracked(|q| q.len()));
        })
    };
    let find_close: Rc<dyn Fn()> = {
        let ed = ed_find.clone();
        Rc::new(move || {
            find_open.set(false);
            find_query.set(String::new());
            find_replace.set(String::new());
            // Return focus to the editor so typing resumes there.
            if let Some(Some(vid)) = ed.editor_view_id.try_get_untracked() {
                vid.request_focus();
            }
        })
    };
    // Close the Go-to-line popup and return focus to the editor.
    let goto_close: Rc<dyn Fn()> = {
        let ed = ed_goto.clone();
        Rc::new(move || {
            goto_open.set(false);
            goto_query.set(String::new());
            if let Some(Some(vid)) = ed.editor_view_id.try_get_untracked() {
                vid.request_focus();
            }
        })
    };
    // Enter in the popup: jump to the typed line (start of it) and centre it, or do
    // nothing if the input isn't a valid, in-range line number. Always closes.
    let goto_submit: Rc<dyn Fn()> = {
        let ed = ed_goto.clone();
        Rc::new(move || {
            let raw = goto_query.get_untracked();
            goto_open.set(false);
            goto_query.set(String::new());
            let off = raw
                .trim()
                .parse::<usize>()
                .ok()
                .and_then(|line| offset_of_line(&ed.doc().text().to_string(), line));
            if let Some(off) = off {
                ed.cursor.update(|c| c.set_offset(off, false, false));
                ed.center_window();
            }
            if let Some(Some(vid)) = ed.editor_view_id.try_get_untracked() {
                vid.request_focus();
            }
        })
    };
    // Replace the current match with the replacement text, then recompute matches
    // (the doc→`query` sync updates the text; we read the doc directly for an
    // up-to-date, synchronous result) and reveal the next occurrence.
    let replace_one: Rc<dyn Fn()> = {
        let ed = ed_find.clone();
        let reveal = reveal.clone();
        Rc::new(move || {
            let q = find_query.get_untracked();
            if q.is_empty() {
                return;
            }
            let hits = find_hits.get_untracked();
            if hits.is_empty() {
                return;
            }
            let idx = find_idx.get_untracked().min(hits.len() - 1);
            let repl = find_replace.get_untracked();
            // Belt and braces over the effect above: never edit a span that isn't
            // the needle any more. The document is the authority at this instant —
            // the hit list is a signal, and one stale offset here rewrites text
            // the user never searched for.
            let text_now = ed.doc().text().to_string();
            let off = if matches_at(&text_now, hits[idx], &q) {
                hits[idx]
            } else {
                let fresh = find_matches(&text_now, &q);
                let Some(&off) = fresh.get(idx).or(fresh.first()) else {
                    find_hits.set(Vec::new());
                    find_idx.set(0);
                    return;
                };
                off
            };
            edit_untyped(
                &ed,
                comp,
                Selection::region(off, off + q.len()),
                &repl,
                EditType::Other,
            );
            let text = ed.doc().text().to_string();
            let new_hits = find_matches(&text, &q);
            if new_hits.is_empty() {
                find_hits.set(new_hits);
                find_idx.set(0);
                return;
            }
            // Advance to the first match at/after the replacement end (wrapping),
            // so repeated Replace walks forward through the document.
            let after = off + repl.len();
            let next = new_hits.iter().position(|&h| h >= after).unwrap_or(0);
            find_idx.set(next);
            reveal(new_hits[next], q.len());
            find_hits.set(new_hits);
        })
    };
    // Replace every match in one edit, then recompute (a replacement that itself
    // contains the needle would surface fresh matches).
    let replace_all_cb: Rc<dyn Fn()> = {
        let ed = ed_find.clone();
        Rc::new(move || {
            let q = find_query.get_untracked();
            if q.is_empty() {
                return;
            }
            let repl = find_replace.get_untracked();
            let text = ed.doc().text().to_string();
            let (new_text, n) = replace_all(&text, &q, &repl);
            if n == 0 {
                return;
            }
            edit_untyped(
                &ed,
                comp,
                Selection::region(0, text.len()),
                &new_text,
                EditType::Other,
            );
            find_hits.set(find_matches(&new_text, &q));
            find_idx.set(0);
        })
    };
    let find_bar = {
        let (go_submit, go_prev, go_next, go_up, go_down) =
            (go.clone(), go.clone(), go.clone(), go.clone(), go.clone());
        let close = find_close.clone();
        let replace_one = replace_one.clone();
        let replace_all_cb = replace_all_cb.clone();
        dyn_container(
            move || find_open.get(),
            move |open| {
                if !open {
                    return empty().into_any();
                }
                let icon_btn = |markup: &'static str, sz: f32, on: Rc<dyn Fn()>| {
                    container(icons::icon(markup, sz))
                        .on_click_stop(move |_| (on)())
                        .style(|s| {
                            s.items_center()
                                .color(theme::text_dim())
                                .hover(|s| s.color(theme::text()))
                        })
                };

                // Expand/collapse the replace row: chevron points right when
                // collapsed, down when open. It sits in the outer row (`items_center`)
                // so it slides to stay vertically centred as the bar grows.
                let toggle = container(dyn_container(
                    move || find_replace_visible.get(),
                    move |vis| {
                        icons::icon(
                            if vis {
                                icons::CHEVRON_DOWN
                            } else {
                                icons::CHEVRON_RIGHT
                            },
                            14.0,
                        )
                        .into_any()
                    },
                ))
                .on_click_stop(move |_| find_replace_visible.update(|v| *v = !*v))
                .style(|s| {
                    s.items_center()
                        .margin_left(theme::scaled(2.0))
                        .color(theme::text_dim())
                        .hover(|s| s.color(theme::text()))
                });

                // ── Row 1: find ──
                let on_submit: Rc<dyn Fn()> = {
                    let g = go_submit.clone();
                    Rc::new(move || (g)(1))
                };
                let on_up: Rc<dyn Fn()> = {
                    let g = go_up.clone();
                    Rc::new(move || (g)(-1))
                };
                let on_down: Rc<dyn Fn()> = {
                    let g = go_down.clone();
                    Rc::new(move || (g)(1))
                };
                let esc = close.clone();
                let input = edit_field(
                    find_query,
                    FieldCfg {
                        placeholder: "Find",
                        autofocus: true,
                        font_size: theme::font_body,
                        border_radius: 6.0,
                        height: Some(field_input_h),
                        on_submit: Some(on_submit),
                        on_escape: Some(Rc::new(move || (esc)())),
                        on_arrow_up: Some(on_up),
                        on_arrow_down: Some(on_down),
                        ..Default::default()
                    },
                )
                .style(|s| s.width(theme::scaled(170.0)));
                let count = dyn_container(
                    // `with`, not `get` — reading a length shouldn't clone the hits.
                    move || (find_hits.with(|h| h.len()), find_idx.get()),
                    move |(n, i)| {
                        let cur = if n == 0 { 0 } else { i + 1 };
                        text(format!("{cur}/{n}"))
                            .style(|s| {
                                s.font_size(theme::font_label())
                                    .color(theme::text_dim())
                                    .min_width(30.0)
                            })
                            .into_any()
                    },
                );
                let prev_btn = icon_btn(icons::CHEVRON_UP, 15.0, {
                    let g = go_prev.clone();
                    Rc::new(move || (g)(-1))
                });
                let next_btn = icon_btn(icons::CHEVRON_DOWN, 15.0, {
                    let g = go_next.clone();
                    Rc::new(move || (g)(1))
                });
                let close_btn = icon_btn(icons::X, 14.0, close.clone());
                // A flex spacer pins the counter + nav + × to the right edge; row 2
                // does the same, so `All` lines up under the ×.
                let row1 = h_stack((
                    input,
                    empty().style(|s| s.flex_grow(1.0_f32)),
                    count,
                    prev_btn,
                    next_btn,
                    close_btn,
                ))
                .style(|s| s.items_center().gap(theme::scaled(8.0)));

                // ── Row 2: replace ──
                // Text buttons: colour-only hover (no background). Fixed 26px height
                // with centred text so they line up with the 26px field when the row
                // is top-aligned (`items_start`, which keeps the reveal from spilling
                // upward over the find row).
                let text_btn = |label: &'static str, on: Rc<dyn Fn()>| {
                    container(text(label).style(|s| s.font_size(theme::font_label())))
                        .on_click_stop(move |_| (on)())
                        .style(|s| {
                            s.items_center()
                                .height(theme::scaled(26.0))
                                .color(theme::text_dim())
                                .hover(|s| s.color(theme::text()))
                        })
                };
                let ro = replace_one.clone();
                let esc2 = close.clone();
                let rinput = edit_field(
                    find_replace,
                    FieldCfg {
                        placeholder: "Replace",
                        font_size: theme::font_body,
                        border_radius: 6.0,
                        height: Some(field_input_h),
                        on_submit: Some(ro),
                        on_escape: Some(esc2),
                        ..Default::default()
                    },
                )
                .style(|s| s.width(theme::scaled(170.0)));
                // The replace row is always mounted but shown/hidden via `display`
                // (no animation — an in-flow height transition through a clip was
                // janky, and the reveal isn't worth it here). Hidden ⇒ `display:none`.
                // Left-packed (gap 0, explicit margins): `Replace` is offset 16px past
                // the field so it lines up under the "n/total" counter in the find row
                // (input 170 + the row's 2×8px gaps), and `All` sits 15px after it.
                let replace_row = h_stack((
                    rinput,
                    text_btn("Replace", replace_one.clone())
                        .style(|s| s.margin_left(theme::scaled(16.0))),
                    text_btn("All", replace_all_cb.clone())
                        .style(|s| s.margin_left(theme::scaled(15.0))),
                ))
                .style(move |s| {
                    let s = s.items_center().padding_top(theme::scaled(6.0));
                    if find_replace_visible.get() {
                        s.flex()
                    } else {
                        s.hide()
                    }
                });

                // Fixed content width so BOTH rows have free space for their flex
                // spacer — otherwise the wider row drives the width and its spacer
                // collapses, leaving `All` hugging `Replace` instead of pinned under
                // the ×. Sized just to the find row's packed width so the leftover
                // spacer (hence the field→controls gap) is ~15px, not ~33px.
                let content = v_stack((row1, replace_row)).style(|s| s.width(theme::scaled(283.0)));
                h_stack((toggle, content))
                    .style(|s| {
                        s.items_center()
                            .gap(theme::scaled(8.0))
                            .padding_horiz(theme::scaled(8.0))
                            .padding_vert(theme::scaled(6.0))
                            .background(theme::bg_panel())
                            .border(1.0)
                            .border_color(theme::border())
                            .border_radius(8.0)
                    })
                    .into_any()
            },
        )
        .style(|s| {
            s.absolute()
                .inset_top(float_inset())
                .inset_right(float_inset())
        })
    };

    // Go-to-line popup: styled like the find bar (same panel + position), one row —
    // a "Go to:" label and a narrow (≈4-char) line-number field that autofocuses.
    let goto_bar = {
        let submit = goto_submit.clone();
        let close = goto_close.clone();
        dyn_container(
            move || goto_open.get(),
            move |open| {
                if !open {
                    return empty().into_any();
                }
                let esc = close.clone();
                let input = edit_field(
                    goto_query,
                    FieldCfg {
                        placeholder: "",
                        autofocus: true,
                        font_size: theme::font_body,
                        border_radius: 6.0,
                        height: Some(field_input_h),
                        on_submit: Some(submit.clone()),
                        on_escape: Some(Rc::new(move || (esc)())),
                        ..Default::default()
                    },
                )
                // Wide enough for a six-figure line number and the caret. 52px
                // fit four digits, so a line number in a generated script — the
                // case the popup exists for — scrolled inside its own field.
                .style(|s| s.width(theme::scaled(78.0)));
                // Close ✕ — same glyph size, styling, and row gap as the
                // find/replace bar's × (`icon_btn` there is a local closure).
                let close_x = close.clone();
                let close_btn = container(icons::icon(icons::X, 14.0))
                    .on_click_stop(move |_| (close_x)())
                    .style(|s| {
                        s.items_center()
                            .color(theme::text_dim())
                            .hover(|s| s.color(theme::text()))
                    });
                h_stack((
                    text("Go to:")
                        .style(|s| s.font_size(theme::font_label()).color(theme::text_dim())),
                    input,
                    close_btn,
                ))
                .style(|s| {
                    s.items_center()
                        .gap(theme::scaled(8.0))
                        .padding_horiz(theme::scaled(8.0))
                        .padding_vert(theme::scaled(6.0))
                        .background(theme::bg_panel())
                        .border(1.0)
                        .border_color(theme::border())
                        .border_radius(8.0)
                })
                .into_any()
            },
        )
        .style(|s| {
            s.absolute()
                .inset_top(float_inset())
                .inset_right(float_inset())
        })
    };

    // Order: editor + inline-diff band strips, syntax squiggles, statement
    // highlight, run overlay, then the completion popup / error+guard bars /
    // Ctrl+K, the scrollbars, and the run menu / find bar / goto bar on top.
    // (The right-click AI menu is rendered at the workspace root via `popup_menu`,
    // so it floats over the results pane instead of being clipped here.)
    //
    // The scrollbars sit that late deliberately — see the note at their entry.
    let editor_area = stack((
        // The band strips sit directly over the editor and under every other
        // overlay: they paint only the gutter and right-padding ends of the
        // inline-diff bands, which carry no text, so they must not cover the
        // overlays that do. Nested with the editor rather than listed beside it
        // because `stack` takes at most 16 children — the pair occupies the same
        // rect either way, so the absolute overlays below still share its coords.
        stack((editor_box, inline_band_view))
            .style(|s| s.flex_grow(1.0_f32).width_full().flex_col().min_height(0.0)),
        syntax_view,
        highlight_view,
        bracket_match_view,
        occurrences_view,
        run_overlay,
        completion_popup(comp, area_h, area_w, ed_vp),
        signature_popup(comp, ed_vp),
        error_bar,
        guard_bar,
        cmdk_view,
        // Above the Ctrl+K bars, not below them. The bars run edge to edge — they
        // belong to the block of lines they close, and a row that stopped short of
        // the border would read as a floating panel — so at this layer they covered
        // the scrollbars, which are pinned to the border rather than to the content
        // edge. The scrollbars are thin and sit at the extremes, so they cost the
        // bars nothing by being drawn last, and being on top also puts a drag on
        // the bar where the user aimed it.
        //
        // **Listed one by one, never wrapped in a stack to save a child slot.**
        // Each scrollbar positions itself against the border and is only as big as
        // it looks; a wrapper around them is `absolute().inset(0)` — a view the
        // size of the whole pane, sitting above the editor, that takes pointer
        // events. Floem stops a pointer event at the first eligible view under it,
        // so that wrapper silently ate every click, drag and wheel in the editor.
        v_scrollbar,
        h_scrollbar,
        run_menu_view,
        find_bar,
        goto_bar,
    ))
    .style(|s| {
        s.flex_grow(1.0_f32)
            .width_full()
            .flex_col()
            .min_height(0.0)
            .min_width(0.0)
    })
    // Track editor_area's height so the Ctrl+K expand animation fills it exactly
    // (it's the positioned ancestor of the cmdk overlay), and its width so the
    // right-click / run menus can flip leftward at the pane edge.
    .on_resize(move |rect| {
        area_h.set(rect.height());
        area_w.set(rect.width());
    });
    // The pane no longer pads its contents (so the title can sit flush at the
    // pane edge, matching SCHEMA); the editor's inset moves to this wrapper.
    // Padding the wrapper — not `editor_area` itself — keeps editor_area's
    // internal origin unchanged, so the completion popup stays aligned to the
    // caret (its anchor constants are relative to editor_area's origin).
    let editor_wrap = container(editor_area).style(|s| {
        s.flex_grow(1.0_f32)
            .width_full()
            .flex_col()
            .min_height(0.0)
            .min_width(0.0)
            // 3px top: nudges the editor up and grows it taller (the bottom inset
            // is unchanged), tightening the gap under the QUERY toolbar.
            .padding_top(theme::scaled(3.0))
            .padding_horiz(theme::scaled(13.0))
            .padding_bottom(theme::scaled(13.0))
    });

    // Active-database selector: a borderless menu trigger (like the header's
    // connection switcher, minus the border) — the active tab's database + a
    // chevron, in the chat-bubble text colour. Clicking toggles the DB menu
    // (positioned right-aligned under this trigger via the captured geometry).
    let trig_origin = RwSignal::new(Point::ZERO);
    let trig_size = RwSignal::new((0.0_f64, 0.0_f64));
    create_effect(move |_| {
        let o = trig_origin.get();
        let (w, h) = trig_size.get();
        active_db_anchor.set(Point::new(o.x + w, o.y + h));
    });
    // Hover brightens the label + chevron to `text()` (matching the header's
    // search/settings glyphs), no background. The colour is set on the *outer*
    // h_stack (a stable scope) and inherited by the inner label — crucially NOT
    // read inside the `active_db` dyn_container's child, because that child rebuilds
    // when `active_db` changes *while the query pane is being disposed* (opening a
    // table), which would read the freed `db_hov` signal and panic (disposed-signal
    // read). The chevron reads it directly — safe, it's not inside that container.
    // Open outranks hover for the same reason the menu-opening icons take the
    // accent (`widgets::menu_icon_color`): the pointer is still on the trigger it
    // just clicked, so a hover that won would leave the open state unmarked for
    // as long as the menu is up. Not that helper itself — this control rests in
    // its own colour rather than `text_muted`, and both halves of it (label and
    // chevron) take the answer.
    let db_hov = RwSignal::new(false);
    let db_color = move || {
        if active_db_menu_open.get() {
            theme::accent()
        } else if db_hov.get() {
            theme::text()
        } else {
            theme::bubble_claude_text()
        }
    };
    // The label is the tab's database *only while this connection has actually
    // loaded it* (`schema::shown_database`). The binding itself is left alone —
    // it is what a recovered connection restores the tab from — but a connection
    // that loaded nothing shows an empty tree and a "Disconnected" header, and a
    // toolbar naming a database nobody can list, select or read would be the one
    // surface still claiming the connection is fine.
    let shown_db = move || {
        let db = active_db.get();
        db_nodes.with(|ns| {
            let loaded: Vec<String> = ns.iter().map(|n| n.database.clone()).collect();
            schemaic_core::schema::shown_database(db.as_deref(), &loaded).map(str::to_string)
        })
    };
    let db_selector = h_stack((
        dyn_container(shown_db, move |db| {
            let name = db.unwrap_or_else(|| "No database".to_string());
            // No `.color(...)` — inherits the h_stack's (hover-reactive) colour.
            text(name)
                .style(|s| s.font_size(theme::font_title()))
                .into_any()
        }),
        icons::icon(icons::CHEVRON_DOWN, 16.0)
            // Nudge the chevron 1px down relative to its centered baseline.
            .style(move |s| {
                s.color(db_color())
                    .flex_shrink(0.0_f32)
                    .margin_top(theme::scaled(1.0))
            }),
    ))
    .on_move(move |p| trig_origin.set(p))
    .on_resize(move |r| trig_size.set((r.width(), r.height())))
    // **Guards its own launch**, in the same step that launches it: with no
    // databases the menu renders no panel, so setting the flag would leave one
    // nothing can clear — and the overlay used to stretch a transparent,
    // handler-less sheet over the whole window for it, which swallowed every
    // click in the app until the process was killed.
    .on_click_stop(move |_| {
        // Nothing *offerable* — no databases, or the eye has hidden them all —
        // and the menu renders no panel, so setting the flag would leave one
        // nothing can clear.
        let any = hidden_dbs.with_untracked(|h| {
            db_nodes.with_untracked(|ns| {
                ns.iter()
                    .any(|n| schemaic_core::schema::db_visible(h, &n.database))
            })
        });
        if !any {
            return;
        }
        // Mutual exclusivity is the trigger's own job once it absorbs the press
        // (below): the root's `close_except(None)` no longer runs for it, so
        // opening this one has to close the others itself — the shape the schema
        // eye, the gear and the activity clock already have. **After** the guard,
        // so an inert trigger closes nothing.
        menus.close_except(Some(crate::widgets::MenuId::ActiveDb));
        active_db_menu_open.update(|o| *o = !*o)
    })
    // **A menu trigger absorbs its own pointer-down** — the premise the
    // workspace root's `MenuFlags::close_except(None)` rests on. `on_click_stop`
    // registers a `Click` handler and nothing else, so the root's dismissal ran
    // first and the `Click` above turned it straight back into an open: the
    // selector could not be shut from the control that opened it. The other
    // trigger missing this was the connection switcher, and they were the only
    // two.
    //
    // Unconditional, unlike the `Click` above: the press is absorbed even when
    // there is nothing offerable and the toggle returns early. That is the right
    // way round — the root's dismissal is about *other* menus, and pressing an
    // inert control should not close one somewhere else.
    .on_event_stop(
        EventListener::PointerDown,
        crate::widgets::menu_trigger_press,
    )
    .on_event_cont(EventListener::PointerEnter, move |_| db_hov.set(true))
    .on_event_cont(EventListener::PointerLeave, move |_| db_hov.set(false))
    .style(move |s| {
        s.color(db_color())
            .flex_row()
            .items_center()
            .gap(theme::scaled(6.0))
            .padding_horiz(theme::scaled(6.0))
            .padding_vert(theme::scaled(3.0))
            .border_radius(5.0)
            .cursor(CursorStyle::Default)
    });

    // Title flush at (12, 8) from the pane edge — same as SCHEMA — via bare
    // `section_title` (no pane/toolbar padding on the left). The DB selector
    // keeps a 14px right inset, where Run used to sit.
    let toolbar = h_stack((
        section_title("QUERY"),
        empty().style(|s| s.flex_grow(1.0_f32)),
        db_selector,
    ))
    .style(|s| {
        s.width_full()
            .flex_row()
            .items_center()
            .padding_right(theme::scaled(14.0))
    });

    // Under the editor and above the results: the parameters bar takes its
    // height out of the editor pane's own region rather than the grid's, so a
    // query that grows a placeholder doesn't shrink the results.
    let params_row = params_bar(query, tab_params, dialect);

    // Non-shrinking, resizable height (the `editor_h` divider): fixed so the
    // flexbox can't collapse the bar under the grid's huge intrinsic height, and
    // the results grid below flex-grows into the remaining space.
    v_stack((toolbar, editor_wrap, params_row)).style(move |s| {
        // Collapsed → height 0 so the RESULTS grid takes the whole region (instant,
        // no animation). `editor_h` is unchanged — the restore height for
        // un-collapse — and the floor is applied here rather than written back to
        // it (`effective_editor_h`).
        let collapsed = editor_collapsed.get();
        let h = crate::consts::effective_editor_h(editor_h.get(), collapsed);
        let s = s
            .width_full()
            .height(h)
            .min_height(h)
            .min_width(0.0)
            .flex_shrink(0.0_f32)
            .flex_col()
            // No inter-row gap: the editor's 7px top inset is `editor_wrap`'s
            // own `padding_top`.
            .background(theme::bg_editor())
            .border_color(theme::border());
        // Drop the 1px bottom border when collapsed so the RESULTS panel covers the
        // region seamlessly (no leftover hairline above the grid).
        if collapsed {
            s.border_bottom(0.0)
        } else {
            s.border_bottom(1.0)
        }
    })
}

/// Largest char boundary `<= i` (std's `str::floor_char_boundary` is unstable).
/// Used to make byte offsets captured earlier safe to slice on multi-byte text.
fn floor_char_boundary(s: &str, i: usize) -> usize {
    let mut i = i.min(s.len());
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod cmdk_geometry_tests {
    use super::*;

    /// The three deleted tests' property, on the function it moved into: the bar
    /// is under the statement, and if that puts it past the pane's bottom the
    /// editor scrolls by exactly the shortfall.
    #[test]
    fn the_editor_scrolls_by_exactly_what_the_bar_overhangs() {
        // Anchor 300px down a 400px pane whose viewport starts at 0, with 60px
        // reserved: 300 + 60 = 360 ≤ 400, nothing to do.
        assert_eq!(cmdk_scroll_overflow(300.0, 0.0, 60.0, 400.0), None);
        // 20px further down and it overhangs by 20.
        assert_eq!(cmdk_scroll_overflow(360.0, 0.0, 60.0, 400.0), Some(20.0));
        // **The viewport offset is subtracted, not added.** A scrolled editor
        // measures the anchor from the document's top; the pane measures from the
        // viewport's. Getting this backwards puts the bar at the bottom of the
        // pane whenever the statement is deep in a long buffer.
        assert_eq!(
            cmdk_scroll_overflow(1360.0, 1000.0, 60.0, 400.0),
            Some(20.0)
        );
        assert_eq!(cmdk_scroll_overflow(1300.0, 1000.0, 60.0, 400.0), None);
        // Exactly flush is not an overflow.
        assert_eq!(cmdk_scroll_overflow(340.0, 0.0, 60.0, 400.0), None);
    }

    /// The footer is absolutely positioned in the pane, not clipped to the
    /// editor, and the editor scrolls under it — so it has to be refused at
    /// **both** edges.
    #[test]
    fn the_verdict_footer_is_refused_off_either_edge() {
        assert!(footer_fits(100.0, 30.0, 400.0));
        assert!(footer_fits(0.0, 30.0, 400.0), "flush with the top fits");
        assert!(
            footer_fits(370.0, 30.0, 400.0),
            "flush with the bottom fits"
        );
        // Scrolled off the top: without this it rides over the toolbar and the
        // tab strip.
        assert!(!footer_fits(-1.0, 30.0, 400.0));
        // Past the bottom by a pixel.
        assert!(!footer_fits(371.0, 30.0, 400.0));
        // A collapsed editor has room for nothing.
        assert!(!footer_fits(0.0, 30.0, 0.0));
    }

    /// The gutter strips ask the layout only about lines that are on screen.
    ///
    /// The case that matters is the whole-buffer rewrite `fix_with_ai` produces
    /// when the error's token cannot be located: `del` is the document and the
    /// viewport is fifty lines of it.
    #[test]
    fn a_hunk_is_walked_only_over_the_lines_on_screen() {
        // A 5,000-line deletion with lines 100..=149 visible.
        let (del, anchor) = visible_hunk_lines(0..5_000, 4_999, true, Some((100, 149)));
        assert_eq!(del, 100..150, "the walk is bounded by the viewport");
        assert_eq!(anchor, None, "a hunk that deletes has no separate anchor");
        // A hunk entirely above or below the viewport is skipped outright.
        assert_eq!(
            visible_hunk_lines(0..40, 39, true, Some((100, 149))).0,
            0..0
        );
        assert_eq!(
            visible_hunk_lines(900..1_000, 999, true, Some((100, 149))).0,
            0..0
        );
        // A hunk that straddles an edge keeps only its visible half — and the
        // last visible line is **inclusive**.
        assert_eq!(
            visible_hunk_lines(90..110, 109, true, Some((100, 149))).0,
            100..110
        );
        assert_eq!(
            visible_hunk_lines(140..200, 199, true, Some((100, 149))).0,
            140..150
        );
        // A hunk that fits inside the viewport is untouched.
        assert_eq!(
            visible_hunk_lines(110..120, 119, true, Some((100, 149))).0,
            110..120
        );
    }

    /// A pure insertion has no deleted line, so its anchor is the only line to
    /// visit — and it is subject to the same viewport test.
    #[test]
    fn a_pure_insertions_anchor_is_visited_only_when_it_is_on_screen() {
        assert_eq!(
            visible_hunk_lines(7..7, 7, true, Some((0, 20))),
            (0..0, Some(7))
        );
        assert_eq!(
            visible_hunk_lines(7..7, 7, true, Some((10, 20))),
            (0..0, None),
            "an anchor above the viewport is not visited"
        );
        assert_eq!(
            visible_hunk_lines(7..7, 7, true, Some((0, 6))),
            (0..0, None),
            "nor one below it"
        );
        // A pure *deletion* adds nothing, so there is no block to hang and no
        // anchor to visit — only the deleted lines' own bands.
        assert_eq!(
            visible_hunk_lines(7..9, 8, false, Some((0, 20))),
            (7..9, None)
        );
        assert_eq!(
            visible_hunk_lines(7..7, 7, false, Some((0, 20))),
            (0..0, None)
        );
    }

    /// An editor with nothing laid out places no offset at all, so there is
    /// nothing to ask about — and asking anyway is exactly the wasted scan this
    /// filter exists to avoid.
    #[test]
    fn an_unlaid_out_editor_is_asked_about_no_line() {
        assert_eq!(visible_hunk_lines(0..5_000, 0, true, None), (0..0, None));
        assert_eq!(visible_hunk_lines(3..3, 3, true, None), (0..0, None));
    }

    /// A single visible line is a valid viewport, not an empty one — the
    /// inclusive/exclusive seam where an off-by-one would silently drop the only
    /// banded line the user can see.
    #[test]
    fn a_one_line_viewport_still_yields_that_line() {
        assert_eq!(
            visible_hunk_lines(0..100, 99, true, Some((42, 42))).0,
            42..43
        );
        assert_eq!(
            visible_hunk_lines(42..43, 42, true, Some((42, 42))).0,
            42..43
        );
    }
}

#[cfg(test)]
mod inline_pane_tests {
    use super::*;

    /// The regression: a pane that did not ask for the suggestion drew it and
    /// froze itself. A `CmdK` that was never opened is the only signal a fresh
    /// pane has, and all three consequences hang off it.
    #[test]
    fn a_pane_that_did_not_open_the_prompt_does_nothing_at_all() {
        for settled in [false, true] {
            let act = inline_pane_action(false, settled);
            assert!(!act.draw, "settled={settled}");
            assert!(!act.freeze, "settled={settled}");
            assert!(!act.focus, "settled={settled}");
        }
    }

    /// The counterweight — the owning pane still does the whole job.
    #[test]
    fn the_pane_that_asked_draws_while_working_and_freezes_once_it_lands() {
        let working = inline_pane_action(true, false);
        assert!(working.draw, "the in-flight fade is what shows the range");
        assert!(
            !working.freeze,
            "the buffer is editable until there is a verdict to protect"
        );
        assert!(!working.focus);

        let landed = inline_pane_action(true, true);
        assert!(landed.draw && landed.freeze && landed.focus);
    }

    /// **The reported bug.** "Optimize" and *Fix with AI* open the bar already
    /// `Busy` from a menu click, so the editor still holds the keyboard — and the
    /// editor's Escape branch was gated on `Ready`. Escape closed the bar while
    /// prompting and while previewing a diff, and did nothing at all in between,
    /// which is the state a user most wants out of.
    #[test]
    fn escape_takes_down_a_request_that_has_not_landed_yet() {
        let working = cmdk_editor_keys(true, &InlineAiState::Busy);
        assert!(
            working.reject_on_escape,
            "Escape must abandon an in-flight request from the editor"
        );
        assert!(
            !working.accept_on_enter,
            "there is nothing to accept until the suggestion lands"
        );
    }

    /// The counterweight, so the fix above can't be had by answering every key in
    /// every state: a closed bar is not the editor's business, and the two states
    /// that keep a live focused field answer their own Escape — taking it here
    /// would steal the key from the completion popup underneath.
    #[test]
    fn the_editor_leaves_a_closed_bar_and_a_live_field_alone() {
        for state in [
            InlineAiState::Idle,
            InlineAiState::Busy,
            InlineAiState::Ready("SELECT 1".into()),
            InlineAiState::Failed("nope".into()),
        ] {
            let shut = cmdk_editor_keys(false, &state);
            assert!(!shut.accept_on_enter && !shut.reject_on_escape);
        }
        for state in [InlineAiState::Idle, InlineAiState::Failed("nope".into())] {
            let asking = cmdk_editor_keys(true, &state);
            assert!(
                !asking.reject_on_escape && !asking.accept_on_enter,
                "the prompt field answers its own keys"
            );
        }
    }

    /// The state the branch was written for still works.
    #[test]
    fn a_settled_suggestion_answers_both_keys_in_the_editor() {
        let landed = cmdk_editor_keys(true, &InlineAiState::Ready("SELECT 1".into()));
        assert!(landed.accept_on_enter && landed.reject_on_escape);
    }
}

#[cfg(test)]
mod geometry_tests {
    use super::*;

    /// A stand-in for `Editor::points_of_offset` that places every offset on one
    /// line at a fixed document y — enough to check the viewport arithmetic.
    fn at(y: f64) -> impl Fn(usize) -> Option<(Point, Point)> {
        move |off| {
            Some((
                Point::new(off as f64 * 8.0, y),
                Point::new(off as f64 * 8.0, y + 18.0),
            ))
        }
    }

    const SQL: &str = "SELECT 1;\nSELECT 2;\nSELECT * FROM nosuchtbl;";

    /// An unscrolled viewport wide enough that nothing in these fixtures clamps —
    /// the cases that *do* clamp pass their own.
    const VP: Rect = Rect::new(0.0, 0.0, 800.0, 180.0);

    // ── The vertical scrollbar's geometry ─────────────────────────────────

    /// The observed case: one long statement on a single buffer line, wrapped to
    /// thirty visual rows in a 187px editor. Counting buffer lines said it fit,
    /// so no scrollbar was drawn at all — while the wheel still scrolled it.
    ///
    /// The virtual space has since made *both* counts scrollable, so what pins
    /// the bug now is the distance: the wrapped rows scroll a whole document,
    /// the two buffer lines only their own line of virtual space.
    #[test]
    fn a_wrapped_line_overflows_even_though_its_buffer_line_does_not() {
        let (_, _, wrapped) = scrollbar_geo(30, 18.0, 187.0).expect("30 visual rows");
        let (_, _, buffer) = scrollbar_geo(2, 18.0, 187.0).expect("2 buffer lines");
        assert_eq!(wrapped, 30.0 * 18.0 - 18.0);
        assert_eq!(buffer, 2.0 * 18.0 - 18.0);
        assert!(wrapped > 10.0 * buffer, "{wrapped} vs {buffer}");
    }

    /// Only a document that ends where it starts has nothing to scroll: with the
    /// virtual space on, every further row can still be lifted to the top.
    #[test]
    fn a_single_row_hides_the_bar() {
        assert!(scrollbar_geo(1, 18.0, 187.0).is_none());
        // Defensive: a zero-row document can't be produced by `last_vline() + 1`,
        // but it must not underflow into a bar either.
        assert!(scrollbar_geo(0, 18.0, 187.0).is_none());
    }

    /// The virtual space is scrollable height, so the bar has to measure it. A
    /// document that fits the viewport still scrolls — by everything below its
    /// first row — and before this the bar was simply hidden there.
    #[test]
    fn a_document_that_fits_still_scrolls_into_the_virtual_space() {
        // 10 rows of 18 = 180px of text in a 187px viewport: the text fits, the
        // virtual space under it does not.
        let (_, _, max_scroll) = scrollbar_geo(10, 18.0, 187.0).expect("virtual space");
        assert_eq!(max_scroll, 180.0 - 18.0);
        // Exactly filling the viewport is the same story.
        assert!(scrollbar_geo(10, 18.0, 180.0).is_some());
    }

    /// An unmeasured (zero-height) viewport has no bar rather than a bar of
    /// nonsense size.
    #[test]
    fn an_unmeasured_viewport_has_no_bar() {
        assert!(scrollbar_geo(100, 18.0, 0.0).is_none());
    }

    /// The scroll ends with the **last** row at the top, not with it at the
    /// bottom — the whole point of the virtual space. Measuring the text alone
    /// stopped the thumb a viewport short of where the wheel could still go.
    #[test]
    fn the_scroll_ends_with_the_last_row_at_the_top() {
        let (_, _, max_scroll) = scrollbar_geo(30, 18.0, 180.0).expect("overflows");
        assert_eq!(max_scroll, 540.0 - 18.0);
    }

    #[test]
    fn the_thumb_shrinks_with_the_visible_fraction_but_stays_grabbable() {
        let (track, thumb, max_scroll) = scrollbar_geo(30, 18.0, 180.0).expect("overflows");
        assert_eq!(track, 180.0);
        // 540px of text + (180 − 18) of virtual space below it.
        let content = 540.0 + 162.0;
        assert_eq!(max_scroll, content - 180.0);
        // The visible fraction of *that* is the thumb's share of the track.
        assert!((thumb - 180.0 / content * 180.0).abs() < 0.01, "{thumb}");
        // A very long document keeps a minimum thumb rather than a hairline.
        let (_, thumb, _) = scrollbar_geo(10_000, 18.0, 180.0).expect("overflows");
        assert!(thumb >= 24.0, "{thumb}");
    }

    // ── The off-screen rule ───────────────────────────────────────────────
    //
    // `points_of_offset` answers `(ZERO, ZERO)` for an offset outside
    // `screen_lines`, which the old code consumed as a real position — a
    // 2px stub pinned at the editor's top-left carrying a tooltip for an
    // error 25 lines away.

    #[test]
    fn an_offset_the_editor_cannot_place_produces_no_segment() {
        assert_eq!(underline_seg_at(|_| None, SQL, 30, 40, (0.0, 0.0)), None);
        assert_eq!(span_box_at(|_| None, SQL, 30, 40, (0.0, 0.0)), None);
        assert!(statement_line_boxes_at(|_| None, SQL, 0, SQL.len(), VP).is_empty());
    }

    #[test]
    fn the_origin_pair_means_unplaced_but_a_real_offset_zero_does_not() {
        // floem's "not on screen" answer is both points at the origin. A
        // genuinely placed offset 0 differs: its *bottom* carries the line
        // height, so the two cases are distinguishable without knowing `off`.
        assert!(!placed(Point::ZERO, Point::ZERO));
        assert!(placed(Point::ZERO, Point::new(0.0, 18.0)));
    }

    // ── The viewport rule ─────────────────────────────────────────────────

    #[test]
    fn a_placed_segment_is_relative_to_the_visible_area_not_the_document() {
        // Line ~25 of a long script: document y 450, viewport scrolled to 400.
        // The old code returned the document y, putting the squiggle hundreds
        // of pixels below a ~190px pane.
        let seg = underline_seg_at(at(450.0), SQL, 10, 18, (0.0, 400.0)).unwrap();
        assert!(seg.1 < 200.0, "y must be viewport-relative, got {}", seg.1);

        let b = span_box_at(at(450.0), SQL, 10, 18, (0.0, 400.0)).unwrap();
        assert!(b.1 < 200.0, "y must be viewport-relative, got {}", b.1);

        let boxes =
            statement_line_boxes_at(at(450.0), SQL, 10, 18, Rect::new(0.0, 400.0, 800.0, 590.0));
        assert!(boxes.iter().all(|b| b.1 < 200.0), "{boxes:?}");
    }

    #[test]
    fn horizontal_scroll_shifts_every_overlay_left() {
        let unscrolled = span_box_at(at(0.0), SQL, 10, 18, (0.0, 0.0)).unwrap();
        let scrolled = span_box_at(at(0.0), SQL, 10, 18, (120.0, 0.0)).unwrap();
        assert!(
            (unscrolled.0 - scrolled.0 - 120.0).abs() < 1.5,
            "{} vs {}",
            unscrolled.0,
            scrolled.0
        );
        let u = underline_seg_at(at(0.0), SQL, 10, 18, (0.0, 0.0)).unwrap();
        let s = underline_seg_at(at(0.0), SQL, 10, 18, (120.0, 0.0)).unwrap();
        assert!((u.0 - s.0 - 120.0).abs() < 1.5, "{} vs {}", u.0, s.0);
    }

    // ── Per-line boxes ────────────────────────────────────────────────────

    #[test]
    fn a_multi_line_statement_gets_one_box_per_line() {
        let boxes = statement_line_boxes_at(at(0.0), SQL, 0, SQL.len(), VP);
        assert_eq!(boxes.len(), 3, "three lines in the fixture");
    }

    #[test]
    fn a_line_the_editor_cannot_place_is_skipped_not_collapsed_to_the_origin() {
        // Only the first line is on screen. The other two must contribute no
        // box at all rather than a box at (0,0) — that is the difference
        // between a border that stops at the fold and one that draws a stray
        // rectangle over the gutter.
        let first_line_only = |off: usize| {
            (off < 10).then(|| (Point::new(off as f64 * 8.0, 0.0), Point::new(0.0, 18.0)))
        };
        let boxes = statement_line_boxes_at(first_line_only, SQL, 0, SQL.len(), VP);
        assert_eq!(boxes.len(), 1);
    }

    // ── Staying inside the editor ─────────────────────────────────────────
    //
    // These overlays are absolutely positioned in `editor_area`, which neither
    // scrolls nor clips — so a box wider than the visible code column drew its
    // border out of the editor and across the panel beside it.

    #[test]
    fn a_statement_wider_than_the_viewport_stops_at_the_fold() {
        // One line, 400px of text, in a viewport only 200px wide.
        let wide = |off: usize| Some((Point::new(off as f64 * 8.0, 0.0), Point::new(0.0, 18.0)));
        let sql = "x".repeat(50);
        let vp = Rect::new(0.0, 0.0, 200.0, 180.0);
        let content_x = content_x_of(&sql);
        let b = statement_line_boxes_at(wide, &sql, 0, sql.len(), vp)[0];
        assert!(
            b.0 + b.2 <= content_x + 200.0 + 0.01,
            "right edge {} past the fold at {}",
            b.0 + b.2,
            content_x + 200.0
        );
    }

    #[test]
    fn a_box_scrolled_off_to_the_left_never_covers_the_gutter() {
        // Scrolled 300px right, so the statement's start is off-screen left. The
        // box begins at the code column, not over the line numbers.
        let wide = |off: usize| Some((Point::new(off as f64 * 8.0, 0.0), Point::new(0.0, 18.0)));
        let sql = "x".repeat(50);
        let content_x = content_x_of(&sql);
        let b = statement_line_boxes_at(
            wide,
            &sql,
            0,
            sql.len(),
            Rect::new(300.0, 0.0, 500.0, 180.0),
        )[0];
        assert!(b.0 >= content_x, "{} is left of the code column", b.0);
    }

    #[test]
    fn an_unmeasured_viewport_clamps_nothing_rather_than_blanking_the_highlight() {
        // Before first layout the width is 0. Treating that as "no room" would
        // drop every box; it means "unknown".
        let wide = |off: usize| Some((Point::new(off as f64 * 8.0, 0.0), Point::new(0.0, 18.0)));
        let sql = "x".repeat(50);
        let boxes = statement_line_boxes_at(wide, &sql, 0, sql.len(), Rect::ZERO);
        assert_eq!(boxes.len(), 1);
        assert!(boxes[0].2 > 300.0, "{:?}", boxes[0]);
    }

    // ── The gutter ────────────────────────────────────────────────────────

    #[test]
    fn the_code_column_widens_with_the_line_number_digit_count() {
        let one_digit = content_x_of("SELECT 1;");
        let two_digit = content_x_of(&"x\n".repeat(20));
        assert!(two_digit > one_digit);
        assert_eq!(two_digit - one_digit, HL_DIGIT_W);
    }

    // ── The run menu's placement ──────────────────────────────────────────
    //
    // The fourth `editor_area` overlay, and the last one to solve neither rule
    // above: its anchor was stored in content coords at Ctrl+Enter time and never
    // had the viewport taken off, and its edge check compared that anchor against
    // `area_w` — so a caret near the right of a long line put the menu past the
    // code column, where it was cut off mid-panel.

    const MENU: (f64, f64) = (170.0, 82.0);
    /// A 500×180 visible box, unscrolled. The code column starts at `content_x_of`,
    /// so the fold is at `content_x + 500`.
    const SEEN: Rect = Rect::new(0.0, 0.0, 500.0, 180.0);

    fn cx() -> f64 {
        content_x_of("SELECT 1;\nSELECT 2;")
    }

    #[test]
    fn a_menu_with_room_opens_at_the_caret() {
        let p = run_menu_pos(Point::new(cx() + 40.0, 60.0), MENU, cx(), SEEN);
        assert_eq!(p, Point::new(cx() + 40.0, 60.0));
    }

    /// The reported bug: the caret near the end of a long line. The menu flips to
    /// the caret's left rather than running past the code column.
    #[test]
    fn a_caret_near_the_right_edge_flips_the_menu_left_of_it() {
        let anchor_x = cx() + 480.0; // 20px short of the fold
        let p = run_menu_pos(Point::new(anchor_x, 60.0), MENU, cx(), SEEN);
        assert_eq!(p.x, anchor_x - MENU.0, "the menu's right edge is the caret");
        assert!(
            p.x + MENU.0 <= cx() + SEEN.width(),
            "{} runs past the fold",
            p.x
        );
    }

    /// Rule 1 for this overlay: the anchor is a document position, so a scrolled
    /// editor has to have the origin taken off — otherwise the menu opens where
    /// the caret *would* be with the editor scrolled home, which is what pushed it
    /// off the right edge on a long line.
    #[test]
    fn a_scrolled_editor_moves_the_menu_with_the_caret() {
        let scrolled = Rect::new(300.0, 40.0, 800.0, 220.0);
        let p = run_menu_pos(Point::new(cx() + 380.0, 100.0), MENU, cx(), scrolled);
        assert_eq!(p, Point::new(cx() + 80.0, 60.0));
    }

    /// A flip is not enough on its own: an anchor already past the fold (a caret
    /// scrolled out to the right) flips to somewhere still past it, so the result
    /// is clamped as well.
    #[test]
    fn an_anchor_past_the_fold_is_clamped_inside_it() {
        let p = run_menu_pos(Point::new(cx() + 900.0, 60.0), MENU, cx(), SEEN);
        assert!(
            p.x + MENU.0 <= cx() + SEEN.width(),
            "{} runs past the fold",
            p.x
        );
    }

    /// A visible column narrower than the menu starts it flush rather than at a
    /// negative x, which would hide the labels instead of the panel's right edge.
    #[test]
    fn a_menu_wider_than_the_column_starts_flush() {
        let narrow = Rect::new(0.0, 0.0, 60.0, 180.0);
        assert_eq!(
            run_menu_pos(Point::new(20.0, 10.0), MENU, cx(), narrow).x,
            0.0
        );
    }

    /// Vertically it clamps rather than flipping — the menu can't hang below the
    /// pane and across the results grid.
    #[test]
    fn a_caret_on_the_last_visible_line_keeps_the_menu_in_the_pane() {
        let p = run_menu_pos(Point::new(cx() + 10.0, 175.0), MENU, cx(), SEEN);
        assert!(
            p.y + MENU.1 <= EDITOR_PAD_TOP + SEEN.height(),
            "{} hangs below the pane",
            p.y
        );
        assert!(p.y > 0.0, "and not pinned to the top: {}", p.y);
    }

    /// Before the first layout the viewport is zero-sized. Clamping against that
    /// would pin every menu to the editor's top-left corner.
    #[test]
    fn an_unmeasured_viewport_clamps_nothing() {
        let p = run_menu_pos(Point::new(400.0, 90.0), MENU, cx(), Rect::ZERO);
        assert_eq!(p, Point::new(400.0, 90.0));
    }

    // ── The error bar drops Explain rather than crowding ──────────────────

    /// The direction of the comparison, and that the answer is a *share* rather
    /// than "do the buttons physically fit" — deliberately not exact widths,
    /// because the glyph measurement behind them depends on the fonts the machine
    /// running the test happens to have, and a pinned number would be a test of
    /// the CI image. Every assertion here holds at any measurement, including one
    /// that reports zero.
    #[test]
    fn a_narrow_error_bar_gives_up_explain_and_a_wide_one_does_not() {
        assert!(error_bar_fits_explain(2000.0), "a wide bar holds all three");
        assert!(
            !error_bar_fits_explain(0.0),
            "and an unmeasured one holds nothing"
        );
        // The message's share is what makes this stricter than "they fit": find
        // the width where the buttons exactly fill the bar, and the bar has to
        // still be refusing there, because at that width the message has nothing.
        let mut just_fits = 0.0_f64;
        while just_fits < 4000.0 && !error_bar_fits_explain(just_fits) {
            just_fits += 1.0;
        }
        assert!(just_fits > 0.0, "some width must be too narrow");
        assert!(
            just_fits >= 1.0 / (1.0 - ERROR_BAR_MSG_PCT),
            "the threshold is the buttons' share of the bar, not their width"
        );
    }
}
