# Running a SQL script

Runs a `.sql` file — a dump, a migration, a seed script — against a database, statement by
statement, with progress and a way to stop.

## Opening it

Right-click a database in the schema tree and choose **Import**, then **Choose a file…**. On
PostgreSQL and SQL Server you can start from a schema's row as well. It's greyed out on a
read-only connection.

The script runs on the database you right-clicked, but its own statements have the last word: a
`USE` or `CREATE DATABASE` in the file takes it elsewhere, and on PostgreSQL anything the file
doesn't qualify goes wherever the server's `search_path` says — not necessarily into the schema
you started from.

## Before you run it

Schemaic reads the file and tells you, under **What this file does**:

- how many statements of each kind it holds;
- in red, how many *destroy* data — `DROP`, `TRUNCATE`, `DELETE` — since that can't be undone from
  here;
- whether the file opens its own transaction. If it does, it lands whole or not at all; if not, a
  failure part-way leaves what ran before it in place. Schemaic adds no transaction of its own.

A very large file is only read in part for this summary, and the summary says so.

## Running

Press **Run**. The window counts the statements as they go, and **Stop** halts the run. When it
ends you see how many statements ran, or the line the failure came from and what the server
said — and, either way, whether what ran before is still applied.

A `.sql` file from [Export](export.md) loads back this way.
