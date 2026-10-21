# `ui/chart_view.rs`

`chart_view.rs` — the **Chart** modal (`chart_overlay`): a bar, line or histogram of the result
on screen, over `core::chart`, opened by setting `overlay.chart` to a `ChartTarget { rs:
Arc<ResultSet>, title }` (re-exported as `schemaic_ui::ChartTarget`; the app creates the signal
beside `used_by`) from the results strip's chart icon (*Data grid*), which titles it *"Chart —
this result"*. Read-only like Properties — it changes nothing in the result or the database: a
title, the pickers with a save icon at the row's right end, the chart, a split footer of a
status line on the left and **Close** on the right, and Escape and the backdrop closing it. The modal
is keyed on the `Arc`'s pointer, so charting
another result rebuilds it and nothing else does.
**Three pickers**: *Chart* — the kind, a `settings::focusable_dropdown`, tab 10; *Values* — the
numeric columns only (`chart::numeric_columns`, read once per open), a
`table_designer::focusable_owned_dropdown`, tab 20, starting on the **last** numeric column; and
*Labels* — *Row number* or any column, a `table_designer::focusable_indexed_dropdown` told the
pick by position (`after_none_row`), so a column named like the interface's *Row number* is
still that column; tab 30, starting on the first column that is not the
value, and not built at all for a histogram, whose labels are its bins — a `dyn_container` on the
kind rather than `s.hide()`, since a hidden picker would still be a Tab stop. A picked name maps
back to its column by name, so the names are `chart::picker_labels`' — distinct, a repeated
name or one equal to *Row number* taking its 1-based position (`id (3)`); with the result's
raw names a self-join's second `id` could not be picked, the lookup answering the first
(`repeated_column_names_get_distinct_picker_labels`). The body is a `dyn_container` keyed on `(kind, value, label)`: *"No column of numbers to
chart in this result."* where there is none, *"Every value in that column is NULL."* where the
series or the bins came back empty, and otherwise the canvas, 360 high, with one faint line under
it, `chart::caption` — *"N rows · K not drawn (NULL, or past 2000) · the rows read, not the whole
result"*, the middle clause when anything was skipped (a histogram's reading *"K NULL, not
drawn"*, since it has no cap) and the last when the result is `truncated`. What is drawn is a
`chart::Plot` (`Plot::of`), whose range and under-axis labels the canvas asks rather than
decides.
**The canvas is a painted view, `ChartCanvas`** — floem 0.2 has no plotting, so it is a custom
`View` with a `paint` of its own, as the frozen header's `Hatch` (*Data grid*) and the ER
diagram's `EdgeCanvas` (under `erd_view.rs`) are. **Where everything goes is
`core::chart_draw::layout`'s**, not `paint`'s — the export draws the same `Scene` — so `paint`
only strokes and fills what it is handed, at the canvas's size with `Metrics::BASE.map(
theme::scaled)` and text measured at `theme::font_hint` by floem's own `TextLayout`. **Its
measurer keeps what it builds**: each layout, shaped in the ticks' colour, goes onto a list, and
the tick labels are drawn from that list by position — so a tick label is shaped once per paint,
not once to measure and again to draw. That rests on `layout`'s stated call order (ticks first,
in order; `chart_draw.rs` above), and only the at most three labels under the axis are laid out
again, in their own colour. The export measures through `measure`, the same `TextLayout` at the
file's size. The value axis is `chart::ticks` over the drawn range — a
bar's or a histogram's range always includes 0 — with grid lines in `theme::border` and the
tick labels drawn with `cx.draw_text`, so the axis is the app's face at the app's scale; the
gutter is the widest label. Bars, or a 2px polyline (with a dot per point while there are at
most 60), or contiguous histogram bins, all in `theme::accent`; a zero line over the marks; and
three labels under the axis — first, middle and last, cut at 18 characters, a histogram's being
its edges and midpoint as tick labels. **Every colour is read at paint time**, so a theme
switch repaints in the new one, and the text helper takes its colour as `fn() -> Color` for the
same reason the themable-colour invariant gives.
**Saving is the Live Monitor's export control, not a pair of buttons** — a feature wanted this
rarely kept out of the footer, and a bare glyph with no background or border sits in a modal
better than a boxed toolbar control. It is built as `monitor_view.rs`'s Export is:
`widgets::toolbar_icon` with the `DOWNLOAD` icon (muted, the text colour on hover) and the
tooltip *"Save the chart…"*, inside `widgets::in_ring_button` at `EXPORT_TAB` (40) — so it **is**
a stop in the modal's Tab ring, after the three pickers, and Enter or Space opens its menu.
`controls` takes it as its final argument and puts a `flex_grow` spacer before it, in a row that
is `width_full` and `items_center`, so the icon sits at the row's right end. It stands under a
blank caption with the pickers' own gap, so the row's centring puts its middle on the
**dropdowns'** rather than the row's — exact at any UI scale, where a fixed nudge would not be.
Pressing it raises the app's shared popup — `overlay.popup_menu`, the one surface painted
above this modal — at `PopupAnchor::BelowIcon` off the **ring wrapper's** `layout_rect()`, the
monitor's rule: the wrapper's `ViewId` goes into `export_at` once the button exists, since the
closure that reads it has to be built before the view it anchors to. The menu is
`erd_view::export_menu_w()` wide — `pub(crate)` for this — with two entries,
`ErdExportFormat::Png.label()` and `Svg.label()` (*PNG image*, *SVG image*); `popup_anchor` is
written immediately before `popup_menu`, and pressing the icon again closes its own menu
(`widgets::menu_anchored_at`). **Closing the modal takes the menu with it** — `close` sets
`popup_menu` and `popup_anchor` to `None` before `target`, for the ER diagram's reason: the
shared popup outlives this modal and its entries hold this scope's signals. Every exit — ✕,
Escape, the backdrop, **Close** — routes through `close`. That does not cover the one exit
that bypasses it, a new target replacing this one under an open menu, which disposes the scope
with the menu still up; so the `save` an entry calls reads `kind`, `value` and `label` with
`try_get_untracked` and does nothing when any is gone, since a bare read of a disposed signal
panics. The entries reuse the ER diagram's formats (`export_doc` answers `None` for the four text ones,
since a chart is a picture) and its default name, `file_stem("<title> chart", fmt)`. **The
chart is captured from the pickers before the save dialog opens** (`export_doc`), for the ER
diagram's reason: the dialog is modal, the signals are this modal's, and the file should be
what the user was looking at when they clicked. It is laid out at a **fixed** 800×400 with
12px text and 16px of background round it (`EXPORT_W`, `EXPORT_H`, `EXPORT_FONT`,
`EXPORT_PAD`), at `Metrics::BASE` rather than the canvas's scaled spacing, so a chart saved at
a zoomed UI is the same picture as one saved at 100%; a PNG is that at 2× (`EXPORT_PNG_SCALE`)
— `a_saved_chart_is_a_png_of_the_export_size_and_an_svg` pins the pixel size. The colours are
the live theme's (`erd_view::hex`, `pub(crate)` for this), on `bg_panel`, the colour the canvas
sits on. The SVG is built on the UI thread, because its labels are measured by floem's font
system, and that is affordable because the document is **bounded whatever the result's size**:
a series draws at most `chart::MAX_POINTS` (2,000) marks and a histogram at most 50 bins
(`export_doc`'s comment). The PNG's rasterise is left to the worker as
`ErdDoc::Raster` (under `erd_view.rs`). The write is the ER diagram's own writer,
`TabActions::export_erd`, which `modals.rs` passes into `chart_overlay` — off the UI thread,
through `persist::write_file_atomic` — rather than a second one, and the dialog and the reading
of its outcome are the diagram's too: `save` hands the document to `erd_view::save_document`
with *"Save chart"* as the dialog's title. Nothing drawable (no numeric
column, or a plot `layout` refuses) says *"Nothing to save."* rather than opening a dialog.
**The modal reports for itself**, a notice in the footer's status line, because the app's
shared error modal is painted *under* this one — the diagram's `notice_bar` reason. The
outcome arms are `save_document`'s, spelled out, `Cancelled` saying nothing. A confirmation
fades after `NOTICE_LINGER` (4 s, the diagram's) behind a sequence
number, and a failure stays; the notice is reached through `try_get_untracked`, because the
completion can land after the modal closed and disposed its scope. **A notice is about the
chart it was said of**: an effect over `kind`, `value` and `label` clears it when a picker
changes — skipping its first run, which is the build, not a change — or *"Nothing to save."*
would stand, as a failure does, over a column that can now be saved.
**Its own words are the catalogue's** (`chart-*`), **Close** `dialog-close`,
and the kinds `chart_kind_label(ChartKind)` (the core-words rule under *UI conventions*). *Row
number* is `row_number()`, a message where it was the constant `ROW_NUMBER`, and it is a
**value as well as a label**: the picker hands back the text it shows, so a pick is compared
with `row_number()`, and `picker_labels` reserves it, so the entry, the comparison and the name
a same-named column steps aside for are all one language. The line under the canvas is
`caption_text`, `chart::caption`'s clauses as `chart-caption-*` messages
(`english_captions_every_chart_as_core_does`), and the note in place of an undrawable range
`chart-too-large`, held to `Plot::undrawable`'s English by a test — the view asks `undrawable`
only whether it is `Some`, and `chart::caption` is now read by tests alone. Its *"Saved {name}"* and
that notice's *the file* fallback are not `chart-*`: they are `save_document`'s `erd-saved` and
`erd-unnamed-file`, shared with the diagram, so they are translated with `erd_view.rs`.
Mounted in the workspace group directly after `used_by_overlay`, counted by
`workspace_modals_up` as a plain term (`chart_open`) — it raises nothing — and on
`modal_backdrop_gate::PAINTS_A_BACKDROP`.
