# `core/schema.rs`

`schema.rs` — the introspected model. `ColumnInfo` carries the **full** column definition
(type *with* parameters, default as ready-to-emit SQL text, auto-increment/identity, generated
expression, `ON UPDATE`, comment, collation) because MySQL's `MODIFY COLUMN` replaces a column
outright — anything not restated is silently destroyed, so a schema editor can't stand on a
thinner model. `ColumnInfo::definition_sql` is that one emitter, shared by `CREATE` and (later)
`MODIFY` so they can't drift.
**` STORED` is written on every engine, and MySQL was missing from that line while its own
reader had always set the flag.** `generated_stored` comes off `EXTRA`'s `stored generated`, and
`definition_sql` wrote the keyword for PostgreSQL and SQLite only — so on the busiest engine a
dump, Copy DDL, Duplicate table and a designer `ADD COLUMN` all emitted the keywordless form and
the restored column came back `VIRTUAL`, recomputed on every read instead of materialised, with
nothing said; and where the column is in the primary key the restore does not differ, it
**fails**, after the file's earlier statements have run. Measured on MySQL 8.4.11 and MariaDB
10.11.14: the keywordless form reports `VIRTUAL GENERATED` on both, both accept an explicit
`STORED` on `CREATE TABLE` and on `ADD COLUMN`, and a virtual generated column in the primary key
is refused (*ERROR 3106* / *ERROR 1903*). The `false` case stays unwritten because `VIRTUAL` is
every engine's default, so omitting it round-trips and writing it would rewrite the DDL of every
existing virtual column (`a_stored_generated_column_keeps_its_keyword_on_every_engine`).
`identity_always` separates PostgreSQL's `GENERATED ALWAYS AS
IDENTITY` from `BY DEFAULT`/`serial`/MySQL `AUTO_INCREMENT`, because only the first **rejects**
an explicit value — `is_server_assigned()` is that question (`generated.is_some() ||
identity_always`) and is what a write path must ask before naming a column.
**On SQL Server `definition_sql` hands off to `tsql_definition` first**, T-SQL's column differing
from the shape the other arms share at most clauses. A computed column has no type —
`[c] AS (expr)`, `PERSISTED` for a stored one and `NOT NULL` only then, the server refusing it on
a column computed on read; an identity is `IDENTITY(seed,increment)` and always `NOT NULL`,
whatever the draft says; and **a nullable column is written `NULL` out loud**, because a new column's
nullability otherwise follows the session's `ANSI_NULL_DFLT_*` settings and the same statement
could make a different column from another client. `DEFAULT` goes through `is_bare_default`;
there is no `ON UPDATE`, inline comment or `INVISIBLE`, T-SQL having none of them.
**The seed and increment are `ColumnInfo::identity_spec`**, the pair `db::mssql` reads from
`sys.identity_columns` — `None` for a column the designer is making, which gets `(1,1)`, and on
every other engine. Without it every recreate wrote `(1,1)`, renumbering a table whose ids start
at 1000 or count down. The pair is spliced into the DDL, so `mssql::identity_spec` keeps it only
as integer text and drops anything else back to `(1,1)` and the comment that says so
(`an_identity_spec_is_kept_only_as_integer_text`); `tsql_identity` is the one place both
emitters write it (`create_ddl_sql_server_restates_the_identity_it_read`).
**`ColumnInfo::invisible` is `index_disabled_sql`'s argument one object down**: MySQL 8.0.23+
and MariaDB 10.3+ mark a column `INVISIBLE`, which leaves it selectable by name and out of
`SELECT *`, and that is the standard way to retire a column without breaking an application —
a decision a DBA *stages*. With no field for it every emitter restated the column visible, so a
dump-and-restore or a Copy DDL brought the retirement back undone: `SELECT *` starts returning
the column again and every query written against the visible column set gets an extra one, with
nothing said. It is read from `EXTRA` in `db/lib.rs`'s `mysql_column` — the **fourth** question
that column answers, beside `auto_increment`, `on update current_timestamp` and
`stored generated`, and the one that was not being asked — and `definition_sql` writes
` INVISIBLE` **last** and on the MySQL family only, since both servers print it at the end of
the definition and neither PostgreSQL nor SQLite has the concept (`false` on both). The
difference from a *switched-off index*, which is answered by marking it `IndexInfo::lossy`, is
that the model cannot restate the index and can restate the column — so this is a real field and
the emitter writes the keyword. `IndexColumn` keeps prefix lengths + `DESC`; `ForeignKeyInfo`
keeps its name (both engines drop by name), its referential actions, and **two PostgreSQL
attributes that were being dropped in silence**. `match_type` is `MATCH FULL` / `MATCH PARTIAL`,
`None` meaning the unwritten `MATCH SIMPLE` so an untouched key round-trips exactly, and it is
the difference between refusing and accepting a **partially NULL** composite key: under `SIMPLE`
a row with any NULL in the key passes the constraint, under `FULL` only an all-NULL one does, so
recreating a `FULL` key as `SIMPLE` widens what the table accepts and says nothing. `deferrable`
is `DEFERRABLE INITIALLY DEFERRED` / `… INITIALLY IMMEDIATE` held whole, `None` being
`NOT DEFERRABLE`, and it changes what an application can *do*: `INITIALLY DEFERRED` is what lets
a transaction insert children before parents and have the key checked at commit, so a copy
restored without it refuses those inserts at statement time and the application fails against a
database that "restored fine". That this matters was already settled here —
`ddl::unrestatable_sqlite_clauses` names `SqliteClause::Deferrable` (*"a foreign key's
DEFERRABLE clause"*) and **withholds**
the SQLite rebuild rather than drop it — so until now the same attribute was a refusal on one
engine and a silent drop on another, because the model had no field for either side to read.
`pg.rs` reads both off `con.confmatchtype` / `con.condeferrable` + `con.condeferred` through
`fk_match` and `fk_deferrable` (`condeferred` without `condeferrable` cannot happen and is read
as the default rather than invented into a clause), and `ddl::fk_clause` emits them in the
**grammar's** order — `MATCH`, then the actions, then `DEFERRABLE` — which is the language's and
not a layout preference. Both stay `None` on the other two engines and nothing about them
changes: MySQL parses `MATCH` and ignores it, and SQLite honours no `MATCH` while
`pragma foreign_key_list` reports no deferrability at all — so the model says nothing rather than
guessing, and a SQLite key declared `DEFERRABLE` goes on being refused a rebuild by
`unrestatable_sqlite_clauses`, which reads the declaration *text* and is the one place that can
see it. `ViewOptions` is the same
idea for a view (check option, MySQL definer/security/algorithm, PG storage params +
`materialized`) — `CREATE OR REPLACE VIEW` replaces the whole view, so what isn't restated
resets, and `SQL SECURITY DEFINER → INVOKER` is a privilege change. `definer_sql` quotes the
two halves of a MySQL account. **`column_list` is SQLite's and SQL Server's, and arrives by the
opposite route**: the explicit `(x, y)` of `CREATE VIEW v (x, y) AS …`, held verbatim and
without its parentheses, `None` on the two engines that bake the names into the body they report
(`ddl::view_keeps_column_list` is that split). It has to be modelled because *every* SQLite view
edit is a drop-and-re-create (`ddl::supports_or_replace_view`), and T-SQL's `ALTER VIEW` resets
what it is not told, so a list left behind would silently rename the view's columns to whatever
the body calls them; verbatim rather than a parsed `Vec<String>` because SQLite hands the list
back with whatever quoting it was written with and re-quoting it is a way to change it.
**`attributes` is SQL Server's alone** — `SCHEMABINDING` and `VIEW_METADATA`, upper-cased in
the header's order, empty elsewhere — restated for the same `ALTER VIEW` reason: a schema-bound
view altered without the word comes back unbound. Restating it does **not** keep the view's
indexes — `ALTER VIEW` drops every one either way (measured on 2022 and 2025), where this line
used to say the binding was what kept them; `TsqlView::indexes` is what puts them back.
`ENCRYPTION` never appears, an encrypted view having no readable definition to edit. `db::mssql` reads both
off the stored definition's header, through `ddl::tsql_view_parts`. **`tsql: TsqlView` says
whether it could** — `verbatim` for a header the walk could not read, `hidden` for a view the
server shows no text for, default on every other engine — and either leaves the view listed
and droppable but not editable (`ddl::view_is_editable`), the call `TsqlTrigger`'s fields of
the same names make for a trigger. **Its `indexes` are an indexed view's** — the unique
clustered one that materialises it and any nonclustered ones on it — moved there off
`TableInfo::indexes` by `db::mssql`, because that field is a table's and what the grid, the
designer and the comparison read, none of which should see a change; they are held only to be
built again after an edit and in the view's script (under `ddl.rs`). **`TsqlModule` is what every SQL Server module carries beyond
its own parts** — one struct, as `module` on `TsqlView`, `TsqlTrigger` and `TsqlRoutine` alike,
because what a rebuild loses is `sys.sql_modules`' text rather than anything about the object's
kind. Its field `header_comments` is the stored statement's text before `CREATE`, verbatim —
SSMS's template Author/Description block is the usual one — then each comment between the
header's parts (`AFTER INSERT /* why */ AS`), one to a line; a comment inside a part kept as
text (a parameter list, a column list, a `RETURNS`) stays in that part. `restate` puts them in
front of a rebuilt statement on lines of their own, and is the one place they are restated:
`TriggerInfo::tsql_statement`, `RoutineInfo::tsql_create_sql` and `ddl::create_view_sql` (as
its last step, a no-op on every other engine) all end in it. They were dropped by every edit —
the header is rebuilt from parts and the editor's body box starts after `AS` — and, once the
trigger's and routine's Copy DDL and dump moved onto the rebuild, by every script of those too:
the object's only documentation gone, with nothing on screen to say so. It sits inside the
parts the round-trip gate compares — `TsqlRoutine`'s hand-written `PartialEq` compares `module`
whole, so a field added to it is compared without a second edit there
(`a_triggers_header_comments_survive_its_rebuild` and its routine and view siblings, under
`ddl.rs`; live, `a_modules_header_comments_survive_an_edit` on 2022 and 2025).
**Its two flags are the settings the module was created under** — `ansi_nulls_off` and
`quoted_identifier_off`, `sys.sql_modules.uses_ansi_nulls`/`uses_quoted_identifier` read as 0,
and `false`, the ANSI default, for anything not read. `CREATE` has no clause for either: a
module takes the session's setting when it is created, and Schemaic's session is an
ANSI-defaults one, so an edit re-filed a module written OFF under ON and its results changed
with nothing on screen to say so — measured on 2022 and 2025, a view created under
`ANSI_NULLS OFF` filtering `WHERE d = NULL` returned one row before an edit and none after, and
a `QUOTED_IDENTIFIER OFF` trigger's `"x"` literal became a column. Nothing read the two columns.
`ddl::tsql_settings_wrapped` restates them, the counterpart of MySQL's `sql_mode` wrapper
(both under `ddl.rs`).
**`signed` is the one loss that cannot be restated**: `ADD SIGNATURE` leaves a row in
`sys.crypt_properties`, and any `CREATE OR ALTER` — every edit of a procedure, function or
trigger here — drops it (measured on SQL Server 2022), while signing again needs the
certificate's private key. Module signing is how a module is granted rights its callers lack, so
the edit silently broke every call that relied on it, and the preview said only "Redefines p".
`db::mssql`'s trigger and routine listings read it (`EXISTS` over `sys.crypt_properties`,
`class = 1`), and `ddl::Risk::SignatureLost` puts a risk sentence under the `ReplaceRoutine` and
`ReplaceTrigger` arms, in place or recreated
(`editing_a_signed_routine_says_it_strips_the_signature` and its trigger sibling; live,
`a_signed_modules_edit_says_it_strips_the_signature` on 2022 and 2025, which also finds the
signature gone after the edit it warned of). A view cannot be signed. **It is not part of the
module's equality** (`TsqlModule`'s hand-written `PartialEq`, destructured so a new field must
be placed): a certificate is one database's, so the comparison reported a signed module whose
text matched the other side's as Differing and planned an alter that strips the target's
signature and cannot sign — a difference no Apply resolved. The risk reads the **server's**
flag: `ReplaceRoutine::server`'s, and `ReplaceTrigger::server_signed`, since the trigger arm read
the draft's, which in a comparison is the source's
(`a_signature_alone_is_not_a_difference`, `an_alter_names_the_targets_signature_it_strips`,
under `compare.rs`; live, `a_comparison_leaves_a_signature_out_of_the_difference`).
**Copy DDL and the dump say so of a signed routine**: `ObjectItem::create_sql` writes `-- NOTE: … is signed, and the
signature is not in this script` ahead of its `CREATE`, a batch of its own, since a comment in
the `CREATE`'s batch would be stored as part of the module
(`copy_ddl_of_a_signed_routine_says_the_signature_is_not_in_it`; live, in
`a_replay_drops_no_module_it_cannot_restate_whole`). **The dump says the same of a signed
trigger**: its script is `TriggerInfo::create_set_sql`, not `ObjectItem::create_sql`, so the
note there is a statement of its own ahead of the trigger's settings, which `client_script`
gives a batch apart (`a_signed_triggers_script_says_the_signature_is_not_in_it`; live, the same
leg).
`TableInfo::create_ddl` — `CREATE TABLE`/`VIEW`, built on the
above; its **view** branch delegates to `ddl::view_ddl` so Copy DDL, the MCP table-info tool
and the apply path all emit through one view emitter (it used to have its own, which restated
none of the options). **`TableInfo::implicit_key` is a capability, read rather than
asked-by-engine**: the spelling of a row identity the table has that is none of its columns
(SQLite's `rowid`), or `None` — which is every MySQL and PostgreSQL table, every view, and every
SQLite `WITHOUT ROWID` table. `filter::table_query` reads it to decide whether to project it, so
no caller has to test which engine it is holding.
**`TableInfo::shape()` is the question with three answers, and `is_view` is the one with two.**
`information_schema.TABLE_TYPE` gives three, and MariaDB stores a `CREATE SEQUENCE` as a one-row
table listed beside the real ones — so `is_view` put it on the *not a view, therefore an
ordinary table* side and it was offered every table action there is: it arrived with eight
internal counter columns (`next_not_cached_value`, `minimum_value`, …), sat in the Tables
folder, opened in the designer offering `ALTER TABLE`, and dumped as `CREATE TABLE` **without**
the `SEQUENCE=1` option that is what makes it one — so restoring the dump gave a plain table and
every `NEXTVAL(sq1)` in the restored schema failed (measured on MariaDB 10.11.14). Passing
`is_view = true` for one would be no better: it is not a view either, and the view-only entries
(triggers, refresh, *Edit view*) would light up instead. `TableShape::{Table, View, Sequence}`
makes it a case somebody has to answer, and the consumers are the places that decide what an
object *is*: `dump.rs`'s `DROP`-and-data arms and `ui/overlays.rs`'s menus, then
`propose::resolve_target` (the one gate whose whole job is refusing a non-table),
`erd::build_graph`'s `drawable` filter, `stats::drop_warning`, and `create_ddl_script`'s
partition below. `is_view` and `is_sequence` stay public because the three backends fill them
directly and privatising them needs a constructor first; what they are not is the question a
decision asks.
**`TableInfo::sequence` is what lets a MariaDB sequence be restated** — a `SequenceDef {
options, next }` that `db::mysql`'s `read_sequences` fills, `None` on everything else and on a
sequence it could not read. `create_ddl` emits `CREATE SEQUENCE <qname> <options>;` from it, and
keeps the comment-only note — reworded to *a sequence whose definition could not be read* —
only where it is `None`; inventing parameters there would be worse than saying so. **The
options are the server's text, not numbers re-spelt**: `SequenceDef::from_show_create` splits
`SHOW CREATE SEQUENCE` at the backticked name (a doubled backtick inside it allowed) and keeps
the tail verbatim, because 11.5 and later write an `as <type>` whose unsigned range no `i64`
holds. That tail reaches a script as SQL, so it admits only `[A-Za-z0-9 _=+-]` and no `--`, and
refuses anything else whole (`show_create_sequence_is_split_at_the_name`,
`a_definition_that_could_carry_sql_is_refused`); a `next` that is not an integer is dropped
rather than written. `next` is `next_not_cached_value`, a live value and not definition, so
`create_ddl` never writes it: `sequence_restart_sql(dialect)` does, as `SELECT SETVAL(q,
<next>, 0);` — forward only, `SETVAL` answering `NULL` and moving nothing for a value below the
counter (measured on 10.11.14 and 11.8.9) — or `None` without both a definition and a position
(`a_read_sequence_is_restated_with_the_servers_options`,
`no_restart_without_a_position_or_a_definition`). **The not-cached value is the point, not an
approximation**: it skips whatever the source had cached and not handed out, as a server
restart does, so no value is ever issued twice. It moves past `maxvalue` as readily, so an
exhausted `nocycle` sequence restores exhausted (ERROR 4084 at the next value, measured). What
it does **not** restore is the cycle count.
**`TableShape::ForeignTable` is the fourth answer, and it arrived the same way the third did.**
PostgreSQL's `table_list_sql` already listed `relkind = 'f'` but typed it `BASE TABLE`, so a
foreign table was indistinguishable from a table: it was offered Edit table, Truncate, Import
and triggers, and its Drop emitted `DROP TABLE`, which PostgreSQL 16 refuses (*"is not a
table"*, hinting `DROP FOREIGN TABLE`). Measured on 16 over `file_fdw`: `ALTER TABLE`'s rename
and `ADD COLUMN` and `COMMENT ON TABLE` succeed on one, while `DROP TABLE`, `TRUNCATE` and
`CREATE INDEX` are refused. `TableInfo::is_foreign` carries it (false on every other engine),
and `shape()` answers it after view and sequence. `create_ddl` for one is a comment-only note
with the name comment-safed, as an unread MariaDB sequence's is, rather than a `CREATE TABLE`:
the model holds the columns and not the server or options, and restored, a `CREATE TABLE` made
an empty local table under the name where the wrapper's rows had been
(`a_foreign_table_is_its_own_shape`, `a_foreign_tables_script_is_a_note_not_a_create_table`).
The consumers follow under their own modules: its drop is `ddl::Change::DropForeignTable`
(`stats::drop_change`), the comparison keys it apart and plans nothing for it, the diagram
draws it as a table, and the tree and the palette give it `schema_tree::table_glyph`'s
`EXTERNAL_LINK`.
**A partition is not a fifth shape: it is a table, on the one engine where it is a table.**
`TableInfo::partitioning` is how a table is partitioned — `Partitioning { key, partitions,
subpartition }`, the key as the engine prints it (`RANGE (at)`, `pg_get_partkeydef`'s text)
and each `Partition { name, schema, bound }`, its namespace kept because a PostgreSQL partition may live
in another than its parent's — and `TableInfo::partition_of` is the other end, `PartitionOf {
schema, parent, bound, own_columns }`; a sub-partitioned partition has both. `own_columns`
names the partition's columns whose default or `NOT NULL` is its own rather than its
parent's, which `mark_partitions_own_columns` fills after the full read (under `pg.rs`) by
comparing each partition's columns with its parent's — never a generated or identity column,
and nothing where the parent was not read
(`a_partitions_own_column_options_are_marked_against_its_parent`). It is not part of which
table a partition is: `PartitionOf::same_place` compares the other three, and the comparison's
*repartitioned* test asks it, since a differing default is an ordinary column difference. `partitions_are_tables(dialect)`
is the capability the two are read through, `true` on PostgreSQL alone: there a partition is
listed, sized, queried and dumped as a table, holding the rows while its parent holds none,
whereas on MySQL and SQL Server a partition is storage inside its table and the table holds
every row. `TableInfo::rows_live_in_partitions` asks it of one table (partitioned, on such an
engine) and `is_partition_of(parent)` matches `partition_of` against a table by name and
namespace; the dump is their reader (under `dump.rs`). **PostgreSQL, MySQL and SQL Server fill
`partitioning`, and PostgreSQL alone `partition_of`** — `pg_fold_partitions`,
`mysql_fold_partitions` and `mssql_partitioning` (under `pg.rs`, `mysql.rs` and `mssql.rs`).
On MySQL a `Partition` is `p0` with
`VALUES LESS THAN (2024)`, `VALUES IN (…)` or, for `HASH`/`KEY`, no bound at all, its `schema`
`None`; `subpartition` is MySQL's clause after `SUBPARTITION BY`
(`` HASH (`id`) SUBPARTITIONS 2 ``), the subpartitions' own names not kept, so restated they
take the engine's defaults.
On SQL Server the key is `RANGE RIGHT ([at]) ON [ps_year]` — the function's side, the
partitioning column and the scheme — and the partitions are named by number, `1` to `n + 1`,
as the server names them, each bound from `tsql_partition_bounds(range_right, boundaries)`:
`RANGE RIGHT` puts a boundary value in the partition to its right, so `< b1`,
`>= b1 AND < b2`, …, `>= bn`, and `RANGE LEFT` in the one to its left, `<= b1`,
`> b1 AND <= b2`, …, `> bn`; `n` boundaries make `n + 1` partitions, and none makes one partition
with an empty bound (`tsql_partition_bounds_follow_the_range_side`).
**`restates_partitioning(dialect)` is whether a `CREATE TABLE` carries the partitioning** —
`true` on PostgreSQL and MySQL, where it is a clause of the statement; `false` on SQL Server,
where a table is partitioned onto a partition scheme over a partition function, objects of their
own that the model does not hold, and on SQLite, which has none
(`partitioning_is_restated_on_postgres_and_mysql`). It is a second capability rather than
`partitions_are_tables` asked again, because MySQL and SQL Server answer that one alike and
this one apart. `ddl::create_table_sql` and the comparison ask it (under `ddl.rs` and
`compare.rs`); `create_ddl`'s T-SQL arm, *Script table as CREATE*, answers it with a note
line instead of the clause — `-- [dbo].[orders] is partitioned (RANGE RIGHT ([at]) ON
[ps_year]); its partition function and scheme are not read, so this script creates it
unpartitioned.`, the key through `export::comment_text` — so a script that creates the table
plain says so rather than passing for a restatement
(`a_partitioned_sql_server_tables_script_says_it_is_not_restated`).
**`Partitioning::clause_sql(dialect)` is the one spelling of the clause**, which `create_ddl`
and `ddl::create_table_sql` both write where `restates_partitioning`. Where `partitions_are_tables` it is `PARTITION BY
<key>` alone, each partition being a `CREATE` of its own; elsewhere a partition is storage the
table must declare, so the clause goes on to the `SUBPARTITION BY` line and every partition,
`` PARTITION `name` <bound> `` — or a bare `` PARTITION `p0` `` for `HASH`/`KEY`
(`the_partition_clause_lists_mysqls_partitions_and_only_postgres_key`).
**Where a partition is listed is `DbSchema::lists_under_parent`**: one whose parent this schema
holds is shown under it — the schema tree nests it there, and the ER diagram and the assistant's
schema draw the parent alone (under `schema_tree.rs`, `erd.rs` and `cli/mcp.rs`) — while one
whose parent is not here stands on its own, since under a parent nobody lists it would be
listed nowhere. `partitions_of(t)` is the other direction, in the schema's order, and
`listed_as(t)` the topmost ancestor held — the table a sub-partition is drawn as — its walk
bounded by the table count, since a catalogue cannot hold a cycle but a model built by hand
could (`a_partition_is_listed_under_the_parent_this_schema_holds`).
`TableInfo::partition_facts(cap)` is what the properties panel says about either end, as
`PartitionLine`s: `Of` and `Bound`, or `By` and a `Part` per partition with its bound (`None`
where it has none, drawn `—`) up to `cap`, then `More(n)`. `partition_lines` is their English
as `(label, value)` rows — *Partition of*, *Bound*, *Partitioned by*, *and N more partitions*
(`partition_lines_name_the_key_the_partitions_and_the_parent`) — and the panel words them
itself (under `properties.rs`).
`create_ddl`'s PostgreSQL arm restates both ends: a partitioned table's `CREATE TABLE` closes
`) PARTITION BY <key>;`, and a partition is `CREATE TABLE q PARTITION OF parent [(own column
options, own key, own checks)] <bound> [PARTITION BY <key>];` — no column list, which it takes
from the parent, and a primary key only where the partition declared one itself (its `PRIMARY`
index not `inherited`, possible where the parent has none), an inherited one coming back with
it from the parent (`a_partitions_own_primary_key_is_restated_and_an_inherited_one_is_not`).
**Its own column options are `"c" WITH OPTIONS [NOT NULL] [DEFAULT …]`**, one per
`PartitionOf::own_columns`, and a default the parent has and the partition does not is an
`ALTER TABLE ONLY q ALTER COLUMN "c" DROP DEFAULT;` after it: the clause gives a partition only
its parent's, so a dump's replay onto the source put back the parent's default and dropped the
partition's `NOT NULL` under a success report
(`a_partitions_own_column_options_are_restated_with_options`;
`a_partition_replay_keeps_its_own_defaults_and_not_null`, live, failed against a reader that
did not mark them).
Restated as two plain tables, the parent held no rows and routed none, and the partition was a
stray copy beside it (`a_partitioned_table_and_its_partition_are_restated_as_such`,
`a_sub_partitioned_partition_keeps_its_own_key`, `partitions_are_tables_on_postgres_alone`).
The MySQL arm ends `\n) PARTITION BY …;` with every partition listed, a partition there being
storage its table declares rather than a `CREATE` of its own
(`a_mysql_partitioned_tables_script_keeps_its_partitions`).
**`IndexInfo::inherited` and `ForeignKeyInfo::inherited` are what the partition takes from its
parent with it**: PostgreSQL clones a parent's index and key onto every partition, and both are
read and marked (under `pg.rs`) — kept, since a partition's grid edits key on its primary key,
and never restated, because the partition gets them again when it is created and a second
`CREATE INDEX` or `ADD CONSTRAINT` of the same name fails the restore. `create_ddl` skips an
inherited index, as `ddl::create_table_sql` and the dump's key section skip both. Not to be
confused with `CheckInfo::inherited`, which is the opposite flag on a different question —
`false` there means `NO INHERIT`.
**`IndexInfo::identifies_a_row` is the one answer to "does this index name a row", and it is not
"is it unique"** — the question three parts of the app ask and used to answer three ways. Unique,
not a foreign key, not partial (a `WHERE deleted_at IS NULL` promises uniqueness only over the
rows it admits), at least one key and every key a plain column (an expression key is no result
column, so `(a, lower(b))` reduces to `[a]`, which is not unique — measured on PostgreSQL 16.15),
not `lossy` (what was read back is not the whole index, so none of the checks above was made
against the whole of it), and — the sixth — **no key carrying its own `collation`**. The
collation is what the uniqueness is *measured in*, and a `WHERE col = ?` built from the key is
measured in the **column's**: given `email TEXT COLLATE NOCASE` and `CREATE UNIQUE INDEX ux ON t
(email COLLATE BINARY)` the index accepts `'A@x'` beside `'a@x'` while the `UPDATE` matches both,
so the 1-row net rolls the batch back and tells the user the edit failed for a reason that is not
the reason — the outcome this predicate exists to end. It refuses the safe direction (column
`BINARY`, index `NOCASE`) with it, which is the same conservatism the rest of the conjunction
takes. SQLite-only in effect: MySQL reports no per-key collation and PostgreSQL reports a
non-default operator class as `lossy` already. It says nothing about NULL — that is a fact about
the *table's* columns, which an index cannot see, and the two key resolvers add it.
**`IndexInfo::clustered` is SQL Server's `CLUSTERED` (`Some(true)`) or `NONCLUSTERED`
(`Some(false)`)**, `None` on every other engine and for an index the designer is making, which
takes T-SQL's default — `CLUSTERED` for a key, `NONCLUSTERED` otherwise. Three states rather
than a flag because the key is the case that needs the third: a `false` would have read every
MySQL key as nonclustered. It is modelled because restating an index without it is a different
table — a clustered one recreated as a plain `CREATE INDEX` leaves the table a heap, and a
`NONCLUSTERED` key re-added with the default becomes the clustered one, or is refused where
another index already is (Msg 1902) — and until it was, a clustered index other than the key's
could only be withheld as `lossy`. `tsql_create_ddl` restates it as `PRIMARY KEY NONCLUSTERED`,
`UNIQUE CLUSTERED` and `CREATE CLUSTERED INDEX`, writing only the side the default gets wrong
(`create_ddl_sql_server_restates_clustering`).
**A SQL Server columnstore index is `IndexInfo::method` of `TSQL_COLUMNSTORE`** (`COLUMNSTORE`),
T-SQL's counterpart of PostgreSQL's access method, its own spelling rather than one of
PostgreSQL's names, with `clustered` saying which kind: `is_columnstore` asks the method,
`is_clustered_columnstore` adds `Some(true)`. A clustered one is the table's rows stored by
column and **names no columns** — the reader clears the list the catalogue gives it (under
`mssql.rs`) — and a nonclustered one names the columns it stores, in no key order; neither is
unique or has an `INCLUDE` list. Until it was modelled every columnstore index was `lossy`, so
it could not be edited, a T-SQL rebuild refused the table over it, and Copy DDL and the dump
left it out under a note. **`IndexInfo::order` is a columnstore index's `ORDER (…)`** — the
columns it sorts its segments by, in that order: 2022 on for a clustered one, 2025 on for a
nonclustered one, whose order may name only columns it stores (Msg 1911, measured on 2025).
Empty for an unordered columnstore and for every other index. Until it was modelled an ordered
one was `lossy` too, the order unread. It is a column list rather than text so that a column
the designer renames or drops is followed there as it is in `include`. How it is written and
refused is under `ddl.rs`.
**SQL Server's XML and spatial indexes are `method` too** — `TSQL_PRIMARY_XML`
(`PRIMARY XML`), `TSQL_SELECTIVE_XML` (`SELECTIVE XML`), `TSQL_XML` (`XML`, a secondary one)
and `TSQL_SPATIAL` (`SPATIAL`), each the words `CREATE … INDEX` takes, which
`is_tsql_xml_or_spatial` asks; `is_tsql_xml_parent` asks for the two a secondary can be built
on, primary and selective, which a plan and Copy DDL create first and drop last (under
`ddl.rs`), and `xml_parent` the one a secondary's `USING XML INDEX` names — bracketed, `]]` its
escaped `]`, or bare — which `using_xml_parent` writes again naming another, for a parent the
plan renames (`xml_parent_tests`) — with what the kind takes after its one column in **`IndexInfo::using`**: a
secondary XML index's `USING XML INDEX [primary] FOR PATH|VALUE|PROPERTY`, a selective one's
`[WITH XMLNAMESPACES (…)] FOR (path = N'…' [AS …], …)`, a secondary selective one's `USING
XML INDEX [sxi] FOR ([path])` — an ordinary `TSQL_XML` index whose clause names a path rather
than a kind — and a spatial one's `USING <scheme> WITH (BOUNDING_BOX = (…), GRIDS = (…),
CELLS_PER_OBJECT = n)`. It is T-SQL text rather than fields, and that is safe only
because of where it comes from: composed by the reader from the catalogue's numbers and words
with **every option restated**, so a restatement is the index as it stands rather than whatever
the defaults are when it is made again, each value checked before it reaches the text (under
`mssql.rs`) — or typed in the designer, as a filter's predicate is. `None` on every other
index and every other engine. Until they were modelled each was `lossy`, so a T-SQL rebuild
refused its table and Copy DDL and the dump left it out under a note; a **selective** XML index
was the last of them.
**`IndexInfo::include` is an index's included columns, in order** — `INCLUDE (…)`, carried in
the index's leaf rows without being part of its key, on SQL Server and PostgreSQL (11 on), and
empty on MySQL and SQLite, which have no such clause (`ddl::supports_index_include`). It is
modelled rather than left to `lossy`, which is what it was, and on SQL Server that cost three
things at once: an index with one could not be edited, nor restated by a rebuild — which
refused five of WideWorldImporters' 48 tables over it — nor written into a dump.
`include_sql` is the one spelling, ` INCLUDE (…)` after the key list and before `WHERE`, or
nothing where the list is empty or the engine has no clause; `ddl::create_index_sql` and
`create_ddl`'s one hand-rolled index emitter, PostgreSQL's model path, both write it.
`tsql_create_ddl` was a second, and now writes each index it can restate through
`create_index_sql` (`pub(crate)` for it), so a script and a plan write one index one way, a
columnstore's form included. `create_ddl_sql_server_writes_t_sql` restates a covering index
with `INCLUDE ([balance])` and a nonclustered columnstore `ix_cs` as `CREATE NONCLUSTERED
COLUMNSTORE INDEX`, and leaves a lossy `ix_xml` out under the note, which for a table and an
indexed view alike now reads *"is of a kind this script cannot restate; it is left out"*. It
began *"has included columns, or…"*, and named columnstore, then XML and spatial, then a
selective XML index and an ordered columnstore, until each was modelled; nothing an index can
be made with is left behind it on SQL Server now but one naming a graph column and a reading
the reader refuses to splice into T-SQL, so the note names no kind. **The indexes are written
XML parents first** — a stable sort on `is_tsql_xml_parent`, the rest keeping their order —
because the catalogue lists them by name: AdventureWorks' secondary `IXML_…` sorts before the
`PXML_…` it is built on, so the script created the secondary first and failed there
(`create_ddl_writes_an_xml_parent_before_its_secondaries`; live, the Copy DDL half of
`a_selective_xml_index_is_read_whole_and_survives_a_rebuild`, which drops the table and runs
the script from nothing, its secondary named to sort first). How the designer's plan keeps the
list through a rename, a removal and a retype is under `ddl.rs`.
**`classify_column_type` reads all three engines' spellings now**, the type name being what the
icon in the schema tree, the ER diagram's cards and tooltips, the completion popup and Find
Anywhere are chosen from. The MySQL and PostgreSQL gaps were closed one engine at a time and
SQLite's survived both, which is where it matters most: SQLite stores the declared type text
verbatim, so an unusual spelling there is ordinary rather than exotic. Every name added is an
example in SQLite's *own* type-affinity documentation — `VARYING CHARACTER`, `NATIVE CHARACTER`,
`NVARCHAR`, `NCHAR`, `CLOB`, `UNSIGNED BIG INT`, `INT2`/`INT4`/`INT8` — and `FLOAT4`/`FLOAT8` come
with the same edit as PostgreSQL's internal aliases for `real`/`double precision`, which
`format_type` does not emit but a hand-written draft does. All of them drew the "unrecognised
type" glyph before (`classify_column_type_reads_sqlites_own_documented_spellings`, which also
asserts the affinity rule's other examples already worked, so the set is the whole of what was
missing rather than a sample of it).
**`index_disabled_sql(version)` is how the MySQL family is asked whether an index is switched
off**, and it is pure and here because the column that answers differs in three ways at once. A DBA
hides an index to test a plan change — MySQL 8's `ALTER TABLE … ALTER INDEX … INVISIBLE`,
MariaDB 10.6's `… IGNORED` — and neither state was read, so a hidden index came back as an
ordinary one with `IndexInfo::lossy` `false`, any edit to it took `ddl::diff`'s drop-and-recreate
arm, and the index came back **live** with the optimizer using it again and the preview saying
nothing. What the function returns is an `information_schema.STATISTICS` *expression* answering
`1`/`0` rather than a column name: the columns are named differently, carry opposite polarity
(`IGNORED = 'YES'` against `IS_VISIBLE = 'NO'`), and do not exist at all on a server older than
the feature — where naming one fails the whole introspection rather than the one value, which is
the same reason the `EXPRESSION` and `ALGORITHM` columns beside it hold the row shape steady with
a `NULL`. A version string it cannot parse falls to the constant `0`, an unread flag reported as
"not switched off": that is what the model said before this existed, and it is the reading that
offers an edit rather than withholding one on a guess. `major_minor` is the private half — the
digits before the second `.`, which is all `10.11.14-MariaDB-1:10.11.14+maria~ubu2204`, `8.4.11`
and `5.7.44-log` have in common. The live pin is
`a_switched_off_index_is_not_silently_brought_back`, which asserts the read **and** the
composition through `diff` that the read exists to govern, since either half alone passes for the
wrong reason.
**`TableInfo::create_sql` short-circuits all of that where the engine keeps its own `CREATE`
text** — which of the three only SQLite does (`sqlite_master.sql`, plus the `CREATE INDEX`
statements it stores separately and without which a table's DDL is incomplete). That is a
fidelity decision, not a shortcut: reconstructing a SQLite table from this model emitted
`AUTO_INCREMENT`, which SQLite **accepts** by reading it as part of the type name — so the
statement replayed silently into a different column — plus MySQL's inline `KEY name (cols)`,
which SQLite cannot parse at all, and an empty column list for an index whose keys are
`lossy`; and it dropped the foreign key, `WITHOUT ROWID`, CHECK constraints and column
collations, none of which the model carries. It is deliberately **not** used for a *view*, even
on SQLite: there the model is genuinely complete (a name and a body), so the shared emitter is
both correct and consistent with the other engines. The test asserts SQLite takes its own DDL
back, which is the only assertion that would have caught the `AUTO_INCREMENT` case.
**PostgreSQL has a narrower version of the same short-circuit, per index rather than per table.**
`create_ddl`'s PG arm emits each non-primary index from the model, except one the model only
partly read — an `IndexInfo::lossy` one with a `create_sql` — which is emitted as the server's own
`pg_get_indexdef` text instead. Emitting that one from the model is not merely different but
wrong: there is no field for `NULLS NOT DISTINCT` or a storage parameter — nor was there for an
`INCLUDE` list, and a structure dump of `CREATE INDEX ix ON t (a, b) INCLUDE (c, d)` restored an
index that no longer covered, with no edit anywhere and the dump reporting success. A **fully**
read index keeps the model's emission, which is the one the designer's preview and the compare
pane are written against, and `create_ddl_postgres_emits_a_lossy_index_from_the_servers_own_text`
asserts both halves so the fix cannot quietly change every PostgreSQL table's DDL. A plain
index's `INCLUDE` list is that second half now — read into `IndexInfo::include` and written by
`include_sql` as `INCLUDE ("c", "d")`, which the same test checks — while one on a primary-key
or unique **constraint**'s index stays lossy and goes out as the server's text, since the model
restates a key as a column list and a unique constraint as `ADD CONSTRAINT … UNIQUE`, and
neither carries the clause (under `pg.rs`).
**`TableInfo::dependent_ddl` is the same fidelity call made for the rebuild**: the `CREATE` text
of the objects that go down with the table and have to be put back — SQLite's triggers, filled
by `sqlite::trigger_statements`, empty for a **table** on the two engines that alter one in place
and so never destroy the table their triggers hang off. **It is no longer SQLite's alone, and the
exception is a view.** PostgreSQL has no `CREATE OR REPLACE VIEW` that can narrow a column list,
so `ddl::diff_view` takes the drop-and-create arm there — and `DROP VIEW` takes every `INSTEAD OF`
trigger on it, which is the only way a PostgreSQL view is written to at all. `Change::ReplaceView`
has always emitted its `replay` after the re-create; the field was simply empty by construction on
the one engine that takes that arm, so a view with an `INSTEAD OF INSERT` trigger, narrowed in the
view editor, came back with none — measured on 16.15, with the plan reporting success and the
preview naming nothing. `pg::fetch_schema` fills it from `pg_get_triggerdef` for a view and leaves
it empty for a table — in the pure `pg_fold_checks_and_triggers`, where
`a_views_triggers_are_kept_for_its_re_create_with_their_state` pins the replay and the disabled
state it restates without a server — and the live tier's
`a_recreated_view_keeps_the_triggers_the_drop_took`
gates on the arm the plan takes rather than on the engine. **SQL Server leaves it empty for a
view, on purpose**: its one re-create is a rename (`ddl::supports_view_rename`), and an
`INSTEAD OF` trigger's stored text names the old view, so a replay would address a view the
plan has just dropped. The re-create's risk says instead that the grants, the description
(`MS_Description`) and the `INSTEAD OF` triggers go with the drop
(`sql_server_renames_a_view_by_re_creating_it`); an edit that keeps the name is a `CREATE OR
ALTER`, which drops nothing but an indexed view's indexes — and those are not a replay either:
the plan builds them again from `TsqlView::indexes` (under `ddl.rs`). Deliberately the server's own statement
rather than a
re-emission from `TriggerInfo` — and that stays the call now that `sqlite::triggers_of` *does*
read a SQLite trigger into the model. The two are not redundant: the model is what the **editor**
diffs, the text is what a **rebuild** puts back without depending on the parse being perfect,
and the failure mode the rebuild has to avoid is the one `IndexInfo::lossy` exists to prevent:
the part that didn't survive the parse is gone from a trigger that still looks armed. A view that
*selects from* the rebuilt table needs
nothing here — `DROP TABLE` leaves a view that selects from the table in place, SQLite resolving
a view's references when it runs rather than when it is declared, and the table returns under
the same name before the transaction ends.
**`TableInfo::referring_ddl` is its sibling, and its opposite in both directions**: the `CREATE`
text of triggers on *other* tables whose SQL names this one, SQLite only and empty everywhere
else, filled by `sqlite::all_trigger_sql` and matched through `intel::code_names`. These are not
dropped by a rebuild and never replayed — and they are also never *rewritten* by one, which is
the whole reason the field exists: SQLite's own `ALTER TABLE … RENAME COLUMN` rewrites every
trigger in the database that names the column, while the twelve-step rebuild renames the table
underneath them and leaves their text exactly as it was, so a trigger on `other` goes on naming a
column of `t` that is gone and `other` starts refusing every write. `ddl`'s
`rebuild_strands_a_trigger` is the one reader, and the text is verbatim for the reason
`dependent_ddl`'s is — a refusal built on a parse that doesn't round-trip triggers is the same
argument against.
**`TableInfo::referenced_by` is the other engine's version of looking one table over**: the
foreign keys declared on *other* tables that reference this one, as `InboundForeignKey { schema,
table, key }`, SQL Server only and empty everywhere else. It exists for `ddl::tsql_rebuild_sql`,
which drops the table in the middle — a key another table has on it refuses that `DROP TABLE`
(Msg 3726), so each is dropped first and put back after, pointed at the table as it comes out.
A self-reference is not in it: that key is the table's own `foreign_keys` and goes down with it.
`link_inbound_foreign_keys(&mut [TableInfo])` fills it from every table's `foreign_keys`,
matching `ref_schema`/`ref_table` exactly and taking a key that names no schema to mean its own
table's; `db::mssql::collect_schema` calls it last, **after** the keys' actions are filled,
because the rebuild puts the other table's key back as it was, `ON DELETE` and all
(`inbound_foreign_keys_land_on_the_table_they_reference`). SQLite's reader sets it empty: its
rebuild drops nothing another table's key points at without `foreign_keys = OFF`.
**The grid's *Referenced by* does not read it**, and it stays SQL Server's alone.
`incoming_keys(schema, table_schema, table, database, dialect)` asks the same question on
demand, of the loaded `DbSchema`, on every engine — and answers it differently in the one place
that matters to the grid: a **self-reference counts**, because an `employees.boss_id` is how a
tree finds a node's children, which is precisely the key `referenced_by` leaves out. Views are
skipped, carrying no keys. `ref_schema` is matched as the engine resolves it, through
`ddl::ref_schema_is_database` rather than exactly: on MySQL it names a database, so `None` or
`database` itself matches and a key into another database is not this table's; elsewhere it is
a namespace, `None` meaning the declaring table's own. Only `database` is searched — a key
declared in another database is in another loaded schema, and is not found.
`referencing_target(child, values, database, dialect)` builds the query, and it **is**
`follow_target` (Follow relation's) over the key turned round — `columns` and `ref_columns`
swapped, the child as `ref_table`, `ref_schema` `None` on MySQL and the child's namespace
elsewhere — so the quoting and the qualification are that function's and cannot drift from it
(`referenced_by_finds_the_keys_pointing_at_a_table_and_filters_their_rows`,
`referenced_by_matches_namespaces_as_the_engine_resolves_them`). **Except a NULL, which it
refuses**: `follow_target`'s `IS NULL` is harmless forwards and wrong backwards, where a
foreign key with a NULL in it references nothing (MATCH SIMPLE, on all four engines) and
`WHERE fk IS NULL` listed every orphan as this row's children (`nothing_references_a_null_key`).
`referenced_values(rs, data_idx, cols)` is the row's side of the same question — `None` for a
NULL and for a column that holds bytes (`edit::holds_bytes`), whose `<16 bytes>` placeholder
filtered nothing on MySQL and PostgreSQL and was a conversion error on SQL Server
(`referenced_values_refuse_a_null_or_a_byte_column`).
**The tree's Generate DDL entries are two `DbSchema` methods, one per altitude.**
`create_ddl_script(schema, dialect)` emits one namespace in **dependency order** — the
standalone types, then base tables, then views, then the sequences that stand on their own —
because an omitted foreign key leaves a script that still runs while an omitted type fails on
the first `CREATE TABLE`; a sequence a `serial` or an identity column already creates is
skipped, since restating it fails on a name that exists. **The split is three ways and goes
through `shape()`**: a two-way `partition(|t| t.is_view)` put a MariaDB sequence in the
base-table half, harmless only while `create_ddl` withheld it — so MariaDB sequences are a list
of their own, written **first**, after the types and before the tables, since a MariaDB table
whose default calls `nextval(s)` is refused while `s` is missing: a read one is its `CREATE
SEQUENCE` (no position — this is a definition script), an unread one `create_ddl`'s comment
saying so. It left every one out while only the `objects_in(Sequence)` list carried sequences,
and that list holds PostgreSQL's and SQL Server's alone
(`create_ddl_script_carries_a_read_mariadb_sequence_first`). `create_ddl_script_all(dialect)` is
the database node's analogue: it walks `schemas()` in **display** order (`public` first, then
alphabetical), so the script reads down the tree it was raised from, and joins only the
namespaces that produced something rather than leaving a blank run where an empty one was.
Where an engine has no namespaces at all — MySQL and SQLite, whose tables carry `None` — there
is nothing to walk and it *is* `create_ddl_script(None, dialect)`
(`create_ddl_script_all_is_the_flat_script_without_namespaces` pins that equivalence, which is
the half a namespace-walking loop would quietly get wrong). Ordering *between* namespaces is
consequently display order and not dependency order: a type used by a table in another
namespace is emitted after it if the alphabet says so. That is the same class of gap
`create_ddl_script` already carries for foreign keys, and it is accepted for the same reason —
the script goes to the clipboard and an editor tab, is read and edited before it is run, and
`ddl_preview` is still the only thing that runs anything.
**Both join their parts through `ddl::join_scripts`, and so does the tree's folder script**
(`schema_tree`'s object-group menu). On SQL Server each part closes its batch with `GO` — one
that already ends in a `GO` gets no second, which is what a namespace's script is when the
database's joins it — because a view, procedure, function or trigger must begin a batch and a
routine's body runs to the end of its batch: joined with blank lines alone, a script's first
procedure swallowed every statement after it, in this editor and in SQL Server's own tools
(`a_sql_server_script_puts_each_object_in_a_batch_of_its_own`). Every other engine ends a
statement at its `;` and gets the blank lines it always had. It joins whole scripts rather than
emitted statements — a routine's part is already a `client_script` — so it is not a second one.
**`TriggerInfo`/`TriggerAction`/`TriggerEvent`/`TriggerEnabled`/`TriggerSource`** are the
trigger half, and carry three rules the
same "restate everything or it silently resets" logic as `ViewOptions`.
`TriggerAction::Function::name` is **emittable SQL** on both producers — already quoted, and
qualified: always by the editor's `fn_sql` (`qualified_ident`, `public` included), by the
reader's `tgfoid::regproc::text` wherever `search_path` would not resolve it, which on a stock
server is every schema but `public` — never the raw `proname`; it once meant both
depending on who wrote it, which bound triggers to `public`'s copy of a function picked
from another schema. `TriggerEvent`'s **declaration order is load-bearing**: the derived
`Ord` is what the UI sorts into and must be `pg_trigger.tgtype`'s bit order (`INSERT`,
`DELETE`, `UPDATE`, `TRUNCATE`), not DML order, or an introspected trigger re-sorts into a
phantom drop-and-recreate. `TriggerEnabled` is `tgenabled`'s **four** states, because
`ENABLE ALWAYS`/`ENABLE REPLICA` folded into a bool get recreated as plain `O` and change
what fires during replication apply; `old_table`/`new_table` are `REFERENCING OLD/NEW
TABLE`, whose loss breaks *every write to the table* rather than failing the plan.
`TriggerSource` is the MySQL body + session state, fetched lazily — see `schemaic-db`. Its
`apply_to` copies both onto a `TriggerInfo`, one method because both sides of the diff have to
be patched with it, and `may_overwrite_edit(opened, draft)` decides whether a draft row the user
has *already edited* takes the correction anyway: it does when the body the keystrokes were
typed on top of is not what the server holds, because what the skip preserves there is not the
user's work but a few characters riding a `'it's'` that Apply would `DROP` the trigger to emit.
`None` for `opened` means no record of what the row opened with, so nothing can be said and the
row is left alone. The failure and the call site are under `ui/trigger_editor.rs`.
`TriggerInfo::create_sql` is the one trigger emitter and has **four** arms, not two: SQLite's
shape is neither of the others' — PostgreSQL's `UPDATE OF` and `WHEN` with MySQL's inline
body, no definer, no ordering clause, no session state and always `FOR EACH ROW` — so it is
asked for by name rather than reached by falling off the end of a `!pg`. `update_columns` and
`condition` are consequently **not** PostgreSQL-only fields: MySQL is the engine with neither.
**SQL Server's arm rebuilds the statement from its parts** (`tsql_create_sql`, which is
`tsql_statement(false)`; the apply path asks `tsql_statement(true)` for the `CREATE OR ALTER`).
It used to replay `sys.sql_modules.definition` whole, as `RoutineInfo::create_sql` did a
routine's until the routine editor came (below) — and before that it fell through to MySQL's header and emitted a trigger inside a
trigger, which Compare's DDL panes showed. Replaying is right until something can be edited:
then an edit to any part has to be the statement that runs, so the arm writes the header from
`TriggerInfo::tsql` and appends the body after `AS` verbatim, and the result reads back through
`ddl::tsql_trigger_parts` as the same parts — the round trip the editor rests on
(`a_sql_server_trigger_is_written_from_its_parts`). **The trade is the header's spelling**:
`sys.sql_modules` keeps the statement as typed — measured on SQL Server 2022, a comment ahead of
`CREATE` survives there, and a `CREATE OR ALTER` is stored as `CREATE   TRIGGER` — and a rebuild
keeps the header's parts, not the way they were written, so Copy DDL and the dump now show the
rebuilt statement rather than the stored one (`introspection_reads_the_schema_as_declared`
asserts the rebuilt text on the live leg). **The header's comments are the exception, and were
not at first**: the rebuild dropped them — the SSMS template block ahead of `CREATE`, a comment
between the parts — from every edit, Copy DDL and dump, while `tsql_statement`'s own doc said
the body was kept "comments and all", true only after `AS`. They now travel in
`TsqlTrigger::module` and go back in front of the rebuilt statement (`TsqlModule`, above).
**`TsqlTrigger` is what the shared model lacked**, on the "restate everything or it silently
resets" rule again: an unstated `EXECUTE AS` (`ExecuteAs`) is `CALLER`, an unstated `NOT FOR
REPLICATION` fires during replication, and the `First`/`Last` rank per event (`FiringRank`)
lives outside the statement altogether. **`set_rank` holds one rank per event and keeps the list
in `TriggerEvent` declaration order** (Insert, Delete, Update) — the order `db::mssql`'s reader
sorts it into as well — so a rank cleared and set again is no change to diff
(`a_trigger_rank_is_one_per_event_in_event_order`). `TriggerInfo::set_event` is the event
toggles' one write: it keeps the events in the same order and, switching one off, drops that
event's rank, since `sp_settriggerorder` refuses a rank on an event the trigger does not fire on
(Msg 15125) and the form would otherwise leave a draft only Apply could reject
(`switching_an_event_off_drops_its_rank`). `ExecuteAs::parse_field`/`field_text` are both
editors' single **Execute as** field: empty is none, `CALLER`/`SELF`/`OWNER` in any case, anything
else a user, bare or T-SQL-quoted — and `field_text` quotes a user whose bare name would read
back as the keyword (a user called `owner`), which is what makes the field a round trip
(`execute_as_reads_and_writes_its_field`). Two fields say the parts cannot be trusted. `verbatim`
holds the stored text whole when `tsql_trigger_parts` could not read its header, and the arm
restates it as it was, since a rebuild from what *was* read would drop the rest; `hidden` marks a
trigger the server shows no text for (`WITH ENCRYPTION` — `definition` is NULL, measured — or
not visible to this login), which comes back as a comment line saying so rather than half a
statement (`a_sql_server_trigger_the_parts_cannot_hold_is_restated_verbatim`).
**`TriggerInfo::is_editable` is the one gate over all three refusals** — not a constraint
trigger, not `verbatim`, not `hidden` — and it is asked of what the server *reported*, never of
the body: a body cleared in the editor is a validation error, not a reason to lock the form.
**`tsql_follow_ups` is what a create or an alter loses, and it is never appended to the
statement's text.** Measured on SQL Server 2022, a `CREATE OR ALTER` keeps a disabled trigger
disabled but resets `is_first` from 1 to 0, and a fresh `CREATE` fires whatever the trigger was;
so each rank gets an `sp_settriggerorder` and a disabled trigger a `DISABLE TRIGGER`, on both
paths, one rule. It is the two halves `tsql_rank_statements` and `tsql_disable_statement`
together, and they are apart because an edit plan runs every rank after every alter (under
`ddl.rs`, for Msg 15130) where a dump keeps each trigger's together. Both are statements of
their own, never text in the trigger's, because `CREATE TRIGGER` must be alone in its batch
and anything after the body in that batch is not run after the trigger — it *is* the trigger
(`a_sql_server_triggers_rank_and_disabled_state_follow_it`). **A hidden trigger has none**: its
statement is a comment, so nothing was created, and a dump that then ranked or disabled it
would stop its restore at a trigger that does not exist.
**`create_set_sql` is the whole-set form beside it, and the difference is `FOLLOWS`/`PRECEDES`.**
That clause is a statement about the group as it stands when the statement runs, not a property
of the trigger: both MySQL and MariaDB refuse one naming a trigger that is not there yet
(`ERROR 3011` / `ERROR 4031`, *"Referenced trigger … does not exist"*), and the catalogue always
gives a group's **leader** `PRECEDES <successor>` — right for the caller replacing one trigger
inside a group that exists, impossible for the caller creating the whole group from nothing. A
dump of any table with two triggers in one timing/event group therefore died on its *first*
`CREATE TRIGGER`, on both servers, after the `DROP TABLE` above it had already run.
`with_resolvable_order(exists)` takes an unresolvable clause off, and `TriggerOrder::target` is
the name both arms carry — the one thing a caller deciding whether the clause can be *resolved*
has to read, rather than matching the enum for it. `create_set_sql`
supplies `exists` as "created earlier in this same set", case-insensitively, because a trigger
name is the server's. Dropping the *leader's* clause is sufficient rather than approximate:
every non-leader carries `FOLLOWS <predecessor>`, so a group created in order rebuilds its own
chain, and a trigger created alone into an empty group is its leader whatever it says. It
borrows where there is nothing to take off, which is every trigger on the other two engines —
neither has the clause. **`exists` is a parameter rather than a field because the two callers
mean different things by it.** `core::dump` asks through `create_set_sql`, where the set *is*
the world. The apply path is the second caller and the one this should never have lacked —
`core::ddl`'s `trigger_statements`, which asks a two-term version (created earlier in this plan,
**or** surviving it) and says there what each term is for. A comparison between two databases
would ask a third question, whether the *other* database holds the name; nothing asks it yet.
On SQL Server the set also carries each trigger's `tsql_follow_ups` straight after its create,
flattened in as statements of their own, so the dump and Copy DDL restate a rank and a disabled
state for the batch reason above.
`CheckInfo::validated`/`inherited` are PostgreSQL's `NOT VALID` / `NO INHERIT`, carried and
restated: they are part of the clause, and `pg_get_constraintdef` prints them *after* the
parens, which is why `ddl::check_predicate` must strip them before peeling. **An unnamed check
stays unnamed**: `CheckInfo::clause_sql` writes a bare `CHECK (…)` when the name is empty,
because most SQLite checks have none and `CONSTRAINT "" CHECK (…)` is not a nameless constraint
but a syntax error — while inventing a name would make a rebuild read as though it renamed
something.
**`RoutineInfo`/`RoutineKind`/`SqlDataAccess`/`Volatility`/`RoutineSource`** are the stored
functions and procedures, on the three engines that have them, under the same rule again — a
redefinition replaces the whole routine, so anything the statement doesn't restate reverts.
Which fields those are is per-engine: PostgreSQL's `volatility`, `strict`, `language`, the
per-routine `SET` clauses (a `SECURITY DEFINER` function that loses its pinned `search_path`
is a privilege-escalation hole) and `identity_arguments` (the parameter list in the form that
*identifies* a routine, which is not the form a `CREATE` takes — see `ddl.rs`), MySQL's
`deterministic`, `data_access`, `definer` and `comment`, SQL Server's `tsql` (below).
`security_definer` is the one field PostgreSQL and MySQL share and **their defaults are
opposite** — PostgreSQL's is INVOKER, MySQL's is DEFINER — which is why the MySQL arm of
`create_sql` states the clause in *both* directions instead of leaving the default unwritten
as the PostgreSQL arm does, and why `RoutineDraft::blank` seeds it per-engine rather than
picking the safer-looking answer for both. `RoutineKind` is a tag rather than "is `returns`
empty", because a function and a procedure are different objects to every statement that
addresses one — and on PostgreSQL that is not only a keyword: **`CREATE PROCEDURE` takes a
strict subset of a function's attributes**, so `pg_create_sql` withholds the return type, the
volatility *and* the strictness for one, and `routine_editor` withholds the two controls that
would set them. Any of the three on a procedure is
`ERROR: invalid attribute in procedure definition`, and guarding only `RETURNS` left a plan
that could be built in two clicks and could only fail at Apply.
`RoutineInfo::is_editable` is the other gate on that model: the emitter writes the body as the
routine's *source*, which is right for every language whose body is source text and wrong for
`LANGUAGE c` and `LANGUAGE internal`, where `prosrc` is a link symbol and the recreate needs
`AS 'obj_file', 'link_symbol'` — so those are listed and droppable but not editable, the call a
materialized view gets.
**SQL Server's arm of `create_sql` rebuilds the routine from its parts** (`tsql_create_sql`,
`replace` being `CREATE OR ALTER`): `CREATE [OR ALTER] PROCEDURE name` with the parameter list
on the next line, or `FUNCTION name (params)` then `RETURNS …`; the `WITH` options; a
procedure's `FOR REPLICATION`; then `AS` and the body verbatim, unterminated as a trigger's is.
It returned the stored text whole until the editor came, which is right until an edit to a part
has to be the statement that runs — the trigger arm's reasoning above, and its trade: the
header is the rebuilt spelling, not the stored one, but for its comments, which
`TsqlRoutine::module` carries back in front (`TsqlModule`, above). The result reads back through
`ddl::tsql_routine_parts` as the same parts (`a_sql_server_routine_is_written_from_its_parts`).
**A function's list that ends inside a `--` comment closes on a line of its own**, asked of
`pairs::region_at` as `create_view_sql` asks before its `;`: the list is kept verbatim, comment
and all, and written as `(@a int -- the input)` the comment swallowed the `)`, so every edit,
Copy DDL and dump of such a function was refused (Msg 102, measured on SQL Server 2022)
(`a_parameter_list_ending_in_a_line_comment_keeps_its_close`; live,
`a_parameter_list_ending_in_a_comment_rebuilds_and_replays`). A procedure's list is followed by
a newline already.
**`RoutineInfo::tsql` (`TsqlRoutine`) is what the shared model lacked**, on the same "restate it
or it resets" rule: `options`, a `Vec<TsqlRoutineOption>` (`Recompile`, `SchemaBinding`,
`NativeCompilation`, `ReturnsNullOnNullInput`, `CalledOnNullInput`, `ExecuteAs`, and a scalar's
`Inline(bool)`, each with its `sql()`), because each is one a `CREATE OR ALTER` resets unless
restated — measured on SQL Server 2022, an alter without `WITH RECOMPILE, EXECUTE AS OWNER` left
neither; and `for_replication`. `options` is in the stored text's order, and **`set_option` is
the toggle that keeps it**: switching one on appends it once, switching it off removes it and
leaves the rest in place (`a_routine_option_toggles_without_disturbing_the_rest`), so the
statement that restates the list writes it as the server had it. **Equality is not the list's,
though: `TsqlRoutine`'s `PartialEq` is hand-written and compares the options as a set**
(`routine_options_compare_as_a_set`), because T-SQL takes a `WITH` list in any order and an
option cleared and set again appends, wherever the stored text had it — which a list comparison
would diff as an edit, and an `ALTER` that changes nothing. `execute_as`/`set_execute_as`,
`inline`/`set_inline` and `returns_null_on_null_input`/`set_null_input` are the slot setters the
routine form's single-valued controls write through: each replaces its option in place or
removes it (`a_routine_option_slot_holds_one_value`). **`set_null_input`'s "off" is whichever
spelling the original had** — `CALLED ON NULL INPUT` stated, or nothing, both the default —
which is why it takes the routine as opened: the toggle turned on and off again restores the
text's own clause rather than one nobody wrote
(`the_null_input_toggle_returns_to_the_stored_spelling`). **`TsqlShape`** (`Procedure`,
`Scalar`, `InlineTable`, `MultiStatementTable`, read by `RoutineInfo::tsql_shape` off the kind
and `RETURNS` — `a_sql_server_routine_has_a_shape` — past any comment the kept `RETURNS` text
opens with, since `RETURNS /* rows */ TABLE` read off the raw string was a scalar, offered the
options an inline function refuses and recreated on the comment's deletion:
`a_comment_in_front_of_a_functions_table_keeps_its_shape`) decides which options a routine may carry:
`TsqlShape::allows`, measured on SQL Server 2022, gives an inline table-valued function none of
`EXECUTE AS`, the null-input clauses or `INLINE`, a multi-statement one `EXECUTE AS` alone of
them and no `NATIVE_COMPILATION`, and `RECOMPILE` to a procedure only — pinned through the validator that asks it
(`a_sql_server_routine_option_its_shape_refuses_is_refused`, under `ddl.rs`). **Three cells ask
the rest of the `WITH` list**, which is why `allows` takes it: a procedure takes `SCHEMABINDING`
only when natively compiled (Msg 10796, *supported only for natively compiled modules, and
required for those modules*, 2022 and 2025 alike), where every function takes it alone; and a
natively compiled module takes neither `RECOMPILE` nor `RETURNS NULL ON NULL INPUT` (Msg 10794,
*not supported with natively compiled modules*, both versions), though it takes `EXECUTE AS`,
`INLINE` and `CALLED ON NULL INPUT`. Each cell was once answered by the shape alone, so the form
offered the toggle and Apply failed at the server — the Recompile toggle did not ask `allows`
at all, and asks it now like the others. The validator says *once it is natively compiled*
when the shape alone would take the option; the message's other half — a natively compiled
module without `SCHEMABINDING` — is a missing option rather than a refused one, and the
validator names it separately
(`schema_binding_on_a_procedure_is_refused_unless_it_is_natively_compiled`; live,
`a_natively_compiled_module_is_refused_what_the_server_refuses`). It is also what `ddl::routine_signature_changed`
compares, so one reading of the shape answers both questions. `verbatim` and `hidden` are the trigger's two
fallbacks: the stored text restated whole when the parts could not read its header, a comment
line saying so when the server shows no text (`WITH ENCRYPTION`, or no `VIEW DEFINITION`)
(`a_sql_server_routine_the_parts_cannot_hold_is_restated_verbatim`). `is_editable` is false for
both, beside `c` and `internal` — the same case: listed and droppable, nothing to edit.
**`numbered` is the rest of the numbered procedure group a procedure heads** — `grp;2`,
`grp;3`, as `(number, stored text)` from `sys.numbered_procedures`, in number order — read
because the head gives no sign of them: its text in `sys.sql_modules` has no `;` and
`sys.objects` lists the group once, yet `DROP PROCEDURE grp` takes every member (why the editor
refuses a plan that drops a head is under `ddl.rs`). It is in the hand-written `PartialEq`;
`numbered_names` spells the members `grp;2, grp;3` for a sentence; and `ObjectItem::create_sql`
— Copy DDL and the dump — restates each member's text after the head's, each its own `GO` batch
through `ddl::client_script` (`a_numbered_group_head_scripts_its_members`). A member the server
shows no text for (created `WITH ENCRYPTION`; the reader reads its NULL as `""`) is a comment
naming `grp;n`, as an encrypted routine is: written as its empty text it was a bare `;` that
said nothing, and the dump refuses to drop such a group on a replay (`dump::routine_hold`).
**The comparison leaves the members out of what it compares**: a member can be neither altered
nor dropped apart from its group, so a target missing `grp;2` was Differing over a no-op `CREATE
OR ALTER` of the head and Differing again after Apply. `compare::routine_entry` drafts the
source's routine with the target's members, flags an entry whose members differ `uncertain`,
and shows each side's members in its DDL pane; a group only the source holds is created whole,
`Change::CreateRoutine` going through `ddl::routine_create_statements` like the script
(`a_numbered_groups_members_are_shown_not_planned_and_created_with_it`; live,
`a_comparison_creates_a_numbered_group_whole`). **Except a group with a member the source
shows no text for** (an encrypted `grp;2`): created, that member was the comment above inside
the plan, the group arrived head-only and Apply reported a success, so such a group is given
the empty set and disclosed through `omission_note`, as an unread routine is
(`a_group_whose_member_the_source_could_not_read_is_disclosed_not_created`; live,
`a_comparison_discloses_a_group_whose_member_the_source_encrypts`). Where both sides hold the
group the members are not planned, so the pair is drafted as before.
`RoutineSource` is the MySQL body + session state, fetched lazily, and exists
for exactly the reason `TriggerSource` does — `information_schema.ROUTINE_DEFINITION` resolves
the body's escapes, and every edit on that engine begins with a `DROP` that commits on its own,
so a restate built from the resolved text can fail *after* the only copy is gone. Its `body` is
an **`Option`** where `TriggerSource`'s is not, because the two halves of that row fail
separately: the three session values need no parsing and are always trustworthy, so folding
them into the body's success meant a routine with a header the reader didn't understand was
later recreated under whatever `sql_mode` the applying session happened to have.
`EventInfo` is the **MySQL scheduled event**, and it is the one object modelled for a single
engine's sake: PostgreSQL's nearest equivalent is `pg_cron`, an extension with its own
catalogue and no `CREATE EVENT` grammar, and SQLite has no scheduler at all. Three things
about the model are decisions rather than transcription. **`EventSchedule` is two shapes, not
one nullable set of four fields** — `AT` takes a timestamp and nothing else, `EVERY` takes an
interval and optional bounds, and a single struct would have let a draft describe a syntax
error the form then had to refuse separately. **Every timestamp and interval quantity is held
as SQL, not as a value**: the catalogue reports `2026-01-01 03:00:00` and the reader
(`event_time_expr`/`event_interval_expr`) is what quotes it, once, because a field that can
only hold a literal cannot express `CURRENT_TIMESTAMP + INTERVAL 1 HOUR` — the same contract a
column default already has, and the same reason `event_interval_expr` tests the *value* rather
than the unit, so a compound unit this build has never heard of is still quoted. And
**`time_zone` is modelled** even though `CREATE EVENT` has no clause for it: the schedule is
read in the session's zone, the server records which one, and an edit applied from another
zone moves every future firing — so it rides in the session wrapper beside `sql_mode`
(`ddl::event_session_wrapped`). `EventStatus::SlavesideDisabled` is a third state rather than
a flavour of `Disabled` because a replica sets it for itself and restating the wrong one is a
real edit; `EventSource` is the lazy `SHOW CREATE EVENT` read, and exists for exactly the
reason `RoutineSource` does, with the consequence one notch milder — `ALTER EVENT` edits in
place, so a body restated from the escape-resolved copy is *refused* rather than lost.

**The standalone objects** — `EnumInfo`/`DomainInfo`/`SequenceInfo`, PostgreSQL's, plus the
routines above and the events beside them — sit here beside the tables **on `DbSchema` itself**
rather than being
fetched lazily: the tree lists them and a column's type *is* one of them, so a separately
refreshed second cache would be a second answer to "what is in this database" and the two
would diverge on the first refresh that only updated one. `routines` used to be the exception,
fetched only for the trigger editor's dropdown on the grounds that nothing rendered a body
until an editor asked; browsing them is exactly the reader that argument said didn't exist, and
their bodies are no heavier than the view definitions and trigger bodies this struct already
carried. They are held as **`Arc<RoutineInfo>`**, and `ObjectItem::Routine` carries the same
`Arc`, because `objects_all` is on the **keyboard-walk** path: `visible_nav_rows` rebuilds the
whole row list on every arrow key, through `object_groups` → `objects_all`, so an owned clone
there deep-copied every routine body in the database per keypress. Making the clone a refcount
bump is the fix rather than giving `nav_rows` a cheaper borrowed view of the objects, because
that walk has to stay bug-for-bug identical to the render and a second view of the same list is
exactly how the two drifted before. An enum's `values` are in
`enumsortorder`, not creation order, because that is the order comparisons use and the
order `ADD VALUE … BEFORE/AFTER` manipulates. A domain's constraints are `CheckInfo`s —
the same type a table's are, so `ddl::checks_equal` governs both. `SequenceOwner::internal`
separates an identity column's counter (droppable only with the column) from a `serial`'s
(an object in its own right), and `SequenceInfo::implicit_bounds`/`implicit_start` are why
`create_sql` emits a clean statement instead of restating six clauses that say nothing;
`last_value` is live state and deliberately takes no part in any diff. `qualified_ident` is
the one "qualify with whatever schema it has, then quote both halves" builder every one of
these (and `RoutineInfo::signature_sql`, `TriggerInfo::create_sql`'s table and
`TableInfo::create_ddl`) addresses its object through, and `find_by_ns` the one
namespace-lookup rule behind every `DbSchema::find_*`. **It names `public` like any other
schema**, and it used to leave it off through `sql_qualifier`, as the display name does —
but what it builds runs, and a bare name resolves through `search_path`, whose stock value is
`"$user", public`. Measured on PostgreSQL 16 with a schema named after the login: the enum
rebuild's `CREATE TYPE "mood"` (`recreate_type_sql`) landed in the login's schema and the
recast to `"public"."mood"` after it failed the plan; the routine editor's `DROP FUNCTION
"f"(integer)` dropped the login's own `f`; and `CREATE TRIGGER … ON "orders"` hung the trigger
on the login's `orders` (`public_objects_are_named_in_every_statement_that_runs`, each section
watched fail against the unfixed tree). `ddl::qualified` had made the same change for a plan's
DDL (under `schemaic-db`); `sql_qualifier` is now for display alone — `display_name` and
`dump::tables_in_namespace`, which matches display names.
**SQL Server's are `tsql_objects`, browsed through `ObjectItem::Tsql`** — the list read for the
dump, and kept out of `sequences`/`domains` for the reason given under `dump.rs`.
`TsqlObject::object_kind` browses a sequence as `ObjectKind::Sequence`, beside PostgreSQL's,
and the other three as kinds of their own (`Synonym`, `AliasType`, `XmlSchemaCollection`, under
`ddl.rs`). `objects_any`, `objects_where` and `find_object` chain them after PostgreSQL's
sequences for that kind and answer them alone for the other three, through the private
`tsql_of(kind)` and `find_tsql`, which takes the same `find_by_ns` rule as every other
`find_*`; `schemas()` counts their namespaces, so a schema holding nothing but a synonym is
still reachable in the tree. `ObjectItem::tsql()` is how a surface asks whether a row is one of
these — both of the dump's object loops ask it; the object editor's gate and the row's Edit
entry asked it too while there was no form for one, and no longer do (under
`object_editor.rs`). `synonym_fields` and `synonym_target_of_fields` carry a synonym's target
to and from the editor's four fields — server, database, schema, object, the missing outermost
ones empty — and are one another's inverse, a missing middle part kept empty both ways as
`synonym_target` keeps it (`a_synonyms_target_round_trips_through_the_forms_four_fields`). Four
fields rather than one dotted name, so no dotted-name parser exists.
`TsqlObject::detail` is the tree's summary beside the name: a sequence's type, an alias type's
base with ` NOT NULL` where it is not nullable, a synonym's target parts joined with `.`, and a
collection's schemas counted (`1 schema`, `n schemas`); `create_sql` is the dump's own
`TsqlObject::create_sql`, so a row's script and the dump's statement are one
(`each_object_is_browsed_and_found_as_its_kind`; live,
`standalone_objects_are_browsed_and_dropped`).
**`DbSchema::synonyms(database)` is what a synonym stands for, to the editor** — each synonym
with the local table or view behind it, which the catalogue and the completion index both read
(under `intel.rs` and `completion.rs`). A synonym lives in `tsql_objects`, not `tables`, so to
either it was no relation at all. The target resolves when it is `[schema, object]`,
`[database, schema, object]` naming the database the schema was read from, or a bare `[object]`
(or `db..object`) that **exactly one** table has — the server resolves an unqualified target
by the *caller's* default schema, which nothing here knows, so two candidates are no answer.
Anything else is `None`: another database, a linked server, a routine, a synonym of a synonym,
an ambiguous bare name. `None` is deliberate rather than a gap to fill with a best guess — a
guessed table would report columns the real target has not got, and the readers take `None`
as a name whose columns cannot be judged
(`a_synonym_resolves_to_its_local_table_or_to_nothing`).
**`DbSchema::database` is the schema's own address, and deliberately not part of the schema.**
It is *not* stamped onto the objects in it — `TableInfo::schema` is `None` on MySQL precisely
because a database *is* its namespace there — and it rides on the struct because exactly one
reader has to subtract an object's own address before comparing it against another database's,
and the model records that address nowhere else. MySQL's `collect_schema` stamps it with the
database it was handed, that being the engine where a foreign key's `ref_schema` and a view's
rewritten `view_definition` both come back qualified with the reading database. SQL Server's
`fetch_schema` stamps it too, on success, for a synonym's target that may name the reading
database — before it did, that engine's schema never said where it came from, and the
comparison's re-addressing of a synonym would have been inert. PostgreSQL's
`collect_schema` leaves it `None` on purpose — and says so in a comment on the literal, since
nothing that engine reports carries an address, so filling it would cost a `current_database()`
round trip to change no answer. `None` also means "the reader did not record it", which is the honest
answer for a hand-built schema, and a side with no address is compared exactly as it arrived
rather than guessed at — see `compare.rs`'s `as_read_from`.
**`DbSchema::namespaces` is the reader's own list of namespaces, empty ones included**, and
`schemas()` is that list unioned with every object's namespace. `schemas()` used to be derived
from the objects alone, so a namespace holding nothing did not exist to anything that asked: on
SQL Server a schema made with Create ▸ Schema never appeared in the tree, its Drop was offered
only while the schema held something — exactly when T-SQL must refuse it (Msg 3729; there is no
`CASCADE`) — and a comparison planned `CREATE SCHEMA` for a namespace the target had but held
nothing in, refused with Msg 2714 and the whole migration rolled back with it (review findings
S2-L1-01 and S2-L1-02; under `compare.rs` and `schema_tree.rs`). SQL Server's list is
`db::mssql`'s `NAMESPACE_LISTING` over `sys.schemas` — `dbo` (`schema_id` 1) and the user's own
(5 to 16383), so not `guest` (2), `INFORMATION_SCHEMA` (3), `sys` (4) or the nine `db_*`
fixed-role schemas (16384–16393); PostgreSQL's is `db/pg.rs`'s `namespace_names` over
`pg_namespace`, on the same `user_schema_filter` every object read uses, so an extension-owned
schema is kept as it is everywhere else. MySQL and SQLite leave it empty, having no such level,
and **so does the first-paint partial schema** `fetch_table_list` returns on every engine — so
nothing may read an empty `schemas()` as "this database has no namespace level" (under
`table_designer.rs`). Pinned by `schemas_lists_a_listed_namespace_with_nothing_in_it`; live, by
`an_empty_schema_is_listed_compared_into_and_dropped` on SQL Server 2022 and
`pg_catalog::an_empty_schema_is_listed_and_not_created_again` on PostgreSQL 16, both watched fail
with the fix stashed.
**`DbSchema::names_ignore_case` is whether two names differing only in case are one object
there**, and the comparison is its one reader (its `Names` fold, under `compare.rs`).
SQL Server's `collect_schema` asks the database's collation through `db::mssql`'s
`NAME_CASE_PROBE` — `COLLATIONPROPERTY(DATABASEPROPERTYEX(DB_NAME(), 'Collation'),
'ComparisonStyle')`, whose bit 1 is ignore-case: measured, `SQL_Latin1_General_CP1_CI_AS`
answers 196609 (bit set), `Latin1_General_CS_AS` 196608 and a `BIN2` collation 0. PostgreSQL's
reader sets it `false` explicitly; MySQL, SQLite and a hand-built schema take the default
`false`, which compares names exactly — what every comparison did before the field existed.
The probe is a `?` like the other catalogue reads in `collect_schema`, so a server refusing it
fails the schema load rather than falling back to exact names.
**`DbSchema::extension_routines` is the one field that carries names and nothing else**, and it
exists because two readers of `routines` wanted opposite answers. PostgreSQL only — the other
two engines have no such concept and leave it empty. The tree browses `routines`, which
`pg::routine_filter` deliberately strips of anything an extension owns; the typo checker reads
the same list, through the catalog's `known_idents`, and so squiggled every extension function
as a misspelling under correct SQL. Names alone is the cheapest thing that answers the checker
without reopening what the tree settled: no bodies, no arguments, no settings, nothing the
Functions folder would then have to hide again. See `intel.rs` above for the measurement, and
`db/pg.rs`'s `extension_routine_names` for the query.
**`DbSchema::extensions` is the extensions themselves** — `ExtensionInfo { name, version,
schema }`, PostgreSQL's alone and empty on the other three engines, browsed as
`ObjectItem::Extension` (`ObjectItem::extension()` asks whether a row is one). **An extension
is the database's, not a namespace's**: its name is unique in its database and `DROP EXTENSION`
takes no schema, so `schema` is only where its objects went (`extnamespace`) — a detail the
row's `detail()` shows as `<version> · <schema>`, never part of the address. `ObjectItem::schema`
therefore answers `None` for one, `objects_any`/`objects_where` see it with no namespace (so it
lists in a flat folder and never under the namespace it installed into), and `find_object`
finds it by name whatever namespace is asked. Its `create_sql` is `CREATE EXTENSION "x" SCHEMA
"s";` — the picker's statement plus the namespace, so a copy installs it where this one is —
and deliberately no `VERSION`, since pinning one the target may not ship is a refusal for a
detail nobody chose (`an_extensions_create_names_its_schema_and_no_version`).
**`has_objects_in` skips the database-wide kinds** (`ddl::ObjectKind::is_database_wide`), and
that is load-bearing: every PostgreSQL database has `plpgsql`, so counting it took the tree's
"No tables" hint off every empty database (`an_extension_belongs_to_the_database_not_a_namespace`
pins both halves). `AvailableExtension` is one row of `pg_available_extensions`, and
`installable_extensions(available, installed)` is the Create extension picker's list: a row
the view marks installed is dropped, **and so is any name `installed` lists**, so a picker
opened against a tree read a moment before a `CREATE EXTENSION` elsewhere offers neither copy
of the answer; sorted and de-duplicated (`installable_extensions_leave_out_what_is_installed`).
**`SchemaState::begin_refresh` is why a refresh doesn't blank the tree.** Re-introspection
is whole-database on both engines and always will be — the cost is ~10 catalogue
round-trips, not the rows, so scoping it to one table would optimise the term that doesn't
dominate (measured: 48 ms for 600 tables / 12.6k columns on MySQL, 134 ms on PG, locally).
Dropping to `Loading` for that window replaced every table and column row with one
"Loading" row, after *every* DDL apply as well as every manual Refresh. So a database that
has something on screen keeps it, and the method returns **`Option`**: a floem signal never
dedups, so writing an equal `Loaded` back would dispose and rebuild the very subtree the
refresh is meant to leave alone. Not writing is the only way to keep it. Nothing marks the
row as busy meanwhile, deliberately: at these durations an indicator is a glyph flickering
for a frame or two, which reads as a rendering fault rather than as progress.
**`READ_NOTICE_DELAY` and `report_read` are the other half of that decision, not a reversal
of it.** Keeping the rows also means nothing below the header said a re-introspection was in
flight, so on a database big enough for the read to take seconds pressing Refresh read as the
app having ignored the click; the panel's *title* reports that case instead
(`schema_tree::schema_panel`). `report_read(reading, for_how_long)` is
`reading && for_how_long >= READ_NOTICE_DELAY`, and the delay is 400 ms precisely because of
the two figures above — an instant header indicator would inherit the rows' flicker exactly,
and worse, since applying any DDL refreshes too, so it would strobe after every schema edit.
Both terms are load-bearing: a read that has *ended* says nothing however long it ran, so the
notice cannot outlive the work. `read_notice_tests::the_delay_clears_every_measured_local_refresh`
asserts the constant exceeds both 48 ms and 134 ms, so lowering it has to argue with the
measurement rather than quietly reintroducing the flicker.
**`report_read_since` is how the panel reaches it, and handing the constant straight across is
what it prevents.** Nothing cancels the notice timer when the read it was armed for lands, so the
busy period the timer finds on screen need not be the one it was armed for: expand one database,
let it land at 150 ms, expand a second at 300 ms, and the first timer fires at 400 ms over a read
that is 100 ms old and announces it. `report_read_since(reading, began, now)` asks that period's
own age — `began` is when the period **currently** on screen started — and answers `false` for
`began: None`, there being nothing in flight to date a notice from. The call site has a *timer*
rather than a duration, and passing `READ_NOTICE_DELAY` itself degenerated `report_read`'s
duration term to a constant `true`, leaving three of that function's four tests asserting over
values the composition could never produce: the pure-function-composed-with-its-caller seam the
working rules name. `report_read` is reached only through `report_read_since` now.
**There are two refresh paths and both go through it**, via the app's one
`start_fetch` (`FetchSchemaFn`): the per-database one
(`refresh_db`) and the connection-wide one (`load_schema`, the SCHEMA header's Refresh),
which additionally **reuses the `ConnNode` of every database that is still there** — same
`schema` signal, so the rows survive, and same node id, so the `dyn_stack` doesn't rebuild
a surviving database at all. Reuse means the node `Scope` is kept rather than replaced, so
it is gated on `Connection::targets_same_server` (id + engine + host/port/user + the SSH
hop and the jump hosts before it; *not* the password, name, colour or `read_only`): a **switch** must still clear and
dispose, or the tree shows one server's databases while another's load — and a saved
connection repointed at a new host keeps its id, which is the case an id comparison alone
gets wrong. **Keeping the rows means `Loaded` stopped meaning *current*, and two things
follow.** `main.rs`'s `fetch_landing` stamps each per-database fetch so an older one can't
overwrite a newer (`try_update` guards a *disposed* scope, not a superseded fetch — press the
connection-wide Refresh, apply an `ALTER`, and the pre-`ALTER` snapshot landed last and stayed,
with nothing to detect it and no further refresh scheduled). **That stamp is read after the
fetch, though, and there is a second question to ask before it.** The introspection semaphore
(`main.rs`'s `introspect_permits`) is process-wide and `tokio::sync::Semaphore` is FIFO, so
opening a 200-database connection and then switching to a three-database one left the incoming
connection's three tasks queued behind the outgoing one's remaining ~196 — each of which still
opened a connection to the **abandoned** server and read a whole database's catalogue before
`fetch_landing`, consulted only once `fetch_schema` returns, threw the result away. Every
database on the new connection sat at "loading" until the old load drained: ~15 s at 300 ms a
database, ~100 s at 2 s. So a queued fetch asks `fetch_still_wanted(spawned_at, now)` **after
acquiring its permit and before the read**, against `schema_epoch()` — an `AtomicU64` bumped by
`clear_schema_tree`, which is every connection switch. Atomic and process-wide because that is
what the queued task can reach: the per-node `fetch_seq` stamps live in an `Rc<RefCell<…>>` on
the UI thread while the fetch runs on the tokio runtime. A superseded task releases its permit
immediately and the queue drains at once; `schema::introspection_order`'s whole purpose — put
what is on screen first — only orders within one enqueue, and could not survive the queue
outliving the connection it was enqueued for. And `ConnNode::refreshing` makes
"a re-introspection is in flight" askable, which `table_designer::loaded_table` — the one
funnel all four schema editors go through — answers `None` for: seeding a draft from a
pre-apply `TableInfo` is what makes MySQL's `MODIFY COLUMN` silently restate the old column
definition, and `risks()` discloses nothing because from the plan's view nothing changed.
**`object_editor` had a private near-copy of `loaded_schema` that did *not* apply that filter**,
and it fed `ddl::type_dependents` — the columns a rebuild would re-cast — so within the window
the refresh holds a superseded schema `Loaded`, the type editor built its rebuild against
columns an applied `ALTER` had already moved past. It goes through the funnel now and
`open_for_object` **refuses to open** on its `None`, which is what `open_for_table` already
does for the same window. The alternative — unify and let the call site take the funnel's
`None` as an *empty* dependent list — was rejected as the worse of the two wrong answers: a
stale list at least resembles the type's real dependents, while an empty one emits a rebuild
that re-casts nothing and looks like a clean plan. The refusal costs one round trip's worth of
a menu entry that appears to do nothing, which is the same price every other editor pays.
`object_editor::one_funnel_gate` is what keeps the copy from coming back: the decision is a
`return` in a function that opens a modal, so there is no value to assert on, but there is a
private `fn loaded_schema(` to refuse and a shared call to require — and that is what actually
regressed.
The plan behind the reuse is `plan_nodes`, pure and tested: it decides that a dropped and
re-created database gets a **fresh** id rather than colliding with a live node, that reordering
the server's list renumbers nothing (the tree keys on id), and that a reload against an empty
list still works.
**`departed_nodes` is the complement `plan_nodes` never answered, and it is the half that holds
memory.** The nodes in `existing` that no plan keeps are the databases dropped from another
client between two reloads; on a reload the node scope is deliberately kept alive, so each of
those vanished from `db_nodes` with its `RwSignal<SchemaState>` still installed in the surviving
scope — and a `Loaded` holds an `Arc<DbSchema>`, every table, column, index, key, view, check and
trigger of that database, unreachable and retained until the app exited or a connection switch
happened to replace the whole scope. One full model retained per scratch database per refresh.
The load disposes those nodes' own scopes **deferred**, and only when the scope above them is
staying: on a switch the existing `old.dispose()` already takes the whole generation, children
included. That per-node scope is `ConnNode::cx` — see the child-`Scope` invariant, which this
type was the last exception to.
**And emptying the tree is one closure, `clear_schema_tree`, because three steps that must not
come apart were spelled out at three sites and only one of them had all three.** Of the three
`db_nodes.set(Vec::new())` sites only the failed-connect arm in `load_schema` also forgot
`nodes_conn` and disposed `nodes_scope`. So a switch the user reversed before the first load
landed — seconds wide over a tunnel — left `reload` reading true against an already-empty node
list, `kept_scope` `Some`, the deferred dispose skipped, and the whole set rebuilt inside the
scope that still owned the first one; deleting the last connection orphaned one outright. The
rule is `rearm_activity`'s, and the gate is that the signal has no other writer:
`app_tests::emptying_the_schema_tree_lets_go_of_its_scope` asserts the closure still does all
three and that nothing else writes `db_nodes` empty.
**`unasked_reads` is what a load actually enqueues, and `SchemaState::Unread` is what it leaves
behind.** It is `introspection_order` over the `ListedDatabase`s `Db::list_databases` returns,
with every `single_user` one dropped — not even the active database or an expanded one is
excepted. A read is a connection *into* the database and single-user mode admits one (SQL
Server's `SINGLE_USER`, the one engine that sets the flag; the listing is under `db::mssql`).
A load runs on app start, on Refresh, on a connection switch or edit and after every
server-scope DDL — the `ALTER DATABASE x SET SINGLE_USER WITH ROLLBACK IMMEDIATE` itself
included — so the load most likely to land between that `ALTER` and the administrator's
`RESTORE`, `DBCC CHECKDB` or `USE x` is the one the `ALTER` set off, and it took the slot (Msg
924/3101). An expanded node is no exception for the same reason: the reload after the `ALTER`
finds it expanded. `load_schema` sets a listed single-user node still at `Loading` with no read
in flight to `Unread`, rather than leaving it saying "Loading" for ever; one that already has
rows keeps them and is not re-read by the connection-wide reload — its own Refresh re-reads it.
`begin_refresh` answers `Unread` with `Some(Loading)`, as it does `Failed`, so the tree's *Read*
(`schema_tree.rs`'s `unread_row`) is the per-database `refresh_db` through the one
`start_fetch` like any other. The cost is that an unread database has no model, so completion
and `intel`'s catalogue know nothing of it until it is read.
`a_load_reads_no_single_user_database_unasked` pins the filter, the order of what is kept and
the `begin_refresh` arm.
