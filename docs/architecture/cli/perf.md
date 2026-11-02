# `cli/perf.rs`

`cli/perf.rs` — the performance reads as tables: what the MCP server's four performance tools
answer with (`cli/mcp.rs`), over `core::workload`'s model. Every reply is a `ResultSet` built here
— `statements_result`, `index_health_result`, `missing_result`, `table_sizes_result`,
`index_sizes_result`, `locks_result` — with **typed cells rather than pre-formatted strings**:
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
(`a_suggestions_ddl_is_given_whole_and_numbered`). `suggestions(schema, dialect, missing)` pairs
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
