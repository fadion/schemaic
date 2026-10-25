# Running writes

`exec` is the one command that changes data. It's a separate command rather than a flag on
`query`, so a write is never one typo away from a read.

```sh
schemaic exec "UPDATE orders SET state = 'sent' WHERE id = 7" -c prod -d shop
schemaic exec - -c prod -d shop < change.sql
```

It takes one statement, given [the same three ways](reading.md#giving-the-statement) as `query`.
Several statements are refused rather than half-run — run them one at a time, so a failure
part-way isn't something you have to piece back together.

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

## When it times out

`exec` gives up after `--timeout` seconds (30 by default) and exits with code 5. The statement was
already sent by then, and cancelling it doesn't undo what the server may have done — on an engine
without transactions, part of it may have been applied. Check before running it again.

## The write log

Every write `exec` runs is added to the same write log as the app's own, `write_log.jsonl` in the
settings folder. Values in its SQL are masked unless **Keep values in the write log** is on in
**Settings** › **Query**.
