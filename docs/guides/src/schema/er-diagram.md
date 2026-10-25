# ER diagram

The ER diagram draws tables as cards and their foreign keys as lines between them. It is for
reading a schema — nothing you do in it changes the database.

## Opening it

Right-click in the schema tree and choose **ER diagram**:

- on a *database*, to see the whole database. Tables with no foreign keys at all are left out,
  and a chip in the toolbar counts them;
- on a *table*, to see that table and the tables it links to directly.

A foreign key that points into another database shows as a small stub card.

Double-click a card to open that table.

## Getting around

- Drag a card to move it. Your arrangement is remembered for next time; the reset button (a
  circular arrow, at the right of the toolbar) puts everything back where Schemaic placed it.
- Drag empty space (or drag with the middle button) to pan. The wheel scrolls up and down,
  <kbd>Shift</kbd>+wheel scrolls sideways, and <kbd>Ctrl</kbd>+wheel zooms around the pointer.
- The fit button (a square frame, next to the zoom controls) zooms to show the whole diagram.
- A wide table shows only its key columns, with a row to expand the rest.

## Finding a table or column

Press <kbd>Ctrl</kbd>+<kbd>F</kbd> and type. Every match is highlighted at once, and if there's
only one the diagram pans to it. <kbd>Esc</kbd> closes the find bar, and a second <kbd>Esc</kbd>
closes the diagram.

## Exporting

The export button in the toolbar saves the diagram as a PNG or SVG picture, or as text in Mermaid,
DBML, PlantUML or Graphviz — useful for dropping into a README or a design document. The text
formats can also be copied straight to the clipboard. What you export is what you see: your
layout, collapsed tables and table colours included.
