# Back up and restore

**Back up** and **Restore from file** run the database engine's own tools, in the terminal panel,
signed in as your connection. You watch the tool's own output there, and <kbd>Ctrl</kbd>+<kbd>C</kbd>
stops it.

## What you need installed

The tools are looked up on your `PATH`. If one isn't found, the terminal panel says which.

| Engine            | Back up                       | Restore                                  |
|-------------------|-------------------------------|------------------------------------------|
| MySQL and MariaDB | `mariadb-dump` or `mysqldump` | `mariadb` or `mysql`                     |
| PostgreSQL        | `pg_dump`                     | `pg_restore`, or `psql` for a `.sql` file |
| SQL Server        | `sqlcmd`                      | not offered                              |
| SQLite            | nothing — Schemaic copies the file itself | not offered                  |

The MariaDB names are tried first; on many Linux systems `mysqldump` and `mysql` are really
MariaDB's tools under another name.

Your password goes to the tool privately, never on its command line. Over an SSH tunnel the tool
connects through the tunnel. Connections that sign in with an AWS or Google Cloud IAM token can't
be handed to these tools.

## Backing up

Right-click a database and choose **Back up**, then pick where to save. The file name starts as
the database's name and the date and time.

- *MySQL and MariaDB* — a `.sql` script of one consistent snapshot, with routines, triggers and
  events.
- *PostgreSQL* — a custom-format `.dump`, which `pg_restore` can restore from selectively.
- *SQLite* — a copy of the database file, which you can open as a connection of its own.
- *SQL Server* — there's no file dialog. The server writes a `.bak` into its own default backup
  folder, as a copy-only backup that leaves your scheduled backup chain alone. A second backup
  of the same database within the same minute replaces the first. Since this one is written by
  the server, it's greyed out on a read-only connection.

## Restoring

Right-click a database, choose **Restore from file** and pick the file. You're asked to confirm
first — what a restore creates or replaces can't be undone. It's greyed out on a read-only
connection.

On PostgreSQL a `.sql` file runs through `psql`, stopping at the first error; anything else goes
to `pg_restore`, skipping ownership, since the roles a dump names may not exist on this server.

A script is read before anything runs. If it switches to another database part-way — a `USE` or
`\connect` line — the restore is refused, because it would write there instead of into the
database you picked. Restore it into the database it names, or remove those lines from a copy.

Schemaic can't see how a restore ends: check the terminal panel for the tool's result.
