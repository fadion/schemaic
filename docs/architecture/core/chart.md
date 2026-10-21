# `core/chart.rs`

`chart.rs` — **the numbers behind a result's *Chart*** (`ui/chart_view.rs`): which columns can
be plotted, one column's values against another's labels, a histogram of one column, and the
axis a range is drawn against. Pure over a `ResultSet`, so the view only paints what this
answers — and over **the rows already read**, never a fetch of its own: a chart of a capped
result is a chart of the rows read, which the modal says under it.
**A number is a cell whose *text* reads as one** — trimmed, parsed as a finite `f64`; a NULL, a
non-number and a NaN are not — rather than one whose wire tag says so, because `decimal` and
`numeric` arrive as text from every engine (the storage model `aggregate.rs` above describes)
and are the columns most worth charting. The cost of asking the text is the other direction: a
`varchar` column that happens to hold only digits is offered too. Unlike `aggregate.rs` this is
plain `f64`, not fixed-point — nothing here prints a cell's value back, only a position and a
tick. `numeric_columns(rs)` is every column whose non-NULL cells all read as numbers, with at
least one non-NULL (`a_column_is_numeric_when_every_value_reads_as_a_number`), so a column of
nothing but NULLs is not offered. `picker_labels(names, reserved)` is the modal's pickers' name
per column, no two alike and none equal to a reserved option: the first of a name stays bare, a
repeat takes its 1-based position (`id (3)`).
`ChartKind::{Bar, Line, Histogram}` (`ALL`, `label`). `series(rs, x: Option<usize>, y)` →
`Series { labels, values, skipped }`, in row order: `y`'s numbers labelled by `x`'s text,
`"NULL"` for a NULL label, and the 1-based row number with no `x`; `skipped` counts the rows left
out, a NULL or non-numeric value **and** everything past `MAX_POINTS` (2,000 — past it a bar is
thinner than a pixel on any panel the chart fits), so the modal can say how many it did not draw
(`a_series_pairs_labels_with_values_and_counts_what_it_left_out`,
`a_long_result_draws_at_most_the_cap`). `histogram(rs, col)` → `(Vec<Bin { lo, hi, count }>,
skipped)` over `bins(values)`: equal-width bins, how many by Sturges' rule (`⌈log₂ n⌉ + 1`,
clamped to 50), each `[lo, hi)` with **the last one closed** so the maximum lands in it, and a
flat column one bin holding everything (`a_histogram_bins_every_value_and_closes_its_last_bin`).
A histogram takes every numeric row — the cap is `series`' alone — so its `skipped` is only the
NULLs and non-numbers.
`ticks(lo, hi, want)` is the value axis: a step of 1, 2 or 5 × 10ᵏ, the first at least the raw
`(hi − lo) / want`, covering the range from `floor(lo / step)` up past `hi`. **Each tick is
counted, `(first + i) × step`, never accumulated**, the step is never finer than four times the
spacing of doubles at the range's magnitude, and the count is bounded: distinct values a couple
of ULPs apart — snowflake ids minted in one millisecond, charted as a Line — once picked a step
that adding never moved, and the loop grew its vector on the UI thread until memory ran out
(`ticks_terminate_at_extreme_magnitudes`). A flat range is widened by 1 either side so it still
gets an axis, and there are always at least two ticks; a non-finite bound gets none
(`ticks_are_round_and_cover_the_range`).
`Plot::{Series(kind, Series), Bins(Vec<Bin>)}` is what is drawn — laid out by `chart_draw.rs`
below, for the canvas and the saved file alike — `Plot::of(rs, kind,
value, label)` → `(Plot, skipped)`. `Plot::range` is the value axis's range: **a bar or a bin
stands on 0, so 0 is in it; a line follows its values** (`a_line_range_does_not_force_zero`) —
which is exactly what routes a narrow band of large numbers into `ticks`. `Plot::x_labels` is
first, middle and last with where each sits across the plot, cut at 18 characters, a histogram's
being its edges and midpoint (`x_labels_are_first_middle_and_last`); and `caption(rows, kind,
skipped, truncated)` the line under the canvas, a histogram's skipped count being NULLs alone, so
it never names the series' cap (`a_histogram_caption_does_not_mention_the_point_cap`).
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
(`a_range_too_large_to_draw_has_no_axis_and_says_so`).
