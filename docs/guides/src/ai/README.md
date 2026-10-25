# AI assistant

The AI assistant answers questions about your database in a panel beside the editor. Schemaic
doesn't ship a model of its own: it drives an agent CLI you already have installed and signed in
to, and hands it a small set of database tools. Show or hide the panel with
<kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>A</kbd>.

The assistant is off on a new install. While it's off, nothing is sent anywhere, no agent process
starts, and no AI entry appears in any menu.

## Setting it up

Open the **AI assistant** section of Settings, or press the gear (**AI settings**) at the top of
the panel.

| Setting                 | What it does                                                                        |
|-------------------------|-------------------------------------------------------------------------------------|
| **Agent CLI**           | Which CLI to drive: Claude Code, Codex, Antigravity, OpenCode, GitHub Copilot or Cursor. **Disabled** turns the assistant off. |
| *CLI path*              | Where the CLI lives. Leave it empty and Schemaic finds it on its own, and shows the path it found. |
| **Model**               | Any model id the CLI accepts. Empty uses the CLI's own default. The buttons under it are shortcuts, not the whole list. |
| **Effort**              | How hard the model thinks. Only shown for CLIs that take it — Claude Code, Antigravity and OpenCode — with that CLI's own levels. |
| **Custom instructions** | Your house rules, sent with every question: naming conventions, the dialect quirks you care about. |
| **Schema context**      | How much of the database's structure goes with every message — see below.          |

You sign in to the CLI itself, the way you normally would. Schemaic stores no API key.

Under **Agent CLI** a note says how tightly that CLI can be restricted. Claude Code, OpenCode and
GitHub Copilot can be kept to Schemaic's tools alone. Codex and Antigravity run in a sandbox: their
own tools can read files on your machine but not write them or run commands. Cursor is held back
by a permissions file instead of a sandbox. Antigravity and Cursor also bring along any MCP
servers you've registered with them yourself, which Schemaic can't switch off.

### Schema context

- **Active database only** (the default) sends an outline of the database you're working in:
  tables, columns, keys.
- **All databases** sends the outline of every database on the connection. On a big server this
  can make each message large.
- **None** sends no outline, and also takes away the schema tools, so the assistant has to ask you
  for table and column names.

Databases you've hidden in the schema tree are left out of the outline.

How much *data* the assistant may see is a separate choice, made per connection. That's
[Data access](data-access.md).

## Asking questions

Type in the panel and press <kbd>Enter</kbd>. Along with your question, the assistant gets:

- the schema outline your **Schema context** allows,
- the SQL in the active editor tab — or only the selected part, if you've selected something,
- the *shape* of that tab's last result: column names and types, row count, and the error if the
  run failed. Never the rows themselves.

The conversation is kept per connection and comes back when you reopen Schemaic. **New chat**
starts over.

## What it can do

On its own, the assistant can look things up with Schemaic's tools:

- *List and search* — the databases, their tables and columns, and objects by part of a name.
- *Describe a table* — its full definition, indexes and foreign keys.
- *Run a query* — one read-only statement, if the connection's data access allows it. Results are
  capped at 200 rows, and long values are cut short.
- *Explain a query* — the plan, without running it, under the same data access rule.
- *Check a table change* — it can work out the SQL for a change it wants to suggest, without
  running anything.

None of these tools can change the database. How that's enforced is under
[Data access](data-access.md#how-queries-stay-read-only).

## Acting on an answer

SQL in a reply comes with buttons:

- **Copy** puts it on the clipboard.
- **Insert** opens it in a new editor tab.
- **Run** opens it in a new tab and runs it there, so what ran stays on screen. It goes through
  the same checks as anything you run yourself: a write asks first, and a read-only connection
  refuses it.

When the assistant suggests changing a table, the reply shows a card instead of a script. Press
**Review change…** to open the change preview, where you see the exact SQL and apply it yourself —
the same preview the [table designer](../schema/README.md) uses.

## Elsewhere in the app

The assistant isn't only the panel. These all use the same CLI and the same data access rules:

- **AI explain** at the end of every schema tree menu describes the object in the panel.
- In the editor, <kbd>Ctrl</kbd>+<kbd>K</kbd> asks for SQL right where the caret is, and the
  right-click menu has **Ask AI**, **Explain** and **Optimise**.
- When a query fails, the error has **AI fix** (a corrected statement, shown as a change you
  approve) and **Explain** (what went wrong, in the panel).
- In the results grid: **AI summary** of a selection, **Attach 3 rows to chat** (the count
  follows your selection), and **AI fill value**, **AI insert row** and **AI seed table** for
  generating data.

## Using Schemaic from other agents

The same tools are available to any MCP client — another agent, an editor, a desktop chat app —
through `schemaic mcp`. It serves one saved connection over stdin and stdout:

```sh
schemaic mcp -c "Shop (prod)"
```

The connection needs **CLI access** turned on in its settings, and its **AI data access** level
applies here too, with one difference: there's no grid to attach rows from, so rows are sent only
at **Let it read data**. The tool that proposes table changes is left out, since there's no
preview on the other end to review one. Setup for particular clients is in
[Command line](../cli/README.md).

## In this section

- [Data access](data-access.md) — what each level lets leave your machine, and how queries are
  kept read-only.
