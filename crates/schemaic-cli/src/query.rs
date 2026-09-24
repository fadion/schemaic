//! The one headless read path.
//!
//! **Both non-GUI front ends run their reads through here** — `schemaic query`
//! and the MCP server's `run_query` tool. They had every reason to drift apart:
//! same gate, same timeout, same normalisation, two copies, and only one of
//! them getting a fix. What differs between them is what they do with the rows,
//! which is the caller's business and stays there.
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
    /// The statement is not a read. Carries `read_only_reason`'s words.
    NotARead(String),
    /// The server refused it, or the connection failed.
    Failed(String),
    /// The deadline passed and the statement was cancelled server-side.
    TimedOut(Duration),
}

impl NoRows {
    /// What goes to stderr.
    pub fn message(&self) -> String {
        match self {
            NoRows::Empty => "empty query".to_string(),
            NoRows::NotARead(why) => {
                format!("refused: {why}. `query` only runs reads; use `exec` to write")
            }
            // No prefix of our own: `DbError`'s Display already says what
            // failed ("query failed: …", "could not connect: …"), and a second
            // one printed "query failed: query failed: …".
            NoRows::Failed(e) => e.clone(),
            NoRows::TimedOut(d) => {
                format!("query exceeded {}s and was cancelled", d.as_secs().max(1))
            }
        }
    }

    /// Whether this refusal is the guard's doing rather than the server's.
    ///
    /// The two get different exit codes: a script that is asking the wrong
    /// thing and a database that is down are different problems, and a caller
    /// that cannot tell them apart retries the one that will never succeed.
    pub fn is_refusal(&self) -> bool {
        matches!(self, NoRows::Empty | NoRows::NotARead(_))
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
    let Some(stmt) = normalize_stmt(sql) else {
        return Err(NoRows::Empty);
    };
    // Gated in the connection's own dialect, so a PostgreSQL `#` operator is not
    // mistaken for a comment on the way in.
    if let Err(why) = schemaic_core::sql::read_only_reason(stmt, db.engine().dialect()) {
        return Err(NoRows::NotARead(why));
    }
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

#[cfg(test)]
mod tests {
    use super::*;

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
