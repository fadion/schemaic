# Schemaic — Claude Code notes

@AGENTS.md

The rules imported above are `AGENTS.md`, which every coding agent in this repository follows and
which stays the authority — a source comment citing "CLAUDE.md" means a rule that now lives there.
What follows is Claude Code's own, kept out of `AGENTS.md` because no other tool has it.

## Delegate the reading (`.claude/agents/`)

Three subagents do the reading `AGENTS.md` describes, each in its own window:

- **`scout`** — "where is X wired", "how does feature Y flow across the crates", "what does the
  architecture doc say about Z". Read-only; returns `file:line` citations and a conclusion, never a
  transcript. **Use this instead of reading `docs/architecture.md` yourself**; page in a section by
  hand only when you are about to edit it, or when the citation isn't enough.
- **`locate`** — a pinpoint symbol lookup when you want only the locations.
- **`arch-scribe`** — makes the `docs/architecture.md` edits a finished change requires, in the
  document's own voice. Give it what changed and why. It checks the brief against the code before
  writing and reports where the two disagree, so read its closing notes rather than treating them
  as a formality. **Route the `docs/architecture.md` write through it when a change lands.**

Editing a module you are actively designing in still belongs in the main loop.
