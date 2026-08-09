---
name: codebase-review-pass
description: Run the next pass of an in-progress codebase review (work order in review/plan.md, ledger in review/findings.md). Use whenever the user says "next", "next pass", "continue", "keep going", or "run the next review pass" while a review is underway, and to run the final triage once the last slice pass is done. If review/plan.md doesn't exist yet the review hasn't been set up — use the codebase-review skill instead. Not for pull-request or working-diff review.
---

# Run the next review pass

The point of this skill is that the user only has to say **"next"**. Which pass that is, and
how to run it, are both on disk. **Never ask the user which pass comes next — read it.**

## State

| File | Role |
| --- | --- |
| `review/plan.md` | The work order: slices, lens assignment, project-specific gates and scope. |
| `review/findings.md` | The ledger. Its **pass log** is the cursor; the frozen SHA is at the top. |
| `review/facts.md` | Pass 0's mechanical fact sheet — cite it, don't re-derive it. |
| `.claude/skills/codebase-review/reference.md` | The method spec: lenses (§1), the verification gate (§3), the per-pass protocol (§4), the ledger format (§5). |

If `review/plan.md` is missing, the review isn't set up — say so and point at the
`codebase-review` skill. If it exists but `review/findings.md` doesn't, the ledger was never
created: create it (frozen SHA + freeze mode + an empty pass log, per reference §5) and start
at Pass 0. Don't assume the census ran — with no ledger there's no record either way, and
re-running Pass 0 is cheap next to a review built on a census nobody has.

## Every invocation, in order

**1. Confirm the ground.** `git rev-parse HEAD` must still match the ledger's frozen SHA. If
it has moved, **stop and tell the user** — every `file:line` in the ledger is anchored to it.
Do not re-anchor on your own initiative; offer it as their call, and say that earlier line refs
will have drifted (file and symbol names survive). If the review is running in a worktree at a
tag, this check is against *that* worktree, and a moving `main` is irrelevant.

**2. Audit the previous pass.** Its row needs a findings count and a "Not covered" note, and
its findings need to meet §3 and §5 of the reference. If it looks truncated, unverified, or
still `in progress`, **re-run that pass instead of moving on**, and say why. Unattended
chaining's failure mode is a degraded pass propagating unnoticed — this audit is what stands in
for the checkpoint that automation removed.

**Cap re-runs at two.** Record the attempt in the row (`attempt 2`). A pass that degrades twice
is degrading for a structural reason — the slice is too big, the plan's split point is wrong,
or the lens doesn't fit — so stop, say which you think it is, and let the user decide. Looping
a third time just burns the budget.

**3. Pick the next pass** — the first in the plan's sequence whose row isn't `done`. If the
plan schedules an interim triage checkpoint here (reference §6), run that. If every slice pass
is done, run the **final triage** instead and stop.

**4. Mark it `in progress`** before starting. A pass that dies mid-way must re-run, not be
skipped; the row is what decides that.

**5. Run it** per reference §4, carrying **only** the lenses that pass is assigned in the
plan. Read the whole slice before writing anything; trace 2–3 flows end to end — in most
codebases the real bugs live in the seams between layers, not inside one file; verify every
candidate against §3 before it enters the ledger.

**6. Close the row** — findings by severity, and what you did *not* cover. A blank there is a
claim of full coverage; make it true or fill it in.

**7. Report in ~5 lines:** pass, findings by severity, the single most important one, and
anything needing the user's judgment. Then stop — one pass per invocation.

## Hard rules

- **Edit no source file.** Findings only; fixing is a separate phase with its own risk buckets.
- **Escalate a Critical in the pass that finds it.** Data loss, corruption or a credential leak
  doesn't wait for triage thirty passes away: write it to the ledger, then say so in the report
  and recommend fixing it now — on a branch off the frozen SHA/tag, so the ledger's references
  stay valid and the review itself stays read-only.
- **No scripted or bulk edits**, and check the project's conventions doc for its own warnings.
- **The gate is not optional.** Mature code is full of deliberate conservatism that makes
  plausible-sounding findings wrong. Rejected candidates go in the ledger's `## Rejected`
  section with one line on why.
- **Cap at 12 findings.** More means you aren't ranking.
- **Don't restate a Pass-0 linter item as a finding.**
- **Append to the ledger as you go**, never rewrite it.
- **Read the project's own architecture/conventions docs** before any pass carrying L2 — that
  lens is usually the highest-yield one and it's entirely project-specific.
