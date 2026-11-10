# Charts

The chart icon on the results toolbar (**Chart the results**) draws the result on screen as a bar
chart, a stacked bar, a line, a scatter or a histogram. It's for a quick look at a shape — a trend,
a spread, an outlier — without leaving Schemaic.

## Making one

It opens as a bar chart of the last column of numbers, labelled by the first other column — the
measure usually follows what it measures. Above the chart, choose:

| Control      | What it sets                                                              |
|--------------|---------------------------------------------------------------------------|
| **Chart**    | **Bar**, **Stacked bar**, **Line**, **Scatter** or **Histogram**          |
| **Values**   | The column to plot — any column whose values read as numbers, `DECIMAL` included |
| **Labels**   | The column to label each bar or point with, or **Row number**             |
| **X axis**   | A scatter's horizontal axis, in place of **Labels** — another column of numbers |
| **Stack by** | A stacked bar's segments, beside **Labels** — any column, or **None**     |

A histogram groups one column's values into ranges — up to 50 — and counts the rows in each, so it
has no labels to choose.

A scatter draws one dot per row, **Values** up and **X axis** across, both on a number scale. A row
missing either number is left out.

## Stacked bars

A stacked bar is one bar per label, split into a segment per value of the **Stack by** column, and
each segment is the sum of its rows' values. So a grouped query charts as it stands:

```sql
SELECT month, region, SUM(sales) FROM orders GROUP BY month, region;
```

With `month` as **Labels**, `region` as **Stack by** and the sum as **Values**, each month is one
bar split by region. A legend under the chart names each segment's colour.

- Past eight segments, the rest are summed into one more, **Other**.
- Negative values stack downward from zero.
- **None** makes each bar one segment — a bar chart that adds up rows sharing a label.

## What it shows

A chart draws the rows the grid has read, not the whole table: for a capped result, it is a chart
of the first rows. The caption under it says how many rows it drew and what it left out — NULLs,
and for every kind but a histogram anything past the first 2,000 points.

If the result has no numeric column, the chart says so:
**No column of numbers to chart in this result.**

## Saving it

**Save the chart…** writes it as a PNG image or an SVG file.
