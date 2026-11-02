# `cli/import.rs`

`cli/import.rs` — the decisions behind `schemaic import FILE -t TABLE` that need no file and no
server: how the file is read, and what the `--map`/`--skip` flags do to the mapping. It reads no
field and runs nothing. **The pipeline is the app's, not a second one**: the reading, the check and
the load are `core::import`'s `probe_sample`, `auto_map`, `validate` and `row_iter` and
`Db::import_rows`, driven by `run.rs`'s `run_import` (its entry has the order) as the import modal
drives them, so a file the modal would load the command loads the same way, with the same
coercions, the same server-assigned columns left out (`insert_columns`) and the same single
transaction. What this module adds is what the modal gets from a person looking at a preview.
**`read_config(file_name, &Reading, head)` is the modal's opening answer with every flag given
taking the place of a guess.** `Reading` is the flags. The format is `--format` (`csv`/`tsv`,
`json`/`jsonl`, `xlsx`/`excel`, case-insensitive), else `core::import::infer_format` on the
extension, else an error asking for `--format`. A CSV's delimiter is `core::import::sniff` over
`head` — the first 64 KiB, which `run_import` reads — unless `--delimiter` gives it. **The header
is the flag's and never the sniffer's**: `has_header = !no_header`, whatever `sniff` guessed.
`sniff` takes a numeric first row for data (`a_numeric_first_row_is_not_a_header`, in core), and
in the app that is safe because the modal shows what it decided and the user corrects it; with
nobody looking at a preview, a header row of years would have been imported as a row
(`a_csv_guesses_its_delimiter_but_never_its_header`, whose head is `2019;2020` over numbers).
`--null` tokens *replace* the default `NullRule` rather than adding to it, so the empty field is
NULL only when no `--null` is given (`nulls_default_to_the_empty_field_and_a_flag_replaces_them`).
**A flag for another format is refused, not ignored** — `--delimiter` or `--null` on a JSON or
Excel file, `--sheet` on anything but Excel — since a flag that silently did nothing would leave
the caller believing it had (`a_flag_for_another_format_is_refused`). `--trim` is not
format-bound and passes through to every format.
**`parse_map` is `--map`'s clap parser, `file_column=table_column`, split at the *last* `=`**: a
header may hold an `=` and a column name rarely does, so `a=b=c` maps the file's `a=b` to `c`; an
empty side either way is refused (`a_map_splits_at_its_last_equals`).
**`apply_overrides(mapping, file_columns, table, maps, skips)` corrects `auto_map`'s answer, never
replaces it**, so a file the app would map on its own needs none of the flags. A name in a flag is
matched the way `auto_map` matches a header — exactly, else trimmed and case-insensitive
(`position`) — so a flag reads a name as the automatic mapping reads it. **A flag naming
something that is not there is refused, never ignored**: a misspelt `--skip passwrod` that
quietly did nothing would import the very column it was written to keep out, and the refusal
names the columns that are there (`a_flag_naming_nothing_is_refused`). **A `--map` onto a table
column the automatic match already gave to another file column takes it**, the other file column
falling to `Skip`: the flag is the user's word and the match a guess
(`a_map_onto_a_matched_column_takes_it_from_the_match`). A `--map` onto a server-assigned column
(`ColumnInfo::is_server_assigned`) is refused, since the server fills it and `insert_columns`
would drop it anyway (`a_server_assigned_column_takes_no_mapping`); a column both mapped and
skipped is refused as a contradiction. **That check is asked of the resolved file column, not of
the two flags' text**: every `--map`'s file column is resolved first, so `--map Name=name --skip
name` — one column spelled two ways — is refused. Comparing the spellings let the skip quietly
undo the map, and `skipped` said nothing, the column being among the skips
(`a_flag_naming_nothing_is_refused` holds that case).
**`skipped(mapping, file_columns, skips)` is the sentence for every file column left out**, each
naming the `--map` and the `--skip` that would settle it, and `run_import` prints them on stderr
before the check: a column silently not imported is data lost without anyone deciding to lose it.
The ones the user `--skip`ped need no sentence and get none
(`an_unmatched_column_is_reported_with_the_flags_that_settle_it`,
`a_skip_leaves_a_matched_column_out_and_is_not_reported`). `issue_line` is one `core::import::Issue`
as a stderr line — `line N, column C: <IssueKind::message> ("text")`
(`an_issue_names_its_line_column_and_text`).
**`-t` is resolved by `dump -t`'s resolver, `run.rs`'s `resolve_tables`, not by the MCP
server's lenient `describe_table` lookup**: by label, else `schema.name`, else a bare name only
one schema has, ignoring case where `DbSchema::names_ignore_case`, and the resolved label is then
looked up with `DbSchema::find_by_display`. **A bare name two schemas share is refused** — "qualify
it" — because this command writes, and the lenient lookup would have loaded into `public`'s table,
else whichever schema the listing happened to have first: a wrong table, in silence.
