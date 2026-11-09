# Engine differences

Schemaic works with five engines, and they don't all do the same things. Where an engine can't do
something, Schemaic leaves the menu entry or field out rather than showing it greyed — so if you
can't find an option, check here first.

MySQL and MariaDB are treated as one engine below; where they differ, the note says so. DuckDB,
the newest, has [a section of its own](#duckdb) at the end.

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

## DuckDB

A DuckDB connection is a database file on this machine, like SQLite's; see
[DuckDB files](../connections/README.md#duckdb-files) for what that means in practice.

*What works:*

- running statements and scripts, including `.sql` files;
- the schema tree — schemas, tables and views with their columns, keys and indexes, sequences, and
  macros — and each object's DDL, which is DuckDB's own stored `CREATE` statement. A macro's is
  rebuilt from the catalogue, every overload included. DuckDB doesn't keep a parameter's default
  there, so Schemaic reads defaults such as `b := 2` back from DuckDB's own export of the schema,
  and a dump writes the macro the same way;
- editing results in the grid, for a result read from one table with a primary key — see
  [Editing data](../results-grid/editing.md#what-can-be-edited);
- **Import** into a table, from CSV, JSON or Excel, in one transaction — see
  [Importing a file into a table](../import-export/import-file.md);
- exporting results, and query plans, with **Analyze**;
- **Server variables**, which lists DuckDB's settings;
- **Back up**, which writes a new `.duckdb` file;
- a SQL dump under **Export** — types made with `CREATE TYPE`, tables with their indexes and
  comments, views and macros with theirs, and sequences (which carry on from the source's
  counter). A type is written only for the schemas the file exports tables from;
- **Manual** transactions, as on PostgreSQL: a failed statement aborts the transaction and only
  **Rollback** ends it. A read-only connection gets read-only transactions. The tab keeps the file
  open, and other programs out of it, until it goes back to **Auto-commit**; and because DuckDB
  has no savepoints, grid edits that fail to save roll back the whole transaction — see
  [Editing data](../results-grid/editing.md#committing);
- the `duckdb` command-line client in the terminal panel, when it's installed;
- the [table designer](../schema/table-designer.md#what-differs-between-engines), **Copy to** and
  **Compare with**. DuckDB's `ALTER TABLE` refuses most changes to a table that anything depends
  on, so many changes rebuild the table in one transaction, and a table another table's foreign key
  points at can't be rebuilt. **Compare with** compares one DuckDB file with another, and runs its
  migration in one transaction; see [Comparing schemas](../schema/compare.md#limits) for what it
  leaves out;
- new schemas from **Create** › **Schema**, and **Drop** on a schema's row, which DuckDB refuses
  while the schema still holds anything;
- new sequences from **Create** › **Sequence**. DuckDB can't change a sequence once it's made,
  so editing one changes only its comment — drop it and create it again to change the rest;
- the view editor: **Edit view**, a new view from **Create** › **View** or the query editor's
  **Create view**, renaming a view in its name field, and **Drop**. An edit keeps the view's
  **Column names** and its comment, which Schemaic sets again after the replace, since DuckDB's
  would clear it. DuckDB doesn't track which views read a view, so a change that breaks another
  view goes through, and that view fails the next time it's read;
- the macro editor, from the edit entry on a macro's right-click menu, and a new macro from
  **Create** › **Function**. There's no **Procedure**, since DuckDB has none. Leave **Returns**
  empty for a scalar macro, or write `TABLE` for a table macro, whose **Body** is a query.
  **Parameters** are names, optionally typed, with defaults where wanted (`a INTEGER, b := 0`),
  and further overloads go in **Body** the way one `CREATE MACRO` declares them:
  `a + 1, (a, b) AS a + b`. Renaming a macro, or switching it between scalar and table, drops and
  re-creates it in one transaction. The
  **Comment** is kept across every edit, and **Drop** works on both kinds.

There's no **Restore from file**: to restore a backup, open the copy as a connection, as on SQLite.

There are no users to manage, as on SQLite. The AI assistant and the command line read only the
database's own tables: DuckDB's readers for files on your computer, such as `read_csv`, are
refused there. See [Data access](../ai/data-access.md).
