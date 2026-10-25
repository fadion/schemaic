# Data access

Whatever the assistant reads goes to whichever service your agent CLI talks to. Each connection
decides how much of its *data* that can include. Structure — names, types, definitions — goes at
every level; rows are what the levels are about.

Set it in the connection's settings, under **AI data access**. The **AI assistant** section of
Settings shows the active connection's level, but doesn't change it. It's per connection on
purpose: a scratch database on your laptop and a client's production server deserve different
answers.

## The levels

| Level                    | Rows that can leave your machine                                          |
|--------------------------|---------------------------------------------------------------------------|
| **Schema only**          | None. The assistant can't query or sample, and attaching rows is refused. |
| **Only what I attach**   | Only rows you hand over yourself. This is the default.                    |
| **Let it read data**     | Whatever the assistant chooses to read, as well as what you attach.       |

In more detail:

- **Schema only** — the assistant sees structure and nothing else. Table definitions are sent in
  full, so column defaults, comments, check expressions and a view's whole body are included:
  those are definitions, not rows. In the results grid, the AI actions are replaced by a note
  saying they're off for this connection.
- **Only what I attach** — the assistant can't read rows by itself. Rows reach it only when you
  send them: **Attach 3 rows to chat** in the grid, or **AI summary** of a selection. The gesture
  is the consent, and it covers what you selected and nothing more.
- **Let it read data** — the assistant can run read-only queries, read sample rows, and see a few
  example rows when it describes a table. **AI fill value**, **AI insert row** and **AI seed
  table** also send up to 20 of the table's rows, newest by primary key, as examples to match.

There's no masking level. A model can't tell a masked value from a real one, so it would reason
confidently about made-up data. Either the rows go or they don't.

## Error messages

Database errors often quote a value back: `Duplicate entry 'alice@example.com' for key
'users.email'`. Below **Let it read data**, Schemaic replaces the quoted value with `<redacted>`
before the message goes to the assistant — for **AI fix**, **Explain**, and a failed result in the
panel. The constraint, column and kind of error still go, since they're what make the error
fixable.

## What always goes

Whatever the level, a message carries your question, your **Custom instructions**, the schema
outline your **Schema context** allows, the SQL in the active tab (or your selection), and the
column names and row count of its last result. It also carries the earlier turns of the
conversation — except at **Schema only**, where only your own earlier messages are replayed.

## How queries stay read-only

The assistant's queries are refused if they could write, whatever the connection's own
**Read-only** setting says:

- The statement has to be a single read: it must start with one of the engine's read-only
  statements (`SELECT`, `WITH`, `SHOW` and so on), and anything that could write is turned away
  before it reaches the server. On PostgreSQL and SQL Server, the functions a query may call are
  checked against a list as well.
- It then runs in a read-only session, so a query that's spelled like a read but writes anyway —
  a `SELECT` of a function that deletes rows — is refused by the server.
- SQL Server has no read-only session, so there the query runs inside a transaction that's always
  rolled back. A procedure that reaches outside the database, such as sending mail, can still
  have that effect. For a hard guarantee there, connect with a login that can only read.

This guards against mistakes and keeps the assistant from writing. It isn't a defence against a
database whose owner is out to trick you: table names, comments and values are text someone else
may have written, and the assistant is told to treat them as data, not instructions.

Changes the assistant suggests never run on their own. A table change comes back as a card you
open in the change preview, and SQL in a reply runs only when you press **Run**, under the same
checks as anything you run yourself.
