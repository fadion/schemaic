# `core/users.rs`

`users.rs` — the **Users and privileges** browser's model: which accounts a server knows, and
what each of them is allowed to do, over an already-fetched snapshot. No DB, no UI, no engine,
and deliberately the same shape as `activity.rs` — the fetch is one query set per engine in
`schemaic-db`, because the catalogues share nothing, while every decision about what a
`Principal` *says* lives here with the tests, because a principal means the same thing whichever
engine produced it. `supports_users` is the capability (**false for SQLite, and that is a
statement about SQLite rather than unfinished work**: it is a library linked into this process
and its access control is the filesystem's — the database file's permissions, granted to an OS
user by the OS — so there is no account to browse and no statement that would create one; it
was false for SQL Server too, as unfinished work, and is true there now — see below for how the
one list takes SQL Server's two catalogues), and `supports_user_admin` is *computed* from it rather than spelling out a second `!= Sqlite`. A
`Principal` is `name`/`host`/`kind`/`system`/`attributes`/`role_ambiguous`/`login`, and the `Option`
on `host` is the one place the two engines disagree about what an account *is*: on MySQL/MariaDB
it **is** the `(user, host)` pair, where `'app'@'%'` and `'app'@'localhost'` are two accounts
with different passwords and different privileges, while a PostgreSQL role is not host-scoped
at all — its host
rules live in `pg_hba.conf`, a file, and no catalogue publishes it. **A *role* carries no host on
any engine here, and `from_mysql_rows` drops the one the catalogue stored rather than working
around it in each statement builder.** MariaDB keeps `''` for a role and MySQL 8 keeps `'%'`, and
the first reading of that was that a statement naming one still has to match what the server
stored; measured on MariaDB 10.11, the server says the opposite. `SHOW GRANTS FOR 'r'@''` is
ERROR 1141, `GRANT SELECT ON world.* TO 'r'@''` is ERROR 1133 (*Can't find any matching row in
the user table*), and `DROP ROLE 'r'@''` is ERROR 1064 — a **syntax** error, because MariaDB's
`DROP ROLE` grammar has no `@host` in it at all — while the bare name works for all three, and on
MySQL 8 a bare role name resolves to the `%` row it stored. That was three of the browser's four
role actions broken on MariaDB. It is fixed in the fold and not in the builders because the
*display* was wrong by the same value: `Some("")` renders through `display()` as `readers@`,
trailing `@`, in the list, the detail heading, the preview's subject and what was then the Drop
confirm's title. `a_role_carries_no_host_because_no_statement_naming_one_accepts_it` pins it, and its
counterpart `a_user_keeps_the_host_that_makes_it_a_distinct_account` pins the half that must not
move with it. `system` marks the accounts the server owns and maintains (the reserved
`mysql.`/`mariadb.` prefixes, PostgreSQL's `pg_` predefined roles, **and the one exact name
`mysql`**), and they are **kept rather than filtered** — "why can `pg_monitor` read that"
is a real question — but `sort_principals` leads its key with the flag, because PostgreSQL 16
ships fourteen `pg_*` roles and a fresh cluster has two accounts of its own, so sorting by name
alone buries the ones an administrator actually made in a list that is four-fifths furniture.
**`mysql` is on that list as a name rather than as a prefix because the prefix cannot reach it** —
`"mysql"` does not start with `"mysql."`. MariaDB's `mysql_install_db` creates `mysql@localhost`
for unix-socket authentication of the OS `mysql` user, and while it was uncovered that account
sorted among the administrator's own, undimmed, and was offered Privileges and Drop — the two
actions the browser withholds from server-owned accounts precisely because changing them breaks
the server. The trade is named where the match is: an administrator who named a real account
`mysql` finds it read-only here, against a one-click `DROP USER` on the server's own login.
`an_account_that_only_looks_like_the_servers_is_not_marked_as_it` keeps `mysqldump` and
`mariadbctl` on the administrator's side of that line.
**`MyUserRow`'s every field but the pair is an `Option`, because the two servers disagree about
which columns exist**: MariaDB 10.11 has `is_role` and no `account_locked`, MySQL 8.4 the
reverse, and selecting the union fails outright with `Unknown column`. So `None` here means
*this server does not publish it*, never *no*, and an absent column produces **no attribute at
all** rather than a `No` the browser would be asserting on the server's behalf
(`a_column_the_server_lacks_produces_no_attribute`). **Role detection is MariaDB's `is_role`
flag or nothing**: MySQL 8 has no such column and implements `CREATE ROLE` as a locked,
password-expired user, so reading that pair back as "role" would relabel every genuinely locked
account as one — on MySQL every row is a `User` and the Locked attribute says the rest, which
`a_locked_mysql_account_is_not_guessed_to_be_a_role` pins.
**What that label cannot carry, `Principal::role_ambiguous` does** — "could this row be a role
the server does not label as one?" — and it exists because the labelling decision above was made
for a *read* screen and a later release composed a **write** gate on top of it. On MySQL 8.4.11
the browser offered **Reset password** for a role and the server took the `ALTER USER … IDENTIFIED
BY` it emitted (`authentication_string` 0 → 70 bytes, `password_expired` `Y` → `N`, measured on
`127.0.0.1:3307`), while the identical statement on MariaDB 10.11.14 is `ERROR 1396` — one
`SqlDialect::MySql` covering two servers that answer oppositely, with nothing on screen saying
which. `kind` still reads `User` on such a row, because the display decision stands and a wrong
label on a privilege screen is worse than a missing one; the field is the *uncertainty* beside it.
`from_mysql_rows` sets it only where `is_role` is absent — MariaDB's flag settles both
directions, so a MariaDB row that happens to carry MySQL's fingerprint is not ambiguous — and it
is `false` on the PostgreSQL fold (`rolcanlogin` answers), on `AccountDraft::principal` and on
`GrantDraft::role_change`. The fingerprint is `my_role_fingerprint`: locked **and**
password-expired **and** a *published* empty credential, the three flags MySQL's own
`CREATE ROLE` leaves; its own doc carries the measured four-row table separating that row from a
locked user, a locked-and-expired user and an ordinary one. **All three have to be published
rather than merely not-`'Y'`**, which is `MyUserRow`'s own rule applied one level up: a server
answering none of the columns produces no ambiguity and keeps the reset it always had.
`has_credential` is that third flag, and it is a **presence and never the hash** — the query
answers `LENGTH(authentication_string) > 0` server-side, on `redact_secrets`' terms, so nothing
credential-equivalent crosses into the process to be read back. The residue is named rather than
papered over: a locked, expired, genuinely passwordless *user* matches the fingerprint too and
loses its reset, and a role someone has since `ACCOUNT UNLOCK`ed or already reset reads exactly
like a user — nothing in the catalogue recovers role-ness from that, which is why the guard sits
on the offer and not on the repair. The three tests assert over **the fold's output** rather than
a hand-built `Principal`, because either half reads correctly alone and the fault was only
visible where they met: `a_mysql_role_fingerprint_is_not_offered_a_password_reset`,
`a_locked_mysql_user_keeps_its_password_reset` (three rows that must keep it) and
`mariadb_answers_role_ness_from_its_flag_not_the_fingerprint`.
On PostgreSQL `rolcanlogin` is the
user/role split, and it is the only split PostgreSQL makes; `from_pg_rows` lists only the
attributes that are *set*, since a role with nine "No" rows is where `Superuser` stops standing
out.
**`attributes` is a `Vec<AccountAttribute>`, not text.** It was `(label, value)` pairs of
finished English, which the browser could only show in English whatever the interface's
language. A variant is one kind of fact (`Locked(bool)`, `Superuser`, `ConnectionLimit(i32)`,
`LoginType(LoginType)`, …) and carries the server's own text where the fact has one — a
plugin, `rolvaliduntil` as rendered, a default database or schema, a login, a role list as the
catalogue query joined it, a login-type letter `LoginType::Other` has no word for — which is
data and passes through. `label()`/`value()` are the English pair it replaced, and
`every_account_attribute_reads_as_the_pair_it_replaced` holds them to the old strings; the
three folds push them in the order they pushed the pairs. A PostgreSQL flag listed only when
set carries no value, its presence being the *Yes*, and `NoInherit` reads *Inherit — No*, the
state that is notable. No CLI or MCP surface reads `attributes`, so the type changed nothing
but what the window shows (under `users_view.rs`).
**SQL Server splits an account in two, and the browser lists both halves, linked** — the model
chosen over listing either half alone. A server **login** (`sys.server_principals`) holds the
password and signs in; a database **user** (`sys.database_principals`, one database's) is what
database permissions and role memberships are granted to, and is mapped to a login by SID. So
`PrincipalKind` has a third arm, `Login` (no other engine has one), and `Principal::login` — a
`#[serde(default)]` `Option`, `None` on every other engine and on a user with no login
(`WITHOUT LOGIN`, or contained) — carries a user's backing login as `SUSER_SNAME(sid)` reads
it (on Azure SQL Database, which refuses that call, as the SID join finds it — which from a
user database is never, since `sys.server_principals` there lists only the `##MS_…##` server
roles; under `mssql.rs`): that field is the link, and what the list shows and a reset on the
user row alters.
**A user with no login is one of two things, and only the catalogue can say which**:
`Principal::database_password` (`#[serde(default)]`, `false` everywhere else) is set by
`from_mssql_rows` for a user whose `authentication_type_desc` is `DATABASE` — a contained
database user, signing in with a password of its own — where a user `WITHOUT LOGIN` reads
`NONE` and refuses a password outright (Msg 33234, measured on SQL Server 2022). `display` writes `name ← login` for a user mapped to
a login of another name, so the row shows the link and, `matches` being over `display`, a search
for the login finds the user too. `from_mssql_rows` folds the logins first, then the browser
database's users and roles, each user carrying its login as an attribute as well; the server's
own logins (`NT AUTHORITY\…`, `NT SERVICE\…`, `##…`) and the database's `dbo`, `guest`,
`INFORMATION_SCHEMA`, `sys`, `public` and fixed roles are `system`, kept and sorted last as every
engine's are. Its listed roles are the **database's**; a login's server roles are an attribute
on its row and a read-back, and the grant form's Role shortcut offers them from
`role_suggestions` rather than from the list (below). `account_kinds(dialect, scope)` is the New
account form's Kind list — `[Login, User, Role]` on SQL Server, a login first because a user is
mapped to one, `[User, Role]` elsewhere, and `[User, Role]` on SQL Server too where
`scope.logins_elsewhere` (below) — and `blank_account_draft(dialect, scope)` opens on its first
entry, with `AccountDraft::also_user` set when that is a login: a new login brings a same-named
user in the browser's database **in the same plan** (`companion_user_draft`, `FOR` that login,
no password of its own), since that pair is what giving someone access to a database takes
there. Where the logins live elsewhere it opens on a User bringing nothing
(`where_logins_live_elsewhere_the_form_offers_none`).
`AccountDraft::login` is a new user's `FOR LOGIN`, empty for `WITHOUT LOGIN` or a contained
user; `AccountDraft::principal` sets `database_password` for a SQL Server user drafted with no
login and a non-empty password, the one `tsql_account_draft_sql` creates `WITH PASSWORD`.
`AccountDraft::external` is the fourth kind of SQL Server user, a **Microsoft Entra** one —
`CREATE USER [x] FROM EXTERNAL PROVIDER`, a person or group Entra signs in, with no login and no
password — offered only where `supports_entra_users(dialect, scope)`, which is SQL Server's alone
and there `scope.entra_users`: the connection itself signed in through Entra, since one signed in
any other way is refused the statement (Msg 33159 — Microsoft's documentation, not measured here:
the live tier signs in to Azure only through Entra).
`takes_password(dialect, kind, contained)` answers where the password row belongs — the user on
MySQL and PostgreSQL, the **login** on SQL Server, and a SQL Server user too where the database
is `contained` — and `draft_takes_password(dialect, draft, contained)` is that question asked of
the draft the form holds, answering no for an Entra user whatever the database is. The form asks
the second; the first is what it is built from. `account_form_blocker` refuses a login with no
password, a login being a SQL login by its password, and holds back an external draft that is
not a User or that carries a login or a password — held back rather than either silently
dropped, because those fields can hold a value typed before the toggle hid them
(`an_entra_user_is_created_from_the_external_provider`). **The Kind switch hides the password
rows the same way and is handled the other way: `account_form_as_shown` clears a password —
and its confirmation — that `draft_takes_password` says the form does not show**, and both the
statement and the blocker are read from what it returns. The form opens on a Login, so a
password is typed for one; switched to User on a database that is not contained, the value
stayed in the signals, and a user left without a login went out `CREATE USER … WITH PASSWORD`
(Msg 33233) rather than the `WITHOUT LOGIN` the form described, while one given a login held
Preview back under "leave this blank" with nothing on screen to blank. A reset keeps what was
typed, its one row being the password, and so does an Entra draft, for the held-back sentence
above (`a_password_the_kind_switch_hid_is_neither_emitted_nor_blocking`; the form's call site,
`the_preview_leaves_out_a_password_the_kind_switch_hid` under `account_editor.rs`). **The Entra
toggle is the same hazard on the form's other hidden field**: drawn for a User alone, turned on
and then hidden by switching Kind to Role (or Login), it stayed on, and Preview was held back
under "Only a database user is made from Microsoft Entra." with no toggle on screen. So the
draft as shown carries `external` only where the toggle is drawn — a User, where
`supports_entra_users` — and the password rule reads that draft
(`an_entra_toggle_the_kind_switch_hid_is_neither_emitted_nor_blocking`). The blocker's
not-a-User sentence is now reached only by a caller that builds the draft directly.
**What the form may make is `Principals::scope`, an `AccountScope` read with the list** — three
`bool`s, every one `false` on every other engine, which is also an ordinary server's answer.
`contained` is the first, because SQL Server takes a user's own password only in a contained
database (`CONTAINMENT = PARTIAL`; Msg 33233 anywhere else, measured on SQL Server 2022), and
making a database contained needs the server's `contained database authentication` on — so the
row is offered only where Apply can succeed, rather than offered everywhere and refused there.
Every Azure SQL database takes one, whatever its `containment` reads, so there the flag is always
set. `logins_elsewhere` is the second: **Azure SQL Database's logins are `master`'s, and nothing
the account plan can send reaches them.** Measured there with a probe: in a user database
`CREATE LOGIN` is Msg 5001 and `master.sys.sp_executesql` Msg 40515; in `master` itself `CREATE`
and `DROP LOGIN` and `ALTER SERVER ROLE [##MS_…##] ADD MEMBER` work, but a server-level `GRANT`
is Msg 40521 and `sys.server_permissions` does not exist (Msg 208), so a login's grants could be
neither read nor written from anywhere. The decision is that Azure SQL Database offers **no
logins at all**, in `master` included — its accounts are the contained users above and Entra
users — rather than a Login kind that half works in one database. `entra_users` is the third.
It was a single `contained: bool` until the other two arrived. **Where an account can be granted is asked
of the account** (`levels_for_account`, below): a login is a principal of the server and of no
database, a user or role of its database and of no server, so `GRANT SELECT … TO` a login and
`GRANT VIEW SERVER STATE TO` a user are each Msg 15151 (measured on SQL Server 2022).
`supports_grant_to` is computed from it — any level at all — and answers yes for every account
today; it was a blanket no for a login while only database levels were offered, which is what
kept Privileges off a login row. `core::ddl` asks it of a role's member, and the browser of the
row.
`account_sql` is the account as **executed** SQL names it (`SHOW GRANTS FOR 'app'@'%'`),
and it is deliberately **not** a fifth identifier quoter: MySQL spells an account as two *string
literals*, so it goes through the one literal quoter (`schema::ddl_string` → `export::sql_literal`,
which already knows MySQL escapes a backslash inside a literal and PostgreSQL does not), and
the host-less half is `export::ident_sql` — PostgreSQL's roles, and a MySQL/MariaDB role too now
that one carries no host, which is the branch every statement naming a role takes.
`parse_grantee` is the inverse, for the fallback fetch: it splits
`information_schema.USER_PRIVILEGES`'s `'app'@'%'` cell back into the pair by **unquoting through
`sql::skip_noncode`** rather than splitting on `@`, because
`'a@b'@'%'` is a legal account and the first `@` would invent two — and it unquotes **by the rule
the scanner scanned with**. `skip_noncode` reads the literal as MySQL, where a backslash escapes,
so undoing `''` alone left `'o\'brien'@'%'` scanned as one literal and unquoted to `o\'brien`:
the backslash still in the account name, and every statement then built against an account that
does not exist. `unquote_mysql_literal` is that narrow inverse — the quote and backslash forms
and not the rest of MySQL's escape table (the newline, NUL and Ctrl-Z spellings), because a
half-known table is worse than a stated one. Latent rather than live, since
`information_schema` writes the doubled form; what was wrong was the two rules disagreeing
(`a_backslash_escape_is_undone_the_way_the_scanner_read_it`).
**PostgreSQL has no `SHOW GRANTS`, so `pg_grant_statements` has to write what MySQL merely
repeats.** `PgAclRow` is one `aclexplode` row joined back to its object, and the statements are
grouped by object **and** by `grantable`, because `WITH GRANT OPTION` belongs to the statement
rather than to the privilege and folding two privileges that disagree about it into one line
would claim it for both. The grouping is **one pass, comparing each row against the group before
it** rather than searching every group so far: all four of `pg::fetch_grants`'s queries order by
object, so a row either continues the group before it or starts a new one and the search back is
work with no answer to find. The old shape also cloned two `String`s per row for a key it usually
threw away — a role with privileges on every table of a 500-table schema is 3,500 rows and ~500
groups, roughly 875,000 String-pair comparisons. A row arriving out of order **opens a second
group naming the same object** rather than corrupting anything, so the ordering is an
optimisation the caller supplies and not a contract it can break; the two halves are
`rows_in_object_order_become_one_statement_per_object` and
`rows_out_of_object_order_still_name_the_right_privileges`. A complete set collapses to
`ALL PRIVILEGES` — what the administrator almost certainly typed, and what fits on a line where
seven comma-separated words do not — while a privilege the kind's list doesn't know (a newer
server's) sorts **last**, is still printed, and blocks the collapse. `PgObjectKind` covers databases, schemas, tables and sequences only:
functions and procedures are absent on purpose, since `GRANT EXECUTE ON FUNCTION` must name the
argument types and an overloaded name alone is ambiguous, so the `Grants::note` saying so beats a
statement naming the wrong overload — **and the note had to be made to actually say it**, which
was the half of that reasoning nobody had checked. `pg::fetch_grants` reads four ACL columns and
`pg_proc.proacl` and `pg_attribute.attacl` are two of the ones it does not, so a role holding
nothing but an `EXECUTE` grant and a column grant was shown an empty privilege list under a note
that named only the *database* scope as the thing it could not answer for. Both forms of
`GrantsNote::PgScope`, with a database and without, now name EXECUTE on functions and procedures, and
privileges granted on a single column, as not listed. The grantee is taken **unquoted** and quoted here with
`export::ident_if_needed` (`pg_ident`) — SQL the user only reads — which is exactly what keeps it
from being confused with `account_sql`'s unconditional quoting for SQL that runs.
`Grants { statements, note }` is the honesty half, and `GrantsNote::PgScope` is PostgreSQL's note:
PostgreSQL keeps schema, table and sequence privileges in the catalogue of the database holding
the object, so one connection answers for one database and the note names which
(its `database: None` form is the same sentence for a connection with none selected, where those
queries are not run at all). A privilege
screen that is quietly partial is the one way this feature can mislead.
**`Principals { list, note }` is that same shape for the account list, and it exists because a
`Vec<Principal>` had no channel for "the wide reads were refused, this is one account out of
eight".** An ordinary application account — no `SELECT` on `mysql` — browsed a list of exactly
itself, rendered as a complete list under a footer reading "1 account".
`PrincipalsNote::MyOwnAccountOnly` is what the MySQL ladder's last rung sets, and `Principals::complete`
is the constructor for every rung above it, so the absence is carried out rather than inferred
from a count. **MySQL's grant list carries a note too** — `GrantsNote::MyScope`: `SHOW GRANTS` is
documented on both servers as **direct grants only**, so a privilege held through a granted role
does not appear and no `USING` form is issued here that would expand one. The list shipped with
`note: None`, so a role-based setup — the normal way a modern MySQL 8 or MariaDB installation is
provisioned — showed an account with almost nothing on it and said nothing about why.
**Both notes are values, not sentences** — `PrincipalsNote` and `GrantsNote`, one variant per
thing a list can leave out, each with a `message()` that is the English byte for byte and no
`Display` — so the browser words them in the interface's language (under `users_view.rs`). A
variant carries only what is the server's: a database, a role or a securable class's name.
**`pg_implicit_note` is what an ACL query cannot see, and it is stated rather than emitted as a
`GRANT`.** `aclexplode` reads only *explicit* entries, and three of the commonest ways a role
holds power leave none: a superuser bypasses every check, an owner holds every privilege on what
it owns, and a `pg_*` predefined role's powers are wired into the server. Clicking
`pg_read_all_data` therefore printed the pane's own **"This account holds no privileges."**, and
an owner of fourteen tables got one statement about a database they were not looking at under a
note that enumerated its omissions and did not include these. A line shaped like a `GRANT` would
be a lie in the other direction — there is no statement that would produce these and none that
would revoke them — so it is prose, a `PgImplicit` carried as `PgScope`'s `implicit` and
placed **first**, because "this role is a superuser" changes how the rest of the screen should
be read. The superuser sentence subsumes the other two and returns alone. `Held`'s `database`
is `None` with no database selected, where the English says *this cluster* — a literal `pg.rs`
used to pass in as though it were the database's name. `is_pg_predefined` is
the same `pg_` prefix test `pg::roles` uses to keep those roles out of a *grantable* list, and
they are kept in the browser deliberately, which is exactly why their powers have to be
accounted for. Both forms of the scope note now also admit ownership and superuser among their
omissions.
**SQL Server has no `SHOW GRANTS` either, so `mssql_grant_statements` writes its list** from
`MsPermRow`s — `sys.database_permissions` for a user or role, `sys.server_permissions` for a
login — one statement per row, in catalogue order and ungrouped: `GRANT`, `GRANT … WITH GRANT
OPTION` for state `W`, and **`DENY`** for `D`, which neither other engine has and which beats any
grant, so a list without it would say nothing of what the account is refused. **Every securable
class the catalogue reports is named**, in T-SQL's own keyword for it (the private
`mssql_securable`): `DATABASE::`, `SCHEMA::`, `OBJECT::` with a column list for a column's
permission, and past the four the grant form writes `LOGIN::`/`SERVER ROLE::` and
`USER::`/`ROLE::`/`APPLICATION ROLE::` (a principal's class split by the principal's type,
`MsPermRow::kind`), `ENDPOINT::`, `TYPE::`, `XML SCHEMA COLLECTION::`, `CERTIFICATE::`, the
keys, the Service Broker and full-text securables and `DATABASE SCOPED CREDENTIAL::` — each
keyword not always the class's name (`SYMMETRIC_KEYS` is `SYMMETRIC KEY::`, `SERVICE_CONTRACT`
is `CONTRACT::`). A `SERVER`-class permission has no `ON`. **It read four classes and dropped
the rest with nothing on screen**, and what it dropped was the escalation path: a login granted
`IMPERSONATE ON LOGIN::sa` — sysadmin one `EXECUTE AS` away — read as holding `CONNECT SQL`.
A row that still cannot be rendered — a class with no keyword here, such as an availability
group, or a securable whose name did not resolve — is left out rather than guessed, and
**`mssql_unshown_note` counts it** into `Grants::note` (`GrantsNote::MsUnshown`), naming the classes, as
`pg_implicit_note` does for what PostgreSQL's reader never reads
(`every_securable_class_reads_back_as_t_sql`, `a_permission_that_cannot_be_rendered_is_reported`).
Role memberships follow as
`ALTER ROLE … ADD MEMBER`, and a login's server roles as `ALTER SERVER ROLE … ADD MEMBER`
(`mssql_server_role_statements`).
**`redact_secrets` is why nothing here can put a credential on screen.** MariaDB's `SHOW GRANTS`
carries the account's stored hash inline (`IDENTIFIED BY PASSWORD '*01E8…'`), and for
`mysql_native_password` that hash *is* the credential — the client proves knowledge of it, so
anyone reading it off a screenshot can authenticate as the account. **The rule is positional,
not a list of spellings**: every single-quoted literal after an `IDENTIFIED` keyword is replaced
with `<hidden>` — single-quoted only, because MySQL also reads `"…"` as a string while on
PostgreSQL the same bytes are an identifier, and blanking those would blank out an object name —
*except* one introduced by `WITH` or `VIA`, which names the plugin rather than the
secret, and scanning stops at `REQUIRE`, whose literals are X.509 subjects and issuers — public
by nature, and the reason a blanket "redact every literal" would be worse rather than safer.
That covers `IDENTIFIED BY 'plaintext'`, MySQL's `IDENTIFIED WITH 'plugin' AS '$A$005$…'` and
MariaDB's `IDENTIFIED VIA … USING '*01E8…'` without knowing which server wrote which. Literal
boundaries come from `sql::skip_noncode` — the one boundary lexer — so an escaped quote inside a
hash cannot end the span early and leave its tail on screen
(`an_escaped_quote_inside_the_secret_does_not_end_it_early`).
**The write half's statement builders live here beside the readers**, for the reason everything
else in this module does: what a `GRANT` says is decided the same way whichever engine will run
it, and here it has tests. `GrantLevel` is one enum with per-dialect arms — `Global`,
`Database`, `Schema`, `Table { qualifier, name }`, `Sequence { qualifier, name }` — because
**the two engines mean different things by the same word**: a MySQL `GRANT … ON db.*` reaches
every table in the database, while a PostgreSQL `GRANT … ON DATABASE d` grants on the database
*object* (`CONNECT`/`CREATE`/`TEMPORARY`) and says nothing about the tables in it, so one shared
"database level" would change meaning under the user on a connection switch. `levels_for` says
which arms an engine offers: MySQL has `Global` and no `Schema`/`Sequence`, and **PostgreSQL has
no `Global` at all** — a statement about PostgreSQL rather than a gap, since its cluster-wide
powers are role *attributes* (`SUPERUSER`, `CREATEDB`, `REPLICATION`) carried on the role and set
with `ALTER ROLE`, not privileges `GRANT` can express. SQL Server offers `Global` — the server
itself — then `Database`, `Schema` and `Table`, a database user's three securables, written
`DATABASE::[d]`, `SCHEMA::[s]` and `OBJECT::[s].[t]` by `GrantLevel::object_sql`, which writes
*nothing* for the server level, so `privilege_sql` leaves out the `ON`: a server permission names
no securable (`GRANT VIEW SERVER STATE TO [app]`). **`levels_for_account` is the half an account
actually gets** — `[Global]` for a login, the other three for a user or role, `levels_for`
unchanged on every other engine — and the level picker and `ddl::account_change_supported` both
read it rather than `levels_for`, so the form cannot offer a level the plan's gate then refuses
(`a_login_is_granted_at_the_server_and_a_user_below_it`). On Azure SQL Database `Global` is never
reached, because no login is listed or offered there (`AccountScope::logins_elsewhere`, above):
its `master` refuses a server-level `GRANT` (Msg 40521) and has no `sys.server_permissions` to
read one back from (Msg 208). SQLite gets an empty list rather than a
panic, `supports_user_admin` being the gate that should have stopped the caller.
**`default_grant_level` is the level a grant form opens on, and it is deliberately not
`levels_for(dialect).first()`** — the widest of the *account's* levels (`levels_for_account`)
that is not `Global`, `Global` only where that is the account's one level (a SQL Server login,
which therefore opens on the whole server), and `None` only where the engine grants at no level
at all. The first-entry spelling is right on PostgreSQL by
accident, its list starting at `Database` for the reason just given; MySQL's does start at
`Global`, and that level takes **no name fields**, so the form opened already satisfied at the
widest scope the server has and its shortest path was `GRANT … ON *.*` from two clicks — tick a
privilege, press Preview SQL. Asked as a capability so the answer is one the engine gives rather
than one its list order gives, and so a fourth dialect inherits it by saying what it grants at
instead of by where an entry happens to sit.
`privileges_for(dialect, level)` is **curated, not exhaustive, on MySQL's global level**:
`GRANT` there also takes the server-administration privileges (`SHUTDOWN`, `SUPER`, and MySQL
8's few dozen dynamic ones), a list that differs by server *and by version*, that no catalogue
publishes as a menu, and that nobody should be handed a checkbox for next to `SELECT` — the
editor is where the whole language is available. PostgreSQL's lists are the complete ones and
are *read off* `PgObjectKind::all_privileges` rather than restated, so the set this form offers
and the set that reads back as `ALL PRIVILEGES` cannot disagree;
`granting_every_postgres_table_privilege_reads_back_as_all_privileges` pins exactly that
composition by ticking every box and running the result through `pg_grant_statements`.
SQL Server's three lists are curated the way MySQL's global one is — the everyday permissions on
each securable, not T-SQL's forty-odd (`ALTER ANY …`, `IMPERSONATE`, `TAKE OWNERSHIP`), which
stay a statement away in the editor. Its server level's fourteen (`CONNECT SQL` … `CONTROL
SERVER`) are curated the same way and are all present since SQL Server 2014: 2022's `VIEW SERVER
PERFORMANCE STATE` family is left out because an older server refuses it by name, and the
availability-group, endpoint and event-session permissions and `SHUTDOWN` stay a statement away
with the rest.
`PrivilegeChange` + `privilege_sql(change, dialect, revoke)` write the `GRANT`/`REVOKE` — one
struct for both directions, since what a revoke takes away is exactly what a grant gives, and
`WITH GRANT OPTION` is ignored on the revoke side rather than given a second field nobody sets.
**A SQL Server revoke always carries `CASCADE`** (`users::revoke_cascades`): without it, taking back a permission granted
`WITH GRANT OPTION` is Msg 4611 (measured), and the form cannot know how the grant it is revoking
was made; on one granted without the option `CASCADE` is accepted and does what the plain revoke
would (`a_sql_server_revoke_cascades_so_a_grantable_permission_can_be_taken_back`). **A SQL
Server table-level grant keeps the account's column denials** (`users::table_grant_lifts_column_denies`):
T-SQL deletes a column's `DENY SELECT` when `SELECT` is granted on the whole table (measured on
2022 and 2025 — the denied column became readable, under an empty risk block), so for a grant
holding a column permission (`SELECT`, `UPDATE`, `REFERENCES`; `column_permissions_kept`) the
statement is one batch that reads those `DENY` rows into a string of statements, grants, and
runs the string — read inside the plan's transaction, so it is what is there at Apply rather
than what the form saw, and with no `;` outside its literals, as `tsql_drop_default` has.
**No `CASCADE` on the re-denial**: measured, `DENY … (ssn) TO u CASCADE` also deleted a table
grant `u` had made to someone else. So where the account holds the grant option on the table
the server refuses the bare re-denial (Msg 4611) and the plan rolls back — the denial kept, the
way for this to fail — and the risk sentence says so
(`a_table_grant_keeps_the_accounts_column_denials`, live). A grant on the schema or database
leaves an object's `DENY` alone (measured), so only the table level is wrapped. **A table-level
revoke clears the columns too, and is left plain** (`users::table_revoke_clears_columns`):
`REVOKE SELECT ON OBJECT::dbo.t` deleted both a column's `DENY SELECT` and another column's
`GRANT SELECT`, while a `DENY UPDATE` on the same column stayed (measured on 2022 and 2025). A
revoke being how a permission is taken back, nothing is kept the way a grant keeps its denials;
the risk sentence names the permissions whose column rows go (`column_permissions_cleared`,
`a_sql_server_table_revoke_says_it_clears_the_columns_permissions`). **And a SQL
Server server-level statement is sent to `master`** — `EXEC master.sys.sp_executesql N'GRANT
VIEW SERVER STATE TO [app]'`, the private `tsql_in_master` — because T-SQL grants a server
permission only when the current database is `master` (Msg 4621 from any other, measured), and
an account plan runs through `Db::run_ddl` in the browser's database. `master`'s own
`sp_executesql` runs that one statement there, inside the plan's transaction — a rollback undoes
it, measured — and hands the session back, so a later statement in the same plan still runs in
the browser's database. **A `USE master` would do neither**, and is the tidier-looking spelling a
future editor should not reach for (`a_server_permission_runs_in_master_and_names_no_securable`).
The read-back (`mssql_grant_statements`, class `SERVER`) still prints the plain `GRANT … TO
[login];`, the wrapper being how the statement has to be sent rather than what was granted.
It returns `None` for an empty privilege list, because `GRANT ON db.*` is a syntax error: the
backstop *under* the form's own Apply gate, not the gate. `RoleChange` + `role_sql` are the
membership pair, and `AccountDraft` + `account_draft_sql` the `CREATE USER`/`CREATE ROLE` — a
role takes no host and no password on either engine, and an empty password emits **no clause at
all**, which is a real account on both (PostgreSQL's must authenticate some other way, MySQL's
has simply not been given one yet). **SQL Server's arms are T-SQL's own shapes, not the other
engines' with the keywords swapped.** `role_sql` is `ALTER ROLE [r] ADD|DROP MEMBER [m]` — or
`ALTER SERVER ROLE` when the member is a `Login`, the member deciding because a login belongs to
no database role and a user to no server role (`a_login_joins_a_server_role`) — a membership there
being a statement about the role rather than a grant of it, with no admin option — so
`supports_role_admin_option` is false there, `core::ddl` refuses a role grant asking for one and
the grant form hides the toggle. `role_suggestions` is that form's Role shortcut: the browser's
listed roles, or for a SQL Server login — whose roles the browser does not list — the eight fixed
server roles (`MSSQL_FIXED_SERVER_ROLES`), 2022's `##MS_…##` roles left out since an older server
refuses them by name; a user-defined server role can still be typed. `account_draft_sql` hands SQL Server to `tsql_account_draft_sql`:
`CREATE LOGIN [n] WITH PASSWORD = N'…'`, `CREATE USER [n] FOR LOGIN [l]`, a contained user's
`CREATE USER [n] WITH PASSWORD = N'…'` where there is no login but a password, `WITHOUT LOGIN`
where there is neither, and `CREATE ROLE [n]`; `drop_account_sql` writes `DROP LOGIN` for a login. **The login's password
travels as typed, as MySQL's does, and not in the hashed `WITH PASSWORD = 0x… HASHED` form**,
because the hashed form skips the server's password policy — the same trade `PasswordPolicy`
below makes on PostgreSQL, where a verifier the app computes is a decision the server did not
get to make. `PasswordReset` + `set_password_sql` are the `ALTER` beside
that pair, and `PasswordReset` is deliberately **not** a second use of `AccountDraft`: a draft
describes an account that does not exist yet and carries every field `CREATE` needs, while this
names one the browser listed and carries the single field that is changing — handing `CREATE`'s
shape to an `ALTER` is how a reset comes to reset the host as well. The statement is
`ALTER USER … IDENTIFIED BY` on MySQL and `ALTER ROLE … PASSWORD` on PostgreSQL, `ROLE` there for
a user too, since `ALTER USER` is an alias the manual keeps for compatibility and the browser's
own `GRANT` statements already read `ROLE` throughout — and `ALTER LOGIN … WITH PASSWORD =` on
SQL Server, where a reset on a **user** row alters the login it maps to (`Principal::login`),
the password being the login's; `a_login_and_its_user_are_created_granted_reset_and_dropped`
signs in with the new one and is refused with the old. A user with no login is a contained one
by the time it gets here — the gate below admits no other — and its reset is `ALTER USER [n]
WITH PASSWORD = N'…'`, pinned by `a_contained_user_is_created_and_reset_with_its_own_password`. **An empty password returns `None` rather
than emitting**, which is the one place this parts company with the clause above:
`ALTER USER … IDENTIFIED BY ''` is a legal statement that sets a *blank* password, where
`CREATE`'s missing clause leaves one unset — a lock left open against a lock not yet fitted — so
a caller who wants that has `DropAccount` or the engine's own client. An empty account name is
refused on the same call, the backstop under the form's gate that `privilege_sql`'s empty list
is. `supports_password_reset(dialect, &Principal)` is the capability, gated on
`supports_users(dialect)` rather than restating a list of
engines — an engine with accounts has an `ALTER` for them, so that half is a question already
answered — and then asking the account, per engine since SQL Server's arrived: on MySQL and
PostgreSQL `p.kind == PrincipalKind::User && !p.role_ambiguous`, a role taking no password on
either, the same rule `account_draft_sql` applies to `CREATE`; on SQL Server a `Login`, or a
`User` with a login to alter or a password of its own (`database_password`), where a role and a
user `WITHOUT LOGIN` have none — and never an account Windows or Entra signs in
(`Principal::external_sign_in`, read off the catalogue's type: a login's `U`/`G`, a user's
`U`/`G`/`E`/`X`), where `ALTER LOGIN … WITH PASSWORD` is Msg 15080 (measured on 2022 against
the container's `BUILTIN\Administrators`, which the listing shows as an ordinary login;
`a_windows_login_or_its_user_is_offered_no_password_reset`). **It takes the whole `Principal` and not its
`PrincipalKind`, because the kind cannot say "I don't know."** It asked only the kind for a
release, which is how the `role_ambiguous` finding above got in, and its own rustdoc asserted the
premise that made that look sound — that the engine *rejects* a password on a role outright,
which is true of MariaDB 10.11.14 and measurably false of MySQL 8.4.11. Both servers are named
there now, and the third term refuses where the catalogue cannot classify the row rather than
guessing at it, the way `intel`'s column resolution refuses an unresolved name. `set_password_sql`
asks the same predicate, so a plan built directly is refused on the terms the offer was.
**Both builders write the password through one private `password_literal`, and on PostgreSQL
that literal is a verifier rather than the password.** `AccountDraft` and `PasswordReset` each
carry `scram_salt: Option<scram::Salt>`; where `supports_password_verifier(dialect)` says yes
and a salt is stamped, the clause is `PASSWORD '<scram::verifier>'` and the statement PostgreSQL
logs and shows in `pg_stat_activity` carries no password (`core::scram` has the why). MySQL and
MariaDB answer no, because the server rewrites `IDENTIFIED BY` out of its own logs; SQL Server
answers no as well — it does take a hashed password for a login (`0x… HASHED`, in a format of
its own), but that form skips the password policy, which is why the login's is sent as typed
(above). The predicate is an exhaustive `match`, so a fourth engine has to answer it. **The salt is stamped by
the form, never generated at emit**, so preview and Apply run one statement and the emitter stays
pure — and `None` is the old behaviour, the password as typed, so a caller that forgets to stamp
one sends a password the logs keep rather than locking the account out. A password
`scram::verifier` declines, one SASLprep would rewrite, falls back to typed on the same terms.
**A verifier the app computes is a decision the server did not get to make, and
`PasswordPolicy` hands it back.** Measured on PostgreSQL 16.15, the SCRAM-4096 verifier this
used to send overrode three settings at once: under `password_encryption = md5` the role stored
`SCRAM-SHA-256$…` where the same `CREATE ROLE` from psql stored `md5…`, locking out every client
that cannot do SCRAM on a cluster kept on md5 for exactly those clients; a raised
`scram_iterations` came in cut to 4096; and `passwordcheck` passed a one-character password it
refuses as text, a check hook being able to judge only a plaintext password — credcheck, for its
part, refuses a pre-hashed one outright by default. So `password_literal` takes the policy and
the role's name and answers in that order: a check hook loaded (`checks_plaintext`) sends the
password as typed, so the hook the administrator installed is the one that judges it; `Md5` sends
PostgreSQL's `md5` verifier — `md5` and the hex MD5 of the password followed by the role name,
which is why the name is an argument, and a reset hashes under the *listed* account's; anything
else is SCRAM at the server's own count (`scram::verifier_with`). `PasswordPolicy::from_settings`
reads the server's spellings: `md5`, or `on` (md5's spelling before 14), is `Md5` and anything
else SCRAM; a missing, `0` or non-numeric iteration count is 4096; and a hook counts as loaded
when `passwordcheck` or `credcheck` is in `shared_preload_libraries` — a bare name, a `$libdir/`
path or quoted, while `mypasswordcheckx` is not one — or when a `passwordcheck.*`/`credcheck.*`
setting is listed, which is the half that still answers for a role that may not read the
preload list (`pg::password_policy` has why). `Principals::password_policy` carries it out of
the fetch; `AccountDraft` and `PasswordReset` carry it as `password_policy`, stamped at Preview
beside the salt and on the same terms, and **`None` is the SCRAM-4096 behaviour from before the
policy existed** — the settings did not read, or the plan was built without them. MySQL passes
`None` and takes no verifier whatever a policy says (`a_policy_changes_nothing_on_mysql`).
**Every verifier is still gated on the salt, the `md5` one included, though it uses none** — and
that is load-bearing rather than incidental: `ChangeSet::without_secrets` clears `scram_salt`
and leaves the policy, and it is the salt's absence alone that sends the placeholder out as text
under any policy. The md5 arm's oracle is `the_md5_verifier_matches_one_postgresql_built`, the
`rolpassword` PostgreSQL 16.15 stored for `correct horse ~ battery!` as `zz_md5_oracle` under
`password_encryption = 'md5'`; `md-5` became a direct dependency of core for it, the crate and
version `postgres-protocol` already had in the lock.
`password_hint(dialect)` is the line under the form's password field (its choice
`password_hint_kind`, a `PasswordHint` the interface words) and says which of the two the
preview will show: on a verifier engine a hash in the form this server stores and not the
password, unless it has characters the login would normalise (full-width letters, ligatures,
no-break spaces) or the server runs a password-check extension; elsewhere the password itself.
`a_password_saslprep_would_rewrite_is_sent_as_typed_even_when_salted` (`ｆｕｌｌ`) and
`a_non_ascii_password_saslprep_leaves_alone_is_hashed` pin the two sides of that line. The salted tests pin the PostgreSQL
create and reset to exactly `scram::verifier`'s output with the password nowhere in it, and that
a salt changes nothing on MySQL, a salted role still takes no clause and a salted blank reset
still has no statement.
`account_form_ready(draft, confirm, resetting)` is the account form's Preview gate and
`account_form_blocker` the reason under it — an `AccountBlocker`, whose `message` is the English
and which the interface words — one function behind both, so the button and the reason cannot
disagree about what is missing: a reset needs a password ("A password is
required."), a create a name ("A name is required."), and a user — never a role, which has no
password row to confirm — a second copy equal to the first ("Type the password again to confirm
it." while it is empty, "The two passwords differ." once it is not). A create may still leave
the password blank, the two empty fields matching. A SQL Server user drafted with **both** a
login and a password is held back ("A user for a login signs in with the login's password —
leave this blank."), since `tsql_account_draft_sql` would otherwise drop one of the two
silently; `login` being SQL Server's alone, no other engine's draft reaches it. Why it is typed twice is under the form.
`drop_account_sql` is the last, and like `DropDatabase` never
`IF EXISTS`: the account came off the browser's list, so one that isn't there means the list is
stale and a drop that dropped nothing is about to be reported as a success.
`GrantDraft` is the grant form's state, held here so the view holds none of it — `subject`
(`Privileges` or `Role`), `revoke`, `level`, `qualifier`, `name`, `privileges`,
`with_grant_option`, `role`, `with_admin_option`. **Two name fields for five levels**, because
that is how the levels nest: a database and a schema each name one thing, a table and a sequence
name one thing *inside* another, and the whole-server level names none. `level()`, `change()` and
`role_change()` answer what it describes, and `is_ready` is the **single completeness question
Apply reads**, so a form that cannot produce a statement cannot offer one — the alternative is an
enabled button whose plan turns out to be `GRANT ON …`, refused a second time much further from
the user. `toggle` keeps the ticked privileges in the engine's own documented order however the
boxes were clicked, so two people who pick the same set get the same statement.
**Object names in these statements go through `export::ident_sql`** and its unconditional
quoting, because this is SQL that is *executed* — the opposite call to the browser's read-only
`GRANT` list and its `ident_if_needed` (`pg_ident`), which is the same distinction one paragraph
up. Accounts themselves still go through `account_sql`. `GrantLevel::object_sql` — everything
after `ON`, keyword included — is **the file's only dialect match and it names every arm**,
with no `_ =>`: a catch-all there answers for an engine nobody has looked at, so a fourth engine
would silently take PostgreSQL's spelling rather than failing to compile. SQLite reaches it only
through a caller that skipped `supports_user_admin`, which is what the arms are for.
**One position in that statement is not read as a name, and quoting could not save it.** MySQL
and MariaDB document the database in a `GRANT … ON db.*` as a `LIKE` pattern — `_` matches any
single character, `%` any sequence — and a backtick does **not** suppress it; the manual's own
escaped example is itself backticked. So ``GRANT SELECT ON `app_db`.* TO …`` also granted on
`appXdb`, measured on MariaDB 10.11.14 and MySQL 8.4.11 where an account granted on
`sc_rev_a_b` read rows out of `sc_rev_aXb`, and nothing afterwards revealed it because
`SHOW GRANTS` prints the stored pattern straight back unescaped. Underscored database names are
the ordinary case, not the exotic one. So the MySQL `Database` arm calls
`export::ident_pattern_sql` — the third member of the quoting family, stated in full under
*Architecture invariants* — and the `Table` arm deliberately keeps `ident_sql`, the table level
being exact-matched on both servers (a qualified `db`.`tbl` grant answers ERROR 1142 rather than
reaching the pattern's other matches).
**Two decisions the browser used to hold live here now.** `WriteGate::of(dialect, read_only,
has_database)` answers whether a write is offered and, if not, why — `NoEngineSupport`,
`ReadOnly`, `NoDatabase`, `Allowed` — and the *ordering* of those four answers is the whole
content of it: engine first, because an engine with no accounts must **omit** the action rather
than dim it (`offered()`), and read-only before no-database, because a read-only connection is
the more specific reason and the one the user can act on. It lived inline in an 860-line view
with no `#[cfg(test)]` at all, so that ordering had nowhere to be pinned. `filter_indices`
returns the *indices* matching the search box in list order, computed once: the view asked
`matches` twice per keystroke — once to build the rows and once for the footer's count — each
call re-`format!`ing every account's `display()`, which at the ~1,000 accounts a shared server
has was measured at ≥13 ms of a 16.7 ms frame, behind a query with no `LIMIT`.
