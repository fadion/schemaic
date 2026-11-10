# `core/chart.rs`

`chart.rs` — **the numbers behind a result's *Chart*** (`ui/chart_view.rs`): which columns can
be plotted, one column's values against another's labels, against a second numeric column, or
summed per label and per a third column's segments, a histogram of one column, and the axis a
range is drawn against. Pure over a `ResultSet`, so the view only paints what this
answers — and over **the rows already read**, never a fetch of its own: a chart of a capped
result is a chart of the rows read, which the modal says under it.
**A number is a cell whose *text* reads as one** — trimmed, parsed as a finite `f64`; a NULL, a
non-number and a NaN are not — rather than one whose wire tag says so, because `decimal` and
`numeric` arrive as text from every engine (the storage model `aggregate.rs` above describes)
and are the columns most worth charting. The cost of asking the text is the other direction: a
`varchar` column that happens to hold only digits is offered too. Unlike `aggregate.rs` this is
plain `f64`, not fixed-point — nothing here prints a cell's value back, only a position and a
tick. `numeric_columns(rs)` is every column whose non-NULL cells all parse as numbers, with at
least one finite (`a_column_is_numeric_when_every_value_reads_as_a_number`), so a column of
nothing but NULLs is not offered. **A NaN or an infinity parses but is no number an axis can
hold**: it keeps the column eligible and is skipped where it would be drawn, counted as a NULL is
— one such cell used to take the whole column out of every picker, the scatter's X axis included
(`a_nan_or_infinite_cell_is_skipped_not_disqualifying`). `picker_labels(names, reserved)` is the modal's pickers' name
per column, no two alike and none equal to a reserved option: the first of a name stays bare, a
repeat takes its 1-based position (`id (3)`).
`ChartKind::{Bar, Line, Histogram, Scatter, StackedBar}` (`label`; `ALL` in the *Chart* picker's
order — Bar, Stacked bar, Line, Scatter, Histogram). `series(rs, x: Option<usize>, y)` →
`Series { labels, values, skipped }`, in row order: `y`'s numbers labelled by `x`'s text,
`"NULL"` for a NULL label, and the 1-based row number with no `x`; `skipped` counts the rows left
out, a NULL or non-numeric value **and** everything past `MAX_POINTS` (2,000 — past it a bar is
thinner than a pixel on any panel the chart fits), so the modal can say how many it did not draw
(`a_series_pairs_labels_with_values_and_counts_what_it_left_out`,
`a_long_result_draws_at_most_the_cap`). `histogram(rs, col)` → `(Vec<Bin { lo, hi, count }>,
skipped)` over `bins(values)`: equal-width bins, how many by Sturges' rule (`⌈log₂ n⌉ + 1`,
clamped to 50), each `[lo, hi)` with **the last one closed** so the maximum lands in it, and a
flat column one bin holding everything (`a_histogram_bins_every_value_and_closes_its_last_bin`).
A histogram takes every numeric row — the cap is the other kinds' alone — so its `skipped` is
only the NULLs and non-numbers.
`points(rs, x, y)` → `Points { xs, ys, skipped }` is a scatter's: one pair per row with a number
in **both** columns, a row missing either skipped, under the same `MAX_POINTS` cap
(`a_scatter_pairs_two_columns_and_skips_a_row_missing_either`). `stacked(rs, label, by, value)`
→ `Stacked { labels, stacks, other, values, skipped }` is a stacked bar's, and it **sums** rather
than drawing a bar per row: `values[label][stack]` adds up every row's value for that pair, labels
and stacks each in the order they first appear and `0.0` where a label has none of a stack,
because a stacked bar's rows name the same label once per segment — so a `GROUP BY a, b` result
reads straight in (`a_stacked_bar_sums_each_label_and_stack_in_first_seen_order`). A NULL label
or segment reads `"NULL"` — drawn so, but **keyed apart** from the text `'NULL'`, now that rows
are summed by key (`a_null_label_or_stack_is_not_the_text_null`) — and no `by` is one segment per label
(`a_stacked_bar_without_a_stack_column_has_one_segment`). At most `MAX_STACKS` (8) segments are
named — one colour each, and no palette tells more apart — and every later one is summed into one
more, `OTHER`, last, with `other` set (`stacks_past_the_cap_fold_into_one_other_segment`);
`OTHER` is core's English, which `Stacked::name_other` renames into the view's language and
leaves alone when nothing was folded. **The cap here is on bars, not rows**: a row whose label is
new once `MAX_POINTS` labels exist is skipped, while a row adding to a label already there still
counts (`a_stacked_bar_draws_at_most_the_cap_of_labels`).
`ticks(lo, hi, want)` is the value axis: a step of 1, 2 or 5 × 10ᵏ, the first at least the raw
`(hi − lo) / want`, covering the range from `floor(lo / step)` up past `hi`. **Each tick is
counted, `(first + i) × step`, never accumulated**, the step is never finer than four times the
spacing of doubles at the range's magnitude, and the count is bounded: distinct values a couple
of ULPs apart — snowflake ids minted in one millisecond, charted as a Line — once picked a step
that adding never moved, and the loop grew its vector on the UI thread until memory ran out
(`ticks_terminate_at_extreme_magnitudes`). A flat range is widened by 1 either side so it still
gets an axis, and there are always at least two ticks; a non-finite bound gets none
(`ticks_are_round_and_cover_the_range`).
`Plot::{Series(kind, Series), Bins(Vec<Bin>), Points(Points), Stacked(Stacked)}` is what is drawn
— laid out by `chart_draw.rs` below, for the canvas and the saved file alike — `Plot::of(rs, kind,
picks: Picks)` → `(Plot, skipped)`. `Picks { value, label, x, by }` names every column any kind
may read, and which of them a kind reads is `of`'s: `value` always, `label` for a bar, a line or a
stacked bar (`None` numbering the rows), `x` for a scatter's horizontal axis — a scatter with none
has nothing to draw — and `by` for a stacked bar's segments, `None` being one segment
(`plot_of_answers_each_new_kind_from_its_picks`). `Plot::range` is the value axis's range: **a
bar or a bin stands on 0, so 0 is in it; a line follows its values**
(`a_line_range_does_not_force_zero`) — which is exactly what routes a narrow band of large numbers
into `ticks` — and so does a scatter, a dot being a point rather than a bar
(`a_scatter_follows_its_values_and_labels_its_x_ticks`). **A stacked bar spans its most negative
and most positive totals separately**, each bar's negative segments summed apart from its positive
ones, with 0 always in: `chart_draw.rs` stacks the two sides away from zero in opposite
directions, so a bar's net total would not reach the ends of its segments
(`a_stacked_range_spans_the_positive_and_negative_totals`). `Plot::x_range` is a scatter's
horizontal axis — the first and last of `ticks` over its X values, the span its dots are placed
across — and `None` for every other kind, whose x is a slot per label. `Plot::x_labels` is first,
middle and last with where each sits across the plot, cut at 18 characters by `short_label` (17
and an ellipsis — the legend's cut too; `short_label_cuts_past_eighteen_characters`) — a stacked bar's from
its labels as a series' are — a histogram's being its edges and midpoint
(`x_labels_are_first_middle_and_last`), and a scatter's **every X tick** at its position,
labelled as one axis by `tick_labels`; and `caption(rows, kind, skipped, truncated)` the line
under the canvas, a histogram's skipped count being NULLs alone, so it never names the series' cap
(`a_histogram_caption_does_not_mention_the_point_cap`) — a scatter's and a stacked bar's name it,
as a series' does.
`tick_label(v)` prints at most two decimals with
trailing zeros dropped, `-0` as `0`, and shortens `≥ 1e4`/`1e6`/`1e9` to `k`/`m`/`b`
(`tick_labels_drop_noise_and_shorten_large_values`) — the x-axis's form. **The value axis is
labelled as one, `tick_labels(ticks)`, its precision set by the step**: while seven significant
digits tell the ticks apart each is the scaled form with as many decimals as the step needs
(`0.001` … for a fraction column of 0..0.004) or an exponent past `1e15`; finer than that the
first tick is written in full and the rest as its offset (`1e17`, `+96`; `1.7e12`, `+200k`).
Per-tick labels of two decimals read the same on a narrow band — `100000000b` twice for the
snowflake ids 3ea81a7d made paintable — and a value near `1e150` printed a hundred digits and
left the plot no width (`an_axis_never_labels_two_ticks_alike`, pairwise distinct and at most 16
characters across the extreme ranges). **A range the axis cannot hold is said**: `ticks` answers
none for a range whose step overflows or whose top it cannot reach (values at or near
`±f64::MAX`), and `Plot::undrawable` is the note the modal shows in the canvas's place — it
painted nothing, and near `MAX` drew the line above the plot
(`a_range_too_large_to_draw_has_no_axis_and_says_so`). A scatter has two axes to fail, so its
`undrawable` answers the same words when `x_range` cannot be built either
(`a_scatter_with_an_undrawable_x_axis_says_so`).
