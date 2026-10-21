# `ui/lib.rs`

`lib.rs` (with `grid.rs`, one of the crate's two largest modules) — the `Ui` struct + bundles,
shared model/state
types, `workspace`/`body`/`center`/`header`/`footer`, `edit_field`/`FieldCfg`,
terminal panel.
**How far the root `Ui` bundle travels is a ratchet now — `whole_ui_gate`.** `Ui` has 36 fields
and transitively pulls `OverlayUi`'s 51 and `DdlUi`'s 40, and it was passed *unnarrowed* into
roughly 140 signatures across this crate while the per-domain child bundles that exist for
exactly this purpose narrowed a call at **two** sites in the whole of it — the state the gate
found on the day it landed, not the state now. Two costs, both
concrete. No such function can be unit-tested: `ddl_preview::connection_label` touched 1 of the
36 and encoded a real decision — the `connection N` fallback — and testing that fallback meant
building a 36-field bundle inside a Floem reactive scope. And a signature stops saying what it
depends on: `schema_tree::object_row` took `ui: Ui` and dereferenced it *zero* times, the only
use being a clone forwarded into `object_editor::open_for_object`, so it held live handles to
~127 signals it never read and a change to `OverlayUi` had no readable blast radius. **Wholly
past tense now — don't go looking for that shape.** The door narrowed first, which turned the
one use into four named fields; then the row itself did, and it takes the `SchemaTreeCtx` its
caller was already holding in place of a `Ui` plus four of that struct's own fields
(`context_menu`, `dialect`, `nav`, `indent_levels`) — the signature had been four pieces of the
context passed one by one with a duplicate of its `ui` beside them. Four
one-field views went first — `connection_label` takes the one `RwSignal<Vec<Connection>>` and
its decision is `core::connection::label_of` with a three-line test, and
`compare_view::left_db_type`/`side_label` and `event_editor::taken_names` were the same shape.
**Then the DDL editors and the designer went through, and the split that fell out of them is
what a next pass should reach for first.** A **form or list** function takes the child
bundle — `DdlUi` in every one of these, with `OverlayUi` named beside it where a suggestion
chevron needs the dropdown channel (`object_editor`'s `form`/`domain_form`, `table_designer`'s
`table_section`/`column_form`, `database_editor`'s `form`) — while a **field helper** takes the
bare signal it writes: `RwSignal<TableDraft>`, `RwSignal<EventDraft>`, `RwSignal<RoutineDraft>`,
`RwSignal<TriggerSetDraft>`, `RwSignal<ViewDraft>`, `RwSignal<DatabaseDraft>`. A field that binds
one signal should say which one. **The chevron is a rider on that half too, not only on the
form half**: `database_editor::optional_field` is a field helper and still takes `OverlayUi`
beside its `RwSignal<DatabaseDraft>`, because the fields it builds are free text *plus* a
`suggest_chevron` — so the rule is "the bare signal it writes, plus the dropdown channel where
the field itself carries one", never "a helper may not name a bundle". Because the
child bundles are `Copy`, narrowing deletes clones rather than merely renaming a parameter: the
designer's three growing lists each kept an `add_ui` and a `del_ui` alive for their two action
closures, four of its lists were cloning a `Ui` *per row* to deliver one `Copy` signal, and
`event_editor::schedule_form` cloned one three times to hand a dropdown a single signal.
**The arm that stops it is an *opening* path — and it has since been beaten, so read it as a
claim to check rather than a licence.** Anything reaching the schema tree or another
editor's `open_for_*` looks like it genuinely wants the root bundle — `edit_ctx` stood on that
list too until it was read properly, which is the correction below — and two were written into
the budget comments so the next pass would not re-argue them: `trigger_editor`'s `pg_action` was
where the "edit this function" button opened the *routine* editor, and `form` the only thing
that could hand it a root bundle, which was why that file stopped at **7** where its siblings
reached 5 and 6 — both halves of which have since stopped being true, worked through below;
and the six shared readers in `table_designer.rs` — `edit_ctx`, `db_flavour`, `loaded_table`,
`loaded_schema`, `default_schema`, `table_names` — are called from across the crate. Those six
have now narrowed, and **the plan once written here was wrong about one of them**: the four
that read only `db_nodes` (`db_flavour`, `loaded_table`, `loaded_schema`, `table_names`) take
`SchemaUi` — `db_flavour` has since gone one step further, to the bare
`RwSignal<Vec<ConnNode>>`, because its second caller is `completion::recompute_completions`,
which holds a `CompletionCtx` and no `SchemaUi` at all, and one shared lookup beats a second
spelling of the same `db_nodes` walk in a file that would then have to be kept in step with
this one. `edit_ctx`, by contrast, reads `ui.conn.active_conn` and `ui.conn.connections` and
touches the schema tree not at all, so it takes `ConnUi`;
`default_schema(conn: ConnUi, ui: SchemaUi, database: &str)` took both, because it asked
`edit_ctx` for the dialect and then the tree whether the database's namespaces loaded — and it
has since joined `edit_ctx` on `ConnUi` alone, the tree half having answered `None` for any
database with no objects yet (under `table_designer.rs`).
What had held `edit_ctx` back was its **39 call
sites** — the count, not the shape.
**The narrowing propagated, and that is the real lesson of that pass** — the lesson
`ddl_preview::preview_container` had already taught below, now with numbers. A caller cannot be
narrower than what it calls, so six shared helpers coming down took three budget entries with
them: `table_designer.rs` **10 → 4**, leaving `open_for_table`, `preview_draft_edit`,
`open_for_new` and the overlay; `object_editor.rs` **5 → 4**, its own private `loaded_schema` —
a near-copy of `table_designer`'s — following onto `SchemaUi`; and `trigger_editor.rs`
**8 → 7**, since `sibling_trigger_names` reads `loaded_schema` and nothing else. That last one
took nothing away from the sentence above at the time — `pg_action` and `form` were still on the
root bundle and were still why that file stopped where it did. They are not, and it does not;
see below.
**An editor's *overlay* is the usual holdout, and `view_editor_overlay` is the first one that
was not.** Every sibling editor's overlay kept the root bundle because something under it
opens something else; this one has no such path — `fetch_algorithm` is reached from
`open_for_view`, not from the modal — so the draft, the target, the body's row cap
(`view_rows`, read by `form`) and the hand-off to `ddl_preview::open_preview` are the whole of
what it touches, it takes `DdlUi`, and `modals.rs` passes `ui.ddl`. It has since taken a
`SchemaUi` beside it (`ui.schema`), read at *Preview SQL* for what uses the view under the
name a rename takes from it (`ddl_preview::with_used_by`) — still no opening path. That left
`view_editor.rs` at **6**: exactly the six opening functions, `open_editor`, `open_for_view`,
`fetch_algorithm`, `open_for_new`, `open_from_query` and `open_blank` — all six of which have
since come down too, in the entry after `script_view.rs`'s below.
`database_editor_overlay` went the same way one step later, and **what had been holding it was
a helper, not its own shape** — which is the more useful half of the lesson. Its direct reads
were only `ui.ddl` and `ui.overlay`, the two-bundle shape `object_editor`'s `form`/`domain_form`
already established as narrower than naming neither; what kept it on the root bundle was its
Preview SQL button calling `ddl_preview::preview_container(&Ui, …)`, a helper that forwarded
`ui.ddl` and read nothing else. Narrowing that one signature (and `preview_account` beside it,
the same shape) took `ddl_preview.rs` 6 → 4 **and** let the overlay take `(DdlUi, OverlayUi)`,
which took `database_editor.rs` to **3** — `open_for_new`, `container_names` and `fetch_roles`.
A caller cannot be narrower than what it calls, so a `&Ui` helper is a budget floor under every
file that reaches it; look down the call graph before concluding a function needs the root.
**Then three whole files came off the list, doors included, and what unblocked them was naming
one dependency.** `routine_editor.rs` (6), `event_editor.rs` (5) and `object_editor.rs` (4)
declare no `ui: Ui` parameter at all now, and they had to move together because
`object_editor`'s doors *are* the other two's — `ObjectDraft::from_item`/`blank` return `None`
for a routine and for an event, and `open_for_object`/`open_for_new` are what route those on.
The single thing keeping all three on the root bundle was that a door ends in a `fetch_source`
reading the object's real body off the server: `routine_editor::fetch_source(ui: &Ui)` was the
only function in its whole module reaching `ui.schema_actions`. It takes the one closure it
calls now (`fetch_source(d: DdlUi, source: &RoutineSrcFn)`, and `&EventSrcFn` in the sibling),
and each door carries that fetch beside `ConnUi` and `DdlUi` — the dependency stated rather
than reachable. **`object_editor`'s two doors take the whole `&SchemaActions` child bundle
instead of both closures, and that is deliberate**: they route by object *kind* to whichever
editor owns it, so naming `&RoutineSrcFn` and `&EventSrcFn` separately would be two parameters
of which exactly one is ever used, at every call site, for a choice made inside the callee. The
overlays needed no fetch at all — `fetch_source` is reached from `open`, never from the modal,
which is exactly the shape `view_editor_overlay` established above — so
`routine_editor_overlay` and `event_editor_overlay` take `(DdlUi, SchemaUi)` and
`object_editor_overlay` takes `(DdlUi, OverlayUi)`. The call sites are where it is paid back:
`schema_tree::object_row` was cloning a whole `Ui` **per row** to reach the door and the
palette's `open_object` closure was holding one, and both now carry three `Copy` bundles and
one `Rc<SchemaActions>`.
**That pass also spent the reason written against `trigger_editor.rs`'s 7 — and the lapse, not
the narrowing, is the entry worth keeping.** This document and that file's `BUDGET` comment both
said `form` and `pg_action` were *deliberately* not narrowed, because `pg_action` is where the
"edit this function" button opens the routine editor and only a root bundle could be handed on;
it stopped at 7 "for a reason, not for want of another pass". That reason was true when it was
written and was spent the moment the routine editor's own doors took `(ConnUi, DdlUi,
&RoutineSrcFn)`, at which point `pg_action` turned out to read exactly those three and nothing
else. Nothing could catch it: the ratchet only forbids going *up*, so **a justification for a
floor can lapse silently while the number it defends stays put** — a "deliberately not narrowed"
describes the *callee*, not the file, so re-read it against the callee before trusting it.
`trigger_editor.rs` is off the list now, 7 to zero: `pg_action` and `form` take `(ConnUi, DdlUi,
&RoutineSrcFn)`, which is exactly the union those two buttons hand on, and the rest followed —
`fetch_sources(DdlUi, &TriggerSrcFn)`, `fetch_functions(DdlUi, &TriggerFnFn)`,
`open_editor(DdlUi, &SchemaActions, …)` and `open_for_table(ConnUi, SchemaUi, DdlUi,
&SchemaActions, …)`, the two `&SchemaActions` for `object_editor`'s reason — the dialect decides
*inside* `open_editor` whether `trigger_source` or `trigger_functions` runs.
**`trigger_editor_overlay` is the one overlay in the crate that genuinely needs a fetch**, and
it is the real exception to *the overlays needed no fetch at all* two paragraphs up rather than a
miss. Every sibling's fetch runs from an `open`,
so the door can absorb it; this one has a *second, later* trigger — `refetch_functions_on_return` re-reads the function list when
the nested routine editor closes back to it, because a function just created has to appear in the
dropdown — so the fetch cannot be left at the door. **It takes the two it needs by name**,
`(ConnUi, DdlUi, TriggerFnFn, RoutineSrcFn)`, where it *stored* an `Rc<SchemaActions>` — the
campaign's one site that held the bundle rather than forwarding it, keeping 31 closures alive for
exactly two fetches. Naming them costs one parameter, four in all, three under the limit the rest
of the campaign cites as its reason for bundling; `whole_ui_gate` counts only `ui: Ui`, so
nothing measured this one.
**`account_editor.rs` came off in the same pass, 6 to zero, and needed no such argument.** Its
three doors (`open_for_new`, `open_for_reset`, `open_for_grant`) read the connection registry —
`edit_ctx` then, `door_read_only` since — and write the
draft and nothing else, so each takes `(ConnUi, DdlUi, …)`; `grant_form` took `&Ui` for
`ui.overlay` alone and takes `OverlayUi` beside the `DdlUi` it already had; and the two overlays
fall out of those — `account_editor_overlay(DdlUi)` and `grant_editor_overlay(DdlUi, OverlayUi)`.
Its callers in `users_view.rs` (New account, Privileges, Reset password) carry `(ui.conn,
ui.ddl)` rather than a `Ui` clone — which is how a propagating narrowing first shows up, as
rewritten call sites rather than as a lower number, and is the easy thing to overcount.
**Then the number did move, 8 → 5**, because those rewritten call sites left three functions
holding a bundle for nothing: `write_gate` takes `ConnUi`, `new_account_row` `(ConnUi, DdlUi)`
and `actions_row` `(ConnUi, DdlUi, OverlayUi)` — Grant, Reset and Drop, which is exactly what
the three doors under them now name. Read that as the second half of the same step rather than
a separate pass: a caller cannot be narrower than what it calls, so every door that comes down
leaves its callers narrowable and *nothing points that out* — the ratchet is satisfied by a
file that stands still. **The five left in `users_view.rs` were a different shape and got the
different answer this entry predicted — and the *difference* from `SchemaTreeCtx` is the part
worth copying.** The modal root, its two panes and the footer each reached `OverlayUi` *and* an
action out of `schema_actions`, on top of `target`, `gate` and a `FocusRing` they already carry,
so all four take a browser-local `UsersCtx` — the three `Copy` bundles (`ConnUi`, `DdlUi`,
`OverlayUi`) and the two fetch closures (`principals`, `grants`) as `Rc`s — the way
`schema_tree`'s rows take a `SchemaTreeCtx`. That struct, at the time, carried a `ui: Ui`
*field* — which the counter sees, because it scans lines rather than signatures, and which was
one of the three `schema_tree.rs` then declared. **It does not any more**, and the reason it
came off is this paragraph's own rule read back at it: the field turned out to be reached in
three functions for six things, while the comment on it named one, so it is six named fields
now and that file is at 2. `UsersCtx` holds no root bundle at all, so it is built by
**naming the reads at the call site** — `UsersCtx::new(ui.conn, ui.ddl, ui.overlay,
&ui.schema_actions)`, once, in `modals.rs` — rather than by a constructor that takes `&Ui` and
can quietly grow. **A context struct is only a narrowing if it is narrower than what it
replaced**: one that holds the root bundle relocates it. `open_for_server` deliberately stayed
*off* the ctx — it is the one function in the module that touches neither fetch, so it takes
`(ConnUi, OverlayUi, conn_id, database)` and clones no `Rc` to write five signals, and its two
call sites (the SCHEMA gear in `overlays.rs`, the blank-space menu in `schema_tree.rs`) pass
`ui.conn, ui.overlay`. Nothing about the browser's behaviour moved with it: the `dyn_container`
key still carries `target_read_only`, `write_gate` still runs in the builder, and
`the_browser_key_carries_the_read_only_flag` passes untouched.
**`import_view.rs` came off next, 9 to zero, and it is that lesson refined by a module where the
split was already visible: the drivers take a ctx, the renderers take the bundle.** Four of the
nine functions only read and write the modal's own signals, and now say so by taking `ImportUi` —
`mapping_row`, `preview_table`, `issue_list` and `mapping_step`; the four that *also* reach a
fetch, the read-only flag or the schema tree take a new `ImportCtx` — `probe`, `run_import`,
`source_step` and `import_overlay`. A context struct on a function that needs one bundle would be
a widening dressed as a convenience, which is the trap on the *other* side of the one
`SchemaTreeCtx` fell into. `ImportCtx` holds six things and no `Ui` — `import: ImportUi`,
`conn: ConnUi`, `schema: SchemaUi` and the `probe`/`run`/`cancel` closures — and is built by
naming the reads at its one call site, `ImportCtx::new(ui.import, ui.conn, ui.schema,
&ui.schema_actions)` in `modals.rs`. `open_import` stayed off it for `open_for_server`'s reason:
a door that only resets the modal's own signals has no business cloning three `Rc`s to do it, so
it takes `ImportUi` alone and its one call site — the table context menu's Import entry in
`overlays.rs` — passes `ui.import`. The mapping step's column list was cloning a whole `Ui` **per
row** to reach `mapping_row` and copies a `Copy` bundle now, the same cost the schema tree's
object rows shed. Nothing about the modal's behaviour moved with it: the two places that read
`ui.schema_actions.import_cancel` call `ctx.cancel`, which is that same closure, so the exit
path's `loading`-not-busy rule and the `TargetVerdict::Cancel` arm are untouched, and
`run_import`'s live read-only read is `ctx.conn.connections`, still `with_untracked` in the same
synchronous step as `accept_launch`.
**`dump_view.rs` went the same way directly after it, 9 to zero, and the field it nearly shipped
without is the entry worth keeping.** `table_picker` takes `DumpUi`; `open_dump`, `run_export`,
`run_dump`, `run_files`, `launch_files` and `dump_overlay` take a `DumpCtx`; `watch_connection`
takes `(DumpUi, ConnUi)`, the two bundles it watches, rather than a ctx it would read two fields
of. `DumpCtx` holds eight things and no `Ui` — `dump: DumpUi`, `script: ScriptUi`,
`conn: ConnUi`, `overlay: OverlayUi` and the `tables`/`run`/`files`/`cancel` closures — and is
built by naming the reads at each of its **two** call sites, `DumpCtx::new(ui.dump, ui.script,
ui.conn, ui.overlay, &ui.schema_actions)`, in `modals.rs` for the overlay and in `overlays.rs`'s
`export_submenu` for the door. **The `overlay` field was nearly left out, because a `ui.<field>`
census misses aliases**: `launch_files`' `FilesOutcome::WouldReplace` arm raises the app-wide
confirm, but it does so through a local alias — `ui_retry.overlay.confirm` as the module then
stood, `ctx_retry.overlay.confirm` now — so a grep for `ui.overlay` came back empty on the one
function in the module that reaches it. The compiler caught it, but only after the struct had
been written a field short — a census that read the aliases too would not have needed it to.
Nothing about the modal's behaviour moved with the
narrowing: `dump_overlay`'s exit calls `ctx.cancel`, which is the same closure
`ui.schema_actions.dump_cancel` was, and `launch_files`' `accept_dialog_launch` reads
`d.running`/`d.target` — the same two signals under the name already bound in that function.
`export_progress_overlay` stayed off the ctx and now says so in its signature,
`(ExportUi, Rc<dyn Fn()>)`, with `modals.rs` passing `ui.export, ui.tab_actions.export_cancel`.
**`compare_view.rs` closed the run, 8 to zero, and what it settled generalises past this file:
whether a ctx is taken by reference or by value is decided by the *return type*, not by taste.**
The split is `import_view.rs`'s again — `open_compare`, `body_for`, `ready_body` and
`filter_bar` touch nothing but the eight `compare_*` signals and take `OverlayUi` (`reset`
already did), while `compare_overlay`, `sources_bar`, `footer` and `open_plan_preview` take a
`CompareCtx`: `overlay: OverlayUi`, `conn: ConnUi`, `ddl: DdlUi` and the
`fetch`/`cancel`/`list_dbs` closures, no `Ui`, built once by naming the reads at its one call
site — `CompareCtx::new(ui.overlay, ui.conn, ui.ddl, &ui.schema_actions)` in `modals.rs`. The
door stayed off it for `open_for_server`'s and `open_import`'s reason, and its call sites (the
database row's and the table or view row's **Compare with** in `overlays.rs`) pass `ui.overlay`.
**The three that return
`impl IntoView` take the ctx *by value*, and that is forced rather than stylistic.** In edition
2024 a return-position `impl Trait` captures every lifetime in scope, so
`fn sources_bar(ctx: &CompareCtx, …) -> impl IntoView` borrows the caller's local for as long as
the view lives and does not compile — "`ctx` does not live long enough". `users_view`'s
`list_pane`/`detail_pane`/`footer` take `&UsersCtx` only because they return `AnyView`, which is
erased, and `open_plan_preview` returns `()` and takes the reference. So read the return type
before choosing: `AnyView` takes the borrow, `impl IntoView` takes the value. Otherwise the
by-value ones read as an oversight and the tidy-up into a borrow will not build. `sources_bar`
pulls `list_dbs` out of the ctx *before* the `dyn_container` builder closure for the neighbouring
reason — a builder is `Fn` and cannot move a field out of a captured struct. Nothing about the
modal's behaviour moved with the narrowing: the `dyn_container` key, the once-per-pair fetch
effect (still keyed on the branch rather than reading `target` inside), the cancel-on-close and
the plan hand-off to `ddl_preview::open_preview` are untouched, and `open_plan_preview`'s live
read-only read is `ctx.conn.connections`, still `with_untracked`.
**`script_view.rs` followed it, 6 to zero, and the field it nearly shipped *with* is the mirror
of `dump_view.rs`'s near-miss.** The module is nearly all driver — a file picker, a probe and a
run — so `pick_file`, `run_script` and `script_overlay` take a `ScriptCtx`, the overlay by value
because it returns `impl IntoView`, and the interesting half is the three that do not.
`open_script` takes `(ScriptUi, DumpUi)` for `open_for_server`'s and `open_import`'s reason — it
only writes signals, and `DumpUi` is there because opening this modal clears the export modal's
target, the sibling it shares a modal-layer tuple element with. `watch_connection` takes the two
bundles it watches. And **`policy(conn: ConnUi, layout: LayoutUi, dialect, conn_id) ->
GuardPolicy` names the write guard's two halves**: a policy assembled out of a context struct
would hide which parts of the app decide whether a `.sql` file may run, and this is the module
whose guard the write-guard invariant's own text cites (`sql::script_verdict`,
`ScriptRequest::approved`). `ScriptCtx` holds six things and no `Ui` — `script: ScriptUi`,
`conn: ConnUi`, `layout: LayoutUi` and the `probe`/`run`/`cancel` closures — built once by
naming the reads, `ScriptCtx::new(ui.script, ui.conn, ui.layout, &ui.schema_actions)` in
`modals.rs`. **It first carried a `dump: DumpUi` field, because `open_script` read it, and when
`open_script` came off the ctx that field had no reader left**: a ctx field with one reader
follows that reader out, and one left behind is a claim about what the modal touches that
nothing checks. The compiler caught this one only because the field went *completely* unread —
a field still read by some other path would have stayed, silently overstating the module's
reach. With `DumpCtx`'s alias census above that is the pair: a ctx can be wrong in both
directions. Nothing about the modal's behaviour moved with it — `run_script` still asks `policy`
and `ScriptRequest::approved` in the same synchronous step as `accept_launch`, and the exit
path's `stop` is `ctx.cancel`, the same closure `ui.schema_actions.script_cancel` was.
**`view_editor.rs` came off after all five of those, 6 to zero, and it is the case where a ctx
would have been wrong** — worth writing down precisely because the run had just established the
ctx pattern five times over. Its six were the opening path and nothing else (the file's own
`BUDGET` comment said so), so they went the way `routine_editor.rs` and its two siblings' doors
went: **a door names the fetch it ends in**. `open_for_view` takes `(ConnUi, SchemaUi, DdlUi,
&ViewAlgoFn)` and `fetch_algorithm(DdlUi, &ViewAlgoFn, …)` — the one function in the module that
reaches the server — while `open_for_new` and `open_blank` take `(ConnUi, DdlUi)`,
`open_from_query` took that plus the `SchemaUi` `default_schema` then asked (the same pair
since `default_schema` stopped reading the tree), and `open_editor` takes `DdlUi`. A `ViewCtx` would have handed the `ViewAlgoFn` to all four doors: these are not one
modal's views sharing a body of state, they are four separate entry points into it, and three of
the four want no fetch at all — the "widening dressed as a convenience" `import_view.rs` already
names, in its other form. **So a ctx is for a modal's *body*, where the same bundles are wanted
repeatedly; the *doors* into a modal name what each one reaches, because they differ from each
other.** Its call sites are `overlays.rs` (the tree's Edit view and Create ▸ View),
`properties.rs` (the properties modal's Edit) and `lib.rs`'s `create_view` callback (the
editor's *Create view from this query*).
**Three named bundles can still be the narrower signature, and can say something the root one
hid.** `preview_change` takes `(ConnUi, DdlUi)` and `preview_proposal`
`(ConnUi, SchemaUi, DdlUi)`; the count looks like a step backwards until you read what `conn`
being *named* asserts — these two answer a context-menu shortcut off the **live switcher**,
where their `PlanTarget` siblings deliberately do not, and that is now visible at every call
site rather than only in the prose two paragraphs up. The rule is the signature saying what the
function depends on, not the parameter count.
**`overlays.rs` came down last, 13 to 4, and the *order* of that tranche is the whole of what
it teaches.** It had sat at 13 as the big one left and genuinely cross-cutting, and two of the
thirteen were not cross-cutting at all — they were downstream of two other files' doors. Six of
the nine that came off were the small menu overlays, which read their bundles into locals at the
top and never touch the root again: `popup_menu_overlay` and `tx_prompt_overlay` take
`OverlayUi`, `db_visibility_overlay` `(SchemaUi, Rc<dyn Fn(String)>)`, `conn_menu_overlay`
`ConnUi` beside the two `Rc`s it calls, `active_db_menu_overlay` `(TabsUi, SchemaUi, …)` and
`activity_menu_overlay` `(ActivityUi, LayoutUi, …)`. The other three are the interesting ones.
**`export_submenu` takes a `&DumpCtx`** — the ctx built for the export modal, reused here
because four of the five fields this submenu reads (`dump`, `script`, `conn`, `overlay`) *are*
its fields — and where it was cloning a whole `Ui` per format, six times per menu build, it
clones a `DumpCtx`; `context_menu_overlay` builds one for the whole menu and hands it to all
three arms that offer `Export ▸`. `DumpCtx` gained `edit_ctx()` for it, and gained it so `conn`
can stay private: the submenu wants the dialect and nothing else out of the registry, and a
`ConnUi` passed beside a ctx that already holds one is two names for one bundle.
**`schema_settings_overlay(conn, schema, ddl, tabs_ui, overlay, Rc<SchemaActions>)` and
`create_submenu(conn, schema_ui, ddl, &Rc<SchemaActions>, …)` take the actions bundle whole**,
which is `object_editor`'s doors' reason — every action either can run lives on that one
bundle — and for the gear it is also what a lint says: naming all four closures individually put
the signature at nine parameters, past clippy's `too_many_arguments` limit of seven. That is a
checkable instance of the judgement this section otherwise makes by hand, with a second opinion
attached to it.
**The four left each carry a reason that names a count rather than a callee**, which is what the
lapsed-justification rule above asks of a floor — it has to be re-readable.
`context_menu_overlay` reaches **seventeen** fields, the schema tree's whole right-click
surface: every editor door, both colour stores and their two savers, the import and export
modals, AI, the tab actions. `palette_commands` reaches nine, Find Anywhere running *every*
command the app has, so its dependency is the app; `find_overlay` reaches eight and is the same
shape, opening objects, tables, snippets and tabs; `error_modal_overlay` reaches five and was
already the recorded contrast. None of the four is blocked by a callee — they are wide because
what they do is wide.
**What did block two of the nine is the rule worth carrying out of this**: `create_submenu` and
`schema_settings_overlay` were each held there by exactly one callee still on the root bundle —
`database_editor::open_for_new(&Ui, …)` for the gear, that *and*
`table_designer::open_for_new(&Ui, …)` for the submenu — so until those came down neither could
be narrower than them. `database_editor.rs` went 3 to zero and `table_designer.rs` 4 to zero in
the same step, and each entry records it. Read it as the mirror of the propagating narrowing
above, where six shared `&Ui` readers coming down took `object_editor.rs` and
`trigger_editor.rs` with them: that one moves outward from the helper, this one asks the
question from the stuck end — **when an entry will not come down, look at what it calls before
concluding the entry is the problem.**
**`connection_import.rs` came off after that, 6 to zero, and it is the cleanest sweep of the
run** — the module has exactly one piece of state, so every one of its six functions read
`ui.conn.import` and nothing else out of the signal half, and all six take `ConnImportUi`:
`paste_row`, `source_buttons`, `row_list`, `skipped_note` and `footer` beside the actions they
call, and `conn_import_overlay(ConnImportUi, Rc<ConnActions>)`, which takes the actions bundle
whole for `schema_settings_overlay`'s reason — it is the one that hands all four down. **The
tell is a body that opens `let imp = ui.conn.import;` and never says `ui` again**: that function
was already narrow and only had to say so, and six of six were. It is the far end of the
spectrum from `context_menu_overlay`'s seventeen fields, and it is why **a file's number says
nothing about how hard it is to move** — this 6 took minutes, where `overlays.rs`'s remaining 4
are immovable.
**`lib.rs` itself went 6 → 5, and the five left are the app shell.** `terminal_panel` was the
one that came down: `(TermUi, ConnUi, Rc<TermActions>, Rc<dyn Fn(Option<String>)>)`, the last
being `tab_actions.open_db_cli`, named because it is the one action the panel reaches outside
its own domain, while `TermActions` goes in whole because the panel reads twelve closures off it
and naming twelve is the `schema_settings_overlay` case several times over; the `ConnUi` is
there for a read-only memo. It has since gained a `LayoutUi`, after `ConnUi`, so its gear can
open Settings on the Terminal section (`settings::open_settings_at`). **Four of the five are
wide by nature** — `workspace` reaches eleven
`Ui` fields, `center` **eighteen** and is the widest function in this file, `footer` seven,
`header` **nine** — so naming any of them lands at or past clippy's seven-argument limit, which is
the lint agreeing with the gate, except that here there is no single child bundle to take
instead, because what they span *is* the app. **The fifth, `body`, is a pass-through blocked by
its children**: its own reads are two (`layout`, `persist_layout`), but it clones the root
bundle three times to hand to `schema_panel`, `center` and the right panel, so it cannot be
narrower than the widest of them. That is `create_submenu`'s shape before the two editor doors
came down — a number held there by a callee rather than by its own reads — with the difference
that this one has nowhere to go, because the callee holding it is the app's centre pane.
**`settings.rs` and `properties.rs` came off next, and the pair is this campaign's two answers
side by side.** `settings.rs` went 4 to zero the way `connection_import.rs` did — narrow
already and only having to say so, each of its four modals owning one domain and reading nothing
else: `term_settings_overlay(TermUi, Rc<TermActions>)`, `ai_settings_overlay(AiUi, ConnUi,
Rc<AiActions>)` — the `ConnUi` being the data-access level the AI pane reports —
`theme_settings_overlay(LayoutUi, open_config_dir, CliCommand)` and
`help_overlay(LayoutUi)`, which reads a
single signal. The first two have since become Settings sections and the third
`settings_overlay(SettingsCtx)`, eight fields being past clippy's seven — the module's entry has
the current shape. `properties.rs` went 5 to zero the other way, on a `PropertiesCtx`, and the
footer is why: **Edit** routes to the table designer or the view editor, so the panel wants what
those two doors want on top of the four arguments `footer` already carries, which named one by
one is nine parameters — past clippy's seven, so the lint and *take the child bundle* agree and
the ctx is what they agree on. `open_for_table` stayed off it for `open_for_server`'s and
`open_import`'s reason a fourth time. **The one to carry forward is `properties::stats_body`,
which reads nothing at all**: it took a `&Ui` purely to hand down to `count_row`, whose two
actions are the only reads on that path, and it carries a `&PropertiesCtx` now — the same shape,
honestly labelled. A parameter that exists only to be passed on is still a dependency the
signature is claiming, and that one was claiming the whole bundle.
**`modals.rs` went 5 → 2 in the same tranche, and the two left are the end of that line.** The
three group predicates take the bundles they read — `ddl_modals_up(OverlayUi, ImportUi, DumpUi,
ScriptUi, DdlUi)`, `workspace_modals_up(OverlayUi, BlobUi, DdlUi)` and
`settings_modals_up(TermUi, AiUi, LayoutUi)` — each already a list of `Copy` flags read at the
top and never touched again; the last is `settings_modals_up(LayoutUi)` now, the terminal's and
the assistant's modals having become Settings sections. `modal_layer` reaches **twenty-one** `Ui` fields and hands the
bundle to every modal constructor in the app — the widest of everything this gate's entries
name, ahead of `center`'s eighteen and `overlays::context_menu_overlay`'s seventeen — and
`modal_backdrop_up` calls all three predicates and adds seven terms of its own, so what it needs
is their union: eleven bundles, a parameter list longer than the function. Neither is a
candidate, which is what a floor looks like when it is the function's own reach rather than a
callee holding it there.
**Every one of those field counts is a hand count that nothing computes, and three of them had
drifted** — `center` was written as sixteen against eighteen, `header` as six against nine and
`modal_layer` as eighteen against twenty-one, which put `modal_layer` and `center` in the wrong
order in two places. The gate itself only counts `ui: Ui` **parameters** per file, so it can
never catch a wrong reach count; re-measure before leaning on one, the way the lapsed
justifications under `trigger_editor.rs` and `schema_tree.rs` have to be re-read before being
trusted. `workspace` (eleven), `footer` (seven), `modal_backdrop_up` (eleven),
`context_menu_overlay` (seventeen), `palette_commands` (nine), `find_overlay` (eight) and
`error_modal_overlay` (five) were all still right at the same measurement.
**Then the whole tail came off in one pass — twelve files — and the lesson is about the ratchet
rather than about any of them: the tail was never hard.** `tabs.rs` (2), `ddl_preview.rs` (2),
`widgets.rs` (1), and `snippet_panel.rs`, `snippet_edit.rs`, `plan_view.rs`, `monitor_view.rs`,
`history_panel.rs`, `erd_view.rs`, `connection_form.rs`, `blob_view.rs` and `activity_panel.rs`
at 1 each all read two to five bundles into locals at the top and never said `ui` again — only
the signatures had not caught up, which is `connection_import.rs`'s tell above seen twelve more
times. They sat at 1 or 2 for the length of the campaign because **a small number reads as a
small remainder rather than as an easy one**, and the attention went to the files with the big
numbers. That is the standing hazard of a per-file budget: the total looks like progress while
saying nothing about which entries are cheap.
**Two lapsed floors made the sweep possible, and both are the kind this section already warns
about.** `widgets.rs`'s `MenuFlags::of` was justified in `BUDGET` as gathering a flag out of
"six child bundles" and so genuinely needing the root one. It is **five** — `overlay`, `schema`,
`conn`, `tabs_ui`, `activity` — and five named parameters is an ordinary signature, so it takes
`MenuFlags::of(OverlayUi, SchemaUi, ConnUi, TabsUi, ActivityUi)` and gathers its eight flags out
of those. The count *is* the whole of the argument for taking a root bundle, which makes
miscounting it the whole of the mistake: `trigger_editor.rs`'s lapse above was a justification
that named a callee and stopped being true, this one was a justification that named a number and
was never true, and the ratchet can check neither. Second, once `MenuFlags` is passed as a
**value** rather than computed inside each panel, the three side panels that reached `schema`
and `tabs_ui` *only* to build one stopped needing them at all: `activity_panel`, `snippet_panel`
and `history_panel` take `menus: MenuFlags` beside their own bundles — which is also what keeps
each of them under clippy's seven-argument limit — and `lib.rs`'s right-panel branch builds the
flags once per panel.
The rest were ordinary once said out loud: `tab_bar(TabsUi, ConnUi, OverlayUi,
RwSignal<Vec<DbColorRule>>, Rc<TabsActions>)` with `tab_chip(Tab, …)` carrying the same four
behind it, where the chip had been cloning a whole `Ui` **per tab** — the same per-row cost the
schema tree's object rows and `import_view`'s mapping rows shed;
`ddl_preview_overlay(DdlUi, ConnUi, DdlFn, Rc<dyn Fn()>, open_query)` with
`apply(DdlUi, ConnUi, DdlFn)` under it; `erd_overlay(OverlayUi, SchemaUi,
RwSignal<Vec<TableColorRule>>, Rc<TabsActions>)`; `monitor_overlay(OverlayUi, Rc<TabsActions>)`;
`blob_overlay(BlobUi, Rc<TabsActions>)`; `manage_modal(ConnUi, OverlayUi, Rc<ConnActions>)`;
`plan_overlay(OverlayUi, ConnUi, LayoutUi, Rc<TabsActions>, Rc<AiActions>)`; and
`snippet_edit_overlay(OverlayUi, SnippetsUi, Rc<SnippetActions>)`.
**`ai_panel.rs` stays at 1 and is the tail's one refusal.** Excluding the menu flags it now
takes
as a value, it still reaches eight bundles — `ai`, `ai_actions`, `conn`, `ddl`, `overlay`,
`schema`, `tab_actions`, `tabs_ui` — because the assistant reads the schema for context, opens a
query tab, and hands a plan to the DDL preview. That is `center`'s shape at panel scale: wide
because what it does is wide, and past clippy's limit if named.
**One gate fired during that sweep, and it is the only one in the crate that could have caught
this class.** The rustdoc `-D warnings` build failed on `blob_view.rs`'s ``/// Owned by [`Ui`]``
and on `ddl_preview.rs`'s `[`Ui`]` — intra-doc links that resolved only while the file imported
`Ui`, which narrowing the signatures took away. Both are ``[`crate::Ui`]`` now. **Narrowing a
module's imports can break a doc link with nothing else noticing**: `cargo test`, `clippy` and
`fmt` were all green at that moment, and rustdoc is the gate no local habit runs.
**The live number is the sum of `BUDGET`, not the "roughly 140" above**, which describes the
state the gate found and by design never moves. That sum went **206 → 14** over this run:
`table_designer.rs` 29 → 26 → 10 → 4 → **0**, `object_editor.rs` 14 → 5 → 4 → **0**,
`routine_editor.rs` 12 → 6 → **0**, `event_editor.rs` 11 → 5 → **0**,
`trigger_editor.rs` 11 → 8 → 7 → **0**, `view_editor.rs` 10 → 6 → **0**,
`database_editor.rs` 7 → 3 → **0**,
`ddl_preview.rs` 6 → 4 → 2 → **0**, `overlays.rs` 15 → 13 → 4, `account_editor.rs` 6 → **0**,
`schema_tree.rs` 6 → 3 → 2, `users_view.rs` 9 → 8 → 5 → **0**, `import_view.rs` 9 → **0**,
`dump_view.rs` 9 → **0**, `compare_view.rs` 8 → **0**, `script_view.rs` 6 → **0**,
`connection_import.rs` 6 → **0**, `properties.rs` 5 → **0**, `settings.rs` 4 → **0**,
`modals.rs` 5 → 2, `lib.rs` 6 → 5, and the tail's rest — `tabs.rs` 2 → **0**, `widgets.rs`
1 → **0** and nine panels and modals 1 → **0** — leaving `ai_panel.rs` alone at 1.
Every step is recorded against its own entry with what it narrowed *to*,
so read the list for where a file stands rather than inferring it from a paragraph — and a file
that reaches zero leaves the list altogether, which is the one way an entry is ever removed and
the point at which it may not take a `Ui` again at all. **Twenty-eight files have left it** —
the three DDL-object editors, then `trigger_editor.rs`, `account_editor.rs`, `users_view.rs`,
`import_view.rs`, `dump_view.rs`, `compare_view.rs`, `script_view.rs`, `view_editor.rs`,
`database_editor.rs`, `table_designer.rs`, `connection_import.rs`, `properties.rs` and
`settings.rs`, and then the tail's twelve above — and each left a comment
behind where it sat, because a name absent from `BUDGET` says nothing on its own about whether
it was ever on it. The twelve share **one** block, filed where the first of them alphabetically
(`activity_panel.rs`) sat, because what there is to say about them is the sweep and not any one
file.
It is a **budget, not a ban**, because narrowing 140 signatures is a campaign and a gate that
fails on the day it lands teaches nothing: `BUDGET` holds what each file declares *now* and the
only legal direction is down, so a new `ui: Ui` in a listed file fails and a file not on the list
may not take one at all. The second test is the floor —
`no_budget_entry_is_looser_than_the_file_it_governs` asserts each entry is *exactly* the count,
since a number left high after a narrowing is a licence the next `ui: Ui` would inherit, which is
the same floor `dividers::scaled_arg_gate` and `theme::color_arg_gate` keep on their exemptions.
App-crate sources come through `source_gate::crate_sources` too and are skipped by path here: the
bundle is this crate's, and `app_view` builds it rather than taking it.
**The counter counts *parameters*; it used to count line shapes.** It matched a fixed set of
literal spellings — own-line forms with a trailing comma, inline forms anchored on the opening
paren — so `fn thing(x: Foo, ui: &Ui)` was invisible: `ui` had to be **first**, and a single-line
signature has no trailing comma on its last parameter. Three sites were exactly that
(`compare_view.rs`'s `body_for` and `ready_body`, `tabs.rs`'s `tab_chip`), and the floor test
asserts `found == allowed` through this same counter, so neither test could see them while the
ratchet's own rule — *a file not on the list may not take one at all* — was walked past by any
new one-line signature in any file. `compare_view.rs` went 6 → 8 and `tabs.rs` 1 → 2, which was
what those files declared *then* rather than a widening — `body_for` and `ready_body` both take
`OverlayUi` now and `compare_view.rs` is off the list altogether, as `tabs.rs` now is, so read
that pair as the counter's history rather than as live signatures — and `widgets.rs` joined at
1 for `MenuFlags::of`, invisible until the counter learned to read `&crate::Ui`. Read that as
history too, and in both directions: the entry it joined at said the function gathered its flags
out of "six child bundles" and so genuinely needed the root one, the tail sweep above found it
was five, and `widgets.rs` is off the list.
**The footer's left segments collapse right-to-left, and coming *back* is a different question
from going away.** `collapsing_seg` hides a status segment once its right edge comes within
`footer_collapse_gap()` of the right-hand icon group, and a hidden one freezes its measured edge,
so all it can do is *predict* whether it would fit again. Freezing alone meant it could only
return when the **window** widened, which is why `footer_seg_edge` also reads a shared `left_edge`
— a neighbour getting shorter is the ordinary way room appears: commit a transaction and the Tx
pill, Commit and Rollback all go at once, freeing ~150px; switch to a SQLite connection and
`mode_seg` disappears outright; "Write mode" becomes "Read-only". **That prediction had no
ordering term, and the order is the premise everything else rests on.** Segments hide
right-to-left, so the hidden set is a *suffix* — which is what `footer_seg_edge` means by "the
right edge of the nearest **shown** segment to this one's left" — yet every hidden segment read
the same `left_edge`, so they came back in order of **width** instead of position. Narrow the
window until the Tx pill, Commit, Rollback, CPU and RAM have all gone, then shorten anything to
their left and `left_edge` drops ~60px: Commit predicts `L+57` and fits, the Tx pill predicts
`L+130` and does not, and the footer settles at `… Read-only Commit` — a lone commit action with
no pill saying a transaction is open and no Rollback beside it. It is *stable* rather than a
flicker, because once Commit is drawn the pill re-predicts `L+187` and still does not fit.
`footer_seg_may_return(measured, shown, left_shown)` is the missing term: `collapsing_seg` takes
its left neighbour's shown-flag and publishes its own, so a segment may return only while
everything to its left already has, and the hidden set stays a suffix.
**`measured` is a signal of its own, and it replaced `width > 0.0`, which conflated two states.**
A zero width means either that floem has never laid the segment out — it returns before the
resize and move listeners for a hidden view, so one hidden from the start never is — or that it
was measured and is **empty**, which the Tx pill, Commit and Rollback are the moment a
transaction commits and `mode_seg` is on a SQLite connection. Only the first has no edge to
predict; the second *is* an edge, at `x + 0`. Conflating them froze the second out for the
session: it kept the stale, non-fitting edge it held while it had width, `width > 0.0` then
locked its `shown` flag false, and because the flags chain left-to-right every segment to its
right was blocked behind it — commit a transaction, narrow the window until CPU and RAM go, widen
it again, and they never came back. Both geometry handlers share one body,
`footer_seg_settled_edge(measured, shown, x, w, prev)`, so `on_move` and `on_resize` cannot
answer differently about which of the two `prev`-keeping cases they are in.
`the_prediction_agrees_with_the_measurement_it_causes` pins the anti-oscillation property for
**one** segment in isolation, which is the limit worth knowing about it: with two or more hidden
segments sharing a `left_edge`, the prediction agrees with the measurement only for whichever
returns leftmost.
**The terminal's geometry is three pure functions, and two of them take the padding as a
parameter because it is `theme::scaled(6.0)` and not 6.** `term_cell_wh(font)` is the IBM Plex
Mono cell metrics; `term_fit(w, h, pad, cw, ch)` is how many columns and rows a surface holds;
`term_cell_at(x, y, pad, …)` is which viewport cell a surface-local point falls on. The last two
**restated** the inset as literals — the PTY fit subtracted `12.0`, `cell_at` `6.0` — which is
exact at 100% and wrong at every other scale, which is why they shipped. At 200% the surface
takes 24 logical px and the fit took off 12, so the PTY was told it had about one and a half
columns and two thirds of a row the panel cannot draw: the shell wraps at a width the user cannot
see and the bottom row can be clipped. `cell_at` mapped a click against a 6px inset where the
real one is 12, so the reported column drifted further the further along a row the pointer went —
by a whole cell across the right-hand part of every line, and with copy-on-select enabled that is
the wrong text on the clipboard. The cursor overlay a few dozen lines below already did it
correctly and states the rule ("the padding is scaled and this overlay is the surface's
*sibling*, so it has to read the same metric rather than restate it"), as does the scrollbar;
these were the two that restated it. Taking `pad` as a parameter is what makes the arithmetic
testable at a scale a test can name — `term_geometry_tests::at_one_hundred_percent_nothing_moves`
pins that nothing changed where it was already right, and the 200% cases assert both halves.
**`term_cell_wh` is deliberately *not* scaled**: the terminal font is the user's own size fed
raw to `.font_size` rather than through `scaled_font`, so the cell metrics are already in the
right units and putting them through the scale would double-apply it.
One test had to be rewritten and the reason is written down at it: its first shape put the probe
exactly on a cell **edge**, and `2.0 * cw` divided back by `cw` is a coin toss in binary floating
point — it reported column 1 for the boundary of column 2. Probes sit mid-cell now.
Two things about `edit_field`'s **multiline** boxes, both found in a body field and both fixed
in the shared helper rather than at one call site. **Enter is only swallowed when there is
something to submit to**: a multiline field with no `on_submit` lets it through and breaks the
line, which is what Enter means in a box of SQL — consuming it unconditionally left every body
field (the snippet editor's, the view editor's) with a dead Enter and a Shift+Enter nobody would
guess at. The corollary is a trap for callers: **the no-submit arm is only safe for a field that
is also read-only**, or Enter silently types a newline where the user meant an action. Separately
from that, `multiline` was answering two questions at once — *wrap and auto-grow*, and *Enter may
break the line* — which are the same answer for a snippet or view body and opposite answers for a
box holding one question, which wants to grow with a long question and has nothing a second line
could mean. `FieldCfg::enter_never_breaks` separates them: the Enter branch gates on
`breaks = multiline && !enter_never_breaks` in *both* arms, and that is the **only** place the
flag is folded in — `plain` stays `!shift && !control`, because once a field cannot break its
line no modifier combination changes what Enter means, so the guards stop consulting `plain` at
all. Wrapping and auto-grow are untouched. Ctrl+K's
prompt sets it, and its `on_submit` is **never `None`** (while a request is in flight the closure
is a no-op), so the key is swallowed on its own terms rather than by depending on read-only to
refuse the edit afterwards.
**All of which is right, and none of it was the reported bug.** The "stray newline" in the Ctrl+K
question was never a newline: the caret could not reach a second line, and it happened when
submitting with the send triangle too, so the Enter key was not involved at all. The bar was
*taller* — its `dyn_container` was keyed on the request state, so every transition **rebuilt the
field**, and a freshly built `edit_field` has almost no width for one frame. A one-line question
wraps in that frame, the row count is measured from that layout and latched, and the bar reopens
a row taller with its text stranded at the top, which is indistinguishable from a typed newline
in a screenshot. Three rounds of fixes went at the Enter key on that resemblance alone. The
lesson is the misdiagnosis: **a taller box and an extra line look identical, and only the caret
tells them apart** — ask where the caret can go before touching the key that would have put it
there. The fix is in `editor_pane` (the container is keyed on `Ready`-or-not now); what is kept
here was worth keeping on its own terms and has no regression test either way.
**That was only half of it**, and the other half outlived the rebuild by a long way: the bar went
on opening two rows tall about once in seven times, with no rebuild left to blame. The
re-measure effect tracked `viewport` alone — but floem answers a width change in an effect of its
own *on that same signal*, `lines.set_wrap(Width(viewport.width()))`, and that only **clears** the
line layouts; they are rebuilt lazily afterwards. So the count read on the `viewport` edge was
computed from the layout of the **previous** width, and `Lines::last_vline` caches its answer —
which is what made a bad frame permanent rather than momentary, since nothing but a keystroke
would ask again. The effect now also tracks `screen_lines`, which floem `update`s once those
layouts exist again (`update_screen_lines` walks the visual lines, and walking them is what
builds them), so the measurement happens at the first moment the count is true. The general
lesson is the one the misdiagnosis above teaches twice: **a value read on the edge that
*announces* a change can be older than the change**, and a cached reader turns that into a
permanent wrong answer instead of a frame of one.
And **the field repaints when its row count changes**: the box height is derived from
`rows`, so the edit that adds a line has already painted against the *old* height, leaving the
caret on the new last line outside the viewport that was drawn — it looks like the caret has
vanished until the next keystroke repaints against the grown box. The repaint is deferred a tick
so it runs against settled layout.
(`FieldCfg::min_rows` is the floor such a box starts at — 1 by default, 3 for a snippet body,
clamped against `max_rows` at use because `clamp` panics when the floor exceeds the cap.)
**`FieldCfg::masked` is where a field whose buffer stands in for a value it does not hold
reports each edit**, as the editor's own copy/insert runs over the buffer that was there before
it — `connection_form::MaskEdit`, and `connection_form::masked_edit_field` is the app's one
consumer. It is not a convenience: a masked buffer is mask characters throughout, so *comparing*
it before and after cannot tell a mask character the user typed from one that was already there,
and the field that tried stored three characters of the previous secret when a `*`-containing
password replaced it. The delta is the editor saying what it did, which is the only unambiguous
answer, and it covers every path into the buffer — keys, clipboard, IME commit — not only the
ones a key handler sees. Two consequences a caller has to know. Setting it **takes undo and redo
away from the field**: the document's history is a history of *masks*, so replaying one puts
mask characters into the value, and Ctrl+Z/Ctrl+Y are swallowed rather than left unbound so
nothing else claims them (which is why `shortcuts`' `EXEMPT` carries `lib.rs`'s `z` and `y`).
And the report is filtered by a plain `syncing: Cell<bool>` set around the signal→doc reconcile,
because the field's own re-mask arrives through the same `update` hook and reporting it back
would loop the pair of effects that produced it — a flag suffices because `on_update` runs
synchronously inside `edit_single`.
**`FieldCfg::suggest` is the inline ghost a free-text field with a menu wears.** While the field
is focused, the caret sits at the end with nothing selected and the field is neither read-only
nor frozen, the first listed value the typed text begins (`text::field_suggestion`) paints its
tail in the placeholder colour, and Tab takes it. One closure, `ghost_of`, is asked by both the
painter and the Tab handler, so the key can only take what is on screen. The tail is an absolute
overlay in the placeholder's stack, moved right by the typed text's width
(`widgets::measure_field_text_px`, in the field's own face) and back by the editor viewport's
`x0`, so a value longer than the box still has its tail at the caret rather than at the
unscrolled end. The list is read **untracked**, on every keystroke: the database editor's Owner
roles arrive from a fetch after the form is built and the next keystroke sees them, while a
tracked read would subscribe the field to whatever signals the closure touches. Accept sets the
field to the option **in the list's spelling**, not the typed head plus the tail — a collation
or a role is the server's spelling — and then puts the caret at the end explicitly, keeping
focus. **That caret move is load-bearing**: the signal→doc reconcile maps the old caret through
the common prefix, which leaves it mid-text whenever the typed head already matched the
option's case. Every caller reaches it through `table_designer::suggest_field`; Tab's precedence
is under *Floem 0.2 gotchas*.
**`FieldCfg::sql` colours a field holding SQL in the interface theme's SQL palette** —
`sql_highlight::FieldSql`, built over the field's own `ed.doc()` and driven from
`FieldStyling::apply_attr_styles`: the editor's lexer, `UiTheme::sql_*`'s colours (under
`sql_highlight.rs`). **It changes nothing else about the field** — the background, text and
placeholder stay the field's own (`bg_deepest`, `theme::text` and `theme::placeholder` by
default), so a SQL field matches the fields around it, and a caller's `background` is honoured.
It used to override all three with the editor's surface, base and comment colour
(`theme::preview_bg`/`preview_fg`/`sql_placeholder`, all three now deleted) to keep the editor
palette on the background it was tuned for, which made every SQL field a dark block in a light
form whenever the two themes disagreed. `FieldStyling::id` keys on `theme::ui_generation()` with
the size, so a UI-theme switch re-lays the lines out rather than leaving them in the old palette,
the hazard `SqlStyling::id` documents; an editor-theme switch leaves a field alone. The callers
are the connection form's Init SQL, the DDL preview's SQL box, the view editor's `SELECT`, the
trigger and routine bodies, and the snippet editor's body — a snippet carries a scope rather
than a dialect, so `snippet::body_dialect` picks one for it.
**`FieldCfg::button` is a labelled button inside a single-line field**, right-aligned — Import
Connections' *Add* and `connection_form::path_field`'s *Browse…* (GitHub #19), each built with
`widgets::field_button`. Beside the field, as a `control_button`, it stood shorter than the box
it serves; inside, its height is the field's. That is the difference from `FieldCfg::trailing`,
whose fixed 16px box holds the AI panel's send icon: this one is in flow beside the editor but
stretched to the row (`align_self(Stretch)`), and negative margins reach back through the box's
padding — `-(pad_v - inset)` top and bottom, `-(chat_pad_h - inset)` on the right — so it stands
`field_button_inset()` (`scaled(3)`) off the border on all three sides, with `trailing`'s left
gap (`field_button_gap_l()`) between it and the text. **Its width is its label's, so it is
measured**: `on_resize` writes `button_w`, and the placeholder and the `suggest` ghost stop short
of it through `field_button_right_inset(button_w)` in place of `placeholder_right_inset` — they
are absolute overlays, and nothing else keeps a long placeholder from painting under the button
(`field_layout_tests::an_inset_button_is_never_painted_over_however_wide`). The padding is not
in that sum because the button stands in it. A `debug_assert` refuses the slot alongside
`trailing` or `clearable`, which take the same corner, and on a `multiline` field. The box's
click-to-focus `PointerDown` leaves a press right of the editor's viewport alone when a button
is there: that corner is the button's, and focusing the text under it would take the keyboard
from the button being pressed.
**Four options exist for the grid's inline cell editor**, the last field in the app to move off
floem's `text_input` (*Data grid* has why). `FieldCfg::bare` draws in place inside a box that
already exists — no border, padding, background or rounding, and exactly one line tall for the parent
to centre, not `height_full`, since the editor fills its box and a full-height one would set the
line at the top — and with no scrollbar even when `multiline`, which the grid always sets to keep
a value's line breaks. `FieldCfg::inset_left` is the box's left padding read reactively, for a numeric
cell's right-alignment; padding the editor itself is a no-op, it being a scroll view. Both feed
one `pad_left` closure, which the placeholder, the `suggest` ghost and the click-in-chrome
mapping read too, so nothing laid over the text drifts from it. `FieldCfg::on_shift_tab` is
Shift+Tab's own override (`tab_action`, under *Floem 0.2 gotchas*). `FieldCfg::on_focus` is
`on_blur`'s mirror, **not deferred** — a gain is never half of a click's lost/gained pair the way
a loss is — and, like `on_blur`, skips the focus effect's initial run, which only establishes
tracking; it is how the calendar cell drops a stale panel press, the focus landing on the inner
editor where no listener on a wrapping view would hear it.
The shared types living in the crate root is what stalls further splitting: the
root depends on the leaves (`mod`) and the leaves depend on the root (types), so a view builder
can't move out until the types do.
**`modals.rs` is the first cut that did move**, and it is the shape the rest should follow: not
the biggest view builder, but the one piece of `workspace` with an *invariant* attached. It took
the modal layer, its four predicates and `modal_backdrop_gate` with it, and `workspace` kept a
single tuple entry plus the one `modal_backdrop_up` call the title-bar band also reads. Nothing
about the types had to move, because the layer needs only `Ui` — which is the test for whether a
piece is ready to leave. **`dividers.rs` is the second**, and it is the other kind: no invariant,
no test, purely a self-contained pair of views (~240 lines) that `body` and `center` call. Both
cuts left the shared types where they are, so the stall above is unchanged — the next cut still
has to answer it.
**A first launch saves nothing and selects nothing.** `app_view` used to seed a "Local
MariaDB" connection (127.0.0.1:3306, this repo's development credentials) whenever the loaded
list came back empty, and write it to disk on the spot — so a fresh install opened onto a
connection its user never made and probably couldn't reach, and deleting `connections.json`
only made the next launch write it back. It is gone; the empty list stands, and the header's
button is what that state offers. `Connection::startup_active_id(saved, &connections,
high_water)` picks the active id — the saved one while it still names a connection, else the
first, else `next_id_after(&[], high_water)`, **the id the first connection created this
session will take**, because `save_conn` loads the schema for a connection only when it is the
active one and a new user's first connection should connect on save rather than wait to be
selected. New now persists that connection on the press, blank and unconnected, so it is still
the first Save — the one that fills in a real server — that connects it. **The coupling is to
the minting function, high-water mark and all**, and that is what the test compares against:
the arm answered `next_id(&[])` = 1 after minting moved to `next_id_after`, so once any
connection had existed, deleting the last one and pressing New → Save minted 4 while the active
id stayed 1, and the header read *No connection* — the delete path and startup now both pass
the high-water mark (`an_empty_list_points_at_the_id_the_first_connection_will_take`).
`import_chosen` is the second creator and asks the same question: when the active id names
nothing (`was_dangling`) the first imported connection takes it, so the import loads its schema
and calls `reset_activity` itself. Persisting New on the press broke that check — the header's
first-run button lists a blank *New connection* under that very id, so the id no longer
dangles, and a new user's imported servers never connected. So an import while the active
connection is still a blank nobody filled in (`on_unsaved_new`, asked through
`Connection::is_untouched_new`) calls `switch_conn` on the first id it added — a switch rather
than the dangling path's two calls, because here the active id *moves* and the tab strip, the
expansion set and the persisted active id have to follow it. The blank stays listed, as the
user's to delete. **`is_untouched_new` asks the connection, not a memory of making it**: it is
`Connection::new_blank` under its own id, name and colour, named *New connection* or *New
connection N* — `DraftSignals::blank` loads that same value, so the two cannot drift. The app
used to keep the id in a session-only `unsaved_new`, which a restart lost while the blank stayed
on disk (and a second New overwrote), so the app then connected to the blank at startup,
exported it, and kept it active through a first-run import. Startup's schema load and
`check_conn` (the health poll's ping) now pass over a blank too.
**`Connection::rebind_tab(saved, &known, fallback)` is the same question for a restored *tab*,
and an empty list is deliberately not the same answer.** The saved id wins while it names a
connection; when it does not, the tab's connection was deleted and the tab moves to the
fallback, which is what stops a session pointing at a connection nobody can select. But
"deleted" and "the connection file did not load" both present as *the id is not in this list*
and want opposite things — on a load failure the saved id is right and the list is wrong.
Collapsing them rebound twelve tabs across three connections onto one id (that same
forward-looking empty-list answer, which a *future* connection will occupy), and the debounced
session save wrote it to disk before the user touched anything, so restoring `connections.json`
from its `.bak` afterwards did not undo it. A tab naming a connection that does not exist is a
state the app already handles — `db_for` answers "connection no longer exists" — while a tab
silently re-pointed at someone else's server is not.
The header's **connection trigger** is one slot with two occupants, chosen by
`connections.is_empty()`: the switcher normally, and on a first run with nothing saved a
`New connection` button in the accent that creates a connection (`new_conn`, under
`connection_form.rs`) and opens Manage Connections in one press. With nothing saved there is nothing to switch *between* — the switcher reads "No
connection" and its menu puts the only action a first run has three clicks down (open the menu,
Manage Connections, New connection), which is a funnel with no traffic at the top. The test is
"none saved", not "none active": a user with connections and no active one still wants the
switcher, since choosing is what it is for. Both wear `switcher_chrome`, shared rather than
spelled twice **because they swap in and out of the same slot** — a margin or radius that
differed would show as the header twitching the moment the first connection exists — which,
New persisting at once, is the very press on the button. The
caller sets the border colour (the active connection's identity colour, or the accent) and may
re-state the horizontal padding: the button does (9px/10px against 11px/7px), because its
content is the switcher's mirrored — glyph leading, label trailing — so the plus carries its own
sidebearing into the left inset while a label ending flush needs more room after it. The pill's
outer edges still land where the switcher's do, which is the part that keeps the header still.
The switcher is the *only* thing that sets `conn_menu_open`, so the button leaves no menu
behind it that nothing can raise.
The **environment badge** beside the switcher still asks core's `Environment::badge_label`
*whether* to show — `None` for no environment — but its word is `i18n::environment_label`'s
(`env-*`), not the `&'static str` that answer carries, which is core's English.
The header's **update chip** (`update_state` + `apply_update` on `Ui`, alongside `resources`) is
built in `header()` and goes in first in the right-hand cluster —
`h_stack((update_chip, search, help, settings, chrome.controls()))` — because a muted text
segment in the status bar was not prominent enough for an offer meant to be acted on. It is
shaped like the connection switcher at the other end of the header: `border(1.0)`,
`border_radius(5.0)` and an opaque `theme::bg_chrome()` fill, the fill for the reason the
switcher documents — an outline over a transparent interior anti-aliases on both edges and looks
blurry. Then `padding_horiz(9.0)`/`padding_vert(3.0)`, `margin_right(26.0)` — 10px more than the
16px the glyphs keep between themselves, so the chip reads as its own thing rather than a fourth
member of the search/help/settings run — and `gap(6.0)` between a **13px** `icons::REFRESH_CW`
and an **upper-cased label at 11px**. The caption is upper-cased because that is what squares the
chip up: in mixed case the lone capital `R` of "Restart to update" stood against a run of
x-height letters, so the glyph block was taller on its left than its right and no *symmetric*
padding could centre an asymmetric shape — the text read as sitting high whatever the numbers
were. All caps is one uniform band, which centres against equal padding. The glyph is sized
against that band rather than against its neighbours: four strokes in a circle read heavier than
the switcher's single-stroke chevron, so matching the chevron's 16px would leave it shouting over
an 11px caption. The design also called for 0.06em tracking, which **Floem 0.2 cannot express** —
neither its `Style` nor the cosmic-text `Attrs` beneath it has a letter-spacing property, and
padding the string to fake it would wreck the metrics the upper-casing exists to fix. It is
tinted like the glyphs beside it — `theme::text_muted()` resting, `theme::text()` on
hover — and **the colour is set on the container**, so the label and the `currentColor` SVG both
inherit one value; `border_color` brightens with them, so the chip reads as one object rather
than a box with a lit label inside it.
It renders a zero-footprint `empty()` whenever `UpdateState::label()` is `None`, which is
most of most sessions, and is clickable only while `is_actionable()` holds — "Updating… 40%" is a
progress readout, and a click on it mid-download would have nothing to apply.
