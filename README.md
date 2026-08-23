# Schemaic

A fast, native SQL editor for MySQL, MariaDB, PostgreSQL, and SQLite — written in
Rust, with an editable results grid, visual schema editing, and schema-aware
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
- **Native** — real desktop app on Windows, Linux and macOS.
- **Guard-rails** — writes go through a missing-`WHERE` check, grid edits commit
  in a transaction that rolls back unless exactly one row changed, and generated
  `ALTER` / `DROP` is always shown as SQL — with what it destroys named in plain
  language — before it runs.
- **Local** — no account, no telemetry, no cloud service. The only things that
  leave your machine are your database traffic and, if you turn the assistant on,
  the prompts you send through your own `claude` CLI. Credentials go to the OS
  keyring — never a URL, never a command line — falling back to the config file
  only on a machine with no keyring at all.
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
- **Results grid** — inline editing that writes back to the database
  (transactional, with a per-row safety net); add / duplicate / delete rows;
  server-side filter and sort straight from the column headers; per-column freeze;
  a whole-row JSON view/edit panel; per-column display formatters; and export to
  CSV / JSON / SQL / Markdown / HTML.
- **Statement timeout** — optional, off by default: cancel a statement that runs
  longer than you meant it to, per statement rather than per script, using the
  same server-side cancellation the Cancel button does.
- **Transactions** — a per-tab manual mode that pins one connection and waits for
  an explicit commit or rollback, with a status pill saying what is open and how
  many statements are in it. MySQL/MariaDB and PostgreSQL; on SQLite the control
  isn't shown, because there is no manual mode there yet.
- **Schema editing** — a visual table designer (columns, indexes, foreign keys,
  CHECK constraints) plus editors for views and triggers, for stored functions
  and procedures on MySQL/MariaDB and PostgreSQL, and for PostgreSQL types,
  domains and sequences. Every change is shown as the SQL it will run,
  with anything destructive spelled out in plain language, before it runs. Tables,
  views and triggers on all three engines — including SQLite, where a column
  change is a table rebuild and the app generates, verifies and runs the whole
  script for you.
- **Live Monitor** — watch a table and see inserts, updates and deletes as they
  land, down to which column changed.
- **Import** — load CSV / TSV / JSON (array or JSON Lines) into a table, with
  column mapping and a full validation pass that reports every problem, with its
  line number, before a single row is written.
- **Navigate** — schema browser with favorites, query history, `EXPLAIN` query
  plans, an ER diagram of a whole database or one table's neighbourhood, and a
  global "find anywhere" for schema objects.
- **Connect** — MySQL / MariaDB / PostgreSQL, direct or over SSH tunnels, and
  SQLite by picking a file (no server, so no host, credentials or tunnel to fill
  in). Per-connection colors, environment badges, and a read-only guard-rail on
  all of them.
- **Terminal** — an embedded shell, and a one-click `mysql` / `mariadb` / `psql` /
  `sqlite3` session against the active connection — through the SSH tunnel when
  there is one, with the password passed by environment rather than on the command
  line, and for SQLite starting in the database file's own directory so `.output`
  and `.read` land where you'd expect.
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

### Accessibility

Schemaic is **keyboard-operable but not screen-reader accessible**, and the
second half is not a plan we haven't got to — it is a limit of what the app is
built on. Every modal has a focus ring and a Tab order, every destructive action
can be reached and confirmed from the keyboard, and the header's **?** opens a
reference of every shortcut.
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

The script picks the artifact that fits the system — a `.deb` on Debian and
Ubuntu, an `.rpm` on Fedora, RHEL and openSUSE, the AppImage everywhere
else — and tells you at the end how that build updates itself. Override the choice
with `SCHEMAIC_PKG_FAMILY=debian|rpm|appimage`. Read it first if you would
rather not pipe a script into a shell; it is
[install.sh](install.sh) in this repository, and it uses `sudo` only for the
package-manager step.

To do it by hand instead, from the
[latest release](https://github.com/fadion/schemaic/releases/latest):

| Artifact | Install | Updates |
| --- | --- | --- |
| `Schemaic-linux-x64.AppImage` | `chmod +x` and run | **Yes**, in-app |
| `schemaic_X.Y.Z_amd64.deb` | `sudo apt-get install ./schemaic_*.deb` | No |
| `schemaic-X.Y.Z-1.x86_64.rpm` | `sudo dnf install --nogpgcheck ./schemaic-*.rpm` | No |
| `schemaic-vX.Y.Z-linux-x86_64.tar.gz` | Extract anywhere | No |

The AppImage is the only Linux artifact that updates itself. A `.deb` or `.rpm`
installs to `/usr/bin`, which the updater correctly refuses to touch, so those
are updated by re-running the script above. The packages are not GPG-signed,
which is why the `.rpm` line above waives the check.

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
