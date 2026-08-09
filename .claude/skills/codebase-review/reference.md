# Codebase review — method spec

The shared authority for both `codebase-review` (setup, Tier A) and `codebase-review-pass`
(everything after). The per-project work order lives in `review/plan.md` and defers to this
file for method.

---

## 1. Lenses

Every lens has a hard scope and an **evidence requirement**. A pass that can't meet the
evidence requirement produces no finding — that's the point, not a failure.

| ID | Lens | In scope | Evidence required |
| --- | --- | --- | --- |
| **L1** | **Correctness / bugs** | wrong results, panics, hangs, races, off-by-one, unhandled error paths, platform/backend divergence, encoding & boundary bugs | a **concrete input + the wrong output/crash**. "Could be a problem" is not a finding. |
| **L2** | **Invariant conformance** | the architecture rules the project documents for itself (CLAUDE.md, ADRs, conventions docs) and the framework hazards it has written down | the invariant quoted + the violating site |
| **L3** | **Code quality** | duplication, god functions, dead code, leaky abstractions, naming/altitude, comment drift | the duplicate/dead site named + a stated cost |
| **L4** | **Performance** | work in hot paths (render, per-keystroke, per-request, per-row), superlinear algorithms, avoidable allocation, blocking the wrong thread, memory at realistic scale | the path it sits on + the input scale where it bites |
| **L5** | **Security & data safety** | credential handling, injection & quoting, destructive operations, write paths, file/path handling, subprocess & env surfaces, authn/authz boundaries | the attack or data-loss scenario, end to end |
| **L6** | **Test coverage / testability** | untested decision functions, logic trapped in untestable layers, missing edge/failure cases, invariant-enforcing tests that no longer enforce | the specific untested decision + the test that should exist |
| **L7** | *(Tier A only)* **Architecture & boundaries** | layering, module cohesion, seams and their honesty, god modules, cyclic coupling | the coupling + what it prevents |
| **L8** | *(Tier A only)* **Doc drift** | project docs vs. actual code | the claim + the contradicting code |

**L2 is usually the highest-yield lens and the one a generic reviewer will miss entirely.** A
project's written invariants encode bugs already paid for. Weight it accordingly.

---

## 2. Tier A — the standard repo-wide sweeps

Narrow and evidence-driven: grep plus targeted reads, not whole-file passes. `review/plan.md`
fills in the concrete evidence set for each.

| # | Sweep | Lenses | Shape |
| --- | --- | --- | --- |
| **A1** | **Invariant conformance census** | L2 | For each documented invariant, enumerate *every* site it governs and check each. This is mechanical and high-yield — do it first and do it properly. |
| **A2** | **Architecture & boundaries** | L7, L3 | Dependency graph; what leaks across layers; whether the seams are real seams or decoration; which modules have become landfills. |
| **A3** | **Security & data safety** | L5 | Trace secrets, untrusted input, and destructive operations end to end across the whole repo rather than per slice — these paths cross every boundary. |
| **A4** | **Performance map** | L4 | Not a code read: a *map*. Enumerate hot paths, give each a cost model and the scale where it degrades, and output a ranked shortlist (≤8). Slice passes then spend L4 effort only on those. |
| **A5** | **Test map** | L6 | Decision functions vs. the tests covering them; logic sitting where it can't be tested; invariant-enforcing tests that have quietly stopped enforcing; modules at zero. |

---

## 3. The verification gate

A candidate becomes a finding only if **all three** hold:

1. **A concrete failure can be named** — specific input or state → specific wrong output,
   crash, hang, corrupted write, or leaked secret. For L3/L6, substitute: a named
   duplicate/dead site, or a named untested decision.
2. **It isn't already guarded.** Search for the guard before claiming the gap. Mature code is
   full of deliberate conservatism, and much of what looks like a bug is a decision.
3. **It isn't a preference.** If the fix is "I'd have written it differently", it isn't a
   finding.

Failed candidates go to a `## Rejected` section at the bottom of the ledger, one line each on
why. That section is load-bearing: it stops the next review re-raising the same non-issue.

**A Critical is escalated the moment it survives the gate.** Write it to the ledger as usual,
then surface it in that pass's report and say it warrants fixing now rather than at triage — a
review that sits on a data-loss bug for thirty passes has traded the user's data for tidiness.
Fixing still happens outside the review, on a branch off the frozen SHA/tag, so the ledger's
line references stay valid.

---

## 4. Per-pass protocol

1. **Read the whole slice before writing anything.** No findings during the first read.
2. **Trace 2–3 flows end to end** through it. Most real bugs surface in the trace, not the read.
3. **Draft candidates**, each with its lens's required evidence.
4. **Verify each against §3.** Not optional.
5. **Write survivors to the ledger, ranked.** Cap **12 per pass** — beyond that you aren't
   ranking; drop the tail.
6. **State what was not covered** — files skimmed, flows not traced. Silent partial coverage
   reads as "reviewed" later, and that's how the second review misses the same bug.

Forbidden during a pass: editing source; bulk/scripted edits; "consider using X" style notes;
restating a Pass-0 linter item as a finding.

---

## 5. The ledger — `review/findings.md`

Opens with the frozen SHA — and which freeze mode produced it, a tag in a worktree or the live
tree — then the **pass log**, which is the review's cursor: it's what makes "run the next pass"
unambiguous, and the only thing a resumed session needs to read.

**The ledger is created before Pass 0**, not before the first sweep: Pass 0 gets a row like
everything else, and a resumed session with a plan but no ledger would otherwise have no way to
tell whether the census had run.

```markdown
# <project> review findings — @ <sha>, <date>

## Pass log

| Pass | Status | Findings (C/H/M/L/D) | Not covered |
| --- | --- | --- | --- |
| 0  | done | — | coverage skipped (tool unavailable) |
| A1 | done | 2/5/3/1/2 | — |
| B1 | **in progress** | | |
```

Rules: one row per pass; `in progress` written *before* the pass starts and completed when it
ends; a pass whose row was never completed is **not done** and re-runs. "Not covered" is never
left blank.

Findings follow:

```markdown
## [B1-L5-02] Critical — <one-line claim>
path/to/file.ext:1284

**Failure:** <concrete input/state → the wrong outcome>

**Evidence:** <what in the code makes it so, with line refs>

**Invariant:** <if L2 — the rule, quoted>

**Fix sketch:** <the shape of the fix, plus the test that should fail first>

**Confidence:** high | medium — <what would raise it>
**Risk to fix:** low | medium | high — <blast radius>
**Status:** open
```

**ID:** `<slice>-<lens>-<nn>`. **Severity:**

| | Meaning |
| --- | --- |
| **Critical** | data loss/corruption, credential exposure, destructive action taken without consent |
| **High** | wrong results, crash, hang, silent divergence between backends/platforms |
| **Medium** | visible misbehaviour, edge-case failure, real perf regression on a real path |
| **Low** | quality, nits, cosmetic |
| **Debt** | architectural — no single failing input, real long-term cost |

---

## 6. Triage and the fix phase

**Triage in checkpoints, not once at the end.** A 30-pass review at a 12-finding cap can reach
300 findings, and a single final pass that must read them all, dedupe across slices and re-rank
globally is the one pass most likely to run out of room — while being the one whose output is
the actual deliverable. Run an interim triage after Tier A and then roughly every 6–8 slice
passes: dedupe what's accumulated, promote anything Critical, and move settled Debt/Low to the
backlog so the ledger the final triage reads is a fraction of the total. Each checkpoint is its
own pass with its own row.

At the final one: read what remains whole, dedupe across slices (one root cause often surfaces
in three), re-rank globally, and sort into buckets. Add it as a `## Triage` section at the top
of the ledger. **Triage changes no code.**

- **Bucket 1 — safe unattended:** pure-logic fixes, test-first.
- **Bucket 2 — attended:** anything touching UI lifetimes, concurrency, or framework internals
  — fix with the app running and verify for real.
- **Bucket 3 — never unattended:** destructive operations, write paths, credentials, migrations
  — fix attended and verify against real infrastructure.
- **Bucket 4 — backlog:** Debt and Low not worth a change now. Move them out with the
  rationale; don't leave them in the ledger.

Fixes follow the project's own testing convention (test-first if that's the house rule) and its
commit convention, one logical change per commit.

---

## 7. Automation

The pass log is external state, so a pass needs the ledger, not the previous session's context.
Context compaction and session boundaries are therefore harmless — the usual thing that breaks
long automated runs.

Automate the tail, not the head:

- **Attended:** setup, Pass 0, all of Tier A, and the highest-blast-radius slices. These set
  the frame or touch the paths where a miss costs data.
- **Automated:** the lower-risk slices. `codebase-review-pass` is parameterless by design, so
  `/loop` pointed at it runs them unattended and is safe to re-fire after an interruption.
- **Attended:** triage, including the interim checkpoints. It's a ranking judgment, which is the
  whole output.

Unattended, the "re-run a degraded pass" rule can loop: a pass that degrades for a structural
reason will degrade again. **Cap it at two attempts** — record the attempt count in the pass
row, and after the second, stop the loop and surface it rather than trying a third time.

**Don't fan slices out in parallel.** It looks like the obvious win and isn't: the gate in §3
needs the whole ledger in view (cross-slice duplicates are common and a parallel run can't see
them), and the risk ordering is deliberate — early slices inform how later ones are read.
