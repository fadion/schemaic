---
name: commit
description: Schemaic's commit flow — group the working tree into logical changes, run the pre-commit bar, weigh whether the change has earned a `/code-review` before it lands, then write the Conventional Commits message with its required trailer and record any review that ran. Use whenever the user asks to commit ("commit this", "commit the changes", "make a commit", "commit what we just did"), including when they ask only for the message. Not for pushing, tagging or releasing — that's the `release` skill — and not for reviewing a whole range, which is `release-review`.
---

# Committing in Schemaic

The order is **group → verify → weigh a review → commit → record**, and it is the
order for one reason: `/code-review` reads the *working diff*. Before the commit
it costs nothing to offer and the answer changes what gets committed; after the
commit the change is history, the offer needs a target, and a finding means a
follow-up commit instead of a better first one. So the review question is asked
while the tree is still dirty, or it isn't worth asking.

**Never commit unless the user asked.** Invoking this skill is the ask. Making
edits earlier in the session is not, and neither is finishing a task — leave the
work in the tree and say it's ready.

This skill does not push. Unpushed commits batch for the next tag, and `release`
owns everything from the push onward.

## Phase 0 — see what is actually changing

```bash
git status --short
```

```bash
git diff --stat
```

Then read the diff itself — not the stat. Phases 2 and 3 both need to know what
the change *does*, and neither judgement can be made from filenames.

**If something is already staged, that is the user's grouping.** Respect it:
review and commit only what's staged, and read it with `git diff --staged` — a
bare `git diff` shows the *unstaged* remainder, which is the opposite set. Mixing
the two up produces a message describing code that isn't in the commit.

**One logical change per commit.** If the tree holds two unrelated ones, say so
and propose the split rather than sweeping them into a message vague enough to
cover both. The user may well want them together — that's their call, but make it
a call rather than an accident.

## Phase 1 — the bar

```bash
cargo test --workspace
```

CLAUDE.md makes a green workspace suite a **pre-commit** rule, and it is the only
one. Green, and no `#[ignore]` added by this change.

```bash
cargo fmt --all --check
```

Formatting is a pre-*push* rule that `release` also gates on, but run it here:
it takes seconds, an unformatted tree is historically this project's most common
CI failure, and catching it now folds the fix into the change instead of leaving
a stray `style:` commit later. If it fails, run `cargo fmt --all` and include the
result in *this* commit — it is the same change, not a separate one.

Clippy (`cargo clippy --workspace --all-targets -- -D warnings`) is not a gate
here; CI runs it and a failure blocks the push, not this commit. Run it anyway
when the change is more than a few lines, because a fixup commit later costs more
than the wait now.

**A red bar is a stop.** Report what failed and leave the tree alone. Don't commit
broken work "so it isn't lost" — that's what the working tree is already for.

A docs-only or comment-only change may skip the bar. Say that you skipped it and
why, rather than silently reporting a commit as verified.

**One docs file is not exempt: CLAUDE.md.** `core/tests/doc_coverage.rs` asserts
that every `crates/schemaic-core/src/*.rs` module is named somewhere in it, so an
edit that drops or renames a module's mention turns the suite red — the one case
where a change with no `.rs` in it can. Run the bar for any CLAUDE.md edit.

## Phase 2 — weigh a review

**The default is silence.** Most commits here don't need this, and an offer on
every one teaches the user to skip past it — at which point the offer that
mattered gets skipped too.

**"Commit without review" skips this phase outright.** However it's phrased — "no
review", "just commit", "skip the review" — it settles the question, so don't
weigh the triggers, don't offer, and don't quietly note a deferral to raise the
same thing later. That last part is the one worth being deliberate about: on a
multi-commit feature, being told to skip cancels the offer at the end of the
feature too, not just this commit's. Say in the report that the review was
skipped on request, so it's visible rather than silently absent, and go straight
to Phase 3.

It binds the ask it was attached to, not the session. A later plain "commit"
weighs the triggers again from scratch — but read the room: having just been told
to skip, don't re-raise the same trigger on the same surface a commit later.

Two triggers. Either one fires, or say nothing — **unless the work is unfinished**,
in which case neither offers anything yet and the last part of this phase
applies instead.

**1 — the diff touches a documented invariant surface.** These are the places
CLAUDE.md marks as regression-prone, and a change to one earns a second pair of
eyes at almost any size:

- the write guard — `sql::run_verdict`, `TabsActions::run`/`run_all`, or any new path that executes user SQL
- the boundary lexer — `sql::skip_noncode` and anything built on it
- the identifier quoters — `export::ident_sql`/`ident_if_needed` and their delegations
- write-back — `commit_writes`, `GridWrite::plan`, `one_row_verdict`, `Rollback`
- DDL apply — `ddl::diff`/`emit`, `run_ddl`, the preview path, the round-trip fixtures
- secrets — `core::secrets`, the app's keyring store, connection persistence
- the pinned connection — `Session`, `tx.rs`
- a new `create_child()` / scope-disposal site

**2 — a *decision* changed and no test moved.** CLAUDE.md's coverage bar names
what a decision is: "parsing, analysis, formatting, export, diffing, key
selection, gating". A diff that adds or changes one of those and touches no
`#[cfg(test)]` block is worth flagging on its own account — the missing test is
the finding, and the review usually finds more.

Read "decision" narrowly, because the loose reading — *any* change in observable
behaviour — fires on presentation work, which is the one place this offer is
reliably wasted. **A change that only alters how something looks does not
trigger this**: a colour, a padding, an icon, a label's wording, a widget swapped
for one that renders the same state. Those are verified by looking at them
(that's the standing preference for small visual tweaks), and the two things
about them a test *can* catch — the contrast gate and the theme-fn capture rule —
are already test-enforced and will have failed Phase 1 before you get here.

**What does not trigger it: size, commit type, or a hunch.** 25 of the 27 commits
in the `v0.12.0..HEAD` range were `feat` or `fix`, median 2 files. A type or size
filter fires on nearly everything, which makes it no filter at all — that is why
the two triggers above are about *surface* and *decisions* instead.

### Unfinished work defers instead of offering

This outranks both triggers, because it changes what the offer even is.

A feature big enough to need several commits fires trigger 1 on every one of
them, and each review is worse than the wait: commit 3 of 6 is a half-wired
feature, so the findings are about the half that isn't wired yet, and the user
declines six times to hear the same thing. Review it once, when it's whole.

So when the change is one step of work still in progress — the user said so, the
TODO entry is still open, or the code is visibly half-wired (a new module with no
caller, a field nothing reads yet) — **name the trigger in a line, say the review
is deferred to when the feature lands, and commit.** Don't offer.

At the commit that completes the work, the offer covers the whole feature, not
that last diff. Nothing has to be remembered across sessions to know what that
is: unpushed commits batch for the next tag, so the range is on disk.

```bash
git log --oneline origin/main..HEAD
```

Read it, take the first commit of the feature, and offer the **range** reviewer:

```bash
release-review <first-sha>^..HEAD
```

`/code-review` is the wrong tool here — it reads the *working diff*, and the
earlier commits aren't in it. `release-review` takes an arbitrary range by
design, slices it by size, and ends at a ledger.

This deliberately gives up Phase 2's ordering: those commits already exist, so a
finding becomes a follow-up commit rather than a better first one. That is the
right trade while nothing is pushed — none of it is published history until
`release` runs, and a fix landing as commit 7 of 7 costs nothing.

If a deferral never resolves — the feature is abandoned, or the user moves on —
let it go. The release review reads the whole range either way; that is what
makes deferring safe rather than a thing to chase.

When one fires on finished work, say it in a line: which trigger, and what it
touched. Then give the command and **stop**:

```bash
/code-review high
```

**`high` is the default — offer it unless the user asked for a different level.**
Nothing reaches this point without clearing the silence bar above, so by
construction every change that gets an offer is one of the two kinds worth
reading properly; grading them down again would undo the filtering. `low` and
`medium` are for when the user names them.

Always name a level, whichever it is: a bare `/code-review` silently reuses the
last one typed, which makes the depth of the review a function of whatever
happened to be reviewed before it. **`ultra` you cannot launch** — it is
user-triggered and billed, so if the change warrants it, print the command and
say it has to come from them.

Offer once. If the user declines, commit and don't raise it again. It is not a
gate, and it never blocks the commit.

If the review runs:

- nothing is committed yet — that is what this ordering buys, so let findings change the code rather than following it
- `--fix` writes to the working tree, so **re-run Phase 1** afterwards and re-read the diff; the message you were about to write may no longer describe the change
- then continue to Phase 3, and record the outcome in Phase 4

## Phase 3 — the commit

Format, per CLAUDE.md, which stays the authority:

- `type(scope): subject` — imperative, no trailing period, lower-case after the colon
- types: `feat` `fix` `refactor` `perf` `docs` `test` `chore` `build` `ci`
- scope = the crate or module the change centers on (`grid`, `editor`, `schema`, `ai`, `sql`, `theme`, `db`, `ci`…); omit only when genuinely cross-cutting
- optional body after a blank line, explaining the **why** — the diff already says what
- every message ends with the trailer `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`

```bash
git log --oneline -20
```

Read that before writing the subject. The history has a voice — subjects say what
the change does *for the codebase*, often with the consequence attached ("let
Escape blur a text field so a modal can still close"), rather than narrating the
edit. A new subject should read like its neighbours.

Multi-line messages never go through `-m`: PowerShell reads a `>=` inside the
text as a redirect and the message arrives split into pathspecs. Use `-F`, and
prefer stdin over a temp file nobody remembers to delete — the Bash tool is a
POSIX shell, so a heredoc works:

```bash
git commit -F - <<'EOF'
type(scope): subject

Body.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
```

Three things this phase never does: stage work that isn't part of this change;
touch `[workspace.package].version` (bumps are explicit-only and belong to
`release`); or run `git tag` / `git push`.

The sin is *sweeping*, not the flag — when Phase 0 established that the whole
tree is the one change, `git add -A` is the honest way to say so. When it isn't,
name the paths.

## Phase 4 — record the review, if one ran

Only when Phase 2's offer was taken **and it was `/code-review`**. No review,
nothing to write — and a deferred range review needs nothing either: it runs
*after* the commit, and `release-review` writes its own ledger under `review/`,
which would only duplicate this one.

The commit SHA doesn't exist until the commit does, so this is the last step:

```bash
git rev-parse --short HEAD
```

Write `review/commits/<sha>.md` — **one file per commit, never a shared ledger.**
`review/` is gitignored, so there is no `git show HEAD:path` and no `git restore`,
and this directory now has two kinds of writer: this skill, often, and the review
skills, rarely. A per-file layout means a bad write costs one commit's notes
rather than the cycle's, two sessions committing at once cannot collide, and the
SHA in the filename is the staleness key `release-review` needs. Write it once
with `Write`; if you must extend it, use `Edit`. **Never mutate one from a shell
script** — a PowerShell splice destroyed this project's review ledger once
already, and the rule applies with more force to a file git cannot restore.

```markdown
# <short-sha> — <commit subject>
Reviewed at: /code-review <level> · <date>
Outcome: <n> open · <n> fixed before commit · clean

## [C<short-sha>-01] High — <one-line claim>
crates/schemaic-core/src/foo.rs:412

**Failure:** <concrete input/state → the wrong outcome>

**Evidence:** <what in the code makes it so, with line refs>

**Fix sketch:** <shape of the fix, plus the test that should fail first>

**Confidence:** high | medium — <what would raise it>
**Risk to fix:** low | medium | high — <blast radius>
**Origin:** introduced
**Status:** open
```

The block shape is reference §5's on purpose, so `release-review` can lift an
entry without reshaping it. Two deliberate differences:

- **IDs are `C<short-sha>-nn`.** §5's `<slice>-<lens>-<nn>` names a slicing that
  doesn't exist at commit time, and reusing that shape would eventually put two
  different findings under one ID.
- **`Origin: introduced`** is true by definition here, so it is recorded rather
  than derived — it saves the release review a `git blame` per entry.

Record three outcomes, not one. Findings left **open**; findings `--fix` closed
before the commit existed (`Status: fixed-before-commit`, so nobody re-litigates
them); and a review that found **nothing**, which is genuine context for triage
later even though — see below — it is never a licence to skip anything.

## What the record is for, and what it is not

It is **prior input** to `release-review`, never a substitute for coverage.

A commit-time review sees one diff. Commit A adds a caller, commit F changes the
callee's contract, and neither diff contains the bug — while `release-review` is
built precisely to catch that, reading whole functions and tracing into unchanged
callers. So these notes save triage effort and get real fixes in early. They do
not shrink what the release review has to read, and "already reviewed at commit
A, skip this slice" is the one conclusion never to draw from them.

The directory is archived at the tag, not at the end of the review — a finished
review can sit for days while its fixes land, and that is exactly when the record
is still wanted. `release` owns that step.

## Finishing

Report, in a few lines: the subject and short SHA, what the bar ran and what it
said, whether a review was offered — and taken, declined, **deferred to the end
of the feature**, **skipped on request**, or not warranted — and where the record
went if one was written. If any part of the bar was skipped, name it and why. If the tree still
holds unrelated changes you deliberately left out, say what's still uncommitted.

A deferral is worth a sentence every time it happens, not just once: it is the
only thing here the user can't see in `git log`, and it is what makes the offer
at the end of the feature make sense rather than arrive from nowhere.
