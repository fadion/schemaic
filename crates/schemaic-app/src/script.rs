//! Running a `.sql` script: the I/O half of [`schemaic_core::script`].
//!
//! The mirror image of [`crate::dump`]. That module reads a database and writes
//! a file; this one reads a file and writes a database, and between them the
//! round trip closes — until now Schemaic exported `.sql` files only another
//! tool could replay.
//!
//! The shape is two halves running at once, as a dump's is. A **blocking reader**
//! walks the file a block at a time, feeds [`Splitter`] and pushes the statements
//! it completes into a bounded channel; the **executor** (`Db::run_script`) pulls
//! from the other end and applies them in order on one pinned connection. Each
//! reports how it ended, and [`run_outcome`] decides which ending the user is
//! told about — a decision with more in it than it looks, and one that is *not*
//! the same as the dump's (see its doc).
//!
//! **The bounded channel is the whole progress design.** The reader cannot get
//! more than `SCRIPT_QUEUE` statements ahead of the server, so
//! `Splitter::consumed` tracks what has actually been applied closely enough to
//! report from — which is why there is no progress channel out of the executor,
//! and why a 2 GB file read at disk speed cannot pile up in memory ahead of a
//! server applying it one statement at a time.

use std::io::Read;

use schemaic_core::script::{ReadEnd, RunOutcome, Splitter, run_outcome};
use schemaic_db::{Db, SCRIPT_QUEUE};
use schemaic_ui::{ScriptProgress, ScriptRequest};
use tokio_util::sync::CancellationToken;

/// How much of the file to read per syscall.
///
/// Independent of `SCRIPT_QUEUE`: this is how much *file* is read at once, that
/// is how many *statements* may be in flight. A dump's extended `INSERT`s put
/// tens of statements in a block of this size, which the queue then meters out.
const BLOCK: usize = 256 * 1024;

/// The largest single read, however much is being held.
///
/// The read grows with what is pending (see the loop below); this stops that
/// from turning into one 256 MB allocation at the ceiling.
const MAX_BLOCK: usize = 32 * 1024 * 1024;

/// Run a script to completion, reporting progress as the file is consumed.
///
/// Returns the outcome rather than reporting it, so the caller owns the single
/// hop back onto the UI thread — the same division `dump::run` uses.
pub(crate) async fn run(
    db: Db,
    req: ScriptRequest,
    token: CancellationToken,
    progress: crossbeam_channel::Sender<ScriptProgress>,
) -> RunOutcome {
    let (tx, rx) = tokio::sync::mpsc::channel(SCRIPT_QUEUE);
    let reader = {
        let (path, dialect, token) = (req.path.clone(), req.dialect, token.clone());
        tokio::task::spawn_blocking(move || read(path, dialect, tx, token, progress))
    };

    // Both halves run at once. The executor ends when the reader drops `tx` —
    // whether that is the end of the file, a cancel or a disk error — and the
    // reader ends when the executor drops `rx`, which is what a refused
    // statement does.
    let (exec_end, ran) = db.run_script(&req.database, rx, token).await;
    let read_end = reader.await.unwrap_or_else(|e| {
        // A panicked or aborted reader is a failure of ours, not of the file's,
        // and saying so beats reporting a clean end of stream.
        ReadEnd::Failed(format!("the file reader stopped unexpectedly: {e}"))
    });
    run_outcome(read_end, exec_end, ran)
}

/// The reading half: file → statements, on a blocking thread.
///
/// `tx` is moved in and dropped when this returns, which is how the executor
/// learns the file is finished — so every exit from this function, including the
/// failures, closes the channel.
fn read(
    path: std::path::PathBuf,
    dialect: schemaic_core::intel::SqlDialect,
    tx: tokio::sync::mpsc::Sender<schemaic_core::script::Statement>,
    token: CancellationToken,
    progress: crossbeam_channel::Sender<ScriptProgress>,
) -> ReadEnd {
    let file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(e) => return ReadEnd::Failed(e.to_string()),
    };
    // Best-effort: a file whose length cannot be read still loads, it just has
    // no denominator to report against.
    let total = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut file = std::io::BufReader::with_capacity(BLOCK, file);
    let mut splitter = Splitter::new(dialect);
    let mut block = vec![0u8; BLOCK];

    loop {
        if token.is_cancelled() {
            return ReadEnd::Cancelled;
        }
        // **The read grows with what is still unfinished**, and that is what
        // keeps the splitter's cost linear. `Splitter::split` re-scans its
        // buffer from the start on every block, so a statement spanning *m*
        // blocks costs O(m²·block) — nothing for a dump, whose statements are
        // far smaller than one block, but a file with *no* terminator in it (a
        // CSV renamed `.sql`, a dump truncated inside a string) grows the buffer
        // to `MAX_PENDING_BYTES` and would re-scan a quarter of a gigabyte a
        // thousand times before being refused: minutes of a pinned thread before
        // the message saying the file is not a script. Doubling the read makes
        // the number of re-scans logarithmic instead.
        let want = BLOCK.max(splitter.pending() / 2).min(MAX_BLOCK);
        if block.len() < want {
            block.resize(want, 0);
        }
        let n = match file.read(&mut block[..want]) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => return ReadEnd::Failed(e.to_string()),
        };
        for stmt in splitter.push(&block[..n]) {
            // The executor has gone. It holds the reason; this half must not
            // invent one (`ReadEnd::Stopped` carries no message on purpose).
            if tx.blocking_send(stmt).is_err() {
                return ReadEnd::Stopped;
            }
        }
        // Checked after the push, so a statement that merely *reaches* the
        // ceiling and then ends is not refused for it.
        if splitter.pending() > schemaic_core::script::MAX_PENDING_BYTES {
            return ReadEnd::NoTerminator;
        }
        let _ = progress.send(ScriptProgress {
            bytes_done: splitter.consumed(),
            bytes_total: total,
        });
    }

    // The last statement need not carry a terminator.
    if let Some(stmt) = splitter.finish()
        && tx.blocking_send(stmt).is_err()
    {
        return ReadEnd::Stopped;
    }
    let _ = progress.send(ScriptProgress {
        bytes_done: splitter.consumed(),
        bytes_total: total,
    });
    ReadEnd::Eof
}

#[cfg(test)]
mod tests {
    use super::*;
    use schemaic_core::script::ExecEnd;

    /// The reader's contract, driven over a real file: every statement handed
    /// over in order, and `Eof` at the end.
    ///
    /// A temp file rather than the workspace's usual no-filesystem rule, and
    /// deliberately narrow about it: the thing under test *is* reading a file,
    /// the same exemption `core/tests/doc_coverage.rs` takes for reading the
    /// module list. Everything the reader decides that does not need a file —
    /// the splitting, the endings, the outcome — is tested in `core::script`
    /// without one.
    #[test]
    fn the_reader_hands_over_every_statement_then_reports_eof() {
        let dir = std::env::temp_dir().join("schemaic-script-reader-test");
        std::fs::create_dir_all(&dir).expect("a temp dir");
        let path = dir.join("ok.sql");
        std::fs::write(
            &path,
            "CREATE TABLE t (a int);\nINSERT INTO t VALUES (1);\n",
        )
        .expect("write the fixture");

        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .build()
            .expect("a runtime");
        let (tx, mut rx) = tokio::sync::mpsc::channel(SCRIPT_QUEUE);
        let (ptx, _prx) = crossbeam_channel::unbounded();
        let end = rt.block_on(async move {
            tokio::task::spawn_blocking(move || {
                read(
                    path,
                    schemaic_core::intel::SqlDialect::MySql,
                    tx,
                    CancellationToken::new(),
                    ptx,
                )
            })
            .await
            .expect("the reader finishes")
        });
        assert_eq!(end, ReadEnd::Eof);

        let mut got = Vec::new();
        while let Ok(s) = rx.try_recv() {
            got.push(s.sql);
        }
        assert_eq!(
            got,
            vec![
                "CREATE TABLE t (a int);".to_string(),
                "INSERT INTO t VALUES (1);".to_string()
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file that is not there fails the *reader*, which `run_outcome` then
    /// ranks above anything the executor saw — the executor only ever sees an
    /// empty stream, which is indistinguishable from an empty file.
    #[test]
    fn a_missing_file_fails_the_reader_rather_than_reading_nothing() {
        let path = std::env::temp_dir().join("schemaic-no-such-script-file.sql");
        let _ = std::fs::remove_file(&path);
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .build()
            .expect("a runtime");
        let (tx, _rx) = tokio::sync::mpsc::channel(SCRIPT_QUEUE);
        let (ptx, _prx) = crossbeam_channel::unbounded();
        let end = rt.block_on(async move {
            tokio::task::spawn_blocking(move || {
                read(
                    path,
                    schemaic_core::intel::SqlDialect::MySql,
                    tx,
                    CancellationToken::new(),
                    ptx,
                )
            })
            .await
            .expect("the reader finishes")
        });
        assert!(
            matches!(end, ReadEnd::Failed(_)),
            "a missing file reported {end:?}"
        );
        // And the consequence, which is the half that matters: folded against an
        // executor that saw nothing and called it a clean finish, the run is
        // still a failure.
        assert!(matches!(
            run_outcome(end, ExecEnd::Done, 0),
            RunOutcome::Failed { .. }
        ));
    }
}
