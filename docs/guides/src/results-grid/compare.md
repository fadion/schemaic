# Comparing results

Compare two results of the same tab row by row: what was added, what was removed, and what
changed. The usual use is before-and-after — keep a result, change something, run again, and see
exactly what moved.

## Comparing two runs

1. Run the query, then right-click its chip in the results strip and choose **Pin**.
2. Make your change, and run the query again. The pinned result stays beside the new one.
3. Right-click either chip and choose **Compare with …** and the other result.

Nothing is re-run: the comparison uses the two results as they were read. The earlier one is
**A** and the later **B**, whichever chip you right-clicked, so *added* means added since. The ⇄
between their cards turns the comparison round.

## Reading the comparison

The differences are listed in a table under the columns the two results share. The pills above it
narrow it to **Changed**, **Added** or **Removed** rows, or show **All**. Changed values are blue,
added rows green, removed rows red and struck through.

**Match rows by** decides which rows count as the same row. Schemaic starts with a column called
`id`, or else the first column whose values are unique and never NULL on both sides. Pick another
column if that's wrong, or **Whole row**, where nothing identifies a row: a row is then either in
both results or in one, never "changed".

A few things the summary will tell you about:

- Columns are matched by name. A column only one side has is listed, and left out.
- Binary columns aren't compared, since the grid holds only their size.
- If a key value appears twice on one side, only its first row is compared.
- If either result was capped, the rows past the cap aren't compared.

Values are compared as the grid shows them, so `1` and `1.0` count as different.

**Export the comparison…** saves the rows on screen, under the current filter, in any of the
[export formats](exporting.md).
