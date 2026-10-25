# Granting and revoking

Select an account in **Users and privileges** and press **Privileges**. One form works in both
directions: pick **Grant** or **Revoke** under **Action**, describe what you mean, and press
**Preview SQL** to see the statement before it runs.

## Privileges on an object

With **Subject** set to **Privileges**:

1. Choose a **Level** — how far the grant reaches.
2. Name what it applies to. The fields suggest the server's databases, schemas and tables as you
   type.
3. Tick the privileges, and **With grant option** if the account may pass them on.

The form opens on the widest level that isn't the whole server, so a stray click can't hand out
server-wide rights.

| Engine            | Levels                                                                       |
|-------------------|------------------------------------------------------------------------------|
| MySQL and MariaDB | **Whole server**, **Database**, **Table**                                    |
| PostgreSQL        | **Database**, **Schema**, **Table**, **Sequence**                            |
| SQL Server        | A login: **Whole server**. A user or role: **Database**, **Schema**, **Table** |

PostgreSQL has no whole-server level. Its cluster-wide powers — superuser, creating databases,
replication — are attributes of the role rather than privileges, and are changed with
`ALTER ROLE` in the editor.

The lists cover the privileges that act on data and schema. Server-administration privileges
such as MySQL's `SUPER` or its dynamic `…_ADMIN` ones, and SQL Server's rarer permissions, aren't
offered; write those statements in the editor.

A grant on the whole server is spelled out in the preview for what it is: on MySQL, every
database, including ones that don't exist yet; on SQL Server, the instance itself.

## Membership of a role

Set **Subject** to **Role** and name the **Role**; the field suggests the server's roles. Granting
a role gives the account everything the role holds. On MySQL, MariaDB and PostgreSQL,
**With admin option** lets it grant the role on to others; SQL Server has no such option. For a
SQL Server login the suggestions are the fixed server roles, such as `sysadmin` and `dbcreator`.

## Revoking

A revoke takes away exactly what the same form would grant. To take back something you can see
in the privileges list, set the form up to match that line. The preview reminds you that
anything relying on the privilege fails at its next statement.

SQL Server has rules of its own, and the preview names each one when it applies:

- A revoke always carries `CASCADE`, so it also takes the permission from anyone the account
  granted it on to.
- A `DENY` beats any grant. Revoking a permission also lifts a `DENY` of it, which can give access
  *back* if a role the account belongs to grants it.
- A table-level revoke also clears the account's column-level grants and denials of the same
  permission.
- A table-level grant would normally erase column-level denials. Schemaic denies those columns
  again straight afterwards, so a column that was denied stays denied.

## What the form doesn't do

- *`DENY`.* SQL Server's denials show in the privileges list, but the form only grants and
  revokes. Write a `DENY` in the editor.
- *Column-level grants.* Granting on some columns of a table — or, on MySQL and PostgreSQL,
  revoking such a grant — is a statement of your own.
- *Grants on a single routine*, such as `EXECUTE` on one procedure. Write those in the editor too.
