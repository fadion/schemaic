//! The center editor pane: the SQL editor (Floem's editor engine) plus everything
//! layered over it — the Ctrl+K inline-AI popup (`cmdk_popup`), the run/AI anchored
//! menus, the statement-highlight + syntax-squiggle overlays and their geometry
//! helpers (`underline_seg`/`highlight_pick`/`statement_line_boxes`/`wavy_svg`),
//! the catalog-aware diagnostics bridge (`compute_diagnostics` → `schemaic_core::
//! intel`), and the custom overlay scrollbars. `query_pane` is the entry point
//! wired into `center`;
//! `editor_placeholder` is the no-tab fallback. Autocomplete lives in
//! `completion`, the diff preview in `diff_view`, statement/guard logic in
//! `schemaic_core::sql`.

use std::rc::Rc;

use floem::event::{Event, EventListener, EventPropagation};
use floem::keyboard::{Key, NamedKey};
use floem::kurbo::Point;
use floem::prelude::*;
use floem::reactive::{Memo, create_effect, create_memo};
use floem::style::{CursorStyle, Height, InsetLeft, InsetRight, InsetTop, Transition};
use floem::unit::Px;
use floem::views::editor::Editor;
use floem::views::editor::command::{Command, CommandExecuted};
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

use schemaic_core::diff::{DiffTag, build_diff_rows, line_diff};
use schemaic_core::intel::{self, Diagnostic, Severity, SqlDialect};
use schemaic_core::model::QueryState;
use schemaic_core::pairs::{self, PairAction};
use schemaic_core::sql::{statement_range, statement_ranges};
use schemaic_core::text_ops::{
    find_matches, matches_at, move_line, offset_of_line, replace_all, soft_tab_indent,
    soft_tab_outdent, toggle_line_comment,
};

use crate::completion::{
    Completion, accept_completion, completion_popup, recompute_completions, signature_popup,
    update_signature_help,
};
use crate::consts::*;
use crate::diff_view::diff_view;
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
        let h = if collapsed { 0.0 } else { editor_h.get() };
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
fn scrollbar_geo(lines: usize, line_h: f64, viewport_h: f64) -> Option<(f64, f64, f64)> {
    let content_h = lines as f64 * line_h;
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

// Line-level diff (`line_diff`/`build_diff_rows`, `DiffTag`/`DiffRow`) lives in
// `schemaic_core::diff` — pure text logic, unit-tested there. The views below
// render its output.

fn approve_button_style(s: floem::style::Style) -> floem::style::Style {
    s.padding_horiz(14.0)
        .padding_vert(5.0)
        .border_radius(5.0)
        .font_size(theme::FONT_BODY)
        .background(theme::approve_bg())
        .color(theme::approve_text())
        .hover(|s| s.background(theme::approve_bg().multiply_alpha(0.88)))
}

fn reject_button_style(s: floem::style::Style) -> floem::style::Style {
    s.padding_horiz(14.0)
        .padding_vert(5.0)
        .border_radius(5.0)
        .font_size(theme::FONT_BODY)
        .background(theme::reject_bg())
        .color(theme::reject_text())
        .hover(|s| s.background(theme::reject_bg().multiply_alpha(0.85)))
}

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
    // Editor-area height (tracked via on_resize), so the expanded overlay fills
    // it exactly with an explicit `height` — needed because animating requires a
    // definite height (can't interpolate to `inset(0)`'s auto height).
    area_h: RwSignal<f64>,
    dialect: SqlDialect,
) -> impl IntoView {
    // The editor's own view id, so closing the overlay can hand focus back to
    // the editor (else focus is left dangling after the input is torn down).
    let editor_view_id = ed.editor_view_id;

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
                let Some(intent) = cmdk.input.try_get_untracked() else {
                    return; // tab closed in the same tick
                };
                if intent.trim().is_empty() {
                    return;
                }
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
                    edit_untyped(&ed, comp, Selection::region(s, e), &sql, EditType::Paste);
                    ed.cursor
                        .update(|c| c.set_offset(s + sql.len(), false, false));
                }
                cmdk.open.set(false);
                inline_ai.set(InlineAiState::Idle);
            });
        }
    };

    let content = dyn_container(
        move || (cmdk.open.get(), inline_ai.get()),
        move |(open, state)| {
            if !open {
                return empty().into_any();
            }
            // Idle = compact prompt box below the line; any other state expands
            // the overlay to cover the editor with the question + diff + buttons.
            let expanded = !matches!(state, InlineAiState::Idle);
            let ready = matches!(state, InlineAiState::Ready(_));
            // Enter: accept when a result is ready, submit when compact, and do
            // nothing while Busy/Failed (swallowed so it can't re-submit).
            let on_submit: Option<Rc<dyn Fn()>> = if ready {
                let accept_k = accept.clone();
                Some(Rc::new(accept_k))
            } else if !expanded {
                let submit_k = submit.clone();
                Some(Rc::new(submit_k))
            } else {
                None
            };
            let discard_esc = discard.clone();
            // The Ctrl+K prompt on the shared editor field. Borderless &
            // transparent in BOTH states — the outer container owns the box
            // surface (border/bg) so it can animate as one element from compact
            // to full. Read-only once expanded. 40px tall in both states.
            let input_row = edit_field(
                cmdk.input,
                FieldCfg {
                    placeholder: "Ask the AI Assistant for help.",
                    background: bg_transparent,
                    font_size: theme::FONT_TITLE,
                    read_only: expanded,
                    // Focus in both states: compact to type, expanded (read-only,
                    // no caret) so Enter=accept / Escape=discard still work.
                    autofocus: true,
                    height: Some(40.0),
                    text_color: Some(theme::cmdk_text),
                    placeholder_color: Some(theme::cmdk_placeholder),
                    border_color: Some(bg_transparent),
                    on_submit,
                    on_escape: Some(Rc::new(discard_esc)),
                    ..Default::default()
                },
            )
            .style(|s| s.width_full().min_width(0.0).flex_shrink(0.0_f32));

            let body = match state {
                InlineAiState::Idle => empty().into_any(),
                InlineAiState::Busy => {
                    let discard_c = discard.clone();
                    container(
                        h_stack((
                            verb_spinner(theme::text_dim, theme::FONT_BODY),
                            // Same style as Reject; cancels the generation and
                            // closes the overlay (see `discard`).
                            container(text("Cancel"))
                                .on_click_stop(move |_| discard_c())
                                .style(reject_button_style),
                        ))
                        .style(|s| s.flex_row().items_center().gap(10.0)),
                    )
                    // 5px gap from the question field above (matches the diff).
                    .style(|s| {
                        s.width_full()
                            .padding_horiz(10.0)
                            .padding_top(5.0)
                            .padding_bottom(10.0)
                    })
                    .into_any()
                }
                InlineAiState::Failed(msg) => container(
                    text(msg).style(|s| s.color(theme::error()).font_size(theme::FONT_BODY)),
                )
                .style(|s| {
                    s.width_full()
                        .padding_horiz(10.0)
                        .padding_top(5.0)
                        .padding_bottom(10.0)
                })
                .into_any(),
                InlineAiState::Ready(sql) => {
                    let discard_b = discard.clone();
                    let accept_b = accept.clone();
                    let old_full = query.get_untracked();
                    let (s0, e0) = (cmdk.start.get_untracked(), cmdk.end.get_untracked());
                    // Clamp to char boundaries so multi-byte text (e.g. 'naïve')
                    // can't slice mid-codepoint and panic (C12).
                    let end = floor_char_boundary(&old_full, e0.min(old_full.len()));
                    let s0c = floor_char_boundary(&old_full, s0.min(end));
                    let new_full = format!("{}{}{}", &old_full[..s0c], sql, &old_full[end..]);
                    let diff = line_diff(&old_full, &new_full);
                    let added = diff
                        .iter()
                        .filter(|(t, _)| matches!(t, DiffTag::Ins))
                        .count();
                    let removed = diff
                        .iter()
                        .filter(|(t, _)| matches!(t, DiffTag::Del))
                        .count();
                    let no_changes = added == 0 && removed == 0;
                    let rows = build_diff_rows(diff);

                    // Body: the diff, or a "no changes" note if the suggestion is
                    // identical to the current text (else the diff would be a lone
                    // "⋯ N unchanged lines" gap, which reads as a bug).
                    let diff_area = if no_changes {
                        container(
                            text("No changes suggested.")
                                .style(|s| s.color(theme::text_dim()).font_size(theme::FONT_BODY)),
                        )
                        .style(|s| {
                            s.width_full()
                                .flex_shrink(0.0_f32)
                                .padding_horiz(10.0)
                                .padding_top(5.0)
                                .padding_bottom(10.0)
                        })
                        .into_any()
                    } else {
                        // Fixed-height scroll (CMDK_DIFF_H); inner clip wrapper
                        // carries the 5px radius (the scroll's own border_radius
                        // didn't round visibly); `shift_hscroll` = Shift+wheel
                        // horizontal scroll. Buttons below always stay visible.
                        container(
                            container(
                                autohide(shift_hscroll(diff_view(rows, dialect)))
                                    .style(|s| s.height(CMDK_DIFF_H).width_full().min_width(0.0)),
                            )
                            .style(|s| s.width_full().border_radius(5.0))
                            .clip(),
                        )
                        .style(|s| {
                            // 5px gap from the question field above; 10px gap to
                            // the buttons below.
                            s.flex_col()
                                .width_full()
                                .min_width(0.0)
                                .flex_shrink(0.0_f32)
                                .padding_horiz(5.0)
                                .padding_top(5.0)
                                .padding_bottom(10.0)
                        })
                        .into_any()
                    };

                    // `+N −M` change summary, left of the buttons (hidden when
                    // there's nothing to summarize).
                    let stat = if no_changes {
                        empty().into_any()
                    } else {
                        h_stack((
                            text(format!("+{added}")).style(|s| {
                                s.font_size(theme::FONT_BODY)
                                    .color(theme::diff_add_marker())
                            }),
                            text(format!("−{removed}")).style(|s| {
                                s.font_size(theme::FONT_BODY)
                                    .color(theme::diff_del_marker())
                            }),
                        ))
                        .style(|s| s.flex_row().items_center().gap(8.0))
                        .into_any()
                    };

                    v_stack((
                        diff_area,
                        container(
                            h_stack((
                                stat,
                                empty().style(|s| s.flex_grow(1.0_f32)),
                                container(text("Approve"))
                                    .on_click_stop(move |_| accept_b())
                                    .style(approve_button_style),
                                container(text("Reject"))
                                    .on_click_stop(move |_| discard_b())
                                    .style(reject_button_style),
                            ))
                            .style(|s| {
                                s.flex_row()
                                    .items_center()
                                    .width_full()
                                    .min_width(0.0)
                                    .gap(10.0)
                            }),
                        )
                        // No top padding: the diff wrapper's 10px bottom padding
                        // alone is the gap above the buttons (was 10+5=15).
                        .style(|s| {
                            s.width_full()
                                .flex_shrink(0.0_f32)
                                .padding_horiz(5.0)
                                .padding_bottom(5.0)
                        }),
                    ))
                    .style(|s| s.flex_col().width_full().min_width(0.0))
                    .into_any()
                }
            };

            // Clip the INNER content (not the absolute outer container — clipping
            // that hides the whole overlay) so long input/diff text stays inside
            // the border. Belt-and-suspenders over the per-element clips below.
            v_stack((input_row, body))
                .style(|s| {
                    s.flex_col()
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
    container(content).style(move |s| {
        if !cmdk.open.get() {
            return s;
        }
        // The box IS the outer container in both states (the field inside is
        // borderless), so it can animate as one element. Position/size go from
        // the compact box (a ~42px box below the caret line, inset 10) to the
        // full editor (inset 0, height = editor-area). All values are Px so they
        // interpolate; the 150ms transition on each animates compact↔expanded.
        // (Open/close is Auto↔Px, which doesn't interpolate, so it snaps — good.)
        let t = Transition::ease_in_out(std::time::Duration::from_millis(100));
        let expanded = !matches!(inline_ai.get(), InlineAiState::Idle);
        let s = s
            .absolute()
            .flex_col()
            .background(theme::bg_deepest())
            .border(1.0)
            .border_color(theme::chip_active())
            .border_radius(6.0)
            .transition(InsetTop, t.clone())
            .transition(InsetLeft, t.clone())
            .transition(InsetRight, t.clone())
            .transition(Height, t);
        if expanded {
            s.inset_left(0.0)
                .inset_right(0.0)
                .inset_top(0.0)
                .height(area_h.get())
        } else {
            // `p.y` = the caret line's bottom edge; +8 clears the line, inset 10
            // from the editor edges — matching the pre-animation compact box.
            let p = cmdk.point.get();
            s.inset_left(10.0)
                .inset_right(10.0)
                .inset_top(p.y + 8.0)
                .height(42.0)
        }
    })
}

/// Editor-local state for the inline (Ctrl+K) AI prompt popup. `start`/`end` are
/// the doc byte-range captured at trigger time — equal ⇒ generate/insert at the
/// caret; distinct ⇒ transform that selection. `point` anchors the popup.
#[derive(Clone, Copy)]
struct CmdK {
    open: RwSignal<bool>,
    point: RwSignal<Point>,
    input: RwSignal<String>,
    start: RwSignal<usize>,
    end: RwSignal<usize>,
}

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
    intel::diagnostics(sql, &catalog, dialect)
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

/// The x origin of the code column in `editor_area` coords: the gutter widens
/// with the line-number digit count, since it sizes to the last line number.
/// `points_of_offset().x` is text-layout-relative (0 = code start), so this is
/// what turns it into editor-area x.
fn content_x_of(sql: &str) -> f64 {
    let total_lines = sql.bytes().filter(|&c| c == b'\n').count() + 1;
    let digits = total_lines.to_string().len();
    HL_GUTTER + digits.saturating_sub(1) as f64 * HL_DIGIT_W
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
fn statement_line_boxes_at(
    points: impl Fn(usize) -> Option<(Point, Point)>,
    sql: &str,
    lo: usize,
    hi: usize,
    vp: (f64, f64),
) -> Vec<(f64, f64, f64, f64)> {
    let content_x = content_x_of(sql);
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
            let x0 = content_x + top.x - HL_PAD - vp.0;
            let x1 = content_x + end.x + HL_PAD - vp.0;
            boxes.push((
                x0,
                top.y + EDITOR_PAD_TOP - vp.1,
                (x1 - x0).max(6.0),
                bot.y - top.y,
            ));
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
    let vp = ed.viewport.get();
    statement_line_boxes_at(editor_points(ed), sql, lo, hi, (vp.x0, vp.y0))
}

/// The full set of signals/callbacks `query_pane` threads into the editor and its
/// overlays. Bundled so the builder takes a single argument.
pub(crate) struct QueryPaneParams {
    pub query: RwSignal<String>,
    /// Caret byte offset, mirrored out of the editor for the status-bar Ln/Col.
    pub cursor_offset: RwSignal<usize>,
    /// Opens the Go-to-line popup (Ctrl+G, or a status-bar Ln/Col click).
    pub goto_open: RwSignal<bool>,
    /// When set, jump the caret to this byte offset (move + centre + focus), then
    /// clear it. Driven by the status-bar warning count.
    pub jump_offset: RwSignal<Option<usize>>,
    /// Where this pane publishes its (debounced) offline diagnostics. Lives on
    /// the tab so the status bar reads the analysis rather than repeating it —
    /// see [`Tab::diagnostics`](crate::Tab::diagnostics).
    pub syntax: RwSignal<Vec<Diagnostic>>,
    pub results: RwSignal<QueryState>,
    /// The **guarded** run action ([`crate::TabsActions::run`]) — a held-back run
    /// lands in `run_guard` and nothing executes.
    pub run: Rc<dyn Fn(String)>,
    /// The guarded batch run ([`crate::TabsActions::run_all`]).
    pub run_all: Rc<dyn Fn(Vec<String>)>,
    /// What the write guard is holding, if anything — rendered as the guard bar.
    pub run_guard: RwSignal<Option<crate::RunGuard>>,
    /// The guard bar's "Run anyway" ([`crate::TabsActions::run_anyway`]).
    pub run_anyway: Rc<dyn Fn()>,
    pub db_nodes: RwSignal<Vec<ConnNode>>,
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
    pub active_db_menu_open: RwSignal<bool>,
    pub active_db_anchor: RwSignal<Point>,
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
        goto_open,
        jump_offset,
        syntax,
        results,
        run,
        run_all,
        run_guard: guard,
        run_anyway,
        db_nodes,
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
        active_db_menu_open,
        active_db_anchor,
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
        sel: RwSignal::new(0),
        open: RwSignal::new(false),
        point: RwSignal::new(Point::ZERO),
        suppress: RwSignal::new(false),
        sig: RwSignal::new(None),
        sig_point: RwSignal::new(Point::ZERO),
    };
    let cmdk = CmdK {
        open: RwSignal::new(false),
        point: RwSignal::new(Point::ZERO),
        input: RwSignal::new(String::new()),
        start: RwSignal::new(0),
        end: RwSignal::new(0),
    };
    // Right-click editor menu (Ask AI… / Explain / Optimize). It's routed through
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
                        db_nodes,
                        comp,
                        adb.as_deref(),
                        dialect.get_untracked(),
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
                cmdk.start.set(start);
                cmdk.end.set(end);
                // Anchor to the BOTTOM of the caret's line (absolute
                // screen coords via `points_of_offset().1`), not the
                // per-line baseline `line_point_of_offset` returns —
                // that baseline is ~0 on an empty line but ~font-ascent
                // once the line has glyphs, which shoved the box down
                // whenever the editor had code.
                let (_, mut below) = e.points_of_offset(offset, CursorAffinity::Backward);
                below.y += EDITOR_PAD_TOP;
                cmdk.point.set(below);
            });
            cmdk.input.set(String::new());
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
                    let edit = toggle_line_comment(&full, a.min(b), a.max(b));
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
                    // Same gutter math as the statement-highlight boxes so the menu
                    // sits under the caret — `COMPLETION_GUTTER` (which the
                    // completion popup hides behind its own padding) underestimates
                    // the real gutter, so the menu drifted ~18px left (§7.4).
                    let digits = (sql.bytes().filter(|&c| c == b'\n').count() + 1)
                        .to_string()
                        .len();
                    let content_x = HL_GUTTER + digits.saturating_sub(1) as f64 * HL_DIGIT_W;
                    run_menu.set(Some(Point::new(
                        content_x + below.x,
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
        // Nothing else reads `read_only` (the app gates writes elsewhere) and the
        // handler → built-in-insert step is synchronous, so there's no race or
        // flicker — the built-in `receive_char` sees `read_only` and no-ops.
        if !mods.control()
            && !mods.alt()
            && let KeyInput::Keyboard(Key::Character(cs), _) = &kp.key
            && let Some(ch) = single_char(cs)
            && matches!(ch, '(' | ')' | '\'' | '"' | '`')
        {
            let dia = dialect.get_untracked();
            let acted = editor_sig.with_untracked(|e| {
                let doc = e.doc();
                let full = doc.text().to_string();
                let cur = e.cursor.get_untracked();
                let off = cur.offset();
                let (a, b) = cur.get_selection().unwrap_or((off, off));
                let Some(action) = pairs::auto_pair(&full, a, b, ch, dia) else {
                    return false;
                };
                // Suppress the built-in char insert for the rest of this dispatch.
                let ro = e.read_only;
                ro.set(true);
                floem::action::exec_after(std::time::Duration::ZERO, move |_| {
                    if ro.try_get_untracked().is_some() {
                        ro.set(false);
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
    let ed_menu2 = ed.clone(); // menu actions: anchor point for "Ask AI…"
    let ed_run = ed.clone(); // run menu: re-focus the editor after running
    let ed_hl = ed.clone(); // statement-highlight overlay geometry
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

    // Mirror the caret's byte offset into the tab's `cursor_offset` signal so the
    // status bar can render Ln/Col. Tracks `ed.cursor`, so it fires on every caret
    // move / selection change; disposed with this pane when the tab closes.
    create_effect(move |_| {
        cursor_offset.set(ed_cursor.cursor.get().offset());
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

    // Builds the right-click menu entries (Ask AI… / Explain / Optimize) for the
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
        Rc::new(move || {
            let ed_ask = ed_menu_act.clone();
            let ai_explain = ai_send.clone();
            let run_optimize = inline_ai_run.clone();
            let show_plan = open_plan.clone();
            let ed_format = ed_fmt.clone();
            let make_view = create_view.clone();
            // The three AI actions carry the sparkle (matching AI Summary in the
            // grid); Plan + Format sit below the separator as plain actions.
            let mut entries = vec![
                MenuEntry::action_icon(
                    "Ask AI…",
                    (icons::SPARKLES, theme::key_foreign),
                    move || {
                        let off = menu_offset.get_untracked();
                        cmdk.start.set(off);
                        cmdk.end.set(off);
                        let (_, mut below) = ed_ask.points_of_offset(off, CursorAffinity::Backward);
                        below.y += EDITOR_PAD_TOP;
                        cmdk.point.set(below);
                        cmdk.input.set(String::new());
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
                    let (lo, hi) =
                        statement_range(&sql, menu_offset.get_untracked(), dialect.get_untracked());
                    if let Some(stmt) = sql.get(lo..hi).filter(|s| !s.trim().is_empty()) {
                        (show_plan)(stmt.to_string());
                        highlight_pick(&sql, lo, hi, highlight);
                    }
                }),
                MenuEntry::action("Format", move || {
                    format_editor(&ed_format, comp, dialect.get_untracked())
                }),
            ];
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

    // Editor-area height, tracked so the Ctrl+K expand animation fills it exactly.
    let area_h: RwSignal<f64> = RwSignal::new(EDITOR_H);
    // Editor-area width, tracked so right-click / run menus flip leftward instead
    // of being clipped by the pane edge (they live in editor-area coords).
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
    // spurious horizontal scrollbar flashes until the first scroll. Once the
    // layout has settled, replay a tiny horizontal scroll (exactly what a manual
    // scroll does) to force the content re-measure that clears it; empty/short
    // content simply clamps back to the origin.
    {
        let ed_poke = ed.clone();
        floem::action::exec_after(std::time::Duration::from_millis(200), move |_| {
            // Guard: the tab (and this editor's scope) may be gone within 200 ms.
            let _ = ed_poke
                .scroll_delta
                .try_update(|d| *d = floem::kurbo::Vec2::new(1.0, 0.0));
        });
    }
    let doc = editor.doc();
    let input = editor
        .styling(sql_highlight::SqlStyling::new(
            doc,
            dialect.get_untracked(),
            zoom,
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
                .gutter_left_padding(14.0);
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
                        db_nodes,
                        comp,
                        adb.as_deref(),
                        dialect.get_untracked(),
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
            // Also dismiss the Ctrl+K prompt: only its *compact* box sits on top as
            // a sibling, so a click in the editor outside it lands here; expanded
            // states cover the editor, so their clicks hit the overlay, not this.
            // Reset to Idle so a later reopen starts clean.
            if cmdk.open.get_untracked() {
                cmdk.open.set(false);
                if !matches!(inline_ai.get_untracked(), InlineAiState::Idle) {
                    inline_ai.set(InlineAiState::Idle);
                }
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
                .padding_bottom(10.0)
                .padding_right(5.0)
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
        dialect.get_untracked(),
    );
    // AI Fix: open the Ctrl+K overlay pre-filled with the error and auto-submit
    // a whole-query fix. Goes straight to Busy (skips the compact prompt); the
    // user only approves/rejects the resulting diff.
    let ai_fix: Rc<dyn Fn()> = {
        let inline_ai_run = inline_ai_run.clone();
        Rc::new(move || {
            let sql = query.get_untracked();
            let err = match results.get_untracked() {
                QueryState::Failed(m) => m,
                _ => return,
            };
            cmdk.start.set(0);
            cmdk.end.set(sql.len());
            let first = err.lines().next().unwrap_or("").trim().to_string();
            cmdk.input.set(format!("Fix this error: {first}"));
            let intent = format!(
                "The query failed with this error:\n{err}\n\nReturn the corrected SQL only."
            );
            inline_ai.set(InlineAiState::Busy);
            cmdk.open.set(true);
            (inline_ai_run)(InlineAiRequest {
                intent,
                current_sql: sql.clone(),
                selection: Some(sql),
            });
        })
    };

    // Error bar: pinned to the editor's bottom (5px inset) when the last run
    // failed. Truncated error + View (opens the modal) + AI Fix. Cleared by any
    // edit (see the editor `.update`). border_radius rounds the filled bar; no
    // `.clip()` (clipping the absolute container would hide it).
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
                h_stack((
                    text(one_line).style(|s| {
                        s.color(theme::reject_text())
                            .font_size(theme::FONT_BODY)
                            .max_width_pct(60.0)
                            .text_ellipsis()
                            .margin_left(8.0)
                    }),
                    text("View")
                        .on_click_stop(move |_| error_modal_open.set(true))
                        .style(|s| {
                            s.color(theme::err_fix_btn())
                                .font_size(theme::FONT_BODY)
                                .margin_left(10.0)
                        }),
                    empty().style(|s| s.flex_grow(1.0_f32)),
                    h_stack((
                        icons::icon(icons::SPARKLES, 16.0)
                            .style(|s| s.color(theme::err_fix_btn()).margin_right(5.0)),
                        text("AI Fix")
                            .style(|s| s.color(theme::err_fix_btn()).font_size(theme::FONT_BODY)),
                    ))
                    .on_click_stop(move |_| (ai_fix)())
                    .style(|s| s.flex_row().items_center().margin_right(8.0)),
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
            if matches!(results.get(), QueryState::Failed(_)) {
                s.absolute()
                    .inset_left(5.0)
                    .inset_right(5.0)
                    .inset_bottom(5.0)
                    .height(35.0)
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
                                .font_size(theme::FONT_BODY)
                                .margin_right(8.0)
                        })
                        .into_any()
                } else {
                    empty().into_any()
                };
                h_stack((
                    text(g.message).style(|s| {
                        s.color(theme::reject_text())
                            .font_size(theme::FONT_BODY)
                            .max_width_pct(70.0)
                            .text_ellipsis()
                            .margin_left(8.0)
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
                    .inset_left(5.0)
                    .inset_right(5.0)
                    .inset_bottom(5.0)
                    .height(35.0)
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
                            let s = s.padding_vert(8.0);
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
                let panel = v_stack((
                    row(0, "Run Current", run_current),
                    row(1, "Run Everything", run_everything),
                ))
                .keyboard_navigable()
                .request_focus(|| {})
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
                        (refocus)();
                    }
                })
                .on_event_stop(EventListener::PointerDown, |_| {})
                .style(|s| {
                    panel_style(s)
                        .background(theme::bg_chrome())
                        .min_width(170.0)
                        .padding_vert(6.0)
                        .font_size(theme::FONT_TITLE)
                });
                let positioned = container(panel).style(move |s| {
                    // Flip leftward at the pane's right edge (same as the AI menu).
                    const MENU_W: f64 = 170.0;
                    let w = area_w.get();
                    let x = if w > 0.0 && pos.x + MENU_W > w {
                        (pos.x - MENU_W).max(0.0)
                    } else {
                        pos.x
                    };
                    s.absolute().inset_left(x).inset_top(pos.y)
                });
                // Same for a click outside: the catcher stops propagation, so the
                // click that dismissed the menu never reaches the editor either.
                let catcher = empty()
                    .on_event_stop(EventListener::PointerDown, move |_| {
                        run_menu.set(None);
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
                            text(msg.clone()).style(|s| s.font_size(12.0).max_width(360.0))
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
                .inset_right(7.0)
                .inset_bottom(7.0)
                .items_center()
                .justify_center()
                .padding_left(10.0)
                .padding_right(8.0)
                .padding_vert(8.0)
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
                        .margin_left(2.0)
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
                        font_size: 13.0,
                        border_radius: 6.0,
                        height: Some(26.0),
                        on_submit: Some(on_submit),
                        on_escape: Some(Rc::new(move || (esc)())),
                        on_arrow_up: Some(on_up),
                        on_arrow_down: Some(on_down),
                        ..Default::default()
                    },
                )
                .style(|s| s.width(170.0));
                let count = dyn_container(
                    // `with`, not `get` — reading a length shouldn't clone the hits.
                    move || (find_hits.with(|h| h.len()), find_idx.get()),
                    move |(n, i)| {
                        let cur = if n == 0 { 0 } else { i + 1 };
                        text(format!("{cur}/{n}"))
                            .style(|s| {
                                s.font_size(theme::FONT_LABEL)
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
                .style(|s| s.items_center().gap(8.0));

                // ── Row 2: replace ──
                // Text buttons: colour-only hover (no background). Fixed 26px height
                // with centred text so they line up with the 26px field when the row
                // is top-aligned (`items_start`, which keeps the reveal from spilling
                // upward over the find row).
                let text_btn = |label: &'static str, on: Rc<dyn Fn()>| {
                    container(text(label).style(|s| s.font_size(theme::FONT_LABEL)))
                        .on_click_stop(move |_| (on)())
                        .style(|s| {
                            s.items_center()
                                .height(26.0)
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
                        font_size: 13.0,
                        border_radius: 6.0,
                        height: Some(26.0),
                        on_submit: Some(ro),
                        on_escape: Some(esc2),
                        ..Default::default()
                    },
                )
                .style(|s| s.width(170.0));
                // The replace row is always mounted but shown/hidden via `display`
                // (no animation — an in-flow height transition through a clip was
                // janky, and the reveal isn't worth it here). Hidden ⇒ `display:none`.
                // Left-packed (gap 0, explicit margins): `Replace` is offset 16px past
                // the field so it lines up under the "n/total" counter in the find row
                // (input 170 + the row's 2×8px gaps), and `All` sits 15px after it.
                let replace_row = h_stack((
                    rinput,
                    text_btn("Replace", replace_one.clone()).style(|s| s.margin_left(16.0)),
                    text_btn("All", replace_all_cb.clone()).style(|s| s.margin_left(15.0)),
                ))
                .style(move |s| {
                    let s = s.items_center().padding_top(6.0);
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
                let content = v_stack((row1, replace_row)).style(|s| s.width(283.0));
                h_stack((toggle, content))
                    .style(|s| {
                        s.items_center()
                            .gap(8.0)
                            .padding_horiz(8.0)
                            .padding_vert(6.0)
                            .background(theme::bg_panel())
                            .border(1.0)
                            .border_color(theme::border())
                            .border_radius(8.0)
                    })
                    .into_any()
            },
        )
        .style(|s| s.absolute().inset_top(5.0).inset_right(5.0))
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
                        font_size: 13.0,
                        border_radius: 6.0,
                        height: Some(26.0),
                        on_submit: Some(submit.clone()),
                        on_escape: Some(Rc::new(move || (esc)())),
                        ..Default::default()
                    },
                )
                .style(|s| s.width(52.0));
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
                        .style(|s| s.font_size(theme::FONT_LABEL).color(theme::text_dim())),
                    input,
                    close_btn,
                ))
                .style(|s| {
                    s.items_center()
                        .gap(8.0)
                        .padding_horiz(8.0)
                        .padding_vert(6.0)
                        .background(theme::bg_panel())
                        .border(1.0)
                        .border_color(theme::border())
                        .border_radius(8.0)
                })
                .into_any()
            },
        )
        .style(|s| s.absolute().inset_top(5.0).inset_right(5.0))
    };

    // Order: editor, syntax squiggles, statement highlight, run overlay, then the
    // completion popup / error+guard bars / Ctrl+K / run menu / find bar on top.
    // (The right-click AI menu is rendered at the workspace root via `popup_menu`,
    // so it floats over the results pane instead of being clipped here.)
    let editor_area = stack((
        editor_box,
        v_scrollbar,
        h_scrollbar,
        syntax_view,
        highlight_view,
        bracket_match_view,
        occurrences_view,
        run_overlay,
        completion_popup(comp),
        signature_popup(comp),
        error_bar,
        guard_bar,
        cmdk_view,
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
            .padding_top(3.0)
            .padding_horiz(13.0)
            .padding_bottom(13.0)
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
    let db_hov = RwSignal::new(false);
    let db_color = move || {
        if db_hov.get() {
            theme::text()
        } else {
            theme::bubble_claude_text()
        }
    };
    let db_selector = h_stack((
        dyn_container(
            move || active_db.get(),
            move |db| {
                let name = db.unwrap_or_else(|| "No database".to_string());
                // No `.color(...)` — inherits the h_stack's (hover-reactive) colour.
                text(name)
                    .style(|s| s.font_size(theme::FONT_TITLE))
                    .into_any()
            },
        ),
        icons::icon(icons::CHEVRON_DOWN, 16.0)
            // Nudge the chevron 1px down relative to its centered baseline.
            .style(move |s| s.color(db_color()).flex_shrink(0.0_f32).margin_top(1.0)),
    ))
    .on_move(move |p| trig_origin.set(p))
    .on_resize(move |r| trig_size.set((r.width(), r.height())))
    .on_click_stop(move |_| active_db_menu_open.update(|o| *o = !*o))
    .on_event_cont(EventListener::PointerEnter, move |_| db_hov.set(true))
    .on_event_cont(EventListener::PointerLeave, move |_| db_hov.set(false))
    .style(move |s| {
        s.color(db_color())
            .flex_row()
            .items_center()
            .gap(6.0)
            .padding_horiz(6.0)
            .padding_vert(3.0)
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
    .style(|s| s.width_full().flex_row().items_center().padding_right(14.0));

    // Non-shrinking, resizable height (the `editor_h` divider): fixed so the
    // flexbox can't collapse the bar under the grid's huge intrinsic height, and
    // the results grid below flex-grows into the remaining space.
    v_stack((toolbar, editor_wrap)).style(move |s| {
        // Collapsed → height 0 so the RESULTS grid takes the whole region (instant,
        // no animation). `editor_h` is unchanged — the restore height for un-collapse.
        let collapsed = editor_collapsed.get();
        let h = if collapsed { 0.0 } else { editor_h.get() };
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

    // ── The vertical scrollbar's geometry ─────────────────────────────────

    /// The observed case: one long statement on a single buffer line, wrapped to
    /// thirty visual rows in a 187px editor. Counting buffer lines said it fit,
    /// so no scrollbar was drawn at all — while the wheel still scrolled it.
    #[test]
    fn a_wrapped_line_overflows_even_though_its_buffer_line_does_not() {
        assert!(scrollbar_geo(30, 18.0, 187.0).is_some(), "30 visual rows");
        assert!(scrollbar_geo(2, 18.0, 187.0).is_none(), "2 buffer lines");
    }

    #[test]
    fn content_that_fits_hides_the_bar() {
        assert!(scrollbar_geo(10, 18.0, 187.0).is_none());
        // Exactly filling the viewport is not overflow.
        assert!(scrollbar_geo(10, 18.0, 180.0).is_none());
        assert!(scrollbar_geo(11, 18.0, 180.0).is_some());
    }

    /// An unmeasured (zero-height) viewport has no bar rather than a bar of
    /// nonsense size.
    #[test]
    fn an_unmeasured_viewport_has_no_bar() {
        assert!(scrollbar_geo(100, 18.0, 0.0).is_none());
    }

    #[test]
    fn the_thumb_shrinks_with_the_visible_fraction_but_stays_grabbable() {
        let (track, thumb, max_scroll) = scrollbar_geo(30, 18.0, 180.0).expect("overflows");
        assert_eq!(track, 180.0);
        assert_eq!(max_scroll, 540.0 - 180.0);
        // A third of the content is visible → a third of the track.
        assert!((thumb - 60.0).abs() < 0.01, "{thumb}");
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
        assert!(statement_line_boxes_at(|_| None, SQL, 0, SQL.len(), (0.0, 0.0)).is_empty());
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

        let boxes = statement_line_boxes_at(at(450.0), SQL, 10, 18, (0.0, 400.0));
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
        let boxes = statement_line_boxes_at(at(0.0), SQL, 0, SQL.len(), (0.0, 0.0));
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
        let boxes = statement_line_boxes_at(first_line_only, SQL, 0, SQL.len(), (0.0, 0.0));
        assert_eq!(boxes.len(), 1);
    }

    // ── The gutter ────────────────────────────────────────────────────────

    #[test]
    fn the_code_column_widens_with_the_line_number_digit_count() {
        let one_digit = content_x_of("SELECT 1;");
        let two_digit = content_x_of(&"x\n".repeat(20));
        assert!(two_digit > one_digit);
        assert_eq!(two_digit - one_digit, HL_DIGIT_W);
    }
}
