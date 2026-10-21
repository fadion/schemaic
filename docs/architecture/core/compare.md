# `core/compare.rs`

`core/compare.rs` — **two databases, object by object**, and a chosen subset of the differences as
one migration. The pure half of schema compare (the UI half is `ui/compare_view.rs`): no DB, no
view code, nothing here runs anything (98 unit tests).
**The differ is the comparator.** An object is `ObjectStatus::Differing` precisely when
`ddl::diff` — or `diff_view`/`diff_trigger`/`diff_routine`/`diff_event`/`diff_enum`/
`diff_domain`/`diff_sequence`/`diff_tsql_object` — hands back a non-empty `ChangeSet` for it, and
`status_of` reads that set rather than comparing fields beside it. That is what keeps a tree's
verdict and the plan's contents one fact: a release that teaches `diff` about a column attribute teaches this
module in the same commit, an object called *differing* can never plan nothing, and — the worse
half — nothing can be called identical while a difference the differ can already see is left
out of the migration. `the_status_is_the_differs_verdict_and_nothing_else` asserts
`changes.is_empty() == (status == Same)` over **every** entry of a mixed comparison rather than
over one, because the failure it guards is a status computed next to the differ instead of from
it. `counts` is the per-status tally a header would read; `differences` is the entries that
would contribute statements, in plan order.
**Right is the source of truth, left is where the DDL runs**, and that fixes every direction in
the module: an object only the left side holds is a `DROP`, one only the right holds is a
`CREATE`, and the `Target` the table differ takes is built from the **left** schema's
`ServerFlavour` — MariaDB's `MODIFY COLUMN` destroys a column's own `CHECK`, and it is the left
server that will read the statements (`the_left_sides_flavour_is_what_the_plan_targets`). That
is the only path carrying a flavour; every other builder takes the dialect alone and leaves
`ChangeSet::flavour` `Unknown`, which is the reading the single-object editors already get and
costs nothing here because the divergence is `ALTER TABLE`'s. Swapping the pair is the caller's
job and reverses all of it.
**The right side is read as if it had come from the left side's database, before anything is
compared.** On MySQL two of the fields the differ reads carry the name of the database they were
read from rather than anything about the object: a foreign key's `ForeignKeyInfo::ref_schema` is
`REFERENCED_TABLE_SCHEMA`, which on that engine *is* the database, and a view's
`TableInfo::view_definition` is the server's rewritten body, qualified throughout. Left alone,
every table holding a key and every view came out `Differing` between two identical databases —
and the half that costs data is that the plan runs against the **left** database while naming the
**right** one: `ADD CONSTRAINT … REFERENCES <right>.parent` puts the left database's referential
integrity in another database, and `CREATE OR REPLACE VIEW` re-points the left view at the right
database's rows. Neither is named anywhere in the preview, and `destructive()` is empty for both.
SQL Server has a third such field: a synonym's `TsqlObjectKind::Synonym::target`, which SSMS's
*New Synonym* dialog fills in three-part, so `[shop_dev].[dbo].[orders]` read from `shop_dev` is
that database's own table — the reading `DbSchema::synonyms` takes of it — yet it compared
`Differing` against `[shop_prod].[dbo].[orders]`, and the plan's `DROP` + `CREATE` sent every
read and write through the left synonym to the right database's table.
`as_read_from` is the pass that closes it, and it **re-addresses rather than strips**: MySQL
allows a key into another database, so one of those genuinely *is* a difference, and `ddl`'s
foreign-key rule that an absent namespace matches an explicit one would have made a
cross-database key compare equal to a local one. A re-addressed right side is also the side the
plan is *built from*, so the statement that comes out already names the database it will run
against instead of leaving the server to guess. The rewrite is `requalify`, over
`intel::code_word_hits` — the one boundary lexer — and it takes only a hit with no `.` in front
of it and a `.` behind it, so `` `t`.`shop` `` stays the column it is
(`a_column_named_after_the_database_is_left_alone`). Which of the two fields to re-address is
asked of two capability predicates rather than of the engine (`ddl::ref_schema_is_database`,
`ddl::view_definition_is_qualified`). The synonym asks none, the field being T-SQL's alone by
type: a target whose database part names the right side's database, case-insensitively, becomes
the left side's database — or, where the left recorded none, the bare two-part name that means
"this database". `synonym_database` reads that part off a three-part target, or a four-part one
whose server part is empty; a linked server's database of the same name is another server's and
is left as read (`a_synonym_naming_its_own_database_is_compared_as_its_own`; live,
`a_synonym_naming_its_own_database_compares_the_same`). The whole pass borrows rather than
clones when there is nothing to do — every PostgreSQL and SQLite comparison, a SQL Server one
with no synonym naming its own database, a pair whose two databases are named the same, and any
side that did not record where it came from
(`the_foreign_key_pair_differs_only_in_where_it_was_read_from`). Where a schema was read from is
`DbSchema::database`, which the model records nowhere else. The **left** side is never touched:
it is the target, and it is already in its own terms. **Those terms still spell a synonym's
target two ways**, three-part naming its own database or the bare two-part name, and only the
source's was re-addressed: a target synonym SSMS wrote as `[shop_dev].[dbo].[orders]` against
a source's `[dbo].[orders]` — one table — compared `Differing`, and the plan dropped and
re-created it, or the replace guard refused it over its grants (CMP-07). `tsql_entry` asks
`same_synonym_target`, which reads each side's target naming the target database as its last
two parts and then compares part for part as the target's names do; when they agree the source
takes the target's spelling before the differ sees it, as it does a case-only difference. The
reading is the comparison's alone — a real difference is still created with the source's target
as written (`an_own_database_synonym_target_compares_the_same_on_either_side`; live,
`a_synonym_naming_its_own_database_compares_the_same`, three-part on one side and two-part on
the other, both ways round).
**A `DEFINER` is deliberately not compared at all**, on either side and on **every carrier that
has one** (`without_definer`) — a view's `view_options`, a `TriggerInfo`, a `RoutineInfo` and an
`EventInfo`, four separate fields the pass clears, because each is restated by its own emitter
and each is compared by a whole-struct equality. This is a different judgement from the one
above — the same "the model cannot vouch for this" reading `CompareEntry::uncertain` takes of a
lossy index, a definer being a *server* account that does not compare across two servers. And
the consequence of comparing it is not a spurious row. The view is the worked case: the `CREATE
OR REPLACE DEFINER = <the other server's account> VIEW …` that follows is *accepted* by the left
server, after which every `SELECT` on the view is `ERROR 1449 … does not exist` and the view is
permanently unusable (measured on MariaDB 10.11.14). The other three end somewhere worse,
because a two-server comparison reported every one of them Differing although the bodies were
byte-identical, and the plan is ticked by default and built for the **left** database: without
`SUPER`/`SET_USER_ID` the `CREATE` is refused `ERROR 1227` and, MySQL DDL not being
transactional, the `DROP TRIGGER` above it has already run, so the trigger is simply gone; with
it, every `INSERT` into the table then fails 1449 and the table is unwritable, a routine gives
the same 1449 on every `CALL`, and an event never fires again — an event having no caller, so
the definer's rights are the only rights its body runs with. The other side of the trade is
named rather than hidden: a deliberate definer change is not migrated, and a view replaced for
some other reason takes the running account — a feature not offered rather than an object
destroyed (`a_view_definer_neither_differs_nor_rides_into_the_statement`,
`a_trigger_routine_or_event_definer_neither_differs_nor_rides_into_the_statement`). `definer` is
`None` on every engine but MySQL's, so this needs no predicate of its own, and the pass borrows
when nothing on that side carries one.
**One dialect, so one engine.** `SchemaComparison::of(left, right, dialect)` takes a single
`SqlDialect` because a `ChangeSet` carries one, and a `DbSchema` has no dialect of its own to
disagree with it (only `flavour`). That single parameter is the honest encoding of a real limit
rather than a missing feature — type names, defaults and index shapes do not map between
engines, so a MySQL-to-PostgreSQL plan would be confidently wrong exactly where it mattered —
and both callers pairing two connections — the app's compare overlay and `schemaic compare`
(`cli/run.rs`'s `run_compare`) — refuse the mismatch before it reaches here, through
`comparable`. That refusal is **by dialect and not by engine**: MySQL and MariaDB speak one
dialect and compare perfectly well, the difference between them riding on the schema's
`ServerFlavour`, while the three dialects do not map onto one another at all. It is asked before
either schema is read, since two full round trips to then say the pair can't be compared would
be the same answer, slower.
**`SchemaPlan` is a `Vec<ChangeSet>`, not a wider `ChangeSet`.** A set carries one
`table`/`schema`/`dialect` and most `Change` variants are addressed at that one name instead of
carrying their own, so a single set *cannot* hold edits to two objects. Widening `Change` would
put a second notion of "which object" beside the one every emitter already reads, so the
aggregate stays a list of single-object sets and **no statement is built here** — `emit` is
exactly its sets' statements in order, and the three script forms are that same list under a
header (`the_plans_statements_are_exactly_its_sets_statements_in_order`). A `Same` entry is never
planned however permissive the `include` predicate is, its set holding nothing to run. For the
UI stage: `ui::DdlPreview` is already flat
(`statements`/`destructive`/`withheld`/`omitted`/`script`), so
it can take a whole plan and the plan answers every field of one — `ddl_preview::preview_of` is
the single-`ChangeSet` shape that would have to grow, and it fills `omitted` with an empty vec:
a designer edit is about one object it either can or cannot change, so nothing can be left out
of it.
**The answers that are more than a concatenation are each load-bearing.** `editor_script`
puts `ddl::withheld_header` above the statements, over the **union** of its sets' omissions: a
`join("\n\n")` dropped the "INCOMPLETE" preamble a single set's own script carries, so
a copied plan read as complete while `emit` was silently leaving a lossy index's `UNIQUE` out of
a SQLite rebuild (`a_plan_that_withholds_a_statement_says_so_above_its_script`,
`a_plan_that_withholds_nothing_has_no_header`). That header is a `pub` free function in
`ddl.rs` over any `&[String]` — `ChangeSet::withheld_header` delegates to it — because a second
copy of that sentence is a second thing to keep true. The statements under it come from
`ddl::client_script` — and **a compare plan carries no MySQL body at all**, which is the
opposite of what this passage used to claim ("several such bodies in one plan is exactly this
module's case"). `CompareEntry::needs_source` had already made that false: `plan` excludes every
MySQL routine, trigger and event being created or redefined and discloses them through
`omitted`, so only a `DROP` gets through and a `DROP` has no body. `client_script`'s `DELIMITER`
branch is therefore unreachable from here, and its terminate-every-statement rule is a no-op in
practice — on the two engines whose bodies *do* reach a plan, the emitters already end each
statement in `;`. The call stays because this is not the place that gets to know that, and
because the day a re-read body reaches a plan — the fix `needs_source` defers rather than
forecloses — the wrapping has to be here already. The engine it asks is
`SchemaPlan`'s own `dialect` field rather than the first set's, so an empty plan still answers as
the engine it was built for.
`export_script` is `ChangeSet::export_script`'s counterpart and is what a preview's `script`
field must be given — and what `schemaic compare` prints, the one way a plan leaves without the
preview, as text and never run (`cli/compare.rs`): it returns `editor_script` unless some set holds a `ddl::is_account_change`,
in which case each set writes its own redaction notice beside its own statements under the one
withheld header. For a comparison the two are byte-identical, no builder it calls producing an
account change — which is exactly why it exists as a function rather than as a sentence saying
so. The property is "no plaintext password leaves this modal", and a builder that one day puts
an account change in a plan should inherit it instead of having to notice the prose.
`risk_heading` takes the **stronger** of its sets' answers by asking `ChangeSet::risk_reversible`
— extracted `pub` for this caller rather than having the aggregate compare headings as strings,
which would agree with every set the day one is reworded. That answer is itself `pub` now, as
`risk_reversible()` (every set's, and no foreign-key cycle), because the preview words the
heading from the bool rather than taking core's English; `risk_heading` is
`ddl::risk_heading_for` over it, the one place both headings' English lives. It only titles a non-empty
`destructive()` list, so a create-only plan's heading is never read at all, and since
`Change::risk_is_reversible` is true of the four account changes alone, a compare plan with a
risk block to head always reads "This can't be undone".
**`summaries`, `destructive` and `unsupported` name the object on every line**, prefixing each
with `schema.object` through the private `SchemaPlan::subject_of` — the three answers that are
prose rather than SQL. A single set's lines are read under a title naming that one table, so
`Change::DropTable` says "Drops the table and every row in it" and leaves the *which* to the
heading; a plan's title is a **count**, so eight dropped tables produced that identical sentence
eight times over "12 objects", on the one surface standing between someone and an irreversible
`DROP`. `unsupported` is qualified for a related reason: a non-empty list disables Apply for the
*whole* plan, so over two hundred objects the line has to say which tick to clear before Apply
comes back.
**Entries come back from `of` already in plan order**, so group by `CompareEntry::kind` for
display rather than re-sorting in place. `phase` maps (status, kind) onto one number:
dependents dropped first, then types created or altered, then tables created **and altered in one
phase**, then tables dropped, then dependents created or altered, then types dropped — and `Same`
sorts past every difference, so a hundred untouched tables cannot land between two statements
that depend on each other (`untouched_objects_do_not_disturb_the_order_of_the_differences`).
**A replaced type is the one entry whose phase is not its (status, kind)'s.** An alias type,
XML schema collection or sequence the differ has to drop and create again
(`Change::ReplaceTsqlObject`) ran in phase 1, ahead of the table drops in the same plan, and its
`DROP` was refused (Msg 3732) while a table the plan was about to drop still used it — though
the plan's later statements removed the only user (S4.2-L1-03). The sort key's `late_replace`
gives such an entry phase 3 instead, behind the table drops and the phase-2 alters that may move
a column off it — `kind_rank` negating a drop's ordinal is what puts the drops first inside that
phase — **unless a table on the right uses it**, a column whose type or default `mentions` its
name (whole word, case aside), whose create needs it first; that one stays in phase 1, and a
false match only keeps the old order. The test is `is_type`, so a re-pointed synonym, also
replaced, takes phase 3 as well (`a_replaced_type_waits_for_the_table_drops_that_release_it`;
live, `a_comparison_orders_shared_names_and_folds_case` on 2022).
**The creates and the alters share a phase because neither order between them is right.** Split,
every `CREATE TABLE` ran ahead of every `ALTER`, so
`CREATE TABLE child (… REFERENCES parent(code))` was emitted before the
`ALTER TABLE parent ADD code` it names — refused by the server, with the statements before it
already applied and no DDL rollback on MySQL — and swapping the two phases only moves the failure
to the other shape, an `ALTER` adding a foreign key onto a table the plan is about to create.
There is no need to pick one: `fk_rank` over the **right** schema answers both, that being the
schema a create and an alter are equally moving toward
(`an_alter_that_a_create_depends_on_runs_before_it`,
`a_create_that_an_alter_depends_on_runs_before_it`), and the alter side gains a foreign-key order
it previously had none of at all
(`two_altered_tables_are_ordered_by_their_foreign_keys_not_their_names`).
**Inside a phase the
order is `CompareKind`'s declaration order** — `kind_rank` is that ordinal, **negated for a
drop**, because dropping runs the dependency order backwards — then a foreign-key rank from
**`dump::order_tables`** (`fk_rank`, with `reverse`
inverting the ranks for the drop phase) rather than a second topological sort over the same
edges, then `CompareEntry::key` as the last tie-break so two runs of one comparison read the
same. **`fk_rank` ranks views as well as tables**, which it did not: `chosen` filtered them out,
so the half of `order_tables` written for view dependencies never ran and two created views fell
through to their names — `CREATE VIEW a_totals AS … FROM z_detail` above the statement creating
`z_detail`, which is the ERROR 1146 that half exists to prevent
(`a_view_is_created_after_the_view_its_body_selects_from`). The rank lookup covers
`CompareKind::Table | View` across `OnlyRight | Differing | OnlyLeft`; every other kind ties at
zero and falls through to the name. The ordinal is in the sort key because the alphabetical
fallback **is not a dependency
order**: sorting a phase on `key` alone put `domain:` ahead of `enum:` and emitted
`CREATE DOMAIN … AS app.mood` above the statement creating `mood`, and `trigger:` ahead of
`view:` for the same reason (`a_new_domain_is_created_after_the_enum_it_names` and
`a_new_view_is_created_before_the_trigger_that_names_it`, both failing before the ordinal went
in; `a_dropped_enum_goes_after_the_domain_that_names_it` guards the negation, where alphabetical
happened to coincide with the right answer).
**The order is no longer purely by phase, and the first exception is a renamed table.** A rename is the
single most ordinary difference a schema-compare tool is opened for, and it arrives as a drop
plus a create: rename `orders` to `orders_old` on the left and the comparison yields `OnlyLeft
table:orders_old` beside `OnlyRight table:orders`. Neither `RENAME TABLE` nor
`ALTER TABLE … RENAME TO` rewrites the names *inside* a table, so both sides still carry
`fk_orders_customer` (or `orders_ibfk_1`, or `orders_pkey`) — and phases 2 and 3 put the create
first. MySQL and MariaDB scope a foreign-key constraint name to the **database**, so the create
comes back `ERROR 1826`; PostgreSQL scopes an index name to the namespace, so `orders_pkey`
already exists and takes the whole transaction with it; and neither MySQL nor MariaDB rolls DDL
back, so the user is left half-migrated under a message naming a constraint they never asked
about. `occupied_names(table, dialect, flavour)` is what a table occupies **beyond its own
name**, and it is per engine because the scope is per engine — every line of it measured on the
live tier by creating two tables in one database that share the name. MySQL and MariaDB key a
foreign-key constraint name to the database (ERROR 1826 on MySQL 8.4.11, ERROR 1005 / errno 121
on MariaDB 10.11.14) and an index name to the table. **`CHECK` names split that family**, which
is why this takes a `ServerFlavour` as well as a dialect: MySQL 8.0.16+ scopes them to the
database (*ERROR 3822 Duplicate check constraint name*) and MariaDB accepts the duplicate, so one
arm for both was wrong on one of them whichever way it was written. `ServerFlavour::Unknown` — a
hand-built schema, or a server not yet asked — takes the *narrow* answer, so an uncertain read
raises no warning about a statement that may be fine. **PostgreSQL scopes only what creates a
relation**: `pg_constraint`'s unique index is on `(conrelid, contypid, conname)`, so a constraint
name is per *table*, and it is the index a `UNIQUE`/`PRIMARY KEY`/`EXCLUDE` builds that shares
the namespace with tables (measured: two tables with the same `CHECK` name and two with the same
foreign-key name are both accepted; two with the same `UNIQUE` name give *relation "uq_x" already
exists*). So that arm is `t.indexes` alone — it was firing on plans the server will not refuse,
**and reordering them on that basis** — and the real clash is still caught, the index a unique or
primary-key constraint creates being an `IndexInfo` already. SQLite keys index names to the
database and names no foreign key at all. **SQL Server holds every constraint name against its
whole schema**, since a T-SQL constraint is an object there (`sys.objects`) whichever clause
declared it: a foreign key, every check — a column-level one as much as a table's — and a
primary-key or unique constraint's name (`IndexInfo::constraint`); only an index that is no
constraint is the table's own (`a_sql_server_table_occupies_every_constraint_name_it_declares`).
That arm is read off the catalogue's scoping; no live measurement is recorded for it.
`clashes_between` intersects each `OnlyLeft` table's set with each claiming table's, and the
claim side is `OnlyRight` **or `Differing`**. The second half was missing under a rationale that
does not cover it — *"a `Differing` table keeps its identity, so its constraint names are the
same table's and the `ALTER` path already drops and re-adds each one in the right order"* — true
of a name the table already had and false of one it is **acquiring from somewhere else**:
`ALTER TABLE orders ADD CONSTRAINT fk_cust` while `archive` still holds `fk_cust` is ERROR 1826,
and the `ALTER` sits in the phase that runs before every drop. A `Differing` table therefore
contributes only the names it does not already hold on the left.
**A clash names every shared identifier, not the first.** `NameClash::names` is a `Vec<String>`
where it was one `name`: two tables sharing three constraint names produced one sentence naming
one of them, the user renamed that one and met the next — and which one was named came out of a
`BTreeSet` intersection, i.e. alphabetically, while the server refuses on whichever it meets
first. **And `NameClash::note` takes the dialect**, because its closing clause was the MySQL
sentence on every engine: a PostgreSQL reader was told that "neither MySQL nor MariaDB rolls DDL
back" about a server whose DDL *is* transactional, which is not a detail — there the refused
statement takes the whole migration with it and nothing is half-applied, and that changes what
the reader should do next.
**The resolvable ones are reordered and the rest disclosed**, which is the posture `cycles`
already takes. `nothing_else_references` is the test for resolvable: a drop can be pulled ahead
only when nothing else on the left points at that table, because a referencing table has to be
dropped (or have its foreign key altered away) first and both of those sit at or after the phase
the pull moves this in front of — moving it past them trades one refused statement for another.
A resolvable drop is lifted to the front of the **whole** table phase rather than merely ahead of
the create it collides with, since an earlier create in the same phase may hold the other half of
a two-way rename and a drop with nothing pointing at it is safe anywhere after the dependents
come off. It is a `retain` + `splice` and not a second sort, because the order everything else is
in was just computed and is not up for revision. `SchemaComparison::name_clashes` keeps the whole
list, `resolved` ones included, so a reader can see the plan is deliberately not in phase order;
`plan()` turns the ones this plan cannot settle into `SchemaPlan::clashes`, one `ClashNote`
each (`NameClash::clash_note`: the holder, the names, the claimer, a `ClashWhy` and the
engine's `ClashCost`, with `note()` its English), and `risk_lines()` appends them as
`PlanRisk::Clash` — `destructive()` being that list's English.
**`resolved` is resolved only if the drop is in *this* plan**, and the filter had no way to say
so: the flag is computed once, in `SchemaComparison::of`, from the whole schema — `include` is
not known until `plan()` — so a clash the reordering settled stayed settled however the ticks
fell. Untick the drop, which is an ordinary thing to do ("create the new table, I'll drop the old
one later"), and the refused `CREATE` came back with nothing above Apply, the constraint name
still held by a table this plan no longer touches. The filter asks whether `freed_by` is in the
chosen set before it trusts `resolved`. The mirror correction is that a clash is about the
**create**: the claim side has to be in the plan for the warning to be about a statement that is
in the script, and a comparison-level clash between two objects the user left out is not this
plan's problem.
**The second exception is a name shared across kinds.** A table, a view, a sequence and a
synonym hold one name per schema between them — SQL Server's `sys.objects`, PostgreSQL's
`pg_class` — while the phases put a synonym's or a sequence's create ahead of every table drop,
and a table's create ahead of a synonym's drop: "move a table, leave a compatibility synonym" was
Msg 2714, and the migration rolled back with it (S4.2-L1-02). `free_names_across_kinds` runs
after the table pull: an `OnlyLeft` entry of one of those four kinds that sits *below* an
`OnlyRight` of another kind under the same name — compared as the keys are, folded where the
target's names ignore case — moves to just ahead of that create, not to the front of a phase,
and so never ahead of phase 0, the create being past it already. It moves only when the drop is
free to run that early: a table `nothing_else_references`, a sequence no column default on the
left `mentions` (whole word, case aside), a view or a synonym always. Anything else becomes a
`NameClash` with `resolved: false`, disclosed through the same `note` as a rename's — whose
reason clause, for a sequence, says a column default draws from it. **A drop this pass moves
is recorded too**, as a `resolved` clash like the table pull's, so the filter above discloses it
when the user unticks the drop and the create of the other kind would be refused. Every shape
was measured live on 2022 — table to synonym, synonym to table, sequence to
synonym, and a left synonym to a right view
(`a_name_another_kind_gives_up_is_dropped_before_it_is_taken`; live,
`a_comparison_orders_shared_names_and_folds_case`, which holds all four, a case-only synonym
rename, a case-only synonym target and a replaced alias type a dropped table used in one plan
that applies and a re-compare that finds nothing, watched fail with `compare.rs` stashed).
A cycle is reported rather than resolved — no
creation order satisfies one — and it is **two facts about two schemas, not one**.
`SchemaComparison::cycles_create` is a tangle among the **right** schema's tables and
`cycles_drop` one among the **left**'s; the `cycles()` method is the comparison-level "either",
for a header that is about the two schemas rather than about a plan. `plan` pairs each half with
the kind of statement it can actually break — `(cycles_create && a set creates a table) ||
(cycles_drop && a set drops one)` — a `CREATE` being the statement that carries an inline foreign
key with nothing to point at yet, a `DROP` the one refused while anything still references the
table, and a plan doing neither unaffected however tangled either schema is. The old spelling got
both halves wrong at once: `cycles` was `c1 || c2`, so a tangle among the *left* schema's tables
raised the *create*-order warning over a plan that created one unreferenced table; and the whole
flag was gated on `creates_a_table`, under a comment asserting that a plan of pure alters **or
drops** is unaffected — false of drops, which left `DROP TABLE b; DROP TABLE a;` to be refused at
statement 1 with no warning at all (`a_drop_only_plan_reports_its_cycle`,
`a_left_side_cycle_does_not_warn_a_plan_that_only_creates`). It is
reported through `destructive()`, a warning above Apply, and forces `risk_heading()` to "This
can't be undone" — and deliberately **not** through `unsupported()`: the statements are all
there and one of them will be refused, so withholding a plan the user may still want to copy and
reorder would be the wrong call, which is the call `DumpPlan` makes too. The flag errs toward
warning, because `dump::order_tables` reports *that* there is a cycle and not which edge, so
whether the selected tables are the ones in it cannot be answered from a `ChangeSet`;
over-reporting costs one line in a risk block, and the flag is a warning and never a refusal.
**`SchemaPlan::omissions` is what no plan built from this comparison can carry**, one
`PlanOmission` per object naming it and why, `omitted()` their English lines — and `plan()`
applies that exclusion **itself** rather than trusting
the caller's `include` predicate. On MySQL every routine, trigger and event difference is kept
out of a plan, `information_schema` handing their bodies back with the escapes already resolved,
and past the tree nothing said so: `unsupported()` was empty so the withheld block stayed hidden,
Apply stayed enabled, the preview's subject counted only what *was* included, and success read
"Applied N statements to 1 object" over a three-difference comparison.
`SchemaComparison::needs_source`, written to disclose exactly this, had **no production caller** —
its only three call sites were its own asserts.
**That is one of three reasons, and the second is not about reading.** `plan()` filters
`e.needs_source() || e.unplannable()`, and `CompareEntry::unplannable` is a difference whose
change set `emit()`s nothing — the same sentence one level down: the row was a difference, the
footer counted it, the preview listed one change over an empty SQL box, and success read
"Applied 0 statements to 1 object". It asks what the set *emits* rather than whether the set is
empty, because the input that defeats the narrower question is live — `columns_equal` raises a
change for a PostgreSQL identity kind (`GENERATED ALWAYS` vs `BY DEFAULT`) that
`pg_column_clauses` has no arm for, so the set is non-empty and emits nothing, and
`unsupported()` cannot see it either, `supports_change` being `true` for a PostgreSQL
`AlterColumn` whatever the clause builder can express. So `CompareEntry::omission` has three
arms rather than two (`OmissionWhy::NeedsSource`, `Unread`, `NoStatement`), and the third says
"this comparison has no statement for the difference it found" instead of the read-failure
sentence, which would send the reader to check a privilege that is fine. **The third is a set
that emits and must not run.** `CompareEntry::held_back` is the reason no plan may carry an
entry's changes — a `HeldBack`, one variant per reason (`PartitionOfHeld { parent, sub }`,
`PartitioningDiffers`, `SequenceUnwritten`, `PartitionedUnrestated`, `SequenceRangeLeft { at,
min, max }`), whose `message()` is the sentence — set where the server would refuse the
statements and the only change it would take does damage the comparison may not choose for the
user, or where the difference needs a statement the comparison does not write at all;
`unplannable` is true whenever it is set, and `omission` reads it ahead of its other three
arms, as `OmissionWhy::HeldBack`, so the omitted line is that sentence. Three producers set it — `tsql_entry`, through
`held_back_sequence`, for a SQL Server sequence's range, `table_entry` for a table whose
partitioning differs or a partitioned one no `CREATE TABLE` can restate, and
`hold_partitions_of_held_tables` for a partition of a table so held (all below) — and every other entry is built with `None`. `is_planned` carries the same term, so counting and building cannot answer
differently.
It is a second list rather than more lines in
`unsupported()` because the two ask different things of the reader: `unsupported` means this plan
writes less than its own change list promises, so Apply is refused until the offending tick is
cleared, while this means a difference is not in the plan at all — what is there is complete and
these objects have no tick to clear. `ddl_preview::omitted_block` therefore **discloses and does
not refuse** (`a_mysql_body_is_kept_out_of_the_plan_and_disclosed_by_it`,
`a_blocked_body_cannot_be_planned_however_the_predicate_answers`).
**`CompareEntry::omission_note` is `pub` for `schemaic compare --table`**, which discloses only
the named objects' omissions and so cannot take the plan's whole list: `omissions` is built over
every difference in the comparison, whatever `include` answered (`cli/compare.rs`).
`omission_note` is `omission()`'s English — an `Omission { what, why }`, `what` a `Described {
kind, label }` whose `message()` is *table dbo.orders* — and the value is what the interface
words, `compare_view::omission_text` saying it from the catalogue (under `compare_view.rs`); the
CLI keeps the English, as it keeps `notes()`.
**`SchemaComparison::new_namespaces` is the prerequisite a migration had no way to state.** A
namespace is not an object this comparison pairs — there is nothing in it to diff — so a
right-only PostgreSQL namespace meant `CREATE TABLE reporting.sales`
against a database with no `reporting` in it, which PostgreSQL refuses and with it the transaction
the whole migration runs in, while `unsupported()` was empty and Apply was live. `plan()`
prepends a `CREATE SCHEMA` for each one a set **in that plan** actually names, rather than for
every one the comparison found: the comparison's list is about the two schemas, and a plan over
one ticked table has no business creating a namespace for an object the user left out
(`a_table_in_a_new_namespace_gets_its_create_schema_first`,
`an_unticked_objects_namespace_is_not_created`). The right side's namespaces are read off the
entries' own statuses; the left's are those **and** the target's `DbSchema::namespaces` (under
`schema.rs`), and the list is empty on MySQL and SQLite by construction rather
than by a dialect test — neither has a level between the database and the table, so every object's
namespace there is `None` (`an_engine_with_no_namespaces_never_creates_one`). **Read off the
entries alone, as it was, a target holding no objects read no namespaces at all**, so comparing
into an empty SQL Server database planned "Create schema dbo" — a change SQL Server's plans
then refused, which withheld the whole plan — and into an empty PostgreSQL one `CREATE SCHEMA "public"`, refused as
already there along with the migration's transaction. A namespace every database comes with
(`ddl::namespace_comes_with_every_database`: SQL Server's `dbo`, `guest`, `sys`,
`INFORMATION_SCHEMA` and the nine `db_*` fixed-role schemas; PostgreSQL's `public`, `pg_catalog`,
`information_schema`) is never one of them. PostgreSQL's `public` can be dropped, and into a
database where it has been the plan's
first `CREATE TABLE public.…` is refused and rolled back — the trade for a plan that applies into
every database that still has it (`a_namespace_every_database_comes_with_is_not_planned`; live,
`a_comparison_into_an_empty_database_does_not_create_dbo`). **An *empty* namespace of the
user's own on the target was read as missing too**, and its `CREATE SCHEMA` refused as there
(Msg 2714 on SQL Server) with the whole migration (S2-L1-02); the target's
`DbSchema::namespaces` is what answers that one (`a_namespace_the_target_already_has_is_not_planned`;
live, `an_empty_schema_is_listed_compared_into_and_dropped` and
`pg_catalog::an_empty_schema_is_listed_and_not_created_again`). It does not make the built-in
list redundant: the reader keeps the role schemas *off* `DbSchema::namespaces`, and they joined
the list in the same change, after a table in `db_datareader` compared into another database
planned `CREATE SCHEMA [db_datareader]` and met the same Msg 2714; a hand-built schema lists
nothing at all.
**SQL Server's plans admit `CreateSchema` now** (under `ddl.rs`), so a comparison into a SQL
Server database that lacks a schema other than those is planned like PostgreSQL's — the
`CREATE SCHEMA` ahead of the table in it — where `tsql_supports` refusing the prepended change
used to withhold the whole plan. That makes the filter above load-bearing there: without it, a
role schema would no longer withhold the plan but reach the server as
`CREATE SCHEMA [db_datareader];` — as `dbo` would have, before the reader listed it.
**`is_planned`, `selection_note` and `SchemaPlan::subject` are decisions, and they were in the
view.** `is_planned(entry, selected)` — `selected.contains(&e.key()) && !e.needs_source() &&
!e.unplannable()` — is the
single predicate deciding which objects reach an irreversible `Db::run_ddl`, and it was written
inside a 1,135-line Floem module with no test module at all while `plan` already took it as a
parameter. The footer's count, the button's enabled state and the statements that get built must
ask the *same* question, the two having already disagreed once: a confident "1 object" over a
button that built an empty plan and returned. `selection_note(planned)` is the footer's sentence
and `subject()` the preview's, both "N objects", replacing two hand-rolled plurals a hundred lines
apart beside `preview_title` — which was extracted precisely so a modal's words could be tested
(`a_plans_subject_and_the_footers_count_agree_and_read_as_english`). Both are now the English of
what the window shows, `compare_view`'s `selection_note_text` and `plan_subject_text` wording it.
**Display order is a second order, and it is deliberately not the plan's.** `rows(filter,
expanded)` is what a tree renders: groups in `CompareKind`'s own order, objects **alphabetical by
`label` within a group**, because a tree is read by looking a name up — while the order the
statements must run in stays in `entries` and reaches SQL only through `plan`, and nothing
derives one from the other. A `CompareRow` is either a kind's `Group`, carrying the tally of what
is *visible* beneath it (`CompareCounts::total` is the figure a heading prints), or an `Object`,
present only while its heading is expanded; a group whose entries are all filtered out is omitted
rather than drawn empty, and `default_expanded` seeds the open set with every kind that has a
difference in it, a kind holding nothing but agreement being the answer "nothing to see here".
`RowFilter::query` matches through **`schema::object_name_matches`** — the one search predicate —
over `CompareEntry::label`, so typing a table's name also finds the triggers hanging off it, and
`show_same` is off by default as a reading decision rather than a performance one: two hundred
identical tables put the four that matter below the fold.
**Why there are *no* rows is its own five-armed answer.** `empty_reason(filter)` returns
`EmptyRows` — `NothingToCompare`, `EverythingAgrees`, `AgreesExceptNoted`, `NoMatch`,
`OnlyIdenticalMatched` — each with its own `message()`, and it takes the same `RowFilter` so it
and `rows` cannot disagree about which emptiness this is. "Nothing matched" and "what matched, you
asked not to see" are different answers, and giving the second the first's sentence tells the
user their filter is wrong when it is the toggle beside it. It was three arms inline in a
`dyn_container`'s child closure where nothing could reach them, and one of the three was exactly
that mistake (`a_filter_that_matched_only_agreements_says_so_rather_than_blaming_itself`,
`the_other_three_emptinesses_read_as_themselves`, `each_emptiness_says_something_different`).
And "every object agrees" is not "the schemas match" while a `SchemaComparison::notes` line
stands, so with no filter and a note the answer is `AgreesExceptNoted`, pointing at the notes
the view shows above the tree (CMP-03,
`a_standing_note_keeps_an_all_same_comparison_from_claiming_a_match`).
**`label` carries a routine's signature** — `app.area(integer)` — because the compare tree is the
first surface in this app to list two PostgreSQL overloads side by side, and without it they drew
as two identical rows: the same text, the same heading over the diff pane, and a filter matching
on `label` that could not separate them, so ticking one while reading the other was invisible.
Their keys always differed; only what a reader could see did not. `rows` sorts on `label`, so
this orders overloads as well as distinguishing them.
**The namespace is in it on both arms**, for the same reason and with a sharper consequence. A
trigger's arm read `format!("{table}.{name}")` and dropped the schema its own `key` keeps, so two
triggers named `t` on `city` in two PostgreSQL namespaces drew as two identical rows — and worse
than looking alike, `selectable_keys` filters on this text, so typing `archive` matched neither
and returned an empty set: the one object that differed was invisible *and* unselectable, and
*Select all* was a silent no-op over it (`a_trigger_label_says_which_namespace_its_table_is_in`).
The qualifier goes on the **table**, as in `key` — `app.city.t_ins`, never `city.app.t_ins`.
**`selectable_keys(filter)` is what "Select all" means**, and it is filtered on purpose. It was
`differences().filter(|e| !e.needs_source())` over the whole comparison, never intersected with
the filter — so narrowing four hundred objects to three and pressing the link that sits *in the
same bar as the filter box* took the footer to 312, every one of them a tick nobody had looked at.
The view's **None** link reads the same keys, so the pair is symmetric: it clears only what is
shown, and a filtered-out selection is not silently discarded either. A `needs_source` body is
left out for the reason its row has no tick at all. It is also **what the app's compare landing
seeds the tick set from**, with `RowFilter::default()` because no filter is on screen yet — see
`compare_view.rs` below for the fourth spelling of the plannable predicate that replaced.
**`selectable_keys_of(schema, name)` is its sibling for a comparison raised from one table or
view**: the same two exclusions, then the entries in that namespace that *are* the table or view
of that exact name, or a trigger whose `table` is it. It matches by identity and not through
`RowFilter`, although such a comparison opens with the name in the filter box, because the
filter is a name fragment — `orders` would tick `orders_archive` and its triggers too
(`a_tables_comparison_ticks_only_that_table_and_its_triggers`); and the namespace is compared
because a PostgreSQL `sales.orders` is not `public.orders`
(`a_tables_comparison_keeps_to_its_own_namespace`).
**`left_ddl`/`right_ddl` are the two `CREATE` texts captured at comparison time**, so a view
showing both sides is one `diff::line_diff` over two strings. Handing a renderer the two
`TableInfo`s and letting it ask would put a second opinion about what an object *is* in a view;
this text is only ever read and never emitted — the statements come from `changes`.
**Identity is a key per kind, each as wide as its engine needs.** Tables and views pair on
`(shape, qualified name)` — `t:`, `v:`, `q:` — so a name that is a table here and a view there is
a drop plus a create rather than an unmigratable "differing table"; no `ALTER` turns one into the
other.
**That key asked `is_view`, and a MariaDB sequence is neither.** `CREATE SEQUENCE sq1` is
reported in `information_schema.TABLES` as a base table whose columns are its counter state, so
the sequence keyed `t:`, `table_entry` fell into the table arm, and the plan emitted
`CREATE TABLE sq1 (next_not_cached_value bigint, …)` — an ordinary table on the target, where
`NEXTVAL(sq1)` then fails with *'db.sq1' is not a SEQUENCE* and a later real `CREATE SEQUENCE`
is refused for the name. The reverse direction emitted `DROP TABLE sq1`. Nothing appeared in
`omitted`, while the right-hand DDL pane — built from `TableInfo::create_ddl`, which *does* ask
the shape — said in plain words that the object could not be restated, so the two halves of one
screen disagreed. The key carries the shape now, the entry is a `CompareKind::Sequence` with an
empty change set, and `CompareEntry::unplannable` discloses it through `omission_note` exactly as
an unreadable view is. `clashes_between`'s `table_of` asks the shape too — pulling a sequence
into the clash census would make it a drop/create candidate for a name it has no `CREATE TABLE`
for. **Measured on MariaDB 10.11.14**: a sequence reports `TABLE_TYPE = SEQUENCE` with eight
counter columns, `DROP TABLE sq1` on one *succeeds*, and `CREATE TABLE sq2 (…)` over an existing
sequence is ERROR 1050. Now that `create_ddl` restates a read sequence (under `schema.rs`), the
panes show its real `CREATE SEQUENCE`; the comparison still plans nothing for one, but no longer
hides what it read. Its disclosure would otherwise contradict that pane with *"its definition
could not be read"*: a read sequence carries `held_back` (`HeldBack::SequenceUnwritten`) saying
the comparison writes no statement for one and to copy it from `SHOW CREATE SEQUENCE`
(`a_read_sequence_is_disclosed_as_unplanned_not_unread`). And two sequences pair by their
definitions — `SequenceDef::options`, never `next`, the counter two copies of one sequence need
not share — so a pair whose options differ is `Differing` and lands in `omissions` instead of
reading `Same` off the empty set, while a pair with either side's definition unread is
`uncertain` (`two_sequences_are_compared_by_their_definitions`).
**A PostgreSQL foreign table takes the same road one step removed.** It keys `f:`, so it never
pairs with a same-named table and is never planned as that table's alter; it is filed under
`CompareKind::Table`, which is where the tree shows it; and its change set is empty, so it is
disclosed in `omissions` as a sequence is. The model holds its columns and not its server or
options, so the only statements the table arms could write are a `CREATE TABLE` making an empty
local table where the wrapper's rows were, and an `ALTER` or `DROP TABLE` the server refuses on
one (`a_foreign_table_is_disclosed_not_planned`).
**A table partitioned differently on the two sides is held back, not planned.** No
PostgreSQL `ALTER` changes a table's partition key, nor which table it is a partition of or for
which values, and `ddl::diff` reads neither field. So where partitions are tables
(`partitions_are_tables`) and the key (`TableInfo::partitioning`'s) or the whole `partition_of`
differs — or elsewhere (MySQL, SQL Server) the whole `Partitioning` does, the partitions being the table's
own, so a partition added or a bound moved is the same difference — `table_entry` makes the
entry `Differing` whatever its change set says, and sets `held_back` to
`HeldBack::PartitioningDiffers`, *"its partitioning
differs, and this comparison writes no statement that repartitions a table — recreate it by
hand"* — worded for the differ rather than the server, since MySQL does have `ALTER TABLE …
PARTITION BY` and `ADD`/`REORGANIZE PARTITION` and `ddl::diff` writes none of them, so the
sentence is true on every engine; without it the two would have read as the same table, or
been planned as alters that leave the partitioning as it was
(`a_change_of_partitioning_is_held_back`, over a changed key and a partition moved to
`DEFAULT`). On PostgreSQL the partitions themselves are not compared through the key — each is
a table of its own and pairs as one. A partitioned table the target lacks is created
partitioned, through `ddl::create_table_sql`, with its partitions after it, since `fk_rank`'s
`dump::order_tables` makes a partition wait on its parent
(`a_missing_partitioned_table_is_created_with_its_partitions`); on MySQL in one statement,
`) ENGINE=InnoDB PARTITION BY RANGE (…)` and every partition, and a changed partition list is
held back (`a_mysql_tables_partitions_are_created_and_compared_with_it`); on SQL Server, whose
key names the scheme, a moved boundary or another scheme is that difference between two
existing tables. **A partitioned table the target lacks is held back where no `CREATE TABLE` restates the
partitioning** (`schema::restates_partitioning`, false on SQL Server, whose partitioning is a
partition scheme over a partition function, objects the model does not hold): `table_entry`
sets `held_back` to `HeldBack::PartitionedUnrestated`, *"it is partitioned, and no CREATE TABLE
here restates its partition scheme
— create the partition function and scheme, then the table, by hand"*, because created plain
it would have read as done
(`a_missing_partitioned_sql_server_table_is_held_back_not_created_plain`, which asserts the
plan writes no `CREATE TABLE` and no `PARTITION`).
**A held table holds its partitions back with it.** Each PostgreSQL partition is an entry of its
own, so the parent's hold reached only the parent, and each partition was planned alone: a
source's created `PARTITION OF` a table the target holds plain, which the server refuses, or a
target's dropped — every row of the table the omission note said was left alone.
`hold_partitions_of_held_tables`, run in `SchemaComparison::of` right after the table entries,
sets `held_back` to `HeldBack::PartitionOfHeld`, *"it is a partition of <parent>, which is
held back — recreate them together, by hand"* (*sub-partition*, `sub: true`, where the held
table is further up) on every table entry
that is a difference, is not already held and has, on either side, a held table up its
`partition_of` chain — **whatever the tables between compare as**: the hold used to pass only
through held entries, so a sub-partition under a `Same` `a_2024` of a held `events` was dropped
with its rows (`a_held_back_partitioned_table_holds_back_its_partitions`,
`a_hold_reaches_a_sub_partition_below_a_same_partition`).
**A two-sided table is diffed on what is its own** (`own_parts`, on both sides before
`ddl::diff`): every `inherited` index and foreign key goes, and on a partition
(`partition_of`) every column but those both sides have, each taking everything but its own
default and nullability from the target's — the column set, types, identity and generation
being the parent's, while PostgreSQL lets a partition keep a default and `NOT NULL` of its
own (measured on 16), which clearing its columns outright compared `Same` and never planned
(`a_partitions_own_default_is_compared_and_planned`, which also holds that a parent's retype
is not restated). PostgreSQL clones a parent's
columns, indexes and keys onto each partition, and onto a table referencing a partitioned one a
clone of its key per partition; diffed as the table's own, a column or index added to the
parent was restated on every partition — *cannot add column to a partition*, *relation …
already exists* — and a new partition's key clone was added beside the one the server makes
(*constraint … already exists*), each refusing the whole plan. The parent's change is planned
once, on the parent, and the clones follow it on the server
(`a_column_added_to_a_partitioned_table_is_not_restated_on_its_partitions`,
`an_index_added_to_a_partitioned_table_is_not_restated_on_its_partitions`,
`a_new_partitions_cloned_key_is_not_restated`). It is the comparison's and not `ddl::diff`'s
because the designer shares the differ and never drafts a clone, so has none to strip.
**Extensions are not compared at all**:
installing one is a server-side act — its control files must be on the target's disk — rather
than a definition a plan can restate, and the comparison does not read the two sides' lists.
`a_compare_group_is_named_as_the_tree_names_it` names `ObjectKind::Extension` as the one kind
with no comparison group, so the next kind cannot slip through the same exemption unnoticed.
**An unreadable view is a real producer of that path now, as is a routine**: a SQL Server
*source* view or routine the server shows no text for (`WITH ENCRYPTION`) or whose header the
walk could not hold (`verbatim`) gets the empty set — the same two the editor refuses
(`ddl::view_not_editable_reason`, `RoutineInfo::is_editable` less its C-function case). Drafted
anyway, `ViewDraft::from_table` answers `Some` for every view, so the encrypted one became
`CREATE [OR ALTER] VIEW v AS ;`, the verbatim one nested its stored `CREATE` inside another, and
an encrypted procedure a comment-only plan that applied "successfully" creating nothing. A
two-sided pair of them does not read as `Same` off the empty set: `unread_status` compares the
two sides' texts (two encrypted modules are the same, flagged `uncertain`), and otherwise the
entry is a `Differing` one `unplannable` discloses. The target's being unreadable is not this
case — a readable source replaces it (`a_module_the_source_could_not_read_is_disclosed_not_planned`;
live, `a_comparison_discloses_the_modules_the_source_would_not_show`, 2022 and 2025).
Triggers pair on the qualified table *and* the name, because MySQL scopes a trigger name to the
schema and PostgreSQL to the table and the wider key is right under both; routines on kind,
qualified name and `identity_arguments`, because PostgreSQL overloads on the arguments and a key
without them pairs the wrong two functions in silence. `CompareEntry::key` is the stable string
form a selection set stores, and it reads through `schema::display_name`, which leaves
PostgreSQL's default `public` off — so a key is `enum:mood`, not `enum:public.mood`, and a
caller matching on one won't miss it. A trigger's key qualifies its **table**, the namespace
being the table's: `trigger:app.city.t_ins`, which is the order a caller composing a key by hand
would write, where qualifying the name instead read `trigger:city.app.t_ins`
(`a_namespaced_triggers_key_qualifies_its_table`). **A sequence a column owns is not an object
of its own**: a `serial` or identity column's sequence is created by the column, so comparing it
separately proposes a `CREATE SEQUENCE` the `CREATE TABLE` already makes, and a drop the table's
own drop already performs. An enum's or a domain's dependents come off the **left** schema
(`ddl::type_dependents`) — they are the columns the change has to re-cast, and they live where
the DDL runs; asking the right side would list columns that aren't there.
**Keys fold case where the target's names do.** `pair` lowercases every key, of every kind, when
the **left** schema's `DbSchema::names_ignore_case` is set — the target's collation decides,
that being where the plan runs, and the right side's flag is not read. On a case-insensitive
SQL Server database `dbo.Orders` and `dbo.orders` are one object, and paired as an `OnlyLeft`
beside an `OnlyRight` the create sorted first and was refused for the name the drop below it
still held (Msg 2714, S4.2-L1-02). Folded, they are one `Differing` entry, and the editor's
differ makes the case-only rename a rename — `EXEC sp_rename N'[dbo].[Orders]', N'orders';` for
a synonym (`a_case_only_difference_is_one_object_where_names_ignore_case`). Only SQL Server's
reader ever sets the flag, so every other comparison pairs on exact keys, as all of them did.
**Every name lookup goes through the same fold**, a `Names` value built once from that flag —
`pair`'s keys, the dependency ranks `fk_rank` keys and the sort looks up, `clashes_between`'s
table lookup and the constraint names it intersects, `nothing_else_references`,
`free_names_across_kinds`, the namespace check, `binding_notes` and a synonym's target. They
were a `bool` each site lowercased by, and not every site did: a paired entry is named as the
*target* spells it while the ranks were keyed as the source does, so `dbo.Orders` paired with
the source's `dbo.orders` ranked 0 and ran its new foreign key ahead of `CREATE TABLE
[dbo].[customers]` (a view lost its rank the same way), and the clash census never found the
source's table to ask what names it acquired (CMP-02,
`a_table_paired_across_a_case_rename_keeps_its_place_in_the_plan`,
`a_table_paired_across_a_case_rename_is_in_the_clash_census`); and the namespace check compared
exactly, so a source's `sales` beside the target's `Sales` planned `CREATE SCHEMA` (Msg 2714),
and two source spellings of one new namespace planned it twice (CMP-04,
`a_namespace_the_target_holds_in_another_case_is_not_planned`). `SchemaComparison` keeps the
flag (`names_ignore_case`) so `plan` matches a new namespace to the sets naming it the same way.
**Two objects one side holds that fold to one key are said, not collapsed** (CMP-05): a
case-sensitive source's `dbo.Orders` and `dbo.orders` are two tables a case-insensitive target
cannot hold together, and the second overwrote the first in `pair`'s map, so one was never
planned or named. The one compared is the one the other side spells exactly, or else the first
read, and the other gets a `CompareNote::SetAside { aside, side, kept }` in
`SchemaComparison::remarks` — `pair` names both through the `what: Fn(&T) -> Described` its
caller passes, and `notes()` is the remarks' English
(`two_source_objects_differing_only_in_case_are_named_not_collapsed`). Live, all three from a
`Latin1_General_CS_AS` source into a `CI` target: `a_comparison_matches_every_name_as_the_target_does`.
A built-in namespace is matched the same way: `ddl::namespace_comes_with_every_database` takes the
target's `names_ignore_case`, so a case-sensitive source's `DBO` is `dbo` on a `CI` target and is
not created, and on a case-sensitive one is a namespace of its own and is
(`a_built_in_namespace_in_another_case_is_not_planned_where_case_folds`).
**SQL Server's sequences, alias types, XML schema collections and synonyms are paired out of
`DbSchema::tsql_objects`**, kind by kind — a local `of_kind` keys each by `display_name`, as
every other kind is keyed — and each pair is one `tsql_entry`: `ddl::diff_tsql_object` between
the two readings (the right one through `TsqlObjectDraft::from_info`), `create_tsql_object` for
one only the right holds, `drop_object` for one only the left does. Those are the object
editor's own builders (under `ddl.rs`), so there is no second differ here, and a sequence whose
step differs comes out as the `ALTER SEQUENCE … INCREMENT BY 5` the form would have written,
and one that has to be re-created carries the editor's `tsql_replace_guard`, so a plan
replacing an object with permissions on it is refused at Apply with the reason;
`left_ddl`/`right_ddl` are `TsqlObject::create_sql`, the dump's own statement. A T-SQL sequence
pairs under `CompareKind::Sequence` beside PostgreSQL's — the two never meet, a T-SQL one being
in `tsql_objects` and never in `sequences` — and the other three are kinds of their own,
labelled `XML schema collection`, `alias type` and `synonym`. The label is the key's prefix, so
a key reads `alias type:dbo.code`, space and all, and the tree's headings pluralise it like any
other (`XML schema collections`). **Where they sit in `CompareKind` is their plan order.**
`XmlSchemaCollection` and `AliasType` come between `Domain` and `Sequence` — a collection before
an alias type whose `xml` base may be typed with one, an alias type before a sequence typed
with it — and `is_type` takes both, so they are created in the phase before the tables whose
columns name them and dropped in the phase after those tables are gone; dropped in reverse
ordinal, a sequence goes before an alias type and an alias type before a collection.
**`Synonym` is a type too**, after `Sequence` and before `Table`, as in the dump's types
section. It was written last, after `Event`, on the guess that a synonym needs what it names to
exist first, and that guess was wrong both ways (measured on 2022): `CREATE SYNONYM` takes a
target that does not exist yet, while a view reading through a synonym not yet created is
Msg 208 — so a plan making both a view and the synonym it reads failed at the view. Created with
the types, it precedes every table, view and routine; its drop is refused by nothing, a view
over it failing only when next run, so it goes with the types too, first of them
(`sql_servers_objects_are_planned_around_the_tables_that_use_them`, whose plan holds a view over
the synonym).
**A sequence's position is not compared.** `diff_tsql_object` never reads `last_used`, so two
sequences that differ only in how far their counters have run are `Same`: two databases'
counters differ by use, not by definition. **Nor is its start**, for the same reason one step
removed: `ALTER SEQUENCE … RESTART WITH` rewrites `sys.sequences.start_value` (measured on
2022), so a sequence merely restarted on one side came out Differing, and the default-ticked
plan dropped and re-created it at the source's start, under keys the target's counter had
already handed out. So `tsql_entry` gives the right side the left's `start` before the
editor's differ sees it — the same differ, not a second one — and a sequence re-created for a
real difference, a type, starts where the target's did
(`a_restarted_sequence_is_the_same_sequence`; live,
`a_sequence_restart_is_honoured_and_not_compared`). **Only where the source's range holds
it** (`within`, over the catalogue's text as `i128`): a target at `START 1 MINVALUE 1` gave a
source at `START 1000 MINVALUE 1000` its 1, so a retype created `START WITH 1 … MINVALUE 1000`
(Msg 11703) and an unchanged type altered `MINVALUE 1000` under a counter still at 1
(Msg 11704), and the migration rolled back. A start the source cannot hold is no restart to
honour, so the source's own stands and the sequence is re-created there; a bound that does not
parse answers the same way (`a_target_start_outside_the_sources_range_is_not_kept`; live, the
same test's last half). **The counter is asked once, and only to hold the entry back.** Where
the two starts already agree and the target's counter (`last_used`) has left the source's new
range — run past a lowered `MAXVALUE`, or short of a raised `MINVALUE` — the change is an
`AlterTsqlSequence` with no `restart`, and that bare `ALTER` is refused (Msg 11704, measured on
2022) and the migration rolls back at it, as it always did. The server would take a `RESTART
WITH` a value inside the range, or a re-create, and either hands out again numbers the tables
may already hold — a choice about the user's data the comparison has no business making for
them. So `held_back_sequence` sets `CompareEntry::held_back` (above) to
`HeldBack::SequenceRangeLeft { at, min, max }` and the entry stays out of
every plan, its sentence naming the counter and the range and telling the user to restart the
sequence on the target by hand, at a value no table holds, and compare again. That sentence is
the one instruction for a difference no plan carries, and a comparison whose only difference it
is plans nothing and never enables Preview, so the omitted list alone would never be read:
`SchemaComparison::standing` puts each held-back entry's `PlanOmission::Entry` ahead of the
remarks, each as a `PlanOmission::Note`, and that is what the view's banner reads (CMP-03's
banner, under `compare_view.rs`); `standing_notes` is its English. A re-create — a new
type or start — begins a fresh counter and is not asked about; a counter never used is the
start, which `tsql_entry` already keeps only inside the range. **A target restarted past the
source's range is not this case**: `RESTART WITH` moves `start_value` too, so its start no
longer agrees and the source's stands — a re-create, as above. The live half draws its counter
to 5000 with `sp_sequence_get_range` instead, for exactly that reason, checks the bare `ALTER`
is refused, then that the entry is unplannable and named in `standing_notes`
(`a_range_the_targets_counter_has_left_is_held_back`; live, the end of
`a_sequence_restart_is_honoured_and_not_compared`). **An alias type's bound rule or default is
not compared or carried** — the rule or default is an object of its own, and the comparison
creates none — so it made the type bare and then called the two databases the same, a loss the
dump names in its header (S4.2-L1-07). It is said instead: `SchemaComparison::remarks` holds a
`CompareNote::BindingNotCarried { alias, rule, default }` for each binding the source's
`DbSchema::tsql_type_bindings` has and the target's lacks (`binding_notes`), matched as the
target's names compare; the comparison view shows them above the tree, where they stand after
Apply too, and every plan's `omissions` carries them
(`an_alias_types_binding_the_target_lacks_is_disclosed`; live,
`a_comparison_names_an_alias_types_binding_it_leaves_behind`, before and after Apply). The trade runs the other way too — a sequence the
comparison creates is `create_sql` alone and starts at its own `START WITH`, where the dump moves
a copy's counter on to the source's (`restart_sql`) only when it carries the rows as well
(`sql_servers_standalone_objects_are_compared`, whose counter-only difference is `Same`; live,
`a_comparison_carries_standalone_objects_in_their_order` on 2022 and 2025 — a source with a
collection, an alias type, a sequence at `INCREMENT BY 5`, a table drawing on all three, a
synonym for it and a view reading through the synonym, against a target holding its own sequence at `INCREMENT BY 1` and a stale alias
type: after the sync the sequence was altered in place, the stale type dropped, the synonym
worked, and a second comparison found nothing).
**A synonym's target compares as the target's names do**, by the same flag `pair` folds on: two
synonyms naming `[dbo].[Orders]` and `[dbo].[orders]` on a case-insensitive database are one
synonym, and were replaced for the spelling alone — a replace `tsql_replace_guard` refuses at
Apply where the synonym carries permissions, and performs for nothing where it does not
(S4.2-L1-06). So `tsql_entry` gives the right side the left's target when the two match part
for part case aside (`same_synonym_target`, which also reads an own-database target as its
two-part name), before the differ sees it, as it does a sequence's start; on a
case-sensitive database the two stay `Differing`
(`a_case_only_difference_is_one_object_where_names_ignore_case`).
**Three flags say what a comparison cannot vouch for, and all three are *emitting* limits rather
than comparing ones** — the verdict is right, and it is the generated SQL that suffers.
`CompareEntry::needs_source` is MySQL's: an eager `Db::fetch_schema` reads a trigger's, a
routine's or an event's body out of `information_schema` with the escapes already resolved, so
two mangled bodies still compare equal and the status stands while the emitted `CREATE` is wrong
until the caller has refreshed the body through the lazy `Db::{trigger,routine,event}_source`.
It is false for a drop, which names the object and nothing else, and it asks the capability
`ddl::schema_body_is_emittable` rather than `dialect == SqlDialect::MySql`, so nobody is sent
after a body they have no use for and a fourth engine has to be answered for rather than sorted
silently onto one side. `CompareEntry::uncertain` is a match over
an `IndexInfo::lossy` index — a PostgreSQL index whose expression keys, opclasses, constraint's
`INCLUDE` list, storage parameters or `NULLS NOT DISTINCT` the model never read (see `schemaic-db` for
what widened that list), so two of them compare equal whatever the server holds; the verdict
stands as the
best the model can do, and a tree drawing it like a fully-read match would be overclaiming. It is
**counted as well as drawn**, in `CompareCounts::uncertain`, which overlaps the four statuses
rather than replacing one of them: the flag was a per-row hint only, and an uncertain match is
overwhelmingly an object that comes out `Same` — which the default `show_same: false` hides — so
the one case the flag exists to disclose was the one case nothing disclosed
(`an_uncertain_match_is_counted_even_though_its_row_is_hidden`,
`the_uncertain_tally_does_not_come_out_of_the_status_tallies`).
`cycles` is the third.
