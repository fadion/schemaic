# `cli/exec.rs`

`cli/exec.rs` — the write path, and the guard that is the only way into it. **A separate
subcommand, not a flag on `query`**: a flag would put the dangerous case one character from the
safe one, and it would give two paths one gate when they do not want the same gate — `query`
runs an allowlist with no override, while a write has to consult the connection's read-only
flag, the unsafe-statement warning (`sql::unsafe_reason` — a missing `WHERE`, a `TRUNCATE`, or
a statement that destroys stored rows with their object, listed under `core::sql`) and the
user's own say-so. `ExecRequest::approved` is the
guard and the only constructor; the fields are private and `run` takes one by value. Its
refusals `NotRun::Blocked`/`NeedsConsent` carry the verdict's `sql::GuardReason` as it came,
and `NotRun::message` says the reason's English `message()`. **The
request carries the target its verdict judged** — a clone of the `Connection` and the
database, read back through `connection()` and `database()` — and `run(db, request, timeout)`
takes no database of its own, running in `request.database()`, while `run.rs`'s `run_exec`
connects through `request.connection()`. When both were arguments beside the request, a second
caller could pair an approval judged against one connection with another connection or database
(`a_request_carries_the_connection_and_database_it_was_judged_against`). Its
reasoning, the `--yes` asymmetry and why several statements are refused rather than half-run are
under *Architecture invariants* with the other two minted requests. The one thing stated here
rather than there is `confirm_writes: false`: typing `exec` is the caller saying this writes,
and asking again on every bounded `UPDATE … WHERE id = 1` would mean `--yes` on essentially
every invocation — a flag that is always passed has stopped carrying information, including for
the unbounded write it would then also wave through. A read through `exec` is allowed and merely
pointless, because refusing it would need `exec` to grow a second gate deciding what a read is.
`run` bounds the statement through `deadline::with_deadline`, the same wrapper `query` uses.
**What it does not share with `query` is what a timeout means.** A read past its deadline was
cancelled and changed nothing, which `NoRows::TimedOut` says; a write past it *had been sent*,
and a cancel does not undo what a non-transactional engine already changed, nor a commit whose
reply was in flight. Live on MariaDB, a timed-out MyISAM `UPDATE` had changed 9 of 40 rows while
the command said it "was cancelled" and exit 4 told the script that retrying may work. So `run`
answers the deadline with `NoRows::Indeterminate`, whose message says the statement may have
been applied in whole or in part and to check before running it again, and `run.rs` gives it
exit 5. The MCP server folds it into its timeout arm, having no write that could produce it.
**It runs through `fetch_query_enforced`, never plain `fetch_query`**, with an `Enforce` that
`approved` mints into a private field from the connection the verdict judged: `ReadOnly` on a
`read_only` connection, because what the verdict lets through there is a read *to the text
check* and `SELECT setval(…)` is one of those; `AsJudged` otherwise, because the one-statement
count `approved` made was the gate's lexer's, and on MySQL only a pinned `sql_mode` makes the
server count the same (`a_read_only_connection_mints_a_read_only_session`,
`a_writable_connection_mints_a_session_that_lexes_like_the_gate`).
**It takes no row cap**: `RETURNED_ROW_CAP` is private and is `args::DEFAULT_LIMIT`, because the
`1` `run.rs` used to pass printed one row of a thousand-row `UPDATE … RETURNING` with no warning,
and a parameter would let a call site choose it again. The cap is on what is printed, never on
the statement — each engine's row loop stops reading and leaves the statement to finish.
