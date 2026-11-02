# Importing a file

`import` loads a CSV, TSV, JSON or Excel file into a table that already exists — the app's
[Import](../import-export/import-file.md) on the command line, with the same checks.

```sh
schemaic import people.csv -t people -c prod -d shop
schemaic import export.xlsx -t sales.orders -c prod -d shop --sheet "March"
schemaic import dump.txt -t people -c prod -d shop --format csv --delimiter tab --null NULL
```

`-t` names the table — `orders`, or `sales.orders` outside the default schema. It loads into
tables, not views.

## Reading the file

The format comes from the file's extension, or from `--format csv`, `json` or `xlsx`:

| Format    | Extensions                     | Flags                                                   |
|-----------|--------------------------------|---------------------------------------------------------|
| CSV / TSV | `.csv`, `.tsv`, `.tab`, `.txt` | `--delimiter`, `--null`, `--trim`                       |
| JSON      | `.json`                        | An array of objects, or one object per line             |
| Excel     | `.xlsx`, `.xlsm`               | `--sheet NAME` — the first sheet when absent            |

- `--delimiter` takes one character (`;`, `|`) or `tab`. Without it the delimiter is guessed from
  the start of the file.
- `--null TEXT` says which field means `NULL`; repeat it for several (`--null NULL --null '\N'`).
  Without it, an empty field is `NULL`.
- `--trim` strips the spaces around every field.

A flag for another format — `--sheet` on a CSV, `--delimiter` on a JSON file — is refused rather
than ignored.

The first row is always read as the header. Unlike in the app, there is no preview to catch a
wrong guess, so it is never guessed: if the file has no header row, say so with `--no-header`.

## Matching the columns

Each column in the file is matched to the table column of the same name, ignoring case. With
`--no-header` they are matched by position instead. Two flags correct the match:

- `--map "Full Name=name"` loads the file's `Full Name` column into the table's `name`. It wins
  over a match by name, so the file column that matched `name` before is left out.
- `--skip notes` leaves the file's `notes` column out.

Repeat either for each column. A flag naming a column the file or the table doesn't have is
refused, so a misspelt `--skip` can't quietly import the column it meant to keep out. Columns the
database fills in itself, such as generated columns, take no value from a file.

Before loading, stderr names every file column that isn't going in, with the flags that settle
it:

```text
schemaic: the file's column 'extra' is not imported: no table column matches it; --map 'extra=<column>' to import it, or --skip 'extra' to say so
```

It also warns about a column that can't be `NULL`, has no default, and that nothing in the file
fills — the load will most likely fail on it.

## What happens

The whole file is read and checked against the table first — numbers that aren't numbers, empty
values in a column that can't be `NULL`, rows with the wrong number of fields. If anything is
wrong, nothing is written. The first 20 problems go to stderr, and the command exits with code 7:

```text
schemaic: line 2, column age: not a whole number ("notanumber")
schemaic: people.csv was not imported; nothing was written
```

Fix the file and run it again. Otherwise the rows go in inside one transaction:

```text
schemaic: imported 2 rows into shop.people
```

If the server refuses a row part-way, the transaction is rolled back and the command exits with
code 4. On a MySQL or MariaDB table whose storage engine isn't transactional, the rows written
before the failure stay.

`--timeout` defaults to an hour. A load stopped by it is rolled back; if the server doesn't
answer the cancel, the command exits with code 5 — check the table before importing again.

## What it refuses

- Any import on a connection marked **Read-only** — exit code 3. There is no `--yes` for it.
- Having no database to load into — pass `-d`, or give the connection a default database.
- An Excel workbook larger than 512 MiB. Save the sheet as CSV and import that instead. A large
  JSON or Excel file is held in memory while it loads, and stderr warns you when that will take a
  lot.

Each import is added to the [write log](exec.md#the-write-log), like a write from `exec`.
