# `cli/dump.rs`

`cli/dump.rs` — the I/O half of `core::dump`: introspect, plan, write — and **the one driver
both front ends call**, the app's *Export ▸ SQL dump* modal and `schemaic dump`. It was
`app/dump.rs` until `schemaic dump` needed it, and moved here so that the CLI calls the same
driver rather than a copy of it; the app reaches it as `schemaic_cli::dump`. The request and outcome types moved with it, out
of `ui/lib.rs` and into `core::dump` — `DumpRequest`, `FilesRequest` and its `approved`,
`FilesOutcome`, `DumpProgress`, `DumpOutcome` — so the driver and its two callers share one
vocabulary; `schemaic-ui` re-exports them with `pub use`, and every `schemaic_ui::` path still
resolves. Its shape is the app's `export_file`'s
`AllRows` branch with one difference that drives the whole module — an export is one statement
into one file and a dump is **many**, so the writer has to outlive each table. A single blocking
task owns the file and reads `Msg`s: a `Text` is written as it arrives, and a `Table` carries the
*receiving end* of that table's row channel, which the writer drains through
`dump::render_rows`. That shares `export::export_inserts_ending` with the grid's SQL export, so
on MySQL, PostgreSQL and SQLite a dump's `INSERT`s and the export's are the same statements;
on a batch-separated engine they are not — on SQL Server each `INSERT` closes its own `GO`
batch, and binary, CLR and variant columns are written from the literals the server rendered
(`DumpStep::Rows::server`), neither of which the grid's export writes. Rows are
read at `EXPORT_CHUNK_ROWS` with the same bound of two blocks in flight, and the `.part`-sibling
+ atomic-rename guarantee is the export's unchanged — `part_of` builds that sibling through
`export::part_file`, which is `export::part_path` — the one function that decides the suffix —
over the file name, and which `schemaic export` builds its file under too, because the modal's cancel
sentence names the same fragment through it and two spellings of `.part` could drift apart in
exactly the situation where the fragment is the thing the user still wants. **So `part_of` is
`pub` — the app's `export_file` reaches it as `schemaic_cli::dump::part_of` — and is the only
speller of it in either crate.** `main.rs`'s `export_file` had an
inline closure appending the literal, and that is the site that matters most: it serves both
export scopes and all five grid formats, and its cancel and failure arms are the ones that name
the fragment — a change to `part_path` (a dot prefix to hide it, a timestamp so two exports
cannot collide) would have pointed both sentences at a path that does not exist, on the one
path where the fragment is the only copy of the user's rows.
`nothing_in_this_crate_spells_the_fragment_suffix_itself` scans `schemaic-cli/src` for the
literal and answers with the call to make instead — **and walks `../schemaic-app/src` as well**,
because the site that matters most stayed behind when the driver moved, and a gate over this
crate alone would have lost it without a sound; comment lines are dropped before the scan,
so the several paragraphs that discuss the sibling are not hits. Unchanged too is
**cancel is the reader's to declare
and every other failure the writer's to describe** — a cancelled read closes the channels, which
the writer sees as an ordinary end of stream and would otherwise call a truncated file finished.
That five-arm resolution is now `core::dump::dump_verdict`, with tests: written out here it sat
inside an `async fn` needing a `Db`, a runtime handle and two channels to reach, so swapping two
arms turned *The disk is full* into *connection reset* with the suite still green.
**`DumpVerdict::Cancelled` carries `partial` as well, and `DumpOutcome::Cancelled` with it.** A
cancel during the schema read returns before `part_of` is computed and before the writer is
spawned, so no `shop.sql.part` exists anywhere — while the modal said *what had been written is
in shop.sql.part* regardless and sent the user to look for a file that was never opened. That is
the identical false claim already removed from the `Failed` arm one line over, which this enum
had not followed. The writer stays the only thing that knows, so the cancel arm reads
`WriteEnd::Failed`'s `opened` rather than assuming, and the sentence is `dump::cancel_note`,
which takes `partial` — worded in the modal by `dump_view`'s `dump_cancel_note`.
**One check the export path has no need of**: after the join, `token.is_cancelled()` is asked
directly, because a cancel that arrives while no table is streaming — anywhere in a
structure-only dump — never reaches the reader's error, and the writer's refusal to
publish surfaces as an *error*, so a stopped dump would be reported as a failed one. **The schema
is freshly introspected, never the tree's cache**: a dump is a backup, and a `CREATE TABLE` for a
shape the server no longer has is a backup that restores the wrong table. That read takes the
run's own token (`Db::fetch_schema`'s entry has why), so the `Err(DbError::Cancelled)` arm ahead
of the plan is reached rather than being the dead code it was while Stop could not touch the
longest phase of a large dump. **Its row reads run on a read-only session, whatever the
connection** — both `stream_query_enforced(…, Some(Enforce::ReadOnly))`, the `.sql` dump's and
`run_files`' — because the `SELECT`s are generated, so no text gate reads them, and a name they
read may be a view whose body calls a function that writes: `core::dump::file_plan` reads every
chosen view's rows, so a folder export on an ordinary session ran that write, on a read-only
connection too (security review S8). The `.sql` dump streams base tables only (a view is structure
and no rows there) and takes the same session regardless, since a dump only reads and on a
writable connection the session costs nothing it was meant to do. That half is pinned in source:
`ui/source_gate.rs`'s `every_headless_read_runs_on_a_read_only_session` reads this file beside
`query.rs` and `mcp.rs`, and was seen red against the plain `stream_query` it replaced. The two counts in the
report are deliberately different figures and must not be made to agree — progress counts the
`DumpStep::Rows` steps (`DumpPlan::streamed_tables`), since a view has no rows of its own and a
structure-only dump streams nothing at all, so counting tables would promise a "12 of 12" that
never arrives, while `DumpOutcome::Done`'s `tables` is `DumpPlan::tables`, what the file actually
covers, and its `missing` is what the fresh introspection could not find at all. Its
`refused` is `DumpPlan::refused`, what a replay of the file will not replace, its `left_out`
what the file leaves out though the export asked for it, and its `rows_left_out` the graph
edges created without their rows; the sentence is `dump::done_note` (the modal's is
`dump_view::dump_done_note`, its English held equal), each part a
sentence of its own with one space between — the `format!` it replaced put a space after the
tally whatever followed, so a quiet dump read "Wrote 2 tables. " and a short one had two
spaces in it (`the_done_note_says_what_the_file_is_short_of_in_single_spaced_sentences`).
**The export's caveat is ended here, not by `export_note`**, which writes no full stop because
its single-file caller is the last clause of a sentence — `export::files_note` already ended it,
`done_note` did not, and the next sentence ran on into it: "…those cells are empty 1 ticked
table not found…". The old test's fixture note carried its own full stop, which is how it hid;
`the_export_note_is_a_sentence_of_its_own_in_the_done_note` asks of `export_note`'s real output,
and the CLI's dump report, which calls `done_note`, is mended with it.
**Beside it rides an `ExportTally`, not a row count.** `write` folds each table's tally into one
through `ExportTally::absorb` — rows summed, and a withheld or blanked column named once however
many tables it appears in, the rule `ExportTally::note` already follows within a single export —
because the driver used to
discard `withheld`/`blanked` and an export that wrote every BLOB as `NULL` came back as a green
*Wrote 5 tables and 115k rows.* What the file could **not** carry is the difference between a
backup and something that looks like one, so it has to survive the trip back.
**Progress leaves the driver through a callback, `dump::Progress`** — `Box<dyn Fn(DumpProgress)
+ Send + Sync>`, called on the run's task as each table starts, so it must return at once —
rather than the crossbeam `Sender` it took while it lived in the app, because its two callers
want different things of it: `schemaic dump` prints `i of n — table` to stderr, and the app
wraps its channel's sender in one. **The channel is the app's half and stays there**: progress
reaches the modal as a **crossbeam channel + `create_signal_from_channel`** rather than a
callback into the view, for the reason the AI stream and `update.rs`'s download do:
`create_ext_action` is one-shot and has to be built on the UI thread, so a worker reporting
*repeatedly* needs a signal fed from a channel. **In the app, the dump owns its own cancel slot**
(`dump_token`) rather than sharing
`export_token`: the *grid* export's slot exists to stop two result-set exports competing for one
disk, while a dump can only be launched from a modal that already refuses a second one, and
folding the two together would let a running result-set export's Cancel stop a dump the user
cannot even see. (Both features are called Export in the interface, which is why that sentence
names which one it means; `core::dump`'s entry has the naming split in full.) **The folder
export shares that slot rather than taking a third**, which is the opposite call and rests on the
same fact: `files_run` and `dump_run` are launched from the one modal, which refuses a second run
while either is going, so the two can never be in flight together — and separate slots would only
create the case where the footer's Stop reaches neither. Its progress rides the dump's channel
for the same reason: it is the same footer line counting the same `3 of 12`. **This module
writes no failure sentence**: why a run stopped short is a `core::dump::ExportFailure`, and the
two front ends word it — the modal through `dump_view`'s `export_failure_text`, the CLI by
printing its `Display`. `WriteFail.message` holds one, its `From<ExportFailure>` (until phase 4
of *Interface translation*, `From<String>`) still converting every later `?` with
`opened: true`, and the private `failed_with(e)` is the one spelling of
`ExportFailure::Failed(e.to_string())` behind every `map_err` but the two renames', so
*Export failed:* is spelled
nowhere here — it is `ExportFailure`'s `Display` and the catalogue's `export-failed`. A plan
with no steps is `NothingMatched`, a stop caught just before a rename `Cancelled`, a table file
that cannot be renamed into place `NotPublished`, and a dump's `.part` that cannot be renamed
over its destination `NotRenamed`.
**`run_files` is the file-per-table sibling of `run`**, over `core::dump::file_plan`, and it
keeps every one of the guarantees above **per file**: a freshly introspected schema, rows
streamed rather than gathered, a `.part` sibling (`write_one`) renamed over the destination only
once that table is whole, the token checked *before* that rename and asked again after the join
— a stop that lands between the last chunk and the publish never reaches the reader at all, so
the writer's refusal would otherwise be reported as a failure — and a cancelled read raised as a
channel *error* rather than read as end of stream. **The `.part` sweep here is unconditional,
which is where a folder export parts company with the single-file one**: there the fragment is
the only trace of the rows that arrived and `export_cancel_note` points the user straight at it,
so removing it would destroy the one thing they might still want; here the finished files *are*
that trace — whole, published, and exactly what `files_cancel_note` and `files_failure_note`
promise — and a fragment left beside them makes both sentences false and drops an unreadable
`orders.csv.part` into a folder the user chose for output and is about to go looking through.
(The single-file paths do gate their sweep on `writes_incrementally()`, and this paragraph used
to claim the same for `run_files`.) **The loop is sequential, one table at a time, on purpose**:
writing four at once would need four connections and four sets of blocks in flight, and it would
make the progress line's `3 of 12` a lie about what is happening — a folder export is disk-bound
at the end anyway. The per-file tallies are folded with `ExportTally::absorb`, the same fold the
single file uses, so one sentence can name what none of them could carry. `FilesOutcome` is its
own type rather than `DumpOutcome` because all three of its endings count **files in a folder**,
which is the one thing the dump's three never have to say — and all three now carry
`replaced: Vec<String>` as well. Its fourth arm, `WouldReplace`, is not an ending at all: it is
the question below. `schemaic dump` does not call `run_files`; the folder export is the app's
alone.
**What the export is about to overwrite is collected before it overwrites any of it, and it is
the guard as well as the report.** A pre-flight walks the plan's file names against the chosen
directory and keeps the ones already there, once, ahead of the loop: after the first `rename` the
answer is contaminated by this export's own output, and a `Cancelled` or `Failed` arm would
report whichever prefix it happened to reach. `select_directories()` has no overwrite prompt, so
this is the folder form's only equivalent of the save dialog's "replace?" — and for one range it
was a disclosure rather than a refusal, which meant the files were replaced and then named. Now
`core::dump::colliding_files` is the census (the one line here that touches a filesystem is the
`req.folder.join(f).is_file()` closure it takes) and `folder_verdict` is the decision: an
unapproved run with collisions returns `FilesOutcome::WouldReplace` **before the first
`rename`**, so the folder is untouched, and the modal asks. **Which list is the consent is
pinned in source, not here**: both of `folder_verdict`'s parameters are lists of file names, so
`folder_verdict(Some(&replaced), &replaced)` compiles, always answers `Write`, and leaves every
one of `core::dump`'s cases green. `the_folder_export_consents_to_the_list_the_user_was_shown`
asserts the call is made with `req.approved.as_deref()` and that the census is the
`colliding_files` reading of the folder; it was this module's own test while the driver lived
in the app, and is `ui/source_gate.rs`'s `folder_export_consent_gate` now, reading this file by
its workspace path, because it needs that module's comment-stripping reader and this crate is
Floem-free. The list still reaches all three
reporting arms as well — `export::files_note`/`files_cancel_note`/`files_failure_note` take it
and `replaced_clause` renders it — because an approved run has still destroyed something worth
naming. **What they take is `core::dump::destroyed(&replaced, &published, &attempted)`, not the
census.** The
census has to be read before the first rename or this export's own output contaminates it, but
only the *finished* arm is reached with the loop complete, and the whole-plan list went verbatim
to all three — so pressing Stop while the first table was still streaming reported *"Export
cancelled — no file was finished, so nothing was written to out. 3 existing files were replaced:
orders.csv, items.csv, users.csv."* Two flatly contradictory sentences, of which the false one is
also the **only** disclosure that a folder export destroys anything, wrong in the direction that
sends a user looking for a backup they do not need. **`attempted` is the third argument because a
`.part` dies at the *start* of a table's attempt, not at its end**: the rename that publishes
`orders.csv` consumes `orders.csv.part`, so publishing is one way to destroy it — but the writer
truncates the fragment with `File::create` before it writes a byte, and both the cancel and the
failure arms then sweep it. A table whose retry was stopped or failed therefore destroyed the
fragment an earlier run had left, and nothing named it, `published` holding only the tables that
finished — so the one file `export_failure_note` had just taught the user to care about was the
one the report was silent about. `published` is a subset of `attempted`, so the published-sibling
rule is the same rule; it stays a term of its own because a published file is destroyed under its
*own* name as well as its fragment's. A fragment whose table the export never reached is still
sitting there and is still not named, which is unchanged.
**The per-file resolution is `core::dump::dump_verdict` too, not a second copy of it.** Those
five arms were written out again here and the copy diverged in the one arm the extraction exists
to protect: `WriteEnd::Failed` carries the writer's own failure, which already says
*"Export failed:"*, and re-prefixing it produced *"Export failed: Export failed: No space left
on device (os error 28) — 3 files already written to out are kept."* That failure is an
`ExportFailure` now rather than the writer's text, and the rule is its type's: every variant
already says it failed, so nothing that shows one adds a prefix. The writer's refusal to
publish a truncated file is folded into the **read** end before asking, because that is what it
is — a cancel whose only witness is the token, a stop that landed between the last chunk and the
rename never reaching the reader at all. And `write_one`'s rename failure no longer names the
`.part` file: the caller's `sweep()` deletes it three lines later, so the one sentence telling
the user where their rows went named a path that no longer existed by the time they read it. It
names the destination instead (`ExportFailure::NotPublished`, whose `path` is it).
