# `core/used_by.rs`

`used_by.rs` — **what depends on a table, a view, a column or a routine**: the views, routines,
triggers, events, foreign keys and table expressions that would break, or go with it, if it
changed — behind the schema tree's *Used by* (`ui/used_by_view.rs`), a table Drop's preview
warning, and the *Used by* block of a rename's or column edit's preview.
`used_by(schema, target, dialect, database) -> Vec<Dependent>` takes a `Target` —
`Relation { ns, name, column }` or `Routine { ns, name }` — and returns each `Dependent` with its
`DependentKind` (`View`, `Routine`, `Trigger`, `Event`, `ForeignKey`, `Expression`), its name as
the tree shows it (`schema::display_name`), a `detail` (the table a trigger or key is on, and the
key's columns) and `goes_with_it`, sorted by kind and then name.
**No round trip, and no dependency catalogue**: it is a scan of the loaded `DbSchema`, which
already holds every view's definition, every routine, trigger and event body and every foreign
key (`body_search.rs`, above, reads the same fields), so the answer costs nothing and is the same
on every engine. The module doc gives the reason not to have asked the server instead: only SQL
Server keeps a dependency catalogue worth the name, and a PostgreSQL function body is opaque to
`pg_depend` anyway.
**A body uses the target when it names it as it would be resolved**, which is the question
`core::dump` already asked before leaving an object out of a file, so it is the same function:
`dump::names_object` (`pub(crate)` for this) over `intel::code_mask` — a whole word in code, not
in a string or comment, its qualifier read so `sales.emp` is not `dbo.emp`, a bare name reaching
its own namespace or the default one (`another_namespaces_same_name_is_not_the_target`). This
is not `body_search`'s substring match, deliberately the other way round: that one is meant to
find a mention in a comment, and here a comment that names a table does not depend on it. A
**column** is used where a body that uses its table also names the column word — the nearest a
text scan gets, and it errs toward listing: a same-named column of another table in that body
counts, and a dynamic-SQL string naming the object is not seen at all, which the modal says
under every answer (`a_column_is_used_where_its_table_and_its_name_both_are`).
**A foreign key is exact** — its `ref_table`, in its `ref_schema` or else its own table's
namespace, and for a column target its `ref_columns`.
**`database` is the one the schema was read from, and on MySQL it is the only qualifier there
is.** Where `ddl::ref_schema_is_database` says yes, the catalogue writes the database into every
view body (`` `shop`.`orders` ``) and into every key's `ref_schema`, while the tables themselves
carry no namespace — so until `used_by` was handed the database, no MySQL or MariaDB view or
foreign key was ever found using anything, in the modal, a drop's warning or a preview's *Used
by* block alike. A qualifier equal to `database` now counts as this database, and one naming
another database does not (`a_mysql_catalogue_qualifier_naming_this_database_is_a_use`). **A
body's qualifier asks `ddl::body_qualifier_is_database`, a key's `ref_schema` still asks
`ref_schema_is_database`**, and the two differ on SQLite: its `main.orders` names a database
while its tables and keys carry none, so with the key predicate alone it said *"Nothing in main
uses it"* of a view and a trigger the drop then broke
(`a_sqlite_qualifier_naming_this_database_is_a_use`). SQL Server's bracketed `[dbo].[orders]`,
three-part `shop.dbo.orders` and a bare name in a `sales` routine reaching `dbo` are pinned as its
catalogue writes them (`sql_server_qualifiers_are_read_as_its_catalogue_writes_them`).
**A trigger is listed three ways**: one *on* the table, with `goes_with_it`, since dropping the table drops it; one on another table
whose body uses the target; and, for a routine target, a PostgreSQL trigger whose
`TriggerAction::Function` names it — that name is emittable SQL, quoted and qualified outside
`public`, so `function_is` reads it with `schema::split_type_name` rather than comparing strings
or splitting on the last dot — a quoted `"audit.v2"` is one name, which `rsplit_once('.')` read
as `v2` in `audit` — and a name it cannot read is no routine
(`a_qualified_pg_trigger_function_matches_only_its_schema`,
`a_quoted_trigger_function_with_a_dot_is_still_the_routine`). **For a column target, a trigger
on the table is listed when its body names the column alone** (`uses_column_of_own_row`), and
is not `goes_with_it`: it reads the row as `NEW.total` or `inserted.total` and never names the
table, so while a body had to name the table first, no MySQL, SQL Server or SQLite trigger on
the table was ever listed for a column, and *"Nothing uses it"* preceded a rename or drop that
broke every insert (`a_trigger_on_the_table_is_listed_for_a_column_its_body_uses`). A
PostgreSQL trigger has no body to read, so it is the **function** a trigger on the table runs
that is listed, as a `Routine`, when its body names the column — and not one a trigger on
another table runs (`a_pg_trigger_function_on_the_table_is_listed_for_a_column_it_uses`). The
same text-scan trade as any column: a same-named column read off another table in that body
counts. A routine target also finds the
tables whose column defaults, generated columns or checks call it, as `Expression`
(`a_routine_is_used_by_callers_expressions_and_pg_triggers`). A table's key onto itself is
`goes_with_it` too.
`drop_warning(deps)` is the sentence a drop's preview adds — *"Used by 2 views, 1 routine and 2
foreign keys: a, b, c, d and 1 more."*, counted by kind and naming the first four — and **leaves
out every `goes_with_it` dependent**, returning `None` when nothing else is left: a trigger on
the dropped table goes with it and is no breakage, so the sentence is about what the drop
breaks (`a_trigger_on_the_table_is_listed_and_one_elsewhere_that_writes_it`,
`the_drop_warning_counts_by_kind_and_names_the_first_few`). It is `drop_listing(deps)`'s
English: that returns the sentence as a value, a `Listing { kinds, names, more }` — the count
per `DependentKind` in the order met, the first four names, how many are left — whose
`message()` is the *"2 views and 1 routine: a, b and 3 more"* part and `drop_message()` the
*"Used by …."* around it, so the preview words the same value in the interface's language
(`used_by_drop_text`, under `ddl_preview.rs`) while core's tests still read the English.
**A rename or a column edit asks the same question**, since it breaks a dependent as surely as
a drop does. `touched(&[Change], table) -> Vec<Touch>` reads off a plan the names it takes away
or alters — a `Touch { column, kind }`, `column: None` for the object itself, `TouchKind` one of
`Rename`, `Recreate`, `Drop` or `Retype`: `RenameTable`, `RenameView` and `RenameRoutine` touch
the whole object, `DropColumn` drops a column, and `AlterColumn` is a rename when the name
differs, a retype when only the type does — compared case-insensitively, so `int` → `INT` is
no change — and nothing otherwise (`an_edit_that_keeps_name_and_type_touches_nothing`). A column renamed
*and* retyped is **one** rename, the name being what breaks a dependent first
(`a_renamed_and_retyped_column_is_one_rename`). **A whole-object drop is deliberately not
here**: its preview already asks through `drop_warning`, and listing it twice would put the
same dependents under two headings. **A rename by building anew is a `Recreate`**, though no
change in the plan says "rename" — which is why `touched` takes the plan's `table`, the name
the server holds: a `ReplaceView { recreate: true }` whose draft is named otherwise (a view
rename on SQL Server or SQLite, which have no verb for it), a `ReplaceRoutine { recreate: true }`
whose draft is named otherwise than its `server` (a routine rename on MySQL or SQL Server), and
a column rename in a plan holding a `RebuildTable` — SQLite's rebuild replays the stored text
under `legacy_alter_table`, so nothing names the new column. Before it, such a rename touched
nothing and the preview carried no *Used by* lines; checked live on SQL Server 2022, `DROP VIEW
v1` succeeds under a `v2` not schema-bound to it, and `v2` then fails with Msg 208/4413
(`a_view_renamed_by_dropping_and_creating_it_is_warned_about`,
`a_routine_renamed_by_dropping_and_creating_it_is_warned_about`).
`change_warning(touch, object, deps, dialect)` words one —
*"orders is used by 2 views and 1 routine: a, b, c. They still name orders after the
rename."*, *"Column note is used by …. They break when it is dropped."*, *"… Check them against
its new type."* — and is `change_note`'s English, a `UsedByNote { subject, listing, then }`:
`subject` a `UsedBySubject` (`Column(name)` or `Object(name)`), `then` a `UsedByThen`
(`StillNames(old)`, `BreaksOnDrop`, `CheckNewType`), `message()` the sentence. Its `listing` is
the private `listing`'s `Listing`, the one `drop_listing` hands out, which `drop_warning` was
factored onto with its text unchanged, so the `goes_with_it` rows drop out here too: a trigger
on a table follows the table's rename on every engine
(`the_change_warning_leaves_out_what_goes_with_the_object`).
**A rename's sentence leaves foreign keys out too**, and a drop's or a retype's keeps them: a
key is held against the object, not its name, and was checked live to follow both a renamed
referenced table and a renamed referenced column on MariaDB, MySQL 8, PostgreSQL, SQL Server and
SQLite — so *"They still name orders after the rename"* was false of every key it listed
(`a_rename_does_not_claim_a_foreign_key_keeps_the_old_name`). **Which other dependents an
in-place rename carries is `ddl::rename_follows(dialect, DependentKind)`**, and the sentence
leaves those out as well — it used to leave out only keys, on every engine, and so told the user
to fix what was not broken. A key follows everywhere; PostgreSQL holds a view, a default, a
generated column, a check and a trigger's `EXECUTE FUNCTION` and `UPDATE OF` by OID or column
number, so all follow, while a routine body is `prosrc` text and does not; SQLite's `RENAME TO`
and `RENAME COLUMN` rewrite its views and triggers; MySQL and SQL Server rewrite nothing. Checked
live on PostgreSQL 16 (a table rename, a column rename and a function rename — the plpgsql bodies
kept the old name and failed) and SQLite 3.45 (views and triggers rewritten, `main.orders`
included) — `a_rename_warns_only_about_dependents_that_do_not_follow_it`,
`a_pg_routine_rename_warns_only_about_bodies_that_call_it`,
`which_dependents_follow_a_rename_and_which_qualifiers_name_the_database`. **A `Recreate` does
not ask it**: building anew carries nothing across but a key, which names a table rather than a
view or routine and is put back against the new table, so only keys drop out and the sentence
is the same *"They still name X after the rename."*
`change_notes(&DbSchema, &ChangeSet, database)` is the composition the preview calls —
`change_warnings`, its English, is what core's tests read — and it asks **under the name before
the plan** — the set's `table`, which is the server's name, a
renamed routine's `from`, or a routine re-created under a new name's `server` — because the
loaded schema still holds the old one and a dependent still names it
(`a_plan_is_warned_about_by_what_uses_the_names_it_changes`,
`a_routine_rename_asks_after_the_routine_by_its_old_name`). A routine rename is therefore seen
both as a `RenameRoutine`, which `diff_routine` emits on PostgreSQL alone and only when the
signature is unchanged, and as the recreate it is folded into everywhere else
(`supports_routine_rename`).
