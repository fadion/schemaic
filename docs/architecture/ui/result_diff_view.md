# `ui/result_diff_view.rs`

`result_diff_view.rs` — the **Compare results** modal (`result_diff_overlay`): two results of
one tab as their differences, over `core::result_diff`, opened by setting `overlay.result_diff`
to a `DiffTarget { left, right }` of two `DiffSide { rs, sql, ran_at }` (re-exported as
`schemaic_ui::DiffTarget`; the app creates the signal beside `chart`, and `lib.rs` hands it to
the grid as `GridCtx::result_diff`) from a result chip's *Compare with* (*The results strip*),
which fills each side from its panel's result, statement and `ran_at`.
The left is the earlier result, **A**, so until the sides are swapped its rows are the
*removed* ones. Of the two results **as they
were read** — a kept one is a snapshot, a live one is what its panel held when the menu opened —
and of their rows only: nothing is fetched. Like *Chart* it changes nothing in the results or
the database, its one action writing a file. It is `modal_w(920)` wide — it was
a fixed 820 with no cap at the window's width: a title, two statement cards, the controls, the
table, a footer of the summary (or the export's notice while one stands) and **Close** —
`ActionKind::Neutral`, since the footer offers no decision to affirm — and Escape and the
backdrop closing it. Unlike
*Chart* it is keyed on whether it is open, not on the pair, which holds only because nothing
can set a second pair while it stands — the chip menu that raises it is under its backdrop.
**The cards are the two statements, A and B**, side by side: a letter badge, *Ran HH:MM:SS*
(`date::Time::at_local` of `ran_at`, left out for a `0`), *N rows · M ms*, and the SQL in mono
with its whitespace collapsed, clipped at three lines — the height computed in the style closure,
so the cut follows the interface scale. **The ⇄ between them (`icons::ARROW_LEFT_RIGHT`, in the
ring at tab 5) compares the other way round**: `compute` hands `diff` the sides in the other
order, which is the whole of a swap (see `core::result_diff`), and each card slot is a
`dyn_container` on `swapped`, so the cards trade places and the letters go with their
statements. What a swap changes is which statement is compared first, never which is called A;
the notes' *Only in A*/*Only in B* follow the same rule.
**Then the controls**: on the left the filter pills *All · Changed · Added · Removed*, each with
its count, each a `widgets::count_pill` (shared with the Live monitor's) with a coloured dot
naming its kind. On the active pill the dot takes `theme::pill_active_text`, since the
*Changed* blue vanished into the pill's fill; an inactive pill keeps the kind's colour. Each
pill is in the ring at radius 6, so the focus outline follows its corners — Tab stops
`PILLS_TAB` 6–9, after the ⇄
(`SWAP_TAB` 5), Enter or Space picking. On the right **one picker, *Match rows by*** — *Whole
row* or any column `result_diff::key_columns` offers, which leaves out a binary one since its
every cell reads `<n bytes>` and would match nothing apart — a
`table_designer::focusable_indexed_dropdown`, `PICKER_TAB` 10, then the export icon
(`EXPORT_TAB` 11), Close (`ACTION_TAB`) and the title's ✕; it starts on
`result_diff::suggested_key`. The label and
picker are an `h_stack` of their own with a 10px gap, not a `margin_right` on the label: with
the margin the picker ended 10px short of the B card's right edge above it, measured in the
running app. The export icon ends that same row, with a further `margin_left(6)` on top of the
gap — set apart from the picker because it acts on the comparison, not on how rows are
matched. It picks **one** column; the core takes a composite key, which
nothing here offers yet. Under them a faint notes line for the columns only one side has, the
binary columns left out of the comparison (*"Binary, not compared: pw_hash"*), the
repeated keys left out, and a capped result (*"rows past its cap are not compared"*), hidden
when it has nothing to say.
**The state is three signals — `swapped`, `key`, `filter` — and an `RwSignal<Rc<Computed>>`.**
An effect on `swapped` and `key` re-runs `diff` over every row read and `result_diff::table_rows`
over its differences, on the UI thread, measures the columns, and stores a `Computed {
generation, swapped, outcome }` — the `Outcome` holding the diff as an `Rc<ResultDiff>`, the
`TableRow`s, and the two results in compared order as their `Arc<ResultSet>`s, which is what
lets the cells be built later. **No cell text is built here beyond the width sample**: the
virtual stack's row builder calls `result_diff::row_cells` for the one row it mounts, where the
old `table_rows` built every differing row's cells up front at open and on every swap or key
change. A filter costs none of that: the table is a `dyn_container` keyed on `(generation,
filter)` and only picks indices out of the rows already listed. It stands at a fixed
`modal_body_h(320)` whatever the filter, so switching pills does not move the footer.
**The table copies the Live monitor's list, for the monitor's reasons** (`monitor_view.rs`, and
the *Floem 0.2 gotchas* on `VirtualItemSize::Fixed` and on padding a virtual stack): a *Change*
column, then one per shared column; the header a hidden-bar `scroll` that follows the body's
horizontal offset through `scroll_to` and swallows its own wheel, so the body is the offset's
one writer; the body a `shift_hscroll` with the autohide thin bars around a `virtual_stack` —
`VirtualItemSize::Fixed(row_h)`, 29 scaled, given to the row as its height, over a `Picked` row
source that clamps its range as `LogSeqs` does — with its vertical band on a wrapping
`container` rather than the stack. **The column widths are fixed for the whole outcome**: each is
its header and the three values longest by characters, measured (`measure_text_px_at`) and
clamped to 40–360 — shaping every cell of a large result would stall the open — taken from the
cells of the first `WIDTH_SAMPLE` (2,000) table rows only, built for the purpose, since building
them all is the cost `row_cells` exists to defer; a longer value further down is cut by its
column rather than widening it. So the content
width does not depend on which rows are mounted, the problem the monitor's `ColumnWidths`
answers for a log that is still growing. They are stored **unscaled** in `Widths` and scaled where a style reads them, so an
interface-scale change with the modal open moves the header and the rows together, as the text
inside them moves.
**The colours are the Live monitor's**: *CHANGED* in `theme::chip_active`, the monitor's
UPDATE colour; *ADDED* in its `new_color` and *REMOVED* in its `old_color` (`removed_color`
and `added_color` just call them, both `pub(crate)` for this) — which are the theme roles
`theme::diff_removed_text`/`diff_added_text` (see `monitor_view.rs`). The dark theme's red,
`#D06B6B`, is lightened from the monitor's first `rgb(9D, 34, 34)`, which was too dark to
read struck through. A removed row's values and a changed cell's old value are **struck
through** with `widgets::struck_text` — Floem 0.2's text has no line-through — and a changed cell
is `widgets::transition_text`, `old → new`, the old struck red and the new green; a changed
row's unchanged values are `cell_text` in `text_dim`. Every colour reaches its style as a
`fn() -> Color`, as the themable-colour invariant asks. What the table cannot show it says in
its own area: *No differences.*, or *No changed rows.* and its siblings for an empty filter; and
a key one side lacks, `DiffError::MissingKey`'s sentence, in the error colour, with the footer's
summary left blank beside it.
**The export is the Live monitor's, control for control** (`monitor_view.rs`): a bare
`toolbar_icon(DOWNLOAD)` in `in_ring_button`, *Export the comparison…* as its tooltip, dimmed —
and its Enter and Space ignored — **while there is nothing to export**: the key cannot match
rows, the `computed` outcome being an `Err`, or the current filter shows none
(`Outcome::count(filter) == 0` — identical results, or a pill with no rows), where a file of
headings alone would be reported saved. It raises the shared `overlay.popup_menu` over `result_diff::DIFF_FORMATS`, anchored
`PopupAnchor::BelowIcon` off the ring wrapper's `layout_rect()`, toggled shut by a second press
through `widgets::menu_anchored_at`, at `erd_view::export_menu_w()`. `result_diff_overlay` takes
the app's `ExportFn` for it, `modals.rs` passing `ui.tab_actions.export_file`. A format runs
`save_diff`, which **snapshots the comparison on screen before the dialog opens** —
`grid::save_export`'s reason, the dialog being modal and slow, so the file is what was shown when
the format was chosen. **The snapshot is reference counts and nothing more** — the two results,
the diff and the table rows, each an `Arc` or `Rc` the `Outcome` already holds, and the filter —
and the filtering (`TableRow::shown_by`) and the projection (`result_diff::export_result_set`)
wait inside the `save_as` callback for a path. They still run on the UI thread, but a cancelled
dialog costs nothing and the dialog opens at once however large the comparison. The dialog is
*Export comparison*, its default name `suggested_filename(Some("comparison"), format)`. The request is the monitor's:
`source: None` and a default dialect, which only the SQL renderer would read and
`DIFF_FORMATS` does not offer; `ExportScope::Fetched`, since nothing could re-run a
comparison; and **`stoppable: false`**, which keeps it out of the app's single export-cancel
slot as the monitor's is kept out. The outcome arms are spelled out as `save_log` spells its
own — `Done` the grid's sentence, `Cancelled` nothing, `Failed` the pipeline's message as it
came — because the day `Cancelled` can arrive, reading it as `Done` would claim a file that was
never wholly written. **`Done(tally)` speaks through `export::export_said(&tally, &name,
true)`**, worded by `i18n::export_note` as the grid's export is: the rows written and any caveat — an XLSX cell cut to the
format's limit — which a bare *Saved* would hide in a file that promises whole values. A
sentence with a caveat (`tally.has_caveat()`) is reported as a failure is, so it stays up rather
than fading, and after the modal has closed it reaches the shared error modal rather than being
dropped; a plain one fades. `save` re-asks the icon's own test before anything — the menu can
stand open over a change of key or filter — and says *"Nothing to export."*, as a failure,
when it fails.
**It reports in the footer**, the notice taking the summary's place while it stands, for
*Chart*'s reason: the shared error modal is painted under this one. A confirmation fades after
`NOTICE_LINGER` (4 s) behind a sequence number and a failure stays; an effect over the
comparison's `generation` and the filter clears it, since a notice is about the comparison it
was said of. A completion that lands after the modal closed reaches the notice through
`try_get_untracked` and finds it gone: a failure then goes to the shared error modal
(`error_modal_text` as `ModalError::app`, `error_modal_open`), as the monitor's does, and a
confirmation is dropped. **`close` clears `popup_menu` and `popup_anchor` before `target`**,
for the chart's and the ER diagram's reason — the shared popup outlives this modal and its
entries hold this scope's signals. `save` still reads `computed` and `filter` with
`try_get_untracked` and does nothing when either is gone — a backstop rather than a known path,
since `close` takes the menu down on every exit and the open-keyed container above lets nothing
replace this scope under an open menu; a menu that did outlive it would otherwise panic on a
disposed signal.
Mounted in the workspace group directly after `chart_overlay`, counted by
`workspace_modals_up` as a plain term (`diff_open`) — it raises nothing — and on
`modal_backdrop_gate::PAINTS_A_BACKDROP`.
**Its words are the catalogue's** (`diff-*`) — the title, the four filter
pills, *Match rows by*, the export tooltip, *Nothing to export.*, the export dialog's window
title, the saved notice's fallback file name (`diff-unnamed-file`), the cards' `diff-ran-at`
(`$time`) and `diff-side-facts` (a plural on `$count`, with `$ms`), the *Change* header and the
four empty-table notes; Close is the shared `dialog-close`. The key picker's *Whole row* is
`diff-whole-row`, which replaced the `WHOLE_ROW` const. The pick is read **by position**
(`key_of_pick`: row 0 is no key, row `i` is shared column `i - 1`). It was matched against that
same text once, so a column named exactly like it, in whatever language, could never key the
diff (`a_column_named_like_the_whole_row_entry_is_still_a_key`). The notes are whole
messages built by `diff_notes(diff, swapped, capped)`, still joined by `"  ·  "` in `notes`:
`diff-only-in` (`$side`, the letter, and `$columns`), `diff-uncompared`, the plural
`diff-repeated-keys` — which replaced a sentence spliced from two `plural` calls — and
`diff-capped`. `the_composed_lines_read_as_they_did_in_english` pins them and the card lines in
English. The footer's headline is `diff_summary_text`, core's `ResultDiff::summary` worded part
by part (`diff-summary-*`, joined by ` · `; `english_says_the_diff_summary_as_core_does`).
Why a key cannot match rows is `diff_error_text`, which `compute` maps core's `DiffError`
through — `diff-missing-key` and `diff-binary-key`, the column a `$column` — where the
outcome's `Err` used to be the `DiffError`'s English
(`english_says_every_key_refusal_as_core_does`). English stays: the A/B letters; `RowChange::label`'s CHANGED/ADDED/REMOVED, core's
word, which the export file writes too and which sizes the *Change* column; the export menu's `ExportFormat::label`s; the suggested file
name `comparison`; and the arrow.
