# `cli/deadline.rs`

`cli/deadline.rs` — one deadline, for every database read the headless front ends make.
`with_deadline(fut, token, timeout)` awaits `fut` for at most `timeout` and, on expiry, cancels
the token and **waits for the future to finish unwinding**. That last part is the whole reason
this is a function: the token goes down into the driver and the driver's cancel branch is what
issues the server-side `KILL`, so dropping the future at the deadline would return promptly and
leave the statement running on the server with nobody left to stop it. It was two
`tokio::select!` blocks, one in `query.rs` and one in `exec.rs`, and the second carried a comment
saying it made *"the same trade for the same reason"* as the first — a duplicated decision
admitting to being one, and a trade subtle enough that it should exist once. The token handed in
must be the one the future was given, or the cancel reaches nothing and the "deadline" returns
after the *query* rather than after `timeout`.
**The wait after the cancel is bounded, at `UNWIND_GRACE`** — `schemaic_db::CANCEL_TIMEOUT`
plus one second — after which the future is abandoned, because not every future is listening. A
driver still *connecting* has not reached its `select!` on the token, so a host that drops
packets held a `--timeout 2` command for the OS connect timeout, ~21 s, on both engines.
Abandoning is safe exactly there — before the statement has started there is nothing
server-side to `KILL` — and once it has started the driver's own `KILL` is bounded by
`CANCEL_TIMEOUT` and finishes inside the grace, so the await-past-the-cancel reasoning above
still holds for every statement that reached the server
(`a_future_that_ignores_the_cancel_is_abandoned_after_the_grace`). The worst case is
therefore `timeout` plus the grace, not `timeout`. The SSH tunnel is opened before this wrapper
is ever entered, so it has a bound of its own in `run.rs`.
**It is deliberately not a merge with `mcp.rs`'s `with_deadline`**, and the server moving into
this crate did not merge them: that one wraps a read none of the subcommands makes
(`fetch_schema`), at the server's fixed `QUERY_TIMEOUT` rather than a `--timeout`, and the
tokenless ones there (`fetch_databases`, `fetch_table_list`) go through
`with_deadline_abandoning` beside it. It does share the bound: its wait after the cancel is
`UNWIND_GRACE` too, for the same still-connecting driver.
**`schemaic databases` is one of two callers here whose token is not the future's.**
`fetch_databases` takes none, so `run.rs` hands `with_deadline` a fresh one and the cancel
reaches nothing — the case the paragraph above warns about. It is wrapped anyway because this
module's gate requires every `db.` read in the crate to be, and it is bounded regardless:
`fetch_databases` gives up at `PING_TIMEOUT`, five seconds, on every engine, inside the
`UNWIND_GRACE` the wait after the cancel allows. What that costs is `--timeout`'s meaning for
the listing — it still bounds the tunnel — where one past five seconds never fires and a
shorter one turns a late answer into a failure without returning any sooner.
**`schemaic ping` is the second, and there the two bounds are one.** `Db::ping` takes no token
either, but it takes the timeout, so `run.rs` passes `--timeout` to both it and the wrapper: the
check gives up by itself at the same moment the deadline would, and the wrap is there for the
gate rather than for a cancel it cannot deliver.
**The gate that produced this module is the part worth keeping.** `mcp.rs`'s
`no_database_read_is_awaited_without_a_deadline` (in `ui/source_gate.rs` now, `mcp.rs`'s entry
below has why) carries a floor on how many reads it finds, so
that a needle which stopped matching could not read as a clean file — and folding `run_query`
onto `query::read_only_query` moved one read out of that file, seven to six, into a crate no gate
was watching. So `no_database_read_in_this_crate_is_awaited_without_a_deadline` lives here and
asks the same thing of `query.rs`, `exec.rs` and `run.rs`, with its own floor of two. **Its
needle is every `db.<method>(` but `engine()`**, the one accessor that does no I/O — the reach
`mcp.rs`'s gate already had. It was `db.fetch_`, which a switch to `db.run_batch` or a new
`db.ping()` would have walked straight past; a scratch `db.ping()` in `run.rs` turns it red. Both are
**lexical** in the same way: the read must be an argument to a `with_deadline` call with no
statement boundary in between, which is what lets a call rustfmt has wrapped across lines still
count. The two unit tests beside it run on tokio's paused clock, and the crate declares `tokio`
with `test-util` in its own dev-dependencies rather than inheriting it — `cargo test --workspace`
had been unifying that feature in from `schemaic-app`, so `cargo test -p schemaic-cli` alone did
not compile.
