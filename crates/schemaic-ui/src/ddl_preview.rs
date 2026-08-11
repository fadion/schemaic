//! The DDL preview: what is about to change, what it costs, and the exact SQL.
//!
//! **Nothing in Schemaic runs generated DDL without going through here.** The
//! designer, Create table, and every context-menu shortcut all end at this
//! modal, so there's one place that shows the statements and one place that says
//! what they destroy. The escape hatch is deliberate too — "Open in editor"
//! drops the script into a query tab, because a generated `ALTER` the user can't
//! read and adjust is exactly the thing that makes people distrust a schema
//! editor.
//!
//! Applying is the app's job (`SchemaActions::run_ddl`, off the UI thread); this
//! module owns the panel and the outcome states.

use std::rc::Rc;

use floem::AnyView;
use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;

use crate::widgets::{
    ExitAction, autohide, exit_action, footer_button, form_section, modal_footer,
    modal_title_owned, panel_style,
};
use crate::{DdlOutcome, DdlPreview, DdlRunRequest, FieldCfg, Ui, edit_field, icons, theme};

const PANEL_W: f64 = 660.0;
const PANEL_H: f64 = 560.0;
/// The SQL box's height before it scrolls. Deep enough that a typical `ALTER`
/// with a handful of clauses is visible whole.
const SQL_ROWS: usize = 16;

/// Open the preview on a change set. `from_designer` decides where Cancel goes.
pub(crate) fn open_preview(ui: &Ui, preview: DdlPreview) {
    let d = ui.ddl;
    d.sql.set(preview.statements.join("\n\n"));
    d.sql_rows.set(SQL_ROWS);
    d.error.set(None);
    d.applied.set(false);
    d.applying.set(false);
    d.generation.update(|g| *g += 1);
    d.preview.set(Some(preview));
}

/// Build a preview from a change set — the one conversion every caller uses, so
/// the summaries, the warnings and the SQL can't come from different places.
pub(crate) fn preview_of(
    conn_id: u64,
    database: &str,
    subject: impl Into<String>,
    cs: &schemaic_core::ddl::ChangeSet,
    read_only: bool,
) -> DdlPreview {
    DdlPreview {
        conn_id,
        database: database.to_string(),
        subject: subject.into(),
        changes: cs.changes.iter().map(|c| c.summary()).collect(),
        destructive: cs.destructive(),
        statements: cs.emit(),
        read_only,
    }
}

/// Send **one** change straight to the preview, skipping the designer — how
/// every context-menu shortcut works. Same modal, same warnings, same Apply: the
/// shortcut saves the designer, not the review.
pub(crate) fn preview_change(
    ui: &Ui,
    database: &str,
    table: &str,
    schema: Option<&str>,
    change: schemaic_core::ddl::Change,
) {
    let ctx = crate::table_designer::edit_ctx(ui);
    let cs = schemaic_core::ddl::single(table, schema, ctx.dialect, change);
    open_preview(
        ui,
        preview_of(
            ctx.conn_id,
            database,
            schemaic_core::schema::display_name(schema, table),
            &cs,
            ctx.read_only,
        ),
    );
}

/// A bullet line in the change list.
fn change_line(label: String) -> impl IntoView {
    h_stack((
        text("•").style(|s| {
            s.color(theme::text_faint())
                .font_size(theme::FONT_BODY)
                .width(12.0)
                .flex_shrink(0.0_f32)
        }),
        text(label).style(|s| {
            s.color(theme::text())
                .font_size(theme::FONT_BODY)
                .flex_grow(1.0_f32)
                .min_width(0.0)
        }),
    ))
    .style(|s| s.flex_row().items_start().width_full().margin_bottom(3.0))
}

/// The destructive block. Present ⇒ this plan takes something away, and the
/// wording says what rather than "are you sure".
fn risk_block(risks: Vec<String>) -> impl IntoView {
    let empty_block = risks.is_empty();
    v_stack((
        h_stack((
            icons::icon(icons::TRIANGLE_ALERT, 15.0)
                .style(|s| s.color(theme::error()).flex_shrink(0.0_f32)),
            text("This can't be undone").style(|s| {
                s.color(theme::error())
                    .font_size(theme::FONT_BODY)
                    .font_bold()
            }),
        ))
        .style(|s| s.flex_row().items_center().gap(7.0).margin_bottom(5.0)),
        v_stack_from_iter(risks.into_iter().map(|r| {
            text(r).style(|s| {
                s.color(theme::text())
                    .font_size(theme::FONT_BODY)
                    .width_full()
                    .margin_bottom(2.0)
            })
        })),
    ))
    .style(move |s| {
        let s = s
            .flex_col()
            .width_full()
            .padding(10.0)
            .border(1.0)
            .border_color(theme::error())
            .border_radius(6.0)
            .background(theme::error().multiply_alpha(0.08));
        if empty_block { s.hide() } else { s }
    })
}

/// Hand the plan to the app, and fold the outcome back into the modal.
fn apply(ui: Ui) {
    let d = ui.ddl;
    let Some(p) = d.preview.get_untracked() else {
        return;
    };
    if p.read_only || d.applying.get_untracked() {
        return;
    }
    d.applying.set(true);
    d.error.set(None);
    let opened = d.generation.get_untracked();
    (ui.schema_actions.run_ddl)(
        DdlRunRequest {
            conn_id: p.conn_id,
            database: p.database.clone(),
            statements: p.statements.clone(),
        },
        Rc::new(move |res| {
            // The modal was closed and reopened on something else while this ran
            // — reporting into that would claim a different plan succeeded.
            if d.generation.get_untracked() != opened {
                return;
            }
            d.applying.set(false);
            match res {
                DdlOutcome::Applied => {
                    d.applied.set(true);
                    // The draft behind this is now the server's state, so
                    // whichever editor opened it has nothing left to show.
                    d.designer.set(None);
                    d.view.set(None);
                }
                DdlOutcome::Failed(e) => d.error.set(Some(e)),
                // The user answered "Cancel" to a question the apply raised, so
                // the modal goes back to where it was — nothing to report, and
                // an error banner would name a failure that never happened.
                DdlOutcome::Declined => {}
            }
        }),
    );
}

/// The DDL preview modal. Absolutely positioned over the workspace when
/// `ui.ddl.preview` is `Some`.
pub(crate) fn ddl_preview_overlay(ui: Ui) -> impl IntoView {
    let d = ui.ddl;
    // Closing returns to the designer when it's still open behind — the draft is
    // untouched, so Cancel here means "not yet", not "throw it away".
    //
    // While an apply is in flight, every exit refuses. `run_ddl` is handed a
    // fresh token nothing holds, and on MySQL each DDL statement has already
    // committed, so there is nothing to cancel — and `d.error` is the *only*
    // reader of "statement 3 of 5 failed, 2 already stuck". Closing would leave
    // a half-migrated table, a stale schema tree and no indication at all. The
    // footer button was already disabled for this reason; Escape and the ✕
    // weren't.
    let exit = move || {
        if exit_action(d.applying.get_untracked(), false) == ExitAction::Close {
            d.preview.set(None);
        }
    };

    dyn_container(
        move || (d.preview.get().is_some(), d.applied.get()),
        move |(open, applied)| {
            if !open {
                return empty().into_any();
            }
            let ui = ui.clone();
            let Some(p) = d.preview.get_untracked() else {
                return empty().into_any();
            };

            let body: AnyView = if applied {
                container(
                    v_stack((
                        text(format!(
                            "Applied {} statement{} to {}.",
                            p.statements.len(),
                            if p.statements.len() == 1 { "" } else { "s" },
                            p.subject
                        ))
                        .style(|s| s.color(theme::text()).font_size(theme::FONT_BODY)),
                        text("The schema has been refreshed.").style(|s| {
                            s.color(theme::text_dim())
                                .font_size(theme::FONT_LABEL)
                                .margin_top(6.0)
                        }),
                    ))
                    .style(|s| s.flex_col()),
                )
                .style(|s| s.padding_vert(10.0))
                .into_any()
            } else {
                let n = p.changes.len();
                v_stack((
                    form_section("Changes"),
                    v_stack_from_iter(p.changes.iter().cloned().map(change_line))
                        .style(|s| s.flex_col().width_full()),
                    text(format!("{n} change{}", if n == 1 { "" } else { "s" })).style(|s| {
                        s.color(theme::text_faint())
                            .font_size(theme::FONT_LABEL)
                            .margin_top(2.0)
                    }),
                    risk_block(p.destructive.clone()).style(|s| s.margin_top(14.0)),
                    form_section("SQL").style(|s| s.margin_top(18.0)),
                    // Read-only, but a real editor field: the script is meant to
                    // be read and selected, and it's the same widget the rest of
                    // the app uses for text. Monospace, because this is the one
                    // place the user reads generated SQL closely — aligned
                    // columns are how a stray clause gets spotted before Apply.
                    edit_field(
                        d.sql,
                        FieldCfg {
                            multiline: true,
                            no_wrap: true,
                            read_only: true,
                            mono: true,
                            font_size: theme::FONT_BODY,
                            max_rows: Some(d.sql_rows),
                            ..Default::default()
                        },
                    )
                    .style(|s| s.width_full()),
                ))
                .style(|s| s.flex_col().gap(8.0).width_full())
                .into_any()
            };

            let err = dyn_container(
                move || d.error.get(),
                move |e| match e {
                    None => empty().into_any(),
                    Some(e) => text(e)
                        .style(|s| {
                            s.color(theme::error())
                                .font_size(theme::FONT_BODY)
                                .max_width(580.0)
                                .margin_top(12.0)
                        })
                        .into_any(),
                },
            );

            let actions = dyn_container(
                move || (d.applying.get(), d.applied.get()),
                move |(busy, applied)| {
                    let ui = ui.clone();
                    if applied {
                        return footer_button(
                            "Close",
                            theme::conn_save,
                            theme::conn_save_hover,
                            true,
                            exit,
                        )
                        .into_any();
                    }
                    let p = match d.preview.get_untracked() {
                        Some(p) => p,
                        None => return empty().into_any(),
                    };
                    let sql = p.statements.join("\n\n");
                    let open_sql = sql.clone();
                    let open_query = ui.tab_actions.open_query.clone();
                    h_stack((
                        // "Back" only when there's somewhere to go back *to*. A
                        // context-menu shortcut opens this modal with nothing
                        // behind it, where Back would point at nowhere.
                        footer_button(
                            if d.designer.get_untracked().is_some()
                                || d.view.get_untracked().is_some()
                            {
                                "Back"
                            } else {
                                "Cancel"
                            },
                            theme::text_dim,
                            theme::text,
                            !busy,
                            exit,
                        ),
                        footer_button("Copy", theme::text_dim, theme::text, !busy, move || {
                            let _ = floem::Clipboard::set_contents(sql.clone());
                        }),
                        // The escape hatch: the generated script, in a tab, where
                        // it can be read, edited and run like anything else.
                        footer_button(
                            "Open in editor",
                            theme::text_dim,
                            theme::text,
                            !busy,
                            move || {
                                (open_query)(open_sql.clone());
                                d.preview.set(None);
                                d.designer.set(None);
                                d.view.set(None);
                            },
                        ),
                        footer_button(
                            if busy { "Applying…" } else { "Apply" },
                            // A destructive plan's affirmative action wears the
                            // colour of what it does — the *same* red pair as the
                            // confirm modal's Yes, since it answers the same
                            // question and the two should always retune together.
                            // (It was `theme::error` for both resting and hover,
                            // which is a red with no hover at all.)
                            if p.destructive.is_empty() {
                                theme::conn_save
                            } else {
                                theme::confirm_yes
                            },
                            if p.destructive.is_empty() {
                                theme::conn_save_hover
                            } else {
                                theme::confirm_yes_hover
                            },
                            !busy && !p.read_only && !p.statements.is_empty(),
                            move || apply(ui.clone()),
                        ),
                    ))
                    .style(|s| s.flex_row().items_center().gap(15.0))
                    .into_any()
                },
            );

            // A read-only connection blocks the write, and says so where the
            // disabled button is rather than leaving it unexplained.
            let read_only_note = text("This connection is read-only.").style(move |s| {
                let s = s
                    .color(theme::plan_warn())
                    .font_size(theme::FONT_LABEL)
                    .margin_right(12.0);
                if d.preview.get().is_some_and(|p| p.read_only) && !d.applied.get() {
                    s
                } else {
                    s.hide()
                }
            });

            let close_x: Rc<dyn Fn()> = Rc::new(exit);
            let panel = v_stack((
                modal_title_owned(format!("Apply changes to {}", p.subject), close_x),
                autohide(scroll(v_stack((body, err)).style(|s| {
                    s.flex_col()
                        .width_full()
                        .padding_horiz(20.0)
                        .padding_vert(18.0)
                })))
                .style(|s| s.width_full().flex_grow(1.0_f32).min_height(0.0)),
                modal_footer(
                    h_stack((read_only_note, actions)).style(|s| s.flex_row().items_center()),
                ),
            ))
            .on_click_stop(|_| {})
            .style(|s| panel_style(s).width(PANEL_W).height(PANEL_H));

            container(panel)
                .keyboard_navigable()
                .request_focus(|| {})
                .on_key_down(Key::Named(NamedKey::Escape), |_| true, move |_| exit())
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
        if d.preview.get().is_some() {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}
