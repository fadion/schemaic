# `app/antigravity.rs`

`antigravity.rs` — the two pieces of that CLI's **global** state a Schemaic session needs, owned
for the life of the session and taken back out again. It is the only harness configured by
writing into the user's *own* files — the others take a flag, a `-c` override, or (OpenCode) a
file in a directory Schemaic owns — which is why this is a module rather than four lines in
`ai.rs`. `AgyRegistration::install` claims the state, runs `agy mcp add` and
merges the connection's `permissions.allow` rules into `settings.json`, and its `Drop` removes
all three. The `--` ahead of the command is load-bearing rather than punctuation: the arguments being
registered start with `-`, and `agy mcp add` rejects a flag placed after the server name. **Both, together, in both directions** — a registration without rules is a server whose
every call is refused, and rules without a registration are a standing grant for a server that
is not there, which is why `install` folds the settings result into `installed`: a registered
server whose rules were declined is withdrawn again rather than reported as a working session,
so `is_installed()` stops answering `true` for a session whose every call that CLI refuses. A
failed install removes **nothing** on drop, since tearing down a registration this session never
added would take out a working one belonging to somebody else — one of the three ways
`may_release` answers no, and `a_failed_registration_removes_nothing_on_drop` asks it directly
now: the version that asserted only "does not panic" passed with the guard deleted, because on
CI there is no settings file and a fake `agy` cannot spawn. The rules come from the connection's own
allow-list, so a schema-only connection never grants `run_query` and `run_command` is never
granted at all; the surgery itself is `schemaic_ai::harness`'s pure, tested half and this module
only does the IO.
**`install` runs on a blocking thread; `Drop` deliberately does not.** It is two `agy`
invocations and a settings rewrite — a Node CLI start, a config parse and a write back, seconds
rather than milliseconds — and `ai::start_ai_session` runs on the Floem UI thread, so doing it
inline froze the window for all of it, on top of the `probe` immediately above. Only the
*arguments* are gathered at that call site now; the work happens in a
`tokio::task::spawn_blocking` inside the session task, ahead of its first `rx.recv()`, so
nothing races the registration the first turn needs.
`Drop` stays blocking on purpose and the reason is stated on the impl: it runs
on a runtime worker with nobody waiting on that thread, and handing the removal to a detached
thread races process exit with the one piece of work that must not be skipped — a standing
permission grant left behind in the user's own config.
**`sweep` runs at startup, off-thread, because a crash leaves both behind and neither expires.**
It clears the full tool set rather than the current connection's, since a crashed session may
have granted more than this one will
(`the_sweep_targets_every_tool_not_just_the_current_level`), and it removes the rules even with
no `agy` on the machine — that half is our own file surgery, while the `agy mcp remove` half is
skipped unless the binary is actually there, rather than shelling out on every launch to clean
up state that cannot exist. **And unless a dead owner's claim is on disk** (`sweep_plan`,
`the_sweep_starts_agy_only_on_a_dead_owners_claim`): it used to shell out at every launch
whenever `agy` was found, for a user who had switched the assistant off too, against the
settings modal's promise that no agent CLI is started while it is off. `install` claims before
it registers and `Drop` releases only after it removes, so a registration of ours is never on
disk without a claim beside it — no claim, nothing of ours to remove.
**And withdrawing never creates the file it withdraws from**, which is what
`settings_to_edit(create, existing)` decides for `edit_settings`. Granting passes `create: true`
— a fresh Antigravity install has no settings file yet and the rules are what its tools need to
run at all — while withdrawing, meaning both the `Drop` and this sweep, passes `false`. The
sweep ran unconditionally at every launch, and for everyone who has never installed that CLI the
sequence was: the read fails, the pure layer takes an empty string as an empty document,
removing nothing from it yields `{}`, that differs from what was read, and Schemaic writes `{}`
into a directory it creates inside Google's config tree. An app that has never run that CLI has
no business leaving a file where it keeps its settings.
`withdrawing_rules_from_a_file_that_is_not_there_writes_nothing` drives the composition rather
than the predicate, because the composition is what bit.
**The settings write is atomic, and its answer is used.** `edit_settings` writes through
`persist::write_file_atomic`, whose own doc opens by naming the failure it exists for:
`fs::write` truncates before it writes, so a full disk, a dropped network share or a crash
between the two leaves the file empty — and this is another vendor's configuration, in a
directory Schemaic does not own and cannot regenerate. It returns whether the file now says what
it was asked to say, which includes the `harness::SettingsEdit::Unchanged` case where it already
did and nothing is written at all. All three callers used to discard that answer, so
`is_installed()` could report `true` with no rules granted, and a withdraw that failed or was
declined — an unwritable file, an unparseable one — left a standing `run_query` auto-approval in
the user's configuration permanently and silently. A failed withdraw is a `tracing::error!` now,
at `Drop` and in `sweep` both, because the state it leaves is the one this module calls the bad
one — rules standing with no registration — and it is permanent: the next launch's sweep fails
on the same file the same way.
**The sweep now asks whether the leftovers are leftovers, and what it used to be was a live bug
rather than a stated limit.** It ran at every launch and removed the registration and the
allow-rules unconditionally, so launching a second Schemaic pulled the database tools out from
under a *live* first instance's running session, which then held a server whose every call had
silently stopped existing. `AgyRegistration::install` writes a `Claim` **before** it installs
anything and the sweep consults what it wrote, returning early while the session named there is
still running; when it does decide to sweep, it clears that stale marker along with the state.
The liveness half of that decision is not this module's any more: `Owner`, `process_start` and
the pure `may_sweep(marker, live) -> bool` live in `liveness.rs`, where the other sweep that
asks the same question can reach them. What stays here is the `Claim { owner, nonce }` the
marker records, `claim_text`/`parse_claim` either side of it, and the pure
`may_release(mine, on_disk, installed)`
(`a_second_instance_does_not_sweep_a_live_instances_registration`).
**The start time is what makes the claim safe, and a pid alone would not have been.** Pids are
reissued, so a crashed instance's marker eventually names some unrelated live process, and a sweep
trusting the number would decline to clean up for the rest of that pid's life — turning a
transient crash into exactly the permanent standing grant this module exists to prevent. A pid
that *is* running but started at a different time is therefore swept, and an absent **or
unreadable** marker is treated as no claim at all, in that same direction and for the same
reason: a truncated file must not be able to strand a permission in the user's config with nothing
left able to withdraw it (`a_crashed_instances_registration_is_still_swept`,
`an_absent_or_unreadable_marker_is_not_a_claim`, `our_own_live_claim_survives_our_own_startup_sweep`
for the marker naming this very process at startup — the old name for that test,
`our_own_stale_marker_does_not_stop_us`, asserted the opposite and encoded the bug `liveness.rs`
was extracted over — and `a_claim_round_trips` over the three-field line that is the whole
interface between the writer and the reader).
**The nonce is the third field, and a respawn is what needs it.** Changing a setting respawns
the AI session: `ai_send` used to start the new one first and drop the old one afterwards —
assigning over `ai_session` is when the old `Drop` ran — on a
different worker with no ordering between them, so the old session's `Drop` regularly ran *after*
the new `install` and removed the registration, the rules and the marker the new session had just
put in place — same pid, same start time, so an `Owner` alone could not tell the two apart, and
the new session then ran with every database tool refused while `is_installed()` said `true`.
With a nonce the ordering stops mattering, which is the only fix available: whoever claims last
owns the state, and a `Drop` whose nonce is no longer the one on disk removes nothing
(`the_previous_sessions_teardown_does_not_disarm_the_one_that_replaced_it`).
`ai_send` takes the old session out **before** it calls `start_ai_session` now, which shortens
the window without being the guarantee — `install` is handed to a blocking thread inside the new
session's task, so the old teardown can still land after it — and the comment at that call site
says as much, so nobody reads the reordering as having replaced the nonce.
**`may_claim` is the other half of that, and the nonce could not have covered it.** The marker
is one slot and `claim` used to overwrite it unconditionally: two Schemaic windows on
Antigravity, A installs and claims, B installs and claims over A's marker, and B's *New Chat*
then drops its session, sees `on_disk == mine` and runs `agy mcp remove schemaic` and withdraws
the allow-rules — machine-global state A is still using. A's session is untouched in memory, so
`is_installed()` still says true and the panel still reports its constraint, while every
database tool call in A is refused; a denied Antigravity turn reports `"status":"SUCCESS"` with
an empty response, so the user watches the assistant silently stop using the database with no
error anywhere. That is verbatim the failure this module's header says it fixed — *"a second
window's session ending disarmed a live one just as surely as its starting used to"* — and the
nonce cannot see it, since it solves two sessions in **one** process, where the pid and start
time are identical, and here they differ. So: no claim on disk, take it; our own process, take
it (a respawn, and the nonce is what keeps the outgoing teardown off the new one); another
process still running, **refuse**; another process gone, or its pid reissued to something with
a different start time, take it. Declining to claim is safe either way, because the startup
sweep clears the full tool set for a crashed owner. What a session got is `Claimed`, three
states rather than a bool: `Mine(claim)` cleans up on teardown, `Shared` gets **no tools** and
withdraws nothing on the way out, and `Refused` installs nothing at all. `Shared` used to
register and grant anyway, on the premise that "both are idempotent upserts of the same state" —
false for the registration, `agy mcp add` being an upsert of the **one machine-global** entry
whose `--endpoint-file` names *that window's* connection. The second window repointed the shared
registration at its own endpoint, the first window's next `agy` respawn read the registry and
began answering questions about connection A by querying connection **B** with B's credentials,
and when B's session ended `AiSession::drop` removed B's endpoint file, leaving the registration
naming a file that is gone. A registration that names one endpoint file cannot serve two
connections, so the second window says it has no database tools rather than quietly taking the
first one's.
**And the claim is read back, because the write is atomic and the decision is not.** `may_claim`
reads the marker, `write_file_atomic` replaces it, and nothing holds the file between the two:
two windows starting an AI turn at the same moment both read "no live claim", both wrote, and
both returned `Mine` — both then running `agy mcp add` against the machine-global registration,
which is the hijack this marker exists to prevent, while `release` correctly declined to remove a
marker that was not its own so the loser withdrew nothing and the winner's grant outlived them
both. The rename is atomic, so exactly one marker survives, and the nonce says whose:
`claim_outcome(mine, settled)` answers `Mine` only when the claim on disk *is* this session's and
`Shared` otherwise, which is already the honest answer for a loser. It is split out of `claim`
so the decision has a test, everything around it being a filesystem and two processes. **What
this does not close**, stated rather than implied: two writes that both land before either
read-back still both see themselves, and the second rename orphans the first's marker — the
window is between one process's rename and its own next read rather than the whole of
`agy mcp add`. Closing it outright needs an `O_EXCL` create, which the takeover path (a marker
left by a dead process) cannot use.
**The claim goes first and is released last, and the first half is what changed.** It used to be
written *after* `agy mcp add` and after the grant, so from the instant the registration landed
until the claim returned there was machine-global state that no marker accounted for, and another
launch's sweep landing in that window removed it out from under a live session; a claim
protecting state it was written after protects nothing. It is written through
`persist::write_file_atomic` for the reason the marker's reader is another process: `fs::write`
truncates first, so a sweep landing between the truncate and the write read `""` or a prefix,
`parse_claim` answers `None` for both, and a torn write by a *live* instance was read as no claim
at all. **A claim that cannot be made means no install at all** — without one there is nothing to
stop another instance's sweep withdrawing the grant mid-session and nothing to tell this session's
own teardown whether the state is still its own, so the session runs without database tools and
says so. `Drop` still releases last, after both removals, because releasing first would open the
mirror window in which another instance's sweep could race this one's own teardown, and
`release(mine)` removes the marker only while the marker is still the caller's. The marker lives at
`persist::private_dir("agy")/registration-owner` rather than in the shared temp directory: it is a
claim a sweep obeys, and world-writable is the wrong permission for that.
**And `Drop` asks `may_release` now, where it consulted nothing at all.** It performed the same
three removals as the sweep eighteen lines away without asking whose state it was, so a second
window's session *ending* silently disarmed a live one just as surely as its starting used to — a
claim that governs one remover and not the other governs nothing. `may_release` answers no three
ways, each a bug that happened: a session that never installed, a session that never claimed, and
a claim that is no longer the one on disk.
**A limit recorded rather than guarded: the grant is user-global while it stands, and cannot be
narrowed.** `agy mcp add` writes into the user's own MCP config and `permissions.allow` is an
allow-list rather than a prompt-list, so for as long as a Schemaic AI session is open, *any*
`agy` run by this user — in any directory, started by anything — finds the `schemaic` server
registered and its database tools pre-approved, with no prompt:
`cd ~/work/some-repo && agy -p "explain this build failure"` can reach the database through a
`README` or `AGENTS.md` the user did not write. No lever was found that scopes either half to one
process; the registration is per user by construction, and a per-session server *name* was
considered and rejected, since an unrelated run inherits whatever name is registered and it would
therefore close the *next* limit rather than this one. What is left is to keep the window as
narrow as it can be — installed when a session starts, withdrawn when it ends, covering only the
tools that connection's access level offers, so a schema-only connection never grants
`run_query` — and to say so rather than let it be assumed shut, which is why
`harness::Constraint::notice` tells the user in the panel that this harness's MCP surface is not
Schemaic's to restrict. The `inline_argv` entry above bounded the same exposure in *time* and
never asked which other processes were in that window; it says so now.
**A second limit, recorded rather than guarded: the server name is not ours to reserve.**
`agy mcp add` is an upsert and `agy mcp remove` is unconditional, so a user who has registered
their *own* MCP server under the name `schemaic` — or one pointing at a different Schemaic build
— has it replaced on the first AI turn and deleted when that session ends (or by the next
`sweep`, if it crashed). Telling ours from theirs
needs a read of that CLI's registry (`agy mcp list`) whose output nobody has measured, and
guessing at a format in order to decide whether to delete somebody's configuration is worse than
the collision.
**And the name really is one constant now — the claim that it always was is what made this worth
fixing.** This paragraph, and `SERVER`'s own doc, said "a single `SERVER` constant, so a future
check has one place to hook" while `SERVER` was a private `const &str = "schemaic"` here and
`harness::antigravity_allow_rules` emitted the literal `mcp(schemaic/…)` in a crate that cannot
see it: two independent spellings of one name, documented as one. Renaming either half leaves the
registration standing while every rule points at a server that is not there, which Antigravity
answers by *denying* the call — and a denied turn still reports `"status":"SUCCESS"` with an
empty response, so the failure would have had no symptom at all. `SERVER` is now
`schemaic_ai::harness::MCP_SERVER`, and
`the_allow_rules_name_the_server_that_was_registered` asserts the **composition**
(`rules == vec![format!("mcp({SERVER}/list_schema)")]`) rather than `SERVER == MCP_SERVER`,
because the rule string is what `agy` matches the registration against.
The settings file's location is `~/.gemini/antigravity-cli/settings.json`, and **the `.gemini`
is not a leftover from the harness that was removed**: `agy` is Google's and keeps its own
settings under the same home-directory root the Gemini CLI used, which the function's own doc
comment now says so nobody tidies it into `~/.antigravity`. The path was **observed, not
documented**, so `$SCHEMAIC_AGY_SETTINGS` overrides it: if that CLI moves the file, the failure
should be "no database tools" and not "wrote into the wrong file"
(`the_settings_path_is_overridable_for_a_cli_that_moves_it`, which asks the pure
`settings_path_from(override_var, home)` rather than mutating the process environment as it used
to — `unsafe { std::env::set_var }` in a parallel test binary whose siblings read the environment
on other threads is documented UB, and it hard-*removed* a documented user-facing override
instead of restoring it). `agy mcp` calls are best-effort by
design — a missing or refusing binary costs the session its database tools, which the failing
tool call already reports, where failing the whole session over a config write would be worse;
a registration that did not take is `tracing::warn!`ed rather than left to be discovered.
