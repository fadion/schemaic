---
name: release-review
description: Review everything that changed since the last release, autonomously, before tagging — takes the commit range (default last tag..HEAD, or one you name), cuts it into slices, and runs the whole multi-pass subagent review end to end without checkpoints. Use when the user asks to review the changes since the last release/tag, review what's about to ship, "review before I release", "review v0.12.0..HEAD", or asks for a pre-release audit. Produces a triaged ledger under review/ and stops there — it fixes nothing and releases nothing. For the periodic whole-codebase review use codebase-review; for the working diff or a PR use /code-review.
---

# Review a release range

Same method as the periodic codebase review, aimed at one commit range and run **unattended**: one
invocation goes from the range to a triaged ledger without asking the user anything. The user says
"review before the release" once and reads the report.

Two things it does not do, both deliberate: it **edits no source file** — a separate writer session
picks up the ledger, builds a fix plan and implements it — and it **does not release**. The
`release` skill owns the tag.

`../codebase-review/reference.md` is the method spec: lenses (§1), the verification gate (§3), the
per-pass protocol (§4), the ledger format and its integrity rules (§5), triage buckets (§6), the
orchestration contract (§7). **Read it before Phase 1.** It transfers unchanged — everything below
is only where a range review differs from a whole-tree one.

`../codebase-review/SKILL.md`'s **"Project-specific notes for reviewers"** section transfers
unchanged too: the deliberate-conservatism traps listed there are the most common source of wrong
findings in this codebase, and every subagent brief carries them. Don't duplicate that section
here — point at it, so there's one copy to keep true.

## Artifacts

One directory per run, under the already-gitignored `review/`:

| File | Role |
| --- | --- |
| `review/release-<base-tag>/plan.md` | The work order: the range, its diffstat, the slice table, the pass sequence. Generated in Phase 1, never signed off. |
| `review/release-<base-tag>/facts.md` | Pass 0's mechanical fact sheet, scoped to the range. |
| `review/release-<base-tag>/findings.md` | The ledger: header, pass log, findings, `## Rejected`. Final triage prepends `## Triage`. |
| `review/release-<base-tag>/index.md` | One line per finding — `ID \| severity \| origin \| file:line \| claim` — plus the rejected one-liners. What each subagent reads instead of the ledger. |
| `review/release-<base-tag>/.snapshots/` | Per-pass ledger snapshots (reference §5). Kept. |

`<base-tag>` is the range's base — `review/release-v0.12.0/` for `v0.12.0..HEAD`. If that directory
already exists from a run at a **different** head SHA, rename it to
`review/release-<base-tag>-<old-head-sha>/` and start clean; same head SHA means resume (below).

**The base need not be a tag.** A mid-cycle run over `<first-sha>^..HEAD` is a normal use — the base
is simply a SHA and the directory is named after it. Such a run reviews a feature, not a release: it
says nothing about whether the tag should go out, and the pre-release run over the full
`<last-tag>..HEAD` still happens and still reads every slice. Being reviewed mid-cycle is never a
reason to narrow that one.

The periodic review's `review/findings.md` is a different, longer-lived ledger. Never write to it.
Do **read** its `## Rejected` section if it exists — re-raising something a previous round already
settled wastes the writer session's time.

Pass IDs are `P0`, `R1`–`R3`, `S1`…`Sn` (sub-passes `S3.1`), `T` for triage — distinct from the
periodic review's `A*`/`B*`, so findings from the two are never confused when read together.

### One field the periodic ledger doesn't have

Every finding block carries **`Origin: introduced | pre-existing`**, on the line above `Status`.
Introduced means the range created it or moved it somewhere it now misbehaves; pre-existing means
the pass met it while reading around the change. `git log -S` or `git blame` on the site settles
it, and a finding that genuinely can't be attributed says `pre-existing` — the conservative answer,
since it's the one that doesn't hold a tag hostage on a guess.

It exists because this review answers a question the periodic one doesn't: **should this tag go
out?** A Critical the range introduced is a regression and blocks it; the same severity in code
that shipped three releases ago is a bug worth fixing on its own schedule, and conflating the two
either holds a good release for old news or ships a regression under cover of a long list.
Reference §6's buckets sort by *how to fix safely*, which is a different axis and doesn't answer
this — so both are recorded and triage reports them crossed. This review is *designed* to surface
pre-existing bugs, by reading whole functions and tracing into unchanged callers; they are welcome,
they just aren't release blockers.

## Phase 0 — the range and the ground

The range is the skill's argument, or defaults to the last tag:

```bash
git describe --tags --abbrev=0
```

Accept a tag or a SHA as the **base**. The head is always `HEAD` — every command below anchors to
it and Phase 4 stops if it moves, so a `base..head` naming some other head would review one thing
and check another. If the user gives one, say the head is ignored and review to `HEAD`, or have
them check that commit out first.

Measure the range, and put the numbers in the plan rather than estimating from them:

```bash
git diff --stat <base>..HEAD | tail -1
```

```bash
git log --oneline <base>..HEAD
```

```bash
git diff --numstat <base>..HEAD
```

The ground has to be checkable, in this order. **This list is what CI's lint job runs, and it must
stay in step with `.github/workflows/ci.yml` rather than with memory** — a local bar narrower than
CI's passes a tree CI will reject, which is the one thing a pre-release gate exists to prevent:

1. `git status --short` empty. Reviewing a dirty tree reviews something that isn't shipping.
2. `cargo fmt --all --check` → exit 0.
3. `cargo clippy --workspace --all-targets -- -D warnings` → clean.
4. `$env:RUSTDOCFLAGS = '-D warnings'; cargo doc --workspace --no-deps` → clean. The one gate no
   local habit runs, and a doc link pointing at a renamed item has failed a release push on exactly
   it. PowerShell form deliberately: a POSIX env-var prefix is a *parse error* here, whose exit code
   reads at a glance like a rustdoc warning while the right reaction to each is the opposite.
5. `cargo deny check` → advisories / bans / licenses / sources ok. It also appears in Pass 0's
   census, but a policy failure is a stop, not a fact.
6. `cargo test --workspace` → green, no `#[ignore]`.
7. Record `git rev-parse HEAD` and the base SHA at the top of the ledger.

**A failure here is one of the run's few hard stops.** Report it and stop — don't fix it, and don't
review past it. A red tree isn't shippable, so a review of it answers a question nobody asked.

Two environment facts that have already cost a pass: on Windows, stop a running `schemaic.exe`
before building; and the agent's `%APPDATA%` view is MSIX-redirected, so an app instance the agent
launches shares no connections, tabs or history with the user's. Any pass that drives the GUI states
which instance it exercised.

## Phase 1 — cut the diff into slices

Generate `plan.md` and **proceed** — there is no sign-off. What replaces it is that the slicing is
mechanically checkable, and the plan states the check:

- **Slice by theme, not by file or by commit.** The commit log is the best available grouping
  signal — a range typically reads as a few clusters (model → introspection → emitter → UI for one
  feature). One cluster is one slice, and it deliberately spans crates, because in this codebase the
  bugs that matter live in the **core→db→ui seam**.
- **A file belongs to as many slices as it has themes.** This follows from slicing by theme and is
  the rule a file-per-slice instinct breaks: a long-lived module collects every feature in the
  range. In `v0.12.0..HEAD`, `ddl.rs`, `schema.rs` and `pg.rs` each carried CHECK constraints *and*
  triggers *and* the PG objects. Assigning such a file to one slice hides the other themes' code
  from the only reviewer who would have recognised it. So the unit is **(file, theme)**, and a
  shared file's plan row names the commits or the line ranges that put it in each slice — use
  `git log --oneline <base>..HEAD -- <path>` to see which themes touched it.
- **Size a pass at ~2.5k changed lines**, `ceil(slice_lines / 2.5k)` passes each, numbered `S3.1`,
  `S3.2`, with **the split point named in the plan**. Changed lines means added + deleted, from
  `--numstat`; for a shared file, count only the hunks that slice owns. Note that this splits a
  slice by *size* and the rule above splits a file by *theme* — a big shared file usually needs
  both.
- **Order by blast radius.** Write paths, DDL apply, secrets and import go first, ahead of larger
  but inert slices. State the rationale and check the order against it.
- **Assign 2–4 lenses per slice**, not all of them to all of them.
- **Coverage is verified mechanically before the plan is written**: every path in
  `git diff --name-only <base>..HEAD` appears in **at least one** slice, and the plan says so with
  the file count. A file in no slice is the failure this catches, and prose like "the PG object
  work" is not coverage. A file in several slices is expected, not a defect — but each of its
  appearances states which hunks it brings, so "covered" never means "somebody looked at the file".
- **State the invocation count** (passes + P0 + sweeps + triage) at the top.
- Flag any slice needing a **live engine** (MariaDB on 3306, MySQL 8 in Docker on 3307, PostgreSQL
  on 5432) or the GUI. It still delegates; its report must name what it exercised, or say the check
  was unverified.

## Phase 2 — Pass 0, the mechanical fact sheet

Create `findings.md` **first** (header + pass log with P0 `in progress`) and an empty `index.md`,
then write `facts.md`. Everything here is scoped to the range — the whole-repo census is the
periodic review's job, and repeating it buries the twenty facts that are about this release.

| Row | Kind | Feeds |
| --- | --- | --- |
| `cargo clippy --workspace --all-targets -- -W clippy::pedantic -W clippy::nursery`, filtered to changed files | census | L1/L3 |
| `unwrap()` / `expect()` / `panic!` / raw `[i]` **added** by the range, outside tests | census | L1 panic surface |
| `#[allow(...)]` added | census — each is a silenced signal | L3 |
| `TODO` / `FIXME` / `HACK` added | census | L3 / triage |
| New `src/*.rs` modules, and whether each is named in `docs/architecture.md` | census | L8 (`doc_coverage` asserts the name exists, not that the description is true) |
| Test-count delta per changed file (`#[test]` added vs. changed lines) | census | L6 — the house rule is TDD |
| `cargo deny check` | census | L5 |
| New `exec_after(` / `create_child(` / `dispose(` sites | census (enumerable) | L1 disposed-signal hazards |
| New `.get()` on a collection inside a `.style(`/render closure; new `Color` captured by value; new SQL scans; new identifier quoters | **candidate lists** — counts and sites only, judgment deferred | R1 |

Deletions are facts too: a **removed** test or a removed guard belongs in the census beside the
additions. Note explicitly whatever failed to run — a silently dropped census reads later as a
clean result.

## Phase 3 — the range sweeps

Three, in order. Each is one delegated pass with a ledger row.

| # | Sweep | Lenses | Shape |
| --- | --- | --- | --- |
| **R1** | **Invariant conformance on what changed** | L2 | Walk `docs/architecture.md`'s *Architecture invariants* and *Floem 0.2 gotchas*, and for each, enumerate the sites **the range added or moved**: new SQL scans (on `sql::skip_noncode`, with the connection's `SqlDialect`?), new identifier quoting (ends at `export::ident_sql`/`ident_if_needed`?), new `Db::` methods and `Session` uses (one-connection-per-op except manual-tx; read-only side channels off the pinned connection?), new secret reads/writes (through `core::secrets`, never `persist::save_connections`), new generated DDL (originates in `ddl::emit`, terminates at `ddl_preview`), new `create_child()`/`dispose()` (deferred?), new `Color` captured by value into a `.style(` closure, new identifier scanners (`>= 0x80` word bytes), new run paths (through the guarded `TabsActions::run`?). This is the highest-yield sweep on a feature range and it is cheap, because the candidate sites are already enumerated in `facts.md`. |
| **R2** | **Doc drift & test map** | L8, L6 | `docs/architecture.md` is this project's real specification and a large feature range is where it goes stale: every claim the range touched, checked against the code that now exists; every new module on the map with an accurate description, not just a mention; new decision functions (parsing, diffing, gating, key selection, emitting) against the tests that should exist per the TDD house rule; and whether the four invariant-enforcing tests still enforce over the new surface — the DDL round-trip gate's fixtures, the 1-row write-back net, lexer agreement, export `*_to` byte-equality. |
| **R3** | **Security & data safety across the range** | L5 | Run it **only if** the range touches write paths, DDL apply, secrets, subprocess/env, file or path handling, or SQL text construction — the plan decides from `--name-only` and says which. Trace end to end rather than per slice: what a new write can destroy, what a new quoter can inject, what a new error message can leak, whether a new engine divergence silently loses data. |

R1's findings are **candidates, not settled facts**, and the slice-pass briefs say so — in the
periodic review that sweep is attended precisely because a wrong call there gets read as settled by
everything after it. Unattended, the mitigation is that each later pass applies §3 independently
rather than inheriting R1's conclusions.

## Phase 4 — the slice passes

The loop from reference §7, with the checkpoints removed. Per iteration:

1. `git rev-parse HEAD` still matches the ledger's head SHA. If it moved, **stop** — every
   `file:line` is anchored to it, and a range review can't re-anchor itself while it runs.
2. **Audit the previous row**, mechanically, without reading the findings: the expected number of
   blocks with that pass's ID prefix landed, each carries Failure / Evidence / Confidence / Origin /
   Status, "Not covered" is non-empty, the row isn't still `in progress`, and the §5 integrity
   checks pass.
   This audit is the whole of the quality control an unattended chain has — a degraded pass
   propagating unnoticed is the failure mode of the shape.
3. Pick the first row that isn't `done`. Mark it `in progress`; snapshot the ledger to
   `.snapshots/findings-<pass-id>.md`.
4. Dispatch the subagent (brief below). Wait.
5. **Guards on return:** `git status --short` must be empty — the run directory is gitignored, so
   anything there means the subagent edited source: revert it, record `attempt 2`, re-run the pass.
   Then the §5 ledger checks against the snapshot.
6. Close the row (counts by severity + what it didn't cover), append its findings to `index.md`,
   loop.

**Cap re-runs at two.** A pass that degrades twice is degrading structurally — slice too big, wrong
split point, wrong lens — so stop and say which you think it is. That is a hard stop; the other
recoverable failures are not.

Passes stay **sequential**, for reference §7's reason: the risk ordering is load-bearing and
cross-slice duplicates are common.

## Phase 5 — final triage

Delegated like everything else — at this scale (typically 5–9 passes, capped at 12 findings each)
there is no volume problem, and no interim checkpoint unless the ledger passes ~60 open findings, in
which case insert one triage pass midway.

Triage reads what's in the ledger whole, dedupes across slices (one root cause commonly surfaces in
three), re-ranks globally, and prepends a `## Triage` section using reference §6's buckets. It
changes no code and no source file. It states its expected line delta before editing, since it is
the one pass allowed to *move* entries (settled Debt/Low to the backlog) rather than only append.

`## Triage` opens with the **release verdict**, before the buckets: every `Origin: introduced`
finding at Critical or High, listed, with the sentence that the tag should wait for them — or an
explicit "nothing introduced by this range blocks the tag" when there are none. That line is what
the user came for, and it must be readable without scrolling into the buckets. Everything else,
pre-existing included, is then bucketed as normal; a pre-existing Critical is called out as urgent
but *not* as a blocker, and triage says which it is rather than leaving the reader to infer it.

The buckets are what the writer session reads first, so the triage output must be actionable
without the reviewer present: each entry keeps its ID, severity, `file:line`, origin, the concrete
failure, and the evidence — never a bare claim.

## Reviewing a diff, not a tree

The rules that don't come up in a whole-tree review, and that a diff reviewer gets wrong by default:

- **The hunk is not the slice.** A reviewer who reads only changed lines misses every bug that is
  visible only in the surrounding code. Read each changed function whole, and trace it into the
  layer on either side. This is the single highest-value rule here.
- **Deletions are findings too.** A removed guard, a removed test, a dropped `?`, a narrowed match
  arm — `git diff` shows them and a reader scanning for new code skips them.
- **The commit message is a claim to check, not context to accept.** "fix(ui): let Escape blur a
  text field so a modal can still close" asserts a behaviour; the pass checks the code delivers it
  and that a test pins it. A commit whose message and diff disagree is a finding.
- **Churn is a signal.** A file touched by several commits in the range was hard to get right; read
  it more carefully than its final diff suggests. `git log --oneline -- <path>` over the range gives
  the count.
- **Interaction with unchanged code is in scope.** New code that is correct in isolation and wrong
  against an existing caller is the characteristic bug of a feature range, and it is invisible from
  the diff alone.
- **Fixture-backed gates need their fixtures extended, not worked around.** New model fidelity
  (a new object kind, a new column attribute) with no new round-trip fixture is an L6 finding.

## Autonomy

The periodic review's attended set is replaced, not dropped:

| Was attended | Replaced by |
| --- | --- |
| Plan sign-off | Mechanical coverage check, stated in `plan.md` with the file count |
| A1 invariant census | R1 delegated, its findings marked as candidates that later passes re-verify under §3 |
| Triage checkpoints | One delegated final triage; an interim one only past ~60 open findings |
| Critical escalation breaks the loop | Recorded, marked in the ledger, and **led with in the final report** — the run is one session, so stopping buys nothing that reporting doesn't. An introduced Critical additionally opens the triage verdict, since it is the one finding that holds the tag |

**The only hard stops:** a Phase 0 ground failure; HEAD moving mid-run; a pass degrading twice; a
ledger integrity failure that a snapshot restore doesn't fix. Everything else is recorded and the
run continues.

## Resuming

Re-invoking the skill with the run directory present and the head SHA unchanged resumes from the
pass log — that is the cursor, and it's on disk precisely so a lost session costs one pass. Don't
regenerate `plan.md`, and don't re-run `P0` if its row says `done`. A head SHA that has changed is a
different review: archive and start clean (see Artifacts).

## The subagent brief

A `general-purpose` subagent (Read/Grep/Glob plus Bash for `git` and probe harnesses). It inherits
nothing:

```
You are running pass <ID> of an automated pre-release review of Schemaic, covering the commit
range <base>..<head>. Read these first:

- .claude/skills/codebase-review/reference.md — §1 lenses, §3 the verification gate, §4 the
  per-pass protocol, §5 the ledger format and its integrity rules. Follow them exactly, with one
  substitution: wherever it names `review/findings.md` or `review/index.md`, the file is
  <run-dir>/findings.md or <run-dir>/index.md. The paths it hardcodes belong to a different,
  longer-lived ledger that this run must not touch.
- .claude/skills/codebase-review/SKILL.md — "Project-specific notes for reviewers". The
  deliberate-conservatism traps listed there are the most common source of wrong findings.
- .claude/skills/release-review/SKILL.md — "Reviewing a diff, not a tree". Those rules are what
  this pass is for.
- docs/architecture.md — the architecture invariants and Floem gotchas. L2 is the highest-yield lens here.
- <run-dir>/index.md — every finding raised so far, one line each, plus the rejected ones. Check
  it before writing: a candidate already there is a duplicate (say so in your report instead of
  re-raising) or already rejected (don't re-raise it at all).
- <run-dir>/facts.md — Pass 0's census. Cite it; never restate one of its linter items as a
  finding.

Your pass:
  Slice:  <slice name>
  Range:  <base>..<head>
  Files:  <files + the split point if this is a sub-pass. A file marked "shared" is in another
           slice too, for its other theme — review only the hunks listed here and leave the rest;
           another pass owns them.>
  Lenses: <only these — do not carry others>
  <"Needs a live <engine>/the GUI. State in your report which instance or server you exercised;
   if it wasn't reachable, say the check is unverified rather than omitting it." — if flagged>

Protocol: start with `git log --oneline <base>..HEAD -- <files>` and `git diff <base>..HEAD --
<files>` to see what changed and what each commit claimed; then read each changed function WHOLE
in the current tree, not just its hunks, and trace 2-3 flows end to end across the core->db->ui
seam. Deletions and interactions with unchanged callers are in scope. Draft candidates with their
lens's required evidence; verify each against §3 before it enters the ledger; write survivors
ranked, capped at 12.

Every finding block carries one extra line above **Status**:

  **Origin:** introduced | pre-existing — <how you decided>

"introduced" means this range created the fault or moved it somewhere it now misbehaves;
"pre-existing" means you met it while reading around the change. Settle it with `git log -S` or
`git blame` on the site, not by whether the line appears in the diff — a caller the range didn't
touch can be the one that breaks. If you genuinely can't attribute it, say pre-existing: that is
the answer that doesn't hold a release on a guess. Pre-existing findings are wanted, not noise —
this pass is meant to read past the hunks — so record them at their real severity and let triage
decide what blocks the tag.

<Only for slice passes:> R1's invariant findings are candidates, not settled facts — re-verify
independently rather than inheriting them.

Writing to <run-dir>/findings.md: append your block at the "**Status:** open" / "---" /
"## Rejected" boundary, using Edit with that anchor or `cat >> <run-dir>/findings.md <<'EOF'`.
NEVER mutate that file with a shell script that rebuilds it — a PowerShell splice destroyed this
project's ledger once (reference §5). Do not touch the pass log or index.md; the orchestrator owns
those. Rejected candidates go in ## Rejected, one line each on why.

Hard rules: edit no source file — findings only, no fixes, no scripted or bulk edits anywhere.
A Critical (data loss, corruption, credential exposure) goes in the ledger AND prominently in your
report.

Return a compact report, NOT the findings themselves:
  - pass ID and counts by severity (C/H/M/L/D), and how many of those are introduced
  - each finding: ID, severity, origin, file:line, one-line claim
  - "Not covered": files skimmed, flows not traced. Never blank — a blank is a claim of full
    coverage.
  - an explicit statement that every finding cleared §3, and how many candidates you rejected.
```

## The report

One report, at the end. ~12 lines, and it **opens with the release verdict** — what the range
introduced at Critical or High, or that it introduced nothing that blocks the tag. Then: the range
and its size, the passes that ran, findings by severity split introduced vs. pre-existing, the
triage buckets' counts, where the ledger is, and anything the run could not verify (an unreachable
engine, a census that didn't run). Between passes stay quiet — a line per pass at most.

Close by naming the two follow-ups the review deliberately doesn't do: a writer session picks up
`<run-dir>/findings.md` for the fix plan, and the `release` skill cuts the tag once the fixes land.

## Hard rules

- **Edit no source file** — this session or any subagent. Findings only. Fixing is a separate
  session's job and mixing them loses the ledger and the ranking.
- **Never mutate the ledger from a shell script** (reference §5); snapshot before every pass;
  append, never rewrite.
- **The verification gate is not optional** (reference §3). This codebase is deliberately
  conservative in specific places and each one is a trap for a plausible-but-wrong finding.
- **Cap 12 findings per pass.** More means the pass isn't ranking.
- **Every pass states what it did not cover.** A blank there is a claim of full coverage.
- **Every finding states its `Origin`.** Without it the ledger can't answer whether the tag should
  go out, which is the only question this review exists for.
- **Don't touch `review/findings.md`** — that's the periodic review's ledger. Read-only.
