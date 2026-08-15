---
name: release
description: Schemaic's push-and-release flow — push main, wait for CI, bump the workspace version, tag, wait for the Release workflow, then write the GitHub release notes. Use this whenever the user asks to cut, tag, ship, or publish a release, or says anything like "push and bump to 0.10.0", "bump to v1.2.0 and push", "release 0.11", "tag a new version", or "ship this". Also use it when they ask only for part of the flow (just the bump, just the release notes) so the surrounding steps and their gates aren't skipped by accident.
---

# Cutting a Schemaic release

The flow is push → **gate** → bump+tag → **gate** → notes. The two gates are the
point of the whole thing: the tag is what triggers the binary build, so tagging
on a red CI ships a broken release to whoever downloads it, and a tag is
awkward to retract once people have it. Never skip a gate to save time — if the
user is in a hurry, tell them what you're waiting on rather than tagging blind.

Work through the phases in order. Report the version and what's about to happen
before phase 2 (the first irreversible step is the tag push).

## Phase 0 — check the ground

```bash
git status --short
```

The tree must be clean. Uncommitted work means the user has something in flight:
stop and ask rather than sweeping it into the release.

Then confirm the tree is green locally. Run **all five** — this list mirrors
`.github/workflows/ci.yml`, so check it against that file rather than against
memory, and add anything CI has grown since. A local bar narrower than CI's is
worse than no local check: it reports the ground as clear and the push fails
anyway.

```bash
cargo fmt --all --check
```

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

```powershell
$env:RUSTDOCFLAGS = '-D warnings'; cargo doc --workspace --no-deps
```

```bash
cargo deny check
```

```bash
cargo test --workspace
```

An unformatted tree is historically the most common CI failure, and rustdoc the
most commonly *forgotten* check — nothing in day-to-day work runs it, so a doc
link left pointing at a renamed item sits green locally and fails the release
push. It has already cost one.

If `fmt --check` fails, run `cargo fmt --all`, commit the result as its own
`style:` or `chore:` commit, and carry on. A broken doc link is the same shape:
fix it, commit it as `docs:`, carry on. Failing clippy, `deny` or tests is a stop
— that's real breakage, not a formatting nit.

Also read the current version so you can sanity-check the requested one:

```bash
grep -n '^version' Cargo.toml
```

The new version must be greater than the current one, and the tag must not
already exist (`git tag --list vX.Y.Z`). If either is off, stop and ask.

Finally, check whether the range being shipped has been reviewed. If
`review/release-<last-tag>/findings.md` doesn't exist — or exists but its
header records a head SHA older than the current one — mention it once and
offer the `release-review` skill, which reviews `<last-tag>..HEAD`
autonomously and writes its findings there. It's an offer, not a gate: the
user may well have reviewed another way, and a green tree is what the two
gates below actually protect. Don't run it uninvited, and never run it after
the tag — its whole value is arriving before one.

## Phase 1 — push the work, wait for CI

```bash
git push origin main
```

Then wait for the **CI** workflow on `main`. There's a short lag before the run
appears, so retry the lookup a couple of times if it comes back empty:

```bash
gh run list --branch main --workflow CI --limit 1 --json databaseId,status --jq '.[0].databaseId'
```

```bash
gh run watch <id> --exit-status
```

`--exit-status` is what makes failure loud — without it the command succeeds
even when the run failed. CI finishes in about three minutes, so watching it is
fine; if the watch ever gets cut off by a command timeout, fall back to the
polling loop in phase 3 rather than assuming the worst.

If CI is red, stop: report which job failed and paste the failing step's output
(`gh run view <id> --log-failed`). Do not proceed to the tag.

## Phase 2 — bump and tag

Edit **one** place: `[workspace.package].version` in the root `Cargo.toml`. Every
crate inherits it via `version.workspace = true`, so a per-crate `version = ` is
a mistake — if you find one, say so rather than editing it.

Bumping the version changes `Cargo.lock` too (the workspace crates are listed
there). Stage both:

```bash
git add Cargo.toml Cargo.lock
```

Commit with exactly this subject, since the release history reads as a series of
them:

```bash
git commit -m "chore: release vX.Y.Z"
```

Per the project's commit convention this still needs the
`Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>` trailer — use
`git commit -F <file>` if a multi-line message is easier to get right than
shell quoting.

Then tag and push both. The tag must match the `Cargo.toml` version exactly, or
the built binaries report a version nobody can find:

```bash
git tag vX.Y.Z
```

```bash
git push origin main
```

```bash
git push origin vX.Y.Z
```

The tag closes the cycle, so this is where any commit-time review records are
retired. The `commit` skill no longer writes them, so this usually finds nothing
and the step is a no-op — say nothing and move on. When the directory *does*
exist (entries predating that change), **rename it, don't delete it** — it is
gitignored, so a wrong delete is unrecoverable, and this matches what
`release-review` does with a stale run directory:

```bash
mv review/commits review/commits-vX.Y.Z
```

The tag is the trigger rather than the review finishing: a finished review can
sit for days while its fixes land, and that is exactly the window where the
per-commit record is still being read. Nothing later in this flow reads the
directory, so if the rename fails, say so and carry on — it is bookkeeping, not
a gate.

## Phase 3 — wait for the Release workflow

The tag push triggers **Release** (`release.yml`), which builds Linux and Windows
binaries and creates the GitHub Release. The branch push separately re-runs
**CI** on the chore commit. Release is the one that matters.

Don't use `gh run watch` here. This run is slow — two OS matrix legs, and the
Linux one installs Zig and `cargo-zigbuild` to link against glibc 2.31 — and a
blocking watch reliably outlives the command timeout, which kills the wait
rather than the run. Poll instead:

```bash
gh run list --workflow Release --limit 1 --json databaseId --jq '.[0].databaseId'
```

```powershell
$id = <id>
do {
  Start-Sleep -Seconds 25
  $r = gh run view $id --json status,conclusion | ConvertFrom-Json
} while ($r.status -ne "completed")
"$($r.status) / $($r.conclusion)"
```

The loop costs nothing and survives a run of any length. A `conclusion` of
anything but `success` is a stop.

If it fails, the tag already exists but the release is incomplete — report the
failure and ask before doing anything drastic. Deleting and re-pushing a tag is
possible but it's the user's call, not yours.

## Phase 4 — write the release notes

The Release workflow creates the release with an empty body. Fill it in from the
commits in the range:

```bash
git log --oneline vPREV..vX.Y.Z
```

Group by **theme, not one bullet per commit** — several commits usually make one
story worth telling, and the reader cares about what changed for them, not about
your commit boundaries. Read the actual diffs for anything you can't summarize
honestly from the subject line.

Format, matching every prior tag (look at `gh release view vPREV --json body` for
the real thing before writing):

- **No title heading.** GitHub already shows the tag as the title. Open with one
  sentence: `Schemaic vX.Y.Z — **theme one**, **theme two**, and a third thing.`
- Then `## ` sections in this order, each **optional if empty**: **Highlights**,
  **Improvements**, **Performance**, **Fixes**.
- Bullets lead with a bold phrase: `* **Feature name.** What changed.`

**No "Under the hood" section.** Every release through v0.10.0 has been rewritten
without one; don't reintroduce it. The audience is someone deciding whether to
download a database client, and internal refactors, module reorganizations and
test counts tell them nothing about whether the app got better. If a piece of
internal work genuinely changed the experience, say it as the experience — "wide
tables stay smooth" rather than "columns are virtualized" — and it belongs in
Improvements or Performance. If it can't be stated that way, it doesn't go in the
notes at all.

Watch for a real feature hiding in there when trimming an old release: v0.5.0's
"Under the hood" held live DB validation, a user-facing setting, which had to move
up rather than be deleted with the section.

**Keep it short — this is the hard part.** The easy failure is writing the commit
message again. A changelog is *scanned*, by someone deciding whether to update or
hunting the thing that broke. The reasoning, the edge cases and the rejected
alternatives already live in the commit; repeating them here buries the release
under its own footnotes.

**Calibrate against the v0.3.0 release** — read it before drafting and match its
scope:

```bash
gh release view v0.3.0 --json body --jq .body
```

- **Highlights: one sentence, two at the most.** Three or four bullets — if
  everything is a highlight, nothing is. Bold the sub-features inline rather than
  spending a sentence on each (`**Go-to-line** (Ctrl+G), a **word-wrap** toggle,
  and an **AI model** picker`) — that packs a lot into a line and stays scannable.
- **Improvements and Fixes: one short sentence each, often just a clause.** A fix
  is *what was broken*, not the diagnosis that found it: "Truncated tab titles no
  longer clip the close icon."
- Where a sentence is spare, spend it on the problem that existed before rather
  than on how the fix works. That's the one piece of "why" worth the space.

If a bullet needs a third sentence to make sense, either it deserves its own
Highlight or the extra sentence isn't needed. Prose paragraphs are the signal
you've drifted — go back to v0.3.0 and cut to its shape.

Write the body to a file and set it with `--notes-file`. Passing multi-line
markdown as a `-m` argument through PowerShell gets mangled (a `>=` in the text
is read as a redirect), and a file sidesteps quoting entirely:

```bash
gh release edit vX.Y.Z --notes-file notes.md
```

Put the file somewhere temporary, not in the repo. Show the user the drafted
notes before setting them if the release is substantial — it's the one part of
this flow that's a judgement call rather than a procedure.

## Command notes (Windows / PowerShell)

Two things mangle commands on this machine, both worth knowing before you spend a
round trip debugging them:

- **`gh --jq` expressions containing `->` or `\(…)` get eaten** — PowerShell reads
  `>` as a redirect. Keep `--jq` to plain field access (`'.[0].databaseId'`) and
  do anything structured by piping the JSON through `ConvertFrom-Json` instead.
- **Multi-line text can't go through `-m`.** A `>=` inside a commit message is
  read as a redirect and the message arrives split into pathspecs. Write the text
  to a file and use `git commit -F` / `gh release edit --notes-file`.

## Finishing

Report: the version, the two workflow runs and their outcomes, and the release
URL. If anything was skipped or went sideways, say so plainly — a release that
half-happened is worth flagging loudly.
