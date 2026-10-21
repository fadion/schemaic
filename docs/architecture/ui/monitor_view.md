# `ui/monitor_view.rs`

`monitor_view.rs` — the **Live Monitor** modal (`monitor_overlay`), opened from the results
title bar with the tab's `conn_id` and the shown result's own table. It renders `overlay.monitor_log` — built
by the app's poll loop through `core::monitor::diff_snapshots` — as a table of Time, Action and
one column per watched column (the key is just one of them), coloured as Compare results
colours its differences, and owns *none* of the polling: closing the modal flips
`overlay.monitor_open` false, and that is what stops the loop.
**The layout is Compare results'**, at its width (`modal_w(920)`, `MON_H` 560). The title row is
its own `title_bar` — *Live monitor*, the watched `database.table` in mono `text_dim`, a pill
badge whose dot says *Watching* (`new_color`), *Paused* (`theme::plan_warn`) or *Stopped*
(`theme::error`), and the ✕ in the
ring at `TITLE_CLOSE_TAB`; `modal_title` takes only a string. **Stopped outranks Paused**, and
the pure `badge_state(paused, stopped)` decides it (`a_stopped_monitor_never_reads_as_watching`):
the loop ends for good when the table has no row key — `monitor_apply` sets
`overlay.monitor_stopped` (carried as `MonitorCtx::stopped`, reset by `open_monitor`) where it
returns without rescheduling — and the badge used to read only `paused`, so it stayed a green
*Watching* over a loop that was not running, beside the footer's *changes can't be tracked*.
Under it, the filter pills *All ·
Inserts · Updates · Deletes* with their counts (`widgets::count_pill`, `PILLS_TAB` 6–9, Enter
or Space picking) on the left, and Pause, Clear, Export and the interval dropdown on the right.
The filter is an `RwSignal<Option<ChangeKind>>`, the counts a memo over
`monitor::kind_counts`. The footer is `modal_footer_split`: `footer_line` on the left and a
Neutral **Close** at `ACTION_TAB` — Neutral as Compare's is, the footer offering no decision to
affirm. The cells come from `monitor::table_cells`: an insert's values in `new_color`, a
delete's struck through in `old_color` (`widgets::struck_text`), an update's changed values
`old → new` (`widgets::transition_text`) beside its unchanged ones in `text_dim`, and a
`LogCell::Missing` empty. The headings are `monitor::log_columns` in a memo — the export's own
union of names, so the screen and the file head their columns alike. `old_color` and
`new_color` are `pub(crate)`, Compare's `removed_color`/`added_color` calling them, and are
**theme roles** — `theme::diff_removed_text`/`diff_added_text`, removed `#D06B6B` and added
`#71C371` on dark, `#A8332E` and `#1B6B2E` on light. They were fixed `Color::rgb8` literals tuned for the dark panel, which on the light theme's
`bg_panel` read at 1.9:1 (added) and 3.1:1 (removed) — under the *Body* floor on text that is
the whole of what an INSERT or DELETE row says. `contrast.rs` now carries both on `bg_panel` at
*Body* in `UI_PAIRINGS`, and `the_diff_texts_meet_aa_on_the_panel_in_every_theme` asserts the
rows are there and pass; the darker red the dark value replaced could not be read struck
through. Every colour, the pills' dots and the Action label
included, reaches its style through `kind_color`'s `fn() -> Color`.
**The table is built once and diffed, not rebuilt per poll**, and the two halves of that are one
change. The body's `dyn_container` reads a **memo**, `Body` — `Waiting` for an empty log (the
*Paused.*/*Waiting…* face, or *Stopped.* once the loop has ended, since a stopped loop is
waiting for nothing either and the footer says why), `NoneOf(kind)` when the picked pill's kind has none (*No inserts
yet.*), `Table(headings)` otherwise — so it rebuilds when what the area *is* changes, or when
an `ALTER` changes the columns, never per poll. Reading the log there directly used to
subscribe to it, and `dyn_container` does not diff — so every poll that landed a single change
tore down and rebuilt the header, both scrolls and up to `LOG_CAP` rows, cloning the log twice on
the way. The list jumped to the top (a fresh scroll starts at zero, and `follow` is false exactly
when the reader has scrolled away to read history) and the header desynchronised from the body
horizontally, so reading back through a live log was impossible except by pausing. And the row
stack is keyed on `MonitorEntry::seq`, a number `monitor::append_changes` assigns, **not** on the
list index: at the cap the log slides, so `{0..999}` describes a different thousand changes after
every poll while the key set stays identical — floem reuses a view whose key didn't change, so
memoising the selector alone would have frozen the rendered list at the first thousand changes
while the log and its export went on moving. The unconditional rebuild was what hid that.
**And the stack iterates the sequence numbers, not the entries.** `log.get()` deep-copies the
whole `Vec<MonitorEntry>` — every entry carries the watched row's full `Vec<Option<String>>` —
and the stack copies it again into its own storage (a `SmallVec`, in the `dyn_stack` this was
measured against): **1.19 / 2.81 / 5.34 ms per landing poll at 10 / 30 / 60 table columns**, on
the UI thread, at an interval as short as a second. A
memo of `Vec<u64>` is 8 KiB at `LOG_CAP` and copies in microseconds, and `entry_row(log, seq,
…)` then clones the *one* entry it is for — which floem asks for only when the key is new.
The log is appended in ascending `seq` and never reordered, so that lookup is a binary search; a
`seq` the log no longer holds (the window slid while the row was being built) renders nothing,
which is what the row would have shown anyway. The filter is applied in that memo — it picks
which sequence numbers — so switching pills rebuilds nothing but the rows that come into view.
**The list is a `virtual_stack` over a `LogSeqs` row source**, and this paragraph used to say
the opposite. A `dyn_stack` built every entry: at `LOG_CAP` that is 1,000 rows of fifteen-odd
text views apiece, each carrying its own shaped text layout. Measured on the real app against
MariaDB 10.11.14, a ten-column 250-row table polled every 2s, the process held 336 MB with an
empty log and **813 MB** with a full one — dead linear at 336 / 470 / 584 / 699 / 813 MB across
0 / 250 / 500 / 750 / 1,000 rows, about **0.46 MB per mounted row**. The log's own data at 1,000
entries is well under a megabyte, so effectively all of that was the view tree; virtualised, the
same full log costs 335 MB. `LogSeqs` is a `VirtualVector<u64>` and keeps both properties the
old list argued for above — the keyed diff still keys on `seq`, and the stack still iterates
sequence numbers instead of deep-copying `Vec<MonitorEntry>`. Its `slice` **clamps** the range
it is handed: `virtual_stack` computes what it wants from the viewport it last laid out, a
landing poll can shorten the log underneath that, and indexing a slice with a stale range panics
inside a layout pass — which takes the window with it.
`a_range_past_the_end_of_the_log_is_clamped_rather_than_panicking` pins it.
**The row's height is `mon_row_h()` (29) on the row itself**, where it was `padding_vert(7.0)`
plus a 2px gap on the stack reaching the same pitch: `VirtualItemSize::Fixed` places child `i`
at `i * item_size`, so a height computed one way for layout and another for the stack drifts
further out of place the further you scroll — see the Floem gotcha, which is also where the
contrast with `conn_import::ROW_VIEW_CAP` lives.
**The 8px vertical band is a wrapping `container`'s, not the stack's**, and that is arithmetic
rather than taste: `virtual_stack` derives start, end and `before_size` straight from
`viewport.y0 / item_size` and knows nothing about its own padding, while `compute_view_layout`
hands it a viewport in its own border-box space — so `padding_vert(8)` on the stack made the
window short by the top inset and the first row at the top edge was never built. It also forces
`height(content_size)`, which taffy reads as a border box, so the 16px would have come back out
of the rows. Padding the parent leaves both exact.
**Two further things keep `Fixed`'s promise, and neither is the row's own style.** `entry_row`'s
missing-`seq` slot is `empty().style(|s| s.height(mon_row_h()))` rather than `widgets::nothing()`
— `nothing()` is `display: none`, so it occupies zero rows under a stack that has already
reserved 29px for it and puts every later row in the window that far above its place. And every
value is flattened to one line before it reaches `text()` — by `core::monitor::table_cells`,
through `text::cell_line` — because "a change row is always one line" is a property of what the
row is handed and not of the row: a watched `TEXT`/`VARCHAR` column holding an address, a note
or pretty-printed JSON is the ordinary case, and it shaped to two lines, laid out past its own
box and painted across the row below. The premise was stated in three places and checked in
none until the view's own `cell` was tested; the flattening now lives in the core beside Compare
results', one definition for both tables, pinned by `a_cell_line_is_one_short_line` and
`the_table_cells_show_each_kind_whole`. The export path (`log_result_set`) is deliberately not
routed through it and keeps the engine's value verbatim, and Compare results' export
(`result_diff::export_result_set`) keeps the same distance from its own table's line.
**The width objection the old paragraph raised is answered rather than withdrawn.** With only
the visible rows mounted, the widest row in the *window* would set the content width, so a
range that shrank past the current offset would drag the body and the header — which mirrors
the body's `hscroll` — sideways under a *vertical* gesture. So the widths are decided from the
**log**, never from the mounted rows: `ColumnWidths` measures each entry once, as it lands, and
only shapes a value longer by characters than its column's longest so far, so a poll costs a
few measurements rather than one per cell. A column only ever widens, clamped 40–360; the
widths are kept **unscaled** in an `RwSignal<Rc<Vec<f64>>>` and scaled where a style reads them
(`col_w`, `table_w`), so an interface-scale change moves the header and the rows together. It
starts over when the headings change or the log is cleared — its last `seq` going back is the
signal — and the stack's `min_width` is `table_w(widths)`, known from the log rather than from
the rows that happen to be mounted. It replaced a monotone `widest` kept up through `on_resize`
on the mounted rows. The sideways drag it prevents is **reasoned from the layout, not
observed**: synthetic shift+wheel never reached floem's horizontal scroll, so unlike the memory
figures above nothing measured it. Pause, Clear and Export sit left of the interval dropdown and
join the modal's `FocusRing` after the pills, at tabindex 10/11/12 with the dropdown at 13, so a
monitor is watchable with both hands off the mouse. **Pause holds the fetch, not the loop**: `monitor_tick` reads the three signals and
switches on `core::monitor::tick_action(open, superseded, paused) -> Stop | Reschedule | Fetch`,
which is where the decision lives so it can be tested — the arms sat inside a floem-scheduled
function no test could call, and swapping two statements would have turned Pause into Stop with
nothing failing. A paused tick **reschedules**, because a pause that unwound the loop would need
`open_monitor` to restart it and that resets the baseline and the log — the opposite of what
Pause is for. A closed modal is the only thing that ends the loop; a superseded generation stops
*without* re-arming, or the old target's loop polls beside the new one forever. The cost is that the baseline ages, so the first poll after a
resume diffs against the pre-pause table and logs the *net* change stamped at the resume; that
is the log's standing rule (an entry is stamped when a poll observed it, not when it happened),
just coarser. **Clear empties `overlay.monitor_log` only** — the app's baseline snapshot is
deliberately untouched, so clearing loses history you have already read and never a change that
hasn't been reported yet — **and it asks first**, through the shared `overlay.confirm`, when
`monitor::discard_needs_asking` says the log has something to lose and no copy on disk. The
button is one glyph from Export, at the same metric, dimmed by the same predicate. **Closing the
modal no longer empties the log at all.** It used to, "for tidiness / no stale flash", which was
harmless while the log could only be watched and became a total unprompted loss the moment the
same commit made it an exportable record — on Escape, the most reflexive key in the app.
`open_monitor` resets every monitor signal on the way in, so there was never a stale flash to
avoid. **Export** raises the shared `overlay.popup_menu` over
`core::monitor::LOG_FORMATS`, anchored `PopupAnchor::BelowIcon` from the ring wrapper's
`layout_rect()` exactly as `table_designer::suggest_chevron` does (it paints above the modal
because `popup_menu_overlay` is mounted after every modal in the workspace stack — only
`submenu_layer`, which draws this menu's own submenu, sits above it); choosing a format runs
`save_log`, which mirrors `grid::save_export` in snapshotting the log **before** the file dialog
opens — the dialog is modal and slow and the poll keeps appending behind it — then renders
through `log_result_set` and hands an `ExportRequest` to the app's `ExportFn` worker with
`source: None` and a default dialect, both of which only the SQL renderer would read. The log is
always `ExportScope::Fetched` — it is a record of what the monitor saw, not a query anything
could re-run — and it asks for **`stoppable: false`**, which is what keeps it out of the app's
single export-cancel slot: nothing in this modal offers a Stop, so no token this run watches is
ever cancelled, and a run in the slot would refuse the grid's export (and be refused by it) over
a window the user cannot see. So its `ExportOutcome::Cancelled` arm does not arrive, and it is
spelled out as an explicit no-op rather than folded in with `Done`: marking the log exported off
a half-written file would stop Clear asking about a log that was never fully saved
(`monitor::discard_needs_asking`). **The reason is not the one the arm shipped with.** It used to
be that a log export is never streamed and the `Fetched` scope was uncancellable by construction;
that stopped carrying the argument the moment the `Fetched` render was chunked and gained a Stop
between blocks, and for a while the arm rested on a fact that no longer implied anything. What
holds it now is the absence of any control bound to this run, which is exactly what
`stoppable: false` says. Compare results' export (`result_diff_view.rs`) is built on this one —
the same glyph in the ring, the same popup, the same request — and inherits both reasons.
`footer_line(export_err, poll_err, partial, capped, events, since)` decides what the footer
says: an error takes the line outright in `Tone::Error`, worst-to-mislead first (a failed export
beats a poll error, because the user believes they have a file and doesn't). Otherwise it is
*"200 events since 11:48:12 · newest at the bottom"* in `Tone::Calm`, `since` being the oldest
entry the log still holds, or *No changes yet* for an empty log; with a caveat the tail is
replaced by **every** caveat that applies, joined, in `Tone::Warn`. `partial` (only the first
`ROW_CAP` rows are watched) and `capped` co-occur and neither replaces the other. **Binary
columns are not a caveat of this line**, though they were its third (S6.1-L1-05): the watched
columns `monitor::length_only_columns` names on the baseline poll, and each later poll adds to
(`monitor::merge_length_only` — SQLite marks a column binary only as blobs arrive), held in
`overlay.monitor_length_only` (`MonitorCtx::length_only`, reset by `open_monitor`). A blob's
snapshot cell is `<n bytes>`, so a blob rewritten at the same length is no change to the poll,
and a log that said nothing would read as complete — but appended to the line, the sentence
turned every table with a blob amber, and the user read that as something wrong with a table
that is perfectly normal. It is a property of the table, not of the log, so it is said beside
the line instead: the footer's left side is `h_stack((icon, line))`, a `TRIANGLE_ALERT` in
`plan_warn` that is hidden when the list is empty and whose tooltip is `length_only_note` —
*"photo is binary, so its changes are seen by length only"*, plural *are … their* for several,
`None` (and no icon) for none. A table with a binary column therefore reads plain *No changes
yet*, in `Tone::Calm`, like any other (`a_binary_column_is_said_by_the_icon_not_the_line`).
Paused or stopped is the
title's badge, not this line. The line ends in an ellipsis and its tooltip carries the whole of
it, since a poll error can outrun the footer — which takes `min_width(0)`/`flex_shrink(1)` on
the `.tooltip()` wrapper as well as `min_width(0)` on the label, or the line clips without its
`…` (*`.tooltip()`'s wrapper is the flex item* under *Floem 0.2 gotchas*). `capped` is
`overlay.monitor_dropped > 0`, accumulated from `monitor::trim_log`'s return, **not** the log's
length: at exactly `LOG_CAP` nothing has been dropped yet, and the caveat used to be printed
there anyway. An export failure that lands after the modal has closed goes to the shared error
modal rather than to `monitor_export_err`, which nothing renders once it is shut, and the
message is passed through as the pipeline wrote it — a second `Export failed —` in front of it
read "Export failed — Export failed: Access is denied". `Tone` resolves to a `fn() -> Color`, per the themable-colour invariant.
`kind_color`, behind the Action label and the pills' dots, is the same rule, found the hard way
in `entry_row`: the rows are keyed on
`MonitorEntry::seq`, so nothing rebuilds them on a theme switch and one resolved `Color` in a
three-arm `match` left UPDATE in the old theme beside INSERT and DELETE in the new one — see the
invariant for the shape.
It is pure and tested inline, which is what keeps the copy honest: the two caveats co-occurring
is precisely the case a per-state `match` got wrong.
**Its words are the catalogue's** (`monitor-*`) — the title, the badge's three
words (`badge_state` returns `(String, fn() -> Color)`), the four filter pills (`count_pill`
takes its name as `impl Into<String>`, as Compare results' pills now pass it too), the Pause/
Resume, Clear and Export tooltips, the interval labels (`interval_label`, `monitor-interval`
with `$secs`; the `—` fallback stays), the Clear confirm's title and message — a plural on
`$count`, one whole sentence per form — the Time and Action headers, the three no-kind-yet
notes, the Stopped./Paused./Waiting faces and the export dialog's window title; Close is the
shared `dialog-close`. `footer_line`'s parts are whole messages, still joined by ` · `:
`monitor-events-since` (plural, `$count`, `$since`), `monitor-no-changes`, `monitor-partial`
and `monitor-capped` (`$cap`, `monitor::ROW_CAP`/`LOG_CAP`), and `monitor-newest-at-bottom`;
`monitor-length-only` (plural on `$count`, `$columns`) — which replaced the assembled
*is … its*/*are … their* pair — is now the icon's tooltip, built by `length_only_note`. The footer's tests pin the English and pass unchanged;
`the_clear_prompt_and_the_intervals_read_as_they_did_in_english` pins the rest. English stays:
`monitor::action_label`'s INSERT/UPDATE/DELETE (SQL keywords, and what the export writes), the
export menu's `ExportFormat::label`s, the `old → new` arrow, the file name's `-monitor`
suffix, and a poll or export error's text.
