# Importing a file into a table

Loads the rows of a CSV, TSV, JSON or Excel file into an existing table. The table has to exist
first — create it with **Create** › **Table** if it doesn't (see [Schema](../schema/README.md)).

## Opening it

Right-click a table in the schema tree and choose **Import**. It's greyed out on a read-only
connection, and until the table's columns have loaded — expand it once if so.

## Choosing the file

Press **Choose a file…**. The format is picked from the file's extension, and you can change it
under **Format**:

| Format     | Extensions                         | Notes                                              |
|------------|------------------------------------|----------------------------------------------------|
| CSV / TSV  | `.csv`, `.tsv`, `.tab`, `.txt`     | The delimiter and header row are detected for you  |
| JSON       | `.json`                            | An array of objects, or one object per line        |
| Excel      | `.xlsx`, `.xlsm`                   | One sheet at a time — pick it under **Sheet**      |

The **Reading** settings change how the file is read, and the preview below follows them as you
change them:

- **Delimiter** and **First row is a header** — fix what detection got wrong.
- **Trim whitespace** strips spaces around every value, inside quotes too.
- **Empty field is NULL**, and **Other values meaning NULL** (such as `NULL`, `\N`, `NA`), decide
  which CSV values arrive as `NULL` rather than as text. JSON and Excel have real empty values
  and don't need them.

Press **Next** when the preview looks right.

## Mapping the columns

Each column in the file is matched to a column of the table — by name when the file has a
header, by position when it doesn't. Change any of them, or set one to **Skip** to leave it out.

Two warnings can appear here:

- a list of table columns that aren't mapped and can't be empty — the import will fail unless the
  database fills them in;
- on a MySQL or MariaDB table whose storage engine isn't transactional, a note that a failed
  import can't be undone.

Columns the database computes itself, such as generated columns, are never written, so a file
Schemaic exported imports back cleanly.

## Importing

Press **Import**. Schemaic first reads the whole file and checks every value against its
column — numbers that aren't numbers, empty values in a column that can't be `NULL`, rows with
the wrong number of fields. If anything is wrong, nothing is written: you get the list, with
line numbers, to fix and try again. Beyond that, the database decides what it accepts.

Otherwise the rows go in inside one transaction, so a failure leaves the table as it was.
**Cancel** while it runs rolls the import back.

## Large files

A CSV is read as it goes, so its size doesn't matter. JSON and Excel files are held in memory
while they load, and the window warns you when that will take a lot. An Excel workbook larger
than 512 MiB isn't opened at all — save the sheet as CSV and import that instead.
