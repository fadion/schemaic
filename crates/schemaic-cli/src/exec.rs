//! The headless **write** path, and the guard that is the only way into it.
//!
//! **`schemaic exec` is a separate subcommand, not a flag on `query`.** A flag
//! would make the dangerous case a one-character edit away from the safe one
//! and would put the decision in the caller's hands at the moment it is easiest
//! to get wrong. More importantly it would give the two paths one gate, and
//! they do not want the same gate: `query` runs an allowlist with no override,
//! while a write has to consult the connection's read-only flag, the
//! missing-`WHERE` warning and the user's own say-so.
//!
//! **The guard mints the request.** [`ExecRequest::approved`] is the only
//! constructor, its fields are private, and [`run`] takes one by value — so
//! there is no spelling of "run this statement" that has not been through
//! `sql::run_verdict` first. That is the shape the app's own invariant is
//! written to, and it is written that way because the alternative has already
//! failed here once: when the guard was a step the launcher had to remember,
//! deleting one `return` left a read-only connection running a whole file with
//! the workspace green.

use std::time::Duration;

use schemaic_core::connection::Connection;
use schemaic_core::model::ResultSet;
use schemaic_core::sql::{GuardPolicy, RunVerdict};
use schemaic_db::{Db, Enforce};
use tokio_util::sync::CancellationToken;

use crate::query::{NoRows, normalize_stmt};

/// Why a write was not run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotRun {
    /// Nothing but whitespace and semicolons.
    Empty,
    /// More than one statement. Refused rather than half-run: a one-shot
    /// command has nowhere to report "statements 1 and 2 committed, 3 failed".
    Several,
    /// The guard said no outright — a read-only connection, or no database
    /// selected. There is no override for this and `--yes` does not reach it.
    Blocked(String),
    /// The guard wants the user to look first, and `--yes` was not given.
    NeedsConsent(String),
}

impl NotRun {
    /// What goes to stderr.
    pub fn message(&self) -> String {
        match self {
            NotRun::Empty => "empty statement".to_string(),
            NotRun::Several => {
                "`exec` runs one statement; this is several. Run them one at a time, \
                 so a failure half-way is not something you have to reconstruct"
                    .to_string()
            }
            NotRun::Blocked(why) => format!("refused: {why}"),
            NotRun::NeedsConsent(why) => {
                format!("{why} Pass --yes to run it anyway.")
            }
        }
    }
}

/// A statement approved for execution.
///
/// **Only [`ExecRequest::approved`] can build one**, and that function *is* the
/// guard. The field is private and there is no other constructor, so a future
/// caller cannot reach [`run`] without passing through `sql::run_verdict`.
#[derive(Clone, Debug, PartialEq)]
pub struct ExecRequest {
    sql: String,
    /// What the session must enforce, minted from the connection the verdict
    /// judged: a read-only connection's approved statement is a read *to the
    /// text check*, and `SELECT setval(…)` is one of those. The session is what
    /// refuses it.
    enforce: Enforce,
    /// **The target the verdict judged, minted with it.** `run` took a `Db` and
    /// a database of its own, so nothing tied the connection a statement ran on
    /// to the one whose `read_only` flag approved it — one caller passing the
    /// same locals twice was the whole guarantee. The caller now connects
    /// through [`ExecRequest::connection`] and `run` reads the database from
    /// here, so a second caller cannot pair an approval with another target.
    conn: Connection,
    database: Option<String>,
}

impl ExecRequest {
    /// The guard and the request in one step.
    ///
    /// `assume_yes` answers a [`RunVerdict::Confirm`] — on this path
    /// `sql::unsafe_reason`'s: a missing `WHERE`, a `TRUNCATE`, or a statement
    /// that destroys stored rows with their object — and **cannot** answer a
    /// [`RunVerdict::Block`],
    /// which is what a read-only connection produces. That asymmetry is the
    /// point: `--yes` is for a question, not for a refusal.
    pub fn approved(
        conn: &Connection,
        database: Option<&str>,
        sql: &str,
        assume_yes: bool,
    ) -> Result<ExecRequest, NotRun> {
        let Some(stmt) = normalize_stmt(sql) else {
            return Err(NotRun::Empty);
        };
        // One statement, checked before the guard: `run_verdict` judges a slice
        // and would happily approve a batch this command has no way to report
        // on half-way through.
        let dialect = schemaic_core::intel::SqlDialect::from_db_type(&conn.db_type);
        if schemaic_core::sql::executable_statements(stmt, dialect).len() > 1 {
            return Err(NotRun::Several);
        }
        // **`confirm_writes: false`, and that is not the app's preference being
        // ignored — it is where the consent lives.** Typing `exec` is the
        // caller saying "this one writes"; asking again for every bounded
        // `UPDATE … WHERE id = 1` would mean `--yes` on essentially every
        // invocation, and a flag that is always passed has stopped carrying
        // information — including for the unbounded write it would then also
        // wave through. So `--yes` is reserved for what the guard actually
        // flags, which leaves it rare enough to read.
        let policy = GuardPolicy::of(Some(conn), database.is_none(), false);
        match schemaic_core::sql::run_verdict(&[stmt.to_string()], policy) {
            RunVerdict::Allow => Ok(()),
            RunVerdict::Block(why) => Err(NotRun::Blocked(why)),
            RunVerdict::Confirm(why) => {
                if assume_yes {
                    Ok(())
                } else {
                    Err(NotRun::NeedsConsent(why))
                }
            }
        }
        .map(|()| ExecRequest {
            sql: stmt.to_string(),
            enforce: if conn.read_only {
                Enforce::ReadOnly
            } else {
                Enforce::AsJudged
            },
            conn: conn.clone(),
            database: database.map(str::to_string),
        })
    }

    pub fn sql(&self) -> &str {
        &self.sql
    }

    /// The saved connection the verdict judged — what the caller connects to.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// The database the verdict judged, which is where [`run`] runs it.
    pub fn database(&self) -> Option<&str> {
        self.database.as_deref()
    }

    /// What the session this request runs on must enforce.
    pub fn enforce(&self) -> Enforce {
        self.enforce
    }
}

/// How many of the rows a write **returns** `exec` keeps — `UPDATE …
/// RETURNING`, a `CALL` that selects. The cap is on what is printed, never on
/// the statement: each engine's row loop stops reading at the cap and leaves the
/// statement to finish.
///
/// It was `1`, passed at the call site, so a thousand-row `RETURNING` printed
/// one row and no warning. It is the query's default now, and it lives here
/// rather than as a parameter so the call site cannot choose it again.
const RETURNED_ROW_CAP: usize = crate::args::DEFAULT_LIMIT;

/// Run an approved write. Returns the result the engine gave back — for a DML
/// statement that is a row count in [`ResultSet::affected`]; for one that
/// returns rows, up to [`RETURNED_ROW_CAP`] of them, `truncated` if there were
/// more.
///
/// `db` must be opened on [`ExecRequest::connection`]; the database is the
/// request's own.
pub async fn run(db: &Db, request: ExecRequest, timeout: Duration) -> Result<ResultSet, NoRows> {
    let token = CancellationToken::new();
    match crate::deadline::with_deadline(
        // Never plain `fetch_query`: `approved` counted one statement with the
        // gate's lexer, and on MySQL only a pinned `sql_mode` makes the server
        // count the same.
        db.fetch_query_enforced(
            request.database(),
            request.sql(),
            RETURNED_ROW_CAP,
            token.clone(),
            request.enforce(),
        ),
        token,
        timeout,
    )
    .await
    {
        Some(r) => r.map_err(|e| NoRows::Failed(e.to_string())),
        // Not `TimedOut`: that says "cancelled", which a write cannot promise.
        None => Err(NoRows::Indeterminate(timeout)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(read_only: bool) -> Connection {
        let mut c: Connection = serde_json::from_str(
            r#"{"id":1,"name":"c","host":"h","port":3306,"user":"u","password":"","database":"app"}"#,
        )
        .expect("a minimal saved connection parses");
        c.read_only = read_only;
        c.cli_access = true;
        c
    }

    fn approved(sql: &str, yes: bool) -> Result<ExecRequest, NotRun> {
        ExecRequest::approved(&conn(false), Some("app"), sql, yes)
    }

    #[test]
    fn a_targeted_write_is_approved() {
        let r = approved("DELETE FROM t WHERE id = 1", false).expect("a WHERE-bounded delete");
        assert_eq!(r.sql(), "DELETE FROM t WHERE id = 1");
    }

    /// **`DROP TABLE` waits for `--yes`, as `TRUNCATE` does.** A CLI test run
    /// found it running unasked and exiting 0 with "(0 rows affected)".
    #[test]
    fn dropping_a_table_needs_yes() {
        for sql in [
            "DROP TABLE t",
            "DROP TABLES t",
            "DROP DATABASE app",
            "CREATE OR REPLACE TABLE t (id int)",
            "EXPLAIN ANALYZE DELETE FROM t",
            "ALTER TABLE t TRUNCATE PARTITION ALL",
        ] {
            assert!(
                matches!(approved(sql, false), Err(NotRun::NeedsConsent(_))),
                "{sql}"
            );
            assert!(approved(sql, true).is_ok(), "{sql} with --yes");
        }
    }

    #[test]
    fn a_trailing_semicolon_does_not_reach_the_server() {
        assert_eq!(
            approved("DELETE FROM t WHERE id = 1;", false)
                .unwrap()
                .sql(),
            "DELETE FROM t WHERE id = 1"
        );
    }

    #[test]
    fn an_empty_statement_is_refused() {
        assert_eq!(approved("  ; ", false), Err(NotRun::Empty));
    }

    /// **A read-only connection has no override.** `--yes` answers a question;
    /// this is not a question, and a flag that could unlock it would make the
    /// guard-rail decorative.
    #[test]
    fn a_read_only_connection_blocks_a_write_and_yes_does_not_help() {
        for yes in [false, true] {
            let got =
                ExecRequest::approved(&conn(true), Some("app"), "DELETE FROM t WHERE id = 1", yes);
            assert!(
                matches!(got, Err(NotRun::Blocked(_))),
                "read-only must block with assume_yes = {yes}"
            );
        }
    }

    /// An unbounded write is the one the guard wants looked at, so it must not
    /// run unasked — and must run when the caller says it meant it.
    #[test]
    fn an_unbounded_delete_needs_consent_and_yes_gives_it() {
        let got = approved("DELETE FROM t", false);
        assert!(
            matches!(got, Err(NotRun::NeedsConsent(_))),
            "a WHERE-less DELETE must ask first"
        );
        assert!(approved("DELETE FROM t", true).is_ok(), "--yes answers it");
    }

    /// The consent message has to say how to answer it, or the caller is stuck
    /// with a refusal and no next step.
    #[test]
    fn the_consent_refusal_names_the_flag_that_answers_it() {
        let NotRun::NeedsConsent(_) = approved("DELETE FROM t", false).unwrap_err() else {
            panic!("expected a consent refusal");
        };
        let m = approved("DELETE FROM t", false).unwrap_err().message();
        assert!(m.contains("--yes"));
    }

    /// A batch has nowhere to report a half-way failure in a one-shot command.
    #[test]
    fn several_statements_are_refused_rather_than_half_run() {
        assert_eq!(
            approved(
                "DELETE FROM t WHERE id = 1; DELETE FROM u WHERE id = 2",
                true
            ),
            Err(NotRun::Several)
        );
    }

    /// **A read-only connection's approval runs on a read-only session.** The
    /// verdict is a text check, and `SELECT setval(…)` is a read to it; only the
    /// session sees that it writes.
    #[test]
    fn a_read_only_connection_mints_a_read_only_session() {
        let r = ExecRequest::approved(&conn(true), Some("app"), "SELECT setval('s', 1)", false)
            .expect("a SELECT passes the text check on a read-only connection");
        assert_eq!(r.enforce(), Enforce::ReadOnly);
    }

    /// A writable connection still gets the lexer pin, never the bare session:
    /// the one-statement count above was the gate's, and only a session that
    /// lexes like it makes the server's count agree.
    #[test]
    fn a_writable_connection_mints_a_session_that_lexes_like_the_gate() {
        let r = approved("DELETE FROM t WHERE id = 1", false).unwrap();
        assert_eq!(r.enforce(), Enforce::AsJudged);
    }

    /// **The request names the target its verdict judged** — the connection
    /// whose `read_only` flag it read and the database whose absence it
    /// weighed — so `run` and the connect cannot be handed another pair.
    #[test]
    fn a_request_carries_the_connection_and_database_it_was_judged_against() {
        let ro = conn(true);
        let r = ExecRequest::approved(&ro, None, "SELECT 1", false).unwrap();
        assert_eq!(r.connection(), &ro);
        assert!(r.connection().read_only);
        assert_eq!(r.database(), None);
        let w = approved("DELETE FROM t WHERE id = 1", false).unwrap();
        assert_eq!(w.database(), Some("app"));
        assert!(!w.connection().read_only);
    }

    /// A read through `exec` is not an error — it is pointless but harmless,
    /// and refusing it would mean `exec` needs its own second gate to decide
    /// what a read is. `query` is the one with the allowlist.
    #[test]
    fn a_read_is_allowed_through_exec_without_consent() {
        assert!(approved("SELECT 1", false).is_ok());
    }
}
