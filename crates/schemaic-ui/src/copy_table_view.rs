//! **Copy to** — a table, its structure and its rows, into another database on
//! this connection or on another of the same engine (`core::copy_table`).
//!
//! The modal picks where the copy goes and what it is called, then hands the
//! `CREATE TABLE` to the DDL preview like every other generated DDL — Apply
//! there is where the write guard and the target's read-only flag are asked.
//! The rows follow **only once that plan has committed** (`DdlPreview::then`):
//! the preview closes and this modal comes back counting them, then says how
//! it ended. A copy whose rows fail leaves the new table in place and empty —
//! the rows are one transaction (`Db::copy_rows`), the `CREATE` was its own.
//! With *Copy data* off there are no rows to follow: the copy is the
//! `CREATE` alone, the preview's *Applied* is its report, and this modal
//! closes behind it.

use std::rc::Rc;

use floem::AnyView;
use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;
use floem::reactive::{create_effect, create_memo};

use schemaic_core::copy_table::{self, CopyTarget};
use schemaic_core::schema::display_name;

use crate::widgets::{
    ACTION_TAB, ActionKind, FocusRing, action_button, action_gap, autohide, cursor_row, fact_note,
    focus_root_with_ring, form_gap, form_setting, in_ring_button, modal_footer, modal_pad_h,
    modal_title_owned, modal_w, panel_style, pick_group,
};

/// The chosen target's button, and the list under it — one stop, the
/// arrows moving inside it — ahead of the Schema (20) and Name (30) fields.
const CHOSEN_TAB: u32 = 10;
const LIST_TAB: u32 = 15;
use crate::{ConnUi, DdlUi, FieldCfg, OverlayUi, SchemaUi, edit_field, icons, theme};

/// The table *Copy to* was raised on — always the active connection's, since
/// the tree shows no other.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopySource {
    pub conn_id: u64,
    pub database: String,
    pub schema: Option<String>,
    pub table: String,
}

/// The rows half, as the app runs it (`Db::copy_rows`, then `settle`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyRowsRequest {
    pub src_conn: u64,
    pub src_database: String,
    /// The read on the source — `copy_table::select_sql`.
    pub select: String,
    pub dst_conn: u64,
    pub target: CopyTarget,
    /// The columns `select` reads, in its order — `copy_table::columns`.
    pub columns: Vec<String>,
    /// What runs on the target after the rows — `copy_table::settle_sql`.
    pub settle: Vec<String>,
}

/// How the rows half ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CopyOutcome {
    Done(u64),
    Cancelled,
    /// The whole sentence the modal shows — the app's, since only it knows
    /// whether the rows were in before the failure.
    Failed(String),
}

/// Run the rows half, answering once on the UI thread.
pub type CopyRowsFn = Rc<dyn Fn(CopyRowsRequest, Rc<dyn Fn(CopyOutcome)>)>;

/// Where the modal is: choosing a target, copying rows, or finished.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Stage {
    Choosing,
    Copying,
    Finished(CopyOutcome),
}

/// What the modal needs from the root bundle.
#[derive(Clone)]
pub(crate) struct CopyCtx {
    pub overlay: OverlayUi,
    pub conn: ConnUi,
    pub schema: SchemaUi,
    pub ddl: DdlUi,
    /// List one connection's databases into `overlay.compare_dbs` — the
    /// compare picker's lister and its signal, which this modal borrows: the
    /// two never open together, and it is the same question.
    pub list_dbs: Rc<dyn Fn(u64)>,
    pub copy_rows: CopyRowsFn,
    pub cancel: Rc<dyn Fn()>,
}

fn panel_w() -> f64 {
    modal_w(500.0)
}

pub(crate) fn copy_table_overlay(ctx: CopyCtx) -> impl IntoView {
    let open = ctx.overlay.copy_table;
    let preview_open = ctx.ddl.preview;
    dyn_container(
        move || open.with(Option::is_some),
        move |up| {
            let Some(src) = up.then(|| open.get_untracked()).flatten() else {
                return empty().into_any();
            };
            modal(ctx.clone(), src).into_any()
        },
    )
    // **Hidden while the preview it raised is up**, the compare modal's rule:
    // the preview is painted in an earlier group, and a backdrop of this one's
    // over it would swallow every click meant for Apply.
    .style(move |s| {
        if open.with(Option::is_some) && preview_open.with(Option::is_none) {
            s.absolute().inset(0.0)
        } else {
            s.hide()
        }
    })
}

fn modal(ctx: CopyCtx, src: CopySource) -> impl IntoView {
    let o = ctx.overlay;
    let conns = ctx.conn.connections;
    let stage = RwSignal::new(Stage::Choosing);
    let dst: RwSignal<Option<(u64, String)>> = RwSignal::new(None);
    let name = RwSignal::new(format!("{}_copy", src.table));
    let ns = RwSignal::new(src.schema.clone().unwrap_or_default());
    let refusal: RwSignal<Option<String>> = RwSignal::new(None);
    let picking = RwSignal::new(true);
    // On by default: a copy is the rows too unless asked otherwise. Off makes
    // it the structure alone — the `CREATE TABLE` and nothing after it.
    let copy_data = RwSignal::new(true);
    // *Preview SQL* is offered once there is somewhere to copy to and a name
    // to copy as. A memo, so the footer rebuilds when that flips and not on
    // every keystroke in Name. `preview` still refuses both itself.
    let ready =
        create_memo(move |_| dst.with(Option::is_some) && name.with(|n| !n.trim().is_empty()));
    // A refusal answers the form as it was when Preview was pressed; any
    // change to what it was about makes it stale, so it goes.
    create_effect(move |first: Option<()>| {
        dst.track();
        name.track();
        ns.track();
        if first.is_some() {
            refusal.set(None);
        }
    });
    o.compare_dbs.set(None);
    o.copy_progress.set(0);

    let ring = FocusRing::new();
    let root_ring = ring.clone();
    let close: Rc<dyn Fn()> = {
        let cancel = ctx.cancel.clone();
        Rc::new(move || {
            if stage.get_untracked() == Stage::Copying {
                // Closing mid-copy stops it: the rows are one transaction, so
                // the target is left as the `CREATE` made it.
                cancel();
            }
            o.copy_table.set(None);
        })
    };
    let src_label = display_name(src.schema.as_deref(), &src.table);
    let src_type = conns.with_untracked(|cs| {
        schemaic_core::connection::by_id(cs, src.conn_id).map(|c| c.db_type.clone())
    });

    // ── the target picker: connection, then one of its databases ────────────
    let target_label = move || match dst.get() {
        Some((cid, db)) => {
            let conn =
                conns.with(|cs| schemaic_core::connection::by_id(cs, cid).map(|c| c.name.clone()));
            match conn {
                Some(c) => format!("{c} · {db}"),
                None => db,
            }
        }
        None => String::new(),
    };
    // The choice, once there is one: pressing it opens the list again, or
    // folds it back up. Before a choice there is nothing to show and nothing
    // to fold — the list is the whole question — so there is no button, and
    // the list cannot be closed into an empty field.
    let has_dst = create_memo(move |_| dst.with(Option::is_some));
    let chosen = {
        let ring = ring.clone();
        dyn_container(
            move || has_dst.get(),
            move |shown| {
                if !shown {
                    return empty().into_any();
                }
                in_ring_button(
                    container(label(target_label).style(|s| {
                        s.font_family(crate::consts::MONO_FAMILY.to_string())
                            .font_size(theme::font_body())
                    }))
                    .on_click_stop(move |_| picking.update(|p| *p = !*p)),
                    ring.clone(),
                    CHOSEN_TAB,
                    true,
                    4.0,
                    move || picking.update(|p| *p = !*p),
                )
                .style(|s| s.color(theme::accent()).hover(|s| s.color(theme::text())))
                .into_any()
            },
        )
    };
    // **The list is one Tab stop** (`widgets::pick_group`): Up/Down move a
    // cursor over its rows, Enter or Space picks the one under it. Each step
    // of the picker rebuilds the list, removing the view that held the
    // keyboard, so a pick made from the keyboard hands it on — to the new
    // list, or, once a database is chosen, to the choice above it.
    let cursor: RwSignal<Option<usize>> = RwSignal::new(None);
    let keyed = RwSignal::new(false);
    let list = {
        let list_dbs = ctx.list_dbs.clone();
        let src_type = src_type.clone();
        let ring = ring.clone();
        dyn_container(
            move || (picking.get(), o.compare_dbs.get()),
            move |(up, listed)| {
                cursor.set(None);
                if keyed.get_untracked() {
                    keyed.set(false);
                    let ring = ring.clone();
                    let tab = if up { LIST_TAB } else { CHOSEN_TAB };
                    floem::action::exec_after(std::time::Duration::ZERO, move |_| {
                        ring.focus_at(tab)
                    });
                }
                if !up {
                    return empty().into_any();
                }
                // Each row's label, colour and pick, in the order drawn.
                let mut picks: Vec<(String, fn() -> floem::peniko::Color, Rc<dyn Fn()>)> =
                    Vec::new();
                match listed {
                    Some((cid, dbs)) => {
                        picks.push((
                            "‹ back".to_string(),
                            theme::text_muted,
                            Rc::new(move || o.compare_dbs.set(None)),
                        ));
                        for db in dbs {
                            picks.push((
                                db.clone(),
                                theme::text,
                                Rc::new(move || {
                                    dst.set(Some((cid, db.clone())));
                                    picking.set(false);
                                }),
                            ));
                        }
                    }
                    None => {
                        let same = conns.get().into_iter().filter(|c| {
                            src_type.as_ref().is_some_and(|t| {
                                schemaic_core::connection::same_engine(&c.db_type, t)
                            })
                        });
                        for c in same {
                            let list_dbs = list_dbs.clone();
                            let id = c.id;
                            picks.push((c.name, theme::text, Rc::new(move || (list_dbs)(id))));
                        }
                    }
                }
                let actions: Rc<Vec<Rc<dyn Fn()>>> =
                    Rc::new(picks.iter().map(|(_, _, f)| f.clone()).collect());
                let inner = v_stack_from_iter(picks.into_iter().enumerate().map(
                    |(i, (text, color, f))| {
                        cursor_row(
                            crate::compare_view::pick_row(text, color, move || f()),
                            cursor,
                            i,
                        )
                    },
                ))
                .style(|s| s.flex_col().width_full());
                let len = actions.len();
                let body = autohide(scroll(inner).style(|s| s.width_full())).style(move |s| {
                    // Clear of the chosen target above it; with none shown,
                    // the label's own gap is all the list sits under.
                    s.width_full()
                        .max_height(theme::scaled(180.0))
                        .apply_if(has_dst.get(), |s| s.margin_top(theme::scaled(6.0)))
                        .padding(theme::scaled(4.0))
                        .background(theme::bg_deepest())
                        .border(1.0)
                        .border_color(theme::border())
                        .border_radius(5.0)
                });
                pick_group(
                    body,
                    ring.clone(),
                    LIST_TAB,
                    cursor,
                    move || len,
                    move |i| {
                        keyed.set(true);
                        (actions[i])();
                    },
                )
            },
        )
        .style(|s| s.flex_col().width_full())
    };
    let db_err = dyn_container(
        move || o.compare_dbs_err.get(),
        |err| match err {
            Some(e) => fact_note(icons::TRIANGLE_ALERT, theme::error, e),
            None => empty().into_any(),
        },
    )
    // Off the list it explains, as the list is off the target above it.
    .style(move |s| {
        s.apply_if(o.compare_dbs_err.with(Option::is_some), |s| {
            s.margin_top(theme::scaled(6.0))
        })
    });

    // ── Preview SQL: the plan, into the preview, with the rows to follow ────
    let preview: Rc<dyn Fn()> = {
        let ctx = ctx.clone();
        let src = src.clone();
        Rc::new(move || {
            let Some((dst_conn, database)) = dst.get_untracked() else {
                return refusal.set(Some("Choose the database to copy into.".into()));
            };
            // The schema and the dialect read below are the **active**
            // connection's, which was the source's when this opened. Switched
            // since, they would describe another server's same-named table
            // while the rows still came from this one.
            if ctx.conn.active_conn.get_untracked() != src.conn_id {
                return refusal.set(Some(
                    "The active connection changed — reopen Copy to from the table.".into(),
                ));
            }
            let Some(info) = crate::table_designer::loaded_table(
                ctx.schema,
                &src.database,
                src.schema.as_deref(),
                &src.table,
            ) else {
                return refusal.set(Some(format!(
                    "{}'s schema is not loaded — expand it in the tree, then try again.",
                    src.database
                )));
            };
            let to = CopyTarget {
                database,
                schema: src
                    .schema
                    .as_ref()
                    .map(|_| ns.get_untracked().trim().to_string()),
                name: name.get_untracked().trim().to_string(),
            };
            let same_database = dst_conn == src.conn_id && to.database == src.database;
            if let Some(why) = copy_table::refusal(&info, &to, same_database) {
                return refusal.set(Some(why.to_string()));
            }
            refusal.set(None);
            let dialect = ctx.conn.dialect.get_untracked();
            let cs = copy_table::plan(&info, &to, dialect);
            let read_only = ctx
                .conn
                .connections
                .with_untracked(|cs| schemaic_core::connection::read_only_of(cs, dst_conn));
            let columns = copy_table::columns(&info);
            let req = CopyRowsRequest {
                src_conn: src.conn_id,
                src_database: src.database.clone(),
                select: copy_table::select_sql(&src.database, &info, &columns, dialect),
                dst_conn,
                settle: copy_table::settle_sql(&to, &info, dialect),
                target: to.clone(),
                columns,
            };
            let mut p = crate::ddl_preview::preview_of(
                dst_conn,
                &to.database,
                format!(
                    "Copy of {}",
                    display_name(info.schema.as_deref(), &info.name)
                ),
                &cs,
                read_only,
            );
            if !copy_data.get_untracked() {
                // The structure alone: the preview's own *Applied* is the
                // whole report, so this modal goes rather than coming back to
                // count rows that are not coming.
                p.then = Some(Rc::new(move || o.copy_table.set(None)));
                crate::ddl_preview::open_preview(ctx.ddl, p);
                return;
            }
            // The rows are no statement in the script, so the change list
            // carries them as a line of their own — without it, the preview
            // read as a structure-only copy.
            p.changes.push(copy_table::rows_change(&info));
            let (copy_rows, d) = (ctx.copy_rows.clone(), ctx.ddl);
            p.then = Some(Rc::new(move || {
                // The table is there; the preview has nothing more to say,
                // and this modal is where the rows are counted.
                crate::ddl_preview::close_preview(d);
                o.copy_progress.set(0);
                stage.set(Stage::Copying);
                (copy_rows)(
                    req.clone(),
                    Rc::new(move |outcome| stage.set(Stage::Finished(outcome))),
                );
            }));
            crate::ddl_preview::open_preview(ctx.ddl, p);
        })
    };

    // ── the body, by stage ──────────────────────────────────────────────────
    // Built once and kept: the stage switch below only shows or hides it, so
    // the fields keep their text and their caret while the copy runs.
    let form = {
        let preview = preview.clone();
        let ring = ring.clone();
        let has_schema = src.schema.is_some();
        v_stack((
            form_setting(
                "Copy into",
                v_stack((chosen, list, db_err)).style(|s| s.width_full()),
            ),
            form_setting(
                "Schema",
                edit_field(
                    ns,
                    FieldCfg {
                        focus: Some((ring.clone(), 20)),
                        ..Default::default()
                    },
                )
                .style(|s| s.width_full()),
            )
            .style(move |s| s.apply_if(!has_schema, |s| s.hide())),
            form_setting(
                "Name",
                edit_field(
                    name,
                    FieldCfg {
                        focus: Some((ring.clone(), 30)),
                        on_submit: Some(preview.clone()),
                        ..Default::default()
                    },
                )
                .style(|s| s.width_full()),
            ),
            crate::settings::focusable_toggle_row(
                "Copy data",
                "Copy the rows once the table is created. Off copies the structure only.",
                copy_data,
                ring.clone(),
                40,
            ),
            dyn_container(
                move || copy_data.get(),
                |rows| {
                    fact_note(
                        icons::CIRCLE_DOT,
                        theme::text_dim,
                        if rows {
                            "The structure and the rows. Foreign keys and triggers are not \
                             copied, and a generated column is computed afresh."
                        } else {
                            "The structure only. Foreign keys and triggers are not copied."
                        }
                        .to_string(),
                    )
                },
            ),
            dyn_container(
                move || refusal.get(),
                |why| match why {
                    Some(why) => fact_note(icons::TRIANGLE_ALERT, theme::error, why),
                    None => empty().into_any(),
                },
            ),
        ))
        .style(move |s| {
            let s = s.flex_col().width_full().gap(form_gap());
            if stage.with(|st| *st == Stage::Choosing) {
                s
            } else {
                s.hide()
            }
        })
    };
    let status = dyn_container(
        move || stage.get(),
        move |st| match st {
            Stage::Choosing => empty().into_any(),
            Stage::Copying => label(move || format!("Copying rows… {}", o.copy_progress.get()))
                .style(|s| s.font_size(theme::font_body()).color(theme::text()))
                .into_any(),
            Stage::Finished(CopyOutcome::Done(n)) => fact_note(
                icons::CIRCLE_CHECK,
                theme::text,
                format!("Copied {n} {}.", if n == 1 { "row" } else { "rows" }),
            ),
            Stage::Finished(CopyOutcome::Cancelled) => fact_note(
                icons::TRIANGLE_ALERT,
                theme::text_dim,
                // Not "holds no rows": on a table whose engine ignores the
                // rollback (MyISAM, Aria) what arrived before the stop stays.
                "Stopped before every row was copied. The table was created.".to_string(),
            ),
            // The app's whole sentence: it alone knows whether the rows went
            // in before what failed (a counter left behind) or not at all.
            Stage::Finished(CopyOutcome::Failed(e)) => {
                fact_note(icons::TRIANGLE_ALERT, theme::error, e)
            }
        },
    );
    let body = v_stack((form, status)).style(|s| {
        s.flex_col()
            .width_full()
            .padding_horiz(modal_pad_h())
            .padding_vert(theme::scaled(18.0))
    });

    let footer = {
        let ring = ring.clone();
        let close = close.clone();
        let cancel = ctx.cancel.clone();
        modal_footer(dyn_container(
            move || (stage.get(), ready.get()),
            move |(st, ready)| {
                let close = close.clone();
                let actions: AnyView = match st {
                    Stage::Choosing => {
                        let preview = preview.clone();
                        h_stack((
                            action_button(
                                "Cancel",
                                ActionKind::Neutral,
                                true,
                                ring.clone(),
                                ACTION_TAB,
                                move || close(),
                            ),
                            action_button(
                                "Preview SQL",
                                ActionKind::Primary,
                                ready,
                                ring.clone(),
                                ACTION_TAB + 10,
                                move || preview(),
                            ),
                        ))
                        .into_any()
                    }
                    Stage::Copying => {
                        let cancel = cancel.clone();
                        action_button(
                            "Stop",
                            ActionKind::Neutral,
                            true,
                            ring.clone(),
                            ACTION_TAB,
                            move || cancel(),
                        )
                        .into_any()
                    }
                    Stage::Finished(_) => action_button(
                        "Close",
                        ActionKind::Primary,
                        true,
                        ring.clone(),
                        ACTION_TAB,
                        move || close(),
                    )
                    .into_any(),
                };
                actions.style(|s| s.flex_row().items_center().gap(action_gap()))
            },
        ))
    };

    let panel = v_stack((
        modal_title_owned(
            format!("Copy {src_label}"),
            close.clone(),
            root_ring.clone(),
        ),
        body,
        footer,
    ))
    .on_click_stop(|_| {})
    .style(|s| panel_style(s).width(panel_w()));

    // **No click-away**: a stray click must not close a copy in flight. ✕ and
    // Escape close it, and stop the rows if they are going.
    let esc = close.clone();
    focus_root_with_ring(container(panel), root_ring)
        .on_key_down(Key::Named(NamedKey::Escape), |_| true, move |_| esc())
        .style(|s| {
            s.size_full()
                .flex_col()
                .items_center()
                .justify_center()
                .background(theme::modal_backdrop())
        })
}
