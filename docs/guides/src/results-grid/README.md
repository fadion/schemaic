# Results grid

Whatever you run lands in the **RESULTS** pane under the editor, as a grid. Opening a table from
the schema tree lands there too, in a tab of its own. The grid reads, sorts and filters what came
back, and — where the rows come from a table Schemaic can identify — edits it and writes the
changes back.

## Results and the strip

One run gives one result. Running several statements at once gives one result per statement, and
the strip along the top of the pane lists them as chips; click a chip to show that result. Hover
over a chip to see the full statement, how long ago it ran, and how much memory it holds.

Right-click a chip for:

- **Pin** / **Unpin** — a pinned result stays when you run again. The next run replaces only the
  unpinned ones, so you can keep a "before" while you change things.
- **Close**, **Close other results**, **Close all results** — the last spares pinned results.
- **Compare with …** — set this result against another one in the tab. See
  [Comparing results](compare.md).

A pinned result is a snapshot, and it is read-only: the status line and its chip say **kept — read-only**,
and editing, filtering, paging and re-reading are all switched off on it. Writing back from rows
that may be twenty minutes old would quietly overwrite whatever changed since. Pinned results stay
in memory until you close them.

When a statement fails, its error appears under the SQL in the editor, beside **Explain** and
**AI fix**, rather than in the grid.

## Reading the grid

The status line above the grid says which database the result came from, how many rows and
columns it has, and how long it took: `world · 100 rows · 15 cols · 1 ms`.

- *Big results are capped.* A query stops reading at the **Default row limit** in Settings
  (200,000 rows unless you change it), and the line says `(capped)` — or `200k of ~4.2m rows` where
  the table's size is known. Click **read all rows** (or **read 1m rows**, on a very big table) to fetch more.
- *Table tabs page.* A table opened from the tree shows 100 rows at a time, with `‹ Rows 101–200 ›`
  arrows on the status line. Any single-table `SELECT` with a fixed `LIMIT` gets the same arrows.
- *Headers say what a column is* — its type under its name, a key icon for primary keys, indexes
  and foreign keys (when the table was opened with its schema loaded), and a faint ∅ when the
  column can hold NULL.

Click a cell to select it; Shift+click, Shift+arrows or a drag selects a block, and a click in the
row-number gutter selects whole rows. Select cells in one column and a readout at the bottom shows
their count, nulls, distinct values and, for numbers, sum, average, median, min and max. Click a
figure to copy it.

| You want to…              | Do this                                                        |
|---------------------------|----------------------------------------------------------------|
| Find a value              | <kbd>Ctrl</kbd>+<kbd>F</kbd>, then <kbd>Enter</kbd> for the next match |
| Jump to a row             | <kbd>Ctrl</kbd>+<kbd>G</kbd> and type a number — `200k` works too |
| Select everything         | <kbd>Ctrl</kbd>+<kbd>A</kbd>                                   |
| See one row as a form     | Right-click a cell › **Edit row** (or **View row**)            |

## Sorting and filtering

Click a column header to sort by it: ascending, then descending, then back to unsorted.

Where the result comes from a single table with a key — a table tab, or a plain
`SELECT … FROM one_table` — the sort and filter go to the server: Schemaic re-runs the query with
an `ORDER BY` or `WHERE`, so they cover the whole table, not just the rows on screen. Those results
also get a filter bar labelled `WHERE`: type a condition, such as `age > 30 AND city = 'Oslo'`, and
press <kbd>Enter</kbd>. The ✕ at its end clears it.

Right-clicking a cell adds to that filter in one move: **Filter by this value** or
**Exclude this value**. Each adds its condition to what is already there with `AND`.

Any other result — a join, a query with a `GROUP BY` — sorts the rows already read, in memory. If
the result was capped, the line says *sorted subset (capped) — not the full order*.

A sort, filter or page change re-reads the rows, so it is refused while you have unsaved edits:
**Commit or discard your changes first — re-reading the rows would throw them away**.

## Arranging columns

Drag a header's right edge to resize a column; double-click it to fit the column to its content.
Right-click a header for the rest:

- **Freeze** keeps the column at the left while the others scroll. Freeze as many as you like;
  their headers are hatched. **Unfreeze** lets one go.
- **Hide column** takes it off screen. While any are hidden, **Show hidden columns (N)** brings them
  back one at a time, or all at once with **Show all** — and the `#` corner of the grid turns into
  an eye with the count.
- **Format as** changes how the values *look*, never what they are: **Timestamp** reads an epoch
  number as a date, **Number grouping** adds separators, **File size** shows bytes as `1.5 MB`,
  **Boolean** shows `0` and `1` as `false` and `true`. Schemaic remembers the choice for that column. **None** clears it.
- **Copy name**, **Copy qualified name** and **Copy values** — see
  [Copying and exporting](exporting.md).

## Following relations

A foreign key is a link, both ways:

- **Follow relation** on a foreign-key cell — or <kbd>Ctrl</kbd>+click on it — opens the table it
  points at, in a new tab, filtered to the row it names.
- **Referenced by** lists the tables whose foreign keys point at this row's table. Pick one to open
  it filtered to the rows that point at this row.

Both need the table's schema to be loaded, and both work on saved rows only — a row you have added
but not committed has nothing to follow yet.

The **RESULTS** title bar has two more ways into the table behind the result: **Table properties…**
and **Live monitor…**. They dim when the result doesn't come from a single table.

## In this section

- [Editing data](editing.md) — change cells, add and delete rows, and how the changes reach the
  database.
- [Copying and exporting](exporting.md) — the clipboard, the file formats, and exporting every row
  of a capped result.
- [Charts](chart.md) — a bar, line or histogram of the result.
- [Comparing results](compare.md) — what changed between two runs of a query.
