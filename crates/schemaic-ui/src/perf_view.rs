//! The **Performance** view: why a server is slow, beside Server Activity's
//! who-is-running — the statements it has spent most on, the indexes worth a
//! look, and the largest tables and indexes, for the active connection and
//! database ([`schemaic_core::workload`]).
//!
//! Opened from the SCHEMA gear and the palette by setting `overlay.perf`; an
//! effect asks the app for the list showing, which lands in
//! `overlay.perf_state`. Each of the three lists is its own fetch, as the
//! Server variables viewer's two are.
//!
//! **An absent source is said, never drawn as an empty list.** The statement
//! sources can each be off (`pg_stat_statements` not installed or not
//! preloaded, `performance_schema` off, Query Store off), and the view names
//! which, with the one statement that turns it on where one does — shown, for
//! the user to run in the editor, never run from here.
//!
//! **Nothing here writes.** *Create index* on a missing-index suggestion and
//! *Drop* on an index finding each go through the table's draft, `ddl::diff`
//! and the DDL preview ([`crate::table_designer::preview_draft_edit`]), the
//! path every index edit takes, so the user reads the statement and applies
//! it there. The view steps aside while that preview is up: the preview is
//! painted in an earlier group, and this backdrop over it would swallow Apply.

use std::rc::Rc;

use floem::AnyView;
use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;
use floem::reactive::create_effect;

use schemaic_core::workload::{
    IndexFinding, IndexProblem, MissingIndex, SizeEntry, StatementOrder, StatementReport,
    StatementStat, Unavailable,
};

use crate::consts::MONO_FAMILY;
use crate::i18n::{self, tr};
use crate::theme::{font_body, font_hint};
use crate::tooltip::TooltipExt;
use crate::widgets::{
    ACTION_TAB, ActionKind, FocusRing, action_button, action_gap, autohide, dismiss_layer,
    focus_root_with_ring, modal_footer_split, modal_pad_h, modal_title_owned, modal_w, panel_style,
};
use crate::{
    ConnUi, DdlUi, OverlayUi, PerfData, PerfState, PerfTarget, PerfView, SchemaUi, icons, theme,
};

/// Wide enough that a statement's first two lines carry its `FROM` and most
/// of its `WHERE`.
fn panel_w() -> f64 {
    modal_w(860.0)
}

/// Fixed, for `server_vars_view::PANEL_H`'s reason.
const PANEL_H: f64 = 600.0;

/// How long a read runs before the view says it is reading. Below this an
/// answer reads as instant, and a line shown for it is only a flash.
const LOADING_DELAY: std::time::Duration = std::time::Duration::from_millis(250);

// ── what the view reads ──────────────────────────────────────────────────────

/// Everything the view reaches out of the root bundle — built at its one call
/// site, for `whole_ui_gate`'s reason.
#[derive(Clone)]
pub(crate) struct PerfCtx {
    overlay: OverlayUi,
    conn: ConnUi,
    schema: SchemaUi,
    ddl: DdlUi,
    fetch: Rc<dyn Fn(PerfTarget, PerfView, StatementOrder)>,
}

impl PerfCtx {
    pub(crate) fn new(
        overlay: OverlayUi,
        conn: ConnUi,
        schema: SchemaUi,
        ddl: DdlUi,
        actions: &crate::SchemaActions,
    ) -> Self {
        Self {
            overlay,
            conn,
            schema,
            ddl,
            fetch: actions.perf.clone(),
        }
    }
}

// ── opening ──────────────────────────────────────────────────────────────────

/// Open the view on the active connection and `database`. Every signal it
/// reads is reset here, so a second opening never flashes the last one's
/// list (`server_vars_view::open_for_server`'s rule). A no-op with no
/// connection selected.
pub(crate) fn open_for_active(conn: ConnUi, overlay: OverlayUi, database: Option<String>) {
    let id = conn.active_conn.get_untracked();
    let Some((name, dialect)) = conn.connections.with_untracked(|cs| {
        schemaic_core::connection::by_id(cs, id).map(|c| {
            (
                c.name.clone(),
                schemaic_core::intel::SqlDialect::from_db_type(&c.db_type),
            )
        })
    }) else {
        return;
    };
    overlay.perf_state.set(PerfState::Loading);
    overlay.perf_view.set(PerfView::Statements);
    overlay.perf_order.set(StatementOrder::Total);
    overlay.perf.set(Some(PerfTarget {
        conn_id: id,
        name,
        database: database.filter(|d| !d.is_empty()),
        dialect,
    }));
}

pub(crate) fn perf_overlay(ctx: PerfCtx) -> impl IntoView {
    let target = ctx.overlay.perf;
    let view = ctx.overlay.perf_view;
    let order = ctx.overlay.perf_order;
    let state = ctx.overlay.perf_state;
    let preview_open = ctx.ddl.preview;

    // **The loading line waits before it shows.** A local server answers a
    // tab switch inside a frame, so a line drawn the instant the state turned
    // `Loading` flashed for one frame in the body and the footer on every
    // switch — read as a flicker, not as news. It appears only once a read has
    // run for `LOADING_DELAY`, so a fast server never shows it and a slow one
    // still says it is working. A sequence number retires a timer a later
    // state superseded — `dividers::DelayedHover`'s rule — and both signals
    // are made here, in the overlay's stable scope rather than the per-open
    // `dyn_container`, so a timer out at a close never reads a disposed one.
    let slow = RwSignal::new(false);
    {
        let seq: RwSignal<u64> = RwSignal::new(0);
        create_effect(move |_| {
            let loading = state.with(|s| matches!(s, PerfState::Loading));
            let mine = seq.get_untracked().wrapping_add(1);
            seq.set(mine);
            if slow.get_untracked() {
                slow.set(false);
            }
            if loading {
                floem::action::exec_after(LOADING_DELAY, move |_| {
                    if seq.try_get_untracked() == Some(mine) {
                        slow.set(true);
                    }
                });
            }
        });
    }

    // The fetch keys on the target, the list and the order, above the
    // container for `server_vars_view`'s reason. The order only matters to
    // the statements, so a change of it on another list fetches nothing.
    {
        let fetch = ctx.fetch.clone();
        create_effect(
            move |prev: Option<(Option<PerfTarget>, PerfView, StatementOrder)>| {
                let now = (target.get(), view.get(), order.get());
                let changed = match &prev {
                    None => true,
                    Some(p) => {
                        p.0 != now.0
                            || p.1 != now.1
                            || (now.1 == PerfView::Statements && p.2 != now.2)
                    }
                };
                if changed && let Some(t) = now.0.clone() {
                    // Indexes and sizes are one database's; with none chosen
                    // there is nothing to ask, and the view says so.
                    if now.1 != PerfView::Statements && t.database.is_none() {
                        state.set(PerfState::Failed(tr!("perf-no-database")));
                    } else {
                        state.set(PerfState::Loading);
                        (fetch)(t, now.1, now.2);
                    }
                }
                now
            },
        );
    }

    let body_ctx = ctx.clone();
    dyn_container(
        move || target.get(),
        move |open| {
            let Some(t) = open else {
                return empty().into_any();
            };
            modal(body_ctx.clone(), t, slow)
        },
    )
    // **Hidden while the preview it raised is up** — `copy_table_view`'s rule.
    .style(move |s| {
        if target.with(Option::is_some) && preview_open.with(Option::is_none) {
            s.absolute().inset(0.0)
        } else {
            s.hide()
        }
    })
}

fn modal(ctx: PerfCtx, t: PerfTarget, slow: RwSignal<bool>) -> AnyView {
    let target = ctx.overlay.perf;
    let view = ctx.overlay.perf_view;
    let order = ctx.overlay.perf_order;
    let state = ctx.overlay.perf_state;
    let close: Rc<dyn Fn()> = Rc::new(move || {
        target.set(None);
        state.set(PerfState::Loading);
    });
    let ring = FocusRing::new();

    let title = modal_title_owned(
        match &t.database {
            Some(db) => tr!(
                "perf-title-database",
                connection = t.name.clone(),
                database = db.clone()
            ),
            None => tr!("perf-title", connection = t.name.clone()),
        },
        close.clone(),
        ring.clone(),
    );

    let strip = h_stack((
        view_switch(view),
        empty().style(|s| s.flex_grow(1.0_f32)),
        dyn_container(
            move || view.get() == PerfView::Statements,
            move |on| {
                if on {
                    order_switch(order).into_any()
                } else {
                    empty().into_any()
                }
            },
        ),
    ))
    .style(|s| {
        s.width_full()
            .flex_shrink(0.0_f32)
            .items_center()
            .padding_horiz(modal_pad_h())
            .padding_vert(theme::scaled(8.0))
            .border_bottom(1.0)
            .border_color(theme::border())
    });

    let rows_ctx = ctx.clone();
    let rows_target = t.clone();
    let list = autohide(
        scroll(
            dyn_container(
                move || state.get(),
                move |st| body(&rows_ctx, &rows_target, st, slow),
            )
            .style(|s| s.width_full().flex_col()),
        )
        .style(|s| {
            s.width_full()
                .flex_grow(1.0_f32)
                .min_height(0.0)
                .padding_top(theme::scaled(6.0))
        }),
    );

    let refresh = {
        let fetch = ctx.fetch.clone();
        let t = t.clone();
        move || {
            let v = view.get_untracked();
            // The opening effect's rule, for the same reason.
            if v != PerfView::Statements && t.database.is_none() {
                state.set(PerfState::Failed(tr!("perf-no-database")));
                return;
            }
            state.set(PerfState::Loading);
            (fetch)(t.clone(), v, order.get_untracked());
        }
    };
    let footer = footer(slow, refresh, close.clone(), ring.clone());

    let panel = v_stack((title, strip, list, footer))
        .on_click_stop(|_| {})
        .style(|s| {
            panel_style(s)
                .background(theme::bg_panel())
                .width(panel_w())
                .height(crate::widgets::modal_h(PANEL_H))
                .flex_col()
        });

    let esc = close.clone();
    focus_root_with_ring(stack((dismiss_layer(move || close()), panel)), ring)
        .on_key_down(Key::Named(NamedKey::Escape), |_| true, move |_| esc())
        .style(|s| {
            s.size_full()
                .items_center()
                .justify_center()
                .background(theme::modal_backdrop())
        })
        .into_any()
}

/// A row of pill tabs over `current`, the table designer's
/// ([`crate::widgets::pill_tab_style`]). A switch is a fetch, which the
/// modal's effect makes.
fn pills<T: Copy + PartialEq + 'static>(
    current: RwSignal<T>,
    tabs: Vec<(String, T)>,
) -> impl IntoView {
    let tabs = tabs.into_iter().map(move |(label, which)| {
        text(label)
            .on_click_stop(move |_| {
                if current.get_untracked() != which {
                    current.set(which);
                }
            })
            .style(move |s| crate::widgets::pill_tab_style(s, current.get() == which))
    });
    h_stack_from_iter(tabs).style(|s| s.items_center().gap(theme::scaled(4.0)))
}

fn view_switch(view: RwSignal<PerfView>) -> impl IntoView {
    pills(
        view,
        vec![
            (tr!("perf-view-statements"), PerfView::Statements),
            (tr!("perf-view-indexes"), PerfView::Indexes),
            (tr!("perf-view-sizes"), PerfView::Sizes),
        ],
    )
}

fn order_switch(order: RwSignal<StatementOrder>) -> impl IntoView {
    pills(
        order,
        vec![
            (tr!("perf-order-total"), StatementOrder::Total),
            (tr!("perf-order-mean"), StatementOrder::Mean),
            (tr!("perf-order-calls"), StatementOrder::Calls),
        ],
    )
}

// ── the body ─────────────────────────────────────────────────────────────────

fn body(ctx: &PerfCtx, t: &PerfTarget, st: PerfState, slow: RwSignal<bool>) -> AnyView {
    match st {
        // Blank until `slow` says the read is taking a while.
        PerfState::Loading => dyn_container(
            move || slow.get(),
            |slow| {
                if slow {
                    message(icons::CLOCK, theme::text_faint, tr!("perf-loading"))
                } else {
                    empty().into_any()
                }
            },
        )
        .style(|s| s.width_full())
        .into_any(),
        PerfState::Failed(e) => message(icons::TRIANGLE_ALERT, theme::error, e),
        PerfState::Loaded(data) => match &*data {
            PerfData::Statements(StatementReport::Unavailable(u)) => unavailable(u),
            PerfData::Statements(StatementReport::Ready { statements, since }) => {
                statements_list(ctx, statements, since.as_deref())
            }
            PerfData::Indexes {
                findings,
                counted,
                missing,
                missing_error,
            } => indexes_list(
                ctx,
                t,
                findings,
                *counted,
                missing,
                missing_error.as_deref(),
            ),
            PerfData::Sizes {
                tables,
                indexes,
                per_table,
            } => sizes_list(tables, indexes, *per_table),
        },
    }
}

/// What a missing source says: the reason in the interface's words, then the
/// statement that would turn it on, where one does, with a Copy beside it.
fn unavailable(u: &Unavailable) -> AnyView {
    let reason = match u {
        Unavailable::NotSupported => tr!("perf-src-not-supported"),
        Unavailable::ExtensionNotInstalled => tr!("perf-src-pgss-not-installed"),
        Unavailable::ExtensionNotLoaded => tr!("perf-src-pgss-not-loaded"),
        Unavailable::PerformanceSchemaOff => tr!("perf-src-ps-off"),
        Unavailable::DigestConsumerOff => tr!("perf-src-digest-off"),
        Unavailable::QueryStoreOff => tr!("perf-src-qs-off"),
        Unavailable::QueryStoreError => tr!("perf-src-qs-error"),
        Unavailable::NoPermission(e) => tr!("perf-src-no-permission", error = e.clone()),
    };
    let mut rows = vec![message(icons::CIRCLE_QUESTION, theme::text_faint, reason)];
    if let Some(sql) = u.enable_sql() {
        let copy = row_icon(icons::COPY, tr!("perf-copy"), move || {
            let _ = floem::Clipboard::set_contents(sql.to_string());
        });
        rows.push(
            v_stack((
                text(tr!("perf-src-enable")).style(|s| {
                    s.font_size(font_hint())
                        .color(theme::text_faint())
                        .margin_bottom(theme::scaled(4.0))
                }),
                h_stack((
                    text(sql).style(|s| {
                        s.font_family(MONO_FAMILY.to_string())
                            .font_size(font_body())
                            .color(theme::text())
                            .min_width(0.0)
                            .flex_grow(1.0_f32)
                    }),
                    copy,
                ))
                .style(|s| s.items_center().gap(theme::scaled(10.0)).width_full()),
            ))
            .style(|s| {
                s.flex_col()
                    .width_full()
                    .padding_horiz(modal_pad_h())
                    .padding_vert(theme::scaled(6.0))
            })
            .into_any(),
        );
    }
    v_stack_from_iter(rows)
        .style(|s| s.flex_col().width_full())
        .into_any()
}

/// A millisecond figure in the interface's units and marks — `took`'s for a
/// millisecond and up, two places under one, where a lookup lives.
fn ms(f: f64) -> String {
    if f >= 1.0 {
        i18n::took(schemaic_core::history::took(f.round() as u64))
    } else {
        tr!("perf-sub-ms", ms = i18n::figure(&format!("{f:.2}")))
    }
}

fn statements_list(ctx: &PerfCtx, v: &[StatementStat], since: Option<&str>) -> AnyView {
    if v.is_empty() {
        return message(
            icons::CIRCLE_QUESTION,
            theme::text_faint,
            tr!("perf-statements-none"),
        );
    }
    let mut rows: Vec<AnyView> = v.iter().map(|s| statement_row(ctx, s)).collect();
    if let Some(since) = since {
        rows.push(message(
            icons::CLOCK,
            theme::text_faint,
            tr!("perf-since", since = since.to_string()),
        ));
    }
    v_stack_from_iter(rows)
        .style(|s| s.flex_col().width_full())
        .into_any()
}

/// One statement: its text, two lines then clipped, and its figures under it.
/// Right-click copies the whole text — the one thing to do with it here.
fn statement_row(ctx: &PerfCtx, s: &StatementStat) -> AnyView {
    let figures = {
        let mut parts = vec![
            tr!("perf-total", time = ms(s.total_ms)),
            tr!("perf-mean", time = ms(s.mean_ms)),
            tr!(
                "perf-calls",
                n = s.calls,
                shown = i18n::count(s.calls as usize)
            ),
        ];
        if let Some(r) = s.rows {
            parts.push(tr!("perf-rows", n = r, shown = i18n::count(r as usize)));
        }
        if let Some(db) = &s.database {
            parts.push(db.clone());
        }
        parts.join(" \u{00b7} ")
    };
    let sql = s.query.clone();
    let overlay = ctx.overlay;
    v_stack((
        text(schemaic_core::history::preview(&s.query)).style(|s| {
            s.font_family(MONO_FAMILY.to_string())
                .font_size(font_body())
                .color(theme::text())
                .width_full()
                .max_height((font_body() as f64) * 1.4 * 2.0)
                .line_height(1.4)
        }),
        text(figures).style(|s| s.font_size(font_hint()).color(theme::text_faint())),
    ))
    .clip()
    .on_secondary_click_stop(move |_| {
        let sql = sql.clone();
        overlay.popup_anchor.set(None);
        overlay
            .popup_menu
            .set(Some(vec![crate::widgets::MenuEntry::action(
                tr!("perf-copy-statement"),
                move || {
                    let _ = floem::Clipboard::set_contents(sql.clone());
                },
            )]));
    })
    .style(row_style)
    .into_any()
}

fn row_style(s: floem::style::Style) -> floem::style::Style {
    s.flex_col()
        .width_full()
        .gap(theme::scaled(3.0))
        .padding_horiz(modal_pad_h())
        .padding_vert(theme::scaled(7.0))
        .border_bottom(1.0)
        .border_color(theme::border())
        .hover(|s| s.background(theme::row_hover_soft()))
}

fn section(label: String) -> AnyView {
    text(label)
        .style(|s| {
            s.font_size(font_hint())
                .font_bold()
                .color(theme::text_dim())
                .padding_horiz(modal_pad_h())
                .padding_top(theme::scaled(12.0))
                .padding_bottom(theme::scaled(4.0))
        })
        .into_any()
}

fn table_name(schema: Option<&str>, table: &str) -> String {
    schemaic_core::schema::display_name(schema, table)
}

/// An icon button inside a row, with what it does in a tooltip, reachable
/// from the keyboard ([`crate::widgets::key_pressable`]).
///
/// **The list editors' face** (`table_designer`'s row buttons): a 15px glyph
/// in 5px of padding that is the hitbox, no background, the colour going from
/// dim to full on hover — colour being the whole affordance, as for every icon
/// button in the app. The face carries no click listener of its own:
/// `key_pressable` wraps it in the one that answers both the pointer and the
/// keyboard, and a second would fire twice.
fn row_icon(glyph: &'static str, tip: String, on_press: impl Fn() + 'static) -> AnyView {
    let face = container(icons::icon(glyph, 15.0))
        .style(|s| {
            s.padding(theme::scaled(5.0))
                .color(theme::text_dim())
                .hover(|s| s.color(theme::text()))
        })
        .tooltip(move || text(tip.clone()).style(crate::widgets::tooltip_style));
    crate::widgets::key_pressable(face, 4.0, on_press)
}

/// Is `table` loaded in the schema tree, so a draft edit has a model to start
/// from? [`crate::table_designer::preview_draft_edit`] does nothing without
/// one, so its buttons are offered only where it would act — and only on the
/// connection the tree is showing.
fn editable(ctx: &PerfCtx, t: &PerfTarget, schema: Option<&str>, table: &str) -> bool {
    ctx.conn.active_conn.get_untracked() == t.conn_id
        && t.database.as_deref().is_some_and(|db| {
            crate::table_designer::loaded_table(ctx.schema, db, schema, table).is_some()
        })
}

fn indexes_list(
    ctx: &PerfCtx,
    t: &PerfTarget,
    findings: &[IndexFinding],
    counted: bool,
    missing: &[MissingIndex],
    missing_error: Option<&str>,
) -> AnyView {
    let mut rows: Vec<AnyView> = Vec::new();
    if findings.is_empty() {
        rows.push(message(
            icons::CIRCLE_CHECK,
            theme::text_faint,
            tr!("perf-indexes-none"),
        ));
    } else {
        rows.extend(findings.iter().map(|f| finding_row(ctx, t, f)));
    }
    rows.push(message(
        icons::CIRCLE_QUESTION,
        theme::text_faint,
        if counted {
            tr!("perf-usage-note")
        } else {
            tr!("perf-usage-uncounted")
        },
    ));
    if schemaic_core::workload::supports_missing_index_hints(t.dialect) {
        rows.push(section(tr!("perf-missing-title")));
        match missing_error {
            Some(e) => rows.push(message(icons::TRIANGLE_ALERT, theme::error, e.to_string())),
            None if missing.is_empty() => rows.push(message(
                icons::CIRCLE_CHECK,
                theme::text_faint,
                tr!("perf-missing-none"),
            )),
            None => {
                rows.extend(missing.iter().map(|m| missing_row(ctx, t, m)));
                rows.push(message(
                    icons::CIRCLE_QUESTION,
                    theme::text_faint,
                    tr!("perf-missing-note"),
                ));
            }
        }
    }
    v_stack_from_iter(rows)
        .style(|s| s.flex_col().width_full())
        .into_any()
}

/// One index finding: the index, what is wrong with it, its size, and a
/// *Drop* that previews the `DROP INDEX` the table's draft makes.
fn finding_row(ctx: &PerfCtx, t: &PerfTarget, f: &IndexFinding) -> AnyView {
    let what = match &f.problem {
        IndexProblem::Unused => tr!("perf-problem-unused"),
        IndexProblem::Duplicate { of } => tr!("perf-problem-duplicate", of = of.clone()),
        IndexProblem::Redundant { by } => tr!("perf-problem-redundant", by = by.clone()),
    };
    let detail = match f.bytes {
        Some(b) => format!("{what} \u{00b7} {}", i18n::bytes(b)),
        None => what,
    };
    let left = v_stack((
        text(format!(
            "{}.{}",
            table_name(f.schema.as_deref(), &f.table),
            f.index
        ))
        .style(|s| {
            s.font_family(MONO_FAMILY.to_string())
                .font_size(font_body())
                .color(theme::text())
        }),
        text(detail).style(|s| s.font_size(font_hint()).color(theme::text_faint())),
    ))
    .style(|s| {
        s.flex_col()
            .gap(theme::scaled(3.0))
            .flex_grow(1.0_f32)
            .min_width(0.0)
    });
    let action: AnyView = if editable(ctx, t, f.schema.as_deref(), &f.table) {
        let (conn, schema_ui, ddl) = (ctx.conn, ctx.schema, ctx.ddl);
        let database = t.database.clone().unwrap_or_default();
        let (schema, table, index) = (f.schema.clone(), f.table.clone(), f.index.clone());
        row_icon(icons::TRASH_2, tr!("tree-drop-index"), move || {
            let index = index.clone();
            crate::table_designer::preview_draft_edit(
                conn,
                schema_ui,
                ddl,
                &database,
                schema.as_deref(),
                &table,
                move |d, _| d.indexes.retain(|ix| ix.info.name != index),
            );
        })
    } else {
        empty().into_any()
    };
    h_stack((left, action))
        .style(|s| {
            row_style(s)
                .flex_row()
                .items_center()
                .gap(theme::scaled(12.0))
        })
        .into_any()
}

/// One missing-index suggestion, with a *Create index* that previews the
/// statement `ddl::diff` emits for it.
fn missing_row(ctx: &PerfCtx, t: &PerfTarget, m: &MissingIndex) -> AnyView {
    let key: Vec<&str> = m
        .equality
        .iter()
        .chain(&m.inequality)
        .map(String::as_str)
        .collect();
    let mut head = format!(
        "{} ({})",
        table_name(m.schema.as_deref(), &m.table),
        key.join(", ")
    );
    if !m.include.is_empty() {
        head.push_str(&format!(" INCLUDE ({})", m.include.join(", ")));
    }
    let detail = tr!(
        "perf-missing-detail",
        uses = i18n::count(m.uses as usize),
        impact = i18n::figure(&format!("{:.0}", m.avg_impact))
    );
    let left = v_stack((
        text(head).style(|s| {
            s.font_family(MONO_FAMILY.to_string())
                .font_size(font_body())
                .color(theme::text())
        }),
        text(detail).style(|s| s.font_size(font_hint()).color(theme::text_faint())),
    ))
    .style(|s| {
        s.flex_col()
            .gap(theme::scaled(3.0))
            .flex_grow(1.0_f32)
            .min_width(0.0)
    });
    let action: AnyView = if editable(ctx, t, m.schema.as_deref(), &m.table) {
        let (conn, schema_ui, ddl) = (ctx.conn, ctx.schema, ctx.ddl);
        let database = t.database.clone().unwrap_or_default();
        let m = m.clone();
        row_icon(icons::PLUS, tr!("perf-create-index"), move || {
            // `add_to` is the same edit `workload::suggestion_ddl` makes for
            // the MCP tool, so the two front ends create one index.
            let suggestion = m.clone();
            crate::table_designer::preview_draft_edit(
                conn,
                schema_ui,
                ddl,
                &database,
                m.schema.as_deref(),
                &m.table,
                move |d, _| suggestion.add_to(d),
            );
        })
    } else {
        empty().into_any()
    };
    h_stack((left, action))
        .style(|s| {
            row_style(s)
                .flex_row()
                .items_center()
                .gap(theme::scaled(12.0))
        })
        .into_any()
}

fn sizes_list(tables: &[SizeEntry], indexes: &[SizeEntry], per_table: bool) -> AnyView {
    if tables.is_empty() {
        return message(
            icons::CIRCLE_QUESTION,
            theme::text_faint,
            tr!("perf-sizes-none"),
        );
    }
    let mut rows = vec![section(tr!("perf-sizes-tables"))];
    rows.extend(tables.iter().map(|e| {
        let rows_text = e
            .rows
            .map(|r| tr!("perf-rows-estimate", shown = i18n::count(r as usize)));
        size_row(
            table_name(e.schema.as_deref(), &e.table),
            e.bytes,
            rows_text,
        )
    }));
    rows.push(section(tr!("perf-sizes-indexes")));
    if indexes.is_empty() {
        // Two different empties: an engine that sizes indexes per table, and a
        // database with no indexes to size.
        rows.push(message(
            icons::CIRCLE_QUESTION,
            theme::text_faint,
            if per_table {
                tr!("perf-sizes-no-index-sizes")
            } else {
                tr!("perf-sizes-no-indexes")
            },
        ));
    } else {
        rows.extend(indexes.iter().map(|e| {
            size_row(
                format!(
                    "{}.{}",
                    table_name(e.schema.as_deref(), &e.table),
                    e.index.as_deref().unwrap_or_default()
                ),
                e.bytes,
                None,
            )
        }));
    }
    v_stack_from_iter(rows)
        .style(|s| s.flex_col().width_full())
        .into_any()
}

/// One sized object: its name, then its size (and rows) hard right.
fn size_row(name: String, bytes: u64, extra: Option<String>) -> AnyView {
    let figures = match extra {
        Some(x) => format!("{} \u{00b7} {x}", i18n::bytes(bytes)),
        None => i18n::bytes(bytes),
    };
    h_stack((
        text(name).style(|s| {
            s.font_family(MONO_FAMILY.to_string())
                .font_size(font_body())
                .color(theme::text())
                .min_width(0.0)
                .flex_grow(1.0_f32)
                .text_ellipsis()
        }),
        text(figures).style(|s| {
            s.font_size(font_hint())
                .color(theme::text_dim())
                .flex_shrink(0.0_f32)
        }),
    ))
    .style(|s| {
        s.width_full()
            .items_center()
            .gap(theme::scaled(12.0))
            .padding_horiz(modal_pad_h())
            .padding_vert(theme::scaled(6.0))
            .border_bottom(1.0)
            .border_color(theme::border())
    })
    .into_any()
}

/// An icon-led sentence standing in the list — a state or a note.
fn message(icon: &'static str, color: fn() -> floem::peniko::Color, text: String) -> AnyView {
    container(crate::widgets::fact_note(icon, color, text))
        .style(|s| {
            s.width_full()
                .min_width(0.0)
                .padding_horiz(modal_pad_h())
                .padding_vert(theme::scaled(6.0))
        })
        .into_any()
}

/// Refresh and Close; the left side says nothing the list doesn't.
///
/// `slow` is true only while a read has run past `LOADING_DELAY`.
fn footer(
    slow: RwSignal<bool>,
    refresh: impl Fn() + 'static,
    close: Rc<dyn Fn()>,
    ring: FocusRing,
) -> AnyView {
    let status = label(move || {
        if slow.get() {
            tr!("perf-loading")
        } else {
            String::new()
        }
    })
    .style(|s| {
        s.font_size(font_hint())
            .color(theme::text_faint())
            .min_width(0.0)
            .text_ellipsis()
    });
    modal_footer_split(
        status,
        h_stack((
            action_button(
                tr!("perf-refresh"),
                ActionKind::Quiet,
                true,
                ring.clone(),
                ACTION_TAB,
                refresh,
            ),
            action_button(
                tr!("dialog-close"),
                ActionKind::Neutral,
                true,
                ring,
                ACTION_TAB + 1,
                move || close(),
            ),
        ))
        .style(|s| s.flex_row().items_center().gap(action_gap())),
    )
    .into_any()
}

#[cfg(test)]
mod tests {
    use crate::i18n::tr;

    #[test]
    fn the_counted_figures_read_in_both_numbers() {
        assert_eq!(tr!("perf-calls", n = 1, shown = "1"), "1 call");
        assert_eq!(tr!("perf-calls", n = 40, shown = "40"), "40 calls");
        assert_eq!(tr!("perf-rows", n = 1, shown = "1"), "1 row");
        assert_eq!(tr!("perf-rows", n = 2_000, shown = "2k"), "2k rows");
    }

    #[test]
    fn english_says_every_missing_source_as_core_does() {
        use schemaic_core::workload::Unavailable;
        for (u, key) in [
            (Unavailable::NotSupported, tr!("perf-src-not-supported")),
            (
                Unavailable::ExtensionNotInstalled,
                tr!("perf-src-pgss-not-installed"),
            ),
            (
                Unavailable::ExtensionNotLoaded,
                tr!("perf-src-pgss-not-loaded"),
            ),
            (Unavailable::PerformanceSchemaOff, tr!("perf-src-ps-off")),
            (Unavailable::DigestConsumerOff, tr!("perf-src-digest-off")),
            (Unavailable::QueryStoreOff, tr!("perf-src-qs-off")),
            (Unavailable::QueryStoreError, tr!("perf-src-qs-error")),
        ] {
            assert_eq!(key, u.reason(), "{u:?}");
        }
        assert_eq!(
            tr!("perf-src-no-permission", error = "denied"),
            Unavailable::NoPermission("denied".into()).reason()
        );
    }
}
