# `core/chart_draw.rs`

`chart_draw.rs` — **where a chart's marks go**: the geometry of a `chart::Plot` in a box of a
given size, and that geometry written out as a standalone SVG. **One layout, two painters** —
`ui/chart_view.rs`'s `ChartCanvas::paint` draws the `Scene` this returns, and the modal's save
menu writes the same `Scene`, so the file is the chart on screen rather than a second drawing of it
that drifts. The arithmetic was inline in `paint` until the export needed it; it moved here
rather than being copied, the bargain `erd_export`'s `SvgScene` strikes with `EdgeCanvas`.
`layout(plot, width, height, &Metrics, figures, measure) -> Option<Scene>` places the horizontal grid
lines (one per `chart::ticks` step, labelled as one axis by `tick_labels`), the tick labels
in a gutter as wide as the widest of them plus `gutter_pad`, the marks — `Marks::Line { points,
dots }`, a point per value at its slot's centre and dots while there are at most `MAX_DOTS`
(60), `Marks::Bars(Vec<Area>)` for bars and histogram bins, `Marks::Dots` for a scatter — a
point per pair, its x on the number line `Plot::x_range` spans rather than a slot
(`a_scatter_places_each_dot_on_both_axes`) — or `Marks::Stacks(Vec<(usize, Area)>)` for a stacked
bar, each segment with the stack it is a piece of: **positive segments stack up from zero and
negative ones down from it**, in stack order so a stack sits at the same place in every bar that
has it, and a zero is no segment at all (`stacked_segments_stack_up_and_down_from_zero`) — the
zero line `base` (the axis's nearer end when 0 is off it), and the under-axis labels, clamped
inside the box (`labels_under_the_axis_stay_inside_the_box`). A scatter also has `Scene::vgrid`,
a vertical grid line per X tick. A stacked bar of more than one stack has `Scene::legend`, a
`LegendEntry { stack, swatch, label }` per stack, flowed left to right from the plot's left edge
under the labels and wrapped at its right one, **its rows' height taken off the plot's** so it
stays inside the box (`a_wide_legend_wraps_onto_more_rows`). Each name is measured **after
`chart::short_label` cuts it**, as the axis labels are, because an entry first on its row is never
wrapped and a long one ran past the canvas and out of the exported file
(`a_long_stack_name_stays_inside_the_canvas`); a single stack is a bar chart and
has none, the plot keeping its full height. `Scene::other_stack` is the folded *Other* segment's
index. `stack_fill(scene, stack, palette, mark, other)` is **the one answer to a segment's or a
swatch's colour**, generic over the colour type so the canvas asks it in `Color`s and the export in
`SvgColors` strings: the `mark` colour for a single stack, `other` for the folded one, and
otherwise the `palette` in turn — so the file's colours are the screen's
(`a_stacked_bar_has_a_legend_in_its_stacks_colours`). Text is measured by a **caller-supplied
closure**, since the font system is the UI's and this crate has none — `erd_export::ellipsize`
takes its measurer the same way. **The order it calls that closure in is a contract**, stated
on `layout`: each of `Scene::ticks` first, in order, then each label under the axis, then each
of `Scene::legend`. The canvas
keeps the layouts its measurer builds and draws the tick labels from them by position, so a
reorder here would paint one tick's text at another's place
(`the_tick_labels_are_measured_first_in_order_then_the_labels_under_the_axis`,
`the_legend_is_measured_after_the_labels_under_the_axis`). **`Scene::under` can be fewer than
were measured**: a scatter labels every X tick, and where one would reach its neighbour the row is
thinned to every other label until none touch, never below two
(`a_scatter_s_x_labels_are_thinned_until_none_touch`) — after measuring, since the overlap is
only known from the sizes. Its `vgrid` is not thinned with them and keeps a line at every tick.
Only the ticks' place in the order is load-bearing for the canvas; it lays the rest out again.
A tick label is `Align::End`: placed by its measured box but
anchored at its right edge, so an SVG renderer whose glyphs come out a hair wider than the
canvas's still lines the column up against the plot
(`the_gutter_is_the_widest_tick_label_and_the_labels_end_at_it`). The tick labels, and a
histogram's edge labels and a scatter's X ticks under the axis, are written in `figures` — the
interface's `NumberMarks`, which the export carries too, being a picture of the chart — while a
series' and a stacked bar's labels are the label column's values, and a legend's names the stack
column's, and are never re-marked
(`the_tick_labels_are_written_in_the_marks_given`, `only_a_histogram_s_edges_take_the_marks`;
*Figures in the interface's marks*). It answers `None` for no
axis (`Plot::undrawable`'s range, or a scatter's X axis), for a box too small for the gutter, and **for an empty
plot** — checked first, because every mark's width divides by the count
(`nothing_to_lay_out_without_an_axis_or_a_box`).
`Metrics` is the spacing, a parameter rather than constants because the two painters size it
differently: `Metrics::BASE` is UI scale 1, which the export uses as it is, and the canvas
passes `BASE.map(theme::scaled)` — **each value through `scaled`, not one factor**, because
`scaled` rounds per value, so multiplying by a scale would not reproduce the rest of the UI's
spacing. `legend_gap` is the room between two legend entries and above the legend.
`to_svg(scene, &SvgColors, font_size, pad)` writes the scene on a `background` rect
with `pad` round it, text centred on its box's middle line with `dominant-baseline="central"`
as the ER diagram's file does; colours are `SvgColors` strings the caller resolves from the
theme — `stacks` the segment palette in stack order and `other` the folded segment's, both read
through `stack_fill` — and a scatter's dots are filled at `DOT_OPACITY` (0.7), which the canvas
applies too, so overlapping dots darken where they crowd. It reuses `erd_export`'s `n` (two-decimal coordinates) and `svg_text` (escaping), now
`pub(crate)` for it, rather than a second copy of either
(`the_svg_escapes_a_label`).
