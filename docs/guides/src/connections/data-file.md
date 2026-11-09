# Querying a data file

Query a CSV, Parquet or JSON file with SQL, without importing it into a database first. Schemaic
opens the file in a throwaway in-memory DuckDB session.

## Opening a file

Open the command palette (<kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>P</kbd>), choose
**Open data file**, and pick the file.

| Kind    | Extensions                                       |
|---------|--------------------------------------------------|
| CSV     | `.csv`, `.tsv`, and either compressed as `.gz` or `.zst` |
| Parquet | `.parquet`                                       |
| JSON    | `.json`, `.jsonl`, `.ndjson`, and the same compressed as `.gz` or `.zst` |

A compressed Parquet file, such as `sales.parquet.gz`, isn't opened; Parquet compresses inside
the file already.

What you get:

- *A session named after the file*, such as `sales.csv`, which appears as a connection and
  becomes the active one.
- *A view named after the file* without its extensions, such as `sales` — or `sales_2` if the
  name is taken — in the session's `main` schema.
- *A tab running* `SELECT * FROM sales`.

## Joining two files

With a data-file session as the active connection, **Open data file** adds the next file's view
to that same session, so you can join the two in one query:

```sql
SELECT o.*, c.name
FROM orders o
JOIN customers c ON c.id = o.customer_id;
```

With any other connection active, it opens a new session.

## What happens to the file

The file is only read, never written. Its view reads the file each time you query it, so changes
to the file show up in the next query, and nothing is copied in. If you move or delete the file,
queries on its view fail.

Within the session you can do anything DuckDB allows — create a table from the view
(`CREATE TABLE t AS SELECT …`), add more views, and so on. All of it lives in memory only.

## When a CSV reads wrong

DuckDB guesses a CSV file's delimiter and whether its first line holds column names. When it
guesses wrong, right-click the file's view in the schema tree and choose **CSV read options**.

- **Delimiter** — **Detect**, **Comma**, **Semicolon**, **Tab** or **Pipe**.
- **Header row** — **Detect**, **Yes** or **No**.

The dialog opens on what the view reads with now; **Detect** leaves the choice to DuckDB.
**Continue** opens the view editor with the view's `read_csv(…)` call rewritten. Nothing changes
until you press **Preview SQL** there and apply it, as with any view edit.

The entry is there only while the view still reads one file with `SELECT * FROM read_csv(…)` and
no options besides those two, because the dialog writes the whole call again. If you've edited
the view by hand to add more — another option, a filter, several files — change it in
**Edit view** instead.
That is also where to fix a column type DuckDB guesses wrong, with `read_csv`'s `types` option,
for example `types = {'zip': 'VARCHAR'}`.

## Closing a session

A data-file session is never saved. It isn't in your saved connections, **Export connections**
leaves it out, and it's gone when Schemaic closes.

To close it sooner, open **Manage connections**, right-click the session and choose
**Close session**. Schemaic asks first: everything made in the session goes with it — its views, any
tables, its query history and its tabs. The data files stay as they are.

The AI assistant and the command line work from your saved connections, so they can't reach a
data-file session.
