# Charts

The chart icon on the results toolbar (**Chart the results**) draws the result on screen as a bar
chart, a line or a histogram. It's for a quick look at a shape — a trend, a spread, an outlier —
without leaving Schemaic.

## Making one

It opens as a bar chart of the last column of numbers, labelled by the first other column — the
measure usually follows what it measures. Above the chart, choose:

| Control     | What it sets                                                              |
|-------------|---------------------------------------------------------------------------|
| **Chart**   | **Bar**, **Line** or **Histogram**                                        |
| **Values**  | The column to plot — any column whose values read as numbers, `DECIMAL` included |
| **Labels**  | The column to label each bar or point with, or **Row number**             |

A histogram groups one column's values into ranges — up to 50 — and counts the rows in each, so it
has no labels to choose.

## What it shows

A chart draws the rows the grid has read, not the whole table: for a capped result, it is a chart
of the first rows. The caption under it says how many rows it drew and what it left out — NULLs,
and for a bar or line chart anything past the first 2,000 points.

If the result has no numeric column, the chart says so:
**No column of numbers to chart in this result.**

## Saving it

**Save the chart…** writes it as a PNG image or an SVG file.
