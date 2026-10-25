# Designing a table

The table designer creates a new table or changes an existing one. You describe the table you
want; Schemaic works out the SQL to get there and shows it to you before anything runs.

## Opening it

- *Change a table:* right-click it in the schema tree and choose **Edit table**. Right-clicking a
  column, index or key opens the designer on that row instead — **Edit column**, **Edit index**,
  **Edit foreign key**, **Edit primary key**.
- *Create a table:* right-click a database (or schema), then **Create** › **Table**.

A view has its own editor, under **Edit view**.

## The tabs

| Tab              | What it holds                                                               |
|------------------|-----------------------------------------------------------------------------|
| **Table**        | Name and comment; on MySQL and MariaDB, the storage engine and collation too |
| **Columns**      | Each column's name, type, nullability, default, identity, comment           |
| **Indexes**      | Indexes and unique constraints                                              |
| **Foreign keys** | References to other tables, with their on-delete and on-update actions      |
| **Checks**       | Check constraints                                                           |

Each list tab has a bar for adding, removing and reordering rows, and a form for the selected row.

A few things worth knowing:

- **Type** is free text and goes to the server as written — the server decides what it means.
- A generated column takes an expression in **Generated from**, and can't also have a default.
- Indexes flagged *redundant*, and foreign keys with *no index* behind them, are advice. They never
  stop you from going ahead.

## Reviewing and applying

When you're done, press **Preview SQL**. If something is missing — a table with no name, no
columns — the designer says so first.

The preview shows the exact SQL that will run. From there you can:

- **Apply** it to the database,
- **Copy** it, or **Open in editor** to run it yourself later or keep it in a migration,
- go **Back** to the designer to change something.

The preview also lists what depends on the table under **Used by**, so you can see which views or
routines a change might affect before you apply it. On a read-only connection there is no
**Apply**; the preview is still there to read and copy.

## What differs between engines

Some changes can't be made with a plain `ALTER TABLE` on every engine. Where that's the case,
Schemaic rebuilds the table: it creates a new one with the new shape, copies the rows across and
swaps it in, all inside one transaction, so a failure leaves the original untouched. The preview
shows every step.

- **SQLite** rebuilds for most changes, since its `ALTER TABLE` can do very little.
- **SQL Server** rebuilds to turn identity on or off, to make a column computed or plain, and to
  move a column.
- **PostgreSQL** can't reorder columns, so there are no move arrows there.

Other engine-specific fields appear only where the engine has them: **On update** on MySQL; index
methods on PostgreSQL; partial indexes (**Only rows where**) on PostgreSQL, SQLite and SQL Server;
included columns on PostgreSQL and SQL Server; columnstore and XML index kinds on SQL Server.
