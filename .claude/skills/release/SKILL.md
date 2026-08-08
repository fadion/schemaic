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

Then confirm the tree is green locally, because CI checks exactly these three and
an unformatted tree is historically the most common CI failure:

```bash
cargo fmt --all --check
```

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

```bash
cargo test --workspace
```

If `fmt --check` fails, run `cargo fmt --all`, commit the result as its own
`style:` or `chore:` commit, and carry on. Failing clippy or tests is a stop —
that's real breakage, not a formatting nit.

Also read the current version so you can sanity-check the requested one:

```bash
grep -n '^version' Cargo.toml
```

The new version must be greater than the current one, and the tag must not
already exist (`git tag --list vX.Y.Z`). If either is off, stop and ask.

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
even when the run failed. If CI is red, stop: report which job failed and paste
the failing step's output (`gh run view <id> --log-failed`). Do not proceed to
the tag.

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

## Phase 3 — wait for the Release workflow

The tag push triggers **Release** (`release.yml`), which builds Linux and Windows
binaries and creates the GitHub Release. The branch push separately re-runs
**CI** on the chore commit. Release is the one that matters — watch it:

```bash
gh run list --workflow Release --limit 1 --json databaseId,headBranch --jq '.[0].databaseId'
```

```bash
gh run watch <id> --exit-status
```

It takes several minutes (two OS matrix legs, and the Linux leg installs Zig and
`cargo-zigbuild` to link against glibc 2.31). If it fails, the tag already
exists but the release is incomplete — report the failure and ask before doing
anything drastic. Deleting and re-pushing a tag is possible but it's the user's
call, not yours.

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
  **Improvements**, **Performance**, **Fixes**, **Under the hood**.
- Bullets lead with a bold phrase, then one to three sentences:
  `* **Feature name.** What it does and why it matters.`
- Write the *why*, not just the what. The good entries in past releases explain
  the problem that existed before — that's what makes a changelog readable
  rather than a diff summary.
- "Under the hood" is where architecture/testing notes go, including the current
  test count if it's worth mentioning.

Write the body to a file and set it with `--notes-file`. Passing multi-line
markdown as a `-m` argument through PowerShell gets mangled (a `>=` in the text
is read as a redirect), and a file sidesteps quoting entirely:

```bash
gh release edit vX.Y.Z --notes-file notes.md
```

Put the file somewhere temporary, not in the repo. Show the user the drafted
notes before setting them if the release is substantial — it's the one part of
this flow that's a judgement call rather than a procedure.

## Finishing

Report: the version, the two workflow runs and their outcomes, and the release
URL. If anything was skipped or went sideways, say so plainly — a release that
half-happened is worth flagging loudly.
