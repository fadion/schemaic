# `core/write_log.rs`

`write_log.rs` — **the write log**: an append-only record of every write the app and
`schemaic exec` ran — when, on which connection and database, what ran, how many rows it
touched and how it ended — one JSON object a line in `<config_dir>/write_log.jsonl` (`FILE`).
**It is not the history, and the difference is the point.** `history.json` is capped at
`MAX_PER_CONN`, folds a re-run statement onto its newest run, deletes the runs a batch never
dispatched, leaves a credential-bearing statement out altogether and has a *Clear* — all right
for a recall list, and each a way for a record to lose a write. **Nothing in Schemaic
truncates, rewrites, prunes or rotates this file**: deleting a connection leaves its lines,
and there is no *Clear*, because a record the tool can quietly drop is not one. It is the
user's file to archive or delete, and the next write starts a fresh one. The cost is named
rather than hidden: it grows without bound, and a Run All logs one line per writing statement
with the statement in it. Nothing in the app reads it back; `a_line_reads_back_as_the_entry`
holds a line to the `Entry` it came from.
**Each entry is opened, written and closed.** `append` resolves `persist::config_dir`, opens
the file through `persist::open_private_append` (0600 on Unix, the tracing log's reason) and
writes the whole line in one `write_all`, so the GUI and a `schemaic exec` in another process
appending at once do not interleave inside a line. Its error is the caller's to report or drop
and never to turn into the write failing: the write it records has already happened.
`Entry { ts, source, connection, database, sql, detail, rows, outcome, error, masked }` — `ts`
in Unix milliseconds, `Source` one of `editor`/`script`/`grid`/`ddl`/`import`/`copy`/`cli`/
`restore`, `connection` the name the user saved. `Outcome` is `ok`/`failed`/`cancelled` and
**`unknown`, for a write the client lost track of** — `schemaic exec`'s deadline passing after the
statement was sent, which may or may not have taken effect (exit 5, under `cli/exec.rs`), and
every *Restore from file*, which is logged as it launches and ends in the terminal's client,
out of Schemaic's sight (`launch::restore_log_entry`). `sql`
is empty where a path has no statement to name — an import, a restore, a file that ran to the
end — and
`detail` says what happened instead, in Schemaic's own words over a path and a table name, so
it is never masked. `now` stamps an entry (an empty database is `None`,
`an_empty_database_is_no_database`), `with_detail` adds the detail, and `ended` (`Ok(rows)` or
`Err(message)`) and `ended_as` (an outcome with no row count) say how it went.
**How a run ended is decided here, not in the front ends.** Each maps its own state onto `Ran`
(`Rows(Option<u64>)`, `Failed`, `Cancelled`, `Unknown`, `Pending`) in one thin match — the app's
`ran_of(&QueryState)`, the CLI's `exec_ran` — and `Entry::landed(ran, aborted_before, dialect)`
ends the entry, `None` for `Pending`, which has no verdict to record
(`each_way_a_run_ends_maps_to_its_outcome`, `an_indeterminate_run_is_unknown`). These decisions
lived in an app closure and a CLI function, untested, until a typed `COMMIT` of an aborted
PostgreSQL transaction was logged `ok`: PostgreSQL answers a `COMMIT` of an aborted block with a
`ROLLBACK` it reports as a success. So **a `COMMIT`/`END` that "succeeded" with
`aborted_before`** — the tab's `tx::TxState::Poisoned` when it ran — **is logged `failed`**, *the
transaction was already aborted, so COMMIT rolled it back — nothing was committed*: the Commit
button's `aborted_commit` refusal, for the statement typed by hand
(`a_commit_typed_into_an_aborted_transaction_is_logged_failed`). **`PREPARE TRANSACTION` is
the third spelling** — measured on PG 16, a rollback answered with success and the tag
`ROLLBACK` — and is logged the same way, naming itself
(`a_prepare_typed_into_an_aborted_transaction_is_logged_failed`). **`batch_aborted(start,
engine, steps)` is each batch statement's `aborted_before`**: the state *ahead* of it, folded
in order from the tab's at launch, a statement with no outcome moving nothing — so a batch that
poisons its transaction and then commits logs the `COMMIT` as the rollback it is
(`a_batch_reads_each_statement_against_the_state_before_it`; the app folded it inline, where no
test saw the order). `batch_logged(ran, stopped)`
is which statements of a Run All get a line: a `Cancelled` one only if it was in flight at Stop
— the first without `Rows`, after the run of successes — since a batch reports every statement
after a failure or a stop as cancelled without having dispatched it, and a statement already
sent is not unsent; `Pending` none
(`a_stopped_batch_logs_the_in_flight_statement_and_not_the_undispatched`,
`a_failed_batch_logs_nothing_after_the_failure`).
**`abandoned(database, why)` is the end line for a transaction nobody ended.** A Manual tab's
statements are logged `ok` as they run, and its `COMMIT` or `ROLLBACK` is the only line that
says whether they stayed — so when Schemaic itself drops an open transaction's session, its
connection edited or deleted, the log used to keep those writes as `ok` with nothing after
them. The entry is a `ROLLBACK`, `Source::Editor`, outcome `ok`, `why` as its detail: the
rollback the server makes on the disconnect, in Schemaic's words for why it happened
(`a_dropped_transaction_is_logged_as_the_rollback_the_server_made`).
**Values are masked by default, because the SQL is the user's data.** An `UPDATE … SET email =
'ann@example.com'` carries the very value a log kept for years should not hold in the clear.
`line(entry, dialect, keep_values)` is the entry as the file holds it: unless `keep_values`
(`UiState::write_log_values`, off by default), `sql` goes through `mask_values` and `error`
through `prompt::redact_engine_error`, which strips the values a uniqueness or not-null report
quotes back. **A credential is masked whatever the setting says**: a statement
`sql::carries_credential` flags is masked under `keep_values` too, where `history.rs` drops it
altogether (`keeping_values_keeps_them_but_never_a_credentials`). `masked` is written on every
line, so a reader can tell a `?` that was a value from the statement's own placeholder
(`a_line_masks_by_default_and_says_so`).
`mask_values` replaces every string literal — its prefix or introducer with it (`N'…'`, `E'…'`,
`X'…'`, `_utf8mb4'…'`), and a double-quoted string where the dialect makes one a string rather
than a name — and every numeric literal with `?`, and **drops comments, since a comment is
prose the user may have put a value in**. Identifiers, keywords and bind placeholders (`?`,
`$1`, `:name`, `@p1`) stay, so which table, which columns and which `WHERE` remain readable. A
prefix is only a lone `E`/`N`/`X`/`B`/`U` or an `_`-led word directly before the quote; any
other word stays and the literal after it becomes the `?`. **A dollar-quoted body is masked as
code, not swallowed** — it is a function's body, so its literals go and its shape stays
(`a_dollar_quoted_body_is_masked_as_code_not_swallowed`). Boundaries come from
`sql::skip_noncode` and `skip_comment`, the one boundary lexer, so a quote inside a bracketed
`[o'k]` name or a comment opens no "string" (`placeholders_and_quoted_names_are_left_alone`).
`grid_sql(write, dialect)` is a grid commit as the log names it: each staged step in
`GridWrite::plan`'s order — the executor's, deletes then updates then inserts — as a statement
with its values written in through `export::sql_literal` and its names through
`export::ident_sql`, a NULL key column as `IS NULL`, a column set to its default as `DEFAULT`,
and **a byte value as `<binary, N bytes>`, never its bytes, even with values kept**. It is the
record's spelling, not what went over the wire — the executor binds its values
(`a_grid_commit_is_named_step_by_step_in_the_executors_order`); a PostgreSQL step is qualified
by its schema, a MySQL one by its database (`a_postgres_grid_write_is_qualified_by_its_schema`).
**What counts as a write is the caller's path, not this module's**: the editor and `exec` log a
statement `is_logged` answers for, and the grid commit, the DDL preview, the import and a `.sql`
file run log whatever they ran. `is_logged` is `sql::contains_write` **or** a statement
`sql::read_only_reason` will not call a read — because `SELECT archive_and_purge(2023)` deletes
rows behind a read head, PostgreSQL's usual way to write through a function, and
`contains_write` fails open on it where the read-only gate's PostgreSQL and SQL Server function
allowlists fail closed (`a_write_through_a_function_call_is_logged`). Logging a read is the
acceptable direction; missing a write is not. **It does not cover MySQL/MariaDB or SQLite**,
whose gate answers a call with a keyword deny-list rather than an allowlist
(`sql::read_call_policy` is `None` for both): a `SELECT my_stored_fn()` that writes still reads
as a read and gets no line. Closing it is an open design decision, not an oversight — a
catalogue-aware "is this a user-defined routine" check, or a line for every `SELECT` that calls
anything not built in — and neither has been taken. Where each
path hooks in, and what is deliberately not logged, is under `app/main.rs` and `cli/run.rs`.
