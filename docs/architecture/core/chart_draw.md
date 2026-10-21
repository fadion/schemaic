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
(60), or `Marks::Bars(Vec<Area>)` for bars and histogram bins — the zero line `base` (the
axis's nearer end when 0 is off it), and the under-axis labels, clamped inside the box
(`labels_under_the_axis_stay_inside_the_box`). Text is measured by a **caller-supplied
closure**, since the font system is the UI's and this crate has none — `erd_export::ellipsize`
takes its measurer the same way. **The order it calls that closure in is a contract**, stated
on `layout`: each of `Scene::ticks` first, in order, then each of `Scene::under`. The canvas
keeps the layouts its measurer builds and draws the tick labels from them by position, so a
reorder here would paint one tick's text at another's place
(`the_tick_labels_are_measured_first_in_order_then_the_labels_under_the_axis`). A tick label is `Align::End`: placed by its measured box but
anchored at its right edge, so an SVG renderer whose glyphs come out a hair wider than the
canvas's still lines the column up against the plot
(`the_gutter_is_the_widest_tick_label_and_the_labels_end_at_it`). The tick labels, and a
histogram's edge labels under the axis, are written in `figures` — the interface's
`NumberMarks`, which the export carries too, being a picture of the chart — while a series'
labels are the label column's values and are never re-marked
(`the_tick_labels_are_written_in_the_marks_given`, `only_a_histogram_s_edges_take_the_marks`;
*Figures in the interface's marks*). It answers `None` for no
axis (`Plot::undrawable`'s range), for a box too small for the gutter, and **for an empty
plot** — checked first, because every mark's width divides by the count
(`nothing_to_lay_out_without_an_axis_or_a_box`).
`Metrics` is the spacing, a parameter rather than constants because the two painters size it
differently: `Metrics::BASE` is UI scale 1, which the export uses as it is, and the canvas
passes `BASE.map(theme::scaled)` — **each value through `scaled`, not one factor**, because
`scaled` rounds per value, so multiplying by a scale would not reproduce the rest of the UI's
spacing. `to_svg(scene, &SvgColors, font_size, pad)` writes the scene on a `background` rect
with `pad` round it, text centred on its box's middle line with `dominant-baseline="central"`
as the ER diagram's file does; colours are `SvgColors` strings the caller resolves from the
theme. It reuses `erd_export`'s `n` (two-decimal coordinates) and `svg_text` (escaping), now
`pub(crate)` for it, rather than a second copy of either
(`the_svg_escapes_a_label`).
