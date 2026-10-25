# Editing data

You edit a result in place: change cells, add and delete rows, then commit. Nothing reaches the
database until you do, and what you commit runs as one transaction.

## What can be edited

A cell can be edited when it comes straight from a table column and Schemaic can tell which row it
is — the table's primary key, or a unique key with no NULLs, has to be in the result. On SQLite a
table with no key is edited through its hidden `rowid`.

- *Not editable:* computed columns (`price * qty`, `COUNT(*)`), a table with no usable key, any
  result on a connection marked read-only, and a pinned result.
- *A join* can be edited cell by cell when each table's key is in the result. Adding, cloning and
  deleting rows needs a result from a single table.

The right-click menu tells you which case you are in: it offers **Edit row** where something in
the row can be written, and **View row** where nothing can.

## Changing cells

Double-click a cell, or press <kbd>Enter</kbd>, to edit it. <kbd>Enter</kbd> keeps the change,
<kbd>Tab</kbd> keeps it and moves to the next editable cell, <kbd>Esc</kbd> throws it away. A
changed cell turns green. It is *staged*: on screen, waiting to be committed.

- **Set to NULL** and **Set to DEFAULT** on the cell's right-click menu stage those values.
  Typing the word `NULL` stages the *text* `NULL`; a NULL shows in italics, text does not.
- <kbd>Ctrl</kbd>+<kbd>V</kbd> pastes a block copied from the grid or a spreadsheet, starting at the
  selected cell. A single copied value fills the whole selection — the quick way to set a column
  to one value. A pasted `NULL` is a real NULL. What couldn't be pasted (read-only columns, rows
  marked for deletion) is counted in the note at the bottom.
- **Fill down** and **Fill up**, with several rows selected, copy the top or bottom row's values
  into the rest.

Columns whose values are spelled out get a control instead of a text box. Booleans and `ENUM`s open
a list to pick from. A `SET` picks one member at a time in a cell, and shows every member as a chip
in the row panel. Dates and times keep their text box, with a calendar beside it and **Today** or
**Now** to fill in the present.

## Adding, cloning and deleting rows

The buttons at the right of the status line do these:

- **Add a row** appends a blank row at the bottom, marked `*`. Cells you leave empty show what the
  server will fill in — `<auto>`, `<default>`, `<null>` — or `<required>`, which the row can't be
  saved without.
- **Clone the selected row** adds a copy of it, leaving out auto-increment columns so the server
  numbers the copy. **Duplicate row** on the right-click menu does the same; with rows selected in
  the gutter, it copies all of them, and asks first past 500.
- **Delete row**, or <kbd>Del</kbd>, marks the selected rows in red for deletion. **Undo delete**,
  or <kbd>Del</kbd> again, unmarks them.

## Committing

The ✓ button shows how many changes are waiting; its tooltip reads *Commit 3 changes
(Ctrl+Enter)*. Press it, or <kbd>Ctrl</kbd>+<kbd>Enter</kbd>, to write them. The ✗ beside it
(**Discard all pending changes**) drops them, and the copy icon (**Copy the changes as SQL**) puts
the statements on the clipboard instead, to read or to run elsewhere.

Here is what a commit does:

1. Schemaic turns each change into one statement — a `DELETE` per deleted row, an `UPDATE` per
   changed row, an `INSERT` per new row — that finds its row by the key.
2. It runs them all in *one transaction*, deletes first, then updates, then inserts.
3. *Every statement has to affect exactly one row.* If any of them affects none or more than one,
   the whole commit is rolled back and nothing is written.

That last rule is the safety net. It fires when the row you edited has been deleted by someone else
since you read it (zero rows), or when the key isn't as unique as it looked (two rows) — the cases
where writing anyway would change the wrong data. When it fires, the error appears at the bottom
of the grid and your green edits stay, so you can re-run the query, look, and try again.

The net checks that the row is still there, not that it is unchanged. If someone else changed a
different column of the same row, your `UPDATE` still finds the row and writes over their value,
so re-run a result that has been open a long time before editing it.

On success, updated rows are refreshed in place; adding or deleting rows re-runs the query.

- *In a Manual transaction tab*, the commit goes into your open transaction, and is permanent only
  when you commit that.
- *On a MySQL `MyISAM` table*, there is no transaction to roll back. If a commit fails part-way,
  Schemaic says so rather than claiming nothing was written.
- *If another session holds a lock* on a row you are writing, the commit waits, and a note at the
  bottom says so. When the lock is held by one of your own tabs, the note offers
  **Roll back Query 2** (named after that tab) to release it.

## The row panel

**Edit row** opens the row as a form at the bottom of the results pane, one field per column, with
the full value in each. Use it for long text or JSON. A `json` or `jsonb` column opens as a tree you
can expand and edit leaf by leaf; invalid JSON outlines the field in red.

The panel's ✓ saves that row straight away, as its own commit, with the same one-row rule. It
doesn't commit the green edits waiting elsewhere in the grid.

## Binary values

The grid shows a binary value as its size, `<2048 bytes>`, because it doesn't hold the bytes.
Double-click the cell, press <kbd>Enter</kbd>, or choose **Edit binary** (**View binary** where it
can't be written) to open it:

- **Preview** shows PNG, JPEG, GIF, BMP, WebP and icon images; **Hex** shows any value byte by byte.
- **Save to file** writes the value out.
- **Load from file** puts a file's bytes in the cell, staged like any other edit — commit in the
  grid to write it.

The panel reads up to 64 MiB of a value.

## Generating rows with AI

The ✦ menu (**Generate rows with AI…**) has **AI fill value**, **AI insert row** and
**AI seed table**. Each sends some of the table's rows to the AI assistant and stages what comes back
as ordinary edits, for you to review before committing. A connection can turn this off, because it
sends data — see [AI assistant](../ai/README.md).
