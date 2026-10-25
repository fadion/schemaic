# Import and dump

Getting data into a database and out of it again. Everything here starts with a right-click in
the [schema tree](../schema/README.md), on a database or on a table.

## Which one you want

| You want to…                                    | Right-click   | Choose                     |
|-------------------------------------------------|---------------|----------------------------|
| Load a CSV, JSON or Excel file into a table     | the table     | **Import**                 |
| Run a `.sql` file against a database            | the database  | **Import**                 |
| Write tables to a `.sql` file, or to CSV, JSON… | database or table | **Export**, then a format |
| Take a backup with the engine's own tool        | the database  | **Back up**                |
| Load such a backup back in                      | the database  | **Restore from file**      |
| Copy a table into another database              | the table     | **Copy to**                |

**Import** means two things depending on where you click: on a table it loads a data file into
that table; on a database it runs a SQL script, whose statements say for themselves which tables
they touch. **Export** on a table opens the same window as on its database, with just that
table ticked.

To save the rows of a query result rather than whole tables, use the results grid's own export —
see [Results grid](../results-grid/README.md).

## Export or back up?

Both give you a file you can load again later. The difference is who writes it.

- **Export** is written by Schemaic. It works the same on every engine, lets you pick tables and
  choose what goes in, and the `.sql` it writes can be loaded back with **Import** on a database.
- **Back up** runs the engine's own tool — `mysqldump`, `pg_dump` and so on — in the terminal
  panel. Use it when you want the file those tools make, or when you'll restore it with them.

## Read-only connections

Anything that only reads the server and writes a file on your machine is allowed on a read-only
connection: **Export**, **Back up** (except on SQL Server, where the server itself writes the
backup) and **Copy to** another connection. **Import** and **Restore from file** are greyed out.

## Copying a table to another database

Right-click a table and choose **Copy to**. It copies the table's structure, and its rows unless
you say otherwise, into another database — on this connection or another one of the same engine.

1. Pick the connection, then the database. The chosen one shows on the **Copy into** button;
   press it to choose again.
2. Set **Schema** (where the engine has them) and **Name** — the name starts as the table's with
   `_copy` on the end.
3. Leave **Copy data** on to copy the rows; turn it off for the structure alone.
4. Press **Preview SQL**. You see the `CREATE TABLE` first, as with any change Schemaic makes;
   **Apply** creates the table, then the rows are copied and counted as they go. **Stop** halts
   the copy, leaving the table created.

What doesn't come across: foreign keys (the tables they point at may not exist there) and
triggers. A generated column's values are computed afresh, and a partitioned table arrives as
one plain table. MySQL and MariaDB count as the same engine here, so you can copy between them,
but nothing translates a type or option one has and the other lacks.

## What differs between engines

- *Back up* writes a `.sql` script on MySQL and MariaDB, a custom-format `.dump` on PostgreSQL,
  and a copy of the database file on SQLite. On SQL Server the server writes a `.bak` into its
  own backup folder.
- *Restore from file* is offered on MySQL, MariaDB and PostgreSQL only.
- *Disable foreign-key checks* in a SQL export exists on MySQL, MariaDB and SQLite; elsewhere the
  file adds the foreign keys after the data instead.
- *Import into a table* works on all four engines. On a MySQL table whose storage engine isn't
  transactional (MyISAM, for one), a failed import can't be undone.

## In this section

- [Importing a file into a table](import-file.md) — CSV, TSV, JSON or Excel, with a column mapping.
- [Running a SQL script](run-script.md) — load a `.sql` file into a database.
- [Exporting](export.md) — a database or some of its tables, as one `.sql` file or a file per table.
- [Back up and restore](backup.md) — the engine's own tools, and what they need installed.
