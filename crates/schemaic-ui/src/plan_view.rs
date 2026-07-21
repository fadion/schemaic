//! The Query Plan modal (`EXPLAIN` / `EXPLAIN ANALYZE`).
//!
//! Opened from the editor right-click menu's **Plan** action (which seeds
//! `overlay.plan_sql` and opens `overlay.plan_open`). While open, an effect runs
//! the EXPLAIN for the current statement — re-running whenever the **Analyze**
//! toggle flips — and drops the result into `overlay.plan_state`. The body renders
//! the plan as a table with heuristic warnings (full scans, filesort, temp
//! tables) called out, and an **Ask AI** button ships the plan + query to the
//! assistant for optimization suggestions.
//!
//! Plain `EXPLAIN` only plans (never executes), so it's always safe. `EXPLAIN
//! ANALYZE` executes the statement to measure it, so the Analyze toggle is
//! disabled for statements that write (`sql::contains_write`).

use std::collections::HashSet;
use std::rc::Rc;

use floem::AnyView;
use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;
use floem::reactive::create_effect;

use schemaic_core::plan::{PlanWarningKind, QueryPlan};

use crate::settings::themed_toggle;
use crate::theme::{FONT_BODY, FONT_LABEL};
use crate::widgets::{
    autohide, loading_dots, measure_px, modal_title_borderless, panel_style, shift_hscroll,
};
use crate::{PlanState, RightPanel, Ui, icons, theme};

pub(crate) fn plan_overlay(ui: Ui) -> impl IntoView {
    let plan_open = ui.overlay.plan_open;
    let plan_state = ui.overlay.plan_state;
    let plan_sql = ui.overlay.plan_sql;
    let plan_analyze = ui.overlay.plan_analyze;
    let run_plan = ui.tab_actions.run_plan.clone();
    let ai_send = ui.ai_actions.send.clone();
    let right_panel = ui.layout.right_panel;

    dyn_container(
        move || plan_open.get(),
        move |is_open| {
            if !is_open {
                return empty().into_any();
            }
            let close: Rc<dyn Fn()> = Rc::new(move || {
                plan_open.set(false);
                plan_state.set(PlanState::Idle);
            });

            // Drive the EXPLAIN: fires once on open, and again whenever the Analyze
            // toggle flips. `plan_sql` is read untracked (it only changes together
            // with a fresh open, which rebuilds this whole branch).
            {
                let run_plan = run_plan.clone();
                create_effect(move |_| {
                    let analyze = plan_analyze.get();
                    let sql = plan_sql.get_untracked();
                    if !sql.trim().is_empty() {
                        (run_plan)(sql, analyze);
                    }
                });
            }

            // Statements that write can't be safely ANALYZE'd (it would execute
            // them), so the toggle is offered only for read-only statements.
            let is_write = schemaic_core::sql::contains_write(&plan_sql.get_untracked());

            let analyze_control = if is_write {
                text("Analyze unavailable (statement writes data)")
                    .style(|s| s.font_size(FONT_LABEL).color(theme::text_faint()))
                    .into_any()
            } else {
                h_stack((
                    themed_toggle(plan_analyze),
                    v_stack((
                        text("Analyze").style(|s| s.font_size(FONT_BODY).color(theme::text())),
                        text("Executes the statement to measure real timings")
                            .style(|s| s.font_size(FONT_LABEL).color(theme::text_faint())),
                    ))
                    .style(|s| s.flex_col().gap(1.0)),
                ))
                .style(|s| s.items_center().gap(10.0))
                .into_any()
            };

            let toolbar = h_stack((
                analyze_control,
                empty().style(|s| s.flex_grow(1.0_f32)),
                ask_ai_button(
                    plan_state,
                    plan_sql,
                    ai_send.clone(),
                    right_panel,
                    close.clone(),
                ),
            ))
            .style(|s| {
                s.items_center()
                    .width_full()
                    .gap(12.0)
                    .margin_top(10.0)
                    .padding_horiz(14.0)
                    .padding_vert(8.0)
            });

            let body = dyn_container(
                move || plan_state.get(),
                move |st| match st {
                    PlanState::Idle | PlanState::Running => {
                        container(loading_dots("Explaining", theme::text_dim, FONT_BODY))
                            .style(|s| s.height(180.0).width_full().items_center().justify_center())
                            .into_any()
                    }
                    PlanState::Failed(e) => {
                        autohide(scroll(text(e).style(|s| {
                            s.color(theme::error()).font_size(FONT_BODY).padding(16.0)
                        })))
                        .style(|s| s.width_full().max_height(520.0))
                        .into_any()
                    }
                    PlanState::Loaded(plan) => loaded_body(&plan).into_any(),
                },
            )
            .style(|s| s.width_full().flex_col());

            let panel = v_stack((
                modal_title_borderless("Query plan", close.clone()),
                body,
                toolbar,
            ))
            .on_click_stop(|_| {})
            .style(|s| panel_style(s).background(theme::bg_panel()).width(760.0));

            let esc = close.clone();
            container(panel)
                .keyboard_navigable()
                .request_focus(|| {})
                .on_key_down(Key::Named(NamedKey::Escape), |_| true, move |_| esc())
                .on_click_stop(move |_| close())
                .style(|s| {
                    s.size_full()
                        .items_center()
                        .justify_center()
                        .background(theme::modal_backdrop())
                })
                .into_any()
        },
    )
    .style(move |s| {
        if plan_open.get() {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}

/// The loaded plan: warnings (if any) above the plan table.
fn loaded_body(plan: &QueryPlan) -> AnyView {
    let warnings = plan_warnings(plan);
    let table = plan_table(plan);
    let content = v_stack((warnings, table)).style(|s| s.flex_col().width_full().padding(14.0));
    autohide(scroll(content))
        .style(|s| s.width_full().max_height(520.0))
        .into_any()
}

/// The heuristic-warning list (empty view when the plan is clean).
fn plan_warnings(plan: &QueryPlan) -> AnyView {
    if plan.warnings.is_empty() {
        return empty().into_any();
    }
    let rows: Vec<_> = plan
        .warnings
        .iter()
        .map(|w| {
            let kind_color: fn() -> floem::peniko::Color = match w.kind {
                PlanWarningKind::FullScan => theme::error,
                _ => theme::plan_warn,
            };
            h_stack((
                icons::icon(icons::TRIANGLE_ALERT, 15.0)
                    .style(move |s| s.color(kind_color()).flex_shrink(0.0_f32)),
                text(w.message.clone()).style(|s| s.font_size(FONT_BODY).color(theme::text())),
            ))
            .style(|s| s.items_center().gap(8.0))
            .into_any()
        })
        .collect();

    v_stack((
        text("Potential issues")
            .style(|s| s.font_size(FONT_LABEL).font_bold().color(theme::text_dim())),
        v_stack_from_iter(rows).style(|s| s.flex_col().gap(6.0)),
    ))
    .style(|s| {
        s.flex_col()
            .gap(8.0)
            .width_full()
            .margin_bottom(14.0)
            .padding(12.0)
            .border(1.0)
            .border_color(theme::border())
            .border_radius(8.0)
            .background(theme::plan_warn_bg())
    })
    .into_any()
}

/// Render the plan as a table. Column widths are measured from content (capped)
/// so cells align across rows; flagged rows get an amber tint.
fn plan_table(plan: &QueryPlan) -> AnyView {
    // `PAD` covers the 10px horizontal padding each side; `GUARD` adds a few px of
    // slack over the pixel-exact text measurement so the last character never wraps
    // on a sub-pixel rounding (the columns "wrapping by one character" bug).
    const PAD: f64 = 20.0;
    const GUARD: f64 = 5.0;
    const CAP: f64 = 360.0;
    const MIN: f64 = 34.0;

    let ncols = plan.columns.len();
    let mut widths = vec![0.0f64; ncols];
    for (i, h) in plan.columns.iter().enumerate() {
        widths[i] = measure_px(h, FONT_BODY);
    }
    for row in &plan.rows {
        for (i, cell) in row.iter().enumerate() {
            if i < ncols {
                widths[i] = widths[i].max(measure_px(cell, FONT_BODY));
            }
        }
    }
    for w in widths.iter_mut() {
        *w = (*w + PAD + GUARD).clamp(MIN, CAP);
    }

    let flagged: HashSet<usize> = plan.warnings.iter().map(|w| w.row).collect();
    let last_row = plan.rows.len().saturating_sub(1);

    // Header row.
    let header_cells: Vec<_> = plan
        .columns
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let w = widths[i];
            let last_col = i + 1 == ncols;
            text(h.clone())
                .style(move |s| {
                    s.width(w)
                        .flex_shrink(0.0_f32)
                        .padding_horiz(10.0)
                        .padding_vert(6.0)
                        .font_size(FONT_LABEL)
                        .font_bold()
                        .color(theme::text_dim())
                        .border_right(if last_col { 0.0 } else { 1.0 })
                        .border_color(theme::border())
                })
                .into_any()
        })
        .collect();
    // No `items_center`: the default `items_stretch` makes each cell fill the row's
    // full height, so its right-border spans the whole cell (a wrapped multi-line
    // value would otherwise leave gaps in the shorter cells' borders).
    let header = h_stack_from_iter(header_cells).style(|s| {
        s.border_bottom(1.0)
            .border_color(theme::border())
            .background(theme::bg_editor())
    });

    // Data rows.
    let body_rows: Vec<_> = plan
        .rows
        .iter()
        .enumerate()
        .map(|(ri, row)| {
            let is_flagged = flagged.contains(&ri);
            let cells: Vec<_> = (0..ncols)
                .map(|ci| {
                    let w = widths[ci];
                    let last_col = ci + 1 == ncols;
                    let val = row.get(ci).cloned().unwrap_or_default();
                    text(val)
                        .style(move |s| {
                            s.width(w)
                                .flex_shrink(0.0_f32)
                                .padding_horiz(10.0)
                                .padding_vert(6.0)
                                .font_size(FONT_BODY)
                                .color(theme::text())
                                .border_right(if last_col { 0.0 } else { 1.0 })
                                .border_color(theme::border())
                        })
                        .into_any()
                })
                .collect();
            let is_last = ri == last_row;
            h_stack_from_iter(cells)
                .style(move |s| {
                    // The table container draws the outer bottom border, so the last
                    // row omits its own — otherwise the two stack into a double-thick line.
                    let s = s
                        .border_bottom(if is_last { 0.0 } else { 1.0 })
                        .border_color(theme::border());
                    if is_flagged {
                        s.background(theme::plan_warn_bg())
                    } else {
                        s
                    }
                })
                .into_any()
        })
        .collect();

    // Horizontal scroll so a wide plan (many columns / long `Extra`) doesn't force
    // the modal wider than the window.
    let table =
        v_stack((header, v_stack_from_iter(body_rows).style(|s| s.flex_col()))).style(|s| {
            s.flex_col()
                .border(1.0)
                .border_color(theme::border())
                .border_radius(6.0)
        });
    autohide(shift_hscroll(table))
        .style(|s| s.width_full())
        .into_any()
}

/// The "Ask AI" button: reveals the AI panel and sends the plan + query for
/// optimization suggestions. Dimmed until a plan has loaded.
fn ask_ai_button(
    plan_state: RwSignal<PlanState>,
    plan_sql: RwSignal<String>,
    ai_send: Rc<dyn Fn(String)>,
    right_panel: RwSignal<RightPanel>,
    close: Rc<dyn Fn()>,
) -> impl IntoView {
    h_stack((
        icons::icon(icons::SPARKLES, 15.0).style(|s| s.flex_shrink(0.0_f32)),
        text("Ask AI").style(|s| s.font_size(FONT_BODY)),
    ))
    .style(move |s| {
        let loaded = matches!(plan_state.get(), PlanState::Loaded(_));
        let s = s
            .items_center()
            .gap(8.0)
            .flex_shrink(0.0_f32)
            .padding_horiz(6.0)
            .padding_vert(4.0)
            .color(theme::key_foreign());
        if loaded {
            s.hover(|s| s.color(theme::text()))
        } else {
            s.color(theme::key_foreign().multiply_alpha(0.4))
        }
    })
    .on_click_stop(move |_| {
        let PlanState::Loaded(plan) = plan_state.get_untracked() else {
            return;
        };
        let sql = plan_sql.get_untracked();
        let msg = format!(
            "Here is the MySQL/MariaDB EXPLAIN plan for a query. Explain what it's \
             doing, then suggest concrete optimizations — indexes to add, or query \
             rewrites — to make it faster.\n\nQuery:\n```sql\n{sql}\n```\n\nEXPLAIN \
             output:\n```\n{}\n```",
            plan.to_prompt_text()
        );
        if !matches!(right_panel.get_untracked(), RightPanel::Ai) {
            right_panel.set(RightPanel::Ai);
        }
        (ai_send)(msg);
        (close)();
    })
}
