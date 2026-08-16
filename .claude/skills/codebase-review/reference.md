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
| **L2** | **Invariant conformance** | the architecture rules the project documents for itself (`docs/architecture.md`, ADRs, conventions docs) and the framework hazards it has written down | the invariant quoted + the violating site |
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

### Writing to the ledger — non-negotiable

The ledger is gitignored, so there is no `git show HEAD:path` and no `git restore`. It is also the
review's only durable state. Both facts point the same way.

1. **Append; never splice.** New findings go in at the `**Status:** open` → `---` → `## Rejected`
   boundary via `Edit`, or with `cat >> … <<'EOF'`. Both are structurally incapable of truncating
   the file. Reads may use anything.
2. **Never mutate the ledger from a shell script.** A PowerShell list-splice destroyed this
   project's ledger mid-review — 9,720 lines to 176 — because `$lines[0..$n]` yields `object[]`,
   `List[string].AddRange` rejected it, and `$ErrorActionPreference` defaults to `Continue`, so
   two non-terminating failures still reached the write. The errors *printed*; they just didn't
   stop anything. The project's own rule against scripted source rewrites applies here with more
   force, not less, because the ledger isn't in git.
3. **Snapshot before each pass, and keep the snapshots** —
   `review/.snapshots/findings-<pass-id>.md`. Not one rolling `.bak`: a corruption nobody notices
   gets copied over the last good copy on the next edit, and delayed detection is exactly the
   failure an unattended run invites. A round's snapshots cost a few tens of MB.
4. **Verify after every edit**, before trusting it:
   - size delta ≤ ~20% for a single edit (9,720 → 176 fails this instantly);
   - line count and finding-ID count never *decrease* during a pass. Triage is the exception —
     it moves settled Debt/Low to the backlog, so it states its expected delta before editing and
     checks against that;
   - `## Pass log` and `## Rejected` both still present, `## Rejected` still last.

   Any failure ⇒ restore from the snapshot. Don't retry the same way.
5. **If it happens anyway**, the Claude Code session transcripts
   (`~/.claude/projects/<project>/*.jsonl`) are a complete write log: extract every `tool_use`
   targeting the ledger, sort by timestamp, replay (`Write` → whole file, `Edit` →
   replace-first-occurrence, appended heredoc → append). **Dedupe by `tool_use` id** — resumed and
   compacted sessions re-log earlier messages, and an undeduped replay double-applies. Validate
   against a known line count before trusting the result.

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

## 7. Orchestration

The pass log is external state, so a pass needs the ledger, not the previous session's context.
Session boundaries and compaction are therefore harmless — the usual thing that breaks long
automated runs.

That same fact is what makes delegation correct rather than merely convenient: **each pass runs in
its own subagent**, started cold with its plan row and this file, instead of in a session carrying
residue from thirty unrelated slices. The orchestrator (`codebase-review-pass`) never reads a
findings block in full — only the plan row, the subagent's report, and greps of the ledger — which
is what lets one session drive a whole segment without drowning.

### Attended vs. delegated

The attended set exists for decisions that mis-shape everything downstream, not for supervision.
Keep it minimal.

- **Attended:** the plan sign-off (the slice map determines every later pass and is expensive to
  discover wrong at pass 12); **A1** (a mistaken call in the invariant census is read as settled
  by every slice pass after it); every **triage** checkpoint including the final one — triage is a
  ranking judgment, and the ranking is the deliverable.
- **Delegated:** everything else, Pass 0 and A2–A5 included.

A pass needing a live GUI or a live database still delegates. It is flagged in its plan row and
must state in its report which instance or server it exercised — an unstated live check reads
later as a performed one.

### The loop

Per iteration:

1. `git rev-parse HEAD` still matches the ledger's frozen SHA. If not, stop — every `file:line`
   is anchored to it.
2. **Audit the previous row.** Mechanical: the expected number of blocks with that pass's ID
   prefix landed, each carries Failure / Evidence / Confidence, "Not covered" is non-empty, the
   row isn't still `in progress`. This audit is what stands in for the checkpoint delegation
   removed — a degraded pass propagating unnoticed is the failure mode of any unattended chain.
3. Pick the first row that isn't `done`. **Attended pass or triage checkpoint ⇒ stop and hand
   back.**
4. Mark it `in progress`, then snapshot the ledger (§5).
5. Dispatch the subagent. Wait.
6. **Guards, on return:** `git status --short` in the frozen tree must be empty — `review/` is
   gitignored, so anything there means the subagent edited source: revert it, record `attempt 2`,
   re-run. Then the §5 ledger checks.
7. Close the row, append the pass's findings to the index, loop.

**Break the loop** on: an attended pass, a triage checkpoint, a Critical, or a pass that degraded
twice. A Critical does not wait for the next checkpoint (§3) — surfacing it is the whole point of
escalation, and an orchestrator that keeps going has re-buried it.

**Cap re-runs at two**, recorded in the row. A pass that degrades twice is degrading structurally
— the slice is too big, the split point is wrong, or the lens doesn't fit — so stop and say which.

### `review/index.md`

One line per finding — `ID | severity | file:line | one-line claim` — plus the running `Rejected`
one-liners. The orchestrator is its only writer.

It exists because §3's gate assumes the whole ledger is in view, and no subagent can hold a ledger
that has grown to hundreds of findings. The index is what a pass reads instead: enough to
recognize a cross-slice duplicate by its claim and then go read that one block. Each triage starts
from it too.

### The subagent's brief

It inherits nothing. Everything it needs is in the prompt: its plan row (slice, files, lenses,
split point), this file's §3–§5, the project's architecture/conventions docs, `review/index.md`,
and the ledger's frozen-SHA header. It appends its own findings block at the `## Rejected`
boundary and touches nothing else in the ledger — the orchestrator owns the pass log and the
index, so there is exactly one writer per region.

It returns a compact report, not the findings themselves: pass id, counts by severity, finding IDs
with one-line claims, "Not covered", and an explicit statement that each finding cleared §3.

The hard rules — edit no source, no scripted or bulk edits, never mutate the ledger from a shell
script, cap 12, escalate a Critical in the report — must be restated in the prompt every time.

### Why not in parallel

Fanning a segment's slices out concurrently is the obvious speed-up and is deliberately not taken.
The risk ordering is load-bearing: early slices inform how later ones are read, and cross-slice
duplicates are common enough that the interim triages exist specifically to catch them. If the
wall-clock ever justifies it, the only defensible shape is fan-out *within* a triage segment
(segments stay sequential, dedupe still happens at the checkpoint) with each pass writing
`review/passes/<pass-id>.md` for the orchestrator to merge — never concurrent writers on the
ledger.
