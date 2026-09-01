//! The script-load modal — **Import** to a user, opened from a database's or a
//! PostgreSQL namespace's context menu, directly above the *Export* it is the
//! inverse of.
//!
//! One word, two scopes: *Import* on a **table** is the CSV/JSON loader in
//! [`crate::import_view`], and *Import* on a **database** is this — a `.sql`
//! script, which has no table to load into because its statements name their
//! own. They share the entry point and the frame rather than the state; see
//! [`crate::ScriptUi`] for why folding them into one bundle was the worse trade.
//!
//! The shape follows [`crate::dump_view`] deliberately, since the two are the
//! same journey in opposite directions: pick a file, see what it holds, run it
//! with progress and a stop, read the outcome. **The modal stays open while the
//! run goes**, for that module's reason — its signals are the only channel the
//! run reports to, so closing would hide work still in flight.
//!
//! **Nothing here decides whether the run is allowed.** That is
//! [`schemaic_core::sql::script_verdict`], asked in the same synchronous step
//! that launches, and it is deliberately stricter than the guard a typed
//! statement answers to: a script is treated as writing without being read, so
//! a read-only connection is refused before the file is opened.

use std::rc::Rc;

use floem::file::{FileDialogOptions, FileSpec};
use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;
use floem::reactive::create_effect;

use schemaic_core::intel::SqlDialect;
use schemaic_core::script::{Probe, RunOutcome};
use schemaic_core::sql::{GuardPolicy, RunVerdict, script_verdict};

use crate::theme;
use crate::widgets::{
    ACTION_TAB, ActionKind, ExitAction, FocusRing, action_button, autohide, control_button,
    exit_action, focus_root_with_ring, form_hint, form_section, form_separator, modal_footer_split,
    modal_h, modal_pad_h, modal_title_owned, modal_w, panel_style,
};
use crate::{ScriptRequest, ScriptTarget, Ui};

fn panel_w() -> f64 {
    modal_w(560.0)
}
fn panel_h() -> f64 {
    modal_h(520.0)
}

/// The file-choosing button's tab stop; the footer's is [`ACTION_TAB`].
const TAB_PICK: u32 = 10;

/// Open the modal on a database. Nothing is read until a file is picked.
pub(crate) fn open_script(
    ui: Ui,
    conn_id: u64,
    database: String,
    schema: Option<String>,
    dialect: SqlDialect,
) {
    let s = ui.script;
    // **Clear the sibling this one shares a tuple element with.** The two are
    // painted in one nested `stack` and each fills the modal layer when open, so
    // both being set would stack two full-screen overlays. "Only ever one at a
    // time" was true by reachability alone; the trigger/routine/event group next
    // to it states the same rule and enforces it the same way, and relying on
    // reachability is how that group's members once ended up on screen together.
    ui.dump.target.set(None);
    s.path.set(None);
    s.probe.set(None);
    s.probing.set(false);
    s.running.set(false);
    s.progress.set(None);
    s.error.set(None);
    s.done.set(None);
    s.generation.update(|g| *g += 1);
    s.target.set(Some(ScriptTarget {
        conn_id,
        database,
        schema,
        dialect,
    }));
}

/// The guard, as this modal has to ask it.
///
/// `no_database` is `false` because the modal is only reachable from a database
/// node, and `confirm_writes` is passed as the user set it even though
/// [`script_verdict`] does not consult it — a policy assembled with a *made-up*
/// value would be one that quietly starts lying the day the verdict does.
fn policy(ui: &Ui, dialect: SqlDialect, conn_id: u64) -> GuardPolicy {
    GuardPolicy {
        read_only: ui.conn.connections.with_untracked(|cs| {
            cs.iter()
                .find(|c| c.id == conn_id)
                .is_some_and(|c| c.read_only)
        }),
        confirm_writes: ui.layout.confirm_writes.get_untracked(),
        dialect,
        no_database: false,
    }
}

/// Ask for a file and probe it. The probe is what the second half of the panel
/// shows, and it is also the first thing that can tell the user they picked the
/// wrong file.
fn pick_file(ui: Ui) {
    let s = ui.script;
    let Some(target) = s.target.get_untracked() else {
        return;
    };
    let dialog = FileDialogOptions::new()
        .title("Run a SQL script")
        .allowed_types(vec![FileSpec {
            name: "SQL script",
            extensions: &["sql"],
        }]);
    let actions = ui.schema_actions.clone();
    let asked_at = s.generation.get_untracked();
    floem::action::open_file(dialog, move |file| {
        let Some(path) = file.and_then(|f| f.path.first().cloned()) else {
            return; // Dismissed.
        };
        // floem's open dialog is not window-modal, so the modal that asked may
        // be closed or reopened on another database by now — the same hazard
        // `dump_view` names at its save dialog.
        if s.generation.get_untracked() != asked_at || s.target.get_untracked().is_none() {
            return;
        }
        s.path.set(Some(path.clone()));
        s.probe.set(None);
        s.error.set(None);
        s.done.set(None);
        s.probing.set(true);
        (actions.script_probe)(
            path,
            target.dialect,
            Rc::new(move |res| {
                if s.generation.get_untracked() != asked_at {
                    return;
                }
                s.probing.set(false);
                match res {
                    Ok(p) => s.probe.set(Some(p)),
                    Err(e) => s.error.set(Some(e)),
                }
            }),
        );
    });
}

/// Launch the run.
///
/// **The guard and the launch are the same synchronous step** — the disabled
/// button says a run is going, it does not prevent a second one
/// (`widgets::accept_launch`).
fn run_script(ui: Ui) {
    let s = ui.script;
    let (Some(target), Some(path)) = (s.target.get_untracked(), s.path.get_untracked()) else {
        return;
    };
    if !crate::widgets::accept_launch(s.running.get_untracked(), false) {
        return;
    }
    // Refused outright, with no override: the read-only block has none by
    // design, and here it applies to a file nobody has read.
    if let RunVerdict::Block(why) = script_verdict(
        policy(&ui, target.dialect, target.conn_id),
        &file_name(&path),
    ) {
        s.error.set(Some(why));
        return;
    }
    s.running.set(true);
    s.error.set(None);
    s.done.set(None);
    s.progress.set(None);
    let opened = s.generation.get_untracked();
    let name = file_name(&path);
    (ui.schema_actions.script_run)(
        ScriptRequest {
            path,
            conn_id: target.conn_id,
            database: target.database.clone(),
            dialect: target.dialect,
        },
        Rc::new(move |outcome| {
            // Closing does not stop the run, so by the time it reports the modal
            // may be open on another database.
            if s.generation.get_untracked() != opened {
                return;
            }
            s.running.set(false);
            s.progress.set(None);
            match outcome {
                RunOutcome::Done { ran } => s.done.set(Some(format!(
                    "Ran {ran} {} from {name}.",
                    schemaic_core::text::plural(ran, "statement", "statements")
                ))),
                // **The count is the message, not a footnote.** A script is not
                // transactional unless the file said so, so a stopped run has
                // very often changed the database — and the number of statements
                // that landed is the only thing that says how much.
                RunOutcome::Cancelled { ran } => s.error.set(Some(format!(
                    "Stopped after {ran} {}. Any statement that already ran is still applied \
                     unless {name} opened its own transaction.",
                    schemaic_core::text::plural(ran, "statement", "statements")
                ))),
                RunOutcome::Failed { message, ran, at } => {
                    let where_ = match at {
                        // The line, because this is a file too big to open in
                        // the editor — "statement 30,000" would be no answer.
                        Some((_, line)) => format!(" at line {line} of {name}"),
                        None => String::new(),
                    };
                    s.error.set(Some(format!(
                        "Failed{where_}: {message} — {ran} {} ran before it.",
                        schemaic_core::text::plural(ran, "statement", "statements")
                    )))
                }
            }
        }),
    );
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

/// What the probe found, as the second step's body.
fn probe_body(p: &Probe) -> impl IntoView {
    let label = |s: String, muted: bool| {
        text(s).style(move |st| {
            st.font_size(theme::font_label()).color(if muted {
                theme::text_muted()
            } else {
                theme::text()
            })
        })
    };
    // The histogram, one line per kind, floors marked as floors.
    let kinds: Vec<_> = p
        .kinds
        .iter()
        .map(|(kind, n)| {
            label(format!("{}  ×{}", kind, p.count_label(*n)), false)
                .style(|s| s.padding_vert(theme::scaled(1.0)))
                .into_any()
        })
        .collect();

    let summary = if p.statements == 0 {
        "This file holds no statements Schemaic can run.".to_string()
    } else {
        format!(
            "{} {} read from the first {}.",
            p.count_label(p.statements),
            schemaic_core::text::plural(p.statements, "statement", "statements"),
            schemaic_core::stats::format_bytes(p.bytes_read),
        )
    };

    v_stack((
        form_section("What this file does"),
        label(summary, true),
        v_stack_from_iter(kinds).style(|s| {
            s.flex_col()
                .width_full()
                .padding_top(theme::scaled(6.0))
                .padding_left(theme::scaled(2.0))
        }),
        // The two facts that change what the run means. Built directly rather
        // than through a `dyn_container` over constants — `probe_body` is itself
        // rebuilt whenever the probe changes, so there was nothing for an inner
        // reactive wrapper to react to.
        {
            let mut rows: Vec<floem::AnyView> = Vec::new();
            // Only when there is destruction to report: a permanent "destroys
            // nothing" line would be one more thing to read past on every file.
            if p.destructive > 0 {
                rows.push(
                    text(format!(
                        "{} {} in this file {} something — DROP or TRUNCATE. \
                         That cannot be undone from here.",
                        p.count_label(p.destructive),
                        schemaic_core::text::plural(p.destructive, "statement", "statements"),
                        schemaic_core::text::plural(p.destructive, "destroys", "destroy"),
                    ))
                    .style(|s| {
                        s.font_size(theme::font_label())
                            .color(theme::error())
                            .padding_top(theme::scaled(8.0))
                    })
                    .into_any(),
                );
            }
            // **Same size as the warning above it, not a hint.** Whether the
            // file wraps itself is what decides the meaning of a Stop half way
            // through — the difference between "nothing happened" and "412
            // statements are applied" — so it is not one step quieter than the
            // line above it. Muted rather than red: it is a fact about the file,
            // and on the `own_transaction` side it is the reassuring one.
            let tx_line = if p.own_transaction {
                "This file opens its own transaction, so it either lands whole or not at all. \
                 Schemaic adds none of its own."
            } else {
                "This file opens no transaction, so a failure part-way leaves what ran before \
                 it in place."
            };
            rows.push(
                text(tx_line)
                    .style(|s| {
                        s.font_size(theme::font_label())
                            .color(theme::text_muted())
                            // The 4px stack gap plus 10 — the two sentences are
                            // about different things and were reading as one
                            // paragraph.
                            .padding_top(theme::scaled(10.0))
                    })
                    .into_any(),
            );
            v_stack_from_iter(rows).style(|s| s.flex_col().width_full().gap(theme::scaled(4.0)))
        },
    ))
    .style(|s| s.flex_col().width_full().gap(theme::scaled(6.0)))
}

pub(crate) fn script_overlay(ui: Ui) -> impl IntoView {
    let s = ui.script;
    watch_connection(ui.clone());
    // One decision for every exit — footer, Escape, ✕ — so none can disagree.
    // While a run goes they **stop** rather than close.
    let exit: Rc<dyn Fn()> = {
        let stop = ui.schema_actions.clone();
        Rc::new(move || match exit_action(s.running.get_untracked(), true) {
            ExitAction::Close => s.target.set(None),
            ExitAction::Cancel => (stop.script_cancel)(),
            ExitAction::Ignore => {}
        })
    };

    dyn_container(
        move || s.target.get(),
        move |target| {
            let Some(target) = target else {
                return empty().into_any();
            };
            let ring = FocusRing::new();
            let ui = ui.clone();
            let (exit_x, exit_esc, exit_footer) = (exit.clone(), exit.clone(), exit.clone());

            // The file row, in the table-import modal's shape: the button first,
            // what is chosen to its right. One spelling of "pick a file", so the
            // two Imports do not look like two features.
            let pick_ui = ui.clone();
            let pick = control_button("Choose file…", ring.clone(), TAB_PICK, move || {
                pick_file(pick_ui.clone())
            });
            let chosen = dyn_container(
                move || (s.path.get(), s.probing.get()),
                |(path, probing)| match (path, probing) {
                    (Some(p), false) => text(file_name(&p))
                        .style(|st| st.color(theme::text()).font_size(theme::font_body()))
                        .into_any(),
                    (Some(_), true) => crate::widgets::loading_dots(
                        "Reading the file",
                        theme::text_muted,
                        theme::font_body,
                    )
                    .into_any(),
                    (None, _) => text("No file chosen")
                        .style(|st| st.color(theme::text_dim()).font_size(theme::font_body()))
                        .into_any(),
                },
            );
            let file_row = h_stack((
                pick,
                chosen.style(|st| st.flex_grow(1.0_f32).min_width(0.0)),
            ))
            .style(|st| st.items_center().gap(theme::scaled(10.0)).width_full());

            // Choices and findings only; everything the run says about itself is
            // in the footer, which does not scroll.
            let body = v_stack((
                form_section("SQL script"),
                // No *File* caption over the row: the section heading already
                // says what is being chosen, and `form_setting`'s label would be
                // a second word for the same thing directly above the button
                // that says it a third time.
                file_row,
                // **The one thing the modal's title cannot be trusted about.**
                // It says *Import into `shop`*, and a dump written with
                // `--databases` opens `CREATE DATABASE …; USE …;` — so the file
                // may build somewhere else entirely, or in several places. The
                // run is scoped to the connection, not to the database, and
                // nothing here can confine it: the statements name their own
                // targets. Saying so is the only honest option.
                form_hint(
                    "The script may create and select a different database from the one you're \
                     importing into. It may also create more than one database. Check the script \
                     to make sure it does what you expect.",
                ),
                form_separator(theme::scaled(16.0)),
                dyn_container(
                    move || s.probe.get(),
                    |p| match p {
                        Some(p) => probe_body(&p).into_any(),
                        None => empty().into_any(),
                    },
                ),
            ))
            .style(|st| st.flex_col().width_full().gap(theme::scaled(12.0)));

            let run_ui = ui.clone();
            let footer_ring = ring.clone();
            let footer = dyn_container(
                move || {
                    (
                        s.running.get(),
                        // A file is chosen and the probe has finished. Not
                        // "the probe succeeded": a file Schemaic could not read
                        // ahead is still one the server may accept, and the
                        // failure it would give is more use than a Run that
                        // stays grey with no reason on it.
                        s.path.get().is_some() && !s.probing.get(),
                        s.done.get(),
                        s.error.get(),
                    )
                },
                move |(running, ready, done, error)| {
                    let (run_ui, ring) = (run_ui.clone(), footer_ring.clone());
                    let exit = exit_footer.clone();
                    // Its own `dyn_container`, so a progress tick doesn't rebuild
                    // the buttons and take the focus ring with them.
                    let left: floem::AnyView = if running {
                        dyn_container(
                            move || s.progress.get(),
                            move |p| match p {
                                // **Bytes, because a statement total cannot be
                                // known without reading the file** — see
                                // `ScriptProgress`. A file whose length could not
                                // be read reports what it has done and no
                                // denominator, rather than dividing by zero.
                                Some(p) if p.bytes_total > 0 => text(format!(
                                    "{} of {}",
                                    schemaic_core::stats::format_bytes(p.bytes_done),
                                    schemaic_core::stats::format_bytes(p.bytes_total),
                                ))
                                .style(|st| {
                                    st.color(theme::text_muted())
                                        .font_size(theme::font_label())
                                        .min_width(0.0)
                                })
                                .into_any(),
                                Some(p) => text(format!(
                                    "{} read",
                                    schemaic_core::stats::format_bytes(p.bytes_done)
                                ))
                                .style(|st| {
                                    st.color(theme::text_muted())
                                        .font_size(theme::font_label())
                                        .min_width(0.0)
                                })
                                .into_any(),
                                None => crate::widgets::loading_dots(
                                    "Connecting",
                                    theme::text_muted,
                                    theme::font_label,
                                )
                                .into_any(),
                            },
                        )
                        .into_any()
                    } else if let Some(e) = error {
                        text(e)
                            .style(|st| {
                                st.color(theme::error())
                                    .font_size(theme::font_label())
                                    .min_width(0.0)
                            })
                            .into_any()
                    } else if let Some(msg) = done {
                        text(msg)
                            .style(|st| {
                                st.color(theme::status_ok())
                                    .font_size(theme::font_label())
                                    .min_width(0.0)
                            })
                            .into_any()
                    } else {
                        empty().into_any()
                    };
                    modal_footer_split(
                        left,
                        h_stack((
                            action_button(
                                if running { "Stop" } else { "Close" },
                                if running {
                                    ActionKind::Danger
                                } else {
                                    ActionKind::Neutral
                                },
                                true,
                                ring.clone(),
                                ACTION_TAB,
                                move || (exit)(),
                            ),
                            // **`Primary`, the same as the table import's own
                            // button.** It was `Danger` on the argument that
                            // this runs someone else's DDL with no undo — but
                            // that made the one modal reachable by two menu
                            // entries wear two different confirming colours,
                            // which reads as two different features rather than
                            // as a warning. What the file will do is said in
                            // words, in the panel, where it can be specific;
                            // the button is the same button.
                            action_button(
                                "Run",
                                ActionKind::Primary,
                                !running && ready,
                                ring,
                                ACTION_TAB + 10,
                                move || run_script(run_ui.clone()),
                            ),
                        ))
                        .style(|st| st.items_center().gap(theme::scaled(8.0)))
                        .into_any(),
                    )
                    .into_any()
                },
            )
            .style(|st| st.width_full());

            let close_x: Rc<dyn Fn()> = exit_x.clone();
            let title = match target.schema.as_deref() {
                Some(ns) => format!("Import into {}.{ns}", target.database),
                None => format!("Import into {}", target.database),
            };
            let panel = v_stack((
                modal_title_owned(title, close_x, ring.clone()),
                autohide(scroll(body.style(|st| {
                    st.flex_col()
                        .width_full()
                        .padding_horiz(modal_pad_h())
                        .padding_vert(theme::scaled(18.0))
                })))
                .style(|st| st.width_full().flex_grow(1.0_f32).min_height(0.0)),
                footer,
            ))
            .on_click_stop(|_| {})
            .style(move |st| panel_style(st).width(panel_w()).height(panel_h()));

            focus_root_with_ring(container(panel), ring)
                .on_key_down(
                    Key::Named(NamedKey::Escape),
                    |_| true,
                    move |_| (exit_esc)(),
                )
                .style(|st| {
                    st.size_full()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .background(theme::modal_backdrop())
                })
                .into_any()
        },
    )
    .style(move |st| {
        if s.target.get().is_some() {
            st.absolute().inset(0.0)
        } else {
            st
        }
    })
}

/// Close the modal when the connection it belongs to goes away — `dump_view`'s
/// rule, for its reason: the modal would otherwise still be describing the old
/// connection, and its Run button would reach the new one.
fn watch_connection(ui: Ui) {
    let s = ui.script;
    let active = ui.conn.active_conn;
    create_effect(move |_| {
        let conn = active.get();
        if s.target
            .with(|t| t.as_ref().is_some_and(|t| t.conn_id != conn))
            && !s.running.get()
        {
            s.target.set(None);
        }
    });
}
