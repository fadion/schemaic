# Writing SQL

The editor knows the schema of the database a tab is on, and uses it to complete, check and
explain what you type. None of this runs anything: the server only sees your SQL when you run it.

## Completion

The completion list opens as you type and offers what fits where the caret is — tables after
`FROM`, that table's columns after `WHERE`, a function's name, a keyword. Pick with
<kbd>↑</kbd>/<kbd>↓</kbd> and <kbd>Enter</kbd> or <kbd>Tab</kbd>.

- <kbd>Ctrl</kbd>+<kbd>Space</kbd> opens it where it didn't open on its own. Press it again to
  widen the list to every database and schema on the connection, not just the tab's own.
- After `JOIN`, the list offers whole joins along foreign keys, with the `ON` condition written for
  you.
- After `SELECT *`, a row offers to expand the `*` into the table's columns.
- A table written after `FROM` or `JOIN` gets a short alias from its initials — `orders o`,
  `order_items oi`. Turn this off under Settings › **Editor** › **Table aliases**.
- Items you've picked before rank higher next time.

**Inline predictions** are the faint text after the caret: the likely rest of what you're writing,
such as the `BY` after `GROUP`, the columns to group by, or the rest of a line you've written
before. <kbd>Tab</kbd> takes all of it, <kbd>Ctrl</kbd>+<kbd>→</kbd> one word, <kbd>Esc</kbd> turns
it down. Both settings live under Settings › **Editor**: **Inline predictions**, and
**Predictions from past statements** if you'd rather your old queries didn't show up.

## Reading as you write

- *Hover* over a table, column or function for a summary: its columns, type, default, arguments.
- <kbd>Ctrl</kbd>+click a name to open it — a table's data, at the column for a column, or a
  routine's editor.
- *Squiggles* mark syntax errors and mistakes the editor can see without the server, such as a
  column missing from `GROUP BY`. Where there's an obvious fix, <kbd>Alt</kbd>+<kbd>Enter</kbd>
  applies it. The count at the bottom of the window jumps to the first one.
- **Live database validation** (Settings › **Query**) goes further: on each pause it asks the server
  to prepare the statement under the caret, without running it, and shows the server's own error.
  It costs a round trip to the server, so it's off unless you turn it on.

## Formatting

| You want to…                                | Press                                                       |
|---------------------------------------------|-------------------------------------------------------------|
| Format the selection, or the whole tab      | <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>L</kbd> (**Format code**) |
| Format only the statement under the caret   | <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>Shift</kbd>+<kbd>L</kbd> (**Format statement**) |
| Put a statement on one line                 | <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>J</kbd> (**Make single line**) |

How formatting looks — keyword case, leading or trailing commas, wrap width, aligned columns — is
set under Settings › **Code formatting**. Names, strings and comments keep their case whatever you
choose.

## Find and replace

<kbd>Ctrl</kbd>+<kbd>F</kbd> opens the find bar over the editor, <kbd>Ctrl</kbd>+<kbd>H</kbd> opens
it with a replace field. Its toggles are **Match case**, **Whole word**, **Regular expression** and
**Find in selection**; **Replace** changes the current match and **All** every one.
<kbd>Ctrl</kbd>+<kbd>G</kbd> goes to a line.

## The right-click menu

Right-click in the editor for what applies to the statement under the pointer:

- **Plan** — the statement's query plan. See [Query plans](query-plans.md).
- **Format statement**, **Make single line**, **Copy as single line**.
- **Find usages** lists every place a table alias, CTE or column alias is used in the statement.
  **Rename alias** (or **Rename CTE**) renames it everywhere at once, and refuses a new name that
  would change what the query reads.
- **Create view** from a `SELECT`.
- With the AI assistant on: **Ask AI**, **Explain** and **Optimise**. See
  [AI assistant](../ai/README.md).

Every other editing key — comments, moving lines, duplicating, case — is in
[Keyboard shortcuts](../reference/shortcuts.md).
