//! **A connection's init SQL** — the session settings a saved connection runs
//! on every connection Schemaic opens for it (`Connection::init_sql`): a
//! `search_path`, a time zone, a role.
//!
//! **It is user SQL that runs with nobody confirming it**, on every operation
//! (one connection per operation: a run, a listing, a ping, a `KILL`), so it
//! has a gate of its own, and the gate is an **allowlist**, not `run_verdict`'s
//! confirm: [`check`] accepts a statement only when it is a session `SET` of a
//! setting **named on this engine's list** (on SQLite, a `PRAGMA` named in
//! [`SQLITE_PRAGMAS`]) and refuses the rest outright. That makes it one of the
//! refusals strictly stronger than `run_verdict` that AGENTS.md's write-guard
//! invariant lists.
//!
//! **The lists name what may be set, not what may not.** It was a deny list
//! of the words a guard rests on, and every setting nobody had thought of went
//! through: `ANSI_DEFAULTS ON` (which turns on `IMPLICIT_TRANSACTIONS`, so
//! every auto-commit write rolled back under an `ok` report), `FMTONLY`,
//! `NOEXEC`, `sql_select_limit` and `TEXTSIZE` (a dump or copy silently
//! short), a non-UTF-8 client character set (the server and the read-only
//! text gate then disagree on where a string ends), and any denied name
//! written quoted, since a quoted name was skipped as a literal. A name on no
//! list is refused now, so a setting is let in only once someone has weighed
//! it. What stays shut out, and why:
//!
//! - **Anything but `SET`** — a `USE` would move every operation off the
//!   database it asked for (the tab's, the tree node's), and an `INSERT` would
//!   be a write on every ping.
//! - **A parenthesis** — `SET @x = f()` on MySQL or SQL Server calls a function,
//!   and a function can write. No setting worth having needs one.
//! - **A quoted setting name** — it names the same setting to the server, and
//!   no setting needs the quotes.
//! - **Server-wide or durable settings** — MySQL's `GLOBAL`, `PERSIST` and
//!   `PERSIST_ONLY` scopes, `SET PASSWORD` and `SET DEFAULT ROLE` change the
//!   server or an account, not the session.
//! - **What the read-only guard and the write path rest on** — the
//!   transaction characteristics and read-only flags, `autocommit`,
//!   `IMPLICIT_TRANSACTIONS`/`ANSI_DEFAULTS`, `NOCOUNT`/`ROWCOUNT` (the row
//!   counts the one-row safety net reads), what makes a statement a no-op
//!   (`FMTONLY`, `NOEXEC`, `PARSEONLY`) or a read short (`sql_select_limit`,
//!   `TEXTSIZE`), and the lexing the literals Schemaic writes and the text
//!   gate reads depend on: PostgreSQL's `standard_conforming_strings`, MySQL's
//!   `sql_mode`, a client character set other than UTF-8 (`SET NAMES` and
//!   `client_encoding` pass with a UTF-8 value only), and SQL Server's
//!   `QUOTED_IDENTIFIER`, `ANSI_WARNINGS` and the other ISO switches, which
//!   pass only `ON`. SQL Server's `SET TRANSACTION ISOLATION LEVEL` is
//!   allowed: its read-only guard is a rollback, which an isolation level
//!   cannot lift.
//!
//! The enforcement statements of a read-only connection run *after* the init
//! SQL, inside each operation, so a transaction setting is overridden there
//! too; the lists are what keep everything else from mattering.

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

/// The MySQL/MariaDB session variables an init may set: time zone, locale,
/// timeouts, buffer sizes, optimiser knobs, an isolation level (which cannot
/// lift `SET SESSION TRANSACTION READ ONLY`). Not here, on purpose:
/// `autocommit`, `sql_mode`, the read-only flags, `sql_select_limit` (a dump
/// or copy silently short), the `character_set_*` variables (`SET NAMES` with
/// a UTF-8 value is the way in), `foreign_key_checks`/`unique_checks`, and
/// anything else nobody has weighed yet.
const MYSQL_SETTINGS: &[&str] = &[
    "TIME_ZONE",
    "LC_TIME_NAMES",
    "LC_MESSAGES",
    "WAIT_TIMEOUT",
    "INTERACTIVE_TIMEOUT",
    "NET_READ_TIMEOUT",
    "NET_WRITE_TIMEOUT",
    "LOCK_WAIT_TIMEOUT",
    "INNODB_LOCK_WAIT_TIMEOUT",
    "MAX_EXECUTION_TIME",
    "MAX_STATEMENT_TIME",
    "TRANSACTION_ISOLATION",
    "TX_ISOLATION",
    "GROUP_CONCAT_MAX_LEN",
    "DIV_PRECISION_INCREMENT",
    "DEFAULT_WEEK_FORMAT",
    "OPTIMIZER_SWITCH",
    "OPTIMIZER_SEARCH_DEPTH",
    "OPTIMIZER_PRUNE_LEVEL",
    "JOIN_BUFFER_SIZE",
    "SORT_BUFFER_SIZE",
    "READ_BUFFER_SIZE",
    "READ_RND_BUFFER_SIZE",
    "TMP_TABLE_SIZE",
    "MAX_HEAP_TABLE_SIZE",
    "BIG_TABLES",
    "SQL_SAFE_UPDATES",
    "CTE_MAX_RECURSION_DEPTH",
    "MAX_SP_RECURSION_DEPTH",
    "LONG_QUERY_TIME",
    "PROFILING",
    "PROFILING_HISTORY_SIZE",
    "DEFAULT_STORAGE_ENGINE",
];

/// The PostgreSQL settings an init may make, besides `SET TIME ZONE`, the
/// planner's `enable_*` switches ([`POSTGRES_SETTING_PREFIX`]) and a custom
/// `prefix.name` setting (an application's or an extension's, the usual
/// carrier of a row-level-security tenant). Not here, on purpose:
/// `standard_conforming_strings`, the transaction characteristics and
/// read-only default, `session_replication_role` (it switches triggers and
/// foreign keys off), `bytea_output`/`extra_float_digits` (how values the app
/// reads are spelt), `client_encoding` with a value other than UTF-8, and
/// anything else nobody has weighed yet.
const POSTGRES_SETTINGS: &[&str] = &[
    "SEARCH_PATH",
    "SCHEMA",
    "TIMEZONE",
    "DATESTYLE",
    "INTERVALSTYLE",
    "STATEMENT_TIMEOUT",
    "LOCK_TIMEOUT",
    "IDLE_IN_TRANSACTION_SESSION_TIMEOUT",
    "IDLE_SESSION_TIMEOUT",
    "TRANSACTION_TIMEOUT",
    "APPLICATION_NAME",
    "WORK_MEM",
    "MAINTENANCE_WORK_MEM",
    "TEMP_BUFFERS",
    "HASH_MEM_MULTIPLIER",
    "ROLE",
    "AUTHORIZATION",
    "LC_MESSAGES",
    "LC_MONETARY",
    "LC_NUMERIC",
    "LC_TIME",
    "RANDOM_PAGE_COST",
    "SEQ_PAGE_COST",
    "CPU_TUPLE_COST",
    "CPU_INDEX_TUPLE_COST",
    "CPU_OPERATOR_COST",
    "EFFECTIVE_CACHE_SIZE",
    "EFFECTIVE_IO_CONCURRENCY",
    "JIT",
    "PLAN_CACHE_MODE",
    "FROM_COLLAPSE_LIMIT",
    "JOIN_COLLAPSE_LIMIT",
    "GEQO",
    "MAX_PARALLEL_WORKERS_PER_GATHER",
    "DEFAULT_STATISTICS_TARGET",
    "CONSTRAINT_EXCLUSION",
    "CLIENT_MIN_MESSAGES",
    "ROW_SECURITY",
    "DEFAULT_TABLESPACE",
    "TEMP_TABLESPACES",
    "DEFAULT_TRANSACTION_ISOLATION",
    "DEFAULT_TRANSACTION_DEFERRABLE",
    "DEFAULT_TEXT_SEARCH_CONFIG",
    "CHECK_FUNCTION_BODIES",
];

/// The prefix every PostgreSQL planner method switch shares (`enable_seqscan`,
/// `enable_hashjoin`, …): each only steers the plan.
const POSTGRES_SETTING_PREFIX: &str = "ENABLE_";

/// SQL Server's settings that take a value (`SET LANGUAGE british`).
const MSSQL_VALUE_SETTINGS: &[&str] = &[
    "LANGUAGE",
    "DATEFORMAT",
    "DATEFIRST",
    "LOCK_TIMEOUT",
    "DEADLOCK_PRIORITY",
    "CONTEXT_INFO",
    "QUERY_GOVERNOR_COST_LIMIT",
];

/// SQL Server's `ON`/`OFF` switches an init may turn either way.
const MSSQL_SWITCHES: &[&str] = &[
    "XACT_ABORT",
    "ARITHIGNORE",
    "NUMERIC_ROUNDABORT",
    "FORCEPLAN",
];

/// SQL Server's ISO switches an init may only turn **on** — their default and
/// what Schemaic's SQL is written for: `QUOTED_IDENTIFIER OFF` makes `"x"` a
/// string, `ANSI_WARNINGS OFF` stores an over-long value truncated instead of
/// failing the write. Off is refused. (`ANSI_DEFAULTS` is on no list: it
/// turns `IMPLICIT_TRANSACTIONS` on.)
const MSSQL_ON_ONLY: &[&str] = &[
    "ANSI_NULLS",
    "ANSI_PADDING",
    "ANSI_WARNINGS",
    "QUOTED_IDENTIFIER",
    "ARITHABORT",
    "CONCAT_NULL_YIELDS_NULL",
];

/// `sql`'s statements, each trimmed and without its terminator, if every one
/// is a setting an init may make on `dialect` — else why the first one that is
/// not, in words the connection form shows. Empty SQL is no statements.
pub fn check(sql: &str, dialect: SqlDialect) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for (lo, hi) in statement_ranges(sql, dialect) {
        let stmt = sql[lo..hi].trim().trim_end_matches(';').trim();
        let toks = tokens(stmt, dialect);
        let Some(head) = toks.first() else {
            continue; // a comment alone
        };
        let shown = crate::history::preview(stmt);
        let pragmas = settings_are_pragmas(dialect);
        if !is_word(Some(head), if pragmas { "PRAGMA" } else { "SET" }) {
            return Err(if pragmas {
                format!("“{shown}”: only PRAGMA settings can run when connecting")
            } else {
                format!("“{shown}”: only SET statements can run when connecting")
            });
        }
        if toks.contains(&Tok::Punct(b'(')) {
            return Err(format!(
                "“{shown}”: a setting cannot call a function when connecting"
            ));
        }
        let rest = &toks[1..];
        let verdict = match dialect {
            SqlDialect::Sqlite => pragma(rest),
            SqlDialect::MySql => mysql(rest, dialect),
            SqlDialect::Postgres => postgres(rest, dialect),
            SqlDialect::MsSql => mssql(rest, dialect),
        };
        verdict.map_err(|why| format!("“{shown}”: {why}"))?;
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

/// One token of an init statement, as [`tokens`] reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    /// A bare word, upper-cased.
    Word(String),
    /// A quoted name — its spelling is never compared, only refused where a
    /// setting's name would be.
    Quoted,
    /// A string literal's text, without its quotes.
    Str(String),
    /// Any other byte at code level (`=`, `,`, `.`, `@`, `(`).
    Punct(u8),
}

/// `stmt`'s tokens, comments dropped — strings, quoted names and comments
/// found by [`skip_noncode`], the one boundary lexer, and a quoted name told
/// from a string by `intel::ident_quote`, the one per-dialect answer to which
/// bytes quote a name. A quoted name is kept as a token rather than skipped,
/// which is what let `` SET `autocommit` = 0 `` through the old word scan.
fn tokens(stmt: &str, dialect: SqlDialect) -> Vec<Tok> {
    let b = stmt.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if let Some(j) = skip_noncode(b, i, dialect).filter(|&j| j > i) {
            if crate::intel::ident_quote(dialect, b[i]).is_some() {
                out.push(Tok::Quoted);
            } else if matches!(b[i], b'\'' | b'"' | b'$') {
                let span = &stmt[i..j];
                let inner = span
                    .strip_prefix(b[i] as char)
                    .and_then(|s| s.strip_suffix(b[i] as char))
                    .unwrap_or(span);
                out.push(Tok::Str(inner.to_string()));
            }
            // Anything else `skip_noncode` covers is a comment.
            i = j;
        } else if is_word_byte(b[i]) {
            let s = i;
            while i < b.len() && is_word_byte(b[i]) {
                i += 1;
            }
            out.push(Tok::Word(stmt[s..i].to_ascii_uppercase()));
        } else {
            if !b[i].is_ascii_whitespace() {
                out.push(Tok::Punct(b[i]));
            }
            i += 1;
        }
    }
    out
}

/// Is `t` the bare word `w` (upper-case)?
fn is_word(t: Option<&Tok>, w: &str) -> bool {
    matches!(t, Some(Tok::Word(x)) if x == w)
}

/// The setting name `t` spells, or why it names none: a quoted name is the
/// same setting to the server, so it cannot be let past a list of bare ones.
fn name_of(t: Option<&Tok>) -> Result<&str, String> {
    match t {
        Some(Tok::Word(w)) => Ok(w),
        Some(Tok::Quoted) => Err("a setting's name cannot be quoted when connecting".into()),
        _ => Err("that names no setting".into()),
    }
}

/// The refusal for a setting on no list.
fn not_listed(name: &str) -> String {
    format!(
        "{} cannot be set when connecting — it is not one of the session settings Schemaic \
         knows leave its guards alone",
        name.to_ascii_lowercase()
    )
}

/// The refusal for a client character set other than UTF-8.
fn not_utf8() -> String {
    "the client character set must stay UTF-8 — Schemaic's read-only check reads every \
     statement as UTF-8"
        .into()
}

/// A literal value's text — a bare word or a string.
fn value_text(t: Option<&Tok>) -> Option<&str> {
    match t {
        Some(Tok::Word(w)) | Some(Tok::Str(w)) => Some(w),
        _ => None,
    }
}

/// `v` is one character-set value `utf8` accepts, optionally followed — where
/// `collate` — by `COLLATE` and a UTF-8 collation, and nothing else.
fn charset(v: &[Tok], utf8: fn(&str) -> bool, collate: bool) -> Result<(), String> {
    if !value_text(v.first()).is_some_and(utf8) {
        return Err(not_utf8());
    }
    match &v[1..] {
        [] => Ok(()),
        [c, coll] if collate && is_word(Some(c), "COLLATE") => match value_text(Some(coll)) {
            Some(n) if n.to_ascii_lowercase().starts_with("utf8") => Ok(()),
            _ => Err(not_utf8()),
        },
        _ => Err(not_utf8()),
    }
}

/// MySQL's names for UTF-8.
fn mysql_utf8(v: &str) -> bool {
    matches!(
        v.to_ascii_lowercase().as_str(),
        "utf8mb4" | "utf8" | "utf8mb3"
    )
}

/// PostgreSQL's names for UTF-8, which it reads ignoring case and punctuation.
fn postgres_utf8(v: &str) -> bool {
    let n: String = v
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    n == "utf8" || n == "unicode"
}

/// A `SET TRANSACTION …` — only an isolation level, and only where the
/// read-only guard is a rollback, which an isolation level cannot lift;
/// elsewhere the transaction characteristics *are* the guard.
fn transaction(dialect: SqlDialect, after: &[Tok]) -> Result<(), String> {
    if dialect.read_only_is_a_rollback()
        && is_word(after.first(), "ISOLATION")
        && is_word(after.get(1), "LEVEL")
    {
        Ok(())
    } else {
        Err(
            "transaction settings cannot be made when connecting — the read-only guard sets them"
                .into(),
        )
    }
}

/// A SQLite `PRAGMA` (after the head): a listed name, unqualified, with an
/// `=` value or none.
fn pragma(rest: &[Tok]) -> Result<(), String> {
    let refused = || {
        "that PRAGMA cannot run when connecting — it changes the file or the read-only guard"
            .to_string()
    };
    let name = name_of(rest.first()).map_err(|_| refused())?;
    if SQLITE_PRAGMAS.contains(&name) && matches!(rest.get(1), None | Some(Tok::Punct(b'='))) {
        Ok(())
    } else {
        Err(refused())
    }
}

/// A MySQL/MariaDB `SET` (after the head): `SET ROLE …`, or a comma list of
/// assignments, each checked by [`mysql_assignment`].
fn mysql(rest: &[Tok], dialect: SqlDialect) -> Result<(), String> {
    if is_word(rest.first(), "ROLE") {
        return Ok(());
    }
    rest.split(|t| *t == Tok::Punct(b','))
        .try_for_each(|piece| mysql_assignment(piece, dialect))
}

/// One MySQL assignment: `@user = …` (a user variable, which sets nothing
/// Schemaic reads), or `[SESSION|LOCAL] name = …`, `@@[session.]name = …`,
/// `NAMES …`, `CHARACTER SET …` — the name on [`MYSQL_SETTINGS`], the scope
/// never `GLOBAL`/`PERSIST`/`PERSIST_ONLY`.
fn mysql_assignment(p: &[Tok], dialect: SqlDialect) -> Result<(), String> {
    let at = p.iter().take_while(|t| **t == Tok::Punct(b'@')).count();
    if at == 1 {
        return Ok(());
    }
    let mut i = at;
    if let Some(Tok::Word(scope)) = p.get(i) {
        let dotted = p.get(i + 1) == Some(&Tok::Punct(b'.'));
        let scoped = if at == 2 { dotted } else { p.len() > i + 1 };
        if scoped {
            match scope.as_str() {
                "GLOBAL" | "PERSIST" | "PERSIST_ONLY" => {
                    return Err(format!(
                        "{} settings change the server, not the session",
                        scope.to_ascii_lowercase()
                    ));
                }
                "SESSION" | "LOCAL" => i += if at == 2 { 2 } else { 1 },
                _ if at == 2 => return Err(not_listed(scope)),
                _ => {}
            }
        }
    }
    let name = name_of(p.get(i))?;
    match name {
        "NAMES" => charset(&p[i + 1..], mysql_utf8, true),
        "CHARSET" => charset(&p[i + 1..], mysql_utf8, false),
        "CHARACTER" if is_word(p.get(i + 1), "SET") => charset(&p[i + 2..], mysql_utf8, false),
        "TRANSACTION" => transaction(dialect, &p[i + 1..]),
        n if MYSQL_SETTINGS.contains(&n) => Ok(()),
        n => Err(not_listed(n)),
    }
}

/// A PostgreSQL `SET` (after the head): `[SESSION|LOCAL] name {TO|=} …`, the
/// name on [`POSTGRES_SETTINGS`] or a planner switch or a custom
/// `prefix.name`; `TIME ZONE …`; `NAMES`/`client_encoding` with UTF-8 only.
fn postgres(rest: &[Tok], dialect: SqlDialect) -> Result<(), String> {
    let mut i = 0;
    if (is_word(rest.first(), "SESSION") || is_word(rest.first(), "LOCAL")) && rest.len() > 1 {
        i = 1;
    }
    let name = name_of(rest.get(i))?;
    if rest.get(i + 1) == Some(&Tok::Punct(b'.')) {
        // A custom setting: an application's (`app.tenant_id`) or an
        // extension's. Only the server's own settings are unqualified.
        return name_of(rest.get(i + 2)).map(|_| ());
    }
    // `client_encoding TO x` / `= x` — the value after its connective.
    let value = |from: usize| {
        let skip =
            usize::from(is_word(rest.get(from), "TO") || rest.get(from) == Some(&Tok::Punct(b'=')));
        &rest[(from + skip).min(rest.len())..]
    };
    match name {
        "TIME" if is_word(rest.get(i + 1), "ZONE") => Ok(()),
        "NAMES" => charset(&rest[i + 1..], postgres_utf8, false),
        "CLIENT_ENCODING" => charset(value(i + 1), postgres_utf8, false),
        "TRANSACTION" => transaction(dialect, &rest[i + 1..]),
        n if POSTGRES_SETTINGS.contains(&n) || n.starts_with(POSTGRES_SETTING_PREFIX) => Ok(()),
        n => Err(not_listed(n)),
    }
}

/// A SQL Server `SET` (after the head): a value setting
/// ([`MSSQL_VALUE_SETTINGS`]), `TRANSACTION ISOLATION LEVEL …`, or a comma
/// list of switches and one `ON`/`OFF` for them all — each on
/// [`MSSQL_SWITCHES`], or on [`MSSQL_ON_ONLY`] and turned on.
fn mssql(rest: &[Tok], dialect: SqlDialect) -> Result<(), String> {
    if rest.first() == Some(&Tok::Punct(b'@')) {
        return Err("a variable is not a session setting — it ends with its batch".into());
    }
    let name = name_of(rest.first())?;
    if name == "TRANSACTION" {
        return transaction(dialect, &rest[1..]);
    }
    if MSSQL_VALUE_SETTINGS.contains(&name) {
        return Ok(());
    }
    let Some((value, names)) = rest.split_last() else {
        return Err(not_listed(name));
    };
    let on = match value {
        Tok::Word(v) if v == "ON" => true,
        Tok::Word(v) if v == "OFF" => false,
        _ => return Err(not_listed(name)),
    };
    for (k, t) in names.iter().enumerate() {
        if k % 2 == 1 {
            if *t != Tok::Punct(b',') {
                return Err(not_listed(name));
            }
            continue;
        }
        let n = name_of(Some(t))?;
        if MSSQL_ON_ONLY.contains(&n) {
            if !on {
                return Err(format!(
                    "{} can only be turned on when connecting — Schemaic relies on it",
                    n.to_ascii_lowercase()
                ));
            }
        } else if !MSSQL_SWITCHES.contains(&n) {
            return Err(not_listed(n));
        }
    }
    Ok(())
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
        let e = check("SET time_zone = '+00:00';\nSET autocommit = 0", MY).unwrap_err();
        assert!(
            e.contains("SET autocommit = 0") && e.contains("autocommit"),
            "{e}"
        );
    }

    /// S1.1-L5-01 / R3-L5-02: settings under which a write silently does not
    /// land (`ANSI_DEFAULTS` implies `IMPLICIT_TRANSACTIONS`; `FMTONLY`,
    /// `NOEXEC`, `PARSEONLY` make every statement a successful no-op) or a
    /// read silently delivers less (`sql_select_limit`, `TEXTSIZE`), or an
    /// over-long write is stored truncated (`ANSI_WARNINGS OFF`).
    #[test]
    fn settings_that_make_an_operation_deliver_less_are_refused() {
        for (sql, d) in [
            ("SET ANSI_DEFAULTS ON", MS),
            ("SET FMTONLY ON", MS),
            ("SET NOEXEC ON", MS),
            ("SET PARSEONLY ON", MS),
            ("SET TEXTSIZE 100", MS),
            ("SET ANSI_WARNINGS OFF", MS),
            ("SET ANSI_NULLS, ANSI_WARNINGS OFF", MS),
            ("SET sql_select_limit = 1000", MY),
            ("SET SESSION sql_select_limit = 1000", MY),
            ("SET @@sql_select_limit = 1000", MY),
            ("SET time_zone = '+00:00', sql_select_limit = 5", MY),
        ] {
            assert!(check(sql, d).is_err(), "{sql}");
        }
    }

    /// S1.1-L5-02: a client character set other than UTF-8 moves where the
    /// server ends a string away from where `skip_noncode` does, which is the
    /// read-only text gate's whole footing. `QUOTED_IDENTIFIER OFF` moves a
    /// `"` from a name to a string the same way on SQL Server.
    #[test]
    fn a_client_character_set_other_than_utf8_is_refused() {
        for (sql, d) in [
            ("SET NAMES gbk", MY),
            ("SET NAMES 'sjis'", MY),
            ("SET NAMES utf8mb4 COLLATE gbk_chinese_ci", MY),
            ("SET CHARACTER SET big5", MY),
            ("SET CHARSET cp932", MY),
            ("SET character_set_client = gbk", MY),
            ("SET time_zone = '+00:00', NAMES gb18030", MY),
            ("SET client_encoding = 'SJIS'", PG),
            ("SET client_encoding TO GBK", PG),
            ("SET NAMES 'BIG5'", PG),
            ("SET QUOTED_IDENTIFIER OFF", MS),
        ] {
            assert!(check(sql, d).is_err(), "{sql}");
        }
        for (sql, d) in [
            ("SET NAMES utf8mb4", MY),
            ("SET NAMES 'utf8mb4' COLLATE 'utf8mb4_unicode_ci'", MY),
            ("SET CHARACTER SET utf8mb4", MY),
            ("SET client_encoding = 'UTF8'", PG),
            ("SET NAMES 'utf-8'", PG),
            ("SET QUOTED_IDENTIFIER ON", MS),
        ] {
            assert!(check(sql, d).is_ok(), "{sql}: {:?}", check(sql, d));
        }
    }

    /// R1-L2-01: a quoted setting name is the same setting to the server, so
    /// quoting must not carry one past the gate.
    #[test]
    fn a_quoted_setting_name_is_still_denied() {
        for (sql, d) in [
            ("SET `autocommit` = 0", MY),
            ("SET SESSION `sql_mode` = ''", MY),
            ("SET @@`sql_select_limit` = 1", MY),
            ("SET \"standard_conforming_strings\" = off", PG),
            ("SET \"default_transaction_read_only\" = off", PG),
            ("SET [NOCOUNT] ON", MS),
            ("PRAGMA \"query_only\" = foreign_keys", LITE),
        ] {
            assert!(check(sql, d).is_err(), "{sql}");
        }
    }

    /// The settings people put in an init SQL keep passing.
    #[test]
    fn everyday_session_settings_pass() {
        for (sql, d) in [
            ("SET search_path = \"$user\", public", PG),
            ("SET SESSION search_path TO app", PG),
            ("SET TIME ZONE 'UTC'", PG),
            ("SET timezone TO 'Europe/London'", PG),
            ("SET lock_timeout = '5s'", PG),
            ("SET statement_timeout = 30000", PG),
            ("SET LOCAL work_mem = '64MB'", PG),
            ("SET ROLE \"Reporting Team\"", PG),
            ("SET SESSION AUTHORIZATION app_user", PG),
            ("SET app.tenant_id = '42'", PG),
            ("SET enable_seqscan = off", PG),
            ("SET DateStyle = 'ISO, DMY'", PG),
            ("SET default_transaction_isolation = 'serializable'", PG),
            ("SET time_zone = 'Europe/Berlin'", MY),
            ("SET SESSION time_zone = '+00:00'", MY),
            ("SET @@session.time_zone = '+00:00'", MY),
            ("SET @@time_zone = '+00:00', @@lc_time_names = 'de_DE'", MY),
            ("SET LOCAL max_execution_time = 10000", MY),
            ("SET max_statement_time = 10", MY),
            ("SET group_concat_max_len = 1000000", MY),
            ("SET transaction_isolation = 'READ-COMMITTED'", MY),
            ("SET @tenant = 42", MY),
            ("SET ROLE reporting, auditor", MY),
            ("SET DATEFORMAT dmy", MS),
            ("SET LOCK_TIMEOUT 5000", MS),
            ("SET DEADLOCK_PRIORITY LOW", MS),
            ("SET XACT_ABORT ON", MS),
            ("SET ANSI_NULLS, QUOTED_IDENTIFIER ON", MS),
            ("SET TRANSACTION ISOLATION LEVEL READ COMMITTED", MS),
            ("PRAGMA busy_timeout = 5000", LITE),
        ] {
            assert!(check(sql, d).is_ok(), "{sql}: {:?}", check(sql, d));
        }
    }

    /// The list is an allowlist of names: a setting nobody has weighed is
    /// refused rather than let through.
    #[test]
    fn a_setting_nobody_has_weighed_is_refused() {
        for (sql, d) in [
            ("SET session_replication_role = replica", PG),
            ("SET no_such_setting_here = 1", PG),
            ("SET foreign_key_checks = 0", MY),
            ("SET STATISTICS XML ON", MS),
            ("SET @x = 1", MS),
            ("PRAGMA main.query_only = 1", LITE),
        ] {
            assert!(check(sql, d).is_err(), "{sql}");
        }
    }
}
