# Safeguards

The **Safeguards** section of the connection form limits what can happen through a connection. Each
setting belongs to one connection, so a scratch database and a production server can be set up
differently.

## Read-only

Turn on **Read-only** for a connection you only mean to look at. Schemaic then refuses to change
anything through it:

- The query editor runs reads only. A write or a change to the structure is refused with
  *Read-only connection* and the reason, and the read itself runs in a session the server holds
  read-only, so a statement that looks like a read but writes is caught too.
- The results grid's cells can't be edited, and rows can't be added or deleted.
- In the schema tree, entries that would change something are greyed out. Where an editor still
  opens, such as the table designer, its SQL preview has no **Apply**: you can read and copy the
  SQL, but not run it.
- Restoring a backup into the connection, copying a table into it, killing sessions and cancelling
  queries in the activity panel are all refused, and the terminal won't open the database's own
  command-line client for it.

*On SQL Server* there's no read-only session to ask for, so Schemaic runs each statement in a
transaction and rolls it back. A procedure that does something outside the database — sends mail,
writes a file — can still have that effect. For a real guarantee, connect with a login that can
only read. A read-only SQL Server connection also can't hold a manual transaction.

Read-only guards against mistakes; it isn't a security boundary. If it matters that nothing can be
written, give the connection a database account that has no write privileges.

## Command-line and AI access

- **CLI access** lets the `schemaic` command line use this connection, and AI agents through
  `schemaic mcp`. It's off for every connection until you turn it on. See
  [Command line](../cli/README.md).
- **AI data access** sets how much of the database's *data* Schemaic's AI assistant may see. Its
  structure — names, types, definitions — is always available to it.

  | Level                    | What leaves your machine                                          |
  |--------------------------|-------------------------------------------------------------------|
  | **Schema only**          | No rows at all, and attaching rows to a question is refused       |
  | **Only what I attach**   | The rows you attach from a result yourself. The default.          |
  | **Let it read data**     | Whatever the assistant reads with its own read-only queries       |

  See [AI assistant](../ai/README.md).

## Statement timeout

**Statement timeout** cancels any statement on this connection that runs longer than the time you
pick. **Same as the app** follows the timeout in **Settings** › **Query**.

## Init SQL

**Init SQL**, in the **Database** section, holds settings to apply every time Schemaic connects — a
search path, a time zone, a role:

```sql
SET search_path TO app, public;
SET TIME ZONE 'Europe/London';
```

On SQLite it takes `PRAGMA` settings instead.

It only accepts session settings Schemaic knows are safe. It refuses anything that isn't a `SET`
(or a `PRAGMA` on SQLite), anything that changes the server rather than the session, and the few
settings Schemaic's own guards rely on: transaction and read-only settings, `autocommit`, MySQL's
`sql_mode`, a character set other than UTF-8, and the like. The line under the box says what's
wrong as you type, and a connection whose init SQL would be refused doesn't connect.
