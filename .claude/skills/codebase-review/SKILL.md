---
name: codebase-review
description: Set up and start a multi-level review of a whole codebase — freeze a clean tree, cut the code into review slices, run a mechanical fact sheet, then the repo-wide sweeps. Use when the user asks for a full/deep/multi-level codebase review, a review "by module" or "in different lenses" (bugs, quality, performance, architecture, security), an audit of the whole repo, or says "start the review" / "set up a codebase review". Produces review/plan.md + review/findings.md; individual passes after setup are run by the codebase-review-pass skill. Do NOT use for reviewing a pull request or the working diff — that's /review and /code-review.
---

# Start a codebase review

A whole-codebase review that stays useful is **slice × lens**, run as a sequence of small
passes against a frozen tree, with every finding written to one ledger. This skill does the
setup and the repo-wide part; `codebase-review-pass` runs everything after that, one pass per
invocation, driven by the word "next".

`reference.md` (next to this file) is the method spec — lenses, per-pass protocol, the
verification gate, the findings format, the pass log. **Read it before Phase 2.** Both skills
follow it, so it's the one place the method lives.

## Artifacts

Everything goes in `review/` so it can be ignored with one line:

| File | Role |
| --- | --- |
| `review/plan.md` | The work order for *this* codebase — slices, lens assignment, project-specific gates. Generated in Phase 1. |
| `review/facts.md` | Pass 0's mechanical fact sheet. |
| `review/findings.md` | The ledger: pass log (the cursor), findings, rejected candidates. |

If `review/plan.md` already exists, the review is set up — **don't regenerate it**. Hand
straight over to `codebase-review-pass`.

Offer to add `/review/` to `.gitignore` if it isn't there. These are working notes.

## Phase 0 — freeze the tree

A review of a moving tree produces `file:line` refs that rot before anyone fixes them, and
burns passes on churn.

**Prefer a worktree at a tag over freezing the live tree.** The freeze is the main cost of the
whole exercise, and it doesn't have to be paid on the branch the user works on:

```bash
git worktree add ../<project>-review <tag>
```

A tag never moves, so every `file:line` stays valid for as long as the review runs, and normal
development continues meanwhile. Offer this first; freezing the checked-out tree is the
fallback when there's no suitable tag. Record which mode is in use at the top of the ledger.

Then verify, in the review tree, in order:

1. Working tree clean (`git status --short` empty). **The review's own artifacts count** — if
   this skill's files or the `.gitignore` line for `review/` are uncommitted, commit them
   first rather than reviewing from a dirty tree.
2. The project's formatter, linter and test suite all green — use whatever the repo actually
   uses (check CLAUDE.md / CI config, don't guess).
3. The app/library builds, and any documented smoke check passes.

**If any of these fails, stop and report.** Don't fix them uninvited and don't review anyway —
the user decides. Once green, record `git rev-parse HEAD`; that SHA anchors every later
reference.

If the live tree is being frozen rather than a tag reviewed, tell the user plainly: **it has to
stay at this SHA for the whole review**, and committing between passes means later passes stop
and ask.

A precondition that names live infrastructure ("connects to both databases") must be
*checkable*. If it can't be met, say which part is unverified and let the user decide whether
to proceed — don't quietly downgrade it, because a later pass will read the gap as covered.

## Phase 1 — generate the plan

Inventory first: crates/packages/modules, line counts per file, test counts per file, and the
architecture notes in CLAUDE.md or equivalent. Then cut slices.

**Slicing heuristics** — these are the part that makes or breaks the review:

- **Cut along flows, not directories.** A slice should be one capability end to end (its core
  logic + its I/O path + its UI), because the bugs that matter cluster in the seams between
  layers, not inside one file. A slice that is "one directory" will miss exactly those.
- **A slice is a capability; a pass is ~2.5k lines.** These are different units and conflating
  them is the classic way a plan under-promises: aim for **12–20 slices** (fewer fragments the
  ledger and multiplies cross-slice duplicates), then give each slice **`ceil(LOC / 2.5k)`
  passes**, numbered `B7.1`, `B7.2`. The pass log tracks passes; the findings group by slice.
- **Measure the lines, don't estimate them, and do the arithmetic in the plan.** A 74k-line
  codebase needs ~30 passes minimum however it's cut. State the total invocation count
  (passes + Pass 0 + sweeps + triage checkpoints) up front — that number is what the user
  budgets their week against, and it is the single easiest thing for a plan to get wrong.
- **Name each sub-pass's split point in the plan**, not mid-pass. Deferring it with "split this
  if it's too big" pushes the decision to the moment of maximum incentive to skim; a pass that
  skims is worse than one that doesn't run, because it looks covered.
- **Order by blast radius, not by size.** Slices where a bug destroys data or leaks secrets go
  first; slices where a bug costs the user time go last. State the rationale in the plan — then
  check the order *against* it: bulk-write paths (import, migrations, batch jobs) belong at the
  top even when they feel peripheral.
- **Assign lenses per slice, not all lenses to every slice.** Most cells in a full
  module × lens matrix are empty; filling them produces noise and hides the real findings.
  Typically 2–4 lenses per slice.
- **Verify coverage mechanically before showing the plan.** List every source file, check each
  appears in exactly one slice, and state in the plan that it does. Prose like "the plan/monitor
  pairs" reads as coverage and isn't — that's how files go missing.

Also define, in the plan:

- **Tier A sweeps** — the repo-wide passes that can only be done globally, with a concrete
  evidence set for *this* codebase (see `reference.md` for the standard five).
- **Project-specific invariants to check.** If the repo documents architecture rules
  (CLAUDE.md, ADRs, a conventions doc), enumerate them — conformance to *those* is almost
  always the highest-yield lens, and a generic reviewer will miss all of it.
- **Scope decisions** — what is deliberately not covered, and why.

**Show the plan and get agreement before Phase 2.** The slice map determines everything
downstream; a wrong cut is expensive to discover at pass 12.

## Phase 2 — Pass 0, the mechanical fact sheet

Create `review/findings.md` **first** — frozen SHA (and which freeze mode) at the top, then the
pass log with Pass 0's row marked `in progress`. The ledger has to exist before the first pass,
or Pass 0 has nowhere to record itself and a resumed session can't tell it ran.

Machine-findable issues must never consume a model pass. Run the linters at their strictest,
plus a census of what greps well, and write `review/facts.md`. Adapt to the language; the
shape is:

- Linter at maximum strictness (e.g. `clippy::pedantic`/`nursery`, `ruff --select ALL`,
  `tsc --strict`), full output saved.
- Panic/abort surface: unchecked unwraps, bare `except`, `!` assertions, raw indexing.
- Suppressions: every `#[allow]` / `# noqa` / `@ts-ignore` — each one is a silenced signal.
- `TODO`/`FIXME`/`HACK` comments.
- Dependency hygiene: audit, unused deps, duplicate versions.
- Coverage per file, if the toolchain supports it.
- Anything the project's own docs flag as a recurring hazard.

**Separate a census from a candidate list.** A census is mechanical and its output is a fact
(`unsafe` blocks, suppressions, TODOs, coverage). A grep whose hits still need per-site
judgment — "is this `.get()` the right one here?", "is this clone hot?" — is a *candidate
list*, and it belongs in the fact sheet as counts and file distribution only, feeding a later
pass. Putting one in as a census either buries the real facts under hundreds of undifferentiated
hits or quietly turns Pass 0 into a model pass.

Note explicitly in the fact sheet whatever **failed to run** (a missing nightly toolchain, an
uninstallable coverage tool). A silently dropped census reads later as a clean result.

Then stop and summarize.

## Phase 3 — Tier A sweeps

Run the repo-wide sweeps one at a time, appending to the ledger, pausing after each. These set
the frame every slice pass reads against, so they're worth the checkpoints — and the invariant
sweep most of all, since a wrong call there mis-shapes every downstream pass.

## Then hand over

After Tier A, tell the user the setup is done and that **"next" runs each remaining pass** via
`codebase-review-pass`. Don't keep going yourself unless asked.

## Hard rules (both skills, every pass)

- **Edit no source file.** This phase produces findings; fixing is a separate phase with its
  own risk buckets. A session that starts fixing loses the ledger and the ranking.
- **A Critical is escalated, not queued.** Finding data loss, corruption or a credential leak
  at pass 3 of 40 and burying it in the ledger for a fortnight is its own failure. Write it up,
  then **tell the user immediately, in that pass's report**, and say plainly that it warrants
  fixing now. The fix still doesn't happen here: it goes on a branch off the frozen SHA/tag, so
  the review's line references stay valid and the ledger stays the record.
- **No scripted or bulk edits.** Check the project's own conventions doc — several codebases
  have been damaged this way.
- **The verification gate is not optional** (`reference.md` §3). Mature codebases are full of
  deliberate conservatism that makes plausible-sounding findings wrong.
- **Cap findings per pass.** More than ~12 means you aren't ranking.
- **Every pass states what it did not cover.** A blank there is a claim of full coverage.
