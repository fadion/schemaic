# Query plans

A query plan is the server's own account of how it will run a statement: which indexes it uses,
which tables it reads in full, where it sorts. Schemaic shows it in a table and points out the parts
that are usually slow.

## Opening one

Right-click a statement in the editor and choose **Plan**. The **Query plan** window opens on that
statement with its `:name` parameters filled in, the way a run would send it.

The table is the server's own plan, laid out as the engine returns it — `EXPLAIN` rows on MySQL
and MariaDB, `EXPLAIN QUERY PLAN` on SQLite, the indented plan on PostgreSQL, and on SQL Server
one row per operator with its estimated rows and cost.

## Analyze

By default the plan is an estimate: the statement isn't run. Turn on **Analyze** to run it and see
real row counts and timings instead (`EXPLAIN ANALYZE`, `ANALYZE` on MariaDB, or SQL Server's
actual plan). SQLite has no such form, so there the plan stays an estimate.

Because that runs the statement, **Analyze** is only offered for a statement that reads. For one
that writes, the toggle is replaced by *Analyze unavailable (statement writes data)*. On a read-only
connection, a plan of a statement that writes can be refused outright.

## Potential issues

Under the table, **Potential issues** lists what's worth a look:

- a full table scan, or a sequential scan on PostgreSQL;
- a sort or temporary table the query builds on the fly;
- on SQL Server, the indexes the server says are missing — with the improvement it expects — and
  its own warnings, such as a join with no condition or a sort that spilled to `tempdb`.

These are hints, not verdicts: a full scan of a ten-row table is the right plan.

**Ask AI** sends the statement and its plan to the AI assistant, to explain the plan or suggest a
change. See [AI assistant](../ai/README.md).
