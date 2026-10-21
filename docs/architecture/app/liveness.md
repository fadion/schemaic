# `app/liveness.rs`

`liveness.rs` — **is the process that left this behind still running?**, asked once for the
whole app. Two unrelated sweeps asked it and answered it two different ways, each wrong in one
direction: `antigravity::sweep` read a pid marker and special-cased its own pid, while `ai`'s
temp sweep — which deletes MCP config and endpoint files, and those carried the database
password until the endpoint named its connection instead — answered with a 24-hour mtime, so a session older than a day plus a second window was enough
to delete a *live* instance's endpoint file, and abandoned siblings with the wrong suffix were
never collected at all. An `Owner` is a pid **and** the start time of the process that held it,
because pids are reissued: a crashed instance's claim eventually names some unrelated live
process, and anything trusting the number alone would decline to clean up for the rest of that
pid's life — turning a transient crash into a permanent standing grant, or a permanent plaintext
credential on disk (`a_reissued_pid_is_not_the_owner`). `me()` is this process, or `None` when
its start time cannot be read, because a claim that cannot be told from a pid-reuse cannot
expire and is worse than none — so callers treat that as "do not claim" rather than falling back
to the bare pid. `process_start` is the `sysinfo` lookup and the only impure part, kept to one
line of answer so `is_live` and `may_sweep` stay testable with no process to look at. An absent,
unreadable or garbled claim is **not** a claim
(`an_absent_or_unreadable_claim_never_blocks_a_sweep`): both callers have to be able to clean up
after a crash they cannot identify, and refusing to act on a garbled marker would strand exactly
what the sweep exists to remove.
**`may_sweep` has no "our own pid" arm, and that arm is why this module exists.** It used to
answer `true` for any marker naming this pid, on the reasoning that a sweep runs at startup and
so cannot be the instance mid-session. The sweep is a *detached* thread with `detect_bin`, a
`sysinfo` refresh and an `agy mcp remove` ahead of it, nothing joins it and no flag marks it
done, so a user who presses Enter in the AI panel first has already written a marker naming this
pid *with this pid's start time* — which that arm then swept, disarming the session that had just
installed it. The start-time comparison answers our own process correctly with no special case: a
marker written by *this* process carries *this* start time and is live, one left by a previous
process on the same pid carries a different one and is not
(`our_own_live_claim_is_not_sweepable_even_though_it_names_our_pid`, which composes the two rather
than testing `is_live` alone). **Both sweeps call it now**, so the module doc's second consumer
is real rather than pending: `ai.rs`'s `stale_mcp_file` and `stale_run_dir` parse an `Owner` out
of the file or directory name and ask `may_sweep`, and `MCP_STALE_AFTER` has dropped to the
fallback for a name that carries no owner at all. The live endpoint file the 24-hour mtime used
to delete went with it.
