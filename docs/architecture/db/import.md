# `Db::import_rows` and `Db::copy_rows`

`import_rows` is the bulk-load path, and it has an arm for **all four** engines, each handing
off to its module's own (`pg::`, `sqlite::`, `mysql::` and `mssql::import_rows`, SQL Server's
under `mssql.rs` above), and the shape is the same in each — one transaction of batched multi-row
`INSERT`s pulled from a `RowSource` iterator, each batch required to affect exactly as many rows
as it carried — the `commit_writes` 1-row safety net scaled to a file, without its
statement-per-row round-trips.
**Its cancel lives inside the loop, not in a `select!` around the whole of it.** The MySQL arm
used to race `import_on` against the token and *drop* that future on cancel, which leaves a
`mysql_async` connection's result stream desynchronised: measured on MariaDB 10.11.14 and MySQL
8.4.11, `SELECT CONNECTION_ID()` afterwards came back `Ok(None)`, the following `ROLLBACK`
"succeeded" and the `SHOW WARNINGS` behind `rollback()` was empty — so `Rollback::Complete` was
classified off a reply that was not its own, and a cancelled import into a `MyISAM` table reported
`DbError::Cancelled`, the variant the modal renders as *"nothing was written"*, over 1,000 rows
that were permanently there. That is the exact failure this path's `Rollback::note()` was added to
prevent, still live on the one exit that dropped the connection out from under it. The token goes
*into* `import_on` now, which stops only where the protocol is intact: between batches and again
before the `COMMIT` — both points where the connection is idle and a `ROLLBACK` is its own next
statement — and it races each individual `query_drop`, killing that statement and then **awaiting**
the future rather than dropping it, the same "the killed statement is awaited, not dropped" rule
`run_script_mysql` states. `cancelled_import(Rollback)` is the one place that outcome becomes an
error, so `Cancelled` is reserved for a rollback that completed and anything else carries the note.
Its other exits each roll back and say what that achieved, as a `DbError::Write`: a statement's
error and a short batch count build one directly, and a reader's or statement builder's error
goes through `with_partial_rollback`, which leaves it untouched after a complete rollback — the
message already implies one — and otherwise gives it the `Rollback`: a `Query`'s text becomes a
`WriteCause::Server`, and a row source's failure, which arrives as a `Write` with `undone:
None`, keeps its cause and gains the `undone`.
**A row source's error reaches the engine as a value, not text.** `RowSource`'s item is
`Result<Vec<Value>, import::RowError>`, and the crate-private `row_failure` makes one a
`DbError::write(WriteCause::Row(e), None)` — a statement's failure, so "query failed:" in front
exactly as the `DbError::Query` it used to be — at the three places a row surfaces:
`next_batch`, `ImportTarget::insert_sql` (a `CopyStop` from `build_insert_as`) and SQLite's
import loop, which pulls the source directly. The rollback is left to the engine because only
the engine knows it: MySQL's `with_partial_rollback` above, SQL Server's `failed_on` (under
`mssql.rs`); PostgreSQL and SQLite return it bare, as they returned the `Query`. `every_import_exit_says_what_the_rollback_achieved` reads `import_on`'s source
and fails on a `return Err(` whose next ten lines name none of `Some(undone)`,
`cancelled_import(` and `with_partial_rollback(`, with a floor of three `Some(undone)`. The
`COMMIT` exit's `Server` holds the driver's own sentence, as every other exit's does: it held
`qerr(e).to_string()`, already prefixed, so the modal said "query failed: query failed: …".
PostgreSQL's arm is unchanged — it still races the whole of its own `import_on` — and its
`Cancelled` really does mean nothing was written, every PostgreSQL table being transactional.
`a_cancelled_import_rolls_back_and_says_so` and
`a_cancelled_import_on_a_non_transactional_table_says_the_rows_remain` are the live pins — what
each of them asserts, and why it takes two, is under `tests/live/` below.
**`mysql::cancel_awaited` is that rule as a function, and the pinned `Session` is why it had to
become one.** It races a future against the token and, on a cancel, kills the statement at the
server and then **awaits** the future instead of dropping it. `let _ = fut.await` is not a wait
for the work to finish — the `KILL QUERY` has already stopped it; what is awaited is the *error
reply*, and awaiting it is precisely what leaves the result stream aligned. A body that runs
several statements propagates that error with `?` and stops, which is why `refetch_on`'s loop
needs nothing of its own. All four of `Session`'s MySQL arms — `fetch_query`, `commit_writes`,
`fetch_blob`, `refetch_rows` — raced and **dropped**, and the pinned connection is the one place
in this crate where that is not survivable: everywhere else one connection per operation throws
the poisoned connection away before anyone reads from it again, and `Session` is the documented
exception to that invariant. Measured in opposite directions on a tab that ran an `INSERT`, had a
long `SELECT` cancelled, and then pressed **Commit**. MySQL 8.4.11 returned `Ok(())` from the
`COMMIT`, the app reported success, and a fresh connection saw **0 rows** — silent data loss under
a success report. MariaDB 10.11.14 returned `ERROR 1317` (*"Query execution was interrupted"*),
the app reported failure, and the row **was** committed — Rollback offered over durable data. One
desynchronisation read through two servers' different replies, which is why neither of them looked
like a protocol fault.
**The await is bounded only when the kill went unanswered** (security review S22). A server that
cannot be reached to `KILL` will not answer the statement either, and awaiting it without bound
held the `Session`'s lock — so the tab's Rollback and close queued behind it for ever. So
`mysql::kill_query` returns whether the `KILL QUERY` ran, and the body is
`cancel_awaited_with(fut, kill, cancel, grace)`, pure over its kill and its grace: a kill the
server took is awaited in full as before, however long the statement's own rollback takes, since
there the reply is coming and reading it is what keeps the stream in step; an unanswered one gives
the statement `UNKILLED_GRACE` (10 s) and past that returns
`Err(ConnectFailed(ConnectFailure::SessionRetired))`. Returning there leaves the stream out of step
by construction, so the connection must never be read again: `Session::lost_after_stop`, which all
four `Session` operations already pass through, turns that error on a `Backend::MySql` into
`Backend::Retired` with `in_tx` cleared and reports the operation lost, and dropping the
connection is what the server rolls back on. The pins are
`an_unanswered_kill_retires_the_connection_after_the_grace` and
`an_answered_kill_or_a_reply_in_the_grace_is_only_a_cancel`, on paused tokio time, so the grace
costs the suite nothing. The gate is `session::pg_cancel_gate`'s second test,
`every_mysql_cancel_on_the_pinned_connection_awaits_the_statement_it_killed`, spelled as
"`kill_query` is not called from `session.rs`" rather than as "no `select!`" — the PostgreSQL arms
in that file race quite correctly, `Client` being a handle onto a connection task that drains
regardless of the receiver, so forbidding the construct would forbid the wrong thing — and it
carries a floor of four calls, so it cannot pass by the cancels having been deleted instead.
`a_stop_inside_a_manual_transaction_leaves_the_connection_usable` is the live pin, under
`tests/live/` below.
**One `select!` in that file was deleted rather than converted.** `Session::fetch_query`'s
PostgreSQL branch had an outer race around `pg::run_statement`, which already cancels around
`simple_query_raw` and again per message of the result stream, both on this connection's own
transport — so the outer one was redundant, and since `select!` polls ready branches in random
order, which of the two answered a cancel was a coin flip. Its one *distinct* effect was the
harmful one: winning during the **describe** phase drops the future between `SAVEPOINT
schemaic_describe` and its `RELEASE`, leaving the savepoint and an un-`CLOSE`d prepared statement
in the user's transaction — measured on PG 16.15 as three stacked savepoints after three cancels.
The describe is one round trip and is **not interruptible** now; if it ever must be, the
interruption belongs *inside* `run_statement`, between those two statements, where it can clean up
after itself. That half is a deletion and no test can be made to fail on it, which is said here
and in the commit rather than left to look like coverage.
**`Db::copy_rows(database, select, to, ImportTarget, cancel, progress)` is `import_rows` with a
database for its file** — the data half of *Copy to* (`core::copy_table`), run once the preview
has created the table. The read is `stream_query_enforced(…, Some(Enforce::ReadOnly))` on `self`
in `COPY_CHUNK_ROWS` (2,000) chunks: the `SELECT` is generated, but a copy writes nothing on its
source by any route. **Both ends run on `Db::for_rows`** — the connection's init SQL, then
`export::rows_session_sql` (MySQL's `SET time_zone = '+00:00'`, last so an init's own zone does
not move it) — because a MySQL `TIMESTAMP` is handed over in the session's zone and read back
from it, so two sessions in different zones moved every one; the dump's read takes the same
handle (`cli::dump::run`). The write is `to.import_rows`, so it inherits that path's whole discipline —
one transaction, every batch's count checked, the cancel inside the loop — and a copy that stops
part-way leaves a transactional target table as the `CREATE` made it rather than holding the
first rows as if they were all. A non-transactional one keeps what arrived, exactly as an import
into it does, and the error carries `Rollback::note` to say so. **The read is a spawned task of its own, and that is load-bearing.** The writer
pulls its rows through the private `ChunkRows` iterator with `blocking_recv`, which
`next_batch_off_executor` makes legal by pulling under `block_in_place`; joined on one task
instead, that blocking pull would hold the very thread the read needs to produce the next chunk.
The channel between them holds two chunks. `next_batch_off_executor` pulls inline on any other
runtime flavour, so `copy_rows` **refuses on anything but the multi-threaded runtime** rather
than block the only thread the read could run on — the app runs that runtime, and the SQLite
tests that call it are `flavor = "multi_thread"`. **It relies on
`stream_query_enforced` sending its failure down the channel as well as returning it**: the
write ends when the channel closes, so a read that stopped on an error and only dropped its
sender would read as the end of the rows and commit what had arrived.
`a_copy_whose_read_fails_writes_nothing` (SQLite) pins it with an `abs` overflow on row 3,001,
after a whole chunk has gone to the writer — a `const` assertion holds `COPY_CHUNK_ROWS` under
3,000 so it stays after one — and it was seen to fail with 2,000 rows committed when that send
was removed (per the change's author). After the write, whatever it did, the read's child token
is cancelled and the task awaited; the write's error wins over the read's. `progress` is called
with the rows handed to the writer so far, once per chunk. **`ChunkRows` vets each chunk before
handing over its first row**: when `copy_table::chunk_refusal` answers — a `<n bytes>`
placeholder in a bytes column, or a column past the chunk's arena cap — its `CopyStop` is the
reader's error, a `RowError::Copy` (the read's own failure a `RowError::Read`), and the write
rolls back as it does on a bad record in a file, rather than writing a value the read never had
(`a_copy_never_writes_a_bytes_placeholder`). `ImportTarget` carries
the copy's `forms` and `overriding` (`copy_table::rows_read`) and derives `Default`, a file
import passing neither; its `insert_sql` is how all four engines' writers build a batch,
through `import::build_insert_as`, whose refusal of a cell not in its form's shape comes back
as `CopyStop::NotItsForm` and leaves as `row_failure`'s `DbError::Write`.
`a_copy_moves_every_row_from_one_database_to_another` is the SQLite happy path; the live tier's
`a_copied_table_has_every_row_and_numbers_on_past_them` runs it end to end — plan, rows, settle —
in `runtime.rs` on MariaDB, MySQL 8 and PostgreSQL and under the same name in `mssql.rs`.
