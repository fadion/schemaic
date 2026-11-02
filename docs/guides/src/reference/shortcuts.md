# Keyboard shortcuts

Every shortcut below is also listed in the app: click the question-mark icon in the top bar, just left of
the settings gear (its tooltip says **Keyboard shortcuts**). The list there is grouped the same
way as this one, but shows your own keys where you've [changed them](#changing-a-shortcut) — this
page lists the defaults. Type in its search box to filter the rows by what they do or by their
keys: `ctrl+t` and `ctrlt` both find <kbd>Ctrl</kbd>+<kbd>T</kbd>.

On macOS, read <kbd>Cmd</kbd> for <kbd>Ctrl</kbd> — and <kbd>Ctrl</kbd> keeps working too. In an
interface language other than English, the app writes the key names as your keyboard does.

## Changing a shortcut

You can move these commands to other keys: everything in the *Global* table except
<kbd>Ctrl</kbd>+<kbd>1</kbd>…<kbd>9</kbd>, and, in the *Editor* table, Format code, Format
statement, Make single line and Go to line. Every other key is fixed; its row has no menu.

1. In the **Shortcuts** window, right-click the command's row and choose **Assign**. Double-clicking
   the row, or pressing <kbd>Enter</kbd> on it, does the same.
2. Press the new key combination. <kbd>Esc</kbd> cancels.

The change takes effect and is saved at once. The command palette and the editor's menu show the
new key too.

The same menu has **Clear**, which leaves the command with no key (its row then says **Not set**),
and **Reset to default**. From the keyboard, <kbd>Tab</kbd> moves between the command rows and
<kbd>Shift</kbd>+<kbd>F10</kbd> or the menu key opens a row's menu.

### When a key is taken

If another command already has the key, the row asks whether to take it: **Replace** gives it to
this command and leaves the other one with no key; **Cancel** keeps things as they were.
<kbd>Enter</kbd> and <kbd>Esc</kbd> answer too. **Reset to default** asks the same way if its
default has since gone to another command.

A key the editor uses can still mean something else in the results grid — <kbd>Ctrl</kbd>+<kbd>G</kbd>
is *Go to line* in one and *Go to row* in the other. A Global command, though, is heard
everywhere, so it can't take a fixed key from any part of the app.

### Keys you can't use

A key that's fixed, or reserved, is refused with the reason under the row, and the row keeps
listening for another:

- A letter, digit or <kbd>Tab</kbd> needs <kbd>Ctrl</kbd> (<kbd>Cmd</kbd> on macOS), or it would
  type.
- <kbd>Enter</kbd>, <kbd>Space</kbd>, the arrows and the other named keys need <kbd>Ctrl</kbd> or
  <kbd>Alt</kbd>. Function keys need nothing.
- <kbd>Esc</kbd> is never a command's key.
- <kbd>Ctrl</kbd> with <kbd>C</kbd>, <kbd>V</kbd>, <kbd>X</kbd>, <kbd>A</kbd>, <kbd>Z</kbd> or
  <kbd>Y</kbd>, <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>Z</kbd>, and <kbd>Ctrl</kbd> with an arrow,
  <kbd>Home</kbd>, <kbd>End</kbd>, <kbd>Backspace</kbd> or <kbd>Del</kbd> are kept for editing
  text.
- <kbd>Alt</kbd>+<kbd>F4</kbd> belongs to the desktop.

### Where your keys are kept

Your changes are saved in `keymap.json`, in `%APPDATA%\schemaic` on Windows and
`~/.config/schemaic` elsewhere. It holds only what you changed; a cleared command is `null`:

```json
{ "bindings": { "new-tab": "Ctrl+N", "close-tab": null } }
```

You can edit it by hand while the app is closed. Write <kbd>Ctrl</kbd> even on macOS — it means
<kbd>Cmd</kbd> there. The commands are `find-anywhere`, `command-palette`, `new-tab`, `close-tab`,
`next-tab`, `previous-tab`, `reopen-tab`, `open-file`, `save-file`, `save-file-as`,
`toggle-schema`, `toggle-ai`, `toggle-terminal`, `format-code`, `format-statement`,
`make-single-line` and `go-to-line`.

If something in the file can't be applied, Schemaic lists it in a notice at startup and that
command keeps its default — or, where two commands end up with the same key, one of them is left
with none, and the notice says which.

Settings › **General** › **Settings file** carries your shortcuts along with your other
preferences. Importing a file counts the shortcuts it changes in its question, and **Yes** makes
its shortcuts yours: a command it leaves at its default goes back to its default here too.

## The command palette

<kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>P</kbd> opens the command palette: the
**Find anywhere** box with a `>` already typed. Type after it to pick a command — open or save a
file, format the code, switch connection, change the theme, toggle a panel and more. Where a
command has a shortcut of its own, the palette shows it beside the command. Typing `>` into
Find anywhere (<kbd>Ctrl</kbd>+<kbd>P</kbd>) does the same.

## Global

| Keys | Does |
|---|---|
| <kbd>Ctrl</kbd>+<kbd>P</kbd> | Find anywhere |
| <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>P</kbd> | Command palette |
| <kbd>Ctrl</kbd>+<kbd>T</kbd> | New query tab |
| <kbd>Ctrl</kbd>+<kbd>W</kbd> | Close query tab |
| <kbd>Ctrl</kbd>+<kbd>Tab</kbd> | Next tab |
| <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>Tab</kbd> | Previous tab |
| <kbd>Ctrl</kbd>+<kbd>1</kbd>…<kbd>9</kbd> | Jump to tab |
| <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>T</kbd> | Reopen last closed tab |
| <kbd>Ctrl</kbd>+<kbd>O</kbd> | Open SQL file |
| <kbd>Ctrl</kbd>+<kbd>S</kbd> | Save SQL file |
| <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>S</kbd> | Save SQL file as |
| <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>E</kbd> | Toggle schema panel |
| <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>A</kbd> | Toggle AI panel |
| <kbd>Ctrl</kbd>+<kbd>&#96;</kbd> | Toggle terminal |

## Editor

| Keys | Does |
|---|---|
| <kbd>Ctrl</kbd>+<kbd>Enter</kbd> | Run query |
| <kbd>Ctrl</kbd>+<kbd>Space</kbd> | Autocomplete |
| <kbd>Ctrl</kbd>+<kbd>K</kbd> | Inline AI edit |
| <kbd>Ctrl</kbd>+<kbd>F</kbd> | Find in editor |
| <kbd>Ctrl</kbd>+<kbd>H</kbd> | Find and replace |
| <kbd>Ctrl</kbd>+<kbd>G</kbd> | Go to line |
| <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>&#92;</kbd> | Jump to the matching bracket or BEGIN/END |
| <kbd>Ctrl</kbd>+<kbd>/</kbd> | Toggle line comment |
| <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>/</kbd> | Toggle block comment |
| <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>U</kbd> | Toggle case |
| <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>J</kbd> | Join lines |
| <kbd>Ctrl</kbd>+<kbd>D</kbd> | Duplicate line / selection |
| <kbd>Ctrl</kbd>+<kbd>X</kbd> | Cut line / selection |
| <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>L</kbd> | Format code |
| <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>Shift</kbd>+<kbd>L</kbd> | Format statement |
| <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>J</kbd> | Make single line |
| <kbd>Alt</kbd>+<kbd>↑</kbd> / <kbd>Alt</kbd>+<kbd>↓</kbd> | Move line up / down |
| <kbd>Alt</kbd>+<kbd>Enter</kbd> | Apply the quick fix under the caret |
| <kbd>Tab</kbd> / <kbd>Shift</kbd>+<kbd>Tab</kbd> | Indent / outdent |
| <kbd>Tab</kbd> / <kbd>Ctrl</kbd>+<kbd>→</kbd> | Take the inline prediction (→ too, at a line's end) / its next word |

<kbd>Ctrl</kbd>+<kbd>Enter</kbd> runs the statement under the caret. To run every statement in
the tab, use **Run** in the command palette.

## Results grid

| Keys | Does |
|---|---|
| <kbd>Ctrl</kbd>+<kbd>F</kbd> | Find in results |
| <kbd>Ctrl</kbd>+<kbd>G</kbd> | Go to row |
| <kbd>Ctrl</kbd>+<kbd>C</kbd> | Copy |
| <kbd>Ctrl</kbd>+<kbd>V</kbd> | Paste as staged edits |
| <kbd>Ctrl</kbd>+<kbd>A</kbd> | Select all |
| <kbd>Ctrl</kbd>+<kbd>Home</kbd> / <kbd>Ctrl</kbd>+<kbd>End</kbd> | First / last cell |
| <kbd>Enter</kbd> | Edit cell |
| <kbd>Tab</kbd> / <kbd>Shift</kbd>+<kbd>Tab</kbd> | Next / previous cell while editing |
| <kbd>Ctrl</kbd>+<kbd>Enter</kbd> | Commit edits |
| <kbd>Del</kbd> | Mark row for deletion |
| <kbd>F6</kbd> | Go to the results toolbar |
| <kbd>←</kbd> / <kbd>→</kbd> | Move along the toolbar (Esc returns to the grid) |

## Schema tree

| Keys | Does |
|---|---|
| <kbd>Enter</kbd> | Open the selected table, column or object |
| <kbd>Shift</kbd>+<kbd>F10</kbd> | Context menu for the selected row |

## Server activity

| Keys | Does |
|---|---|
| <kbd>Shift</kbd>+<kbd>F10</kbd> | Context menu for the selected row |
| <kbd>Enter</kbd> | Context menu for the selected row |

The menu offers **Cancel query** and **Kill session** for that row's session. Move between rows
with the arrow keys, <kbd>Home</kbd> and <kbd>End</kbd>.

## AI panel

| Keys | Does |
|---|---|
| <kbd>Enter</kbd> | Send message |
| <kbd>Shift</kbd>+<kbd>Enter</kbd> | New line |
| <kbd>Ctrl</kbd>+<kbd>↑</kbd> / <kbd>Ctrl</kbd>+<kbd>↓</kbd> | Recall previous / next prompt |

## Terminal

| Keys | Does |
|---|---|
| <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>C</kbd> / <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>V</kbd> | Copy / paste |

## ER diagram

| Keys | Does |
|---|---|
| <kbd>Ctrl</kbd>+<kbd>F</kbd> | Find in the diagram |

## More

- [Query editor](../query-editor/README.md), [Results grid](../results-grid/README.md) and
  [Schema](../schema/README.md) explain what these keys act on.
- Arrow keys, <kbd>Enter</kbd> and <kbd>Esc</kbd> work as you'd expect almost everywhere, so
  they're listed only where they do something you wouldn't guess.
