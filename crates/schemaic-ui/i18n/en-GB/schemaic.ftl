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
