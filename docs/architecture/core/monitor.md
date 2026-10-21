# `core/monitor.rs`

`monitor.rs` — the **Live Monitor**'s pure change detector: no DB, no timer, no UI.
`Snapshot::from_result` captures a `ResultSet` keyed by its table's key columns (cells are
`Option<String>` so NULL stays distinct from `""`), and `diff_snapshots` matches two snapshots
by key into `RowChange::{Insert,Update,Delete}`, an update carrying per-column `FieldChange`.
Row identity is just `Vec<String>`, so a new engine's fetch path only has to produce a
`Snapshot`. The caller must skip the *first* poll itself — diffing against an empty prior reads
every row as an insert. A delete carries the row's last-seen cells deliberately: it is the one
case where the row is gone from the database and the log is the only remaining record.
**A snapshot sees a binary column's change by its length only**: its cells are the result's
text, and a blob's text is `<n bytes>`, so a blob rewritten at the same length is no change to
`diff_snapshots`. `length_only_columns(rs)` names those columns — binary by type or by
`ResultSet::binary_columns`, as `result_diff` decides — so the modal's footer can say so, by a
warning icon beside its status line, rather than let the log read as complete (`a_binary_column_is_watched_by_length_only`). It is a
caveat, not a fix: the
bytes are not in the result to compare. `merge_length_only(prev, rs)` is that list grown by a
poll — `None` where the poll adds no column — since SQLite marks a column binary as blobs
arrive in it, so a later poll can name one the baseline could not, and a column once seen by
length stays so (`a_column_marked_after_the_baseline_joins_the_length_only_list`).
**A `Snapshot` records *how it was taken* in two flags, and they were one.** `ordered` says the
poll asked the server for a deterministic total order (`ORDER BY` the key), so position means
something and two windows are comparable as prefixes of one sequence; `window_full` says the
fetch came back at its row limit, so the table holds more than these. They were a single
`ordered_window_full`, and conflating them is what made the log false. A bounded window has to
be a *stable* one — delete a row inside it and the next is promoted in, which reads as an insert
of a row that existed all along, so `diff_snapshots` suppresses the tail, but only for a window
it knows is an ordered prefix. A window that is **full but unordered** is neither state the old
flag could express and it landed on the "report everything" branch: measured on MariaDB 10.11,
deleting one row of a 1,200-row table with no primary key logged a phantom INSERT beside the
real DELETE; on PostgreSQL 16.15 an `UPDATE` that moved the heap tuple out of the window was
logged as a **DELETE of the row it updated**, with the update never reported at all. There are
three states per window, and each cut asks about the *other* one, because that is the window
whose silence has to be interpreted: below the limit the window **is** the table, so absence
from it is real; at the limit and ordered, absence before the last row the two share is real and
everything after it is the window sliding; at the limit and unordered, which rows came back is
arbitrary and absence means nothing. The boundary is positional (`last_common`) and not a key
comparison — the keys are text, and comparing them here would order `"1000"` before `"500"`
where the server did not. Rows both windows hold are diffed either way, so a real change to a
visible row is still seen.
**`ordered` has to describe the query that was actually sent, and the app is where that came
apart.** The order key is resolved by `main.rs`'s `monitor_order_key` off the live `db_nodes`,
and it was asked **twice**: once in `monitor_tick` to shape the fetch and again in
`monitor_apply` to stamp `Snapshot::ordered` on the reply. On a fresh connect the schema
routinely lands between the two — poll 1 goes out with no `ORDER BY`, the catalogue arrives while
it is in flight, and the reply is stamped ordered — after which `diff_snapshots` reads an
arbitrary sample as an ordered prefix and reports every row of it the real first page does not
hold as a DELETE, cells and all, into a log that is exportable and treated as a record
(`an_arbitrary_window_called_ordered_reports_deletes_of_rows_that_are_still_there`). The second
consequence needs no race at all: recomputed per tick, the *window* moves mid-session, which is
the same false diff by another door. It is now `MonitorCtx::order_by`, resolved once in
`open_monitor` before the first tick and read by both — pinned for the reason `key_cols` and
`dialect` already were, and the gate is
`the_monitors_order_key_is_resolved_at_open_and_nowhere_else`, a source gate because both calls
were individually correct and the defect was that there were two. This matters more than a display nicety because the log is
exportable, and `discard_needs_asking` exists precisely because it is treated as a record
somebody keeps.
**`baseline_is_stale(old_cols, new_cols)` is the other thing a snapshot cannot survive, and it
is the *shape* of the table rather than its rows.** Everything here is positional:
`Snapshot::from_result` builds each row's `cells` by column index, `diff_snapshots` compares two
`cells` vectors by index, `field_changes` walks them by index, and the modal names each
`FieldChange { col }` off the column list captured on the **baseline** poll. So an
`ALTER TABLE orders ADD COLUMN note TEXT AFTER id` run in another session while the monitor is
open shifts every cell from index 1 on by one place: every row in the window is logged as an
UPDATE, each field named from the pre-`ALTER` order, so the log claims `name: 'Ada' →
'2024-01-03'` for a column that never changed — in a record the modal exports and treats as the
only copy. The mirror case is quieter and worse: a `DROP COLUMN` before a key column leaves the
resolved key indices pointing past the new width, `from_result`'s fallback gives *every* row the
empty key, the map collapses to one entry, and the diff reports the whole window deleted. The
comparison is by **name and position**, not as a set — two columns swapped is the same shift, a
rename at the same position is a different column to a positional diff — and case-sensitively,
since a server that changes a column's case has changed the text the log will print. The answer
is to restart the baseline rather than diff across it, because nothing here can say what happened
to a row while the table's shape was changing; `main.rs`'s `monitor_apply` does that and **says
so in the modal**, a silent restart being indistinguishable from a poll that found nothing.
The **log** lives here too, not just the diff. `MonitorEntry { at, change, seq, cols }` — a
change, the local wall-clock `HH:MM:SS` at which a poll *observed* it (stamped by `main.rs`
off `date::local_now` as the poll lands, and read by the modal's *since 11:48:12*; it used to
be `M:SS` elapsed since the modal opened, and the export's `Time` column changed with it), its
position in the log and **the watched
table's column list as it was when the entry was recorded** — moved down from `ui/lib.rs`, which re-exports it so every
use site still reads as a UI type, because the log's export is a pure projection of these
entries and belongs beside the diff that produces them. `log_result_set` is that projection: it
renders the log to a `ResultSet` so it exports through the ordinary `core::export` renderers
instead of a second set written for it. One row per change — `Time`, `Action`, `Key`, then one
column per watched-table column — with an insert carrying the new values, a delete the last-seen
ones, and an update `old → new` in the columns that changed and the plain value everywhere else.
A value standing alone is a real NULL cell, so each format spells NULL its own way; a NULL
*inside* a transition can only be the literal text `NULL`, because the transition is one cell.
**Width comes from the data, not from `cols`** — a change carrying more cells than the baseline
named widens the result (the extra columns become `column_N`) rather than being truncated,
because a silently narrowed export is the failure nobody notices later.
**And every entry is projected under the names *it* was recorded with.** Everything about a
change is positional — a `FieldChange { col: 1 }` means "the second column", and which column
that is came from a list held beside the log rather than in it. That was sound while the list
could only be written once per monitor session, and the baseline restart that answers an `ALTER`
mid-session re-opened the branch that writes it: from then on the log held entries recorded
against the old list and the export rendered all of them against the new one. After
`ALTER TABLE orders ADD COLUMN note TEXT AFTER id`, every earlier UPDATE was exported under
`note` instead of `name` and every earlier INSERT/DELETE row sat one column off its heading — in
the artefact this module's own doc calls the only remaining record of a row the database no
longer has, and silently, because the restart notice goes to the poll's error slot and the next
poll clears it. So the output columns are the **union** of the names seen, in first-seen order,
and a pre-`ALTER` change lands under `name` while a post-`ALTER` one lands under `note` — which
needs no gap marker, nothing being mis-stated to begin with. `cols` still leads, so an unchanged
session exports exactly as before; an entry with no list of its own (a restored log, a fixture)
falls back to the list passed in, which is what the single-list form did for every entry. The
field is an `Arc` because every entry of a poll shares one list and the log runs to `LOG_CAP`,
and `append_changes` takes the list so the capture happens where the entry is made rather than
at render time. `LOG_FORMATS` is
everything the grid offers **except SQL**: SQL renders `INSERT INTO <table>`, and a change log
has no such table — its rows are observations *about* one, a third of them deletions. It has a
second user now: Compare results exports through `result_diff::DIFF_FORMATS`, which *is* this
list, and `result_diff::export_result_set` is this projection's shape for two results,
`TRANSITION` (`pub(crate)` for it) included. Both caps
are here for one reason, that the modal has to be able to *name* them: `ROW_CAP` (rows per poll,
past which the monitor watches a page rather than a table) and `LOG_CAP` (changes kept, oldest
dropping — a cap the app used to keep private, moved here once the log became exportable, since
a silently truncated record looks complete and isn't). **`trim_log` is the one place `LOG_CAP`
is applied**, and it returns how many entries went, because the app trimming on `> LOG_CAP`
while the modal's caveat printed on `>= LOG_CAP` meant a log resting exactly at the cap claimed
a loss it hadn't had — on a record whose only value is that it can be trusted. The modal's
footer reads the count. `discard_needs_asking(len, exported)` is the other decision that belongs here
rather than in a modal: the log is the only record of what a deleted row held and no poll
re-reports a change, so throwing it away is irreversible — unless it is empty, or already on
disk, which is why the confirmation isn't unconditional.
**The modal's table is a projection from here too, and it heads its columns as the file does.**
`log_columns(entries, cols)` is the union of names `log_result_set` used to compute inline —
factored out, and called by both, so the screen and the file cannot disagree about which value
sits under which heading after an `ALTER` (`a_change_shows_under_the_names_it_was_recorded_with`
asserts the two lists equal). The modal asks it after every poll that lands, so it walks each
run of entries naming their columns alike once (`shapes`) rather than re-walking each — alike
by **content** and width, the pointer only settling the common case first: each poll records
under a fresh `Arc` of the same list, so a pointer key split a log of one-change polls into a
run per entry, 6–18 ms on the UI thread per landing poll at 60–120 columns
(`equal_column_lists_under_fresh_arcs_are_one_run`).
`table_cells(entry, headings, cols)` is one row of it as `LogCell`s: `Value` (an insert's new
value, a delete's last-seen one, a value an update left alone), `Changed(old, new)`, or
`Missing` for a column the entry was not recorded with — rendered empty, not borrowed from a
neighbour. Every string goes through `text::cell_line`, which is what keeps the view's
one-line-per-row promise (`the_table_cells_show_each_kind_whole` feeds it a newline). The
private `names_of`, `width_of`, `name_at` and `entry_indices` are shared by the two projections.
`kind_counts` gives the filter pills a `KindCounts`, whose `of(None)` is the whole log
(`the_pills_count_each_kind_and_all`), and `action_label` is public because the rows print it.
**The table carries every watched column, uncapped.** The cap it once had was on a single
*Data* column of `col=value` spans, unreadable as one line past a dozen; with a column per
watched column there is no such line, and since the list is a `virtual_stack` a wide table costs
views only in the twenty-odd mounted rows, not the 23,000/63,000/123,000 counted at 10/30/60
columns when every row was built. `the_export_carries_every_column_of_a_wide_change` pins the
export's half.
