//! Manual-transaction state — pure over statement outcomes, no DB, no UI.
//!
//! In **Manual** mode a query tab pins one connection open and holds a
//! transaction across many UI actions, so the user decides when to `COMMIT` or
//! `ROLLBACK`. The connection pinning lives in `schemaic-db`'s `Session` and the
//! wiring in the app; what belongs *here* is the decision logic: how a statement
//! outcome moves the transaction, which engine poisons a transaction on error,
//! which statements silently end one, and what the status pill reads.
//!
//! The engine divergence is the whole reason this is a state machine rather than
//! a boolean:
//!
//! * **PostgreSQL** aborts the entire transaction on *any* statement error
//!   (`25P02` — "current transaction is aborted, commands ignored until end of
//!   transaction block"), so the only way forward is `ROLLBACK`. A cancelled
//!   statement (`57014`) is an error too, so it poisons as well.
//! * **MySQL/MariaDB** leaves the transaction usable after a failed statement,
//!   but *implicitly commits* it when DDL runs mid-transaction — the transaction
//!   is silently gone, which the UI has to say out loud.

/// Which engine's transaction semantics apply. Mirrors `schemaic_db::Engine`
/// (core doesn't depend on the db crate).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TxEngine {
    #[default]
    MySql,
    Postgres,
}

/// A tab's commit mode. Session-only — a tab always starts in [`TxMode::Auto`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TxMode {
    /// Every statement commits on its own, each on a fresh connection. The
    /// behaviour Schemaic has always had.
    #[default]
    Auto,
    /// Statements run on one pinned connection inside a transaction the user
    /// commits or rolls back explicitly.
    Manual,
}

impl TxMode {
    /// Status-bar label.
    pub fn label(self) -> &'static str {
        match self {
            TxMode::Auto => "Auto-commit",
            TxMode::Manual => "Manual",
        }
    }

    pub fn is_manual(self) -> bool {
        matches!(self, TxMode::Manual)
    }
}

/// What happened to one statement run on the session connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StmtOutcome {
    Ok,
    /// The server rejected it (syntax, constraint, permission…).
    Failed,
    /// The server rejected it, but it ran inside its own `SAVEPOINT` and that
    /// savepoint has already been rolled back — so the enclosing transaction is
    /// untouched and still committable.
    ///
    /// Distinct from [`StmtOutcome::Failed`] because on PostgreSQL the two have
    /// opposite consequences: a bare failure aborts the transaction, while a
    /// savepoint-isolated one is exactly what the savepoint exists to prevent.
    /// Reporting the second as the first tells the user their work is lost and
    /// offers only the action that loses it.
    FailedIsolated,
    /// The user cancelled it (`KILL QUERY` / PG cancel request).
    Cancelled,
    /// The connection itself died — idle-in-transaction timeout, server
    /// restart, network drop. Whatever was in the transaction is gone.
    ConnectionLost,
}

/// Where a tab's transaction stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TxState {
    /// No transaction open (always the case in [`TxMode::Auto`]).
    #[default]
    Idle,
    /// Open, with the number of statements run inside it so far.
    Open { stmts: u32 },
    /// PostgreSQL only: a statement errored, so the server rejects everything
    /// until `ROLLBACK`. The count is kept for the pill.
    Poisoned { stmts: u32 },
    /// The pinned connection died with a transaction open — the work is gone.
    /// Reported rather than silently reconnected into a fresh, empty one.
    Lost,
}

impl TxState {
    /// Is a transaction open in any form (including poisoned)? Drives the
    /// "you'll lose work" prompts on close / disconnect / mode switch.
    pub fn is_open(self) -> bool {
        matches!(self, TxState::Open { .. } | TxState::Poisoned { .. })
    }

    /// Statements run inside the current transaction.
    pub fn stmts(self) -> u32 {
        match self {
            TxState::Open { stmts } | TxState::Poisoned { stmts } => stmts,
            TxState::Idle | TxState::Lost => 0,
        }
    }

    /// Can the user commit right now? A poisoned transaction can't — PostgreSQL
    /// turns `COMMIT` on an aborted transaction into a `ROLLBACK`, so offering
    /// Commit would be a lie. A lost one has nothing to commit.
    pub fn can_commit(self) -> bool {
        matches!(self, TxState::Open { .. })
    }

    /// Can the user roll back? Any live transaction, poisoned included — that's
    /// the way out of `25P02`.
    pub fn can_rollback(self) -> bool {
        self.is_open()
    }

    /// `BEGIN` succeeded. Lazily called on the first statement of a Manual tab.
    pub fn begun() -> TxState {
        TxState::Open { stmts: 0 }
    }

    /// A `COMMIT`/`ROLLBACK` completed, or the session was closed — either way
    /// there's no transaction any more. Also the way out of `Poisoned`/`Lost`.
    pub fn closed() -> TxState {
        TxState::Idle
    }

    /// Fold one statement's outcome into the transaction state.
    ///
    /// `sql` is the statement that ran — needed because MySQL DDL implicitly
    /// commits (see [`implicit_commit`]). A statement arriving while [`TxState::Idle`]
    /// opens the transaction (the app issues `BEGIN` lazily, so the first
    /// statement lands as the first statement of a fresh transaction).
    pub fn on_statement(self, engine: TxEngine, sql: &str, outcome: StmtOutcome) -> TxState {
        if outcome == StmtOutcome::ConnectionLost {
            // Only meaningful mid-transaction; with none open there's nothing to
            // mourn and the next op just reconnects.
            return if self.is_open() {
                TxState::Lost
            } else {
                TxState::Idle
            };
        }
        match self {
            TxState::Lost => TxState::Lost,
            // Postgres rejects everything until ROLLBACK, so nothing counts.
            TxState::Poisoned { stmts } => TxState::Poisoned { stmts },
            TxState::Idle | TxState::Open { .. } => {
                let stmts = self.stmts();
                match outcome {
                    StmtOutcome::Ok => {
                        if implicit_commit(engine, sql) {
                            // MySQL DDL committed the transaction out from under
                            // us — back to no transaction, not to `stmts + 1`.
                            TxState::Idle
                        } else {
                            TxState::Open { stmts: stmts + 1 }
                        }
                    }
                    // A statement that didn't apply doesn't count. On Postgres it
                    // also poisons: both a server error and a cancellation leave
                    // the transaction in the aborted state.
                    StmtOutcome::Failed | StmtOutcome::Cancelled => match engine {
                        TxEngine::Postgres => TxState::Poisoned { stmts },
                        TxEngine::MySql => TxState::Open { stmts },
                    },
                    // Its savepoint already absorbed the abort, so the enclosing
                    // transaction is untouched on either engine — it just gains
                    // no statement.
                    StmtOutcome::FailedIsolated => TxState::Open { stmts },
                    StmtOutcome::ConnectionLost => unreachable!("handled above"),
                }
            }
        }
    }
}

/// Does running `sql` silently end an open transaction?
///
/// MySQL/MariaDB has no transactional DDL: `CREATE`/`ALTER`/`DROP`/`TRUNCATE`/
/// `RENAME`, and a few session statements, commit the current transaction before
/// they run ("statements that cause an implicit commit"). PostgreSQL has fully
/// transactional DDL, so nothing does this there.
///
/// The keyword is read with [`crate::sql::leading_keyword`], so it's the shared
/// boundary lexer deciding what the first token is — a leading comment or an
/// `/* … */` block can't fool it.
///
/// **A miss is not harmless, and the direction matters.** After a statement the
/// server committed but this didn't match, the pill keeps counting: a subsequent
/// **Commit** still reaches the user's intended outcome, but a **Rollback** runs
/// as a successful no-op and the UI reports an undo that never happened, over
/// data that is now permanently written. A false *positive* is the mirror image —
/// the pill goes quiet over an open transaction — so this matches MySQL's
/// documented set as closely as a keyword can and no wider. That is still a
/// guess: both engines report transaction status on the wire (MySQL's
/// `SERVER_STATUS_IN_TRANS`, PostgreSQL's `ReadyForQuery`), and a pinned
/// [`crate::tx`] session reading it would replace this with the truth, leaving
/// this as the fallback.
pub fn implicit_commit(engine: TxEngine, sql: &str) -> bool {
    if engine != TxEngine::MySql {
        return false;
    }
    let dialect = crate::intel::SqlDialect::MySql;
    let Some(kw) = crate::sql::leading_keyword(sql, dialect) else {
        return false;
    };
    if kw == "SET" {
        return set_commits(sql, dialect);
    }
    matches!(
        kw.as_str(),
        "ALTER"
            | "ANALYZE"
            | "CACHE"
            | "CHECK"
            | "CREATE"
            | "DROP"
            | "FLUSH"
            | "GRANT"
            | "INSTALL"
            // `LOAD INDEX INTO CACHE`. `LOAD DATA` doesn't commit, but it is a
            // client-side statement Schemaic doesn't run, so this doesn't
            // distinguish them.
            | "LOAD"
            | "LOCK"
            | "OPTIMIZE"
            | "RENAME"
            | "REPAIR"
            | "REVOKE"
            | "TRUNCATE"
            | "UNINSTALL"
            | "UNLOCK"
            // Opening a new transaction commits the current one.
            | "BEGIN"
            | "START"
    )
}

/// After `sql` ran **successfully**, does the connection still have an open
/// transaction? `None` means "unchanged" — the ordinary case.
///
/// This is the pinned session's own flag ([`schemaic_db::Session::ensure_tx`]),
/// not the pill's [`TxState`]: the session has to know whether to issue a
/// `BEGIN` *before* the next statement, and it decides that under the same lock
/// the `BEGIN` goes out on.
///
/// The whole reason this isn't just [`implicit_commit`] is the last arm of that
/// list. `BEGIN` and `START TRANSACTION` implicitly commit — opening a
/// transaction ends the current one — but unlike every other entry they leave a
/// **new** transaction open. Treating them like `DROP TABLE` would clear the
/// flag, the next statement would decide it needed its own `BEGIN`, and on MySQL
/// that second `BEGIN` would implicitly commit everything in between. That is
/// [B12.1-L1-01] arriving from the other direction, which is why the carve-out
/// is a named function with tests rather than a `matches!` inside the session.
///
/// [`schemaic_db::Session::ensure_tx`]: https://docs.rs/schemaic-db
pub fn tx_open_after(engine: TxEngine, sql: &str) -> Option<bool> {
    if !implicit_commit(engine, sql) {
        return None;
    }
    // `implicit_commit` is false for every non-MySQL engine, so reaching here
    // means MySQL and the dialect is settled.
    let opens = matches!(
        crate::sql::leading_keyword(sql, crate::intel::SqlDialect::MySql).as_deref(),
        Some("BEGIN") | Some("START")
    );
    Some(opens)
}

/// Does this `SET …` statement implicitly commit? Only two forms do, so `SET`
/// can't join the list wholesale — `SET NAMES`, `SET @x`, `SET SESSION sql_mode`
/// and the rest leave the transaction exactly where it was.
///
/// - `SET PASSWORD …`
/// - `SET autocommit = 1` — turning it **on**, and only for this session. `= 0`
///   commits nothing, and `SET GLOBAL autocommit` isn't this session's variable.
///
/// Anything it can't read is not a commit: claiming one that didn't happen would
/// hide an open transaction, which is the failure this function's caller reports
/// on.
fn set_commits(sql: &str, dialect: crate::intel::SqlDialect) -> bool {
    // Tokens after `SET`, upper-cased, split on the punctuation that separates a
    // variable from its scope and its value.
    let after = crate::sql::leading_keyword_end(sql, dialect).map_or("", |e| &sql[e..]);
    let mut words = after
        .split(|c: char| c.is_whitespace() || matches!(c, '=' | '.' | ',' | ';' | ':'))
        .filter(|w| !w.is_empty())
        .map(|w| w.trim_start_matches('@').to_ascii_uppercase());

    let Some(first) = words.next() else {
        return false;
    };
    if first == "PASSWORD" {
        return true;
    }
    // An optional scope word. `GLOBAL`/`PERSIST` set a variable this session's
    // transaction doesn't read, so they're excluded rather than skipped.
    let name = match first.as_str() {
        "SESSION" | "LOCAL" => match words.next() {
            Some(w) => w,
            None => return false,
        },
        _ => first,
    };
    if name != "AUTOCOMMIT" {
        return false;
    }
    matches!(words.next().as_deref(), Some("1" | "ON" | "TRUE"))
}

/// A pinned session has finished connecting — does the tab that asked for it
/// still want it?
///
/// `mode` is the tab's current mode, or `None` when the tab is **gone**: opening
/// a session is a full connect (seconds through an SSH tunnel), and the tab can
/// be closed or flipped back to Auto while it is in flight. Both answers are
/// "close it", and the `None` one is the reason this is a function: a session
/// filed under a closed tab's id is a connection — and any transaction on it —
/// held until the process exits, because the `drop_session` that would have
/// removed it already ran, before the entry existed.
pub fn session_still_wanted(mode: Option<TxMode>) -> bool {
    matches!(mode, Some(TxMode::Manual))
}

/// One tab's transaction, as much of it as [`ddl_blocking_tabs`] needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TabTx {
    pub tab_id: usize,
    /// The connection the tab's pinned session is on.
    pub conn_id: u64,
    pub state: TxState,
}

/// Which tabs' open transactions a schema change against `conn_id` would have to
/// queue behind — the tabs to ask about before applying, in tab order.
///
/// A schema change falls between the two halves of the one-connection-per-
/// operation rule: it is the tab's own work *and* a write, so it runs on a fresh
/// connection like every side channel — and then waits, on that connection, for
/// the metadata lock (MySQL) or `ACCESS EXCLUSIVE` (PostgreSQL) that the user's
/// own uncommitted `SELECT` is holding. Nothing times out, so Apply simply never
/// returns.
///
/// **Scope is the connection, not the database or the table.** Schemaic doesn't
/// track which tables a transaction has touched, and a MySQL statement can name
/// any database on the server, so anything narrower would silently miss the case
/// the prompt exists for. The cost of being conservative is a question the user
/// answers with "apply anyway"; the cost of being precise-but-wrong is the hang.
///
/// [`TxState::Lost`] is not blocking: the connection that held the locks is gone,
/// so the server released them.
pub fn ddl_blocking_tabs(tabs: &[TabTx], conn_id: u64) -> Vec<usize> {
    tabs.iter()
        .filter(|t| t.conn_id == conn_id && t.state.is_open())
        .map(|t| t.tab_id)
        .collect()
}

/// Which of *our own* tabs' transactions could be holding a lock that a write
/// from `writer_tab` is queued behind, in tab order.
///
/// The writer's own tab is excluded, and that is the whole difference from
/// [`ddl_blocking_tabs`]: a grid write from a Manual tab runs on that tab's
/// pinned session, *inside* its own transaction, so it cannot wait on itself. A
/// schema change runs on a fresh connection and therefore does queue behind the
/// tab's own uncommitted work — hence two functions rather than one.
///
/// Scope is the connection, for the reason spelled out on `ddl_blocking_tabs`.
/// Over-reporting is cheaper here than there: this answers a wait that is
/// already happening rather than gating an action, and what it produces is a
/// sentence saying a transaction *may* be responsible.
pub fn write_blocking_tabs(tabs: &[TabTx], conn_id: u64, writer_tab: usize) -> Vec<usize> {
    tabs.iter()
        .filter(|t| t.tab_id != writer_tab && t.conn_id == conn_id && t.state.is_open())
        .map(|t| t.tab_id)
        .collect()
}

/// What to say about a write that hasn't come back yet — see [`write_wait_note`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WaitNote {
    pub text: String,
    /// The tab (id + title) to offer a one-click `ROLLBACK` for, when exactly
    /// one of ours is a candidate.
    ///
    /// `None` for none — there is nothing of ours to end — and `None` for
    /// several, where one button would have to choose, and choosing wrong
    /// discards a transaction the user never meant to end. They can still roll
    /// back any of the named tabs from its own status bar.
    pub rollback: Option<(usize, String)>,
}

/// How long a grid write may be outstanding before Schemaic narrates the wait.
///
/// A commit that returns promptly needs no narration, and a note on every commit
/// is noise the user learns to look past. Past this, round-trip time no longer
/// explains it: a write batch is a handful of single-row statements, so what's
/// left is a slow server or a lock — and the lock is the one the user can act on.
pub const WRITE_WAIT_MS: u128 = 1500;

/// The note for a write that has been outstanding `waited_ms`, given the tabs of
/// ours holding a transaction on the same connection ([`write_blocking_tabs`],
/// resolved to titles). `None` while the wait is still short enough to be
/// ordinary.
///
/// Deliberately hedged ("may be holding"): Schemaic doesn't track which rows a
/// transaction has touched, so an open transaction elsewhere is a *candidate*,
/// not the diagnosis. Saying so plainly is still the difference between a hang
/// with no explanation and a hang with one thing to try — and when the holder is
/// the user's own second tab, which is the common case, it usually is the answer.
/// The sentence deliberately doesn't name the tab: the **button** does, and a
/// custom tab name is arbitrarily long — spelled into the sentence it pushed the
/// bar's own action off the edge.
pub fn write_wait_note(waited_ms: u128, holders: &[(usize, String)]) -> Option<WaitNote> {
    if waited_ms < WRITE_WAIT_MS {
        return None;
    }
    let (text, rollback) = match holders {
        // Nothing of ours is a candidate, so there's no tab to offer and the
        // sentence has to carry the whole answer.
        [] => ("Another session may be holding the lock.", None),
        [(id, title)] => (
            "A transaction may be holding the lock.",
            Some((*id, title.clone())),
        ),
        // No button (see `WaitNote::rollback`), so say that it's one of theirs —
        // which tab is a question their own status bars answer.
        _ => (
            "One of your open transactions may be holding the lock.",
            None,
        ),
    };
    Some(WaitNote {
        text: text.to_string(),
        rollback,
    })
}

/// The status-bar pill for a transaction, or `None` when there's nothing to say
/// (no transaction open).
pub fn pill_text(state: TxState) -> Option<String> {
    match state {
        TxState::Idle => None,
        // "3 Open" — the count is the useful part, and it sits next to Commit /
        // Rollback in the status bar, which already says what it's counting.
        TxState::Open { stmts } => Some(format!("{stmts} Open")),
        TxState::Poisoned { .. } => Some("Tx aborted — rollback to continue".to_string()),
        TxState::Lost => Some("Transaction lost".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MY: TxEngine = TxEngine::MySql;
    const PG: TxEngine = TxEngine::Postgres;

    fn ok(state: TxState, engine: TxEngine, sql: &str) -> TxState {
        state.on_statement(engine, sql, StmtOutcome::Ok)
    }

    // ── Counting ──────────────────────────────────────────────────────────

    #[test]
    fn begun_opens_an_empty_transaction() {
        assert_eq!(TxState::begun(), TxState::Open { stmts: 0 });
        assert!(TxState::begun().is_open());
        assert_eq!(TxState::begun().stmts(), 0);
    }

    #[test]
    fn successful_statements_count_up() {
        let s = ok(TxState::begun(), MY, "UPDATE t SET a = 1");
        assert_eq!(s, TxState::Open { stmts: 1 });
        let s = ok(s, MY, "DELETE FROM t WHERE id = 2");
        assert_eq!(s, TxState::Open { stmts: 2 });
    }

    #[test]
    fn a_statement_while_idle_opens_the_transaction() {
        // The app issues BEGIN lazily, so the first statement of a Manual tab
        // arrives with the state still Idle.
        assert_eq!(
            ok(TxState::Idle, PG, "UPDATE t SET a = 1"),
            TxState::Open { stmts: 1 }
        );
    }

    #[test]
    fn closed_resets_the_counter() {
        let s = ok(TxState::begun(), MY, "UPDATE t SET a = 1");
        assert_eq!(s.stmts(), 1);
        assert_eq!(TxState::closed(), TxState::Idle);
        assert_eq!(TxState::closed().stmts(), 0);
        assert!(!TxState::closed().is_open());
    }

    // ── Engine divergence on a failed statement ───────────────────────────

    #[test]
    fn postgres_poisons_the_transaction_on_error() {
        let s = ok(TxState::begun(), PG, "UPDATE t SET a = 1");
        let s = s.on_statement(PG, "SELECT nope", StmtOutcome::Failed);
        assert_eq!(s, TxState::Poisoned { stmts: 1 });
        assert!(s.is_open(), "still an open transaction to get rid of");
        assert!(!s.can_commit(), "COMMIT on an aborted PG tx is a ROLLBACK");
        assert!(s.can_rollback(), "rollback is the way out of 25P02");
    }

    #[test]
    fn postgres_stays_poisoned_and_stops_counting() {
        let s = TxState::Poisoned { stmts: 2 };
        assert_eq!(ok(s, PG, "SELECT 1"), TxState::Poisoned { stmts: 2 });
        assert_eq!(
            s.on_statement(PG, "SELECT 1", StmtOutcome::Failed),
            TxState::Poisoned { stmts: 2 }
        );
    }

    /// A grid write runs under `SAVEPOINT schemaic_w`, and `pg::write_on` rolls
    /// back to that savepoint on failure — which clears PostgreSQL's aborted
    /// state. Folding it as a bare failure told the user their transaction was
    /// dead and left Rollback as the only enabled action, destroying every
    /// statement they had built up.
    #[test]
    fn a_savepoint_isolated_failure_leaves_the_transaction_usable() {
        let s = TxState::Open { stmts: 20 };
        let after = s.on_statement(PG, "UPDATE t SET a = 1", StmtOutcome::FailedIsolated);
        assert_eq!(
            after,
            TxState::Open { stmts: 20 },
            "the failure didn't count"
        );
        assert!(after.can_commit(), "the savepoint already rescued it");
        // Same on MySQL, which never poisoned anyway.
        assert_eq!(
            s.on_statement(MY, "UPDATE t SET a = 1", StmtOutcome::FailedIsolated),
            TxState::Open { stmts: 20 }
        );
    }

    /// The contrast that makes the new variant meaningful: a bare failure — a
    /// plain `SELECT` error in the same tab, with no savepoint around it —
    /// really does abort the transaction and must still poison.
    #[test]
    fn a_bare_failure_still_poisons_postgres() {
        let s = TxState::Open { stmts: 20 };
        assert_eq!(
            s.on_statement(PG, "SELECT 1/0", StmtOutcome::Failed),
            TxState::Poisoned { stmts: 20 }
        );
    }

    /// An isolated failure can't resurrect a transaction that is already dead.
    #[test]
    fn a_savepoint_isolated_failure_does_not_revive_a_poisoned_transaction() {
        let s = TxState::Poisoned { stmts: 3 };
        assert_eq!(
            s.on_statement(PG, "UPDATE t SET a = 1", StmtOutcome::FailedIsolated),
            TxState::Poisoned { stmts: 3 }
        );
        assert_eq!(
            TxState::Lost.on_statement(PG, "UPDATE t SET a = 1", StmtOutcome::FailedIsolated),
            TxState::Lost
        );
    }

    #[test]
    fn mysql_survives_a_failed_statement() {
        let s = ok(TxState::begun(), MY, "UPDATE t SET a = 1");
        let s = s.on_statement(MY, "SELECT nope", StmtOutcome::Failed);
        assert_eq!(s, TxState::Open { stmts: 1 }, "failed stmt doesn't count");
        assert!(s.can_commit());
    }

    #[test]
    fn cancellation_poisons_on_postgres_only() {
        let pg = ok(TxState::begun(), PG, "UPDATE t SET a = 1");
        assert_eq!(
            pg.on_statement(PG, "SELECT pg_sleep(60)", StmtOutcome::Cancelled),
            TxState::Poisoned { stmts: 1 },
            "PG cancellation is error 57014 — it aborts the transaction"
        );
        let my = ok(TxState::begun(), MY, "UPDATE t SET a = 1");
        assert_eq!(
            my.on_statement(MY, "SELECT SLEEP(60)", StmtOutcome::Cancelled),
            TxState::Open { stmts: 1 },
            "KILL QUERY kills the statement, not the transaction"
        );
    }

    // ── Connection loss ───────────────────────────────────────────────────

    #[test]
    fn a_dropped_connection_loses_an_open_transaction() {
        let s = ok(TxState::begun(), MY, "UPDATE t SET a = 1");
        let s = s.on_statement(MY, "SELECT 1", StmtOutcome::ConnectionLost);
        assert_eq!(s, TxState::Lost);
        assert!(!s.can_commit());
        assert!(!s.can_rollback(), "there's no connection left to roll back");
    }

    #[test]
    fn a_dropped_connection_with_no_transaction_is_not_a_loss() {
        assert_eq!(
            TxState::Idle.on_statement(PG, "SELECT 1", StmtOutcome::ConnectionLost),
            TxState::Idle
        );
    }

    #[test]
    fn lost_is_terminal_until_closed() {
        let s = TxState::Lost;
        assert_eq!(ok(s, MY, "SELECT 1"), TxState::Lost);
        assert_eq!(
            s.on_statement(PG, "SELECT 1", StmtOutcome::Failed),
            TxState::Lost
        );
        assert_eq!(TxState::closed(), TxState::Idle, "close clears it");
    }

    // ── MySQL implicit commit ─────────────────────────────────────────────

    #[test]
    fn mysql_ddl_implicitly_commits() {
        for sql in [
            "CREATE TABLE t (id INT)",
            "drop table t",
            "ALTER TABLE t ADD COLUMN c INT",
            "TRUNCATE TABLE t",
            "RENAME TABLE a TO b",
            "LOCK TABLES t WRITE",
        ] {
            assert!(implicit_commit(MY, sql), "{sql} should implicitly commit");
            assert_eq!(
                ok(TxState::Open { stmts: 3 }, MY, sql),
                TxState::Idle,
                "{sql} ends the transaction"
            );
        }
    }

    #[test]
    fn postgres_ddl_is_transactional() {
        assert!(!implicit_commit(PG, "CREATE TABLE t (id INT)"));
        assert_eq!(
            ok(TxState::Open { stmts: 3 }, PG, "CREATE TABLE t (id INT)"),
            TxState::Open { stmts: 4 },
            "PG DDL is just another statement in the transaction"
        );
    }

    #[test]
    fn dml_does_not_implicitly_commit() {
        for sql in [
            "SELECT 1",
            "UPDATE t SET a = 1",
            "INSERT INTO t VALUES (1)",
            "DELETE FROM t",
            "REPLACE INTO t VALUES (1)",
        ] {
            assert!(!implicit_commit(MY, sql), "{sql} must not commit");
        }
    }

    #[test]
    fn mysqls_non_ddl_implicit_commits_are_matched_too() {
        // Every one of these is on MySQL's "statements that cause an implicit
        // commit" list and none was matched. `FLUSH` and `CHECK TABLE` are
        // ordinary things to type in a SQL editor — and after one of them, a
        // Rollback reports an undo the server never performed.
        for sql in [
            "FLUSH TABLES",
            "flush privileges",
            "CHECK TABLE t",
            "CACHE INDEX t IN c",
            "LOAD INDEX INTO CACHE t",
            "INSTALL PLUGIN p SONAME 'p.so'",
            "UNINSTALL PLUGIN p",
        ] {
            assert!(implicit_commit(MY, sql), "{sql} should implicitly commit");
            assert_eq!(
                ok(TxState::Open { stmts: 3 }, MY, sql),
                TxState::Idle,
                "{sql} ends the transaction"
            );
        }
    }

    #[test]
    fn only_the_two_set_forms_that_commit_are_matched() {
        // `SET` can't be matched wholesale — most of it is session state that
        // leaves the transaction alone, and claiming a commit that didn't happen
        // is the mirror-image lie (the pill would go quiet over open work).
        for sql in [
            "SET autocommit = 1",
            "SET AUTOCOMMIT=1",
            "SET @@autocommit = 1",
            "SET SESSION autocommit = ON",
            "SET @@session.autocommit = TRUE",
            "SET PASSWORD FOR u = 'x'",
        ] {
            assert!(implicit_commit(MY, sql), "{sql} should implicitly commit");
        }
        for sql in [
            "SET @x = 1",
            "SET @autocommit_backup = 1",
            "SET NAMES utf8mb4",
            "SET SESSION sql_mode = ''",
            "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE",
            // Turning autocommit *off* inside a transaction commits nothing…
            "SET autocommit = 0",
            // …and the global variable isn't this session's.
            "SET GLOBAL autocommit = 1",
            "SET",
        ] {
            assert!(!implicit_commit(MY, sql), "{sql} must not commit");
        }
    }

    #[test]
    fn postgres_has_transactional_ddl_so_none_of_them_commit() {
        for sql in ["FLUSH TABLES", "CHECK TABLE t", "SET autocommit = 1"] {
            assert!(!implicit_commit(PG, sql), "{sql} must not commit on PG");
        }
    }

    #[test]
    fn implicit_commit_sees_past_comments() {
        // `leading_keyword` runs on the shared boundary lexer, so a leading
        // comment doesn't hide the DDL keyword behind it.
        assert!(implicit_commit(MY, "/* migration step */ DROP TABLE t"));
        assert!(implicit_commit(MY, "-- cleanup\nTRUNCATE TABLE t"));
        // …and a keyword that only appears *inside* a comment isn't the leader.
        assert!(!implicit_commit(MY, "/* DROP */ SELECT 1"));
    }

    #[test]
    fn implicit_commit_ignores_a_word_that_merely_starts_with_a_keyword() {
        assert!(!implicit_commit(MY, "CREATED_AT_CHECK()"));
        assert!(!implicit_commit(MY, "SELECT dropped FROM t"));
    }

    #[test]
    fn empty_or_comment_only_sql_commits_nothing() {
        assert!(!implicit_commit(MY, ""));
        assert!(!implicit_commit(MY, "   "));
        assert!(!implicit_commit(MY, "-- just a note"));
    }

    // ── Pill text ─────────────────────────────────────────────────────────

    #[test]
    fn pill_is_silent_when_idle() {
        assert_eq!(pill_text(TxState::Idle), None);
    }

    #[test]
    fn pill_counts_open_statements() {
        assert_eq!(
            pill_text(TxState::Open { stmts: 0 }).as_deref(),
            Some("0 Open")
        );
        assert_eq!(
            pill_text(TxState::Open { stmts: 1 }).as_deref(),
            Some("1 Open")
        );
        assert_eq!(
            pill_text(TxState::Open { stmts: 2 }).as_deref(),
            Some("2 Open")
        );
        assert_eq!(
            pill_text(TxState::Open { stmts: 42 }).as_deref(),
            Some("42 Open")
        );
    }

    #[test]
    fn pill_names_the_abnormal_states() {
        assert_eq!(
            pill_text(TxState::Poisoned { stmts: 3 }).as_deref(),
            Some("Tx aborted — rollback to continue")
        );
        assert_eq!(
            pill_text(TxState::Lost).as_deref(),
            Some("Transaction lost")
        );
    }

    // ── Mode ──────────────────────────────────────────────────────────────

    #[test]
    fn mode_defaults_to_auto() {
        assert_eq!(TxMode::default(), TxMode::Auto);
        assert!(!TxMode::default().is_manual());
        assert_eq!(TxMode::Auto.label(), "Auto-commit");
        assert_eq!(TxMode::Manual.label(), "Manual");
    }

    // ── The pinned session's "is a transaction still open?" flag ──────────

    #[test]
    fn an_ordinary_statement_leaves_the_flag_alone() {
        // The overwhelmingly common case: nothing implicitly committed, so the
        // session must not touch what it believes about the connection.
        for sql in [
            "UPDATE t SET a = 1 WHERE id = 2",
            "SELECT * FROM t",
            "INSERT INTO t VALUES (1)",
            "DELETE FROM t WHERE id = 1",
            "SET NAMES utf8mb4",
            "SET @x = 1",
        ] {
            assert_eq!(tx_open_after(TxEngine::MySql, sql), None, "{sql}");
        }
    }

    #[test]
    fn mysql_ddl_implicitly_commits_and_leaves_nothing_open() {
        for sql in [
            "CREATE TABLE t (a INT)",
            "ALTER TABLE t ADD b INT",
            "DROP TABLE t",
            "TRUNCATE TABLE t",
            "FLUSH TABLES",
            "SET autocommit = 1",
        ] {
            assert_eq!(tx_open_after(TxEngine::MySql, sql), Some(false), "{sql}");
        }
    }

    /// The carve-out this function exists for, and the one with teeth.
    ///
    /// `BEGIN`/`START TRANSACTION` are on the implicit-commit list because
    /// opening a transaction commits the current one — but they leave a *new*
    /// one open. Reporting `Some(false)` for them would clear the session's
    /// flag, the next statement would decide it needed its own `BEGIN`, and on
    /// MySQL that second `BEGIN` would implicitly commit the work in between:
    /// exactly [B12.1-L1-01], reintroduced from the other side.
    #[test]
    fn opening_a_transaction_commits_the_old_one_but_leaves_a_new_one_open() {
        for sql in [
            "BEGIN",
            "begin",
            "START TRANSACTION",
            "start transaction read write",
            "/* comment first */ BEGIN",
            "-- leading line comment\nSTART TRANSACTION",
        ] {
            assert_eq!(tx_open_after(TxEngine::MySql, sql), Some(true), "{sql}");
        }
    }

    #[test]
    fn postgres_never_implicitly_commits_so_the_flag_never_moves() {
        // Transactional DDL: the transaction survives all of these, including
        // the ones that would end it on MySQL.
        for sql in [
            "CREATE TABLE t (a INT)",
            "DROP TABLE t",
            "TRUNCATE t",
            "UPDATE t SET a = 1",
        ] {
            assert_eq!(tx_open_after(TxEngine::Postgres, sql), None, "{sql}");
        }
    }

    #[test]
    fn the_flag_rule_agrees_with_implicit_commit_by_construction() {
        // `tx_open_after` says "unchanged" exactly when `implicit_commit` says
        // no. The two are read together by the session and the pill; a
        // disagreement is the drift the shared predicate exists to prevent.
        for sql in [
            "CREATE TABLE t (a INT)",
            "UPDATE t SET a = 1",
            "BEGIN",
            "SET NAMES utf8mb4",
            "SET autocommit = 0",
            "FLUSH TABLES",
        ] {
            for engine in [TxEngine::MySql, TxEngine::Postgres] {
                assert_eq!(
                    tx_open_after(engine, sql).is_some(),
                    implicit_commit(engine, sql),
                    "{engine:?} {sql}"
                );
            }
        }
    }

    // ── Whether a session that finished connecting still has an owner ─────

    #[test]
    fn a_session_is_kept_only_by_a_tab_that_is_still_manual() {
        assert!(session_still_wanted(Some(TxMode::Manual)));
    }

    #[test]
    fn a_tab_that_flipped_back_to_auto_does_not_want_it() {
        assert!(!session_still_wanted(Some(TxMode::Auto)));
    }

    #[test]
    fn a_tab_that_closed_while_connecting_does_not_want_it() {
        // The case that leaks: `None` is "the tab isn't in the list any more",
        // and keying the map by its id would pin a connection nothing can ever
        // remove — `drop_session` already ran, before the entry existed.
        assert!(!session_still_wanted(None));
    }

    // ── Which transactions a schema change has to wait behind ─────────────

    fn tab(tab_id: usize, conn_id: u64, state: TxState) -> TabTx {
        TabTx {
            tab_id,
            conn_id,
            state,
        }
    }

    #[test]
    fn a_tab_with_no_transaction_blocks_nothing() {
        let tabs = [
            tab(1, 7, TxState::Idle),
            // The connection died with the transaction open, so the server has
            // already released everything it held.
            tab(2, 7, TxState::Lost),
        ];
        assert!(ddl_blocking_tabs(&tabs, 7).is_empty());
    }

    #[test]
    fn an_open_transaction_on_this_connection_blocks() {
        let tabs = [tab(3, 7, TxState::Open { stmts: 1 })];
        assert_eq!(ddl_blocking_tabs(&tabs, 7), vec![3]);
    }

    #[test]
    fn a_poisoned_transaction_blocks_too() {
        // PostgreSQL rejects statements in it, but the locks it took are held
        // until `ROLLBACK` — which is exactly what the prompt offers.
        let tabs = [tab(3, 7, TxState::Poisoned { stmts: 2 })];
        assert_eq!(ddl_blocking_tabs(&tabs, 7), vec![3]);
    }

    #[test]
    fn another_connections_transaction_is_not_ours_to_ask_about() {
        let tabs = [tab(4, 8, TxState::Open { stmts: 1 })];
        assert!(ddl_blocking_tabs(&tabs, 7).is_empty());
    }

    #[test]
    fn every_open_transaction_on_the_connection_is_reported_in_tab_order() {
        // One prompt per tab, chained: `tx_prompt` holds one question at a time,
        // and settling the first tab doesn't settle the second.
        let tabs = [
            tab(1, 7, TxState::Open { stmts: 1 }),
            tab(2, 8, TxState::Open { stmts: 1 }),
            tab(3, 7, TxState::Idle),
            tab(4, 7, TxState::Poisoned { stmts: 3 }),
        ];
        assert_eq!(ddl_blocking_tabs(&tabs, 7), vec![1, 4]);
    }

    // ── Which transactions a *grid write* could be waiting behind ─────────

    #[test]
    fn a_writers_own_transaction_never_blocks_its_own_write() {
        // The one case that parts from the DDL rule: the write runs on tab 3's
        // own pinned session, inside that transaction. Naming it would send the
        // user to roll back the very transaction they're writing into.
        let tabs = [tab(3, 7, TxState::Open { stmts: 1 })];
        assert!(write_blocking_tabs(&tabs, 7, 3).is_empty());
        // …and the DDL path, which runs on a fresh connection, still queues
        // behind it.
        assert_eq!(ddl_blocking_tabs(&tabs, 7), vec![3]);
    }

    #[test]
    fn another_tabs_transaction_on_the_connection_is_a_candidate() {
        let tabs = [
            tab(1, 7, TxState::Open { stmts: 2 }),
            tab(2, 7, TxState::Idle),
            tab(3, 8, TxState::Open { stmts: 1 }), // another connection
            tab(4, 7, TxState::Lost),              // released on disconnect
            tab(5, 7, TxState::Poisoned { stmts: 1 }),
        ];
        assert_eq!(write_blocking_tabs(&tabs, 7, 2), vec![1, 5]);
    }

    // ── What the user is told while a write waits ─────────────────────────

    fn holder(id: usize, title: &str) -> (usize, String) {
        (id, title.to_string())
    }

    #[test]
    fn a_short_wait_says_nothing() {
        // Every commit crosses the network; narrating that is noise.
        assert_eq!(write_wait_note(WRITE_WAIT_MS - 1, &[]), None);
        assert_eq!(write_wait_note(0, &[holder(3, "Query 3")]), None);
    }

    #[test]
    fn a_wait_with_no_transaction_of_ours_still_names_the_likely_cause() {
        // Nothing to offer, but "a lock" is the difference between a hang the
        // user can reason about and one they can't.
        let n = write_wait_note(WRITE_WAIT_MS, &[]).expect("note past the threshold");
        assert_eq!(n.text, "Another session may be holding the lock.");
        assert_eq!(n.rollback, None);
    }

    #[test]
    fn one_open_transaction_is_offered_for_rollback() {
        // The tab is named by the button, never by the sentence — a custom tab
        // name is arbitrarily long.
        let n = write_wait_note(WRITE_WAIT_MS, &[holder(3, "orders")]).expect("note");
        assert_eq!(n.text, "A transaction may be holding the lock.");
        assert_eq!(n.rollback, Some((3, "orders".to_string())));
    }

    #[test]
    fn several_open_transactions_leave_the_choice_to_the_user() {
        // A single button would have to choose one, and choosing wrong throws
        // away a transaction the user never meant to end.
        let n = write_wait_note(
            WRITE_WAIT_MS * 4,
            &[holder(3, "Query 3"), holder(5, "customers")],
        )
        .expect("note");
        assert_eq!(
            n.text,
            "One of your open transactions may be holding the lock."
        );
        assert_eq!(n.rollback, None);
    }
}
