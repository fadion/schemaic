# `cli/perf.rs`

`cli/perf.rs` — the performance reads as tables: what the MCP server's four performance tools
answer with (`cli/mcp.rs`), and the rows `schemaic statements`, `indexes`, `sizes` and `locks`
print (`cli/run.md`), over `core::workload`'s model. Every reply is a `ResultSet` built here
— `statements_result`, `index_health_result`, `missing_result`, `missing_with_ddl_result`,
`table_sizes_result`, `index_sizes_result`, `locks_result` — with **typed cells rather than pre-formatted strings**:
milliseconds a `Float` rounded to three places (enough to tell a 0.2 ms lookup from a 0.02 ms one
without printing float noise), counts and bytes `UInt`, a figure the source does not keep `Null`.
So a tool's text goes through `mcp.rs`'s `format_table` and the one capped pipe-table renderer
`run_query`'s rows take, not a second formatter. Each table leads with its figures and ends with
its text — `statements_result` puts `total_ms`, `mean_ms`, `calls`, `rows` and `database` before
`query`, the column `top_statements` renders wider (`STATEMENT_CHARS`, under `mcp.rs`) — and a
table is named `schema.table` where it has a namespace. The sizes carry both the byte count and
`stats::format_bytes`' reading of it; a finding's `detail` is `same as <keeper>` or `prefix of
<wider>`; a lock's `state` is `holds` or `waits`, with the blocking sessions joined.
**A suggestion's DDL is printed whole, below the table, and never in a cell.** `missing_result`
numbers the suggestions and `suggestion_ddl_text` prints each one's statements under the same
number, because a cell is cut at `MCP_CELL_CHARS` and a cut `CREATE INDEX` is no DDL at all
(`a_suggestions_ddl_is_given_whole_and_numbered`). **The CLI's table is the exception, and
carries the DDL as its last column**: `missing_with_ddl_result` is `missing_result`'s columns and
rows — the two share `missing_columns` and `missing_row`, so the numbering and the figures cannot
drift apart — plus `ddl`, each statement whole with its trailing newline trimmed, and `NULL` where
the table was not in the schema read rather than `""`. A CLI row is not cut as a tool cell is, and
a `--format json` reader wants the statement on its row, with no numbered text to match it
against (`the_cli_suggestion_table_carries_each_ddl_whole`). **Nor does the cell keep the editor
script's closing `GO`** (`without_closing_go`): it is sqlcmd's batch separator rather than T-SQL,
so a driver handed the cell would send it to the server as a word — the first live run on SQL
Server 2022 showed `…INCLUDE ([Total]);\nGO`. Only a last line that is `GO` alone is dropped; one
between batches is the script's own (`the_cli_ddl_cell_drops_the_scripts_closing_go`). `suggestions(schema, dialect, missing)` pairs
each `MissingIndex` with `workload::suggestion_ddl` against the table as `schema` read it, under a
`ddl::Target` of the dialect and the schema's flavour — the differ's statement, not one of this
file's. The table is found by name, and by namespace too when the DMV gave one; a table the
schema read no longer holds — dropped since the DMV saw it — gets no DDL, and its numbered line
says so rather than emitting against nothing
(`a_suggestion_for_a_table_the_schema_lacks_has_no_ddl`).
**What an absent source says is decided here too.** `unavailable_text` is
`Unavailable::reason()` and, where `enable_sql()` has a statement, "To turn it on, the user can
run: …" — addressed to the user through the model, which the tool's description tells to pass
the advice on rather than try it; nothing runs it
(`a_missing_source_says_why_and_offers_only_a_one_statement_fix`). `usage_note` is the caveat
every `index_health` answer ends with, chosen by `workload::usage_counted`: where usage was
counted, that the counts run from the last restart or statistics reset, so an index only a
monthly job uses can look unused; where nothing was, that no index can be called unused and only
duplicates and prefixes are shown (`the_usage_note_says_when_nothing_was_counted`).
**And how a `schemaic` performance command exits is decided here**, as a pure function of what its
read returned — `statements_outcome(report, dialect)`, `sizes_outcome(stats, dialect, database,
of_indexes, limit)` and `locks_outcome(list)`, each a `perf::Outcome`: the rows (`None` when there
is no answer), the stderr lines as `Note::Warn` and `Note::Hint`, and whether the server
`answered`, which `run.rs`'s `perf_exit` maps to exit 0 or 2. The contract is "exit 2 when the
server cannot answer, exit 0 when it answered empty": a statement source that is off is no answer,
with the statement that turns it on as a hint; one that has counted nothing is an empty answer
with a note; `sizes` on an engine that reports no sizes (`workload::reports_sizes`, DuckDB) is no
answer with or without `--indexes`, as is `--indexes` where indexes are sized per table — MySQL's
primary-key-only database included — while a database with no indexes is an empty answer; and an
empty lock list says which empty it is in the MCP tool's words. The decisions were inline in the
runners after a live `connect`, which is how the primary-key-only exit 0 went unseen
(`index_sizes_on_a_primary_key_only_mysql_database_are_no_answer`,
`sizes_on_duckdb_are_no_answer_and_say_why`,
`a_statement_source_that_is_off_is_no_answer_with_the_statement_that_turns_it_on`).
