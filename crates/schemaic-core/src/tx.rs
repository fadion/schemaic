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
    /// The user cancelled it, but it ran inside its own `SAVEPOINT` and that
    /// savepoint has since been rolled back — so the enclosing transaction is
    /// untouched and still committable.
    ///
    /// [`StmtOutcome::FailedIsolated`]'s twin, and it exists for the same reason
    /// with a sharper edge: on PostgreSQL a cancellation aborts the transaction
    /// exactly as an error does, so a **read** the user dismissed — a blob panel
    /// closed with Escape while its `SELECT` was still running — discarded every
    /// uncommitted statement of a Manual-mode tab, and the pill was not wrong
    /// about it. A read has nothing of its own to lose by being rolled back,
    /// which is what makes fencing one and reporting this both safe and true.
    ///
    /// Only ever reported where the rollback was **issued and accepted** — see
    /// `Session::classify_fenced`. A cancellation with no fence around it is
    /// still [`StmtOutcome::Cancelled`], because nothing then guarantees the
    /// transaction survived.
    CancelledIsolated,
    /// It failed or was cancelled, **and the transaction it was inside is gone**:
    /// MySQL performs its implicit commit *before* a DDL statement runs, so an
    /// `ALTER` the server rejected — or one a statement timeout killed halfway
    /// through — has committed the open transaction just as surely as a
    /// successful one would.
    ///
    /// Distinct from [`StmtOutcome::Failed`] because **the text of the statement
    /// cannot decide it**, and [`implicit_commit`] is therefore not enough on its
    /// own. Verified against MariaDB 10.11.14: a parsed-but-rejected
    /// `DROP TABLE nosuch` (`ERROR 1051`) leaves `@@in_transaction` at `0` — the
    /// transaction is committed and a later `ROLLBACK` undoes nothing — while a
    /// *syntax* error on the same keyword (`ALTER TABLE t GARBAGE`,
    /// `ERROR 1064`) leaves it at `1`, with the transaction untouched. Only the
    /// server can tell those two apart, which is why this variant exists rather
    /// than a wider reading of `implicit_commit`: see [`failure_committed`].
    FailedAndCommitted,
    /// It **succeeded**, and the transaction it was inside is gone — as reported
    /// by the connection rather than read off the statement.
    ///
    /// [`FailedAndCommitted`](Self::FailedAndCommitted)'s twin for the success
    /// path, and it exists for the same reason: the text cannot decide it. The
    /// one statement that reaches it is `SET autocommit`, whose effect on the
    /// current transaction depends on the value the variable *had* —
    /// [`TxAfter::Ask`] is what routes it to the probe, and this is how the
    /// probe's answer reaches the pill, which cannot ask.
    OkAndClosed,
    /// **Nothing happened to the transaction, whatever happened to the
    /// statement** — because there was no transaction for it to happen to.
    ///
    /// The answer for the two session entry points that never call
    /// `ensure_tx`: `fetch_blob` and `refetch_rows`. Every other fold arm rests
    /// on the premise that "the app issues `BEGIN` lazily, so the first
    /// statement lands as the first statement of a fresh transaction" — true of
    /// `fetch_query`, which `ensure_tx` always precedes, and false of a read on
    /// the pinned connection, which opens nothing. Without this variant those
    /// reads *invented* a transaction:
    ///
    /// * a cancelled read between transactions folded `Idle` to
    ///   `Poisoned { stmts: 0 }` on PostgreSQL — a tab with no transaction
    ///   reading "Tx aborted", `Poisoned` sticky for every later outcome
    ///   including `Ok`, Commit **hidden** by `can_commit()`, and every exit the
    ///   UI then offered rolling back the real work that followed. MySQL hid it:
    ///   `SAVEPOINT` outside a transaction is accepted there, so the read was
    ///   fenced and the fold was harmless.
    /// * a read cancelled *before dispatch* folded `Idle` to `Open { stmts: 0 }`
    ///   on both engines, so the pill read "0 Open", `guard_tx` prompted about a
    ///   transaction on every tab close, mode switch and database switch, and
    ///   `ddl_blocking_tabs` told a designer Apply to queue behind it.
    ///
    /// See [`read_outcome`], which is where a read path asks for this.
    Untouched,
    /// The connection itself died — idle-in-transaction timeout, server
    /// restart, network drop. Whatever was in the transaction is gone.
    ConnectionLost,
    /// It **never reached the server**: the run's token was already cancelled
    /// when the statement's turn came, so nothing was dispatched.
    ///
    /// The reachable case is the tab's own connection being busy — a
    /// `commit_writes` or the re-fetch after one holds the session's lock, and
    /// the run behind it is cancelled (by the user, or by the statement timeout)
    /// while still waiting for it. Reported apart from [`StmtOutcome::Cancelled`]
    /// because that variant means *the server was asked and the answer was
    /// killed*, and the two have opposite consequences on PostgreSQL: a real
    /// cancellation aborts the transaction, while a statement that was never sent
    /// leaves it exactly where it was. Folding the second as the first is what put
    /// **Tx aborted — rollback to continue** over a healthy transaction and told
    /// the user their only way on was to discard work they had just saved.
    NotSent,
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
            // **PostgreSQL rejects everything until `ROLLBACK` — and that is the
            // statement the pill tells the user to run.** This arm used to
            // absorb it too, so a typed rollback left the tab reading
            // "Tx aborted" for the rest of its life while `ensure_tx` opened a
            // real, healthy transaction underneath: Commit hidden in the footer
            // and in the close prompt, and the only offered actions destroying
            // work that was never in trouble. `TxState::closed()`'s doc already
            // called itself the way out of `Poisoned`; the typed rollback had no
            // route to it.
            //
            // Gated on `Ok`: a rollback the server refused changes nothing, and
            // a statement it rejected inside an aborted transaction is exactly
            // what the absorbing behaviour is for.
            TxState::Poisoned { .. }
                if outcome == StmtOutcome::Ok
                    && matches!(tx_after(engine, sql), TxAfter::Closed | TxAfter::Open) =>
            {
                match tx_after(engine, sql) {
                    TxAfter::Open => TxState::Open { stmts: 0 },
                    _ => TxState::Idle,
                }
            }
            // **PostgreSQL's own documented way out, which is not a close.**
            // `ROLLBACK TO SAVEPOINT s` is accepted *inside* an aborted
            // transaction and un-aborts it, leaving the transaction live and
            // committable — measured on PostgreSQL 16.15:
            // `SAVEPOINT s1; SELECT 1/0; ROLLBACK TO SAVEPOINT s1; SELECT 1;`
            // and the `COMMIT` after it both succeed. `tx_after` answers
            // `Unchanged` for it, correctly, because the question *that*
            // predicate was written for is `Session::in_tx` — and the arm above
            // enumerates `Closed | Open`, so the statement fell to this one and
            // the tab stayed `Poisoned` for the rest of its life: `can_commit()`
            // false, Commit hidden in the footer and in the close prompt, and
            // the only offered action discarding a healthy transaction's work.
            //
            // **Only the typed statement, not `FailedIsolated`.** The review
            // that found this proposed letting that outcome out of `Poisoned`
            // too, on the grounds that `Session::classify_isolated` mints it
            // only after a `ROLLBACK TO SAVEPOINT schemaic_w` the server
            // accepted. The premise is right and the input cannot occur:
            // reaching that rollback needs a `SAVEPOINT` first, and PostgreSQL
            // refuses one inside an aborted transaction. So the combination is
            // unreachable, `a_savepoint_isolated_failure_does_not_revive_a_poisoned_transaction`
            // already decided it the conservative way, and this does not
            // reverse that decision for a case nothing can produce.
            TxState::Poisoned { stmts }
                if outcome == StmtOutcome::Ok && clears_abort(engine, sql) =>
            {
                TxState::Open { stmts }
            }
            TxState::Poisoned { stmts } => TxState::Poisoned { stmts },
            TxState::Idle | TxState::Open { .. } => {
                let stmts = self.stmts();
                match outcome {
                    StmtOutcome::Ok => match tx_after(engine, sql) {
                        // MySQL DDL committed the transaction out from under
                        // us, or the user typed `COMMIT` — back to no
                        // transaction, not to `stmts + 1`.
                        TxAfter::Closed => TxState::Idle,
                        // **A new transaction, so the count starts over rather
                        // than the pill going blank.** This arm read
                        // `implicit_commit` alone, which is `true` for `BEGIN`,
                        // so a typed `BEGIN` folded to `Idle` while the session
                        // and the server both kept a transaction open — and
                        // `ddl_blocking_tabs`, which asks `is_open()`, then
                        // reported nothing and let a schema Apply queue behind
                        // the tab's own metadata lock until the lock-wait
                        // timeout.
                        TxAfter::Open => TxState::Open { stmts: 0 },
                        // The pill cannot ask the server. It keeps counting
                        // until the session tells it, which it does by
                        // upgrading the outcome to `OkAndClosed`.
                        TxAfter::Unchanged | TxAfter::Ask => TxState::Open { stmts: stmts + 1 },
                    },
                    // The session probed and the server says the transaction is
                    // gone — see [`StmtOutcome::OkAndClosed`].
                    StmtOutcome::OkAndClosed => TxState::Idle,
                    // There was no transaction, so nothing happened to one.
                    StmtOutcome::Untouched => self,
                    // A statement that didn't apply doesn't count. On Postgres it
                    // also poisons: both a server error and a cancellation leave
                    // the transaction in the aborted state.
                    StmtOutcome::Failed | StmtOutcome::Cancelled => match engine {
                        TxEngine::Postgres => TxState::Poisoned { stmts },
                        TxEngine::MySql => TxState::Open { stmts },
                    },
                    // The same, except the server has *said* the transaction is
                    // gone — see [`StmtOutcome::FailedAndCommitted`]. It is the
                    // one failure that ends a transaction rather than leaving it
                    // where it was, and it ends it on either engine, because the
                    // answer came from the connection and not from the engine.
                    StmtOutcome::FailedAndCommitted => TxState::Idle,
                    // Its savepoint already absorbed the abort, so the enclosing
                    // transaction is untouched on either engine — it just gains
                    // no statement.
                    StmtOutcome::FailedIsolated | StmtOutcome::CancelledIsolated => {
                        TxState::Open { stmts }
                    }
                    // Nothing was sent, so nothing happened to the transaction on
                    // either engine — and there *is* one, because the session
                    // opens it before the statement is attempted. `Open` rather
                    // than `self` for that reason: the first statement of a
                    // Manual tab arrives while this reads `Idle`, and its `BEGIN`
                    // has already gone out.
                    StmtOutcome::NotSent => TxState::Open { stmts },
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
    // **MySQL's own documented exception**: *"CREATE TABLE and DROP TABLE
    // statements do not commit a transaction if the TEMPORARY keyword is
    // used."* Without it the pill went blank over a live transaction holding
    // the user's uncommitted work — the false positive the doc above calls the
    // mirror image, and the one MySQL habit that produces it. Read through the
    // same lexer, so a comment between the two words cannot hide it.
    if matches!(kw.as_str(), "CREATE" | "DROP")
        && crate::sql::leading_words(sql, 2, dialect)
            .get(1)
            .map(String::as_str)
            == Some("TEMPORARY")
    {
        return false;
    }
    // **MySQL's second carve-out**: *"RESET (but not RESET PERSIST)"*. Without
    // it the pill would go blank over a live transaction, which this function's
    // doc prices as the mirror failure.
    if kw == "RESET"
        && crate::sql::leading_words(sql, 2, dialect)
            .get(1)
            .map(String::as_str)
            == Some("PERSIST")
    {
        return false;
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
            // Opening a new transaction commits the current one. `START` is
            // here for both readings: `START TRANSACTION` opens one, and
            // `START REPLICA`/`SLAVE`/`GROUP_REPLICATION` open nothing but are
            // in MySQL's replication-control group, which commits.
            | "BEGIN"
            | "START"
            // The rest of that group, which matched nothing at all — so the
            // pill kept counting a transaction the server had already
            // committed. `STOP REPLICA`/`SLAVE`, `RESET REPLICA`/`SLAVE`/
            // `MASTER` and `CHANGE REPLICATION SOURCE TO`/`CHANGE MASTER TO`;
            // no other statement in the language leads with `CHANGE`, and
            // `RESET PERSIST` is excluded above.
            | "STOP"
            | "RESET"
            | "CHANGE"
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
    match tx_after(engine, sql) {
        TxAfter::Unchanged | TxAfter::Ask => None,
        TxAfter::Closed => Some(false),
        TxAfter::Open => Some(true),
    }
}

/// The outcome a read on the pinned connection may report, given whether the
/// session actually has a transaction open.
///
/// **The seam the fold's own premise does not cover.**
/// [`TxState::on_statement`] merges `Idle` with `Open` because "the app issues
/// `BEGIN` lazily, so the first statement lands as the first statement of a
/// fresh transaction" — true of `fetch_query`, which `ensure_tx` always
/// precedes, and false of `Session::fetch_blob` and `Session::refetch_rows`,
/// which never call it. Clicking a `bytea` cell in the ordinary state *between*
/// transactions — right after Commit — and then dismissing the panel therefore
/// left a PostgreSQL tab reading "Tx aborted" with Commit hidden and every exit
/// the UI offered rolling back the real work that came next.
///
/// `in_tx` is the session's own flag, read under the lock the `BEGIN` goes out
/// on, so this is the connection's answer rather than a guess about it.
/// [`StmtOutcome::ConnectionLost`] passes through: a dead connection is a fact
/// about the connection, not about a transaction, and the tab has to hear it.
pub fn read_outcome(in_tx: bool, stmt: StmtOutcome) -> StmtOutcome {
    match stmt {
        _ if in_tx => stmt,
        StmtOutcome::ConnectionLost => stmt,
        _ => StmtOutcome::Untouched,
    }
}

/// What running `sql` **successfully** leaves the connection's transaction in.
///
/// The whole of the question `tx_open_after`'s `Option<bool>` could not put:
/// there is a fourth answer, and leaving it out is how a `SET autocommit = 1`
/// that committed nothing came to clear the session's flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxAfter {
    /// Nothing about whether a transaction is open changed — the ordinary case.
    Unchanged,
    /// No transaction is open now.
    Closed,
    /// A transaction is open now, and it is a **new** one. Both the statements
    /// that open one (`BEGIN`, `START TRANSACTION`) and the ones that close one
    /// and open another in the same breath (`COMMIT AND CHAIN`).
    Open,
    /// **The statement's text cannot decide it.** Ask the connection —
    /// `Session::tx_alive`, the probe a failed DDL already uses. The only
    /// statement here is `SET autocommit`, whose effect depends on the value the
    /// variable *had*, which no text carries.
    Ask,
}

/// [`TxAfter`] for `sql` — the one predicate the session's flag and the pill's
/// [`TxState`] both read, so the two cannot drift.
///
/// Three groups, and the first is the one that was missing entirely:
///
/// 1. **The statements that close a transaction on purpose** — `COMMIT`,
///    `ROLLBACK`, `END`, `ABORT` — on **either** engine, because the flag is
///    engine-independent. `implicit_commit` names the openers and every MySQL
///    statement that commits as a *side effect*, and none of these; so a typed
///    `COMMIT` in a Manual tab left `in_tx` true, `ensure_tx` issued no `BEGIN`
///    for the next statement, and that statement was permanent the instant it
///    landed while the pill counted and Rollback reported an undo that never
///    happened. Measured on MariaDB 10.11.14 and PostgreSQL 16.15.
///    A `ROLLBACK TO [SAVEPOINT] s` is **not** one of these — it discards work
///    inside the transaction and leaves it open — and neither is
///    `RELEASE SAVEPOINT`.
/// 2. **The statements that leave a new transaction open** — `BEGIN`, `START`,
///    and `COMMIT`/`ROLLBACK … AND CHAIN`. This is `tx_open_after`'s original
///    carve-out: treating them as plain closers would clear the flag, the next
///    statement would decide it needed its own `BEGIN`, and on MySQL that second
///    `BEGIN` would implicitly commit everything in between.
/// 3. **Everything [`implicit_commit`] names** — MySQL's non-transactional DDL
///    and `SET PASSWORD`.
pub fn tx_after(engine: TxEngine, sql: &str) -> TxAfter {
    // The dialect only decides how the *lexer* reads the head keyword, and both
    // engines spell these the same, so MySQL's rules answer for both — the
    // superset (backslash escapes, `#` comments) can only end a token earlier,
    // never later.
    let dialect = crate::intel::SqlDialect::MySql;
    let Some(kw) = crate::sql::leading_keyword(sql, dialect) else {
        return TxAfter::Unchanged;
    };
    // **Through the shared lexer, not a split on the raw tail.** The leading
    // keyword is read comment-aware and the words after it were not, so
    // `ROLLBACK/* x */TO SAVEPOINT s` produced the token `/*`. `leading_words`
    // is bounded, which is what keeps this cheap on a sixteen-megabyte `INSERT`.
    let words = crate::sql::leading_words(sql, 4, dialect);
    let word = |n: usize| words.get(n + 1).map(String::as_str);
    match kw.as_str() {
        // `AND CHAIN` starts the next transaction immediately; `TO [SAVEPOINT]`
        // is not a close at all.
        "COMMIT" | "ROLLBACK" | "END" | "ABORT" => {
            // **`WORK`/`TRANSACTION` are noise words, and they sit between the
            // keyword and the word that decides.** Reading only the first word
            // after the keyword found `WORK`, missed both guards below, and
            // answered `Closed` for `ROLLBACK WORK TO SAVEPOINT s` — legal on
            // both engines. `Session::in_tx` then went false over a transaction
            // the server still held, and the next statement's `ensure_tx`
            // issued a `BEGIN`, which on MySQL commits the work the rollback
            // was meant to keep.
            let i = usize::from(matches!(word(0), Some("WORK" | "TRANSACTION")));
            match word(i) {
                Some("TO") => TxAfter::Unchanged,
                Some("AND") if word(i + 1) == Some("CHAIN") => TxAfter::Open,
                // `AND NO CHAIN` is the default.
                _ => TxAfter::Closed,
            }
        }
        // The one statement whose effect the text cannot carry.
        "SET" if engine == TxEngine::MySql && set_touches_autocommit(sql, dialect) => TxAfter::Ask,
        // **Both engines**, and not through `implicit_commit`, which is
        // MySQL-only: on PostgreSQL a `BEGIN` opens a transaction just as
        // surely, and one issued inside an open transaction warns and leaves it
        // open. Either way there is a transaction afterwards, which is the whole
        // of what this answers.
        "BEGIN" => TxAfter::Open,
        // **`START` alone is not an opener.** `START TRANSACTION` is; MySQL's
        // other three — `START REPLICA`, `START SLAVE`,
        // `START GROUP_REPLICATION` — open nothing, and matching the bare
        // keyword set the session's flag over no transaction. `ensure_tx` then
        // issued no `BEGIN` for the next statement, so it ran auto-committed
        // and permanent while the pill counted it and Rollback reported an undo
        // that never happened — the failure group 1 of this doc exists for,
        // reached from the other side. They fall through to `implicit_commit`,
        // which is where they belong: MySQL lists the replication-control
        // statements among those that commit.
        "START" if word(0) == Some("TRANSACTION") => TxAfter::Open,
        _ if implicit_commit(engine, sql) => TxAfter::Closed,
        _ => TxAfter::Unchanged,
    }
}

/// Does `sql` take an **aborted** transaction back to a working one?
///
/// **The third question about a statement, and the one nothing was asking.**
/// [`tx_after`] answers "is there a transaction afterwards" — the session's
/// `in_tx` flag — and [`implicit_commit`] answers "did this end one". Neither is
/// "is it still aborted", and PostgreSQL has exactly one statement where the
/// three come apart: `ROLLBACK [WORK|TRANSACTION] TO [SAVEPOINT] s` is accepted
/// *inside* an aborted transaction, clears the aborted state, and leaves the
/// transaction open and committable. `tx_after` says `Unchanged` for it, which
/// is right for the flag and left `TxState::Poisoned` with no exit: Commit was
/// hidden for the rest of the tab's life while the user went on building work in
/// a transaction that was fine.
///
/// Measured on PostgreSQL 16.15: `SAVEPOINT s1; SELECT 1/0;
/// ROLLBACK TO SAVEPOINT s1; SELECT 1;` — the `SELECT` and the `COMMIT` after it
/// both succeed.
///
/// **PostgreSQL only.** MySQL has no aborted-transaction state to clear: a
/// failed statement there leaves the transaction usable, so the fold never
/// reaches `Poisoned` on that engine at all.
pub fn clears_abort(engine: TxEngine, sql: &str) -> bool {
    if engine != TxEngine::Postgres {
        return false;
    }
    let dialect = crate::intel::SqlDialect::MySql;
    let Some(kw) = crate::sql::leading_keyword(sql, dialect) else {
        return false;
    };
    if kw != "ROLLBACK" {
        return false;
    }
    // The same noise-word walk `tx_after`'s closer arm does, and for the same
    // reason: `WORK`/`TRANSACTION` sit between the keyword and the word that
    // decides, and reading only the first word after `ROLLBACK` finds `WORK`.
    let words = crate::sql::leading_words(sql, 4, dialect);
    let word = |n: usize| words.get(n + 1).map(String::as_str);
    let i = usize::from(matches!(word(0), Some("WORK" | "TRANSACTION")));
    word(i) == Some("TO")
}

/// Did a statement that **did not apply** nonetheless end the transaction it was
/// running in?
///
/// The question only has a `true` answer on the engine with no transactional DDL,
/// and only for the statements [`implicit_commit`] names — but on that engine the
/// statement's text is not sufficient, because the implicit commit happens after
/// the parser and before the executor. `DROP TABLE nosuch` is rejected *by the
/// executor* and has committed; `ALTER TABLE t GARBAGE` is rejected by the parser
/// and has not. Both arrive here as [`StmtOutcome::Failed`] over identical
/// leading keywords.
///
/// So the deciding input is the server's own answer, passed in as `tx_alive`:
///
/// * `Some(false)` — asked, and the transaction is gone. With the statement's
///   text agreeing that this is a statement that ends one, that is a confirmed
///   implicit commit: [`StmtOutcome::FailedAndCommitted`].
/// * `Some(true)` — asked, and the transaction is still there. The statement
///   never got far enough to commit anything, so the failure is an ordinary one.
/// * `None` — **not asked, or the server could not answer.** The fallback is the
///   conservative one: report an ordinary failure, leaving the pill counting. It
///   is the behaviour that was there before the probe existed, and its cost is
///   the one [`implicit_commit`]'s doc names — a later **Rollback** reporting an
///   undo that never happened. Widening it to a guess here would trade that for
///   the mirror failure, a pill that says *Idle* over an open transaction whose
///   next statement's `BEGIN` would commit it without being asked.
///
/// `sql` is consulted first, so the caller only pays for the round trip on the
/// statements that could possibly have committed.
pub fn failure_committed(engine: TxEngine, sql: &str, tx_alive: Option<bool>) -> bool {
    implicit_commit(engine, sql) && tx_alive == Some(false)
}

/// The message a failed statement carries, with what the error itself cannot say.
///
/// A server error names what it refused. It does not mention that refusing it
/// cost the user their open transaction — and on the one engine where that
/// happens the loss is invisible: the statements folded into the transaction are
/// already permanent, **Rollback** will succeed and undo nothing, and the pill
/// going quiet is the only thing on screen that moved.
///
/// So the disclosure is attached to the message, at the one seam that knows both
/// halves. Only [`StmtOutcome::FailedAndCommitted`] gets it, which is only ever
/// set when the server was asked and said the transaction was gone — see
/// [`failure_committed`]. Every other outcome's message is returned untouched.
pub fn failed_message(message: &str, stmt: StmtOutcome) -> String {
    if stmt != StmtOutcome::FailedAndCommitted {
        return message.to_string();
    }
    format!(
        "{message}\n\nThe transaction this ran in is gone: MySQL commits before it \
         runs a DDL statement, so the statements already in it are permanent and \
         Rollback will not undo them."
    )
}

/// What a **cancelled** run has to say beyond "Cancelled", or `None` when the
/// bare word is the whole truth.
///
/// **A Stop is not a smaller timeout.** MySQL commits the open transaction
/// before it runs a DDL statement, so pressing Stop on a slow
/// `ALTER TABLE` inside a Manual transaction makes everything already in that
/// transaction permanent — `Session::fetch_query` detects it and sets
/// [`StmtOutcome::FailedAndCommitted`] for a cancel exactly as it does for a
/// failure. The pill goes quiet, Rollback will succeed and undo nothing, and
/// that was the only thing on screen that moved.
///
/// The run paths had two cancel arms: one for the statement timeout, which
/// appended [`failed_message`]'s disclosure in full, and a bare one below it
/// that returned `QueryState::Cancelled` — the single arm that never called
/// `failed_message`. `timeout_reached` is `timed_out && …`, and a user's Stop
/// leaves `timed_out` false, so the identical server state disclosed the loss
/// when the clock ran out and said nothing when the user clicked.
///
/// Here rather than at the two call sites, so a third run path cannot arrive
/// without it — the same argument [`failed_message`] itself makes for being one
/// function.
pub fn cancelled_message(stmt: Option<StmtOutcome>) -> Option<String> {
    (stmt == Some(StmtOutcome::FailedAndCommitted)).then(|| {
        failed_message(
            "The statement was cancelled.",
            StmtOutcome::FailedAndCommitted,
        )
    })
}

/// Did the statement timeout stop the **statement**, or something before it?
///
/// The watchdog is armed around the whole run — connecting, opening the
/// transaction, the statement, and pulling the rows back — deliberately, because
/// a connection that never completes is as much a hang as a query that never
/// returns. That makes its flag the right answer to "did the clock run out" and
/// the wrong answer to "what ran too long": a run cancelled while it was still
/// queued behind the tab's own connection reported *the statement ran longer than
/// the 1 minute statement timeout* for a statement that was never sent.
///
/// [`StmtOutcome::NotSent`] is the one outcome that knows the difference, so the
/// timeout's own message is reserved for the runs it can honestly describe.
pub fn timeout_reached(stmt: Option<StmtOutcome>, timed_out: bool) -> bool {
    timed_out && stmt != Some(StmtOutcome::NotSent)
}

/// What to say about a run whose statement never left the client.
///
/// It names the clock only when the clock is why (`timed_out`), and it says the
/// two things the user cannot see from anywhere else: nothing ran, and the
/// transaction is untouched. The second is the load-bearing half — the alternative
/// this replaces put **Tx aborted** on the pill and pointed at Rollback.
pub fn not_sent_message(timed_out: bool) -> String {
    let why = if timed_out {
        "The statement timeout fired while this run was still waiting for the tab's connection."
    } else {
        "This run was cancelled while it was still waiting for the tab's connection."
    };
    format!(
        "{why} An earlier operation on it — a commit, or the re-fetch after one — \
         had not finished, so the statement was never sent to the server. Nothing \
         ran, and the transaction is exactly where it was."
    )
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

    matches!(words.next().as_deref(), Some("PASSWORD"))
}

/// Does this `SET` name the session's `autocommit`?
///
/// **Split off [`set_commits`], which used to answer `true` for
/// `SET autocommit = 1|ON|TRUE`.** That reading was wrong on every connection
/// Schemaic opens: the app never sets `autocommit`, it issues an explicit
/// `BEGIN`, so the variable is the server default of **1** — and MySQL commits
/// on `SET autocommit = 1` only when the value *was* 0. So the statement
/// committed nothing, the transaction stayed open, and clearing `in_tx` on the
/// strength of it made the next statement's `BEGIN` implicitly commit the user's
/// uncommitted work. Measured on MariaDB 10.11.14.
///
/// Both values are named, and `= 0` is here for the same reason `= 1` is: what
/// the statement does to the *current* transaction depends on the value the
/// variable had, which is a fact about the connection. [`TxAfter::Ask`] is the
/// honest answer to all of them.
///
/// `GLOBAL`/`PERSIST` are excluded rather than skipped — they set a variable
/// this session's transaction does not read.
fn set_touches_autocommit(sql: &str, dialect: crate::intel::SqlDialect) -> bool {
    let after = crate::sql::leading_keyword_end(sql, dialect).map_or("", |e| &sql[e..]);
    let mut words = after
        .split(|c: char| c.is_whitespace() || matches!(c, '=' | '.' | ',' | ';' | ':'))
        .filter(|w| !w.is_empty())
        .map(|w| w.trim_start_matches('@').to_ascii_uppercase());
    let Some(first) = words.next() else {
        return false;
    };
    let name = match first.as_str() {
        "SESSION" | "LOCAL" => match words.next() {
            Some(w) => w,
            None => return false,
        },
        _ => first,
    };
    name == "AUTOCOMMIT"
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

    /// **The one statement PostgreSQL does not reject in an aborted
    /// transaction is the one the pill tells the user to run.**
    ///
    /// The pill reads *"Tx aborted — rollback to continue"*, the user types
    /// `ROLLBACK;`, the server accepts it and `Session::in_tx` goes false — but
    /// the fold matched `Poisoned` above the outcome and returned unchanged. So
    /// the tab stayed `Poisoned` for the rest of its life: Commit hidden in the
    /// footer *and* in the close prompt, while `ensure_tx` opened a real,
    /// healthy transaction for everything typed afterwards. The user then builds
    /// work in a live transaction whose only offered actions destroy it.
    ///
    /// Gated on `Ok`, because a *failed* rollback changes nothing — that is the
    /// case the absorbing arm was written for.
    #[test]
    fn a_typed_rollback_is_the_way_out_of_poisoned() {
        let s = TxState::Poisoned { stmts: 2 };
        assert_eq!(ok(s, PG, "ROLLBACK"), TxState::Idle);
        // PostgreSQL turns a `COMMIT` in an aborted transaction into a
        // rollback and answers `Ok`, so it lands in the same place.
        assert_eq!(ok(s, PG, "COMMIT"), TxState::Idle);
        // `AND CHAIN` closes one and opens another, here too.
        assert_eq!(ok(s, PG, "ROLLBACK AND CHAIN"), TxState::Open { stmts: 0 });
        // A rollback the server refused leaves the tab exactly as it was.
        assert_eq!(
            s.on_statement(PG, "ROLLBACK", StmtOutcome::Failed),
            TxState::Poisoned { stmts: 2 }
        );
        // And the composition: the next statement counts, and Commit is
        // offered again for the transaction it belongs to.
        let after = ok(ok(s, PG, "ROLLBACK"), PG, "UPDATE t SET a = 1");
        assert_eq!(after, TxState::Open { stmts: 1 });
        assert!(after.can_commit());
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

    /// **A dismissed read is not a lost transaction.** On PostgreSQL a
    /// cancellation aborts the enclosing transaction exactly as an error does,
    /// so closing the binary-cell panel while its `SELECT` was still running —
    /// Escape, the ✕, the footer, or clicking a second binary cell, all of which
    /// cancel — poisoned a Manual-mode tab and discarded every uncommitted
    /// statement in it. The pill was not wrong, which is what made it data loss.
    ///
    /// The read is fenced by a savepoint now, and this is what the fence means
    /// once the server has accepted the rollback.
    #[test]
    fn a_savepoint_isolated_cancellation_leaves_the_transaction_usable() {
        let s = TxState::Open { stmts: 20 };
        let after = s.on_statement(PG, "SELECT data FROM blobs", StmtOutcome::CancelledIsolated);
        assert_eq!(
            after,
            TxState::Open { stmts: 20 },
            "a read that was undone changed nothing"
        );
        assert!(after.can_commit(), "the user's work is still committable");
        assert_eq!(
            s.on_statement(MY, "SELECT data FROM blobs", StmtOutcome::CancelledIsolated),
            TxState::Open { stmts: 20 }
        );
    }

    /// And the contrast, which is what keeps the fence honest: an *unfenced*
    /// cancellation — a run the user stopped, with no savepoint around it —
    /// still poisons. The isolated variant is only ever reported where the
    /// rollback was issued and accepted.
    #[test]
    fn a_bare_cancellation_still_poisons_postgres() {
        let s = TxState::Open { stmts: 20 };
        assert_eq!(
            s.on_statement(PG, "SELECT pg_sleep(60)", StmtOutcome::Cancelled),
            TxState::Poisoned { stmts: 20 }
        );
    }

    /// Nor can it revive a transaction that is already gone.
    #[test]
    fn a_savepoint_isolated_cancellation_does_not_revive_a_dead_transaction() {
        assert_eq!(
            TxState::Poisoned { stmts: 3 }.on_statement(
                PG,
                "SELECT 1",
                StmtOutcome::CancelledIsolated
            ),
            TxState::Poisoned { stmts: 3 }
        );
        assert_eq!(
            TxState::Lost.on_statement(PG, "SELECT 1", StmtOutcome::CancelledIsolated),
            TxState::Lost
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

    /// `SET` can't be matched wholesale — most of it is session state that
    /// leaves the transaction alone, and claiming a commit that didn't happen is
    /// the mirror-image lie (the pill would go quiet over open work).
    ///
    /// **`SET autocommit` has moved off this predicate**, and the `= 1` rows
    /// that used to assert `implicit_commit` now assert the opposite. That
    /// reading was wrong on every connection Schemaic opens: the app never sets
    /// `autocommit`, so the variable is the server default of 1, and MySQL
    /// commits on `SET autocommit = 1` only when the value *was* 0. The
    /// statement committed nothing while the flag was cleared on the strength of
    /// it, and the next statement's `BEGIN` then implicitly committed the user's
    /// work. It is [`TxAfter::Ask`] now — see
    /// `setting_autocommit_asks_the_server_rather_than_guessing`.
    #[test]
    fn only_set_password_commits_unconditionally() {
        assert!(implicit_commit(MY, "SET PASSWORD FOR u = 'x'"));
        for sql in [
            "SET autocommit = 1",
            "SET AUTOCOMMIT=1",
            "SET @@autocommit = 1",
            "SET SESSION autocommit = ON",
            "SET @@session.autocommit = TRUE",
            "SET autocommit = 0",
            "SET @x = 1",
            "SET @autocommit_backup = 1",
            "SET NAMES utf8mb4",
            "SET SESSION sql_mode = ''",
            "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE",
            "SET GLOBAL autocommit = 1",
            "SET",
        ] {
            assert!(!implicit_commit(MY, sql), "{sql} must not claim a commit");
        }
        // The autocommit rows are not "unchanged" either — they are the ones the
        // text cannot decide.
        for sql in [
            "SET autocommit = 1",
            "SET AUTOCOMMIT=1",
            "SET @@autocommit = 1",
            "SET SESSION autocommit = ON",
            "SET @@session.autocommit = TRUE",
            "SET autocommit = 0",
        ] {
            assert_eq!(tx_after(MY, sql), TxAfter::Ask, "{sql}");
        }
        // …and the ones that really are session noise stay unchanged, including
        // the global variable, which is not this session's.
        for sql in [
            "SET @x = 1",
            "SET @autocommit_backup = 1",
            "SET NAMES utf8mb4",
            "SET GLOBAL autocommit = 1",
            "SET",
        ] {
            assert_eq!(tx_after(MY, sql), TxAfter::Unchanged, "{sql}");
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

    /// **The commit happens before the DDL does**, so an `ALTER` that is killed
    /// or rejected has ended the transaction just as surely as one that
    /// succeeded — and folding it as `Open` is what makes the next **Rollback**
    /// report an undo that never happened over data now permanently written.
    ///
    /// Verified against MariaDB 10.11.14, by opening a transaction, running
    /// `UPDATE z_tx SET v = …`, running a DDL statement, then `ROLLBACK` and
    /// re-reading the row:
    ///
    /// * DDL **succeeds** (`ALTER TABLE z_tx ADD COLUMN tmpc INT`) — the update
    ///   survives the rollback.
    /// * DDL is **rejected by the executor** (`DROP TABLE z_does_not_exist` →
    ///   `ERROR 1051 Unknown table`) — the update survives, and the DDL never
    ///   touched a table.
    /// * DDL is **killed mid-flight** (`ALTER TABLE z_big ADD INDEX …`, a second
    ///   session issuing `KILL QUERY` → `ERROR 1317 Query execution was
    ///   interrupted`) — the update survives, and `SHOW INDEX` reports the index
    ///   was never built.
    /// * DDL is **rejected by the parser** (`ALTER TABLE z_tx GARBAGE GARBAGE` →
    ///   `ERROR 1064`) — the update is **rolled back**. This is the case that
    ///   keeps the decision out of `implicit_commit`, whose input is the leading
    ///   keyword and which cannot see the difference.
    ///
    /// PostgreSQL is the contrast that keeps the whole thing engine-conditional:
    /// the same sequences there roll the update back every time, and a failing
    /// `ALTER` leaves the transaction aborted rather than committed.
    #[test]
    fn a_confirmed_implicit_commit_ends_the_transaction_a_failure_left_open() {
        for sql in [
            "ALTER TABLE t ADD INDEX ix (c)",
            "CREATE TABLE t (a INT)",
            "DROP TABLE t",
            "TRUNCATE TABLE t",
        ] {
            // The server said the transaction is gone.
            assert!(
                failure_committed(TxEngine::MySql, sql, Some(false)),
                "{sql}"
            );
            // It said the transaction is still there — the parser rejection.
            assert!(
                !failure_committed(TxEngine::MySql, sql, Some(true)),
                "{sql}"
            );
            // Nobody could ask.
            assert!(!failure_committed(TxEngine::MySql, sql, None), "{sql}");
            // And PostgreSQL never commits its DDL out from under a transaction,
            // so the probe's answer is not even consulted.
            for alive in [Some(false), Some(true), None] {
                assert!(
                    !failure_committed(TxEngine::Postgres, sql, alive),
                    "{sql} / {alive:?}"
                );
            }
        }
        // A statement that commits nothing is not turned into one by a probe that
        // happens to answer `false` — that would be a lost connection wearing an
        // implicit commit's clothes.
        assert!(!failure_committed(
            TxEngine::MySql,
            "UPDATE t SET a = 1",
            Some(false)
        ));
    }

    /// And the fold, which is the half the pill and the session both read: the
    /// confirmed commit is the one failure that *ends* a transaction instead of
    /// leaving it where it was, on either engine, because the answer came from
    /// the connection rather than from the engine.
    #[test]
    fn a_confirmed_commit_folds_to_idle_and_a_plain_failure_does_not() {
        for engine in [TxEngine::MySql, TxEngine::Postgres] {
            assert_eq!(
                TxState::Open { stmts: 3 }.on_statement(
                    engine,
                    "ALTER TABLE t ADD INDEX ix (c)",
                    StmtOutcome::FailedAndCommitted
                ),
                TxState::Idle,
                "{engine:?}"
            );
        }
        // Unconfirmed, so unchanged: MySQL keeps counting, PostgreSQL poisons.
        for outcome in [StmtOutcome::Failed, StmtOutcome::Cancelled] {
            assert_eq!(
                TxState::Open { stmts: 3 }.on_statement(
                    TxEngine::MySql,
                    "ALTER TABLE t ADD INDEX ix (c)",
                    outcome
                ),
                TxState::Open { stmts: 3 },
                "{outcome:?}"
            );
            assert_eq!(
                TxState::Open { stmts: 3 }.on_statement(
                    TxEngine::Postgres,
                    "ALTER TABLE t ADD COLUMN c int",
                    outcome
                ),
                TxState::Poisoned { stmts: 3 },
                "{outcome:?}"
            );
        }
    }

    /// A statement that never left the client leaves the transaction alone — on
    /// **both** engines. PostgreSQL is the one that mattered: folding this as a
    /// real cancellation poisoned a transaction the server had never aborted, and
    /// the pill then read "Tx aborted — rollback to continue" over work the user
    /// had just saved, with Rollback — the action that discards it — as the way
    /// out it named.
    #[test]
    fn a_statement_that_was_never_sent_leaves_the_transaction_alone() {
        for engine in [TxEngine::MySql, TxEngine::Postgres] {
            assert_eq!(
                TxState::Open { stmts: 4 }.on_statement(
                    engine,
                    "SELECT * FROM t",
                    StmtOutcome::NotSent
                ),
                TxState::Open { stmts: 4 },
                "{engine:?}"
            );
            // The first statement of a Manual tab: the pill still reads `Idle`
            // when it is attempted, but `ensure_tx` has already put a `BEGIN` on
            // the wire, so the transaction is open and empty rather than absent.
            assert_eq!(
                TxState::Idle.on_statement(engine, "SELECT * FROM t", StmtOutcome::NotSent),
                TxState::Open { stmts: 0 },
                "{engine:?}"
            );
        }
        // Contrast, unchanged: a cancellation the server *did* see still poisons
        // PostgreSQL.
        assert_eq!(
            TxState::Open { stmts: 4 }.on_statement(
                TxEngine::Postgres,
                "SELECT * FROM t",
                StmtOutcome::Cancelled
            ),
            TxState::Poisoned { stmts: 4 }
        );
    }

    /// And the message: the timeout may describe a run it stopped, but never a
    /// statement that was never sent.
    #[test]
    fn the_timeout_only_describes_a_statement_that_was_dispatched() {
        assert!(timeout_reached(Some(StmtOutcome::Cancelled), true));
        assert!(timeout_reached(None, true));
        assert!(!timeout_reached(Some(StmtOutcome::NotSent), true));
        // And a run nobody timed out is never described by the clock, whatever
        // else became of it.
        for stmt in [
            None,
            Some(StmtOutcome::Cancelled),
            Some(StmtOutcome::NotSent),
        ] {
            assert!(!timeout_reached(stmt, false), "{stmt:?}");
        }

        let timed = not_sent_message(true);
        let plain = not_sent_message(false);
        assert!(timed.contains("statement timeout fired"));
        assert!(!plain.contains("timeout"));
        for m in [&timed, &plain] {
            assert!(m.contains("never sent"), "{m}");
            assert!(m.contains("transaction is exactly where it was"), "{m}");
            // The claim the old message made and could not support.
            assert!(!m.contains("ran longer than"), "{m}");
        }
    }

    /// The disclosure, which is the only thing on screen that can say a
    /// transaction was spent — and which must not appear on any other outcome,
    /// because an unconfirmed failure leaves the transaction open and telling the
    /// user it is gone would send them to re-run work that is still pending.
    #[test]
    fn only_a_confirmed_commit_says_the_transaction_is_gone() {
        let confirmed = failed_message("ERROR 1317: interrupted", StmtOutcome::FailedAndCommitted);
        assert!(confirmed.starts_with("ERROR 1317: interrupted"));
        assert!(confirmed.contains("Rollback will not undo them"));
        for stmt in [
            StmtOutcome::Ok,
            StmtOutcome::Failed,
            StmtOutcome::FailedIsolated,
            StmtOutcome::Cancelled,
            StmtOutcome::ConnectionLost,
            StmtOutcome::NotSent,
        ] {
            assert_eq!(
                failed_message("ERROR 1064: syntax", stmt),
                "ERROR 1064: syntax",
                "{stmt:?}"
            );
        }
    }

    /// **A Stop and a timeout are the same server state and used to get
    /// different messages.** MySQL commits the open transaction before a DDL
    /// statement, so cancelling a slow `ALTER` inside a Manual one makes
    /// everything already in it permanent — `Session::fetch_query` sets
    /// `FailedAndCommitted` for a cancel exactly as it does for a failure. The
    /// run paths had a timeout arm that disclosed that in full and a bare cancel
    /// arm below it that returned `QueryState::Cancelled`: the one arm that
    /// never called `failed_message`. `timeout_reached` is `timed_out && …`, so
    /// a user's click fell through it.
    #[test]
    fn a_cancel_that_spent_the_transaction_says_so_like_a_timeout_would() {
        let m = cancelled_message(Some(StmtOutcome::FailedAndCommitted))
            .expect("a spent transaction has something to say");
        assert!(m.contains("cancelled"), "{m}");
        assert!(m.contains("Rollback will not undo them"), "{m}");
        // The exact sentence the timeout arm appends, so the two disclosures
        // cannot drift: both come from `failed_message`.
        assert!(
            m.ends_with(
                failed_message("x", StmtOutcome::FailedAndCommitted)
                    .strip_prefix("x")
                    .expect("the disclosure is appended")
            ),
            "{m}"
        );
    }

    /// And nothing else says it. An unconfirmed failure leaves the transaction
    /// open, and telling the user it is gone would send them to re-run work that
    /// is still pending — the same reason `failed_message` is narrow.
    #[test]
    fn an_ordinary_cancel_is_still_just_cancelled() {
        for stmt in [
            None,
            Some(StmtOutcome::Ok),
            Some(StmtOutcome::Failed),
            Some(StmtOutcome::FailedIsolated),
            Some(StmtOutcome::Cancelled),
            Some(StmtOutcome::ConnectionLost),
            Some(StmtOutcome::NotSent),
            Some(StmtOutcome::Untouched),
        ] {
            assert_eq!(cancelled_message(stmt), None, "{stmt:?}");
        }
    }

    /// **The composition, which is where the defect was**: the arm order, fed
    /// the state a user's Stop produces. `timed_out` is false — that is what a
    /// click means — and the outcome is `FailedAndCommitted`, and the pair must
    /// not come out a bare cancel. A test of `failed_message` alone was already
    /// green against the unfixed tree.
    #[test]
    fn the_stop_arm_and_the_timeout_arm_disclose_the_same_loss() {
        let stmt = Some(StmtOutcome::FailedAndCommitted);
        // What the run paths ask, in the order they ask it.
        assert!(
            !timeout_reached(stmt, false),
            "a Stop leaves the clock alone, which is what sent this to the bare arm"
        );
        assert!(
            cancelled_message(stmt).is_some(),
            "so the arm below it has to carry the disclosure"
        );
        // And with the clock, the message that arm produces still carries it.
        assert!(timeout_reached(stmt, true));
        assert!(
            failed_message(
                "the statement timeout fired",
                StmtOutcome::FailedAndCommitted
            )
            .contains("Rollback will not undo them")
        );
    }

    /// The session's own flag, on the same evidence: `tx_open_after` answers what
    /// the *statement* does, and the session pairs it with the outcome — the
    /// success path takes it directly, the failure path takes it only once
    /// [`failure_committed`] has the server's word for it.
    #[test]
    fn the_session_flag_and_the_pill_agree_on_a_confirmed_commit() {
        let sql = "ALTER TABLE t ADD INDEX ix (c)";
        assert_eq!(tx_open_after(TxEngine::MySql, sql), Some(false));
        assert!(failure_committed(TxEngine::MySql, sql, Some(false)));
        // Both silent about a statement that commits nothing, whatever became of
        // it.
        assert_eq!(tx_open_after(TxEngine::MySql, "UPDATE t SET a = 1"), None);
        assert!(!failure_committed(
            TxEngine::MySql,
            "UPDATE t SET a = 1",
            Some(false)
        ));
    }

    #[test]
    fn mysql_ddl_implicitly_commits_and_leaves_nothing_open() {
        for sql in [
            "CREATE TABLE t (a INT)",
            "ALTER TABLE t ADD b INT",
            "DROP TABLE t",
            "TRUNCATE TABLE t",
            "FLUSH TABLES",
            "SET PASSWORD FOR u = 'x'",
        ] {
            assert_eq!(tx_open_after(TxEngine::MySql, sql), Some(false), "{sql}");
        }
        // `SET autocommit = 1` was in this list and is not a commit —
        // see `only_set_password_commits_unconditionally`.
        assert_eq!(tx_open_after(TxEngine::MySql, "SET autocommit = 1"), None);
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

    /// **A typed `COMMIT` or `ROLLBACK` left the session believing the
    /// transaction was still open**, so everything after it auto-committed while
    /// the pill counted, and Rollback reported an undo that never happened.
    ///
    /// `implicit_commit`'s keyword list names the statements that *open* a
    /// transaction (`BEGIN`, `START`) and every MySQL statement that commits one
    /// as a side effect — and none of the statements whose whole purpose is to
    /// **close** one. So on
    /// `UPDATE …; COMMIT; DELETE FROM orders WHERE id = 5;` in a Manual tab, the
    /// `DELETE` ran with no transaction at all (`ensure_tx` saw `in_tx` still
    /// true and issued no `BEGIN`, and both engines default to autocommit) and
    /// was permanent the instant it landed. Measured on MariaDB 10.11.14 and PG
    /// 16.15 with the exact sequence the session emits: `v = 99` after the
    /// `ROLLBACK`, and on PG a `WARNING: there is no transaction in progress`
    /// that `batch_execute` returns `Ok` over, so the UI reported a clean end.
    ///
    /// Engine-independent, which is why it is not a widening of
    /// `implicit_commit` — that models MySQL's implicit-commit list, and
    /// PostgreSQL has to answer here too.
    #[test]
    fn the_statements_that_close_a_transaction_close_it() {
        for engine in [TxEngine::MySql, TxEngine::Postgres] {
            for sql in [
                "COMMIT",
                "commit",
                "COMMIT;",
                "COMMIT WORK",
                "ROLLBACK",
                "rollback;",
                "ROLLBACK WORK",
                "END",
                "END TRANSACTION",
                "ABORT",
                "/* x */ COMMIT",
            ] {
                assert_eq!(tx_after(engine, sql), TxAfter::Closed, "{engine:?} {sql}");
            }
            // `AND CHAIN` closes one and opens another in the same breath —
            // `BEGIN`'s carve-out, arriving from the other side.
            for sql in ["COMMIT AND CHAIN", "ROLLBACK AND CHAIN"] {
                assert_eq!(tx_after(engine, sql), TxAfter::Open, "{engine:?} {sql}");
            }
            // And a rollback *to a savepoint* does not close the transaction.
            for sql in [
                "ROLLBACK TO SAVEPOINT s1",
                "ROLLBACK TO s1",
                "RELEASE SAVEPOINT s1",
                "SAVEPOINT s1",
            ] {
                assert_eq!(
                    tx_after(engine, sql),
                    TxAfter::Unchanged,
                    "{engine:?} {sql}"
                );
            }
        }
    }

    /// **The noise word is optional, and it sits between the keyword and the
    /// word that decides.** MySQL's grammar is
    /// `ROLLBACK [WORK] TO [SAVEPOINT] identifier` and PostgreSQL's is
    /// `ROLLBACK [ WORK | TRANSACTION ] TO [ SAVEPOINT ] name`; `COMMIT`, `END`
    /// and `ABORT` take the same noise word before `AND [NO] CHAIN`. Reading
    /// only the first word after the keyword found `WORK`, missed both guards,
    /// and fell through to `Closed`.
    ///
    /// The cost is not a wrong label. `Session::in_tx` goes false over a
    /// transaction the server still holds, so the next statement's `ensure_tx`
    /// issues a `BEGIN` — which on MySQL **implicitly commits the work the
    /// rollback was meant to keep**. In between, `TxState::Idle` hides Commit
    /// and Rollback, blanks the pill and lets `guard_tx` close the tab unasked.
    ///
    /// The two halves of the existing case never met: it lists the noise-word
    /// forms only on the closing side and the savepoint forms only without one.
    #[test]
    fn a_noise_word_does_not_turn_a_savepoint_rollback_into_a_close() {
        for engine in [TxEngine::MySql, TxEngine::Postgres] {
            for sql in [
                "ROLLBACK WORK TO SAVEPOINT s1",
                "ROLLBACK TRANSACTION TO SAVEPOINT s1",
                "ROLLBACK WORK TO s1",
                // The `rest` this reads used to be raw text rather than lexed,
                // so a comment between the two words was itself the token.
                "ROLLBACK/* x */TO SAVEPOINT s1",
            ] {
                assert_eq!(
                    tx_after(engine, sql),
                    TxAfter::Unchanged,
                    "{engine:?} {sql}"
                );
            }
            for sql in [
                "COMMIT WORK AND CHAIN",
                "END TRANSACTION AND CHAIN",
                "ROLLBACK WORK AND CHAIN",
            ] {
                assert_eq!(tx_after(engine, sql), TxAfter::Open, "{engine:?} {sql}");
            }
            // The composition, not the predicate alone: the pill keeps the
            // transaction it is counting.
            assert!(
                TxState::Open { stmts: 3 }
                    .on_statement(engine, "ROLLBACK WORK TO SAVEPOINT s", StmtOutcome::Ok)
                    .is_open(),
                "{engine:?}"
            );
        }
    }

    /// **PostgreSQL's documented way out of an aborted transaction had no route
    /// out of `Poisoned`.**
    ///
    /// `ROLLBACK TO SAVEPOINT s` is accepted *inside* an aborted transaction and
    /// un-aborts it, leaving the transaction live and committable — measured on
    /// PostgreSQL 16.15. The exit arm enumerated `Closed | Open`, and `tx_after`
    /// answers `Unchanged` for this statement (correctly: the question it
    /// answers is `Session::in_tx`), so the fold dropped it into the absorbing
    /// arm. From there `can_commit()` is false, so **Commit is hidden in the
    /// footer and in the close prompt for the rest of the tab's life**, while
    /// the session's flag is correctly still true and the user goes on building
    /// work in a healthy transaction whose only offered action discards it.
    ///
    /// Stated over the composition, which is where it lives: the predicates were
    /// each right about their own question.
    #[test]
    fn a_savepoint_rollback_takes_an_aborted_transaction_back() {
        let poisoned = TxState::Poisoned { stmts: 2 };
        for sql in [
            "ROLLBACK TO SAVEPOINT s1",
            "ROLLBACK TO s1",
            "ROLLBACK WORK TO SAVEPOINT s1",
            "ROLLBACK TRANSACTION TO s1",
            // Through the shared lexer, so a comment between the words cannot
            // hide it — the same property the closer arm carries.
            "ROLLBACK/* x */TO SAVEPOINT s1",
        ] {
            let after = poisoned.on_statement(TxEngine::Postgres, sql, StmtOutcome::Ok);
            assert!(
                after.can_commit(),
                "{sql}: Commit is still hidden over a healthy transaction — {after:?}"
            );
            assert!(after.is_open(), "{sql}: {after:?}");
            // The count is kept: the work before the savepoint is still there.
            assert_eq!(after.stmts(), 2, "{sql}");
        }

        // **A plain `ROLLBACK` still closes it**, and a statement the server
        // refused inside the aborted transaction still absorbs — the two cases
        // the arm above and the arm below exist for.
        assert!(
            !poisoned
                .on_statement(TxEngine::Postgres, "ROLLBACK", StmtOutcome::Ok)
                .is_open()
        );
        assert!(
            !poisoned
                .on_statement(TxEngine::Postgres, "SELECT 1", StmtOutcome::Failed)
                .can_commit()
        );
        // A savepoint rollback the server *refused* changes nothing.
        assert!(
            !poisoned
                .on_statement(
                    TxEngine::Postgres,
                    "ROLLBACK TO SAVEPOINT s1",
                    StmtOutcome::Failed
                )
                .can_commit()
        );
        // And `RELEASE SAVEPOINT` is not a way out: it discards the savepoint,
        // not the abort.
        assert!(
            !poisoned
                .on_statement(TxEngine::Postgres, "RELEASE SAVEPOINT s1", StmtOutcome::Ok)
                .can_commit()
        );

        // **`FailedIsolated` is deliberately left alone**, and
        // `a_savepoint_isolated_failure_does_not_revive_a_poisoned_transaction`
        // is the decision. Reaching it from here would need a `SAVEPOINT` inside
        // an aborted transaction, which PostgreSQL refuses — so the combination
        // cannot occur, and this widening does not reverse that answer for it.
        assert!(
            !poisoned
                .on_statement(
                    TxEngine::Postgres,
                    "UPDATE t SET a = 1",
                    StmtOutcome::FailedIsolated
                )
                .can_commit()
        );

        // MySQL has no aborted state to clear — a failed statement there leaves
        // the transaction usable — so the predicate answers no for it.
        assert!(!clears_abort(TxEngine::MySql, "ROLLBACK TO SAVEPOINT s1"));
        assert!(clears_abort(TxEngine::Postgres, "ROLLBACK TO SAVEPOINT s1"));
        assert!(!clears_abort(TxEngine::Postgres, "ROLLBACK"));
        assert!(!clears_abort(TxEngine::Postgres, "COMMIT"));
    }

    /// **Three of MySQL's four `START` statements open no transaction.**
    ///
    /// The arm matched the bare keyword, so `START REPLICA` / `START SLAVE` /
    /// `START GROUP_REPLICATION` set `Session::in_tx` true over nothing:
    /// `ensure_tx` then issued no `BEGIN` for the next statement, that statement
    /// ran auto-committed and permanent, and the pill still offered Rollback —
    /// which succeeded as a no-op while the app reported the work undone. The
    /// same seam the first group of `tx_after`'s doc exists for, reached from
    /// the other side.
    ///
    /// They are not `Unchanged` either: MySQL lists the replication-control
    /// statements among those that cause an implicit commit, so the transaction
    /// that *was* open is gone. **Measured on MySQL 8.4.11**: inside an open
    /// transaction holding a `DELETE`, a `STOP REPLICA` and a `RESET REPLICA`
    /// each made the delete permanent — the following `ROLLBACK` succeeded and
    /// the row did not come back. MariaDB 10.11.14 refuses both there instead
    /// (*ERROR 1192: Can't execute the given command because you have … an
    /// active transaction*), which `failure_committed` already handles: it
    /// asks the server whether the transaction survived.
    #[test]
    fn only_start_transaction_opens_one() {
        for sql in [
            "START REPLICA",
            "START SLAVE",
            "START GROUP_REPLICATION",
            "START REPLICA UNTIL SOURCE_LOG_FILE = 'x'",
        ] {
            assert_ne!(
                tx_after(TxEngine::MySql, sql),
                TxAfter::Open,
                "{sql} opens no transaction"
            );
            assert_ne!(tx_open_after(TxEngine::MySql, sql), Some(true), "{sql}");
            // The composition, not the predicate alone: the pill must not start
            // counting a transaction the server never opened.
            assert!(
                !TxState::Idle
                    .on_statement(TxEngine::MySql, sql, StmtOutcome::Ok)
                    .is_open(),
                "{sql}"
            );
        }
        // And the one that does still does, noise word or comment in between.
        for sql in [
            "START TRANSACTION",
            "start transaction read only",
            "START/* x */TRANSACTION",
            "BEGIN",
            "BEGIN WORK",
        ] {
            assert_eq!(tx_after(TxEngine::MySql, sql), TxAfter::Open, "{sql}");
            assert_eq!(tx_after(TxEngine::Postgres, sql), TxAfter::Open, "{sql}");
        }
        // The mirror half: the replication statements that *end* a transaction
        // and matched nothing at all, so the pill kept counting one the server
        // had already committed.
        for sql in [
            "STOP REPLICA",
            "STOP SLAVE",
            "RESET REPLICA ALL",
            "CHANGE REPLICATION SOURCE TO SOURCE_HOST = 'h'",
            "CHANGE MASTER TO MASTER_HOST = 'h'",
        ] {
            assert_eq!(tx_after(TxEngine::MySql, sql), TxAfter::Closed, "{sql}");
            assert!(implicit_commit(TxEngine::MySql, sql), "{sql}");
            // PostgreSQL has transactional DDL and none of these statements.
            assert_eq!(
                tx_after(TxEngine::Postgres, sql),
                TxAfter::Unchanged,
                "{sql}"
            );
        }
        // `RESET PERSIST` is MySQL's own carve-out from that list.
        assert!(!implicit_commit(TxEngine::MySql, "RESET PERSIST"));
        assert!(!implicit_commit(
            TxEngine::MySql,
            "RESET PERSIST IF EXISTS x"
        ));
    }

    /// **MySQL's one documented exception to its own implicit-commit list.**
    ///
    /// *"CREATE TABLE and DROP TABLE statements do not commit a transaction if
    /// the TEMPORARY keyword is used."* Reading the leading keyword alone said
    /// they do, so a `CREATE TEMPORARY TABLE` — a routine working habit —
    /// blanked the pill over a live transaction holding the user's uncommitted
    /// `UPDATE`. From there Commit and Rollback are both hidden, closing the tab
    /// asks nothing and rolls back, and one more statement's `BEGIN` commits the
    /// work instead. `implicit_commit`'s own doc forbids exactly this: it says
    /// the list matches MySQL's documented set "and no wider".
    #[test]
    fn a_temporary_table_does_not_commit_the_transaction() {
        for sql in [
            "CREATE TEMPORARY TABLE tmp (a INT)",
            "create temporary table tmp as select 1",
            "DROP TEMPORARY TABLE tmp",
            "DROP TEMPORARY TABLE IF EXISTS tmp",
            "/* x */ CREATE TEMPORARY TABLE tmp (a INT)",
        ] {
            assert!(!implicit_commit(TxEngine::MySql, sql), "{sql}");
            assert_eq!(tx_after(TxEngine::MySql, sql), TxAfter::Unchanged, "{sql}");
            assert_eq!(
                TxState::Open { stmts: 1 }.on_statement(TxEngine::MySql, sql, StmtOutcome::Ok),
                TxState::Open { stmts: 2 },
                "{sql}"
            );
        }
        // The permanent forms still commit — the carve-out is `TEMPORARY` only.
        for sql in ["CREATE TABLE t (a INT)", "DROP TABLE t"] {
            assert!(implicit_commit(TxEngine::MySql, sql), "{sql}");
        }
        // And a column or an index named `temporary` is not the keyword.
        assert!(implicit_commit(
            TxEngine::MySql,
            "CREATE TABLE temporary (a INT)"
        ));
    }

    /// The seam, not the predicate: the pill folds on the same answer, so it
    /// stops counting when the user closes the transaction by hand.
    #[test]
    fn the_pill_follows_a_typed_commit() {
        for engine in [TxEngine::MySql, TxEngine::Postgres] {
            let open = TxState::Open { stmts: 3 };
            assert_eq!(
                open.on_statement(engine, "COMMIT", StmtOutcome::Ok),
                TxState::Idle,
                "{engine:?}"
            );
            assert_eq!(
                open.on_statement(engine, "ROLLBACK", StmtOutcome::Ok),
                TxState::Idle,
                "{engine:?}"
            );
            // A failed `COMMIT` changed nothing about the transaction.
            assert_ne!(
                open.on_statement(engine, "COMMIT", StmtOutcome::Failed),
                TxState::Idle,
                "{engine:?}"
            );
        }
    }

    /// **A typed `BEGIN` folded the pill to `Idle` while the session and the
    /// server both kept a transaction open**, so `is_open()` was false,
    /// `ddl_blocking_tabs` reported nothing, and a schema Apply queued behind
    /// the tab's metadata lock and failed after the lock-wait timeout — the
    /// exact outcome the prompt exists to prevent. `guard_tx` let a database
    /// switch through unasked for the same reason.
    ///
    /// The two consumers of one predicate had diverged deliberately —
    /// `tx_open_after` carved `BEGIN` out and `on_statement` did not — and the
    /// test that was supposed to hold them together asserted only that the two
    /// *agreed about having an opinion*, never about the direction.
    #[test]
    fn a_typed_begin_starts_the_count_over_rather_than_ending_it() {
        for engine in [TxEngine::MySql, TxEngine::Postgres] {
            for sql in ["BEGIN", "START TRANSACTION"] {
                assert_eq!(tx_after(engine, sql), TxAfter::Open, "{engine:?} {sql}");
                let folded = TxState::Open { stmts: 3 }.on_statement(engine, sql, StmtOutcome::Ok);
                assert_eq!(
                    folded,
                    TxState::Open { stmts: 0 },
                    "{engine:?} {sql}: the transaction is still open"
                );
                assert!(folded.is_open(), "{engine:?} {sql}");
            }
        }
    }

    /// **`SET autocommit = 1` is a no-op on every connection Schemaic opens**,
    /// and the session read it as a commit.
    ///
    /// Schemaic never sets `autocommit`; it issues an explicit `BEGIN`, so the
    /// variable is the server default, **1**. MySQL commits on
    /// `SET autocommit = 1` only when the value *was* 0 — so the statement
    /// committed nothing and the transaction stayed open, while `set_commits`
    /// said `true`, `in_tx` was cleared, and the pill went blank. The user's
    /// next statement then found `in_tx == false` and `ensure_tx` sent a second
    /// `BEGIN`, which on MySQL **implicitly commits** the open transaction: the
    /// uncommitted work became permanent and a later Rollback undid only the
    /// last statement. Measured on MariaDB 10.11.14 — the app's own sequence
    /// ends with `v = 7`, the write the user never committed and cannot roll
    /// back.
    ///
    /// The text cannot decide it, so it no longer pretends to: `TxAfter::Ask`
    /// routes to `Session::tx_alive`, the probe that already exists for a failed
    /// DDL. `SET PASSWORD` stays unconditional.
    #[test]
    fn setting_autocommit_asks_the_server_rather_than_guessing() {
        for sql in [
            "SET autocommit = 1",
            "SET AUTOCOMMIT=ON",
            "SET SESSION autocommit = TRUE",
            "SET autocommit = 0",
        ] {
            assert_eq!(tx_after(TxEngine::MySql, sql), TxAfter::Ask, "{sql}");
            assert!(
                !implicit_commit(TxEngine::MySql, sql),
                "{sql} must stop claiming a commit the server may not have made"
            );
        }
        // Postgres has no such variable, and nothing there commits out of band.
        assert_eq!(
            tx_after(TxEngine::Postgres, "SET autocommit = 1"),
            TxAfter::Unchanged
        );
        // The unconditional one is unchanged.
        assert!(implicit_commit(TxEngine::MySql, "SET PASSWORD = 'x'"));
        assert_eq!(
            tx_after(TxEngine::MySql, "SET PASSWORD = 'x'"),
            TxAfter::Closed
        );
        // And the pill, which cannot ask, keeps counting until the session
        // tells it otherwise through `OkAndClosed`.
        let open = TxState::Open { stmts: 3 };
        assert_eq!(
            open.on_statement(TxEngine::MySql, "SET autocommit = 1", StmtOutcome::Ok),
            TxState::Open { stmts: 4 }
        );
        assert_eq!(
            open.on_statement(
                TxEngine::MySql,
                "SET autocommit = 1",
                StmtOutcome::OkAndClosed
            ),
            TxState::Idle
        );
    }

    /// **A read on the pinned connection cannot report a transaction it did not
    /// open**, which is what poisoned a PostgreSQL tab permanently.
    ///
    /// The state is the ordinary one *between* transactions — right after
    /// Commit, with the session still pinned and the grid still showing rows.
    /// Click a `bytea` cell: `fence_read` sends `SAVEPOINT schemaic_w` and
    /// PostgreSQL 16.15 answers `ERROR: SAVEPOINT can only be used in
    /// transaction blocks` (measured), so the read is unfenced. Dismiss the
    /// panel while the bytes are still coming — Escape, the ✕, the footer, or
    /// clicking a second binary cell, which `fence_read`'s own doc calls the
    /// common case — and the fold turned `Idle` into `Poisoned { stmts: 0 }`.
    /// From there `Poisoned` is terminal for **every** outcome including `Ok`,
    /// `can_commit()` is false so the Commit segment is *hidden*, and the only
    /// exits the UI offers — Rollback, closing the tab, switching mode or
    /// database — all roll back the genuine transaction the user's next
    /// statements opened. MySQL hid it: `SAVEPOINT` outside a transaction is
    /// accepted there (measured on MariaDB 10.11.14), so the read was fenced.
    ///
    /// Asserted as the **composition** — `read_outcome` then the fold — because
    /// either half alone is green: `on_statement(Postgres, "SELECT", Cancelled)`
    /// is *supposed* to poison, and the pinned session has no test seam at all
    /// (`Session::open` refuses SQLite, so the in-memory backend cannot reach
    /// it). The one step this cannot cover is the call in `Session::fetch_blob`.
    #[test]
    fn a_read_outside_a_transaction_leaves_the_state_alone() {
        let cancelled = [
            StmtOutcome::Cancelled,
            StmtOutcome::Failed,
            StmtOutcome::NotSent,
            StmtOutcome::Ok,
        ];
        for engine in [TxEngine::MySql, TxEngine::Postgres] {
            for stmt in cancelled {
                // No transaction: the pill must not move, whatever happened.
                let out = read_outcome(false, stmt);
                assert_eq!(out, StmtOutcome::Untouched, "{engine:?} {stmt:?}");
                assert_eq!(
                    TxState::Idle.on_statement(engine, "SELECT", out),
                    TxState::Idle,
                    "{engine:?} {stmt:?}: invented a transaction"
                );
                // …including the phantom "0 Open" that made `guard_tx` prompt
                // on every tab close and told a designer Apply to queue.
                assert!(
                    !TxState::Idle.on_statement(engine, "SELECT", out).is_open(),
                    "{engine:?} {stmt:?}"
                );
                // A transaction that really is open keeps its own semantics —
                // the fence and `Poisoned` are right there.
                assert_eq!(read_outcome(true, stmt), stmt, "{engine:?} {stmt:?}");
            }
            // Poisoning still happens where it should: inside a transaction.
            assert_eq!(
                TxState::Open { stmts: 2 }.on_statement(
                    TxEngine::Postgres,
                    "SELECT",
                    read_outcome(true, StmtOutcome::Cancelled)
                ),
                TxState::Poisoned { stmts: 2 }
            );
            // A dead connection is a fact about the connection, and the tab has
            // to hear it either way.
            assert_eq!(
                read_outcome(false, StmtOutcome::ConnectionLost),
                StmtOutcome::ConnectionLost
            );
        }
        // And `Untouched` really is neutral, from every state.
        for state in [
            TxState::Idle,
            TxState::Open { stmts: 4 },
            TxState::Poisoned { stmts: 1 },
            TxState::Lost,
        ] {
            assert_eq!(
                state.on_statement(TxEngine::Postgres, "SELECT", StmtOutcome::Untouched),
                state
            );
        }
    }

    /// **The session's flag and the pill agree about the *direction*, which is
    /// the part that was never asserted.**
    ///
    /// This test used to say `tx_open_after(..).is_some() == implicit_commit(..)`
    /// — true of `BEGIN` and silent about what either side then *did* with it.
    /// The two consumers had diverged on exactly that statement:
    /// `tx_open_after` carved it out and kept the flag true, `on_statement`
    /// folded the pill to `Idle`, and in that window `is_open()` was false while
    /// the server held a transaction and its locks. Both read `tx_after` now, so
    /// the property worth pinning is that the pill's fold and the flag say the
    /// same thing about whether a transaction is open afterwards.
    #[test]
    fn the_pill_and_the_sessions_flag_never_disagree() {
        for sql in [
            "CREATE TABLE t (a INT)",
            "UPDATE t SET a = 1",
            "BEGIN",
            "START TRANSACTION",
            "COMMIT",
            "ROLLBACK",
            "COMMIT AND CHAIN",
            "ROLLBACK TO SAVEPOINT s",
            "SAVEPOINT s",
            "SET NAMES utf8mb4",
            "SET autocommit = 0",
            "FLUSH TABLES",
            "SELECT 1",
        ] {
            for engine in [TxEngine::MySql, TxEngine::Postgres] {
                // The pill's answer, from an open transaction.
                let pill = TxState::Open { stmts: 3 }.on_statement(engine, sql, StmtOutcome::Ok);
                // The session's, from the same predicate: `None` means it leaves
                // the flag as it found it, which here is `true`.
                let flag = tx_open_after(engine, sql).unwrap_or(true);
                assert_eq!(
                    pill.is_open(),
                    flag,
                    "{engine:?} {sql}: pill {pill:?} vs flag {flag}"
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
