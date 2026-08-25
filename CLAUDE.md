# Schemaic — working notes for Claude

A native SQL editor (Rust + [Floem](https://github.com/lapce/floem) 0.2.0), MySQL/MariaDB-first,
Zed-inspired, aiming to replace DataGrip. Workspace crates: `schemaic-core` (models + the pure,
unit-tested SQL/edit/export/DDL logic), `schemaic-db` (MySQL/MariaDB + PostgreSQL + SQLite + SSH
tunnels), `schemaic-ai`, `schemaic-term`, `schemaic-ui` (the Floem views), `schemaic-app` (signal
wiring, the built-in MCP server).

**Three engines, and they are not equal.** MySQL/MariaDB and PostgreSQL are full; SQLite reads,
writes, imports and edits **tables** (through the twelve-step rebuild — `ddl::sqlite_rebuild_sql`),
**views** and **triggers**, but has **no manual-transaction mode** — a statement about SQLite
rather than unfinished work (`db::session::Session::open` carries the reason). What differs between the
engines now lives in the *narrow* predicates that decide how an edit is performed rather than
whether it is offered: `ddl::supports_or_replace_view`, `supports_view_rename`,
`supports_column_reorder`, `supports_change`, `alter_column_disturbs_checks`,
`stats::supports_table_stats`. Ask a **capability**, never an engine: a `dialect == Postgres` or
`!= MySql` compiles cleanly while silently sorting a third engine onto whichever side it happens to
fall — and a *constant* in place of a capability is the same failure with no comparison to grep for,
which is why the predicates that do answer the same for all three engines today
(`supports_view_editing`, `supports_trigger_editing`, `supports_table_design`) *compute* that answer
from `supports_change` rather than returning `true`.

**`docs/architecture.md` is the reference document** — the module map (one entry per source file),
the architecture invariants, the UI conventions, the Floem hazards, and the data grid end to end.
This file holds only the working rules. Keep the two disjoint: a fact about the system goes there,
an instruction about how to work goes here.

**Keep it honest as you work, not afterwards.** It is the map every contributor and every session
reads, so silent drift from the code is the most damaging kind of bug there —
`core/tests/doc_coverage.rs` catches a module nobody wrote down, and nothing catches a paragraph
that has quietly become false. Route the write through `arch-scribe` when a change lands.

**`TODO.md` is the user's scratchpad — a plain list of things to be done, and the place to park an
idea for a future release.** It is gitignored, so it is also the one file here with no git history
to recover from: back it up before a large rewrite, and never edit it with a script. Entries are
short and imperative; delete them as they land rather than checking them off. **It is not a
decision board.** The moment an entry starts explaining *why* something was chosen or rejected, that
paragraph belongs in `docs/architecture.md` (or the commit that supersedes it) and the entry here
shrinks back to the work that is left. Keep the three disjoint: a fact about the system goes in
`docs/architecture.md`, an instruction about how to work goes in this file, an open piece of work
goes in `TODO.md`.

## Delegate the reading (`.claude/agents/`)

`docs/architecture.md` is ~3.4k lines and several modules are thousands each (`ui/grid.rs` ~6.3k,
`ui/lib.rs` ~5.6k, `app/main.rs`), so paging them into the main context is what runs a session out
of room. Three subagents exist to do that reading in their own windows:

- **`scout`** — "where is X wired", "how does feature Y flow across the crates", "what does the
  architecture doc say about Z". Read-only; returns `file:line` citations and a conclusion, never a
  transcript. **Use this instead of reading `docs/architecture.md` yourself**; page in a section by
  hand only when you are about to edit it, or when the citation isn't enough.
- **`locate`** — a pinpoint symbol lookup when you want only the locations.
- **`arch-scribe`** — makes the `docs/architecture.md` edits a finished change requires, in the
  document's own voice. Give it what changed and why. It checks the brief against the code before
  writing and reports where the two disagree, so read its closing notes rather than treating them
  as a formality.

Editing a module you are actively designing in still belongs in the main loop.

## The invariants — don't regress these

Each is stated in full, with the bug that motivated it, under *Architecture invariants* in
`docs/architecture.md`. This index exists so you know a rule is there to be read; it is **not** a
substitute for the statement, and none of these is a style preference.

- **The write guard lives on the run action**, not in a caller of it — every path executing user
  SQL goes through `TabsActions::run`/`run_all` and `sql::run_verdict`, or through a refusal
  *strictly stronger* than it (`sql::rerunnable_for_export`, which has no `Confirm` arm). Never a
  second, laxer gate.
- **One SQL boundary lexer** — everything scanning SQL for string/comment/quote boundaries builds
  on `core::sql::skip_noncode`, and it is dialect-aware.
- **Structure-aware SQL analysis goes through `core::intel`** (a real per-dialect AST), not a new
  hand-rolled scanner. The DB stays the semantic authority.
- **One connection per operation** — every `Db` method connects, runs, disconnects. The single
  exception is a `TxMode::Manual` tab's pinned `Session`.
- **Connection identity is the `Db` handle / `conn_id`**, never a `mysql://user:pass@host` URL. No
  credential in a URL, argv or log.
- **Connection secrets persist to the OS keyring**, not `connections.json` — saves route through
  the app's `secrets::{load,save}_connections`.
- **Own per-entity signals in a child `Scope`, and dispose it *deferred*** (`exec_after(ZERO, …)`).
- **Themable colours reach reactive styles as `fn() -> Color`**, never a captured `Color`.
- **Pure logic lives in `schemaic-core` with unit tests**; the UI/app keep thin wrappers.
- **Generated DDL is never run silently, and never emitted from a second differ** — draft →
  `ddl::diff` → `emit` → the preview modal → `Db::run_ddl`.
- **Write-back is transactional with a 1-row safety net**, and the report never claims more than
  the engine delivered (`GridWrite::plan`, `one_row_verdict`, `Rollback::note`).
- **A destructive modal action guards its own launch**, in the same step that launches it
  (`widgets::accept_launch`) — not via the disabled button.
- **One identifier quoter** — `export::ident_sql` (executed SQL) or `ident_if_needed` (SQL the user
  reads). Don't write a fifth.
- **Every schema-search surface matches through one predicate** (`schema::object_name_matches`,
  which the ER diagram's find bar calls too), and Find-Anywhere searches names → objects → columns
  in that order, undebounced.
- **Identifier scanning treats bytes `>= 0x80` as word bytes** — `sql::is_word_byte`/`is_word_start`
  are the only definitions.
- **A Velopack channel name is app identity, like `--packId`** — add a name, never rename one;
  the names live only in `release.yml`, so the guard is a CI step there, not a `cargo test`.
- **Splitting `lib.rs`/`main.rs`** has its own procedure; read it before starting one.

Two further sections are load-bearing and easy to regress by not knowing they exist: **Floem 0.2
gotchas** (focus and Tab handling, scroll-sync, overlays, transitions, `with` vs `get`) and **Data
grid**. Consult them before touching either area — most entries are there because the obvious
spelling shipped a bug.

## Testing (TDD is the default)

**Test-driven development is the working approach for this project.** New behavior and bug fixes
start with a failing test, then the code that makes it pass.

- **Red → green → refactor.** For any new pure-logic behavior or bug fix, write the test first (it
  fails), then implement until it passes, then clean up with the test still green. When a bug is
  reported, first add a test that reproduces it (red), then fix it.
- **Watch the new test fail against the *unfixed* tree, and say so in the commit.** Not a
  formality — it is the one check that separates a test from a decoration. A pre-release review of
  a whole fix campaign found **thirteen** tests that were green against the very bug they were
  written to guard, three of them guarding fixes in that same range. The shared defect was always
  the same: the test was written against the fix's *description* rather than its *effect*, by the
  author who already knew the property held, while the bug sat at the seam they had not written
  down — nearly always **a pure function's composition with its caller**. `overlay_open_key`'s pin
  tested the memo in isolation and never a call site, so the High regression the memo introduced
  passed it; `a_superseded_check_still_answers_the_action_that_asked` tested two predicates
  separately while the revert sat between them. If the test cannot be made to fail — because the
  fix is a deletion, or the decision lives in a view — say *that* in the commit instead of implying
  coverage. Stage the fix, `git stash` it, run the test, unstash: three commands, and it would have
  caught all thirteen.
- **Where tests live.** Pure logic belongs in `schemaic-core` (or the owning crate's `src`) with an
  inline `#[cfg(test)] mod tests`; regression tests for a bug live next to the code they guard. The
  UI/app keep thin wrappers over `schemaic-core`, so push logic *down* into a testable core function
  rather than testing it through the UI.
- **Coverage bar.** Every public function that encodes a decision (parsing, analysis, formatting,
  export, diffing, key selection, gating) must have unit tests covering the happy path, empty/edge
  inputs, and known failure modes. Prefer many small, named tests over one broad one.
- **Keep the suite green + fast.** `cargo test --workspace` must pass before any commit; tests stay
  pure (no live DB / network / filesystem — model those at the boundary). **In-memory SQLite is
  allowed** and is not an exception to that rule: it needs no server, touches no file and is
  deterministic, which is why `db::sqlite` is the one backend whose DB layer is tested directly.
  Use SQLite's shared-cache memory URI (`file:name?mode=memory&cache=shared`, unique name per test)
  where several connections must reach one database, as the write paths do — a plain `:memory:` is
  private to one connection, and a temp file would break the rule for real. Don't commit with failing
  or `#[ignore]`d tests unless the user asks. The single exception is
  `core/tests/doc_coverage.rs`, which asserts every `src/*.rs` module is named somewhere in
  `docs/architecture.md` — the thing under test *is* a file. A new module fails it until it's on the
  map there.
- **Architecture invariants are test-enforced where possible** — e.g. the single SQL boundary lexer,
  the 1-row write-back safety net, and edit-model key selection all have regression tests; extend
  them rather than working around them.

## Build & run

- `cargo build` / `cargo run -p schemaic-app`.
- **Windows:** if the app is running, the linker can't overwrite `target/debug/schemaic.exe`
  ("Access is denied"). Stop it first (`Get-Process schemaic | Stop-Process -Force`).
- Visual and interaction changes: **build only, and let the user verify.** There is no screenshot
  harness in this repository, and launching the app from a session is worse than useless — it
  writes the user's real `%APPDATA%\Roaming\schemaic` (tabs, expansion set, active connection) and
  the `.bak` sibling is rewritten in the same save, so there is no pre-agent restore point. Write
  the hand checks down instead, in the shape `review/user-verify-fix.md` uses: setup, the exact
  action, what should happen, and what would mean the fix is wrong.

## Never bulk-rewrite source with a script

**Do not use `sed -i`/`awk`/`perl -i` (or any generated script) to edit `.rs` files in place.** Use
the editor tools, one site at a time, driven by the compiler's error list. Adding a field to a
widely-constructed struct breaks 20+ literals and the temptation to "just script it" is exactly
when this goes wrong.

This is written down because it already destroyed ~900 lines across seven files in one command. The
awk had a line-buffering bug: it printed a held line only on *some* paths, so every branch that
didn't print silently dropped a line. The damage isn't localized to the intended matches — it's
scattered through whole files — and it compiles-ish, so the error list *shrinks* and looks like
progress. Recovery was only cheap because the tree happened to be committed a few minutes earlier.

If a mechanical edit really is unavoidable:

- **Commit (or stash) first.** A dirty tree plus an in-place script is how uncommitted work dies.
- Verify with `git diff --stat` **before** trusting the build: net line count should match what you
  intended. Mass deletions are the tell — a shrinking error count is not.
- Prefer a change that avoids the churn (a `Default` derive + `..Default::default()`, a constructor,
  a `From` impl) over a change that requires touching every call site.

Recovery, if it happens anyway: `git show HEAD:<path> > <path>` per file. Plain `git checkout --`
/`git restore` may be blocked as a destructive operation, and `git show` is read-only.

## Commits & releases

- **Never commit unless the user explicitly asks.** Making edits does not imply committing — leave
  changes in the working tree. Same for `git tag`/`git push`. Amending is fine when the user is
  iterating on a commit.
- **Always run `cargo fmt --all` before a push.** CI (`ci.yml`) fails the build on an unformatted
  tree (`cargo fmt --all --check`), and it's historically the most common CI failure. Run it and
  commit any resulting changes *before* `git push` — verify with `cargo fmt --all --check` (exit 0).
- **Conventional Commits** — `type(scope): subject`, imperative, no trailing period, lower-case
  after the colon. Types: `feat`/`fix`/`refactor`/`perf`/`docs`/`test`/`chore`/`build`/`ci`. Scope
  = the crate/module the change centers on (`grid`, `editor`, `schema`, `ai`, `sql`, `theme`, `db`,
  `ci`…); omit only when cross-cutting. Optional body (blank line first) explains the *why*. Every
  message ends with the trailer `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`. Example:
  `feat(grid): add row cloning via context menu`.
- **Version bumps are explicit-only.** Bump only when asked; never as a side effect of an unrelated
  commit. Edit **one** place — `[workspace.package].version` in the root `Cargo.toml` (all crates
  inherit via `version.workspace = true`; never a per-crate `version`). Commit as
  `chore: release vX.Y.Z`.
- **Releases are tag-driven.** Bump → commit → `git tag vX.Y.Z && git push origin vX.Y.Z` (keep tag
  and `Cargo.toml` in sync). The tag triggers `release.yml` (Linux + Windows binaries → GitHub
  Release); `ci.yml` runs fmt + clippy (`-D warnings`) + **rustdoc (`RUSTDOCFLAGS=-D warnings cargo
  doc --workspace --no-deps`)** + `cargo deny` + build/test on push/PR. Keep the tree green before
  tagging. The rustdoc gate is the one no local habit runs, and a doc link pointing at a renamed
  item has failed a push on exactly it — in PowerShell that check is
  `$env:RUSTDOCFLAGS = '-D warnings'; cargo doc --workspace --no-deps`, since the POSIX env-var
  prefix is a parse error there.
