# `schemaic-cli`

`schemaic-cli` — Schemaic without a window: `schemaic list` / `databases` / `ping` / `tables` /
`describe` / `query` / `explain` / `export` / `dump` / `compare` / `exec` / `mcp` / `version` / `help`, so a person or an agent can run SQL against a saved connection with the app closed and without being
handed a credential. It also holds the **MCP server** (`mcp.rs`), which has two ways in — the
app's `--mcp-serve` for the AI panel, and `schemaic mcp -c <conn>` for any MCP client. Its whole
dependency list is `schemaic-core`, `schemaic-conn`, `schemaic-db`, `clap`, `serde_json`, `tokio`
(with `io-util` and `io-std`, for that server's stdin/stdout loop) and `tokio-util` — **no
floem**, which is what splitting `schemaic-conn` out of `schemaic-app` was for. It is a **library** with two front ends, because the front ends differ per
platform. On Linux and macOS the `schemaic` binary takes an argv branch into `run::main` before it
builds a window, the shape `--mcp-serve` already had, so the CLI *is* the app's own executable —
deliberately so on macOS, where the keychain binds an item's ACL to the creating application and a
second binary would prompt for every password the GUI had stored. On Windows the app is
`windows_subsystem = "windows"` and therefore has no console to print to, so a second,
console-subsystem binary is built from this crate and shipped beside it as `schemaic.com`:
`PATHEXT` is `.COM;.EXE;…`, so a bare `schemaic` at a prompt resolves to the CLI while shortcuts,
the Start menu and Explorer keep launching the GUI. It is an ordinary PE — nothing about the DOS
`.com` format is involved — and the arrangement was checked on Windows 11 with the real binaries.
**A bare `schemaic` resolves only once its directory is on `PATH`, though, and of the installs
only the `.deb`/`.rpm` (in `/usr/bin`) puts it there** — so on Windows and macOS the CLI shipped
reachable only by its full path. Settings → General → Command line → **Install** closes that, on a
click and never at install time: `core::cli_install` decides (the user `PATH` on Windows, a
`~/.local/bin` link elsewhere) and `app/install_cli.rs` writes. **Remove** beside it undoes that,
and on Windows so does the Velopack uninstall hook. On a `.deb`/`.rpm` the row says where the
command already is and greys Install out, rather than offering a link it would not make.
The command surface was verified end to end against a live MariaDB; `databases` and the
no-database hint against MySQL 8.4 and PostgreSQL 16.
