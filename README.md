# Schemaic

A fast, native SQL editor for MySQL, MariaDB, and PostgreSQL — written in Rust,
with an editable results grid, visual schema editing, and schema-aware
intelligence, built to feel instant.

<p align="center">
  <img src="assets/screenshot.png" alt="Schemaic — SQL editor and results grid" width="820">
</p>

## Notice

Schemaic is in active development. It should **not** be used or trusted with
production data, or any data you care about.

## Why

- **Fast** — GPU-rendered UI ([Floem](https://github.com/lapce/floem)); scrolls
  200k-row result sets smoothly and searches them without lag.
- **Lightweight** — a single native binary. No runtime to install, no embedded
  browser.
- **Native** — real desktop app on Windows and Linux.
- **Guard-rails** — writes go through a missing-`WHERE` check, grid edits commit
  in a transaction that rolls back unless exactly one row changed, and generated
  `ALTER` / `DROP` is always shown as SQL — with what it destroys named in plain
  language — before it runs.
- **Local** — no account, no telemetry, no cloud service. The only things that
  leave your machine are your database traffic and, if you turn the assistant on,
  the prompts you send through your own `claude` CLI. Credentials go to the OS
  keyring — never a URL, never a command line — falling back to the config file
  only on a machine with no keyring at all.
- **Both engines, properly** — MySQL/MariaDB and PostgreSQL are separate dialects
  all the way down: quoting, DDL, completion and diagnostics follow the server
  you're actually connected to, not a shared lowest common denominator.

## Features

- **SQL editor** — syntax highlighting, schema-aware autocomplete, structure-aware
  diagnostics (unknown tables/columns, syntax errors, typo hints) from a real
  per-dialect parser, one-key formatting, auto-closing pairs, and bracket matching.
- **Results grid** — inline editing that writes back to the database
  (transactional, with a per-row safety net); add / duplicate / delete rows;
  server-side filter and sort straight from the column headers; per-column freeze;
  a whole-row JSON view/edit panel; per-column display formatters; and export to
  CSV / JSON / SQL / Markdown / HTML.
- **Transactions** — a per-tab manual mode that pins one connection and waits for
  an explicit commit or rollback, with a status pill saying what is open and how
  many statements are in it.
- **Schema editing** — a visual table designer (columns, indexes, foreign keys,
  CHECK constraints) plus editors for views, triggers, and PostgreSQL functions,
  types, domains and sequences. Every change is shown as the SQL it will run,
  with anything destructive spelled out in plain language, before it runs.
- **Live Monitor** — watch a table and see inserts, updates and deletes as they
  land, down to which column changed.
- **Import** — load CSV / TSV / JSON (array or JSON Lines) into a table, with
  column mapping and a full validation pass that reports every problem, with its
  line number, before a single row is written.
- **Navigate** — schema browser with favorites, query history, `EXPLAIN` query
  plans, an ER diagram of a whole database or one table's neighbourhood, and a
  global "find anywhere" for schema objects.
- **Connect** — MySQL / MariaDB / PostgreSQL, direct or over SSH tunnels, with
  per-connection colors, environment badges, and a read-only guard-rail.
- **Terminal** — an embedded shell, and a one-click `mysql` / `mariadb` / `psql`
  session against the active connection — through the SSH tunnel when there is
  one, and with the password passed by environment rather than on the command
  line.
- **AI assistant** — a `claude` CLI session wired into the app rather than bolted
  beside it: **AI Fix** on a failed query, which hands it the error and the query
  and offers you the corrected SQL as a diff; rewrite the statement
  under the caret and accept or reject that diff yourself (Ctrl+K); explain or
  optimize it from the right-click menu; ask about an `EXPLAIN` plan without
  retyping it; summarize a column or a single value; or generate realistic rows
  for a table from the shape of the data already in it. A built-in MCP server
  lets it read your schema and query the database, so answers are about your data
  rather than a generic guess.
- **Themeable** — dark / light UI themes and multiple editor color schemes.

## Build & run

Requires a recent Rust toolchain (edition 2024). On any platform:

```sh
cargo run -p schemaic-app
```

### Windows

Nothing else to install.

### Linux

The renderer needs a few GUI system libraries first — the package names differ by
distribution, the set doesn't.

Debian / Ubuntu:

```sh
sudo apt-get install -y libxkbcommon-dev libwayland-dev libxcb1-dev libx11-dev pkg-config
```

Fedora / RHEL:

```sh
sudo dnf install libxkbcommon-devel wayland-devel libxcb-devel libX11-devel pkgconf-pkg-config
```

Arch:

```sh
sudo pacman -S --needed libxkbcommon wayland libxcb libx11 pkgconf
```

## License

MIT — see [LICENSE](LICENSE). Third-party notices are in
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).
