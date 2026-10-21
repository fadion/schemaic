# `core/edit.rs`

`edit.rs` — `analyze_edit` → `EditModel` (write-back updatability analysis) + `refetch_template`
and `refetch_key`, the **one** post-edit re-fetch key builder. A key column *is* editable
(`EditModel::editable` asks only whether a column maps to a base table), so the `UPDATE` keys on
the original row while the re-fetch must look for the value it just wrote; there were two
builders and only one knew that.
**`insert_target` counts the result's *origin* tables, not the keyed ones**, and the difference
was a row destroyed with nothing on screen naming the table. It read
`match self.tables.as_slice() { [only] => Some(only), _ => None }`, and `tables` holds only the
base tables that *got* a key — one `resolve_key` refuses is absent rather than counted. So on
`SELECT o.id, o.total, l.note FROM orders o JOIN audit_log l ON …`, where `audit_log` has no
primary key and no fully-present unique NOT NULL index (a log table — or **any view**, whose
columns carry no PK flag, so the no-schema fallback refuses it too), it answered `Some(orders)`.
That answer gates the gutter menu's **Delete row** and **Duplicate row** and the row-action
strip, so Delete was offered on a row of the join and Commit ran
`DELETE FROM db.orders WHERE id <=> 7` with `l.note` beside it; on a 1:many join one selected
display row destroys the parent the other display rows also stand for. **The write-back's 1-row
safety net structurally cannot catch this** — the statement affects exactly one row, so
`one_row_verdict` passes and the report is correct about what it did, which leaves the gate as
the only line. Both docs asserted the property they didn't have: this one said `None` "for a
multi-table join", and `gutter_menu`'s comment said the actions are offered on rows "of a single
writable table". `EditModel::origin_tables` is now taken off `analyze_edit`'s own `groups` —
every distinct base table the columns came from — and `insert_target` requires it to be 1. The
looseness `tables.len() == 1` was standing in for is kept, because an expression column has
`origin: None` and so is not a second table: one base table plus computed columns still answers.
`build_inserts` and the grid's `filterable` read the same predicate, and `refetch_template`
already verified every column's origin itself.
`a_join_with_one_keyless_table_has_no_insert_target` is the fixture that was missing — the only
join fixture this family had carried two *writable* tables, which is the half that already
worked — and `a_join_with_one_unkeyed_side_still_offers_no_insert_target` is the same shape
against real servers, where what each side of a join reports as its provenance is the driver's
answer rather than a test's.
**The one key column that is never editable is an implicit key** (`ColumnOrigin::implicit_key`).
`resolve_key` falls back to it only after a primary key and a fully-present unique NOT NULL
index have *both* failed — a real key is what the user means by the row's identity, and it is
what survives a re-fetch, where a rowid the engine may reassign is not — and `analyze_edit` then
leaves that column out of `col_table` altogether: it is the handle the write holds the row by,
not data the table has, and a newly inserted row has no value to offer for it. That exclusion is
also what keeps it out of a staged `RowInsert`, with no second rule to remember. Tested by
`keyless_table_is_editable_through_its_implicit_key`,
`a_real_key_still_wins_over_a_projected_implicit_one` and
`refetch_template_keys_on_the_implicit_key` — the last because a read-only key column is still
part of the row the splice re-reads.
**A generated column is left out of `col_table` the same way**, read off the loaded
`TableInfo`'s `ColumnInfo::generated`: every engine refuses a value for one (SQL Server's
Msg 271 on a computed column), so its cell took an edit only the commit refused, and *Duplicate
row* — `cloned_row` copies every `text_editable` column — failed on every table that had one,
AdventureWorks' `Sales.SalesOrderHeader` among them. It stays a key or confirming column if it
is one; only writing to it is off (`a_generated_column_is_read_only_and_left_out_of_a_clone`).
**And an implicit key nothing can confirm is a refusal, not a bare `rowid = ?`.**
`confirm_columns` answers `Option<Vec<usize>>` — `Some(vec![])` for every real key on every
engine, `None` when the key is implicit and the candidate set comes out *empty* — and
`analyze_edit` treats `None` exactly as it treats an unresolvable key: the table is never pushed,
so the result is read-only. `EditTable::confirm_cols` states the premise the 1-row safety net
rests on — a stale key matches zero rows — and with nothing to compare, the `WHERE` is the rowid
alone and that premise is false: a rowid reassigned by a `VACUUM`, a delete-then-insert or the
twelve-step rebuild lands the `UPDATE` on a *different* row, affects exactly 1, and is reported
as a success. `blobs(data BLOB)` — a rowid table whose every non-key column holds bytes — is
precisely that shape, so this **takes the blob write back off that one table shape**, which is
what it was before the panel could write at all.
**`EditModel::table_for_origin` is the lookup that does not go through `col_table`**, and it
exists because a *read* can be aimed at a column a write may not touch. A binary column **was**
the case it was written for: it was excluded from the map outright, so `table_index` answered
`None` for it while its base table sat in `tables` with a perfectly good resolved key, and a
fetch written against the write model's own lookup would simply never have offered itself.
Since C2 narrowed to text (below) that column has a `col_table` entry like any other, and
`blob::blob_source` still asks through here — an implicit key is excluded on the same terms, and
this is the lookup that cannot go stale on the question a second time. It matches the whole
`(database, schema, table)` triple, the
same requirement `analyze_edit`'s grouping states: `sales.orders` and `archive.orders` are
different tables and one must never answer for the other.
**C2 is two rules now, and only one of them still refuses the column.** It used to be one
sentence — a binary column can't round-trip as text — enforced in two places: `analyze_edit`
left the column out of `col_table`, and `resolve_key` refused it as a WHERE key. The first was
one claim doing two jobs. It is exactly true of *text* and was never true of bytes, and
excluding the column made the cell unwritable by any means at all, which is what kept the blob
panel a viewer. So a binary column is `editable` like any other now, and
`EditModel::text_editable(ci)` — `editable(ci) && !binary(ci)` — is the narrower refusal that
replaced it: the grid asks it wherever text would land in the cell (`plan_paste`'s and
`fill_plan`'s predicates,
`start_edit`, the Tab/Enter hop `next_editable_col`, the row panel's `ColSpec::editable`, AI
Fill Value and the AI seed, `add_cloned_rows`, the *Edit field*, *Paste* and *Fill* cell-menu
entries,
and — through `activation`, below — the Enter key and the double-click). *Set to NULL*
deliberately still asks `editable`, because
emptying a blob writes no text, and *Set to DEFAULT* asks it for the same reason. `resolve_key`'s half is unchanged — a binary column is still no
WHERE key, where the lossiness really is fatal and there is no byte-shaped way round it.
**`activation(ci)` is where the two *open what I am on* gestures meet, and it exists because
they had drifted apart.** A double-click has always fallen through to the binary panel on a cell
that takes no text, but the Enter arm asked `text_editable` and stopped there — so on a
`photo BLOB` Enter did nothing, and since there is no context-menu key binding either, the
grid's one byte-write surface was reachable by pointer alone. `CellActivation`
(`TextEdit` / `OpenPanel` / `Nothing`) is the single answer both gestures read now
(`enter_and_a_double_click_aim_at_the_same_surface`). `OpenPanel` means *aim the panel at this
cell*, not *the panel will open*: the row-level refusals — a NULL cell, a pending row, a row
marked for deletion — stay in `grid::blob_launch`, which is what keeps this answer free of the
state only the caller holds. A binary column answers `OpenPanel` **whether or not it can be
written**, because the panel is the viewer too.
**`binary(ci)` is a recorded `col_binary` vector, and it asks two signals where editing used to
ask one**: `holds_bytes` is `Column::is_binary` **or** `ResultSet::binary_columns`. SQLite types
values rather than columns, so a blob living in a column declared `TEXT` has neither a wire flag
nor a type-name signal, and only the backend's own `ValueRef::Blob` assertion — the second
signal the export has always asked — can answer for it. `resolve_key` was moved onto the same
function, because C2's two halves have to agree on the word: a column one of them calls binary
and the other does not is either a lossy `WHERE` or a writable placeholder. **It is `pub(crate)`
and it is now the one definition of the word**, because two more readers were spelling the same
pair by hand — `blob::blob_source` and `export::dropped_binary_columns` — while
`confirm_columns` asked the wire flag alone and so disagreed with all three: a blob stored in a
SQLite `TEXT`-declared column is legal, reads back as `<n bytes>`, and entered the confirming
`WHERE` as that literal placeholder text, so **every** edit to such a table matched zero rows and
rolled back — and since `blob_source` builds its `BlobRef` key from the same `row_key`, the read
found nothing either. It is recorded on
the model rather than re-derived at each call because "editable" and "editable *as text*" are
two different questions once a blob can be written, and the callers still asking the first are
the two that write no text: `blob_launch`, the surface that puts bytes in a cell (through
`takes_bytes`, below), and *Set to NULL* and *Set to DEFAULT*.
**`takes_bytes(ci, row_deleted)` — `editable(ci) && !row_deleted` — is the blob write's gate,
and it is one function because it is asked twice.** The grid offers *Load from file* through it
and asks it again when the bytes come back from the file dialog
(`GridState::stage_bytes`/`stage_new_bytes`); the two are separated by however long the user
spends choosing a file, and they were two hand-written spellings of one rule with no test over
either. The `del_rows` half is the load-bearing one: staging into a row already marked for
deletion builds a commit carrying a `RowDelete` **and** a `RowEdit` for one row, the update then
matches nothing, and the 1-row safety net rolls the whole batch back over an edit the user could
not see. A pending row passes `false` — it has no committed row to mark, and that its skeleton
still exists is a different check, belonging where the rows are.
**`takes_generated_text(ci, row_deleted)` is its sibling for AI Fill Value** — `text_editable(ci)
&& !row_deleted`, the text half where `takes_bytes` asks the bytes one — and it exists because
that failure had a fourth door. `start_edit` refuses a marked row, `stage_bytes` refuses one and
a paste counts such a cell as `skipped_deleted`; AI Fill went through `stage_set`, four lines
with no gate, while the sparkle menu's own `fill_enabled` was `text_editable` alone. So filling a
cell on a red-washed row staged an edit the commit could not reconcile with the delete, and the
net rolled the **whole batch** back — every other marked row and every other staged edit with
it — over a green cell on a row the user had already decided was going away. Both the offer and
the sink ask this one function, and it is down here for the reason `takes_bytes` is: a test
written against `stage_set` cannot fail, that being a private method on a struct no test can
construct. A pending row passes `false` here too.
**`byte_cap(ci)` is a third recorded vector (`col_cap`), filled where the schema is already in
hand.** `analyze_edit` resolves each table group's schema **once** and both readers take that one
answer: it records `blob::column_byte_cap` of each binary column's declared type, and hands the
same `Option<&TableInfo>` to `resolve_key`, which no longer takes the closure at all
(`resolve_key(info, cis, rs)`). The UI's `schema_for` hands back a *cloned* `TableInfo` — every
column, index, check and foreign key — and `analyze_edit` runs in an effect on each result load
and each schema refresh, so asking twice doubled that clone per group. The type is read from the
*schema* and not from `rs`, because a result column's type name has had its length stripped and
its family flattened (see `column_byte_cap` for why only the schema can answer). Only binary columns are
asked, since only they have a surface that can overrun a limit, and a table with no loaded
schema simply has no caps: `None` is "no answer" here too. **That vector is why `analyze_edit`
takes a `SqlDialect` as its second parameter**, and it is the only field the dialect reaches: a
declared byte length is a promise on one engine and a note on another, so a type name cannot be
read without knowing whose it is (`blob::column_byte_cap`, below). Two call sites pass it — the
grid's model effect, off `GridState::dialect`, and the Live Monitor's baseline poll, off the new
`MonitorCtx::dialect`, carried on the context rather than re-resolved because `monitor_apply`
runs on the UI thread with no `Db` in hand and the key it resolves must not depend on connecting
again. The unit tests go through a local shim that
fixes MySQL, so a test that means a *cap* has to name its engine and call `super::analyze_edit`
outright rather than inherit one by default — a convention the module's own doc stated and no
test followed, so the dialect argument was unexercised end to end while `blob::column_byte_cap`
sat well covered in isolation, which is exactly the "a pure function's composition with its
caller" split the testing rules warn about.
`the_byte_cap_comes_from_the_schemas_type_for_binary_columns_only` closes it on four counts:
which columns get a cap (binary only), where the type comes from (the loaded `TableInfo`, not
`rs.columns[ci].type_name`, whose length the wire has stripped), how the schema column is
matched (by `origin.column`), and that the dialect really reaches the answer.
The answer rides to the panel on
`BlobTarget::cap` rather than being asked for when the file comes back, because by then the grid
it was read from may have been re-run underneath.
`refetch_key` reads a `Bytes` edit's **original** value rather than guessing at a text form for
it, so the re-fetch finds nothing rather than the wrong row. That arm is unreachable today,
since `resolve_key` never lets a binary column be a key; it is written down because the one
thing that function must not do is invent a byte string's text form. **A staged `Default` takes
the same arm, and that one is reachable** — a key or confirming column can be set to its default
whenever `Column::takes_default` holds. The value it was changed *to* is the server's and not
known here, so the original is the only thing to look the row up by: either the default left it
as it was and the row is found, or it moved it and the short answer re-runs the whole result —
never another row's. `NULL` in its place would key the re-read on a value nobody wrote
(`a_key_column_set_to_its_default_refetches_by_its_original`).
**`staged_cell` is the one revert-to-original rule**, for a single cell: `Revert` when the value
being staged *is* what was already there — drop any edit at that cell and don't count it — and
`Stage` otherwise. The NULL normalisation is inside it rather than at the call site because it is
the whole of the decision: a NULL cell *displays* as the four characters `NULL`, so comparing the
display text made a NULL original read `Some("NULL")` and never equal a pasted NULL, turning
"copy a column and paste it straight back" into a full staged rewrite and making `paste_report`
announce `Pasted 300,000 cells` over a paste that changed nothing. A cell that is not there at
all is `(None, true)`, which is what an out-of-range index silently meant in the view.
`GridState::stage_many` is a loop over it and counts exactly the entries it answers `Stage` for,
so the report and the dirty map cannot disagree about what landed.
**`cloned_row` is *Duplicate row*'s rule, and it sits here beside `staged_cell` because it is the
same question — what does this cell say — asked by a path that answered it for itself.**
`add_cloned_rows` read `rs.cell` unconditionally and never consulted `dirty`, so typing `Bob`
into a cell, watching it go green, and duplicating that row seeded the copy with `Alice`, the
value still on the server, with nothing saying the copy differed from the row that was copied;
commit, and the table gained a row the user had never previewed. Every neighbouring reader had
already been fixed the other way — `ai_fill_value`'s row context prefers the staged edit,
`blob_value` subscribes to `dirty`, and the aggregates bar's own note is that it reads each cell
*as the grid draws it* — and `GridCells` exists to be the one resolver they all go through. It
is core rather than a `GridState` method for the reason that split shipped the bug: no test can
construct a `GridState`. The clone's two exclusions moved with the rule and keep their reasons —
a column that is not `text_editable` (a binary cell *displays* `<n bytes>`, and copying that
would put the placeholder into the new row as text, so the `INSERT` takes the default instead)
and an auto-increment column the server assigns. Watched failing:
`a_clone_copies_the_staged_value_not_the_stored_one` reports `Some(Text("Alice"))` against the
unstaged lookup where `Some(Text("Bob"))` is what the user is looking at.
**How large that gesture may get before it asks is here too.** `DUPLICATE_CONFIRM_FLOOR` (500),
`duplicate_needs_confirm` — strictly above it, so the floor itself still goes through without a
modal — and `duplicate_prompt`, which names the count and says the rows are *staged*: the
question is about the size of a gesture, and a prompt implying a statement had reached the
server would be the wrong warning on the one grid action that commits nothing. The gutter menu
acts on the whole selection, so Ctrl+A on a 200,000-row result offers *Duplicate 200,000 rows*,
and taking it builds 200,000 `HashMap`s — one per row, every text-editable column cloned into
each — on the UI thread. The write is already one batched update rather than N, which is what
keeps the *staging* from being quadratic; nothing bounded its size. A **floor** rather than a
cap because the count is in the entry's own label: the user asked for this number and can see
it, so the honest answer is to make them say yes, not to refuse. `stats::CONFIRM_ROW_FLOOR`
(1,000) is the neighbouring idea for a destructive figure worth naming; this one gates whether
the question is asked at all and sits lower, staging being undoable and merely expensive. Which
caller asks it is the load-bearing half and lives in the view — `grid::clone_rows`, the action,
never the menu entry (*Data grid*).
**The staged-cells → `RowEdit` grouping is here for the same reason, and it is the step a paste
stresses.** `build_edits(model, rs, dirty)` folds a `DirtyCells` map — `(data row, result
column) → new value`, each a `model::CellEdit`, the shape `GridState::dirty` holds — into one
`RowEdit` per (base table, data row) via `row_edit_for`, which builds the `SET` list and the
`WHERE` key off the row's *original* values; `origin_column` is the one reader of a result
column's real name on its base table. `GridState::build_edits` is now a thin wrapper over it. A
typed edit stages a single cell, so every fixture this family has ever had carried one `SET`
column and one key column, while a paste stages a rectangle — and grouping that rectangle is
exactly what stands between it and `GridWrite::plan`'s 1-row safety net. The grouping lived in
`GridState`, which no test can construct, so the chain paste → `dirty` → these edits → `plan` →
`one_row_verdict` was untested end to end. The `BTreeMap` and `row_edit_for`'s sorted `SET` list
are load-bearing rather than tidiness: a failing commit has to reproduce identically.
**Three grid gestures resolve to a decision here rather than in the view.**
`selected_data_rows(order, selection, pos)` is what a gutter gesture acts on: the highlighted
range when the click landed inside it, otherwise the row it pointed at alone, mapped through the
grid's display→data `order` and with pending new rows (which live past `order.len()`) left out.
It is pure because it decides *which rows are deleted* — on a sorted grid the display index and
the data index are different numbers, and the write-back's 1-row net checks the count and not the
identity, so an inverted mapping deletes the wrong row and reports success. `copy_scope` answers
what a cell menu's Copy takes — `CopyScope::Selection` for a right-click inside a multi-cell
block (which no longer collapses the selection, so the menu is about the block),
`CopyScope::Cell` otherwise — and `CopyScope::label` gives the two amounts two words, because an
entry reading "Copy" that took one cell out of nine said what Ctrl+C and the gutter menu's own
Copy say for three different amounts. `attach_span(r0, r1, cap)` returns *sent* and *selected*
as two numbers: `prompt::ATTACH_ROW_CAP` is about the context window rather than about consent,
so exceeding it is reported in the header instead of silently applied, and a figure derived from
the rows that survived would tell a user who picked 900 that 200 went.
**And the surfaces that *read* the grid resolve a cell here rather than in the view.**
`GridCells` is a borrow struct over what the grid's signals hold — `rs`, the display→data
`order`, the per-column `formats`, `dirty` and `new_rows`, plus the `marks` a formatted column
is written in (the interface's, as the painter writes them) — and `text(i, ci, formatted)`
resolves one *display* cell in the painter's order: a pending new row's typed value, then a
staged edit, then the stored cell through `format::apply`. `tsv_block(rect, layout)` is the
clipboard's block **plus what the format could not carry** — `TsvBlock { text, split }`, `split`
counting the copied cells that hold a tab or a newline of their own, phrased by
`copy_split_note` and reported by the grid (`tsv` is the wrapper that throws the count away);
and `attached(rect, cap, layout)` is an AI attachment's column names, rows and pre-cap
total — both emitting the selected columns in the order they are **drawn** (`visual_cols`) rather
than in index order, because whoever receives the block reads it left to right and a copy across
a freeze reached a spreadsheet transposed.
**`exported()` is the fourth surface, and it is the one this list did not have.** The export
could not ask this type for a cell at all: it renders through `core::export`, which walks a
`ResultSet` and a display order and has no notion of an overlay — so `Copy ▸ CSV`, the two
column copies and the saved file each wrote the pre-edit value while Ctrl+C on the same
selection wrote the staged one, and a pending row appeared in none of them. It resolves
*first* instead of teaching a second reader what `dirty` means: `exported` returns the stored
`ResultSet` with the staged edits spliced in per cell (`model::ResultSet::splice_cells`) and the
pending rows appended (`append_rows`), plus the display order to read it in, with the pending
rows past that order's end exactly as they are drawn past the real ones. Per *cell* because
supplying whole rows re-formats the untouched numerics it round-trips — `model`'s entry has
that. Unformatted, like `tsv`: a formatter is how a value is shown and an export writes the
value, so `formats` is the one field of this struct the method does not consult. Cheap on a
clean grid, since the columns are refcounted and `splice_cells` rebuilds only a column whose
listed cell really differs. `exported_rows_at(selection, pos, layout)` is that resolution
narrowed to what a gutter gesture at **display** row `pos` means — the highlighted rows when
the click is inside them, else that row alone, the reading `selected_data_rows` gives — for the
*Copy as* on the gutter's menu and the cell's. It slices the *exported* order by display position rather than reading the
selection as data indices, which on a sorted grid are different numbers and would copy the wrong
rows; slicing the resolved order is what carries the sort, the staged values and the pending
rows at once. Unlike the delete it keeps the pending rows, since a copy has nothing to refuse
them for, and a range past the last row is clamped rather than a panic
(`exported_rows_at_clamps_to_the_rows_that_exist`). **And the columns the gesture means, in the
order they are drawn**: inside the highlighted block, the block's columns in `visual_cols` order;
outside it, the whole row in that order. It used to take only the row pair and copy every column
in index order, so one menu said *Copy* for a 3×2 block and meant six cells while *Copy as*
beside it took every column of the same three rows
(`exported_rows_at_takes_the_blocks_columns_in_drawn_order`). `gesture_at` reads the rows and
columns off the gesture, `pick` slices the order and `project` narrows the columns, handing the
result back untouched for a whole row outside a freeze. **`insert_blocks_at(selection, pos,
layout)` and `insert_blocks_all()` are the same gesture, and the whole result, for an `INSERT`
— and they return blocks rather than one result, because a pending row's unset cell is not a
NULL.** It takes the server's default — the grid previews it as `<auto>` and its own Commit
leaves the column out — so an `INSERT` listing it as `NULL` wrote something else: PostgreSQL
refused the row over a `serial` key, and MySQL in non-strict mode stored an implicit `0` or `''`
over the declared default. So the stored rows are one block, and the pending rows are grouped by
which columns they set, each group projected to those (`model::ResultSet::project_columns`); a
pending row with nothing set has nothing to insert and is left out
(`an_insert_of_a_pending_row_omits_the_cells_it_did_not_set`). `DEFAULT` in `VALUES` would say
the same in one statement, and SQLite refuses it. A pending row never holds a
`CellEdit::Default` for this to meet: *Set to DEFAULT* on one puts the cell back to unset
(`GridState::unset_new`), which is what the default means before the row exists. CSV and JSON keep `exported_rows_at`'s single
result, blank cells and all.
**`updates_at` and `where_in_at` are the two *keyed* copies — *Copy as ▸ SQL UPDATE* and *SQL
WHERE IN* — and they aim by `row_key`, the identity the write-back itself uses**, so a copied
`UPDATE` names the row the grid's own Commit would, by its **original** key values rather than
a staged one. The private `key_match` is that key as SQL text, with `IS NULL` for a NULL, since
`=` never matches one and an implicit key's confirming column may hold it. The `SET` list is the
gesture's columns that are `text_editable` and not key columns, valued from `exported()` — what
the grid shows, staged edits included — through `export::script_literal` and named by
`origin_column` through `export::ident_sql`; so a staged change to a key column is in neither
half of the copy. A pending row has no key yet and is skipped (`saved_rows`)
(`copy_as_update_sets_the_staged_value_and_keys_by_the_original`). **A column past the
result's arena ceiling (`ResultSet::capped_columns`) is left out of the `SET` list**: its
later cells read blank while holding real text, so `SET body = ''` erased every value it was
run over (`copy_as_update_leaves_out_a_column_the_arena_blanked`); a key or confirming column
there refuses both copies (`key_aimable`), since `WHERE k = ''` names another row or none.
**The `UPDATE`'s table is `export::qualified_row_target`**, the name Commit writes to — three
parts on SQL Server (`copy_as_update_on_sql_server_names_the_database`). `where_in_at` writes
`WHERE k IN (…)` for one key column and `OR`ed, parenthesised matches for a composite one — SQL
Server has no tuple `IN` — each row once, in display order
(`copy_as_where_in_ors_a_composite_key`); it refuses an implicit key (non-empty `confirm_cols`,
SQLite's `rowid`), a rowid alone being no identity a pasted predicate should rest on and, with
its confirming values, no longer a key list. Both refuse with `CopySqlRefusal::NoKeyedTable`
unless `EditModel::insert_target()` names one table — the gate Duplicate and Delete row ask —
and the refusal is the `Err` the grid puts on the bar instead of copying. **It is a value, not
a sentence**: `CopySqlRefusal` is every reason either copy gives — `NoKeyedTable`,
`KeyTooLarge` (`key_aimable`), `UpdateColumnsTooLarge` and `UpdateNothingWritable`
(`updates_at`), `WhereInNoKey`, `NewRowHasNoKey` (`saved_rows`), and the `Pinned` and
`ReadOnly` that `withheld_model_reason` answers — with `message()` the English and `Display`
it. They were a `COPY_NEEDS_ONE_KEYED_TABLE` const and `&'static str` errors, which reached
the bar in English whatever the interface's language; the grid words the value
(`copy_sql_refusal_text`, under *Data grid*). One rule,
because this resolution kept going out one source short where nothing could test it:
`attached_rows` first read `rs.cell` and never `dirty`, so a green uncommitted edit was on
screen while the pre-edit value went to the model, and the fix for *that* left the rule in
`grid.rs`, where it went short again — no `format::apply`, so a `Timestamp` column sent
`1709294400` where the grid showed `2024-03-01 12:00:00`, with the sent-attachment card
agreeing with the wrong copy because it is built from the same rows (`Bytes`, `Grouped` and
`Bool` columns diverged the same way). `grid.rs`'s `copy_selection` and `attached_rows` are now
the signal reads and nothing else; its `displayed_cell_text`/`pending_cell_text` are gone.
**The painter is deliberately not a caller.** `data_cell`'s content `dyn_container` runs per
cell per frame and reads the signals one at a time, so it stays the reference implementation
and `GridCells::text` is written to match it — change one and read the other. `formatted` is a
parameter because the two readers differ on purpose: an attachment passes `true`, its whole
promise being that the model is answering about what the user is looking at, while Ctrl+C stays
raw and the cell menu offers *Copy formatted* as its own entry. A staged value is never
formatted either way, because it is text the user typed and the painter doesn't format one.
**`with_text` is the same resolution handed to a closure borrowed wherever it can be**, and
`text` is now the wrapper that allocates. Three of the four sources are already `&str` in
memory — the arena text of a stored cell, and a staged or pending `CellEdit::Text` — so only a
staged NULL or blob (which render as a sentinel) and a formatted column have to build anything.
It was written for the find bar's jump: `grid_find` walks up to `rows × cols` cells from the
caret on *every character typed into the find box*, and each cell cost two heap allocations,
`CellRef::to_value` copying the arena string and `Value::display` copying it again. At the
200,000-row cap × 50 columns that is ten million cells and twenty million allocations,
synchronously on the UI thread, per keystroke, whenever the needle or one of its prefixes is
absent — while its sibling the match *count* is debounced and capped for exactly this reason
and says so.
**`summary_text` is the fourth reader and differs from `text` on one point that is the whole
point**: a NULL has to be *absent* rather than the string `NULL`, because the aggregate counts
nulls and parses numbers out of the rest. A **staged blob is a value** there —
`CellEdit::as_text` answers `None` for `Bytes`, which is what every text caller wants (the
re-fetch key, the paste round-trip, the clipboard) and read as NULL here, so loading a file
into a selected cell turned `3 rows` into `3 rows · 1 null` while the *stored* blob it replaced
counted as a value. That was fixed once for the staged-over-a-real-row arm and the
pending-new-row arm four lines below reproduced it verbatim, which is why the rule now lives
beside the painter's own resolution and there is one of it. An **unset** cell of a pending new
row is deliberately not the same thing as a NULL: what it will hold is a server default,
previewed as `<auto>`, and a staged `CellEdit::Default` on a real row is that same case — the
server has not yet said which it is. **`summary_text` cannot say so**, its `None` being both,
and for as long as it was what the summary counted, both printed as nulls while this paragraph
said they counted as neither. **`summary_cell` is the reader underneath it**, answering an
`aggregate::Counted` — `Value`, `Null`, or `Unset` for those two — and `summary_text` is now
`summary_cell` collapsed to an `Option`. The selection summary hands `summary_cell` straight to
`aggregate::aggregate_counted`, which counts an `Unset` as a row and never as a null
(`a_staged_default_is_counted_as_a_row_and_not_as_a_null`); keep it off `summary_text`, which
would quietly bring the old count back. It returns a `Cow`, for `with_text`'s reason — a Ctrl+A over a 200,000-row column
resolves every cell and only a staged blob's `<n bytes>` has to be built.
**`fill_plan(rect, layout, editable, fill)` is the cell menu's *Fill down* / *Fill up*, and it
reads its source through `summary_cell` because all three of that reader's answers are ones a
fill has to tell apart.** For each selected *drawn* column (`selected_cols`, hidden ones out) it
takes the selection's top cell (`Fill::Down`) or bottom cell (`Fill::Up`) and plans it into
every other row of the selection, as `(display row, column, Option<String>)` for the grid to
stage as it stages a paste. The value is what the grid shows less the formatter — a staged
edit wins over the stored cell, a NULL is `None` and so fills as SQL NULL rather than the
string `NULL` — which is the paste round-trip's reading of the same cell. `Unset` is the other
distinction: a pending row's unset cell and a staged `DEFAULT` will both take a server
default, so a column whose source is either plans nothing at all rather than writing NULLs
over the rows below it. The staged `DEFAULT` was read through `summary_text` as a NULL and
filled as one — a NOT NULL column then failed the commit, and a nullable one wrote NULLs where
the user had asked for the default (`fill_down_from_a_staged_default_fills_nothing_in_its_column`). A column
`editable` refuses is skipped in place, as a paste's is, and so is a column past the arena
ceiling, whose blank cells hold text the result never carried — filling with their `''` is
what Commit would then write (`fill_skips_a_column_the_arena_blanked`); a selection of one
row, after the rows are clipped to the real ones plus the pending ones, plans nothing
(`fill_down_copies_each_columns_top_cell_into_the_rows_below`,
`fill_up_copies_the_bottom_cell_as_the_grid_shows_it`,
`fill_skips_what_it_cannot_write_or_has_no_value_for`).
**`staged_sql` is the staged batch as SQL a person reads — the commit strip's *Copy the changes
as SQL* — and it renders the very `GridWrite` Commit would send** rather than reading `dirty` a
second time, so the copy and the commit cannot come to differ on which rows or which keys: the
statements are `GridWrite::plan`'s, in its order (DELETEs, UPDATEs, INSERTs), one a line
(`staged_sql_is_the_plan_in_its_own_order`). **It is not what Commit executes, and does not
pretend to be**: the commit binds its values and matches a key NULL-safely (`<=>`, `IS NOT
DISTINCT FROM`), where this writes literals through `export::script_literal` and a key as `=` or
`IS NULL`, `key_match`'s spelling. A literal's type is its result column's, found by matching
provenance because the plan names columns rather than indexing them — which is what gives SQL
Server its `N'…'` (`staged_sql_speaks_each_engines_dialect`); names go through
`export::ident_sql` and the table through **`export::qualified_row_target`, the rule
`db::mssql::qname3` delegates to** — so on SQL Server the copy is three-part, as Commit is. It
was `qualified_table`, which leaves the database out there for an export's sake, and a copy of
an edit to `B.dbo.t` pasted back into a tab in A wrote A's same-named table
(`staged_sql_on_sql_server_names_the_edits_database`). An insert that sets an IDENTITY column
is bracketed by `SET IDENTITY_INSERT … ON/OFF` on SQL Server, as Commit runs it — the identity
read off the result column's `flags.auto_increment`
(`staged_sql_brackets_an_identity_insert_on_sql_server`). An `INSERT` that sets no
column is `() VALUES ()` on MySQL and `DEFAULT VALUES` elsewhere. A staged `CellEdit::Default`
prints as the keyword `DEFAULT` on every engine
(`staged_sql_writes_a_staged_default_as_the_keyword`) — which is what three of the four writers
send, and **not** what SQLite's does: there Commit substitutes the column's declared default,
read on the write connection, and the script cannot see it, so the copied `SET c = DEFAULT` is a
syntax error if run on SQLite. **A staged blob is withheld,
not written**: no literal here carries bytes, so its value is the grid's `<n bytes>`
placeholder *unquoted* and a `-- NOTE:` line heads the text — the statement fails loudly if run,
where a quoted placeholder would store that string in the column
(`staged_sql_says_null_keys_and_withholds_bytes`).
**`order_key_columns` is `resolve_key`'s ladder walked over the *table* alone**, for a caller
that needs the key before it has a result set. The Live Monitor is that caller: it must put an
`ORDER BY` on the very first poll and cannot ask `analyze_edit` until a poll has come back, and
it reached for the primary key alone instead — so the whole band "no PK, but a usable key
exists" polled **unordered** while still diffing happily, and the exportable log filled with
inserts and deletes that never happened (see `monitor::Snapshot::ordered`). It cannot be
`resolve_key` and does not try: that one interleaves a *presence* check against the result's
columns and falls from a primary key that is not fully projected down to an index that is,
while there is no result here to check against and the caller selects `*`. SQLite's implicit
`rowid` is likewise out of reach — it is asserted by the backend on a returned column, not by
the schema.
**`visual_cols(ncols, frozen)` is the one definition of the order the grid draws columns in** —
the frozen columns, then every other column in index order. **Several may be frozen**, and they
are drawn first in *index* order whatever order they were frozen in; `frozen` is any iterable of
indices, an `Option<usize>` being the one-column case, so every caller from before there could be
several is unchanged (`several_frozen_columns_are_drawn_first_in_index_order`). It is a set
rather than Excel's "freeze through this column" because Freeze was already offered on any
column — column 3 alone, drawn first — and a through-column reading would have changed what that
one gesture meant; a set keeps it and makes a second freeze its generalisation. A frozen column
keeps its *absolute*
index on purpose, so selection, sort and resize stay consistent, which means draw order and index
order disagree the moment `frozen` is `Some(f)` with `f > 0`. That is fine for a rectangle's
*membership* and wrong for everything about adjacency or reading order: `plan_paste` extends a
block along this order from the anchor, and `tsv`/`attached` emit along it. Walking the index
range instead put the second value of a two-wide paste dropped on `email` into a frozen `ssn` —
a column at the far *left* of the screen the user never pointed at, one `UPDATE … SET ssn` away
from destroying it — and sent a copy across the freeze to a spreadsheet transposed. One list,
because the grid's pane builder and `grid::scroll_active_into_view` partition the columns the
same way (both now through `ColLayout::scrolling`, below) and a second spelling of the order is a
second chance to disagree with it. The **single-value** paste is not
an exception: it fills the *selection*, which is the set of cells already painted highlighted, so
no translation applies. **What this deliberately does not fix**: a selection whose absolute range
straddles a frozen column is visually discontiguous, so copy→paste of such a selection no
longer round-trips. The ambiguity is in the selection model, not in either surface, and both now
agree with what is drawn rather than with each other.
**`ColLayout` is that order once columns can be hidden** — the frozen and the hidden columns,
each a `BTreeSet` of absolute indices (`is_frozen`, `is_hidden`) — and hiding is the stronger of the two reasons draw and index order part: a
frozen column only moves, a hidden one is not there. Every surface above takes
`layout: impl Into<ColLayout>` where it took `frozen`, and `From<Option<usize>>` makes a frozen
column alone a layout, so every caller and test that predates hiding still passes `Some(n)` or
`None`. It answers four different lists, and they differ on purpose: `visual_cols` is the free
function's order less the hidden — what copy, attach and a block paste walk; `shown` is the drawn
columns in **index** order — what the arrow keys step through; `scrolling` is `shown` less the
frozen columns — the data pane's columns and what scroll-into-view sums widths over, equal
to `visual_cols` less its frozen head
(`the_scrolling_pane_is_the_draw_order_less_the_frozen_column`); and `frozen_drawn` is the frozen
columns the frozen pane draws, in index order, leaving out one that is hidden — not drawn first
but not drawn at all (`a_hidden_column_leaves_the_draw_order`) — or past the result. `selected_cols` keeps the absolute range as
membership and subtracts the hidden, since the grid paints a hidden column inside the range not
at all (`a_copy_across_a_hidden_column_leaves_it_out`). A block paste extends across a hidden
column to the next drawn one, and the single-value fill skips it, its span taken from the columns
it fills so the skip is not reported as `dropped` (`a_paste_skips_a_hidden_column`) — a write
aimed at the screen lands on the screen. `may_hide` refuses the last drawn column, because a grid
of none is a gutter with nothing on it to bring the others back from
(`the_shown_columns_and_the_last_one_that_must_stay`). **The inline editor is drawn columns
only, too.** `next_editable` is the Tab/Enter hop, walking `visual_cols` — draw order, so Tab
off a frozen column goes to the one drawn beside it — with `None` the caller's cue to close the
editor. The grid's `next_editable_col` used to walk `ci+1..ncols` by index, so a Tab past a
hidden column opened an editor nobody could see; it seeded a NULL cell with `""`, and the next
commit wrote `SET hidden_col = ''` over the NULL (`the_editor_hop_steps_over_a_hidden_column`,
`the_editor_hop_walks_the_draw_order`). `first_editable` is where `+ Row` opens its editor and a
clone lands, which picked the first editable column by index for the same reason
(`a_new_rows_editor_opens_on_the_first_drawn_editable_column`). `edges` is the first and last
drawn column in index order — the corners of every **whole-row** gesture (the gutter, *Go to
row*), which said `0` and `ncols - 1`: with the first column hidden, a gutter-selected row was
anchored on a cell nobody could see, a paste there found no drawn column to start from and
dropped every value (`a_block_paste_onto_a_gutter_selected_row_lands_past_a_hidden_first_column`),
and Enter opened an editor the data pane never built. With nothing drawn it is `(0, ncols - 1)`,
saturating (`the_edges_are_the_first_and_last_drawn_columns`). `hide` is the hide itself — the
unfreeze, of that column alone, and the corner move described under *Data grid*, through a private `nearest_shown` that
used to live in the grid (`hiding_a_column_unfreezes_it_and_moves_the_corners_off_it`,
`a_corner_on_a_hidden_column_moves_to_the_nearest_shown`). **What hiding deliberately does not
reach**: the whole-result exports, `exported` and `insert_blocks_all`. They are about the result,
and the *All rows* export re-runs the query on the server, which knows nothing of a grid's
hidden columns — so a fetched export honouring the hidden set would have two exports of one
result disagree about its shape. The gesture-scoped `exported_rows_at`/`insert_blocks_at` do
honour it, being about what was pointed at.
