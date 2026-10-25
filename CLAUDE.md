# Schemaic — Claude Code notes

@AGENTS.md

The rules imported above are `AGENTS.md`, which every coding agent in this repository follows and
which stays the authority — a source comment citing "CLAUDE.md" means a rule that now lives there.
What follows is Claude Code's own, kept out of `AGENTS.md` because no other tool has it.

## Delegate the reading and the doc writes (`.claude/agents/`)

Four subagents do the reading and the documentation writes `AGENTS.md` describes, each in its own
window:

- **`scout`** — "where is X wired", "how does feature Y flow across the crates", "what does the
  architecture doc say about Z". Read-only; returns `file:line` citations and a conclusion, never a
  transcript. **Use this instead of reading `docs/architecture/` yourself**; page in a file's range
  by hand only when you are about to edit it, or when the citation isn't enough.
- **`locate`** — a pinpoint symbol lookup when you want only the locations.
- **`arch-scribe`** — makes the architecture-doc edits a finished change requires (the index
  `docs/architecture.md` and the files under `docs/architecture/`), in the
  document's own voice. Give it what changed and why. It checks the brief against the code before
  writing and reports where the two disagree, so read its closing notes rather than treating them
  as a formality. **Route the architecture write through it when a change lands.**
- **`guide-writer`** — the same, for the user guide (`docs/guides/`): give it what a user would
  now see differently, or a chapter to write. It confirms every label and engine claim in the
  code, runs the guide's tests, and lists what it couldn't confirm. **When a change is visible to
  users, route the guide update through it alongside `arch-scribe`.**

Editing a module you are actively designing in still belongs in the main loop.
