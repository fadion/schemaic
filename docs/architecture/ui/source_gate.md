# `ui/source_gate.rs`

`source_gate.rs` — the shared machinery behind the crate's *source gates*:
the tests that read the crate's own `.rs` files and fail on a spelling production code must not
contain (a floem `Dropdown`, a captured `Color`, a raw pixel inset, an unguarded `exec_after`,
a menu trigger that doesn't close its siblings, a document edit that doesn't ask whether the
editor is frozen — `editor_pane`'s `editor_freeze_gate`, two tests, whose subject is under
*Floem 0.2 gotchas* — a text box whose `max_width` is a bare float while the `font_size` beside
it scales (`lib.rs`'s `scaled_text_width_gate`, whose subject is under *Architecture
invariants*, with the empty allowlist it should keep), an unscaled `const` length in
`consts.rs` that says nowhere why (`consts::unscaled_const_gate`), a menu label ending in an
ellipsis (`no_menu_label_ends_in_an_ellipsis`, whose subject is under *UI conventions*), a
behaviour keyed on an English label (`nothing_is_found_by_its_english_label`, likewise), a
pointer cursor outside a hyperlink (`no_pointer_cursor_outside_a_hyperlink`, likewise), a
connection form that decides its own trimming instead of going through `Connection::trimmed` (`lib.rs`'s
`the_connection_form_trims_through_core`, which is a gate because `to_connection` reads a couple
of dozen signals and needs a Floem scope) — and an engine comparison with no capability behind it,
`lib.rs`'s
`engine_comparison_gate`, which is a per-file budget with a written reason rather than a ban and
whose subject is under *Architecture invariants*; and more of the root `Ui` bundle than the file
already took — `lib.rs`'s `whole_ui_gate`, a per-file budget too, written up under `lib.rs`
below). The captured-`Color` member is `theme::color_arg_gate`, and it holds only the
*parameter* half of its invariant — see that rule for why the rest is not greppable at all.
**One of the family fails on a spelling that is *missing* rather than present**, and it reads
differently for that reason: `lib.rs`'s `read_only_door_gate` finds every editor door by
the stamp it writes and asserts the refusal appears *above* it in
the same `fn`, so what it holds is an ordering rather than an absence. The stamp is a
`SUBJECTS` list of `(stamp, refusal)` pairs — one per subject the flag can be about — rather
than the single literal it started as, since that literal made "the door asks the switcher" part
of the definition of a door and so **failed** a door written to ask its target; each subject
must still have at least one user, so a spelling that stopped matching cannot hide behind the
other's count. It needs the same two
halves regardless — the whole file scanned, and both view crates enumerated — and it carries the
other pattern this module made possible, an exemption list holding its reason as data
(`ddl_preview::PlanTarget`, the one literal that carries the flag without being an editor
target) with an assertion that the exemption is still *needed*, so a stale entry fails rather
than accumulating. **That list is keyed on the struct literal and used to be keyed on the
file** — and the file was `overlays.rs`, the launch point for every editor door in the app, so
the skip covered far more than the two `PlanTarget`s it was written for and made this gate's own
claim (a new editor caught the moment it is written) false exactly where it mattered.
**`widgets::focus_root_gate::every_modal_root_publishes_a_ring` is the second of that shape**:
it finds every bare `focus_root(` across both view crates and fails unless the site is a
`focus_root_with_ring`, naming the offender by its enclosing `fn` — a modal root with no ring is
a modal whose Tab does nothing at all, which is how the error modal shipped. Its `EXEMPT` list is
`(file, enclosing fn, why)` and holds the seven roots that legitimately have no ring, every one
of them a popup driving its own arrows, Enter and Escape.
**`widgets::menu_return_gate::every_navigable_view_reports_the_press_that_focuses_it` is the
third**, and it is the shape's limit case: it holds every `.keyboard_navigable()` in both view
crates to a `takes_pointer_focus` in the preceding 600 bytes, and it cannot see the views floem
focuses *without* one — the SQL editor and `edit_field`'s editor (the grid's inline cell editor
among them, since it stopped being floem's `text_input`) carry no such call for a needle to
match, so those two are a census kept by hand in `takes_pointer_focus`'s doc and the test says
so rather than reading as broader than it is.
**`lib.rs`'s `persisted_store_gate` was the fourth, and it is deleted — a type made it
unnecessary.** It held each of four core mutators (`db_color::upsert(`,
`db_color::table_upsert(`, `favorite::toggle(`, `format::upsert(`) to a `persist::Saving::`
somewhere in the following 700 bytes, with a count of six sites to keep a renamed needle from
passing vacuously, because the stores `app/ui_stores.rs` loads are mutated only in this crate and
forgetting the save is invisible until a restart: the signal updates, the dot appears on the
node, the star turns gold, and the colour is simply gone the next launch, with no error, no
failed write and nothing in the log. Those stores are `stored::Stored` now, whose only writers
save, so there is no save left to forget and nothing for a needle to find — `stored.rs` below
has the rest. **Its one lesson outlives it**: `format.json`'s writer in `grid.rs` was exempted
from it on a reason that was false of the code — the doc said that write went "through
`GridState::fmt_rules` rather than a `format::` mutator", while the code called
`format::upsert(` character for character like the other needles, its save three lines later —
and while the exemption stood, deleting that save left the suite green with `ui_stores.rs`
claiming all three of its stores were guarded. An exemption is a claim about the code, and this
one had never been checked against it.
**`lib.rs`'s `diagram_layout_gate::diagram_layouts_are_loaded_through_the_one_reporting_door` is
the fifth**, and it is what a rule that could only be *counted* looks like once the code has been
reshaped to make it greppable. Its subject is under `core/persist.rs`: every lazy `diagrams.json`
load must report what the read recovered, and the gate it replaced lived in `schemaic-app` and
could only assert that the two crates hold more reporters than loads, globally. With
`load_diagram_layouts` the one reader, the check becomes an exact site list — every
`load_json("diagrams.json")` over `workspace_sources()`, asserted equal to `["lib.rs"]` — so a
failure names the file that added a second reader instead of reporting a number that is one too
low. It moved into this crate because that is where the door is, and its needle is assembled from
fragments (`["load_json", "(\"diagrams", ".json\")"].concat()`) for this family's standing
reason: a gate in the same file as its subject is otherwise its own first match. **It is two
tests, because one door does not make the reporting true by construction**:
`the_one_door_reports_before_it_returns` reads `load_diagram_layouts`' own body and requires the
load, then `report_recoveries`, then the return. The old gate asserted both `loads >= 3` and
`reports > loads`; only the counting half was carried across, and deleting the report from the
door leaves the site list exactly `["lib.rs"]` with the whole failure back.
`production_code` blanks every
`#[cfg(test)]` **item** — brace-aware, skipping braces inside strings, chars and comments — and
every `//` line; `crate_sources` enumerates the files to scan.
**`let_regions(body, bindings)` is the module's second public function, and it exists because
counting was not enough.** `app/history_store.rs` and `app/snippet_store.rs` each hold a `wire()`
full of closures with a gate asserting the erase policy — a `remove` saves `Erasing` so the
deleted row does not survive in `<file>.json.bak`, an ordinary edit saves `Replacing` — and both
gates counted *occurrences* over the whole body. A count holds just as well when two verbs are
**swapped**: give `remove` the `Replacing` and `record` the `Erasing` and every total is
identical, while a deleted run survives in `history.json.bak` and a deleted snippet's body in
`snippets.json.bak`, under a modal saying it cannot be undone. So the caller names the bindings
it expects and gets each one's text back in source order, cut at the next binding, and asserts
about the closure rather than about the file. A binding is matched as `let <name>` at a line
start with the identifier ending there, so `let record` is not `let record_use` and a mention
inside another closure's body is not a boundary; and **every name must be found**, the function
returning `None` rather than yielding one region for the rest of the function, which is the
failure mode a `split_once` has and the way a rename would otherwise make the gate weaker
without making it red.
**`inside_a_closure(code, at)` is the third shared helper, and it replaced a heuristic that was
standing in for the question.** The two "this read sits in a tracking scope" gates —
`grid`'s server-sort one and `editor_pane`'s cmdk-preview one, both written up under *Floem 0.2
gotchas* — asked whether `move |` appeared anywhere in the three lines above the read, and the
single commonest three-line neighbour in either file is an unrelated `.style(move |s| …)`, which
satisfies that while the read sits inside nothing. Both gates' failure *messages* had stated the
real question all along. This answers it: find the nearest `move |` above, step past the
parameter list and walk the brackets forward, so a closure that has already closed takes the
depth below zero before the read is reached. A closure whose body is a bare expression ending at
a `,` rather than a bracket is not detected as closed — narrower than the truth in that one
direction, and still strictly stronger than counting lines. Callers hand it `production_code`,
so comments are already gone; a bracket inside a string literal would still be counted, and
neither region holds one. It is `#[cfg(test)]`, unlike `production_code` and beside the corpus
walkers, because both callers are gates in this crate and an unconditional one is a dead-code
warning under CI's `-D warnings`.
**The cut is compiled into the library, and it has to be.** A `#[cfg(test)] mod` is invisible to
a *different* crate's tests, which is how `schemaic-app` came to carry a twelfth private copy
holding both defects this one exists to remove — so `production_code` and the byte scanners under
it are unconditional **public API** of `schemaic-ui`, rendered by `cargo doc` and held to
`RUSTDOCFLAGS=-D warnings`. What is test-only is the corpus walkers (`crate_sources`,
`workspace_sources`), which read the source tree at paths derived from `CARGO_MANIFEST_DIR` and
mean nothing at runtime. Nothing here is *called* from the app — a handful of pure string
functions a linker with `--gc-sections` drops — which is a different claim from "not compiled",
and three places in this module said the second one for a while after it had stopped being true.
They were **corrected rather than made true**: the alternative to exporting the walk is another
copy of it. Both halves exist because the
idiom was written out eleven times across nine files and every copy had the same two holes. It cut each
file at the **first** `#[cfg(test)]`, which is right only for a file whose tests are all at the
bottom: `widgets.rs` has an inline test-only `fn` a tenth of the way in, so its gates read 929
of 7,259 lines and a planted `Dropdown` at line 2000 passed. And it walked
`env!("CARGO_MANIFEST_DIR")/src` — `schemaic-ui` alone — while the invariants the gates enforce
are stated app-wide, so a violation added to `schemaic-app` (which builds views too, in
`app_view`) passed the whole suite. A copy each also meant that fixing either hole reached one
gate of eleven.
**The cut *blanks* rather than deletes, so a line number means something.** It used to remove
the lines, and the result then bore no relation at all to the file's own numbering: measured
over the tree, `widgets.rs` went 7,881 source lines → 2,990, `lib.rs` 11,402 → 5,811,
`shortcuts.rs` 727 → 113. Two gates print a position from that text as if it were a file
position, so a violation at `widgets.rs:6500` was reported at roughly `widgets.rs:2500` and the
reader opened the wrong function — and `popup_anchor_gate`'s exemption array was *keyed* on that
number. Every gate scans with `contains`/`find`, so the blank lines cost nothing and the numbers
are right by construction, for any future gate as well as these two.
`stripping_a_file_keeps_its_line_numbering` asserts it over every file of this crate's own `src`
rather than over a fixture, because the property is about what the gates are actually handed,
and every one of them fails against the old helper. It is also what caught the off-by-one the
obvious fix leaves behind: a file whose last line falls inside a cut test module ends on an
empty one, and `str::lines()` drops a trailing empty segment, so the round trip has to split on
the newline rather than iterate lines.
Two details in there are load-bearing and read as tidiable. **Not every `#[cfg(test)]` sits on a
block**: it can be on a `use` (ends at `;`), on a struct field, an enum variant or a match arm
(ends at the `,`, or at the enclosing `}` when it is the last member), so `item_end` reading only
`{`/`}` made that last case decrement past zero and panic with *attempt to subtract with
overflow* — taking all eleven gates, and so the whole suite, down with a message naming neither
the file nor the construct. And `(` and `[` are counted alongside `{` for one reason: without
them a `,` at "depth 0" lands inside `fn f(a: u32, b: u32)` and hands the body of a test-only
function back as production code. An unbalanced file is refused rather than guessed at — the rest
is scanned, because a false positive fails loudly and a silent truncation is the failure this
module exists to end. `crate_sources` closes the same hole at the other end with a floor on how
many files it found: a moved or renamed `src` would otherwise pass every gate by finding no
files at all. **That floor is near the real count rather than a token one.** It was
`out.len() > 20` against **65** files, so two thirds of the corpus could vanish and every gate
over it would still report green — the same shape as the floors those gates carry, one level up.
It is `>= 60` now, with `the_scan_reaches_both_crates_that_build_views` (which `.expect`s
`lib.rs` and `schemaic-app/main.rs` by name) doing the precise half. **And the walk is recursive,
which is a fix rather than a refinement.** It was one non-recursive `read_dir` per crate, under a
comment saying a future `src/<subdir>/*.rs` "would fall outside every gate silently — this floor
is what would notice". It would not: the floor is a *lower* bound on the whole corpus, so moving
six files out of a flat directory into a subdirectory leaves the count at 65 minus 6 and every
gate over those six blind, with nothing red. `collect_rs` does the walk — sorted, so a gate that
reports the first offender names the same file on every machine where `read_dir` order is the
filesystem's, and joining the relative path with `/` on every platform so a name a gate reports or
matches on reads the same on Windows and Linux. Both view crates are flat today; the point is that
they no longer have to be.
**Four gates spelled the walk themselves and have been pointed at `crate_sources` too** —
`consts::float_inset_gate`, `modals::modal_backdrop_gate`, `widgets::popup_anchor_gate` and
`widgets::menu_trigger_gate`'s `crate_source`, the last of which also put text `crate_sources`
had already stripped through `production_code` a second time. All four repeated verbatim the mistake
this helper's doc describes: they walked `schemaic-ui` alone, and it is the two menu gates the
gap bites hardest, since `schemaic-app`'s `main.rs` constructs `context_menu`, `popup_menu`,
`popup_anchor` and `date_pick` — the very channels they police. Checked at the time: no such
opener existed there, so the gap was structural rather than live, and all four stay green over
the wider corpus.
**And the surviving `split("#[cfg(test)]")` cuts go through `production_code` now** — in
`app/main.rs` and `ui/erd_view.rs`, and in what was `app/mcp.rs`, whose two are this module's
own `mcp_deadline_gate` below now, over `schemaic-cli/mcp.rs`: the last places still doing it by
hand. That
cut is positional, so a file with an inline test-only `fn` above its test module loses everything
after it, which is how `widgets.rs` lost 87% of itself; and it is not comment-aware, so a `///`
line that merely *mentions* the attribute cuts there — several of the gates behind those cuts are
`!body.contains(…)` with no floor, and would have passed by scanning nothing.
**`no_continuation_typed_as_newline_gate` lives in this module rather than beside a subject**,
because its subject is the whole tree: Rust's `\`-before-newline strips the newline *and* the
next line's indentation, while `\n` inserts a newline and keeps the indentation, so a sentence
meant to be one line renders with a hard break and twenty-odd spaces in the middle of it. Three
shipped, in three files, each byte-verified with `cat -A` — `app/main.rs`'s warning that
read-only and the environment badge are not carried over (`\n` + 17 spaces), `ui/users_view.rs`'s
grant-statement cap note (+ 25), `ui/lib.rs`'s pinned-results memory tooltip (+ 21). None is
testable on its own: each is a literal inside a view and the damage is what a renderer does with
it, which is exactly the case for a lint instead. **Eight spaces is the threshold** — a
deliberate `\n` in prose is followed by the next word, and a wrapped string literal nested in a
view is indented well past eight columns; the only matches outside the two crates' production
code are a CLI-help fixture, a synthetic source fixture and two live-test SQL strings, all of
which mean their newline. A deliberate break puts the next line at column 0. **It reads
`schemaic-cli` as well**, since `mcp.rs` moved there: every tool description and refusal a model
reads is a literal in that file, and the move took it out of `crate_sources()` — a
`"one\n          two"` planted in `mcp.rs` passed the gate until the corpus followed it, and fails
it now. So it scans `crate_sources()` plus the `schemaic-cli/` files of `workspace_sources()`.
**`no_string_literal_carries_a_wrapped_lines_indentation` is that gate's mirror image**, over the
same typing habit inverted: a string wrapped across source lines with **no** `\` at the break
keeps the newline *and the whole of the next line's indent* as content, so a sentence carries a
fourteen- or eighteen-space run in the middle of it wherever that string goes. Six sites, and
they differed only in who saw them — `app/antigravity.rs`'s `blocked_reason` reached the AI
panel's no-tools note verbatim, `core/ddl.rs`'s generated-column refusal shows in the DDL
preview, `ui/blob_view.rs`'s is a cell-preview message, `core/schema.rs`'s is the sequence note
in a copied script, and `core/launch.rs`'s and `conn/secrets.rs`'s are latent because their
consumers collapse space runs, which is a copy of the mistake rather than a live one and exactly
how a class survives being fixed at its noisy sites. It runs over `workspace_sources()` and
applies **Rust's own continuation rule before looking**, or every correctly-written multi-line
string in the workspace is a hit; it skips raw strings and char literals (`'"'` desynchronises a
naive scan), reads production code only — a test's expected output lines its columns up on
purpose, and so does a SQL fixture — leaves alone any literal spelling its own `\n` or `\t`, that
being the mark of a block whose spacing is deliberate, and carries floors on both the file count
and the literal count. **`secrets.rs` is in that list of six and was for a while no longer in
the census**: `workspace_sources()` named `ui`, `app`, `core` and `db`, the file moved to
`schemaic-conn`, and
every gate here stopped reading it — silently, as a side effect of the move. What kept it out
afterwards was the floor: `sources_of` took one flat count and asserted at least five source
files per directory, and that crate has two, so declaring it would have failed the suite and
leaving it out cost nothing visible. **The floor is per label now** — `sources_of` takes
`(label, floor)` pairs and `schemaic-conn` declares 2 — because forcing a genuinely small crate
out of the census is the exact failure the floor exists to catch, one directory short-changing
the corpus while the total still looks healthy. The floor still cannot catch a crate *deleted
from the label list*, which reads as a smaller-but-healthy corpus, so
`the_wider_scan_reaches_the_keyring_store` names `schemaic-conn/secrets.rs` outright — the way
`the_scan_reaches_both_crates_that_build_views` names `lib.rs` and `schemaic-app/main.rs` — and
says in its own prose that this file left the census once already. **`schemaic-cli` is in
`workspace_sources()` since `mcp.rs` moved there** (floor 10), for `conn/secrets.rs`'s reason
over again: the file was a member of gates here — the `emit().join` one among them — while it
lived in `schemaic-app`, and a move out of the corpus is a silent exit from every one. It was out
before, with no gate having a rule about it; including it surfaced `args.rs`'s `AFTER_HELP` in
the wrapped-indentation gate above, answered there by a `concat!` of `\n`-terminated literals
(`args.rs`'s entry). It is in `workspace_sources` only — `crate_sources` stays the two view
crates, and the `\n` gate above takes its `schemaic-cli/` files from there.
**`mcp_deadline_gate` is two gates about one file of another crate**, `schemaic-cli/mcp.rs`'s
`every_database_read_carries_the_deadline` and `no_database_read_is_awaited_without_a_deadline`,
moved here from that file's own tests because they need `production_code` and that crate is
Floem-free — `mcp.rs`'s entry has the rules. They take the file through
`workspace_source(name)`, which takes the name `workspace_sources()` reports a file under, reads
that one file directly — `crates/<crate>/src/<rel>`, then `production_code` — and panics naming
it when it is not there, so a gate over a file that moves fails rather than scanning nothing. It
used to walk the whole census to keep one entry, every crate's source read and cut for nothing.
**`headless_read_session_gate` is the same arrangement for the other half of the headless read
guard.** `every_headless_read_runs_on_a_read_only_session` asserts that every
`.fetch_query_enforced(` and `.stream_query_enforced(` in `schemaic-cli/query.rs`,
`schemaic-cli/mcp.rs` and `schemaic-cli/dump.rs` carries `Enforce::ReadOnly` or
`Enforce::Generated` within its own argument list — cut at the matching `)`, so one elsewhere in
the file cannot answer for a call without one — and that no such file calls plain `.fetch_query(` or
`.stream_query(`, with a floor of one call per file so a needle that stops matching fails.
`dump.rs` joined the list with security review S8: its `SELECT`s are generated, so no text gate
reads them, and a view's body can call a function that writes (`cli/dump.md`); the gate was seen
red against the plain `stream_query` it had. **It takes either variant in `dump.rs` and `mcp.rs`,
and only `ReadOnly` in `query.rs`**: `Generated` (the dump's reads, `mcp.rs`'s `sample_rows`, since
release review S1-L1-02) refuses writes as `ReadOnly` does but skips the checks a text gate's
judgement rests on, and every read in `query.rs` is the gated text, so one moved onto it fails
here. `mcp.rs`'s `run_query` is not told apart from its `sample_rows` this way. It is a source gate because nothing else could be red: changing
`read_only_query`'s `Enforce::ReadOnly` to `Enforce::None` passed the whole suite, since
in-memory SQLite has no statement that passes its text gate and writes. The editor's twin is
`every_editor_sql_path_runs_on_a_session_enforcing_read_only`.
`a_headless_plan_never_executes_the_statement` sits beside it for the MCP server's `explain`:
every `.explain(` in the same two files must pass the literals `false, true` as its third and
fourth top-level arguments — `analyze`, which would execute the statement, and `read_only` — and
a floor of one call across both files fails a needle that stops matching. A source gate for the
same reason: SQLite, the one engine the suite reaches, ignores `analyze`, so no behavioural test
could tell. Seen red with `analyze` flipped to `true`.
**`folder_export_consent_gate` is the same arrangement again, over `schemaic-cli/dump.rs`**:
`the_folder_export_consents_to_the_list_the_user_was_shown` was that file's own test while the
dump's driver lived in `schemaic-app`, which could reach `production_code`, and came here when
the driver moved into the Floem-free crate. It asserts the folder export hands `folder_verdict`
`req.approved.as_deref()` — the list the user was shown — and a `colliding_files` census read
off the folder; `cli/dump.rs`'s entry has why a swap would pass every other test.
