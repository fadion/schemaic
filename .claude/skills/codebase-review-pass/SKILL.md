---
name: codebase-review-pass
description: Run the in-progress codebase review forward (work order in review/plan.md, ledger in review/findings.md) — dispatches each pass to a subagent and keeps going until an attended checkpoint. Use whenever the user says "next", "next pass", "continue", "keep going", or "run the review" while a review is underway, and to run the final triage once the last slice pass is done. If review/plan.md doesn't exist yet the review hasn't been set up — use the codebase-review skill instead. Not for pull-request or working-diff review.
---

# Drive the review forward

The user should have to say **"next" a handful of times per review, not forty**. Which pass comes
next, and how to run it, are both on disk. **Never ask the user which pass comes next — read it.**

Each pass runs in its own **subagent**. This session is the orchestrator: it reads the plan row,
dispatches, audits the result, closes the row, and moves on — it does not read findings blocks in
full, which is what lets it drive a whole segment without drowning. Method and rationale:
`reference.md` §7.

## State

| File | Role |
| --- | --- |
| `review/plan.md` | The work order: slices, lens assignment, attended/delegated flags, triage schedule. |
| `review/findings.md` | The ledger. Its **pass log** is the cursor; the frozen SHA is at the top. |
| `review/index.md` | One line per finding + the rejected one-liners. This session owns it. |
| `review/facts.md` | Pass 0's fact sheet — cite it, don't re-derive it. |
| `review/.snapshots/` | Per-pass ledger snapshots. Kept. |
| `.claude/skills/codebase-review/SKILL.md` | This project's preconditions, A1 evidence set, reviewer notes, scope. |
| `.claude/skills/codebase-review/reference.md` | The method: lenses (§1), the gate (§3), the per-pass protocol (§4), the ledger + its integrity rules (§5), triage (§6), orchestration (§7). |

If `review/plan.md` is missing, the review isn't set up — say so and point at the `codebase-review`
skill. If it exists but `review/findings.md` doesn't, the ledger was never created: create it
(frozen SHA + freeze mode + an empty pass log, per reference §5) and start at Pass 0. Don't assume
the census ran — with no ledger there's no record either way, and re-running Pass 0 is cheap next
to a review built on a census nobody has.

## The loop

Repeat until a stop condition fires.

**1. Confirm the ground.** `git rev-parse HEAD` must still match the ledger's frozen SHA. If it has
moved, **stop and tell the user** — every `file:line` in the ledger is anchored to it. Do not
re-anchor on your own initiative; offer it as their call, and say that earlier line refs will have
drifted (file and symbol names survive). If the review runs in a worktree at a tag, this check is
against *that* worktree and a moving `main` is irrelevant.

**2. Audit the previous pass.** Mechanically, without reading the findings themselves:

- its row has a findings count and a non-empty "Not covered", and isn't still `in progress`;
- the ledger gained the number of blocks its report claimed, with that pass's ID prefix;
- each new block carries **Failure**, **Evidence**, **Confidence**, **Status**;
- the §5 integrity checks pass (size delta, monotonic counts, anchors intact).

If it looks truncated, unverified, or short of what it reported, **re-run that pass instead of
moving on**, and say why. This audit is what stands in for the checkpoint that delegation removed.

**Cap re-runs at two**, recorded in the row (`attempt 2`). A pass that degrades twice is degrading
for a structural reason — the slice is too big, the plan's split point is wrong, or the lens
doesn't fit — so stop, say which you think it is, and let the user decide.

**3. Pick the next pass** — the first in the plan's sequence whose row isn't `done`. If it's
flagged **attended**, or it's a **triage checkpoint** (reference §6), stop and hand back. If every
slice pass is done, the next thing is the **final triage**, which is attended — stop.

**4. Mark it `in progress`** and snapshot the ledger to `review/.snapshots/findings-<pass-id>.md`.
A pass that dies mid-way must re-run, not be skipped; the row is what decides that, and the
snapshot is what makes a bad write recoverable.

**5. Dispatch the subagent** (template below) and wait.

**6. Check the guards.**

- `git status --short` in the frozen tree must be **empty**. `review/` is gitignored, so anything
  there means the subagent edited source: restore it, record `attempt 2`, re-run the pass.
- The §5 ledger checks against the snapshot.

**7. Close the row** — findings by severity, and what the pass did *not* cover, taken from its
report. Append its findings to `review/index.md`, one line each. Loop.

## Stop conditions

Stop the loop, report, and hand back on any of:

- the next pass is **attended** or is a **triage checkpoint**;
- the subagent reports a **Critical** — surface it in your report immediately and say it warrants
  fixing now, on a branch off the frozen SHA so the ledger's references stay valid. An orchestrator
  that keeps going has re-buried the thing escalation exists for;
- a pass **degraded twice**;
- the frozen SHA moved, or a guard failed in a way you can't resolve.

## Reporting

When you stop, ~8 lines: which passes ran, findings by severity across them, the single most
important one, what's next, and anything needing the user's judgment. Between passes, stay quiet —
a line per pass at most.

## The subagent brief

Use a `general-purpose` subagent (it needs Read/Grep/Glob plus Bash for probe harnesses). It
inherits nothing from this session, so the prompt carries everything:

```
You are running pass <ID> of an in-progress codebase review of Schemaic. Read these first:

- .claude/skills/codebase-review/reference.md — §1 lenses, §3 the verification gate,
  §4 the per-pass protocol, §5 the ledger format and its integrity rules. Follow them exactly.
- .claude/skills/codebase-review/SKILL.md — "Project-specific notes for reviewers". The
  deliberate-conservatism traps listed there are the most common source of wrong findings.
- CLAUDE.md — the architecture invariants and Floem gotchas. L2 is the highest-yield lens here.
- review/index.md — every finding raised so far, one line each, plus the rejected candidates.
  Check it before writing anything: if your candidate is already there, it's a duplicate (say so
  in your report instead of re-raising it) or already rejected (don't re-raise it at all).

Your pass:
  Slice:  <slice name>
  Files:  <files + the split point if this is a sub-pass>
  Lenses: <only these — do not carry others>
  <"This pass needs a live <engine>/the GUI. State in your report which instance or server you
   exercised." — only if the plan row flags it>

Protocol: read the whole slice before writing anything; trace 2-3 flows end to end (the real bugs
in this codebase live in the core->db->ui seam, not inside one file); draft candidates with their
lens's required evidence; verify each against §3 before it enters the ledger; write survivors
ranked, capped at 12.

Writing to review/findings.md: append your findings block at the "**Status:** open" / "---" /
"## Rejected" boundary, using Edit with that anchor or `cat >> review/findings.md <<'EOF'`. NEVER
mutate that file with a shell script that rebuilds it — a PowerShell splice destroyed it once
(reference §5). Do not touch the pass log or review/index.md; the orchestrator owns those.
Rejected candidates go in the ## Rejected section, one line each on why.

Hard rules: edit no source file — findings only, no fixes, no scripted or bulk edits anywhere.
Don't restate a Pass-0 linter item (review/facts.md) as a finding. If you find a Critical (data
loss, corruption, credential exposure), write it to the ledger and say so prominently in your
report — it gets escalated immediately, not at triage.

Return a compact report, NOT the findings themselves:
  - pass ID and counts by severity (C/H/M/L/D)
  - each finding: ID, severity, file:line, one-line claim
  - "Not covered": files skimmed, flows not traced. Never blank — a blank is a claim of full
    coverage.
  - an explicit statement that every finding cleared §3, and how many candidates you rejected.
```

## Hard rules

- **Edit no source file** — this session or any subagent. Findings only.
- **Escalate a Critical in the pass that finds it**, and break the loop.
- **Never mutate the ledger from a shell script** (reference §5), and snapshot before every pass.
- **The gate is not optional.** Mature code is full of deliberate conservatism that makes
  plausible-sounding findings wrong.
- **Cap at 12 findings per pass.** More means the pass isn't ranking.
- **Append to the ledger, never rewrite it.**
- **Read the project's architecture/conventions docs** before any pass carrying L2 — that lens is
  usually the highest-yield one and it's entirely project-specific.
