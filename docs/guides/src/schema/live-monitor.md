# Live monitor

The live monitor watches one table and logs every row that is inserted, updated or deleted while
it's open — whoever makes the change. It's useful for seeing what an application actually does to
a table when you click a button in it.

## Opening it

Right-click a table or view in the schema tree and choose **Live monitor**. It's also on the
results toolbar when you're looking at a table's rows.

## Reading the log

Each change is a row: when it was seen, what happened, and the row's values. An update shows
`old → new` for the columns that changed; a deleted row is struck through. Newest changes are at
the bottom. The pills along the top — **Inserts**, **Updates**, **Deletes** — filter the log to
one kind, each with a count.

## Controls

- **Pause polling** stops checking for changes. **Resume polling** picks up again, and its first
  check shows everything that changed in between.
- The interval picker sets how often the table is checked: every 1, 2, 5 or 10 seconds.
- The clear button (**Clear the log**) empties the log, and asks first if you haven't saved it.
  The export button (**Export the log…**) saves it to a file.

Closing the monitor stops it watching, but the log is kept until you clear it.

## Limits

The monitor works by reading the table repeatedly and comparing, not by hooking into the server,
so it needs nothing set up on the server. That has a cost:

- It watches the first 1,000 rows by primary key, and keeps the last 1,000 changes.
- The table needs a key to tell rows apart. Without one, it can't track changes, and says so.
- Large binary values are compared by length only.
- If the table's columns change while it's watching, it starts over from that point.

Looking for running queries and sessions rather than row changes? That's the **Server activity**
panel, which can also cancel a query or kill a session.
