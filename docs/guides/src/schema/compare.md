# Comparing schemas

Schema compare shows what differs between two databases — on the same server or on different ones
— and can generate the migration that brings one in line with the other.

## Opening it

Right-click a database in the schema tree and choose **Compare with**. Then pick the other side:
first a connection, then a database on it.

Choosing **Compare with** on a single table or view opens the same comparison, filtered to that
object.

## Which way it goes

The database you right-clicked is the one that will change. The one you picked is the *source of
truth*. The header shows this with an arrow, so check it before applying anything:

- an object only on the source-of-truth side will be created;
- an object only on the side that changes will be dropped;
- an object on both sides that differs will be altered to match.

## Reviewing the differences

The left rail lists every object grouped by kind — tables, views, triggers, routines, sequences
and more, as the engine has them — with a tally of how many differ, exist on one side only, or are
identical. Select an object to see both sides' definitions next to each other. **Filter by name**
narrows the list, and **Include identical** shows the objects that match as well.

Every difference that can be applied starts ticked. Untick anything you want to leave alone.

## Applying

Press **Preview migration**. The comparison closes and the SQL for the ticked objects opens in the
same preview as the [table designer](table-designer.md): **Apply**, **Copy** or **Open in editor**.
Objects you left unticked are listed there too, so it's clear what the migration leaves as it is.

## Limits

- Both databases must be on the same kind of engine. MySQL and MariaDB compare with each other;
  any other mix can't be compared. Two DuckDB files can be compared: each is its own connection,
  with one database to pick.
- On MySQL and MariaDB, a trigger, routine or event body is shown but can't be ticked: the server
  has to give its definition again before Schemaic can recreate it safely.
- A few differences are shown but held back from the migration, each with a note saying why — for
  example a table whose partitioning differs.
- Both databases are read in full every time, so comparing a large server takes a moment.

On DuckDB, the whole migration runs in one transaction, and a few things stop it:

- *A sequence that differs* gets no statement, because DuckDB can't alter one. Sequences that exist
  on one side only are created or dropped.
- *A macro with parameter defaults* (`b := 2`) on the source-of-truth side is held back by the MCP
  server's `compare_schemas`. DuckDB's catalogue doesn't hold the defaults, and Schemaic reads them
  from DuckDB's own export, which needs file access the MCP server turns off. **Compare with**
  plans such a macro like any other.
- *A view or macro that names a type you made with `CREATE TYPE`*, when the other database
  doesn't have that type, is held back with a note naming it: the comparison doesn't create types.
  Create the type there by hand, then compare again.
- *A table another table's foreign key points at can't be rebuilt.* The preview lists it under
  **This engine can't express part of this plan**, and **Apply** stays off until you leave that
  table unticked.
- *A sequence a column default uses can't be dropped.* DuckDB refuses, and the whole migration
  rolls back.
