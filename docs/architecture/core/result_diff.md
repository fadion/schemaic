# `core/result_diff.rs`

`result_diff.rs` — **what two results differ by**, behind a result chip's *Compare with*
(`ui/result_diff_view.rs`): the rows one has that the other has not, and the rows both have
with different values, matched by a key the user picks. Pure over two `ResultSet`s and over
the rows already read, like `chart.rs` above — a capped result is compared up to its cap, and
the modal says so. **Columns are matched by name**: `shared_columns(left, right)` is the
compared set, in the left's order, and a column only one side has goes to `only_left` /
`only_right` and is reported rather than failing the comparison. **A binary column is left out
too, and said to be** (S6.1-L1-05): a column binary on either side — by its type
(`Column::is_binary`) or because the backend saw a blob in it (`ResultSet::binary_columns`,
SQLite's per-value answer) — has cells reading `model::binary_display`'s `<n bytes>`, a
placeholder rather than the value, so two different blobs of one length compared as the same
and the row was counted *unchanged*. `key_columns(left, right)` is the shared columns minus
those, and is what is compared: such a column is not in `ResultDiff::shared` but in
`uncompared`, and `summary` ends in *" · pw_hash not compared"* so *unchanged* is never read as
covering it (`a_binary_column_is_named_as_not_compared`). It is never a key either — asked for
one, `diff` answers `DiffError::BinaryKey` (`a_binary_column_is_never_a_key`). What this does
not do is compare the bytes: the `ResultSet` holds only the placeholder, so the honest answer
is to name the column, not to guess.
`diff(left, right, key)` → `ResultDiff { shared, only_left, only_right, left_only, right_only,
changed: Vec<Changed { left, right, columns }>, same, repeated_keys, uncompared }` — `left_only` being the
*removed* rows and `right_only` the *added* ones, so which result goes on which side is the
caller's decision and decides the words. **Keyed**, a row is matched by its key columns'
values; **a key repeated within one side** is counted in `repeated_keys` and its later rows
ignored, the first kept, because the diff can only say which row a key names when it names one
(`a_repeated_key_is_counted_and_its_later_rows_ignored`). **A key with a NULL in it names no
row**, as in SQL: such a row matches nothing and is its own side's alone, and is not counted as a
repeat (`a_null_key_matches_nothing`). **With an empty key
the whole shared row is the identity, as a multiset**: a row is in both or in one, never
*changed*, and one repeated n times on one side and m on the other is |n − m| rows added or
removed (`without_a_key_rows_are_counted_as_a_multiset`) — the honest answer for a result with
nothing that identifies a row. **Each bucket of equal rows is consumed through a cursor, not
`rows.remove(0)`**: the remove shifted the whole bucket per match, quadratic in a column of few
distinct values, and 300,000 equal rows took about 25 s on the UI thread (the fix's own
measurement; `whole_row_identity_over_many_equal_rows_is_linear` holds it under 5 s). Don't
tidy it back. **A row's key borrows the result's text** (`RowKey<'a>`, `Option<&str>` per
cell) rather than copying it: a `String` per compared cell was most of an open's or a swap's
cost — at 200k × 20 in release, 0.35 s keyed and 0.68 s whole-row before, 0.13 s and 0.32 s
after (S5-L4-01). A timing bound could not tell the two apart in a loaded debug run, so
`a_row_key_borrows_the_results_text` pins the cause instead: each key cell points into the
result. A key column one side lacks is `DiffError::MissingKey`
(`a_key_one_side_lacks_is_refused`).
**Values compare by their displayed text, NULL being its own value**, so `1` and `1.0` from two
engines differ, as they would on screen, and NULL is not `''`
(`null_is_its_own_value_and_text_compares_as_shown`). `is_identical` and `summary` give the
footer's headline — *"2 changed · 3 added · 1 removed · 40 unchanged"*, in the filter pills'
order, a zero kind left out (the unchanged count never is), or *"No differences · N rows
unchanged"*. `suggested_key` is where the picker starts: a `key_columns` column
named `id` in any case, else the first one unique and never NULL on both sides, else
none, the whole row (`the_suggested_key_is_id_else_a_unique_column_else_the_row`). It picks
from `key_columns` rather than every shared column because a `BINARY(16)` UUID `id` was
suggested, every row of it read `<16 bytes>`, and the comparison collapsed into the first row
and *repeated keys*.
**`table_rows(&diff)` is the modal's table, by reference**: a `TableRow { change, at }` per
difference and no text — `at` a removed row's index in the first result, an added row's in the
second, a changed row's in `diff.changed` — the first result's changed and removed rows in the
order it holds them, then the added ones in the second's order
(`the_table_lists_the_first_results_rows_in_order_then_the_added`). **`row_cells(left, right,
&diff, &row)` builds one row's cells**, one per `shared` column, and a row out of range has none
(`table_rows_hold_places_and_row_cells_builds_one_row`). They were one function returning a
`DiffRow { change, cells }` per difference, which built a string or three per cell of every
differing row, on the UI thread, at open and again on every swap or key change; split, the view
builds only the rows its virtual stack mounts. `DiffRow` is gone from the public API. A cell is
`DiffCell::Value` — an added or removed row's own value, or one a changed row's sides agree on —
or `DiffCell::Changed(before, after)`; `RowChange::label` is the *Change* column's
`CHANGED`/`ADDED`/`REMOVED`. **Every cell is one line**, through the private `table_value`,
which is `text::cell_line` — `NULL` for a null, every mandatory line break as a space (a CRLF
as one), cut at `CELL_LINE_CHARS` (120) ending in `…`; it owned that logic until the Live
monitor's table needed the same line (`a_table_cell_is_one_short_line` still pins it from
here). That is load-bearing rather than tidy — the view's
`virtual_stack` places row `i` at `i * row_h`, and a cell that shaped to two lines would paint
across the row below (the `VirtualItemSize::Fixed` gotcha). The cut is the cell's only; the
comparison reads the whole value. **Comparing the other way round is `diff(right, left, …)` and
nothing more** — added and removed trade places and each change reverses
(`swapping_the_sides_swaps_added_for_removed_and_reverses_each_change`), which is all the modal's
⇄ does.
**`export_result_set(left, right, &diff, rows)` is the comparison as a file**, the shape
`monitor::log_result_set` gives a change log, so it goes out through the ordinary `export`
renderers rather than a second set (`an_export_renders_through_the_ordinary_csv_renderer`): a
`Change` column in the table's own words (`RowChange::label`), then one per `shared` column
(`an_export_is_a_change_column_then_the_shared_ones_typed_as_both_sides`). **The label column's
name is clear of every shared one** — `Change`, with `_` appended while a shared column has that
name in any case — because a result with its own `change` column would give a JSON row two keys
of one name, and readers keep the last, losing the label
(`the_change_column_never_takes_a_shared_columns_name`). **Only the compared columns are in the
file**: a binary column left out of the comparison and a column one side lacks are not, and the
file carries no note of them, the formats having nowhere to put one — the modal's notes line is
what says so, on screen. An added or removed row carries
its values from its own side; a changed row carries `old → new` in the columns that differ and
the first result's value everywhere else, the sides agreeing there
(`an_export_has_each_kind_of_row_in_the_tables_order`); which columns differ is read by index
through a name→index map of `shared` built once per export, rather than a scan of the change's
column list per shared column per row. The arrow is `monitor::TRANSITION`,
`pub(crate)` now so the two projections spell it once. A value standing alone keeps its type
(`Cell::to_value`) and a NULL stays a real NULL, so each format writes it its own way; inside a
transition, which is one cell, it can only be the text `NULL`
(`a_null_inside_a_transition_is_the_word`). **It is the whole value, never `row_cells`'
one-line rendering**: that line is the table's answer to a fixed row height, and a file is read
away from the modal (`an_export_keeps_a_multi_line_value_whole`). **A shared column keeps a type
only when both sides agree on it** — `type_name` compared case-insensitively, the first result's
spelling written — **and is TEXT otherwise**, unlike the log's all-TEXT columns. Both halves are
XLSX's doing, which makes a decimal cell a number off its column's `type_name`
(`export::is_fixed_point`, then `exact_decimal`): agreeing types keep a `DECIMAL` money column
summing in a spreadsheet, while the first side's type alone would be no promise about an added
row, whose value is the second side's — under a first side's `DECIMAL`, a second side's
`VARCHAR` `007` would land as the number 7
(`a_column_the_sides_type_differently_is_exported_as_text`). A transition, which `exact_decimal`
cannot parse, is written as text whatever the column says; the *Change* column is TEXT. `rows`
is whichever of `table_rows` the caller
passes, kept in the order given, and one out of range is left out
(`an_export_holds_only_the_rows_it_is_given`). The modal passes the filtered ones, through
**`TableRow::shown_by(filter)`, the one filter predicate** — the table asks it too, so the file
holds what the modal shows (`a_filter_shows_its_own_kind_and_no_filter_shows_every_row`).
`DIFF_FORMATS` is `monitor::LOG_FORMATS` itself, SQL left out for the log's reason: it renders
`INSERT INTO <table>`, and these rows are observations about two results, not rows of a table
(`diff_formats_are_the_logs_and_never_sql`).
