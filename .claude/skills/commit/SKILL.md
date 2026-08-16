---
name: commit
description: Schemaic's commit flow — group the working tree into logical changes, run the pre-commit bar, then write the Conventional Commits message with its required trailer. Use whenever the user asks to commit ("commit this", "commit the changes", "make a commit", "commit what we just did"), including when they ask only for the message. Also carries the single-pass prompt for reviewing a range of commits, for when the user asks for one. Not for pushing, tagging or releasing — that's the `release` skill — and not for the full pre-release audit, which is `release-review`.
---

# Committing in Schemaic

The order is **group → verify → commit**.

**Never commit unless the user asked.** Invoking this skill is the ask. Making
edits earlier in the session is not, and neither is finishing a task — leave the
work in the tree and say it's ready.

**Reviews are the user's call, and this skill never makes it.** Don't weigh
whether a change has earned one, don't offer, don't flag a surface as
review-worthy, and don't note a deferral to raise later. The user decides before
a commit or after a multi-commit feature, and says so. When they do ask, the two
shapes are in *Reviewing on request* below. Everything published is read again by
`release-review` before it ships, which is what makes silence here safe rather
than optimistic.

This skill does not push. Unpushed commits batch for the next tag, and `release`
owns everything from the push onward.

## Phase 0 — see what is actually changing

```bash
git status --short
```

```bash
git diff --stat
```

Then read the diff itself — not the stat. Both judgements left in this skill —
how the tree splits into logical changes, and what the message says the change
does — need to know what the code actually does, and neither can be made from
filenames.

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

**One docs file is not exempt: `docs/architecture.md`.** `core/tests/doc_coverage.rs`
asserts that every `crates/schemaic-core/src/*.rs` module is named somewhere in it, so
an edit that drops or renames a module's mention turns the suite red — the one case
where a change with no `.rs` in it can. Run the bar for any edit to that file.

## Reviewing on request

**Not a phase.** Nothing here runs unless the user asks for a review. Two shapes,
chosen by *what* is being reviewed rather than by how large it is.

### The working tree, before it lands

The usual case, and the reason to ask before committing rather than after —
nothing is committed yet, so findings can change the code instead of following it.

```bash
/code-review high (working tree only — git diff HEAD, not the unpushed range)
```

**Name the scope, not just the level**, and that parenthesis is why. Left to
itself the review takes `git diff @{upstream}...HEAD` and folds the working tree
in on top. That default suits a branch-per-change workflow; here commits batch on
`main` for the next tag, so `@{upstream}` is however many commits back the last
push was — twenty-seven, at the time this was written — and a pre-commit review
reads all of them alongside the two files actually in question. The working tree
isn't excluded, it arrives buried, and the review costs many times what it should.
None of `/code-review`'s documented targets (a PR number, a branch, a path) spells
"working tree", so the scope goes in the argument as plain text — the skill body is
instructions the reviewer follows, so a clearly stated scope is honoured anyway.

Always name a level too: a bare `/code-review` silently reuses the last one
typed, which makes the depth of the review a function of whatever happened to be
reviewed before it. **`ultra` you cannot launch** — it is user-triggered and
billed, so print the command and say it has to come from them.

`--fix` writes to the working tree, so **re-run Phase 1** afterwards and re-read
the diff: the message you were about to write may no longer describe the change.

### A range of commits that already landed

A multi-commit feature the user wants looked at now that it is whole.
`/code-review` cannot do it — it reads the *working diff*, and those commits
aren't in it. Find the range:

```bash
git log --oneline origin/main..HEAD
```

Take the first commit of the feature and review `<first-sha>^..HEAD` in **one
pass** — in this context, or a single subagent if the range would crowd it — with
this brief:

> Review the commits in `<range>` as one pass — no slicing, no ledger. Read
> `git diff <range>` in full, plus enough surrounding code to judge it. Focus on
> three things, in order: **correctness bugs** (wrong conditions, missing guards,
> broken callers, disposal and focus hazards), **`docs/architecture.md`'s architecture
> invariants** for the surfaces the range touched, and **data safety** (writes,
> DDL, secrets, quoting) where it touches them. Report findings inline, most
> severe first, each with the concrete scenario in which the code misbehaves.
> Don't fix anything.

**One pass, deliberately.** `release-review` is the wrong instrument here: its
cost is dominated by fixed overhead — a fact sheet, three range sweeps, a final
triage — which runs whether the range is two commits or twenty-seven. A
two-commit feature was billed two hours of review plus an hour of fixes, what a
whole release audit costs, for a fraction of the ground. A single reader finds
less than a seven-pass fan-out; that is the trade, made knowingly.
`release-review` reads the same commits again before anything ships, so this is a
first look and never the only one.

**Nothing is recorded** — no file, no ledger. `release-review` does its own full
pass over the same commits, so a record here would only be a second copy to keep
honest. Report the findings in the conversation and let the fixes land as
ordinary commits.

## Phase 2 — the commit

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

## Finishing

Report, in a few lines: the subject and short SHA, what the bar ran and what it
said, and — only if a review actually ran because the user asked for one — that
it ran and what came of it. If any part of the bar was skipped, name it and why.
If the tree still holds unrelated changes you deliberately left out, say what's
still uncommitted.

Nothing here reports on a review that *didn't* run. There is no offer to decline
and no deferral to track, so "no review was warranted" is not a line this skill
has to write — it is a judgement it doesn't make.
