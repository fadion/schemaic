# Performance

The performance view helps you find out why a server is slow: the statements it spends most time
on, the indexes worth a second look, and the tables and indexes taking the most space. It only
reads. Anything it suggests you change goes through the SQL preview first.

## Opening it

Choose **Performance** from the **SCHEMA** panel's gear menu, from a right-click on the tree's
empty space, or from the command palette. It
opens on the active connection and the database chosen in the query toolbar, both named in the
title. If the tab you're in is on another connection, it opens with no database, since that
tab's database belongs to the other server.

**Statements** works without a database. **Indexes** and **Sizes** are about one database, so
choose one in the query toolbar first.

## Statements

The statements the server has spent most time on, ranked by **Total time**, **Mean time** or
**Calls**. Each row shows the statement with its total and mean time, calls, rows and database.
Right-click a row and choose **Copy statement** to take it into a query tab. A line at the end
says when the counting started: every figure is since then.

A statement that sets a password, such as `ALTER ROLE app PASSWORD '…'`, keeps its figures but
not its text: the row says it was withheld and has no **Copy statement**. `schemaic statements`
and the AI's `top_statements` tool withhold it the same way, so the password never reaches the AI
provider.

The figures come from a source that has to be turned on in the server. When it's off, the view
says which source and why. Where one statement turns it on, the view shows it with a copy icon
beside it (**Copy**); run it in a query tab yourself, since Schemaic never runs it for you.
[Performance tools](../cli/mcp.md#performance-tools) lists each engine's source and what turning
it on takes. SQLite and DuckDB keep no statement statistics, and the view says so.

## Indexes

Three kinds of index are listed, each with its size:

- indexes not read since the server's counters were last reset;
- exact duplicates of another index;
- indexes whose key is the start of another's, so the longer one already covers them.

Usage counts start again when the server restarts or its statistics are reset, so an index used
only by a monthly job can look unused. Check before dropping one. Where the server says when its
counts began, the view gives that date; on a PostgreSQL primary with standbys it also says their
reads are not counted, and that dropping an index drops it on them too. If the server keeps no
usage counts, the view says so and lists only duplicates and prefixes.

Where two indexes overlap, the one listed is the one you could drop:

- Of two identical indexes, the primary key, a clustered, unique or foreign-key index is kept
  over a plain one, and otherwise the one queries read.
- An index queries read is never called redundant beside a wider one nothing reads. The wider one
  is listed as unused instead.
- An index a finding names as the one to keep is never listed as unused itself, so dropping
  everything listed never takes both of a pair.

On SQL Server the clustered index is never listed: it is the table itself, in key order. An
unread index is not listed as unused when the server would refuse to drop it: the last index a
foreign key can use (on MySQL and MariaDB, any index that starts with the key's columns), or a
PostgreSQL partition's copy of an index on its partitioned table.

On SQL Server a second list, **Missing indexes the optimiser recorded since the last restart**,
shows the indexes the optimiser wished it had, with how often and how much it expected to gain.
Each is one estimate: check it against the indexes above it before creating one.

The bin icon on a finding (**Drop index**), and the plus icon on a missing index
(**Create index**), open the SQL preview, where you
read the statement and apply it. Both appear only for tables the schema tree has loaded for this
connection, and **Drop index** only for an index the tree already shows. If one is missing,
expand the database in the tree (or refresh it, for an index made since), then press **Refresh**
in the view.

## Sizes

**Largest tables** lists tables by size, with an estimate of their rows. **Largest indexes** lists
indexes by size. MySQL and MariaDB report index size per table rather than per index, so there
that list says so instead, even when every table has only a primary key. DuckDB reports no sizes
at all, only an estimate of each table's rows, and the view says so.

## Keeping it current

The view reads the server when it opens and when you switch tabs or ranking. Press **Refresh** to
read it again.

## From the command line

`schemaic statements`, `indexes` and `sizes` print the same lists in a terminal or a script — see
[Looking into a slow server](../cli/reading.md#looking-into-a-slow-server).
