# Reading and exporting

`query`, `explain` and `export` only ever read. `dump` and `compare` read the database's
structure and write nothing to the server. None of them asks a question first, because there is
nothing to say yes to.

## Giving the statement

`query`, `explain`, `export` and `exec` all take one statement, in one of three ways:

```sh
schemaic query "SELECT count(*) FROM orders" -c prod
schemaic query -f report.sql -c prod
schemaic query - -c prod < report.sql
```

It must be a single statement; a whole script is refused. Pass a statement that carries a password
with `-` on stdin, so it doesn't land in your shell history. A statement can't come from stdin
together with `--password-stdin`, since stdin has only one reader.

## Only reads

These commands accept only a statement that reads, such as a `SELECT`, and refuse anything else
with exit code 3. There is no flag to get past it, and it doesn't matter whether the connection is
marked read-only: a write belongs to [`exec`](exec.md).

The check isn't only about the text. The statement also runs on a session the server itself holds
read-only, so a function that writes is refused when it tries. SQL Server has no such session;
there the read runs inside a transaction that is always rolled back.

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
is listed on stderr. Both sides must be the same engine; MySQL and MariaDB count as one.
