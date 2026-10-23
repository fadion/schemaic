# `cli/run.rs`

`cli/run.rs` — dispatch, and the module that owns the output contract. **stdout is data; stderr
is everything else** — no banners, no warnings, no progress — which is what makes `--format=json`
safe to parse and `--format=csv` safe to redirect. The other half is `Exit`, and it has **six**
outcomes rather than two: 0 ok, 2 usage (including a connection, `-f` file or `describe` table that is not there), 3 a guard
refusal, 4 a server or connection failure, 5 a **write** whose deadline passed after it was
sent, 6 a read `--limit` cut short, under `query --fail-on-cap` only. A new outcome gets a new
number, never an old one's. A caller that can only tell success from failure
retries the refusal that will never succeed and gives up on the timeout that would have;
`the_exit_codes_are_the_documented_ones` pins the numbers, because scripts depend on them.
**The top-level `--help` lists them** (`args.rs`'s `AFTER_HELP`), and
`the_help_lists_every_exit_code_and_no_other` holds the list to `every_exit()` — all six
variants, beside a nested `_exhaustive` `match` so a seventh fails to compile until it is added
there, and so to the help. `the_help_lists_every_format` does the same for the formats against
`Format::NAMES`, and checks `query --help` names each. Both were seen red before the section
existed.
**5 is the one a script must not retry blind**: 4 says retrying may work, and the timed-out
MyISAM `UPDATE` that got it had already changed 9 of 40 rows (`exec.rs` has the rest). Which
outcome gets which code is `exit_for_no_connection` and `exit_for_no_rows`, pure and tested
(`a_connection_not_exposed_is_a_refusal_and_a_missing_one_is_usage`,
`a_guard_refusal_a_server_failure_and_an_unknown_write_exit_differently`) rather than spelled
at each call site.
**6 answers issue #3**: a capped `--format json` read exited 0 with the only word of the cap on
stderr, so a script or agent reading stdout alone got a clean-looking answer that was silently
short — the issue's "there are only 200 companies". `exit_for_rows(rs, fail_on_cap)` gives it
only when `rs.truncated` and the flag are both true (`a_capped_read_exits_6_only_when_asked_to`,
`a_complete_read_exits_0_whatever_the_flag_says`), and `run_query` asks it *after* the rows
and the truncation warning are out: the rows are still printed, being correct and only
incomplete. **Not 4**, because nothing failed and retrying returns the same cap. The in-band
alternative the issue also offered is declined under `format.rs`.
Verified against PostgreSQL 16: `--limit 2 --format jsonl` on a 4078-row table exited 0
without the flag and 6 with it, rows and stderr warning printed both times; `table` with the
flag exited 6 under its capped footer; a complete result with the flag exited 0.
**3 promises that nothing was sent, and the order of the steps is what keeps that true** — with
one stated exception, a read-only PostgreSQL session refused after connecting and before the
statement went (`NoRows::SessionRefused`, `query.rs`'s entry), which `Exit::Refused`'s doc and the
`--help` line both name:
select the connection, take the statement, gate it (`query::gate`) or approve it
(`ExecRequest::approved`), and only then connect. It used to connect first, so a refusal had
already read the keyring and logged in over SSH, and a loop on a refusal made a real SSH login
each time round. That is why opening is two functions: `select_conn` reads no secret and dials
nothing, and `connect` is the half that does. `connect` hands back the `TunnelHandle` alongside
the `Db` because dropping it closes the tunnel, and it hydrates and patches a *clone* of the
**saved** connection, so the guard's subject is what the user configured — for `exec`, the
connection `ExecRequest` carries, so what is dialled is what was judged. **The tunnel is
bounded by `--timeout` in a `tokio::time::timeout` of its own**, because the statement's
deadline never covered it: an SSH endpoint that accepts TCP and never sends a banner held the
command until something killed it. Measured after the fix, an unreachable SSH host under
`--timeout 2` returned in 2.02 s. `ssh::open_tunnel` now bounds its own handshake too (20 s to
connect, 60 s to authenticate — the `ssh.rs` entry has why), so this wrapper is no longer the
only thing between a silent endpoint and a hung command. It stays: `--timeout` may be shorter
than either, and whichever bound is shorter answers first.
**Every byte of data goes through `emit`, never `print!`**, which panics when the write fails:
`schemaic list | head -3` exited 101 — no code the contract names — with a panic message on the
stderr an agent reads as the diagnosis. A `BrokenPipe` is a quiet `Exit::Ok`, the reader having
had what it wanted, and any other write error is `Exit::Failed`
(`a_closed_stdout_ends_quietly_and_any_other_write_error_fails`). The
tokio runtime is **current-thread and built here**, mirroring `--mcp-serve`: one statement on
one connection has no use for a pool and would pay its startup. **Both build it through
`block_on_own_stack`, on a scoped thread with `RUNTIME_STACK` (8 MiB), never on the main
thread**: `block_on` keeps the whole future on the calling thread's stack, and Windows gives the
main thread 1 MiB where Linux gives 8. In a debug build the MCP server's `list_schema` overflowed
it on every engine — the process died mid-call and the agent reported the server as
*connection closed* (reproduced against Azure SQL; the release build did not overflow on that
path) — and `schemaic version` overflowed too, since `dispatch`'s future carries every command's
state at once. The GUI never met it: its DB work runs on tokio's worker threads. A panic on the
runtime thread is resumed on the caller, so a bug still ends the process as it did before
(`a_future_larger_than_the_callers_stack_runs_on_its_own_thread`,
`a_panic_on_the_runtime_thread_reaches_the_caller`). `list` builds a `ResultSet` and
hands it to the same renderers a query's rows go through, so `--format=json` means the same
thing there as here and there is no second table-drawing path to keep in step. **Without
`--all` it says on stderr how many it left out**, not only when it left out everything: a
listing of three read as "these are my connections", and the fourth, with no CLI access, was
simply absent — the reader went looking for a typo. `list_note(total, shown, all)` is the pure
decision: silent under `--all` or when nothing was hidden, "K connection(s) without CLI access
not shown" pointing at `schemaic list --all` otherwise, and an empty list still no error but a
hint, since "none has CLI access yet" is the correct answer to *what may I use* and the
baffling one without it. **Nothing saved at all is a different sentence** — add one in
Schemaic, no `--all` advice — because the old single hint told a user with no connections to
turn CLI access on (`list_counts_what_it_hid`, `list_is_quiet_when_it_hid_nothing`,
`an_empty_list_says_whether_anything_is_saved`). stdout stays the rows alone. `database_for` is
the engine's implied database first (`schemaic_db::Engine::implied_database`, `main` on
SQLite), then the flag, then the connection's own through `Connection::default_database`, else
`None` — an empty string there would read as a real database name to the guard's `no_database`
arm, which is why the saved name is read blank-as-none (`a_blank_saved_database_is_none`).
*First*, not a fallback, is `mcp::Endpoint::standalone`'s rule for its reason: a SQLite file has
exactly one database, so a `-d` there names nothing else. It was the flag, else the raw
`conn.database`, so `dump` and `compare` on a SQLite file demanded a `-d` that any value
satisfied. `Engine::implied_database` is `Db::implied_database`'s answer asked of the engine
alone, and the `Db` method now delegates to it, because the CLI resolves its database before it
has connected. The rule itself is `default_database(flag, conn)`, factored out for `mcp` and
`compare`, whose `-d` is not in a `Target` (`mcp_defaults_to_the_connections_database`, which
covers the flag and the saved name; `a_sqlite_file_defaults_to_main` pins the implied-first
half, a `-d` and a stale saved name both losing to `main`).
**`version` is answered at the top of `dispatch`, before `load_connections_readonly` runs**: it
needs no saved connection, and a damaged `connections.json` must not stop anyone learning which
build they have — the first thing asked of someone reporting it damaged. Checked by hand with a
corrupt file: `schemaic version` printed `schemaic 0.27.1` and exited 0, `schemaic list` 4. The
`match` below keeps an `unreachable!` arm for it; folding `version` into that `match` like its
peers would look tidier and put the file read back in front of it. **Under the AI panel's agent
it is the only command that answers**: next, `agent_refusal(command, under_agent)` — pure,
`under_agent` being whether `schemaic_core::AGENT_CHILD_ENV` is set, as it is in every agent
child the panel spawns and so in whatever that agent's shell runs — refuses every other command
with a `warn` and `Exit::Refused`, before anything is read or dialled, because the assistant's
way to a connection is the panel's own tools at the level the user chose, and an agent shell
running `schemaic exec` or `query` would step past both
(`under_the_panels_agent_only_version_answers`, which holds `version` to answering and every
other command to refusing only when the variable is set). It is a speed bump and not a boundary —
a shell can unset the variable — and `ai/harness.md` says what does stand there. **The output
flags are judged next, still before the file is read**: `command.output_args()` resolved through
`OutputArgs::output`, and a pair that does not go together — `--no-header` with a format that
has no header — is a `warn` and `Exit::Usage` before anything is read, the way clap's own
refusals are. The resolved
`Output` is what `list`, `databases`, `ping`, `tables`, `describe`, `run_query`, `run_explain`,
`run_exec` and `emit_rows` take in place of
a `Format`. No test pins that order; it is the code's.
**`databases` lists the names `-d` takes, because a CLI user usually starts with none.** The
app never needs a default database — the tree lists every one — so a saved connection often
has none, and on MySQL 8.4 `schemaic query -c AEU "SELECT COUNT(*) FROM company"` failed with
the server's bare `ERROR 1046 (3D000): No database selected` and nothing pointing at `-d`
(issue #2). It is `Db::fetch_databases` — the names of the list the schema tree is built from
(`Db::list_databases`, less its single-user flag), system
schemas left out, though not the ones the user hid there — rendered like `list` through
`format::render_rows` as a one-column `database` result, so `--format` means what it does
elsewhere. It takes the same `select_conn` and `connect` as the others, and a failed connection
or listing is exit 4; `deadline.rs` has what its `--timeout` does and does not bound.
**`ping` is the app's health check, run once**: `select_conn` (so the CLI-access gate and exits
2 and 3 as everywhere), `connect` (keyring or `--password-stdin`, the tunnel under `--timeout`),
then `Db::ping` — log in, `SELECT 1` — under `deadline::with_deadline`. An answer is **one
row**, `ping_rows(conn, elapsed)`: `connection`, `engine`, `endpoint`, `status` (`ok`) and `ms`,
whole milliseconds as a `UInt` and timed around `Db::ping` alone, so its login is in the figure
and the tunnel is not. It goes through `format::render_rows`, so `--format` and `--no-header`
mean what they do for `list` (`a_ping_reports_the_connection_and_its_round_trip`). **No answer
is the driver's error on stderr and exit 4, and there is no "down" row**: a script tests the
exit code, and a person reads the driver's reason on stderr, where every other failure goes.
**`tables` and `describe` are `run_catalog`, which is `run_query`'s order with a statement the
user did not type**: `select_conn`, the dialect, `database_for`, the `catalog.rs` builder, then
`query::gate` asked early as `run_query` asks it, then `connect` and `read_only_query`. A canned
statement the gate refused would be this crate's bug, but it is still refused before a login. A
builder's `None` — MySQL or PostgreSQL with no database — is a `warn` that the subcommand "needs
a database to look in", the `Refused` arm of the no-database hint below, and `Exit::Usage`, with
nothing dialled. `tables` prints its rows and the truncation warning as `query` does.
`describe` runs under `DESCRIBE_CAP`, 32 768, a bound on memory rather than a cap any real
table meets (SQLite's compile-time column ceiling is 32 767), and **zero rows is "no table or
view named 'x' in <db>", exit 2** — the command named something that is not there, as a
connection that is not there is.
**`export` is `run_export`, `run_query`'s order with the format judged first.** The format is
the command line's to get right, so it is resolved before a connection is selected:
`ExportFormat::from_name(--format)`, else `ExportFormat::for_path(--output)`, and neither is a
`warn` naming the six — json, csv, sql, md, html, xlsx — and `Exit::Usage`, an extensionless
output with no `--format` included. Then `select_conn`, `statement`, `query::gate` in the
connection's dialect (a refusal is exit 3 with nothing sent, as for `query`), `connect` under
`query::DEFAULT_TIMEOUT` — the export's own `--timeout` bounds the read, not the login — and
`query::read_only_export`. **The one line of success goes to stderr** — `wrote N rows to
<path>` — since stdout is not where the rows went. A failure is `run_query`'s: the message, the
no-database `hint:` when it applies, and `exit_for_no_rows`.
**`explain` is `run_explain`, `run_query`'s order around the MCP server's `explain`**:
`select_conn`, `statement`, `query::gate` in the connection's dialect, `connect`, then
`query::read_only_explain` — the one function both front ends plan through, so the CLI's plan
is the tool's, on the same read-only session behind the same gate. Refusal is exit 3 with
nothing sent and a server failure or timeout exit 4, with the no-database `hint:`, all as
`query`. **The plan's rows go to stdout through the ordinary renderers**, by way of the pure
`plan_rows(&QueryPlan) -> ResultSet`, so `--format` and `--no-header` mean what they mean for
a query, and two details there are load-bearing. **The `ResultSet` is never marked
truncated**, even when the plan was: the renderers' capped wording points at `--limit`, which
`explain` does not have. **A cell reading exactly `NULL` becomes a real null** — `QueryPlan`
holds display text, and MySQL's empty `key` and `ref` came out of `--format json` as the string
`"NULL"`; the function's doc rests on no engine's plan writing that word as a whole cell for
anything else (`a_plan_prints_as_rows_in_any_format`). Everything else is `plan_notes`, on
stderr, since stdout is data: a `possible issue: …` line per finding of `core::plan`'s
heuristics, then a warning when the plan was cut by the server read's own cap
(`QueryPlan::truncated`) that the issues cover only the rows shown — never naming `--limit`
(`a_plans_findings_and_its_cut_go_to_stderr`). **A plan with no rows prints nothing to stdout**,
not an empty table, and says "the server returned no plan" on stderr, still exit 0
(`an_empty_plan_is_said`). Unlike the MCP tool there is no row or cell cap of this command's
own: the whole plan the server read returned is printed.
**`dump` is `run_dump`, and it is the app's Export ▸ SQL dump, not a second one** — it builds a
`core::dump::DumpRequest` and calls `dump::run`, the driver the modal calls (`cli/dump.rs`).
The usage errors come first, before anything is dialled: an output whose folder does not exist
or that is itself a folder, `output_problem` as `run_export` refuses it, then `select_conn`,
then a database — `database_for`, so SQLite's `main`, else `-d` or the connection's own, and
none is a `warn` that a dump needs one and `Exit::Usage`. Then
`connect` under `query::DEFAULT_TIMEOUT`. Then it always lists the database through
`Db::fetch_table_list`, under `deadline::with_deadline` and `query::DEFAULT_TIMEOUT` (a failed
or timed-out listing is `Exit::Failed`). **With no `-t` it names every table itself**, passing
all of their display names. That is the command's default and not the request's
— to `core::dump::plan` an empty list still means nothing was chosen, and the modal's empty
selection is unchanged; the order is not taken from the listing either, the driver's own fresh
introspection deciding it. **A `-t` name is resolved against that listing before it reaches
`plan`** — the pure `resolve_dump_tables`, the way `cli::compare::resolve` resolves `compare -t`:
by label (`schema::display_name`), else `schema.name` (PostgreSQL's `public.orders`, which the
label leaves bare), else a bare name only one schema has (SQL Server's `orders` for
`dbo.orders`), exactly or ignoring case where `DbSchema::names_ignore_case` — which SQL
Server's table list now carries (`mssql::names_ignore_case`, the collation probe the full read
asks too): it was always `false` there, so `dump -t Orders` failed on a `_CI_` database where
`compare -t` found it (`introspection_reads_the_schema_as_declared`, live). A name that is not
there, or one two schemas share, is a `warn` and `Exit::Usage` before any file is written.
`plan` used to take `-t` at its word, matching display names exactly: SQL Server's documented
`-t orders` and PostgreSQL's `-t public.orders` found nothing (exit 4, "Nothing to export"), and
one missing name beside a found one wrote a dump a table short with exit 0
(`a_dump_table_is_found_by_label_qualified_or_bare_name`,
`a_dump_table_that_is_not_there_or_is_ambiguous_is_refused`). The resolved name only *selects*
— it never reaches the SQL — and one that has gone between the listing and the driver's
introspection still comes back in `missing`. `--schema-only` and `--data-only` are
`DumpOptions::structure`/`data` turned off, and clap makes them a conflict. **The whole driver
runs under `with_deadline`**, the dump's own `--timeout` (an hour by default, `export`'s figure) rather than the login's, and its token is the
run's, so the deadline is a cancel the driver honours and not a dropped future. Progress is a
`dump::Progress` that prints `i of n — table` to stderr, a line per streamed table. A finished
dump prints `dump::done_note`, the modal's sentence in English, over `export::export_note`'s tally
clause, and exits 0 — unless `missing` is not empty, which `exit_for_dump_done` makes
`Exit::Failed`, since a file a table short looks like a whole one to a script reading `$?`
(`a_dump_that_left_a_named_table_out_does_not_exit_ok`); a failure prints
`export::export_failure_note` over `dump::ExportFailure`'s `Display`, naming the `.part` only where `partial` says one exists, and a deadline says it was exceeded and adds
`dump::cancel_note` — both `Exit::Failed`. **The deadline's note names the `.part` only where
there is one** (`dump_timeout_note`): the run's own `DumpOutcome::Cancelled { partial }` when it
unwound in time, and past the unwind grace, where `with_deadline` returns `None` and the run has
no answer, whether `dump::part_of(output)` exists on disk. It always passed `true`, so a dump
timed out during its schema read — before any file is created — sent the user to look for a
fragment that was never opened (`a_timed_out_dump_names_a_fragment_only_when_there_is_one`). It asks no read gate and runs no `Enforce` session:
every statement it sends is one `core::dump::plan` built, through the driver's plain
`fetch_schema` and `stream_query`, exactly as the modal's dump does. Verified through the built
CLI (per the change's author): on SQL Server 2022 `-t orders -t customers` wrote `dbo.orders`
and `sales.customers`, exit 0, and `-t orders -t nope` exited 2 with no file; on PostgreSQL
`-t public.zz_d_orders` dumped it.
**`compare` is `run_compare`, the app's *Compare with* without the window**, and every refusal
comes before anything is dialled. `CompareSides` carries the two databases as typed, the
source connection already defaulted to `-c`'s. Each side is `select_conn` over a `ConnArgs`
with `password_stdin: false` — so CLI access is asked of **both** connections, with exits 2
and 3 as everywhere — then `default_database` for each; a side with neither a flag nor a
connection default is a `warn` naming `-d` and `--source-database`, and `Exit::Usage`. **A
database compared with itself** — the same connection id and the same name — is `Exit::Usage`
too, the message naming the two flags that would make it a comparison. Then
`compare::comparable` over the two connections' dialects, so two engines are refused as the
app's overlay refuses them, before either round trip (exit 2). Only then are the two connected,
one after the other and each under the compare's own `--timeout` rather than
`query::DEFAULT_TIMEOUT`, and the two schemas read **concurrently** — `tokio::join!` over
`read_schema`, each `Db::fetch_schema` under `deadline::with_deadline` — a failure or a
deadline on either side being its `reading <db>: …` line and `Exit::Failed` (the target's,
when both fail). **A side naming a database that is not there is such a failure on every
engine** — on MySQL/MariaDB only since `mysql::require_database` (under `mysql.rs`'s
`collect_schema`), before which a typo read as an empty schema and the script dropped every
table in the target, exit 0. Verified through the built CLI against MariaDB 10.11 (per the
change's author): `reading shop_stagin: query failed: Unknown database 'shop_stagin'`, exit 4.
Then `SchemaComparison::of(&target, &source, dialect)`, the target's dialect,
and `cli::compare::report`, whose `Err` is a `warn` and `Exit::Usage`. **The script goes to
stdout, the notes after it to stderr**, and the exit is 0 whether or not the two differ.
`side_label` names each side by its
database, adding "on <connection>" only when the two sides are different connections.
**Nothing is run, so no guard is asked**: what reaches the servers is catalogue reads, as
`dump`'s are, and the plan leaves as text for a person — the app's preview and `Db::run_ddl`
stay the only way a comparison's plan is applied. Verified live on MariaDB, two databases on one
connection: a table created, one altered, one dropped and a view re-addressed into the target
by `as_read_from`; and the three refusals — no database, the same database, two engines — each
exiting 2. `run_compare` itself has no unit test.
**After a failure for want of a database, a `hint:` line on stderr names the way out** — `-d
<database>`, a default picked in Schemaic, and `schemaic databases -c <conn>` with the user's own
`-c` echoed back, quoted if it has whitespace (`shell_word`, which `mcp_notice` shares).
`hint` prints it on a line of its own after
`warn`'s, so the error line stays the driver's words, and no exit code changes. Whether a failure
is that one is `core::sql::no_database_failure(message, dialect)`, and `run.rs`'s
`no_database_failure` asks it only of a `NoRows::Failed` on a run `database_for` answered `None`,
because each half alone is wrong: an unscoped run fails for every other reason too, and on
PostgreSQL a scoped run's missing table reads exactly like the unscoped one
(`a_scoped_failure_gets_no_hint`). The engines say it differently and one does not say it.
MySQL/MariaDB refuse, `ERROR 1046 (3D000)`, matched on the code and SQLSTATE. PostgreSQL never
refuses — an unscoped connection lands in a maintenance database — so what it reports is
`relation "…" does not exist`, matched only where the message *is* about the relation, at its
start or after `: `, since `column "x" of relation "t" does not exist` names one that is there.
That match is on English text, because `DbError` carries no SQLSTATE for PostgreSQL: a server
with a translated `lc_messages` loses the hint, and nothing else. SQLite answers `None`, and the
`match` is exhaustive over the dialect, per the capability rule.
**`NoDatabaseFailure` has two arms because the hint must not say a statement ran when it did
not.** `RanElsewhere` adds that it ran in the server's maintenance database — without that,
"does not exist" reads as a typo in the table name — and `Refused` does not. The first cut keyed
that wording on the dialect, and so told `exec`'s refused PostgreSQL write it had run
(`only_a_statement_that_ran_is_told_where_it_ran`). That refusal is the guard's own
`Block(GuardReason::NoDatabase)`, reached through `needs_database`, and it gets the `Refused`
hint: `GuardReason::NoDatabase` is the one value `run_verdict` and `script_verdict` both block
with, and `refused_for_no_database` matches `NotRun::Blocked(GuardReason::NoDatabase)` by
variant. It used to keep that identity by comparing the text with
`core::sql::NO_DATABASE_SELECTED`, which is now only that variant's English
(`the_no_database_refusal_is_one_reason_on_both_paths`,
`the_guards_no_database_refusal_is_recognised_and_nothing_else_is`).
**An up-front refusal of an unscoped `query` was considered and rejected.** The CLI cannot tell
from the text whether a statement needs a database — `SELECT 1`, `SELECT … FROM app.t` and `SHOW
DATABASES` all run fine without one — and PostgreSQL never reports "no database" at all. So the
server decides and the CLI adds the hint; `exec`'s up-front `no_database` arm — a PostgreSQL
statement that `needs_database`, which no read is — is unchanged.
**`run_exec` appends to the write log the GUI keeps**, so `schemaic exec` is in the same record
as the editor (`core::write_log`). `log_entry` stamps a `Source::Cli` entry under the
connection's saved name only when `write_log::is_logged` answers for the statement, because
`exec` runs reads too and the log has no business holding them — though it errs toward a
write, a `SELECT` of a writing function included on PostgreSQL and SQL Server — not on the
MySQL family or SQLite (under `write_log.rs`). It is stamped after
`connect`, so a refusal or a connection that failed leaves no line, nothing having been sent.
`log_exec` ends it through `Entry::landed`, `exec::run`'s answer mapped by `exec_ran` — rows
affected, the message on a failure, and **`Unknown` for `NoRows::Indeterminate`** — exit 5's
statement that may have been applied in whole or in part — and for `TimedOut` alike
(`an_indeterminate_exec_is_unknown`). Its `aborted_before` is always `false`: `exec` holds no
transaction across runs, so nothing it runs is inside an aborted one. It reads `ui_state.json`'s `write_log_values` on every
run, so the one file follows the setting the user chose in the GUI, and an append that fails is
a warning on stderr that **leaves the exit status alone**: the status is the statement's, and
the write has happened either way.
**`mcp` is `serve_mcp`, and it keeps the order that makes exit 3 true.** `select_conn` first, so
`cli_access` gates it like every subcommand: a connection the user never exposed is exit 3 and
an unknown one exit 2, before the keyring is read, and the MCP client shows a server that failed
to start, with that line as its log, rather than one with a tool list
(`mcp_refuses_a_connection_without_cli_access`). Then `connect` under `query::DEFAULT_TIMEOUT`,
which is where an SSH tunnel is opened — **once, and held (`_tunnel`) for the server's
lifetime**; every tool call is still its own database connection, `Db` connecting per operation.
*Once* also means never reopened: after the tunnel drops, every tool call fails until the
client restarts the server — a known limit, left as it is. Then `mcp::Endpoint::standalone` over `default_database(-d, conn)` and the connection's
`ai_data`, one line on stderr, and `mcp::serve` until the client closes stdin — exit 0. **No
database connection is made before the first tool call**, deliberately: a database that is down
right now is an error the model sees on that call and may retry, not a server that never
started. stdout is the protocol's, so the line goes to stderr, which MCP clients keep as the
server's log, and it is `mcp_notice`: which connection is served and — when the row-reading
tools are withheld — that `run_query`, `explain` and `sample_rows` are, the level the connection
is at and the one that allows it, because a model that finds no query tool reports it as a bug
and the reason has to be in the log beside it (`the_mcp_notice_explains_a_withheld_query_tool`;
it named `run_query` alone, though `explain` was withheld with it) — and, when the endpoint has no database, that
the tools run where the server puts them unless a call names one, with `-d` and the `schemaic
databases -c <name>` that lists what to give it. An agent that does not know that sees
`relation does not exist` for a table `list_schema` just showed it
(`the_mcp_notice_says_when_there_is_no_default_database`). `connect` is handed a `ConnArgs` with
`password_stdin: false`, `mcp` having no such flag.
