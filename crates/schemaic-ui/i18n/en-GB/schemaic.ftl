# Schemaic — the interface's source catalogue (British English).
#
# Every other language's catalogue falls back to this one message by message.
# Ids are kebab-case and start with the area they belong to. Spelling is
# British throughout (AGENTS.md); SQL keywords and persisted names are never
# put here, and a key name only as the `key-*` legends at the end.

## Settings › Appearance › Language

settings-language = Language
# The "follow the operating system" entry; $language is the language that
# resolves to, named in itself (English, Deutsch).
settings-language-system = System ({ $language })
settings-language-restart = The language change takes effect the next time Schemaic starts.

## Settings › Terminal

settings-term-shell = Shell
settings-term-font-size = Font size
settings-term-cursor-style = Cursor style
settings-term-copy-on-select = Copy on selection
settings-term-copy-on-select-hint = Copy selected text to the clipboard the moment a selection ends.
settings-term-blink = Blink cursor
settings-term-blink-hint = Blink the cursor while the terminal is focused.

## Settings › AI

settings-ai-agent = Agent CLI
settings-ai-off-hint = Nothing is sent to any agent, no agent CLI is started, and no AI action is offered. Pick a CLI to turn the assistant on.
# $cli is the agent CLI's product name (Claude Code, Codex).
settings-ai-cli-path = { $cli } path
settings-ai-cli-path-placeholder = Leave empty to auto-detect
settings-ai-detected = Auto-detected: { $path }
settings-ai-detect-failed = Auto-detect failed. { $cli } not found — give it a path above.
settings-ai-cli-path-missing = File doesn't exist.
settings-ai-model = Model
settings-ai-model-placeholder = Leave empty for the harness's default
settings-ai-model-hint = Any id this CLI accepts — an alias, or a dated snapshot to pin one across upgrades.
settings-ai-model-hint-with-chips = Any id this CLI accepts — an alias, or a dated snapshot to pin one across upgrades. The buttons are shortcuts, not the whole list.
settings-ai-effort = Effort
settings-ai-instructions = Custom instructions
settings-ai-instructions-placeholder = Dialect, conventions, house rules…
settings-ai-schema-context = Schema context
settings-ai-schema-context-hint = How much database structure rides in every message. None also withholds the schema tools, so the assistant asks you for names instead of reading the catalogue itself.
settings-ai-data-access = Data access
settings-ai-data-access-where = Set per connection, in the connection's settings.
settings-ai-gutter = Accent rule on replies
settings-ai-gutter-hint = Mark the assistant's replies with a coloured rule down their right edge. Off gives them the same margin on both sides.

## Settings › section list

settings-section-general = General
settings-section-editor = Editor
settings-section-formatting = Code formatting
settings-section-query = Query
settings-section-terminal = Terminal
settings-section-ai = AI assistant
settings-section-appearance = Appearance

## Settings › shared

# A dropdown's "no value" entry (a wrap width of 0).
settings-off = Off

## Settings › Code formatting

settings-format-wrap-width = { $n ->
    [one] { $n } character
   *[other] { $n } characters
}

## Settings › General

settings-log = Log file
settings-log-hint = Diagnostics, including crashes, are written to { $path }.
settings-log-hint-none = No log file could be opened on this machine, so nothing is being written to one.
settings-log-open-folder = Open folder
settings-bundle = Settings file
settings-bundle-hint = Your preferences and snippets in one file, to take to another machine. Connections, window layout, the terminal's shell and connection-specific snippets stay here.
settings-bundle-export = Export
settings-bundle-import = Import
settings-cli = Command line
settings-cli-install = Install
settings-cli-installing = Installing
settings-cli-remove = Remove
settings-cli-removing = Removing

## Settings › modal

settings-title = Settings
settings-restore-tabs = Restore tabs on startup
settings-restore-tabs-hint = Reopen the query tabs from your last session when the app starts.

## Settings › Editor

settings-editor-font-size = Font size
settings-editor-predictions = Inline predictions
settings-editor-predictions-hint = Show the likely next words after the caret — the partner of a keyword, a join's foreign key, the columns to group by or insert. Tab, or → at the end of a line, takes them whole, Ctrl+→ (⌘→ on macOS) a word at a time, and Escape turns them down.
settings-editor-remembered = Predictions from past statements
settings-editor-remembered-hint = Also offer the rest of a line from a statement you ran, wrote in another tab or saved as a snippet. Switch off to keep past statements off the screen.
settings-editor-aliases = Table aliases
settings-editor-aliases-hint = When completion writes a table after FROM or JOIN, follow it with a short alias made of its initials — orders o, order_items oi — and use the alias in a join's ON condition.
settings-editor-rainbow = Rainbow brackets
settings-editor-rainbow-hint = Colour each pair of brackets by how deeply it is nested, so a closing bracket can be matched to its opening one at a glance.
settings-editor-whitespace = Show whitespace
settings-editor-whitespace-hint = Draw spaces and tabs as faint marks, so a tab can be told from spaces.
settings-editor-guides = Indent guides
settings-editor-guides-hint = Draw a faint vertical line at each indent level.

## Settings › Code formatting (SQL keywords stay as they are written)

settings-format-keyword-case = Keyword case
settings-format-keyword-case-hint = How Format Code writes keywords such as SELECT and FROM. Names, strings and comments keep their case.
settings-format-function-case = Function case
settings-format-function-case-hint = How Format Code writes the engine's own functions, such as COUNT and COALESCE, where they are called. Your own functions keep their case.
settings-format-type-case = Type case
settings-format-type-case-hint = How Format Code writes the engine's own data types, such as INT and VARCHAR. Types you created keep their case.
settings-format-commas = Commas
settings-format-commas-hint = Where Format Code puts the comma of a list it breaks across lines. Lists kept on one line, like a function's arguments, are left alone unless the wrap width breaks them.
settings-format-statement-gap = Between statements
settings-format-statement-gap-hint = The blank lines Format Code leaves between two statements. As typed keeps a blank line wherever there was one.
settings-format-wrap = Wrap width
settings-format-wrap-hint = A list Format Code keeps on one line, like a function's arguments, goes one item a line once its line would run past this width.
settings-format-river = Right-align clause keywords
settings-format-river-hint = Line SELECT, FROM, WHERE and the rest up on their right edge, each clause's body beside its keyword, instead of a keyword on a line of its own.
settings-format-align = Align columns
settings-format-align-hint = Line up the AS of a select list's aliases, the = of an UPDATE's assignments and the types of a table's columns.
settings-format-join-on = Join condition on its own line
settings-format-join-on-hint = Put a join's ON, and the AND and OR of its condition, on a line of their own under the join.

## Settings › Query

settings-query-row-limit = Default row limit
settings-query-timeout = Statement timeout
settings-query-timeout-hint = Cancel a statement that runs longer than this. Off by default — an import or a report can legitimately take a while.
settings-query-confirm-writes = Confirm before running writes
settings-query-confirm-writes-hint = Ask before executing any statement that modifies data or schema.
settings-query-validate = Live database validation
settings-query-validate-hint = Check the statement under the cursor against the database as you type (a non-executing PREPARE) to surface exact errors. Adds a DB round-trip on each pause.
settings-query-notify = Notify when a run finishes in the background
settings-query-notify-hint = Show a desktop notification when a query, script, export or dump that took a second or more ends while Schemaic is not the active window. The taskbar button is highlighted either way. The notification never includes the SQL.
settings-query-write-log = Keep values in the write log
# write_log.jsonl and "schemaic exec" are a file name and a command: never translated.
settings-query-write-log-hint = Every write Schemaic or schemaic exec runs is appended to write_log.jsonl in the settings folder, and never cleared. Values in its SQL are masked unless this is on; a password is masked either way.

## Settings › Appearance

settings-appearance-ui-theme = Interface theme
settings-appearance-editor-theme = Editor theme
settings-appearance-scale = Interface scale

## Shortcuts modal (its rows are `shortcuts.rs`'s)

shortcuts-title = Shortcuts

## Shortcuts modal › group headings

shortcuts-group-global = Global
shortcuts-group-editor = Editor
shortcuts-group-results-grid = Results grid
shortcuts-group-schema-tree = Schema tree
shortcuts-group-ai-panel = AI panel
shortcuts-group-terminal = Terminal
shortcuts-group-er-diagram = ER diagram

## Shortcuts modal › rows (the keys beside them are never translated)

shortcuts-find-anywhere = Find Anywhere
shortcuts-command-palette = Command palette
shortcuts-new-tab = New query tab
shortcuts-close-tab = Close query tab
shortcuts-next-tab = Next tab
shortcuts-previous-tab = Previous tab
shortcuts-jump-to-tab = Jump to tab
shortcuts-reopen-tab = Reopen last closed tab
shortcuts-open-file = Open SQL file
shortcuts-save-file = Save SQL file
shortcuts-save-file-as = Save SQL file as
shortcuts-toggle-schema = Toggle schema panel
shortcuts-toggle-ai = Toggle AI panel
shortcuts-toggle-terminal = Toggle terminal
shortcuts-run-query = Run query
shortcuts-autocomplete = Autocomplete
shortcuts-inline-ai = Inline AI edit
shortcuts-find-in-editor = Find in editor
shortcuts-find-replace = Find and replace
shortcuts-go-to-line = Go to line
# BEGIN/END are SQL keywords.
shortcuts-matching-bracket = Jump to the matching bracket or BEGIN/END
shortcuts-line-comment = Toggle line comment
shortcuts-block-comment = Toggle block comment
shortcuts-toggle-case = Toggle case
shortcuts-join-lines = Join lines
shortcuts-duplicate = Duplicate line / selection
shortcuts-cut = Cut line / selection
# Also the editor menu's entry, and named like the palette's "Format Code".
shortcuts-format-code = Format code
shortcuts-format-statement = Format statement
shortcuts-single-line = Make single line
shortcuts-move-line = Move line up / down
shortcuts-quick-fix = Apply the quick fix under the caret
shortcuts-indent = Indent / outdent
# Its keys are "Tab / Ctrl+→": the first half is Tab (or →), the second Ctrl+→.
shortcuts-take-prediction = Take the inline prediction (→ too, at a line's end) / its next word
shortcuts-find-in-results = Find in results
shortcuts-go-to-row = Go to row
shortcuts-copy = Copy
shortcuts-paste-staged = Paste as staged edits
shortcuts-select-all = Select all
shortcuts-first-last-cell = First / last cell
shortcuts-edit-cell = Edit cell
shortcuts-next-cell = Next / previous cell while editing
shortcuts-commit-edits = Commit edits
shortcuts-mark-deletion = Mark row for deletion
shortcuts-results-toolbar = Go to the results toolbar
shortcuts-along-toolbar = Move along the toolbar (Esc returns to the grid)
shortcuts-open-selected = Open the selected table, column or object
shortcuts-context-menu = Context menu for the selected row
shortcuts-send-message = Send message
shortcuts-new-line = New line
shortcuts-recall-prompt = Recall previous / next prompt
shortcuts-copy-paste = Copy / paste
shortcuts-find-in-diagram = Find in the diagram

## Key names in shortcut labels — a keyboard's own legends, so a language
## whose keyboards print another word uses it (German: Strg, Umschalt, Entf).
## Keys written the same everywhere (letters, arrows, Tab, Esc, F6) and macOS's
## Cmd are not here. Read key by key by `shortcuts::keys_label`.

key-ctrl = Ctrl
key-shift = Shift
key-alt = Alt
key-enter = Enter
key-del = Del
key-home = Home
key-end = End
key-space = Space
