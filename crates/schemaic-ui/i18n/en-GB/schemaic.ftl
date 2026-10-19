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

## Command palette › commands
## A command's label is also what is typed to pick it, lowercased (Tab types
## the label into the box), so no argument-taking command's label may be the
## first words of another command's: "History" before "Clear History" is
## fine, "Verlauf" before "Verlauf leeren" is not. The English names match in
## every language as well.

palette-toggle-panel = Toggle Panel
palette-toggle-panel-hint = schema · ai · terminal · query history · server activity
palette-panel-schema = Schema
palette-panel-ai = AI
palette-panel-terminal = Terminal
palette-panel-history = Query History
palette-panel-activity = Server Activity
palette-new-tab = New Tab
palette-export-settings = Export Settings
palette-import-settings = Import Settings
palette-duplicate-tab = Duplicate Tab
palette-close-tab = Close Tab
palette-reopen-closed-tab = Reopen Closed Tab
palette-reopen-closed-tab-hint = <tab>
palette-open-file = Open File
palette-open-file-hint = a .sql script
palette-save-file = Save File
palette-save-file-as = Save File As
palette-next-tab = Next Tab
palette-previous-tab = Previous Tab
# Named like the editor menu's and the shortcut sheet's "Format code".
palette-format-code = Format Code
palette-format-statement = Format Statement
palette-format-statement-hint = the statement at the caret
palette-make-single-line = Make Single Line
palette-make-single-line-hint = the selection or statement, on one line
palette-run = Run
palette-run-hint = run all statements
palette-go-to-line = Go to Line
palette-go-to-line-hint = <number>
palette-go-to-statement = Go to Statement
palette-search-hint = <search>
palette-history = History
palette-clear-history = Clear History
palette-clear-history-hint = current connection
palette-ask-ai = Ask AI
palette-ask-ai-hint = <prompt>
palette-ask-ai-empty = Type a prompt…
palette-terminal = Terminal
palette-terminal-hint = <command>
palette-run-in-terminal = Run in Terminal
palette-terminal-empty = Type a command…
palette-ui-theme = UI Theme
palette-ui-theme-hint = light · dark
palette-editor-theme = Editor Theme
palette-editor-theme-hint = <theme>
palette-font-size = Font Size
palette-increase-font-size = Increase Font Size
palette-decrease-font-size = Decrease Font Size
palette-indent-style = Indent Style
palette-indent-style-hint = tabs · spaces
palette-indent-spaces = Spaces
palette-indent-tabs = Tabs
palette-indent-width = Indent Width
palette-toggle-word-wrap = Toggle Word Wrap
palette-toggle-whitespace = Toggle Whitespace
palette-toggle-whitespace-hint = show spaces and tabs
palette-toggle-indent-guides = Toggle Indent Guides
palette-switch-connection = Switch Connection
palette-switch-connection-hint = <connection>
palette-server-variables = Server Variables
palette-server-variables-hint = the active connection's settings
palette-enter-number = Enter a number
palette-nothing-found = Nothing found

## Command palette › what a result row is

palette-row-snippet = snippet
palette-row-open-tab = open tab
palette-row-history = history
palette-row-view-body = view body
# $kind is the object's kind as the schema names it (procedure, function, event).
palette-row-body = { $kind } body
# $table is the table the trigger fires on.
palette-row-trigger-on = trigger on { $table }

## Schema tree › right-click menu and the SCHEMA gear's menu
## No ellipsis on a menu label (AGENTS.md).

tree-open-in-cli = Open in CLI
tree-open = Open
tree-open-in-new-tab = Open in new tab
tree-copy-name = Copy name
tree-copy-qualified-name = Copy qualified name
tree-properties = Properties
tree-used-by = Used by
tree-live-monitor = Live monitor
tree-er-diagram = ER diagram
tree-compare-with = Compare with
tree-generate = Generate
tree-generate-ddl = Generate DDL
# The Generate submenu's children, each the statement it drafts.
tree-generate-create = Create
tree-generate-insert = Insert
tree-generate-update = Update
tree-generate-delete = Delete
tree-refresh = Refresh
tree-collapse-all = Collapse all
tree-favourite = Favourite
tree-unfavourite = Unfavourite
tree-colour = Colour
tree-colour-none = None
tree-hide = Hide
tree-import = Import
tree-export = Export
tree-back-up = Back up
tree-restore-from-file = Restore from file
tree-copy-to = Copy to
tree-create = Create
# The Create submenu's children: the noun, the verb being the parent row's.
tree-create-table = Table
tree-create-view = View
tree-create-type = Type
tree-create-domain = Domain
tree-create-sequence = Sequence
tree-create-synonym = Synonym
tree-create-alias-type = Alias type
tree-create-xml-schema-collection = XML schema collection
tree-create-function = Function
tree-create-procedure = Procedure
tree-create-event = Event
tree-create-extension = Extension
tree-create-database = Database
tree-create-schema = Schema
# $kind is the object's kind as the schema names it (type, domain, sequence).
tree-create-kind = Create { $kind }
tree-edit-kind = Edit { $kind }
tree-folder-create-extension = Create extension
tree-edit-table = Edit table
tree-edit-view = Edit view
tree-edit-column = Edit column
tree-edit-index = Edit index
tree-edit-primary-key = Edit primary key
tree-edit-foreign-key = Edit foreign key
tree-triggers = Triggers
# Only shown with one or more triggers; none is the bare "Triggers".
tree-triggers-count = Triggers ({ $n })
tree-refresh-view = Refresh view
tree-truncate = Truncate
tree-truncate-title = Truncate table
tree-drop = Drop
tree-drop-index = Drop index
tree-drop-foreign-key = Drop foreign key
tree-ai-explain = AI explain
tree-users = Users and privileges
tree-server-variables = Server variables
# The gear's and the tree's blank-space entry; under Create ▸ it is just "Database".
tree-new-database = Create database
tree-show-table-sizes = Show table sizes

## Connection switcher

conn-menu-manage = Manage Connections

## Shared dialog buttons

dialog-cancel = Cancel
dialog-yes = Yes
dialog-no = No

## Open-transaction prompt

tx-prompt-title = Open transaction
# $tab is the tab's title; $n how many statements the transaction holds.
tx-prompt-body = { $tab } has an open transaction with { $n ->
    [one] 1 statement
   *[other] { $n } statements
} in it. You can commit to keep the changes, or rollback to discard them.
tx-prompt-body-aborted = { $tab } has a transaction that a failed statement aborted, with { $n ->
    [one] 1 statement
   *[other] { $n } statements
} in it. It can only be rolled back, which discards the changes.
tx-prompt-rollback = Rollback
tx-prompt-commit = Commit

## Error modal

error-modal-title = Error
error-modal-none = No error.
error-modal-ai-fix = AI fix
error-modal-explain = Explain

## Schema tree › the panel

tree-panel-title = SCHEMA
tree-tip-databases = Show or hide databases
tree-tip-options = Schema options
tree-search-placeholder = Search…
tree-loading = Loading
tree-no-tables = No tables
tree-failed-to-connect = Failed to connect
# Opens the full error, as the editor's and the grid's error bars do.
tree-failed-view = View
# A single-user database the load left unread on purpose.
tree-single-user-unread = Single-user — not read
tree-single-user-read = Read
# The capsules beside a database or namespace, and under a table.
tree-count-tables = { $n ->
    [one] { $n } table
   *[other] { $n } tables
}
tree-count-cols = { $n ->
    [one] { $n } col
   *[other] { $n } cols
}
tree-count-keys = { $n ->
    [one] { $n } key
   *[other] { $n } keys
}
tree-count-partitions = { $n ->
    [one] { $n } partition
   *[other] { $n } partitions
}

## Object kinds — the interface's words for core's `ObjectKind`
## English must read exactly as core's `label`/`group_label` (a test holds it).
## The singular is used inside a sentence, so it is lower case in English.

kind-type = type
kind-domain = domain
kind-sequence = sequence
kind-function = function
kind-procedure = procedure
kind-event = event
kind-synonym = synonym
kind-alias-type = alias type
kind-xml-schema-collection = XML schema collection
kind-extension = extension
kind-group-type = Types
kind-group-domain = Domains
kind-group-sequence = Sequences
kind-group-function = Functions
kind-group-procedure = Procedures
kind-group-event = Events
kind-group-synonym = Synonyms
kind-group-alias-type = Alias types
kind-group-xml-schema-collection = XML schema collections
kind-group-extension = Extensions

## Query history panel

history-title = QUERY HISTORY
history-clear-tip = Clear this connection's history…
history-clear-title = Clear query history
history-clear-message = Delete { $n } recorded { $n ->
    [one] query
   *[other] queries
} for this connection? This can't be undone.
history-scope-current = Current
history-scope-all = All
# Above the list when the cap left rows out; $shown rows of $matched.
history-cut-note = Showing the newest { $shown } of { $matched } — search to narrow.
history-empty = No queries yet.
history-no-match = No matching queries.
history-open-in-new-tab = Open in new tab
history-delete = Delete
history-failed = Failed

## Snippet library panel

snippets-title = SNIPPET LIBRARY
snippets-save-tip = Save the editor's SQL as a snippet
snippets-no-match = No snippet matches.
snippets-band-this-connection = THIS CONNECTION
snippets-band-all-connections = ALL CONNECTIONS
snippets-builtin = Built-in
snippets-name-placeholder = Snippet name
snippets-insert = Insert into editor
snippets-open-in-new-tab = Open in new tab
snippets-edit = Edit
snippets-duplicate = Duplicate
snippets-delete = Delete
snippets-show-in = Show in
snippets-scope-this-connection = This connection
# $engine is the product's name (MySQL, PostgreSQL).
snippets-scope-engine = Every { $engine } connection
snippets-scope-all-connections = All connections

## Query history panel › recency bands (core's history::Bucket words)

history-band-today = TODAY
history-band-this-week = THIS WEEK
history-band-earlier = EARLIER

## Query tab strip

# A file tab's tooltip when its text differs from the file on disk.
tabs-tip-unsaved = { $path } — unsaved changes
tabs-pin = Pin
tabs-unpin = Unpin
tabs-rename = Rename
tabs-duplicate = Duplicate
tabs-close = Close
tabs-open-file = Open file
tabs-save = Save
tabs-save-as = Save as
tabs-reload = Reload from disk
tabs-reopen-last = Reopen last tab
tabs-close-others = Close other tabs
tabs-close-all = Close all tabs

## Server Activity panel

activity-title = SERVER ACTIVITY
activity-refresh-tip = Refresh now
activity-auto-refresh-off = Auto-refresh off
# $secs is the poll interval in seconds.
activity-auto-refresh-every = Auto-refresh every { $secs }s
activity-view-sessions = Sessions
activity-view-locks = Locks
activity-search-sessions = Search sessions…
activity-search-locks = Search locks…
activity-loading = Loading…
activity-sqlite-no-sessions = SQLite has no server sessions — it runs inside this process, so there is nothing else connected to see.
activity-sessions-no-match = No matching sessions.
activity-sessions-none = No other sessions on this server.
# Below the drawn rows when the search matched more than the panel draws.
activity-sessions-undrawn = { $n } more { $n ->
    [one] session matches — narrow the search to see it.
   *[other] sessions match — narrow the search to see them.
}
activity-locks-no-match = No matching locks.
activity-locks-no-waits = No lock waits on this server.
activity-locks-none = No other session holds a lock.
activity-locks-undrawn = { $n } more { $n ->
    [one] lock matches — narrow the search to see it.
   *[other] locks match — narrow the search to see them.
}
# The fetch stopped at its cap, so the list is not the whole server's.
activity-list-capped = list capped
activity-lock-wait = Lock wait
# $id is the blocking session's id.
activity-kill-holder = Kill { $id }
activity-terminate-tip = Terminate session { $id }
activity-read-only-tip = This connection is marked read-only.
activity-lock-holds = holds
activity-lock-waits-for = waits for
# $ids is a comma-separated list of session ids.
activity-lock-held-by = held by { $ids }
activity-copy-statement = Copy statement
# KillKind's words, as core's label names them.
activity-cancel-query = Cancel query
activity-kill-session = Kill session

## AI assistant panel
## The panel's own words only — prompts sent to the agent stay English.

ai-title = AI ASSISTANT
ai-new-chat-tip = New chat
ai-settings-tip = AI settings
ai-empty-ask = Ask about your SQL…
# $cli is the agent CLI's product name (Claude Code, Codex).
ai-empty-not-connected = { $cli } not connected.
ai-input-placeholder = Message…
ai-input-off = The AI assistant is off
ai-input-db-down = Not connected to the database
# The user's messages are headed by this; the agent's by its own name.
ai-speaker-you = YOU
# After a tool result cut to its first lines; $n is how many were cut.
ai-result-more-lines = … (+{ $n } more lines)

## Shared dialog buttons (continued)

dialog-close = Close

## Chart modal

# $tab is the result's tab title.
chart-title = Chart — { $tab }
chart-caption-kind = Chart
chart-caption-values = Values
chart-caption-labels = Labels
# The label picker's entry for "no column — number the rows".
chart-row-number = Row number
# ChartKind's words, as core's label names them.
chart-kind-bar = Bar
chart-kind-line = Line
chart-kind-histogram = Histogram
chart-save-tip = Save the chart…
chart-save-dialog = Save chart
# The saved file's suggested name, before its extension.
chart-file-stem = { $name } chart
chart-nothing-to-save = Nothing to save.
chart-no-numbers = No column of numbers to chart in this result.
chart-all-null = Every value in that column is NULL.

## Schema editors — shared

editor-preview-sql = Preview SQL
# The footer's count of what the draft would change.
editor-change-count = { $n ->
    [0] No changes
    [one] 1 change
   *[other] { $n } changes
}

## View editor

# $view is database.schema.view (or database.view).
view-editor-title-edit = Edit view { $view }
# $location is the database, or database.schema.
view-editor-title-create = Create view in { $location }
view-editor-section-view = View
view-editor-section-definition = Definition
view-editor-section-options = Options
view-editor-name = Name
view-editor-select = SELECT statement
view-editor-check-option = Check option
view-editor-column-names = Column names
view-editor-column-names-placeholder = optional — taken from the SELECT when empty
view-editor-sql-security = SQL security
view-editor-definer = Definer
view-editor-recreate = Re-create instead of replacing
view-editor-recreate-hint = PostgreSQL can only add columns to an existing view. Renaming, retyping or reordering one needs a DROP and a CREATE, which takes dependent views and grants with it.

## Table properties panel

# $table is the table's display name.
props-title = Properties — { $table }
props-reading = Reading statistics
props-no-stats-view = A view has no storage of its own, so the server reports no statistics for it.
props-no-stats-table = The server reported no statistics for this table.
# Under the size figure.
props-on-disk = on disk
props-counting = Counting
props-count-rows = Count rows
props-count-slow = A full scan of the table. Slow on a large one.
props-storage-data = Data
props-storage-indexes = Indexes
props-storage-free = Free
props-section-structure = Structure
props-columns = Columns
props-indexes = Indexes
props-foreign-keys = Foreign keys
props-count-columns = { $n ->
    [one] { $n } column
   *[other] { $n } columns
}
props-count-indexes = { $n ->
    [one] { $n } index
   *[other] { $n } indexes
}
props-count-keys = { $n ->
    [one] { $n } key
   *[other] { $n } keys
}
props-type = Type
props-type-view = View
props-type-table = Table
props-section-options = Options
props-engine = Engine
props-row-format = Row format
props-collation = Collation
props-next-auto-increment = Next auto-increment
props-created = Created
props-updated = Updated
props-comment = Comment
props-section-partitioning = Partitioning
props-section-indexes = Indexes
# $pct is a whole-number percentage. VACUUM is a SQL keyword.
props-vacuum-note = { $pct }% of this table's row versions are dead and not yet reclaimed. A VACUUM would return that space.
props-copy = Copy

## Shared panel search box

panel-search-placeholder = Search…

## How long ago — history::ago's buckets (English must read as core's relative_time)

ago-just-now = just now
ago-minutes = { $n }m ago
ago-hours = { $n }h ago
ago-days = { $n }d ago
ago-weeks = { $n }w ago

## A history row's outcome line — the row count (core composes the line)

outcome-rows = { $n ->
    [one] { $n } row
   *[other] { $n } rows
}
# The fetch stopped at its cap: "at least this many", so always plural.
outcome-rows-capped = { $n }+ rows

## Terminal panel header

term-title = TERMINAL
term-db-cli-tip = Open the database CLI
term-db-cli-tip-read-only = Open the database CLI — this connection is marked read-only
term-restart-tip = Restart the terminal
term-settings-tip = Terminal settings

## Results panel header

results-title = RESULTS
results-properties-tip = Table properties…
results-monitor-tip = Live monitor…
results-restore-editor-tip = Restore the editor
results-collapse-editor-tip = Collapse the editor

## Query panel header

query-title = QUERY
# The database selector when the tab has none chosen.
query-no-database = No database

## Shared dialog buttons (continued)

dialog-stop = Stop

## Copy table modal

# $table is the source table's display name.
copy-table-title = Copy { $table }
copy-table-into = Copy into
copy-table-schema = Schema
copy-table-name = Name
copy-table-back = ‹ back
copy-table-data = Copy data
copy-table-data-hint = Copy the rows once the table is created. Off copies the structure only.
copy-table-what-with-rows = The structure and the rows. Foreign keys and triggers are not copied, and a generated column is computed afresh.
copy-table-what-structure = The structure only. Foreign keys and triggers are not copied.
copy-table-choose-database = Choose the database to copy into.
copy-table-connection-changed = The active connection changed — reopen Copy to from the table.
# $database is the source database's name.
copy-table-schema-not-loaded = { $database }'s schema is not loaded — expand it in the tree, then try again.
# The DDL preview's subject; $table is the source table.
copy-table-preview-subject = Copy of { $table }
# $n is how many rows have been copied so far.
copy-table-copying = Copying rows… { $n }
copy-table-copied = Copied { $n } { $n ->
    [one] row
   *[other] rows
}.
copy-table-stopped = Stopped before every row was copied. The table was created.

## Database, schema and extension editor

database-editor-title-database = Create database
database-editor-title-schema = Create schema
database-editor-title-extension = Create extension
# $database is the database the schema or extension goes into.
database-editor-title-schema-in = Create schema in { $database }
database-editor-title-extension-in = Create extension in { $database }
database-editor-section-database = Database
database-editor-section-schema = Schema
database-editor-section-extension = Extension
database-editor-name = Name
database-editor-charset = Character set
database-editor-collation = Collation
database-editor-owner = Owner
# Placeholder of the character set and collation fields left empty.
database-editor-server-default = server default
# Placeholder of the owner field left empty.
database-editor-owner-you = you
database-editor-no-extensions = No extensions left to install
database-editor-no-charsets = No character sets to suggest
database-editor-no-collations = No collations to suggest
database-editor-no-roles = No roles on this server

## SQL script import modal
## $count is a statement count as shown — "412+" when the read-ahead stopped
## short — and $n the number the plural is chosen by. $size is a size such as
## "8 MB". DROP, TRUNCATE, DELETE, UPDATE and WHERE are SQL keywords.

# $database is the database the modal was opened on.
script-title = Import into { $database }
script-dialog-title = Run a SQL script
script-section = SQL script
script-choose-file = Choose a file…
script-no-file = No file chosen
script-reading = Reading the file
script-hint = The script may create and select a different database from the one you're importing into. It may also create more than one database. Check the script to make sure it does what you expect.
# $namespace is the schema the modal was opened from. search_path is PostgreSQL's setting.
script-hint-namespace = The script may create and select a different database from the one you're importing into. It may also create more than one database. Opening this from { $namespace } does not put the script in it either: anything the file does not qualify goes wherever the server's search_path says. Check the script to make sure it does what you expect.
script-what-it-does = What this file does
script-probe-nothing = This file holds no statements Schemaic can run.
script-probe-cut-off = The first { $size } of this file is one unfinished statement, so there is nothing to summarise yet. Running it sends the file whole.
script-probe-counted = { $count } { $n ->
    [one] statement
   *[other] statements
} read from the first { $size }.
script-destroys = { $count } { $n ->
    [one] statement
   *[other] statements
} in this file { $n ->
    [one] destroys
   *[other] destroy
} data — DROP, TRUNCATE or DELETE. That cannot be undone from here.
script-unread = Only the first { $size } of this file was read, and nothing in it destroys data — but the rest was not looked at. It may DROP, TRUNCATE or DELETE, and that cannot be undone from here.
script-unqualified = { $count } { $n ->
    [one] statement
   *[other] statements
} in this file { $n ->
    [one] acts
   *[other] act
} on every row of a table — a DELETE, UPDATE or TRUNCATE with no WHERE. That cannot be undone from here.
script-tx-own = This file opens its own transaction, so it either lands whole or not at all. Schemaic adds none of its own.
script-tx-none = This file opens no transaction, so a failure part-way leaves what ran before it in place.
script-connecting = Connecting
# Progress, in bytes: $done of $total, or $done alone when the size is unknown.
script-progress-of = { $done } of { $total }
script-progress-read = { $done } read
script-run = Run
# $file is the script's file name.
script-done = Ran { $n } { $n ->
    [one] statement
   *[other] statements
} from { $file }.
script-stopped = Stopped after { $n } { $n ->
    [one] statement
   *[other] statements
}.
# $message is the server's own error, in the server's language.
script-failed = Failed: { $message } — { $n } { $n ->
    [one] statement
   *[other] statements
} ran before it.
script-failed-at = Failed at line { $line } of { $file }: { $message } — { $n } { $n ->
    [one] statement
   *[other] statements
} ran before it.
script-durability-rolled-back = { $file } opened its own transaction, so the server rolled the whole run back — nothing was applied, including the statements that succeeded.
script-durability-applied = { $file } opened no transaction, so every statement that ran is still applied.
script-durability-applied-before = Any statement that ran before { $file } opened its own transaction is still applied; what ran inside the transaction was rolled back.
script-durability-unknown = Any statement that already ran is still applied unless { $file } opened its own transaction.

## Schema comparison modal

compare-title = Compare schemas
compare-choose-database = Choose a database
compare-no-connection = No connection of this engine to compare against.
# Under the left side's name: it is the side the plan alters.
compare-will-change = will change
# Under the right side's name: it is the side the plan copies from.
compare-source-of-truth = source of truth
compare-idle = Choose a database to compare against.
compare-loading = Reading both schemas
compare-select-object = Select an object to see both sides.
compare-summary = { $differ } differ · { $only_here } only here · { $only_there } only there · { $same } identical
compare-summary-uncertain = { $n } { $n ->
    [one] match
   *[other] matches
} the comparison can't vouch for
compare-filter-placeholder = Filter by name
compare-include-identical = Include identical
compare-select-all = Select all
compare-select-none = None
compare-hint-held-back = held back from the plan; the note above says why
compare-hint-blocked = body must be re-read before this can be applied
compare-hint-uncertain = an index this model reads only in part — a match here isn't certain
compare-preview-migration = Preview migration
# Group headings that are not object kinds (those are kind-group-*).
compare-group-table = Tables
compare-group-view = Views
compare-group-trigger = Triggers
# The diff pane heads an object with its kind's word: CompareKind's own.
compare-kind-enum = enum
compare-kind-domain = domain
compare-kind-xml-schema-collection = XML schema collection
compare-kind-alias-type = alias type
compare-kind-sequence = sequence
compare-kind-table = table
compare-kind-view = view
compare-kind-function = function
compare-kind-procedure = procedure
compare-kind-trigger = trigger
compare-kind-event = event
compare-kind-synonym = synonym

## Schema comparison modal › why the list is empty (core's EmptyRows)

compare-empty-nothing = Neither database holds anything to compare.
compare-empty-agrees = These two schemas match, object for object.
compare-empty-agrees-noted = Every object agrees, but not everything could be compared — see the notes above.
compare-empty-no-match = Nothing matches that filter.
compare-empty-only-identical = Everything matching that filter is identical — turn on Include identical to see it.

## Schema editors — shared (continued)

# The comment field's placeholder, in every editor that has one.
editor-comment-placeholder = what it does

## Event editor

# $event is database.schema.event (or database.event).
event-editor-title-edit = Edit event { $event }
# $location is the database, or database.schema.
event-editor-title-create = Create event in { $location }
event-editor-section-event = Event
event-editor-section-schedule = Schedule
event-editor-section-body = Body
event-editor-section-options = Options
event-editor-name = Name
event-editor-body = Body
event-editor-runs = Runs
# The schedule picker's two shapes: EVERY … or AT ….
event-editor-runs-repeating = Repeating
event-editor-runs-once = Once
event-editor-at = At
event-editor-every = Every
event-editor-starts = Starts
event-editor-ends = Ends
event-editor-status = Status
# EventStatus's words, as core's label names them.
event-editor-status-enabled = Enabled
event-editor-status-disabled = Disabled
event-editor-status-replica = Disabled on replica
event-editor-preserve = Preserve after it completes
event-editor-preserve-hint = ON COMPLETION PRESERVE. Off is MySQL's default and means the server deletes the event once its last run is past — which for a one-off is immediately after it runs.
event-editor-definer = Definer
event-editor-comment = Comment
event-editor-reading-source = Reading the event's source…
event-editor-source-stale = The event's real source arrived after you started typing, so this Body is built on the catalogue's copy — which isn't what the server holds. Close and reopen to edit it.

## Routine editor

# $routine is database.schema.name, with its parameter list where the engine overloads.
routine-editor-title-edit-function = Edit function { $routine }
routine-editor-title-edit-procedure = Edit procedure { $routine }
# $location is the database, or database.schema.
routine-editor-title-create-function = Create function in { $location }
routine-editor-title-create-procedure = Create procedure in { $location }
routine-editor-section-function = Function
routine-editor-section-procedure = Procedure
routine-editor-section-body = Body
routine-editor-section-options = Options
routine-editor-name = Name
routine-editor-parameters = Parameters
routine-editor-returns = Returns
# SQL Server's: a scalar type, or TABLE for a table function. int and TABLE are SQL.
routine-editor-returns-placeholder-mssql = int, or TABLE
routine-editor-language = Language
routine-editor-body = Body
routine-editor-recompile = Recompile
routine-editor-recompile-hint = WITH RECOMPILE — a fresh plan on every call, never a cached one.
routine-editor-schema-binding = Schema binding
routine-editor-schema-binding-hint = WITH SCHEMABINDING — the tables and views it reads can't be changed out from under it.
routine-editor-execute-as = Execute as
# CALLER, SELF and OWNER are SQL Server keywords.
routine-editor-execute-as-placeholder = CALLER, SELF, OWNER or a user
routine-editor-null-input = Returns NULL on NULL input
routine-editor-null-input-hint = RETURNS NULL ON NULL INPUT — the body doesn't run when an argument is NULL.
routine-editor-inlining = Inlining
routine-editor-inlining-automatic = Automatic
routine-editor-comment = Comment
routine-editor-volatility = Volatility
routine-editor-strict = Strict
routine-editor-security-definer = Security definer
routine-editor-security-definer-hint-pg = Runs with the owner's rights instead of the caller's. Pin a search_path below when you use this.
routine-editor-security-definer-hint-mysql = Runs with the definer's rights instead of the caller's. This is MySQL's default — turning it off is SQL SECURITY INVOKER.
routine-editor-settings = Settings
routine-editor-add-setting = Add setting
routine-editor-deterministic = Deterministic
routine-editor-deterministic-hint = Promises the same result for the same arguments. A server with binary logging refuses a non-deterministic routine unless it trusts creators.
routine-editor-data-access = Data access
routine-editor-definer = Definer
routine-editor-reading-source = Reading the routine's source…
routine-editor-source-stale = The routine's real source arrived after you started typing, so this Body is built on the catalogue's copy — which isn't what the server holds. Close and reopen to edit it.

## Users and privileges browser (its title is tree-users)

users-search-placeholder = Search accounts
users-loading-accounts = Loading accounts
users-unsupported = This connection's engine has no user accounts.
users-none = This server reports no accounts.
# $needle is the filter text as typed.
users-no-match = No account matches “{ $needle }”.
users-pick-account = Pick an account to see what it may do.
users-section-attributes = Attributes
users-section-privileges = Privileges
# PrincipalKind's words, as core's label names them.
users-kind-user = User
users-kind-role = Role
users-kind-login = Login
# After the account's kind, for one the server itself owns.
users-maintained-by-server = maintained by the server
users-loading-privileges = Loading privileges
users-no-privileges = This account holds no privileges.
# $shown is the cap on statements drawn, $total how many there are.
users-statements-capped = Showing the first { $shown } of { $total } statements. Copy privileges takes all of them.
users-new-account = New account
users-system-account = This account belongs to the server, so Schemaic will not change it.
users-reset-password = Reset password
users-gate-no-database = Open a query tab on a database to change privileges — the statement has to run in one.
users-gate-read-only = This connection is read-only.
users-count = { $n } { $n ->
    [one] account
   *[other] accounts
}
# While a filter narrows the list: $shown of $n.
users-count-filtered = { $shown } of { $n } accounts
users-copy-privileges = Copy privileges

## Schema editors — shared (continued)

# The tooltip on a value row's bin.
editor-row-remove = Remove

## Trigger editor

# $table is database.schema.table (or database.table).
trigger-editor-title = Triggers on { $table }
# The DDL preview's subject: a plan can touch several of a table's triggers.
trigger-editor-preview-subject = triggers on { $table }
trigger-editor-none-selected = No trigger selected.
trigger-editor-readonly-constraint = This is a constraint trigger. Schemaic doesn't model the deferral settings one carries, so it can be removed here but not edited — change it in SQL instead.
trigger-editor-readonly-hidden = This trigger's definition is encrypted or hidden from this login, so it can be removed here but not edited.
trigger-editor-readonly-unmodelled = This trigger uses an option Schemaic doesn't model, so it can be removed here but not edited — change it in SQL instead.
trigger-editor-section-trigger = Trigger
trigger-editor-section-condition = Condition
trigger-editor-section-action = Action
trigger-editor-name = Name
trigger-editor-timing = Timing
trigger-editor-event = Event
trigger-editor-fires = Fires
# Beside TRUNCATE in PostgreSQL's event toggles.
trigger-editor-truncate-hint = Statement-level only.
trigger-editor-not-for-replication-hint = Silent while a replication agent writes the table.
# SQL Server's firing order for one event (sp_settriggerorder).
trigger-editor-rank-any = Any order
trigger-editor-rank-first = Fires first
trigger-editor-rank-last = Fires last
trigger-editor-when = When
trigger-editor-of-columns = Of columns
trigger-editor-of-columns-placeholder = optional — every column when empty
trigger-editor-body = Body
trigger-editor-function = Function
trigger-editor-edit-function = Edit
trigger-editor-new-function = New function
trigger-editor-arguments = Arguments
trigger-editor-add-argument = Add argument

## Schema editors — shared (continued)

# The tooltips on a value row's arrows.
editor-row-move-up = Move up
editor-row-move-down = Move down

## Object editor (enum and domain types, sequences, SQL Server's synonyms, alias types and XML schema collections)

# $kind is the object kind's word ("sequence", "alias type"); $name is database.schema.name.
object-editor-title-edit = Edit { $kind } { $name }
# $location is database.schema (or database).
object-editor-title-create = Create { $kind } in { $location }
object-editor-name = Name
object-editor-comment = Comment
object-editor-type = Type
object-editor-default = Default
object-editor-section-type = Type
object-editor-section-values = Values
object-editor-section-domain = Domain
object-editor-section-constraints = Constraints
object-editor-section-sequence = Sequence
object-editor-section-position = Position
object-editor-section-synonym = Synonym
object-editor-section-alias-type = Alias type
object-editor-section-xml-schema-collection = XML schema collection
object-editor-section-stands-for = Stands for
object-editor-value-placeholder = value
object-editor-add-value = Add value
object-editor-values-order = In comparison order — this is the order ORDER BY uses
object-editor-add-constraint = Add constraint
object-editor-no-base-types = This engine suggests no base types
# Beside the NOT NULL toggle, whose label is the SQL clause.
object-editor-not-null-hint = Every column of this domain rejects NULL. Turning it on fails if one already holds one.
object-editor-stores = Stores
object-editor-increment = Increment
object-editor-start = Start
object-editor-cache = Cache
object-editor-minimum = Minimum
object-editor-maximum = Maximum
# $field is one of the number fields' labels.
object-editor-not-whole-number = { $field } has to be a whole number.
object-editor-cycle = Cycle
object-editor-cycle-hint = Wrap around to the other end of the range instead of failing when the sequence runs out.
# $last is the value the sequence last handed out.
object-editor-restart-at = Restart at — the counter last handed out { $last }
object-editor-restart-at-unused = Restart at — the counter has not been used
object-editor-restart-not-whole = Restart at has to be a whole number.
object-editor-restart-placeholder = leave empty to keep the position
# SQL Server: a new type re-creates the sequence, which keeps no position.
object-editor-tsql-restart-placeholder = empty keeps it, unless the type changes
object-editor-owned-by = Owned by
object-editor-detach = Detach from its column
object-editor-detach-hint = The sequence stops being dropped with the column, and outlives it.
object-editor-cache-hint = Hand out values from memory, faster, at the cost of a gap after a restart of the server.
object-editor-cache-size = Cache size
object-editor-cache-size-placeholder = server default
# The parts of the name a synonym stands for; each placeholder says what an empty one means.
object-editor-synonym-server = Server
object-editor-synonym-server-placeholder = this server
object-editor-synonym-database = Database
object-editor-synonym-database-placeholder = this database
object-editor-synonym-schema = Schema
object-editor-synonym-schema-placeholder = the default schema
object-editor-synonym-object = Object
object-editor-synonym-object-placeholder = table, view or routine
object-editor-base-type = Base type
object-editor-nullable = Nullable
object-editor-nullable-hint = A column of this type accepts NULL unless the column says otherwise.
object-editor-schemas = Schemas

## Account and grant editors (Users and privileges)

account-editor-title-create = Create account
account-editor-title-reset = Reset password
# $account is the account as the server names it ('app'@'%', app).
account-editor-title-privileges = Privileges — { $account }
account-editor-account = Account
account-editor-kind = Kind
account-editor-name = Name
account-editor-host = Host
# MySQL's own default for an unqualified CREATE USER; % is the wildcard and stays.
account-editor-host-placeholder = % (any host)
# SQL Server: the login a database user is made FOR.
account-editor-for-login = For login
account-editor-for-login-placeholder = login — empty for a user without one
# SQL Server: a new login can bring a user of its own name into this database.
account-editor-also-user = Also a user in { $database }
account-editor-password = Password
account-editor-confirm = Confirm
account-editor-action = Action
account-editor-action-grant = Grant
account-editor-action-revoke = Revoke
account-editor-subject = Subject
account-editor-subject-privileges = Privileges
account-editor-subject-role = Role
account-editor-role = Role
account-editor-no-roles = This server has no roles
account-editor-with-admin-option = With admin option
account-editor-level = Level
account-editor-level-global = Whole server
account-editor-level-database = Database
account-editor-level-schema = Schema
account-editor-level-table = Table
account-editor-level-sequence = Sequence
account-editor-no-suggestions = No suggestions
account-editor-section-privileges = Privileges
account-editor-with-grant-option = With grant option
account-editor-needs-role = Name a role.
account-editor-needs-privileges = Pick a level, name what it applies to, and tick a privilege.

## Table designer

# $table is database.schema.table (or database.table).
designer-title-edit = Edit { $table }
# $location is database.schema (or database).
designer-title-create = Create table in { $location }
designer-tab-table = Table
designer-tab-columns = Columns
designer-tab-indexes = Indexes
designer-tab-foreign-keys = Foreign keys
designer-tab-checks = Checks
# The tooltip on the list bar's +; the bin and arrows are editor-row-*.
designer-list-add = Add
designer-no-suggestions = No suggestions
designer-name = Name
designer-comment = Comment
designer-table-comment-placeholder = What this table is for
designer-engine = Engine
designer-collation = Collation
designer-no-column = No column selected.
designer-type = Type
designer-type-hint = Written straight through — the server decides what it means.
designer-nullable = Nullable
designer-nullable-hint = Allow NULL in this column.
designer-primary-key = Primary key
designer-primary-key-hint = Part of the table's primary key, in column order.
# The server-assigned-key toggle: PostgreSQL and SQL Server call it Identity, MySQL and SQLite Auto-increment.
designer-identity = Identity
designer-auto-increment = Auto-increment
# $keyword is the SQL the toggle writes (AUTO_INCREMENT, IDENTITY(1,1), GENERATED BY DEFAULT AS IDENTITY).
designer-identity-hint = The server assigns the value ({ $keyword }).
designer-default = Default
designer-default-hint = SQL text, as you'd write it: 'draft', 0, CURRENT_TIMESTAMP.
designer-generated = Generated from
designer-generated-hint = An expression, e.g. qty * price. A generated column takes no default.
designer-on-update = On update
# The faint detail beside an index in the list.
designer-index-unique-detail = unique
designer-index-redundant-detail = redundant
designer-no-index = No index selected. The primary key lives on the columns.
designer-columns = Columns
designer-index-columns-hint-single = The one xml, geometry or geography column it indexes.
designer-index-columns-hint-columnstore = Comma-separated columns it stores.
designer-index-columns-hint-prefix = Comma-separated, in key order. bio(20) is a prefix length; add DESC to sort down.
designer-index-columns-hint = Comma-separated, in key order. Add DESC for a descending column.
designer-index-method = Method
designer-index-method-hint = Leave empty for the default (btree).
designer-index-filter = Only rows where
designer-index-filter-hint = A partial index's condition, without the WHERE.
designer-index-storage = Storage
# SQL Server's index kinds.
designer-storage-rowstore = Rowstore
designer-storage-columnstore = Columnstore
designer-storage-clustered-columnstore = Clustered columnstore
designer-storage-primary-xml = Primary XML
designer-storage-selective-xml = Selective XML
designer-storage-secondary-xml = Secondary XML
designer-storage-spatial = Spatial
designer-index-using = Using
designer-index-using-hint = The clause after the column: a secondary XML index's USING XML INDEX … FOR PATH, VALUE or PROPERTY (or FOR (path) on a selective one), a selective XML index's [WITH XMLNAMESPACES (…)] FOR (name = '/path' [AS …], …), or a spatial index's USING … WITH (…).
designer-index-order = Order
designer-index-order-hint = Comma-separated columns the segments are sorted by — SQL Server 2022 on, and 2025 on for a nonclustered index, which sorts only by columns it stores.
designer-index-include = Include
designer-index-include-hint = Comma-separated columns carried in the index but not part of its key.
designer-index-unique = Unique
designer-index-unique-hint = Refuse duplicate values across these columns.
# A foreign key's row detail when no index covers its columns; $table is the table it references.
designer-fk-detail-unindexed = → { $table } · no index
designer-no-fk = No foreign key selected.
designer-fk-ref-table = References table
designer-fk-on-delete = On delete
designer-fk-columns-hint = Comma-separated, paired in order with the referenced columns.
designer-fk-ref-columns = References columns
designer-no-check = No check selected.
designer-check-expression = Expression
designer-check-expression-hint = The condition every row must satisfy. Written without CHECK (…).
designer-check-enforced = Enforced
designer-check-enforced-hint = Off records the constraint without applying it — existing and new rows are both accepted.

## Manage Connections modal (connection_form.rs)
## The engine names in the Type picker (MySQL, PostgreSQL, SQLite, SQL Server)
## are product names and persisted: never translated.

connections-title = Manage Connections
connections-new = New connection
connections-import = Import connections
connections-export = Export connections
connections-duplicate = Duplicate
connections-delete = Delete
connections-test = Test
connections-save = Save
# The tooltip on a failed test when the failure carried no text of its own.
connections-test-failed = Connection failed
# The button inside a path field that opens the file picker.
connections-browse = Browse…
connections-section-general = General
connections-section-safeguards = Safeguards
connections-section-database = Database
connections-section-network = Network
connections-name = Name
connections-colour = Colour
connections-folder = Folder
connections-no-folders = No folders yet
connections-folder-hint = Optional. Connections with the same folder are listed together.
connections-prominent-colour = Prominent colour in editor
connections-prominent-colour-hint = Frame the query editor in this connection's colour.
connections-read-only = Read-only
connections-read-only-hint = Disable cell edits and block write/DDL queries on this connection.
# "schemaic" and "schemaic mcp" are a command and its subcommand: never translated.
connections-cli-access = CLI access
connections-cli-access-hint = Let the schemaic command line, and AI agents through schemaic mcp, use this connection.
connections-ai-data-access = AI data access
connections-statement-timeout = Statement timeout
connections-statement-timeout-hint = Cancels a statement on this connection that runs longer.
connections-environment = Environment
connections-type = Type
connections-init-sql = Init SQL
# PRAGMA and SET are SQL keywords.
connections-init-sql-pragma-hint = Optional. PRAGMA settings run on every connection Schemaic opens.
connections-init-sql-set-hint = Optional. SET statements run on every connection Schemaic opens — a search path, a time zone, a role.
connections-host = Host
connections-port = Port
connections-database = Database
connections-database-hint = Optional, and the database the connection opens in. Some hosted providers allow only their own and refuse a connection that does not name it.
connections-authentication = Authentication
connections-user = User
connections-password = Password
# The commands in backticks are typed as written: never translated.
connections-azure-cli-hint = Signs in as whoever is signed in to the Azure CLI (run `az login` first). The token is fetched when a connection needs one and never saved.
connections-aws-iam-hint = Signs in as the user below with a token from the AWS CLI's credentials (`aws configure` or `aws sso login` first). The user needs the rds_iam role (PostgreSQL) or the AWSAuthenticationPlugin (MySQL). The token is fetched when a connection needs one and never saved.
connections-gcp-iam-hint = Signs in as the user below — the IAM user or service account Cloud SQL knows — with a token from the Google Cloud CLI (`gcloud auth login` first). The token is fetched when a connection needs one and never saved.
# $account is the Windows account, DOMAIN\user.
connections-windows-hint-named = Signs in as the Windows account Schemaic runs as ({ $account }). The server must know that account as a Windows login.
connections-windows-hint = Signs in as the Windows account Schemaic runs as. The server must know that account as a Windows login.
connections-database-file = Database file
# The file pickers' window titles.
connections-database-file-dialog = Select SQLite database
connections-sqlite-hint = A SQLite database is a single file, so there is no server to reach — no host, user, password or tunnel. Manual transactions aren't available on one yet: statements commit as they are sent.
connections-ssh-tunnel = SSH tunnel
connections-ssh-tunnel-hint = Reach the server through an SSH tunnel.
connections-ssh-host = SSH host
connections-ssh-port = SSH port
connections-ssh-user = SSH user
connections-ssh-password = SSH password
connections-ssh-agent-hint = Signing is delegated to your running SSH agent (OpenSSH agent / Pageant on Windows). No key is stored by Schemaic.
connections-jump-hosts = Jump hosts
connections-jump-hosts-hint = Optional. Hosts to pass through before the SSH host, nearest first — each signs in the way set below.
connections-private-key = Private key
connections-private-key-dialog = Select private key
connections-passphrase = Passphrase
connections-tls = SSL / TLS
connections-ca-certificate = CA certificate
connections-ca-certificate-dialog = Select CA certificate
connections-ca-certificate-hint = Leave empty to trust the certificate authorities your operating system trusts, including any your organisation installed. Name a file to trust that one instead.
connections-client-certificate = Client certificate
connections-client-certificate-dialog = Select client certificate
connections-client-key = Client key
connections-client-key-dialog = Select client key
connections-client-identity-hint = A client certificate and key are only needed where the server asks you to prove who you are (mutual TLS). The key must not be passphrase-protected.

## Import modal (import_view.rs)
## The format names (CSV / TSV, JSON, Excel) are core's and stay as they are.

# $table is the target table, as the schema tree names it.
import-title = Import into { $table }
import-section-source = Source
import-section-reading = Reading
import-section-columns = Columns
import-section-preview = Preview
import-file = File
import-choose-file = Choose a file…
# The file picker's window title.
import-file-dialog = Import into table
import-no-file = No file chosen
# Shown for a chosen file whose name cannot be read.
import-unnamed-file = (file)
import-format = Format
import-sheet = Sheet
import-delimiter = Delimiter
import-header-row = First row is a header
import-header-row-hint = Use the first row as column names instead of data.
import-trim = Trim whitespace
import-trim-hint = Strip spaces around every value — including inside quotes.
# NULL is the SQL keyword.
import-empty-is-null = Empty field is NULL
import-empty-is-null-hint = An empty field means NULL rather than an empty string.
import-null-tokens = Other values meaning NULL
# NULL, \N and NA are values as they appear in files: never translated.
import-null-tokens-hint = Comma-separated, e.g. NULL, \N, NA.
# A file column's mapping that imports it nowhere.
import-skip = Skip
import-nothing-mapped = Nothing is mapped, so there's nothing to import.
import-values-withheld = This file's first record is too large to preview, so only the column names were read. Mapping and import read the whole file.
# $columns is a comma-separated list of column names.
import-missing-required = Not mapped, and can't be empty: { $columns }. The import will fail unless the database fills them in.
# $engine is the storage engine's name (MyISAM, MEMORY).
import-engine-not-transactional = This table's storage engine ({ $engine }) is not transactional, so a failed import can't be undone — the rows loaded before the failure will remain.
import-problems = { $count ->
    [one] { $count } problem in the file — nothing was imported.
   *[other] { $count } problems in the file — nothing was imported.
}
# One problem in the list: $line is the record's number, $column the column's
# name and $message what is wrong (one of the import-issue-* messages).
import-problem-at-line = line { $line }: { $message }
import-problem-at-column = line { $line }, { $column }: { $message }
# What is wrong with a field. English must read exactly as core's
# IssueKind::message (a test holds it).
import-issue-not-an-integer = not a whole number
import-issue-not-a-number = not a number
import-issue-not-a-boolean = not a true/false value
import-issue-cell-error = the sheet could not evaluate this cell
import-issue-null-in-not-null = empty, but the column can't be NULL
import-issue-field-count = has { $found } fields, expected { $expected }
import-cancelled = Import cancelled — the transaction rolled back, so nothing was written.
import-done = { $count ->
    [one] Imported { $count } row into { $table }.
   *[other] Imported { $count } rows into { $table }.
}
import-next = Next
import-reading = Reading…
import-back = Back
import-import = Import
import-importing = Importing…
# Beside a disabled write button, saying why it is disabled.
dialog-read-only = This connection is read-only.

## Export modal — a database to a .sql file or a folder of files (dump_view.rs)
## $format is the export format's name (SQL, CSV, JSON, Excel…): never translated.

# $database is the database; $schema the namespace inside it.
dump-title-schema = Export { $database }.{ $schema } to { $format }
dump-title = Export { $database } to { $format }
# The file pickers' window titles.
dump-sql-dialog = Export to SQL
dump-folder-dialog = Export to { $format }
dump-folder-hint = One { $format } file per table, written into a folder you choose.
dump-all-selected = All { $total } selected
dump-some-selected = { $chosen } of { $total } selected
dump-select-all = Select all
dump-select-none = None
dump-reading-tables = Reading the table list…
dump-no-tables = This database has no tables.
dump-tables-unreadable = The table list could not be read.
dump-section-contents = Contents
# CREATE TABLE and INSERT are SQL keywords.
dump-structure = Structure
dump-structure-hint = CREATE TABLE, its triggers, and the foreign keys, restated after the data.
dump-data = Data
dump-data-hint = Every row, as INSERT statements.
dump-other-objects = Other objects
dump-other-objects-hint = Types, sequences, routines and events these tables lean on.
dump-section-replaying = Replaying
dump-drop-before-create = Drop before create
dump-one-transaction = One transaction
dump-one-transaction-hint = Wrap the load, so a failure leaves nothing behind.
dump-disable-fk-checks = Disable foreign-key checks
dump-no-fk-guard-hint = This engine has no session switch for foreign-key checks; the file adds the constraints after the data instead.
dump-export = Export
# The progress line; $rows is an abbreviated count (9.4k).
dump-progress = Writing { $table } — { $index } of { $total }, { $rows } rows so far
# Animated with trailing dots while it lasts.
dump-reading-schema = Reading the schema
# The title of the prompt asking whether to overwrite files already in the folder.
dump-replace-files = Replace files

## Grid export's progress box (dump_view.rs)
## $rows and $total are abbreviated counts (9.4k, ~180k).

export-progress-of-about = { $rows } of ~{ $total } rows
export-progress-of = { $rows } of { $total } rows
# $count is the exact number, which decides the plural; $rows is how it is shown.
export-progress-rows = { $count ->
    [one] { $rows } row
   *[other] { $rows } rows
}
# Animated with trailing dots while it lasts.
export-progress-starting = Starting
# $name is the destination file's name.
export-progress-file = File: { $name }
export-progress-writing = Writing { $name }

## Live monitor (monitor_view.rs)
## The Action column's INSERT/UPDATE/DELETE are SQL keywords and stay as they are.

monitor-title = Live monitor
monitor-badge-watching = Watching
monitor-badge-paused = Paused
monitor-badge-stopped = Stopped
monitor-filter-all = All
monitor-filter-inserts = Inserts
monitor-filter-updates = Updates
monitor-filter-deletes = Deletes
monitor-resume = Resume polling
monitor-pause = Pause polling
monitor-clear-tip = Clear the log
monitor-export-tip = Export the log…
# A poll interval in the dropdown, in seconds.
monitor-interval = { $secs }s
monitor-clear-title = Clear the log?
monitor-clear-message = { $count ->
    [one] { $count } change would be discarded. The log is the only record of what a deleted row held — the poll will not report a change again — and this can't be undone. Export it first if you want to keep it.
   *[other] { $count } changes would be discarded. The log is the only record of what a deleted row held — the poll will not report a change again — and this can't be undone. Export it first if you want to keep it.
}
monitor-col-time = Time
monitor-col-action = Action
monitor-no-inserts = No inserts yet.
monitor-no-updates = No updates yet.
monitor-no-deletes = No deletes yet.
monitor-empty-stopped = Stopped.
monitor-empty-paused = Paused.
# Animated with trailing dots while it lasts.
monitor-waiting = Waiting
# The footer's line is made of these, joined by " · ". $since is a time (11:48:12).
monitor-events-since = { $count ->
    [one] { $count } event since { $since }
   *[other] { $count } events since { $since }
}
monitor-no-changes = No changes yet
monitor-partial = only the first { $cap } rows by primary key are watched
monitor-capped = the oldest are dropping at the { $cap }-change cap
# $columns is a comma-separated list of column names; $count how many.
monitor-length-only = { $count ->
    [one] { $columns } is binary, so its changes are seen by length only
   *[other] { $columns } are binary, so their changes are seen by length only
}
monitor-newest-at-bottom = newest at the bottom
# The file picker's window title.
monitor-export-dialog = Export change log

## Compare results (result_diff_view.rs)
## A and B name the two results compared and stay as they are; the table's
## CHANGED/ADDED/REMOVED are core's words, which the export file writes too.

diff-title = Compare results
diff-filter-all = All
diff-filter-changed = Changed
diff-filter-added = Added
diff-filter-removed = Removed
diff-match-rows-by = Match rows by
# The key picker's entry for matching on the whole row rather than a column.
diff-whole-row = Whole row
diff-export-tip = Export the comparison…
diff-nothing-to-export = Nothing to export.
# The file picker's window title.
diff-export-dialog = Export comparison
# Names the file in the saved notice when its own name cannot be read.
diff-unnamed-file = the comparison
# $time is when the statement ran (11:48:12).
diff-ran-at = Ran { $time }
# $ms is how long it took, in milliseconds.
diff-side-facts = { $count ->
    [one] { $count } row · { $ms } ms
   *[other] { $count } rows · { $ms } ms
}
# The notes under the table, joined by a middle dot. $side is A or B; $columns a
# comma-separated list of column names.
diff-only-in = Only in { $side }: { $columns }
diff-uncompared = Binary, not compared: { $columns }
diff-repeated-keys = { $count ->
    [one] { $count } row repeats a key already taken on its side, and is left out.
   *[other] { $count } rows repeat a key already taken on its side, and are left out.
}
diff-capped = A result was capped: rows past its cap are not compared.
diff-col-change = Change
diff-no-differences = No differences.
diff-no-changed = No changed rows.
diff-no-added = No added rows.
diff-no-removed = No removed rows.

## The DDL preview — the plan a schema editor or a comparison applies (ddl_preview.rs)
## The change list, the risk heading and its lines, and the Used by,
## withheld and omitted lines are core's words and stay as they are for now.

# $connection is the connection's name; $subject what the plan changes
# (shop.orders, app@%).
preview-title = Apply changes to { $connection } · { $subject }
# The heading over the change list.
preview-changes = { $count ->
    [one] { $count } Change
   *[other] { $count } Changes
}
preview-used-by = Used by
preview-withheld = This engine can't express part of this plan
preview-withheld-hint = Nothing is applied while this is listed.
preview-omitted = Not included in this migration
preview-omitted-hint = Applying leaves these objects as they are.
preview-copy = Copy
preview-open-in-editor = Open in editor
# Returns to the editor the plan came from, with the draft intact.
preview-back = Back
preview-apply = Apply
preview-applying = Applying…
preview-applied = { $count ->
    [one] Applied { $count } statement to { $subject }.
   *[other] Applied { $count } statements to { $subject }.
}
preview-refreshed = The schema has been refreshed.

## Import Connections modal (connection_import.rs)
## Why an entry was skipped, and what is wrong with a pasted URL, are core's
## words and stay as they are for now.

conn-import-title = Import Connections
conn-import-url = Connection URL
# The button inside the URL field.
conn-import-add = Add
# DATABASE_URL and .env are a variable and a file name: never translated.
conn-import-url-hint = A connection URL, a JDBC URL, or a DATABASE_URL line out of a .env — one per line.
conn-import-choose-file = Choose a file…
conn-import-scan = Scan installed clients
conn-import-scanning = Scanning…
conn-import-docker = Docker containers
conn-import-asking-docker = Asking Docker…
conn-import-looking = Looking for connections on this machine…
conn-import-none-found = No connections found. DBeaver, the JetBrains IDEs and the command-line clients keep their files in known places; if yours is elsewhere, choose it above.
conn-import-nothing-yet = Nothing found yet — paste a URL, read a file, or scan for clients you already use.
conn-import-found = { $count ->
    [one] { $count } connection found
   *[other] { $count } connections found
}
conn-import-select-all = Select all
conn-import-select-none = None
# A row's notes, joined by a middle dot. English must read exactly as core's
# ImportNote::label (a test holds it).
conn-import-note-no-password = No password in the source
conn-import-note-already-saved = Already saved
conn-import-note-unexpanded-path = Path contains an unexpanded macro
conn-import-note-pgpass = Password taken from your own .pgpass
conn-import-note-port-assumed = The source named no port; this is the default
conn-import-note-external-login = Signs in a way Schemaic can't: set a SQL login
# $names is up to three entries, each "name (why)", joined by commas.
conn-import-skipped = { $count ->
    [one] { $count } entry was not imported: { $names }.
   *[other] { $count } entries were not imported: { $names }.
}
# As above, with $rest more entries counted rather than named.
conn-import-skipped-more = { $count ->
    [one] { $count } entry was not imported: { $names }, and { $rest } more.
   *[other] { $count } entries were not imported: { $names }, and { $rest } more.
}
conn-import-import = Import
# The Import button with how many connections it will add.
conn-import-import-count = Import { $count }

## Binary value panel (blob_view.rs)
## Sizes ($size, $shown, $got, $most) arrive formatted (16 B, 64.0 MB).

# What a value is. English must read exactly as core's BlobKind::label (a
# test holds it); the save dialog's file type still uses core's.
blob-kind-png = PNG image
blob-kind-jpeg = JPEG image
blob-kind-gif = GIF image
blob-kind-bmp = BMP image
blob-kind-webp = WebP image
blob-kind-ico = Icon
blob-kind-opaque = Binary data
# The line over the value; $kind is one of the blob-kind-* messages.
blob-summary = { $kind } · { $size }
blob-summary-truncated = { $kind } · { $size } · showing the first { $shown }
blob-pane-preview = Preview
blob-pane-hex = Hex
blob-reading = Reading…
# NULL is the SQL keyword.
blob-empty = This cell is NULL, or its row is no longer there.
blob-preview-too-large = This image is { $width } × { $height } — too large to preview here. The bytes are intact: read them as Hex, or save them to a file.
blob-preview-truncated = Only the first { $shown } of this { $kind } was read, so it cannot be drawn. Read the bytes as Hex.
blob-preview-unreadable = These bytes begin like a { $kind } but cannot be read as one. Read them as Hex, or save them to a file.
blob-save-truncated = Only the first { $shown } of { $size } could be read, so this would save the front of the value.
blob-load = Load from file
blob-save = Save to file
# The file pickers' window titles.
blob-load-dialog = Load binary value from file
blob-save-dialog = Save binary value
# $path is where the file was written.
blob-saved-to = Saved to { $path }
blob-loaded-file = Loaded file. Commit in the grid to write it.
blob-not-writable = This cell cannot be written.
blob-file-too-large = That file is { $got } — this column holds at most { $most }.
blob-not-accepting = That cell is no longer accepting a value — nothing was staged.

## ER diagram (erd_view.rs)
## The export formats' names (PNG, SVG, Mermaid…) are core's and stay as they are.

erd-title = ER diagram
# Before the diagram's database or table.
erd-scope = Scope
# The toolbar's counts. Unpluralised in English as they always were; a
# language that needs a plural may select on $count.
erd-chip-tables = { $count } tables
erd-chip-relationships = { $count } relationships
erd-chip-hidden = { $count } unrelated hidden
erd-find-placeholder = Find table or column
# A collapsed table's row for the columns it is not showing; also drawn in an
# exported picture.
erd-more-columns = +{ $count } more
erd-show-less = show less
erd-not-loaded = Schema isn't loaded for this database yet.
erd-no-relationships = No foreign-key relationships to diagram.
erd-export-tip = Export the diagram…
erd-copy-as = Copy as
# The file picker's window title.
erd-export-dialog = Export diagram
erd-nothing-to-export = Nothing to export.
erd-nothing-to-copy = Nothing to copy.
# $format is the export format's name.
erd-copied-as = Copied as { $format }
erd-clipboard-unavailable = The clipboard is not available.
# $error is the platform's own message.
erd-copy-failed = Couldn't copy: { $error }
# $name is the file written.
erd-saved = Saved { $name }
# Names the file in the saved notice when its own name cannot be read.
erd-unnamed-file = the file

## Server variables modal (server_vars_view.rs)

# $connection is the connection's name.
server-vars-title = Server variables — { $connection }
server-vars-search = Search settings
server-vars-variables = Variables
server-vars-status = Status
server-vars-loading = Loading…
server-vars-unsupported = This connection's engine publishes no list of this kind.
server-vars-no-match = No matching settings.
# Under the rows shown, for the matches past the list's cap.
server-vars-more = { $count ->
    [one] { $count } more setting matches — narrow the search
   *[other] { $count } more settings match — narrow the search
}
server-vars-count = { $count ->
    [one] { $count } setting
   *[other] { $count } settings
}
# While the search narrows the list; $count is the total.
server-vars-count-filtered = { $shown } of { $count } settings
server-vars-refresh = Refresh

## Edit snippet modal (snippet_edit.rs)

# $name is the snippet's name.
snippet-edit-title = Edit snippet — { $name }
snippet-edit-name = Name
snippet-edit-name-placeholder = Snippet name
snippet-edit-shortcut = Expansion shortcut
# The empty shortcut field: the snippet has no shortcut.
snippet-edit-shortcut-none = none
snippet-edit-query = Query
snippet-edit-save = Save

## Query plan modal (plan_view.rs)
## The Analyze toggle names EXPLAIN ANALYZE and is never translated; the plan's
## columns, rows and warnings are the server's and core's.

plan-title = Query plan
# Shown in place of the Analyze toggle; "Analyze" is the SQL keyword.
plan-analyze-unavailable = Analyze unavailable (statement writes data)
plan-analyze-hint = Executes the statement to measure real timings
# Animated with trailing dots while it lasts.
plan-explaining = Explaining
plan-potential-issues = Potential issues
plan-ask-ai = Ask AI

## Used by modal (used_by_view.rs)
## A dependent's detail ("on lines") is core's and stays as it is for now.

# $object is what was asked about (shop.orders). Also the Copy text's first line.
used-by-title = Used by — { $object }
used-by-copy = Copy
used-by-not-loaded = { $database }'s schema has not loaded yet — expand it in the tree, then ask again.
used-by-nothing-in = Nothing in { $database } uses it.
# The Copy text when nothing does.
used-by-nothing = Nothing uses it.
used-by-limits = Found by reading definitions in this database: SQL built at run time, and other databases, are not seen.
# $detail is what core says about the dependent (on lines).
used-by-dropped-with-it = { $detail } · dropped with it
# Each kind's group heading with its count. English must read as core's
# DependentKind::noun, capitalised (a test holds it).
used-by-heading-view = { $count ->
    [one] View ({ $count })
   *[other] Views ({ $count })
}
used-by-heading-routine = { $count ->
    [one] Routine ({ $count })
   *[other] Routines ({ $count })
}
used-by-heading-trigger = { $count ->
    [one] Trigger ({ $count })
   *[other] Triggers ({ $count })
}
used-by-heading-event = { $count ->
    [one] Event ({ $count })
   *[other] Events ({ $count })
}
used-by-heading-foreign-key = { $count ->
    [one] Foreign key ({ $count })
   *[other] Foreign keys ({ $count })
}
used-by-heading-expression = { $count ->
    [one] Table expression ({ $count })
   *[other] Table expressions ({ $count })
}
# A kind in the singular, as the Copy text's list names it.
used-by-noun-view = view
used-by-noun-routine = routine
used-by-noun-trigger = trigger
used-by-noun-event = event
used-by-noun-foreign-key = foreign key
used-by-noun-expression = table expression

## Interface themes (themes.rs) — Settings › Appearance's first dropdown.
## The editor themes' names (One Dark Pro, Tokyo Night…) are product names.

theme-dark = Dark
theme-light = Light

## The assistant's rendered replies (markdown.rs)
## The model's own text is never translated; these are the controls around it.

# A code block's label when it names no language; "SQL" stays as it is.
md-code-block = CODE
md-code-copy = Copy
md-code-insert = Insert
md-code-run = Run
# A proposed table change's card; $table is the table.
md-proposal-title = Proposed change to { $table }
md-proposal-count = { $count ->
    [one] { $count } change
   *[other] { $count } changes
}
md-proposal-review = Review change…
# $error says why the proposal could not be read.
md-proposal-unreadable = The assistant proposed a change that couldn't be read — { $error }

## The grid's cell pickers and calendar (cell_editors.rs)

# An enum, set or date cell with nothing chosen yet.
cell-choose = Choose…
# The calendar's header; $month is one of the cell-month-* messages.
cell-calendar-month = { $month } { $year }
# The calendar's button for the current instant (a date-and-time cell) or date.
cell-calendar-now = Now
cell-calendar-today = Today
# English must read exactly as core's MONTH_NAMES and WEEKDAY_INITIALS (a test
# holds it). The week starts on Monday.
cell-month-1 = January
cell-month-2 = February
cell-month-3 = March
cell-month-4 = April
cell-month-5 = May
cell-month-6 = June
cell-month-7 = July
cell-month-8 = August
cell-month-9 = September
cell-month-10 = October
cell-month-11 = November
cell-month-12 = December
cell-weekday-mon = Mo
cell-weekday-tue = Tu
cell-weekday-wed = We
cell-weekday-thu = Th
cell-weekday-fri = Fr
cell-weekday-sat = Sa
cell-weekday-sun = Su

## Rename alias / Rename CTE dialog (alias_rename.rs; its two openers are the
## editor's menu entries in editor_pane.rs). CTE is a SQL term.

alias-rename-title-alias = Rename alias
alias-rename-title-cte = Rename CTE
alias-rename-placeholder-alias = Alias
alias-rename-placeholder-cte = CTE name
alias-rename-new-name = New name
alias-rename-rename = Rename

## Why it will not rename (core's AliasRenameRefusal). Said of an alias and a
## CTE alike. English must read exactly as core's message (a test holds it).

alias-rename-no-alias = There is nothing here to rename any more.
alias-rename-empty = Give it a name.
# $name is the name typed, blanks trimmed.
alias-rename-conflict = “{ $name }” is already a name in this query: renaming to it would change which table a reference reads.
alias-rename-unsure = A reference here may read another column of this name, or read this one where the rename cannot follow, and the loaded schema cannot tell — so no edit is made. Load the tables' schema, or rename by hand.
alias-rename-changed = The query changed while this was open. Close it and rename again.
alias-rename-read-only = The editor is read-only until the AI suggestion is accepted or rejected.

## The editor's usages list (usages_view.rs)

# $name is what was looked up (an alias, a CTE, a column).
usages-title = { $count ->
    [one] { $count } usage of { $name }
   *[other] { $count } usages of { $name }
}

## Saving a diagram or chart as PNG (erd_raster.rs) — the failed export's notice.
## $error is the library's own message.

raster-invalid-svg = Invalid SVG: { $error }
# "Image", not "Diagram": the chart's PNG comes through here too.
raster-too-large = Image is too large to rasterise ({ $width }×{ $height })
raster-png-failed = PNG encoding failed: { $error }

## The results grid's context menus (grid.rs) — a cell's, a row gutter's and a
## column header's. SQL keywords (NULL, DEFAULT, IN) stay as they are, and
## the export formats' names (CSV, JSON, SQL VALUES…) are not here.

grid-menu-copy = Copy
# A cell's Copy when it copies the one raw value rather than the selection.
grid-menu-copy-value = Copy value
grid-menu-copy-formatted = Copy formatted
grid-menu-copy-as = Copy as
grid-menu-paste = Paste
grid-menu-fill-down = Fill down
grid-menu-fill-up = Fill up
grid-menu-edit-field = Edit field
# Opens the whole-row panel: Edit row where something in it is writable.
grid-menu-edit-row = Edit row
grid-menu-view-row = View row
grid-menu-edit-binary = Edit binary
grid-menu-view-binary = View binary
grid-menu-filter-by-value = Filter by this value
grid-menu-exclude-value = Exclude this value
grid-menu-follow-relation = Follow relation
# A submenu of the tables whose keys point at this row.
grid-menu-referenced-by = Referenced by
grid-menu-set-null = Set to NULL
grid-menu-set-default = Set to DEFAULT
grid-menu-duplicate-rows = { $count ->
    [one] Duplicate row
   *[other] Duplicate { $count } rows
}
grid-menu-delete-rows = { $count ->
    [one] Delete row
   *[other] Delete { $count } rows
}
grid-menu-undo-delete = Undo delete
grid-menu-ai-summary = AI summary
# $scope is what would be sent, as core phrases it (3 rows × 2 columns from
# shop.orders); it stays English for now.
grid-attach-to-chat = Attach { $scope } to chat
grid-menu-freeze = Freeze
grid-menu-unfreeze = Unfreeze
grid-menu-hide-column = Hide column
grid-menu-show-hidden-columns = Show hidden columns ({ $count })
grid-menu-show-all = Show all
grid-menu-format-as = Format as
grid-menu-copy-name = Copy name
grid-menu-copy-qualified-name = Copy qualified name
grid-menu-copy-values = Copy values
grid-menu-sql-in-list = SQL IN list

## The results grid's toolbar (grid.rs) — the status line, the commit and row
## controls, and the copy, save, AI and chart icons.

# The status line. $rows is core's phrase for what was read (1,000 of ~4.2m
# rows (capped)), which stays English for now; $ms is the run time.
grid-stats = { $rows } · { $cols ->
    [one] { $cols } col
   *[other] { $cols } cols
} · { $ms } ms
# The same, led by the database the result came from.
grid-stats-in-database = { $database } · { $rows } · { $cols ->
    [one] { $cols } col
   *[other] { $cols } cols
} · { $ms } ms
# Beside the status line for a result kept (pinned) on its panel.
grid-kept-note = · kept — read-only
# $columns lists the columns whose values ran past what the grid holds.
grid-arena-note = · { $columns } too large to hold in full — later rows show blank
grid-sorted-subset = · sorted subset (capped) — not the full order
# Replaces the "read all rows" offer while that re-read runs.
grid-reading = reading…
grid-committing = Committing…
# $keys is the shortcut (Ctrl+Enter), in the keyboard's own legends.
grid-commit-tip = { $count ->
    [one] Commit { $count } change ({ $keys })
   *[other] Commit { $count } changes ({ $keys })
}
grid-discard-tip = Discard all pending changes
grid-copy-sql-tip = Copy the changes as SQL
grid-add-row-tip = Add a row
# $key is the Delete key's legend.
grid-delete-row-tip = Mark the selected row for deletion ({ $key })
grid-clone-row-tip = Clone the selected row
grid-copy-results-tip = Copy the results…
grid-copy-results-empty = Nothing to copy — no rows
grid-save-results-tip = Save the results to a file…
grid-save-results-empty = Nothing to save — no rows
grid-ai-tip = Generate rows with AI…
grid-ai-generating = Generating…
# The AI menu's one entry on a connection whose data access is schema-only.
grid-ai-off = AI actions send data — off for this connection
grid-ai-fill-value = AI fill value
grid-ai-insert-row = AI insert row
grid-ai-seed-table = AI seed table
grid-chart-tip = Chart the results
grid-chart-empty = Nothing to chart — no rows
# What the chart's title (Chart — …) calls a result charted from the grid.
grid-chart-this-result = this result

## The results grid's bars (grid.rs) — the error bar, the lock-wait bar, find,
## the selection readout and go to row. The wait and note bars' sentences are
## core's and stay English for now.

# Opens the full error in a modal.
grid-error-view = View
# The lock-wait bar's button; $tab is the blocking tab's name, clipped.
grid-rollback-tab = Roll back { $tab }
grid-find-placeholder = Find in results
# Stands in for a figure of the selection readout just after it is clicked.
grid-figure-copied = Copied
grid-goto-label = Go to row:

## The results grid's clipboard, fill, attach and export (grid.rs). The
## export's own notes (rows written, columns withheld) are core's and stay
## English for now; so does "the clipboard" or "the file" they name.

grid-fill-nothing = Nothing to fill: the selected columns can't be written, or the cell to copy is a new row's unset value or a staged DEFAULT.
# $filled and $skipped are counts written out (1,204); $count is $filled as a
# number, for the plural.
grid-fill-skipped = { $count ->
    [one] Filled { $filled } cell, skipping { $skipped } in rows marked for deletion.
   *[other] Filled { $filled } cells, skipping { $skipped } in rows marked for deletion.
}
# What a chat attachment's chip says it holds, kept with the conversation.
grid-attach-summary = { $rows ->
    [one] { $rows } row
   *[other] { $rows } rows
} × { $cols ->
    [one] { $cols } column
   *[other] { $cols } columns
}
# The same, for rows read from one table; $table is its name.
grid-attach-summary-from = { $rows ->
    [one] { $rows } row
   *[other] { $rows } rows
} × { $cols ->
    [one] { $cols } column
   *[other] { $cols } columns
} from { $table }
# $summary is one of the two above, when the selection ran past what is sent.
grid-attach-summary-capped = { $summary } (the first { $taken } of { $total } selected)
grid-copy-sql-unset = Nothing to copy as SQL — the selected cells of the new row have no value yet, and take the server's default.
grid-copy-sql-joined = Nothing to copy as SQL: these rows come from a query over more than one table.
# The export menu's scope for the rows already read; $count is written out.
grid-export-fetched-rows = Fetched rows ({ $count })
# The Save dialog's title.
grid-export-dialog-title = Export results
grid-export-already-running = An export is already running — nothing was written.

## The results grid's row panel (grid.rs) — Edit row / View row. The <null>
## sentinel is SQL and stays as it is.

# $row is the row's number on screen.
grid-row-title = Row { $row }
# The same, for a row read from one table; $table is its name.
grid-row-title-in-table = Row { $row }  ·  { $table }
grid-row-set-value = Set value
grid-row-set-null = Set NULL
# A binary field's button; the row already names the column, so one word.
grid-row-edit-binary = Edit
grid-row-view-binary = View
# $error is the JSON parser's own message.
grid-row-invalid-json = Invalid JSON: { $error }
# Animated with trailing dots while the row is written.
grid-row-saving = Saving

## The rest of the results grid (grid.rs) — a statement's affected-rows line,
## the filter's refusals, the hidden-columns chip, new-row cells and the AI
## seed popover. <null> and <default> in a new row's cell are SQL and stay.

# In place of a grid, for a statement that returned no rows; $ms is the run time.
grid-rows-affected = { $count ->
    [one] { $count } row affected · { $ms } ms
   *[other] { $count } rows affected · { $ms } ms
}
grid-rerun-would-write = Can't re-run this statement — it would write to the database
grid-filter-not-simple = Can't filter this query — not a simple single-table SELECT
grid-hidden-columns-tip = { $count ->
    [one] { $count } hidden column — click to show all, or right-click a header to choose
   *[other] { $count } hidden columns — click to show all, or right-click a header to choose
}
grid-copied-staged-sql = Copied the staged changes as SQL.
# The confirmation's title; its question is core's and stays English for now.
grid-duplicate-title = Duplicate rows
# A column header's tooltip on the mark for a column that allows NULL.
grid-nullable-tip = Nullable
# What a new row's unset cell will become: an auto-increment value, or an
# error if left blank. Kept in angle brackets, as <null> and <default> are.
grid-cell-auto = <auto>
grid-cell-required = <required>
grid-seed-title = Seed rows
grid-seed-generate = Generate

## The status bar (lib.rs's footer) — its panel toggles, the editor's
## segments, the transaction controls and the resource readings.

# The panel toggles' tooltips: each names its panel, in sentence case.
footer-panel-schema = Schema
footer-panel-ai = AI assistant
footer-panel-history = Query history
footer-panel-snippets = Snippet library
footer-panel-activity = Server activity
footer-panel-terminal = Terminal
# A toggle's tooltip in place of its name while the window is too narrow.
footer-panel-narrow = The window is too narrow for this panel — widen it to bring the panel back.
# The caret's position in the editor.
footer-cursor = Ln { $line }, Col { $col }
# The indentation segment; $width is the indent's width.
footer-indent-spaces = Spaces: { $width }
footer-indent-tabs = Tabs: { $width }
footer-spaces = Spaces
footer-tabs = Tabs
footer-wrap = Wrap
footer-no-wrap = No wrap
footer-read-only = Read-only
footer-write-mode = Write mode
# The tab's transaction mode.
footer-tx-auto = Auto-commit
footer-tx-manual = Manual
# While a manual transaction is open; $count is the statements run in it.
footer-tx-open = { $count } Open
footer-tx-aborted = Tx aborted — rollback to continue
footer-tx-lost = Transaction lost
footer-commit = Commit
footer-rollback = Rollback
# $value is the reading (12%, 1.4 GB).
footer-cpu = CPU: { $value }
footer-ram = RAM: { $value }

## lib.rs's shared words — a new tab's title, a result's default name, the
## AI-off note and the connection test's status line.

# A query tab with no name and no file; $n numbers it. Also the file name a
# first Save suggests.
lib-tab-default-title = Query { $n }
# A result chip whose statement has no text to show; $n numbers it.
lib-result-default-label = Result { $n }
# What every AI surface says while the assistant is off.
lib-ai-off-note = The AI assistant is off. Pick an Agent CLI in AI settings to turn it on.
# Beside the connection form's Test button.
lib-test-testing = Testing…
lib-test-connected = Connected
lib-test-failed = Connection failed

## The settings choices lib.rs defines — Settings' dropdowns show them. Their
## persisted keys (minimal, xhigh, active, block…) are not here.

# The AI assistant's effort levels.
lib-effort-minimal = Minimal
lib-effort-low = Low
lib-effort-medium = Medium
lib-effort-high = High
lib-effort-extra = Extra
lib-effort-max = Max
# How much database structure each AI message carries. Settings › AI's hint
# names "None"; keep the two the same.
lib-schema-scope-active = Active database only
lib-schema-scope-all = All databases
lib-schema-scope-none = None
# The terminal's cursor shapes.
lib-cursor-block = Block
lib-cursor-bar = Bar
lib-cursor-underline = Underline

## The window's header (lib.rs) — the connection switcher, its tools and the
## disconnected notice — and the identity colours' names.

header-no-connection = No connection
header-new-connection = New connection
# $keys is the shortcut (Ctrl+P), in the keyboard's own legends.
header-find-anywhere-tip = Find Anywhere ({ $keys })
header-shortcuts-tip = Keyboard shortcuts
header-settings-tip = Settings
header-disconnected = Disconnected
header-retry = Retry
# The preset identity colours, as a database's or table's Colour menu names them.
colour-red = Red
colour-orange = Orange
colour-amber = Amber
colour-green = Green
colour-teal = Teal
colour-blue = Blue
colour-purple = Purple
colour-pink = Pink

## A file-backed tab's changed-on-disk banner, and the results area before a
## result (lib.rs).

# $name is the file's name.
disk-changed = “{ $name }” changed on disk.
disk-reload = Reload
disk-keep-mine = Keep mine
# A new tab's results area; $keys is the run shortcut (Ctrl+Enter).
results-run-hint = Run a query  ({ $keys })
results-failed = Statement failed.
results-cancelled = Query cancelled.

## The results strip's chips (lib.rs) — what a pinned result costs, a chip's
## tooltip and its menu.

# Beside the strip once pinned results hold a lot of memory; $size is how much
# (1.2 GB).
results-pinned-total = { $count } pinned · { $size }
results-pinned-tip = Pinned results are held in memory until you close them. Close the ones you are done with to give it back.
# A chip's tooltip, in place of a statement, before anything has run.
results-chip-nothing-run = Nothing has run in this tab yet.
# A chip's tooltip, for a pinned result.
results-chip-kept = kept — read-only
results-menu-pin = Pin
results-menu-unpin = Unpin
results-menu-close = Close
# $result is the other result's chip label, clipped.
results-menu-compare-with = Compare with { $result }
results-menu-close-others = Close other results
results-menu-close-all = Close all results

## Find Anywhere's search field (lib.rs's search_box).

search-everywhere-placeholder = Search everywhere

## Shared widgets (widgets.rs).

# The one entry of a picker whose list is empty.
widgets-nothing-to-choose = Nothing to choose from

# The loader's whimsical verbs, one picked at random each time a loader shows
# ("Pondering..."). Comma-separated; a language lists its own, as many as it
# likes, kept short enough for a compact loader (a test measures each).
widgets-spinner-verbs = Accomplishing, Actioning, Actualising, Architecting, Baking, Beaming, Beboppin', Befuddling, Billowing, Blanching, Bloviating, Boogieing, Boondoggling, Booping, Bootstrapping, Brewing, Burrowing, Calculating, Canoodling, Caramelising, Cascading, Catapulting, Cerebrating, Channeling, Channelling, Choreographing, Churning, Clauding, Coalescing, Cogitating, Combobulating, Composing, Computing, Concocting, Considering, Contemplating, Cooking, Crafting, Creating, Crunching, Crystallising, Cultivating, Deciphering, Deliberating, Determining, Dilly-dallying, Doing, Doodling, Drizzling, Ebbing, Effecting, Elucidating, Embellishing, Enchanting, Envisioning, Evaporating, Fermenting, Finagling, Flambeing, Flowing, Flummoxing, Fluttering, Forging, Forming, Frolicking, Frosting, Gallivanting, Galloping, Garnishing, Generating, Germinating, Gitifying, Grooving, Gusting, Harmonising, Hashing, Hatching, Herding, Honking, Hullaballooing, Hyperspacing, Ideating, Imagining, Improvising, Incubating, Inferring, Infusing, Ionising, Jitterbugging, Julienning, Kneading, Leavening, Levitating, Lollygagging, Manifesting, Marinating, Meandering, Metamorphosing, Misting, Moonwalking, Moseying, Mulling, Mustering, Musing, Nebulising, Nesting, Newspapering, Noodling, Nucleating, Orbiting, Orchestrating, Osmosing, Perambulating, Percolating, Perusing, Philosophising, Pollinating, Pondering, Pontificating, Pouncing, Precipitating, Processing, Proofing, Propagating, Puttering, Puzzling, Quantumising, Razzmatazzing, Reticulating, Roosting, Ruminating, Sauteing, Scampering, Schlepping, Scurrying, Seasoning, Shenaniganing, Shimmying, Simmering, Skedaddling, Sketching, Slithering, Smooshing, Sock-hopping, Spelunking, Spinning, Sprouting, Stewing, Sublimating, Swirling, Swooping, Symbioting, Synthesising, Tempering, Thinking, Thundering, Tinkering, Tomfoolering, Topsy-turvying, Transfiguring, Transmuting, Twisting, Undulating, Unfurling, Unravelling, Vibing, Waddling, Wandering, Warping, Whirlpooling, Whirring, Whisking, Wibbling, Working, Wrangling, Zesting, Zigzagging

## The editor's inline AI prompt (Ctrl+K; editor_pane.rs's cmdk_popup). What
## it sends the model stays English.

cmdk-placeholder = Ask the AI Assistant for help.
# Keep or drop the suggested edit shown as a diff.
cmdk-accept = Accept
cmdk-reject = Reject

## The editor's bind-parameter row (editor_pane.rs). The kind chip's own
## labels (abc, 123, NULL, SQL) are symbols and stay.

params-value-placeholder = value
# On the kind chip; NULL and SQL are the chip's own words.
params-kind-tip = Text, number, NULL, or raw SQL — click to change
# Why the run holds (core's ParamError; English must read as its Display).
# $names is the list as written, e.g. ":id, :from".
params-missing = No value for { $names }
# $name is the parameter's name without its colon; $text what was typed.
params-not-a-number = :{ $name } is a number, but reads "{ $text }"

## The editor's error and guard bars (editor_pane.rs). Explain and AI fix are
## the error modal's own (error-modal-explain, error-modal-ai-fix).

# Opens the full error in the error modal.
editor-error-view = View
# The guard bar's button that runs a statement it warned about.
editor-run-anyway = Run anyway
# A squiggle's tooltip, under its message: the fix a key would make. $keys is
# the key (Alt+Enter); $fix is what it does.
editor-diag-fix = { $keys }: { $fix }

## The editor's context menu (editor_pane.rs). AI fix is the error modal's
## message, Format code the shortcut's, and Rename alias / Rename CTE the
## rename dialog's titles.

editor-menu-ask-ai = Ask AI
# Explains the statement under the caret in the AI panel.
editor-menu-explain = Explain
editor-menu-optimise = Optimise
# Opens the statement's query plan.
editor-menu-plan = Plan
editor-menu-copy-single-line = Copy as single line
editor-menu-make-single-line = Make single line
editor-menu-format-statement = Format statement
editor-menu-find-usages = Find usages
editor-menu-create-view = Create view
# What the inline AI box shows while Optimise runs (the request it sends the
# model stays English).
cmdk-optimise-label = Optimise this query
# Refused at Ctrl+Enter with several carets and no selection.
editor-several-carets = Several carets and nothing selected: select what to run, or leave one caret

## The editor's run menu (Ctrl+Enter with several statements; editor_pane.rs).

editor-run-current = Run Current
editor-run-everything = Run Everything

## The editor's find / replace bar and go to line (editor_pane.rs). The option
## glyphs Aa, W and .* stay as they are.

find-placeholder = Find
# In place of the match count while the pattern is not a valid regex.
find-invalid = Invalid
find-match-case = Match case
find-whole-word = Whole word
find-regex = Regular expression
# The in-selection toggle's short glyph; keep it as short.
find-scope = Sel
find-in-selection-tip = Find in selection
find-replace-placeholder = Replace
find-replace = Replace
# Replaces every match.
find-replace-all = All
goto-line-label = Go to:

## The database CLI and the backup tools, when none is found (main.rs). The
## names ($client, $tool, $first, $second) are program names, never translated.
## The terminal panel prints these through a shell's echo, which turns quotes
## and most symbols into spaces — keep to letters and plain punctuation.

# $client is a client's program name (psql, sqlcmd, mysql/mariadb).
cli-no-client = No { $client } client found (PATH or WSL).
cli-no-client-on-path = No { $client } client found on PATH.
cli-no-tool = No { $tool } found on PATH.
cli-no-either-tool = No { $first } or { $second } found on PATH.
cli-no-either-client = No { $first } or { $second } client found on PATH.
cli-sqlcmd-windows-in-wsl = Windows sign-in needs a Windows sqlcmd, and the one found is inside WSL. Install sqlcmd on Windows, or open the connection in a query tab.

## Settings › Install's failures (install_cli.rs). What it reports on success
## is core's and stays English for now. $error is the operating system's own
## text; $path a file path.

install-no-exe-path = Can't tell where Schemaic is running from: { $error }
install-registry-open = Couldn't open your user environment in the registry: { $error }
install-path-read = Couldn't read your user PATH: { $error }
install-path-write = Couldn't write your user PATH: { $error }
install-look-at = Couldn't look at { $path }: { $error }
install-read-link = Couldn't read the link at { $path }: { $error }
install-create = Couldn't create { $path }: { $error }
install-remove-old = Couldn't remove the old { $path }: { $error }
install-remove = Couldn't remove { $path }: { $error }
# Asked of the wrong platform's install, which the Settings row never offers.
install-path-is-windows = A user PATH entry is a Windows install; this platform links instead.
install-link-is-unix = Linking the command is a macOS and Linux install.

## The connection gate's refusals (main.rs) — what a run, a query plan or a
## grid commit says when the check that the server is reachable stops it.

gate-plan-not-connected = Not connected — the plan was not run. The server didn't answer the health check, so nothing was sent to it.
gate-plan-connection-changed = You switched connection while Schemaic was checking that it was reachable, so the plan was not run. Switch back and ask again.
gate-plan-tab-moved-on = You switched tabs while the connection was being re-checked, so the plan was not run. It describes the tab it was asked from — go back to that tab and ask again.
# $name is the connection's name, or gate-this-connection.
gate-connection-changed = Didn't run against { $name } — you switched connection while Schemaic was checking that it was reachable. Switch back and try again.
gate-not-connected = Not connected to { $name }. The server didn't answer — check that it's running and that this connection's settings are right.
# Stands in for $name above when the connection has no name to give.
gate-this-connection = this connection
gate-run-tab-moved-on = You switched tabs while the connection was being re-checked, so the statement was not run. It was checked against the tab it was typed in, and running it here would run it somewhere else. Go back to that tab and run it again.
gate-commit-not-connected = Not connected — the commit was not attempted. Staged edits are kept.

## The terminal panel's own lines, and Back up / Restore (main.rs). Most are
## printed in the panel through a shell's echo, which turns quotes and most
## symbols into spaces — keep to letters and plain punctuation. $error is the
## operating system's or the engine's own text; $path a file path.

# $program is the shell's program, as Settings › Terminal names it.
term-shell-failed = Couldn't start { $program }: { $error }. Pick another shell in Settings, Terminal.
term-cli-read-only = This connection is read-only in Schemaic. Its command-line client would not be, so it is not opened here.
backup-read-only-server = This connection is read-only in Schemaic, and this engine's backup is written by the server, so none was taken.
backup-dialog-title = Back up database
# The save dialog's file-type filter, one per engine's backup format.
backup-kind-pg-dump = PostgreSQL custom-format dump
backup-kind-sql = SQL script
backup-kind-sqlite = SQLite database
backup-not-started = The backup did not start: { $error }
backup-done = Backed up to { $path }.
backup-failed = The backup failed: { $error }
# The open dialog's title, and the question asked before a restore runs.
restore-title = Restore from file
# $database is the database restored into; $connection the connection's name.
restore-confirm = Run { $path } into { $database } on { $connection }? What it creates or replaces cannot be undone. It runs in the terminal, where Ctrl+C stops it.
restore-read-only = This connection is read-only in Schemaic, so nothing is restored into it.
restore-engine-not-here = This engine's backups are not restored from here.
restore-unreadable = { $path } could not be read before restoring it, so nothing was run: { $error }

## Closing tabs and a tab's SQL file — open, save, reload (main.rs). $name is
## a tab's or a file's name; $path a file path; $size a size as the app
## writes one (1.2 MB); $error the operating system's own text.

tab-close-title = Close “{ $name }”
tab-close-unsaved = “{ $name }” has unsaved changes. Closing discards them; the file on disk is left as it is. Close anyway?
tab-close-all-title = Close all tabs
tab-close-all-question = Are you sure you want to close all the tabs?
tab-close-others-title = Close other tabs
tab-close-others-question = Are you sure you want to close all the other tabs?
file-open-title = Open SQL file
file-save-title = Save SQL file
# The open and save dialogs' file-type filter.
file-sql-filter = SQL script
file-read-failed = Couldn't read { $path }: { $error }
file-save-failed = Couldn't save { $path }: { $error }
file-too-large = { $path } is { $size } — too large to open in an editor tab. Schemaic would spend most of its time re-analysing it. Run it from a query tab, or use Import for a data file.
file-too-large-short = The file is too large to open.
file-large-title = Open a large file?
file-large-question = “{ $name }” is { $size }. Schemaic re-analyses the whole document shortly after every pause in typing, so a file this size makes the editor slow to respond for as long as the tab is open. Open it anyway?
file-changed-on-disk = { $path } has changed on disk since it was opened. Saving now would discard those changes — reload the file (or Save As to a different name) instead.
file-lossy-title = Replace unreadable bytes?
# � is the replacement character the editor shows in place of each byte.
file-lossy-question = Schemaic couldn't read every byte of “{ $name }” as text and showed those bytes as “�”. Saving writes what you see, so each of them is replaced permanently — in the whole file, not just the lines you edited. Save anyway?
file-replace-title = Replace file?
# $typed is the name typed in the dialog; $name that name with .sql added.
file-replace-question = “{ $name }” already exists — “{ $typed }” was saved with the .sql extension added. Replace it?
file-reload-title = Reload from disk
file-reload-unsaved = “{ $name }” has unsaved changes. Reloading discards them. Reload anyway?

## The export and dump runners' failures, and a binary cell's load and save
## (main.rs). The dump's own failures from schemaic-cli stay English for now.
## $error is the operating system's, the engine's or the writer's own text.

export-already-running = An export is already running. Cancel it or wait for it to finish.
export-failed = Export failed: { $error }
# The background task writing the file stopped without an answer.
export-worker-died = Export failed: worker died: { $error }
dump-failed = Dump failed: { $error }
# $size is the file's size, $most the cap, both as the app writes a size (64 MB).
blob-load-too-large = That file is { $size } — the most that can be loaded is { $most }.
blob-load-failed = Load failed: { $error }
blob-save-failed = Save failed: { $error }

## Settings › Export / Import settings, Export connections, Import
## Connections' file and Docker sources, and deleting a connection (main.rs).
## $path is a file path; $file a file's name; $error the operating system's
## own text.

file-write-failed = Couldn't write { $path }: { $error }
settings-export-title = Export settings
settings-import-title = Import settings
# The export dialog's file-type filter.
settings-file-filter = Schemaic settings
settings-import-nothing = Nothing to import: every setting in { $path } already matches yours, and it adds no snippets.
# $changed settings would change; $snippets snippets would be added.
settings-import-question = { $path } changes { $changed ->
        [one] { $changed } setting
       *[other] { $changed } settings
    } and adds { $snippets ->
        [one] { $snippets } snippet
       *[other] { $snippets } snippets
    }. The settings it changes replace your current values; the snippets are added beside yours.
# After the file's path and a colon.
settings-import-unreadable = couldn't read it: { $error }
connections-export-title = Export connections
connections-file-filter = Schemaic connections
connections-replace-title = Replace “{ $file }”
connections-replace-question = A file named “{ $file }” is already in that folder. The export adds that ending to the name you typed, so the save dialog did not ask about it. Replacing it can't be undone.
import-choose-file-title = Choose a connections file
import-no-containers = No database containers are running.
import-added = { $count ->
        [one] Added { $count } connection.
       *[other] Added { $count } connections.
    }
# Said when a connection came from another client (DBeaver, DataGrip…).
import-added-foreign = { $count ->
        [one] Added { $count } connection.
       *[other] Added { $count } connections.
    } Read-only and the environment badge are not carried over from another client — set them in Manage connections for anything that matters.
conn-delete-title = Delete “{ $name }”
conn-delete-question = This deletes the connection, its stored password, its saved AI conversation, its query history and its tabs. It can't be undone.
# A notice shown after a delete; its two lines are shown one under the other.
# The ** marks bold.
conn-delete-keyring-kept = Schemaic could not reach the OS keyring, so the deleted connection's stored password and SSH secrets are **still in it**.
conn-delete-keyring-later = They will be removed the next time a connection is deleted or saved while the keyring is reachable. No new connection can be given them in the meantime — connection ids are never reused.

## The rest of main.rs's own words — the inline AI's and the AI fill's
## failures, a Manual tab's connection, snippets, Copy table, Compare schemas,
## the table monitor and the statement timeout. $error is the engine's, the
## agent's or the operating system's own text.

inline-ai-no-sql = No SQL returned
inline-ai-not-sql = The model did not return SQL
# The agent failed and said nothing.
inline-ai-failed = generation failed
ai-fill-no-value = The AI returned no value.
# The connection an action was asked of was deleted meanwhile.
conn-gone = connection no longer exists
tx-session-not-ready = the transaction connection isn't ready — switch to Auto-commit and back
tx-session-open-failed = couldn't open a transaction connection: { $error }
# A new snippet's name when no tab gives it one.
snippet-default-name = Snippet
snippet-delete-title = Delete snippet
snippet-delete-question = Delete “{ $name }”? This can't be undone.
copy-target-read-only = The target connection is read-only, so no rows were copied.
copy-target-gone = The target connection is gone, so no rows were copied.
copy-not-started = No rows were copied: { $error }
copy-counter-not-moved = { $count ->
        [one] { $count } row was copied, but the table's counter was not moved past it, so the next insert may collide: { $error }
       *[other] { $count } rows were copied, but the table's counter was not moved past them, so the next insert may collide: { $error }
    }
copy-stopped = The copy stopped on an error: { $error }
# $database is the side of the comparison that could not be read.
compare-reading-failed = Reading { $database }: { $error }
monitor-no-row-key = No row key for this table — changes can't be tracked.
monitor-restarted = The table's columns changed, so change tracking restarted from here.
# $timeout is the setting's value as its dropdown shows it, lower-cased (15 minutes).
timeout-cancelled-app = Cancelled: the statement ran longer than the { $timeout } statement timeout. Change or turn it off in Settings → Query.
timeout-cancelled-connection = Cancelled: the statement ran longer than the { $timeout } statement timeout. Change or turn it off in this connection's settings.
ai-chat-no-database = Can't reach the database, so the assistant can't be started for this connection. Check the connection and try again.

## The AI session's own notes and refusals (ai.rs, antigravity.rs) — what the
## panel and the inline AI say when an agent CLI cannot start or stops. What
## is sent to the model stays English. $cli is the agent CLI's product name
## (Claude Code, Codex); $bin its program name; $error the CLI's or the
## operating system's own text. ** marks bold.

ai-no-cwd = **The assistant could not start.** Schemaic gives every agent process a private working directory of its own, and neither the system temp directory nor the config directory would take one — so there is nowhere to run it that is not somebody else's directory. Check that the temp directory is writable and try again.
ai-no-reply-file = Couldn't create a temporary file for the reply.
ai-inline-no-opencode-config = Couldn't write OpenCode's configuration, so the generation was not started (without it the CLI would run with its own tools enabled).
ai-inline-no-copilot-home = Couldn't create GitHub Copilot's private home directory, so the generation was not started (without it the CLI would load your own Copilot configuration, including its MCP servers and hooks).
ai-inline-no-env-writer = { $cli } is marked as isolated by its environment, but Schemaic has no way to set that environment up, so the generation was not started.
ai-inline-no-cursor-rules = Couldn't write Cursor's permissions file, so the generation was not started (without it the CLI writes files unprompted).
ai-inline-no-rules-writer = { $cli } is marked as restricted by a permissions file, but Schemaic has no way to write one for it, so the generation was not started.
# $cause is one of the ai-no-tools causes below: a clause, without a full stop.
ai-no-tools = **This session has no database tools.** { $cause }, so the assistant cannot look anything up — it can only answer from the schema outline already in its prompt. Closing and reopening the panel will try again.
ai-no-endpoint-file = Schemaic could not create the private file that tells the assistant how to reach your database
ai-cursor-not-approved = Schemaic could not approve its database tools with Cursor ({ $error })
ai-antigravity-not-registered = Schemaic could not register its database tools with Antigravity
ai-antigravity-shared = Another Schemaic window is already using Antigravity. Antigravity keeps one machine-wide MCP registration, which can point at only one connection, so this window's assistant has no database tools
ai-antigravity-not-claimed = Schemaic could not claim Antigravity's configuration for this session
ai-refused-no-opencode-config = Schemaic could not write the configuration that restricts OpenCode, so the assistant is disabled — running it without that file would give the session a shell. Check that the app's data directory is writable.
ai-refused-no-copilot-home = Schemaic could not create GitHub Copilot's private home directory, so the assistant is disabled — without it the CLI would load your own Copilot configuration, including its MCP servers and hooks. Check that the app's data directory is writable.
ai-refused-no-cursor-rules = Schemaic could not write the permissions file that keeps Cursor from writing files and running commands, so the assistant is disabled — without it Cursor writes files unprompted. Check that the app's data directory is writable.
ai-launch-failed-turn = Couldn't launch the `{ $bin }` CLI ({ $error }). Ensure { $cli } is installed, or give it a path in Settings → AI.
ai-launch-failed-session = Couldn't launch the `{ $bin }` CLI ({ $error }). Ensure { $cli } is installed (or set its path in Settings → AI).
ai-turn-ended = The { $cli } turn ended unexpectedly: { $error }
ai-session-ended-badly = The AI session ended unexpectedly: { $error }
ai-session-ended = The { $cli } session ended. Ask again to start a new one.
ai-session-restart-failed = The { $cli } session could not be restarted after stopping ({ $error }), so it has ended. Ask again to start a new one.
ai-session-not-accepting = The { $cli } session is no longer accepting questions. Ask again to start a new one.

## Import Connections' sources (conn_sources.rs) — a picked file it cannot
## read, and Docker. $path is a file path; $size and $most sizes as the app
## writes one (1.2 MB); $error the operating system's or Docker's own text.

import-source-not-a-file = { $path } is not a file.
import-source-too-big = { $path } is { $size }, past the { $most } this reads.
import-source-unreadable = { $path } could not be read: { $error }
docker-not-found = Docker was not found — neither on PATH nor in WSL.
docker-not-started = Docker could not be started: { $error }
docker-timed-out = Docker did not answer within { $secs } seconds.
docker-not-waited = Docker could not be waited on: { $error }
# $error is the first line Docker wrote to its error output.
docker-answered = Docker answered: { $error }
# $status is the process's exit status as the system words it.
docker-exited = Docker exited with { $status }.

## A script import's reader (script.rs), and Cursor's tool approval (cursor.rs).

# $error is the system's own text.
script-reader-stopped = the file reader stopped unexpectedly: { $error }
script-changed-since-read = This file changed on disk after it was read, so the summary above is not what would run. Choose it again.
# mcp enable is the Cursor CLI command; never translated.
ai-cursor-enable-unfinished = `mcp enable` did not finish
