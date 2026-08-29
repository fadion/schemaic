//! The schema + data dump modal — **Export** to a user, opened from a
//! database's or a PostgreSQL namespace's context menu next to *Generate DDL*,
//! and from a table's directly below *Import*. The naming split (Export in the
//! interface, `dump` in the code) is deliberate and explained in
//! [`schemaic_core::dump`].
//!
//! It collects a selection and six options, then hands both to
//! `SchemaActions::dump_run`, which introspects, plans
//! ([`schemaic_core::dump::plan`]) and writes. Nothing here builds SQL: the file
//! is decided in the core and written in the app, and this view only says what
//! the user asked for.
//!
//! **The modal stays open while the dump runs**, like the import modal and for
//! the same reason: its signals are the only channel the run reports to, so
//! closing would hide work still in flight and leave its outcome with no reader.
//! Every exit therefore cancels rather than closes while `running`.

use std::rc::Rc;

use floem::file::{FileDialogOptions, FileSpec};
use floem::keyboard::{Key, NamedKey};
use floem::prelude::*;
use floem::reactive::{Memo, create_effect, create_memo};

use schemaic_core::intel::SqlDialect;

use crate::theme;
use crate::widgets::{
    ACTION_TAB, ActionKind, ExitAction, FocusRing, action_button, autohide, exit_action,
    focus_root_with_ring, form_hint, form_section, form_separator, modal_footer_split, modal_h,
    modal_pad_h, modal_title_owned, modal_w, panel_style,
};
use crate::{DumpOutcome, DumpRequest, DumpTarget, Ui, icons};

/// The panel's nominal size, scaled like every other modal's.
fn panel_w() -> f64 {
    modal_w(560.0)
}
fn panel_h() -> f64 {
    modal_h(600.0)
}

/// Tab stops. The table list is one stop (its rows are pointer targets, not a
/// hundred focusables), the options follow, the footer is last.
const TAB_ALL: u32 = 10;
const TAB_NONE: u32 = 20;
const TAB_OPTS: u32 = 100;

/// Open the modal on a database, and start reading its table list.
///
/// Resets every signal rather than owning a per-open scope — the bundle rule
/// [`crate::DumpUi`] states. `generation` goes up here, which is what lets a
/// late outcome tell whether it is still being waited for.
/// `preselect` ticks exactly one table instead of all of them — what a table
/// node's own *Export* opens. The picker still lists the whole database, so the
/// neighbours that table's foreign keys point at are one click away.
pub(crate) fn open_dump(
    ui: Ui,
    conn_id: u64,
    database: String,
    schema: Option<String>,
    preselect: Option<String>,
    dialect: SqlDialect,
) {
    let d = ui.dump;
    d.tables.set(Vec::new());
    d.chosen.set(Vec::new());
    d.progress.set(None);
    d.error.set(None);
    d.done.set(None);
    d.running.set(false);
    d.listing.set(true);
    d.generation.update(|g| *g += 1);
    d.target.set(Some(DumpTarget {
        conn_id,
        database: database.clone(),
        schema: schema.clone(),
        dialect,
    }));

    let opened = d.generation.get_untracked();
    (ui.schema_actions.dump_tables)(
        conn_id,
        database,
        Rc::new(move |res| {
            // The modal may have been reopened on another database while this
            // read was out — filling *that* picker with these names would offer
            // tables the dump would then fail to find.
            if d.generation.get_untracked() != opened {
                return;
            }
            d.listing.set(false);
            match res {
                Ok(names) => {
                    // Both decisions are `core::dump`'s, with tests: which of the
                    // database's tables belong to this namespace, and what the
                    // picker opens ticked. They were written out here, inside a
                    // `create_ext_action` callback, in the range's largest file
                    // with no tests at all — and each is a rule with a
                    // counter-intuitive arm (`public` is *unqualified*; a
                    // preselect the list has lost has to be named, not ignored).
                    let mut names =
                        schemaic_core::dump::tables_in_namespace(&names, schema.as_deref());
                    names.sort();
                    let (chosen, error) =
                        schemaic_core::dump::initial_selection(&names, preselect.as_deref());
                    d.chosen.set(chosen);
                    if let Some(e) = error {
                        d.error.set(Some(e));
                    }
                    d.tables.set(names);
                }
                Err(e) => d.error.set(Some(e)),
            }
        }),
    );
}

/// Launch the dump the modal describes, after the save dialog names a file.
///
/// **The guard is in the same synchronous step as the launch** — the disabled
/// button says a dump is running, it does not prevent a second one, because a
/// disabled style only takes effect on a later update pass
/// (`widgets::accept_launch`, and the two imports that each opened their own
/// transaction before it existed).
fn run_dump(ui: Ui) {
    let d = ui.dump;
    let Some(target) = d.target.get_untracked() else {
        return;
    };
    // **Read at the moment of the launch, not before the dialog.** floem's save
    // dialog is not window-modal, so the modal stays live behind it: untick every
    // table, tick one, choose a filename — and the file was written from the
    // selection as it stood before the dialog opened, contradicting the modal
    // still on screen. The pre-dialog read below is only to decide whether there
    // is anything to ask a filename *for*.
    let selection = move || {
        (
            schemaic_core::dump::DumpOptions {
                structure: d.structure.get_untracked(),
                data: d.data.get_untracked(),
                other_objects: d.other_objects.get_untracked(),
                drop_if_exists: d.drop_if_exists.get_untracked(),
                wrap_transaction: d.wrap_transaction.get_untracked(),
                disable_fk_checks: d.disable_fk_checks.get_untracked(),
            },
            d.chosen.get_untracked(),
        )
    };
    let (opts, tables) = selection();
    if opts.is_empty() || tables.is_empty() {
        return;
    }
    let dialog = FileDialogOptions::new()
        .title("Export to SQL")
        .default_name(format!("{}.sql", target.database))
        .allowed_types(vec![FileSpec {
            name: "SQL",
            extensions: &["sql"],
        }]);
    let actions = ui.schema_actions.clone();
    // Read **before** the dialog opens: the launch below has to be able to tell
    // "the modal that asked for this" from "a modal that has since been closed
    // and reopened on another database". The late-outcome guard further down
    // already reads it for the same reason.
    let asked_at = d.generation.get_untracked();
    floem::action::save_as(dialog, move |file| {
        let Some(path) = file.and_then(|f| f.path.first().cloned()) else {
            return; // The dialog was dismissed.
        };
        // `accept_dialog_launch`, not `accept_launch`: floem's save dialog is not
        // window-modal, so the modal that asked can be gone by now — and then
        // `running` is still `false` and the plain guard says yes to an invisible,
        // unstoppable dump.
        if !crate::widgets::accept_dialog_launch(
            d.running.get_untracked(),
            false,
            d.target.get_untracked().is_some(),
            asked_at,
            d.generation.get_untracked(),
        ) {
            return;
        }
        // The selection as it stands *now* — see `selection` above.
        let (opts, tables) = selection();
        if opts.is_empty() || tables.is_empty() {
            return;
        }
        d.running.set(true);
        d.error.set(None);
        d.done.set(None);
        d.progress.set(None);
        // The destination's name, for the two sentences below. Both of them are
        // about *this* file and its `.part` sibling, and the outcome carries
        // neither — the modal is the only place that still knows.
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        let opened = d.generation.get_untracked();
        (actions.dump_run)(
            DumpRequest {
                path,
                conn_id: target.conn_id,
                database: target.database.clone(),
                tables: tables.clone(),
                opts,
                dialect: target.dialect,
            },
            Rc::new(move |outcome| {
                // Closing the modal does not stop the dump, so by the time it
                // reports the modal may be open on another database — reporting
                // into that would claim a file for the wrong one.
                if d.generation.get_untracked() != opened {
                    return;
                }
                d.running.set(false);
                d.progress.set(None);
                // A failure is `export::export_failure_note` verbatim: it makes
                // exactly the promise this path makes — the destination is
                // untouched, and what was written is in the sibling.
                //
                // **The cancel is not `export::export_cancel_note`**, which is
                // otherwise the same sentence. That one ends "the rows that were
                // written are in …", and it is right to, because a result export
                // is nothing but rows. This file is `CREATE TABLE`s and triggers
                // as well, and a structure-only one has no rows in it at all, so
                // borrowing that wording would describe a file that isn't there.
                // The half that must not drift is *where the fragment went*, and
                // that comes from `part_path` — the one function that knows the
                // suffix — in both spellings.
                match outcome {
                    // **Through `export_note`, which is where a caveat is
                    // worded.** The tally carries what the file could not hold —
                    // binary columns written as `NULL`, values past the arena
                    // ceiling left blank — and a green "Wrote 5 tables." over a
                    // file whose every blob is `NULL` is the failure this whole
                    // sentence exists to prevent. The table count is this path's
                    // own; the rest is the same sentence the grid's bar shows.
                    DumpOutcome::Done {
                        tables,
                        tally,
                        missing,
                    } => {
                        // A ticked table the dump's own fresh introspection could
                        // not find is the difference between a backup and a file
                        // that looks like one, so it goes in the same sentence as
                        // the tally rather than only into the file's header.
                        let short = if missing.is_empty() {
                            String::new()
                        } else {
                            format!(
                                " {} ticked {} not found and {} not in the file: {}.",
                                missing.len(),
                                schemaic_core::text::plural(missing.len(), "table", "tables"),
                                schemaic_core::text::plural(missing.len(), "is", "are"),
                                missing.join(", "),
                            )
                        };
                        d.done.set(Some(format!(
                            "Wrote {} {}. {}{short}",
                            tables,
                            schemaic_core::text::plural(tables, "table", "tables"),
                            schemaic_core::export::export_note(&tally, &name, true)
                                .unwrap_or_default(),
                        )))
                    }
                    DumpOutcome::Cancelled => d.error.set(Some(format!(
                        "Export cancelled — {name} was not changed; what had been written is in {}",
                        schemaic_core::export::part_path(&name)
                    ))),
                    DumpOutcome::Failed { message, partial } => {
                        d.error.set(Some(schemaic_core::export::export_failure_note(
                            &message,
                            partial.then_some(name.as_str()),
                        )))
                    }
                }
            }),
        );
    });
}

/// One table row: a check mark when it is in, a hollow square when it is out.
fn table_row(
    d: crate::DumpUi,
    chosen: Memo<std::collections::HashSet<String>>,
    name: String,
) -> impl IntoView {
    // **A set, and `hide()` rather than a rebuild.** Every row watches the
    // selection, so a single tick used to cost one linear `contains` *per row*
    // over the whole selection and one view rebuild each — quadratic, and
    // visibly so at the few hundred tables the perf fixtures carry. The memo
    // makes each row's read O(1), and the pair of glyphs is shown and hidden by
    // style, which is the codebase's rule for a reactive show-hide: nothing is
    // constructed, so nothing takes the row apart under the pointer.
    let is_in = {
        let name = name.clone();
        move || chosen.with(|c| c.contains(&name))
    };
    let (on_in, on_out) = (is_in.clone(), is_in.clone());
    let toggle_name = name.clone();
    h_stack((
        icons::icon(icons::CHECK, 14.0).style(move |s| {
            let s = s.color(theme::accent());
            if on_in() { s } else { s.hide() }
        }),
        icons::icon(icons::SQUARE, 14.0).style(move |s| {
            let s = s.color(theme::text_faint());
            if on_out() { s.hide() } else { s }
        }),
        text(name.clone()).style(|s| {
            s.font_family(crate::consts::MONO_FAMILY.to_string())
                .font_size(theme::font_body())
                .color(theme::text())
        }),
    ))
    .on_click_stop(move |_| {
        let name = toggle_name.clone();
        d.chosen
            .update(|c| match c.iter().position(|n| *n == name) {
                Some(i) => {
                    c.remove(i);
                }
                None => c.push(name),
            });
    })
    .style(|s| {
        s.items_center()
            .width_full()
            .gap(theme::scaled(8.0))
            .padding_horiz(theme::scaled(8.0))
            .padding_vert(theme::scaled(4.0))
            .border_radius(4.0)
            .hover(|s| s.background(theme::row_hover_soft()))
    })
}

/// The picker: All / None, then the list.
fn table_picker(ui: Ui, ring: FocusRing) -> impl IntoView {
    let d = ui.dump;
    // The selection as a set, computed once per change instead of once per row:
    // the list is the one signal every row in it reads. `chosen` stays a `Vec`
    // because it is what the request carries and what All resets from.
    let chosen_set = create_memo(move |_| {
        d.chosen
            .with(|c| c.iter().cloned().collect::<std::collections::HashSet<_>>())
    });
    let count = move || {
        let (a, b) = (d.chosen.with(|c| c.len()), d.tables.with(|t| t.len()));
        if a == b {
            format!("All {b} selected")
        } else {
            format!("{a} of {b} selected")
        }
    };
    let head = h_stack((
        label(count).style(|s| s.color(theme::text_dim()).font_size(theme::font_label())),
        empty().style(|s| s.flex_grow(1.0_f32)),
        action_button("All", ActionKind::Quiet, true, ring.clone(), TAB_ALL, {
            move || d.chosen.set(d.tables.get_untracked())
        }),
        action_button("None", ActionKind::Quiet, true, ring, TAB_NONE, move || {
            d.chosen.set(Vec::new())
        }),
    ))
    .style(|s| s.items_center().width_full().gap(theme::scaled(6.0)));

    let list = dyn_container(
        move || (d.listing.get(), d.tables.get()),
        move |(listing, names)| {
            if listing {
                return text("Reading the table list…")
                    .style(|s| s.color(theme::text_muted()).font_size(theme::font_body()))
                    .into_any();
            }
            if names.is_empty() {
                return text("This database has no tables.")
                    .style(|s| s.color(theme::text_muted()).font_size(theme::font_body()))
                    .into_any();
            }
            v_stack_from_iter(names.into_iter().map(move |n| table_row(d, chosen_set, n)))
                .style(|s| s.flex_col().width_full())
                .into_any()
        },
    )
    .style(|s| s.flex_col().width_full());

    v_stack((
        head,
        autohide(scroll(list).style(|s| s.width_full())).style(|s| {
            s.width_full()
                .height(theme::scaled(190.0))
                .margin_top(theme::scaled(6.0))
                .padding(theme::scaled(4.0))
                .background(theme::bg_deepest())
                .border(1.0)
                .border_color(theme::border())
                .border_radius(5.0)
        }),
    ))
    .style(|s| s.flex_col().width_full())
}

/// The dump modal. Absolutely positioned over the workspace while
/// `ui.dump.target` is `Some`.
pub(crate) fn dump_overlay(ui: Ui) -> impl IntoView {
    let d = ui.dump;
    // Built once, with the modal rather than with each open: this view is
    // constructed at startup and the `dyn_container` below is what appears and
    // disappears, so an effect created here has the lifetime the modal does.
    watch_connection(ui.clone());
    // One decision for every exit — footer, Escape, ✕ — so none of them can
    // disagree. While an export runs they **stop** it rather than closing, the
    // import modal's rule and for its reason: closing would hide a write that is
    // still going and leave its outcome with no reader, since this modal's
    // signals are the only channel the run reports to. The footer says so in the
    // one place the user looks, by wearing the word Stop and the colour of an
    // action while that is what it does.
    let exit: Rc<dyn Fn()> = {
        let stop = ui.schema_actions.clone();
        Rc::new(move || match exit_action(d.running.get_untracked(), true) {
            ExitAction::Close => d.target.set(None),
            ExitAction::Cancel => (stop.dump_cancel)(),
            // Unreachable while `cancellable` is true, matched explicitly so a
            // later caller can't fall through to closing over a running export.
            ExitAction::Ignore => {}
        })
    };

    dyn_container(
        move || d.target.get(),
        move |target| {
            let Some(target) = target else {
                return empty().into_any();
            };
            let ring = FocusRing::new();
            let ui = ui.clone();
            let (exit_x, exit_esc, exit_footer) = (exit.clone(), exit.clone(), exit.clone());

            let what = v_stack((
                form_section("Contents"),
                crate::settings::focusable_toggle_row(
                    "Structure",
                    "CREATE TABLE, its triggers, and the foreign keys, restated after the data.",
                    d.structure,
                    ring.clone(),
                    TAB_OPTS,
                ),
                crate::settings::focusable_toggle_row(
                    "Data",
                    "Every row, as INSERT statements.",
                    d.data,
                    ring.clone(),
                    TAB_OPTS + 10,
                ),
                crate::settings::focusable_toggle_row(
                    "Other objects",
                    "Types, sequences, routines and events these tables lean on.",
                    d.other_objects,
                    ring.clone(),
                    TAB_OPTS + 20,
                ),
            ))
            .style(|s| s.flex_col().width_full().gap(theme::scaled(10.0)));

            // The guard is offered only where the engine has one an ordinary
            // role can throw — asked of the dialect through the core predicate,
            // never spelled out again here.
            let has_guard = schemaic_core::dump::fk_guard_sql(target.dialect).is_some();
            let replay = v_stack((
                form_section("Replaying"),
                crate::settings::focusable_toggle_row(
                    "Drop before create",
                    "DROP TABLE IF EXISTS before each CREATE, so the file loads onto a database \
                     that already holds these tables.",
                    d.drop_if_exists,
                    ring.clone(),
                    TAB_OPTS + 30,
                ),
                crate::settings::focusable_toggle_row(
                    "One transaction",
                    "Wrap the load, so a failure leaves nothing behind.",
                    d.wrap_transaction,
                    ring.clone(),
                    TAB_OPTS + 40,
                ),
                if has_guard {
                    crate::settings::focusable_toggle_row(
                        "Disable foreign-key checks",
                        "Off for the duration of the load, so the order rows arrive in can't \
                         refuse them.",
                        d.disable_fk_checks,
                        ring.clone(),
                        TAB_OPTS + 50,
                    )
                    .into_any()
                } else {
                    // Said, rather than shown greyed: PostgreSQL's switch is
                    // superuser-only, so offering it would be a checkbox that
                    // fails the restore for most roles.
                    form_hint(
                        "This engine has no session switch for foreign-key checks; the file \
                         adds the constraints after the data instead.",
                    )
                    .into_any()
                },
            ))
            .style(|s| s.flex_col().width_full().gap(theme::scaled(10.0)));

            // Choices only. **Everything the run says about itself is in the
            // footer** — this body scrolls, so a progress line or an outcome put
            // here is one the user has to go looking for, and the whole point of
            // both is being seen without looking.
            let body = v_stack((
                table_picker(ui.clone(), ring.clone()),
                form_separator(theme::scaled(16.0)),
                what,
                form_separator(theme::scaled(16.0)),
                replay,
            ))
            .style(|s| s.flex_col().width_full().gap(theme::scaled(16.0)));

            // **Rebuilt on what enables it**, not read once while the panel is
            // built: `action_button` takes a plain `bool`, so a state read here
            // is the state at build time. The footer that only *looks* right is
            // the failure — an Export button left enabled after the last table is
            // unchecked launches an export of nothing. Same `dyn_container` the
            // import footer uses, and for the same reason.
            //
            // The **left** half is where the run says what it is doing and how it
            // ended, so both live at eye level next to the buttons rather than at
            // the bottom of a body that scrolls.
            let run_ui = ui.clone();
            let footer_ring = ring.clone();
            let footer = dyn_container(
                move || {
                    (
                        d.running.get(),
                        d.chosen.with(|c| c.is_empty()),
                        d.options().is_empty(),
                        d.done.get(),
                        d.error.get(),
                    )
                },
                move |(running, none_chosen, no_content, done, error)| {
                    let (run_ui, ring) = (run_ui.clone(), footer_ring.clone());
                    let exit = exit_footer.clone();
                    // **Text only, and no control beside it.** The progress line
                    // grows as the count does (`3 of 12, 9k` → `98k rows so far`),
                    // so a button laid out after it walked left and right on every
                    // tick — a moving target, and the one control the user wants
                    // at that moment. Stopping is the footer's own dismissive
                    // button now, which never moves.
                    //
                    // Its own `dyn_container` inside the footer's: this ticks once
                    // per table, and rebuilding the buttons on each tick would
                    // take the focus ring with them.
                    let left: floem::AnyView = if running {
                        dyn_container(
                            move || d.progress.get(),
                            move |p| match p {
                                // **What it is writing, not that it is writing.**
                                // The table name and the running count are the
                                // difference between an export that looks stuck
                                // and one visibly grinding through a large table.
                                Some(p) => text(format!(
                                    "Writing {} — {} of {}, {} rows so far",
                                    p.table,
                                    p.index,
                                    p.total,
                                    schemaic_core::text::human_count(p.rows as usize),
                                ))
                                .style(|s| {
                                    s.color(theme::text_muted())
                                        .font_size(theme::font_label())
                                        .min_width(0.0)
                                })
                                .into_any(),
                                // Before the first table there is nothing to count
                                // yet, and the schema read is the slow part on a
                                // large database — animated, because a static
                                // label and a hung one look identical.
                                None => crate::widgets::loading_dots(
                                    "Reading the schema",
                                    theme::text_muted,
                                    theme::font_label,
                                )
                                .into_any(),
                            },
                        )
                        .into_any()
                    } else if let Some(e) = error {
                        text(e)
                            .style(|s| {
                                s.color(theme::error())
                                    .font_size(theme::font_label())
                                    .min_width(0.0)
                            })
                            .into_any()
                    } else if let Some(msg) = done {
                        // Built at the callback, where the file's name was still
                        // in scope — `export_note` names the file and every column
                        // the export could not carry, in the grid's own wording.
                        text(msg)
                            .style(|s| {
                                s.color(theme::status_ok())
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
                            // **The dismissive slot becomes Stop while it runs,
                            // and turns red.** It is the modal's one fixed
                            // position, which is what the export needs a stop to
                            // be — the alternative, a button beside the progress
                            // text, moved with the row count under the cursor.
                            // Red because it is an action rather than a way out:
                            // in `Neutral` it reads as the same dismissive button
                            // with a different word on it, which is precisely what
                            // it is not. `exit` routes it, so Escape and the ✕
                            // stop the export too and no exit can disagree with
                            // another.
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
                            action_button(
                                "Export",
                                ActionKind::Primary,
                                !running && !none_chosen && !no_content,
                                ring,
                                ACTION_TAB + 10,
                                move || run_dump(run_ui.clone()),
                            ),
                        ))
                        .style(|s| s.items_center().gap(theme::scaled(8.0)))
                        .into_any(),
                    )
                    .into_any()
                },
            )
            .style(|s| s.width_full());

            let close_x: Rc<dyn Fn()> = exit_x.clone();
            let title = match target.schema.as_deref() {
                Some(ns) => format!("Export {}.{ns}", target.database),
                None => format!("Export {}", target.database),
            };
            let panel = v_stack((
                modal_title_owned(title, close_x, ring.clone()),
                autohide(scroll(body.style(|s| {
                    s.flex_col()
                        .width_full()
                        .padding_horiz(modal_pad_h())
                        .padding_vert(theme::scaled(18.0))
                })))
                .style(|s| s.width_full().flex_grow(1.0_f32).min_height(0.0)),
                footer,
            ))
            .on_click_stop(|_| {})
            .style(move |s| panel_style(s).width(panel_w()).height(panel_h()));

            focus_root_with_ring(container(panel), ring)
                .on_key_down(
                    Key::Named(NamedKey::Escape),
                    |_| true,
                    move |_| (exit_esc)(),
                )
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
        if d.target.get().is_some() {
            s.absolute().inset(0.0)
        } else {
            s
        }
    })
}

/// Close the modal when the connection it belongs to goes away.
///
/// A dump names a connection and a database; if the user switches connections
/// the modal would still be describing the old one, and its Dump button would
/// launch against a `conn_id` that is no longer selected.
fn watch_connection(ui: Ui) {
    let d = ui.dump;
    let active = ui.conn.active_conn;
    create_effect(move |_| {
        let conn = active.get();
        if d.target
            .with(|t| t.as_ref().is_some_and(|t| t.conn_id != conn))
            && !d.running.get()
        {
            d.target.set(None);
        }
    });
}
