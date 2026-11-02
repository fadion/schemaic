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
variant the write log itself never holds, and they go through `write_log::line`, not a masker of
their own: the values in a `run_query`'s SQL are masked as a write's are, a credential's whatever
the setting says, and *Keep values in the write log* (`UiState::write_log_values`) keeps them in
both files or in neither (`a_querys_values_are_masked_as_a_writes_are`).
`entry(caller, connection, database, tool, sql, subject, refused_or_failed)` builds the line.
`Caller` is `Panel` or `Standalone`, written into `detail` after the tool and its subject as
`(panel)` or `(mcp)` — `describe_table orders (panel)` — because the panel's assistant and an
outside client of the user's choosing are different answers to "who asked?". `sql` is the
statement the tool was given, `run_query`'s or `explain`'s, and empty for one that takes none;
`subject` is what else the call named, a table or a search. An answered call is `ok` with no row
count, and `refused_or_failed` makes it `failed` with that text as its `error` — Schemaic's
sentence or the engine's, **never rows**: a successful answer is the data the agent was shown,
and a copy of it in a file on disk is exactly what this record must not become
(`an_answered_call_names_its_tool_subject_and_caller`,
`a_query_keeps_its_sql_and_a_refusal_its_reason`).
**Capped, unlike the write log — and that is the trade.** The write log grows by the writes a
person chose to run; an agent calls a tool many times a minute, and nothing would ever clear this
file. Past `ROLL_OVER_BYTES` (8 MiB) `append` renames it to `ROLLED` (`agent_log.jsonl.1`),
replacing the previous one, and a fresh file starts: the most recent stretch is kept whole, and
anything older than two files' worth is gone — the price of a log that is always on.
`rolls_over(len, adding)` is the decision, and **an empty file never rolls**, however long the
line: a line larger than the cap would otherwise roll on every append and leave each file holding
only itself (`the_file_rolls_over_past_the_cap_and_never_when_empty`). **A failed rename is
ignored on purpose** and the file grows past the cap, which is better than a call nobody
recorded.
**Each line is one `write_all`**, through `persist::open_private_append`, as the write log's is, so
the panel's server and a `schemaic mcp` appending at once cannot interleave inside a line. The
size check and the rename before it take no lock between those processes; it is the line that is
kept whole, not the roll. `append`'s error is the caller's to drop: the call it records has already
been answered, and a log that could not be written must not turn into a failed tool call.
