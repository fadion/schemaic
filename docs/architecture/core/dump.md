# `core/dump.rs`

`core/dump.rs` — the **schema + data dump**: what one replayable `.sql` file holds and in what
order, as a `DumpPlan` of `DumpStep::Text`/`DumpStep::Rows` that `cli/dump.rs` executes for
both the app's modal and `schemaic dump`. It writes nothing and connects to nothing, so every decision in it is unit-tested.
**The interface calls this *Export*; the code calls it a dump, and the split is deliberate.**
`export` is already taken in this crate by `crate::export`, which renders *one result set* to a
file — a different feature with different inputs, and a reader who meets `export` in
`schemaic-core` should be able to assume the grid one. So the menu entries, the modal's title and
its primary button all say Export, while `dump_run`, `DumpPlan` and `cli::dump` keep the code
name; the module doc is where the two vocabularies are reconciled, and it is what to read before
"fixing" a `dump_` name that sits behind a menu called Export.
**It joins the emitters that already existed.** Structure is `TableInfo::create_ddl_holding`
(`create_ddl` split around the rows, which routes a view on to `ddl::view_ddl`), the closing
constraints are a `ddl::ChangeSet` of `Change::AddForeignKey` through `ChangeSet::emit` — the
emitter the apply path uses — and the rows are `dump::render_rows`, streamed by `cli/dump.rs`, which
shares `export::export_inserts_ending` with the grid's SQL export: on MySQL, PostgreSQL and
SQLite the two write the same statements, while on SQL Server each `INSERT` closes its own
`GO` batch and binary, CLR and variant columns go out as the literals the server rendered,
which the grid's export does not write. **SQL Server has emitters of the dump's own**, for
what nothing else here scripts: `TsqlObject::create_sql`, `create_if_absent_sql`, `drop_sql`
and `restart_sql` for its sequences, alias types, XML schema collections and synonyms, and the
up-front `DROP … IF EXISTS` and `refuse_if_present_sql` statements of a replay; PostgreSQL's
refusal ahead of the drops is `cascade_guard_sql`, a `DO` block that stops a replay whose
`DROP … CASCADE` would take what the file does not put back (under `drop_cascade` below). Triggers are
`TriggerInfo::create_set_sql`, the **whole-set** form and not one `create_sql` per trigger: the
catalogue gives a group's leader `PRECEDES <successor>` and a restore reads the file top to
bottom, so the first `CREATE TRIGGER` named a trigger the file had not created yet and both
servers refused it (`ERROR 3011` on MySQL 8.4.11, `ERROR 4031` on MariaDB 10.11.14) — after the
`DROP TABLE` above it had already run against the target. `core::schema`'s entry has that rule in
full. **Each table's set is written in a trailing `-- Triggers` section, after every table's
rows and after the routines**, not beside its `CREATE TABLE`: created before the rows, an
`INSERT` trigger fired once per restored row on every engine — an audit trigger doubled its
audit table, a stamping trigger rewrote every restored value — and the restore reported success.
After the routines because a PostgreSQL trigger names its function, which must exist first.
`mysqldump` writes triggers after the data for the same reason
(`triggers_are_created_after_the_rows_they_would_fire_on`, over all four engines; the live
round trip's audit trigger restores an audit table of the source's size). That ordering cannot
reach a trigger the server makes itself: a PostgreSQL partition of a parent the file leaves
standing clones the parent's row triggers at its `CREATE`, so its rows are written with them
switched off (`cloned_triggers_held_sql`, under the named parent below). Identifiers go through
`export::ident_sql`/`qualified_table`, so a dump cannot quote differently from the SQL export.
**The header's own comment lines are *escaped*, not quoted** — `export::comment_text`, and the
two are different guarantees. Four lines here interpolate a schema-fetched name: the
`-- Schemaic dump of {database}` opener, the server-assigned-columns note, the "ticked but not
found" note, and the per-table `-- {name}` line, which is the cheapest of the four for an
attacker to control. A `--` comment ends at the first physical newline and a PostgreSQL name may
hold one, so a table called `orders` / `DROP TABLE customers;` put that second line into the file
as a **top-level statement**, which then runs at *restore* against whichever database the restore
targets. `ident_sql` is not the fix — it doubles a quote character and says nothing about `\n` —
and the rule is stated in full under *Architecture invariants*.
`a_newline_in_a_name_cannot_open_a_line_of_its_own` asserts it over the emitted plan rather than
over the escaper, since a test of the escaper alone was green against the unfixed tree.
**The row `SELECT` names its columns and is never `SELECT *`.** `dump_columns` projects
everything `ColumnInfo::is_server_assigned` says the server does *not* fill for itself — the same
predicate `import::insert_columns` asks, about the same columns — plus the one server-assigned
column a file can carry, SQL Server's identity (below). The renderer names every column
the result carries, so `SELECT *` put generated columns and PostgreSQL `GENERATED ALWAYS AS
IDENTITY` columns straight into the `INSERT` column list, which all three engines refuse (MySQL
3105, SQLite *cannot INSERT into generated column*, PostgreSQL *cannot insert a non-DEFAULT
value*) — the file died on its first row. PostgreSQL's identity would need an `OVERRIDING SYSTEM
VALUE` the shared renderer has no way to emit. **The cost:** such an identity column's values are
not carried, so the restored rows are renumbered — which the **header names, column by column**,
alongside the cycle and dropped-constraint notices. The person replaying the file is the one who
needs to know, and it is the same silence about a `NULL`ed blob that the tally exists to break;
a file that carries every column says nothing, because a caveat printed on every dump is one
nobody reads (`a_column_the_file_cannot_carry_is_announced_in_it`,
`a_file_that_carries_every_column_says_nothing_about_it`). A table whose columns are *all*
server-assigned gets **no data step at all**: nothing about it is insertable, and `SELECT  FROM`
would not even parse. Both halves are pinned by
`a_generated_column_is_never_selected_into_the_insert` — which also asserts the column is still
*declared*, since it is only the `INSERT` it has to stay out of — and
`a_table_the_server_fills_entirely_gets_no_data_step`.
**SQL Server's identity is the exception, and renumbering it was never an option.** It is
`identity_always` — the server refuses an explicit value — except under `SET IDENTITY_INSERT t
ON`, which a session may hold for one table at a time; and renumbering the keys a foreign key
points at breaks that key, on every row that names one. So `carries_identity` admits an
`auto_increment` column with no `generated` expression (not a `rowversion`, not a computed
column) wherever `identity_insert_sql` has a switch to throw — SQL Server alone, an exhaustive
`match` — and `plan` puts that table's `Rows` step between the `ON` and the `OFF`, a table
carrying no identity getting neither. No resync follows it, unlike PostgreSQL's
`sequence_resync_sql` below: the server moves the identity's next value past the highest one
inserted. The exception lives in `dump_columns`, the one projection, which took
`dialect` for it and absorbed the dialect-free `exported_columns` it replaced. **The header's lost-columns sentence says *renumbered* only when a lost column is a
counter** (`auto_increment`): on SQL Server what the file leaves out is a `rowversion` or a
computed column, which the server recomputes, and telling the person replaying it that their
rows were renumbered would be false (`a_sql_server_identity_is_carried_inside_identity_insert`,
which asserts the switch brackets the rows, the identity is absent from the header and the word
is too).
**On SQL Server the dump carries what a text cell cannot, by asking the server for SQL.** A
`varbinary`, `binary` or `image` cell is the `<n bytes>` placeholder, and so are the CLR types
`hierarchyid`, `geography` and `geometry`; a `sql_variant` arrives as its value's text, so a
`date` or `decimal` variant restored as an `nvarchar` one and compared differently.
`literal_select` gives the rows step's `SELECT` an expression for each such column instead —
`'0x' + CONVERT(varchar(max), CAST(c AS varbinary(max)), 2)`, which T-SQL converts back to the
CLR type on insert with the SRID, Z and M kept (measured; style 2 because style 1 renders empty
bytes, the hierarchy's root among them, as an empty string), and for a variant its base type
with length, precision, scale and collation beside its value in an exact, language-proof text
form (a variant's own text of a date is style 0, `Jan  2 2026  3:04AM`, which drops the
seconds) — and names it in `DumpStep::Rows::server` with its `export::ServerLiteral` form. **A
column typed by an alias is judged by the alias's base**, found in `DbSchema::tsql_objects`:
its `type_name` is the alias's qualified name (`[dbo].[Hash]`), so a `NOT NULL` alias over
`binary` was written as a withheld blob's `NULL` and stopped the restore (Msg 515), and a variant
through one came back `nvarchar` (`a_column_of_an_alias_type_is_read_as_its_base`, live
`a_dump_restores_alias_typed_bytes_and_variants`). **A character variant's value is the server's `FOR JSON` of it** (`{"x":"…"}`): `CAST(v AS
nvarchar(max))` stops at 4,000 characters, the `max` not honoured from a variant, while a
`varchar` one holds 8,000, so a 5,000-character value restored cut with `Done`; `CAST(v AS
varchar(8000))` keeps the length but converts to the *database's* code page; `FOR XML` refuses a
control character (Msg 6841) and `OPENJSON` needs compatibility level 130 — so the renderer
unpacks the document itself. The renderer writes those cells from the server's text **only
after checking it**: bytes must be `0x` and hex digits, a variant's type must have a system type
name's shape, a character one's value must be that one-member document, and the text goes
through `sql_literal`, written `CAST(CAST(… AS <type>) AS sql_variant)` (the outer cast because
a multi-row `VALUES` gives a column one type, and a `date` beside a `decimal` is Msg 206) — **a
character one `CAST(CAST(N'…' COLLATE <c> AS <type>) AS sql_variant)`, the collation on the text
inside the cast**: after it, the cast has already converted under the restoring database's code
page and the `COLLATE` converts those bytes again, so a Greek variant restored into a Latin-1
database as `Oµ??a`. A cell that fails is `NULL` and its column is named as withheld. Every
other engine keeps its blobs withheld and noted
(`a_sql_server_dump_reads_bytes_and_variants_as_literals`,
`server_rendered_literals_are_checked_and_quoted`,
`a_character_variant_converts_under_its_own_collation`; the live round trip's `blobs` table
carries each of those types and a variant of every base type, and compares bytes, SRID and
every variant property on the copy, and `a_dump_restores_character_variants_byte_for_byte`
compares the bytes of Greek, Japanese, UTF-8, control-character and 5,000-character variants).
**On an engine whose scripts are cut into batches, every statement closes one.**
`close_batches` runs over the finished plan wherever `SqlDialect::batch_separator` is true —
SQL Server — and ends each `Text` step that is more than comments with a `GO` line, unless its
last line already is one (`ddl::client_script`'s trigger text carries its own). **A `Rows` step
closes its own batches, one per `INSERT`**: `dump::render_rows` is the renderer the driver's writer
runs for every such step (through `export::export_inserts_ending`, which ends each statement
`;\nGO\n` there), where the step used to share one `GO` placed after it. SQL Server refuses a
batch past 65,536 network packets — 256 MB at the default size — so a `sqlcmd` restore of a
large table was dropped mid-file, and the app's own restore got through only because its
splitter also cut at every `;`; a statement is at most `INSERT_BATCH_BYTES` plus a row, so a
batch per statement holds whatever the table's size
(`a_sql_server_rows_step_closes_a_batch_after_every_insert`). `CREATE VIEW`, `CREATE TRIGGER` and a routine must each open a
batch of their own, and a restore — `Db::run_script` behind the script splitter, or `sqlcmd` —
cuts the file at its `GO` lines, so without them the first view after a table's rows failed
(Msg 111, per the test's doc) — `a_sql_server_dump_closes_every_batch_with_go`, which also
asserts MySQL's file carries neither a `GO` nor an `IDENTITY_INSERT`. The live pin is
`a_dump_restores_into_an_empty_database` in `tests/live/mssql.rs`, **one fixture that grows with
each restore bug**: a scratch database with an identity holding a gap, a `rowversion`, a
computed column, foreign keys, views, an audit trigger, a function and a procedure, a table
whose computed column, check and default call the function, a disabled check and key and an
untrusted check over violating rows, more rows than one `INSERT` carries, `float`/`real`
extremes and `datetime`/`smalldatetime`/`datetime2`/`date`/`decimal`/Unicode values is dumped,
its rows rendered through `render_rows` as the driver's writer renders them, split and replayed
through `run_script` into an empty database under `SET LANGUAGE british`, then replayed a second
time onto that copy; each time one query per side (a debug build's poll frame grows with every
`await`, and the fixture overflowed a 2 MiB test thread) must read back the same rows,
constraint states, trigger and routine, and the next identity continues from the highest
carried. Even so it sat one `await` from the limit, so it runs on a thread of its own with a
16 MiB stack and its own runtime (`on_a_large_stack`), and each later restore shape — a view
reached through a function, a left-out temporal or ledger table and what reads it, a synonym's
empty part, a routine-only type — is a leg of its own beside it rather than one more await in
it. `SCHEMAIC_IT_KEEP_DUMP=<path>` keeps the file for a hand restore through `sqlcmd`.
**`sqlcmd` substitutes `$(name)` inside string literals**, from its variables and the
environment of the shell running it, and only its `-x` flag stops it: a row holding `cost
$(HOME) here` restored as `cost /home/mssql here`, with exit 0 (measured with ODBC sqlcmd 18 on
2022 and 2025; a `GO` or `!!` line inside a literal is *not* acted on). So every literal a
script writes goes through `export::script_literal`, which on such a client
(`export::client_substitutes_variables`) cuts the literal between each `$` and its `(` —
`(N'cost $' + N'(HOME) here')`, the same value with no reference in it, parenthesised so a
variant's `COLLATE` covers the whole, and its first piece `nvarchar(max)` past 4,000 units
because two shorter `nvarchar` pieces concatenate to at most 4,000
(`a_sql_server_script_literal_holds_no_sqlcmd_variable`; the export and *Copy as SQL INSERT*
share it). A module body, a default or a name is the server's text and must be restated as it
is, so where one holds a `$(` the dump's header says to restore with `sqlcmd -x` or *Run SQL
file* — in a sentence holding no `$(` of its own
(`a_definition_holding_a_sqlcmd_variable_is_named_in_the_header`); measured, the rows of such a
file restore byte-equal without `-x`, and the procedure body only with it. **A name is rewritten
as well, brackets and all** — `CREATE TABLE dbo.[z$(HOME)t]` made `z/home/mssqlt`, exit 0 — and
cannot be cut without becoming another name. The dump's header already reads its table list for
the check (`a_data_only_file_whose_table_name_holds_a_sqlcmd_variable_says_so`); a file of
`INSERT`s with no header — the grid's SQL export, *Copy as SQL INSERT*, each file of a folder
export, and a dump's rows alike — gets a `-- NOTE:` before its first `INSERT` from
`export_inserts_ending` where its table or column list holds a `$(`
(`a_sql_server_script_whose_name_holds_a_sqlcmd_variable_says_how_to_run_it`).
**Foreign keys are restated after the data.** `create_ddl` deliberately emits none: for Copy DDL
an omitted key still leaves a script that runs, which is why the ordering effort there went to
types and views instead (`create_ddl_script`'s own account of it). A dump can't take that trade —
a restore that silently drops every constraint is not a restore — and the trailing section is
also what stops the insert order invalidating the file. It is **skipped for a table whose
`create_sql` is `Some`** (`needs_fk_section`): SQLite's verbatim captured `CREATE TABLE` already
carries the keys, and SQLite has no `ALTER TABLE … ADD CONSTRAINT` to restate them with. That
question is asked of the *table*, never of the dialect — the data answers it directly.
**A table no `CREATE` from the model restates is left out of the file and named in its
header** — a system-versioned table, its history table, a memory-optimised table, and an
updatable or append-only ledger table with its history table and its ledger view
(`TsqlTableKind::unrestatable`, the view being the one of a kind): kept, its `DROP` destroyed
what the file could not put back and its rows landed in nothing, and a ledger table came back
as a plain one carrying its ledger columns as data, its history as one more table and its view
as a plain view, a copy that took updates and deletes with no trace while the restore reported
success (`a_ledger_table_its_history_and_its_view_are_left_out_and_named`; live,
`a_dump_leaves_a_ledger_table_out_and_names_it`). Out of `order`, a key onto one is counted with the keys to tables
outside the export. **What cannot be created without it goes with it**
(`dependents_left_out`): every chosen view reading one, every routine in the export's
namespaces that binds to one at `CREATE` — schema-bound, natively compiled, or an inline
table-valued function (`binds_at_create`) — and every table whose expressions call such a
routine, each in turn counting as gone. One left in stopped the whole restore at its
`CREATE VIEW` (Msg 208, measured on 2022) and *One transaction* rolled everything back, where
without it the file restored one table short; a routine that only reads the table when it runs
stays. Names are matched with their qualifier read off the text (`names_object`), so a view
over `sales.emp` is not taken for one over `dbo.emp` — `pub(crate)`, because `core::used_by`
asks the same question of every body in a schema. The lot is `DumpPlan::left_out`, named
in the header and, through `dump::done_note` (worded alike by `dump_view`'s `dump_done_note`),
in the modal's report beside the missing tables (`what_reads_a_table_left_out_of_the_file_is_left_out_with_it`; live,
`a_dump_leaves_out_what_reads_a_table_it_cannot_restate`). A graph edge is restated, empty, and the header says why: an edge names
the nodes it joins by node id, and a restore gives every node a new one; a node's rows are
carried (`a_table_the_file_cannot_restate_is_named_and_left_out`; the live round trip's node
comes back a node with its rows, its edge an empty edge, and its temporal pair absent). For
the same reason a replay never drops either — see `dump::Hold` below.
**A constraint that was off in the source goes back off, after the rows.** SQL Server keeps a
disabled or untrusted check or key over rows that violate it; restated as an ordinary one the
restore stopped at those rows (Msg 547), and where none did the copy silently enforced what the
original had switched off. `ForeignKeyInfo::not_enforced`/`not_validated` carry the key's
state (`FK_LISTING` reads `is_disabled`/`is_not_trusted`; `false` on the engines with no such
state), and `ChangeSet::emit`'s SQL Server `AddForeignKey` arm restates it as its `AddCheck` arm
already did a check's — `WITH NOCHECK ADD`, then `NOCHECK CONSTRAINT` for a disabled one. The
check half is `TableInfo::create_ddl_holding`: the `CREATE TABLE` without such a check, and the
`ALTER TABLE`s that put it back, which `plan` writes in a `-- Checks` section after every table's
rows — an untrusted check is still enforced for new rows, so added before them it refused them
(`a_sql_server_constraint_that_was_off_is_restated_off_after_the_rows`; the live round trip
carries a disabled check and key and an untrusted check over violating rows and compares each
one's `is_disabled`/`is_not_trusted` on the copy). **PostgreSQL's `NOT VALID` is the same
state**, and `create_ddl_holding` holds it back wherever `schema::unvalidated_check_waits_for_rows`
says the engine keeps a constraint over rows that violate it in a form `CREATE TABLE` cannot
recreate — not wherever the engine is SQL Server, which is what it asked before: PostgreSQL
ignores `NOT VALID` inside `CREATE TABLE` and validates the check there (measured on 16), so the
dump's own violating rows failed the restore and its transaction rolled everything back. The
check now goes back `ADD … NOT VALID` in the `-- Checks` section; Copy DDL, which loads no
rows, keeps it inline. The key half: `pg::fetch_schema` reads `convalidated` into
`not_validated`, which it never did, so a `NOT VALID` key's closing `ADD CONSTRAINT` validated
the orphans the source had kept, and `ddl::fk_clause` writes `NOT VALID` where
`schema::writes_not_valid` says the engine spells it so
(`a_postgres_not_valid_check_and_key_go_back_not_valid_after_the_rows`; live,
`a_not_valid_constraint_restores_over_the_rows_it_spares`, a no-op on the engines with no
such state).
**And only a key whose target table is in the same file.** Exporting one table is a first-class
action now — a table node's own *Export* — and `ALTER TABLE orders ADD CONSTRAINT … REFERENCES
customers` in a file that never creates `customers` fails at restore: on PostgreSQL with no guard
to hide behind, and *after* the rows have landed. The keys pointing outside the selection are
dropped, counted, and **the header says how many and why**, which is the honest half of the trade
— emitting a statement that cannot succeed was the alternative, not a completer restore. That is
also why the whole constraints section is **decided before the header is emitted** even though it
is written last: the header has to be able to report the count, so a future tidy that moves the
section back into file order silently empties that sentence
(`a_foreign_key_to_a_table_outside_the_selection_is_not_restated`).
**Which keys those are is `fk_targets`, and on MySQL it could not answer at all until it was
given the home database.** A key with no namespace of its own means "in `owner`'s", not "in
any" — and the predicate compared `fk.ref_schema.or(owner.schema)` against `cand.schema`, while
on MySQL `TableInfo::schema` is always `None`, a database *being* its namespace. So both sides
were unknown, the "one side unknown still cannot be answered no" arm fired, and a key matched on
`ref_table` **alone**: a cross-database `REFERENCES archive.customers` was classified as in-dump
because a table called `customers` was in the dump, and the strip below then emitted it bare, so
the restored file points the key at the dump's own `customers`. `fk_targets` takes a
`home: Option<&str>` now — the database being dumped, which is the namespace every table in it
has on an engine where `ref_schema` is a database — and `plan` derives it from
`ddl::ref_schema_is_database` rather than from the engine. `order_tables` grew the same
parameter; the schema comparison passes `None`, its two sides being two different databases. The
premise is measured on MariaDB 10.11.14: a cross-database InnoDB key is legal, and
`information_schema.KEY_COLUMN_USAGE` reports `REFERENCED_TABLE_SCHEMA` for it.
**The strip is what consumes that answer, and it restates the rule at its own line.** A key whose
target is in this dump names it the way the file names its own tables — bare — because
`information_schema` reports `ref_schema` even for an ordinary same-database key, and a
hard-qualified `REFERENCES` under a bare `ALTER TABLE` means editing the `USE` line moves every
table, row and trigger while every key still points at the source, which on the same server
succeeds *silently* and constrains the copy against production. It is `DumpStep::Rows`'
`insert_database` rule one section further down, found and fixed on the `INSERT` half first. The
restatement is not redundancy: the partition decides *which* keys reach it and this decides
whether the qualifier is this database's name, so the guard holds at the line that does the
stripping and not only at the one that sorts.
**A key carried inside a verbatim `CREATE TABLE` still points outside the export, and that is a
second counter with a second sentence.** `needs_fk_section` used to gate the whole loop body, so
on SQLite — where the keys ride in the captured DDL and there is nothing to restate — the count
stayed 0 and the header said nothing at all: replayed against SQLite 3.53.2 the load is clean,
`PRAGMA foreign_keys = ON` not validating existing rows, and every later write to the table fails
with *no such table: main.customers*. The question that predicate answers is *"restate it
separately?"*, never *"is it carried?"*, and only the first belongs in front of the emit, so
`dangling_fks` is tallied where the `continue` used to be. Its sentence is deliberately not the
`dropped_fks` one: nothing was dropped, the constraint is in the file and restores without
complaint, and it points at nothing — saying the key is gone would be the opposite lie
(`a_verbatim_key_pointing_out_of_the_export_is_still_reported`, which asserts the new wording
appears **and** that the restatable engines' one does not).
**The FK guard wraps the transaction, never the other way round.** `PRAGMA foreign_keys` is a
silent no-op inside a transaction on SQLite, so a guard emitted after `BEGIN` reads correctly in
the file and does nothing at all. `fk_guard_sql` and `transaction_sql` are the strings and `plan`
is what composes them, which is why `the_sqlite_guard_sits_outside_the_transaction` asserts on
the whole file rather than on either function — the bug is the composition. **PostgreSQL gets no
guard**: `session_replication_role` is superuser-only, so offering it would be a checkbox that
fails the restore for most roles, and the ordering plus the trailing constraints section is the
answer there. **On SQLite the guard is written whenever the file drops a table, whatever the
toggle says** (`drop_runs_referential_actions`): SQLite's `DROP TABLE` with foreign keys on is
an implicit `DELETE`, which runs every referencing table's `ON DELETE` action, so unticking
*Disable foreign-key checks* — the stricter-looking choice — had a replay of `customers` empty an
`orders` the file never names (reproduced: 2 rows → 0) under a success report. MySQL refuses
such a drop (ERROR 3730), SQL Server too (Msg 3726), and PostgreSQL's `CASCADE` drops a key, not
a row, so their toggle stands; the toggle's hint says so where it does not —
`fk_guard_says_drops` answers `FkHint::DropsTurnItOff` there, and `fk_guard_hint` is its English
(`a_sqlite_file_that_drops_tables_turns_foreign_keys_off_whatever_the_toggle`).
**The guard's mode is the import's and it pins the zone**: `export::import_mode_sql` — the
flag off and `NO_AUTO_VALUE_ON_ZERO` on, so a key of 0 restores as 0 — and, saved and put back
like the mode, `time_zone = '+00:00'` (`export::rows_session_sql`), the zone the rows were read
in (`Db::for_rows`): MySQL hands a `TIMESTAMP` over in the session's zone and reads a literal
back from it, and the file named none, so a dump from a `+05:00` session replayed on a `-03:00`
one moved every `TIMESTAMP` eight hours (`a_mysql_dump_pins_utc_and_keeps_a_zero_key_around_its_rows`;
live, `a_timestamp_restores_as_the_instant_it_was`, red by five hours with the read unpinned).
`mysqldump`'s two pins, for its reasons.
**`literal_mode_guard_sql` sits outside both, and is not one of the two optional scaffolds.**
It is `export::literal_mode_sql` — the statement that takes `NO_BACKSLASH_ESCAPES` off the
session so every `'…'` in the file means what `sql_literal` wrote it to mean — with the
save-and-restore pair a *file* needs around it, because a `.sql` file is replayed into a session
the user goes on using (`Db::run_script` holds one connection for the whole file, and the client
a user replays it in holds one for the evening). `plan` emits it **before the container
statements**, ahead of the FK guard and the transaction: the `CREATE DATABASE` and `CREATE
TABLE`s in front of the transaction carry literals too, a column comment or a quoted default.
And it is emitted **unconditionally** — `disable_fk_checks` and `wrap_transaction` choose how the
load behaves, while this one is the file saying what it says. `mysqldump` pins the mode at the
head of every file for the same reason. **`date_format_sql` is its SQL Server sibling, emitted
beside it**: `SET DATEFORMAT ymd;`. `datetime` and `smalldatetime` read `2026-01-02 03:04:05`
by the session's language, so a restore by a `british` login (or a German, French, Italian or
Spanish one) swapped day and month up to the 12th, silently, and failed the batch from the 13th
(Msg 242). It is session state, so one line holds for every later `GO` batch on both restore
paths; there is no restore half, since T-SQL cannot read the format back
(`a_sql_server_dump_pins_its_date_format_before_any_row`; the live round trip restores under
`SET LANGUAGE british`). The rows' own literals no longer lean on it — `export::typed_literal`
writes those two types in the `T` form — and the pin stays for whatever other date text the
file carries. **`ansi_settings_sql` follows it**: `SET ANSI_NULLS ON;` and `SET
QUOTED_IDENTIFIER ON;`, because `sqlcmd` opens with the second OFF unless given `-I`, and a
filtered index or a computed column's index is refused under it — each module's own script
states the settings it was created under (`ddl::tsql_settings_around`). **The
`@SCHEMAIC_OLD_SQL_MODE` wrapper is MySQL's
session-variable syntax**, so this leans on `literal_mode_sql` answering for MySQL alone and
carries a `debug_assert` saying so: a second dialect needing a statement there has to grow an
arm here, not have its statement wrapped in these two. Whether a PostgreSQL *dump* should pin
`standard_conforming_strings` the way `pg_dump` does is a separate question and an open one —
the live path has no need of it, `pg::connect_probe` pinning the GUC on the startup packet, but
a file is replayed in somebody else's client.
**`target_database_sql` emits `USE` on MySQL only, and it is load-bearing.**
`create_ddl` names a MySQL table bare — a database is not a namespace there, so there is nothing
to qualify with — while the `INSERT`s come from the export renderer, which addresses a table
through `qualified_table` and *does* name the database; without that line the file would create
`orders` wherever the client is pointed and then insert into `shop`.`orders`.
PostgreSQL needs none (both halves name the namespace) and SQLite has no qualifier at all.
**Both halves name it `public` included**, and so do the file's `DROP … IF EXISTS`, T-SQL's
`SET IDENTITY_INSERT` and up-front key drops, and `sequence_resync_sql`'s table — all through
`schema::qualified_ident` — so a `DROP` and its `CREATE` still name one table. `create_ddl`
used to leave `public` off while the rows named it: replayed into a database holding a schema
named after the login, the `CREATE TABLE`s landed there through `search_path` and the first
`INSERT INTO "public".…` failed (live, `public_objects_are_named_in_every_statement_that_runs`).
What that line is **not** is a retarget: a `mysqldump` is replayed elsewhere by editing its one
`USE`, because its `INSERT`s name the table bare, while these name `` `shop`.`orders` `` outright
— so the file is locked to the database it came from, and that is **known debt rather than a
finished answer**. The fix is a way to render an unqualified target in
`export::qualified_table`/`export_inserts_chunks`, which is the grid's export path too — too wide
a change to make from inside this feature, so it is written down here rather than done. A second
edge is deliberately left alone: `qualified_table(database, None, table, Postgres)` names the
*database* as a namespace, which is wrong, but `pg::fetch_schema` always sets a
`TableInfo::schema` so nothing here reaches it, and the fallback lives in the shared quoter where
changing it for this one caller is the worse trade.
**Three further statements are here because the file failed on replay, and each asks a capability
rather than an engine.** `create_container_sql` makes the file's own container before anything
uses it — `CREATE DATABASE IF NOT EXISTS` on MySQL, one `CREATE SCHEMA IF NOT EXISTS` per
non-default namespace on PostgreSQL — because the primary use case is a restore onto a *fresh*
server, where `USE shop` is ERROR 1049 and a table in `sales` is `schema "sales" does not exist`:
the file died on line 1. `IF NOT EXISTS` throughout, since the other use case is replaying onto
the database it came from and that must not open with an error either; PostgreSQL gets no
`CREATE DATABASE` (it cannot be run from inside the database being restored into, and the
connection is already pointed at one) and `public` is skipped. `drop_cascade` appends ` CASCADE`
on PostgreSQL alone: MySQL and SQLite both have a session switch that turns key enforcement off
for the whole load, PostgreSQL's is superuser-only and `fk_guard_sql` returns `None` there, so
nothing else in the file protects the `DROP` — replaying a default dump onto the database it came
from, which is how anyone tests a dump, stopped at the first parent table with *cannot drop table
customers because other objects depend on it* and never reached the rest. Dropping the file's own
dependants is right precisely here, because the section below is their `CREATE` and the closing constraints
section puts the keys back — **those from tables outside the file as well**, onto any table in
it, each through `unless_present_sql` (`Present::Constraint`) so it is added only where it is
missing. That section once restated only the keys between the file's own tables, and `CASCADE`
does not stop at them: replaying a dump of `customers` onto its source left `orders`' key onto
it gone, and the replay reported success (measured live on PostgreSQL 16, per the change's
author). Restated after the rows, the key is validated against them, so a replay whose rows no
longer satisfy it fails loudly rather than leaving it off. No other engine writes one, and none
needs to: under their key switch a MySQL/MariaDB or SQLite `DROP` leaves the referencing table's
key standing (verified live on MySQL 8 and MariaDB 10.11, per the change's author), SQL Server's
up-front drop is refused by such a key (Msg 3726, below), and `unless_present_sql` is `None`
on all three — unguarded, a replay that took nothing would stop at a name already taken
(`a_key_from_outside_onto_a_chosen_table_is_put_back_where_missing`; live, on each
`live_suite!` leg, `a_replay_keeps_the_key_a_table_outside_the_file_holds`, which also checks
the restored key refuses an orphan). **And right only there: `CASCADE` does not stop at the
file's own dependants.** It also took a view or materialized view outside the file that reads a
dumped table — the materialized view with its rows — a table inheriting from one, a column typed
with its row type, a default on a sequence it owns and keys the introspection never read, and
the replay reported success (live on PostgreSQL 16, per the change's author, the view over a
view outside the file was gone: *relation "public.v_chain" does not exist*). So ahead of the
first `DROP`, inside the file's transaction where it has one, `cascade_guard_sql` writes a `DO`
block that walks `pg_depend` from every table and view the file drops the way `CASCADE` does —
every dependant, and up through an internal dependency to its owner (a view from its rule, an
outside key from a partition's clone of it) — then takes away what the dropped tables own
(reached through their auto, internal and partition dependencies alone), the keys the closing
section restates, the routines the file recreates with `CREATE OR REPLACE`, and any part of
something already counted. Whatever is left is named in one error — *replaying this file would
drop … along with the tables it replaces, and does not put them back as they are*, with the hint *Add them
to the export, or drop them yourself first* — before anything is dropped. A relation the target
does not have seeds nothing (`to_regclass` is `NULL`), so a restore into an empty database walks
nothing. Measured on PostgreSQL 16 by the same author: a one-partition dump's walk reaches the
key an outside table holds on the parent, through the partition's internal clone of it, and
accepts it only because the closing section restates it. **Refusing is the answer, not putting
them back from the model** — the tempting "fix": the model holds no grants, owners, comments,
`INSTEAD OF` triggers or rules; a view in a schema the dump did not read is not in it at all; one
changed since the dump would come back stale; and a materialized view recreated `WITH DATA` is a
new snapshot, not the one the user had. The trade is that a replay onto a source holding a view
outside the file over a dumped table, which used to "succeed" with the view gone, now stops
loudly with nothing dropped. A routine the file recreates is accepted although `CASCADE` drops
it **only where recreating it changes nothing about it** — `proacl IS NULL` (the default
privileges) and owned by the replaying role (`a_cascade_may_take_a_routine_the_file_recreates`).
The file's `CREATE OR REPLACE` *creates* a routine the cascade took, with the default ACL and
the replaying role as owner, so a `SECURITY DEFINER` function over a dumped table's row type,
revoked from `PUBLIC`, came back executable by every role under a success report (measured on
PostgreSQL 16: `proacl` `{schemaic=X/schemaic}` → `NULL`); one with grants or another owner is
named now, and the replay stops (`a_replay_does_not_reopen_a_locked_down_routine`, live — red
against the old guard). **The dropped tables' and views' own owner, grants, row security and
policies are put back** at the file's end, inside its transaction — read for the dump rather
than from the model (`core::access`; `a_pg_replay_puts_back_each_dropped_tables_owner_grants_and_row_security`,
and `a_pg_replay_puts_back_the_owner_grants_and_row_security`, live, red with the read taken
out); a section headed *-- Owners, grants and row security*, after the foreign keys, so no
statement above it runs under an owner or a row security the replaying role does not have. SQL
Server's file writes the same section with its owners and permissions
(`a_replay_puts_back_the_owner_and_permissions_of_what_it_drops`, live on 2022 and 2025, with
and without *One transaction*). Where the read failed, the header says the file *does not put
back the owner, grants or row security* of what it replaces
(`a_pg_replay_without_the_access_read_says_it_does_not_put_it_back`). The block is asked of `drop_cascade`, so it
is written only where that is non-empty and the file drops something — MySQL, SQLite and SQL
Server files are byte-identical to before — and never with *Drop before create* or Structure off
(`a_replay_stops_before_a_cascade_would_drop_what_the_file_does_not_put_back`,
`the_cascade_guard_is_written_where_the_drop_cascades`; every name a string literal and the
dollar tag one no name contains, `the_cascade_guard_quotes_every_name_it_carries`; an empty list
a typed empty set, since `VALUES` with no rows is not SQL, `the_cascade_guard_takes_empty_lists`;
live, on each `live_suite!` leg, `a_replay_leaves_the_views_outside_the_file_standing`, red on
PostgreSQL 16 before the fix, which also replays the same table with its views in the file).
**A materialized view is dropped as one** (`drop_keyword`, asking `ddl::is_materialized_view`):
the drop asked only `is_view`, so the file wrote `DROP VIEW IF EXISTS` for it, which PostgreSQL
refuses (*"mv" is not a view*). It went unseen wherever an earlier table's `CASCADE` had already
taken the view, and stopped the replay wherever none had — a materialized view over a table the
file does not carry (`a_materialized_view_is_dropped_as_one`; live,
`a_replay_replaces_the_materialized_view_it_carries`, red on PostgreSQL 16 before the fix).
**SQL Server has neither answer**, so it drops everything the file
recreates up front (`drops_up_front`, computed from the two: no guard and no `CASCADE`): a
`-- Dropped first` section before any `CREATE` drops each key between the dumped tables that
is there (`IF OBJECT_ID(…, N'F') IS NOT NULL ALTER TABLE … DROP CONSTRAINT`), then everything
else in the exact mirror of the file's creation order — the trailing routines first, then each
view and table children-first with the functions `creation_order` moved in ahead of it dropped
straight after it (`DROP FUNCTION`/`DROP PROCEDURE IF EXISTS`) — and the per-table `DROP`
beside each `CREATE` is not written. **The mirror is the only order that works**: a
schema-bound function holds the table it reads and is held by the table whose column calls
it, so the routines dropped as one block after every table stopped a replay at the read
table's drop (Msg 3729) and no single place for the block meets both edges
(`a_sql_server_dump_drops_in_the_reverse_of_its_creation_order`; live,
`a_replay_drops_a_schema_bound_function_between_its_tables`). Beside its `CREATE`, the `DROP`
of a referenced table found the child's key still standing, since the tables are created
parents first, and replaying a default dump onto its source stopped there (Msg 3726); a
routine's `CREATE` would have stopped at a name already taken next. A key from a table outside the export still blocks its target's drop, loudly — the file
drops nothing it does not put back
(`a_sql_server_dump_drops_everything_it_recreates_before_creating_any_of_it`; the live round
trip replays the file a second time onto the restored copy and compares it again). **The
section is written only inside the file's own transaction** (`DumpOptions::wrap_transaction`),
because a failure in it is safe only when the failure undoes it: without one, a replay stopped
at an up-front `DROP` — that outside key — left every object dropped ahead of it gone, with
every `CREATE` still below the failure. With *One transaction* off each table and view is
dropped beside its own `CREATE`, as on the other engines, and no key or routine is dropped, so
a failure has dropped only what was already put back
(`without_one_transaction_a_sql_server_file_drops_each_table_beside_its_create`).
**Nor does a replay ever drop what the file cannot put back as it is** (`dump::Hold`,
`table_hold`, `routine_hold`): a graph edge, whose rows are not in the file; a graph node
while an edge exists anywhere, whose rows come back under new node ids; a table carrying an
encrypted or signed trigger, which goes down with it; a signed routine; and a numbered group
with a member the server shows no text for (`DROP PROCEDURE grp` takes every member). Each was
dropped and came back empty, as a comment or a bare `;`, or unsigned — every relationship of a
graph database gone, an encrypted procedure recoverable only from a backup — and the run
reported success. These are left out of every `DROP`, with or without the transaction, and
ahead of the drops `refuse_if_present_sql` writes one `IF OBJECT_ID(…) IS NOT NULL THROW` per
object, naming it and why, so a replay onto a database that holds one stops before it has
changed anything while a restore into an empty database still creates it. Left out of the
drops alone, its `CREATE` stopped the run further down with only "the name is taken". What the
server shows no text for at all — an encrypted routine, an encrypted view — has a comment for
its `CREATE`, so nothing collides: it is left as it is, and the header says so. The refused
list is `DumpPlan::refused`, named in the header and, through `refused_note`, in the modal's
report (`a_replay_does_not_drop_a_graph_table_and_stops_where_one_is_there`,
`a_replay_leaves_an_encrypted_routine_alone_and_refuses_one_it_cannot_restate`,
`a_replay_refuses_to_drop_a_table_whose_trigger_it_cannot_restate`; live,
`a_replay_refuses_to_replace_a_graph_table` and
`a_replay_drops_no_module_it_cannot_restate_whole`, and the round trip now drops the graph
tables itself before its replay).
**Nor a `DROP` for what the file only comments**: a PostgreSQL foreign table's structure step is
a comment, and so is a MariaDB sequence's whose definition was not read (`create_ddl` cannot
restate either), so `dump::file_creates` keeps both out of both drop sites. The foreign table's
`DROP TABLE IF EXISTS … CASCADE` once stayed, which on a target holding an ordinary table of
that name dropped it and its dependents with nothing put back
(`a_foreign_table_is_neither_dropped_nor_given_rows`,
`a_sequence_gets_neither_a_drop_nor_a_row_step`, which now holds the unread sequence).
**A MariaDB sequence that was read is created and its counter moved forward — never dropped,
never rewound.** `file_creates` is `true` for one with `TableInfo::sequence` filled; its
structure step is `create_ddl`'s `CREATE SEQUENCE` — `CREATE SEQUENCE IF NOT EXISTS` where the
file is one to replay (*Drop before create*), and **no `DROP` before it**, unlike a table's; and
its data step — when *Data* is on — is `TableInfo::sequence_restart_sql`'s `SELECT SETVAL(s,
<next>, 0)`, never an `INSERT` into its state row. A sequence's position is state no file
carries — the rule the other engines' sequences already follow — and the drop, recreate and
`RESTART WITH` this once wrote rewound a live sequence on a replay onto its source: the next
insert drew a value already used, for every table drawing from it, inside the file or not, and
the restart, DDL, committed even under a replay that then failed. `SETVAL` ignores a value below
the counter. Before any of this every sequence's step was the comment, so a dump restored no
MariaDB sequence at all (`a_read_sequence_is_created_and_its_counter_restored`; live,
`a_replay_restores_a_mariadb_sequence_and_its_position`, which advances the source past the
dump's position, replays onto it and asserts the next value is not reissued — 20 against the
unfixed file, 30 now — then drops both objects, replays, and asserts the next id is 20; green on
10.11 and 11.8).
**Where the file does not drop up front, what it carries beside the tables is written to be met
again** (`replay_create`, whenever *Drop before create* is on and `drops_up_front` is not): a
table's `DROP` takes no routine, type or sequence, so replaying a default dump onto its source
stopped at the first one still standing — *function … already exists* and *type … already
exists* on PostgreSQL 16, ERROR 1304 on MariaDB 10.11 and MySQL 8 (measured live, per the
change's author) — after every table ahead of it had been dropped and refilled. **A routine or
an event is replaced** (`ObjectItem::replacing_sql`): in place where
`ddl::replay_replaces_routine_in_place` says the server can — PostgreSQL's `CREATE OR REPLACE`,
which keeps a trigger or default outside the file that runs the function, where a `DROP` is
refused and `CASCADE` would take it, and **MariaDB's** `CREATE OR REPLACE FUNCTION`/`PROCEDURE`/
`EVENT`, asked of `DbSchema::flavour` — and otherwise by `DROP FUNCTION`/`PROCEDURE`/`EVENT IF
EXISTS` inside the same client script, just before the `CREATE`: MySQL 8's, the server without
the form, where nothing holds a routine by reference. **MariaDB is not left on the `DROP`
because the drop revokes grants**: with `automatic_sp_privileges` (the default) every account's
`EXECUTE` on the routine went with it, and an account that may not write the `DEFINER` had its
`CREATE` refused (ERROR 1227) after the `DROP` had committed, so the routine was gone — measured
on MariaDB 10.11.14, where a grant survives `CREATE OR REPLACE` and not `DROP` + `CREATE`
(`a_replay_keeps_the_grants_on_the_routines_it_replaces`, live on MariaDB and MySQL 8; the edit
path's `supports_or_replace_routine` still declines the form, for the reason its doc gives). On
MySQL 8 the header says what the `DROP` costs, and so does the toggle's hint
(`mariadb_replaces_routines_in_place_and_mysql_says_what_its_drop_costs`) — **and the file
grants back at its end what the `DROP` revoked**, under *-- Routine grants*, from what the dump
read of `mysql.procs_priv` (`core::access::RoutineGrant`;
`a_mysql_replay_grants_back_what_its_routine_drops_revoke`, and the live leg above, red on MySQL
8 with the read taken out). Where the dumping account could not read that table the header says
so instead. The `DEFINER` half is unchanged: an account that may not write it still has its
`CREATE` refused after the `DROP`, which the header says. Each is named in the
header's *this file replaces …* line, which says *events* or *routines and events* where there
are events among them, as the up-front path already named its routines
(`a_replay_replaces_each_routine_it_carries`). **An enum, a domain or a sequence is created only
where it is missing**, through `unless_present_sql` (`Present::Type`, `to_regtype`, for the two
types; `Present::Table`, `to_regclass`, for the sequence), and **never replaced** — bringing
them into line with the routines is the tidy to resist: dropping a type takes every column
typed with it, in the file or not, and a sequence's position is state no script carries, SQL
Server's rule for its own objects below. The trade is that one already there is left exactly as
it is, whatever its shape; and where the engine has no such guard (the MySQL family) the
`CREATE` is left as written (`a_replay_creates_a_type_or_sequence_only_where_it_is_missing`).
The live tier holds both on each `live_suite!` leg (`tests/live/dump_replay.rs`, sharing
`partitions.rs`' `dump_of` and `count`): `a_replay_replaces_the_routines_it_carries` replays a
dump carrying a function onto its source and then calls it, and
`a_replay_keeps_the_types_and_sequences_it_carries` replays one carrying an enum, a domain and a
sequence advanced three times, and asserts the sequence still answers 4 next (a no-op on the
MySQL legs, which have neither type).
`sequence_resync_sql` closes the PostgreSQL restore:
`dump_columns` carries a `serial` or `GENERATED BY DEFAULT AS IDENTITY` column
deliberately — someone
re-importing their own keys wants them — but an *explicit* insert does not advance the sequence
behind the column, so the restored table holds keys 1..10000 with its counter still at 1 and the
first ordinary insert afterwards is a duplicate-key error that repeats until the counter catches
up. It emits a `setval` per column, written so that a column with **no** sequence behind it is a
no-op rather than an error: `pg_get_serial_sequence` answers `NULL` there, and selecting through a
subquery with `WHERE … IS NOT NULL` means no row is produced and `setval` is never called — which
also covers an empty table. Live-verified, and `pg_dump` emits the same for the same reason.
MySQL's `AUTO_INCREMENT` and SQLite's `rowid` both advance from the data already in the table, so
the gate is `ddl::supports_sequence_resync` — an exhaustive `match` beside
`supports_column_reorder`, replacing the `dialect != SqlDialect::Postgres` this shipped as.
**`DumpPlan::missing` is the ticked tables the dump's own fresh introspection could not find** —
renamed, dropped or permission-revoked between the picker and the save dialog. The
re-introspection is deliberate (a backup of a shape the server no longer has is not a backup) and
this is its cost, so it is reported rather than silent: a file one table short of what was ticked
looks exactly like a complete one, and only the sibling *preselect* case was ever named.
**`file_plan` is the folder export beside `plan`, and deliberately not `plan` with its options
turned down.** The five non-SQL formats write one file per table, and the row step is where the
two genuinely differ: a dump's `SELECT` names its columns through `dump_columns`, which
leaves out everything the server assigns for itself, while a CSV of `orders` without `orders.id`
is not the table — so this reads `SELECT *`. Views are included for the mirror-image reason: a
view gets structure and no rows in a dump because an `INSERT` into one is not a restore, and a
CSV of a view is simply its rows. The two differ in exactly the place a shared step would have
hidden. Everything else is shared where it can be: the file names are
`export::export_file_names`, so the sanitizing and collision rules are stated once, and they are
taken from the **resolved** tables so the counter that breaks a collision is not spent on a table
that will never be written (`file_plan_breaks_a_file_name_collision`). `FilePlan::missing` is
`DumpPlan::missing`'s guarantee word for word, and `FileStep` keeps the table's namespace so the
writer can name the source the way the SQL export does. None of the ordering work above applies
to it — one file per table, and nothing in one file refers to anything in another.
**The folder export asks before it replaces, and the three functions that make it ask are
here.** A single-file export's consent is the save dialog's own "replace?";
`select_directories()` has none, and the per-table names are constructed by `file_plan` rather
than typed — so aiming an export at a project's `sql/` directory holding hand-written
`orders.sql` destroyed it with no prompt, no `.bak` and no undo. That is a gap against *a
destructive modal action guards its own launch, in the same step that launches it*, and the
collision list was already being computed ahead of the loop, with a comment saying why, and had
nowhere to go: `FilesOutcome` had no arm that could ask, so the list populated a post-mortem
field on three arms that could only report. `colliding_files(plan, exists)` is the census —
`exists` is an `exists` **predicate** rather than a filesystem, so it is unit-tested without
one, and the order follows the plan because that is the order the files would be replaced in;
**each planned file's `.part` sibling is censused with it**, because the export destroys that too
and this is the only guard in front of it: `write_one` opens the fragment through
`persist::create_part`, which removes whatever is there before it creates, and the failure path
removes it — so a fragment left by an earlier failed export,
the one file in that directory this app teaches the user to care about (`export_failure_note`
having just said *"the rows that were written are in `orders.csv.part`"*), was replaced by a retry
and swept by its cancel with no prompt naming it. The suffix comes from `export::part_path`, the
one function that decides it.
`folder_verdict(consented, colliding)` is the guard, separate from the census because the caller
needs the same list for its *report* whichever way the verdict goes and computing it twice is
how the two come to disagree. **`consented` is `Option<&[String]>` — the list the modal named,
not a `bool`** — and that is the property that makes this path satisfy *a destructive modal
action guards its own launch*: the approved re-launch re-censuses the folder and the verdict is
`Write` only while every colliding file is one the user said yes to, so anything that appeared
while the question stood — an editor autosaving, another export — is a fresh `Ask` rather than a
file destroyed under an older answer. As a `bool` it returned `Write` without looking at the new
census at all, which is strictly worse than not re-reading: it is the one place that *knows* the
consented set and the actual set differ. The set can move without anyone touching the folder,
too — `file_plan` re-resolves the chosen tables against a freshly fetched schema and the
collision counter is spent over the *resolved* names, so a table that disappears between the
question and the answer can shift a sibling from `orders_2.csv` onto `orders.csv`, a name the
prompt never contained. Consent covers exactly what it named; anything else is a new question.
And `folder_replace_prompt` writes the question, beside the three
report sentences and for the same reason — a message with arms (one file, a few, more than fit)
is a decision, and not one to make inside a callback the suite cannot reach. The modal no
longer shows this English: it words the same arguments itself
(`dump_view::folder_replace_prompt`), held equal to this by
`english_says_every_folder_outcome_as_core_does`, and
`FOLDER_REPLACE_SHOWN` — the eight it names before *and N more* — is `pub` so the two read one
constant.
`destroyed(colliding, published)` is the fourth and belongs to the *report* rather than the ask —
of the files the export was going to replace, the ones it actually has — and it counts a `.part`
as destroyed **with its published sibling**, since the rename that publishes `orders.csv` is what
consumes `orders.csv.part`; a fragment whose table the run never reached is still sitting there
and is not named. See `cli/dump.rs` for the two contradictory sentences that made it necessary.
**Why a dump or a folder export stopped short is a value, `ExportFailure`**, and `dump_verdict`
— the reader's and the writer's endings resolved into the one the user is told about, for both
drivers (`cli/dump.rs` has why cancel is the reader's to declare) — answers one: it builds
`WorkerDied` and `Failed` where it used to `format!` the sentence, and passes the writer's own
(`WriteEnd::Failed::message`) through. `DumpVerdict::Failed`, `DumpOutcome::Failed` and
`FilesOutcome::Failed` carry it where each held a `String`, so its tests compare typed values,
the one view that shows it words it (`dump_view`'s `export_failure_text`), and the CLI prints
its `Display` — the old English byte for byte (`every_export_failure_reads_as_it_did`). **Every
variant already says it failed**, so nothing that shows one adds a prefix: the folder export
once re-prefixed the writer's words into *Export failed: Export failed: No space left on
device*. `NotRenamed` — a dump's `.part` that could not be renamed over its destination — has no
*Export failed:* of its own and never had, its sentence (*The export wrote … but could not
rename it to …*) saying so itself. `Worded` is the one variant that is not a fact: a sentence
the interface has already written — the app's two refusals before a run begins, `dump-failed`
and `export-failed` when the connection cannot be had — passed through as it stands. The CLI
never builds one.
**`Listing`/`PickerBody`/`picker_body` are here for that same reason** — what the table picker
says when it has no rows is a decision, and it was a `bool` plus an `if` in the view that told a
failed read from an empty database wrongly. `dump_view.rs`'s entry has the bug in full.
**Ordering is a topological sort over `TableInfo::foreign_keys`** (`order_tables`): a referenced
table before the table referencing it, views last — a view's body selects from the tables above
it and it holds no rows to order against anything — MariaDB sequences first, by name, ahead of
every base table, since a table whose default calls `NEXTVAL(s)` is refused while `s` is missing
(ERROR 1146) and a sequence names nothing itself
(`sequences_come_before_the_tables_that_default_from_them`), and ties broken by name, so two
dumps of one schema are the same file. A self-reference is not an edge, being one table that can only be
created before itself. A real cycle is **reported, never dropped**: no creation order satisfies
one, so an edge is broken at the smallest name, every chosen table still reaches the file, and
`DumpPlan::cycles` is what puts the explanation in the header.
**A PostgreSQL partition is an edge too, and it widens the selection.** A partition waits on
the table it is a partition of (`TableInfo::is_partition_of`), in `order_tables`' base edges and
again in `creation_order`'s waits, since its `CREATE TABLE … PARTITION OF` names the parent —
sorted by name alone, a partition called `aa_2024` would be created ahead of `zz_events`,
which is how the tests name the pair. And
`order_tables` expands what was chosen before it sorts: a chosen table brings its partitions,
and theirs, because they hold its rows; a chosen partition brings the tables it is a partition
of, up to the root, because its `CREATE` names them — but **not its siblings**, which nothing in
it needs (`choosing_one_side_of_a_partition_brings_the_other`). **A parent brought that way is
named, never owned** (`named_parents`: each ancestor of a chosen partition that was neither
chosen nor itself a partition of a chosen table). It used to be dropped beside its `CREATE` like
any chosen table, and PostgreSQL drops a partitioned table's partitions with it — so a
one-partition dump replayed onto the database it came from destroyed every sibling partition and
its rows, and the file put back only the one it carried. Now `plan` puts a named parent in
`held`, out of both drop sites, and writes all of it through `unless_present_sql`: a
`DO $schemaic$ … IF <absent> THEN … END IF … $schemaic$;` block over the catalogue
(`to_regclass`, `pg_constraint`, `pg_trigger`, as `Present` says) around its `CREATE` and
indexes, and one more around each held check, trigger and foreign key, each looking for its own
name — `CREATE TABLE IF NOT EXISTS` covers the table alone, and every piece after it would stop a
replay at a name already taken. A constraint or trigger on a table that is not there counts as
present, there being nothing to put it on, and the dollar tag is one the body does not contain
(`a_guard_never_shares_its_dollar_quote_with_the_body`). **The chosen partition's own
`DROP … CASCADE` is the second half of the bug**: it takes a key from a table *outside* the file
onto the parent whole, not just that key's clone onto the partition — seen live, `orders` lost
its key onto `events` entirely, and nothing said so. So every such key is restated in the closing
constraints section, after the rows and so validated against them, and only where a guard
exists, since unguarded a replay that took nothing would stop at the name — the same
restatement, by the same loop, that a chosen table's own `CASCADE` needs (`drop_cascade`,
above); it was written for the named parent first, and a chosen table went without it until a
plain `customers` dump was replayed. The cascade guard (`cascade_guard_sql`, above) reaches that
key through the partition's clone and lets it pass on that restatement alone; anything else the
partition's `CASCADE` would take stops the replay before it drops anything, and the held parent,
being dropped by nothing, is no seed of the walk. **Leaving the parent standing has a third cost: its
triggers.** A partition created `PARTITION OF` a parent that exists takes a clone of each of the
parent's row triggers at that `CREATE`, so the trailing `-- Triggers` section, which keeps a
chosen table's own triggers off its rows, was no help — on a replay onto the source every
restored row fired the clones, and an `AFTER INSERT` audit trigger on the parent wrote a
duplicate audit row per restored row (measured live on PostgreSQL 16 by the change's author:
the audit table went 3 → 5 for 2 restored rows). This arrived with the fix above; while the
parent was dropped, its triggers were written after the rows like any chosen table's. So,
with Structure on, a partition any of whose ancestors is in `named_parents` has its rows
bracketed by `cloned_triggers_held_sql`: `ALTER TABLE <p> DISABLE TRIGGER USER;` before —
the owner may, where `ALL` would need a superuser for the internal key triggers `USER` leaves
alone — and after, a `DO` block that sets each clone back to **its parent trigger's own state**
through `tgparentid` (`D` → `DISABLE`, `R` → `ENABLE REPLICA`, `A` → `ENABLE ALWAYS`, otherwise
`ENABLE`). Not a blanket `ENABLE`: a clone takes its parent's state, so a trigger switched off
on the parent is off on the partition too, and `ENABLE` would have switched it on (verified
live). The partition's own triggers still come in the trailing section. Nothing is written
when the parent is chosen as well — it is recreated, and its triggers follow the rows anyway —
nor in a data-only file, where every table's existing triggers fire, consistently, nor on the other three engines (`None`), where a partition is no
table (`a_partitions_cloned_triggers_are_off_while_its_rows_go_in`; live,
`a_partition_replay_fires_no_parent_trigger_on_its_rows` in `tests/live/dump_replay.rs`,
which asserts no audit rows on the replay, the clones' states `O`/`D` afterwards, and that a
clone fires on the next insert). The header names the
parents and says the file never drops them and creates them only where missing. The trade is
that a parent already there is left exactly as it is, whatever shape it has
(`dumping_one_partition_drops_neither_its_parent_nor_its_siblings`,
`a_parent_brought_for_its_partition_restates_its_key_only_where_missing`,
`a_key_from_outside_onto_the_parent_is_put_back_where_missing`,
`every_ancestor_of_a_chosen_partition_is_only_named`); choosing the partitioned table itself
still replaces it whole, every partition being in the file then
(`dumping_a_partitioned_table_still_replaces_it`). `unless_present_sql` is `None` on the other
three engines, where a partition is no table (`partitions_are_tables`) and no dump has a named
parent to write. The comparison's `fk_rank`
passes every table on a side as chosen, so the widening adds nothing there. **A table whose
rows live in its partitions gets no data step** (`TableInfo::rows_live_in_partitions`): its
`SELECT` reads every partition's rows, so dumped beside them each row went back twice — once
routed through the parent, once into the partition directly — or was refused over a unique key
(`a_partition_follows_its_parent_and_the_parent_carries_no_rows`). **A key a partition
inherited is not restated** in the closing constraints section, the parent's own
`ADD CONSTRAINT` putting it back on every partition, while a second one of the same name fails
the restore (`a_partitions_inherited_key_is_not_restated`); a partition with only inherited
keys is skipped there like a view. Verified live on PostgreSQL 16 (per the change's author): a
fixture with range partitions across two schemas, a sub-partitioned partition, two `DEFAULT`
partitions, a parent index, a partition's own index and a foreign key, dumped as
`events` + `customers` and restored into a fresh database with the same per-partition row
counts (641/546/552/261), 13 indexes and 6 foreign-key constraints. The live tier now holds it
on each `live_suite!` leg (`tests/live/partitions.rs`): `a_partitioned_table_reads_back_partitioned`
over the catalogue reads — the key, a partition's bound, and `inherited` on the partition's
index and key and on `orders`' clones onto each partition, which a broken read loses with
nothing failing, the dump then writing the parent's rows twice and restating the clones;
`a_partitioned_dump_restores_with_every_partitions_rows`, into an empty database and replayed
onto itself, counting each partition's rows; and `a_dumped_partition_replays_beside_its_siblings`,
the bug above, replaying a one-partition dump onto its source and asserting the sibling's rows
and `orders`' key with both clones, then restoring it into an empty database with the parent
and its trigger (a no-op on the MySQL legs, where a partition is not a table to dump). SQL
Server's read is `mssql.rs`'s `a_partitioned_table_reads_back_partitioned`. **None of this touches
MySQL**, where a partition is no table: `partition_of` stays `None`, so nothing waits on
anything, and `rows_live_in_partitions` answers no, so the partitioned table keeps its data
step — its partitions coming back inside its own `CREATE TABLE`, through
`Partitioning::clause_sql` (under `schema.rs`). Verified live on MariaDB 10.11 (per the
change's author): a database of `RANGE`, `LIST COLUMNS`, `HASH`, `KEY`, `RANGE COLUMNS` and
`RANGE` + `SUBPARTITION BY HASH` tables, dumped through the CLI's `dump::run` and restored into
a fresh database, listed an identical `information_schema.PARTITIONS` (an md5 of every
partition and subpartition row equal) with identical row counts.
**The views are then sorted among themselves, by the same walk.** "Views last" ordered them
against the base tables and left them in name order against each other, and a view built on
another view is `CREATE VIEW … FROM other_view` where `other_view` does not exist yet — MySQL
1146, *after* the file's own `DROP VIEW IF EXISTS` has already removed it from the target. The
dependency walk is built from `foreign_keys` and a view has none, so nothing ordered them at all.
A view's edges are the other picked views its body names, matched as **whole words in code**
(`intel::code_word_hits_in`) so a name inside a comment, a string literal or a longer identifier
is not an edge. Both halves share `topo_order`, which takes its members already in name order so
that "the first ready one" *is* the tie-break and two dumps stay byte-identical. The mask is
built **once per definition** (`intel::code_mask`, split out of `code_word_hits` for this
caller): the question is every picked view's name against every other's definition, V² questions
over V definitions, and folding the lex back into the search re-lexes each definition V times.
**`order_by_mention` is the third instance of that same edge — "waits for what it names".**
Base tables are ordered by their foreign keys and views by the views their bodies name; a
routine calling another routine, and a domain built on another domain, had nothing, because
`objects_where` is a `filter().cloned()` over the catalogue vector with no sort and no walk. With
`check_function_bodies` on — PostgreSQL's default — a `LANGUAGE sql` `CREATE FUNCTION a_total()
… SELECT b_base()` ahead of `b_base` stops the restore, and by then the file's `DROP TABLE`s have
run against the target. Routines are now ordered against each other and so is the `objects` list,
through the same `topo_order`, with input order (already kind-then-catalogue) as the tie-break so
two dumps stay byte-identical, and a cycle broken rather than dropped because mutually recursive
routines are legal and the file must still hold both. **The text it searches is the body, not the
`CREATE` that carries it**, and that is the part a tidy would get wrong: a PostgreSQL function's
`CREATE` wraps its body in `$$ … $$`, which is exactly what `intel::code_mask` marks as *not*
code — so asking the emitted statement finds nothing, every time, and the walk would be a no-op
that looked like a fix. **`creation_order` is the fourth, and the edge the other way.**
Routines go after the tables because a function reads them, but a table or view can call a
function too, and SQL Server resolves one at `CREATE TABLE`/`CREATE VIEW` time (PostgreSQL does
for a default and a view): a computed column, a check, a default or a view calling a function
the file created later stopped the restore (Msg 4121). So a function named by a table's
expressions (generated, default, check — not its `CREATE`, where a column named like a function
is no call) or a view's definition, and every function such a one calls (`routine_slots` picks
them), is written just ahead of its first caller and after the last table or view it names
itself. **Tables, views and those functions are sorted as one graph**, not a function fitted
into a table order fixed without it: that order took the caller's side wherever the function's
two edges disagreed with it, so a view reaching another view only through an inline function
came out ahead of it, and a table whose check calls a function counting another table had its
rows refused before that table existed (Msg 208 both, on 2022 and 2025). Now the caller waits.
`order_tables`' answer is the tie-break — the first ready table or view in it goes next, its
own edges kept only as it already satisfies them, so a cycle it broke stays broken the same
way — and a function goes as soon as everything it names is there, so where no function is
called the file is byte-identical. Only a real cycle, a function reading a table whose own
column calls it, is broken, at the function, since deferred name resolution lets it stand
first. Every other routine stays in the trailing section
(`a_function_a_table_or_view_calls_is_created_before_it`, SQL Server and PostgreSQL;
`a_view_reached_through_a_function_comes_after_what_the_function_reads`,
`a_table_whose_check_calls_a_function_comes_after_what_the_function_reads`,
`the_one_sort_keeps_the_old_order_and_breaks_a_real_cycle_at_the_function`; the live round trip
carries a table whose computed column, check and default call a function, and a view that does,
and `a_dump_orders_tables_and_views_through_the_functions_they_call` both chains, restored and
replayed). The standalone-objects section
covers only the namespaces the chosen tables live in — a dump of `sales` has no business
recreating `archive`'s types — and skips `ObjectItem::is_internal`, since a `serial`'s own
sequence is created by the column's definition and restating it fails the load on a name that
already exists. It also drops a sequence **owned by one of the chosen tables**, which
`is_internal` cannot answer on its own: a catalogue can report the link as external while the
column still owns the counter, the same question `DbSchema::create_ddl_script` asks and the two
scripts must not disagree about. The owner comparison is on **`(namespace, name)`**, not the name
— `create_ddl_script` gets away with a name because it works inside one namespace, while a
selection spans them, so `sales.orders_id_seq` owned by the unexported `sales.orders` was being
dropped on the strength of a chosen `public.orders`, leaving the column that defaults to it with
nothing behind it (`a_sequence_owned_by_a_same_named_table_in_another_namespace_is_kept`). A
sequence carries its own namespace and its owner is in that namespace, which is what makes the
pair available to compare. It is on by default because a file without it fails on the first
column typed as one of the database's enums.
**SQL Server's standalone objects are `DbSchema::tsql_objects`, not PostgreSQL's lists** —
its sequences, alias types, XML schema collections and synonyms (`TsqlObject`, read by
`db::mssql`'s `SEQUENCE_LISTING`, `ALIAS_TYPE_LISTING`, `XML_COLLECTION_LISTING` and
`SYNONYM_LISTING`; a typed `xml` column's type names its collection, `xml([dbo].[coll])`).
A synonym's target keeps `PARSENAME`'s parts in their places (`TsqlObject::synonym_target`):
`db..t` has no schema part, and dropping it wrote `[db].[t]`, schema `db` in the restoring
database, so the copy pointed at nothing or at the wrong object; an empty part between two
others is written empty (`a_synonyms_missing_middle_part_stays_in_its_place`; live,
`a_dump_keeps_a_synonyms_empty_schema_part`). An alias type's bound default and rule
(`sp_bindefault`/`sp_bindrule`, read by `ALIAS_TYPE_LISTING` into
`DbSchema::tsql_type_bindings`) are not restated — deprecated, and no `CREATE TYPE` carries
them — so the header names each alias type the file creates that has one, since the restored
columns of it silently lost their default and their check
(`an_alias_types_bound_default_and_rule_are_named_in_the_header`; live,
`a_dump_names_an_alias_types_bound_default_and_rule`). **One bound to a column is named by the
table's own script**: `COLUMN_BINDING_LISTING` reads it into `TableInfo::tsql_bindings` —
there, not on `ColumnInfo`, whose equality the differs compare, and a binding no script can
restate is no difference a plan could act on — and `TableInfo::create_ddl` writes a `--`
note per column ahead of the `CREATE TABLE`, so Copy DDL and the dump both say it
(`create_ddl_sql_server_names_a_columns_bound_default_and_rule`; live,
`a_columns_bound_default_and_rule_are_named_in_its_script`). A binding the column has from its
alias type is left out: a column of a bound type carries the type's object in
`default_object_id` too (measured), and binding the type again rebinds it, which the header
already says.
None was read, so a table with a `NEXT VALUE FOR` default or an alias-typed column stopped the
restore at its `CREATE TABLE` (Msg 208/2715) and the header said nothing. They are not put in
`sequences`/`domains` because those have PostgreSQL's emitters, editor and compare behind them
— a SQL Server sequence there was scripted with `OWNED BY` and offered an editor that applies
nothing. The dump is no longer this list's only reader: the schema tree browses it through
`ObjectItem::Tsql` (under `schema.rs`), and `objects_all(Sequence)` now returns a T-SQL
sequence after PostgreSQL's. So both of this module's object-kind loops skip
`ObjectItem::tsql()` — `outside_dependencies`' (PostgreSQL's question; a SQL Server object an
outside table names is `tsql_named`'s, asked of the T-SQL pass) and the `other_objects` kinds
loop — or the first would ask PostgreSQL's question of a SQL Server sequence, and the second
would write one a second time, outside the order the T-SQL pass gives it. The T-SQL pass still
owns all four kinds, and the file is unchanged.
The dump writes them in the types
section in the order one can name another (collections, alias types, sequences, synonyms) and
moves each sequence's counter on to the source's through `sp_sequence_get_range` beside its
`CREATE`, since `ALTER SEQUENCE … RESTART WITH` rewrites `start_value` (measured) and the rows
carry their own values. **Only an object a chosen table names, in an exported namespace, and
not a sequence is replaced on a replay** — dropped after the routines where the file drops up
front and created plainly. Everything else is created under `IF … IS NULL EXEC(…)` with any
counter move inside the same `EXEC`, and never dropped: every object in the namespace used to
be, so replaying an older one-table dump rewound a sequence other tables draw from and their
next insert took a key already used (Msg 2627, measured), and one an unexported table's
default names refused its drop (Msg 3729). A sequence's position is state no script restates,
so it is left where it stands even when a chosen table names it
(`a_replay_never_drops_a_sequence_or_an_object_its_tables_do_not_name`; live,
`a_replay_leaves_a_sequence_where_it_stands`). **One
outside the export's namespaces that a chosen table names is carried too** — WideWorldImporters
keeps every key's sequence in a schema with no table in it, so the namespace rule alone left
them all out — but it is not the file's to own: its schema is made if missing, it is created
under `IF … IS NULL EXEC(…)` with its counter moved inside the same `EXEC`, so a replay that
finds it does not move it twice, and it is never dropped. Without `other_objects` such an
object is named by `outside_dependencies`, which reads a T-SQL column with its brackets taken
off (`a_sql_server_dump_carries_the_objects_its_tables_name`,
`a_sql_server_object_is_created_dropped_and_restarted_in_t_sql`; the live round trip carries a
sequence in a table-less schema behind one default and one in `dbo` behind another, an alias
type, a schema collection behind a typed `xml` column and a synonym a view reads through, and
compares each counter's next value on the copy and again after the replay). **So is one only a
routine the file creates names** (`tsql_named_by_routines`): the routines come with their
namespace whatever the tables name, and a `dbo` alias type that only an `s3` procedure's
parameter used was neither carried nor named, so the restore stopped at the procedure (Msg
2715). A parameter's or the return's type counts, bare or qualified, and the body by qualified
name only — bare, a name there is far more often a column or a variable, and one wrongly
carried makes a schema the restore did not need
(`an_object_only_a_dumped_routine_names_is_carried`; live,
`a_dump_carries_a_type_only_its_routines_name`).
**What that namespace filter leaves out is now accounted for in the header** (`outside_dependencies`):
the types and sequences the chosen tables' columns name that this file will not create, as
display names, in the order they would have been emitted. Emitting only the chosen namespaces'
objects is a choice about what the file holds, not a licence to say nothing about the rest — a
`public` enum used by a `sales` column is simply absent, the `CREATE TABLE` still declares the
column with it, and on a fresh server the restore stops before any data lands. It is the same
accounting `dropped_fks` already had and strictly louder, since a missing type is a
`CREATE TABLE` that cannot run at all. A column names such an object in one of two places and
both are read as text, because that is what the catalogue hands back: the declared type
(`order_status`, or `public.order_status` where the search path does not cover it) and the
default expression (`nextval('public.order_seq'::regclass)`). Matched as a whole identifier so
`order_status_v2` is not a hit — and deliberately **not** through `intel::code_word_hits_in`,
because a sequence's name lives inside a string literal in that default, which is precisely what
a code mask hides. An internal sequence is skipped: it comes back with its column, so it is not
missing. **So is one this file emits a local twin of** — an unqualified mention resolves through
the search path, so a `sales` table whose column is typed `status` was reporting `archive.status`
missing while the file itself emits `CREATE TYPE sales.status`. The suppression is per *kind*,
never across all three: a sequence named `status` must not silence a real dependency on an enum
of that name, those being separate catalogues in PostgreSQL. A **qualified** mention is
definitive and suppresses nothing — `archive.status`, or a `nextval('archive.status_seq')`, names
that object and no other whatever else the file creates.
The names are qualified unconditionally, unlike `display_name`, since *where* the
missing object lives is the whole point of the sentence and `public` — the namespace
`display_name` drops as the default — is the case this is usually about.
**That block is gated on `opts.structure`, because the sentence is about the structure section.**
It says *the `CREATE TABLE` statements name it*, and a data-only file has none — it is `INSERT`s,
and a data-only restore into an existing database needs the type no more and no less than the
target already has — so the user was being sent to satisfy a dependency the file does not have.
Its sibling one block up is gated by construction, `dropped_fks` only being incremented inside
the structure step; this census is computed above that step and had no such arithmetic in front
of it.
Pure + unit-tested; extend the tests that assert on the whole file
(`file_of`) rather than on one string, since ordering across the two step kinds is where this
module's real bugs live.
