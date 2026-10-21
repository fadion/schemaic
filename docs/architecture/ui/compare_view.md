# `ui/compare_view.rs`

`compare_view.rs` — the **schema-compare** overlay (`compare_overlay`), over `core::compare`:
two databases side by side, and the migration between them. Not a tab — there is no tab kind in
this app, `Tab`/`SavedTab` being query tabs — so it is the sixth entry of the modal layer's
**workspace** group, beside the ERD and the binary-cell panel, and counted by
`workspace_modals_up`. `open_compare(o, conn_id, database, table)` is the way in, from a
database row's **Compare with** and a table or view row's (each beside ER diagram in
`overlays.rs`): it returns every signal the modal owns to its opening value through `reset` —
the one door `close` goes through too, since two paths writing their own subsets is what left
`show_same` cleared on open and not on close — and fixes the **left** side, the right one being
the question the modal is open to ask. **From a table or view the comparison is narrowed, not
reduced.** `CompareTarget::table` carries its namespace and name; `open_compare` writes the name
into `compare_query` *after* `reset`, which is what clears the filter for every other opening,
and the landing ticks only that object's differences (below). Both databases are still read
whole, because a table's foreign keys and its triggers' bodies name objects beyond it — only the
opening reading is narrowed, and clearing the filter shows the whole comparison. It decides nothing
about what differs, in what order the statements run, or what a plan withholds — the tree's rows,
each heading's tally, the diff pane's two sides and the plan the footer previews are all reads of
one `SchemaComparison`, and an opinion of its own here would be the second differ the DDL
invariant exists to prevent.
**A kind's heading is named as the schema tree names it** — `CompareKind::group_label`, which
is `ddl::ObjectKind::group_label` (the tree's folder name, moved out of `schema_tree.rs` for
this) wherever the comparison's kind is an object kind, and *Tables*, *Views* and *Triggers*
otherwise — with the count after it, muted. It was the singular noun pluralised with the count
in brackets, *"synonym (1)"*, *"enums (2)"*, beside a tree filing the same objects under
*Synonyms* and *Types* (`a_compare_group_is_named_as_the_tree_names_it`). `CompareKind::label`
stays the expand state's key and `CompareEntry::key`'s prefix; only the heading changed.
**They agree in every language because the heading is drawn as the folder is**:
`compare_kind_group(kind)` calls `i18n::object_kind_group` for an object kind — the tree's own
function, not a second message for the same word — and `compare-group-*` for the three that are
not. Core's words are drawn here rather than translated (the core-words rule under *UI
conventions*): `compare_kind_word(kind)` is `CompareKind::label`'s singular for the diff pane's
heading, while `label` itself keeps core's English as the key above, and
`empty_rows_message(reason)` is `EmptyRows::message`'s sentence
(`english_names_every_compare_kind_as_core_does`, `english_says_every_empty_reason_as_core_does`
— the first listing the kinds by hand, there being no `CompareKind::ALL`, so a new kind is
caught by `compare_kind_word`'s exhaustive `match` instead). The rest of the modal's words are
`compare-*` messages; the tally is one message with all four counts, which
`the_summary_reads_as_it_did` holds to the `format!` it replaced. The foreign-key cycle's
banner (`compare-fk-cycle-note`) and the line under a `Same` row whose stored text differs
(`compare-same-but-stored-differently`) were the last two literals the modal drew, and
`english_says_the_cycle_and_storage_notes_as_before` holds them to the sentences they replaced.
**What the comparison noted is shown above the tree**, one line each, under the banner a
foreign-key cycle takes: `SchemaComparison::remarks` — an alias type's bound rule or default the
target lacks, a source object a case-insensitive target cannot hold beside another — reached the
reader only through a plan's omitted list, and once every object agreed no plan was built, so a
second comparison after Apply read "These two schemas match" with Preview disabled and the note
nowhere (CMP-03). The empty tree says `EmptyRows::AgreesExceptNoted` then, not
`EverythingAgrees` (under `compare.rs`). The banner reads `SchemaComparison::standing()`
rather than the remarks alone, which puts each **held-back** entry's omission first
(`CompareEntry::held_back` — a SQL Server sequence range the target's counter has left, a
table whose partitioning differs, a partitioned SQL Server table the target lacks, a
partition of a held table, or a MariaDB sequence the comparison writes no statement for): that
line is the instruction for the one difference no plan carries, and a comparison holding only
that difference plans nothing, so Preview is never enabled and the omitted list it would have
been read in is never shown.
**Those lines are worded here, and so is the preview's omitted list.** `omission_text`
(`pub(crate)`, for `ddl_preview::preview_of_plan`) words a `compare::PlanOmission` from the
catalogue: an entry's `compare-omitted` (`$what`, `$why`), its object `compare-described` with
the kind through `compare_kind_word`, its reason one of `compare-omitted-needs-source`,
`-unread`, `-no-statement` or a `compare-held-*` message per `HeldBack` — `$sub` selecting
*partition* or *sub-partition*, the range's `$at`, `$min` and `$max` as the server gave them,
`SHOW CREATE SEQUENCE` never translated; and a note's `compare-note-set-aside` (a `$side`
selector) or `compare-note-binding`, whose `$bound` selector says the rule, the default or
both. `english_says_every_omission_as_core_does` holds it equal to `PlanOmission::message()`
over every `OmissionWhy` and `HeldBack` (both values of `sub`) under three kinds, a set-aside
each way round, and each binding — a hand-written list, so a new variant fails the `match` at
compile time but is held to its English only once someone adds it. The CLI keeps the English
(`omission_note`, `notes()`).
**The module names no `Ui` — it is off `whole_ui_gate`'s list, 8 to zero — and the split is
`import_view.rs`'s.** The renderers take `OverlayUi`, being the eight `compare_*` signals and
nothing else: `open_compare`, `reset`, `body_for`, `ready_body` and `filter_bar`. The four that
*act* take a `CompareCtx` — `overlay: OverlayUi`, `conn: ConnUi`, `ddl: DdlUi` and the
`fetch`/`cancel`/`list_dbs` closures, built once in `modals.rs` — and three of those four take it
**by value**, because they return `impl IntoView` and edition 2024's return-position `impl Trait`
captures every lifetime in scope, so a `&CompareCtx` would borrow the caller's local for the life
of the view and not compile; `open_plan_preview` returns `()` and takes the reference. That rule
and the rest of the campaign are under `lib.rs`.
**It has its own fetch, and that is the deliberate divergence from `erd_overlay`.**
`SchemaActions::compare_fetch` (implemented in `main.rs` beside `table_stats`) calls
`Db::fetch_schema` on *both* sides. The diagram instead reads whatever the schema tree already
loaded and says "not loaded yet" otherwise, which would rule this feature out entirely: the
right-hand side is routinely a database on another connection the tree has never introspected.
**It carries a `CancellationToken` even though there is no Stop to offer.** Staleness is still
handled the properties fetch's way — the landing is dropped unless `overlay.compare` still equals
the target it was asked for — but dropping the *answer* goes on paying for the *read*, and this
read is two full `fetch_schema` sweeps across two servers: picking a different right-hand side
three times left three of them running to completion for nobody. Each ask cancels the one it
supersedes — **and `SchemaActions::compare_cancel` is the other canceller, because "each ask" was
all there was.** `compare_fetch` cancels its predecessor on the way *in*, which made every
canceller a new fetch, so closing the modal — the one path that starts nothing — discharged eight
signals and left two full sweeps reading a catalogue nobody would see. The token is the app's, so
the modal asks for the cancellation rather than holding one of its own. The dialect refusal happens **before either round
trip**, through `compare::comparable` (see its entry for why that is by dialect and not by
engine), its refusal worded by `ui::i18n::compare_incomparable` (`compare-incomparable`).
**The five view-state signals are kept apart from the comparison, for the properties modal's
reason** — `compare_selected` (ticked entry keys), `compare_expanded` (open group labels),
`compare_focus` (whose text the diff pane shows), `compare_query`, `compare_show_same`. They
change without the comparison changing, and re-fetching two schemas because someone opened a
group would be the same failure as a failed row count replacing statistics that loaded fine.
`CompareState::Ready` holds an `Rc<SchemaComparison>` for the reason `SchemaState` holds an
`Arc`: a comparison carries a `ChangeSet` per object, and both the tree and the pane read the
signal per render. Two of the five are seeded by the **landing** rather than by the open —
`default_expanded()` opens every kind that differs, and every difference that *can* be applied
arrives ticked, a comparison being opened to migrate the difference rather than to admire it.
**The landing seeds that tick set through
`SchemaComparison::selectable_keys(RowFilter::default())`, not through a filter written in
`main.rs`** — the default filter because none is on screen yet; a comparison raised from one
table seeds through `selectable_keys_of` instead, by identity (under `compare.rs`) — and that
is the same rule
`compare::is_planned` exists for: which differences a plan can carry has to have one spelling,
or the footer's count, the Apply button
and the statements actually built can disagree. The seed was a fourth one,
`differences().filter(|e| !e.needs_source())`, which drops `unplannable()` — so the tree could
open with objects ticked that the footer refuses to count. Nothing diverged then only because
`unplannable` had no reachable producer — it has since (a SQL Server source module the server
would not show, `compare::table_entry`/`routine_entry`) — and it sat in a *third* crate, outside every test
that could have noticed. `the_compare_seed_does_not_respell_the_plannable_predicate` is a source
gate rather than a unit test for exactly that reason — a fifth spelling compiles and passes
every assertion in the workspace — and it fails on `needs_source` or `unplannable` appearing
anywhere in `app/main.rs`'s production code. This module's own `needs_source` is not caught by
it and is not a violation: it draws the tick-box's *absence*, which is the reason the exclusion
exists.
A side is a **connection plus a database** (`CompareSide`/`CompareTarget`), because the two halves
may live on different servers — which is why **the picker asks in two steps, connection then
database**, and that is not decoration. The one-step version is every connection crossed with
`schema.db_nodes`, and it is wrong in both directions: `db_nodes` only ever holds the *active*
connection's list, so the cross product offers another server databases it may not hold and
cannot offer one only that server has, which is the comparison this feature exists for. Picking
a connection asks *it* what it has instead — `SchemaActions::compare_list_dbs` →
`Db::fetch_databases`, filtered through `schema::db_visible` against
**`db_hidden::names_for(rules, conn_id)`**, because a database the user hid has no more business
in a picker than in the tree — and the answer lands in
`compare_dbs`/`compare_dbs_err`, held apart from `compare_state` for the same reason the five
above are: a picker whose list failed must not blank a comparison that loaded fine. Step two
carries a `‹ another connection` link back, and `db_nodes` is not read by this module at all.
**Whose hidden set that is, is the whole point of the spelling.** `ui.schema.hidden_dbs` is a
memo over the **active** connection, so filtering another server's list through it hid databases
that server really has, offered ones the user had hidden on it, and — on SQLite, where the list
is the single `main` — emptied every other SQLite connection's list outright. `names_for` is
handed the rules and the id of the connection being *listed*, which is the same distinction
`db_hidden`'s own entry exists to keep.
**That listing is guarded against two clicks landing out of order, in both arms**, and it had a
guard in neither: a listing the user abandoned — a second server clicked while the first was
still out — landed late, its *success* arm replacing the picker under the pointer so that a click
already on its way retargeted to a different server's database, and its *failure* arm replacing a
fully rendered comparison with an error about a server the user had moved on from. Each ask takes
the next generation and the landing returns early unless that is still the current one. The
second defence is unchanged and still load-bearing: the connection id travels *with the list* —
`compare_dbs` holds `(u64, Vec<String>)` and step two builds its `CompareSide` from that pair,
not from whichever row was clicked — so even a stale list is picked against the connection it
came from. Capturing the clicked id in the row's closure instead would cost nothing visible and
turn a stale list into a comparison run against the wrong server.
**A no-op `set` is not free on either of this modal's keyed signals.** Clearing `compare_dbs_err`
asks `is_some` first, because the modal's body is a `dyn_container` keyed partly on it, so
writing `None` over `None` disposed and rebuilt the filter bar, the whole row tree and the diff
pane — losing the tree's scroll position for a listing that had not even been sent yet. The row
click guards `compare_focus` the same way, where a rebuild costs a fresh `line_diff` over both
sides' DDL and a restyle of every row.
**The MySQL bodies are shown, readable, and cannot be ticked**, which is the limitation most
worth knowing here. `CompareEntry::needs_source` is true of a trigger, routine or event whose
body the eager `fetch_schema` read out of `information_schema` with its escapes already
resolved — so the *verdict* is right and a `CREATE` built from it is not. Such a row carries a
warning triangle where its tick would be, plus the sentence "body must be re-read before this can
be applied"; **Select all** goes through `SchemaComparison::selectable_keys`, which filters the
same way (and by the query too — see its entry), the app's seed above filters the same way, and
**`plan_of` is `c.plan(|e| is_planned(e, selected))`** — one predicate, `compare::is_planned`,
which now lives in the core crate rather than here: it was the single thing deciding which
objects reach an irreversible `Db::run_ddl`, sitting untestable in this module while `plan`
already took it as a parameter. `SchemaComparison::plan` re-applies the same exclusion on its own
account, so a tick that arrived by some other route (a comparison replaced under a stale
selection, a future "invert") cannot reach `emit` however the caller's predicate answers.
Counting over a laxer predicate than the one that builds the plan is how a confident "1 object"
comes to sit over a button that builds an empty plan and returns. **And the preview now says so
out loud**: what is kept out travels as `SchemaPlan::omissions` into the modal's own block, where
it discloses without refusing — before that, the last surface ahead of Apply counted only what
was included and reported "Applied N statements to 1 object" over a three-difference comparison.
Lifting the block means re-reading each body through `Db::{trigger,routine,event}_source` first —
open work, not something to fake in the meantime. **A held-back entry is drawn the same way** —
the row's `blocked` is `needs_source() || held_back.is_some()` — with the warning triangle where
its tick would be and "held back from the plan; the note above says why" as its hint, the
sentence itself standing in the banner (above). Unlike a MySQL body there is nothing to lift
here: the user restarts the sequence, recreates the repartitioned table, or creates the
partition function, scheme and table, on the target and compares again.
**Preview never applies.** `open_plan_preview` builds the `SchemaPlan`, reads the left
connection's `read_only` off the connection list, names it through `plan_subject_text` —
`SchemaPlan::subject_in`'s count and the database it lands in, worded from the plural
`compare-plan-subject` (`english_names_the_plan_s_subject_as_core_does`) — and hands it to
`ddl_preview::preview_of_plan` — the same modal, the same Apply and the same `Db::run_ddl` a
one-table designer edit takes. The comparison closes on the way there, so the plan is a snapshot
rather than a selection the preview no longer reflects.
**The footer counts objects, not statements, and it never builds a plan to do it.** The memo ran
`plan_of` and then `emit()` on every checkbox tick — cloning a `ChangeSet` per selected object
and rendering the whole migration's SQL synchronously on the UI thread to display a number. It
now counts the entries the predicate accepts, reading the selection with `with` so the `HashSet`
is not cloned, and the sentence beside the button is `compare::selection_note(planned)`, said
in the interface's language by `selection_note_text` (`compare-selected`, held to it by
`english_counts_the_selection_as_core_does`) — the same plural rule as the preview's subject,
one function away rather than hand-rolled a hundred lines from it; the statement count lives in the
preview, which is the only place it can be had without emitting. The button is keyed on a
separate `Memo<bool>` rather than on the count, because `dyn_container` has no equality check of
its own and was tearing the button — and with it its focus-ring registration — down on every tick
instead of on the two transitions that matter. It registers in **`ACTION_TAB`**, the band every
modal's footer actions take, so a body that later grows `VALUE_TAB`-based rows cannot overtake it.
**A foreign-key cycle is said twice on purpose.** A warning strip sits above the tree as well as
reaching the preview's risk block through `SchemaPlan::cycles`, because the tree is where someone
decides what to tick and the risk block is read after that decision is made. The strip asks
`SchemaComparison::cycles()` — the "either" of the two halves, not the plan's narrowed pairing —
because it is drawn before there is a selection to narrow by.
**The chrome is this module's own.** `schema_tree`'s `tree_row`/`chevron`/`tree_row_min_w` are
module-private and nothing was promoted to share them. Status is one glyph per row (`+` arrives,
`−` goes, `~` changes, `=` agrees) so a long list reads down a column instead of as a wall of
words, and the diff pane is one `diff::line_diff` over `CompareEntry::{left_ddl,right_ddl}`. The
filter bar's summary line carries `CompareCounts::uncertain` beside the four status tallies,
because it is the one line about the whole comparison: the per-row hint is still there, but an
uncertain match is overwhelmingly an object that came out `Same`, which the default hides. The
right-side picker is an **inline expand-in-place list rather than the popup menu**, because
`menu_panel` anchors itself to a measured rect and this control sits in a fixed header — the
import modal's sheet picker is the precedent.
**An empty tree says *which* emptiness it is, and it is chosen inside the container that tracks
the filter** rather than once above it — but the choosing is `SchemaComparison::empty_reason`'s
now, not this module's. Four cases are reachable and they read differently — a filter that
matched nothing, a filter that matched only objects the two schemas agree on while *Include
identical* is off, two schemas that agree, and two databases with nothing in them — and "nothing
matches that filter" over an unfiltered identical pair is the wrong answer to the question asked,
while "these match" over a failed filter is worse: it claims agreement the comparison never
checked. The second of the four was the one this module got wrong while the arms lived here,
inside a `dyn_container`'s child closure where no test could reach them.
The message was an early return *outside* that container, which also
made **Identical too** dead in an all-identical comparison, the tick having nothing left to
re-render. The picker's failure line is the same lesson one level up: it renders **above**
whichever step is showing rather than instead of one, because an error arm of its own replaced
the connection rows that were the only control able to clear `compare_dbs_err` — a listing that
failed could then be escaped only by closing the whole modal.
