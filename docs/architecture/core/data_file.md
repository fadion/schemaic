# `core/data_file.rs`

`data_file.rs` — the words for **Open data file**: which DuckDB reader a path takes, the view's
name and its `CREATE`, and the query its tab opens with. The database they run in is
`db::duckdb::DataSession`, and the statement is run by `db::duckdb::open_data_file`, whose entry
under `db/duckdb.rs` has the session's life. **A view, not a copy**: the session holds a view over
DuckDB's own reader for the file (`read_csv`, `read_parquet`, `read_json` — `DataFileKind::reader`),
so opening a large file costs nothing until it is queried, the file stays the data, and nothing
written to the session reaches it. The reader is called with the path alone, so a delimiter, a
header and the column types are DuckDB's sniffer's guess; a `.tsv` is `Csv` for that reason, the
sniffer finding the tab, and `read_json` tells an array from newline-delimited JSON itself.

**The extension decides, and the dialog's filter is not the decision.** `kind_of` reads the file
name's extension without case — `csv`/`tsv`, `parquet`, `json`/`jsonl`/`ndjson` — and looks
through a trailing `.gz` or `.zst` for the text kinds alone: DuckDB's text readers undo that
compression themselves, while Parquet compresses inside the file, so `x.parquet.gz` is no file this
opens (`compression_is_looked_through_for_text_alone`). `EXTENSIONS`, the dialog's filter, carries
`gz` and `zst` bare, because a dialog filter matches the *last* extension alone and `x.csv.gz`
would otherwise not be offered; that lets `x.gz` through the dialog too, and `kind_of` has the last
word — `DbRefusal::DuckDbNotADataFile`. Only the file name is read, split on either separator, so a
directory named like a data file counts for nothing
(`a_directory_named_like_a_file_does_not_count`).

**The view is named for the file** (`view_name`): the stem with the data and compression
extensions off — inner dots and spaces kept, `a.b.parquet` → `a.b` — and `data` for a file with no
stem, made distinct from `taken` by `_2`, `_3`… compared without case, as DuckDB resolves a name
(`a_taken_name_gets_a_suffix_compared_without_case`). `taken` is what the session's `main` already
holds, read by `db::duckdb`'s `free_view_name`, so opening a second `sales.csv` adds `sales_2`
rather than replacing the first.

**Two quoters for the two readers, as the one-quoter invariant has it.** `create_view_sql` is
executed SQL, so the name goes through `export::ident_sql` and the path through
`export::sql_literal` — a Windows path's `\` kept and `o'brien` doubled
(`the_view_reads_the_file_with_its_path_as_a_literal`); `select_sql` is the tab's text, which the
user reads, so it quotes through `ident_if_needed` and `SELECT * FROM sales` stays bare
(`the_tab_selects_from_the_view_quoting_only_when_needed`). **`create_view_sql` is a plain `CREATE
VIEW` on purpose**: it is the one generated statement run without the preview, and only ever
creating — never `OR REPLACE` — is one of the terms that let it (*Generated DDL is never run
silently*, in the invariants).

**Where the sniffer guesses a CSV wrong, the delimiter and the header can be said** — the schema
tree's *CSV read options* (`ui/csv_options.rs`), which replaces the view's body and leaves the
statement to the view editor. `csv_view_body(path, options)` writes `SELECT * FROM
read_csv('path', delim = ';', header = false)` with only the options set, so
`CsvReadOptions::default()` — `Auto` on both — is the body `create_view_sql` opens with
(`a_csv_body_writes_the_options_set`). `CsvDelimiter` is a closed list — comma, semicolon, tab,
pipe, and `Auto` for the sniffer's choice — rather than free text, so no escaping question (a tab
typed as `\t`) reaches the user; `CsvHeader` is `Auto`, `Yes` or `No`. The column types stay the
sniffer's guess. `csv_view_of(body)` reads one back through `intel::star_from_table_function`, as
written and as DuckDB re-prints it — `(delim = ';'), ("header" = false)`, measured on 2.0.0 by
the change's author — taking `read_csv` or `read_csv_auto`, `delim` or `sep`, and `header`
(`a_csv_body_reads_back_written_or_reprinted`); a `read_csv_auto` body is written back as
`read_csv`. **It answers `None` for any body carrying what it does not model** — `columns = …`, a
two-character delimiter, a list of files, a filter, an alias, a projection
(`a_body_the_form_would_rewrite_lossily_is_not_read`) — and that is the gate, not a gap: the form
writes the whole call again, so a body it had half read would lose what the user wrote by hand,
and the menu entry is offered only where this answers `Some`.
