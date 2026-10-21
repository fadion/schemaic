# `ui/table_designer.rs` + `ddl_preview.rs`

`table_designer.rs` + `ddl_preview.rs` — **schema editing**, over `core::ddl`. The
designer is a list-plus-form per section (Table / Columns / Indexes / Foreign keys / Checks) over
one `DdlUi::draft`; the footer's change count *is* `ddl::diff` of that draft, the same
call the preview emits from, so the two can't disagree. The list re-renders on every
draft change but the **form must not** — it seeds local signals from the draft and writes
back through effects, so a draft-keyed form would tear down the field being typed into
(it's keyed on `(tab, selected, rev)`, where `rev` is bumped on structural edits because
removing the selected row leaves `selected` unchanged over a different item).
The footer status is validation first, since it blocks; then the change count; then — only when
nothing fails validation, and **after** the count, never instead of it — the first of
`ddl::index_hints` in `theme::plan_warn()`, an `ellipsized_text` capped at 420px with the whole
sentence on hover. It gives way before the count (`flex_shrink(1000)`). A bare `.tooltip()`
over it once kept its full width, clipped it without its `…`, and squeezed the count out first. The rest ride the list rows' faint detail (`hints_of`, on the designer's own dialect):
an Indexes row reads *redundant* (a unique one still reads *unique*, and is never flagged), a
Foreign keys row *→ t · no index*. None of it touches Preview.
It also owns three things the other editors reuse: `list_pane` — the list-plus-action-bar that
is **one** Tab stop with Up/Down inside it (over `widgets::list_step`) —
`focusable_owned_dropdown`, the picker for a value that isn't `Copy` (with
`focusable_indexed_dropdown`, its sibling that hands back an index, for a list whose row 0 is a
"none" row of its own wording — read by `after_none_row` rather than by matching text), and `suggest_field`, the
**one** builder of a free-text field with a `suggest_chevron` beside it. One options closure
feeds both the menu and the field's inline ghost (`FieldCfg::suggest`, under `lib.rs`), so the
ghost cannot offer what the menu does not hold; the field takes `tabindex` and the chevron
`tabindex + 1`. The pair was hand-assembled in four modules before the ghost came, and a field
built on its own copy would have gained the menu and not the ghost — the column Type and MySQL
Engine (`bound_field_with_menu`), the database editor's `optional_field`, the grant form's
`suggested_field` and the domain's base Type all go through it now, as does the connection
form's Folder. The chevron's *nothing to suggest* row is the caller's: `suggest_field` takes it
as `impl Into<String>` and `suggest_chevron` as a `String`, so a `tr!` note passes as readily as
a literal.
The preview's SQL box is a read-only monospace `edit_field` that **wraps**: it was `no_wrap`,
and an `edit_field` has no wheel to scroll sideways (only the editor pane wires Shift+wheel, see
the Floem gotchas), so the end of a long statement — a selective XML index's paths, a rebuild's
column list — ran past the box's edge and could not be read before Apply. It is syntax-coloured
in the plan's dialect (`FieldCfg::sql`, `p.dialect`) and sits on a field's own surface, as every
SQL field now does — under `lib.rs`.
`preview_proposal` is the AI's way in, beside `preview_change`'s: it seeds from `loaded_table`
like every editor, applies the ops with `propose::apply`, diffs against
`Target::new(dialect, db_flavour(…))` — the flavour matters, or the same change would preview
differently here than in the designer — and opens the same modal. It returns `Err` rather than
raising anything, because the caller is a card in the chat and that is where the user can see
what the model asked for. Going through `loaded_table` is what makes it refuse mid-refresh,
which matters more on this path than any other: the model may have read the table minutes ago,
and a draft seeded from a stale `TableInfo` is how an `ALTER` comes to restate an old column
definition and silently revert a change that already landed.
**The form asks the engine for a capability, not "is this PostgreSQL".** It asked the latter in
three places, which put SQLite on the side that gets a storage engine, a table collation, table
and column comments and `ON UPDATE` — it has none of them (a collation is per column there, and
`ON UPDATE` is MySQL's timestamp attribute), so each was a field whose input the emitter then
dropped on the floor. Column reorder arrows are the one place the old question happens to be the
right one and stay offered on SQLite: MySQL moves a column with `AFTER`, SQLite gets there by
rebuilding — the new table is created in the draft's column order, so a move costs nothing
beyond the rebuild already under way — and PostgreSQL has neither, where an arrow would promise
an edit no statement can carry out. **A fourth question left the crate rather than becoming a
predicate.** The placeholder type a new column starts at was an `== Postgres` in this file's own
`default_type`; it is `ddl::default_new_column_type` now — a value, not a capability — and the
local function is gone, taking one of this file's admitted `engine_comparison_gate` comparisons
(9 → 8) with it. **SQL Server's Create table took it 8 → 5**: both comment fields — table and
column — ask `ddl::supports_comments`, so SQL Server's were hidden rather than typed into and
dropped, until the emitter wrote them and the predicate turned them back on; the identity toggle took `ddl::identity_wording`'s words (drawn UI-side now, by this
file's own `identity_wording` — below), having said *Auto-increment
(AUTO_INCREMENT)* there; the index key hint asks `ddl::supports_index_prefix`, having named
MySQL's `bio(20)` prefix on every engine but PostgreSQL. **The MySQL-only fields took it
4 → 1**: the storage-engine and table-collation row asks `ddl::takes_table_options` — the
emitter's own question, made public for it — a column's *On update* `supports_column_on_update`,
and a check's *Enforced* toggle `supports_unenforced_check(dialect, flavour)`, which is MySQL's
*server's* rather than its dialect's: MariaDB answers `NOT ENFORCED` with `ERROR 1064`
(measured live), and an `Unknown` flavour is withheld it rather than offered a clause the server
may reject (`the_designers_mysql_fields_are_asked_as_capabilities`). **The last comparison took
it 1 → 0**: the index form's PostgreSQL-only *Method* and partial-index predicate block, which
is two fields now — *Method* built where `ddl::supports_index_method` holds (PostgreSQL alone),
*Only rows where* where `ddl::index_takes_filter` answers yes (`supports_partial_index`, the
kind's `IndexStorage::takes_filter`, and neither a constraint index where
`constraint_takes_filter` says no nor a clustered index — under `ddl.rs`). It was kept whole for as long as SQLite's
introspection read no predicate back, since the field would have opened empty over a partial
index that was replayed verbatim; `sqlite::index_predicate_of` reading it is what made the split
honest (under `sqlite.rs`). The index form's *Include* field, after
*Columns*, is built only where `ddl::index_takes_include` answers yes — an engine with `INCLUDE`
(`supports_index_include`, SQL Server and PostgreSQL), a kind that keeps a list, and neither a
SQL Server unique constraint nor a clustered index, on either of which the server refuses one
(under `ddl.rs`) — and parses through `ddl::parse_name_list` into `IndexInfo::include`, so a list is
never typed where no emitter would write it, or where the server would refuse it. Its *Storage* dropdown, after *Include* (tab 27) —
*Rowstore*, *Columnstore*, *Clustered columnstore*, *Primary XML*, *Selective XML*, *Secondary
XML*, *Spatial* —
is built only where `IndexStorage::offered` leaves more than *Rowstore*, and lists only the
kinds it offers — each family asking its own capability, `ddl::supports_columnstore` or
`ddl::supports_xml_and_spatial_indexes`, SQL Server alone for both today — and maps through
`ddl::IndexStorage`, whose `apply` also clears what the chosen kind cannot carry (under
`ddl.rs`). **The form reads the kind as the engine has it** — `IndexStorage::of_in(ix,
dialect)`, a kind the engine does not offer being *Rowstore* — because MySQL's `SPATIAL` index
carries the `method` SQL Server's does, and read through `of` alone it was given T-SQL's *Using*
field (`a_kind_the_engine_lacks_reads_as_rowstore`). **Unique, Columns and Include are built
only for a kind that keeps them** — `takes_unique` and `takes_include` for a rowstore index
alone, `takes_columns` for every kind but the clustered columnstore, which stores every column
— so a columnstore, XML or spatial index no longer offers a toggle and lists its Storage choice
has just emptied. *Only rows where* follows the same rule through `takes_filter`, true for a
rowstore and a nonclustered columnstore index and false for the clustered columnstore, XML and
spatial kinds — exactly where `apply` clears `predicate`. Each is pinned to what `apply` leaves, not listed beside it
(`each_kind_takes_exactly_the_fields_its_apply_keeps`). The *Columns* hint follows the kind
too: the one column an XML or spatial index is over, the columns a nonclustered columnstore
stores. A *Using* field (tab 28) follows the Storage dropdown, built only when the kind has a
`using_example()` — a selective XML, a secondary XML or a spatial index — and typed as SQL
into `IndexInfo::using`, like a filter's predicate, its hint naming each clause shape, a
secondary selective index's `FOR (path)` among them. **The placeholder is that example, so it
follows the kind**: a selective index's field showed a secondary one's `USING XML INDEX … FOR
PATH`, a clause it does not take. An *Order* field (tab 29) follows that, built only
for a columnstore index where `ddl::supports_columnstore_order` answers yes, and parsed through
`ddl::parse_name_list` into `IndexInfo::order` as *Include* is; its hint names the versions
that take one and the nonclustered index's stored-columns limit, which `validate` refuses
before the server does. **They all appear and go with the choice because a Storage change
bumps the form's `rev` and rebuilds it**,
the same rebuild that keeps the fields above from showing what `apply` just cleared. The
foreign-key action dropdown lists
`ddl::fk_actions` besides, which has no `RESTRICT` on SQL Server. The designer opens on an
**existing** SQL Server table too, now that `supports_table_design` answers yes there
(`emit_mssql`'s phases are under `mssql.rs`); the reorder arrows appear there too, and an
identity toggled on an existing column is written, since `supports_column_reorder` says yes now
and both reach the table rebuild (`ddl::tsql_rebuild_sql`, under `ddl.rs`) — until it existed
the arrows were off and the identity toggle was withheld in the preview. The Primary key toggle still leaves Nullable as it was, which
SQL Server would refuse a key over (Msg 8111); `diff` makes the key's columns `NOT NULL` there
instead (`ddl::primary_key_implies_not_null`, under `mssql.rs`). A table created from a database row is
written `[dbo].[t]`: `default_schema` asks `schema::default_namespace` — `public`, `dbo`, or
`None` where there is no namespace level — where it had compared `!= Postgres`, and so sent the
statement out unqualified on SQL Server, landing in the *login's* default schema, which for a
login mapped with `DEFAULT_SCHEMA = sales` is not the `dbo` the row stands for. **The dialect
alone decides it now**, and `default_schema` takes `ConnUi` alone. It used to be refined by the
loaded schema — `None` whenever `DbSchema::schemas()` came back empty, read as "this database
has no namespace level" — but that list was then derived from the objects, so it was empty for
*any* database with none yet, while a SQL Server or PostgreSQL database always has its default
namespace. It carries the reader's own list now (`DbSchema::namespaces`, under `schema.rs`), but
the first-paint partial schema still carries none, so the inference stays wrong. On an empty SQL Server database Create ▸ Table went out unqualified into the login's
default schema all the same (review finding S3.1-L1-03, measured with a user mapped
`DEFAULT_SCHEMA = sales`) — the first table of a new database, the likeliest one to be created
there — and a table comment in the same draft failed the plan instead, the comment naming
`dbo`. No test can reach the branch that went, which lived in the view; the decision is
`default_namespace`'s now, pinned by `default_namespace_names_each_engines_own`. A foreign key
is the same question one level down: the picker lists only the designed table's schema and
writes only `ref_table`, and a bare target on SQL Server bound the login's default schema's
table instead — `sales.orders` → `dbo.customers` (S3.1-L1-02) — so `ddl::fk_clause` names the
table's own schema wherever `bare_reference_is_own_namespace` says a bare name would not.
**Off `whole_ui_gate`'s list, 4 to zero, and the last four were the three opening paths and the
overlay.** Those three write across `ddl`, `schema` and the peer editors, so they name all
three — `open_for_table(ConnUi, SchemaUi, DdlUi, …)`, and the same for `preview_draft_edit` and
`open_for_new` — while `table_designer_overlay` takes `(DdlUi, OverlayUi, SchemaUi)`, there
being no opening path under it; the `SchemaUi` is read only at *Preview SQL*, for the preview's
*Used by* lines (`ddl_preview::with_used_by`, below). **That last step is half of what unblocked `overlays.rs`**, the other
half being `database_editor.rs`: `create_submenu` routes to four editors' doors, so it could not
be narrower than the widest of them, and `open_for_new(&Ui, …)` here was the widest. How this
file got from 29 to 4 is under `lib.rs`'s `whole_ui_gate`, and the steps before that are in
`account_editor.rs`'s entry, the change that started them.
**`ddl_preview.rs` left that list too**, in the tail's one-pass sweep, its last two being
`apply(DdlUi, ConnUi, DdlFn)` and `ddl_preview_overlay(DdlUi, ConnUi, DdlFn, Rc<dyn Fn()>,
open_query)` — `conn` named in both because `plan_read_only` is asked of the connection list
live, at the click and in the footer, rather than read off the stamp the plan was built with.
Narrowing it took the `Ui` import out of the file and broke an intra-doc `[`Ui`]` link that only
the rustdoc `-D warnings` build noticed; it is `[`crate::Ui`]` now.
**Every path ends at `ddl_preview`** — designer, Create table, and the context-menu
shortcuts — so there's one place that shows the SQL, one that names what's destroyed, and
one "Open in editor" escape hatch. Never run generated DDL without it.
`DdlPreview::withheld` (`ChangeSet::withheld_lines()`, worded by `withheld_text` — the values
`unsupported()` is the English of) is the other half of that honesty:
when the dialect can't express part of the plan the modal shows a "This engine can't express
part of this plan" block listing each one, and Apply refuses — **both on the button and inside
`apply()`**, the same rule the write guard follows, since a disabled button is not a guard. It
is a block of its own rather than a line in the risk list because it says the opposite thing —
the risk block warns what will happen, this one what won't — and it is the call
`Change::KeepLossyIndex` already established: the SQL is *less* than the change list above it,
and a preview that didn't say so would be the dishonest half of a destructive operation. Only
SQLite produces one today, and two things reach it. The column row's Drop is the shortcut: it
goes through the draft, so it takes the column's dependents with it, and a foreign key or the
primary key among them is a `Change` that engine can't express — the preview then says so and
refuses, rather than dropping the column out from under them. The other is a designer plan that
rebuilds a table carrying an index `IndexInfo::lossy` marked **and cannot replay it** — it was
edited, it has no `CREATE` text of its own, or the plan moves a column that text may name — so
the block names it and Apply refuses until the user drops the index or undoes the edit. An
untouched one is simply replayed and never reaches this block. A partial index whose condition
names a column the plan drops reaches it the same way without being `lossy`
(`ddl::partial_index_replay`, under `ddl.rs`). Entry
points:
`table_designer::open_for_table`/`open_for_new`/`preview_draft_edit` (a shortcut whose
edit has dependents — dropping a column takes its index and FK with it) and
`ddl_preview::preview_change` (a lone `Change`). `preview_change_warning` is `preview_change`
with the plan's change lines and `destructive` list reworded by the caller, one closure over
both, for facts it holds and the change set does not — the lines as `ChangeLine`s, which
`change_line_text` words only after the closure returns, so a caller rewords a value rather
than an English sentence: the schema tree's Drop passes
`stats::drop_preview` — a table's row figure, a MariaDB sequence's `DROP TABLE` worded as a
sequence's in its line and its warning alike — which is what the confirm it replaced had said
before the preview said everything else again. **The closure takes the risks as `Risk`s and
hands back finished strings**, the one half it words itself, with `risk_text`: the same
closure then appends `used_by_drop_text` over `used_by::drop_listing`, so every line of the
list it returns is worded — it was mixed while that line was `used_by::drop_warning`'s English,
and only the caller knew where the worded part ended. The appended line is
over the object's dependents (`used_by_view::dependents_of`, read when
the entry is clicked), so what the drop breaks — *"Used by 2 views …"* — is named before Apply
rather than by the next error; nothing is appended when the database's schema is not loaded, or
when everything that uses the object goes with it.
**A rename's and a column edit's dependents are `DdlPreview::used_by`, a block of their own** —
not more lines in `destructive`. A rename that breaks a view loses nothing; what it costs is a
fix-up afterwards, and under "This can't be undone" in the error box it would claim a loss the
plan does not make. So `used_by_block`, drawn between the risk block and the withheld block,
heads itself a neutral *"Used by"* beside an accent-coloured triangle, has no error box, hides
when empty, and **Apply stays live**. A drop's dependents stay where they were, in
`destructive`, since `used_by::touched` leaves whole-object drops out. It is filled by
`ddl_preview::with_used_by(preview, SchemaUi, &cs)` over `used_by::change_notes`, each worded by
`used_by_note_text` (under *The string catalogue*), reading the
schema through `table_designer::loaded_schema` and leaving the list empty while that is not
loaded — the preview opens either way. `preview_of` cannot fill it, holding no `SchemaUi`, so
every editor that previews a rename or a column edit calls `with_used_by` itself: the table
designer's *Preview SQL*, `preview_draft_edit` (the tree's column Drop), the view editor's and
the routine editor's *Preview SQL*. A new path that previews such a plan has to call it too,
and nothing checks that it does — skipping it just loses the block, silently.
**`DdlPreview::then` is what runs once *this* plan has committed**, and only then: `apply`
clones it when Apply is pressed and calls it in the `Applied` arm, after `close_editors` — not
on `Failed`, not on `Declined`, and not when the generation check above it finds the preview
reopened on another plan meanwhile, which returns before any arm. *Copy to* is its one user —
its `CREATE TABLE` is the plan, and the rows follow it, or with *Copy data* off `then` only
closes the copy modal (`copy_table_view.rs`) — and `preview_of` and `preview_of_plan` both
build `None`. It is not `after_commit`, which is SQL the app runs after the plan and the box
shows; `then` is UI work, and the box shows nothing for it. So **`changes` is not always one
line per `Change`**: what `then` will write has to be said by its caller in the change list —
*Copy to* pushes `copy_table::rows_change` there, worded (`copy_table_view::rows_line_text`) —
or the preview promises less than Apply does.
**`ddl_preview::close_peers` is the one editor-target list**, called by every editor's `open`
before it sets its own and by `close_editors` after an Apply. Each of those `open`s used to
keep its own copy and they had **drifted**: the table designer cleared the view editor and
nothing else, the view editor the designer and nothing else, while the object, routine and
trigger editors cleared four apiece — every one of them carrying the same comment about two
panels being painted at once, which is precisely what a partial list does. Six flags maintained
by hand in five places is where one list becomes the only version that stays true. The single
exception is the caller's, not the list's: `keep_trigger` is what leaves a half-filled trigger
form standing under the routine editor it opened. **The drafts go with the targets**:
`account_draft` and `grant_draft` are app-lifetime signals, so clearing `ddl.account` alone left
the plaintext password sitting in a signal for the rest of the process — after Cancel and after
Apply alike, reachable by anything holding the bundle. Both forms re-seed from their target on
open, so there is nothing to keep. `account_confirm`, the form's second copy of the password, is
cleared on the same line for the same reason; the account form's `reset_then_seed` reaches it
through this list on every open, and its own close clears it beside the draft. `close_preview` is the same one-door idea for the preview
itself, because `ddl.sql` is app-lifetime too and held the last plan's script for the life of
the process; there were two `set(None)` sites and a third would have had to remember. **That
clear is load-bearing rather than defence in depth**, and it changed sides without the helper
changing: it was the weaker thing while the box showed `ChangeSet::export_script`'s *redacted*
copy, and it now holds `ChangeSet::emit`'s statements, which for an account plan carry the real
password — or, on PostgreSQL, a SCRAM verifier of it, which is still enough to brute-force the
password offline — see `open_preview` for why the box shows those. So it is the same rule as
`account_draft` and `grant_draft` above rather than a tidy-up beside them.
**`has_editor_behind` is the other reader of that same list, and the word on the preview's exit
button is what depends on it.** The label was a hand-spelled two-name test —
`designer.is_some() || view.is_some()` — while the canonical list of what stacks under the
preview sat in `close_peers` just above it, nine entries long. So seven of the nine editors got
**Cancel** on a button that does not cancel: `exit` resolves to `close_preview`, which writes
only `preview` and `sql`, so whichever editor signal is still `Some` re-renders with the draft
intact — and both `object_editor` and `database_editor` carry the comment "Cancel there returns
here with the draft intact", the code asserting the button is a **Back** while the button said
Cancel. It is one predicate rather than a third spelling, for the reason this file already gives
once about the five hand-written copies that had drifted.
**What holds the lists together is a test, not the compiler**, and knowing which is the
difference between trusting them and checking them: there are *three* nine-entry lists — this
one, `close_editors` and `modals::ddl_editors_up` — each hand-written, and
`every_editor_reaches_the_three_lists_that_must_know_about_it` raises each of the nine targets
**alone** and asserts all three see it. So they agree because something checks, which is the most
that is available while the nine are nine signals rather than one enumerable thing; nothing
structural stops a tenth editor being added to two of the three.
**`DdlPreview::qualified` is a *different* question to `scope`, and both are read off the change
set.** `scope` says which runner a plan takes; `qualified` says whether the plan is *in* a
database and so whether the title bar writes `db.subject`. An account change is the case where
they differ: it deliberately takes the ordinary in-database runner (`ddl::is_account_change`),
and it inherited the qualifier as a side effect, so `CREATE USER 'app'@'%'` — server-wide on
both engines — was titled `shop.app@%`, which reads as scoped and is not. `preview_title` is a
free function so that sentence has a test; it was a `match` inside a `dyn_container` child.
`risk_heading` rides on the preview beside it, a `String` worded by `risk_heading_text` from
`ChangeSet::risk_reversible` (`SchemaPlan::risk_reversible` in `preview_of_plan`) — a heading
that contradicts the sentence beneath it costs the sentence its weight. A multi-object plan is
one of `qualified`'s two exceptions to being derived at all — the entry below says why. The
other is the trigger editor's `preview_from`, whose subject is a phrase (`triggers on
shop.orders`, the table qualified inside it): derived, the title read `shop.triggers on orders`,
as if `shop` held an object called that
(`the_preview_title_qualifies_the_table_inside_the_phrase`).
**`preview_of_plan` is `preview_of`'s multi-object counterpart**, for a schema comparison's
`compare::SchemaPlan`, and `DdlPreview` needed **nothing added** to carry one: every field it
holds was already flat text, and a plan answers each of them — the statements as its sets'
statements in order, the prose lists with the object named per line. So a twelve-object migration
reaches the same modal, the same Apply and the same `Db::run_ddl` as one table's designer edit —
a second apply path being precisely what the DDL invariant forbids.
`scope` is read off **every** set's changes rather than the first set's or the caller's, for the
reason `preview_of` gives; a comparison produces no server-level change today, and asking the
plan rather than assuming that is what keeps the answer right if it ever grows one.
**`qualified` is `false` unconditionally**, and it is the one field this counterpart does not
derive. Deriving it the way `preview_of` does answered `true`, and `preview_title` then wrote
`Apply changes to My MariaDB · shop.12 objects` — every input to it true and the result nonsense,
because a plan's subject is a **count**, not an object that lives in a database. Nothing is lost
by the title declining to qualify it: each statement is still qualified and each change line
names its object — `SchemaPlan::lines()` pairs each `ChangeLine` with its subject, which
`plan_line_text` words, and `summaries()` is that list's English. `preview_of_plan` has four unit tests of its own, two of which fail against
the old derivation — `a_plan_is_never_qualified_by_the_database_it_runs_in`, and
`an_empty_plan_previews_as_empty_and_unqualified`, where `all()` over no changes was vacuously
true and flipped the answer for a reason that had nothing to do with accounts. It had none while
`preview_of` had them, which is how the nonsense title shipped.
`withheld` is `SchemaPlan::withheld_lines()` worded by `plan_withheld_text` — the values
`unsupported()` is the English of — so the "not applied in part" refusal fires on the
button *and* inside `apply()` on this path too. **`omitted` is `SchemaPlan::omissions`, each
worded by `compare_view::omission_text`, and is a second block beside it, drawn by `omitted_block`, which discloses and does not refuse** — the
difference between the two is what makes them two blocks. Withheld means half an edit, and half
an edit is not a smaller edit, so Apply waits for the tick to be cleared; omitted means whole
objects sat out, what is below is complete, and the user has no tick to clear because those rows
have none. Refusing on it would make a comparison unusable on MySQL the moment one routine
differs, so it is stated in the muted colour above the statements and Apply stays live.
`preview_of` fills it with an empty list: a designer edit is about one object, which is either in
the plan or has no plan. `script` is
`SchemaPlan::export_script()` — the same field `preview_of` fills that way, and for the same
reason: Copy and Open in editor both put the text somewhere durable. It happens to equal
`editor_script()` byte for byte for a comparison, which produces no account change, but the rule
is enforced by *calling* the scrubbing function rather than by a comment observing there is
nothing to scrub today. `subject` names the plan and the database it lands in
(`SchemaPlan::subject_in` — "12 objects in shop" — worded by `compare_view::plan_subject_text`
for the title and the success line alike) rather than an object, there being no single
object to name, and the post-apply `refresh_db` already re-introspects a
whole database, which is the right refresh for a plan that touched many of its objects.
**`preview_account` takes a `PlanTarget` captured where the plan was raised**, not the
live `edit_ctx`. `conn_id`, `dialect` and `read_only` come off the `AccountTarget`/`GrantTarget`
the form was opened with — which is what those fields are *for*, and they were carried and never
read while this re-derived all three from whichever connection the switcher pointed at now: a
form opened on MySQL and previewed after a switch to PostgreSQL was emitted at the wrong
dialect, and the wrong connection's read-only flag decided whether Apply was offered. It is a
struct rather than three more parameters because all three come from one place and have to stay
together — taking them individually is how one call site comes to pass the live connection's
`read_only` beside the target's `conn_id`. **It takes a `Vec<Change>`, built into one set by
`ddl::accounts`**, because SQL Server's New account form can raise two: a login and the user it
brings, which it sends as one plan under one preview and one Apply. The account form passes
`account_editor::preview_changes`; the grant form and the browser's Drop pass one change each.
**The preview's words are the catalogue's** (`preview-*`) — the title through
`preview_title`, one whole message `preview-title` (`$connection`, `$subject`) whose subject is
`database.subject` when `qualified` and the bare subject otherwise, so its tests pass unchanged;
the change-list heading `preview-changes`, a plural on `$count` that replaced `text::plural`;
the risk heading, `preview-risk-reversible` or `preview-risk-irreversible` through
`risk_heading_text`, which `english_heads_the_risk_block_as_core_does` holds equal to
`ddl::risk_heading_for`; the Used by, withheld and omitted headings and the latter two's closing lines; Copy, Open in
editor, Back (`preview-back`) and Apply/Applying…; and the applied view's `preview-applied`
(plural, `$count`, `$subject`) and *The schema has been refreshed.* Stop, Cancel and Close are
the shared `dialog-stop`, `dialog-cancel` and `dialog-close`, and the note beside the disabled
Apply is the shared `dialog-read-only` — the import modal's message, so the sentence the two
footers share is now one message rather than two literals that modal's source gate held equal.
`the_counted_lines_read_as_they_did_in_english` pins both plurals. English stays: the SQL box
(`sql_box_text`, its `-- After the plan commits…` comment included, being generated SQL) and the
*SQL* heading, and what the server says, and nothing else — the change lines, the
risk lines, the risk heading, the *Used by* lines, the after-commit refresh note, the omitted
lines and, last, the withheld lines have left that list, worded from `ChangeLine`,
`Risk`/`PlanRisk`, `used_by::Listing`/`UsedByNote`, `ddl::ElsewhereNote`,
`compare::PlanOmission` and `ddl::Withheld` (`risk_text`, `plan_risk_text`, `used_by_drop_text`,
`used_by_note_text`, `elsewhere_note_text`, `withheld_text`, `plan_withheld_text`, all under
*The string catalogue*, and `omission_text`, under `compare_view.rs`) and from the bool
above, and so has every `Err` `preview_proposal` hands the card — its own two refusals
(`propose-not-loaded`, `propose-nothing-to-change`) and each `ProposeError` that
`resolve_target` or `apply` returns, through `propose_error_text`.
**`open_for_table` takes a `DesignerFocus`** — `Table`, `Column(&str)` or
`Key { index, foreign_key }` — which is the row the designer lands on once it opens, so the
tree's column and key right-clicks (`Edit column`, and `Edit index` / `Edit foreign key` /
`Edit primary key`) put you on the row you clicked instead of on the table summary with the row
still to find. It is **named, not positional**: the designer's sequence is the draft's, and the
draft doesn't exist until the modal opens. That is also why the two cases resolve on opposite
sides of the open — a `Column` against the introspected `TableInfo` before it moves into the
`DesignerTarget`, a `Key` against `ui.ddl.draft` *after* `open_designer` returns, through
`ddl::TableDraft::find_key`, since the seeded draft is the sequence the Indexes and Foreign keys
lists actually render (and a `PrimaryKeyColumn` answer lands on the Columns tab, the key being a
tick there rather than a row). Landing itself is the three signal writes any selection change is:
`ddl.tab`, `ddl.selected`, and the `ddl.rev` bump the form is keyed on. A focus that resolves to
`None` — a name the draft doesn't hold — is dropped **silently** and the designer opens on the
`Table` tab, which is what it did before it was asked at all: the request that failed is the
landing, not the edit.
**Its words are the catalogue's** (`designer-*`) — the tab strip, sections,
fields, toggles and their hints, the Indexes rows' *unique*/*redundant* details and the Foreign
keys row's *→ {table} · no index* (`designer-fk-detail-unindexed`) — with Cancel, Preview SQL
and the change count the shared `dialog-cancel`, `editor-preview-sql` and
`editor-change-count`. The titles are whole messages, `designer-title-edit` with `$table` and
`designer-title-create` with `$location`. `DesignerTab::label` (in `lib.rs`) returns a `String`
from `designer-tab-*`, and this strip is its only reader. `list_actions`' tooltips are
`designer-list-add` for `+` and the shared `editor-row-remove` and
`editor-row-move-up`/`-down` for the bin and the arrows — so the trigger editor's list bar,
which calls it, is translated with this file — and its `btn` closure takes the tip as a
`String`, as `bound_toggle`'s title and hint, `hint` and `field_with_hint` do. **A placeholder
is a sample or a phrase** (`pub(crate) enum Placeholder`): `bound_field`, `sql_field` and
`field_view` take `impl Into<Placeholder>`, and a `&'static str` converts into
`Placeholder::Sample`, so the SQL and identifier samples — `table_name`, `varchar(255)`,
`InnoDB`, `utf8mb4_general_ci`, `btree`, `id, name`, `qty > 0`, `CURRENT_TIMESTAMP` — pass as
they always did and stay English, while the table comment's *What this table is for* is the one
`Placeholder::Phrase`, reaching the field as `FieldCfg::placeholder_of`. **Two words core gives
are drawn here** (the core-words rule under *UI conventions*): `identity_wording(dialect)` in
place of `ddl::identity_wording`, the keyword in its hint staying SQL as a `$keyword`
placeable, and `storage_label` in place of `IndexStorage::label` in the Storage dropdown.
**The pick is matched back through `storage_label` too**, because the picker hands back the
text it shows: no English label decides which kind was chosen, and two kinds sharing a word
would leave one unpickable, which `english_names_every_index_kind_as_core_does_and_each_apart`
checks beside holding the English equal to core's.
`english_words_the_identity_toggle_as_core_does_on_every_engine` does the same for the toggle on
every dialect, and `the_tabs_and_the_composed_lines_read_as_they_did_in_english` pins the tabs,
the foreign-key detail and both titles. What stays English: the referential actions in the On
delete/On update dropdowns (`NO ACTION`, `CASCADE`, …), which are SQL keywords; the samples
above; and a new item's name (`column`, `{table}_idx`, `fk_{table}`, `{table}_chk`). An index
hint is worded here, `index_hint_text` over the `IndexHint` and its `subject`
(`designer-hint-*`, `english_says_every_index_hint_as_core_does`), and so are the draft's
refusals: the footer and Preview's readiness read `TableDraft::problems`, never `validate`, and
`table_problem_text` words each `TableProblem` from `table-problem-*` (under `i18n.rs`) — and,
being `pub(crate)`, the AI proposal card's `ProposeError::Invalid` too.
