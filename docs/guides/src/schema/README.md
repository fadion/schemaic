# Schema

The schema tree is the **SCHEMA** panel on the left. It shows every database on the active
connection, and everything in them. Show or hide it with <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>E</kbd>.
Most of what you can do to a database object starts with a right-click on its row here.

## What's in the tree

Each database opens to its tables and views, and each table opens to its columns, then its keys
and indexes. Objects that aren't tables — **Functions**, **Procedures**, **Sequences** and the
like — sit in folders after the tables. A folder only appears if it has something in it.

What you see depends on the engine:

- *Schemas* appear as their own level on PostgreSQL and SQL Server, but only when there's more
  than one, or the only one isn't the default (`public`, `dbo`). MySQL, MariaDB and SQLite have no
  schema level.
- *Folders* follow what the engine has: **Events** on MySQL; **Sequences** on PostgreSQL, SQL
  Server and MariaDB; **Types**, **Domains** and **Extensions** on PostgreSQL; **Alias types**,
  **XML schema collections** and **Synonyms** on SQL Server. SQLite has only tables and views.
- *Partitions* nest under their table on PostgreSQL. On MySQL and SQL Server they're listed after
  the table's keys.
- *Triggers* have no row of their own. Reach them from their table's **Triggers** entry, or by
  searching their body with [Find anywhere](#finding-things).

A database marked **Single-user — not read** is in a state where reading it would take the only
connection it allows. Press **Read** if you mean to.

## Opening things

| You want to…                          | Do this                                                     |
|---------------------------------------|-------------------------------------------------------------|
| See a table's or view's rows          | Double-click it, or select it and press <kbd>Enter</kbd>    |
| Jump to a column in the data          | Double-click the column                                     |
| Edit a function, sequence, type…      | Double-click it                                             |
| Expand or collapse a database/folder  | Double-click it, or click its arrow                         |

Opening a table that already has a tab switches to that tab instead of opening a second one;
**Open in new tab** on the right-click menu forces a new one.

The tree is fully keyboard-driven: <kbd>↑</kbd>/<kbd>↓</kbd> move, <kbd>→</kbd> expands or steps
into a row, <kbd>←</kbd> collapses or steps out to the parent.

## Finding things

There are two searches, and they answer different questions.

- *The tree's filter box* (**Search…**, at the top of the panel) narrows the tree to names that
  contain what you type. A table whose *column* matches stays visible with that column revealed. A
  database or schema whose own name matches shows everything inside it.
- **Find anywhere** (<kbd>Ctrl</kbd>+<kbd>P</kbd>) searches across every database at once: object
  and column names first, then the *bodies* of views, routines, triggers and events — so it finds
  the procedure that writes to a table, not just the table — then your snippets, open tabs and
  query history. Databases you've hidden are left out.

## Right-click menus

Each kind of row has its own menu. The ones you'll use most:

- *Database* — **ER diagram**, **Compare with**, **Generate DDL**, **Import**, **Export**,
  **Back up**, **Create** (a table, view, routine…), and housekeeping: **Favourite**, **Colour**,
  **Hide**.
- *Table* — **Open**, **Properties**, **Used by**, **Live monitor**, **ER diagram**,
  **Compare with**, **Generate** (ready-made `CREATE`, `INSERT`, `UPDATE` and `DELETE` statements),
  **Export**, **Copy to** another database, **Edit table**, **Triggers**, **Truncate**, **Drop**.
- *View* — much the same, with **Edit view** in place of **Edit table**.
- *Column*, *key* and *index* rows — **Copy name**, **Used by**, and the matching
  **Edit column**, **Edit index** or **Edit foreign key**.

Every menu ends with **AI explain**, which asks the AI assistant to describe the
object.

Two rules hold across all of them. **Drop** is always last and in red, and it never runs straight
away: you see the SQL first. An entry that's missing means the engine can't do it; an entry that's
greyed out means the connection is marked read-only.

The panel's gear menu (and a right-click on empty space) holds what isn't about one object:
**Refresh**, **Collapse all**, **Users and privileges**, **Server variables**, **Create database**,
and **Show table sizes**, which adds each table's size on disk to its row.

## Keeping a big server tidy

- **Favourite** pins a database to the top of the tree with a star.
- **Colour** marks a database or table with a coloured dot. A database's colour also tints its
  query tabs — handy for telling production from staging at a glance — and a table's colour
  carries into the ER diagram.
- **Hide** takes a database out of the tree and out of Find anywhere. Bring it back from the eye
  button at the top of the panel.
- **Refresh** re-reads the whole database, even when you pick it on a single table. The tree
  doesn't notice changes made outside Schemaic until you do.

## In this section

- [Designing a table](table-designer.md) — create a table or change one, and review the SQL first.
- [ER diagram](er-diagram.md) — see how tables relate, and export the picture.
- [Live monitor](live-monitor.md) — watch rows change in a table as it happens.
- [Comparing schemas](compare.md) — find what differs between two databases, and bring one in line.
