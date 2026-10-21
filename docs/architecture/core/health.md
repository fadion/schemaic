# `core/health.rs`

`health.rs` — connection health-poll policy: `tick(HealthCfg, TickCtx) -> Tick` decides
ping-or-skip + the delay until the next tick (exponential `backoff` on consecutive failures,
longer interval for SSH-tunnelled connections, skip while the window is unfocused / a query is
already in flight / the tunnel isn't up). The app owns only the timer, `Db::ping` and the
landing guard: a ping is up to five seconds of waiting, so `main.rs`'s `check_landing` stamps
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
