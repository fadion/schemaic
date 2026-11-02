# `cli/args.rs`

`cli/args.rs` — the clap surface, kept separate from doing anything so that defaults, aliases
and which flag belongs to which subcommand are all testable without a database. `DEFAULT_LIMIT`
is **200, deliberately small**: the GUI's row cap is about what a grid can hold, this one is
about what a caller can sensibly receive down a pipe, and the caller is very often a language
model with a context window; a person who wants the whole table says so with `--limit`.
**SQL that opens with a `--` comment is moved behind a `--` separator before clap sees it**
(`comment_led_sql_last`, inside `args::parse_argv`, which is `run::main`'s whole front): clap
read `$'-- note\nSELECT 1'` as an unknown long option and exited 2, though a saved snippet very
often starts with a comment line. Either of two things tells the two apart — whitespace right
after `--`, never in an option (`--connection=Prod EU` included), or a line break anywhere in
the token, which catches PostgreSQL's and SQLite's `--TODO` and a `-----` banner — and a command
line with no such token comes back unchanged. The test drives `parse_argv`, not the pure half:
with only `comment_led_sql_last` pinned, its one call site could be deleted with the suite
green (`sql_led_by_a_line_comment_is_the_statement`, `nothing_but_a_comment_led_token_is_moved`).
`--limit` and `--timeout` refuse `0` at parse time — zero rows is a typo and zero seconds
cancels every statement before it runs — and `-d` goes through `non_blank`, because a blank is
what `-d "$DB"` sends with `DB` unset and it is not "no flag": PostgreSQL took it as a name and
connected to the database named after the user, the unscoped landing the exec guard's
no-database arm exists to stop, reached without the guard seeing it
(`a_blank_database_is_refused_at_parse_time`, `a_zero_timeout_or_limit_is_refused`).
**`-c` falls back to `SCHEMAIC_CONNECTION` and `-d` to `SCHEMAIC_DATABASE`** (clap's `env`; a
flag on the command line wins), on `query`, `explain`, `export`, `dump`, `exec`, `import`, `tables` and `describe` alike, `-c` on
`databases` and `ping`
(through `ConnArgs`), both on `mcp` (through `McpTarget`) and both on `compare`, whose
`--source-*` pair has no fallback (below), so an agent's shell is pointed at a connection and a database once. A `SCHEMAIC_DATABASE` that is set
but blank goes through the same `non_blank` and is refused, not read as unset — it is `-d "$DB"`
one step removed — which is why the message names both sources. The test reads the `env`
attribute off clap's `Command` rather than setting the variable, since the process environment
is shared by tests running in parallel
(`the_database_and_the_connection_can_come_from_the_environment`).
`--yes` exists on `exec` and **must not** exist on `query` — a read has nothing to consent to,
and accepting the flag there would teach the habit of passing it everywhere
(`query_has_no_yes_flag`). `--fail-on-cap` is the reverse, on `query` alone: exit 6 for a read
`--limit` capped (`run.rs` has why), **off by default**, since a person at a terminal reads the
footer and only a caller that asks should get a non-zero exit for rows read correctly
(`fail_on_cap_is_an_opt_in_query_flag`). `exec` has no `--limit` and its cap bounds only what
is printed, so there is nothing for the flag to report (`exec_has_no_fail_on_cap_flag`).
**`OutputArgs { format, no_header }` is flattened into every subcommand that prints rows** —
`list`, `databases`, `ping`, `tables`, `describe`, `query`, `explain` and `exec` — in place of a `format`
field of each one's own, so `--no-header` is on all of them at once
(`no_header_is_on_every_row_printing_subcommand`).
`OutputArgs::output()` is `format::Output::new`, and whether the pair goes together is judged
there rather than by clap; `Command::output_args()` hands it to `run.rs`, `None` for `version`,
`completions`, `mcp`, `export`, `dump`, `compare` and `import` — `export` and `dump` write their rows to a file, `export`
in a format of its own, `compare` prints a script rather than rows, and `import` prints nothing to
stdout at all, so none of the four has a stdout format to judge.
**`explain` is `query`'s inputs and nothing a read does not take**: `SqlArgs`, `Target` and
`OutputArgs` as `query` has them, comment-led SQL included, and `--timeout` at
`DEFAULT_TIMEOUT_SECS`. It has no `--yes`, for `query`'s reason; no `--limit`, the plan being
bounded by the server read's own cap rather than the caller's (`run.rs`); and **no
`--analyze`**, because a headless plan never runs the statement — the analysing form executes
it, and `a_headless_plan_never_executes_the_statement` pins the `false` on the call
(`query.rs`). `explain_takes_a_statement_a_target_and_the_output_flags` holds all three to
parse errors.
**`export` is `query` without the cap, into a file**: `SqlArgs` and `Target` as `query` takes
them, `-o`/`--output <PATH>` required, and a `--format` of its own — a bare `Option<String>`,
not `OutputArgs`'s `Format`, because the names are `core::export`'s six (`json`, `csv`, `sql`,
`md`, `html`, `xlsx`) rather than stdout's five, and `None` means the output's extension decides
(`run.rs` resolves it). It has no `--limit`, the whole result being the point, and its
`--timeout` defaults to `EXPORT_TIMEOUT_SECS`, an hour rather than `query`'s 30 s, since a whole
table can legitimately take that long to stream; it still refuses `0`.
**`dump` is the app's SQL dump, and takes no statement**: `Target`, `-o`/`--output <PATH>`
required, `-t`/`--table` repeatable — a display name, `orders` or `sales.orders` outside the
default schema (or `public.orders`, or a bare name one schema alone has — `run.rs` resolves
it), and every table and view when absent — and `--schema-only`/`--data-only`, which
`conflicts_with` makes a parse error together. Its `--timeout` is `export`'s
`EXPORT_TIMEOUT_SECS`. It has no `--format`, the file being `.sql` by definition, and is the
fourth `None` of `output_args()`.
**`compare` names two databases, and its flags are its own rather than a `Target`**: `-c` and
`-d` (`SCHEMAIC_CONNECTION`/`SCHEMAIC_DATABASE`, `-d` through `non_blank`) are the database the
script would change — the comparison's left side, where every other subcommand's `-c`/`-d`
point too — and `--source-connection` (defaulting to `-c`'s, in `run.rs`) and
`--source-database` (`non_blank`, defaulting to that connection's own) the one it should come
to match, so two databases on one server is the short form. `-t`/`--table` repeats, as
`dump`'s does. **No `--password-stdin`**: with two connections it would be unclear whose
password it was, so both sides come from the keyring. It takes no `--url` either. **No `OutputArgs`**, since what it prints
is a script, not rows. Its `--timeout` is `COMPARE_TIMEOUT_SECS`, 120 s, because two whole
catalogues are read and a large one takes longer than one statement's 30 s — `pub(crate)`, since
the MCP server's `compare_schemas` reads its catalogues under the same deadline (`cli/mcp.rs`)
(`compare_takes_a_target_a_source_and_tables`, which holds `--password-stdin` and `--format`
to parse errors and a blank `--source-database` to a validation error).
**`import` takes a file, a table and the reading flags, and no statement**: the positional `FILE`,
`-t`/`--table` required (one, unlike `dump`'s), a whole `Target`, and the flags `cli/import.rs`
turns into a `Reading` — `--format` (a bare `Option<String>`, as `export`'s, `None` meaning the
extension decides), `--no-header`, `--delimiter`, `--sheet`, `--null` (repeatable), `--trim` —
plus `--map FILE_COLUMN=TABLE_COLUMN` (repeatable, parsed by `import::parse_map`) and `--skip`
(repeatable). Which flag belongs to which format is judged by `import::read_config`, not by
clap. `--delimiter` is `parse_delimiter`: one ASCII character, or `tab` (or `\t`, either as typed
or as the real character), a literal tab being the separator a shell makes awkward to type; `"`
is refused (`a_delimiter_is_one_character_or_tab`). **No `--yes`**: typing `import` is the
caller saying this writes, as an `INSERT` through `exec` needs none, and a read-only connection
is refused outright in `run.rs`, which no flag could answer anyway. Its `--timeout` defaults to
`EXPORT_TIMEOUT_SECS`, an hour, the load being a whole file's rows, and still refuses `0`
(`import_takes_a_file_a_table_and_its_reading_flags`, which also holds a missing `-t` to a parse
error).
**The top-level `--help` ends with the formats and the exit codes** (`AFTER_HELP`, as `Cli`'s
`after_help`), since a script's author reads the help and would otherwise have had to find
either in the README; `OutputArgs::format`'s doc line names the five, so each subcommand's
help lists them where `--format` is. It is **a literal, checked against the code rather than
built from it**, because clap takes a `&'static str` there — `run.rs`'s tests hold it to `Exit`
and `Format::NAMES`. Those tests read the text by its shape, which is the part to keep when
editing it: an exit line is one whose first word is a number (the `\` continuation on 3 keeps
its tail on the same line), and a format line is one indented by two spaces, so the
`--no-header` and stdout/stderr lines under the formats sit at column 0 on purpose. **It is a
`concat!` of `\n`-terminated literals, one per line**, not one literal with its breaks typed in:
the columns are aligned with space runs, and a space run after a real line break is what
`source_gate`'s `no_string_literal_carries_a_wrapped_lines_indentation` reports — which it did,
the moment `schemaic-cli` joined that census for `mcp.rs`'s sake. A spelt `\n` is the gate's mark
of a block whose spacing is deliberate; the rendered help was checked byte-identical by diff
across the change.
`--password-stdin` is for the headless case the keyring cannot serve:
Linux's Secret Service needs an unlocked desktop collection, which an SSH session or a container
does not have, and without it the CLI would simply be unusable there. It reads a pipe and
nothing else: `run.rs` refuses it as a usage error when stdin is a terminal, where
`read_to_string` waited for an EOF nobody knew to type and the command simply hung — and does
not prompt instead, because a prompt would echo the password. What it reads becomes the
password through `run.rs`'s pure `piped_password`, which takes **one** leading U+FEFF and
**exactly one** trailing line ending off and nothing else: Windows PowerShell 5.1 pipes a
byte-order mark ahead of the text, and the server was sent U+FEFF as the password's first
character — "Access denied" for the right password — while the newline `echo` adds is the
pipe's but a second one may be the password's, which stripping every trailing CR/LF, as it
used to, took as well (`a_piped_password_loses_a_bom_and_one_line_ending`).
**The statement is `SqlArgs`, flattened into `query`, `explain`, `export` and `exec`: the positional or `-f`/`--file
<PATH>`, exactly one**, which `#[group(required = true, multiple = false)]` makes a parse error
(exit 2) rather than a precedence rule (`the_statement_can_come_from_a_file`,
`a_statement_and_a_file_together_are_refused`, `a_statement_or_a_file_is_required`).
`SqlArgs::source()` resolves it to `SqlSource::{Text, Stdin, File}`, and **`-`
(`SQL_FROM_STDIN`) means stdin in either place** — psql's `-f -`. Stdin is where a statement
carrying a password belongs: argv sits in the process list while the command runs and in the
shell's history afterwards, the exposure the app keeps such statements out of its own history
for, so an argv statement `sql::carries_credential` recognises is run but warned about on
stderr, and one from stdin or a file is not, being in neither — unless a `-p` value is what
put the credential there, whatever the statement's source (`run.rs`'s `credential_on_argv`
decides which part of argv it came from). Stdin has one reader, so stdin
(either spelling) together with `--password-stdin` is a usage error rather than a guess at which
came first. **A file is still one statement**, not a script, unless `exec --script` asks for one
(below): several hit the guards they would as an argument — `exec`'s `NotRun::Several`,
`query`'s gate — and only `--script` goes near `Db::run_script`. `run.rs`'s
`statement(&SqlArgs, …)` takes it; `read_sql_file` answers an
unreadable or non-UTF-8 file with a warning and `Exit::Usage` before anything is sent, and
**`sql_text` takes one leading U+FEFF off text read from stdin or a file**, for the reason
`piped_password` does: Notepad and Windows PowerShell 5.1 put a byte-order mark ahead of the
text, and `read_only_reason` refused `\u{FEFF}SELECT 1` as not a read — a file saying nothing
else came back refused. Only a *leading* mark goes; anywhere else it is data
(`a_statement_from_a_file_or_pipe_loses_its_bom`, which asserts the gate refuses the raw text
first). Both readers go through `read_source`, which takes them as parameters so the
composition is tested too (`a_file_or_pipe_source_reads_without_its_bom`). `--timeout`'s five `default_value_t`s read
`query::DEFAULT_TIMEOUT` through `DEFAULT_TIMEOUT_SECS`, not a second literal beside it, and so
does `exec`'s, which is an `Option<u64>` with no clap default (below)
(`exec_defaults_to_the_shared_timeout`).
**`exec --script` runs the input as a whole `.sql` script** — a migration, or a `schemaic dump`
being restored — through `run.rs`'s `run_script` rather than `statement`. It
`conflicts_with = "params"`, so `-p` beside it is a parse error: a script is run as written, and
substituting into a whole file is not something the app does either
(`a_script_takes_no_params`). Its help says it always needs `--yes` and is refused if the script
would switch database, and `--yes`'s own help says that with `--script` it answers the question
every script is asked. **Its `--timeout` default is an hour**, which is why `exec`'s `--timeout`
is `Option<u64>`: `exec_timeout(script, timeout)` resolves it — the given value whichever mode,
else `DEFAULT_TIMEOUT_SECS` for a statement and `EXPORT_TIMEOUT_SECS` for a script, since a dump
being restored is a whole database's worth of rows and thirty seconds would stop it a few tables
in (`a_script_defaults_to_an_hour_and_a_given_timeout_wins`). It still refuses `0`.
**`-p`/`--param` is `ParamArgs`, flattened beside `SqlArgs` into the same four commands, and kept
out of it on purpose**: `SqlArgs`'s group admits exactly one member, so a `-p` inside it would
make "a statement and a parameter" a parse error. It is the app's parameters bar on the command
line — a repeatable `NAME[:KIND]=VALUE`, or `NAME:null`, parsed by `parse_param` into a
`core::params::Binding`. The kinds are `text` (the default), `number`, `null` and `sql`, the last
being `ParamValue::Raw`, put in as written for `ORDER BY :col`. **The kind is spelled before the
`=`, never guessed from the value**: `42` is as often a zip code or an id kept as text as it is a
number, and a guess that emitted it bare would change what a `WHERE` matches; text is the default
because it is the one kind that cannot carry SQL, being quoted. Only the first `=` splits, so a
value keeps every later one and may be empty. A leading `:` on the name is accepted, since that is
how the statement spells it, and the name must be one the statement could spell —
`sql::is_word_start` then `is_word_byte`, the scanner's own definitions
(`a_param_is_text_unless_its_kind_says_otherwise`,
`a_params_value_keeps_every_later_equals_and_may_be_empty`,
`a_params_name_may_carry_the_statements_colon`, `a_malformed_param_is_refused`,
`each_statement_command_takes_repeated_params`). Parsing a `-p` decides nothing about the
statement: binding it, and every refusal that depends on the statement, is `run.rs`'s `bind`.
**`ConnArgs` is `Target` without the database** — `-c` or `--url`, `--read-only` and
`--password-stdin`, flattened into
`Target { conn, database }` for `query`, `explain`, `export`, `dump`, `exec`, `import`, `tables` and `describe`, and taken alone by the
two subcommands with
no database to run in: `databases` lists the names `-d` takes, and `ping` asks whether the
server answers at all, so neither has a `-d` and passing one is a parse error
(`databases_has_no_database_flag`, `ping_takes_a_connection_and_the_reachability_timeout`).
`run.rs`'s `select_conn` and `connect` take `&ConnArgs`, which is what lets every subcommand
above share them — and `compare` and `mcp`, which have no `ConnArgs` of their own, build one with
only `connection` set (`..Default::default()`, which `ConnArgs` derives for them) to use them
too, and `mcp` one more with only `url` set for its `--url` (below); `compare` takes no `--url`.
`mcp`'s also set **`stdin_is_protocol`**, the one field that is never a flag (`#[arg(skip)]`):
stdin there is the client's JSON-RPC stream, so `run.rs`'s refusal of a password-carrying URL
must not offer `--password-stdin` (`run.rs`'s `mcp_conn_args`).
**`--url` (`SCHEMAIC_URL`) is a connection given rather than saved**, for a container or a CI job
with no Schemaic setup — `postgresql://user@host/db`, `mysql://…`, `sqlserver://…`, `sqlite:path`,
whatever `core::conn_import::parse_url` reads. `-c` is `required_unless_present = "url"` and
`conflicts_with = "url"`, and **`ConnArgs::source()` is what decides, returning
`ConnSource::{Saved, Url}` or the sentence that says why not**. Exactly one source: **both is
refused, not ranked**, because either may arrive from the environment, and an exported variable
quietly beating a typed flag — or the other way round — is a write landing on a connection the
person at the keyboard did not name. `--read-only` beside a saved connection is refused there too:
a saved connection's read-only flag is the app's to set, and a CLI flag beside it would be a second
answer to the same question. That check lives in `source()` rather than in a clap `requires`
because, per the change's author, `requires` on the flattened flag was not enforced
(`read_only_needs_a_url`; the composition is `run.rs`'s
`select_conn_takes_a_url_or_a_name_but_not_both`). What `run.rs` does with a URL — refusing one
that carries a password, and never asking the keyring — is under `run.rs`.
**`ping`'s `--timeout` defaults to `PING_TIMEOUT_SECS`, not `DEFAULT_TIMEOUT_SECS`** — it is
`schemaic_db::PING_TIMEOUT`, five seconds, the bound after which the app's own health check
says *Disconnected*, so the CLI and the app agree on when a host is dead; the 30 s statement
default would have a script wait six times as long to be told the same thing.
**`tables` and `describe <table>` take a whole `Target`**, `-d` and `SCHEMAIC_DATABASE`
included, because what they list lives in a database — `catalog.rs` has which engines cannot
answer without one. `tables` has `--limit` at `DEFAULT_LIMIT`, like `query`; `describe` has
none, and passing one is a parse error — it names one table, so there is nothing to cap, and
`run.rs` holds it to a memory bound instead (`tables_takes_a_target_and_a_limit`,
`describe_takes_one_table_and_a_target`). Both default `--timeout` to `DEFAULT_TIMEOUT_SECS`.
**`mcp` takes `McpTarget` — `-c`, `--url`, `--ai-data` and `-d`, and deliberately no
`--password-stdin`.** Stdin is
the MCP client's JSON-RPC stream, so a password read from it would be the client's first
request; the flag is absent rather than present and refusing. `-c` and `-d` keep the
`SCHEMAIC_CONNECTION`/`SCHEMAIC_DATABASE` fallbacks and `-d` the same `non_blank`, and there are
no output flags, since nothing here prints rows
(`mcp_takes_a_connection_and_a_database_and_nothing_from_stdin`, which holds `--password-stdin`,
`--format` and `--limit` to parse errors). **`-c` repeats**: `connections` is a `Vec<String>`,
`required_unless_present = "url"`, in the order given, and serving several is `cli/mcp.rs`'s `serve_many`; the
first is the default for a tool call that names none, and `-d` is that first connection's alone.
The field was renamed from `connection` but the arg keeps `id = "connection"`, which is what
`SCHEMAIC_CONNECTION` and the env-lookup tests resolve through — the variable still supplies one
connection. The same test now parses `-c prod -c stage` into both, in order. The cost is the headless case `--password-stdin`
exists for: a connection whose password the keyring cannot serve has no `schemaic mcp` — unless
it is given as a `--url`, whose password comes from `SCHEMAIC_PASSWORD` instead.
**`--url` is one more connection, served after the `-c`s, not in place of them**
— so with any `-c` it is not the default — and `--ai-data schema-only|on-request|full`
(`parse_ai_data`) is its AI data level, the one field of a connection only `mcp` reads, which
`parse_url` leaves unset. `McpTarget` now derives `Default`, which is how `run.rs`'s tests build
one. Unlike `ConnArgs`, `-c` has no `conflicts_with = "url"`: both are served, neither ranked
above the other — **which is why `mcp`'s `--url` reads no `SCHEMAIC_URL`**: beside a typed `-c`,
an exported one would quietly add a connection to the agent's server, where a one-connection
command refuses the pair (`ConnArgs::source`). A URL served to an agent is typed into the client's
config or not served at all. **The same holds the other way round: an exported
`SCHEMAIC_CONNECTION` beside `--url` is refused** (`ArgumentConflict`, naming the variable), since
the variable did not give way to the URL as it does everywhere else — it became the server's
first, *default* connection, `-d` landing on it, and every tool call naming none read the
developer's own connection while the person believed the agent was on the URL's. clap has no way
to say "conflicts only when it came from the environment", so `parse_argv` is now `parse_with`,
which asks `refuse_an_exported_connection_beside_a_url` of the matches — `value_source` being
`EnvVariable` — before they become a `Cli`; a typed `-c` beside `--url` still serves both, as
asked, and the variable alone still serves (`mcp_refuses_an_exported_connection_beside_a_url`,
which points the arg's `env` at a variable no other test reads). The level's absence and
`--ai-data` without a `--url` are `run.rs`'s to settle (`mcp_takes_a_url_and_its_data_level`,
which also holds `mcp` with neither `-c` nor `--url` to a parse error).
**`wants_cli` is the routing predicate, and it is an allowlist of the *first* argument rather
than "are there any arguments".** This binary is re-invoked with argv by things that are not the
CLI — the Velopack installer and updater (`--veloapp-install`, `--veloapp-updated`,
`--veloapp-obsolete`, `--veloapp-uninstall`, `--veloapp-firstrun`) and the AI panel
(`--mcp-serve`, `--endpoint-file`) — and handing any of those to clap gets them rejected and the
process exited, breaking an install, an update or the assistant, each in a way that looks nothing
like a CLI bug. So a new flag on the app is safe by default: it has to be added here to reach the
CLI (`the_apps_own_flags_are_not_cli_invocations` names all seven, and
`a_subcommand_further_along_does_not_route` holds the *first*-argument half, since
`schemaic --mcp-serve list` is the app being asked to serve). The converse holds too: through the
app's own binary a new subcommand does not reach the CLI until its name is added here, as
`databases` and `ping` were, and `tables` and `describe` after them — the test was seen red
until both names went in (`every_subcommand_routes_to_the_cli`) — and `mcp`, which is not
the app's `--mcp-serve` above though both start the one server, and `export` after it,
`dump` after that, `explain` after that, `compare` after that, `completions` after that, and
`import` after that.
`schemaic.com` calls
`run::main` without asking, so a missing name shows everywhere except through it. **That test
and `the_help_text_names_every_subcommand` read the names off clap's `Command`**
(`subcommand_names()`, plus `help`) rather than a list of their own, which is what makes them
catch this: a hand list was one more place a new subcommand had to be added, and a subcommand
left off both passed. Taking `ping` out of `wants_cli` turns it red. **`version` is what a
missing name costs**: `--version`/`-V` were routed, but `schemaic version` was neither a
subcommand nor on this list, so through `schemaic.com` it was a usage error and through the
app's own binary it **opened the GUI** instead of answering. The test now names it, and was
watched fail before the name went in. `Command::Version` takes no arguments and prints
`version_text`, which is clap's own `render_version()`, so the subcommand and the flag are one
answer rather than two that agree today (`version_prints_what_the_flag_prints`).
**`completions <shell>` prints a shell completion script** for bash, zsh, fish, elvish or
powershell — the positional is `clap_complete::Shell`, so any other name, or none, is a parse
error (`completions_take_each_shell_by_its_name`). `completion_script` generates it from
`<Cli as CommandFactory>::command()` under the bin name `schemaic`: the parser itself rather than
a list beside it, so a subcommand or flag added to `Cli` is completed without being named a
second time — the same reason the routing test reads its names off clap's `Command`
(`every_completion_script_names_every_subcommand`). **It is static: commands and flags only, never
saved connection names.** Completing those would mean running a subprocess on every Tab press,
and that was deliberately not taken. Like `version` it touches no connection, which is why
`run.rs` answers it before the saved connections are read.
