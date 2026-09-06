# Schemaic

A fast, native SQL editor for MySQL, MariaDB, PostgreSQL, and SQLite — written in
Rust, with an editable results grid, visual schema editing, and schema-aware
intelligence, built to feel instant.

<p align="center">
  <img src="assets/screenshot.png" alt="Schemaic — SQL editor and results grid" width="820">
</p>

## Why

- **Fast** — GPU-rendered UI ([Floem](https://github.com/lapce/floem)); scrolls
  200k-row result sets smoothly and searches them without lag.
- **Lightweight** — a single native binary. No runtime to install, no embedded
  browser.
- **Native** — real desktop app on Windows, Linux and macOS.
- **Guard-rails** — writes go through a missing-`WHERE` check, grid edits commit
  in a transaction that rolls back unless exactly one row changed, and generated
  `ALTER` / `DROP` is always shown as SQL — with what it destroys named in plain
  language — before it runs.
- **Local** — no account, no telemetry, no cloud service. Schemaic itself has no
  API key and talks to no model provider. Credentials go to the OS keyring —
  never a URL, never a command line — falling back to the config file only on a
  machine with no keyring at all. Turn the assistant on and your prompts go out
  through your own agent CLI, under that CLI's own account and its own terms;
  what *else* that CLI sends is its business and not Schemaic's, and it is not
  nothing — three of the four keep built-in tools that can read the machine
  they run on, and one of those was measured reading files without being asked.
  The AI panel tells you which restrictions are actually in force for the CLI
  you picked, in a sentence, before you send anything.
- **Every engine, properly** — MySQL/MariaDB, PostgreSQL and SQLite are separate
  dialects all the way down: quoting, DDL, completion and diagnostics follow what
  you're actually connected to, not a shared lowest common denominator. Where an
  engine can't do something, the app doesn't offer it rather than failing at it —
  and where it can do it differently, the app does the work: editing a SQLite
  table means the twelve-step rebuild SQLite's own docs prescribe, generated,
  checked and run in one transaction that either lands or rolls back.

## Features

- **SQL editor** — syntax highlighting, schema-aware autocomplete, structure-aware
  diagnostics (unknown tables/columns, syntax errors, typo hints) from a real
  per-dialect parser, one-key formatting, auto-closing pairs, and bracket matching.
- **Snippets and parameters** — a snippet library scoped the way you actually
  work (this connection, this engine, or everywhere), expanded by abbreviation
  from the completion popup, with a per-engine starter pack you can duplicate but
  not break. Write `:name` in a statement and a bar appears to fill it in — the
  values are substituted before the run, so the missing-`WHERE` check reads the
  SQL that will really execute rather than the template.
- **Results grid** — inline editing that writes back to the database
  (transactional, with a per-row safety net); add / duplicate / delete rows;
  server-side filter and sort straight from the column headers; per-column freeze;
  a whole-row JSON view/edit panel; per-column display formatters; paste a block
  from a spreadsheet straight onto the selection (staged as ordinary edits, so
  you still commit or discard it); range selection that totals what you've
  highlighted; and export to CSV / JSON / SQL / Markdown / HTML / Excel —
  streaming the whole table when you ask for it, not just the rows that happened
  to be fetched. Results worth keeping can be pinned to a strip that stays on
  screen while you go on querying.
- **Binary columns** — a blob cell isn't dragged into the result set with
  everything else: open one and its bytes are fetched on their own, shown as an
  image preview (PNG, JPEG, GIF, BMP, WebP, ICO) or a hex dump, and a file can be
  staged back into the cell as bytes, committed like any other edit. A value too
  large to fetch whole is said to be truncated and then refuses to be saved to a
  file, rather than handing you a partial blob that looks complete.
- **Statement timeout** — optional, off by default: cancel a statement that runs
  longer than you meant it to, per statement rather than per script, using the
  same server-side cancellation the Cancel button does.
- **Transactions** — a per-tab manual mode that pins one connection and waits for
  an explicit commit or rollback, with a status pill saying what is open and how
  many statements are in it. MySQL/MariaDB and PostgreSQL; on SQLite the control
  isn't shown, because there is no manual mode there yet.
- **Schema editing** — a visual table designer (columns, indexes, foreign keys,
  CHECK constraints) plus editors for views and triggers, for stored functions
  and procedures on MySQL/MariaDB and PostgreSQL, for PostgreSQL types, domains
  and sequences, and for MySQL's scheduled events. Databases and schemas
  can be created and dropped, and a PostgreSQL materialized view refreshed, from
  the tree's own menus. Every change is shown as the SQL it will run,
  with anything destructive spelled out in plain language, before it runs. Tables,
  views and triggers on all three engines — including SQLite, where a column
  change is a table rebuild and the app generates, verifies and runs the whole
  script for you.
- **Compare schemas** — pair two databases object by object (tables, views,
  triggers, routines, events, enums, domains, sequences), tick the differences you
  want, and get one migration through the same preview and Apply as every other
  change. Same dialect only — a MySQL-to-PostgreSQL migration generated from a
  diff would be wrong, so the app refuses it instead of emitting it. Anything it
  can't express is named in an "omitted" block above the plan rather than quietly
  dropped.
- **Export a database** — structure and data from a whole database, one schema, or
  a single table. As SQL it's one replayable file, with foreign keys restated
  after the rows; in any of the other five formats it's one file per table in a
  folder. The header says what it left out and why — keys pointing outside the
  selection, a dependency cycle, and the identity columns that mean restored rows
  are renumbered rather than carrying their old ids.
- **Users and privileges** — browse a server's accounts and roles, read one
  account's privileges as the actual `GRANT` statements, and create, drop, grant
  or revoke through the ordinary DDL preview. MySQL/MariaDB and PostgreSQL;
  server-owned accounts are shown read-only, and where a privilege could be held
  indirectly — through a role, ownership, or superuser — the pane says the list is
  direct grants only instead of implying it is the whole picture.
- **Server activity** — the connection's live sessions, with a lock-wait banner,
  and *Kill session* / *Cancel query* on any of them. MySQL/MariaDB and
  PostgreSQL. Where the engine can't say what is blocking what, it still shows who
  is waiting and admits it can't name the blocker.
- **Live Monitor** — watch a table and see inserts, updates and deletes as they
  land, down to which column changed; pause, clear or export the change log.
- **Import** — load CSV / TSV / JSON (array or JSON Lines) / Excel `.xlsx` into a
  table, with column mapping and a full validation pass that reports every
  problem, with its line number, before a single row is written.
- **Navigate** — schema browser with favorites, table sizes and a properties panel
  that marks an estimate as an estimate rather than dressing it up as a count (the
  two server engines; SQLite keeps no such statistics), query history, `EXPLAIN`
  query plans, and a global "find anywhere" for schema objects. The ER diagram
  covers a whole database or one table's neighbourhood, finds tables and columns
  with Ctrl+F (Cmd+F on macOS), and exports as an image or as diagram source.
- **`.sql` files** — open a script into a tab and save it back, or run a whole
  file against a database. A script is treated as a write without reading it
  first, so it can't slip past the guard that stands in front of everything else.
- **Connect** — MySQL / MariaDB / PostgreSQL, direct or over SSH tunnels, with
  TLS from *prefer* through *verify-full* (client certificates included, verified
  against the OS trust store rather than a root set compiled in years ago), and
  SQLite by picking a file (no server, so no host, credentials or tunnel to fill
  in). Per-connection colors, environment badges, and a read-only guard-rail on
  all of them. Coming from another client, you can import the servers you already
  have — a pasted URL or DSN, DBeaver, DataGrip, `~/.my.cnf`, `~/.pgpass`,
  `~/.pg_service.conf` — as a proposal you review row by row. Where a source keeps
  its passwords encrypted or in the OS credential store, you're told so rather
  than left with a connection that silently won't open.
- **Terminal** — an embedded shell, and a one-click `mysql` / `mariadb` / `psql` /
  `sqlite3` session against the active connection — through the SSH tunnel when
  there is one, with the password passed by environment rather than on the command
  line, and for SQLite starting in the database file's own directory so `.output`
  and `.read` land where you'd expect.
- **AI assistant** — an agent-CLI session wired into the app rather than bolted
  beside it, driving **your own** installed CLI — Claude Code, Codex,
  Antigravity or OpenCode — picked in Settings → AI with the model id and
  reasoning effort, and never started on a binary Schemaic could not confirm it
  can restrict. Fix a failed query from its error, rewrite the statement under
  the caret and accept or reject the diff (Ctrl+K, Cmd+K on macOS), explain or
  optimize it, ask about an `EXPLAIN` plan, summarize a column or a value, or
  generate realistic rows for a table from the shape of the data already in it.
  A built-in MCP server lets it read your schema and query the database, so
  answers are about your data rather than a generic guess — and it proposes a
  table change as a patch that lands in the same preview any hand edit does,
  never as SQL run behind your back. How much it may see is set **per
  connection**: schema only, on request, or full.
- **Themeable** — dark / light UI themes, multiple editor color schemes, and an
  interface scale (80% / 100% / 130% / 160%) for the app's own text and rows.

### Accessibility

Schemaic is **keyboard-operable but not screen-reader accessible**, and the
second half is not a plan we haven't got to — it is a limit of what the app is
built on. Every modal has a focus ring and a Tab order, every destructive action
**in a modal, in the schema tree or in the results grid** can be reached and
confirmed from the keyboard, the header's **?** opens a reference of every
shortcut, and **Settings → Appearance → Interface scale** enlarges the whole
interface (not just one font) up to 160%. The one place that qualifier is doing
work is the Server Activity panel: its rows have no keyboard cursor and their
context menu has no `Shift+F10` opener, so *Kill session* and *Cancel query* are
reachable by right-click only. The lock-wait banner's own **Kill** button is
keyboard-reachable.
But Floem 0.2 exposes no accessibility tree at all — there is no AccessKit
integration in the toolkit, so there is nothing for Narrator, VoiceOver or Orca
to read, and no amount of markup in this repository can add one. If you need a
screen reader, this is not yet a tool you can use, and we would rather say so
than let you find out after the download.

## Install

Prebuilt binaries for every release are on the
[Releases page](https://github.com/fadion/schemaic/releases/latest). Schemaic
runs on **Windows and Linux (x86_64)** and **macOS (Apple Silicon)**. There is
no Intel Mac build.

### Windows

Download **`Schemaic-win-x64-Setup.exe`** and run it. It installs per-user into
`%LocalAppData%`, so there is no admin prompt, and it updates itself: the app
checks for new releases in the background and offers a **Restart to update**
button in the header when one is staged.

The installer is not code-signed, so SmartScreen shows an "unknown publisher"
warning the first time — *More info* then *Run anyway*. That is a deliberate
choice rather than an oversight; a self-signed certificate chains to no trusted
root and would change nothing.

Prefer no installer? `schemaic-vX.Y.Z-windows-x86_64.zip` is the same build as a
portable folder. It does not auto-update.

### macOS

```sh
curl -fsSL https://raw.githubusercontent.com/fadion/schemaic/main/install.sh | bash
```

Installs the `.pkg` into `/Applications`. The app updates itself from then on.

**Prefer to download it by hand?** Take **`Schemaic-osx-arm64.dmg`** and drag
Schemaic into `/Applications` — the familiar route, and the app still updates
itself afterwards; `Schemaic-osx-arm64-Setup.pkg` is the same app with an
installer in front of it. Either way macOS will refuse to open it the first
time — the app is not signed with an Apple Developer ID, which is a
paid, ongoing thing and not yet warranted. To get past it: open the app once
and let it be blocked, then **System Settings → Privacy & Security**, scroll
to the message naming Schemaic, and click **Open Anyway**.

Right-click → Open, which you will find in older advice, no longer works for
unsigned apps on macOS Sequoia and later. The command-line equivalent is
`xattr -dr com.apple.quarantine /Applications/Schemaic.app`.

The script above avoids all of that, and not by weakening anything: the
quarantine flag is set by whatever downloads the file, and `curl` doesn't set
it.

### Linux

```sh
curl -fsSL https://raw.githubusercontent.com/fadion/schemaic/main/install.sh | bash
```

The script picks the route that fits the system — the **apt repository** on
Debian and Ubuntu, the **dnf/zypper repository** on Fedora, RHEL and openSUSE,
the self-updating AppImage everywhere else — and tells you at the end how that
install updates itself. Override the choice with
`SCHEMAIC_PKG_FAMILY=debian|rpm|appimage`, or set `SCHEMAIC_NO_REPO=1` to take a
single downloaded package and add nothing to your source lists. Read it first if
you would rather not pipe a script into a shell; it is [install.sh](install.sh)
in this repository, and it uses `sudo` only for the package-manager step.

**Everything on this list updates itself now**, by one of two mechanisms: the
AppImage checks GitHub in the background and offers a restart, and a packaged
install is carried forward by the package manager along with the rest of your
system.

#### The package repositories

Signed, hosted at <https://fadion.github.io/schemaic>, and holding the five most
recent releases. To add them by hand — Debian, Ubuntu and derivatives:

```sh
curl -fsSL https://fadion.github.io/schemaic/schemaic-archive-keyring.gpg \
  | sudo tee /usr/share/keyrings/schemaic-archive-keyring.gpg > /dev/null
curl -fsSL https://fadion.github.io/schemaic/schemaic.sources \
  | sudo tee /etc/apt/sources.list.d/schemaic.sources > /dev/null
sudo apt-get update && sudo apt-get install schemaic
```

Fedora, RHEL, CentOS (and openSUSE, with `zypper` in place of `dnf`):

```sh
sudo curl -fsSL https://fadion.github.io/schemaic/schemaic.repo \
  -o /etc/yum.repos.d/schemaic.repo
sudo dnf install schemaic
```

The first `dnf install` reports `repomd.xml GPG signature verification error:
Signing key not found` and then offers to import the key — twice, once for the
repository index and once for the packages. That is what a machine which has
never seen the key is supposed to do, not a failure, and it shows the
fingerprint below each time so you can check before answering. The script above
never shows it, because it imports the key before adding the repository.

Upgrades then arrive with `apt-get upgrade` or `dnf upgrade`. Nothing upgrades
on its own unless you have already set that up — Debian and Ubuntu users can add
`"Schemaic:stable";` to `Unattended-Upgrade::Allowed-Origins` to include
Schemaic in it.

Both repositories are signed, and every `.rpm` in them is signed too. That key
says a package came from this repository and arrived unaltered; it is not a
code-signing certificate and vouches for no identity beyond that. Its
fingerprint is:

```
ABDBDC3958F3FAFC734273796566ECED7795DC1A
```

That is printed here as well as on the site on purpose: a fingerprint you can
only check against the same server the key came from is not a check at all,
and this repository's history is a channel that server does not control.

#### Or by hand, from a release

From the [latest release](https://github.com/fadion/schemaic/releases/latest):

| Artifact | Install | Updates |
| --- | --- | --- |
| `Schemaic-linux-x64.AppImage` | `chmod +x` and run | **Yes**, in-app |
| `schemaic_X.Y.Z_amd64.deb` | `sudo apt-get install ./schemaic_*.deb` | No |
| `schemaic-X.Y.Z-1.x86_64.rpm` | `sudo dnf install --nogpgcheck ./schemaic-*.rpm` | No |
| `schemaic-vX.Y.Z-linux-x86_64.tar.gz` | Extract anywhere | No |

None of these update themselves. A `.deb` or `.rpm` installs to `/usr/bin`,
which the in-app updater correctly refuses to touch — with the repository added
that is the package manager's job, and without it there is nothing behind the
install to update from. The packages *on the Releases page* are unsigned, which
is why the `.rpm` line waives the check; the copies in the repository are
signed.

The binary needs a GPU stack and the usual desktop libraries at runtime
(`libxkbcommon`, Wayland or X11, Vulkan or EGL). The `.deb` and `.rpm` declare
them; the AppImage and the tarball assume a working desktop session.

## Build & run

Requires a recent Rust toolchain (edition 2024). On any platform:

```sh
cargo run -p schemaic-app
```

No database client libraries to install for any of the three engines — SQLite is
compiled in from source, so building needs a working C compiler (the MSVC tools on
Windows, `build-essential` / `gcc` on Linux, the Xcode command line tools on
macOS) alongside the GUI libraries below.

### On Windows

Nothing else to install.

### On macOS

`xcode-select --install`, if you haven't already. Everything the renderer needs
ships with the OS.

### On Linux

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
