# Running writes

`exec` is the one command that changes data. It's a separate command rather than a flag on
`query`, so a write is never one typo away from a read.

```sh
schemaic exec "UPDATE orders SET state = 'sent' WHERE id = 7" -c prod -d shop
schemaic exec - -c prod -d shop < change.sql
```

It takes one statement, given [the same three ways](reading.md#giving-the-statement) as `query`,
with its `:name` placeholders [filled in by `-p`](reading.md#filling-in-parameters).
Several statements are refused rather than half-run — run them one at a time, so a failure
part-way isn't something you have to piece back together, or run the file as
[a script](#running-a-script).

A write prints how many rows it changed — `(3 rows affected)`, or `{"affected": 3}` with
`--format json`. A statement that returns rows prints them instead.

## What it refuses, and what it asks

The command checks the statement before it connects, and there are two kinds of answer.

*Refused outright* — exit code 3, and no flag changes it:

- any write on a connection marked **Read-only**;
- a statement that needs a database when there is none to run in. Pass `-d`, or give the
  connection a default database.

*Asked first* — exit code 3 with the reason, unless you add `--yes`:

- a `DELETE` or `UPDATE` with no `WHERE`;
- a `TRUNCATE`;
- a statement that deletes stored rows along with their table — a `DROP TABLE`, a
  `DROP DATABASE` or `DROP SCHEMA`, a `CREATE OR REPLACE TABLE`.

`--yes` answers the question; it can't lift a refusal. In particular it never lets a write through
on a read-only connection — turn **Read-only** off in the connection's settings if you mean to
write there.

An ordinary write, such as an `UPDATE` with a `WHERE`, runs without asking: typing `exec` is your
say-so. That holds even with **Confirm before running writes** turned on for the editor.

## Running a script

`--script` runs a whole `.sql` file — a migration, the output of
[`compare`](reading.md#compare), or a [`dump`](reading.md#dump) to restore — on one connection,
statement by statement, in order:

```sh
schemaic compare -c staging -d shop --source-connection prod --source-database shop > m.sql
schemaic exec --script -f m.sql -c staging -d shop --yes
schemaic exec --script -c staging -d shop --yes - < m.sql
```

The script comes from `-f`, from stdin with `-`, or as the argument. `-f` takes a pipe as well as
a file, such as `-f /dev/stdin` or `-f <(…)`. It is run as written, so `-p` can't be used with
it, and a script on stdin can't share it with `--password-stdin`. A script given as the argument
that carries a password is warned about on stderr, as a statement is — pass it with `-f` or `-`.

A file or stdin in UTF-16 with its byte-order mark — what Windows PowerShell 5.1's `>` writes — is
read as such, and a byte that isn't valid UTF-8 becomes a replacement character rather than
stopping the run.

Before anything is sent:

- *Every script needs `--yes`*, whatever it holds — it isn't read for what it does, so there is no
  script that runs without the question. Without it, exit code 3.
- *A read-only connection refuses it*, even with `--yes`, even a file of `SELECT`s. So does having
  no database to run in.
- *A script that moves to another database is refused*, and the lines that do it are named: a
  `USE`, psql's `\connect`, or a `CREATE DATABASE` or `DROP DATABASE` naming any database other
  than the one you passed. Run it against the database it names, or remove those lines from a
  copy. A MySQL or MariaDB dump names the database it came from in its `USE` line, so restore it
  into a database of that name.

It stops at the first statement the server refuses. Nothing goes to stdout; stderr says how it
ended:

```text
schemaic: 3 statements ran
```

```text
schemaic: no such table: nope
schemaic: the statement on line 2 failed: INSERT INTO nope VALUES (1);
schemaic: 1 statement ran before it; unless the script holds its own transaction, those are applied
```

The statements before the failure stay applied, unless the script wraps itself in its own
transaction — fix the cause and work out where to pick up, rather than running the whole file
again. A failure exits with code 4 — but a connection Schemaic refuses to open, such as one whose
[init SQL](../connections/safeguards.md#init-sql) it won't run, exits with code 3, as on every
command.

On SQL Server the script is sent batch by batch, split at its `GO` lines (one batch if it has
none), so stderr counts batches: `2 batches ran`. When a batch of several statements fails, stderr
names `the batch starting on line 12`, and statements of that batch, before and after the error,
may have been applied — SQL Server carries on past an error inside a batch.

`--timeout` defaults to an hour with `--script`. A script stopped by it exits with code 5, and the
statements that ran before it may be applied.

It runs plain SQL text. A PostgreSQL archive in `pg_dump`'s custom format isn't one — restore that
with `pg_restore`.

## When it times out

`exec` gives up after `--timeout` seconds (30 by default) and exits with code 5. The statement was
already sent by then, and cancelling it doesn't undo what the server may have done — on an engine
without transactions, part of it may have been applied. Check before running it again.

## The write log

Every write `exec` runs is added to the same write log as the app's own, `write_log.jsonl` in the
settings folder. A script is one line there: the file, how many statements ran, and the one that
failed. Values in its SQL are masked unless **Keep values in the write log** is on in
**Settings** › **Query**.
