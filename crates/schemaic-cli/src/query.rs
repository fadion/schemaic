//! The one headless read path.
//!
//! **Both non-GUI front ends run their reads through here** — `schemaic query`
//! and the MCP server's `run_query` tool. They had every reason to drift apart:
//! same gate, same timeout, same normalisation, two copies, and only one of
//! them getting a fix. What differs between them is what they do with the rows,
//! which is the caller's business and stays there. A plan — the MCP server's
//! `explain` — is a read too, and [`read_only_explain`] takes the same gate.
//!
//! The gate is [`schemaic_core::sql::read_only_reason`], and it is **strictly
//! stronger than the editor's** `run_verdict`: an allowlist of read-only
//! statement *heads* per dialect plus a deny-list of keywords anywhere, with no
//! confirm arm to say yes to. That is the right shape when there is nobody at
//! the keyboard to ask — and it is **not** what makes the statement read-only.
//! A head is a spelling; a function called from a `SELECT` can write. The
//! statement runs in a read-only session ([`Enforce::ReadOnly`]), which refuses
//! a write by its effect, and the gate stays in front for what such a session
//! allows (sleeps, locks, server-side file reads). Writes are [`crate::exec`],
//! behind their own guard.

use std::time::Duration;

use schemaic_core::model::ResultSet;
use schemaic_db::{Db, Enforce};
use tokio_util::sync::CancellationToken;

/// How long a headless statement may run before it is cancelled.
///
/// A backstop against a `SLEEP()` or a heavy scan holding a connection open
/// with no window to close. It is the CLI's default and the MCP server's fixed
/// value; `schemaic query --timeout` moves it.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Why a headless read produced no rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoRows {
    /// Nothing but whitespace and semicolons.
    Empty,
    /// The statement is not a read — it writes, so `exec` is where it goes.
    /// Carries `read_only_reason`'s words.
    NotARead(String),
    /// Refused as no read-only query, but **not as a write**: a lock, a sleep,
    /// a second statement (`sql::ReadRefusal::writes` is false). `exec` is no
    /// answer to any of them, so the message does not point there.
    NotPermitted(String),
    /// The server refused it, or the connection failed.
    Failed(String),
    /// The deadline passed and the statement was cancelled server-side.
    TimedOut(Duration),
    /// A **write**'s deadline passed. It was sent, and a cancel does not undo
    /// what a non-transactional engine already changed — nine of forty MyISAM
    /// rows were updated by one that reported "cancelled" — nor a commit whose
    /// reply was in flight. What happened is unknown, and saying "cancelled"
    /// told a script that retrying was safe.
    Indeterminate(Duration),
}

impl NoRows {
    /// What goes to stderr.
    pub fn message(&self) -> String {
        match self {
            NoRows::Empty => "empty query".to_string(),
            NoRows::NotARead(why) => {
                format!("refused: {why}. `query` only runs reads; use `exec` to write")
            }
            NoRows::NotPermitted(why) => format!("refused: {why}"),
            // No prefix of our own: `DbError`'s Display already says what
            // failed ("query failed: …", "could not connect: …"), and a second
            // one printed "query failed: query failed: …".
            NoRows::Failed(e) => e.clone(),
            NoRows::TimedOut(d) => {
                format!("query exceeded {}s and was cancelled", d.as_secs().max(1))
            }
            NoRows::Indeterminate(d) => format!(
                "the statement exceeded {}s and was stopped, but it had been sent: it may have \
                 been applied in whole or in part. Check before running it again",
                d.as_secs().max(1)
            ),
        }
    }

    /// Whether this refusal is the guard's doing rather than the server's.
    ///
    /// The two get different exit codes: a script that is asking the wrong
    /// thing and a database that is down are different problems, and a caller
    /// that cannot tell them apart retries the one that will never succeed.
    pub fn is_refusal(&self) -> bool {
        matches!(
            self,
            NoRows::Empty | NoRows::NotARead(_) | NoRows::NotPermitted(_)
        )
    }
}

/// Trim surrounding whitespace and a single trailing `;`, returning `None` if
/// nothing is left.
///
/// Pure, so the empty and `;`-only cases are tested rather than discovered. A
/// person types the semicolon out of habit and a `SELECT 1;` that came back
/// "empty query" would be a baffling way to learn it was not wanted.
pub fn normalize_stmt(sql: &str) -> Option<&str> {
    let stmt = sql.trim().trim_end_matches(';').trim();
    (!stmt.is_empty()).then_some(stmt)
}

/// The text half of the read guard: the statement to run, or why not.
///
/// Public so a front end can ask it **before** connecting — a refusal that has
/// already read the keyring and logged in over SSH has sent something to a
/// server, which exit 3 promises it has not. [`read_only_query`] asks again, so
/// asking early is never the only check.
pub fn gate(sql: &str, dialect: schemaic_core::intel::SqlDialect) -> Result<&str, NoRows> {
    let stmt = normalize_stmt(sql).ok_or(NoRows::Empty)?;
    schemaic_core::sql::read_only_refusal(stmt, dialect).map_err(|r| {
        if r.writes {
            NoRows::NotARead(r.reason)
        } else {
            NoRows::NotPermitted(r.reason)
        }
    })?;
    Ok(stmt)
}

/// Run a **read**, or say why not.
///
/// `row_cap` bounds what is held in memory and reaches the caller;
/// [`ResultSet::truncated`] says whether it bit, and every front end is
/// responsible for passing that on — a caller that silently reports a capped
/// result as the whole answer has been given a wrong answer by a command that
/// succeeded.
pub async fn read_only_query(
    db: &Db,
    database: Option<&str>,
    sql: &str,
    row_cap: usize,
    timeout: Duration,
) -> Result<ResultSet, NoRows> {
    // Gated in the connection's own dialect, so a PostgreSQL `#` operator is not
    // mistaken for a comment on the way in.
    let stmt = gate(sql, db.engine().dialect())?;
    let token = CancellationToken::new();
    // **Read-only at the session too, because the gate reads only the text.**
    // `SELECT setval(…)`, `SELECT lo_unlink(…)` and a `SELECT` of a function
    // whose body deletes all pass it and all write; a read-only transaction
    // refuses them by what they do rather than how they are spelled.
    match crate::deadline::with_deadline(
        db.fetch_query_enforced(database, stmt, row_cap, token.clone(), Enforce::ReadOnly),
        token,
        timeout,
    )
    .await
    {
        Some(r) => r.map_err(|e| NoRows::Failed(e.to_string())),
        None => Err(NoRows::TimedOut(timeout)),
    }
}

/// Rows per chunk a streamed export reads — the app's whole-result export's
/// figure, a block worth the per-chunk cost and modest to hold.
pub const EXPORT_CHUNK_ROWS: usize = 10_000;

/// What a finished export wrote.
#[derive(Debug)]
pub struct Exported {
    pub tally: schemaic_core::export::ExportTally,
}

/// Run a **read** and write its whole result to `path` in `format` — the
/// uncapped sibling of [`read_only_query`], behind the same gate and on the
/// same read-only session, and the one other headless read path (so the
/// read-only census reads this file for both).
///
/// The rows stream: a reader task pulls chunks off the server while a blocking
/// writer renders each into the file (`ExportFormat::stream_to`), so memory is
/// a chunk or two, not the result. The file is built under `export::part_file`
/// and renamed into place only when everything was written; a failure leaves
/// the `.part`, which the message names. `timeout` bounds the whole read, as
/// every read in this crate is bounded (`deadline`'s gate); the command's
/// default is an hour, since a table export can legitimately take a while.
pub async fn read_only_export(
    db: &Db,
    database: Option<&str>,
    sql: &str,
    format: schemaic_core::export::ExportFormat,
    path: &std::path::Path,
    timeout: Duration,
) -> Result<Exported, NoRows> {
    let stmt = gate(sql, db.engine().dialect())?;
    let token = CancellationToken::new();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<schemaic_db::ExportChunk>(2);
    let part = schemaic_core::export::part_file(path);
    let dialect = db.engine().dialect();
    let writer = {
        let (part, token) = (part.clone(), token.clone());
        tokio::task::spawn_blocking(
            move || -> std::io::Result<schemaic_core::export::ExportTally> {
                use std::io::Write as _;
                let mut w = std::io::BufWriter::new(std::fs::File::create(&part)?);
                let mut src =
                    schemaic_core::export::PullChunks::new(move || match rx.blocking_recv() {
                        None if token.is_cancelled() => {
                            Err(std::io::Error::other("export cancelled"))
                        }
                        None => Ok(None),
                        Some(Ok(rs)) => Ok(Some(rs)),
                        Some(Err(e)) => Err(std::io::Error::other(e)),
                    });
                let tally = format.stream_to(&mut w, &mut src, None, dialect)?;
                w.flush()?;
                Ok(tally)
            },
        )
    };
    // **Read-only at the session too**, for `read_only_query`'s reason: the
    // gate reads the text, the session refuses what the statement does.
    let read = crate::deadline::with_deadline(
        db.stream_query_enforced(
            database,
            stmt,
            EXPORT_CHUNK_ROWS,
            token.clone(),
            tx,
            Some(Enforce::ReadOnly),
        ),
        token.clone(),
        timeout,
    )
    .await
    .ok_or(NoRows::TimedOut(timeout));
    let written = writer
        .await
        .map_err(|e| NoRows::Failed(format!("the export's writer stopped: {e}")))?
        .map_err(|e| e.to_string());
    let read = match read {
        Err(_) => Err(timeout),
        Ok(r) => Ok(r.map(|_| ()).map_err(|e| e.to_string())),
    };
    let shown = part.display().to_string();
    let tally = export_verdict(read, written, format.writes_incrementally(), &shown)
        .map_err(NoRows::Failed)?;
    std::fs::rename(&part, path).map_err(|e| {
        NoRows::Failed(format!(
            "the export is complete in {shown} but could not be renamed to {}: {e}",
            path.display()
        ))
    })?;
    Ok(Exported { tally })
}

/// How an export ended, from its two halves: the read (`Err` for the deadline
/// passing) and the writer's own result — the tally, or the message the user
/// is told.
///
/// **The writer's error wins**, unless the deadline stopped the read: a writer
/// that fails (a full disk, a folder that went away) drops its end of the
/// channel, and the reader's next send then fails with its own "the export
/// stopped reading" — true, and useless, with the real cause thrown away. So a
/// failed write is told as itself. A failed read is told as the server's.
/// Either way the `.part` is named only where it holds the rows written so far
/// (`incremental`) — an XLSX workbook is assembled whole and its `.part` is
/// empty until the end.
pub fn export_verdict(
    read: Result<Result<(), String>, Duration>,
    written: Result<schemaic_core::export::ExportTally, String>,
    incremental: bool,
    part: &str,
) -> Result<schemaic_core::export::ExportTally, String> {
    let left = if incremental {
        format!("; what was written is in {part}")
    } else {
        String::new()
    };
    match (read, written) {
        (Err(t), _) => Err(format!(
            "the export exceeded {}s and was stopped{left}",
            t.as_secs().max(1)
        )),
        (_, Err(w)) => Err(format!("could not write {part}: {w}")),
        (Ok(Err(r)), Ok(_)) => Err(format!("{r}{left}")),
        (Ok(Ok(())), Ok(tally)) => Ok(tally),
    }
}

/// The plan of a **read**, or why not — the MCP server's `explain` tool.
///
/// **Behind the same gate as [`read_only_query`]**, although a plan executes
/// nothing: the planner is not inert. PostgreSQL folds an `IMMUTABLE`
/// function's call at plan time — and nothing stops such a function writing —
/// and a statement this path would plan is one `run_query` would refuse to run,
/// so a model has no reason to ask for it. Only ever the plain form: `analyze`
/// is `false`, so the statement is never run to be measured, and `read_only`
/// is `true` for whatever the engine does with it regardless.
pub async fn read_only_explain(
    db: &Db,
    database: Option<&str>,
    sql: &str,
    timeout: Duration,
) -> Result<schemaic_core::plan::QueryPlan, NoRows> {
    let stmt = gate(sql, db.engine().dialect())?;
    let token = CancellationToken::new();
    match crate::deadline::with_deadline(
        db.explain(database, stmt, false, true, token.clone()),
        token,
        timeout,
    )
    .await
    {
        Some(r) => r
            .map(|rs| schemaic_core::plan::QueryPlan::from_result(&rs))
            .map_err(|e| NoRows::Failed(e.to_string())),
        None => Err(NoRows::TimedOut(timeout)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod export_verdict_tests {
        use super::*;
        use schemaic_core::export::ExportTally;

        fn tally(rows: u64) -> ExportTally {
            ExportTally {
                rows,
                ..ExportTally::default()
            }
        }

        #[test]
        fn a_whole_read_and_write_is_the_tally() {
            assert_eq!(
                export_verdict(Ok(Ok(())), Ok(tally(5)), true, "a.part"),
                Ok(tally(5))
            );
        }

        /// The writer's own error, not the reader's echo of it.
        #[test]
        fn a_failed_write_is_told_as_itself() {
            let got = export_verdict(
                Ok(Err("the export stopped reading".into())),
                Err("No space left on device".into()),
                true,
                "a.part",
            );
            assert_eq!(
                got,
                Err("could not write a.part: No space left on device".into())
            );
        }

        #[test]
        fn a_failed_read_names_the_part_only_where_it_holds_rows() {
            let read = || Ok(Err("query failed: relation does not exist".to_string()));
            assert_eq!(
                export_verdict(read(), Ok(tally(0)), true, "a.part"),
                Err("query failed: relation does not exist; what was written is in a.part".into())
            );
            assert_eq!(
                export_verdict(read(), Ok(tally(0)), false, "a.xlsx.part"),
                Err("query failed: relation does not exist".into()),
                "an XLSX .part is empty until the end"
            );
        }

        #[test]
        fn a_timeout_says_so_and_names_what_it_left() {
            assert_eq!(
                export_verdict(
                    Err(Duration::from_secs(60)),
                    Err("cancelled".into()),
                    true,
                    "a.part"
                ),
                Err(
                    "the export exceeded 60s and was stopped; what was written is in a.part".into()
                )
            );
        }
    }

    #[test]
    fn a_trailing_semicolon_is_not_part_of_the_statement() {
        assert_eq!(normalize_stmt("SELECT 1;"), Some("SELECT 1"));
        assert_eq!(normalize_stmt("  SELECT 1 ;  "), Some("SELECT 1"));
    }

    #[test]
    fn several_trailing_semicolons_go_too() {
        assert_eq!(normalize_stmt("SELECT 1;;;"), Some("SELECT 1"));
    }

    #[test]
    fn nothing_but_punctuation_is_an_empty_statement() {
        assert_eq!(normalize_stmt(""), None);
        assert_eq!(normalize_stmt("   "), None);
        assert_eq!(normalize_stmt(";"), None);
        assert_eq!(normalize_stmt(" ;; "), None);
    }

    /// A semicolon *inside* the statement is the statement's business.
    #[test]
    fn an_interior_semicolon_is_left_alone() {
        assert_eq!(normalize_stmt("SELECT ';' AS x"), Some("SELECT ';' AS x"));
    }

    /// **The guard's refusals and the server's failures are different exits.**
    /// A caller that cannot tell "you asked for the wrong thing" from "the
    /// database is down" retries the one that will never succeed.
    #[test]
    fn a_guard_refusal_is_distinguishable_from_a_server_failure() {
        assert!(NoRows::Empty.is_refusal());
        assert!(NoRows::NotARead("it is a DELETE".into()).is_refusal());
        assert!(!NoRows::Failed("connection reset".into()).is_refusal());
        assert!(!NoRows::TimedOut(DEFAULT_TIMEOUT).is_refusal());
    }

    /// The refusal has to point at the subcommand that would have worked.
    #[test]
    fn refusing_a_write_names_exec() {
        let m = NoRows::NotARead("DELETE is not a read".into()).message();
        assert!(m.contains("DELETE is not a read"));
        assert!(m.contains("exec"), "the message must say where writes go");
    }

    /// **And only a write's does.** Through `gate`, the composition: a
    /// locking read was told "use `exec` to write" after core's own words
    /// said the lock was no write — and `exec` would not run two `SELECT`s or
    /// a `SLEEP` any better. Each is still a refusal, exit 3.
    #[test]
    fn refusing_what_is_no_write_does_not_name_exec() {
        use schemaic_core::intel::SqlDialect;
        for sql in [
            "SELECT * FROM t FOR UPDATE",
            "SELECT * FROM t FOR SHARE",
            "SELECT 1; SELECT 2",
            "SELECT SLEEP(5)",
        ] {
            let refusal = gate(sql, SqlDialect::MySql).unwrap_err();
            let m = refusal.message();
            assert!(!m.contains("exec") && !m.contains("write"), "{sql}: {m}");
            assert!(m.starts_with("refused: "), "{sql}: {m}");
            assert!(refusal.is_refusal(), "{sql}");
        }
        let m = gate("DELETE FROM t", SqlDialect::MySql)
            .unwrap_err()
            .message();
        assert!(m.contains("exec"), "{m}");
    }

    /// The driver's message is already contextualised; repeating our own in
    /// front of it produced "query failed: query failed: …" on a real server.
    #[test]
    fn a_server_failure_is_reported_in_the_drivers_own_words_only() {
        let m = NoRows::Failed("query failed: Server error: no such table".into()).message();
        assert_eq!(m, "query failed: Server error: no such table");
        assert_eq!(m.matches("query failed").count(), 1);
    }

    #[test]
    fn a_timeout_reports_the_deadline_it_passed() {
        assert!(
            NoRows::TimedOut(Duration::from_secs(30))
                .message()
                .contains("30s")
        );
    }
}
