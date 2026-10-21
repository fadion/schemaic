# `ui/copy_table_view.rs`

`copy_table_view.rs` — the **Copy to** modal (`copy_table_overlay`, over `core::copy_table`): a
table's structure and — unless *Copy data* is off — its rows into another database, on this
connection or another of the same engine. It is raised from a table row's *Copy to* in `overlays.rs`, directly after the Export
submenu — the other way a table leaves the tree, into a database rather than a file — offered
where `offers.edit && !is_view`, and **not dimmed on a read-only source**, since it only reads
the source and the target's flag is asked at the preview's Apply. `menu_order_gate` files it in
the write group (4): it writes another database, if not this one. Setting `OverlayUi::copy_table`
to a `CopySource { conn_id, database, schema, table }` opens it; the source is always the
active connection's, the tree showing no other. `CopySource`, `CopyRowsRequest`, `CopyOutcome`
(`Done(n)`, `Cancelled`, `Failed(sentence)`) and `CopyRowsFn` are re-exported from `lib.rs`.
**The `CREATE TABLE` goes through the DDL preview like every other generated DDL, and the rows
follow only once it has committed.** *Preview SQL* first refuses — *"The active connection
changed — reopen Copy to from the table."* — when `ConnUi::active_conn` is no longer
`CopySource::conn_id`: the schema it reads and the dialect it emits in are the **active**
connection's, so after a switch they would describe another server's same-named table while
the rows still came from the source. It then reads the source's `TableInfo` through
`table_designer::loaded_table` — refusing with *"expand it in the tree"* while the database's
schema is not loaded — asks `copy_table::refusal`, builds `copy_table::plan`, and opens it with
`ddl_preview::preview_of` on the **target** connection and database, with the target's
`read_only`. With **Copy data** on it pushes `rows_line_text` — `copy_table::rows_change` worded
from `copy-table-rows-line` (`english_says_the_rows_line_as_core_does`) — onto the preview's
`changes`, so the heading counts the rows as a change of their own — they are no statement in
the script, and without the line the preview read as a structure-only copy while Apply went on
to write every row — and sets a `then` (`DdlPreview::then`, under `ddl_preview.rs`) carrying a
`CopyRowsRequest` built at that moment — `copy_table::rows_read`'s `select`, `columns`, `forms`
and `overriding`, and the `settle_sql`.
So the write guard and the target's read-only flag are the preview's, asked at Apply, and a
`CREATE` that fails, is declined or is superseded copies no rows. `then` closes the preview
through `ddl_preview::close_preview` — the one door, so the SQL box is cleared with it —
switches the modal to *Copying* and calls `CopyCtx::copy_rows`, which `modals.rs` fills from
`SchemaActions::copy_rows`. **With *Copy data* off the copy is the structure alone**: no rows line, and
a `then` that only clears `OverlayUi::copy_table`, leaving the preview's own *Applied* as the
whole report rather than bringing this modal back to a *Copying* stage for rows that are not
coming.
**It stays open behind the preview it raised, and hides itself while that is up** —
`copy_open && preview_open.is_none()`, the clause `modals::workspace_modals_up` asks too —
because the preview is painted in the earlier DDL group and a backdrop of this one's over it
would swallow every click meant for Apply. Compare closes itself on the way to its preview;
this one cannot, being where the rows are counted.
**The target picker is compare's, borrowed.** Connection first — filtered to those
`connection::same_engine` matches with the source's, the source's own included — then that
connection's databases through `SchemaActions::compare_list_dbs` into
`OverlayUi::compare_dbs`/`compare_dbs_err`, the same lister and the same signals, since the two
modals never open together and it is the same question; rows are `compare_view::pick_row`,
`pub(crate)` for this. The database is picked against the connection id that travels with the
list, compare's defence against a stale listing. **Before a choice the list is the whole
question and cannot be closed**: the *Copy into* button (`in_ring_button`, ring tabindex 10,
reading `connection · database`, toggling the list open and shut) is built only once a
database is chosen — a `dyn_container` on a `has_dst` memo. It used to be there from the start,
reading *"Choose a database"*, and pressing it folded the list away into an empty field whose
only way back was to press it again. **The list is one Tab stop**, `widgets::pick_group` at
`LIST_TAB` (15, between the button's `CHOSEN_TAB` 10 and Schema's 20): Up/Down move a cursor
over its rows (`widgets::cursor_row`), Enter or Space picks the one under it — the modal's first
question had been pointer-only, its rows plain `pick_row` clicks, so *Preview SQL* was
unreachable without a mouse. Each step of the picker rebuilds the list and with it the view
that held the keyboard, so a pick made from the keyboard (`keyed`) hands it on, deferred, to
the new list or — once a database is chosen — to the button above it (`FocusRing::focus_at`).
The modal clears `compare_dbs` and
`copy_progress` when it opens. Below the picker are **Schema**, hidden when the source has
none, defaulting to the source's, **Name**, defaulting to `<table>_copy`, and **Copy data**, a
`settings::focusable_toggle_row` on by default at ring tabindex 40, after Name. Under them a
line follows the toggle and says what does not come: foreign keys and triggers, and — when the
rows come — a generated column's values, which are computed afresh. Below that sits the
refusal line, and **an effect clears it whenever `dst`, `name` or `ns` changes**: a refusal
answers the form as it was when Preview was pressed, and it used to stay up after the user had
fixed what it was about. The effect clears any refusal, the active-connection and
schema-not-loaded ones included, though neither is about those fields — *Preview SQL* gives
them again if they still hold. **Preview SQL is disabled until a database is chosen and Name
is non-blank** — a `ready` memo, keyed into the footer's `dyn_container` beside `stage`, so the
footer rebuilds when readiness flips rather than on every keystroke in Name. `preview` still
refuses both itself, and that is not redundant: the Name field's Enter reaches it directly,
past the disabled button.
**Three stages** — *Choosing* (the form, Cancel and Preview SQL), *Copying* (*"Copying rows…
n"* off `OverlayUi::copy_progress`, and Stop) and *Finished* (what happened, and Close). The form
is built once and only hidden past *Choosing*, so its fields keep their text. A cancel reads
*"Stopped before every row was copied. The table was created."* — not "holds no rows", which a
MyISAM or Aria copy (the draft carries the source's `engine`) does not make true; a failure is
the app's whole sentence, since only it knows whether the rows were in before what failed. **There is no click-away**:
a stray click must not close a copy in flight. ✕ and Escape close it, and **closing
mid-copy cancels** through `SchemaActions::copy_cancel`.
**The rows are the app's** (`main.rs`, beside `compare_list_dbs`). `copy_rows` **guards its own
launch** by re-reading the target's read-only flag — Apply asked it for the `CREATE`, and this
is a second write, launched later — and a target connection deleted meanwhile **fails the
copy** rather than falling back to some engine. It runs `Db::copy_rows` from the source's `Db`
into the target's, then `run_ddl(settle)` on the target when there is anything to settle, on
the same token, reports once through `create_ext_action` and logs `Source::Copy` (under
`app/main.rs`'s write-log paragraph). Progress comes through a crossbeam channel into
`copy_progress`, the dump's way, since an ext action answers once; `copy_cancel` cancels the
token held in a slot for the copy in flight. The failure sentence says which side of the
commit it fell. Before it, *"The copy stopped on an error: {e}"* — **deliberately not "the table
is empty"**: the rows are one transaction, but a non-transactional copy keeps what arrived, and
`e` carries `Rollback::note` saying so, so the sentence used to contradict its own error. After
it, *"{n} rows were copied, but the table's counter was not moved past them, so the next insert
may collide: {e}"*.
**The modal's own words are the catalogue's** (`copy-table-*`) — title,
fields, the *Copy data* toggle and its notes, the refusals it raises itself, the preview's
*Copy of {table}*, progress, the stop note and *Copied N rows.*, a plural
(`the_shared_counts_read_as_they_did` pins 0, 1 and 3) — with Cancel, Preview SQL, Stop and
Close the shared `dialog-cancel`, `editor-preview-sql`, `dialog-stop` and `dialog-close`.
`copy_table::refusal` answers a `CopyRefusal`, which `copy_refusal_text` words
(`copy-table-unnamed`, `-choose-database`, `-onto-itself`), held to its `message` by
`english_says_every_copy_refusal_as_core_does`. The failure sentences above are `copy-stopped`
and `copy-counter-not-moved`, and the copy's own error inside the first is `db_error_shown`'s,
so a `DbError::Write` — the rollback clause included — is worded too. That is the modal's
copy only: the write log's entry takes the same error's `to_string()`, carried beside it as the
app's `CopyFailed` `(logged, shown)`, because one string for both wrote a German interface's
wording into the persisted log. The settle's `run_ddl` failure, a `DdlError`, is
`ddl_error_shown`'s beside its `to_string()` the same way — it used to pass `to_string()` as
both. What stays English: the server's words inside either error, and the `<table>_copy`
default, which is an identifier.
