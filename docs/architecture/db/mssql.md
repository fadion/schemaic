# `db/mssql.rs`

**`mssql.rs` is the fourth engine — SQL Server over TDS, through `tiberius` 0.13.** What it does:
connect (following **one** routing redirect, which is how Azure SQL's
gateway hands a client to the node that serves its database), `ping`, the database list,
`fetch_query` and `run_batch` (with `Enforce`, above), `prepare_check`, `fetch_table_list` and
`fetch_schema`, the monitor's `fetch_table`, `count_rows` (`COUNT_BIG`, through `fetch_query`), table statistics from
`sys.dm_db_partition_stats`, Server Activity, `run_script`, and the grid's write-back —
`commit_writes`, `refetch_rows` and `fetch_blob` (below) — `import_rows` (below), and `run_ddl`,
for the table changes `supports_change` admits (below), a Manual tab's pinned `Session`
(below), `explain`, the estimated and the measured plan (below), and the account browser's
`fetch_principals` and `fetch_grants` (below), and `run_server_ddl`, a database's create and
drop, attached to `master` and untransacted (under `lib.rs`'s `run_server_ddl`, above). That was
the last entry point to be written, and the `not_yet` helper went with it: nothing on SQL Server
answers `DbError::Refused("… is not available for SQL Server yet.")` any more. **What it does
not do yet is named here rather than left to a label**: the engine was called a preview until
the rest was written, and the word had come to read as half-built, which it is not. Containers
are no longer on this list. A schema's create and drop fell to `tsql_supports`'s catch-all
`false`, which also withheld whole any comparison into a database lacking a schema other than
the four every database has, until `emit_mssql` wrote them; a database's were refused by name
there, ahead of `run_server_ddl`'s refusal, until that was written (both under `ddl.rs`; live,
`a_database_is_created_with_its_collation_and_dropped` on 2022 and 2025, which skips on Azure
SQL Database — a created database there is a new billable one — so `master` taking the two
statements on Azure is the code's word, not a measurement). A selective XML index and a
columnstore one with an `ORDER (…)` were the last index kinds on it, read as `IndexInfo::lossy`
rather than authored, and are not now: each is read whole, restated and authored from the
designer (below). What is still read `lossy` — an index whose key names a graph table's
internal column, or one whose reading T-SQL does not spell — a rebuild and a view edit refuse
to touch rather than drop (`tsql_rebuild_refusals`, `lossy_view_index_refusal`). XML and spatial indexes were on this
list — first not read at all, until the listing's join was fixed, then read only as lossy — and
are not now: a primary or secondary XML index and a spatial one are read with every option,
restated and authored from the designer (below). A columnstore index was on this list, and is
not now: it is read, restated and authored from the designer (below). An index with included
columns was on it too, and is not now: they are read and restated (below). So was a named
instance's port, which had to be given
by hand and is now SQL Server Browser's answer (below). So were sequences, alias types, synonyms
and XML schema collections, read into `TsqlObject` for the dump (under `dump.rs`), which are
now browsed in the tree and dropped from their rows (under `schema.rs` and `ddl.rs`), created
and edited in the object editor (`TsqlObjectDraft`, under `ddl.rs` and `object_editor.rs`) and
compared (under `compare.rs`; live, `a_comparison_carries_standalone_objects_in_their_order` on
2022 and 2025). **The refusals are the backstop,
not the gate**: the app is kept off them by
capabilities, each an exhaustive `match` with `MsSql` on `false`, asked at the UI site that
offers the thing — chief among them
`ddl::supports_change`, whose SQL Server answer comes before anything else and is
`tsql_supports`: a table's own changes, new or existing, the table, view and routine drops, a
view's `CreateView` and `ReplaceView`, a trigger's create, replace and drop, a routine's
`CreateRoutine` and `ReplaceRoutine`, a namespace's `CreateSchema` and `DropSchema`, a
database's `CreateDatabase` and `DropDatabase`, the shared `DropObject` for a sequence, synonym,
alias type or XML schema collection and `RenameObject` for the first three, the object editor's
`CreateTsqlObject`, `ReplaceTsqlObject` and `AlterTsqlSequence`, and the seven account changes,
through the same
`account_change_supported` every engine's answer goes through (under `ddl.rs`) — and nothing
more. The four editor predicates compute
from it, so it decides which editors open: `supports_table_design` probes a column retype, which
T-SQL's `ALTER COLUMN` writes, so the designer opens on an existing table — in place, as on MySQL
and PostgreSQL, except for the edits `ALTER COLUMN` cannot make, which rebuild the table
(`ddl::tsql_rebuild_sql`, under `ddl.rs`) — and `supports_view_editing` probes the create and the replace,
which `emit_mssql` writes through the shared `view_statements` (`CREATE OR ALTER VIEW`, under
`ddl.rs`), so the view editor opens too; `supports_trigger_editing` probes all three trigger
changes, which `emit_mssql` writes through the shared `trigger_statements` (`CREATE OR ALTER
TRIGGER` for a same-name edit, under `ddl.rs`), so the trigger editor opens as well; and
`supports_routine_editing` probes a routine's create, replace and drop, which `emit_mssql` writes
through the shared `routine_statements` (`CREATE OR ALTER PROCEDURE`/`FUNCTION`, under `ddl.rs`),
so the routine editor opens last of the four
(`sql_server_admits_the_table_changes_it_can_write`, `sql_server_offers_the_trigger_editor`,
`sql_server_offers_the_routine_editor`).
What `tsql_supports`
refuses is under `run_ddl` (below). The three drops needed no emitter
change — `DROP TABLE`/`DROP VIEW` over `export::ident_sql`'s brackets were T-SQL already, and
`RoutineInfo::signature_sql` already had an `MsSql` arm, the bare name, T-SQL having no
overloading (`sql_server_drops_read_as_t_sql`); `CREATE TABLE` did, under `ddl.rs` and
`schema.rs`, and so did every edit of an existing table. Its early answer is an
exhaustive `match` of its own ahead of the change arms; it was an `== SqlDialect::MsSql` guard,
the one exception to "exhaustive" here, and a `match` makes the next engine say which side of it
it is on. It turned off only what asked it, and three schema-tree entries asked nothing — the
table/view **Drop**, a standalone object's **Drop**, and **Create ▸ Table**, under a comment
saying every engine can create a table — so while it answered `false` to everything each opened a
confirm and a DDL preview whose Apply could only end in `run_ddl`'s refusal. Each now asks
`supports_change` of the statement it would really build, as Truncate already did:
`ObjectEntries::drop` asks it of `overlays::object_drop_change` (`stats::drop_change`), the one `Change` the menu action
also sends; an object row asks `overlays::object_drop_offered`, over the set `ddl::drop_item`
builds — the set the entry opens — and wants it non-empty as well as expressible; and
`create_children`'s Table entry asks it of `Change::CreateTable`. **All three are offered on SQL
Server now, because their statements emit** — Drop on a table or a view but not a materialized
one, which the engine does not have; Drop on a routine row, and — now that `emit_mssql` writes
`DROP SYNONYM`/`TYPE`/`XML SCHEMA COLLECTION`/`SEQUENCE` — on a sequence, synonym, alias type or
XML schema collection row; and *Table* in the Create menu. A table is offered **Edit table** and
**Truncate** besides, now that `emit_mssql` writes them, and **Import** now that `import_rows`
is written; a view **Edit view** with its Drop, and
the Create menu *View* after *Table*, now that it writes a view's create and replace; and both a
table and a plain view **Triggers**, now that it writes `CREATE OR ALTER TRIGGER`, a view's
`INSTEAD OF` being how one is written to at all; and the Create menu *Function* and *Procedure*,
now that it writes `CREATE OR ALTER PROCEDURE`/`FUNCTION` — the entries `create_children` gates
on `supports_routine_editing`, so nothing in the menu changed but the test's expected list; and
the Create menu *Schema* and a schema node's **Drop**, both gated on
`ddl::supports_namespace_editing`, now that it writes `CREATE`/`DROP SCHEMA` — the same: the
predicate computes from `supports_change`, so only the expected lists moved; then the
Create menu *Database*, the gear's and the blank space's *Create database* and a database
node's **Drop**, all gated on `ddl::supports_database_editing`, now that `run_server_ddl` runs
`CREATE`/`DROP DATABASE` — the same again; and last the Create menu's *Sequence*, *Synonym*,
*Alias type* and *XML schema collection*, each of those folders' own Create entry and each such
row's **Edit**, now that `emit_mssql` writes the object editor's plans — the Creates gated on
`ddl::supports_object_creation`, which computes from the plan the blank form raises
(`object_menu_tests::sql_server_offers_its_table_changes_and_a_views_drop`,
`a_standalone_objects_drop_is_offered_only_where_its_statement_emits`,
`create_menu_tests::sql_server_is_offered_what_it_emits`,
`only_the_engines_with_namespaces_are_offered_one`). Unlike SQLite's gaps,
the capabilities that said no for want of code were **unfinished work**, not statements about the
engine; the noes that *are* about it — the materialized view above, a read-only connection's
Manual mode and *Cancel query* below — say so where they stand. `edit::supports_grid_writes`
was one of them and is the first to have come back: asked inside `analyze_edit`, it kept every
cell unwritable until the write-back below landed, and it answers `true` for all four engines now
— kept an exhaustive `match` rather than deleted, so the next engine, arriving without a
write-back, has to say which side of it it is on. `import::supports_import` is the second, on
the same terms, since `import_rows` landed, and `tx::supports_manual_mode` the third, since the
pinned session below was written — though a *read-only* SQL Server connection is still not
offered the mode (`tx::offers_manual_mode`), and that one is about the engine.
`plan::supports_plan` is the fourth, since `explain` was written (below), and
`dump::supports_dump` — the tree's *Export ▸ SQL* — the fifth, once `core::dump` carried an
identity's values under `IDENTITY_INSERT` and closed every batch with `GO` (under `dump.rs`);
its live pin is `a_dump_restores_into_an_empty_database`. `users::supports_users` is the sixth,
once the browser could list SQL Server's two catalogues as one — logins, and the browser
database's users linked to them (under `core::users`). It had said no because the one list of
`(name, kind)` rows fitted neither half; `PrincipalKind::Login` and `Principal::login` are what
made it fit both. `mssql::fetch_principals` reads `LOGIN_LISTING` (SQL and Windows logins and
groups, the `##…##` certificate logins left out, each login's server roles through
`STRING_AGG`) and, with a database, `USER_LISTING` (that database's users and roles, each user's
login as `SUSER_SNAME(sid)`, its role memberships); with no database it lists the logins alone
and its note says to pick one. **On Azure SQL Database (`EngineEdition` 5, `AZURE_SQL_DATABASE`)
it reads `AZURE_USER_LISTING` instead**, because Azure refuses `SUSER_SNAME` a parameter (Msg
40507) and the whole accounts listing failed there with it; that query finds the login by
joining `sys.server_principals` on the SID. It is not the one query for both on purpose: on a
server `SUSER_SNAME` also names a Windows user who has no login row of their own, which the join
cannot. **There it also skips `LOGIN_LISTING` altogether** and sets
`AccountScope::logins_elsewhere`, with a note in place of the no-database one ("Azure SQL
Database keeps its logins in master, and they are not managed here…"): from a user database
`sys.server_principals` shows only the `##MS_…##` server roles and no login, and nothing the
account form could do to one would run (the measurements, and the decision, are under
`core::users`) — so none is listed rather than rows no form could act on. The edition is the
listing's first read for that reason. With a database it also reads that database's
`sys.databases.containment` (`WHERE database_id = DB_ID()`) into `AccountScope::contained`,
which is what puts a password row on the New account form's User (under `core::users`) — except
on Azure SQL Database, where `contained` is simply `true`: its `containment` reads 0, yet it
accepts a user with a password (Msg 33233 is what a genuinely uncontained database answers).
`AccountScope::entra_users` is `db.auth == AuthMode::AzureCli` — read off the connection, not the
server, and on every edition. **It also asks `HAS_PERMS_BY_NAME(…, 'VIEW ANY DEFINITION')`**,
because a login without that permission is shown only the principals it may see — itself and
little else — with no error to say so, which is the MySQL ladder's "one account out of eight"
in another catalogue; the note says the list may be short. The listing's note is one
`PrincipalsNote::Ms { listed, short }` — `MsListed` for which principals it holds, `short` for
that permission's absence — rather than those sentences joined inline. `mssql::fetch_grants` reads a login's
server permissions and server roles on a connection to no database, and a user's or role's
database permissions and roles in the browser's database, noting when there is none
(`GrantsNote::MsPickDatabase`); both fold
through `users::mssql_grant_statements`, and both read **every class**: `SERVER_PERMISSIONS`
names a login, server role or endpoint securable, `DATABASE_PERMISSIONS` a schema-scoped one
through `OBJECT_SCHEMA_NAME`/`OBJECT_NAME` (which name master's `sys.xp_cmdshell` too) and the
rest through one `LEFT JOIN` per catalogue view, each name `COLLATE DATABASE_DEFAULT` since the
views disagree (Msg 451 in the `CASE` otherwise); what comes back unnamed is
`mssql_unshown_note`'s. `every_class_of_permission_a_principal_holds_is_listed` pins it, on 2022,
2025 and Azure SQL Database (the database half; the login half needs a server): permissions on
a user, a role, a type, an XML schema collection and a column read back, **replaying them for
another user reproduces the same list** — so the sentences are statements the server takes —
and a login's `IMPERSONATE ON LOGIN::sa`, `CONTROL ON LOGIN::` and `CONNECT ON ENDPOINT::` are
listed. The live pin is
`a_login_and_its_user_are_created_granted_reset_and_dropped`, through the real
`ChangeSet::emit` → `Db::run_ddl` path: one plan creates the login and its user, which are listed
linked; the login signs in; a grantable schema grant and a `db_datareader` membership read back as
the sentences that made them, and the login's own `GRANT CONNECT SQL`; the schema grant is revoked,
the revoke `CASCADE` makes possible; the login's `VIEW SERVER STATE` and its `dbcreator`
membership are granted and revoked in one plan each from the scratch database, the path that
needs `master`; a reset on the **user** row
changes the login's password, the old one then refused; and the drops leave neither behind.
`ScratchLogin` drops the login on the way out, a login being the server's and outliving the
scratch database. `a_contained_user_is_created_signs_in_and_is_reset` is the contained half: it
makes the scratch database `CONTAINMENT = PARTIAL`, sees the listing report it contained, creates
a user with its own password through the same plan path, finds it listed with no login and
`database_password` set, signs in with it, resets it on its row, and is then refused with the
old password and let in with the new. **It only reads the server-wide `contained database
authentication` setting**, never sets it — it is the server's, not the scratch database's — and
reports the leg a no-op through
`endpoint::note_leg_no_op` when it is off. CI's live job turns it on before the tests run, in its
*Enable contained database authentication* step (`sp_configure … 1; RECONFIGURE` through `docker
exec` on each throwaway service container, 2022's and 2025's), so there the round trip runs rather than reporting
itself skipped. **No login is created on Azure SQL Database, and that is decided rather than
unfinished**: `CREATE LOGIN` runs there only in `master` (Msg 5001) while the account plan runs
in the user database, and even `master` cannot read or write a login's server permissions, so the
form offers no Login there (under `core::users`) and the login round trip reports itself a no-op
under `mssql-on-azure` (in the live tier, below) rather than pinning a path that fails.
`the_account_scope_is_read_with_the_list` pins the scope: on a server neither `logins_elsewhere`
nor `entra_users` is set and a login is listed; on Azure both are set, no login is listed, the
note names `master`, and an Entra user for a name Entra does not know is refused as a principal
that "could not be found" — the statement taken as one rather than failing as syntax.
Server Activity is the one
split that *is* about the engine: `KILL` ends a session, but no T-SQL statement cancels another
session's request and leaves the session standing — a cancel is an attention sent by the owner's
own client — so `activity::supports_kill_kind` says no to *Cancel query* there and
`kill_session` refuses it rather than performing a `KILL` under a label that promises less.
The panel's row menu asks the same predicate (`activity_panel::row_menu`), so on SQL Server
the entry is absent rather than dimmed; the refusal is the backstop. The activity query drops its
own session (`session_id <> @@SPID`), as PostgreSQL's `pg_backend_pid()` and MySQL's
`CONNECTION_ID()` do — without it every refresh listed the poll as a running session with a
*Kill session* under it.
**It signs in three ways, and `auth_method` is the one place that decides which.** `Db` carries
an `auth`, set by `Db::connect` from `Connection::effective_auth`, by `Db::with_auth` — for the MCP
subprocess, and asked through the engine there too by the same `AuthMode::in_force`, so an
endpoint cannot carry a mode its engine does not offer — and read back by `Db::auth()`; `from_parts` is a password. `Password` is
`AuthMethod::sql_server`. `Windows` is `AuthMethod::Integrated`, SSPI through the OS, which is
tiberius's `winauth` feature — enabled for the Windows target alone in this crate's
`Cargo.toml`, since it is pure-Rust bindings there and the Unix counterpart would be GSSAPI over
`libkrb5`, a system dependency for every Linux build; elsewhere the arm is a refusal, never reached
because `AuthMode::offered` does not offer the mode. `AzureCli` is `AuthMethod::aad_token` over
`entra::sql_token()`, the login's FedAuth extension in stock tiberius — none of the vendored
patches is about it. That is why `config` is `async` now. The two cloud IAM modes are an arm that
refuses (`DbError::Refused`) — PostgreSQL's and the MySQL family's, never in force here, so it is
for a hand-made `Db`. **`config` refuses an Entra sign-in
whose plan does not reach its TLS floor, and does it before the token is fetched**
(`AuthMode::transport_refusal` → `DbError::Refused`). A `Db` resolved from a connection already
carries the `VerifyFull` plan `Connection::tls_plan` raises it to (`connection.rs`), so this is
for a handle assembled from parts, which carries only the plan it was handed: with none, the
Azure CLI is never asked and nothing is dialled
(`an_entra_handle_without_a_verifying_plan_is_refused_before_the_token`). The raise is verified
live on the Azure SQL test bed — an Entra connection left at `Disable` signs in over a verifying
plan with `encrypt_option` `TRUE` (`an_entra_connection_left_at_disable_signs_in_verified`, the
opt-in `mssql-azure` leg). **A refused Entra login forgets its
token** (Msg 18456 → `entra::forget`), so the next connection asks the CLI again rather than
handing over one the server will not take — the user signed in to the CLI as someone else since,
or was removed — until it expired.
**An Azure SQL host gets a minute to sign in, not fifteen seconds.** `is_azure_sql` tells one by
its name — `*.database.windows.net` and the two sovereign clouds' spellings, whatever the case or
a trailing dot, and not a host that merely contains the suffix (`an_azure_sql_host_is_told_by_its_name`)
— and `config` raises tiberius's handshake bound to `AZURE_HANDSHAKE` (60 s) for it. **A
serverless database that has paused holds the login at the gateway while it resumes**: measured
on a free-offer database in Sweden Central (auto-pause after an hour), the first login after an
idle spell outlasted fifteen seconds and failed with a bare handshake timeout, and a retry a
minute later went straight through. A timeout there is reported as `azure_timeout` (the
`ConnectFailure::AzureTimedOut` the interface words) — the
likely reason, and that this attempt has woken the database, so connect again shortly — rather
than as an I/O error that names neither. **A waking database has a second answer**, seen on the
same database minutes later: the gateway refuses at once with Msg 40613, *Database … is not
currently available*, rather than holding the login — so that code on an Azure host keeps the
server's words and gains the same sentence (`azure_unavailable`, `AzureUnavailable`). The bound is on the whole
config, so the one routing redirect inherits it.
**A named instance is reached through SQL Server Browser**, as the Microsoft drivers reach one: a
host written `server\INSTANCE` (`connection::sql_server_instance`) is asked of the Browser on UDP
1434 for the instance's TCP port, through tiberius's `sql-browser-tokio` — upstream's own
feature, switched on in the workspace `Cargo.toml` rather than patched, and bringing no crate
the tree lacked. `config` sets `host(server)` and `instance_name(instance)` and **deliberately
no `port`**: tiberius sends the Browser's question to the configured port when there is one, so
setting the saved port would ask the wrong one and hear nothing. The host is the bare server
because DNS, the login packet and the certificate's name check all want it without the
instance. **`config` takes the host apart through `connection::sql_server_address`**, so with no
instance it sets the server and the `dialled` port — the inline one winning over the saved one,
`tcp:` and `(local)` resolved. Before that, `db\SQLEXPRESS,1500` asked the Browser for an
instance named `SQLEXPRESS,1500` and `(local)\X` reached DNS verbatim (S5-L1-02). Live,
`a_host_is_read_as_microsofts_clients_read_it` connects `127.0.0.1,1433`,
`127.0.0.1\NOSUCH,1433`, `tcp:127.0.0.1,1433` and `(local),1433` with a saved port of 1, so the
saved one cannot be what is dialled (passed on 2022, red with the change stashed: *No such host
is known*); it skips on Azure and on any host but `127.0.0.1`. `connect_with(cfg, named)` opens the socket through
`<TcpStream as tiberius::SqlBrowser>::connect_named` and then **writes the port the Browser
named back into the configuration** (`cfg.port(tcp.peer_addr()?.port())`): Windows sign-in
names the service it signs in to as `MSSQLSvc/host:port`, built from the configuration's port,
which would otherwise be the Browser's 1434. That is reasoned from tiberius's source, not
measured — there is no Windows named instance to try it on — so a password sign-in working
without the line is no evidence it can go. A routed reconnect (Azure's gateway) passes
`named = false`, the redirect naming a host and a port of its own. **The Browser's silence is
told in words that say what it means** (`browser_silent`, `ConnectFailure::BrowserSilent`): tiberius gives up on it after a
second with an `Error::Conversion` mentioning the browser, and the Browser answers nothing
alike whether the service is not running, UDP 1434 is blocked on the way, or no instance there
has that name — so the text names all three, and the way round them: the bare host and the port
the instance listens on (`a_silent_browser_names_the_instance_and_the_way_round`). The match is
on the two shapes silence takes — the driver's `SQL browser timeout …` and the reset mapped
below to `SQL browser gave no answer …` — so a re-vendor that rewords the driver's loses the
sentence and falls back to the driver's own. **It once matched any message naming the
browser**, which took in the driver's *Could not resolve SQL browser instance* — not silence
but an **answer without a TCP port**: a reply carries `tcp;<port>` only while the instance
listens on TCP (MS-SQLR), so an instance with TCP/IP off, SQL Express's install default, was
reported as a Browser not running, UDP blocked or a wrong name, all three false (S5-L1-01). That
answer is `is_browser_without_tcp` now, told by `browser_without_tcp` (`BrowserWithoutTcp`): the Browser answered,
the instance does not listen on TCP, enable TCP/IP for it in SQL Server Configuration Manager
(`a_browser_answer_without_a_tcp_port_is_not_its_silence`). **On Windows the silence arrives as a reset, not a
timeout**: with nothing listening on UDP 1434 the host answers the query with an ICMP
port-unreachable, which Windows hands the next `recv` as `WSAECONNRESET` (os error 10054) at
once, so tiberius surfaced `Error::Io { kind: ConnectionReset }` and the connection form showed
the driver's *An existing connection was forcibly closed by the remote host* in place of the
sentence (seen on `127.0.0.1\SQLEXPRESS`, by 7f728746's hand check). `connect_with` maps the
error of `connect_named` through `browser_silence`, which rewrites that reset into the timeout's
shape — a `Conversion` whose message names the browser — and `connect` asks `is_browser_silence`
of the result, where it had matched the `Conversion` inline. Reading a reset as the Browser is
sound only because of where the mapping sits: that step holds just the lookup and the TCP
connect to the port it answered, and a refused TCP connect is `ConnectionRefused`; the login
after it is outside the `map_err`, so a reset there keeps the driver's words. The word
`browser` in the rewritten message is what `is_browser_silence` reads, so it is load-bearing
for the same reason the driver's is (`a_reset_browser_query_is_the_browsers_silence`, which
composes the two functions rather than testing either alone, and was red before the mapping).
**That login is awaited `Box::pin`ned**, which a tidy-up would undo: tiberius's
`Client::connect` — prelogin, TLS, the handshake — is a ~20 KiB future, and inline it rode in
every SQL Server future, measured in a debug build at 22,896 bytes for `connect` and 24,448 for
`fetch_query`, against PostgreSQL's `fetch_query` at 4,848, MySQL's at 2,976 and SQLite's at
176. Since a debug poll frame gives each `.await` site its own slot, a live test awaiting a few
dozen `Db` calls overflowed the 2 MiB test thread (the live tier's entry has the case). Boxed,
they are 6,080 and 7,632, for one allocation per connection beside the round trips it makes.
`a_sql_server_future_does_not_carry_the_drivers_login` holds both under 12 KiB — room for the
rest of `connect`, not for the login — and fails at 22,896 against the inline await.
Live,
`a_named_instance_is_reached_through_sql_server_browser` puts a stand-in responder on UDP
`127.0.0.1:1434`, since no container runs the Browser, answering MS-SQLR's `SVR_RESP` with the
container's port; the saved port is 1, so a connect that used it would fail, and the instance
is `MSSQLSERVER`, the name a default instance accepts in the login's instance check. With the
stand-in silenced, the same connect has to name the Browser and the instance; answering with
named pipes alone, it has to name TCP/IP and not a silent Browser (red, on 2022, with the
classifier's old `contains("browser")` match). It skips on Azure
SQL Database, which has no named instances, and on any host but `127.0.0.1`; the whole leg
passed on 2022 and 2025. The two paths that do not come through this connect answer the
spelling themselves: `sqlcmd` does its own Browser lookup (`core::launch::sqlcmd_args`), and an
SSH tunnel refuses one (`ssh.rs`).
**Values arrive typed**, not as text — an `int` as an `i32`, a `decimal` as a scaled integer, a
`datetime2` as a day count and ticks — so `cell_value` renders each to the text SQL Server's own
tools print, and nothing in it is lossy: a `decimal` from its scaled integer, never through a
float; a `real` from its own shortest form rather than widened to `f64` first (which shows `0.1`
as `0.10000000149011612`); dates from their epochs' day counts; a `datetimeoffset` in the offset
it was stored with, which takes arithmetic because **the wire carries the instant in UTC** with
the offset beside it; a `uniqueidentifier` upper-case; bytes as `binary_display`, as on every
engine. `money` needed the driver patched (below) — upstream decoded it to an `f64`, which loses
cents past about 900 billion (2^53 at scale 4) and prints `12.5` for `12.5000` below it. The live tier's type matrix holds every arm to
the server (`every_type_renders_as_sql_server_prints_it`).
**Column provenance is not in TDS's metadata**, so each statement is first described by
`sys.dm_exec_describe_first_result_set` in browse mode, which compiles without running and names
each column's base table and column — PostgreSQL's `PREPARE`, here — at the cost of one more
round trip per statement. The description is of the batch's *first* result set, compiled a
moment before it ran, so `result_columns` uses it **only when it agrees with the wire** about how
many columns there are and what they are called: an `IF … SELECT … ELSE SELECT …` can describe
one shape and return another, and every cell would sit under another column's name and type.
That provenance is what the grid's write-back stands on (below). A read reports **one result set**: a second is logged,
and **the read goes on to the stream's end regardless**, its rows dropped and its `DONE` counts
kept. tiberius raises a batch's error only when the stream ends, and `run_statement` used to
stop at a second result set, and at the row cap, and drop the stream — so a failed batch was
reported `Ok`, whether the statements after the stop ran depended on how much the first result
returned, and Run All went on past a piece that had failed. At the row cap it reads on unless
the piece is one read (`sql::drains_past_row_cap`), so a lone `SELECT` still stops at the cap
rather than draining a table. **The lone write was the case that stayed broken**: it used to read
on only for a piece holding several statements, on the premise that a lone statement's
remaining rows are its own — and a write's `OUTPUT` rows are the write still running. An `UPDATE
… OUTPUT` over 100,000 rows read to a cap of 10 was reported a success over a truncated grid with
**no row changed**, the dropped connection having made the server roll it back; and a procedure
that `THROW`s after a result larger than the cap was reported `Ok`, on a Manual tab too, where
`flush_stream` discarded the error at the next request (the write on 2022 and 2025, the
procedure on 2022; `a_write_or_a_procedure_past_the_row_cap_is_read_to_its_end`). And a server error from a piece
holding several statements
carries `batch_error`'s sentence (`QueryFailure::BatchContinued`, the server's text above it,
under `schemaic-db`'s `lib.rs`): SQL Server runs a batch on past a statement's error unless the
error ends the batch, so a duplicate key followed by an `UPDATE` reports the duplicate while the
`UPDATE` commits (measured on 2022; `a_batch_reports_its_error_past_its_first_result`). **`prepare_check` is the same DMF, and not `SET NOEXEC ON`**, which was the
first version: `NOEXEC` compiles without resolving names, so `SELECT * FROM nope` came back clean
on SQL Server 2022, and a missing table is the error validation exists to show. The describe
answers it as error 208 in a row; `describe_error` passes over its own 115xx answers (a temporary
table, dynamic SQL — "cannot say"), which are not faults in the statement.
**A cancel is TDS's attention, on the query's own connection** — `Client::cancel_query` aborts
the running batch and waits for the server's acknowledgement, bounded by `CANCEL_TIMEOUT` — so
there is no second connection and no transport to choose, which is why `pg_cancel_gate` skips
this file (below). **That wait needed the driver patched** (patch 5, below): SQL Server finishes
its reply to the aborted request, ending that message, and sends the `DONE_ATTN` acknowledgement
as a message of its own, and upstream stopped at the first end-of-message with
`Protocol("Never got a DONE token acknowledging the Attention signal.")`. That is what a Stop of
the grid commit's statement held by a trigger was seen to get — the attention failed with that
error, every time — and the same patch is what query Stop's `cancel_now` waits through, though
nobody observed it failing there. `attention` sends it and answers whether the server acknowledged it; `cancel_now` ignores the
answer, since nothing further is sent on that connection after it, and the commit below cannot.
**Every step of a read is raced against Stop, and every Stop sends the
attention** — `cancel_now`, which `fetch_schema` and `run_script` share. `run_statement` awaited
its describe alone, and the describe compiles the statement, which waits behind another session's
schema lock: Stop did nothing until the lock let go — 4.5 s on the live tier, measured before the
fix against the five-second lock `a_query_waiting_to_compile_stops_when_asked` takes, which now
holds the read under 3 s. `simple_query` does not return until the statement's first
result set — for an `UPDATE`, until it has finished — and a Stop there had been left to the
connection's close. `run_script` holds one connection for the file, as on every engine, over
pieces the splitter has already cut — at `GO` lines, and at the `;`s between statements that
share nothing, with each routine body and each batch-scoped run (a variable, a block, an
`IF … ELSE`) kept whole (`core::sql`'s `batch_separator` and `BatchScope`). That is **not** what
SQL Server's own tools send, which cut at `GO` alone; each piece here is a batch of its own, and
the scope rules are what keep a piece from splitting what a batch scopes. A failed T-SQL
statement leaves its transaction usable, as MySQL's does, but its DDL does not commit, so
`tx_engine_of` maps it to a model of its own, `tx::TxEngine::MsSql` (under `core::tx`) — and
what a failure did under `XACT_ABORT ON`, a session's own choice to make, is read off the server
rather than assumed (the pinned session, below).
**Grid write-back is this engine's arm of *Write-back is transactional…*** (*Architecture
invariants*). `commit_writes` connects to the batch's database — a `GridWrite` is one table's —
runs `GridWrite::plan` step by step inside `BEGIN TRANSACTION` through `execute` with `@P1…`
parameters, and holds each count to `one_row_verdict`. **The count is the *last* one TDS reports
for the statement**: a trigger's statements report theirs first, in the same batch
(`a_trigger_does_not_trip_the_one_row_guard`). The statements are `statement_for`,
`refetch_statement` and `blob_statement`, pure and pinned in `write_tests`: every name through
`export::ident_sql`, qualified **`[database].[schema].[table]`** (`qname3`, which is
`export::qualified_row_target` — the grid's copies of these statements take it too) whenever the
database is known — `db..t` without a schema, two-part without a database — and the column facts
read from that database's own `sys.columns`. **The database is part of the name because a pinned
session's connection is in the session's database, not the edit's**: a Manual tab pinned to A
that read `B.dbo.t` (or ran `USE`) and edited a row of it wrote A's same-named `dbo.t`, the 1-row
net passed because A had such a row, and the re-read — the same two-part name on the same
connection — showed the typed value over a table the user never touched
(`a_manual_grid_edit_of_another_databases_table_lands_there`, which failed against the two-part
names; `SET IDENTITY_INSERT` takes the three-part name too). A NULL written as the literal `NULL`
and never a parameter, since a TDS parameter is
typed; a NULL key value compared as `IS NULL`, T-SQL having no null-safe equality before 2022's
`IS NOT DISTINCT FROM`; an insert that sets nothing as `DEFAULT VALUES`; a staged
`CellEdit::Default` as the keyword `DEFAULT`, which `cell_param` answers `None` for and `cell_sql`
writes in place of the parameter — T-SQL takes it in `SET` and `VALUES` alike
(`a_staged_default_is_the_keyword_and_binds_nothing`). **A float key is bound
as its text**: a `real` compared with an `f64` is widened to it, and `0.1` stored as a `real` is
not `0.1` as an `f64`, so the key would match nothing — text converts to the column's own type
instead, a string having the lowest precedence. A `u64` past `i64` goes as text too, other
integers as `bigint`, strings as `nvarchar` (`keys_of_every_shape_find_their_row`: `real`,
`datetime2`, `uniqueidentifier` and `decimal` keys, as the grid read them back). **A `datetime` or
`smalldatetime` is the exception to "the server's own rendering converts back"**: it converts
`yyyy-mm-dd hh:mm:ss` under the session's `DATEFORMAT`, which a day-first login language
(`british`, and the German, French, Italian and Spanish installers' default) makes
year-*day*-month — deleting the 2 January row deleted 1 February and the 1-row net passed it,
measured on 2022. So `column_text` rewrites such a column's text, key and staged value alike, into
the ISO `T` form (`export::language_safe_datetime`: `2026-01-02T00:00:00.000`, a bare date as
midnight), which every language reads the same — **core's, because the same text reaches SQL by
three other roads**: `export::typed_literal`, behind every `INSERT` the export renderer writes
(the SQL export, *Copy as SQL INSERT*, a dump's rows — and the grid's *SQL IN list* and *SQL
VALUES* copies, which are its literals without the statement, through `script_literal`) and the grid's *Filter by /
Exclude this value*, which wrote `N'2026-01-02 10:30:00.000'` and so pasted or filtered February for January
(`a_copied_insert_and_a_cell_filter_keep_their_day_under_a_day_first_language`); the predicate
all four ask is `export::reads_dates_by_language`. The column's type comes from the same `ColumnFacts` the
guards read, so `refetch_on` and `blob_on` read them too, and the import rewrites each batch
(`language_safe_batch`). `date`, `datetime2` and `datetimeoffset` read `yyyy-mm-dd` as ISO already
and are left alone (`a_datetime_key_finds_its_row_under_a_day_first_login`, a `british` login, on
2022 and 2025). The re-read is
`SELECT TOP (1)` by key and confirming columns, rendered by the same `cell_value` as the read that
produced the grid; a binary cell is `DATALENGTH` plus a `SUBSTRING` to `FETCH_CAP` — `DATALENGTH`,
not `LEN`, which counts characters and trims trailing blanks.
**Three guards the other engines do not need**, all read off `sys.columns` before the transaction
opens (`COLUMN_FACTS` — the base type through `TYPE_NAME(system_type_id)`, so an alias type is
its base type, `is_identity`, and `collation_name` with its `COLLATIONPROPERTY(…, 'CodePage')`).
**SQL Server converts `''` where PostgreSQL refuses it** — `0`
for a number, `1900-01-01` for a date, `0` for a `bit` — and a cleared grid cell is `''`, so on
this engine alone clearing a quantity would store zero and report success. `blank_refusal`
refuses, with `DbError::Refused` and before anything runs, a batch that writes `''` to a column
whose base type does not hold text (`an_empty_value_is_not_written_as_zero`, an alias type
included); a column the catalogue does not name is left to the server. **A value of spaces is
`''` to it as well** — measured on 2022, `' '` stores `0` in an `int`, `1900-01-01` in a
`datetime` or `date`, `0.0000` in a `money` and `0` in a `float` or `bit` — so `converts_blank`,
the one predicate both blank refusals ask, takes any run of ASCII spaces, and a stray space no
longer walks round the refusal (`a_whitespace_only_value_is_refused_like_an_empty_one`). A tab, a
line break or an ideographic space the server refuses itself (Msg 245/241), so only spaces are
refused here. **The other silent conversion is text a `char`/`varchar`/`text` column's code page
cannot hold**: the grid binds text as `nvarchar` and the import writes `N'…'`, so it reaches the
column whole, and the column stores what its code page lacks as `?` or a best-fit look-alike —
`Ωμέγα` into a Latin-1 `varchar` came back `Oµ??a`, measured — and reports success.
`ColumnFacts::code_page` is `0` for every other column and `65001` under a UTF-8 collation, and
`code_page_fit` answers locally with `encoding_rs` (a direct dependency now, already in the tree
through tiberius and calamine) for the single-byte Windows code pages — 874 and 1250–1258 —
**strictly**: a character with no byte there is `Fit::Loses`, never the best-fit substitute the
server would pick. ASCII, `0` and `65001` always fit. **The double-byte ones, 932, 936, 949 and
950, are not answered locally**: `encoding_rs`'s `SHIFT_JIS`, `GBK`, `EUC_KR` and `BIG5` are the
WHATWG Encoding Standard's tables, written for browsers, not Windows', and they encode `¥` as `\`
and `−` as Shift_JIS's minus, 99 characters GB18030 has and 936 has not, and 870 HKSCS
ideographs — every one of which SQL Server stored as `\` or `?` while the write reported success
(each BMP character measured against the server's own conversion on 2022; the single-byte tables
encode none the server then loses). So non-ASCII text for one of them is `Fit::Unknown`, as it is
for the IBM code pages behind the `SQL_Latin1_General_CP437`/`CP850` collations, which
`encoding_rs` lacks, and `server_code_page_refusal` settles it by asking: `first_lost` sends up
to a thousand values in one `VALUES` list per collation (T-SQL takes 2,100 parameters) and picks
the first whose `CAST(CAST(t COLLATE <coll> AS varchar(max)) AS nvarchar(max))` differs from it
under `Latin1_General_BIN2` — a question per batch, not per value, since an import of Japanese
text is all such values — then asks the same of that value's distinct characters, so the refusal
names the one lost and, for an import, the file's row. It splices the collation name only when it
is a plain word and leaves any other to the server unasked. `code_page_check` runs beside
`blank_refusal` and names the column and the character
(`text_a_varchar_cannot_hold_is_refused_before_it_becomes_a_question_mark`,
`a_code_page_says_which_characters_it_cannot_hold`; live,
`text_a_varchar_cannot_hold_is_refused_not_stored_as_question_marks` and
`a_double_byte_varchar_refuses_what_its_windows_code_page_lacks` — `¥`, `−`, an HKSCS and a
GB18030 character refused, Japanese and Big5 text kept byte for byte, an import refused at its
row — on 2022 and 2025). All of these are asked in `write_on` before its `SAVE`/`BEGIN`, so a refusal
on the pinned session leaves `undone` at `None` (below). And an insert that gives
an identity column a value needs `IDENTITY_INSERT` on — which, once on, refuses an insert that
does *not* give one — so `sets_identity` has it switched on and off around that one statement,
never for the batch (`inserts_take_defaults_identities_and_a_deleted_key`).
**Every failure rolls back through `IF @@TRANCOUNT > 0 ROLLBACK TRANSACTION`**, the guard because
some errors — a deadlock victim, a severe error — end the transaction themselves, and a bare
`ROLLBACK` after one fails. The verdict is `Rollback::Complete` when the server answered and
`Unknown` when it did not, never `Incomplete`: every SQL Server table is transactional
(`a_batch_that_misses_is_rolled_back_whole` — 0 rows, 2 rows and a conversion error, each after
a statement that had already succeeded). Stop is raced on every statement and sends the
attention, and `execute_counted` answers `Ran::Stopped { in_step }` — whether the server
acknowledged it. **Only an acknowledged attention lets a rollback be sent and believed**: the
statement's future was dropped mid-reply, and without the `DONE_ATTN` the stream is at an unknown
point, so a `ROLLBACK` sent on it could read the aborted request's leftovers as its own success —
MySQL's desynchronised-stream defect, on this engine. Unacknowledged, nothing is sent and the
verdict is `Rollback::Unknown`; the server rolls back when the connection closes, but this side
did not see it. Either way the answer goes through this module's own `cancelled_write` — MySQL's
shape, `DbError::Cancelled` only for a confirmed rollback, a `DbError::Write` of
`WriteCause::CommitCancelled` carrying the `Rollback` otherwise. Every other failure of the batch
leaves through `failed` (the server's words) or `failed_as` (any `WriteCause`), which roll back
and carry what that achieved. **The first version rolled back after every Stop and reported
`Complete` off an attention that had never been acknowledged**: before patch 5 every such
attention failed (above), so each of those confirmations had been read off a stream nobody knew
the position of.
`a_stopped_commit_is_undone` pins it against a trigger that holds the edit five seconds — it
asserts `DbError::Cancelled`, the row unchanged and the whole commit under 3 s, and failed against
the unpatched driver.
**`commit_writes` is a connect and `write_on`, the body the pinned session calls directly** —
`mysql.rs`'s shape — and `fetch_blob`/`refetch_rows` are a connect and `blob_on`/`refetch_on` on
the same terms, each read raced against Stop inside. The write is bracketed by `WriteScope`
rather than the shared `TxScope`, because T-SQL's savepoint is not the other engines'.
`WriteScope::Own` is everything above. `WriteScope::Savepoint` is `SAVE TRANSACTION schemaic_w`
(`SAVEPOINT`) inside the tab's transaction, undone by `ROLLBACK TRANSACTION schemaic_w`, never
committed — the transaction is the caller's to end — and its errors carry no rollback note, since
"rolled back all changes" there would claim the user's whole transaction. `WriteScope::failure`
is that choice: under `Own` a `DbError::Write` with the `Rollback`; under `Savepoint` the
server's words stay a bare `DbError::Query`, as they were, and any other cause — the 1-row miss —
is a `Write` whose `undone` is `None`. **Its undo also
switches off an `IDENTITY_INSERT` the batch left on**: a savepoint rollback leaves session
settings alone, and on a pinned connection the setting would outlive the batch and refuse the
tab's next ordinary insert into that table (Msg 545). **What the undo achieved goes out through
`write_on`'s `undone` parameter, and the session reads that rather than rolling back again**,
which is where this arm parts from the other engines' `classify_isolated`: a T-SQL savepoint
cannot be released, so its name stays live for the rest of the transaction, and a second
`ROLLBACK TRANSACTION schemaic_w` after a batch that failed before its own `SAVE` would land on an
*earlier* batch's — silently undoing the user's statements since. `undone` stays `None` when the
batch failed before anything was written. Both decisions are pinned live:
`a_grid_batch_refused_before_its_savepoint_keeps_the_work_before_it` (a batch the blank refusal
stops before its `SAVE` leaves the earlier grid batch and the user's own `INSERT` in place) and
`a_failed_identity_insert_leaves_identity_insert_off` (a duplicate identity insert rolled back to
the savepoint, then an ordinary insert on the same session succeeds).
**Manual mode is `Session`'s `Backend::MsSql` arm** — one pinned client, boxed because tiberius
keeps its buffers inline — and it holds the transaction as SQL Server's own tools do, with
`XACT_ABORT` left at its default, off. `Session::open_enforced` connects through `connect` and
learns `@@SPID` (`spid`) as the session's server id, which is what lets Server Activity recognise
the tab's own session, as MySQL's thread id and PostgreSQL's pid do. **It refuses a read-only
connection outright** (`DbError::Refused`, saying such a connection runs in Auto mode): SQL Server
has no read-only transaction, its fresh-connection guard is a transaction that is never committed
(`fetch_query`, above), and that cannot outlive one statement on a pinned session, where a typed
`COMMIT` or the button would keep whatever a hidden write had done. Auto mode loses such a
connection nothing, since it has nothing to commit, and `tx::offers_manual_mode` keeps the footer
from offering the mode there, so the refusal is the backstop. The session's own control
statements are T-SQL's (`Session::control_on`, over a private `Ctl`): `BEGIN TRANSACTION`,
because a bare `BEGIN` opens a block; `WHILE @@TRANCOUNT > 0 COMMIT TRANSACTION`, so the button
commits every nested level the tab opened rather than lowering the count by one; and
`IF @@TRANCOUNT > 0 ROLLBACK TRANSACTION`, since a `ROLLBACK` with nothing open is an error
(Msg 3903) rather than a warning, which a transaction the server had already rolled back would
hit. `close` issues the same guarded rollback, `alive` is a `SELECT 1` (tiberius has no liveness
flag to read), and `block_is_aborted` is `XACT_STATE()` of -1 — a doomed transaction, whose
`COMMIT` fails with Msg 3930 — or no answer. **After every operation the session asks the
server** (`tx::probes_after_every_statement`; `core::tx` has why no text can decide it):
`tx_state` reads `@@TRANCOUNT` and `XACT_STATE()` in one round trip, and
`Session::settle_from_server` sets `in_tx` from the count and folds the answer into the outcome
through `tx::settle_from_server` (under `core::tx`) — a success with the count at 0 after an open
transaction is `OkAndClosed`, a failure there `FailedAndRolledBack` — or `FailedAfterCommit`
when the user's statement holds a `COMMIT` of its own (`tx::text_commits`, which only
`fetch_query` passes) — a failure with the state at
-1 `FailedAndDoomed`, and a probe nobody answers leaves everything as it was. `tx_state` and
`spid` read their rows with `try_get`, never `get`, which unwraps: a reply of the wrong shape —
a connection out of step answering an earlier request — panicked the run task rather than
answering `None`. `fetch_query`, `commit_writes`, `fetch_blob` and
`refetch_rows` all end in it, except over a lost connection, and `tx_alive` — MySQL's probe —
is never asked on this engine. `fetch_query` runs `run_statement` on the pinned client, and a lone read's result cut short at the
row cap needs no draining for the next statement: tiberius resynchronises the stream at the start
of every request (`flush_stream`). Anything else is drained by `run_statement` itself
(`sql::drains_past_row_cap`), since that resynchronisation discards unread whatever error the
rest of the stream carried.
**A Stop is followed by a question: does the connection still answer its own requests?** An
attention the server did not acknowledge leaves its reply on the wire, and every reply after it
belongs to the request before — the next statement's grid showing the previous one's result, the
transaction probe reading another query's row. The driver now reads past an aborted request's
own error to the acknowledgement (patch 5, below), which is how a Stop of a batch that had
already failed was falling out of step; what is left is an attention refused, or not
acknowledged within `CANCEL_TIMEOUT`. So after any `DbError::Cancelled` from those four
operations — `Session::lost_after_stop`, the one place they ask — `Session::resync_after_stop`
asks `answers_in_step` — a `SELECT CAST(<n> AS bigint)`
of a number only that call could have sent, bounded by `CANCEL_TIMEOUT` — which costs one round
trip per Stop. Out of step, **the pinned connection is replaced** — the one place a `Session`
swaps its own connection: nothing more is sent on the old one, and dropping it closes the socket, so
the server rolls the transaction back and releases its locks; a fresh `connect` takes its place,
**which runs the connection's init SQL again, as every `connect` does, and nothing else**: a `SET`
the user typed in the tab — `SET LANGUAGE`, `SET DATEFORMAT`, `SET LOCK_TIMEOUT` — was the old
connection's session state and is gone with it, and the replacement runs under the init SQL and
the server's defaults alone. The tab is told about its transaction (the `tx-lost-*` notices), not
that its settings went too, so the same statement can answer differently either side of the
replacement. `server_id` (behind a `std::sync::Mutex` now, for this one change) becomes the new `@@SPID` so
Server Activity still recognises the tab, `in_tx` and the scope reset, and the operation reports
`StmtOutcome::ConnectionLost` — the transaction is gone and the tab has to hear it. If no
connection can be opened the backend becomes `Backend::Retired`, whose statements, writes and
reads answer `retired_error` until the tab is switched to Auto and back — a Rollback is `Ok`,
the server having rolled back already, and a Commit is refused as an aborted transaction
(`block_is_aborted` answers `true` for it) — and its `server_id` is none, since the old spid
may be another session's by then. The live pin is
`a_stop_after_an_error_leaves_the_manual_session_in_step` (a `SELECT 1/0` and a `RAISERROR`, each
before a `WAITFOR` that is stopped, then the next statement's own reply; 2022 and 2025), which
panicked before the driver fix. It covers the in-step branch only: an out-of-step connection
cannot be produced live. So what the replacement leaves is `replace_pinned`, synchronous and
split out of the resync for this, and the unit tests build a `Session` over `Backend::Retired`
directly, with no network: `a_retired_session_rolls_back_and_refuses_everything_else` (Rollback
`Ok`; `BEGIN`, Commit, a statement and a blob read refused, a statement as `ConnectionLost`),
`a_connection_replaced_after_a_stop_starts_over` (`in_tx`, scope and `server_id` reset) and
`only_a_stop_on_a_live_sql_server_connection_is_resynchronised`. Whether a fresh `connect`
succeeds is the one branch that stays untested.
**`run_scope_sql` answers `false` for SQL Server without
asking**, over a private `ScopeStep`, for `write_on`'s reason — an unreleasable savepoint name —
so the pinned reads run unfenced, which costs nothing because a cancelled read does not abort a
SQL Server transaction. The grid's writes report isolation off `undone` instead: nothing
written, or the savepoint rolled back and the server saying so, is `FailedIsolated` or
`CancelledIsolated`, and the probe then says whether the server itself ended or doomed the
transaction. The live pins are `a_manual_session_commits_only_when_told`,
`t_sql_transaction_statements_are_settled_by_the_server` (a `COMMIT` inside a nested
`BEGIN TRAN` closes nothing, `ROLLBACK TRANSACTION sp` keeps the transaction, a plain `COMMIT` is
`OkAndClosed`), `a_failure_is_folded_as_the_server_left_the_transaction` (a duplicate key is
`Failed` and still committable; after `SET XACT_ABORT ON` the same error is
`FailedAndRolledBack`), `a_caught_error_that_dooms_the_transaction_rolls_it_back_at_the_batch_end`
(under `core::tx`), `a_grid_edit_in_a_manual_session_is_isolated_and_uncommitted`,
`a_stopped_statement_keeps_the_manual_transaction` (a `WAITFOR` stopped, the transaction kept, the
next statement fine) and `a_read_only_connection_is_refused_a_manual_session`. Their outside view
reads `WITH (READPAST)`: under READ COMMITTED a plain read from a second connection waits on the
session's locks until the transaction ends.
**`import_rows` is that discipline scaled to a file**, in the shape every engine's load takes
(`Db::import_rows`, under `lib.rs` below): one `BEGIN TRANSACTION`, batches pulled through the
shared `next_batch_off_executor`, each required to insert exactly its own rows or the whole
import rolls back. An error the import is *handed* — the batch read's, or
`ImportTarget::insert_sql`'s — leaves through `failed_on`: a row source's `DbError::Write`, which
arrives with no `undone`, keeps its cause and gains the rollback, and anything else is `failed`
over its own sentence (`err_text`), as before. **The batches are literal SQL, not bound parameters** — the shared
`import::build_insert_as` through `ImportTarget::insert_sql`, `N'…'` for text — where
`commit_writes` binds: a bound batch spends a
parameter per cell, SQL Server takes at most 2,100 per request, and 500 rows of five columns is
already past it. The literal form has no such ceiling, and `INSERT_BATCH_ROWS`' 500 is under
T-SQL's 1,000-row limit on a `VALUES` list — and the constant is shared with three engines that
have no such limit, so raising it past 1,000 would break this one alone — which is why a
`const` assertion beside it (and beside `export::INSERT_BATCH_ROWS`, which batches a dump's
rows) makes that a build error. All three of the grid's guards come with it, read off the same `column_facts`.
**`import_blank_refusal` matters more here than `blank_refusal` does in the grid**: a CSV's empty
field is the ordinary spelling of "no value", and one the import's NULL rule did not catch
arrives as `''`, which this engine stores as `0` or `1900-01-01` and reports as success — for
every row of a column at once. It is asked of each batch before the batch runs, names the file's
row (1-based, counted across batches) and the column, and the import rolls back
(`an_imported_blank_number_is_refused_not_stored_as_zero`). `import_code_page_check` follows it
on the same terms, naming the row and the character, the server asked after it for the IBM code
pages. Both refuse inside the transaction, after earlier batches may have run, so unlike the
grid's — a `DbError::Refused` before anything opens — each is a `DbError::Write` of
`WriteCause::Refused` carrying what the rollback achieved: Schemaic's sentence, so no *Explain*,
and no "query failed:" in front (the live tests above assert that variant). An identity column among the
import's columns runs the **whole** import under `IDENTITY_INSERT` (`import_sets_identity`),
switched off before the `COMMIT` — once rather than per statement as the grid's is, since every
batch writes the same columns. **Stop is asked twice per batch** — at the top of the loop and
again once the batch has been read — and the batch in flight is raced through
`execute_counted`, the rollback trusted only after an acknowledged attention. The second ask is
load-bearing: reading a batch is where an import spends its time, so it is where Stop lands, and
left to the race the attention went to a request barely sent, the acknowledgement was not seen,
and a rollback nothing had prevented was reported as not known to have happened — which is how
the live `a_cancelled_import_rolls_back_and_says_so` failed before the check went in. The other
live pins are `an_import_loads_every_row_across_batches` (non-Latin text, a NULL, identity values
under `IDENTITY_INSERT`, and an insert that leaves the identity to the server working after it)
and `a_refused_import_row_rolls_the_whole_import_back` (a duplicate key past the first batch,
Msg 2627, 0 rows); `write_tests` pins the two predicates
(`an_imported_blank_is_refused_where_sql_server_would_convert_it`,
`an_import_wants_identity_insert_only_when_it_writes_the_identity`).
**`explain` answers with the server's own plan documents, and which batches go ahead of the
statement is `explain_setup`'s pure decision.** The estimated plan is `SET SHOWPLAN_XML ON`,
which compiles the batch and **executes none of it**, so it needs no guard and a write is as
welcome as a read. The measured plan runs the statement: `BEGIN TRANSACTION`, then
`SET STATISTICS XML ON`, and the connection is dropped uncommitted, so the work rolls back.
That rollback is the whole of the guarantee on this engine, since there is no read-only
transaction to open, and it does not undo a procedure's effect outside the database, a
sequence's advance or a remote write — so **on a read-only connection the statement must first
pass `sql::read_only_reason`**, the gate that refuses those by name, and anything it refuses is
`DbError::Refused` before a connection is made (`a_plan_is_set_up_by_whether_it_runs_the_statement`:
`EXEC dbo.p` and `SELECT NEXT VALUE FOR dbo.s` among the refusals). **Each `SET` is a batch of
its own**, which `SHOWPLAN_XML` requires. `plan_documents` then streams the statement and keeps
only the result sets `is_showplan_set` recognises — exactly one column, named `SHOWPLAN_COLUMN`
— because under `STATISTICS XML` the statement's own rows come first, in sets of their own
(`only_a_showplan_result_set_is_a_plan`); each read is raced against Stop and a Stop sends the
attention through `cancel_now`, as `run_statement` does. The documents go back as a
`ResultSet` under `SHOWPLAN_COLUMN`, so the app's `run_plan` → `QueryPlan::from_result` path is
the other engines' unchanged, and `core::plan` does the reading. The live pin is
`a_plan_is_read_from_the_servers_showplan_and_changes_nothing`: an estimated `DELETE` leaves all
three rows, a measured `SELECT` counts the two its scan returned, a measured `UPDATE` is rolled
back, and a measured `DELETE` on a read-only connection is refused.
**`run_ddl` wraps the whole plan in one `BEGIN TRANSACTION`**, since T-SQL's `CREATE TABLE`,
`CREATE INDEX` and `DROP` are transactional, as PostgreSQL's are: a best-effort
`lock_wait_sql(MsSql)` first, as on the other engines, then each statement raced against Stop. A
failure rolls back and reports `DdlError { at: i, applied: 0 }` — `applied` is always 0 here,
a half-applied plan being a state this path never leaves behind. Stop sends the attention and
rolls back **only if it was acknowledged**, `commit_writes`' rule for `commit_writes`' reason;
unacknowledged, the connection's close is what rolls the plan back. **`whole: false` is the
exception to all of that, and `Db::run_ddl_piecewise` is its only caller**: no `BEGIN
TRANSACTION`, each statement committing as it runs, nothing rolled back, and a failure reporting
what is already in effect through `ddl::applied_count`, as MySQL's runner does — for the plan
that touches a natively compiled module, which Msg 12331 keeps out of a transaction (why, and how
the plan chooses, is under `Db::run_ddl` above). **The plan comes out of
`emit_mssql`**, `ChangeSet::emit`'s own arm for SQL Server in an exhaustive `match`, which walks
only the changes `supports_change` admits, whole objects through the helpers the other emitters
share — `create_table_sql`, `DROP TABLE`, `TRUNCATE TABLE`, `drop_view_sql`, `DROP {kind}` over
`signature_sql`. **It was `emit_mysql` through a `_`**, whose whole-table loop asks no
capability, so a SQL Server Truncate went into the script as `TRUNCATE TABLE [dbo].[t];` in
MySQL's grammar while the preview listed it as refused. **An edit of an existing table runs in
phases, and the order is T-SQL's, not a tidy one**: first the guard (`tsql_in_place_guard`,
under `ddl.rs`), one batch that `THROW`s before anything runs where the plan would re-create or
reset what the model does not read, by every object's name before the plan since it runs ahead
of the renames, and no statement at all where the plan re-creates nothing; then, where the plan
moves columns, what selects `*` from the table captured (`tsql_collect_star_dependents`, under
`ddl.rs`); then the drops of what covers columns —
foreign keys first of all, since one referencing the table's own key blocks the key's drop, then
indexes (`DROP CONSTRAINT` for one a constraint backs, `DROP INDEX [i] ON t` otherwise), then
the index renames (`Change::RenameIndex`'s `sp_rename`, under `ddl.rs`) — after every index
drop, each of which names the index as it was, so a rename onto a freed name finds it free, and
before the adds that may take the old one — then
checks and the primary key (`DROP CONSTRAINT` by name). **Indexes before the key, not after**:
an XML or spatial index stands on the clustered key and the server refuses the key's drop while
one exists (Msg 3734, measured on 2022), so with the key's drop first even a plan that also
deleted its XML indexes failed (S3.2-L1-02). Nothing else in the phase waits for the key, and a
nonclustered index dropped first is one the key's drop does not rebuild. Then dropped columns,
each after its default's drop; **then** the column renames, `EXEC sp_rename N'[s].[t].[c]', N'new',
N'COLUMN';`; then altered columns (`tsql_alter_column`), added columns, the keys, checks and
foreign keys, the indexes, a primary XML one ahead of the rest (under `ddl.rs`); then the
comments, by the columns' new names and the table's
old one; then those `SELECT *` dependents refreshed, while the table still has the name they
name; and the table's own `sp_rename` last, the new name bare
because the procedure takes it literally and brackets would become part of it — so every
earlier statement names the table as it was (`sql_server_orders_a_designer_plan`, which also
doubles a quote inside `N'…'`, through `ddl_string` — `tsql_n` is that one literal rule, not a
second). **The renames sit between two walls, each hit once.** After the constraint drops,
because T-SQL refuses to rename a column a check depends on (Msg 15336, measured live), so a
check the edit replaces must be gone first. After the dropped columns, because they were first
written the other way round and a rename onto the name of a column the same plan drops collided
with it (Msg 15335) — `a_dropped_column_goes_before_a_rename_takes_its_name`. **A check the
edit leaves alone is moved out of the way by `diff`**, as on MySQL 8:
`alter_column_disturbs_checks` answers yes for SQL Server, so a rename under an unchanged check
becomes a `DropCheck` plus an `AddCheck` re-pointed by `repoint_check_column`, which reads
T-SQL's `[bracketed]` names — the one quote that opens and closes differently, `]]` a literal
`]` — since that is how the server prints every predicate
(`a_bracketed_column_in_a_check_is_repointed`,
`a_sql_server_rename_under_an_unchanged_check_moves_the_check`). This was a known limit, failing
with Msg 15336, until it was. **So was a retype under an index, the key, a foreign key or a
check**, refused by `ALTER COLUMN` (Msg 5074, then 4922) until `alter_column_disturbs_dependents`
had `diff` take each one it disturbs off and put it back (`repair_tsql_dependents`, under
`ddl.rs`). **A unique constraint goes back as one**, which is new with it: an `AddIndex` that
carries a `constraint` name and no predicate is written `ALTER TABLE … ADD CONSTRAINT [k] UNIQUE
(…)`, not `CREATE UNIQUE INDEX`, because `sys.key_constraints` is where tools and a schema compare
look for it and the next read would have seen an index where the draft said constraint. The
designer's own edit of a unique constraint goes through the same arm.
**A default is dropped by a name looked up as the plan runs** (`tsql_drop_default`). T-SQL drops
one only by its constraint's name, which introspection does not read and `ColumnInfo` does not
carry — deliberately not added as a field: `columns_equal` has missed a new field five times (under
`ddl.rs`'s round-trip gate), and a constraint's name is not a property of the
column. So the statement reads `sys.default_constraints` itself and cannot be stale. **It is one
statement with no `;` inside** — `DECLARE @df … IF @df IS NOT NULL EXEC (N'ALTER TABLE …
DROP CONSTRAINT ' + @df);`, the `DECLARE` and the `IF` side by side — because the preview's Copy
and Open in editor split a script at semicolons, and a `DECLARE` cut from its use is an
undeclared variable; and `QUOTENAME` is applied inside the `SELECT` because `EXEC (…)`
concatenates only literals and variables, a function call there being Msg 102 (measured).
**A copied plan closes every statement with `GO`**: `client_script` goes through `join_scripts`
on a batch-separator dialect, since SQL Server's tools run a paste as one batch and two dropped
columns' lookups each `DECLARE @df` — Msg 134 before anything ran
(`a_copied_sql_server_plan_is_one_batch_a_statement`). **A statement already closed by `GO`
passes through unchanged** (`ddl::ends_in_go`): a routine's `ObjectItem::create_sql` is
`client_script`'s own output, and the dump's routine section wrapped it again — `terminated`
made the `GO` a `GO;`, which is no separator, so every SQL Server dump holding a routine stopped
its restore at the first one with Msg 102.
**A comment is the `MS_Description` extended property, and an edit sets it without trusting the
read** (`TsqlComment`). On a table the plan creates, or a column it adds, there is none yet, so
`add` is a plain `EXEC sp_addextendedproperty`. Anything else is `set`: `IF EXISTS (SELECT 1 FROM
sys.extended_properties …) EXEC sp_updateextendedproperty … ELSE EXEC sp_addextendedproperty …;`,
or, clearing it, the `IF EXISTS` in front of `sp_dropextendedproperty` alone — the existence
looked up as the plan runs, as `tsql_drop_default` looks up its constraint, so a read gone stale
by Apply cannot make it fail, and one statement with no `;` inside for that one's reason. The
procedures take their names as literal strings, so the level path is unbracketed `N'…'`s
(`sql_server_sets_and_clears_comments_by_the_names_the_plan_leaves`).
`tsql_alter_column` restates **both** type and nullability, with the collation, whenever any of
them changed, since T-SQL resets what `ALTER COLUMN` isn't told — a retype that left `NOT NULL`
off would make the column nullable — and an identity is always altered `NOT NULL`. **What it
cannot restate, the plan refuses to reset**: the reset takes `MASKED` and `SPARSE` with it and
the model reads neither, so a retype, a nullability or a collation change left a masked column
readable in the clear while the plan reported success (S3.1-L1-01, measured on 2022 and 2025).
So wherever `tsql_alter_restates` says an `ALTER COLUMN` will be written — a change of type,
collation or nullability, a new default, name or comment leaving the definition alone — the
guard refuses the plan over a masked, sparse or column-set column, by its name before any
rename. `ROWGUIDCOL`, reset the same way, needs no arm: the server already refuses that
`ALTER COLUMN` (Msg 4928)
(`an_in_place_alter_column_is_refused_over_a_masked_or_sparse_column`). **A default
is redone only when it changes, or around a change of type or collation**, which a default
constraint blocks; a change of nullability alone leaves it, the server allowing that (measured).
Defaults are compared through `norm_default`. **A default dropped and re-added keeps its name**:
where there is an old one and a new one, one statement looks the name up, drops it, runs the
`ALTER COLUMN` if there is one, and re-adds the new default under the same name — `DECLARE @add
nvarchar(max) = … + ISNULL(N'CONSTRAINT ' + @df + N' ', N'') + … EXEC (@add)`, built in a
variable because `EXEC` takes no function call, and still with no `;` inside — so `df_qty` stays
`df_qty` rather than becoming the server's `DF__t__qty__5EBF139D`, a name scripts and a schema
compare use. A default added where there was none is `ADD DEFAULT (v) FOR [c]`, system-named
(`sql_server_alters_a_column_in_t_sqls_order`,
`sql_server_writes_only_the_statements_a_column_change_needs`). **The same rule keeps the
primary key's name**: it is re-added as `ADD CONSTRAINT [k] PRIMARY KEY (…)` from the
`drop_constraint` it was dropped by, T-SQL otherwise inventing a `PK__t__…` — and with
` NONCLUSTERED` where it was, from `Change::PrimaryKey::clustered`. **A nullable column
added with a default is added `WITH VALUES`**, filling the rows already there as MySQL and
PostgreSQL do; without it SQL Server gives the default only to new rows and leaves every existing
one `NULL` (`a_nullable_column_added_with_a_default_fills_existing_rows`). **A re-added check
keeps its trust and its state**: one that was not validated or not enforced goes back `WITH
NOCHECK`, and a disabled one is disabled again with `NOCHECK CONSTRAINT [n]` — re-added as an
ordinary check it would validate every existing row and start enforcing, which is a different
constraint from the one the user edited
(`a_re_added_sql_server_check_keeps_its_trust_and_its_state`). **A key column is made `NOT NULL`
by `diff`, not by the server**: MySQL and PostgreSQL do it themselves when a primary key takes a
column in, SQL Server refuses the key instead (Msg 8111), and the designer's key toggle does not
clear Nullable. So where `primary_key_implies_not_null` says no — SQL Server alone, an exhaustive
`match` — `diff` reads the draft's key columns as `NOT NULL` and the plan alters them before
`ADD PRIMARY KEY` (`a_nullable_column_keyed_on_sql_server_is_made_not_null_first`). And
**`ddl::qualified` writes every schema it is given, on every engine — PostgreSQL's `public`
included.** It went through `sql_qualifier`, which drops `public` for a display name, first
on every engine and then on PostgreSQL alone, "where it is what an unqualified name resolves
to" — which it is not: a bare name resolves through `search_path`, whose stock value is
`"$user", public`, so with a schema named after the login that holds a same-named table (the
per-user layout PostgreSQL's own documentation recommends) `TRUNCATE TABLE "orders"` for
`public.orders` emptied the login's `orders` while the confirm counted `public`'s rows, and a
designer key to `customers` bound to the login's `customers` (S2-L5-02, measured on 16). The
write path already qualified `public` for that reason (`db::pg::pg_qname`), and nothing in
`schemaic-db` sets `search_path` on a connection. So the preview of a `public` table's plan now
reads `"public"."orders"`. `sql_qualifier` keeps the elision, but for display alone: the rest
of what *runs* had gone on building names through it — every standalone object's `CREATE`, a
routine's `DROP`, a trigger's table, `TableInfo::create_ddl` and so the dump, the browse query
and a key's Follow — and now names `public` through `schema::qualified_ident` and
`filter::qualified_table_name` (under `schema.rs`, `filter.rs` and `core/dump.rs`). A
schema called `public` cannot be made on SQL Server (the `public` role holds the name, Msg 2714,
measured) (`a_sql_server_schema_named_public_is_still_named`,
`public_ddl_does_not_land_in_the_logins_own_schema`,
`public_objects_are_named_in_every_statement_that_runs`).
**`tsql_supports`' `AlterColumn` arm is an allowlist.** It copies `to`'s name, type,
nullability, collation, default, key flag and comment onto `from` — the fields `tsql_alter_column`,
the rename and the comment phase write — and admits the change only if that already equals `to` under `columns_equal`,
with no position, no inline check and neither side computed. A list of the fields it cannot
write would have admitted the next one nobody listed (`identity_always`, `on_update`, …) as a
change that emits nothing and so never converges; the comparison that raised the change is the
one that knows every field (`a_sql_server_column_change_is_admitted_only_when_it_is_written`).
**What it refuses one change at a time, a rebuild does for the whole set.** Facts about T-SQL's
`ALTER COLUMN`: an identity switched on or off, a computed column changed or a column turned into
or out of one — in place each is a drop and re-add, which would lose the column's values — and a
column's position, T-SQL having no reorder. A set holding any of them is answered by `diff` with
a `RebuildTable`, which `tsql_supports` admits and `ddl::tsql_rebuild_sql` writes (under
`ddl.rs`); until it did, the first two were withheld in the preview and the reorder arrows were
off.
Besides those it refuses MariaDB's inline check, and an unnamed check or a primary key whose
constraint name wasn't read, since T-SQL drops both by name — and neither of those two is
reachable from a reading, the reader naming every key through `sys.indexes` and every check
through `sys.check_constraints`. A `TableOptions` is admitted only as the comment alone — engine
and collation are MySQL's table options.
**Introspection reads clustering rather than withholding it**: `IdxRow::clustered` is `Some` of
whether `sys.indexes.type` is 1 or 5 — a clustered rowstore or a clustered columnstore
(`IndexInfo::clustered`, under `schema.rs`) — and an index is `lossy` for the graph-column
marker (under the dump, below), where a selective XML one and an ordered columnstore were too
until each was modelled (below); the fold still marks every XML and spatial one (types 3 and 4) `lossy`, and a second
pass clears it where the kind is stated whole (below). Before the
field a clustered index other than the key's was marked `lossy` too, and an edit to it withheld
as a `KeepLossyIndex`, since recreating it plainly left the table a heap; the emitter writes
`CLUSTERED` and `NONCLUSTERED` where T-SQL's default is wrong now (under `ddl.rs`), and
`a_clustered_index_and_a_nonclustered_key_keep_their_clustering` adds a column to a clustered
`cx` beside a `NONCLUSTERED` key, retypes the key's column, and reads both back as they were,
the result round-tripping. **Included columns are read the same way, rather than withheld.**
Column 9 of `INDEX_LISTING` and `VIEW_INDEX_LISTING` is each row's own `is_included_column`,
where it was an `EXISTS` asking whether the index had any — which made the whole index `lossy`,
so it could not be edited, restated by a rebuild or written into a dump — and `IdxRow::included`
carries it to the fold (under `lib.rs`). The join admits a row with `key_ordinal > 0`, an
included one, or any row of an XML, spatial or columnstore index (types 3 to 6), and the
`ORDER BY` puts `is_included_column` ahead of `key_ordinal`, so the key comes first and the list
keeps its own order. **The flag is forced to 0 for types 3 to 6, and that is load-bearing**: a
columnstore has no key columns, SQL Server listing its columns with `key_ordinal` 0 *and*
flagged as included, so taken at its word it would fold into an index with an empty key. Read
as key rows, a nonclustered one's are the columns it stores, which `create_index_sql` lists.
**A columnstore index is read whole now** (`IndexInfo::method`, under `schema.rs`): types 5 and
6 are `method` `TSQL_COLUMNSTORE`, and after `assemble_schema` a clustered one's columns are
cleared, since the catalogue lists every column of the table under it and the fold took them
for its key. **An ordered one (`ORDER (…)`, 2022 on) is read with its order** into
`IndexInfo::order`. It was `lossy` until then: cell 12 of `INDEX_LISTING`, an `{ordered}`
placeholder, was an `EXISTS` on `column_store_order_ordinal > 0` that marked the whole index
`lossy` without reading which columns. That cell is gone; a listing of its own,
`columnstore_order_listing`, reads the `column_store_order_ordinal > 0` rows of types 5 and 6 in
ordinal order, and a pass after the clustered-columnstore one pushes each column onto the
matching columnstore index's `order`. It answers `None` where `Catalogue::columnstore_order`
says the column is missing — before 2022, which has no ordered columnstore to read — so the
pass never runs there, rather than a constant standing in for the column.
`introspection_reads_the_schema_as_declared` expects a clustered columnstore read whole
and written as `COLUMNSTORE` in Copy DDL; `a_columnstore_index_is_read_rebuilt_and_authored` is
the rest, below. **XML and spatial indexes were not read at all.** One lists its one column with
`key_ordinal` 0 and `is_included_column` 0 (measured on 2022), and the join admitted only a
keyed row, an included one or a columnstore's, so it never reached the fold: Copy DDL and the
dump left it out with no note, a rebuild's refusal never named it — the rebuild's own
index-count arm would have stopped it at run time as changed since it was read — and a
comparison never saw it. The join now admits types 3 and 4 too, so they were read — at first
as lossy indexes, named by the rebuild's refusal and noted by Copy DDL. AdventureWorksLT's
`SalesLT.ProductModel.PXML_ProductModel_CatalogDescription` was invisible:
`a_real_schema_reads_and_every_table_round_trips` printed 0 notes over that sample before the fix
and 1 after — and 0 again once the index was read whole, which is the next step.
**An XML or spatial index is read whole now.** The fold still takes types 3 and 4 as `lossy`,
knowing only their type; after the clustered-columnstore and order passes a second listing,
`xml_spatial_listing(cat)`, gives one row per such index, `LEFT JOIN`ed to `sys.xml_indexes` (the
`xml_index_type`, the using index's name through `using_xml_index_id`, `secondary_type_desc`),
`sys.spatial_indexes` (`tessellation_scheme`) and `sys.spatial_index_tessellations` (the
bounding box, the four grid levels, `cells_per_object`), and an eighteenth cell for a secondary
selective index's path (below). The pure `tsql_xml_or_spatial` turns a
row into `method` and `using`, and the load patches the matching index — same schema, table and
name, exactly one column — with both and `lossy` `false`. A primary XML index (type 0) has no
clause; a secondary (1) is `USING XML INDEX [primary] FOR PATH|VALUE|PROPERTY`, the name through
the one quoter; a spatial one is `USING <scheme>` with a `WITH (…)` of whatever the catalogue
holds. **Every value is checked for what it is before it reaches T-SQL text**, because `using` is
text an emitter writes verbatim: the `FOR` one of three words, the scheme one of
`GEOMETRY_GRID`/`GEOMETRY_AUTO_GRID`/`GEOGRAPHY_GRID`/`GEOGRAPHY_AUTO_GRID`, a grid level one of
`LOW`/`MEDIUM`/`HIGH`, the bounding box written by Rust's `Display` only when all four parse as
finite `f64`s, `CELLS_PER_OBJECT` only when it parses as a `u32`. **A bounding-box value past
`numeric(38)` is written with an exponent** (`{v:e}`, a float literal): a plain decimal whose
integer digits, less leading zeros, plus its scale come to more than 38 — `1e39` plainly, or
`1.2345e-35` — is refused (Msg 1007, measured on 2022), so such an index could be read but not
replayed. Within 38 the plain spelling stays, so an existing index restates as it always did
(in `an_xml_or_spatial_index_is_read_with_every_option`). An unknown `FOR`, scheme or
grid word answers `None`, leaving the index `lossy` rather than splicing it. **So does an option read in part** — a value present that does not parse, a
bounding box or a grid missing a part: it was left out of the clause while the index still read
as whole, so a restatement took the server's default for it, a different index under the same
name; each option is now there whole or the index is `lossy`. **Every option is restated**, not just
the ones that differ from a default, so a restatement is the index as it stands rather than
whatever the server's defaults are when it is made again.
`an_xml_or_spatial_index_is_read_with_every_option` holds rows as 2022 printed them for the
indexes they were made from, and injection-shaped values refused.
**A selective XML index is read whole too** (`xml_index_type` 2), where until now it answered
`None` and stayed `lossy`, its grammar being its own. Two gated listings read it,
`selective_path_listing` (`sys.selective_xml_index_paths`, joined to `sys.types` for an `AS
SQL` path's type, in `path_id` order) and `selective_namespace_listing`
(`sys.selective_xml_index_namespaces`), both `None` where `Catalogue::selective_xml` — the probe
of `sys.xml_indexes.path_id`, SQL Server 2012 SP1 — says the server has none. The load groups
their rows by index and the pure `tsql_selective_xml(paths, namespaces)` composes `[WITH
XMLNAMESPACES (…)] FOR (…)`, which `tsql_xml_or_spatial` takes for type 2. **The namespaces
are in the reader's order, not the collation's** — the default first, then the prefixes byte
for byte — because `selective_namespace_listing`'s `ORDER BY n.prefix` runs in the database's
collation: the same index read `[a], [B]` in a CI_AS database and `[B], [a]` in a BIN2 one, so
every comparison between the two re-created it (measured on 2022; in
`a_selective_xml_index_is_restated_as_written`). **Each path says
only what was written**, because the catalogue fills in what it inferred and a restatement that
wrote it back would be another index: the XQuery type unless the server inferred it, `MAXLENGTH`
only when it is above 0 and not inferred — an untyped path and an unbounded `xs:string` both
read `-1` — `SINGLETON`, `node()`, and an `AS SQL` type through `mssql_type_name`. Names go
through `ident`; paths, XQuery types and namespace URIs are `N''` literals, which the server
takes (measured). A collation on an `AS SQL` type is not restated, `COLLATE` there being a
syntax error (measured). **What T-SQL does not spell is `None`, never a guess**, so the index
stays `lossy`: `SINGLETON` or `MAXLENGTH` with no type written, each a syntax error (measured),
whether the path is untyped or its type was inferred — though `node()` takes `SINGLETON` and is
restated with it (measured on 2022 and 2025); an alias or user-defined `AS SQL` type, which the
server refuses (Msg 6375); a type name that is not a bare word; a path kind past the two; and an
index with no paths read at all. A **secondary** selective index (`xml_index_type` 3) is an
ordinary `TSQL_XML` index, its clause `USING XML INDEX [sxi] FOR ([path])` built from the using
index's name and the eighteenth cell, a subquery of `sys.selective_xml_index_paths` on
`using_xml_index_id` and `path_id` — the literal `NULL` where the catalogue has no such view, so
the row keeps its shape — and `None` with no path read
(`a_selective_xml_index_is_restated_as_written`,
`a_selective_xml_index_the_model_cannot_state_is_lossy`,
`a_selective_xml_index_and_its_secondary_are_read_as_their_kinds`). `VIEW_INDEX_LISTING` keeps its
join, a view carrying no XML or spatial index. A
partitioning column outside the key, neither keyed nor included, is still left out. Every
admitted change is
written by one of the phases bar `KeepLossyIndex`, whose statement is none; the catch-all arms
write nothing, and that has the opposite hazard — an admitted change one swallowed would vanish
from the plan in silence — so `sql_server_admits_the_table_changes_it_can_write` walks both
lists, asserting that every admitted change emits something and every refused one nothing,
which keeps the two one. The live leg pins it:
`a_designed_table_is_created_as_drafted` creates an identity key, a collation, a default, a persisted computed column, a check, a unique index and a cascading foreign key into
another schema, reads each back by introspection and has an insert fill the identity;
`a_failing_plan_is_rolled_back_whole` follows a `CREATE TABLE` with a `CREATE INDEX` on a missing
column and gets `at` 1, `applied` 0 and no table; `a_table_a_view_and_a_procedure_are_dropped`
is the three drops; `a_designer_edit_lands_as_drafted` renames, retypes, makes `NOT NULL` and
re-defaults one column, drops a column that has a default, adds one, swaps a check, adds an index
and renames the table in one plan, then reads it back with the data kept, the new default filling
a new row and the result round-tripping; `a_primary_key_is_replaced` widens a key by its read
constraint name; `an_identity_toggle_is_withheld_over_an_index_the_rebuild_cannot_restate`
switches an identity on over an edge table's index on its `$from_id` (`gx`), a column the model
leaves out, and gets the one refusal, naming it (it planted an index with included columns
until those were read, a nonclustered columnstore until that was, a primary XML and a spatial
index until those were, and an ordered clustered columnstore until that was);
`xml_and_spatial_indexes_are_read_whole_and_survive_a_rebuild` reads a primary and a secondary
XML index, a `GEOMETRY_GRID` with every option, a `GEOMETRY_AUTO_GRID` and a geography index as
their kinds, not lossy, rebuilds the table around them by an identity toggle, and reads each
back with the same `method` and `using`, the table diffing to nothing;
`a_selective_xml_index_is_read_whole_and_survives_a_rebuild` reads a selective index with two
namespaces and every kind of path — untyped, XQuery-typed with a `MAXLENGTH` and `SINGLETON`,
`node()`, `AS SQL` with and without `SINGLETON` — and a secondary on one of its paths, named
to sort before it, as their kinds, not lossy; rebuilds the table around them and reads both
back with the same `method` and `using`, diffing to nothing; drops the table and runs its Copy
DDL from nothing, reading both back again; and lands both from the designer's Storage choice,
the secondary listed first;
`xml_and_spatial_indexes_are_made_and_kept_whole` compares two tables — one with a secondary
XML index that sorts before its parent and a spatial index whose bounding box is past
`numeric(38)`, one with a selective index declaring two prefixed namespaces and a secondary on
it — into an empty BIN2 database (Msg 6333 and Msg 1007 before), then compares again to
nothing; renames `pxml` to `pxml2` and gives the selective parent a path, and finds
`ixml_path` and `sx_q` still there after each;
`a_columnstore_index_is_read_rebuilt_and_authored` reads a clustered columnstore and a filtered
nonclustered one back not lossy, rebuilds each table around its index by an identity toggle and
reads the index back equal with a diff of nothing, lands a nonclustered one made through
`IndexStorage::Columnstore`, reads an ordered clustered one (`ORDER (b, a)`) with its order,
rebuilds around it with `ORDER ([b], [a])` and re-orders it from the draft alone, and on 2025
(or Azure SQL Database) authors an ordered nonclustered one, which 2022 refuses by name;
`a_storage_switch_to_columnstore_applies` switches a filtered `(qty DESC)` index to
*Columnstore* and to *Clustered columnstore* beside a nonclustered key through
`IndexStorage::apply` alone, and applies both plans — the first was Msg 35302 before `apply`
cleared the direction;
`a_rebuild_keeps_an_indexs_included_columns` reads `(a DESC) INCLUDE (c, b) WHERE a > 0` back as
key `[a]` and list `[c, b]`, not lossy, rebuilds the table around it by an identity toggle,
reads it back equal with a diff of nothing, then lands an edit of the list alone;
`a_moved_column_rebuilds_the_table_and_keeps_what_stood_on_it` moves a column and adds one with a
default, and reads back the rows, the key, check, unique index and defaults under their names,
the comments, a disabled trigger still disabled, another table's `ON DELETE CASCADE` key, an
identity carrying on past a deleted row rather than from the highest one left, and the result
round-tripping; `an_identity_switched_on_keeps_the_rows_values` numbers on from the highest;
`a_rebuild_is_refused_where_the_table_has_what_it_would_drop` has the guard refuse a table with a
grant on it and leave it as it was; `a_rebuild_under_a_new_name_switches_off_an_identity_and_computes_a_column`
does both in a renamed rebuild, the table's key on itself coming back pointed at the new name;
`a_designer_edit_keeps_what_it_did_not_change` holds the review's findings to
the server at once — named defaults kept across a retype and untouched by a nullability change, a
check re-pointed around a rename, a nullable column keyed, a column added `WITH VALUES`, a
disabled check still disabled, the key's name kept, and the result round-tripping;
`comments_are_set_changed_and_cleared` sets, changes and clears table and column comments,
whether or not one was there and by the names the plan's renames leave, reading each back;
`a_retype_under_its_dependents_lands` retypes columns under the key, an index, a unique
constraint and a check, reads each back under its name — the unique one still a constraint —
with the data kept, the foreign key on a nullability-only change left on, and the result
round-tripping; `a_column_under_a_computed_one_is_renamed_and_retyped` renames and retypes the
column a persisted computed column reads, the computed one carrying an index and a comment, and
reads it back last in the table — reading the new name, still persisted, its index and comment
back — with its value recomputed and the result round-tripping. That was a known limit, loud,
until then: a computed column referencing a column the plan retyped or renamed was not moved out
of the way, so the retype failed with Msg 5074 or the rename with 15336, and the plan rolled back
whole; `repair_tsql_dependents` now rebuilds it (under `ddl.rs`).
`a_view_is_altered_in_place_and_renamed` takes a schema-bound view with a column list and a
grant, alters its body and reads all three back, then renames it and reads the stored
definition naming the new name; a unique clustered and a nonclustered index on it are read into
`TsqlView::indexes` (and not `TableInfo::indexes`), and are there again after the in-place
edit, after the rename under the new name, and after a replay of the view's own script.
`a_trigger_is_altered_in_place_and_keeps_what_the_alter_resets` is the trigger editor's leg: a
trigger written with a leading comment, `EXECUTE AS OWNER`, two events and `NOT FOR REPLICATION`,
ranked `First` on `INSERT` and disabled, beside a plain one and an encrypted one. It reads the
header options, the rank and the state back through the parts, the encrypted one `hidden` and not
editable, and the set diffing to nothing; then one plan edits the first one's body — asserted
altered, never dropped — and renames the second, and it reads the rank restored, the trigger still
disabled, the rename landed and the encrypted one untouched, the round-trip gate holding again.
The renamed one fires, the disabled one stays silent until it is enabled and then fires its edited
body, and a new `INSTEAD OF` trigger on a view fires in place of the write to it.
`the_trigger_controls_rank_and_execute_as_on_the_server` is the form's controls on the same
terms: two triggers swap `First` and `Last` on `INSERT` in one plan — the case that needs every
rank set after every alter (under `ddl.rs`) — one takes `EXECUTE AS` a user, and an event switched
off takes its rank with it, each state reading back as written and diffing to nothing.
`a_routine_is_altered_in_place_and_keeps_what_the_alter_resets` is the routine editor's leg: a
procedure with two defaulted parameters (one a string default holding a comma), `WITH RECOMPILE,
EXECUTE AS OWNER`, a grant and a comment, beside a scalar function written without `AS` and an
encrypted procedure. It reads the defaults and the options back, the encrypted one `hidden` and
not editable, and every routine diffing to nothing; then an edit of the body and a default —
asserted to carry no `DROP` — runs with the default kept and the grant, the options and the
comment still there. A rename lands and restates the comment; the scalar made table-valued is
recreated and returns its rows; and a procedure made from `RoutineDraft::blank` validates,
creates, runs and diffs to nothing. `the_routine_controls_reach_the_server` writes through the
slots the form writes: a scalar function stored `WITH CALLED ON NULL INPUT` is given `RETURNS
NULL ON NULL INPUT`, `INLINE = OFF` and `EXECUTE AS OWNER` — and stops running its body on a
NULL — then turned back off, reading back as stored, the stated `CALLED ON NULL INPUT` included;
a procedure given `EXECUTE AS` a user returns that user from `USER_NAME()`.
**A trigger is read from `TRIGGER_LISTING`, one row per event**, folded into one `TriggerInfo`
each; the row carries `sys.trigger_events.is_first`/`is_last`, so the rank is per event, as
`sp_settriggerorder` sets it. The events are sorted into `TriggerEvent`'s declaration order as
they fold, the order the editor's toggles keep, since a catalogue order they re-sorted would be a
phantom change — and so are the ranks, the order `TsqlTrigger::set_rank` keeps for the same
reason. The stored text goes through `tsql_trigger_reading`, pure: `tsql_trigger_parts`
gives the body after the header's `AS`, the options and the header's comments (into
`TsqlTrigger::module`, `TsqlModule` under `schema.rs`); a header it cannot read keeps the whole
text as `verbatim` (and as the body, for whatever displays it); a NULL `definition` is `hidden`
(`a_stored_trigger_reads_into_its_parts_or_is_kept_whole`).
**A routine's text goes through `tsql_routine_reading`**, pure, on the same terms: the parts
`tsql_routine_parts` reads give its parameter list, `RETURNS`, body and `TsqlRoutine`, the
header's comments in its `module`; a header
it cannot read is `verbatim`; a NULL `definition` is `hidden`. **The text's parameter list wins
whenever it reads**, over the one `PARAMETER_LISTING` builds from `sys.parameters`, because that
one has no defaults (`has_default_value` is 0 for `@a int = 5`, measured) and fed back to `CREATE
OR ALTER` it would drop every one; the catalogue's list and return type are kept only for a
routine that cannot be rebuilt, where they are for display
(`a_stored_routine_reads_into_its_parts_or_is_kept_whole`). **`NUMBERED_LISTING` reads what the
routine listing cannot see** — a numbered group's members past the first, which live only in
`sys.numbered_procedures` — and `fetch_schema`'s routine loop attaches them to their head as
`TsqlRoutine::numbered` (under `schema.rs`; why a plan that drops a head is refused is under
`ddl.rs`).
**A view's text goes through `tsql_view_reading`**, pure, on the same terms: the parts
`ddl::tsql_view_parts` reads give the body after the header's own `AS`, the column list as
`ViewOptions::column_list`, verbatim, and the attributes as `ViewOptions::attributes`, which
`ALTER VIEW` resets unless they are restated (under `schema.rs`), and the header's comments as
`TsqlView::module`; a header it cannot read keeps
the whole text as the body and as `create_sql` and is `TsqlView::verbatim`, with no attribute
kept from a list it could not read whole; a NULL `definition` is `hidden` — so an encrypted view
now has `view_options` at all, where it had `None`
(`a_stored_view_reads_into_its_parts_or_is_kept_whole`). The walk was two byte scans here,
`view_select_body` and `view_header_options`, until the `v$as` they split and the guesses they
fell back on moved it into core (under `ddl.rs`). `dependent_ddl` is left
empty for a view, deliberately — see `TableInfo::dependent_ddl`, under `schema.rs`.
**`VIEW_LISTING`, `TRIGGER_LISTING` and `ROUTINE_LISTING` also read the settings each module was
created under** — `sys.sql_modules.uses_ansi_nulls` and `uses_quoted_identifier`, cast to `int`
— and `module_settings` sets `TsqlModule::ansi_nulls_off`/`quoted_identifier_off` after the
text's reading, whichever way the header walk went. A flag goes off only where the cell is `0`:
a NULL, no module row visible, leaves the ANSI default a rebuild would have used anyway. Nothing
read them before, and an edit silently re-filed a module written OFF under this session's ON
(why, and the measurement, under `schema.rs`; the wrapper that restates them under `ddl.rs`).
**`VIEW_INDEX_LISTING` reads an indexed view's indexes** — `INDEX_LISTING`'s row shape over
`sys.views`, so the table listing is untouched — and its rows are chained into the index rows
`assemble_schema` folds, clustering and `lossy` read exactly as a table's, then moved off the
view's `TableInfo::indexes` onto `TsqlView::indexes`, because `ALTER VIEW` and `DROP VIEW` both
take them and the edit plan has to build them again (under `ddl.rs`). The live
`a_view_is_altered_in_place_and_renamed` pins it end to end on 2022 and 2025.
**`DATABASE_LISTING` asks `HAS_DBACCESS` inside a `CASE`, and never of a single-user database.**
On one another session holds `SINGLE_USER`, that call took 2,174 ms against 150 ms (SQL Server
2022 CU27; 1,997 ms re-measured for a plain login) — what an administrator's maintenance window
would cost every tree refresh — and a plain `AND user_access <> 1` beside it does not stop it
being evaluated, since T-SQL promises no order for `AND`. `CASE` does. A plain login cannot tell
a held single-user database from a free one without that stall, so for it one is left out, held
or not. **A `RESTRICTED_USER` database is asked like any other** — `HAS_DBACCESS` answers it at
once (4 ms) and correctly, 0 for a login that is not its `db_owner` — and **a login holding
`CONNECT ANY DATABASE` sees both kinds**, the short-circuit coming first in the `CASE`. The
listing once left every restricted and single-user database out for everyone, so the database
an administrator had just put in maintenance vanished from their tree while opening it by name
worked. **Listed is not read, though.** Both listings return `user_access` beside the name, and
`listed_database` turns its `1` into `ListedDatabase::single_user` — this is the one engine that
sets it — because the schema load connects *into* every database it reads (`fetch_schema`'s
`connect(db, Some(database))`), and single-user mode admits one connection. Once sysadmin's
listing included these, a load landing between an administrator's `ALTER DATABASE x SET
SINGLE_USER WITH ROLLBACK IMMEDIATE` and their `RESTORE … WITH REPLACE`, `DBCC CHECKDB(x,
REPAIR_…)` or `USE x` took the slot and failed the maintenance step (Msg 924/3101) — and a load
runs after every server-scope DDL, that `ALTER` included, so the one most likely to land there is
the app's own. Such a database is therefore listed and left `SchemaState::Unread`
(`schema::unasked_reads`, under `schema.rs`); expanding it offers a *Read*, which says it is held
if another session has it (`schema_tree.rs`'s `unread_row`). The filter still names
`user_access` only inside the `CASE` — a filter on it beside the `CASE` is how a restricted
database once vanished from every tree — and `the_listings_read_past_a_database_mid_create`
counts it there and requires both queries to select it;
`a_listing_row_says_whether_its_database_is_single_user` pins the reading, and the live
`a_database_in_maintenance_is_listed_for_a_login_that_may_open_it` (2022 and 2025) asserts the
flag and that `unasked_reads` leaves the single-user scratch out even as the active database. The four system databases (ids 1–4) are left out.
**The listing can also wait on a database another session is creating or dropping**, and that is
why it reads `sys.databases WITH (READPAST)` and asks nothing per database of a login holding
`CONNECT ANY DATABASE` (`sysadmin` does). Its catalogue row is held under a lock that reading
`sys.databases` and `HAS_DBACCESS` both wait on — `LCK_M_S`, which `SET LOCK_TIMEOUT` does not
govern, `READ UNCOMMITTED` does not avoid, and every other source tried (`sys.sysdatabases`,
`sys.master_files`, `DB_NAME`) waits on the same way. Under six parallel create/drop loops (SQL
Server 2022 CU27) the plain listing took up to 5.1 s against its 5 s `PING_TIMEOUT`; the
`READPAST` scan took 0 ms, and with the short-circuit `sa`'s listing 0–4 ms. `READPAST` skips
only the rows under that lock — a database mid-create or mid-drop, which the next refresh shows.
**A plain login still asks `HAS_DBACCESS`, which still waits** (to 7.8 s measured), so
`listing_within` signs in first, bounded by the whole five seconds, then gives the
access-checked **query** `access_check_share` of what is left, and on a stall runs
`DATABASE_LISTING_UNFILTERED` on a fresh connection in the rest of the five. **The sign-in
is not in the share**: it was, and a connect of 3–5 s — a Microsoft Entra one through the Azure
CLI — abandoned the filtered listing mid-connect and left the fallback's own connect the 2 s
remaining, failing a listing the one 5 s bound before it answered
(`a_slow_connect_is_not_charged_to_the_access_check`). **The fallback's sign-in is reserved
before the share is taken**, estimated as the connect just measured, plus
`FALLBACK_QUERY_ALLOWANCE` (500 ms) for its query; the check gets `ACCESS_CHECK_BUDGET`, three
seconds, or what is left beyond that reserve if less. The share was a flat `min(3 s, left)`,
and behind a 1.5 s connect — a remote server, a VPN, Azure SQL's redirect — a stalled check ran
to 4.5 s and left half a second for a second sign-in and a query, answering "timed out" where
the listing before the check had listed; the old test modelled a fallback with no sign-in at
all, so it never saw this (`a_stalled_access_check_leaves_the_fallback_its_own_sign_in`, connects
of 100 ms to 2 s, each fallback signing in for as long again). Where even the reserve does not
fit no fallback could finish, so the check is given all of what is left — the Entra sign-in
above is answered by the filtered query or not at all. The cost of the fallback is a list that
may name a database this login cannot enter, a restricted or held one included, which then says
so when it is expanded — a single-user one when its *Read* is pressed, since it is listed
`Unread` like any other — the answer it had before the check existed. **Only a stall falls
back**: an error from the sign-in or the filtered query is returned as the answer, since a
refused login would be refused again
(`the_fallback_is_bounded_and_an_error_is_not_retried`, beside
`a_stalled_access_check_falls_back_to_the_unfiltered_listing` on a paused clock;
`the_listings_read_past_a_database_mid_create` pins both queries' text). The live leg's
`a_ping_and_the_database_list_reach_the_server` used to retry the listing three times for exactly
this — the rest of the leg creates and drops databases in parallel — and no longer does.
**`sys.objects.type` is `char(2)`**, so a procedure's `P` arrives padded as `P `, and until
`routine_shape` trimmed it every stored procedure was read as a function
(`a_routine_is_shaped_by_its_padded_object_type`).
**Partitioning is read in `collect_schema`, from two listings.** `PARTITION_LISTING` takes each
table whose heap or clustered index (index 0 or 1) sits on a partition scheme, with the scheme,
its function, the function's `boundary_value_on_right` and the partitioning column, the one
whose `sys.index_columns.partition_ordinal` is 1; `PARTITION_BOUNDARY_LISTING` takes every
function's `sys.partition_range_values` in order, each as a SQL literal — a date quoted in style
121, a string quoted with its quotes doubled, a number bare. The pure `mssql_partitioning` folds
the two into `TableInfo::partitioning` per table: the key `RANGE RIGHT ([at]) ON [ps_year]`, and
the partitions named `1` to `n + 1` with `tsql_partition_bounds`' bounds (under `schema.rs`;
`partition_schemes_fold_into_numbered_partitions`). Both are `?` reads like the rest of the
load. Nothing restates it: no `CREATE TABLE` here writes a partition scheme
(`schema::restates_partitioning`), so *Script table as CREATE* says the table is partitioned and
the comparison holds a missing one back (under `schema.rs` and `compare.rs`). Verified live on
SQL Server 2022 (per the change's author): a `RANGE RIGHT` date function over a table with a
clustered primary key and a `RANGE LEFT` int function over a heap read back as these keys and bounds, and the
script note appeared.
**A schema load names no catalogue column the server has not got.** A static query naming one
fails whole at compile time (Msg 207, `CASE` or not), and `collect_schema` stops at the first
failure, so once the graph, temporal and sequence reads arrived every schema load on SQL Server
2016 or earlier failed — the tree, the dump, Copy DDL and the designer with it. `collect_schema`
now runs `CATALOGUE_PROBE` first, one `COL_LENGTH` per column, into `Catalogue`, and
`column_listing`, `index_listing`, `sequence_listing`, `table_kind_listing` and
`xml_spatial_listing` are built from its answer: `sys.columns.graph_type` and
`sys.tables.is_node`/`is_edge` (2017), `sys.tables.temporal_type` (2016), `is_memory_optimized`
(2014), `sys.tables.ledger_type` and `ledger_view_id` (2022), `sys.sequences.last_used_value`
(2017) and `sys.xml_indexes.path_id` (2012 SP1, the probe's eighth cell, behind
`xml_spatial_listing`'s selective-path cell), each a constant in its place
where it is missing so the
reader's cell indices hold, and no table counted as a kind its server cannot have. Three
listings are not built with a constant but run only where the server has what they read, each
answering `None` otherwise: `columnstore_order_listing` on
`sys.index_columns.column_store_order_ordinal` (2022, the probe's seventh cell), and
`selective_path_listing` and `selective_namespace_listing` on `path_id`. Before 2017
`current_value` stands in for `last_used_value`; the two differ only on a sequence that has
handed out nothing, which then restores one step on — a gap, where reading nothing put every
used sequence back at its start. Asked by column rather than by `ProductMajorVersion`, since
Azure SQL Database reports 12 and has every one
(`a_listing_names_no_catalogue_column_the_server_has_not_got`,
`a_listing_without_a_column_keeps_its_cell_count`,
`the_catalogue_probe_reads_each_column_in_its_place`). The first walks `every_listing`, the
three optional ones included, and **asks for `t.is_node`, not a bare `is_node`**:
`sys.selective_xml_index_paths` has an `is_node` of its own, which `selective_path_listing`
reads on every server that has the view, so the bare name would fail the check on a listing that
is right. No pre-2017 server is reachable here;
both forms of every listing were run against 2022 and 2025 — bar the fallbacks added since,
`{ordered}`'s constant `0` (now gone with its cell) and the `NULL` in `xml_spatial_listing`'s
selective-path cell, which neither server builds. **Nor does the account browser
call a function the server has not got**: the login and user listings gathered each
principal's roles with `STRING_AGG`, which is 2017's, so on 2016 the browser failed whole. The
memberships are now queries of their own (`SERVER_ROLE_MEMBERSHIPS`,
`DATABASE_ROLE_MEMBERSHIPS`), joined to the listing's `principal_id` by `roles_by_member` into
the same `, `-joined text, and `no_query_here_aggregates_with_string_agg` holds `mssql.rs`'s
queries to that.
**The driver is vendored and patched**, on the terms `vendor/floem/` is (*Floem 0.2 gotchas*):
`vendor/tiberius/` is the published 0.13.0 through `[patch.crates-io]`, each change marked
`schemaic patch (PATCHES.md)`, and `vendor/tiberius/PATCHES.md` is the record. Five patches:
**`ring` instead of `aws-lc-rs`**, because the workspace keeps one rustls provider and upstream's
default features bring a C/cmake/NASM build and a second provider rustls cannot choose between;
**`Config::rustls_client_config`**, so `db::tls`'s configuration is the one that handshakes
(`tls.rs`, below); **`QueryStream::rows_affected`**, since upstream skips every `DONE` token and
the affected count lives only there — and `forward_to_metadata` must collect them as well, since
`simple_query` calls it before handing the stream back, and for a statement with no result set
that is every token it produces; **`money` as an exact `Numeric` at scale 4**; and
**`cancel_query` reading past the aborted request's reply to the acknowledgement** —
`flush_done_attention` answers `None` where a message ends without it, and `cancel_request`
loops over messages, clearing `flushed` between them, until it arrives. A server that never sends
one leaves it waiting, which is why every caller here bounds it with `CANCEL_TIMEOUT`. **An
`Error::Server` at a message's end is that end, not the drain's answer**: the first version
returned it through `?`, so a Stop of a batch that had already failed (`SELECT 1/0; WAITFOR …`)
answered with the division's error and left the `DONE_ATTN` message unread, and every later reply
on the connection belonged to the request before it — the next statement panicked a pinned
session's transaction probe (measured on 2022 and 2025). Any other error still propagates. A re-vendor
re-applies the list; when it is empty the directory and the `[patch]` entry go. **One limit is
not patched**: the driver cannot decode `varchar` text in code page 437 at all ("Encoding error:
unsupported encoding (LCID 0x409, sort ID 32)"), so reading a `SQL_Latin1_General_CP437` column
fails, and the live code-page test reads its column back through `CAST(… AS nvarchar)`.
The rest of the engine's surface is in `core`: `TableInfo::create_ddl` has a T-SQL arm
(`tsql_create_ddl` — the identity with the seed and increment `sys.identity_columns` reported,
and `(1,1)` with a comment saying so only where they were not read, named primary-key and unique
constraints and their clustering (`create_ddl_sql_server_restates_clustering`), checks — a
disabled or untrusted one not inline but after the table, `WITH NOCHECK ADD` and `NOCHECK
CONSTRAINT` through `ChangeSet::emit`, since inline it was enforced over the rows it had been
switched off for (`a_sql_server_check_that_is_off_is_added_after_the_table_as_it_was`) —,
`AS (…) PERSISTED` — with the `NOT NULL` a persisted one may carry, as the designer's
`tsql_definition` writes it, which the copy otherwise dropped and so took rows the original
refused (`a_persisted_not_null_computed_column_keeps_its_not_null`) — a graph table `AS NODE`
or `AS EDGE` and a temporal, memory-optimised or ledger one — or a ledger view, which the
same listing reads — not at all (`TsqlTableKind`, read by
`table_kind_listing`; its `unrestatable` reason in a comment, the sequence arm's rule, since
read as a plain table a node carried its internal `graph_id_…`/`$node_id_…` columns and refused
the original's rows, Msg 515, and a system-versioned table became two unrelated plain ones —
`a_sql_server_graph_temporal_or_memory_optimised_table_is_not_scripted_as_a_plain_one`); the
internal columns are left out of `column_listing` by `graph_type`, the index keyed on nothing
but graph ids is dropped and one naming another graph column is withheld as `lossy` — other indexes as separate
statements through `ddl::create_index_sql`, their `INCLUDE` lists, a columnstore's form and an
XML or spatial index's kind and `USING` clause with them, a selective one's paths and an
ordered columnstore's `ORDER` included, XML parents ahead of their secondaries
(`IndexInfo::include_sql`, `IndexInfo::using`, `IndexInfo::order`; a lossy index is left out
under a note naming no kind, `create_ddl_sql_server_writes_t_sql`,
`create_ddl_writes_an_xml_parent_before_its_secondaries`), the table's and columns' comments after them through `ddl::tsql_add_comment` — the
same `sp_addextendedproperty` the emitter writes (`create_ddl_sql_server_restates_the_comments`)
— and what it cannot restate named in a comment; a view is its stored definition **with the
header rebuilt under the catalogue's name**, as SQL Server's own scripter writes it, through
`ddl::tsql_view_parts` and the one view emitter `ddl::view_ddl` — `sp_rename` leaves the stored
text naming the old view, and a view created unqualified under a non-`dbo` default schema is
stored as `CREATE VIEW v`, so restated verbatim the script created another object, and a dump's
own `DROP VIEW IF EXISTS` above it removed the real one (measured on SQL Server 2022); an
indexed view's `TsqlView::indexes` follow it through `ddl::view_index_statements`, clustered
first, each in a `GO` batch of its own because `CREATE VIEW` must be alone in one, and one it
cannot restate named in a comment as a table's is
(`create_ddl_sql_server_indexed_view_builds_its_indexes_after_it`); only a
header the walk cannot read is restated as written, under a `-- NOTE:` saying so, its indexes
still after it (`create_ddl_sql_server_view_is_rebuilt_under_its_catalogue_name`,
`create_ddl_sql_server_view_it_cannot_read_is_restated_with_a_note`; live,
`a_views_script_names_the_view_the_catalogue_has`)), held
to the server, comments included, by `a_tables_ddl_rebuilds_the_table_it_was_read_from`; a trigger's DDL is rebuilt from
the parts its stored statement was read into, with its rank and disabled state after it as
statements of their own, and a schema script closes each object's batch with `GO` (`TriggerInfo::create_sql`,
`ddl::join_scripts`, both under `schema.rs`);
`activity::from_mssql_rows`/`mssql_state` fold the session rows, drawing no blocking edge to a
negative id (not a session) or to the session's own — a parallel query's threads waiting on one
another are reported as the session blocking itself, which is no lock anyone else holds
(`a_sql_server_blocker_is_an_edge_only_when_it_is_a_session`); `snippet` has an `mssql` scope
with a five-snippet DMV pack (`every_builtin_snippet_runs`). In the app, the terminal's *open DB
client* opens `sqlcmd` (`mssql_shell`, with the WSL fallback the other network clients have),
its argv built — and refused where `sqlcmd` cannot be told the connection honestly — by
`core::launch::sqlcmd_args`; see there.
