# `cli/query.rs`

`cli/query.rs` — **the one headless read path, and both non-GUI front ends are on it**:
`schemaic query` and `schemaic explain`, and the MCP server's `run_query` and `explain` — and `tables` and `describe`, whose canned SQL
(`catalog.rs`) is a query the user did not type — and `schemaic export`, whose uncapped
`read_only_export` sits beside `read_only_query` behind the same gate (below). Written twice they would have had every
reason to drift — same gate, same timeout, same normalisation, and only one of them getting the
next fix. The gate is
`core::sql::read_only_reason` and is strictly stronger than the editor's `run_verdict`: an
allowlist of read-only statement *heads* per dialect plus a deny-list of keywords anywhere —
and, on PostgreSQL and SQL Server, an allowlist of the functions a statement may call — with no
confirm arm to say yes to, which is the right shape when there is nobody at the keyboard.
**It is not what makes the statement read-only**, and this entry used to say it was: a head is a
spelling, and `SELECT setval(…)` or a `SELECT` of a function that deletes passed it and wrote —
PostgreSQL's function allowlist refuses both there now, but not a `SELECT … INTO newtable`, a
view's body, or a function on MySQL/MariaDB (`core::sql`'s entry). So
the statement runs through `Db::fetch_query_enforced` with `Enforce::ReadOnly`, a session that
refuses a write by its effect — and on MySQL/MariaDB one whose `sql_mode` is pinned to the lexer
the gate counted statements with — while the gate stays in front for what such a session still
allows: sleeps, locks, server-side file reads, PostgreSQL's non-transactional writers
(`core::sql`'s entry). **Nothing behavioural pins the session half**: changing
`read_only_query`'s `Enforce::ReadOnly` to `Enforce::None` passed the whole suite, because
in-memory SQLite has no statement that passes its gate and writes. So
`ui/source_gate.rs`'s `every_headless_read_runs_on_a_read_only_session` reads this file and
`mcp.rs` instead (and `dump.rs`, whose generated reads meet no text gate at all) — forbidding
plain `.fetch_query(` and `.stream_query(` in each, and requiring
`Enforce::ReadOnly` inside the argument list of every `.fetch_query_enforced(` and
`.stream_query_enforced(` call, both needles counted toward its floor. A **locking read** (`FOR UPDATE`, `FOR NO KEY
UPDATE`, `LOCK IN SHARE MODE`, and the shared `FOR SHARE`/`FOR KEY SHARE` no deny-list word
catches, which a MySQL 8 read-only session runs, locks held) is refused by naming the clause and
the lock (`sql::locking_clause`, `shared_locks`); it used to answer "`UPDATE` is not permitted",
which read as a write found (`a_locking_read_is_refused_by_naming_its_locking_clause`). A lock
is named only when nothing else in the statement is refused: a locking CTE ahead of a deleting
one was told to drop the clause, and the retry was refused as the `DELETE`
(`a_write_hidden_in_a_read_keeps_its_own_refusal`). **`sql::read_only_refusal` carries whether
the refusal is about a write** (`ReadRefusal::writes`), and `gate` sorts it into
`NoRows::NotARead`, whose message points at `exec`, or `NoRows::NotPermitted` — a lock, a
sleep, a second statement — whose message does not: the CLI answered a `FOR UPDATE` with "use
`exec` to write" after core's own words said it was no write
(`refusing_what_is_no_write_does_not_name_exec`). Both exit 3. **A third refusal is the
session's, not the text's**: a read-only PostgreSQL session refuses to open on a path where a
schema holds what PostgreSQL could pick in a builtin's place (`DbRefusal::ShadowedBuiltin`, under
`schemaic-db`'s `Enforce::ReadOnly`), and `NoRows::from_db` sorts that one `DbError` into
`NoRows::SessionRefused`, carrying the refusal's own sentence, and every other into `Failed` —
other `DbRefusal`s included (`a_shadowed_builtin_is_a_refusal_and_everything_else_a_failure`).
`read_only_query`, `read_only_explain` and `read_only_export` all go through it — the export asks
for the refusal *before* it stringifies the read's error for `export_verdict`, and removes the
`.part`, which is empty since nothing was read. `is_refusal` answers `true` for it and `run.rs`
gives it exit 3, not 4, though it has logged in to learn it: the statement never ran, and a retry
unchanged refuses again, which is the thing a caller branches on 3 to know
(`a_guard_refusal_a_server_failure_and_an_unknown_write_exit_differently`). So `Exit::Refused`'s
doc and the `--help` exit line now say that one refusal comes after connecting, before the
statement is sent — 3's "nothing was sent" promise (`run.rs`'s entry) holds for every other
refusal. **`NotARead`'s message names no
subcommand but `exec`** — "Only reads are taken here" — because `query`, `explain` and `export`
all refuse through it, and a message that said `query` told the other two's callers they had
typed it. It is asked in the **connection's own**
dialect, so a PostgreSQL `#` operator is not lexed as a comment on the way in. `normalize_stmt`
is pure and owns the trailing-`;` rule — a person types the semicolon out of habit and a
`SELECT 1;` answered "empty query" would be a baffling way to learn it was unwanted — and
`NoRows::is_refusal` separates the guard's answer from the server's, because a caller that
cannot tell "you asked for the wrong thing" from "the database is down" retries the one that
will never succeed. **`gate(sql, dialect)` is the text half of that guard, public so a front end
can ask it before connecting**: `run.rs` used to connect first, so a refusal — exit 3, whose
promise is that nothing reached a server — had already read the keyring and logged in over SSH.
`read_only_query` asks it again, in the connection's own dialect, so asking early is never the
only check. `DEFAULT_TIMEOUT` is 30 s — the CLI's default and the MCP server's fixed
value, moved by `schemaic query --timeout` — and both read it from here rather than restating
it: `args.rs`'s defaults through `DEFAULT_TIMEOUT_SECS`, `mcp.rs`'s `QUERY_TIMEOUT` as the
constant itself. The read goes through `deadline::with_deadline`. `NoRows::Indeterminate` is
the one arm a read never produces — it is `exec`'s, below. The row cap bounds what is held and
what is returned, and `ResultSet::truncated` is the front end's to pass on — a caller silently
told a capped result was the whole answer has been given a wrong answer by a command that
succeeded.
**`read_only_explain` is the plan of a read, behind the same `gate`**, then `Db::explain` under
`deadline::with_deadline`, parsed by `core::plan::QueryPlan::from_result` — the MCP server's
`explain` tool and `schemaic explain`, one function for both. Gated although a plan executes nothing, because the planner is not inert —
PostgreSQL folds an `IMMUTABLE` function's call at plan time, and nothing stops such a function
writing — and because a statement `run_query` would refuse to run has no reason to be planned.
**Only ever the plain form**: `analyze` is `false`, since the analysing form executes the
statement to measure it, inside a rollback a MyISAM table or a DDL statement does not honour
(`Db::explain`'s doc). **`read_only` is `true`, and on the plain form it is a read-only
session**, as `read_only_query`'s `Enforce::ReadOnly` is: the text gate does not see into a
view's body (above), or into what a function the planner calls goes on to call, and a plain
`EXPLAIN` emptied a table that way on PostgreSQL and on MariaDB (`schemaic-db`'s entry has the
probes, and `a_read_only_plan_refuses_a_write_its_planner_makes`). The plan of a write is
therefore refused outright on MySQL/MariaDB — which the gate refuses first anyway — and SQL
Server's `SHOWPLAN_XML` executes nothing and needs no session. Both `bool`s are positional, which is what a later edit
flips without anything failing (SQLite, the one engine the suite can reach, ignores `analyze`),
so `a_headless_plan_never_executes_the_statement` pins them in source (`ui/source_gate.rs`).
**`read_only_export(db, database, sql, format, path, timeout)` is `read_only_query` without the
cap, into a file** — `schemaic export`'s read, returning `Exported { tally }`. Same `gate`,
asked again here for the reason `read_only_query` asks it, and the same session half: the rows
come through `Db::stream_query_enforced` with `Some(Enforce::ReadOnly)`, `EXPORT_CHUNK_ROWS`
(10,000, the app's whole-result export's figure) at a time, down a channel two chunks deep, the
read inside `deadline::with_deadline` as every `db.` read in this crate must be. The other end
is a `spawn_blocking` writer over a `BufWriter` on `export::part_file(path)`, rendering through
`PullChunks` and `ExportFormat::stream_to` — so memory is a chunk or two rather than the result,
and every byte is core's emitter's, as `format.rs`'s are. **A closed channel is an end of stream
only while the token is not cancelled**: after the deadline cancels it, `PullChunks` is handed
an error rather than `Ok(None)`, the renderers reading `None` as a finished result; an `Err`
chunk — `stream_query_enforced` sends its failure down the channel as well as returning it — is
an `io::Error` the same way, **marked** (`forwarded_read_failure`), so `writer_stop` reads the
writer's end as a `WriterStop::Read` echo rather than a `WriterStop::Write` of its own. The
`.part` is renamed onto `path` only once the read returned `Ok` and the writer finished; how it
ended otherwise is the pure `export_verdict(read, written, incremental, part)`. **A failed
write is told as itself** unless the deadline stopped the read: it drops its end of the
channel, and the reader's next send fails with its own "the export stopped reading", which is
true and useless — so the disk's error is what the user reads. A deadline says it was
exceeded; a failed read is the server's message, whatever the writer echoed. Unmarked, the
echo took the write arm and every failed read said *"could not write rows.csv.part: query
failed: relation "nope" does not exist"*, and the test that pinned the read arm fed the writer
a tally the composition never produces; it now builds the echo through `writer_stop`. Each
names the `.part` only where it holds rows (`ExportFormat::writes_incrementally` — an XLSX
`.part` is empty until the end — and a `.part` that is still empty, which a read failing at
the connect leaves and which is then removed), as the app's export does
(`export_verdict_tests`). A rename that fails says the
export is complete in the `.part` rather than that it failed. `run_export` also refuses an
output whose folder does not exist, or that is itself a folder, as a usage error before
anything is read — `run.rs`'s `output_problem`, which `run_dump` shares; a folder used to get
through and fail only at the final rename, after the whole query had run
(`an_output_that_is_a_folder_is_the_command_lines_mistake`). Its one filesystem question,
`is_dir`, is a seam (`output_problem_with`), so the tests model the folders rather than stat
the checkout's, which the house rule's source-reading exemption does not cover. Verified live:
25,000 rows from PostgreSQL to CSV across three chunks, a `DELETE` refused with exit 3, and an
XLSX from SQL Server; the read itself has no unit test.
