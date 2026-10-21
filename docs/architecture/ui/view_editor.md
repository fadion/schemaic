# `ui/view_editor.rs`

`view_editor.rs` — the **view** modal (tree "Edit" on a view, "Create view" on a database/
schema node *and* on the editor's right-click when the statement under the caret can be a
view body — `ddl::can_be_view_body`, which seeds the draft with it), over `core::ddl`'s
`ViewDraft`. Not a designer tab: a view is a name and a
`SELECT`, so it's one form on the shared modal chrome, ending at the same `ddl_preview`.
Same seed-local-signals-then-write-back rule as the designer (the form is built once per
open; only the footer is keyed on the draft). The options are shown because they're
*carried* through a replace, and the PG "re-create instead of replacing" toggle is the
override for the cases `ddl::pg_replaceable` can't read off the statement. The `SELECT` body is
SQL-coloured in the target's dialect (`FieldCfg::sql`, under `lib.rs`).
The form is built **per engine** — check option for MySQL and PostgreSQL, the security/definer
block for MySQL, the re-create toggle for PostgreSQL, and a "Column names" field for
`ViewOptions::column_list` on SQLite and SQL Server. The first and last ask
`ddl::supports_view_check_option` and `ddl::view_keeps_column_list`; both were `== Sqlite`,
which, once SQL Server's editor opened, offered it a `CASCADED`/`LOCAL` picker nothing writes
and hid the column list its `ALTER VIEW` has to restate. SQL Server's attributes
(`SCHEMABINDING`, `VIEW_METADATA`) have no control: they are carried through the draft, not
edited. `needs_algorithm` asked `!= Postgres`, which sent a SQLite
connection off to fetch a `SHOW CREATE VIEW` algorithm; it asked `== MySql` after that, and
now asks `ddl::supports_view_security` (MySQL alone), as the security/definer block does. The
re-create toggle asks `ddl::view_replace_can_refuse` (PostgreSQL alone), the same predicate
`diff_view` asks before it will re-create, so the toggle is offered exactly where the plan
reads it. Both are pinned as a truth table in
`the_view_editors_options_are_asked_as_capabilities`, and with them the file has no entry left
in `engine_comparison_gate`'s budget (3 → none).
`is_editable_view` is the entry point's gate — a materialized view reaches no editor, and
the two things its context menu can still do to it are **Drop** and **Refresh view**
(`ObjectEntries::refresh_view`, over `ddl::refresh_view_change`), neither of which opens a
form. Its materialized half is `ddl::is_materialized_view`, the same predicate the menu
asks to *offer* the refresh: two hand-written copies are two chances for the editor and
the menu to disagree about one node. **The whole decision is `ddl::view_is_editable`'s now**,
which also refuses a SQL Server view whose text the server hides or whose header could not be
read (under `ddl.rs`): an encrypted one opened on an empty body, and Apply wrote it back
unencrypted. `open_for_view` asks it again at the launch rather than trusting the disabled
entry (`the_edit_gates_refuse_what_the_editors_cannot_express`, in `trigger_editor.rs`).
**Off `whole_ui_gate`'s list, 6 to zero, and the six were exactly the opening path** — the
overlay, the form and the two field helpers had come down a pass earlier. They went the way
`routine_editor.rs`, `event_editor.rs` and `object_editor.rs`'s doors went rather than the way
the five `…Ctx` modals did: **a door names the fetch it ends in**, so `open_for_view` takes
`(ConnUi, SchemaUi, DdlUi, &ViewAlgoFn)` and the `fetch_algorithm` it ends in takes `(DdlUi,
&ViewAlgoFn, …)`, while `open_for_new`, `open_blank` and `open_from_query` take `(ConnUi,
DdlUi)` — `open_from_query` took the `SchemaUi` too until `default_schema` stopped reading the
tree (under `table_designer.rs`) — and `open_editor` takes `DdlUi` alone. A `ViewCtx` would
have been the wrong answer here: these are not one modal's views sharing a body of state, they
are four separate entry points into it, and **three of the four want no fetch at all** — a ctx
would have handed the `ViewAlgoFn` to every one of them,
which is the widening-dressed-as-a-convenience the `import_view.rs` entry already names, in its
other form. Nothing about the modal's behaviour moved with it: on MariaDB a view's **Edit view**
still opens with its body, check option, SQL security and definer, and the footer still reads
*No changes* seconds after opening — which is the state `fetch_algorithm`'s both-sides patch
exists to preserve, since patching only the draft makes a view open already-changed. **That
check cannot tell a fetch that landed from one that never fired**, though, because both leave
the diff clean: MariaDB reports `ALGORITHM` in `information_schema` where MySQL 8 does not, and
an `UNDEFINED` one folds to `None` anyway. The round trip itself is MySQL 8's path and is
covered by nothing here. The four doors are called from `overlays.rs` (the tree's Edit view and Create ▸
View), `properties.rs` (the properties modal's Edit) and `lib.rs`'s `create_view` callback (the
editor's *Create view from this query*).
**Its words are the catalogue's** (`view-editor-*`) — labels, sections, the
column-names placeholder, the re-create toggle and its hint, both titles — with **Cancel**
`dialog-cancel` and the footer's other two named for no one editor: `editor-preview-sql`, which
the Copy to modal, the database editor and the routine and event editors now share, and
`editor-change-count`, shared with the routine and event editors' footers, whose `[0]` case is
*No changes*
(`the_shared_counts_read_as_they_did` pins 0, 1 and 7). The `view_name` and `SELECT …`
placeholders and the `CASCADED`/`LOCAL`/`DEFINER`/`INVOKER` choices are SQL and stay. The
footer's refusal and Preview's readiness read `ViewDraft::problems`, worded by
`view_problem_text` from `view-problem-*` (the editors' refusals, under `i18n.rs`), held by
`english_says_every_view_problem_as_core_does`.
