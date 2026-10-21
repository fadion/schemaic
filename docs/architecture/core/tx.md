# `core/tx.rs`

`tx.rs` — the **manual-transaction** state machine behind `TxMode::Manual` (no DB, no UI).
Three engines of the four: **SQLite has no manual mode**, so the status-bar segment offering it
is hidden on such a connection and `Session::open` refuses one — not because SQLite lacks
transactions but because a pinned `rusqlite::Connection` is blocking and `!Sync`, needing a
thread of its own and a channel, which is worth building deliberately rather than as a side
effect of adding an engine. Running the tab's statements on fresh connections instead would
break the single promise the mode makes. `supports_manual_mode` is that answer, an exhaustive
`match` (the footer asked `is_sqlite` before there was a second engine without one; SQL Server
was that second engine until its pinned session was written, under `db::mssql`).
**`offers_manual_mode(dialect, read_only)` is what the footer actually asks** — its
`manual_supported` memo, with the connection's `read_only` — and it is `supports_manual_mode`
narrowed by whether the engine can hold a *read-only* transaction. MySQL and PostgreSQL can, so
a read-only connection's pinned session is a read-only one (`Session::open_enforced`, under
`schemaic-db`); SQL Server cannot, so `open_enforced` refuses a read-only SQL Server connection
a session, and the segment is left off there rather than offering a mode whose every press ends
in that refusal (`a_read_only_connection_is_offered_manual_mode_where_the_engine_holds_one`).
`Session::open`'s refusals stay as the backstop.
`TxState::on_statement(engine, sql, outcome)` folds one statement into
`Idle`/`Open{stmts}`/`Poisoned{stmts}`/`Lost`. It is a state machine rather than a bool because
the engines diverge: PostgreSQL aborts the *whole* transaction on any error (`Poisoned` — only
`ROLLBACK` gets out), MySQL survives a failed statement but silently commits on mid-transaction
DDL, and SQL Server (`TxEngine::MsSql`) is a third model rather than either — its DDL is
transactional, so nothing commits by side effect, and a failed statement leaves the transaction
open and committable under `XACT_ABORT OFF`, the default, unless the server rolled it back
itself or *doomed* it (below). `implicit_commit` is MySQL's list, read through the shared
`leading_keyword` lexer; a **miss is not harmless** — after one, a Rollback runs as a successful
no-op and reports an undo that never happened.
**What that list never named is the statements whose whole purpose is to close a transaction**,
and three consequences came out of the gap. A typed `COMMIT`/`ROLLBACK` in a Manual tab left the
session's `in_tx` true, so `ensure_tx` issued no `BEGIN` for the next statement and it ran with
no transaction at all — both engines default to autocommit — permanent the instant it landed,
while the pill read `3 Open` and Rollback stayed live, no-oped on the server, and `end_tx` closed
the state with no message. Measured with the exact sequence the session emits: MariaDB 10.11.14
and PostgreSQL 16.15 both end at `v = 99`, and PG adds a `WARNING: there is no transaction in
progress` that `batch_execute` returns `Ok` over, so the UI reported a clean end. A typed
`BEGIN` was the mirror — it folded the pill to `Idle` while the session and the server both kept
a transaction open, so `is_open()` was false, `ddl_blocking_tabs` reported nothing, and a schema
Apply queued behind the tab's own metadata lock until the lock-wait timeout, which is the
outcome that prompt exists to prevent; `guard_tx` let a database switch through unasked for the
same reason. And `SET autocommit = 1` is a **no-op on every connection Schemaic opens**: the app
never sets `autocommit`, it issues an explicit `BEGIN`, so the variable is the server default of
1, and MySQL commits on that statement only when the value *was* 0. `set_commits` said `true`,
`in_tx` was cleared, the pill went blank, and the next statement's `BEGIN` then implicitly
committed the user's uncommitted work — the app's own sequence ends at `v = 7`, measured on
MariaDB.
**`tx::tx_after` is the one predicate the session's flag and the pill both read**, so the two
cannot drift, and it carries the fourth answer `Option<bool>` could not: `TxAfter::Closed` for
the deliberate closers on **both** engines (`COMMIT`/`ROLLBACK`/`END`/`ABORT`, but not
`ROLLBACK TO [SAVEPOINT]`, which discards work *inside* the transaction and leaves it open, and
not `RELEASE SAVEPOINT`); `TxAfter::Open` for the statements that leave a *new* transaction
(`BEGIN` unconditionally, `START` only ahead of `TRANSACTION`, and `COMMIT`/`ROLLBACK … AND
CHAIN`); `TxAfter::Ask` for the two cases no text carries — a MySQL `SET autocommit`, whose
effect turns on the value the variable *had*, and a replication `START`, where the two engines
behind `TxEngine::MySql` disagree (below) — both MySQL-only, that variable being MySQL's alone
and `tx_alive` having no PostgreSQL probe to answer with; and `Unchanged` otherwise.
`tx_open_after` is derived from it, `set_commits` is down to `SET PASSWORD`, and
`set_touches_autocommit` is the new sibling. `Ask` routes to
`Session::tx_alive` — the probe a failed DDL already used, so no new machinery — and the probe's
answer reaches the pill, which cannot ask, as the new `StmtOutcome::OkAndClosed`. The old
agreement test asserted only that the two consumers *had an opinion*, never which, which is
exactly how the `BEGIN` divergence stayed untested; it is
`the_pill_and_the_sessions_flag_never_disagree` now, over the direction.
**That opener arm matched the bare keyword `START`, and three of MySQL's four `START` statements
open no transaction.** `START REPLICA` — with `START SLAVE` and `START GROUP_REPLICATION` — set
`Session::in_tx` true over nothing; `ensure_tx` then issued no `BEGIN`, the next statement ran
auto-committed and permanent, and Rollback reported an undo that never happened, which is the
failure the closers group exists for reached from the other side. **Whether they *close* one is
a second question, and the text cannot answer it.** They fell through to `implicit_commit`, on
the reading that MySQL lists the replication-control statements among those that commit.
It does, and MariaDB does not — and both engines are `TxEngine::MySql` here, so the arm answered
`Closed` for a statement that leaves a MariaDB transaction open. Measured with `FLUSH LOGS` as
the control, it being unambiguously in MySQL's implicit-commit group, so a method that cannot see
*it* commit is a broken method: on MySQL 8.4.11 `START REPLICA` commits, and on MariaDB 10.11.14
`START SLAVE` and `START ALL SLAVES` do not — `START TRANSACTION; DELETE … LIMIT 1;
<statement>; ROLLBACK;` brought the row back. Reproducing that on MariaDB needs a master
configured, or the statement fails with *ERROR 1200* before it can say anything; a dummy
unreachable one is enough. And `Closed` is the worse of the two errors rather than the safe one:
`TxState::is_open` is what raises the close/disconnect "you'll lose work" prompt, so the tab
reported no transaction while the server still held one and the user's uncommitted work was
discarded at close with nothing said, Commit and Rollback withdrawn in the same breath —
Rollback being the one action that would have saved it. The review finding behind this
(`S1.2-L1-01`) had the direction backwards, reading the arm as over-reporting `Open`. So
`"START" => TxAfter::Ask` is the arm now, sitting below the `TRANSACTION` one: assume the
transaction survived, then probe, which lands right on both engines — MariaDB's probe says the
transaction is alive so the flag stays, MySQL's says it is gone so it clears.
`a_replication_start_cannot_be_read_off_the_text` pins the composition as well as the predicate,
and `only_start_transaction_opens_one`'s composition assertion now runs through
`StmtOutcome::OkAndClosed`, the outcome the session upgrades to once it has asked, because the
text no longer decides it. **`implicit_commit`'s list is unchanged**, `START` included —
`START TRANSACTION` does commit the previous transaction — and it still answers "did this commit"
for the `STOP`/`RESET`/`CHANGE` group, which had matched **none** of them, so the pill kept
counting a transaction the server had already ended. `STOP`, `RESET` and `CHANGE` are its entries
(`STOP REPLICA`/`SLAVE`, `RESET REPLICA`/`SLAVE`/`MASTER`, `CHANGE REPLICATION SOURCE TO`/
`CHANGE MASTER TO`; no other statement in the language leads with `CHANGE`), with MySQL's own
carve-out for `RESET PERSIST` beside the `CREATE`/`DROP TEMPORARY` one. **Measured on MySQL
8.4.11**: inside an open transaction holding a `DELETE`, `STOP REPLICA` and `RESET REPLICA` each
made the delete permanent — the following `ROLLBACK` succeeded and the row did not come back.
MariaDB 10.11.14 refuses both there instead (`ERROR 1192`), which `failure_committed` already
covers by asking the server whether the transaction survived.
**`tx::clears_abort` is the third question about a statement, and nothing was asking it.**
`tx_after` answers "is there a transaction afterwards" and `implicit_commit` answers "did this
end one"; neither is "is it still aborted", and PostgreSQL has exactly one statement where the
three come apart. `ROLLBACK [WORK|TRANSACTION] TO [SAVEPOINT] s` is accepted *inside* an aborted
transaction, clears the aborted state, and leaves the transaction open and committable —
`SAVEPOINT s1; SELECT 1/0; ROLLBACK TO SAVEPOINT s1; SELECT 1;`, where the `SELECT` and the
`COMMIT` after it both succeed, measured on PostgreSQL 16.15. `tx_after` says `Unchanged` for it,
which is right for the session's flag, and `Poisoned`'s exit arm enumerated `Closed | Open`, so
the statement fell to the absorbing arm and the tab stayed `Poisoned` for the rest of its life:
`can_commit()` false, **Commit hidden** in the footer and in the close prompt, while the
session's own flag was correctly still true and the user went on building work in a healthy
transaction whose only offered action discarded it. It is PostgreSQL-only by construction —
MySQL has no aborted state to clear, a failed statement there leaving the transaction usable, so
the fold never reaches `Poisoned` on that engine. And it is deliberately **narrower than the
review that found it proposed**: `StmtOutcome::FailedIsolated` is left alone, because reaching it
from `Poisoned` needs a `SAVEPOINT` inside an aborted transaction and PostgreSQL refuses one, so
the combination is unreachable and
`a_savepoint_isolated_failure_does_not_revive_a_poisoned_transaction` had already decided it the
conservative way.
**For a statement that *failed*, the list is necessary but not sufficient**, and
`failure_committed(engine, sql, tx_alive)` is the pair: MySQL's implicit commit sits between the
parser and the executor, so a parsed-but-rejected `DROP TABLE nosuch` (`ERROR 1051`) has
committed while a syntax error over the same leading keyword (`ALTER TABLE t GARBAGE`,
`ERROR 1064`) has not — verified against MariaDB 10.11.14, where `@@in_transaction` reads `0`
and `1` respectively. Only the server can tell those apart, so the text is consulted first (the
round trip is paid only where the answer could matter) and `tx_alive` decides;
`None` — not asked, or the server could not answer — keeps the conservative reading that was
there before the probe, rather than trading a silent no-op Rollback for its mirror, a pill
saying *Idle* over a transaction whose next statement's `BEGIN` would commit it.
`StmtOutcome::FailedAndCommitted` is the confirmed case and the first outcome `failed_message`
appended a disclosure to (SQL Server's two, below, are the others), because that loss is
otherwise invisible: the statements folded in are
already permanent, **Rollback** succeeds and undoes nothing, and the pill going quiet is the
only thing on screen that moved.
**`cancelled_message` is the same disclosure for the arm that had none, and a Stop is not a
smaller timeout.** MySQL commits before it runs a DDL statement whether the statement finishes or
is killed, so pressing Stop on a slow `ALTER` inside a Manual transaction spends it exactly as a
timeout does — and the run paths had two cancel arms: the timeout one, which appended
`failed_message`'s sentence in full, and a bare one below it returning `QueryState::Cancelled`,
the single arm that never called `failed_message`. `timeout_reached` is `timed_out && …` and a
user's Stop leaves that flag false, so the identical server state disclosed the loss when the
clock ran out and said nothing when the user clicked. It answers `None` where the bare word is
the whole truth, and lives in `core::tx` rather than at the two call sites so a third run path
cannot arrive without it. `StmtOutcome::NotSent` is the opposite error — a run cancelled
(by the user or by the statement timeout) while still *queued* behind the tab's own connection,
which never reached the server at all. It is reported apart from `Cancelled`, which now means
strictly *dispatched and killed*, because the two have opposite consequences on PostgreSQL:
folding the second as the first put **Tx aborted — rollback to continue** over a healthy
transaction and offered discarding work as the only way on. `timeout_reached` is the same
distinction for the watchdog, which is armed around the whole run — connect, `BEGIN`, statement,
rows — deliberately, so its flag answers "did the clock run out" and not "what ran too long";
`not_sent_message` is what such a run says instead, and its load-bearing half is that the
transaction is untouched. `StmtOutcome::FailedIsolated` is what a `SAVEPOINT`-wrapped grid
write reports, and it must not poison PostgreSQL, or one bad cell edit would tell the user their whole
transaction died. **`StmtOutcome::CancelledIsolated` is its twin, and it exists for the *reads*.**
On PostgreSQL a cancelled statement aborts the enclosing transaction exactly as a failed one
does, so dismissing the binary-cell panel mid-read discarded every uncommitted statement a
Manual-mode tab had built up — and the pill was not wrong about it. A read has nothing of its own
to lose by being rolled back, which is what makes fencing one and reporting this both safe and
true; both isolated outcomes fold to `Open { stmts }` on either engine, the enclosing transaction
being untouched but gaining no statement. It is reported **only** where a savepoint rollback was
issued and accepted (`Session::classify_fenced`) — a cancellation with no fence around it is
still `Cancelled`, because nothing then guarantees the transaction survived.
**`run_error_source` is what a failed run's outcome adds to its error's source.**
`DbError::modal_source` reads the variant, and a statement the server rejected and one whose
pinned connection died under it both arrive as `Query` — but the session reports
`StmtOutcome::ConnectionLost` beside the second, and that statement did nothing wrong. So
`Statement` + `ConnectionLost` is `Server` (explained, never "fixed" into the buffer) and every
other pair keeps what the error said. `run_query_core`'s generic error arm and Run All's session
loop both call it (`a_lost_connection_is_the_servers_failure_not_the_statements`).
**`run_failed_message` is `failed_message` given the state the run found**, because a dead
pinned connection said nothing about the work that went with it. `ConnectionLost` over an open
transaction folds the tab to `Lost`, which keeps no count and whose pill says only "Transaction
lost", and the driver's error is about a socket — while the server rolls an unfinished
transaction back when its connection ends, so every statement run in it since it began is
undone. The sentence says so with the number, which only `before` still knows:
`run_query_core` reads `tab.tx` as it launches (one run at a time holds a pinned session, so
nothing moves it meanwhile), and Run All's session loop folds each outcome through the same
`on_statement` as it goes and hands each statement the state it found. Every other pair is
`failed_message`'s (`a_lost_connection_says_the_open_transaction_and_its_statements_are_gone`,
`a_run_failure_without_a_lost_transaction_is_failed_messages`). Both paths' timeout arms go
through it too. **It is also given the engine and the statement lost, because "undone" is not
always what the engine delivered**: a connection lost on a `COMMIT` (or PostgreSQL's `END`, or
on SQL Server a `COMMIT` anywhere in the batch, `text_commits`) may have committed first, so the
outcome is said to be *unknown*; one lost under a MySQL statement `implicit_commit` names says the
earlier statements were most likely committed before it began; and on MySQL "undone" carries
the caveat that a write to a non-transactional table (MyISAM, MEMORY) stays. The first two end
in *Check the data before running any of it again* — the plain "undone" had a user re-run two
credits that had landed (`a_connection_lost_on_a_commit_does_not_claim_the_transaction_was_undone`,
`a_connection_lost_on_mysql_ddl_says_the_earlier_statements_were_committed`,
`a_connection_lost_on_mysql_names_what_a_rollback_cannot_undo`). **`run_cancelled_message` is the same for a Stop**: a `DbError::Cancelled`
carrying `ConnectionLost` over an open transaction — SQL Server replacing a connection a Stop
left out of step, under `db::mssql` — is `run_failed_message` of "The statement was cancelled.",
where `cancelled_message`, which has no arm for it, answered the bare *Cancelled*; anything else
is `cancelled_message`'s (`a_stop_that_lost_the_connection_says_the_transaction_went_with_it`).
**What the app shows is not these functions' English but the interface's wording of the same
choice.** Each disclosure is a `TxNote` (`Copy`) — `CommittedBeforeDdl`, `RolledBackByServer`,
`Doomed`, `AfterOwnCommit`, and the lost connection's three, `LostOnCommit`,
`LostOnImplicitCommit` and `LostRolledBack`, each carrying the `stmts` count and the last a
`mysql` flag for the MyISAM caveat — picked by `failure_note`, `run_failure_note`,
`cancelled_note` and `run_cancelled_note`, with `TxNote::message` the English and `CANCELLED`
the sentence a Stop's note follows. The four `*_message` functions are now the private
`with_note` over their chooser — the message, a blank line, the note — so every decision above
stays here and is tested here, and their output is byte-identical to before. Both run paths
call `ui::i18n`'s `run_failed_message`, `run_cancelled_message` and `not_sent_message` instead,
which take core's arguments, ask core's choosers and word only the answer from `tx-*` (under
*UI conventions*' string catalogue); `i18n::run_cancelled_message(` is therefore the name the
main.rs source gate over a Stop's disclosure matches. **A failure's wording is its `shown`,
never its `text`**: `run_query_core`'s error arm and Run All's session loop build the
`ModalError`'s `text` from this module's `run_failed_message` over the error's English
`to_string()`, and only `shown_as` from `i18n::run_failed_message` over the app's
`db_error_shown`. They used to put the interface's wording in `text`, so under a German
interface *Explain* sent the model the note in German and the write log (`ran_of`) recorded it
so. The not-sent, timeout and cancel notes were the last to be split, and had the same fault:
each was a `ModalError::app` of the wording, so its `text` — what *Explain* sends and the write
log keeps — was the translation. Each is now `ModalError::app(<English>).shown_as(<wording>)`:
not-sent is this module's `not_sent_message` beside `i18n::not_sent_message`; the timeout is
`connection::StatementTimeout::cancelled_message` beside the app's `timeout_message` — inside
`run_failed_message` from here and from `ui::i18n` respectively where a session's note follows
it, bare on Run All's own connection; and a Stop's disclosure is `run_cancelled_message` from
here as `text`, `i18n`'s as `shown`. Core's English otherwise
stays for core's own tests and the live tier, whose `tests/live/mssql.rs` reads
`failed_message`.
**And `Lost` has no way back on the tab itself.** `can_commit` and `can_rollback` are both false
there, so the footer hides Commit and Rollback alike, and a dead MySQL or PostgreSQL connection
is never re-dialled — `Session::alive` only classifies, because reconnecting silently would hand
the user a fresh, empty transaction over work already discarded. The next run goes to the same
dead connection (the session's `in_tx` is still set, so `ensure_tx` sends nothing), comes back
`ConnectionLost` again, and since `Lost` is not `is_open` the fold takes it to `Idle`: the pill
goes blank over a session that still cannot run anything, and every later run fails the same
way. A working connection comes only from something that calls `open_session` — switching the
tab to Auto-commit and back (`guard_tx` runs straight through, nothing being open), terminating
the session from Server Activity (`repair_killed_session`), or picking another database — and
**`open_session` resets `tab.tx` to closed right after dropping the old session**, since a fresh
session holds no transaction. That reset is load-bearing for the database switch, which
otherwise re-pinned without touching `tab.tx`: from `Lost` it left a live session under a pill
still reading "Transaction lost", and `Lost` absorbs every outcome but `ConnectionLost`, so the
real transaction the next `BEGIN` opened was never shown — Commit and Rollback hidden, and no
close or switch prompt, `is_open` being false
(`a_fresh_session_starts_the_tab_with_no_transaction`).
**And a read on the pinned connection must not report a transaction it never opened.**
`TxState::on_statement` merges `Idle` with `Open` on the stated premise that the app issues
`BEGIN` lazily, so the first statement lands as the first statement of a fresh transaction —
true of `fetch_query`, which `ensure_tx` always precedes, and false of `Session::fetch_blob` and
`Session::refetch_rows`, which never call it. So between transactions, right after Commit with
the session still pinned and the grid still showing rows, dismissing the binary-cell panel while
the bytes were coming folded `Idle` into `Poisoned { stmts: 0 }`: the pill read "Tx aborted" for
a tab with no transaction, `Poisoned` is terminal for **every** later outcome including `Ok`,
`can_commit()` is false so the Commit segment is *hidden* rather than dimmed, and every exit the
UI offers — Rollback, closing the tab, switching mode or database — rolled back the genuine
transaction the user's next statements opened. A read cancelled *before dispatch* folded `Idle`
to `Open { stmts: 0 }` on both engines instead, so the pill read "0 Open", `guard_tx` prompted
about a transaction on every tab close, mode switch and database switch, and `ddl_blocking_tabs`
told a designer Apply to queue behind one the server never opened.
`read_outcome(in_tx, stmt)` is the answer, and `StmtOutcome::Untouched` — neutral from every
state — is what it returns when there is no transaction to have an opinion about. `in_tx` is the
session's own flag, read under the lock the `BEGIN` goes out on, so this is the connection's
answer rather than a guess about it, and `ConnectionLost` passes through: a dead connection is a
fact about the connection and the tab has to hear it. It is tested as the **composition** —
`read_outcome` and then the fold — because either half alone is green:
`on_statement(Postgres, "SELECT", Cancelled)` is *supposed* to poison, and the pinned session
has no test seam of its own at all, `Session::open` refusing SQLite so the in-memory backend
cannot reach it.
**SQL Server's state is read off the server after every statement, because its text decides
none of it.** `probes_after_every_statement(engine)` is `true` for `TxEngine::MsSql` alone
(`sql_server_has_a_manual_mode_that_asks_the_server`): `@@TRANCOUNT` counts nested `BEGIN TRAN`s, so a typed `COMMIT` may close nothing; `ROLLBACK
TRANSACTION s` is a savepoint's and leaves the transaction open; a batch can end a transaction
inside an `IF` or a `TRY` the head keyword never sees; and the server rolls a transaction back
itself for a deadlock victim or a batch-aborting error. So `tx_after(MsSql, …)` answers `Ask`
for a leading `COMMIT`/`ROLLBACK`/`BEGIN`/`SAVE` — a bare `BEGIN` is a block in T-SQL, not a
transaction, where the other two engines' arm answers `Open` for it — and `Unchanged` for
everything else, with `implicit_commit` `false` throughout
(`t_sql_transaction_statements_are_asked_about_and_ddl_does_not_commit`). `Ask` there says only
that the text is not to be trusted; the session probes regardless (`Session::settle_from_server`,
under `db::mssql`), one round trip per statement, which is the price of never guessing on a mode
whose promise is that nothing commits by surprise. The probe's answers arrive as outcomes no text
could produce. `OkAndClosed` was already there. `StmtOutcome::FailedAndRolledBack` — the server
ended the transaction by rolling it back — folds to `Idle`, `FailedAndCommitted`'s mirror: the
transaction is gone and the pill quiet either way, but here everything since `BEGIN` is undone
rather than permanent. `StmtOutcome::FailedAndDoomed` — `XACT_STATE()` of -1, still open but
uncommittable until a `ROLLBACK` — folds to `Poisoned`, PostgreSQL's aborted state by another
name; an ordinary failure stays `Open` there, as on MySQL
(`a_sql_server_failure_folds_to_what_the_server_said`). **`Poisoned` has one more exit, and it
is load-bearing**: `Poisoned` + `OkAndClosed` → `Idle`. A doomed transaction's way out is a
`ROLLBACK` whose text cannot confirm it closed anything — `tx_after` asks — so the probe's answer
arrives as `OkAndClosed`, while the older exit arm matches only `Ok`; without the new arm the tab
read "Tx aborted" for the rest of its life over no transaction at all. `failed_message` and
`cancelled_message` carry a sentence for each new outcome, since neither loss is in the server's
error: that the rolled-back transaction's statements are undone, and that the doomed one can
only be rolled back (`a_rolled_back_transaction_is_disclosed`).
**The fold from the probe's answer is `settle_from_server(stmt, was_open, trancount,
xact_state, commits)`**, pure here and pinned arm by arm (`sql_servers_answer_folds_into_the_outcome`);
it had lived in the session, with no test but a live one exercising a single failure.
**`@@TRANCOUNT` of 0 reads the same after a commit as after a rollback**, so `commits` —
`text_commits`, a `COMMIT` among the statement's `sql::tsql_statements` — turns a failure over a
closed transaction into `StmtOutcome::FailedAfterCommit` (→ `Idle`), whose sentence says what
came before the `COMMIT` is likely committed. A T-SQL piece is a batch, and from a `DECLARE`
the rest of it is one piece, so `DECLARE @id int = 1; UPDATE …; COMMIT; SELECT 1/0;` failed as
one and was reported as undoing a debit another connection could read; a user who believed it
ran the debit twice (`a_failure_after_a_pieces_own_commit_is_not_reported_rolled_back`; live,
`a_piece_that_commits_then_fails_is_not_reported_rolled_back`, on 2022 and 2025). A `COMMIT`
inside a procedure the piece calls is out of the text's sight and folds as before. **No
success folds to doomed, and that is not a missing arm.** A review expected one — a `TRY …
CATCH` that catches a dooming error raises nothing itself — but every operation the session
runs is its own batch, and SQL Server rolls back an uncommittable transaction when its batch
ends (Msg 3998), so the statement arrives failed over a closed transaction and folds
`FailedAndRolledBack` (live, `a_caught_error_that_dooms_the_transaction_rolls_it_back_at_the_batch_end`,
on 2022 and 2025).
`pill_text` is the status-bar string. It also owns what the user is told
while a write **waits**: `write_blocking_tabs` (which of our own tabs' transactions a grid
write could be queued behind — same connection scope as `ddl_blocking_tabs`, but excluding
the writer's own tab, whose write runs *inside* that transaction) and `write_wait_note` →
`WaitNote` (the sentence after `WRITE_WAIT_MS`, plus the one tab to offer a `ROLLBACK` for —
`None` for several, since one button would have to pick and picking wrong ends a transaction
the user didn't mean to). The sentence never names the tab; **the button does**, clipped by
the bar — a custom tab name is arbitrarily long, and spelled into the sentence it pushed the
button off the edge. Deliberately hedged: Schemaic doesn't track which rows a
transaction touched, so an open one elsewhere is a *candidate*, not a diagnosis.
