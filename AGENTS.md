# Schemaic — working notes for coding agents

A native SQL editor (Rust + [Floem](https://github.com/lapce/floem) 0.2.0), MySQL/MariaDB-first,
Zed-inspired, aiming to replace DataGrip. Workspace crates: `schemaic-core` (models + the pure,
unit-tested SQL/edit/export/DDL logic), `schemaic-db` (MySQL/MariaDB + PostgreSQL + SQLite + SSH
tunnels), `schemaic-conn` (saved connections hydrated from the OS keyring — the seam a non-GUI
front end loads a connection through), `schemaic-ai`, `schemaic-term`, `schemaic-ui` (the Floem
views), `schemaic-app` (signal wiring, the built-in MCP server).

**Three engines, and they are not equal.** MySQL/MariaDB and PostgreSQL are full; SQLite reads,
writes, imports and edits **tables** (through the twelve-step rebuild — `ddl::sqlite_rebuild_sql`),
**views** and **triggers**, but has **no manual-transaction mode** — a statement about SQLite
rather than unfinished work (`db::session::Session::open` carries the reason). What differs between the
engines now lives in the *narrow* predicates that decide how an edit is performed rather than
whether it is offered: `ddl::supports_or_replace_view`, `supports_view_rename`,
`supports_column_reorder`, `supports_change`, `alter_column_disturbs_checks`,
`stats::supports_table_stats` — and, for the *comparison* rather than any editor,
`ddl::ref_schema_is_database` and `view_definition_is_qualified`, which ask whether a field the
differ reads names the object or the database it was read from. Ask a **capability**, never an
engine: a `dialect == Postgres` or `!= MySql` compiles cleanly while silently sorting a third
engine onto whichever side it happens to fall — and a *constant* in place of a capability is the
same failure with no comparison to grep for, which is why the predicates that do answer the same
for all three engines today
(`supports_view_editing`, `supports_trigger_editing`, `supports_table_design`) *compute* that answer
from `supports_change` rather than returning `true`.

**`docs/architecture.md` is the reference document** — the module map (one entry per source file),
the architecture invariants, the UI conventions, the Floem hazards, and the data grid end to end.
This file holds only the working rules. Keep the two disjoint: a fact about the system goes there,
an instruction about how to work goes here.

**Keep it honest as you work, not afterwards.** It is the map every contributor and every session
reads, so silent drift from the code is the most damaging kind of bug there —
`core/tests/doc_coverage.rs` catches a module nobody wrote down, and nothing catches a paragraph
that has quietly become false. Write it as part of the change that made it stale, never in a
later pass.

**`TODO.md` is the user's scratchpad — a plain list of things to be done, and the place to park an
idea for a future release.** It is gitignored, so it is also the one file here with no git history
to recover from: back it up before a large rewrite, and never edit it with a script. Entries are
short and imperative; delete them as they land rather than checking them off. **It is not a
decision board.** The moment an entry starts explaining *why* something was chosen or rejected, that
paragraph belongs in `docs/architecture.md` (or the commit that supersedes it) and the entry here
shrinks back to the work that is left. Keep the three disjoint: a fact about the system goes in
`docs/architecture.md`, an instruction about how to work goes in this file, an open piece of work
goes in `TODO.md`.

## Delegate the reading

`docs/architecture.md` runs to several thousand lines, and so do the crate's largest modules
(`ui/grid.rs`, `ui/lib.rs`, `app/main.rs`), so paging them into the main context is what runs a
session out of room. **No figures here on purpose** — they were 40–70% under the real ones by the
time anyone checked, and the rule they support does not depend on the value. If your tool can
farm a search out to a sub-agent with its own context window, do that for any multi-file discovery
and for consulting `docs/architecture.md`, and take back a conclusion with `file:line` citations
rather than a transcript. Read a section by hand only when you are about to edit it, or when the
citation isn't enough.

Editing a module you are actively designing in still belongs in the main loop.

## The invariants — don't regress these

Each is stated in full, with the bug that motivated it, under *Architecture invariants* in
`docs/architecture.md`. This index exists so you know a rule is there to be read; it is **not** a
substitute for the statement, and none of these is a style preference.

- **The write guard lives on the run action**, not in a caller of it — every path executing user
  SQL goes through `TabsActions::run`/`run_all` and `sql::run_verdict`, or through a refusal
  *strictly stronger* than it. There are **three** such refusals now:
  `sql::rerunnable_for_export`, which has no `Confirm` arm; `sql::script_verdict`, which treats
  a whole `.sql` file as a write without reading it; and `sql::read_only_reason`, a per-dialect
  allowlist of read-only statement heads with no confirm arm at all, which is the gate for both
  paths that run SQL with nobody at the keyboard — the MCP server's `run_query` tool and
  `schemaic query`. Never a second, laxer gate — and **each is reached only through a request its
  guard mints**: `ScriptRequest::approved` for the file, `RerunRequest::approved` for `apply_view`,
  `open_table_filtered` and the grid's post-commit re-fetch, and
  `cli::exec::ExecRequest::approved` for `schemaic exec`, whose `--yes` answers a `Confirm` and
  cannot answer a `Block`. That shape is the invariant, not a detail of it: the guard being a
  *step* the launcher had to remember is how one `return` came to be all that stood between a
  read-only connection and a file, and how three of the four re-run affordances came to rest on
  predicates that were not about writes — the post-commit re-fetch was the last of them, and the
  census meant to catch it could not see its call, which is spelled `(run)(sql)`.
- **One SQL boundary lexer** — everything scanning SQL for string/comment/quote boundaries builds
  on `core::sql::skip_noncode`, and it is dialect-aware.
- **Structure-aware SQL analysis goes through `core::intel`** (a real per-dialect AST), not a new
  hand-rolled scanner. The DB stays the semantic authority.
- **One connection per operation** — every `Db` method connects, runs, disconnects. **Two**
  exceptions, both because their statements are not independent: a `TxMode::Manual` tab's pinned
  `Session`, and `Db::run_script`, which holds one connection for a whole `.sql` file (a dump's
  `SET FOREIGN_KEY_CHECKS`, its own `BEGIN`, its `DELIMITER` — all session state).
- **Connection identity is the `Db` handle / `conn_id`**, never a `mysql://user:pass@host` URL. No
  credential in a URL, argv or log.
- **Connection secrets persist to the OS keyring**, not `connections.json` — saves route through
  the app's `secrets::{load,save}_connections`.
- **Own per-entity signals in a child `Scope`, and dispose it *deferred*** (`exec_after(ZERO, …)`).
- **Themable colours reach reactive styles as `fn() -> Color`**, never a captured `Color`.
- **Pure logic lives in `schemaic-core` with unit tests**; the UI/app keep thin wrappers.
- **Generated DDL is never run silently, and never emitted from a second differ** — draft →
  `ddl::diff` → `emit` → the preview modal → `Db::run_ddl`, or `Db::run_server_ddl` for a plan
  about a **container** (`CREATE`/`DROP DATABASE`/`SCHEMA`), which needs a connection attached to
  no database. Which runner a plan takes is read off the change set by `ddl_preview::preview_of`,
  never chosen by the caller.
- **Write-back is transactional with a 1-row safety net**, and the report never claims more than
  the engine delivered (`GridWrite::plan`, `one_row_verdict`, `Rollback::note`).
- **A destructive modal action guards its own launch**, in the same step that launches it
  (`widgets::accept_launch`) — not via the disabled button.
- **One identifier quoter** — `export::ident_sql` (executed SQL) or `ident_if_needed` (SQL the user
  reads). Don't write a fifth. **Quoting is not comment-safety**: `export::comment_text` is the
  second question's answer, and nothing server-supplied reaches a `--` or `/* */` line without it.
- **A string handed to a process launcher is validated in `core::launch`** — the one place, at the
  boundary where it stops being data; no shell is ever in between, and a refusal is a `Result` the
  caller cannot skip.
- **Every schema-search surface matches through one predicate** (`schema::object_name_matches`,
  which the ER diagram's find bar calls too), and Find-Anywhere searches names → objects → columns
  in that order, undebounced.
- **Identifier scanning treats bytes `>= 0x80` as word bytes** — `sql::is_word_byte`/`is_word_start`
  are the only definitions.
- **A Velopack channel name is app identity, like `--packId`** — add a name, never rename one;
  the names live only in `release.yml`, so the guard is a CI step there, not a `cargo test`. **The
  published package-repository identity is the same rule, second instance**: the Pages base URL,
  `Origin`/`Suite`, the `Signed-By` keyring path and the **published key fingerprint** are written
  into users' source lists — or checked against by hand — and moving one orphans every install just
  as silently. Its guards are CI steps in `pages.yml`, for the same reason. They were added one
  value at a time and the list above ran ahead of them: for a while only the base URL was actually
  compared, while this sentence claimed all of it.
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
  private to one connection, and a temp file would break the rule for real. **Exactly one function
  is sanctioned to break it** — `export::export_xlsx_chunks`, with the user's say-so; why, and what
  it costs, is in `docs/architecture.md`. Read it as *the* one case, not as licence for a second:
  anything else wanting a real file still models it at the boundary. Don't commit with failing
  or `#[ignore]`d tests unless the user asks. **Reading the repository's own source is the one
  exempt kind of filesystem access**, because there the thing under test *is* a file:
  `core/tests/doc_coverage.rs` asserts every `src/*.rs` module is named somewhere in
  `docs/architecture.md` (a new module fails it until it's on the map there), and the
  `ui::source_gate` family scans `.rs` files for a pattern an invariant forbids. That is a whole
  family, not one test — this line said "the single exception is `doc_coverage.rs`" while a
  couple of dozen source files were already doing it.
- **Architecture invariants are test-enforced where possible** — e.g. the single SQL boundary lexer,
  the 1-row write-back safety net, and edit-model key selection all have regression tests; extend
  them rather than working around them.

## Build & run

- `cargo build` / `cargo run -p schemaic-app`.
- **Windows:** if the app is running, the linker can't overwrite `target/debug/schemaic.exe`
  ("Access is denied"). Stop it first (`Get-Process schemaic | Stop-Process -Force`).
- Visual and interaction changes: **build only, and write the hand checks down where they will
  outlive the round.** A check names four things: the setup, the exact action, what should happen,
  and what would mean the fix is wrong. Put it **in the commit message** — that is the only place
  guaranteed to still exist when someone runs it. A review in progress may collect them in
  `review/` as well, but that directory is gitignored and goes away with the round, so it is a
  worklist, never the record. There is no screenshot harness in this repository.

- **The app may be launched from a session, but only sandboxed, and only when the desk is free.**
  A naive launch writes the user's real `%APPDATA%\Roaming\schemaic` — tabs, expansion set, active
  connection — and rewrites the `.bak` sibling in the same save, so there is no pre-agent restore
  point. That is a reason to redirect the profile, not a reason never to launch:
  `core::persist::config_dir` resolves it from the **`APPDATA` environment variable**, so
  `$env:APPDATA = "<scratch>\profile"` before `Start-Process` gives a wholly throwaway one.
  Copy `connections.json` in and the real connections come with it — the keyring is keyed on the
  constant service `schemaic` with per-connection accounts, so secrets resolve while every piece of
  throwaway state stays throwaway. Verify the isolation by mtime afterwards rather than assuming it.

  `PrintWindow(hwnd, hdc, PW_RENDERFULLCONTENT)` captures the Floem/wgpu window at full fidelity,
  needs no foreground, and catches in-window overlays (menus, the completion ring). `SendKeys`
  plus `SetCursorPos`/`mouse_event` drives it. **Two standing limits:** driving it takes the
  foreground, so ask before running while the user may be working; and the redirected `APPDATA`
  breaks AI-harness auto-detect and the DBeaver/HeidiSQL import, so anything behind Ctrl+K cannot
  be reached this way. Never type SQL through `SendKeys` — it eats `(`, `)`, `{`, `}`, `^`, `%`,
  `+` and `~` as metacharacters; put it on the clipboard and paste.

  **Prefer a test to a screenshot wherever one can fail.** This is for the tier where none can —
  floem focus, layout, placement — and for settling a finding whose fix sketch is a guess.

## Writing the UI's words

- **No ellipsis on a menu label.** `Create database`, never `Create database…` (or `...`). The
  convention that an entry opening a dialog trails three dots is a Windows-menu habit this app does
  not follow, and every label in it is written without one — a new entry that carries one is the
  odd one out, which is why it keeps having to be corrected after the fact.

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
  `ci`…); omit only when cross-cutting. Optional body (blank line first) explains the *why*. **No
  attribution trailer** — no `Co-Authored-By:`, no "Generated with" line; the history was rewritten
  once to strip them and a new one puts it straight back. Example:
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
