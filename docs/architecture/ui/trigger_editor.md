# `ui/trigger_editor.rs`

`trigger_editor.rs` — the **trigger** modal, over `core::ddl`'s
`TriggerSetDraft`. Reached from the schema context menu's per-table
**Triggers…** entry — and from a **view's**, on every engine but MySQL, since `INSTEAD OF`
lives on PostgreSQL, on SQLite and on SQL Server, and on the last two it is the only way a view
is written to at all (`overlays::object_entries` asks `ddl::supports_view_triggers`, as
`blank_trigger` does for the `INSTEAD OF` a new view trigger opens on, where the two used to
spell `!= MySql` each their own way; it still excludes a materialized view:
PostgreSQL refuses one outright); same chrome, same seed-local-signals-then-write-back rule and same
`ddl_preview` ending as `view_editor`. The trigger modal is the **designer's list-plus-form
shape** — the table's triggers on the left, the selected one's form on the right, `+`/`−`
under the list (no ↑/↓: list position is display order, while firing order is MySQL's
`FOLLOWS`, PostgreSQL's alphabetical and SQL Server's `sp_settriggerorder` rank) — so one plan can drop one trigger, edit another and
add a third. It shares the designer's `selected`/`rev` signals, since only one of the two is
ever open, and splits list-vs-form re-rendering the same way for the same reason. The Body field
is SQL-coloured in the target's dialect (`FieldCfg::sql`, under `lib.rs`).
**It is deliberately not a designer tab**: what belongs there is what can be a *clause* of
`ALTER TABLE`, which is why checks are and triggers aren't — a trigger needs its own
statement, so folding it in would turn MySQL's one coalesced `ALTER TABLE` into an `ALTER`
plus N statements that commit one at a time, and `DdlError::applied` would stop meaning much.
It reaches `routine_editor.rs` and used to *contain* it: a PG trigger has no body, only a
**function** to call, so the trigger form would be a dead end without a way to write one. The
function modal lived here for that reason and moved out when routines became browsable in
their own right; what didn't change is the handoff below. Three rules are written down because
each was a bug waiting: **the form is
per-engine because the objects are** (MySQL owns a body and one event; PG calls a function,
takes several events, a `WHEN` and `UPDATE OF` columns; SQLite owns a body, one event, a `WHEN`
and `UPDATE OF` columns, and offers a view only `INSTEAD OF`; SQL Server owns a
body and several events, fires once per statement, takes `AFTER` or `INSTEAD OF` on a table and
only `INSTEAD OF` on a view, and has a **NOT FOR REPLICATION** toggle no other engine is built),
so it *hides* what an engine can't express rather than offering it and failing at apply. The
lists come from `core::ddl` rather than a `(dialect, is_view)` match here — `trigger_timings`,
`trigger_events`, and `trigger_fires_on_several_events` choosing between the event toggles and
the one-event dropdown — and are held there to exactly what `TriggerDraft::validate` accepts.
So does every other control: the function picker, the level, `WHEN` and the "Of columns" field
ask `trigger_runs_function`, `supports_trigger_level_choice`, `supports_trigger_when` and
`supports_trigger_update_of`, and the *Condition* header shows when either of the last two is
offered. **PostgreSQL's "Of columns" came with that change** — the field was SQLite's alone
while `TriggerInfo::update_columns`, PostgreSQL's introspection and `create_sql` all carried the
clause, so a PostgreSQL trigger's `UPDATE OF` list was restated on apply but could be neither
seen nor changed in the form.
The event toggles write through `TriggerInfo::set_event` rather than editing `events` here, so
the order the emitter round-trips against and the rule that an event switched off drops its rank
live in core. **Beside each event toggle on SQL Server sits that event's rank** — `rank_choice`,
*Any order* / *Fires first* / *Fires last*, gated on `supports_trigger_firing_rank` — and it is
**read from the draft on every frame, not seeded once** as `bound_choice` is: switching the event
off clears the rank from under it, and a dropdown seeded at build would go on showing a rank
the draft no longer holds. Its select writes only a real change, since `update` notifies whatever
it writes. The picker hands back the text it shows, so **one `label` closure builds both the
entries and the match a pick is read back through** — it replaced `ANY`/`FIRST`/`LAST` consts,
and translating the entries alone would have left the comparison against an English const
silently false (*Nothing decides behaviour by an English label*). The closure captures nothing,
which keeps it `Copy`, as the picker's `current` closure needs. `INSTEAD OF` with a rank is left to the validator to name, as `UPDATE OF` is. An
**Execute as** field (`supports_trigger_execute_as`) follows NOT FOR REPLICATION, the same one
field as the routine form's over `ExecuteAs::parse_field`. The Tab order interleaves them: event
toggle `30 + 2n`, its rank `31 + 2n`, NOT FOR REPLICATION 45, Execute as 47.
`blank_trigger` opens a SQL Server trigger on `AFTER` (a view's on `INSTEAD OF`), statement
level and `NEW_BODY_MSSQL`, a `BEGIN SET NOCOUNT ON; END` rather than MySQL's empty block, which T-SQL refuses as SQLite does
(`a_new_sql_server_trigger_opens_valid`). That per-engine shape is also why
`blank_trigger`/`trigger_list` take a `SqlDialect` rather than a `pg: bool`, and why the
MySQL-only `fetch_sources` (`SHOW CREATE TRIGGER`) is gated on `ddl::catalogue_body_is_unescaped`
— whose reply is also why
**this overlay's `dyn_container` key is deliberately not the memo the routine and view editors
took** (*Floem 0.2 gotchas*): `form` is built once per `(selected, rev)` and its Body seeds at
build, so the rebuild that key causes is the only delivery of the escape-corrected body.
**That fetch is a correction, not an enhancement, and the skip that protected the user's typing
was preserving the corruption instead.** The editor opens on
`information_schema.ACTION_STATEMENT`, whose escapes MySQL 8 has already resolved — a trigger
written `'it''s'` reads back as `'it's'` — so both sides of the diff are patched, the rule
`view_editor::fetch_algorithm` established. A draft row the user had already edited was left
alone on the reasonable ground that a round trip landing in milliseconds is no reason to
overwrite somebody's keystrokes; but `current` was patched unconditionally, so a user typing one
character inside the fetch window kept the mangled body on the side Apply emits, and Apply is
`DROP TRIGGER` followed by a `CREATE` carrying `'it's'` — 1064 on both engines, after the drop
has committed, with no transaction around it. `schema::TriggerSource::may_overwrite_edit(opened,
draft)` is the rule now, asked of the *opened* body rather than of the edit: an untouched row
always takes the correction, and a touched one takes it only where the fetch disagrees with what
the editor opened. The common case — a body with no escapes to resolve — leaves the keystrokes
alone, because there is nothing to rescue them from. The bodies the editor opened with are read
out of `current` **before** `current` is corrected, since they are what says whether the row was
touched at all;
**the function list is re-fetched on its own** (`Db::trigger_functions`
via `TriggerFnFn`) and arrives a round trip late, so the
picker keeps whatever the draft already names instead of selecting the first entry and silently
re-pointing the trigger — the schema fetch carries the same list, so this is what puts a
function *just created* in the dropdown before the next reload; and **the trigger target is
never cleared while the routine modal is
up** — its overlay just renders nothing — so closing that one reveals the half-filled trigger
form intact, with no "return to trigger" flag to be a second source of truth. `is_editable_trigger`
is the entry point's gate, and a door onto `TriggerInfo::is_editable` rather than a rule of its
own: a constraint trigger's deferral settings aren't modelled, and a SQL Server trigger whose
text is hidden or whose header could not be read has nothing to be rebuilt from, so each is
listed and droppable but not editable, the call a materialized view gets — the form shows the
name and a sentence saying which of the three it is (`an_unreadable_sql_server_trigger_shows_read_only`).
**Which function a trigger names is asked in one place — `matching` — and the picker and the
Edit button both ask it there.** `TriggerAction::Function::name` is emittable SQL on both
producers, so *showing* it means mapping back through the fetched list: `fn_names` drops the
quotes and compares against `fn_display`, which is always qualified. That exact comparison is
only half the answer, because `tgfoid::regproc::text` omits the schema whenever `search_path`
already resolves the name, which on a stock server is every function in `public` (`fn_sql`'s
`qualified_ident` used to as well, and names `public` now — a function picked here is stored
`"public"."audit_fn"`). So the stored SQL for the commonest trigger
there is the bare `audit_fn`, it matched nothing, and the picker listed that function **twice**
(once as the "whatever the draft names stays selectable" fallback row, once as the real fetched
row) with **Edit permanently disabled**, putting "edit the function this trigger calls" out of
reach for it. `matching` tries `fn_names` first and then, only for a name carrying no `.`, a
**unique** match on the bare name: two schemas holding `audit_fn` is exactly when a guess would
bind the display to the wrong function, so that case stays unresolved and shows the stored SQL
verbatim — the honest degradation the module already chose.
**The reason it could hide for so long is that there were two spellings of "which function is
this".** `display_of` asked `fn_names`; the Edit button independently compared `fn_display`
against the already-displayed string, and those two agree only while `display_of` has
*succeeded* — the moment it falls back to showing the stored SQL verbatim, Edit's comparison is
silently `None`, which is why Edit stayed grey on a stock `public` function long after the
qualified case was fixed. Both call sites go through `matching` now.
`a_stored_name_matches_its_function_in_either_quoting` pins
the qualified half and `a_bare_stored_name_resolves_when_only_one_function_can_be_meant` the
bare one, including the ambiguous pair it must refuse — but both drive `matching` in isolation,
and `matching` was never the broken half, which is why that seam is held by
`one_answer_gate::both_doors_ask_which_function_through_the_one_helper` rather than by
construction. It is a source gate because the decision is which helper a `dyn_container` builder
calls and there is no value to assert on. Two halves: `fn_names` must appear exactly twice in
this file's production code — its own definition and the one call inside `matching` — so a
caller reaching past `matching` to it loses the bare-name arm and answers `None` for the
commonest trigger on a stock server; and the Edit button's own `let` region inside `pg_action`
must name `matching(` and must not name `fn_display(`. The second is scoped to that binding
rather than counted over the file because counting cannot separate the Edit button's question
from the picker's *inverse* mapping at `on_select`, where `fn_display(f) == v` is right by
construction because `v` came out of the options list. Resolving a display never fabricates an
edit: `display_of` feeds the `sel` display signal and never the draft.
**`TriggerTarget::sibling_triggers` is read at the door, because the modal cannot see far
enough.** On MySQL, MariaDB, SQLite and SQL Server a trigger name is unique across the whole *schema*
(`ddl::trigger_names_are_schema_scoped`), so the second table in a database to get a trigger
through the `+` button was offered `new_trigger` again — accepted by the form, accepted by
`TriggerSetDraft::validate`, which holds one table's set and legitimately checks only the narrow
half, and refused by the server at apply with `ERROR 1359`, *after* any `DROP` in the same plan
had already committed, naming a table the modal never mentioned. Widening the *scope* is what was
wrong; the suffix walk had already been fixed by sharing `unique_name`. So
`sibling_trigger_names` gathers every trigger name belonging to some other table in the database
off the schema the tree is showing — the way `object_editor` reads a type's dependents — and it is
**empty on PostgreSQL**, where the name is scoped to the table and avoiding a sibling's would
propose `new_trigger_2` for no reason the user can see; empty too until the database's schema has
loaded. Only `blank_trigger` reads it, and only to pick a name.
**Its words are the catalogue's** (`trigger-editor-*`) — the title, the
preview's subject, the empty-selection hint, the three read-only explanations, sections,
fields, the TRUNCATE and NOT FOR REPLICATION hints, the function picker's *Edit* / *New
function*, *Arguments* and *Add argument* — with Cancel, Preview SQL and the change count the
shared `dialog-cancel`, `editor-preview-sql` and `editor-change-count`, *Execute as* and its
placeholder the routine editor's `routine-editor-execute-as{,-placeholder}`, and the value
rows' bin the new shared `editor-row-remove` (so `widgets::row_button`'s tooltip, like
`table_designer::empty_hint`'s message, takes `impl Into<String>`). The NOT FOR REPLICATION
toggle's own label is the SQL clause and stays, as do the SQL samples and keyword choices
(`INSERT`/`UPDATE`/`DELETE`, `FOR EACH ROW`, `BEGIN … END`). The list's `+`/`−` are
`table_designer::list_actions`' and were converted with that file. The footer's refusal and
Preview's readiness read `TriggerSetDraft::problems`, not its English wrapper `validate` (the
editors' refusals, under `i18n.rs`): `trigger_problem_text` words each problem from
`trigger-problem-*`, `$engine` a product name and `$event` an event's SQL. A firing rank's word
is a message of its own — `-first-without-event`/`-last-without-event`, `-first-clash`/
`-last-clash` — rather than `FiringRank::label`'s English *first*/*last* going in as a
variable; the clash's *both*/*all* is a selector on `$n` (`[2]` exactly), and its names are
joined with ", " in code, as core joins them. Held by
`english_says_every_trigger_problem_as_core_does`.
