# Users and access

**Users and privileges** lists the accounts on a server, shows what each one may do, and lets you
create, change and drop them. As with every other change in Schemaic, nothing runs until you've
seen the SQL.

## Opening it

Open the SCHEMA panel's gear menu, or right-click empty space in the tree, and choose
**Users and privileges**. Accounts belong to the server rather than to any one object, so no
right-click on a table or database leads here.

The database still matters: the browser reads the one your active query tab is on. PostgreSQL
keeps schema and table privileges in each database separately, and a SQL Server user belongs to
one database, so open a tab on the right database first.

SQLite has no user accounts, so there is no such entry on a SQLite connection.

## Reading an account

The left column lists every account, with **Search accounts** above it. Pick one, and the right
side shows its **Attributes** — authentication, locked, connection limit and so on, depending on
the engine — and its **Privileges**, written out as the `GRANT` statements that would recreate
them. **Copy privileges** puts all of them on the clipboard.

Accounts the server maintains itself (`mysql.sys`, `pg_monitor` and the like) are shown faint.
You can read them, but Schemaic won't change or drop them.

A note under the statements says plainly what the list leaves out. The main gaps:

- *MySQL and MariaDB* list what's granted to the account directly, and the roles it's in — not
  what those roles hold. Open the role to see that.
- *PostgreSQL* lists privileges in the current database only, and leaves out `EXECUTE` on
  functions and grants on single columns. A superuser's or a predefined role's powers aren't
  granted, so they don't appear as statements; the note says so instead.
- *SQL Server* shows `DENY` alongside `GRANT`. A login without `VIEW ANY DEFINITION` may be shown
  a short list.

The browser doesn't notice changes made from elsewhere. Press **Refresh** to re-read the server.

## Accounts on each engine

| Engine            | What's listed                                                                   |
|-------------------|---------------------------------------------------------------------------------|
| MySQL and MariaDB | Users and roles. A user is a name *and a host*: `'app'@'%'` and `'app'@'localhost'` are two accounts |
| PostgreSQL        | Roles. A user is a role that can log in                                         |
| SQL Server        | Server logins, plus the current database's users and roles                      |

On SQL Server the two halves do different jobs. The *login* holds the password and signs in to
the server; the database *user* is what gets permissions in a database. A user mapped to a login
of another name reads `name ← login`. With no database selected, only the logins are listed.

On Azure SQL Database the logins live in `master` and can't be managed from a user database, so
the list holds that database's users and roles only.

## Creating an account

Press **New account** at the foot of the list. Choose the **Kind**, give it a **Name**, and fill
in what that kind needs:

- *MySQL and MariaDB* — a **Host**. Left blank it's `%`, meaning any machine on the network, and
  the preview says so.
- *PostgreSQL* — a user takes a password; a role doesn't.
- *SQL Server* — a login takes a password. Leave **Also a user in _database_** ticked to create
  the matching user in the current database in the same step, which is usually what giving someone access
  takes. A user on its own is made **For login** an existing login, or with no login at all if
  you leave that empty.
- *A contained database* (every Azure SQL database is one) — a user can have a password of its
  own, with no login behind it.
- *Microsoft Entra* — when you signed in to SQL Server through Entra, a user can be made from an
  Entra person or group instead. Turn on **Microsoft Entra**; there is then no login and no
  password, since Entra signs it in.

Type the password twice, in **Password** and **Confirm**, then press **Preview SQL**. The note
under the password field tells you whether the preview shows the password itself or, on
PostgreSQL, a hash of it. Either way, what the preview shows is the exact statement that runs.

A MySQL or PostgreSQL user created without a password can sign in with none, and the preview
warns you of it.

## Changing and dropping an account

Pick the account, then use the buttons under its name:

- **Privileges** — grant or revoke; see [Granting and revoking](privileges.md).
- **Reset password** — sets a new one. It isn't offered on a role, or on an account with no
  password of its own to change. The old password can't be recovered, so anything still
  configured with it stops connecting.
- **Drop** — goes straight to the SQL preview. Dropping an account takes every privilege it holds
  with it, and they aren't recorded anywhere else. Sessions already connected as it keep running
  on MySQL and PostgreSQL; on SQL Server they fail at their next statement in that database.
  Dropping a SQL Server login leaves its database users behind, mapped to no login.

These buttons and **New account** are dimmed on a read-only connection, and when no database is
selected — the statement has to run in one. A sentence under the buttons says which.

## In this section

- [Granting and revoking](privileges.md) — give an account privileges or a role, or take them
  away.
