//! Writing a schema + data dump: the I/O half of [`schemaic_core::dump`].
//!
//! The core module decides *what* the file holds and in what order; everything
//! here is the part that touches a server and a disk. The shape is the streamed
//! export's (`main`'s `export_file`), with one difference that drives the whole
//! module: an export is one statement into one file, and a dump is **many**
//! statements into one file, so the writer has to outlive each table.
//!
//! So the file is written by a single blocking task that reads [`Msg`]s: a
//! `Text` is written as it arrives, and a `Table` carries the *receiving end* of
//! that table's row channel, which the writer then drains through
//! [`ExportFormat::Sql`] — the same renderer the grid's SQL export uses, so a
//! dump's `INSERT`s and an export's are the same statements by construction.
//!
//! **The destination is not opened until the dump has succeeded.** Rows go to a
//! `.part` sibling that is renamed over the target at the end, which is atomic
//! because it is a sibling. A cancelled or failed dump leaves the fragment in the
//! sibling and the user's file untouched — the same guarantee, and the same
//! reasoning, as the export path.

use std::path::{Path, PathBuf};

use schemaic_core::dump::{DumpStep, DumpVerdict, ReadEnd, WriteEnd, dump_verdict, plan};
use schemaic_core::export::{ExportFormat, ExportTally, PullChunks};
use schemaic_db::{Db, DbError, ExportChunk};
use schemaic_ui::{DumpOutcome, DumpProgress, DumpRequest, FilesOutcome, FilesRequest};
use tokio_util::sync::CancellationToken;

/// A writer failure, and whether the `.part` file exists to name in the note.
///
/// **`opened` is the fact `DumpVerdict::Failed::partial` is about**, and nothing
/// carried it: `File::create` is each writer's first statement, so a read-only
/// folder or a full volume fails before any fragment exists — and the note told
/// the user "the rows that were written are in shop.sql.part" about a file that
/// had never been opened.
///
/// `From<String>` is what makes this cheap: every later `?` in a writer is a
/// failure *after* the create, so it converts with `opened: true` and no site
/// changes.
#[derive(Clone, Debug)]
pub(crate) struct WriteFail {
    pub message: String,
    pub opened: bool,
}

impl From<String> for WriteFail {
    fn from(message: String) -> Self {
        WriteFail {
            message,
            opened: true,
        }
    }
}

/// What the writer task is fed, in file order.
enum Msg {
    /// SQL or a comment, written as-is.
    Text(String),
    /// A table's rows: the `(database, schema, table)` the `INSERT`s name, and
    /// the channel they arrive on. The writer owns the receiver for as long as
    /// that one table takes.
    Table {
        source: (String, Option<String>, String),
        rows: tokio::sync::mpsc::Receiver<ExportChunk>,
    },
}

/// The `.part` sibling a dump is built in.
///
/// **The suffix comes from `export::part_path`**, the one function that decides
/// it, because the modal tells the user where the fragment went through that
/// same function. Spelling `.part` again here would let the file this writes and
/// the file that message names drift apart — in the one situation where the
/// fragment is the thing the user still wants.
pub(crate) fn part_of(path: &Path) -> PathBuf {
    match path.file_name().map(|n| n.to_string_lossy().to_string()) {
        Some(name) => path.with_file_name(schemaic_core::export::part_path(&name)),
        // A path with no file name is not one we can write to anyway; the
        // `File::create` below is where that is reported.
        None => path.to_path_buf(),
    }
}

/// Run a dump to completion, reporting each table on `progress`.
///
/// Returns the outcome rather than reporting it, so the caller owns the single
/// hop back onto the UI thread.
pub(crate) async fn run(
    db: Db,
    req: DumpRequest,
    handle: tokio::runtime::Handle,
    token: CancellationToken,
    progress: crossbeam_channel::Sender<DumpProgress>,
    chunk_rows: usize,
) -> DumpOutcome {
    let failed = |message: String, partial: bool| DumpOutcome::Failed { message, partial };

    // **Freshly introspected, never the tree's cache.** A dump is a backup, and
    // a `CREATE TABLE` for a shape the server no longer has is a backup that
    // restores the wrong table.
    // The token, so **Stop really stops this phase**. It is the longest one on a
    // large database and the modal animates it behind a full backdrop whose only
    // exit is a cancel; before `fetch_schema` took a token the `Cancelled` arm
    // below was unreachable and the press did nothing until the whole read was
    // done.
    let schema = match db.fetch_schema(&req.database, token.clone()).await {
        Ok(s) => s,
        // `partial: false` — this is the whole point of the flag. The writer is
        // spawned thirty lines below, so nothing has been created and the note
        // must not point at a `.part` that does not exist.
        Err(DbError::Cancelled) => return DumpOutcome::Cancelled { partial: false },
        Err(e) => return failed(format!("Export failed: {e}"), false),
    };
    let dump = plan(&schema, &req.database, &req.tables, req.opts, req.dialect);
    if dump.steps.is_empty() {
        return failed(
            "Nothing to export — no table matched the selection.".to_string(),
            false,
        );
    }

    let (path, part) = (req.path.clone(), part_of(&req.path));
    let (tx, rx) = tokio::sync::mpsc::channel::<Msg>(1);
    let w_token = token.clone();
    let (w_path, w_part) = (path.clone(), part.clone());
    let dialect = req.dialect;
    let writer = handle.spawn_blocking(move || write(&w_part, &w_path, rx, dialect, w_token));

    // What the progress line counts against is the number of tables that will
    // actually be *streamed*, not `dump.tables`: a view has structure and no rows,
    // and a structure-only dump streams nothing at all, so counting tables would
    // promise a "12 of 12" that never arrives.
    let total = dump.streamed_tables();
    let mut index = 0usize;
    let mut rows_so_far = 0u64;
    // The reader's own failure, kept aside: the writer has to be let go of first
    // (it holds the file), and its report is the better one for anything that is
    // not a cancel — see the match at the end.
    let mut read_err: Option<DbError> = None;

    for step in dump.steps {
        match step {
            DumpStep::Text(sql) => {
                if tx.send(Msg::Text(sql)).await.is_err() {
                    break; // The writer is gone; its error is the real one.
                }
            }
            DumpStep::Rows {
                database,
                insert_database,
                schema,
                table,
                select,
            } => {
                index += 1;
                // Best-effort: a full progress channel must never hold up a dump.
                let _ = progress.send(DumpProgress {
                    index,
                    total,
                    table: table.clone(),
                    rows: rows_so_far,
                });
                // Two blocks in flight, exactly as the export path: enough for the
                // server to read the next while the disk takes the last, and small
                // enough that the queue is not the memory this streaming avoids.
                let (row_tx, row_rx) = tokio::sync::mpsc::channel::<ExportChunk>(2);
                if tx
                    .send(Msg::Table {
                        // The **target**, not the source: `select` reads from
                        // `database`, the `INSERT`s name `insert_database`.
                        source: (insert_database, schema.clone(), table.clone()),
                        rows: row_rx,
                    })
                    .await
                    .is_err()
                {
                    break;
                }
                match db
                    .stream_query(Some(&database), &select, chunk_rows, token.clone(), row_tx)
                    .await
                {
                    Ok(n) => rows_so_far += n,
                    Err(e) => {
                        read_err = Some(e);
                        break;
                    }
                }
            }
        }
    }
    // Closing the control channel is what tells the writer the file is complete —
    // so it must happen before the join, or the two wait on each other.
    drop(tx);
    let written = writer.await;

    // A cancel that arrived while no table was streaming — during the schema
    // read, or anywhere in a structure-only dump — never reaches `read_err`,
    // because nothing was reading. The writer refuses to publish in that case and
    // says so as an *error*, which would be reported as a failed dump rather than
    // a stopped one. Ask the token instead: it is the only witness either way.
    if token.is_cancelled() {
        // The writer ran, so ask *it* whether a fragment exists rather than
        // assuming one — a structure-only dump cancelled before `File::create`
        // is the case that has none.
        let partial = !matches!(&written, Ok(Err(e)) if !e.opened);
        return DumpOutcome::Cancelled { partial };
    }

    // The five-arm resolution is `core::dump::dump_verdict`'s, with tests: it is a
    // decision about which of two failures the user is told about, and written out
    // here it sat inside an `async fn` needing a `Db`, a runtime handle and two
    // channels to reach. Swapping two arms turned "The disk is full" into
    // "connection reset" with the suite green.
    let tally = match &written {
        Ok(Ok(t)) => Some(t.clone()),
        _ => None,
    };
    let read = match read_err {
        None => ReadEnd::Clean,
        Some(DbError::Cancelled) => ReadEnd::Cancelled,
        Some(e) => ReadEnd::Failed(e.to_string()),
    };
    let write = match written {
        Ok(Ok(_)) => WriteEnd::Wrote,
        Ok(Err(e)) => WriteEnd::Failed {
            message: e.message,
            opened: e.opened,
        },
        Err(e) => WriteEnd::Died(e.to_string()),
    };
    match dump_verdict(read, write) {
        DumpVerdict::Cancelled { partial } => DumpOutcome::Cancelled { partial },
        DumpVerdict::Failed { message, partial } => failed(message, partial),
        DumpVerdict::Done => DumpOutcome::Done {
            // The file's own count: every table it covers, streamed or not.
            tables: dump.tables,
            tally: tally.unwrap_or_default(),
            missing: dump.missing,
        },
    }
}

/// Run a **folder** export to completion, reporting each table on `progress`.
///
/// The sibling of [`run`] for the picker's five non-SQL formats: one file per
/// table instead of one file for the set. Every guarantee the dump makes is kept
/// per file — a freshly introspected schema, rows streamed rather than gathered,
/// a `.part` sibling renamed over the destination only once the table is whole —
/// and the one thing that changes is what "finished" means, so the outcome
/// counts files rather than describing a single one.
///
/// **The loop is sequential, one table at a time, and that is deliberate.**
/// Writing four files at once would need four connections and four in-flight
/// blocks each, and it would make the progress line ("3 of 12") a lie about what
/// is happening. A folder export is disk-bound at the end anyway.
pub(crate) async fn run_files(
    db: Db,
    req: FilesRequest,
    handle: tokio::runtime::Handle,
    token: CancellationToken,
    progress: crossbeam_channel::Sender<DumpProgress>,
    chunk_rows: usize,
) -> FilesOutcome {
    // Freshly introspected, never the tree's cache — [`run`]'s rule, for the same
    // reason: the file names and the `SELECT`s both come off this schema, and a
    // stale one exports a table that is no longer there.
    let schema = match db.fetch_schema(&req.database, token.clone()).await {
        Ok(s) => s,
        // Nothing is planned yet, so nothing is known to be missing either —
        // `missing` is the *plan's* answer and the plan needs this schema.
        Err(DbError::Cancelled) => {
            return FilesOutcome::Cancelled {
                files: 0,
                missing: Vec::new(),
                replaced: Vec::new(),
            };
        }
        Err(e) => {
            return FilesOutcome::Failed {
                message: format!("Export failed: {e}"),
                files: 0,
                missing: Vec::new(),
                replaced: Vec::new(),
            };
        }
    };
    let plan = schemaic_core::dump::file_plan(
        &schema,
        &req.database,
        &req.tables,
        req.format,
        req.dialect,
    );
    if plan.files.is_empty() {
        return FilesOutcome::Failed {
            message: "Nothing to export — no table matched the selection.".to_string(),
            files: 0,
            replaced: Vec::new(),
            // The interesting case for this arm: every ticked table went missing
            // between the picker and the launch, so "no table matched" is true
            // and useless on its own — the names are what says why.
            missing: plan.missing,
        };
    }

    // **What this export is about to destroy, read before it destroys any of
    // it.** `select_directories()` has no overwrite prompt — the single-file
    // export's only guard against replacing the user's work is the save dialog's
    // own "replace?", and the folder form has no equivalent. Nothing between the
    // picker and the `rename` checked, and no arm of `FilesOutcome` could say so
    // afterwards.
    //
    // Read here, once, ahead of the loop: after the first `rename` the answer is
    // contaminated by this export's own output, and a `Cancelled` or `Failed`
    // arm would report whichever prefix it happened to reach.
    // **And it is the guard as well as the report now.** The list was computed
    // here and used only for a post-mortem: `FilesOutcome` had no arm that could
    // ask, so the files were replaced and then named. The census and the verdict
    // are `core::dump`'s, which is where their tests are; the one line of this
    // that touches a filesystem is the closure.
    let replaced = schemaic_core::dump::colliding_files(&plan, |f| req.folder.join(f).is_file());
    if let schemaic_core::dump::FolderVerdict::Ask(replaced) =
        schemaic_core::dump::folder_verdict(req.approved.as_deref(), &replaced)
    {
        // Before the first `rename`, so the folder is untouched.
        return FilesOutcome::WouldReplace { replaced };
    }

    let total = plan.files.len();
    let mut done = 0usize;
    let mut rows_so_far = 0u64;
    let mut tally = ExportTally::default();

    // Reported by every arm below, not just the happy one: a folder that is two
    // files short of what was ticked looks exactly like a complete one, and a
    // stopped export is *more* likely to be inspected than a finished one, not
    // less. `FilePlan::missing` used to reach only `Done`.
    let missing = plan.missing.clone();
    // **What this run has actually replaced**, as against `replaced`, which is
    // the census of what it was *going* to. Only the finished arm below is
    // reached with the loop complete, and the census used to go verbatim to all
    // three — so a Stop during the first table reported three files destroyed in
    // a folder the same sentence had just said nothing was written to. See
    // `dump::destroyed`.
    let mut published: Vec<String> = Vec::new();
    // **And what it has *opened*.** `write_one` truncates the table's `.part`
    // with `File::create` before it writes a byte, and both the cancel and the
    // failure arms then sweep it — so a table whose retry was stopped destroyed
    // the fragment an earlier run had left, while `published` (which only the
    // finished tables reach) said nothing about it. That fragment is the one
    // file in the folder the user might still have wanted back.
    let mut attempted: Vec<String> = Vec::new();

    for (i, step) in plan.files.iter().enumerate() {
        // **Asked before the table is begun**, so a Stop that landed between two
        // tables does not open the next one's `.part` before the read it is
        // waiting on returns `Cancelled`. Without this the folder gained an empty
        // fragment for a table the export never started reading.
        if token.is_cancelled() {
            return FilesOutcome::Cancelled {
                files: done,
                missing,
                replaced: schemaic_core::dump::destroyed(&replaced, &published, &attempted),
            };
        }
        // Best-effort, exactly as the dump's: a full progress channel must never
        // hold up a write.
        let _ = progress.send(DumpProgress {
            index: i + 1,
            total,
            table: step.table.clone(),
            rows: rows_so_far,
        });
        let path = req.folder.join(&step.file);
        let part = part_of(&path);
        // Recorded here, before the writer is spawned: from this point the
        // `.part` is this run's, truncated whatever happens next.
        attempted.push(step.file.clone());
        let (row_tx, row_rx) = tokio::sync::mpsc::channel::<ExportChunk>(2);
        let w_token = token.clone();
        let (format, dialect) = (req.format, req.dialect);
        // The **source**, so a format that names its origin gets the right one:
        // `ExportFormat::Xlsx` titles the worksheet from it (`export::sheet_name`),
        // and a workbook of twelve tables all called "Result" is not a folder
        // anyone can read back.
        let source = (
            req.database.clone(),
            step.schema.clone(),
            step.table.clone(),
        );
        let (w_path, w_part) = (path.clone(), part.clone());
        let writer = handle.spawn_blocking(move || {
            write_one(&w_part, &w_path, row_rx, format, dialect, source, w_token)
        });
        let read = db
            .stream_query(
                Some(&req.database),
                &step.select,
                chunk_rows,
                token.clone(),
                row_tx,
            )
            .await;
        let written = writer.await;

        // **Cancel is the reader's to declare; every other failure is the
        // writer's to describe.** The streamed export's rule, and it is here for
        // the reasons stated there: a cancelled read closes the channel, which
        // the writer would otherwise see as an ordinary end of stream, while a
        // full disk fails the *writer* and only then fails the reader's next
        // send — so asking the reader first reports the symptom.
        // **The fragment of the table that did not finish is swept, always.**
        //
        // This is where a folder export parts company with the single-file one.
        // There, the `.part` is the only trace of the rows that arrived and
        // `export_cancel_note` points the user straight at it, so removing it
        // would destroy the one thing they might still want. Here the finished
        // files *are* that trace: they are whole, they are published, and
        // `files_cancel_note` and `files_failure_note` promise exactly them —
        // "the table in progress was left unwritten". A fragment left beside
        // them makes both sentences false and drops an unreadable
        // `orders.csv.part` into a directory the user chose for output and is
        // about to go looking through.
        let sweep = || {
            let _ = std::fs::remove_file(&part);
        };
        // **`dump_verdict`, not a second copy of it.** These five arms were
        // written out again here, and the copy diverged in the one arm the
        // extraction exists to protect: `WriteEnd::Failed` carries the writer's
        // own words, which already begin "Export failed:", and re-prefixing them
        // produced "Export failed: Export failed: No space left on device (os
        // error 28) — 3 files already written to out are kept."
        //
        // The writer's refusal to publish a truncated file is folded into the
        // *read* end before asking, because that is what it is: a cancel, whose
        // only witness is the token — a stop that landed between the last chunk
        // and the rename never reaches the reader at all.
        let stopped_at_publish = token.is_cancelled() && matches!(written, Ok(Err(_)));
        let read_end = match &read {
            Err(DbError::Cancelled) => ReadEnd::Cancelled,
            _ if stopped_at_publish => ReadEnd::Cancelled,
            Err(e) => ReadEnd::Failed(e.to_string()),
            Ok(_) => ReadEnd::Clean,
        };
        let (write_end, tally_of) = match written {
            Ok(Ok(t)) => (WriteEnd::Wrote, Some(t)),
            Ok(Err(e)) => (
                WriteEnd::Failed {
                    message: e.message,
                    opened: e.opened,
                },
                None,
            ),
            Err(e) => (WriteEnd::Died(e.to_string()), None),
        };
        match schemaic_core::dump::dump_verdict(read_end, write_end) {
            // A folder export names the files it published rather than a single
            // `.part`, so `partial` has nothing to say here.
            DumpVerdict::Cancelled { .. } => {
                sweep();
                return FilesOutcome::Cancelled {
                    files: done,
                    missing,
                    replaced: schemaic_core::dump::destroyed(&replaced, &published, &attempted),
                };
            }
            DumpVerdict::Failed { message, .. } => {
                sweep();
                return FilesOutcome::Failed {
                    message,
                    files: done,
                    missing,
                    replaced: schemaic_core::dump::destroyed(&replaced, &published, &attempted),
                };
            }
            DumpVerdict::Done => {
                rows_so_far += read.unwrap_or(0);
                // One fold across every file, so one sentence can name what none
                // of them could carry — `ExportTally::absorb`'s reason.
                if let Some(t) = tally_of {
                    tally.absorb(t);
                }
                // The rename has landed, so if this name was in the census it is
                // now genuinely gone. Recorded here and nowhere else: a failed
                // write is swept as a `.part` and never reaches the real name.
                published.push(step.file.clone());
                done += 1;
            }
        }
    }

    FilesOutcome::Done {
        files: done,
        tally,
        missing,
        // Every step published, so this equals the census — computed the same
        // way regardless, so the three arms cannot drift apart again.
        replaced: schemaic_core::dump::destroyed(&replaced, &published, &attempted),
    }
}

/// The blocking writer for **one table's file**: stream it, then the atomic
/// publish. [`write()`]'s guarantees for a single table.
#[allow(clippy::too_many_arguments)]
fn write_one(
    part: &Path,
    path: &Path,
    mut rows: tokio::sync::mpsc::Receiver<ExportChunk>,
    format: ExportFormat,
    dialect: schemaic_core::intel::SqlDialect,
    source: (String, Option<String>, String),
    token: CancellationToken,
) -> Result<ExportTally, WriteFail> {
    use std::io::Write as _;

    // The create is the one failure that leaves nothing behind — see `WriteFail`.
    let mut w = match std::fs::File::create(part) {
        Ok(f) => std::io::BufWriter::new(f),
        Err(e) => {
            return Err(WriteFail {
                message: format!("Export failed: {e}"),
                opened: false,
            });
        }
    };
    let src_token = token.clone();
    let mut src = PullChunks::new(move || match rows.blocking_recv() {
        // **A cancelled read is an error, not an end of stream** — the streamed
        // export's rule. Without it a buffered format assembles and compresses
        // the whole discarded workbook before anyone notices the Stop.
        None if src_token.is_cancelled() => Err(std::io::Error::other("export cancelled")),
        None => Ok(None),
        Some(Ok(rs)) => Ok(Some(rs)),
        // The reader's own reason, carried across so a half-written table is
        // never mistaken for a finished one.
        Some(Err(e)) => Err(std::io::Error::other(e)),
    });
    let tally = format
        .stream_to(
            &mut w,
            &mut src,
            Some((source.0.as_str(), source.1.as_deref(), source.2.as_str())),
            dialect,
        )
        .map_err(|e| format!("Export failed: {e}"))?;
    w.flush().map_err(|e| format!("Export failed: {e}"))?;
    drop(w);
    // **Only now** does the file appear under its real name — and not at all if
    // this was cancelled. See `write`: a cancel reaches here as an ordinary end
    // of stream, so publishing first and declaring the cancel afterwards would
    // leave a truncated table looking exactly like a finished one.
    if token.is_cancelled() {
        return Err("Export cancelled.".to_string().into());
    }
    std::fs::rename(part, path).map_err(|e| {
        // **It must not point at the `.part`.** This message used to say "the
        // export wrote <part> but could not rename it", and the caller's `sweep()`
        // deletes that file three lines later — so the one sentence telling the
        // user where their rows were named a path that no longer existed by the
        // time they read it.
        format!(
            "Export failed: {} could not be published: {e}",
            path.display()
        )
    })?;
    Ok(tally)
}

/// The blocking writer: one file, every step, then the atomic publish.
fn write(
    part: &Path,
    path: &Path,
    mut rx: tokio::sync::mpsc::Receiver<Msg>,
    dialect: schemaic_core::intel::SqlDialect,
    token: CancellationToken,
) -> Result<ExportTally, WriteFail> {
    use std::io::Write as _;

    // The create is the one failure that leaves nothing behind — see `WriteFail`.
    let mut w = match std::fs::File::create(part) {
        Ok(f) => std::io::BufWriter::new(f),
        Err(e) => {
            return Err(WriteFail {
                message: format!("Export failed: {e}"),
                opened: false,
            });
        }
    };
    // **The tally, folded across every table, not a row count.** What the file
    // could not carry — a binary column written as `NULL`, a value past the arena
    // ceiling left blank — is the difference between a backup and something that
    // looks like one, and each table reports its own. A column is named once
    // however many tables it appears in, the same rule `ExportTally::note`
    // follows within one.
    let mut total = ExportTally::default();
    while let Some(msg) = rx.blocking_recv() {
        match msg {
            Msg::Text(sql) => {
                writeln!(w, "{sql}\n").map_err(|e| format!("Export failed: {e}"))?;
            }
            Msg::Table {
                source,
                rows: mut rows_rx,
            } => {
                let mut src = PullChunks::new(move || match rows_rx.blocking_recv() {
                    None => Ok(None),
                    Some(Ok(rs)) => Ok(Some(rs)),
                    // The reader's own reason, carried across so a half-written
                    // table is never mistaken for a finished one.
                    Some(Err(e)) => Err(std::io::Error::other(e)),
                });
                let tally = ExportFormat::Sql
                    .stream_to(
                        &mut w,
                        &mut src,
                        Some((source.0.as_str(), source.1.as_deref(), source.2.as_str())),
                        dialect,
                    )
                    .map_err(|e| format!("Export failed: {e}"))?;
                // The fold is `ExportTally::absorb`'s, beside `note`, which
                // answers the same question one level down.
                total.absorb(tally);
                writeln!(w).map_err(|e| format!("Export failed: {e}"))?;
            }
        }
    }
    w.flush().map_err(|e| format!("Export failed: {e}"))?;
    drop(w);
    // **Only now** does the destination change — and not at all if this was
    // cancelled. A cancel arrives as an ordinary end of stream, so the check has
    // to be here: publishing and letting the caller declare the cancel afterwards
    // would rename a truncated file over the user's, which is the whole reason
    // the sibling exists.
    if token.is_cancelled() {
        return Err("Export cancelled.".to_string().into());
    }
    std::fs::rename(part, path).map_err(|e| {
        format!(
            "The export wrote {} but could not rename it to {}: {e}",
            part.display(),
            path.display()
        )
    })?;
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fragment's name has to be the one the modal tells the user about, and
    /// that sentence is built by `export::part_path` — so this must not spell
    /// `.part` a second time. It is the one situation where the fragment is the
    /// thing the user still wants.
    #[test]
    fn the_part_file_is_a_sibling_named_by_the_one_function_that_names_them() {
        let p = part_of(Path::new("/tmp/shop.sql"));
        assert_eq!(p.parent(), Path::new("/tmp/shop.sql").parent());
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        assert_eq!(name, schemaic_core::export::part_path("shop.sql"));
        assert_ne!(name, "shop.sql", "the fragment must not be the destination");
    }

    /// A path with no file name cannot be written to at all; `File::create` is
    /// where that is reported, and this must not panic on the way there.
    #[test]
    fn a_path_with_no_file_name_is_returned_unchanged() {
        let p = Path::new("/");
        assert_eq!(part_of(p), p.to_path_buf());
    }

    /// **The gate.** Nothing in this crate spells `.part` — the suffix belongs
    /// to `export::part_path`, and every sentence the user reads about the
    /// fragment (`export_cancel_note`, `export_failure_note`) is built from
    /// that same function.
    ///
    /// `export_file` in `main.rs` had its own inline closure appending the
    /// literal, and it is the site that matters most: it serves both export
    /// scopes and all five grid formats, and its cancel and failure arms are
    /// the ones that name the fragment. A change to `part_path` — a dot prefix
    /// to hide it, a timestamp so two exports cannot collide — would have made
    /// both of those sentences point at a path that does not exist, on the one
    /// path where the fragment is the only copy of the user's rows.
    ///
    /// Doc comments are dropped before the scan, so the prose above (and the
    /// several paragraphs in `main.rs` that discuss the sibling) is not a hit.
    ///
    /// **The needle is the suffix at the end of a literal, not the literal
    /// `".part"`.** It used to be `code.contains("\".part\"")` — the suffix with
    /// a *leading* quote — which matches the one violating spelling it was
    /// written for (`p.push(".part")`) and misses the two commoner ones:
    /// `format!("{name}.part")` and `with_extension("sql.part")` carry no quote
    /// before the dot. `.part"` catches all three, and the rule the gate states
    /// is "nothing spells it", not "nothing spells it that way".
    ///
    /// **And the walk is recursive**, because a rule phrased as "nothing in
    /// this crate" was enforced over one non-recursive `read_dir` of `src`: a
    /// submodule directory added later would have been outside it silently.
    ///
    /// The comment skip is still `starts_with("//")` on the trimmed line, so a
    /// violating expression inside a `/* … */` block is not seen. That is a
    /// known and deliberate limit — this crate has no block comments, and the
    /// alternative is a second copy of `source_gate::production_code` in a
    /// crate that cannot reach it.
    #[test]
    fn nothing_in_this_crate_spells_the_fragment_suffix_itself() {
        fn walk(dir: &std::path::Path, rel: &str, offenders: &mut Vec<String>, files: &mut usize) {
            for entry in std::fs::read_dir(dir).expect("the crate's src") {
                let path = entry.expect("a dir entry").path();
                let name = path.file_name().unwrap().to_string_lossy().to_string();
                if path.is_dir() {
                    walk(&path, &format!("{rel}{name}/"), offenders, files);
                    continue;
                }
                if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                    continue;
                }
                *files += 1;
                let src = std::fs::read_to_string(&path).expect("a source file");
                for (i, line) in src.lines().enumerate() {
                    let code = line.trim_start();
                    if code.starts_with("//") {
                        continue;
                    }
                    if code.contains(".part\"") {
                        offenders.push(format!("{rel}{name}:{}: {}", i + 1, code.trim()));
                    }
                }
            }
        }

        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut offenders = Vec::new();
        let mut files = 0usize;
        walk(&dir, "", &mut offenders, &mut files);
        assert!(
            offenders.is_empty(),
            "call `dump::part_of` (which asks `export::part_path`) instead:\n{}",
            offenders.join("\n")
        );
        // The scan has to still be reading the crate: a moved `src` would pass
        // this gate by finding nothing at all.
        assert!(files >= 12, "only {files} source files scanned");
    }

    /// **Which list is the consent and which is the census.**
    ///
    /// `folder_verdict`'s whole point is that the consent is the list the user
    /// was shown and said yes to, while the collisions are a *fresh* reading of
    /// the folder — so a file that appeared while the modal stood is asked about
    /// again instead of being destroyed silently. Both parameters are lists of
    /// file names, so `folder_verdict(Some(&replaced), &replaced)` compiles,
    /// always answers `Write`, and restores the exact defect the `Option<&[…]>`
    /// signature replaced — with the whole suite green. `core::dump`'s eight
    /// cases drive the pure function and none of them can see which list *this*
    /// caller hands it.
    ///
    /// The other half is that the census is read before the first `rename`: once
    /// this export has published a file, the folder's contents are partly its
    /// own output and the answer is contaminated.
    #[test]
    fn the_folder_export_consents_to_the_list_the_user_was_shown() {
        let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/dump.rs"))
            .expect("this file's own source");
        let body = schemaic_ui::source_gate::production_code(&src);
        assert!(
            body.contains("folder_verdict(req.approved.as_deref(), &replaced)"),
            "the folder export's consent is no longer the list the user saw. \
             Handing it the fresh census instead answers `Write` unconditionally, \
             and every test of the pure function stays green."
        );
        // And the census the verdict judges is read from the folder, not
        // reconstructed from the plan.
        assert!(
            body.contains("colliding_files(&plan, |f| req.folder.join(f).is_file())"),
            "the collision census is no longer a reading of the folder"
        );
    }
}
