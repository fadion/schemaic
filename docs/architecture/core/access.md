# `core/access.rs`

`access.rs` — **what a dump's replay drops besides the objects, and the statements that put it
back**: `ReplayAccess { tables, routines }`, on `DbSchema::replay_access`, each half `None`
where it was not read. It is read by the dump runner alone, for a file that drops
(`Db::fetch_replay_access`, called from `cli::dump::run` when Structure and *Drop before create*
are both on), and never by `Db::fetch_schema`, whose every other caller would pay four
catalogue queries for it; a read that fails leaves the half unread rather than failing the
export, and the header says so. **Three engines lose something to a replay's `DROP`, and
`drop_resets_table_access(dialect)` is the table half's capability**: PostgreSQL's recreated
table belonged to whoever replayed, with the default ACL, row security off and no policies, so
an application role lost `SELECT` — or kept it through `PUBLIC` past the policies that had
limited it — under a success report (S1.1-L5-03); SQL Server's held no permission and belonged
to its schema's owner; MySQL keeps a dropped table's privileges in `mysql.tables_priv`, and they
apply again to the table recreated under its name, so it answers `false`, as SQLite, with no
grants, does. **`TableAccess::restate_sql(qname, keyword, dialect)`** is PostgreSQL's
`ALTER … OWNER TO`, then — where `relacl` was not `NULL` (`default_acl`) — `REVOKE ALL` from
`PUBLIC` and the owner and every grant again, as `pg_dump` writes it, so a privilege the owner
gave up stays given up (`an_empty_explicit_acl_still_revokes`); the column grants, each
privilege naming its columns; `ENABLE`/`FORCE ROW LEVEL SECURITY`; and each `CREATE POLICY`
with its kind, command (`pg_policy_command`), roles and expressions
(`an_explicit_acl_is_revoked_and_granted_back_by_grantee`,
`column_grants_are_granted_back_per_grantee`, `row_security_and_policies_are_restated`). On
SQL Server it is `ALTER AUTHORIZATION ON OBJECT::` where the owner is not the schema's, then
each `GRANT`, grant option, `DENY` and column `REVOKE` (`GrantState`, `sys.database_permissions`'
`state`) by grantee — nothing revoked first, since a new object holds no permission
(`sql_server_permissions_are_granted_denied_and_revoked_back`). **The routine half is MySQL
8's**: a routine has no `CREATE OR REPLACE` there, and its `DROP` revokes every account's grant
on it (`automatic_sp_privileges`), so the application's next call was *execute command denied*
(S1.1-L5-02, partial until this). `RoutineGrant::grant_sql` gives an account back what
`mysql.procs_priv` held — `EXECUTE`, `ALTER ROUTINE`, `Grant` as `WITH GRANT OPTION`, `USAGE`
where the option was all — on the unqualified name, beside the `CREATE` that made it in the
replay's database (`a_routine_grant_is_granted_back_as_held`); `ReplayAccess::routine_grants`
matches a routine's name ignoring case, as MySQL does. MySQL publishes routine privileges
nowhere in `information_schema`, so an account without `SELECT` on `mysql` reads `None`, and
the header says it could not grant them back. **Read at dump time, so a replay onto a server
without one of the roles or accounts stops at the statement naming it** — the cost the user
chose over refusing such replays, and the one `pg_dump` without `--no-acl` has. Where `plan`
writes them is under `dump.rs`.
