//! **A connection's init SQL** — the session settings a saved connection runs
//! on every connection Schemaic opens for it (`Connection::init_sql`): a
//! `search_path`, a time zone, a role.
//!
//! **It is user SQL that runs with nobody confirming it**, on every operation
//! (one connection per operation: a run, a listing, a ping, a `KILL`), so it
//! has a gate of its own, and the gate is an **allowlist**, not `run_verdict`'s
//! confirm: [`check`] accepts a statement only when it is a session `SET`
//! (on SQLite, a `PRAGMA` named in [`SQLITE_PRAGMAS`]) and refuses the rest
//! outright. That makes it one of the refusals strictly stronger than
//! `run_verdict` that AGENTS.md's write-guard invariant lists. What it shuts
//! out, and why:
//!
//! - **Anything but `SET`** — a `USE` would move every operation off the
//!   database it asked for (the tab's, the tree node's), and an `INSERT` would
//!   be a write on every ping.
//! - **A parenthesis** — `SET @x = f()` on MySQL or SQL Server calls a function,
//!   and a function can write. No setting worth having needs one.
//! - **Server-wide or durable settings** — MySQL's `GLOBAL`, `PERSIST`,
//!   `PERSIST_ONLY`, `SET PASSWORD` and `SET DEFAULT ROLE` change the server or
//!   an account, not the session.
//! - **What the read-only guard and the write path rest on** — the transaction
//!   characteristics (`TRANSACTION`, `CHARACTERISTICS`,
//!   `default_transaction_read_only`, `transaction_read_only`, `tx_read_only`,
//!   SQLite's `query_only`), `autocommit` (a write would never commit),
//!   `IMPLICIT_TRANSACTIONS`, `NOCOUNT` and `ROWCOUNT` (the row counts the
//!   one-row safety net reads), and the lexing the literals Schemaic writes
//!   depend on: PostgreSQL's `standard_conforming_strings` and MySQL's
//!   `sql_mode`. SQL Server's `SET TRANSACTION ISOLATION LEVEL` is allowed:
//!   its read-only guard is a rollback, which an isolation level cannot lift.
//!
//! The enforcement statements of a read-only connection run *after* the init
//! SQL, inside each operation, so even a setting this list missed is
//! overridden there; the list is what keeps it from mattering on the paths
//! that are not enforced.

use crate::intel::SqlDialect;
use crate::sql::{is_word_byte, skip_noncode, statement_ranges};

/// The PRAGMAs a SQLite connection's init SQL may set: per-connection
/// behaviour only. A pragma that writes the file (`user_version`,
/// `journal_mode`, `auto_vacuum`) or lifts the read-only guard (`query_only`,
/// `writable_schema`) is not here.
pub const SQLITE_PRAGMAS: &[&str] = &[
    "FOREIGN_KEYS",
    "DEFER_FOREIGN_KEYS",
    "RECURSIVE_TRIGGERS",
    "CASE_SENSITIVE_LIKE",
    "CACHE_SIZE",
    "TEMP_STORE",
    "BUSY_TIMEOUT",
    "AUTOMATIC_INDEX",
    "CACHE_SPILL",
    "MMAP_SIZE",
];

/// Words no init statement may hold at code level — see the module doc.
const DENIED: &[&str] = &[
    "GLOBAL",
    "PERSIST",
    "PERSIST_ONLY",
    "PASSWORD",
    "AUTOCOMMIT",
    "SQL_MODE",
    "TRANSACTION_READ_ONLY",
    "TX_READ_ONLY",
    "DEFAULT_TRANSACTION_READ_ONLY",
    "STANDARD_CONFORMING_STRINGS",
    "CHARACTERISTICS",
    "NOCOUNT",
    "ROWCOUNT",
    "IMPLICIT_TRANSACTIONS",
    "QUERY_ONLY",
];

/// `sql`'s statements, each trimmed and without its terminator, if every one
/// is a setting an init may make on `dialect` — else why the first one that is
/// not, in words the connection form shows. Empty SQL is no statements.
pub fn check(sql: &str, dialect: SqlDialect) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for (lo, hi) in statement_ranges(sql, dialect) {
        let stmt = sql[lo..hi].trim().trim_end_matches(';').trim();
        let (words, paren) = code_words(stmt, dialect);
        let Some(head) = words.first() else {
            continue; // a comment alone
        };
        let shown = crate::history::preview(stmt);
        if settings_are_pragmas(dialect) {
            if head != "PRAGMA" {
                return Err(format!(
                    "“{shown}”: only PRAGMA settings can run when connecting"
                ));
            }
            match words.get(1) {
                Some(p) if SQLITE_PRAGMAS.contains(&p.as_str()) => {}
                _ => {
                    return Err(format!(
                        "“{shown}”: that PRAGMA cannot run when connecting — it changes the \
                         file or the read-only guard"
                    ));
                }
            }
        } else if head != "SET" {
            return Err(format!(
                "“{shown}”: only SET statements can run when connecting"
            ));
        }
        if paren {
            return Err(format!(
                "“{shown}”: a setting cannot call a function when connecting"
            ));
        }
        if let Some(w) = words.iter().find(|w| DENIED.contains(&w.as_str())) {
            return Err(format!(
                "“{shown}”: {} cannot be set when connecting — Schemaic relies on it",
                w.to_ascii_lowercase()
            ));
        }
        if words.get(1).is_some_and(|w| w == "DEFAULT") {
            return Err(format!(
                "“{shown}”: SET DEFAULT changes an account, not the session"
            ));
        }
        // Where the read-only guard is a rollback, a transaction setting
        // (an isolation level) cannot lift it; elsewhere it is the guard.
        let guard_is_a_rollback = dialect.read_only_is_a_rollback();
        if words.iter().any(|w| w == "TRANSACTION") && !guard_is_a_rollback {
            return Err(format!(
                "“{shown}”: transaction settings cannot be made when connecting — the \
                 read-only guard sets them"
            ));
        }
        out.push(stmt.to_string());
    }
    Ok(out)
}

/// Is a session setting on `dialect` a `PRAGMA` rather than a `SET`? SQLite's
/// alone — exhaustive, so a fifth engine answers here rather than defaulting.
pub fn settings_are_pragmas(dialect: SqlDialect) -> bool {
    match dialect {
        SqlDialect::Sqlite => true,
        SqlDialect::MySql | SqlDialect::Postgres | SqlDialect::MsSql => false,
    }
}

/// `stmt`'s code-level words, upper-cased, and whether a `(` occurs outside a
/// literal — strings, quoted names and comments skipped by
/// [`skip_noncode`], the one boundary lexer.
fn code_words(stmt: &str, dialect: SqlDialect) -> (Vec<String>, bool) {
    let b = stmt.as_bytes();
    let (mut words, mut paren) = (Vec::new(), false);
    let mut i = 0;
    while i < b.len() {
        if let Some(j) = skip_noncode(b, i, dialect) {
            i = j;
        } else if is_word_byte(b[i]) {
            let s = i;
            while i < b.len() && is_word_byte(b[i]) {
                i += 1;
            }
            words.push(stmt[s..i].to_ascii_uppercase());
        } else {
            paren |= b[i] == b'(';
            i += 1;
        }
    }
    (words, paren)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MY: SqlDialect = SqlDialect::MySql;
    const PG: SqlDialect = SqlDialect::Postgres;
    const MS: SqlDialect = SqlDialect::MsSql;
    const LITE: SqlDialect = SqlDialect::Sqlite;

    #[test]
    fn session_settings_pass_and_come_back_as_statements() {
        assert_eq!(
            check("SET search_path TO app, public;\nSET ROLE reporting;", PG).unwrap(),
            ["SET search_path TO app, public", "SET ROLE reporting"]
        );
        assert_eq!(
            check("SET time_zone = '+00:00'; SET NAMES utf8mb4", MY).unwrap(),
            ["SET time_zone = '+00:00'", "SET NAMES utf8mb4"]
        );
        assert_eq!(
            check(
                "SET LANGUAGE us_english; SET TRANSACTION ISOLATION LEVEL SNAPSHOT",
                MS
            )
            .unwrap()
            .len(),
            2
        );
        assert_eq!(check("PRAGMA foreign_keys = ON", LITE).unwrap().len(), 1);
    }

    #[test]
    fn nothing_is_nothing_to_run() {
        assert!(check("", PG).unwrap().is_empty());
        assert!(check("  -- just a note\n", MY).unwrap().is_empty());
    }

    #[test]
    fn anything_but_a_set_is_refused() {
        for (sql, d) in [
            ("USE other", MY),
            ("USE other", MS),
            ("INSERT INTO log VALUES (1)", PG),
            ("SELECT 1", PG),
            ("DELETE FROM t", LITE),
            ("SET search_path TO x; DROP TABLE t", PG),
        ] {
            assert!(check(sql, d).is_err(), "{sql}");
        }
    }

    #[test]
    fn a_setting_that_calls_a_function_is_refused() {
        assert!(check("SET @x = (SELECT f())", MY).is_err());
        assert!(check("SET @x = dbo.f()", MS).is_err());
        // A parenthesis inside a value is a value.
        assert!(check("SET application_name = 'a (b)'", PG).is_ok());
    }

    #[test]
    fn server_wide_and_account_settings_are_refused() {
        for sql in [
            "SET GLOBAL max_connections = 10",
            "SET @@global.time_zone = '+00:00'",
            "SET PERSIST time_zone = '+00:00'",
            "SET PASSWORD = 'x'",
            "SET DEFAULT ROLE ALL TO me",
        ] {
            assert!(check(sql, MY).is_err(), "{sql}");
        }
    }

    #[test]
    fn what_the_guards_rest_on_cannot_be_moved() {
        for (sql, d) in [
            ("SET standard_conforming_strings = off", PG),
            ("SET default_transaction_read_only = off", PG),
            ("SET SESSION CHARACTERISTICS AS TRANSACTION READ WRITE", PG),
            ("SET TRANSACTION READ WRITE", MY),
            ("SET SESSION transaction_read_only = 0", MY),
            ("SET sql_mode = 'ANSI_QUOTES'", MY),
            ("SET autocommit = 0", MY),
            ("SET NOCOUNT ON", MS),
            ("SET ROWCOUNT 1", MS),
            ("SET IMPLICIT_TRANSACTIONS ON", MS),
            ("PRAGMA query_only = OFF", LITE),
            ("PRAGMA user_version = 3", LITE),
            ("PRAGMA journal_mode = DELETE", LITE),
        ] {
            assert!(check(sql, d).is_err(), "{sql}");
        }
    }

    #[test]
    fn a_denied_word_inside_a_value_is_only_a_value() {
        assert!(check("SET application_name = 'sql_mode autocommit'", PG).is_ok());
    }

    #[test]
    fn the_refusal_names_the_statement() {
        let e = check("SET search_path TO x;\nSET autocommit = 0", MY).unwrap_err();
        assert!(
            e.contains("SET autocommit = 0") && e.contains("autocommit"),
            "{e}"
        );
    }
}
