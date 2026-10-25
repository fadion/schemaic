# Getting started

This page takes you from nothing to a first query: install Schemaic, connect it to a database, and
run something.

## Installing

Every release is on the [Releases page](https://github.com/fadion/schemaic/releases/latest), built
for Windows and Linux on x86_64 and for macOS on Apple Silicon.

- *Windows* — download and run `Schemaic-win-x64-Setup.exe`. It installs for your user only, so it
  needs no admin rights, and it keeps itself up to date. The installer isn't code-signed, so
  SmartScreen warns you the first time: click *More info*, then *Run anyway*. If you want a copy
  that doesn't install or update, use the portable `.zip` instead.
- *macOS and Linux* — run the install script:

  ```sh
  curl -fsSL https://raw.githubusercontent.com/fadion/schemaic/main/install.sh | bash
  ```

  On macOS it puts Schemaic in `/Applications`, and the app updates itself from then on. On
  Debian and Ubuntu it adds Schemaic's signed apt repository, and on Fedora, RHEL and openSUSE its
  dnf/zypper repository, so Schemaic updates along with the rest of your system. On any other
  distribution it installs the self-updating AppImage. The script asks for `sudo`, and prints how
  to uninstall when it's done.

If you'd rather install by hand — the `.dmg` on macOS, or adding the repository from
`https://fadion.github.io/schemaic` yourself — the project's
[README](https://github.com/fadion/schemaic#install) has the steps, including the fingerprint to
check the signing key against.

The self-updating builds check for a new release in the background. When one has downloaded, a
*Restart to update* button appears in the header. Set the environment variable
`SCHEMAIC_NO_UPDATE_CHECK=1` if you don't want Schemaic to check.

## Your first connection

On first launch there's nothing to connect to yet, so the header shows **New connection** where
the connection switcher will go. Click it.

1. Give the connection a **Name**, and pick its **Type**: MySQL (also for MariaDB), PostgreSQL,
   SQLite or SQL Server.
2. For a server, fill in **Host**, **Port**, **User** and **Password**. **Database** is optional.
   For SQLite, pick the **Database file** instead.
3. Press **Test** to check that Schemaic can reach the server, then **Save**.

Saving connects. The **SCHEMA** panel on the left fills with the server's databases, and the
connection's name takes the place of **New connection** in the header.

The password goes to your operating system's keyring, not into a file. Everything else on the
form — SSH tunnels, SSL / TLS, read-only mode and the rest — is covered in
[Connections](../connections/README.md).

## Your first query

Press <kbd>Ctrl</kbd>+<kbd>T</kbd> for a new query tab, type a statement, and press
<kbd>Ctrl</kbd>+<kbd>Enter</kbd>. Schemaic runs the statement under the caret, and the rows appear
in the grid below the editor. On macOS, use <kbd>Cmd</kbd> wherever this guide says
<kbd>Ctrl</kbd>.

To see a table's rows without writing anything, double-click the table in the schema tree.

## Where to go next

- [Connections](../connections/README.md) — more connections, tunnels, sign-in and safeguards.
- [Query editor](../query-editor/README.md) — completion, running several statements, transactions.
- [Results grid](../results-grid/README.md) — editing rows, filtering, exporting.
- [Schema](../schema/README.md) — finding your way round a database and changing its structure.
- [Keyboard shortcuts](../reference/shortcuts.md) — or press the question-mark button in the header.
