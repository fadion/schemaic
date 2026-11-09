# Reading and exporting

`query`, `explain` and `export` only ever read, as do
[the performance commands](#looking-into-a-slow-server). `dump` and `compare` read the database's
structure and write nothing to the server. None of them asks a question first, because there is
nothing to say yes to.

## Giving the statement

`query`, `explain`, `export` and `exec` all take one statement, in one of three ways:

```sh
schemaic query "SELECT count(*) FROM orders" -c prod
schemaic query -f report.sql -c prod
schemaic query - -c prod < report.sql
```

It must be a single statement; a whole script is refused, except by
[`exec --script`](exec.md#running-a-script). Pass a statement that carries a password with `-` on
stdin, so it doesn't land in your shell history. A statement can't come from stdin
together with `--password-stdin`, since stdin has only one reader.

## Filling in parameters

A statement can carry `:name` placeholders, as in the editor's
[parameters](../query-editor/README.md#running-sql). Give each one a value with `-p`
(`--param`), repeated once per name:

```sh
schemaic query "SELECT * FROM orders WHERE customer_id = :id AND state = :state" -c prod -d shop -p id:number=7 -p state=sent
schemaic query -f report.sql -c prod -d shop -p col:sql=created_at
```

| Written                  | Goes in as                                                                 |
|--------------------------|----------------------------------------------------------------------------|
| `-p name=value`          | Text, quoted for the engine                                                |
| `-p name:number=42`      | A number; a value that isn't one is refused                                |
| `-p name:null`           | `NULL`                                                                     |
| `-p name:sql=created_at` | SQL, put in as written — for what isn't a value, like the column in `ORDER BY :col` |

Only the first `=` splits, so a value may contain `=`. Quote the whole `-p` for your shell when
the value has spaces.

A placeholder with no `-p`, or a name given twice, stops the command with exit code 2 before
anything is sent. A `-p` the statement doesn't use is only warned about on stderr. The checks
below read the statement with the values in, so a `:sql` value that writes is refused by `query`
just as if you'd typed it into the statement. A password in a `-p` value lands in your shell
history as one in the statement would, and gets a warning on stderr: write it into the statement
instead, and pipe that with `-`.

## Only reads

These commands accept only a statement that reads, such as a `SELECT`, and refuse anything else
with exit code 3. There is no flag to get past it, and it doesn't matter whether the connection is
marked read-only: a write belongs to [`exec`](exec.md).

The check isn't only about the text. The statement also runs on a session the server itself holds
read-only, so a function that writes is refused when it tries. SQL Server has no such session;
there the read runs inside a transaction that is always rolled back.

On DuckDB a read reaches only the database's own tables. DuckDB can query files on your computer
directly, so `read_csv`, `read_parquet` and the other file readers are refused, and so is a file
name in `FROM`.

## `query`

Prints the rows, 200 at most unless you raise `--limit`. When the limit cuts the result short, a
note says so on stderr; add `--fail-on-cap` to get exit code 6 as well, for a script that doesn't
read stderr.

## `explain`

```sh
schemaic explain "SELECT * FROM orders WHERE customer_id = 7" -c prod -d shop
```

Prints the server's plan as rows, and lists what in it looks slow — full scans, sorts, temporary
tables, an index the server says is missing — on stderr. The statement is planned, never run, so
the figures are the optimiser's estimates.

## `export`

```sh
schemaic export "SELECT * FROM orders" -c prod -d shop -o orders.xlsx
```

Streams the whole result into a file, with no row cap. The format follows the file's extension —
JSON, CSV, SQL, Markdown, HTML or Excel — or name it with `--format` (`json`, `csv`,
`sql`, `md`, `html`, `xlsx`). The file is written as `orders.xlsx.part` and renamed when it's
complete, so a failed export never leaves a half-written file under the real name.

## `dump`

```sh
schemaic dump -c prod -d shop -o shop.sql
schemaic dump -c prod -d shop -t orders -t customers --data-only -o orders.sql
```

Writes a `.sql` file that recreates the database: its tables and views, their rows, and its types,
sequences and routines. `-t` limits it to the tables you name (repeat it for more; outside the
default schema, write `sales.orders`). `--schema-only` leaves out the rows; `--data-only` leaves
out the `CREATE`s. It is the same dump as the app's, and progress is printed to stderr a table at
a time.

## `compare`

```sh
schemaic compare -c staging -d shop --source-connection prod --source-database shop
```

Prints the SQL that would make one database's structure match another's — the same comparison as
[the schema tree's](../schema/README.md) **Compare with**. `-c` and `-d` name the database that
would change; `--source-connection` and `--source-database` name the one it should come to match,
on the same connection unless you say otherwise. `-t` narrows it to the tables you name, with
their triggers.

Nothing is run. The script goes to stdout for you to read or save, and what it drops or leaves out
is listed on stderr. To apply it, save it and run it with [`exec --script`](exec.md#running-a-script). Both sides must be the same engine; MySQL and MariaDB count as one.

## Looking into a slow server

```sh
schemaic statements -c prod --order mean --limit 10
schemaic indexes -c prod -d shop
schemaic sizes -c prod -d shop --indexes
schemaic locks -c prod
```

Four commands read what the app's [Performance](../schema/performance.md) view and
**Server activity** show. They only read: a fix they point to is yours to run.

| Command      | What it lists                                                                    | Columns |
|--------------|----------------------------------------------------------------------------------|---------|
| `statements` | The statements the server spent most time on. `--order` is `total` (the default), `mean` or `calls`; `--limit` is 20 by default, up to 500 | `total_ms`, `mean_ms`, `calls`, `rows`, `database`, `query` |
| `indexes`    | Indexes unused since the counters were reset, exact duplicates, and indexes whose key starts another's | `table`, `index`, `problem`, `detail`, `bytes` |
| `sizes`      | The largest tables, or with `--indexes` the largest indexes. `--limit` is 20 by default, up to 100 | `table`, `bytes`, `size`, `rows_estimate` (or `table`, `index`, `bytes`, `size`) |
| `locks`      | The locks sessions hold and wait for, and who blocks whom                        | `session`, `state`, `kind`, `mode`, `object`, `blocked_by` |

- *A database.* `indexes` and `sizes` look in one, so they need `-d` or a connection with its
  own. `locks` is the whole server's and takes no `-d`.
- *Statement statistics have to be on.* `statements` reads `pg_stat_statements` on PostgreSQL,
  `performance_schema`'s digests on MySQL and MariaDB, and the database's Query Store on SQL
  Server ([what turning each on takes](mcp.md#performance-tools)). When the source is off it says
  why on stderr and exits 2, with `hint: to turn it on, run:` and the statement where one does it.
  It never runs it. SQLite keeps no statistics, so it exits 2 there too.
- *Usage counts start again* when the server restarts or its statistics are reset, so an index
  `indexes` calls unused may only be unused lately. The note on stderr says so, or that the server
  keeps no counts and only duplicates and prefixes are listed.
- *Missing indexes.* On SQL Server, `indexes --missing` lists the indexes the optimiser recorded
  as missing instead, with a `ddl` column holding the `CREATE INDEX` for each. Elsewhere it exits 2.
- *Index sizes.* MySQL and MariaDB size indexes per table, not per index, so `sizes --indexes`
  exits 2 there.
- *Locks.* SQLite has no server sessions, so `locks` exits 2 there. MariaDB lists only locks
  someone is waiting on, and says so on stderr.
