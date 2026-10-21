# `ui/dump_view.rs`

`dump_view.rs` — the **schema + data dump** modal, **Export** to the user, over `core::dump`.
Three entry points and one modal: a database's schema context menu and a PostgreSQL namespace's,
where it is the middle of **`Import → Export ▸ → Create ▸`** — the three entries about the node
as a whole, behind their own separator, a database's carrying *Back up* and *Restore from file*
between the last two (`core::launch`) — and a **table's, directly below *Import***. All three
pair it with the import it is the round trip of. **`Export` is a submenu of six**, one entry per
`export::ExportFormat::ALL` — JSON, CSV, SQL, Markdown, HTML, Excel, in the order the grid's
Download menu already lists them, so a user meets the formats in one order throughout the app.
All three nodes build it through the one `overlays::export_submenu(&DumpCtx, database, schema,
preselect)`, over a single ctx `context_menu_overlay` builds for the whole menu: the entry is
the same offer wherever it appears and the arms differ only in what they hand the picker, while
spelled out per arm the six labels and their order would be three lists to keep in step. It is
the one label that sits inside a writing group
without writing anything, which is why `menu_order_gate` exempts it from the skeleton by name:
it writes a *file* and never the server, so it can never be the irreversible entry the ordering
exists to place. The table
entry passes `open_dump`'s `preselect`, which ticks that one table instead of all of them, and
deliberately passes **no namespace**: the picker still lists the whole database, because
narrowing to the table's own namespace would hide the neighbours its foreign keys point at. It is
offered on a **read-only** connection, unlike Import — it reads the server and writes a local
file. The code keeps the `dump` name throughout, for the reason `core::dump`'s entry gives. It
collects a table selection and — for `Sql` — the six `DumpOptions`, and hands both to
`SchemaActions::dump_run`; nothing here builds SQL. **The *Drop before create* hint is
`dump::drop_hint(dialect)`**, a `DropHint` that `drop_hint_text` words (core's English being
`drop_before_create_hint`) — the only consent the export asks for what a replay
drops: on SQL Server (`drops_up_front`) the toggle also replaces every routine in the dumped
schemas, used by the tables or not, while the hint spoke of tables alone — so a replay of an
older dump silently put older routine code in place of newer. The file's header names those
routines too (`the_drop_toggle_and_the_header_name_the_routines_a_replay_replaces`). It also
says the other side: **without *One transaction*** no routine is dropped (the up-front section
is safe to fail in only inside the transaction), so a replay onto a database that still holds
one stops at its `CREATE` — a choice the hint once left unsaid
(`the_drop_hint_says_what_a_replay_does_without_one_transaction`). Every
other engine with routines (`ddl::supports_routine_editing`) gets a hint that says so as well,
since there the file replaces each routine it carries beside its `CREATE` (`dump::replay_create`,
under `core/dump.rs`); SQLite's still speaks of tables alone. Where a table's drop takes its
owner and grants (`access::drop_resets_table_access`) the hint says they are put back at the end,
and MySQL's that the routine grants its drops revoke are granted back where the account can read
`mysql.procs_priv`.
**One panel serves all six, and the split is asked as a capability.** Which entry was clicked is
`DumpTarget::format`, and `DumpTarget::writes_folder()` — `!matches!(format, Sql)`, written once
— is what the modal branches on: the title (`Export sakila to CSV`, the one thing about this
panel the body cannot show and the user cannot change from here), whether the option sections are
built at all, which dialog opens, which writer launches, what enables the footer's Export, and how
the outcome is worded. Six sites, and a `format == Sql` spelled out at each is six chances for one
of them to disagree with the others. `Sql` is the dump this module was written for, unchanged.
The other five write **one file per table into a folder** (`dump::file_plan` →
`SchemaActions::files_run`), and for those the two option sections are **not built**, not merely
hidden: every row in them is a `focusable_toggle_row`, so constructing and then hiding them would
leave eight tab stops behind controls that are not on screen — which is why they were lifted into
`dump_options(d, dialect, ring)`, and a `form_hint` naming the format takes their place. Shown
greyed instead they would read as options this format happens to have turned off, which is not
what they are: every one of them is about a `.sql` file, and a CSV can honour none of it.
`run_export` is the **one** branch point and the only place either writer can start, because the
thing that must never happen is a modal opened on CSV launching the dump. `run_files` opens a
**directory** dialog (`FileDialogOptions::select_directories()`, the first folder picker in the
codebase) and repeats every guard `run_dump` has, for the reasons stated there: the selection is
re-read *after* the dialog closes because floem's dialog is not window-modal and the modal stays
live behind it, the launch goes through `accept_dialog_launch` because by then the modal that
asked may be gone, and the outcome checks `generation` because closing the modal does not stop the
export. Its three sentences are `files_done_note`/`files_cancel_note`/`files_failure_note` —
`export::files_note`/`files_cancel_note`/`files_failure_note` worded here — rather than the
dump's, because all three are about a folder.
**Starting the export is `launch_files`, split out of `run_files` so it can be re-entered**,
which is what the replace confirm needs: a directory dialog has no "replace?" and the file names
are `file_plan`'s to choose, so the first run computes the collisions, writes nothing and comes
back `FilesOutcome::WouldReplace`. The view then raises the **shared** `Confirm` — not a fourth
bespoke overlay — with `folder_replace_prompt` (`dump::folder_replace_prompt`'s arguments,
worded here) as its body, and Yes re-launches through
`launch_files` with `FilesRequest::approved`, which re-reads the folder and is the more correct
answer anyway, since it may have changed while the question stood. `approved` is **never** true
on the launch the picker starts, the folder's contents being knowable only off the UI thread.
The re-launch takes the same `accept_dialog_launch` the picker's own launch takes, with the
generation captured *before* the question, so a confirm answered after the modal moved to
another database cannot start an export into it. The picker is **its own read**
(`SchemaActions::dump_tables` → `Db::fetch_table_list`, names only — `fetch_schema` would read
every column of every table to print them), because it has to be right for a database the tree
has never been expanded on, where a picker built from the cached tree would silently offer
nothing; a namespace's entry then filters that list to its own (`dump::tables_in_namespace`), so
a `sales` dump carries no `public` table. That filter goes through `schema::sql_qualifier` and
**not** a bare `"{ns}."` prefix, because `display_name` *omits* `public` — matching on the prefix
filtered a `public` dump down to nothing, none of its tables carrying one, and `None` from the
qualifier means "the unqualified ones are mine", which is exactly the set to keep. What opens
ticked is `dump::initial_selection`: everything, the common case being all of it — or exactly the
preselected table when the entry was a table's own, since the click already said which one, and
only if the read's list actually contains that name — when it doesn't, the modal **says which
table is missing** in its error line. A click that names a table the server no longer reports
(dropped or renamed since the tree was last refreshed) otherwise opened a full list with nothing
ticked and a dead `Export` button, which reads as broken rather than as an answer.
**What the picker says when it has no rows to show is `dump::picker_body`, matched
exhaustively** — `Reading`, `NoTables`, `Unreadable`, `Tables` — over a `dump::Listing` of
`Reading`/`Done`/`Failed` that `DumpUi::listing` carries in place of the `bool` it used to be. A
`bool` was enough while the read could only be "out" or "back", and it is not enough to tell
"back, and the database really is empty" from "never came back": the view branched on the flag
and then on `names.is_empty()`, so a listing that *failed* landed in the reassuring arm and
printed **"This database has no tables."** four lines above the connection error explaining why
the list was empty — two contradictory statements about one database, with the wrong one in the
panel the user is reading. It is the same shape and the same reason as `script::ProbeSummary`
next door, whose `== 0` branch described a file its probe had never finished reading as holding
*"no statements Schemaic can run"*. An enum rather than a second `if` so a fourth state cannot
fall into the reassuring arm by default, and the failing arm is the only place that knows: the
callback sets `Listing::Done`/`Failed` off the `Result` before it looks at the names.
The picker's rows read the selection through a `create_memo` of it as a `HashSet` and draw
`widgets::check_box`, the app's one checkbox, which fills and empties by style (`s.hide()` on
the tick), never a per-row `dyn_container`. It wore a check-glyph-and-hollow-square pair of its
own until the import list's box was made shared: two spellings of "ticked" in two modals is the
inconsistency a shared widget exists to prevent. Its header went the same way — the count, then
`widgets::link_button`'s "Select all" / "None", where two `ActionKind::Quiet` buttons labelled
"All" / "None" used to be. `chosen` stays a `Vec`,
because that is what the request carries and what "Select all" resets from, but every row
watches it, so
a single tick cost a linear `contains` per row *plus* a rebuild of every row — quadratic in the
number of tables listed. The memo makes each row's read O(1) and the show-hide is the *Floem 0.2
gotchas* rule (`display: none`/`flex` beats a rebuild), which here also means nothing is taken
apart under the pointer mid-click.
It follows `import_view`'s discipline for the same reasons: `widgets::accept_launch` in the same
synchronous step as the launch — inside the save dialog's callback (titled `Export to SQL`,
defaulting to `{database}.sql`), since that is where the launch is, and with `read_only` false
because a dump writes to the local disk and never to the server — a `running` flag that is what
the buttons gate on (`listing` says only how far the table read has got, and is read by the
picker's body), and `DumpUi::generation` bumped on every open so a table list
or an outcome that lands after the modal was reopened elsewhere reports into nothing. Every exit
— the footer's dismissive button, Escape, the ✕ — **stops the export rather than closing**
(`widgets::exit_action` with `cancellable: true`), the import modal's rule and for its reason:
closing would hide a write still going and leave its outcome with no reader, since the modal's
signals are the only channel the run reports to.
**The footer carries the run; the body is choices only.** The dismissive button is where the
stop lives: it reads `Close` and turns into a red `Stop` (`ActionKind::Danger`) for as long as
that is what pressing it does — a word and a colour, because in `Neutral` it reads as the same
way out with a different label, which is what it is not. `Export` is disabled while the run is
on. **The stop is deliberately not beside the progress text**: it was, for one revision, and the
line grows as its count does (`3 of 12, 9k` → `98k rows so far`), so the one control the user
wanted walked left and right under the cursor on every tick. The footer's **left** slot
(`modal_footer_split`) therefore carries text alone — the per-table line,
`Writing orders — 3 of 12, 56k rows so far`, the count through `text::human_count`, the same
abbreviation the grid uses — falling back to an animated `Reading the schema`
(`widgets::loading_dots`) before the first table, where there is nothing to count yet and a
static label and a hung one look identical. The **outcome** replaces it in that slot: the green
`Wrote 5 tables.` followed by `i18n::export_note` of `export::export_said` — the grid's own
caveat wording, reused
rather than restated, so a file that lost a column at the text arena's cap says so instead of
reading as a clean success — and the red failure or cancel sentence. **That wording carries the
withheld half again**, and a dump is one of the cases it was restored for: the clause naming
binary columns a text export could not hold had been dropped on the grid's argument that the cell
on screen already reads `<7 bytes>`, and a dump's reader is not looking at a grid at all. It was
said nowhere but the file's own `-- NOTE: binary column … exported as NULL`, and for a folder
export of CSVs — a format with no comment syntax — nowhere at all (*core::export*). A ticked table the run's **own** fresh
introspection could not find (`DumpPlan::missing`) is named in that same green sentence rather
than only in the file's header, for the reason the tally is named there: a file one table short
of what was ticked looks exactly like a complete one. `DumpUi::done` is therefore the
finished `String` rather than the counts for the view to word: it is built in the outcome
callback, which is the last place the destination's file name is still in scope, and
`export_said` names that file. Both used to sit at the
bottom of the modal's scrolling body, where the thing whose whole purpose is being seen could
land below the fold; nothing about the run is written there now. The progress line is its
**own** `dyn_container` inside the footer's, because it ticks once per table and rebuilding the
buttons beside it on each tick would take the focus ring with it. The whole footer is a `dyn_container` keyed on
running / the chosen set / `!folder && DumpUi::options().is_empty()` / the outcome, the shape the
import footer uses and for the same reason: `action_button` takes a plain `bool`, so a state read
while the panel is built is the state at build time, and an `Export` button left enabled after the
last table is unchecked launches an export of nothing. **Only the dump can have nothing to
write**, which is what the `!folder` is doing: the six toggles are what `options().is_empty()`
reads, a folder export does not show them, and asking it there would leave Export disabled by
controls that are not on screen and whose last values came from someone else's dump.
It is painted in the DDL group, so `modals::ddl_modals_up` has to name `ui.dump.target` — the
wrapper's `inset(0)` otherwise resolves against a box that predicate is keeping at zero by zero,
and the modal renders nothing at all. The foreign-key toggle is offered only where
`dump::fk_guard_sql` returns something, and where it doesn't the modal *says* so in a `form_hint`
rather than greying out a control: PostgreSQL's switch is superuser-only, so the honest thing is
a sentence about the constraints section, not a checkbox that fails the restore for most roles.
`watch_connection` closes the modal when the active connection changes under it — the Export
button would otherwise launch against a `conn_id` that is no longer selected — unless a dump is
running.
**Off `whole_ui_gate`'s list, 9 to zero, and the split is `import_view.rs`'s beside it: the
drivers take a ctx, the renderers take the bundle.** `table_picker` touches nothing but the
modal's own signals and takes `DumpUi`, as `dump_options` and `table_row` beneath it already
did; `open_dump`, `run_export`, `run_dump`, `run_files`, `launch_files` and `dump_overlay` take
a `DumpCtx` — `run_export` among them not because it reads anything outside the modal, but
because it routes to one of the two launchers, which do.
`watch_connection` takes `(DumpUi, ConnUi)`, the two bundles it watches, rather than a ctx it
would read two fields of. `DumpCtx` holds eight things and no `Ui` — `dump`, `script` (cleared
on the way in, the two modals sharing one element of the modal-layer tuple), `conn`, `overlay`,
and the `tables`/`run`/`files`/`cancel` closures — and `export_progress_overlay` is deliberately
**not** on it, taking `(ExportUi, Rc<dyn Fn()>)` instead, because it is the grid export's modal
and not this one. Where the ctx is built, and what nearly got left out of it, is under `lib.rs`'s
`whole_ui_gate`. **`edit_ctx()` is the one method on it**, added when the `Export ▸` submenu
started carrying a ctx rather than a `Ui`: it returns `table_designer::edit_ctx(self.conn)`, and
it exists so `conn` can stay private — the submenu needs the dialect and nothing else out of the
registry, and a `ConnUi` handed in beside a ctx that already holds one is two names for one
bundle.
**The grid export's progress modal lives in this file too** (`export_progress_overlay`),
which is a different feature reached from a different surface and is nonetheless this panel's
footer with nothing above it: the same running line, the same red Stop in the same fixed slot,
the same backdrop. Someone who exports a database from the tree and a result from the grid meets
one thing twice, and keeping the two side by side in one file is what makes that survive the next
change to either. **Living in this file is not living in this group**: it is its own entry in
`modal_layer`, above every group and directly below the shared confirm, and it takes its own
term in `modals::modal_backdrop_up` rather than being spoken for by `ddl_modals_up`. It was the
third slot of the tuple element `dump_overlay` and `script_overlay` share — the stack is at
floem's 16-arity limit — on the claim that the three can never be up together, and that claim
was unenforceable by construction; *modals.rs* has the whole argument and what it cost. It is
still the shape most likely to be forgotten of the three: a modal nobody opens
deliberately, raised by an export started somewhere else entirely. Its one line counts against
`ExportTarget::total`, and **the `~` on that figure is conditional**: it is drawn only where
`approx` says the denominator is the catalogue's guess, because a `Fetched` export knows exactly
how many rows it holds and dressing that up as an estimate hedges a number that needs no
hedging. **The outcome then replaces that line in the same slot**, the way the dump's does —
green for a clean write, the error colour for a failure *or* a cancel — and the line beneath it
switches from `Writing <file>` to `File: <file>`, so the panel cannot describe a file still being
written beside a sentence saying it is finished. Its `dyn_container` is keyed on
`(target, done, error)` rather than on `target` alone: the panel has to be rebuilt when the run
ends for the footer to change word and colour, and reading only `target` left a finished export
showing a red Stop over a file that was already written. For one revision it reported nothing at
all, on the argument that the grid's bar told the whole story in one strip; that bar has no
export state any more. The state behind it (`ExportUi`, the single `exit` closure both its exits
run, and who may write into it) is in *Data grid*.
**Its words are the catalogue's** (`dump-*` and `export-progress-*`). The
export modal's: the title through `dump_title` — two whole messages, `dump-title-schema` and
`dump-title`, rather than a namespace spliced in — the save and folder dialogs' window titles
(`dump-sql-dialog`, `dump-folder-dialog` with `$format`), the folder hint, the picker's count
(`dump-all-selected`, `dump-some-selected`), its Select all/None and its three notes, the two
sections' toggles and the hints this file owns, the no-FK-guard hint, Export, the progress line
`dump-progress` through `dump_progress_text` (`$table`, `$index`, `$total`, `$rows`,
`human_count`'s abbreviated figure, and `$count`, the raw number the plural selects on),
*Reading the schema* and the *Replace files* confirm's title. The progress box's: its count
through `export_count_line` — `export-progress-of-about` for the `~` case, `export-progress-of`
(both selecting on `$count`, the total, so a one-row export reads "1 of 1 row" rather than
"rows"), and the plural `export-progress-rows`, whose `$count` is the exact number Fluent chooses the
plural by and `$rows` the abbreviated one shown, since a `9.4k` is no number to choose one with
(`script_view`'s two-argument count, above) — *Starting*, and the destination line,
`export-progress-file` or `export-progress-writing` (`$name`). Stop and Close in both footers
are the shared `dialog-stop`/`dialog-close`. `the_composed_lines_read_as_they_did_in_english`
pins the titles, counts, progress line and destination lines in English. **The outcome
sentences are worded here too**, each by a function taking exactly the arguments of the core
one it mirrors, so no new core type was needed: `dump_done_note` for `dump::done_note`
(`dump-done-tables`, then `i18n::export_note` of `export_said`'s value inside
`dump-done-export` (`{ $note }.`, the full stop core's `done_note` adds), then
`dump-done-missing`/`-left-out`/`-rows-left-out`/`-refused`, joined by a space in code as core
joins them), `dump_cancel_note` for `dump::cancel_note` (`dump-cancelled-partial`, or the
shared `export-cancelled-nothing`), `files_done_note`/`files_cancel_note`/`files_failure_note`
for `export`'s three (`files-*`, with `files_tail` standing in for core's private
`replaced_clause`/`missing_clause`), and `folder_replace_prompt` for `dump`'s
(`dump-replace-prompt`/`-more`), naming `dump::FOLDER_REPLACE_SHOWN` files — `pub` so the eight
is written once. `english_says_every_dump_outcome_as_core_does` and
`english_says_every_folder_outcome_as_core_does` (`core_words_tests`) hold the English equal to
core's, which stays English for the CLI's dump report and core's tests. **Why a run stopped
short arrives typed**, a `dump::ExportFailure`, and `export_failure_text` words it — the
existing `export-failed` and `export-worker-died`, then `export-not-published`,
`export-not-renamed`, `export-nothing-matched` and `export-stopped`, adding no prefix since
each variant's sentence is already whole — the four failures say they failed, and
`NothingMatched` and `Cancelled` say what happened instead — while `Worded`, a refusal the app
wrote before the run
began, passes through as it stands. The result goes into `i18n::export_failure_note` for a dump
and `files_failure_note` for a folder, and `english_says_every_export_failure_as_core_does`
holds every variant equal to core's `Display`, which the CLI prints. The two replay toggles'
hints arrive typed as well — `dump::drop_hint` and `fk_guard_says_drops` — and
`drop_hint_text`/`fk_hint_text` word them from `dump-drop-hint-*` and `dump-fk-hint`/`-drops`,
held by `english_hints_every_replay_toggle_as_core_does`. The grid export's outcome
lines in the progress box are the
grid's, worded the same way (*Data grid*). What stays English: the format names
(`ExportFormat::label`, a `$format` placeable). The save dialog's file-type name was an English
`SQL` once, reasoned as `FileSpec::name` being a `&'static str`; it is `file-sql-filter` through
`tr_static!` now, the message `script_view.rs`'s open dialog uses, so both SQL dialogs name the
type alike.
