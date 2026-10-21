# `ui/object_editor.rs`

`object_editor.rs` — the **enum / domain / sequence** modal, over `core::ddl`'s
`ObjectDraft`, and SQL Server's **sequence / synonym / alias type / XML schema collection**
in the same modal. Reached from a tree object's **Edit**, from a database or schema node's
**Create ▸ Type / Domain / Sequence** on PostgreSQL and **Create ▸ Sequence / Synonym / Alias
type / XML schema collection** on SQL Server (each entry asks `ddl::supports_object_creation`,
so on MySQL none of them exists, the same "hide what an engine can't express" call
`trigger_editor`'s form makes; `overlays::create_submenu` is the one builder both nodes' Create
submenu comes from), and from the folder that holds the objects themselves — each object folder
offers a flat, kind-named **Create sequence** / **Create type** / **Create domain** and so on
of its own, which calls `open_for_new` directly rather than through `create_submenu`: the
folder has already said which kind, so the submenu's other children would be entries belonging
to the folders either side of it. `open_for_new` names a new object `new_domain`,
`new_sequence`, `new_synonym`, `new_xml_schema_collection` or `new_type` (an enum or an alias
type), and passes the connection's dialect to `ObjectDraft::blank`, which is what puts a SQL
Server sequence in SQL Server's form rather than PostgreSQL's.
One modal for all seven because the chrome, the footer, the change count and the
ending at `ddl_preview` are identical and only the middle section differs. **A routine is
not one of them**, and the split is made at the type level rather than by convention:
`ObjectDraft::from_item`/`blank` return `None` for a routine **and for an event**, and this
module's own `open_for_object`/`open_for_new` are what route those to `routine_editor` and
`event_editor` — so the tree, the palette and the menu all keep asking one function to open an
object, and a routine handed to a type form would have to be an explicit mistake rather than
whichever arm compiles. That routing is also why both doors take the whole `&SchemaActions`
bundle rather than the two source-fetch closures the editors behind them actually want
(`&RoutineSrcFn`, `&EventSrcFn`): the kind is decided *inside*, so naming both would put two
parameters at every call site of which exactly one is ever used — the narrowing pass that took
all three files off `whole_ui_gate`'s list is written up under `lib.rs`.
Same
seed-local-signals-then-write-back rule as the other editors, and three more written down:
the list rows are keyed on `object_rev`, a **structural** counter, so typing into a row
doesn't tear it down and removing one doesn't leave its neighbour showing the old text;
an enum's values are **rows, not a newline-separated box**, because a label may contain a
newline and splitting one would rebuild the type around the split (data loss on apply,
not a failure); and a sequence's numbers go through `object_errors` beside the draft,
since the draft holds `i64` and a half-typed `-` has nowhere to live in it — writing
nothing back on a failed parse would silently swallow what somebody typed.
`is_editable_object` is the entry point's gate: an identity column's counter is listed
and alterable but not editable-as-an-object, the call a materialized view gets. **SQL Server's
sequences, synonyms, alias types and XML schema collections answered no** (`item.tsql()`)
while there was no form for one, and since the palette's gate delegates here, Find-Anywhere
withheld them while the tree's filter found them. They have a form now, the `item.tsql()`
exclusion is gone, and the palette offers them again — the divergence the schema-search
invariant records has ended (under *Architecture invariants*). **A PostgreSQL extension answers
no**, having no form at all (`ObjectDraft::from_item`/`blank` are `None`): it is created from
`database_editor`'s picker and dropped from its row. So the tree's filter finds one and the
palette, through the same delegation, does not offer it.
**`tsql_form` is SQL Server's four forms**, over `TsqlObjectDraft` through `tsql_field`,
`tsql_toggle` and `with_sequence`. **Numbers are text here, as the model holds them**: a
`decimal(38,0)` sequence's bounds pass any integer type the form could parse into, so nothing
goes through `object_errors`, as the PostgreSQL sequence form's `i64` fields do, and
`TsqlObjectDraft::problems` judges them. A sequence
has Name, *Stores* (its type, free text), Increment, Minimum, Maximum, a Cycle toggle, and a
Cache toggle beside a *Cache size* field whose empty value means the server's size — the
three states `None`, `Some(None)`, `Some(Some(n))` in two controls, **both written from one
pair of signals** rather than each from the draft: a toggle that rebuilt the cache from the
draft forgot the size its field still showed when turned off and on, and a size typed while
it was off went nowhere, so the preview wrote a bare `CACHE` under a field reading a number.
**Start appears on a new
sequence alone**: `ALTER SEQUENCE` has no `START WITH`, so on an existing one the field could
only ever mean dropping it, and a *Position* section's *Restart at* — labelled with the value
the counter last handed out — is what moves the counter instead. It is also where a sequence
the plan has to re-create (a new type) starts, and the control `validate` names when a raised
minimum or lowered maximum leaves the old start outside the range (both under `ddl.rs`). A
synonym is four fields,
Server, Database, Schema and Object (`TsqlObject::synonym_fields`, under `schema.rs`); an alias
type a *Base type* and a Nullable toggle; a collection one multi-line, monospaced *Schemas*
field.
**Its words are the catalogue's** (`object-editor-*`) — sections, fields,
toggles and their hints, the notes and placeholders that have words in them — with Cancel,
Preview SQL and the change count the shared `dialog-cancel`, `editor-preview-sql` and
`editor-change-count`, and the value rows' arrows the new shared `editor-row-move-up` /
`editor-row-move-down` beside the trigger editor's `editor-row-remove`. **The title is two
whole messages** (`object-editor-title-{edit,create}`) around `i18n::object_kind`'s word, built
by `editor_title`, where it had been `format!("Edit {} …", kind.label())` — so a language can
put the verb last (`the_title_reads_as_it_did_in_english`). `restart_label` is both engines'
*Restart at* label, and **the counter goes to Fluent as text, not a number**: a Fluent number
is an `f64`, which loses digits past 2^53, and it is the server's value to the digit
(`the_restart_label_prints_the_counter_as_the_server_has_it`). **A number field's message is
also the key `object_errors` withdraws it by** — `object-editor-not-whole-number`, naming the
field — so `num_field` builds it once per field: built afresh per report, a message that came
out differently would never be withdrawn (`a_number_fields_error_names_the_field`).
`tsql_field`'s placeholder is a closure (`FieldCfg::placeholder_of`), so a SQL sample and a
catalogue phrase (*this server*) go in alike, and the synonym's four parts are a
`[(String, fn() -> String); 4]` table; `bound_toggle`/`tsql_toggle` and `num_col` take owned
`String`s. The NOT NULL toggle's label is the SQL clause and stays, as do the SQL samples and
identifier placeholders (`domain_name`, `sequence_name`, `bigint`, `nvarchar(50)`,
`<xsd:schema …>`) and the storage-type dropdown's `smallint`/`integer`/`bigint`.
The footer's refusals and Preview's readiness read `ObjectDraft::problems`, not its English
wrapper `validate`: the footer still shows the editor's own `object_errors` first and, failing
those, the draft's *last* problem (`pop()`), which `object_problem_text` words from
`object-problem-*` — `NotWhole` a whole message per `SequenceField` rather than the field's
name spliced in, and the figures as `i64::to_string` prints them — a plain integer, which
stays ungrouped in every language (*Figures in the interface's marks*). Held by
`english_says_every_object_problem_as_core_does`.
