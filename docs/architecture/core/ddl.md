# `core/ddl.rs`

`ddl.rs` — **schema editing**, and every engine now reshapes a table, edits a view and edits a
trigger. `supports_schema_editing(dialect)` is **gone**: it would have become "always true", and
a vacuous predicate invites deletion for the wrong reason. The capabilities it used to answer
for came apart into `supports_view_editing` and `supports_trigger_editing` — and both of those
have since gone true everywhere too. They stay because the question is per *object* and the
menus ask it per object, and each **computes** its answer out of `supports_change` instead of
stating it: a predicate that discards its `SqlDialect` and returns `true` is the constant the
*"ask a capability"* rule exists to prevent, wearing the name of the fix for it, and it answers
for a fourth engine as confidently as for these three. Two more join them.
`supports_table_design` is what the tree's **Edit table**, **Edit column** and **Edit index**
entries ask — three literal `true`s until it existed — and its probe is a column retype, since
that is the designer's own edit: expressible in place on MySQL, PostgreSQL and SQL Server
(T-SQL's `ALTER COLUMN`, `emit_mssql`), reached through the rebuild on SQLite, and either route
counts (`table_design_is_offered_exactly_where_a_retype_emits`, over `SqlDialect::ALL`). `supports_column_reorder` is the one where the
engines genuinely disagree: MySQL places a column with `AFTER`, SQLite is created in the draft's
order by the rebuild it is already doing, SQL Server the same way through its own rebuild
(`tsql_rebuild_sql`, below — it answered no until that existed), and PostgreSQL cannot move one
at all. It is an
exhaustive `match` rather than the `!= Postgres` it was spelled as at *both* its sites — in
`diff`, which must not raise a move PostgreSQL has no statement for, and again on the designer's
arrow buttons, which must not offer one. `supports_sequence_resync` is the newest of the family
and the only one no editor asks: does an auto-increment column draw from a **separate counter** a
restore has to be told about? PostgreSQL's does — a `serial` or identity column is backed by a
sequence that explicit inserts leave where it was — while MySQL's `AUTO_INCREMENT` and SQLite's
`rowid` both advance from the data already in the table, so there is nothing to resync;
`dump::sequence_resync_sql` is the one caller, and this replaced the `!= Postgres` it shipped as.
**One member of the family answers with a *value* rather than a yes.** `default_new_column_type`
is the placeholder type the designer drops into a new column's field before the user has picked
one: `varchar(255)` on MySQL and SQLite, `text` on PostgreSQL. The difference is not house style
— on PostgreSQL `varchar(255)` is a limit the **server enforces**, on a column the user has not
finished describing, while `text` there is the same storage with no limit; SQLite keeps MySQL's
spelling because its declared length is a note rather than a promise, which is what
`enforces_declared_byte_length` says just below — the parameter is ignored outright and
`varchar(255)` is TEXT affinity, so the two spellings mean the same thing to that engine. It is
an exhaustive `match` rather than a capability for `intel::builtin_catalog`'s reason: *which type
name* has no yes/no behind it to compute from, so a fourth engine has to be answered for instead
of inheriting whichever arm a comparison left open. It replaced an `== SqlDialect::Postgres`
inside `table_designer::default_type`, which is gone;
`a_new_column_starts_at_the_engines_own_string_type` asserts the literal strings rather than that
they differ, since a test saying only "PostgreSQL is not MySQL" passes on any two values.
**Four more came with SQL Server's Create table**, each an exhaustive `match` the designer asks
where it had compared engines or asked nothing, and each for a field it offered and SQL Server
could not take.
`fk_actions(dialect)` is the action dropdown's list — and `propose::is_fk_action`'s, under
`propose.rs` — `FK_ACTIONS` less what two capabilities withhold. `supports_fk_restrict` is false
on SQL Server, whose refusing action is `NO ACTION` — already the first entry — so the other was
an entry whose every use is a syntax error. `supports_fk_set_default` is false on MySQL/MariaDB,
and there the failure is quieter: InnoDB performs no `SET DEFAULT` on either server and refuses
the clause on neither — MariaDB 10.11 stores `RESTRICT` in its place, MySQL 8.4 stores the
clause and then refuses the parent's delete as `RESTRICT` would (both measured) — so "Set
default" applied cleanly and enforced the opposite of the choice. `supports_comments(dialect)` is false on SQLite alone, which has no
comments in the language, so both comment fields are hidden there rather than typed into and
dropped. It was false on SQL Server too until the emitter wrote that engine's comments, which
are the `MS_Description` extended property — a statement of its own, `TsqlComment`, under
`mssql.rs`. `identity_wording(dialect)` is the key toggle's label and
hint, naming the keyword `definition_sql` writes — it read *Auto-increment (AUTO_INCREMENT)* on
SQL Server. `supports_index_prefix(dialect)` is MySQL's alone, and the index key hint names the
`bio(20)` prefix syntax only there; it named it on every engine but PostgreSQL.
`supports_partial_index(dialect)` is true on PostgreSQL, SQLite and SQL Server — each a partial
(or filtered) index whose `WHERE` `create_index_sql` writes and whose introspection reads the
condition back — and false on MySQL, which has none. It is necessary, not sufficient: the index
form's *Only rows where* field and `validate` ask `index_takes_filter(ix, dialect)`, which adds a
kind that can be filtered (`IndexStorage::takes_filter`) and refuses two shapes the plan could
not carry. One is a unique constraint's index where `constraint_takes_filter` says no — SQLite
alone, whose constraint index lives in the table body where no `WHERE` goes, so its rebuild kept
the constraint and dropped the condition without a word; PostgreSQL and SQL Server turn a
filtered constraint into a filtered `CREATE UNIQUE INDEX`. The other is a clustered index, which
covers every row (Msg 102 at the `WHERE`, measured on 2025)
(`a_filter_is_offered_only_where_the_index_can_carry_one`,
`a_filter_an_index_cannot_carry_is_refused`). `supports_index_method(dialect)` is PostgreSQL's alone, where the access method is a free
choice among installed ones, and the *Method* field asks it. The two were one PostgreSQL-only
block until the condition field was split off for the engines it serves
(`a_partial_index_is_offered_where_the_emitter_writes_its_where`).
`supports_index_include(dialect)` sits beside `supports_index_prefix`, true on SQL Server and PostgreSQL and false on
MySQL and SQLite, which have no `INCLUDE` clause: the index form builds its *Include* field only
where it is true, and `IndexInfo::include_sql` writes nothing where it is false (below, and
under `schema.rs`). It is necessary, not sufficient: the field asks `index_takes_include(ix,
dialect)`, which adds a kind that keeps a list (`IndexStorage::takes_include`) and refuses two
shapes SQL Server rejects — a unique *constraint* where `constraint_takes_include` says no, and
a clustered index, whose leaf is every column already (Msg 10601). `constraint_takes_include`
is PostgreSQL's alone, whose `UNIQUE (…) INCLUDE (…)` exists; T-SQL has no `INCLUDE` on a
constraint (Msg 102), and `emit_mssql`'s constraint re-add dropped a typed list while the
preview said *"including"*. `key_clusters_by_default(dialect)`, SQL Server's alone, asks whether
a key with no stated clustering is the table's clustered structure — T-SQL's `PRIMARY KEY` is
`CLUSTERED` unless told otherwise — and `validate` asks it to count those structures (below).
Both are exhaustive `match`es.
`supports_columnstore(dialect)` is SQL Server's alone: the index form builds
its *Storage* choice only there, and only `create_index_sql`'s T-SQL arm writes `COLUMNSTORE`
(below). `supports_xml_and_spatial_indexes(dialect)` is its sibling for the XML and spatial
kinds, also SQL Server's alone, and `IndexStorage::offered` asks each family its own predicate
rather than one standing in for both. `supports_columnstore_order(dialect)` asks whether a
columnstore index can carry an `ORDER (…)`: the designer builds its *Order* field and
`create_index_sql` writes the clause only where it is true. It is *computed* —
`supports_columnstore(dialect) &&` an exhaustive `match` with `MsSql` alone on `true` — the
shape the capability rule asks of a predicate that answers the same as another today, rather
than that other predicate under a second name. Which server *version* takes one (2022 for a
clustered index, 2025 for a nonclustered one, which 2022 refuses by name, Msg 35342) it does
not ask: that is the server's question, asked by the server.
**Three more answer for things outside the designer entirely.** `enforces_declared_byte_length`
asks whether a column's *declared* type binds how many bytes a value in it may hold: MySQL's is a
promise, enforced with `ERROR 1406: Data too long`; PostgreSQL's `bytea` declares no length to
read, which is the same answer from the other side; and SQLite's is a **note**, that engine
typing values rather than columns, so `VARBINARY(16)` there stores a megabyte happily and `BLOB`
is bounded only by `SQLITE_MAX_LENGTH`. `blob::column_byte_cap` has to ask it *before* it reads a
type name — its table is MySQL's four blob families, and applied to SQLite it capped a `BLOB` at
65,535 bytes and a `VARBINARY(16)` at sixteen. `schema_body_is_emittable` asks whether a routine,
trigger or event body that an **eager** `fetch_schema` read can be re-emitted verbatim: no on
MySQL, whose `information_schema` resolves the escapes, yes on PostgreSQL (`pg_get_*`) and SQLite
(`sqlite_master.sql`), where the eager read is the authority. `compare::CompareEntry::needs_source`
is its one caller, and it replaced the `dialect == SqlDialect::MySql` that question was spelled as.
`publishes_index_ddl` is the newest of the three and asks whether the engine hands back a whole
`CREATE INDEX` per index, so an index the model only **partly** read can be replayed verbatim
instead of reconstructed from the parts that were read: yes on SQLite (`sqlite_master.sql`) and
PostgreSQL (`pg_get_indexdef`), no on MySQL, whose `SHOW CREATE TABLE` inlines the keys and
leaves a partly-read index there with nothing to fall back on but a refusal. It is the question
behind `IndexInfo::create_sql`, asked by name where a caller needs to know *before* looking, and
its one caller today is the live tier's `a_partly_read_index_says_so_and_is_emitted_whole`, which
gates on it rather than on the engine — the shapes it plants are PostgreSQL's, that being the
only leg answering yes.
**Two more answer for the *comparison* rather than for any editor**, and both are about the
difference between an object and its address. `ref_schema_is_database` asks whether the namespace
a foreign key reports (`ForeignKeyInfo::ref_schema`) is the **database** the key lives in rather
than a namespace inside one: true on MySQL and MariaDB, whose
`KEY_COLUMN_USAGE.REFERENCED_TABLE_SCHEMA` *is* the database, so a key to a table in its own
database still names that database; false on PostgreSQL, whose namespace is part of the object,
and on SQLite, which has neither. `view_definition_is_qualified` asks whether the catalogue hands
a view's body back **rewritten and qualified** with the database it lives in — MySQL's
`VIEWS.VIEW_DEFINITION` turns `CREATE VIEW v AS SELECT id FROM t` in `shop` into
``select `shop`.`t`.`id` AS `id` from `shop`.`t` ``, while `pg_get_viewdef` rewrites but
qualifies with the *schema* and SQLite stores the statement verbatim. Both are exhaustive
`match`es, both are asked by `compare::SchemaComparison::of`, and the consequence of
getting either wrong is written down where it lands: two identical databases differ in every
object holding one, and the migration carries the right database's name into a statement that
runs against the left. `ref_schema_is_database` is not the comparison's alone: `dump::plan`,
`schema::follow_target`, `incoming_keys` and `referencing_target` ask it too, and so does
`used_by::used_by`, which reads a key's `ref_schema` equal to the database it was handed as that
database. **A qualifier in a body is a third predicate's, `body_qualifier_is_database`**, and
not this one flipped for SQLite: SQLite's `main.orders` names a database while its keys carry no
`ref_schema` at all, so the body answer is MySQL's and SQLite's and the key answer MySQL's alone
— asked by `used_by` only (under `used_by.rs`). Beside them, `rename_follows(dialect,
DependentKind)` asks which dependents an in-place rename carries along; it is `used_by`'s too.
**One more asks where a bare name resolves, which decides how a foreign key's target is
*written*.** `bare_reference_is_own_namespace` asks whether a bare table name in a statement
about a table resolves in that table's own namespace, so that a key whose
`ForeignKeyInfo::ref_schema` is `None` — which the model, and `fks_equal`, read as "the same
one" — may go out unqualified: true on MySQL, where a bare name is the connection's database and
a key's `None` means this database, and on SQLite, which has one namespace; false on SQL Server,
where a bare name resolves in the *login's* default schema and then `dbo`, never in the altered
table's, and on PostgreSQL, where it resolves through `search_path`. `fk_clause(fk, owner,
dialect)` takes `owner`, the namespace of the table the key stands on, and writes a `None` key
qualified with it wherever the answer is no (`public` included, since S2-L5-02) — `emit_mssql`,
`emit_mysql` and `emit_postgres` pass the change set's schema, `create_table_sql` the draft's,
and the rebuild's inbound keys their own table's. The designer's foreign-key picker lists only
the designed table's schema and writes only `ref_table`, so on SQL Server a key added to
`sales.orders` naming `customers` bound to `dbo.customers`: orders for `sales` customers were
refused, and under `ON DELETE CASCADE` a delete in `dbo.customers` deleted `sales` orders
(S3.1-L1-02, measured on 2022). A new table's keys and an AI proposal's `NewForeignKey` with no
`ref_schema` went out the same way, and PostgreSQL had the same bare spelling from before.
`a_foreign_key_with_no_namespace_names_the_tables_own` pins the four answers and both spellings
— an added key and a created table's; the live
`a_designer_key_outside_dbo_references_its_own_schema` runs both doors against the server, on
2022 and 2025.
**One more answers for a trigger's *identity*, which is the one place the three engines genuinely
disagree about it.** `trigger_names_are_schema_scoped` asks whether a trigger's name has to be
unique across the whole schema rather than only within its own table — measured on all four
servers with `new_trigger` already sitting on a sibling table in the same database: MariaDB
10.11.14 `ERROR 1359: Trigger 'db.new_trigger' already exists`, MySQL 8.4.11 `ERROR 1359`,
SQLite 3.45.1 `trigger new_trigger already exists`, PostgreSQL 16.15 **accepted, both triggers
created**. `TriggerSetDraft::validate` states the same divergence and deliberately checks only
the narrow half, because one modal holds one table's set and that is the only scope it can see;
this predicate is asked by whoever *proposes* a name and has the wider list
(`ui::TriggerTarget::sibling_triggers`), so the `+` button stops offering a name the server will
refuse at apply — after any `DROP` in the same set has already committed on MySQL. Exhaustive
`match`, not a comparison: it is a fact about each engine's namespace rather than a capability
computed from a statement, so there is no `supports_change` to derive it from.
`supports_database_editing` and `supports_namespace_editing` are the newest pair, for the
**container** the rest of this module's objects live in: `CREATE`/`DROP DATABASE` on MySQL,
PostgreSQL and SQL Server, and PostgreSQL's and SQL Server's `CREATE`/`DROP SCHEMA`. They are two predicates
rather than one because MySQL answers them *differently* — its `CREATE SCHEMA` is a synonym for
`CREATE DATABASE`, one level up from what the change means, so a single "supports containers"
answer would have offered `Create schema` there and quietly made a database. SQLite answers no
to both: a database there is a file, so a create is the connection form's business and a drop
would be deleting the user's file off disk — absent rather than dimmed, since there is nothing
another connection could enable. **The namespace one is deliberately not called
`supports_schema_editing`**: that name belonged to the predicate deleted two paragraphs up,
which meant the whole of this module rather than one statement in it, and the same name for
two opposite scopes is worse than a longer one. The menu entry is still labelled `Schema`,
because that is what PostgreSQL and SQL Server call it.
The four changes they gate (`CreateDatabase`, `DropDatabase`, `CreateSchema`, `DropSchema`)
are the only ones in the module that **do not address `ChangeSet::table`** — each carries its
own name, `ddl::server_level` is the constructor that builds such a set, and
`container_creates`/`container_drops` are the one emitter pair that reads neither `table` nor
`qname()` (`a_container_change_ignores_the_change_sets_subject` is the pin, since an emitter
reaching for `qname()` like every sibling would address the wrong object while the change
list still read right). **They are two functions emitted at opposite ends of the plan, and
the asymmetry is the point**: a container has to exist *before* anything inside it is created
and survive *until* everything inside it is done with, so a `DROP SCHEMA` at the front would
take the namespace away from every later statement naming `sales.orders`. Written as one
front-loaded loop first, with a comment claiming the position was universally safe — which is
the kind of claim a later edit builds a real bug on;
`a_container_is_created_first_and_dropped_last` hand-builds the mixed set nothing produces
yet. `is_server_level` splits them again along a different seam — the database pair
runs on a connection that names no database (on SQL Server, one attached to `master`) and
outside any transaction, the namespace pair on
the ordinary in-database path — which is what `DdlScope` and `Db::run_server_ddl` are for.
`DROP SCHEMA` is never `CASCADE`, the same call `DropObject` makes: cascading drops every
table in the namespace, so the server is left to refuse and name what is still in there.
**SQL Server takes the namespace pair through the same two functions**, at the same two ends of
`emit_mssql` as of `emit_mysql`/`emit_postgres` — `container_creates` ahead of even
`tsql_in_place_guard`, `container_drops` after `account_statements` — and needed nothing of
its own from them, because `CREATE SCHEMA [sales];` is one statement and so its own batch in
`mssql::run_ddl`, which T-SQL requires (`CREATE SCHEMA` must be first in its batch); it runs
inside the plan's transaction, which T-SQL allows. There is no `AUTHORIZATION`, since
`supports_owners` is false there and the form offers no owner, and T-SQL has no `CASCADE` to
leave out — so the drop's warning (`Change::risks`) does not claim to withhold one there:
*"Schemaic never sends CASCADE"* is added only where `supports_drop_schema_cascade` answers
yes, PostgreSQL alone (`the_drop_schema_warning_names_cascade_only_where_it_exists`), the rest
of the sentence — the server refuses while the schema holds anything — being every engine's
(`sql_server_creates_and_drops_a_namespace`,
`sql_server_creates_the_namespace_before_the_table_in_it`; live, on SQL Server 2022 and 2025,
`a_namespace_is_created_with_its_table_and_dropped_only_when_empty`, whose drop of a schema
still holding its table is refused and leaves both).
**It takes the database pair as well**, through the same two functions and
`DatabaseDraft::create_sql`, and `tsql_supports` admits both, so `supports_database_editing`
answers yes there and the gear's *Create database*, the Create menu's *Database* and a database
node's **Drop** are offered (`sql_server_creates_and_drops_a_database`); the run is
`mssql::run_server_ddl`'s. What it brought with it is the one option it takes: a **collation**,
and no character set, its collation naming the code page. That is why
`supports_database_collation` (MySQL and SQL Server) is a capability of its own beside
`supports_database_charset`, which stays MySQL's alone — one predicate for the pair would either
have offered SQL Server a character set or withheld its collation
(`a_collation_is_offered_where_a_database_takes_one`). `database_collations(dialect)` is the
shortcut list the form offers beside the field — `MYSQL_COLLATIONS`, `MSSQL_COLLATIONS`, or
nothing where a database takes no collation — and SQL Server's five open on
`SQL_Latin1_General_CP1_CI_AS`, the default an English-locale install is set up with, with the
server's own `sys.fn_helpcollations()` the authority on the rest.
**T-SQL's `COLLATE` takes a bare name and nothing else**: `COLLATE [x]` and `COLLATE 'x'` are
both Msg 102 (measured on 2022), so this is the one name `create_sql` writes unquoted — MySQL's
stays a string literal, the form `mysqldump` writes. Don't "fix" it by routing it through
`ident_sql`; the brackets are the syntax error. Unquoted means gated instead:
`is_collation_name` (ASCII letters, digits and underscores, non-empty — which every collation
on every engine that takes one is) must hold, or **no clause is written at all** rather than
text spliced into the statement; and so the dropped clause is never a silent one,
`DatabaseDraft::validate` refuses the same name in the form's words — *"A collation name holds
only letters, digits and underscores."* — the one option a client can judge, where every other
is the server's question (`a_collation_is_a_bare_name_and_nothing_else_is_one`, whose collation
ends `; DROP DATABASE x`; `every_offered_sql_server_collation_lands_in_the_clause`). Each of
`create_sql`'s clauses is gated on the capability that answers for it rather than on the field
being set, since a draft outlives the form it came from: a draft carrying a character set, a
collation and an owner emits for SQL Server with the collation alone.
**Seven account changes join them in not addressing `ChangeSet::table`** — `CreateAccount`,
`SetAccountPassword`, `DropAccount`, `Grant`/`RevokePrivileges` and `Grant`/`RevokeRole`, the
Users and privileges
browser's write half — built by `ddl::account(subject, dialect, change)`, a sibling of
`server_level` whose `subject` is the account's display name (`app@%`, or a bare role) and lands
in `ChangeSet::table` only so the preview's title names what the plan is about; each change
carries its own account. `ddl::accounts(subject, dialect, changes)` is the same for several
changes about one subject in one plan, which only SQL Server's form raises today: a new login
and the user it brings (`users::companion_user_draft`), two `CreateAccount`s that
`account_statements` emits in the order they were given, the login first.
`is_account_change` groups them because every downstream question is
the same question for all seven, and the load-bearing answer is that **they are not
`is_server_level`**. An account belongs to the server, but the server-level route deliberately
connects to no particular database (on SQL Server, always `master`), and a PostgreSQL `GRANT SELECT ON TABLE public.users` names
an object in *one database's* catalogue — it would grant on whatever the maintenance database
happens to hold, or fail. So they take the ordinary in-database route, in the database the
browser is showing privileges for, which is correct on both engines: MySQL's grant tables are
server-wide and answer the same from any connection, and PostgreSQL's are exactly the ones the
browser was already scoped to (`an_account_change_takes_the_ordinary_in_database_route`). On
SQL Server that database is where half of them live: a database user, its permissions and its
role memberships belong to one database, the browser's, while a login is the server's.
`supports_change` answers for them in an early arm asking `users::supports_user_admin`, so it
and the browser that offers the action cannot drift about which engines have accounts at all —
**and then asks `users::levels_for` for the two changes that carry a level.** That arm used to
answer for all six variants on "does this engine have accounts", which made `unsupported()`
blind to the one thing an account change can ask an engine for and not get: `GRANT … ON *.*` has
no PostgreSQL grammar and `ON SCHEMA` has no MySQL one. Blind meant no INCOMPLETE header, a
silent `apply` guard, and Apply enabled over a statement the server will refuse. Every other
narrow arm in that function is shape-aware; this is the capability that already knew the answer,
consulted until now only by the form's own picker. **A third narrowing followed, and it is about
the account rather than the engine**: `SetAccountPassword` asks `users::supports_password_reset`
with the whole `Principal` it carries, because on MySQL 8 the catalogue cannot tell a role from a
locked account and the change is otherwise `true` for a plan `set_password_sql` will decline to
write — exactly the degradation the paragraph above describes, no INCOMPLETE header and Apply
enabled over a plan that emits nothing at all. The browser withholds the button and
`account_editor::open_for_reset` refuses the door, so this is the *third* gate rather than the
only one, which is why it is here and not left to those two. **The arm is its own function now,
`account_change_supported`, because SQL Server could not reach it inline**: `supports_change`
hands SQL Server to `tsql_supports` before anything else, so an account arm further down was
unreachable for the one engine that needed it, and `tsql_supports` asks the same function
rather than a second copy of the answer. SQL Server also brought **the grantee** into it —
`GrantPrivileges`/`RevokePrivileges` ask `users::levels_for_account` whether the account holds
the change's level, where they asked `levels_for` of the engine and `supports_grant_to` of the
account, since a login holds only server permissions and a user only database ones, each the
other's Msg 15151 (`a_sql_server_grant_is_withheld_at_the_other_kinds_level`), and
`GrantRole`/`RevokeRole` ask `supports_grant_to` of the member — and **the admin option**, refused on a role grant where `users::supports_role_admin_option`
says the engine's membership has none.
`ChangeSet::account_statements` emits them at the **end** of the plan, called from `emit_mysql`
and `emit_postgres` beside `container_drops`, and last in `emit_mssql` too — it first sat after
the routines and before the views there, which would have granted on a view before creating
it (`a_sql_server_grant_follows_what_the_plan_creates`) — and filtered on
`supports_change` like its
neighbours — a privilege is stated *on* something, so it comes after whatever the plan creates.
Within the group the order is **create → set password → grant → revoke → drop**, the only one
that composes: an
account has to exist before it can be granted anything and has to still exist while it is.
`an_account_is_created_before_it_is_granted_and_dropped_after` hand-builds the mixed set nothing
produces yet, exactly as `a_container_is_created_first_and_dropped_last` does one level up.
`SetAccountPassword` is the **one `ALTER` on an account this module emits**, and it takes the
second slot because it names an account that already exists: nothing above it is a dependency,
so it sits there only to make a plan that does both read in the order the statements happen. It
is deliberately **not** in `risk_is_reversible`. Nothing is destroyed and the account keeps every
privilege, which is exactly what makes it read mild — but the old password cannot be recovered by
this app or by the server, so every client still configured with it is broken until each one is
found and changed by hand, and that is the sentence `risks` writes. Its `summary` names the
account through `Principal::display`, host included, because on MySQL `app@%` and `app@10.0.0.%`
are two accounts with two passwords and a line naming only `app` would not say which one moved.
`line`'s arms and the grant and revoke `Risk`s carry their privilege list as
`PrivilegeWords::of` (`PrivilegeWords::message` is its English), which names them while
there are three or fewer and counts them beyond: eighteen is a legal selection at MySQL's
database level, and the summary is one line above Apply. An empty list reads **"no privileges",
not "nothing"** — `users::privilege_sql` emits no statement for one, so the preview headed a plan
"1 Change", listed it, and showed an empty SQL box under a dimmed Apply: a change described with
nothing behind it. The form's Apply asks `GrantDraft::is_ready` first, so this is reachable only
by a caller that builds the change directly. `risks` speaks for `DropAccount`,
`RevokePrivileges`, `RevokeRole`, **`CreateAccount`**, `SetAccountPassword` and — narrowly —
`GrantPrivileges`, and
the account drop's sentence says
what is actually lost — **its privileges, not its data**, with no record of them left anywhere
to put back — plus the surprise that anything still connected as it keeps running until it
disconnects. **On SQL Server a user's sessions do not keep running** but fail at their next
statement in that database (Msg 916 — measured on 2022 and 2025 with a session that waited
through the `DROP USER`), so the clause asks `users::drop_user_cuts_off_sessions`. **A SQL
Server login's drop has a sentence of its own**, since both halves of that are false for it
(measured on 2022 and 2025): the server refuses the drop while the login has a session, running
or idle (Msg 15434), and its users in each database are left behind with every grant, mapped to
no login, which a login re-made under the name (a new SID) does not re-attach to. What it does
lose is its own — every server permission and server-role membership, `VIEW SERVER STATE` and
`dbcreator` in the measurement, recorded nowhere else and absent from a login re-made under the
name — and the sentence says so, as the shared one it replaced had. The arm asks the account's
kind, a login being SQL Server's alone
(`a_sql_server_login_drop_says_it_is_refused_while_connected_and_orphans_its_users`). **The
revoke's sentence grows three clauses where the engine has the facts**, each a capability in
`users`: `revoke_cascades` — the revoke carries `CASCADE`, so the privilege is also taken from
every account the grantee granted it on to — `supports_deny` — revoking a privilege the
account is *denied* lifts the `DENY`, which gives access back through any role that grants it
— and, on a table, `table_revoke_clears_columns` — every column-level grant and `DENY` of a
column permission goes with it. All three are SQL Server's and all were measured there, where
the sentence said only "takes X away"
(`a_sql_server_revoke_says_it_cascades_and_can_lift_a_deny`,
`a_sql_server_table_revoke_says_it_clears_the_columns_permissions`). **Creating an account destroys nothing and is still the highest-consequence thing
this module emits**, which is the call `DropCheck`'s "rows the constraint refused are accepted
from now on" already makes: a blank password on MySQL is an account anyone who can reach the
server can log in as, with no `IDENTIFIED BY` for `validate_password` to fire on, and a blank
host means `%` — every machine on the network. The form discloses the host default and what of
the password the preview shows (`users::password_hint`); nothing said what leaving it *blank*
produces. **The password sentence is asked of the engine** (`users::blank_password_signs_in`),
and SQL Server answers no: a `FOR LOGIN` user is reached only through its login's password, a
`WITHOUT LOGIN` user cannot sign in, an Entra user is Entra's, and the sentence's advice — set
one "with ALTER USER afterwards" — is Msg 33234 outside a contained database (measured on 2022
and 2025). It had sat on every SQL Server user, the companion user the New account form brings
by default included, so the engine's commonest plan previewed a false alarm. **The host sentence
is asked the same way** (`users::accounts_have_hosts`, MySQL's alone), and so are the draft's
principal (`AccountDraft::principal`, which writes `%` for a blank host) and the form's Host
field — the three had been `dialect == SqlDialect::MySql` each, the shape a fourth engine falls
through (`a_blank_host_is_percent_and_said_to_be_only_where_accounts_have_hosts`).
**One grant carries a consequence too, where no grant carried one at all.** Every arm above
describes what a plan takes away or rewrites; a `GrantPrivileges` whose level is
`GrantLevel::Global` is the case where what it *gives* is the risk. `GRANT DROP ON *.*` reaches
every database on the server — including databases that do not exist yet, which is the half the
sentence says out loud — and it showed an empty risk block two entries from a revoke of `SELECT`
on one table that warned. It was also the shortest path through that screen, the form having
opened pre-set to that level (`users::default_grant_level` is the other half of the fix). **The
sentence is per-dialect**, because the level is not one reach: on SQL Server `Global` is a
login's server permission (`VIEW SERVER STATE`), which reaches the instance rather than every
database's rows, so it reads "on the whole server — the instance itself, not the database being
browsed". MySQL's sentence there would tell every grant to a login — which opens on that level
— that it reached databases that do not exist yet, naming the wrong reach. It still
warns, since the grant is still server-wide
(`a_sql_server_server_grant_names_the_server_not_its_databases`). The arm
is narrow on purpose: a grant at a named database or table is scoped to a thing the user typed,
and warning every time would train the block to be ignored where it means something. **The one
named-level grant with a sentence is a SQL Server table grant that keeps its columns' denials**
(`users::column_permissions_kept`, below): its statement is not the one the form described, so
the sentence says what the extra lines do and when the server refuses them
(`a_sql_server_table_grant_says_it_keeps_the_column_denials`). So the risk
block
can no longer head itself "This can't be undone": `ChangeSet::risk_heading` reads **"Before you
apply"** when every change in the set is `risk_is_reversible` — the two revokes, the create and
now the grant,
a deliberately narrow list so a fourth change inherits the strong heading rather than losing it
by omission. The grant joins it for the revoke's reason read backwards: it destroys nothing and
is undone by revoking it, only the whole-server case and that table grant have a risk sentence
for the heading to head at all, and "This can't be undone" over a widened privilege would spend, on the one plan that is
genuinely a keystroke from being taken back, the heading `DROP USER` needs to keep.
A revoke's own sentence says it destroys no data and is undone by granting it back,
and it appeared under the strong heading two entries away from `DROP USER`, which is the one
modal where that heading has to keep its meaning. It is here rather than in the view because it
is a claim about the changes. **A SQL Server revoke is not on the list**, since that reason is
false there: its `CASCADE` takes the privilege from accounts the grantee granted it to and its
revoke lifts a `DENY`, and granting it back restores neither — over a lifted `DENY` the grant
widens access further — so `risk_is_reversible` takes the dialect and a revoke is reversible
only where neither `users::revoke_cascades` nor `users::supports_deny` holds
(`a_sql_server_revoke_is_headed_as_one_that_cannot_be_undone`).
**What leaves the preview is `ChangeSet::export_script`, and it carries no password.** A
`CREATE USER … IDENTIFIED BY 'hunter2'` was the one plan this app emits that holds a plaintext
credential — `ALTER USER … IDENTIFIED BY` is the second — and both exits from the modal put it
somewhere durable: *Open in editor* makes a
query tab whose text the next session save writes into `tabs.json` in the clear and restores on
every launch, and *Copy* puts it on the OS clipboard, which on Windows persists in Clipboard
History and cloud-syncs. Three doc sites asserted the opposite ("never persisted, never
logged"). One function, so a third exit inherits the rule instead of remembering it — the shape
the write guard's own invariant exists to enforce. The substitution happens **before** the
emitter runs, not by scanning its output: `without_secrets` clones the set with
`PASSWORD_PLACEHOLDER` in place of each secret and re-emits, so there is no scanning to get
wrong and no second emitter, and the placeholder is shaped so the statement still parses and
fails at the server rather than quietly creating an account whose password is that string. A
header on the script says the password is missing and what to replace. `without_secrets` is
separate so the *decision* — which changes carry a secret — is a pure function with a test
(`every_account_change_that_carries_a_password_is_scrubbed`), which a seventh such change has to
extend — **and the seventh has since arrived and been caught by it**: `SetAccountPassword` went
in with its emitter and its fixture and without its arm here, and that test named the leaking
statement. **The salt goes with the password**: the scrub clears `scram_salt` as well, because
the clone is re-emitted and a salt left on it hashed the *placeholder* into a verifier — the
script said "Replace PUT-THE-PASSWORD-HERE below" over a statement with nothing to replace, and
running it would have set the password to that text. A verifier is not for a clipboard in any
case, being enough to brute-force the password offline. The `password_policy` beside it is left
in place, and clearing the salt is still enough: `users::password_literal` builds *every*
verifier only under a salt, the `md5` one included though it uses none.
`a_salted_password_exports_the_placeholder_and_no_verifier` was seen failing with exactly that
output before the fix. The preview itself still renders `statements`, which is the statement
that runs: a modal showing a blanked-out plan would not be showing the plan.
`grant_change(draft, account)` is the last piece: the mapping from the grant form's Action and
Subject dropdowns (two toggles when the test was named) to those four statements, out of the
view because a mapping invisible in a rendered form is the
kind that ships backwards (`the_forms_two_toggles_choose_between_exactly_four_changes`).
`every_account_change_the_engine_accepts_emits_a_statement` is a hand-written list, extended on
purpose when a seventh arrives, and `an_account_change_sqlite_refuses_is_withheld_not_silent` is
the other side of it. **What holds that list to the variants that exist is
`every_account_change_is_in_the_list_the_account_tests_walk`**, which counts discriminants rather
than trusting the fixture: hand-written means a new variant has to be added on purpose, and
nothing otherwise stops three tests passing over six while a seventh walks past them. It found
the literal still reading six when `SetAccountPassword` landed. The fixture for that arm carries
a real password where `CreateAccount`'s is blank, because `set_password_sql` refuses an empty one
— a blank fixture would emit nothing and read exactly like an arm nobody wrote.
What actually varies for views moved down a level, into
two narrower facts that are false on SQLite — and the second on SQL Server too (below).
`supports_or_replace_view` — SQLite has no
`CREATE OR REPLACE VIEW` in any form, so a redefinition there is a `DROP` plus a `CREATE`, the
arm PostgreSQL already takes when `pg_replaceable` says no, reached unconditionally rather than
on a body test. And `supports_view_rename` — SQLite has no verb that renames a view at all:
`ALTER VIEW` is not a statement there, and `ALTER TABLE v RENAME TO v2` refuses with *"view v
may not be altered"* (measured against the engine, not read off the grammar), so `diff_view`
turns a bare rename into a re-create and the new name comes out of the `CREATE` half.
**SQL Server answers no as well, though `sp_rename` runs**: it renames the object and leaves
`sys.sql_modules` holding the old `CREATE VIEW` text, so the next *Script as CREATE* — its own
tools' or Schemaic's — builds the view under the old name, and Microsoft's advice is to drop and
re-create. That made SQL Server the first engine that can *replace* but not *rename*, and
`diff_view` had never met one: it forced the re-create only where the engine could not replace,
so a renamed SQL Server view would have come out as a `CREATE OR ALTER` under the old name plus a
`RenameView` nothing can write. It now forces `recreate` whenever `renamed &&
!supports_view_rename`, whatever the engine can replace
(`sql_server_renames_a_view_by_re_creating_it`).
`ChangeSet::view_statements`' `RenameView` arm is therefore spelled out per dialect with a
`debug_assert!` on the SQLite and SQL Server ones — unreachable by construction, and a `_ =>`
there is exactly what would hand SQLite MySQL's `RENAME TABLE`. The arm is guarded by
`supports_change`, as its `DropView` and `RefreshView` siblings are, so a refused rename writes
nothing rather than reaching them. **Triggers needed the reader before the
emitter**; that is `sqlite_trigger_info`, below. The schema tree's
table/view menu, the editor's right-click and the Create submenu each ask the predicate for the
object in front of them and offer **nothing** where the answer is no (absent, not dimmed — "not
supported" rather than "not here").
A third narrow fact joined them: **`requires_named_checks`**, false on SQLite alone —
see `TableDraft::validate` below. And `supports_change` lists **`CreateTable`**, which
it did not: `emit_sqlite` had no arm for it either, while `create_table_sql` has had a
SQLite arm since the engine landed. A set holding one `CreateTable` is not empty, so
the designer's New-table path opened its preview and offered Apply on an empty script
— the plan reported success and no table existed. That shape is the reason both the
capability list and the emitter's dispatch are worth reading together: a change kind
missing from either is silent.
**The twelve-step rebuild is `sqlite_rebuild_sql(current, draft)`**, over
`Rebuild { current, draft }` — both sides, because which new column takes which old one's data
can only be answered by looking at both. SQLite's `ALTER TABLE` does `RENAME TABLE`,
`RENAME COLUMN`, `ADD COLUMN` and `DROP COLUMN` and nothing else, so every other edit is reached
by creating the table the draft describes under a shadow name (`_schemaic_rebuild`, deliberately
not the manual's `new_X`, since a collision with a real table fails the whole plan), copying the
rows into it, dropping the original, renaming the shadow into place, recreating the indexes
**after** the rename (an index name is unique per schema, so creating one earlier collides with
the index it replaces) and replaying `TableInfo::dependent_ddl`. Each index goes back the way
`ddl::sqlite_index_replay` says: emitted from the model, which is the only form that follows a
renamed column into its index, or — for one the pragmas read `lossy` and this plan leaves alone
— replayed verbatim from `IndexInfo::create_sql`. **A partial index is its own arm**
(`partial_index_replay`), since its predicate is read back now and is free SQL naming columns:
refused when the condition names a column the plan drops — nothing to repoint it to, and SQLite
would refuse the `CREATE INDEX` with the whole rebuild rolled back — replayed verbatim when it is
untouched and no column moved, and otherwise emitted with the predicate repointed across every
rename at once by `repoint_check_columns`, as the checks are. The condition is the user's own
text, so the walk reads all three of SQLite's quoted-name forms (`"n"`, `` `n` ``, `[n]`) and
ends a bare name where `sql::continues_name` does: a `"n"` it once left behind was taken by
SQLite's double-quoted-string fallback for the constant `'n'`, quietly making the partial index
a whole one (`a_quoted_column_in_a_partial_index_condition_follows_a_rename`, red with a
`UNIQUE` failure on the exempted rows before). Before the predicate was read, a
rename anywhere under a partial index refused the plan; emitting it naively instead fails with
`no such column` in the re-emitted `CREATE INDEX`
(`renaming_a_column_a_partial_index_names_carries_its_condition`,
`dropping_a_column_a_partial_index_names_is_refused`, both red before the arm). The copy is what makes a rename
a rename rather than a drop-and-add: each new column is mapped to the one it came from, a column
the user added is in neither list so its default applies, and a generated column is in neither
because it cannot be inserted into. It is `INSERT … SELECT` and not `CREATE TABLE … AS SELECT`,
which takes its column types from the query and discards every constraint. **A rename of the
table itself is not part of it** — the rebuild always ends under the original name and
`ALTER TABLE … RENAME TO` is emitted after it, natively, which is what makes SQLite repoint the
references other objects hold. **And the plan carries `PRAGMA legacy_alter_table = ON` around
the drop-and-rename**, or the rename fails on any view over the table: from 3.25 SQLite
re-parses every view and trigger during `ALTER TABLE … RENAME`, and by then the original table
is gone, so a view selecting from it kills the rebuild with `error in view v: no such table:
main.t`. The pragma rides in the plan rather than in the backend because it is a property of
these statements and not of the connection — and so the preview shows the whole procedure, which
is the honest thing to put in front of someone about to approve it
(`ddl::sqlite_rebuild_tests`).
**The foreign-key guard rides in the plan for the same reason and one more.** The list opens
with `ddl::FK_OFF` and closes with `ddl::FK_ON`, which is step 1 and step 12 of SQLite's own
procedure: with enforcement on, the `DROP TABLE` is an implicit `DELETE FROM` the table and
fires every `ON DELETE CASCADE` pointing at it — the table comes back exactly as drawn and
another table has quietly lost every row. `sqlite::run_ddl` still sets the pragma out of band
for its *own* execution, because SQLite ignores it inside a transaction; the copy in the plan is
for the other consumer, since the same list is what the preview's **Copy** and **Open in
editor** hand to a query tab, whose connection enforces foreign keys with nothing around it
(`the_script_guards_itself_when_run_outside_run_ddl`). **That copy is only worth anything if the
runner keeps one connection for the list**, which is the part that was missing: the SQLite arm
of `Db::run_batch` opened one per statement, so `FK_OFF` was set on a connection already closed
by the time the `DROP TABLE` ran and the cascade fired anyway (`sqlite::run_batch`). A table
rename is inserted *before* the closing pragma so the whole procedure stays inside the guard.
**What the rebuild writes is the model, so the model has to be the table.** Everything the
declaration says and the pragmas don't report is read out of `sqlite_master.sql` through the
shared boundary lexer — each column's `COLLATE` (`sqlite::collations_of`), the `AUTOINCREMENT`
keyword (`declares_autoincrement`, not `sqlite_sequence`, which has no row until the first
insert) — and `WITHOUT ROWID`/`STRICT` come from the `pragma_table_list` row `has_rowid`
already reads. A `UNIQUE` constraint's `sqlite_autoindex_*` is **not** an index statement: it is
re-declared as a `UNIQUE (…)` line inside the rebuilt body, from the *draft's* column names, so
it follows a rename — `sqlite_index_replay` answers `Skip` for it, since the engine refuses
`CREATE UNIQUE INDEX "sqlite_autoindex_u_1"` by name. And the draft's `CHECK` predicates are
re-pointed across a rename here, because here is where the declaration is written; the
MySQL-family (and SQL Server) repair `ddl::alter_column_disturbs_checks` gates is a different
repair for a different statement. `db::sqlite::rebuild_fidelity_tests` reads `sqlite_master.sql` back after
each of these and compares — the question the consequence-shaped suites beside it cannot ask.
**`AUTOINCREMENT`'s counter is carried too**, and it is not part of the model: it is a row in
`sqlite_sequence` that `DROP TABLE` takes with it, while the copy re-seeds the new table's from
the rows that *survived* — so a table whose highest rows had been deleted came back handing
those ids out a second time, which is the one thing the keyword promises never happens. Two
statements between the copy and the drop move the row onto the shadow table (a `DELETE` first:
`sqlite_sequence` has no unique index, so `INSERT OR REPLACE` would leave two rows for one
table), and the rename carries it back under the real name. Gated on **both** sides declaring
the keyword — `declares_sqlite_autoincrement`, over `sqlite_inline_key`, which is also what
`create_table_sql` asks, so there is one definition of SQLite's inline-key rule.
**Three things the rebuild refuses rather than does.** `ChangeSet::unsupported` withholds a plan
whose replayed `dependent_ddl` would name a column the plan renames or drops
(`rebuild_strands_a_trigger`): the text is a snapshot, `legacy_alter_table = ON` stops SQLite
fixing it, and SQLite validates `NEW.<col>` at write time rather than at `CREATE TRIGGER` — so
the plan used to *succeed* and the table then rejected every write. The route that does work is
offered instead: a rename **on its own** is `ALTER TABLE … RENAME COLUMN`
(`supports_change` + `is_rename_only`), which re-points every view and trigger for us.
**And that refusal now looks one table over**, because the identical failure lands there and
nothing was watching it. `TableInfo::referring_ddl` is the `CREATE` text of triggers on *other*
tables whose SQL names this one — never replayed by a rebuild and never dropped by one, and
therefore never *rewritten* by one either. A trigger declared `AFTER INSERT ON other` whose body
reads `UPDATE t SET b = 'hit'` survives a rebuild of `t` that renamed `b`, so the plan succeeds,
the report says it applied, and the next `INSERT INTO other` fails *no such column: b*: `other`
now rejects every insert and nothing said so (`db::sqlite`'s
`a_trigger_on_another_table_is_seen_by_the_rebuilds_refusal`, measured against a real in-memory
database). That half is asked **per moved column and per trigger** — `moved_columns` returns all
of them rather than the first, since the first moved column is not necessarily the one a given
trigger spells, and refusing on any trigger that merely names the *table* would withhold edits
that are fine — and each match goes through `intel::code_names`, so a mention in a comment or a
string literal is not one. `column_moved` stays as the thin first-column wrapper for the callers
that only need something to say, and what it returns is a `Moved` — `Renamed { from, to }` or
`Dropped { name }` — which the refusal carries and the interface words, not a phrase.
It also withholds a rebuild of a table whose *declaration* says more than the model can restate
(`rebuild_cannot_restate`, over `unrestatable_sqlite_clauses`): a foreign key's `DEFERRABLE`, a
column's `ON CONFLICT`, a `DESC` primary-key column, and a `COLLATE` **inside** a
`PRIMARY KEY ( … )` group. What introspection doesn't model, the
rebuild deletes — and because the draft is built from the same incomplete model, `diff` reads
the untouched draft as a no-op, so no round-trip check can see the loss either. Each was
measured deleted by a plan that reported success, and each changes what the table *does*. The
scan is the shared boundary lexer's and covers the table body only, so an **index** key's `DESC`
— which the model does carry (`IndexColumn::descending`) and does re-emit — is not a refusal.
**And it is positional: each word asks where it is, not merely whether it is there.** A refusal
here is permanent — `unsupported()` is non-empty for *every* rebuild of the table, so the
designer opens on a table nobody has touched with Apply disabled and a clause named that the
declaration does not contain — which is why a false refusal costs far more than a needless
rebuild. Measured on 3.50.4, `conflict` and `desc` are both accepted as bare column names and
`PRIMARY KEY (desc)` is accepted as a key on one, so `CONFLICT` counts only directly after `ON`
and `DESC` only as a non-first word of its comma-separated group (`PRIMARY KEY (a DESC)` is a
direction, `PRIMARY KEY (desc)` a column). `COLLATE` is positional for a different reason: a
column's own collation is `ColumnInfo::collation` and an index key's is `IndexColumn::collation`,
both modelled and both round-tripped, and the *key's* is the one place it has nowhere to go —
`is_primary()` filters the key's index out of the draft and the `Vec<String>` that replaces it
has no field for one. `PRIMARY KEY (email COLLATE NOCASE)` came back as `PRIMARY KEY ("email")`
and the key started comparing in `BINARY`: measured on 3.50.4, with `'A@x'` already present,
`'a@x'` is refused *UNIQUE constraint failed* before the rebuild and accepted after it.
`DEFERRABLE` needs no position, and not by luck of the scan — SQLite refuses it as a bare column
name (*near "deferrable": syntax error*), so the only way it reaches code is as the clause
(`a_primary_keys_collation_is_withheld`, `a_column_named_after_a_clause_is_still_a_column`).
`Change::RebuildTable(Box<Rebuild>)` is how it reaches a plan: `diff` inserts one at the
**front** of the set the moment that set holds a change SQLite has no statement of its own for,
and that one change performs the whole set. Where it does so is `rebuilds_tables(dialect)` — an
exhaustive `match`, true on SQLite and SQL Server (`tsql_rebuild_sql`, below), false on MySQL
and PostgreSQL, which alter in place and whose refusals a rebuild would not make possible — and
it replaced the literal `dialect == SqlDialect::Sqlite` the insertion was spelled as. It sits *beside* the changes it performs rather than
instead of them, so the preview still lists the user's edits in their own terms and one line
says how they will happen. The trigger is "is there a change here with no statement of its own",
not "is this an alter" — a set of nothing but the drops the engine does have keeps its direct
path and pays nothing. **An `ADD COLUMN` is the one fast path taken**, through
`sqlite_native_add(column, position)`: appending a column is the commonest designer edit there
is and SQLite performs it instantly, so copying the whole table to achieve it was correct and
absurd. The predicate answers false for anything it isn't sure of, because the failure mode is a
plan that *half-applies* — fast path taken, engine refuses the statement, and the edit the
preview promised is simply gone. A needless rebuild is slow; a wrong fast path is a lie. The
rules, each measured against SQLite 3.46.0 rather than read off the grammar (the bundled engine
is 3.53.2 now — the tests below say which half of the gate that re-checked): `position` must be
`None`, since `ADD COLUMN` always appends and a column dropped into the middle would land at the
end, leaving the designer showing one order and the table having another — **this is the one
rule with no error message behind it, the statement succeeds, in the wrong place** (and
`apply_positions` runs for MySQL and SQLite alike, so a `Position` is a reliable signal); no
primary key (*"Cannot add a PRIMARY KEY column"*); no `auto_increment`, because `AUTOINCREMENT`
is legal only inline as `INTEGER PRIMARY KEY AUTOINCREMENT` and `ColumnInfo::definition_sql`
therefore drops it for SQLite, so a native add would silently lose the counter the rebuild's
table builder can place; a constant default if there is one (*"Cannot add a column with
non-constant default"*); and `NOT NULL` requires a non-null default (*"Cannot add a NOT NULL
column with default value NULL"*). One deliberate non-rule and one that is narrower than it
reads: **uniqueness** isn't on
`ColumnInfo` at all — it arrives as an index, which has no native arm and takes the set back to
a rebuild by itself, and that is a fact about *this gate* rather than about SQLite, which
refuses only an inline `UNIQUE` in the column definition and would take a native add followed by
a `CREATE UNIQUE INDEX` quite happily — while a **`VIRTUAL` generated** column is addable, which
is not the same claim as "a generated column is". The justification used to be that the emitter
writes no `VIRTUAL`/`STORED` keyword, so SQLite's own default (`VIRTUAL`) applies — and the
emitter does write it: `ColumnInfo::definition_sql` emits ` STORED` for a SQLite column whose
`generated_stored` is set, a line added deliberately so a `STORED` column survives a rebuild,
with the flag round-tripped from `table_xinfo`'s `hidden == 3`. So the predicate and the emitter
disagreed about what the statement would say, and the engine refuses *"cannot add a STORED
column"* on any table that has rows — through `run_ddl` a rolled-back plan with a confusing
message, through Copy or Open in editor a two-column add that half-applies
(`a_stored_generated_column_is_not_a_native_add`). Either way a generated column carries its
expression instead of a default, so the
null-default rule has nothing to reach. `sqlite_constant_default` decides the default: the `CURRENT_TIME`/
`CURRENT_DATE`/`CURRENT_TIMESTAMP` keywords and anything parenthesised are not constants (the
paren test also catches a bare `now()`, which isn't a legal `DEFAULT` there at all), and the
parenthesis is looked for at a **code** position through `sql::skip_noncode` rather than with a
`contains('(')`, so a default of `'a (b)'` stays native — its parens are data. A native add
*beside* an unsupported change is still subsumed by the rebuild and must not also emit its own
`ADD COLUMN`. There is a test per restriction in `ddl::sqlite_designer_tests`, and
`db::sqlite`'s `every_natively_added_column_is_one_sqlite_accepts` runs every shape the
predicate calls native through `run_ddl` at real in-memory SQLite — which is what the fast path
actually rests on, since a predicate drifting from the engine's restrictions can't be caught by
reasoning, only by asking SQLite. That test compiles against whatever amalgamation `rusqlite`
bundles, so the move to 3.53.2 re-checked the **permissive** half for free and it still passes.
The refusals get no such guard: nothing here compiles a shape the gate withholds, so the list
above remains what it was measured to be on 3.46.0 and a later SQLite quietly accepting one of
them would cost only a rebuild nobody needed — which is the direction this gate is built to fail
in.
**`supports_change(dialect, &Change)` is the second gate, and it answers a different
question.** It is about one change **on its own** — a context-menu shortcut, which has no draft
behind it to build a table from — where the designer's plan can answer anything by rebuilding.
On SQLite it is true for `DropTable`, `DropView { materialized: false }`, `DropColumn`,
`DropIndex { constraint: None }` and `DropTrigger` — the drops the engine genuinely has
statements for — for the whole-statement objects it creates like anyone else (`CreateTable`,
`CreateView`, `ReplaceView`, `CreateTrigger`, `ReplaceTrigger`, the replaces being a
drop-and-create on every engine), for `TruncateTable`, which SQLite spells `DELETE FROM t` with
no `WHERE` and optimises into the same operation, and for `RebuildTable`, which only `diff`
raises. `CreateTable` and `TruncateTable` were both missing from this list for a while and each
absence is in `ddl.rs`'s comments: without the first the designer's New-table path built a
non-empty change set, opened a preview, offered Run and emitted nothing; without the second
Truncate was offered red and enabled, asked *"Delete all ~4.2m rows in orders?"*, and then
opened an empty script behind an inert Apply. `RenameView` is deliberately **not**
on the list: `diff_view` resolves a SQLite rename into the re-create before it can reach here.
**It is no longer purely a question of the change's kind**: two arms are answered from what the
change *contains* and never reach that list. `AddColumn` asks `sqlite_native_add` above, and
`AlterColumn` is true exactly when it is a bare rename — `position.is_none()`,
`inline_check.is_none()` and `is_rename_only` — which is the `ALTER TABLE … RENAME COLUMN` route
the rebuild's refusal points at. It is false for everything else, where each false is
the twelve-step rebuild in disguise: a foreign key or a constraint-backed index comes off only
by recreating the table around it. Every other dialect answers true but SQL Server, whose four
changes are an early arm of their own (under `db::mssql`). It exists because the
per-row menus were built with **no** gate at all — not this one, not even `read_only` — so a
column row's **Edit column** and a key row's edit entry opened the designer on a SQLite
connection, ran `diff`, and reached a preview that only `Db::run_ddl` refused at the last
moment. Those two rows now ask `overlays::field_entries`/`key_entries` (separate from the menu
builder so the rule can be asserted without a `Ui`, the same shape as `object_entries`): Edit
column and the key row's edit entry — **Edit index**, **Edit foreign key** or
**Edit primary key**, named for whichever the row is — are offered on every engine, since the
designer is the thing that reaches a rebuild, **Drop** (column) and **Drop index** stay because
the engine performs them, and Drop foreign key — plus Drop index when the index backs a
constraint — is absent (`overlays::row_menu_tests`). `ChangeSet::unsupported()` is the same
predicate read over a whole set, returning the plain-language summaries of what the dialect
can't express, which is what the preview shows and refuses to apply around. **Where a rebuild is
present it withholds nothing except an index flagged `IndexInfo::lossy` that cannot be put back
as it was**: the rebuild drops the table, so every index has to be created again, and one
re-emitted from a partial reading is not the index that was there — a partial index comes back
covering every row, an expression index missing its key. (A partial index is no longer one of
these: `sqlite::index_predicate_of` reads its condition back, and it is `lossy` only where its
text yields none; its own refusal — a condition naming a dropped column — is under the
twelve-step rebuild above.) The way through is the one a trigger
takes: `ddl::sqlite_index_replay` replays the index's **own** `CREATE` text
(`IndexInfo::create_sql`) verbatim, which cannot lose what the pragmas never read. That leaves
three narrow cases where no faithful statement exists, and only those are withheld — the index
was **edited** (the stored text is the old one, and the model can't describe the new one), the
index has no stored text of its own, or the plan **renames or drops a column**
(`ddl::column_moved`). The last is not caution: the unread part is exactly where an expression
key lives (and a partial index's predicate, where it could not be cut), so which columns the index uses is a question the
model cannot answer, and it is asked of the whole table instead. A withheld index is not a
warning to read past; the plan is refused until the user drops it or undoes the edit.
**SQL Server rebuilds too, through `tsql_rebuild_sql(current, draft)`**, for the three edits
T-SQL's `ALTER COLUMN` cannot make: an identity switched on or off, a column turned into or out
of a computed one, and a column put anywhere but last. Until then the first two were withheld
in the preview and the third was never offered; `tsql_supports` still refuses them one by one, and `diff` answers a set holding one with
the `RebuildTable` it now admits, by the rule above. The shape is SSMS's designer's rather than
SQLite's twelve steps — eight steps, each its own batch, all inside `Db::run_ddl`'s one
transaction, so a failure anywhere leaves the table as it was. (1) **A guard that stops the
plan before it touches anything**, one `DECLARE @t … CASE WHEN … END IF @why IS NOT NULL THROW
50000, @why, 1;` naming the first reason, wherever the table carries something the model does
not read and the `DROP TABLE` would take with it: permissions granted on it; an owner of its
own (`ALTER AUTHORIZATION`, a non-NULL `principal_id` — the copy is the schema owner's); a
schema-bound dependent; system versioning, memory optimisation, replication, change data
capture or a lock escalation setting; a ledger; change tracking or a full-text index;
partitioning, compression or a filegroup other than the default — an XML or spatial index's
compression included, which sits on the partitions of its internal table rather than the
table's; a columnstore index's `COMPRESSION_DELAY`, or `XML_COMPRESSION` on the table or on an
XML index on it; an extended property on the
table or its columns other than `MS_Description`; **any** extended property on what stands on
the table — class 1 on an object whose `parent_object_id` is the table (its key, default and
check constraints, its triggers) or class 7 on its indexes — a description included, and on a
key **another** table holds on it, which step 3 drops and step 7 adds back; an index option
(fill factor, padding, `IGNORE_DUP_KEY`, row or page locks) or a disabled index; a sparse,
column-set, `FILESTREAM`, `ROWGUIDCOL`, masked, encrypted, XML-schema-typed, hidden or
generated-always column; a rule or a default bound to a column (`sp_bindrule`,
`sp_bindefault` — the reader takes defaults from `sys.default_constraints` only); a sensitivity
classification; a foreign key
on it or to it that is disabled, untrusted or `NOT FOR REPLICATION`, or a check or an identity
that is; and a count of its indexes, own keys, inbound keys, checks or triggers that no longer
matches the reading the draft was made from. The alternative to each is a plan that succeeds
and reports nothing lost — a ledger table came back an ordinary writable one, its original
kept as `MSSQL_DroppedLedgerTable_…`, and a bound rule's `INSERT … a = -5` was accepted, before
S2-L5-04 added those arms. **Two arms bar what a columnstore index is by nature**, since the
model states one now (`IndexInfo::method`, under `schema.rs`): its own `COLUMNSTORE`
compression, `data_compression` 3, so the arm refuses `NOT IN (0, 3)` — `COLUMNSTORE_ARCHIVE`
(4) is an option, and still refused — and row and page locks, which both columnstore kinds
report off (measured on 2022), so the index-option arm asks them only `AND type NOT IN (5, 6)`.
Asked as they had been, they refused a columnstore rebuild whose plan was otherwise right, which
the live `a_columnstore_index_is_read_rebuilt_and_authored` caught; `tsql_index_carries`, the
in-place guard's predicate (below), takes the same two bars. **Four arms ask of a catalogue an
older server lacks**: `sys.tables.ledger_type` (2022), `sys.sensitivity_classifications`
(2019), `sys.indexes.compression_delay` (2016) and `sys.partitions.xml_compression` (2022).
Named in the `CASE` they would fail its compile, and so every rebuild, there; `tsql_late_arm`
runs each behind a probe (`COL_LENGTH`, `OBJECT_ID`) through `sp_executesql`, ahead of the
`CASE` so a ledger table is named as one rather than by the generated-always columns every
ledger table has. **An XML or spatial index keeps its rows in an internal table of its own**
(`sys.internal_tables`, `parent_object_id` the table, `parent_minor_id` the index — a
secondary XML index's in its parent's, as an index of it), whose
partitions carry its `DATA_COMPRESSION` and `XML_COMPRESSION` under another object id — so an
arm reading `sys.partitions` for the table alone never saw a spatial index's `PAGE`
compression (measured on 2022), and the compression arm and the `xml_compression` arm each
ask the table's internal tables too (R3-L5-03). XML, spatial and columnstore indexes became
restatable in one release without the guard learning where their options live; the
table-level `XML_COMPRESSION` gap predated it. The schema-bound test leaves out objects whose
parent is the table itself, because a check or default constraint is an object of its own, schema-bound to the
table it stands on — found live. **The arm about what stands on the table is the one that
looked covered and was not** (S3.2-L5-03): the table's arm read class 1 on the table's own id
alone, but a constraint's or a trigger's property sits on *its own* object id and an index's
is class 7, so none of them was seen — and the model reads only the table's and its columns'
comments, so an AdventureWorks-style schema with an `MS_Description` on every key and index
lost all of them on any column move, measured live on 2022, while the risk line promised that
"comments" came back. A disabled index is refused beside the index options because
`CREATE INDEX` builds it: it came back enabled. **Every arm is run live** by
`every_arm_of_the_rebuild_guard_refuses_its_table` (S3.2-L6-01) — one scratch table per arm,
the smallest that trips it, its columns swapped, the plan refused with that arm's reason and the
table left with its columns in order and its row kept. Before it nine of the then ten arms had
never run under a test, and an arm whose catalogue predicate is wrong fails open without a
sound, which is what the one above did; its four cases for the widening — a description on the
key, on an index and on a trigger, and the disabled index — were red, the plan applied, before
it, and it is green on 2022 and 2025. R3-L5-03's four — a `PAGE`-compressed spatial index, a
columnstore index with `COMPRESSION_DELAY = 30`, an `XML_COMPRESSION` primary XML index and a
table rebuilt with `XML_COMPRESSION = ON` — were red too, on 2022, with that fix stashed.
Full-text and
memory-optimised tables are not in it: the containers have no full-text service and no
memory-optimised filegroup. (2) The default constraints' names, captured into the session
temp table `#schemaic_rebuild_defaults` (`TSQL_REBUILD_DEFAULTS`), since the model reads a
default's value and not its name, and one put back as `DF__t__a__5EBF139D` is not the one
scripts and a schema compare name; a `#` table because `run_ddl`'s one connection carries it
across the batches. Beside them, what selects `*` from the table
(`tsql_collect_star_dependents`, below) — here and not later because the first level is found
by the table's id, which the `DROP TABLE` ends. (3) The keys **other tables** have on it (`TableInfo::referenced_by`, under
`schema.rs`) dropped, since each refuses the `DROP TABLE` (Msg 3726). (4) A bare shadow,
`<table>_schemaic_rebuild`, of the draft's columns alone — no key, index, check or default,
because a constraint's name is the schema's and the old table still holds each one. (5) The
rows, `INSERT … SELECT … WITH (TABLOCKX)`, under `SET IDENTITY_INSERT … ON`/`OFF` where an
identity column is copied into; a new column takes its default's expression in the `SELECT`,
which is what a new column's default does to the rows already there, and a new identity numbers
them itself. (6) `DROP TABLE` and an `sp_rename` of the shadow to the **draft's** name, so a
table rename is inside the rebuild rather than after it as on SQLite; where an identity is
kept, a `DBCC CHECKIDENT` reseed past the last value the old table issued, so values freed by
deleted rows are not handed out again. It reads `sys.identity_columns.last_value` rather than
`IDENT_CURRENT`, which answers a table never inserted into with its seed; it works out the next
value the new table would give — its seed where it has had no insert, past its own last value
otherwise — and reseeds only where that is not past the old table's last, in the increment's
direction (`SIGN(@inc)`), so a descending identity is carried on too. **Where the new table has
had no insert — a copy of a table whose rows were all deleted — it reseeds to the value wanted
itself**, because `DBCC CHECKIDENT` then hands out the reseed value rather than the one after
it: reseeding to the old last value reissued it (S3.2-L1-01, measured on 2022 and 2025;
`a_rebuild_reissues_no_identity_value` runs an emptied, a partly deleted, a descending and a
never-used identity). The statement is
built into a variable first because `EXEC (…)` takes only literals and variables (Msg 102,
measured). (7) What stood on it: the key under its old constraint name, the checks, the indexes
and unique constraints — through a synthetic `ChangeSet` that `emit_mssql` writes, so none of
them is a second spelling — then the table's own foreign keys, after the unique ones a
self-reference may name and with that self-reference re-pointed at the new name; the defaults
under their captured names; the comments; the triggers through `trigger_create_statements`,
disabled and ranked as they were, on the new name; the inbound keys re-added pointing at the new
name and the renamed columns, and naming the table's schema outright — another table's key is
written on *its* table, whose namespace a bare name would not mean. (8) The views and inline functions that select `*` from it
refreshed (`tsql_refresh_star_dependents`, below) — this step and its capture in (2) only
where the table keeps its name — and then the defaults' temp table dropped.
Every statement but a trigger's own
is one batch with no `;` inside, because the preview's Open in editor splits at them
(`sql_server_rebuilds_what_alter_column_cannot` pins the order and that). `emit_mssql` returns
the rebuild alone when one is in the set: it writes the table the draft describes, name
included, so the set's other entries are already in it.
**Every SQL Server plan that moves a table's columns refreshes what selects `*` from it** — the
rebuild and `emit_mssql` alike. A non-schema-bound view or inline table-valued function keeps
the column list its `*` expanded to and binds it to the table's columns **by position** until
`sp_refreshsqlmodule` re-expands it. Measured on 2022 and 2025: after a rebuild that put
`credit_limit` before `balance`, a `SELECT *` view read each under the other's name and
`UPDATE v SET balance = 0` zeroed `credit_limit` (R3-L5-02); after a `RebuildComputedColumn`
moved `c` last, one read `x` as `c` and an `UPDATE` of `x` wrote `y` (S3.2-L5-01); and a column
dropped beside one added leaves the count unchanged, so the view mislabels just as silently.
Which plans move columns is `tsql_moves_columns` — a `RebuildTable`, a `RebuildComputedColumn`
or a `DropColumn`; a rename or retype leaves the positions alone and an add alone appends.
Whether an engine needs the refresh at all is `refreshes_star_dependents(dialect)`, an exhaustive
`match` true on SQL Server alone: MySQL stores the expanded list by name, PostgreSQL by column
number but refuses to drop a column a view reads and cannot reorder one, and SQLite re-parses a
view at every use. It is two statements around the move, each one batch with no `;` inside.
`tsql_collect_star_dependents(table)`, run **before** the columns move, copies into the session
temp table `#schemaic_star_dependents` (`TSQL_STAR_DEPENDENTS`) the table at level 0 and then,
level by level to a cap of 32, every view and inline function that references something the
level before found, and every synonym naming one, out of **`sys.sql_expression_dependencies`**
and `sys.synonyms` — by id, or by name where a reference is unresolved or spelt with this
database's name. It **used to read `sys.sql_dependencies` where `is_select_all = 1`**, the
deprecated catalogue chosen because it carries that flag, and it missed exactly the views that
needed it (R3-L5-01, S2-L5-01, measured on 2022 and 2025): that catalogue keeps a view's rows
only from its creation or last refresh, so a table SSMS's designer or any `SELECT … INTO`/`DROP
TABLE`/`sp_rename` migration had ever rebuilt left it **no row** for any view over the table, and
a view over a synonym has its row under the synonym, which the walk never visited — each left
bound by position, `UPDATE acct_v SET balance = 0` landing on `credit_limit`, with the plan
reporting success. The expression catalogue keeps a reference by name and resolves it afresh
(after the outside rebuild its `referenced_id` was the new table's), and it has no
`is_select_all`, so the collector is **fail-safe rather than exact**: of the views and inline
functions it keeps those whose text holds a `*` at all, or cannot be read. One that names its
columns and writes a `*` elsewhere is refreshed for nothing, which costs nothing unless it
names a column the plan drops — and then the plan fails whole, the direction to fail in. **The
temp table's names are `COLLATE CATALOG_DEFAULT`**, the collation of the catalogue views it is
joined to: it was `DATABASE_DEFAULT`, which is the same until the database is contained, whose
catalogue is always `Latin1_General_100_CI_AS_KS_WS_SC` — and there the join was Msg 468, so no
plan that moves a column could be applied in a contained database
(`the_star_collector_compares_under_the_catalogues_collation`; live,
`a_column_move_applies_in_a_contained_database`). **A view in another database is not looked
for inside the plan**: no catalogue in this database lists one, and entering every database
from inside the plan's transaction was built and measured on 2022 before it was dropped — the
transaction then holds a shared lock on each database it entered until it commits, so for a
rebuild's whole copy any `ALTER DATABASE`/`DROP DATABASE` on the server waits on the plan or,
`WITH ROLLBACK IMMEDIATE`, kills it (Msg 596 — the live tier's own scratch teardown did it on the
first run), and a database dropped between the listing and its turn dooms the transaction even
inside a `CATCH` (Msg 911, `XACT_STATE()` -1). **It is refreshed after the plan commits
instead** (R3-L5-01): `ChangeSet::refresh_elsewhere()` — `Some` exactly where
`refreshes_star_dependents()` is — is one batch (`tsql_refresh_elsewhere`) the preview carries
beside the plan (`DdlPreview::after_commit`, shown in the SQL box under its own heading and
appended to the copied script), and the app runs it through `Db::refresh_elsewhere` on a
connection of its own once `run_ddl` has returned `Ok`. Outside a transaction each step holds
its locks only while it runs, and a database gone mid-walk is an ordinary caught error (Msg
911, measured). The batch re-runs the collector in the table's database, then for each other
database in `sys.databases WITH (READPAST)` that is online and multi-user (never asking
`HAS_DBACCESS`, which waits on a database mid-create; a snapshot is not asked) enters it with
`<db>.sys.sp_executesql` and walks as the plan does from whatever names a collected object by a
three-part name or through a synonym — so a view there over this database's own `*` view is
found too — and refreshes each in its own `TRY`, writing `(db, module, error)` rows; one offline
or single-user is a row with no module, and Azure SQL Database, with no cross-database reference
to find, is skipped. `Db::refresh_elsewhere` sets the plan's lock wait and runs the batch twice
where the first fails whole, since a `WITH ROLLBACK IMMEDIATE` on a database the walk is inside
kills its session. `ddl::elsewhere_notes` turns the rows into what the walk could not do — an
`ElsewhereNote` for a view whose refresh failed, named with the server's reason, and one for a
database not searched, each list a `Capped` cut at eight, or a single `Failed` where the batch
itself failed, its error `db_error_shown`'s — and those, worded by `i18n::elsewhere_note_text`
one line each, are `DdlOutcome::Applied`'s note, shown under the success lines in the error
colour, the plan standing either way (`ddl::elsewhere_report` is their English, under *The
string catalogue*). A view the walk refreshed was promised, and is
not repeated (`a_plan_that_moves_columns_refreshes_other_databases_after_it_commits`,
`the_refresh_elsewhere_reports_what_it_could_not_do`,
`the_sql_box_shows_what_runs_after_the_commit`; live,
`a_column_move_refreshes_other_databases_views_after_it_commits`, with views by three-part
name, through a synonym, over a view, over this database's own `*` view, one naming its
columns, and one in a read-only database). The cost is a window: between the commit and the
refresh such a view reads the old positions, which the preview's sentence says. Inside the
plan, after the collector, `tsql_refresh_star_dependents()` walks a
cursor running `EXEC sys.sp_refreshsqlmodule` on each kept object of type `V` or `IF`, nearest
level first so a view over a view re-expands against one already refreshed, and drops the temp
table; a synonym needs nothing, and a procedure, a trigger or a multi-statement function
expands its `*` at each compile. **There is deliberately no `TRY … CATCH` around it**: a failed `sp_refreshsqlmodule`
rolls back the caller's whole transaction (measured), so swallowing the error would leave the
rest of the plan running auto-committed. A dependent that no longer compiles against the new
columns therefore fails the plan whole, and `ChangeSet::destructive()` says so beforehand in one
plan-level sentence — the plan's step rather than any one change's, so said once however many of
its changes move a column. Both `emit_mssql` and that sentence read one answer,
`ChangeSet::refreshes_star_dependents()`, so the preview cannot disagree with the script: the
dialect's capability and `tsql_moves_columns` over the admitted changes — **except a rebuild
that also renames the table** (`tsql_rebuild_keeps_name`), whose dependents all name the table
as it was, so a refresh there could only fail the plan, where a dependent left naming a table
that is gone fails loudly on its next read, as after any rename. In `emit_mssql` the capture
runs after the trigger statements and before anything covering the columns is dropped, and the
refresh after the comments and **before** the table's own `sp_rename`, since a dependent names
the table as it is and would not compile against a name that is gone. The unit pins are
`a_sql_server_rebuild_refreshes_what_selects_star_from_the_table`,
`a_rebuilt_computed_column_refreshes_what_selects_star_from_the_table`,
`only_a_plan_that_moves_columns_refreshes_what_selects_star` and
`the_star_collector_finds_what_the_deprecated_catalogue_misses`; the live ones, each red before
the fix, are under `mssql.rs` — their fixture has a view over a synonym of the table, and the
rebuild's runs after an SSMS-shaped rebuild that leaves `sys.sql_dependencies` empty.
**Which columns the copy skips is asked of the type, not of `identity_always`**
(`tsql_server_filled`): a computed column and a `rowversion`/`timestamp`, whose values the
server makes. The reader sets `identity_always` on an identity too, while the designer's toggle
switches one off by `auto_increment` alone, so a switched-off identity was being left out of
the copy — found live. **What the reading already shows the rebuild cannot put back is
`unsupported()`'s answer** — `tsql_rebuild_refusals`, SQL Server's arm of an exhaustive `match`
on the dialect in the rebuild branch, where SQLite's is the list above: an index `lossy` there
(an index whose key names a graph table's internal column, `$from_id` say, or one whose
reading T-SQL does not spell; every XML and spatial index was too, until
`IndexInfo::method` and `using` held one — the live
`xml_and_spatial_indexes_are_read_whole_and_survive_a_rebuild` — a selective XML one until
`using` held its paths — `a_selective_xml_index_is_read_whole_and_survives_a_rebuild` — every
columnstore until `method` held one — `a_sql_server_rebuild_restates_a_columnstore_index` — an
ordered one until `IndexInfo::order` held its order, and one
with included columns was, refusing five of
WideWorldImporters' 48 tables, until `IndexInfo::include` held them and the rebuild restated
them through `create_index_sql` —
`a_sql_server_rebuild_restates_an_index_with_included_columns`) would come back without what
was not read, an encrypted trigger has no text to put back, a **signed** one (`TsqlModule::signed`)
would come back from its text without its `ADD SIGNATURE`, which only the certificate's private
key could restate — the rights the certificate's user held for it stopped applying, every DML
on the table failing or a `TRY…CATCH` in the trigger quietly ending its auditing (S2-L5-05) —
and one kept verbatim names the table as it was, which a rename leaves behind. The guard is the
half only the server can answer, and it asks `sys.crypt_properties` of the table's triggers too,
for a signature added after the reading (`a_rebuild_is_refused_over_a_signed_trigger`).
`Change::risks` has a SQL Server arm saying what is put back, that the plan stops first on what
isn't, and the one thing neither half sees — statistics created by hand are not carried over —
and `Change::summary` for a rebuild is engine-neutral now. What it says is put back is named
whole — the keys, indexes, checks, defaults and triggers, *its own and its columns'* comments,
and the foreign keys other tables have on it — and what stops it includes a disabled index and
a description on a key, index, default, check or trigger, because a bare "comments" was the promise
S3.2-L5-03 found broken. The unit pins are
`sql_server_rebuilds_what_alter_column_cannot`,
`a_sql_server_rebuild_copies_into_an_identity_and_fills_a_new_column`,
`a_sql_server_rebuild_refuses_what_it_cannot_put_back` and
`the_rebuild_guard_refuses_what_stands_on_the_table_that_it_would_drop`; the live ones are
under `mssql.rs`.
`TableDraft` (the desired table; column/index/FK
entries each carry the name they had on the server, which is what tells a *rename*
from a drop-plus-add) → `diff(current, draft, dialect) -> ChangeSet` → `emit()`.
Every `Change` answers `line()` and `risks()`, which is what the preview
modal renders — `risks()` returns *every* consequence, not the first, because one edit can
narrow a column **and** make it NOT NULL and the NOT-NULL sentence ("the statement fails")
otherwise reads as a promise that nothing is lost. `line()` is a `ChangeLine`, the change's
sentence as a value the interface words, and `summary()` is its English, `line().message()`,
for the CLI, the MCP server and the tests (under *The string catalogue*); a refused change
reaches the withheld list as that `ChangeLine` itself, `Withheld::Change`.
`ChangeSet::withheld_lines()` is the same move made for the withheld list: one `Withheld` per
sentence, every producer above and below — `sqlite_index_replay`'s `IndexReplay::Refuse`
included — returning one rather than a `String`, and `unsupported()` is its English, which is
still what `withheld_header`, the CLI and the tests read.
`risks()` is the same move made for the risk block: `risk_lines()` answers one `Risk` per
sentence, `risks()` is its English, and `is_destructive` asks `risk_lines()`; a set's
`ChangeSet::risk_lines()` adds the plan's own `Risk::StarRefresh` and `Risk::Piecewise`
(`piecewise_risk`), and `destructive()` is its English.
**The change list is a *diff*, and `summary()` used to print only the new side.** A column's
four free-SQL fields — default, generated expression, `ON UPDATE`, `COLLATE` — are free SQL by
design, a default having to be able to say `nextval('s')`, and `column_clauses` renders only
what is *present*: so *clearing* a default made the two sides differ (the guard in the
`AlterColumn` arm falls through) while `column_clauses(to)` came out empty, and the line was
byte-identical to the "nothing interesting changed" case, `Change column qty`, while `emit()`
wrote `MODIFY COLUMN qty INT NOT NULL`. That is against `column_clauses`' own stated contract —
the change list is the consent gesture, so nothing may reach `emit()` without reaching it.
`clause_pairs` gives the four as `(ClauseKind, Option<value>)` and `clause_changes` names each pair
that differs **in both directions**, `, default 0 → (none)`, on all three `AlterColumn` arms;
they shared the one value, so a rename or a retype hid it too. The asymmetry was exact — an
added or edited clause was named, a cleared one was silent — and clearing a `COLLATE` changes
comparison and sort semantics for every existing row without touching a byte of data, which was
as silent as the rest. It **changes user-visible text**: an added default now reads
`, default (none) → CURRENT_TIMESTAMP` rather than `, default CURRENT_TIMESTAMP`.
**`risks()` was systematically incomplete, and an empty answer is the dangerous one** — it
empties `destructive()` with it, so the preview's risk block hides itself on the edit that
needed it. `alter_risks` covered a nullable→NOT NULL flip and `type_change_risk`, so a
**generated-expression** edit fired neither: it now names all three directions of one (gained,
lost, changed), and the PostgreSQL destruction below is what that silence was covering.
**`type_change_risk(from, to, subject, dialect)` compared the spellings the user typed, and got
both directions wrong.** It normalises each side through `normalize_type` before comparing —
which is the `dialect` it and `alter_risks` now take — and still quotes the raw pair in the
sentence, the comparison being the machine's question and the wording the user's. A synonym is
the same type family: `decimal(10,4)` → `numeric(10,0)` had differing bases, so the pairwise arm
was skipped and the generic *rewrites every value* fired in place of the *rounds every value in
the column* sentence that exists precisely because a smaller scale loses data without the
statement complaining; and in the other direction PostgreSQL's own `character varying(45)`
widened to `varchar(255)` — metadata only — was reported as a full rewrite **and** flipped
`is_destructive`, putting Apply behind the destructive path, as did `integer` → `int4` and
`boolean` → `bool`. The normalisation already existed at `Change::RecreateDomain`'s call site,
where its comment names this defect in so many words, so the fix reached the newer path and not
the one every table edit on every engine takes; it lives in the function now, and that call site
no longer needs it. Three
constraint removals likewise had **no arm at all** and fell to `_ => Vec::new()`, though
`DropCheck` — the same class, losing no data while the table stops guaranteeing something no
later surface mentions — has had one with the argument written above it: a dropped **foreign
key** (orphan rows accepted from now on), a dropped **unique** index (duplicates accepted, and
the grid may lose the key it was editing rows by), and a primary-key *swap*, which fell through
because only the drop-to-nothing case was guarded. `Change::DropIndex` carries a `unique: bool`
for that one arm alone, so it can tell a lost guarantee from lost query speed; a menu entry that
only asks `supports_change` may leave it `false`, the answer it wants not depending on it.
The emitter owns the engine divergence: MySQL coalesces into one
`ALTER TABLE` and restates a whole column via `definition_sql` (`MODIFY` replaces it,
so anything omitted is destroyed); PostgreSQL splits renames / `DROP INDEX` /
`CREATE INDEX` / `COMMENT ON` into their own statements and drops a key by
*constraint* name (`IndexInfo::constraint` — it has no `DROP PRIMARY KEY`).
**PostgreSQL's one destructive omission was a generated column becoming plain.** It cannot
*change* a generated expression in place, so `pg_column_clauses` turns that into a drop and a
re-add — and the branch was `from.generated != to.generated`, justified as a drop and a re-add
of a column whose values were derived anyway. True when `to.generated` is `Some`, where the new
expression recomputes every value; false when it is `None`, where the column becomes plain and
there is nothing left to derive the values *from*. Measured live on PostgreSQL 16.15: clearing
the designer's "Generated from" field turned a `total` holding 2/4/6 into three empty cells, and
moved the column to the end of the table. PG 13+ has the statement that does what the user asked
and keeps the values, measured in the same run — `ALTER COLUMN … DROP EXPRESSION` — and that is
the emitted clause for that direction now. It **falls through** rather than returning, because
the same edit may also have changed the type, the nullability or the default, and each of those
moves on its own here. The `NOT NULL` variant was safe by accident: `to.definition_sql` restates
`NOT NULL`, the server refuses `ADD COLUMN … NOT NULL` on a populated table, and PG rolls the
plan back — so it was exactly the ordinary nullable column that was destroyed silently.
**SQLite has its own arm (`emit_sqlite`) rather than the fall-through to MySQL's it used to
take**, because two shapes there are refusals and not merely infelicities: its `ALTER TABLE`
takes exactly one operation — there is no clause list — so two dropped columns are two
statements, and an index comes off by a standalone `DROP INDEX` as on PostgreSQL, MySQL's
`ALTER TABLE … DROP INDEX` having no SQLite form at all. It emits indexes before columns
(SQLite refuses to drop a column an index still names, so the two in one plan only work in
that order) and filters every change through `supports_change`, so a gate that drifts open
shows an empty preview instead of handing SQLite MySQL's spelling of the change
(`ddl::sqlite_drop_tests`). **Every loop, and one of them was not.** The table-rename loop
iterated `&self.changes` where every loop above it iterates `supported()`, and
`supports_change(Sqlite, RenameTable)` is *false* — so a set holding a bare rename reported it
in `unsupported()`, printed the `-- INCOMPLETE … would leave out: Rename the table` header,
refused Apply, and still put the `ALTER TABLE … RENAME TO` in the script the escape hatch hands
over. That is the shape `container_creates`' own doc records this codebase having paid for once
already, where a `DROP SCHEMA` went into a script under a header saying that very change had
been left out. It is not reachable through today's menus — `diff`'s SQLite collapse always pairs
a rename with a `RebuildTable` — which is why it is Low and why the test says so, and the
rebuild branch returns before this loop, so the rename it inserts *deliberately*, as the native
statement that re-points dependent objects, is untouched. Its `AddColumn` arm writes
`ALTER TABLE <t> ADD COLUMN <definition>` from `ColumnInfo::definition_sql` — the same column
emitter the rebuild's `CREATE TABLE` uses, so the two can't disagree about what the column is,
only about how it gets there — and adds come **after** drops, so a column replacing a dropped
one of the same name finds the name free. Where the set holds a `RebuildTable` that arm returns
the rebuild's statements and the trailing `RENAME TO` **and nothing else** — the other entries
describe what the rebuild achieves, so emitting them beside it would apply the same edit twice.
`create_table_sql` splits on the same fault line, and for the same reason: it used to split on
PostgreSQL and give everything else MySQL's shape, which for SQLite is not unidiomatic but
invalid — inline `KEY`/`UNIQUE KEY`, `ENGINE=`, `COLLATE=` and `COMMENT=` are each a syntax
error there. It now sides with PostgreSQL on indexes (statements of their own) and has no table
options at all — except the two suffixes that change what the table *is*, `WITHOUT ROWID` and
`STRICT`, which the draft carries and a rebuild that didn't restate them silently dropped. Its
one real divergence is the **inline single-column key**, which is legal only as
`INTEGER PRIMARY KEY [AUTOINCREMENT]`: that column takes the whole declaration and the
table-level `PRIMARY KEY (…)` clause stands down rather than declaring a second key (a composite
key is unaffected). **`AUTOINCREMENT` is a narrower question than "server-assigned"** and has
its own flag, `ColumnInfo::sqlite_autoincrement`: every `INTEGER PRIMARY KEY` is the rowid and
is filled in for you, which is what `auto_increment` says, while the keyword adds the promise
never to reuse an id and a `sqlite_sequence` row to keep it — so reading the first as the second
put `AUTOINCREMENT` on every plain key a rebuild touched. `ColumnInfo::definition_sql` makes the
same distinctions — SQLite keeps `COLLATE`, the generated expression (with `STORED` when
`generated_stored`, since its default is `VIRTUAL`), `NOT NULL` and `DEFAULT`, parenthesising an
expression default because `pragma_table_xinfo` strips the parentheses the grammar requires
(`schema::is_bare_default`), and drops `AUTO_INCREMENT`, `ON UPDATE` and the column
comment (`ddl::sqlite_create_tests`).
**That parenthesis is not SQLite's alone**, which is why the predicate is
`is_bare_default(d, dialect)` and answers for all three engines rather than for one. MySQL 8
prints an expression default parenthesised in `SHOW CREATE TABLE` — the runnable form — and does
*not* in `information_schema.COLUMNS.COLUMN_DEFAULT`, so
`b varchar(30) DEFAULT (CONCAT('a','c'))` restated from the model as `DEFAULT concat(…)` is
`ERROR 1064` (measured on 8.4.11, where the wrapped form is accepted); MariaDB 10.11.14 accepts
both and normalises the pair away, so one answer serves the family. Over-wrapping is not free in
the other direction — MySQL reads `DEFAULT (7)` as an *expression* default rather than a literal
one — so the predicate states the grammar positively per dialect: a string or blob literal, a
signed number, `NULL`/`TRUE`/`FALSE` and the `CURRENT_*` keywords everywhere, plus on MySQL its
`b'…'` bit literal, the `CURRENT_TIMESTAMP` family's optional precision, and the character-set
introducer its own catalogue writes (`_utf8mb3'draft'`, which is how MySQL 8 reports a *literal*
it recorded as an expression default — part of the literal, not a reason to re-wrap it). On
PostgreSQL everything answers `true` and nothing is ever wrapped, `pg_get_expr` returning what
the grammar takes bare. The test module is `schema::default_clause_tests`, no longer SQLite's
alone (`a_mysql_expression_default_is_parenthesised`,
`a_mysql_expression_default_is_wrapped_whatever_shape_it_is`).
**And the question is balanced, not two-ended.**
It asked `starts_with('(') && ends_with(')')` back when it was `is_bare_sqlite_default`, and the
pragma strips only
the *outer* pair — so `DEFAULT ((1+2)*(3+4))` arrives as `(1+2)*(3+4)` and `DEFAULT (('x')||('y'))`
as `('x')||('y')`: a paren at each end, and an expression between them. Called bare,
`definition_sql` re-emitted them without the wrapper and SQLite refuses the result (`near "||":
syntax error`, measured on SQLite 3.45.1). Since `create_table_sql` builds the rebuild's new
table from *every* column's `definition_sql`, editing any column of such a table aborted the
whole twelve-step rebuild with a message about a column the user had not touched — the table was
permanently uneditable through the designer, and via Run all it landed with the table already
dropped. It asks `sql::balanced_paren_span` now, which is the one-boundary-lexer invariant's own
helper, so a paren inside a string literal is not a paren
(`a_parenthesised_pair_that_is_not_one_group_still_needs_wrapping`).
**SQL Server's `create_table_sql` sides with PostgreSQL and SQLite on indexes** — T-SQL has no
inline non-key index, so each is a `CREATE [UNIQUE] INDEX` after the table, and
`create_index_sql` writes no `USING` there (`writes_index_method`), the method clause not being
T-SQL's — and it takes none of MySQL's `ENGINE=`/`COLLATE=`/`COMMENT=`: the table's and the
columns' comments follow the table as `sp_addextendedproperty` calls, as PostgreSQL's follow it
as `COMMENT ON` (`create_table_on_sql_server_writes_comments_as_extended_properties`). So the split is asked of
two capabilities, `inlines_indexes` and `takes_table_options` (public, since the designer asks
it for its storage-engine and table-collation row), both MySQL's alone, where
it had been `!pg && !sqlite` — a test a fourth engine passes as MySQL. The columns are
`ColumnInfo::definition_sql`'s T-SQL arm (under `schema.rs`), and a check writes no
`NOT ENFORCED` there (`CheckInfo::clause_sql`, asking `writes_not_enforced`), T-SQL having no
such clause (`create_table_on_sql_server_writes_t_sql`,
`create_table_on_sql_server_writes_no_mysql_options`).
**A partitioning is a `create`-only field**: `TableDraft::partitioning` and `partition_of`,
which `from_table` clones off `TableInfo::partitioning` and `TableInfo::partition_of` (under
`schema.rs`). `create_table_sql` writes a partition as
`CREATE TABLE q PARTITION OF parent [(…)] <bound>`, its parenthesised list holding only its
own foreign keys and checks, since the parent gives it its columns and key, and appends
`Partitioning::clause_sql` to a partitioned table or a sub-partitioned partition — **after the
table options**, where MySQL takes it, so a MySQL table reads `) ENGINE=InnoDB PARTITION BY
RANGE (…)` and then its partitions, PostgreSQL's the key alone; an inherited foreign key or
index (`inherited`) is skipped, coming with the partition from its parent. **The clause is
written only where `schema::restates_partitioning` says a `CREATE TABLE` carries one** — false
on SQL Server, which now reads a `Partitioning` too, so without the filter MySQL's `PARTITION
BY` spelling would reach T-SQL; the comparison holds such a table back instead of creating it
plain (under `compare.rs`). The draft held the
key alone (`partition_key`) until MySQL's were read, which a table that must declare its
partitions cannot be created from.
**`diff` never reads either**, because no `ALTER` changes a table's partitioning — so an edit in
the designer cannot express one, and the comparison holds a repartitioned table back rather
than letting the two sides read as the same table (under `compare.rs`). Copy to clears both
(under `copy_table.rs`). The comparison's
`a_missing_partitioned_table_is_created_with_its_partitions` is the pin on this emitter.
**Clustering is written wherever T-SQL's default would be wrong, and the default differs by
kind**: a key is `CLUSTERED` unless it says otherwise, an index or a unique constraint
`NONCLUSTERED`. So `create_index_sql` writes `CREATE [UNIQUE] CLUSTERED INDEX` for an
`IndexInfo::clustered` of `Some(true)`, and `emit_mssql`'s unique-constraint re-add writes
`UNIQUE CLUSTERED` likewise, while `key_clustering` gives a key ` NONCLUSTERED` for `Some(false)`
and nothing otherwise — the one spelling `emit_mssql`'s `ADD … PRIMARY KEY`,
`create_table_sql`'s key clause and `tsql_create_ddl`'s key share. The key's clustering rides on
`TableDraft::primary_key_clustered`, which `from_table` reads off the `PRIMARY` index, and on
`Change::PrimaryKey::clustered`, which `diff` fills from the draft — so a key redrawn over other
columns keeps what it had — and `repair_tsql_dependents` from the key's own index. Either
default taken wrongly is a different table: a clustered index recreated plainly leaves the
table a heap, and a `NONCLUSTERED` key re-added plainly becomes the clustered one, or is refused
where another index already is (Msg 1902)
(`sql_server_recreates_a_clustered_index_clustered`,
`sql_server_keeps_a_nonclustered_primary_key_nonclustered`). `None` — every other engine, and a
key or index the designer is making — takes the default; the designer has no control for it.
**`validate` counts the table's clustered structures**, since a table holds only one: the key, where
it is non-empty, `key_clusters_by_default` answers yes and `primary_key_clustered` is not
`Some(false)`, plus every other index whose `clustered` is `Some(true)`, a clustered columnstore
among them. More than one is refused — *"A table holds only one clustered index, and the
primary key and index X each cluster it - make all but one nonclustered."* — ahead of the
server's Msg 35372 or 1902 (`sql_server_index_shapes_it_refuses_are_refused_first`). With no
control for the key's clustering, a clustered columnstore on a table whose key clusters by
default is now refused up front rather than at the server; a key read as `NONCLUSTERED` passes,
`from_table` carrying it (`a_clustered_columnstore_names_no_column_and_is_its_own_index` makes
its key nonclustered for exactly this reason, its old fixture being a table SQL Server refuses).
**An index's included columns are `IndexInfo::include`, and `create_index_sql` writes them**
after the key list and before `WHERE`, through `include_sql` — so the designer's `AddIndex`, an
indexed view's indexes, the T-SQL rebuild and SQLite's replay all go through it, the last
writing nothing (`an_index_restates_its_included_columns`). Every place that reads an index's
columns reads the list as well. `indexes_equal` compares it, or an edit of
the list alone diffs as nothing and never lands (`an_include_only_edit_is_an_index_change`).
`rename_index` and `TableDraft::move_references` carry a rename into it, so an index on a
renamed column compares unchanged. `TableDraft::remove_column` takes a removed column off the
list and **keeps the index** — a key column still takes the index with it — so the diff drops
the index ahead of the `DROP COLUMN`, which T-SQL would otherwise refuse, and adds it back
without the column (`an_include_follows_a_rename_and_a_removal`, which pins the draft's half).
`validate` refuses a list naming no column (*"Index X includes c, which isn't a column."*,
`an_include_naming_no_column_is_refused`) and deliberately **not** a key column repeated in the
list: SQL Server refuses that (Msg 1909) and PostgreSQL 16 accepts it, both measured, so it is
the server's question, asked by the server. It does refuse a list where `index_takes_include`
answers no, for a draft built other than through the form — *"…a clustered index holds every
column already."* or *"…a unique constraint has no INCLUDE here."*
(`sql_server_index_shapes_it_refuses_are_refused_first`); a columnstore index has its own
refusal (below), and an XML or spatial one is left to its kind's.
**A columnstore index is written as one, and only on SQL Server** (`supports_columnstore`, an
exhaustive `match` with `MsSql` alone on `true`): `create_index_sql` writes `CREATE CLUSTERED
COLUMNSTORE INDEX ix ON t;` with no column list, or `CREATE NONCLUSTERED COLUMNSTORE INDEX ix ON
t (…)` with its `WHERE`, and never `UNIQUE` or `INCLUDE` — so the designer's `AddIndex`, the
T-SQL rebuild and `tsql_create_ddl` (under `schema.rs`) restate one the one way
(`a_columnstore_index_is_restated_as_one`, `a_sql_server_rebuild_restates_a_columnstore_index`).
`indexes_equal` compares `is_clustered_columnstore` as well as the method: a clustered
columnstore is the table's storage and a nonclustered one a copy beside it, the same method and
different indexes, so an edit between the two must not diff as nothing
(`a_clustered_columnstore_names_no_column_and_is_its_own_index`). `validate` lets a clustered
one name no column and refuses a columnstore index that is unique (*"…a columnstore index
stores columns, not keys"*) or includes columns, T-SQL taking neither on one
(`a_columnstore_index_is_refused_uniqueness_and_an_include_list`), and one that sorts a column
`DESC` (*"…a columnstore column has no direction."*, Msg 35302) or, clustered, is filtered
(*"…a clustered columnstore index stores every row."*, Msg 102 on `CREATE CLUSTERED
COLUMNSTORE INDEX … WHERE`), both measured on 2022. `IndexStorage` — *Rowstore*,
*Columnstore*, *Clustered columnstore* — is the designer's choice mapped onto `method` and
`clustered`, so the form only shows it: `apply` clears what the choice cannot carry, the unique
flag, the include list and every column's direction, and a clustered one's columns and filter,
rather than leaving `validate` to refuse it, and back to rowstore clears both fields, the index
taking T-SQL's default clustering again (`the_storage_choice_maps_onto_the_index_and_back`).
**The direction and the filter were the two it missed** (S3.1-L1-02): a filtered `(qty DESC)`
index switched to *Columnstore* kept its direction, and to *Clustered columnstore* its filter —
and the filter field is PostgreSQL's alone, so on SQL Server that filter could not even be
cleared from the form
(`a_columnstore_index_keeps_no_filter_or_direction`; live,
`a_storage_switch_to_columnstore_applies`). **An ordered columnstore is written
with its `ORDER`** (`IndexInfo::order`, under `schema.rs`): ` ORDER ([b], [a])` after a
clustered one's table or a nonclustered one's column list and before its `WHERE`, the grammar's
place, measured on 2022 and 2025, and only where `supports_columnstore_order` answers yes
(`an_ordered_columnstore_is_restated_with_its_order`). It was read `lossy` until the model held
it, so a rebuild refused its table. `indexes_equal` compares `order`, or an edit of the order
alone would diff as nothing and never land (`an_order_only_edit_is_an_index_change`). Being a
column list rather than text, it follows the draft as `include` does: `move_references` renames
it, `remove_column` takes a dropped column out of it, and `diff`'s `rename_index` renames it
too, so an ordered index over a renamed column compares unchanged rather than as a drop and a
create (`an_order_follows_a_rename_and_a_removal`). `validate` refuses an `ORDER` on any index
but a columnstore one, one naming no column of the table, and a nonclustered one ordering by a
column it does not store — SQL Server's Msg 1911, measured on 2025
(`an_order_is_refused_what_it_cannot_name`); and `IndexStorage::apply` clears `order` for every
kind but the two columnstore ones. **The column-change repair deliberately does not count it**:
`repair_tsql_dependents` asks an index's key and included columns whether a change disturbs
them, not its order, because an `ALTER COLUMN` on a column an ordered clustered columnstore
sorts by is allowed with the index in place (measured on 2025) — and a nonclustered one's order
names only columns it stores, which the key check already asks.
**A filtered index's condition holds its columns against a rename, where its key does not**:
`sp_rename` renames a key column under its index, but refuses one a filter names (Msg 5074
"The index 'ix_live' is dependent on column 'n'", measured on 2022). So the repair also takes
an index off and puts it back when its `predicate` reads a column the plan renames, and
`remake_index` repoints that predicate across all the renames in one pass with
`repoint_check_columns` — one rename after another, in the renames map's `HashMap` order, turned
`a → a2` beside `b → a` into a filter on `a2` twice, at random
(`a_rename_chain_under_a_filtered_index_filters_the_right_columns`) — there
and not in `rename_index`, which `diff` compares the draft through, and the draft keeps
the condition as it was read (`sql_server_remakes_a_filtered_index_around_a_rename_its_condition_names`;
live, `a_column_a_filtered_index_names_is_renamed`, red with Msg 5074 before and green on 2022
and 2025 after).
**An XML or spatial index is written as one too, and also only on SQL Server**
(`supports_xml_and_spatial_indexes`, the same exhaustive shape): `create_index_sql` writes
`CREATE {method} INDEX ix ON t (col)` — `PRIMARY XML`, `SELECTIVE XML`, `XML` or `SPATIAL` —
followed by `IndexInfo::using` where there is one, a secondary XML index's `USING XML INDEX
[primary] FOR PATH`, a selective one's `[WITH XMLNAMESPACES (…)] FOR (path = N'…' [AS …], …)`,
a secondary selective one's `USING XML INDEX [sxi] FOR ([path])` or a spatial one's
tessellation and its options, so the designer, the rebuild and `tsql_create_ddl` restate it the
one way (`an_xml_or_spatial_index_is_restated_as_one`,
`a_selective_xml_index_is_restated_as_one`). The selective kind needed no branch of its own:
its keyword is its `method` and the rest is `using`, as for the others.
`indexes_equal` compares `using`, or an edit of the clause alone would diff as nothing.
`validate` refuses one that names other than exactly one column, or is unique, covering or
filtered, a secondary XML index with no `using`, which is built on a primary or selective one
by name (`an_xml_or_spatial_index_is_refused_what_it_cannot_take`), and a selective one with
none, whose promoted paths are what it is (in `a_selective_xml_index_is_restated_as_one`) — and
a secondary whose parent (`IndexInfo::xml_parent`) is no primary or selective XML index of the
draft, by its name or, for one the draft renames, by its `original`: *"Index … is built on …,
which isn't a primary or selective XML index of this table: it goes when its parent does"*,
since the server drops a secondary with its parent and will not build one on nothing (in
`re_creating_an_xml_parent_puts_back_its_secondaries`).
**An XML index a secondary is built on — a primary one, measured on 2022, or a selective one
(`IndexInfo::is_tsql_xml_parent`) — is ordered against its secondaries at both ends**, and the
two halves sit in different places on purpose. Added, it goes first: `emit_mssql`'s `AddIndex` phase writes a
parent XML index before the rest, since a secondary naming it is refused until it exists, and
`create_table_sql` stable-sorts its separate indexes the same way, as `tsql_create_ddl` does
(under `schema.rs`). It was the one of the three that did not, so a plan that creates the table
— a comparison into a database without it, the designer's new table — wrote a catalogue-ordered
`ixml_…` before its `pxml` and failed with Msg 6333
(`a_new_tables_xml_parent_is_created_before_its_secondaries`). Dropped, it goes last: dropping a primary drops every secondary built on it, so a secondary's
own `DROP INDEX` after it found nothing and failed the plan with Msg 3701. That half is set at
the end of `diff`, which sorts a `DropIndex` of a parent XML index behind every other change,
because the drop carries a name and no kind and only `diff` has the reading that says which
the name is (`a_primary_xml_index_is_created_first_and_dropped_last`,
`a_selective_xml_index_is_created_first_and_dropped_last`). Both halves key on
`is_tsql_xml_parent`, not on the primary kind, and that is load-bearing: asked of the primary
alone, as they were, a selective index could be written after a secondary built on it. The
sort runs after `repair_tsql_dependents`, so it orders the drops that repair adds as well — and it adds them for
an XML or spatial index on a column the plan's change disturbs now, which, no longer `lossy`,
is taken off and put back like any other. **Every one is taken off and put back when the plan
re-creates the clustered key** — its columns, its clustering, or the repair around a retype of
a key column — since the server will not drop the key while one stands on it (Msg 3734,
measured on 2022) and the repair, asking only what a change disturbs, never took them off:
right after `repair_tsql_dependents`, `diff` drops and re-adds (through `rename_index`) every
current XML or spatial index the plan has not already dropped or withheld, the sort above
dropping a parent last and `emit_mssql` adding the key before the indexes and a parent before
its secondaries. A `lossy` one is left, as the repair leaves one, and the server then
refuses the plan naming it (`an_xml_index_comes_off_before_the_key_it_stands_on`; live,
`a_key_under_xml_and_spatial_indexes_is_re_created`, under `mssql.rs`, red with Msg 3734
before). **A secondary the draft keeps is put back when its
parent is re-created**: dropping a primary or selective XML index drops every secondary built
on it, silently (measured on 2022), and the plan re-created the parent alone — on a rename, an
edit of its `using`, a comparison — so each secondary the draft still listed was lost with
nothing said. `diff` now drops and re-adds every current secondary the draft keeps whose parent
is in `dropped_ix`, the sort above dropping it ahead of the parent and the `AddIndex` phase
creating it after; one the draft edits too is re-created already. **One the draft renames is
re-made under its new name, not renamed**: its `sp_rename` ran after the parent's drop had
already taken it, and failed with Msg 15248 (IDX-01,
`a_renamed_secondary_is_not_touched_after_its_parent_goes`). **A parent the draft only
renames is not re-created at all** — it is an `sp_rename` (`Change::RenameIndex`, below),
and a secondary names its parent by id, so nothing under it moves. **Every secondary the plan
creates from the draft names its parent as the plan leaves it**
(`IndexInfo::using_xml_parent`): the draft's own parent of that name first, else the current one
the draft renamed — the two names `validate` accepts. The re-point once looked only at the
parents in `dropped_ix`, so a secondary edited or added beside a parent that was only renamed
named the old name, and the server refused it (Msg 6333, IDX-02,
`a_secondary_follows_a_parent_that_is_only_renamed`; both live, in
`index_renames_meeting_drops_and_each_other_apply`, under `mssql.rs`, red before). A
`lossy` secondary under
a dropped parent withholds the parent's edit instead — a `KeepLossyIndex` named for the
secondary — rather than re-make it from a partial reading. That arm is practically
unreachable, the reader leaving a secondary `lossy` only beside a `lossy` parent whose edit is
withheld already, and no test reaches it. `IndexStorage` has four kinds for them, *Primary XML*,
*Selective XML*, *Secondary XML* and *Spatial* — a secondary selective index is a *Secondary
XML* one whose clause names a path; `offered(dialect)` asks each family its own capability,
`takes_using()` is true for the selective, secondary and spatial kinds, `using_example()` is
the designer's placeholder for exactly those three — a selective index's `FOR ([name] = N'/path'
AS SQL int)`, a secondary one's `USING XML INDEX [primary] FOR PATH`, a spatial one's `USING
GEOMETRY_AUTO_GRID` — and `None` for the rest, the two pinned to agree kind by kind in
`the_storage_choice_maps_onto_the_index_and_back`, and `of` reads all seven
off `method` (and `clustered`, for the two columnstore kinds). `apply` clears `using`
whenever the kind changes — the clause is one kind's grammar and no other's — and for an XML or
spatial kind clears clustering, the unique flag, the include list and the predicate and keeps
the first column only; back to rowstore clears `clustered` only when the index was another kind.
A **selective** XML index was read `lossy` until `using` held its paths (under `mssql.rs`).
**The preview's line for an added index names its kind and every list it carries** —
`Change::summary` writes *"Add [unique] [kind] index ix [using method] on (keys [desc])
including (…) ordered by (…) [where filter]"*, the kind from `IndexStorage::summary_noun` (none
for rowstore), a method that is no such kind — PostgreSQL's `gist`, `gin` — as `using`, the
`on (…)` left out when there are no keys and an expression key shown as its SQL. It once wrote
the key column names alone, so a clustered columnstore index read *"Add index ix_cs on ()"*, as
did a PostgreSQL expression index, and an edit of an index's includes, order or kind — and,
until S3.1-L1-04, its method, a key's direction or its filter — previewed as a drop and an add
of the same line (`an_added_index_is_summarised_with_its_kind_and_columns`).
Ordering
is dependency-first (FKs and indexes off before the columns under them; keys back on
after), and **the column clauses inside that are ordered by their dependencies rather than
grouped by change kind** — `ColumnClause` carries the name a clause makes available and the name
its `AFTER` anchors on, `anchor_of` reads the second off a `Position`, and
`ordered_column_clauses` is the walk. MySQL applies the clauses of one `ALTER TABLE` in order, so
an anchor is evaluated against the table as it stands at that clause. The emitter used to emit
all the changes and then all the adds, under a comment that read the constraint backwards —
*"adds come last so a new column's `AFTER` can name one added earlier"* — which is what makes an
**add**'s anchor safe and an **alter**'s impossible: on `t(a int, b int)`, adding `c`, moving it
first and then moving `b` above `a` gives `apply_positions` a target of `[c, b, a]`, and the
statement came out `MODIFY COLUMN b int AFTER c, ADD COLUMN c int FIRST`, which MariaDB 10.11.14
and MySQL 8.4.11 both answer `ERROR 1054 Unknown column 'c' in 't'` — naming a column the user
can see on the screen, and on an engine whose DDL does not roll back, so on a plan with
statements in front of it the refusal is not merely a refusal. The naive swap (adds first) is
wrong in the other direction, a new column anchored `AFTER` a column being *renamed* needing the
`CHANGE COLUMN` ahead of it — so what this reproduces is `apply_positions`' own simulation rather
than any kind's precedence. It is **stable**: a clause with nothing to wait for keeps its place,
so a change set with no anchor on a new name emits exactly what it emitted before. A cycle cannot
come out of `apply_positions`, which assigns positions by walking one linear target order; if one
ever arrives the remaining clauses go out in input order rather than being dropped or looped
over, the server's refusal being a better outcome than a silently shortened statement
(`a_move_anchored_on_a_new_column_is_emitted_after_the_add`,
`an_add_anchored_on_a_renamed_column_is_emitted_after_the_rename`).
**`emit_mysql` builds those clauses in *one* pass over `changes`, and only over the changes this
engine supports** — two separate things, and the loop structure that held them both was wrong on
each. *One* pass, because `apply_positions` computes each `AFTER` against a simulation it
advances in target order and `reorder_moves` permutes `changes` into that order; collecting every
`AlterColumn` and then every `AddColumn` threw that order away, so a positioned `ADD` could never
precede a positioned `MODIFY`. On `t(a,b,c,d)`, adding `e` and dragging to `[e,b,c,a,d]`, the
table lands `[e,b,a,c,d]` — no error, nothing withheld, "applied" reported, measured on MariaDB
10.11.14 and MySQL 8.4.11. `ordered_column_clauses` cannot repair that one, a plain `MODIFY`'s
`provides` being `None` with nothing waiting on it. *Supported*, because `unsupported()` and
`emit()` are two halves of one decision and every other emitter that can meet a refused change
filters at the site — `view_statements`, `routine_statements`, the container builders,
`object_statements`, `event_statements`. The column clauses were the one family with no such filter, sound only while
MySQL's `supports_change` had no `false` arm an `AlterColumn` could reach; once it did, the
INCOMPLETE header read "would leave out: Change column g" with that exact `MODIFY COLUMN g`
printed below it, and Copy and Open-in-editor are deliberately still live, so that sentence is
the only thing telling the user what is missing. The refused change is a generated column moved
between `VIRTUAL` and `STORED`, and the refusal is the **engine's** — MySQL 8.4.11 answers
*ERROR 3106 'Changing the STORED status' is not supported for generated columns* and MariaDB
10.11.14 rejects the spelling outright (`ERROR 1064`), both measured. `supports_change`'s comment
used to justify it by an emitter gap (that `definition_sql` wrote no keyword, so the column came
back `VIRTUAL`); that gap was real and is closed, and it was never why this refusal is right.
`normalize_type`/`types_equal` + `defaults_equal` are the reason a designer
opens clean — `int(11)` ≡ `int`, `character varying(45)` ≡ `varchar(45)`. **The
round-trip gate is test-enforced**: `TableDraft::from_table(t)` diffed against `t`
must be empty over captured fixtures from classicmodels/sakila/employees/world +
PG world/chinook + two SQLite shapes (`ddl::tests::roundtrip`), and a SQL Server
shape beside them (`a_sql_server_table_round_trips_through_its_draft`: an identity
key under a named `NONCLUSTERED` constraint with its seed and increment, a `rowversion`, a
`getdate()` default, a collation, a persisted computed column, a unique constraint, a
`CLUSTERED` descending index, a check, an alias type, table and column descriptions, and a
foreign key with actions whose referenced table carries it under `referenced_by` — each field
set as `collect_schema` sets it, which the test's doc states) — extend those
fixtures rather than working around them, since any model-fidelity gap surfaces to
the user as a phantom change. **On SQLite it is worse than a stray preview line**:
a non-empty diff is what routes an edit through the twelve-step rebuild, so an
invented change drops and recreates the table for an edit nobody made. The gate has
a second half a fixture cannot cover — `db::sqlite`'s
`an_introspected_table_diffs_to_nothing_against_its_own_draft` makes the same
assertion over **real introspection**, across a corpus declaring every fidelity
property the engine added; a fixture states what the reader is believed to produce,
and that belief is exactly what the range's findings kept disagreeing with. (SQL
Server's live leg has the same half, `every_sample_table_diffs_to_nothing_against_its_own_draft`
over every AdventureWorksLT table, skipped where that sample is not installed — which
includes CI. It asks `sys.databases` whether the sample is there before it reads it, so an
introspection that fails on a machine that has it is a failure, not a skip. Its sibling
`a_real_schema_reads_and_every_table_round_trips` does the same over a database someone uses,
named by `SCHEMAIC_IT_MSSQL_REAL_DATABASE` on whichever leg runs — on `mssql-on-azure` one
beside `schemaic_it`, never a scratch name, which the leg's wipe would empty — and adds the
account list and each table's rebuild plan, built and never run; what the reading cannot
rebuild or edit is printed, not failed on, since a real schema is entitled to features
Schemaic does not restate.) The SQLite
test found
three shipped bugs on its first run (`CreateTable` unhandled by `emit_sqlite`,
`validate` refusing an unnamed `CHECK`, and two unnamed checks pairing onto one
original). **`columns_equal` is the comparison under that gate, and it skipped four fields the
model carries.** `generated_stored`, `sqlite_autoincrement` and `identity_always` were absent
with nothing saying whether that was deliberate, so a draft changing one raised no change at all
— dropped with nothing on screen, which is the shape the preview exists to prevent. Each is a
decision the emitter restates: SQLite defaults a generated column to `VIRTUAL`, so a `STORED`
one re-emitted without the word stops being materialised and the trade the user chose is
reversed silently; `AUTOINCREMENT`'s whole purpose is that a freed id is never reused, and the
rebuild carries `sqlite_sequence` over for it, on a plan `diff` could not build; and `ALWAYS`
versus `BY DEFAULT` decides whether PostgreSQL *rejects an explicit write*. No designer control
produces such a draft today, which is why this was latent rather than reported — the day one
does (an import, an AI `propose_table_change`) the edit would have been dropped and the suite
stayed green. **`invisible` was the fourth**, and it is not latent: the reader and the emitter
both learned it while this did not, so comparing two MySQL databases whose column is `INVISIBLE`
on one side answered `Same` — the tree showed nothing, the plan carried nothing, and the
retirement the column was marked for was silently not migrated. `fks_equal` had the same class,
in `ForeignKeyInfo::match_type` and `deferrable`: a PostgreSQL `MATCH FULL DEFERRABLE INITIALLY
DEFERRED` key compared equal to the bare form, and both terms change what the key *enforces* —
under `MATCH SIMPLE` a partly-NULL composite key is accepted where `FULL` refuses it, and without
`INITIALLY DEFERRED` an application that inserts children before parents fails at statement time
against a database that restored fine. All three are `None`/`false` on the engines that cannot
report them, so the terms cost nothing there. The `users()` fixture populates all three now, so
the round-trip gate actually covers them and the fixture's own doc — *every field the model
carries is populated, so the round-trip test can't pass by leaving something empty* — is true
again. **`identity_spec` was the fifth**, the same way round: SQL Server's reader and
`tsql_identity` learned the seed and increment and `columns_equal` did not, so a compare
between a table counting down from 1000 and one counting up from 1 answered `Same`. It compares
them now, `None` standing for the `(1,1)` the emitter writes for it, and T-SQL having no
`ALTER COLUMN` for an identity the difference goes to the rebuild
(`an_identity_seed_or_increment_is_a_change`). **`ForeignKeyInfo::not_enforced`/`not_validated`
were the sixth**, in `fks_equal`: the reader and the `AddForeignKey` arm learned SQL Server's
disabled and untrusted key while the equality did not, so Compare schemas called a `NOCHECK` key
equal to an enforced one and the migration carried nothing. It compares both now, `false` on
every other engine, and the SQL Server round-trip fixture carries a disabled key and a disabled,
untrusted check (`a_disabled_or_untrusted_key_differs_from_an_enforced_one`). Also `key_list_text`/`parse_key_list` (the designer's `bio(20), age DESC`
field) and `common_types`. **That round trip is asserted twice here, and a quoted paren used to
break it**: `unwrap_parens` counted parens over raw `char_indices` and `split_keys` counted raw
bytes, so a close paren inside a string literal ended the group early. A PostgreSQL index keyed
`(lower((name || ')'::text)))` — which is `key_list_text`'s own output — came back from
`parse_key_list` as a plain *column* name, and `TableDraft::validate` then refused the whole
table with *Index ix names …, which isn't a column*, locking the user out of an edit they never
made; and `(f(a, '('))` left `split_keys` at depth 1, swallowing every key after it into the
first. Both are built on `sql::balanced_paren_span`/`skip_noncode` now — the one-boundary-lexer
invariant — as `peel_parens` five hundred lines below them already was, its own doc naming the
case. Pure + unit-tested.
**How much of a type's spelling may be canonicalised is a capability, `TypeAliasing`**, not a
dialect test. It was `dialect == Postgres`, which sorted SQLite onto MySQL's side — its alias
table, its meaningless integer display widths, its `BOOLEAN` → `TINYINT(1)` rewrite — and the
designer then answered *no change* to `INT` → `INTEGER`, which is the edit that makes a SQLite
column the rowid alias and the only declared type `AUTOINCREMENT` is legal on. The third arm is
`Verbatim`, and it normalises **nothing**: SQLite stores the declared text and derives an
affinity from it, so two spellings that differ are two different declarations even where the
affinity agrees, and collapsing them hides an edit. The enum is exhaustive so a fourth engine
has to be looked at rather than inheriting MySQL's. `common_types` gained a SQLite arm for the
same reason it needed one — it fell into MySQL's, offering `tinyint(1)`, `longtext`, `datetime`
and `year`, and no `INTEGER` at all.
**A `CHECK` is matched to its original by name, and an unnamed one has none.**
SQLite keeps a constraint unnamed — a bare `CHECK (a > 0)` in the table body is the
ordinary spelling there and `CheckInfo::clause_sql` emits it back that way — so
`diff` claims by name first and then pairs the unnamed ones **by predicate**, each
original claimable once, with whatever is left on either side the real drop or add.
Matching on the shared empty name gave every unnamed draft check the *first*
original and produced a `DROP`+`ADD` on an untouched table.
**`TableDraft::validate(dialect)`** takes the dialect for three rules, each a capability rather
than an engine test. `ddl::requires_named_checks` is false on SQLite alone, because MySQL and
PostgreSQL
assign a constraint name themselves and a blank one there can only be an unfinished
form. Demanding one everywhere made every SQLite table carrying an unnamed check
**uneditable** — the designer opened on an untouched table with a blocking footer
error and no field to fix it in. The predicate rule (`CHECK ()`) is unchanged and
applies to all three. The other two are the **table clauses the model carries and nothing
checked** — `requires_rowid_key`, since a `WITHOUT ROWID` table must name a primary key, and
`strict_type_allowed`, since a `STRICT` table takes only `INT`/`INTEGER`/`REAL`/`TEXT`/`BLOB`/
`ANY` and the most natural thing to type into the designer is `VARCHAR(20)`. `TableDraft` copies
both clauses in and its own doc says why ("the rebuild writes the table from *this*, so a clause
missing here is a clause the edit silently drops"), which makes them this validator's to judge —
and instead they reached the engine as `PRIMARY KEY missing on table t` and
`unknown datatype for t.a: "VARCHAR(20)"`, both measured on SQLite 3.50.4, by which point the
twelve-step rebuild has already dropped the original table. `TriggerDraft::validate` is the
precedent: refused in the modal, which can say which control is wrong, rather than at Apply.
Both answer for every engine — the clauses exist on no other one today, so there is nothing
there to require, and a fourth engine that grew them answers for itself.
**Index advice is `index_hints(draft, dialect) -> Vec<IndexHint>`, and it never enters
`validate`** — `validate` is what blocks Preview, and an `IndexHint` is advice, never a refusal.
Redundant indexes come first, in list order, then unindexed foreign keys; each carries its own
sentence (`IndexHint::message`, over `subject`, the name of what it is about). **Only a plain index is offered as redundant**: not unique —
it enforces what its cover may not — with no predicate, no `INCLUDE`, and not columnstore, XML
or spatial — and not `lossy`, which is neither advised against nor taken as a cover: it is how
introspection records what the model could not read (PostgreSQL's operator class, MySQL's
invisible index, SQLite's unread condition), and `text_pattern_ops` serves `LIKE 'a%'` where the
plain cover cannot. Its key columns must lead the cover's, compared ASCII-case-insensitively as
`validate` compares names, with the same prefix length, sort order, expression flag and per-key
collation (SQLite's `name COLLATE NOCASE` serves what `name` does not), and
under the same `method` (`a_lossy_or_differently_collated_index_is_no_basis_for_advice`). The primary key is no row of `draft.indexes` (`from_table` filters it
out — `find_key` below), so it is asked separately and first, and covers only a default-method
index. An identical pair flags only the **later** index, so taking the advice leaves one rather
than dropping both. A foreign key is covered by the primary key or any non-partial index whose
leading columns are its own in any order, and is asked about only where
`foreign_keys_create_their_index` is false — every engine but MySQL, whose InnoDB indexes a
key's columns itself; elsewhere a key with no index makes every delete or key update on the
referenced table scan this one (`an_index_led_by_another_is_flagged_redundant`,
`an_index_that_does_more_than_its_cover_is_not_redundant`,
`a_foreign_key_with_no_index_of_its_own_is_flagged_where_the_engine_makes_none`).
**`TableDraft::find_key(index, foreign_key) -> Option<DraftKey>`** says where one of a table's
keys sits in the draft — which of the designer's sections holds it, and which row of that
section — and it exists because the schema tree's sequence of keys and the draft's are **not
the same sequence**, so a position taken from one lands on the wrong row in the other. Three
ways they differ, one per `DraftKey` arm. The tree lists the primary key among the keys while
`TableDraft::from_table` filters `is_primary()` out of `indexes` entirely, so an index position
read off `TableInfo::indexes` is one row late in any table that has a primary key
(`DraftKey::Index`). The tree shows a foreign key under its **backing index**, whose name
needn't match the constraint's — classicmodels' `customerNumber` index backs `orders_ibfk_1` —
while the draft keeps `foreign_keys` as a collection of its own, so the lookup is by
*constraint* name and the index name is ignored (`DraftKey::ForeignKey`). And the primary key
is no row anywhere in the draft: it is the per-column tick `set_in_primary_key` writes, which
is what the index form's own hint — *"No index selected. The primary key lives on the
columns."* — tells the user, so `DraftKey::PrimaryKeyColumn` names its **first column in key
order** instead. The arguments are exactly what a `CtxKind::Key` tree row carries, asked
together because they answer one question. `None` for a key this draft doesn't hold, which is
the caller's cue to open on nothing in particular rather than on row 0
(`find_key_positions_an_index_in_the_primary_key_less_list`,
`find_key_sends_the_primary_key_to_its_first_column`,
`find_key_resolves_a_foreign_key_by_its_constraint_name`,
`find_key_answers_nothing_for_a_key_the_draft_does_not_hold`).
**CHECK constraints** are `CheckDraft`/`CheckInfo` → `Change::{AddCheck, DropCheck}`,
a drop-and-add on both engines (neither can alter one in place). Three rules are
written down because each was a bug waiting: the predicate is normalized **on the
way in** by `check_predicate` (PG's `pg_get_constraintdef` returns the whole clause,
MySQL's `CHECK_CLAUSE` a parenthesised predicate, a person types neither), so the
model holds it bare and the emitter wraps it exactly once; `checks_equal` compares
modulo wrapping parens and whitespace runs — via `peel_parens`, which goes through
`skip_noncode`, since `name <> ')'` has a close-paren in a string — but deliberately
*not* modulo tokenisation, so `qty>0` vs `qty > 0` costs a re-validating drop-and-add
rather than risking a kept edit; and a column rename is rewritten into the
predicate **only where the server won't do it itself** — PostgreSQL rewrites its own
stored parse tree and MariaDB rewrites a table-level check's text, while MySQL 8
*refuses the rename outright* (`ERROR 3959`) unless the constraint comes off first,
and SQL Server's `sp_rename` likewise (Msg 15336), so there and only there `diff`
adds a `DropCheck`+`AddCheck` pair with the predicate re-pointed through
`repoint_check_column` (a **token walk** over `skip_noncode`, so `qty` never
matches inside `qty_total` or `qty$x`, inside `'qty'`, or in a comment; it reads every
quoted-name form the engine does — T-SQL's `[qty]`, which opens and closes on different
bytes, and all three of SQLite's — as a name too). Several renames go through its plural,
`repoint_check_columns`, **as one substitution**: chained, `a → a2` beside `b → a` moved the
old `b` on to `a2`.
**A retype needs the same move on SQL Server, and for every kind of dependent.** `ALTER COLUMN`
is refused while an index, the key, a unique constraint, a foreign key or a check stands on the
column (Msg 5074 naming it, then 4922, measured on 2022), where MySQL's `MODIFY` and
PostgreSQL's `ALTER COLUMN … TYPE` rebuild and re-check them themselves and SQLite's column
change *is* the rebuild — so `alter_column_disturbs_dependents` is true there alone, and `diff`
runs `repair_tsql_dependents` after the check repair, whose pairs it then leaves alone. An
index stands on a column it only `INCLUDE`s as much as on a key column — T-SQL refuses the
`ALTER COLUMN` naming it either way (Msg 5074) — so the repair reads `IndexInfo::include`
beside the key, and `rename_index` re-points both. Which
change disturbs which kind is `tsql_alter_disturbs`, measured case by case because the
documented exceptions are not the server's: nothing is disturbed when only the name, default
or comment changes; a nullability change alone disturbs an index or key but not a foreign key
or a check; widening a `varchar`, `nvarchar` or `varbinary` — the same type, a length no
shorter, the same collation and nullability, `max` not counting as a length — disturbs a
foreign key only; any other change of type or collation disturbs all of them. **A computed
column survives none of it** — not a nullability change, not a widening, and not a rename
either (Msg 15336; the rest Msg 5074, measured on 2022) — so `tsql_alter_disturbs` answers yes
for one once anything but the name, default or comment changes, and the caller asks the rename
separately. Computed columns are worked out **first**, since what stands on a rebuilt one — an
index, the key, a foreign key, a check — is disturbed with it and goes off and back on too.
Each disturbed dependent goes off and back on as it was, in the
draft's column names: an index or unique constraint as `DropIndex` + `AddIndex`, a foreign key
and a check as their pairs, and the key as a `Change::PrimaryKey` whose `from` equals its `to`,
under the constraint name and with the clustering introspection read — a `NONCLUSTERED` key
re-added with T-SQL's default would come back clustered, or be refused beside a clustered
index (Msg 1902). The preview summarises that one as *Rebuild the
primary key (…) around the column change*, not the swap's *no longer unique* sentence, and its
risk says what it does cost: a clustered key rewrites the table, and a foreign key in another
table referencing it stops the plan. A computed column goes off and back on as a
`Change::RebuildComputedColumn`, carrying the column with its expression re-pointed at the
plan's new names through `repoint_check_column`. **It is a variant of its own, not a
`DropColumn` plus an `AddColumn`**, because the preview has to tell it from a real drop of a
column and an add of a computed one under the same name, which does lose the column's data.
`emit_mssql` drops it among the dropped columns (it has no default to look up), adds it back
`ADD [x] AS (…)` after the added columns, and re-adds its comment in the comment phase, since
the extended property went with the drop. The preview summarises it as *Rebuild computed column
x as (…) around the column change*, and its risk names the one cost: nothing is lost, the value
being the expression's, but the column moves to the end of the table, T-SQL having no reorder.
That move is why a plan holding one also refreshes the views and inline functions that select
`*` from the table (`tsql_moves_columns`, above): bound by position, one read `x` as `c` and an
`UPDATE` of `x` through it wrote `y` (S3.2-L5-01).
`supports_change` is true for it on SQL Server and false elsewhere — an exhaustive `match` after
SQL Server's early return — because only SQL Server's `diff` raises it and no other emitter
writes it. **A dependent the draft already drops or adds is the
draft's**, as with the check repair — and so is a computed column the draft itself alters or
drops; and a lossy index — one whose key names a graph table's internal column, say — is not
touched, so the server refuses the retype naming it and the plan, one transaction, rolls back
whole (`sql_server_takes_a_retyped_columns_dependents_off_and_back_on`,
`sql_server_rebuilds_only_the_dependents_a_change_disturbs`,
`sql_server_leaves_a_drafted_dependent_to_the_draft`,
`sql_server_rebuilds_a_computed_column_around_what_it_reads`). **One the model reads only in
part is refused before it is touched.** Put back from `IndexInfo`, an index states none of its
options: an `IGNORE_DUP_KEY` unique index came back without it, so the application's duplicate
inserts started failing; fill factor, page locks, compression, a disabled state and a
description went the same way, a key lost its fill factor, and a disabled, untrusted foreign
key came back enabled and trusted — each plan succeeding and reporting nothing lost
(S3.2-L5-02, measured). So `emit_mssql` opens with `tsql_in_place_guard`, the rebuild's kind of
guard over exactly what this plan re-creates rather than the whole table, whose `THROW` names
the object and ends *change it in SQL*: an index or unique constraint dropped and added back
under one name that carries a fill factor, padding, `IGNORE_DUP_KEY`, row or page locks off,
compression — neither of the last two counted where it is what a columnstore index is by nature,
as in the rebuild's guard (above) — a filegroup or partition scheme other than the default, a
disabled state, or an
extended property on the index or on its key constraint; the primary key re-created (a
`Change::PrimaryKey` with both sides non-empty) carrying any of the same; a foreign key or a
check re-added that is `NOT FOR REPLICATION` or carries an extended property — their disabled and
untrusted states the re-add restates (`WITH NOCHECK`, `NOCHECK CONSTRAINT`; the key's since
`ForeignKeyInfo::not_enforced`/`not_validated`), so neither is refused for those, where the key's
arm went on saying the re-add "would enable and trust it" after it no longer did (R2-L8-06,
`a_retype_under_a_disabled_or_untrusted_key_keeps_its_state`); and a rebuilt computed column
carrying an extended property other than the `MS_Description` the comment phase puts back, or a
column permission — `DROP COLUMN` takes a `GRANT` or `DENY` on the column, so renaming `salary`
under `annual AS (salary * 12)` lifted a `DENY SELECT` on `annual` and the denied user read it
(S2-L5-03, `a_rebuilt_computed_column_is_refused_where_it_carries_what_the_drop_takes`). A
sensitivity classification is not on that list because a computed column cannot carry one
(Msg 16111, measured on 2022 and 2025); an in-place `ALTER COLUMN` keeps both. **An index's
options are asked where its kind keeps them** (R3-L5-03): an XML or spatial index's compression
on its internal table's partitions, inside `tsql_index_carries`, and a columnstore index's
`COMPRESSION_DELAY` and an `XML_COMPRESSION` through `tsql_index_carries_late` — two
`tsql_late_arm`s for the key and for each index re-created, behind the rebuild's probes, so the
batch now has the rebuild guard's shape, the late arms ahead of the `CASE` (`DECLARE @t …
DECLARE @why … {late}SET @why = ISNULL(@why, CASE … END)`). Before them a key retype brought a
`PAGE`-compressed spatial index and an `XML_COMPRESSION` primary XML index back without either,
and a column retype a columnstore index without its delay, each plan applying — the live
`an_index_re_created_in_place_is_refused_over_hidden_options` was red on 2022 with the fix
stashed, and now sees each refused as *Re-creating the index*. **A secondary XML index has no
internal table of its own**: its rows are a nonclustered index on its parent's, and in either
table an index's rows are the internal index of its own name, a name that follows an
`sp_rename` (measured on 2022). So the internal-table term (`TSQL_INTERNAL_PARTITIONS`) finds
the table through `sys.xml_indexes.using_xml_index_id` where there is one, and the partitions
through the internal index named as the index is. Asked by `parent_minor_id` alone, a
compressed secondary was re-made uncompressed past the guard, while its parent was refused over
the secondary's partitions even with the secondary deleted (IDX-04). **A table's clustered
rowstore index is not asked its own partitions' compression**: that compression is the
table's, which the heap its drop leaves keeps and its re-create takes again — a `PAGE` and
`XML_COMPRESSION` clustered key retyped under, and a `PAGE` clustered index widened, came back
compressed (measured on 2022) — so every key change on a compressed clustered key was refused
over nothing it lost (IDX-05). A nonclustered key, or any other index, loses its own and is
still refused; the rebuild's guard, which drops the whole table, still asks every partition
(`the_guard_asks_compression_where_it_lives`; live, `index_compression_is_asked_where_it_lives`,
red on 2022 before both).
**It refuses rather than restates**, as the rebuild's guard does. It keys on the name, not on who raised the pair, so
the designer's own edit of an index, foreign key or check that keeps its name is refused by the
same arm — deliberately, the loss being the same
(`re_creating_a_dependent_is_guarded_against_what_the_model_does_not_read`).
**Keyed on the name, it never saw a rename** (R3-L5-04): an index the designer only renamed was
a `DropIndex` of the old name and an `AddIndex` of the new, which the guard's `both(..)` never
pairs, so the gentler edit lost exactly what an in-place edit of the same index is refused over
— fill factor, compression, filegroup, `IGNORE_DUP_KEY`, locks, extended properties — and a
unique constraint renamed in the designer went back under its **old** name, since
`IndexInfo::constraint` kept the name it was read with and the re-add writes `ADD CONSTRAINT`
from it. A rename is now `Change::RenameIndex { from, to, constraint }`, asked of
`renames_indexes_in_place` — an exhaustive `match`, SQL Server alone on `true`. `diff` raises it
where the draft index differs from the current one only by name — compared after `rename_index`
re-points renamed columns, with the name and constraint set to the draft's — **even for a
`lossy` index**, since a rename restates nothing. `emit_mssql` writes it straight after the
index drops, because a dropped index frees its name first and an added one may take the old
name after, in the order `tsql_rename_steps` gives: a rename waits while another rename's index
still holds its new name, so a chain runs from its free end, and where every rename left waits
on another — a swap, any cycle — the first steps aside through a temporary
`schemaic_rename_N` that no rename holds or takes. In draft order, each of those renamed onto a
name still in use (Msg 15335, IDX-03, `index_renames_onto_freed_or_swapped_names_run`, which
runs every plan over the table's index names as the server would). Each step is
`EXEC sp_rename N'[s].[t].[old]', N'new', N'INDEX';`, or, for an index a constraint
backs, `EXEC sp_rename N'[s].[constraint]', N'new', N'OBJECT';`, which renames the constraint
and its index together. The preview reads *Rename index ix to ix2*, with no risk sentence. The
other engines keep the drop and the create — none of their emitters writes the change, so
`supports_change` answers it through the capability for them, and `tsql_supports` for SQL
Server. Separately, and on every engine, a constraint-backed index whose name changed while its
`constraint` still equals the old index name takes the new name as its constraint, so one
renamed **and** changed is re-added under the name the draft gave it
(`a_renamed_sql_server_index_is_renamed_in_place`; live, `a_renamed_index_keeps_what_it_had`,
under `mssql.rs` — a `FILLFACTOR = 80`, `PAGE`-compressed index, a unique constraint, a primary
XML and a spatial index renamed together, each an `sp_rename` with its options kept, the
constraint named anew, a re-read diffing empty — red on 2022 with the change stashed). **A
`RenameIndex` is only ever of an index the plan otherwise keeps, and every `DropIndex` names an
index as it is before the plan**: one renamed *and* otherwise changed — or only renamed but
taken off around a retype (`repair_tsql_dependents`), its clustered key or its XML parent
(`remake_index`) — is dropped under the name it has and created under the new one, and all the
drops run before any rename. The drop says itself that the index comes back (`DropIndex`'s
`remade`), and the guard asks of exactly those, by the name they have — so a renamed and widened
index is still asked what it carries. The first answer was the other way round: such an index
was renamed first and dropped under its new name, so the guard's name-pairing saw it, and the
emitter ran the drops in two halves around the renames, decided by name. That broke three ways —
a renamed secondary was renamed after its parent's drop had taken it (IDX-01), a deleted index
whose name a rename took was dropped after the rename onto it (IDX-03), and a deleted index and
one re-made under its name paired by name in the guard. The passes that address a current index
after the index pass — the dependents repair, the key-bound XML/spatial pass and the
parents-last sort — key on the name before the plan, and name what they put back as the plan
leaves it (`index_name_after`, over the draft's index renames the plan carries) (same tests;
live, a renamed-and-widened index with a fill factor refused, one with none re-made).
`DropCheck` carries a risk sentence though it deletes no data — the table stops
guaranteeing something and nothing else says so — but `ChangeSet::destructive`
suppresses it when the same name is re-added in the same plan, since every check
*edit* is a drop-and-add and "rows the constraint refused are accepted from now on"
is simply false about one. **Foreign-key names are on that same re-added list now**, and an
index drop marked `remade` is passed over the same way, because `diff` recreates both: without
them, the two risk sentences a dropped unique index and a dropped foreign key had been missing
would have put a false loss on every ordinary index or FK *edit*. The index asks its own flag,
not the list, since one re-made under a new name has no add of its own name and a deleted one
may share its name with another renamed onto it (`a_re_added_constraint_is_not_a_loss`).
`a_re_added_constraint_is_not_a_loss` pins that composition, which is the half
neither predicate can be tested for alone. `enforced` is MySQL's `NOT ENFORCED` only — the
server's, not the dialect's, so the designer's toggle asks `supports_unenforced_check` with the
flavour, MariaDB having no such clause: PG's
`NOT VALID` exempts existing rows and so can't silently change what a write does.
**`CheckInfo::column_level` is MariaDB's `CHECK_CONSTRAINTS.LEVEL`**, and it is the
one place a check is not a table-level object: `q INT CHECK (q > 0)` makes a
constraint that is *part of the column*, so `MODIFY`/`CHANGE COLUMN` deletes it
unless the clause restates it (measured on 10.11.14 — the constraint is simply gone
and the next `-5` is accepted). It has no name of its own — the syntax refuses a
`CONSTRAINT` label at column level, MariaDB names it after its column and renames it
with the column — and `DROP CONSTRAINT` cannot address one at all (`ERROR 1091`).
So the emitter goes through the column both ways: `Change::AlterColumn::inline_check`
restates an unchanged one (re-pointed when the column is renamed, or the `CHANGE` is
`ERROR 1054`), and a check the draft **dropped or edited** has its impossible
`DropCheck` swapped for a `MODIFY COLUMN` without the clause. MySQL 8 has none of
this — it folds the same syntax into a table constraint at `CREATE` time — so all of
it is gated on `ServerFlavour::MariaDb`, and a MariaDB older than 10.5 (no `LEVEL`
column) degrades to the pre-flag behaviour rather than losing its checks.
**Views** ride the same rails: `ViewDraft` (name + body + the `ViewOptions` it
carries) → `diff_view` → `Change::{CreateView, ReplaceView, RenameView, DropView}`
→ the same preview. One engine rule each lives here.
`ViewDraft::validate(dialect)` takes a dialect for one question — could this body be a view's at
all — and `can_be_view_body` answers it from the **head keyword** only, deliberately: that is
the same answer mid-edit as on a finished statement, which is what both callers need, the draft
and the editor's right-click menu deciding whether *Create view* applies to the statement under
the cursor. It reads that keyword through `sql::leading_words` rather than a `char::split`,
because the split took `--` as the head word — so a body opening with a comment, which is the
ordinary way to write one, answered *not a query*: *Create view* vanished from the editor's
context menu with no explanation, and the view editor refused a pasted body every engine
accepts. It was the one scanner in this module not built on `sql::skip_noncode`, which
`unrestatable_sqlite_clauses` and `is_begin_end_block` both are, and it keeps the `(SELECT 1)`
case the split handled by accident, a `(` not being a word.
`Change::RefreshView` is the exception to the draft-and-diff shape, and joins
`TruncateTable` in coming straight off the context menu: a **materialized** view's rows
are not part of its definition, so there is nothing to diff — the whole change is which
of the two statements to send. `supports_concurrent_refresh` decides that from the
view's own indexes, because `CONCURRENTLY` is a *capability of the view* rather than a
preference: PostgreSQL takes an `ACCESS EXCLUSIVE` lock for the plain form, and refuses
the concurrent one without a `UNIQUE` index that is neither partial nor keyed on an
expression. Schemaic adds a fourth condition the server never states — the index must
not be `IndexInfo::lossy`, because there what was read back is not the whole index and
the other three were checked against a fragment. Uncertainty resolves to the plain
refresh, which always works.
A never-populated view is the one case the model can't see (`pg_class.relispopulated`
isn't in it) and the server's own refusal is the answer there. **`refresh_view_change`
is the whole decision** — is there anything to refresh, and which form does it take —
in one function over one `TableInfo`, because the two halves used to be assembled in the
schema menu's closure, which closes over a `Ui` and so could not be tested at all; what
that left uncovered was never either predicate but their composition with the caller.
`supports_change` gates
the change to PostgreSQL exhaustively, and `view_statements` asks it again at the
emitter: `emit_sqlite`'s `supported()` filter was the only `supports_change` in any of
the three emitters (`emit_mssql`, the fourth, filters the same way), and this builder runs
before even that one, so an engine with no
materialized view would otherwise be handed a
statement it has no word for. **Its `DropView` neighbour now asks the same question, and it
needed it more**: `supports_change` distinguishes `DropView { materialized: false }` (SQLite
can) from `{ materialized: true }` (it cannot), so the arm answering precisely that question was
the one emitting without asking — a `DROP MATERIALIZED VIEW` in the handed-over script under an
INCOMPLETE header naming that change as left out. Unreachable today, a SQLite view never being
materialized, and guarded anyway for the reason the rename loop next door was. It carries no
`risks`, so the preview's Apply stays
`Primary` and no confirmation is asked; the lock is stated in `summary()` instead,
where the SQL alone doesn't show it. MySQL's `CREATE OR REPLACE VIEW`
replaces the *whole* view, so the emitter restates `ALGORITHM`/`DEFINER`/
`SQL SECURITY`/`CHECK OPTION` — omitting the security type silently turns a
`DEFINER` view into an `INVOKER` one, which is a privilege change, the same class
of bug as `MODIFY COLUMN`'s. PostgreSQL's may only **append** columns, so an edit
that renames/retypes/reorders one needs `DROP` + `CREATE`, which takes dependent
views and grants with it: `pg_replaceable` (over `intel::select_output_names`)
decides where it can, **uncertainty resolves to replace-and-let-the-server-refuse,
never to drop**, and `ViewDraft::force_recreate` is the user's override. `diff_view` reads
both only where `view_replace_can_refuse` says a replace can be refused — PostgreSQL alone,
where it asked `dialect == Postgres` — and ignores the override elsewhere.
SQLite's is the pair of predicates above: every edit is a drop and a create, so nothing is
carried through a *replace* there — it is carried through the re-create, which is why
`ViewOptions::column_list` had to be modelled (see `core::schema`). **SQL Server's is
`CREATE OR ALTER VIEW`** (2016 SP1 on), which `create_view_sql` writes where a replace is
asked for there: like MySQL's it replaces anything and keeps the view's grants and triggers —
but T-SQL's `ALTER VIEW` resets whatever it is not told, so the header is restated in full, the
column list and `ViewOptions::attributes` (`SCHEMABINDING`, `VIEW_METADATA`) with it; a
schema-bound view altered without the word comes back unbound
(`sql_server_alters_a_view_in_place_restating_its_header`). **Restating it does not keep an
indexed view's indexes**, which this passage used to imply: `ALTER VIEW` drops every index on a
view whatever its header says, and the rename's `DROP VIEW` does too (measured on 2022 and 2025
— `sys.indexes` for the view went from two rows to none), so any edit to an indexed view left it
un-materialised, with nothing in the preview. `view_statements` now follows the `CREATE OR ALTER`,
or the rename's `CREATE VIEW`, with `view_index_statements` — `create_index_sql` for each of
`TsqlView::indexes` (under `schema.rs`), on the name the view now has, **the clustered one
first**: it is what materialises the view, and a nonclustered index on a view is refused until
it exists. That sort is load-bearing, because the listing arrives in name order — the fixture
names its nonclustered index `a_nix` so that name order is the wrong one. Both `ReplaceView`
arms of the risks carry `view_indexes_rebuilt`, naming the indexes and saying they are built
again in the same transaction, which on a large view takes as long as building them did; the
in-place arm had no risk at all before. An `IndexInfo::lossy` one (a kind the model does not
state — a columnstore one and one with included columns were too, and are modelled now; a view
carries no XML or spatial index, so those never reached this arm) cannot
be built again whole, so `lossy_view_index_refusal`, in
`unsupported()`, refuses the edit rather than bring the index back as less than it was
(`an_indexed_sql_server_views_edit_creates_its_indexes_again`). **That refusal covers only
what the model knows it missed**: `IndexInfo` has no fill factor, padding, lock options,
compression, filegroup, disabled state or description, and `lossy` is set for none of them, so
a `PAGE`-compressed or documented index came back without it while the sentence said it was
built again. `tsql_view_index_guard` now opens the plan — before the alter or the rename's drop
— with the table in-place guard's catalogue `THROW`, over the same predicate
(`tsql_index_carries`, shared with `tsql_in_place_guard`) for each index the plan drops and
builds again, and the sentence says such an index stops the plan. It takes that predicate's
internal-table term with it, harmlessly, but not `tsql_index_carries_late`'s arms, since an
indexed view can hold no columnstore or XML index — and it asks the view's clustered index its
compression where the table's guard does not (`heap_keeps_compression`), since a view's
clustered index leaves no heap to keep it. And `ALTER VIEW` drops the
view's hand-made statistics too, which nothing reads; the sentence names that loss as the
table rebuild's does (`an_indexed_views_edit_guards_what_its_indexes_cannot_restate`; live,
`an_indexed_views_edit_refuses_what_its_indexes_cannot_carry`, 2022 and 2025: the compressed
index's edit refused with nothing changed, the statistics gone after an edit that names them).
Copy DDL and the dump restate a view's indexes from the same model, as a table's script does,
and so without those options. **What is dropped is the
server's, what is built the draft's**: `ReplaceView::server_indexes` carries `diff_view`'s
`current` indexes, and the sentence names those, saying which the plan builds again and which
it does not; the refusal reads both sets. In the editor they are one reading; in a comparison
the draft is the source's view, and reading its indexes alone dropped a target's `cix` with an
empty risk list, a lossy one with no refusal
(`a_view_alter_names_the_indexes_the_server_holds`,
`an_alter_names_the_targets_indexes_it_drops`; live,
`a_comparisons_view_alter_names_the_targets_indexes`). Its only re-create is a rename
(above). **`Change::CreateView` takes the same two steps** — the settings wrapper and
`view_index_statements` — with the summary naming the indexes and the lossy refusal matching it
too: the comparison creates a view only the source holds from the source's reading, which
carries both, and its bare `CREATE VIEW` filed it under the applying session's settings and
unmaterialised, seen only on the next comparison
(`a_sql_server_view_created_from_a_reading_keeps_its_settings_and_indexes`; through the
comparison, `a_missing_view_is_created_with_its_settings_and_its_indexes`; live,
`a_comparison_creates_a_missing_view_with_its_settings_and_indexes`). A view new from the
editor has neither, and gets the bare statement. `create_view_sql`
asks per engine (`my`/`pg` locals) rather than `!pg`, which had been sorting SQLite onto
MySQL's side and would have emitted `ALGORITHM`/`DEFINER`/`SQL SECURITY` at an engine that
has none of them. The check option and the column list are capabilities of their own,
`supports_view_check_option` (MySQL, PostgreSQL) and `view_keeps_column_list` (SQLite, SQL
Server), asked by `create_view_sql` and `view_editor.rs` alike in place of an `== Sqlite` that
sorted SQL Server onto MySQL's side of both. SQL Server's `WITH CHECK OPTION` has neither
`CASCADED` nor `LOCAL` and stays in the body the user edits, where introspection leaves it.
`emit_sqlite` and `emit_mssql`
now call `view_statements` (and `emit_sqlite` `trigger_statements`) rather than keeping a
hand-rolled `DropView` arm, so there is still one view emitter; `tsql_supports` admits
`CreateView` and `ReplaceView`, which is what switched SQL Server's view editor on.
The MySQL `ALGORITHM` a replace would reset arrives *after* the editor opens
(`SchemaActions::view_algorithm` → `Db::view_algorithm`; see `schemaic-db`), and
`view_editor::fetch_algorithm` patches **both** sides of the diff with it — writing
only the draft would make every MySQL 8 view open already-changed against a `current`
that still said `None`. It leaves the draft alone if the user already picked one. Materialized
views are drop-only (no `CREATE OR REPLACE` exists for one). Same round-trip gate,
same rule about extending fixtures.
**PostgreSQL's standalone objects** — enums, domains, sequences — are `EnumDraft`/
`DomainDraft`/`SequenceDraft` → `diff_enum`/`diff_domain`/`diff_sequence` → the same
preview, with `ObjectKind` folding the three identical-but-for-a-keyword changes
(`RenameObject`/`DropObject`/`SetObjectComment`) into one arm each. The shape is dictated
by what PostgreSQL *can't* do. `enum_value_plan` is the heart of it: appending, inserting
and renaming a value are `ALTER TYPE`, but there is no `DROP VALUE` and no way to move
one, so the moment a value is **removed or reordered** the whole edit collapses into a
single `Change::RecreateEnum` — `recreate_type_sql`'s park-create-recast-drop dance, which
casts each dependent column through `text` — **and for an enum only, on to the rebuilt
type**, because `text` has no assignment cast to one. A *domain* stops at `::text`
deliberately: the second, explicit cast is what silently truncates (`varchar(64)` →
`varchar(16)` destroyed 48 characters per row and committed), where the assignment cast
PostgreSQL then applies refuses instead. `RecreateDomain` therefore carries the base type
it had *before* the edit, so `risks()` can run the same `column_risks` narrowing analysis a
column's type change gets — it was the one narrowing path in the emitter that disclosed
nothing. It also restates the default it had to drop first. A domain's
base type is the same story (`ALTER DOMAIN` has no action for it); everything else about
one alters in place. `type_dependents` reads the columns that dance has to touch off the
introspected schema, matched on the **type's** identity and scanned across *every*
namespace (a qualified declaration must match both halves; an unqualified one is the
default namespace's type wherever the table lives, which is what `format_type` means by
printing it bare). It is deliberately a **lower bound** — a view or function built on
the type can't be enumerated from `DbSchema` at all — which is why `recreate_risk` names
the columns *and* says the server may still refuse. Two disclosures exist because the
model can't distinguish the intent: a value list can't tell a rename from a
delete-plus-add, so the plan takes the reading that keeps the data and
`RenameEnumValue::risks` says every row will be relabelled; and `AddEnumValue` warns that
PostgreSQL can never take a value back. A sequence's `restart` is on the **draft, not the
model** — it's an action, not a state, so folding it in would make every re-opened editor
dirty against a sequence nothing had changed — and `sequence_edits`/
`sequence_alter_clauses` are kept in step so the sentence the user reads and the statement
that runs come from one comparison. All PostgreSQL-only, and `supports_change` now says so
with an exhaustive arm of its own: the thirteen enum, domain and sequence changes are `true` on
PostgreSQL and `false` on the other three, where MySQL used to inherit the blanket yes. Nothing
reachable moved — MySQL never fills those lists — but the Create menu asks it (below, after the
drops), and that yes is why the menu had spelled its gate as an engine comparison.
`object_statements` is still called from both `emit_mysql` and `emit_postgres`, and **filters
on `supports_change`** as `container_creates` does: once MySQL answered no,
`a_refused_change_is_in_no_script_the_set_produces` caught it writing `CREATE TYPE … AS ENUM`
into the script beneath an INCOMPLETE header that named that change as withheld. It used to be
left unfiltered on purpose, so a MySQL set holding one emitted SQL the server could reject
rather than dropping it on the floor; with the refusal stated, the header names the change as
left out, so filtering it no longer drops it silently.
**Triggers and stored routines** ride the same rails again:
`TriggerSetDraft`/`TriggerDraft` → `diff_triggers` and `RoutineDraft` → `diff_routine`
→ `Change::{CreateTrigger, ReplaceTrigger, DropTrigger, CreateRoutine, ReplaceRoutine,
RenameRoutine, DropRoutine}` → the same preview. MySQL, PostgreSQL and SQLite cannot *alter* a
trigger here, so there **every** edit is a drop-and-create and `ReplaceTrigger` is that pair —
SQL Server's same-name edit is the one altered in place, below — which is why
`trigger_statements` emits **all the drops, then all the creates** rather than each pair
together: adjacent pairs collide the moment two triggers swap names, and on MySQL
statement 1 has already committed when statement 2 fails, so the first trigger is simply
gone. Same rule, same reason as `GridWrite::plan` in `core::model`.
**It emits each trigger's own `create_sql` and *not* `TriggerInfo::create_set_sql`, and it
resolves the ordering clause against the plan before emitting it.** This passage used to say the
clause was safe here by coupling: a designer plan replaces one trigger inside a group that
already exists, which is precisely the caller a `PRECEDES` anchor is right for, and the only
caller that would replay a whole group into nothing was the comparison's migration plan, from
which `compare::CompareEntry::needs_source` withholds every MySQL trigger anyway. The premise was
false of the plan this emitter actually builds. Every trigger edit is a drop-and-create and all
the drops run first, so an edit *inside* a `(table, timing, event)` group recreates that group
from nothing — and the catalogue gives the group's **leader** `PRECEDES <successor>`, naming a
trigger the plan has not made yet. Both servers refuse it (measured: MariaDB `ERROR 4031`, MySQL
`ERROR 3011`), MySQL-family DDL has no transaction so the drops above have already committed, and
**both triggers are destroyed** — unrepairable in the app, the modal having no control for
`order` and every retry re-emitting the clause. Each create therefore goes through
`TriggerInfo::with_resolvable_order` with a **two-term** `exists`, the two terms being what "all
the drops, then all the creates" costs: *created earlier in this plan* — the successor's own
`FOLLOWS` then rebuilds the chain, which is what makes stripping the leader's clause sufficient
rather than lossy — **or** *survives this plan*, a name this plan neither creates nor drops being
one the server holds throughout, which is what keeps an edit inside an existing group in its
position instead of appending it last. A name this plan creates is deliberately **not**
surviving: it is gone until its own create runs. The residue is a draft whose `order` names a
trigger that neither exists nor is touched — the server refuses that, as it did before, and no
catalogue produces it.
`a_recreated_group_does_not_name_a_trigger_it_has_not_created_yet` pins both directions, so a
fix that stripped *every* clause fails it too. **Nothing on this path had ever emitted a
`TriggerOrder`**: the only fixtures in the workspace holding two triggers in one timing/event
group were `core::dump`'s own unit tests, written when `create_set_sql` was, so `order` was
`None` in every `ChangeSet` test and on every live leg. That is why the defect outlived a
release, and why the fix is a fixture as much as it is a predicate.
`session_wrapped_with` is the MySQL half of that emitter, shared by triggers
(`session_wrapped_create`) and routines (`session_wrapped`): neither `CREATE TRIGGER` nor
`CREATE PROCEDURE` has a clause for the `sql_mode`/`character_set_client`/
`collation_connection` the object was written under, yet all three are part of what it does,
so the values are set on the session around the statement and restored after (`run_ddl` runs
a MySQL plan in order on one connection, which is what makes that safe). **The routine half is
the later of the two and closes a hole of its own**: every MySQL edit to a routine is a `DROP`
that commits on its own followed by a `CREATE`, so an unwrapped recreate re-filed the routine
under whatever `sql_mode` the applying session happened to have — one written under
`sql_mode = ''` and recreated under a strict one starts raising on rows it used to truncate,
with the original already gone. One helper rather than two because it is the same failure, and
the wrap goes on **every** `CREATE` the routine emitter produces, not only a recreate's.
Nothing is emitted
when nothing is known — `None` means "not fetched", and inventing a session state is a change
nobody asked for. It early-returns on `!= MySql`; the old `== Postgres` test would have handed
SQLite `SET SESSION sql_mode = …`.
**SQL Server's half is `tsql_settings_wrapped`**, and `session_wrapped` is an exhaustive match
that hands a routine to it: T-SQL's creation-time settings are its own two, `ANSI_NULLS` and
`QUOTED_IDENTIFIER` (`TsqlModule`'s flags, under `schema.rs`, with the bug they fix), not
MySQL's three. When either is off it emits `SET … OFF;`, then `lead` (a recreate's `DROP`) and
the create, then `SET … ON;` — **each `SET` a statement of its own**, because `CREATE VIEW`/
`TRIGGER`/`PROCEDURE` must be alone in its batch and `QUOTED_IDENTIFIER` takes effect only when
the *next* batch is parsed; `run_ddl` runs each statement as a batch on one connection and a
script is cut at `GO`, so both keep the setting for the create and only it. Folding the `SET`s
into the create's statement would look tidier and break both. **It restores `ON`, not what the
session had** — ON is what Schemaic's session and SQL Server's own tools open with, and T-SQL
has no variable to save the old value in, so where MySQL's wrapper puts back a user variable
this one assumes. Every SQL Server module path goes through it: `trigger_create_statements`
(the edit plan and the table rebuild's trigger replay), `TriggerInfo::create_set_sql` (the
dump), `session_wrapped` (the routine plan), `routine_create_statements`
(`ObjectItem::create_sql`'s Copy DDL and dump — through `tsql_settings_around`, which wraps a
numbered group's head **and its other members** in the one pair: `sys.sql_modules` keeps one
row of settings for the group, and members scripted after the closing `SET … ON` were created
under `ON`, a member's `"member"` string turning into a column, Msg 207 —
`a_numbered_groups_members_are_scripted_inside_its_settings`; live,
`a_numbered_groups_script_keeps_its_settings_for_its_members`), both `ReplaceView` arms of
`view_statements`, and `TableInfo::tsql_create_ddl`'s view branch (Copy DDL and the dump, the
`SET`s as `GO` batches before the indexes). `alters_the_database` already counted a `SET` as
scaffolding. **A plan adds nothing for a module at the ANSI defaults; a script states both
settings whichever way they are** (`tsql_settings_around`'s `script`, through
`tsql_settings_scripted` for a view's and a trigger's, and `routine_create_statements`), as SQL
Server's own scripter does. A plan runs on Schemaic's own ANSI-defaults session; a script's
reader may be `sqlcmd`, which opens with `QUOTED_IDENTIFIER` OFF unless given `-I` — and when
only an OFF setting was stated, a dump or Copy DDL restored through it re-filed every module at
the defaults as OFF: its `"id"` became the string `'id'`, an indexed view's index was refused
(Msg 1935), and which modules came out wrong depended on where an OFF module's closing `SET …
ON` fell in the file (measured with `sqlcmd` 18 on 2022 and 2025). A module the server shows
no text for is a comment, and gets no `SET`s. The dump's head states both `ON` too
(`dump::ansi_settings_sql`, beside `SET DATEFORMAT`), for the tables before any module — a
filtered index or a computed column's index is refused under `QUOTED_IDENTIFIER` OFF.
`a_view_is_edited_under_the_settings_it_was_created_with` and its trigger and routine siblings
pin it, with `a_sql_server_modules_script_states_both_settings_at_the_defaults`; live,
`a_module_keeps_its_creation_settings_through_an_edit` (2022 and 2025) reads each module with
its settings, edits it and finds them still in `sys.sql_modules` and the module behaving as
before, and replays a view's script to the same end, and
`a_modules_script_restores_its_settings_through_a_session_that_has_them_off` replays a view's,
an indexed view's and a procedure's script after `SET QUOTED_IDENTIFIER OFF` and `SET
ANSI_NULLS OFF` and finds each module back at `11`.
**A routine's diff is per-engine at the *rename*, and that is the whole shape of
`diff_routine`.** PostgreSQL replaces one in place (`supports_or_replace_routine`) and renames
it with a statement of its own (`supports_routine_rename`), so a rename is a separate change
ordered *after* the redefinition — which has to address the signature the server still holds.
MySQL has neither verb: every edit there is a `DROP … IF EXISTS` plus a `CREATE`, so a rename
is folded into that recreate (the drop names the old routine, the create the new one) and a
bare rename reads as a redefinition, exactly as `diff_view` resolves one on SQLite. SQL Server
has the first verb and not the second, which is why a rename on an engine with no rename verb
now forces the recreate *even where the replace exists* — altering in place under the new name
creates a second routine and leaves the first standing (below, after the triggers). Which
route a change took rides on `Change::ReplaceRoutine { recreate }` rather than being re-derived
at emit time, so the preview's risk sentence and the SQL cannot disagree — and `recreate` is
what earns the "a definition the server rejects leaves no procedure at all" warning, the same
sentence `ReplaceTrigger` carries and for the same reason. `RoutineInfo::signature_sql` is what
knows the two engines address a routine differently: PostgreSQL by its **argument types**
(overloads share a name), MySQL by name alone (the parameter list there is a syntax error on a
`DROP`). The list it writes is `RoutineInfo::identity_arguments` —
`pg_get_function_identity_arguments`, fetched alongside `pg_get_function_arguments` and empty on
MySQL, whose routine identity is the bare name. A second field rather than a reformatting,
because the two strings genuinely differ and each is a syntax error where the other belongs:
`CREATE` needs the defaults (`b boolean DEFAULT false`), while `DROP`/`ALTER … RENAME` take
`[argmode] [argname] argtype` and answer `syntax error at or near "DEFAULT"` to anything more —
so while the `CREATE` form was spliced in here, a routine with a defaulted parameter could not be
dropped or renamed from the app at all. It falls back to `arguments` for a routine assembled by
hand rather than read from a catalogue, where the two are the same string.
`Change::ReplaceRoutine` therefore carries the routine **as the server holds it**
(`server: Box<RoutineInfo>`) beside the draft: a recreate's `DROP` must name that one and not
`draft.info`, which differs in the *name* on MySQL (where a rename rides the recreate) and in the
*parameter list* on PostgreSQL (where an edited list is a different function, so the `DROP`
dropped nothing and the `CREATE` left the original standing beside a new overload). It is what
`summary` names as the thing being changed, and what `risks` compares against to warn that
callers passing the old arguments no longer resolve.
That is also why `ObjectKind::Function`/`Procedure` are on the browse enum but refused
by `drop_object` — the refusal is the capability `ObjectKind::uses_shared_changes`, which is
what the three shared-arm constructors ask. It returns an **empty** change set for one, so a
preview says "no changes" rather than a bare-name statement that is right up until
the first overload; `drop_routine` takes the whole `RoutineInfo` and is the route.
`drop_item` is that routing written once — a routine to `drop_routine`, an event to
`drop_event`, anything else to `drop_object` — and it was lifted out of the tree's menu handler
so the row's **Drop** entry could be gated on the same set it opens
(`overlays::object_drop_offered`) rather than on a second copy of the match.
**SQL Server's sequences, synonyms, alias types and XML schema collections take the shared arm
for the drop, and for a rename `sp_rename` can find.** A T-SQL sequence is
`ObjectKind::Sequence`, the kind PostgreSQL's browses as; the other three are kinds of their
own — `AliasType` is not `Domain`, which is PostgreSQL's and carries checks and a default and
alters in place, where an alias type does neither. All four answer yes to
`uses_shared_changes`, and `tsql_supports` admits `DropObject` for exactly those four,
`RenameObject` for three of them — not an XML schema collection, which `sp_rename` cannot find
to rename (Msg 15225, measured) — and never `SetObjectComment`, T-SQL having no `COMMENT ON`.
`emit_mssql`'s whole-objects loop writes
`DROP {sql_keyword} {q};` — `DROP TYPE` for an alias type,
`DROP XML SCHEMA COLLECTION [sales].[invoice];` for a collection
(`each_object_drops_with_its_own_statement`,
`only_the_drop_and_a_findable_rename_of_the_shared_changes_are_admitted`;
live, `standalone_objects_are_browsed_and_dropped` on 2022 and 2025, through `drop_item` and
`run_ddl`). **The drop's risk sentence is per kind**, and true of both engines that have these
objects, measured on PostgreSQL 16 and SQL Server 2022. A sequence a default still draws from
is refused (SQL Server's Msg 3729; PostgreSQL's *cannot drop sequence … because other objects
depend on it*), so its sentence says those defaults have to go first — it said a `nextval`
default "stops working", a drop PostgreSQL never performs
(`dropping_a_sequence_says_the_server_refuses_while_a_default_uses_it`). A type or a collection a column
still uses is refused by the server (Msg 3732 for an alias type, Msg 6328 for a collection),
and the sentence used to say *PostgreSQL* refuses, which was false the moment it covered SQL
Server. A synonym is refused by nothing: a view through a dropped one fails only when it next
runs (Msg 208/4413), so its sentence says that rather than promising a refusal that never comes
(`a_drop_says_what_sql_server_does_with_what_uses_it`).
**Their editor's draft is `TsqlObjectDraft`** — `original`, the `TsqlObject` the dump reads and
writes, and a `restart` — its own type rather than `SequenceDraft`, whose counters are `i64`
and whose emitters are PostgreSQL's, where a T-SQL sequence may be `decimal(38,0)` and its
bounds are restated as the catalogue printed them. So the numbers stay **text**: `validate`
asks each to be a whole number of any length and compares them as `i128` where they parse —
the increment not 0, the minimum below the maximum, the start between them, a cache size above
0, a restart a whole number between the bounds (the server's Msg 11703 otherwise, measured on
2022) — and leaves to the server what only it can judge, a type name or
a target that exists. **An existing sequence's start is judged by its restart**: `RESTART WITH`
rewrites `start_value`, so a minimum raised to the restart is an edit the server takes
(`ALTER SEQUENCE … RESTART WITH 100 MINVALUE 100` on 2022), and with a restart given the
catalogue start is not checked against the bounds at all. Without one the refusal names the
control the form shows — *It started at {start}, outside the new range: Restart at a value
between the minimum and the maximum* — where it used to ask for a *Start* the form hides on an existing
sequence; a new one keeps *The start must lie between the minimum and the maximum*
(`an_existing_sequences_start_is_judged_by_its_restart`). A synonym needs one to four target
parts with the object named, an alias type a base. **A collection's text has no rule**: the
server is its judge and takes `AS N''`, which is what the blank form's component-less
`<xsd:schema>` reads back as, so the old *needs at least one schema* refusal made the
collection the editor had just created uneditable, rename included
(`an_empty_xml_schema_collection_can_be_edited`). `blank` states every clause of a new sequence —
`bigint` from 1 by 1 up to `i64::MAX`, `NO CYCLE`, `CACHE` at the server's size — because
T-SQL's own default starts an ascending sequence at its type's minimum; an alias type starts as
a nullable `nvarchar(50)`, a collection as one empty `<xsd:schema>`, a synonym with no target.
`restart` is a request and not a field, as on PostgreSQL's draft: the counter's position is not
part of the definition. `create_tsql_object` is `Change::CreateTsqlObject`, written as
`TsqlObject::create_sql`, so a created object and a dumped one come from one builder.
**`diff_tsql_object` alters what T-SQL alters and replaces the rest.**
`Change::AlterTsqlSequence { from, to, restart }` is a sequence's increment, bounds, cycle and
cache as one `ALTER SEQUENCE` restating only the clauses that differ (`tsql_sequence_clauses`,
which the summary reads too), with `RESTART WITH` first, T-SQL's own order. A rename is the
shared `RenameObject`, written through `sp_rename` in `emit_mssql`'s rename loop beside
`RenameTable` — after the `ALTER`, which names the sequence as it was — with `N'USERDATATYPE'`
for an alias type, which is not an object to `sp_rename` until it is told so.
`Change::ReplaceTsqlObject { from, to }` — a plain `DROP` of `from`, then `to.create_sql()` —
is everything else: a sequence's type or start, a synonym's target, an alias type's base or
nullability, and any change at all to a collection, its name included. **A new start is a
replace, not a restart**: `ALTER SEQUENCE` has no `START WITH`, and `RESTART WITH` moves the
counter as well, which is why the form offers *Start* on a new sequence alone. **A restart
asked for is where the re-created sequence starts** — `to` takes it as its `START WITH`.
Before S4.1-L1-01 the typed value appeared nowhere in the plan and the new sequence began at
the old `START`, so its next value was one the tables already held
(`a_retyped_sequence_starts_where_it_was_asked_to_restart`). The drop is
plain rather than the dump's `IF EXISTS` because the object came off the catalogue, and the
server's refusal over what uses it is the plan's answer. Measured on 2022: `sp_rename` renames
a sequence keeping its position, a synonym, and an alias type given `USERDATATYPE`, and all of
it works inside a transaction. **What stands on the object rather than in it goes with the
drop**, and `create_sql` restates the definition alone: a GRANT, a DENY — a DENY dropped is a
DENY lifted — an owner set by `ALTER AUTHORIZATION`, extended properties, and an alias type's
`sp_bindrule`/`sp_bindefault`, none of which the model reads. Before R3-L5-01 a replace took
four permission rows to none and `rule_object_id` to 0, measured on 2022, while the plan said
nothing. So the arm opens with `tsql_replace_guard`, a
`DECLARE @id … CASE … THROW 50000` batch of the table rebuild's shape, which refuses over each
— refuse rather than restore, as the rebuild does over a table's permissions and owner, because
restoring grants would need a model of them this one does not have. Each kind asks its own
catalogue class — 1 for a sequence or synonym through `OBJECT_ID`, 6 for an alias type through
`TYPE_ID`, 10 for a collection through `sys.xml_schema_collections` — and a class asked wrong
fails open without a sound, as the rebuild guard's arm about what stands on the table did
(above). A comparison's replace is the same `Change`, so its Apply is refused with the same
reason. **The risk sentences follow the act**: an `AlterTsqlSequence`
carrying a restart gets `RestartSequence`'s sentence; a sequence's replace says the position it
had reached is lost and names the start it begins over at — `to`'s, so a restart asked for
included — and that the server refuses the drop while a default draws from it; an alias
type's or a collection's, that it refuses while a column uses one; a synonym's, that nothing refuses its drop — and every replace a second sentence naming what the
guard refuses over. A synonym's replace used to carry none, so its Apply wore `Primary` over a
lifted DENY; it wears `Danger` now (`an_unedited_object_is_no_change`,
`a_new_object_is_created_with_the_dumps_statement`,
`a_sequence_is_altered_in_place_and_retyped_by_replacing_it`,
`each_sequence_clause_is_altered_as_it_changed` — MINVALUE/MAXVALUE, CYCLE/NO CYCLE, NO CACHE,
CACHE n and a bare CACHE —
`what_t_sql_cannot_alter_is_replaced_and_a_name_is_renamed`,
`a_draft_is_refused_for_what_the_form_can_know`,
`a_replace_refuses_what_its_drop_would_take_and_says_so`; live,
`standalone_objects_are_created_and_edited_as_drafted` on 2022 and 2025 — a sequence, alias
type and collection created from their blank forms, each read back equal to the draft itself
and its re-read draft valid, the one normalisation the blank collection's definition reading
back empty (it used to be compared with a draft built from the read-back, true by
construction), a synonym
created with a target, a sequence moved to increment 5, restarted at 100 and renamed with
`NEXT VALUE FOR` then answering 100 and 105, a synonym re-pointed and renamed, an alias type
renamed through `sp_rename` and a collection renamed by replacement —
`a_replace_refuses_over_permissions_and_a_bound_rule`, every arm on every kind — a GRANT or
DENY, an owner of its own, an extended property, and a bound rule and a bound default on an
alias type — each replace refused with every permission surviving and its unguarded twin
replaced, on 2022 — and `a_sequence_restart_is_honoured_and_not_compared` on 2022: retyped to
`int` with a restart of 500 it answers 500, its minimum raised to 600 is refused without a
restart and applies with one at 600, and a sequence restarted on one side compares the same).
`ObjectDraft::Tsql` wires
it into the editor: `from_item` answers it for `ObjectItem::Tsql`, and `kind`, `name`,
`problems` (and so `validate`, its English) and `change_set` route to it. **`ObjectDraft::blank` takes the dialect now, because
a sequence's draft depends on it** — PostgreSQL's `SequenceDraft` on PostgreSQL, MySQL and
SQLite, a `TsqlObjectDraft` on SQL Server. The form and the plan follow the draft, so the wrong
one would offer PostgreSQL's clauses and emit its grammar. A synonym, alias type or collection
gets a `TsqlObjectDraft` whatever the dialect; the capability below is what keeps one from being
offered where its plan would be refused. The comparison asks this same differ of two readings
of one object, so a schema compare and the editor cannot disagree about what a change to one is
(`compare::tsql_entry`, under `compare.rs`).
**`supports_object_creation(dialect, kind)` is what an object folder's Create entry and the
database node's Create ▸ ask**, computed from the plan the blank form raises —
`ObjectDraft::blank(kind, …, dialect)` through `change_set(None, …)`, every change in it asked
of `supports_change` — with routines and events deferring to
`supports_routine_editing`/`supports_event_editing`. It began because SQL Server's Sequences
folder would have opened PostgreSQL's sequence form, whose `CREATE SEQUENCE` `tsql_supports`
refuses. It answers Sequence, Synonym, Alias type and XML schema collection on SQL Server and
not Type or Domain; Type, Domain and Sequence on PostgreSQL; and nothing on MySQL or SQLite
(`a_folder_offers_create_where_the_engine_takes_the_form`; for the menu,
`create_menu_tests::mysql_offers_only_what_it_has` and `sql_server_is_offered_what_it_emits`).
**It used to be sound only behind a folder that exists**: MySQL inherited `supports_change`'s
blanket yes for PostgreSQL's three creates, so the predicate said yes to a MySQL sequence, and
`overlays::create_children` kept a `dialect == Postgres` for Type, Domain and Sequence rather
than ask it. With that family PostgreSQL's alone (above), the menu asks the predicate of all six
kinds and the literal is gone — `engine_comparison_gate`'s `overlays.rs` budget 2 → 1 — where
SQL Server's four would otherwise have wanted a second literal beside it. SQL Server's menu is
now Table, View, Sequence, Synonym, Alias type, XML schema collection, Function, Procedure,
Database, Schema; PostgreSQL's and MySQL's are unchanged.
SQLite has no stored routines at all — a function there is registered by the host program,
not stored in the database — so its arms are absent from `supports_change`,
`supports_routine_editing` is false, and the tree grows no folder and the Create menu no entry.
**`ObjectKind::Extension` is PostgreSQL's extension**, the last of `ObjectKind::ALL` (label
*extension*, folder *Extensions*, keyword `EXTENSION`), and the first kind that belongs to the
**database** rather than a namespace — `ObjectKind::is_database_wide`, an exhaustive `match` with
it alone on `true`, is what the tree's folder placement and `DbSchema::has_objects_in` ask
(under `schema.rs`). Its create is `Change::CreateExtension { name }`, built by
`create_extension(name, dialect)` against the name and no namespace and emitted by
`object_statements` as `CREATE EXTENSION "name";` — no `SCHEMA`, `VERSION` or `CASCADE`: the
server installs its default version into the first schema on the path, and an extension that
needs another installed first is refused by name, which is the plan's answer. Its drop is the
shared `DropObject` through `drop_item` (`uses_shared_changes` is true for it), `DROP EXTENSION
"name";`, never `CASCADE` for `DROP SCHEMA`'s reason; the risk sentence says the server refuses
while anything still depends on it — a column of its type, an index, a server on its wrapper
— and names what (measured on 16: `file_fdw` with a server on it is refused, naming the server
and the foreign table behind it). **`supports_change` asks two new early arms, both ahead of
the blanket "everything but SQLite"**, which would otherwise have admitted MySQL: a
`RenameObject` of an extension is refused on every engine, `ALTER EXTENSION` naming nothing new;
and `CreateExtension`, `DropForeignTable` and the extension's `DropObject`/`SetObjectComment`
are PostgreSQL's alone through an exhaustive dialect `match` — objects the other three cannot
hold, not arms they lack. `supports_extensions(dialect)` is the capability every menu asks,
computed from `supports_change` over the create and the drop, as `supports_namespace_editing`
is, so the entry and the plan cannot disagree (`extensions_are_postgresqls_alone`, which also
asserts the refused sets emit nothing). **`supports_object_creation(_, Extension)` is false**,
because there is no `ObjectDraft` for one (`from_item`/`blank` answer `None`): the folder's
Create and the Create ▸ entry open the picker in `database_editor.rs`, gated on
`supports_extensions` instead (`an_extension_has_no_rename_and_no_object_form`,
`an_extension_is_created_by_name_alone`, `an_extension_is_dropped_by_name_without_cascade`;
live, `pg_catalog::an_extension_is_created_and_dropped_through_its_plans` installs `pg_trgm`
through the emitted plans via `Db::run_ddl` and reads it back, the picker's list before and
after, and has `citext`'s drop refused with *"depend"* while a column is of its type).
**`Change::DropForeignTable` is a foreign table's drop**, `DROP FOREIGN TABLE <qname>;` from
`emit_postgres`, since PostgreSQL refuses `DROP TABLE` on one (`TableShape::ForeignTable`, under
`schema.rs`). Destructive all the same — the definition, its server and options go and nothing
here can restate them — though its risk sentence says the rows stay where the wrapper reads
them (`a_foreign_table_is_dropped_as_one`; live,
`pg_catalog::a_foreign_table_is_read_and_dropped_as_one`, over a `postgres_fdw` server, has
`DROP TABLE` refused and this land).

**MySQL's scheduled events** — `EventDraft` → `diff_event` → `event_alter_clauses` → the same
preview — are the fourth browsable object kind, and the one whose shape is dictated by what
its engine *can* do rather than what it can't: `ALTER EVENT` reaches the schedule, the status,
the comment, the definer, the name **and** the body, so every edit here is one statement
restating only what changed and there is no drop-and-create anywhere. A rename rides inside
that same `ALTER` as its `RENAME TO` clause rather than being a `Change` of its own — split
into two, the pair would need an order (rename first and the second statement must address the
new name; alter first and a failed rename leaves a half-applied edit) for no gain, since MySQL
gives DDL no transaction either way. The header names the event **the server holds** and
`RENAME TO` the one the draft wants, which is why `Change::AlterEvent` carries `from` as a
whole `EventInfo`.
Three details are load-bearing. The clause order is **MySQL's grammar**, not a preference —
`ON SCHEDULE`, `ON COMPLETION`, `RENAME TO`, the status, `COMMENT`, `DO`, and any other
arrangement is a syntax error. `diff_event`'s emptiness test is `event_alter_clauses` rather
than `draft.info == *current`, because the two sides carry session state and a `time_zone` no
clause restates, and comparing the whole struct emitted `ALTER EVENT e` with no clauses — itself
a syntax error — every time the lazy `SHOW CREATE` corrected one side and not the other. And
the **definer is the one edit that lives entirely in the header**: MySQL needs at least one
alteration clause after the name, so a definer-only change carries a restatement of
`ON COMPLETION` with it, chosen over the status because it is provably a no-op *and* because a
stray `ENABLE` in the preview reads as an edit somebody made.
The one-shot warning (`Change::risks`) compares **both sides** on the alter arm: an event that
was already `AT … NOT PRESERVE` is not made self-deleting by a comment change, and warning
about it would make every such edit `is_destructive` and put it behind the preview's
destructive gate. It fires where the plan *introduces* the combination — the way every other
comparison in this module works.
A `CREATE`/`ALTER` that restates the body takes **no trailing semicolon** — the body is a
compound whose own statements end in `;` — and a `DROP` does; `event_restates_body` is asked
once and answers both the `DO` clause and the terminator. `supports_event_editing` is the
capability every surface asks (`supports_change` answers the three event arms with an
exhaustive dialect match ahead of its blanket "everything but SQLite"), and
`uses_shared_changes` is false for `ObjectKind::Event` for its own reason: the comment is a
clause of `ALTER EVENT`, so `SetObjectComment` would emit a `COMMENT ON EVENT` no engine has.
**SQLite got here through the reader, not the emitter.** It publishes no catalogue of a
trigger's parts — no `information_schema`, no pragma, only the statement text — so
`sqlite_trigger_info(create_sql)` is what made an editor possible at all, and until it existed
the list was empty and an editor over it would show a table's triggers as gone and offer to
"add" one that is already there. **Structure from the AST, body from the text**: the
per-dialect sqlparser AST (`intel`'s `SqlDialect::parser()`) answers the timing, event,
`UPDATE OF` columns and `WHEN` guard, which a scanner gets wrong on a body containing the same
words, while `sqlite_trigger_body` takes the `BEGIN … END` block verbatim over `skip_noncode` —
re-printing it from the AST would normalise away the user's comments, casing and line breaks,
which is a phantom change on every open and a rewritten trigger on every apply. A statement it
can't read yields `None` and the caller drops it, safe in the one way that matters because
`diff_triggers` only drops what the *server copy* lists. `TriggerDraft::validate`'s SQLite arm
is the engine's own grammar, every rule measured against 3.45 rather than inferred: one event
per trigger, no `TRUNCATE`, `INSTEAD OF` only on a view *and* a view taking only `INSTEAD OF`
(`cannot create INSTEAD OF trigger on table` / `cannot create BEFORE trigger on view`), row
level only (`FOR EACH STATEMENT` is a syntax error), and a body that must be a `BEGIN … END`
block with at least one statement in it — a bare statement and an empty block are both syntax
errors. `is_begin_end_block` asks that last one through the shared lexer rather than with a
`starts_with`, since a body may open with a comment and a `BEGIN` inside a string is not the
block's own. It is refused in the modal rather than at Apply because the `DROP` has already run
by the time the `CREATE` fails.
**SQL Server got here through a reader too, and that reader is a stated exception to the `intel`
invariant.** Its catalogue keeps a trigger's timing, events and state, but the header options a
`CREATE OR ALTER` must restate — `EXECUTE AS`, `SCHEMABINDING`, `NATIVE_COMPILATION`, `NOT FOR
REPLICATION` — live only in the stored text, so `tsql_trigger_parts` reads them out of it into
`TsqlTriggerParts`. It walks tokens (`TsqlCursor`) over `sql::skip_noncode` rather than asking
sqlparser, because 0.62's T-SQL `CREATE TRIGGER` knows neither the `WITH` options nor `NOT FOR
REPLICATION` and parses the body as statements, which a real trigger's body often is not to it —
so the AST answers `None` for exactly the triggers this exists to read. The walk is the grammar's
header and nothing more, and the thing it has to get right is **the header's own `AS`**, past
`EXECUTE AS`'s and any a bracketed name or a comment holds
(`the_body_starts_at_the_headers_own_as`). It answers `None` — the trigger then listed, droppable
and restated verbatim, never rebuilt — for `WITH ENCRYPTION`, the obsolete `WITH APPEND`, an
option it does not model, a DDL trigger (`ON DATABASE`, `ON ALL SERVER`) and a non-DML event
(`a_header_the_parts_cannot_restate_is_unreadable`), since a rebuild from the parts that *were*
read would silently drop the rest. Widening it to an option means modelling that option on
`TsqlTrigger` and restating it in `tsql_statement` in the same change; admitting a word the walk
merely skips is the silent drop this refuses.
**A same-name edit there is altered in place, not dropped and created.**
`supports_trigger_alter_in_place` answers yes for SQL Server alone (PostgreSQL's `CREATE OR
REPLACE TRIGGER` stays unused for the reason `trigger_statements` gives), and `trigger_statements`
then emits a `CREATE OR ALTER` with no `DROP`; the draft neither leaves nor re-enters the set,
which is what keeps the ordering walk exact for it. A rename stays a drop and a create on every
engine — `sp_rename` would leave the stored text naming the old trigger, the call
`supports_view_rename` makes for a view — and a `CREATE OR ALTER` cannot move a trigger to another
table either (Msg 2110, measured on SQL Server 2022), which the modal, holding one table's set,
never asks of it. **"Same name" is the same bytes**, not an ASCII case-fold: whether `tr` and
`TR` are one trigger is the database collation's call, and the case-folding test took a rename by
case alone for an in-place alter — so a case-sensitive database got a second trigger and every
insert fired both (measured on SQL Server 2022: one insert, two audit rows), and a
case-insensitive one kept the old name. As a drop and a create it is right under either; the
test is `trigger_alters_in_place`, which the preview's risk sentence asks too
(`a_case_only_trigger_rename_drops_and_creates`; live, `a_trigger_renamed_by_case_is_renamed_once`
under both collations, 2022 and 2025). `trigger_create_statements` is the one exhaustive `match` both routes go
through: MySQL's session wrap on one arm, and on SQL Server two lists — the statement and its
`DISABLE TRIGGER`, then its `sp_settriggerorder` ranks (`TriggerInfo::tsql_rank_statements`,
under `schema.rs`), which `trigger_statements` holds back until **every** create and alter in the
plan has run. **The ranks go last because only an alter lets a trigger's rank go**: an event on a
table holds one `First` and one `Last` (Msg 15130), so while each rank followed its own alter, a
plan moving `First` from one trigger to another, or swapping `First` and `Last` between two,
failed at the first rank — the alter that would have released it had not run yet
(`a_rank_moves_between_triggers_in_one_plan`; the swap is live in
`the_trigger_controls_rank_and_execute_as_on_the_server`). A disabled state has no such clash
and stays straight after its own trigger. `emit_mssql` routes
`trigger_statements` beside `view_statements`, and `tsql_supports` admits the three trigger
changes, which is all it took for `supports_trigger_editing(MsSql)` to answer yes
(`sql_server_offers_the_trigger_editor`, `an_edited_sql_server_trigger_is_altered_in_place`,
`a_renamed_sql_server_trigger_is_dropped_and_created`).
**`TriggerDraft::problems` is one exhaustive `match dialect` now** (`validate` its English
wrapper — the editors' refusals, under `i18n.rs`), because of what the old shape
would have done: it was an `if pg … else if sqlite … else` chain, and the `else` validated a SQL
Server draft as MySQL's — refusing a second event and every `INSTEAD OF` — the moment the editor
opened there. The SQL Server arm is T-SQL's grammar: no `BEFORE`, a view takes only `INSTEAD OF`
while a table takes it as well as `AFTER`, statement level only, no `TRUNCATE`, no `WHEN`, no
`UPDATE OF`, and a body rather than a function (`a_sql_server_trigger_draft_is_held_to_its_grammar`).
It refuses what `sp_settriggerorder` would, too, where the form can say so and the server's
message cannot: a rank on an `INSTEAD OF` trigger (Msg 15133) and a rank on an event the trigger
does not fire on (Msg 15125); two triggers holding the same rank on one event (Msg 15130) is
`TriggerSetDraft::validate`'s, since that clash is between triggers
(`a_rank_the_server_refuses_is_refused`).
What the form offers is computed from the same facts — `trigger_timings(dialect, host)`,
`trigger_events`, `trigger_fires_on_several_events`, `supports_trigger_not_for_replication`,
`supports_trigger_execute_as`, `supports_trigger_firing_rank` (the last three SQL Server's alone) — and
`the_offered_timings_and_events_are_ones_the_validator_accepts` holds every offered timing and
event to a clean validation on every engine and host, since an option offered and then refused is
the form opening on an error. The rest of the form's controls ask predicates of their own rather
than a dialect: `trigger_runs_function` and `supports_trigger_level_choice` (PostgreSQL's alone),
`supports_trigger_when` and `supports_trigger_update_of` (PostgreSQL's and SQLite's — neither is
"not MySQL", since SQL Server has neither clause), pinned as a truth table in
`the_trigger_forms_controls_are_offered_where_the_clause_exists`; and `supports_view_triggers`,
false on MySQL alone (`ERROR 1347: … is not BASE TABLE`), for whether a view takes one at all.
`view_triggers_are_offered_where_a_view_trigger_validates` holds that last one to the validator,
which is why `trigger_timings(MySql, View)` is now empty and the MySQL arm refuses a trigger on a
view — it accepted one before, so the predicate and the validator disagreed with nothing to say
so. PostgreSQL's arm took SQLite's rule refusing `UPDATE OF` without an `UPDATE` event when its
form gained the field: the clause is written only under an `UPDATE` event, so a column list on
any other would be dropped rather than refused
(`update_of_without_an_update_event_is_refused_on_postgres`). `TriggerSetDraft::validate` refuses a *change* to a trigger
`TriggerInfo::is_editable` says no to, on the constraint trigger's terms: it may sit in the set
while a neighbour is edited, and dropping it is fine
(`an_unreadable_sql_server_trigger_can_be_dropped_but_not_changed`).
**SQL Server's routines came the triggers' way, reader first.** `tsql_routine_parts` reads a
stored procedure or function into `TsqlRoutineParts` — kind, parameter list, `RETURNS`, the
`WITH` options, a procedure's `FOR REPLICATION`, the body — by the same `TsqlCursor` walk and
under the same stated exception, since sqlparser's T-SQL routine grammar does not carry the
`WITH` options and a body is exactly the text an AST would re-print rather than keep. The
cursor now records each token's `start`, which the three helpers the routine header needed
stand on: `close_paren`, `span_until` (the text up to a keyword at depth 0) and
`tsql_strip_outer_parens`. The header is `CREATE [OR ALTER] {PROC | PROCEDURE} name [params]
[WITH …] [FOR REPLICATION] AS body` or `CREATE [OR ALTER] FUNCTION name (params) RETURNS
{type | TABLE | @t TABLE (…)} [WITH …] [AS] body` — **a function's `AS` is optional**, so its
body may start at `BEGIN` or `RETURN` instead. **The parameter list is kept as text, defaults
and all**, because the catalogue does not keep a T-SQL default:
`sys.parameters.has_default_value` is 0 for `@a int = 5` (measured on SQL Server 2022), so a
list rebuilt from `sys.parameters` and fed back to `CREATE OR ALTER` would silently drop every
default — the stored text is the only place one lives. A procedure's optional parentheses are
taken off and written back without — so a comment between the name and the `(` belongs to
neither part, and `tsql_strip_outer_parens` says where the `(` stands so the walk files it
among the header's comments; before, every rebuild dropped it
(`a_comment_before_a_procedures_parentheses_survives_its_rebuild`). It answers `None` — listed, droppable, restated verbatim,
never rebuilt — for `WITH ENCRYPTION`, an option it does not model, a numbered group's member
(`p;2` — the group's head reads, and is handled below), a CLR routine's `EXTERNAL NAME` and
anything else outside that shape
(`a_routine_header_the_parts_cannot_restate_is_unreadable`), on the trigger walk's rule that a
word merely skipped is a word the rebuild drops (`a_stored_procedure_splits_into_its_parts`,
`each_function_shape_splits_into_its_parts`).
**The cursor reads a name the way T-SQL spells one, and a parameter's own `AS` is not the
header's.** It used to end a word where `sql::is_word_byte` did, so `@as` was a `@` and the
keyword `AS`, and `tr$as` was `tr`, `$` and `as`. It now keeps a `@`/`@@`/`#`/`##` prefix on the
word (`sql::t_sql_name_prefix`) and runs on through `$`, `#` and `@` (`sql::continues_name`),
so `RETURNS @t TABLE` is matched as a word starting with `@` rather than a `@` punct. And
`span_until` never stops at an `AS` straight after a `@name` word, because `@a AS int` is how
T-SQL lets a procedure declare a parameter. Before, `CREATE PROCEDURE p @a AS int = 5 AS
SELECT @a` read the list as `@a` and the body as `int = 5 AS SELECT @a` — header text in the
body box, the `WITH` options behind it hidden — and `@as int = 1` read the list as `@` and
rebuilt into a statement the server refuses (Msg 137, measured on SQL Server 2022). `@with` and
`@for`, which used to answer `None`, now read as the parameters they are
(`a_parameters_own_as_is_not_the_headers`, `a_trigger_name_holding_a_dollar_is_one_name`; live,
`a_parameter_declared_with_as_reads_and_rebuilds_whole` on 2022 and 2025).
**A view's header is the third walk over the cursor**: `tsql_view_parts` reads `CREATE [OR
ALTER] VIEW name [(columns)] [WITH attribute, …] AS body` into `TsqlViewParts` — the column
list verbatim, the attributes (`SCHEMABINDING`, `VIEW_METADATA`, the two it models) and the
body after the header's own `AS`. It was a pair of byte scans in `db::mssql`,
`view_select_body` and `view_header_options`, that ended a word where `sql::is_word_byte` did
— the cursor's old bug — so `v$as` read as `v` and the header's `AS`: the editor opened on
`AS SELECT …`, Apply emitted `AS AS`, and a `WITH SCHEMABINDING` after the false `AS` was
dropped (`a_view_name_holding_a_dollar_is_one_name`). And where they could not read a header
they guessed — the whole text as the body, an unknown attribute dropped while the known ones
beside it were kept. It answers `None` now, on the trigger walk's rule, for `WITH ENCRYPTION`,
an attribute it does not model and any other shape
(`a_view_header_the_parts_cannot_restate_is_unreadable`); `db::mssql` marks such a view
`TsqlView::verbatim` (under `schema.rs`). **`view_is_editable` is the one gate over a view's
refusals** — a view, not materialized, and `view_not_editable_reason` silent, which it is
unless the view is `verbatim` or `hidden`, when it answers `ViewProblem::Hidden`/`Verbatim` —
and `ViewDraft::problems` pushes the same value, for a caller that did not ask the gate. The encrypted case is the one that shipped: its
definition is NULL, so **Edit view** opened on an empty body, and a body typed there was
applied as a plain `CREATE OR ALTER VIEW`, dropping the encryption, the binding and every
index without a word (`a_sql_server_view_it_cannot_read_is_not_edited`; live,
`a_view_is_edited_only_when_its_header_was_read` on 2022 and 2025). Both kinds stay listed
and droppable, as a trigger does.
**All three walks also hand back the header's comments** — `header_comments` on each parts
struct, bound for `TsqlModule` (under `schema.rs`), because the rebuild writes the header afresh
and the body box starts after `AS`. Each records where its `CREATE` token starts, where the
header ends — its own `AS`, or for a function with none the point the body starts — and the
spans it keeps as text (a parameter list, a column list, a `RETURNS`), and the private
`tsql_header_comments`
collects the text before `CREATE` verbatim and every comment after it up to the header's end,
skipping the kept spans, which carry their own. It walks `sql::skip_noncode` like the rest, so
a `--` inside a string or a bracketed name is not taken for a comment
(`a_triggers_header_comments_survive_its_rebuild`,
`a_routines_header_comments_survive_its_rebuild`, `a_views_header_comments_survive_its_rebuild`).
**`supports_or_replace_routine(MsSql)` is true**: `CREATE OR ALTER` keeps the routine's grants
(measured: a `GRANT EXECUTE` survived one), so an edit is altered in place. **What it refuses is
what `routine_signature_changed` asks there**, which is per-engine now: on SQL Server a change
of **kind** (procedure ↔ function) or of a function's **shape** — scalar, inline table-valued
(`RETURNS TABLE`), multi-statement (`RETURNS @t TABLE (…)`), each its own object type and
compared as `RoutineInfo::tsql_shape` (`TsqlShape`, under `schema.rs`) — which
`CREATE OR ALTER` answers with Msg 2010, *"incompatible object type"* (measured for a scalar
turned inline table-valued and for a procedure turned function). The parameter list is *not*
part of it, T-SQL having no overloading, so a new list or a scalar's new return type is the
same routine altered (`an_edited_sql_server_routine_is_altered_in_place`). A rename is a
recreate, `sp_rename` leaving the stored text naming the old routine — the call
`supports_view_rename` makes for a view — and it is the case the `diff_routine` rule above
exists for (`a_renamed_or_reshaped_sql_server_routine_is_dropped_and_created`).
**Except where the routine heads a numbered procedure group**: `DROP PROCEDURE grp` takes
`grp;2 … n` with it (measured on SQL Server 2022: `sys.numbered_procedures` one row to none),
and nothing can put them back — their text names the group's old name, and the model holds them
only to script them (`TsqlRoutine::numbered`, under `schema.rs`). A rename planned exactly that
drop and create, with nothing on screen to warn of it, the head's own text having no `;`. A
`CREATE OR ALTER` of the head keeps them (measured), so an edit in place stays allowed and only
a plan that drops first is refused: `numbered_group_refusal`, chained into
`ChangeSet::unsupported` beside `unreplayable_rename`, answers a `ReplaceRoutine { recreate:
true }` over a head — a rename, a change of kind or shape — so the preview withholds Apply;
`RoutineDraft::validate`'s SQL Server arm says the same while the new name is being typed; and a
deliberate `DropRoutine`'s risk names every member that goes with it
(`a_numbered_group_head_is_altered_in_place_and_never_dropped`; live,
`a_numbered_procedure_group_survives_every_plan_for_its_head`, which Azure SQL Database skips
for having no numbered procedures).
**What a create loses is its comment**, the `MS_Description` property, which a `CREATE` has no
clause for and a recreate's `DROP` takes with it. `routine_follow_ups`, one exhaustive `match`,
sets it through `TsqlComment::set` after a create or a recreate when there is one, and after an
in-place alter only when it changed (`a_sql_server_routines_comment_is_set_in_place`);
`TsqlComment` gained `object_type` for it, the level-1 type being `PROCEDURE` or `FUNCTION`
rather than `TABLE`. **The recreate's risk sentences were written for the other two engines
and had to learn this**: they told the user the comment "does not survive … re-apply any
COMMENT" over a plan that sets it again, and named the new name as the one dropped. The comment
half now asks `recreate_restates_routine_comment` (MySQL's `CREATE` carries a `COMMENT` clause,
SQL Server's plan sets it after; PostgreSQL's `COMMENT ON` is not repeated), the sentence names
the name the server holds, and `RoutineDraft::clash` (behind `name_clash`) asks `ddl_is_transactional` for its
consequence — on MySQL the `DROP` commits before the `CREATE` is refused, so the rename destroys
the original; SQL Server's `run_ddl` is one transaction, so the refused `CREATE` takes the
`DROP` back and nothing changes, which is what it now says
(`a_sql_server_routine_recreates_sentences_match_its_plan`). **Except for a natively compiled
routine**, draft's side or server's: its plan runs piecewise (`ChangeSet::runs_whole`, under
`Db::run_ddl`), the `DROP` commits as on MySQL, and the sentence says the original is destroyed
(`a_natively_compiled_routine_renamed_onto_a_taken_name_says_it_is_destroyed`). The trigger arm's "drops it first"
had the same fault over SQL Server's in-place alter; it asks `trigger_alters_in_place`, the
predicate `trigger_statements` plans with, and says the trigger is redefined in place
(`a_trigger_altered_in_place_is_not_said_to_be_dropped`). The change list's line above it
still said "Re-create trigger tr", because `Change::summary` has no dialect to ask with; the
answer is now asked once, when the change is built, and carried as
`ReplaceTrigger::in_place` — which the statement, the sentence and the line ("Redefine
trigger tr") all read, as `ReplaceView` carries `recreate`. `emit_mssql` now routes the shared `routine_statements`, whose drop arm
replaced the inline `DropRoutine` one it had, and `tsql_supports` admits `CreateRoutine` and
`ReplaceRoutine`, which is what switched `supports_routine_editing(MsSql)` on
(`sql_server_offers_the_routine_editor`). `RoutineDraft::validate` has a SQL Server arm — every
parameter named with `@`, the list cut at its top-level commas by `tsql_parameters` so neither
`decimal(10, 2)` nor a `N'x, y'` default splits one — and `RoutineDraft::blank` already opened
on T-SQL that compiles (`a_new_sql_server_routine_opens_valid`). The same arm refuses a `WITH`
option the routine's shape cannot carry (`TsqlShape::allows`), because the server's Msg 487
names neither the option nor why, and `INLINE = ON` beside `EXECUTE AS`, which a scalar function
cannot be inlined with (Msg 16203, measured on SQL Server 2022)
(`a_sql_server_routine_option_its_shape_refuses_is_refused`).
