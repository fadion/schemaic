# `core/agent_log.rs`

`agent_log.rs` — **the agent log**: a record of every call an AI agent made to Schemaic's MCP
server — which tool, on which connection and database, with what SQL, and how it ended — one JSON
object a line in `<config_dir>/agent_log.jsonl` (`FILE`), beside the write log. Both ways into the
server write it, the AI panel's `--mcp-serve` and `schemaic mcp`; where a call is recorded, and
what of it, is under `cli/mcp.rs`. No setting turns it off, and nothing in the app reads it back.
**A separate file from the write log, in its format.** The write log is a record of writes, and
the MCP server writes nothing; what an agent *looked at* on a production connection is the
question this one answers, and mixing an agent's reads — many a minute — into the writes would
bury the record that file exists for. The lines are `write_log::Entry` with `Source::Agent`, a
variant the write log itself never holds, and they go through `write_log::masked`, not a masker of
their own: the values in a `run_query`'s SQL are masked as a write's are, a credential's whatever
the setting says, and *Keep values in the write log* (`UiState::write_log_values`) keeps them in
both files or in neither (`a_querys_values_are_masked_as_a_writes_are`).
**Masked, then cut — and the order is load-bearing.** `line_entry` (which `line` serialises)
masks first and only then cuts the SQL to `SQL_CHARS` (32 KiB) and the error to `ERROR_CHARS`
(500), each cut ending `… (N characters more)`. It used to be the other way round, the error cut
to 500 in `cli/mcp.rs` before it reached the masker, and PostgreSQL's redactor finds a rejected
value by its *closing* quote: a value long enough to lose that quote to the cut was written whole
up to the cut, under `"masked": true` (release review S5-L5-01,
`a_long_error_is_redacted_before_it_is_cut`). Kept values are cut all the same.
`entry(caller, connection, database, tool, sql, subject, refused_or_failed, rows)` builds the line.
`Caller` is `Panel` or `Standalone`, written into `detail` after the tool and its subject as
`(panel)` or `(mcp)` — `describe_table orders (panel)` — because the panel's assistant and an
outside client of the user's choosing are different answers to "who asked?". `sql` is the
statement the tool was given — `run_query`'s, `explain`'s or `propose_sql`'s, the last usually a
write the model drafted — and empty for one that takes none; `subject` is what else the call
named: a table, a search, or the other side of a comparison. An answered call is `ok`, with
`rows` the count of rows it showed the agent where it showed any — `run_query`, `sample_rows`,
`describe_table`'s samples — which is the row count the roadmap decided each line keeps, and
which had been dropped without a word (release review R2-L8-04,
`an_answer_with_rows_records_their_count`). `refused_or_failed` makes it `failed` with that text
as its `error` and no count — Schemaic's sentence or the engine's, **never rows**: a successful
answer is the data the agent was shown, and a copy of it in a file on disk is exactly what this
record must not become (`an_answered_call_names_its_tool_subject_and_caller`,
`a_query_keeps_its_sql_and_a_refusal_its_reason`).
**Capped, unlike the write log — and that is the trade.** The write log grows by the writes a
person chose to run; an agent calls a tool many times a minute, and nothing would ever clear this
file. Past `ROLL_OVER_BYTES` (8 MiB) `append` renames it to `ROLLED` (`agent_log.jsonl.1`),
replacing the previous one, and a fresh file starts: the most recent stretch is kept whole, and
anything older than two files' worth is gone — the price of a log that is always on.
`rolls_over(len, adding)` is the decision, and **an empty file never rolls**, however long the
line: a line larger than the cap would otherwise roll on every append and leave each file holding
only itself (`the_file_rolls_over_past_the_cap_and_never_when_empty`). That guard alone was not
enough while the SQL was uncapped: two agent-sized statements in a row rolled the file twice and
left both files holding only themselves, erasing the record. `SQL_CHARS` keeps any one line far
under the cap, so no single call can roll it (release review R3-L5-03,
`an_oversized_statement_is_cut_well_under_the_roll_over`). **A failed rename is ignored on
purpose** and the file grows past the cap, which is better than a call nobody recorded.
**Each line is one `write_all`**, through `persist::open_private_append`, as the write log's is, so
the panel's server and a `schemaic mcp` appending at once cannot interleave inside a line.
**The roll is taken under a lock, and the size measured again inside it** — `RollLock`, the file
`agent_log.jsonl.lock` created with `create_new` and removed on drop. Without it, two of those
processes seeing the full file at once each renamed it, the second renaming the fresh one-line
file over the 8 MiB one the first had just rolled (release review S5-L1-01). One that finds the
lock taken skips the roll and appends — whoever holds it is rolling, and one line past the cap is
no loss. A lock older than `STALE_LOCK` (60 s) is a crashed process's, a rename taking
milliseconds, and is taken over rather than leaving the log never to roll again
(`a_roll_lock_goes_stale_after_a_minute`, which pins only the age rule: the two-process race
itself has no test). `append`'s error is the caller's to drop: the call it records has already
been answered, and a log that could not be written must not turn into a failed tool call.
