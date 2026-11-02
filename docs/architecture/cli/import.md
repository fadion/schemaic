# `cli/import.rs`

`cli/import.rs` — the decisions behind `schemaic import FILE -t TABLE` that need no server: how
the file is read, which columns it has, and what the `--map`/`--skip` flags do to the mapping. It
reads the file's column names and nothing else, and runs nothing. **The pipeline is the app's, not
a second one**: the reading, the check and the load are `core::import`'s `probe_sample` (a JSON
file's columns aside — `file_columns`, below), `auto_map`, `validate` and `row_iter` and
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
the caller believing it had (`a_flag_for_another_format_is_refused`). `--no-header` on JSON is
refused for a worse reason: a record's keys are its header, and without one `auto_map` paired
them with the table's columns *by position*, in their sorted order
(`no_header_is_refused_on_a_json_file`). `--trim` is not format-bound and passes through to every
format.
**`file_columns(open, format, cfg)` is the file's columns as the load will read them.** For JSON
that is `core::import::json_columns`, every key of every record; for CSV and Excel the header, or
the first row's width, from a one-row `probe_sample`. It was that one-row probe for every format,
and `trim_to_mapping` behind it dropped every JSON key the first record lacked — a null an export
left out — from every row, silently (`a_json_key_first_seen_after_record_one_is_imported`). Its
failure is a `FileProblem`: `Unreadable(ImportError)` for a file that cannot be parsed, and
`Empty` for one with no columns at all — a zero-byte CSV, a JSON `[]` — which says "<file> is
empty; there is nothing to import" rather than answering with a `--map` the next run would refuse
(`an_empty_file_is_said_to_be_empty`, `an_unparseable_file_is_a_file_problem`). Both are the
file's problem and not the command line's, which is why `run_import` ends them at exit 7, not 2.
**`plan_mapping(columns, format, cfg, table, maps, skips)` is every decision between reading the
file and checking it, in one place a test reaches** — they sat inline in `run_import`, which no
test drives. It tells `auto_map` `has_header || format == Json`, as the app's import view does (a
JSON file always has its keys), applies `apply_overrides`, refuses a mapping whose
`insert_columns` is empty with a sentence naming `--map`, and answers an `ImportPlan` — the
`mapping` and the `notes`, `skipped`'s sentences. The tests compose `read_config`, `file_columns`
and `plan_mapping` over in-memory bytes, as `run_import` does.
**`parse_map` is `--map`'s clap parser, `file_column=table_column`, split at the *last* `=`**: a
header may hold an `=` and a column name rarely does, so `a=b=c` maps the file's `a=b` to `c`; an
empty side either way is refused (`a_map_splits_at_its_last_equals`).
**`apply_overrides(mapping, file_columns, table, maps, skips)` corrects `auto_map`'s answer, never
replaces it**, so a file the app would map on its own needs none of the flags. A name in a flag is
matched the way `auto_map` matches a header — exactly, else trimmed and case-insensitive
(`position`) — so a flag reads a name as the automatic mapping reads it. **A flag naming
something that is not there is refused, never ignored**: a misspelt `--skip passwrod` that
quietly did nothing would import the very column it was written to keep out, and the refusal
names the columns that are there (`a_flag_naming_nothing_is_refused`). **So is a flag naming a
file column the file has more than once** — no flag can say which it means, and `position`
reaching the first, the matched one, made `--skip name` leave both out; the refusal gives their
column numbers and says to rename one in the file (`a_flag_naming_a_duplicated_column_is_refused`).
**A `--map` onto a table
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
**`skipped(auto, mapping, file_columns, table, skips)` is the sentence for every file column left
out**, each naming the `--map` and the `--skip` that would settle it, and `run_import` prints them
on stderr before the check: a column silently not imported is data lost without anyone deciding to
lose it. The ones the user `--skip`ped need no sentence and get none
(`an_unmatched_column_is_reported_with_the_flags_that_settle_it`,
`a_skip_leaves_a_matched_column_out_and_is_not_reported`). **It asks by position, not by name**:
one `--skip name` used to silence every column of that name. A name the file has twice is said as
"another column has the same name" with no flag advice, since `apply_overrides` would refuse the
flag it gave. And `auto`, the mapping before the flags, tells a column a `--map` displaced from one
nothing matched: that one says "its match 'x' was given to 'y' by --map", where it used to say no
table column matched it (`a_displaced_column_is_said_to_be_displaced`). `issue_line` is one `core::import::Issue`
as a stderr line — `line N, column C: <IssueKind::message> ("text")`
(`an_issue_names_its_line_column_and_text`).
**`-t` is resolved by `dump -t`'s resolver, `run.rs`'s `resolve_tables`, not by the MCP
server's lenient `describe_table` lookup**: by label, else `schema.name`, else a bare name only
one schema has, ignoring case where `DbSchema::names_ignore_case`, and the resolved label is then
looked up with `DbSchema::find_by_display`. **A bare name two schemas share is refused** — "qualify
it" — because this command writes, and the lenient lookup would have loaded into `public`'s table,
else whichever schema the listing happened to have first: a wrong table, in silence.
