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
fn part_of(path: &Path) -> PathBuf {
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
        Err(DbError::Cancelled) => return DumpOutcome::Cancelled,
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
        return DumpOutcome::Cancelled;
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
        Ok(Err(e)) => WriteEnd::Failed(e),
        Err(e) => WriteEnd::Died(e.to_string()),
    };
    match dump_verdict(read, write) {
        DumpVerdict::Cancelled => DumpOutcome::Cancelled,
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
            };
        }
        Err(e) => {
            return FilesOutcome::Failed {
                message: format!("Export failed: {e}"),
                files: 0,
                missing: Vec::new(),
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
            // The interesting case for this arm: every ticked table went missing
            // between the picker and the launch, so "no table matched" is true
            // and useless on its own — the names are what says why.
            missing: plan.missing,
        };
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

    for (i, step) in plan.files.iter().enumerate() {
        // **Asked before the table is begun**, so a Stop that landed between two
        // tables does not open the next one's `.part` before the read it is
        // waiting on returns `Cancelled`. Without this the folder gained an empty
        // fragment for a table the export never started reading.
        if token.is_cancelled() {
            return FilesOutcome::Cancelled {
                files: done,
                missing,
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
        let failed = |message: String| {
            sweep();
            FilesOutcome::Failed {
                message,
                files: done,
                missing: missing.clone(),
            }
        };
        let cancelled = || {
            sweep();
            FilesOutcome::Cancelled {
                files: done,
                missing: missing.clone(),
            }
        };
        match (read, written) {
            (Err(DbError::Cancelled), _) => return cancelled(),
            // The writer's own refusal to publish a truncated file. It is a
            // cancel, not a failure, and the token is the only witness — a stop
            // that landed between the last chunk and the rename never reaches the
            // reader at all.
            (_, Ok(Err(_))) if token.is_cancelled() => return cancelled(),
            (_, Ok(Err(e))) => return failed(format!("Export failed: {e}")),
            (_, Err(e)) => return failed(format!("Export failed: worker died: {e}")),
            (Err(e), _) => return failed(format!("Export failed: {e}")),
            (Ok(n), Ok(Ok(t))) => {
                rows_so_far += n;
                // One fold across every file, so one sentence can name what none
                // of them could carry — `ExportTally::absorb`'s reason.
                tally.absorb(t);
                done += 1;
            }
        }
    }

    FilesOutcome::Done {
        files: done,
        tally,
        missing,
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
) -> Result<ExportTally, String> {
    use std::io::Write as _;

    let mut w = std::fs::File::create(part)
        .map(std::io::BufWriter::new)
        .map_err(|e| format!("Export failed: {e}"))?;
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
        return Err("Export cancelled.".to_string());
    }
    std::fs::rename(part, path).map_err(|e| {
        format!(
            "The export wrote {} but could not rename it to {}: {e}",
            part.display(),
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
) -> Result<ExportTally, String> {
    use std::io::Write as _;

    let mut w = std::fs::File::create(part)
        .map(std::io::BufWriter::new)
        .map_err(|e| format!("Export failed: {e}"))?;
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
        return Err("Export cancelled.".to_string());
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
}
