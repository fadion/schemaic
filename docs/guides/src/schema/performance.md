# Performance

The performance view helps you find out why a server is slow: the statements it spends most time
on, the indexes worth a second look, and the tables and indexes taking the most space. It only
reads. Anything it suggests you change goes through the SQL preview first.

## Opening it

Choose **Performance** from the **SCHEMA** panel's gear menu, from a right-click on the tree's
empty space, or from the command palette. It
opens on the active connection and the database chosen in the query toolbar, both named in the
title.

**Statements** works without a database. **Indexes** and **Sizes** are about one database, so
choose one in the query toolbar first.

## Statements

The statements the server has spent most time on, ranked by **Total time**, **Mean time** or
**Calls**. Each row shows the statement with its total and mean time, calls, rows and database.
Right-click a row and choose **Copy statement** to take it into a query tab. A line at the end
says when the counting started: every figure is since then.

The figures come from a source that has to be turned on in the server. When it's off, the view
says which source and why. Where one statement turns it on, the view shows it with a **Copy**
button; run it in a query tab yourself, since Schemaic never runs it for you.
[Performance tools](../cli/mcp.md#performance-tools) lists each engine's source and what turning
it on takes. SQLite keeps no statement statistics, and the view says so.

## Indexes

Three kinds of index are listed, each with its size:

- indexes not read since the server's counters were last reset;
- exact duplicates of another index;
- indexes whose key is the start of another's, so the longer one already covers them.

Usage counts start again when the server restarts or its statistics are reset, so an index used
only by a monthly job can look unused. Check before dropping one. If the server keeps no usage
counts, the view says so and lists only duplicates and prefixes.

On SQL Server a second list, **Missing indexes the optimiser recorded since the last restart**,
shows the indexes the optimiser wished it had, with how often and how much it expected to gain.
Each is one estimate: check it against the indexes above it before creating one.

**Drop** on a finding, and **Create index** on a missing index, open the SQL preview, where you
read the statement and apply it. Both appear only for tables the schema tree has loaded for this
connection. If one is missing, expand the database in the tree, then press **Refresh** in the
view.

## Sizes

**Largest tables** lists tables by size, with an estimate of their rows. **Largest indexes** lists
indexes by size. MySQL and MariaDB report index size per table rather than per index, so there
that list says so instead.

## Keeping it current

The view reads the server when it opens and when you switch tabs or ranking. Press **Refresh** to
read it again.
