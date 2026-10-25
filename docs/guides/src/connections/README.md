# Connections

A connection is one saved server — or, for SQLite, one database file. You add and change them in
**Manage connections**, and switch between them from the header.

## Managing connections

Open the connection switcher in the header and choose **Manage connections** at the bottom of its
menu. Your saved connections are listed on the left, and the selected one's settings on the right.

- **New connection** adds one and selects it.
- **Test** tries the settings as they stand, without saving them. If it fails, hover over the
  message to read the whole reason.
- **Save** stores the changes and connects.
- Right-click a connection in the list for **Duplicate** and **Delete**. Deleting removes the
  connection's stored password, its AI conversation, its query history and its tabs along with it.

**Import connections** and **Export connections** sit next to **New connection**; see
[Importing connections](import.md).

## The form

The form has four sections.

| Section        | What it holds                                                                      |
|----------------|------------------------------------------------------------------------------------|
| **General**    | **Name**, **Colour**, and an optional **Folder** to group connections under         |
| **Safeguards** | Read-only mode and the other limits on what can happen through the connection       |
| **Database**   | **Environment**, **Type**, where the server is and how to sign in, and **Init SQL** |
| **Network**    | **SSH tunnel** and **SSL / TLS**                                                    |

The **Database** section changes with **Type**:

- *MySQL, PostgreSQL, SQL Server* — **Host**, **Port**, **Database**, then **User** and
  **Password**. Changing the type changes the port to the new engine's default (3306, 5432, 1433)
  unless you've typed your own. **Database** is optional; it's the one the connection opens in,
  and some hosted providers refuse a connection that doesn't name it. For MariaDB, choose MySQL.
- *SQL Server named instances* — write the host as `server\instance`. SQL Server Browser then
  supplies the port, so **Port** is ignored.
- *SQLite* — just a **Database file**. There's no server, so no host, password, tunnel or TLS, and
  the **Network** section goes away. SQLite statements commit as they're sent: there's no manual
  transaction mode.

Where an engine offers more than a password, an **Authentication** picker appears above **User**.
See [Signing in and TLS](sign-in.md).

## Switching connections

Click the connection's name in the header to open the switcher, and pick another. Each connection
keeps its own tabs, query history and AI conversation, so switching back puts you where you were.
From the keyboard, open the command palette (<kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>P</kbd>) and use
**Switch connection**.

Connections with a **Folder** are listed under it, in the switcher and in **Manage connections**
alike. Hover over a connection whose name or address is cut short to see it in full.

If a connection drops, the header says **Disconnected**, with a **Retry** button.

## Telling connections apart

Three settings help you see at a glance which server you're on — useful when staging and
production look the same.

- **Colour** marks the connection in the switcher and on the switcher's border.
- **Prominent colour in editor** also frames the query editor in that colour.
- **Environment** — **Local**, **Development**, **Testing**, **Staging** or **Production** — adds a
  badge with that name to the header.

You can also colour individual databases in the schema tree, which tints their query tabs; see
[Schema](../schema/README.md).

## Where passwords are kept

Passwords, SSH passwords and SSH key passphrases go to your operating system's keyring — Credential
Manager on Windows, the Keychain on macOS, the Secret Service (GNOME Keyring, KWallet) on Linux.
Everything else is saved in `connections.json`, in `%APPDATA%\schemaic` on Windows and
`~/.config/schemaic` elsewhere.

If no keyring is available, such as on a Linux machine with no Secret Service running, Schemaic
keeps working by leaving the password in `connections.json` in plain text. Schemaic needs no
password for Windows, Microsoft Entra or cloud IAM sign-in, and stores none.

## In this section

- [Signing in and TLS](sign-in.md) — Windows, Microsoft Entra and cloud IAM sign-in, and
  encrypting the connection.
- [SSH tunnels](ssh.md) — reach a server through an SSH host, with jump hosts and `~/.ssh/config`.
- [Safeguards](safeguards.md) — read-only mode, command-line and AI access, timeouts and init SQL.
- [Importing connections](import.md) — bring connections over from other tools, and share your own.
