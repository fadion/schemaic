# `ui/import_view.rs`

`import_view.rs` — the file-import modal (schema context menu → **Import**), over
`core::import`. Two steps (Source → Mapping) in one panel driven by the `ImportUi` bundle;
`SchemaActions::import_probe`/`import_run` do the file + DB work off the UI thread, reached
through `ImportCtx`. **The drivers take that ctx; the renderers take the bundle.** `probe`,
`run_import`, `source_step` and `import_overlay` reach a fetch, the read-only flag or the schema
tree, so they take `ImportCtx` — `ImportUi`, `ConnUi`, `SchemaUi` and the probe/run/cancel
closures, and no `Ui`; `mapping_row`, `preview_table`, `issue_list` and `mapping_step` touch
nothing but the modal's own signals and take `ImportUi`, which is also what stopped the mapping
step cloning a whole `Ui` per row. `open_import` takes `ImportUi` alone. Why the split falls
there, and why a ctx on the second half would have been a widening, is under `lib.rs`'s
`whole_ui_gate`. A probe or
an import can outlive the modal, so both callbacks check `ImportUi::generation` (bumped on
every open) before writing — and a probe checks `probe_seq` (bumped per *request*) too, via
`import::probe_verdict`: several probes of one file are routinely in flight, they report in
completion order, and only the newest may write. **Discarded whole, `busy` included** — that
flag staying set is what keeps Next and Import disabled until the newest answer lands, which
is in turn why `run_import` may send a fresh `read_config` beside the *stored* mapping.
A schema change goes through `import::target_survives`: closing needs positive evidence the
table is gone, since `load_schema` empties `db_nodes` before it fetches, and a running load is
*cancelled* rather than abandoned. `ImportTargetInfo::display` calls `schema::display_name` like
the four sibling `display()`s in `lib.rs` (the fifth, `object_location`, deliberately does not
and says why); it used to be that rule expanded by hand, inline `"public"` and all. It *agreed*,
which is exactly the cost — `sql_qualifier`'s doc carries a live-reproduced bug as the reason
for its shape, a schema literally named `"PUBLIC"` being a different schema — so the next change
to that rule would have reached five of the six spellings. The effect that re-probes on a settings change tracks only settings
that change how the file *parses* — the NULL rules apply at coercion time, so tracking them
would re-read the file per keystroke and stamp over a hand-edited mapping.
**Which is why the NULL settings join the *preview's* `dyn_container` key instead.** They cannot
change what a probe returns, so the rule has to be applied at render time over the sample already
in hand — costing no file read, and making the tokens box live-verifiable for the first time.
Each cell goes through `import::preview_field` (see `core/import.rs` for the defect that bought
it) and a NULL is drawn the way the grid draws one, faint italic, so it cannot be mistaken for
the text that spells it. `null_rule(empty_is_null, tokens)` is the two controls as one
`NullRule`, kept separate from `read_config` because the preview needs it from *tracked* reads
and a second spelling is how the screen and the load come to disagree.
**`ImportUi::begin_probe` clears where the question changes, not where the answer does.** `issues`
is written in one place — the check refusing an import — and used to be cleared in two, a new
modal *open* and the next launch, neither of which is a new file: importing `bad.csv`, pressing
Back and picking `clean.csv` showed `clean.csv`'s columns and preview under `bad.csv`'s problems
and line numbers, attributed to "the file", and fixing the cause by typing a NULL token left the
list up too. One call on the probe covers both triggers, because a file pick and every tracked
settings change both route through it, and it deliberately keeps `path`, `format`, `sample` and
`sheets` so the step does not blank while the new read is in flight. `ImportUi::new` is the
bundle's opening state, extracted so the decision has a subject at all — the app used to spell
all twenty-four signals out at the call site — and
`a_probe_invalidates_the_previous_read_through_the_bundle` pins that `probe` asks the bundle
rather than reaching for the one signal it happened to remember; asserting that `begin_probe`
empties `issues` would prove nothing.
**Excel adds an `xlsx_settings` block beside `csv_settings`**, built and torn down rather than
hidden, for that section's Tab-order reason: a Sheet picker and the "First row is a header"
toggle, which is all a workbook has to be asked (there is no delimiter and no quote). The header
toggle is repeated rather than lifted out of `csv_settings` and shared, since hoisting the one
setting the two formats have in common would move a control CSV users already know the position
of. The picker is `settings::in_ring_picker` **directly** and not `focusable_dropdown`, because
that wrapper needs `T: Copy` and a sheet name is a `String`, and it is built only when the
workbook has two sheets or more — a dropdown with one entry is furniture. **The picker sits in
its own `dyn_container`, keyed on `sheets`, inside the block's** — which is keyed only on
whether the format is Excel. The list is rewritten on *every* probe, and floem's `dyn_container`
takes no `PartialEq` (`create_updater` does not dedup), so keying the outer block on it rebuilt
the header toggle the instant the user pressed Space on it, dropping keyboard focus. `ImportUi`
carries the two signals behind it, `sheets` and `sheet`, cleared in `open_import`, again when a
probe reports a *different* file, and again when a probe **fails**: a sheet list left standing
would describe the previous workbook beside an error about this one, and clearing the chosen
sheet is also what unwedges a name carried over from a file that had it. `sheet` joins the
settings effect above, because which sheet is read is a fact about how the file parses exactly as
the delimiter is. The probe itself fills `ImportProbeResult::sheets` from the same parse as the
sample and **skips the CSV sniffer for a workbook**: an `.xlsx`'s head is deflated ZIP bytes, so
running the sniffer over it would let a compressed stream's byte frequencies decide `has_header`.
While a load runs,
the footer's Cancel fires `SchemaActions::import_cancel`, as `ctx.cancel` (the app owns the
token, as it does for query runs) instead of closing — the load rolls back on its own connection and the report
says what that achieved, which on a non-transactional MySQL table is that the rows already loaded
are still there (`db::cancelled_import`; the modal renders `DbError::Cancelled` as *nothing was
written*, so that variant is reserved for a rollback the server confirmed).
**The problem list renders `ISSUE_LINES` (50) of them and derives its tail from that same
number**, through `import::issue_tail_of`, which is the one function that sees this cap and
core's `max_issues` together. The section sits inside the body's own scroll, so the cut was never a
layout requirement — 20 was a bare literal, and its whole cost was that the disclosure beneath
it answered about the other cap (see `core::import`).
**Its words are the catalogue's** (`import-*`) — the title (`import-title`,
`$table`), the four section headings, every field, toggle and hint, the file picker's button and
window title, *Skip* in the mapping dropdown (`target_label`), the nothing-mapped and
values-withheld notes, the footer's Next/Reading…/Back/Import/Importing… and the cancelled line;
Cancel and Close are the shared `dialog-cancel`/`dialog-close`, and the read-only sentence
beside the disabled Import is the shared `dialog-read-only`, the message `ddl_preview` puts
beside its own disabled Apply. Composed lines are whole messages:
`import-missing-required` (`$columns`), `import-engine-not-transactional` (`$engine`), the
plurals `import-problems` and `import-done`, and each problem's line through `issue_line` —
`import-problem-at-line` or `import-problem-at-column`, a message per shape rather than a place
spliced in. What is wrong with a field is core's word drawn here, early, as *The string
catalogue*'s rule for a property of a typed value has it: `i18n::issue_kind_message` stands in for
`IssueKind::message` (`import-issue-*`), core keeping its English for the CLI — in `i18n.rs`
rather than here, `pub(crate)`, because the load's backstop says it too (*The string
catalogue*) — and `english_words_every_issue_kind_as_core_does`, still in this file, holds the
two equal for every kind;
`the_composed_lines_read_as_they_did_in_english` pins the rest. Core's composed prose is the
catalogue's too, from the value core decides: `issue_tail_text` words `import::issue_tail_of`'s
`IssueTail` from `import-tail-*`, and `memory_warning_text` the `MemoryWarning`
`memory_warning` answers from `import-memory-json`/`-xlsx`, its sizes going in as
`format::human_bytes` prints them through `i18n::figure`, in the interface's marks —
`english_tells_every_issue_tail_as_core_does` and `english_warns_of_every_memory_cost_as_core_does`
holding each to core's `message`. Why a file cannot be read is worded by the app, which owns the
probe, the check and the load, through `i18n::read_refusal_text` for the probe's
`xlsx_size_refusal` and `i18n::import_error_text` for every `ImportError` the three return, in
place of `e.to_string()`. How the load *failed* is the app's too: `ImportOutcome::Failed` takes
`db_error_shown`, so a `DbError::Write` — the cancelled sentence, the batch count, the rollback
clause — is `i18n::write_failure_text`'s (*The string catalogue*), and so, now that it is a
value (`import::RowError`), is the load's per-row backstop, `RowCtx::row`'s *line N, column
M: …*, which used to travel through the db layer as English text. What stays English: the
format names (`ImportFormat::label`); a library's own text inside `ImportError::Read`, the CSV
parser's inside `RowError::Read`, and an io error's OS text; the preview's `NULL` cell, a SQL
keyword; the `—` for a missing column; and a target's `name  (type)`.
