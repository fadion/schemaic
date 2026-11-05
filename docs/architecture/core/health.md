# `core/health.rs`

`health.rs` — connection health-poll policy: `tick(HealthCfg, TickCtx) -> Tick` decides
ping-or-skip + the delay until the next tick (exponential `backoff` on consecutive failures,
longer interval for SSH-tunnelled connections, skip while the window is unfocused / a query is
already in flight / the tunnel isn't up). The app owns only the timer, `Db::ping` and the
landing guard: a ping is up to five seconds of waiting on the server — after the keyring read,
which is not on that clock (`db/overview.md`) — so `main.rs`'s `check_landing` stamps
each check with `(connection id, generation)` and a check that lands after a switch — or after
a newer check of the same connection — **writes** nothing: not `ConnStatus`, not the failure
count `health::record` folds. Without it, leaving a dead connection for a healthy one repainted
the header's "Disconnected · Retry" over a server that was answering, and only the next poll
could clear it — the poll the same stale failure had just told to back off.
**The `with_conn` continuation it may be carrying is a separate question, and asks
`check_continues` — connection identity only, not the generation.** A write is a repaint, where
a stale answer is worse than none; a continuation is somebody's click waiting on a reply, and
dropping it doesn't leave the screen stale, it makes the control they pressed do nothing at all
with no error — precisely what `with_conn` exists to prevent. Gating both alike made that
promise false on a timer: a blocked action pings, the ping takes its five seconds against a dead
host, and the health poll ticking (or the window regaining focus) inside that window stamped a
newer generation and threw the user's own answer away. A superseded result of the *same*
connection is still an answer about the same server and a few seconds old — enough to decide
whether to proceed. A **different** connection still refuses, which is the case that matters:
running an action gated on a server the user has left, or reporting the old one unreachable in a
modal over the new one.
**A check is not the only news about reachability, and `health::fold` takes all three**:
`Evidence::Checked` (a check the guard let write), `Listed` (the schema load listed the
databases — a sign-in and a query with the settings as they stand, everything a ping proves —
so `Connected` and the backoff cleared) and `Edited` (a save of the active connection's
settings — failures to zero, the verdict left for the check the save runs at once to answer;
clearing it to `Unknown` made a colour-only save of a connection that really is down hide
"Disconnected" until that check came back). Every write of the verdict
goes through `main.rs`'s `note_health`, which composes `fold` with the generation in
`note_evidence`. Only a check used to write, so a connection "Disconnected" on a wrong password
had its tree fill in after the corrected one was saved while the header went on saying
"Disconnected · Retry" until the poll, backed off by every failure that password earned, came
round; `save_conn` now also re-checks at once. **An edit retires every check in flight
(`Evidence::retires_checks_in_flight`) and a listing retires none**, and the asymmetry is the
point: a ping out at the time of the save carries the old password, and since a save moves no
connection id only a generation bump stops its failure landing over the new settings; a check
started *after* a load is newer news than the load, so a failure it brings back must still
land rather than be masked by the listing. Only a load that passes `load_landing` speaks, and a
*failed* load is no evidence — a listing can fail on a privilege a ping does not need.
**But a failed load is owed a retry, and `reloads_failed_tree(before, e, tree_failed)` says when
— only on a check that passes *and* moves the verdict to `Connected`.** A load that failed at
launch — the server not up yet, the tunnel still coming up, a keychain dialog nobody had
answered — cleared the tree, and nothing loaded it again: the poll recovered on its own and the
header dropped "Disconnected" over a tree that stayed empty until the user found Refresh. A
listing cannot be the trigger, being the load itself, nor an edit, which starts its own; and a
passing check that finds the connection already `Connected` must not be either, for the same
reason a failed load is no evidence — a load that fails on a privilege or a catalogue the ping
does not need would be retried at every poll, fail, check, reload, fail. A real recovery is a
change of verdict, so `before` is the verdict the evidence is folded *over*, and `note_health`
asks before it folds. The app's half: `load_schema`'s error arm records `tree_failed_for` (the
connection id, and whether the failure was one of reaching the server — below) in the same `LoadLanding::Install` branch that clears the tree, a load that
installs clears it, and `note_health` compares it with `active_conn` and bumps
`reload_failed_tree`; the reload itself is an effect on that trigger, declared after
`load_schema` because `note_health` is built before it, and it skips its first run. Pinned by
`a_check_that_recovers_the_connection_reloads_a_failed_tree`,
`a_check_that_changes_nothing_does_not_retry_a_failed_load` and `only_a_passing_check_reloads`.
**For an SSH connection that check could never come, so the poll reloads instead.** A load
caches its tunnel only when it succeeds — the one a failed load opened closes with it — and
`check_conn_then` returns without pinging while no tunnel is cached, `load_schema` being the only
thing that opens one. So a tunnelled connection whose load failed had nothing to ping through, no
check could pass, `reloads_failed_tree` never fired, and the tree stayed empty for good. `tick`
now answers `reload: true` on the tunnel-pending path — still `skip: Some(TunnelPending)`, still
no ping — when `TickCtx::tree_failed` is set and the window is focused and idle, the two gates a
ping has; the poll then bumps the same `reload_failed_tree` trigger a recovering check does, the
load being both the retry and the thing that opens the tunnel. **`tree_failed` is narrower than
"the tree failed"**: it holds only when the last load failed on *reaching* the server —
`main.rs`'s `unreached`, `DbError::modal_source() == ErrorSource::Server`, read off the error
before it becomes text and carried as the `bool` beside `ConnectResult`'s `LoadFailure`, which is
the second half of `tree_failed_for`'s `(u64, bool)`. A refusal of the app's own — a keychain read
the user declined, init SQL the guard refused — is not something a retry changes, and the poll
would only ask about it again. `note_health`'s reload still reads the connection id alone. The
retries back off on their own count, `reload_failures`, apart from `failures`, which counts
checks: every installing load's failure increments it — or starts it again at 1 when the failure
is another connection's, whose count says nothing about this one — and an installing success
resets it, and
`next` is `backoff` over the *tunnelled* interval capped at `max_backoff` — 30 s, 60 s, then the
120 s ceiling with the default `HealthCfg` — because each retry is an SSH handshake against a host
that may be down for hours, where the plain tunnel-pending skip re-arms at the base interval since
it costs nothing. The load that set `tree_failed` is itself counted, so the first retry comes on
the next plain tick and the 30 s step is not reached from the app today. Pinned by
`a_tunnel_that_never_opened_is_reopened_by_reloading`,
`reopening_a_tunnel_backs_off_on_the_reloads_that_failed`,
`a_tunnel_is_only_reopened_for_a_failed_tree_in_a_focused_idle_window` and
`a_failed_tree_over_an_open_tunnel_is_left_to_the_check`, the last because with the tunnel up the
ping can reach the server and the check's own reload is the right one.
