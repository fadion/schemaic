# Query editor

The query editor is where you write and run SQL. Each tab is one editor with its own connection
and database, and the results of its last run sit underneath it.

## Tabs

<kbd>Ctrl</kbd>+<kbd>T</kbd> opens a new tab on the active connection, and
<kbd>Ctrl</kbd>+<kbd>W</kbd> closes the current one. The database a tab runs against is the
selector in the **QUERY** header above the editor; a tab with none chosen reads **No database**.

| You want to…                     | Do this                                                        |
|----------------------------------|----------------------------------------------------------------|
| Switch tabs                      | <kbd>Ctrl</kbd>+<kbd>Tab</kbd>, or <kbd>Ctrl</kbd>+<kbd>1</kbd>…<kbd>9</kbd> |
| Rename a tab                     | Double-click its title                                         |
| Keep a tab from being closed     | Right-click it › **Pin**                                       |
| Get back a tab you just closed   | <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>T</kbd>                  |
| Copy a tab, text and all         | Right-click it › **Duplicate**                                 |

A pinned tab has no ×, and **Close other tabs** and **Close all tabs** leave it alone; **Unpin**
it to close it. Tabs come back when the app starts again unless you turn off
**Restore tabs on startup** in Settings. A tab can also be a `.sql` file on disk — see
[SQL files](sql-files.md).

If a database has a colour (see [Schema](../schema/README.md)), its tabs carry it too.

## Running SQL

<kbd>Ctrl</kbd>+<kbd>Enter</kbd> runs SQL. What it runs depends on where you are:

- *With a selection*, it runs exactly what's selected. A selection holding several statements runs
  them all.
- *With just a caret, in a tab holding one statement*, it runs that statement.
- *With a caret, in a tab holding several*, a small menu opens with the statement under the caret
  outlined: **Run current** (already selected — press <kbd>Enter</kbd>) or **Run everything**.

Every statement that's run gets its own result in the strip under the editor, so a
**Run everything** over three `SELECT`s gives you three results to flick between. Results and what
you can do with them are in [Results grid](../results-grid/README.md).

With several carets and no selection, nothing runs: select what you mean, or go back to one caret.

*Parameters.* Write `:name` anywhere a value goes — `WHERE id = :id`, or even `LIMIT :n` — and a
row of fields appears under the editor, one per name. Each has a chip that says what kind of value
it is (text, number, `NULL` or raw SQL); click it to change. The run waits until every parameter
has a value. Values are kept for the session only.

## What asks before running

Some statements stop at a bar above the editor before they run. There are two kinds of stop.

*Refused outright* — nothing you press will run it:

- the connection is marked read-only and the statement writes, or isn't one Schemaic can prove
  only reads;
- the tab has no database and the statement needs one.

*Asked first* — the bar says why, and **Run anyway** goes ahead:

- a statement that destroys rows wholesale: `DELETE` or `UPDATE` with no `WHERE`, `TRUNCATE`,
  `DROP TABLE`, `DROP DATABASE`, `DROP SCHEMA`, `CREATE OR REPLACE TABLE`, and similar;
- any write at all, while **Confirm before running writes** is on (Settings › **Query**). It's on
  by default;
- a run that would replace a result holding edits you haven't committed yet.

Whether a connection is read-only shows at the bottom of the window as **Read-only** or
**Write mode** (amber). Click it to switch. The same guard covers every other way Schemaic runs your
SQL — the AI assistant, the command line, a script file — and none of them is laxer than this one.

## Transactions

Each tab is in one of two modes, shown at the bottom of the window. Click it to switch.

- **Auto-commit** (the default) — every statement is committed as soon as it runs.
- **Manual** — the tab holds its own connection and a transaction stays open across runs until you
  end it. While one is open the footer counts the statements in it and shows **Commit** and
  **Rollback**. Closing the tab, or switching it back to **Auto-commit**, asks which you want
  first.

A few things the server decides, not Schemaic:

- *MySQL and MariaDB* commit an open transaction before running DDL such as `CREATE` or `ALTER`.
  Schemaic tells you when that has happened, since **Rollback** can no longer undo what came
  before.
- *PostgreSQL* stops a transaction at the first failed statement. The footer then reads
  *Tx aborted — rollback to continue*.
- *SQLite* has no Manual mode in Schemaic, so the switch isn't shown: every statement commits as it
  runs. Nor does a read-only SQL Server connection.

## Panels beside the editor

The buttons at the top right open panels in a column beside the editor:
**Query history** and **Snippet library** (see [History and snippets](history-and-snippets.md)),
the **AI assistant** ([AI assistant](../ai/README.md)), **Server activity**, and the **Terminal**.

The **Terminal** (<kbd>Ctrl</kbd>+<kbd>&#96;</kbd>) is a shell of your choice — pick it under
Settings › **Terminal**. Its database button opens the connection's own command-line client
(`mysql`, `psql`, `sqlite3`, `sqlcmd`) already signed in. It isn't offered on a read-only
connection, since the client wouldn't be read-only.

## In this section

- [Writing SQL](writing-sql.md) — completion, checks as you type, formatting, find and replace.
- [History and snippets](history-and-snippets.md) — find what you ran, and keep what you'll run again.
- [Query plans](query-plans.md) — see how the server will run a statement.
- [SQL files](sql-files.md) — open and save `.sql` files, and run a whole script.
