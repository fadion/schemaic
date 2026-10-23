# `schemaic-app`

`schemaic-app` — `main.rs` wires signals + callbacks and builds the `Ui`; also the `--mcp-serve`
branch that runs the MCP server the AI panel talks to (the server is `schemaic-cli`'s `mcp.rs`,
handed the endpoint blob the app wrote, on `cli::run::block_on_own_stack`'s thread rather than the
main thread, for the stack reason the `cli/run.rs` entry gives), and the argv branch into the headless
CLI (`schemaic-cli`, below). `main` returns a `std::process::ExitCode` so the one-shot front ends can
answer with one. A query tab's identity is `(conn_id, database)`;
the app resolves `conn_id` → `Db` at run time (`db_for`), so a tab keeps its connection after a
switch.
**The app draws its words from `schemaic-ui`'s catalogue, not a second one.** `main.rs` defines
its own `tr!` — `fl!(schemaic_ui::i18n::LOADER, …)` — above the `mod` lines, so every module of
the crate sees it, and `crates/schemaic-app/i18n.toml` points `assets_dir` at
`../schemaic-ui/i18n` with the domain `schemaic`. That file is the whole point: `fl!` checks an
id and its argument names against the *calling* crate's `i18n.toml`, so `schemaic-ui`'s own
`tr!` exported here would have compiled with no check at all. `build.rs` prints
`cargo:rerun-if-changed=../schemaic-ui/i18n` for the same reason — nothing else tells cargo that
an id added to another crate's `.ftl` means recompiling this one.
**Getting past the row cap is a per-tab override, not a fetch mode.** The cap is read once per
run, so `Tab::row_cap_override` — set by the results toolbar's "read N rows", cleared by the next
manual run — is all it takes; there is no second path through the DB layer. The label names a
**number** on purpose: the cap is a client-side cutoff of the result stream (`db::collect_rows`),
not a `LIMIT`/`OFFSET`, so there is no cursor to advance, "load more" would be a lie, and the
action re-runs the whole statement at a bigger ceiling — on an unordered query the second read
can legitimately disagree with the first. (The grid's pager, beside it on the strip, *is* a
`LIMIT`/`OFFSET`, and a different thing: it turns the statement's own row limit, never the cap —
`filter::with_page`, under *Data grid*.) `stats::next_row_cap` picks that number: five times the
rows actually **read** (not the configured cap, which differs whenever a filter or a small table
stopped the read short), floored at a thousand so a three-row result still offers something worth
pressing, and rounded up to two significant figures so the offer reads as a figure rather than as
arithmetic. **The step alone is not the offer, because it can name rows that do not exist**:
`stats::read_more_offer` takes the same `row_total` the stats line prints and, when the step
already covers it, says *"read all rows"* instead — its English `Display`, that is, over
`stats::read_more`'s `ReadMore::{All, Rows(n)}`, which the grid words by `read_more_text`, the
step through `i18n::count` (`english_offers_every_read_more_as_core_does`). That case shipped wrong — a 200k read of a
~292k table stepped to a million and the toolbar read "read 1m rows", on a line that had just
described the table as ~292.02k. It then shipped wrong the other way: *"read all ~292.02k rows"*
named the total a second time, three words from where `rows_read` had already printed it,
and the pair was the widest thing on the strip. **The offer names a figure only when the figure
is its own** — the step is a number that appears nowhere else on the line, and dropping it would
leave "read more" implying a cursor the row cap has none of; "all" needs no number, because
there is only one thing it could mean. The total is consulted, never trusted, **and an estimate
is given room to have been wrong**. Rounding it up to two significant figures tidies its tail but
is not slack, and re-capping is not the self-correcting answer it was written as: `employees`
samples to 292,025 and rounds to 300,000, which is 25 rows short of the table, so *"read all
rows"* came back capped — and the offer behind it, with 292,025 now stale against 300,000 read,
fell to the step and asked for **1.5m rows to fetch those 25**. So an estimate is padded by
`stats::ESTIMATE_SLACK` (a half — InnoDB's own documented 40–50% sampling error) before it is
rounded. The padding is free, because the cap cuts off a stream that ends when the table does: one
set above the real count is never reached. What bounds it is the step it replaced — *"read all
rows"* never asks for more than the numbered offer it was chosen over. A total at or below the
rows already read is stale and says nothing, the same rule `rows_read` follows. The offer is a
`label`, not a `text`, for the same reason `stats` beside it is one — the total arrives from a
catalogue query after the strip is built — and the click re-asks rather than capturing, so the
cap always matches the words the user just read.
**The offer takes itself off while it is being answered.** The re-run is a view run, which
deliberately leaves the current table on screen, so nothing else on the panel says one is
happening: the grid goes on looking idle and the words go on inviting a second click — a second
full read of the very table the label has just promised to read *all* of. `Tab::view_busy` is
that flag, and the app's run path owns it because only it knows when the re-run lands: set on the
last line before the spawn (so every early return above it leaves the flag alone), cleared on
every arm of the landing including `Cancelled` — a link disabled for ever is worse than one that
can be clicked twice — and cleared **after** the supersede check rather than before it, since a
run that has been replaced is not the one the flag is about any more. Whatever did the
superseding clears it instead, beside the `token.cancel()` in `run_query_core` and `run_all`,
because the superseded run returns at that check and never reaches its own clear. The label, its
colour and the click all read the one flag, so they cannot disagree about whether it is still a
link — and the click is guarded on it rather than on hover, since a pointer already over the
words when the first re-run started never leaves and re-enters them.
The re-run goes through `GridState::current_statement`, **not** `apply_grid_query`:
the latter reports a base it cannot rewrite as a *filter* failure ("not a simple single-table
SELECT"), and a join is perfectly re-runnable at a bigger cap — telling a user with no filter
that their filter is at fault is worse than the cap they were trying to get past
(`an_ineligible_base_is_still_ineligible_with_nothing_to_splice` pins the premise). **And the same
call is the write guard**: `current_statement` wraps `filter::rerun_statement`, so a statement that
may not be executed a second time draws no link at all — the offer's predicate and its click ask
the one function, where they used to ask only whether `base_sql` was `Some` (*Architecture
invariants*, the write guard). **Nor does the click run over staged work**: it re-runs into the
same panel, whose landing clears the staged edits, new rows and deletions with the rows they sat
on, so it asks `GridState::has_staged` first and refuses with `i18n::staged_requery_note`
(`filter::STAGED_REQUERY_MSG`'s words) in the bottom bar, as `apply_grid_query` does (the pager's entry under *Data grid* has why). Clearing the
override on a fresh manual run is the other half: a raised cap belongs to the result it was
raised for, and carrying it forward would be the global setting the user didn't change.
**The export is the other way past the cap, and it is a different shape.** `export_file` branches
on `ExportScope`: `Fetched` is the one `spawn_blocking` it has always been and needs no channel of
rows, everything being in memory already — but it renders in `EXPORT_CHUNK_ROWS` blocks all the
same, through `export::SliceChunks` rather than `ExportFormat::render_to` — or, for SQL, through
`export::export_insert_blocks` over the request's first block followed by `ExportRequest::more`,
with the same `watching` closure, because a pending ＋Row's `INSERT` names only the columns it
set and cannot share one stream's column list (`core::export` has it). `AllRows` ignores `more`:
it re-runs the statement, and pending rows are not on the server. **Not for memory**: the
rows are in hand and a chunk copies nothing either way. It is for the two things a single block
cannot offer — **progress**, since rendered whole there is no moment between "started" and
"finished" at which anything can be reported and a large Excel export is indistinguishable from a
hung one, and **a Stop that works**, between blocks being the only place a synchronous render can
notice one. It reports through the same `export_tx` channel the streamed arm uses, out of the one
`watching` closure that also asks the token. The arm's outcome match therefore carries an
`Err(_) if token.is_cancelled() => ExportOutcome::Cancelled` arm, the token being the witness
because the refusal's error text cannot be told from a write error's; it asks the token once more
before `publish`, so a stopped render is never renamed over the destination; and its failure arm
**and its cancel arm** run the same `writes_incrementally()` sweep the streamed one does, since a
buffered format has written nothing into the sibling whatever ended the export — a 0-byte
`foo.xlsx.part` in the user's folder, under a message reading *"foo.xlsx was not changed."* that
does not mention it, is litter either way, while an incremental format's sibling holds the rows
that arrived and is the one thing worth keeping. The `AllRows` arm had the identical gap on cancel
and both now sweep. **The worker's `JoinHandle` is awaited, not dropped**: this arm's only exit
from the modal is an outcome, so a worker that ended without reporting — a panic in a renderer, a
`spawn_blocking` pool shutdown — left an undismissable modal *and* a permanently occupied cancel
slot; it reports `Export failed: worker died: …`, which its `AllRows` sibling has said since it was
written. `AllRows` is
**two tasks and a bounded channel of 2** — the reader is async (two of the three drivers are) and
the writer is synchronous file IO, which must not run on a runtime worker. The writer's
`PullChunks` closure turns a channel `Err` into an `io::Error` and a channel close into
end-of-stream, and **the reader's verdict wins where the two disagree**: a cancelled read closes
the channel, which the writer sees as an ordinary end of stream, so the writer alone would report
a truncated file as a finished export. `EXPORT_CHUNK_ROWS` is 10,000, with the trade stated at
the constant and nothing downstream depending on the figure — but **it is no longer the only thing
that ends a block**: `RowDest::chunk_full` also cuts at `db::CHUNK_BYTE_BUDGET`, because a row
count is the wrong unit for a promise about memory (see `schemaic-db`'s streaming account). `export_token` is an
`Rc<RefCell<Option<CancellationToken>>>` in the same shape as `import_token` — cleared when a run
reports, so a later Cancel can't cancel an export that already finished.
**One slot means one export, and the second is refused rather than queued or keyed.**
Two sharing the slot is what the shape would otherwise permit: Cancel reached only the later one,
and whichever finished first cleared the slot and left the other with no way to stop it. Either
branch therefore answers `ExportOutcome::Failed("An export is already running. Cancel it
or wait for it to finish.")` to a run that would claim a full slot, rather than growing into a
keyed map — a single slot is correct once nothing can overwrite it. **What claims it is
`ExportRequest::stoppable`, not the scope.** The slot exists so one Stop always reaches the run it
points at, and it refuses a second export to keep that true — so only a run somebody can stop needs
to be in it.
`grid::save_export` answers `true` for both of its scopes, the progress modal carrying a Stop for
both; `monitor_view`'s log export answers `false`, and is therefore never refused and never
refuses. Claiming the slot on the *scope* alone was a real regression for exactly that save: the
Live Monitor's log is `ExportScope::Fetched` too, so a log export begun while a grid export ran met
*"An export is already running"* — a message about a modal the user cannot see, for a save that has
nothing to do with theirs — and a grid export begun while a log was writing met the same. Among the
runs that *do* claim it there is no save the refusal can cost, because the modal covers the window
while one is going: there is no second grid export to start. An unstoppable run gets its own
`CancellationToken` that nothing ever cancels, so the arm's two token checks are constant-false and
that path behaves exactly as it did before the `Fetched` render was chunked. **The completion
action clears the slot only when `stoppable` too** — otherwise a log export finishing would release
it out from under the grid export actually holding it, and the next Stop would reach nothing. The
log export's side of this is in `monitor_view.rs`'s entry.
`import_token` gets away with the same shape and no refusal
only because its modal admits one run at a time, while the Download menu sits on every result tab.
**The offer wears the accent, and its separator does not.** It shipped `text_dim` like the
description beside it, reaching the accent only under the pointer — which put the one escape from
the row cap behind a hover nobody had a reason to perform. It is now accent at rest (registered
as `accent on bg_results` in `contrast.rs`, Body threshold) and **stays blue on hover**, stepping
to `accent_hover`: trading the accent for `text` on hover read as the link switching off at the
moment it was aimed at. `accent_hover` is a themed role and deliberately not "a lighter accent" —
it moves *away from the surface*, which is lighter on dark (`#7C9CF0` → `#A3BCF8`, 6.5:1 → 9.18:1
on `bg_results`) and **darker on light** (`#3D66D6` → `#2B4FB0`, 5.03:1 → 7.19:1), because
lightening the light palette's accent on a near-white surface walks a Body-weight label towards
failing AA. A hover that reduces contrast is not a hover. Both states are registered pairs, so
the suite measures them in every theme. That makes it two views rather than one string — the `·` stays `text_dim`,
because a blue separator reads as part of the link — and the colour is driven off an explicit
`offer_hov` signal rather than `.hover()`, since a parent's hover colour does not cascade to a
child and the click target is the pair.
**The statement timeout is a clock wired to the Cancel button, not a second way to stop a
query.** `RunTimeout::arm` spawns one sleeper racing a `done` token and, when the sleep wins,
cancels *the run's own* `CancellationToken` — the same one Cancel fires, which each backend
already honours its own way (MySQL `KILL QUERY` on a second connection, PostgreSQL
`cancel_query`, SQLite the interrupt handle). Nothing new can go wrong at the database. Three
things about it are load-bearing. **`0` is off**, read once in
`core::persist::statement_timeout`, and off is the default — every release before this ran
statements unbounded, so a default timeout would start killing the long imports and reports
people already rely on. **Disarming is `Drop`**, because dropping a `CancellationToken` does not
cancel it: without that, every finished statement would leave an hour-long `sleep` behind, one
per run, for the life of the process (`a_dropped_watchdog_never_fires` pins it, and fails
against a tree with the `Drop` body removed). **One watchdog per statement, not per run** — the
setting says *statement*, so a ten-statement script gets the full allowance each, which in the
batch path means re-arming inside `run_batch`'s per-statement callback; a statement that does
expire still stops the batch, because the token it fires is the batch's. A timeout and the
Cancel button arrive as the identical `DbError::Cancelled`, so the watchdog's `fired` flag is the
only thing that can tell the user which stopped their query — hence `timeout_message` (the
wording; core's `StatementTimeout::cancelled_message` is the English the error's `text`
keeps), which quotes the timeout in the very words the settings dropdown uses (`ui::i18n::statement_timeout`
renders both, over `persist::statement_timeout_span`, for exactly that reason) and says where to
change it: *Settings → Query*, or *this
connection's settings* when the value that fired was the connection's own, since the app's may
then be off or different and the message would send the user to a setting that did not fire
(`a_connections_own_timeout_names_the_connection_as_where_to_change_it`). **It quotes the label
as the dropdown shows it, case and all** — it used to lower-case it, which a statement never
needed (it only times out under a non-zero timeout, whose English units are lower case already)
and which would break a German noun. `EXPLAIN` is bounded
too: `EXPLAIN ANALYZE` *executes* the statement, so it is as runaway-capable as the query itself,
and there the timeout must produce a `PlanState::Failed` rather than the plain-cancel `return`,
which would leave the modal spinning on `Running` for ever.
**A connection may override the app's timeout** (`Connection::statement_timeout_secs`, under
`connection.rs`), and the three sites that arm the watchdog — the run, EXPLAIN and Run All —
each resolve it at launch through `connection::statement_timeout_of(connections, tab.conn_id,
app)`, read untracked as the app's value always was, and arm `timeout.secs`; the
`StatementTimeout` they get back travels to `timeout_message` so the wording follows whose value
it was. Nothing else changed about the clock: it is still client-side, through the same watchdog
and the same Cancel token, with nothing set on the server. It bounds the editor alone — the MCP
server and the CLI keep their own deadlines (`QUERY_TIMEOUT`, `--timeout`), and the AI panel's
MCP subprocess is not handed a connection's value.
**The Server Activity poll is generation-guarded, and only runs while the panel is open *and the
window has focus*.** One effect watches `(right_panel, active_conn, activity_interval,
window_focused, activity_view)`; it bumps `activity_gen`, clears the snapshot when the
*connection* changed, then arms `activity_poll` — refreshing immediately only when the connection
changed, the panel just became visible, or the view switched between Sessions and Locks. Focus is in the rule because every tick is a full connect + authenticate against
the server being watched (one connection per operation, §7), so a panel left open behind another
window was opening and tearing down a connection every couple of seconds, indefinitely, for
nobody — the same reason the health poll pauses unfocused. Refreshing on an *interval* change was
the mirror-image waste: moving a struggling server from 2s to 30s, done precisely to lean on it
less, fired an immediate extra fetch, and since the generation had just been bumped the in-flight
guard couldn't suppress it — two `PROCESSLIST` queries at once, on the server that prompted the
change. Each timer carries
the generation it was armed under and stops the moment it differs, which is what makes closing the
panel, switching connections or changing the interval end the old loop instead of starting a second
one beside it — the same guard the Live Monitor's tick uses, and the same `try_get_untracked`
discipline, since a pending timer can outlive its signals at shutdown. An in-flight fetch carries
the generation too and is **dropped** on arrival if it no longer matches: a reply describing the
previous server, landing under the new one's heading, is a list of session ids the user might kill.
Clearing the snapshot on a connection change is the same rule stated once more.
**The Locks view rides the same loop rather than a second one.** `refresh_activity` fetches
`Db::fetch_locks` instead of the sessions when `activity_view` is Locks, under the same generation
and in-flight guard, and reports into `activity_locks`. A switch of view re-arms, and the re-arm
bumps the generation, so a poll of the other list still in flight can neither hold the new fetch
off through the in-flight guard nor land when it returns. A connection change and
`reset_activity` put `activity_locks` back to `Idle` beside the sessions snapshot.
**A switch is not the only way the server under the panel changes, and `reset_activity` is the
other way in.** The effect keys on `active_conn`, which is an *id*, and editing a connection in
place does not move it — so repointing the active connection from host X to host Y with the panel
open left X's sessions on screen, live-looking, under a connection now pointing at Y, and a kill
from that list would have sent X's thread ids to Y. `save_conn` calls `reset_activity` for **any**
save of the active connection, without asking whether the target actually moved: comparing would
mean carrying the old host/port/socket alongside the snapshot — a second copy of connection
identity kept in step by hand — to save one `PROCESSLIST` query when someone renames a connection.
**A snapshot is where that trade is right, and it is the only thing in `save_conn` for which it
is** — see *What an edit invalidates* below, where the same question is asked of a transaction and
answered the other way. **`delete_conn_now` calls it too**, and for
the same reason rather than a related one: deleting the *last* connection sets `active_conn` to
`startup_active_id(None, &[], high_water)`, the id the next connection will take — which can be
the value the signal already held (on an install whose high-water mark was never written it is
`1`, the first connection's), so deleting that one leaves the signal where it was and the
effect sees no change. The panel went on listing a deleted connection's sessions, offering to kill
them. Two call sites, one for each way an id can fail to move.
Inside it the generation bump comes first
and is the load-bearing half, since a fetch already in flight against the old host would otherwise
land and refill the panel with the rows being thrown away. **The refetch, unlike the clear, is
conditional on `db_for` succeeding**: clearing is always right, but refetching *now* is right only
when the connection can be reached this instant, and after an edit it routinely cannot — `save_conn`
has just dropped the tunnel and `load_schema` re-opens it asynchronously, so `db_for` answers "SSH
tunnel is not established yet" (the re-open having cleared any earlier load's failure from
`TunnelFailures`, so it is that sentence and not a stale reason) and refreshing anyway painted that
across the panel as `Failed`, with
no tick to retire it when the interval is off. `Failed` is the right answer for a refresh the *user*
asked for and the wrong one for a reset nobody asked for, where "no snapshot yet" is the truth; the
delete path reaches the same guard through `db_for`'s "connection no longer exists". For the same
ordering reason `save_conn` calls `load_schema` **before** the reset, so the re-open has at least
been asked for. All three arming sites — the effect, `reset_activity` and the **kill handler** —
take the "is the panel polling" gate from one `activity_polling` closure, which is
`activity::should_poll`'s three conjuncts; its reads are tracked, which is what the effect needs
and what makes them inert at the other callers. That gate has grown a conjunct before and a second
copy is how the next one reaches one asker and not the others, which is exactly what the kill
handler demonstrated: it passed a literal `true`, because the closure was defined eighty lines
*below* it, so a successful kill restarted auto-refresh on a panel switched away from or a window
that had lost focus — reinstating the load the `right_panel_visible` conjunct had been
added to remove, immediately after the one action on this panel that is reliably followed by
looking somewhere else. The closure moved above all three; the gate is
`every_poll_arming_asks_whether_anyone_is_watching`, a source gate rather than a unit test because
the defect was a literal at one call site out of three and nothing about the value was wrong. A
refresh over a
live snapshot leaves it on screen rather than passing back through `Loading`, or a two-second
interval would be a panel that flashes instead of one that updates. The interval is the only part
persisted, and it is **per connection**: `UiState::activity_intervals` holds the rules and
`activity_interval` is a *memo* over `(active_conn, the store)`, which is what makes the clock's
tint, the menu's marked row and the timer's re-arm all repoint together on a switch. The panel
writes through `ActivityActions::set_interval` rather than to a signal, so there is no effect
reading out of the store and another writing back into it.
**What an edit invalidates, and how expensive the wrong answer is.** `save_conn` asks **two**
questions, and which artefact answers to which is the whole of it. The cached SSH tunnel goes on
`targets_same_server` — the predicate the schema tree already asks, not a second reading of the
same fields — because a TLS change needs no new listener and tearing one down takes the forwarded
connections with it. Every pinned Manual `Session` on the connection and the live AI session go on
`Connection::invalidates_open_connections`, which is that predicate *plus* the transport: the
question there is not which server the next query reaches but whether anything already open is
still valid, and changing TLS mode from `Disable` to `Require` moves none of the nine fields the
narrow one compares — so the block was skipped, the pinned `Session` kept running, and every
statement in that tab and its `COMMIT` went on travelling in the clear while the form and the
status bar reported TLS. The MCP subprocess went on serving tool calls over the `Db` it was
spawned with, `ai::needs_respawn` being unable to see a transport either. `db_for` rebuilds a `Db`
per call, which is why every *other* path picked the change up and only those two long-lived
artefacts did not. It used to ask nothing at all: the tunnel was dropped on **every** save, which is true of an edit
that moved something and quietly destructive of one that didn't, because tearing the listener down
takes the forwarded connections with it — so changing a connection's *colour* killed the socket
under a pinned Manual transaction and rolled back uncommitted work while the tab went on offering
Commit and Rollback. **Cheap-and-unconditional is right for a snapshot and wrong for a
transaction**, which is the whole distinction between this and `reset_activity` above.
**`resettle_sessions(id, fate)` is what becomes of the pinned tabs**, shared by `save_conn` and
the status-bar `toggle_read_only` (which asks `open_session_after` of the flag's flip, so ticking
read-only no longer leaves a writable session committing — R1-L2-01,
`the_read_only_toggle_resettles_pinned_sessions`). It applies `OpenSession::pinned_fate` per
Manual tab. Under `Dropped` — the tunnel just evicted — a tab gets `delete_conn_now`'s treatment
rather than `repair_killed_session`'s: drop to Auto-commit, `TxState::closed()`, no prompt, since
the server rolls back on disconnect, and a reopen would put the tab on a tunnel that has just
gone; each open transaction released that way gets a `write_log::abandoned` line. **Under
`AskFirst` the socket and its transaction are alive**, so a tab whose transaction is open goes
through `guard_tx` (the release was what rolled the work back, unasked, and the write log kept
those writes as `ok` with no end line — S1.1-L5-03, S3.1-L5-02): Commit or Roll back ends it
through `end_tx`, which logs its own end, and the tab is re-pinned in Manual through
`open_session`, as one with nothing open is at once; **Cancel keeps the transaction and owes the
re-pin** (`repin_owed`), which `repin_if_owed` pays when the transaction ends — in `end_tx` and in
both run folds, so a typed `COMMIT` counts. It used to keep the stale session for the tab's life:
the old role, the old `search_path`, or writes on a connection since made read-only (S3.1-L5-01).
**Ticking read-only never reaches that Cancel**: owing the re-pin still left a writable session
writing, until its transaction ended, on a connection the whole UI called read-only (security
review S20). So when an edit `Connection::ticks_read_only`, both routes ask first, through
`settle_before_read_only(id, proceed, cancelled)`, which chains `guard_tx` over every Manual tab
on the connection with an open transaction *before* anything applies, and runs `cancelled` —
asking nothing further — on the first Cancel. `toggle_read_only`'s Cancel leaves the connection
writable and unchanged; once all are settled the flip applies and `resettle_sessions` re-pins the
now idle sessions read-only at once. The form's `save_conn` is thin for this: it reads `id`,
`previous` and the drafted `conn`, and either calls `save_conn_now` — the save as it was — or
settles first, its Cancel saving the rest of the edit with `read_only = false` and setting the
draft's `read_only` back, so the read-only change alone is not saved. Unticking goes straight
through, as before. `the_read_only_toggle_resettles_pinned_sessions` pins both routes' asking
`ticks_read_only` and going through the settle, and the form's `writable.read_only = false`.
`open_session` is built after the run actions that ask, so it is reached through `repin_late`, a
`LateAction`. `tx_prompt` holds one question, so the prompts are chained one tab at a time, the
way `run_ddl` chains its blockers. The AI session is on the same list
because `needs_respawn` cannot see a repoint: it compares the conn id, which does not move, and the
`AiSettings`, none of which name a host, so the assistant's MCP subprocess went on reading the
previous server — or, tunnelled, a local port that no longer answers. Dropping it costs nothing,
since `ai_send` replays the conversation into the next session's prompt. `delete_conn_now` drops
the live AI session too, and not merely for symmetry: it already cleared the *saved* transcript,
but the running `claude` child survived, and while deleting the active connection usually moves
`active_conn` and so respawns by the side door, that door had a hole: `next_id` was `max + 1`, so
deleting the highest-numbered connection freed its id for the next one created, which became
active under the same id with settings unchanged, and nothing asked for a respawn.
`Connection::next_id_after` has since closed the hole — an id is never handed out twice — and the
drop stays regardless, and is no weaker for it. A session aimed at a server the user has just
deleted must not outlive it whatever id comes next, and leaving that to the side door would make
the teardown depend on how ids happen to be allocated, which is the coupling that produced the
hole in the first place.
**Pointing the panel at a connection is `reset_ai_panel`, and it is one closure because
`active_conn` moves in two places and only one of them did it.** It drops the live session, clears
the staged attachment (rows belong to the connection they were taken from), swaps in the
conversation saved for the new id through `chat::for_conn`, marks those messages seen so they
reappear without the entrance animation, and clears `ai_busy`/`ai_stopping`. `switch_conn` had all
of that inline; `delete_conn_now` reimplemented two of the neighbouring resets (`conn_status`,
`health_failures`) and omitted these — so deleting the active connection left its transcript on
screen under the fallback's header, the next turn spawned a session on the fallback and replayed
the deleted conversation into its system prompt, and `persist_chat` then wrote the whole thread
back to `chats.json` **under the fallback's id**, undeleting the one thing the confirm modal says
in as many words is unrecoverable. The line that erased it ran two hundred lines before the write
that re-created it, which is what made the omission invisible. The delete path calls it with the
fallback's id, or with `0` when there is no connection left to restore a conversation from.
**A killed session may be one of ours, and the tab has to be told.** A Manual tab pins a
`Session`, that connection is an ordinary row in the panel, and the idle-in-transaction holder
blocking another tab is very often exactly it. Terminating it left the tab holding a dead socket:
the footer went on offering Commit and Rollback, the next statement failed with a connection
error, and the only way out was closing the tab. `repair_killed_session` matches the killed
`(conn_id, server id)` against `Session::server_id` across the session map and, on a hit, clears
the tab's `TxState` and reopens a fresh pinned connection through the existing `open_session` —
the tab stays in Manual with no transaction open, which is the truth after the kill. **The
`conn_id` half of that key is not a formality**: a server id is only unique on its own server, and
MySQL thread ids and PostgreSQL backend pids are small integers each server hands out from its own
counter, so two Manual tabs on two connections routinely hold the same one. Matching on the id
alone reached into whichever tab the map yielded first and, when that was the wrong one, closed a
transaction still open on a server nobody had touched and re-pinned its connection underneath it —
losing the uncommitted work of a tab the user never acted on, while the tab that actually lost its
socket stayed broken. **That lookup is `owning_tab_of`, a free function rather than the
`Rc<dyn Fn>` bound inside `app_view` it used to be, because the composition is where the bug
was.** `Connection::targets_same_server` is exhaustively covered and was never wrong; what was
wrong was the lookup around it, which compared `conn_id` — and two Schemaic connections routinely
point at one server (`local (app)` and `local (root)`, or two entries differing only in default
database), so a true match was rejected and the tab was left holding the dead socket. That is the
seam CLAUDE.md names, a pure function's composition with its caller, and while the lookup had no
name outside `app_view` nothing could call it, so the same substitution could be made again with
the suite green. The `owning_tab` closure that remains only *gathers* the three registries —
pinned server ids, tab→connection, connections — and hands them over; nothing in it chooses
anything. Ties fall to the `HashMap`'s arbitrary order, as before the extraction, since two tabs
pinned to one session on one server is not a state the app can reach.
**The kill asks `may_launch_destructive` before it raises the confirm, and again inside the
resolve** — the shared guard every other destructive modal action asks, which this path asked
nothing at all — and a read-only connection gets the refusal in words rather than a modal it cannot
complete. The second ask is the one the rule is about: the early one only spares the user a
question that would be refused anyway, while the confirm stands for an unbounded stretch of time
and the flag can flip under it. It reads the `conn_id` captured for the handle, so both asks are
about the same connection by construction.
The kill itself resolves its target `Db` **when the confirm is raised**, not
when the button is clicked, for the same reason a session id means nothing without its server: a
modal is open across an unbounded stretch of time, and reading `active_conn` inside `resolve` sent
`KILL CONNECTION 1148` to whatever connection was active by then, under a title naming the first
one. A **cancel** is not a kill and repairs nothing: it stops the statement and leaves the session
and its transaction alive. The session the confirm names comes from `activity::kill_subject`, told
which list the kill was asked from (`KillFrom`): from a session row or the banner, an id the
snapshot no longer holds asks nothing, the row having gone away; from a Locks row it is confirmed
by its id and the lock the row showed, both read from the **current** `activity_locks` — never
from the sessions snapshot, which is not polled while Locks is shown and can hold an earlier
session under a reused id — and an id that list no longer holds asks nothing too. Everything
after the lookup — both guards, the target, the confirm — is the same for both. A `db_for`
failure at the click is said in `activity_kill_error` rather than returning in silence, and so
is the read-only refusal, per kind, in `ui::i18n::activity_read_only_refusal`'s words (core's
`activity::read_only_refusal` is the English it is held to).
**Opening and saving a tab's `.sql` file** is `TabsActions::open_sql_file`/`save_sql_file`/
`save_sql_file_as`/`reload_sql_file` (Ctrl+O / Ctrl+S / Ctrl+Shift+S, bound in `NavKeys::handle`
so they work at the workspace root and inside the editor alike, and offered as Open File / Save
File / Save File As in the palette). They are split the way the results export is: the dialog
(`floem::file_action::open_file` / `floem::action::save_as`, filtered on
`sqlfile::SQL_EXTENSIONS`) and the tab bookkeeping run on the UI thread, while the read/write goes
to `handle.spawn_blocking` and comes back through `create_ext_action` — the IO is synchronous and
a large script would otherwise freeze the window. Every decision about bytes and names is
`core::sqlfile`. A tab closed mid-flight is detected with `try_get_untracked` and the callback
degrades to a no-op: the bytes are on disk either way, there is just no tab left to mark saved,
and reading a disposed signal would panic. A save snapshots the text *before* the write, so typing
during it correctly leaves the tab modified afterwards. Failures land in the shared error modal
(`error_modal_text` + `error_modal_open`), because a failed Open or Save has no grid and no error
bar of its own to land in and silence is the one thing it must not be. It goes as a
`ModalError::app` — a file error, not the server's words, so the modal offers neither AI action. Open activates an
already-open tab on the same connection rather than opening one file twice — two tabs saving over
each other is a lost edit — and otherwise places the new tab through `place_tab`, whose blank-slate
predicate gained `path.is_none()`: a tab bound to a file is not a blank slate even when the file
is empty, since reusing it would drop the binding and the next Ctrl+S would go somewhere else.
Save falls through to Save As when the tab has no path, and Reload asks through the shared
`Confirm` channel when the tab is modified, since nothing else in the app can put those edits
back. None of the four is connection-gated, deliberately and like the export: a file is between
the editor and the disk.
**Closing a modified file tab asks too, and `guard_close` is where both close questions live.**
It has `GuardTxFn`'s signature (aliased `GuardCloseFn`) and wraps `guard_tx`, so it drops into
every path that already took one — `close_tab` (×, middle-click, Ctrl+W) and `close_tabs_seq`,
which means Close all / Close other tabs ask per dirty file tab as their chain reaches it. The
blanket "close all tabs?" confirm is about closing tabs, not about discarding file edits, and the
chain keeps one question on screen at a time.
**The order of the two questions is load-bearing, and so is asking neither of a tab that can't
close.** Answering the transaction prompt is not an answer but an *action* — it commits or rolls
back — so anything that can still call the close off has to be settled first. `guard_close`
therefore checks `tabsel::can_close` before it opens anything (a pinned tab, or an id already
gone), then asks about the file, then hands over to `guard_tx`. Both orderings were wrong before:
Ctrl+W on a pinned tab holding a transaction prompted, took the commit and then declined to close,
because the pinned test lived only at the far end in `close_tab_now` — which still refuses, as the
backstop every close path passes through, but by then the damage is done. The file question is
raised only on a file-backed tab, since `Tab::modified` is false for an ordinary one.
`close_tab_now`'s keep-≥1 branch clears `path`/`disk_sql`/
`file_format` along with the text: the blank slate it leaves behind must not still point at a
file, or the next Ctrl+S would overwrite that file with an empty document. It clears
`disk_changed` too — the *changed on disk* offer was about the file just shed, and left up it
showed over a tab with no file and, once Open reused the slate, offered the old file's text as
the new one's (found documenting the bar, before it shipped).
**It drops the tab out of `TxMode::Manual` too, and that is a rule about every release rather than
about this branch.** `session_for` matches `Manual` and then looks the tab up in `sessions`; with
the entry gone it returns `None` and the run fails before dispatch with *"the transaction
connection isn't ready — switch to Auto-commit and back"*, and nothing re-opens it —
`open_session`'s callers are `set_tx_mode`, `set_active_db` and `repair_killed_session`, and a
closed-and-reused tab is on none of those paths. That branch resets nine pieces of tab state and
`tx_mode`/`tx` were the two it did not touch, which are exactly the two the release invalidates;
both siblings that drop a session while keeping the tab (`save_conn`'s repoint, `delete_conn_now`)
set `TxMode::Auto` in the same breath. `app_tests::releasing_a_session_drops_its_tab_out_of_manual`
holds every `(drop_session)` call site to it, with `open_session`'s own release exempt as data —
it drops only in order to re-pin, and its `Err` arm is what falls back to Auto.
A file tab survives both kinds of restore. `persist::SavedTab` carries `path`, `file_crlf`,
`file_bom`, `file_utf16`, `file_lossy` and
`file_dirty`, each `#[serde(default, skip_serializing_if = …)]` so a session file written before
the feature still restores its tabs — flat fields rather than a nested struct for exactly that
reason (`file_utf16` is an `Option`, absent meaning UTF-8); `ClosedTab` carries `path`/`disk_sql`/`file_format` so
Ctrl+Shift+T brings back a *file* tab rather than an untitled copy of its text, and its "worth
restoring" guard counts a path as worth restoring on its own — the binding is the thing being
lost, even from an empty file. The file's *contents* are deliberately not persisted: `file_dirty`
is one bit that lets the restore decide whether the `query` it already has **is** the on-disk copy
(`disk_sql = Some(query)`) or unknown (`None`, which reads as modified until the next save or
reload settles it). The reopen ring takes the same road to and from `closed_tabs.json`:
`ClosedTab::to_entry` writes `file_dirty` by `Tab::modified`'s rule (a path, and `disk_sql` not
the text) with `pinned` false, and `from_entry` comes back through `sqlfile::restored_binding`,
as a restored open tab does.
**A long run that ends behind another window calls the user back**: the taskbar flash on Windows
and Linux, the dock bounce on macOS (`WindowIdExt::request_attention` with
`Urgency::Informational`, which stops on its own once the window is focused). `call_back_after`
asks `history::wants_attention(took, window_focused, stopped)` — at least `ATTENTION_AFTER_MS`,
one second, the window not in front, where the result is already on screen
(`a_long_run_finishing_in_the_background_asks_for_attention`), and **not a run the user
stopped** (`a_run_the_user_stopped_does_not_ask_for_attention`). The two editor run paths pass
that flag because a Stop does not always reach them as `Cancelled`: Run Everything keeps the
statements that finished before it, so `runs` stayed non-empty and a stopped batch read *Query
finished*, and the single run builds a Stop before the statement was sent, or after a MySQL
DDL's implicit commit, as `Failed`, which read *Query failed*. The single run's flag is
`Err(Cancelled)` without the watchdog having fired; Run Everything's is the token cancelled with
no statement's watchdog fired (the timeout cancels the same token). **The bar is low on purpose**: a
window not in front when the run ends is already the evidence the user walked away, so the bar
only keeps a run that was over before anyone could leave from asking. It was ten seconds, and a
1.2 s query switched away from the moment it was run never flashed
(`a_run_walked_away_from_after_a_second_asks_for_attention`). Both run paths ask it after their
supersede checks, so only a run the tab still shows does, and only one that reached a verdict:
the single run with its `took`, Run Everything with its statements' times summed, since a long
script of quick statements is as much a walk-away as one slow statement. A `.sql` file run
(`script_run`), both of `export_file`'s paths, a dump (`dump_run`) and a folder export
(`files_run`) ask it too, timed by an `Instant` taken as each claims its cancel slot — and **not
on a cancelled outcome**, which each filters itself and so passes `stopped` as `false`: the user
pressed Stop in this window, and a cancel can still land after they have switched away, where a
flash would call them back to something they ended. The fetched
export's two ext actions over one `done` share an `asked` cell, so only the report that lands
first asks — the same one `ExportTarget::run` keeps.
**The same moment can also show a desktop notification, and only if the user asked for one.**
`call_back_after` takes `(Finished, failed, took_ms, stopped)` rather than the time alone, and when
`wants_attention` says yes and the `notify_finished` signal is on (Settings → Query, *Notify
when a run finishes in the background*; `persist::UiState::notify_finished`, `serde`-defaulted
off and autosaved as `live_validate` is) it hands `notify::finished_note`'s title and body to
`notify::show`. Each caller says what ended and whether it failed: the single run
`Finished::Query` with `!ok`, Run Everything `Query` failed if any statement's `RunResult` is
not `ok`, `script_run` `Script`, both `export_file` paths and `files_run` `Export`, `dump_run`
`Dump` — each failed on its outcome's `Failed` arm. **There is no second gate**: the
notification is asked behind the flash's `wants_attention`, never beside it, so a cancelled run,
a run in a focused window and a superseded one stay as quiet as they were. Off by default
because the flash is the quiet half and this is the half that interrupts. Seen live on Windows 11
in a sandboxed profile — a toast headed *Schemaic* with the app icon, *Query finished* / *Took
2.0s.*, and none with the toggle off; macOS and Linux have not been watched.
**Every write the app runs is appended to the write log (`core::write_log`) when it lands.**
`log_write`, built beside the history writers, is the one sink: it fills the entry's
`connection` with the saved name looked up by `conn_id` (`connection N` once it has gone),
reads `write_log_values` at that moment, and appends on `spawn_blocking`. An append that fails
is a `tracing::warn!` and nothing else — the write it records has already happened, and must not
be reported as failing because its record did. `log_landed` hands a run's entry to the sink as
`write_log::Entry::landed` ends it, through the thin `ran_of(&QueryState)` — `Loaded` its
`affected`, `Failed` the engine's message — the `ModalError`'s English `text`, never its `shown`
wording — `Cancelled` cancelled, which may still have written, a
sent statement not being unsent; `Idle`/`Running` nothing. Its `aborted_before` is whether the
tab's transaction was `TxState::Poisoned` when the statement ran, which is what turns a typed
`COMMIT` PostgreSQL reported as a success into the `failed` line it really was (under
`write_log.rs`). The hooks, and why each sits where it does:
`run_query_core` stamps an entry at launch for a statement `write_log::is_logged` answers for and
that is not a view re-run (`!is_view`), and logs it in the `send` ext action **before the
supersede check**, where the history is finished: the write happened whichever run the tab now
shows. **It reads `tab.tx` before the transaction fold below it** — after the fold, the state is
the one the statement produced, not the one it ran in. Run All stamps one per logged statement
and logs those `write_log::batch_logged` picks, **skipping a `Cancelled` statement**, which a
batch reports for every statement after a failure without dispatching it (`drop_runs`' reason,
under `history.rs`) — except the one in flight when the user pressed Stop, the first statement
without a `Loaded` state. Its `aborted_before` is `write_log::batch_aborted` over the tab's
`TxState` at launch, since one batch can poison the transaction and then `COMMIT` it; the tab's
own state is folded afterwards, as before. `end_tx` logs a Manual tab's
`COMMIT` or `ROLLBACK` when the transaction was open (`TxState::is_open`): the tab's writes are
logged `ok` as they run, before anything commits them, and that end entry is the only line that
says whether they stayed. **So a transaction Schemaic drops rather than ends gets one too**:
`resettle_sessions` releasing a session under `OpenSession::Dropped`, and `delete_conn_now`, log
`write_log::abandoned` — a `ROLLBACK` whose detail says the connection was edited or deleted — for
each tab whose transaction was open; without it those writes stood in the log as `ok` with no
end. `delete_conn_now` resolves the dialect from the saved connection, and logs nothing if it
cannot find it. A transaction `save_conn` asks about ends through `end_tx`, which logs its own.
`script_run` logs a `.sql` file whole — `Source::Script`, its detail
`file <path>, N statements ran`, and as `sql` the failing statement when `RunOutcome::Failed`'s
`at` names one; the statements that succeeded are not lines of their own. Its error is
`ScriptFailure::message`, English: the failure is a value (`Said`, `Changed`, `ReaderStopped`,
`NoTerminator`) that `script_view` words for the modal, since the reader's own two sentences
once reached this persisted record in the interface's language
(`english_says_every_script_failure_as_core_does`). `commit_edits` logs
`grid_sql` of the staged batch with `commit_writes`' count, through a `log_commit` ext action
called right after the write and before the re-fetch, which is a read. The DDL preview's run
logs the plan's statements joined by `;\n` — an account's password masked by the log whatever
the setting says. `import_run` logs the load alone (`log_load`, after `import_rows`), never the
validation pass before it, which writes nothing; its detail is `import <path> into
db.schema.table` and it has no `sql`, the rows being the file's. *Copy to*'s rows are
`Source::Copy`, logged against the **target** connection in `copy_rows`' report with no `sql`
and the detail `copy <SELECT> into db.schema.table` — the `SELECT` being `copy_table::select_sql`,
each column as `copy_table::read_form` reads it (see `core/copy_table.rs` for the forms, which
differ by engine and on SQLite wrap every column), but never a value; its `CREATE` is the DDL
preview's entry like any plan's. A copy whose rows
committed but whose PostgreSQL settle failed is logged `failed` **with the committed count** in
`rows` — the report's payload is `Result<u64, (Option<u64>, Option<String>)>`, its `Option<u64>`
`Some` only for that case — so the line does not read as a copy that wrote nothing.
*Restore from file* is `Source::Restore` (`launch::restore_log_entry`), logged by `restore_db` once
the confirmed restore's client argv is built and just before it goes to the panel — so its
outcome is `unknown`, the terminal's client and not Schemaic seeing how it ends, and its detail
`restore <file> into <db>, run in the terminal`, with no `sql`.
**Not logged**: EXPLAIN ANALYZE, which `Db::explain` always rolls back or never executes; a view
re-run, a read Schemaic built; the MCP server, which has no write tool; and SQL Server's *Back
up*, whose `BACKUP DATABASE … WITH COPY_ONLY` writes a file and `msdb`'s history on the server
(`launch::backup_writes_server`, which is why read-only refuses it) but changes no data in any
database. The setting is
Settings → Query's *Keep values in the write log* (`LayoutUi::write_log_values`, tabindex 250
after the notify row's 240), `serde`-defaulted off and autosaved as `confirm_writes` is; its
hint says the log is never cleared and a password is masked either way.
**A file changed by another program is offered back when the window regains focus**, not by a
filesystem watcher: the edit that matters was made elsewhere, and coming back is when the user
would act on a stale copy. A second `create_effect` on `window_focused`, beside the connection
re-check and with the same false → true test (the effect's own first run is a mount, so a launch
is not a focus gain), collects every tab holding both a `path` and a `Some` `disk_sql`, and for
each reads the file on `handle.spawn_blocking` and asks `sqlfile::disk_change`; the answer comes
back through `create_ext_action` into `Tab::disk_changed`. **The report is dropped when the tab
is no longer bound as it was when the read began** (`sqlfile::report_is_current`): its `path`
not the file read, or its `disk_sql` not the text compared against — a tab closed, saved, saved
as or reloaded while the read was in flight. The path is asked too because a clean tab's Save As
writes the text it already held, and a report from the old file passed the text check and raised
"“b.sql” changed on disk" with `a.sql`'s contents
(`a_focus_report_is_dropped_once_the_tab_is_bound_otherwise`). It writes the signal only when the value differs, so a file
changed *back* clears the bar. `write_tab_to`'s and `reload_sql_file`'s success branches clear it
too, since either makes the file the tab's again. What it does not cover, stated rather than
implied: a tab restored with unsaved edits (`disk_sql` `None`) is never checked until a save or
reload settles it, and a tab restored clean against a file changed while the app was closed is
caught at the first focus regain, not at launch — the restore does not re-read the file.
The offer is `lib.rs`'s `disk_bar`, **a row of the centre column's `v_stack` between the tab strip
and `editor_area`, not another overlay over the text** — `editor_area`'s `stack` is at floem's
16-child limit, and freeing a slot by wrapping is the trap described under *Floem 0.2 gotchas*.
It is a `dyn_container` keyed on the active tab's id while that tab's `disk_changed` is `Some`,
reading "“<file>” changed on disk.", in `text_dim` on `bg_deepest`, with **Reload** in the
footer's `tx_commit` green and **Keep mine** in its `tx_rollback` red (hovers likewise) — a
commit/discard pair, on the same surface whose pairs `contrast.rs` gates. Not the grid's
`grid_edit_staged`/`grid_edit_discard`, which are icon colours and fail as body text there
(2.6:1 at best). **Reload** is the tab menu's own
`TabsActions::reload_sql_file`, which asks through `Confirm` first when the tab is modified.
**Keep mine** sets `disk_sql` to the text the check found and clears the bar, so the tab reads
modified and the next save overwrites the file deliberately. That assignment is load-bearing:
leaving `disk_sql` at the old text would have the save-time guard — which refuses when the bytes
are not what the tab read — refuse exactly the save the user just chose.
**Quitting the window is the one close path `guard_close` never sees, so it is answered by a
flush rather than a question.** Floem 0.2 handles `WindowEvent::CloseRequested` by closing
unconditionally — there is no veto to hang a prompt off — but the close *is* observable
(`WindowHandle::destroy` fires `WindowClosed` before disposing the scope), so the workspace root
carries a `WindowClosed` listener that writes the session synchronously. Without it a quit inside
the 600 ms save debounce left `tabs.json` holding the **previous** save, whose `file_dirty` was
`false` because the tab was clean then: the tab came back with the pre-edit text, no italic and no
dot, reporting itself as matching disk. Confidently wrong is worse than stale. The snapshot is
built by one closure (`session_snapshot`) shared with the debounced effect, because the builder
that runs at quit is the one nobody watches.
**The listener then ends the process itself, with `floem::quit_app()` after its three flushes.**
The vendored Floem's `AppHandle::close_window` exits the event loop once the last window is gone
on every platform but macOS, where nothing does: closing the window left the process in the menu
bar, and *Restart to update* (`app/update.rs`) waited for an exit only the menu bar's *Quit*
produced. The flushes are synchronous, so `quit_app` cuts nothing short; elsewhere the loop is
already exiting and the extra request is harmless. The app is single-window, so macOS's
stay-alive-with-no-window convention has nothing to reopen. It is a call here rather than a Floem
patch by decision, so `vendor/floem/PATCHES.md` has no entry for it. **Unverified:** whether
macOS's own *Quit* / Cmd+Q — winit's `NSApplication` terminate, not vendored and not read — fires
`WindowClosed` at all; if it does not, text typed inside the last debounce interval is lost on
that path.
**The schema tree's filter rides the same listener, as `flush_filter`, for the same reason.** It is
saved to `ui_state.json` on a 600 ms trailing debounce in `tabs.json`'s generation shape — each
change bumps a generation and only the last of a burst writes — because it changes per keystroke
and every other `ui_state` effect saves off a setting that does not; the effect's first run is the
value just loaded and is skipped. `flush_filter` writes only when a save is pending (a
`filter_unsaved` cell, set on change and cleared by the timer's save), so a quit inside the window
keeps the text and a quit with nothing pending does not rewrite `ui_state.json` and its `.bak`.
**And "restore tabs off" is not a licence to drop unrecoverable text.** With the setting off, the
flush writes — and the restore reads — only `SavedTabsFile::unsaved_files_only`: a file-backed tab
with unsaved edits, whose text is neither on disk nor retypeable. Every other tab is a query the
user can retype or a table they can reopen, so nothing else is stored against their preference; the
same subset is read back so a full session left over from when the setting was *on* isn't silently
restored either.
**The reopen ring follows the same setting, with no such carve-out.** `closed_tabs.json`
(`core/persist.rs`'s entry) seeds `recently_closed` at startup only when *restore tabs* is on,
less any entry whose connection no longer exists — the delete prunes the ring, but a file from
before that rule, or edited by hand, may still hold one. `save_closed(erasing)`, defined beside
the `restore_tabs` signal, is called after every change to the ring — each push in
`close_tab_now`'s `record` (capped at `CLOSED_TABS_CAP`), each reopen, and the connection delete
straight after its `retain`. **An ordinary call is debounced 600 ms, as `tabs.json`'s save is**:
the ring holds up to ten whole tab texts, and *Close All* over twenty tabs used to rewrite it —
serialise, `.tmp`, `.bak`, rename — twenty times on the UI thread. A call bumps a generation and
schedules `write_closed`; a later one supersedes it, and `flush_closed` writes a pending one from
the `WindowClosed` handler beside `flush_session`. **The delete's `(save_closed)(true)` is never
deferred**: it cancels any pending write and erases at once. With the setting off the write is an
empty file, erasing; the delete passes `erasing = true` whatever the setting, so no `.bak` keeps a
deleted connection's closed queries. What is written is `ClosedTabsFile::for_disk`'s, so a closed
tab carrying a credential stays off disk. An effect on `restore_tabs` calls it, so turning the
setting off empties the file within the debounce rather than at the next close; its first run,
at startup, rewrites the file as just filtered. **An empty file is written once, not per
change**: `write_closed` keeps what the last write left (`persist::RingOnDisk`) and skips an
ordinary write that would leave the file saying nothing again, so with the setting off a close
is not an erasing save paying the fsync `persist.rs` budgets one per deletion — but not an
erasing one, nor the setting's own going off over a kept `.bak`
(`persist::closed_ring_saving`): the ordinary write that emptied the file kept the ring before it
as `.bak`, which the delete's erasure or the setting's removes. What this does not
keep, stated rather than implied: with the setting off, a closed file tab's unsaved text in the
ring does not survive a restart, unlike an open one's.
**What the assistant knows about the result on screen, and how rows reach it.** `TurnContext`
carries a `result` — `core::prompt::result_shape` over `Tab::shown_result`, which is
`shown_panel` again rather than a second fallback rule (a stale `active_result` shows the first
panel in the pane, and the AI has to describe the same statement) — so columns, types, counts, the cap and
the elapsed time ride on every turn, and *no cell value ever does*. A failed run's error text rides
on every level, but **verbatim only where the connection's `AiData` allows it**: the server quotes
stored values into its messages, so below `Full` `core::prompt::redact_engine_error` takes those
out and the rest of the message still goes, while a note of Schemaic's own, which quotes none,
goes verbatim at every level (`core::prompt`'s entry); `ai_context` passes the level in beside the dialect. It is
diffed like the rest of the context, and a cleared panel is reported as cleared rather than
omitted: a stale shape would have the model explaining an error the user has already fixed. The
same snapshot sends the editor **selection** in place of the whole buffer when there is one
(`core::text_ops::selected_text` over the range `Tab::selection` mirrors out of the mounted
editor), labelled as a selection by `editor_section_label` — one function, because the system
prompt and every later delta have to agree about whether the model is looking at a fragment or
the script. Rows themselves travel only as an `Attachment`: the grid's *Attach N rows to chat*
stages one in `AiUi::attachment`, the panel shows it as a chip with an ×, and `ai_send` takes it
(clearing the signal, so a second question can't silently re-send it), puts it on the
`ChatMessage` for the transcript and prepends `prompt_block()` to the turn. The take is gated on
the level of the connection the turn is **going to**, and the chip is cleared by a connection
switch and by New Chat: rows are staged against the connection they came from, and the user can
switch before sending. `ai_regenerate` carries the attachment back into the signal, since the
rows travel with the turn rather than in `ChatMessage::text` and a regenerated question without
them asks about data the model cannot see. The whole path is
governed by the connection's `AiData`: `ai_context` reads the level from the same lookup that
gives it the dialect — rather than taking it as a parameter that could disagree with the tools
the session was actually given — `start_ai_session` turns it into the tools list and the MCP
blob's `samples` flag, and `ai_send` respawns the session when it changes, since both were fixed
at spawn and a data-access setting that doesn't take effect is the worst kind of lie.
**Which settings respawn is a rule, and it is `ai::needs_respawn`** — was an inline `need_new`
closure in `main.rs`, where no test could reach it. The rule is **a setting the spawn froze**,
which is very nearly all of them. The gravest decide what may leave this machine — `data`,
`hidden` and `schema_scope`, all three riding in the tools list and the MCP blob, written once at
spawn. `hidden` is the one that shipped without this: hiding a database mid-session left
`list_schema` enumerating it and its every table to the vendor while the *prompt* half of the same
feature updated per turn, so the user watched the assistant stop volunteering the database with no
way to know the tool it can call still saw it. **`model`, `effort` and `instructions` are frozen
just as hard** and the rule used to say they could wait: the first two are argv on the `claude`
child and the third is written into the system prompt, which `ai_context` composes once and every
later turn only sends deltas against. Nothing was visibly broken, because all three are settable
only in the AI settings modal and `ai_apply` compared the whole `AiSettings` with `!=` on close —
a second, blunter rule quietly carrying the case the tested one declined, holding only as long as
no control outside that modal writes them. A premise, not a design. A different connection is
always a new session, since the level, the hidden set and the `Db` handle all belong to it.
**`cli_path` and `harness` are the two exceptions, and they are why the rule takes a `cli_usable`
argument.** Every other setting is a value the app can act on the moment it changes; these two
name a *binary*, and adopting a name that resolves to nothing trades a working conversation for
one that cannot start. So they count only when the result is spawnable —
`agent_cli::harness_reachable`, asked of the **new** settings: an override that resolves, or an
empty value whose auto-detect succeeds for that harness. Choosing a harness that is not installed
therefore keeps the live conversation instead of trading it for a binary nothing can spawn — and
**that gate is why the pending bubble cannot stamp the selected harness**, which is
`ai::turn_harness`'s job and is in `core::transcript`'s entry. The
AI assistant section of Settings is where the user finds out it is unreachable — its `cli_ok` reads the
harness *signal* rather than closing over a value, so the hint follows the selection rather than
validating against whichever CLI was chosen when the closure was built. Two corollaries, both
easy to get wrong in
the other direction: **manual → empty respawns**, because empty is *auto-detect* rather than
"unset" and resolves to a binary the session was not started from; and a broken path is **not** a
licence to ignore the rest, the gate sitting on the `cli_path`/`harness` comparisons alone. The
filesystem
question belongs to the caller because the function is pure — the reason it lives in `ai.rs` at
all. Both call sites ask it: `ai_send` before a turn, and `ai_apply` when the AI assistant
section's page goes away — a switch to another section, or Settings closing. `ai_apply` used to compare the whole `AiSettings` with `!=` instead, which is a second
rule for one question and had already drifted — `!=` counts `cli_path` unconditionally, so typing
a path that resolves to nothing (the state the field's own red hint is for) threw the live
conversation away.
**The function enumerates its comparisons rather than deriving them, and that shape has one
failure mode worth knowing before you add a field**: a field added to `AiSettings` and not added
here is ignored in silence — nothing fails to compile, and the symptom is a session that goes on
running with a setting the user changed. That is exactly what happened when `harness` was added,
and the test is what noticed
(`a_cli_path_that_cannot_spawn_leaves_the_live_session_alone`, which carries the harness
assertions beside the path ones because both turn on `cli_usable`).
**A respawn
that can't happen refuses the turn** rather than falling through to the old session: with no
`Db` (a tunnel still coming up) the previous session is dropped and the panel says the database
is unreachable, because answering through a session built for the previous level is this control
failing open. The grid
asks `ai_data_of` (`GridState::conn_at_load` — the connection the rows were *loaded* under, not
the tab's live one) through `grid::ai_offered`, which is `GridState::ai_enabled` **and**
`ai_data_of(gs).may_attach()` — the one question every AI entry there asks, so none can check the
connection's consent and forget the switch, or the reverse — and checks it at each action as well
as when building the menu. `ai_data_of` itself is unchanged, so its source gate still pins it to
`conn_at_load`. The toolbar's sparkle is where the two halves part: a schema-only connection keeps
its explanatory disabled entry, a choice about *this* connection worth saying so, while an
assistant that is off has no sparkle built at all — off is the user saying no AI anywhere, and a
sparkle opening onto a refusal would keep asking.
**A session may only start on a binary the probe established can be restricted, and that gate is
the first thing `start_ai_session` does** — ahead of the MCP config being written and ahead of the
oversize check. `ai::spawn_refusal` is the rule: a `Constraint` that is not runnable is refused
with `Constraint::notice`'s sentence, drawn in the interface's language by
`i18n::constraint_notice_text`. It used to have a second arm — a harness this build did not drive,
which was Gemini and only Gemini — and with that harness deleted every variant the enum names is
driven, so the unestablished constraint is the only refusal left. **The arm was rewritten rather
than removed**: it is now an exhaustive
`match harness { Claude | Codex | Antigravity | OpenCode | Copilot | Cursor => None }`,
so the *next* harness added to the enum lands here as a non-exhaustive-match error and has to be
decided. A `None` fall-through would instead spawn it on an argv read off documentation, which
dies on its first unknown flag and gets reported as an installation problem — the one thing that
would not be wrong with it. **The refusal has to keep answering, and for a while it answered once
and then swallowed everything.** It sent its line to the panel over `ai_tx` and returned its sender
with nothing spawned — which drops `rx` and closes the channel, and `needs_respawn` does not rebuild
a session whose settings have not changed, so `start_ai_session` was never re-entered: the *second*
question was a discarded `Err` on a dead sender, the pending bubble spun until the user pressed
Stop, and nothing anywhere said why. The comment here claimed the opposite — that the next question
re-entered and met the same check — which is the shape of drift this document exists to catch, in a
comment rather than a paragraph. `refuse_every_turn` is the cheap way to make that claim true: a
task holding `rx` open that re-states the reason per turn (and ignores `Interrupt`, since Stop on an
idle panel is not a question). **All five** non-spawning returns take it — the constraint one, the
OpenCode config-write failure, Copilot's home-write failure and Cursor's workspace-write failure
(the same `refused` block, an arm each further down), and the persistent branch's oversize
refusal, which was the one left
behind and had the same silence to show for it: it returned a `tx` whose `rx` it had just dropped,
`needs_respawn` saw no settings change, and every later question was a discarded `Err`.
It lives on the one function that spawns an agent for the
same reason the write guard lives on the run action: a gate a caller has to remember is one
`return` from not existing. And it **reuses the binary and the `Probe` it checked** for the spawn
and for `build_session_args`'s seal — re-resolving would let the thing checked and the thing run
diverge the moment `harness_bin` gains a reason to answer differently on a second call. The rule
itself is pure and tested separately (`spawn_refusal` is reachable in a test where the spawn is
not): `the_driven_harnesses_run_and_the_rest_say_so`, which now loops `Harness::ALL` and asserts
both halves — a runnable grade refuses nobody, and `Unknown` refuses everybody — rather than
naming the harnesses by hand, so a harness added to the enum is covered the moment it compiles.
**There are two session models behind one channel, and the seam is `SessionMsg`.**
`start_ai_session` branches on `harness.is_persistent()`: Claude and Antigravity each keep one
bidirectional child per conversation, while Codex, OpenCode, Copilot and Cursor spawn **one
process per turn**, continuity coming from an id captured off `StreamEvent::SessionStarted`
(`thread.started`/`sessionID`, Copilot's closing `result`, Cursor's opening `system`/`init`) and
handed to the next turn as `TurnSpec::resume`.
**The interface upstream did not change**, and that is the point:
`AiSession::stdin_tx` carries a typed `SessionMsg::{Turn, Interrupt}` rather than the raw stdin
JSON line `main.rs` used to build, because that made the app's send path fluent in one CLI's wire
protocol — and a process-per-turn harness has no such protocol at all. The encoding now belongs to
whichever task owns the child. On the per-turn branch `Interrupt` mid-turn kills it and the turn is
closed by the not-`ended` arm; between turns it is a no-op, because nothing is running to stop.
**Nothing on the persistent branch is Claude's by construction any more.** It was written when
Claude was the only harness that reached it, so it took its argv from `build_session_args`, built
its stdin lines itself, and decoded with `parse_stream_line` — Claude's own decoder, called
directly. All three are asked of the harness now: `harness::session_args` for the argv,
`harness.session_turn_line` for each stdin line, and `StreamParser::new(harness)` for the stream,
which is also what brings the parser's turn-boundary reset onto this path and is what keeps a
second Antigravity turn's tool chips from being swallowed (see `stream.rs`). `owes_system` is the
fourth piece: `harness.session_system_in_first_turn()` decides it, and where it is true the schema
context is folded into the first turn's text and taken, so the outline is sent once for the life of
the process rather than with every question. And the launch-failure message names `harness.bin()`
and `harness.label()` instead of a hardcoded `claude`/"Claude Code", for the reason
`cli_failure_message` takes a harness at all: sending a user who picked Antigravity off to check
their Claude installation is a wrong cause stated confidently.
**Stop, on a persistent harness with no interrupt, kills and respawns in place.** Antigravity has
no stdin message that ends a turn in flight (`Harness::session_interrupt` is `None`, and the
measurement behind that is under `harness.rs`), so that arm kills the child, calls `pump.stop()` —
which sends the prose already accumulated, not a bare "Stopped." — and spawns a fresh child with
`session_args(…, resume: conversation)`, rebinding `child`, `stdin`, `reader` and the parser and
carrying on round the loop. The session therefore survives a Stop rather than ending on one, and
the id it resumes with is the one captured off `SessionStarted`, which on this branch fills only
for Antigravity because Claude's dialect emits none. If the respawn itself fails the session
**ends**: a task still holding `rx` with no child behind it accepts questions nothing can answer,
which is the silent-swallow failure `refuse_every_turn` exists further up to prevent.
**The MCP setup moved with the harness rather than being left where it was.** Claude still gets
`write_mcp_config`; Antigravity's `AgyRegistration` — the endpoint file, the `agy mcp add`
registration and the allow-rules — is gathered on this persistent path and installed inside the
session task on a blocking thread, exactly as it was on the per-turn path and for the same reason:
it is two `agy` invocations and a settings rewrite, and `start_ai_session` runs on the Floem UI
thread. The first turn's `rx.recv()` has not been reached by then, so nothing races it. The
per-turn branch's Antigravity arm and its `_registration` block were **deleted rather than left
unreachable**, because configuration for a harness that no longer takes that path is the copy that
quietly stops matching the one that runs.
**The per-turn child's stdin is piped exactly when the prompt travels on it**
(`Harness::turn_stdin_prompt`, which is `Harness::prompt_on_stdin` — Codex, Copilot and Cursor)
**and `Stdio::null()` otherwise**, and the old unconditional `null()` is the reason the "otherwise" has
to stay: measured, and still true on codex-cli 0.153.4, `codex exec` prints *"Reading additional
input from stdin…"* when stdin is a pipe at EOF, and appends piped stdin to the prompt as a
`<stdin>`
block, so a pipe nobody writes to would silently append itself to every turn — and so would a pipe
written to while the positional was also there, which is why the argv builder *drops* the
positional under the same predicate rather than duplicating the prompt. The write happens
immediately after the spawn and the pipe is then shut down, because the CLI waits on EOF to know
the instructions are whole.
**What a per-turn child is started as, and where, are not always `bin` and `cwd` — and the first
is resolved per turn.** Where is per session; *what* is not, because Cursor's launcher is the
newest `versions\<v>\` directory and the CLI updates itself into a new one, so a path resolved
when the session opened can name a version since replaced; one directory read per turn keeps
every spawn on the one that is there. `agent_cli::launch(harness, &bin)` gives the program and the arguments
that go ahead of the harness's own — for Cursor on Windows the newest version's `node.exe` and its
`index.js` rather than the `.cmd` that was found, for the reason under `agent_cli.rs` — and
`spawn_refusal`'s size check measures that program and that argv, prefix included, since that is
what is spawned. `child_cwd` is Cursor's workspace root, because its permissions file is found
from the working directory, and `cwd` for everyone else. **Cursor's one approval runs before the
first turn**, inside the session task: `cursor::approve` is a node start and a file write, so it
goes through `spawn_blocking` as Antigravity's registration does, and a failure becomes the
no-database-tools note rather than a refusal — the calls would be refused anyway, and the
permissions file that makes the session safe was already written. It is owed only when the
workspace configures a server (`CursorWorkspace::has_server`).
And every harness folds its events through **one**
`TurnPump` — prose and chips accumulate, a snapshot goes out when anything changed, `TurnDone`
closes the turn and resets, and `fail()` always sends so the panel cannot spin. Claude was
refactored onto it rather than left alongside: it briefly had its own copy, and two copies drift
on exactly the details a user notices — whether a half-finished turn renders, whether stats reach
the footer, whether the accumulator is reset at the boundary.
**`fail()` *appends* its reason to what streamed in rather than replacing it** — as the turn's
`TurnNotes` tail, through `end(segs, tail)`, not as one more segment (*`transcript.rs`*) — and
sending the reason alone is not a smaller version of that: the consumer assigns
`last.segs = msg.segs` (and `last.notes = msg.notes`) wholesale, so a one-segment message threw
away every word already on screen. That was invisible on
Claude, whose `result` event carries the accumulated turn so the final snapshot is complete, and
wrong wherever a turn ends by the process *exiting* — press Stop on a long Codex or OpenCode
answer, or on an Antigravity one, whose Stop kills the child because there is no interrupt to send
it, and the prose you were reading vanished, replaced by "Stopped.". The consumer's
own comment already promised the opposite, which is what made the two halves look consistent.
**`note()` is the pump's `lead`, held on the pump rather than sent as a `TextDelta`**: it rides
every snapshot of the turn it was put on and is cleared when that turn ends, because a snapshot
of its own would be erased by the next real one, the notes being assigned wholesale too. As a
delta it became the turn's prose and was replayed to the model as its own words. The senders
that answer with a refusal and nothing else — `refuse_every_turn`, `spawn_refusal`, `no_cwd`,
the isolation refusal, a session that failed to launch — send `segs: Vec::new()` with
`TurnNotes::tail(why)`, for the same reason. The session tests read a snapshot two ways,
`shown_of` (what the panel draws, notes included) and `text_of` (what a replay would read).
**The per-turn child is released before it is waited on, and the wait is bounded.** After a decoded
`TurnDone` the loop breaks and the child is reaped — but the loop owns the read half of stdout, and
a child that keeps writing past its terminal event fills the pipe and blocks in `wait()` forever.
`kill_on_drop` cannot help, because `child` is not dropped until `wait()` returns, so the task never
reached the outer `rx.recv()` again: the session accepted no further question and Stop could not
reach it either, the inner `select!` having already gone. So `drop(reader)` closes the pipe first,
and `tokio::time::timeout(CHILD_EXIT_GRACE, child.wait())` — ten seconds — kills a child that hangs
for its own reasons. The turn is already rendered by then, so the grace is only about reaping: long
enough that a healthy CLI flushing and shutting down is never killed mid-cleanup, short enough that
one which never exits cannot wedge the session against every later question.
**stderr has the same hazard from the other pipe, which is why `drain_stderr` is a function.** A
child whose diagnostics fill the pipe buffer — 64 KiB on Linux — blocks in `write` until somebody
reads, so reading only when it dies is a deadlock, and `kill_on_drop` cannot help because the
owner is blocked in `wait()`. It is a function rather than an inline spawn because the persistent
session creates a child **twice**: the Stop respawn above reassigns `child`, `stdin`, `reader`,
`parser` and `raw_output` and left `child.stderr` alone, so nothing read that pipe for the rest of
the session — and `stderr_buf` still held the *killed* child's output, which `cli_failure_message`
prefers over everything. A source gate counts the `drain_stderr(&mut child` sites so a third spawn
cannot quietly skip it.
**A dropped session is a turn nobody is listening to, and the per-turn loop distinguishes it from
a Stop with its own `abandoned` flag.** `switch_conn` sets `*ai_session.borrow_mut() = None` and,
eight lines later, replaces the panel with the connection switched *to* and clears `ai_busy`.
That `None` arm used to fold in with `Interrupt` and close the turn, which sends an
`AiStreamMsg { segs, done: true, is_error: true }` carrying **connection A's half-written
answer** — and that message carries no session or connection id, so the consumer applies it blind
to `v.last_mut()`: B's last assistant answer replaced by A's text, recoloured as an error, and
written into B's entry in `chats.json` by B's next `persist_chat`. Not a race — `switch_conn`
runs to completion on the UI thread, so the restored transcript is always already in place. The
persistent branch had always got this right (`None => break`, falling out to `child.kill()` with
nothing sent), and the comment beside the consumer names this exact hazard and then guards only
the *save*, which fails open because `ai_session` is `None` by then: nothing is written at that
moment and the corruption stays in the signal. So the per-turn branch kills and sets `abandoned`,
and the close is `if !ended && !abandoned` — a Stop still reports, a drop says nothing at all.
**That flag covered the turn's *terminal* snapshot and left the streaming ones, so every
`AiStreamMsg` carries the session that produced it now.** The consumer applies a snapshot to
`v.last_mut()`, and after a switch the session task is parked in a `tokio::select!` with **both**
arms ready — `rx.recv()` yielding `None`, and the reader yielding whatever the child wrote in the
meantime — which `select!` without `biased;` picks between at random. When the reader arm wins,
connection B's last answer is replaced by A's partial text and marked pending, so the bubble spins
for the rest of the session (`ai_busy` is already false and no further snapshot is coming) and the
corrupted message rides B's next `persist_chat` into `chats.json`. `AiSession::session_id` comes
from `next_session_id` — process-wide and monotonic, so an id is never reused, which a `conn_id`
could not promise here: switching away and back gives the same connection a *different* session
and the first one's stale snapshots would still match. The panel's consumer asks
`snapshot_is_current(live, msg)` and discards anything from a session it is not showing, `live`
being `None` once the switch has taken the session, so a snapshot arriving after that belongs to
nobody. An id closes the **class** where `biased;` would only narrow the window — a line already
decoded before the drop is still in flight — and it is its own function because the race has no
unit seam of its own, being an unbiased `select!` between a closed channel and a child's stdout,
which is also why the guard beside it is a source check.
**Two answers were not enough, because the guard `return`s above the whole effect body.** The
`false` arm skipped everything below it, `ai_busy.set(false)` included, so a session taken
*mid-turn* left the panel spinning for ever: the pending bubble kept its timer, the send box stayed
a Stop button, `ai_send` returned early on `ai_busy`, and the answer the CLI did produce was
discarded before `persist_chat` — recovery was Stop or New chat. `ai::SnapshotAction` /
`ai::snapshot_action(live, msg, done)` give the guard a **third** answer, `Settle`: not this
session's text, so the transcript is left alone, but nothing else is going to settle the panel, so
the flag is cleared. It is confined to "no session live" — if a *different* one is, `ai_busy`
belongs to that turn, and clearing it would strand the new turn exactly as this stranded the old
(`a_terminal_snapshot_with_no_live_session_still_settles_the_panel`). **The two takers reachable
mid-stream settle it themselves as well** — the AI settings page's apply, and editing the active
connection in place — because nothing guarantees a snapshot arrives after the drop at all: the drop
closes `stdin_tx`, the task breaks to `child.kill()`, and whether the reader got one more line out
first is a race. `every_taker_of_the_ai_session_answers_for_ai_busy` is the source gate over
`main.rs`, each exemption carrying its reason and with a floor, because the way this arrived was a
new taker that forgot: two of the nine sites did not answer, and they were the two reachable while
a turn was in flight.
**Stop's five-second safety net needed a counter of its own.** It fired on the global `ai_stopping`
alone and it is armed five seconds ahead, so a first Stop's timer fired on a *second* Stop pressed
inside that window and destroyed that turn's session — a process respawn and the whole multi-turn
context, three seconds before its own grace period was up, which is precisely what the interrupt
route exists to avoid. The session id cannot tell the two apart, the interrupt route deliberately
keeping the session alive, so `ai_stop_seq` is the counter that can.
The MCP subprocess gets its DB endpoint as JSON in `$SCHEMAIC_MCP_ENDPOINT` via a
per-session `--mcp-config` file (removed on drop) — never argv, so credentials don't leak
to other same-user processes. **That file, and every other per-session file carrying the endpoint,
lives in `ai::mcp_dir()` — `persist::private_dir("ai-mcp")` — and not in the shared temp
directory.** They held the DB host, user and **plaintext password** — the password is no longer
among them, see *the endpoint names its connection* below — and in a directory every account on
the machine can list the only thing between them and another user was `O_EXCL` plus a random name. `private_dir` is `0o700` on Unix and ACL-scoped to the profile on Windows;
`private_dir_in` is its pure half, separate so the *choice* of root is the part a test can pin.
`O_EXCL` is kept anyway, in the one `create_private(kind, ext, bytes)` that `write_mcp_config`,
`write_endpoint_file` and `inline_reply_path` all now go through: it refuses an existing path and
refuses to follow a symlink, and the mode a create applies never applies to a file that was merely
*opened*. **A harness Schemaic cannot hand a config file gets a *path*
instead of the blob, and still never the credential.** Codex's only configuration lever is
`-c key=value` and those are argv, so `mcp_endpoint_from_env` now prefers the file named by
`--endpoint-file <path>` and falls back to `$SCHEMAIC_MCP_ENDPOINT`; the path is not the secret,
what it points at was (`harness::codex_mcp_overrides` builds that override, and `main.rs`'s
`--mcp-serve` comment block states both routes and why neither is a plain argument).
`endpoint_file_arg` is the pure half, split out and tested because "the flag came last with
nothing after it" is the case that would otherwise read as a path and leave the server pointed at
a null endpoint (`the_endpoint_file_flag_is_read_only_when_it_has_a_value`, plus
`the_codex_override_names_the_file_and_never_the_endpoint`, which puts a real password through the
override on the app's side of the seam). **Antigravity takes the same route by a longer road**:
its server is registered *globally* with `agy mcp add`, so the command line holding
`--endpoint-file <path>` is written into that CLI's own config rather than passed per invocation —
still the path and never the endpoint, and `write_endpoint_file` puts the file in the same
directory, with the same prefix, as the Claude config so `sweep_stale_mcp_configs` already
collects it. It holds the same secret and must not outlive its session any longer. **OpenCode is on
that route for a third reason**: it is configured by a file, like Claude, but by a *reused* one, so
the `--endpoint-file <path>` in its `opencode.json` is what keeps the credential out of a directory
that outlives the session (`crate::opencode`). **Copilot is on it for OpenCode's reason**: its MCP
config *is* a file, but it lives in a `COPILOT_HOME` reused across sessions, so it carries the
path, and the endpoint file is what this session owns (`crate::copilot`). The two differ where the
endpoint file cannot be written. OpenCode's arm refuses, because its config is also its seal;
Copilot's degrades the way Codex's does — its seal is the argv's allowlist, which holds without a
server — to a server-less home (`CopilotHome::write_inline`, so the isolation survives) and the
same no-database-tools note. The allowlist still names the access level's tools there, and with no
`--additional-mcp-config` (nor, therefore, any `--allow-tool`) nothing provides them. **Cursor is
on the route for the same reason** — its `mcp.json` is in a workspace reused across sessions
(`crate::cursor`) — and without an endpoint file it degrades the way Copilot does, but for its own
reason: the permissions file is written regardless, because that is the half that cannot be
missing, and `mcp.json` is not (one an earlier session left is removed), so the session runs
with no server and says so. It is refused only when the workspace itself cannot be written, since
a missing permissions file fails open for writes. **The refusing half is `isolation_refusal(harness,
wrote)`**, pure: the harnesses that refuse when their directory could not be written are exactly
those whose restriction *is* a directory of ours — `env_seal` or `restricts_by_workspace_rules`
— and `a_harness_restricted_by_a_directory_refuses_without_it` holds that over `Harness::ALL`.
It was decided inline beside the three degrading arms, and turning Cursor's refusal into one of
them kept every test green.
**Which of those two routes a harness takes is one decision, `endpoint_plumbing(harness) ->
EndpointPlumbing { mcp_config, endpoint_file }`**, because it used to be two independent
`match harness` sites — one in each of `start_ai_session`'s branches — and that is precisely how
the two came to disagree. The persistent branch handed back `mcp_cfg`, which is `write_mcp_config`
for Claude and `None` for every other harness, so when Antigravity became persistent its endpoint
file (host, user and, then, plaintext password) had nothing to unlink it and accumulated one per session for
the life of the machine. What a session hands back is a `SessionPrivate { files: Vec<PathBuf>, cwd:
Option<PathBuf> }` rather than an `Option<PathBuf>`, gathered by a single `SessionPrivate::of` call
so neither carrier can be the one left out, and `AiSession`'s `Drop` unlinks every file and removes
the working directory — `remove_dir`, **not** `remove_dir_all`, because an agent CLI ran in there
and a recursive delete of whatever it left is not ours to do; the sweep collects the rest once the
owner is gone. `every_harness_carries_the_endpoint_exactly_one_way` walks `Harness::ALL` and holds
both halves at once: each harness carries the endpoint exactly one way, *and* whichever carrier
that is reaches `SessionPrivate`. The second half is the composition, and the composition was the
bug.
**`tools_missing(plumbing, wrote_config, wrote_endpoint)` asks whether the session that is about to
start has any database tools at all, and it asks it off the *plumbing* rather than the harness.**
`TurnPump::note`'s doc enumerates three ways to reach that state and says all three were silent;
four arms actually produce it, and the fourth had no arm at all — and it was Claude's, the default.
Claude's plumbing is an MCP config file, so when `write_mcp_config` returns `None` (an unwritable
profile directory) `build_session_args` emits neither `--mcp-config` nor `--allowedTools`, the
session has no database tools, and the system prompt goes on telling the model it can list the
schema, describe a table and run a query. No note, no `tracing::warn!`: the user watched the
assistant refuse to look anything up with nothing anywhere saying why. Asking the plumbing means a
fifth harness lands in the same question rather than adding a fifth silence, and the Antigravity
endpoint-file arm folds into it, leaving that `match` to do only what it is for. The Antigravity
*registration* failure stays separate and has to: it is known only after two `agy` invocations,
i.e. after an await — and so does Cursor's `mcp enable` failure, for the same reason.
`a_harness_that_lost_its_endpoint_plumbing_says_so_whichever_one_it_is` walks
`Harness::ALL` rather than testing Claude, because the defect was a harness nobody wrote an arm
for.
**And the subprocess reading that file fails closed.** `mcp_endpoint_from_env` returns
`Result<McpEndpoint, String>`; the decision is the pure `endpoint_blob(file, read, env)`, which
takes the filesystem and the environment as arguments because the only way to reach the real
function is to be launched as an MCP subprocess by an agent CLI — so the rule it applies would
otherwise be tested by nothing at all, which is how it came to have no rule. It used to fall
through: an unreadable `--endpoint-file` dropped to `None`, fell back to `$SCHEMAIC_MCP_ENDPOINT`
— which **only Claude's config sets** — found nothing, and handed `Value::Null` to
`endpoint_from_value`, which *defaults*. The server came up on `127.0.0.1:3306` with an empty user,
`samples: true` and `schema: true`, so a session the user had pinned to `AiData::SchemaOnly` or
`SchemaScope::None` started answering with sample rows and a full catalogue listing from whatever
local MySQL or MariaDB was listening — both access gates re-opened against a database nobody
authorised, and the trigger was not hypothetical, because the age-based sweep below could delete a
live session's endpoint file. **A missing field inside a blob that *did* parse still defaults, and
must**: those defaults are what every endpoint written before a given field existed relies on
(`an_old_blob_still_gets_its_back_compat_defaults`), so failing closed is about the blob being
absent or unreadable and not about it being old. `mcp.rs` is untouched by this — it destructured
the defaulted endpoint and started answering, so the fix belongs upstream of it — and `main.rs`'s
`--mcp-serve` arm prints the reason to **stderr**, because stdout is the JSON-RPC stream, then
exits 2 so the launching CLI surfaces the server as failed
(`an_endpoint_file_that_cannot_be_read_refuses_rather_than_defaulting`, which also pins that it
does *not* fall through to the environment, and `a_blob_that_is_not_a_json_object_refuses`).
**The endpoint names its connection and never its password.** `endpoint_json(db, conn_id, …)`
writes `"conn_id"` — the saved connection the `Db` was built from, which `StartAiParams::conn_id`
carries from `main.rs`'s active connection — and no `"pass"`; the subprocess puts the password
back itself, `mcp_endpoint_from_env` calling `resolve_password(&mut blob, keyring_password)`
before `endpoint_from_value`. It used to write the password, and owner-private was the whole of
its protection — which guards against other accounts and not against the agent, which runs *as*
the user. The harnesses whose built-in readers stay live could be steered to read the file and put
the plaintext DB password into the conversation, at every AI data level: Cursor's `Grep`, with
the path sitting in `.cursor/mcp.json` inside the model's own working directory; Codex's read-only
sandbox; Antigravity's `view_file`. The keyring is out of a file reader's reach, and since the
same blob rides Claude's config `env` map, that no longer holds the password either. **The
sign-in mode crosses as `"auth"`, and an Entra or cloud IAM token does not cross at all**: `endpoint_from_value`
hands the mode to `Db::with_auth`, an absent one being a password as every blob written before
the field meant (`the_endpoint_handoff_carries_the_sign_in_mode`), and the subprocess asks the
CLI for its own token through its own process's cache (`db/entra.rs`, `db/cloud_iam.rs`). **The connection's
init SQL crosses as `"init_sql"`**, written from `Db::init_sql()` and handed back to
`Db::with_init_sql`, so the assistant's queries run under the `search_path` or role the user's
own do; an absent one is none, which is what every blob written before the field meant
(`the_endpoint_handoff_carries_the_init_sql`). The subprocess's connects check it again, as
every connect does (`schemaic-db`'s init SQL paragraph).
`resolve_password` is pure with the lookup injected; `keyring_password(id)` is the real one —
`conn::secrets::load_connections_readonly`, **never** the app's self-healing load, which may
rewrite `connections.json` under the running app; the connection found by id; `hydrate_for_cli`,
whose notices go to stderr because stdout is the JSON-RPC stream. Host and port are still the
app's own SSH tunnel, so no SSH secret is needed on this side. An unreadable connections file or
an id not among the saved connections is an `Err`, and so the same stderr-and-exit-2 refusal as
above — an empty password would come back as the server's *Access denied*, which names the wrong
cause. A keyring that will not answer is **not** one of those: `hydrate_for_cli` reports it as a
notice and leaves the field empty, so that case still reaches the server with an empty password
and the reason is on stderr only. A blob with no `conn_id` is left alone, so an older blob's
`pass` still parses (`the_endpoint_names_the_connection_and_never_its_password`,
`the_subprocess_reads_the_password_by_the_connection_id`). **What this does not cover**: a
harness that can *execute* commands as the user can still reach the keyring — Windows Credential
Manager answers any process of the user — so this closes the file-reader path and not that one;
and on a machine with no working keyring the password is in `connections.json` as plaintext
already, the documented fallback, which is what `hydrate_for_cli` then reads. Two alternatives
were rejected: the password in each harness config's `env` map (Cursor's `mcp.json` is inside the
model's working directory, and Codex's only lever is argv), and a local credential broker handing
out a one-time token (the per-turn harnesses respawn the MCP server every turn).
**The sweep that collects what a crash left behind asks liveness, not age.**
`stale_mcp_file(name, owner_live, age)` and `stale_run_dir` both parse an `Owner` out of the name —
one parser, `owner_in(name, prefix)`, since the owner sits immediately after the prefix in both
shapes — and hand it to `liveness::may_sweep`. `MCP_STALE_AFTER` (still 24 hours) is now only the
fallback for a name carrying **no** owner: one written by a build older than the naming, or by a
process whose start time could not be read. Age alone was wrong in both directions at once. It
deleted a *live* instance's endpoint file — a session older than a day plus a second window was
enough, and `modified()` on a write-once file is its creation time and never advances — while its
`.json` requirement meant `inline_reply_path`'s `.txt` could **never** be collected, though the
sweeper's own doc said it was, and an existing test pinned that denial against a hand-written name
no caller produces. `sweep_stale_mcp_configs` sweeps `mcp_dir()` **and** the old temp location,
because an upgrading user has plaintext passwords sitting there under the old ownerless names — the
case the age fallback exists for. Five tests hold it:
`a_live_sessions_files_are_never_swept_however_old_they_get`,
`every_kind_of_file_this_module_creates_can_be_swept`,
`a_file_with_no_owner_falls_back_to_the_age_rule`, `nothing_that_is_not_ours_is_ever_swept` and
`a_run_directory_outlives_its_owner_and_no_longer`. Pure clusters
split out: `agent_cli.rs` (agent-CLI
discovery — env override / known install locations / PATH+PATHEXT — plus the one part of it that
is *not* pure, `probe`, which spawns `<bin>` with that harness's `help_args` and caches what it
reads per
`(harness, path)`; its own entry is below), `antigravity.rs` (that CLI's two pieces of global
state, also below), `liveness.rs` (whether the process that left something behind is still
running, which `antigravity.rs`'s sweep and `ai.rs`'s own sweep both ask through `may_sweep`, and
`opencode.rs`'s and `cursor.rs`'s ask more directly through `process_start` — also below),
`opencode.rs` (the config directory that harness is sealed by, and the
environment pointing it there — also below), `copilot.rs` (the `COPILOT_HOME` that harness is
isolated by and the MCP config inside it, on `opencode.rs`'s per-instance roots — also below),
`cursor.rs` (the working directory that harness is restricted by, its one MCP approval, and the
sweep of what the CLI keeps about that directory in the user's own Cursor config — also below) and
`ai.rs` (`AiSession`/`start_ai_session` streaming,
MCP-config plumbing, `ai_context`/`inline_system_prompt`). Reactive wiring (`app_view` closures)
stays in `main.rs`.
**Every prompt's database list comes out of one funnel**, `snapshot_databases`: it reads the
schema-tree signals once into `(database, Some(schema))` plain data, and a database the SCHEMA
eye has hidden is not in it — not its name, not its tables, not its columns — bar the database
being worked *in*, the `db_contributes` exception autocomplete makes for the same reason. Both
the chat panel's context (`turn_context`) and Ctrl+K's generator (`inline_system_prompt`)
snapshot through it, so neither can be filtered while the other isn't. The filtering half is
`visible_snapshot`, split out over plain data because that guarantee was enforced by reading
alone: the funnel takes two signals, so no test could call it, and the two renderers it feeds
take an already-filtered slice, so theirs never saw a hidden set at all. `db_contributes` was
tested in core; *that this funnel is what calls it* was not.
**Ctrl+K's prompt fences and labels the editor buffer and the selection** the way the chat panel
does — `prompt::fenced_as("sql", …)` plus `UNTRUSTED_NOTE` — because they are the same
provenance: *Generate DDL* pastes introspected `CREATE TABLE` into a tab, so a column `COMMENT`
from a server the user does not control lands there. The chat panel's block was fenced and
labelled while this one, three functions away, was spliced raw immediately before the instruction
block, and Ctrl+K's output goes into the editor one Ctrl+Enter from running
(`the_inline_editor_block_cannot_be_closed_from_inside_it`, which also counts the fences so a
payload's own three-backtick line reaches no margin). And at `SchemaScope::None` the
withheld-schema note tells the model to use **only what the editor already names** rather than to
ask the user: Ctrl+K is one one-shot spawn with no stdin and no session, under a preamble whose
first sentence is "Output ONLY SQL — no prose", so a question is the one thing it cannot carry
out. Given an instruction it cannot obey beside one it can, it obeyed the one it could and
invented `orders(placed_at)`, and the invented SQL landed at the caret with nothing on screen
marking it as ungrounded (`the_withheld_schema_note_is_something_a_one_shot_can_do`). The chat
panel's wording is right *there*, where the model can answer back.
**One spawn shared by all three one-shot features, because they had drifted apart.** `inline_plan`
resolves the selected harness into a runnable `InlinePlan` — binary, argv, where the reply comes
back from, and the environment an `env_seal` harness is isolated by — or hands back the reason it
will not run, and `run_inline` executes it. Each of Ctrl+K, AI Fill and AI Seed carried its own copy of that before: two passed
`Stdio::null()` and Ctrl+K did not, so Ctrl+K alone paid the CLI's wait on a stdin that was never
going to arrive. That null is now conditional for the same reason the session path's is —
`InlinePlan::stdin_prompt` is `Harness::inline_stdin_prompt`, so the pipe exists only where the
prompt travels on it — and where it does, `run_inline` **`spawn`s rather than `output()`s**:
`output()` closes the child's stdin before anything can reach it, so the prompt would never arrive
and the CLI would answer an empty instruction. `stdout`/`stderr` are then stated explicitly, since
a bare `spawn` inherits them and would put the CLI's output on Schemaic's own stdout while
`wait_with_output` read two empty buffers. It takes the effort setting alongside the model now — each of the three call sites
passes `ai_effort.get_untracked().cli()`, and `harness::inline_argv` is what clamps that to the
selected harness's own vocabulary. **It also carries the gate the inline paths never had.** They read the probe for
Claude's seal flags and spawned whatever it said about the *constraint*, which was survivable while
they were Claude-only — an unsealed Claude is a refusal at `start_ai_session`, and inline never
reached that function — but a one-shot can be Codex or Antigravity now, whose constraint *is* the
sandbox flag, so `spawn_refusal` is asked here for the same reason it is asked there, and the
oversize check sits beside it — measuring what will really be spawned, since `agent_cli::launch`
is resolved first and its program and prefix replace the bare binary in the plan (Cursor's node,
on Windows). `inline_reply_path` is Codex's `-o` file, in `mcp_dir()` and named
with `MCP_FILE_PREFIX` so `sweep_stale_mcp_configs` collects it if the process dies between
the spawn and the read; `run_inline` reads and removes it whatever the exit status, so a failed run
that still wrote one leaves nothing for the sweeper. **And so does the oversize refusal**, which
used not to: the path has to exist to go into the argv that check measures, so `inline_plan`
created one and then refused, orphaning a file every time. It is removed at the refusal rather than
left to the sweep, which only collects once the owning process is gone.
**`run_inline` runs its child in a working directory of its own too**, `InlinePlan::cwd` from the
same `session_cwd` — or, for Cursor, the instance's `cursor-inline` workspace (`inline_cwd`,
below) — removed when the generation finishes. The removal is `remove_dir`, empty-only, and on
Cursor that is doing real work: its workspace is one directory shared by every one-shot of the
instance and is never empty, so it survives for `cursor::sweep`, where a recursive removal would
take the permissions file out from under a concurrent generation. It used to set no
`current_dir` at all,
so the child inherited the app's *own* process working directory: a user who launches Schemaic
from a world-writable directory gave any local account a `.claude/settings.json` whose `hooks` run
as them on the next Ctrl+K, and Claude — the default harness — has only the cwd standing between it
and that. It also clears OpenCode's config variables **before** setting ours, the order the session
path states in a comment and this one had backwards, and clears them unconditionally rather than
behind `harness == Harness::OpenCode`. **That list is `inherited_env_to_clear()` now, one function
both spawns call**: OpenCode's `env_remove` chained with Copilot's, so the one-shot and the chat
turn cannot come to clear different sets. It used to be OpenCode's list alone, which was complete
only while OpenCode was the only harness isolated by its environment.
`no_seal_variable_is_also_cleared` pins the disjointness the clear-then-set order leans on — no
lever (`XDG_CONFIG_HOME`, `OPENCODE_DISABLE_PROJECT_CONFIG`, `COPILOT_HOME`) is ever in the list,
since a lever there would strip its own harness's isolation while the panel reported it sealed —
and that both lists are in it, not only the first. (A comment here already cited that test by
name before it existed; it exists now.)
**Which environment is built is `inline_env(harness)`, and it asks `env_seal` and then matches the
harness exhaustively.** It used to be `match harness.env_seal() { true =>
OpenCodeConfig::write_inline() … }` — the capability asked, then one harness's writer assumed — so
the moment a second harness answered `true` it would have been "isolated" by OpenCode's config and
nothing of its own. Now OpenCode gets `OpenCodeConfig::write_inline`, Copilot gets
`CopilotHome::write_inline`, and a failure to write either **refuses the generation** rather than
running it bare, each for its own reason: OpenCode's `--agent` would fall back to `build` and its
`bash`, Copilot would start the user's MCP servers and load their hooks. The arms for the
harnesses whose `env_seal` is false **refuse too** rather than return an empty environment — they
are reached only if one is flipped to `true` without a writer here, and that has to fail closed
instead of running unisolated. `a_harness_not_isolated_by_its_environment_gets_an_empty_one` is
named for the half it covers — the empty environment for the harnesses whose `env_seal` is false —
because the other half needs the real config directory to exercise; what holds the refusing shape
is the exhaustive `match`, not a test.
**Where the one-shot runs is `inline_cwd(harness)`, the same shape one question over.** It asks
`restricts_by_workspace_rules`: for every harness that answers `false` it is a fresh
`session_cwd()`, as before, and for Cursor it is `CursorWorkspace::write_inline()`'s root — or a
**refusal**, never a bare private directory, because without the permissions file a headless run
writes files unprompted. The arms for the other harnesses refuse, so one flipped to `true` with no
writer here fails closed. `inline_plan` asks it after the size check, and a refusal there removes
the reply file as the size refusal does.
**What comes back is gated, not trusted**: `inline_outcome` runs `extract_sql` (fences off) and
then `intel::sql_reply` (the parse gate above), and a reply that will not parse becomes
`Failed("The model did not return SQL")` rather than an edit. It takes the runner's
`Result<String, String>` rather than the raw `(success, stdout, stderr)` it used to, because reading
the reply is no longer one thing — five harnesses answer on stdout and Codex answers in a file —
and that decision belongs with the argv that made it. The composition is what the caller
relies on, so it is pinned *here* as well as in `intel` —
`inline_outcome_drops_a_tool_diagnostic_riding_on_the_sql` puts the chatter inside the fences,
where neither function alone would have to deal with it, and
`inline_outcome_refuses_a_reply_that_is_only_prose` holds the refusal.
`render_ai_context` also tells the model how a **schema change** reaches the
user — call `propose_table_change`, then echo the JSON in a `FENCE_TAG` block — spelled out in
the prompt as well as in the tool's own description, because the shape it replaces (an `ALTER` in
a code block for the user to run) is the one every model reaches for by default. It is stated
whether or not queries are allowed, since proposing reads the schema and that was never the gated
part, and the tag comes from the constant the renderer reads
(`the_prompt_names_the_fence_tag_the_renderer_reads`). Its **tools line** names the tools the
connection's `AiData` level leaves the session: `list_databases`, `list_schema`,
`search_objects` and `describe_table` at every level, `sample_rows`, `run_query` and `explain`
at `Full` alone; and at *Schema context: None* the withheld-schema sentence names
`list_databases`, `list_schema`, `search_objects`, `describe_table` and `sample_rows` as
unavailable. The harnesses' allow-lists need no such edit when a tool is added, being derived
from `McpTool::ALL` and `offered`. The MCP server itself is `cli/mcp.rs`
now, under `schemaic-cli` below — its tools, its gates, its deadlines, and how `ai_allowed_tools`
is derived from them. What stays here is the blob the panel hands it: `McpEndpoint` is an alias
of `schemaic_cli::mcp::Endpoint`, and `endpoint_from_value` builds one as `Host::Panel`, only the
app ever writing a blob. `conn/secrets.rs` is the
keyring-backed `SecretStore` behind `core::secrets`.
