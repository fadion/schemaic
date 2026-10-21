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
//! **One panel, six formats.** `Export ▸` is a submenu, and the format the user
//! picked is [`crate::DumpTarget::format`]. `SQL` is the dump described above.
//! The other five write **one file per table into a folder**
//! ([`schemaic_core::dump::file_plan`] → `SchemaActions::files_run`), and for
//! those the six options are not shown at all: every one of them is about a
//! `.sql` file — what structure to emit, what to drop first, what to wrap the
//! load in — and a CSV can honour none of it. The split is asked as a capability,
//! [`crate::DumpTarget::writes_folder`], never as a `format == Sql` at each of
//! the six places that branch on it.
//!
//! The **grid** export's progress modal lives here too
//! ([`export_progress_overlay`]) — a different feature reached from a different
//! surface, but this panel's footer with nothing above it, and keeping the two
//! in one file is what keeps them looking alike through the next change to
//! either.
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

use schemaic_core::dump::PickerBody;
use schemaic_core::intel::SqlDialect;

use crate::i18n::tr;
use crate::theme;
use crate::widgets::{
    ACTION_TAB, ActionKind, ExitAction, FocusRing, action_button, autohide, exit_action,
    focus_root_with_ring, form_hint, form_section_header, form_separator, link_button,
    modal_footer_split, modal_h, modal_pad_h, modal_title_owned, modal_w, panel_style,
};
use schemaic_core::export::ExportFormat;

use crate::{
    ConnUi, DumpFn, DumpOutcome, DumpRequest, DumpTablesFn, DumpTarget, DumpUi, ExportUi, FilesFn,
    FilesOutcome, FilesRequest, OverlayUi, ScriptUi,
};

/// Everything the export modal reaches out of the root bundle.
///
/// **The drivers take this; the renderers take [`DumpUi`].** `table_picker`
/// decides nothing that is not already in the modal's own signals, and says so —
/// as `dump_options` and `table_row` beneath it already did. The rest either
/// launch a write — which means a fetch — or watch something outside the modal,
/// and those are what is gathered here. [`run_export`] is on it not because it
/// reads anything outside the modal but because it **routes** to one of the two
/// launchers, which do.
///
/// Holds no `Ui`, for `whole_ui_gate`'s reason: it is built by naming the reads
/// at each of its two call sites, so what this modal touches is visible there.
///
/// [`export_progress_overlay`] is **not** on it. It is the grid export's modal —
/// a different feature sharing this file — and it reads `ExportUi` and
/// `TabsActions::export_cancel`, neither of which is here.
#[derive(Clone)]
pub(crate) struct DumpCtx {
    /// The modal's own state — target, table list, selection, options,
    /// progress, outcome.
    dump: DumpUi,
    /// The script modal's target, cleared on the way in. The two share one
    /// element of the modal-layer tuple, so each opening has to close the other
    /// — see [`open_dump`].
    script: ScriptUi,
    /// The connection registry, watched so a switch closes a modal that would
    /// otherwise launch against a `conn_id` nobody is on — see
    /// [`watch_connection`].
    conn: ConnUi,
    /// The app-wide confirm, for the one question this modal asks outside its
    /// own panel: the folder export's *Replace files* prompt. Reached only from
    /// [`launch_files`]'s `WouldReplace` arm.
    overlay: OverlayUi,
    /// One database's table and view names, for the picker.
    tables: DumpTablesFn,
    /// Introspect, plan and write the `.sql` dump.
    run: DumpFn,
    /// The same into a folder, one file per table — the other five formats.
    files: FilesFn,
    /// Stop whichever of the two is running. One stop for both because there is
    /// one modal; see [`crate::SchemaActions::dump_cancel`].
    cancel: Rc<dyn Fn()>,
}

impl DumpCtx {
    /// Gather the modal's reads where the root bundle is in reach.
    pub(crate) fn new(
        dump: DumpUi,
        script: ScriptUi,
        conn: ConnUi,
        overlay: OverlayUi,
        actions: &crate::SchemaActions,
    ) -> Self {
        Self {
            dump,
            script,
            conn,
            overlay,
            tables: actions.dump_tables.clone(),
            run: actions.dump_run.clone(),
            files: actions.files_run.clone(),
            cancel: actions.dump_cancel.clone(),
        }
    }

    /// The live connection's id, dialect and read-only flag, read at a click.
    ///
    /// Here rather than at the call site so `conn` can stay private: the export
    /// submenu needs the dialect to open this modal and nothing else out of the
    /// registry, and handing it a `ConnUi` beside a ctx that already holds one
    /// is two names for the same bundle — the shape `whole_ui_gate`'s own note
    /// on `SchemaTreeCtx` warns about, one level down.
    pub(crate) fn edit_ctx(&self) -> crate::table_designer::EditCtx {
        crate::table_designer::edit_ctx(self.conn)
    }
}

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
///
/// `format` is which entry of the `Export ▸` submenu was clicked. It decides
/// what the modal *is*: `Sql` is the dump this module was written for, and the
/// other five are a folder of one file per table. See [`DumpTarget::format`].
pub(crate) fn open_dump(
    ctx: &DumpCtx,
    conn_id: u64,
    database: String,
    schema: Option<String>,
    preselect: Option<String>,
    dialect: SqlDialect,
    format: ExportFormat,
) {
    let d = ctx.dump;
    // The other half of `script_view::open_script`'s rule — the two share one
    // tuple element in the modal layer, and each fills it when open.
    ctx.script.target.set(None);
    d.tables.set(Vec::new());
    d.chosen.set(Vec::new());
    d.progress.set(None);
    d.error.set(None);
    d.done.set(None);
    d.running.set(false);
    d.listing.set(schemaic_core::dump::Listing::Reading);
    d.generation.update(|g| *g += 1);
    d.target.set(Some(DumpTarget {
        conn_id,
        database: database.clone(),
        schema: schema.clone(),
        dialect,
        format,
    }));

    let opened = d.generation.get_untracked();
    (ctx.tables)(
        conn_id,
        database,
        Rc::new(move |res| {
            // The modal may have been reopened on another database while this
            // read was out — filling *that* picker with these names would offer
            // tables the dump would then fail to find.
            if d.generation.get_untracked() != opened {
                return;
            }
            // Which of the two ways it came back, not just that it did: the
            // picker's "no tables" sentence is a claim about a database that was
            // successfully read, and this is the only place that knows.
            d.listing.set(match &res {
                Ok(_) => schemaic_core::dump::Listing::Done,
                Err(_) => schemaic_core::dump::Listing::Failed,
            });
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

/// The footer's Export button: whichever of the two writers this modal is for.
///
/// **The branch is here and nowhere else.** Both halves below end at
/// `accept_dialog_launch` and set the same four signals, and the one thing that
/// must never happen is a modal opened on CSV launching the dump — so the
/// question is asked once, of [`DumpTarget::writes_folder`], at the only point
/// where either can start.
fn run_export(ctx: &DumpCtx) {
    let Some(target) = ctx.dump.target.get_untracked() else {
        return;
    };
    if target.writes_folder() {
        run_files(ctx, target);
    } else {
        run_dump(ctx, target);
    }
}

/// Launch the dump the modal describes, after the save dialog names a file.
///
/// **The guard is in the same synchronous step as the launch** — the disabled
/// button says a dump is running, it does not prevent a second one, because a
/// disabled style only takes effect on a later update pass
/// (`widgets::accept_launch`, and the two imports that each opened their own
/// transaction before it existed).
fn run_dump(ctx: &DumpCtx, target: DumpTarget) {
    let d = ctx.dump;
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
        .title(tr!("dump-sql-dialog"))
        .default_name(format!("{}.sql", target.database))
        .allowed_types(vec![FileSpec {
            name: "SQL",
            extensions: &["sql"],
        }]);
    let run = ctx.run.clone();
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
        (run)(
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
                // A failure is `export::export_failure_note`'s sentence (worded
                // by `i18n::export_failure_note`): it makes
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
                //
                // Its arms are `dump::cancel_note`'s, decided and tested there and
                // worded here by `dump_cancel_note`, for the reason `dump_verdict`
                // is not written out in this file: the sentence had two halves
                // and only one of them was ever said. A cancel during the schema
                // read creates no `.part` at all, and this pointed at one
                // regardless.
                match outcome {
                    // **Through `export_note`, which is where a caveat is
                    // worded.** The tally carries what the file could not hold —
                    // binary columns written as `NULL`, values past the arena
                    // ceiling left blank — and a green "Wrote 5 tables." over a
                    // file whose every blob is `NULL` is the failure this whole
                    // sentence exists to prevent. The table count is this path's
                    // own; the rest is the same sentence the grid's bar shows.
                    // What the file is short of, and what a replay of it will
                    // not replace, in the same sentence as the tally — see
                    // `dump::done_note`.
                    DumpOutcome::Done {
                        tables,
                        tally,
                        missing,
                        refused,
                        left_out,
                        rows_left_out,
                    } => d.done.set(Some(dump_done_note(
                        tables,
                        schemaic_core::export::export_said(&tally, &name, true).as_ref(),
                        &missing,
                        &left_out,
                        &rows_left_out,
                        &refused,
                    ))),
                    DumpOutcome::Cancelled { partial } => {
                        d.error.set(Some(dump_cancel_note(&name, partial)))
                    }
                    DumpOutcome::Failed { message, partial } => {
                        d.error.set(Some(crate::i18n::export_failure_note(
                            &export_failure_text(&message),
                            partial.then_some(name.as_str()),
                        )))
                    }
                }
            }),
        );
    });
}

/// Launch a **folder** export, after the directory dialog names a folder.
///
/// [`run_dump`]'s twin for the picker's five non-SQL formats, and every guard in
/// it is here for the reason stated there: the selection is read *after* the
/// dialog closes because floem's dialog is not window-modal and the modal stays
/// live behind it; the launch is accepted through `accept_dialog_launch` because
/// by then the modal that asked may be gone; and the outcome checks `generation`
/// because closing the modal does not stop the export.
///
/// What it does **not** carry is [`schemaic_core::dump::DumpOptions`]. A CSV has
/// no `CREATE TABLE` to drop first, no transaction to wrap and no foreign-key
/// guard to lower, so there is nothing for the six toggles to say — which is why
/// the modal hides them for these formats rather than passing values the writer
/// would ignore.
fn run_files(ctx: &DumpCtx, target: DumpTarget) {
    let d = ctx.dump;
    let ctx = ctx.clone();
    if d.chosen.with_untracked(|c| c.is_empty()) {
        return;
    }
    let dialog = FileDialogOptions::new()
        // A directory, not a file: this export writes one file *per table*, and
        // the names are `dump::file_plan`'s to choose — they have to be, because
        // two tables can sanitize to one file name and only the plan sees the
        // whole set.
        .select_directories()
        .title(tr!("dump-folder-dialog", format = target.format.label()));
    let asked_at = d.generation.get_untracked();
    let format = target.format;
    floem::action::open_file(dialog, move |file| {
        let Some(folder) = file.and_then(|f| f.path.first().cloned()) else {
            return; // The dialog was dismissed.
        };
        if !crate::widgets::accept_dialog_launch(
            d.running.get_untracked(),
            false,
            d.target.get_untracked().is_some(),
            asked_at,
            d.generation.get_untracked(),
        ) {
            return;
        }
        // The selection as it stands *now* — the tables may have been unticked
        // while the dialog stood open. `run_dump`'s rule, and its bug.
        let tables = d.chosen.get_untracked();
        if tables.is_empty() {
            return;
        }
        // The folder's own name, for the three sentences below: they are about
        // *this* folder and the outcome carries none of it, so the modal is the
        // only place that still knows. The last component rather than the whole
        // path — the path can be long enough to push the caveats off the footer,
        // and the user chose this folder a moment ago.
        let name = folder
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| folder.display().to_string());
        launch_files(
            &ctx,
            FilesRequest {
                folder,
                conn_id: target.conn_id,
                database: target.database.clone(),
                tables,
                format,
                dialect: target.dialect,
                // **Never true here.** The collision list is only knowable off
                // the UI thread, so the first run reports `WouldReplace` and the
                // confirm re-launches — see `FilesRequest::approved`.
                approved: None,
            },
            name,
        );
    });
}

/// Start a folder export and report its outcome into the modal.
///
/// **Split out of [`run_files`] so it can be re-entered**, which is what the
/// replace confirm needs: a directory picker has no "replace?" prompt and the
/// per-table names are `file_plan`'s to choose, so the export's first run
/// computes the collisions, writes nothing, and comes back
/// [`FilesOutcome::WouldReplace`]. Yes re-launches through here with the request
/// approved, which re-reads the folder — the more correct answer anyway, since
/// it may have changed while the question stood.
///
/// The re-launch takes the **same guard** the picker's own launch takes:
/// `accept_dialog_launch`, with the generation captured before the question, so
/// a confirm answered after the modal moved to another database cannot start an
/// export into it.
fn launch_files(ctx: &DumpCtx, req: FilesRequest, name: String) {
    let d = ctx.dump;
    let files = ctx.files.clone();
    d.running.set(true);
    d.error.set(None);
    d.done.set(None);
    d.progress.set(None);
    let opened = d.generation.get_untracked();
    {
        let ctx_retry = ctx.clone();
        let req_retry = req.clone();
        let name_retry = name.clone();
        (files)(
            req,
            Rc::new(move |outcome| {
                if d.generation.get_untracked() != opened {
                    return;
                }
                d.running.set(false);
                d.progress.set(None);
                // All three sentences' arms are `core::export`'s `files_*`, beside
                // the ones the dump and the grid use — a report about a folder is
                // a decision with arms (nothing finished, some finished, a table
                // ticked and never found), and it is not one to make inside a
                // callback the suite cannot reach. The `files_*_note`s below word
                // them, held to core's English by `core_words_tests`.
                match outcome {
                    FilesOutcome::Done {
                        files,
                        tally,
                        missing,
                        replaced,
                    } => d.done.set(Some(files_done_note(
                        files, &tally, &name, &missing, &replaced,
                    ))),
                    FilesOutcome::Cancelled {
                        files,
                        missing,
                        replaced,
                    } => d
                        .error
                        .set(Some(files_cancel_note(files, &name, &missing, &replaced))),
                    FilesOutcome::Failed {
                        message,
                        files,
                        missing,
                        replaced,
                    } => d.error.set(Some(files_failure_note(
                        &export_failure_text(&message),
                        files,
                        &name,
                        &missing,
                        &replaced,
                    ))),
                    // **Nothing was written**, so this is a question and not a
                    // report. Its English is `core::dump::folder_replace_prompt`'s,
                    // worded here by `folder_replace_prompt` as the three notes
                    // above are.
                    FilesOutcome::WouldReplace { replaced } => {
                        let ctx = ctx_retry.clone();
                        let req = req_retry.clone();
                        let name = name_retry.clone();
                        ctx_retry.overlay.confirm.set(Some(crate::Confirm {
                            title: tr!("dump-replace-files"),
                            message: folder_replace_prompt(&name, &replaced),
                            resolve: Rc::new(move |yes| {
                                if !yes {
                                    return;
                                }
                                if !crate::widgets::accept_dialog_launch(
                                    d.running.get_untracked(),
                                    false,
                                    d.target.get_untracked().is_some(),
                                    opened,
                                    d.generation.get_untracked(),
                                ) {
                                    return;
                                }
                                launch_files(
                                    &ctx,
                                    // The list the prompt named, which is what
                                    // the re-run's census is compared against.
                                    req.clone().approved(replaced.clone()),
                                    name.clone(),
                                );
                            }),
                        }));
                    }
                }
            }),
        );
    }
}

/// The dump's two option sections — what goes in the file, and what the file
/// does when it is replayed.
///
/// **Only ever built for [`ExportFormat::Sql`]**, which is why it is a function:
/// the rows are `focusable_toggle_row`s, so building them for a folder export
/// and then not showing them would leave eight tab stops behind controls that
/// are not on screen. The caller's branch decides; this only knows how to draw
/// them.
fn dump_options(d: crate::DumpUi, dialect: SqlDialect, ring: FocusRing) -> impl IntoView {
    let what = v_stack((
        form_section_header(tr!("dump-section-contents")),
        crate::settings::focusable_toggle_row(
            tr!("dump-structure"),
            tr!("dump-structure-hint"),
            d.structure,
            ring.clone(),
            TAB_OPTS,
        ),
        crate::settings::focusable_toggle_row(
            tr!("dump-data"),
            tr!("dump-data-hint"),
            d.data,
            ring.clone(),
            TAB_OPTS + 10,
        ),
        crate::settings::focusable_toggle_row(
            tr!("dump-other-objects"),
            tr!("dump-other-objects-hint"),
            d.other_objects,
            ring.clone(),
            TAB_OPTS + 20,
        ),
    ))
    .style(|s| s.flex_col().width_full().gap(theme::scaled(10.0)));

    // The guard is offered only where the engine has one an ordinary role can
    // throw — asked of the dialect through the core predicate, never spelled out
    // again here.
    let has_guard = schemaic_core::dump::fk_guard_sql(dialect).is_some();
    let replay = v_stack((
        form_section_header(tr!("dump-section-replaying")),
        crate::settings::focusable_toggle_row(
            tr!("dump-drop-before-create"),
            // What it drops differs by engine — see the core function.
            drop_hint_text(schemaic_core::dump::drop_hint(dialect)),
            d.drop_if_exists,
            ring.clone(),
            TAB_OPTS + 30,
        ),
        crate::settings::focusable_toggle_row(
            tr!("dump-one-transaction"),
            tr!("dump-one-transaction-hint"),
            d.wrap_transaction,
            ring.clone(),
            TAB_OPTS + 40,
        ),
        if has_guard {
            crate::settings::focusable_toggle_row(
                tr!("dump-disable-fk-checks"),
                fk_hint_text(schemaic_core::dump::fk_guard_says_drops(dialect)),
                d.disable_fk_checks,
                ring.clone(),
                TAB_OPTS + 50,
            )
            .into_any()
        } else {
            // Said, rather than shown greyed: PostgreSQL's switch is
            // superuser-only, so offering it would be a checkbox that fails the
            // restore for most roles.
            form_hint(tr!("dump-no-fk-guard-hint")).into_any()
        },
    ))
    .style(|s| s.flex_col().width_full().gap(theme::scaled(10.0)));

    v_stack((
        form_separator(|| theme::scaled(16.0)),
        what,
        form_separator(|| theme::scaled(16.0)),
        replay,
    ))
    .style(|s| s.flex_col().width_full().gap(theme::scaled(16.0)))
}

/// One table row: the app's [`crate::widgets::check_box`], then the table name.
fn table_row(
    d: crate::DumpUi,
    chosen: Memo<std::collections::HashSet<String>>,
    name: String,
) -> impl IntoView {
    // **A set, and `hide()` rather than a rebuild.** Every row watches the
    // selection, so a single tick used to cost one linear `contains` *per row*
    // over the whole selection and one view rebuild each — quadratic, and
    // visibly so at the few hundred tables the perf fixtures carry. The memo
    // makes each row's read O(1), and the box is filled and emptied by style,
    // which is the codebase's rule for a reactive show-hide: nothing is
    // constructed, so nothing takes the row apart under the pointer.
    let is_in = {
        let name = name.clone();
        move || chosen.with(|c| c.contains(&name))
    };
    let toggle_name = name.clone();
    h_stack((
        crate::widgets::check_box(is_in),
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
fn table_picker(d: DumpUi, ring: FocusRing) -> impl IntoView {
    // The selection as a set, computed once per change instead of once per row:
    // the list is the one signal every row in it reads. `chosen` stays a `Vec`
    // because it is what the request carries and what "Select all" resets from.
    let chosen_set = create_memo(move |_| {
        d.chosen
            .with(|c| c.iter().cloned().collect::<std::collections::HashSet<_>>())
    });
    let count = move || {
        let (a, b) = (d.chosen.with(|c| c.len()), d.tables.with(|t| t.len()));
        if a == b {
            tr!("dump-all-selected", total = b)
        } else {
            tr!("dump-some-selected", chosen = a, total = b)
        }
    };
    // Links, not buttons, and the import list's pair verbatim — same words, same
    // colours, same order: they adjust the list the user is reading rather than
    // answering the modal, whose two real buttons are in the footer.
    let head = h_stack((
        label(count).style(|s| s.color(theme::text_dim()).font_size(theme::font_label())),
        empty().style(|s| s.flex_grow(1.0_f32)),
        link_button(
            tr!("dump-select-all"),
            theme::accent,
            ring.clone(),
            TAB_ALL,
            move || d.chosen.set(d.tables.get_untracked()),
        ),
        link_button(
            tr!("dump-select-none"),
            theme::text_muted,
            ring,
            TAB_NONE,
            move || d.chosen.set(Vec::new()),
        ),
    ))
    .style(|s| s.items_center().width_full().gap(theme::scaled(10.0)));

    // The four states are `dump::picker_body`'s, matched exhaustively: a second
    // `if names.is_empty()` here is what printed "This database has no tables."
    // over a listing that never finished, four lines above the error explaining
    // why the list was empty.
    let list = dyn_container(
        move || (d.listing.get(), d.tables.get()),
        move |(listing, names)| {
            let note = |s: String| {
                text(s)
                    .style(|s| s.color(theme::text_muted()).font_size(theme::font_body()))
                    .into_any()
            };
            match schemaic_core::dump::picker_body(listing, names.len()) {
                PickerBody::Reading => note(tr!("dump-reading-tables")),
                PickerBody::NoTables => note(tr!("dump-no-tables")),
                PickerBody::Unreadable => note(tr!("dump-tables-unreadable")),
                PickerBody::Tables => {
                    v_stack_from_iter(names.into_iter().map(move |n| table_row(d, chosen_set, n)))
                        .style(|s| s.flex_col().width_full())
                        .into_any()
                }
            }
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
pub(crate) fn dump_overlay(ctx: DumpCtx) -> impl IntoView {
    let d = ctx.dump;
    // Built once, with the modal rather than with each open: this view is
    // constructed at startup and the `dyn_container` below is what appears and
    // disappears, so an effect created here has the lifetime the modal does.
    watch_connection(d, ctx.conn);
    // One decision for every exit — footer, Escape, ✕ — so none of them can
    // disagree. While an export runs they **stop** it rather than closing, the
    // import modal's rule and for its reason: closing would hide a write that is
    // still going and leave its outcome with no reader, since this modal's
    // signals are the only channel the run reports to. The footer says so in the
    // one place the user looks, by wearing the word Stop and the colour of an
    // action while that is what it does.
    let exit: Rc<dyn Fn()> = {
        let stop = ctx.cancel.clone();
        Rc::new(move || match exit_action(d.running.get_untracked(), true) {
            ExitAction::Close => d.target.set(None),
            ExitAction::Cancel => (stop)(),
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
            let (exit_x, exit_esc, exit_footer) = (exit.clone(), exit.clone(), exit.clone());

            // **The two option sections are the dump's alone.** Every one of them
            // is about a `.sql` file — what structure to emit, what to drop
            // first, what to wrap the load in — and a CSV can honour none of it.
            // Shown greyed they would read as options this format merely happens
            // to have turned off; absent, the panel says what it is.
            //
            // Built inside the branch, not merely hidden by it: each row is a
            // `focusable_toggle_row`, so constructing them for a folder export
            // would leave eight tab stops behind controls that are not on screen.
            let folder = target.writes_folder();
            let options: floem::AnyView = if folder {
                // What replaces them: the one sentence a folder export needs,
                // since it has no options and its destination is not a file.
                form_hint(tr!("dump-folder-hint", format = target.format.label())).into_any()
            } else {
                dump_options(d, target.dialect, ring.clone()).into_any()
            };

            // Choices only. **Everything the run says about itself is in the
            // footer** — this body scrolls, so a progress line or an outcome put
            // here is one the user has to go looking for, and the whole point of
            // both is being seen without looking.
            let body = v_stack((table_picker(d, ring.clone()), options))
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
            let run_ctx = ctx.clone();
            let footer_ring = ring.clone();
            let footer = dyn_container(
                move || {
                    (
                        d.running.get(),
                        d.chosen.with(|c| c.is_empty()),
                        // **Only the dump can have nothing to write.** The six
                        // toggles are what `is_empty` reads, and a folder export
                        // does not show them — so asking it here would leave the
                        // Export button gated on controls that are not on screen
                        // and whose last values came from someone else's dump.
                        !folder && d.options().is_empty(),
                        d.done.get(),
                        d.error.get(),
                    )
                },
                move |(running, none_chosen, no_content, done, error)| {
                    let (run_ctx, ring) = (run_ctx.clone(), footer_ring.clone());
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
                                Some(p) => text(tr!(
                                    "dump-progress",
                                    table = p.table.clone(),
                                    index = p.index,
                                    total = p.total,
                                    rows = crate::i18n::count(p.rows as usize),
                                ))
                                .style(|s| {
                                    s.color(theme::text_muted())
                                        .font_size(theme::font_label())
                                        .min_width(0.0)
                                        .text_ellipsis()
                                })
                                .into_any(),
                                // Before the first table there is nothing to count
                                // yet, and the schema read is the slow part on a
                                // large database — animated, because a static
                                // label and a hung one look identical.
                                None => crate::widgets::loading_dots(
                                    tr!("dump-reading-schema"),
                                    theme::text_muted,
                                    theme::font_label,
                                )
                                .into_any(),
                            },
                        )
                        .into_any()
                    } else {
                        empty().into_any()
                    };
                    // **The outcome gets a row of its own, wrapping.** A done
                    // note names the ticked tables that were not found and the
                    // objects left out of the file, and a failure names the
                    // `.part` path and the OS error: ellipsized beside the
                    // buttons, a partial backup read as a green success with the
                    // rest on hover. The progress line stays one line above.
                    //
                    // The done note is built at the callback, where the file's
                    // name was still in scope — `export_note` names the file and
                    // every column the export could not carry, in the grid's own
                    // wording.
                    let outcome: Option<(String, fn() -> floem::peniko::Color)> = if running {
                        None
                    } else if let Some(e) = error {
                        Some((e, theme::error))
                    } else {
                        done.map(|msg| (msg, theme::status_ok as fn() -> floem::peniko::Color))
                    };
                    let actions = h_stack((
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
                            if running {
                                tr!("dialog-stop")
                            } else {
                                tr!("dialog-close")
                            },
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
                            tr!("dump-export"),
                            ActionKind::Primary,
                            !running && !none_chosen && !no_content,
                            ring,
                            ACTION_TAB + 10,
                            move || run_export(&run_ctx),
                        ),
                    ))
                    .style(|s| s.items_center().gap(theme::scaled(8.0)))
                    .into_any();
                    match outcome {
                        Some((msg, color)) => crate::widgets::modal_footer_noted(
                            text(msg)
                                .style(move |s| s.color(color()).font_size(theme::font_label())),
                            actions,
                        )
                        .into_any(),
                        None => modal_footer_split(left, actions).into_any(),
                    }
                },
            )
            .style(|s| s.width_full());

            let close_x: Rc<dyn Fn()> = exit_x.clone();
            // **The format is in the title**, because it is the one thing about
            // this modal the picker below cannot show and the user cannot change
            // from here — six submenu entries open the same panel, and without
            // it the CSV one and the dump are the same window.
            let title = dump_title(&target.database, target.schema.as_deref(), target.format);
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

            // **No click-away, on purpose**: while the dump runs a close cancels
            // it, which is not for a stray click. ✕ and Escape close it. See
            // docs/architecture.md, *which modals close on a backdrop click*.
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

/// The dump modal's title — two whole messages, with the namespace and without.
/// Why the format is in it is said where it is called.
fn dump_title(database: &str, schema: Option<&str>, format: ExportFormat) -> String {
    match schema {
        Some(ns) => tr!(
            "dump-title-schema",
            database = database,
            schema = ns,
            format = format.label()
        ),
        None => tr!("dump-title", database = database, format = format.label()),
    }
}

/// The export modal's count while it runs: `rows` written so far, out of
/// `total` when it is known.
///
/// The `~` is the *estimated* case only — a streamed export is re-reading the
/// server and carries the catalogue's guess, the same mark and the same figure
/// the `All rows` entry the user chose from shows. A fetched export knows
/// exactly what it holds, and dressing that up as an estimate would hedge a
/// number that needs no hedging.
fn export_count_line(rows: u64, total: Option<u64>, approx: bool) -> String {
    let shown = crate::i18n::count(rows as usize);
    match total {
        Some(total) => {
            let total = crate::i18n::count(total as usize);
            if approx {
                tr!("export-progress-of-about", rows = shown, total = total)
            } else {
                tr!("export-progress-of", rows = shown, total = total)
            }
        }
        // The count decides the plural; the abbreviated figure is what is shown.
        None => tr!("export-progress-rows", count = rows, rows = shown),
    }
}

/// The grid export's progress modal — raised by **every** grid export, both
/// scopes.
///
/// **Here rather than in `grid.rs`, because it is this panel's twin.** It is the
/// dump modal's footer with nothing above it: the same running line, the same
/// red Stop in the same fixed position, the same backdrop. A user who exports a
/// database from the tree and a result from the grid meets one thing twice, and
/// keeping the two in one file is what makes that survive the next change to
/// either.
///
/// It is the export's **single affordance**: progress and a Stop while the write
/// runs, then the outcome and a Close. It does not close itself — see
/// [`crate::ExportUi`] for why a modal that did could not confirm a fast export
/// at all, and why the grid's bar no longer reports one.
pub(crate) fn export_progress_overlay(e: ExportUi, cancel: Rc<dyn Fn()>) -> impl IntoView {
    // **One decision for every exit** — the footer button and Escape — so the two
    // cannot disagree, which is the rule `dump_overlay` states and the reason it
    // routes both through one closure. Here it matters more than there: the two
    // exits mean *opposite* things depending on the state, and an Escape wired
    // straight to `target.set(None)` would silently abandon a running export,
    // leaving a stream writing a `.part` sibling with its Stop nowhere on screen.
    let exit: Rc<dyn Fn()> = {
        let cancel = cancel.clone();
        Rc::new(move || {
            let finished =
                e.done.with_untracked(Option::is_some) || e.error.with_untracked(Option::is_some);
            if finished {
                e.target.set(None);
                e.progress.set(None);
                e.done.set(None);
                e.error.set(None);
            } else {
                // Stop. The modal stays up and turns into its finished state
                // when the cancel is reported, so the user sees what became of
                // the file rather than the window simply vanishing.
                (cancel)();
            }
        })
    };
    dyn_container(
        // The **whole** state, not just `target`: the panel is rebuilt when the
        // run ends so the footer can change word and colour. Reading only
        // `target` left a finished export showing a red Stop over a file that
        // was already written.
        move || (e.target.get(), e.done.get(), e.error.get()),
        move |(target, done, error)| {
            let Some(target) = target else {
                return empty().into_any();
            };
            let ring = FocusRing::new();
            let (exit_btn, exit_esc) = (exit.clone(), exit.clone());
            let finished = done.is_some() || error.is_some();
            let (total, approx) = (target.total, target.approx);
            let outcome: Option<(String, bool)> = match (done, error) {
                // The error arm wins if both are somehow set: a failure the user
                // has to act on must not be hidden behind a success.
                (_, Some(e)) => Some((e, false)),
                (Some(d), None) => Some((d, true)),
                (None, None) => None,
            };
            let body = v_stack((
                dyn_container(
                    move || (e.progress.get(), outcome.clone()),
                    move |(rows, outcome)| match outcome {
                        // **Finished: what became of the file, in place of the
                        // count.** Green for a clean write, the error colour for
                        // a failure *or a cancel* — a stopped export's sentence
                        // is about a file that is not what was asked for, and a
                        // green tick over it is the one reading it must not get.
                        // **Capped, or it paints out of the modal.** The column
                        // in this v_stack is `items_center()`, which sizes a
                        // child to its own content rather than stretching it —
                        // so a sentence longer than the panel laid out at its
                        // natural width and ran past both borders instead of
                        // wrapping. `max_width_pct(100)` bounds it to the
                        // content box (floem's default `TextOverflow` is `Wrap`,
                        // so bounding it *is* the wrap) and `min_width(0)` is
                        // what lets it shrink to that bound; the cap rather than
                        // `width_full` because a one-line note stays centred.
                        // The caveat clauses name columns, so there is no length
                        // this can be trusted to stay under.
                        Some((msg, ok)) => text(msg)
                            .style(move |s| {
                                s.color(if ok {
                                    theme::status_ok()
                                } else {
                                    theme::error()
                                })
                                .font_size(theme::font_label())
                                .max_width_pct(100.0)
                                .min_width(0.0)
                            })
                            .into_any(),
                        // **What has been written, not that something is being
                        // written.** The count is the difference between an
                        // export that looks stuck and one visibly grinding
                        // through a large table — the dump's line, and its
                        // reason.
                        None => match rows {
                            Some(rows) => text(export_count_line(rows, total, approx))
                                .style(|s| s.color(theme::text()).font_size(theme::font_body()))
                                .into_any(),
                            // Before the first block there is nothing to count,
                            // and the server's first read is the slow part —
                            // animated, because a static label and a hung one
                            // look identical.
                            None => crate::widgets::loading_dots(
                                tr!("export-progress-starting"),
                                theme::text_muted,
                                theme::font_label,
                            )
                            .into_any(),
                        },
                    },
                ),
                // The destination, under whichever line is above it. **Past
                // tense once the run is over**: `Writing orders.csv` beside
                // `Exported 16k rows` would describe a file still being written
                // and one already finished, in the same breath.
                text(if finished {
                    tr!("export-progress-file", name = target.name.clone())
                } else {
                    tr!("export-progress-writing", name = target.name.clone())
                })
                // Bounded for the reason the sentence above it is: a long file
                // name is as able to paint past the panel as a long caveat.
                .style(|s| {
                    s.color(theme::text_muted())
                        .font_size(theme::font_label())
                        .max_width_pct(100.0)
                        .min_width(0.0)
                }),
            ))
            .style(|s| {
                s.flex_col()
                    .items_center()
                    .width_full()
                    .gap(theme::scaled(6.0))
                    .padding_horiz(modal_pad_h())
                    .padding_vert(theme::scaled(22.0))
            });

            let panel = v_stack((
                body,
                modal_footer_split(
                    empty().into_any(),
                    // **One button in one place, wearing the word for the state
                    // it is in** — the dump footer's rule. Red while it stops
                    // something, because that is an action rather than a way out;
                    // neutral once it is only a way out. Keeping the slot fixed
                    // is what stops the control moving under the cursor when the
                    // line above it changes width.
                    action_button(
                        if finished {
                            tr!("dialog-close")
                        } else {
                            tr!("dialog-stop")
                        },
                        if finished {
                            ActionKind::Neutral
                        } else {
                            ActionKind::Danger
                        },
                        true,
                        ring.clone(),
                        ACTION_TAB,
                        move || (exit_btn)(),
                    )
                    .into_any(),
                )
                .into_any(),
            ))
            .on_click_stop(|_| {})
            .style(move |s| panel_style(s).width(modal_w(360.0)));

            // Escape routes through the **same** decision as the button, so it
            // stops a running export and dismisses a finished one — never the
            // other way round, and never a close that abandons a live write.
            // **No title row and no click-away, by design**: a one-button
            // progress box whose only exit is that decision. See
            // docs/architecture.md, *which modals close on a backdrop click*.
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
        if e.target.get().is_some() {
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
fn watch_connection(d: DumpUi, conn: ConnUi) {
    let active = conn.active_conn;
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

// ── Core's export sentences, in the interface's language ────────────────────
// Each takes the arguments core's own takes (`dump::done_note`,
// `dump::cancel_note`, `dump::folder_replace_prompt`, `export::files_*`) and
// reads in English exactly as it does — the tests below hold them together.
// Sentences are joined by a space in code, as core joins them.

/// `dump::done_note` in the interface's language.
fn dump_done_note(
    tables: usize,
    note: Option<&schemaic_core::export::ExportNote>,
    missing: &[String],
    left_out: &[String],
    rows_left_out: &[String],
    refused: &[String],
) -> String {
    let mut parts = vec![tr!("dump-done-tables", n = tables)];
    parts.extend(note.map(|n| tr!("dump-done-export", note = crate::i18n::export_note(n))));
    if !missing.is_empty() {
        let (n, names) = (missing.len(), missing.join(", "));
        parts.push(tr!("dump-done-missing", n = n, names = names));
    }
    if !left_out.is_empty() {
        let (n, names) = (left_out.len(), left_out.join(", "));
        parts.push(tr!("dump-done-left-out", n = n, names = names));
    }
    if !rows_left_out.is_empty() {
        let (n, names) = (rows_left_out.len(), rows_left_out.join(", "));
        parts.push(tr!("dump-done-rows-left-out", n = n, names = names));
    }
    if !refused.is_empty() {
        let (n, names) = (refused.len(), refused.join(", "));
        parts.push(tr!("dump-done-refused", n = n, names = names));
    }
    parts.join(" ")
}

/// The *Drop before create* toggle's hint — `dump::DropHint` in the
/// interface's language.
fn drop_hint_text(hint: schemaic_core::dump::DropHint) -> String {
    use schemaic_core::dump::DropHint;
    match hint {
        DropHint::UpFront => tr!("dump-drop-hint-up-front"),
        DropHint::RoutinesInPlace => tr!("dump-drop-hint-routines-in-place"),
        DropHint::RoutinesDroppedFirst => tr!("dump-drop-hint-routines-dropped-first"),
        DropHint::TablesOnly => tr!("dump-drop-hint-tables-only"),
    }
}

/// The *Disable foreign-key checks* toggle's hint — `dump::FkHint` in the
/// interface's language.
fn fk_hint_text(hint: schemaic_core::dump::FkHint) -> String {
    use schemaic_core::dump::FkHint;
    match hint {
        FkHint::Plain => tr!("dump-fk-hint"),
        FkHint::DropsTurnItOff => tr!("dump-fk-hint-drops"),
    }
}

/// Why a dump or a folder export stopped short — `dump::ExportFailure` in the
/// interface's language. Each already says it failed; nothing adds a prefix.
fn export_failure_text(failure: &schemaic_core::dump::ExportFailure) -> String {
    use schemaic_core::dump::ExportFailure;
    match failure {
        ExportFailure::Failed(e) => tr!("export-failed", error = e.as_str()),
        ExportFailure::WorkerDied(e) => tr!("export-worker-died", error = e.as_str()),
        ExportFailure::NotPublished { path, error } => tr!(
            "export-not-published",
            path = path.as_str(),
            error = error.as_str()
        ),
        ExportFailure::NotRenamed { part, path, error } => tr!(
            "export-not-renamed",
            part = part.as_str(),
            path = path.as_str(),
            error = error.as_str()
        ),
        ExportFailure::NothingMatched => tr!("export-nothing-matched"),
        ExportFailure::Cancelled => tr!("export-stopped"),
        ExportFailure::Worded(s) => s.clone(),
    }
}

/// `dump::cancel_note` in the interface's language.
fn dump_cancel_note(name: &str, partial: bool) -> String {
    if partial {
        tr!(
            "dump-cancelled-partial",
            name = name,
            part = schemaic_core::export::part_path(name)
        )
    } else {
        tr!("export-cancelled-nothing", name = name)
    }
}

/// `export::files_note` in the interface's language.
fn files_done_note(
    files: usize,
    tally: &schemaic_core::export::ExportTally,
    folder: &str,
    missing: &[String],
    replaced: &[String],
) -> String {
    let head = tr!("files-done", n = files);
    let mut s = match schemaic_core::export::export_said(tally, folder, true) {
        Some(note) => tr!(
            "files-done-with-note",
            head = head,
            note = crate::i18n::export_note(&note)
        ),
        None => head,
    };
    files_tail(&mut s, missing, replaced);
    s
}

/// `export::files_cancel_note` in the interface's language.
fn files_cancel_note(
    files: usize,
    folder: &str,
    missing: &[String],
    replaced: &[String],
) -> String {
    let mut s = if files == 0 {
        tr!("files-cancelled-none", folder = folder)
    } else {
        tr!("files-cancelled", n = files, folder = folder)
    };
    files_tail(&mut s, missing, replaced);
    s
}

/// `export::files_failure_note` in the interface's language.
fn files_failure_note(
    message: &str,
    files: usize,
    folder: &str,
    missing: &[String],
    replaced: &[String],
) -> String {
    let mut s = if files == 0 {
        message.to_string()
    } else {
        tr!(
            "files-failed-kept",
            message = message,
            n = files,
            folder = folder
        )
    };
    files_tail(&mut s, missing, replaced);
    s
}

/// The replaced files and the tables never found, each its own sentence after
/// a folder export's first — `export`'s `replaced_clause` and `missing_clause`.
fn files_tail(s: &mut String, missing: &[String], replaced: &[String]) {
    if !replaced.is_empty() {
        s.push(' ');
        s.push_str(&tr!(
            "files-replaced",
            n = replaced.len(),
            names = replaced.join(", ")
        ));
    }
    if !missing.is_empty() {
        s.push(' ');
        s.push_str(&tr!(
            "files-missing",
            n = missing.len(),
            names = missing.join(", ")
        ));
    }
}

/// `dump::folder_replace_prompt` in the interface's language.
fn folder_replace_prompt(folder: &str, replaced: &[String]) -> String {
    use schemaic_core::dump::FOLDER_REPLACE_SHOWN;
    let n = replaced.len();
    let names = replaced
        .iter()
        .take(FOLDER_REPLACE_SHOWN)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    match n.saturating_sub(FOLDER_REPLACE_SHOWN) {
        0 => tr!("dump-replace-prompt", n = n, folder = folder, names = names),
        more => tr!(
            "dump-replace-prompt-more",
            n = n,
            folder = folder,
            names = names,
            more = more
        ),
    }
}

#[cfg(test)]
mod core_words_tests {
    use super::{
        dump_cancel_note, dump_done_note, files_cancel_note, files_done_note, files_failure_note,
        folder_replace_prompt,
    };
    use schemaic_core::export::{ExportTally, export_note, export_said};
    use schemaic_core::{dump, export};

    fn names(n: usize, stem: &str) -> Vec<String> {
        (1..=n).map(|i| format!("{stem}{i}")).collect()
    }

    fn tally() -> ExportTally {
        ExportTally {
            rows: 115_000,
            withheld: vec!["avatar".into()],
            ..Default::default()
        }
    }

    /// **A dump's done and cancel lines read in English exactly as core's** —
    /// every list empty, one and several, with and without the export's note.
    #[test]
    fn english_says_every_dump_outcome_as_core_does() {
        for tables in [1, 5] {
            for k in [0, 1, 3] {
                for with_note in [false, true] {
                    let note = with_note.then(|| export_said(&tally(), "shop.sql", true).unwrap());
                    let english =
                        with_note.then(|| export_note(&tally(), "shop.sql", true).unwrap());
                    let (m, l, r, f) = (
                        names(k, "ghost"),
                        names(k, "v"),
                        names(k, "edge"),
                        names(k, "t"),
                    );
                    assert_eq!(
                        dump_done_note(tables, note.as_ref(), &m, &l, &r, &f),
                        dump::done_note(tables, english.as_deref(), &m, &l, &r, &f),
                        "{tables} {k} {with_note}"
                    );
                }
            }
        }
        for partial in [false, true] {
            assert_eq!(
                dump_cancel_note("shop.sql", partial),
                dump::cancel_note("shop.sql", partial)
            );
        }
    }

    /// **The replay toggles' hints read in English exactly as core's**, every
    /// variant.
    #[test]
    fn english_hints_every_replay_toggle_as_core_does() {
        use schemaic_core::dump::{DropHint, FkHint};
        for hint in [
            DropHint::UpFront,
            DropHint::RoutinesInPlace,
            DropHint::RoutinesDroppedFirst,
            DropHint::TablesOnly,
        ] {
            assert_eq!(super::drop_hint_text(hint), hint.text(), "{hint:?}");
        }
        for hint in [FkHint::Plain, FkHint::DropsTurnItOff] {
            assert_eq!(super::fk_hint_text(hint), hint.text(), "{hint:?}");
        }
    }

    /// **Why an export stopped short reads in English exactly as core's
    /// `ExportFailure` says it**, every variant — the CLI prints that one.
    #[test]
    fn english_says_every_export_failure_as_core_does() {
        use schemaic_core::dump::ExportFailure;
        for failure in [
            ExportFailure::Failed("No space left on device (os error 28)".into()),
            ExportFailure::WorkerDied("panicked".into()),
            ExportFailure::NotPublished {
                path: "out/orders.csv".into(),
                error: "Access is denied.".into(),
            },
            ExportFailure::NotRenamed {
                part: "shop.sql.part".into(),
                path: "shop.sql".into(),
                error: "in use".into(),
            },
            ExportFailure::NothingMatched,
            ExportFailure::Cancelled,
            ExportFailure::Worded("Dump failed: refused".into()),
        ] {
            assert_eq!(
                super::export_failure_text(&failure),
                failure.to_string(),
                "{failure:?}"
            );
        }
    }

    /// The same for a folder export's done, cancelled and failed lines, and
    /// the question asked before it replaces files — up to and past the eight
    /// it names.
    #[test]
    fn english_says_every_folder_outcome_as_core_does() {
        let message = "Export failed: No space left on device (os error 28)";
        for files in [0, 1, 12] {
            for k in [0, 1, 2] {
                let (m, r) = (names(k, "ghost"), names(k, "f"));
                assert_eq!(
                    files_done_note(files, &tally(), "out", &m, &r),
                    export::files_note(files, &tally(), "out", &m, &r),
                );
                assert_eq!(
                    files_cancel_note(files, "out", &m, &r),
                    export::files_cancel_note(files, "out", &m, &r),
                );
                assert_eq!(
                    files_failure_note(message, files, "out", &m, &r),
                    export::files_failure_note(message, files, "out", &m, &r),
                );
            }
        }
        for n in [1, 2, 8, 9, 20] {
            let replaced = names(n, "t");
            assert_eq!(
                folder_replace_prompt("out", &replaced),
                dump::folder_replace_prompt("out", &replaced),
                "{n}"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{dump_title, export_count_line};
    use crate::i18n::tr;
    use schemaic_core::export::ExportFormat;
    use schemaic_core::text::human_count;

    /// The lines this file composes read as they did in English before the
    /// catalogue. The counts are spelled through `human_count`, as the old
    /// `format!`s were, so the test pins the sentence and not the abbreviation.
    #[test]
    fn the_composed_lines_read_as_they_did_in_english() {
        assert_eq!(
            dump_title("shop", Some("sales"), ExportFormat::Csv),
            "Export shop.sales to CSV"
        );
        assert_eq!(
            dump_title("shop", None, ExportFormat::Sql),
            "Export shop to SQL"
        );
        assert_eq!(tr!("dump-all-selected", total = 12), "All 12 selected");
        assert_eq!(
            tr!("dump-some-selected", chosen = 3, total = 12),
            "3 of 12 selected"
        );
        assert_eq!(
            tr!(
                "dump-progress",
                table = "orders",
                index = 3,
                total = 12,
                rows = human_count(9_400)
            ),
            format!(
                "Writing orders — 3 of 12, {} rows so far",
                human_count(9_400)
            )
        );
        assert_eq!(
            export_count_line(9_400, Some(180_000), true),
            format!("{} of ~{} rows", human_count(9_400), human_count(180_000))
        );
        assert_eq!(
            export_count_line(9_400, Some(180_000), false),
            format!("{} of {} rows", human_count(9_400), human_count(180_000))
        );
        assert_eq!(export_count_line(1, None, false), "1 row");
        assert_eq!(
            export_count_line(16_000, None, false),
            format!("{} rows", human_count(16_000))
        );
        assert_eq!(
            tr!("export-progress-file", name = "orders.csv"),
            "File: orders.csv"
        );
        assert_eq!(
            tr!("export-progress-writing", name = "orders.csv"),
            "Writing orders.csv"
        );
    }
}
