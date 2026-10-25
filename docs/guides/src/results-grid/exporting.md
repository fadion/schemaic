# Copying and exporting

Everything you copy or export from the grid is what the grid is showing: in the order on screen,
with your staged edits and new rows included, and with hidden columns left out of a copy.

## Copying

- <kbd>Ctrl</kbd>+<kbd>C</kbd> copies the selection as tab-separated text, ready to paste into a
  spreadsheet or back into the grid. A single cell copies its value alone.
- **Copy formatted**, on a cell's right-click menu, copies values the way a **Format as** choice
  shows them — the date rather than the epoch number.
- **Copy as**, on the cell and row-number menus, copies the selected rows (or the clicked row) in
  a format: JSON, CSV, SQL `INSERT`s, Markdown or HTML — and three more for working with the same
  rows in SQL:

  | Entry            | Copies                                                          |
  |------------------|-----------------------------------------------------------------|
  | SQL VALUES       | The rows as bare `(…), (…)` tuples, to paste into your own `INSERT` |
  | SQL UPDATE       | An `UPDATE` per row, setting the selected columns, keyed on the table's key |
  | SQL WHERE IN     | A `WHERE` clause that picks out exactly these rows by key       |

- A column header's **Copy values** copies that column's values as CSV, JSON or an
  **SQL IN list** — `IN (3, 7, 12)`, ready for a `WHERE`.
- **Copy the results…**, on the toolbar, copies the whole result in any of the text formats.

A cell holding a tab or a line break can't survive tab-separated text — it splits into two cells or
two rows when pasted. When a copy contains one, the note at the bottom of the grid says how many.

## Saving to a file

**Save the results to a file…** on the toolbar saves the result as JSON, CSV, SQL `INSERT`
statements, Markdown, HTML or Excel. A progress window counts the rows as they're written, has a
Stop button, and stays up to say how the export ended — including anything the file couldn't hold,
such as binary columns, which aren't exported as data, or an Excel cell cut to its
32,767-character limit.

Schemaic writes to `name.part` first and renames it at the end. If the export fails or you stop
it, a file you were overwriting is left as it was, and the rows already written are in the `.part`
file.

Both menus stay closed on a result with no rows.

## Every row of a capped result

When a result was capped at the row limit and can be re-run, the save menu asks which rows first:

- **Fetched rows (N)** — the rows the grid holds.
- *All rows (~N)* — runs the statement again and streams every row straight to the file, however
  many there are, without loading them into the grid.

The *All rows* entry says when its file will differ from the screen: *server order* if you sorted
by clicking a header (that sort only rearranges the rows already read), *committed rows only* in a
Manual transaction tab (the new read can't see your uncommitted work), and
*without your staged edits*.

To export a whole table or database without running a query first, use the schema tree's
**Export** — see [Import and dump](../import-export/README.md).
