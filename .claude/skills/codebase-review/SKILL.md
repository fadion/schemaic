---
name: codebase-review
description: Set up and start a multi-level review of the Schemaic codebase — freeze a clean tree, cut the code into review slices, run a mechanical fact sheet, then the repo-wide sweeps. Use when the user asks for a full/deep/multi-level codebase review, a review "by module" or "in different lenses" (bugs, quality, performance, architecture, security), an audit of the whole repo, or says "start the review" / "set up a codebase review". Produces review/plan.md + review/findings.md; individual passes after setup are run by the codebase-review-pass skill. Do NOT use for reviewing a pull request or the working diff — that's /review and /code-review.
---

# Start a codebase review

A whole-codebase review that stays useful is **slice × lens**, run as a sequence of small passes
against a frozen tree, with every finding written to one ledger. This skill does the setup and the
repo-wide part; `codebase-review-pass` runs everything after that, dispatching each pass to its own
subagent and stopping only at the attended checkpoints.

`reference.md` (next to this file) is the method spec — lenses, per-pass protocol, the verification
gate, the ledger format and its integrity rules, the orchestration contract. **Read it before
Phase 2.** Both skills follow it, so it's the one place the method lives.

**This skill is Schemaic-specific.** The parts below that name cargo commands, CLAUDE.md's
invariants, this repo's traps and its scope decisions are the highest-yield content in it — a
generic reviewer misses all of it. Copying this to another project means copying the whole
`.claude/skills/codebase-review*` pair and rewriting those sections for that repo; `reference.md`
transfers unchanged.

## Artifacts

Everything goes in `review/` so it can be ignored with one line:

| File | Role |
| --- | --- |
| `review/plan.md` | The work order for *this round* — the slice table with measured LOC, attended/delegated flags, the triage schedule. Generated in Phase 1. |
| `review/facts.md` | Pass 0's mechanical fact sheet. |
| `review/findings.md` | The ledger: pass log (the cursor), findings, rejected candidates. |
| `review/index.md` | One line per finding (`ID \| severity \| file:line \| claim`) + the rejected one-liners. What each subagent reads instead of the ledger. |
| `review/.snapshots/` | Per-pass ledger snapshots (reference §5). Kept, not rotated. |

What lives *here* rather than in `plan.md` is everything durable across rounds: the preconditions,
the A1 evidence set, the reviewer notes, the scope decisions. `plan.md` holds only what is measured
against one frozen SHA and gets revised mid-round — so a revision never dirties the frozen tree.

If `review/plan.md` already exists, the review is set up — **don't regenerate it**. Hand straight
over to `codebase-review-pass`.

Offer to add `/review/` to `.gitignore` if it isn't there. These are working notes.

## Phase 0 — freeze the tree

A review of a moving tree produces `file:line` refs that rot before anyone fixes them, and burns
passes on churn.

**Prefer a worktree at a tag over freezing the live tree:**

```bash
git worktree add ../schemaic-review v0.11.0
```

A tag never moves, so every `file:line` stays valid for as long as the review runs, and normal
development continues on `main` meanwhile. Offer this first; freezing the checked-out tree is the
fallback when there's no suitable tag. Record which mode is in use at the top of the ledger — and
if it's the live tree, tell the user plainly that **it has to stay at this SHA for the whole
review**, and that committing between passes means later passes stop and ask.

Then verify, in the review tree, in order:

1. `git status --short` empty. **The review's own artifacts count** — if these skills or the
   `/review/` line in `.gitignore` are uncommitted, commit them first rather than reviewing from a
   dirty tree.
2. `cargo fmt --all --check` → exit 0.
3. `cargo clippy --workspace --all-targets -- -D warnings` → clean.
4. `cargo test --workspace` → green, no `#[ignore]`.
5. `cargo build -p schemaic-app` → ok, and the app launches and connects against **at least one**
   live engine. Record which. Both is better: B4/B5 lean on being able to check a MySQL/PostgreSQL
   divergence claim against a real server, so if only one is reachable, say in the ledger that the
   other engine's behaviour is unverified rather than quietly proceeding.
6. Record `git rev-parse HEAD` at the top of `review/findings.md`.

**If any of these fails, stop and report.** Don't fix them uninvited and don't review anyway — the
user decides.

Two environment facts that have already cost a pass here:

- **Windows:** stop a running `schemaic.exe` before building, or the linker can't overwrite it.
- **The agent's `%APPDATA%` view is MSIX-redirected.** An app instance launched by the agent does
  not share connections, tabs or history with the user's app. Any pass that drives the GUI must
  state which instance it exercised; any conclusion mixing the two views is invalid.

A precondition naming live infrastructure must be *checkable*. If it can't be met, say which part
is unverified and let the user decide — don't quietly downgrade it, because a later pass will read
the gap as covered.

## Phase 1 — generate the plan

Inventory first: crates, line counts per file, test counts per file, and CLAUDE.md's architecture
notes. Then cut slices.

**Slicing heuristics** — the part that makes or breaks the review:

- **Cut along flows, not directories.** A slice should be one capability end to end (core logic +
  I/O path + UI). In this codebase the bugs that matter cluster in the **core→db→ui seam**, so a
  slice that is "one directory" misses exactly those.
- **A slice is a capability; a pass is ~2.5k lines.** Different units — conflating them is how a
  plan under-promises. Aim for **12–20 slices**, then give each `ceil(LOC / 2.5k)` passes, numbered
  `B7.1`, `B7.2`. The pass log tracks passes; findings group by slice.
- **Measure the lines, don't estimate, and do the arithmetic in the plan.** State the total
  invocation count (passes + Pass 0 + sweeps + triage checkpoints) up front — that number is what
  the user budgets against, and it's the single easiest thing for a plan to get wrong.
- **Name each sub-pass's split point in the plan**, not mid-pass. Deferring it pushes the decision
  to the moment of maximum incentive to skim, and a pass that skims is worse than one that doesn't
  run, because it looks covered.
- **Order by blast radius, not size.** Slices where a bug destroys data or leaks a credential go
  first. Bulk-write paths (import, DDL apply, write-back) belong at the top even when they feel
  peripheral. State the rationale, then check the order against it.
- **Assign lenses per slice, not all lenses to every slice.** Typically 2–4. Most cells in a full
  module × lens matrix are empty and filling them produces noise.
- **Flag each pass attended or delegated** (reference §7). Default is delegated. Attended: the plan
  sign-off, A1, and every triage checkpoint. Add a pass to the attended set only for a specific
  reason, and write the reason in the row.
- **Verify coverage mechanically before showing the plan.** List every source file, check each
  appears in exactly one slice, and state in the plan that it does. Prose like "the plan/monitor
  pairs" reads as coverage and isn't — that's how files go missing.

Also define in the plan: the **triage schedule** (reference §6 — after Tier A, then every 6–8 slice
passes; budget them against the *final* slice count, or the final triage opens on a pile of unseen
findings) and any **scope decisions** specific to this round.

**Show the plan and get agreement before Phase 2.** The slice map determines everything downstream.

## Phase 2 — Pass 0, the mechanical fact sheet

Create `review/findings.md` **first** — frozen SHA and freeze mode at the top, then the pass log
with Pass 0's row marked `in progress`. The ledger has to exist before the first pass, or Pass 0
has nowhere to record itself and a resumed session can't tell it ran. Create `review/index.md`
empty at the same time.

Machine-findable issues must never consume a model pass:

```bash
cargo clippy --workspace --all-targets -- -W clippy::pedantic -W clippy::nursery
```

plus the census below, written to `review/facts.md`.

| Row | Kind | Feeds |
| --- | --- | --- |
| `unwrap()` / `expect()` / `panic!` / `unreachable!` / raw `[i]` outside tests | census | L1 panic surface |
| `unsafe` blocks | census | L5 |
| `#[allow(...)]` in source | census — each is a silenced signal | L3 |
| `TODO` / `FIXME` / `XXX` / `HACK` | census | L3 / triage |
| `cargo deny check`, `cargo udeps` (nightly), `cargo tree --duplicates` | census | L5 / L3 |
| `cargo llvm-cov --workspace --summary-only` per file | census | L6 baseline |
| `exec_after(` sites | census (small, enumerable) | L1 disposed-signal hazards |
| `.clone()` on `Vec`/`String`/`Arc<Vec>` inside `.style(`/render closures | **candidate list** — counts per file only | L4 |
| `get()` vs `get_untracked()` | **candidate list** — counts per file only; which is correct is a per-site judgment and there are hundreds | L1 |

**Separate a census from a candidate list.** A census is mechanical and its output is a fact. A
grep whose hits still need per-site judgment is a *candidate list*, and it belongs in the fact
sheet as counts and file distribution only, feeding a later pass. Putting one in as a census either
buries the real facts under hundreds of undifferentiated hits or quietly turns Pass 0 into a model
pass.

**Note explicitly whatever failed to run** (no nightly toolchain, an uninstallable coverage tool).
A silently dropped census reads later as a clean result — and A5 leans on the coverage numbers.

Then stop and summarize.

## Phase 3 — Tier A sweeps

Standard shapes in reference §2. What makes each concrete here:

| # | Sweep | This codebase |
| --- | --- | --- |
| **A1** | Invariant conformance | Walk CLAUDE.md's *Architecture invariants* and *Floem 0.2 gotchas* one at a time, enumerating **every** site each governs: (a) every SQL scan → through `sql::skip_noncode`, with the right `SqlDialect`? (b) every `Db::` method + `Session` use → one-connection-per-op except manual-tx; do read-only side channels stay off the pinned connection? (c) every secret read/write → through `core::secrets` + the keyring store, never `persist::save_connections` directly? (d) every `ALTER`/`CREATE`/`DROP` string → originates in `ddl::emit`, terminates at `ddl_preview`? (e) every `create_child()`/`dispose()` → deferred? (f) every `Color` captured by value into a `.style(` closure. (g) `>= 0x80` word-byte handling in every identifier scanner. |
| **A2** | Architecture & boundaries | Crate dep graph; what leaks from `schemaic-db` up into `schemaic-ui`; the `Ui` bundles vs. what closures actually capture; the god modules (`intel.rs`, `grid.rs`, `ui/lib.rs`, `main.rs`) — still cohesive, or landfills? Are `SqlDialect` / `SecretStore` / `RowSource` / `Catalog` real seams, or is dialect logic still scattered? |
| **A3** | Security & data safety | Secrets end to end (keyring ↔ JSON ↔ memory ↔ subprocess env ↔ logs); every identifier/literal reaching SQL text — is anything bypassing `export::ident_sql`/`sql_literal`? the MCP and AI read-only gates; the write-back 1-row net and `TxScope` SAVEPOINT nesting; `run_ddl` atomicity honesty per engine; SSH TOFU; import/export path handling; panic messages carrying data. |
| **A4** | Performance map | Per-keystroke (diagnostics / completion / highlight), per-frame (grid render, the `ColWindow` memo), per-scroll, per-result-set (fetch → model → widths), import/export streaming. Output: ranked ≤8 paths worth deep L4 attention. |
| **A5** | Test map | Decision functions vs. tests per module; logic in UI/app that belongs in core; and whether the four invariant-enforcing tests still enforce — the DDL round-trip gate, the 1-row write-back net, lexer agreement, export `*_to` byte-equality. |

A1 is **attended** — a wrong call there is read as settled by every slice pass after it. A2–A5
delegate like any other pass. Run them one at a time, appending to the ledger, and pause after A1.

## Project-specific notes for reviewers

These go into every subagent's brief. They are the difference between a review of this codebase and
a generic one.

- **The invariants are the point.** CLAUDE.md's *Architecture invariants* and *Floem 0.2 gotchas*
  each encode a bug already paid for — five drifting SQL lexers, plaintext credentials,
  disposed-signal panics, silent `MODIFY COLUMN` data loss. **L2 outranks everything else here.**
- **This codebase is deliberately conservative in specific places**, and each is a trap for a
  plausible-but-wrong finding: `intel::colres` treats an unenumerable source as *open* so
  uncertainty never yields a false positive; `ddl::pg_replaceable` resolves uncertainty to
  replace-and-let-the-server-refuse, never to drop; `commit_writes` requires exactly 1 row per
  statement; the DDL round-trip gate asserts a draft built from a table diffs to nothing. **Check
  the guard before claiming the gap** (reference §3.2).
- **The DB is the semantic authority** by design — absence of client-side type checking is a
  decision, not a gap.
- **Two engines, one model.** Anywhere MySQL and PostgreSQL diverge (DDL atomicity, `MODIFY` vs.
  per-statement `ALTER`, view replace semantics, comment syntax, quoting) is where divergence bugs
  hide. The PG slice explicitly cross-checks against the MySQL one.
- **Testing is TDD by house rule** — an L6 finding names the test that should exist, and a fix
  starts red.
- **Never bulk-rewrite source with a script.** CLAUDE.md says why; it cost ~900 lines once. The
  ledger falls under the same rule and is worse off, since it isn't in git (reference §5).

## Scope decisions

Carried across rounds unless the user changes them: `THIRD-PARTY-NOTICES.md`, CI workflow files,
the `release` skill, packaging (TODO.md owns that track), `build.rs`, and
`crates/schemaic-core/examples/result_footprint.rs` (a throwaway measurement harness) are out of
scope. Round-specific additions go in `plan.md`.

## Then hand over

After A1, tell the user setup is done and that `codebase-review-pass` now runs the remaining passes
— delegating each to a subagent and stopping only at attended passes, triage checkpoints, an
escalated Critical, or a pass that degraded twice. Don't keep going yourself unless asked.

## Hard rules (both skills, every pass)

- **Edit no source file.** This phase produces findings; fixing is a separate phase with its own
  risk buckets. A session that starts fixing loses the ledger and the ranking.
- **A Critical is escalated, not queued.** Finding data loss, corruption or a credential leak at
  pass 3 of 40 and burying it for a fortnight is its own failure. Write it up, tell the user
  immediately in that pass's report, and say plainly that it warrants fixing now. The fix still
  doesn't happen here: it goes on a branch off the frozen SHA/tag, so the review's line references
  stay valid and the ledger stays the record.
- **No scripted or bulk edits** — of source or of the ledger (reference §5).
- **The verification gate is not optional** (reference §3). Mature codebases are full of deliberate
  conservatism that makes plausible-sounding findings wrong.
- **Cap findings per pass.** More than ~12 means you aren't ranking.
- **Every pass states what it did not cover.** A blank there is a claim of full coverage.
