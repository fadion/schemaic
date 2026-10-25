# Engine differences

Schemaic works with four engines, and they don't all do the same things. Where an engine can't do
something, Schemaic leaves the menu entry or field out rather than showing it greyed — so if you
can't find an option, check here first.

MySQL and MariaDB are treated as one engine below; where they differ, the note says so.

## At a glance

| | MySQL / MariaDB | PostgreSQL | SQLite | SQL Server |
|---|---|---|---|---|
| Sign-in | Password, AWS IAM, Google Cloud IAM | Password, AWS IAM, Google Cloud IAM | None — a file on this machine | SQL login, Windows, Microsoft Entra |
| Manual transactions | Yes | Yes | No | Yes, except on a read-only connection |
| Query plans | Yes, with **Analyze** | Yes, with **Analyze** | Yes, without **Analyze** | Yes, with **Analyze** |
| Users and privileges | Yes; accounts have a host | Yes | No | Yes; logins and database users, and `DENY` |
| Create and drop databases | Yes | Yes | No | Yes |
| Schemas inside a database | No | Yes | No | Yes |
| Change a table | In place | In place | By rebuilding it, for most changes | In place; by rebuilding it for a few |
| Move a column | Yes | No | Yes | Yes |
| A failed change undoes its earlier steps | No | Yes | Yes | Yes, mostly |
| Triggers on views | No | Yes | Yes | Yes |
| Events | Yes | — | — | — |
| Sequences | MariaDB: listed, not edited | Yes | — | Yes |
| Types | — | Enum types, domains | — | Alias types, XML schema collections |
| Synonyms | — | — | — | Yes |
| Extensions, materialized views | — | Yes | — | — |
| **Back up** | `mariadb-dump` or `mysqldump` | `pg_dump` | A copy of the file | `BACKUP DATABASE`, written on the server |
| **Restore from file** | Yes | Yes | No | No |
| Dump to a `.sql` file | Yes | Yes | Yes | Yes |
| Compare with | MySQL or MariaDB | PostgreSQL | SQLite | SQL Server |

## Notes

*Sign-in.* **User and password** works everywhere but SQLite. **AWS IAM (AWS CLI)** and
**Google Cloud IAM (gcloud)** are for MySQL and PostgreSQL running on RDS or Cloud SQL; Schemaic
asks the cloud's own CLI for a token each time it connects. On SQL Server, **Windows (this
account)** signs in as you and is offered only in the Windows build, and **Microsoft Entra (Azure
CLI)** uses the Azure CLI's sign-in. See [Connections](../connections/README.md).

*Manual transactions.* SQLite has transactions, but a SQLite tab can't be switched to **Manual**;
it always runs in **Auto-commit**. On SQL Server, a read-only connection stays in **Auto-commit**
too: it has nothing to commit. See
[Query editor](../query-editor/README.md).

*Query plans.* **Analyze** runs the statement to measure it, so it's offered only for a statement
that doesn't write. SQLite has no measuring form, so there you get the plan alone.

*Users.* SQLite has no accounts — whoever can open the file can read it. A MySQL account is a
name *and* a host (`'app'@'%'`). SQL Server keeps server logins and each database's users apart,
and a `DENY` there wins over any grant. See [Users and access](../users/README.md).

*Changing tables.* SQLite's `ALTER TABLE` can do very little, so most changes rebuild the table:
a new table with the new shape, the rows copied across, the old one swapped out. SQL Server rebuilds
to switch identity on or off, to make a column computed or plain, and to move a column. PostgreSQL
can't move a column at all. Every rebuild is in the preview before it runs. See
[Schema](../schema/README.md).

*When a change fails halfway.* On MySQL and MariaDB each `CREATE`, `ALTER` or `DROP` takes effect
the moment it runs, so if the third statement of a change fails, the first two stay done. The other
engines roll the whole change back — except for the few statements SQL Server won't run inside a
transaction, which run one by one.

*Smaller differences.* **On update** for a column, and a table's storage engine, are MySQL's. A
check that isn't enforced is MySQL's but not MariaDB's. Partial indexes are PostgreSQL's, SQLite's
and SQL Server's. These fields appear only where they apply.

*Back up and restore.* On MySQL, MariaDB and PostgreSQL, **Back up** and **Restore from file** run
the engine's own tools in the terminal panel, so those tools have to be installed. SQLite's copy is
made inside Schemaic; to restore it, open the copy as a connection. SQL Server's backup is written
by the server, into its own folder, so it's greyed out on a read-only connection, and restoring one
is done on the server with SQL Server's own tools. See
[Import and dump](../import-export/README.md).

*Compare.* Both sides must be the same engine; MySQL and MariaDB count as one. On MySQL and
MariaDB, a trigger, routine or event that differs is shown but can't be ticked for the migration.
See [Schema](../schema/README.md).
