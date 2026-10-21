# `cli/format.rs`

`cli/format.rs` — how a result reaches stdout, and **every emitter in it is `core::export`'s**.
Nothing here formats a cell; it chooses which of core's emitters to call and what to say about
truncation, so a duplicate column name, an embedded newline or a withheld blob reads the same
whether a row left through the Download menu or down a pipe. **With one deliberate difference:
CSV here has no formula guard.** `render_rows` calls `export::export_csv_plain`, RFC 4180
quoting and nothing else, where the file export prefixes `'` to a value a spreadsheet would
evaluate — right for a file a person opens in Excel, wrong down a pipe whose reader is a
program: `+15551234` came out `'+15551234`, a different phone number from the one the JSON of
the same cell gave (`csv_writes_a_formula_like_value_as_it_is`). **Every format tells a NULL
from an empty string**, which the table and the CSV used not to (review finding S3-L1-04):
`table` goes through `export::export_markdown_null_as(rs, order, "NULL", header)`, the `mysql`
client's spelling, with `''` left empty; CSV writes `''` as `""` and NULL as the bare empty
field, PostgreSQL's `COPY … CSV` convention; JSON always had `null` against `""`
(`the_table_tells_a_null_from_an_empty_string`, `csv_tells_a_null_from_an_empty_string`,
`json_tells_a_null_from_an_empty_string`). The table inherits the `mysql` client's ambiguity
along with its spelling: a string `'NULL'` prints the same. **`withheld_warning` says on
stderr what the machine formats cannot say in band**: json, jsonl and csv write a blob they
cannot carry as `null` or an empty field, and a column of those reads as "no data", so the
columns `export::withheld_columns` names are listed — after the rows, for `query` and for rows
`exec` returned alike — while `table` and `vertical` stay silent, their `<n bytes>` placeholder
already saying what it is (`a_withheld_blob_is_warned_about_in_every_machine_format`). The `lossy()` fixture —
a NULL, an empty string, a blob, a leading `+` and a repeated column name in one row — pins
what each format makes of the cells they disagree about, and
`json_and_jsonl_render_the_lossy_row_identically` that the two JSON shapes agree on all of them.
`Format` is `table` / `json` / `jsonl` / `csv` / `vertical`, parsed through `FromStr` so a bad
name fails at parse time naming the real ones instead of reaching a database and failing after the work is done; `table` is the default
whether or not stdout is a terminal, there being **no TTY auto-switch** to make a piped command
mean something different from the same command run by hand. `vertical` is
`export::export_vertical` with NULL as `NULL`, the table's spelling, and every `--format`
subcommand takes it. **`Output { format, header }` is what the row renderers take** —
`render_rows`, `truncation_warning` and `render_affected`, as `impl Into<Output>`, with
`From<Format>` giving the header, so every caller but `--no-header` still passes a bare
`Format`; `withheld_warning` takes `Format` alone, the header changing nothing it says.
`Output::new(format, no_header)` refuses the flag for `json`, `jsonl` and `vertical`, which
name every value (or every line) and so have no header to leave out — accepting it there and
changing nothing would be a promise the output does not keep
(`no_header_is_refused_where_there_is_no_header`). **A header-less `table` drops the
`row_count_note` footer too**, psql's `-t`, so `wc -l` and `while read` count records
(`without_a_header_the_table_and_csv_are_the_rows_alone`), and **a header-less write is the
bare count**, `3\n` for `table` and `csv` alike, for `n=$(schemaic exec …)`
(`a_headerless_write_reports_the_bare_count`).
**Only the two formats a person reads report truncation in band** —
`table` and `vertical`, the private `for_a_reader()`, which `withheld_warning` asks directly and
`truncation_warning` through `Output::reports_truncation_in_band()` — as the same
`row_count_note` footer (for `vertical` with no rows, just
`(0 rows)` with no blank line over nothing, `vertical_of_no_rows_is_just_the_count`;
`vertical_reports_a_cap_in_band_and_withholds_nothing`). **Because the footer is what carries
the cap, the in-band answer depends on the header too**: `reports_truncation_in_band` is
`for_a_reader() && header`, so a header-less `table` that was capped warns on stderr as a
machine format does — without that, `--no-header` would have removed the only word of the cap,
and a result cut short would have said nothing from a command that succeeded
(`a_headerless_table_reports_its_cap_on_stderr`). Asking `for_a_reader()` alone there, as it
used to be asked, is the tidy spelling that reintroduces the silence. The machine formats stay pure data, because a JSON array with a metadata
object in it or a CSV with a comment row is worse for every consumer than a clean stream plus a
line on stderr, which is what `truncation_warning` is for
(`a_capped_result_is_reported_in_band_for_table_and_on_stderr_otherwise` pins that it is said in
exactly one place). Issue #3's in-band alternative for JSON — a `"truncated": true`, or an
envelope around the rows — was declined for that reason and one more: an envelope changes the
shape every existing `--format json` consumer parses
(`a_capped_machine_format_still_emits_only_rows`). A caller reading stdout alone asks for
exit 6 instead, through `query --fail-on-cap`.
**`exec_truncation_warning` is the exception, on stderr in every format,
`table` included**: the footer's "more were available" is true and still misleading about a
write, where what the reader needs is that the cap applied to the display and the statement ran
in full — and it offers no `--limit`, which `exec` does not have
(`a_capped_exec_result_warns_that_the_statement_still_ran_in_full`). `newline_terminated` is the one thing this module adds to core's output —
`export` writes for *files*, where a trailing newline is noise, and a terminal wants one or the
next shell prompt lands on the last line of the data — and an empty rendering stays empty, since
a lone newline is not nothing to a reader counting lines. `render_affected` is purpose-built
rather than a row renderer because a write has no rows for `export`'s emitters to describe;
`vertical` gives the table's `(n rows affected)`.
